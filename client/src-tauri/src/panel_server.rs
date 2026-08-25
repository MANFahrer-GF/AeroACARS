//! Loopback-only local server for the MSFS in-sim panel (#msfs-hud).
//!
//! Independent of the opt-in LAN Remote Control server (`remote/mod.rs`):
//! different threat model (this panel only ever runs on the SAME PC as
//! AeroACARS, a tablet is a genuinely different device on the LAN), so it
//! gets its own **fixed, non-configurable port**.
//!
//! This replaces an earlier round-2 design that put `/panel/*` routes on
//! the SAME server/port as the tablet feature. That had two real bugs,
//! both caught live by Thomas testing round 2: (1) that server is opt-in
//! (`remote_server_start`) — the panel sat on "Verbinde..." forever unless
//! the pilot separately toggled "Fernzugriff" on; (2) that server's port
//! is pilot-configurable (`remote_server_set_port`) — changing it in
//! Settings silently broke the panel. A fixed, dedicated port sidesteps
//! both: nothing to toggle on the panel side, nothing to mismatch.
//!
//! Binds literally to loopback (`127.0.0.1` AND `::1`) — genuinely
//! unreachable from the LAN at the socket level, not just via an
//! app-level peer check. [`reject_non_loopback`] below is still applied
//! per-request as belt-and-suspenders.
//!
//! ## Härtung (QS 09.08.2026)
//!
//! Die erste Fassung verzichtete bewusst auf Verbindungs-Obergrenzen
//! („kein Bedarf auf reiner Rückschleife"). Der Feldtest hat das
//! widerlegt — nur kam die Last nicht von einem Angreifer, sondern vom
//! eigenen Klienten: Coherent GT (der eingebettete Renderer des
//! Simulators) hielt tote Keep-Alive-Verbindungen unbegrenzt offen, und
//! Alt-Kopien des Widgets stapelten weitere dazu (`netstat` zeigte fünf
//! gleichzeitig). Deshalb jetzt, nach dem Vorbild von
//! `remote::LimitedListener`:
//!
//!   * **Obergrenze gleichzeitiger Verbindungen** pro Adresse
//!     ([`MAX_CONNS`]) — Zombies können den Prozess nicht fluten.
//!   * **Lebenszeit-Schranke pro Verbindung** ([`CONN_LIFETIME`]): nach
//!     Ablauf wird die Verbindung zwischen zwei Anfragen sauber per EOF
//!     geschlossen. Damit räumen sich tote Steckplätze GARANTIERT selbst
//!     auf, statt bis zum Sim-Neustart zu liegen; das Widget verbindet
//!     transparent neu.
//!   * **Keine WebSocket-Route mehr.** Das Widget nutzt seit v1.2 reines
//!     Abfragen; die einzigen WS-Klienten wären Alt-Kopien im Sim, deren
//!     halbfertige Upgrades hier Aufgaben liegen ließen. Ein 404 ist die
//!     robustere Antwort.
//!   * **Abschaltbar** über die Einstellungen ([`is_enabled`], wirkt beim
//!     nächsten Start). Primär ein Diagnose-Werkzeug: die Beta-Abstürze
//!     (Ereignis 1000, `0xC0000374`) sind ungeklärt, und der Schalter
//!     erlaubt den A/B-Test „stürzt es auch OHNE Panel-Server ab?" —
//!     ohne auf v1.4.7 zurückzumüssen.

use std::future::Future as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;
use tauri::{AppHandle, Manager};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::Semaphore;

use crate::remote::net;

/// Fixed port for the panel server. Not user-configurable — see the
/// module doc for why that's the point, not an oversight. Chosen distinct
/// from the LAN Remote Control server's default (8765) specifically so
/// the two are never confusable.
pub const PANEL_SERVER_PORT: u16 = 47847;

/// Obergrenze gleichzeitig offener Verbindungen PRO Adresse. Das Widget
/// braucht genau eine; die Reserve fängt Coherents Verbindungsvorrat und
/// kurzlebige Überlappungen beim Neuverbinden ab. Zombies darüber hinaus
/// bleiben im TCP-Rückstau des Betriebssystems statt hier Ressourcen zu
/// binden.
const MAX_CONNS: usize = 8;

