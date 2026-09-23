//! v0.9.0 (#Discord-RPC) Tests fuer format.rs — pure-fn, kein Discord-Mock noetig.
//!
//! Spec-Pflicht-Pruefungen:
//!   - phase_to_label deckt ALLE 18 Phasen ab (= keine $UNKNOWN-Fallbacks)
//!   - Anonymisierung wirklich anonym
//!   - Altitude-Schwelle 18000 ft korrekt
//!   - build_details / build_state Format-Beispiele aus Spec stimmen

use discord_presence::format::*;
use discord_presence::{FlightPhase, PresenceInput, SimKind};

fn dummy_input(phase: FlightPhase) -> PresenceInput {
    PresenceInput {
        callsign: "GSG3184".to_string(),
        dep_icao: "EDDB".to_string(),
        arr_icao: "KMRH".to_string(),
        aircraft: "A320".to_string(),
        altitude_ft: Some(36000),
        phase,
        sim: SimKind::Msfs2024,
        start_unix: 1700000000,
        profile_url: None,
    }
}

// ─── phase_to_label: alle 18 Phasen muessen ein Mapping haben ──────────────

#[test]
fn phase_to_label_covers_all_20_phases() {
    use FlightPhase::*;
    let all = [
        Preflight,
        Boarding,
        Pushback,
        TaxiOut,
        TakeoffRoll,
        Takeoff,
        RejectedTakeoff,
        Climb,
        Cruise,
        Holding,
        Descent,
        Approach,
        Final,
        Landing,
        GoAround,
        TaxiIn,
        Arrived,
        BlocksOn,
        Deboarding,
        PirepSubmitted,
    ];
    // v0.9.0-Hotfix F2: vorher waren's 18 (ohne Holding + PirepSubmitted), jetzt 20.
    // 17 davon emittiert der heutige Rust-FSM, 3 (RejectedTakeoff, GoAround,
    // Deboarding) sind fuer v0.10.0 vorgesehen aber bereits hier gemappt.
    assert_eq!(all.len(), 20, "20 Phasen (17 vom FSM + 3 v0.10.0-ready)");
    for p in all {
        let label = phase_to_label(p);
        assert!(!label.is_empty(), "{:?} ohne Label", p);
        assert!(
            !label.contains("UNKNOWN"),
            "{:?} -> {} darf kein UNKNOWN-Fallback sein",
            p,
            label
        );
    }
}

#[test]
fn holding_and_pirep_submitted_have_distinct_labels() {
    // QS-Regression-Test: vorher fielen diese in den parse_phase-Default (Preflight)
    // und kamen in Discord als "PREFLIGHT" raus. Spec-Garantie ist eigene Beschriftung.
    assert_eq!(phase_to_label(FlightPhase::Holding), "HOLDING");
    assert_eq!(phase_to_label(FlightPhase::PirepSubmitted), "PIREP FILED");
    assert_ne!(
        phase_to_label(FlightPhase::Holding),
        phase_to_label(FlightPhase::Preflight)
    );
    assert_ne!(
        phase_to_label(FlightPhase::PirepSubmitted),
        phase_to_label(FlightPhase::Preflight)
    );
}

#[test]
fn phase_warn_labels_have_warning_prefix() {
    assert!(phase_to_label(FlightPhase::RejectedTakeoff).starts_with("⚠"));
    assert!(phase_to_label(FlightPhase::GoAround).starts_with("⚠"));
}

#[test]
fn phase_normal_labels_have_no_warning_prefix() {
    for p in [
        FlightPhase::Cruise,
        FlightPhase::Climb,
        FlightPhase::Approach,
        FlightPhase::Boarding,
    ] {
        assert!(
            !phase_to_label(p).starts_with("⚠"),
            "{:?} darf kein ⚠ haben",
            p
        );
    }
}

// ─── phase_to_asset_key: nur 6 erlaubte Asset-Keys ────────────────────────

#[test]
fn phase_to_asset_key_uses_only_six_registered_assets() {
    use FlightPhase::*;
    let allowed = [
        "phase_taxi",
        "phase_climb",
        "phase_cruise",
        "phase_descent",
        "phase_approach",
        "phase_landed",
    ];
    for p in [
        Preflight,
        Boarding,
        Pushback,
        TaxiOut,
        TakeoffRoll,
        Takeoff,
        RejectedTakeoff,
        Climb,
        Cruise,
        Holding,
        Descent,
        Approach,
        Final,
        Landing,
        GoAround,
        TaxiIn,
        Arrived,
        BlocksOn,
        Deboarding,
        PirepSubmitted,
    ] {
        let key = phase_to_asset_key(p);
        assert!(
            allowed.contains(&key),
            "{:?} mapped auf nicht-registriertes Asset '{}'",
            p,
            key
        );
    }
}

// ─── format_altitude: 18000-ft-Schwelle ────────────────────────────────────

#[test]
fn altitude_below_18000_ft_uses_feet_format() {
    assert_eq!(format_altitude(0), "0 ft");
    assert_eq!(format_altitude(2500), "2500 ft");
    assert_eq!(format_altitude(17999), "17999 ft");
}

#[test]
fn altitude_at_or_above_18000_ft_uses_flight_level() {
    assert_eq!(format_altitude(18000), "FL180");
    assert_eq!(format_altitude(36000), "FL360");
    assert_eq!(format_altitude(41000), "FL410");
}

