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

// ── Band-Grenzen / Banding ─────────────────────────────────────────────
// v1.6.7: gewertet wird die ECHTE genutzte Bahnstrecke (Aufsetzpunkt +
// Ausrollstrecke) gegen die LDA. Keine toleranzbereinigte Zweitgroesse
// mehr — die Zahl im Panel und die Zahl hinter den Punkten sind
// dieselbe. Bandgrenzen 60/70/80/90 % (Landestrecken-Faktoren 1,67 /
// 1,43 / 1,25 / 1,15).

#[test]
fn no_pre_rounding_at_band_boundary() {
    // 59.9 % genutzt, Light (keine Allowance): wuerde vorher gerundet,
    // waere es 60 → good_stop (80). Ungerundet 59.9 < 60.0
    // (FULL_MARGIN_MAX_PCT) → excellent_margin (100).
    // td 100 + rollout 499 = 599 m von 1000 m LDA.
    let input = ok_input(100.0, 499.0, 1000.0, 0, "C172");
    let r = sub_rollout_v2(&input);
    assert_eq!(r.points, 100);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.excellent_margin"));
    // Und einen Meter weiter kippt es sauber ins naechste Band.
    let r = sub_rollout_v2(&ok_input(100.0, 500.0, 1000.0, 0, "C172"));
    assert_eq!(r.points, 80);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.good_stop"));
}

#[test]
fn heavy_allowance_5pp_at_band_boundary() {
    // Die 5-Prozentpunkte-Gutschrift fuer Widebodies (LE5) haengt an der
    // 60 %-Grenze genauso wie vorher an der 40er.
    //   Heavy, genutzt 650 m / 1000 m LDA → 65 % − 5 pp = 60 %
    //     → `60 < 60` false → good_stop (80)
    //   Heavy, genutzt 640 m → 64 % − 5 pp = 59 % < 60 → excellent (100)
    let r_heavy_65 = sub_rollout_v2(&ok_input(100.0, 550.0, 1000.0, 0, "A388"));
    assert_eq!(
        r_heavy_65.points, 80,
        "Heavy 65 % genutzt → 60 % nach Gutschrift → good_stop"
    );
    let r_heavy_64 = sub_rollout_v2(&ok_input(100.0, 540.0, 1000.0, 0, "A388"));
    assert_eq!(
        r_heavy_64.points, 100,
        "Heavy 64 % genutzt → 59 % nach Gutschrift → excellent"
    );
}

#[test]
fn medium_no_allowance() {
    // A320 (Medium): dieselben 65 % wie oben, aber OHNE Gutschrift
    // → 65 % faellt ins good_stop-Band (80). Zeigt, dass die Gutschrift
    // wirklich nur an der Kategorie haengt.
    let r = sub_rollout_v2(&ok_input(100.0, 550.0, 1000.0, 0, "A320"));
    assert_eq!(r.points, 80);
    let r = sub_rollout_v2(&ok_input(100.0, 440.0, 1000.0, 0, "A320"));
    assert_eq!(r.points, 100, "54 % genutzt ist auch ohne Gutschrift volle Punktzahl");
}

