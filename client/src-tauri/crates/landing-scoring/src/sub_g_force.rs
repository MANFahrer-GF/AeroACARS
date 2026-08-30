//! G-Force Sub-Score.
//!
//! # v1.7.12: Punktleiter an die Sinkrate angeglichen
//!
//! Die SCHWELLEN beider Achsen waren schon immer sauber aufeinander
//! abgebildet — 400 fpm entsprechen etwa 1,4 g, 600 fpm etwa 1,7 g,
//! 1000 fpm etwa 2,1 g. Die PUNKTE dahinter waren es nicht:
//!
//!   Haerte          Sinkrate        G-Kraft (alt)   Differenz
//!   weich           100             100                 0
//!   leicht darueber  80              85                +5
//!   hart             45              60               +15
//!   sehr hart        20              30               +10
//!
//! Dieselbe Landung hiess auf der einen Achse „hart, 45 Punkte" und auf
//! der anderen „spuerbar, 60 Punkte". Zwei Messungen desselben Vorgangs,
//! die bei gleicher Haerte verschiedene Noten geben.
//!
//! ⚠ Gemessen ueber 675 Landungen seit Juni: Die G-Achse lag im Mittel
//! **22 Punkte ueber** der Sinkrate, sobald diese eine harte Landung
//! meldete (45,0 gegen 67,0 bei 88 Faellen). Weil beide zusammen 6 von
//! 16 Gewichten tragen, zog die mildere die haertere systematisch nach
//! oben — die Abfederung war am staerksten genau dort, wo die Bewertung
//! beissen soll.
//!
//! Die Schwellen bleiben unveraendert; nur die Noten sind jetzt
//! konsistent: 100 / 80 / 45 / 20 / 0, wie bei der Sinkrate.
//!
//! (Urspruenglich 1:1-Port von TS `subGForce` in
//! landingScoring.ts:161-168.)

use crate::{Band, SubScoreEntry};

pub const T_G_SMOOTH: f32 = 1.20;
pub const T_G_FIRM: f32 = 1.40;
pub const T_G_HARD: f32 = 1.70;
pub const T_G_SEVERE: f32 = 2.10;

pub fn sub_g_force(peak_g: f32) -> SubScoreEntry {
    let value = format!("{:.2} G", peak_g);

    if peak_g < T_G_SMOOTH {
        SubScoreEntry::scored("g_force", "landing.sub.g_force", 100, value, "smooth_g", Band::Good)
    } else if peak_g < T_G_FIRM {
        SubScoreEntry::scored(
            "g_force",
            "landing.sub.g_force",
            80,
            value,
            "comfortable_g",
            Band::Good,
        )
    } else if peak_g < T_G_HARD {
        SubScoreEntry::scored(
            "g_force",
            "landing.sub.g_force",
            45,
            value,
            "noticeable_g",
            Band::Ok,
        )
    } else if peak_g < T_G_SEVERE {
        SubScoreEntry::scored("g_force", "landing.sub.g_force", 20, value, "firm_g", Band::Bad)
    } else {
        SubScoreEntry::scored("g_force", "landing.sub.g_force", 0, value, "severe_g", Band::Bad)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(g: f32) -> (u8, String) {
        let s = sub_g_force(g);
        (s.points, s.rationale_key.unwrap())
    }

    #[test]
    fn ts_table_match() {
        // landingScoring.ts:161-168
        assert_eq!(run(1.0), (100, "landing.rat.smooth_g".into()));
        assert_eq!(run(1.19), (100, "landing.rat.smooth_g".into()));
        assert_eq!(run(1.20), (80, "landing.rat.comfortable_g".into()));
        assert_eq!(run(1.39), (80, "landing.rat.comfortable_g".into()));
        assert_eq!(run(1.40), (45, "landing.rat.noticeable_g".into()));
        assert_eq!(run(1.69), (45, "landing.rat.noticeable_g".into()));
        assert_eq!(run(1.70), (20, "landing.rat.firm_g".into()));
        assert_eq!(run(2.09), (20, "landing.rat.firm_g".into()));
        assert_eq!(run(2.10), (0, "landing.rat.severe_g".into()));
        assert_eq!(run(3.5), (0, "landing.rat.severe_g".into()));
    }

    #[test]
    fn value_format_matches_ts() {
        assert_eq!(sub_g_force(1.32).value.unwrap(), "1.32 G");
        assert_eq!(sub_g_force(1.0).value.unwrap(), "1.00 G");
    }
}

#[cfg(test)]
mod leitern_gleich_tests {
    use super::*;
    use crate::sub_landing_rate::sub_landing_rate;

    /// Dieselbe Haerte, dieselbe Note — auf beiden Achsen.
    ///
    /// ⚠ DIE Wache gegen das Auseinanderlaufen. Bis v1.7.12 hiess
    /// dieselbe Landung auf der Sinkrate „hart, 45 Punkte" und auf der
    /// G-Achse „spuerbar, 60 Punkte". Gemessen ueber 675 Landungen lag
    /// die G-Achse im Mittel 22 Punkte hoeher, sobald die Sinkrate eine
    /// harte Landung meldete — und weil beide zusammen 6 von 16
    /// Gewichten tragen, zog die mildere die haertere nach oben.
    ///
    /// Die Schwellen sind fachlich aufeinander abgebildet (400 fpm ≈
    /// 1,4 g, 600 fpm ≈ 1,7 g, 1000 fpm ≈ 2,1 g). Wer eine der beiden
    /// Leitern anfasst, muss die andere mit anfassen — sonst faellt
    /// dieser Test.
    #[test]
    fn beide_achsen_benoten_dieselbe_haerte_gleich() {
        // Je ein Punkt MITTEN in jedem Band, damit Rundungen an den
        // Grenzen nichts verfaelschen.
        let paare = [
            (-150.0_f32, 1.10_f32, "weich"),
            (-300.0, 1.30, "leicht darueber"),
            (-500.0, 1.55, "hart"),
            (-800.0, 1.90, "sehr hart"),
            (-1500.0, 2.50, "Pruefung faellig"),
        ];
        for (fpm, g, name) in paare {
            let vs = sub_landing_rate(fpm).score;
            let gf = sub_g_force(g).score;
            assert_eq!(
                vs, gf,
                "{name}: Sinkrate {fpm} fpm gibt {vs}, G-Kraft {g} g gibt {gf} — \
                 dieselbe Haerte muss dieselbe Note geben"
            );
        }
    }
}
