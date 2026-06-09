//! LAN remote-control server (v0.16.0, #LAN-Remote).
//!
//! AeroACARS runs on the sim PC. This module lets a pilot drive the
//! *real* flight from a tablet or a second PC on the same local network:
//! it serves the genuine React SPA over HTTP and bridges every safe
//! Tauri command to that browser over `POST /api/cmd/{name}` + a WS push
//! channel, all PIN-gated.
//!
//! ## Why this is safe to bolt onto the existing command layer
//!
//! The ~67 `#[tauri::command]` fns take `(app: AppHandle,
//! state: tauri::State<'_, AppState>, ...args)` and *none* take a
//! `Window`. A command can therefore be called directly from outside the
//! IPC layer via `app.state::<AppState>()` — exactly what the auto-start
//! watcher already does (`flight_start(app, app.state::<AppState>(),
//! bid_id, None)`). The HTTP bridge reuses that same call shape. See
//! [`bridge`].
//!
//! ## Threat model
//!
//! The tablet controls a *real* PIREP, so auth is mandatory on every
//! `/api` + `/ws` route (bearer token from a 6-digit PIN). Defence in
//! depth: peers off the private LAN are rejected at the socket
//! (`ConnectInfo` → [`net::is_private_peer`]), a strict same-host
//! `CorsLayer` is applied, and WS upgrades with a foreign `Origin` are
//! refused. The server is *opt-in*: it is started by the
//! [`remote_server_start`] command, never auto-started.
//!
//! ## Layout
//!
//! - [`auth`]   — PIN/token store, constant-time compare, rate limiter.
//! - [`net`]    — private-range peer check, LAN URL builder, QR SVG.
//! - [`router`] — axum `Router` assembly + middleware.
//! - [`bridge`] — the `POST /api/cmd/{name}` dispatch table.
//! - [`events`] — WS handler: 3 push events + a `flight_status` tick.

pub mod auth;
pub mod bridge;
pub mod events;
pub mod net;
pub mod router;

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tokio::sync::{broadcast, oneshot, Semaphore};

use crate::{AppState, UiError};

/// Default TCP port the LAN server binds when the user has not chosen one.
/// Picked from the unassigned 8765 range. The effective port is
/// **user-configurable + persisted** (see [`read_persisted_port`] /
/// [`write_persisted_port`] and the `remote_server_set_port` command).
pub const DEFAULT_PORT: u16 = 8765;

/// Secrets-store account name for the persisted bearer token. Survives
/// restarts so a paired tablet keeps working without re-pairing (the
/// token lives in the same 0600 `secrets.json` as the API key).
const TOKEN_ACCOUNT: &str = "remote_access_token";

/// Broadcast capacity for the event tap. Small — WS subscribers only
/// need recent events; a slow client that lags past this just sees a
/// `Lagged` skip, which the WS handler tolerates.
const EVENT_CHANNEL_CAP: usize = 64;

/// Hard cap on concurrent WebSocket sessions. The LAN audience is a
/// handful of the pilot's own devices, so this is generous; it exists to
/// bound resource use (and the amplification an unbounded fan-out would
/// allow) rather than to limit normal use. Upgrades beyond this are
/// refused cleanly with a 503.
pub const MAX_WS_CONNECTIONS: usize = 12;

/// Cadence of the single shared `flight_status` tick (one timer total,
/// not one-per-connection). The frame is computed once per second and
/// published into the broadcast bus that every WS already subscribes to.
const FLIGHT_STATUS_TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// Event name used for the periodic `flight_status` push frame.
pub const FLIGHT_STATUS_EVENT: &str = "flight_status";

// ----------------------------------------------------------------------
// Port persistence
// ----------------------------------------------------------------------
//
// Mirrors the existing settings-persistence pattern (`auto_start.json` via
// `app_config_dir()`, lib.rs `read/write_auto_start_persisted`): a tiny
// JSON file in the app config dir. The chosen port survives restarts, so
// `remote_server_start` always binds the port the user last picked.

/// Path of the persisted-port file (`<app_config_dir>/remote_server.json`).
fn port_settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|p| p.join("remote_server.json"))
}

