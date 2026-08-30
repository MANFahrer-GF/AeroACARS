//! Fuel Sub-Score.
//!
//! Phase 0 (jetzt): 1:1-Port von TS `subFuel` in landingScoring.ts:207-216.
//!   - Symmetrische Schwelle (Math.abs(efficiency)).
//!   - Returns `None` wenn efficiency_pct nicht verfuegbar ist.
//!
//! Phase 2/F2+F3 (spaeter): wird durch `sub_fuel_v0_7_1` ersetzt mit
//!   - Hard-Gate: kein planned_burn → skipped (kein Fallback)
//!   - Asymmetrie: Minderverbrauch nicht bestrafen
//!   - Label-Wechsel "Spritverbrauch" → "OFP-Treue"
//!
//! v1.6.7 (score_algorithm_version 7→8): die Asymmetrie war nur halb
//! umgesetzt. Der Kopf sagte „Minderverbrauch wird nicht bestraft", die
//! Tabelle zog ab 5 % Minderverbrauch trotzdem 5 Punkte ab (95 statt 100)
//! — der Widerspruch stammt 1:1 aus dem Spec-Entwurf (docs/spec/historical/
//! v0.7.1-landing-ux-fairness.md §F3, „nie Strafe" direkt ueber `score(95,
//! ...)`). Ausloeser war EWG 2047 (EDDH→EDDS, 16.08.2026): 2047,8 statt
//! 2163 kg geplant = -5,3 %, also 0,32 Prozentpunkte ueber der Grenze —
//! sieben Kilo mehr Verbrauch haetten die volle Punktzahl gegeben.
//! Jetzt: Minderverbrauch bis 15 % = 100 Punkte. Erst jenseits der 15 %
//! bleibt es bei 85 + Warnung — und auch das ist keine Strafe fuers
//! Sparen, sondern der Hinweis, dass eher der Plan als der Flug falsch war.
//!
//! Phase 0 behaelt die Legacy-Funktion fuer Goldenset-Tests.
//! Phase 2 fuegt `sub_fuel_v0_7_1` hinzu — wird ab v0.7.1 verwendet.

use crate::{Band, SubScoreEntry};

/// v0.7.1 Phase 2 (F2 + F3): Fuel-Score mit Hard-Gate + Asymmetrie.
///
/// F2: kein planned_burn → skipped (KEIN Fallback)
///     kein actual_trip_burn → skipped
/// F3: efficiency = (actual - planned) / planned * 100
///     Mehrverbrauch (efficiency > 0): wie Legacy bestraft
///     Minderverbrauch (efficiency <= 0): NIE bestraft — bis 15 % unter
///       Plan volle 100 Punkte (v1.6.7; vorher 95 ab 5 % unter Plan)
///     Starker Minderverbrauch (>15% under): 85 + Warning
///       "planned_burn_may_be_off" — Zweifel am Plan, nicht am Piloten
/// Label-Aenderung: "Spritverbrauch" → "OFP-Treue" (i18n key bleibt
/// `landing.sub.fuel`, der String dahinter aendert sich in Phase 3).
/// Toleranzboden als Anteil des Abfluggewichts.
///
/// # Warum ein Boden ueberhaupt, und warum kein fester
///
/// Ein reiner Prozentsatz bestraft KURZE Fluege systematisch: Rollzeit,
/// ein Vektor, eine Warteschleife kosten immer etwa dasselbe, laufen bei
/// einem kurzen Flug aber gegen einen kleinen Nenner.
///
/// Gemeldet an einem Flug mit 208 t Abfluggewicht und nur 4,78 t
/// geplantem Verbrauch (knapp eine Stunde fuer ein Grossraumflugzeug):
/// 400 kg mehr = **+8 %** und damit 55 statt 100 Punkte. Dieselben 400 kg
/// auf einem Zwoelfstundenflug waeren 0,4 % und volle Punktzahl.
///
/// Ein FESTER Boden waere aber genauso falsch: 400 kg sind fuer eine
/// C172 mehr als ihr ganzer Reiseverbrauch. Deshalb waechst er mit dem
/// Abfluggewicht — 0,2 % davon entsprechen grob ein paar Minuten Flug,
/// unabhaengig von der Groesse:
///
///   208 t  →  416 kg      57 t  →  114 kg      1,1 t  →  2,2 kg
///
/// Ohne bekanntes Abfluggewicht bleibt es beim reinen Prozentsatz.
const TOLERANZ_ANTEIL_VOM_TOW: f32 = 0.002;

