//! v0.9.0 (#Discord-RPC) — Discord Rich Presence Manager fuer AeroACARS.
//!
//! Spec: docs/spec/v0.9.0-discord-rich-presence.md
//!     + docs/spec/v0.9.0-telemetry-contract.md Sektion 9 (DSGVO-Gates)
//!
//! Ziel:
//!   - Pilot-Flugstatus live im Discord-Profil sichtbar
//!     ("GSG3184 · EDDB → KMRH" / "CRUISE · A320 · FL360")
//!   - 60s Heartbeat + Sofort-Update bei Phase-Wechsel
//!   - Default AUS (Opt-In), Privacy-by-Default (DSGVO Art. 6 (1) a)
//!   - Graceful Fallback wenn Discord nicht installiert / nicht offen
//!   - Optionaler Anonym-Modus ("GSG-Flight" statt "GSG3184")
//!
//! Architektur:
//!   - `DiscordPresenceManager` ist die einzige oeffentliche Surface
//!     fuer den Tauri-Code; alles andere ist intern oder pure-fn (in format.rs).
//!   - State-Machine im Mutex-geschuetzten Inner, async-Methoden auf der
//!     Outer-Wrapper-Struct sodass Tauri-Commands sie await-en koennen.
//!   - Heartbeat-Loop laeuft als spawned tokio-task; wird auf disable/shutdown
//!     abgebrochen.

pub mod format;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use chrono::Utc;
use discord_rich_presence::{
    activity::{Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

/// Discord-App-ID. Wird vom Pilot-Client zur Laufzeit beim Init gesetzt —
/// die Quelle ist `GET /api/public/discord-rpc-config` der VA-eigenen
/// aeroacars-live VPS (von dort vom VA-Owner via Webapp-Admin gepflegt).
///
/// Konsequenz: kein Re-Release noetig wenn die VA die Discord-App wechselt
/// oder erst nachschiebt; nicht im Binary einkompiliert; Forks sind
/// automatisch konfigurierbar gegen die jeweilige VPS-Konfiguration.

/// Heartbeat-Intervall — Discord erwartet einen Refresh innerhalb ~120s,
/// sonst loescht es die Activity. 60s gibt Sicherheits-Marge ohne nervig zu sein.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(60);

/// Test-Presence bleibt 15s sichtbar, dann automatisches Clear (Spec §UI/Test-Button).
const TEST_PRESENCE_DURATION: Duration = Duration::from_secs(15);

// ─── Public Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordPresenceSettings {
    /// Master-Toggle: AN -> Pipe oeffnen + Presence senden. Default AUS.
    pub enabled: bool,
    /// Wenn AN: "GSG3184" -> "GSG-Flight" (Route bleibt sichtbar).
    pub anonymize_callsign: bool,
    /// Wenn AN: Profil-Button-Anker auf der Presence (= phpVMS-URL).
    /// Default AN — Pilot kann es ausschalten wenn ihm das Profil-Link unangenehm ist.
    #[serde(default = "default_true")]
    pub show_profile_button: bool,
}

impl Default for DiscordPresenceSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            anonymize_callsign: false,
            show_profile_button: true,
        }
    }
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize)]
pub struct DiscordPresenceState {
    pub status: PresenceStatus,
    pub last_connect_attempt_at: Option<String>,
    pub last_update_at: Option<String>,
    pub client_id: String,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    Connected,
    NotFound,
    Disabled,
    Error,
}

#[derive(Debug, Clone)]
pub struct PresenceInput {
    pub callsign: String,
    pub dep_icao: String,
    pub arr_icao: String,
    pub aircraft: String,
    pub altitude_ft: Option<i32>,
    pub phase: FlightPhase,
    pub sim: SimKind,
    /// Flight-Start als UNIX-Timestamp. Discord nutzt das fuer die „X min"-Anzeige.
    pub start_unix: i64,
    /// Optionaler phpVMS-Profil-Link fuer den "Open Profile"-Button.
    pub profile_url: Option<String>,
}