/// Lebenszeit einer Verbindung. Nach Ablauf schließt [`PanelStream`] die
/// Verbindung zwischen zwei Anfragen per EOF — der einzige Weg, auf dem
/// tote Keep-Alive-Steckplätze (Feldbefund: fünf gleichzeitig, gehalten
/// bis zum Sim-Neustart) sich garantiert selbst aufräumen. Ein gesundes
/// Widget verbindet danach transparent neu; auf der Rückschleife kostet
/// das nichts.
const CONN_LIFETIME: Duration = Duration::from_secs(120);

/// Wie viele Aktivitäts-Einträge `/panel/activity` ohne `?limit=` liefert.
const ACTIVITY_DEFAULT_LIMIT: usize = 5;
/// Obergrenze, damit ein `?limit=100000` nicht den kompletten Ringpuffer
/// durch eine Anzeige schiebt, die davon eine Zeile benutzt.
const ACTIVITY_MAX_LIMIT: usize = 50;

/// Dateiname der Panel-Server-Einstellung im App-Konfigverzeichnis.
const CONFIG_FILE: &str = "panel_server.json";

#[derive(Clone)]
struct PanelCtx {
    app: AppHandle,
}

/// v1.5.1 (#hud-pilotenfeedback, Thomas' Frage "kann man das HUD auch per
/// Browser oeffnen?"): dieselbe Widget-Quelle als DRITTES Ziel, direkt vom
/// Panel-Server ausgeliefert. `http://127.0.0.1:47847/hud` im Browser —
/// same-origin, also kein CORS-Thema, und weil der Server Loopback-only
/// ist, bleibt es beim eigenen Rechner.
///
/// Die Dateien kommen per `include_str!` ZUR COMPILE-ZEIT aus dem
/// msfs-panel-Paket — `panel.js` ist dort byte-identisch mit dem
/// Flow-Widget (eine Quelle, jetzt drei Ziele). Kein Laufzeit-Dateipfad,
/// nichts zu installieren, nichts kann fehlen. Der Browser nimmt
/// automatisch den "nativen" Einstieg des Skripts (kein Flow-`run()`),
/// und `aa2-nativ` am Body waehlt die fuellende Positionierung.
async fn hud_handler() -> axum::response::Html<String> {
    const JS: &str = include_str!(
        "../../../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/panel.js"
    );
    const CSS: &str = include_str!(
        "../../../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/panel.css"
    );
    // v1.5.3 (Thomas, 11.08.2026): NUR der Kasten, sonst nichts.
    //
    // Die erste Fassung erbte ueber `aa2-nativ` das Fensterfuell-
    // Verhalten des nativen Panels: dunkler Seitengrund ueber den ganzen
    // Bildschirm, Streifen ueber die volle Breite. Wer die Seite in
    // OBS/Streamdeck & Co. einbettet, bekam damit "ein riesengrosses
    // Bild" statt des Widgets. Jetzt:
    //   - Seitengrund TRANSPARENT (Einbettungen zeigen nur den Kasten;
    //     im normalen Browser bleibt er lesbar, weil er dafuer eine
    //     OPAKE Flaeche bekommt statt der halbtransparenten aus dem Sim)
    //   - Streifen als inline-flex im Fluss: endet hinter dem Inhalt,
    //     nicht an der Bildschirmkante
    axum::response::Html(format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>AeroACARS HUD</title><style>{CSS}\
         html,body{{margin:0;padding:0;background:transparent;}}\
         .aa2-strip{{position:static;display:inline-flex;width:auto;\
         background:#101820;}}\
         .aa2-strip.aa2-quiet{{opacity:1;}}\
         .aa2-strip:empty{{display:none;}}</style></head>\
         <body><div class=\"aa2-strip\"></div>\
         <script>{JS}</script></body></html>"
    ))
}

// ---------------------------------------------------------------------------
// Ein/Aus-Einstellung (Diagnose-Schalter)
// ---------------------------------------------------------------------------

#[derive(serde::Serialize, serde::Deserialize)]
struct PanelConfig {
    enabled: bool,
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join(CONFIG_FILE))
}

/// Pure Variante für Tests: liest die Einstellung von einem Pfad.
/// Fehlende oder kaputte Datei = eingeschaltet — der Server ist das
/// Standardverhalten, die Datei existiert nur, um ihn abzuschalten.
fn read_enabled_from(path: &Path) -> bool {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<PanelConfig>(&bytes)
            .map(|c| c.enabled)
            .unwrap_or(true),
        Err(_) => true,
    }
}

