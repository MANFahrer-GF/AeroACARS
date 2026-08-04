//! Hoppie ACARS network PDC/CPDLC wiring (v1.3.0, #Hoppie-PDC-CPDLC —
//! see docs/spec/v1.3.0-hoppie-pdc-cpdlc.md).
//!
//! AeroACARS talks to the free Hoppie ACARS network
//! (`hoppie.nl/acars/`) to let a pilot request a PDC (Pre-Departure
//! Clearance) and exchange CPDLC messages without leaving the app —
//! the protocol/data logic (wire codec, GOLD element table, MIN/MRN
//! threading, PDC formatting) lives in the pure `hoppie-protocol`
//! crate; this module is the thin Tauri-facing wiring layer: shared
//! HTTP client, settings/secrets persistence, the background poller,
//! and the commands the settings panel + (from Phase 2 on) the CPDLC
//! tab call.
//!
//! ## Opt-in by default
//!
//! [`settings::HoppieSettings::enabled`] defaults to `false`. Until a
//! pilot switches it on AND stores a logon code, [`hoppie_connect`]
//! refuses to start — no poller, no requests to hoppie.nl, no
//! notifications. Pilots who don't want PDC/CPDLC at all are entirely
//! unaffected, matching how the LAN remote-control server
//! ([`crate::remote`]) is opt-in.
//!
//! ## Logon-code validation
//!
//! The official docs (`hoppie.nl/acars/system/tech.html`) describe a
//! `ping` request as the way to "test whether the link works" without
//! registering the station as online or locking the callsign — see
//! [`verify_logon`]. [`hoppie_verify_logon_code`] exposes this
//! standalone (for a "Test code" button in the settings panel, before
//! the pilot even saves it), and [`hoppie_connect`] runs the same
//! check before starting the poller, so a stale/typo'd code surfaces
//! immediately as a clear error instead of polling silently into the
//! void.
//!
//! ## Lifecycle
//!
//! [`HoppieHandle`] mirrors `remote::RemoteServerHandle`'s shape: an
//! `Option<HoppieHandle>` field on `AppState`, `Some` while the poller
//! is running, dropping it (via [`hoppie_disconnect`] or app shutdown)
//! fires the stop signal.

pub mod poller;
pub mod settings;

use std::sync::{Arc, Mutex as StdMutex};

use serde::Serialize;
use tauri::AppHandle;
use tokio::sync::watch;

use crate::{log_activity_handle, ActivityLevel, AppState, UiError};

pub use settings::HoppieSettings;

/// `crates/secrets` account name for the Hoppie logon code — treated
/// as a credential, same as the existing MQTT/phpVMS API keys.
const HOPPIE_LOGON_CODE_ACCOUNT: &str = "hoppie_logon_code";

const BASE_URL: &str = "https://www.hoppie.nl/acars/system/connect.html";

/// Shared HTTP client — built ONCE and reused, mirroring
/// `crates/api-client`'s `Client::new` (which explicitly warns against
/// constructing a fresh `reqwest::Client` per request). The rustls
/// `CryptoProvider` pitfall that comment describes is moot here in
/// practice, since `run()` installs the process-wide default before
/// `.setup()` (and therefore this code) ever executes — but reusing
/// one client is still the right call for connection pooling.
pub struct HoppieHttp {
    http: reqwest::Client,
}

impl HoppieHttp {
    fn new() -> Result<Self, UiError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
            // 15s connection timeout per the official docs'
            // recommendation (this is the CONNECT timeout, not the
            // polling rate — see poller.rs for that).
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| UiError::new("hoppie_http_init", e.to_string()))?;
        Ok(Self { http })
    }

    async fn send(
        &self,
        req: &hoppie_protocol::wire::HoppieRequest,
    ) -> Result<hoppie_protocol::wire::HoppieResponseLine, UiError> {
        let pairs = hoppie_protocol::wire::query_pairs(req);
        // POST, not GET. The official docs: "in most cases, you will
        // want to use POST protocol to avoid having to URL-encode the
        // packet and/or run into maximum URL length limits. Good
        // practice is to always keep URLs under 256 characters." A
        // single free-text element blows past that as a query string —
        // the logon code alone is ~24 chars and every '/' in the
        // /data2/ prefix percent-encodes to three.
        let resp = self
            .http
            .post(BASE_URL)
            .form(&pairs)
            .send()
            .await
            .map_err(redact_transport_error)?;
        let body = resp.text().await.map_err(redact_transport_error)?;
        hoppie_protocol::wire::parse_response(&body)
            .map_err(|e| UiError::new("hoppie_protocol", e.to_string()))
    }
}

/// Turn a transport failure into something safe to show a pilot.
///
/// `reqwest`'s `Display` appends the request URL, and ours carries
/// `?logon=<the pilot's secret>`. That string was ending up in a UI
/// tooltip. The real cause still reaches the log, where it belongs.
fn redact_transport_error(e: reqwest::Error) -> UiError {
    tracing::warn!(error = %e, "hoppie: transport error");
    let kind = if e.is_timeout() {
        "Zeitüberschreitung"
    } else if e.is_connect() {
        "Keine Verbindung zum Hoppie-Netz"
    } else {
        "Netzwerkfehler"
    };
    UiError::new("hoppie_network", kind)
}

/// Result of a logon-code check.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyOutcome {
    pub valid: bool,
    pub reason: Option<String>,
}

/// Test a logon code with a single side-effect-free `ping` (see the
/// module docs). Shared by [`hoppie_verify_logon_code`] (standalone,
/// settings-panel "Test code" button) and [`hoppie_connect`] (run once
/// automatically before starting the poller).
async fn verify_logon(http: &HoppieHttp, logon: &str, callsign: &str) -> VerifyOutcome {
    let req = hoppie_protocol::wire::HoppieRequest {
        logon: logon.to_string(),
        from: callsign.to_string(),
        to: settings::DEFAULT_STATION_ID.to_string(),
        kind: hoppie_protocol::wire::PacketKind::Ping,
        packet: None,
    };
    match http.send(&req).await {
        Ok(hoppie_protocol::wire::HoppieResponseLine::Ok)
        | Ok(hoppie_protocol::wire::HoppieResponseLine::OkWithPayload(_)) => VerifyOutcome {
            valid: true,
            reason: None,
        },
        Ok(hoppie_protocol::wire::HoppieResponseLine::Error(reason)) => VerifyOutcome {
            valid: false,
            reason: Some(reason),
        },
        Err(e) => VerifyOutcome {
            valid: false,
            reason: Some(e.message),
        },
    }
}

