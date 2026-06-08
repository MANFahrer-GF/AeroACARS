//! Touchdown-Forensik v2 — Layer 1 (Detection) + Layer 2 (Validation) +
//! Layer 3 (VS-Calculation am impact_frame).
//!
//! Spec: docs/spec/touchdown-forensics-v2.md (v2.3, approved)
//!
//! **Designed pure** (sim-agnostisch, ohne Side-Effects auf Tauri-State)
//! damit die Logik 1:1 gegen historische JSONL-Replays gepruft werden kann.
//! Layer 4 (LandingEpisode-Aggregation + Lifecycle) sitzt ausserhalb dieses
//! Moduls weil sie Tauri-State + Persistierung braucht.
//!
//! **Sim-Trennung** ist STRUKTURELL unvermeidbar weil X-Plane das wichtigste
//! Validation-Signal hat (gear_normal_force_n) und MSFS nicht. Siehe Spec
//! Sektion 4.

use chrono::{DateTime, Duration, Utc};
use recorder::TouchdownWindowSample;
use serde::{Deserialize, Serialize};

use crate::aircraft_category::AircraftCategory;
use crate::SimKind;

/// **forensics_version=2** Marker fuer Events + PIREP-payload.
/// Recorder/aeroacars-live identifiziert via diesem Wert welche
/// Auswertungs-Logik zu verwenden ist.
pub const FORENSICS_VERSION: u8 = 2;

// ─── v0.7.6 P1-2: Bounce-Threshold-Konstanten ─────────────────────────────
//
// Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-2.
//
// Trennung zwischen "forensisch sichtbar" und "scoring-relevant" damit Pilot
// kleine Federwerk-Hopser im Replay sehen kann ohne dass jeder Mikro-Hopser
// ihn im Score bestraft. Real-Beleg: SAS9987 (v0.7.5 PW68L0QGJkq0D63J) hatte
// bounce_max_agl_ft=13.6 → forensisch ein Hopser, scoring-irrelevant.
//
// Beide Konstanten leben hier (touchdown_v2 = Forensik-Schicht), NICHT in
// der landing-scoring-Crate, weil nur die Forensik AGL-Verlauf + Hopser-
// Hoehen kennt. Die landing-scoring-Crate vertraut dem Caller und bekommt
// nur den finalen scored_bounce_count als Input.

/// Mindestens 5 ft AGL-Excursion damit ein Wiederabheben **forensisch**
/// gezaehlt wird. Filtert Sim-Float-Noise (typisch 1-2 ft) und sanftes
/// Federwerk-Oszillieren raus, laesst aber sichtbare Hopser im Replay-
/// Tab durch. Erscheint im PIREP als `forensic_bounce_count`.
pub const BOUNCE_FORENSIC_MIN_AGL_FT: f32 = 5.0;

/// Mindestens 15 ft AGL-Excursion damit ein Wiederabheben im Sub-Score
/// **bestraft** wird. Hoch genug dass ein "echter" Bounce (Pitch-Up nach
/// harter Landung) erfasst wird, aber nicht jeder Federwerk-Hopser den
/// Pilot bestraft. Erscheint im PIREP als `scored_bounce_count` und
/// landet im `landing-scoring::sub_bounces`-Sub-Score.
///
/// v0.7.7+: Schwelle bei 15.0 ft eingependelt nach Echt-Daten-Review,
/// kein Patch notwendig — Beobachtungs-Sample war ausgeglichen.
pub const BOUNCE_SCORED_MIN_AGL_FT: f32 = 15.0;

/// Seaplane / amphibian water-touchdown descent gate (fpm). A water touchdown
/// is the floats / hull SETTLING onto the water — the impact V/S must be a
/// genuine sink below this. This is what separates a landing from a level low
/// pass, a step-taxi skim, or a glassy go-around that merely lingers in the
/// water-contact band: those have V/S ≈ 0 and are rejected even though they
/// sustain low-AGL. A real glassy-water landing descends ~100-200 fpm (well
/// past this), so it is unaffected. Deliberately larger in magnitude than the
/// fixed-wing −10 fpm floor so no phantom-validation window is left open for
/// non-landing low flight over water (review finding, v0.15.21).
pub const WATER_TOUCHDOWN_MIN_DESCENT_FPM: f32 = -50.0;

// ─── Layer 1: TD-Candidate Detection ──────────────────────────────────────

