//! v0.10.0 (#runway-utilization-score) — Tests fuer den LDA-basierten
//! Bahn-Auslastungs-Sub-Score.
//!
//! Spec: docs/spec/v0.10.0-runway-utilization-score.md (SPEC ACCEPTED R5).
//!
//! Test-Schwerpunkte:
//!   1. Reale Cases (EK406-A380, displaced threshold @ EDDF 25C)
//!   2. Skip-Gates (alle 6 Reasons)
//!   3. Overrun-vor-Allowance (R2-P0-2 Fix)
//!   4. Heavy-Allowance an Band-Grenzen (Float-Banding, R2-P2-1 Fix)
//!   5. pre_displaced-Cap mit Rationale-Override (R4-P1-3 Fix)
//!   6. Negativ-TD-Distance Clamp auf Rollout-Only (LE3)
//!   7. Wire-Schema-Golden-File-Snapshot (LE8)

use landing_scoring::sub_rollout::{sub_rollout_v2, RolloutInput};

/// Builder fuer ein vollstaendig vertrauenswuerdiges Input.
/// Spec-konforme Defaults: runway_geometry_trusted=Some(true),
/// airport_source="runway_match". Tests die einen Skip wollen
/// uebersteuern explizit das jeweilige Feld.
fn ok_input<'a>(
    td_m: f64,
    rollout_m: f32,
    runway_m: f32,
    displaced_ft: i32,
    icao: &'a str,
) -> RolloutInput<'a> {
    RolloutInput {
        td_distance_from_threshold_m: Some(td_m),
        rollout_distance_m: Some(rollout_m),
        landing_float_distance_m: Some((td_m as f32).max(0.0)),
        runway_length_m: Some(runway_m),
        runway_displaced_threshold_ft: Some(displaced_ft),
        pre_displaced_threshold: Some(false),
        runway_geometry_trusted: Some(true),
        airport_source: Some("runway_match"),
        runway_match_icao: Some("XXXX"),
        runway_match_ident: Some("00"),
        aircraft_icao: Some(icao),
    }
}

// ── Reale Cases ────────────────────────────────────────────────────────

#[test]
fn ek406_a380_real_case_excellent() {
    // EK406 reale Werte (Recorder-DB Touchdown id=225):
    //   td_distance_from_threshold_m = 516.93
    //   rollout_distance_m = 583.55
    //   runway_length_m = 3657 (YMML 16, Melbourne)
    //   displaced = 0
    //   aircraft = A388 (Heavy)
    // raw_used = (516.93 + 583.55).max(583.55) = 1100.48 m
    // raw_ratio = 1100.48 / 3657 = 30.09 %
    // Heavy-Allowance -5 pp → effective = 25.09 %
    // 25.09 < 30 → excellent_margin (100 PTS)
    let r = sub_rollout_v2(&ok_input(516.93, 583.55, 3657.0, 0, "A388"));
    assert_eq!(r.points, 100, "EK406-A380 muss excellent_margin sein");
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.excellent_margin"));
    assert!(r.value.as_deref().unwrap_or("").contains("30 %"));
    assert!(!r.skipped);
    assert!(r.warning.is_none());
}

#[test]
fn eddf_25c_displaced_threshold_ok_stop() {
    // EDDF 25C: 4000 m physisch, 1968 ft displaced (≈600 m), LDA ≈ 3400 m
    // TD 800 m past threshold + 1500 m Rollout = 2300 m used
    // raw_ratio = 2300 / 3400 = 67.6 %; A320 = Medium, keine Allowance
    // v0.20.x: tolerance 0.20*3400=680, eff_float=800-680=120,
    // eff_distance=1500+120=1620, eff_ratio=47.6 % → < 60 → good_stop (80 PTS)
    let r = sub_rollout_v2(&ok_input(800.0, 1500.0, 4000.0, 1968, "A320"));
    assert_eq!(r.points, 80);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.good_stop"));
}

// ── Overrun-vor-Allowance (R2-P0-2 Fix) ────────────────────────────────

#[test]
fn a380_short_runway_overrun_risk() {
    // Raw 108 % → overrun_risk (vor Heavy-Allowance gechecked).
    // OHNE die Reihenfolge-Garantie waere 108 - 5 = 103 % → ein anderer
    // Branch → marginal_runway (5 PTS) — der Overrun waere verschluckt.
    let r = sub_rollout_v2(&ok_input(500.0, 2200.0, 2500.0, 0, "A388"));
    // (500+2200)/2500 = 108 % → overrun_risk
    assert_eq!(r.points, 0);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.overrun_risk"));
}

