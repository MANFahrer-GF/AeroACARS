//! `/data2/{MIN}/{MRN}/{RESPONSE}/{ELEMENT}` codec for Hoppie CPDLC
//! packets.
//!
//! Format + response-requirement key verified against
//! `devHazz/hoppie-acars-docs`'s `Messaging/CPDLC Format.md` (ATS
//! Datalink NAT-Airspace GOLD terminology). Element *interpretation*
//! (matching free text back to a known GOLD element, or filling a
//! template) is [`crate::elements`]'s job — this module only
//! frames/unframes the MIN/MRN/response envelope.

use crate::elements::{self, Direction, ParsedElement};

/// Response-requirement code, per the GOLD "Response Requirements Key"
/// the community docs reference (WU/AN/R/Y/N/NE).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ResponseRequirement {
    /// `WU` — Wilco/Unable response required.
    WilcoUnable,
    /// `AN` — Affirm/Negative response required.
    AffirmNegative,
    /// `R` — Roger response required.
    Roger,
    /// `Y` — any response required (free text).
    AnyRequired,
    /// `N` — no response required.
    NotRequired,
    /// `NE` — no response *expected*, but an operational ack is
    /// implied (used for `LOGON ACCEPTED`/`UNABLE`).
    NoResponseExpected,
}

impl ResponseRequirement {
    pub fn code(self) -> &'static str {
        match self {
            Self::WilcoUnable => "WU",
            Self::AffirmNegative => "AN",
            Self::Roger => "R",
            Self::AnyRequired => "Y",
            Self::NotRequired => "N",
            Self::NoResponseExpected => "NE",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        match code {
            "WU" => Some(Self::WilcoUnable),
            "AN" => Some(Self::AffirmNegative),
            "R" => Some(Self::Roger),
            "Y" => Some(Self::AnyRequired),
            "N" => Some(Self::NotRequired),
            "NE" => Some(Self::NoResponseExpected),
            _ => None,
        }
    }

