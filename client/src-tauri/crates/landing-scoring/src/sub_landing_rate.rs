//! Landing-Rate Sub-Score. Point-banding is a 1:1 port of TS
//! `subLandingRate` in landingScoring.ts (kept in lockstep — see that
//! file). `T_VS_*` below are gespiegelt aus lib.rs TOUCHDOWN_VS_* and
//! feed `classify_by_g`/`classify_by_vs`'s CATEGORY ladder (score_label)
//! and the forensics-chart tone functions — deliberately UNCHANGED by
//! the v0.20.x corridor redesign below, so category labels and chart
//! coloring keep their existing meaning. Only the POINT bands (this
//! function's own logic) use the new, separate corridor constants.
//!
//! v0.20.x (score_algorithm_version 4→5): replaced the old monotonic
//! "softer is always better" curve with a TARGET CORRIDOR. A touchdown
//! well under ~90 fpm is not actually the goal — it's the signature of
//! a long-held flare/float, which delays derotation and therefore
//! spoiler/autobrake/reverse deployment, which is exactly what also
//! drives the rollout distance up (see sub_rollout.rs, scored
//! separately but causally the SAME technique choice). Real-world FOQA
//! landing-rate bands treat a firm, positive touchdown in roughly the
//! 90-250 fpm range as the professional target, not a compromise vs.
//! "smoother = better". The >=400 fpm bands (real hard-landing /
//! inspection triggers) are unchanged — this only touches what counts
//! as "excellent" at the soft end.

use crate::{Band, SubScoreEntry};

pub const T_VS_SMOOTH_FPM: f32 = 200.0;
pub const T_VS_FIRM_FPM: f32 = 400.0;
pub const T_VS_HARD_FPM: f32 = 600.0;
pub const T_VS_SEVERE_FPM: f32 = 1000.0;

/// Below this, a touchdown is soft enough to suggest an extended
/// flare/float rather than a deliberate firm touchdown — small
/// deduction, not a safety flag (see `possible_float` rationale).
const VS_FLOAT_RISK_BELOW_FPM: f32 = 90.0;
/// Top of the "firm, positive touchdown" target corridor — full marks
/// from `VS_FLOAT_RISK_BELOW_FPM` up to this value.
const VS_TARGET_CORRIDOR_TOP_FPM: f32 = 250.0;

pub fn sub_landing_rate(peak_vs_fpm: f32) -> SubScoreEntry {
    let vs = peak_vs_fpm.abs();
    let signed = peak_vs_fpm.round() as i32;
    let value = format!("{} fpm", if signed == 0 { 0 } else { signed });

    if vs < VS_FLOAT_RISK_BELOW_FPM {
        SubScoreEntry::scored(
            "landing_rate",
            "landing.sub.landing_rate",
            85,
            value,
            "possible_float",
            Band::Good,
        )
    } else if vs < VS_TARGET_CORRIDOR_TOP_FPM {
        SubScoreEntry::scored(
            "landing_rate",
            "landing.sub.landing_rate",
            100,
            value,
            "firm_positive_touchdown",
            Band::Good,
        )
    } else if vs < T_VS_FIRM_FPM {
        SubScoreEntry::scored(
            "landing_rate",
            "landing.sub.landing_rate",
            80,
            value,
            "above_target",
            Band::Good,
        )
    } else if vs < T_VS_HARD_FPM {
        SubScoreEntry::scored(
            "landing_rate",
            "landing.sub.landing_rate",
            45,
            value,
            "hard_landing",
            Band::Ok,
        )
    } else if vs < T_VS_SEVERE_FPM {
        SubScoreEntry::scored(
            "landing_rate",
            "landing.sub.landing_rate",
            20,
            value,
            "very_hard",
            Band::Bad,
        )
    } else {
        SubScoreEntry::scored(
            "landing_rate",
            "landing.sub.landing_rate",
            0,
            value,
            "severe_inspection",
            Band::Bad,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(vs: f32) -> (u8, &'static str) {
        let s = sub_landing_rate(vs);
        let r = match s.rationale_key.as_deref() {
            Some("landing.rat.possible_float") => "possible_float",
            Some("landing.rat.firm_positive_touchdown") => "firm_positive_touchdown",
            Some("landing.rat.above_target") => "above_target",
            Some("landing.rat.hard_landing") => "hard_landing",
            Some("landing.rat.very_hard") => "very_hard",
            Some("landing.rat.severe_inspection") => "severe_inspection",
            _ => "?",
        };
        (s.points, r)
    }

    #[test]
    fn ts_table_match() {
        // v0.20.x target-corridor bands — kept in lockstep with
        // landingScoring.ts's subLandingRate(). vs.abs() Schwellen.
        assert_eq!(run(-30.0), (85, "possible_float"));
        assert_eq!(run(-89.99), (85, "possible_float"));
        assert_eq!(run(-90.0), (100, "firm_positive_touchdown"));
        assert_eq!(run(-249.99), (100, "firm_positive_touchdown"));
        assert_eq!(run(-250.0), (80, "above_target"));
        assert_eq!(run(-399.99), (80, "above_target"));
        assert_eq!(run(-400.0), (45, "hard_landing"));
        assert_eq!(run(-599.99), (45, "hard_landing"));
        assert_eq!(run(-600.0), (20, "very_hard"));
        assert_eq!(run(-999.99), (20, "very_hard"));
        assert_eq!(run(-1000.0), (0, "severe_inspection"));
        assert_eq!(run(-2500.0), (0, "severe_inspection"));
    }

    #[test]
    fn value_format_matches_ts() {
        // TS rounded `signed` und schreibt `${signed} fpm`, 0 → "0 fpm"
        assert_eq!(sub_landing_rate(-191.4).value.unwrap(), "-191 fpm");
        assert_eq!(sub_landing_rate(0.0).value.unwrap(), "0 fpm");
        assert_eq!(sub_landing_rate(-413.7).value.unwrap(), "-414 fpm");
    }
}
