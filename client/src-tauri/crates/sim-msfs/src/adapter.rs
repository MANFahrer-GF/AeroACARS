//! Windows-only raw SimConnect adapter.
//!
//! Owns a worker thread that connects to SimConnect, registers a
//! single data definition, subscribes to per-second updates and
//! pushes parsed [`SimSnapshot`]s into a shared mutex. The public
//! [`MsfsAdapter`] API is the same as the legacy adapter so the rest
//! of the application doesn't need to change.

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::facility;
use chrono::Utc;
use serde::Serialize;
use sim_core::{AircraftProfile, SimKind, SimSnapshot, Simulator};

mod sys;
mod telemetry;

use telemetry::{InspectorState, Touchdown, TELEMETRY_FIELDS, TOUCHDOWN_FIELDS};
pub use telemetry::{InspectorWatch, WatchKind, WatchValue};

// IDs used in our SimConnect calls — chosen freely as long as they're
// unique within the connection. Data definition #1 holds the per-tick
// telemetry; #2 the touchdown snapshot, which only the simulation
// itself fills (and only at the moment the gear hits the ground).
// Splitting them means a touchdown SimVar rejection can never shift
// the live telemetry layout — same reason we left the old crate
// behind.
const DEFINITION_ID: sys::SIMCONNECT_DATA_DEFINITION_ID = 1;
const REQUEST_ID: sys::SIMCONNECT_DATA_REQUEST_ID = 1;
const TOUCHDOWN_DEFINITION_ID: sys::SIMCONNECT_DATA_DEFINITION_ID = 2;
const TOUCHDOWN_REQUEST_ID: sys::SIMCONNECT_DATA_REQUEST_ID = 2;
/// Definition #3: live inspector watchlist, re-registered on every
/// add/remove. Lives in its own slot so a typo in a user-supplied
/// SimVar name can't take down the per-tick telemetry.
const INSPECTOR_DEFINITION_ID: sys::SIMCONNECT_DATA_DEFINITION_ID = 3;
const INSPECTOR_REQUEST_ID: sys::SIMCONNECT_DATA_REQUEST_ID = 3;
/// Definition #10: Bahnen und Rollwege aus der geladenen Szenerie
/// (v1.7.8). Eigener Platz, damit ein abgelehnter Feldname weder die
/// Telemetrie noch die Aufsetzprobe verschiebt — dieselbe Ueberlegung
/// wie bei der Trennung von Telemetrie und Touchdown.
const FACILITY_DEFINITION_ID: sys::SIMCONNECT_DATA_DEFINITION_ID = 10;
/// Basis der Facility-Anfragekennungen.
///
/// ⚠ Jeder VERSUCH bekommt `BASIS + Auftragskennung`. Eine feste
/// Kennung fuer alle Anfragen liess eine nach der Wartezeit
/// eintreffende Antwort nicht mehr von der laufenden unterscheiden
/// (QS-Befund 1, 01.09.2026). Die 1000 liegt ueber allen anderen
/// vergebenen Kennungen (1, 2, 3, 100, 101, 200).
const FACILITY_REQUEST_BASE: sys::SIMCONNECT_DATA_REQUEST_ID = 1000;

// ---- PMDG SDK ClientData IDs (Phase H.4) ----
//
// The PMDG NG3 + 777X SDKs use SimConnect ClientData (NOT the
// standard SimObject data). They define their own data area names
// + IDs in the SDK header (constants from `pmdg::ng3` /
// `pmdg::x777`). We re-use the IDs defined by PMDG verbatim so the
// `MapClientDataNameToID` call binds correctly. Definition + request
// IDs we choose ourselves (must be unique within our own SimConnect
// session). 100+ keeps them out of the existing telemetry-id range.
const PMDG_NG3_DEFINITION_ID: sys::SIMCONNECT_DATA_DEFINITION_ID = 100;
const PMDG_NG3_REQUEST_ID: sys::SIMCONNECT_DATA_REQUEST_ID = 100;
const PMDG_X777_DEFINITION_ID: sys::SIMCONNECT_DATA_DEFINITION_ID = 101;
const PMDG_X777_REQUEST_ID: sys::SIMCONNECT_DATA_REQUEST_ID = 101;
const AIRCRAFT_LOADED_REQUEST_ID: sys::SIMCONNECT_DATA_REQUEST_ID = 200;
const SIM_START_EVENT_ID: u32 = 300;
/// Spec v0.7.15 F5 (QS-Round-2): SimConnect `Pause_EX1`-Event statt der
/// zwei separaten `Paused`/`Unpaused`-Events. `Pause_EX1` schickt
/// sofort den aktuellen Pause-State + bei jedem Wechsel ein Update
/// mit `dwData`-Flag-Set:
///   bit 0 (0x01) = SIMCONNECT_PAUSE_FLAG_PAUSE         (Full Pause)
///   bit 1 (0x02) = SIMCONNECT_PAUSE_FLAG_PAUSE_WITH_SOUND
///   bit 2 (0x04) = SIMCONNECT_PAUSE_FLAG_ACTIVE_PAUSE
///   bit 3 (0x08) = SIMCONNECT_PAUSE_FLAG_SIM_PAUSE
/// = 0 → kein Pause. != 0 → irgendeine Pause-Variante.
///
/// Vorteil ggue. `Paused`+`Unpaused`: wenn AeroACARS connectet
/// waehrend MSFS schon pausiert ist, kommt sofort ein initialer
/// Pause_EX1-Event mit dem aktuellen State. Bei den zwei separaten
/// Events haette `sim_paused` bis zum naechsten Toggle weiter `false`
/// gezeigt.
///
/// SDK-Doku: https://docs.flightsimulator.com/html/Programming_Tools/SimConnect/API_Reference/Events_And_Data/SimConnect_SubscribeToSystemEvent.htm
const PAUSE_EX1_EVENT_ID: u32 = 301;
/// v0.7.19 GAF-707 Accident-Detection: SimConnect-`Crashed`-System-
/// Event. Wird gefeuert wenn der User-Aircraft im Sim crashed (Boden-
/// kontakt mit nicht-ueberlebbaren Parametern, Stall ins Terrain etc).
/// Adapter latcht das in `shared.crashed` — `CrashReset` (= MSFS-UI
/// Cut-Scene fertig) loescht den raw Flag wieder, aber der aktive
/// Flug behaelt seinen accident_detected-Latch in lib.rs/FlightStats.
/// SDK-Doku siehe oben SubscribeToSystemEvent Link.
const CRASHED_EVENT_ID: u32 = 302;
const CRASH_RESET_EVENT_ID: u32 = 303;
/// Flow-Ereignisse, bei denen die Telemetrie nicht den geflogenen Zustand
/// beschreibt. Werte aus `SimConnect.h`, Enum `SIMCONNECT_FLOW_EVENT`.
///
/// Bewusst ueber die bindgen-Konstanten und nicht ueber Zahlen: die
/// Reihenfolge im Enum ist SDK-Sache und darf sich aendern.
const FLOW_REPLAY_START: u32 = sys::SIMCONNECT_FLOW_EVENT_SIMCONNECT_FLOW_EVENT_REPLAY_START as u32;
const FLOW_REPLAY_END: u32 = sys::SIMCONNECT_FLOW_EVENT_SIMCONNECT_FLOW_EVENT_REPLAY_END as u32;
const FLOW_TELEPORT_START: u32 =
    sys::SIMCONNECT_FLOW_EVENT_SIMCONNECT_FLOW_EVENT_TELEPORT_START as u32;
const FLOW_TELEPORT_DONE: u32 =
    sys::SIMCONNECT_FLOW_EVENT_SIMCONNECT_FLOW_EVENT_TELEPORT_DONE as u32;
const FLOW_SKIP_START: u32 = sys::SIMCONNECT_FLOW_EVENT_SIMCONNECT_FLOW_EVENT_SKIP_START as u32;
const FLOW_SKIP_DONE: u32 = sys::SIMCONNECT_FLOW_EVENT_SIMCONNECT_FLOW_EVENT_SKIP_DONE as u32;

const STALE_TIMEOUT: Duration = Duration::from_secs(5);

/// Public connection state mirrored to the frontend.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// External-facing MSFS adapter. Cheap to clone-state; drives a
/// background worker thread that talks to SimConnect.
use sim_core::szenerie::SzenerieDiagnose;

