//! Simulator-agnostic abstractions.
//!
//! All simulator adapters (`sim-msfs`, `sim-xplane`, future `sim-prepar3d`, …) implement
//! the `SimAdapter` trait and emit `SimSnapshot`s at a configurable rate. The flight phase
//! FSM and recorder consume these snapshots without knowing which simulator they came from.

#![allow(dead_code)] // Phase 1 stub.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// One sample of simulator telemetry.
///
/// Field set tracks the requirements spec §8 ("Simulator-Daten und Telemetrie").
/// Adapters fill what's available; downstream consumers tolerate `None`.
///
/// `Default` is provided to make unit testing of the FSM and detectors
/// painless — tests build a baseline snapshot via `SimSnapshot::default()`
/// and override only the fields they care about. The default body is
/// "parked, engines off, on the ground at 0/0" — never produced by a
/// real simulator, but a stable reference state. Manual impl because
/// `chrono::DateTime` doesn't impl Default; we use the unix epoch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimSnapshot {
    pub timestamp: DateTime<Utc>,

    // Position
    pub lat: f64,
    pub lon: f64,
    pub altitude_msl_ft: f64,
    pub altitude_agl_ft: f64,
    /// v0.7.17 (B-003): MSFS `INDICATED ALTITUDE` SimVar — what the
    /// cockpit PFD reads with the current baro setting. Diverges from
    /// `altitude_msl_ft` (geometric MSL) by 1–2k ft in arctic cold or
    /// strong ISA deviations. `None` for sims that don't wire it.
    #[serde(default)]
    pub altitude_indicated_ft: Option<f64>,
    /// v0.7.17 (B-003): MSFS `PRESSURE ALTITUDE` SimVar — always STD
    /// (29.92 inHg / 1013 hPa). What Mode-C transponders and VATSIM
    /// transmit. `None` for sims that don't wire it.
    #[serde(default)]
    pub altitude_pressure_ft: Option<f64>,

    // Attitude / motion
    pub heading_deg_true: f32,
    pub heading_deg_magnetic: f32,
    pub pitch_deg: f32,
    pub bank_deg: f32,
    /// Vertical speed for DISPLAY / phase-FSM / approach-stability (fpm).
    /// On X-Plane this is the instrument VVI (`vvi_fpm_pilot`), which reads ~0
    /// in level flight; the previous source (`local_vy`, OpenGL world-frame)
    /// carried a ground-speed-proportional bias that mis-read level cruise as a
    /// few-hundred-fpm descent. On MSFS this is the true earth-frame `VERTICAL
    /// SPEED` SimVar (unchanged). Use [`Self::touchdown_vs_source_fpm`] for the
    /// responsive touchdown signal.
    pub vertical_speed_fpm: f32,
    /// Raw, lag-free vertical speed for the TOUCHDOWN capture (fpm). On X-Plane
    /// this is `local_vy × 196.85` (responsive, no VSI damping — the v0.4.3
    /// reason for moving off the laggy `vh_ind_fpm`). `None` on sims whose
    /// `vertical_speed_fpm` is already raw+responsive (MSFS `VERTICAL SPEED`),
    /// in which case the touchdown path falls back to `vertical_speed_fpm`.
    #[serde(default)]
    pub vertical_speed_raw_fpm: Option<f32>,
    /// Body-frame velocity components in feet per second.
    /// X = right (+) / left (−) component; Z = forward (+) / aft (−).
    /// Used to compute touchdown sideslip natively
    /// (`atan2(VEL_BODY_X, VEL_BODY_Z) × 180/π`) — same approach as
    /// GEES. None for sims/addons that don't wire them.
    pub velocity_body_x_fps: Option<f32>,
    pub velocity_body_z_fps: Option<f32>,

    // Speeds
    pub groundspeed_kt: f32,
    pub indicated_airspeed_kt: f32,
    pub true_airspeed_kt: f32,
    /// Body-frame wind components in knots. Positive
    /// `aircraft_wind_x_kt` = wind from the right (= crosswind from
    /// the right side). Positive `aircraft_wind_z_kt` = tailwind.
    /// MSFS gives us these natively rotated to airframe axes —
    /// saves us computing wind-vs-heading at PIREP time. None when
    /// the SimVar isn't wired.
    pub aircraft_wind_x_kt: Option<f32>,
    pub aircraft_wind_z_kt: Option<f32>,

    // Forces & flags
    pub g_force: f32,
    pub on_ground: bool,
    /// v0.7.19 (Accident-Detection): Adapter-Snapshot-Flag, true wenn
    /// der Sim ein hartes Crash-Signal liefert. MSFS setzt das aus dem
    /// `Crashed` System-Event (gelatcht im Adapter-Shared-State).
    /// X-Plane setzt es in v0.7.19 nicht (= immer false; gemeinsame
    /// Heuristik greift dort statt Sim-Event). `CrashReset` darf den
    /// Adapter-Flag fuer neue Snapshots loeschen — der aktive Flug
    /// behaelt seinen Accident-Latch unabhaengig davon bis Flight-End/
    /// Cleanup. Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md.
    #[serde(default)]
    pub crashed: bool,
    /// v0.7.19: Welcher Pfad hat den `crashed`-Flag gesetzt? Werte:
    /// "msfs_crashed_event" | "xplane_crash_dataref" | "heuristic" |
    /// `None` wenn `crashed=false`.
    #[serde(default)]
    pub crash_source: Option<String>,
    /// v0.4.4: X-Plane only — Normal force on the landing gear (N).
    /// 0 in the air, spikes the moment a wheel touches the runway.
    /// Used by the touchdown sampler for sub-frame-accurate edge
    /// detection (xgs-style). MSFS doesn't provide this — `None` there,
    /// the existing `PLANE TOUCHDOWN NORMAL VELOCITY` SimVar is the
    /// MSFS-equivalent (already wired).
    #[serde(default)]
    pub gear_normal_force_n: Option<f32>,
    pub parking_brake: bool,
    pub stall_warning: bool,
    pub overspeed_warning: bool,
    pub paused: bool,
    pub slew_mode: bool,
    pub simulation_rate: f32,

    // Configuration
    pub gear_position: f32, // 0.0 = up, 1.0 = down
    pub flaps_position: f32,
    pub engines_running: u8,

    // Fuel & weight
    pub fuel_total_kg: f32,
    pub fuel_used_kg: f32,
    pub zfw_kg: Option<f32>,
    pub payload_kg: Option<f32>,
    /// Gross weight in kg (`TOTAL WEIGHT` SimVar). Includes fuel + payload.
    /// `None` for aircraft addons that don't wire it (notably Fenix —
    /// then we fall back to addon LVars or leave the PIREP field blank).
    pub total_weight_kg: Option<f32>,

    // ---- Touchdown sample (latched by MSFS at the moment the gear
    // touches the ground; values stay valid until the next takeoff).
    // Reading these is more reliable than sampling V/S continuously
    // and trying to guess which frame was the actual touchdown — the
    // sim itself takes the snapshot for us. ----
    /// Vertical velocity at touchdown, fpm. Negative on a real landing
    /// (we mirror MSFS' sign convention).
    pub touchdown_vs_fpm: Option<f32>,
    pub touchdown_pitch_deg: Option<f32>,
    pub touchdown_bank_deg: Option<f32>,
    pub touchdown_heading_mag_deg: Option<f32>,
    pub touchdown_lat: Option<f64>,
    pub touchdown_lon: Option<f64>,

    // Environment
    pub wind_direction_deg: Option<f32>,
    pub wind_speed_kt: Option<f32>,
    pub qnh_hpa: Option<f32>,
    pub outside_air_temp_c: Option<f32>,
    /// Total Air Temperature — corrected for compression heating.
    pub total_air_temp_c: Option<f32>,
    /// Current Mach number.
    pub mach: Option<f32>,
    /// Aircraft empty weight in kg, sourced from `EMPTY WEIGHT`.
    /// `None` when the value looks bogus (Asobo's default airliners
    /// return ~1400 kg here — clearly not a real OEW).
    pub empty_weight_kg: Option<f32>,

    // Identity
    pub aircraft_title: Option<String>,
    pub aircraft_icao: Option<String>,
    /// Aircraft registration / tail number as set in the sim (e.g. "D-AILU").
    pub aircraft_registration: Option<String>,
    pub simulator: Simulator,
    pub sim_version: Option<String>,

    // ---- Avionics (Phase H.1) ----
    /// 4-digit transponder / squawk code, e.g. 7000.
    pub transponder_code: Option<u16>,
    /// Active COM1 frequency in MHz (e.g. 121.500).
    pub com1_mhz: Option<f32>,
    pub com2_mhz: Option<f32>,
    pub nav1_mhz: Option<f32>,
    pub nav2_mhz: Option<f32>,

    // ---- Exterior lights ----
    pub light_landing: Option<bool>,
    pub light_beacon: Option<bool>,
    pub light_strobe: Option<bool>,
    pub light_taxi: Option<bool>,
    pub light_nav: Option<bool>,
    pub light_logo: Option<bool>,
    /// 3-state strobe selector: 0 = OFF, 1 = AUTO, 2 = ON. Set on
    /// aircraft profiles whose LVar exposes the position (Fenix).
    /// `None` when only the binary `light_strobe` is available, so
    /// the activity log can fall back to ON/OFF labels.
    pub strobe_state: Option<u8>,

    // ---- Autopilot ----
    pub autopilot_master: Option<bool>,
    pub autopilot_heading: Option<bool>,
    pub autopilot_altitude: Option<bool>,
    pub autopilot_nav: Option<bool>,
    pub autopilot_approach: Option<bool>,
    /// Autothrottle / Airbus A/THR engaged. Only filled by profiles
    /// with a verified state source: Aerosoft A346 (`L:AB_AP_ATHR_
    /// LIGHT_ON` FCU annunciator) and Fenix (`L:S_FCU_ATHR`). `None`
    /// everywhere else (incl. X-Plane) so the activity log stays
    /// silent instead of logging a dead default. `serde(default)`
    /// keeps pre-existing JSONL flight-log replays deserializable.
    #[serde(default)]
    pub autothrottle_on: Option<bool>,

    // ---- Powerplant (totals — per-engine arrays land later) ----
    /// Total fuel-flow across all running engines, kg/h.
    pub fuel_flow_kg_per_h: Option<f32>,

    // ---- Surfaces ----
    /// 0.0..1.0, current position of the spoiler / speed-brake handle.
    /// Drives both ground-spoiler and in-flight speed-brake feedback.
    pub spoilers_handle_position: Option<f32>,
    /// Auto-spoilers armed for landing.
    pub spoilers_armed: Option<bool>,
    /// MSFS pushback state — 0/1/2 = pushing (straight/left/right),
    /// 3 = no pushback (tug disconnected). Drives the Pushback →
    /// TaxiOut FSM transition.
    pub pushback_state: Option<u8>,
    /// Cabin SEAT BELTS sign — 0=OFF, 1=AUTO, 2=ON.
    pub seatbelts_sign: Option<u8>,
    /// Cabin NO SMOKING sign — 0=OFF, 1=AUTO, 2=ON.
    pub no_smoking_sign: Option<u8>,
    /// FCU selected altitude (feet). e.g. 36000.
    pub fcu_selected_altitude_ft: Option<i32>,
    /// FCU selected heading (deg).
    pub fcu_selected_heading_deg: Option<i32>,
    /// FCU selected airspeed (kt).
    pub fcu_selected_speed_kt: Option<i32>,
    /// FCU selected vertical speed (fpm).
    pub fcu_selected_vs_fpm: Option<i32>,
    /// Autobrake setting — None means we don't know, "OFF"/"LO"/
    /// "MED"/"MAX" otherwise.
    pub autobrake: Option<String>,

    // ---- Systems ----
    pub apu_switch: Option<bool>,
    /// 0..100. Useful to tell "starting" (rising) from "running" (~95).
    pub apu_pct_rpm: Option<f32>,
    pub battery_master: Option<bool>,
    pub avionics_master: Option<bool>,
    pub pitot_heat: Option<bool>,
    /// "any engine has anti-ice on" — combined per-engine readings.
    pub engine_anti_ice: Option<bool>,
    /// Wing / structural deice (Airbus "WING ANTI ICE").
    pub wing_anti_ice: Option<bool>,
    /// Wing-illumination switch (Boeing). Some Airbus profiles also
    /// expose it. None when the source platform doesn't report it
    /// — generic SimVars / X-Plane base catalog don't, but PMDG
    /// (MSFS) and Zibo/LevelUp 737 (X-Plane) do.
    pub light_wing: Option<bool>,
    /// Wheel-well light switch — Boeing 737-only feature in practice.
    /// None on most aircraft.
    pub light_wheel_well: Option<bool>,
    /// Transponder mode label as shown on the cockpit panel:
    /// "OFF"/"STBY"/"XPNDR"/"TEST"/"ALT"/"TA"/"TA-RA". Empty/None
    /// when the source platform doesn't expose the mode separately
    /// from the squawk code. Filled by both the PMDG SDK (MSFS) and
    /// the X-Plane standard `transponder_mode` DataRef.
    pub xpdr_mode_label: Option<String>,
    /// Boeing "TAKEOFF CONFIG" annunciator — fires on the ground
    /// when the aircraft is mis-configured for takeoff (flaps/trim/
    /// parking-brake/spoilers wrong). 737 NG3 + Zibo/LevelUp 737
    /// expose this via SDK / `laminar/B738/annunciator/takeoff_config`.
    /// None on aircraft that don't have an EICAS takeoff-config check.
    pub takeoff_config_warning: Option<bool>,

    // ---- ATC / Gate info (from MSFS ATC system) ----
    /// Stand identifier from `ATC PARKING NAME` (e.g. "GATE_HEAVY").
    /// Only filled while the aircraft sits on a named stand; goes
    /// empty after pushback. The recorder snapshots it at flight
    /// start (departure gate) and on BlocksOn (arrival gate).
    pub parking_name: Option<String>,
    /// Stand number suffix (e.g. "12", "A 8") to combine with the name.
    pub parking_number: Option<String>,
    /// `ATC RUNWAY SELECTED` — the runway the MSFS ATC system
    /// currently has the aircraft cleared for. Useful to record the
    /// approach runway at touchdown.
    pub selected_runway: Option<String>,

    // ---- Aircraft profile (Phase H.4) ----
    /// Detected aircraft profile. Drives which set of variables (default
    /// MSFS SimVars vs add-on-specific LVars) the adapter reads from.
    /// `Default` covers Asobo + most payware that pipes state into the
    /// standard SimVars (e.g. PMDG 737/777 mostly do); the named variants
    /// cover study-level add-ons that pull state from their own LVars.
    pub aircraft_profile: AircraftProfile,

    // ---- PMDG SDK premium telemetry (Phase H.4) ----
    /// Live cockpit state from a PMDG aircraft's SimConnect SDK
    /// (737 NG3 or 777X). `None` when no PMDG aircraft is loaded
    /// OR the user hasn't enabled `EnableDataBroadcast=1` in the
    /// aircraft's options ini. When `Some`, the consumer (FSM,
    /// activity log, PIREP fields) gets cockpit values that
    /// standard MSFS SimVars don't expose: A/T FMA modes, MCP
    /// selected values, FMC-computed V-speeds, exact flap angle.
    /// See `pmdg-sdk-integration.md` for the full mapping.
    pub pmdg: Option<PmdgState>,

    // ---- Gear type / surface contact (category-aware landing) ----
    // Live signals that distinguish rotorcraft / seaplane / amphibian from
    // wheeled fixed-wing so the touchdown pipeline can branch. All `None`
    // when the sim doesn't expose them (older JSONL replays, X-Plane with
    // the Web API off). Purely additive — fixed-wing behaviour is unchanged.
    /// MSFS `IS GEAR SKIDS` (true ⇒ rotorcraft hint). X-Plane sets this from
    /// `sim/aircraft/gear/acf_gear_is_skid`.
    #[serde(default)]
    pub gear_is_skid: Option<bool>,
    /// MSFS `IS GEAR FLOATS` (true ⇒ seaplane/float hint). None on X-Plane
    /// (no float boolean — we infer water-capability from the water rudder).
    #[serde(default)]
    pub gear_is_floats: Option<bool>,
    /// MSFS `IS GEAR WHEELS`. Lets us tell an amphibian (floats+wheels) from
    /// a pure seaplane. None on X-Plane (no equivalent dataref).
    #[serde(default)]
    pub gear_is_wheels: Option<bool>,
    /// MSFS `CONTACT POINT IS ON GROUND` aggregated over the contact points —
    /// true if any wheel/skid/float is on the surface. Unlike the wheeled
    /// `on_ground` flag this asserts on WATER too, so it is the
    /// surface-agnostic touchdown signal for seaplanes/floats. None on
    /// X-Plane (no per-contact-point ground dataref confirmed in the SDK).
    #[serde(default)]
    pub contact_point_on_ground: Option<bool>,
    /// MSFS `GEAR WATER DEPTH` converted to metres — depth of the gear/floats
    /// in the water. >0 when immersed → a positive water-contact signal for a
    /// seaplane touchdown. Reads 0 when floats skip across water at speed, so
    /// it supplements (does not replace) the descent-arrest heuristic. None on
    /// X-Plane.
    #[serde(default)]
    pub gear_water_depth_m: Option<f32>,
    /// Water rudder present — MSFS `WATER RUDDER HANDLE POSITION` != -1, or
    /// X-Plane `acf_water_rud_area` > 0. A static seaplane/amphibian
    /// discriminator (NOT a touchdown detector — it reflects a handle
    /// position, not water contact).
    #[serde(default)]
    pub water_rudder_present: Option<bool>,

    // ---- v0.16.10 (#Premium): Autoflight-/Cockpit-Tiefendaten ----
    // Premium cockpit state from study-level addons (FMA modes, V-speeds,
    // warnings, …). Carrier fields only in this phase — the per-addon
    // mappers (PMDG SDK, FBW/Fenix/iniBuilds LVars, TFDi) fill them in
    // later phases. All `Option` + `serde(default)` so existing JSONL
    // replays and every downstream consumer stay compatible; `None` =
    // "source doesn't expose it", never "off".
    /// Normalized FMA lateral/roll-mode label, e.g. "LNAV", "LOC", "NAV".
    #[serde(default)]
    pub fma_lateral_mode: Option<String>,
    /// Normalized FMA vertical/pitch-mode label, e.g. "VNAV PTH", "G/S", "ALT".
    #[serde(default)]
    pub fma_vertical_mode: Option<String>,
    /// Normalized FMA thrust/speed-mode label, e.g. "N1", "SPEED",
    /// "THR CLB", "MAN FLX".
    #[serde(default)]
    pub fma_thrust_mode: Option<String>,
    /// Aircraft-authoritative FMGC/FWC flight-phase label (Airbus
    /// "TAKEOFF"/"CLIMB"/…). Info only — never drives our phase FSM.
    #[serde(default)]
    pub flight_phase_aircraft: Option<String>,
    /// Takeoff decision speed V1 (kt), as entered/computed in the FMS.
    #[serde(default)]
    pub v1_kt: Option<f64>,
    /// Rotation speed VR (kt).
    #[serde(default)]
    pub vr_kt: Option<f64>,
    /// Takeoff safety speed V2 (kt).
    #[serde(default)]
    pub v2_kt: Option<f64>,
    /// Approach speed VAPP (kt, Airbus).
    #[serde(default)]
    pub vapp_kt: Option<f64>,
    /// Lowest selectable speed VLS (kt, Airbus).
    #[serde(default)]
    pub vls_kt: Option<f64>,
    /// Landing reference speed VREF (kt, Boeing).
    #[serde(default)]
    pub vref_kt: Option<f64>,
    /// FLEX / assumed temperature (°C). `Some(>0)` ⇒ FLEX/derated takeoff.
    #[serde(default)]
    pub flex_temp_c: Option<f64>,
    /// Thrust-lever detent label: "TOGA"/"FLX/MCT"/"CL"/"MAN".
    #[serde(default)]
    pub thrust_gate: Option<String>,
    /// MASTER CAUTION annunciator lit.
    #[serde(default)]
    pub master_caution: Option<bool>,
    /// MASTER WARNING annunciator lit.
    #[serde(default)]
    pub master_warning: Option<bool>,
    /// Airbus managed (dot) vs. selected speed target on the FCU.
    #[serde(default)]
    pub managed_speed: Option<bool>,
    /// Airbus managed vs. selected heading (NAV vs. HDG).
    #[serde(default)]
    pub managed_heading: Option<bool>,
    /// Airbus managed vs. selected altitude.
    #[serde(default)]
    pub managed_altitude: Option<bool>,
    /// Any engine thrust reverser unlocked/deployed.
    #[serde(default)]
    pub reverser_deployed: Option<bool>,
    /// Auto ground-spoiler deployment at touchdown/RTO — distinct from
    /// `spoilers_armed` (armed ≠ deployed) and from an in-flight speedbrake.
    #[serde(default)]
    pub ground_spoilers_active: Option<bool>,
    /// Per-engine N1 in % from premium sources (len = engine count).
    #[serde(default)]
    pub eng_n1_pct: Option<Vec<f64>>,
    /// Baro reference is STD (above transition) vs. QNH.
    #[serde(default)]
    pub baro_std: Option<bool>,
    /// Per-tank fuel in kg (tank order is addon-specific — for the
    /// imbalance display, not for totals; `fuel_total_kg` stays the sum).
    #[serde(default)]
    pub fuel_per_tank_kg: Option<Vec<f64>>,
    /// Below-glideslope annunciator (PMDG etc.).
    #[serde(default)]
    pub below_gs_alert: Option<bool>,
    /// Cabin-altitude warning horn/annunciator.
    #[serde(default)]
    pub cabin_altitude_warning: Option<bool>,
    /// Stabilizer out-of-trim annunciator.
    #[serde(default)]
    pub stab_out_of_trim: Option<bool>,
    /// Numeric baro minimums (DA/MDA, ft) where exposed (PMDG 777).
    #[serde(default)]
    pub minimums_baro_ft: Option<f64>,

    // ---- v0.16.12 (#phase-v2): Schatten-Phasen-Engine ----
    // Beide Felder werden vom STREAMER nach dem Schatten-Engine-Tick
    // gestempelt — NIE von den Sim-Adaptern (die bauen den Snapshot mit
    // `None`). Sie landen so additiv in der Position-JSONL (lokaler
    // Recorder → VPS-Log-Upload), womit der Schatten-Diff alte-FSM-Phase
    // (PhaseChanged-Events) vs. v2-Sicht pro Tick auswertbar ist.
    // `serde(default)` + skip-if-None: alte JSONLs deserialisieren
    // unverändert, Nicht-Flug-Snapshots bleiben sauber.
    /// snake_case-Phase der Schatten-Engine v2 (z. B. "climb").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_phase: Option<String>,
    /// Fenster-Segment der Schatten-Engine v2 ("ground" / "climbing" /
    /// "level" / "descending" / "insufficient").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow_segment: Option<String>,
}

