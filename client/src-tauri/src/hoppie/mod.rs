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

use crate::{AppState, UiError};

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
///
/// In [`HoppieSettings::mock_mode`], [`send`](Self::send) never touches
/// `http` at all — see [`mock_send`](Self::mock_send).
pub struct HoppieHttp {
    http: reqwest::Client,
    mock: bool,
    /// Canned reply envelopes waiting to be "delivered" on the next
    /// mocked `poll` — only ever touched when `mock` is true.
    mock_inbox: StdMutex<std::collections::VecDeque<hoppie_protocol::wire::InboundEnvelope>>,
    /// MIN counter for synthesized UPLINK messages (e.g. `LOGON
    /// ACCEPTED`) — a distinct, high range so it can never collide
    /// with our own downlink MIN sequence (which starts at 1).
    mock_uplink_min: StdMutex<u32>,
}

impl HoppieHttp {
    fn new(mock: bool) -> Result<Self, UiError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
            // 15s connection timeout per the official docs'
            // recommendation (this is the CONNECT timeout, not the
            // polling rate — see poller.rs for that).
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| UiError::new("hoppie_http_init", e.to_string()))?;
        Ok(Self {
            http,
            mock,
            mock_inbox: StdMutex::new(std::collections::VecDeque::new()),
            mock_uplink_min: StdMutex::new(9000),
        })
    }

    async fn send(
        &self,
        req: &hoppie_protocol::wire::HoppieRequest,
    ) -> Result<hoppie_protocol::wire::HoppieResponseLine, UiError> {
        if self.mock {
            return Ok(self.mock_send(req));
        }
        let pairs = hoppie_protocol::wire::query_pairs(req);
        let resp = self
            .http
            .get(BASE_URL)
            .query(&pairs)
            .send()
            .await
            .map_err(|e| UiError::new("hoppie_network", e.to_string()))?;
        let body = resp
            .text()
            .await
            .map_err(|e| UiError::new("hoppie_network", e.to_string()))?;
        hoppie_protocol::wire::parse_response(&body)
            .map_err(|e| UiError::new("hoppie_protocol", e.to_string()))
    }

    /// Fabricate a response without any network access. `ping` always
    /// succeeds (any code "works" in simulation — there is nothing to
    /// validate against), a PDC-request `telex` gets queued a synthetic
    /// clearance reply for the next `poll`, and every other `telex`
    /// is just accepted with no reply. Every synthesized reply is
    /// tagged `[SIMULATION]` so it can never be mistaken for a real
    /// clearance.
    fn mock_send(&self, req: &hoppie_protocol::wire::HoppieRequest) -> hoppie_protocol::wire::HoppieResponseLine {
        use hoppie_protocol::wire::{HoppieResponseLine, InboundEnvelope, PacketKind};
        match req.kind {
            PacketKind::Ping => HoppieResponseLine::Ok,
            PacketKind::Telex => {
                if let Some(packet) = &req.packet {
                    if let Some(reply) = mock_pdc_reply(packet) {
                        self.mock_inbox
                            .lock()
                            .expect("hoppie mock_inbox mutex")
                            .push_back(InboundEnvelope {
                                from: req.to.clone(),
                                kind: PacketKind::Telex,
                                packet: reply,
                            });
                    }
                }
                HoppieResponseLine::Ok
            }
            PacketKind::Poll => {
                let mut inbox = self.mock_inbox.lock().expect("hoppie mock_inbox mutex");
                if inbox.is_empty() {
                    return HoppieResponseLine::Ok;
                }
                let body: String = inbox
                    .drain(..)
                    .map(|e| format!("{{{} {} {{{}}}}}", e.from, e.kind.as_wire_str(), e.packet))
                    .collect();
                HoppieResponseLine::OkWithPayload(body)
            }
            PacketKind::Peek => HoppieResponseLine::Ok,
            PacketKind::Cpdlc => {
                if let Some(packet) = &req.packet {
                    if let Some(reply_packet) = self.mock_cpdlc_reply(packet) {
                        self.mock_inbox
                            .lock()
                            .expect("hoppie mock_inbox mutex")
                            .push_back(InboundEnvelope {
                                from: req.to.clone(),
                                kind: PacketKind::Cpdlc,
                                packet: reply_packet,
                            });
                    }
                }
                HoppieResponseLine::Ok
            }
        }
    }

    /// If `packet` is our own encoded `REQUEST LOGON` downlink, build
    /// an encoded `LOGON ACCEPTED` uplink reply (MRN threaded back to
    /// it) so simulation mode exercises the full logon handshake, not
    /// just PDC. Every other CPDLC send gets no synthetic reply (real
    /// ATC-instruction simulation is future work — see the module's
    /// Phase 3 notes).
    fn mock_cpdlc_reply(&self, packet: &str) -> Option<String> {
        let msg = hoppie_protocol::cpdlc::decode(
            packet,
            hoppie_protocol::elements::Direction::Downlink,
        )
        .ok()?;
        let hoppie_protocol::elements::ParsedElement::Recognized(r) = &msg.parsed else {
            return None;
        };
        if r.spec_id != "DM_REQUEST_LOGON" {
            return None;
        }
        let mut next_min = self.mock_uplink_min.lock().expect("hoppie mock_uplink_min mutex");
        let min = *next_min;
        *next_min += 1;
        // Element text must stay byte-exact "LOGON ACCEPTED" — the
        // receiving side's `thread::logon_outcome()` matches it
        // structurally (via `elements::match_uplink_text`) to flip
        // `logged_on`, so an appended "[SIMULATION]" tag would silently
        // break that (Raw fallback, never recognized). The persistent
        // mock-mode badge already shown in the connection header is
        // this app's simulation indicator; individual CPDLC element
        // text can't also carry one without breaking the state
        // machine, unlike free-form PDC telex replies.
        let reply = hoppie_protocol::cpdlc::CpdlcMessage {
            min,
            mrn: Some(msg.min),
            response: hoppie_protocol::cpdlc::ResponseRequirement::NoResponseExpected,
            element_text: "LOGON ACCEPTED".to_string(),
            parsed: hoppie_protocol::elements::ParsedElement::Raw(String::new()),
        };
        Some(hoppie_protocol::cpdlc::encode(&reply))
    }
}

