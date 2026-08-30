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

    let vs_band: u8 = if vs < 100.0 {
        100
    } else if vs < 200.0 {
        80
    } else if vs < 400.0 {
        50
    } else if vs < 700.0 {
        25
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

    #[test]
    fn ts_voting_min_of_axes() {
        // VS=50 → 100, Bank=4° → 80, min=80 → "stable"
        assert_eq!(
            run(Some(50.0), Some(4.0)),
            Some((80, "landing.rat.stable".into()))
        );
        // VS=300 → 50, Bank=1° → 100, min=50 → "average_stability"
        assert_eq!(
            run(Some(300.0), Some(1.0)),
            Some((50, "landing.rat.average_stability".into()))
        );
        // VS=800 → 0, Bank=1° → 100, min=0 → "very_unstable"
        assert_eq!(
            run(Some(800.0), Some(1.0)),
            Some((0, "landing.rat.very_unstable".into()))
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
