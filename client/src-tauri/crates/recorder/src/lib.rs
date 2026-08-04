//! Flight recorder — append-only JSONL log per flight.
//!
//! Captures a chronological event stream (phase transitions, position
//! samples, activity-log items) plus the final analyzer bundle when
//! the PIREP is filed. Files live under
//! `<app_data_dir>/flight_logs/<pirep_id>.jsonl` so each flight is a
//! self-contained replay artifact: copy/paste it into a debugger,
//! diff two flights, or feed it back into the FSM offline.
//!
//! Format: one JSON object per line, written via append-mode `O_APPEND`
//! so concurrent writers (we don't have any today, but future replay
//! agents might) can't tear a row.
//!
//! See requirements spec §11, §13–§22.

#![allow(dead_code)]

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sim_core::{FlightPhase, SimSnapshot};
use thiserror::Error;

const LOGS_SUBDIR: &str = "flight_logs";

#[derive(Debug, Error)]
pub enum RecorderError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Tagged union of everything we write into the per-flight log. New
/// variants get added as the FSM and analyzers grow — the JSONL format
/// is forward-compatible because each row is self-describing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FlightLogEvent {
    /// Flight was started (fresh prefile or adopt). Captures the
    /// route + airline so a viewer doesn't need a sidecar manifest.
    FlightStarted {
        timestamp: DateTime<Utc>,
        pirep_id: String,
        airline_icao: String,
        flight_number: String,
        dpt_airport: String,
        arr_airport: String,
    },
    /// Flight resumed after a Tauri restart.
    FlightResumed {
        timestamp: DateTime<Utc>,
        pirep_id: String,
        age_minutes: i64,
        /// v0.20 (Process-Integrity): `Some(false)` if the PREVIOUS
        /// AeroACARS run's crash-sentinel was still present at this run's
        /// start (did NOT exit cleanly — crash/kill/power-loss). `Some(true)`
        /// if it exited via the clean `RunEvent::ExitRequested` path. `None`
        /// for logs written before this field existed, or if the check
        /// itself couldn't run. Additive — old JSONL rows without this key
        /// still deserialize.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        previous_exit_clean: Option<bool>,
    },
    /// Phase-FSM transitioned. Recorded once per change so post-hoc
    /// you can see exactly when boarding ended, takeoff fired, etc.
    PhaseChanged {
        timestamp: DateTime<Utc>,
        from: FlightPhase,
        to: FlightPhase,
        altitude_msl_ft: f64,
        groundspeed_kt: f32,
        altitude_agl_ft: f64,
    },
    /// Per-tick position snapshot. The full SimSnapshot is embedded so
    /// downstream tooling (offline analyzer, replay viewer) has every
    /// telemetry value the streamer saw at that moment.
    Position {
        timestamp: DateTime<Utc>,
        snapshot: SimSnapshot,
    },
    /// Activity-log entry (squawk change, lights toggle, AP engage,
    /// METAR fetch, …) — same string the user sees in the dashboard.
    Activity {
        timestamp: DateTime<Utc>,
        level: String,
        message: String,
        detail: Option<String>,
    },
    /// One datalink message, sent or received, over the Hoppie network —
    /// CPDLC, PDC and plain telex alike. Written so a flight's ATC
    /// conversation can be reconstructed afterwards next to position and
    /// phase, which is what post-flight fault-finding needs: whether an
    /// uplink was answered, how long a logon stayed unanswered, whether a
    /// clearance arrived before or after the phase changed.
    ///
    /// Never carries the Hoppie logon code — that is a credential and has
    /// no business in a log that gets uploaded.
    Datalink {
        timestamp: DateTime<Utc>,
        /// `"uplink"` (from ATC) or `"downlink"` (from us).
        direction: String,
        /// `"cpdlc"`, `"pdc"` or `"telex"`.
        channel: String,
        /// Facility as addressed on the wire, e.g. `"LKAA"`. `None` for
        /// traffic that carried no station.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        station: Option<String>,
        /// CPDLC message identification number. Only unique WITHIN a
        /// direction — uplink and downlink number independently, so never
        /// key on this alone.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<u32>,
        /// The MIN this message answers, when it is a reply.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        mrn: Option<u32>,
        /// Expected-response code (`WU`, `AN`, `R`, `Y`, `N`, `NE`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response_code: Option<String>,
        /// Message text as it went over the wire, `@` line breaks intact.
        text: String,
    },
    /// Touchdown analyzer settled — final score with the contributing
    /// peak values. Mirrors the LandingScore enum in lib.rs.
    /// **Beibehalten fuer Backwards-Compat** — neue Forensik-Konsumenten
    /// nutzen `TouchdownComplete` (siehe unten).
    LandingScored {
        timestamp: DateTime<Utc>,
        score: String,
        peak_vs_fpm: f32,
        /// Raw 50 Hz single-frame G peak. **Stays raw** (v0.12.3 LE7) —
        /// backward-compatible, never re-purposed to the EMA value.
        peak_g_force: f32,
        bounce_count: u8,
        /// v0.12.3 (LE7): EMA-smoothed window-peak G — the value the
        /// landing is actually scored on. Additive + `serde(default)` so
        /// pre-v0.12.3 JSONL logs without it still deserialize (→ `None`).
        #[serde(default)]
        scored_g_force: Option<f32>,
        /// v0.12.3 (LE8): how `scored_g_force` was derived —
        /// `"ema_max"` (normal) or `"raw_fallback"` (no touchdown window).
        #[serde(default)]
        scored_g_method: Option<String>,
    },
    /// v0.5.34: vollstaendiger Touchdown-Forensik-Payload (gleiche Daten
    /// wie der MQTT-`touchdown`-Topic). Enthaelt ALLE Felder die der
    /// Live-Recorder bekommt — Approach-Stability v2, Landing-Quality
    /// (Wing-Strike, Float, TD-Zone, Vref), V/S-Estimator-Vergleiche,
    /// Runway-Match, Wind-Komponenten, etc.
    ///
    /// Damit kann ein offline Re-Importer (recorder/cli/recoverFromJsonl)
    /// die Touchdown-Row 1:1 rekonstruieren falls die DB-Daten verloren
    /// gehen. Format: serde_json::Value damit das Schema mitwachsen kann
    /// ohne dass alte Logs unparsbar werden.
    TouchdownComplete {
        timestamp: DateTime<Utc>,
        payload: serde_json::Value,
    },
    /// v0.5.39: hi-res 50 Hz Sample-Buffer um den Touchdown-Moment.
    ///
    /// Die normale `Position`-Cadence im Streamer-Tick reicht von 500 ms
    /// (AGL <100 ft) bis 30 s (Cruise). Selbst die schnellste 500-ms-Rate
    /// kann den exakten Touchdown-Frame verpassen — und Netzwerk-Latenz
    /// vom phpVMS-POST in der gleichen Schleife strecht den Tick noch
    /// weiter (typisch 1.5–2 s Loch genau im TD-Moment, siehe Volanta-
    /// Vergleichs-Issue 2026-05-09).
    ///
    /// `spawn_touchdown_sampler` läuft separat bei 50 Hz (20 ms) und
    /// puffert die letzten 5 s im RAM. Bei TD-Edge-Detection sammelt
    /// er weiter für 5 s post-TD und dumpt das gesamte 10-s-Fenster
    /// als ein einzelnes Event in die JSONL — ~500 Samples × ~80 B =
    /// ~40 KB pro Landung. Genug Auflösung um:
    ///   - exakten on_ground-Edge zwischen 2 Samples zu interpolieren
    ///   - VS in mehreren Fenster-Größen zu vergleichen (Volanta vs
    ///     Instantaneous vs DLHv-Style)
    ///   - Peak-G im 500 ms post-TD-Fenster zu finden (Gear-Compression)
    ///   - Bounce-Profile mit Höhe + Dauer pro Excursion zu rekonstruieren
    ///
    /// `samples` ist chronologisch geordnet, dichteste Samples möglich.
    TouchdownWindow {
        timestamp: DateTime<Utc>,
        /// Zeitpunkt des on_ground-Edge (Referenz für relative Berechnungen).
        edge_at: DateTime<Utc>,
        samples: Vec<TouchdownWindowSample>,
    },
    /// v0.5.39: berechnete Forensik-Metriken aus dem TouchdownWindow-Buffer.
    /// Wird IMMER zusammen mit dem TouchdownWindow-Event geschrieben, direkt
    /// danach. Gibt:
    ///   - Multi-Window-VS-Mittel (250/500/1000/1500 ms vor TD-Edge) =
    ///     Volanta-/DLHv-equivalente Werte
    ///   - Peak-G post-TD im 500-ms-Fenster (Gear-Compression-Spike)
    ///   - Flare-Qualität: dVS/dt + Reduktions-Score + Pilot-flare-detected?
    ///   - Bounce-Profile: Anzahl + Peak-AGL + Dauer pro Excursion
    /// Format = serde_json::Value damit das Schema mitwachsen kann ohne
    /// alte Logs unparseable zu machen.
    LandingAnalysis {
        timestamp: DateTime<Utc>,
        edge_at: DateTime<Utc>,
        analysis: serde_json::Value,
    },
    /// PIREP filed (clean or manual) or cancelled. Closes the log.
    FlightEnded {
        timestamp: DateTime<Utc>,
        pirep_id: String,
        outcome: FlightOutcome,
    },
    /// v0.5.34: vollstaendiger PIREP-Payload (gleiche Daten wie der
    /// MQTT-`pirep`-Topic). Block/Flight-Time, Fuel-Aggregate, Distance,
    /// Peak-Altitude, Landing-Score, Go-Around-Count, Touchdown-Count,
    /// Gates, Approach-Runway, Divert-Hints, Notes.
    PirepFiled {
        timestamp: DateTime<Utc>,
        payload: serde_json::Value,
    },
    /// v0.5.34: Block-Snapshot beim Out-Of-Block (gleiche Daten wie der
    /// MQTT-`block`-Topic). Pre-Flight Plan-Snapshot fuer Forensik.
    BlockSnapshot {
        timestamp: DateTime<Utc>,
        payload: serde_json::Value,
    },
    /// v0.5.34: Takeoff-Snapshot beim Wheels-Up (gleiche Daten wie der
    /// MQTT-`takeoff`-Topic). Wheels-Up-Snapshot fuer Forensik.
    TakeoffSnapshot {
        timestamp: DateTime<Utc>,
        payload: serde_json::Value,
    },
    /// v0.7.0 (Forensik v2): emittiert pro VALIDATED contact_frame
    /// (= Layer 2 PASS in touchdown_v2). Enthaelt strukturierte Daten
    /// mit forensics_version=2 marker damit aeroacars-live + zukuenftige
    /// Re-Analyzer wissen: dieses Event kommt aus der v2-Pipeline.
    ///
    /// **Unterschied zu TouchdownComplete (legacy):**
    ///   - TouchdownComplete kann von der alten Single-Shot-Pipeline kommen
    ///   - TouchdownDetected kommt nur von touchdown_v2::validate_candidate
    ///   - Pro Episode kann es 1+ TouchdownDetected geben (Multi-TD Support
    ///     bei T&G/Bounce — wenn der Sampler reset macht)
    ///   - is_final wird beim PIREP-Filing nachgereicht via LandingFinalized
    ///
    /// Spec: docs/spec/touchdown-forensics-v2.md Sektion 7.2.
    TouchdownDetected {
        timestamp: DateTime<Utc>,
        forensics_version: u8,
        contact_at: DateTime<Utc>,
        impact_at: DateTime<Utc>,
        vs_fpm: f32,
        confidence: String,
        source: String,
        sim: String,
    },
    /// v0.7.0 (Forensik v2): emittiert beim PIREP-Filing/Cancel mit dem
    /// finalen Score. Markiert die letzte LandingEpisode als "die Landung"
    /// (= Multi-TD lifecycle: erst hier ist klar welche Episode gilt).
    LandingFinalized {
        timestamp: DateTime<Utc>,
        forensics_version: u8,
        final_vs_fpm: Option<f32>,
        final_score: Option<String>,
    },
}

