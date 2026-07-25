//! Hoppie ACARS wire protocol: `connect.html` GET request/response
//! codec.
//!
//! Reference: the OFFICIAL protocol description at
//! `https://www.hoppie.nl/acars/system/tech.html` — cross-checked
//! against the community docs at `devHazz/hoppie-acars-docs` and
//! `islandcontroller/hoppie-connector`'s `Responses.py` parser (MIT).
//! Pure string handling — no `reqwest`/HTTP types touch this crate; the
//! actual GET is issued by `client/src-tauri/src/hoppie/mod.rs` using a
//! shared client, and the response body text is handed to
//! [`parse_response`].

/// Hoppie's `type=` values actually used by AeroACARS. `progress`,
/// `position`, `datareq` and `inforeq` exist in the protocol but are
/// out of scope for the PDC/CPDLC feature — omitted rather than
/// modeled unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PacketKind {
    Telex,
    Cpdlc,
    Ping,
    Poll,
    Peek,
}

impl PacketKind {
    pub fn as_wire_str(self) -> &'static str {
        match self {
            PacketKind::Telex => "telex",
            PacketKind::Cpdlc => "cpdlc",
            PacketKind::Ping => "ping",
            PacketKind::Poll => "poll",
            PacketKind::Peek => "peek",
        }
    }

    /// Parse a lowercase wire type string back to a [`PacketKind`].
    /// Used when unpacking poll/peek envelopes, which each carry their
    /// own `type` field. Unrecognized/out-of-scope types (e.g.
    /// `progress`) return `None` — callers skip such envelopes rather
    /// than erroring the whole poll.
    pub fn from_wire_str(s: &str) -> Option<Self> {
        match s.trim() {
            "telex" => Some(PacketKind::Telex),
            "cpdlc" => Some(PacketKind::Cpdlc),
            "ping" => Some(PacketKind::Ping),
            "poll" => Some(PacketKind::Poll),
            "peek" => Some(PacketKind::Peek),
            _ => None,
        }
    }
}

/// One outbound request to `connect.html`.
#[derive(Debug, Clone)]
pub struct HoppieRequest {
    pub logon: String,
    pub from: String,
    pub to: String,
    pub kind: PacketKind,
    pub packet: Option<String>,
}