/// PMDG aircraft "premium telemetry" — generic across 737 NG3 and
/// 777X. The `sim-msfs` crate fills this struct from variant-
/// specific raw data (`Pmdg738RawData` / `Pmdg777XRawData`); the
/// FSM and PIREP code consume this generic shape so they don't
/// need PMDG-variant-specific code paths.
///
/// Field choice: only the cockpit values that EITHER aren't in
/// standard MSFS SimVars at all (FMA modes, MCP selected speed
/// when blanked, FMC V-speeds) OR are more accurate from PMDG
/// (autobrake setting, flap angle in degrees vs. handle ratio).
///
/// `Default` (all-off / all-None) exists for tests that need a
/// PmdgState carrier without filling ~50 fields — production code
/// always builds it from a real SDK raw block in `sim-msfs`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PmdgState {
    /// Which PMDG variant produced this state — useful for the
    /// activity log ("PMDG 737-800: …") and for PIREP custom
    /// fields that want to record the exact aircraft.
    pub variant_label: String,

    // ---- MCP (Mode Control Panel) — selected autopilot targets ----
    /// MCP IAS/Mach window value. Above 10.0 = knots; below 10.0
    /// = Mach. None when the MCP is blanked.
    pub mcp_speed_raw: Option<f32>,
    /// MCP heading bug, degrees true. None when MCP is unpowered.
    pub mcp_heading_deg: Option<u16>,
    /// MCP altitude window, feet. None when MCP is unpowered.
    pub mcp_altitude_ft: Option<u16>,
    /// MCP V/S window, fpm. None when V/S is blanked.
    pub mcp_vs_fpm: Option<i16>,

    // ---- FMA (Flight Mode Annunciator) — active autoflight modes ----
    /// Speed-mode label as shown on FMA. Empty string when no
    /// speed mode is engaged (= cockpit shows nothing). Possible
    /// values: "N1", "SPD", "TOGA" (when AT pushed at takeoff),
    /// "RETARD", "IDLE", etc. — exact labels are variant-specific.
    pub fma_speed_mode: String,
    /// Roll-mode label: "HDG SEL", "HDG HOLD", "LNAV", "VOR/LOC",
    /// "APP", "" (none). Non-empty only when AP is engaged.
    pub fma_roll_mode: String,
    /// Pitch-mode label: "VNAV", "LVL CHG", "ALT HOLD", "VS",
    /// "APP", "" (none).
    pub fma_pitch_mode: String,
    /// Auto-Throttle armed.
    pub at_armed: bool,
    /// At least one AP CMD channel engaged (CMD_A or CMD_B).
    pub ap_engaged: bool,
    /// Flight Director on (either side).
    pub fd_on: bool,

    // ---- FMC (Flight Management Computer) — pilot's plan ----
    /// FMC takeoff flap setting in degrees. None if not entered.
    pub fmc_takeoff_flaps_deg: Option<u8>,
    /// FMC landing flap setting in degrees. None if not entered.
    pub fmc_landing_flaps_deg: Option<u8>,
    /// FMC V1 speed in knots. None if not entered.
    pub fmc_v1_kt: Option<u8>,
    /// FMC VR (rotate) speed in knots.
    pub fmc_vr_kt: Option<u8>,
    /// FMC V2 (takeoff safety) speed in knots.
    pub fmc_v2_kt: Option<u8>,
    /// FMC VREF (landing reference) speed in knots.
    pub fmc_vref_kt: Option<u8>,
    /// FMC cruise altitude, feet. None if not set.
    pub fmc_cruise_alt_ft: Option<u16>,
    /// Distance to top of descent, nautical miles. None when
    /// already past TOD or not yet computed.
    pub fmc_distance_to_tod_nm: Option<f32>,
    /// Distance to destination, nautical miles. None when not
    /// yet computed.
    pub fmc_distance_to_dest_nm: Option<f32>,
    /// FMC flight number (whatever the pilot entered in the FMC).
    /// Empty if not set. Useful as a sanity check vs. the bid-
    /// supplied flight number — mismatch = pilot loaded the
    /// wrong route.
    pub fmc_flight_number: String,
    /// True when the pilot has completed the FMC PERF-INIT page.
    /// Pre-flight checklist signal.
    pub fmc_perf_input_complete: bool,

    // ---- Controls — flaps, gear, autobrake (PMDG-precise) ----
    /// Trailing-edge flap angle in degrees (live, not handle
    /// position). For Boeing 737: 0/1/2/5/10/15/25/30/40 detents.
    pub flap_angle_deg: f32,
    /// Boeing flap-handle label (Premium-First). NG3 detents:
    /// "UP"/"1"/"2"/"5"/"10"/"15"/"25"/"30"/"40". 777X detents:
    /// "UP"/"1"/"5"/"15"/"20"/"25"/"30". Empty when not applicable.
    /// Activity-log uses this directly instead of the Airbus-style
    /// quantisation of `flaps_position`.
    pub flap_handle_label: String,
    /// Speedbrake/spoiler handle position normalised 0.0..1.0. Used
    /// to override the standard `spoilers_handle_position` SimVar
    /// which jitters near the ARMED detent. None when not available.
    pub speedbrake_lever_pos: Option<f32>,
    /// Autobrake setting label: "RTO" / "OFF" / "1" / "2" / "3"
    /// / "MAX". The values vary by variant (777 has 4 LO/MED/...
    /// settings) so we keep this as a label string.
    pub autobrake_label: String,
    /// True if the pilot has the speedbrake armed for landing.
    pub speedbrake_armed: bool,
    /// True if the speedbrake is currently extended in flight.
    pub speedbrake_extended: bool,
    /// Transponder mode label as shown on the cockpit panel:
    /// "STBY"/"ALT-OFF"/"XPNDR"/"TA"/"TA-RA". Empty when not
    /// applicable. PMDG-cockpit-authoritative — the standard
    /// `TRANSPONDER STATE` SimVar only exposes the binary on/off.
    pub xpdr_mode_label: String,
    /// Cockpit "TAKEOFF CONFIG" warning is active. If true at
    /// takeoff roll start, the PIREP gets a flag.
    pub takeoff_config_warning: bool,

    // ---- 777-specific extras (None for NG3) ----
    /// FMC thrust-limit mode label as shown on the EICAS:
    /// "TO" / "TO 1" / "TO 2" / "CLB" / "CRZ" / "CON" / "G/A"
    /// / "D-TO" / "A-TO" etc. Empty when not applicable
    /// (NG3 doesn't expose this).
    pub thrust_limit_mode: String,
    /// Electronic Checklist completion state — 10 phases.
    /// `true` at index N = pilot has marked phase N complete in
    /// the cockpit checklist. None for NG3 (no ECL there).
    /// Phases:
    ///   0=PREFLIGHT 1=BEFORE_START 2=BEFORE_TAXI
    ///   3=BEFORE_TAKEOFF 4=AFTER_TAKEOFF 5=DESCENT
    ///   6=APPROACH 7=LANDING 8=SHUTDOWN 9=SECURE
    pub ecl_complete: Option<[bool; 10]>,
    /// APU running per the SDK's authoritative bit (more accurate
    /// than the RPM heuristic on the standard SimVar). None when
    /// the variant doesn't expose it (NG3 derives from standard
    /// SimVars instead).
    pub apu_running: Option<bool>,
    /// Wheel chocks set at the gate (777-specific). None for NG3.
    pub wheel_chocks_set: Option<bool>,

    // ---- Cockpit-state overrides for the Standard SimSnapshot ----
    //
    // These are the same things the standard MSFS SimVars expose
    // (LIGHT LANDING, BATTERY MASTER, etc.) but read directly from
    // the PMDG cockpit-state struct — guaranteed real-time vs. the
    // standard SimVars which can lag during cold-start. The adapter
    // overrides the matching SimSnapshot fields when these are set,
    // so the existing activity-log change detection automatically
    // uses the better values without any branching downstream.
    /// Light switches mirrored from PMDG_*_Sw_ON / LTS_*Sw fields.
    pub light_landing: Option<bool>,
    pub light_beacon: Option<bool>,
    pub light_strobe: Option<bool>,
    pub light_taxi: Option<bool>,
    pub light_nav: Option<bool>,
    pub light_logo: Option<bool>,
    pub light_wing: Option<bool>,
    /// NG3-only: separate "WHEEL WELL" light switch (no Standard
    /// SimVar exists for this — pure PMDG bonus).
    pub light_wheel_well: Option<bool>,
    /// Anti-ice: WING ANTI-ICE switch position.
    pub wing_anti_ice: Option<bool>,
    /// Anti-ice: at least one ENG ANTI-ICE switch is ON.
    pub engine_anti_ice: Option<bool>,
    /// Pitot/probe heat (combined).
    pub pitot_heat: Option<bool>,
    /// Battery master switch state.
    pub battery_master: Option<bool>,
    /// Parking brake set (PMDG-cockpit-authoritative).
    pub parking_brake: Option<bool>,

    // ---- v0.16.10 (#Premium): zusaetzliche SDK-Felder ----
    // Carrier-Felder fuer Werte, die der PMDG-SDK-Struct bereits
    // mitsendet — die sim-msfs-Mapper fuellen sie in Phase 3. Alle
    // `Option` + `serde(default)`, damit aeltere serialisierte
    // PmdgState-Bloecke (JSONL-Replays) weiter deserialisieren.
    // V-Speeds + FMA-Modi existieren oben schon (`fmc_*_kt`,
    // `fma_*_mode`) und werden NICHT dupliziert.
    /// Any engine thrust reverser unlocked/deployed.
    #[serde(default)]
    pub reverser_deployed: Option<bool>,
    /// MASTER CAUTION annunciator lit.
    #[serde(default)]
    pub master_caution: Option<bool>,
    /// MASTER WARNING annunciator lit.
    #[serde(default)]
    pub master_warning: Option<bool>,
    /// Below-glideslope (GPWS "BELOW G/S") annunciator.
    #[serde(default)]
    pub below_gs: Option<bool>,
    /// Cabin-altitude warning active.
    #[serde(default)]
    pub cabin_altitude_warning: Option<bool>,
    /// Stabilizer out-of-trim annunciator.
    #[serde(default)]
    pub stab_out_of_trim: Option<bool>,
    /// Per-tank fuel in kg (PMDG tank order).
    #[serde(default)]
    pub fuel_per_tank_kg: Option<Vec<f64>>,
    /// Numeric baro minimums (DA/MDA, ft) — 777 exposes this.
    #[serde(default)]
    pub minimums_baro_ft: Option<f64>,
    /// GPWS ground-proximity warning active (carrier only — no
    /// generic SimSnapshot field yet; consumers read it via `pmdg`).
    #[serde(default)]
    pub gnd_prox_warning: Option<bool>,
}

