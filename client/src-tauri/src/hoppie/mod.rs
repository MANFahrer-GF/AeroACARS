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

/// Lives in `AppState::hoppie` while the poller is running, `None`
/// while stopped. Same start/stop-via-`Drop` shape as
/// `remote::RemoteServerHandle`.
pub struct HoppieHandle {
    stop_tx: watch::Sender<bool>,
    thread: Arc<StdMutex<hoppie_protocol::thread::CpdlcThread>>,
    last_error: Arc<StdMutex<Option<String>>>,
    last_verify: Option<VerifyOutcome>,
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
            }
        }
        None => HoppieStatus {
            connected: false,
            logged_on: false,
            pending_response_count: 0,
            last_error: None,
            logon_verified: None,
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
    let http = HoppieHttp::new()?;
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
    let logon = secrets::load_api_key(HOPPIE_LOGON_CODE_ACCOUNT)
        .map_err(|e| UiError::new("hoppie_secrets", e.to_string()))?
        .ok_or_else(|| {
            UiError::new(
                "hoppie_no_logon_code",
                "Kein Hoppie-Logon-Code hinterlegt.",
            )
        })?;
    // Phase 1: only the explicit callsign_override is honored — the
    // active-flight-derived default is wired in Phase 2, once a command
    // actually needs that context (see the project plan doc).
    let from = settings
        .callsign_override
        .clone()
        .filter(|c| !c.trim().is_empty())
        .map(|c| c.trim().to_uppercase())
        .ok_or_else(|| {
            UiError::new(
                "hoppie_no_callsign",
                "Kein Callsign hinterlegt. Bitte in den Hoppie-Einstellungen setzen.",
            )
        })?;

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

    let thread = Arc::new(StdMutex::new(hoppie_protocol::thread::CpdlcThread::new()));
    let last_error = Arc::new(StdMutex::new(None));
    let (stop_tx, stop_rx) = watch::channel(false);
    poller::spawn(
        app.clone(),
        Arc::clone(&http),
        Arc::clone(&thread),
        Arc::clone(&last_error),
        from,
        logon,
        settings.station_id.clone(),
        stop_rx,
    );

    *guard = Some(HoppieHandle {
        stop_tx,
        thread,
        last_error,
        last_verify: Some(verify),
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
