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

/// v0.20.x QS fix: tombstone list for `landing_delete`. See
/// [`DeletedLandingsTombstone`].
const DELETED_LANDINGS_FILE: &str = "deleted_landings.json";
/// Cap on retained tombstones — bounds the file for a pilot who deletes a
/// lot of landings over years without needing real GC.
const DELETED_LANDINGS_MAX_ROWS: usize = 500;

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
    /// v0.12.3 (LE4/LE7): EMA-geglätteter gescorter G-Wert (FOQA-Methode)
    /// — der Wert, auf dem die Landung gescort wird und den die
    /// G-Force-Card als Headline zeigt. `peak_g_post_*` bleibt der rohe
    /// Forensik-Peak.
    #[serde(default)]
    pub landing_scored_g_force: Option<f32>,
    /// v0.12.3 (LE8): `"ema_max"` | `"raw_fallback"` — wie
    /// `landing_scored_g_force` abgeleitet wurde.
    #[serde(default)]
    pub scored_g_method: Option<String>,
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

    // v0.11.0-dev: Approach-Stability-Card im LandingPanel zeigt 7 Kacheln
    // analog zur Webapp. 5 Werte sind oben bereits persistiert; diese 3
    // hier waren bisher nur im MQTT-Payload, im lokalen LandingRecord aber
    // nicht. Backend (lib.rs:14441) füllt die Werte schon — wir reichen
    // sie nur ins Storage durch. Alte landing_history.json bleibt mit
    // serde(default) lesbar.
    /// Mean |V/S − target_vs_for_3deg_ils|, fpm, über das Stability-Gate.
    #[serde(default)]
    pub approach_vs_deviation_fpm: Option<f32>,
    /// Max |V/S − target_vs_for_3deg_ils|, fpm, für Samples unter 500 ft HAT.
    #[serde(default)]
    pub approach_max_vs_deviation_below_500_fpm: Option<f32>,
    /// True wenn das Gate-Window auf Height-Above-Touchdown gefiltert wurde
    /// (Airport-Elevation bekannt). False = AGL-Fallback. Wichtig für die
    /// Quellen-Zeile in der Approach-Stability-Card.
    #[serde(default)]
    pub approach_used_hat: Option<bool>,

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
    /// Score); Some(3) = v0.12.0 (Float-Toleranz-Refinement); Some(4) =
    /// v0.16.21 (MSFS touchdown V/S SimVar-lag corrected); Some(5) =
    /// v0.20.x (Bahnauslastung-QS: Float-Toleranz 15→20 % LDA, Banding
    /// 30/50/70/90 → 40/60/80/95; Sinkraten-Score von monotoner Kurve
    /// auf Ziel-Korridor 90-250 fpm umgestellt). UI nutzt diesen Marker
    /// um zu entscheiden ob die neuen Felder (`extra`, neue Rationale-
    /// Keys, neue Warning-Werte) gerendert werden. Spec docs/spec/
    /// v0.10.0-runway-utilization-score.md LE11. Backward-compat: alte
    /// landing_history.json-Eintraege ohne diese Feld bleiben
    /// deserialisierbar (None).
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
                // v0.20.x QS fix: same bug `LandingStore::read_all` was
                // fixed against this release — an unparseable file used to
                // be treated as silently empty with nothing preserved, so
                // the very next `enqueue`/`replace` would persist that
                // empty state over the original (possibly only partially
                // damaged) bytes. Best-effort: copy the original bytes
                // aside before falling back to empty.
                let backup_path = self.path.with_extension(format!(
                    "json.corrupt-{}",
                    chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
                ));
                if let Err(backup_err) = std::fs::write(&backup_path, &bytes) {
                    tracing::error!(
                        error = %e,
                        backup_error = %backup_err,
                        "pending_bid_cleanup.json unreadable AND could not back up the original bytes — starting fresh with NO recovery copy"
                    );
                } else {
                    tracing::warn!(
                        error = %e,
                        backup_path = %backup_path.display(),
                        "pending_bid_cleanup.json unreadable — original bytes preserved, starting fresh"
                    );
                }
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