/// When each message was sent or received, keyed by `(is_uplink, MIN)`.
///
/// The direction is part of the key because the two MIN spaces are
/// independent — ATC numbers uplinks, we number downlinks, and both
/// typically start near 1. Sharing one key let an inbound message
/// overwrite the timestamp of our own.
pub(crate) type MinTimestamps =
    std::collections::HashMap<(bool, u32), chrono::DateTime<chrono::Utc>>;

/// One sent or received telex/PDC-request-reply line. CPDLC messages
/// (MIN/MRN-threaded) live in `HoppieHandle::thread` instead — this is
/// only for the un-threaded plain-telex traffic PDC uses, which has no
/// GOLD element table entry of its own (see `hoppie-protocol::pdc`'s
/// docs).
pub(crate) struct TelexEntry {
    direction: &'static str,
    text: String,
    at: chrono::DateTime<chrono::Utc>,
    /// True when this arrived on the CPDLC channel but carried no
    /// parseable `/data2/` header — vSMR sends STANDBY, "UNABLE CALL ON
    /// FREQ" and its logon refusal that way. It has no MIN/MRN so it
    /// can't join the threaded history, but it must still surface in the
    /// CPDLC log rather than the PDC tab, which is a different
    /// conversation entirely.
    from_cpdlc_channel: bool,
}

/// Lives in `AppState::hoppie` while the poller is running, `None`
/// while stopped. Same start/stop-via-`Drop` shape as
/// `remote::RemoteServerHandle`.
pub struct HoppieHandle {
    stop_tx: watch::Sender<bool>,
    /// Same shared client the poller uses — commands issued after
    /// connect (e.g. sending a PDC request) reuse it rather than
    /// building a fresh one, per the "build once" principle in
    /// [`HoppieHttp`]'s docs.
    http: Arc<HoppieHttp>,
    thread: Arc<StdMutex<hoppie_protocol::thread::CpdlcThread>>,
    telex_log: Arc<StdMutex<Vec<TelexEntry>>>,
    /// (direction, MIN) -> when we sent/received it. The pure
    /// `CpdlcThread` is deliberately wall-clock-free (keeps it a pure,
    /// fast-testable state machine); this wiring-layer map is the only
    /// place a CPDLC message's timestamp lives.
    ///
    /// Keyed by direction as well as MIN because the two numbering
    /// spaces are INDEPENDENT: ATC assigns uplink MINs, we assign
    /// downlink MINs, and both commonly start near 1. A shared map let
    /// an inbound message overwrite the timestamp of our own — which
    /// reordered the log and could mask or fabricate a logon timeout.
    min_timestamps: Arc<StdMutex<MinTimestamps>>,
    last_error: Arc<StdMutex<Option<String>>>,
    last_verify: Option<VerifyOutcome>,
    /// Resolved at connect time — reused by every send command so they
    /// don't need to re-resolve settings/active-flight state.
    from_callsign: String,
    /// The ATC facility CPDLC messages are addressed to. Mutable because
    /// a CPDLC logon always names a specific facility and a pilot
    /// re-logs-on to the next one mid-flight (EDGG -> EDUU -> LOVV)
    /// without dropping the Hoppie connection.
    to_station: Arc<StdMutex<String>>,
}

impl Drop for HoppieHandle {
    fn drop(&mut self) {
        let _ = self.stop_tx.send(true);
    }
}

/// Returned by [`hoppie_connect`]/[`hoppie_disconnect`]/[`hoppie_status`].
#[derive(Debug, Clone, Serialize)]
pub struct HoppieStatus {
    pub connected: bool,
    pub logged_on: bool,
    pub pending_response_count: usize,
    /// Open UPLINKS only — what the pilot still owes ATC an answer for.
    /// The tab badge and the attention banner use THIS, never
    /// `pending_response_count`, which also counts our own outstanding
    /// requests (see `CpdlcThread::pending_uplink_count`).
    pub pending_uplink_count: usize,
    pub last_error: Option<String>,
    pub logon_verified: Option<VerifyOutcome>,
    /// The station every message in the thread is to/from — shown as
    /// a per-message label in the UI. `None` while disconnected.
    pub station_id: Option<String>,
    /// A `REQUEST LOGON` is out and its answer hasn't arrived yet — the
    /// DCDU's interim "LOGON SENT" state. Comes from the thread state
    /// machine rather than being guessed in the UI: inferring it from
    /// "we have sent some CPDLC message" was wrong the moment anything
    /// else had been sent, and left the header claiming a logon was in
    /// flight right after the pilot logged OFF.
    pub logon_pending: bool,
    /// A `REQUEST LOGON` has been outstanding longer than
    /// [`LOGON_TIMEOUT_SECS`]. Plenty of stations never answer one — a
    /// delivery desk, or any controller client without CPDLC logon — and
    /// without this the UI would sit on "logon sent" indefinitely.
    pub logon_timed_out: bool,
}

/// How long to wait for an answer to `REQUEST LOGON` before telling the
/// pilot it isn't coming. Three baseline poll cycles (60s each) — long
/// enough that a slow round trip isn't mistaken for silence.
const LOGON_TIMEOUT_SECS: i64 = 180;

