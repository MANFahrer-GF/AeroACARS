//! Background poll loop for a running Hoppie ACARS connection.
//!
//! Modeled on `lib.rs`'s `spawn_position_streamer` (adaptive-interval
//! background task pattern) and `remote/mod.rs`'s `watch`-driven stop
//! signal, adapted for the simpler case of a plain polling loop with no
//! listener socket to release gracefully.

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tauri::AppHandle;
use tokio::sync::watch;

use hoppie_protocol::cpdlc;
use hoppie_protocol::elements::Direction;
use hoppie_protocol::wire::{self, HoppieRequest, HoppieResponseLine, PacketKind};

use super::{HistoryMeta, HoppieHttp, HoppieSession, MinMeta, TelexEntry};
use crate::{log_activity_handle, ActivityLevel};

/// The official docs' recommended idle band
/// (`hoppie.nl/acars/system/tech.html`): "heavily recommended to poll
/// once between every 45 and 75 seconds, randomly timed".
const BASELINE_POLL_MIN_SECS: u64 = 45;
const BASELINE_POLL_MAX_SECS: u64 = 75;

/// Pick a fresh interval inside the band on every tick. The "randomly
/// timed" part of the recommendation is not decoration: a fixed 60s
/// means every client that started together keeps polling together, so
/// the load arrives in a spike instead of spread out. Hoppie is run by
/// one volunteer.
///
/// Falls back to the band's midpoint if the OS entropy source refuses —
/// no worse than what we did before.
fn randomized_baseline() -> Duration {
    let mut byte = [0u8; 1];
    let span = BASELINE_POLL_MAX_SECS - BASELINE_POLL_MIN_SECS;
    let secs = match getrandom::getrandom(&mut byte) {
        Ok(()) => BASELINE_POLL_MIN_SECS + (byte[0] as u64 * span) / 255,
        Err(_) => (BASELINE_POLL_MIN_SECS + BASELINE_POLL_MAX_SECS) / 2,
    };
    Duration::from_secs(secs)
}

/// Faster cadence while a response is outstanding, per the docs ("you
/// may increase the polling rate to once per 20 seconds").
const FAST_POLL_SECS: u64 = 20;

/// Stable sort key for [`reorder_same_batch_envelopes`]: `false` (0)
/// sorts before `true` (1), i.e. informational messages that need no
/// reply move ahead of ones that do.
///
/// Field feedback 31.07.2026 (Thomas): logging on to Sofia Control, a
/// single poll delivered, in this exact order, MIN 8 `LOGON ACCEPTED`
/// (NE — no reply expected), MIN 9 `PROCEED DIRECT TO NIKTI` (WU —
/// needs WILCO/UNABLE), MIN 10 `CURRENT ATC UNIT ... SOFIA CTR` (also
/// no reply expected). That's the ground station's own send order —
/// AeroACARS doesn't choose it, it just appends uplinks as they arrive
/// (`CpdlcThread::record_received` → `history.push`). Displaying an
/// instruction ("proceed direct to X") before the pilot has even been
/// told which station issued it reads as backwards, even though it's
/// perfectly correct on the wire.
///
/// Anything that isn't a cleanly decodable CPDLC uplink (telex, a
/// GOLD-undecodable bare-text packet, a CPDLC packet this decoder can't
/// parse) sorts as `true` — the same bucket as "reply owed" — since we
/// have no response code to judge it by. Only messages positively
/// identified as "no reply owed" (`N`/`NE`, matching
/// [`hoppie_protocol::cpdlc::ResponseRequirement::requires_reply`]) sort
/// as `false` and move earlier.
///
/// This key ALONE does not guarantee an unclassifiable entry stays in
/// its original position — a stable sort only preserves order *within*
/// one key value, so `true` can still move relative to `false` items
/// around it (this bit the first version of this fix, see the "left
/// exactly where the ground station put it" reasoning debunked on
/// [`reorder_same_batch_envelopes`]). That guarantee comes from the
/// caller's `all_classifiable` check refusing to sort the batch at all
/// once any entry would sort as `true` — not from this function.
fn requires_reply_for_batch_order(env: &wire::InboundEnvelope) -> bool {
    if env.kind != PacketKind::Cpdlc {
        return true;
    }
    match cpdlc::decode(&env.packet, Direction::Uplink) {
        Ok(msg) => msg.response.requires_reply(),
        Err(_) => true,
    }
}

/// Reorders uplinks from ONE poll response so informational messages
/// (no reply owed — `LOGON ACCEPTED`, `CURRENT ATC UNIT`, ...) are
/// shown before instructional ones (reply owed — clearances, direct-tos,
/// frequency changes, ...) that arrived in the same batch.
///
/// Deliberately scoped to a single poll: this only reorders messages
/// the ground station sent close enough together that our one HTTP poll
/// caught them all at once. Messages arriving in different polls (i.e.
/// genuinely seconds/minutes apart) are never touched — they already
/// display in true arrival order, which is correct as-is.
///
/// [`Vec::sort_by_key`] is a stable sort, so within each of the two
/// groups (no-reply-owed / reply-owed) messages keep their original
/// relative order — this reorders GROUPS, it never reshuffles two
/// instructions or two informational messages against each other. The
/// actual WILCO/UNABLE reply linkage is MIN/MRN-based
/// ([`hoppie_protocol::thread::CpdlcThread`]), not screen position, so
/// moving an instruction later on screen cannot detach it from its
/// eventual reply.
///
/// Only runs when EVERY envelope in the batch is a cleanly decodable
/// CPDLC packet with a known response code, AND none of them is a
/// handover directive. Two independent reasons to bail out to the
/// untouched original order, both from real gaps found while testing
/// this fix rather than guessed at upfront:
///
/// - **Telex/undecodable entries.** First version sorted them as "reply
///   owed" (`true`) on the theory that they'd then just stay put
///   relative to instructions — wrong: a stable sort by a two-valued key
///   only preserves order *within* a key group, so a telex that arrived
///   BEFORE an NE/N message still gets pulled to AFTER it (both are just
///   "the `true` group" and "the `false` group" — the telex's true
///   original position is not part of the key at all). We have no
///   response-requirement code to judge telex or an undecodable packet
///   by, so rather than guess, leave the whole batch alone.
/// - **A handover or session-end directive anywhere in the batch.**
///   Unlike every other uplink here, a `HANDOVER <station>`, GOLD
///   `NEXT DATA AUTHORITY`/`END SERVICE`, or bare `LOGOFF` has real side
///   effects beyond the message log — see `poll_once`'s handling arms:
///   they end the current session (superseding whatever is still open)
///   and can fire a new `REQUEST LOGON` for the named successor, all
///   synchronously, before the loop moves on to the next envelope. Every
///   one of these typically carries a no-reply code, so without this
///   guard any of them would be a candidate for being pulled EARLIER in
///   the batch by the very sort this function does — changing not just
///   what the pilot sees but the actual order those side effects run in
///   relative to a reply-owed instruction in the SAME batch. QS round
///   06.09.2026: this guard originally only checked `parse_handover`,
///   which missed the three session-ending forms added alongside it —
///   an `END SERVICE` sorted ahead of a same-batch clearance would
///   supersede the thread before that clearance was even recorded,
///   letting it settle into `open` unsuperseded on the new session. That
///   risk costs nothing to close (batches mixing a session-end with
///   another uplink are already rare), so it's excluded outright rather
///   than reasoned to be "probably fine".
fn reorder_same_batch_envelopes(
    envelopes: Vec<wire::InboundEnvelope>,
) -> Vec<wire::InboundEnvelope> {
    let all_classifiable = envelopes.iter().all(|env| {
        if env.kind != PacketKind::Cpdlc {
            return false;
        }
        match cpdlc::decode(&env.packet, Direction::Uplink) {
            Ok(msg) => {
                parse_handover(&msg.element_text).is_none()
                    && parse_next_data_authority(&msg).is_none()
                    && !is_end_service(&msg)
            }
            Err(_) => false,
        }
    });
    if !all_classifiable {
        return envelopes;
    }
    let mut envelopes = envelopes;
    envelopes.sort_by_key(requires_reply_for_batch_order);
    envelopes
}

/// Pure — testable without tokio. Mirrors `lib.rs`'s
/// `adaptive_tick_interval` shape (a pure Duration-selection function
/// the loop calls each tick).
pub fn poll_interval(pending_response_count: usize) -> Duration {
    if pending_response_count > 0 {
        Duration::from_secs(FAST_POLL_SECS)
    } else {
        randomized_baseline()
    }
}

/// Spawn the poll loop. Runs until `stop_rx` flips to `true` (fired by
/// `HoppieHandle::drop`, i.e. `hoppie_disconnect` or app shutdown).
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    app: AppHandle,
    http: Arc<HoppieHttp>,
    session: Arc<StdMutex<HoppieSession>>,
    telex_log: Arc<StdMutex<Vec<TelexEntry>>>,
    min_meta: Arc<StdMutex<MinMeta>>,
    history_meta: Arc<StdMutex<HistoryMeta>>,
    last_error: Arc<StdMutex<Option<String>>>,
    from_callsign: String,
    logon: String,
    notify_os: bool,
    mut stop_rx: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        // The very first poll drains whatever the network queued while we
        // were away. That backlog is history, not news: it still lands in
        // the log, but it must not fire a chime or a toast per message —
        // otherwise starting the app after a long break means a burst of
        // notifications for messages that are long stale.
        let mut first_poll = true;
        loop {
            let interval = {
                let s = session.lock().expect("hoppie session mutex");
                poll_interval(s.thread.pending_response_count())
            };
            tokio::select! {
                res = stop_rx.changed() => {
                    if res.is_err() || *stop_rx.borrow() {
                        break;
                    }
                }
                _ = tokio::time::sleep(interval) => {
                    poll_once(&app, &http, &session, &telex_log, &min_meta, &history_meta, &last_error, &from_callsign, &logon, notify_os && !first_poll).await;
                    first_poll = false;
                }
            }
        }
        tracing::debug!("hoppie: poller stopped");
    });
}

/// Fire an OS-native toast for a newly-arrived message — visible even
/// when the app isn't focused/is minimized to tray, mirroring the
/// existing tray-mode notification pattern in `lib.rs` (PIREP-
/// cancelled-remotely). `body` deliberately omits the full message
/// text (OS notifications can be visible on a locked screen).
fn notify_new_message(app: &AppHandle, from: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title("AeroACARS — CPDLC")
        .body(format!("Neue Nachricht von {from}"))
        .show();
}

