//! v0.9.0 (#Discord-RPC) — Wiring zwischen Tauri-Commands und dem
//! `discord-presence`-Crate.
//!
//! Spec: docs/spec/v0.9.0-discord-rich-presence.md
//!
//! Design:
//!   - Globaler `MANAGER: OnceLock<Arc<DiscordPresenceManager>>` — eine
//!     einzige Instanz pro App-Run, init im setup-Hook von `lib.rs::run()`.
//!   - Settings werden in `<app_data_dir>/discord_rpc_settings.json`
//!     persistiert (analog activity_log etc.).
//!   - Tauri-Commands sind alle dünn: laden Settings, rufen den Manager,
//!     geben Status zurueck.
//!   - Phase-Updates kommen vom Frontend (das eh `ActiveFlightInfo` bei
//!     jedem Position-Tick neu rendert) per `discord_rpc_push_state`-Command —
//!     so spar ich mir die tiefe Integration in die Phase-FSM (= bewusst
//!     entkoppelt, damit der Discord-Code keinen einzigen flight_*-Pfad
//!     blockieren kann).

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use discord_presence::{
    DiscordPresenceManager, DiscordPresenceSettings, DiscordPresenceState, FlightPhase as DPhase,
    PresenceInput, SimKind as DSim,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

static MANAGER: OnceLock<Arc<DiscordPresenceManager>> = OnceLock::new();
static SETTINGS_PATH: OnceLock<PathBuf> = OnceLock::new();

const SETTINGS_FILE: &str = "discord_rpc_settings.json";

/// Settings beim Boot vom Disk laden. Fehler/Datei-fehlt → Defaults.
fn load_settings(app: &AppHandle) -> DiscordPresenceSettings {
    let Ok(dir) = app.path().app_data_dir() else {
        return DiscordPresenceSettings::default();
    };
    let path = dir.join(SETTINGS_FILE);
    SETTINGS_PATH.get_or_init(|| path.clone());
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<DiscordPresenceSettings>(&s).ok())
        .unwrap_or_default()
}

/// Settings auf Disk schreiben. Best-effort — ein Schreibfehler ist non-fatal.
fn save_settings(settings: &DiscordPresenceSettings) {
    let Some(path) = SETTINGS_PATH.get() else {
        return;
    };
    if let Ok(json) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(path, json);
    }
}

/// Einmaliger Init im Tauri-setup-Hook. Spawnt den Manager + zieht die
/// Discord-App-ID vom VPS-Public-Endpoint nach + ruft enable wenn der Pilot
/// in einem frueheren Run bereits zugestimmt hatte.
///
/// **Bugfix F18 (v0.9.2-Hotfix nach v0.9.1-QS):** Vorher wurde der Manager
/// mit den persistierten Settings erzeugt (inkl. `enabled=true`), dann
/// `apply_settings(same)` aufgerufen. Das `(was_enabled=true, new=true)`
/// matchte den No-Op-Branch und enable() lief nie → Pipe nie geoeffnet,
/// Status blieb "Deaktiviert" obwohl die Checkbox im UI an war. Fix:
/// Manager mit Default-Settings (enabled=false) erstellen, dann persistierte
/// Settings via apply_settings nachschieben → korrekter (false→true)-
/// Transition → enable() → Discord-Pipe wird geoeffnet.
pub fn init(app: &AppHandle) {
    let persisted = load_settings(app);
    // Manager mit DEFAULT-Settings (enabled=false) starten, NICHT mit den
    // persistierten Settings. Sonst frisst der no-op-Branch in apply_settings
    // den enable()-Call (F18-Bug v0.9.1).
    let manager = DiscordPresenceManager::new(DiscordPresenceSettings::default());
    let _ = MANAGER.set(manager.clone());
    tauri::async_runtime::spawn(async move {
        // App-ID vom Server nachziehen — laufzeit-konfiguriert via Webapp-
        // Admin. Wenn das fehlschlaegt (Server offline beim Boot), versucht's
        // beim naechsten Settings-Touch erneut.
        let _ = refresh_app_id(&manager).await;
        // Persistierte Settings JETZT anwenden — apply_settings sieht
        // (enabled=false → enabled=persisted.enabled) und triggert sauberen
        // enable()-Path falls persisted.enabled=true.
        let _ = manager.apply_settings(persisted).await;
    });
}