fn build_status(handle: &Option<HoppieHandle>) -> HoppieStatus {
    match handle {
        Some(h) => {
            let thread = h.thread.lock().expect("hoppie thread mutex");
            let logon_pending = thread.pending_logon_min().is_some();
            let logon_timed_out = thread
                .pending_logon_min()
                .and_then(|min| {
                    h.min_timestamps
                        .lock()
                        .expect("hoppie min_timestamps mutex")
                        .get(&(false, min))
                        .copied()
                })
                .is_some_and(|sent| (chrono::Utc::now() - sent).num_seconds() > LOGON_TIMEOUT_SECS);
            HoppieStatus {
                connected: true,
                logged_on: thread.is_logged_on(),
                pending_response_count: thread.pending_response_count(),
                pending_uplink_count: thread.pending_uplink_count(),
                last_error: h
                    .last_error
                    .lock()
                    .expect("hoppie last_error mutex")
                    .clone(),
                logon_verified: h.last_verify.clone(),
                station_id: Some(
                    h.to_station
                        .lock()
                        .expect("hoppie to_station mutex")
                        .clone(),
                ),
                logon_pending,
                logon_timed_out,
            }
        }
        None => HoppieStatus {
            connected: false,
            logged_on: false,
            pending_response_count: 0,
            pending_uplink_count: 0,
            last_error: None,
            logon_verified: None,
            station_id: None,
            logon_pending: false,
            logon_timed_out: false,
        },
    }
}

// ----------------------------------------------------------------------
// Tauri commands
// ----------------------------------------------------------------------

#[tauri::command]
pub fn hoppie_get_settings(app: AppHandle) -> HoppieSettings {
    settings::read_settings(&app)
}

#[tauri::command]
pub fn hoppie_set_settings(app: AppHandle, settings: HoppieSettings) -> HoppieSettings {
    settings::write_settings(&app, &settings);
    settings
}

#[tauri::command]
pub fn hoppie_set_logon_code(code: String) -> Result<(), UiError> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Err(UiError::new(
            "hoppie_logon_code_empty",
            "Logon-Code darf nicht leer sein.",
        ));
    }
    secrets::store_api_key(HOPPIE_LOGON_CODE_ACCOUNT, trimmed)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))
}

#[tauri::command]
pub fn hoppie_has_logon_code() -> Result<bool, UiError> {
    Ok(secrets::load_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))?
        .is_some())
}

#[tauri::command]
pub fn hoppie_clear_logon_code() -> Result<(), UiError> {
    secrets::delete_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))
}

/// Whether a station is currently logged on to the Hoppie network.
#[derive(Debug, Clone, Serialize)]
pub struct StationStatus {
    pub station: String,
    /// `true` only when the network listed the station as online. A
    /// `false` means "not listed" — which is also what a network error
    /// looks like, hence `reason`.
    pub online: bool,
    /// Set when the check itself failed, as opposed to the station
    /// simply not being there. The UI must not claim "offline" when it
    /// actually means "couldn't ask".
    pub reason: Option<String>,
}

/// Ask the network whether `station` is online, so the pilot isn't left
/// sending a clearance request into a void.
///
/// The protocol has no delivery or read receipt: `ok` on a send only
/// means the message reached the addressee's mailbox, and `peek` shows
/// our own mailbox, not theirs. Whether a controller is even connected
/// is the one thing we CAN establish — via `ping`, which the docs
/// describe as side-effect-free (it does not register us as online or
/// lock the callsign), so it is safe to call before every send.
#[tauri::command]
pub async fn hoppie_ping_station(
    state: tauri::State<'_, AppState>,
    station: String,
) -> Result<StationStatus, UiError> {
    let station = station.trim().to_uppercase();
    if station.is_empty() {
        return Err(UiError::new(
            "hoppie_no_station",
            "Keine Station angegeben.",
        ));
    }
    // Take what we need and DROP the lock before the round trip. Holding
    // it across a request that can run into the 15s timeout would stall
    // every other command on the same mutex — including the status poll
    // and a pilot pressing WILCO on a live clearance.
    let (http, from_callsign) = {
        let guard = state.hoppie.lock().await;
        let handle = guard.as_ref().ok_or_else(|| {
            UiError::new(
                "hoppie_not_connected",
                "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
            )
        })?;
        (Arc::clone(&handle.http), handle.from_callsign.clone())
    };
    let logon = resolve_logon_code()?;

    let req = hoppie_protocol::wire::HoppieRequest {
        logon,
        from: from_callsign,
        // A ping is addressed to the SERVER, not to the station being
        // asked about. The docs: the `to` field is "ignored for certain
        // messages that are essentially sent to the server ... but you
        // still need to provide something. Such as SERVER." Putting the
        // station here risks the server rejecting an unknown callsign,
        // which would surface as "couldn't check" for every typo.
        to: settings::DEFAULT_STATION_ID.to_string(),
        kind: hoppie_protocol::wire::PacketKind::Ping,
        // The payload names who we're asking about; the reply lists
        // whichever of them are online.
        packet: Some(station.clone()),
    };
    match http.send(&req).await {
        Ok(hoppie_protocol::wire::HoppieResponseLine::OkWithPayload(body)) => {
            let online = hoppie_protocol::wire::parse_ping_stations(&body)
                .iter()
                .any(|s| s.eq_ignore_ascii_case(&station));
            Ok(StationStatus {
                station,
                online,
                reason: None,
            })
        }
        // A bare `ok` carries no station list — nobody matched.
        Ok(hoppie_protocol::wire::HoppieResponseLine::Ok) => Ok(StationStatus {
            station,
            online: false,
            reason: None,
        }),
        Ok(hoppie_protocol::wire::HoppieResponseLine::Error(reason)) => Ok(StationStatus {
            station,
            online: false,
            reason: Some(reason),
        }),
        Err(e) => Ok(StationStatus {
            station,
            online: false,
            reason: Some(e.message),
        }),
    }
}

