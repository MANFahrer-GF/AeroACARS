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
        ConnectInfo, Query, State,
    },
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::remote::net;

/// Fixed port for the panel server. Not user-configurable — see the
/// module doc for why that's the point, not an oversight. Chosen distinct
/// from the LAN Remote Control server's default (8765) specifically so
/// the two are never confusable.
pub const PANEL_SERVER_PORT: u16 = 47847;

const TICK: Duration = Duration::from_secs(1);

/// Wie viele Aktivitäts-Einträge `/panel/activity` ohne `?limit=` liefert.
/// Das HUD zeigt eine Zeile; ein paar mehr kosten nichts und erlauben eine
/// kurze Historie, ohne dass die Anzeige nachfragen muss.
const ACTIVITY_DEFAULT_LIMIT: usize = 5;
/// Obergrenze, damit ein `?limit=100000` nicht den kompletten Ringpuffer
/// durch eine Anzeige schiebt, die davon eine Zeile benutzt.
const ACTIVITY_MAX_LIMIT: usize = 50;

#[derive(Clone)]
struct PanelCtx {
    app: AppHandle,
    /// Flippt beim App-Ende auf `true`. Siehe [`shutdown`].
    stop: tokio::sync::watch::Receiver<bool>,
}

/// Absender des Stopp-Signals, gesetzt von [`spawn`], ausgelöst von
/// [`shutdown`]. `OnceLock` dient zugleich als Doppelstart-Sperre.
static SHUTDOWN: std::sync::OnceLock<tokio::sync::watch::Sender<bool>> =
    std::sync::OnceLock::new();

/// Fährt den Panel-Server geordnet herunter. Muss auf dem
/// `RunEvent::ExitRequested`-Pfad in `lib.rs` aufgerufen werden.
///
/// **Warum das nötig ist (QS 09.08.2026):** dieser Server lief als einzige
/// langlebige Aufgabe der App ohne jeden Stopp-Weg — MQTT, Hoppie und der
/// LAN-Fernzugriff-Server fahren alle geordnet herunter, dieser nicht. Er
/// hielt einen `AppHandle`, löste im Sekundentakt `app.state::<AppState>()`
/// auf und lief dabei gegen den Abbau der App an. Genau daraus kann ein
/// Zugriff auf bereits freigegebenen Speicher werden, und der zeigt sich
/// als das, was in der Ereignisanzeige stand: `0xC0000374`
/// (Heap-Korruption) beziehungsweise `0xC000001D`.
///
/// Das ist ausdrücklich eine begründete Vermutung, kein Beweis — den
/// liefert erst ein Speicherabbild. Der fehlende Stopp-Weg ist aber
/// unabhängig davon ein Defekt.
pub fn shutdown() {
    if let Some(tx) = SHUTDOWN.get() {
        let _ = tx.send(true);
    }
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
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    if SHUTDOWN.set(stop_tx).is_err() {
        // Schon einmal gestartet. Ein zweiter Server auf demselben Port
        // käme ohnehin nicht hoch; wichtiger ist, den ersten Stopp-Sender
        // nicht zu überschreiben und damit unerreichbar zu machen.
        tracing::warn!("panel_server: spawn called twice — ignoring the second call");
        return;
    }
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
        let ctx = PanelCtx { app, stop: stop_rx.clone() };
        let router = Router::new()
            .route("/panel/status", get(status_handler))
            .route("/panel/debrief", get(debrief_handler))
            .route("/panel/activity", get(activity_handler))
            .route("/panel/ws", get(ws_handler))
            .with_state(ctx);
        let mut stop_serve = stop_rx;
        if let Err(e) = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move {
            // `wait_for` kehrt auch zurück, wenn der Sender wegfällt —
            // dann ist die App ohnehin weg und Herunterfahren richtig.
            let _ = stop_serve.wait_for(|stopped| *stopped).await;
        })
        .await
        {
            tracing::warn!(error = %e, "panel_server: serve loop ended unexpectedly");
        }
        tracing::info!("panel_server: stopped");
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

#[derive(Deserialize)]
struct ActivityQuery {
    limit: Option<usize>,
}

/// Wieviele Einträge tatsächlich geliefert werden. Als eigene Funktion, weil
/// der Test unten genau diese Entscheidung prüfen soll und nicht eine
/// Nachbildung davon.
fn effective_activity_limit(requested: Option<usize>) -> usize {
    match requested {
        Some(n) if n > 0 => n.min(ACTIVITY_MAX_LIMIT),
        _ => ACTIVITY_DEFAULT_LIMIT,
    }
}

