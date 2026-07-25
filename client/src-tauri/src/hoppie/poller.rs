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

use super::{HoppieHttp, MinTimestamps, TelexEntry};
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
    min_timestamps: Arc<StdMutex<MinTimestamps>>,
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
                    poll_once(&app, &http, &thread, &telex_log, &min_timestamps, &last_error, &from_callsign, &logon, &to_station, notify_os && !first_poll).await;
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
    let next = text.trim().strip_prefix("HANDOVER")?.trim();
    // Guard against a message that merely starts with the word (e.g. a
    // free-text remark) — a real handover carries exactly one token.
    let mut parts = next.split_whitespace();
    let station = parts.next()?;
    if parts.next().is_some() || station.is_empty() {
        return None;
    }
    Some(station.to_uppercase())
}

/// Fire `REQUEST LOGON` at `station` as part of an automatic handover.
/// Best-effort: a failure here leaves the pilot on the old facility with
/// a visible "not logged on" state rather than silently pretending.
async fn send_logon(
    http: &HoppieHttp,
    thread: &StdMutex<CpdlcThread>,
    min_timestamps: &StdMutex<MinTimestamps>,
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
    min_timestamps
        .lock()
        .expect("hoppie min_timestamps mutex")
        .insert((false, min), chrono::Utc::now());

    let req = HoppieRequest {
        logon: logon.to_string(),
        from: from_callsign.to_string(),
        to: station.to_string(),
        kind: PacketKind::Cpdlc,
        packet: Some(cpdlc::encode(&message)),
    };
    if let Err(e) = http.send(&req).await {
        tracing::warn!(error = %e.message, station = %station, "hoppie: handover logon failed");
    } else {
        tracing::info!(station = %station, "hoppie: handed over to next facility");
    }
}

#[allow(clippy::too_many_arguments)]
async fn poll_once(
    app: &AppHandle,
    http: &HoppieHttp,
    thread: &StdMutex<CpdlcThread>,
    telex_log: &StdMutex<Vec<TelexEntry>>,
    min_timestamps: &StdMutex<MinTimestamps>,
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
            let envelopes = wire::parse_poll_envelopes(&content);
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
                            send_logon(http, thread, min_timestamps, from_callsign, logon, &next)
                                .await;
                            continue;
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
                            let station =
                                to_station.lock().expect("hoppie to_station mutex").clone();
                            crate::hoppie::settings::set_open_session(app, &station);
                        }
                        min_timestamps
                            .lock()
                            .expect("hoppie min_timestamps mutex")
                            .insert((true, min), chrono::Utc::now());
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
}