// ── pre_displaced (R2-P1-4 + R4-P1-3 Fixes) ────────────────────────────

#[test]
fn pre_displaced_caps_at_55_pts_with_rationale_override() {
    // Sonst waere excellent_margin (100 PTS); mit Cap → 55 PTS
    // R4-P1-3-Fix: Rationale-Override auf "pre_displaced_capped"
    // (NICHT excellent_margin), sonst zeigt UI "Viel Bahn-Reserve" bei
    // 55 PTS = unehrlich.
    let mut input = ok_input(516.93, 583.55, 3657.0, 0, "A388");
    input.pre_displaced_threshold = Some(true);
    let r = sub_rollout_v2(&input);
    assert_eq!(r.points, 55);
    assert_eq!(r.warning.as_deref(), Some("pre_displaced_threshold"));
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.pre_displaced_capped")
    );
}

#[test]
fn negative_td_distance_clamped_to_rollout_only() {
    // Pre-displaced + neg TD-Distance:
    // raw_used = -50 + 800 = 750; max(800) = 800 → ratio = 80 %
    // Light/Medium ohne Allowance → 80 % → long_rollout (25 PTS)
    // pre_displaced cap min(55) → bleibt 25 (Cap nur senkt nie hebt)
    let mut input = ok_input(-50.0, 800.0, 1000.0, 0, "A320");
    input.pre_displaced_threshold = Some(true);
    let r = sub_rollout_v2(&input);
    assert_eq!(r.points, 25);
    assert!(r.warning.is_some());
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.pre_displaced_capped")
    );
}

// ── Band-Grenzen / Banding (v0.12.0: effective_distance) ───────────────
// HINWEIS v0.12.0: das Banding läuft auf der toleranzbereinigten
// effective_distance. Damit kein Float die Band-Grenze verfälscht,
// halten diese Tests td_dist INNERHALB der 20 %-Toleranz (effective_
// float = 0 → effective_distance = rollout).
//
// v0.20.x: Toleranz 15→20 %, Bandgrenzen 30/50/70/90 → 40/60/80/95
// (EXCELLENT_MAX_PCT/GOOD_MAX_PCT/OK_MAX_PCT/LONG_MAX_PCT). Alle
// Boundary-Tests unten zielen jetzt auf die NEUEN Grenzen.

#[test]
fn no_pre_rounding_at_band_boundary() {
    // 39.5 % effective, Light (keine Allowance):
    //   Wenn vorher gerundet wuerde: 40 → good_stop (80 PTS)
    //   Mit Float-Banding: 39.5 < 40.0 (EXCELLENT_MAX_PCT) → excellent_margin (100 PTS)
    // td 100 m liegt unter Toleranz (0.20*1000 = 200 m) → effective_float=0,
    // effective_distance = rollout 395 m → 39.5 %.
    let input = ok_input(100.0, 395.0, 1000.0, 0, "C172");
    let r = sub_rollout_v2(&input);
    assert_eq!(r.points, 100);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.excellent_margin"));
}

#[test]
fn heavy_allowance_5pp_at_band_boundary() {
    // Heavy-Allowance an der 40 %-Bandgrenze (EXCELLENT_MAX_PCT). td 100 m
    // < Toleranz 200 m → effective_float=0 → effective_distance = rollout.
    //   Heavy, rollout 450 m / 1000 m LDA → 45 % − 5 pp = 40 %
    //     → `40 < 40` false → `e if e < 60` → good_stop (80 PTS)
    //   Heavy, rollout 440 m → 44 % − 5 pp = 39 % < 40 → excellent (100)
    let r_heavy_45 = sub_rollout_v2(&ok_input(100.0, 450.0, 1000.0, 0, "A388"));
    assert_eq!(
        r_heavy_45.points, 80,
        "Heavy 45% effective → 40% nach Allowance → good_stop"
    );
    let r_heavy_44 = sub_rollout_v2(&ok_input(100.0, 440.0, 1000.0, 0, "A388"));
    assert_eq!(
        r_heavy_44.points, 100,
        "Heavy 44% effective → 39% nach Allowance → excellent"
    );
}

#[test]
fn medium_no_allowance() {
    // A320 (Medium, keine Allowance): rollout 500 m / 1000 m LDA = 50 %
    // effective → jetzt good_stop (80 PTS, GOOD_MAX_PCT=60). td 100 m <
    // Toleranz → kein Float-Anteil.
    let r = sub_rollout_v2(&ok_input(100.0, 500.0, 1000.0, 0, "A320"));
    assert_eq!(r.points, 80);
}

