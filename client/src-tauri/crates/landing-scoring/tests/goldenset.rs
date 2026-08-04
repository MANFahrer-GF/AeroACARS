//! Goldenset-Tests fuer landing-scoring Crate.
//!
//! v0.7.1 Phase 2 Update: nutzt jetzt sub_fuel_v0_7_1 (Hard-Gate +
//! Asymmetrie) und sub_loadsheet (NEU). Replay-Fluege erwarten
//! bewusste Score-Drifts gegen v0.7.0 nur wo F3 Asymmetrie greift
//! (Minderverbrauch statt Penalty).
//!
//! Spec §7.2 Drift-Tabelle:
//!   - Mehrverbrauch-Fluege: bit-identisch zu v0.7.0
//!   - Minderverbrauch-Fluege: hoeherer fuel-Score (= weniger Penalty)
//!   - Master-Score steigt entsprechend wenn fuel-Sub-Score relevant war

use landing_scoring::{aggregate_master_score, compute_sub_scores, LandingScoringInput};

fn pts(subs: &[landing_scoring::SubScoreEntry], key: &str) -> u8 {
    subs.iter()
        .find(|s| s.key == key)
        .unwrap_or_else(|| panic!("sub-score '{}' fehlt: {:?}", key, subs))
        .score
}

fn skipped(subs: &[landing_scoring::SubScoreEntry], key: &str) -> bool {
    subs.iter()
        .find(|s| s.key == key)
        .map(|s| s.skipped)
        .unwrap_or(false)
}

fn no_sub(subs: &[landing_scoring::SubScoreEntry], key: &str) {
    assert!(
        !subs.iter().any(|s| s.key == key),
        "sub-score '{}' sollte nicht vorhanden sein: {:?}",
        key,
        subs
    );
}

// ─── Replay-Fluege (Spec §7.2 nach Phase-2) ────────────────────────

#[test]
fn pto105_msfs_smooth_55fpm_with_loadsheet() {
    // PTO 105: vs=-55, g=1.10, bounces=0, stab=80/2.5, rollout=900,
    //   planned_burn=1500, actual=1515 (= +1.0%, on_plan, 100 pkt),
    //   planned_zfw=21000, planned_tow=23500 (+ block_fuel)
    //
    // v0.20.x target-corridor: |55| < 90 fpm → possible_float (85 pkt),
    // nicht mehr 100 — 55 fpm ist SEHR weich (moegliches langes
    // Abfangen), nicht mehr das erklaerte Ziel.
    //
    // Sub-Scores: landing_rate=85, g=100, bounces=100, stability=80,
    //             rollout=80, fuel=100, loadsheet=100
    // Gewichte: 3+3+2+2+1+1+1 = 13
    // Sum = 255+300+200+160+80+100+100 = 1195
    // Master = 1195/13 = 91.9 → 92
    let input = LandingScoringInput {
        vs_fpm: Some(-55.0),
        peak_g_load: Some(1.10),
        bounce_count: Some(0),
        approach_vs_stddev_fpm: Some(80.0),
        approach_bank_stddev_deg: Some(2.5),
        rollout_distance_m: Some(900.0),
        planned_burn_kg: Some(1500.0),
        actual_trip_burn_kg: Some(1515.0),
        planned_zfw_kg: Some(21000.0),
        planned_tow_kg: Some(23500.0),
        ..Default::default()
    };
    let subs = compute_sub_scores(&input);
    assert_eq!(pts(&subs, "landing_rate"), 85);
    assert_eq!(pts(&subs, "g_force"), 100);
    assert_eq!(pts(&subs, "bounces"), 100);
    assert_eq!(pts(&subs, "stability"), 80);
    assert_eq!(pts(&subs, "rollout"), 80);
    assert_eq!(pts(&subs, "fuel"), 100);
    assert_eq!(pts(&subs, "loadsheet"), 100);
    assert_eq!(aggregate_master_score(&subs), Some(92));
}

