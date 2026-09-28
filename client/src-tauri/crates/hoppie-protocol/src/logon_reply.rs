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
//! - No MRN (vSMR sets one on nothing): only wording that NAMES the
//!   logon or connection counts ("LOGON ACCEPTED", "CONNECTION
//!   REJECTED"). External QS (Codex, 28.09.2026) P1: a bare `ACCEPTED`
//!   or vSMR's PDC refusal "FSM … RCD REJECTED @FLIGHT PLAN NOT HELD"
//!   (real, EDDM 18.09.) from the same station would otherwise settle a
//!   logon that was never mentioned — Fenix, too, reads "FLIGHT PLAN NOT
//!   HELD" only as the answer to its logon's MIN.
//!
//! Refusal wording is checked FIRST, so `NOT ACCEPTED` or `LOGON
//! ACCEPTED ... REJECTED` never reads as an acceptance.

/// The logon verdict carried by a reply's text, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogonReply {
    Accepted,
    Refused,
}

/// Uppercase, `@`/`_` (Hoppie's line-break and filler conventions) and
/// sentence punctuation as spaces, runs of whitespace collapsed — so a
/// headerless "ACCEPTED." reads like "ACCEPTED" (external review P3).
fn normalize(text: &str) -> String {
    text.to_uppercase()
        .replace(['@', '_', '.', ',', '!', ';', ':'], " ")
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
    n.contains("LOGON") || n.contains("LOG ON") || n.contains("LOGGED ON") || n.contains("CONNECTION")
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
            "LOGON@ACCEPTED",
            "  LOGON   ACCEPTED  ",
            "CPDLC LOGON ACCEPTED BY EDGG",
            "LOGON ACCEPTED.",
            "LOGON ACCEPTED, EDGG",
        ] {
            assert_eq!(classify(text, false), A, "{text:?}");
        }
    }

    #[test]
    fn without_an_mrn_the_text_must_name_the_logon() {
        // External QS (Codex, 28.09.2026) P1 — verbatim vSMR PDC refusal,
        // EDDM 18.09.2026: must not settle a logon to the same station.
        assert_eq!(
            classify("FSM 2024 260918 ---- DLH2AS@DLH2AS@ RCD REJECTED @FLIGHT PLAN NOT HELD @REVERT TO VOICE PROCEDURES", false),
            None
        );
        assert_eq!(classify("FLIGHT PLAN NOT HELD", false), None);
        assert_eq!(classify("ACCEPTED", false), None);
        assert_eq!(classify("ACCEPTED.", false), None);
        // With the MRN on our logon, the same words DO answer it (Fenix).
        assert_eq!(classify("FLIGHT PLAN NOT HELD", true), R);
        assert_eq!(classify("ACCEPTED.", true), A);
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
            "LOGON REJECTED",
            "CONNECTION REJECTED",
            "LOGON NOT ACCEPTED",
            "LOGON DENIED",
            "LOGON ACCEPTED - REJECTED",
            "LOGON REJECTED - FLIGHT PLAN NOT HELD",
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