    /// Whether a message carrying this code obliges the recipient to
    /// send some reply. `NotRequired` (N) and `NoResponseExpected` (NE)
    /// are two DIFFERENT codes that both mean "nobody owes a reply" —
    /// conflating them with only the latter is a real bug: it would
    /// leave e.g. our own `WILCO` (N) permanently "pending" in
    /// [`crate::thread::CpdlcThread`], since nothing ever closes it.
    pub fn requires_reply(self) -> bool {
        matches!(
            self,
            Self::WilcoUnable | Self::AffirmNegative | Self::Roger | Self::AnyRequired
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CpdlcMessage {
    pub min: u32,
    pub mrn: Option<u32>,
    pub response: ResponseRequirement,
    pub element_text: String,
    pub parsed: ParsedElement,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CpdlcError {
    #[error("packet missing /data2/ prefix: {0:?}")]
    MissingPrefix(String),
    #[error("invalid MIN field: {0:?}")]
    InvalidMin(String),
    #[error("invalid MRN field: {0:?}")]
    InvalidMrn(String),
    #[error("unknown response-requirement code: {0:?}")]
    UnknownResponseCode(String),
}

/// Encode a [`CpdlcMessage`] to the wire packet string (the value of
/// the `packet` request parameter for `type=cpdlc`).
pub fn encode(msg: &CpdlcMessage) -> String {
    let mrn = msg.mrn.map(|m| m.to_string()).unwrap_or_default();
    format!(
        "/data2/{}/{}/{}/{}",
        msg.min,
        mrn,
        msg.response.code(),
        msg.element_text
    )
}

/// Decode a raw CPDLC packet. `direction` tells the decoder which
/// element table to match the element text against — see
/// [`elements::match_uplink_text`]'s docs for why direction must be
/// supplied explicitly rather than guessed from the packet itself.
pub fn decode(packet: &str, direction: Direction) -> Result<CpdlcMessage, CpdlcError> {
    let rest = packet
        .strip_prefix("/data2/")
        .ok_or_else(|| CpdlcError::MissingPrefix(packet.to_string()))?;
    // MIN/MRN/RESPONSE/ELEMENT — the element text itself may contain
    // '/', so split into at most 4 parts on the first three slashes.
    let mut parts = rest.splitn(4, '/');
    let min_s = parts.next().unwrap_or_default();
    let mrn_s = parts.next().unwrap_or_default();
    let resp_s = parts.next().unwrap_or_default();
    let element_text = parts.next().unwrap_or_default().to_string();

    let min: u32 = min_s
        .parse()
        .map_err(|_| CpdlcError::InvalidMin(min_s.to_string()))?;
    let mrn = if mrn_s.is_empty() || mrn_s == "-" {
        None
    } else {
        Some(
            mrn_s
                .parse()
                .map_err(|_| CpdlcError::InvalidMrn(mrn_s.to_string()))?,
        )
    };
    let response = ResponseRequirement::from_code(resp_s)
        .ok_or_else(|| CpdlcError::UnknownResponseCode(resp_s.to_string()))?;
    let parsed = match direction {
        Direction::Uplink => elements::match_uplink_text(&element_text),
        Direction::Downlink => elements::match_downlink_text(&element_text),
    };

    Ok(CpdlcMessage {
        min,
        mrn,
        response,
        element_text,
        parsed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_worked_example_from_the_docs() {
        // "LRBL -> SWR160, /data2/3//WU/PROCEED DIRECT TO @UDROS, MIN=3"
        // from devHazz/hoppie-acars-docs' CPDLC Format.md.
        let msg = decode("/data2/3//WU/PROCEED DIRECT TO @UDROS", Direction::Uplink).unwrap();
        assert_eq!(msg.min, 3);
        assert_eq!(msg.mrn, None);
        assert_eq!(msg.response, ResponseRequirement::WilcoUnable);
        match msg.parsed {
            ParsedElement::Recognized(r) => {
                assert_eq!(r.spec_id, "UM_PROCEED_DIRECT_TO");
                assert_eq!(r.values, vec!["UDROS".to_string()]);
            }
            other => panic!("expected Recognized, got {other:?}"),
        }
    }

    #[test]
    fn decode_worked_reply_from_the_docs() {
        // "SWR160 -> LRBL, /data2/8/3/N/WILCO, MIN=8 MRN=3".
        let msg = decode("/data2/8/3/N/WILCO", Direction::Downlink).unwrap();
        assert_eq!(msg.min, 8);
        assert_eq!(msg.mrn, Some(3));
        assert_eq!(msg.response, ResponseRequirement::NotRequired);
    }

    #[test]
    fn encode_round_trips_with_empty_mrn() {
        let msg = CpdlcMessage {
            min: 3,
            mrn: None,
            response: ResponseRequirement::WilcoUnable,
            element_text: "PROCEED DIRECT TO UDROS".to_string(),
            parsed: ParsedElement::Raw(String::new()),
        };
        assert_eq!(encode(&msg), "/data2/3//WU/PROCEED DIRECT TO UDROS");
    }

    #[test]
    fn encode_round_trips_with_mrn() {
        let msg = CpdlcMessage {
            min: 8,
            mrn: Some(3),
            response: ResponseRequirement::NotRequired,
            element_text: "WILCO".to_string(),
            parsed: ParsedElement::Raw(String::new()),
        };
        assert_eq!(encode(&msg), "/data2/8/3/N/WILCO");
    }

    #[test]
    fn decode_rejects_missing_prefix() {
        assert!(matches!(
            decode("not-a-cpdlc-packet", Direction::Uplink),
            Err(CpdlcError::MissingPrefix(_))
        ));
    }

    #[test]
    fn decode_rejects_invalid_min() {
        assert!(matches!(
            decode("/data2/notanumber//WU/TEXT", Direction::Uplink),
            Err(CpdlcError::InvalidMin(_))
        ));
    }

    #[test]
    fn decode_rejects_unknown_response_code() {
        assert!(matches!(
            decode("/data2/1//ZZ/TEXT", Direction::Uplink),
            Err(CpdlcError::UnknownResponseCode(_))
        ));
    }

    #[test]
    fn decode_accepts_dash_as_empty_mrn_defensively() {
        // Not the docs' own convention (which uses a bare empty field),
        // but some clients may use "-"; accept both.
        let msg = decode("/data2/1/-/N/TEXT", Direction::Uplink).unwrap();
        assert_eq!(msg.mrn, None);
    }

    #[test]
    fn response_requirement_code_round_trips() {
        for r in [
            ResponseRequirement::WilcoUnable,
            ResponseRequirement::AffirmNegative,
            ResponseRequirement::Roger,
            ResponseRequirement::AnyRequired,
            ResponseRequirement::NotRequired,
            ResponseRequirement::NoResponseExpected,
        ] {
            assert_eq!(ResponseRequirement::from_code(r.code()), Some(r));
        }
    }
}
