//! axum `Router` assembly + the HTTP/WS handlers + LAN-safety middleware.
//!
//! Route map:
//!
//! ```text
//! POST /api/auth          {pin}              → {token} | 401 | 429
//! POST /api/cmd/{name}     <named args json>  → 200 Ok | 422 UiError | 404
//! GET  /ws?token=…         (WebSocket)         → push stream
//! GET  /*path              (SPA via ServeDir, fallback index.html)
//! ```
//!
//! Defence in depth applied to **every** `/api` + `/ws` request:
//! 1. the peer's socket IP must be private/loopback ([`net::is_private_socket`]),
//! 2. the bearer token must match (header on `/api/cmd`, query on `/ws`),
//! 3. a strict same-origin `CorsLayer`, and the WS upgrade additionally
//!    refuses a foreign `Origin` header.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    body::Bytes,
    extract::{ws::WebSocketUpgrade, ConnectInfo, Path as AxumPath, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

use crate::remote::{auth::AuthError, bridge, events, net, RemoteContext};

/// Header the tablet sends the bearer token in on `/api/cmd/*`.
const TOKEN_HEADER: &str = "X-AeroACARS-Token";

// ----------------------------------------------------------------------
// Listener binding
// ----------------------------------------------------------------------

/// Bind a `0.0.0.0:<port>` TCP listener, mapping `EADDRINUSE` (and any
/// other bind error) to a `std::io::Error` the caller turns into a clean
/// user-facing `UiError`.
pub async fn bind(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tokio::net::TcpListener::bind(addr).await
}

// ----------------------------------------------------------------------
// SPA directory resolution
// ----------------------------------------------------------------------

/// Resolve the on-disk directory Tauri loads the SPA from, for `ServeDir`.
///
/// `tauri.conf.json` sets `frontendDist: "../dist"`. In a bundled build
/// those files are copied into the platform resource dir; in `cargo
/// run`/dev they live next to the crate. We probe the bundled resource
/// dir first, then fall back to the dev path relative to `CARGO_MANIFEST_DIR`.
/// Returns the first candidate that contains an `index.html`.
pub fn resolve_spa_dir(app: &AppHandle) -> PathBuf {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(res) = app.path().resource_dir() {
        // Tauri lays the frontend out under the resource dir; the exact
        // sub-path varies by platform/version, so probe the common ones.
        candidates.push(res.join("dist"));
        candidates.push(res.join("_up_/dist")); // tauri's escaped "../dist"
        candidates.push(res.clone());
    }
    // Dev / `cargo run` fallback: <crate>/../dist.
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist"));

    for c in &candidates {
        if c.join("index.html").is_file() {
            return c.clone();
        }
    }
    // Last resort: the dev path even if not yet built — ServeDir will just
    // 404 until `npm run build` has produced it.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist")
}

// ----------------------------------------------------------------------
// Router assembly
// ----------------------------------------------------------------------

/// Build the full axum `Router` for the LAN server.
pub fn build_router(ctx: RemoteContext, spa_dir: PathBuf) -> Router {
    // SPA: serve files; for any unmatched path fall back to index.html so
    // the client-side router handles deep links (e.g. /logbook).
    let index = spa_dir.join("index.html");
    let serve_dir = ServeDir::new(&spa_dir).fallback(ServeFile::new(index));

    Router::new()
        .route("/api/auth", post(auth_handler))
        .route("/api/cmd/{name}", post(cmd_handler))
        // SECURITY: the `/ws` route authenticates via the `?token=` query
        // parameter (a WebSocket upgrade can't carry a custom header from a
        // browser). That token therefore lives in the request URI. NO
        // request-URI / access-logging middleware (e.g. `TraceLayer` with
        // URI capture, or any tower logger that records the path+query) may
        // EVER be added to this router — it would write the bearer token to
        // logs. The token transport itself must stay as-is; do not "fix"
        // this by logging and redacting.
        .route("/ws", get(ws_handler))
        // SPA + static assets (also the unmatched-route fallback).
        .fallback_service(serve_dir)
        // CORS note: a default (deny-all-cross-origin) `CorsLayer` is
        // applied, but CORS is NOT the security boundary here. CORS only
        // governs whether a *browser* exposes a cross-origin response to
        // scripts; it does not stop a request from reaching+executing on
        // the server (a non-browser client ignores CORS entirely). The
        // REAL protection for `/api` is the bearer token (checked on every
        // `/api/cmd` + `/ws` request) plus the private-LAN peer check. The
        // CorsLayer is just belt-and-suspenders for the same-origin SPA.
        .layer(CorsLayer::new())
        .with_state(ctx)
}

// ----------------------------------------------------------------------
// Shared guards
// ----------------------------------------------------------------------

/// Reject a peer that is not on the private LAN / loopback. Returns the
/// 403 response to short-circuit with, or `None` to proceed.
fn reject_non_private(peer: SocketAddr) -> Option<Response> {
    if net::is_private_socket(peer) {
        None
    } else {
        tracing::warn!(%peer, "remote: rejected non-private peer");
        Some((StatusCode::FORBIDDEN, "forbidden: LAN only").into_response())
    }
}

/// Extract + verify the bearer token from the `X-AeroACARS-Token` header.
fn header_token_ok(ctx: &RemoteContext, headers: &HeaderMap) -> bool {
    headers
        .get(TOKEN_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|t| ctx.auth.verify_token(t))
        .unwrap_or(false)
}

// ----------------------------------------------------------------------
// POST /api/auth
// ----------------------------------------------------------------------

#[derive(Deserialize)]
struct AuthBody {
    pin: String,
}

async fn auth_handler(
    State(ctx): State<RemoteContext>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<AuthBody>,
) -> Response {
    if let Some(r) = reject_non_private(peer) {
        return r;
    }
    // Rate-limit keyed by the peer's IP so one hostile LAN device can't
    // lock out pairing from a legitimate tablet on a different IP.
    match ctx.auth.try_pin(peer.ip(), &body.pin) {
        Ok(token) => (StatusCode::OK, Json(json!({ "token": token }))).into_response(),
        Err(AuthError::BadPin) => {
            (StatusCode::UNAUTHORIZED, Json(json!({ "error": "bad_pin" }))).into_response()
        }
        Err(AuthError::RateLimited) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "rate_limited" })),
        )
            .into_response(),
    }
}

