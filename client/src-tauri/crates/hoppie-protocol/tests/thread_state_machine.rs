//! Fixture-replay tests for the MIN/MRN threading state machine.
//! Mirrors the AeroACARS app crate's `phase_v2_replay.rs` pattern —
//! small recorded (or spec-accurate hand-built) exchanges replayed
//! end-to-end, asserting the final thread state rather than testing
//! each step in isolation. Fixtures live in `tests/fixtures/*.txt`.

use hoppie_protocol::cpdlc;
use hoppie_protocol::elements::{self as els, find};
use hoppie_protocol::thread::CpdlcThread;

/// One line of a fixture file, after parsing.
enum FixtureLine {
    Send {
        element_id: String,
        values: Vec<String>,
        mrn: Option<u32>,
    },
    Recv {
        packet: String,
    },
}

fn parse_fixture(text: &str) -> Vec<FixtureLine> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let mut parts = line.splitn(2, '|');
            let tag = parts.next().unwrap();
            let rest = parts.next().unwrap_or_default();
            match tag {
                "SEND" => {
                    let mut fields = rest.splitn(3, '|');
                    let element_id = fields.next().unwrap_or_default().to_string();
                    let values_str = fields.next().unwrap_or_default();
                    let values: Vec<String> = if values_str.is_empty() {
                        Vec::new()
                    } else {
                        values_str.split(',').map(str::to_string).collect()
                    };
                    let mrn_str = fields.next().unwrap_or_default().trim();
                    let mrn = if mrn_str.is_empty() {
                        None
                    } else {
                        Some(mrn_str.parse().expect("fixture MRN must be a number"))
                    };
                    FixtureLine::Send {
                        element_id,
                        values,
                        mrn,
                    }
                }
                "RECV" => FixtureLine::Recv {
                    packet: rest.to_string(),
                },
                other => panic!("unknown fixture line tag {other:?}"),
            }
        })
        .collect()
}

fn replay(fixture_text: &str) -> CpdlcThread {
    let mut thread = CpdlcThread::new();
    for line in parse_fixture(fixture_text) {
        match line {
            FixtureLine::Send {
                element_id,
                values,
                mrn,
            } => {
                let spec = find(&element_id)
                    .unwrap_or_else(|| panic!("fixture references unknown element {element_id:?}"));
                let resolved = els::resolve(spec, &values)
                    .unwrap_or_else(|e| panic!("resolve({element_id}) failed: {e}"));
                thread.record_sent(
                    spec.response,
                    mrn,
                    resolved.filled_text.clone(),
                    els::ParsedElement::Recognized(resolved),
                );
            }
            FixtureLine::Recv { packet } => {
                let msg = cpdlc::decode(&packet, els::Direction::Uplink)
                    .unwrap_or_else(|e| panic!("decode({packet:?}) failed: {e}"));
                thread.record_received(msg);
            }
        }
    }
    thread
}

#[test]
fn logon_accepted_fixture_ends_logged_on_with_nothing_pending() {
    let thread = replay(include_str!("fixtures/logon_accepted.txt"));
    assert!(thread.is_logged_on());
    assert_eq!(thread.pending_response_count(), 0);
    assert_eq!(thread.history().len(), 2);
}

#[test]
fn logon_unable_fixture_ends_not_logged_on_with_nothing_pending() {
    let thread = replay(include_str!("fixtures/logon_unable.txt"));
    assert!(!thread.is_logged_on());
    assert_eq!(thread.pending_response_count(), 0);
}

#[test]
fn direct_to_sequence_fixture_closes_the_uplink_via_wilco() {
    let thread = replay(include_str!("fixtures/cpdlc_direct_to_sequence.txt"));
    assert_eq!(thread.pending_response_count(), 0);
    let history = thread.history();
    assert_eq!(history.len(), 2);
    assert!(history[0].closed, "the PROCEED DIRECT TO uplink must be closed by the WILCO reply");
    assert_eq!(history[1].mrn, Some(7));
}
