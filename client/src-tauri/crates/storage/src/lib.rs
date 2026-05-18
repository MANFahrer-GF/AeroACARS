//! Local persistence layer.
//!
//! Phase K scope — offline queue for ACARS position posts. When the
//! phpVMS network call fails (no internet, server hiccup, rate-limit),
//! the streamer enqueues the position into a file-based queue. The next
//! successful tick drains it before the new post, so phpVMS sees the
//! correct chronological order even after a network gap.
//!
//! File-based instead of SQLite for now: a flight is unlikely to queue
//! more than a few hundred rows in a real-world outage, and a JSON file
//! keeps the dependency surface minimal. SQLite remains an option in
//! the workspace if we ever need indexed queries (flight log, settings
//! cache, analytics) — see requirements spec §26.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const QUEUE_FILE: &str = "position_queue.json";
/// v0.7.19 GAF-707: persistente Cleanup-Queue fuer den Fall
/// "FILED ok, delete_bid hat transient gefailed". Spec §Pending Bid
/// Cleanup Queue. Eintrag wird durch den Background-Worker mit
/// `delete_bid(bid_id, flight_id)` retried; erfolgreich → loeschen.
const PENDING_BID_CLEANUP_FILE: &str = "pending_bid_cleanup.json";
/// Cap on retained queued positions. Past this point the oldest are
/// dropped — a multi-hour outage isn't worth blocking forever or
/// blowing up the file. ~1000 rows ≈ 3 h of cruise-cadence (30 s) or
/// ~1.5 h of ground/approach (10 s).
const QUEUE_MAX_ROWS: usize = 1000;

/// File name for the landing-history store inside the app data dir.
const LANDINGS_FILE: &str = "landings.json";
/// Cap on retained landing records. ~500 landings ≈ several years of
/// active virtual-airline flying for one pilot. Past this we drop the
/// oldest so the file can't grow unbounded.
const LANDINGS_MAX_ROWS: usize = 500;

/// One pending position post, ready to be replayed once connectivity
/// returns. The serialised `position` is opaque from the storage crate's
/// point of view — it's just whatever JSON the API client wants to
/// `POST /api/pireps/{pirep_id}/acars/position` with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedPosition {
    /// PIREP the position belongs to. The streamer uses this to route
    /// drained rows to the right phpVMS endpoint, and the queue
    /// implicitly partitions by PIREP so an old flight's leftovers
    /// don't bleed into a new one (they're discarded by `pirep_id`
    /// mismatch on drain).
    pub pirep_id: String,
    /// Serialized `PositionEntry` JSON. We don't import api-client into
    /// the storage crate to avoid a circular dependency.
    pub position: serde_json::Value,
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// File-backed FIFO queue of pending position posts. Cheap to read /
/// write — entire file is loaded, mutated, and rewritten atomically.
/// At ~1 KB per row × 1000 rows = ~1 MB worst case, that's fine.
pub struct PositionQueue {
    path: PathBuf,
}

