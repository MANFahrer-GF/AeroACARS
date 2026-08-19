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
    assert!(
        history[0].closed,
        "the PROCEED DIRECT TO uplink must be closed by the WILCO reply"
    );
    assert_eq!(history[1].mrn, Some(7));
}

// --- v1.6.12 (#pdc-station): a reply that never reached the wire ---
//
// Field case 2026-08-19: a PDC clearance was answered with WILCO, the
// card read "WILCO gesendet 08:44:28z", and ten minutes later ATC sent
// "ACK NOT RECEIVED / CLEARANCE CANCELLED". Nothing in the app could
// tell a delivered acknowledgement from one that failed on the way out,
// because the thread was mutated before the send and never undone.

/// Send helper mirroring what `send_cpdlc_element` does to the thread.
fn send(thread: &mut CpdlcThread, element_id: &str, mrn: Option<u32>) -> u32 {
    let spec = find(element_id).expect("known element");
    let resolved = els::resolve(spec, &[]).expect("no placeholders");
    let (message, _) = thread.record_sent(
        spec.response,
        mrn,
        resolved.filled_text.clone(),
        els::ParsedElement::Recognized(resolved),
    );
    message.min
}

fn recv(thread: &mut CpdlcThread, packet: &str) {
    let msg = cpdlc::decode(packet, els::Direction::Uplink).expect("decodable");
    thread.record_received(msg);
}

#[test]
fn rolled_back_wilco_leaves_the_clearance_open_and_unanswered() {
    let mut thread = CpdlcThread::new();
    // ATC's clearance, MIN 4, WILCO/UNABLE required.
    recv(&mut thread, "/data2/4//WU/CLEARED TO EDDB");
    assert_eq!(thread.pending_uplink_count(), 1);

    let min = send(&mut thread, "DM0", Some(4));
    assert!(thread.history()[0].closed, "sanity: the WILCO closed it");
    assert_eq!(thread.pending_uplink_count(), 0);

    // ...and the send failed.
    thread.rollback_sent(min);

    assert_eq!(
        thread.history().len(),
        1,
        "a WILCO that never went out must not stand in the log"
    );
    assert!(
        !thread.history()[0].closed,
        "the clearance is still waiting for an answer"
    );
    assert_eq!(
        thread.pending_uplink_count(),
        1,
        "ATC is still owed a reply — the badge must say so"
    );
    assert_eq!(
        thread.peek_next_min(),
        min,
        "the unused MIN is handed out again rather than skipped"
    );
}

#[test]
fn rolled_back_standby_clears_the_deferred_flag_only_when_no_standby_remains() {
    let mut thread = CpdlcThread::new();
    recv(&mut thread, "/data2/4//WU/CLEARED TO EDDB");
    let first = send(&mut thread, "DM2", Some(4));
    assert!(thread.history()[0].deferred);

    let second = send(&mut thread, "DM2", Some(4));
    thread.rollback_sent(second);
    assert!(
        thread.history()[0].deferred,
        "the first STANDBY still stands — ATC has seen a deferral"
    );

    thread.rollback_sent(first);
    assert!(
        !thread.history()[0].deferred,
        "no STANDBY ever reached ATC, so nothing was deferred"
    );
    assert_eq!(thread.pending_uplink_count(), 1);
}

#[test]
fn rollback_keeps_later_min_numbers_that_are_already_on_the_wire() {
    let mut thread = CpdlcThread::new();
    recv(&mut thread, "/data2/4//WU/CLEARED TO EDDB");
    let failed = send(&mut thread, "DM0", Some(4));
    let later = send(&mut thread, "DM2", Some(4));
    thread.rollback_sent(failed);
    assert_eq!(
        thread.peek_next_min(),
        later + 1,
        "a MIN that is already on the wire must never be reused"
    );
}

#[test]
fn rollback_of_an_unknown_min_changes_nothing() {
    let mut thread = CpdlcThread::new();
    recv(&mut thread, "/data2/4//WU/CLEARED TO EDDB");
    thread.rollback_sent(99);
    assert_eq!(thread.history().len(), 1);
    assert_eq!(thread.pending_uplink_count(), 1);
}

#[test]
fn a_failed_second_reply_does_not_reopen_what_the_first_one_answered() {
    // QS 19.08.2026: WILCO goes out and is delivered; a later UNABLE for
    // the same clearance fails to send. Rolling that one back must not
    // undo the first, real answer.
    let mut thread = CpdlcThread::new();
    recv(&mut thread, "/data2/4//WU/CLEARED TO EDDB");
    send(&mut thread, "DM0", Some(4)); // WILCO — delivered
    let failed = send(&mut thread, "DM1", Some(4)); // UNABLE — never sent
    thread.rollback_sent(failed);

    assert!(
        thread.history()[0].closed,
        "the delivered WILCO still answers this clearance"
    );
    assert_eq!(
        thread.pending_uplink_count(),
        0,
        "ATC is not waiting on us — we answered once, successfully"
    );
}