/// Start the poller. Idempotent — returns the current status without
/// double-starting if already connected (the `AppState` mutex is held
/// across the whole start, so concurrent callers serialize, same as
/// `remote::start_server`). Verifies the stored logon code BEFORE
/// starting the poll loop — see the module docs.
#[tauri::command]
pub async fn hoppie_connect(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<HoppieStatus, UiError> {
    let mut guard = state.hoppie.lock().await;
    if guard.is_some() {
        return Ok(build_status(&guard));
    }

    let settings = settings::read_settings(&app);
    if !settings.enabled {
        return Err(UiError::new(
            "hoppie_disabled",
            "Hoppie ACARS ist in den Einstellungen deaktiviert.",
        ));
    }
    let logon = match secrets::load_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))?
    {
        Some(code) => code,
        None => {
            return Err(UiError::new(
                "hoppie_no_logon_code",
                "Kein Hoppie-Logon-Code hinterlegt.",
            ))
        }
    };
    // Explicit override wins; otherwise fall back to the active flight's
    // callsign (same direct-mutex-read pattern every other subsystem
    // uses for `ActiveFlight` — no pub/sub, see `flight_context`).
    let from = match resolve_callsign(&app) {
        Some(cs) => cs,
        None => {
            return Err(UiError::new(
                "hoppie_no_callsign",
                "Kein Callsign hinterlegt und kein aktiver Flug — bitte in den Hoppie-Einstellungen setzen.",
            ))
        }
    };

    let http = Arc::new(HoppieHttp::new()?);
    let verify = verify_logon(&http, &logon, &from).await;
    if !verify.valid {
        return Err(UiError::new(
            "hoppie_invalid_logon",
            verify
                .reason
                .clone()
                .unwrap_or_else(|| "Logon-Code ungültig.".to_string()),
        ));
    }

    // Clean up ONLY a session we actually left open. `open_session` is
    // written when a logon is accepted and cleared on logoff, so a
    // marker here means the previous run died without logging off
    // (crash, kill, power loss) and the facility still holds us.
    //
    // Firing this unconditionally was wrong and controller-visible: an
    // unsolicited LOGOFF matches none of vSMR's filters and lands as an
    // unread message, so every app restart made the tag of a controller
    // we had never spoken to blink (SMRPlugin.cpp:162/:176 fall through
    // to :182). On a fresh install it would even go to the "SERVER"
    // placeholder.
    if let Some(stale) = settings::open_session(&app) {
        let stale_logoff = hoppie_protocol::wire::HoppieRequest {
            logon: logon.clone(),
            from: from.clone(),
            to: stale.clone(),
            // The previous session's MIN sequence died with it and this
            // expects no reply, so the number carries no meaning. Kept
            // clear of the new session's range (which starts at 1) so a
            // MIN-tracking controller client doesn't see a duplicate.
            packet: Some("/data2/9999//N/LOGOFF".to_string()),
            kind: hoppie_protocol::wire::PacketKind::Cpdlc,
        };
        match http.send(&stale_logoff).await {
            Ok(_) => {
                tracing::info!(station = %stale, "hoppie: closed session left open by the previous run")
            }
            Err(e) => {
                tracing::debug!(error = %e.message, "hoppie: stale-session LOGOFF failed (harmless)")
            }
        }
        settings::clear_open_session(&app);
    }

    let thread = Arc::new(StdMutex::new(hoppie_protocol::thread::CpdlcThread::new()));
    let telex_log = Arc::new(StdMutex::new(Vec::new()));
    let min_timestamps = Arc::new(StdMutex::new(std::collections::HashMap::new()));
    let last_error = Arc::new(StdMutex::new(None));
    // Shared with the poller so an automatic sector handover re-points
    // both the poll loop and every send command at the new facility.
    let to_station = Arc::new(StdMutex::new(settings.station_id.clone()));
    let from_for_log = from.clone();
    let (stop_tx, stop_rx) = watch::channel(false);
    poller::spawn(
        app.clone(),
        Arc::clone(&http),
        Arc::clone(&thread),
        Arc::clone(&telex_log),
        Arc::clone(&min_timestamps),
        Arc::clone(&last_error),
        from.clone(),
        logon,
        Arc::clone(&to_station),
        settings.notify_os,
        stop_rx,
    );

    *guard = Some(HoppieHandle {
        stop_tx,
        http,
        thread,
        telex_log,
        min_timestamps,
        last_error,
        last_verify: Some(verify),
        from_callsign: from.clone(),
        to_station: Arc::clone(&to_station),
    });
    log_activity_handle(
        &app,
        ActivityLevel::Info,
        format!("Hoppie: Empfang gestartet als {from_for_log}"),
        None,
    );
    Ok(build_status(&guard))
}

/// Send `LOGOFF` if a CPDLC session is open, so the facility stops
/// showing us as connected and stops queueing messages for us.
/// Best-effort: a failure must never block disconnecting.
async fn logoff_if_logged_on(app: &AppHandle, handle: &HoppieHandle) {
    {
        let t = handle.thread.lock().expect("hoppie thread mutex");
        // Nothing to end AND nothing outstanding — stay quiet rather than
        // send an unsolicited LOGOFF a controller would see as an unread
        // message from an aircraft they never spoke to.
        if !t.is_logged_on() && t.pending_logon_min().is_none() {
            return;
        }
    }
    let Ok(logon) = resolve_logon_code() else {
        return;
    };
    let Some(spec) = hoppie_protocol::elements::find("DM_LOGOFF") else {
        return;
    };
    if let Err(e) = send_cpdlc_element(&app, handle, logon, spec, Vec::new(), None).await {
        tracing::warn!(error = %e.message, "hoppie: LOGOFF failed");
    } else {
        tracing::info!("hoppie: LOGOFF sent");
    }
}

/// Same as [`logoff_if_logged_on`] but also forgets the persisted
/// open-session marker, so the next connect has nothing to clean up.
async fn logoff_and_forget(app: &AppHandle, handle: &HoppieHandle) {
    logoff_if_logged_on(app, handle).await;
    settings::clear_open_session(app);
}

