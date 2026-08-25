//! Aircraft category classification for category-aware landing detection,
//! validation, and scoring.
//!
//! The whole touchdown→landing pipeline was originally fixed-wing-shaped:
//! wheeled gear, a runway, a sharp gear-force / `on_ground` edge at a clearly
//! negative V/S. Helicopters and seaplanes break those assumptions. A
//! helicopter set-down is a deliberately-minimised near-zero-V/S event on
//! skids (no force spike); a seaplane water touchdown produces no
//! wheeled-gear contact and frequently no `on_ground=true` at all. This
//! module is the single, well-tested place that decides which kind of
//! aircraft we are looking at, so the pipeline can branch instead of
//! silently mis-handling these categories.
//!
//! The ICAO taxonomy is ported from the frontend (`client/src/lib/
//! aircraftIcon.ts`, itself from the VPS live-map; source ICAO Doc 8643).
//! Live sim gear-type signals — MSFS `IS GEAR SKIDS/FLOATS/WHEELS` and
//! `WATER RUDDER HANDLE POSITION`, X-Plane `acf_gear_is_skid` and the
//! `acf_water_rud_*` family — take precedence over the ICAO guess, because
//! the *same* ICAO type can be wheeled or float-equipped (a DHC-2 or C208 on
//! wheels vs on floats), whereas the gear-type flag reflects the actually
//! loaded configuration.

use serde::{Deserialize, Serialize};

/// Coarse, landing-relevant aircraft category. `FixedWing` is the default and
/// preserves all existing wheeled-fixed-wing behaviour unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AircraftCategory {
    /// Wheeled fixed-wing — the historical assumption. Unchanged behaviour.
    #[default]
    FixedWing,
    /// Rotorcraft (skids or wheels): vertical / run-on set-down, near-zero
    /// V/S, no glideslope, no fixed-wing wing-strike geometry.
    Helicopter,
    /// Float / hull, water-only: touches down on water, no wheeled-gear edge.
    Seaplane,
    /// Floats + wheels: can land on a runway OR water. The effective mode is
    /// decided at landing time from the live surface signals.
    Amphibian,
}

impl AircraftCategory {
    /// Rotorcraft.
    pub fn is_rotorcraft(self) -> bool {
        matches!(self, AircraftCategory::Helicopter)
    }

    /// Can put down on water (pure seaplane or amphibian).
    pub fn water_capable(self) -> bool {
        matches!(
            self,
            AircraftCategory::Seaplane | AircraftCategory::Amphibian
        )
    }

    /// Needs the category-aware (no-spike / near-zero-V/S / water) touchdown
    /// path rather than the wheeled-gear fixed-wing path.
    pub fn is_non_conventional(self) -> bool {
        !matches!(self, AircraftCategory::FixedWing)
    }

    /// Stable lower-case tag for a future telemetry / PIREP payload field.
    /// NOT yet wired into the payload — the recorder currently re-derives the
    /// category from the ICAO type (see `NON_CONVENTIONAL_ICAOS` there). Kept
    /// as the single source of truth for the tag string when that wiring lands.
    pub fn as_tag(self) -> &'static str {
        match self {
            AircraftCategory::FixedWing => "fixed_wing",
            AircraftCategory::Helicopter => "helicopter",
            AircraftCategory::Seaplane => "seaplane",
            AircraftCategory::Amphibian => "amphibian",
        }
    }
}

/// Rotorcraft ICAO type designators (ICAO Doc 8643). Ported 1:1 from the
/// frontend `HELI_ICAOS` set.
fn is_heli_icao(code: &str) -> bool {
    matches!(
        code,
        "EC20" | "EC30" | "EC35" | "EC45" | "EC55" | "EC75"
            | "AS50" | "AS55" | "AS65" | "AS32" | "AS3B"
            | "B06" | "B06T" | "B212" | "B412" | "B429" | "B407" | "B505"
            | "R22" | "R44" | "R66"
            | "S61" | "S70" | "S76" | "S92"
            | "H125" | "H135" | "H145" | "H155" | "H160" | "H175" | "H215" | "H225"
            | "MI8" | "MI17" | "MI24" | "MI26" | "MI28"
            | "CH47" | "CH53"
            | "UH60" | "UH72"
            | "A109" | "A119" | "A139" | "A149" | "A169" | "A189"
    )
}