/// Read the persisted remote-server port, or [`DEFAULT_PORT`] if unset /
/// unreadable / out of range.
pub fn read_persisted_port(app: &AppHandle) -> u16 {
    let Some(path) = port_settings_path(app) else {
        return DEFAULT_PORT;
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return DEFAULT_PORT;
    };
    parse_persisted_port(&text)
}

/// Persist `port` to `<app_config_dir>/remote_server.json`. Best-effort —
/// a write failure is logged but not fatal (the chosen port still applies
/// to the running process via `AppState`, it just won't survive restart).
fn write_persisted_port(app: &AppHandle, port: u16) {
    let Some(path) = port_settings_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, serialize_persisted_port(port)) {
        tracing::warn!(error = %e, "remote: failed to persist port");
    }
}

/// Serialize a port to the on-disk JSON body. Pure (testable).
fn serialize_persisted_port(port: u16) -> String {
    format!("{{\"port\":{port}}}")
}

/// Parse the on-disk JSON body back to a port, falling back to
/// [`DEFAULT_PORT`] on malformed/empty/zero input. Pure (testable).
fn parse_persisted_port(text: &str) -> u16 {
    #[derive(serde::Deserialize)]
    struct Stored {
        port: u16,
    }
    match serde_json::from_str::<Stored>(text) {
        Ok(s) if s.port != 0 => s.port,
        _ => DEFAULT_PORT,
    }
}

// ----------------------------------------------------------------------
// Event tap
// ----------------------------------------------------------------------

/// One push frame sent to every connected WS client as
/// `{"event": <name>, "payload": <json>}`. The three `name` values mirror
/// the existing `tauri::Emitter::emit` event names so the SPA's remote
/// transport can register the same listeners it uses under Tauri IPC.
#[derive(Debug, Clone, Serialize)]
pub struct RemoteEvent {
    /// Event name, e.g. `"integrity-flag"`, `"pirep_auto_filed"`,
    /// `"pirep_cancelled_remotely"`, or `"flight_status"` (the tick).
    pub event: String,
    /// Arbitrary JSON payload — the same value the Tauri emit site sends.
    pub payload: serde_json::Value,
}

impl RemoteEvent {
    pub fn new(event: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            event: event.into(),
            payload,
        }
    }
}

/// `Default`-able wrapper around a `broadcast::Sender<RemoteEvent>`.
///
/// `AppState` derives `Default`, but `broadcast::Sender` does not
/// implement `Default`, so we can't add a bare sender field. This newtype
/// creates the channel in its own `Default` impl, keeping the giant
/// `#[derive(Default)] struct AppState` untouched. The tap is *always*
/// live (created at app start) even before the server is running, so the
/// three `emit` sites can fan out unconditionally with one cheap
/// `send` — `send` on a sender with zero receivers is a no-op `Err`
/// we ignore.
#[derive(Debug, Clone)]
pub struct RemoteEventBus {
    sender: broadcast::Sender<RemoteEvent>,
}

impl Default for RemoteEventBus {
    fn default() -> Self {
        let (sender, _rx) = broadcast::channel(EVENT_CHANNEL_CAP);
        Self { sender }
    }
}

impl RemoteEventBus {
    /// Fan out one event to all current WS subscribers. No-op (ignored
    /// `Err`) when nobody is connected — callers at the emit sites use
    /// `let _ = state.remote_events.send(...)`.
    pub fn send(&self, event: RemoteEvent) {
        let _ = self.sender.send(event);
    }

    /// Subscribe a fresh WS connection to the tap.
    pub fn subscribe(&self) -> broadcast::Receiver<RemoteEvent> {
        self.sender.subscribe()
    }
}

// ----------------------------------------------------------------------
// Server handle
// ----------------------------------------------------------------------

/// Lives in `AppState::remote_server` while the server is running. Holds
/// the graceful-shutdown trigger, the bound port, and the LIVE auth state.
/// Dropping the handle (via `remote_server_stop`) fires the oneshot, which
/// axum's `with_graceful_shutdown` awaits.
pub struct RemoteServerHandle {
    /// `Some` until shutdown is triggered; taking it fires the oneshot.
    shutdown: Option<oneshot::Sender<()>>,
    /// Port the server actually bound (always `DEFAULT_PORT` today).
    pub port: u16,
    /// The SAME `Arc<AuthState>` the running axum task holds. Status calls
    /// MUST read the PIN from here (`auth.pin()`) so the Settings panel/QR
    /// show the EXACT PIN the live server accepts — never a throwaway minted
    /// by `resolve_auth()`/`load_or_init` (which would regenerate a fresh PIN
    /// the server rejects). A global-backstop rotation mutates this very
    /// instance, so status automatically tracks the rotated PIN.
    pub auth: Arc<auth::AuthState>,
}