fn write_enabled_to(path: &Path, enabled: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(&PanelConfig { enabled })
        .expect("PanelConfig serialisiert immer");
    std::fs::write(path, json)
}

/// Ist der Panel-Server eingeschaltet? (Persistierte Einstellung;
/// Standard: ja.)
pub fn is_enabled(app: &AppHandle) -> bool {
    config_path(app)
        .map(|p| read_enabled_from(&p))
        .unwrap_or(true)
}

/// Schreibt die Einstellung. Wirkt beim NÄCHSTEN App-Start — bewusst kein
/// Stopp/Start zur Laufzeit: der Mehrwert wäre kosmetisch, und genau die
/// Start/Stopp-Verschränkung mit laufenden Verbindungen ist die Sorte
/// Komplexität, die dieses Modul nach dem 09.08. nicht mehr trägt.
pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let Some(path) = config_path(app) else {
        return Err("kein Konfigurationsverzeichnis".to_string());
    };
    write_enabled_to(&path, enabled).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Stopp-Signal (App-Ende)
// ---------------------------------------------------------------------------

/// Absender des Stopp-Signals, gesetzt von [`spawn`], ausgelöst von
/// [`shutdown`]. `OnceLock` dient zugleich als Doppelstart-Sperre.
static SHUTDOWN: std::sync::OnceLock<tokio::sync::watch::Sender<bool>> = std::sync::OnceLock::new();

/// Fährt den Panel-Server geordnet herunter. Wird AUSSCHLIESSLICH aus dem
/// `RunEvent::Exit`-Zweig in `lib.rs` gerufen (nicht `ExitRequested` —
/// das kann abgelehnt werden, und der Server wäre dann tot, während die
/// App weiterläuft).
///
/// Hintergrund: dieser Server war die einzige langlebige Aufgabe der App
/// ohne Stopp-Weg und griff im Sekundentakt auf `AppState` zu, während
/// die App abgebaut wurde. Ob genau das die Beta-Abstürze erklärt, ist
/// OFFEN (die Logdatei hat die erste Vermutung widerlegt; Klärung braucht
/// ein Speicherabbild) — der fehlende Stopp-Weg war unabhängig davon ein
/// Defekt.
pub fn shutdown() {
    if let Some(tx) = SHUTDOWN.get() {
        // Absichtlich auf `warn`: taucht diese Zeile im Log auf, OBWOHL die
        // App danach weiterläuft, ist der ExitRequested-Fehler zurück.
        tracing::warn!("panel_server: shutdown signalled");
        let _ = tx.send(true);
    }
}

// ---------------------------------------------------------------------------
// Start
// ---------------------------------------------------------------------------

/// Spawn the panel server for the lifetime of the app. Call exactly once,
/// at startup. Respects the persisted [`is_enabled`] setting.
///
/// A bind failure (something else already holding the port) is logged and
/// swallowed, not propagated — the MSFS panel simply won't connect this
/// session, which is a strictly better failure mode than taking the whole
/// app down over a feature most pilots will never open.
pub fn spawn(app: AppHandle) {
    if !is_enabled(&app) {
        // Die eine Zeile, die den A/B-Test dokumentiert: steht sie im Log
        // und es stürzt TROTZDEM ab, ist der Panel-Server entlastet.
        tracing::warn!("panel_server: disabled by setting — not starting");
        return;
    }
    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    if SHUTDOWN.set(stop_tx).is_err() {
        tracing::warn!("panel_server: spawn called twice — ignoring the second call");
        return;
    }
    let ctx = PanelCtx { app };
    // Beide Rückschleifen-Adressen: der Renderer im Sim führt seinen
    // Verbindungsvorrat pro Ziel-Adresse; ist einer vergiftet, gibt die
    // zweite Adresse dem Widget einen unabhängigen Weg ohne Sim-Neustart.
    // `::1` ist genauso Rückschleife wie `127.0.0.1`; schlägt eine Bindung
    // fehl (System ohne IPv6), läuft die andere weiter.
    for addr in [
        SocketAddr::from(([127, 0, 0, 1], PANEL_SERVER_PORT)),
        SocketAddr::from((std::net::Ipv6Addr::LOCALHOST, PANEL_SERVER_PORT)),
    ] {
        spawn_listener(addr, ctx.clone(), stop_rx.clone());
    }
}