/// Build a canned `[SIMULATION]`-tagged PDC reply if `packet` looks
/// like a PDC request (per the format `hoppie_send_pdc_request`
/// sends), else `None` (nothing to synthesize for arbitrary telex).
fn mock_pdc_reply(packet: &str) -> Option<String> {
    // "REQUEST PREDEP CLEARANCE {CALLSIGN} {TYPE} TO {DEST} AT {DEP} STAND {STAND} ATIS {ATIS}"
    let rest = packet.strip_prefix("REQUEST PREDEP CLEARANCE ")?;
    let callsign = rest.split_whitespace().next().unwrap_or("UNKNOWN");
    let dest = rest
        .split(" TO ")
        .nth(1)
        .and_then(|s| s.split(" AT ").next())
        .unwrap_or("DEST")
        .trim();
    Some(format!(
        "{callsign} CLRD TO {dest} VIA DCT SQUAWK 2200 INITIAL CLB FL050 CTC DEL 121.9 [SIMULATION]"
    ))
}

/// Result of testing a logon code via [`verify_logon`]. `reason` — when
/// present — is the Hoppie server's own raw error text, never a guessed
/// message (the exact wording for e.g. an invalid code isn't
/// documented).
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

/// One sent or received telex/PDC-request-reply line. CPDLC messages
/// (MIN/MRN-threaded) live in `HoppieHandle::thread` instead — this is
/// only for the un-threaded plain-telex traffic PDC uses, which has no
/// GOLD element table entry of its own (see `hoppie-protocol::pdc`'s
/// docs).
pub(crate) struct TelexEntry {
    direction: &'static str,
    text: String,
    at: chrono::DateTime<chrono::Utc>,
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
    /// MIN -> when we sent/received it. The pure `CpdlcThread` is
    /// deliberately wall-clock-free (keeps it a pure, fast-testable
    /// state machine); this wiring-layer map is the only place a
    /// CPDLC message's timestamp lives, populated by every send
    /// command and by the poller on receipt.
    min_timestamps: Arc<StdMutex<std::collections::HashMap<u32, chrono::DateTime<chrono::Utc>>>>,
    last_error: Arc<StdMutex<Option<String>>>,
    last_verify: Option<VerifyOutcome>,
    /// Resolved at connect time — reused by every send command so they
    /// don't need to re-resolve settings/active-flight state.
    from_callsign: String,
    to_station: String,
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
    pub last_error: Option<String>,
    pub logon_verified: Option<VerifyOutcome>,
    /// Whether the current connection is simulated (no real network
    /// access) — the frontend shows a clear badge whenever this is
    /// true so simulated traffic can never be mistaken for real.
    pub mock_mode: bool,
}

