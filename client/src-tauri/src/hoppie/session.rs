//! Single owner of "which station, in what state" — station identity and
//! its lifecycle, kept separate from `CpdlcThread` (the pure MIN/MRN
//! automaton in the `hoppie-protocol` crate, which stays exactly as it
//! was: wall-clock- and station-free, its own 74 tests untouched).
//!
//! QS rounds 3-5 (07.09.2026, #pdc-session-model) each found a real P1
//! caused by station/session identity being spread across four separate
//! pieces of state — `to_station: Arc<StdMutex<String>>`,
//! `CpdlcThread::logged_on`, `EndedSessions: Arc<StdMutex<HashSet<...>>>`,
//! `CpdlcThread::pending_logon_min` — updated at different times by
//! different call sites, with no single lock making an update to all of
//! them atomic:
//!   - A `LOGOFF` with no named successor left `to_station` pointed at
//!     the very station that just ended (by design — the UI needs the
//!     name), and later checks compared incoming traffic against that
//!     bare NAME, not against whether a session was actually live.
//!     Field-confirmed: MIN 9 `LOGOFF`, MIN 10 `WU` from the same,
//!     now-abandoned facility, waved straight through.
//!   - A logon ACCEPT was trusted from ANY sender, never checked against
//!     which station the outstanding request actually targeted — thread.rs
//!     deliberately has no station concept to check it against, and
//!     nothing in the wiring layer did either.
//!   - `to_station` moved to a new name the instant a logon REQUEST went
//!     out, before any acceptance — late traffic from that name's PREVIOUS,
//!     actually-ended session could regain unearned trust early.
//!   - Manual logoff updated `CpdlcThread` (network call pending) before
//!     `EndedSessions` (only updated after the network call returned) —
//!     a message arriving in between saw a thread that said "logged off"
//!     but a quarantine that didn't know yet.
//!   - HANDOVER/NEXT DATA AUTHORITY/END SERVICE authorization compared
//!     the sender only against the bare `to_station` name, the same gap
//!     as the abandoned-uplink check.
//!
//! `HoppieSession` closes all of these the same way: ONE struct, ONE lock
//! (`HoppieHandle::session`), covering the `CpdlcThread` automaton AND the
//! station/session state together — no operation here is "half done" from
//! another thread's point of view. Every call site that used to touch
//! `to_station`/`ended_sessions`/`next_data_authority`/`thread` now goes
//! through this one struct instead.

use std::collections::HashSet;

use hoppie_protocol::thread::CpdlcThread;

/// Case/whitespace-normalized station id — the same normalization this
/// module (and `poller.rs`'s `normalize_station`) already applies
/// everywhere a station name enters the app, so "ldzo " and "LDZO" are
/// never treated as two different facilities.
fn normalize(raw: &str) -> String {
    raw.trim().to_uppercase()
}

#[derive(Debug, Clone, PartialEq)]
enum Session {
    /// No station targeted right now — a fresh connect with nothing
    /// logged on yet, or a session that ended with nowhere automatic to
    /// go.
    None,
    /// A `REQUEST_LOGON` is outstanding for `station`, MIN `min`, not yet
    /// answered.
    Pending { station: String, min: u32 },
    /// `station` has accepted us. Only ever reached via
    /// [`HoppieSession::accept_logon`], which requires the accepting
    /// message to actually have come FROM `station`.
    Accepted { station: String },
}

pub(crate) struct HoppieSession {
    pub thread: CpdlcThread,
    session: Session,
    /// Every station we've had an ACCEPTED (or attempted) session with
    /// that has since ended. However many stations we've moved through
    /// (A -> B -> C), ALL of them stay quarantined here, not just the
    /// most recent — unlike the single `to_station` name this replaces,
    /// which could only ever "remember" one at a time.
    ended_stations: HashSet<String>,
    /// A GOLD `NEXT DATA AUTHORITY` name awaiting the `END SERVICE` that
    /// acts on it — see `poller::parse_next_data_authority`'s doc
    /// comment for why acting on it immediately would be premature.
    next_data_authority: Option<String>,
    /// The pilot's configured "usual" station (`HoppieSettings::station_id`
    /// at connect time) — shown/used as the send target before any logon
    /// has ever been attempted this connection. Once a logon is attempted
    /// (`begin_logon`), [`Self::addressee`] reports the live session
    /// instead and this value stops mattering until the session ends with
    /// nowhere automatic to go.
    default_station: String,
    /// Bumped by every call that changes what SHOULD be persisted as the
    /// crash-recovery "open session" marker (`begin_logon`, `accept_logon`,
    /// `cancel_pending`, `end_current`).
    ///
    /// QS round 9 (07.09.2026, #pdc-session-model, external QS follow-up):
    /// closes the remaining race in `set_open_session`/`clear_open_session`
    /// — the poller (on accept) and a Tauri command (on disconnect) are
    /// two genuinely parallel OS threads, each independently deciding to
    /// write or clear the SAME file, with no lock spanning both the
    /// decision and the eventual write (the write for a LOGOFF can only
    /// happen after a real network round trip confirms success, which is
    /// necessarily after any `HoppieSession` lock has been released — see
    /// `send_cpdlc_element`'s doc comment). "No `.await` between decision
    /// and write" does NOT establish mutual exclusion on a multi-threaded
    /// runtime — only a happens-before relationship the OS scheduler is
    /// free to violate at any machine instruction, including mid-syscall
    /// between `write` and `rename`. Before performing ANY file write for
    /// this marker, the writer re-locks `HoppieSession` and checks that
    /// this generation still matches the one captured when the write was
    /// decided — if a newer event (a different accept, a different
    /// logoff, a fresh begin_logon) has since run, the write is stale and
    /// skipped, because that newer event's OWN write/clear is now what's
    /// authoritative. Standard optimistic-concurrency-control pattern:
    /// shrinks the vulnerable window to the few instructions between the
    /// re-check and the write, rather than the entire decide-to-write
    /// span — the minimum achievable without holding one lock across a
    /// real network call, which is itself the strictly worse trade (it
    /// would conflict with keeping the marker clear ONLY on confirmed
    /// LOGOFF success — see that finding's own fix).
    persist_generation: u64,
}