impl PositionQueue {
    /// Open (or implicitly create) the queue file in the given app
    /// data directory. The file itself is only written when something
    /// is actually enqueued.
    pub fn open(app_data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let dir = app_data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join(QUEUE_FILE),
        })
    }

    /// Append a position to the queue. Drops the oldest entry when the
    /// queue is at capacity so a long outage can't unbounded-grow the
    /// file. Returns the new queue length on success.
    pub fn enqueue(&self, item: QueuedPosition) -> Result<usize, StorageError> {
        let mut items = self.read_all()?;
        items.push(item);
        if items.len() > QUEUE_MAX_ROWS {
            // Drop oldest until we're back at the cap.
            let drop_count = items.len() - QUEUE_MAX_ROWS;
            items.drain(0..drop_count);
            tracing::warn!(
                drop_count,
                kept = items.len(),
                "position queue at capacity — dropped oldest rows"
            );
        }
        self.write_all(&items)?;
        Ok(items.len())
    }

    /// Read every queued row without modifying the file. Used to
    /// inspect (e.g. show the user a count in the UI) and to drain.
    pub fn read_all(&self) -> Result<Vec<QueuedPosition>, StorageError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Write the given list as the new full queue contents. Atomic via
    /// write-then-rename so a crash in the middle leaves the previous
    /// file intact.
    fn write_all(&self, items: &[QueuedPosition]) -> Result<(), StorageError> {
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(items)?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// Replace the on-disk queue with the supplied list. Used by the
    /// streamer's drain logic: read all → try posting each → write
    /// back the rows that still failed.
    pub fn replace(&self, items: &[QueuedPosition]) -> Result<(), StorageError> {
        if items.is_empty() {
            // No need to keep an empty file around — remove it so the
            // next read returns an empty Vec without I/O.
            if self.path.exists() {
                let _ = std::fs::remove_file(&self.path);
            }
            return Ok(());
        }
        self.write_all(items)
    }

    /// Current queue length (cheap — reads the file once). Useful for
    /// surfacing "X positions queued offline" in the dashboard.
    pub fn len(&self) -> Result<usize, StorageError> {
        Ok(self.read_all()?.len())
    }

    /// Drop every queued row regardless of PIREP. Called on
    /// `flight_cancel` / `flight_forget` to avoid replaying positions
    /// for a flight the user discarded.
    pub fn clear(&self) -> Result<(), StorageError> {
        if self.path.exists() {
            std::fs::remove_file(&self.path)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Landing history store
// ---------------------------------------------------------------------------
//
// The Landing tab in the desktop UI shows a per-flight breakdown of every
// touchdown: score / letter grade, V/S, peak G, runway match, sideslip,
// approach stability, rollout distance, and the SimBrief plan-vs-actual
// fuel comparison. Persisted as a JSON array on disk so a Tauri restart
// doesn't lose history; same atomic-write pattern as the position queue.
//
// One record per filed PIREP — we don't store in-progress flights here.
// Records are immutable once written.

/// Single subsample of the touchdown profile around the on-ground edge.
/// Mirrors `TouchdownProfilePoint` in the main crate; duplicated here
/// to keep storage independent from sim/api crates.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingProfilePoint {
    /// Milliseconds relative to the touchdown edge (negative = before).
    pub t_ms: i32,
    pub vs_fpm: f32,
    pub g_force: f32,
    pub agl_ft: f32,
    pub on_ground: bool,
    pub heading_true_deg: f32,
    pub groundspeed_kt: f32,
    pub indicated_airspeed_kt: f32,
    pub pitch_deg: f32,
    pub bank_deg: f32,
}

/// Runway-correlation result (mirrors `runway::RunwayMatch`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingRunwayMatch {
    pub airport_ident: String,
    pub runway_ident: String,
    pub surface: String,
    pub length_ft: f64,
    pub centerline_distance_m: f64,
    pub centerline_distance_abs_ft: f64,
    pub side: String,
    pub touchdown_distance_from_threshold_ft: f64,

    // ─── v0.8.0 navdata extension ────────────────────────────────────
    // All fields Option + #[serde(default)] for backward-compat:
    // pre-v0.8.0 landing_history.json entries deserialize cleanly with
    // these as None. Populated only when the match came from a
    // VPS-loaded NavAirport (source = "navigraph").
    /// "navigraph" | "ourairports_fallback". `None` for pre-v0.8.0 records.
    #[serde(default)]
    pub source: Option<String>,
    /// AIRAC-Cycle the navigraph match was resolved against (e.g. "2604").
    /// `None` for `ourairports_fallback` or pre-v0.8.0 records.
    #[serde(default)]
    pub nav_cycle: Option<String>,
    /// Geographic true-course of the landing direction. Needed by the
    /// RunwayDiagram for the visual axis + by wind-vs-runway recompute.
    #[serde(default)]
    pub true_course_deg: Option<f64>,
    /// Displaced-threshold distance in feet (0 when the painted
    /// threshold = landing threshold).
    #[serde(default)]
    pub displaced_threshold_ft: Option<i32>,
    /// Threshold Crossing Height the pilot was supposed to be at.
    #[serde(default)]
    pub tch_expected_ft: Option<i32>,
    /// Published glideslope angle in degrees (typical 3.0).
    #[serde(default)]
    pub glideslope_angle_deg: Option<f64>,
}

/// One landing record — written once when the PIREP is filed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingRecord {
    /// PIREP id (phpVMS UUID). Doubles as the record's primary key.
    pub pirep_id: String,
    /// When the touchdown occurred (UTC).
    pub touchdown_at: chrono::DateTime<chrono::Utc>,
    /// When the record was written (= PIREP file time).
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    /// Callsign / flight number (display only).
    pub flight_number: String,
    pub airline_icao: String,
    pub dpt_airport: String,
    /// Geplante Destination aus dem Bid. Bei Divert / Off-airport-Crash
    /// **nicht** der tatsaechliche Landeplatz — siehe v0.7.18 (B-012)
    /// `touchdown_airport_*` Felder weiter unten.
    pub arr_airport: String,
    /// v0.7.18 (B-012): aufgelöster tatsächlicher Touchdown-Airport.
    /// - Wenn runway_match zur Runway korreliert wurde: dessen ICAO.
    /// - Sonst der nächste Airport innerhalb 25 nmi.
    /// - Sonst fallback auf arr_airport.
    /// None wenn Pre-v0.7.18-Record (Backwards-Compat).
    #[serde(default)]
    pub touchdown_airport: Option<String>,
    /// Aufloesungs-Quelle: "runway_match" / "nearest_25nm" / "planned_fallback".
    #[serde(default)]
    pub touchdown_airport_source: Option<String>,
    /// Distanz vom TD-Punkt zur geplanten Destination (nmi).
    #[serde(default)]
    pub touchdown_distance_to_destination_nm: Option<f32>,
    /// Distanz vom TD-Punkt zum nearest Airport (nmi), nur bei
    /// `nearest_25nm`-Source gesetzt.
    #[serde(default)]
    pub touchdown_nearest_distance_nm: Option<f32>,
    pub aircraft_registration: Option<String>,
    pub aircraft_icao: Option<String>,
    pub aircraft_title: Option<String>,
    /// Sim that produced the flight ("MSFS" | "X-PLANE").
    pub sim_kind: Option<String>,

    // Score
    pub score_numeric: i32,
    pub score_label: String,
    pub grade_letter: String,

    // Touchdown vitals
    pub landing_rate_fpm: f32,
    pub landing_peak_vs_fpm: Option<f32>,
    pub landing_g_force: Option<f32>,
    pub landing_peak_g_force: Option<f32>,
    pub landing_pitch_deg: Option<f32>,
    pub landing_bank_deg: Option<f32>,
    pub landing_speed_kt: Option<f32>,
    pub landing_heading_deg: Option<f32>,
    pub landing_weight_kg: Option<f64>,
    pub touchdown_sideslip_deg: Option<f32>,
    pub bounce_count: u8,

    // Wind at touchdown
    pub headwind_kt: Option<f32>,
    pub crosswind_kt: Option<f32>,

    // Approach stability + rollout (Stage 1)
    pub approach_vs_stddev_fpm: Option<f32>,
    pub approach_bank_stddev_deg: Option<f32>,
    pub rollout_distance_m: Option<f64>,

    // SimBrief plan vs actual (Stage 2)
    pub planned_block_fuel_kg: Option<f32>,
    pub planned_burn_kg: Option<f32>,
    pub planned_tow_kg: Option<f32>,
    pub planned_ldw_kg: Option<f32>,
    pub planned_zfw_kg: Option<f32>,
    pub actual_trip_burn_kg: Option<f32>,
    pub fuel_efficiency_kg_diff: Option<f32>,
    pub fuel_efficiency_pct: Option<f32>,
    pub takeoff_weight_kg: Option<f64>,
    pub takeoff_fuel_kg: Option<f32>,
    pub landing_fuel_kg: Option<f32>,
    pub block_fuel_kg: Option<f32>,

    // Runway
    pub runway_match: Option<LandingRunwayMatch>,

    // Touchdown profile (V/S + G curve, ~150 samples)
    #[serde(default)]
    pub touchdown_profile: Vec<LandingProfilePoint>,

    /// Rolling buffer of (V/S, bank) samples captured during the
    /// Approach + Final phases. ~120 entries at 5-8 s cadence.
    /// Drives the approach-stability time-series chart in the Landing
    /// tab. Indexed left-to-right oldest-to-newest.
    #[serde(default)]
    pub approach_samples: Vec<ApproachSample>,

    // ─── v0.5.43 50-Hz-TouchdownWindow Forensik ──────────────────────
    //
    // Aus dem Sampler-Buffer (5 s pre + 10 s post @ 50 Hz) berechnete
    // Werte. Werden vom build_landing_record aus stats.landing_analysis
    // gelesen wenn der Buffer-Dump erfolgreich war (sonst alle None).
    // Ergaenzen die bestehenden Touchdown-Vitals um die Volanta-/DLHv-
    // equivalenten Mehrfach-Fenster + Flare-Quality-Metriken.
    //
    // Backwards-compatible: alle Felder Optional + serde(default) damit
    // alte landing_history.json-Eintraege ohne Forensik weiter parsen.
    /// V/S linear interpoliert auf den exakten on_ground-Edge zwischen
    /// zwei 20-30 ms Samples. Volanta-equivalent.
    #[serde(default)]
    pub vs_at_edge_fpm: Option<f32>,
    /// Mean V/S ueber 250 ms vor Edge (nur negative Samples).
    #[serde(default)]
    pub vs_smoothed_250ms_fpm: Option<f32>,
    /// Mean V/S ueber 500 ms vor Edge (= Volanta-Display-Wert).
    #[serde(default)]
    pub vs_smoothed_500ms_fpm: Option<f32>,
    /// Mean V/S ueber 1000 ms vor Edge (= DLHv-Display-Wert).
    #[serde(default)]
    pub vs_smoothed_1000ms_fpm: Option<f32>,
    /// Mean V/S ueber 1500 ms vor Edge.
    #[serde(default)]
    pub vs_smoothed_1500ms_fpm: Option<f32>,
    /// Peak G im 500 ms post-Edge — der echte Gear-Compression-Spike.
    #[serde(default)]
    pub peak_g_post_500ms: Option<f32>,
    /// Peak G im 1000 ms post-Edge.
    #[serde(default)]
    pub peak_g_post_1000ms: Option<f32>,
    /// v0.7.17 (B-009): G-Force-Forensik (analog vs_smoothed_*).
    /// G im Edge-Frame (interpoliert) — was der Pilot beim Aufsetzen
    /// gefuehlt hat, bevor die Strut komprimiert.
    #[serde(default)]
    pub g_at_edge: Option<f32>,
    /// Mean G ueber 0..250 ms post-Edge — was Volanta/Cockpit zeigt.
    #[serde(default)]
    pub g_smoothed_250ms_post: Option<f32>,
    /// Median G ueber 0..500 ms post-Edge — robust gg Sim-Strut-Spikes.
    #[serde(default)]
    pub g_median_post_500ms: Option<f32>,
    /// 95th-Percentile G ueber 0..500 ms — verschluckt 5 % Spikes.
    #[serde(default)]
    pub g_p95_post_500ms: Option<f32>,
    /// Max Fahrwerks-Normalkraft in N — Strut-Compression-Mass fuer
    /// die Aufklaerung in der G-Forensik-UI (Erklaer-Tile).
    #[serde(default)]
    pub max_gear_force_n: Option<f32>,
    /// Steepste Sinkrate in [-2000, -100] ms vor Edge.
    #[serde(default)]
    pub peak_vs_pre_flare_fpm: Option<f32>,
    /// V/S unmittelbar vor Edge.
    #[serde(default)]
    pub vs_at_flare_end_fpm: Option<f32>,
    /// Reduktion durch Flare (positiv = Sinkrate verkleinert).
    #[serde(default)]
    pub flare_reduction_fpm: Option<f32>,
    /// dV/S/dt im Flare-Window in fpm/sec.
    #[serde(default)]
    pub flare_dvs_dt_fpm_per_sec: Option<f32>,
    /// Flare-Quality-Score 0..100 (Endpoint + Reduktions-Bonus).
    #[serde(default)]
    pub flare_quality_score: Option<i32>,
    /// True wenn signifikante VS-Reduktion (>50 fpm) im Flare-Window.
    #[serde(default)]
    pub flare_detected: Option<bool>,
    /// Sample-Count im 50-Hz-Buffer (>500 = OK, <100 = ggf. Sample-Loch).
    #[serde(default)]
    pub forensic_sample_count: Option<u32>,

    // ─── v0.8.3 (#8) — Forensische Bounce-Counts surface ─────────────
    /// Hoechster gemessener AGL-Wert in den post-TD-Excursions, ft.
    /// Aus `touchdown_v2::compute_landing_rate`. None wenn kein Hopser
    /// erkannt oder Sampler-Buffer unvollstaendig. Quelle fuer das
    /// „Hopser X ft erkannt"-Label im UI bei score-freien Bounces.
    #[serde(default)]
    pub bounce_max_agl_ft: Option<f32>,
    /// Anzahl forensisch erkannter Hopser (>= 5 ft AGL). Subset:
    /// `forensic_bounce_count >= scored_bounce_count`. Wenn
    /// `bounce_count = 0` aber `forensic_bounce_count > 0`: rein
    /// score-freie Hopser — UI zeigt dezenten „Light bounce"-Hinweis.
    #[serde(default)]
    pub forensic_bounce_count: Option<u8>,
    /// Anzahl score-relevanter Hopser (>= 15 ft AGL). Was in
    /// `bounce_count` und den Landing-Score einfliesst.
    #[serde(default)]
    pub scored_bounce_count: Option<u8>,

    // ─── v0.7.1 Erweiterung (Spec §5.1 + §5.4) ───────────────────────
    // Alle Felder mit #[serde(default)] — alte landing_history.json-
    // Eintraege ohne diese Felder bleiben deserialisierbar.

    /// UX-Cutoff-Marker. 0 = pre-v0.7.1, 1 = v0.7.1+ (sub_scores
    /// vorhanden, Asymmetrie-Logik aktiv). UI nutzt den Marker fuer
    /// §3.5 Legacy-Schutz: bei `< 1` wird `LegacyPirepNotice` gezeigt
    /// statt Sub-Score-Breakdown.
    #[serde(default)]
    pub ux_version: u8,

    /// Touchdown-Forensik-Version (1 = legacy, 2 = touchdown_v2).
    /// v0.7.1 P2.4-Fix: explizit im Record persistieren statt UI zu
    /// zwingen den Wert zu raten. ForensicsBadge nutzt diesen Wert
    /// + ux_version >= 1 als Bedingung.
    #[serde(default)]
    pub forensics_version: u8,

    // F4: Forensik-Sichtbarkeit
    #[serde(default)]
    pub landing_confidence: Option<String>,
    #[serde(default)]
    pub landing_source: Option<String>,

    // F7: Stability-v2-Felder (P2.1-A: bestehende Backend-Felder
    // exponieren, keine neue Berechnung)
    #[serde(default)]
    pub approach_vs_jerk_fpm: Option<f32>,
    #[serde(default)]
    pub approach_ias_stddev_kt: Option<f32>,
    #[serde(default)]
    pub approach_stable_config: Option<bool>,
    #[serde(default)]
    pub approach_excessive_sink: Option<bool>,
    /// Stability-Gate-Window-Metadaten (welche Sample-Region wurde
    /// bewertet). Werte aus landing-scoring/src/gate.rs Konstanten.
    #[serde(default)]
    pub gate_window: Option<GateWindow>,

    /// Sub-Score-Breakdown aus der landing-scoring Crate (Spec §3.1
    /// SSoT). UI liest diese Werte direkt — KEIN Recompute. Bei alten
    /// PIREPs (ux_version < 1) ist der Vec leer; UI zeigt dann
    /// LegacyPirepNotice statt Breakdown.
    #[serde(default)]
    pub sub_scores: Vec<landing_scoring::SubScoreEntry>,

    // ─── v0.7.6 P1-3: Runway-Geometry-Trust ──────────────────────────
    // Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-3.
    //
    // Bei trusted=Some(false) blendet das LandingPanel die Touchdown-Zone
    // und Float-Distance-Tiles aus und zeigt einen Hinweis-Pill mit dem
    // reason. Rollout-Sub-Score bleibt valide (kommt aus GPS-Track).
    //
    // Backward-Compat: alte v0.7.5-PIREPs ohne diese Felder bleiben
    // deserialisierbar (serde(default)). Frontend behandelt None wie
    // trusted=true (= Verhalten vor v0.7.6).
    /// Ist die Runway-Geometrie plausibel? Definitionen + Reasons siehe
    /// `runway_geometry_trust_check` in `aeroacars_app::lib`.
    #[serde(default)]
    pub runway_geometry_trusted: Option<bool>,
    /// "no_runway_match" / "icao_mismatch" / "centerline_offset_too_large"
    /// / "negative_float_distance"
    #[serde(default)]
    pub runway_geometry_reason: Option<String>,

    // ─── v0.8.0 VPS-Navdata + Runway-Awareness ───────────────────────
    //
    // Spec docs/spec/v0.8.0-vps-navdata-runway-awareness.md. Alle Felder
    // optional + serde(default) damit pre-v0.8.0-Records weiter laden.
    // Werden vom Streamer-Tick populiert wenn ActiveFlight.navdata Some
    // ist (= VPS-Daten geladen). Bei OurAirports-Fallback bleiben TDZ /
    // Aim / TCH / DDS None (= Pills zeigen "n/a", LandingPanel skippt
    // sie).

    /// Signed along-track distance from the landing threshold to the
    /// touchdown point, in meters. Positive = past threshold, negative
    /// = undershoot. Source-agnostic — present for both navigraph and
    /// fallback matches when a runway was correlated.
    #[serde(default)]
    pub td_distance_from_threshold_m: Option<f64>,
    /// F3 TDZ-Result: true when the touchdown sits inside the painted
    /// TDZ marker (0..900 m or 0..length/3, whichever is shorter).
    /// `None` when the runway is too short for TDZ markings (< 1200 m).
    #[serde(default)]
    pub td_in_tdz: Option<bool>,
    /// 1-indexed third of the runway the touchdown lies in (1/2/3).
    #[serde(default)]
    pub td_third: Option<u8>,
    /// F3 TDZ-Marker-Länge in Metern (≤ 900, ≤ length/3). UI braucht
    /// den Wert um die TDZ-Box im RunwayDiagram zu rendern ohne die
    /// `min(900, length/3)`-Logik im Frontend zu duplizieren.
    #[serde(default)]
    pub td_tdz_length_m: Option<f64>,
    /// F4 Aim-Point delta in meters (positive = past aim, negative = short).
    #[serde(default)]
    pub aim_delta_m: Option<f64>,
    /// F4 Aim-Point classification: "perfect" | "short_of_aim" |
    /// "past_aim" | "long_landing" | "severe".
    #[serde(default)]
    pub aim_class: Option<String>,
    /// F4 Aim-Point-Distanz in Metern (300 m für kurze Bahnen, 400 m
    /// für lange ≥ 2400 m / 7874 ft). UI rendert daraus den Aim-Marker.
    #[serde(default)]
    pub aim_point_m: Option<f64>,
    /// F5 actual TCH measured at threshold-crossing (AGL ft). Captured
    /// by the streamer-tick from the 50 Hz buffer when available.
    #[serde(default)]
    pub tch_actual_ft: Option<f64>,
    /// F5 TCH delta = actual - expected (ft). Sign-convention: positive
    /// = above profile, negative = below.
    #[serde(default)]
    pub tch_delta_ft: Option<f64>,
    /// F5 TCH classification: "on_profile" | "slightly_low" |
    /// "slightly_high" | "high" | "below_profile".
    #[serde(default)]
    pub tch_class: Option<String>,
    /// F6 Displaced-Threshold-Warning: pilot touched down in the
    /// pre-threshold paint zone (illegal). Only true when the runway
    /// has a non-zero displaced_threshold_ft AND the touchdown sits
    /// between the painted start and the landing threshold.
    #[serde(default)]
    pub pre_displaced_threshold: Option<bool>,

    // ─── v0.7.19 GAF-707 Accident-Detection ───────────────────────────
    //
    // Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md. Alle
    // Felder `#[serde(default)]` damit pre-v0.7.19-Records weiter laden.
    // `landing-scoring` bleibt unangetastet — Score-Werte oben sind
    // orthogonal zur Accident-Klassifikation.
    /// True wenn der Touchdown als Accident klassifiziert wurde (Confirmed
    /// per Spec). Suspected wird hier NICHT als true gespeichert — die
    /// Frontend-Logik liest `accident_confidence` fuer die Banner-Variante.
    #[serde(default)]
    pub accident: bool,
    /// "sim_crash" | "impact" | "off_airport_impact". None wenn kein Accident.
    #[serde(default)]
    pub accident_kind: Option<String>,
    /// "high" | "medium". `high` = Confirmed, `medium` = Suspected.
    /// None wenn kein Accident-/Verdachts-Signal.
    #[serde(default)]
    pub accident_confidence: Option<String>,
    /// Begruendungs-Strings (free-form, lesbar fuer Notes/UI), z. B.
    /// `["vs_at_edge_fpm=-2249.9", "peak_g_load=4.41", "no_runway_match"]`.
    #[serde(default)]
    pub accident_reasons: Vec<String>,
    /// Wann der Accident detektiert wurde. Bei Sim-Event-Pfad kann das
    /// mehrere Sekunden vor `touchdown_at` liegen (mid-air Crash). Bei
    /// Heuristik-Pfad gleich `touchdown_at`. None wenn kein Accident.
    #[serde(default)]
    pub accident_at: Option<chrono::DateTime<chrono::Utc>>,

    /// v0.10.0 (#runway-utilization-score) — Algorithmus-Version des
    /// `sub_scores`-Arrays. None/Some(1) = pre-v0.10 (meter-only Bahn-
    /// Auslastung); Some(2) = v0.10 (LDA-basierter Runway-Utilization-
    /// Score). UI nutzt diesen Marker um zu entscheiden ob die neuen
    /// Felder (`extra`, neue Rationale-Keys, neue Warning-Werte)
    /// gerendert werden. Spec docs/spec/v0.10.0-runway-utilization-
    /// score.md LE11. Backward-compat: alte landing_history.json-
    /// Eintraege ohne diese Feld bleiben deserialisierbar (None).
    #[serde(default)]
    pub score_algorithm_version: Option<u8>,
}