pub struct MsfsAdapter {
    shared: Arc<Shared>,
    worker: Option<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

struct Shared {
    state: Mutex<ConnectionState>,
    snapshot: Mutex<Option<SimSnapshot>>,
    last_error: Mutex<Option<String>>,
    /// v1.7.8 — die zuletzt vollstaendig gelieferte Szenerie-Auskunft.
    ///
    /// ⚠ Wird ERST bei `FacilityDataEnde` gefuellt. Die Lieferung kommt
    /// stueckweise; eine halbe Bahnliste saehe aus wie ein Flughafen mit
    /// einer Bahn — und die Bewertung wuerde gegen die falsche messen,
    /// ohne dass etwas anschlaegt.
    ///
    /// ⚠ Seit v1.7.14 ein Buch nach Platz statt eines einzelnen Fachs.
    /// Der Grund steht bei `Auftragsbuch`: Start und Ziel haben sich
    /// gegenseitig verdraengt, eine Lieferung wurde mit dem falschen
    /// Platz beschriftet, und ein stummer Platz wurde nie wieder
    /// gefragt.
    szenerie: Mutex<sim_core::szenerie::Auftragsbuch>,
    /// Steht eine Anfrage aus, die der Verbindungsfaden noch stellen muss?
    szenerie_offen: AtomicBool,
    /// Wie weit die Szenerie-Abfrage gekommen ist — fuer die Diagnose.
    ///
    /// ⚠ Ohne das ist die Sache nicht messbar: Am Flug stand bisher nur
    /// "navdaten", und darin steckten drei voellig verschiedene Faelle
    /// (nie gefragt / abgelehnt / geantwortet-aber-unbrauchbar). Genau
    /// deshalb war am 29.08.2026 nicht zu sagen, warum die MSFS-Haelfte
    /// von v1.7.8 bei 5 von 5 Landungen nichts geliefert hat.
    szenerie_diagnose: Mutex<SzenerieDiagnose>,
    /// Name und Version, mit denen sich der Simulator beim Verbinden
    /// meldet (aus `SIMCONNECT_RECV_OPEN`). MSFS 2020 und 2024 melden
    /// sich unterschiedlich — und die Feldnamen der Facility-Abfrage
    /// stammen aus der 2024er-SDK-Doku. Ohne diese Kennung laesst sich
    /// nicht sagen, ob eine Ablehnung an der Fassung liegt.
    sim_kennung: Mutex<Option<String>>,
    /// Spec v0.7.15 F5: SimConnect-`Paused`/`Unpaused`-System-Events
    /// setzen dieses Atomic. Wird beim Bauen jedes `SimSnapshot` in
    /// `telemetry::parse` zurueck nach `snap.paused` kopiert, damit
    /// der Streamer-Loop in `lib.rs` ueber den bestehenden Snapshot-
    /// Pfad lesen kann (= keine zweite IPC-Schiene noetig). Default
    /// false bis der Sim das erste Mal pausiert.
    sim_paused: AtomicBool,
    /// v1.6.12 — der Simulator spielt eine Aufzeichnung ab oder versetzt das
    /// Flugzeug, statt es fliegen zu lassen.
    ///
    /// Quelle sind die Flow-Ereignisse des SimConnect-SDK
    /// (`REPLAY_START`/`REPLAY_END`, `TELEPORT_START`/`TELEPORT_DONE`,
    /// `SKIP_START`/`SKIP_DONE`). Behandelt wird das wie eine Pause — genau so
    /// haelt es der X-Plane-Adapter seit Spec v0.7.15 F6 mit
    /// `sim/time/is_in_replay`: aus AeroACARS-Sicht ist das „die Telemetrie ist
    /// gerade nicht echt".
    ///
    /// Zaehler statt Ja/Nein: die Ereignisse koennen sich ueberlappen (ein
    /// Teleport waehrend eines Replays). Ein einzelnes `..._DONE` wuerde sonst
    /// den noch laufenden anderen Vorgang mit abraeumen.
    sim_unecht_tiefe: AtomicI32,
    /// v0.7.19 GAF-707 Accident-Detection: SimConnect-`Crashed`-
    /// System-Event setzt das Atomic. Wird beim Snapshot-Build in
    /// `snap.crashed` gelesen. `CrashReset` (= MSFS-UI Cut-Scene
    /// quittiert) loescht den raw Flag wieder, aber der aktive Flug
    /// behaelt seinen `accident_detected`-Latch in lib.rs unabhaengig
    /// davon bis Flight-End/Cleanup (Spec §Leitentscheidung 6).
    sim_crashed: AtomicBool,
    /// Last touchdown sample as seen on data definition #2. Updated
    /// asynchronously by SimConnect — we merge it into each emitted
    /// `SimSnapshot` so downstream consumers see a unified view.
    touchdown: Mutex<Option<Touchdown>>,
    /// User-driven SimVar/LVar inspector watchlist. UI mutates the
    /// vec via add_watch / remove_watch (which sets `dirty=true`),
    /// the worker re-registers definition #3 on the next tick.
    inspector: Mutex<InspectorState>,
    /// PMDG SDK live data, available only when a PMDG aircraft is
    /// loaded AND the user has set `EnableDataBroadcast=1` in the
    /// aircraft's options ini. Variant tells which PMDG family
    /// is currently parsed; the bytes are decoded at consume-time
    /// to the appropriate `Pmdg738Snapshot` / `Pmdg777XSnapshot`.
    /// `None` when no PMDG aircraft is loaded.
    /// Phase 5.2 — wired into the dispatch loop in this commit.
    pmdg: Mutex<PmdgSharedState>,
}

/// Convert a PMDG NG3 (737-specific) snapshot to the generic
/// `sim_core::PmdgState` shape. The FSM, activity log, and PIREP
/// code consume `PmdgState` so they don't have to branch on
/// 737 vs. 777 — this is the boundary that makes that work.
///
/// FMA-mode strings: 737 NG MCP shows the active mode via boolean
/// annunciator lights (one per mode — VNAV, LVL CHG, ALT HOLD,
/// VS, HDG SEL, LNAV, VOR/LOC, APP, SPEED, N1). We pick the
/// "most active" one in priority order matching what the real
/// FMA shows when multiple are momentarily active.
fn ng3_to_pmdg_state(s: &crate::pmdg::ng3::Pmdg738Snapshot) -> sim_core::PmdgState {
    use sim_core::PmdgState;

    // Speed-mode: FMA-priority order. SPD wins over N1 if both
    // (rare; usually only one). Real cockpit shows N1 during
    // takeoff, SPD during climb/cruise, etc.
    let fma_speed_mode = if s.fma.speed_n1 {
        "N1"
    } else if s.fma.speed {
        "SPD"
    } else {
        ""
    };
    // Roll-mode: LNAV wins over VOR/LOC over HDG SEL.
    let fma_roll_mode = if s.fma.lnav {
        "LNAV"
    } else if s.fma.vor_loc {
        "VOR/LOC"
    } else if s.fma.app {
        "APP"
    } else if s.fma.hdg_sel {
        "HDG SEL"
    } else {
        ""
    };
    // Pitch-mode: VNAV / LVL CHG / VS / ALT HOLD priority.
    let fma_pitch_mode = if s.fma.vnav {
        "VNAV"
    } else if s.fma.lvl_chg {
        "LVL CHG"
    } else if s.fma.alt_hold {
        "ALT HOLD"
    } else if s.fma.vs {
        "V/S"
    } else if s.fma.app {
        "G/S"
    } else {
        ""
    };

    PmdgState {
        variant_label: s.variant.label().to_string(),

        // MCP — None when blanked or unpowered.
        mcp_speed_raw: if s.mcp_speed_blanked || !s.mcp_powered {
            None
        } else {
            Some(s.mcp_speed_raw)
        },
        mcp_heading_deg: if s.mcp_powered {
            Some(s.mcp_heading_deg)
        } else {
            None
        },
        mcp_altitude_ft: if s.mcp_powered {
            Some(s.mcp_altitude_ft)
        } else {
            None
        },
        mcp_vs_fpm: if s.mcp_vs_blanked || !s.mcp_powered {
            None
        } else {
            Some(s.mcp_vs_fpm)
        },

        // FMA modes
        fma_speed_mode: fma_speed_mode.to_string(),
        fma_roll_mode: fma_roll_mode.to_string(),
        fma_pitch_mode: fma_pitch_mode.to_string(),
        at_armed: s.fma.at_armed,
        ap_engaged: s.fma.cmd_a || s.fma.cmd_b,
        fd_on: s.fma.fd_capt || s.fma.fd_fo,

        // FMC plan
        fmc_takeoff_flaps_deg: if s.fmc_takeoff_flaps_deg == 0 {
            None
        } else {
            Some(s.fmc_takeoff_flaps_deg)
        },
        fmc_landing_flaps_deg: if s.fmc_landing_flaps_deg == 0 {
            None
        } else {
            Some(s.fmc_landing_flaps_deg)
        },
        fmc_v1_kt: s.fmc_v_speeds.v1_kt,
        fmc_vr_kt: s.fmc_v_speeds.vr_kt,
        fmc_v2_kt: s.fmc_v_speeds.v2_kt,
        fmc_vref_kt: s.fmc_v_speeds.vref_kt,
        fmc_cruise_alt_ft: if s.fmc_cruise_alt_ft == 0 {
            None
        } else {
            Some(s.fmc_cruise_alt_ft)
        },
        fmc_distance_to_tod_nm: if s.fmc_distance_to_tod_nm < 0.0 {
            None
        } else {
            Some(s.fmc_distance_to_tod_nm)
        },
        fmc_distance_to_dest_nm: if s.fmc_distance_to_dest_nm < 0.0 {
            None
        } else {
            Some(s.fmc_distance_to_dest_nm)
        },
        fmc_flight_number: s.fmc_flight_number.clone(),
        fmc_perf_input_complete: s.fmc_perf_input_complete,

        // Controls
        flap_angle_deg: s.flap_angle_deg,
        flap_handle_label: s.flap_handle_label.to_string(),
        speedbrake_lever_pos: Some(s.speedbrake_lever_pos),
        autobrake_label: s.autobrake.label().to_string(),
        speedbrake_armed: s.speedbrake_armed,
        speedbrake_extended: s.speedbrake_extended,
        takeoff_config_warning: s.takeoff_config_warning,
        xpdr_mode_label: crate::pmdg::pmdg_xpdr_mode_label(s.xpdr_mode).to_string(),

        // NG3 doesn't have a dedicated `APURunning` bool in the
        // SDK header (777 does), but we have something better
        // than the standard SimVar APU_PCT_RPM heuristic:
        // `Pmdg738Snapshot::apu_running` derives from
        // (APU_Selector==ON && APU_EGTNeedle>350°C). That's a
        // PMDG-cockpit-aware signal. Surface it here so the
        // generic activity-log path uses it.
        apu_running: Some(s.apu_running),
        // Genuine NG3 SDK gaps — fields below don't exist in
        // PMDG_NG3_SDK.h at all. Leave None so downstream code
        // skips the matching activity-log entries silently.
        thrust_limit_mode: String::new(),
        ecl_complete: None,
        wheel_chocks_set: None,
        // ---- Cockpit overrides (Premium-First) ----
        light_landing: Some(s.light_landing),
        light_beacon: Some(s.light_beacon),
        light_strobe: Some(s.light_strobe),
        light_taxi: Some(s.light_taxi),
        light_nav: Some(s.light_nav),
        light_logo: Some(s.light_logo),
        light_wing: Some(s.light_wing),
        light_wheel_well: Some(s.light_wheel_well), // NG3-bonus
        wing_anti_ice: Some(s.wing_anti_ice),
        engine_anti_ice: Some(s.engine_anti_ice),
        pitot_heat: Some(s.pitot_heat),
        battery_master: Some(s.battery_master),
        parking_brake: Some(s.parking_brake_set),

        // ---- v0.16.10 (#Premium): deep-data carrier fields ----
        // Annunciator booleans are always `Some(..)` while a PMDG
        // aircraft is active — `Some(false)` = "light is off", a
        // real cockpit observation. Presence-gating happens one
        // level up (`snap.pmdg == None` ⇒ the premium override is
        // a no-op), so an absent SDK block never injects fake
        // `false` signals, and lit→off transitions stay
        // distinguishable from "no data".
        reverser_deployed: Some(s.reverser_deployed),
        master_caution: Some(s.master_caution),
        // The 737 NG has no MASTER WARNING light — the red master
        // on the glareshield IS the FIRE WARN light, so it maps to
        // the generic master_warning carrier.
        master_warning: Some(s.fire_warn),
        below_gs: Some(s.below_gs),
        cabin_altitude_warning: Some(s.cabin_altitude_warning),
        stab_out_of_trim: Some(s.stab_out_of_trim),
        // Tank order: [left, center, right] — already canonical kg
        // (WeightInKg handling lives in `Pmdg738Snapshot::from_raw`).
        fuel_per_tank_kg: Some(s.fuel_per_tank_kg.to_vec()),
        // Genuine NG3 SDK gaps: PMDG_NG3_SDK.h has no numeric
        // minimums field (only the BARO/RADIO selector switch
        // `EFIS_MinsSelBARO`) and no GND PROX annunciator (only
        // `GPWS_annunINOP`). None ⇒ the override keeps whatever
        // the generic telemetry provides.
        minimums_baro_ft: None,
        gnd_prox_warning: None,
    }
}

/// Convert a PMDG 777X snapshot to the generic `sim_core::PmdgState`
/// shape. 777 differs from NG3 in autoflight modes — instead of
/// CMD A/B engagement annunciators, the 777 has push-button
/// engagement with a single AP annunciator per side, and the
/// FMA modes are FLCH / HDG HOLD / VS_FPA instead of LVL CHG /
/// HDG SEL / VS. We map them to the closest generic equivalents.
fn x777_to_pmdg_state(s: &crate::pmdg::x777::Pmdg777XSnapshot) -> sim_core::PmdgState {
    use sim_core::PmdgState;

    // Speed-mode label. 777 doesn't have a separate "N1" annunciator
    // (uses FMC ThrustLimitMode for that). When AT is engaged + AP
    // is engaged, FMA usually shows the active sub-mode label.
    let fma_speed_mode = if s.fma.at { "SPD" } else { "" };
    // Roll-mode priority: APP > LOC > LNAV > HDG HOLD.
    let fma_roll_mode = if s.fma.app {
        "APP"
    } else if s.fma.loc {
        "LOC"
    } else if s.fma.lnav {
        "LNAV"
    } else if s.fma.hdg_hold {
        "HDG HOLD"
    } else {
        ""
    };
    // Pitch-mode: VNAV > FLCH > VS_FPA > ALT_HOLD.
    let fma_pitch_mode = if s.fma.vnav {
        "VNAV"
    } else if s.fma.flch {
        "FLCH"
    } else if s.fma.alt_hold {
        "ALT HOLD"
    } else if s.fma.vs_fpa {
        if s.mcp_dial_in_fpa_mode {
            "FPA"
        } else {
            "V/S"
        }
    } else {
        ""
    };

    // Convert 777 flap handle to an approximate degree value for
    // the generic `flap_angle_deg` field. The actual flap surface
    // angle isn't in the SDK; we use the canonical handle-to-
    // degree mapping (0=UP, 1=1°, 2=5°, 3=15°, 4=20°, 5=25°, 6=30°)
    // which IS what the cockpit FLAP indicator shows.
    let flap_angle_deg = match s.flap_handle_pos {
        0 => 0.0,
        1 => 1.0,
        2 => 5.0,
        3 => 15.0,
        4 => 20.0,
        5 => 25.0,
        6 => 30.0,
        _ => 0.0,
    };

    PmdgState {
        variant_label: s.model.label().to_string(),

        mcp_speed_raw: if s.mcp_speed_blanked {
            None
        } else {
            Some(s.mcp_speed_raw)
        },
        mcp_heading_deg: Some(s.mcp_heading_deg),
        mcp_altitude_ft: Some(s.mcp_altitude_ft),
        mcp_vs_fpm: if s.mcp_vs_blanked {
            None
        } else {
            Some(s.mcp_vs_fpm)
        },

        fma_speed_mode: fma_speed_mode.to_string(),
        fma_roll_mode: fma_roll_mode.to_string(),
        fma_pitch_mode: fma_pitch_mode.to_string(),
        at_armed: s.fma.at,
        ap_engaged: s.fma.ap_capt || s.fma.ap_fo,
        fd_on: s.fma.fd_capt || s.fma.fd_fo,

        fmc_takeoff_flaps_deg: if s.fmc_takeoff_flaps_deg == 0 {
            None
        } else {
            Some(s.fmc_takeoff_flaps_deg)
        },
        fmc_landing_flaps_deg: if s.fmc_landing_flaps_deg == 0 {
            None
        } else {
            Some(s.fmc_landing_flaps_deg)
        },
        fmc_v1_kt: s.fmc_v_speeds.v1_kt,
        fmc_vr_kt: s.fmc_v_speeds.vr_kt,
        fmc_v2_kt: s.fmc_v_speeds.v2_kt,
        fmc_vref_kt: s.fmc_v_speeds.vref_kt,
        fmc_cruise_alt_ft: if s.fmc_cruise_alt_ft == 0 {
            None
        } else {
            Some(s.fmc_cruise_alt_ft)
        },
        fmc_distance_to_tod_nm: if s.fmc_distance_to_tod_nm < 0.0 {
            None
        } else {
            Some(s.fmc_distance_to_tod_nm)
        },
        fmc_distance_to_dest_nm: if s.fmc_distance_to_dest_nm < 0.0 {
            None
        } else {
            Some(s.fmc_distance_to_dest_nm)
        },
        fmc_flight_number: s.fmc_flight_number.clone(),
        fmc_perf_input_complete: s.fmc_perf_input_complete,

        flap_angle_deg,
        flap_handle_label: s.flap_handle_label.to_string(),
        // 777 SDK gives the lever as 0..100 — normalise to 0.0..1.0.
        speedbrake_lever_pos: Some(f32::from(s.speedbrake_lever_pos) / 100.0),
        autobrake_label: s.autobrake.label().to_string(),
        speedbrake_armed: s.speedbrake_armed,
        speedbrake_extended: s.speedbrake_extended,
        // 777 doesn't have a "TAKEOFF CONFIG" annunciator the
        // same way NG3 does — closest equivalents are GPWS
        // bottom warnings during ground-roll, but those aren't
        // a perfect match. Leave `false` for now; if needed,
        // we can derive from EICAS messages later.
        takeoff_config_warning: false,
        xpdr_mode_label: crate::pmdg::pmdg_xpdr_mode_label(s.xpdr_mode).to_string(),

        // 777-specific extras (Phase 5.5b — wider integration).
        thrust_limit_mode: crate::pmdg::x777::x777_thrust_limit_label(s.fmc_thrust_limit_mode)
            .to_string(),
        ecl_complete: Some(s.ecl_complete),
        apu_running: Some(s.apu_running),
        wheel_chocks_set: Some(s.wheel_chocks_set),

        // ---- Cockpit overrides (Premium-First, v0.2.3) ----
        // Same overrides as NG3, but the 777 SDK has no dedicated
        // wheel-well light bool — leave None there.
        light_landing: Some(s.light_landing),
        light_beacon: Some(s.light_beacon),
        light_strobe: Some(s.light_strobe),
        light_taxi: Some(s.light_taxi),
        light_nav: Some(s.light_nav),
        light_logo: Some(s.light_logo),
        light_wing: Some(s.light_wing),
        light_wheel_well: None,
        wing_anti_ice: Some(s.wing_anti_ice),
        engine_anti_ice: Some(s.engine_anti_ice),
        pitot_heat: Some(s.pitot_heat),
        battery_master: Some(s.battery_master),
        parking_brake: Some(s.parking_brake_set),

        // ---- v0.16.10 (#Premium): deep-data carrier fields ----
        // Booleans are always `Some(..)` while a PMDG 777 is active
        // (Some(false) = light off) — see the NG3 mapper for the
        // presence-gating rationale.
        //
        // PMDG_777X_SDK.h exposes NO thrust-reverser annunciator or
        // state anywhere in PMDG_777X_Data (verified against the
        // v3.x header: the only "reverse" hit is the CDU reverse-
        // video display flag). None ⇒ the generic SimVar-based
        // reverser detection stays in charge.
        reverser_deployed: None,
        master_caution: Some(s.master_caution),
        master_warning: Some(s.master_warning),
        // More genuine 777X SDK gaps: no BELOW G/S annunciator
        // (`GPWS_GSInhibit_Sw` is a switch, not a light), no CABIN
        // ALTITUDE annunciator, no STAB OUT OF TRIM annunciator.
        below_gs: None,
        cabin_altitude_warning: None,
        stab_out_of_trim: None,
        // Tank order: [left, center, right, aux] — already canonical
        // kg (WeightInKg handling in `Pmdg777XSnapshot::from_raw`).
        fuel_per_tank_kg: Some(s.fuel_per_tank_kg.to_vec()),
        // Captain-side EFIS baro minimums (DA/MDA); None until
        // dialed — the `EFIS_BaroMinimumsSet[0]` gate lives in
        // `Pmdg777XSnapshot::from_raw`.
        minimums_baro_ft: s.minimums_baro_ft.map(f64::from),
        // GPWS GND PROX: top or bottom annunciator lit.
        gnd_prox_warning: Some(s.gpws_top_warn || s.gpws_bottom_warn),
    }
}

/// Public PMDG SDK status — exposed via `MsfsAdapter::pmdg_status()`
/// so the UI can show "SDK enabled?" hints, log warnings, etc.
#[derive(Debug, Clone)]
pub struct PmdgStatus {
    /// Detected PMDG variant from the most recent AircraftLoaded.
    pub variant: Option<crate::pmdg::PmdgVariant>,
    /// True once `RequestClientData` has succeeded for the variant.
    pub subscribed: bool,
    /// True once at least one ClientData packet has arrived (i.e.
    /// the SDK is genuinely active and broadcasting).
    pub ever_received: bool,
    /// Seconds since the last ClientData packet. `u64::MAX` when
    /// no packet has ever arrived.
    pub stale_secs: u64,
}

impl PmdgStatus {
    /// True when PMDG aircraft is loaded but no data is flowing.
    /// Drives the "SDK probably not enabled" hint in the UI.
    pub fn looks_like_sdk_disabled(&self) -> bool {
        self.variant.is_some() && self.subscribed && !self.ever_received && self.stale_secs > 5
    }
}

/// Tracking state for the PMDG SDK ClientData subscription.
#[derive(Debug, Default)]
struct PmdgSharedState {
    /// Detected PMDG variant from the most recent AircraftLoaded
    /// event. `None` if no PMDG aircraft is loaded.
    variant: Option<crate::pmdg::PmdgVariant>,
    /// True once we've successfully called
    /// `RequestClientData` for the current variant. Cleared on
    /// aircraft change so the next dispatch re-subscribes.
    subscribed: bool,
    /// Most recent NG3 raw data bytes. Stored as the raw 916-byte
    /// block; decoded on demand via `Pmdg738Snapshot::from_raw()`.
    /// `None` until the first frame arrives.
    ng3_raw: Option<Box<crate::pmdg::ng3::Pmdg738RawData>>,
    /// Most recent 777X raw data bytes (684-byte block; decoded
    /// on demand via `Pmdg777XSnapshot::from_raw()`).
    x777_raw: Option<Box<crate::pmdg::x777::Pmdg777XRawData>>,
    /// Timestamp of the last PMDG ClientData packet. Used by the
    /// "SDK appears not enabled" UI hint — if we know the variant
    /// (= aircraft loaded) but no packets for >5 s, the user
    /// probably hasn't enabled the SDK.
    last_packet_at: Option<std::time::Instant>,
}

impl Default for MsfsAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl MsfsAdapter {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(ConnectionState::Disconnected),
                snapshot: Mutex::new(None),
                last_error: Mutex::new(None),
                szenerie: Mutex::new(sim_core::szenerie::Auftragsbuch::neu()),
                szenerie_offen: AtomicBool::new(false),
                szenerie_diagnose: Mutex::new(SzenerieDiagnose::default()),
                sim_kennung: Mutex::new(None),
                sim_paused: AtomicBool::new(false),
                sim_unecht_tiefe: AtomicI32::new(0),
                sim_crashed: AtomicBool::new(false),
                touchdown: Mutex::new(None),
                inspector: Mutex::new(InspectorState::default()),
                pmdg: Mutex::new(PmdgSharedState::default()),
            }),
            worker: None,
            stop: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Start the worker thread. Idempotent: a second call is a no-op
    /// while a worker is already running.
    pub fn start(&mut self, kind: SimKind) {
        if self.worker.is_some() {
            return;
        }
        if !kind.is_msfs() {
            *self.shared.state.lock() = ConnectionState::Disconnected;
            return;
        }
        self.stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&self.shared);
        let stop = Arc::clone(&self.stop);
        *shared.state.lock() = ConnectionState::Connecting;
        *shared.last_error.lock() = None;
        tracing::info!(?kind, "MSFS raw adapter started");
        let handle = thread::Builder::new()
            .name("sim-msfs-worker".into())
            .spawn(move || worker_loop(shared, stop, kind))
            .expect("could not spawn sim-msfs worker thread");
        self.worker = Some(handle);
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.worker.take() {
            // Give the worker a moment to wind down cleanly. We don't
            // join indefinitely — SimConnect_Close inside the worker
            // can hang if MSFS itself is gone.
            let _ = h.join();
        }
        *self.shared.state.lock() = ConnectionState::Disconnected;
        tracing::info!("MSFS raw adapter stopped");
    }

    pub fn state(&self) -> ConnectionState {
        *self.shared.state.lock()
    }

    pub fn snapshot(&self) -> Option<SimSnapshot> {
        let mut snap = self.shared.snapshot.lock().clone()?;
        // Merge PMDG SDK data when available (Phase 5.4 + 5.4b).
        // The standard SimVar telemetry fills the SimSnapshot's
        // main body; PMDG fills the optional `pmdg` field with
        // cockpit-exact values. NG3 wins if both are somehow
        // present (would be a bug — only one PMDG aircraft can
        // be loaded at a time — but defensive).
        if let Some(ng3_state) = self.pmdg_ng3_snapshot() {
            snap.pmdg = Some(ng3_to_pmdg_state(&ng3_state));
        } else if let Some(x777_state) = self.pmdg_x777_snapshot() {
            snap.pmdg = Some(x777_to_pmdg_state(&x777_state));
        }

        // ---- Premium-First Override (v0.2.3) ----
        // Where PMDG provides a cockpit-exact value for a field that
        // the Standard SimVar telemetry also fills, prefer the PMDG
        // value. This keeps every downstream consumer (FSM,
        // activity-log, PIREP fields, UI) on the most accurate
        // signal we have, without each consumer needing to know
        // about PMDG. The override is silent when PMDG is absent
        // or the specific field isn't supported (NG3 doesn't have
        // wheel_well in the override path? actually it does; 777
        // doesn't — `light_wheel_well` is only set when present).
        if let Some(pmdg) = snap.pmdg.as_ref() {
            if let Some(v) = pmdg.light_landing {
                snap.light_landing = Some(v);
            }
            if let Some(v) = pmdg.light_beacon {
                snap.light_beacon = Some(v);
            }
            if let Some(v) = pmdg.light_strobe {
                snap.light_strobe = Some(v);
            }
            if let Some(v) = pmdg.light_taxi {
                snap.light_taxi = Some(v);
            }
            if let Some(v) = pmdg.light_nav {
                snap.light_nav = Some(v);
            }
            if let Some(v) = pmdg.light_logo {
                snap.light_logo = Some(v);
            }
            // SimSnapshot has no top-level light_wing — only PMDG
            // exposes it; downstream reads it from snap.pmdg.
            if let Some(v) = pmdg.wing_anti_ice {
                snap.wing_anti_ice = Some(v);
            }
            if let Some(v) = pmdg.engine_anti_ice {
                snap.engine_anti_ice = Some(v);
            }
            if let Some(v) = pmdg.pitot_heat {
                snap.pitot_heat = Some(v);
            }
            if let Some(v) = pmdg.battery_master {
                snap.battery_master = Some(v);
            }
            if let Some(v) = pmdg.parking_brake {
                snap.parking_brake = v;
            }
            // APU: SimSnapshot stores `apu_switch` (selector ON?)
            // and `apu_pct_rpm` (rising/running heuristic). PMDG's
            // `apu_running` is the cockpit-truth boolean — surface
            // it via apu_switch so the FSM picks it up.
            if let Some(v) = pmdg.apu_running {
                snap.apu_switch = Some(v);
            }
            // Spoilers/autobrake: PMDG has cockpit-exact values for
            // `speedbrake_armed` and `autobrake_label`. Mirror them
            // into the standard fields so generic consumers benefit.
            snap.spoilers_armed = Some(pmdg.speedbrake_armed);
            // v0.2.4: prefer PMDG-derived lever position too — the
            // Standard SimVar `SPOILERS HANDLE POSITION` jitters
            // around the ARMED detent, causing flicker entries
            // ("DEPLOYED 76% / RETRACTED") in the activity log.
            // PMDG gives a stable lever value (NG3 synthesised
            // from the bools, 777 from the raw 0..100 lever).
            if let Some(v) = pmdg.speedbrake_lever_pos {
                snap.spoilers_handle_position = Some(v);
            }
            if !pmdg.autobrake_label.is_empty() {
                snap.autobrake = Some(pmdg.autobrake_label.clone());
            }
            // v0.3.0: surface PMDG light_wing / light_wheel_well /
            // xpdr_mode_label / takeoff_config_warning via the
            // top-level SimSnapshot fields (the same fields X-Plane
            // also fills). Lets the generic activity-log code share
            // a single path across simulators.
            if let Some(v) = pmdg.light_wing {
                snap.light_wing = Some(v);
            }
            if let Some(v) = pmdg.light_wheel_well {
                snap.light_wheel_well = Some(v);
            }
            if !pmdg.xpdr_mode_label.is_empty() {
                snap.xpdr_mode_label = Some(pmdg.xpdr_mode_label.clone());
            }
            // PMDG NG3 has a real takeoff-config bit; 777 leaves it
            // false (no equivalent annunciator). Surface either way
            // so the X-Plane / MSFS activity-log path can fire on it.
            snap.takeoff_config_warning = Some(pmdg.takeoff_config_warning);
        }
        // v0.16.7: AP master + A/THR from the PMDG annunciators (737
        // CMD A/B + A/T ARM, 777 AP L/R + AT), OR'd with the standard
        // SimVars. Data-audit 2026-06-11: the standard `AUTOPILOT
        // MASTER` SimVar reads permanently false on PMDG 737/777, so
        // the "Autopilot ENGAGED/OFF" / "A/THR" activity-log lines
        // never fired for those pilots. Presence-gated no-op when
        // `snap.pmdg` is None — semantics + tests live in
        // `SimSnapshot::apply_pmdg_autoflight_override` (sim-core,
        // cross-platform tested; this adapter is Windows-only).
        snap.apply_pmdg_autoflight_override();
        // v0.16.10 (#Premium): sibling override for the deep-data
        // fields (FMA labels, warn annunciators, reverser, per-tank
        // fuel, V-speeds, baro minimums, ground spoilers). Same
        // presence-gating — no-op when `snap.pmdg` is None.
        // Semantics + tests live in
        // `SimSnapshot::apply_pmdg_premium_override` (sim-core,
        // cross-platform tested; this adapter is Windows-only).
        snap.apply_pmdg_premium_override();

        Some(snap)
    }

    /// Latest PMDG 777X cockpit state, if a PMDG 777 is loaded
    /// AND the SDK is enabled in `777X_Options.ini`. Same on-
    /// demand decoding semantics as the NG3 variant.
    pub fn pmdg_x777_snapshot(&self) -> Option<crate::pmdg::x777::Pmdg777XSnapshot> {
        let g = self.shared.pmdg.lock();
        g.x777_raw
            .as_ref()
            .map(|raw| crate::pmdg::x777::Pmdg777XSnapshot::from_raw(raw))
    }

    /// Latest PMDG NG3 cockpit state, if a PMDG 737 is loaded AND
    /// the SDK is enabled in `737NG3_Options.ini`. Returns the
    /// "useful subset" view (`Pmdg738Snapshot`), not the raw 916-
    /// byte struct — so callers don't have to know about layout.
    /// `None` when no PMDG NG3 is loaded or no data has arrived yet.
    pub fn pmdg_ng3_snapshot(&self) -> Option<crate::pmdg::ng3::Pmdg738Snapshot> {
        let g = self.shared.pmdg.lock();
        g.ng3_raw
            .as_ref()
            .map(|raw| crate::pmdg::ng3::Pmdg738Snapshot::from_raw(raw))
    }

    /// PMDG SDK status report — what variant is loaded (if any),
    /// whether we've subscribed, and how stale the most recent
    /// data is. Drives the Settings-tab "SDK enabled?" hint.
    pub fn pmdg_status(&self) -> PmdgStatus {
        let g = self.shared.pmdg.lock();
        let stale_secs = g
            .last_packet_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(u64::MAX);
        PmdgStatus {
            variant: g.variant,
            subscribed: g.subscribed,
            ever_received: g.ng3_raw.is_some() || g.x777_raw.is_some(),
            stale_secs,
        }
    }

    /// Force-clear the cached snapshot + touchdown so the next read
    /// returns `None` until SimConnect delivers a fresh frame. Used by
    /// the UI's "Re-check sim position" button when the pilot suspects
    /// the cached lat/lon is stale (e.g. flight changed in MSFS but
    /// our 5 s stale-timeout hasn't fired because SimConnect kept
    /// trickling data through the pause). State is downgraded to
    /// Connecting so the UI shows "waiting for sim position …" until
    /// the next real packet lands.
    pub fn clear_snapshot(&self) {
        *self.shared.snapshot.lock() = None;
        *self.shared.touchdown.lock() = None;
        // PMDG raw data is part of the same "stale snapshot"
        // problem — clear it on manual re-sync too. Variant
        // stays (we still know what aircraft is loaded), but
        // we clear `subscribed=false` so the next dispatch
        // re-subscribes and gets a fresh data block.
        {
            let mut g = self.shared.pmdg.lock();
            g.ng3_raw = None;
            g.x777_raw = None;
            g.subscribed = false;
            g.last_packet_at = None;
        }
        *self.shared.state.lock() = ConnectionState::Connecting;
        tracing::info!("MSFS snapshot cleared by user (force-resync)");
    }

    /// v1.7.8 — die Szenerie-Auskunft fuer einen Flughafen anfordern.
    ///
    /// Der Aufruf hinterlegt nur den Wunsch; die Anfrage stellt der
    /// Verbindungsfaden beim naechsten Durchlauf, und die Antwort kommt
    /// asynchron. `szenerie()` liefert sie, sobald sie vollstaendig ist.
    ///
    /// Gedacht fuer den Anflug: Dann ist das Ziel bekannt, es ist Zeit
    /// im Ueberfluss, und beim Aufsetzen liegt die Auskunft bereit.
    /// Beim Aufsetzen anzufordern waere zu spaet — die Antwort kommt
    /// stueckweise ueber mehrere Durchlaeufe.
    /// ⚠ Mehrfach aufzurufen ist ausdruecklich richtig. Bis v1.7.13
    /// stand hier `if wunsch == icao { return }` — damit war ein Ziel,
    /// das am Gate nicht antwortete, fuer den Rest des Fluges tot. Das
    /// Buch entscheidet jetzt, ob und wann wirklich gefragt wird.
    pub fn szenerie_anfordern(&self, icao: &str) {
        self.szenerie_anfordern_mit_rang(icao, 0);
    }

    /// Wie `szenerie_anfordern`, aber mit Rangfolge — kleiner heisst
    /// frueher dran. Das Ausweichziel gehoert vor das geplante.
    pub fn szenerie_anfordern_mit_rang(&self, icao: &str, rang: u8) {
        let icao = icao.trim().to_ascii_uppercase();
        if icao.is_empty() {
            return;
        }
        self.shared.szenerie.lock().wunsch_mit_rang(&icao, rang);
        self.shared.szenerie_offen.store(true, Ordering::Relaxed);
    }

    /// Zustand EINES Platzes als Kurzwort.
    pub fn szenerie_diagnose_fuer(&self, icao: &str) -> String {
        self.shared.szenerie.lock().diagnose(icao)
    }

    /// Einen neuen Versuchsvorrat oeffnen (Eintritt in den Anflug).
    pub fn szenerie_neues_versuchsfenster(&self) -> usize {
        self.shared.szenerie.lock().neues_versuchsfenster()
    }

    /// Die Raenge der derzeit gueltigen Ziele setzen.
    pub fn szenerie_raenge_setzen(&self, ziele: &[(String, u8)]) {
        self.shared.szenerie.lock().raenge_setzen(ziele);
    }

    /// Das Auftragsbuch leeren — neuer Flug.
    ///
    /// ⚠ Der Anfragezustand gehoert dem Flug und der Verbindung, nicht
    /// dem Adapter. Ohne diesen Schnitt ueberdauern verbrauchte
    /// Versuche, dauerhafte Ablehnungen und die Szenerie des vorigen
    /// Fluges (QS-Befund 2, dritte Runde).
    pub fn szenerie_zuruecksetzen(&self) {
        self.shared.szenerie.lock().zuruecksetzen();
    }

    /// Wie weit die Szenerie-Abfrage gekommen ist.
    pub fn szenerie_diagnose(&self) -> SzenerieDiagnose {
        self.shared.szenerie_diagnose.lock().clone()
    }

    /// Womit sich der Simulator gemeldet hat (Name + Version).
    pub fn sim_kennung(&self) -> Option<String> {
        self.shared.sim_kennung.lock().clone()
    }

    /// Die zuletzt vollstaendig gelieferte Szenerie-Auskunft.
    ///
    /// `None` heisst: nicht angefordert, noch unterwegs, oder der
    /// Simulator kennt den Platz nicht. In allen drei Faellen bleibt es
    /// bei den Navdaten.
    ///
    /// ⚠ NUR zu diesem Platz. Kein Rueckfall auf "irgendeine" — am
    /// 01.09.2026 lag beim Aufsetzen in Sevilla die Szenerie Frankfurts
    /// vor, und ein bequemer Rueckfall haette sie benutzt.
    pub fn szenerie_fuer(&self, icao: &str) -> Option<sim_core::szenerie::SzenerieFlughafen> {
        self.shared.szenerie.lock().auskunft(icao).cloned()
    }

    /// Wie oft dieser Platz schon gefragt wurde — fuer die Diagnose.
    pub fn szenerie_versuche(&self, icao: &str) -> u8 {
        self.shared.szenerie.lock().versuche(icao)
    }

    pub fn last_error(&self) -> Option<String> {
        self.shared.last_error.lock().clone()
    }

    // ---- Inspector (Phase B) ----

    /// Add a SimVar/LVar to the live inspector watchlist. Returns the
    /// stable id assigned to this entry — pass it to `remove_watch`.
    /// Re-registration of SimConnect data definition #3 happens on the
    /// next worker tick (asynchronous, sub-second).
    pub fn add_watch(&self, name: String, unit: String, kind: WatchKind) -> u32 {
        let mut g = self.shared.inspector.lock();
        g.add(name, unit, kind)
    }

    pub fn remove_watch(&self, id: u32) {
        let mut g = self.shared.inspector.lock();
        g.remove(id);
    }

    /// Snapshot of the current watchlist (cloning so the caller doesn't
    /// hold the inspector mutex). Each entry carries its latest value
    /// — `value: None` means we haven't received a tick yet.
    pub fn watches(&self) -> Vec<InspectorWatch> {
        self.shared.inspector.lock().watches.clone()
    }
}

