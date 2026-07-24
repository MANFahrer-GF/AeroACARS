//! Data-driven GOLD ("ATS Data Link Services in NAT Airspace" / ICAO
//! GOLD) uplink (UM) / downlink (DM) message-element table, plus a
//! generic template fill/match engine.
//!
//! [`crate::elements_data`] holds two kinds of rows: the real GOLD
//! catalog (`GOLD_UM_TABLE`/`GOLD_DM_TABLE`, ~287 rows, DATA
//! transcribed from `skiselkov/libcpdlc`'s `cpdlc_msg_infos[]`, MIT, C
//! — via a one-off extraction script, not hand-typed) plus a handful of
//! Hoppie-specific rows (`HOPPIE_UM_TABLE`/`HOPPIE_DM_TABLE`) for the
//! network's own simplified logon handshake, which has no GOLD
//! equivalent (real ATN/FANS logon is a binary application-layer
//! handshake, not a CPDLC text message — Hoppie invented a plain-text
//! `REQUEST LOGON`/`LOGON ACCEPTED` convention instead). [`um_table`]/
//! [`dm_table`] chain both sources so callers never need to know which
//! table a given id came from. The engine below is fully data-driven —
//! every `ElementSpec` is `'static` data, there are no per-element
//! match arms, so growing either table is a data-only change.
//!
//! ## A note on `@`
//!
//! The official Hoppie docs (`hoppie.nl/acars/system/tech.html`) state
//! that `@` characters inside CPDLC text are "line feeds for
//! presentation purposes" and "do not really mean anything" on the
//! wire. `ElementSpec::template` nonetheless uses `@1`, `@2`, ... as
//! OUR OWN internal placeholder syntax — those tokens are fully
//! substituted by [`resolve`] before anything reaches the wire, so they
//! never collide with a real `@` sent by another station. The matcher
//! strips a leading `@` off a captured placeholder value defensively,
//! since some real-world uplinks (see the worked example in the
//! community docs, `PROCEED DIRECT TO @UDROS`) do include one.

use crate::cpdlc::ResponseRequirement;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Uplink,
    Downlink,
}

/// The full set of GOLD message-element argument types
/// (`skiselkov/libcpdlc`'s `cpdlc_arg_type_t`, 26 variants) plus
/// nothing extra — every placeholder in the real table maps to exactly
/// one of these, so the composer UI (Phase 2/3) can render the right
/// typed input for each without a fallback "unknown" case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceholderKind {
    Altitude,
    Speed,
    Time,
    TimeDuration,
    Position,
    /// Lateral offset direction (e.g. left/right of route) — named
    /// `LateralDirection` rather than `Direction` to avoid colliding
    /// with [`Direction`] (uplink/downlink) above.
    LateralDirection,
    Distance,
    DistanceOffset,
    VerticalRate,
    ToFrom,
    Route,
    Procedure,
    Squawk,
    IcaoId,
    IcaoName,
    Frequency,
    Degrees,
    AltimeterSetting,
    FreeText,
    PersonsOnBoard,
    PositionReport,
    PdcData,
    Tp4Table,
    ErrorInfo,
    Version,
    AtisCode,
    LegType,
}

/// One row of the GOLD element table. `'static` data — see the module
/// docs on why this is never expressed as match arms.
#[derive(Debug, Clone, Copy)]
pub struct ElementSpec {
    /// GOLD element id (e.g. `"UM74"`, `"DM67b"`) for the transcribed
    /// rows, or a descriptive Hoppie-specific id (`"DM_REQUEST_LOGON"`)
    /// for the handful of rows with no GOLD equivalent — see the
    /// module docs.
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

/// Uplink table: Hoppie-specific rows first, then the transcribed GOLD
/// catalog. Order only matters for [`find`]'s first-match semantics,
/// and the two sources never share an id (see the module docs).
pub fn um_table() -> impl Iterator<Item = &'static ElementSpec> {
    crate::elements_data::HOPPIE_UM_TABLE
        .iter()
        .chain(crate::elements_data::GOLD_UM_TABLE.iter())
}