/// v0.7.1: Stability-Gate-Window-Metadaten (Spec §5.4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GateWindow {
    pub start_at_ms: i64,
    pub end_at_ms: i64,
    pub start_height_ft: f32,
    pub end_height_ft: f32,
    pub sample_count: u32,
}

/// One (V/S, bank) sample taken during Approach/Final, used by the
/// approach-stability chart.
///
/// v0.7.1 (P1.1-D + P1.3-C): erweitert um Zeit/Hoehe/Flags damit der
/// Approach-Chart Vorlauf/Gate/Flare-Zonen rendern kann. Alle neuen
/// Felder optional + #[serde(default)] fuer Backward-Compat: alte
/// landing_history.json-Eintraege ohne diese Felder lesen sie als None
/// und der Chart faellt auf den Index-basierten Plot zurueck (kein Crash).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApproachSample {
    pub vs_fpm: f32,
    pub bank_deg: f32,
    /// ms relativ zum Touchdown (negativ = vor TD)
    #[serde(default)]
    pub t_ms: Option<i32>,
    /// AGL ODER HAT in ft zum Sample-Zeitpunkt
    #[serde(default)]
    pub agl_ft: Option<f32>,
    /// True wenn das Sample im Stability-Gate liegt
    /// (`MIN_HEIGHT < height <= MAX_HEIGHT` UND nicht in den letzten
    /// `FLARE_CUTOFF_MS` vor TD).
    #[serde(default)]
    pub is_scored_gate: Option<bool>,
    /// True wenn das Sample in den letzten `FLARE_CUTOFF_MS` vor TD
    /// liegt (zeitbasiert, Werte aus landing_scoring::gate).
    #[serde(default)]
    pub is_flare: Option<bool>,
}