/// Eine einzelne 50-Hz-Probe aus dem Touchdown-Window-Buffer. Felder
/// matchen die wichtigsten `SimSnapshot`-Werte für Touchdown-Forensik.
/// `Copy + Clone` damit der Sampler den Buffer billig dupliziert.
///
/// **v0.7.0 / forensics_version=2:** zwei neue Optional-Felder fuer
/// Touchdown-Forensik v2:
///   - `gear_normal_force_n`: bei X-Plane immer Some, bei MSFS None
///     (Sim-Limit). Kritisch fuer A1 MUST-PASS Validation in der
///     Forensik-v2-Selection-Chain. Spec docs/spec/touchdown-forensics-v2.md.
///   - `total_weight_kg`: snapshot des Aircraft-Gewichts zum Zeitpunkt
///     des Samples. Brauchen wir fuer die mass-aware Gear-Force-Threshold
///     (3% des statischen Gewichts mit 1000N Floor) UND fuer
///     deterministische Acceptance-Replays aus dem JSONL.
///
/// Backward-compat: alte JSONLs ohne diese Felder deserialisieren mit
/// `None` via `serde(default)`. Forensik-v1-Logik ignoriert sie.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TouchdownWindowSample {
    pub at: DateTime<Utc>,
    pub vs_fpm: f32,
    pub g_force: f32,
    pub on_ground: bool,
    pub agl_ft: f32,
    pub heading_true_deg: f32,
    pub groundspeed_kt: f32,
    pub indicated_airspeed_kt: f32,
    pub lat: f64,
    pub lon: f64,
    pub pitch_deg: f32,
    pub bank_deg: f32,
    /// v0.7.0: gear_normal_force_n in Newton (X-Plane only — Sim-Limit MSFS).
    /// MUST-PASS Validation Anchor in Forensik-v2 (siehe Spec Sektion 4.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gear_normal_force_n: Option<f32>,
    /// v0.7.0: total weight in kg fuer mass-aware gear-force threshold
    /// und deterministische Replay-Acceptance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_weight_kg: Option<f32>,
}

