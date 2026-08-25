//! v0.7.19 GAF-707 Crash / Accident Detection — Classifier.
//!
//! Pure functions, sim-neutral. MSFS-`Crashed`-Event und X-Plane laufen
//! gegen denselben `classify_accident_*`-Pfad. Sim-spezifische Crash-
//! Events liefern nur Confidence-Boost / Kind-Override, sie sind kein
//! Pflicht-Signal.
//!
//! Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md.
//!
//! Lifecycle (Spec §Klassifikator-Trigger-Punkte):
//! - **Sim-Event-Pfad:** wird im aktiven Flug aufgerufen sobald
//!   `snap.crashed` von false→true flippt. Setzt sofort
//!   `accident_detected=true`. Nicht in dieser Crate — Caller in
//!   `lib.rs`/step_flight.
//! - **Heuristik-Pfad:** `classify_accident_heuristic` wird EINMAL pro
//!   Touchdown am TD-Edge aufgerufen (in build_landing_record).
//!   Nicht im 50-Hz-Loop, sonst Flicker.

use serde::Serialize;

/// Sentinel-String der Webapp signalisiert "Client hat klassifiziert".
/// Wird in `TouchdownPayload.accident_classifier_version` gesetzt —
/// auch wenn kein Accident erkannt wurde. So unterscheidet die Webapp
/// "Classifier lief, false" von "historischer Payload, bitte
/// nachklassifizieren".
pub const ACCIDENT_CLASSIFIER_VERSION: &str = "v0.7.19";

/// Ergebnis einer Accident-Klassifikation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum AccidentDecision {
    /// Kein Accident-Verdacht.
    None,
    /// Indizien vorhanden, aber kein harter Confirm. UI zeigt Warnung,
    /// Filing bleibt nicht-clean, Pilot wird gefragt.
    Suspected {
        kind: AccidentKind,
        reasons: Vec<String>,
    },
    /// Harter Confirm. Filing-Pfad wird auf Accident/Review umgeschaltet,
    /// Pilot bestaetigt im Dialog.
    Confirmed {
        kind: AccidentKind,
        reasons: Vec<String>,
    },
}

/// Akut-Klasse des Accidents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum AccidentKind {
    /// Simulator hat ein Crash-System-Event gefeuert (`MSFS Crashed`).
    SimCrash,
    /// Heuristik aus Telemetrie: extremer Impact (V/S, G, Sideslip).
    Impact,
    /// Off-Runway-Impact: kein Runway-Match + harter Aufschlag +
    /// Zusatzmarker (Stall, Wing-Strike, kurzer Rollout).
    OffAirportImpact,
}

impl AccidentKind {
    /// Wire-String fuer Payloads/PIREP-Notes.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            Self::SimCrash => "sim_crash",
            Self::Impact => "impact",
            Self::OffAirportImpact => "off_airport_impact",
        }
    }
}

/// Eingabe-Bundle fuer den Heuristik-Klassifikator. Sim-neutral,
/// aus FlightStats/landing_analysis am TD-Edge zusammengestellt.
///
/// Alle Felder Option-wrapped: fehlt ein Wert (alte Pre-v0.5.x-Payloads,
/// X-Plane ohne Sideslip etc.), wird der entsprechende Marker einfach
/// nicht gezaehlt. Spec §Anti-False-Positive: kein Accident aus zu
/// duennen Daten.
#[derive(Debug, Clone, Default)]
pub struct AccidentHeuristicInput {
    /// Interpolierter V/S exakt am Touchdown-Frame (fpm). Erwartet
    /// negativ bei realer Landung; classifier nimmt `.abs()`.
    pub vs_at_edge_fpm: Option<f32>,
    /// Peak G im post-TD-Window. > 2.0 = strukturell auffaellig.
    pub peak_g_load: Option<f32>,
    /// Sideslip beim Aufsetzen (deg). Hoch = unkontrollierter Kontakt.
    pub sideslip_deg: Option<f32>,
    /// `|bank_at_td| / aircraft_max_bank_deg × 100`. >= 100% = Wing-Strike
    /// wahrscheinlich.
    pub landing_wing_strike_severity_pct: Option<f32>,
    /// Anzahl Stall-Warnings im Approach-Buffer.
    pub approach_stall_warning_count: Option<u32>,
    /// True wenn `RunwayMatch` gefunden wurde. None = Datenpfad lieferte
    /// nichts (alte Records). False = kein Match.
    pub runway_match_found: Option<bool>,
    /// Rollout in Metern. Sehr kurz (<300m) bei Airliner = Crash-Indikator.
    pub rollout_distance_m: Option<f32>,
}