/// VPS-Public-Endpoint nachfragen + dem Manager die App-ID melden.
/// No-op-tolerant: jede Fehler-Stufe (Netz, JSON, leeres Feld) macht
/// einfach keine ID — der Manager zeigt im UI "nicht konfiguriert".
async fn refresh_app_id(manager: &Arc<DiscordPresenceManager>) -> Result<(), ()> {
    #[derive(serde::Deserialize)]
    struct Resp {
        rpc_app_id: String,
    }
    // Recorder-URL ist die gleiche wie fuer phpVMS-Position-Posts (= aus
    // Login-State); wir nutzen aber den festen Public-Pfad — keine Auth.
    // Wenn die VPS-URL noch nicht konfiguriert ist, fallen wir auf die
    // Default-VPS aus dem `aeroacars-mqtt`-Crate zurueck.
    let url = "https://live.kant.ovh/api/public/discord-rpc-config";
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|_| ())?
        .get(url)
        .send()
        .await
        .map_err(|_| ())?;
    let cfg: Resp = resp.json().await.map_err(|_| ())?;
    manager.set_app_id(cfg.rpc_app_id).await;
    Ok(())
}

fn manager() -> Option<Arc<DiscordPresenceManager>> {
    MANAGER.get().cloned()
}

// ─── Tauri-Commands ────────────────────────────────────────────────────────

#[tauri::command]
pub async fn discord_rpc_get_settings() -> Result<DiscordPresenceSettings, String> {
    match manager() {
        Some(m) => Ok(m.current_settings().await),
        None => Ok(DiscordPresenceSettings::default()),
    }
}

#[tauri::command]
pub async fn discord_rpc_set_settings(
    settings: DiscordPresenceSettings,
) -> Result<DiscordPresenceState, String> {
    save_settings(&settings);
    let Some(m) = manager() else {
        return Err("discord_rpc not initialized".into());
    };
    // App-ID nochmal frisch ziehen — falls Server beim Boot offline war oder
    // der VA-Owner gerade die ID neu gesetzt hat, sieht's der Pilot sofort.
    let _ = refresh_app_id(&m).await;
    // apply_settings selbst loggt; UI bekommt den frischen State zurueck
    let _ = m.apply_settings(settings).await;
    Ok(m.current_state().await)
}

#[tauri::command]
pub async fn discord_rpc_get_status() -> Result<DiscordPresenceState, String> {
    match manager() {
        Some(m) => Ok(m.current_state().await),
        None => Err("discord_rpc not initialized".into()),
    }
}