impl HoppieSession {
    pub fn new(default_station: String) -> Self {
        let default_station = normalize(&default_station);
        Self {
            thread: CpdlcThread::new(),
            session: Session::None,
            ended_stations: HashSet::new(),
            next_data_authority: None,
            persist_generation: 0,
            // QS round 7 (07.09.2026, #pdc-session-model): the caller
            // (`settings.station_id`) normally defaults to `"SERVER"`
            // via serde — but only when the JSON key is MISSING
            // entirely; an explicitly present but blank/whitespace value
            // (a hand-edited `hoppie.json`, or any future caller that
            // skips validation) sails straight through serde's default
            // and would otherwise leave `addressee()` returning an empty
            // string before any logon is ever attempted — sent as-is on
            // the wire as `to=` (poll requests, an unsent telex with no
            // explicit recipient). Falling back to the same documented
            // placeholder settings.rs itself uses keeps this the ONE
            // place that has to know about the rule.
            default_station: if default_station.is_empty() {
                super::settings::DEFAULT_STATION_ID.to_string()
            } else {
                default_station
            },
        }
    }

    /// Seed a station as already-ended without ever having modeled a
    /// live session with it here — used at reconnect for whatever
    /// station `hoppie_session.json` names (the previous run crashed
    /// with a session open; see `hoppie_connect`'s stale-session
    /// cleanup). Its already-queued late traffic must be quarantined by
    /// the NEW process exactly as it would have been by the old one.
    pub fn seed_ended(&mut self, station: &str) {
        let station = normalize(station);
        if !station.is_empty() {
            self.ended_stations.insert(station);
        }
    }

    /// The station a `Pending` or `Accepted` session is with —
    /// deliberately `None` while no session is live, unlike
    /// [`Self::addressee`], which always returns a sendable string.
    pub fn live_station(&self) -> Option<String> {
        match &self.session {
            Session::None => None,
            Session::Pending { station, .. } | Session::Accepted { station } => {
                Some(station.clone())
            }
        }
    }

    /// Where an ordinary send with no more specific routing (a composer
    /// telex/free-text with no MRN to resolve a reply-to station from, or
    /// the UI's status display) should go: the live session's station
    /// when there is one, otherwise the pilot's configured default. Never
    /// falls back to a NAME that no longer has a live session behind it —
    /// exactly the class of bug this module exists to close — because
    /// once ANY logon has been attempted, [`Self::live_station`] (and
    /// therefore this) tracks that attempt, not a stale leftover name.
    pub fn addressee(&self) -> String {
        self.live_station()
            .unwrap_or_else(|| self.default_station.clone())
    }

    pub fn is_logged_on(&self) -> bool {
        matches!(self.session, Session::Accepted { .. })
    }

    pub fn is_logon_pending(&self) -> bool {
        matches!(self.session, Session::Pending { .. })
    }

    /// The MIN of our outstanding `REQUEST_LOGON`, if any — mirrors
    /// `CpdlcThread::pending_logon_min` for UI/timeout purposes (the
    /// value itself still lives in `CpdlcThread`, allocated by
    /// `record_sent`; this only tracks which STATION it was sent to, kept
    /// in lockstep by always calling [`Self::begin_logon`] at the same
    /// call site that allocates the MIN).
    pub fn pending_logon_min(&self) -> Option<u32> {
        match &self.session {
            Session::Pending { min, .. } => Some(*min),
            _ => None,
        }
    }

    /// Start pursuing a logon to `station` (MIN already allocated by the
    /// caller via `CpdlcThread::record_sent`, in the SAME lock hold —
    /// see `send_cpdlc_element`). Ends whatever session was current
    /// first (superseding its still-open uplinks and quarantining its
    /// station), so the old and new attempts can never be confused with
    /// each other, even when they name the same station (A -> B -> A).
    pub fn begin_logon(&mut self, station: &str, min: u32) {
        self.end_current();
        self.session = Session::Pending {
            station: normalize(station),
            min,
        };
        self.persist_generation += 1;
    }

    /// Undo a [`Self::begin_logon`] whose `REQUEST_LOGON` never made it
    /// onto the wire (mirrors `CpdlcThread::rollback_sent`, called
    /// alongside it) — back to `None`, NOT quarantined: a request that
    /// never sent was never a real, ended session, so there's nothing to
    /// remember it as abandoning.
    ///
    /// Also the right call for a genuine `UNABLE`/refusal reply: thread.rs
    /// itself already required the refusal's MRN to match our pending
    /// request before it would even report the refusal outcome, so by
    /// the time the caller gets here the correlation is already
    /// established — same non-quarantining "never happened" semantics
    /// apply.
    ///
    /// `min` must be the MIN of the SPECIFIC attempt being cancelled — a
    /// no-op unless it still matches the currently pending attempt.
    /// QS round 6 (07.09.2026, #pdc-session-model): both call sites
    /// (`send_cpdlc_element` in mod.rs, `send_logon` in poller.rs) call
    /// this from a failure branch AFTER an `await`ed HTTP round trip, and
    /// that HTTP send and a manual `hoppie_send_logon_request` (a
    /// completely separate Tokio task — the poller holds no lock that
    /// would serialize it against a Tauri command; only
    /// `Arc<StdMutex<HoppieSession>>` itself does) can genuinely
    /// interleave. Without the MIN check, a slow, ultimately-failing
    /// automatic handover attempt at station X could cancel a NEWER,
    /// still-live manual attempt at station Y that had already superseded
    /// X's `Pending` entry in the meantime — silently reverting the
    /// session to `None` and losing Y's otherwise-legitimate outstanding
    /// logon (its later `LOGON ACCEPTED` then has nothing to correlate
    /// against). An un-correlated `matches!(.., Pending { .. })` check —
    /// the first version of this method — cannot tell those two attempts
    /// apart; only the MIN can, since `begin_logon` always pairs a NEW MIN
    /// with a NEW attempt.
    pub fn cancel_pending(&mut self, min: u32) {
        if matches!(&self.session, Session::Pending { min: m, .. } if *m == min) {
            self.session = Session::None;
            self.persist_generation += 1;
        }
    }

    /// `station` has genuinely accepted us. The CALLER must have already
    /// verified the accepting message's sender IS `station` — this only
    /// checks that we were actually pending a logon to exactly that name
    /// right now (a stray/duplicate accept after we've already moved on
    /// is a no-op, returns `false`).
    pub fn accept_logon(&mut self, station: &str) -> bool {
        let station_norm = normalize(station);
        match &self.session {
            Session::Pending { station: s, .. } if s.eq_ignore_ascii_case(&station_norm) => {
                self.session = Session::Accepted { station: s.clone() };
                self.persist_generation += 1;
                true
            }
            _ => false,
        }
    }