/// Clears the persisted open-session marker ONLY if `generation` still
/// matches the session's CURRENT one — see
/// `HoppieSession::persist_generation`'s doc comment for the full
/// reasoning. An unconditional clear right after `end_current` isn't
/// safe on this app's multi-threaded runtime: a concurrent event (a
/// fresh accept processed by a DIFFERENT poll cycle, or a manual command
/// on another OS thread) landing between `end_current` and this call
/// could already have written a newer, genuinely-valid marker that an
/// unconditional clear would wipe.
///
/// QS round 10 (07.09.2026, #pdc-session-model, external QS Finding 3):
/// the check and the file write happen under the SAME lock hold — the
/// FIRST version of this function re-locked, read the generation, and
/// let the guard drop BEFORE calling `clear_open_session`, which
/// reopened exactly the race the generation counter was meant to close:
/// the check and the write were two separate, unsynchronized moments
/// again, with the actual file I/O happening fully unlocked. Holding the
/// guard through the (tiny, local, synchronous — no network, no
/// `.await`) file write is what makes the compare-and-write atomic
/// against any other thread that also needs this same lock before
/// touching the marker file.
fn clear_marker_if_current(app: &AppHandle, session: &StdMutex<HoppieSession>, generation: u64) {
    let s = session.lock().expect("hoppie session mutex");
    if s.persist_generation() == generation {
        crate::hoppie::settings::clear_open_session(app);
    } else {
        tracing::debug!("hoppie: skipped clearing an already-superseded open-session marker");
    }
}

/// Extract the next facility from a `HANDOVER <ICAO>` uplink, which is
/// how the network transfers a CPDLC session between centres. Pure, so
/// the parsing is testable without a live connection.
fn parse_handover(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let next = trimmed.strip_prefix("HANDOVER")?;
    // v0.19.x FIX: require a separator after the literal word. Without
    // this, any free-text starting with the 8 characters "HANDOVER"
    // immediately followed by a token-looking string (e.g. a controller
    // remark "HANDOVEREDUU..." or coincidentally similar text) parsed as
    // a real handover to "EDUU" — silently logging the aircraft off a
    // real ATC session.
    if !next.starts_with(char::is_whitespace) {
        return None;
    }
    let next = next.trim();
    // Guard against a message that merely starts with the word (e.g. a
    // free-text remark) — a real handover carries exactly one token.
    let mut parts = next.split_whitespace();
    let station = parts.next()?;
    if parts.next().is_some() || station.is_empty() {
        return None;
    }
    Some(station.to_uppercase())
}

/// Extract the next facility from a GOLD `UM160 NEXT DATA AUTHORITY
/// <ICAO>` uplink — the standards-compliant counterpart to the
/// Hoppie-specific `HANDOVER <ICAO>` free-text convention above. Real
/// ATC units use this one; `HANDOVER` only ever appears from a Hoppie-
/// side client that made the convention up.
///
/// v1.7.20 (#pdc-cpdlc-session-end): unlike `HANDOVER`, this is
/// deliberately NOT treated as "switch now" — GOLD sends `NEXT DATA
/// AUTHORITY` as a heads-up well before the actual transfer, often while
/// the CURRENT facility is still very much in charge. Acting on it
/// immediately would log the aircraft off a session that hasn't ended
/// yet. The caller only remembers the name; `END SERVICE` (`UM161`,
/// below) is the actual transfer point.
fn parse_next_data_authority(msg: &cpdlc::CpdlcMessage) -> Option<String> {
    match &msg.parsed {
        hoppie_protocol::elements::ParsedElement::Recognized(r) if r.spec_id == "UM160" => {
            let raw = r.values.first()?;
            // QS round 06.09.2026: the generic placeholder resolver hands
            // back whatever text filled `@1` verbatim — it does not itself
            // check that a "single ICAO" placeholder actually IS a single
            // token. "NEXT DATA AUTHORITY LRBB EXTRA" must not be logged
            // on to as station "LRBB EXTRA" — same discipline `parse_handover`
            // already applies to its own free-text station token.
            let mut parts = raw.split_whitespace();
            let station = parts.next()?;
            if parts.next().is_some() || station.is_empty() {
                return None;
            }
            Some(station.to_uppercase())
        }
        _ => None,
    }
}

/// Whether this uplink is a session-end signal: the current facility
/// unilaterally ending the CPDLC session. End the session now, and if a
/// `NEXT DATA AUTHORITY` was named earlier, hand over to it exactly like
/// `HANDOVER` does; otherwise the pilot has no known next facility and
/// must log on manually.
///
/// Two forms recognized, both field-observed:
/// - GOLD `UM161 END SERVICE`, decoded and `Recognized`.
/// - A bare `LOGOFF` uplink — Hoppie has no uplink element for this in
///   the GOLD table at all (`DM_LOGOFF` is downlink-only, aircraft-to-
///   ground), so a ground system that mirrors the aircraft's own
///   convention back sends undecodable free text that falls through to
///   `Raw`. QS round 06.09.2026: this is documented as a real,
///   previously-seen case (`feedback` from the 25.07.2026 vSMR audit —
///   "ein unaufgeforderter LOGOFF matcht keinen Filter") that the UM161
///   check alone does not cover; missing it reproduces the exact
///   38-minute-open-uplink field incident this whole fix addresses.
fn is_end_service(msg: &cpdlc::CpdlcMessage) -> bool {
    match &msg.parsed {
        hoppie_protocol::elements::ParsedElement::Recognized(r) => r.spec_id == "UM161",
        hoppie_protocol::elements::ParsedElement::Raw(text) => {
            text.trim().eq_ignore_ascii_case("LOGOFF")
        }
    }
}

/// Whether `text` is one of vSMR's two documented, LEGITIMATE raw-text
/// conventions that are NOT a logon refusal — `STANDBY` and "UNABLE CALL
/// ON FREQ" (SMRPlugin.cpp, same citation as the undecodable-packet
/// handling this feeds). Either can arrive as an interim ack to a
/// just-sent `REQUEST_LOGON`, entirely unrelated to accepting or refusing
/// it — a controller pressing "standby" before actually looking at the
/// request is completely normal.
///
/// QS round 10 (07.09.2026, #pdc-session-model, external QS Finding 2):
/// exists because the alternative — treating EVERY undecodable text from
/// the pending station as a presumed refusal — wrongly aborted a
/// still-live logon attempt on exactly these two named, real, previously
/// field-observed message shapes. vSMR's actual refusal wording is not
/// documented anywhere this codebase can cite, so this is deliberately
/// an EXCLUSION list (positively known non-refusals), not a positive
/// identification of the refusal itself — whatever undecodable text
/// remains after excluding these two is presumed to be it, which is the
/// best available signal given the undocumented exact wording.
fn is_a_known_non_refusal_raw_text(text: &str) -> bool {
    let t = text.trim();
    t.eq_ignore_ascii_case("STANDBY") || t.eq_ignore_ascii_case("UNABLE CALL ON FREQ")
}

/// v0.19.x FIX: whether an inbound `HANDOVER <station>` directive should
/// actually be honored. Hoppie has no cryptographic sender verification —
/// any account can address a packet to any callsign — and this client
/// used to act on a HANDOVER regardless of who sent it, so any sender
/// could redirect a pilot's live CPDLC session to an arbitrary facility
/// (or off it) by simply naming their own outbound `from`. A real
/// handover always comes from the facility the pilot is CURRENTLY
/// connected to; requiring that match doesn't (and can't) close Hoppie's
/// network-level lack of authentication, but it does close the class of
/// stray/misrouted/spoofed packets that don't even bother pretending to
/// be the current controller — which is the entire realistic threat
/// surface a client-side check can actually reduce.
///
/// v1.7.21 (#pdc-session-model): this used to be its own pure function
/// comparing against the bare `to_station` name — QS round 5's Finding 4
/// caught that a late HANDOVER/NEXT DATA AUTHORITY/END SERVICE from an
/// ABANDONED station whose name still happened to match got authorized
/// anyway, the same gap the abandoned-uplink check had. Both are now the
/// SAME check — `HoppieSession::is_authorized_to_control`, which requires
/// an actually live (pending or accepted) session with that name, not
/// just a name match — called directly at each of the three sites below
/// (and, for the sharper version of the same class of bug, before
/// trusting a claimed logon accept/refuse — see `process_poll_payload`).
///
/// What to do about a same-MIN uplink collision across two different
/// stations — see [`resolve_min_collision`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MinCollisionResolution {
    /// No live collision (no prior entry for this MIN, same station as
    /// before, or the prior entry is already closed) — proceed normally.
    NoCollision,
    /// The prior, still-open entry belongs to the CURRENT station; the
    /// incoming message is the stale one and must be superseded instead.
    SupersedeIncoming,
    /// The incoming message belongs to the CURRENT station; the prior,
    /// still-open entry is the stale one and must be superseded so the
    /// incoming message can take the slot normally.
    SupersedeExisting,
}

/// Decide which side of a same-MIN, cross-station uplink collision is
/// stale, using `current_station` (`HoppieSession::addressee`) as the
/// tie-breaker.
///
/// QS round 06.09.2026: Hoppie's poll model gives no cross-station
/// delivery-order guarantee — only per-station MIN numbering. A message
/// station A queued for us can still be sitting unpolled when we move on
/// to station B; if B independently reuses the same MIN number (common,
/// since most facilities start counting near 1) before A's message
/// finally arrives, blindly treating "most recently processed" as
/// authoritative — the previous behavior — let the STALE message
/// overwrite `min_meta`'s recorded station for a still-open entry the
/// pilot might be about to answer. The reply would then go out addressed
/// to whichever station was processed last, not the one actually asking.
///
/// Pure and side-effect-free so it's testable without a live
/// thread/HTTP/AppHandle, matching every other decision function in this
/// file (`parse_handover`, `is_end_service`, ...) — the call site in
/// `process_poll_payload` only executes what this returns.
fn resolve_min_collision(
    existing_station: Option<&str>,
    existing_is_open: bool,
    incoming_station: &str,
    current_station: &str,
) -> MinCollisionResolution {
    let Some(existing_station) = existing_station else {
        return MinCollisionResolution::NoCollision;
    };
    // Defensive trim, same discipline as `normalize_station` — every
    // production caller already passes normalized (trimmed+uppercased)
    // station strings, but comparing case/whitespace-tolerantly here too
    // costs nothing and matches this file's established discipline.
    let existing_station = existing_station.trim();
    let incoming_station = incoming_station.trim();
    let current_station = current_station.trim();
    if !existing_is_open || existing_station.eq_ignore_ascii_case(incoming_station) {
        return MinCollisionResolution::NoCollision;
    }
    let existing_is_current = existing_station.eq_ignore_ascii_case(current_station);
    let incoming_is_current = incoming_station.eq_ignore_ascii_case(current_station);
    match (existing_is_current, incoming_is_current) {
        (true, false) => MinCollisionResolution::SupersedeIncoming,
        (false, true) => MinCollisionResolution::SupersedeExisting,
        // Neither matches (two unrelated delivery desks) or, in theory,
        // both do (impossible — they'd be equal and never reach here) —
        // no authority signal to prefer one over the other either way.
        _ => MinCollisionResolution::NoCollision,
    }
}