pub fn sub_fuel_v0_7_1(
    planned_burn_kg: Option<f32>,
    actual_trip_burn_kg: Option<f32>,
    planned_tow_kg: Option<f32>,
    diverted: Option<bool>,
) -> SubScoreEntry {
    // ⚠ Nach einem Ausweichflug ist die OFP-Treue nicht bewertbar.
    //
    // Es wurde eine ANDERE Strecke geflogen als die geplante. Ein
    // Vergleich gegen den urspruenglichen Verbrauch misst den Umweg,
    // nicht den Piloten — und faellt je nach Richtung als "gespart" oder
    // "verschwendet" aus, beides ohne Aussage.
    //
    // (Thomas, 30.08.2026: „starker Minderverbrauch kann ja nicht sein,
    // oder nur bei Divert." Genau so — deshalb hier ueberspringen statt
    // eine Zahl zu erfinden.)
    if diverted == Some(true) {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "diverted");;
    }
    let Some(planned) = planned_burn_kg else {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "no_planned_burn");
    };
    if planned <= 0.0 {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "no_planned_burn");
    }
    let Some(actual) = actual_trip_burn_kg else {
        return SubScoreEntry::skipped("fuel", "landing.sub.fuel", "no_actual_burn");
    };

    let efficiency = ((actual - planned) / planned) * 100.0;
    let value = if efficiency > 0.0 {
        format!("+{:.1}%", efficiency)
    } else {
        format!("{:.1}%", efficiency)
    };

    // ⚠ Der Toleranzboden gilt VOR den Prozentbaendern.
    let toleranz_kg = planned_tow_kg
        .filter(|t| *t > 0.0)
        .map(|t| t * TOLERANZ_ANTEIL_VOM_TOW)
        .unwrap_or(0.0);
    if efficiency > 0.0 && (actual - planned) <= toleranz_kg {
        return SubScoreEntry::scored(
            "fuel",
            "landing.sub.fuel",
            100,
            value,
            "on_plan",
            Band::Good,
        );
    }

    if efficiency > 0.0 {
        // Mehrverbrauch — score-relevant wie Legacy
        // ⚠ Band auf +5 % geweitet (vorher +2 %).
        //
        // Gemessen ueber 412 Fluege seit dem 1. Juli: **45 % aller
        // Grossraum-Landungen** und 21 % der schmaleren lagen ueber +2 %
        // und verloren Punkte. Eine Schwelle, die die Mehrheit trifft,
        // misst kein Koennen mehr, sondern Rauschen — in der Luftfahrt
        // gilt eine OFP-Genauigkeit von ±5 % als gut.
        //
        // Wind weicht vom Prognosewert ab, ATC vektort, Steigprofile
        // unterscheiden sich. Nichts davon ist dem Piloten anzulasten.
        if efficiency < 5.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "on_plan",
                Band::Good,
            )
        } else if efficiency < 10.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                80,
                value,
                "near_plan",
                Band::Good,
            )
        } else if efficiency < 20.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                55,
                value,
                "off_plan",
                Band::Ok,
            )
        } else if efficiency < 35.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                25,
                value,
                "very_off_plan",
                Band::Bad,
            )
        } else {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                5,
                value,
                "way_off_plan",
                Band::Bad,
            )
        }
    } else {
        // Minderverbrauch (efficiency <= 0) — KEIN Penalty. v1.6.7: das
        // gilt jetzt auch zwischen 5 % und 15 % unter Plan (vorher 95).
        // Die Rationale bleibt getrennt, damit der Pilot „Effizient
        // (Minderverbrauch)" liest und nicht „Auf Plan" — gleiche Punkte,
        // ehrlichere Aussage.
        let under = efficiency.abs();
        if under < 5.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "on_plan",
                Band::Good,
            )
        } else if under < 15.0 {
            SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "efficient",
                Band::Good,
            )
        } else {
            // ⚠ Auch starker Minderverbrauch kostet KEINE Punkte mehr.
            //
            // Vorher: 85 Punkte ab 15 % unter Plan. Der Kopf dieser Datei
            // sagt seit jeher „Minderverbrauch wird nie bestraft" — und
            // die Tabelle tat es trotzdem, nur eine Stufe weiter hinten.
            // Zum zweiten Mal derselbe Widerspruch (v1.6.7 hat ihn schon
            // einmal zwischen 5 % und 15 % beseitigt).
            //
            // Sparen darf nichts kosten. Der Hinweis, dass eher der PLAN
            // als der Flug falsch war, bleibt als Warnung erhalten — aber
            // als Information, nicht als Abzug.
            let mut entry = SubScoreEntry::scored(
                "fuel",
                "landing.sub.fuel",
                100,
                value,
                "very_efficient",
                Band::Good,
            );
            entry.warning = Some("planned_burn_may_be_off".to_string());
            entry
        }
    }
}