/// Predominantly water-operating ICAO types (flying boats / amphibians that
/// are always or near-always on floats/hull). Deliberately small and
/// conservative: dual-use types that are commonly wheeled (DHC2/DHC6/C208)
/// are NOT here — for those we rely on the live float / water-rudder gear
/// flags, so a wheeled DHC-2 is never mis-tagged as a seaplane.
fn is_seaplane_icao(code: &str) -> bool {
    matches!(
        code,
        "SEAB"            // Republic RC-3 Seabee
            | "LA4"       // Lake LA-4
            | "G21"       // Grumman G-21 Goose
            | "G44"       // Grumman G-44 Widgeon
            | "G73"       // Grumman G-73 Mallard
            | "PBY"       // Consolidated PBY Catalina
    )
}

/// ICAO-only category (fallback when the sim exposes no live gear-type
/// flags). Heavy / medium / light / turboprop all collapse to `FixedWing`
/// for landing purposes — only the rotorcraft / seaplane distinction matters.
pub fn category_from_icao(icao: Option<&str>) -> AircraftCategory {
    let code = match icao {
        Some(s) => s.trim().to_ascii_uppercase(),
        None => return AircraftCategory::FixedWing,
    };
    if is_heli_icao(&code) {
        AircraftCategory::Helicopter
    } else if is_seaplane_icao(&code) {
        AircraftCategory::Seaplane
    } else {
        AircraftCategory::FixedWing
    }
}