#[test]
fn altitude_rounds_to_nearest_hundred() {
    assert_eq!(format_altitude(36049), "FL360", "rundet ab");
    assert_eq!(format_altitude(36050), "FL361", "rundet auf");
    assert_eq!(format_altitude(35975), "FL360");
}

// ─── Anonymisierung ───────────────────────────────────────────────────────

#[test]
fn anonymize_off_keeps_callsign_as_is() {
    assert_eq!(maybe_anonymize_callsign("GSG3184", false), "GSG3184");
    assert_eq!(maybe_anonymize_callsign("DLH1234", false), "DLH1234");
}

#[test]
fn anonymize_on_strips_numeric_suffix_keeps_icao_prefix() {
    assert_eq!(maybe_anonymize_callsign("GSG3184", true), "GSG-Flight");
    assert_eq!(maybe_anonymize_callsign("DLH1234", true), "DLH-Flight");
    assert_eq!(maybe_anonymize_callsign("RYR9", true), "RYR-Flight");
}

#[test]
fn anonymize_uppercases_icao() {
    assert_eq!(maybe_anonymize_callsign("gsg3184", true), "GSG-Flight");
}

#[test]
fn anonymize_handles_pure_number_callsign() {
    // Kein Buchstaben-Prefix => "Flight"
    assert_eq!(maybe_anonymize_callsign("12345", true), "Flight");
    assert_eq!(maybe_anonymize_callsign("", true), "Flight");
}

// ─── build_details ────────────────────────────────────────────────────────

#[test]
fn build_details_normal_case_matches_spec_example() {
    let input = dummy_input(FlightPhase::Cruise);
    assert_eq!(build_details(&input, false), "GSG3184 · EDDB → KMRH");
}

#[test]
fn build_details_anonymized() {
    let input = dummy_input(FlightPhase::Cruise);
    assert_eq!(build_details(&input, true), "GSG-Flight · EDDB → KMRH");
}

#[test]
fn build_details_missing_dest_shows_dash() {
    let mut input = dummy_input(FlightPhase::Cruise);
    input.arr_icao = String::new();
    assert_eq!(build_details(&input, false), "GSG3184 · EDDB → —");
}

// ─── build_state ──────────────────────────────────────────────────────────

#[test]
fn build_state_cruise_matches_spec_example() {
    let input = dummy_input(FlightPhase::Cruise);
    assert_eq!(build_state(&input, false), "CRUISE · A320 · FL360");
}

#[test]
fn build_state_ground_phase_omits_altitude() {
    let mut input = dummy_input(FlightPhase::TaxiOut);
    input.altitude_ft = None;
    assert_eq!(build_state(&input, false), "TAXI OUT · A320");
}

#[test]
fn build_state_arrived_shows_arr_icao_not_altitude() {
    let input = dummy_input(FlightPhase::Arrived);
    assert_eq!(build_state(&input, false), "ARRIVED · A320 · KMRH");
}

#[test]
fn build_state_warn_phases_keep_their_prefix() {
    let input = dummy_input(FlightPhase::GoAround);
    assert!(build_state(&input, false).starts_with("⚠ GO-AROUND"));

    let input2 = dummy_input(FlightPhase::RejectedTakeoff);
    assert!(build_state(&input2, false).starts_with("⚠ REJECTED TAKE-OFF"));
}

#[test]
fn build_state_sim_lost_appends_suffix() {
    let input = dummy_input(FlightPhase::Cruise);
    let text = build_state(&input, true);
    assert!(text.ends_with("⚠ Sim getrennt"), "got: {text}");
}

#[test]
fn build_state_low_altitude_uses_feet() {
    let mut input = dummy_input(FlightPhase::Approach);
    input.altitude_ft = Some(2500);
    assert_eq!(build_state(&input, false), "APPROACH · A320 · 2500 ft");
}

#[test]
fn build_state_missing_altitude_shows_dash() {
    let mut input = dummy_input(FlightPhase::Cruise);
    input.altitude_ft = None;
    assert_eq!(build_state(&input, false), "CRUISE · A320 · —");
}

#[test]
fn build_state_missing_aircraft_uses_fallback_word() {
    let mut input = dummy_input(FlightPhase::Cruise);
    input.aircraft = String::new();
    assert_eq!(build_state(&input, false), "CRUISE · Aircraft · FL360");
}

// ─── sim_to_asset_key / tooltip ───────────────────────────────────────────

#[test]
fn sim_to_asset_key_returns_none_for_unknown() {
    assert_eq!(sim_to_asset_key(SimKind::Unknown), None);
    assert_eq!(sim_to_asset_key(SimKind::Msfs2024), Some("sim_msfs2024"));
    assert_eq!(sim_to_asset_key(SimKind::Xplane12), Some("sim_xplane12"));
}

#[test]
fn sim_to_tooltip_has_human_readable_label() {
    assert_eq!(sim_to_tooltip(SimKind::Msfs2024), "MSFS 2024");
    assert_eq!(sim_to_tooltip(SimKind::Xplane12), "X-Plane 12");
    assert_eq!(sim_to_tooltip(SimKind::Unknown), "Simulator");
}