/// External QS (Codex, 15.09.2026) P1: an incoming uplink whose wire MIN
/// is held by an OPEN uplink of a DIFFERENT station that is itself still a
/// live authority (our accepted station or an invited PDC desk) must not
/// displace it — both are valid, current instructions. The caller then
/// files the incoming one under a wiring-internal MIN. Collisions with a
/// station that is no longer live keep going through
/// [`resolve_min_collision`] unchanged.
fn needs_min_remap(
    existing_station: Option<&str>,
    existing_is_open: bool,
    existing_is_live: bool,
    incoming_station: &str,
) -> bool {
    existing_is_open
        && existing_is_live
        && existing_station
            .is_some_and(|st| !st.trim().eq_ignore_ascii_case(incoming_station.trim()))
}

/// Whether an OPEN uplink holding a colliding MIN must be kept rather
/// than handed to [`resolve_min_collision`]'s supersede decision: it is
/// from a current authority, it is an open PDC clearance (session-
/// independent, even past its invitation window), or the supersede logic
/// would pick neither side — which in the thread means silently
/// overwriting the open slot and misaddressing its reply.
fn holder_is_protected(
    holder_is_live_authority: bool,
    holder_is_session_independent: bool,
    fallback: MinCollisionResolution,
) -> bool {
    holder_is_live_authority
        || holder_is_session_independent
        || fallback == MinCollisionResolution::NoCollision
}

/// The MRN a sender trusted only via a PDC invitation may use: only one
/// that references a downlink WE addressed to that same station.
fn mrn_usable_for_sender(
    meta: &crate::hoppie::MinMeta,
    mrn: Option<u32>,
    sender: &str,
) -> Option<u32> {
    mrn.filter(|m| {
        meta.get(&(false, *m))
            .is_some_and(|x| x.station.trim().eq_ignore_ascii_case(sender.trim()))
    })
}

/// Station ids on the wire are compared case-insensitively everywhere
/// else in this module, but a RECORDED station is also used as a send
/// address later. Normalizing once, where it enters the app, keeps "ldzo "
/// and "LDZO" from becoming two different addressees.
fn normalize_station(raw: &str) -> String {
    raw.trim().to_uppercase()
}

