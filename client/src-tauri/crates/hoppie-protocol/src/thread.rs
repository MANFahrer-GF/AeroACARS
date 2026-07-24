//! MIN/MRN threading state machine for a single CPDLC connection.
//!
//! **MIN (Message Identification Number)** — assigned sequentially by
//! the sender to every message it originates (uplink MINs by the
//! ground system, downlink MINs by us). **MRN (Message Reference
//! Number)** — when replying to a message, its MIN is echoed back as
//! the reply's MRN, closing the loop. See
//! `devHazz/hoppie-acars-docs`'s `Messaging/CPDLC Format.md`.

use std::collections::HashMap;

use crate::cpdlc::{CpdlcMessage, ResponseRequirement};
use crate::elements::{Direction, ParsedElement};

#[derive(Debug, Clone, PartialEq)]
struct PendingMessage {
    direction: Direction,
    response: ResponseRequirement,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadEntry {
    pub min: u32,
    pub mrn: Option<u32>,
    pub direction: Direction,
    pub message: CpdlcMessage,
    /// Set once a matching reply (by MRN) has closed this entry's open
    /// response requirement. Always `false` for entries that never
    /// required a response.
    pub closed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ThreadEvent {
    /// We just sent a downlink message, assigned `min`.
    Sent { min: u32 },
    /// An uplink arrived. `resolves` is `Some(x)` when this message's
    /// `mrn` closed one of OUR open MINs.
    ReceivedUplink {
        min: u32,
        mrn: Option<u32>,
        resolves: Option<u32>,
    },
}

/// Per-connection MIN/MRN bookkeeping + full message history.
#[derive(Debug, Default)]
pub struct CpdlcThread {
    next_min: u32,
    open: HashMap<u32, PendingMessage>,
    history: Vec<ThreadEntry>,
    logged_on: bool,
}

impl CpdlcThread {
    pub fn new() -> Self {
        Self {
            next_min: 1,
            ..Default::default()
        }
    }

    /// Peek the MIN the next `record_sent` call will assign — mainly
    /// useful for tests/UI preview.
    pub fn peek_next_min(&self) -> u32 {
        self.next_min
    }

    /// Allocate a MIN, build the full [`CpdlcMessage`], record it as
    /// sent (opening a pending-response entry unless `response` is
    /// [`ResponseRequirement::NoResponseExpected`]), and — when `mrn`
    /// references one of our open uplink MINs — close that entry.
    /// Returns the constructed message; the caller passes it to
    /// [`crate::cpdlc::encode`] to get the actual wire string to send.
    pub fn record_sent(
        &mut self,
        response: ResponseRequirement,
        mrn: Option<u32>,
        element_text: String,
        parsed: ParsedElement,
    ) -> (CpdlcMessage, ThreadEvent) {
        let min = self.next_min;
        self.next_min += 1;

        if response.requires_reply() {
            self.open.insert(
                min,
                PendingMessage {
                    direction: Direction::Downlink,
                    response,
                },
            );
        }
        if let Some(m) = mrn {
            self.close_open_entry(m);
        }

        let message = CpdlcMessage {
            min,
            mrn,
            response,
            element_text,
            parsed,
        };
        self.history.push(ThreadEntry {
            min,
            mrn,
            direction: Direction::Downlink,
            message: message.clone(),
            closed: false,
        });
        (message, ThreadEvent::Sent { min })
    }

    /// Record an inbound uplink message (already decoded via
    /// [`crate::cpdlc::decode`] with `Direction::Uplink`).
    pub fn record_received(&mut self, message: CpdlcMessage) -> ThreadEvent {
        let min = message.min;
        let mrn = message.mrn;
        let mut resolves = None;
        if let Some(m) = mrn {
            if self.open.remove(&m).is_some() {
                resolves = Some(m);
                self.mark_closed(m);
            }
        }
        if message.response.requires_reply() {
            self.open.insert(
                min,
                PendingMessage {
                    direction: Direction::Uplink,
                    response: message.response,
                },
            );
        }
        if let Some(accepted) = logon_outcome(&message) {
            self.logged_on = accepted;
        }
        self.history.push(ThreadEntry {
            min,
            mrn,
            direction: Direction::Uplink,
            message,
            closed: false,
        });
        ThreadEvent::ReceivedUplink { min, mrn, resolves }
    }

    fn close_open_entry(&mut self, min: u32) {
        self.open.remove(&min);
        self.mark_closed(min);
    }

    fn mark_closed(&mut self, min: u32) {
        if let Some(entry) = self.history.iter_mut().find(|e| e.min == min) {
            entry.closed = true;
        }
    }

    /// Number of messages awaiting a response — drives the poller's
    /// fast-poll (~20s) vs. baseline (45-75s) cadence.
    pub fn pending_response_count(&self) -> usize {
        self.open.len()
    }

    /// Whether the CPDLC logon handshake has completed with
    /// `LOGON ACCEPTED` (and not since superseded by an `UNABLE`).
    pub fn is_logged_on(&self) -> bool {
        self.logged_on
    }

    pub fn history(&self) -> &[ThreadEntry] {
        &self.history
    }
}

/// `Some(true)` for a `LOGON ACCEPTED` uplink, `Some(false)` for a
/// logon-rejection `UNABLE`, `None` for anything else.
fn logon_outcome(message: &CpdlcMessage) -> Option<bool> {
    match &message.parsed {
        ParsedElement::Recognized(r) => match r.spec_id {
            "UM_LOGON_ACCEPTED" => Some(true),
            "UM_LOGON_UNABLE" => Some(false),
            _ => None,
        },
        ParsedElement::Raw(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpdlc;
    use crate::elements::{self as els, find};

    fn send(thread: &mut CpdlcThread, spec_id: &str, values: &[&str], mrn: Option<u32>) -> u32 {
        let spec = find(spec_id).unwrap();
        let values: Vec<String> = values.iter().map(|s| s.to_string()).collect();
        let resolved = els::resolve(spec, &values).unwrap();
        let filled_text = resolved.filled_text.clone();
        let (msg, event) = thread.record_sent(
            spec.response,
            mrn,
            filled_text,
            els::ParsedElement::Recognized(resolved),
        );
        match event {
            ThreadEvent::Sent { min } => {
                assert_eq!(min, msg.min);
                min
            }
            other => panic!("expected Sent, got {other:?}"),
        }
    }

    fn receive(thread: &mut CpdlcThread, packet: &str) -> ThreadEvent {
        let msg = cpdlc::decode(packet, els::Direction::Uplink).unwrap();
        thread.record_received(msg)
    }

    #[test]
    fn min_is_monotonic_and_starts_at_one() {
        let mut thread = CpdlcThread::new();
        assert_eq!(thread.peek_next_min(), 1);
        let min1 = send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert_eq!(min1, 1);
        let min2 = send(&mut thread, "DM_WILCO", &[], None);
        assert_eq!(min2, 2);
        assert_eq!(thread.peek_next_min(), 3);
    }

    #[test]
    fn logon_accepted_flips_logged_on_and_does_not_open_a_pending_entry() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert_eq!(thread.pending_response_count(), 1, "REQUEST LOGON (Y) awaits a response");
        assert!(!thread.is_logged_on());

        let event = receive(&mut thread, "/data2/1/1/NE/LOGON ACCEPTED");
        assert!(thread.is_logged_on());
        assert_eq!(
            event,
            ThreadEvent::ReceivedUplink {
                min: 1,
                mrn: Some(1),
                resolves: Some(1),
            }
        );
        assert_eq!(
            thread.pending_response_count(),
            0,
            "the MRN=1 uplink must close our open REQUEST LOGON"
        );
    }

    #[test]
    fn logon_unable_sets_logged_on_false() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        receive(&mut thread, "/data2/1/1/NE/UNABLE");
        assert!(!thread.is_logged_on());
    }

    #[test]
    fn full_arc_uplink_instruction_then_downlink_reply_closes_the_loop() {
        let mut thread = CpdlcThread::new();
        // Uplink: "PROCEED DIRECT TO UDROS" (WU), no MRN (unsolicited).
        let event = receive(&mut thread, "/data2/7//WU/PROCEED DIRECT TO UDROS");
        assert_eq!(
            event,
            ThreadEvent::ReceivedUplink {
                min: 7,
                mrn: None,
                resolves: None,
            }
        );
        assert_eq!(thread.pending_response_count(), 1);

        // We reply WILCO, MRN=7 — must close the uplink's pending entry.
        send(&mut thread, "DM_WILCO", &[], Some(7));
        assert_eq!(thread.pending_response_count(), 0);

        let history = thread.history();
        assert_eq!(history.len(), 2);
        assert!(history[0].closed, "the original uplink must be marked closed");
        assert_eq!(history[1].mrn, Some(7));
    }

    #[test]
    fn unmatched_mrn_does_not_panic_and_stays_pending() {
        let mut thread = CpdlcThread::new();
        // Reply referencing a MIN we never opened.
        let event = receive(&mut thread, "/data2/9/999/N/ROGER");
        assert_eq!(
            event,
            ThreadEvent::ReceivedUplink {
                min: 9,
                mrn: Some(999),
                resolves: None,
            }
        );
    }

    #[test]
    fn pending_response_count_reflects_multiple_open_entries() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/1//WU/PROCEED DIRECT TO UDROS");
        receive(&mut thread, "/data2/2//WU/PROCEED DIRECT TO UDROS");
        assert_eq!(thread.pending_response_count(), 2);
        send(&mut thread, "DM_WILCO", &[], Some(1));
        assert_eq!(thread.pending_response_count(), 1);
        send(&mut thread, "DM_WILCO", &[], Some(2));
        assert_eq!(thread.pending_response_count(), 0);
    }
}
