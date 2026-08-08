//! Always-on, loopback-only local server for the MSFS in-sim panel
//! (v0.2.0 #msfs-panel, round 2b, 2026-08-08).
//!
//! Independent of the opt-in LAN Remote Control server (`remote/mod.rs`):
//! different threat model (this panel only ever runs on the SAME PC as
//! AeroACARS, a tablet is a genuinely different device on the LAN), so it
//! gets its own **fixed, non-configurable port** and **starts
//! unconditionally with the app**, no settings toggle.
//!
//! This replaces an earlier round-2 design that put `/panel/*` routes on
//! the SAME server/port as the tablet feature. That had two real bugs,
//! both caught live by Thomas testing round 2: (1) that server is opt-in
//! (`remote_server_start`) — the panel sat on "Verbinde..." forever unless
//! the pilot separately toggled "Fernzugriff" on, silently reintroducing
//! the "needs configuration" friction PIN removal was supposed to kill;
//! (2) that server's port is pilot-configurable
//! (`remote_server_set_port`) — changing it in Settings silently broke the
//! panel, which had no way to learn about the change. A fixed, dedicated,
//! always-on port sidesteps both: nothing to toggle, nothing to
//! mismatch.
//!
//! Binds literally to `127.0.0.1` (not `0.0.0.0`) — genuinely unreachable
//! from the LAN at the socket level, not just via an app-level peer
//! check. [`reject_non_loopback`] below is still applied per-request as
//! belt-and-suspenders, mirroring `remote/router.rs`'s own style, even
//! though the bind address alone already rules out a non-loopback peer.
//!
//! Deliberately minimal: no auth, no rate limiting, no connection caps,
//! no CORS middleware stack — none of that machinery is meaningful for a
//! read-only, loopback-only, unauthenticated route set with no ambient
//! credential to protect. That's also why this does NOT reuse
//! `remote/router.rs`'s `LimitedListener`/`PeerAddr` (slowloris
//! mitigation sized for a LAN-reachable, potentially hostile-peer server)
//! — a plain `TcpListener` bound to loopback has a categorically smaller
//! attack surface, and axum's built-in `SocketAddr` `Connected` impl is
//! sufficient.

use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        ConnectInfo, State,
    },
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::remote::net;

/// Fixed port for the panel server. Not user-configurable — see the
/// module doc for why that's the point, not an oversight. Chosen distinct
/// from the LAN Remote Control server's default (8765) specifically so
/// the two are never confusable.
pub const PANEL_SERVER_PORT: u16 = 47847;

const TICK: Duration = Duration::from_secs(1);

#[derive(Clone)]
struct PanelCtx {
    app: AppHandle,
}

/// Spawn the panel server for the lifetime of the app. Call exactly once,
/// at startup, unconditionally — mirrors how the auto-start watcher is
/// spawned once in `lib.rs`'s `setup()` hook regardless of whether its
/// underlying feature is toggled on, because the *spawn* isn't the opt-in
/// part; here there is no opt-in part at all.
///
/// A bind failure (something else already holding the port) is logged and
/// swallowed, not propagated — the MSFS panel simply won't connect this
/// session, which is a strictly better failure mode than taking the whole
/// app down over a feature most pilots will never open.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let addr = SocketAddr::from(([127, 0, 0, 1], PANEL_SERVER_PORT));
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    port = PANEL_SERVER_PORT,
                    "panel_server: bind failed — MSFS in-sim panel will not be able to connect this session"
                );
                return;
            }
        };
        let ctx = PanelCtx { app };
        let router = Router::new()
            .route("/panel/status", get(status_handler))
            .route("/panel/debrief", get(debrief_handler))
            .route("/panel/ws", get(ws_handler))
            .with_state(ctx);
        if let Err(e) = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            tracing::warn!(error = %e, "panel_server: serve loop ended unexpectedly");
        }
    });
}

fn reject_non_loopback(peer: SocketAddr) -> Option<Response> {
    if net::is_loopback_socket(peer) {
        None
    } else {
        tracing::warn!(%peer, "panel_server: rejected non-loopback peer");
        Some((StatusCode::FORBIDDEN, "forbidden: loopback only").into_response())
    }
}

/// The panel is loaded from `file://` inside Coherent GT, always
/// cross-origin from this server's perspective — see
/// `remote/router.rs::cors_open`'s doc for the identical reasoning. Open
/// ACAO costs nothing here: there is no token/ambient credential for CORS
/// to meaningfully protect.
fn cors_open(mut resp: Response) -> Response {
    resp.headers_mut()
        .insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, HeaderValue::from_static("*"));
    resp
}

async fn status_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    let value = crate::remote::current_flight_status_value(&ctx.app);
    cors_open((StatusCode::OK, Json(value)).into_response())
}

async fn debrief_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    let state = ctx.app.state::<crate::AppState>();
    let record = crate::landing_get_current(ctx.app.clone(), state);
    let value = serde_json::to_value(record).unwrap_or(Value::Null);
    cors_open((StatusCode::OK, Json(value)).into_response())
}

async fn ws_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    upgrade.on_upgrade(move |socket| handle_socket(ctx, socket))
}

/// Self-contained 1Hz push loop, independent of `remote::RemoteEventBus`
/// (which only exists while the opt-in tablet server is running — exactly
/// the coupling this module exists to avoid). In practice there is only
/// ever one MSFS instance/panel connection at a time, so an
/// intentionally simple per-connection ticker (rather than a shared
/// broadcast bus) costs nothing and keeps this module fully independent
/// of `remote/`.
async fn handle_socket(ctx: PanelCtx, mut socket: WebSocket) {
    let mut ticker = tokio::time::interval(TICK);
    let mut last: Option<Value> = None;
    loop {
        tokio::select! {
            _ = ticker.tick() => {
                let value = crate::remote::current_flight_status_value(&ctx.app);
                if last.as_ref() != Some(&value) {
                    last = Some(value.clone());
                    let frame = serde_json::json!({ "event": "flight_status", "payload": value }).to_string();
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        break;
                    }
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Err(_)) => break,
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_lan_peers_a_full_remote_route_would_allow() {
        // The whole point of this module vs. reusing remote/router.rs: a
        // LAN tablet is a legitimate /api peer there but must NOT reach
        // the panel server at all.
        let lan_peer: SocketAddr = "192.168.1.5:5000".parse().unwrap();
        assert!(reject_non_loopback(lan_peer).is_some());

        let loop_peer: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        assert!(reject_non_loopback(loop_peer).is_none());
    }

    #[test]
    fn panel_server_port_is_distinct_from_lan_remote_default() {
        // Regression guard for the exact bug this module exists to fix:
        // the panel's port must never accidentally end up equal to (or
        // dependent on) the LAN Remote Control server's configurable
        // default — see the module doc.
        assert_ne!(PANEL_SERVER_PORT, 8765);
    }
}