/// Resolve the effective category, preferring live sim gear-type signals over
/// the ICAO guess. A `None` flag means "the sim didn't tell us"; the live
/// flags are the loaded configuration (float/skid/wheel) and override ICAO
/// because the same type can be flown wheeled or on floats.
pub fn resolve_category(
    icao: Option<&str>,
    gear_is_skid: Option<bool>,
    gear_is_floats: Option<bool>,
    gear_is_wheels: Option<bool>,
    water_rudder_present: Option<bool>,
) -> AircraftCategory {
    let floats = gear_is_floats == Some(true);
    let wheels = gear_is_wheels == Some(true);
    let skids = gear_is_skid == Some(true);
    let water_rudder = water_rudder_present == Some(true);
    let icao_cat = category_from_icao(icao);

    // 1. Physical float configuration is the most reliable water signal, and
    //    is checked BEFORE skids / heli-ICAO: a float-equipped aircraft lands
    //    on water regardless of being a rotorcraft, and the water touchdown
    //    path handles it. (A helicopter on floats is vanishingly rare in the
    //    sims; it resolves to Seaplane/Amphibian here — both water-capable,
    //    which is the behaviour we want for a water set-down.)
    if floats {
        // Floats + wheels = amphibian; floats alone = pure seaplane.
        return if wheels {
            AircraftCategory::Amphibian
        } else {
            AircraftCategory::Seaplane
        };
    }

    // 2. Skids ⇒ rotorcraft; so does a rotorcraft ICAO type (covers
    //    wheel-equipped helis that the skid flag misses).
    if skids || icao_cat == AircraftCategory::Helicopter {
        return AircraftCategory::Helicopter;
    }

    // 3. Water rudder present but no float flag (typical X-Plane seaplane,
    //    which exposes `acf_water_rud_*` but no float boolean). Combine with
    //    wheels / ICAO to decide amphibian vs seaplane.
    if water_rudder {
        return if wheels {
            AircraftCategory::Amphibian
        } else {
            AircraftCategory::Seaplane
        };
    }

    // 4. No live signal disambiguates → ICAO taxonomy (flying-boat seaplane
    //    types + the FixedWing default).
    icao_cat
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icao_taxonomy_classifies_helis() {
        for code in ["A109", "A139", "R44", "EC35", "H145", "UH60", "MI8", "B407"] {
            assert_eq!(
                category_from_icao(Some(code)),
                AircraftCategory::Helicopter,
                "{code} should be Helicopter"
            );
        }
    }

    #[test]
    fn icao_taxonomy_classifies_fixed_wing() {
        for code in ["A320", "B738", "C172", "E190", "CRJ9", "DH8D", "A35K"] {
            assert_eq!(
                category_from_icao(Some(code)),
                AircraftCategory::FixedWing,
                "{code} should be FixedWing"
            );
        }
    }

    #[test]
    fn icao_taxonomy_classifies_flying_boats() {
        for code in ["SEAB", "G21", "G73", "PBY", "LA4"] {
            assert_eq!(
                category_from_icao(Some(code)),
                AircraftCategory::Seaplane,
                "{code} should be Seaplane"
            );
        }
    }

    #[test]
    fn icao_handles_case_whitespace_and_none() {
        assert_eq!(category_from_icao(Some(" a109 ")), AircraftCategory::Helicopter);
        assert_eq!(category_from_icao(Some("r44")), AircraftCategory::Helicopter);
        assert_eq!(category_from_icao(None), AircraftCategory::FixedWing);
        assert_eq!(category_from_icao(Some("")), AircraftCategory::FixedWing);
    }

    #[test]
    fn live_skid_flag_means_helicopter() {
        // X-Plane heli: acf_gear_is_skid=1, no ICAO.
        assert_eq!(
            resolve_category(None, Some(true), None, None, None),
            AircraftCategory::Helicopter
        );
    }

    #[test]
    fn live_floats_only_means_seaplane() {
        // MSFS floatplane: IS GEAR FLOATS=true, no wheels.
        assert_eq!(
            resolve_category(Some("C208"), None, Some(true), Some(false), Some(true)),
            AircraftCategory::Seaplane
        );
    }

    #[test]
    fn live_floats_plus_wheels_means_amphibian() {
        assert_eq!(
            resolve_category(Some("C208"), None, Some(true), Some(true), Some(true)),
            AircraftCategory::Amphibian
        );
    }

    #[test]
    fn water_rudder_without_float_flag_is_seaplane() {
        // X-Plane seaplane: acf_water_rud_area>0 → water_rudder_present, no
        // float boolean, no wheels signal.
        assert_eq!(
            resolve_category(None, None, None, None, Some(true)),
            AircraftCategory::Seaplane
        );
    }

    #[test]
    fn water_rudder_plus_wheels_is_amphibian() {
        assert_eq!(
            resolve_category(None, None, None, Some(true), Some(true)),
            AircraftCategory::Amphibian
        );
    }

    #[test]
    fn icao_heli_with_wheels_no_skid_still_helicopter() {
        // Wheel-equipped heli (e.g. H225) reported as wheels, not skids.
        assert_eq!(
            resolve_category(Some("H225"), Some(false), Some(false), Some(true), Some(false)),
            AircraftCategory::Helicopter
        );
    }

    #[test]
    fn plain_fixed_wing_with_wheels_unchanged() {
        assert_eq!(
            resolve_category(Some("A320"), Some(false), Some(false), Some(true), Some(false)),
            AircraftCategory::FixedWing
        );
    }

    #[test]
    fn no_signals_defaults_fixed_wing() {
        assert_eq!(
            resolve_category(None, None, None, None, None),
            AircraftCategory::FixedWing
        );
    }

    #[test]
    fn live_skid_overrides_unknown_icao() {
        // ICAO not in any set, but the sim says skids → Helicopter.
        assert_eq!(
            resolve_category(Some("ZZZZ"), Some(true), None, None, None),
            AircraftCategory::Helicopter
        );
    }

    #[test]
    fn category_helpers() {
        assert!(AircraftCategory::Helicopter.is_rotorcraft());
        assert!(!AircraftCategory::Seaplane.is_rotorcraft());
        assert!(AircraftCategory::Seaplane.water_capable());
        assert!(AircraftCategory::Amphibian.water_capable());
        assert!(!AircraftCategory::Helicopter.water_capable());
        assert!(AircraftCategory::Helicopter.is_non_conventional());
        assert!(!AircraftCategory::FixedWing.is_non_conventional());
        assert_eq!(AircraftCategory::default(), AircraftCategory::FixedWing);
    }
}