/// v0.9.1 F7 — Spec LE8 endgueltig verdrahten: vom Frontend gerufen wenn
/// SimConnect/X-Plane-Status auf "disconnected" oder "connecting" (= Verbindung
/// abgebrochen, Re-Connect-Loop) ueber den von der UI beobachteten SimStatus
/// faellt. Bei `true` wird der naechste Presence-Push das "⚠ Sim getrennt"-
/// Suffix anhaengen. Bei `false` faellt's wieder weg.
///
/// Manager dedupliziert intern (no-op wenn Wert nicht geaendert) — also
/// billig bei jedem Tick aufrufbar.
#[tauri::command]
pub async fn discord_rpc_set_sim_lost(lost: bool) -> Result<(), String> {
    let Some(m) = manager() else { return Ok(()) };
    m.set_sim_lost(lost).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn discord_rpc_send_test() -> Result<(), String> {
    let Some(m) = manager() else {
        return Err("discord_rpc not initialized".into());
    };
    m.send_test_presence().await.map_err(|e| e.to_string())
}

/// Vom Frontend bei flight_start aufgerufen. Setzt die initiale Presence.
/// Frontend liest dafuer ActiveFlightInfo (das eh in jedem Tab existiert).
#[derive(Debug, Deserialize)]
pub struct PushStateArgs {
    pub callsign: String,
    pub dep_icao: String,
    pub arr_icao: String,
    pub aircraft: String,
    pub altitude_ft: Option<i32>,
    pub phase: String, // canonical phase-string (PREFLIGHT, BOARDING, …)
    pub sim: String,   // "msfs2024" | "msfs2020" | "xplane11" | "xplane12" | "p3d" | ""
    pub start_unix: i64,
    pub profile_url: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PushStateResult {
    pub pushed: bool,
}

#[tauri::command]
pub async fn discord_rpc_push_state(args: PushStateArgs) -> Result<PushStateResult, String> {
    let Some(m) = manager() else {
        return Ok(PushStateResult { pushed: false });
    };
    let settings = m.current_settings().await;
    if !settings.enabled {
        return Ok(PushStateResult { pushed: false });
    }
    let input = PresenceInput {
        callsign: args.callsign,
        dep_icao: args.dep_icao,
        arr_icao: args.arr_icao,
        aircraft: args.aircraft,
        altitude_ft: args.altitude_ft,
        phase: parse_phase(&args.phase),
        sim: parse_sim(&args.sim),
        start_unix: args.start_unix,
        profile_url: args.profile_url,
    };
    // v0.19.x FIX: was `m.set_flight(input)` unconditionally — an
    // uncapped full Discord IPC push on every call from this ~2Hz
    // frontend loop. `push_state` reuses the existing activity for
    // routine altitude-only refreshes (throttled) and only does a full
    // push for a genuine new/changed flight identity.
    match m.push_state(input).await {
        Ok(()) => Ok(PushStateResult { pushed: true }),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn discord_rpc_clear_flight() -> Result<(), String> {
    let Some(m) = manager() else { return Ok(()) };
    m.clear_flight().await.map_err(|e| e.to_string())
}

/// Convenience-Hook fuer den App-Quit-Pfad: Activity sauber loeschen + Pipe zu.
/// Aktuell noch nicht aktiv verdrahtet — vorbereitet fuer einen
/// `on_window_event(Destroyed)`-Hook in lib.rs::run(). Im Praxis-Fall
/// macht der Discord-Client das Pipe-cleanup ohnehin selber, das ist
/// nur "extra clean".
#[allow(dead_code)]
pub async fn shutdown() {
    if let Some(m) = manager() {
        let _ = m.shutdown().await;
    }
}

// ─── Phase + Sim Mapping vom Telemetry-Contract-String ────────────────────

fn parse_phase(s: &str) -> DPhase {
    match s.to_ascii_uppercase().as_str() {
        "PREFLIGHT" => DPhase::Preflight,
        "BOARDING" => DPhase::Boarding,
        "PUSHBACK" => DPhase::Pushback,
        "TAXI_OUT" | "TAXIOUT" => DPhase::TaxiOut,
        "TAKEOFF_ROLL" | "TAKEOFFROLL" => DPhase::TakeoffRoll,
        "TAKEOFF" => DPhase::Takeoff,
        "REJECTED_TAKE_OFF" | "REJECTED_TAKEOFF" | "REJECTEDTAKEOFF" => DPhase::RejectedTakeoff,
        "CLIMB" => DPhase::Climb,
        "CRUISE" => DPhase::Cruise,
        // v0.9.0 QS-Hotfix F2: Holding + PirepSubmitted wurden vom FSM emittiert
        // aber hier nicht erkannt → fielen auf "PREFLIGHT" zurueck. Pilot im
        // Holding-Pattern sah falsch "PREFLIGHT" in Discord. Jetzt korrekt.
        "HOLDING" => DPhase::Holding,
        "DESCENT" => DPhase::Descent,
        "APPROACH" => DPhase::Approach,
        "FINAL" => DPhase::Final,
        "LANDING" => DPhase::Landing,
        "GO_AROUND" | "GOAROUND" => DPhase::GoAround,
        "TAXI_IN" | "TAXIIN" => DPhase::TaxiIn,
        "ARRIVED" => DPhase::Arrived,
        "BLOCKS_ON" | "BLOCKSON" => DPhase::BlocksOn,
        "DEBOARDING" => DPhase::Deboarding,
        "PIREP_SUBMITTED" | "PIREPSUBMITTED" => DPhase::PirepSubmitted,
        // Fallback: Preflight — falls eine neue Phase auftaucht die wir noch
        // nicht kennen, niemals crashen. Sollte aber heute keine Phase mehr
        // hier landen (alle FSM-Werte sind oben gemapped).
        _ => DPhase::Preflight,
    }
}

fn parse_sim(s: &str) -> DSim {
    match s.to_ascii_lowercase().as_str() {
        "msfs2024" => DSim::Msfs2024,
        "msfs2020" => DSim::Msfs2020,
        "xplane11" => DSim::Xplane11,
        "xplane12" => DSim::Xplane12,
        "p3d" | "prepar3d" => DSim::Prepar3d,
        _ => DSim::Unknown,
    }
}