impl SimSnapshot {
    /// Vertical speed the TOUCHDOWN capture should use: the raw, lag-free signal
    /// when the adapter provides one (X-Plane `local_vy`), otherwise the display
    /// V/S (MSFS, whose `VERTICAL SPEED` SimVar is already raw + responsive).
    /// This keeps touchdown detection responsive (no VSI lag) while
    /// `vertical_speed_fpm` is the curvature-free value for display / phase-FSM /
    /// approach-stability.
    pub fn touchdown_vs_source_fpm(&self) -> f32 {
        self.vertical_speed_raw_fpm.unwrap_or(self.vertical_speed_fpm)
    }

    /// Premium-First override for the autoflight booleans (v0.16.7).
    ///
    /// Data-audit finding (2026-06-11, 426 flights): the standard
    /// `AUTOPILOT MASTER` SimVar reads permanently false on the PMDG
    /// 737/777 (24 audited flights) — PMDG drives its autoflight purely
    /// through the SDK, so the "Autopilot ENGAGED/OFF" and "A/THR"
    /// activity-log lines never fired for those pilots. The real signals
    /// are already in the SDK snapshot we receive: `ap_engaged`
    /// (737 `MCP_annunCMD_A || MCP_annunCMD_B`, 777 `MCP_annunAP` L/R)
    /// and `at_armed` (737 `MCP_annunATArm`, 777 `MCP_annunAT`).
    ///
    /// Semantics — same standard-var-OR-tiebreaker pattern as the
    /// A346 / Fenix LVar mappings in the sim-msfs telemetry parser:
    ///   * presence-gated: when `pmdg` is `None` (non-PMDG aircraft,
    ///     SDK broadcast off, X-Plane, old JSONL replays) NOTHING
    ///     changes — proven by the tests below;
    ///   * `autopilot_master = standard || pmdg.ap_engaged` — if the
    ///     standard SimVar is ever wired it can only agree or win;
    ///   * `autothrottle_on = standard || pmdg.at_armed`. The NG3 SDK
    ///     only exposes the MCP "A/T ARM" annunciator (lit while armed
    ///     AND while engaged, off after disconnect) — there is no
    ///     separate "engaged" bit in `PMDG_NG3_SDK.h`, so ARM is the
    ///     closest cockpit truth and is mapped knowingly. The 777
    ///     `MCP_annunAT` is the A/T engage-button light.
    ///
    /// Called by `MsfsAdapter::snapshot()` right after the PMDG state is
    /// merged. Lives here (not in the Windows-only adapter) so the
    /// mapping is unit-tested on every platform.
    pub fn apply_pmdg_autoflight_override(&mut self) {
        let Some((ap_engaged, at_on)) = self.pmdg.as_ref().map(|p| (p.ap_engaged, p.at_armed))
        else {
            return;
        };
        self.autopilot_master = Some(self.autopilot_master.unwrap_or(false) || ap_engaged);
        self.autothrottle_on = Some(self.autothrottle_on.unwrap_or(false) || at_on);
    }