/// Stop the poller (no-op if not running). Ends the CPDLC session first
/// — leaving it open means the controller still sees the aircraft as
/// connected and the network keeps queueing messages, which then all
/// arrive at once on the next start. Dropping the handle fires the stop
/// signal.
#[tauri::command]
pub async fn hoppie_disconnect(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<HoppieStatus, UiError> {
    let mut guard = state.hoppie.lock().await;
    let was_running = guard.is_some();
    if let Some(handle) = guard.as_ref() {
        logoff_and_forget(&app, handle).await;
    }
    *guard = None;
    // Logged so a "it stayed connected" report can be checked against
    // what actually happened, instead of guessed at.
    if was_running {
        log_activity_handle(&app, ActivityLevel::Info, "Hoppie: Empfang gestoppt", None);
    }
    Ok(build_status(&guard))
}

/// Shutdown hook: end the CPDLC session before the process goes away.
/// Called from `lib.rs`'s `ExitRequested` handler, alongside the MQTT
/// publisher teardown.
pub async fn shutdown(app: &AppHandle, state: &AppState) {
    let mut guard = state.hoppie.lock().await;
    if guard.is_some() {
        tracing::info!("hoppie: shutting down, ending any CPDLC session");
    }
    if let Some(handle) = guard.as_ref() {
        logoff_and_forget(app, handle).await;
    }
    *guard = None;
}

#[tauri::command]
pub async fn hoppie_status(state: tauri::State<'_, AppState>) -> Result<HoppieStatus, UiError> {
    let guard = state.hoppie.lock().await;
    Ok(build_status(&guard))
}

/// Best-effort prefill context for the PDC request form, read directly
/// off `AppState::active_flight` (same direct-mutex-read pattern every
/// other subsystem uses — no pub/sub "flight changed" mechanism exists
/// or is warranted here). All fields `None` when no flight is active;
/// the frontend disables the form in that case.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FlightContext {
    pub callsign: Option<String>,
    pub aircraft_type: Option<String>,
    pub dep_icao: Option<String>,
    pub dest_icao: Option<String>,
}

fn flight_context(app: &AppHandle) -> FlightContext {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let guard = state.active_flight.lock().expect("active flight mutex");
    match guard.as_ref() {
        Some(flight) => FlightContext {
            callsign: Some(format!("{}{}", flight.airline_icao, flight.flight_number)),
            aircraft_type: Some(flight.aircraft_icao.clone()),
            dep_icao: Some(flight.dpt_airport.clone()),
            dest_icao: Some(flight.arr_airport.clone()),
        },
        None => FlightContext::default(),
    }
}

#[tauri::command]
pub fn hoppie_get_flight_context(app: AppHandle) -> FlightContext {
    flight_context(&app)
}

/// The callsign every request goes out under: an explicit override
/// wins, otherwise the active flight's. Shared by connect and the
/// settings panel's verify button so they can never disagree — the
/// button used to receive the callsign FROM the UI, which passed an
/// empty string whenever no override was set and made "Test code" fail
/// with "no callsign" while a perfectly good one sat in the flight plan.
fn resolve_callsign(app: &AppHandle) -> Option<String> {
    settings::read_settings(app)
        .callsign_override
        .filter(|c| !c.trim().is_empty())
        .map(|c| c.trim().to_uppercase())
        .or_else(|| {
            flight_context(app)
                .callsign
                .map(|c| c.trim().to_uppercase())
                .filter(|c| !c.is_empty())
        })
}

/// Load the stored logon code.
fn resolve_logon_code() -> Result<String, UiError> {
    match secrets::load_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))?
    {
        Some(code) => Ok(code),
        None => Err(UiError::new(
            "hoppie_no_logon_code",
            "Kein Hoppie-Logon-Code hinterlegt.",
        )),
    }
}