// ─── v0.12.3: FOQA-konforme Scored-G-Berechnung ──────────────────────────
//
// Spec: docs/spec/v0.12.3-landing-g-foqa-measurement.md (LE1–LE3, LE8).
//
// Der gescorte Touchdown-G-Wert ist NICHT der rohe 50-Hz-Einzelframe-Peak,
// sondern der Peak eines leicht geglätteten Signals — so misst echte
// Flugdaten-Überwachung (FOQA/FDM): Anti-Aliasing-Filter, dann Peak. Die
// Glättung ist ein framerate-unabhängiger EMA (tau = 100 ms).

/// v0.12.3 (LE2): EMA-Zeitkonstante für den Scored-G-Filter (Sekunden).
pub const SCORED_G_TAU_SECS: f64 = 0.100;
/// v0.12.3 (LE3): der `max()` wird über `[edge, edge + dies]` genommen.
pub const SCORED_G_WINDOW_SECS: f64 = 1.0;
/// v0.12.3 (LE2): Fallback-`dt`, wenn zwei Samples keinen positiven
/// Zeitabstand haben (doppelter Timestamp o. ä.).
const SCORED_G_NOMINAL_DT_SECS: f64 = 0.020;

/// v0.12.3 (LE8): wie `ScoredG::scored_g` zustande kam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScoredGMethod {
    /// EMA-geglätteter Fenster-Peak — der normale FOQA-konforme Pfad.
    EmaMax,
    /// Kein Touchdown-Fenster vorhanden → `scored_g` == roher G-Wert.
    RawFallback,
}