// ---------------------------------------------------------------------------
// v0.20.x QS fix: deleted-landings tombstone
// ---------------------------------------------------------------------------
//
// `LandingStore::merge_from` (below) is a plain union with no delete
// concept — it exists to stop a client with an INCOMPLETE local store
// (failed restore, fresh install) from shrinking the server's backup.
// But `landing_delete` (in lib.rs) only ever rewrites the LOCAL file; the
// server's backup blob still has the old copy. Without a tombstone, the
// very next backup upload (which now merges the server's copy in FIRST,
// per that same fix) — or an explicit "restore from server" — silently
// resurrects a landing the pilot just deleted. This tombstone is
// deliberately LOCAL-ONLY, never itself uploaded: it only has to stop
// THIS machine's own merge from undoing THIS machine's own deletion.

/// Small file-backed set of `pirep_id`s the pilot has explicitly deleted
/// via the Landing tab. Consulted before merging an incoming server/
/// restore payload so a deleted record can't come back from a backup
/// that predates the deletion.
pub struct DeletedLandingsTombstone {
    path: PathBuf,
}

impl DeletedLandingsTombstone {
    pub fn open(app_data_dir: impl AsRef<Path>) -> Result<Self, StorageError> {
        let dir = app_data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            path: dir.join(DELETED_LANDINGS_FILE),
        })
    }

    pub fn read_all(&self) -> Result<Vec<String>, StorageError> {
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
                tracing::warn!(error = %e, "deleted_landings.json unreadable — starting fresh");
                Ok(Vec::new())
            }
        }
    }

    /// Record a `pirep_id` as deleted. Idempotent.
    pub fn record_deleted(&self, pirep_id: &str) -> Result<(), StorageError> {
        let mut ids = self.read_all()?;
        if ids.iter().any(|id| id == pirep_id) {
            return Ok(());
        }
        ids.push(pirep_id.to_string());
        if ids.len() > DELETED_LANDINGS_MAX_ROWS {
            let excess = ids.len() - DELETED_LANDINGS_MAX_ROWS;
            ids.drain(0..excess);
        }
        self.write_all(&ids)
    }

    fn write_all(&self, ids: &[String]) -> Result<(), StorageError> {
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(ids)?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Drop any `incoming` record whose `pirep_id` is tombstoned. Call this on
/// a server/restore payload BEFORE `LandingStore::merge_from`, so a
/// locally-deleted-but-still-server-backed-up record can't be merged back
/// in. A pure function (no I/O) so it's directly unit-testable.
pub fn filter_tombstoned(
    incoming: Vec<LandingRecord>,
    tombstoned: &[String],
) -> Vec<LandingRecord> {
    incoming
        .into_iter()
        .filter(|r| !tombstoned.iter().any(|id| id == &r.pirep_id))
        .collect()
}

#[cfg(test)]
mod pending_bid_cleanup_queue_tests {
    use super::*;

    fn temp_queue() -> (PendingBidCleanupQueue, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aeroacars-pendingbidqueue-test-{}-{n}",
            std::process::id()
        ));
        let queue = PendingBidCleanupQueue::open(&dir).expect("open temp queue");
        (queue, dir)
    }

    /// v0.20.x QS fix: mirrors `LandingStore`'s
    /// `corrupt_file_is_backed_up_before_falling_back_to_empty` — this
    /// queue had the SAME bug (corruption silently treated as empty, then
    /// the next write persisted that empty state over the original bytes)
    /// until this release fixed it here too.
    #[test]
    fn corrupt_file_is_backed_up_before_falling_back_to_empty() {
        let (queue, dir) = temp_queue();
        std::fs::write(dir.join(PENDING_BID_CLEANUP_FILE), b"{not valid json")
            .expect("write garbage");

        let items = queue
            .read_all()
            .expect("a corrupt file must not error — it falls back to empty");
        assert!(items.is_empty());

        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt"))
            .collect();
        assert_eq!(backups.len(), 1, "exactly one backup of the corrupt file must exist");
        let backup_bytes = std::fs::read(backups[0].path()).unwrap();
        assert_eq!(
            backup_bytes,
            b"{not valid json",
            "the backup must be byte-identical to the original corrupt file"
        );
        let _ = std::fs::remove_dir_all(&dir);
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

    /// Merge a server backup into the local list and persist the result.
    ///
    /// Returns how many records the merge ADDED. Used when restoring on a
    /// fresh machine, and before every upload so a second computer's
    /// landings aren't overwritten.
    pub fn merge_from(&self, incoming: Vec<LandingRecord>) -> Result<usize, StorageError> {
        let local = self.read_all()?;
        let before = local.len();
        let merged = merge_landings(local, incoming);
        let added = merged.len().saturating_sub(before);
        self.write_all(&merged)?;
        Ok(added)
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
                // v0.19.x FIX: an unparseable file used to be treated as
                // silently empty with nothing preserved — the very next
                // write (e.g. the next landing's upsert) would then
                // persist that empty state over the original bytes,
                // permanently losing the history a corruption might only
                // have partially damaged. Best-effort: copy the original
                // bytes aside before falling back to empty, so there's a
                // recovery path even though the app itself can't read it.
                let backup_path = self.path.with_extension(format!(
                    "json.corrupt-{}",
                    chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
                ));
                if let Err(backup_err) = std::fs::write(&backup_path, &bytes) {
                    tracing::error!(
                        error = %e,
                        backup_error = %backup_err,
                        "landings.json unreadable AND could not back up the original bytes — starting fresh with NO recovery copy"
                    );
                } else {
                    tracing::warn!(
                        error = %e,
                        backup_path = %backup_path.display(),
                        "landings.json unreadable — original bytes preserved, starting fresh"
                    );
                }
                Ok(Vec::new())
            }
        }
    }

    /// v0.19.x FIX: the on-disk invariant is "oldest first" — `upsert`'s
    /// cap-trim (`items.drain(0..drop_count)`) and `merge_landings`'s both
    /// depend on that to drop the OLDEST records when the 500-row cap is
    /// hit, not an arbitrary end. `replace_all` (the delete-record flow)
    /// used to write back whatever order its caller handed it — `list()`
    /// returns NEWEST first, so deleting one record via the Landing tab
    /// silently flipped the on-disk order. The next `upsert` past the cap
    /// would then drop the NEWEST landings instead of the oldest. Sorting
    /// here, at the one choke point every write goes through, makes the
    /// invariant hold regardless of what order any caller passes in —
    /// closing this class of bug for every current AND future caller, not
    /// just the one that happened to trigger it.
    fn write_all(&self, items: &[LandingRecord]) -> Result<(), StorageError> {
        let mut items = items.to_vec();
        items.sort_by_key(|r| r.touchdown_at);
        let tmp = self.path.with_extension("json.tmp");
        let bytes = serde_json::to_vec(&items)?;
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

/// Merge two landing lists into one, newest write per PIREP winning.
///
/// Why merging rather than "last upload wins": A pilot who flies from two
/// machines would otherwise lose whatever the other one recorded — the
/// second machine uploads its shorter list and silently erases the rest.
/// Landings carry the PIREP id as a natural key, so duplicates are
/// unambiguous and a merge is always well-defined.
///
/// Conflict rule is `recorded_at`, not `touchdown_at`: the same landing can
/// legitimately be re-recorded (a re-filed PIREP, a corrected score), and the
/// later WRITE is the better record even though the touchdown time is
/// identical.
pub fn merge_landings(
    local: Vec<LandingRecord>,
    incoming: Vec<LandingRecord>,
) -> Vec<LandingRecord> {
    use std::collections::HashMap;

    let mut by_id: HashMap<String, LandingRecord> = HashMap::new();
    for rec in local.into_iter().chain(incoming) {
        match by_id.get(&rec.pirep_id) {
            Some(existing) if existing.recorded_at >= rec.recorded_at => {}
            _ => {
                by_id.insert(rec.pirep_id.clone(), rec);
            }
        }
    }

    let mut out: Vec<LandingRecord> = by_id.into_values().collect();
    // Oldest first on disk — `upsert` trims from the front when the cap is
    // hit, so this ordering is what makes it drop the OLDEST rather than a
    // random one.
    out.sort_by_key(|r| r.touchdown_at);
    if out.len() > LANDINGS_MAX_ROWS {
        let drop_count = out.len() - LANDINGS_MAX_ROWS;
        out.drain(0..drop_count);
    }
    out
}

#[cfg(test)]
mod merge_tests {
    use super::*;

    /// Minimal-Datensatz. Nur die Pflichtfelder — alles andere hat
    /// `serde(default)`, weshalb der Umweg über JSON hier deutlich weniger
    /// Rauschen erzeugt als 95 Felder von Hand.
    fn rec(pirep_id: &str, touchdown: &str, recorded: &str) -> LandingRecord {
        serde_json::from_value(serde_json::json!({
            "pirep_id": pirep_id,
            "touchdown_at": touchdown,
            "recorded_at": recorded,
            "flight_number": "1224",
            "airline_icao": "TKJ",
            "dpt_airport": "EDDP",
            "arr_airport": "LTFE",
            "score_numeric": 80,
            "score_label": "gut",
            "grade_letter": "B",
            "landing_rate_fpm": -236.0,
            "bounce_count": 0,
            "touchdown_profile": [],
            "approach_samples": [],
            "ux_version": 1,
            "forensics_version": 1,
            "sub_scores": [],
            "accident": false,
            "accident_reasons": [],
        }))
        .expect("test record")
    }

    /// Der Fall, für den es die Zusammenführung gibt: zwei Rechner, jeder
    /// mit eigenen Flügen. "Letzter Upload gewinnt" hätte die Hälfte gelöscht.
    #[test]
    fn keeps_landings_from_both_machines() {
        let local = vec![
            rec("A", "2026-07-01T10:00:00Z", "2026-07-01T10:05:00Z"),
            rec("B", "2026-07-02T10:00:00Z", "2026-07-02T10:05:00Z"),
        ];
        let remote = vec![
            rec("C", "2026-07-03T10:00:00Z", "2026-07-03T10:05:00Z"),
            rec("D", "2026-07-04T10:00:00Z", "2026-07-04T10:05:00Z"),
        ];
        let merged = merge_landings(local, remote);
        let mut ids: Vec<&str> = merged.iter().map(|r| r.pirep_id.as_str()).collect();
        ids.sort_unstable();
        assert_eq!(ids, ["A", "B", "C", "D"], "kein Flug darf verlorengehen");
    }

    /// Derselbe Flug auf beiden Seiten darf nicht doppelt erscheinen.
    #[test]
    fn deduplicates_by_pirep_id() {
        let a = vec![rec("A", "2026-07-01T10:00:00Z", "2026-07-01T10:05:00Z")];
        let b = vec![rec("A", "2026-07-01T10:00:00Z", "2026-07-01T10:05:00Z")];
        assert_eq!(merge_landings(a, b).len(), 1);
    }

    /// Bei Konflikt gewinnt der SPÄTER GESCHRIEBENE Datensatz, nicht der mit
    /// späterem Aufsetzzeitpunkt: Ein neu gefilter PIREP oder eine korrigierte
    /// Bewertung hat dieselbe Aufsetzzeit, ist aber die bessere Angabe.
    #[test]
    fn later_write_wins_on_conflict() {
        let old = vec![rec("A", "2026-07-01T10:00:00Z", "2026-07-01T10:05:00Z")];
        let mut newer = rec("A", "2026-07-01T10:00:00Z", "2026-07-01T18:00:00Z");
        newer.score_numeric = 95;
        let merged = merge_landings(old, vec![newer]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].score_numeric, 95, "die spätere Fassung muss gewinnen");
    }

    /// Reihenfolge auch bei umgekehrter Übergabe — die Regel darf nicht davon
    /// abhängen, welche Seite "lokal" heißt.
    #[test]
    fn conflict_rule_is_symmetric() {
        let older = rec("A", "2026-07-01T10:00:00Z", "2026-07-01T10:05:00Z");
        let mut newer = rec("A", "2026-07-01T10:00:00Z", "2026-07-01T18:00:00Z");
        newer.score_numeric = 95;
        assert_eq!(
            merge_landings(vec![newer.clone()], vec![older.clone()])[0].score_numeric,
            95,
        );
        assert_eq!(merge_landings(vec![older], vec![newer])[0].score_numeric, 95);
    }

    /// Ein leerer Serverstand darf lokale Landungen nicht löschen — genau der
    /// Unfall, den ein fehlerhafter Client sonst auslösen würde.
    #[test]
    fn an_empty_side_never_deletes_anything() {
        let local = vec![
            rec("A", "2026-07-01T10:00:00Z", "2026-07-01T10:05:00Z"),
            rec("B", "2026-07-02T10:00:00Z", "2026-07-02T10:05:00Z"),
        ];
        assert_eq!(merge_landings(local.clone(), vec![]).len(), 2);
        assert_eq!(merge_landings(vec![], local).len(), 2);
    }

    /// Über der Obergrenze fallen die ÄLTESTEN heraus, nicht beliebige.
    #[test]
    fn trims_the_oldest_when_over_the_cap() {
        let mut many: Vec<LandingRecord> = (0..LANDINGS_MAX_ROWS + 10)
            .map(|i| {
                rec(
                    &format!("P{i}"),
                    &format!("2026-01-01T00:{:02}:00Z", i % 60),
                    "2026-01-01T00:00:00Z",
                )
            })
            .collect();
        // Aufsetzzeiten eindeutig machen, damit die Sortierung definiert ist.
        for (i, r) in many.iter_mut().enumerate() {
            r.touchdown_at = chrono::DateTime::from_timestamp(1_700_000_000 + i as i64 * 60, 0)
                .expect("ts");
        }
        let merged = merge_landings(many, vec![]);
        assert_eq!(merged.len(), LANDINGS_MAX_ROWS);
        assert_eq!(merged[0].pirep_id, "P10", "die ältesten zehn müssen weg sein");
    }
}