#[test]
fn cessna_grass_strip_long_rollout() {
    // 425 m Rollout / 500 m Bahn → 85 % → long_rollout (25 PTS,
    // OK_MAX_PCT=80/LONG_MAX_PCT=95 — 70 % waere jetzt nur noch ok_stop).
    let r = sub_rollout_v2(&ok_input(0.0, 425.0, 500.0, 0, "C172"));
    assert_eq!(r.points, 25);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.long_rollout"));
}

// ── Skip-Gates (LE6 — alle 6 Reasons) ──────────────────────────────────

#[test]
fn skip_missing_td_distance() {
    let mut input = ok_input(0.0, 583.55, 3657.0, 0, "A388");
    input.td_distance_from_threshold_m = None;
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("missing_td_distance"));
}

#[test]
fn skip_missing_rollout() {
    let mut input = ok_input(516.93, 0.0, 3657.0, 0, "A388");
    input.rollout_distance_m = None;
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("missing_rollout_distance"));
}

#[test]
fn skip_missing_length() {
    let mut input = ok_input(516.93, 583.55, 0.0, 0, "A388");
    input.runway_length_m = None;
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("missing_length"));
}

#[test]
fn skip_runway_geometry_trusted_none_is_not_trusted() {
    // R2-P1-2 Fix: None ist NICHT trusted, nur Some(true).
    let mut input = ok_input(516.93, 583.55, 3657.0, 0, "A388");
    input.runway_geometry_trusted = None;
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("untrusted_geometry"));
}

#[test]
fn skip_runway_geometry_trusted_false() {
    let mut input = ok_input(516.93, 583.55, 3657.0, 0, "A388");
    input.runway_geometry_trusted = Some(false);
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("untrusted_geometry"));
}

#[test]
fn skip_off_airport_landing() {
    let mut input = ok_input(516.93, 583.55, 3657.0, 0, "A388");
    input.airport_source = Some("nearest_25nm");
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("off_airport_landing"));
}

#[test]
fn off_airport_priority_over_missing_data_fields() {
    // QS-Code-R1 P1-2 + R2-P1-1: realer Off-Airport-Pfad (= kein
    // runway_match) propagiert im fill_v2_rollout_fields-Helper
    // FAKTISCH zu:
    //   airport_source            = None  (rm.map(|_| "runway_match"))
    //   runway_geometry_trusted   = Some(false)  (runway_geometry_trust_check
    //                                returnt no_runway_match → trusted=false)
    //   td_distance/length/rollout = None  (alle aus rm abgeleitet)
    // Mit Geometry-zuerst-Reihenfolge wäre der Reason „untrusted_geometry"
    // — semantisch falsch („untrusted" impliziert: es gibt eine, sie ist
    // nur fragwürdig). „off_airport_landing" ist spezifischer.
    let mut input = ok_input(0.0, 0.0, 0.0, 0, "C172");
    input.airport_source = None;
    input.runway_geometry_trusted = Some(false);
    input.td_distance_from_threshold_m = None;
    input.rollout_distance_m = None;
    input.runway_length_m = None;
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(
        r.reason.as_deref(),
        Some("off_airport_landing"),
        "Production-shaped Off-Airport-Case MUSS off_airport_landing \
         liefern, NICHT untrusted_geometry oder missing_*."
    );
}

#[test]
fn off_airport_with_nearest_25nm_still_wins_over_data_missing() {
    // Variant des obigen Tests: airport_source ist gesetzt auf
    // "nearest_25nm" (= Touchdown nahe einem Airport, aber NICHT auf
    // einer korrelierten Runway). Hier ist die Geometrie meist trusted=true
    // (kein no_runway_match), aber die Datenfelder fehlen.
    let mut input = ok_input(0.0, 0.0, 0.0, 0, "C172");
    input.airport_source = Some("nearest_25nm");
    input.runway_geometry_trusted = Some(true);
    input.td_distance_from_threshold_m = None;
    input.rollout_distance_m = None;
    input.runway_length_m = None;
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("off_airport_landing"));
}

#[test]
fn untrusted_geometry_with_runway_match_but_geometry_check_failed() {
    // untrusted_geometry trifft nur noch wenn AIRPORT-SOURCE OK ist
    // aber Geometry-Check failed (z.B. centerline_offset > 200m,
    // negative float_distance). Dann hat man EINE Bahn, aber ihrer
    // Geometrie ist nicht zu trauen.
    let mut input = ok_input(516.93, 583.55, 3657.0, 0, "A388");
    input.airport_source = Some("runway_match"); // runway WAR korreliert
    input.runway_geometry_trusted = Some(false); // aber geometry-check failed
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("untrusted_geometry"));
}