impl Drop for MsfsAdapter {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---- Worker loop ----

fn worker_loop(shared: Arc<Shared>, stop: Arc<AtomicBool>, kind: SimKind) {
    // Outer reconnect loop. SimConnect_Open returns E_FAIL while MSFS
    // isn't running; we simply retry every 2s until it's up.
    while !stop.load(Ordering::Relaxed) {
        match Connection::open("AeroACARS") {
            Ok(mut conn) => {
                tracing::info!("SimConnect_Open succeeded — registering data definition");
                // ⚠ Neue Verbindung heisst neuer Kontext. Der Simulator
                // kann neu gestartet, ein anderer sein (MSFS 2020 gegen
                // 2024) oder eine andere Szenerie geladen haben. Ohne
                // diesen Schnitt gaelte die Szenerie der vorigen
                // Verbindung als „geliefert" und wuerde nie erneut
                // angefordert; verbrauchte Versuche und dauerhafte
                // Ablehnungen ueberdauerten ebenfalls (QS-Befund 2,
                // dritte Runde).
                // ⚠ `verbindung_zuruecksetzen`, nicht `zuruecksetzen`:
                // Hier wird die Felddefinition gleich neu registriert,
                // also darf auch ein alter Definitionsfehler fallen. Bei
                // einem blossen Flugwechsel waere das falsch.
                shared.szenerie.lock().verbindung_zuruecksetzen();
                if let Err(e) = conn.register_telemetry() {
                    set_error(&shared, format!("RegisterDataDefinition failed: {e}"));
                    tracing::error!(error = %e, "register_telemetry failed");
                    drop(conn);
                    sleep_or_stop(&stop, Duration::from_secs(2));
                    continue;
                }
                // Touchdown registration is best-effort: a failure
                // there should NOT take down live telemetry. Log and
                // proceed.
                if let Err(e) = conn.register_touchdown() {
                    tracing::warn!(error = %e, "register_touchdown failed — touchdown values will stay None");
                }
                // v1.7.8 — die Facility-Definition einmal je Verbindung
                // registrieren. Ohne diesen Aufruf gaebe es die
                // Definition nicht, und jede Anfrage liefe ins Leere —
                // still, denn `RequestFacilityData` scheitert dann nicht,
                // es kommt nur nie eine Antwort.
                //
                // Ein Fehlschlag ist hier kein Grund, die Verbindung
                // aufzugeben: Die Navdaten bleiben der Rueckfall, und
                // Telemetrie und Aufsetzprobe sind davon unberuehrt.
                if let Err(e) = conn.register_facility() {
                    tracing::warn!(
                        error = %e,
                        "register_facility fehlgeschlagen — Bahndaten kommen weiter aus den Navdaten"
                    );
                    // ⚠ Und das Buch MUSS es erfahren.
                    //
                    // Vorher wurde hier nur gewarnt: Die Definition war
                    // nachweislich nicht registriert, das Buch wusste
                    // nichts davon und stellte munter Facility-Anfragen.
                    // Der harte Riegel griff nur bei den SPAETEREN,
                    // asynchronen Ausnahmen — der synchrone Fehlschlag,
                    // der den Weg genauso sicher schliesst, lief durch
                    // (QS-Befund 3, vierte Runde).
                    shared
                        .szenerie
                        .lock()
                        .definition_abgelehnt("register_facility".into(), e.clone());
                    *shared.szenerie_diagnose.lock() = SzenerieDiagnose::Abgelehnt(e);
                }
                if let Err(e) = conn.request_data_per_second() {
                    set_error(&shared, format!("RequestDataOnSimObject failed: {e}"));
                    tracing::error!(error = %e, "request_data_per_second failed");
                    drop(conn);
                    sleep_or_stop(&stop, Duration::from_secs(2));
                    continue;
                }
                if let Err(e) = conn.request_touchdown_per_second() {
                    tracing::warn!(error = %e, "request_touchdown_per_second failed — touchdown values will stay None");
                }
                // PMDG SDK preflight (Phase 5.2/5.3): subscribe to
                // AircraftLoaded so the dispatch loop can detect
                // PMDG variants. This is best-effort — if it fails
                // we just lose PMDG-specific data, the standard
                // telemetry continues to work.
                if let Err(e) = conn.subscribe_aircraft_loaded() {
                    tracing::warn!(
                        error = %e,
                        "AircraftLoaded subscribe failed — PMDG variant detection disabled"
                    );
                }
                run_dispatch(&shared, &stop, &mut conn, kind);
                // run_dispatch only returns when stop is signalled or
                // the connection has gone stale. Either way, drop and
                // try again at the top of the loop.
                //
                // CRITICAL: clear the cached snapshot + touchdown so a
                // post-reconnect read can't return stale data from the
                // pre-disconnect session. Without this, a pilot who
                // loaded MSFS at the default airport (KSEA), then
                // changed the flight to a remote airport (SCEL),
                // would see a phantom "3142.5 nm from SCEL" check
                // failure because our cached snapshot still showed
                // the old KSEA position from before the load. Live
                // bug 2026-05-03. State stays "Disconnected" until
                // the next snapshot lands.
                *shared.snapshot.lock() = None;
                *shared.touchdown.lock() = None;
                // PMDG state too — variant + raw + subscribed flag
                // all reset so the next dispatch session re-detects
                // and re-subscribes from scratch.
                *shared.pmdg.lock() = PmdgSharedState::default();
                *shared.state.lock() = ConnectionState::Connecting;
            }
            Err(e) => {
                let msg = format!("SimConnect_Open failed: {e}");
                set_error(&shared, msg);
                *shared.state.lock() = ConnectionState::Connecting;
            }
        }
        sleep_or_stop(&stop, Duration::from_secs(2));
    }
    *shared.state.lock() = ConnectionState::Disconnected;
    *shared.snapshot.lock() = None;
    *shared.touchdown.lock() = None;
    *shared.pmdg.lock() = PmdgSharedState::default();
}

fn run_dispatch(
    shared: &Arc<Shared>,
    stop: &Arc<AtomicBool>,
    conn: &mut Connection,
    kind: SimKind,
) {
    let mut last_data = Instant::now();
    let mut got_first = false;
    let simulator = kind.as_simulator();
    // Force inspector re-registration after a reconnect — the new
    // SimConnect handle starts with an empty definition table even
    // if the user already populated the watchlist before the drop.
    if !shared.inspector.lock().watches.is_empty() {
        shared.inspector.lock().dirty = true;
    }

    // v1.7.14 — die offenen Facility-Lieferungen, nach Anfragekennung
    // getrennt.
    //
    // ⚠ AUSSERHALB der Tick-Schleife. Eine Lieferung kommt stueckweise
    // und kann sich ueber mehrere Durchlaeufe ziehen; wuerde sie je Tick
    // neu angelegt, ginge der Anfang verloren.
    //
    // Getrennt nach Kennung, weil sich sonst eine nach der Wartezeit
    // eintreffende Antwort mit der laufenden vermischt — die drei
    // Rollweglisten haengen ueber Indizes zusammen, und zwei Lieferungen
    // in denselben Listen verschieben jede Kante. Siehe `Lieferungen`.
    let mut facility_lieferungen = facility::Lieferungen::neu();
    // Paketkennung → Auftragskennung, damit eine spaetere Ausnahme dem
    // richtigen Platz zugeordnet werden kann. Siehe `auftrag_zu_paket`.
    let mut facility_pakete: Vec<(u32, u32)> = Vec::new();

    while !stop.load(Ordering::Relaxed) {
        // Re-register the inspector watchlist whenever the UI has
        // mutated it. The dirty flag avoids hot-looping the
        // SimConnect call in the steady state.
        let needs_inspector_register = {
            let g = shared.inspector.lock();
            g.dirty
        };
        if needs_inspector_register {
            let watches = {
                // Fresh registration attempt — clear stale errors so a
                // name the pilot just corrected (or a transient
                // SimConnect hiccup) gets a clean slate instead of
                // showing yesterday's exception forever.
                let mut g = shared.inspector.lock();
                g.clear_errors();
                g.watches.clone()
            };
            match conn.register_inspector(&watches) {
                Ok(()) => {
                    if !watches.is_empty() {
                        if let Err(e) = conn.request_inspector_per_second() {
                            tracing::warn!(error = %e, "request_inspector failed");
                        }
                    }
                    shared.inspector.lock().dirty = false;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "register_inspector failed; will retry");
                }
            }
        }

        // v1.7.14 — die offenen Lieferungen, nach Anfragekennung
        // getrennt. Sie liegen in `facility.rs`, weil DIESER Block hinter
        // `cfg(target_os = "windows")` steht und hier nichts pruefbar ist.
        // Getrennt, weil sich sonst eine verspaetete Antwort mit der
        // laufenden vermischt — siehe `Lieferungen`.
        //
        // v1.7.8 — eine ausstehende Szenerie-Anfrage stellen.
        //
        // Erst hier, im Faden mit dem Verbindungsgriff: `szenerie_anfordern`
        // laeuft im Aufrufer-Faden und darf SimConnect nicht anfassen.
        //
        // ⚠ Seit v1.7.14 bei JEDEM Durchlauf, nicht nur wenn das Flag
        // gesetzt ist. Das Flag sagte "jemand hat etwas angemeldet";
        // eine Wiederholung nach ausgebliebener Antwort haette es nie
        // ausgeloest. Das Buch gibt von sich aus nur dann einen Auftrag
        // heraus, wenn wirklich einer faellig ist — es ist ein Vergleich
        // je Durchlauf.
        shared.szenerie_offen.store(false, Ordering::Relaxed);
        {
            let jetzt_ms = chrono::Utc::now().timestamp_millis();
            let faellig = shared.szenerie.lock().naechster(jetzt_ms);
            if let Some(icao) = faellig {
                // ⚠ EIGENE Kennung je VERSUCH. Mit einer festen Kennung
                // fuer alle Anfragen liesse sich eine nach der Wartezeit
                // eintreffende Antwort nicht mehr von der laufenden
                // unterscheiden — sie wuerde dem falschen Platz
                // zugeschlagen, und ueberlappende Bloecke koennten sich
                // sogar vermischen. Das SDK gibt die clientdefinierte
                // Kennung in JEDER Nachricht zurueck, genau dafuer.
                let auftrag_id = shared.szenerie.lock().gestellt(&icao, jetzt_ms);
                let request_id = FACILITY_REQUEST_BASE + auftrag_id;
                match conn.request_facility(&icao, request_id) {
                    Ok(paket) => {
                        *shared.szenerie_diagnose.lock() = SzenerieDiagnose::Angefordert;
                        facility_lieferungen.eroeffnen(request_id);
                        if let Some(send_id) = paket {
                            facility_pakete.push((send_id, auftrag_id));
                            while facility_pakete.len() > facility::PAKETE_GEDAECHTNIS {
                                facility_pakete.remove(0);
                            }
                        }
                        tracing::info!(
                            %icao,
                            request_id,
                            versuch = shared.szenerie.lock().versuche(&icao),
                            "Szenerie-Auskunft angefordert"
                        );
                    }
                    Err(e) => {
                        // Nicht erneut versuchen: Ein Platz, den der
                        // Simulator nicht kennt, wird beim naechsten
                        // Durchlauf auch nicht bekannt. Die Navdaten
                        // bleiben, und das ist der richtige Rueckfall.
                        //
                        // ⚠ Aber festhalten, WARUM. Bis v1.7.9 stand am
                        // Flug nur "navdaten", und eine Ablehnung sah
                        // genauso aus wie "nie gefragt".
                        *shared.szenerie_diagnose.lock() =
                            SzenerieDiagnose::Abgelehnt(e.to_string());
                        shared
                            .szenerie
                            .lock()
                            .abgelehnt_zu_kennung(auftrag_id, e.to_string());
                        tracing::warn!(%icao, error = %e, "Szenerie-Anfrage abgelehnt");
                    }
                }
            }
        }

        // Drain whatever messages SimConnect has queued for us.
        loop {
            match conn.get_next_dispatch() {
                Ok(None) => break, // queue empty
                Ok(Some(DispatchMsg::Open { kennung })) => {
                    tracing::info!(%kennung, "SimConnect_RECV_OPEN — handshake done");
                    *shared.sim_kennung.lock() = Some(kennung);
                }
                Ok(Some(DispatchMsg::Quit)) => {
                    tracing::warn!("SimConnect sent QUIT — dropping connection");
                    return;
                }
                // v1.7.8 — ein Element aus der Facility-Lieferung.
                //
                // Gesammelt wird nur, was wir angefordert haben; alles
                // andere wird verworfen statt geraten. Der Sammler
                // liegt im ungattierten Teil, damit das Zerlegen ohne
                // Simulator pruefbar bleibt.
                Ok(Some(DispatchMsg::FacilityData {
                    request_id,
                    typ,
                    bytes,
                    ..
                })) => {
                    // ⚠ Nachrichten ohne offene Lieferung werden
                    // VERWORFEN, nicht geraten. Eine Antwort, deren
                    // Anfrage abgelaufen ist, gehoert niemandem mehr.
                    let Some(lieferung) = facility_lieferungen.zu(request_id) else {
                        if request_id >= FACILITY_REQUEST_BASE {
                            tracing::debug!(
                                request_id,
                                "Facility-Nachricht ohne offene Lieferung — verworfen"
                            );
                        }
                        continue;
                    };
                    if typ == sys::FACILITY_DATA_AIRPORT {
                        // Der Referenzpunkt — ohne ihn sind die
                        // Rollwegpunkte nicht umrechenbar.
                        if let Some(w) = facility::zerlege(facility::FLUGHAFEN_FELDER, &bytes) {
                            lieferung.referenz = Some((w[0].als_f64(), w[1].als_f64()));
                        }
                    } else if typ == sys::FACILITY_DATA_TAXI_POINT {
                        // ⚠ Auch ein unlesbarer Punkt muss einen Platz
                        // belegen. `START`/`END` sind POSITIONEN in
                        // dieser Liste — wer einen Eintrag auslaesst,
                        // verschiebt jede Kante danach auf einen anderen
                        // Punkt. Lieber ein unmoeglicher Punkt, den der
                        // Zusammenbau verwirft, als eine verschobene
                        // Liste.
                        match facility::zerlege(facility::ROLLWEG_PUNKT_FELDER, &bytes) {
                            Some(w) => lieferung.punkte.push((w[1].als_f64(), w[2].als_f64())),
                            None => lieferung.punkte.push((f64::NAN, f64::NAN)),
                        }
                    } else if typ == sys::FACILITY_DATA_TAXI_NAME {
                        // Ebenso: Der Index zaehlt, nicht der Inhalt.
                        lieferung.namen.push(facility::name_aus_bytes(&bytes));
                    } else if typ == sys::FACILITY_DATA_TAXI_PATH {
                        if let Some(w) = facility::zerlege(facility::ROLLWEG_KANTE_FELDER, &bytes) {
                            let (a, b, n) = (w[2].als_i32(), w[3].als_i32(), w[4].als_i32());
                            if a >= 0 && b >= 0 && n >= 0 {
                                lieferung.kanten.push((a as usize, b as usize, n as usize));
                            }
                        }
                    } else if typ == sys::FACILITY_DATA_PAVEMENT {
                        // Die versetzte Schwelle — EIGENE Satzart, keine
                        // eingebetteten Felder. Sie kommt nach ihrem
                        // Bahnsatz, in der Reihenfolge der Definition:
                        // erst PRIMARY_THRESHOLD, dann SECONDARY.
                        if !lieferung.sammler.pavementsatz(&bytes) {
                            tracing::warn!(
                                laenge = bytes.len(),
                                "PAVEMENT-Satz ohne passende Bahn — verworfen"
                            );
                        }
                    } else if typ == sys::FACILITY_DATA_RUNWAY {
                        if !lieferung.sammler.bahnsatz(&bytes) {
                            // Kein stiller Verlust: Ein Block, der nicht
                            // zur Definition passt, heisst, dass die
                            // Feldliste nicht stimmt.
                            tracing::warn!(
                                laenge = bytes.len(),
                                "Facility-Bahnblock passt nicht zur Definition — \
                                 Feldliste pruefen"
                            );
                        }
                    }
                }
                Ok(Some(DispatchMsg::FacilityDataEnde { request_id })) => {
                    // ⚠ Nur die Lieferung ZU DIESER Kennung. Ist sie
                    // nicht mehr offen, gehoert die Antwort niemandem —
                    // verwerfen, nicht dem laufenden Auftrag zuschlagen.
                    if let Some(lieferung) = facility_lieferungen.abschliessen(request_id) {
                        // ⚠ Der Platz kommt aus der KENNUNG, nicht aus
                        // `laufender()`. Nach der Wartezeit laeuft
                        // laengst ein anderer Auftrag; eine verspaetete
                        // Antwort bekaeme sonst dessen Namen — derselbe
                        // Fehler wie in v1.7.13, nur verschoben.
                        let auftrag_id = request_id.wrapping_sub(FACILITY_REQUEST_BASE);
                        let Some(icao) = shared.szenerie.lock().platz_zu_kennung(auftrag_id) else {
                            tracing::warn!(
                                request_id,
                                "Facility-Lieferung ohne bekannten Platz — verworfen"
                            );
                            continue;
                        };
                        tracing::info!(
                            %icao,
                            request_id,
                            bahnen = lieferung.sammler.anzahl(),
                            "Facility-Lieferung vollstaendig"
                        );
                        // Erst JETZT sichtbar machen — vorher waere es
                        // eine halbe Wahrheit.
                        // Rollwege erst hier zusammensetzen: Vorher
                        // sind die drei Listen nicht vollstaendig, und
                        // eine Kante koennte auf einen Punkt zeigen, der
                        // noch nicht eingetroffen ist.
                        let rollwege = match lieferung.referenz {
                            Some(r) => facility::rollwege_zusammensetzen(
                                r,
                                &lieferung.punkte,
                                &lieferung.namen,
                                &lieferung.kanten,
                            ),
                            None => {
                                if !lieferung.punkte.is_empty() {
                                    tracing::warn!(
                                        "Rollwegpunkte ohne Referenzpunkt — nicht umrechenbar"
                                    );
                                }
                                Vec::new()
                            }
                        };
                        tracing::info!(rollwege = rollwege.len(), "Rollwege zusammengesetzt");
                        let auskunft = sim_core::szenerie::SzenerieFlughafen {
                            icao,
                            bahnen: lieferung.sammler.fertig(),
                            rollwege,
                            quelle: "msfs".to_string(),
                        };
                        // ⚠ Die Diagnose VOR dem Ablegen setzen und aus
                        // derselben Auskunft speisen — sonst zaehlt sie
                        // irgendwann etwas anderes als das, was benutzt
                        // wird.
                        *shared.szenerie_diagnose.lock() = SzenerieDiagnose::Geliefert {
                            icao: auskunft.icao.clone(),
                            bahnen: auskunft.bahnen.len(),
                            rollwege: auskunft.rollwege.len(),
                        };
                        shared
                            .szenerie
                            .lock()
                            .geliefert_zu_kennung(auftrag_id, auskunft);
                    }
                }
                Ok(Some(DispatchMsg::Exception {
                    exception,
                    send_id,
                    index,
                })) => {
                    // ⚠ ZUERST fragen, ob die Ausnahme zu einem
                    // Facility-FELDNAMEN gehoert. Der `index` waere
                    // sonst ueber die TELEMETRIE-Feldliste gedeutet, und
                    // ein falsch geschriebenes Facility-Feld saehe aus
                    // wie ein fremdes SimVar-Problem.
                    if let Some(feld) =
                        facility::feld_zu_paket(&conn.facility_feld_send_ids, send_id)
                    {
                        tracing::error!(
                            exception,
                            send_id,
                            %feld,
                            "Facility-Feldname vom Simulator zurueckgewiesen — \
                             die Feldliste passt nicht zu dieser Fassung des \
                             Simulators"
                        );
                        // ⚠ HART behandeln, nicht nur protokollieren.
                        // Ist ein Feld der Definition abgelehnt, ist der
                        // ganze Facility-Weg unbrauchbar: Jede Antwort
                        // haette ein anderes Raster als erwartet. Vorher
                        // lief er weiter, und Auftraege meldeten
                        // „unterwegs" oder „geliefert", obwohl die
                        // Definition nachweislich zurueckgewiesen war
                        // (QS-Befund 4, dritte Runde).
                        shared
                            .szenerie
                            .lock()
                            .definition_abgelehnt(feld, format!("Ausnahme {exception}"));
                        *shared.szenerie_diagnose.lock() =
                            SzenerieDiagnose::Abgelehnt(format!("Felddefinition ({exception})"));
                        facility_lieferungen = facility::Lieferungen::neu();
                        continue;
                    }
                    // This is the diagnostic the legacy crate didn't
                    // give us — log the exact SimVar that failed.
                    let field = TELEMETRY_FIELDS.get(index as usize).map(|f| f.name);
                    tracing::warn!(
                        exception,
                        send_id,
                        index,
                        ?field,
                        "SIMCONNECT_RECV_EXCEPTION — SimVar request was rejected"
                    );
                    // ⚠ Gehoert sie zu einer Szenerie-Anfrage, ist der
                    // Platz damit ABGELEHNT — nicht weiter „unterwegs".
                    // Sonst fragt das Buch zehnmal nach einem Flughafen,
                    // den der Simulator nicht kennt, und die Diagnose am
                    // Flug behauptet die ganze Zeit, es sei noch etwas
                    // unterwegs.
                    if let Some(auftrag_id) = facility::auftrag_zu_paket(&facility_pakete, send_id)
                    {
                        let grund = format!("SimConnect-Ausnahme {exception}");
                        let platz = shared
                            .szenerie
                            .lock()
                            .abgelehnt_zu_kennung(auftrag_id, grund);
                        tracing::warn!(
                            ?platz,
                            exception,
                            "Szenerie-Anfrage vom Simulator zurueckgewiesen"
                        );
                    }
                    // Route it to the Inspector tool too, if this
                    // exception's send_id matches one of its watches'
                    // AddToDataDefinition calls — otherwise the pilot
                    // sees the mistyped LVar just sit at "no value"
                    // forever, indistinguishable from "sim hasn't sent
                    // data yet" (see InspectorWatch::error).
                    if let Some(watch_id) =
                        telemetry::inspector_watch_for_exception(&conn.inspector_send_ids, send_id)
                    {
                        shared.inspector.lock().set_error(
                            watch_id,
                            format!("SimConnect exception #{exception} (send_id {send_id})"),
                        );
                    }
                }
                Ok(Some(DispatchMsg::SimObjectData { request_id, bytes })) => {
                    last_data = Instant::now();
                    match request_id {
                        REQUEST_ID => {
                            // v0.7.17 (F-001): no more Fenix-Beta flag — the
                            // adapter always applies the Fenix-A32x extension
                            // LVARs when the aircraft profile is Fenix.
                            let mut snap = telemetry::parse(&bytes, simulator);
                            // Spec v0.7.15 F5: Pause-State aus dem Atomic
                            // in den Snapshot kopieren — wird vom Streamer-
                            // Loop in lib.rs ausgewertet damit der Pause-
                            // Akkumulator auch waehrend MSFS-Esc-Pause
                            // (= eingefrorene Snapshots) korrekt zaehlt.
                            // v1.6.12: Replay/Teleport/Vorspulen zaehlen wie
                            // Pause. Der Streamer in lib.rs friert damit die
                            // Phasen-Engine ein, statt eine abgespielte
                            // Aufzeichnung als Flug zu werten — dieselbe
                            // Behandlung wie im X-Plane-Adapter.
                            snap.paused = shared.sim_paused.load(Ordering::Relaxed)
                                || shared.sim_unecht_tiefe.load(Ordering::Relaxed) > 0;
                            // v0.7.19 GAF-707: Crash-Latch aus dem Shared-
                            // State in den Snapshot mergen. Caller in
                            // lib.rs/step_flight reagiert auf den Flip
                            // false→true und setzt FlightStats.accident_*.
                            // CrashReset setzt das Flag zurueck, ohne den
                            // FlightStats-Latch zu beruehren (Spec §Leit-
                            // entscheidung 6).
                            snap.crashed = shared.sim_crashed.load(Ordering::Relaxed);
                            snap.crash_source = if snap.crashed {
                                Some("msfs_crashed_event".into())
                            } else {
                                None
                            };
                            // Merge in the most recent touchdown sample
                            // so consumers see a unified snapshot.
                            if let Some(td) = *shared.touchdown.lock() {
                                if !td.is_uninitialised() {
                                    // PLANE TOUCHDOWN NORMAL VELOCITY in MSFS
                                    // returns the touchdown impact velocity as
                                    // a POSITIVE magnitude (verified against
                                    // LandingToast: pilot lands at -234 fpm,
                                    // SimVar reports +234). Conventional V/S
                                    // notation is negative for descent, so we
                                    // negate. Take the absolute value first to
                                    // be defensive against odd addons that
                                    // might report signed — we always want a
                                    // descent (negative) value at touchdown.
                                    snap.touchdown_vs_fpm =
                                        Some(-((td.vs_fps * 60.0).abs()) as f32);
                                    // v0.5.24: invert MSFS pitch — same
                                    // convention bug as live PLANE PITCH
                                    // DEGREES (positive=nose-down in
                                    // MSFS, but universal aviation
                                    // expects positive=nose-up). Without
                                    // this an A321 flare with +5° real
                                    // pitch was stored as -5° in PIREPs.
                                    snap.touchdown_pitch_deg = Some(-(td.pitch_deg as f32));
                                    snap.touchdown_bank_deg = Some(td.bank_deg as f32);
                                    snap.touchdown_heading_mag_deg =
                                        Some(td.heading_mag_deg as f32);
                                    snap.touchdown_lat = Some(td.lat_rad.to_degrees());
                                    snap.touchdown_lon = Some(td.lon_rad.to_degrees());
                                }
                            }
                            // First-frame logging: fire once per dispatch
                            // session (= per SimConnect handle) so we get
                            // an info-line per real reconnect but don't
                            // log on every snap. Driven by the local
                            // `got_first` flag.
                            if !got_first {
                                got_first = true;
                                tracing::info!(
                                    aircraft = ?snap.aircraft_title,
                                    profile = ?snap.aircraft_profile,
                                    "MSFS first snapshot received"
                                );
                                log_first_snapshot_diagnostics(&snap);
                            }
                            // Connection-state bump: read SHARED state on
                            // every frame so a manual `clear_snapshot()`
                            // (Fix #8 user button) which set state to
                            // Connecting gets correctly transitioned back
                            // to Connected on the next live frame.
                            // Without this, a local-only `got_first` flag
                            // would stay true across the manual clear,
                            // and the state would freeze at Connecting
                            // until the next reconnect cycle even though
                            // fresh snapshots are flowing again.
                            // Mirrors how the X-Plane listener handles
                            // this exact case.
                            {
                                let mut s = shared.state.lock();
                                if *s != ConnectionState::Connected {
                                    *s = ConnectionState::Connected;
                                }
                            }
                            *shared.snapshot.lock() = Some(snap);
                        }
                        TOUCHDOWN_REQUEST_ID => {
                            let td = Touchdown::from_block(&bytes);
                            *shared.touchdown.lock() = Some(td);
                        }
                        INSPECTOR_REQUEST_ID => {
                            shared.inspector.lock().ingest(&bytes);
                        }
                        other => {
                            tracing::trace!(request_id = other, "unknown SimObjectData request_id");
                        }
                    }
                }
                Ok(Some(DispatchMsg::ClientData { request_id, bytes })) => {
                    // PMDG SDK ClientData arrived. The 916-byte
                    // NG3 block (or future 777X block) gets stored
                    // verbatim in `shared.pmdg.{ng3,x777}_raw`;
                    // higher layers (snapshot integration in
                    // Phase 5.4) decode on demand via
                    // `Pmdg738Snapshot::from_raw()`.
                    match request_id {
                        PMDG_NG3_REQUEST_ID => {
                            let expected_len =
                                std::mem::size_of::<crate::pmdg::ng3::Pmdg738RawData>();
                            if bytes.len() < expected_len {
                                tracing::warn!(
                                    got = bytes.len(),
                                    expected = expected_len,
                                    "PMDG NG3 ClientData payload too short — ignoring"
                                );
                            } else {
                                // Safety: `Pmdg738RawData` is `#[repr(C)]`,
                                // matches MSVC layout, and we just verified
                                // the payload has at least `size_of()` bytes.
                                // The struct is `Copy + Clone` so a bytewise
                                // copy is safe. We Box it because the struct
                                // is ~1 KB and we don't want it on the stack.
                                let raw: Box<crate::pmdg::ng3::Pmdg738RawData> = unsafe {
                                    let mut b: Box<
                                        std::mem::MaybeUninit<crate::pmdg::ng3::Pmdg738RawData>,
                                    > = Box::new(std::mem::MaybeUninit::uninit());
                                    std::ptr::copy_nonoverlapping(
                                        bytes.as_ptr(),
                                        b.as_mut_ptr() as *mut u8,
                                        expected_len,
                                    );
                                    Box::from_raw(
                                        Box::into_raw(b) as *mut crate::pmdg::ng3::Pmdg738RawData
                                    )
                                };
                                let mut g = shared.pmdg.lock();
                                g.ng3_raw = Some(raw);
                                g.last_packet_at = Some(Instant::now());
                            }
                        }
                        PMDG_X777_REQUEST_ID => {
                            let expected_len =
                                std::mem::size_of::<crate::pmdg::x777::Pmdg777XRawData>();
                            if bytes.len() < expected_len {
                                tracing::warn!(
                                    got = bytes.len(),
                                    expected = expected_len,
                                    "PMDG 777X ClientData payload too short — ignoring"
                                );
                            } else {
                                let raw: Box<crate::pmdg::x777::Pmdg777XRawData> = unsafe {
                                    let mut b: Box<
                                        std::mem::MaybeUninit<crate::pmdg::x777::Pmdg777XRawData>,
                                    > = Box::new(std::mem::MaybeUninit::uninit());
                                    std::ptr::copy_nonoverlapping(
                                        bytes.as_ptr(),
                                        b.as_mut_ptr() as *mut u8,
                                        expected_len,
                                    );
                                    Box::from_raw(
                                        Box::into_raw(b) as *mut crate::pmdg::x777::Pmdg777XRawData
                                    )
                                };
                                let mut g = shared.pmdg.lock();
                                g.x777_raw = Some(raw);
                                g.last_packet_at = Some(Instant::now());
                            }
                        }
                        other => {
                            tracing::trace!(request_id = other, "unknown ClientData request_id");
                        }
                    }
                }
                Ok(Some(DispatchMsg::SystemState {
                    request_id,
                    air_path,
                })) => {
                    if request_id == AIRCRAFT_LOADED_REQUEST_ID {
                        let detected = crate::pmdg::PmdgVariant::detect_from_air_path(&air_path);
                        let mut g = shared.pmdg.lock();
                        if g.variant != detected {
                            tracing::info!(
                                ?detected,
                                old = ?g.variant,
                                air_path = %air_path,
                                "PMDG variant change detected"
                            );
                            g.variant = detected;
                            // Aircraft changed → drop any cached
                            // raw data + reset subscribed flag so
                            // the worker re-subscribes for the new
                            // variant on the next loop iteration.
                            g.ng3_raw = None;
                            g.x777_raw = None;
                            g.subscribed = false;
                            g.last_packet_at = None;
                        }
                    }
                }
                Ok(Some(DispatchMsg::FlowEvent { event })) => {
                    // Der Simulator sagt uns selbst, dass die Telemetrie
                    // gerade nicht den geflogenen Zustand beschreibt. Behandelt
                    // wie eine Pause — dieselbe Entscheidung wie im X-Plane-
                    // Adapter fuer `sim/time/is_in_replay`.
                    //
                    // Tiefenzaehler statt Ja/Nein: die Vorgaenge koennen sich
                    // ueberlappen (Teleport waehrend eines Replays). Ein
                    // einzelnes _DONE wuerde sonst den noch laufenden anderen
                    // Vorgang mit abraeumen. Nie unter null, damit ein
                    // verpasstes _START (Abonnement erst mitten im Replay) den
                    // Zaehler nicht negativ und damit taub macht.
                    let (delta, name): (i32, &str) = match event {
                        e if e == FLOW_REPLAY_START => (1, "Replay"),
                        e if e == FLOW_REPLAY_END => (-1, "Replay"),
                        e if e == FLOW_TELEPORT_START => (1, "Teleport"),
                        e if e == FLOW_TELEPORT_DONE => (-1, "Teleport"),
                        e if e == FLOW_SKIP_START => (1, "Vorspulen"),
                        e if e == FLOW_SKIP_DONE => (-1, "Vorspulen"),
                        _ => (0, ""),
                    };
                    if delta != 0 {
                        // Getrenntes Lesen und Schreiben, KEIN atomares
                        // Aendern: zulaessig nur, weil genau dieser eine
                        // Dispatch-Faden schreibt. Kommt je ein zweiter dazu
                        // (weitere Ereignisquelle, Ruecksetzen beim
                        // Verbindungsabbruch), muss das auf `fetch_add` mit
                        // Klemmen umgestellt werden — sonst ueberschreiben
                        // sich zwei Aenderungen, der Zaehler bleibt auf 1 und
                        // die Telemetrie gilt fuer immer als unecht
                        // (QS-Befund 21.08.2026).
                        let vorher = shared.sim_unecht_tiefe.load(Ordering::Relaxed);
                        let nachher = (vorher + delta).max(0);
                        shared.sim_unecht_tiefe.store(nachher, Ordering::Relaxed);
                        tracing::info!(
                            vorgang = name,
                            beginnt = delta > 0,
                            tiefe = nachher,
                            "Sim meldet: Telemetrie gerade nicht echt"
                        );
                    }
                }
                Ok(Some(DispatchMsg::SystemEvent { event_id, data })) => {
                    if event_id == SIM_START_EVENT_ID {
                        // SimStart fires when the user loads a new
                        // flight. Re-request AircraftLoaded so we
                        // pick up any aircraft change.
                        if let Err(e) = conn.subscribe_aircraft_loaded() {
                            tracing::warn!(error = %e, "re-request AircraftLoaded failed");
                        }
                    } else if event_id == PAUSE_EX1_EVENT_ID {
                        // Spec v0.7.15 F5 (QS-Round-2): Pause_EX1 sendet
                        // bei jedem Pause-Wechsel ein Event mit Flag-Set
                        // im dwData. Wir behandeln jedes != 0 als
                        // "Pause aktiv" — Full/Active/Sim-Pause-
                        // Unterscheidung kommt erst mit einer
                        // spaeteren Iteration falls relevant.
                        let paused = data != 0;
                        shared.sim_paused.store(paused, Ordering::Relaxed);
                        tracing::info!(
                            data = format!("0x{:x}", data),
                            paused,
                            "MSFS SimConnect Pause_EX1-Event empfangen"
                        );
                    } else if event_id == CRASHED_EVENT_ID {
                        // v0.7.19 GAF-707: latch im Shared-State, der
                        // Snapshot-Builder mergt ihn ein. Spec §Detection.
                        shared.sim_crashed.store(true, Ordering::Relaxed);
                        tracing::warn!(
                            data = format!("0x{:x}", data),
                            "MSFS SimConnect Crashed-Event empfangen — Accident wird gelatcht"
                        );
                    } else if event_id == CRASH_RESET_EVENT_ID {
                        // v0.7.19 GAF-707: Adapter-Raw-Flag wieder loeschen.
                        // Der aktive Flug in lib.rs behaelt seinen
                        // accident_detected-Latch unabhaengig davon
                        // (Spec §Leitentscheidung 6).
                        shared.sim_crashed.store(false, Ordering::Relaxed);
                        tracing::info!(
                            data = format!("0x{:x}", data),
                            "MSFS SimConnect CrashReset-Event empfangen — Adapter-Flag geloescht"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "SimConnect dispatch error");
                    return;
                }
            }
        }

        // PMDG subscription gate. Once we know which variant is
        // loaded AND we haven't yet subscribed for it, register
        // the ClientData definition + request data. Best-effort —
        // an FFI failure logs a warning but doesn't kill the
        // dispatch loop. Subscribed flag prevents redundant
        // re-subscriptions on every iteration.
        let pmdg_action = {
            let g = shared.pmdg.lock();
            if !g.subscribed {
                g.variant
            } else {
                None
            }
        };
        if let Some(variant) = pmdg_action {
            match variant {
                crate::pmdg::PmdgVariant::Ng3 => {
                    if let Err(e) = conn.register_pmdg_ng3() {
                        tracing::warn!(
                            error = %e,
                            "PMDG NG3 ClientData subscription failed (SDK probably not enabled in 737NG3_Options.ini)"
                        );
                    } else {
                        tracing::info!("PMDG NG3 ClientData subscription registered");
                        shared.pmdg.lock().subscribed = true;
                    }
                }
                crate::pmdg::PmdgVariant::X777 => {
                    if let Err(e) = conn.register_pmdg_x777() {
                        tracing::warn!(
                            error = %e,
                            "PMDG 777X ClientData subscription failed (SDK probably not enabled in 777X_Options.ini)"
                        );
                    } else {
                        tracing::info!("PMDG 777X ClientData subscription registered");
                        shared.pmdg.lock().subscribed = true;
                    }
                }
            }
        }

        // Stale watchdog: if no data has arrived for a while assume
        // MSFS crashed or the pipe died, and let the outer loop
        // re-open the connection.
        if got_first && last_data.elapsed() > STALE_TIMEOUT {
            tracing::warn!("no SimConnect data for {:?} — reconnecting", STALE_TIMEOUT);
            return;
        }

        thread::sleep(Duration::from_millis(50));
    }
}

fn log_first_snapshot_diagnostics(snap: &SimSnapshot) {
    tracing::info!(
        fuel_total_kg = snap.fuel_total_kg,
        total_weight_kg = ?snap.total_weight_kg,
        aircraft_title = ?snap.aircraft_title,
        aircraft_profile = ?snap.aircraft_profile,
        "raw SimConnect first-snapshot fuel/weight diagnostic"
    );
}

fn set_error(shared: &Arc<Shared>, msg: String) {
    *shared.last_error.lock() = Some(msg);
}

fn sleep_or_stop(stop: &Arc<AtomicBool>, dur: Duration) {
    let step = Duration::from_millis(100);
    let mut left = dur;
    while !left.is_zero() {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        let s = std::cmp::min(step, left);
        thread::sleep(s);
        left = left.saturating_sub(s);
    }
}

// ---- Connection wrapper ----

/// Owns the SimConnect handle and provides the higher-level operations
/// the worker loop drives. `Drop` calls `SimConnect_Close`.
struct Connection {
    handle: sys::HANDLE,
    /// `(send_id, watch_id)` captured while registering the inspector
    /// data definition — lets a later async SIMCONNECT_RECV_EXCEPTION be
    /// attributed back to the specific watch whose AddToDataDefinition
    /// call produced it. Rebuilt from scratch on every
    /// `register_inspector()` call. See `telemetry::inspector_watch_for_exception`.
    /// Keyed on the watch's stable `id`, not its name — two watches can
    /// legitimately share a name (see `InspectorState::set_error`'s doc).
    /// Paketkennung → Facility-FELDNAME, fuer asynchrone
    /// Zurueckweisungen von `AddToFacilityDefinition`.
    facility_feld_send_ids: Vec<(u32, String)>,
    inspector_send_ids: Vec<(u32, u32)>,
}

impl Connection {
    fn open(name: &str) -> Result<Self, String> {
        let cname = std::ffi::CString::new(name).expect("connection name must be plain ASCII");
        let mut handle: sys::HANDLE = std::ptr::null_mut();
        let hr = unsafe {
            sys::SimConnect_Open(
                &mut handle,
                cname.as_ptr(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                0,
            )
        };
        if hr != 0 {
            return Err(format!("HRESULT 0x{hr:08X}"));
        }
        Ok(Self {
            handle,
            facility_feld_send_ids: Vec::new(),
            inspector_send_ids: Vec::new(),
        })
    }

    /// Register every entry in `TELEMETRY_FIELDS` in order.
    fn register_telemetry(&mut self) -> Result<(), String> {
        for (idx, field) in TELEMETRY_FIELDS.iter().enumerate() {
            let cname = std::ffi::CString::new(field.name)
                .map_err(|_| "SimVar name contained NUL".to_string())?;
            let cunit = std::ffi::CString::new(field.unit)
                .map_err(|_| "Unit string contained NUL".to_string())?;
            let datatype = match field.kind {
                telemetry::FieldKind::Float64 => sys::SIMCONNECT_DATATYPE_FLOAT64,
                telemetry::FieldKind::Int32 => sys::SIMCONNECT_DATATYPE_INT32,
                telemetry::FieldKind::String256 => sys::SIMCONNECT_DATATYPE_STRING256,
            };
            let hr = unsafe {
                sys::SimConnect_AddToDataDefinition(
                    self.handle,
                    DEFINITION_ID,
                    cname.as_ptr(),
                    cunit.as_ptr(),
                    datatype,
                    0.0,
                    u32::MAX,
                )
            };
            if hr != 0 {
                return Err(format!(
                    "AddToDataDefinition for SimVar #{idx} \"{}\" returned 0x{hr:08X}",
                    field.name
                ));
            }
        }
        Ok(())
    }

    /// Die Facility-Definition zusammensetzen — Bahnen und Rollwege.
    ///
    /// # Wie die Schnittstelle arbeitet
    ///
    /// Die Definition besteht aus Feldnamen und Klammern:
    /// `OPEN AIRPORT` … `OPEN RUNWAY` … `CLOSE RUNWAY` … `CLOSE AIRPORT`.
    /// Danach liefert `RequestFacilityData` die Elemente einzeln, in der
    /// Reihenfolge der Definition, als rohe Datenbloecke.
    ///
    /// ⚠ Die Feldnamen stehen NICHT in `SimConnect.h`, sondern in der
    /// SDK-Dokumentation, und sie sind GROSS_MIT_UNTERSTRICH. Ich hatte
    /// sie zuerst als `Latitude`/`Heading` geraten — jeder dieser Namen
    /// waere hier abgelehnt worden, und zwar erst zur Laufzeit.
    ///
    /// Ein abgelehntes Feld ist deshalb ein **harter** Fehler und kein
    /// „best effort": Faehlt `WIDTH`, kommt die Bahn ohne Breite zurueck
    /// — und die Breite ist genau das Mass, mit dem entschieden wird, ob
    /// eine Rollspur die befestigte Flaeche verlaesst.
    fn register_facility(&mut self) -> Result<(), String> {
        let mut eintraege: Vec<&str> = vec!["OPEN AIRPORT"];
        // Der Referenzpunkt des Platzes — ohne ihn sind die
        // Rollwegpunkte nicht umrechenbar, sie kommen als Versatz in
        // Metern (`BIAS_X`/`BIAS_Z`).
        eintraege.extend(facility::FLUGHAFEN_FELDER.iter().map(|(n, _)| *n));
        eintraege.push("OPEN RUNWAY");
        // ⚠ `BAHN_DEFINITION`, NICHT `BAHN_FELDER`: Die versetzte
        // Schwelle liegt in einem PAVEMENT-Untersatz, der mit
        // OPEN/CLOSE PRIMARY_THRESHOLD geoeffnet wird. Diese Marken
        // liefern keine Bytes und stehen darum nicht im Byte-Raster.
        eintraege.extend(facility::BAHN_DEFINITION.iter().copied());
        eintraege.push("CLOSE RUNWAY");
        // Rollwege: Punkte, Kanten, Namen — drei Listen, die ueber
        // Indizes zusammenhaengen, genau wie X-Planes 1201/1202.
        eintraege.push("OPEN TAXI_POINT");
        eintraege.extend(facility::ROLLWEG_PUNKT_FELDER.iter().map(|(n, _)| *n));
        eintraege.push("CLOSE TAXI_POINT");
        eintraege.push("OPEN TAXI_NAME");
        eintraege.extend(facility::ROLLWEG_NAME_FELDER.iter().map(|(n, _)| *n));
        eintraege.push("CLOSE TAXI_NAME");
        eintraege.push("OPEN TAXI_PATH");
        eintraege.extend(facility::ROLLWEG_KANTE_FELDER.iter().map(|(n, _)| *n));
        eintraege.push("CLOSE TAXI_PATH");
        eintraege.push("CLOSE AIRPORT");

        for name in eintraege {
            let cname =
                std::ffi::CString::new(name).map_err(|_| "Feldname enthielt NUL".to_string())?;
            let hr = unsafe {
                sys::SimConnect_AddToFacilityDefinition(
                    self.handle,
                    FACILITY_DEFINITION_ID,
                    cname.as_ptr(),
                )
            };
            if hr != 0 {
                return Err(format!(
                    "AddToFacilityDefinition fuer \"{name}\" gab 0x{hr:08X} zurueck — \
                     Feldname pruefen (SDK-Doku, GROSS_MIT_UNTERSTRICH)"
                ));
            }
            // ⚠ Ein `hr == 0` heisst NICHT, dass der Feldname stimmt.
            // Das SDK nennt eine asynchrone Ausnahme ausdruecklich als
            // moeglichen Ausgang dieses Aufrufs — sie kommt spaeter und
            // verweist auf die Paketkennung DIESES Aufrufs.
            //
            // Ohne diese Zuordnung deutet der Ausnahmezweig ihren
            // `index` ueber die TELEMETRIE-Feldliste und nennt einen
            // voellig fremden SimVar-Namen. Ein falsch geschriebenes
            // Facility-Feld sieht dann aus wie ein Telemetrie-Problem
            // (QS-Befund 4, zweite Runde).
            let mut send_id: sys::DWORD = 0;
            let hr = unsafe { sys::SimConnect_GetLastSentPacketID(self.handle, &mut send_id) };
            if hr == 0 {
                self.facility_feld_send_ids
                    .push((send_id, name.to_string()));
            } else {
                tracing::warn!(
                    feld = %name,
                    "GetLastSentPacketID nach AddToFacilityDefinition fehlgeschlagen — \
                     eine Zurueckweisung dieses Feldes bleibt unzuordenbar"
                );
            }
        }
        Ok(())
    }

    /// Die Bahnen eines Flughafens anfordern.
    ///
    /// Die Antworten kommen asynchron ueber die Empfangsschleife als
    /// `DispatchMsg::FacilityData`, abgeschlossen von
    /// `FacilityDataEnde`. Der Aufruf selbst kehrt sofort zurueck.
    fn request_facility(
        &mut self,
        icao: &str,
        request_id: sys::SIMCONNECT_DATA_REQUEST_ID,
    ) -> Result<Option<u32>, String> {
        let cicao = std::ffi::CString::new(icao).map_err(|_| "ICAO enthielt NUL".to_string())?;
        let leer = std::ffi::CString::new("").expect("leere Zeichenkette");
        let hr = unsafe {
            sys::SimConnect_RequestFacilityData(
                self.handle,
                FACILITY_DEFINITION_ID,
                request_id,
                cicao.as_ptr(),
                leer.as_ptr(),
            )
        };
        if hr != 0 {
            return Err(format!(
                "RequestFacilityData({icao}) gab 0x{hr:08X} zurueck"
            ));
        }
        // ⚠ Ein `hr == 0` heisst NICHT, dass der Simulator den Platz
        // kennt. Eine Zurueckweisung kommt spaeter und asynchron als
        // `SIMCONNECT_RECV_EXCEPTION`, und sie verweist auf die
        // Paketkennung DIESES Aufrufs. Ohne sie bliebe der Auftrag
        // „unterwegs", und das Buch fragte zehnmal nach einem Platz, den
        // es nicht gibt.
        let mut send_id: sys::DWORD = 0;
        let hr = unsafe { sys::SimConnect_GetLastSentPacketID(self.handle, &mut send_id) };
        if hr != 0 {
            tracing::warn!(
                %icao,
                "GetLastSentPacketID nach RequestFacilityData fehlgeschlagen — \
                 eine Zurueckweisung dieses Platzes bleibt unzuordenbar"
            );
            return Ok(None);
        }
        Ok(Some(send_id))
    }

    /// Register the touchdown sample fields under definition #2.
    /// Best-effort: we already log per-field exceptions in the
    /// dispatch loop, so a partial registration here is recoverable.
    fn register_touchdown(&mut self) -> Result<(), String> {
        for (idx, field) in TOUCHDOWN_FIELDS.iter().enumerate() {
            let cname = std::ffi::CString::new(field.name)
                .map_err(|_| "SimVar name contained NUL".to_string())?;
            let cunit = std::ffi::CString::new(field.unit)
                .map_err(|_| "Unit string contained NUL".to_string())?;
            let datatype = match field.kind {
                telemetry::FieldKind::Float64 => sys::SIMCONNECT_DATATYPE_FLOAT64,
                telemetry::FieldKind::Int32 => sys::SIMCONNECT_DATATYPE_INT32,
                telemetry::FieldKind::String256 => sys::SIMCONNECT_DATATYPE_STRING256,
            };
            let hr = unsafe {
                sys::SimConnect_AddToDataDefinition(
                    self.handle,
                    TOUCHDOWN_DEFINITION_ID,
                    cname.as_ptr(),
                    cunit.as_ptr(),
                    datatype,
                    0.0,
                    u32::MAX,
                )
            };
            if hr != 0 {
                return Err(format!(
                    "AddToDataDefinition for touchdown SimVar #{idx} \"{}\" returned 0x{hr:08X}",
                    field.name
                ));
            }
        }
        Ok(())
    }

    /// Re-register the inspector data definition from scratch using
    /// the supplied watchlist. Always clears the existing definition
    /// first so a removed entry actually goes away — SimConnect has
    /// no per-field "remove" call. An empty watchlist is valid (just
    /// clears the definition and skips the request).
    fn register_inspector(&mut self, watches: &[InspectorWatch]) -> Result<(), String> {
        let hr =
            unsafe { sys::SimConnect_ClearDataDefinition(self.handle, INSPECTOR_DEFINITION_ID) };
        // ClearDataDefinition returns S_OK even when the definition
        // didn't exist yet — non-zero is a real error.
        if hr != 0 {
            return Err(format!("ClearDataDefinition returned 0x{hr:08X}"));
        }
        // Rebuilt below, one entry per successfully-issued
        // AddToDataDefinition call — this is what lets a later async
        // exception be attributed back to the right watch.
        self.inspector_send_ids.clear();
        for (idx, w) in watches.iter().enumerate() {
            let cname = std::ffi::CString::new(w.name.as_str())
                .map_err(|_| format!("watch #{idx} name contained NUL"))?;
            let cunit = std::ffi::CString::new(w.unit.as_str())
                .map_err(|_| format!("watch #{idx} unit contained NUL"))?;
            let datatype = match w.kind {
                WatchKind::Number => sys::SIMCONNECT_DATATYPE_FLOAT64,
                WatchKind::Bool => sys::SIMCONNECT_DATATYPE_INT32,
                WatchKind::String => sys::SIMCONNECT_DATATYPE_STRING256,
            };
            let hr = unsafe {
                sys::SimConnect_AddToDataDefinition(
                    self.handle,
                    INSPECTOR_DEFINITION_ID,
                    cname.as_ptr(),
                    cunit.as_ptr(),
                    datatype,
                    0.0,
                    u32::MAX,
                )
            };
            if hr != 0 {
                return Err(format!(
                    "AddToDataDefinition for inspector watch \"{}\" returned 0x{hr:08X}",
                    w.name
                ));
            }
            // AddToDataDefinition frequently reports success (hr == 0)
            // synchronously even for a name SimConnect can't actually
            // resolve — MSFS only raises SIMCONNECT_RECV_EXCEPTION for
            // that later, asynchronously, referencing this call's own
            // send ID. Capture it now so the dispatch loop can route
            // that exception back to watch `w`.
            let mut send_id: sys::DWORD = 0;
            let hr = unsafe { sys::SimConnect_GetLastSentPacketID(self.handle, &mut send_id) };
            if hr == 0 {
                self.inspector_send_ids.push((send_id, w.id));
            } else {
                tracing::warn!(
                    watch = %w.name,
                    "GetLastSentPacketID failed after AddToDataDefinition; this watch's exceptions won't be attributable"
                );
            }
        }
        Ok(())
    }

    fn request_inspector_per_second(&mut self) -> Result<(), String> {
        let hr = unsafe {
            sys::SimConnect_RequestDataOnSimObject(
                self.handle,
                INSPECTOR_REQUEST_ID,
                INSPECTOR_DEFINITION_ID,
                sys::SIMCONNECT_OBJECT_ID_USER,
                sys::SIMCONNECT_PERIOD_SECOND,
                0,
                0,
                0,
                0,
            )
        };
        if hr != 0 {
            return Err(format!("HRESULT 0x{hr:08X}"));
        }
        Ok(())
    }

    fn request_touchdown_per_second(&mut self) -> Result<(), String> {
        // Bumped from SECOND to VISUAL_FRAME (~30 Hz) for the same
        // reason the main telemetry runs that fast: the FSM's
        // Final → Landing tick can fire just before the next
        // SECOND-period touchdown update has propagated the freshly
        // latched values into shared.touchdown, leaving the V/S
        // capture stale by up to 1 second. At ~30 Hz the latch is
        // visible to the next snapshot within ~33 ms.
        let hr = unsafe {
            sys::SimConnect_RequestDataOnSimObject(
                self.handle,
                TOUCHDOWN_REQUEST_ID,
                TOUCHDOWN_DEFINITION_ID,
                sys::SIMCONNECT_OBJECT_ID_USER,
                sys::SIMCONNECT_PERIOD_VISUAL_FRAME,
                0,
                0,
                0,
                0,
            )
        };
        if hr != 0 {
            return Err(format!("HRESULT 0x{hr:08X}"));
        }
        Ok(())
    }

    /// Subscribe to live telemetry at VISUAL_FRAME cadence (~30 Hz).
    /// The 1 Hz SECOND rate we ran on previously was too sparse for
    /// touchdown capture: the actual ground-contact subframe dropped
    /// between two snapshots, the ring buffer only had 5 entries in
    /// the 5-second look-back window, and the recorded V/S routinely
    /// caught the bounce-rebound rather than the impact (logged
    /// "V/S -4 fpm" while MSFS reported -114 fpm). At 30 Hz the
    /// buffer holds 150 entries → impossible to miss the actual
    /// touchdown frame.
    ///
    /// CPU cost is negligible: the dispatch loop already drains all
    /// queued messages each tick via `get_next_dispatch`, so the
    /// only difference is more byte-level parsing per second
    /// (~30 KB/s of data).
    fn request_data_per_second(&mut self) -> Result<(), String> {
        let hr = unsafe {
            sys::SimConnect_RequestDataOnSimObject(
                self.handle,
                REQUEST_ID,
                DEFINITION_ID,
                sys::SIMCONNECT_OBJECT_ID_USER,
                sys::SIMCONNECT_PERIOD_VISUAL_FRAME,
                0,
                0,
                0,
                0,
            )
        };
        if hr != 0 {
            return Err(format!("HRESULT 0x{hr:08X}"));
        }
        Ok(())
    }

    // ------------------------------------------------------------
    // PMDG SDK ClientData (Phase 5.2)
    // ------------------------------------------------------------

    /// Subscribe to the PMDG NG3 `PMDG_NG3_Data` ClientData channel.
    ///
    /// Three-step setup per the SDK reference (`PMDG_NG3_ConnectionTest.cpp`):
    ///   1. Map the well-known data area name to PMDG's reserved ID.
    ///   2. Define the area shape (one big 916-byte block at offset 0).
    ///   3. Request data on change (`PERIOD_ON_SET + FLAG_CHANGED`).
    ///
    /// Returns `Err(_)` for any FFI failure. Note that even a perfect
    /// subscription returns silently if the user hasn't enabled
    /// `EnableDataBroadcast=1` in the PMDG options ini — the
    /// `last_packet_at` field of `PmdgSharedState` is the way to
    /// detect "subscription succeeded but no data flowing".
    fn register_pmdg_ng3(&mut self) -> Result<(), String> {
        let cname = std::ffi::CString::new(crate::pmdg::ng3::PMDG_NG3_DATA_NAME)
            .expect("PMDG_NG3_Data is plain ASCII");
        let hr = unsafe {
            sys::SimConnect_MapClientDataNameToID(
                self.handle,
                cname.as_ptr(),
                crate::pmdg::ng3::PMDG_NG3_DATA_ID,
            )
        };
        if hr != 0 {
            return Err(format!("MapClientDataNameToID returned 0x{hr:08X}"));
        }

        let hr = unsafe {
            sys::SimConnect_AddToClientDataDefinition(
                self.handle,
                PMDG_NG3_DEFINITION_ID,
                0, // offset 0 — entire struct in one shot
                std::mem::size_of::<crate::pmdg::ng3::Pmdg738RawData>() as sys::DWORD,
                0.0,      // fEpsilon (unused for this layout)
                u32::MAX, // DatumID (unused)
            )
        };
        if hr != 0 {
            return Err(format!("AddToClientDataDefinition returned 0x{hr:08X}"));
        }

        // PERIOD_ON_SET means "send only when PMDG actually pushes
        // a new value" (NOT once per second), and FLAG_CHANGED
        // further filters to "only when bytes differ from last".
        // Combined: zero traffic when nothing changes; near-instant
        // when something does.
        let period_on_set: sys::SIMCONNECT_CLIENT_DATA_PERIOD =
            sys::SIMCONNECT_CLIENT_DATA_PERIOD_SIMCONNECT_CLIENT_DATA_PERIOD_ON_SET;
        let flag_changed: sys::SIMCONNECT_CLIENT_DATA_REQUEST_FLAG =
            sys::SIMCONNECT_CLIENT_DATA_REQUEST_FLAG_CHANGED;
        let hr = unsafe {
            sys::SimConnect_RequestClientData(
                self.handle,
                crate::pmdg::ng3::PMDG_NG3_DATA_ID,
                PMDG_NG3_REQUEST_ID,
                PMDG_NG3_DEFINITION_ID,
                period_on_set,
                flag_changed,
                0,
                0,
                0,
            )
        };
        if hr != 0 {
            return Err(format!("RequestClientData returned 0x{hr:08X}"));
        }
        Ok(())
    }

    /// Subscribe to the PMDG 777X `PMDG_777X_Data` ClientData channel.
    /// Same 3-step pattern as `register_pmdg_ng3` but with the 777X
    /// names + IDs and a different struct size (684 bytes vs 916).
    fn register_pmdg_x777(&mut self) -> Result<(), String> {
        let cname = std::ffi::CString::new(crate::pmdg::x777::PMDG_777X_DATA_NAME)
            .expect("PMDG_777X_Data is plain ASCII");
        let hr = unsafe {
            sys::SimConnect_MapClientDataNameToID(
                self.handle,
                cname.as_ptr(),
                crate::pmdg::x777::PMDG_777X_DATA_ID,
            )
        };
        if hr != 0 {
            return Err(format!("MapClientDataNameToID(777X) returned 0x{hr:08X}"));
        }

        let hr = unsafe {
            sys::SimConnect_AddToClientDataDefinition(
                self.handle,
                PMDG_X777_DEFINITION_ID,
                0,
                std::mem::size_of::<crate::pmdg::x777::Pmdg777XRawData>() as sys::DWORD,
                0.0,
                u32::MAX,
            )
        };
        if hr != 0 {
            return Err(format!(
                "AddToClientDataDefinition(777X) returned 0x{hr:08X}"
            ));
        }

        let period_on_set: sys::SIMCONNECT_CLIENT_DATA_PERIOD =
            sys::SIMCONNECT_CLIENT_DATA_PERIOD_SIMCONNECT_CLIENT_DATA_PERIOD_ON_SET;
        let flag_changed: sys::SIMCONNECT_CLIENT_DATA_REQUEST_FLAG =
            sys::SIMCONNECT_CLIENT_DATA_REQUEST_FLAG_CHANGED;
        let hr = unsafe {
            sys::SimConnect_RequestClientData(
                self.handle,
                crate::pmdg::x777::PMDG_777X_DATA_ID,
                PMDG_X777_REQUEST_ID,
                PMDG_X777_DEFINITION_ID,
                period_on_set,
                flag_changed,
                0,
                0,
                0,
            )
        };
        if hr != 0 {
            return Err(format!("RequestClientData(777X) returned 0x{hr:08X}"));
        }
        Ok(())
    }

    /// Subscribe to the AircraftLoaded system state — both as a
    /// one-shot request (so we know what's loaded right now) and as
    /// a subscription to "SimStart" for live aircraft changes.
    fn subscribe_aircraft_loaded(&mut self) -> Result<(), String> {
        let cstate =
            std::ffi::CString::new("AircraftLoaded").expect("AircraftLoaded is plain ASCII");
        let hr = unsafe {
            sys::SimConnect_RequestSystemState(
                self.handle,
                AIRCRAFT_LOADED_REQUEST_ID,
                cstate.as_ptr(),
            )
        };
        if hr != 0 {
            return Err(format!(
                "RequestSystemState(AircraftLoaded) returned 0x{hr:08X}"
            ));
        }

        let cevent = std::ffi::CString::new("SimStart").expect("SimStart is plain ASCII");
        let hr = unsafe {
            sys::SimConnect_SubscribeToSystemEvent(self.handle, SIM_START_EVENT_ID, cevent.as_ptr())
        };
        if hr != 0 {
            return Err(format!(
                "SubscribeToSystemEvent(SimStart) returned 0x{hr:08X}"
            ));
        }

        // Spec v0.7.15 F5 (QS-Round-2): `Pause_EX1`-Event statt der
        // zwei separaten `Paused`/`Unpaused`-Events. Pause_EX1 sendet
        // sofort beim Subscribe den aktuellen Pause-State + bei jedem
        // Wechsel ein Update mit `dwData`-Flag-Set. Damit ist die
        // Pause-Detection korrekt auch wenn AeroACARS connectet
        // waehrend MSFS schon pausiert ist.
        let cevent = std::ffi::CString::new("Pause_EX1").expect("ASCII");
        let hr = unsafe {
            sys::SimConnect_SubscribeToSystemEvent(self.handle, PAUSE_EX1_EVENT_ID, cevent.as_ptr())
        };
        if hr != 0 {
            return Err(format!(
                "SubscribeToSystemEvent(Pause_EX1) returned 0x{hr:08X}"
            ));
        }

        // v0.7.19 GAF-707 Accident-Detection: SimConnect-`Crashed` und
        // `CrashReset` abonnieren. Crashed feuert beim Bodenkontakt mit
        // nicht-ueberlebbaren Parametern, CrashReset wenn die Cut-Scene
        // im MSFS-UI quittiert wird.
        let cevent = std::ffi::CString::new("Crashed").expect("ASCII");
        let hr = unsafe {
            sys::SimConnect_SubscribeToSystemEvent(self.handle, CRASHED_EVENT_ID, cevent.as_ptr())
        };
        if hr != 0 {
            return Err(format!(
                "SubscribeToSystemEvent(Crashed) returned 0x{hr:08X}"
            ));
        }
        let cevent = std::ffi::CString::new("CrashReset").expect("ASCII");
        let hr = unsafe {
            sys::SimConnect_SubscribeToSystemEvent(
                self.handle,
                CRASH_RESET_EVENT_ID,
                cevent.as_ptr(),
            )
        };
        if hr != 0 {
            return Err(format!(
                "SubscribeToSystemEvent(CrashReset) returned 0x{hr:08X}"
            ));
        }
        // v1.6.12 — Flow-Ereignisse des SDK: der Simulator meldet selbst,
        // wenn er eine Aufzeichnung abspielt, das Flugzeug versetzt oder
        // vorspult. Autoritativer als jede Ableitung aus der Telemetrie.
        //
        // BEWUSST NICHT FATAL: MSFS 2020 kennt diese Ereignisse nicht. Ein
        // `return Err` haette dort die ganze Verbindung gerissen — fuer einen
        // Zugewinn, den es auf 2020 ohnehin nicht gibt. Schlaegt es fehl,
        // bleibt der kinematische Rueckfall (`replay_erkennung`) zustaendig.
        let hr = unsafe { sys::SimConnect_SubscribeToFlowEvent(self.handle) };
        if hr != 0 {
            tracing::info!(
                hr = format!("0x{hr:08X}"),
                "SubscribeToFlowEvent nicht verfuegbar (aelteres SimConnect) — Replay-Erkennung laeuft ueber die Telemetrie"
            );
        }

        Ok(())
    }

    /// Pull one message off the SimConnect queue, returning None when
    /// the queue is empty. Distinguishes the receiver IDs we actually
    /// care about; the rest are logged at trace level and dropped.
    fn get_next_dispatch(&mut self) -> Result<Option<DispatchMsg>, String> {
        let mut p_data: *mut sys::SIMCONNECT_RECV = std::ptr::null_mut();
        let mut cb_data: sys::DWORD = 0;
        let hr = unsafe { sys::SimConnect_GetNextDispatch(self.handle, &mut p_data, &mut cb_data) };
        if hr == sys::E_FAIL {
            // Empty queue — not an error in SimConnect-land.
            return Ok(None);
        }
        if hr != 0 {
            return Err(format!("GetNextDispatch returned 0x{hr:08X}"));
        }
        if p_data.is_null() || cb_data == 0 {
            return Ok(None);
        }
        let recv = unsafe { &*p_data };
        let id = recv.dwID;
        let msg = match id {
            sys::SIMCONNECT_RECV_ID_OPEN => {
                // ⚠ Bis v1.7.9 wurde der Inhalt weggeworfen. Darin steht,
                // WOMIT wir reden — MSFS 2020 und 2024 melden sich
                // verschieden, und die Feldnamen der Facility-Abfrage
                // stammen aus der 2024er-SDK-Doku. Ohne diese Kennung ist
                // eine Ablehnung nicht einzuordnen.
                let o = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_OPEN) };
                // Auch das Namensfeld erst kopieren. Es ist zwar
                // byteweise ausgerichtet und `.iter()` daher zulaessig —
                // aber ein Feld eines gepackten Typs ueberhaupt zu
                // referenzieren ist die Sorte Zeile, die genau hier schon
                // einmal die CI gekostet hat. Ein Array aus `char` ist
                // `Copy`; das Kopieren kostet nichts.
                let namensfeld = o.szApplicationName;
                let name = namensfeld
                    .iter()
                    .take_while(|c| **c != 0)
                    .map(|c| *c as u8 as char)
                    .collect::<String>();
                // ⚠ Erst kopieren, dann formatieren. `SIMCONNECT_RECV_OPEN`
                // ist ein GEPACKTER Typ, und `format!` nimmt Referenzen auf
                // seine Argumente — eine Referenz auf ein Feld darin ist
                // nicht ausgerichtet (E0793). Ein Lesen BY VALUE ist
                // erlaubt, wie ein paar Zeilen tiefer bei `exc.dwException`.
                let major = o.dwApplicationVersionMajor;
                let minor = o.dwApplicationVersionMinor;
                Some(DispatchMsg::Open {
                    kennung: format!("{} {}.{}", name.trim(), major, minor),
                })
            }
            sys::SIMCONNECT_RECV_ID_QUIT => Some(DispatchMsg::Quit),
            sys::SIMCONNECT_RECV_ID_EXCEPTION => {
                let exc = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_EXCEPTION) };
                Some(DispatchMsg::Exception {
                    exception: exc.dwException,
                    send_id: exc.dwSendID,
                    index: exc.dwIndex,
                })
            }
            sys::SIMCONNECT_RECV_ID_SIMOBJECT_DATA => {
                // dwData[1] in the SDK header — first byte of the
                // payload — is at the same offset as
                // `SIMCONNECT_RECV_SIMOBJECT_DATA::dwData`. We copy
                // the bytes out so the dispatch ptr can be reused.
                let recv_data = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_SIMOBJECT_DATA) };
                let request_id = recv_data.dwRequestID;
                let header_size = std::mem::size_of::<sys::SIMCONNECT_RECV_SIMOBJECT_DATA>();
                let total = cb_data as usize;
                if total < header_size {
                    return Ok(None);
                }
                let payload_start = header_size - std::mem::size_of::<sys::DWORD>();
                let payload_len = total - payload_start;
                let bytes = unsafe {
                    let base = p_data as *const u8;
                    std::slice::from_raw_parts(base.add(payload_start), payload_len)
                };
                Some(DispatchMsg::SimObjectData {
                    request_id,
                    bytes: bytes.to_vec(),
                })
            }
            id if id == SIMCONNECT_RECV_ID_CLIENT_DATA => {
                // ClientData has the same payload layout as
                // SimObjectData. bindgen represents the C++ class
                // inheritance as `_base` — `_base.dwRequestID` is
                // the field on the parent SIMOBJECT_DATA struct.
                let recv_data = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_CLIENT_DATA) };
                let request_id = recv_data._base.dwRequestID;
                let header_size = std::mem::size_of::<sys::SIMCONNECT_RECV_CLIENT_DATA>();
                let total = cb_data as usize;
                if total < header_size {
                    return Ok(None);
                }
                let payload_start = header_size - std::mem::size_of::<sys::DWORD>();
                let payload_len = total - payload_start;
                let bytes = unsafe {
                    let base = p_data as *const u8;
                    std::slice::from_raw_parts(base.add(payload_start), payload_len)
                };
                Some(DispatchMsg::ClientData {
                    request_id,
                    bytes: bytes.to_vec(),
                })
            }
            id if id == SIMCONNECT_RECV_ID_SYSTEM_STATE => {
                // szString is a fixed-size char buffer in the
                // SIMCONNECT_RECV_SYSTEM_STATE struct. For
                // AircraftLoaded that's the .air file path (Windows
                // path with backslashes). We read it as a NUL-
                // terminated C-string.
                let recv = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_SYSTEM_STATE) };
                let request_id = recv.dwRequestID;
                // szString length is implementation-defined in the
                // SDK; SimConnect docs guarantee NUL-termination.
                let cstr = unsafe { std::ffi::CStr::from_ptr(recv.szString.as_ptr()) };
                let air_path = cstr.to_string_lossy().to_string();
                Some(DispatchMsg::SystemState {
                    request_id,
                    air_path,
                })
            }
            id if id == SIMCONNECT_RECV_ID_EVENT => {
                let evt = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_EVENT) };
                Some(DispatchMsg::SystemEvent {
                    event_id: evt.uEventID,
                    data: evt.dwData,
                })
            }
            id if id == SIMCONNECT_RECV_ID_FLOW_EVENT => {
                let evt = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_FLOW_EVENT) };
                Some(DispatchMsg::FlowEvent {
                    event: evt.FlowEvent as u32,
                })
            }
            // v1.7.8 — ein Element der Facility-Lieferung (Bahn,
            // Rollwegpunkt, …). Die Nutzdaten haengen als variabler
            // Schwanz hinter der Struktur; `SIMCONNECT_DATAV` ist ein
            // Ein-Element-Feld, das in Wahrheit weiterlaeuft.
            //
            // ⚠ `dwSize` ist die GESAMTgroesse der Nachricht. Die
            // Nutzdaten beginnen dort, wo das Feld `Data` liegt — nicht
            // am Ende der Struktur, denn `Data` IST schon Teil von ihr.
            id if id == sys::SIMCONNECT_RECV_ID_FACILITY_DATA => {
                let fd = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_FACILITY_DATA) };
                let kopf = std::mem::offset_of!(sys::SIMCONNECT_RECV_FACILITY_DATA, Data);
                let gesamt = fd._base.dwSize as usize;
                let laenge = gesamt.saturating_sub(kopf);
                let bytes =
                    unsafe { std::slice::from_raw_parts((p_data as *const u8).add(kopf), laenge) }
                        .to_vec();
                Some(DispatchMsg::FacilityData {
                    request_id: fd.UserRequestId,
                    typ: fd.Type,
                    ist_listeneintrag: fd.IsListItem != 0,
                    index: fd.ItemIndex,
                    bytes,
                })
            }
            id if id == sys::SIMCONNECT_RECV_ID_FACILITY_DATA_END => {
                let fd = unsafe { &*(p_data as *const sys::SIMCONNECT_RECV_FACILITY_DATA_END) };
                Some(DispatchMsg::FacilityDataEnde {
                    request_id: fd.RequestId,
                })
            }
            _ => None,
        };
        Ok(msg)
    }
}