fn build_status(handle: &Option<HoppieHandle>) -> HoppieStatus {
    match handle {
        Some(h) => {
            let thread = h.thread.lock().expect("hoppie thread mutex");
            HoppieStatus {
                connected: true,
                logged_on: thread.is_logged_on(),
                pending_response_count: thread.pending_response_count(),
                last_error: h
                    .last_error
                    .lock()
                    .expect("hoppie last_error mutex")
                    .clone(),
                logon_verified: h.last_verify.clone(),
                mock_mode: h.http.mock,
            }
        }
        None => HoppieStatus {
            connected: false,
            logged_on: false,
            pending_response_count: 0,
            last_error: None,
            logon_verified: None,
            mock_mode: false,
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

/// Test a logon code against the real Hoppie network via a single
/// `ping` (see the module docs) — side-effect-free, safe to call
/// repeatedly. Does NOT read/write the stored code; the caller decides
/// whether to persist it (typically only after a successful verify).
#[tauri::command]
pub async fn hoppie_verify_logon_code(
    app: AppHandle,
    code: String,
    callsign: String,
) -> Result<VerifyOutcome, UiError> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Ok(VerifyOutcome {
            valid: false,
            reason: Some("Logon-Code ist leer.".to_string()),
        });
    }
    let mock = settings::read_settings(&app).mock_mode;
    let http = HoppieHttp::new(mock)?;
    let from = if callsign.trim().is_empty() {
        "TEST".to_string()
    } else {
        callsign.trim().to_uppercase()
    };
    Ok(verify_logon(&http, trimmed, &from).await)
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
    // Simulation mode needs neither a real logon code nor a real
    // callsign — the whole point is testing the UI without touching
    // the live network or an active flight. Both still fall back to
    // whatever IS configured, so a half-configured settings panel
    // (e.g. a real code already saved) is still honored.
    let logon = match secrets::load_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))?
    {
        Some(code) => code,
        None if settings.mock_mode => "MOCK".to_string(),
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
    let from = match settings
        .callsign_override
        .clone()
        .filter(|c| !c.trim().is_empty())
        .map(|c| c.trim().to_uppercase())
        .or_else(|| flight_context(&app).callsign)
    {
        Some(cs) => cs,
        None if settings.mock_mode => "MOCKPILOT".to_string(),
        None => {
            return Err(UiError::new(
                "hoppie_no_callsign",
                "Kein Callsign hinterlegt und kein aktiver Flug — bitte in den Hoppie-Einstellungen setzen.",
            ))
        }
    };

    let http = Arc::new(HoppieHttp::new(settings.mock_mode)?);
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

    let thread = Arc::new(StdMutex::new(hoppie_protocol::thread::CpdlcThread::new()));
    let telex_log = Arc::new(StdMutex::new(Vec::new()));
    let min_timestamps = Arc::new(StdMutex::new(std::collections::HashMap::new()));
    let last_error = Arc::new(StdMutex::new(None));
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
        settings.station_id.clone(),
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
        from_callsign: from,
        to_station: settings.station_id.clone(),
    });
    Ok(build_status(&guard))
}