    /// End whatever session is current (if any): supersedes any
    /// still-open uplinks (mirrors the old `CpdlcThread::mark_logged_off`
    /// — this now IS that operation, moved here since it's a session-
    /// level concept, not a pure-automaton one), quarantines the
    /// station, resets to no session, and clears any leftover `NEXT DATA
    /// AUTHORITY` name (it belongs to the session that just ended, never
    /// a later, unrelated one). Idempotent — safe to call even when
    /// `CpdlcThread` already reflects "logged off" (e.g. right after a
    /// manual `DM_LOGOFF`'s own `record_sent` branch already did the
    /// equivalent thread-level work; this only ADDS the quarantine step
    /// that used to happen later, separately, after the network round
    /// trip — see the module doc comment's "unprotected I/O window"
    /// finding).
    ///
    /// Returns the resulting [`Self::persist_generation`] — callers that
    /// need to gate a LATER file write (see that field's doc comment) on
    /// "has nothing superseded this specific end_current since" capture
    /// this value at the moment the ending happens, not after whatever
    /// network round trip follows.
    pub fn end_current(&mut self) -> u64 {
        if let Session::Pending { station, .. } | Session::Accepted { station } =
            std::mem::replace(&mut self.session, Session::None)
        {
            self.thread.mark_logged_off();
            self.ended_stations.insert(station);
            self.persist_generation += 1;
        }
        self.next_data_authority = None;
        self.persist_generation
    }

    /// Whether `station` is one we specifically remember having left —
    /// `false` while it's the CURRENT, ACCEPTED session (A -> B -> A
    /// re-acceptance clears the quarantine for A), `true` if it has an
    /// ended entry on record and isn't currently live.
    ///
    /// QS round 8 (07.09.2026, #pdc-session-model, external QS Finding 1):
    /// no longer the primary gate on incoming traffic — that's now
    /// [`Self::is_authorized_to_control`] (an ALLOWLIST: sender must be
    /// our current accepted station), since a station we've NEVER
    /// interacted with at all also isn't in `ended_stations` and this
    /// blocklist alone couldn't catch it either. Kept as a documented,
    /// narrower primitive and used at the one call site (`poller.rs`) to
    /// distinguish "a station we specifically remember having left" from
    /// "a station we have simply never spoken to" in the diagnostic log
    /// — a materially different threat profile for later incident
    /// analysis, even though both are rejected identically.
    pub fn is_abandoned(&self, station: &str) -> bool {
        let station = normalize(station);
        if station.is_empty() {
            // No real Hoppie envelope has a blank sender (see `wire.rs`'s
            // framing) — but never let one match a same-blank entry that
            // could, in principle, only exist here if something upstream
            // failed to reject an empty station name in the first place.
            return false;
        }
        let is_live = matches!(&self.session, Session::Accepted { station: s } if s == &station);
        !is_live && self.ended_stations.contains(&station)
    }

    /// Whether an uplink claiming to control our session (HANDOVER, NEXT
    /// DATA AUTHORITY, END SERVICE) — or, per round 8, an ORDINARY CPDLC
    /// instruction expecting a reply — may legitimately act. The sender
    /// must be the station we are actually, ACCEPTED-ly logged on to —
    /// comparing only against a bare name (the old `to_station`-based
    /// check) let a late message from an ABANDONED station whose name
    /// still happened to match trigger a brand new automatic handover.
    ///
    /// QS round 8 (07.09.2026, #pdc-session-model, external QS Finding 2):
    /// this used to also accept `Pending` — but a station we've merely
    /// SENT a request to, not yet accepted by, has not earned any
    /// operational control over the session yet. Field regression this
    /// closed: A -> B -> A, where a stale, queued `HANDOVER`/`END
    /// SERVICE` from the FIRST A session (still in flight when we left
    /// for B) could arrive after we've begun re-requesting A and — since
    /// the station NAME matches our new, not-yet-accepted `Pending{A}`
    /// attempt — get treated as authorized, ending our own outstanding
    /// re-logon before it ever had a chance to be accepted. Verifying a
    /// claimed LOGON ACCEPT/REFUSE itself needs a DIFFERENT, Pending-only
    /// check — see [`Self::is_authorized_to_answer_pending_logon`] —
    /// because that message IS what earns Accepted status, so it can't
    /// be gated on already having it.
    ///
    /// QS round 7 (07.09.2026, #pdc-session-model): explicitly refuses a
    /// blank sender, matching the discipline the function this replaced
    /// (`handover_sender_is_authorized`) already had. `HoppieSession::new`
    /// now guarantees `default_station` is never blank, so `station` here
    /// should never legitimately be empty either — this is defense in
    /// depth against a future caller that skips that guarantee, not a
    /// currently reachable gap.
    pub fn is_authorized_to_control(&self, from: &str) -> bool {
        let from = normalize(from);
        if from.is_empty() {
            return false;
        }
        matches!(&self.session, Session::Accepted { station } if station == &from)
    }

    /// Whether `from` is the station our CURRENTLY OUTSTANDING logon
    /// request was sent to — used ONLY to verify a claimed logon
    /// accept/refuse actually came from the station we asked, before
    /// `thread.rs`'s deliberately station-free `logon_outcome` is allowed
    /// to interpret it (see `poller.rs`'s `claims_our_logon_outcome`
    /// gate). Deliberately `Pending`-only, unlike
    /// [`Self::is_authorized_to_control`] (round 8, #pdc-session-model):
    /// a station that has only just been ASKED has not earned general
    /// operational control over the session — it has earned only the
    /// narrow right to answer the one question "did you accept us."
    pub fn is_authorized_to_answer_pending_logon(&self, from: &str) -> bool {
        let from = normalize(from);
        if from.is_empty() {
            return false;
        }
        matches!(&self.session, Session::Pending { station, .. } if station == &from)
    }