    /// v0.16.10 (#Premium): Premium-First override for the new cockpit
    /// deep-data fields — sibling of [`Self::apply_pmdg_autoflight_override`],
    /// called right after it in `MsfsAdapter::snapshot()` once the PMDG
    /// ClientData block is merged. Lives here (not in the Windows-only
    /// adapter) so the mapping is unit-tested on every platform.
    ///
    /// Semantics:
    ///   * presence-gated like the autoflight override: `pmdg == None`
    ///     (non-PMDG aircraft, SDK broadcast off, X-Plane, old JSONL
    ///     replays) ⇒ NOTHING changes;
    ///   * per-field PMDG-wins-when-present: a `Some` from the SDK
    ///     replaces the generic value, a `None` (mapper not wired yet —
    ///     Phase 3) keeps whatever is already in the snapshot;
    ///   * FMA: the PMDG columns map onto the generic labels
    ///     (roll → lateral, pitch → vertical, speed → thrust — Boeing's
    ///     FMA column 1 IS the A/T thrust mode). Empty string = the
    ///     cockpit shows nothing ⇒ stays `None`, no empty labels
    ///     downstream;
    ///   * V-speeds come from the existing `fmc_*_kt` fields (FMC
    ///     entries, `Option<u8>` kt) widened to the generic f64 fields;
    ///   * ground spoilers: PMDG only reports "speedbrake extended"
    ///     without ground context. On the ground that's the (auto)
    ///     ground-spoiler deployment → mapped; in the air it's a normal
    ///     in-flight speedbrake → `ground_spoilers_active` stays
    ///     untouched (deliberately simple, see v0.16.10 spec).
    pub fn apply_pmdg_premium_override(&mut self) {
        // v0.16.10 QS (Minor 10): kein PmdgState-Vollklon pro Tick mehr —
        // der Struct traegt etliche Strings/Vecs (variant_label, FMC-
        // Flight-Number, Label-Felder, fuel_per_tank, …). Stattdessen
        // unter dem &-Borrow nur die benoetigten Werte herauskopieren
        // (die 3 FMA-Strings allozieren nur wenn non-empty, der Tank-Vec
        // nur wenn Some), dann den Borrow fallen lassen und zuweisen.
        let non_empty = |s: &str| {
            let s = s.trim();
            (!s.is_empty()).then(|| s.to_string())
        };
        let Some(p) = self.pmdg.as_ref() else {
            return;
        };
        // FMA columns → generic labels. Leerstring = Cockpit zeigt
        // nichts an → None bleibt None.
        let fma_lateral = non_empty(&p.fma_roll_mode);
        let fma_vertical = non_empty(&p.fma_pitch_mode);
        let fma_thrust = non_empty(&p.fma_speed_mode);
        let master_caution = p.master_caution;
        let master_warning = p.master_warning;
        let reverser_deployed = p.reverser_deployed;
        let below_gs = p.below_gs;
        let cabin_altitude_warning = p.cabin_altitude_warning;
        let stab_out_of_trim = p.stab_out_of_trim;
        let minimums_baro_ft = p.minimums_baro_ft;
        let fuel_per_tank_kg = p.fuel_per_tank_kg.clone();
        let (v1, vr, v2, vref) = (p.fmc_v1_kt, p.fmc_vr_kt, p.fmc_v2_kt, p.fmc_vref_kt);
        let speedbrake_extended = p.speedbrake_extended;

        if let Some(m) = fma_lateral {
            self.fma_lateral_mode = Some(m);
        }
        if let Some(m) = fma_vertical {
            self.fma_vertical_mode = Some(m);
        }
        if let Some(m) = fma_thrust {
            self.fma_thrust_mode = Some(m);
        }

        // Warn-/Status-Bits + Minimums: SDK-Wert gewinnt wenn vorhanden.
        self.master_caution = master_caution.or(self.master_caution);
        self.master_warning = master_warning.or(self.master_warning);
        self.reverser_deployed = reverser_deployed.or(self.reverser_deployed);
        self.below_gs_alert = below_gs.or(self.below_gs_alert);
        self.cabin_altitude_warning = cabin_altitude_warning.or(self.cabin_altitude_warning);
        self.stab_out_of_trim = stab_out_of_trim.or(self.stab_out_of_trim);
        self.minimums_baro_ft = minimums_baro_ft.or(self.minimums_baro_ft);
        if fuel_per_tank_kg.is_some() {
            self.fuel_per_tank_kg = fuel_per_tank_kg;
        }

        // FMC V-Speeds (Option<u8> kt) → generische f64-Felder.
        self.v1_kt = v1.map(f64::from).or(self.v1_kt);
        self.vr_kt = vr.map(f64::from).or(self.vr_kt);
        self.v2_kt = v2.map(f64::from).or(self.v2_kt);
        self.vref_kt = vref.map(f64::from).or(self.vref_kt);

        // Ground-Spoiler nur am Boden mappen — in der Luft ist
        // `speedbrake_extended` eine normale Speedbrake, kein
        // Ground-Spoiler-Signal.
        if self.on_ground {
            self.ground_spoilers_active = Some(speedbrake_extended);
        }
    }
}

impl Default for SimSnapshot {
    fn default() -> Self {
        Self {
            timestamp: DateTime::<Utc>::from_timestamp(0, 0).expect("epoch is valid"),
            lat: 0.0,
            lon: 0.0,
            altitude_msl_ft: 0.0,
            altitude_agl_ft: 0.0,
            altitude_indicated_ft: None,
            altitude_pressure_ft: None,
            heading_deg_true: 0.0,
            heading_deg_magnetic: 0.0,
            pitch_deg: 0.0,
            bank_deg: 0.0,
            vertical_speed_fpm: 0.0,
            vertical_speed_raw_fpm: None,
            velocity_body_x_fps: None,
            velocity_body_z_fps: None,
            groundspeed_kt: 0.0,
            indicated_airspeed_kt: 0.0,
            true_airspeed_kt: 0.0,
            aircraft_wind_x_kt: None,
            aircraft_wind_z_kt: None,
            g_force: 1.0,
            on_ground: true,
            crashed: false,
            crash_source: None,
            gear_normal_force_n: None,
            parking_brake: true,
            stall_warning: false,
            overspeed_warning: false,
            paused: false,
            slew_mode: false,
            simulation_rate: 1.0,
            gear_position: 1.0,
            flaps_position: 0.0,
            engines_running: 0,
            fuel_total_kg: 0.0,
            fuel_used_kg: 0.0,
            zfw_kg: None,
            payload_kg: None,
            total_weight_kg: None,
            touchdown_vs_fpm: None,
            touchdown_pitch_deg: None,
            touchdown_bank_deg: None,
            touchdown_heading_mag_deg: None,
            touchdown_lat: None,
            touchdown_lon: None,
            wind_direction_deg: None,
            wind_speed_kt: None,
            qnh_hpa: None,
            outside_air_temp_c: None,
            total_air_temp_c: None,
            mach: None,
            empty_weight_kg: None,
            aircraft_title: None,
            aircraft_icao: None,
            aircraft_registration: None,
            simulator: Simulator::default(),
            sim_version: None,
            transponder_code: None,
            com1_mhz: None,
            com2_mhz: None,
            nav1_mhz: None,
            nav2_mhz: None,
            light_landing: None,
            light_beacon: None,
            light_strobe: None,
            light_taxi: None,
            light_nav: None,
            light_logo: None,
            strobe_state: None,
            autopilot_master: None,
            autopilot_heading: None,
            autopilot_altitude: None,
            autopilot_nav: None,
            autopilot_approach: None,
            autothrottle_on: None,
            fuel_flow_kg_per_h: None,
            spoilers_handle_position: None,
            spoilers_armed: None,
            pushback_state: None,
            apu_switch: None,
            apu_pct_rpm: None,
            battery_master: None,
            avionics_master: None,
            pitot_heat: None,
            engine_anti_ice: None,
            wing_anti_ice: None,
            light_wing: None,
            light_wheel_well: None,
            xpdr_mode_label: None,
            takeoff_config_warning: None,
            seatbelts_sign: None,
            no_smoking_sign: None,
            fcu_selected_altitude_ft: None,
            fcu_selected_heading_deg: None,
            fcu_selected_speed_kt: None,
            fcu_selected_vs_fpm: None,
            autobrake: None,
            parking_name: None,
            parking_number: None,
            selected_runway: None,
            aircraft_profile: AircraftProfile::default(),
            pmdg: None,
            gear_is_skid: None,
            gear_is_floats: None,
            gear_is_wheels: None,
            contact_point_on_ground: None,
            gear_water_depth_m: None,
            water_rudder_present: None,
            // v0.16.10 (#Premium): Cockpit-Tiefendaten — alle None bis
            // ein Premium-Mapper (PMDG SDK / Addon-LVars) sie fuellt.
            fma_lateral_mode: None,
            fma_vertical_mode: None,
            fma_thrust_mode: None,
            flight_phase_aircraft: None,
            v1_kt: None,
            vr_kt: None,
            v2_kt: None,
            vapp_kt: None,
            vls_kt: None,
            vref_kt: None,
            flex_temp_c: None,
            thrust_gate: None,
            master_caution: None,
            master_warning: None,
            managed_speed: None,
            managed_heading: None,
            managed_altitude: None,
            reverser_deployed: None,
            ground_spoilers_active: None,
            eng_n1_pct: None,
            baro_std: None,
            fuel_per_tank_kg: None,
            below_gs_alert: None,
            cabin_altitude_warning: None,
            stab_out_of_trim: None,
            minimums_baro_ft: None,
            // v0.16.12 (#phase-v2): nur der Streamer stempelt die
            // Schatten-Felder — Default/Adapter liefern None.
            shadow_phase: None,
            shadow_segment: None,
        }
    }
}

/// Identifies the active aircraft add-on so the adapter can read the right
/// LVars (and the activity log can show the pilot which mapping is in use).
/// Detection runs on every snapshot but the answer is cached on the adapter
/// — the title only changes when the pilot loads a different airframe.
///
/// Mapping status (Phase H.4 backlog):
///   * `FbwA32nx`   — LVars wired (untested live, FBW not loaded yet).
///     v0.16.10 (#Premium): deckt jetzt die ganze `A32NX_`-LVar-Familie
///     ab — FBW A32NX, FBW A380X, Headwind A339 (alles Forks derselben
///     Codebasis).
///   * `TfdiMd11`   — detection only (v0.16.10 #Premium); Premium-Mapper
///     folgt in einer spaeteren Phase.
///   * `IflyMax8`   — Premium-Mapper wired (v0.16.11): AP CMD A/B,
///     A/T ARM, Master Caution/Fire, Reverser, Ground Spoiler,
///     Cabin-Altitude-Warnung, Autobrake-Selector (WASM-strings +
///     HubHop, Live-Flug-Verifikation steht aus).
///   * `FenixA320`  — Lights / parking brake / flaps wired and verified
///                    in MSFS 2024. AP indicator LVars (`I_FCU_AP*`) were
///                    observed flickering and are intentionally disabled
///                    until a stable source is identified.
///   * `Pmdg737`    — detection only; LVars TBD (PMDG ships its own
///                    SimConnect ClientData SDK, not plain LVars — needs
///                    a separate subscribe path).
///   * `Pmdg777`    — same as 737.
///   * `IniA340`    — detection only; LVar list TBD.
///   * `IniA350`    — AP1/AP2 + A/THR + APPR/LOC via FCU-LED-LVars (v0.16.8, HubHop).
///   * `IniA346Pro` — detection only; LVar list TBD.
///   * `AerosoftA346` — AP-state LVars wired (`L:AB_AP_*_LIGHT_ON`),
///     confirmed via WASM strings analysis 2026-06-10. Engines/fuel-flow
///     need no profile gate — the aircraft serves the standard
///     `EX1`/CORRECTED-FF SimVar variants which the mapping now reads
///     addon-agnostically.
///
/// Cross-cutting issue: ~50 LVar fields in one SimConnectObject macro
/// appears to overflow the read path on Fenix (lights/flaps return 0.0
/// despite live cockpit changes); future work is a runtime-defined LVar
/// reader (likely via `msfs-rs` or MobiFlight WASM client data) so each
/// profile only subscribes to the LVars it actually uses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AircraftProfile {
    /// Standard MSFS SimVars only — works for Asobo and most payware.
    #[default]
    Default,
    /// FlyByWire A32NX (community Airbus). Reads `L:A32NX_*` LVars.
    /// LVar reference: github.com/flybywiresim/aircraft/blob/master/
    /// fbw-a32nx/docs/a320-simvars.md
    /// v0.16.10 (#Premium): EIN Profil fuer die ganze FBW-Familie —
    /// A32NX, FBW A380X und Headwind A339 nutzen dieselbe
    /// `A32NX_`-LVar-Namensfamilie (Forks derselben Codebasis).
    FbwA32nx,
    /// Fenix Simulations A319 v2 (shares the `FNX_32X` SimObject with
    /// the A320 — LVar names are variant-identical, only the airframe
    /// dimensions differ).
    FenixA319,
    /// Fenix Simulations A320 v2. LVars in `FNX32X_Interior.xml` plus
    /// the Fenix knowledge base.
    FenixA320,
    /// Fenix Simulations A321 v2 (same `FNX_32X` SimObject as A320).
    FenixA321,
    /// PMDG 737 (700/800/900). LVars are in PMDG's SDK header.
    Pmdg737,
    /// PMDG 777 (200/300). Same SDK family as the 737.
    Pmdg777,
    /// INIBuilds A340 (Standard Edition).
    IniA340,
    /// INIBuilds A350.
    IniA350,
    /// INIBuilds A340-600 Pro.
    IniA346Pro,
    /// Aerosoft A340-600 Professional (MSFS, ToLiss port). WASM strings
    /// analysis of `MSFS_ToLiss_Plugin.wasm` (2026-06-10) proved the
    /// aircraft serves NON-standard SimVar variants:
    /// `GENERAL ENG COMBUSTION EX1:1..4` (not the plain SimVar — root
    /// cause of the dead `engines_running` behind v0.13.17),
    /// `TURB ENG CORRECTED FF:1..4` (not `ENG FUEL FLOW PPH` — root
    /// cause behind the v0.13.18 FOB derivation), and AP state ONLY
    /// via `L:AB_AP_*_LIGHT_ON` LVars. Titles start with
    /// "Aerosoft A346", `atc_model`/`icao_type_designator` = "A346".
    AerosoftA346,
    /// v0.13.13: FSReborn Phenom 300E (MSFS 2024 release). Verwendet die
    /// FSR-eigenen Engine-Knob-LVars statt `GENERAL ENG COMBUSTION:N`.
    /// Hintergrund (Pilot-Befund Michael 2026-05-26): Standard-SimVar
    /// liefert in Cold&Dark vor Engine-Start `engines_running > 0` obwohl
    /// Engines aus → Auto-Start scheitert mit "Triebwerke sind an". FSR
    /// nutzt eigene LVars fuer Switch-State (siehe HubHop-Reference
    /// docs/dev/lvar-discovery-hubhop.md):
    ///   L:FSR_300E_ENGINE1_KNOB_POS  Number  0=STOP 1=RUN 2=START
    ///   L:FSR_300E_ENGINE2_KNOB_POS  Number  0=STOP 1=RUN 2=START
    /// Rest der Telemetrie (N1/N2/Fuel/Gear/Flaps) kommt sauber via
    /// Standard-SimVars — kein voller Profile-Override noetig.
    FsrPhenom300e,
    /// v0.16.10 (#Premium): TFDi Design MD-11 (Pax + Freighter — ein
    /// Profil fuer beide). Detection-only in dieser Phase, die
    /// Premium-Quellen (laut 7-Agent-Inventar) werden in einer
    /// spaeteren Phase gemappt. atc_model "MD11"/"MD11F",
    /// ICAO-Designator MD11; Titles tragen "TFDi" + "MD-11".
    TfdiMd11,
    /// v0.16.11: iFly 737 MAX 8 (MSFS). Title aus der aircraft.cfg:
    /// "iFly 737-MAX8 (178Seat)" (Liveries variieren den Suffix).
    /// `atc_model` ist ein nutzloser generischer ATCCOM-B737-Token —
    /// Detection laeuft NUR ueber den Title-Marker. LVar-Quellen:
    /// WASM-Strings-Dump (verifiziert) + HubHop-Output-Presets fuer
    /// die CMD-A/B-LEDs (`L:VC_*_VAL`-Annunciator-Familie).
    IflyMax8,
}