/// Stop the poller (no-op if not running). Dropping the handle fires
/// the stop signal.
#[tauri::command]
pub async fn hoppie_disconnect(state: tauri::State<'_, AppState>) -> Result<HoppieStatus, UiError> {
    let mut guard = state.hoppie.lock().await;
    *guard = None;
    Ok(build_status(&guard))
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

/// Load the stored logon code, falling back to a placeholder in
/// simulation mode (see [`hoppie_connect`]'s docs for why that's safe).
fn resolve_logon_code(mock: bool) -> Result<String, UiError> {
    match secrets::load_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))?
    {
        Some(code) => Ok(code),
        None if mock => Ok("MOCK".to_string()),
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
async fn send_cpdlc_element(
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
        .insert(min, chrono::Utc::now());

    let packet = hoppie_protocol::cpdlc::encode(&message);
    let wire_req = hoppie_protocol::wire::HoppieRequest {
        logon,
        from: handle.from_callsign.clone(),
        to: handle.to_station.clone(),
        kind: hoppie_protocol::wire::PacketKind::Cpdlc,
        packet: Some(packet),
    };
    if let hoppie_protocol::wire::HoppieResponseLine::Error(reason) = handle.http.send(&wire_req).await? {
        return Err(UiError::new("hoppie_cpdlc_rejected", reason));
    }
    Ok(min)
}

/// Send the (Hoppie-specific, no GOLD equivalent — see the module's
/// logon-code-validation docs) `REQUEST LOGON` downlink that starts the
/// CPDLC handshake. [`HoppieStatus::logged_on`] flips once the uplink
/// `LOGON ACCEPTED`/`UNABLE` reply arrives (next poll).
#[tauri::command]
pub async fn hoppie_send_logon_request(
    state: tauri::State<'_, AppState>,
) -> Result<HoppieStatus, UiError> {
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
        )
    })?;
    let logon = resolve_logon_code(handle.http.mock)?;
    let spec = hoppie_protocol::elements::find("DM_REQUEST_LOGON").expect("built-in element");
    send_cpdlc_element(handle, logon, spec, Vec::new(), None).await?;
    Ok(build_status(&guard))
}

/// Send arbitrary free text as a CPDLC downlink (GOLD element `DM67`,
/// "\[freetext\]", response `N`) — the escape hatch for anything the
/// structured composer doesn't cover, or a quick reply that doesn't
/// warrant picking a specific element.
#[tauri::command]
pub async fn hoppie_send_free_text(
    state: tauri::State<'_, AppState>,
    text: String,
    mrn: Option<u32>,
) -> Result<u32, UiError> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(UiError::new("hoppie_empty_text", "Nachricht ist leer."));
    }
    let guard = state.hoppie.lock().await;
    let handle = guard.as_ref().ok_or_else(|| {
        UiError::new(
            "hoppie_not_connected",
            "Nicht mit Hoppie ACARS verbunden — zuerst verbinden.",
        )
    })?;
    let logon = resolve_logon_code(handle.http.mock)?;
    let spec = hoppie_protocol::elements::find("DM67").expect("GOLD free-text element");
    send_cpdlc_element(handle, logon, spec, vec![trimmed.to_string()], mrn).await
}