pub fn dm_table() -> impl Iterator<Item = &'static ElementSpec> {
    crate::elements_data::HOPPIE_DM_TABLE
        .iter()
        .chain(crate::elements_data::GOLD_DM_TABLE.iter())
}

/// Look up an [`ElementSpec`] by id across both directions' tables.
pub fn find(id: &str) -> Option<&'static ElementSpec> {
    um_table().chain(dm_table()).find(|e| e.id == id)
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
    match_in_table(text, um_table())
}

/// Best-effort match of raw downlink wire text against the DM table
/// (used for round-trip tests / re-parsing our own sent history, not
/// for interpreting received packets — see [`match_uplink_text`]).
pub fn match_downlink_text(text: &str) -> ParsedElement {
    match_in_table(text, dm_table())
}

/// Try candidates MOST-specific-first (most literal characters, i.e.
/// least reliant on a placeholder to "explain" the text). The real
/// GOLD table has several elements whose entire template is just a
/// single trailing placeholder (`"@1"`, e.g. `UM73`'s embedded-PDC-data
/// carrier, or `UM169`/`UM183`/... free-text carriers) — matched in
/// table order, one of those would swallow EVERY uplink text before a
/// more specific template (like `UM74`'s `"PROCEED DIRECT TO @1"`)
/// ever got a chance, and a template like `UM6`'s `"EXPECT @1"` would
/// likewise shadow the longer, more specific `UM7`'s
/// `"EXPECT CLIMB AT @1"`. Sorting by specificity first — falling back
/// to a catch-all free-text element only once every more specific
/// template has failed — fixes both.
fn match_in_table(text: &str, table: impl Iterator<Item = &'static ElementSpec>) -> ParsedElement {
    let trimmed = text.trim();
    let mut candidates: Vec<&'static ElementSpec> = table.collect();
    candidates.sort_by_key(|spec| std::cmp::Reverse(literal_specificity(spec.template)));
    for spec in candidates {
        if let Some(resolved) = try_match_template(spec, trimmed) {
            return ParsedElement::Recognized(resolved);
        }
    }
    ParsedElement::Raw(trimmed.to_string())
}

