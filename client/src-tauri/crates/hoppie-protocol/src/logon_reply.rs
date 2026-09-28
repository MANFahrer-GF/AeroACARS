//! What a controller's answer to our `REQUEST LOGON` says, read from its
//! TEXT rather than from a GOLD element id.
//!
//! Hoppie has no dedicated logon-response element: the "LOGON ACCEPTED"
//! convention is just text, and controller clients word it differently.
//! Until v1.9.5 only the exact element `UM_LOGON_ACCEPTED` ("LOGON
//! ACCEPTED") counted — anything else left the logon on "pending" until
//! the 180 s timeout, and a raw (headerless) acceptance was even taken
//! for a refusal. Compared 28.09.2026 against four shipping clients:
//!
//! - PMDG 777 (`CCommPageComms::validate_logon_accepted`) accepts
//!   `LOGON ACCEPTED`, `LOGON SUCCESSFUL`, `CONNECTION ACCEPTED`,
//!   `LOGGED ON` and a bare `ACCEPTED`.
//! - Fenix A320 (`FenixSystem.exe`, CPDLC manager) accepts any reply to
//!   its logon (MRN = logon MIN) whose text contains `ACCEPTED`, and
//!   ends the attempt on `FLIGHT PLAN NOT HELD`.
//! - iniBuilds A350 knows `LOGON ACCEPTED` / `LOGON REJECTED`.
//!
//! Two strengths of evidence, mirroring Fenix:
//!
//! - The uplink's MRN points at our `REQUEST LOGON`: the controller is
//!   answering exactly that, so any accept/refuse wording counts.
//! - No MRN (vSMR sets one on nothing): only wording that is about a
//!   logon or connection counts. A bare `ACCEPTED` without MRN still
//!   counts — PMDG takes it, and the caller has already checked that the
//!   sender is the station we are logging on to.
//!
//! Refusal wording is checked FIRST, so `NOT ACCEPTED` or `LOGON
//! ACCEPTED ... REJECTED` never reads as an acceptance.

/// The logon verdict carried by a reply's text, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogonReply {
    Accepted,
    Refused,
}

/// Uppercase, `@`/`_` (Hoppie's line-break and filler conventions) as
/// spaces, runs of whitespace collapsed.
fn normalize(text: &str) -> String {
    text.to_uppercase()
        .replace(['@', '_'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Refusal wording. `FLIGHT PLAN NOT HELD` is Fenix's; `REJECTED` covers
/// `LOGON REJECTED` (iniBuilds) and `CONNECTION REJECTED`; `NOT
/// ACCEPTED` and `DENIED` are the plain negations.
const REFUSAL_PHRASES: &[&str] = &["FLIGHT PLAN NOT HELD", "REJECTED", "NOT ACCEPTED", "DENIED"];

fn says_refused(n: &str) -> bool {
    REFUSAL_PHRASES.iter().any(|p| n.contains(p))
}

fn says_accepted(n: &str) -> bool {
    n.contains("ACCEPTED")
        || n.contains("LOGON SUCCESSFUL")
        || (n.contains("LOGGED ON") && !n.contains("NOT LOGGED ON"))
}

/// Wording that can only be about a logon/connection — what an answer
/// WITHOUT an MRN has to show before it may settle our logon.
fn is_about_the_logon(n: &str) -> bool {
    n == "ACCEPTED"
        || n.contains("LOGON")
        || n.contains("LOG ON")
        || n.contains("LOGGED ON")
        || n.contains("CONNECTION")
        || n.contains("FLIGHT PLAN NOT HELD")
}

/// Read a logon verdict from `text`. `replies_to_our_logon` is true when
/// the uplink's MRN equals our outstanding `REQUEST LOGON`'s MIN.
///
/// Returns `None` when the text settles nothing — the logon then stays
/// pending, exactly as before.
pub fn classify(text: &str, replies_to_our_logon: bool) -> Option<LogonReply> {
    let n = normalize(text);
    if n.is_empty() {
        return None;
    }
    if !replies_to_our_logon && !is_about_the_logon(&n) {
        return None;
    }
    if says_refused(&n) {
        Some(LogonReply::Refused)
    } else if says_accepted(&n) {
        Some(LogonReply::Accepted)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Option<LogonReply> = Some(LogonReply::Accepted);
    const R: Option<LogonReply> = Some(LogonReply::Refused);

    #[test]
    fn every_acceptance_wording_pmdg_knows_counts_with_an_mrn() {
        for text in [
            "LOGON ACCEPTED",
            "LOGON SUCCESSFUL",
            "CONNECTION ACCEPTED",
            "LOGGED ON",
            "ACCEPTED",
            "LOGON ACCEPTED EDGG",
            "logon accepted",
        ] {
            assert_eq!(classify(text, true), A, "{text:?}");
        }
    }

    #[test]
    fn logon_specific_acceptance_counts_without_an_mrn() {
        for text in [
            "LOGON ACCEPTED",
            "LOGON SUCCESSFUL",
            "CONNECTION ACCEPTED",
            "LOGGED ON",
            "ACCEPTED",
            "LOGON@ACCEPTED",
            "  LOGON   ACCEPTED  ",
            "CPDLC LOGON ACCEPTED BY EDGG",
        ] {
            assert_eq!(classify(text, false), A, "{text:?}");
        }
    }

    #[test]
    fn an_acceptance_of_something_else_needs_an_mrn() {
        // Without an MRN these could answer anything — a PDC, a free
        // text — so they must not settle a logon.
        for text in ["PDC ACCEPTED", "REQUEST ACCEPTED", "CLEARANCE ACCEPTED"] {
            assert_eq!(classify(text, false), None, "{text:?}");
        }
        // With the MRN pointing at our REQUEST LOGON they do (Fenix).
        assert_eq!(classify("REQUEST ACCEPTED", true), A);
    }

    #[test]
    fn refusal_wording_counts_and_beats_acceptance() {
        for text in [
            "FLIGHT PLAN NOT HELD",
            "LOGON REJECTED",
            "CONNECTION REJECTED",
            "LOGON NOT ACCEPTED",
            "LOGON DENIED",
            "LOGON ACCEPTED - REJECTED",
        ] {
            assert_eq!(classify(text, false), R, "{text:?} (no MRN)");
            assert_eq!(classify(text, true), R, "{text:?} (MRN)");
        }
    }

    #[test]
    fn a_bare_refusal_word_without_mrn_is_not_about_the_logon() {
        // "REJECTED" alone could refer to anything the station was asked.
        assert_eq!(classify("REJECTED", false), None);
        assert_eq!(classify("REJECTED", true), R);
    }

    #[test]
    fn negated_logged_on_is_not_an_acceptance() {
        assert_eq!(classify("NOT LOGGED ON", true), None);
        assert_eq!(classify("NOT LOGGED ON", false), None);
    }

    #[test]
    fn unrelated_or_interim_text_settles_nothing() {
        for text in [
            "",
            "   ",
            "STANDBY",
            "UNABLE CALL ON FREQ",
            "CPDLC CONNECTION HAS BEEN TERMINATED",
            "CURRENT ATC UNIT EDGG",
            "CLIMB TO FL350",
        ] {
            assert_eq!(classify(text, false), None, "{text:?} (no MRN)");
            assert_eq!(classify(text, true), None, "{text:?} (MRN)");
        }
    }
}