/// Send a structured downlink element by GOLD id (e.g. `"UM74"`
/// wouldn't apply here since only downlink `DM*`/Hoppie-specific ids
/// are sendable by a pilot — an uplink id is rejected). `values` fill
/// the element's placeholders in order; `mrn` threads a reply to a
/// specific received uplink (e.g. the WILCO/UNABLE response buttons).
#[tauri::command]
pub async fn hoppie_send_cpdlc_element(
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
    let logon = resolve_logon_code(handle.http.mock)?;
    send_cpdlc_element(handle, logon, spec, values, mrn).await
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
    pub callsign: String,
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
        None if handle.http.mock => "MOCK".to_string(),
        None => {
            return Err(UiError::new(
                "hoppie_no_logon_code",
                "Kein Hoppie-Logon-Code hinterlegt.",
            ))
        }
    };

    let pdc_request = hoppie_protocol::pdc::PdcRequest {
        recipient: request.recipient.trim().to_uppercase(),
        callsign: request.callsign.trim().to_uppercase(),
        aircraft_type: request.aircraft_type.trim().to_uppercase(),
        dep_icao: request.dep_icao.trim().to_uppercase(),
        dest_icao: request.dest_icao.trim().to_uppercase(),
        stand: request.stand.trim().to_uppercase(),
        atis_letter: request.atis_letter.trim().to_uppercase(),
    };
    let text = hoppie_protocol::pdc::format_pdc_request(&pdc_request);

    let wire_req = hoppie_protocol::wire::HoppieRequest {
        logon,
        from: pdc_request.callsign.clone(),
        to: pdc_request.recipient.clone(),
        kind: hoppie_protocol::wire::PacketKind::Telex,
        packet: Some(text.clone()),
    };
    if let hoppie_protocol::wire::HoppieResponseLine::Error(reason) =
        handle.http.send(&wire_req).await?
    {
        return Err(UiError::new("hoppie_pdc_rejected", reason));
    }

    let now = chrono::Utc::now();
    handle
        .telex_log
        .lock()
        .expect("hoppie telex_log mutex")
        .push(TelexEntry {
            direction: "sent",
            text: text.clone(),
            at: now,
        });

    Ok(PdcSendResult {
        sent_text: text,
        sent_at: now.to_rfc3339(),
    })
}