impl AircraftProfile {
    /// Best-effort identification from the MSFS `TITLE` and `ATC MODEL`
    /// strings. Falls back to `Default` when nothing matches — that's the
    /// safe path: we keep using the standard SimVar set we already wire
    /// up, so undetected aircraft still produce a working PIREP.
    pub fn detect(title: &str, icao: &str) -> Self {
        let t = title.to_lowercase();
        let i = icao.to_lowercase();
        // FlyByWire — distinguish from real Airbus by FBW's marker text.
        // v0.16.10 (#Premium): Familie erweitert — der FBW A380X
        // ("…A380X…" im Title, ICAO A388) und der Headwind A339
        // (A330-900neo, FBW-Fork) teilen die `A32NX_`-LVar-Familie und
        // landen auf demselben Profil. Keiner der Marker kommt in
        // Fenix-/iniBuilds-Titles vor — Reihenfolge bleibt unkritisch.
        if t.contains("flybywire")
            || t.contains("fbw a32nx")
            || t.contains("a32nx")
            || t.contains("a380x")
            || t.contains("headwind")
        {
            return Self::FbwA32nx;
        }
        // Fenix — title typically begins with "FenixA319" / "FenixA320" /
        // "FenixA321". All three variants share the same `FNX_32X`
        // SimObject and LVar namespace, so the mapping is identical;
        // we differentiate purely so the activity log and PIREP show
        // the correct sub-type. ICAO callout is the secondary signal
        // for repaints whose title doesn't carry the variant suffix
        // (some community liveries flatten everything to "FenixA320").
        if t.contains("fenix") {
            if t.contains("a319") || i.contains("a319") {
                return Self::FenixA319;
            }
            if t.contains("a321") || i.contains("a321") {
                return Self::FenixA321;
            }
            return Self::FenixA320;
        }
        // PMDG 737 — covers 736/737/738/739 NG and MAX variants.
        if t.contains("pmdg") && (t.contains("737") || i.contains("b73")) {
            return Self::Pmdg737;
        }
        // PMDG 777 — 772/773 ER/Freighter.
        if t.contains("pmdg") && (t.contains("777") || i.contains("b77")) {
            return Self::Pmdg777;
        }
        // INIBuilds A350.
        if t.contains("inibuilds") && t.contains("a350") {
            return Self::IniA350;
        }
        // INIBuilds A340-600 Pro — pro suffix is the discriminator vs the
        // standard A340 build.
        if t.contains("inibuilds") && (t.contains("a346") || t.contains("a340-600"))
            && t.contains("pro")
        {
            return Self::IniA346Pro;
        }
        // INIBuilds A340 base.
        // v0.16.10 (#Premium): Per-Livery-Titles der iniBuilds A340-300
        // heissen z.B. "A340-300 EIS1" OHNE "inibuilds" — daher
        // zusaetzlich ICAO A343 bzw. atc_model "A340-300" als Signal.
        // Kollisionsfrei: die Aerosoft A346 meldet atc_model
        // "A340-600"/ICAO A346, die iniBuilds A346 Pro ICAO A346 —
        // beide bleiben auf ihren eigenen Zweigen (Pro-Match steht
        // davor, Aerosoft matcht exakt A346).
        if (t.contains("inibuilds") && t.contains("a340"))
            || clean_atc_model(icao).as_deref() == Some("A343")
            || i.contains("a340-300")
        {
            return Self::IniA340;
        }
        // Aerosoft A340-600 (ToLiss port). aircraft.cfg titles all start
        // with "Aerosoft A346" (e.g. "Aerosoft A346-MahanAir"), the ICAO
        // designator is "A346". The ICAO check uses the CLEANED model
        // (some MSFS aircraft report localization tokens) and is exact —
        // deliberately AFTER the INIBuilds branches above, so an
        // iniBuilds A340-600 Pro (which may also report ICAO A346) keeps
        // its own profile via its title.
        if t.starts_with("aerosoft a346")
            || clean_atc_model(icao).as_deref() == Some("A346")
        {
            return Self::AerosoftA346;
        }
        // v0.16.10 (#Premium): FBW-ICAO-Fallback fuer Repaints/Liveries
        // ohne Marker im Title — NUR noch A339 (= Headwind A339, der
        // einzige nennenswerte MSFS-A339; dessen Liveries tragen nicht
        // immer den "Headwind"-Marker). Bewusst NACH Fenix/iniBuilds/
        // Aerosoft platziert (deren Title-Matches gewinnen) und mit
        // Stock-Guard gegen Asobo-/iniBuilds-Titles.
        //
        // v0.16.10 QS (M4): der fruehere A20N-Fallback ist ENTFERNT —
        // A20N ist ein viel zu generischer Designator (LatinVFR A320neo,
        // marker-lose Stock-Liveries, …). Diese Nicht-FBW-A20N bekamen
        // das FBW-Profil und damit tote A32NX_-LVars → permanente
        // Some(false)-Phantome auf AP-Sub-Modes + A/THR. FBW-Liveries
        // matchen weiterhin ueber die Title-Marker (flybywire/a32nx/
        // a380x/headwind). Akzeptierte Rest-Klasse: ein hypothetischer
        // marker-loser Dritt-A339 wuerde weiter als FBW erkannt — den
        // Fall faengt das Defense-in-Depth-OR im MSFS-Adapter ab
        // (Standard-SimVars gewinnen, A/THR-0 → None statt Some(false)).
        // (ICAO A388 braucht keinen eigenen Zweig — A380X-Titles tragen
        // immer "A380X" bzw. "FlyByWire" und matchen oben.)
        if !t.contains("asobo")
            && !t.contains("inibuilds")
            && clean_atc_model(icao).as_deref() == Some("A339")
        {
            return Self::FbwA32nx;
        }
        // v0.13.13: FSReborn Phenom 300E. Title aus dem Sim heisst typisch
        // "FSReborn Phenom 300E Tristan Interior" (oder mit anderen
        // Interior-Varianten). Wir matchen tolerant auf fsreborn + phenom +
        // 300 — fängt auch Edge-Cases ab wie "FSR Phenom300" ohne Space.
        if t.contains("fsreborn") && t.contains("phenom") && t.contains("300") {
            return Self::FsrPhenom300e;
        }
        // v0.16.10 (#Premium): TFDi Design MD-11 / MD-11F. Title traegt
        // "TFDi" + "MD-11" (z.B. "TFDi Design MD-11 …"); Repaint-
        // Fallback ueber den ATC-MODEL/ICAO-Designator — "MD11" (Pax)
        // und "MD11F" (Freighter) landen beide auf EINEM Profil
        // (identische Premium-Quellen laut 7-Agent-Inventar).
        if (t.contains("tfdi") && t.contains("md-11"))
            || matches!(clean_atc_model(icao).as_deref(), Some("MD11") | Some("MD11F"))
        {
            return Self::TfdiMd11;
        }
        // v0.16.11: iFly 737 MAX 8. Title aus der aircraft.cfg heisst
        // "iFly 737-MAX8 (178Seat)" — Liveries behalten den
        // "iFly … MAX"-Stamm. atc_model ist ein generischer
        // ATCCOM-B737-Token (nutzlos als Signal). BEWUSST KEIN
        // bare-B38M-ICAO-Fallback: Bredok3d verkauft ebenfalls einen
        // B38M ("Bredok3d 737 MAX") — ein ICAO-Fallback waere dieselbe
        // Misdetection-Klasse wie der in v0.16.10 (QS M4) entfernte
        // A20N-Fallback (totes LVar-Profil → Some(false)-Phantome).
        if t.contains("ifly") && t.contains("max") {
            return Self::IflyMax8;
        }
        Self::Default
    }

    /// `true` if this profile is any Fenix A32x variant. All three
    /// share the same `FNX_32X` SimObject + LVar namespace, so most
    /// adapter mapping branches treat them identically.
    pub fn is_fenix(self) -> bool {
        matches!(self, Self::FenixA319 | Self::FenixA320 | Self::FenixA321)
    }

    /// v0.7.17 (B-001): ICAO type designator fallback fuer Profile,
    /// die einen kanonischen ICAO-Code haben. Wird im Adapter genutzt
    /// wenn `aircraft_icao` aus dem Sim leer kommt (typisch bei Fenix,
    /// das den Standard-`ATC MODEL`-SimVar nicht zuverlaessig fuellt
    /// — Pilot saht im Activity-Log „Type ?" trotz erkanntem Profil).
    /// Gibt nur dann ein Some zurueck wenn das Profil eine eindeutige
    /// Variante hat (Fenix A319/A320/A321 ja, FbwA32nx je nach
    /// Repaint mehrdeutig also weiter None).
    pub fn icao_fallback(self) -> Option<&'static str> {
        match self {
            Self::FenixA319 => Some("A319"),
            Self::FenixA320 => Some("A320"),
            Self::FenixA321 => Some("A321"),
            // v0.13.13: FSR Phenom 300E hat ICAO E55P (Embraer Phenom 300
            // Standard-Designator).
            Self::FsrPhenom300e => Some("E55P"),
            // Aerosoft A340-600: kanonischer Designator A346 (deckt sich
            // mit `icao_type_designator` in der aircraft.cfg).
            Self::AerosoftA346 => Some("A346"),
            _ => None,
        }
    }

    /// Short human-readable label for the activity log.
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default (standard SimVars)",
            Self::FbwA32nx => "FlyByWire A32NX",
            Self::FenixA319 => "Fenix A319",
            Self::FenixA320 => "Fenix A320",
            Self::FenixA321 => "Fenix A321",
            Self::Pmdg737 => "PMDG 737",
            Self::Pmdg777 => "PMDG 777",
            Self::IniA340 => "INIBuilds A340",
            Self::IniA350 => "INIBuilds A350",
            Self::IniA346Pro => "INIBuilds A340-600 Pro",
            Self::AerosoftA346 => "Aerosoft A340-600",
            Self::FsrPhenom300e => "FSReborn Phenom 300E",
            Self::TfdiMd11 => "TFDi MD-11",
            Self::IflyMax8 => "iFly 737 MAX 8",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum Simulator {
    Msfs2020,
    Msfs2024,
    XPlane11,
    XPlane12,
    /// Catch-all when no adapter has reported a simulator yet (used as
    /// the `Default` so `SimSnapshot::default()` works for tests).
    #[default]
    Other,
}

/// User-selected simulator kind, persisted across app restarts.
/// Drives which adapter the app boots after login.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SimKind {
    Off,
    Msfs2020,
    Msfs2024,
    XPlane11,
    XPlane12,
}

impl Default for SimKind {
    fn default() -> Self {
        SimKind::Msfs2024
    }
}

impl SimKind {
    pub fn is_msfs(self) -> bool {
        matches!(self, SimKind::Msfs2020 | SimKind::Msfs2024)
    }

    pub fn is_xplane(self) -> bool {
        matches!(self, SimKind::XPlane11 | SimKind::XPlane12)
    }

    /// Map to the `Simulator` enum used inside `SimSnapshot`.
    pub fn as_simulator(self) -> Simulator {
        match self {
            SimKind::Off => Simulator::Other,
            SimKind::Msfs2020 => Simulator::Msfs2020,
            SimKind::Msfs2024 => Simulator::Msfs2024,
            SimKind::XPlane11 => Simulator::XPlane11,
            SimKind::XPlane12 => Simulator::XPlane12,
        }
    }
}

/// Flight phases as required by spec §9.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum FlightPhase {
    #[default]
    Preflight,
    Boarding,
    Pushback,
    TaxiOut,
    TakeoffRoll,
    Takeoff,
    Climb,
    Cruise,
    /// v0.5.11: dedicated Holding phase. Detected when the aircraft
    /// circles at constant altitude (sustained bank > 15° + |VS| <
    /// 200 fpm for > 90 s). Triggered from Cruise (high-altitude
    /// hold over a fix) or Approach (low-altitude approach hold).
    /// On exit, returns to whichever phase we came from — or to
    /// Approach if a sustained descent has begun (= ATC clears us
    /// out of the hold for the approach).
    Holding,
    Descent,
    Approach,
    Final,
    Landing,
    TaxiIn,
    BlocksOn,
    Arrived,
    PirepSubmitted,
}