/// Vollstaendige Phasen-Liste die der Discord-RPC darstellen kann.
///
/// Stand v0.9.0 sind 20 Werte definiert:
///   - 17 Phasen die der aktuelle Rust-FSM (sim_core::FlightPhase) wirklich emittiert
///     — inklusive `Holding` (v0.5.11) und `PirepSubmitted` (Post-Flight-State).
///   - 3 Phasen aus dem Telemetry-Contract Sektion 1.3 die fuer v0.10.0 vorgesehen
///     sind: `RejectedTakeoff`, `GoAround`, `Deboarding`. Werden vom FSM heute nicht
///     emittiert, das Mapping liegt aber schon hier damit der RPC-Code beim Phase-
///     Expansion-Release nicht angefasst werden muss.
///
/// Pflicht: jede dieser Phasen MUSS in format::phase_to_label() ein Mapping haben.
/// QS-Befund F2 (v0.9.0 Hotfix) — vorherige Version vermisste Holding +
/// PirepSubmitted, mit der Folge dass Discord PREFLIGHT statt der echten Phase
/// zeigte. Tests in tests/format.rs verifizieren das fuer alle 20 Werte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightPhase {
    Preflight,
    Boarding,
    Pushback,
    TaxiOut,
    TakeoffRoll,
    Takeoff,
    RejectedTakeoff,
    Climb,
    Cruise,
    /// v0.5.11 — sustained Holding-Pattern (bank > 15°, |VS| < 200 fpm, > 90s)
    Holding,
    Descent,
    Approach,
    Final,
    Landing,
    GoAround,
    TaxiIn,
    Arrived,
    BlocksOn,
    Deboarding,
    /// Post-Flight: PIREP wurde an phpVMS gefilt — Flight ist offiziell zu.
    PirepSubmitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimKind {
    Msfs2020,
    Msfs2024,
    Xplane11,
    Xplane12,
    Prepar3d,
    Unknown,
}

// ─── Manager ───────────────────────────────────────────────────────────────

pub struct DiscordPresenceManager {
    inner: Mutex<Inner>,
}

struct Inner {
    client: Option<DiscordIpcClient>,
    settings: DiscordPresenceSettings,
    state: DiscordPresenceState,
    last_input: Option<PresenceInput>,
    sim_lost: bool,
    heartbeat_handle: Option<JoinHandle<()>>,
    /// Runtime-konfigurierte App-ID vom VPS-Public-Endpoint. Leerer String
    /// = vom VA-Owner noch nicht gepflegt → enable() scheitert mit Hinweis.
    app_id: String,
    /// v0.19.x FIX: wall-clock throttle for altitude-only pushes (see
    /// `ALTITUDE_PUSH_MIN_INTERVAL`). `update_phase`'s own "did anything
    /// change" dedup provides ZERO real rate-limiting during climb/descent
    /// — the frontend reports altitude rounded to the nearest FOOT, which
    /// changes on almost every tick whenever the aircraft isn't level.
    last_altitude_only_push_at: Option<Instant>,
}

/// v0.19.x FIX: Discord's documented limit is 5 presence updates per 20s
/// (= one per 4s minimum). The live push path used to call `set_flight`
/// unconditionally at ~2Hz — ~8x over that limit. Altitude-only refreshes
/// (the ~2Hz stream) are throttled to this interval; phase transitions and
/// genuine identity changes (new flight, divert, aircraft swap) are rare
/// by nature and always push immediately regardless of this throttle.
const ALTITUDE_PUSH_MIN_INTERVAL: Duration = Duration::from_secs(15);

