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

use super::{HoppieHttp, TelexEntry};

/// Baseline poll interval — midpoint of the official docs' recommended
/// 45-75s band (`hoppie.nl/acars/system/tech.html`: "heavily
/// recommended to poll once between every 45 and 75 seconds, randomly
/// timed"). Randomizing within the band is a documented nice-to-have,
/// not implemented in Phase 1 — a fixed 60s is well within spec.
const BASELINE_POLL_SECS: u64 = 60;

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
        Duration::from_secs(BASELINE_POLL_SECS)
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
    min_timestamps: Arc<StdMutex<std::collections::HashMap<u32, chrono::DateTime<chrono::Utc>>>>,
    last_error: Arc<StdMutex<Option<String>>>,
    from_callsign: String,
    logon: String,
    to_station: String,
    notify_os: bool,
    mut stop_rx: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        loop {
            let interval = if http.mock {
                // Simulation mode: poll quickly (fixed 3s) so a pilot
                // testing the UI sees the canned PDC reply land almost
                // immediately, rather than waiting up to the real
                // baseline's 60s.
                Duration::from_secs(3)
            } else {
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
                    poll_once(&app, &http, &thread, &telex_log, &min_timestamps, &last_error, &from_callsign, &logon, &to_station, notify_os).await;
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

#[allow(clippy::too_many_arguments)]
async fn poll_once(
    app: &AppHandle,
    http: &HoppieHttp,
    thread: &StdMutex<CpdlcThread>,
    telex_log: &StdMutex<Vec<TelexEntry>>,
    min_timestamps: &StdMutex<std::collections::HashMap<u32, chrono::DateTime<chrono::Utc>>>,
    last_error: &StdMutex<Option<String>>,
    from_callsign: &str,
    logon: &str,
    to_station: &str,
    notify_os: bool,
) {
    let req = HoppieRequest {
        logon: logon.to_string(),
        from: from_callsign.to_string(),
        to: to_station.to_string(),
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
                        });
                    if notify_os {
                        notify_new_message(app, &from);
                    }
                    continue;
                }
                match cpdlc::decode(&env.packet, Direction::Uplink) {
                    Ok(msg) => {
                        let min = msg.min;
                        let mut t = thread.lock().expect("hoppie thread mutex");
                        t.record_received(msg);
                        drop(t);
                        min_timestamps
                            .lock()
                            .expect("hoppie min_timestamps mutex")
                            .insert(min, chrono::Utc::now());
                        if notify_os {
                            notify_new_message(app, &env.from);
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
            tracing::warn!(reason = %reason, "hoppie: poll rejected");
            *last_error.lock().expect("hoppie last_error mutex") = Some(reason);
        }
        Err(e) => {
            tracing::warn!(error = %e.message, "hoppie: poll request failed");
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
        assert_eq!(poll_interval(0), Duration::from_secs(BASELINE_POLL_SECS));
    }
}