#[test]
fn dlh304_msfs_acceptable_with_underburn_no_penalty() {
    // DLH 304: vs=-357, g=1.45, bounces=0, stab=180/4.0, rollout=1500,
    //   planned_burn=8000, actual=7720 (= -3.5%, F3: nicht mehr 80
    //   wie Legacy sondern 100 weil under<5%)
    //   loadsheet: ZFW + TOW vorhanden → 100
    //
    // Vergleich Legacy vs Phase 2:
    //   Legacy fuel(-3.5%): abs<5 → 80 (near_plan)
    //   Phase 2 fuel(-3.5%): under<5 → 100 (on_plan, KEIN Penalty) ← F3
    //
    // v0.20.x target-corridor: |357| liegt jetzt im 250-400-Band
    // ("above_target") mit 80 statt 70 — die Bänder unter 400 fpm sind
    // insgesamt grosszuegiger gegen die Mitte verschoben.
    //
    // Sub-Scores: 80, 60, 100, 80, 55, 100, 100
    // Gewichte:    3,  3,   2,  2,  1,   1,   1 = 13
    // Sum = 240+180+200+160+55+100+100 = 1035
    // Master = 1035/13 = 79.6 → 80
    let input = LandingScoringInput {
        vs_fpm: Some(-357.0),
        peak_g_load: Some(1.45),
        bounce_count: Some(0),
        approach_vs_stddev_fpm: Some(180.0),
        approach_bank_stddev_deg: Some(4.0),
        rollout_distance_m: Some(1500.0),
        planned_burn_kg: Some(8000.0),
        actual_trip_burn_kg: Some(7720.0),
        planned_zfw_kg: Some(58000.0),
        planned_tow_kg: Some(70000.0),
        ..Default::default()
    };
    let subs = compute_sub_scores(&input);
    assert_eq!(pts(&subs, "landing_rate"), 80);
    assert_eq!(pts(&subs, "g_force"), 60);
    assert_eq!(pts(&subs, "bounces"), 100);
    assert_eq!(pts(&subs, "stability"), 80);
    assert_eq!(pts(&subs, "rollout"), 55);
    // F3 Asymmetrie: -3.5% Minderverbrauch nicht bestraft
    assert_eq!(pts(&subs, "fuel"), 100);
    assert_eq!(pts(&subs, "loadsheet"), 100);
    assert_eq!(aggregate_master_score(&subs), Some(80));
}

#[test]
fn cfg785_msfs_smooth_with_overburn_unchanged() {
    // CFG 785: vs=-142, g=1.18, bounces=0, stab=70/2.0, rollout=750
    //   planned_burn=4500, actual=4523 (+0.5%, on_plan = 100 wie Legacy)
    //
    // v0.20.x target-corridor: |142| liegt jetzt im 90-250-Zielkorridor
    // ("firm_positive_touchdown") → 100 statt 90 — genau die Art fester,
    // positiver Aufsetzer, die der Korridor als Ziel definiert.
    //
    // Sub-Scores: 100, 100, 100, 80, 100, 100, 100
    // Gewichte:    3,  3,  2,  2,  1,  1,  1 = 13
    // Sum = 300+300+200+160+100+100+100 = 1260
    // Master = 1260/13 = 96.9 → 97
    let input = LandingScoringInput {
        vs_fpm: Some(-142.0),
        peak_g_load: Some(1.18),
        bounce_count: Some(0),
        approach_vs_stddev_fpm: Some(70.0),
        approach_bank_stddev_deg: Some(2.0),
        rollout_distance_m: Some(750.0),
        planned_burn_kg: Some(4500.0),
        actual_trip_burn_kg: Some(4523.0),
        planned_zfw_kg: Some(58000.0),
        planned_tow_kg: Some(72000.0),
        ..Default::default()
    };
    let subs = compute_sub_scores(&input);
    assert_eq!(pts(&subs, "landing_rate"), 100);
    assert_eq!(pts(&subs, "g_force"), 100);
    assert_eq!(pts(&subs, "bounces"), 100);
    assert_eq!(pts(&subs, "stability"), 80);
    assert_eq!(pts(&subs, "rollout"), 100);
    assert_eq!(pts(&subs, "fuel"), 100);
    assert_eq!(pts(&subs, "loadsheet"), 100);
    assert_eq!(aggregate_master_score(&subs), Some(97));
}