#[test]
fn skip_invalid_lda() {
    // 100 m Bahn, 500 ft displaced (≈152 m) → LDA < 0 → invalid_lda
    let input = ok_input(50.0, 50.0, 100.0, 500, "C172");
    let r = sub_rollout_v2(&input);
    assert!(r.skipped);
    assert_eq!(r.reason.as_deref(), Some("invalid_lda"));
}

// ── v0.12.0 (#runway-utilization-refinement) — Float-Toleranz Tests ────

#[test]
fn btx8815_real_case_long_float() {
    // Pilot-Beschwerde BTX8815 (Fenix A319, LOWS 15). Echte Flight-Log-
    // Werte. Exzellent gebremst (442 m), aber 540 m hinter der Schwelle
    // aufgesetzt.
    //
    // v0.20.x: mit der breiteren 20 %-Toleranz liegt der Aufsetzpunkt
    // jetzt VOLLSTAENDIG innerhalb der Toleranz — der long_float-
    // Override (der frueher noetig war, um die Punktzahl zu erklaeren)
    // greift gar nicht mehr, weil der Float selbst nicht mehr als
    // "ueber Toleranz" zaehlt. Genau der Fall, den die breitere
    // Toleranz beheben sollte: eine normale, komfortable Landung braucht
    // keine Sonder-Erklaerung mehr, sie ist einfach "excellent_margin".
    //   tolerance      = 0.20 * 2849.88 = 570.0 m
    //   effective_float= max(540.85 - 570.0, 0) = 0 m
    //   effective_dist = 442.50 + 0 = 442.50 m
    //   effective_ratio= 442.50 / 2849.88 = 15.5 %  → < 40 → excellent (100)
    //   float_over_tolerance: 540.85 > 570.0 → false → kein long_float-Override
    let r = sub_rollout_v2(&ok_input(540.85, 442.50, 2849.88, 0, "A319"));
    assert_eq!(r.points, 100, "BTX8815: Float-Toleranz → 100 PT");
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.excellent_margin"));
    assert!(!r.skipped);
    assert!(r.warning.is_none());
    // value zeigt die ECHTE Auslastung (raw 983/2850 = 34.5 % → 35 %)
    assert!(r.value.as_deref().unwrap_or("").contains("35 %"));
}

#[test]
fn float_within_tolerance_no_override() {
    // Float UNTER der 15 %-Toleranz → kein long_float, normale Rationale.
    // td 100 m < tolerance 0.15*1000 = 150 m. rollout 250 m → eff_dist 250
    // → 25 % → excellent_margin, KEIN long_float (Float nicht über Tol.).
    let r = sub_rollout_v2(&ok_input(100.0, 250.0, 1000.0, 0, "A320"));
    assert_eq!(r.points, 100);
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.excellent_margin"),
        "Float in Toleranz → normale Rationale, NICHT long_float"
    );
}

#[test]
fn long_float_needs_excellent_rollout() {
    // R1-P1-1: long_float NUR wenn der reine Bremsweg excellent waere
    // (rollout/LDA < EXCELLENT_MAX_PCT). Hier: Float über Toleranz, aber
    // rollout 400 m / 1000 m LDA = 40 % — mit v0.20.x GENAU an der neuen
    // 40 %-Grenze (`< 40` ist false für 40) → KEIN long_float.
    //   tolerance 200, effective_float = 300-200 = 100, eff_dist = 500,
    //   eff_ratio 50 % → good_stop (80, GOOD_MAX_PCT=60). Band Good, aber
    //   rollout_alone 40 % ist NICHT < 40 → Override greift trotzdem nicht.
    let r = sub_rollout_v2(&ok_input(300.0, 400.0, 1000.0, 0, "A320"));
    assert_eq!(r.points, 80);
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.good_stop"),
        "Bremsweg nicht excellent → normale Rationale, kein long_float"
    );
}

#[test]
fn overrun_still_on_raw_distance() {
    // v0.12.0 LE3: Overrun-Gate bleibt auf der ECHTEN Distanz. Die
    // Float-Toleranz darf ein echtes Overrun NICHT verstecken.
    //   td 1500 + rollout 1100 = 2600 m auf 2500 m LDA → raw 104 % > 100
    //   → overrun_risk, OBWOHL effective (mit Toleranz) < 100 % wäre.
    let r = sub_rollout_v2(&ok_input(1500.0, 1100.0, 2500.0, 0, "A320"));
    assert_eq!(r.points, 0);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.overrun_risk"));
}

