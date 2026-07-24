//! Data-driven GOLD ("ATS Data Link Services in NAT Airspace" / ICAO
//! GOLD) uplink (UM) / downlink (DM) message-element table, plus a
//! generic template fill/match engine.
//!
//! Phase 1 ([`crate::elements_data`]) ships a small placeholder table
//! (~10 rows) just wide enough to exercise the wire codec and MIN/MRN
//! threading end-to-end. The full ~300-row GOLD library lands in
//! Phase 4 as its own dedicated, reviewable diff (see the project plan
//! doc). The engine below is already the FINAL, generic shape: adding
//! row #301 later is a data-only change to `elements_data.rs`, never a
//! code change here — every `ElementSpec` is `'static` data, there are
//! no per-element match arms.
//!
//! Table structure modeled on `skiselkov/libcpdlc`'s `cpdlc_msg_infos[]`
//! (MIT, C) — the DATA is transcribed from there in Phase 4, not the
//! code.
//!
//! ## A note on `@`
//!
//! The official Hoppie docs (`hoppie.nl/acars/system/tech.html`) state
//! that `@` characters inside CPDLC text are "line feeds for
//! presentation purposes" and "do not really mean anything" on the
//! wire. `ElementSpec::template` nonetheless uses `@1`, `@2`, ... as
//! OUR OWN internal placeholder syntax — those tokens are fully
//! substituted by [`resolve`] before anything reaches the wire, so they
//! never collide with a real `@` sent by another station. [`match_text`]
//! strips a leading `@` off a captured placeholder value defensively,
//! since some real-world uplinks (see the worked example in the
//! community docs, `PROCEED DIRECT TO @UDROS`) do include one.

use crate::cpdlc::ResponseRequirement;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Uplink,
    Downlink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    Waypoint,
    Altitude,
    Speed,
    Heading,
    FreeText,
    Time,
    Position,
    Route,
}