impl DiscordPresenceManager {
    pub fn new(settings: DiscordPresenceSettings) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                client: None,
                settings,
                state: DiscordPresenceState {
                    status: PresenceStatus::Disabled,
                    last_connect_attempt_at: None,
                    last_update_at: None,
                    client_id: String::new(),
                    error_message: None,
                },
                last_input: None,
                sim_lost: false,
                heartbeat_handle: None,
                app_id: String::new(),
                last_altitude_only_push_at: None,
            }),
        })
    }

    /// Discord-App-ID setzen. Wird vom Wiring-Code direkt nach `new()`
    /// aufgerufen, sobald `GET /api/public/discord-rpc-config` antwortet.
    /// Idempotent — wenn die ID sich aendert (selten), schliessen wir die
    /// aktuelle Pipe und der naechste enable() oeffnet eine neue.
    pub async fn set_app_id(self: &Arc<Self>, app_id: String) {
        let mut inner = self.inner.lock().await;
        if inner.app_id == app_id {
            return;
        }
        inner.app_id = app_id.clone();
        inner.state.client_id = app_id;
        // Bestehende Verbindung passt zur alten ID — wegwerfen
        if let Some(mut c) = inner.client.take() {
            let _ = c.close();
        }
    }

    /// Snapshot fuer das Frontend (Settings-Status-Anzeige).
    pub async fn current_state(&self) -> DiscordPresenceState {
        let inner = self.inner.lock().await;
        inner.state.clone()
    }

    pub async fn current_settings(&self) -> DiscordPresenceSettings {
        let inner = self.inner.lock().await;
        inner.settings.clone()
    }

    /// Pilot hat den Master-Toggle umgelegt oder ein anderes Setting geaendert.
    /// Bei `enabled=true` und !connected → connect + send + start heartbeat.
    /// Bei `enabled=false` → clear activity + close pipe + stop heartbeat.
    /// Bei Anonym-Toggle change wird das naechste set_activity neu berechnet
    /// (kein Sofort-Push noetig, der Heartbeat-Tick reicht).
    ///
    /// **F18-Hotfix v0.9.2: Defense-in-Depth.** Vorher matchte die
    /// (was_enabled=true, new=true)-Branch nur Settings-Aenderungen und nahm
    /// stillschweigend an, dass der Client bereits gebunden ist. War aber
    /// bei Init-Race nicht der Fall → Pipe nie geoeffnet, Status blieb
    /// Disabled. Jetzt prueft der Code zusaetzlich den tatsaechlichen Client-
    /// Status: wenn enabled=true UND keine Verbindung → enable() retry, egal
    /// in welchem Match-Arm. Macht apply_settings idempotent gegen jeden
    /// inneren Inkonsistenz-Zustand.
    pub async fn apply_settings(self: &Arc<Self>, new_settings: DiscordPresenceSettings) -> Result<()> {
        let was_enabled;
        let client_bound;
        {
            let mut inner = self.inner.lock().await;
            was_enabled = inner.settings.enabled;
            client_bound = inner.client.is_some();
            inner.settings = new_settings.clone();
        }

        // Master-Decision: wenn die neuen Settings "enabled" sagen und wir
        // KEINEN Client gebunden haben (egal aus welchem Grund: Init-Race,
        // vorheriger enable()-Fail, Discord-nicht-da-beim-Boot), versuchen
        // wir zu connecten. enable() ist idempotent + self-heals via
        // Heartbeat-Loop wenn Discord erst spaeter aufgeht.
        if new_settings.enabled && !client_bound {
            return self.enable().await;
        }

        match (was_enabled, new_settings.enabled) {
            (false, true) => self.enable().await,
            (true, false) => self.disable().await,
            (true, true) => {
                // Client schon gebunden + immer noch enabled → nur
                // Anonym-Toggle / Profile-Button-Toggle koennte sich geaendert
                // haben. Sofortiger Re-Push damit der Pilot die Aenderung
                // gleich sieht.
                self.push_current_activity().await.ok();
                Ok(())
            }
            (false, false) => Ok(()),
        }
    }

    async fn enable(self: &Arc<Self>) -> Result<()> {
        let id = {
            let inner = self.inner.lock().await;
            inner.app_id.clone()
        };
        if id.is_empty() {
            let mut inner = self.inner.lock().await;
            inner.state.status = PresenceStatus::Error;
            inner.state.error_message = Some(
                "Discord-App-ID ist nicht vom Server konfiguriert. Der VA-Owner \
                 muss sie im Webapp-Admin → Settings → Discord setzen."
                    .to_string(),
            );
            return Err(anyhow!("missing Discord app_id from server"));
        }

        let now = Utc::now().to_rfc3339();
        let connect_result = {
            let mut inner = self.inner.lock().await;
            inner.state.last_connect_attempt_at = Some(now);

            // Discord-IPC-Client erstellen + connecten.
            // In discord-rich-presence v0.2.5 returns DiscordIpcClient::new ein
            // Result (kann beim Pipe-Path-Build scheitern); .connect() ist sync.
            match DiscordIpcClient::new(&id).and_then(|mut client| {
                client.connect().map(|_| client)
            }) {
                Ok(client) => {
                    inner.client = Some(client);
                    inner.state.status = PresenceStatus::Connected;
                    inner.state.error_message = None;
                    info!(client_id=%id, "[discord-rpc] connected");
                    Ok(())
                }
                Err(e) => {
                    inner.state.status = PresenceStatus::NotFound;
                    inner.state.error_message = Some(format!("{e:?}"));
                    debug!(error=?e, "[discord-rpc] connect failed (Discord not running?)");
                    Err(anyhow!("connect failed: {e:?}"))
                }
            }
        };

        // Heartbeat-Loop unabhaengig vom initialen Connect-Erfolg starten —
        // wenn Discord spaeter aufgeht und der naechste Tick connectet,
        // klappt die Pipe und die Presence kommt nach. (Implementierung
        // im Heartbeat-Loop: wenn !client.is_some(), retry-connect.)
        let weak = Arc::downgrade(self);
        let handle = tokio::spawn(async move {
            let mut tick = tokio::time::interval(HEARTBEAT_INTERVAL);
            tick.tick().await; // erster Tick ist instant, ueberspringen
            loop {
                tick.tick().await;
                let Some(mgr) = weak.upgrade() else { return };
                mgr.heartbeat_step().await;
            }
        });

        {
            let mut inner = self.inner.lock().await;
            if let Some(old) = inner.heartbeat_handle.replace(handle) {
                old.abort();
            }
        }

        // initiale Presence pushen wenn schon ein Flug aktiv ist
        self.push_current_activity().await.ok();

        // Connect-Result transparent durchreichen damit der Caller log/UI weiss
        connect_result
    }

    async fn disable(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        if let Some(handle) = inner.heartbeat_handle.take() {
            handle.abort();
        }
        if let Some(mut client) = inner.client.take() {
            let _ = client.clear_activity();
            let _ = client.close();
        }
        inner.state.status = PresenceStatus::Disabled;
        inner.state.error_message = None;
        info!("[discord-rpc] disabled");
        Ok(())
    }

    /// Sauberer App-Quit-Cleanup. Idempotent.
    pub async fn shutdown(&self) -> Result<()> {
        self.disable().await
    }

    /// Pilot hat einen Flug gestartet → initialer Presence-Push.
    pub async fn set_flight(self: &Arc<Self>, input: PresenceInput) -> Result<()> {
        {
            let mut inner = self.inner.lock().await;
            inner.last_input = Some(input);
            // A fresh flight identity starts its own throttle window —
            // don't let a stale timestamp from a previous flight/session
            // delay this flight's first altitude-only refresh.
            inner.last_altitude_only_push_at = None;
        }
        self.push_current_activity().await
    }

    /// Nur Phase + Altitude haben sich geaendert (alter PresenceInput recyceln).
    ///
    /// v0.19.x FIX: a phase change always pushes immediately (rare, always
    /// meaningful to viewers). An altitude-only change is throttled to
    /// `ALTITUDE_PUSH_MIN_INTERVAL` — the stored state still updates every
    /// call, so the NEXT allowed push (whether from throttle expiry or a
    /// phase change) always carries the latest altitude; only the Discord
    /// IPC call itself is rate-limited.
    pub async fn update_phase(
        self: &Arc<Self>,
        phase: FlightPhase,
        altitude_ft: Option<i32>,
    ) -> Result<()> {
        let should_push = {
            let mut inner = self.inner.lock().await;
            let Some(last) = inner.last_input.as_mut() else {
                return Ok(()); // kein aktiver Flug
            };
            if last.phase == phase && last.altitude_ft == altitude_ft {
                return Ok(()); // nichts geaendert
            }
            let phase_changed = last.phase != phase;
            last.phase = phase;
            last.altitude_ft = altitude_ft;
            if phase_changed {
                true
            } else {
                let now = Instant::now();
                let due = inner
                    .last_altitude_only_push_at
                    .map(|t| now.duration_since(t) >= ALTITUDE_PUSH_MIN_INTERVAL)
                    .unwrap_or(true);
                if due {
                    inner.last_altitude_only_push_at = Some(now);
                }
                due
            }
        };
        if !should_push {
            return Ok(());
        }
        self.push_current_activity().await
    }

    /// v0.19.x FIX: smart entry point for the frontend's live push loop
    /// (`discord_rpc_push_state`, driven by the phase-FSM at up to ~2Hz).
    /// The old wiring called `set_flight` — a full, unconditional Discord
    /// IPC push — on EVERY call, ~8x over Discord's documented 5-updates/
    /// 20s rate limit, because `update_phase`'s dedup/throttle logic had
    /// zero real callers. Reuses the existing Discord activity (routing
    /// through `update_phase`, which throttles altitude-only refreshes)
    /// whenever the core flight identity is unchanged from the last push,
    /// and only does a full push when it actually changed: first call this
    /// flight, a divert, an aircraft/sim swap, or a fresh profile_url.
    pub async fn push_state(self: &Arc<Self>, input: PresenceInput) -> Result<()> {
        let same_core = {
            let inner = self.inner.lock().await;
            inner.last_input.as_ref().is_some_and(|last| {
                last.callsign == input.callsign
                    && last.dep_icao == input.dep_icao
                    && last.arr_icao == input.arr_icao
                    && last.aircraft == input.aircraft
                    && last.sim == input.sim
                    && last.start_unix == input.start_unix
                    && last.profile_url == input.profile_url
            })
        };
        if same_core {
            self.update_phase(input.phase, input.altitude_ft).await
        } else {
            self.set_flight(input).await
        }
    }

    /// MQTT/Sim Disconnect → "⚠ Sim getrennt"-Suffix in der State-Zeile (LE8).
    pub async fn set_sim_lost(self: &Arc<Self>, lost: bool) -> Result<()> {
        {
            let mut inner = self.inner.lock().await;
            if inner.sim_lost == lost {
                return Ok(());
            }
            inner.sim_lost = lost;
        }
        self.push_current_activity().await
    }

    /// Flight ist beendet → Presence komplett weg.
    pub async fn clear_flight(&self) -> Result<()> {
        let mut inner = self.inner.lock().await;
        inner.last_input = None;
        inner.sim_lost = false;
        if let Some(client) = inner.client.as_mut() {
            client.clear_activity().map_err(|e| anyhow!("{e:?}"))?;
            inner.state.last_update_at = Some(Utc::now().to_rfc3339());
        }
        Ok(())
    }

    /// Test-Presence: 15s sichtbar mit Dummy-Daten, dann auto-clear.
    /// Pilot kann verifizieren dass Discord die App sieht ohne dafuer einen
    /// echten Flug starten zu muessen.
    pub async fn send_test_presence(self: &Arc<Self>) -> Result<()> {
        let test = PresenceInput {
            callsign: "AERO-TEST".to_string(),
            dep_icao: "EDDB".to_string(),
            arr_icao: "EDDK".to_string(),
            aircraft: "A320".to_string(),
            altitude_ft: Some(36000),
            phase: FlightPhase::Cruise,
            sim: SimKind::Msfs2024,
            start_unix: Utc::now().timestamp(),
            profile_url: None,
        };
        self.set_flight(test).await?;

        // Auto-Clear-Task
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(TEST_PRESENCE_DURATION).await;
            if let Some(mgr) = weak.upgrade() {
                let _ = mgr.clear_flight().await;
            }
        });
        Ok(())
    }

    /// Heartbeat-Step: re-send Presence (Discord-Timeout-Schutz). Wenn der Client
    /// disconnected ist, versuchen wir einen Re-Connect — Discord koennte
    /// inzwischen aufgegangen sein.
    async fn heartbeat_step(self: &Arc<Self>) {
        // Lock kurz halten — nur lesen ob enabled
        {
            let inner = self.inner.lock().await;
            if !inner.settings.enabled {
                return;
            }
        }

        // Re-connect-Versuch wenn keine Verbindung
        {
            let mut inner = self.inner.lock().await;
            if inner.client.is_none() && !inner.app_id.is_empty() {
                if let Ok(client) = DiscordIpcClient::new(&inner.app_id)
                    .and_then(|mut c| c.connect().map(|_| c))
                {
                    inner.client = Some(client);
                    inner.state.status = PresenceStatus::Connected;
                    inner.state.error_message = None;
                    info!("[discord-rpc] reconnected via heartbeat");
                }
            }
        }
        let _ = self.push_current_activity().await;
    }

    /// Baut das Activity-Objekt aus inner.last_input und sendet es.
    /// No-Op wenn kein aktiver Flug oder kein verbundener Client.
    async fn push_current_activity(self: &Arc<Self>) -> Result<()> {
        let mut inner = self.inner.lock().await;
        let Some(input) = inner.last_input.clone() else {
            return Ok(()); // nichts zu zeigen
        };
        let settings = inner.settings.clone();
        let sim_lost = inner.sim_lost;

        let Some(client) = inner.client.as_mut() else {
            return Ok(()); // Discord offline; Heartbeat retry's spaeter
        };

        let details = format::build_details(&input, settings.anonymize_callsign);
        let state = format::build_state(&input, sim_lost);
        let small_image = format::sim_to_asset_key(input.sim).unwrap_or("");
        let small_tooltip = format::sim_to_tooltip(input.sim);

        let mut assets = Assets::new().large_image(format::ASSET_LOGO);
        if !small_image.is_empty() {
            assets = assets.small_image(small_image).small_text(small_tooltip);
        }

        let activity = Activity::new()
            .details(&details)
            .state(&state)
            .assets(assets)
            .timestamps(Timestamps::new().start(input.start_unix));

        match client.set_activity(activity) {
            Ok(_) => {
                inner.state.status = PresenceStatus::Connected;
                inner.state.error_message = None;
                inner.state.last_update_at = Some(Utc::now().to_rfc3339());
                Ok(())
            }
            Err(e) => {
                // Pipe abgerissen → Client wegwerfen, naechster Heartbeat versucht's
                inner.client = None;
                inner.state.status = PresenceStatus::NotFound;
                inner.state.error_message = Some(format!("{e:?}"));
                warn!(error=?e, "[discord-rpc] set_activity failed, will retry");
                Err(anyhow!("{e:?}"))
            }
        }
    }
}