/// v0.19.x FIX: `LandingStore` behavior against a REAL filesystem — the
/// on-disk ordering invariant (`write_all`), and the corrupt-file recovery
/// path (`read_all`). Both `merge_landings` and `upsert`'s cap-trim were
/// already covered above as pure functions; these tests catch what only
/// shows up once an actual file and an actual caller sequence are involved.
#[cfg(test)]
mod landing_store_tests {
    use super::*;

    fn rec(pirep_id: &str, touchdown: &str, recorded: &str) -> LandingRecord {
        serde_json::from_value(serde_json::json!({
            "pirep_id": pirep_id,
            "touchdown_at": touchdown,
            "recorded_at": recorded,
            "flight_number": "1224",
            "airline_icao": "TKJ",
            "dpt_airport": "EDDP",
            "arr_airport": "LTFE",
            "score_numeric": 80,
            "score_label": "gut",
            "grade_letter": "B",
            "landing_rate_fpm": -236.0,
            "bounce_count": 0,
            "touchdown_profile": [],
            "approach_samples": [],
            "ux_version": 1,
            "forensics_version": 1,
            "sub_scores": [],
            "accident": false,
            "accident_reasons": [],
        }))
        .expect("test record")
    }

    fn ts(offset_secs: i64) -> String {
        chrono::DateTime::from_timestamp(1_700_000_000 + offset_secs, 0)
            .expect("valid timestamp")
            .to_rfc3339()
    }