impl ScoredGMethod {
    /// Wire-/Event-String-Form (`scored_g_method`).
    pub fn as_str(self) -> &'static str {
        match self {
            ScoredGMethod::EmaMax => "ema_max",
            ScoredGMethod::RawFallback => "raw_fallback",
        }
    }
}

/// v0.12.3 (LE1–LE3, LE8): Ergebnis der Scored-G-Berechnung.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScoredG {
    /// Der Wert, auf dem die Landung gescort wird (EMA-max bzw. roh bei
    /// Fallback). Speist `sub_g_force`, Card-Headline, Activity-Text.
    pub scored_g: f32,
    /// Der rohe 50-Hz-Einzelframe-Peak — bleibt als Forensik-Detail.
    pub raw_peak: f32,
    /// Wie `scored_g` abgeleitet wurde.
    pub method: ScoredGMethod,
}

/// v0.12.3 (LE1–LE3): den FOQA-konformen Scored-G aus dem 50-Hz-
/// Touchdown-Fenster berechnen. Das rohe G-Signal wird per
/// framerate-unabhängigem EMA (tau = 100 ms) leicht geglättet, dann der
/// Peak des geglätteten Signals über `[edge, edge + 1.0 s]` genommen.
///
/// `samples` muss nicht vorsortiert sein — wird hier nach Timestamp
/// sortiert (LE2). Nicht-finite `g_force`-Werte werden übersprungen. Hat
/// das Fenster kein brauchbares Post-Edge-Sample, fällt das Ergebnis auf
/// den rohen Peak zurück (`RawFallback`).
pub fn compute_scored_g(samples: &[TouchdownWindowSample], edge_at: DateTime<Utc>) -> ScoredG {
    // LE2 Regel 1 + 4: nach Timestamp sortieren, nicht-finite Samples raus.
    let mut ordered: Vec<&TouchdownWindowSample> =
        samples.iter().filter(|s| s.g_force.is_finite()).collect();
    ordered.sort_by_key(|s| s.at);

    let window_end =
        edge_at + chrono::Duration::milliseconds((SCORED_G_WINDOW_SECS * 1000.0) as i64);
    let in_window = |s: &TouchdownWindowSample| s.at >= edge_at && s.at <= window_end;

    // Roher Peak im Post-Edge-Fenster (Forensik-Referenz + Fallback).
    let mut raw_peak = f32::NEG_INFINITY;
    for s in &ordered {
        if in_window(s) && s.g_force > raw_peak {
            raw_peak = s.g_force;
        }
    }

    // EMA über das gesamte (auch Pre-Edge-)Set — LE2 Regel 2/3/5.
    let mut smoothed: Option<f64> = None;
    let mut prev_at: Option<DateTime<Utc>> = None;
    let mut scored = f32::NEG_INFINITY;
    for s in &ordered {
        let g = s.g_force as f64;
        smoothed = Some(match smoothed {
            None => g, // LE2 Regel 2: Init beim ersten finiten Sample.
            Some(prev) => {
                let dt = prev_at
                    .map(|p| {
                        (s.at - p).num_microseconds().unwrap_or(0) as f64 / 1_000_000.0
                    })
                    .filter(|&d| d > 0.0)
                    .unwrap_or(SCORED_G_NOMINAL_DT_SECS);
                let alpha = 1.0 - (-dt / SCORED_G_TAU_SECS).exp();
                prev + alpha * (g - prev)
            }
        });
        prev_at = Some(s.at);
        if in_window(s) {
            if let Some(sm) = smoothed {
                if (sm as f32) > scored {
                    scored = sm as f32;
                }
            }
        }
    }

    if scored.is_finite() {
        ScoredG {
            scored_g: scored,
            raw_peak: if raw_peak.is_finite() { raw_peak } else { scored },
            method: ScoredGMethod::EmaMax,
        }
    } else {
        // Kein brauchbares Post-Edge-Sample → definierter Fallback (LE8).
        //
        // v0.19.x FIX: dieser Zweig fiel bisher auf 0.0 g zurück, wenn
        // AUCH kein roher In-Window-Peak existiert (z. B. `samples`
        // komplett leer, oder alle Samples liegen vor `edge_at`, wie im
        // Test `raw_fallback_when_no_window_samples` — dort wird das gar
        // nicht geprüft, weshalb der Fehler unbemerkt blieb). 0.0 g heißt
        // Schwerelosigkeit — physikalisch unmöglich bei Bodenkontakt und
        // fällt bei `sub_g_force` unterhalb JEDER Schwelle, was fälschlich
        // als "besonders sanfte" Landung gewertet würde statt als "keine
        // Daten". 1.0 g (Standlast/Ruhewert) ist der einzige physikalisch
        // ehrliche "wir wissen es nicht"-Default.
        let raw = if raw_peak.is_finite() { raw_peak } else { 1.0 };
        ScoredG { scored_g: raw, raw_peak: raw, method: ScoredGMethod::RawFallback }
    }
}