// SimConnect RECV_ID constants we look up dynamically — `sys.rs`
// doesn't yet export these as named DWORD constants because they
// were added with the PMDG SDK work. Pulled directly from the
// bindgen output.
const SIMCONNECT_RECV_ID_CLIENT_DATA: sys::DWORD =
    sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_CLIENT_DATA as sys::DWORD;
const SIMCONNECT_RECV_ID_SYSTEM_STATE: sys::DWORD =
    sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_SYSTEM_STATE as sys::DWORD;
const SIMCONNECT_RECV_ID_EVENT: sys::DWORD =
    sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_EVENT as sys::DWORD;
const SIMCONNECT_RECV_ID_FLOW_EVENT: sys::DWORD =
    sys::SIMCONNECT_RECV_ID_SIMCONNECT_RECV_ID_FLOW_EVENT as sys::DWORD;

impl Drop for Connection {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            unsafe { sys::SimConnect_Close(self.handle) };
            self.handle = std::ptr::null_mut();
        }
    }
}

unsafe impl Send for Connection {}
unsafe impl Sync for Connection {}

#[derive(Debug)]
enum DispatchMsg {
    Open {
        /// Name + Version, wie der Simulator sich meldet.
        kennung: String,
    },
    Quit,
    Exception {
        exception: u32,
        send_id: u32,
        index: u32,
    },
    SimObjectData {
        request_id: u32,
        bytes: Vec<u8>,
    },
    /// PMDG ClientData arrived (or any other ClientData if we ever
    /// subscribe to additional channels). RECV_ID is
    /// `SIMCONNECT_RECV_ID_CLIENT_DATA = 16`. Same byte-layout as
    /// SimObjectData but a different RECV_ID.
    /// v1.6.12 — Flow-Ereignis des SDK (RECV_ID 27). Der Simulator meldet
    /// Replay, Teleport, Vorspulen und aehnliche Vorgaenge, waehrend derer
    /// die Telemetrie nicht den geflogenen Zustand beschreibt.
    FlowEvent {
        event: u32,
    },
    /// v1.7.8 — ein Element aus einer Facility-Lieferung. `typ` ist eine
    /// `SIMCONNECT_FACILITY_DATA_TYPE`; `bytes` folgt der Definition,
    /// die wir vorher zusammengesetzt haben.
    FacilityData {
        request_id: u32,
        typ: u32,
        ist_listeneintrag: bool,
        index: u32,
        bytes: Vec<u8>,
    },
    /// Ende einer Facility-Lieferung.
    FacilityDataEnde {
        request_id: u32,
    },
    ClientData {
        request_id: u32,
        bytes: Vec<u8>,
    },
    /// Response to `RequestSystemState`. We use this to read the
    /// `.air` file path of the loaded aircraft for PMDG variant
    /// detection. The `request_id` will be `AIRCRAFT_LOADED_REQUEST_ID`.
    SystemState {
        request_id: u32,
        air_path: String,
    },
    /// Subscribed system event fired (e.g. `SimStart` when the user
    /// loads a new flight or changes aircraft). On a SimStart we
    /// re-request AircraftLoaded to pick up any variant change.
    /// SimConnect-System-Event (= subscribed via
    /// `SimConnect_SubscribeToSystemEvent`). `data` ist der `dwData`-Wert
    /// aus `SIMCONNECT_RECV_EVENT` — Bedeutung event-spezifisch.
    /// Fuer `Pause_EX1` ist es das Flag-Set (siehe PAUSE_EX1_EVENT_ID-
    /// Konstanten-Doku), fuer `SimStart` u.a. 0.
    SystemEvent {
        event_id: u32,
        data: u32,
    },
}

// Marker so the file always references kind/Utc when stub'd out.
#[allow(dead_code)]
fn _link_assertions() {
    let _ = Utc::now();
    let _ = Simulator::Msfs2024;
    let _ = AircraftProfile::Default;
}