impl RemoteServerHandle {
    /// Fire the graceful-shutdown signal. Idempotent.
    pub fn trigger_shutdown(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for RemoteServerHandle {
    fn drop(&mut self) {
        self.trigger_shutdown();
    }
}

/// Per-process server state shared into axum handlers as
/// `axum::extract::State`. Cheap to clone (all `Arc`/`AppHandle`).
#[derive(Clone)]
pub struct RemoteContext {
    pub app: AppHandle,
    pub auth: Arc<auth::AuthState>,
    pub events: RemoteEventBus,
    /// Bounds concurrent WS sessions to [`MAX_WS_CONNECTIONS`]. The WS
    /// handler holds an `OwnedSemaphorePermit` for the life of the socket;
    /// when the cap is reached the upgrade is refused.
    pub ws_slots: Arc<Semaphore>,
}

// ----------------------------------------------------------------------
// Status DTO + Tauri commands
// ----------------------------------------------------------------------

/// Returned by `remote_server_start` / `remote_server_status`. The SPA's
/// settings panel renders the PIN, the candidate LAN URLs, and the QR.
#[derive(Debug, Serialize)]
pub struct RemoteServerStatus {
    pub running: bool,
    pub port: u16,
    /// `http://<lan-ip>:<port>` for every RFC1918 / link-local interface.
    pub urls: Vec<String>,
    /// 6-digit pairing PIN (also embedded in the QR as `?pin=`).
    pub pin: String,
    /// QR of the primary URL + `?pin=<pin>`, as an `<svg>` data-URL.
    pub qr_svg: String,
}

/// Resolve (or lazily generate + persist) the PIN/token, building the
/// shared [`auth::AuthState`]. Called once per server start.
fn resolve_auth() -> Arc<auth::AuthState> {
    auth::AuthState::load_or_init(TOKEN_ACCOUNT)
}

/// Build the status DTO from the bound port and (when the server is
/// running) the LIVE auth state.
///
/// `auth_state` is `Some` ONLY when the server is actually running — it is
/// the very `Arc<AuthState>` the axum task holds, so the reported `pin`
/// equals the PIN the server accepts (and tracks a rotation). When the
/// server is stopped (`None`), the status reports an EMPTY PIN and a QR
/// without a `?pin=` so the panel shows a "stopped" state rather than a
/// throwaway PIN that implies pairing would work.
fn build_status(
    running: bool,
    port: u16,
    auth_state: Option<&auth::AuthState>,
) -> RemoteServerStatus {
    let urls = net::lan_urls(port);
    let pin = auth_state.map(|a| a.pin()).unwrap_or_default();
    let qr_target = qr_target_url(urls.first().map(String::as_str), port, &pin);
    let qr_svg = net::qr_svg(&qr_target);
    RemoteServerStatus {
        running,
        port,
        urls,
        pin,
        qr_svg,
    }
}

/// Compute the URL the QR encodes: the primary LAN URL, or a `localhost`
/// fallback when no LAN interface was found. A `?pin=` query is appended
/// ONLY when `pin` is non-empty (i.e. the server is running); a stopped
/// server's QR carries no PIN. Pure (testable).
fn qr_target_url(primary: Option<&str>, port: u16, pin: &str) -> String {
    let base = match primary {
        Some(p) if !p.is_empty() => p.to_string(),
        _ => format!("http://localhost:{port}"),
    };
    if pin.is_empty() {
        format!("{base}/")
    } else {
        format!("{base}/?pin={pin}")
    }
}

/// Start the LAN remote-control server. Idempotent: if already running it
/// returns the current status instead of double-binding. Spawned via
/// `tauri::async_runtime::spawn`; NOT auto-started.
#[tauri::command]
pub async fn remote_server_start(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RemoteServerStatus, UiError> {
    let mut guard = state.remote_server.lock().await;
    if let Some(handle) = guard.as_ref() {
        // Already running — surface the current status from the LIVE auth
        // state the running server holds (NOT a throwaway), so the reported
        // PIN is the one the server actually accepts.
        return Ok(build_status(true, handle.port, Some(&handle.auth)));
    }

    // Use the user-configured + persisted port (defaults to DEFAULT_PORT).
    let port = read_persisted_port(&app);
    let auth_state = resolve_auth();
    let ctx = RemoteContext {
        app: app.clone(),
        auth: Arc::clone(&auth_state),
        events: state.remote_events.clone(),
        ws_slots: Arc::new(Semaphore::new(MAX_WS_CONNECTIONS)),
    };

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    // A `watch` lets BOTH the serve task's graceful-shutdown future and the
    // shared status-ticker observe the same stop signal, so the ticker is
    // torn down exactly when the server stops (it never stacks across
    // stop/start cycles). The oneshot above still drives `trigger_shutdown`;
    // we forward it into the watch below.
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);

    // One shared 1 Hz timer computes the `flight_status` frame ONCE per
    // second and publishes it onto the broadcast bus every WS already
    // subscribes to — instead of every connection polling it itself (an
    // O(connections) amplifier). It stops when the server stops.
    spawn_flight_status_ticker(app.clone(), state.remote_events.clone(), stop_rx);

    // Resolve the SPA dir + bind the listener up front so a failure is
    // reported synchronously to the caller (not swallowed in the task).
    let serve_dir = router::resolve_spa_dir(&app);
    let listener = router::bind(port).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            UiError::new(
                "remote_port_in_use",
                format!(
                    "Port {port} ist bereits belegt. Bitte in den Einstellungen einen \
                     anderen Port wählen oder das andere Programm beenden."
                ),
            )
        } else {
            UiError::new(
                "remote_bind_failed",
                format!("Konnte LAN-Server nicht an Port {port} binden: {e}"),
            )
        }
    })?;

    let router = router::build_router(ctx, serve_dir);
    tauri::async_runtime::spawn(async move {
        tracing::info!(port, "LAN remote-control server listening");
        let serve = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
            // Signal the shared status-ticker to stop too.
            let _ = stop_tx.send(true);
            tracing::info!("LAN remote-control server shutting down");
        });
        if let Err(e) = serve.await {
            tracing::error!(error = %e, "LAN remote-control server exited with error");
        }
    });

    *guard = Some(RemoteServerHandle {
        shutdown: Some(shutdown_tx),
        port,
        // Store the SAME `Arc<AuthState>` the spawned axum task uses, so
        // later status polls read the live PIN (incl. after a rotation).
        auth: Arc::clone(&auth_state),
    });
    drop(guard);

    Ok(build_status(true, port, Some(&auth_state)))
}