#[test]
fn dah3181_xplane_firm_with_overburn() {
    // DAH 3181: vs=-414, g=1.55, bounces=0, stab=120/3.0, rollout=1700
    //   planned_burn=12000, actual=12960 (+8.0%, off_plan = 55 wie Legacy)
    //
    // Sub-Scores: 45, 60, 100, 80, 55, 55, 100
    // Gewichte:    3,  3,  2,  2,  1,  1,  1 = 13
    // Sum = 135+180+200+160+55+55+100 = 885
    // Master = 885/13 = 68.07 → 68 (Legacy ohne loadsheet war 65 — Anstieg
    //   weil loadsheet=100 reinkommt)
    let input = LandingScoringInput {
        vs_fpm: Some(-414.0),
        peak_g_load: Some(1.55),
        bounce_count: Some(0),
        approach_vs_stddev_fpm: Some(120.0),
        approach_bank_stddev_deg: Some(3.0),
        rollout_distance_m: Some(1700.0),
        planned_burn_kg: Some(12000.0),
        actual_trip_burn_kg: Some(12960.0),
        planned_zfw_kg: Some(75000.0),
        planned_tow_kg: Some(95000.0),
        ..Default::default()
    };
    let subs = compute_sub_scores(&input);
    assert_eq!(pts(&subs, "landing_rate"), 45);
    assert_eq!(pts(&subs, "g_force"), 60);
    assert_eq!(pts(&subs, "bounces"), 100);
    assert_eq!(pts(&subs, "stability"), 80);
    assert_eq!(pts(&subs, "rollout"), 55);
    // Mehrverbrauch wird bestraft (bit-identisch Legacy)
    assert_eq!(pts(&subs, "fuel"), 55);
    assert_eq!(pts(&subs, "loadsheet"), 100);
    assert_eq!(aggregate_master_score(&subs), Some(68));
}

// ─── F1/F2 Edge-Cases (VFR/Manual ohne Plan) ───────────────────────

#[test]
fn vfr_no_zfw_no_burn_skips_loadsheet_and_fuel() {
    // VFR-Pilot ohne SimBrief: planned_zfw=None, planned_burn=None
    //   → fuel=skipped, loadsheet=skipped
    //   → Master rechnet nur aus 5 Sub-Scores (landing_rate, g_force,
    //     bounces, stability, rollout)
    let input = LandingScoringInput {
        vs_fpm: Some(-200.0),
        peak_g_load: Some(1.30),
        bounce_count: Some(0),
        approach_vs_stddev_fpm: Some(150.0),
        approach_bank_stddev_deg: Some(3.0),
        rollout_distance_m: Some(800.0),
        // VFR: nichts geplant
        planned_burn_kg: None,
        actual_trip_burn_kg: None,
        planned_zfw_kg: None,
        planned_tow_kg: None,
        ..Default::default()
    };
    let subs = compute_sub_scores(&input);
    // v0.20.x target-corridor: |200| liegt jetzt im 90-250-Zielkorridor
    // ("firm_positive_touchdown") → 100 statt 70.
    assert_eq!(pts(&subs, "landing_rate"), 100);
    // g_force(1.30): comfortable_g = 85
    assert_eq!(pts(&subs, "g_force"), 85);
    assert_eq!(pts(&subs, "bounces"), 100);
    assert_eq!(pts(&subs, "stability"), 80);
    assert_eq!(pts(&subs, "rollout"), 80); // 800<m<1200 = good_stop
    // F1 + F2: loadsheet + fuel skipped → 0-Penalty vermieden
    assert!(skipped(&subs, "fuel"));
    assert!(skipped(&subs, "loadsheet"));
    // Master = (100*3+85*3+100*2+80*2+80*1) / (3+3+2+2+1) = (300+255+200+160+80) / 11
    //        = 995/11 = 90.45 → 90
    assert_eq!(aggregate_master_score(&subs), Some(90));
}