/// Ein Lauscher auf genau einer Adresse. Mehrfach aufgerufen von [`spawn`].
fn spawn_listener(addr: SocketAddr, ctx: PanelCtx, stop_rx: tokio::sync::watch::Receiver<bool>) {
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    %addr,
                    "panel_server: bind failed for this address — the in-sim panel can still use the other one"
                );
                return;
            }
        };
        // Positive Meldung, nicht nur eine bei Fehlschlag: ohne sie konnte
        // der beta.3-Log die naheliegendste Frage — „lauscht der Server
        // überhaupt?" — nicht beantworten.
        tracing::info!(%addr, "panel_server: listening");
        let limited = PanelListener {
            inner: listener,
            slots: Arc::new(Semaphore::new(MAX_CONNS)),
        };
        let router = Router::new()
            .route("/panel/status", get(status_handler))
            .route("/panel/debrief", get(debrief_handler))
            .route("/panel/activity", get(activity_handler))
            .route("/hud", get(hud_handler))
            .with_state(ctx);
        let mut stop_serve = stop_rx;
        if let Err(e) = axum::serve(
            limited,
            router.into_make_service_with_connect_info::<PanelPeer>(),
        )
        .with_graceful_shutdown(async move {
            // `wait_for` kehrt auch zurück, wenn der Sender wegfällt —
            // dann ist die App ohnehin weg und Herunterfahren richtig.
            let _ = stop_serve.wait_for(|stopped| *stopped).await;
        })
        .await
        {
            tracing::warn!(error = %e, %addr, "panel_server: serve loop ended unexpectedly");
        }
        tracing::info!(%addr, "panel_server: stopped");
    });
}

// ---------------------------------------------------------------------------
// Gedeckelter Lauscher + Verbindung mit Lebenszeit-Schranke
// ---------------------------------------------------------------------------

/// Nach dem Vorbild von `remote::LimitedListener` (dort seit v0.19.x als
/// Slowloris-Schutz): das Permit wird VOR dem Betriebssystem-`accept`
/// genommen, Überzählige warten im TCP-Rückstau statt hier Dateideskriptoren
/// zu bekommen.
struct PanelListener {
    inner: tokio::net::TcpListener,
    slots: Arc<Semaphore>,
}

impl axum::serve::Listener for PanelListener {
    type Io = PanelStream;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let permit = Arc::clone(&self.slots)
            .acquire_owned()
            .await
            .expect("connection-limit semaphore is never closed");
        let (stream, addr) =
            <tokio::net::TcpListener as axum::serve::Listener>::accept(&mut self.inner).await;
        (
            PanelStream {
                inner: stream,
                _permit: permit,
                deadline: Box::pin(tokio::time::sleep(CONN_LIFETIME)),
                expired: false,
            },
            addr,
        )
    }

    fn local_addr(&self) -> std::io::Result<Self::Addr> {
        self.inner.local_addr()
    }
}

/// TCP-Verbindung mit Ablaufdatum.
///
/// Nach [`CONN_LIFETIME`] liefert die LESE-Seite EOF — für HTTP heißt das:
/// die laufende Antwort wird noch fertig geschrieben (die Schreibseite
/// bleibt offen), danach schließt der Server die Keep-Alive-Verbindung
/// sauber. Der eingebaute `Sleep` weckt den wartenden Lese-Poll zum
/// Stichtag, die Verbindung schließt also ZWISCHEN zwei Anfragen und nicht
/// erst, wenn die nächste hereinkommt — keine Anfrage geht verloren.
struct PanelStream {
    inner: tokio::net::TcpStream,
    _permit: tokio::sync::OwnedSemaphorePermit,
    deadline: Pin<Box<tokio::time::Sleep>>,
    expired: bool,
}

