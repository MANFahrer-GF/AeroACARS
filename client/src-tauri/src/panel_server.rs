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
//!
//! ## StreamDeck (`/panel/kabine`, `/panel/hoppie`, `/panel/cmd/{name}`)
//!
//! Das StreamDeck-Plugin läuft wie das Panel auf DEMSELBEN PC und liest
//! hier mit. Neu ist, dass es auch BEDIENT: Kabine stumm, beide
//! Lautstärken, Boarding-Musik, das VATSIM-CDM-Fenster, und das Hauptfenster
//! auf einer festen Seite nach vorn holen (langer Druck auf eine Anzeigetaste),
//! METAR für Start/Ziel zur Anzeige nachladen (siehe `metar_nachladen`). Genau diese Befehle
//! sperrt die LAN-Brücke („der Ton spielt auf dem Sim-PC“) — das Deck steht
//! am Sim-PC, für das Tablet bleibt die Sperre richtig.
//!
//! Bedrohung: Ohne PIN darf jeder Prozess auf dem eigenen Rechner diese
//! Befehle rufen — vertretbar, weil sie höchstens Ton und ein Fenster
//! betreffen. Nicht vertretbar wäre, dass eine WEBSEITE im Browser des
//! Piloten sie auslöst (die GET-Routen tragen `Access-Control-Allow-Origin: *`,
//! ein `text/plain`-POST braucht keinen Preflight). Deshalb gilt für
//! `/panel/cmd/*` ([`deck_anfrage_erlaubt`]): kein `Origin`-Kopf (Browser
//! senden ihn immer, Node nie), Pflichtkopf [`DECK_KOPF`] (erzwänge einen
//! Preflight, den hier niemand beantwortet), keine CORS-Antwortköpfe. Was
//! einen Flug beenden oder abbrechen kann, gehört NIE in [`DeckBefehl`].