/// One row of the GOLD element table. `'static` data — see the module
/// docs on why this is never expressed as match arms.
#[derive(Debug, Clone, Copy)]
pub struct ElementSpec {
    /// GOLD element id (e.g. `"UM74"`/`"DM32"` once Phase 4 lands the
    /// real table). Phase 1's placeholder rows use descriptive ids
    /// (`"DM_WILCO"`, ...) since the real GOLD numbers aren't
    /// transcribed yet — deliberately NOT guessed here to avoid
    /// asserting false authority on the numbering.
    pub id: &'static str,
    pub direction: Direction,
    /// Template text using `@1`, `@2`, ... as positional placeholder
    /// markers (see the module docs).
    pub template: &'static str,
    pub placeholders: &'static [PlaceholderKind],
    pub response: ResponseRequirement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedElement {
    pub spec_id: &'static str,
    pub filled_text: String,
    pub values: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ParsedElement {
    Recognized(ResolvedElement),
    /// Unrecognized text — always the fallback, never an error. Real
    /// traffic is never guaranteed to be textbook-perfect.
    Raw(String),
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ElementError {
    #[error("element {0:?} not found in table")]
    UnknownId(String),
    #[error("expected {expected} value(s) for {id:?}, got {got}")]
    ArityMismatch {
        id: String,
        expected: usize,
        got: usize,
    },
}

/// Look up an [`ElementSpec`] by id across both tables.
pub fn find(id: &str) -> Option<&'static ElementSpec> {
    crate::elements_data::UM_TABLE
        .iter()
        .chain(crate::elements_data::DM_TABLE.iter())
        .find(|e| e.id == id)
}

/// Fill a template's `@N` placeholders with concrete values, in order.
/// Templates with zero placeholders (e.g. `"WILCO"`) require an empty
/// `values` slice.
pub fn resolve(
    spec: &ElementSpec,
    values: &[String],
) -> Result<ResolvedElement, ElementError> {
    if values.len() != spec.placeholders.len() {
        return Err(ElementError::ArityMismatch {
            id: spec.id.to_string(),
            expected: spec.placeholders.len(),
            got: values.len(),
        });
    }
    let mut filled = spec.template.to_string();
    for (i, v) in values.iter().enumerate() {
        filled = filled.replace(&format!("@{}", i + 1), v);
    }
    Ok(ResolvedElement {
        spec_id: spec.id,
        filled_text: filled,
        values: values.to_vec(),
    })
}

/// Best-effort match of raw uplink wire text against the UM table.
/// Received packets are always uplink relative to us — Hoppie's `poll`
/// only returns messages addressed TO our callsign, never an echo of
/// our own outgoing downlinks — so decode-time matching always targets
/// the direction-specific table, resolving the ambiguity where the same
/// literal text (e.g. `"UNABLE"`) exists as both a downlink WU-response
/// element and an uplink logon-rejection element.
pub fn match_uplink_text(text: &str) -> ParsedElement {
    match_in_table(text, crate::elements_data::UM_TABLE)
}

/// Best-effort match of raw downlink wire text against the DM table
/// (used for round-trip tests / re-parsing our own sent history, not
/// for interpreting received packets — see [`match_uplink_text`]).
pub fn match_downlink_text(text: &str) -> ParsedElement {
    match_in_table(text, crate::elements_data::DM_TABLE)
}

fn match_in_table(text: &str, table: &[ElementSpec]) -> ParsedElement {
    let trimmed = text.trim();
    for spec in table {
        if let Some(resolved) = try_match_template(spec, trimmed) {
            return ParsedElement::Recognized(resolved);
        }
    }
    ParsedElement::Raw(trimmed.to_string())
}

enum TemplateSegment {
    Literal(String),
    Placeholder,
}

/// Split a template into literal/placeholder segments on `@N` markers.
fn split_template(template: &str) -> Vec<TemplateSegment> {
    let mut segments = Vec::new();
    let mut buf = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '@' && chars.peek().is_some_and(char::is_ascii_digit) {
            if !buf.is_empty() {
                segments.push(TemplateSegment::Literal(std::mem::take(&mut buf)));
            }
            while chars.peek().is_some_and(char::is_ascii_digit) {
                chars.next();
            }
            segments.push(TemplateSegment::Placeholder);
            continue;
        }
        buf.push(c);
    }
    if !buf.is_empty() {
        segments.push(TemplateSegment::Literal(buf));
    }
    segments
}

/// Try to match `text` against `spec.template`, extracting placeholder
/// values positionally. Literal segments must appear in order;
/// placeholder segments greedily consume up to the next literal segment
/// (or end of string for a trailing placeholder).
fn try_match_template(spec: &ElementSpec, text: &str) -> Option<ResolvedElement> {
    if spec.placeholders.is_empty() {
        return (spec.template == text).then(|| ResolvedElement {
            spec_id: spec.id,
            filled_text: text.to_string(),
            values: Vec::new(),
        });
    }
    let segments = split_template(spec.template);
    let mut rest = text;
    let mut values = Vec::with_capacity(spec.placeholders.len());
    for (i, seg) in segments.iter().enumerate() {
        match seg {
            TemplateSegment::Literal(lit) => {
                rest = rest.strip_prefix(lit.as_str())?;
            }
            TemplateSegment::Placeholder => {
                let next_literal = segments.get(i + 1).and_then(|s| match s {
                    TemplateSegment::Literal(l) => Some(l.as_str()),
                    TemplateSegment::Placeholder => None,
                });
                let (raw_value, remainder) = match next_literal {
                    Some(lit) if !lit.is_empty() => {
                        let idx = rest.find(lit)?;
                        (&rest[..idx], &rest[idx..])
                    }
                    _ => (rest, ""),
                };
                // Strip a defensive leading '@' — see the module docs.
                let value = raw_value.trim().trim_start_matches('@').trim().to_string();
                if value.is_empty() {
                    return None;
                }
                values.push(value);
                rest = remainder;
            }
        }
    }
    if !rest.is_empty() {
        return None;
    }
    Some(ResolvedElement {
        spec_id: spec.id,
        filled_text: text.to_string(),
        values,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_fills_single_placeholder() {
        let spec = find("UM_PROCEED_DIRECT_TO").expect("placeholder table has this element");
        let resolved = resolve(spec, &["UDROS".to_string()]).unwrap();
        assert_eq!(resolved.filled_text, "PROCEED DIRECT TO UDROS");
    }

    #[test]
    fn resolve_rejects_arity_mismatch() {
        let spec = find("UM_PROCEED_DIRECT_TO").unwrap();
        assert!(matches!(
            resolve(spec, &[]),
            Err(ElementError::ArityMismatch { .. })
        ));
        assert!(matches!(
            resolve(spec, &["A".into(), "B".into()]),
            Err(ElementError::ArityMismatch { .. })
        ));
    }

    #[test]
    fn resolve_zero_placeholder_element_ignores_empty_values() {
        let spec = find("DM_WILCO").unwrap();
        let resolved = resolve(spec, &[]).unwrap();
        assert_eq!(resolved.filled_text, "WILCO");
    }

    #[test]
    fn match_uplink_text_recognizes_logon_accepted() {
        match elements_match_uplink("LOGON ACCEPTED") {
            ParsedElement::Recognized(r) => assert_eq!(r.spec_id, "UM_LOGON_ACCEPTED"),
            other => panic!("expected Recognized, got {other:?}"),
        }
    }

    #[test]
    fn match_uplink_text_recognizes_proceed_direct_to_with_placeholder() {
        match elements_match_uplink("PROCEED DIRECT TO UDROS") {
            ParsedElement::Recognized(r) => {
                assert_eq!(r.spec_id, "UM_PROCEED_DIRECT_TO");
                assert_eq!(r.values, vec!["UDROS".to_string()]);
            }
            other => panic!("expected Recognized, got {other:?}"),
        }
    }

    #[test]
    fn match_uplink_text_strips_defensive_leading_at_sign() {
        // Real-world worked example from the community docs.
        match elements_match_uplink("PROCEED DIRECT TO @UDROS") {
            ParsedElement::Recognized(r) => {
                assert_eq!(r.values, vec!["UDROS".to_string()]);
            }
            other => panic!("expected Recognized, got {other:?}"),
        }
    }

    #[test]
    fn match_uplink_text_falls_back_to_raw_for_unrecognized_text() {
        match elements_match_uplink("SOME UNKNOWN INSTRUCTION 42") {
            ParsedElement::Raw(text) => assert_eq!(text, "SOME UNKNOWN INSTRUCTION 42"),
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn match_uplink_text_never_matches_downlink_only_element() {
        // "WILCO" only exists in the DM table — matching it against the
        // UM table (as an inbound uplink would be) must fall back to Raw,
        // not spuriously match a downlink-only element.
        match elements_match_uplink("WILCO") {
            ParsedElement::Raw(_) => {}
            other => panic!("expected Raw (WILCO is downlink-only), got {other:?}"),
        }
    }

    #[test]
    fn direction_specific_matching_resolves_the_unable_ambiguity() {
        // "UNABLE" exists as BOTH an uplink logon-rejection element and a
        // downlink WU-response element. Direction-specific matching must
        // resolve each to the correct one.
        match elements_match_uplink("UNABLE") {
            ParsedElement::Recognized(r) => assert_eq!(r.spec_id, "UM_LOGON_UNABLE"),
            other => panic!("expected Recognized(UM_LOGON_UNABLE), got {other:?}"),
        }
        match match_downlink_text("UNABLE") {
            ParsedElement::Recognized(r) => assert_eq!(r.spec_id, "DM_UNABLE"),
            other => panic!("expected Recognized(DM_UNABLE), got {other:?}"),
        }
    }

    #[test]
    fn find_returns_none_for_unknown_id() {
        assert!(find("DOES_NOT_EXIST").is_none());
    }

    /// Corpus-sweep: every row in both tables round-trips through
    /// resolve() -> filled text -> match (in the SAME table it came
    /// from) -> back to the same spec_id. Cheap insurance that the
    /// table stays internally consistent as it grows toward Phase 4's
    /// ~300 rows.
    #[test]
    fn every_table_row_round_trips_resolve_then_match() {
        for spec in crate::elements_data::UM_TABLE
            .iter()
            .chain(crate::elements_data::DM_TABLE.iter())
        {
            let placeholder_values: Vec<String> = spec
                .placeholders
                .iter()
                .enumerate()
                .map(|(i, _)| format!("TESTVAL{i}"))
                .collect();
            let resolved = resolve(spec, &placeholder_values)
                .unwrap_or_else(|e| panic!("resolve({}) failed: {e}", spec.id));
            let matched = match spec.direction {
                Direction::Uplink => match_uplink_text(&resolved.filled_text),
                Direction::Downlink => match_downlink_text(&resolved.filled_text),
            };
            match matched {
                ParsedElement::Recognized(r) => assert_eq!(
                    r.spec_id, spec.id,
                    "round-trip of {} matched a different element",
                    spec.id
                ),
                ParsedElement::Raw(text) => {
                    panic!("round-trip of {} fell back to Raw({text:?})", spec.id)
                }
            }
        }
    }

    fn elements_match_uplink(text: &str) -> ParsedElement {
        match_uplink_text(text)
    }
}