/// What every simulator adapter must provide.
///
/// Phase 1: implemented by `sim-msfs` (SimConnect). Phase 2: `sim-xplane` (UDP from XPLM plugin).
pub trait SimAdapter: Send + 'static {
    /// Display name of the adapter, e.g. "MSFS 2024 (SimConnect)".
    fn name(&self) -> &str;

    /// Try to connect to the simulator. Returns `Ok(())` once a snapshot stream is established.
    fn connect(&mut self) -> Result<(), SimError>;

    /// Disconnect cleanly.
    fn disconnect(&mut self);

    /// Whether the adapter is currently connected and producing snapshots.
    fn is_connected(&self) -> bool;

    /// Pull the most recent snapshot, if any. Non-blocking.
    fn latest_snapshot(&self) -> Option<SimSnapshot>;
}

#[derive(Debug, thiserror::Error)]
pub enum SimError {
    #[error("simulator not running")]
    NotRunning,
    #[error("connection refused: {0}")]
    Refused(String),
    #[error("transport error: {0}")]
    Transport(String),
    #[error("not implemented yet")]
    NotImplemented,
}

// TODO(phase-1):
//   * Implement the SimAdapter trait in `sim-msfs`.
//   * Add a Phase FSM driven by SimSnapshot streams.
//   * Add a snapshot ring buffer for downstream consumers.