/// Fire `REQUEST LOGON` at `station` as part of an automatic handover.
/// Best-effort: a failure here leaves the pilot on the old facility with
/// a visible "not logged on" state rather than silently pretending.
async fn send_logon(
    http: &HoppieHttp,
    session: &StdMutex<HoppieSession>,
    min_meta: &StdMutex<MinMeta>,
    from_callsign: &str,
    logon: &str,
    station: &str,
) {
    let Some(spec) = hoppie_protocol::elements::find("DM_REQUEST_LOGON") else {
        return;
    };
    let Ok(resolved) = hoppie_protocol::elements::resolve(spec, &[]) else {
        return;
    };
    let (message, min) = {
        let mut s = session.lock().expect("hoppie session mutex");
        let (message, _) = s.thread.record_sent(
            spec.response,
            None,
            resolved.filled_text.clone(),
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        let min = message.min;
        // v1.7.21 (#pdc-session-model): started tracking the new pending
        // attempt HERE, atomically with the MIN allocation — same
        // discipline as `send_cpdlc_element` in mod.rs (see its doc
        // comment for why this used to be a separate, non-atomic step).
        s.begin_logon(station, min);
        (message, min)
    };
    min_meta.lock().expect("hoppie min_meta mutex").insert(
        (false, min),
        crate::hoppie::MsgMeta {
            at: chrono::Utc::now(),
            station: station.to_string(),
            wire_min: None,
        },
    );

    let req = HoppieRequest {
        logon: logon.to_string(),
        from: from_callsign.to_string(),
        to: station.to_string(),
        kind: PacketKind::Cpdlc,
        packet: Some(cpdlc::encode(&message)),
    };
    // QS 19.08.2026, two findings in three lines:
    //   1. A protocol-level rejection ("error {invalid logon code}") is
    //      an Ok(...) here — this branch logged "handed over" for a
    //      request the network had refused.
    //   2. Either way the downlink stayed recorded as sent. That keeps
    //      an unanswerable logon open forever, and `poll_interval` reads
    //      exactly that count: the poller would sit at its 20s "reply
    //      outstanding" cadence for the rest of the session, against a
    //      free, volunteer-run service that asks for 45-75s.
    let failure = match http.send(&req).await {
        Err(e) => Some(e.message),
        Ok(hoppie_protocol::wire::HoppieResponseLine::Error(reason)) => Some(reason),
        Ok(_) => None,
    };
    match failure {
        Some(reason) => {
            let mut s = session.lock().expect("hoppie session mutex");
            s.thread.rollback_sent(min);
            // Mirrors `CpdlcThread::rollback_sent` undoing its own
            // `logon_request_min` — a request that never sent was never
            // a real attempt, so it's not a "session that ended" either.
            // MIN-correlated (QS round 6) — a newer, still-live attempt
            // that superseded this one while the HTTP send was in
            // flight must not be cancelled by this failure.
            s.cancel_pending(min);
            drop(s);
            min_meta
                .lock()
                .expect("hoppie min_meta mutex")
                .remove(&(false, min));
            tracing::warn!(error = %reason, station = %station, "hoppie: handover logon failed");
        }
        None => tracing::info!(station = %station, "hoppie: handed over to next facility"),
    }
}

/// Everything `poll_once` does with an already-fetched poll response
/// body — split out so it can be exercised in a test with a canned
/// `content` string instead of a live HTTP round-trip. `HoppieHttp`
/// hits Hoppie's real server directly with no injection seam, so a true
/// end-to-end test of `poll_once` itself (including the network fetch)
/// isn't possible without either standing up a mock server or making
/// the base URL configurable — out of proportion for this fix. This is
/// the next best thing: everything downstream of "we have the poll
/// body" — parsing, forensic logging, the reorder this fix adds,
/// `CpdlcThread` mutation, telex logging, notifications — runs exactly
/// as it does in production, verified against `thread.history()` (what
/// the pilot's message log actually reads from), not a stand-in for it.
#[allow(clippy::too_many_arguments)]
async fn process_poll_payload(
    app: &AppHandle,
    http: &HoppieHttp,
    content: &str,
    session: &StdMutex<HoppieSession>,
    telex_log: &StdMutex<Vec<TelexEntry>>,
    min_meta: &StdMutex<MinMeta>,
    history_meta: &StdMutex<HistoryMeta>,
    from_callsign: &str,
    logon: &str,
    // False on the first poll of a session — see `spawn`'s `first_poll`.
    notify_os: bool,
) {
    let envelopes = wire::parse_poll_envelopes(content);
    // Forensic record of EVERY inbound message, in TRUE wire
    // arrival order — a separate pass, deliberately BEFORE the
    // display reorder below. `reorder_same_batch_envelopes`
    // changes what order the PILOT sees messages in; it must
    // never change what order we tell the forensic log they
    // arrived in, or the one record meant to reconstruct exactly
    // what happened over the wire (the arm that matters most for
    // fault-finding is the undecodable one — vSMR's bare-text
    // STANDBY and logon refusals) would itself become unreliable.
    // Decoding twice for the metadata is cheap — one poll carries
    // a handful of messages at most, minutes apart.
    for env in &envelopes {
        // Same reason as the downlink lines in mod.rs: the rotating log
        // file is the only record that survives a restart, and "which
        // station actually sent this" is the question that could not be
        // answered after the LROP clearance was cancelled.
        tracing::info!(
            from = %env.from,
            kind = %env.kind.as_wire_str(),
            "hoppie: Uplink empfangen"
        );
        let decoded = (env.kind == PacketKind::Cpdlc)
            .then(|| cpdlc::decode(&env.packet, Direction::Uplink).ok())
            .flatten();
        crate::record_datalink(
            app,
            "uplink",
            match env.kind {
                PacketKind::Cpdlc => "cpdlc",
                _ => "telex",
            },
            Some(env.from.clone()),
            decoded.as_ref().map(|m| m.min),
            decoded.as_ref().and_then(|m| m.mrn),
            decoded.as_ref().map(|m| m.response.code().to_string()),
            env.packet.clone(),
        );
    }
    let envelopes = reorder_same_batch_envelopes(envelopes);
    for env in envelopes {
        if env.kind != PacketKind::Cpdlc {
            // Telex traffic (PDC replies, free chat) — no MIN/MRN
            // threading, just appended in arrival order.
            let from = env.from.clone();
            telex_log
                .lock()
                .expect("hoppie telex_log mutex")
                .push(TelexEntry {
                    direction: "received",
                    text: env.packet,
                    at: chrono::Utc::now(),
                    station: normalize_station(&from),
                    from_cpdlc_channel: false,
                    superseded: false,
                });
            if notify_os {
                notify_new_message(app, &from);
            }
            continue;
        }
        match cpdlc::decode(&env.packet, Direction::Uplink) {
            Ok(mut msg) => {
                // Sector handover: the current centre names the one
                // taking over and we silently log on there. The pilot
                // never acts on this — it's protocol bookkeeping, not
                // an instruction, so it stays out of the message log.
                if let Some(next) = parse_handover(&msg.element_text) {
                    let (authorized, current_station) = {
                        let s = session.lock().expect("hoppie session mutex");
                        (s.is_authorized_to_control(&env.from), s.addressee())
                    };
                    if !authorized {
                        // v0.19.x FIX: don't act on a HANDOVER from anyone
                        // other than the facility we're currently
                        // pending/connected to — see
                        // `HoppieSession::is_authorized_to_control`'s doc
                        // comment. Fall through to normal message handling
                        // below instead of `continue`ing, so the pilot
                        // still sees the odd uplink rather than it vanishing.
                        tracing::warn!(
                            claimed_next = %next,
                            from = %env.from,
                            expected = %current_station,
                            "hoppie: ignoring HANDOVER from unexpected sender"
                        );
                        log_activity_handle(
                            app,
                            ActivityLevel::Warn,
                            format!(
                                "CPDLC: HANDOVER-Anfrage an {next} von unerwartetem Absender \
                                 ({from}) ignoriert",
                                from = env.from
                            ),
                            Some(format!(
                                "Erwartet wurde die aktuell verbundene Stelle ({current_station}). \
                                 Die Übergabe wurde NICHT ausgeführt."
                            )),
                        );
                    } else {
                        log_activity_handle(
                            app,
                            ActivityLevel::Info,
                            format!("CPDLC: Übergabe an {next}"),
                            None,
                        );
                        // The old centre has let go; the new one has
                        // not accepted yet. Leaving the session `Accepted`
                        // made the header claim "connected <new centre>"
                        // from this instant on, even if that centre never
                        // answers — the pilot would believe they have a
                        // datalink they don't. `end_current` quarantines
                        // `current_station` and clears any leftover NEXT
                        // DATA AUTHORITY atomically with the thread
                        // mutation — a mixed-convention sequence (UM160
                        // from one facility, a Hoppie free-text HANDOVER
                        // from another) must not carry a stale name
                        // forward.
                        let generation =
                            session.lock().expect("hoppie session mutex").end_current();
                        clear_marker_if_current(app, session, generation);
                        send_logon(http, session, min_meta, from_callsign, logon, &next).await;
                        continue;
                    }
                }
                // v1.7.20 (#pdc-cpdlc-session-end): the GOLD-compliant
                // counterpart to the Hoppie-specific HANDOVER convention
                // above. `NEXT DATA AUTHORITY` only announces who's next —
                // remembered for later, not acted on now (see
                // `parse_next_data_authority`'s doc comment for why acting
                // immediately would be premature). Same sender check as
                // HANDOVER: only the facility we're currently connected to
                // may name our next one.
                if let Some(next) = parse_next_data_authority(&msg) {
                    let mut s = session.lock().expect("hoppie session mutex");
                    if s.is_authorized_to_control(&env.from) {
                        s.set_next_data_authority(next.clone());
                        tracing::info!(
                            next = %next,
                            from = %env.from,
                            "hoppie: NEXT DATA AUTHORITY noted, awaiting END SERVICE"
                        );
                        continue;
                    }
                    let current_station = s.addressee();
                    drop(s);
                    tracing::warn!(
                        claimed_next = %next,
                        from = %env.from,
                        expected = %current_station,
                        "hoppie: ignoring NEXT DATA AUTHORITY from unexpected sender"
                    );
                    // Fall through — the pilot still sees the raw uplink.
                }
                // v1.7.20 (#pdc-cpdlc-session-end): GOLD's real transfer
                // point — unlike NEXT DATA AUTHORITY, this ends the
                // session NOW. If a next facility was named earlier, hand
                // over to it exactly like HANDOVER does; otherwise there
                // is nowhere automatic to go, so end the session and tell
                // the pilot to log on by hand — silently doing nothing
                // here is what left LBSR's last instruction open for 38
                // minutes on 06.09.2026 (LOT4TK, LBSR→LRBB).
                if is_end_service(&msg) {
                    let (authorized, current_station) = {
                        let s = session.lock().expect("hoppie session mutex");
                        (s.is_authorized_to_control(&env.from), s.addressee())
                    };
                    if authorized {
                        let (next, generation) = {
                            let mut s = session.lock().expect("hoppie session mutex");
                            let next = s.take_next_data_authority();
                            let generation = s.end_current();
                            (next, generation)
                        };
                        clear_marker_if_current(app, session, generation);
                        match next {
                            Some(next) => {
                                log_activity_handle(
                                    app,
                                    ActivityLevel::Info,
                                    format!(
                                        "CPDLC: {current_station} hat den Dienst beendet — \
                                         Übergabe an {next}"
                                    ),
                                    None,
                                );
                                send_logon(http, session, min_meta, from_callsign, logon, &next)
                                    .await;
                                continue;
                            }
                            None => {
                                log_activity_handle(
                                    app,
                                    ActivityLevel::Warn,
                                    format!(
                                        "CPDLC: {current_station} hat den Dienst beendet (END SERVICE)"
                                    ),
                                    Some(
                                        "Keine Nachfolgestation genannt — bitte manuell bei der \
                                         nächsten zuständigen Stelle anmelden."
                                            .to_string(),
                                    ),
                                );
                                // No known next facility — fall through so
                                // the raw END SERVICE still reaches the
                                // pilot's message log, unlike HANDOVER's
                                // silent bookkeeping above.
                            }
                        }
                    } else {
                        tracing::warn!(
                            from = %env.from,
                            expected = %current_station,
                            "hoppie: ignoring END SERVICE from unexpected sender"
                        );
                    }
                }
                let mut min = msg.min;
                let wire_min = msg.min;
                let this_station = normalize_station(&env.from);
                // QS round 4 (07.09.2026): `s` locked HERE, once, and kept
                // for the abandoned-station check below too — that check
                // needs `is_logged_on()`/pending-logon state, and reading
                // those through a SEPARATE, since-released lock would
                // reopen exactly the kind of race this whole file's
                // locking discipline exists to close. `session` now covers
                // the thread automaton too, so this is ALSO what makes the
                // session-identity bookkeeping further down (accept/
                // cancel/quarantine) atomic with the thread mutation
                // itself — see QS round 5 (07.09.2026, #pdc-session-model)
                // Finding 3.
                let mut s = session.lock().expect("hoppie session mutex");
                let current_station = s.addressee();
                // QS round 5 Finding 1: `logon_outcome` (thread.rs) is
                // deliberately station-free — it accepts a
                // `LOGON ACCEPTED` from ANY sender the instant SOME logon
                // is outstanding, and accepts a `UM0` refusal whenever its
                // MRN merely matches. Neither check is sender-aware; that
                // is by design (`hoppie-protocol` stays pure/station-
                // free), so the wiring layer — which DOES know which
                // station we actually asked — has to gate it. A claimed
                // accept/refuse whose sender does NOT match our pending
                // station never reaches `record_received` at all: same
                // "recorded as a superseded/untrusted entry, never as a
                // fresh instruction" treatment as an abandoned-station
                // uplink, since it's exactly as untrustworthy.
                let claims_our_logon_outcome = matches!(
                    &msg.parsed,
                    hoppie_protocol::elements::ParsedElement::Recognized(r)
                        if r.spec_id == "UM_LOGON_ACCEPTED" || r.spec_id == "UM0"
                );
                if claims_our_logon_outcome
                    && s.thread.pending_logon_min().is_some()
                    && !s.is_authorized_to_answer_pending_logon(&env.from)
                {
                    tracing::warn!(
                        min,
                        claimed_from = %this_station,
                        expected = %current_station,
                        "hoppie: ignoring a claimed logon accept/refuse from an unexpected sender"
                    );
                    drop(s);
                    telex_log
                        .lock()
                        .expect("hoppie telex_log mutex")
                        .push(TelexEntry {
                            direction: "received",
                            text: msg.element_text.clone(),
                            at: chrono::Utc::now(),
                            station: this_station,
                            from_cpdlc_channel: true,
                            superseded: true,
                        });
                    if notify_os {
                        notify_new_message(app, &env.from);
                    }
                    continue;
                }
                // QS round 8 (07.09.2026, #pdc-session-model, external QS
                // Finding 1): this used to be a BLOCKLIST — reject only
                // stations we specifically remember having abandoned
                // (`is_abandoned`). That structurally can't catch a
                // station we have NEVER interacted with at all: it was
                // never `Pending`, never `Accepted`, never quarantined,
                // so it isn't in `ended_stations` either — and sailed
                // straight through as an ordinary, fresh, reply-required
                // instruction, with active reply buttons the pilot could
                // press. GOLD CPDLC never sends an operational element
                // before `LOGON ACCEPTED` — a real controller only ever
                // talks to us once we're actually logged on — so the
                // correct test is an ALLOWLIST: the sender must either be
                // answering our own outstanding logon, or be the station
                // we are ACTUALLY, ACCEPTED-ly logged on to right now.
                // This allowlist subsumes the old blocklist: a
                // previously-abandoned station is by definition no longer
                // `Accepted`, so it already fails
                // `is_authorized_to_control` on its own — `is_abandoned`
                // itself is kept (see its own tests) as a documented,
                // narrower primitive, just no longer the ONLY gate here.
                //
                // QS round 5 Finding 2: `HoppieSession::is_our_pending_logon_reply`
                // covers the MRN-less-accept case — a real controller
                // client routinely omits the MRN on a `LOGON ACCEPTED`
                // (vSMR does; see `logon_outcome`'s doc comment in
                // thread.rs), and an MRN-less accept from the RIGHT
                // station must not be mistaken for untrusted traffic just
                // because it carries no correlating reference.
                //
                // QS round 10 (07.09.2026, #pdc-session-model, external QS
                // Finding 1): the MRN branch must NOT be handed to an
                // ORDINARY instruction — only to a message that actually
                // CLAIMS to be our logon's outcome. MIN/MRN numbers are
                // small, sequential, and Hoppie has no sender
                // authentication at all: any account, e.g. `XXXX`, could
                // address us with an ordinary WU instruction carrying
                // `MRN=<our pending logon's MIN>` and, before this fix,
                // walk straight through this allowlist — never having
                // proven anything about who it actually is. GOLD never
                // sends an operational element as a reply to
                // `REQUEST_LOGON` in the first place — only
                // `LOGON ACCEPTED`/`UM0` legitimately correlate to it by
                // MRN — so for anything else, only the (already
                // authorization-gated, station-name-based) sender check
                // may exempt a message; MRN correlation is passed as
                // `None` to force that.
                let is_our_pending_logon_reply = s.is_our_pending_logon_reply(
                    &env.from,
                    if claims_our_logon_outcome {
                        msg.mrn
                    } else {
                        None
                    },
                );
                // v1.7.28 (#pdc-clearance-without-logon, EDDM 15.09.2026):
                // a PDC clearance arrives on the CPDLC channel from a
                // station we never logged on to — our own PDC request is
                // what invites it (see `HoppieSession::pdc_requests`).
                let is_untrusted_sender = !s.may_send_replyable_uplink(
                    &this_station,
                    is_our_pending_logon_reply,
                    chrono::Utc::now(),
                );
                if is_untrusted_sender {
                    // `is_abandoned` doesn't change the decision (the
                    // allowlist above already covers it) — kept, and used
                    // here, purely to make the log distinguish two very
                    // different threat profiles for later incident
                    // analysis: a station we've genuinely never spoken to
                    // at all, versus one we specifically remember having
                    // left.
                    let previously_abandoned = s.is_abandoned(&this_station);
                    tracing::warn!(
                        min,
                        stale_from = %this_station,
                        previously_abandoned,
                        "hoppie: uplink from a station we have no accepted session with — recording it as untrusted, never as a fresh instruction"
                    );
                    drop(s);
                    telex_log
                        .lock()
                        .expect("hoppie telex_log mutex")
                        .push(TelexEntry {
                            direction: "received",
                            text: msg.element_text.clone(),
                            at: chrono::Utc::now(),
                            station: this_station,
                            from_cpdlc_channel: true,
                            superseded: true,
                        });
                    if notify_os {
                        notify_new_message(app, &env.from);
                    }
                    continue;
                }
                let arrival = chrono::Utc::now();
                // Trusted ONLY because we asked this station for a PDC —
                // not our accepted CPDLC station, not our logon's answer.
                let trusted_only_by_pdc =
                    !is_our_pending_logon_reply && !s.is_authorized_to_control(&this_station);
                let existing_meta_station = {
                    let meta = min_meta.lock().expect("hoppie min_meta mutex");
                    if trusted_only_by_pdc {
                        // External QS (Codex, 15.09.2026) P2: the thread
                        // closes OUR downlink by MRN alone, station-free —
                        // a PDC desk must not close a downlink we sent to a
                        // different station.
                        msg.mrn = mrn_usable_for_sender(&meta, msg.mrn, &this_station);
                    }
                    meta.get(&(true, min)).map(|m| m.station.clone())
                };
                // External QS (Codex, 15.09.2026) follow-up P1: "live" is not
                // only a current authority. A PDC clearance stays open past
                // its invitation window, and any collision the supersede
                // logic below has no opinion on (`NoCollision`) would just
                // overwrite the open slot — so both count as protected.
                let existing_is_live = existing_meta_station.as_deref().is_some_and(|st| {
                    holder_is_protected(
                        s.is_live_uplink_authority(st, arrival),
                        s.thread.is_uplink_session_independent(min),
                        resolve_min_collision(
                            Some(st),
                            s.thread.is_uplink_open(min),
                            &this_station,
                            &current_station,
                        ),
                    )
                });
                if needs_min_remap(
                    existing_meta_station.as_deref(),
                    s.thread.is_uplink_open(min),
                    existing_is_live,
                    &this_station,
                ) {
                    let internal = s.allocate_internal_uplink_min();
                    tracing::warn!(
                        wire_min,
                        internal,
                        from = %this_station,
                        held_by = existing_meta_station.as_deref().unwrap_or(""),
                        "hoppie: MIN already held by an open uplink of another live station — keeping both, this one under an internal MIN"
                    );
                    min = internal;
                    msg.min = internal;
                }
                let existing_meta_station = if min == wire_min {
                    existing_meta_station
                } else {
                    None
                };
                match resolve_min_collision(
                    existing_meta_station.as_deref(),
                    s.thread.is_uplink_open(min),
                    &this_station,
                    &current_station,
                ) {
                    MinCollisionResolution::NoCollision => {}
                    MinCollisionResolution::SupersedeIncoming => {
                        tracing::warn!(
                            min,
                            stale_from = %this_station,
                            current = %current_station,
                            "hoppie: late uplink from a station we've left reused an open MIN — superseding it, not the current one"
                        );
                        drop(s);
                        // QS round 2 (06.09.2026): this used to call
                        // `t.record_received(msg)` then `supersede_uplink`
                        // — but `record_received` APPENDS to `history` and
                        // OVERWRITES `open`'s `(Uplink, min)` slot before
                        // superseding runs. Since every "which entry is
                        // current" lookup for a MIN
                        // (`is_superseded_uplink`, `find_current_entry_mut`)
                        // searches history from the END, appending this
                        // STALE message made IT look like the current one
                        // — `is_superseded_uplink(min)` then read ITS
                        // `superseded=true` flag and wrongly told the pilot
                        // their genuinely still-open, unrelated clearance
                        // from the CURRENT station "is no longer valid",
                        // while `pending_uplink_count` silently dropped it
                        // from the must-reply badge. The one invariant that
                        // makes "most recent wins" safe elsewhere in this
                        // codebase — the old station's traffic is fully
                        // superseded BEFORE the new station's can arrive —
                        // is exactly what does NOT hold for a message that
                        // is chronologically older but PROCESSED later due
                        // to network/poll delay. So this message never
                        // enters the MIN/MRN thread at all: recorded like
                        // an undecodable CPDLC packet instead (own line
                        // below), visible to the pilot with its own true
                        // station, never touching `open`/`history` for a
                        // MIN a DIFFERENT, live entry already owns.
                        telex_log
                            .lock()
                            .expect("hoppie telex_log mutex")
                            .push(TelexEntry {
                                direction: "received",
                                text: msg.element_text.clone(),
                                at: chrono::Utc::now(),
                                station: this_station,
                                from_cpdlc_channel: true,
                                // QS round 3: without this, a discarded
                                // stale clearance displays with no marking
                                // at all — indistinguishable from a live,
                                // currently-actionable instruction.
                                superseded: true,
                            });
                        if notify_os {
                            notify_new_message(app, &env.from);
                        }
                        continue;
                    }
                    MinCollisionResolution::SupersedeExisting => {
                        tracing::warn!(
                            min,
                            stale_from = existing_meta_station.as_deref().unwrap_or(""),
                            current = %current_station,
                            "hoppie: open MIN belonged to a station we've left — superseding it for the current station's message"
                        );
                        s.thread.supersede_uplink(min);
                    }
                }
                // Thread's OWN `logged_on`/pending-logon fields (NOT
                // `HoppieSession::is_logged_on`/`is_logon_pending`, which
                // reflect the SESSION-level `Accepted`/`Pending` enum —
                // that enum is only ever changed BY `accept_logon`/
                // `cancel_pending` below, so reading it here would always
                // observe the value from BEFORE this message is even
                // processed, on both sides of `record_received`. Only
                // `logon_outcome`'s accept/refuse branch (thread.rs) ever
                // changes the THREAD's own fields via `record_received`,
                // so a before/after difference in THOSE is exactly (and
                // only) the signal that this message WAS a resolved
                // accept or refuse — never a side effect of anything else
                // `record_received` does. QS round 6 (07.09.2026,
                // #pdc-session-model): the first version of this read
                // `s.is_logged_on()` (the session-level mirror) for both
                // snapshots — since nothing between them ever touches
                // `s.session`, `was_logged_on == now_logged_on` was true
                // by construction, `accept_logon` below was unreachable
                // dead code, and a real `LOGON ACCEPTED` never flipped the
                // session's own state (nor fired `set_open_session` further
                // down) even though `CpdlcThread` had correctly accepted
                // it internally.
                let was_logged_on = s.thread.is_logged_on();
                let thread_pending_before = s.thread.pending_logon_min();
                // The index this entry is about to occupy — stable for
                // its lifetime (`history` is append-only, existing
                // entries are only ever mutated in place). See
                // `HistoryMeta`'s doc comment for why display needs this
                // instead of the MIN-keyed `min_meta` below.
                let history_idx = s.thread.history().len();
                s.thread.record_received(msg);
                if trusted_only_by_pdc {
                    s.mark_pdc_uplink(min, &this_station);
                }
                let now_logged_on = s.thread.is_logged_on();
                let thread_pending_after = s.thread.pending_logon_min();
                // v1.7.21 (#pdc-session-model): keep the session-identity
                // bookkeeping in lockstep with whatever `record_received`
                // (thread.rs) just decided — still holding `s`, so this is
                // atomic with the thread mutation itself, never a separate
                // step that could observe (or leave) a torn state. The
                // sender was already verified above (Finding 1), so
                // `accept_logon` here always succeeds.
                if !was_logged_on && now_logged_on {
                    s.accept_logon(&this_station);
                } else if let Some(pending_min) = thread_pending_before {
                    if thread_pending_after.is_none() && !now_logged_on {
                        // A refusal (`UM0`) closed our pending request
                        // without accepting it — never happened, not a
                        // session that ended, so no quarantine (see
                        // `cancel_pending`'s doc comment). MIN-correlated
                        // like every other call site, even though this one
                        // runs in the same lock hold as `thread_pending_before`
                        // was captured in, so it can never actually be stale
                        // here — kept for the same defense-in-depth/
                        // consistency reason `resolve_min_collision` trims
                        // already-normalized strings.
                        s.cancel_pending(pending_min);
                    }
                }
                // QS round 3 (07.09.2026): `min_meta`/`history_meta` are
                // inserted HERE, still holding `s` (`session`'s lock) —
                // not after dropping it. `hoppie_get_thread` locks
                // `session` FIRST and `min_meta`/`history_meta` after (see
                // its body), so as long as `s` stays held, no concurrent
                // read of this poll's freshly-appended history entry can
                // observe it before its metadata exists — it simply
                // blocks on `session.lock()` until this whole block is
                // done. Previously these three locks were acquired and
                // released one at a time in sequence, leaving a real
                // (if extremely narrow) window where a concurrent
                // `hoppie_get_thread` call could read the new entry with
                // no metadata yet — `station: None`, `at: now()` — before
                // self-correcting on its own next poll.
                let now = chrono::Utc::now();
                min_meta.lock().expect("hoppie min_meta mutex").insert(
                    (true, min),
                    crate::hoppie::MsgMeta {
                        at: now,
                        // The sender off the wire — this is what a reply
                        // to this uplink must be addressed to.
                        station: this_station.clone(),
                        wire_min: Some(wire_min),
                    },
                );
                history_meta
                    .lock()
                    .expect("hoppie history_meta mutex")
                    .insert(
                        history_idx,
                        crate::hoppie::MsgMeta {
                            at: now,
                            station: this_station,
                            wire_min: Some(wire_min),
                        },
                    );
                // QS round 8 (07.09.2026, #pdc-session-model, external QS
                // Finding 5): the accepted station is captured HERE,
                // still holding `s` — reading it via a fresh
                // `session.lock()` AFTER `drop(s)` (the previous version)
                // left a real window for a concurrent manual logoff/
                // handover to change the session in between, persisting
                // either an already-ended or a not-yet-accepted station
                // as if it were the one truly open. Decision pulled into
                // `HoppieSession::station_to_persist_on_accept` (round 9)
                // so it's unit-tested without an `AppHandle`.
                let accepted = s.station_to_persist_on_accept(was_logged_on);
                drop(s);
                // Record the open session the moment a facility
                // accepts us, so a run that dies without logging
                // off can be cleaned up on the next connect —
                // and ONLY then (see hoppie_connect).
                //
                // QS round 9 (external QS follow-up — Finding 5 was NOT
                // fully closed by the atomic read above): the poller (this
                // task) and a Tauri disconnect command are genuinely
                // parallel OS threads. Between `drop(s)` above and the
                // actual file write below, a concurrent disconnect could
                // run its ENTIRE logoff — including the network round trip
                // — and its own `clear_open_session`. "No `.await` in
                // between" does NOT prevent that on a multi-threaded
                // runtime; it only means Tokio's own cooperative scheduler
                // won't preempt here, which says nothing about true OS
                // thread parallelism.
                //
                // QS round 10 (external QS Finding 3): re-locking, reading
                // the generation, and letting THAT guard drop before
                // calling `set_open_session` (the first version of this
                // fix) reopened the exact race the generation counter was
                // meant to close — the check and the write were still two
                // separate, unsynchronized moments, with the file I/O
                // itself fully unlocked in between. The guard below is
                // held THROUGH the write: if a newer event (a disconnect,
                // or a fresh logon) has run since `accepted` was decided,
                // this write is stale and must be skipped — that newer
                // event's own persistence call is authoritative instead —
                // and nothing else touching this lock can interleave its
                // own check-and-write while this one is still in progress.
                if let Some((station, generation)) = accepted {
                    let s = session.lock().expect("hoppie session mutex");
                    if s.persist_generation() == generation {
                        crate::hoppie::settings::set_open_session(app, from_callsign, &station);
                    } else {
                        tracing::debug!(
                            station = %station,
                            "hoppie: skipped a now-stale open-session marker write — a newer session event already superseded it"
                        );
                    }
                }
                if notify_os {
                    notify_new_message(app, &env.from);
                }
            }
            // An undecodable CPDLC packet must still reach the
            // pilot. vSMR (the VATSIM UK controller plugin) sends
            // three of its four actions — STANDBY, "UNABLE CALL ON
            // FREQ", and a logon refusal — as bare text with
            // `type=cpdlc` and no `/data2/` header at all
            // (SMRPlugin.cpp:96-101, :473, :511, :164). Dropping
            // those meant the controller pressed a button, saw it
            // acknowledged by the network, and the pilot was never
            // told. Surface it like a telex: no MIN/MRN threading,
            // but visible and audible.
            Err(e) if !env.packet.trim().is_empty() => {
                tracing::info!(
                    error = %e,
                    packet = %env.packet,
                    "hoppie: CPDLC packet without a /data2/ header — showing as plain text"
                );
                let from = env.from.clone();
                let this_station = normalize_station(&env.from);
                // QS round 8 (07.09.2026, #pdc-session-model, external QS
                // Finding 6): this raw/undecodable path used to bypass the
                // whole session model entirely — always `superseded:
                // false`, regardless of whether the sender was even
                // authorized, and a raw logon refusal never cleared
                // `Pending`. Decision logic lives in
                // `HoppieSession::handle_undecodable_uplink` (round 9) —
                // pulled out specifically so it's unit-tested without
                // needing an `AppHandle` here.
                //
                // QS round 10 (external QS Finding 2): `looks_like_a_refusal`
                // is computed HERE, from the actual text, rather than
                // `handle_undecodable_uplink` presuming EVERY raw text
                // from the pending station is a refusal — that used to
                // also catch vSMR's two documented, legitimate NON-refusal
                // conventions (a plain interim `STANDBY` or "UNABLE CALL
                // ON FREQ" ack), aborting a still-live logon attempt over
                // an unrelated message.
                let looks_like_a_refusal = !is_a_known_non_refusal_raw_text(&env.packet);
                let (superseded, cancelled_pending) = session
                    .lock()
                    .expect("hoppie session mutex")
                    .handle_undecodable_uplink(
                        &this_station,
                        looks_like_a_refusal,
                        chrono::Utc::now(),
                    );
                if cancelled_pending {
                    tracing::warn!(
                        from = %this_station,
                        "hoppie: undecodable uplink from our pending station treated as a likely logon refusal (vSMR-style) — no longer waiting on it"
                    );
                }
                telex_log
                    .lock()
                    .expect("hoppie telex_log mutex")
                    .push(TelexEntry {
                        direction: "received",
                        text: env.packet,
                        at: chrono::Utc::now(),
                        station: this_station,
                        // Arrived on the CPDLC channel — belongs
                        // in the CPDLC log, not the PDC tab.
                        from_cpdlc_channel: true,
                        superseded,
                    });
                if notify_os {
                    notify_new_message(app, &from);
                }
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    packet = %env.packet,
                    "hoppie: failed to decode CPDLC packet"
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn poll_once(
    app: &AppHandle,
    http: &HoppieHttp,
    session: &StdMutex<HoppieSession>,
    telex_log: &StdMutex<Vec<TelexEntry>>,
    min_meta: &StdMutex<MinMeta>,
    history_meta: &StdMutex<HistoryMeta>,
    last_error: &StdMutex<Option<String>>,
    from_callsign: &str,
    logon: &str,
    // False on the first poll of a session — see `spawn`'s `first_poll`.
    notify_os: bool,
) {
    let req = HoppieRequest {
        logon: logon.to_string(),
        from: from_callsign.to_string(),
        to: session.lock().expect("hoppie session mutex").addressee(),
        kind: PacketKind::Poll,
        packet: None,
    };
    match http.send(&req).await {
        Ok(HoppieResponseLine::Ok) => {
            *last_error.lock().expect("hoppie last_error mutex") = None;
        }
        Ok(HoppieResponseLine::OkWithPayload(content)) => {
            *last_error.lock().expect("hoppie last_error mutex") = None;
            process_poll_payload(
                app,
                http,
                &content,
                session,
                telex_log,
                min_meta,
                history_meta,
                from_callsign,
                logon,
                notify_os,
            )
            .await;
        }
        Ok(HoppieResponseLine::Error(reason)) => {
            // Into the activity log, not just tracing: a rejected poll is
            // the pilot's problem (bad logon code, locked callsign) and
            // must be reviewable after the fact — Warn/Error entries also
            // ship to the error backend, so it's visible off-machine.
            log_activity_handle(
                app,
                ActivityLevel::Warn,
                "Hoppie: Abruf abgelehnt",
                Some(reason.clone()),
            );
            *last_error.lock().expect("hoppie last_error mutex") = Some(reason);
        }
        Err(e) => {
            log_activity_handle(
                app,
                ActivityLevel::Warn,
                "Hoppie: Abruf fehlgeschlagen",
                Some(e.message.clone()),
            );
            *last_error.lock().expect("hoppie last_error mutex") = Some(e.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baseline_interval_is_within_the_docs_recommended_band() {
        let interval = poll_interval(0);
        assert!(interval >= Duration::from_secs(45));
        assert!(interval <= Duration::from_secs(75));
    }

    #[test]
    fn fast_interval_kicks_in_exactly_when_a_response_is_pending() {
        assert_eq!(poll_interval(1), Duration::from_secs(FAST_POLL_SECS));
        assert_eq!(poll_interval(5), Duration::from_secs(FAST_POLL_SECS));
        let idle = poll_interval(0);
        assert!(idle >= Duration::from_secs(BASELINE_POLL_MIN_SECS));
        assert!(idle <= Duration::from_secs(BASELINE_POLL_MAX_SECS));
    }

    /// "Randomly timed" is the documented request, and the point is that
    /// clients which started together don't stay in lockstep. A constant
    /// would pass every range check while defeating that entirely.
    #[test]
    fn idle_interval_actually_varies() {
        let samples: std::collections::HashSet<u64> =
            (0..40).map(|_| poll_interval(0).as_secs()).collect();
        assert!(
            samples.len() > 1,
            "40 draws all landed on the same value — that is not random"
        );
        for secs in samples {
            assert!(
                (BASELINE_POLL_MIN_SECS..=BASELINE_POLL_MAX_SECS).contains(&secs),
                "{secs}s is outside the documented 45-75s band"
            );
        }
    }

    #[test]
    fn handover_yields_the_next_facility_uppercased() {
        assert_eq!(parse_handover("HANDOVER EDGG"), Some("EDGG".to_string()));
        assert_eq!(parse_handover("HANDOVER eduu"), Some("EDUU".to_string()));
        assert_eq!(
            parse_handover("  HANDOVER LOVV  "),
            Some("LOVV".to_string())
        );
    }

    #[test]
    fn non_handover_traffic_is_never_mistaken_for_one() {
        // A real instruction must reach the pilot, not silently re-log-on.
        assert_eq!(parse_handover("CLIMB TO AND MAINTAIN FL240"), None);
        assert_eq!(parse_handover("LOGON ACCEPTED"), None);
        // Prefix-only / multi-token text is not a handover directive.
        assert_eq!(parse_handover("HANDOVER"), None);
        assert_eq!(parse_handover("HANDOVER EDGG WHEN READY"), None);
    }

    /// v0.19.x FIX: free text that merely starts with the 8 characters
    /// "HANDOVER" with no separator must never be mistaken for a real
    /// handover directive — that used to silently log the aircraft off.
    #[test]
    fn handover_requires_a_separator_after_the_literal_word() {
        assert_eq!(parse_handover("HANDOVEREDUU"), None);
        assert_eq!(parse_handover("HANDOVERSHIP HAS SAILED"), None);
        // A real, whitespace-separated handover still works.
        assert_eq!(parse_handover("HANDOVER EDUU"), Some("EDUU".to_string()));
    }

    // --- parse_next_data_authority / is_end_service (UM160/UM161) ---

    fn recognized_uplink(min: u32, text: &str) -> cpdlc::CpdlcMessage {
        cpdlc::decode(&format!("/data2/{min}//NE/{text}"), Direction::Uplink)
            .expect("well-formed test packet")
    }

    #[test]
    fn next_data_authority_extracts_the_named_station() {
        let msg = recognized_uplink(5, "NEXT DATA AUTHORITY LRBB");
        assert_eq!(parse_next_data_authority(&msg), Some("LRBB".to_string()));
    }

    #[test]
    fn next_data_authority_is_none_for_unrelated_traffic() {
        let msg = recognized_uplink(5, "CLIMB TO AND MAINTAIN FL240");
        assert_eq!(parse_next_data_authority(&msg), None);
        let logoff_lookalike = recognized_uplink(5, "END SERVICE");
        assert_eq!(parse_next_data_authority(&logoff_lookalike), None);
    }

    // QS round 06.09.2026: the generic placeholder resolver does not
    // itself validate that a "single ICAO" placeholder is actually one
    // token — without this guard "NEXT DATA AUTHORITY LRBB EXTRA" would
    // have been logged on to as station "LRBB EXTRA".
    #[test]
    fn next_data_authority_rejects_a_multi_token_value() {
        let msg = recognized_uplink(5, "NEXT DATA AUTHORITY LRBB EXTRA");
        assert_eq!(parse_next_data_authority(&msg), None);
    }

    #[test]
    fn end_service_is_recognized() {
        let msg = recognized_uplink(6, "END SERVICE");
        assert!(is_end_service(&msg));
    }

    #[test]
    fn end_service_is_false_for_unrelated_traffic() {
        assert!(!is_end_service(&recognized_uplink(6, "LOGON ACCEPTED")));
        assert!(!is_end_service(&recognized_uplink(
            6,
            "NEXT DATA AUTHORITY LRBB"
        )));
    }

    // QS round 06.09.2026: Hoppie's GOLD table has NO uplink LOGOFF
    // element at all (`DM_LOGOFF` is downlink-only, aircraft-to-ground),
    // so a ground system that mirrors the aircraft's own convention back
    // sends undecodable free text that falls through to `Raw`. Already
    // documented as a real, previously-seen case in the 25.07.2026 vSMR
    // audit ("ein unaufgeforderter LOGOFF matcht keinen Filter") — missing
    // it here reproduces the exact 38-minute-open-uplink field incident
    // this whole fix addresses.
    #[test]
    fn end_service_recognizes_a_bare_logoff_uplink() {
        assert!(is_end_service(&recognized_uplink(6, "LOGOFF")));
        // Case/whitespace-tolerant, same discipline as `normalize_station`.
        assert!(is_end_service(&recognized_uplink(6, "  logoff ")));
    }

    #[test]
    fn end_service_is_false_for_raw_text_that_merely_mentions_logoff() {
        // Guards the OTHER direction: a controller remark that happens to
        // contain the word must not be mistaken for a session end.
        assert!(!is_end_service(&recognized_uplink(
            6,
            "LOGOFF EXPECTED SHORTLY, STANDBY"
        )));
    }

    // --- is_a_known_non_refusal_raw_text (round 10, external QS Finding 2) ---

    #[test]
    fn recognizes_standby_as_a_known_non_refusal() {
        assert!(is_a_known_non_refusal_raw_text("STANDBY"));
        assert!(is_a_known_non_refusal_raw_text("  standby "));
    }

    #[test]
    fn recognizes_unable_call_on_freq_as_a_known_non_refusal() {
        assert!(is_a_known_non_refusal_raw_text("UNABLE CALL ON FREQ"));
        assert!(is_a_known_non_refusal_raw_text(" unable call on freq"));
    }

    #[test]
    fn anything_else_is_not_a_known_non_refusal() {
        // Field regression this guards: treating this as a KNOWN
        // non-refusal would be just as wrong as treating STANDBY as a
        // refusal — an unrecognized text stays presumed-refusal, the
        // best available signal given vSMR's undocumented exact wording.
        assert!(!is_a_known_non_refusal_raw_text("SOME OTHER TEXT"));
        assert!(!is_a_known_non_refusal_raw_text(""));
        assert!(!is_a_known_non_refusal_raw_text("STANDBY, WILL CALL BACK"));
    }

    // `handover_sender_is_authorized`'s own tests moved to session.rs —
    // the check itself is now `HoppieSession::is_authorized_to_control`
    // (see that module's `control_authorization_*` tests).

    // --- resolve_min_collision ---
    //
    // QS round 06.09.2026: field-observed shape — LBSR queues an uplink
    // we haven't polled yet, we move on to LRBB, LRBB independently
    // reuses the same MIN before LBSR's message finally arrives. Which
    // side is "stale" is decided by the current facility
    // (`HoppieSession::addressee`), not by which one this poll happened
    // to process last.

    #[test]
    fn no_prior_entry_is_not_a_collision() {
        assert_eq!(
            resolve_min_collision(None, false, "LRBB", "LRBB"),
            MinCollisionResolution::NoCollision
        );
    }

    #[test]
    fn same_station_reusing_its_own_min_is_not_a_collision() {
        assert_eq!(
            resolve_min_collision(Some("LBSR"), true, "LBSR", "LBSR"),
            MinCollisionResolution::NoCollision
        );
    }

    #[test]
    fn a_closed_prior_entry_is_not_a_collision() {
        // Ordinary MIN reuse after the earlier one was legitimately
        // answered — `find_current_entry_mut`'s "most recent wins" already
        // handles this correctly, nothing to resolve here.
        assert_eq!(
            resolve_min_collision(Some("LBSR"), false, "LRBB", "LRBB"),
            MinCollisionResolution::NoCollision
        );
    }

    #[test]
    fn a_late_message_from_the_station_we_left_is_superseded() {
        // The open entry belongs to LRBB (current); LBSR's late arrival
        // must not evict it.
        assert_eq!(
            resolve_min_collision(Some("LRBB"), true, "LBSR", "LRBB"),
            MinCollisionResolution::SupersedeIncoming
        );
    }

    #[test]
    fn a_stale_open_entry_from_the_old_station_yields_to_the_current_one() {
        // The open entry is a leftover from LBSR (no longer current);
        // LRBB's fresh message is what the pilot is actually engaged with.
        assert_eq!(
            resolve_min_collision(Some("LBSR"), true, "LRBB", "LRBB"),
            MinCollisionResolution::SupersedeExisting
        );
    }

    #[test]
    fn two_unrelated_stations_with_neither_current_has_no_authority_signal() {
        // Two delivery desks answering independently, neither of which is
        // the logged-on facility — no basis to prefer one over the other,
        // so today's arrival-order behavior stands.
        assert_eq!(
            resolve_min_collision(Some("EDDF_DEL"), true, "EDDM_DEL", "LRBB"),
            MinCollisionResolution::NoCollision
        );
    }

    #[test]
    fn station_comparison_is_case_and_whitespace_tolerant() {
        assert_eq!(
            resolve_min_collision(Some(" lrbb "), true, "lbsr", "LRBB"),
            MinCollisionResolution::SupersedeIncoming
        );
    }

    // `is_uplink_from_an_ended_session`'s own tests moved to session.rs.
    // The gate itself is now `HoppieSession::is_authorized_to_control`
    // (an allowlist — round 8, external QS Finding 1), with
    // `is_abandoned` kept as a narrower, still-tested primitive for
    // diagnostics only (see session.rs's abandoned-station and
    // re-acceptance tests, including the field-confirmed LOGOFF-then-WU
    // regression and the A -> B -> A re-acceptance case).

    // --- reorder_same_batch_envelopes / requires_reply_for_batch_order ---
    //
    // Field feedback 31.07.2026: real Sofia-Control log, one poll, MIN 8-10.

    fn cpdlc_env(from: &str, min: u32, response_code: &str, text: &str) -> wire::InboundEnvelope {
        wire::InboundEnvelope {
            from: from.to_string(),
            kind: PacketKind::Cpdlc,
            packet: format!("/data2/{min}//{response_code}/{text}"),
        }
    }

    fn telex_env(from: &str, text: &str) -> wire::InboundEnvelope {
        wire::InboundEnvelope {
            from: from.to_string(),
            kind: PacketKind::Telex,
            packet: text.to_string(),
        }
    }

    #[test]
    fn real_sofia_batch_puts_current_atc_unit_before_the_instruction() {
        // Ground station's own send order was MIN 8, 9, 10 — logon accepted,
        // THEN the clearance, THEN current-atc-unit. That's what shipped on
        // the wire; only the on-screen order changes here.
        let batch = vec![
            cpdlc_env("LBSR", 8, "NE", "LOGON ACCEPTED"),
            cpdlc_env("LBSR", 9, "WU", "PROCEED DIRECT TO NIKTI"),
            cpdlc_env("LBSR", 10, "NE", "CURRENT ATC UNIT _ LBSR _ SOFIA CTR"),
        ];
        let reordered = reorder_same_batch_envelopes(batch);
        let mins: Vec<u32> = reordered
            .iter()
            .map(|e| cpdlc::decode(&e.packet, Direction::Uplink).unwrap().min)
            .collect();
        assert_eq!(
            mins,
            vec![8, 10, 9],
            "LOGON ACCEPTED and CURRENT ATC UNIT (no reply owed) must both land \
             ahead of PROCEED DIRECT TO (reply owed), in their original relative order"
        );
    }

    #[test]
    fn real_sofia_batch_end_to_end_from_the_raw_hoppie_poll_response() {
        // Everything above builds `InboundEnvelope` values by hand — that
        // only proves the sort behaves as intended GIVEN envelopes shaped
        // the way I assumed they'd be shaped. It does not prove the real
        // parser (`wire::parse_poll_envelopes`, a separate crate, hand-
        // written brace scanner) actually hands `reorder_same_batch_envelopes`
        // envelopes in that shape from real wire bytes. This test starts
        // one step further back: a poll response body in the exact
        // `{FROM type {packet}}` framing documented in wire.rs (and
        // covered by that crate's own `parse_poll_envelopes_multiple`
        // test), reproducing the real Sofia Control batch byte-for-byte
        // as it would have arrived over HTTP — not a struct I built to
        // match my own mental model of the data.
        let raw_poll_body = "\
            {LBSR cpdlc {/data2/8//NE/LOGON ACCEPTED}}\
            {LBSR cpdlc {/data2/9//WU/PROCEED DIRECT TO NIKTI}}\
            {LBSR cpdlc {/data2/10//NE/CURRENT ATC UNIT _ LBSR _ SOFIA CTR}}\
        ";

        let parsed = wire::parse_poll_envelopes(raw_poll_body);
        assert_eq!(
            parsed.len(),
            3,
            "the real parser must find all three envelopes"
        );

        let reordered = reorder_same_batch_envelopes(parsed);
        let decoded: Vec<_> = reordered
            .iter()
            .map(|e| cpdlc::decode(&e.packet, Direction::Uplink).unwrap())
            .collect();

        assert_eq!(
            decoded.iter().map(|m| m.min).collect::<Vec<_>>(),
            vec![8, 10, 9],
            "raw wire bytes in → correct pilot-facing order out, through the \
             real parser this actually runs behind, not a stand-in for it"
        );
        // Pin the actual text too, not just the MIN numbers — a MIN-only
        // assertion would still pass if the element text got mangled
        // (e.g. truncated at the first '}' the way `packet_text_containing_
        // a_brace_survives_intact` in wire.rs guards against) as long as
        // the numbers came out in the right slots.
        assert_eq!(decoded[0].element_text, "LOGON ACCEPTED");
        assert_eq!(
            decoded[1].element_text,
            "CURRENT ATC UNIT _ LBSR _ SOFIA CTR"
        );
        assert_eq!(decoded[2].element_text, "PROCEED DIRECT TO NIKTI");
    }

    #[test]
    fn two_reply_required_messages_keep_their_original_relative_order() {
        // Stable sort must never reorder two instructions against each
        // other — only move no-reply messages ahead of them as a group.
        let batch = vec![
            cpdlc_env("EDGG", 4, "WU", "CLIMB TO FL240"),
            cpdlc_env("EDGG", 5, "AN", "CONFIRM ASSIGNED ALTITUDE"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "no reordering needed when nothing is NE/N"
        );
    }

    #[test]
    fn two_no_reply_messages_keep_their_original_relative_order() {
        let batch = vec![
            cpdlc_env("EDGG", 1, "NE", "LOGON ACCEPTED"),
            cpdlc_env("EDGG", 2, "N", "MONITOR UNICOM 122.8"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "both are already 'no reply owed' and in order"
        );
    }

    #[test]
    fn telex_traffic_disables_reordering_for_the_whole_batch() {
        // Telex has no MIN/response code at all — there is nothing to base
        // a "does this need a reply" judgement on. A batch containing any
        // unclassifiable entry is left entirely untouched (see the doc
        // comment on `reorder_same_batch_envelopes` for why a plain
        // "telex always sorts after NE/N" key does NOT achieve this: a
        // stable sort still pulls the telex from before the NE message to
        // after it, because ordering only survives within one key group).
        let batch = vec![
            telex_env("LBSR", "STANDBY"),
            cpdlc_env("LBSR", 3, "NE", "LOGON ACCEPTED"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "telex present → whole batch stays as received"
        );
    }

    #[test]
    fn undecodable_cpdlc_packet_is_left_in_place_not_guessed_at() {
        let batch = vec![
            wire::InboundEnvelope {
                from: "EGTT".to_string(),
                kind: PacketKind::Cpdlc,
                packet: "UNABLE CALL ON FREQ".to_string(), // no /data2/ prefix
            },
            cpdlc_env("EGTT", 7, "NE", "LOGON ACCEPTED"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "an undecodable packet must not be silently reordered — it has no \
             confirmed response requirement to sort on"
        );
    }

    #[test]
    fn a_handover_directive_disables_reordering_for_the_whole_batch() {
        // HANDOVER has real side effects beyond the message log (ends the
        // current session, fires a new REQUEST LOGON) that run in
        // whatever order the loop processes envelopes in — that order must
        // stay exactly what the ground station sent, not something this
        // sort decides. NE is the realistic code for a bookkeeping message
        // like this, which is exactly the code that would otherwise make
        // it a candidate for being pulled earlier.
        let batch = vec![
            cpdlc_env("EDGG", 11, "WU", "CLIMB TO FL240"),
            cpdlc_env("EDGG", 12, "NE", "HANDOVER EDUU"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "a handover anywhere in the batch must disable reordering entirely, \
             not just exempt itself from being moved"
        );
    }

    // QS round 06.09.2026: the guard above originally only checked
    // `parse_handover`, which the two GOLD session-end forms (and the
    // bare-LOGOFF fallback) added alongside it don't go through — an
    // `END SERVICE` sorted ahead of a same-batch clearance would have run
    // its session-end side effects before that clearance was even
    // recorded, letting it settle into `open` unsuperseded on the new
    // session. Same shape as `a_handover_directive_disables_reordering_
    // for_the_whole_batch` above, one case per new form.
    #[test]
    fn an_end_service_directive_disables_reordering_for_the_whole_batch() {
        let batch = vec![
            cpdlc_env("EDGG", 11, "WU", "CLIMB TO FL240"),
            cpdlc_env("EDGG", 12, "NE", "END SERVICE"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "END SERVICE anywhere in the batch must disable reordering entirely"
        );
    }

    #[test]
    fn a_next_data_authority_directive_disables_reordering_for_the_whole_batch() {
        let batch = vec![
            cpdlc_env("EDGG", 11, "WU", "CLIMB TO FL240"),
            cpdlc_env("EDGG", 12, "NE", "NEXT DATA AUTHORITY EDUU"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "NEXT DATA AUTHORITY anywhere in the batch must disable reordering entirely"
        );
    }

    #[test]
    fn a_bare_logoff_disables_reordering_for_the_whole_batch() {
        let batch = vec![
            cpdlc_env("EDGG", 11, "WU", "CLIMB TO FL240"),
            cpdlc_env("EDGG", 12, "NE", "LOGOFF"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(
            reordered, batch,
            "a bare LOGOFF anywhere in the batch must disable reordering entirely"
        );
    }

    #[test]
    fn handover_lookalike_free_text_does_not_falsely_disable_reordering() {
        // Guards the OTHER direction: `parse_handover` is deliberately
        // strict (exactly one token after the literal word), so a message
        // that merely mentions the word must not be mistaken for one and
        // must not block reordering it doesn't actually need to block.
        let batch = vec![
            cpdlc_env("EDGG", 20, "NE", "LOGON ACCEPTED"),
            cpdlc_env("EDGG", 21, "WU", "HANDOVER EXPECTED SHORTLY, STANDBY"),
        ];
        let reordered = reorder_same_batch_envelopes(batch);
        let mins: Vec<u32> = reordered
            .iter()
            .map(|e| cpdlc::decode(&e.packet, Direction::Uplink).unwrap().min)
            .collect();
        assert_eq!(
            mins,
            vec![20, 21],
            "already in the desired order, and not blocked"
        );
    }

    #[test]
    fn already_correct_order_is_left_untouched() {
        let batch = vec![
            cpdlc_env("LBSR", 1, "NE", "LOGON ACCEPTED"),
            cpdlc_env("LBSR", 2, "NE", "CURRENT ATC UNIT _ LBSR _ SOFIA CTR"),
            cpdlc_env("LBSR", 3, "WU", "PROCEED DIRECT TO NIKTI"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(reordered, batch);
    }

    #[test]
    fn empty_and_single_element_batches_do_not_panic() {
        assert_eq!(reorder_same_batch_envelopes(vec![]), vec![]);
        let one = vec![cpdlc_env("LBSR", 1, "WU", "CLIMB TO FL240")];
        assert_eq!(reorder_same_batch_envelopes(one.clone()), one);
    }

    #[test]
    fn requires_reply_matches_the_shared_response_requirement_definition() {
        // Guards against this file quietly drifting from
        // ResponseRequirement::requires_reply if that ever changes.
        assert!(!requires_reply_for_batch_order(&cpdlc_env(
            "x",
            1,
            "NE",
            "LOGON ACCEPTED"
        )));
        assert!(!requires_reply_for_batch_order(&cpdlc_env(
            "x",
            1,
            "N",
            "MONITOR UNICOM 122.8"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x",
            1,
            "WU",
            "CLIMB TO FL240"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x",
            1,
            "AN",
            "CONFIRM ASSIGNED ALTITUDE"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x",
            1,
            "R",
            "ROGER TEST"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x",
            1,
            "Y",
            "FREE TEXT"
        )));
    }

    // --- MIN collisions between live stations (external QS, Codex 15.09.2026) ---

    fn meta(entries: &[((bool, u32), &str)]) -> crate::hoppie::MinMeta {
        entries
            .iter()
            .map(|(k, st)| {
                (
                    *k,
                    crate::hoppie::MsgMeta {
                        at: chrono::Utc::now(),
                        station: (*st).to_string(),
                        wire_min: None,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn a_colliding_min_from_another_live_station_is_remapped_not_superseded() {
        assert!(needs_min_remap(Some("EDMM"), true, true, "EDDM"));
        assert!(needs_min_remap(Some("EDDM"), true, true, "EDMM"));
    }

    #[test]
    fn no_remap_for_same_station_closed_slot_or_a_dead_holder() {
        assert!(!needs_min_remap(Some("EDDM"), true, true, " eddm"));
        assert!(!needs_min_remap(Some("EDMM"), false, true, "EDDM"));
        assert!(
            !needs_min_remap(Some("LBSR"), true, false, "LRBB"),
            "an abandoned holder still goes through resolve_min_collision"
        );
        assert!(!needs_min_remap(None, true, true, "EDDM"));
    }

    #[test]
    fn a_pdc_desk_may_only_reference_downlinks_it_was_sent() {
        let m = meta(&[((false, 3), "EDMM"), ((false, 4), "EDDM")]);
        assert_eq!(mrn_usable_for_sender(&m, Some(3), "EDDM"), None);
        assert_eq!(mrn_usable_for_sender(&m, Some(4), "eddm"), Some(4));
        assert_eq!(mrn_usable_for_sender(&m, Some(9), "EDDM"), None);
        assert_eq!(mrn_usable_for_sender(&m, None, "EDDM"), None);
    }

    #[test]
    fn an_open_holder_is_never_silently_overwritten() {
        // Expired PDC invitation, clearance still open.
        assert!(holder_is_protected(
            false,
            true,
            MinCollisionResolution::NoCollision
        ));
        assert!(holder_is_protected(
            false,
            true,
            MinCollisionResolution::SupersedeExisting
        ));
        // Two unrelated stations, supersede logic has no opinion.
        assert!(holder_is_protected(
            false,
            false,
            MinCollisionResolution::NoCollision
        ));
        // Current authority.
        assert!(holder_is_protected(
            true,
            false,
            MinCollisionResolution::SupersedeExisting
        ));
        // An abandoned station's leftover keeps the established supersede path.
        assert!(!holder_is_protected(
            false,
            false,
            MinCollisionResolution::SupersedeExisting
        ));
        assert!(!holder_is_protected(
            false,
            false,
            MinCollisionResolution::SupersedeIncoming
        ));
    }
}
