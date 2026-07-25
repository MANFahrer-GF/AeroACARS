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

/// GOLD downlink id for STANDBY — the one reply that acknowledges an
/// uplink without answering it. See [`CpdlcThread::record_sent`].
const STANDBY_ID: &str = "DM2";

/// Hoppie-specific downlink that ends the CPDLC session. See
/// [`CpdlcThread::record_sent`].
const LOGOFF_ID: &str = "DM_LOGOFF";

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
    /// MIN of our outstanding `DM_REQUEST_LOGON`, if any. Ties logon-
    /// outcome interpretation to a specific MRN-threaded reply — see
    /// [`logon_outcome`]'s docs for why this can't be a bare text match
    /// once the real GOLD table (where `UM0`/`"UNABLE"` is a
    /// general-purpose response, not logon-specific) is in play.
    logon_request_min: Option<u32>,
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
    ///
    /// STANDBY is the documented exception: it acknowledges receipt but
    /// explicitly defers the real answer, so the uplink stays open and
    /// keeps counting toward `pending_response_count`. Closing on
    /// STANDBY would drop the instruction off the pilot's list while
    /// ATC is still waiting for a WILCO.
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
        let defers = matches!(&parsed, ParsedElement::Recognized(r) if r.spec_id == STANDBY_ID);
        if let Some(m) = mrn {
            if !defers {
                self.close_open_entry(m);
            }
        }
        // Sending LOGOFF ends the session from our side immediately —
        // there is no acknowledgement to wait for. It also has to clear
        // the outstanding logon entry: leaving it in `open` kept
        // `pending_response_count` above zero forever, which pinned the
        // poller to its 20s "reply outstanding" cadence for the rest of
        // the session — against a free, volunteer-run service that asks
        // for 45-75s.
        if matches!(&parsed, ParsedElement::Recognized(r) if r.spec_id == LOGOFF_ID) {
            self.logged_on = false;
            if let Some(pending) = self.logon_request_min.take() {
                self.close_open_entry(pending);
            }
        }
        if let ParsedElement::Recognized(r) = &parsed {
            if r.spec_id == "DM_REQUEST_LOGON" {
                // A previous logon attempt that never got answered must
                // not stay pending — only the newest one counts.
                if let Some(previous) = self.logon_request_min.replace(min) {
                    self.close_open_entry(previous);
                }
            }
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
        if let Some(accepted) = logon_outcome(&message, self.logon_request_min) {
            self.logged_on = accepted;
            // Close our REQUEST LOGON here too, not just via the MRN path
            // above. Controller clients routinely answer without an MRN
            // (vSMR sets one on nothing at all), and then the branch at
            // the top never fires — leaving the logon permanently open,
            // counting as an outstanding reply, and holding the poller at
            // its 20s fast cadence for the whole session.
            if let Some(pending) = self.logon_request_min.take() {
                if resolves != Some(pending) {
                    self.close_open_entry(pending);
                }
            }
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

    /// Open UPLINKS only — the messages ATC is waiting on the pilot for.
    ///
    /// Distinct from [`pending_response_count`](Self::pending_response_count),
    /// which also counts our own outstanding requests (DM9/DM22/DM25 and
    /// friends carry `AnyRequired`, so they stay open until ATC answers).
    /// Both belong in the faster poll cadence, but only this one may
    /// drive a "you must reply" badge — a pilot waiting on their own
    /// direct-to request has nothing to answer.
    pub fn pending_uplink_count(&self) -> usize {
        self.open
            .values()
            .filter(|p| p.direction == Direction::Uplink)
            .count()
    }

    /// Whether the CPDLC logon handshake has completed with
    /// `LOGON ACCEPTED` (and not since superseded by an `UNABLE`).
    pub fn is_logged_on(&self) -> bool {
        self.logged_on
    }

    /// Drop the session without sending anything — used when the
    /// network hands us over to another facility: the old centre has
    /// released us, the new one has not accepted yet, so we are
    /// logged on to nobody until it answers.
    pub fn mark_logged_off(&mut self) {
        self.logged_on = false;
        if let Some(pending) = self.logon_request_min.take() {
            self.close_open_entry(pending);
        }
    }

    /// MIN of a `REQUEST LOGON` still waiting for its answer, if any.
    /// The wiring layer pairs this with the message's timestamp to tell
    /// the pilot when a facility simply isn't answering — several
    /// stations (a delivery desk, or any controller client that doesn't
    /// implement CPDLC logon) never reply at all, and without this the
    /// UI sits on "LOGON SENT" forever.
    pub fn pending_logon_min(&self) -> Option<u32> {
        self.logon_request_min
    }

    pub fn history(&self) -> &[ThreadEntry] {
        &self.history
    }
}

/// `Some(true)` for a `LOGON ACCEPTED` uplink, `Some(false)` for an
/// `UNABLE` uplink — but ONLY when it replies (`mrn ==
/// logon_request_min`) to our own outstanding `REQUEST LOGON`.
///
/// This MRN-gating matters because the real GOLD table makes `UM0`
/// (`"UNABLE"`) a general-purpose "cannot comply" response that ATC
/// can send for all sorts of unrelated requests throughout a session —
/// Hoppie's simplified logon handshake happens to reuse that exact
/// same element for a rejected `REQUEST LOGON`, but a bare text/id
/// match (as a first cut of this function did) would misinterpret ANY
/// later `UNABLE` uplink as a logon failure. Threading it through the
/// MRN instead ties the interpretation to the specific reply.
fn logon_outcome(message: &CpdlcMessage, logon_request_min: Option<u32>) -> Option<bool> {
    // Nothing outstanding — nothing to interpret as its answer.
    logon_request_min?;

    let replies_to_our_logon = message.mrn.is_some() && message.mrn == logon_request_min;
    match &message.parsed {
        ParsedElement::Recognized(r) => match r.spec_id {
            // Unambiguous: this element means one thing and one thing
            // only, so it counts even without an MRN. Real controller
            // clients routinely omit it — vSMR sets an MRN on nothing at
            // all — and insisting on correlation meant a logon there
            // could never complete.
            "UM_LOGON_ACCEPTED" => Some(true),
            // Ambiguous: GOLD's UM0 is a general-purpose "cannot comply"
            // that ATC sends for all kinds of unrelated requests. Only an
            // MRN tying it to OUR logon makes it a logon refusal;
            // otherwise a later UNABLE would silently log us off.
            "UM0" if replies_to_our_logon => Some(false),
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
        let min2 = send(&mut thread, "DM0", &[], None);
        assert_eq!(min2, 2);
        assert_eq!(thread.peek_next_min(), 3);
    }

    #[test]
    fn logon_accepted_flips_logged_on_and_does_not_open_a_pending_entry() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert_eq!(
            thread.pending_response_count(),
            1,
            "REQUEST LOGON (Y) awaits a response"
        );
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
        send(&mut thread, "DM0", &[], Some(7));
        assert_eq!(thread.pending_response_count(), 0);

        let history = thread.history();
        assert_eq!(history.len(), 2);
        assert!(
            history[0].closed,
            "the original uplink must be marked closed"
        );
        assert_eq!(history[1].mrn, Some(7));
    }

    /// The poll cadence is derived from `pending_response_count`, so
    /// anything that stays open forever pins us to the 20s "reply
    /// outstanding" rate against a free, volunteer-run service that asks
    /// for 45-75s. These guard every way a logon could get stuck.
    #[test]
    fn an_mrn_less_logon_accept_still_closes_the_request() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert_eq!(thread.pending_response_count(), 1);

        // vSMR and friends answer without an MRN — the common case.
        receive(&mut thread, "/data2/9000//NE/LOGON ACCEPTED");
        assert!(thread.is_logged_on());
        assert_eq!(
            thread.pending_response_count(),
            0,
            "an MRN-less accept must not leave the logon pending forever"
        );
    }

    #[test]
    fn a_second_logon_attempt_does_not_stack_pending_entries() {
        let mut thread = CpdlcThread::new();
        // Station never answers; the pilot tries the next one.
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert_eq!(
            thread.pending_response_count(),
            1,
            "only the newest attempt may count as outstanding"
        );
    }

    #[test]
    fn logoff_after_an_unanswered_logon_restores_the_normal_poll_rate() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert_eq!(thread.pending_response_count(), 1);

        send(&mut thread, "DM_LOGOFF", &[], None);
        assert_eq!(
            thread.pending_response_count(),
            0,
            "giving up on a silent station must not keep us polling fast"
        );
    }

    #[test]
    fn logoff_clears_both_the_session_and_any_pending_logon() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert!(thread.pending_logon_min().is_some(), "logon is in flight");

        receive(&mut thread, "/data2/9000/1/NE/LOGON ACCEPTED");
        assert!(thread.is_logged_on());
        assert!(
            thread.pending_logon_min().is_none(),
            "the accept resolves it"
        );

        // Logging off must leave NOTHING outstanding. Reporting a pending
        // logon here is what made the UI show "logon sent" in amber
        // immediately after the pilot logged off.
        send(&mut thread, "DM_LOGOFF", &[], None);
        assert!(!thread.is_logged_on(), "session is over");
        assert!(
            thread.pending_logon_min().is_none(),
            "no logon may look outstanding after LOGOFF"
        );
    }

    #[test]
    fn logoff_while_a_logon_is_still_unanswered_also_clears_it() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM_REQUEST_LOGON", &[], None);
        assert!(thread.pending_logon_min().is_some());

        // Pilot gives up on a station that never answers and logs off.
        send(&mut thread, "DM_LOGOFF", &[], None);
        assert!(thread.pending_logon_min().is_none());
        assert!(!thread.is_logged_on());
    }

    #[test]
    fn our_own_open_request_never_counts_as_something_the_pilot_must_answer() {
        let mut thread = CpdlcThread::new();
        // DM22 REQUEST DIRECT TO carries AnyRequired, so it stays open
        // until ATC replies — but the pilot owes nobody an answer.
        send(&mut thread, "DM22", &["UDROS"], None);
        assert_eq!(
            thread.pending_response_count(),
            1,
            "our request is outstanding, so keep polling fast"
        );
        assert_eq!(
            thread.pending_uplink_count(),
            0,
            "but the pilot has nothing to reply to"
        );

        // An actual instruction from ATC is what the pilot must answer.
        receive(&mut thread, "/data2/7//WU/CLIMB TO AND MAINTAIN FL240");
        assert_eq!(thread.pending_response_count(), 2);
        assert_eq!(thread.pending_uplink_count(), 1);

        send(&mut thread, "DM0", &[], Some(7));
        assert_eq!(thread.pending_uplink_count(), 0);
        assert_eq!(
            thread.pending_response_count(),
            1,
            "our own request is still waiting on ATC"
        );
    }

    #[test]
    fn standby_acknowledges_but_leaves_the_uplink_open_for_a_real_answer() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/7//WU/CLIMB TO AND MAINTAIN FL240");
        assert_eq!(thread.pending_response_count(), 1);

        // STANDBY defers — ATC is still waiting, so the instruction must
        // stay on the pilot's list.
        send(&mut thread, "DM2", &[], Some(7));
        assert_eq!(
            thread.pending_response_count(),
            1,
            "STANDBY must not close the uplink"
        );
        assert!(
            !thread.history()[0].closed,
            "the uplink must still read as open after STANDBY"
        );

        // The real answer closes it.
        send(&mut thread, "DM0", &[], Some(7));
        assert_eq!(thread.pending_response_count(), 0);
        assert!(thread.history()[0].closed);
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
        send(&mut thread, "DM0", &[], Some(1));
        assert_eq!(thread.pending_response_count(), 1);
        send(&mut thread, "DM0", &[], Some(2));
        assert_eq!(thread.pending_response_count(), 0);
    }
}