/// Stop the LAN server (graceful shutdown). No-op if not running.
#[tauri::command]
pub async fn remote_server_stop(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RemoteServerStatus, UiError> {
    let mut guard = state.remote_server.lock().await;
    let port = guard
        .as_ref()
        .map(|h| h.port)
        .unwrap_or_else(|| read_persisted_port(&app));
    // Dropping the handle fires the graceful-shutdown oneshot.
    *guard = None;
    drop(guard);
    // Server is now stopped: report no PIN (don't mint a throwaway that
    // would imply pairing works).
    Ok(build_status(false, port, None))
}

/// Current server status (running flag, port, URLs, PIN, QR).
///
/// When the server is RUNNING, the reported PIN comes from the LIVE
/// `AuthState` the server holds (via the handle), so it is byte-for-byte the
/// PIN the server accepts — and it tracks a global-backstop rotation, since
/// rotation mutates that very instance.
///
/// When the server is STOPPED, it reports the persisted port that a
/// `remote_server_start` would bind (so the panel can preview the URLs) but
/// an EMPTY PIN — there is no live auth to honor a PIN, so showing one would
/// be a throwaway that falsely implies pairing works.
#[tauri::command]
pub async fn remote_server_status(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RemoteServerStatus, UiError> {
    let guard = state.remote_server.lock().await;
    match guard.as_ref() {
        Some(h) => {
            let status = build_status(true, h.port, Some(&h.auth));
            drop(guard);
            Ok(status)
        }
        None => {
            drop(guard);
            let port = read_persisted_port(&app);
            Ok(build_status(false, port, None))
        }
    }
}

/// Set + persist the LAN-server port. Validated to a non-privileged range
/// (1024..=65535) so a tablet can't ask the host to bind a privileged
/// port. Takes effect on the next `remote_server_start`; if the server is
/// currently running the caller should stop+start to rebind. Returns the
/// refreshed status (with the new port + recomputed URLs/QR).
#[tauri::command]
pub async fn remote_server_set_port(
    port: u16,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<RemoteServerStatus, UiError> {
    if port < 1024 {
        return Err(UiError::new(
            "remote_port_invalid",
            format!("Port {port} ist reserviert — bitte einen Port ab 1024 wählen."),
        ));
    }
    write_persisted_port(&app, port);

    // The new port takes effect on the next start; if the server is running
    // it keeps its current bound port until restarted. Report the status
    // with the just-set port. When running, read the PIN from the LIVE auth
    // (so it matches what the server accepts); when stopped, report no PIN.
    let guard = state.remote_server.lock().await;
    let status = match guard.as_ref() {
        Some(h) => build_status(true, port, Some(&h.auth)),
        None => build_status(false, port, None),
    };
    drop(guard);
    Ok(status)
}

// ----------------------------------------------------------------------
// Shared flight_status ticker
// ----------------------------------------------------------------------

/// Compute the current `flight_status` payload as a JSON value, or `null`
/// when no flight is active. Synchronous — `crate::flight_status` does not
/// span an `.await`, so it is safe to call inside the timer task without
/// holding a lock across a yield point.
pub(crate) fn current_flight_status_value(app: &AppHandle) -> serde_json::Value {
    let state = app.state::<AppState>();
    let status = crate::flight_status(app.clone(), state);
    serde_json::to_value(status).unwrap_or(serde_json::Value::Null)
}

/// Spawn the single process-wide 1 Hz `flight_status` ticker for the
/// running server. It computes the frame ONCE per tick and publishes it to
/// the broadcast bus (which every WS subscribes to), so `flight_status` is
/// computed once/sec total — NOT once/sec per connection. Publishes only
/// when the JSON actually changed, so an idle tablet sees a quiet stream.
/// Exits when `stop_rx` flips to `true` (server stop / app shutdown).
fn spawn_flight_status_ticker(
    app: AppHandle,
    events: RemoteEventBus,
    mut stop_rx: tokio::sync::watch::Receiver<bool>,
) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(FLIGHT_STATUS_TICK);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last: Option<serde_json::Value> = None;
        loop {
            tokio::select! {
                res = stop_rx.changed() => {
                    // Sender dropped or signalled stop → end the ticker.
                    if res.is_err() || *stop_rx.borrow() {
                        break;
                    }
                }
                _ = ticker.tick() => {
                    let value = current_flight_status_value(&app);
                    if last.as_ref() != Some(&value) {
                        last = Some(value.clone());
                        events.send(RemoteEvent::new(FLIGHT_STATUS_EVENT, value));
                    }
                }
            }
        }
        tracing::debug!("LAN remote: flight_status ticker stopped");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_persistence_round_trips() {
        // The on-disk JSON body a write produces parses back to the same
        // port (the file-IO wrapper just reads/writes this body verbatim).
        for port in [8765u16, 1024, 49152, 65535] {
            let body = serialize_persisted_port(port);
            assert_eq!(parse_persisted_port(&body), port);
        }
    }

    #[test]
    fn parse_persisted_port_falls_back_on_garbage() {
        assert_eq!(parse_persisted_port(""), DEFAULT_PORT);
        assert_eq!(parse_persisted_port("not json"), DEFAULT_PORT);
        assert_eq!(parse_persisted_port("{}"), DEFAULT_PORT);
        // Port 0 is invalid → fall back.
        assert_eq!(parse_persisted_port("{\"port\":0}"), DEFAULT_PORT);
        // A real value is honored.
        assert_eq!(parse_persisted_port("{\"port\":9000}"), 9000);
    }

    #[test]
    fn qr_target_uses_primary_lan_url_with_pin() {
        let url = qr_target_url(Some("http://192.168.1.10:8765"), 8765, "123456");
        assert_eq!(url, "http://192.168.1.10:8765/?pin=123456");
    }

    #[test]
    fn qr_target_falls_back_to_localhost() {
        // No LAN interface found (offline host) → localhost on the port.
        let url = qr_target_url(None, 9999, "000000");
        assert_eq!(url, "http://localhost:9999/?pin=000000");
        // Empty-string primary treated the same as None.
        let url2 = qr_target_url(Some(""), 9999, "000000");
        assert_eq!(url2, "http://localhost:9999/?pin=000000");
    }

    #[test]
    fn qr_target_omits_pin_when_stopped() {
        // A stopped server reports an empty PIN → the QR must carry NO
        // `?pin=` (showing one would imply pairing works when it can't).
        assert_eq!(
            qr_target_url(Some("http://192.168.1.10:8765"), 8765, ""),
            "http://192.168.1.10:8765/"
        );
        assert_eq!(qr_target_url(None, 9999, ""), "http://localhost:9999/");
    }

    // FIX A regression: the status DTO's PIN must come from the LIVE
    // `AuthState` the running server holds (read via the handle), so it is
    // exactly the PIN the server accepts — and it must track a global-backstop
    // rotation (since rotation mutates that very instance).
    #[test]
    fn running_status_pin_matches_live_auth_and_tracks_rotation() {
        use crate::remote::auth::{AuthError, AuthState};
        use std::net::IpAddr;

        // Stand in for the live `Arc<AuthState>` stored in RemoteServerHandle.
        let live = Arc::new(AuthState::for_test("424242", "tok"));

        // A running server's status reports the EXACT live PIN — proving the
        // Settings/QR PIN is the one `verify`/`try_pin` accepts (the pairing
        // flow now works).
        let status = build_status(true, 8765, Some(&live));
        assert!(status.running);
        assert_eq!(status.pin, "424242");
        assert_eq!(status.pin, live.pin());

        // The token the server accepts is unchanged by all of this.
        assert!(live.verify_token("tok"));

        // Drive the global-backstop rotation on THIS SAME instance, spread
        // across fresh spoofed IPs so no per-IP lockout bites.
        for i in 0..crate::remote::auth::GLOBAL_ROTATE_THRESHOLD {
            let ip: IpAddr = format!("10.0.{}.{}", i / 256, i % 256).parse().unwrap();
            assert_eq!(live.try_pin(ip, "000000"), Err(AuthError::BadPin));
        }
        let rotated = live.pin();
        assert_ne!(rotated, "424242", "PIN must rotate after the backstop");

        // Status read AFTER the rotation reflects the rotated PIN — because
        // it reads the same live instance, not a throwaway. This is the core
        // of FIX A: the reported PIN always equals the accepted PIN.
        let status_after = build_status(true, 8765, Some(&live));
        assert_eq!(status_after.pin, rotated);
        assert_eq!(status_after.pin, live.pin());
        // The persisted bearer token still works through a rotation.
        assert!(live.verify_token("tok"));
    }

    // FIX A: a STOPPED server reports no PIN (no throwaway).
    #[test]
    fn stopped_status_reports_no_pin() {
        let status = build_status(false, 8765, None);
        assert!(!status.running);
        assert!(status.pin.is_empty(), "stopped server must report no PIN");
    }

    #[test]
    fn ws_semaphore_caps_concurrent_sessions() {
        // The router uses `try_acquire_owned` on exactly this semaphore to
        // bound concurrent WS sessions; a permit is held for the life of
        // the socket. Prove: MAX_WS_CONNECTIONS permits are grantable, the
        // next is refused, and releasing one frees a slot again.
        let sem = Arc::new(Semaphore::new(MAX_WS_CONNECTIONS));
        let mut held = Vec::new();
        for _ in 0..MAX_WS_CONNECTIONS {
            held.push(
                Arc::clone(&sem)
                    .try_acquire_owned()
                    .expect("permit within cap"),
            );
        }
        // One past the cap is refused (router returns 503 here).
        assert!(Arc::clone(&sem).try_acquire_owned().is_err());
        // A disconnect releases its permit → a new session fits again.
        drop(held.pop());
        assert!(Arc::clone(&sem).try_acquire_owned().is_ok());
    }
}