impl AsyncRead for PanelStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if !self.expired && self.deadline.as_mut().poll(cx).is_ready() {
            self.expired = true;
        }
        if self.expired {
            // EOF: Puffer unangetastet lassen. Hyper wertet das als
            // Verbindungsende und räumt die Keep-Alive-Verbindung ab.
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for PanelStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        // Bewusst KEINE Ablaufprüfung: eine zum Stichtag gerade laufende
        // Antwort darf fertig geschrieben werden.
        Pin::new(&mut self.inner).poll_write(cx, data)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Lokaler `Connected`-Zielträger — gleicher Orphan-Rule-Grund wie
/// `remote::PeerAddr`, siehe dessen Doku.
#[derive(Debug, Clone, Copy)]
struct PanelPeer(SocketAddr);

impl axum::extract::connect_info::Connected<axum::serve::IncomingStream<'_, PanelListener>>
    for PanelPeer
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, PanelListener>) -> Self {
        PanelPeer(*stream.remote_addr())
    }
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

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
    resp.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    resp
}

async fn status_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(PanelPeer(peer)): ConnectInfo<PanelPeer>,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    let mut value = crate::remote::current_flight_status_value(&ctx.app);
    with_display_callsign(&mut value);
    cors_open((StatusCode::OK, Json(value)).into_response())
}

/// v1.5.6 (Feldbefund Thomas): Das HUD zeigte bei GEC-Fluegen "4TK" statt
/// "GEC 4TK".
///
/// Ursache: `ActiveFlightInfo.callsign` ist das ROHE `Bid.flight.callsign`
/// aus phpVMS — bei manchen VA-Konfigurationen steht dort nur der
/// Flug-Identifikator ohne Airline-Praefix ("4TK", "7ME"). Die React-App
/// setzt sich die Anzeige selbst zusammen (`airline_icao + ident`, siehe
/// `lib/callsign.ts`), das Widget nahm den Rohwert.
///
/// Der Fix sitzt bewusst HIER und nicht im Widget: `/panel/status` ist der
/// einzige Konsument dieses JSON auf HUD-Seite, damit wirkt er sofort auf
/// JEDE bereits ausgelieferte Widget- und HUD-Fassung — niemand muss sein
/// Flow-Paket neu bauen. Die App und die LAN-Bruecke sehen den Rohwert
/// unveraendert weiter (sie brauchen ihn fuer den OFP-Abgleich).
///
/// Regel identisch zu phpVMS' eigenem `Flight::atc()` (Airline-ICAO +
/// callsign, sonst + flight_number), nur mit Leerzeichen als Trenner —
/// so wie es die App seit jeher anzeigt.
fn with_display_callsign(value: &mut serde_json::Value) {
    let Some(obj) = value.as_object_mut() else {
        return;
    };
    let s = |v: Option<&serde_json::Value>| {
        v.and_then(|x| x.as_str())
            .map(str::trim)
            .unwrap_or("")
            .to_string()
    };
    let airline = s(obj.get("airline_icao"));
    let raw_callsign = s(obj.get("callsign"));
    let flight_number = s(obj.get("flight_number"));

    let ident = if raw_callsign.is_empty() {
        flight_number
    } else {
        raw_callsign
    };
    if ident.is_empty() {
        return;
    }
    // Traegt der Identifikator das Praefix schon (VAs, die "GEC4TK" in
    // callsign schreiben), NICHT doppeln.
    let display = if airline.is_empty() || ident.starts_with(&airline) {
        ident
    } else {
        format!("{airline} {ident}")
    };
    obj.insert("callsign".into(), serde_json::Value::String(display));
}

async fn debrief_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(PanelPeer(peer)): ConnectInfo<PanelPeer>,
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
/// `limit` wird auf [`ACTIVITY_MAX_LIMIT`] gedeckelt und ein `limit=0` auf
/// den Standard zurückgesetzt — eine leere Antwort wäre für den Aufrufer
/// nicht von „es gibt nichts zu berichten“ zu unterscheiden.
async fn activity_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(PanelPeer(peer)): ConnectInfo<PanelPeer>,
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