#[test]
fn cessna_grass_strip_long_rollout() {
    // 425 m Ausrollstrecke auf 500 m Bahn → 85 % genutzt → 25 PTS.
    // Auf der kurzen Bahn zeigt die Achse weiter Zaehne: 75 m Rest sind
    // wenig, egal wie klein das Flugzeug ist.
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

// ── Spaeter Aufsetzpunkt (Begruendung `long_float`) ────────────────────
// v1.6.7: der Aufsetzpunkt wird nicht mehr gegen eine Toleranz
// verrechnet — er verlaengert schlicht die genutzte Strecke. `long_float`
// ist seither nur noch die ERKLAERUNG dafuer, wo die Punkte geblieben
// sind: hinter der Aufsetzzone aufgesetzt (900 m / erstes Drittel),
// waehrend die Ausrollstrecke allein volle Punkte gegeben haette.

#[test]
fn btx8815_real_case_long_float() {
    // Pilot-Beschwerde BTX8815 (Fenix A319, LOWS 15). Echte Flight-Log-
    // Werte. Exzellent gebremst (442 m), aber 540 m hinter der Schwelle
    // aufgesetzt.
    //
    // v1.6.7: 540.85 + 442.50 = 983 m von 2850 m LDA = 34,5 % genutzt
    // → volle Punktzahl, und der Aufsetzpunkt liegt mit 541 m noch in
    // der Aufsetzzone (900 m) → keine Sonder-Erklaerung noetig. Genau
    // was der Pilot damals reklamiert hatte: eine normale, komfortable
    // Landung ist einfach "excellent_margin".
    let r = sub_rollout_v2(&ok_input(540.85, 442.50, 2849.88, 0, "A319"));
    assert_eq!(r.points, 100, "BTX8815: 34,5 % genutzt → 100 PT");
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.excellent_margin"));
    assert!(!r.skipped);
    assert!(r.warning.is_none());
    // value zeigt die ECHTE Auslastung (raw 983/2850 = 34.5 % → 35 %)
    assert!(r.value.as_deref().unwrap_or("").contains("35 %"));
}

#[test]
fn touchdown_inside_zone_no_override() {
    // Aufsetzen in der Aufsetzzone → normale Begruendung, kein
    // long_float. LDA 1000 m → Zone = 1000/3 = 333 m; td 100 m liegt
    // drin. 350 m von 1000 m genutzt → excellent_margin.
    let r = sub_rollout_v2(&ok_input(100.0, 250.0, 1000.0, 0, "A320"));
    assert_eq!(r.points, 100);
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.excellent_margin"),
        "Aufsetzen in der Zone → normale Begruendung, NICHT long_float"
    );
}

#[test]
fn full_marks_never_get_long_float() {
    // Selbst weit hinter der Aufsetzzone: wer volle Punktzahl hat, hat
    // nichts zu erklaeren. LDA 4000 m → Zone 900 m; td 1200 m liegt
    // dahinter, aber 1700 m genutzt = 42,5 % → 100 PT.
    let r = sub_rollout_v2(&ok_input(1200.0, 500.0, 4000.0, 0, "A320"));
    assert_eq!(r.points, 100);
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.excellent_margin"),
        "100 Punkte brauchen keine Ausrede"
    );
}

#[test]
fn long_float_needs_a_comfortable_rollout() {
    // long_float sagt „Bremsweg top, nur spaet aufgesetzt" — das darf
    // nur stehen, wenn die Ausrollstrecke ALLEIN volle Punkte gaebe.
    // LDA 1000 m → Zone 333 m, td 400 m liegt dahinter. Ausrollstrecke
    // 600 m = 60 % der LDA → `< 60` ist false → die Aussage waere
    // gelogen, also normale Begruendung.
    //   genutzt 1000 m von 1000 m = 100 % → `> 100` false → 5 PT.
    let r = sub_rollout_v2(&ok_input(400.0, 600.0, 1000.0, 0, "A320"));
    assert_eq!(r.points, 5);
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.marginal_runway"),
        "Bremsweg selbst war lang → keine long_float-Ausrede"
    );
}

#[test]
fn swr255_real_case_long_float() {
    // Echter Fall aus dem Bestand: SWR 255, EDDH 15 (A20N). 2670 m
    // hinter der Schwelle aufgesetzt, danach nur 684 m ausgerollt —
    // 3354 m von 3666 m LDA genutzt, 312 m Restbahn.
    // Alt: 55 PT (die Toleranz nahm dem Aufsetzpunkt die Wucht).
    // Neu: 5 PT, und die Begruendung benennt die Ursache.
    let r = sub_rollout_v2(&ok_input(2670.0, 684.0, 3666.0, 0, "A20N"));
    assert_eq!(r.points, 5);
    assert_eq!(
        r.rationale_key.as_deref(),
        Some("landing.rat.long_float"),
        "Bremsweg 19 % der Bahn — die Punkte kostet der Aufsetzpunkt"
    );
}