/// Resolve + send a downlink CPDLC element: allocate a MIN via the
/// thread state machine, encode it to the wire format, send it, stamp
/// its timestamp. Shared by every CPDLC-send command below so the
/// MIN-allocation / wire-send / timestamp sequence lives in exactly
/// one place.
/// `app` is only here so the sent message reaches the flight log — every
/// downlink funnels through this one function, so recording it here can't
/// be forgotten when a new sender is added.
async fn send_cpdlc_element(
    app: &AppHandle,
    handle: &HoppieHandle,
    logon: String,
    spec: &'static hoppie_protocol::elements::ElementSpec,
    values: Vec<String>,
    mrn: Option<u32>,
) -> Result<u32, UiError> {
    let resolved = hoppie_protocol::elements::resolve(spec, &values)
        .map_err(|e| UiError::new("hoppie_element_resolve", e.to_string()))?;
    let filled_text = resolved.filled_text.clone();
    let (message, min) = {
        let mut t = handle.thread.lock().expect("hoppie thread mutex");
        // v0.19.x FIX: a handover supersedes any uplink the pilot hadn't
        // answered yet (see `CpdlcThread::mark_logged_off`). Without this
        // check a late WILCO/UNABLE would still go out — addressed to
        // `to_station`, which by send time is already the NEW centre —
        // silently misdirecting a reply the old controller will never
        // see and the new one can't make sense of (its MRN references a
        // MIN from a numbering space that isn't theirs). Block it here,
        // in the one function every downlink funnels through, rather
        // than relying solely on the UI disabling the button.
        if let Some(m) = mrn {
            if t.is_superseded_uplink(m) {
                return Err(UiError::new(
                    "hoppie_superseded_uplink",
                    "Diese Anweisung ist nicht mehr gültig — die Stelle hat vor deiner Antwort übergeben.",
                ));
            }
        }
        let (message, _event) = t.record_sent(
            spec.response,
            mrn,
            filled_text,
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        let min = message.min;
        (message, min)
    };
    handle
        .min_timestamps
        .lock()
        .expect("hoppie min_timestamps mutex")
        .insert((false, min), chrono::Utc::now());

    let packet = hoppie_protocol::cpdlc::encode(&message);
    let wire_req = hoppie_protocol::wire::HoppieRequest {
        logon,
        from: handle.from_callsign.clone(),
        to: handle
            .to_station
            .lock()
            .expect("hoppie to_station mutex")
            .clone(),
        kind: hoppie_protocol::wire::PacketKind::Cpdlc,
        packet: Some(packet),
    };
    if let hoppie_protocol::wire::HoppieResponseLine::Error(reason) =
        handle.http.send(&wire_req).await?
    {
        return Err(UiError::new("hoppie_cpdlc_rejected", reason));
    }
    // Only after the network accepted it — a rejected send never reached
    // ATC and would read as a message the controller ignored.
    crate::record_datalink(
        app,
        "downlink",
        "cpdlc",
        Some(wire_req.to.clone()),
        Some(min),
        mrn,
        Some(spec.response.code().to_string()),
        message.element_text.clone(),
    );
    Ok(min)
}

/// Send the (Hoppie-specific, no GOLD equivalent — see the module's
/// logon-code-validation docs) `REQUEST LOGON` downlink that starts the
/// CPDLC handshake. [`HoppieStatus::logged_on`] flips once the uplink
/// `LOGON ACCEPTED`/`UNABLE` reply arrives (next poll).
///
/// A CPDLC logon always names the ATC facility it targets, so `station`
/// re-points the connection before the request goes out and every later
/// message follows it — that's how a pilot hands over from one centre
/// to the next without reconnecting. It's also persisted, so the next
/// session starts on the same facility.
#[tauri::command]
pub async fn hoppie_send_logon_request(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    station: Option<String>,
) -> Result<HoppieStatus, UiError> {
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
        )
    })?;

    if let Some(raw) = station {
        let trimmed = raw.trim().to_uppercase();
        if trimmed.is_empty() {
            return Err(UiError::new(
                "hoppie_no_station",
                "Keine ATC-Station angegeben — z. B. EDDF oder EDGG.",
            ));
        }
        // Manually switching facilities is a handover the network didn't
        // announce. Log off the old one FIRST, otherwise it keeps the
        // aircraft on its list and keeps queueing messages for us while
        // the new centre also thinks it's responsible. (An automatic
        // HANDOVER is different — there the old centre initiated it and
        // has already let go; see poller.rs.)
        let previous = handle
            .to_station
            .lock()
            .expect("hoppie to_station mutex")
            .clone();
        if previous != trimmed {
            logoff_and_forget(&app, handle).await;
        }
        *handle.to_station.lock().expect("hoppie to_station mutex") = trimmed.clone();
        let mut settings = settings::read_settings(&app);
        settings.station_id = trimmed;
        settings::write_settings(&app, &settings);
    }

    let logon = resolve_logon_code()?;
    let spec = hoppie_protocol::elements::find("DM_REQUEST_LOGON").expect("built-in element");
    send_cpdlc_element(&app, handle, logon, spec, Vec::new(), None).await?;
    Ok(build_status(&guard))
}

/// Tokens that make a controller's client read an inbound telex as a
/// NEW clearance request instead of what it is. vSMR tests this branch
/// before the acknowledgement branch (SMRPlugin.cpp:162 vs :176), so a
/// reply containing any of them re-flashes the controller's request
/// queue. Enforced here, in the command, rather than in one UI
/// component — every send path has to inherit it.
const REQUEST_TOKENS: &[&str] = &["CLR", "REQ", "PDC", "PREDEP"];

/// Hoppie's encoding rules say uppercase only, and vSMR matches
/// acknowledgements case-SENSITIVELY (`std::string::find("WILCO")`), so a
/// lowercase reply is simply invisible to the controller.
fn normalize_outbound(text: &str) -> Result<String, UiError> {
    let upper = text.trim().to_uppercase();
    if upper.is_empty() {
        return Err(UiError::new("hoppie_empty_text", "Nachricht ist leer."));
    }
    Ok(upper)
}

/// Reject an acknowledgement that would be misread as a fresh request.
fn reject_request_tokens(text: &str) -> Result<(), UiError> {
    let hits: Vec<&str> = REQUEST_TOKENS
        .iter()
        .copied()
        .filter(|tok| text.contains(tok))
        .collect();
    if hits.is_empty() {
        return Ok(());
    }
    Err(UiError::new(
        "hoppie_request_token",
        format!(
            "Enthält {} — ATC würde das als neue Freigabeanfrage lesen.",
            hits.join(", ")
        ),
    ))
}

/// End the CPDLC session with the current facility on the pilot's
/// command, without dropping the ACARS link. This is the counterpart to
/// [`hoppie_send_logon_request`]: after logging off, no facility is
/// responsible for the aircraft until the pilot logs on to the next one.
#[tauri::command]
pub async fn hoppie_send_logoff(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<HoppieStatus, UiError> {
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
        )
    })?;
    // Also allowed while a logon is merely OUTSTANDING: a station that
    // never answers would otherwise trap the pilot — the button was
    // gated on `logged_on`, so the only way out was dropping the whole
    // ACARS link.
    {
        let t = handle.thread.lock().expect("hoppie thread mutex");
        if !t.is_logged_on() && t.pending_logon_min().is_none() {
            return Err(UiError::new(
                "hoppie_not_logged_on",
                "Bei keiner Station angemeldet.",
            ));
        }
    }
    let logon = resolve_logon_code()?;
    let spec = hoppie_protocol::elements::find("DM_LOGOFF").expect("built-in element");
    send_cpdlc_element(&app, handle, logon, spec, Vec::new(), None).await?;
    Ok(build_status(&guard))
}

