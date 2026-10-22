//! Bounces Sub-Score. 1:1-Port von TS `subBounces` in
//! landingScoring.ts:170-175.

use crate::{Band, SubScoreEntry};

/// Leichte Hopser (P1/8b, Thomas 22.10.2026): mindestens 1 s ununterbrochen
/// ohne Bodenkontakt, mindestens 2 ft und unter 5 ft ueber der Bodenhoehe,
/// mit zweitem Kontakt. Teilnote 1 leichter = 85, 2+ leichte = 70. Sobald ein
/// normaler Hopser (>= 5 ft) dabei ist, gilt die normale Tabelle; leichte
/// zaehlen dann nicht zusaetzlich.
///
/// `leichte = None` heisst: der Client hat leichte Hopser nicht gezaehlt
/// (Score-Version vor 26, Altbestand) → exakt die alte Tabelle. So bleibt
/// jede gespeicherte Landung bei Nachrechnung unveraendert.
pub fn sub_bounces_mit_leichten(bounces: u32, leichte: Option<u32>) -> SubScoreEntry {
    match (bounces, leichte.unwrap_or(0)) {
        (0, 0) => sub_bounces(0),
        (0, 1) => SubScoreEntry::scored(
            "bounces",
            "landing.sub.bounces",
            85,
            "1".into(),
            "one_light_bounce",
            Band::Good,
        ),
        (0, n) => SubScoreEntry::scored(
            "bounces",
            "landing.sub.bounces",
            70,
            n.to_string(),
            "light_bounces",
            Band::Ok,
        ),
        (b, _) => sub_bounces(b),
    }
}

pub fn sub_bounces(bounces: u32) -> SubScoreEntry {
    match bounces {
        0 => SubScoreEntry::scored(
            "bounces",
            "landing.sub.bounces",
            100,
            "0".into(),
            "clean_set",
            Band::Good,
        ),
        1 => SubScoreEntry::scored(
            "bounces",
            "landing.sub.bounces",
            70,
            "1".into(),
            "one_bounce",
            Band::Ok,
        ),
        2 => SubScoreEntry::scored(
            "bounces",
            "landing.sub.bounces",
            40,
            "2".into(),
            "two_bounces",
            Band::Bad,
        ),
        n => SubScoreEntry::scored(
            "bounces",
            "landing.sub.bounces",
            15,
            n.to_string(),
            "many_bounces",
            Band::Bad,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ts_table_match() {
        let cases = [
            (0u32, 100u8, "clean_set"),
            (1, 70, "one_bounce"),
            (2, 40, "two_bounces"),
            (3, 15, "many_bounces"),
            (5, 15, "many_bounces"),
        ];
        for (b, p, r) in cases {
            let s = sub_bounces(b);
            assert_eq!(s.points, p, "bounces={}", b);
            assert_eq!(
                s.rationale_key.as_deref(),
                Some(format!("landing.rat.{}", r).as_str())
            );
            assert_eq!(s.value.as_deref(), Some(b.to_string().as_str()));
        }
    }

    /// P1/8b: Tabelle der leichten Hopser (Thomas 22.10.2026).
    #[test]
    fn leichte_hopser_tabelle() {
        // (normal, leicht, Punkte, Begruendung)
        let faelle = [
            (0u32, None, 100u8, "clean_set"),
            (0, Some(0), 100, "clean_set"),
            (0, Some(1), 85, "one_light_bounce"),
            (0, Some(2), 70, "light_bounces"),
            (0, Some(4), 70, "light_bounces"),
            // Normaler Hopser dabei: normale Tabelle, leichte zaehlen nicht.
            (1, Some(1), 70, "one_bounce"),
            (1, Some(3), 70, "one_bounce"),
            (2, Some(1), 40, "two_bounces"),
            (3, None, 15, "many_bounces"),
        ];
        for (b, l, p, r) in faelle {
            let s = sub_bounces_mit_leichten(b, l);
            assert_eq!(s.points, p, "normal={b} leicht={l:?}");
            assert_eq!(
                s.rationale_key.as_deref(),
                Some(format!("landing.rat.{r}").as_str()),
                "normal={b} leicht={l:?}"
            );
        }
        // Ohne Zaehlung der leichten (Altbestand) = exakt die alte Tabelle.
        for b in 0..6u32 {
            assert_eq!(sub_bounces_mit_leichten(b, None), sub_bounces(b));
        }
    }
}