/// One entry in the message history the CPDLC tab renders — a merge of
/// plain telex/PDC traffic (`kind: "telex"`) and MIN/MRN-threaded CPDLC
/// messages (`kind: "cpdlc"`), sorted chronologically. The `min`/`mrn`/
/// `response`/`element_id`/`closed` fields are only ever populated for
/// `"cpdlc"` entries — the frontend uses `response` to decide which
/// (if any) reply buttons to show, and `closed` to grey out an entry
/// that already got one.
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
            kind: "telex",
            direction: e.direction,
            text: e.text.clone(),
            at: e.at.to_rfc3339(),
            min: None,
            mrn: None,
            response: None,
            element_id: None,
            closed: None,
        }));
    }
    {
        let thread = handle.thread.lock().expect("hoppie thread mutex");
        let timestamps = handle.min_timestamps.lock().expect("hoppie min_timestamps mutex");
        entries.extend(thread.history().iter().map(|e| {
            let (element_id, text) = match &e.message.parsed {
                hoppie_protocol::elements::ParsedElement::Recognized(r) => {
                    (Some(r.spec_id.to_string()), e.message.element_text.clone())
                }
                hoppie_protocol::elements::ParsedElement::Raw(t) => (None, t.clone()),
            };
            let at = timestamps
                .get(&e.min)
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
        assert!(!status.mock_mode);
    }

    #[test]
    fn mock_pdc_reply_extracts_callsign_and_destination() {
        let reply = mock_pdc_reply(
            "REQUEST PREDEP CLEARANCE GSG353 A320 TO EDDP AT EDDF STAND A2 ATIS A",
        )
        .expect("PDC request text must synthesize a reply");
        assert!(reply.starts_with("GSG353 CLRD TO EDDP"));
        assert!(reply.ends_with("[SIMULATION]"), "must be unmistakably tagged: {reply:?}");
    }

    #[test]
    fn mock_pdc_reply_ignores_non_pdc_telex() {
        assert!(mock_pdc_reply("just a normal free-text message").is_none());
    }

    #[tokio::test]
    async fn mock_send_ping_always_succeeds() {
        let http = HoppieHttp::new(true).unwrap();
        let req = hoppie_protocol::wire::HoppieRequest {
            logon: "anything".into(),
            from: "TEST".into(),
            to: "SERVER".into(),
            kind: hoppie_protocol::wire::PacketKind::Ping,
            packet: None,
        };
        assert_eq!(
            http.send(&req).await.unwrap(),
            hoppie_protocol::wire::HoppieResponseLine::Ok
        );
    }

    #[tokio::test]
    async fn mock_send_pdc_request_then_poll_delivers_the_canned_reply() {
        let http = HoppieHttp::new(true).unwrap();
        let telex_req = hoppie_protocol::wire::HoppieRequest {
            logon: "MOCK".into(),
            from: "GSG353".into(),
            to: "EDDF".into(),
            kind: hoppie_protocol::wire::PacketKind::Telex,
            packet: Some(
                "REQUEST PREDEP CLEARANCE GSG353 A320 TO EDDP AT EDDF STAND A2 ATIS A"
                    .to_string(),
            ),
        };
        assert_eq!(
            http.send(&telex_req).await.unwrap(),
            hoppie_protocol::wire::HoppieResponseLine::Ok
        );

        let poll_req = hoppie_protocol::wire::HoppieRequest {
            logon: "MOCK".into(),
            from: "GSG353".into(),
            to: "EDDF".into(),
            kind: hoppie_protocol::wire::PacketKind::Poll,
            packet: None,
        };
        let poll_result = http.send(&poll_req).await.unwrap();
        let hoppie_protocol::wire::HoppieResponseLine::OkWithPayload(body) = poll_result else {
            panic!("expected a queued reply, got {poll_result:?}");
        };
        let envelopes = hoppie_protocol::wire::parse_poll_envelopes(&body);
        assert_eq!(envelopes.len(), 1);
        assert!(envelopes[0].packet.contains("[SIMULATION]"));

        // The reply was drained — a second poll finds nothing new.
        let poll_again = http.send(&poll_req).await.unwrap();
        assert_eq!(poll_again, hoppie_protocol::wire::HoppieResponseLine::Ok);
    }

    #[tokio::test]
    async fn mock_cpdlc_logon_round_trip_flips_logged_on() {
        let http = HoppieHttp::new(true).unwrap();
        let mut thread = hoppie_protocol::thread::CpdlcThread::new();

        let spec = hoppie_protocol::elements::find("DM_REQUEST_LOGON").unwrap();
        let resolved = hoppie_protocol::elements::resolve(spec, &[]).unwrap();
        let filled_text = resolved.filled_text.clone();
        let (message, _event) = thread.record_sent(
            spec.response,
            None,
            filled_text,
            hoppie_protocol::elements::ParsedElement::Recognized(resolved),
        );
        let packet = hoppie_protocol::cpdlc::encode(&message);

        let send_req = hoppie_protocol::wire::HoppieRequest {
            logon: "MOCK".into(),
            from: "MOCKPILOT".into(),
            to: "SERVER".into(),
            kind: hoppie_protocol::wire::PacketKind::Cpdlc,
            packet: Some(packet),
        };
        assert_eq!(
            http.send(&send_req).await.unwrap(),
            hoppie_protocol::wire::HoppieResponseLine::Ok
        );
        assert!(!thread.is_logged_on(), "must not flip before the reply is received");

        let poll_req = hoppie_protocol::wire::HoppieRequest {
            logon: "MOCK".into(),
            from: "MOCKPILOT".into(),
            to: "SERVER".into(),
            kind: hoppie_protocol::wire::PacketKind::Poll,
            packet: None,
        };
        let poll_result = http.send(&poll_req).await.unwrap();
        let hoppie_protocol::wire::HoppieResponseLine::OkWithPayload(body) = poll_result else {
            panic!("expected a queued LOGON ACCEPTED reply, got {poll_result:?}");
        };
        let envelopes = hoppie_protocol::wire::parse_poll_envelopes(&body);
        assert_eq!(envelopes.len(), 1);
        let reply = hoppie_protocol::cpdlc::decode(
            &envelopes[0].packet,
            hoppie_protocol::elements::Direction::Uplink,
        )
        .unwrap();
        thread.record_received(reply);
        assert!(thread.is_logged_on(), "LOGON ACCEPTED reply must flip logged_on");
    }
}