/// Send a plain telex — the acknowledgment path for traffic that has no
/// MIN/MRN threading, i.e. PDC replies. CPDLC's structured WILCO/ROGER
/// elements don't apply to telex, so a readback goes back the same way
/// it arrived.
#[tauri::command]
pub async fn hoppie_send_telex(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    text: String,
    recipient: Option<String>,
) -> Result<(), UiError> {
    let trimmed = normalize_outbound(&text)?;
    reject_request_tokens(&trimmed)?;
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
        )
    })?;
    let logon = resolve_logon_code()?;
    let to = match recipient {
        Some(r) if !r.trim().is_empty() => r.trim().to_uppercase(),
        _ => handle
            .to_station
            .lock()
            .expect("hoppie to_station mutex")
            .clone(),
    };
    let wire_req = hoppie_protocol::wire::HoppieRequest {
        logon,
        from: handle.from_callsign.clone(),
        to,
        kind: hoppie_protocol::wire::PacketKind::Telex,
        packet: Some(trimmed.to_string()),
    };
    if let hoppie_protocol::wire::HoppieResponseLine::Error(reason) =
        handle.http.send(&wire_req).await?
    {
        return Err(UiError::new("hoppie_telex_rejected", reason));
    }
    crate::record_datalink(
        &app,
        "downlink",
        "telex",
        Some(wire_req.to.clone()),
        None,
        None,
        None,
        trimmed.to_string(),
    );
    handle
        .telex_log
        .lock()
        .expect("hoppie telex_log mutex")
        .push(TelexEntry {
            direction: "sent",
            text: trimmed.to_string(),
            at: chrono::Utc::now(),
            from_cpdlc_channel: false,
        });
    Ok(())
}

/// Send arbitrary free text as a CPDLC downlink (GOLD element `DM67`,
/// "\[freetext\]", response `N`) — the escape hatch for anything the
/// structured composer doesn't cover, or a quick reply that doesn't
/// warrant picking a specific element.
#[tauri::command]
pub async fn hoppie_send_free_text(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    text: String,
    mrn: Option<u32>,
) -> Result<u32, UiError> {
    let trimmed = normalize_outbound(&text)?;
    // Guard only when this free text ANSWERS an uplink: an unsolicited
    // free-text message may legitimately say "REQUEST VECTORS", but a
    // reply containing a request token is read by the controller's
    // client as a brand-new clearance request.
    if mrn.is_some() {
        reject_request_tokens(&trimmed)?;
    }
    // A sanity bound, not a transport limit — we POST, so the old URL
    // ceiling no longer applies. EasyCPDLC caps its multiline box at
    // 255; matching that keeps us within what controller clients and
    // their displays actually handle.
    const MAX_FREE_TEXT: usize = 255;
    if trimmed.len() > MAX_FREE_TEXT {
        return Err(UiError::new(
            "hoppie_text_too_long",
            format!(
                "Nachricht ist {} Zeichen lang — höchstens {MAX_FREE_TEXT} sind zulässig.",
                trimmed.len()
            ),
        ));
    }
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
        )
    })?;
    let logon = resolve_logon_code()?;
    let spec = hoppie_protocol::elements::find("DM67").expect("GOLD free-text element");
    send_cpdlc_element(&app, handle, logon, spec, vec![trimmed], mrn).await
}

/// Send a structured downlink element by GOLD id (e.g. `"UM74"`
/// wouldn't apply here since only downlink `DM*`/Hoppie-specific ids
/// are sendable by a pilot — an uplink id is rejected). `values` fill
/// the element's placeholders in order; `mrn` threads a reply to a
/// specific received uplink (e.g. the WILCO/UNABLE response buttons).
#[tauri::command]
pub async fn hoppie_send_cpdlc_element(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    element_id: String,
    values: Vec<String>,
    mrn: Option<u32>,
) -> Result<u32, UiError> {
    let spec = hoppie_protocol::elements::find(&element_id).ok_or_else(|| {
        UiError::new(
            "hoppie_unknown_element",
            format!("Unbekanntes CPDLC-Element: {element_id}"),
        )
    })?;
    if spec.direction != hoppie_protocol::elements::Direction::Downlink {
        return Err(UiError::new(
            "hoppie_not_downlink_element",
            format!("{element_id} ist kein Downlink-Element (kann nicht gesendet werden)."),
        ));
    }
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
        )
    })?;
    let logon = resolve_logon_code()?;
    // Normalize in the COMMAND, not just the composer: any other caller
    // (a future quick action, the LAN bridge) would otherwise be able to
    // put lowercase on the wire, where the controller can't match it.
    // Only normalization here — NOT the request-token guard. These are
    // placeholder values, and the elements themselves are legitimately
    // named "REQUEST DIRECT TO ..."; refusing those would break the
    // composer's whole purpose. The guard exists for ACKNOWLEDGEMENTS,
    // which must not read as a fresh request.
    let values = values
        .iter()
        .map(|v| normalize_outbound(v))
        .collect::<Result<Vec<_>, _>>()?;
    send_cpdlc_element(&app, handle, logon, spec, values, mrn).await
}

/// One row of the GOLD downlink catalog, for the composer's element
/// picker. Uplink elements are never listed — a pilot only ever
/// *sends* downlink elements.
#[derive(Debug, Clone, Serialize)]
pub struct ElementSpecDto {
    pub id: String,
    pub template: String,
    pub placeholders: Vec<String>,
    pub response: String,
}

#[tauri::command]
pub fn hoppie_list_elements() -> Vec<ElementSpecDto> {
    hoppie_protocol::elements::dm_table()
        .map(|s| ElementSpecDto {
            id: s.id.to_string(),
            template: s.template.to_string(),
            placeholders: s.placeholders.iter().map(|p| format!("{p:?}")).collect(),
            response: s.response.code().to_string(),
        })
        .collect()
}

/// PDC request form fields, per the EasyCPDLC-verified format (see
/// `hoppie-protocol::pdc`'s docs).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PdcRequestArgs {
    pub recipient: String,
    pub aircraft_type: String,
    pub dep_icao: String,
    pub dest_icao: String,
    pub stand: String,
    pub atis_letter: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PdcSendResult {
    pub sent_text: String,
    pub sent_at: String,
}