/// MSFS often returns SimVar values as localization keys, not plain text.
/// The ATC MODEL var is one of them — e.g. `TT:ATCCOM.AC_MODEL_A320.0.text`
/// or `ATCCOM.AC_MODEL C208.0.text`. Pull out the readable code, or return
/// `None` if the input is an unresolved key we can't decode.
///
/// v0.12.10: Hierher (sim-core) gezogen, damit der MSFS-Telemetrie-
/// Adapter den `ATC MODEL` schon bei der Erfassung bereinigt — vorher
/// landete der rohe Token (z.B. `ATCCOM.AC_MODEL C208.0.text` der
/// BlackSquare Caravan) ungereinigt in `aircraft_icao`, was „Type ?"
/// und einen kaputten PIREP zur Folge hatte.
pub fn clean_atc_model(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(start) = s.find("AC_MODEL") {
        let after = &s[start + "AC_MODEL".len()..];
        let after = after.trim_start_matches(|c: char| c == '_' || c == ' ');
        if let Some(end) = after.find('.') {
            let model = &after[..end];
            if !model.is_empty() {
                return Some(model.to_uppercase());
            }
        }
    }
    let upper = s.to_uppercase();
    if upper.starts_with("TT:") || upper.contains("ATCCOM.") || upper.ends_with(".TEXT") {
        return None;
    }
    // v0.8.1: Vendor-Tag-Prefix-Strip. Einige MSFS-Addons (Flysimware
    // Citation X, manche Carenado-Pakete) schicken den ICAO mit einem
    // "$$:"-Prefix als ATC-MODEL — Sim liefert z.B. "$$:C750" statt
    // "C750". Aircraft-Mismatch-Check verglich dann "C750" (Bid) gegen
    // "$$:C750" (Sim) und schlug fehl. Live-Bug GSG/Sven M 2026-05-13.
    // Wir strippen den Prefix wenn er aus 1-4 Zeichen + ":" besteht
    // und kein Buchstabe enthält (= Sonderzeichen wie "$$:" / "##:"),
    // damit echte ICAO-Codes wie "TT:..." (= text-token, oben schon
    // gefiltert) nicht falsch behandelt werden.
    if let Some(colon_pos) = upper.find(':') {
        if colon_pos <= 4 {
            let prefix = &upper[..colon_pos];
            let is_vendor_tag = !prefix.is_empty()
                && !prefix.chars().any(|c| c.is_ascii_alphanumeric());
            if is_vendor_tag {
                let stripped = upper[colon_pos + 1..].trim().to_string();
                if !stripped.is_empty() {
                    return Some(stripped);
                }
            }
        }
    }
    Some(upper)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_fenix_a319_from_title() {
        let p = AircraftProfile::detect("FenixA319 IAE", "A319");
        assert_eq!(p, AircraftProfile::FenixA319);
        assert!(p.is_fenix());
        assert_eq!(p.label(), "Fenix A319");
    }

    #[test]
    fn detect_fenix_a320_default_when_no_variant() {
        // Plain "Fenix" matches A320 as the canonical fallback. This
        // is the v0.7.15 stable behavior — preserve it so community
        // liveries that flatten the title still get a Fenix profile.
        let p = AircraftProfile::detect("Fenix Simulations Airbus", "A320");
        assert_eq!(p, AircraftProfile::FenixA320);
    }

    #[test]
    fn detect_fenix_a320_explicit() {
        let p = AircraftProfile::detect("FenixA320 CFM SL", "A320");
        assert_eq!(p, AircraftProfile::FenixA320);
        assert!(p.is_fenix());
    }

    #[test]
    fn detect_fenix_a321_from_title() {
        let p = AircraftProfile::detect("FenixA321 NEO LR", "A321");
        assert_eq!(p, AircraftProfile::FenixA321);
        assert!(p.is_fenix());
        assert_eq!(p.label(), "Fenix A321");
    }

    #[test]
    fn detect_fenix_a319_via_icao_only() {
        // Repaint scenario: title omits the variant marker, ICAO still
        // identifies it. Match on either signal is by design.
        let p = AircraftProfile::detect("Fenix Repaint", "A319");
        assert_eq!(p, AircraftProfile::FenixA319);
    }

    #[test]
    fn detect_non_fenix_stays_default() {
        let p = AircraftProfile::detect("Asobo A320 Neo", "A20N");
        assert_eq!(p, AircraftProfile::Default);
        assert!(!p.is_fenix());
    }

    #[test]
    fn is_fenix_covers_all_three_variants() {
        assert!(AircraftProfile::FenixA319.is_fenix());
        assert!(AircraftProfile::FenixA320.is_fenix());
        assert!(AircraftProfile::FenixA321.is_fenix());
        assert!(!AircraftProfile::FbwA32nx.is_fenix());
        assert!(!AircraftProfile::Default.is_fenix());
    }

    #[test]
    fn icao_fallback_for_fenix_variants() {
        assert_eq!(AircraftProfile::FenixA319.icao_fallback(), Some("A319"));
        assert_eq!(AircraftProfile::FenixA320.icao_fallback(), Some("A320"));
        assert_eq!(AircraftProfile::FenixA321.icao_fallback(), Some("A321"));
        assert_eq!(AircraftProfile::Default.icao_fallback(), None);
        assert_eq!(AircraftProfile::FbwA32nx.icao_fallback(), None);
        assert_eq!(AircraftProfile::Pmdg737.icao_fallback(), None);
    }

    // ---- v0.13.13 FsrPhenom300e Profile ----

    #[test]
    fn detect_fsr_phenom_300e_from_full_title() {
        // Realer Title aus Michael's Telemetrie (NJE 245 LEMH->LEBL 26.05.2026):
        let p = AircraftProfile::detect("FSReborn Phenom 300E Tristan Interior", "E55P");
        assert_eq!(p, AircraftProfile::FsrPhenom300e);
    }

    #[test]
    fn detect_fsr_phenom_300e_case_insensitive() {
        let p = AircraftProfile::detect("fsreborn phenom 300e", "E55P");
        assert_eq!(p, AircraftProfile::FsrPhenom300e);
    }

    #[test]
    fn detect_fsr_phenom_300e_with_variant_suffix() {
        // FSR liefert verschiedene Interior-Optionen, z.B. "Tristan", "Default".
        let p = AircraftProfile::detect("FSReborn Phenom 300E Default Interior", "E55P");
        assert_eq!(p, AircraftProfile::FsrPhenom300e);
    }

    #[test]
    fn detect_phenom_without_fsreborn_marker_stays_default() {
        // Asobo Default-Phenom oder andere Studios sollen NICHT auf das
        // FSR-Profile fallen — die haben das Knob-LVar nicht.
        let p = AircraftProfile::detect("Asobo Phenom 300", "E55P");
        assert_eq!(p, AircraftProfile::Default);
    }

    #[test]
    fn detect_fsreborn_lear_75_does_not_match_phenom() {
        // FSReborn hat auch andere Aircraft (FSR500, Sting S4, evtl. spaeter
        // Lear). Diese sollen nicht versehentlich als Phenom 300E erkannt
        // werden — Detection prueft auf alle drei Marker.
        let p = AircraftProfile::detect("FSReborn Lear 75", "LJ75");
        assert_eq!(p, AircraftProfile::Default);
    }

    #[test]
    fn icao_fallback_fsr_phenom_300e_is_e55p() {
        assert_eq!(AircraftProfile::FsrPhenom300e.icao_fallback(), Some("E55P"));
    }

    #[test]
    fn label_fsr_phenom_300e_is_human_readable() {
        assert_eq!(AircraftProfile::FsrPhenom300e.label(), "FSReborn Phenom 300E");
    }

    #[test]
    fn fsr_phenom_300e_is_not_fenix() {
        // Sanity: das Profile darf NICHT als Fenix klassifiziert werden
        // (sonst wuerde der Fenix-LVar-Mapping-Block falsch greifen).
        assert!(!AircraftProfile::FsrPhenom300e.is_fenix());
    }

    // ---- AerosoftA346 Profile (WASM-Analyse 2026-06-10) ----

    #[test]
    fn detect_aerosoft_a346_from_title_prefix() {
        // Reale Titles aus der aircraft.cfg beginnen alle mit
        // "Aerosoft A346" (Livery-Suffix variiert).
        let p = AircraftProfile::detect("Aerosoft A346-MahanAir", "A346");
        assert_eq!(p, AircraftProfile::AerosoftA346);
        let p = AircraftProfile::detect("Aerosoft A346 Pro", "A346");
        assert_eq!(p, AircraftProfile::AerosoftA346);
        assert_eq!(p.label(), "Aerosoft A340-600");
    }

    #[test]
    fn detect_aerosoft_a346_case_insensitive_title() {
        let p = AircraftProfile::detect("AEROSOFT A346 Lufthansa", "");
        assert_eq!(p, AircraftProfile::AerosoftA346);
    }

    #[test]
    fn detect_aerosoft_a346_via_icao_only() {
        // Repaint-Szenario: Title traegt den Marker nicht, der ICAO-
        // Designator A346 identifiziert das Aircraft trotzdem.
        let p = AircraftProfile::detect("Custom A340-600 Repaint", "A346");
        assert_eq!(p, AircraftProfile::AerosoftA346);
        // Auch wenn der Sim den ICAO als Localization-Token liefert.
        let p = AircraftProfile::detect(
            "Custom A340-600 Repaint",
            "ATCCOM.AC_MODEL A346.0.text",
        );
        assert_eq!(p, AircraftProfile::AerosoftA346);
    }

    #[test]
    fn detect_aerosoft_a346_no_false_positives() {
        // Andere Aircraft duerfen NICHT auf das A346-Profil fallen.
        assert_eq!(
            AircraftProfile::detect("FenixA320 CFM SL", "A320"),
            AircraftProfile::FenixA320,
        );
        assert_eq!(
            AircraftProfile::detect("Asobo A320 Neo", "A20N"),
            AircraftProfile::Default,
        );
        // "Aerosoft" allein (anderes Aerosoft-Aircraft) reicht nicht.
        assert_eq!(
            AircraftProfile::detect("Aerosoft CRJ 550", "CRJ5"),
            AircraftProfile::Default,
        );
        // ICAO muss EXAKT A346 sein — A343 darf NICHT auf Aerosoft
        // fallen. v0.16.10 (#Premium): A343 mappt jetzt auf IniA340
        // (Per-Livery-Titles der iniBuilds A340-300 tragen kein
        // "inibuilds") — der Guard hier bleibt: nicht AerosoftA346.
        assert_eq!(
            AircraftProfile::detect("Some A340-300", "A343"),
            AircraftProfile::IniA340,
        );
    }

    #[test]
    fn detect_inibuilds_a346_pro_still_wins_over_aerosoft_icao_match() {
        // Regression-Guard: die iniBuilds A340-600 Pro meldet u.U.
        // ebenfalls ICAO A346 — ihr Title-Match muss weiter VOR dem
        // Aerosoft-ICAO-Fallback greifen.
        let p = AircraftProfile::detect("iniBuilds A340-600 Pro", "A346");
        assert_eq!(p, AircraftProfile::IniA346Pro);
    }

    #[test]
    fn aerosoft_a346_icao_fallback_and_not_fenix() {
        assert_eq!(AircraftProfile::AerosoftA346.icao_fallback(), Some("A346"));
        // Sanity: darf nicht als Fenix klassifiziert werden, sonst
        // greift der Fenix-LVar-Mapping-Block faelschlich.
        assert!(!AircraftProfile::AerosoftA346.is_fenix());
    }

    #[test]
    fn clean_atc_model_basic_and_token_forms() {
        // Plain ICAO codes pass through (uppercased).
        assert_eq!(clean_atc_model("C208"), Some("C208".to_string()));
        assert_eq!(clean_atc_model("a320"), Some("A320".to_string()));
        // Empty / unresolved text tokens → None (caller uses fallback).
        assert_eq!(clean_atc_model(""), None);
        assert_eq!(clean_atc_model("TT:CESSNA"), None);
        // Vendor-tag prefix gets stripped.
        assert_eq!(clean_atc_model("$$:C750"), Some("C750".to_string()));
    }

    #[test]
    fn touchdown_vs_source_prefers_raw_then_falls_back() {
        let mut s = SimSnapshot::default();
        s.vertical_speed_fpm = -50.0; // display value (e.g. the X-Plane VVI)
        // No raw signal (MSFS) → touchdown falls back to the display V/S.
        assert_eq!(s.vertical_speed_raw_fpm, None);
        assert_eq!(s.touchdown_vs_source_fpm(), -50.0);
        // Raw present (X-Plane local_vy) → touchdown uses the raw, responsive
        // value, independent of the curvature-free (and possibly damped)
        // display V/S — this is what keeps the landing rate lag-free.
        s.vertical_speed_raw_fpm = Some(-320.0);
        assert_eq!(s.touchdown_vs_source_fpm(), -320.0);
        // The display V/S is untouched (stays the curvature-free FSM value).
        assert_eq!(s.vertical_speed_fpm, -50.0);
    }

    #[test]
    fn clean_atc_model_blacksquare_caravan_regression() {
        // v0.12.10 live-bug: the BlackSquare Caravan reports its ATC MODEL
        // as the raw token `ATCCOM.AC_MODEL C208.0.text` (note the SPACE
        // and lowercase `.text`). Must resolve to "C208" — otherwise
        // `aircraft_icao` carries garbage → "Type ?" → broken PIREP filing.
        assert_eq!(
            clean_atc_model("ATCCOM.AC_MODEL C208.0.text"),
            Some("C208".to_string()),
        );
        // Underscore variant must also resolve.
        assert_eq!(
            clean_atc_model("ATCCOM.AC_MODEL_C750.0.TEXT"),
            Some("C750".to_string()),
        );
    }

    #[test]
    fn autothrottle_on_defaults_none_and_old_jsonl_replays_deserialize() {
        // 2026-06-10 (A346 full profile): the new `autothrottle_on`
        // field must default to None — only profiles with a verified
        // A/THR state source (A346 ATHR light, Fenix S_FCU_ATHR) set
        // it, everything else stays silent in the activity log.
        let s = SimSnapshot::default();
        assert_eq!(s.autothrottle_on, None);

        // Backward compat: a snapshot serialized BEFORE the field
        // existed (= recorded JSONL flight logs) must still
        // deserialize — `#[serde(default)]` covers the missing key.
        let mut v = serde_json::to_value(&s).expect("serialize");
        v.as_object_mut()
            .expect("object")
            .remove("autothrottle_on")
            .expect("field present after serialize");
        let restored: SimSnapshot =
            serde_json::from_value(v).expect("old JSONL without the key must deserialize");
        assert_eq!(restored.autothrottle_on, None);
    }

    // ---- v0.16.7 PMDG autoflight override (AP master + A/THR) ----
    //
    // Data-audit 2026-06-11: standard `AUTOPILOT MASTER` reads dead
    // (never true) on PMDG 737/777 — the override maps the SDK
    // annunciators onto the generic fields. Presence-gated: a snapshot
    // without `pmdg` must come out bit-identical.

    /// PmdgState carrier with just the two autoflight bits set.
    fn pmdg_autoflight(ap_engaged: bool, at_armed: bool) -> PmdgState {
        PmdgState {
            ap_engaged,
            at_armed,
            ..PmdgState::default()
        }
    }

    #[test]
    fn pmdg_ap_engaged_overrides_dead_standard_simvar() {
        // The audit case: standard SimVar stuck at false, CMD A (or B,
        // or 777 AP L/R — all collapse into `ap_engaged`) is lit.
        let mut s = SimSnapshot::default();
        s.autopilot_master = Some(false); // dead standard var
        s.pmdg = Some(pmdg_autoflight(true, false));
        s.apply_pmdg_autoflight_override();
        assert_eq!(s.autopilot_master, Some(true));
    }

    #[test]
    fn pmdg_both_cmd_channels_off_reports_master_off() {
        let mut s = SimSnapshot::default();
        s.autopilot_master = Some(false);
        s.pmdg = Some(pmdg_autoflight(false, false));
        s.apply_pmdg_autoflight_override();
        assert_eq!(s.autopilot_master, Some(false));
        assert_eq!(s.autothrottle_on, Some(false));
    }

    #[test]
    fn pmdg_standard_simvar_wins_as_or_tiebreaker() {
        // Should PMDG ever wire the standard var, it can only agree or
        // win — same OR semantics as the A346/Fenix LVar mappings.
        let mut s = SimSnapshot::default();
        s.autopilot_master = Some(true);
        s.autothrottle_on = Some(true);
        s.pmdg = Some(pmdg_autoflight(false, false));
        s.apply_pmdg_autoflight_override();
        assert_eq!(s.autopilot_master, Some(true));
        assert_eq!(s.autothrottle_on, Some(true));
    }

    #[test]
    fn pmdg_at_armed_maps_to_autothrottle_on() {
        // PMDG (Default MSFS profile) leaves `autothrottle_on` at None
        // in the telemetry parser — the override fills it from the SDK.
        let mut s = SimSnapshot::default();
        assert_eq!(s.autothrottle_on, None);
        s.pmdg = Some(pmdg_autoflight(false, true));
        s.apply_pmdg_autoflight_override();
        assert_eq!(s.autothrottle_on, Some(true));
    }

    #[test]
    fn absent_pmdg_leaves_snapshot_untouched() {
        // Presence gate: no PMDG state → no field changes at all, for
        // every starting value the standard telemetry can produce.
        for master in [None, Some(false), Some(true)] {
            for athr in [None, Some(false), Some(true)] {
                let mut s = SimSnapshot::default();
                s.autopilot_master = master;
                s.autothrottle_on = athr;
                assert!(s.pmdg.is_none());
                s.apply_pmdg_autoflight_override();
                assert_eq!(s.autopilot_master, master);
                assert_eq!(s.autothrottle_on, athr);
            }
        }
    }

    // ---- v0.16.10 (#Premium): Cockpit-Tiefendaten ----
    //
    // Foundation-Phase: neue SimSnapshot-/PmdgState-Carrier-Felder,
    // neue Profile (TfdiMd11, erweiterte FBW-Familie, IniA340-Livery-
    // Fallback) und der PMDG-Premium-Override. Die Addon-Mapper folgen
    // in spaeteren Phasen — hier sichern wir Serde-Kompatibilitaet,
    // Detection-Regeln und die Override-Semantik ab.

    /// Alle in v0.16.10 neu eingefuehrten SimSnapshot-Keys — fuer den
    /// JSONL-Backward-Compat-Test unten.
    const PREMIUM_SNAPSHOT_KEYS: [&str; 26] = [
        "fma_lateral_mode",
        "fma_vertical_mode",
        "fma_thrust_mode",
        "flight_phase_aircraft",
        "v1_kt",
        "vr_kt",
        "v2_kt",
        "vapp_kt",
        "vls_kt",
        "vref_kt",
        "flex_temp_c",
        "thrust_gate",
        "master_caution",
        "master_warning",
        "managed_speed",
        "managed_heading",
        "managed_altitude",
        "reverser_deployed",
        "ground_spoilers_active",
        "eng_n1_pct",
        "baro_std",
        "fuel_per_tank_kg",
        "below_gs_alert",
        "cabin_altitude_warning",
        "stab_out_of_trim",
        "minimums_baro_ft",
    ];

    #[test]
    fn premium_fields_default_none_and_old_jsonl_replays_deserialize() {
        // Default-Snapshot traegt ueberall None.
        let s = SimSnapshot::default();
        assert_eq!(s.fma_lateral_mode, None);
        assert_eq!(s.v1_kt, None);
        assert_eq!(s.eng_n1_pct, None);
        assert_eq!(s.fuel_per_tank_kg, None);
        assert_eq!(s.ground_spoilers_active, None);
        assert_eq!(s.minimums_baro_ft, None);

        // Backward compat: ein Snapshot, der VOR v0.16.10 serialisiert
        // wurde (= alle aufgezeichneten JSONL-Flight-Logs), traegt
        // KEINEN der neuen Keys — `#[serde(default)]` muss jeden
        // einzelnen abdecken.
        let mut v = serde_json::to_value(&s).expect("serialize");
        let obj = v.as_object_mut().expect("object");
        for key in PREMIUM_SNAPSHOT_KEYS {
            obj.remove(key).expect("premium key present after serialize");
        }
        let restored: SimSnapshot =
            serde_json::from_value(v).expect("old JSONL without premium keys must deserialize");
        // Uniform pruefen: alle 26 Felder sind nach dem Restore null.
        let v2 = serde_json::to_value(&restored).expect("re-serialize");
        for key in PREMIUM_SNAPSHOT_KEYS {
            assert!(
                v2.get(key).expect("key exists").is_null(),
                "premium field `{key}` must default to None",
            );
        }
    }

    #[test]
    fn pmdg_state_old_jsonl_without_premium_fields_deserializes() {
        // Gleiches Spiel fuer den PmdgState-Carrier: aeltere JSONL-
        // Replays mit `pmdg: Some(...)` tragen die neuen SDK-Felder
        // nicht — `#[serde(default)]` muss sie auf None setzen.
        let p = PmdgState::default();
        let mut v = serde_json::to_value(&p).expect("serialize");
        let obj = v.as_object_mut().expect("object");
        for key in [
            "reverser_deployed",
            "master_caution",
            "master_warning",
            "below_gs",
            "cabin_altitude_warning",
            "stab_out_of_trim",
            "fuel_per_tank_kg",
            "minimums_baro_ft",
            "gnd_prox_warning",
        ] {
            obj.remove(key).expect("premium key present after serialize");
        }
        let restored: PmdgState =
            serde_json::from_value(v).expect("old PmdgState JSON must deserialize");
        assert_eq!(restored.reverser_deployed, None);
        assert_eq!(restored.fuel_per_tank_kg, None);
        assert_eq!(restored.gnd_prox_warning, None);
    }

    // ---- v0.16.12 (#phase-v2): Schatten-Felder-Carrier-Vertrag ----

    #[test]
    fn shadow_fields_default_none_and_skip_when_unset() {
        // Adapter/Default liefern None — und None-Felder verschwinden
        // per skip_serializing_if komplett aus dem JSON (Nicht-Flug-
        // Snapshots + Frontend-Streams bleiben sauber).
        let s = SimSnapshot::default();
        assert_eq!(s.shadow_phase, None);
        assert_eq!(s.shadow_segment, None);
        let v = serde_json::to_value(&s).expect("serialize");
        assert!(v.get("shadow_phase").is_none(), "None darf nicht serialisieren");
        assert!(v.get("shadow_segment").is_none());

        // Alte JSONL-Replays (ohne die Keys) deserialisieren weiter.
        let restored: SimSnapshot =
            serde_json::from_value(v).expect("old JSONL without shadow keys must deserialize");
        assert_eq!(restored.shadow_phase, None);

        // Vom Streamer gestempelte Werte runden sauber durch die JSONL.
        let mut stamped = SimSnapshot::default();
        stamped.shadow_phase = Some("climb".into());
        stamped.shadow_segment = Some("level".into());
        let v = serde_json::to_value(&stamped).expect("serialize stamped");
        assert_eq!(v["shadow_phase"], "climb");
        assert_eq!(v["shadow_segment"], "level");
        let rt: SimSnapshot = serde_json::from_value(v).expect("round-trip");
        assert_eq!(rt.shadow_phase.as_deref(), Some("climb"));
        assert_eq!(rt.shadow_segment.as_deref(), Some("level"));
    }

    // ---- v0.16.10 (#Premium): Profile-Detection ----

    #[test]
    fn detect_inibuilds_a340_per_livery_title_via_icao() {
        // Per-Livery-Titles der iniBuilds A340-300 tragen KEIN
        // "inibuilds" — der ICAO-/atc_model-Fallback muss greifen.
        let p = AircraftProfile::detect("A340-300 EIS1", "A343");
        assert_eq!(p, AircraftProfile::IniA340);
        assert_eq!(p.label(), "INIBuilds A340");
        // atc_model-Variante "A340-300" (Substring-Match auf dem Raw-String).
        let p = AircraftProfile::detect("A340-300 EIS2 CFM", "A340-300");
        assert_eq!(p, AircraftProfile::IniA340);
        // Localization-Token-Form des ICAO.
        let p = AircraftProfile::detect("A340-300 EIS1", "ATCCOM.AC_MODEL A343.0.text");
        assert_eq!(p, AircraftProfile::IniA340);
    }

    #[test]
    fn detect_inibuilds_a340_title_marker_still_works() {
        // Regression: der bestehende Title-Pfad bleibt erhalten.
        let p = AircraftProfile::detect("iniBuilds A340-300", "");
        assert_eq!(p, AircraftProfile::IniA340);
        // …und die A346 Pro faellt weiter NICHT auf das Basis-Profil.
        let p = AircraftProfile::detect("iniBuilds A340-600 Pro", "A346");
        assert_eq!(p, AircraftProfile::IniA346Pro);
    }

    #[test]
    fn detect_tfdi_md11_from_title_and_atc_model() {
        let p = AircraftProfile::detect("TFDi Design MD-11", "MD11");
        assert_eq!(p, AircraftProfile::TfdiMd11);
        assert_eq!(p.label(), "TFDi MD-11");
        // Freighter — gleiches Profil.
        let p = AircraftProfile::detect("TFDi Design MD-11F", "");
        assert_eq!(p, AircraftProfile::TfdiMd11);
        // Repaint ohne TFDi-Marker: atc_model/ICAO-Fallback.
        let p = AircraftProfile::detect("Custom MD-11 Repaint", "MD11F");
        assert_eq!(p, AircraftProfile::TfdiMd11);
        let p = AircraftProfile::detect("Some Livery", "MD11");
        assert_eq!(p, AircraftProfile::TfdiMd11);
    }

    #[test]
    fn detect_tfdi_md11_no_false_positives() {
        // MD-80-Familie & Co. duerfen NICHT auf das MD-11-Profil fallen.
        assert_eq!(
            AircraftProfile::detect("Rotate MD-80", "MD80"),
            AircraftProfile::Default,
        );
        // "TFDi" allein (anderes TFDi-Produkt) reicht nicht.
        assert_eq!(
            AircraftProfile::detect("TFDi Design 717", "B712"),
            AircraftProfile::Default,
        );
    }

    // ---- v0.16.11: iFly 737 MAX 8 ----

    #[test]
    fn detect_ifly_max8_from_title() {
        // aircraft.cfg-Title (atc_model ist ein nutzloser generischer
        // ATCCOM-B737-Token — darf keine Rolle spielen).
        let p = AircraftProfile::detect(
            "iFly 737-MAX8 (178Seat)",
            "ATCCOM.AC_MODEL B737.0.text",
        );
        assert_eq!(p, AircraftProfile::IflyMax8);
        assert_eq!(p.label(), "iFly 737 MAX 8");
        // Livery-Title — behaelt den "iFly … MAX"-Stamm.
        let p = AircraftProfile::detect("iFly 737-MAX8 TUI", "");
        assert_eq!(p, AircraftProfile::IflyMax8);
    }

    #[test]
    fn detect_ifly_max8_no_false_positives() {
        // Bredok3d verkauft ebenfalls einen B38M — DESHALB gibt es
        // keinen bare-B38M-ICAO-Fallback (gleiche Misdetection-Klasse
        // wie der entfernte A20N-Fallback): tote VC_*-LVars wuerden
        // dort Some(false)-Phantome erzeugen.
        assert_eq!(
            AircraftProfile::detect("Bredok3d 737 MAX", "B38M"),
            AircraftProfile::Default,
        );
        // PMDG 737-800 bleibt auf dem PMDG-Profil (Branch-Reihenfolge).
        assert_eq!(
            AircraftProfile::detect("PMDG 737-800 Lufthansa", "B738"),
            AircraftProfile::Pmdg737,
        );
    }

    #[test]
    fn detect_fbw_family_titles() {
        // Klassischer A32NX — Title-Marker.
        let p = AircraftProfile::detect("Airbus A320 Neo FlyByWire", "A20N");
        assert_eq!(p, AircraftProfile::FbwA32nx);
        let p = AircraftProfile::detect("FlyByWire A32NX", "A20N");
        assert_eq!(p, AircraftProfile::FbwA32nx);
        // FBW A380X — gleiche LVar-Familie, gleiches Profil.
        let p = AircraftProfile::detect("FlyByWire Simulations A380X", "A388");
        assert_eq!(p, AircraftProfile::FbwA32nx);
        let p = AircraftProfile::detect("Airbus A380X Lufthansa", "A388");
        assert_eq!(p, AircraftProfile::FbwA32nx);
        // Headwind A339 (A330-900neo, FBW-Fork).
        let p = AircraftProfile::detect("Headwind A330-900neo", "A339");
        assert_eq!(p, AircraftProfile::FbwA32nx);
    }

    #[test]
    fn detect_fbw_family_icao_fallback() {
        // v0.16.10 QS (M4): nur noch A339 (Headwind ist der einzige
        // MSFS-A339) faellt per ICAO auf das FBW-Profil zurueck.
        let p = AircraftProfile::detect("Custom A330-900 Livery", "A339");
        assert_eq!(p, AircraftProfile::FbwA32nx);
    }

    #[test]
    fn detect_a20n_without_marker_stays_default() {
        // v0.16.10 QS (M4): A20N-ICAO-Fallback entfernt — A20N ist zu
        // generisch. Nicht-FBW-A20N (LatinVFR, marker-lose Stock-
        // Liveries) bekamen sonst das FBW-Profil und damit tote
        // A32NX_-LVars (Some(false)-Phantome auf AP-Sub-Modes/A/THR).
        assert_eq!(
            AircraftProfile::detect("Airbus A320neo Lufthansa", "A20N"),
            AircraftProfile::Default,
        );
        assert_eq!(
            AircraftProfile::detect("LatinVFR A320neo", "A20N"),
            AircraftProfile::Default,
        );
        // Stock-Guard-Faelle bleiben ebenfalls Default.
        assert_eq!(
            AircraftProfile::detect("Asobo A320 Neo", "A20N"),
            AircraftProfile::Default,
        );
        assert_eq!(
            AircraftProfile::detect("iniBuilds A320neo V2", "A20N"),
            AircraftProfile::Default,
        );
        // Title-Marker matchen weiterhin (kein Regress fuer echte FBW).
        assert_eq!(
            AircraftProfile::detect("FlyByWire A32NX", "A20N"),
            AircraftProfile::FbwA32nx,
        );
        assert_eq!(
            AircraftProfile::detect("Airbus A320 Neo FlyByWire", "A20N"),
            AircraftProfile::FbwA32nx,
        );
    }

    #[test]
    fn detect_premium_regressions_unchanged() {
        // v0.16.10-Guard: bestehende Profile duerfen sich durch die neuen
        // Zweige nicht verschieben.
        assert_eq!(
            AircraftProfile::detect("FenixA320 CFM SL", "A320"),
            AircraftProfile::FenixA320,
        );
        assert_eq!(
            AircraftProfile::detect("iniBuilds A350-900", "A359"),
            AircraftProfile::IniA350,
        );
        assert_eq!(
            AircraftProfile::detect("Aerosoft A346-MahanAir", "A346"),
            AircraftProfile::AerosoftA346,
        );
        assert_eq!(
            AircraftProfile::detect("PMDG 737-800 GSG", "B738"),
            AircraftProfile::Pmdg737,
        );
        assert_eq!(
            AircraftProfile::detect("PMDG 777-300ER", "B77W"),
            AircraftProfile::Pmdg777,
        );
    }

    // ---- v0.16.10 (#Premium): PMDG-Premium-Override ----

    /// PmdgState-Carrier mit gefuellten Premium-Feldern (Phase-3-Werte
    /// simuliert) fuer die Override-Tests.
    fn pmdg_premium_sample() -> PmdgState {
        PmdgState {
            fma_speed_mode: "N1".to_string(),
            fma_roll_mode: "LNAV".to_string(),
            fma_pitch_mode: "VNAV PTH".to_string(),
            fmc_v1_kt: Some(142),
            fmc_vr_kt: Some(145),
            fmc_v2_kt: Some(151),
            fmc_vref_kt: Some(138),
            reverser_deployed: Some(true),
            master_caution: Some(true),
            master_warning: Some(false),
            below_gs: Some(true),
            cabin_altitude_warning: Some(false),
            stab_out_of_trim: Some(true),
            fuel_per_tank_kg: Some(vec![2200.0, 8400.0, 2200.0]),
            minimums_baro_ft: Some(740.0),
            speedbrake_extended: true,
            ..PmdgState::default()
        }
    }

    #[test]
    fn pmdg_premium_override_absent_pmdg_is_noop() {
        // Presence gate — exakt wie beim Autoflight-Override: ohne
        // PMDG-State bleiben ALLE Premium-Felder unangetastet (None).
        let mut s = SimSnapshot::default();
        assert!(s.pmdg.is_none());
        s.apply_pmdg_premium_override();
        assert_eq!(s.fma_lateral_mode, None);
        assert_eq!(s.fma_vertical_mode, None);
        assert_eq!(s.fma_thrust_mode, None);
        assert_eq!(s.master_caution, None);
        assert_eq!(s.master_warning, None);
        assert_eq!(s.reverser_deployed, None);
        assert_eq!(s.below_gs_alert, None);
        assert_eq!(s.cabin_altitude_warning, None);
        assert_eq!(s.stab_out_of_trim, None);
        assert_eq!(s.fuel_per_tank_kg, None);
        assert_eq!(s.v1_kt, None);
        assert_eq!(s.minimums_baro_ft, None);
        // Auch am Boden (Default: on_ground=true) darf ohne PMDG kein
        // Ground-Spoiler-Wert entstehen.
        assert_eq!(s.ground_spoilers_active, None);
    }

    #[test]
    fn pmdg_premium_override_copies_sdk_values() {
        let mut s = SimSnapshot::default(); // on_ground = true
        s.pmdg = Some(pmdg_premium_sample());
        s.apply_pmdg_premium_override();
        // FMA: roll → lateral, pitch → vertical, speed → thrust.
        assert_eq!(s.fma_lateral_mode.as_deref(), Some("LNAV"));
        assert_eq!(s.fma_vertical_mode.as_deref(), Some("VNAV PTH"));
        assert_eq!(s.fma_thrust_mode.as_deref(), Some("N1"));
        // Warn-/Status-Bits — Some(false) bleibt Some(false), das ist
        // ein echter "aus"-Zustand, kein fehlender Wert.
        assert_eq!(s.master_caution, Some(true));
        assert_eq!(s.master_warning, Some(false));
        assert_eq!(s.reverser_deployed, Some(true));
        assert_eq!(s.below_gs_alert, Some(true));
        assert_eq!(s.cabin_altitude_warning, Some(false));
        assert_eq!(s.stab_out_of_trim, Some(true));
        assert_eq!(s.fuel_per_tank_kg, Some(vec![2200.0, 8400.0, 2200.0]));
        assert_eq!(s.minimums_baro_ft, Some(740.0));
        // FMC-V-Speeds (u8 kt) → generische f64-Felder.
        assert_eq!(s.v1_kt, Some(142.0));
        assert_eq!(s.vr_kt, Some(145.0));
        assert_eq!(s.v2_kt, Some(151.0));
        assert_eq!(s.vref_kt, Some(138.0));
        // Boeing hat keine Airbus-Felder — bleiben None.
        assert_eq!(s.vapp_kt, None);
        assert_eq!(s.vls_kt, None);
        // Am Boden: speedbrake_extended → ground_spoilers_active.
        assert_eq!(s.ground_spoilers_active, Some(true));
    }

    #[test]
    fn pmdg_premium_override_fma_empty_strings_stay_none() {
        // Leerer FMA-String = Cockpit zeigt nichts an → KEIN leeres
        // Label downstream, das Feld bleibt None.
        let mut s = SimSnapshot::default();
        s.pmdg = Some(PmdgState::default()); // alle FMA-Strings leer
        s.apply_pmdg_premium_override();
        assert_eq!(s.fma_lateral_mode, None);
        assert_eq!(s.fma_vertical_mode, None);
        assert_eq!(s.fma_thrust_mode, None);
    }

    #[test]
    fn pmdg_premium_override_none_fields_keep_existing_values() {
        // Per-Field-Gating: ein None vom SDK (Mapper noch nicht
        // verdrahtet — Phase 3) darf einen bereits gesetzten Wert
        // nicht ueberschreiben.
        let mut s = SimSnapshot::default();
        s.master_caution = Some(true);
        s.v1_kt = Some(135.0);
        s.pmdg = Some(PmdgState::default()); // alles None/leer
        s.apply_pmdg_premium_override();
        assert_eq!(s.master_caution, Some(true));
        assert_eq!(s.v1_kt, Some(135.0));
    }

    #[test]
    fn pmdg_premium_override_ground_spoilers_only_on_ground() {
        // In der Luft ist `speedbrake_extended` eine normale In-Flight-
        // Speedbrake — KEIN Ground-Spoiler-Signal.
        let mut s = SimSnapshot::default();
        s.on_ground = false;
        s.pmdg = Some(pmdg_premium_sample()); // speedbrake_extended = true
        s.apply_pmdg_premium_override();
        assert_eq!(s.ground_spoilers_active, None);

        // Am Boden wird der Zustand gemappt — auch das "eingefahren"-
        // false ist dann ein echter Wert.
        let mut s = SimSnapshot::default(); // on_ground = true
        let mut p = pmdg_premium_sample();
        p.speedbrake_extended = false;
        s.pmdg = Some(p);
        s.apply_pmdg_premium_override();
        assert_eq!(s.ground_spoilers_active, Some(false));
    }
}