/// Ein Sample-Pair fuer Edge-Detection (prev → current).
/// Liefert Some(TdCandidate) wenn ein Edge-Trigger detected wurde.
///
/// X-Plane: Edge wenn prev.in_air UND (current.on_ground ODER gear_force > epsilon)
/// MSFS:    Edge wenn prev.in_air UND current.on_ground
///
/// `prev.in_air` = !prev.on_ground UND (prev.gear_force.unwrap_or(0) <= epsilon)
pub fn detect_td_candidate(
    prev: Option<&TouchdownWindowSample>,
    current: &TouchdownWindowSample,
    current_idx: usize,
    sim: SimKind,
) -> Option<TdCandidate> {
    const GEAR_FORCE_EPSILON_N: f32 = 1.0;

    let prev = prev?;
    let prev_in_air = !prev.on_ground
        && prev
            .gear_normal_force_n
            .map(|f| f <= GEAR_FORCE_EPSILON_N)
            .unwrap_or(true);
    if !prev_in_air {
        return None;
    }

    let edge_now = if sim.is_xplane() {
        current.on_ground
            || current
                .gear_normal_force_n
                .map(|f| f > GEAR_FORCE_EPSILON_N)
                .unwrap_or(false)
    } else {
        // MSFS / Off — only on_ground edge (kein gear_normal_force_n verfuegbar)
        current.on_ground
    };

    if !edge_now {
        return None;
    }

    Some(TdCandidate {
        edge_sample_index: current_idx,
        edge_at: current.at,
        edge_agl_ft: current.agl_ft,
        edge_vs_fpm: current.vs_fpm,
        edge_gear_force_n: current.gear_normal_force_n,
        edge_g_force: current.g_force,
        edge_total_weight_kg: current.total_weight_kg,
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TdCandidate {
    pub edge_sample_index: usize,
    pub edge_at: DateTime<Utc>,
    pub edge_agl_ft: f32,
    pub edge_vs_fpm: f32,
    pub edge_gear_force_n: Option<f32>,
    pub edge_g_force: f32,
    pub edge_total_weight_kg: Option<f32>,
}

// ─── Layer 2: TD-Validation (sim-spezifisch) ──────────────────────────────

/// Ergebnis der Validation. Bei VALIDATED wird die TD als „echter contact"
/// behandelt; bei FALSE_EDGE ist es ein Streifschuss/Float.
#[derive(Debug, Clone)]
pub enum ValidationResult {
    Validated {
        result: ValidationDetail,
    },
    FalseEdge {
        reason: FalseEdgeReason,
        result: ValidationDetail,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationDetail {
    pub sim: SimKind,
    pub gear_force_threshold_n: f32,
    pub gear_force_pass: bool,
    pub gear_force_peak_in_window_n: Option<f32>,
    pub gear_force_sustained_ms: Option<u64>,
    pub g_force_pass: Option<bool>,
    pub g_force_peak_in_window: f32,
    pub low_agl_persistence_pass: bool,
    pub low_agl_actual_ms: u64,
    pub sustained_ground_pass: Option<bool>,
    pub sustained_ground_actual_ms: u64,
    pub vs_negative_pass: bool,
    pub vs_at_impact_used_for_test: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum FalseEdgeReason {
    /// X-Plane: gear_force unter threshold (oder nicht lange genug)
    GearForceBelowThreshold,
    /// MSFS: weniger als 3 von 4 Tests passed
    InsufficientVoteScore,
}

/// Mass-aware gear-force threshold:
/// max(1000 N floor, 3% × total_weight × 9.80665).
///
/// 1000 N als hartes Minimum schuetzt vor zu strikt-strict bei Glidern/Ultralight.
/// 3% × static weight skaliert mit Aircraft-Mass — A330 (250t) ergibt ~73.5 kN,
/// Cessna 152 (757kg) ergibt ~222 N → floor wins → 1000 N.
pub fn gear_force_threshold_n(total_weight_kg: Option<f32>) -> f32 {
    const ABS_FLOOR: f32 = 1000.0;
    const MASS_RATIO: f32 = 0.03;
    const G: f32 = 9.80665;

    let dynamic = total_weight_kg
        .filter(|w| *w > 100.0)
        .map(|w| w * G * MASS_RATIO)
        .unwrap_or(ABS_FLOOR);
    dynamic.max(ABS_FLOOR)
}

/// Validate eine TdCandidate gegen die sim-spezifischen Tests.
///
/// X-Plane: gear_force ist MUST-PASS (Anchor). A1 FAIL → Validation FAIL.
/// MSFS:    weiches Voting (3 von 4 Tests muessen PASS).
///
/// Time-windows werden via `at`-Timestamps gemessen, NICHT via Sample-Count
/// (Sampler ist nicht garantiert genau 50 Hz).
pub fn validate_candidate(
    candidate: &TdCandidate,
    samples: &[TouchdownWindowSample],
    sim: SimKind,
    impact_frame_vs: f32,
    category: AircraftCategory,
) -> ValidationResult {
    let edge_at = candidate.edge_at;
    let threshold_n = gear_force_threshold_n(candidate.edge_total_weight_kg);

    // Test: gear_force-impact (X-Plane only — MUST-PASS)
    let (gear_force_pass, gear_force_peak, gear_force_sustained_ms) =
        evaluate_gear_force_test(samples, edge_at, threshold_n);

    // Test: g_force-spike (MSFS-relevant)
    let g_force_peak = evaluate_g_force_peak(samples, edge_at);
    let g_force_pass = g_force_peak > 1.05;

    // Test: low_agl_persistence (beide Sims)
    let (low_agl_pass, low_agl_ms) = evaluate_low_agl_persistence(samples, edge_at);

    // Test: sustained_ground_contact (MSFS-relevant)
    let (sustained_pass, sustained_ms) = evaluate_sustained_ground(samples, edge_at);

    // Test: vs_negative_at_impact (beide)
    let vs_negative_pass = impact_frame_vs < -10.0;

    let detail = ValidationDetail {
        sim,
        gear_force_threshold_n: threshold_n,
        gear_force_pass,
        gear_force_peak_in_window_n: gear_force_peak,
        gear_force_sustained_ms,
        g_force_pass: Some(g_force_pass),
        g_force_peak_in_window: g_force_peak,
        low_agl_persistence_pass: low_agl_pass,
        low_agl_actual_ms: low_agl_ms,
        sustained_ground_pass: Some(sustained_pass),
        sustained_ground_actual_ms: sustained_ms,
        vs_negative_pass,
        vs_at_impact_used_for_test: impact_frame_vs,
    };

    // ── Category-aware validation (rotorcraft / seaplane) ──────────────────
    // Real helicopter and seaplane touchdowns are deliberately near-zero V/S
    // with NO gear-force or G-force spike: the FAA Helicopter Flying Handbook
    // has the pilot cushion the set-down with collective to "the slowest rate
    // possible", and a glassy-water seaplane landing is a constant-attitude
    // soft contact (AOPA: "wait for contact … never flare"). The fixed-wing
    // anchors below (gear-force MUST-PASS, g>1.05, vs<-10) therefore REJECT a
    // clean soft set-down as a FalseEdge — which is exactly why these
    // categories were silently dropped. For them we anchor on PRESENCE
    // instead: sustained low-AGL (<5 ft for ≥1000 ms) plus, for rotorcraft,
    // sustained ground contact (≥500 ms). A single glitched on-ground/low-AGL
    // tick CANNOT satisfy a sustained-1000 ms window, so phantom-touchdown
    // protection is fully preserved. Gated on category ⇒ fixed-wing
    // validation is byte-for-byte unchanged.
    if category.is_non_conventional() {
        let presence_ok = if category.water_capable() {
            // Water: `on_ground` stays false while floating, so the sustained
            // low-AGL window (AGL≈0 = on the water surface) confirms the
            // aircraft is ON the surface. But sustained low-AGL ALONE is also
            // satisfied by a level low pass / step-taxi skim / glassy go-around
            // that lingers near the water WITHOUT landing, so we ALSO require a
            // genuine descent onto the surface (impact V/S a clear sink). The
            // two together separate a settling water touchdown from non-landing
            // low flight — without the descent gate any sustained <5 ft water
            // flight would phantom-validate as a touchdown (review finding).
            low_agl_pass && impact_frame_vs < WATER_TOUCHDOWN_MIN_DESCENT_FPM
        } else {
            // Rotorcraft: skids/wheels assert `on_ground`, so require BOTH the
            // sustained ground contact and the sustained low-AGL window.
            low_agl_pass && sustained_pass
        };
        return if presence_ok {
            ValidationResult::Validated { result: detail }
        } else {
            ValidationResult::FalseEdge {
                reason: FalseEdgeReason::InsufficientVoteScore,
                result: detail,
            }
        };
    }

    if sim.is_xplane() {
        // Sonderfall: Wenn der Sample-Buffer KEIN gear_force enthaelt
        // (= alle samples haben gear_normal_force_n = None — passiert bei
        // legacy JSONLs vor v0.7.0 ODER wenn ein X-Plane-Addon das DataRef
        // nicht setzt), fallback auf MSFS-style Voting.
        let any_gear_force_data = samples
            .iter()
            .any(|s| s.gear_normal_force_n.is_some());

        if any_gear_force_data {
            // Echte X-Plane-Validation mit gear_force als MUST-PASS
            if !gear_force_pass {
                return ValidationResult::FalseEdge {
                    reason: FalseEdgeReason::GearForceBelowThreshold,
                    result: detail,
                };
            }
            if !low_agl_pass || !vs_negative_pass {
                return ValidationResult::FalseEdge {
                    reason: FalseEdgeReason::GearForceBelowThreshold,
                    result: detail,
                };
            }
            return ValidationResult::Validated { result: detail };
        }
        // Fallback ohne gear_force: 4-of-4 Voting (= strenger als MSFS
        // damit X-Plane edge-trigger-happy-Streifschuesse nicht durch).
        // Plus: agl_persistence ist hier kritisch weil g_force-spike bei
        // X-Plane ohne gear_force evtl unzuverlaessig
        let passes = [g_force_pass, sustained_pass, low_agl_pass, vs_negative_pass]
            .iter()
            .filter(|p| **p)
            .count();
        if passes >= 4 {
            ValidationResult::Validated { result: detail }
        } else {
            ValidationResult::FalseEdge {
                reason: FalseEdgeReason::InsufficientVoteScore,
                result: detail,
            }
        }
    } else {
        // MSFS / Off: 4 Tests, mind. 3 PASS = VALIDATED.
        let passes = [g_force_pass, sustained_pass, low_agl_pass, vs_negative_pass]
            .iter()
            .filter(|p| **p)
            .count();
        if passes >= 3 {
            ValidationResult::Validated { result: detail }
        } else {
            ValidationResult::FalseEdge {
                reason: FalseEdgeReason::InsufficientVoteScore,
                result: detail,
            }
        }
    }
}

/// gear_force-impact Evaluation (X-Plane).
/// Returns (pass, peak_in_window, sustained_ms_above_threshold).
///
/// Confirmation-Window: Force ueber threshold fuer mind. 60ms anhaltend
/// (gemessen via Timestamps), mit mind. 2 distinct samples (Anti-Glitch).
fn evaluate_gear_force_test(
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
    threshold_n: f32,
) -> (bool, Option<f32>, Option<u64>) {
    let window_end = edge_at + Duration::milliseconds(500);

    // Sammle alle Samples im Window in Reihenfolge (sorted nach `at`).
    let in_window: Vec<&TouchdownWindowSample> = samples
        .iter()
        .filter(|s| s.at >= edge_at && s.at <= window_end)
        .collect();

    let peak_in_window = in_window
        .iter()
        .filter_map(|s| s.gear_normal_force_n)
        .filter(|f| f.is_finite())
        .fold(None::<f32>, |acc, f| Some(acc.map(|a| a.max(f)).unwrap_or(f)));

    // P2-Fix: Suche den LAENGSTEN CONTINUOUS RUN von samples mit
    // gear_force >= threshold. Reset bei Gap (= sample mit force < threshold
    // ODER missing force value).
    //
    // Vorher (BUG): erste/letzte above-sample-Span - das counted Spans mit
    // Luecken in der Mitte als sustained, was die Spec widerspricht.
    let mut best_run_ms: u64 = 0;
    let mut best_run_count: usize = 0;
    let mut current_run_start: Option<DateTime<Utc>> = None;
    let mut current_run_count: usize = 0;

    for s in &in_window {
        let above = s
            .gear_normal_force_n
            .map(|f| f.is_finite() && f >= threshold_n)
            .unwrap_or(false);
        if above {
            if current_run_start.is_none() {
                current_run_start = Some(s.at);
                current_run_count = 1;
            } else {
                current_run_count += 1;
            }
            // Update best wenn dieser run laenger
            let run_ms = (s.at - current_run_start.unwrap()).num_milliseconds().max(0) as u64;
            if run_ms > best_run_ms || (run_ms == best_run_ms && current_run_count > best_run_count) {
                best_run_ms = run_ms;
                best_run_count = current_run_count;
            }
        } else {
            // Gap → reset current run
            current_run_start = None;
            current_run_count = 0;
        }
    }

    let pass = best_run_ms >= 60 && best_run_count >= 2;
    (pass, peak_in_window, Some(best_run_ms))
}

/// peak g_force im Window [edge_at, edge_at + 500ms]
fn evaluate_g_force_peak(samples: &[TouchdownWindowSample], edge_at: DateTime<Utc>) -> f32 {
    let window_end = edge_at + Duration::milliseconds(500);
    samples
        .iter()
        .filter(|s| s.at >= edge_at && s.at <= window_end)
        .map(|s| s.g_force)
        .filter(|g| g.is_finite())
        .fold(0.0_f32, f32::max)
}

/// low_agl_persistence: agl_ft < 5.0 fuer mind. 1000ms ab edge_at.
/// Returns (pass, actual_ms_below_5ft).
fn evaluate_low_agl_persistence(
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
) -> (bool, u64) {
    let target_dur = Duration::milliseconds(1000);
    let window_end = edge_at + target_dur;

    // Suche erste violation (agl >= 5.0) im Target-Window
    let mut first_violation_at: Option<DateTime<Utc>> = None;
    for s in samples.iter().filter(|s| s.at >= edge_at && s.at <= window_end) {
        if s.agl_ft >= 5.0 {
            first_violation_at = Some(s.at);
            break;
        }
    }

    match first_violation_at {
        None => {
            // Keine violation im Window — PASS
            (true, target_dur.num_milliseconds() as u64)
        }
        Some(t) => {
            let actual_ms = (t - edge_at).num_milliseconds().max(0) as u64;
            (actual_ms >= 1000, actual_ms)
        }
    }
}

/// sustained_ground_contact: on_ground=True fuer mind. 500ms continuous.
/// Returns (pass, actual_continuous_ms).
fn evaluate_sustained_ground(
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
) -> (bool, u64) {
    let mut last_at = edge_at;
    let mut found_break = false;

    for s in samples.iter().filter(|s| s.at >= edge_at) {
        if !s.on_ground {
            found_break = true;
            break;
        }
        last_at = s.at;
    }

    let dur_ms = (last_at - edge_at).num_milliseconds().max(0) as u64;
    let pass = !found_break || dur_ms >= 500;
    (pass, dur_ms)
}

// ─── Layer 3: VS-Calculation am IMPACT-Frame ──────────────────────────────

/// Drei Frames die wir aus dem Buffer extrahieren (siehe Spec 5.1):
///   contact_frame:      = candidate.edge_sample_index (von Layer 1)
///   impact_frame:       min vs in [contact-250ms, contact+100ms] = raw härteste Sink
///   initial_load_peak:  max gear_force/g_force in [contact, contact+500ms]
///   episode_load_peak:  max in ganzer Episode (kommt von Layer 4, nicht hier)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImpactFrameResult {
    pub contact_at: DateTime<Utc>,
    pub impact_at: DateTime<Utc>,
    pub impact_vs_fpm: f32,
    pub initial_load_peak_n: Option<f32>,   // X-Plane
    pub initial_load_peak_g: f32,           // beide
}

/// Berechne impact_frame + initial_load_peak aus dem Buffer um den contact_frame.
/// NaN-safe: nur finite vs_fpm Samples + total_cmp().
pub fn compute_impact_frame(
    samples: &[TouchdownWindowSample],
    contact_at: DateTime<Utc>,
) -> Option<ImpactFrameResult> {
    // impact_frame = min vs in [contact-250ms, contact+100ms]
    let window_start = contact_at - Duration::milliseconds(250);
    let window_end = contact_at + Duration::milliseconds(100);

    let impact_sample = samples
        .iter()
        .filter(|s| s.at >= window_start && s.at <= window_end && s.vs_fpm.is_finite())
        .min_by(|a, b| a.vs_fpm.total_cmp(&b.vs_fpm))?;

    // initial_load_peak: max in [contact, contact+500ms]
    let load_window_end = contact_at + Duration::milliseconds(500);
    let load_window: Vec<&TouchdownWindowSample> = samples
        .iter()
        .filter(|s| s.at >= contact_at && s.at <= load_window_end)
        .collect();

    let initial_load_peak_n = load_window
        .iter()
        .filter_map(|s| s.gear_normal_force_n)
        .filter(|f| f.is_finite())
        .fold(None::<f32>, |acc, f| {
            Some(acc.map(|a| a.max(f)).unwrap_or(f))
        });

    let initial_load_peak_g = load_window
        .iter()
        .map(|s| s.g_force)
        .filter(|g| g.is_finite())
        .fold(0.0_f32, f32::max);

    Some(ImpactFrameResult {
        contact_at,
        impact_at: impact_sample.at,
        impact_vs_fpm: impact_sample.vs_fpm,
        initial_load_peak_n,
        initial_load_peak_g,
    })
}

// ─── Layer 3 Cont: VS-Cascade + HARD GUARDS ───────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Confidence {
    High,
    Medium,
    Low,
    VeryLow,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingRateResult {
    pub vs_fpm: f32,
    pub source: String,
    pub confidence: Confidence,
    pub forensics_version: u8,
    pub contact_at: DateTime<Utc>,
    pub impact_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RejectionReason {
    EmptyWindow,
    AllSourcesPositive,
    PositiveVs,
    ImplausiblyHigh,
}

/// HARD GUARD: niemals positiv, niemals < -3000 fpm
fn finalize_vs(candidate_fpm: f32) -> Result<f32, RejectionReason> {
    if !candidate_fpm.is_finite() {
        return Err(RejectionReason::EmptyWindow);
    }
    if candidate_fpm > 0.0 {
        return Err(RejectionReason::PositiveVs);
    }
    if candidate_fpm < -3000.0 {
        return Err(RejectionReason::ImplausiblyHigh);
    }
    Ok(candidate_fpm)
}

/// Compute the final landing rate using the sim-agnostic cascade.
/// Cascade priority: vs_at_impact → smoothed_500ms → smoothed_1000ms →
/// pre_flare_peak → REJECT.
pub fn compute_landing_rate(
    samples: &[TouchdownWindowSample],
    impact_result: &ImpactFrameResult,
    category: AircraftCategory,
) -> Result<LandingRateResult, RejectionReason> {
    let impact_at = impact_result.impact_at;
    let vs_at_impact = impact_result.impact_vs_fpm;

    // Smoothed averages around impact_frame
    let vs_smoothed_500 = avg_vs_in_window(samples, impact_at, -500, 0);
    let vs_smoothed_1000 = avg_vs_in_window(samples, impact_at, -1000, 0);
    let pre_flare_peak = min_vs_in_window(samples, impact_at, -3000, -500);

    // Helicopters / seaplanes touch down at a deliberately near-zero V/S
    // (collective cushion / glassy-water contact), so the -10 fpm fixed-wing
    // acceptance floor would reject a real soft set-down and yield no landing
    // rate. Use a near-zero floor for these categories; `finalize_vs` still
    // rejects any non-negative rate, so a level/climbing sample never produces
    // a landing. Fixed-wing keeps the -10 fpm floor unchanged.
    let floor = if category.is_non_conventional() {
        0.0
    } else {
        -10.0
    };

    let chosen = if vs_at_impact < floor {
        (vs_at_impact, "vs_at_impact_frame", Confidence::High)
    } else if vs_smoothed_500.map(|v| v < floor).unwrap_or(false) {
        (
            vs_smoothed_500.unwrap(),
            "vs_smoothed_500ms_at_impact",
            Confidence::Medium,
        )
    } else if vs_smoothed_1000.map(|v| v < floor).unwrap_or(false) {
        (
            vs_smoothed_1000.unwrap(),
            "vs_smoothed_1000ms_at_impact",
            Confidence::Low,
        )
    } else if pre_flare_peak.map(|v| v < 0.0).unwrap_or(false) {
        (
            pre_flare_peak.unwrap(),
            "pre_flare_peak",
            Confidence::VeryLow,
        )
    } else {
        return Err(RejectionReason::AllSourcesPositive);
    };

    let final_vs = finalize_vs(chosen.0)?;

    Ok(LandingRateResult {
        vs_fpm: final_vs,
        source: chosen.1.to_string(),
        confidence: chosen.2,
        forensics_version: FORENSICS_VERSION,
        contact_at: impact_result.contact_at,
        impact_at,
    })
}

// ─── Layer 4: LandingEpisode + Episode-Klassifizierung ────────────────────

/// Snapshot des Aircraft-Zustands zum Zeitpunkt eines TD-Contacts.
/// Wird einmal pro Episode beim ersten validated contact festgehalten —
/// wird fuer mass-aware threshold + deterministische Replays gebraucht.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AircraftStateSnapshot {
    pub total_weight_kg: Option<f32>,
    pub sim: SimKind,
}

/// Eine TD-Candidate die Validation gefailt hat (Float/Streifschuss).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FalseEdge {
    pub edge_at: DateTime<Utc>,
    pub edge_agl_ft: f32,
    pub edge_vs_fpm: f32,
    pub reason: FalseEdgeReason,
    pub validation: ValidationDetail,
}

/// Echter erster Bodenkontakt einer Episode (validated).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactDetail {
    pub contact_at: DateTime<Utc>,
    pub impact_at: DateTime<Utc>,
    pub vs_at_impact_fpm: f32,
    pub vs_at_contact_fpm: f32,
    pub agl_at_contact_ft: f32,
    pub validation: ValidationDetail,
    pub initial_load_peak_n: Option<f32>,
    pub initial_load_peak_g: f32,
    pub confidence: Confidence,
    pub source: String,
}

/// Ein nachfolgender low-level Touch innerhalb derselben Episode
/// (= aircraft bleibt unter 50ft AGL, kein climb-out).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LowLevelTouch {
    pub at: DateTime<Utc>,
    pub vs_at_impact_fpm: f32,
    pub agl_max_ft: f32,
    pub sustained_ms: u64,
}

/// Wann + wie eine Episode in den finalen Settle uebergeht.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettleDetail {
    pub settle_at: DateTime<Utc>,
    pub final_groundspeed_kt: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum HardestImpactSource {
    Contact,
    LowLevelTouch(u8),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeClass {
    /// aircraft blieb am Boden, gs sinkt — Pilot ist gelandet
    FinalLanding,
    /// aircraft hob nach Touch wieder ab, stieg auf 100-1000ft AGL,
    /// kam zurueck — Pattern-Flug
    TouchAndGo,
    /// aircraft stieg > 1000ft AGL nach Touch — Go-Around
    GoAround,
    /// noch nicht klassifiziert (Episode laeuft noch)
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LandingEpisode {
    pub episode_index: u8,
    pub aircraft_state_at_contact: AircraftStateSnapshot,
    pub false_edges: Vec<FalseEdge>,
    pub contact: ContactDetail,
    pub low_level_touches: Vec<LowLevelTouch>,
    pub settle: Option<SettleDetail>,
    /// max gear_force / g_force innerhalb der GANZEN Episode (incl. rollout) —
    /// Forensik-only, NICHT fuer Score
    pub episode_load_peak_n: Option<f32>,
    pub episode_load_peak_g: f32,
    pub hardest_impact_vs_fpm: f32,
    pub hardest_impact_source: HardestImpactSource,
    pub classification: EpisodeClass,
}

/// Kontext fuer Episode-Klassifizierung — Werte kommen aus der laufenden
/// Sampler-State-Machine (Layer 4 caller).
#[derive(Debug, Clone, Copy)]
pub struct EpisodePostContactState {
    /// Maximum AGL nach contact (in der ganzen post-contact Periode).
    pub max_agl_ft_after_contact: f32,
    /// Hat aircraft fuer >= 30s unter 50ft AGL geblieben mit gs<30kt?
    pub settled_under_50ft_for_30s: bool,
    /// Aktuelle groundspeed (fuer Settle-Detection)
    pub current_gs_kt: f32,
}

/// Klassifiziere eine Episode basierend auf was nach dem contact passiert ist.
/// Spec Sektion 6.2 (FinalLanding) + 6.4 (DAH 3181 Beispiel).
pub fn classify_episode(state: EpisodePostContactState) -> EpisodeClass {
    if state.max_agl_ft_after_contact > 1000.0 {
        EpisodeClass::GoAround
    } else if state.max_agl_ft_after_contact > 100.0 {
        EpisodeClass::TouchAndGo
    } else if state.settled_under_50ft_for_30s {
        EpisodeClass::FinalLanding
    } else {
        EpisodeClass::Pending
    }
}

/// Bestimme den haertesten Impact innerhalb einer Episode (= Bounce-Score-Regel).
/// Spec Sektion 6.5: härtester Impact = min vs_at_impact (= numerisch kleinster
/// = haertester Sink) zwischen contact + allen low_level_touches.
pub fn compute_hardest_impact(
    contact_vs: f32,
    low_level_touches: &[LowLevelTouch],
) -> (f32, HardestImpactSource) {
    let mut hardest = contact_vs;
    let mut source = HardestImpactSource::Contact;

    for (i, touch) in low_level_touches.iter().enumerate() {
        if touch.vs_at_impact_fpm < hardest {
            hardest = touch.vs_at_impact_fpm;
            source = HardestImpactSource::LowLevelTouch(i as u8);
        }
    }

    (hardest, source)
}

// ─── Helpers ──────────────────────────────────────────────────────────────

fn avg_vs_in_window(
    samples: &[TouchdownWindowSample],
    center: DateTime<Utc>,
    delta_start_ms: i64,
    delta_end_ms: i64,
) -> Option<f32> {
    let window_start = center + Duration::milliseconds(delta_start_ms);
    let window_end = center + Duration::milliseconds(delta_end_ms);

    let values: Vec<f32> = samples
        .iter()
        .filter(|s| s.at >= window_start && s.at <= window_end && s.vs_fpm.is_finite())
        .map(|s| s.vs_fpm)
        .collect();

    if values.is_empty() {
        None
    } else {
        Some(values.iter().sum::<f32>() / values.len() as f32)
    }
}

fn min_vs_in_window(
    samples: &[TouchdownWindowSample],
    center: DateTime<Utc>,
    delta_start_ms: i64,
    delta_end_ms: i64,
) -> Option<f32> {
    let window_start = center + Duration::milliseconds(delta_start_ms);
    let window_end = center + Duration::milliseconds(delta_end_ms);

    samples
        .iter()
        .filter(|s| s.at >= window_start && s.at <= window_end && s.vs_fpm.is_finite())
        .map(|s| s.vs_fpm)
        .min_by(|a, b| a.total_cmp(b))
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gear_force_threshold_floor_for_glider() {
        // Cessna 152 weight 757kg → dynamic = 757*9.80665*0.03 = 222.7N → floor wins
        let t = gear_force_threshold_n(Some(757.0));
        assert_eq!(t, 1000.0);
    }

    #[test]
    fn gear_force_threshold_dynamic_for_a330() {
        // DAH 3181 A330 250t → 250000*9.80665*0.03 = ~73550N
        let t = gear_force_threshold_n(Some(250000.0));
        assert!(
            (t - 73549.875).abs() < 1.0,
            "expected ~73550, got {}",
            t
        );
    }

    #[test]
    fn gear_force_threshold_floor_for_no_weight() {
        let t = gear_force_threshold_n(None);
        assert_eq!(t, 1000.0);
    }

    #[test]
    fn gear_force_threshold_floor_for_zero_weight() {
        let t = gear_force_threshold_n(Some(0.0));
        assert_eq!(t, 1000.0);
    }

    #[test]
    fn finalize_vs_rejects_positive() {
        assert_eq!(finalize_vs(100.0), Err(RejectionReason::PositiveVs));
    }

    #[test]
    fn finalize_vs_rejects_implausibly_high() {
        assert_eq!(
            finalize_vs(-3500.0),
            Err(RejectionReason::ImplausiblyHigh)
        );
    }

    #[test]
    fn finalize_vs_rejects_nan() {
        assert_eq!(finalize_vs(f32::NAN), Err(RejectionReason::EmptyWindow));
    }

    #[test]
    fn finalize_vs_accepts_typical_landing() {
        assert_eq!(finalize_vs(-150.0), Ok(-150.0));
    }

    // ── Category-aware validation + landing-rate (rotorcraft / seaplane) ──

    fn cat_sample(
        at: DateTime<Utc>,
        agl_ft: f32,
        on_ground: bool,
        vs_fpm: f32,
        g_force: f32,
    ) -> TouchdownWindowSample {
        TouchdownWindowSample {
            at,
            vs_fpm,
            g_force,
            on_ground,
            agl_ft,
            heading_true_deg: 0.0,
            groundspeed_kt: 0.0,
            indicated_airspeed_kt: 0.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: Some(1100.0),
        }
    }

    /// 1200 ms of on-surface samples from `edge` (AGL≈1 ft, near-zero V/S, no
    /// G-spike) plus a matching candidate. `on_ground` toggles the wheeled
    /// (helicopter skids) vs water (seaplane) case.
    fn soft_setdown(
        edge: DateTime<Utc>,
        on_ground: bool,
    ) -> (Vec<TouchdownWindowSample>, TdCandidate) {
        let mut samples = Vec::new();
        let mut t = edge;
        let end = edge + Duration::milliseconds(1200);
        while t <= end {
            samples.push(cat_sample(t, 1.0, on_ground, -3.0, 1.0));
            t = t + Duration::milliseconds(20);
        }
        let cand = TdCandidate {
            edge_sample_index: 0,
            edge_at: edge,
            edge_agl_ft: 1.0,
            edge_vs_fpm: -3.0,
            edge_gear_force_n: None,
            edge_g_force: 1.0,
            edge_total_weight_kg: Some(1100.0),
        };
        (samples, cand)
    }

    #[test]
    fn heli_soft_setdown_rejected_as_fixed_wing_but_validated_as_heli() {
        let edge = Utc::now();
        let (samples, cand) = soft_setdown(edge, true);
        // Fixed-wing: no G-spike, V/S > -10 → only 2/4 votes → FalseEdge.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -3.0,
                AircraftCategory::FixedWing
            ),
            ValidationResult::FalseEdge { .. }
        ));
        // Helicopter: sustained low-AGL + sustained ground = presence ⇒ Validated.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -3.0,
                AircraftCategory::Helicopter
            ),
            ValidationResult::Validated { .. }
        ));
    }

    #[test]
    fn seaplane_water_contact_validates_without_on_ground() {
        let edge = Utc::now();
        // on_ground stays FALSE — the water case.
        let (samples, cand) = soft_setdown(edge, false);
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -120.0,
                AircraftCategory::FixedWing
            ),
            ValidationResult::FalseEdge { .. }
        ));
        // Seaplane: sustained low-AGL (= on the water) carries it without on_ground.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -120.0,
                AircraftCategory::Seaplane
            ),
            ValidationResult::Validated { .. }
        ));
    }

    #[test]
    fn seaplane_level_low_pass_not_validated_phantom_guard() {
        // Phantom guard (review finding): a seaplane LINGERING in the water-
        // contact band (sustained low-AGL) WITHOUT a genuine descent — a level
        // low pass / step-taxi skim / glassy go-around — must NOT validate as a
        // water touchdown. With impact V/S ≈ 0 the descent gate rejects it even
        // though the low-AGL window is satisfied.
        let edge = Utc::now();
        let (samples, cand) = soft_setdown(edge, false); // on_ground=false (water)
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -5.0, // near-zero sink = not a landing
                AircraftCategory::Seaplane
            ),
            ValidationResult::FalseEdge { .. }
        ));
        // A genuine descending water touchdown (clear sink past the gate) DOES
        // validate — the gate distinguishes landing from non-landing low flight.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -120.0,
                AircraftCategory::Seaplane
            ),
            ValidationResult::Validated { .. }
        ));
    }

    #[test]
    fn single_low_agl_glitch_tick_not_validated_for_heli() {
        // Phantom protection: one low-AGL tick at the edge, then back to cruise.
        let edge = Utc::now();
        let mut samples = vec![cat_sample(edge, 1.0, true, -3.0, 1.0)];
        let mut t = edge + Duration::milliseconds(20);
        let end = edge + Duration::milliseconds(1200);
        while t <= end {
            samples.push(cat_sample(t, 9000.0, false, 0.0, 1.0));
            t = t + Duration::milliseconds(20);
        }
        let cand = TdCandidate {
            edge_sample_index: 0,
            edge_at: edge,
            edge_agl_ft: 1.0,
            edge_vs_fpm: -3.0,
            edge_gear_force_n: None,
            edge_g_force: 1.0,
            edge_total_weight_kg: Some(1100.0),
        };
        // Not sustained ⇒ low-AGL fails ⇒ FalseEdge even for a helicopter.
        assert!(matches!(
            validate_candidate(
                &cand,
                &samples,
                SimKind::Msfs2024,
                -3.0,
                AircraftCategory::Helicopter
            ),
            ValidationResult::FalseEdge { .. }
        ));
    }

    #[test]
    fn landing_rate_near_zero_rejected_fixed_wing_accepted_heli() {
        let base = Utc::now();
        let mut samples = Vec::new();
        // Pre-flare window [base-3000, base-500): hovering, V/S = 0.
        let mut t = base - Duration::milliseconds(3000);
        while t < base - Duration::milliseconds(500) {
            samples.push(cat_sample(t, 2.0, false, 0.0, 1.0));
            t = t + Duration::milliseconds(50);
        }
        // Around impact: a gentle -5 fpm settle.
        let mut t = base - Duration::milliseconds(400);
        let end = base + Duration::milliseconds(100);
        while t <= end {
            samples.push(cat_sample(t, 0.5, true, -5.0, 1.0));
            t = t + Duration::milliseconds(20);
        }
        let impact = ImpactFrameResult {
            contact_at: base,
            impact_at: base,
            impact_vs_fpm: -5.0,
            initial_load_peak_n: None,
            initial_load_peak_g: 1.0,
        };
        // Fixed-wing: -5 fpm is above the -10 floor at every tier and the
        // pre-flare window has no sink → rejected.
        assert!(matches!(
            compute_landing_rate(&samples, &impact, AircraftCategory::FixedWing),
            Err(RejectionReason::AllSourcesPositive)
        ));
        // Helicopter: near-zero floor accepts the real -5 fpm impact.
        let heli = compute_landing_rate(&samples, &impact, AircraftCategory::Helicopter)
            .expect("helicopter near-zero landing accepted");
        assert!((heli.vs_fpm - (-5.0)).abs() < 0.01);
    }

    fn touch(vs: f32) -> LowLevelTouch {
        LowLevelTouch {
            at: Utc::now(),
            vs_at_impact_fpm: vs,
            agl_max_ft: 3.0,
            sustained_ms: 200,
        }
    }

    #[test]
    fn hardest_impact_no_bounces_returns_contact() {
        let (vs, src) = compute_hardest_impact(-300.0, &[]);
        assert_eq!(vs, -300.0);
        assert_eq!(src, HardestImpactSource::Contact);
    }

    #[test]
    fn hardest_impact_contact_harder_than_bounce() {
        // PTO 705 Pattern: contact -182, low_level -61 → hardest = -182 (contact)
        let (vs, src) = compute_hardest_impact(-182.0, &[touch(-61.0)]);
        assert_eq!(vs, -182.0);
        assert_eq!(src, HardestImpactSource::Contact);
    }

    #[test]
    fn hardest_impact_bounce_harder_than_contact() {
        // Hard-Bounce-Pattern: contact -200, bounce -600 → hardest = -600 (bounce)
        let (vs, src) = compute_hardest_impact(-200.0, &[touch(-100.0), touch(-600.0)]);
        assert_eq!(vs, -600.0);
        assert_eq!(src, HardestImpactSource::LowLevelTouch(1));
    }

    #[test]
    fn classify_final_landing() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 30.0,
            settled_under_50ft_for_30s: true,
            current_gs_kt: 15.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::FinalLanding);
    }

    #[test]
    fn classify_touch_and_go_pattern() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 800.0,
            settled_under_50ft_for_30s: false,
            current_gs_kt: 90.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::TouchAndGo);
    }

    #[test]
    fn classify_go_around() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 1500.0,
            settled_under_50ft_for_30s: false,
            current_gs_kt: 130.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::GoAround);
    }

    fn make_sample(at_ms: i64, gear_n: Option<f32>) -> TouchdownWindowSample {
        let at = DateTime::<Utc>::from_timestamp_millis(at_ms).unwrap();
        TouchdownWindowSample {
            at,
            vs_fpm: -200.0,
            g_force: 1.2,
            on_ground: true,
            agl_ft: 1.0,
            heading_true_deg: 0.0,
            groundspeed_kt: 100.0,
            indicated_airspeed_kt: 100.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            gear_normal_force_n: gear_n,
            total_weight_kg: Some(73000.0), // A320-ish → threshold ≈ 21478 N
        }
    }

    #[test]
    fn gear_force_continuous_pass_when_sustained() {
        // 5 samples a 20ms (= 80ms span), alle ueber threshold (50000 N > 21478)
        let samples: Vec<TouchdownWindowSample> = (0..5)
            .map(|i| make_sample(1000 + i * 20, Some(50000.0)))
            .collect();
        let edge_at = samples[0].at;
        let (pass, peak, sustained) = evaluate_gear_force_test(&samples, edge_at, 21478.0);
        assert!(pass, "5 consecutive samples should pass");
        assert_eq!(peak, Some(50000.0));
        assert!(sustained.unwrap() >= 60);
    }

    #[test]
    fn gear_force_continuous_fail_with_gap_in_middle() {
        // P2-Fix: Sample 0 above, 1 below, 2 above → run-laenge = 1 sample.
        // Vorher (BUG) waere span 0→2 = 40ms = pass. Jetzt: korrekt fail.
        let samples = vec![
            make_sample(1000, Some(50000.0)),  // above
            make_sample(1020, Some(100.0)),    // below threshold (gap!)
            make_sample(1040, Some(50000.0)),  // above wieder
        ];
        let edge_at = samples[0].at;
        let (pass, _peak, sustained) = evaluate_gear_force_test(&samples, edge_at, 21478.0);
        // Best run hat nur 1 sample bzw 0ms span -> fail
        assert!(!pass, "gap in middle must NOT count as sustained, got sustained={:?}", sustained);
    }

    #[test]
    fn gear_force_continuous_pass_when_long_run_after_gap() {
        // Sample 0 above (single), gap, dann 5 samples sustained → pass
        let mut samples = vec![
            make_sample(1000, Some(50000.0)),  // single above
            make_sample(1020, Some(100.0)),    // gap
        ];
        for i in 0..5 {
            samples.push(make_sample(1100 + i * 20, Some(50000.0)));
        }
        let edge_at = samples[0].at;
        let (pass, _peak, _sustained) = evaluate_gear_force_test(&samples, edge_at, 21478.0);
        assert!(pass, "long sustained run after gap should pass");
    }

    #[test]
    fn classify_pending_low_agl_not_settled_yet() {
        let s = EpisodePostContactState {
            max_agl_ft_after_contact: 20.0,
            settled_under_50ft_for_30s: false,
            current_gs_kt: 80.0,
        };
        assert_eq!(classify_episode(s), EpisodeClass::Pending);
    }
}