/// Send a PDC request as a plain Hoppie `telex` (no dedicated PDC wire
/// type exists — see `hoppie-protocol::pdc`'s docs). Requires an active
/// connection ([`hoppie_connect`] already verified the logon code and
/// resolved a callsign, both reused here).
#[tauri::command]
pub async fn hoppie_send_pdc_request(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    request: PdcRequestArgs,
) -> Result<PdcSendResult, UiError> {
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst in den Einstellungen verbinden.",
        )
    })?;

    let logon = match secrets::load_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))?
    {
        Some(code) => code,
        None => {
            return Err(UiError::new(
                "hoppie_no_logon_code",
                "Kein Hoppie-Logon-Code hinterlegt.",
            ))
        }
    };

    // The callsign is NOT a form field. It comes from the one place it
    // can come from: the ACARS callsign this connection was opened with.
    // A second, separately-typed callsign meant PDC went out under a
    // different identity than CPDLC, and a controller looking the
    // aircraft up by the other one simply never found it.
    let pdc_request = hoppie_protocol::pdc::PdcRequest {
        recipient: request.recipient.trim().to_uppercase(),
        callsign: handle.from_callsign.clone(),
        aircraft_type: request.aircraft_type.trim().to_uppercase(),
        dep_icao: request.dep_icao.trim().to_uppercase(),
        dest_icao: request.dest_icao.trim().to_uppercase(),
        stand: request.stand.trim().to_uppercase(),
        atis_letter: request.atis_letter.trim().to_uppercase(),
    };
    let text = hoppie_protocol::pdc::format_pdc_request(&pdc_request);

    let wire_req = hoppie_protocol::wire::HoppieRequest {
        logon,
        from: handle.from_callsign.clone(),
        to: pdc_request.recipient.clone(),
        kind: hoppie_protocol::wire::PacketKind::Telex,
        packet: Some(text.clone()),
    };
    if let hoppie_protocol::wire::HoppieResponseLine::Error(reason) =
        handle.http.send(&wire_req).await?
    {
        return Err(UiError::new("hoppie_pdc_rejected", reason));
    }

    crate::record_datalink(
        &app,
        "downlink",
        "pdc",
        Some(pdc_request.recipient.clone()),
        None,
        None,
        None,
        text.clone(),
    );

    let now = chrono::Utc::now();
    handle
        .telex_log
        .lock()
        .expect("hoppie telex_log mutex")
        .push(TelexEntry {
            direction: "sent",
            text: text.clone(),
            at: now,
            from_cpdlc_channel: false,
        });

    Ok(PdcSendResult {
        sent_text: text,
        sent_at: now.to_rfc3339(),
    })
}

/// One entry in the message history the CPDLC tab renders — a merge of
/// plain telex/PDC traffic (`kind: "telex"`) and MIN/MRN-threaded CPDLC
/// messages (`kind: "cpdlc"`), sorted chronologically. The `min`/`mrn`/
/// `response`/`element_id`/`closed`/`superseded` fields are only ever
/// populated for `"cpdlc"` entries — the frontend uses `response` to
/// decide which (if any) reply buttons to show, `closed` to grey out an
/// entry that already got one, and `superseded` to grey out (and refuse
/// reply buttons for) an uplink orphaned by a handover before the pilot
/// answered it — see `CpdlcThread::mark_logged_off`.
#[derive(Debug, Clone, Serialize)]
pub struct ThreadEntryDto {
    pub kind: &'static str,
    pub direction: &'static str,
    pub text: String,
    pub at: String,
    pub min: Option<u32>,
    pub mrn: Option<u32>,
    pub response: Option<String>,
    pub element_id: Option<String>,
    pub closed: Option<bool>,
    /// Already deferred with STANDBY — the UI hides the STANDBY key so
    /// the same instruction can't be pushed back repeatedly.
    pub deferred: Option<bool>,
    /// This uplink was still open when a handover happened — the
    /// controller who sent it is no longer talking to the aircraft.
    /// The UI must grey it out and hide/disable its reply buttons; the
    /// backend also refuses to send a reply for it (see
    /// `send_cpdlc_element`) as defense-in-depth.
    pub superseded: Option<bool>,
}

#[tauri::command]
pub async fn hoppie_get_thread(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ThreadEntryDto>, UiError> {
    let guard = state.hoppie.lock().await;
    let Some(handle) = guard.as_ref() else {
        return Ok(Vec::new());
    };

    let mut entries = Vec::new();
    {
        let log = handle.telex_log.lock().expect("hoppie telex_log mutex");
        entries.extend(log.iter().map(|e| ThreadEntryDto {
            kind: if e.from_cpdlc_channel {
                "cpdlc"
            } else {
                "telex"
            },
            direction: e.direction,
            text: e.text.clone(),
            at: e.at.to_rfc3339(),
            min: None,
            mrn: None,
            response: None,
            element_id: None,
            closed: None,
            deferred: None,
            superseded: None,
        }));
    }
    {
        let thread = handle.thread.lock().expect("hoppie thread mutex");
        let timestamps = handle
            .min_timestamps
            .lock()
            .expect("hoppie min_timestamps mutex");
        entries.extend(thread.history().iter().map(|e| {
            let (element_id, text) = match &e.message.parsed {
                hoppie_protocol::elements::ParsedElement::Recognized(r) => {
                    (Some(r.spec_id.to_string()), e.message.element_text.clone())
                }
                hoppie_protocol::elements::ParsedElement::Raw(t) => (None, t.clone()),
            };
            let at = timestamps
                .get(&(
                    e.direction == hoppie_protocol::elements::Direction::Uplink,
                    e.min,
                ))
                .copied()
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339();
            ThreadEntryDto {
                kind: "cpdlc",
                direction: match e.direction {
                    hoppie_protocol::elements::Direction::Uplink => "received",
                    hoppie_protocol::elements::Direction::Downlink => "sent",
                },
                text,
                at,
                min: Some(e.min),
                mrn: e.mrn,
                response: Some(e.message.response.code().to_string()),
                element_id,
                closed: Some(e.closed),
                deferred: Some(e.deferred),
                superseded: Some(e.superseded),
            }
        }));
    }
    entries.sort_by(|a, b| a.at.cmp(&b.at));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_status_when_disconnected_is_all_falsy() {
        let status = build_status(&None);
        assert!(!status.connected);
        assert!(!status.logged_on);
        assert_eq!(status.pending_response_count, 0);
        assert!(status.last_error.is_none());
        assert!(status.logon_verified.is_none());
    }
}
