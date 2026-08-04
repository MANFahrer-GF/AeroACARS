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
    /// An uplink we have already answered with STANDBY. The instruction
    /// stays open — STANDBY defers rather than answers — but the pilot
    /// must not be able to defer it again: the controller would see a
    /// second and third deferral of the same clearance and learn nothing
    /// from any of them. Always `false` for downlinks.
    pub deferred: bool,
    /// This uplink was still open (no WILCO/UNABLE/etc. sent yet) when a
    /// sector handover happened — the controller who sent it is no
    /// longer the one talking to the aircraft. Distinct from `closed`:
    /// a superseded entry was never actually answered, it just became
    /// unanswerable. Set by [`CpdlcThread::mark_logged_off`]. Always
    /// `false` for downlinks (our own outstanding requests are left
    /// alone — see that method's doc comment for why).
    pub superseded: bool,
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

/// Key for the open-message store and the history: the MIN alone is NOT
/// unique. ATC numbers uplinks, we number downlinks, and both commonly
/// start at 1 — so our third message and ATC's third message are both
/// "MIN 3". Keying on the bare number let our downlink evict ATC's open
/// uplink, which dropped `pending_uplink_count` to zero while a
/// clearance was still waiting for a WILCO: the badge said "nothing to
/// do" while the log still said "awaiting reply".
type MessageKey = (Direction, u32);