/// v1.5.0 (#msfs-hud): die jüngsten Aktivitäts-Einträge, neueste zuerst.
///
/// Eigene Route statt eines Felds in `/panel/status`: der Statusrahmen geht
/// im Sekundentakt über die WebSocket-Verbindung, und ihn um einen
/// Textblock zu erweitern, der sich nur alle paar Minuten ändert, hieße den
/// Änderungsvergleich in `handle_socket` bei jedem Log-Eintrag anschlagen zu
/// lassen. Der Ticker holt sich das lieber selbst, in seinem eigenen Takt.
///
/// `limit` wird auf [`ACTIVITY_MAX_LIMIT`] gedeckelt und ein `limit=0` auf
/// den Standard zurückgesetzt — eine leere Antwort wäre für den Aufrufer
/// nicht von „es gibt nichts zu berichten“ zu unterscheiden.
async fn activity_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<ActivityQuery>,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    let limit = effective_activity_limit(q.limit);
    let state = ctx.app.state::<crate::AppState>();
    let entries = crate::activity_log_tail(&state, limit);
    let value = serde_json::to_value(entries).unwrap_or(Value::Null);
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
    let mut stop = ctx.stop.clone();
    loop {
        tokio::select! {
            // MUSS mit im select stehen: `with_graceful_shutdown` hört zwar
            // auf, neue Verbindungen anzunehmen, wartet aber auf die
            // bestehenden. Eine dauerhaft offene Panel-Verbindung — also der
            // Normalfall, sobald das HUD im Sim läuft — würde das Beenden
            // sonst endlos aufhalten.
            // `changed()` statt `wait_for(..)`: letzteres gibt eine Sperre auf
            // den Wert zurück, die über das folgende `await` gehalten würde —
            // damit wäre die ganze Zukunft nicht mehr zwischen Threads
            // versendbar und axum nimmt sie nicht an. Der Wert wird deshalb
            // sofort in eine lokale Variable kopiert.
            res = stop.changed() => {
                let stopped = res.is_err() || *stop.borrow();
                if stopped {
                    let _ = socket.send(Message::Close(None)).await;
                    break;
                }
            }
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

    /// Die Deckelung ist die einzige Stelle, an der ein Aufrufer diesem
    /// Server etwas vorgeben kann. `limit=0` muss zum Standard werden statt
    /// zu einer leeren Liste: leer hieße für den Ticker „nichts passiert“,
    /// und er würde die letzte Zeile löschen, obwohl es sie noch gibt.
    #[test]
    fn activity_limit_is_clamped_and_never_collapses_to_empty() {
        assert_eq!(effective_activity_limit(None), ACTIVITY_DEFAULT_LIMIT);
        assert_eq!(effective_activity_limit(Some(0)), ACTIVITY_DEFAULT_LIMIT);
        assert_eq!(effective_activity_limit(Some(3)), 3);
        assert_eq!(effective_activity_limit(Some(100_000)), ACTIVITY_MAX_LIMIT);
    }

    /// Das Stopp-Signal muss auch bei einer Verbindung ankommen, die
    /// ERST NACH dem Absenden ihren Empfänger klont — sonst hinge beim
    /// Beenden eine Panel-Verbindung, die zufällig im falschen Moment
    /// aufgebaut wurde, und mit ihr das geordnete Herunterfahren.
    /// Prüft die Annahme über `watch`, auf der `handle_socket` aufbaut.
    #[tokio::test]
    async fn a_socket_opened_around_shutdown_still_sees_the_stop_signal() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        tx.send(true).unwrap();
        // Klon NACH dem Senden — genau der Wettlauf-Fall.
        let mut late = rx.clone();
        let res = tokio::time::timeout(Duration::from_millis(200), late.changed()).await;
        assert!(res.is_ok(), "changed() muss sofort zurückkehren, nicht in den Timeout laufen");
        assert!(*late.borrow());
    }

    /// Fällt der Absender weg (App bereits abgebaut), muss die Schleife das
    /// als Stopp werten und nicht ewig weiterlaufen.
    #[tokio::test]
    async fn a_dropped_sender_counts_as_stop() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let mut recv = rx.clone();
        drop(tx);
        let res = recv.changed().await;
        assert!(res.is_err(), "weggefallener Absender muss als Fehler ankommen");
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