/// Total character length of a template's literal (non-placeholder)
/// segments — the specificity score [`match_in_table`] sorts on.
fn literal_specificity(template: &str) -> usize {
    split_template(template)
        .iter()
        .map(|seg| match seg {
            TemplateSegment::Literal(l) => l.len(),
            TemplateSegment::Placeholder => 0,
        })
        .sum()
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
        let spec = find("UM74").expect("GOLD table has PROCEED DIRECT TO [position]");
        let resolved = resolve(spec, &["UDROS".to_string()]).unwrap();
        assert_eq!(resolved.filled_text, "PROCEED DIRECT TO UDROS");
    }

    #[test]
    fn resolve_rejects_arity_mismatch() {
        let spec = find("UM74").unwrap();
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
        let spec = find("DM0").unwrap();
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
                assert_eq!(r.spec_id, "UM74");
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
    fn match_uplink_text_falls_back_to_a_generic_free_text_element_not_raw() {
        // The real GOLD catalog includes several genuine free-text
        // *carrier* elements on the uplink side too (UM73's embedded
        // PDC data, UM169/UM183/UM194.../UM208's plain free text) —
        // structurally, ANY uplink text COULD be one of those, so an
        // unrecognized instruction correctly resolves to one of them
        // rather than Raw. This mirrors reality: Raw is the fallback
        // for when decode()/matching itself can't produce a value at
        // all (see the next test), not "text didn't match a specific
        // instruction".
        // Which exact free-text-ish element wins isn't pinned here —
        // several UM elements are plausible (single free-text
        // placeholder, or e.g. two placeholders split on a space) and
        // the tie-break among them isn't semantically meaningful. What
        // matters is that a genuinely unrecognized instruction still
        // reconstructs the full original text via its captured
        // value(s), rather than silently truncating input.
        match elements_match_uplink("SOME UNKNOWN INSTRUCTION 42") {
            ParsedElement::Recognized(r) => {
                assert_eq!(r.values.join(" "), "SOME UNKNOWN INSTRUCTION 42");
            }
            other => panic!("expected Recognized(<free-text-ish element>), got {other:?}"),
        }
    }

    #[test]
    fn match_uplink_text_empty_input_falls_back_to_raw() {
        // Every free-text placeholder rejects an empty capture (see
        // try_match_template), and no zero-placeholder template has an
        // empty literal — so empty input is genuinely unmatchable
        // against any real element, unlike the "unknown text" case
        // above.
        match elements_match_uplink("") {
            ParsedElement::Raw(text) => assert_eq!(text, ""),
            other => panic!("expected Raw, got {other:?}"),
        }
    }

    #[test]
    fn match_uplink_text_prefers_the_more_specific_of_two_overlapping_templates() {
        // UM6 "EXPECT @1" and UM7 "EXPECT CLIMB AT @1" both start with
        // "EXPECT " — without specificity-first ordering, UM6's
        // trailing placeholder would greedily swallow UM7's text too.
        match elements_match_uplink("EXPECT CLIMB AT 1200Z") {
            ParsedElement::Recognized(r) => assert_eq!(r.spec_id, "UM7"),
            other => panic!("expected Recognized(UM7), got {other:?}"),
        }
    }

    #[test]
    fn direction_specific_matching_resolves_the_unable_ambiguity() {
        // "UNABLE" exists as BOTH the uplink GOLD element UM0 (an ATC
        // "cannot comply" response — also what Hoppie's simplified
        // logon handshake reuses for a rejected REQUEST LOGON, see
        // thread.rs) and the downlink GOLD element DM1 (a pilot WU
        // reply). Direction-specific matching must resolve each to the
        // correct one rather than cross-matching.
        match elements_match_uplink("UNABLE") {
            ParsedElement::Recognized(r) => assert_eq!(r.spec_id, "UM0"),
            other => panic!("expected Recognized(UM0), got {other:?}"),
        }
        match match_downlink_text("UNABLE") {
            ParsedElement::Recognized(r) => assert_eq!(r.spec_id, "DM1"),
            other => panic!("expected Recognized(DM1), got {other:?}"),
        }
    }

    #[test]
    fn find_returns_none_for_unknown_id() {
        assert!(find("DOES_NOT_EXIST").is_none());
    }

    /// Corpus-sweep: every row across both tables (Hoppie-specific +
    /// the ~287-row GOLD catalog) round-trips through resolve() ->
    /// filled text -> match (in the SAME direction's table it came
    /// from) -> back to the same spec_id. Cheap insurance that the
    /// table stays internally consistent at scale.
    #[test]
    fn every_table_row_round_trips_resolve_then_match() {
        for spec in um_table().chain(dm_table()) {
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
                ParsedElement::Recognized(r) => {
                    // Some GOLD elements share an IDENTICAL template and
                    // differ only by argument TYPE — e.g. UM7 "EXPECT
                    // CLIMB AT [time]" vs UM8 "EXPECT CLIMB AT
                    // [position]". A pure-text matcher structurally
                    // cannot tell "1200Z" apart from "UDROS" without a
                    // per-PlaceholderKind value grammar (real but
                    // separate future work — see elements.rs's docs),
                    // so the meaningful invariant here is landing on
                    // ANY element with the same template in the same
                    // direction, not necessarily the exact original.
                    let template_siblings: Vec<&str> = match spec.direction {
                        Direction::Uplink => um_table()
                            .filter(|s| s.template == spec.template)
                            .map(|s| s.id)
                            .collect(),
                        Direction::Downlink => dm_table()
                            .filter(|s| s.template == spec.template)
                            .map(|s| s.id)
                            .collect(),
                    };
                    assert!(
                        template_siblings.contains(&r.spec_id),
                        "round-trip of {} (template {:?}) matched {}, not template-equivalent (siblings: {:?})",
                        spec.id,
                        spec.template,
                        r.spec_id,
                        template_siblings
                    );
                }
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