// ----------------------------------------------------------------------
// POST /api/cmd/{name}
// ----------------------------------------------------------------------

async fn cmd_handler(
    State(ctx): State<RemoteContext>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    AxumPath(name): AxumPath<String>,
    headers: HeaderMap,
    raw: Bytes,
) -> Response {
    if let Some(r) = reject_non_private(peer) {
        return r;
    }
    if !header_token_ok(&ctx, &headers) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }

    // Parse the body as a JSON object of named args. An empty body (no
    // args) is treated as `{}`.
    let body: Value = if raw.is_empty() {
        json!({})
    } else {
        match serde_json::from_slice(&raw) {
            Ok(v @ Value::Object(_)) => v,
            Ok(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "code": "bad_request", "message": "args must be a JSON object" })),
                )
                    .into_response();
            }
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "code": "bad_request", "message": format!("invalid JSON: {e}") })),
                )
                    .into_response();
            }
        }
    };

    match bridge::dispatch(&ctx, &name, &body).await {
        bridge::Dispatch::Handled(Ok(value)) => (StatusCode::OK, Json(value)).into_response(),
        bridge::Dispatch::Handled(Err(ui)) => {
            // 422 with the {code,message} body — the same shape Tauri's
            // `invoke` rejects with, so the SPA's `.catch` is unchanged.
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({ "code": ui.code, "message": ui.message })),
            )
                .into_response()
        }
        bridge::Dispatch::Unknown => (
            StatusCode::NOT_FOUND,
            Json(json!({ "code": "unknown_command", "message": format!("unknown command: {name}") })),
        )
            .into_response(),
    }
}

// ----------------------------------------------------------------------
// GET /ws?token=…
// ----------------------------------------------------------------------

#[derive(Deserialize)]
struct WsQuery {
    token: String,
}

async fn ws_handler(
    State(ctx): State<RemoteContext>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if let Some(r) = reject_non_private(peer) {
        return r;
    }
    if !ctx.auth.verify_token(&q.token) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    // Refuse a foreign-Origin upgrade. A same-origin browser sends
    // `Origin: http://<host>`; absence (a non-browser WS client) is
    // allowed, but a *present* Origin whose host differs from the request
    // Host is a cross-site attempt and is rejected.
    if let Some(resp) = reject_foreign_origin(&headers) {
        return resp;
    }
    // Bound concurrent WS sessions. We acquire the permit BEFORE the
    // upgrade so an over-cap client is refused cleanly (no upgrade,
    // 503) rather than upgraded-then-dropped. The permit is held for the
    // life of the socket and released on disconnect.
    let Ok(permit) = Arc::clone(&ctx.ws_slots).try_acquire_owned() else {
        tracing::warn!(%peer, "remote: WS connection cap reached — refusing upgrade");
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "too many active remote connections",
        )
            .into_response();
    };

    let ctx2 = ctx.clone();
    upgrade.on_upgrade(move |socket| events::handle_socket(ctx2, socket, permit))
}

/// Reject a WS upgrade whose `Origin` host differs from the `Host` header.
/// Returns `Some(403)` to reject, `None` to allow.
fn reject_foreign_origin(headers: &HeaderMap) -> Option<Response> {
    let origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let Some(origin) = origin else {
        // No Origin (native WS client) — allowed; token already verified.
        return None;
    };
    let host = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    // Compare the Origin's host[:port] against the Host header.
    let origin_host = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .unwrap_or(origin);

    if origin_host == host {
        None
    } else {
        tracing::warn!(%origin, %host, "remote: rejected foreign-Origin WS upgrade");
        Some((StatusCode::FORBIDDEN, "forbidden origin").into_response())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rejects_public_peer_at_socket() {
        let pub_peer: SocketAddr = "8.8.8.8:5000".parse().unwrap();
        assert!(reject_non_private(pub_peer).is_some());
        let lan_peer: SocketAddr = "192.168.1.5:5000".parse().unwrap();
        assert!(reject_non_private(lan_peer).is_none());
    }

    #[test]
    fn foreign_origin_rejected_same_allowed() {
        let mut same = HeaderMap::new();
        same.insert(header::HOST, "192.168.1.10:8765".parse().unwrap());
        same.insert(
            header::ORIGIN,
            "http://192.168.1.10:8765".parse().unwrap(),
        );
        assert!(reject_foreign_origin(&same).is_none());

        let mut foreign = HeaderMap::new();
        foreign.insert(header::HOST, "192.168.1.10:8765".parse().unwrap());
        foreign.insert(header::ORIGIN, "http://evil.example".parse().unwrap());
        assert!(reject_foreign_origin(&foreign).is_some());

        // No Origin header → allowed (native client).
        let none = HeaderMap::new();
        assert!(reject_foreign_origin(&none).is_none());
    }

    #[test]
    fn resolve_spa_dir_dev_fallback_path_shape() {
        // We can't build an AppHandle in a unit test, but the dev fallback
        // is a pure path join we can assert independently.
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
        assert!(dev.ends_with("dist"));
    }
}
