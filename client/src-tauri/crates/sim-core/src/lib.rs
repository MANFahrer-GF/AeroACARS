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
#[derive(Debug, Clone, Serialize, Deserialize)]
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
///   * `FenixA320`  — Lights / parking brake / flaps wired and verified
///                    in MSFS 2024. AP indicator LVars (`I_FCU_AP*`) were
///                    observed flickering and are intentionally disabled
///                    until a stable source is identified.
///   * `Pmdg737`    — detection only; LVars TBD (PMDG ships its own
///                    SimConnect ClientData SDK, not plain LVars — needs
///                    a separate subscribe path).
///   * `Pmdg777`    — same as 737.
///   * `IniA340`    — detection only; LVar list TBD.
///   * `IniA350`    — detection only; LVar list TBD.
///   * `IniA346Pro` — detection only; LVar list TBD.
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
        if t.contains("flybywire") || t.contains("fbw a32nx") || t.contains("a32nx") {
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
        if t.contains("inibuilds") && t.contains("a340") {
            return Self::IniA340;
        }
        // v0.13.13: FSReborn Phenom 300E. Title aus dem Sim heisst typisch
        // "FSReborn Phenom 300E Tristan Interior" (oder mit anderen
        // Interior-Varianten). Wir matchen tolerant auf fsreborn + phenom +
        // 300 — fängt auch Edge-Cases ab wie "FSR Phenom300" ohne Space.
        if t.contains("fsreborn") && t.contains("phenom") && t.contains("300") {
            return Self::FsrPhenom300e;
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
            Self::FsrPhenom300e => "FSReborn Phenom 300E",
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
}
