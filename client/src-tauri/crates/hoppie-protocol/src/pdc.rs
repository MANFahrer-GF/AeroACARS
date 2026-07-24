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
}

/// Build the telex body per the verified EasyCPDLC format.
pub fn format_pdc_request(req: &PdcRequest) -> String {
    format!(
        "REQUEST PREDEP CLEARANCE {} {} TO {} AT {} STAND {} ATIS {}",
        req.callsign, req.aircraft_type, req.dest_icao, req.dep_icao, req.stand, req.atis_letter
    )
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
        };
        assert_eq!(
            format_pdc_request(&req),
            "REQUEST PREDEP CLEARANCE GSG123 A320 TO EDDM AT EDDF STAND A12 ATIS K"
        );
    }

    #[test]
    fn parse_pdc_reply_trims_whitespace() {
        let reply = parse_pdc_reply("  GSG123 CLRD TO EDDM VIA ANEKI2F  \n");
        assert_eq!(reply.raw_text, "GSG123 CLRD TO EDDM VIA ANEKI2F");
    }
}