    /// Whether `msg` (sender `from`, reference `mrn`) could be the reply
    /// to our own outstanding logon request. Checked by SENDER matching
    /// the pending station — not only by MRN — because a real controller
    /// client routinely omits the MRN on a `LOGON ACCEPTED` (vSMR does;
    /// see `logon_outcome`'s doc comment in thread.rs), and an MRN-less
    /// accept must not be mistaken for abandoned-station traffic just
    /// because it carries no correlating reference. `logon_outcome`
    /// (thread.rs) still does the actual accept/refuse interpretation —
    /// this only decides whether the abandoned-station quarantine should
    /// step aside for a message that might be it. The STRICTER
    /// sender-only check for actually TRUSTING a claimed accept/refuse is
    /// [`Self::is_authorized_to_control`], used separately (see
    /// `poller.rs`) — this one stays permissive on purpose, matching by
    /// MRN alone even when the sender name looks wrong, because being
    /// wrongly exempted from the abandoned-station quarantine only means
    /// "processed as a normal message" (thread.rs decides on its own
    /// merits whether it's really an accept), while being wrongly caught
    /// BY the quarantine would hide the one message the pilot most needs
    /// to see: the answer to their own logon request.
    pub fn is_our_pending_logon_reply(&self, from: &str, mrn: Option<u32>) -> bool {
        let from = normalize(from);
        match &self.session {
            Session::Pending { station, min, .. } => station == &from || mrn == Some(*min),
            _ => false,
        }
    }

    pub fn take_next_data_authority(&mut self) -> Option<String> {
        self.next_data_authority.take()
    }

    pub fn set_next_data_authority(&mut self, station: String) {
        self.next_data_authority = Some(normalize(&station));
    }

    /// See [`Self::persist_generation`]'s doc comment (the field this
    /// exposes) for what this is for and why it exists.
    pub fn persist_generation(&self) -> u64 {
        self.persist_generation
    }

    /// Whether `self`'s current state should be persisted as the
    /// crash-recovery "open session" marker, given the transition the
    /// caller detected: `was_logged_on_before` is
    /// `self.thread.is_logged_on()` captured BEFORE `record_received`
    /// ran (see `poller.rs`'s call site). `Some` only on the exact
    /// moment we transition into being accepted — the returned station
    /// is `self.addressee()` read as part of THIS SAME call, atomically
    /// with the transition check. The paired generation is captured here
    /// too, for the caller to re-validate immediately before the actual
    /// file write (see [`Self::persist_generation`]'s doc comment).
    ///
    /// QS round 9 (07.09.2026, #pdc-session-model, external QS Finding
    /// 5): pulled out of `poller.rs` as its own named, pure method
    /// specifically so it's unit-testable without a `tauri::AppHandle` —
    /// the actual file write (`settings::set_open_session`) still needs
    /// one, but the DECISION of what to write no longer does. Before
    /// this, the equivalent inline logic read `self.addressee()` via a
    /// FRESH lock taken AFTER releasing the one that observed the
    /// transition — a window a concurrent logoff/handover could land in,
    /// persisting an already-ended or a not-yet-accepted station as if
    /// it were the one truly open. The generation added in the round-9
    /// follow-up closes the REMAINING window: even reading `addressee()`
    /// atomically with the transition check doesn't stop a *later*,
    /// independent write (this one) and a concurrent clear (a disconnect
    /// on another thread) from landing on disk in the wrong order — only
    /// re-validating immediately before the write does that.
    pub fn station_to_persist_on_accept(
        &self,
        was_logged_on_before: bool,
    ) -> Option<(String, u64)> {
        (!was_logged_on_before && self.is_logged_on())
            .then(|| (self.addressee(), self.persist_generation))
    }

