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
use hoppie_protocol::thread::CpdlcThread;
use hoppie_protocol::wire::{self, HoppieRequest, HoppieResponseLine, PacketKind};

use super::{HoppieHttp, MinMeta, TelexEntry};
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
/// - **A handover directive anywhere in the batch.** Unlike every other
///   uplink here, handling a `HANDOVER <station>` message has real side
///   effects beyond the message log — see `poll_once`'s handover arm:
///   it flips `to_station`, marks the thread logged off, and fires a
///   new `REQUEST LOGON`, all synchronously, before the loop moves on to
///   the next envelope. `HANDOVER` itself typically carries a no-reply
///   code, so without this guard it would be a candidate for being
///   pulled EARLIER in the batch by the very sort this function does —
///   changing not just what the pilot sees but the actual order those
///   side effects run in relative to whatever else was in the batch.
///   That risk costs nothing to close (batches mixing a handover with
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
            Ok(msg) => parse_handover(&msg.element_text).is_none(),
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
    thread: Arc<StdMutex<CpdlcThread>>,
    telex_log: Arc<StdMutex<Vec<TelexEntry>>>,
    min_meta: Arc<StdMutex<MinMeta>>,
    last_error: Arc<StdMutex<Option<String>>>,
    from_callsign: String,
    logon: String,
    to_station: Arc<StdMutex<String>>,
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
                let t = thread.lock().expect("hoppie thread mutex");
                poll_interval(t.pending_response_count())
            };
            tokio::select! {
                res = stop_rx.changed() => {
                    if res.is_err() || *stop_rx.borrow() {
                        break;
                    }
                }
                _ = tokio::time::sleep(interval) => {
                    poll_once(&app, &http, &thread, &telex_log, &min_meta, &last_error, &from_callsign, &logon, &to_station, notify_os && !first_poll).await;
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
/// surface a client-side check can actually reduce. Pure, so it's
/// testable without a live connection.
fn handover_sender_is_authorized(from: &str, current_station: &str) -> bool {
    !current_station.trim().is_empty() && from.trim().eq_ignore_ascii_case(current_station.trim())
}

/// Station ids on the wire are compared case-insensitively everywhere
/// else in this module (see `handover_sender_is_authorized`), but a
/// RECORDED station is also used as a send address later. Normalizing
/// once, where it enters the app, keeps "ldzo " and "LDZO" from becoming
/// two different addressees.
fn normalize_station(raw: &str) -> String {
    raw.trim().to_uppercase()
}

/// Fire `REQUEST LOGON` at `station` as part of an automatic handover.
/// Best-effort: a failure here leaves the pilot on the old facility with
/// a visible "not logged on" state rather than silently pretending.
async fn send_logon(
    http: &HoppieHttp,
    thread: &StdMutex<CpdlcThread>,
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
        let mut t = thread.lock().expect("hoppie thread mutex");
        let (message, _) = t.record_sent(
            spec.response,
            None,
            resolved.filled_text.clone(),
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        let min = message.min;
        (message, min)
    };
    min_meta.lock().expect("hoppie min_meta mutex").insert(
        (false, min),
        crate::hoppie::MsgMeta {
            at: chrono::Utc::now(),
            station: station.to_string(),
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
            thread
                .lock()
                .expect("hoppie thread mutex")
                .rollback_sent(min);
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
    thread: &StdMutex<CpdlcThread>,
    telex_log: &StdMutex<Vec<TelexEntry>>,
    min_meta: &StdMutex<MinMeta>,
    from_callsign: &str,
    logon: &str,
    to_station: &StdMutex<String>,
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
                });
            if notify_os {
                notify_new_message(app, &from);
            }
            continue;
        }
        match cpdlc::decode(&env.packet, Direction::Uplink) {
            Ok(msg) => {
                // Sector handover: the current centre names the one
                // taking over and we silently log on there. The pilot
                // never acts on this — it's protocol bookkeeping, not
                // an instruction, so it stays out of the message log.
                if let Some(next) = parse_handover(&msg.element_text) {
                    let current_station =
                        to_station.lock().expect("hoppie to_station mutex").clone();
                    if !handover_sender_is_authorized(&env.from, &current_station) {
                        // v0.19.x FIX: don't act on a HANDOVER from anyone
                        // other than the facility we're currently connected
                        // to — see `handover_sender_is_authorized`'s doc
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
                        // not accepted yet. Leaving `logged_on` set
                        // made the header claim "connected <new
                        // centre>" from this instant on, even if that
                        // centre never answers — the pilot would
                        // believe they have a datalink they don't.
                        thread
                            .lock()
                            .expect("hoppie thread mutex")
                            .mark_logged_off();
                        crate::hoppie::settings::clear_open_session(app);
                        *to_station.lock().expect("hoppie to_station mutex") = next.clone();
                        send_logon(http, thread, min_meta, from_callsign, logon, &next)
                            .await;
                        continue;
                    }
                }
                let min = msg.min;
                let mut t = thread.lock().expect("hoppie thread mutex");
                let was_logged_on = t.is_logged_on();
                t.record_received(msg);
                let now_logged_on = t.is_logged_on();
                drop(t);
                // Record the open session the moment a facility
                // accepts us, so a run that dies without logging
                // off can be cleaned up on the next connect —
                // and ONLY then (see hoppie_connect).
                if !was_logged_on && now_logged_on {
                    let station = to_station.lock().expect("hoppie to_station mutex").clone();
                    crate::hoppie::settings::set_open_session(app, &station);
                }
                min_meta.lock().expect("hoppie min_meta mutex").insert(
                    (true, min),
                    crate::hoppie::MsgMeta {
                        at: chrono::Utc::now(),
                        // The sender off the wire — this is what a reply
                        // to this uplink must be addressed to.
                        station: normalize_station(&env.from),
                    },
                );
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
                telex_log
                    .lock()
                    .expect("hoppie telex_log mutex")
                    .push(TelexEntry {
                        direction: "received",
                        text: env.packet,
                        at: chrono::Utc::now(),
                        station: normalize_station(&env.from),
                        // Arrived on the CPDLC channel — belongs
                        // in the CPDLC log, not the PDC tab.
                        from_cpdlc_channel: true,
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
    thread: &StdMutex<CpdlcThread>,
    telex_log: &StdMutex<Vec<TelexEntry>>,
    min_meta: &StdMutex<MinMeta>,
    last_error: &StdMutex<Option<String>>,
    from_callsign: &str,
    logon: &str,
    to_station: &StdMutex<String>,
    // False on the first poll of a session — see `spawn`'s `first_poll`.
    notify_os: bool,
) {
    let req = HoppieRequest {
        logon: logon.to_string(),
        from: from_callsign.to_string(),
        to: to_station.lock().expect("hoppie to_station mutex").clone(),
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
                thread,
                telex_log,
                min_meta,
                from_callsign,
                logon,
                to_station,
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

    // --- handover_sender_is_authorized ---

    #[test]
    fn handover_from_the_currently_connected_facility_is_authorized() {
        assert!(handover_sender_is_authorized("EDGG", "EDGG"));
        // Case-insensitive, whitespace-tolerant — matches how callsigns
        // are compared elsewhere in this codebase.
        assert!(handover_sender_is_authorized("edgg", " EDGG "));
    }

    #[test]
    fn handover_from_any_other_sender_is_rejected() {
        // The exact bug this closes: any Hoppie account could address a
        // HANDOVER packet claiming to be from anyone at all.
        assert!(!handover_sender_is_authorized("LBSR", "EDGG"));
        assert!(!handover_sender_is_authorized("EDGG_2", "EDGG"));
    }

    #[test]
    fn handover_is_never_authorized_before_any_station_is_connected() {
        // An empty current_station means "not connected to anyone yet" —
        // nothing should ever be authorized to redirect a session that
        // doesn't exist.
        assert!(!handover_sender_is_authorized("EDGG", ""));
        assert!(!handover_sender_is_authorized("", ""));
    }

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
        assert_eq!(parsed.len(), 3, "the real parser must find all three envelopes");

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
        assert_eq!(decoded[1].element_text, "CURRENT ATC UNIT _ LBSR _ SOFIA CTR");
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
        assert_eq!(reordered, batch, "no reordering needed when nothing is NE/N");
    }

    #[test]
    fn two_no_reply_messages_keep_their_original_relative_order() {
        let batch = vec![
            cpdlc_env("EDGG", 1, "NE", "LOGON ACCEPTED"),
            cpdlc_env("EDGG", 2, "N", "MONITOR UNICOM 122.8"),
        ];
        let reordered = reorder_same_batch_envelopes(batch.clone());
        assert_eq!(reordered, batch, "both are already 'no reply owed' and in order");
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
        assert_eq!(reordered, batch, "telex present → whole batch stays as received");
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
        // HANDOVER has real side effects beyond the message log (flips
        // to_station, logs off, fires a new REQUEST LOGON) that run in
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
        assert_eq!(mins, vec![20, 21], "already in the desired order, and not blocked");
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
            "x", 1, "NE", "LOGON ACCEPTED"
        )));
        assert!(!requires_reply_for_batch_order(&cpdlc_env(
            "x", 1, "N", "MONITOR UNICOM 122.8"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x", 1, "WU", "CLIMB TO FL240"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x", 1, "AN", "CONFIRM ASSIGNED ALTITUDE"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x", 1, "R", "ROGER TEST"
        )));
        assert!(requires_reply_for_batch_order(&cpdlc_env(
            "x", 1, "Y", "FREE TEXT"
        )));
    }
}