#[test]
fn vfr_no_burn_skips_only_fuel() {
    // VFR mit ZFW aber ohne planned_burn (z.B. Pilot hat ZFW eingegeben
    // aber keinen OFP-Plan) → loadsheet=100, fuel=skipped
    let input = LandingScoringInput {
        vs_fpm: Some(-100.0),
        peak_g_load: Some(1.15),
        bounce_count: Some(0),
        planned_burn_kg: None, // nicht geplant
        actual_trip_burn_kg: Some(2000.0), // gemessen aber ohne Plan
        planned_zfw_kg: Some(20000.0),
        planned_tow_kg: Some(22500.0),
        ..Default::default()
    };
    let subs = compute_sub_scores(&input);
    assert!(skipped(&subs, "fuel"));
    assert!(!skipped(&subs, "loadsheet"));
    assert_eq!(pts(&subs, "loadsheet"), 100);
}

// ─── F3 Asymmetrie explizit ───────────────────────────────────────

#[test]
fn underburn_minus_25_pct_warns() {
    // -25% Minderverbrauch → 85 mit Warning planned_burn_may_be_off
    let input = LandingScoringInput {
        planned_burn_kg: Some(8000.0),
        actual_trip_burn_kg: Some(6000.0),
        ..Default::default()
    };
    let subs = compute_sub_scores(&input);
    let fuel = subs.iter().find(|s| s.key == "fuel").unwrap();
    assert_eq!(fuel.score, 85);
    assert_eq!(fuel.warning.as_deref(), Some("planned_burn_may_be_off"));
}

#[test]
fn empty_input_returns_only_bounces_loadsheet_fuel() {
    // v0.20.2 — ERWARTUNG BEWUSST GEAENDERT.
    //
    // Vorher schrieb dieser Test fest: Default-Input (keinerlei Messwerte) →
    // Master = **100 Punkte**, gebildet allein aus `bounces` (default 0 Hopser
    // = perfekt). Die Sinkraten-Achse fehlte einfach, und `skipped` fliegt aus
    // der Gewichtung.
    //
    // Das war ein geschenkter Score. Solange die Kanonik immer irgendeine
    // Landerate lieferte (notfalls eine falsche), war es nur theoretisch. Seit
    // sie unplausible Werte verwirft (Flug 804, ELLX: Edge-Wert +24 fpm), ist es
    // real: ein Glitch-Flug haette die Bestnote bekommen, gerade WEIL seine
    // Landung nicht messbar war.
    //
    // Eine Landungsbewertung ohne die Sinkrate der Landung ist keine Bewertung.
    // Die Achse taucht jetzt sichtbar als "nicht bewertet" auf, und der Master
    // ist None statt einer geschenkten 100.
    let input = LandingScoringInput::default();
    let subs = compute_sub_scores(&input);
    assert!(
        skipped(&subs, "landing_rate"),
        "die Sinkraten-Achse muss sichtbar als 'nicht bewertet' erscheinen, \
         nicht stillschweigend fehlen",
    );
    no_sub(&subs, "g_force");
    no_sub(&subs, "stability");
    no_sub(&subs, "rollout");
    assert_eq!(pts(&subs, "bounces"), 100);
    assert!(skipped(&subs, "fuel"));
    assert!(skipped(&subs, "loadsheet"));
    assert_eq!(
        aggregate_master_score(&subs),
        None,
        "ohne messbare Landerate gibt es keine Note — lieber keine als eine geschenkte",
    );
}