    /// Decide what to do with an UNDECODABLE (raw-text, no `/data2/`
    /// header) uplink from `from` — vSMR's STANDBY / "UNABLE CALL ON
    /// FREQ" / logon-refusal convention (see `poller.rs`'s doc comment
    /// for the citation). `looks_like_a_refusal` is the CALLER's
    /// determination (see `poller.rs`'s text-matching helper) that the
    /// raw text is neither of the two known NON-refusal conventions —
    /// this function itself has no opinion on message content, only on
    /// session state. Returns `(superseded, cancelled_pending)`:
    ///
    /// - `superseded`: `true` unless `from` is authorized (our current
    ///   accepted station, or the reply to our own outstanding logon) —
    ///   the same allowlist as [`Self::is_authorized_to_control`]/
    ///   [`Self::is_our_pending_logon_reply`], since raw text carries no
    ///   MRN to correlate by.
    /// - `cancelled_pending`: `true` only if this raw text came
    ///   specifically FROM the station we are CURRENTLY pending a logon
    ///   with AND `looks_like_a_refusal` — mutates `self` accordingly via
    ///   [`Self::cancel_pending`] AND [`CpdlcThread::abandon_pending_logon`]
    ///   TOGETHER, atomically (same lock, same call), so the session-level
    ///   and automaton-level pending state can never observe each other
    ///   torn. Reverting a pending attempt that might still resolve is the
    ///   conservative failure direction (the pilot can simply retry) — the
    ///   opposite mistake, wrongly claiming acceptance, is what this whole
    ///   module exists to prevent.
    ///
    /// QS round 10 (07.09.2026, #pdc-session-model, external QS Finding
    /// 2): the FIRST version of this method treated ANY raw text from the
    /// pending station as a presumed refusal — including vSMR's two
    /// documented NON-refusal conventions, STANDBY and "UNABLE CALL ON
    /// FREQ" (a controller can legitimately send either as an interim ack
    /// to a just-sent logon request, before actually accepting or
    /// refusing it). It ALSO only called `Self::cancel_pending` — never
    /// `CpdlcThread::abandon_pending_logon` — so `self.thread` kept
    /// counting the same `REQUEST_LOGON` as outstanding
    /// (`pending_response_count`/`pending_logon_min`) even after the
    /// session level had already moved to `None`: the UI could report
    /// "no logon pending" and "an answer is still owed" at the same time.
    ///
    /// QS round 9 (07.09.2026, #pdc-session-model, external QS Finding
    /// 6): pulled out of `poller.rs` as its own named, pure(-ish —
    /// `&mut self`, no I/O) method for the same reason as
    /// [`Self::station_to_persist_on_accept`] — unit-testable without an
    /// `AppHandle`. Before THAT fix the undecodable-packet path bypassed
    /// the whole session model entirely: always `superseded: false`, and
    /// a raw logon refusal never cleared `Pending` at all.
    pub fn handle_undecodable_uplink(
        &mut self,
        from: &str,
        looks_like_a_refusal: bool,
    ) -> (bool, bool) {
        let is_our_pending_logon_reply = self.is_our_pending_logon_reply(from, None);
        let authorized = is_our_pending_logon_reply || self.is_authorized_to_control(from);
        // The SESSION-level mirror, not `self.thread.pending_logon_min()`
        // — this function only ever needs to reason about `self`'s own
        // state, and staying self-contained means it can't be broken by
        // `self.thread` momentarily lagging behind `self.session` (the
        // exact class of bug round 8/external-QS Finding found in
        // `poller.rs`'s `was_logged_on`/`now_logged_on` snapshots — this
        // unit test caught the same mistake here immediately, before it
        // ever reached production, by exercising `begin_logon` without
        // separately driving `self.thread.record_sent`, which real
        // callers always pair but a test in isolation need not).
        let cancelled = match self.pending_logon_min() {
            Some(pending_min) if is_our_pending_logon_reply && looks_like_a_refusal => {
                self.cancel_pending(pending_min);
                self.thread.abandon_pending_logon();
                true
            }
            _ => false,
        };
        (!authorized, cancelled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hoppie_protocol::{cpdlc, elements::Direction};

    /// Exercises the exact before/after-`record_received` pattern
    /// `poller.rs`'s `process_poll_payload` uses to detect a real logon
    /// acceptance and mirror it into the session-level `Accepted` state.
    ///
    /// QS round 6 (07.09.2026, #pdc-session-model): the first version of
    /// that call site snapshotted `HoppieSession::is_logged_on()` (the
    /// SESSION-level enum) before and after `record_received` instead of
    /// `session.thread.is_logged_on()` (the underlying automaton's own
    /// field, which `record_received` actually flips via
    /// `logon_outcome`). Nothing between the two snapshots ever touches
    /// the session-level enum — only `accept_logon` does, and THAT call
    /// was itself gated on the (always-false) session-level diff — so the
    /// condition was unreachable dead code: a real, correctly-sender-
    /// verified `LOGON ACCEPTED` flipped `CpdlcThread`'s own state
    /// perfectly well, but `HoppieSession` never found out, leaving
    /// `is_logged_on()` (and everything that reads it — `HoppieStatus`,
    /// the manual-logoff gate, the crash-recovery `hoppie_session.json`
    /// marker) stuck reporting "not logged on" forever. Caught by a fresh
    /// QS pass explicitly checking for unreachable code from the
    /// t→s rename, not by the two rounds before it (which reasoned about
    /// sender authorization, not about whether `accept_logon` was ever
    /// actually reached).
    #[test]
    fn a_real_logon_accept_is_only_detectable_via_the_threads_own_field_not_the_session_mirror() {
        let mut s = HoppieSession::new(String::new());
        let spec = hoppie_protocol::elements::find("DM_REQUEST_LOGON").unwrap();
        let resolved = hoppie_protocol::elements::resolve(spec, &[]).unwrap();
        let (message, _event) = s.thread.record_sent(
            spec.response,
            None,
            resolved.filled_text.clone(),
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        let min_value = message.min;
        s.begin_logon("LBSR", min_value);

        // The buggy pattern: snapshotting the SESSION-level mirror before
        // and after `record_received` never observes a difference,
        // because nothing in between touches `s.session`.
        let buggy_was = s.is_logged_on();
        let accepted = cpdlc::decode(
            &format!("/data2/1/{min_value}/NE/LOGON ACCEPTED"),
            Direction::Uplink,
        )
        .expect("well-formed LOGON ACCEPTED");
        s.thread.record_received(accepted);
        let buggy_now = s.is_logged_on();
        assert_eq!(
            buggy_was, buggy_now,
            "the session-level mirror alone can never detect the transition — \
             this is exactly why using it for the before/after snapshot was the bug"
        );
        assert!(
            !buggy_now,
            "and specifically it's stuck reporting false, even though the \
             thread itself was genuinely accepted"
        );

        // The correct pattern: the THREAD's own field reflects the
        // transition immediately, and the caller mirrors it into the
        // session level explicitly.
        assert!(
            s.thread.is_logged_on(),
            "CpdlcThread itself DID accept the logon — record_received worked fine"
        );
        assert!(s.accept_logon("LBSR"));
        assert!(
            s.is_logged_on(),
            "only after accept_logon is called does the session level catch up"
        );
    }

    #[test]
    fn a_fresh_session_has_no_live_station_but_addresses_the_configured_default() {
        let s = HoppieSession::new("EDDF_DEL".to_string());
        assert_eq!(s.live_station(), None);
        assert_eq!(s.addressee(), "EDDF_DEL");
        assert!(!s.is_logged_on());
        assert!(!s.is_logon_pending());
    }

    // --- QS round 7 (07.09.2026, #pdc-session-model): blank-station edge
    // cases found by an edge-case-focused QS pass. A hand-edited
    // `hoppie.json` with `"station_id": ""` (serde's own `#[serde(default)]`
    // only fires for a MISSING key, not an explicitly blank one) used to
    // reach `HoppieSession::new` unguarded.

    #[test]
    fn a_blank_default_station_falls_back_to_the_documented_placeholder() {
        let s = HoppieSession::new("".to_string());
        assert_eq!(s.addressee(), super::super::settings::DEFAULT_STATION_ID);
    }

    #[test]
    fn a_whitespace_only_default_station_also_falls_back() {
        let s = HoppieSession::new("   ".to_string());
        assert_eq!(s.addressee(), super::super::settings::DEFAULT_STATION_ID);
    }

    #[test]
    fn is_authorized_to_control_refuses_a_blank_sender_even_with_a_matching_blank_session() {
        // Defense in depth: even if something upstream failed to reject a
        // blank station (shouldn't happen — `new` no longer allows it),
        // an empty sender must never be treated as authorized.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("", 1);
        s.accept_logon("");
        assert!(!s.is_authorized_to_control(""));
    }

    #[test]
    fn is_authorized_to_answer_pending_logon_refuses_a_blank_sender() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("", 1);
        assert!(!s.is_authorized_to_answer_pending_logon(""));
    }

    #[test]
    fn is_abandoned_refuses_a_blank_station() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("", 1);
        s.end_current();
        assert!(!s.is_abandoned(""));
    }

    #[test]
    fn begin_logon_moves_to_pending_with_the_given_min_and_addresses_it() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        assert_eq!(s.live_station().as_deref(), Some("LRBB"));
        assert_eq!(s.addressee(), "LRBB");
        assert!(s.is_logon_pending());
        assert!(!s.is_logged_on());
        assert_eq!(s.pending_logon_min(), Some(4));
    }