#[test]
fn overrun_keeps_its_own_rationale() {
    // Overrun-Gate unveraendert: td 1500 + rollout 1100 = 2600 m auf
    // 2500 m LDA → 104 % → 0 PT. Frueher konnte die Float-Toleranz so
    // etwas verwaessern; die Groesse ist jetzt ohnehin nur noch eine.
    // Und: die Warnung darf NICHT von „Bremsweg top, spaet aufgesetzt"
    // verdeckt werden, obwohl hier beides zutraefe.
    let r = sub_rollout_v2(&ok_input(1500.0, 1100.0, 2500.0, 0, "A320"));
    assert_eq!(r.points, 0);
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.overrun_risk"));
}

#[test]
fn ezy2995_near_overrun_is_no_longer_forgiven() {
    // Echter Fall: EZY 2995, LIRF 16L (A319). 3822 m von 3902 m LDA
    // genutzt — 80 m Restbahn. Der alte Algorithmus gab dafuer 55 PT
    // („OK — sportlich"), weil er die toleranzbereinigte Groesse
    // bewertete und das Overrun-Gate erst ueber 100 % greift.
    let r = sub_rollout_v2(&ok_input(760.0, 3062.0, 3902.0, 0, "A319"));
    assert_eq!(r.points, 5, "98 % Bahnnutzung ist nicht 'sportlich'");
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.marginal_runway"));
}

#[test]
fn touchdown_zone_scales_with_lda() {
    // Die Aufsetzzone ist 900 m — auf kurzen Bahnen das erste Drittel.
    // Gleicher Aufsetzpunkt (400 m), zwei Bahnlaengen:
    //   1500 m LDA → Zone 500 m → 400 m liegt DRIN
    //   1000 m LDA → Zone 333 m → 400 m liegt DAHINTER
    let kurz = sub_rollout_v2(&ok_input(400.0, 600.0, 1500.0, 0, "A320"));
    assert_eq!(kurz.points, 80, "1000 m von 1500 m = 67 %");
    assert_eq!(
        kurz.rationale_key.as_deref(),
        Some("landing.rat.good_stop"),
        "Aufsetzpunkt in der Zone → keine long_float-Begruendung"
    );
    let kuerzer = sub_rollout_v2(&ok_input(400.0, 400.0, 1000.0, 0, "A320"));
    assert_eq!(kuerzer.points, 25, "800 m von 1000 m = 80 % genutzt");
    assert_eq!(
        kuerzer.rationale_key.as_deref(),
        Some("landing.rat.long_float"),
        "hinter der Zone aufgesetzt, Bremsweg allein waere voll gewesen"
    );
}

#[test]
fn ewg2047_regression_full_marks() {
    // DER Ausloeser-Fall (16.08.2026, EWG 2047, EDDH→EDDS 25, A20N):
    // 286 m hinter der Schwelle aufgesetzt, 1553 m ausgerollt, 1839 m
    // von 3345 m LDA genutzt — 1,5 km Bahn blieben uebrig. Alt: 80 PT.
    let r = sub_rollout_v2(&ok_input(286.33, 1552.7, 3344.88, 0, "A20N"));
    assert_eq!(r.points, 100, "1,5 km Restbahn sind kein Punktabzug");
    assert_eq!(r.rationale_key.as_deref(), Some("landing.rat.excellent_margin"));
    assert!(r.value.as_deref().unwrap_or("").contains("55 %"));
}

#[test]
fn nan_geometry_is_skipped_not_punished() {
    // Lehre aus der Ausrichtungs-Achse (v1.6.2): `NaN < x` ist immer
    // false — ohne Riegel faellt ein kaputter Messwert durch ALLE
    // Baender und bekommt die haerteste Note, statt ehrlich „nicht
    // bewertet" zu sagen.
    let r = sub_rollout_v2(&ok_input(f64::NAN, 800.0, 3000.0, 0, "A320"));
    assert!(r.skipped, "NaN darf nicht bewertet werden");
    assert_eq!(r.reason.as_deref(), Some("invalid_geometry"));
    let r = sub_rollout_v2(&ok_input(300.0, f32::NAN, 3000.0, 0, "A320"));
    assert!(r.skipped);
    let r = sub_rollout_v2(&ok_input(300.0, 800.0, f32::NAN, 0, "A320"));
    assert!(r.skipped);
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