/// v0.12.3 (LE8): definierter Fallback für Score-Pfade **ohne**
/// Touchdown-Fenster (z. B. spätes Roh-Peak-Tracking). `scored_g` ist der
/// rohe G-Wert, als `RawFallback` markiert.
pub fn scored_g_raw_fallback(raw_g: f32) -> ScoredG {
    ScoredG { scored_g: raw_g, raw_peak: raw_g, method: ScoredGMethod::RawFallback }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlightOutcome {
    Filed,
    Manual,
    Cancelled,
    Forgotten,
}

/// Append-only writer for one flight's log. Cheap to construct — just
/// holds a path. Each `append` opens / appends / closes so a Tauri
/// crash never leaves a half-written line.
pub struct FlightRecorder {
    path: PathBuf,
}

impl FlightRecorder {
    /// Open (or implicitly create) the log file for this PIREP under
    /// `<app_data_dir>/flight_logs/<pirep_id>.jsonl`. The PIREP id is
    /// path-sanitised so a malicious server can't traverse the FS.
    pub fn open(app_data_dir: impl AsRef<Path>, pirep_id: &str) -> Result<Self, RecorderError> {
        let dir = app_data_dir.as_ref().join(LOGS_SUBDIR);
        std::fs::create_dir_all(&dir)?;
        let safe = sanitize_pirep_id(pirep_id);
        Ok(Self {
            path: dir.join(format!("{safe}.jsonl")),
        })
    }

    /// Append one event as a JSON line. Best-effort — errors are
    /// returned to the caller but the recorder is intended to be
    /// fire-and-forget from the streamer's perspective.
    pub fn append(&self, event: &FlightLogEvent) -> Result<(), RecorderError> {
        let mut line = serde_json::to_vec(event)?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(&line)?;
        Ok(())
    }

    /// Path to the underlying file. Useful for the dashboard's "open
    /// flight log folder" button (future) or for a `Show in Explorer`
    /// helper.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Strip anything that isn't a safe filename character. PIREP ids are
/// always alphanumeric in practice, but harden against `..`/`/` if a
/// future phpVMS deployment changes the format.
fn sanitize_pirep_id(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

/// Aggregate stats across all per-flight log files under
/// `<app_data_dir>/flight_logs/`. Used by the Settings → Storage panel
/// to show "X Logs · Y MB belegen" before the user clicks delete.
#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct FlightLogStats {
    pub count: u32,
    pub total_bytes: u64,
}

pub fn flight_logs_stats(app_data_dir: impl AsRef<Path>) -> Result<FlightLogStats, RecorderError> {
    let dir = app_data_dir.as_ref().join(LOGS_SUBDIR);
    if !dir.exists() {
        return Ok(FlightLogStats::default());
    }
    let mut stats = FlightLogStats::default();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_file() && entry.path().extension().and_then(|e| e.to_str()) == Some("jsonl") {
            stats.count += 1;
            stats.total_bytes += meta.len();
        }
    }
    Ok(stats)
}

/// Delete every `*.jsonl` under `<app_data_dir>/flight_logs/`. Returns
/// the count of files actually removed (best-effort — read errors on
/// individual files are skipped, not reported).
pub fn flight_logs_delete_all(app_data_dir: impl AsRef<Path>) -> Result<u32, RecorderError> {
    let dir = app_data_dir.as_ref().join(LOGS_SUBDIR);
    if !dir.exists() {
        return Ok(0);
    }
    let mut removed = 0u32;
    for entry in std::fs::read_dir(&dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Delete `*.jsonl` files whose mtime is older than `older_than_days`.
/// Used by the Settings auto-purge toggle (default 30 days). Returns
/// the count of files removed. Files whose mtime can't be read are
/// left alone.
pub fn flight_logs_purge_older_than(
    app_data_dir: impl AsRef<Path>,
    older_than_days: u32,
) -> Result<u32, RecorderError> {
    let dir = app_data_dir.as_ref().join(LOGS_SUBDIR);
    if !dir.exists() {
        return Ok(0);
    }
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(u64::from(older_than_days) * 86_400))
        .unwrap_or(std::time::UNIX_EPOCH);
    let mut removed = 0u32;
    for entry in std::fs::read_dir(&dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        if mtime < cutoff && std::fs::remove_file(&path).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(test)]
mod scored_g_tests {
    use super::*;

    /// Build a `TouchdownWindowSample` with only `at` + `g_force` set —
    /// the only two fields `compute_scored_g` looks at.
    fn s(at: DateTime<Utc>, g: f32) -> TouchdownWindowSample {
        TouchdownWindowSample {
            at,
            vs_fpm: 0.0,
            g_force: g,
            on_ground: false,
            agl_ft: 0.0,
            heading_true_deg: 0.0,
            groundspeed_kt: 0.0,
            indicated_airspeed_kt: 0.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: None,
        }
    }

    /// Sample a continuous g(t) curve every `dt_ms` from -300 ms to
    /// +1000 ms around the edge (t = 0).
    fn sampled(edge: DateTime<Utc>, dt_ms: i64, g: impl Fn(f64) -> f32) -> Vec<TouchdownWindowSample> {
        let mut out = Vec::new();
        let mut t_ms = -300_i64;
        while t_ms <= 1000 {
            out.push(s(edge + chrono::Duration::milliseconds(t_ms), g(t_ms as f64 / 1000.0)));
            t_ms += dt_ms;
        }
        out
    }

    /// TAP533-shaped touchdown: airborne ~0.95 g, then a sustained
    /// ~1.9 g plateau for ~160 ms, decaying afterwards.
    fn tap533_like(t: f64) -> f32 {
        let g: f64 = if t < 0.0 {
            0.95
        } else if t < 0.11 {
            0.95 + (1.95 - 0.95) * (t / 0.11) // rise to the raw peak
        } else if t < 0.27 {
            1.92 // sustained plateau
        } else {
            (1.92 - (t - 0.27) * 4.0).max(0.8) // decay
        };
        g as f32
    }

    #[test]
    fn ema_keeps_sustained_peak() {
        let edge = Utc::now();
        let r = compute_scored_g(&sampled(edge, 22, tap533_like), edge);
        // A genuine ~160 ms plateau is kept (lightly lagged), not flattened.
        assert!(r.scored_g > 1.65 && r.scored_g < 1.90, "scored_g={}", r.scored_g);
        // Raw peak ~= the sampled plateau (1.92) — far above the smoothed
        // value, confirming the smoothing demotes the raw single-frame max.
        assert!(r.raw_peak >= 1.90, "raw_peak={}", r.raw_peak);
        assert!(r.raw_peak > r.scored_g, "raw should exceed scored");
        assert_eq!(r.method, ScoredGMethod::EmaMax);
    }

    #[test]
    fn tap533_shaped_window_scores_around_1_78() {
        // The TAP533-shaped trace at ~22 ms (~45 Hz) → ~1.78 g (spec table).
        let edge = Utc::now();
        let r = compute_scored_g(&sampled(edge, 22, tap533_like), edge);
        assert!(r.scored_g > 1.70 && r.scored_g < 1.86, "scored_g={}", r.scored_g);
    }

    #[test]
    fn ema_frame_rate_independent() {
        // The SAME g(t) curve sampled at 50 Hz vs 30 Hz must score equal.
        let edge = Utc::now();
        let at_50 = compute_scored_g(&sampled(edge, 20, tap533_like), edge).scored_g;
        let at_30 = compute_scored_g(&sampled(edge, 33, tap533_like), edge).scored_g;
        assert!((at_50 - at_30).abs() < 0.04, "50Hz={at_50} 30Hz={at_30}");
    }

    #[test]
    fn ema_attenuates_single_frame_spike() {
        // Constant 1.10 g with ONE 20 ms frame at 3.0 g. The EMA cannot
        // fully erase it, but attenuates it far below the raw peak — the
        // raw peak (3.0) stays available as forensic detail.
        let edge = Utc::now();
        let mut v = sampled(edge, 20, |_| 1.10);
        // Spike one in-window frame.
        for x in v.iter_mut() {
            if (x.at - edge).num_milliseconds() == 200 {
                x.g_force = 3.0;
            }
        }
        let r = compute_scored_g(&v, edge);
        assert!((r.raw_peak - 3.0).abs() < 0.001, "raw_peak={}", r.raw_peak);
        assert!(r.scored_g < 1.7, "spike not attenuated: scored_g={}", r.scored_g);
    }

    #[test]
    fn ema_non_finite_samples_ignored() {
        let edge = Utc::now();
        let mut v = sampled(edge, 22, tap533_like);
        for x in v.iter_mut() {
            if (x.at - edge).num_milliseconds() == 110 {
                x.g_force = f32::NAN;
            }
            if (x.at - edge).num_milliseconds() == 132 {
                x.g_force = f32::INFINITY;
            }
        }
        let r = compute_scored_g(&v, edge);
        assert!(r.scored_g.is_finite(), "scored_g not finite");
        assert!(r.scored_g > 1.5 && r.scored_g < 1.9, "scored_g={}", r.scored_g);
    }

    #[test]
    fn ema_init_from_first_finite_sample() {
        // No pre-edge samples: the EMA must init at the first in-window
        // sample, not at a constant — a steady 1.30 g window scores ~1.30.
        let edge = Utc::now();
        let v: Vec<_> = (0..40)
            .map(|i| s(edge + chrono::Duration::milliseconds(i * 22), 1.30))
            .collect();
        let r = compute_scored_g(&v, edge);
        assert!((r.scored_g - 1.30).abs() < 0.01, "scored_g={}", r.scored_g);
    }

    #[test]
    fn raw_fallback_when_no_window_samples() {
        // Only pre-edge samples → no post-edge data → RawFallback. None of
        // those pre-edge samples count toward `raw_peak` either (it only
        // scans in-window samples), so this is the exact "nothing usable
        // at all" case the 0.0 g bug hid in — asserting only `method`
        // here previously let a scored_g of 0.0 (impossible: weightless
        // on the ground) slip through unnoticed.
        let edge = Utc::now();
        let v: Vec<_> = (1..10)
            .map(|i| s(edge - chrono::Duration::milliseconds(i * 22), 1.0))
            .collect();
        let r = compute_scored_g(&v, edge);
        assert_eq!(r.method, ScoredGMethod::RawFallback);
        assert_eq!(r.scored_g, 1.0, "must fall back to nominal 1g, not the impossible 0.0g");
        assert_eq!(r.raw_peak, 1.0);
    }

    #[test]
    fn raw_fallback_on_totally_empty_input_is_not_impossible_zero_g() {
        // v0.19.x FIX: the exact bug — an empty sample slice used to
        // score 0.0 g (weightless), which sub_g_force would misread as
        // "impossibly smooth" instead of "no data at all".
        let edge = Utc::now();
        let r = compute_scored_g(&[], edge);
        assert_eq!(r.method, ScoredGMethod::RawFallback);
        assert_eq!(r.scored_g, 1.0);
        assert_eq!(r.raw_peak, 1.0);
    }

    #[test]
    fn scored_g_raw_fallback_helper() {
        let r = scored_g_raw_fallback(1.42);
        assert_eq!(r.scored_g, 1.42);
        assert_eq!(r.raw_peak, 1.42);
        assert_eq!(r.method, ScoredGMethod::RawFallback);
        assert_eq!(r.method.as_str(), "raw_fallback");
    }

    #[test]
    fn old_landing_scored_event_deserializes() {
        // A pre-v0.12.3 LandingScored row without the new fields must
        // still deserialize (scored_g_* default to None).
        let json = r#"{"type":"landing_scored","timestamp":"2026-05-20T19:55:45Z",
            "score":"hard","peak_vs_fpm":-339.0,"peak_g_force":1.95,"bounce_count":0}"#;
        let ev: FlightLogEvent = serde_json::from_str(json).expect("deserialize old event");
        match ev {
            FlightLogEvent::LandingScored {
                peak_g_force,
                scored_g_force,
                scored_g_method,
                ..
            } => {
                assert_eq!(peak_g_force, 1.95);
                assert_eq!(scored_g_force, None);
                assert_eq!(scored_g_method, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn old_flight_resumed_event_deserializes() {
        // v0.20: a pre-existing FlightResumed row without previous_exit_clean
        // must still deserialize (defaults to None) — old JSONL logs must
        // stay readable.
        let json = r#"{"type":"flight_resumed","timestamp":"2026-07-22T22:19:26Z",
            "pirep_id":"g8wrmzEKnY9XGk6r","age_minutes":98}"#;
        let ev: FlightLogEvent = serde_json::from_str(json).expect("deserialize old event");
        match ev {
            FlightLogEvent::FlightResumed {
                age_minutes,
                previous_exit_clean,
                ..
            } => {
                assert_eq!(age_minutes, 98);
                assert_eq!(previous_exit_clean, None);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn flight_resumed_round_trips_previous_exit_clean() {
        let ev = FlightLogEvent::FlightResumed {
            timestamp: Utc::now(),
            pirep_id: "abc123".to_string(),
            age_minutes: 5,
            previous_exit_clean: Some(false),
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        assert!(json.contains("\"previous_exit_clean\":false"));
        let back: FlightLogEvent = serde_json::from_str(&json).expect("deserialize");
        match back {
            FlightLogEvent::FlightResumed {
                previous_exit_clean, ..
            } => assert_eq!(previous_exit_clean, Some(false)),
            _ => panic!("wrong variant"),
        }
    }

    /// The row shape post-flight analysis reads. Pinned deliberately: the
    /// whole point of recording datalink traffic is querying it later, and
    /// a silently renamed field breaks every saved query.
    #[test]
    fn datalink_row_keeps_its_wire_shape() {
        let ev = FlightLogEvent::Datalink {
            timestamp: Utc::now(),
            direction: "uplink".to_string(),
            channel: "cpdlc".to_string(),
            station: Some("LKAA".to_string()),
            min: Some(53),
            mrn: Some(2),
            response_code: Some("WU".to_string()),
            text: "CLIMB TO FL370".to_string(),
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        assert!(json.contains("\"type\":\"datalink\""), "{json}");
        for field in [
            "\"direction\":\"uplink\"",
            "\"channel\":\"cpdlc\"",
            "\"station\":\"LKAA\"",
            "\"min\":53",
            "\"mrn\":2",
            "\"response_code\":\"WU\"",
            "\"text\":\"CLIMB TO FL370\"",
        ] {
            assert!(json.contains(field), "missing {field} in {json}");
        }

        let back: FlightLogEvent = serde_json::from_str(&json).expect("deserialize");
        match back {
            FlightLogEvent::Datalink { min, mrn, text, .. } => {
                assert_eq!(min, Some(53));
                assert_eq!(mrn, Some(2));
                assert_eq!(text, "CLIMB TO FL370");
            }
            _ => panic!("wrong variant"),
        }
    }

    /// Telex and PDC carry no MIN/MRN and no response code. Those keys must
    /// be OMITTED rather than serialized as null, so a reader can treat
    /// "has a MIN" as "is a threaded CPDLC message".
    #[test]
    fn datalink_row_omits_absent_cpdlc_fields() {
        let ev = FlightLogEvent::Datalink {
            timestamp: Utc::now(),
            direction: "downlink".to_string(),
            channel: "pdc".to_string(),
            station: Some("EDDP".to_string()),
            min: None,
            mrn: None,
            response_code: None,
            text: "REQUEST PREDEP CLEARANCE".to_string(),
        };
        let json = serde_json::to_string(&ev).expect("serialize");
        for absent in ["\"min\"", "\"mrn\"", "\"response_code\"", "null"] {
            assert!(!json.contains(absent), "{absent} should be omitted: {json}");
        }
    }

    /// A row written before this variant existed, and one from a future
    /// build with extra keys, must both still parse — the JSONL is append
    /// only and gets read by tooling of a different vintage than the writer.
    #[test]
    fn datalink_row_tolerates_missing_and_unknown_keys() {
        let minimal = r#"{"type":"datalink","timestamp":"2026-07-25T12:42:49Z",
            "direction":"uplink","channel":"telex","text":"STANDBY"}"#;
        let ev: FlightLogEvent = serde_json::from_str(minimal).expect("minimal row parses");
        match ev {
            FlightLogEvent::Datalink {
                station,
                min,
                text,
                ..
            } => {
                assert_eq!(station, None);
                assert_eq!(min, None);
                assert_eq!(text, "STANDBY");
            }
            _ => panic!("wrong variant"),
        }
    }
}