    #[test]
    fn accept_logon_requires_the_matching_pending_station() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        assert!(!s.accept_logon("LBSR"), "wrong station must not accept");
        assert!(!s.is_logged_on());
        assert!(s.accept_logon("LRBB"));
        assert!(s.is_logged_on());
        assert!(!s.is_logon_pending());
    }

    #[test]
    fn accept_logon_is_case_and_whitespace_tolerant() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon(" lrbb ", 4);
        assert!(s.accept_logon("LRBB"));
        assert!(s.is_logged_on());
    }

    #[test]
    fn cancel_pending_goes_back_to_none_without_quarantining() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.cancel_pending(4);
        assert_eq!(s.live_station(), None);
        assert!(
            !s.is_abandoned("LRBB"),
            "a request that never sent, or a refusal, is not an ended session"
        );
    }

    #[test]
    fn cancel_pending_is_a_noop_once_accepted() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        s.cancel_pending(4);
        assert!(s.is_logged_on(), "must not undo a real acceptance");
    }

    #[test]
    fn cancel_pending_with_a_stale_min_does_not_touch_a_newer_pending_attempt() {
        // QS round 6 (07.09.2026, #pdc-session-model): the field bug this
        // closes. An automatic handover attempt at LBSR (min 4) is still
        // in flight (awaiting its HTTP response) when a manual logon
        // request to LRBB (min 7) supersedes it — begin_logon(LRBB, 7)
        // ends LBSR's Pending entry the same way any begin_logon does.
        // LBSR's HTTP send THEN fails and its caller calls
        // cancel_pending(4) — the stale MIN must be a no-op, not revert
        // the now-current LRBB attempt back to None.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 4);
        s.begin_logon("LRBB", 7); // supersedes LBSR's attempt
        s.cancel_pending(4); // LBSR's now-stale failure arrives late
        assert_eq!(
            s.live_station().as_deref(),
            Some("LRBB"),
            "a stale MIN must never cancel a newer, still-pending attempt"
        );
        assert!(s.is_logon_pending());
        assert!(s.accept_logon("LRBB"), "LRBB's real accept must still work");
    }

    #[test]
    fn cancel_pending_with_the_current_min_still_clears_it() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.cancel_pending(4);
        assert_eq!(s.live_station(), None);
        assert!(!s.is_logon_pending());
    }

    // --- field-confirmed regression: MIN 9 LOGOFF, MIN 10 WU, same station ---

    #[test]
    fn a_logoff_with_no_successor_leaves_the_station_abandoned_not_immune() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        s.end_current();
        assert_eq!(
            s.live_station(),
            None,
            "no session is live after a plain logoff"
        );
        assert!(
            s.is_abandoned("LRBB"),
            "field-confirmed regression: the SAME station's later traffic \
             must be flagged, not waved through because its name used to \
             match"
        );
    }

    #[test]
    fn a_matching_name_alone_grants_no_immunity_before_acceptance() {
        // Round 4's "target string authorized before logon acceptance"
        // finding: begin_logon() alone (request sent, not yet accepted)
        // must not immunize a PREVIOUSLY-ended session at the same name.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        s.end_current();
        s.begin_logon("LRBB", 9); // re-logon attempt, not yet accepted
        assert!(
            s.is_abandoned("LRBB"),
            "late traffic from the OLD, ended session must stay quarantined \
             until the NEW attempt is actually accepted"
        );
    }

    #[test]
    fn re_acceptance_of_a_previously_abandoned_station_clears_its_quarantine() {
        // A -> B -> A, and the second A is genuinely accepted this time.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 1);
        s.accept_logon("LBSR");
        s.end_current();
        s.begin_logon("LRBB", 2);
        s.accept_logon("LRBB");
        s.end_current();
        s.begin_logon("LBSR", 3);
        assert!(
            s.is_abandoned("LBSR"),
            "not accepted yet — still quarantined"
        );
        assert!(s.accept_logon("LBSR"));
        assert!(
            !s.is_abandoned("LBSR"),
            "round 5 regression: an MRN-less re-accept of a previously \
             abandoned station must clear its quarantine, not stay flagged"
        );
    }

    #[test]
    fn multiple_abandoned_stations_all_stay_quarantined_not_just_the_latest() {
        // A -> B -> C: round 5 finding — only remembering the LAST one
        // left A and B's late traffic unrecognized.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("A", 1);
        s.accept_logon("A");
        s.begin_logon("B", 2); // ends A's session
        s.accept_logon("B");
        s.begin_logon("C", 3); // ends B's session
        assert!(s.is_abandoned("A"));
        assert!(s.is_abandoned("B"));
    }

    #[test]
    fn is_our_pending_logon_reply_matches_by_sender_even_without_mrn() {
        // vSMR-style accept: no MRN at all. Round 5 regression — the
        // MRN-only check blocked exactly this, quarantining a legitimate
        // re-logon accept because the station was still in `ended`.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 7);
        assert!(s.is_our_pending_logon_reply("LBSR", None));
        assert!(!s.is_our_pending_logon_reply("LRBB", None));
    }

    #[test]
    fn is_our_pending_logon_reply_also_matches_by_mrn_from_a_different_looking_sender() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 7);
        assert!(s.is_our_pending_logon_reply("lbsr ", Some(7)));
    }

    #[test]
    fn is_our_pending_logon_reply_is_false_with_no_pending_logon() {
        let s = HoppieSession::new(String::new());
        assert!(!s.is_our_pending_logon_reply("LBSR", None));
    }

    #[test]
    fn is_our_pending_logon_reply_deliberately_trusts_mrn_alone_from_any_sender() {
        // QS round 10 (07.09.2026, #pdc-session-model, external QS
        // Finding 1): documents WHY the caller in `poller.rs` must gate
        // which `mrn` this function ever sees. This primitive itself, by
        // design, treats `station == from` OR `mrn == pending_min` as
        // sufficient — an UNRELATED sender ("XXXX") that merely knows or
        // guesses our pending logon's MIN (small, sequential, no
        // authentication on Hoppie's network) gets `true` here. That is
        // fine ONLY for messages that actually CLAIM to be
        // `LOGON ACCEPTED`/`UM0` (where thread.rs's own MRN correlation
        // is the intended signal) — `poller.rs` must never pass a real
        // `mrn` into this function for an ORDINARY instruction, exactly
        // because this primitive will not itself refuse it.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 7);
        assert!(
            s.is_our_pending_logon_reply("XXXX", Some(7)),
            "by design — the caller, not this function, must restrict \
             which messages are allowed to supply a real `mrn` here"
        );
    }

    // --- control-message / logon-outcome authorization ---

    #[test]
    fn control_authorization_requires_a_live_session_not_just_a_matching_name() {
        // Round 5: a late HANDOVER/END SERVICE (or a spoofed LOGON
        // ACCEPTED) from an ABANDONED station whose name still happened
        // to be the last one targeted must not be authorized to act.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        s.end_current();
        assert!(!s.is_authorized_to_control("LRBB"));
    }

    #[test]
    fn control_authorization_does_not_yet_extend_to_a_merely_pending_station() {
        // Round 8 (external QS Finding 2): a station we've only SENT a
        // request to has not earned operational control rights yet — a
        // stale, queued HANDOVER/END SERVICE from a PREVIOUS session at
        // this same name must not be able to act on our new, unrelated,
        // not-yet-accepted attempt.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        assert!(!s.is_authorized_to_control("LRBB"));
        assert!(s.is_authorized_to_answer_pending_logon("LRBB"));
    }

    #[test]
    fn control_authorization_works_once_accepted() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        assert!(s.is_authorized_to_control("LRBB"));
        assert!(
            !s.is_authorized_to_control("LBSR"),
            "round 5: a message from a DIFFERENT station than the one we \
             are actually logged on to must never be trusted"
        );
    }

    #[test]
    fn control_authorization_is_false_with_no_session_at_all() {
        let s = HoppieSession::new(String::new());
        assert!(!s.is_authorized_to_control("LRBB"));
    }

    #[test]
    fn answer_pending_logon_authorization_requires_the_matching_pending_station() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        assert!(s.is_authorized_to_answer_pending_logon("LRBB"));
        assert!(!s.is_authorized_to_answer_pending_logon("LBSR"));
    }

    #[test]
    fn answer_pending_logon_authorization_is_false_once_accepted() {
        // Once accepted, `is_authorized_to_control` is the right check —
        // this one is specifically for the STILL-outstanding request.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        assert!(!s.is_authorized_to_answer_pending_logon("LRBB"));
    }

    #[test]
    fn seed_ended_quarantines_without_ever_modeling_a_live_session() {
        let mut s = HoppieSession::new(String::new());
        s.seed_ended("LRBB");
        assert!(s.is_abandoned("LRBB"));
        assert_eq!(s.live_station(), None);
    }

    #[test]
    fn seed_ended_ignores_a_blank_station() {
        let mut s = HoppieSession::new(String::new());
        s.seed_ended("   ");
        assert!(!s.is_abandoned(""));
        assert!(!s.is_abandoned("   "));
    }

    #[test]
    fn next_data_authority_round_trips_and_take_clears_it() {
        let mut s = HoppieSession::new(String::new());
        assert_eq!(s.take_next_data_authority(), None);
        s.set_next_data_authority("lrbb".to_string());
        assert_eq!(s.take_next_data_authority().as_deref(), Some("LRBB"));
        assert_eq!(s.take_next_data_authority(), None);
    }

    #[test]
    fn ending_a_session_clears_next_data_authority_too() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 1);
        s.set_next_data_authority("LKPR".to_string());
        s.end_current();
        assert_eq!(s.take_next_data_authority(), None);
    }

    // --- station_to_persist_on_accept (round 9, external QS Finding 5) ---

    #[test]
    fn persists_the_station_exactly_on_the_accept_transition() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        assert_eq!(
            s.station_to_persist_on_accept(false),
            Some(("LRBB".to_string(), s.persist_generation())),
            "was_logged_on_before=false, now accepted -> this IS the transition"
        );
    }

    #[test]
    fn a_write_gated_on_a_stale_generation_must_be_skipped_by_the_caller() {
        // The exact scenario the external QS follow-up raised: the
        // poller decides to persist station A (capturing generation G),
        // but before it actually writes, a concurrent disconnect (a
        // logoff via begin_logon-of-nothing-new, or here simulated
        // directly via end_current) bumps the generation. The CALLER
        // (poller.rs) is responsible for re-checking
        // `persist_generation()` against the captured value immediately
        // before the write and skipping it on a mismatch — this test
        // documents that the generation value itself changes exactly
        // when it must for that check to catch the race.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        let (station, captured_generation) = s.station_to_persist_on_accept(false).unwrap();
        assert_eq!(station, "LRBB");
        // A concurrent event (disconnect) runs on the SAME HoppieSession
        // (same lock, different thread in production) before the poller
        // gets to its write.
        s.end_current();
        assert_ne!(
            s.persist_generation(),
            captured_generation,
            "end_current must bump the generation so a re-check catches this"
        );
    }

    #[test]
    fn a_stale_persist_decision_is_skipped_even_after_a_newer_ones_own_action_already_ran() {
        // QS round 10 (07.09.2026, #pdc-session-model, external QS
        // Finding 3): models BOTH directions of the exact race Codex
        // named — not just "the generation changed" (already covered
        // above), but that a decision captured EARLIER stays correctly
        // skippable even after a LATER, independent decision has already
        // completed its own action. This is what "check the generation
        // under the SAME lock hold as the write" (poller.rs/mod.rs,
        // fixed alongside this test) has to guarantee: two decisions
        // racing to touch the same marker must never both "win", and
        // specifically the OLDER one must lose to the NEWER one — not
        // whichever happens to reach the file system first.
        let mut s = HoppieSession::new(String::new());

        // Actor A (the poller): LBSR just got accepted — decides to
        // persist it, capturing the generation at that moment.
        s.begin_logon("LBSR", 1);
        s.accept_logon("LBSR");
        let (station_a, generation_a) = s.station_to_persist_on_accept(false).unwrap();
        assert_eq!(station_a, "LBSR");

        // Actor B (a manual station switch, on another thread in
        // production, same lock): moves on to LRBB entirely — this is
        // itself a real, independent, newer session event. It decides to
        // persist LRBB once accepted, under its OWN generation.
        s.begin_logon("LRBB", 2); // supersedes LBSR (end_current runs internally)
        s.accept_logon("LRBB");
        let (station_b, generation_b) = s.station_to_persist_on_accept(false).unwrap();
        assert_eq!(station_b, "LRBB");

        // B's action runs (under the SAME lock hold as its own check, in
        // production — modeled here as one atomic step since a
        // `std::sync::Mutex` guarantees exactly that exclusion once both
        // the check and the action are under one guard).
        let b_should_write = s.persist_generation() == generation_b;
        assert!(b_should_write, "B's own, still-current decision must win");

        // A's action FINALLY runs (e.g. its network round trip was
        // slower) — its captured generation is now doubly stale (B's
        // begin_logon AND accept_logon each bumped it further).
        let a_should_write = s.persist_generation() == generation_a;
        assert!(
            !a_should_write,
            "A's stale decision must be skipped even though B's newer \
             action already completed — writing LBSR now would silently \
             undo B's correct, more recent LRBB marker"
        );
    }

    #[test]
    fn does_not_persist_when_already_logged_on_before_this_call() {
        // Negative: the caller must pass the BEFORE snapshot — if it was
        // already true, this isn't a fresh transition, even though we
        // ARE currently accepted.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        assert_eq!(s.station_to_persist_on_accept(true), None);
    }

    #[test]
    fn does_not_persist_while_merely_pending() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        assert_eq!(s.station_to_persist_on_accept(false), None);
    }

    #[test]
    fn does_not_persist_with_no_session_at_all() {
        let s = HoppieSession::new(String::new());
        assert_eq!(s.station_to_persist_on_accept(false), None);
    }

    #[test]
    fn does_not_persist_once_the_session_has_already_ended() {
        // Guards the exact race external QS Finding 5 found: reading
        // CURRENT state (not a snapshot from before some other mutation
        // could have run) means a session that ended in between never
        // gets wrongly persisted as freshly accepted.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LRBB", 4);
        s.accept_logon("LRBB");
        s.end_current();
        assert_eq!(s.station_to_persist_on_accept(false), None);
    }

    // --- handle_undecodable_uplink (round 9, external QS Finding 6;
    // round 10, external QS Finding 2) ---

    #[test]
    fn undecodable_uplink_from_the_accepted_station_is_authorized_and_not_cancelled() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 1);
        s.accept_logon("LBSR");
        let (superseded, cancelled) = s.handle_undecodable_uplink("LBSR", true);
        assert!(!superseded, "an authorized sender must not be marked stale");
        assert!(
            !cancelled,
            "no logon is pending once accepted — nothing to cancel"
        );
    }

    #[test]
    fn undecodable_uplink_from_a_never_seen_station_is_untrusted() {
        // Negative: the exact scenario external QS Finding 1/6 raised —
        // a station we have NEVER interacted with at all (not even in
        // `ended_stations`) must still be rejected, not waved through
        // for lack of a blocklist entry.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 1);
        s.accept_logon("LBSR");
        let (superseded, cancelled) = s.handle_undecodable_uplink("XXXX", true);
        assert!(superseded, "an unrelated, never-seen station is untrusted");
        assert!(!cancelled);
    }

    #[test]
    fn undecodable_uplink_from_an_abandoned_station_is_untrusted() {
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 1);
        s.accept_logon("LBSR");
        s.end_current(); // LBSR is now abandoned
        let (superseded, cancelled) = s.handle_undecodable_uplink("LBSR", true);
        assert!(superseded);
        assert!(!cancelled);
    }

    #[test]
    fn undecodable_uplink_from_the_pending_station_is_treated_as_a_likely_refusal() {
        // vSMR sends a logon refusal in exactly this undecodable form,
        // with no structured spec_id for thread.rs to ever recognize.
        // `looks_like_a_refusal=true` is the caller's determination
        // (poller.rs) that the raw text is NEITHER of the two known
        // non-refusal conventions — see the next two tests for those.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 1);
        let (superseded, cancelled) = s.handle_undecodable_uplink("LBSR", true);
        assert!(!superseded, "still authorized — it IS the station we asked");
        assert!(
            cancelled,
            "treated as a likely refusal, so we stop waiting on it"
        );
        assert!(!s.is_logon_pending());
        assert_eq!(s.live_station(), None);
    }

    #[test]
    fn undecodable_uplink_from_the_pending_station_that_does_not_look_like_a_refusal_leaves_pending_alone(
    ) {
        // QS round 10 (external QS Finding 2): the field-confirmed gap —
        // a genuine `STANDBY` or "UNABLE CALL ON FREQ" ack from the
        // station we just asked (an entirely normal, documented vSMR
        // interim response, unrelated to accepting/refusing the logon)
        // must NOT abort our still-live attempt. `looks_like_a_refusal`
        // being `false` is exactly what the caller passes for those two
        // known texts.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 1);
        let (superseded, cancelled) = s.handle_undecodable_uplink("LBSR", false);
        assert!(!superseded, "still authorized");
        assert!(
            !cancelled,
            "a known non-refusal ack must not end a still-live pending attempt"
        );
        assert!(s.is_logon_pending(), "the attempt is still outstanding");
        assert_eq!(s.live_station().as_deref(), Some("LBSR"));
    }

    #[test]
    fn undecodable_uplink_treated_as_a_refusal_closes_both_the_session_and_the_thread_together() {
        // QS round 10 (external QS Finding 2, second half): the FIRST
        // version of this fix only called `cancel_pending` — never
        // `CpdlcThread::abandon_pending_logon` — so the automaton kept
        // counting the abandoned `REQUEST_LOGON` as outstanding even
        // after the session level had already moved to `None`. Verified
        // here against `s.thread` directly, not just the session-level
        // view, since that's exactly the layer the first version left
        // out of sync.
        let mut s = HoppieSession::new(String::new());
        let spec = hoppie_protocol::elements::find("DM_REQUEST_LOGON").unwrap();
        let resolved = hoppie_protocol::elements::resolve(spec, &[]).unwrap();
        let (message, _event) = s.thread.record_sent(
            spec.response,
            None,
            resolved.filled_text.clone(),
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        s.begin_logon("LBSR", message.min);
        assert_eq!(s.thread.pending_response_count(), 1, "logon is outstanding");
        assert!(s.thread.pending_logon_min().is_some());

        let (_, cancelled) = s.handle_undecodable_uplink("LBSR", true);
        assert!(cancelled);

        assert!(!s.is_logon_pending(), "session level: no logon pending");
        assert_eq!(
            s.thread.pending_response_count(),
            0,
            "automaton level must ALSO stop counting it — this is the exact \
             inconsistency (UI shows both \"nothing pending\" AND \"an \
             answer is owed\") the fix closes"
        );
        assert!(s.thread.pending_logon_min().is_none());
    }

    #[test]
    fn undecodable_uplink_does_not_cancel_a_pending_logon_at_a_different_station() {
        // Negative: raw text from an unrelated station must not disturb
        // an unrelated, still-live pending attempt.
        let mut s = HoppieSession::new(String::new());
        s.begin_logon("LBSR", 1);
        let (superseded, cancelled) = s.handle_undecodable_uplink("XXXX", true);
        assert!(superseded);
        assert!(!cancelled);
        assert!(
            s.is_logon_pending(),
            "the actual pending attempt (LBSR) must be untouched"
        );
    }
}