// v0.19.x FIX: rate-limiting for the live push path. `inner.client` is
// always `None` in these tests (no real Discord IPC connection), which
// makes `push_current_activity` a clean no-op per its own doc comment —
// exactly what lets these tests observe the throttle/routing STATE
// (last_altitude_only_push_at, last_input) without needing to mock IPC.
#[cfg(test)]
mod tests {
    use super::*;

    fn input(callsign: &str, phase: FlightPhase, altitude_ft: i32) -> PresenceInput {
        PresenceInput {
            callsign: callsign.to_string(),
            dep_icao: "EDDF".to_string(),
            arr_icao: "EDDM".to_string(),
            aircraft: "A320".to_string(),
            altitude_ft: Some(altitude_ft),
            phase,
            sim: SimKind::Msfs2024,
            start_unix: 1_700_000_000,
            profile_url: None,
        }
    }

    #[tokio::test]
    async fn altitude_only_pushes_are_throttled() {
        let m = DiscordPresenceManager::new(DiscordPresenceSettings::default());
        m.set_flight(input("TKJ1224", FlightPhase::Climb, 1000))
            .await
            .unwrap();

        m.update_phase(FlightPhase::Climb, Some(1050)).await.unwrap();
        let first_push_at = m.inner.lock().await.last_altitude_only_push_at;
        assert!(
            first_push_at.is_some(),
            "the first altitude-only push after set_flight must be allowed"
        );

        // Rapid follow-up altitude change — must be throttled (real-world
        // equivalent: the next tick, ~0.5-1s later).
        m.update_phase(FlightPhase::Climb, Some(1100)).await.unwrap();
        let second_push_at = m.inner.lock().await.last_altitude_only_push_at;
        assert_eq!(
            first_push_at, second_push_at,
            "a rapid follow-up altitude-only change must be throttled, not pushed"
        );

        // The STORED altitude must still be the latest value even though
        // it wasn't pushed to Discord yet — only the IPC call is throttled.
        let stored_alt = m.inner.lock().await.last_input.as_ref().unwrap().altitude_ft;
        assert_eq!(stored_alt, Some(1100));
    }

