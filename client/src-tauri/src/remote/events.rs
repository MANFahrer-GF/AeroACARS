//! WebSocket push channel for the LAN remote-control server.
//!
//! A connected tablet opens `GET /ws?token=…`. After auth (token query +
//! private-peer + Origin checks + the WS connection-cap permit, all done
//! in [`crate::remote::router`] BEFORE the upgrade) we run one task per
//! socket that fans the broadcast bus to the client as
//! `{"event":<name>,"payload":<json>}` frames:
//!
//! - **the three Tauri push events** — `integrity-flag`,
//!   `pirep_auto_filed`, `pirep_cancelled_remotely` — and
//! - **the `flight_status` tick** — produced by the ONE shared 1 Hz timer
//!   ([`crate::remote::spawn_flight_status_ticker`]), not by this task.
//!
//! All of these arrive on the same process-wide `broadcast` channel
//! ([`crate::remote::RemoteEventBus`]); this task just subscribes and
//! forwards, so `flight_status` is computed once per second *total* rather
//! than once per second *per connection* (the previous amplifier).

use axum::extract::ws::{Message, WebSocket};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::OwnedSemaphorePermit;

use crate::remote::{RemoteContext, RemoteEvent};

/// Drive one authenticated WebSocket connection until it closes.
///
/// `_permit` is the WS-connection-cap slot acquired in the router before
/// the upgrade; holding it for the life of this task is what bounds
/// concurrent sessions — it is released when the task ends (disconnect).
///
/// Runs two concurrent concerns in a single `select!` loop:
/// - forward broadcast events (the 3 emit events + the shared
///   `flight_status` tick),
/// - drain inbound frames (we ignore payloads but must read so the
///   close/ping handshake works; a `Close` or read error ends the loop).
pub async fn handle_socket(
    ctx: RemoteContext,
    mut socket: WebSocket,
    _permit: OwnedSemaphorePermit,
) {
    let mut events = ctx.events.subscribe();

    // Send an immediate status frame so a freshly-connected tablet renders
    // the live panel without waiting for the next shared tick.
    let initial = crate::remote::current_flight_status_value(&ctx.app);
    let initial_json = initial.to_string();
    if send_event(&mut socket, crate::remote::FLIGHT_STATUS_EVENT, &initial_json)
        .await
        .is_err()
    {
        return;
    }

    loop {
        tokio::select! {
            // --- inbound: detect close / drain pings ---
            inbound = socket.recv() => {
                match inbound {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => { /* ignore client payloads */ }
                    Some(Err(_)) => break,
                }
            }

            // --- broadcast: the 3 push events + the shared flight_status tick ---
            evt = events.recv() => {
                match evt {
                    Ok(RemoteEvent { event, payload }) => {
                        let body = payload.to_string();
                        if send_event(&mut socket, &event, &body).await.is_err() {
                            break;
                        }
                    }
                    // A slow client lagged past the buffer — skip the
                    // dropped events and keep going rather than dropping
                    // the whole connection.
                    Err(RecvError::Lagged(_)) => continue,
                    Err(RecvError::Closed) => {
                        // The bus is gone (app shutting down) — close.
                        break;
                    }
                }
            }
        }
    }
}

/// Send one `{"event":name,"payload":<raw json>}` frame. `payload_json`
/// is already-serialized JSON text, embedded verbatim so we don't
/// double-encode it.
async fn send_event(
    socket: &mut WebSocket,
    name: &str,
    payload_json: &str,
) -> Result<(), axum::Error> {
    // Build the envelope by hand to avoid re-parsing+re-serializing the
    // already-valid payload JSON.
    let escaped_name = escape_json_string(name);
    let frame = format!("{{\"event\":\"{escaped_name}\",\"payload\":{payload_json}}}");
    socket.send(Message::Text(frame.into())).await
}

/// Escape a string for embedding inside JSON double-quotes. Event names
/// are fixed ASCII identifiers, but escape defensively anyway.
fn escape_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_json_special_chars() {
        assert_eq!(escape_json_string("flight_status"), "flight_status");
        assert_eq!(escape_json_string("a\"b"), "a\\\"b");
        assert_eq!(escape_json_string("a\\b"), "a\\\\b");
        assert_eq!(escape_json_string("a\nb"), "a\\nb");
    }

    #[test]
    fn frame_envelope_is_valid_json() {
        // Mirror what send_event builds, then re-parse to prove validity.
        let name = escape_json_string("integrity-flag");
        let payload = r#"{"session_id":"abc","flag":true}"#;
        let frame = format!("{{\"event\":\"{name}\",\"payload\":{payload}}}");
        let v: serde_json::Value = serde_json::from_str(&frame).unwrap();
        assert_eq!(v["event"], "integrity-flag");
        assert_eq!(v["payload"]["session_id"], "abc");
        assert_eq!(v["payload"]["flag"], true);
    }
}