use std::future::Future as _;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::{
    extract::{ConnectInfo, Path as UrlPfad, Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
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
/// v3 (Etappe 5, P-8.20): `panel.js`/`panel.css` kommen ueber den Lader aus
/// derselben Generation wie Hauptfenster und LAN (`hud/` im Frontend-Bau,
/// von `vite.config.ts` aus dem msfs-panel-Paket kopiert — weiter eine
/// Quelle mit Flow-Widget und nativem Panel). Der Browser nimmt
/// automatisch den "nativen" Einstieg des Skripts (kein Flow-`run()`),
/// und `aa2-nativ` am Body waehlt die fuellende Positionierung.
async fn hud_handler(State(ctx): State<PanelCtx>) -> Response {
    let text = |pfad: &str| {
        crate::ui_lader::datei(&ctx.app, pfad).and_then(|d| String::from_utf8(d.bytes).map_err(|e| e.to_string()))
    };
    match (text("hud/panel.js"), text("hud/panel.css")) {
        (Ok(js), Ok(css)) => axum::response::Html(hud_seite(&js, &css)).into_response(),
        (Err(e), _) | (_, Err(e)) => {
            tracing::warn!(error = %e, "panel_server: HUD nicht in der Oberflaeche");
            (StatusCode::SERVICE_UNAVAILABLE, "HUD nicht verfuegbar").into_response()
        }
    }
}

/// Die HUD-Seite aus Widget-Skript und -Stil.
fn hud_seite(js: &str, css: &str) -> String {
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
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <title>AeroACARS HUD</title><style>{CSS}\
         html,body{{margin:0;padding:0;background:transparent;}}\
         .aa2-strip{{position:static;display:inline-flex;width:auto;\
         background:#101820;}}\
         .aa2-strip.aa2-quiet{{opacity:1;}}\
         .aa2-strip:empty{{display:none;}}</style></head>\
         <body><div class=\"aa2-strip\"></div>\
         <script>{js}</script></body></html>",
        CSS = css
    )
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
            .route("/panel/kabine", get(kabine_handler))
            .route("/panel/hoppie", get(hoppie_handler))
            .route("/panel/cmd/{name}", post(cmd_handler))
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
pub(crate) fn with_display_callsign(value: &mut serde_json::Value) {
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
    let beginn = std::time::Instant::now();
    let record = crate::landing_get_current(ctx.app.clone(), state);
    crate::landungsabfrage_protokollieren("panel_debrief", record.as_ref(), beginn.elapsed());
    let value = debrief_fuer_anzeige(serde_json::to_value(record).unwrap_or(Value::Null));
    cors_open((StatusCode::OK, Json(value)).into_response())
}

/// Die Landung, wie HUD-Band, MSFS-Panel und Flow-Widget sie bekommen.
///
/// Bei „nicht bewertbar" steht dort statt eines leeren Etiketts „nicht
/// bewertbar" — die Anzeigen schreiben das Etikett unverändert (groß) hin
/// und brauchen dafür kein eigenes Update. Nur hier, nicht im gespeicherten
/// Datensatz: `score_label` ist dort eine feste Kategorie (`smooth` …), an
/// der Übersetzung und Historie hängen.
pub(crate) fn debrief_fuer_anzeige(mut v: Value) -> Value {
    let nicht_bewertbar = v
        .get("landung_nicht_bewertbar")
        .is_some_and(|n| !n.is_null());
    let ohne_etikett = v.get("score_label").is_none_or(Value::is_null);
    if nicht_bewertbar && ohne_etikett {
        if let Some(o) = v.as_object_mut() {
            o.insert(
                "score_label".into(),
                Value::String("nicht bewertbar".into()),
            );
        }
    }
    v
}

#[cfg(test)]
mod debrief_tests {
    use super::debrief_fuer_anzeige;
    use serde_json::json;

    #[test]
    fn nicht_bewertbar_bekommt_ein_etikett_fuer_die_anzeigen() {
        let v = debrief_fuer_anzeige(json!({
            "score_label": null,
            "landung_nicht_bewertbar": {"groesste_luecke_ms": 300, "proben": 5}
        }));
        assert_eq!(v["score_label"], "nicht bewertbar");
        // Eine echte Note bleibt unangetastet, eine bewertete Landung auch.
        let v =
            debrief_fuer_anzeige(json!({"score_label": "hard", "landung_nicht_bewertbar": null}));
        assert_eq!(v["score_label"], "hard");
        let v = debrief_fuer_anzeige(json!({"score_label": null}));
        assert!(v["score_label"].is_null());
        assert!(debrief_fuer_anzeige(json!(null)).is_null());
    }
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

// ---------------------------------------------------------------------------
// StreamDeck
// ---------------------------------------------------------------------------

/// Pflichtkopf auf `/panel/cmd/*`, Wert `1`. Siehe Modul-Doku „StreamDeck“.
pub(crate) const DECK_KOPF: &str = "x-aeroacars-deck";

/// Darf diese Anfrage einen Befehl auslösen? Eigene Funktion, damit der
/// Test genau diese Entscheidung an echten Köpfen prüft.
fn deck_anfrage_erlaubt(koepfe: &HeaderMap) -> Result<(), StatusCode> {
    if koepfe.contains_key(header::ORIGIN) {
        return Err(StatusCode::FORBIDDEN);
    }
    match koepfe.get(DECK_KOPF) {
        Some(v) if v.as_bytes() == b"1" => Ok(()),
        _ => Err(StatusCode::FORBIDDEN),
    }
}

/// Die Befehle, die das Deck auslösen darf — abschließend.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DeckBefehl {
    Stumm(bool),
    Lautstaerke { wert: f32, speichern: bool },
    MusikLautstaerke { wert: f32, speichern: bool },
    BoardingMusik(bool),
    CdmFenster,
    /// Hauptfenster nach vorn holen, auf eine Seite aus [`DECK_SEITEN`].
    Fenster(&'static str),
    /// METAR für die Anzeige nachladen; `true` = Ziel, `false` = Start.
    Metar(bool),
}

/// Seiten der App, die das Deck per langem Druck öffnen darf. Die Namen sind
/// die `Tab`-Werte in `client/src/App.tsx`.
const DECK_SEITEN: &[&str] = &["cockpit", "briefing", "landing", "kabine", "map", "cpdlc"];

/// Ereignis an die Oberfläche: auf diese Seite wechseln.
pub(crate) const EREIGNIS_DECK_SEITE: &str = "deck-seite";

impl DeckBefehl {
    /// 404 für jeden Namen außerhalb der Liste, 422 für fehlende oder
    /// unsinnige Werte. Lautstärken werden HIER geprüft und nicht erst von
    /// `normalisiert()` — das machte aus NaN still den Werkswert und
    /// überschriebe so die Einstellung des Piloten.
    fn lesen(name: &str, args: &Value) -> Result<Self, StatusCode> {
        let wahr = |k: &str| args.get(k).and_then(Value::as_bool).ok_or(StatusCode::UNPROCESSABLE_ENTITY);
        let pegel = || {
            let wert = args
                .get("lautstaerke")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && (0.0..=1.0).contains(v))
                .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
            Ok::<_, StatusCode>((wert as f32, wahr("speichern")?))
        };
        match name {
            "kabinen_stumm" => Ok(Self::Stumm(wahr("stumm")?)),
            "kabinen_lautstaerke" => pegel().map(|(wert, speichern)| Self::Lautstaerke { wert, speichern }),
            "kabinen_musik_lautstaerke" => {
                pegel().map(|(wert, speichern)| Self::MusikLautstaerke { wert, speichern })
            }
            "kabinen_boarding_musik" => Ok(Self::BoardingMusik(wahr("an")?)),
            "vdgs_fenster_oeffnen" => Ok(Self::CdmFenster),
            "metar_laden" => match args.get("ort").and_then(Value::as_str) {
                Some("start") => Ok(Self::Metar(false)),
                Some("ziel") => Ok(Self::Metar(true)),
                _ => Err(StatusCode::UNPROCESSABLE_ENTITY),
            },
            "fenster_zeigen" => {
                let seite = args.get("seite").and_then(Value::as_str).unwrap_or("");
                DECK_SEITEN
                    .iter()
                    .find(|s| **s == seite)
                    .map(|s| Self::Fenster(s))
                    .ok_or(StatusCode::UNPROCESSABLE_ENTITY)
            }
            _ => Err(StatusCode::NOT_FOUND),
        }
    }
}

/// Zustand der Kabine für das Deck — schmal statt des ganzen Überblicks
/// (der trägt Verlauf, Profil, Kapitänsname). Beide Lautstärken doppelt:
/// `live` ist, was gerade klingt, `gespeichert` die Einstellung. Daraus
/// liest das Plugin „Musik stumm“ (live 0, gespeichert > 0), ohne sich
/// selbst etwas merken zu müssen.
fn kabine_zusammenfassen(
    einst: &crate::kabinenansagen::Einstellungen,
    stumm: bool,
    ansage_live: f32,
    musik_live: f32,
    ueberblick: Option<&Value>,
) -> Value {
    let kurz = |g: &Value| {
        serde_json::json!({
            "slot": g.get("slot"),
            "wann": g.get("wann"),
            "faellig": g.get("faellig"),
            "wartet": g.get("wartet"),
        })
    };
    let geplant = ueberblick.and_then(|u| u.get("geplant")).and_then(Value::as_array);
    let naechste = geplant.and_then(|g| g.first()).map(kurz);
    // Für den Kabinen-Zeitstrahl am Deck: die nächsten vier.
    let folgende: Vec<Value> = geplant.map(|g| g.iter().take(4).map(kurz).collect()).unwrap_or_default();
    let feld = |k: &str| ueberblick.and_then(|u| u.get(k)).cloned().unwrap_or(Value::Null);
    serde_json::json!({
        "an": einst.an,
        "stumm": stumm,
        "lautstaerke_live": ansage_live,
        "lautstaerke_gespeichert": einst.lautstaerke,
        "musik_live": musik_live,
        "musik_gespeichert": einst.musik_lautstaerke,
        "boarding_musik": einst.boarding_musik,
        "flug": ueberblick.is_some(),
        "grund": feld("grund"),
        "belegt": feld("belegt"),
        "hinweis": feld("hinweis"),
        "naechste": naechste,
        "folgende": folgende,
    })
}

fn kabine_fuer_deck(app: &AppHandle) -> Value {
    let einst = crate::kabinenansagen::kabinen_einstellungen_lesen(app.clone());
    let ueberblick = crate::kabinenansagen::kabinen_ueberblick(app.clone(), app.state::<crate::AppState>())
        .and_then(|u| serde_json::to_value(u).ok());
    kabine_zusammenfassen(
        &einst,
        crate::kabinenansagen::ist_stumm(),
        crate::kabinenansagen::ausgabe::lautstaerke(),
        crate::kabinen_musik::lautstaerke(),
        ueberblick.as_ref(),
    )
}

async fn kabine_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(PanelPeer(peer)): ConnectInfo<PanelPeer>,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    cors_open((StatusCode::OK, Json(kabine_fuer_deck(&ctx.app))).into_response())
}

async fn hoppie_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(PanelPeer(peer)): ConnectInfo<PanelPeer>,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    let value = match crate::hoppie::hoppie_status(ctx.app.state::<crate::AppState>()).await {
        Ok(s) => serde_json::json!({
            "connected": s.connected,
            "logged_on": s.logged_on,
            "station_id": s.station_id,
            "pending_uplink_count": s.pending_uplink_count,
            "pending_response_count": s.pending_response_count,
        }),
        Err(_) => Value::Null,
    };
    cors_open((StatusCode::OK, Json(value)).into_response())
}

/// Bewusst OHNE [`cors_open`]: Browser bekommen keine Antwort zu lesen.
async fn cmd_handler(
    State(ctx): State<PanelCtx>,
    ConnectInfo(PanelPeer(peer)): ConnectInfo<PanelPeer>,
    UrlPfad(name): UrlPfad<String>,
    koepfe: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    if let Some(r) = reject_non_loopback(peer) {
        return r;
    }
    if let Err(code) = deck_anfrage_erlaubt(&koepfe) {
        tracing::warn!(%name, "panel_server: Deck-Befehl ohne Deck-Kopf oder mit Origin abgewiesen");
        return (code, "forbidden").into_response();
    }
    let args = if body.is_empty() {
        Value::Object(Default::default())
    } else {
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) => v,
            Err(_) => return (StatusCode::UNPROCESSABLE_ENTITY, "bad json").into_response(),
        }
    };
    let befehl = match DeckBefehl::lesen(&name, &args) {
        Ok(b) => b,
        Err(code) => return (code, "unknown command or bad arguments").into_response(),
    };
    let app = ctx.app.clone();
    let ergebnis = match befehl {
        DeckBefehl::Stumm(stumm) => {
            crate::kabinenansagen::kabinen_stumm(app.clone(), app.state::<crate::AppState>(), stumm);
            Ok(())
        }
        DeckBefehl::Lautstaerke { wert, speichern } => {
            crate::kabinenansagen::kabinen_lautstaerke(app.clone(), wert, speichern)
        }
        DeckBefehl::MusikLautstaerke { wert, speichern } => {
            crate::kabinenansagen::kabinen_musik_lautstaerke(app.clone(), wert, speichern)
        }
        DeckBefehl::BoardingMusik(an) => crate::kabinenansagen::kabinen_boarding_musik(app.clone(), an),
        DeckBefehl::CdmFenster => crate::vdgs_fenster_oeffnen(app.clone()).await,
        DeckBefehl::Metar(ziel) => crate::metar_nachladen(&app, ziel).await,
        DeckBefehl::Fenster(seite) => {
            // Wie „Anzeigen“ im Tray-Menü, dazu der Seitenwechsel.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            let _ = tauri::Emitter::emit(&app, EREIGNIS_DECK_SEITE, seite);
            Ok(())
        }
    };
    match ergebnis {
        // Antwort = frischer Kabinenzustand, damit die Taste sofort stimmt.
        Ok(()) => (StatusCode::OK, Json(kabine_fuer_deck(&app))).into_response(),
        Err(e) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({ "code": e.code, "message": e.message })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod deck_tests {
    use super::*;
    use serde_json::json;

    fn koepfe(paare: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in paare {
            h.insert(
                axum::http::HeaderName::from_bytes(k.as_bytes()).unwrap(),
                HeaderValue::from_str(v).unwrap(),
            );
        }
        h
    }

    #[test]
    fn nur_das_deck_darf_befehle_ausloesen() {
        assert!(deck_anfrage_erlaubt(&koepfe(&[(DECK_KOPF, "1")])).is_ok());
        // Ohne Kopf: z. B. ein `text/plain`-POST aus einer Webseite.
        assert_eq!(deck_anfrage_erlaubt(&koepfe(&[])), Err(StatusCode::FORBIDDEN));
        assert_eq!(deck_anfrage_erlaubt(&koepfe(&[(DECK_KOPF, "0")])), Err(StatusCode::FORBIDDEN));
        // Mit Kopf, aber aus einem Browser (Origin): trotzdem nein.
        assert_eq!(
            deck_anfrage_erlaubt(&koepfe(&[(DECK_KOPF, "1"), ("origin", "https://example.com")])),
            Err(StatusCode::FORBIDDEN)
        );
        assert_eq!(
            deck_anfrage_erlaubt(&koepfe(&[(DECK_KOPF, "1"), ("origin", "null")])),
            Err(StatusCode::FORBIDDEN)
        );
    }

    #[test]
    fn die_befehlsliste_ist_abschliessend() {
        assert_eq!(DeckBefehl::lesen("kabinen_stumm", &json!({"stumm": true})), Ok(DeckBefehl::Stumm(true)));
        assert_eq!(
            DeckBefehl::lesen("kabinen_musik_lautstaerke", &json!({"lautstaerke": 0.0, "speichern": false})),
            Ok(DeckBefehl::MusikLautstaerke { wert: 0.0, speichern: false })
        );
        assert_eq!(
            DeckBefehl::lesen("kabinen_lautstaerke", &json!({"lautstaerke": 0.5, "speichern": true})),
            Ok(DeckBefehl::Lautstaerke { wert: 0.5, speichern: true })
        );
        assert_eq!(DeckBefehl::lesen("kabinen_boarding_musik", &json!({"an": false})), Ok(DeckBefehl::BoardingMusik(false)));
        assert_eq!(DeckBefehl::lesen("vdgs_fenster_oeffnen", &json!({})), Ok(DeckBefehl::CdmFenster));
        assert_eq!(
            DeckBefehl::lesen("fenster_zeigen", &json!({"seite": "cpdlc"})),
            Ok(DeckBefehl::Fenster("cpdlc"))
        );
        assert_eq!(DeckBefehl::lesen("metar_laden", &json!({"ort": "ziel"})), Ok(DeckBefehl::Metar(true)));
        assert_eq!(DeckBefehl::lesen("metar_laden", &json!({"ort": "start"})), Ok(DeckBefehl::Metar(false)));
        assert_eq!(DeckBefehl::lesen("metar_laden", &json!({"ort": "EDDF"})), Err(StatusCode::UNPROCESSABLE_ENTITY));
        // Nur Seiten aus der Liste: keine Einstellungen, kein Logbuch-Löschen o. Ä.
        for seite in ["settings", "logbook", "", "../x"] {
            assert_eq!(
                DeckBefehl::lesen("fenster_zeigen", &json!({"seite": seite})),
                Err(StatusCode::UNPROCESSABLE_ENTITY),
                "{seite}"
            );
        }
        // Alles, was einen Flug beendet, abbricht oder abgibt, bleibt draußen.
        for name in [
            "flight_end",
            "flight_end_manual",
            "flight_cancel",
            "flight_forget",
            "flight_start",
            "phpvms_logout",
            "kabinen_einstellungen_setzen",
            "remote_server_start",
        ] {
            assert_eq!(DeckBefehl::lesen(name, &json!({})), Err(StatusCode::NOT_FOUND), "{name}");
        }
    }

    #[test]
    fn unsinnige_lautstaerken_werden_abgewiesen_statt_auf_den_werkswert_gesetzt() {
        for wert in [json!(1.5), json!(-0.1), json!("0.5"), json!(null)] {
            assert_eq!(
                DeckBefehl::lesen("kabinen_lautstaerke", &json!({"lautstaerke": wert, "speichern": true})),
                Err(StatusCode::UNPROCESSABLE_ENTITY),
                "{wert}"
            );
        }
        // NaN/Unendlich kommen aus JSON nicht an; fehlendes `speichern` schon.
        assert_eq!(
            DeckBefehl::lesen("kabinen_musik_lautstaerke", &json!({"lautstaerke": 0.3})),
            Err(StatusCode::UNPROCESSABLE_ENTITY)
        );
        assert_eq!(DeckBefehl::lesen("kabinen_stumm", &json!({})), Err(StatusCode::UNPROCESSABLE_ENTITY));
    }

    #[test]
    fn kabinenzustand_ohne_flug_und_mit_naechster_ansage() {
        let einst = crate::kabinenansagen::Einstellungen {
            lautstaerke: 0.8,
            musik_lautstaerke: 0.2,
            boarding_musik: true,
            ..Default::default()
        };
        // Ohne Flug: Schalter und Pegel trotzdem da, keine nächste Ansage.
        let v = kabine_zusammenfassen(&einst, false, 0.8, 0.0, None);
        assert_eq!(v["flug"], false);
        assert_eq!(v["musik_live"], 0.0);
        assert!((v["musik_gespeichert"].as_f64().unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(v["boarding_musik"], true);
        assert!(v["naechste"].is_null());

        let u = json!({
            "geplant": [
                {"slot": "kabine_sinkflug", "wann": 1_800_000_000, "faellig": false, "wartet": null, "teile": []},
                {"slot": "nach_der_landung", "wann": null, "faellig": false, "wartet": null}
            ],
            "hinweis": "anschnallzeichen", "belegt": false, "grund": null, "kapitaen": "Max Muster"
        });
        let v = kabine_zusammenfassen(&einst, true, 0.8, 0.2, Some(&u));
        assert_eq!(v["flug"], true);
        assert_eq!(v["stumm"], true);
        assert_eq!(v["naechste"]["slot"], "kabine_sinkflug");
        assert_eq!(v["naechste"]["wann"], 1_800_000_000);
        assert_eq!(v["folgende"].as_array().map(Vec::len), Some(2));
        assert_eq!(v["folgende"][1]["slot"], "nach_der_landung");
        assert_eq!(v["hinweis"], "anschnallzeichen");
        // Schmal: der Kapitänsname geht nicht über die offene Route.
        assert!(v.get("kapitaen").is_none());
    }
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
        let html = super::hud_seite(
            include_str!("../../../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/panel.js"),
            include_str!("../../../msfs-panel/Build/PackageSources/html_ui/InGamePanels/AeroACARSPanel/panel.css"),
        );
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
