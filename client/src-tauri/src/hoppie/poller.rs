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

use super::HoppieHttp;

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
    _app: AppHandle,
    http: Arc<HoppieHttp>,
    thread: Arc<StdMutex<CpdlcThread>>,
    last_error: Arc<StdMutex<Option<String>>>,
    from_callsign: String,
    logon: String,
    to_station: String,
    mut stop_rx: watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
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
                    poll_once(&http, &thread, &last_error, &from_callsign, &logon, &to_station).await;
                }
            }
        }
        tracing::debug!("hoppie: poller stopped");
    });
}

async fn poll_once(
    http: &HoppieHttp,
    thread: &StdMutex<CpdlcThread>,
    last_error: &StdMutex<Option<String>>,
    from_callsign: &str,
    logon: &str,
    to_station: &str,
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
            let mut t = thread.lock().expect("hoppie thread mutex");
            for env in envelopes {
                if env.kind != PacketKind::Cpdlc {
                    // Phase 1: telex/PDC-reply envelopes aren't stored
                    // yet — `hoppie_get_thread` (Phase 2) is what will
                    // surface them. Nothing sends a PDC request before
                    // Phase 2 either, so nothing meaningful is lost.
                    continue;
                }
                match cpdlc::decode(&env.packet, Direction::Uplink) {
                    Ok(msg) => {
                        t.record_received(msg);
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