/// Heuristik-Klassifikator. Sim-neutral. Wird EINMAL pro Touchdown am
/// TD-Edge aufgerufen — NICHT im 50-Hz-Loop.
///
/// Schwellen siehe Spec §Detection.
pub fn classify_accident_heuristic(input: &AccidentHeuristicInput) -> AccidentDecision {
    let mut confirmed_reasons: Vec<String> = Vec::new();
    let mut suspected_reasons: Vec<String> = Vec::new();

    let abs_vs = input.vs_at_edge_fpm.map(|v| v.abs());
    let g = input.peak_g_load;
    let abs_sideslip = input.sideslip_deg.map(|s| s.abs());
    let wing = input.landing_wing_strike_severity_pct;
    let stalls = input.approach_stall_warning_count.unwrap_or(0);
    let no_runway = matches!(input.runway_match_found, Some(false));
    let rollout = input.rollout_distance_m;

    // ── Confirmed Path 1: GAF-707-Pattern — extremer Impact ─────────
    // |V/S| >= 1500 fpm UND G >= 3.0
    let extreme_impact = matches!((abs_vs, g), (Some(v), Some(g)) if v >= 1500.0 && g >= 3.0);

    // ── Confirmed Path 2: Off-Airport Impact ────────────────────────
    // |V/S| >= 1000 fpm UND kein Runway-Match UND mind. 2 rote Marker
    let off_airport_markers = count_off_airport_markers(
        g,
        abs_sideslip,
        wing,
        stalls,
        rollout,
    );
    let off_airport_impact = matches!(abs_vs, Some(v) if v >= 1000.0)
        && no_runway
        && off_airport_markers >= 2;

    if extreme_impact || off_airport_impact {
        // Reasons fuer beide Pfade sammeln
        if let Some(v) = abs_vs {
            confirmed_reasons.push(format!("vs_at_edge_fpm={:.1}", -v));
        }
        if let Some(g) = g {
            confirmed_reasons.push(format!("peak_g_load={g:.2}"));
        }
        if let Some(s) = input.sideslip_deg {
            if s.abs() >= 45.0 {
                confirmed_reasons.push(format!("sideslip_deg={s:.1}"));
            }
        }
        if let Some(w) = wing {
            if w >= 100.0 {
                confirmed_reasons.push(format!("wing_strike_severity_pct={w:.1}"));
            }
        }
        if stalls >= 3 {
            confirmed_reasons.push(format!("approach_stall_warning_count={stalls}"));
        }
        if no_runway {
            confirmed_reasons.push("no_runway_match".into());
        }
        if let Some(r) = rollout {
            if r <= 300.0 {
                confirmed_reasons.push(format!("rollout_distance_m={r:.0}"));
            }
        }

        let kind = if extreme_impact {
            AccidentKind::Impact
        } else {
            AccidentKind::OffAirportImpact
        };
        return AccidentDecision::Confirmed {
            kind,
            reasons: confirmed_reasons,
        };
    }

    // ── Suspected Path ──────────────────────────────────────────────
    // |V/S| >= 1000 fpm UND mind. 2 Suspected-Marker
    let suspected_markers = count_suspected_markers(
        g,
        abs_sideslip,
        wing,
        stalls,
        no_runway,
    );
    if matches!(abs_vs, Some(v) if v >= 1000.0) && suspected_markers >= 2 {
        if let Some(v) = abs_vs {
            suspected_reasons.push(format!("vs_at_edge_fpm={:.1}", -v));
        }
        if let Some(g) = g {
            if g >= 2.1 {
                suspected_reasons.push(format!("peak_g_load={g:.2}"));
            }
        }
        if let Some(s) = input.sideslip_deg {
            if s.abs() >= 30.0 {
                suspected_reasons.push(format!("sideslip_deg={s:.1}"));
            }
        }
        if let Some(w) = wing {
            if w >= 75.0 {
                suspected_reasons.push(format!("wing_strike_severity_pct={w:.1}"));
            }
        }
        if stalls >= 1 {
            suspected_reasons.push(format!("approach_stall_warning_count={stalls}"));
        }
        if no_runway {
            suspected_reasons.push("no_runway_match".into());
        }
        return AccidentDecision::Suspected {
            kind: AccidentKind::Impact,
            reasons: suspected_reasons,
        };
    }

    AccidentDecision::None
}

