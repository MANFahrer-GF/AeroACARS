//! The actual GOLD uplink (UM) / downlink (DM) element table.
//!
//! **Phase 1 placeholder** — ~10 rows, just enough to exercise the wire
//! codec, the element engine, and the MIN/MRN threading state machine
//! end-to-end (logon handshake + the one worked CPDLC example from the
//! community docs). The full ~300-row GOLD library is a Phase 4 task:
//! transcribe the DATA from `skiselkov/libcpdlc`'s `cpdlc_msg_infos[]`
//! (MIT, C) into rows of this same shape — no engine changes needed,
//! see `elements.rs`'s module docs.
//!
//! Ids here (`"DM_WILCO"`, `"UM_LOGON_ACCEPTED"`, ...) are descriptive
//! placeholders, NOT real GOLD element numbers (e.g. the real "UM74"
//! for a similar instruction) — those aren't transcribed yet, and
//! guessing them here would assert false authority on the numbering.

use crate::cpdlc::ResponseRequirement;
use crate::elements::{Direction, ElementSpec, PlaceholderKind};

pub const UM_TABLE: &[ElementSpec] = &[
    ElementSpec {
        id: "UM_LOGON_ACCEPTED",
        direction: Direction::Uplink,
        template: "LOGON ACCEPTED",
        placeholders: &[],
        response: ResponseRequirement::NoResponseExpected,
    },
    ElementSpec {
        id: "UM_LOGON_UNABLE",
        direction: Direction::Uplink,
        template: "UNABLE",
        placeholders: &[],
        response: ResponseRequirement::NoResponseExpected,
    },
    ElementSpec {
        id: "UM_PROCEED_DIRECT_TO",
        direction: Direction::Uplink,
        // Worked example from the community docs
        // (`devHazz/hoppie-acars-docs`, `Messaging/CPDLC Format.md`).
        template: "PROCEED DIRECT TO @1",
        placeholders: &[PlaceholderKind::Waypoint],
        response: ResponseRequirement::WilcoUnable,
    },
];

pub const DM_TABLE: &[ElementSpec] = &[
    ElementSpec {
        id: "DM_REQUEST_LOGON",
        direction: Direction::Downlink,
        template: "REQUEST LOGON",
        placeholders: &[],
        response: ResponseRequirement::AnyRequired,
    },
    ElementSpec {
        id: "DM_WILCO",
        direction: Direction::Downlink,
        template: "WILCO",
        placeholders: &[],
        response: ResponseRequirement::NotRequired,
    },
    ElementSpec {
        id: "DM_UNABLE",
        direction: Direction::Downlink,
        template: "UNABLE",
        placeholders: &[],
        response: ResponseRequirement::NotRequired,
    },
    ElementSpec {
        id: "DM_AFFIRM",
        direction: Direction::Downlink,
        template: "AFFIRM",
        placeholders: &[],
        response: ResponseRequirement::NotRequired,
    },
    ElementSpec {
        id: "DM_NEGATIVE",
        direction: Direction::Downlink,
        template: "NEGATIVE",
        placeholders: &[],
        response: ResponseRequirement::NotRequired,
    },
    ElementSpec {
        id: "DM_ROGER",
        direction: Direction::Downlink,
        template: "ROGER",
        placeholders: &[],
        response: ResponseRequirement::NotRequired,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_duplicate_ids_within_a_table() {
        for table in [UM_TABLE, DM_TABLE] {
            let mut seen = std::collections::HashSet::new();
            for spec in table {
                assert!(seen.insert(spec.id), "duplicate element id {}", spec.id);
            }
        }
    }

    #[test]
    fn every_row_direction_matches_its_table() {
        for spec in UM_TABLE {
            assert_eq!(spec.direction, Direction::Uplink, "{} in UM_TABLE", spec.id);
        }
        for spec in DM_TABLE {
            assert_eq!(spec.direction, Direction::Downlink, "{} in DM_TABLE", spec.id);
        }
    }
}