#[cfg(test)]
mod hud_page_tests {
    /// Die eingebettete Browser-Fassung muss dieselbe Quelle tragen wie
    /// das Flow-Widget und das native Panel — und ein vollstaendiges,
    /// eigenstaendiges Dokument sein. Bricht jemand die include-Pfade
    /// (Verzeichnis umbenannt, Datei verschoben), faellt es HIER auf und
    /// nicht erst beim Piloten im Browser.
    #[tokio::test]
    async fn hud_page_embeds_the_widget() {
        let html = super::hud_handler().await.0;
        assert!(html.contains("aa2-strip"), "Streifen-Buehne fehlt");
        assert!(
            html.contains("background:transparent"),
            "Seitengrund muss transparent sein (Einbettung in OBS & Co.)"
        );
        assert!(
            html.contains("display:inline-flex"),
            "Streifen muss hinter dem Inhalt enden, nicht an der Bildschirmkante"
        );
        // Der Klassen-NAME darf in der eingebetteten CSS auftauchen (die
        // .aa2-nativ-Regeln des Panels sind mit drin, matchen aber nie) —
        // nur am BODY darf er nicht haengen.
        assert!(
            !html.contains("body class=\"aa2-nativ\""),
            "aa2-nativ am Body wuerde das Fensterfuell-Verhalten zurueckholen"
        );
        assert!(html.contains("__aeroacarsHudV2_"), "Widget-Kern fehlt");
        assert!(html.len() > 20_000, "verdaechtig klein: {}", html.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_lan_peers_a_full_remote_route_would_allow() {
        let lan_peer: SocketAddr = "192.168.1.5:5000".parse().unwrap();
        assert!(reject_non_loopback(lan_peer).is_some());

        let loop_peer: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        assert!(reject_non_loopback(loop_peer).is_none());
    }

    /// Beide Rückschleifen-Adressen müssen durchkommen, alles andere nicht —
    /// die zweite Adresse wäre sonst ein offenes Tor ohne Token.
    #[test]
    fn the_second_address_is_just_as_loopback_only_as_the_first() {
        let v6_loopback: SocketAddr = "[::1]:5000".parse().unwrap();
        assert!(
            reject_non_loopback(v6_loopback).is_none(),
            "::1 muss durchkommen"
        );

        let v6_public: SocketAddr = "[2001:db8::1]:5000".parse().unwrap();
        assert!(reject_non_loopback(v6_public).is_some());
        let v6_mapped_lan: SocketAddr = "[::ffff:192.168.1.5]:5000".parse().unwrap();
        assert!(reject_non_loopback(v6_mapped_lan).is_some());
    }

    #[test]
    fn activity_limit_is_clamped_and_never_collapses_to_empty() {
        assert_eq!(effective_activity_limit(None), ACTIVITY_DEFAULT_LIMIT);
        assert_eq!(effective_activity_limit(Some(0)), ACTIVITY_DEFAULT_LIMIT);
        assert_eq!(effective_activity_limit(Some(3)), 3);
        assert_eq!(effective_activity_limit(Some(100_000)), ACTIVITY_MAX_LIMIT);
    }

    #[test]
    fn panel_server_port_is_distinct_from_lan_remote_default() {
        assert_ne!(PANEL_SERVER_PORT, 8765);
    }

    /// Watch-Semantik, auf der das geordnete Herunterfahren aufbaut: ein
    /// Empfänger, der erst NACH dem Absenden geklont wird, muss das Signal
    /// trotzdem sehen, und ein weggefallener Absender zählt als Stopp.
    #[tokio::test]
    async fn shutdown_signal_semantics_hold() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        tx.send(true).unwrap();
        let mut late = rx.clone();
        let res = tokio::time::timeout(Duration::from_millis(200), late.changed()).await;
        assert!(res.is_ok(), "changed() muss sofort zurückkehren");
        assert!(*late.borrow());

        let (tx2, rx2) = tokio::sync::watch::channel(false);
        let mut recv = rx2.clone();
        drop(tx2);
        assert!(
            recv.changed().await.is_err(),
            "weggefallener Absender = Stopp"
        );
    }

    /// Der Kern der Härtung: eine abgelaufene Verbindung liefert auf der
    /// Lese-Seite EOF — und zwar von selbst (der eingebaute Sleep weckt den
    /// Poll), nicht erst bei der nächsten Anfrage. Real über ein
    /// TCP-Sockelpaar geprüft, kein Nachbau.
    #[tokio::test]
    async fn an_expired_connection_reads_eof_on_its_own() {
        use tokio::io::AsyncReadExt;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        // Klientenseite offen halten — es soll der ABLAUF schließen, nicht
        // das Gegenüber.
        let _client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (server_side, _) = listener.accept().await.unwrap();

        let slots = Arc::new(Semaphore::new(1));
        let permit = Arc::clone(&slots).acquire_owned().await.unwrap();
        let mut stream = PanelStream {
            inner: server_side,
            _permit: permit,
            deadline: Box::pin(tokio::time::sleep(Duration::from_millis(50))),
            expired: false,
        };

        let mut buf = [0u8; 16];
        // Ohne die Sleep-Weckung würde dieser read ewig hängen (der Klient
        // schickt nichts) — der Test würde in den äußeren Timeout laufen.
        let n = tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buf))
            .await
            .expect("read muss durch den Ablauf geweckt werden")
            .expect("EOF ist ein sauberes Ok(0), kein Fehler");
        assert_eq!(n, 0, "abgelaufene Verbindung muss EOF liefern");
    }

    /// Verbindungs-Deckel real am Sockel: mit vollem Semaphor darf accept()
    /// nicht durchkommen — gleiche Prüfung wie beim LAN-Server-Vorbild.
    #[tokio::test]
    async fn connection_cap_blocks_accept_beyond_the_limit() {
        use axum::serve::Listener as _;

        let raw = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = raw.local_addr().unwrap();
        let mut limited = PanelListener {
            inner: raw,
            slots: Arc::new(Semaphore::new(1)),
        };

        let _c1 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (_held, _) = limited.accept().await;

        let _c2 = tokio::net::TcpStream::connect(addr).await.unwrap();
        let blocked = tokio::time::timeout(Duration::from_millis(200), limited.accept()).await;
        assert!(
            blocked.is_err(),
            "zweite Verbindung darf den Deckel nicht passieren"
        );
    }

    /// Die Ein/Aus-Einstellung: Standard ist AN (fehlende/kaputte Datei),
    /// und ein geschriebenes AUS kommt wieder heraus.
    #[test]
    fn enabled_setting_round_trips_and_defaults_to_on() {
        let dir = std::env::temp_dir().join(format!("aa-panel-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(CONFIG_FILE);

        assert!(read_enabled_from(&path), "fehlende Datei = eingeschaltet");

        write_enabled_to(&path, false).unwrap();
        assert!(!read_enabled_from(&path));
        write_enabled_to(&path, true).unwrap();
        assert!(read_enabled_from(&path));

        std::fs::write(&path, b"kaputt{{{").unwrap();
        assert!(read_enabled_from(&path), "kaputte Datei = eingeschaltet");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// v1.5.6 (Feldbefund Thomas, GEC-Flug): Das HUD zeigte "4TK" statt
    /// "GEC 4TK", weil phpVMS in `Bid.flight.callsign` nur den
    /// Flug-Identifikator ohne Airline-Praefix liefert.
    #[test]
    fn hud_callsign_gets_the_airline_prefix() {
        // Der echte Fall: callsign="4TK", airline="GEC".
        let mut v = serde_json::json!({
            "airline_icao": "GEC",
            "callsign": "4TK",
            "flight_number": "4TK",
        });
        with_display_callsign(&mut v);
        assert_eq!(v["callsign"], "GEC 4TK");
    }

    #[test]
    fn hud_callsign_falls_back_to_the_flight_number() {
        // phpVMS fuellt `callsign` nicht -> Flugnummer nehmen.
        let mut v = serde_json::json!({
            "airline_icao": "GSG",
            "callsign": serde_json::Value::Null,
            "flight_number": "1316",
        });
        with_display_callsign(&mut v);
        assert_eq!(v["callsign"], "GSG 1316");
    }

    #[test]
    fn hud_callsign_is_not_doubled_when_already_prefixed() {
        // VAs, die "GEC4TK" komplett in callsign schreiben, duerfen nicht
        // zu "GEC GEC4TK" werden.
        let mut v = serde_json::json!({
            "airline_icao": "GEC",
            "callsign": "GEC4TK",
            "flight_number": "4TK",
        });
        with_display_callsign(&mut v);
        assert_eq!(v["callsign"], "GEC4TK");
    }

    #[test]
    fn hud_callsign_survives_missing_fields() {
        // Kein aktiver Flug (null) oder leere Felder -> nichts anfassen,
        // niemals panicken.
        let mut null = serde_json::Value::Null;
        with_display_callsign(&mut null);
        assert!(null.is_null());

        let mut leer = serde_json::json!({ "airline_icao": "GSG" });
        with_display_callsign(&mut leer);
        assert!(
            leer.get("callsign").is_none(),
            "ohne Identifikator kein Feld erfinden"
        );
    }
}