#[test]
fn tolerance_scales_with_lda() {
    // Toleranz ist FLOAT_TOLERANCE_FRACTION (20 %) der LDA — skaliert mit
    // der Bahnlänge. Kurze Bahn 1500 m → Toleranz 300 m · lange Bahn
    // 3500 m → 700 m. Gleicher Float 400 m: auf kurzer Bahn über
    // Toleranz, auf langer drunter — rationale-Ausgang bleibt bei beiden
    // Bahnlängen unveraendert ggue. v0.12.0 (nur die Zwischenwerte
    // verschieben sich), deshalb bleiben die Assertions gleich.
    let short = sub_rollout_v2(&ok_input(400.0, 300.0, 1500.0, 0, "A320"));
    // tolerance 300, eff_float = 400-300 = 100, eff_dist = 400, ratio 26.7%
    // → excellent_margin-Band. Float 400 > 300 ✓, rollout/LDA 300/1500 =
    // 20 % < 40 (EXCELLENT_MAX_PCT) ✓, Band Good ✓ → Override → long_float.
    assert_eq!(short.rationale_key.as_deref(), Some("landing.rat.long_float"));
    let long = sub_rollout_v2(&ok_input(400.0, 300.0, 3500.0, 0, "A320"));
    // tolerance 700, eff_float = max(400-700,0) = 0, eff_dist = 300,
    // ratio 8.6 % → excellent_margin, Float 400 < 700 → kein long_float.
    assert_eq!(long.rationale_key.as_deref(), Some("landing.rat.excellent_margin"));
}

#[test]
fn pre_displaced_has_priority_over_long_float() {
    // pre_displaced + langer Float: pre_displaced_capped gewinnt,
    // NICHT long_float (pre_displaced hat Vorrang, eigener Cap).
    let mut input = ok_input(540.0, 300.0, 2850.0, 0, "A320");
    input.pre_displaced_threshold = Some(true);
    let r = sub_rollout_v2(&input);
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.pre_displaced_capped"),
        "pre_displaced hat Vorrang vor long_float"
    );
    assert!(r.points <= 55, "pre_displaced cappt auf 55");
}

#[test]
fn extra_is_empty_for_v3() {
    // v0.12.0 LE5: das Crate baut KEINE extra-Zeilen mehr — der
    // TS-Renderer macht das aus den Record-Feldern. extra ist leer.
    let r = sub_rollout_v2(&ok_input(540.85, 442.50, 2849.88, 0, "A319"));
    assert!(r.extra.is_empty(), "v3-Score liefert extra = []");
}

#[test]
fn effective_vs_raw_ratio_in_value() {
    // v0.12.0 LE4: value zeigt die RAW-Auslastung (echte Distanz), NICHT
    // die toleranzbereinigte effective. Sprachneutrales Format.
    let r = sub_rollout_v2(&ok_input(540.85, 442.50, 2849.88, 0, "A319"));
    let v = r.value.as_deref().unwrap_or("");
    assert!(v.contains("983 m"), "value zeigt echte distance_used 983 m");
    assert!(v.contains("2850 m"), "value zeigt LDA");
    assert!(v.contains("35 %"), "value zeigt raw-% (34.5 → 35), nicht 20 %");
}

// ── Wire-Schema-Snapshot (LE8 — Mini-Golden-JSON-Datei) ────────────────

#[test]
fn wire_schema_matches_golden_fixture() {
    // R4 LE8: Mini-Golden-JSON statt insta (insta NICHT im Workspace-
    // Dep-Tree). Bei beabsichtigter Schema-Aenderung: Test laufen,
    // Diff sehen, Fixture-File aktualisieren, Reviewer prueft Diff im
    // Code-Review.
    let mut input = ok_input(516.93, 583.55, 3657.0, 0, "A388");
    input.runway_match_icao = Some("YMML");
    input.runway_match_ident = Some("16");
    let sample = sub_rollout_v2(&input);
    let actual = serde_json::to_string_pretty(&sample).unwrap();
    let expected = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/subscoreentry_v2_ek406.json"
    ))
    .expect("golden fixture missing — siehe Spec LE8");
    assert_eq!(
        actual.trim(),
        expected.trim(),
        "Wire-Schema drifted vs golden fixture — beabsichtigt? Dann fixture aktualisieren."
    );
}
