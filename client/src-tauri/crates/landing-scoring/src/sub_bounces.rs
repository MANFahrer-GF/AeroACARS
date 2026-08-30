//! Bounces Sub-Score. 1:1-Port von TS `subBounces` in
//! landingScoring.ts:170-175.

use crate::{Band, SubScoreEntry};

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
}