    #[tokio::test]
    async fn phase_change_always_pushes_immediately_even_within_the_throttle_window() {
        let m = DiscordPresenceManager::new(DiscordPresenceSettings::default());
        m.set_flight(input("TKJ1224", FlightPhase::Climb, 1000))
            .await
            .unwrap();
        m.update_phase(FlightPhase::Climb, Some(1050)).await.unwrap(); // consumes the throttle slot
        let throttle_after_altitude = m.inner.lock().await.last_altitude_only_push_at;

        // Immediately — well within the throttle window — transition phase.
        m.update_phase(FlightPhase::Cruise, Some(35000)).await.unwrap();
        let phase_now = m.inner.lock().await.last_input.as_ref().unwrap().phase;
        assert_eq!(
            phase_now,
            FlightPhase::Cruise,
            "a phase change must apply immediately, never throttled"
        );
        // A phase-change push is a different code path from the altitude
        // throttle and must not touch its timestamp.
        let throttle_after_phase = m.inner.lock().await.last_altitude_only_push_at;
        assert_eq!(throttle_after_altitude, throttle_after_phase);
    }

    #[tokio::test]
    async fn push_state_routes_through_update_phase_when_the_core_identity_is_unchanged() {
        let m = DiscordPresenceManager::new(DiscordPresenceSettings::default());
        m.push_state(input("TKJ1224", FlightPhase::Climb, 1000))
            .await
            .unwrap();
        assert!(
            m.inner.lock().await.last_altitude_only_push_at.is_none(),
            "the first call (set_flight path) must not touch the altitude throttle"
        );

        m.push_state(input("TKJ1224", FlightPhase::Climb, 1050))
            .await
            .unwrap();
        let after_first_altitude = m.inner.lock().await.last_altitude_only_push_at;
        assert!(after_first_altitude.is_some());

        m.push_state(input("TKJ1224", FlightPhase::Climb, 1100))
            .await
            .unwrap();
        let after_second_altitude = m.inner.lock().await.last_altitude_only_push_at;
        assert_eq!(
            after_first_altitude, after_second_altitude,
            "a rapid same-core-identity altitude push must be throttled via update_phase"
        );
    }

    #[tokio::test]
    async fn push_state_does_a_full_push_when_the_core_identity_changes() {
        let m = DiscordPresenceManager::new(DiscordPresenceSettings::default());
        m.push_state(input("TKJ1224", FlightPhase::Climb, 1000))
            .await
            .unwrap();
        m.update_phase(FlightPhase::Climb, Some(1050)).await.unwrap(); // consume the throttle slot
        assert!(m.inner.lock().await.last_altitude_only_push_at.is_some());

        // A different arr_icao (divert / effectively a new flight) must go
        // through set_flight — NOT the throttled update_phase path.
        let mut divert = input("TKJ1224", FlightPhase::Climb, 1200);
        divert.arr_icao = "EDDL".to_string();
        m.push_state(divert).await.unwrap();

        assert!(
            m.inner.lock().await.last_altitude_only_push_at.is_none(),
            "a core-identity change must route through set_flight, which resets the throttle"
        );
        assert_eq!(
            m.inner.lock().await.last_input.as_ref().unwrap().arr_icao,
            "EDDL"
        );
    }
}