pub fn sub_fuel_legacy(efficiency_pct: Option<f32>) -> Option<SubScoreEntry> {
    let pct = efficiency_pct?;
    let dev = pct.abs();
    let value = if pct > 0.0 {
        format!("+{:.1}%", pct)
    } else {
        format!("{:.1}%", pct)
    };

    let entry = if dev < 2.0 {
        SubScoreEntry::scored("fuel", "landing.sub.fuel", 100, value, "on_plan", Band::Good)
    } else if dev < 5.0 {
        SubScoreEntry::scored("fuel", "landing.sub.fuel", 80, value, "near_plan", Band::Good)
    } else if dev < 10.0 {
        SubScoreEntry::scored("fuel", "landing.sub.fuel", 55, value, "off_plan", Band::Ok)
    } else if dev < 20.0 {
        SubScoreEntry::scored("fuel", "landing.sub.fuel", 25, value, "very_off_plan", Band::Bad)
    } else {
        SubScoreEntry::scored("fuel", "landing.sub.fuel", 5, value, "way_off_plan", Band::Bad)
    };
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(pct: f32) -> (u8, String) {
        let s = sub_fuel_legacy(Some(pct)).unwrap();
        (s.points, s.rationale_key.unwrap())
    }

    #[test]
    fn none_returns_none() {
        assert!(sub_fuel_legacy(None).is_none());
    }

    #[test]
    fn ts_table_match_symmetric() {
        // Phase-0 Legacy: Math.abs → -5% gleich +5%
        assert_eq!(run(0.0), (100, "landing.rat.on_plan".into()));
        assert_eq!(run(1.99), (100, "landing.rat.on_plan".into()));
        assert_eq!(run(-1.99), (100, "landing.rat.on_plan".into()));
        assert_eq!(run(2.0), (80, "landing.rat.near_plan".into()));
        assert_eq!(run(-2.0), (80, "landing.rat.near_plan".into()));
        assert_eq!(run(4.99), (80, "landing.rat.near_plan".into()));
        assert_eq!(run(5.0), (55, "landing.rat.off_plan".into()));
        assert_eq!(run(-7.5), (55, "landing.rat.off_plan".into()));
        assert_eq!(run(10.0), (25, "landing.rat.very_off_plan".into()));
        assert_eq!(run(-15.0), (25, "landing.rat.very_off_plan".into()));
        assert_eq!(run(20.0), (5, "landing.rat.way_off_plan".into()));
        assert_eq!(run(-30.0), (5, "landing.rat.way_off_plan".into()));
    }

    #[test]
    fn value_format_matches_ts() {
        assert_eq!(sub_fuel_legacy(Some(5.2)).unwrap().value.unwrap(), "+5.2%");
        assert_eq!(sub_fuel_legacy(Some(-5.2)).unwrap().value.unwrap(), "-5.2%");
        assert_eq!(sub_fuel_legacy(Some(0.0)).unwrap().value.unwrap(), "0.0%");
    }

    // ─── v0.7.1 sub_fuel_v0_7_1 (F2 Hard-Gate + F3 Asymmetrie) ────────

    #[test]
    fn v0_7_1_hard_gate_no_planned() {
        let s = sub_fuel_v0_7_1(None, Some(5000.0), None, None);
        assert!(s.skipped);
        assert_eq!(s.reason.as_deref(), Some("no_planned_burn"));
        assert_eq!(s.score, 0); // skipped → 0 — wird via aggregate ignoriert
    }

    #[test]
    fn v0_7_1_hard_gate_no_actual() {
        let s = sub_fuel_v0_7_1(Some(5000.0), None, None, None);
        assert!(s.skipped);
        assert_eq!(s.reason.as_deref(), Some("no_actual_burn"));
    }

    #[test]
    fn v0_7_1_hard_gate_zero_planned() {
        let s = sub_fuel_v0_7_1(Some(0.0), Some(5000.0), None, None);
        assert!(s.skipped);
        assert_eq!(s.reason.as_deref(), Some("no_planned_burn"));
    }

    #[test]
    fn v0_7_1_overburn_punished() {
        // v1.7.12: +7 % gibt jetzt 80 statt 55 — das 100-Punkte-Band
        // reicht bis +5 %, siehe `ofp_treue_baender_v1_7_12`.
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(5350.0), None, None); // +7%
        assert_eq!(s.score, 80);
        // Der Begruendungstext wandert mit: +7 % heisst jetzt "nahe am
        // Plan", nicht mehr "abseits". Die Texte sind gegen die neuen
        // Baender geprueft — 5 / 10 / 20 / 35 %.
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.near_plan"));
        assert!(s.warning.is_none());
    }

    #[test]
    fn v0_7_1_underburn_not_punished() {
        // v1.6.7: -10% Minderverbrauch → 100 (efficient), KEIN Warning.
        // Vorher 95 — der Abzug widersprach der eigenen Doku.
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(4500.0), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.efficient"));
        assert!(s.warning.is_none());
    }

    /// v1.6.7 — der Fall, der die Aenderung ausgeloest hat: EWG 2047
    /// (EDDH→EDDS, 16.08.2026). Geplant 2163 kg, verbrannt 2047,83 kg =
    /// -5,32 %. Lag 0,32 Prozentpunkte hinter der alten 5-%-Grenze und
    /// kostete deshalb 5 Punkte auf der Achse (98 statt 100 gesamt).
    #[test]
    fn v1_6_7_ewg2047_underburn_scores_full() {
        let s = sub_fuel_v0_7_1(Some(2163.0), Some(2047.8259), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.efficient"));
        assert_eq!(s.value.as_deref(), Some("-5.3%"));
        assert!(s.warning.is_none());
    }

    /// Bandgrenzen des Minderverbrauchs am Stueck — inklusive der Stelle,
    /// an der es NICHT mehr 100 gibt. Ohne diesen Test kann die
    /// 15-%-Warnschwelle still mitwandern, wenn jemand die Zahlen
    /// anfasst.
    #[test]
    fn v1_6_7_underburn_band_edges() {
        // knapp unter der alten 5-%-Grenze — war schon immer 100
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(950.5), None, None); // -4,95 %
        assert_eq!((s.score, s.rationale_key.as_deref()), (100, Some("landing.rat.on_plan")));
        // exakt auf der alten Grenze — hier stand vorher die 95
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(950.0), None, None); // -5,0 %
        assert_eq!((s.score, s.rationale_key.as_deref()), (100, Some("landing.rat.efficient")));
        // kurz vor der Warnschwelle — weiterhin volle Punktzahl
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(850.5), None, None); // -14,95 %
        assert_eq!((s.score, s.rationale_key.as_deref()), (100, Some("landing.rat.efficient")));
        assert!(s.warning.is_none());
        // ⚠ v1.7.12: Ab hier zweifelt die Bewertung am Plan, nicht am
        // Piloten — und sagt das jetzt als HINWEIS statt als Abzug.
        // Vorher 85 Punkte; Sparen darf nichts kosten (Thomas,
        // 30.08.2026). Siehe `sparen_kostet_nie_punkte`.
        let s = sub_fuel_v0_7_1(Some(1000.0), Some(850.0), None, None); // -15,0 %
        assert_eq!((s.score, s.rationale_key.as_deref()), (100, Some("landing.rat.very_efficient")));
        assert_eq!(s.warning.as_deref(), Some("planned_burn_may_be_off"));
    }

    /// Mehrverbrauch bleibt unangetastet — die Aenderung ist einseitig.
    #[test]
    fn ofp_treue_baender_v1_7_12() {
        // ⚠ Die Baender wurden am 30.08.2026 geweitet. Anlass: Gemessen
        // ueber 412 Fluege lagen **45 % aller Grossraum-Landungen** und
        // 21 % der schmaleren ueber der alten +2-%-Grenze und verloren
        // Punkte. Eine Schwelle, die die Mehrheit trifft, misst kein
        // Koennen mehr, sondern Rauschen — in der Luftfahrt gilt eine
        // OFP-Genauigkeit von ±5 % als gut.
        //
        // TOW hier None, damit NUR die Prozentbaender geprueft werden;
        // der Toleranzboden hat eigene Tests.
        let f = |geplant: f32, echt: f32| sub_fuel_v0_7_1(Some(geplant), Some(echt), None, None).score;
        assert_eq!(f(1000.0, 1019.0), 100); // +1,9 %
        assert_eq!(f(1000.0, 1049.0), 100); // +4,9 % — vorher 80
        assert_eq!(f(1000.0, 1053.0), 80);  // +5,3 % — vorher 55
        assert_eq!(f(1000.0, 1070.0), 80);  // +7,0 %
        assert_eq!(f(1000.0, 1150.0), 55);  // +15,0 % — vorher 25
        assert_eq!(f(1000.0, 1250.0), 25);  // +25,0 %
        assert_eq!(f(1000.0, 1400.0), 5);   // +40,0 %
    }

    #[test]
    fn sparen_kostet_nie_punkte() {
        // ⚠ Der Kopf dieser Datei sagt seit jeher „Minderverbrauch wird
        // nie bestraft" — und die Tabelle tat es trotzdem, zuletzt mit
        // 85 Punkten ab 15 % unter Plan. Zum ZWEITEN Mal derselbe
        // Widerspruch (v1.6.7 hat ihn zwischen 5 % und 15 % beseitigt).
        let f = |geplant: f32, echt: f32| sub_fuel_v0_7_1(Some(geplant), Some(echt), None, None);
        for unter in [1.0_f32, 5.0, 15.0, 30.0, 60.0] {
            let e = f(1000.0, 1000.0 * (1.0 - unter / 100.0));
            assert_eq!(e.score, 100, "-{unter} % kostete Punkte");
        }
        // Der HINWEIS bleibt — als Information, nicht als Abzug.
        assert!(f(1000.0, 300.0).warning.is_some());
    }

    #[test]
    fn der_toleranzboden_waechst_mit_dem_flugzeug() {
        // ⚠ Der gemeldete Fall: 208 t Abfluggewicht, 4,78 t geplanter
        // Verbrauch (knapp eine Stunde fuer ein Grossraumflugzeug),
        // 400 kg mehr = +8,4 %. Das kostete 55 statt 100 Punkte, obwohl
        // 400 kg dort ein Rollweg mehr sind.
        assert_eq!(
            sub_fuel_v0_7_1(Some(4780.0), Some(5180.0), Some(208_400.0), None).score,
            100,
            "400 kg auf einem 208-t-Flugzeug sind kein Mehrverbrauch"
        );
        // Ohne bekanntes Abfluggewicht bleibt es beim Prozentsatz.
        assert_eq!(
            sub_fuel_v0_7_1(Some(4780.0), Some(5180.0), None, None).score,
            80
        );
        // ⚠ Und er darf NICHT fest sein: Fuer eine C172 (1,1 t) waeren
        // 400 kg mehr als ihr ganzer Reiseverbrauch.
        assert_eq!(
            sub_fuel_v0_7_1(Some(60.0), Some(120.0), Some(1_100.0), None).score,
            5,
            "eine Verdopplung bei einer C172 muss sichtbar bleiben"
        );
    }

    #[test]
    fn nach_einem_divert_wird_nicht_bewertet() {
        // Es wurde eine ANDERE Strecke geflogen. Ein Vergleich gegen den
        // urspruenglichen Plan misst den Umweg, nicht den Piloten.
        let e = sub_fuel_v0_7_1(Some(5000.0), Some(2000.0), Some(70_000.0), Some(true));
        assert!(e.skipped, "nach einem Divert darf nicht bewertet werden");
        assert_eq!(e.reason.as_deref(), Some("diverted"));
    }

    #[test]
    fn v0_7_1_strong_underburn_warns() {
        // v1.7.12: -25 % gibt volle Punktzahl PLUS Hinweis. Der Hinweis
        // bleibt wichtig — ein so starker Minderverbrauch ist bei einem
        // planmaessigen Flug kaum moeglich, da stimmt eher der Plan
        // nicht (oder es war ein Divert, der eigene Zweig davor).
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(3750.0), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.very_efficient"));
        assert_eq!(s.warning.as_deref(), Some("planned_burn_may_be_off"));
    }

    #[test]
    fn v0_7_1_on_plan() {
        // Exact match → 100 (on_plan)
        let s = sub_fuel_v0_7_1(Some(5000.0), Some(5000.0), None, None);
        assert_eq!(s.score, 100);
        assert_eq!(s.rationale_key.as_deref(), Some("landing.rat.on_plan"));
    }
}