fn count_off_airport_markers(
    g: Option<f32>,
    abs_sideslip: Option<f32>,
    wing: Option<f32>,
    stalls: u32,
    rollout: Option<f32>,
) -> u32 {
    let mut n = 0;
    if matches!(g, Some(g) if g >= 2.5) {
        n += 1;
    }
    if matches!(abs_sideslip, Some(s) if s >= 45.0) {
        n += 1;
    }
    if matches!(wing, Some(w) if w >= 100.0) {
        n += 1;
    }
    if stalls >= 3 {
        n += 1;
    }
    if matches!(rollout, Some(r) if r <= 300.0) {
        n += 1;
    }
    n
}

fn count_suspected_markers(
    g: Option<f32>,
    abs_sideslip: Option<f32>,
    wing: Option<f32>,
    stalls: u32,
    no_runway: bool,
) -> u32 {
    let mut n = 0;
    if matches!(g, Some(g) if g >= 2.1) {
        n += 1;
    }
    if matches!(abs_sideslip, Some(s) if s >= 30.0) {
        n += 1;
    }
    if matches!(wing, Some(w) if w >= 75.0) {
        n += 1;
    }
    if stalls >= 1 {
        n += 1;
    }
    if no_runway {
        n += 1;
    }
    n
}

/// PIREP-Notes-Kurzfassung. Pilot-/VA-Admin-lesbar.
pub fn build_accident_notes(
    kind: AccidentKind,
    confidence: &str,
    reasons: &[String],
) -> String {
    let kind_label = match kind {
        AccidentKind::SimCrash => "simulator crash event",
        AccidentKind::Impact => "impact",
        AccidentKind::OffAirportImpact => "off-airport impact",
    };
    let reasons_str = if reasons.is_empty() {
        "(no reason recorded)".to_string()
    } else {
        reasons.join(", ")
    };
    format!(
        "AeroACARS Accident detected: {kind_label}, {confidence} confidence.\n\
         Reasons: {reasons_str}."
    )
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn gaf707() -> AccidentHeuristicInput {
        // Echte Werte aus flight-gsg-15 (1).json touchdowns[0].payload.
        AccidentHeuristicInput {
            vs_at_edge_fpm: Some(-2249.9),
            peak_g_load: Some(4.41),
            sideslip_deg: Some(-115.6),
            landing_wing_strike_severity_pct: Some(111.5),
            approach_stall_warning_count: Some(5),
            runway_match_found: Some(false),
            rollout_distance_m: Some(205.8),
        }
    }

    #[test]
    fn gaf707_confirmed_impact() {
        let r = classify_accident_heuristic(&gaf707());
        match r {
            AccidentDecision::Confirmed { kind, reasons } => {
                // GAF 707 trifft Confirmed Path 1 (extreme impact) — kind=Impact.
                // Off-airport-Pfad triggert auch, aber Path 1 gewinnt ueber kind.
                assert!(matches!(kind, AccidentKind::Impact));
                assert!(reasons.iter().any(|r| r.contains("peak_g_load=4.41")));
                assert!(reasons
                    .iter()
                    .any(|r| r.contains("vs_at_edge_fpm=-2249.9")));
                assert!(reasons.iter().any(|r| r == "no_runway_match"));
            }
            other => panic!("expected Confirmed(Impact), got {other:?}"),
        }
    }

    #[test]
    fn hard_landing_on_runway_not_accident() {
        let i = AccidentHeuristicInput {
            vs_at_edge_fpm: Some(-700.0),
            peak_g_load: Some(2.5),
            sideslip_deg: Some(5.0),
            landing_wing_strike_severity_pct: Some(10.0),
            approach_stall_warning_count: Some(0),
            runway_match_found: Some(true),
            rollout_distance_m: Some(1800.0),
        };
        let r = classify_accident_heuristic(&i);
        assert_eq!(r, AccidentDecision::None);
    }

    #[test]
    fn smooth_vs_high_g_not_accident() {
        // B-009-Anchor: -116 fpm / 2.30 G muss NICHT als Accident gelten,
        // sonst hat v0.7.17 sub-rollout/G-Spike-Fix vergebliche Arbeit
        // gemacht.
        let i = AccidentHeuristicInput {
            vs_at_edge_fpm: Some(-116.0),
            peak_g_load: Some(2.30),
            sideslip_deg: Some(2.0),
            landing_wing_strike_severity_pct: Some(5.0),
            approach_stall_warning_count: Some(0),
            runway_match_found: Some(true),
            rollout_distance_m: Some(1200.0),
        };
        let r = classify_accident_heuristic(&i);
        assert_eq!(r, AccidentDecision::None);
    }

    #[test]
    fn soft_off_airport_divert_not_accident() {
        // Off-Airport-Notlandung weich: kein Confirm, kein Suspected.
        let i = AccidentHeuristicInput {
            vs_at_edge_fpm: Some(-300.0),
            peak_g_load: Some(1.4),
            sideslip_deg: Some(3.0),
            landing_wing_strike_severity_pct: Some(15.0),
            approach_stall_warning_count: Some(0),
            runway_match_found: Some(false),
            rollout_distance_m: Some(800.0),
        };
        let r = classify_accident_heuristic(&i);
        assert_eq!(r, AccidentDecision::None);
    }

    #[test]
    fn xplane_confirmed_impact_without_sim_event() {
        // X-Plane liefert kein Crashed-Event in v0.7.19. Heuristik
        // muss trotzdem Confirmed-Impact erkennen wenn V/S+G passen.
        let i = AccidentHeuristicInput {
            vs_at_edge_fpm: Some(-1800.0),
            peak_g_load: Some(3.2),
            sideslip_deg: Some(20.0),
            landing_wing_strike_severity_pct: Some(40.0),
            approach_stall_warning_count: Some(0),
            runway_match_found: Some(false),
            rollout_distance_m: Some(400.0),
        };
        let r = classify_accident_heuristic(&i);
        assert!(matches!(r, AccidentDecision::Confirmed { kind: AccidentKind::Impact, .. }));
    }

    #[test]
    fn suspected_accident_review_only() {
        let i = AccidentHeuristicInput {
            vs_at_edge_fpm: Some(-1050.0),
            peak_g_load: Some(2.2),
            sideslip_deg: Some(35.0),
            landing_wing_strike_severity_pct: Some(60.0),
            approach_stall_warning_count: Some(1),
            runway_match_found: Some(false),
            rollout_distance_m: Some(1000.0),
        };
        let r = classify_accident_heuristic(&i);
        match r {
            AccidentDecision::Suspected { reasons, .. } => {
                assert!(reasons.iter().any(|r| r.contains("vs_at_edge_fpm=-1050.0")));
            }
            other => panic!("expected Suspected, got {other:?}"),
        }
    }

    #[test]
    fn off_airport_emergency_landing_weich_not_suspected() {
        // -300 fpm / 1.4 G, nur 1 Marker (no_runway) → Suspected braucht
        // 2 Marker. Soll None bleiben.
        let i = AccidentHeuristicInput {
            vs_at_edge_fpm: Some(-300.0),
            peak_g_load: Some(1.4),
            sideslip_deg: Some(10.0),
            landing_wing_strike_severity_pct: Some(20.0),
            approach_stall_warning_count: Some(0),
            runway_match_found: Some(false),
            rollout_distance_m: Some(900.0),
        };
        let r = classify_accident_heuristic(&i);
        assert_eq!(r, AccidentDecision::None);
    }

    #[test]
    fn empty_input_is_none() {
        // Pre-v0.5.x Payload ohne irgendwelche Felder darf nicht
        // false-positive werden.
        let r = classify_accident_heuristic(&AccidentHeuristicInput::default());
        assert_eq!(r, AccidentDecision::None);
    }

    #[test]
    fn accident_kind_wire_str_stable() {
        assert_eq!(AccidentKind::SimCrash.as_wire_str(), "sim_crash");
        assert_eq!(AccidentKind::Impact.as_wire_str(), "impact");
        assert_eq!(
            AccidentKind::OffAirportImpact.as_wire_str(),
            "off_airport_impact"
        );
    }

    #[test]
    fn build_accident_notes_includes_kind_and_reasons() {
        let n = build_accident_notes(
            AccidentKind::Impact,
            "high",
            &[
                "vs_at_edge_fpm=-2249.9".into(),
                "peak_g_load=4.41".into(),
                "no_runway_match".into(),
            ],
        );
        assert!(n.contains("impact"));
        assert!(n.contains("high confidence"));
        assert!(n.contains("vs_at_edge_fpm=-2249.9"));
        assert!(n.contains("no_runway_match"));
    }

    // ─── Fixture-Replay (QS-R1 Finding 6) ──────────────────────────
    //
    // Echte Payloads aus dem GAF-707-Pilot-Export 2026-05-13. Crash =
    // touchdown id 127 (vs=-2250 fpm, 4.41 G, sideslip=-115°, no runway).
    // Control = touchdown id 113 aus demselben Export, normale Landung
    // bei UMKK (vs=-232 fpm, 1.30 G, sideslip=-2.5°, runway match).
    //
    // Zweck: beweisen, dass die Heuristik tatsaechlich diskriminiert
    // und nicht alle Score-0/Hart-Landungen blind als Accident
    // markiert. Wenn jemand spaeter die Schwellen in classify_accident_
    // heuristic anfasst, falt einer dieser Tests.
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct FixturePayload {
        vs_at_edge_fpm: Option<f32>,
        peak_g_load: Option<f32>,
        sideslip_deg: Option<f32>,
        landing_wing_strike_severity_pct: Option<f32>,
        approach_stall_warning_count: Option<u32>,
        runway_match_icao: Option<String>,
        rollout_distance_m: Option<f32>,
    }

    fn fixture_to_input(p: &FixturePayload) -> AccidentHeuristicInput {
        AccidentHeuristicInput {
            vs_at_edge_fpm: p.vs_at_edge_fpm,
            peak_g_load: p.peak_g_load,
            sideslip_deg: p.sideslip_deg,
            landing_wing_strike_severity_pct: p.landing_wing_strike_severity_pct,
            approach_stall_warning_count: p.approach_stall_warning_count,
            runway_match_found: Some(p.runway_match_icao.is_some()),
            rollout_distance_m: p.rollout_distance_m,
        }
    }

    #[test]
    fn fixture_gaf707_crash_is_confirmed_accident() {
        let raw = include_str!("../tests/fixtures/gaf707-crash-touchdown.json");
        let payload: FixturePayload = serde_json::from_str(raw).expect("fixture parses");
        let input = fixture_to_input(&payload);
        let decision = classify_accident_heuristic(&input);
        match decision {
            AccidentDecision::Confirmed { kind, reasons } => {
                // Path 1 (extreme impact) gewinnt das kind-Tag.
                assert!(matches!(kind, AccidentKind::Impact));
                assert!(reasons.iter().any(|r| r.contains("peak_g_load")));
                assert!(reasons.iter().any(|r| r.contains("no_runway_match")));
            }
            other => panic!(
                "GAF 707 crash fixture should be Confirmed(Impact), got {other:?}"
            ),
        }
    }

    #[test]
    fn fixture_gaf707_control_is_not_accident() {
        // Aus demselben Export: regulaere Landung bei UMKK. Wenn die
        // Heuristik HIER false-positive markiert, sind die Schwellen
        // zu locker.
        let raw = include_str!("../tests/fixtures/gaf707-control-touchdown.json");
        let payload: FixturePayload = serde_json::from_str(raw).expect("fixture parses");
        let input = fixture_to_input(&payload);
        let decision = classify_accident_heuristic(&input);
        assert_eq!(
            decision,
            AccidentDecision::None,
            "control fixture must not be classified as accident"
        );
    }
}