    /// Isolated temp dir per test — unique per call so parallel test
    /// threads (same process id) never collide.
    fn temp_store() -> (LandingStore, PathBuf) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "aeroacars-landingstore-test-{}-{n}",
            std::process::id()
        ));
        let store = LandingStore::open(&dir).expect("open temp store");
        (store, dir)
    }

    #[test]
    fn write_all_persists_oldest_first_regardless_of_input_order() {
        let (store, dir) = temp_store();
        // Deliberately newest-first input — exactly what `list()` (which
        // is documented "newest first") hands to `replace_all` in the
        // Landing tab's delete-record flow.
        let items = vec![
            rec("NEW", &ts(300), &ts(300)),
            rec("OLD", &ts(100), &ts(100)),
            rec("MID", &ts(200), &ts(200)),
        ];
        store.replace_all(&items).expect("write");
        let on_disk = store.read_all().expect("read");
        let ids: Vec<&str> = on_disk.iter().map(|r| r.pirep_id.as_str()).collect();
        assert_eq!(
            ids,
            ["OLD", "MID", "NEW"],
            "on-disk order must always be oldest-first, regardless of what order the caller wrote"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The exact real-world sequence: fill to the cap, delete one record
    /// via the same list()->retain->replace_all pattern `landing_delete`
    /// uses, then add one more landing. Before the fix, `replace_all`'s
    /// newest-first write flipped the on-disk order, so this last upsert's
    /// cap-trim dropped the NEWEST records instead of the oldest.
    #[test]
    fn deleting_a_record_does_not_break_the_next_cap_trims_direction() {
        let (store, dir) = temp_store();
        for i in 0..LANDINGS_MAX_ROWS {
            store
                .upsert(rec(&format!("R{i}"), &ts(i as i64), &ts(i as i64)))
                .expect("upsert within cap");
        }

        // Simulate landing_delete: list() (newest-first) -> retain -> replace_all.
        let mut all = store.list().expect("list");
        assert_eq!(
            all[0].pirep_id,
            format!("R{}", LANDINGS_MAX_ROWS - 1),
            "list() must be newest-first, per its own doc comment"
        );
        all.retain(|r| r.pirep_id != "R0"); // pilot deletes the oldest record
        store.replace_all(&all).expect("replace_all");
        // 499 records left — exactly at the cap after one more, so add TWO
        // to actually push past LANDINGS_MAX_ROWS and force a trim.
        store
            .upsert(rec(
                "NEWEST1",
                &ts(LANDINGS_MAX_ROWS as i64),
                &ts(LANDINGS_MAX_ROWS as i64),
            ))
            .expect("upsert 1/2");
        store
            .upsert(rec(
                "NEWEST2",
                &ts(LANDINGS_MAX_ROWS as i64 + 1),
                &ts(LANDINGS_MAX_ROWS as i64 + 1),
            ))
            .expect("upsert 2/2 triggers cap trim");

        let final_ids: std::collections::HashSet<String> =
            store.list().unwrap().into_iter().map(|r| r.pirep_id).collect();
        assert!(
            !final_ids.contains("R1"),
            "the oldest surviving record (R1, since R0 was already deleted) must be \
             the one the trim drops"
        );
        assert!(
            final_ids.contains("NEWEST1") && final_ids.contains("NEWEST2"),
            "the just-added newest records must survive the trim — a corrupted \
             on-disk order would drop them instead"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_is_backed_up_before_falling_back_to_empty() {
        let (store, dir) = temp_store();
        std::fs::write(dir.join(LANDINGS_FILE), b"{not valid json").expect("write garbage");

        let items = store
            .list()
            .expect("a corrupt file must not error — it falls back to empty");
        assert!(items.is_empty());

        let backups: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("corrupt"))
            .collect();
        assert_eq!(backups.len(), 1, "exactly one backup of the corrupt file must exist");
        let backup_bytes = std::fs::read(backups[0].path()).unwrap();
        assert_eq!(
            backup_bytes,
            b"{not valid json",
            "the backup must be byte-identical to the original corrupt file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod deleted_landings_tombstone_tests {
    use super::*;

    fn rec(pirep_id: &str) -> LandingRecord {
        serde_json::from_value(serde_json::json!({
            "pirep_id": pirep_id,
            "touchdown_at": "2026-08-01T00:00:00Z",
            "recorded_at": "2026-08-01T00:00:00Z",
            "flight_number": "1224",
            "airline_icao": "TKJ",
            "dpt_airport": "EDDP",
            "arr_airport": "LTFE",
            "score_numeric": 80,
            "score_label": "gut",
            "grade_letter": "B",
            "landing_rate_fpm": -236.0,
            "bounce_count": 0,
            "touchdown_profile": [],
            "approach_samples": [],
            "ux_version": 1,
            "forensics_version": 1,
            "sub_scores": [],
            "accident": false,
            "accident_reasons": [],
        }))
        .expect("test record")
    }

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "aeroacars-tombstone-test-{}-{n}",
            std::process::id()
        ))
    }

    #[test]
    fn record_deleted_is_idempotent() {
        let dir = temp_dir();
        let tombstone = DeletedLandingsTombstone::open(&dir).expect("open");
        tombstone.record_deleted("PIREP1").expect("record 1");
        tombstone.record_deleted("PIREP1").expect("record again");
        assert_eq!(tombstone.read_all().unwrap(), vec!["PIREP1".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn filter_tombstoned_drops_only_the_deleted_record() {
        let incoming = vec![rec("KEEP1"), rec("DELETED"), rec("KEEP2")];
        let filtered = filter_tombstoned(incoming, &["DELETED".to_string()]);
        let ids: Vec<&str> = filtered.iter().map(|r| r.pirep_id.as_str()).collect();
        assert_eq!(ids, ["KEEP1", "KEEP2"]);
    }

    #[test]
    fn filter_tombstoned_is_a_no_op_with_no_tombstones() {
        let incoming = vec![rec("A"), rec("B")];
        let filtered = filter_tombstoned(incoming, &[]);
        assert_eq!(filtered.len(), 2);
    }

    /// The exact real-world regression this tombstone exists to close:
    /// delete a record locally, then simulate the next backup's
    /// merge-server-copy-in-first step — the deleted record must NOT
    /// reappear in the merged local store.
    #[test]
    fn a_deleted_record_does_not_reappear_after_the_next_merge() {
        let dir = temp_dir();
        let store = LandingStore::open(&dir).expect("open store");
        let tombstone = DeletedLandingsTombstone::open(&dir).expect("open tombstone");

        store.upsert(rec("KEEP")).expect("upsert keep");
        store.upsert(rec("TO_DELETE")).expect("upsert to-delete");

        // landing_delete's real sequence: remove locally, record the tombstone.
        let mut all = store.list().expect("list");
        all.retain(|r| r.pirep_id != "TO_DELETE");
        store.replace_all(&all).expect("replace_all");
        tombstone.record_deleted("TO_DELETE").expect("tombstone");

        // The next backup upload: server still has the old copy (it was
        // never told about the deletion) — this is exactly what
        // `upload_landing_backup` merges in.
        let server_copy = vec![rec("KEEP"), rec("TO_DELETE")];
        let tombstoned = tombstone.read_all().expect("read tombstones");
        let filtered = filter_tombstoned(server_copy, &tombstoned);
        store.merge_from(filtered).expect("merge");

        let ids: std::collections::HashSet<String> =
            store.list().unwrap().into_iter().map(|r| r.pirep_id).collect();
        assert!(ids.contains("KEEP"));
        assert!(
            !ids.contains("TO_DELETE"),
            "a locally-deleted record must not be resurrected by the next backup merge"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