/// File-backed JSON store of past landings.
pub struct LandingStore {
    path: PathBuf,
}

// ---------------------------------------------------------------------------
// v0.7.19 GAF-707: Pending bid cleanup queue
// ---------------------------------------------------------------------------
//
// Hintergrund (Spec §Pending Bid Cleanup Queue): wenn `file_pirep`
// erfolgreich war, `delete_bid` danach aber wegen Netzwerk/5xx/Timeout
// scheitert, darf der Pilot nicht mit einem filed PIREP und haengendem
// Bid zurueckbleiben — sonst sieht phpVMS das Aircraft weiterhin als
// reserviert. Diese Queue persistiert solche Faelle und der
// Background-Worker (in lib.rs) retried `delete_bid` im naechsten
// passenden Moment.

/// Ein einzelner pending Cleanup-Eintrag. Persistiert als JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingBidCleanup {
    /// PIREP der schon erfolgreich geFILED wurde — nur fuer Log/Diagnose,
    /// der Worker macht KEIN cancel_pirep darauf.
    pub pirep_id: String,
    /// Numerischer Bid-Identifier (wenn phpVMS einen liefert).
    #[serde(default)]
    pub bid_id: Option<i64>,
    /// String-Flight-ID — Fallback fuer VAs ohne separates bid_id-Feld.
    #[serde(default)]
    pub flight_id: Option<String>,
    /// Warum landete der Eintrag in der Queue. "accident_filed" |
    /// "hard_landing_override" | "normal_filed" | spaeter ggf. weitere.
    pub reason: String,
    /// Erstellungs-Zeitpunkt.
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Letzter Retry-Zeitpunkt (zum Throttling).
    #[serde(default)]
    pub last_attempt_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Anzahl bisheriger Retries.
    #[serde(default)]
    pub attempts: u32,
}