/// Build the `(key, value)` query pairs for [`HoppieRequest`]. Returned
/// as owned pairs rather than a raw URL string so the caller (which DOES
/// depend on `reqwest`) can hand them to `.query(&pairs)` and let the
/// `url` crate do correct percent-encoding — this crate deliberately
/// does not hand-rolled URL-encode anything itself.
pub fn query_pairs(req: &HoppieRequest) -> Vec<(&'static str, String)> {
    let mut pairs = vec![
        ("logon", req.logon.clone()),
        ("from", req.from.clone()),
        ("to", req.to.clone()),
        ("type", req.kind.as_wire_str().to_string()),
    ];
    if let Some(packet) = &req.packet {
        pairs.push(("packet", packet.clone()));
    }
    pairs
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WireError {
    #[error("unrecognized Hoppie response body: {0:?}")]
    InvalidResponse(String),
}

/// Parsed top-level response line. Grammar verified against the
/// official docs and `hoppie-connector`'s `HoppieResponseParser`:
/// `^(ok|error)\s?(.*)$`, with the error reason wrapped in `{...}`.
#[derive(Debug, Clone, PartialEq)]
pub enum HoppieResponseLine {
    /// Plain `ok` — request accepted, no payload (a telex/cpdlc send,
    /// an empty ping, etc).
    Ok,
    /// `ok <content>` — poll/peek/ping success carrying a payload. The
    /// raw content is handed back unparsed; use [`parse_poll_envelopes`]
    /// for poll/peek bodies or [`parse_ping_stations`] for ping bodies
    /// — the two grammars differ and can't be told apart from the body
    /// alone, so the caller (which knows which request it sent) picks.
    OkWithPayload(String),
    /// `error {reason}` — request rejected. `reason` is the server's
    /// raw text, carried through unmodified: the exact wording for
    /// e.g. an invalid logon code is not documented, so this never
    /// guesses or rewrites it.
    Error(String),
}

/// Parse a raw HTTP response body into a [`HoppieResponseLine`].
pub fn parse_response(body: &str) -> Result<HoppieResponseLine, WireError> {
    let trimmed = body.trim_start();
    if let Some(rest) = trimmed.strip_prefix("ok") {
        let rest = rest.trim();
        if rest.is_empty() {
            Ok(HoppieResponseLine::Ok)
        } else {
            Ok(HoppieResponseLine::OkWithPayload(rest.to_string()))
        }
    } else if let Some(rest) = trimmed.strip_prefix("error") {
        let rest = rest.trim();
        let reason = rest
            .strip_prefix('{')
            .and_then(|r| r.strip_suffix('}'))
            .unwrap_or(rest);
        Ok(HoppieResponseLine::Error(reason.to_string()))
    } else {
        Err(WireError::InvalidResponse(body.to_string()))
    }
}

/// One message envelope out of a poll/peek success payload.
#[derive(Debug, Clone, PartialEq)]
pub struct InboundEnvelope {
    pub from: String,
    pub kind: PacketKind,
    pub packet: String,
}

/// Parse the content of an `ok {...}{...}` poll/peek response into
/// individual envelopes. Envelope shape (per the official docs and
/// `hoppie-connector`'s `PollResponseParser`): `{FROM type {packet}}`
/// — hand-written brace-aware scan rather than a `regex` dependency, to
/// keep this crate's dependency footprint at `serde`/`thiserror` only.
/// Malformed/unrecognized envelopes are skipped rather than aborting
/// the whole poll (Hoppie is a dumb relay; one bad envelope must never
/// hide the rest).
pub fn parse_poll_envelopes(content: &str) -> Vec<InboundEnvelope> {
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find('{') {
        rest = &rest[start + 1..];
        let Some(sp) = rest.find(char::is_whitespace) else {
            break;
        };
        let from = rest[..sp].to_string();
        rest = rest[sp..].trim_start();
        let Some(brace2) = rest.find('{') else {
            break;
        };
        let type_str = rest[..brace2].trim();
        let kind = PacketKind::from_wire_str(type_str);
        rest = &rest[brace2 + 1..];
        // The envelope is `{FROM type {packet}}`, so the packet ends at
        // the `}}` pair — NOT at the first `}`. Stopping at the first one
        // silently truncated any message whose text contains a brace,
        // and a controller's free text is arbitrary: everything after it
        // vanished without a trace. Falling back to the last `}` keeps a
        // malformed final envelope readable instead of dropping it.
        let (packet, consumed) = match rest.find("}}") {
            Some(i) => (rest[..i].to_string(), i + 2),
            None => match rest.rfind('}') {
                Some(i) => (rest[..i].to_string(), i + 1),
                None => break,
            },
        };
        rest = &rest[consumed..];
        if let Some(kind) = kind {
            out.push(InboundEnvelope { from, kind, packet });
        }
    }
    out
}

/// Parse a `ping` success payload — a space-separated list of station
/// callsigns (empty when the ping carried no `ALL-CALLSIGNS`/callsign
/// payload, which is the normal "just testing the link" case).
pub fn parse_ping_stations(content: &str) -> Vec<String> {
    content.split_whitespace().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_pairs_include_packet_only_when_present() {
        let req = HoppieRequest {
            logon: "abc123".into(),
            from: "GSG123".into(),
            to: "SERVER".into(),
            kind: PacketKind::Ping,
            packet: None,
        };
        let pairs = query_pairs(&req);
        assert!(!pairs.iter().any(|(k, _)| *k == "packet"));

        let req_with_packet = HoppieRequest {
            packet: Some("hello".into()),
            ..req
        };
        let pairs = query_pairs(&req_with_packet);
        assert_eq!(
            pairs
                .iter()
                .find(|(k, _)| *k == "packet")
                .map(|(_, v)| v.as_str()),
            Some("hello")
        );
    }

    #[test]
    fn parse_response_bare_ok() {
        assert_eq!(parse_response("ok").unwrap(), HoppieResponseLine::Ok);
        assert_eq!(parse_response("ok\n").unwrap(), HoppieResponseLine::Ok);
    }

    #[test]
    fn parse_response_error_extracts_reason() {
        assert_eq!(
            parse_response("error {invalid logon code}").unwrap(),
            HoppieResponseLine::Error("invalid logon code".to_string())
        );
    }

    #[test]
    fn parse_response_error_without_braces_falls_back_to_raw_reason() {
        // Defensive: if the server ever omits the {} wrapper, don't lose
        // the reason text.
        assert_eq!(
            parse_response("error something went wrong").unwrap(),
            HoppieResponseLine::Error("something went wrong".to_string())
        );
    }

    #[test]
    fn parse_response_ok_with_payload() {
        assert_eq!(
            parse_response("ok {GSG123 telex {hello}}").unwrap(),
            HoppieResponseLine::OkWithPayload("{GSG123 telex {hello}}".to_string())
        );
    }

    #[test]
    fn parse_response_rejects_garbage() {
        assert!(parse_response("garbage").is_err());
    }

    #[test]
    fn parse_poll_envelopes_single() {
        let envs = parse_poll_envelopes("{EDDF cpdlc {/data2/1//NE/LOGON ACCEPTED}}");
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].from, "EDDF");
        assert_eq!(envs[0].kind, PacketKind::Cpdlc);
        assert_eq!(envs[0].packet, "/data2/1//NE/LOGON ACCEPTED");
    }

    #[test]
    fn parse_poll_envelopes_multiple() {
        let envs = parse_poll_envelopes(
            "{EDDF telex {hello}}{EDDF cpdlc {/data2/2//WU/PROCEED DIRECT TO UDROS}}",
        );
        assert_eq!(envs.len(), 2);
        assert_eq!(envs[0].kind, PacketKind::Telex);
        assert_eq!(envs[1].kind, PacketKind::Cpdlc);
    }

    #[test]
    fn parse_poll_envelopes_empty_content_yields_empty_vec() {
        assert!(parse_poll_envelopes("").is_empty());
    }

    #[test]
    fn packet_text_containing_a_brace_survives_intact() {
        // A controller's free text is arbitrary; stopping at the first
        // '}' used to silently drop everything after it.
        let envs = parse_poll_envelopes("{EDGG telex {CALL ME ON 118.5 (NOT 119.1} PLEASE}}");
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].packet, "CALL ME ON 118.5 (NOT 119.1} PLEASE");
    }

    #[test]
    fn a_brace_in_one_envelope_does_not_swallow_the_next() {
        let envs = parse_poll_envelopes("{EDGG telex {ODD } TEXT}}{EDDF telex {SECOND}}");
        assert_eq!(envs.len(), 2, "the following envelope must still parse");
        assert_eq!(envs[0].packet, "ODD } TEXT");
        assert_eq!(envs[1].packet, "SECOND");
    }

    #[test]
    fn parse_poll_envelopes_skips_unrecognized_type() {
        // A "progress" envelope (out of scope) must not abort parsing
        // of the rest of the payload.
        let envs = parse_poll_envelopes("{EDDF progress {OUT 1200}}{EDDF telex {hello}}");
        assert_eq!(envs.len(), 1);
        assert_eq!(envs[0].kind, PacketKind::Telex);
    }

    #[test]
    fn parse_ping_stations_splits_on_whitespace() {
        assert_eq!(
            parse_ping_stations("GSG123 DLH456 SWR789"),
            vec!["GSG123", "DLH456", "SWR789"]
        );
        assert!(parse_ping_stations("").is_empty());
    }
}
