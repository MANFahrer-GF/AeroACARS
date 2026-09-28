//! PDC (Pre-Departure Clearance) request/reply formatting.
//!
//! There is no dedicated Hoppie wire type for PDC — it is sent as a
//! plain `telex`. Format verified against the source of
//! `quassbutreally/EasyCPDLC` (GPL-3.0, `RequestForm.cs:588`), an
//! established, actively-used Hoppie-network client:
//!
//! ```text
//! REQUEST PREDEP CLEARANCE {CALLSIGN} {ACTYPE} TO {DEST} AT {DEP} STAND {STAND} ATIS {ATIS}
//! ```
//!
//! Note the field order: `TO` is the *destination*, `AT` is the
//! *departure* airport — easy to transpose by mistake.

#[derive(Debug, Clone, PartialEq)]
pub struct PdcRequest {
    /// Station the request is addressed to (`to=` on the wire) — e.g.
    /// the departure airport ICAO or a delivery-desk callsign.
    pub recipient: String,
    pub callsign: String,
    pub aircraft_type: String,
    pub dep_icao: String,
    pub dest_icao: String,
    pub stand: String,
    pub atis_letter: String,
    /// v1.9.5 (#hoppie-pdc-freetext): optional remark appended after the
    /// ATIS letter — "REQ PUSH", "READY IN 5 MIN", a de-icing note. Fenix,
    /// iniBuilds and the A220 plugin all offer it; vSMR shows the whole
    /// request text to the controller. Empty = nothing appended. Pass it
    /// through [`sanitize_free_text`] first.
    pub free_text: String,
}

/// Longest remark accepted — two CDU scratchpad lines, as Fenix offers.
pub const FREE_TEXT_MAX_CHARS: usize = 48;

/// Make a pilot's remark safe to put on the wire: uppercase (Hoppie's
/// convention; vSMR matches case-sensitively), no `{`/`}` (they delimit
/// messages in the receiver's poll response — one stray brace would cut
/// our request short at the controller), control characters as spaces, single
/// spaces, at most [`FREE_TEXT_MAX_CHARS`].
pub fn sanitize_free_text(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '{' | '}'))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .to_uppercase();
    let joined = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    joined.chars().take(FREE_TEXT_MAX_CHARS).collect::<String>().trim_end().to_string()
}

/// Build the telex body per the verified EasyCPDLC format.
pub fn format_pdc_request(req: &PdcRequest) -> String {
    let base = format!(
        "REQUEST PREDEP CLEARANCE {} {} TO {} AT {} STAND {} ATIS {}",
        req.callsign, req.aircraft_type, req.dest_icao, req.dep_icao, req.stand, req.atis_letter
    );
    let remark = req.free_text.trim();
    if remark.is_empty() {
        base
    } else {
        format!("{base} {remark}")
    }
}

/// A PDC reply is free-form human/network text — real clearances vary
/// by controller/tool, so there is no reliable fixed structure to
/// parse. This simply wraps the raw text; extracting SID/squawk/
/// frequency is explicitly out of scope (see the project plan doc).
#[derive(Debug, Clone, PartialEq)]
pub struct PdcReply {
    pub raw_text: String,
}

pub fn parse_pdc_reply(text: &str) -> PdcReply {
    PdcReply {
        raw_text: text.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_pdc_request_matches_verified_easycpdlc_format() {
        let req = PdcRequest {
            recipient: "EDDF".to_string(),
            callsign: "GSG123".to_string(),
            aircraft_type: "A320".to_string(),
            dep_icao: "EDDF".to_string(),
            dest_icao: "EDDM".to_string(),
            stand: "A12".to_string(),
            atis_letter: "K".to_string(),
            free_text: String::new(),
        };
        assert_eq!(
            format_pdc_request(&req),
            "REQUEST PREDEP CLEARANCE GSG123 A320 TO EDDM AT EDDF STAND A12 ATIS K"
        );
    }

    #[test]
    fn a_remark_follows_the_atis_letter_after_one_space() {
        let req = PdcRequest {
            recipient: "EDDF".to_string(),
            callsign: "GSG123".to_string(),
            aircraft_type: "A320".to_string(),
            dep_icao: "EDDF".to_string(),
            dest_icao: "EDDM".to_string(),
            stand: "A12".to_string(),
            atis_letter: "K".to_string(),
            free_text: sanitize_free_text("  ready in 5 min "),
        };
        assert_eq!(
            format_pdc_request(&req),
            "REQUEST PREDEP CLEARANCE GSG123 A320 TO EDDM AT EDDF STAND A12 ATIS K READY IN 5 MIN"
        );
    }

    #[test]
    fn sanitize_strips_braces_and_control_characters_and_uppercases() {
        assert_eq!(sanitize_free_text("de-ice {pad} 2\nplease"), "DE-ICE PAD 2 PLEASE");
        assert_eq!(sanitize_free_text("a}b{c"), "ABC");
        assert_eq!(sanitize_free_text("  two   spaces\t tab "), "TWO SPACES TAB");
        assert_eq!(sanitize_free_text("   "), "");
    }

    #[test]
    fn sanitize_caps_the_length_without_a_trailing_space() {
        let long = "X".repeat(FREE_TEXT_MAX_CHARS + 20);
        assert_eq!(sanitize_free_text(&long).chars().count(), FREE_TEXT_MAX_CHARS);
        // Cut lands right after a space: no dangling blank at the end.
        let mut spaced = "A".repeat(FREE_TEXT_MAX_CHARS - 1);
        spaced.push_str(" BBBB");
        assert_eq!(sanitize_free_text(&spaced), "A".repeat(FREE_TEXT_MAX_CHARS - 1));
    }

    #[test]
    fn parse_pdc_reply_trims_whitespace() {
        let reply = parse_pdc_reply("  GSG123 CLRD TO EDDM VIA ANEKI2F  \n");
        assert_eq!(reply.raw_text, "GSG123 CLRD TO EDDM VIA ANEKI2F");
    }
}