/// Per-connection MIN/MRN bookkeeping + full message history.
#[derive(Debug, Default)]
pub struct CpdlcThread {
    next_min: u32,
    open: HashMap<MessageKey, PendingMessage>,
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
                (Direction::Downlink, min),
                PendingMessage {
                    direction: Direction::Downlink,
                    response,
                },
            );
        }
        let defers = matches!(&parsed, ParsedElement::Recognized(r) if r.spec_id == STANDBY_ID);
        if let Some(m) = mrn {
            if defers {
                // STANDBY leaves the instruction open on purpose, but
                // records that we've already deferred it once.
                if let Some(entry) = self.find_current_entry_mut(Direction::Uplink, m) {
                    entry.deferred = true;
                }
            } else {
                // Our MRN references ATC's uplink — that is what this
                // answer closes, not anything of ours.
                self.close_open_entry(Direction::Uplink, m);
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
                self.close_open_entry(Direction::Downlink, pending);
            }
        }
        if let ParsedElement::Recognized(r) = &parsed {
            if r.spec_id == "DM_REQUEST_LOGON" {
                // A previous logon attempt that never got answered must
                // not stay pending — only the newest one counts.
                if let Some(previous) = self.logon_request_min.replace(min) {
                    self.close_open_entry(Direction::Downlink, previous);
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
            deferred: false,
            superseded: false,
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
            // An uplink's MRN references one of OUR downlinks.
            if self.open.remove(&(Direction::Downlink, m)).is_some() {
                resolves = Some(m);
                self.mark_closed(Direction::Downlink, m);
            }
        }
        if message.response.requires_reply() {
            self.open.insert(
                (Direction::Uplink, min),
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
                    self.close_open_entry(Direction::Downlink, pending);
                }
            }
        }
        self.history.push(ThreadEntry {
            min,
            mrn,
            direction: Direction::Uplink,
            message,
            closed: false,
            deferred: false,
            superseded: false,
        });
        ThreadEvent::ReceivedUplink { min, mrn, resolves }
    }

    /// Close an entry in a specific direction. Which one matters: when
    /// WE reply with an MRN we are closing ATC's UPLINK, but when we
    /// abandon our own logon we are closing our own DOWNLINK. Keying on
    /// the bare MIN conflated the two.
    fn close_open_entry(&mut self, direction: Direction, min: u32) {
        self.open.remove(&(direction, min));
        self.mark_closed(direction, min);
    }

    /// Mark the history entry closed. Matches on direction as well as
    /// MIN — otherwise `find` returns whichever of the two numbering
    /// spaces happens to come first and ticks off the wrong message.
    ///
    /// A superseded entry is deliberately left alone: it was never
    /// legitimately answered, just orphaned by a handover, and a reply
    /// that still reaches this far (the UI is expected to block it, see
    /// [`Self::is_superseded_uplink`]) went to the wrong — new — station
    /// rather than the one that actually asked. Recording it as `closed`
    /// would misrepresent the audit trail as a real WILCO/UNABLE.
    fn mark_closed(&mut self, direction: Direction, min: u32) {
        if let Some(entry) = self.find_current_entry_mut(direction, min) {
            if !entry.superseded {
                entry.closed = true;
            }
        }
    }

    /// The most recent history entry for `(direction, min)`.
    ///
    /// A MIN is only unique within ONE controller's own numbering space
    /// for as long as that station is active — ATC renumbers from
    /// (usually) 1 with every new facility, so the same `(direction,
    /// min)` key can legitimately appear more than once across a
    /// handover: an old, now-superseded entry from the station that let
    /// go, and a brand new, live one from whichever facility took over.
    /// The live one is always the LATEST match: [`Self::mark_logged_off`]
    /// supersedes anything still open in the OLD station's numbering
    /// space before the new station's traffic can start arriving, so
    /// nothing colliding with it can exist yet at the time an older
    /// entry gets superseded. Searching from the end (rather than a
    /// plain `find`, which returns whichever occurrence is chronologically
    /// FIRST) is what makes closing/deferring/superseded-checks resolve
    /// to the right one instead of accidentally re-triggering the old,
    /// unrelated controller's message.
    fn find_current_entry_mut(&mut self, direction: Direction, min: u32) -> Option<&mut ThreadEntry> {
        self.history
            .iter_mut()
            .rev()
            .find(|e| e.min == min && e.direction == direction)
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
            .keys()
            .filter(|(direction, _)| *direction == Direction::Uplink)
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
    ///
    /// Also supersedes every still-open UPLINK: an instruction the old
    /// centre sent but the pilot hadn't answered yet becomes
    /// unanswerable the instant it lets go — that controller is no
    /// longer talking to the aircraft, and its MIN numbering space
    /// belongs to a session that's now over. Left open, it did two
    /// things wrong: kept counting toward `pending_uplink_count` (a
    /// permanent "you must reply" badge for an instruction nobody is
    /// waiting on any more), and if the pilot answered it anyway, the
    /// reply would go out addressed to whatever station is CURRENT at
    /// send time — the new centre, not the one that asked, silently
    /// misdirected. Superseding here removes it from `open` (fixing the
    /// badge/poll-cadence side) and flags the history entry (`superseded
    /// = true`, NOT `closed` — it was never actually answered) so the UI
    /// can grey it out and refuse to send a reply for it.
    ///
    /// Deliberately UPLINK-only: our own still-open downlink requests
    /// (e.g. a REQUEST DIRECT TO the old centre hasn't answered) aren't
    /// touched — nothing about them requires the pilot to act, and the
    /// pending logon they might reference is already closed just above.
    pub fn mark_logged_off(&mut self) {
        self.logged_on = false;
        if let Some(pending) = self.logon_request_min.take() {
            self.close_open_entry(Direction::Downlink, pending);
        }
        let stale_mins: Vec<u32> = self
            .open
            .keys()
            .filter(|(direction, _)| *direction == Direction::Uplink)
            .map(|(_, min)| *min)
            .collect();
        for min in stale_mins {
            self.open.remove(&(Direction::Uplink, min));
            // v0.20.x QS fix: a plain forward `.find()` here returned
            // whichever entry with this MIN came FIRST in history — on
            // a SECOND handover reusing a MIN a prior handover already
            // superseded, that's the old, already-superseded (no-op)
            // entry, not the new station's now-actually-stale one. Must
            // use the same reverse "most recent" search as
            // `mark_closed`/`is_superseded_uplink`, or a second-handover
            // MIN collision silently defeats the whole supersede
            // mechanism: `is_superseded_uplink` would keep reading the
            // NEW (unmarked) entry as not-superseded, letting a reply
            // through to the wrong station.
            if let Some(entry) = self.find_current_entry_mut(Direction::Uplink, min) {
                entry.superseded = true;
            }
        }
    }

    /// Whether uplink `min` was left unanswered by a handover — see
    /// [`Self::mark_logged_off`]. The sending layer must refuse to build
    /// a reply whose `mrn` references one of these: the station it would
    /// actually reach is not the one that asked.
    pub fn is_superseded_uplink(&self, min: u32) -> bool {
        // Must check only the MOST RECENT entry for this MIN, not
        // whether ANY entry ever was superseded — a `.any()` here would
        // permanently block replying to every future reuse of this MIN
        // number after just one handover superseded an older message
        // with the same number, even a brand new, genuinely still-open
        // instruction from whichever station took over. See
        // `find_current_entry_mut`'s doc for why "most recent" is
        // always the live one.
        self.history
            .iter()
            .rev()
            .find(|e| e.min == min && e.direction == Direction::Uplink)
            .is_some_and(|e| e.superseded)
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
    /// The two MIN spaces are independent — ATC numbers uplinks, we
    /// number downlinks, and both commonly start at 1. Sharing a key let
    /// our own message evict ATC's open instruction, so the badge said
    /// "nothing to answer" while a clearance was still waiting. The log
    /// and the counter would then disagree, which is worse than either
    /// being wrong on its own.
    /// STANDBY defers but does not answer, so the instruction stays
    /// open. It may only be sent ONCE though: a second deferral tells
    /// the controller nothing and just clutters their screen.
    #[test]
    fn standby_marks_the_uplink_deferred_without_closing_it() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/7//WU/CLIMB TO AND MAINTAIN FL240");

        let before = thread
            .history()
            .iter()
            .find(|e| e.direction == Direction::Uplink)
            .unwrap();
        assert!(!before.deferred, "nothing deferred yet");

        send(&mut thread, "DM2", &[], Some(7));

        let after = thread
            .history()
            .iter()
            .find(|e| e.direction == Direction::Uplink)
            .unwrap();
        assert!(after.deferred, "STANDBY must mark it deferred");
        assert!(!after.closed, "but STANDBY does not answer it");
        assert_eq!(
            thread.pending_uplink_count(),
            1,
            "ATC is still waiting for a real answer"
        );

        // A real answer closes it and the deferred flag is irrelevant.
        send(&mut thread, "DM0", &[], Some(7));
        assert_eq!(thread.pending_uplink_count(), 0);
    }

    #[test]
    fn our_downlink_cannot_evict_atcs_uplink_with_the_same_min() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/1//WU/CLIMB TO AND MAINTAIN FL240");
        assert_eq!(thread.pending_uplink_count(), 1);

        // Our first own message also gets MIN 1 — a request, unrelated.
        send(&mut thread, "DM22", &["UDROS"], None);
        assert_eq!(
            thread.pending_uplink_count(),
            1,
            "ATC still waits for the WILCO — our own MIN 1 must not erase it"
        );
        assert_eq!(
            thread.pending_response_count(),
            2,
            "both are genuinely outstanding, in opposite directions"
        );

        send(&mut thread, "DM0", &[], Some(1));
        assert_eq!(thread.pending_uplink_count(), 0);
        assert_eq!(
            thread.pending_response_count(),
            1,
            "our own request is still waiting on ATC"
        );
    }

    #[test]
    fn closing_marks_the_right_history_entry_when_mins_collide() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/1//WU/CLIMB TO AND MAINTAIN FL240");
        send(&mut thread, "DM22", &["UDROS"], None);
        send(&mut thread, "DM0", &[], Some(1));

        let uplink = thread
            .history()
            .iter()
            .find(|e| e.direction == Direction::Uplink && e.min == 1)
            .expect("uplink is in the history");
        assert!(uplink.closed, "the answered uplink must be marked closed");

        let ours = thread
            .history()
            .iter()
            .find(|e| e.direction == Direction::Downlink && e.min == 1)
            .expect("our request is in the history");
        assert!(
            !ours.closed,
            "our own unanswered request must NOT be ticked off by an unrelated reply"
        );
    }

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

    // ---- Handover: stale open uplinks must not survive it ----
    // Bug this replaces: a controller sends an instruction requiring a
    // reply, then hands over BEFORE the pilot answers. `mark_logged_off`
    // (the handover event) used to only touch the pending logon —
    // ATC's still-open uplink stayed in `open` forever (permanent
    // "you must reply" badge for an instruction nobody's waiting on),
    // and if the pilot replied anyway, `record_sent`'s `to_station`
    // would already be the NEW centre — the reply silently went to the
    // wrong controller with an MRN from the OLD one's numbering space.

    #[test]
    fn handover_supersedes_a_still_open_uplink_and_clears_the_badge() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/3//WU/CLIMB TO AND MAINTAIN FL350");
        assert_eq!(thread.pending_uplink_count(), 1);
        assert!(!thread.is_superseded_uplink(3));

        thread.mark_logged_off();

        assert_eq!(
            thread.pending_uplink_count(),
            0,
            "the badge must not keep asking the pilot to answer a controller who let go"
        );
        assert!(thread.is_superseded_uplink(3));
        let entry = thread
            .history()
            .iter()
            .find(|e| e.direction == Direction::Uplink && e.min == 3)
            .unwrap();
        assert!(entry.superseded);
        assert!(
            !entry.closed,
            "superseded is not the same as answered — it was never actually closed"
        );
    }

    #[test]
    fn second_handover_supersedes_the_new_stations_reused_min_not_the_stale_old_one() {
        // v0.20.x QS fix: mark_logged_off's own supersede loop used a
        // plain forward `.find()` — on a SECOND handover reusing a MIN
        // the FIRST handover already superseded, that returned the old,
        // already-superseded (no-op) entry instead of the new station's
        // now-actually-stale one, leaving it with `superseded: false`.
        // is_superseded_uplink (which correctly reverse-searches) would
        // then read the wrong entry as "not superseded" — letting a
        // reply through to the wrong (third) station.
        let mut thread = CpdlcThread::new();
        // Station A: uplink MIN 3, left open at handover 1.
        receive(&mut thread, "/data2/3//WU/CLIMB TO AND MAINTAIN FL240");
        thread.mark_logged_off(); // handover 1: A → B
        assert!(thread.is_superseded_uplink(3));

        // Station B reuses MIN 3 for an unrelated instruction, also
        // left open when handover 2 fires before the pilot answers.
        receive(&mut thread, "/data2/3//WU/DESCEND TO AND MAINTAIN FL180");
        assert!(
            !thread.is_superseded_uplink(3),
            "B's fresh MIN-3 entry must not inherit A's stale supersession"
        );
        thread.mark_logged_off(); // handover 2: B → C

        assert!(
            thread.is_superseded_uplink(3),
            "B's entry must now be superseded too — the pilot never answered it either"
        );
        assert_eq!(thread.pending_uplink_count(), 0);

        let mut uplinks_min_3 = thread
            .history()
            .iter()
            .filter(|e| e.direction == Direction::Uplink && e.min == 3);
        let from_a = uplinks_min_3.next().unwrap();
        let from_b = uplinks_min_3.next().unwrap();
        assert!(from_a.superseded, "A's entry: superseded by handover 1");
        assert!(from_b.superseded, "B's entry: must be superseded by handover 2, not left untouched");
    }

    #[test]
    fn a_reply_to_a_superseded_uplink_does_not_retroactively_mark_it_closed() {
        // Defense-in-depth: even if something upstream fails to block
        // the send (the UI is expected to), the protocol layer must not
        // let a stray reply repaint a superseded/misdirected message as
        // a legitimate, answered one in the audit trail.
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/3//WU/CLIMB TO AND MAINTAIN FL350");
        thread.mark_logged_off();

        send(&mut thread, "DM0", &[], Some(3));

        let entry = thread
            .history()
            .iter()
            .find(|e| e.direction == Direction::Uplink && e.min == 3)
            .unwrap();
        assert!(entry.superseded);
        assert!(!entry.closed, "must stay unanswered in the audit trail");
    }

    #[test]
    fn handover_does_not_touch_our_own_still_open_downlink_requests() {
        let mut thread = CpdlcThread::new();
        send(&mut thread, "DM22", &["UDROS"], None); // AnyRequired, stays open
        assert_eq!(thread.pending_response_count(), 1);

        thread.mark_logged_off();

        assert_eq!(
            thread.pending_response_count(),
            1,
            "our own outstanding request is not an uplink the pilot must answer — leave it alone"
        );
        let ours = thread
            .history()
            .iter()
            .find(|e| e.direction == Direction::Downlink)
            .unwrap();
        assert!(!ours.superseded);
    }

    #[test]
    fn handover_with_nothing_open_is_a_harmless_noop() {
        let mut thread = CpdlcThread::new();
        thread.mark_logged_off();
        assert_eq!(thread.pending_uplink_count(), 0);
        assert!(thread.history().is_empty());
    }

    #[test]
    fn is_superseded_uplink_is_false_for_a_normal_open_or_closed_uplink() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/3//WU/CLIMB TO AND MAINTAIN FL350");
        assert!(!thread.is_superseded_uplink(3), "still open, no handover happened");

        send(&mut thread, "DM0", &[], Some(3));
        assert!(!thread.is_superseded_uplink(3), "answered normally, not superseded");
    }

    // ---- MIN-collision across a handover ----
    // ATC renumbers MINs per station (often restarting at 1), so the
    // same (Uplink, min) key can legitimately recur after a handover:
    // an old, now-superseded message from the station that let go, and
    // a brand new, live one from whichever facility took over. Before
    // this fix, `mark_closed`/the STANDBY-defer lookup used a plain
    // `.find()`, which always returns the OLDER (first-in-history)
    // match — so replying to the NEW controller's message with the
    // reused MIN silently closed/deferred the OLD, unrelated one
    // instead, while the new one stayed open forever.

    #[test]
    fn handover_min_collision_closes_the_new_controllers_message_not_the_stale_one() {
        let mut thread = CpdlcThread::new();
        // Old controller's uplink, MIN 3, still open at handover time.
        receive(&mut thread, "/data2/3//WU/CLIMB TO AND MAINTAIN FL240");
        thread.mark_logged_off();
        assert!(thread.is_superseded_uplink(3));

        // New controller reuses MIN 3 for a completely unrelated uplink.
        receive(&mut thread, "/data2/3//WU/DESCEND TO AND MAINTAIN FL180");
        assert!(
            !thread.is_superseded_uplink(3),
            "the CURRENT MIN-3 entry is the new controller's live one, not the old superseded one"
        );
        assert_eq!(thread.pending_uplink_count(), 1);

        // Pilot replies WILCO, mrn=3 — must resolve to the new message.
        send(&mut thread, "DM0", &[], Some(3));
        assert_eq!(
            thread.pending_uplink_count(),
            0,
            "the new controller's instruction must actually get answered"
        );

        let mut uplinks_min_3 = thread
            .history()
            .iter()
            .filter(|e| e.direction == Direction::Uplink && e.min == 3);
        let old = uplinks_min_3.next().unwrap();
        let new = uplinks_min_3.next().unwrap();
        assert!(old.superseded && !old.closed, "old message stays superseded, never answered");
        assert!(!new.superseded && new.closed, "new message is the one that actually got the WILCO");
    }

    #[test]
    fn handover_min_collision_defers_the_new_controllers_message_via_standby() {
        let mut thread = CpdlcThread::new();
        receive(&mut thread, "/data2/3//WU/CLIMB TO AND MAINTAIN FL240");
        thread.mark_logged_off();
        receive(&mut thread, "/data2/3//WU/DESCEND TO AND MAINTAIN FL180");

        send(&mut thread, "DM2", &[], Some(3)); // STANDBY

        let mut uplinks_min_3 = thread
            .history()
            .iter()
            .filter(|e| e.direction == Direction::Uplink && e.min == 3);
        let old = uplinks_min_3.next().unwrap();
        let new = uplinks_min_3.next().unwrap();
        assert!(!old.deferred, "STANDBY must not touch the old, unrelated superseded message");
        assert!(new.deferred, "STANDBY must defer the new controller's actual instruction");
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
