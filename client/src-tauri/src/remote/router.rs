//! axum `Router` assembly + the HTTP/WS handlers + LAN-safety middleware.
//!
//! Route map:
//!
//! ```text
//! POST /api/auth          {pin}              → {token} | 401 | 429
//! POST /api/cmd/{name}     <named args json>  → 200 Ok | 422 UiError | 404
//! GET  /ws?token=…         (WebSocket)         → push stream
//! GET  /*path              (SPA via embedded asset_resolver,
//!                           fallback index.html for deep-links)
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
    http::{header, HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};
use tower_http::cors::CorsLayer;

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

/// Resolve the on-disk directory the SPA *might* live in, used ONLY as the
/// dev / `cargo run` fallback when the frontend is not embedded.
///
/// In a PACKAGED build the frontend is embedded in the binary (served via
/// [`AppHandle::asset_resolver`] in [`serve_spa`]), so this path is not
/// hit. `tauri.conf.json` sets `frontendDist: "../dist"`; in dev those
/// files live next to the crate. We probe the bundled resource dir first
/// (harmless — usually empty for the frontend in Tauri 2), then fall back
/// to the dev path relative to `CARGO_MANIFEST_DIR`. Returns the first
/// candidate that contains an `index.html`.
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
    // Last resort: the dev path even if not yet built — the FS fallback in
    // `serve_spa` will just 404 until `npm run build` has produced it.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dist")
}

// ----------------------------------------------------------------------
// SPA serving (embedded assets, FS dev fallback)
// ----------------------------------------------------------------------

/// Serve the SPA for an unmatched (non-`/api`, non-`/ws`) request.
///
/// PRIMARY path — the EMBEDDED assets via [`AppHandle::asset_resolver`].
/// In a packaged Tauri app the frontend is compiled into the binary (via
/// `generate_context!`), NOT shipped as loose files, so a filesystem
/// `ServeDir` finds nothing and the tablet gets a blank page. The asset
/// resolver reads the bundled bytes instead.
///
/// Tauri's resolver keys assets WITHOUT a leading slash (it strips one
/// internally) and maps the empty path to `index.html`; it also already
/// falls back to `index.html` for an unknown path, so a SPA deep-link like
/// `/logbook` resolves to the app shell and the client router takes over.
/// We pass the request path as-is — the resolver normalizes it — and only
/// 404 if even `index.html` is absent (no frontend embedded at all).
///
/// DEV fallback — when the resolver yields nothing (e.g. `tauri dev` with
/// `devUrl` set and no embedded assets, where the resolver may not find
/// the file), read from the on-disk `spa_dir` so `cargo run` still serves
/// a UI after `npm run build`.
async fn serve_spa(app: &AppHandle, spa_dir: &std::path::Path, uri: Uri) -> Response {
    let path = uri.path();

    // 1) Embedded assets (production). The resolver strips a leading slash
    //    and maps "" → index.html, so the raw request path works directly;
    //    it also falls back to index.html for SPA deep-links itself.
    if let Some(asset) = app.asset_resolver().get(path.to_string()) {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, asset.mime_type)],
            asset.bytes,
        )
            .into_response();
    }

    // 2) Dev / `cargo run` FS fallback. Resolve the on-disk file; for an
    //    unmatched path (SPA deep-link) serve index.html so the client
    //    router handles it.
    if let Some(resp) = serve_from_fs(spa_dir, path).await {
        return resp;
    }

    // 3) Nothing embedded and nothing on disk — genuine miss.
    (StatusCode::NOT_FOUND, "not found").into_response()
}

/// Read the requested file from the on-disk `spa_dir` (dev fallback only).
/// Returns the file bytes for a real file, the `index.html` shell for an
/// unmatched path (SPA deep-link), or `None` when even `index.html` is
/// absent on disk. The path is sanitized to its file components so a
/// `../` traversal can't escape `spa_dir`.
async fn serve_from_fs(spa_dir: &std::path::Path, req_path: &str) -> Option<Response> {
    // Strip the leading slash and drop any non-`Normal` components
    // (RootDir / ParentDir / CurDir) so the path can't escape spa_dir.
    let rel: PathBuf = std::path::Path::new(req_path.trim_start_matches('/'))
        .components()
        .filter(|c| matches!(c, std::path::Component::Normal(_)))
        .collect();

    let candidate = spa_dir.join(&rel);
    if rel.as_os_str().is_empty() || !candidate.is_file() {
        // Directory request or unknown path → SPA shell (deep-link).
        let index = spa_dir.join("index.html");
        let bytes = tokio::fs::read(&index).await.ok()?;
        return Some(
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "text/html")],
                bytes,
            )
                .into_response(),
        );
    }

    let bytes = tokio::fs::read(&candidate).await.ok()?;
    let mime = mime_for_ext(&candidate);
    Some((StatusCode::OK, [(header::CONTENT_TYPE, mime)], bytes).into_response())
}

/// Minimal extension → MIME mapping for the dev FS fallback. The packaged
/// build never hits this (the asset resolver supplies the real MIME); this
/// only needs to cover the handful of types a Vite build emits so a JS/CSS
/// asset is served with a content-type the browser executes.
fn mime_for_ext(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html",
        Some("js") | Some("mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("wasm") => "application/wasm",
        Some("map") => "application/json",
        _ => "application/octet-stream",
    }
}

// ----------------------------------------------------------------------
// Router assembly
// ----------------------------------------------------------------------

/// Build the full axum `Router` for the LAN server.
pub fn build_router(ctx: RemoteContext, spa_dir: PathBuf) -> Router {
    // SPA fallback: in a PACKAGED build the frontend is EMBEDDED in the
    // binary (via `generate_context!`), NOT shipped as loose files, so
    // `ServeDir` on `spa_dir` finds nothing → blank page. We serve the
    // embedded assets via `app.asset_resolver()` instead, keeping `spa_dir`
    // only as the dev fallback (where assets aren't embedded). The handler
    // captures `spa_dir` and reads the `AppHandle` from the shared
    // `RemoteContext` router state.
    let dev_spa_dir = spa_dir;
    let spa_fallback = move |state: State<RemoteContext>, uri: Uri| {
        let dev_spa_dir = dev_spa_dir.clone();
        async move { serve_spa(&state.0.app, &dev_spa_dir, uri).await }
    };

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
        // SPA + static assets (also the unmatched-route fallback). Served
        // from the EMBEDDED assets; a deep-link with no asset falls back to
        // index.html so the client router handles it.
        .fallback(spa_fallback)
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
