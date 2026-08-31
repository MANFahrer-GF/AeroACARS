//! Stability Sub-Score.
//!
//! Phase 0: 1:1-Port von TS `subStability` in landingScoring.ts:177-194
//! (2-Faktor: VS-stddev + Bank-stddev). Beide Werte muessen vorhanden
//! sein (oder beide None — dann skipped). NaN-Defaults wie TS:
//! `vs ?? 0`, `bk ?? 0` (ein Wert vorhanden, anderer None → der None
//! wird wie 0 behandelt = Score 100 in dieser Achse).
//!
//! Phase 3 (F7-B): wird durch 4-Faktor-Voting + 2 Modifier ersetzt.
//! Diese Funktion bleibt als `sub_stability_legacy` erhalten fuer
//! Goldenset-Backward-Compat-Tests.

use crate::{band_from_points, SubScoreEntry};

/// Phase-0 Legacy 2-Faktor-Stability. Returns `None` wenn beide
/// Inputs `None` sind (matched TS-Verhalten).
pub fn sub_stability_legacy(
    sigma_vs_fpm: Option<f32>,
    sigma_bank_deg: Option<f32>,
) -> Option<SubScoreEntry> {
    if sigma_vs_fpm.is_none() && sigma_bank_deg.is_none() {
        return None;
    }
    let vs = sigma_vs_fpm.unwrap_or(0.0);
    let bk = sigma_bank_deg.unwrap_or(0.0);

    // ⚠ v1.7.13: Baender geweitet und auf die Leiter 100/80/45/20/0
    // gebracht. Gemessen ueber 699 Landungen von 90 Tagen lagen **59 %**
    // ueber der alten 100er-Grenze, der Mittelwert bei 153 fpm.
    //
    // Drei Gruende:
    //
    // 1. Eine Schwelle, die die Mehrheit trifft, misst kein Koennen mehr.
    //    Dasselbe Argument wie bei den OFP-Baendern (v1.7.12).
    //
    // 2. Der Code widersprach sich selbst: Das 80-Punkte-Band heisst
    //    unten `stable`. Es zog also 20 Punkte ab fuer etwas, das die
    //    Bewertung selbst "stabil" nennt (gemeldet an ITY400,
    //    31.08.2026: σ 108 fpm, Begruendung `stable`, 80 Punkte).
    //
    // 3. σ misst zum Teil das WETTER, nicht den Piloten. Ueber dieselben
    //    699 Landungen, nach Seitenwind:
    //
    //      < 5 kt  (475 Fluege): σ 146 fpm, 44 % volle Punkte
    //      5-12 kt (199 Fluege): σ 163 fpm, 36 %
    //      12-20 kt (24 Fluege): σ 208 fpm, 29 %
    //
    //    Bei gleichem Koennen sinkt die Chance auf die volle Punktzahl
    //    mit dem Wind. Turbulenz ist kein Pilotenfehler — dieselbe
    //    Einwendung, die die Ladepapier-Achse gekostet hat.
    //
    // Die Achse bleibt, denn ein wirklich unruhiger Anflug SOLL Punkte
    // kosten. Sie greift jetzt bei rund 20 % statt bei 59 %.
    let vs_band: u8 = if vs < 200.0 {
        100
    } else if vs < 400.0 {
        80
    } else if vs < 700.0 {
        45
    } else if vs < 1000.0 {
        20
    } else {
        0
    };
    let bk_band: u8 = if bk < 2.0 {
        100
    } else if bk < 5.0 {
        80
    } else if bk < 10.0 {
        50
    } else if bk < 15.0 {
        25
    } else {
        0
    };
    let points = vs_band.min(bk_band);

    let rationale = if points >= 90 {
        "very_stable"
    } else if points >= 70 {
        "stable"
    } else if points >= 40 {
        "average_stability"
    } else if points >= 20 {
        "unstable_approach"
    } else {
        "very_unstable"
    };

    let value = format!("σ {} fpm / {:.1}°", vs.round() as i32, bk);
    Some(SubScoreEntry::scored(
        "stability",
        "landing.sub.stability",
        points,
        value,
        rationale,
        band_from_points(points),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(vs: Option<f32>, bk: Option<f32>) -> Option<(u8, String)> {
        sub_stability_legacy(vs, bk).map(|s| (s.points, s.rationale_key.unwrap()))
    }

    #[test]
    fn both_none_returns_none() {
        assert!(sub_stability_legacy(None, None).is_none());
    }

    /// Was die Bewertung "stabil" NENNT, muss auch volle Punkte geben.
    ///
    /// ⚠ Der Widerspruch, den v1.7.13 beseitigt: Bis dahin lag das
    /// 80-Punkte-Band bei `stable` — die Achse zog also 20 Punkte ab
    /// fuer etwas, das sie selbst stabil nannte. Gemeldet an ITY400
    /// (31.08.2026): σ 108 fpm, Begruendung `stable`, 80 Punkte.
    #[test]
    fn ein_ruhiger_anflug_bekommt_volle_punkte() {
        // Der gemeldete Fall: 108 fpm Streuung, 0,3 Grad Querneigung.
        assert_eq!(
            run(Some(108.0), Some(0.3)),
            Some((100, "landing.rat.very_stable".into()))
        );
        // Und der Mittelwert des Bestands (153 fpm) ebenfalls.
        assert_eq!(
            run(Some(153.0), Some(0.5)),
            Some((100, "landing.rat.very_stable".into()))
        );
    }

    /// Ein wirklich unruhiger Anflug kostet weiterhin.
    ///
    /// Der Gegenpol: Die Weitung darf die Achse nicht zahnlos machen.
    #[test]
    fn ein_unruhiger_anflug_kostet_weiterhin() {
        assert_eq!(run(Some(500.0), Some(1.0)).unwrap().0, 45);
        assert_eq!(run(Some(900.0), Some(1.0)).unwrap().0, 20);
        assert_eq!(run(Some(1200.0), Some(1.0)).unwrap().0, 0);
    }

    #[test]
    fn ts_voting_min_of_axes() {
        // VS=50 → 100, Bank=4° → 80, min=80 → "stable"
        assert_eq!(
            run(Some(50.0), Some(4.0)),
            Some((80, "landing.rat.stable".into()))
        );
        // ⚠ v1.7.13: VS=300 gibt jetzt 80, nicht mehr 50 — die Baender
        // sind geweitet (Begruendung oben bei `vs_band`). Bank=1° → 100,
        // min = 80 → "stable".
        assert_eq!(
            run(Some(300.0), Some(1.0)),
            Some((80, "landing.rat.stable".into()))
        );
        // ⚠ v1.7.13: VS=800 gibt jetzt 20 statt 0 — erst ueber 1000 fpm
        // Streuung ist der Anflug voellig aus dem Ruder.
        assert_eq!(
            run(Some(800.0), Some(1.0)),
            Some((20, "landing.rat.unstable_approach".into()))
        );
    }

    #[test]
    fn one_axis_none_treated_as_zero() {
        // TS: vs ?? 0 → wenn None=0 → vs_band=100. So bk=4° entscheidet → 80.
        assert_eq!(
            run(None, Some(4.0)),
            Some((80, "landing.rat.stable".into()))
        );
    }

    #[test]
    fn value_format_matches_ts() {
        // JS Math.round(250.5) = 251 (away-from-zero auf .5).
        // Rust f32::round: "ties round away from zero" → identisch.
        // → 250.5 rundet zu 251 in beiden Sprachen.
        let s = sub_stability_legacy(Some(250.5), Some(4.0)).unwrap();
        assert_eq!(s.value.unwrap(), "σ 251 fpm / 4.0°");

        // Auch andere Werte testen
        let s = sub_stability_legacy(Some(80.0), Some(2.5)).unwrap();
        assert_eq!(s.value.unwrap(), "σ 80 fpm / 2.5°");
    }
}