/// File-backed Cleanup-Queue. Klein (in der Praxis selten >1 Eintrag),
/// keine Cap-Logik — Background-Worker raeumt selber wenn `delete_bid`
/// erfolgreich war.
pub struct PendingBidCleanupQueue {
    path: PathBuf,
}

impl PendingBidCleanupQueue {
    pub fn open(app_data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let dir = app_data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join(PENDING_BID_CLEANUP_FILE),
        })
    }

    /// Eintrag anhaengen. Dedupliziert auf pirep_id+bid_id+flight_id —
    /// wenn ein identischer Eintrag schon drinsteht, wird nicht
    /// nochmal erstellt (Idempotenz).
    pub fn enqueue(
        &self,
        item: PendingBidCleanup,
    ) -> Result<usize, StorageError> {
        let mut items = self.read_all()?;
        let already_present = items.iter().any(|existing| {
            existing.pirep_id == item.pirep_id
                && existing.bid_id == item.bid_id
                && existing.flight_id == item.flight_id
        });
        if !already_present {
            items.push(item);
            self.write_all(&items)?;
        }
        Ok(items.len())
    }

    pub fn read_all(&self) -> Result<Vec<PendingBidCleanup>, StorageError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        match serde_json::from_slice(&bytes) {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::warn!(error = %e, "pending_bid_cleanup.json unreadable — starting fresh");
                Ok(Vec::new())
            }
        }
    }

    /// Ersetzen aller Eintraege (= nach Retry-Pass). Leere Liste loescht
    /// die Datei.
    pub fn replace(
        &self,
        items: &[PendingBidCleanup],
    ) -> Result<(), StorageError> {
        if items.is_empty() {
            if self.path.exists() {
                let _ = std::fs::remove_file(&self.path);
            }
            return Ok(());
        }
        self.write_all(items)
    }

    /// Eintrag per pirep_id loeschen. Used vom Worker nach erfolgreichem
    /// `delete_bid`.
    pub fn remove(&self, pirep_id: &str) -> Result<(), StorageError> {
        let mut items = self.read_all()?;
        let before = items.len();
        items.retain(|e| e.pirep_id != pirep_id);
        if items.len() != before {
            self.replace(&items)?;
        }
        Ok(())
    }

    fn write_all(&self, items: &[PendingBidCleanup]) -> Result<(), StorageError> {
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(items)?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

impl LandingStore {
    /// Open (or implicitly create) the landings file in the app data dir.
    pub fn open(app_data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let dir = app_data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join(LANDINGS_FILE),
        })
    }

    /// Append (or replace by `pirep_id`) a record. We dedupe so a
    /// re-filed PIREP doesn't show up twice; the last write wins.
    pub fn upsert(&self, record: LandingRecord) -> Result<(), StorageError> {
        let mut items = self.read_all()?;
        if let Some(pos) = items.iter().position(|r| r.pirep_id == record.pirep_id) {
            items[pos] = record;
        } else {
            items.push(record);
        }
        if items.len() > LANDINGS_MAX_ROWS {
            let drop_count = items.len() - LANDINGS_MAX_ROWS;
            items.drain(0..drop_count);
        }
        self.write_all(&items)
    }

    /// Read all records, newest first.
    pub fn list(&self) -> Result<Vec<LandingRecord>, StorageError> {
        let mut items = self.read_all()?;
        items.sort_by(|a, b| b.touchdown_at.cmp(&a.touchdown_at));
        Ok(items)
    }

    /// Look up one record by PIREP id.
    pub fn get(&self, pirep_id: &str) -> Result<Option<LandingRecord>, StorageError> {
        Ok(self
            .read_all()?
            .into_iter()
            .find(|r| r.pirep_id == pirep_id))
    }

    /// Replace the on-disk list with the given vector. Used by the
    /// Landing tab's delete-record flow.
    pub fn replace_all(&self, items: &[LandingRecord]) -> Result<(), StorageError> {
        if items.is_empty() {
            if self.path.exists() {
                let _ = std::fs::remove_file(&self.path);
            }
            return Ok(());
        }
        self.write_all(items)
    }

    fn read_all(&self) -> Result<Vec<LandingRecord>, StorageError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes = std::fs::read(&self.path)?;
        if bytes.is_empty() {
            return Ok(Vec::new());
        }
        // Tolerate corruption / older shape: empty list is better than
        // taking down the whole UI.
        match serde_json::from_slice(&bytes) {
            Ok(v) => Ok(v),
            Err(e) => {
                tracing::warn!(error = %e, "landings.json unreadable — starting fresh");
                Ok(Vec::new())
            }
        }
    }

    fn write_all(&self, items: &[LandingRecord]) -> Result<(), StorageError> {
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(items)?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
