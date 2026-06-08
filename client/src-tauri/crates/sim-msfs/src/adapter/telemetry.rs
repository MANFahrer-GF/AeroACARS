//! Static SimVar list + byte-level parser for the data block
//! SimConnect sends back per `SIMCONNECT_RECV_SIMOBJECT_DATA`.
//!
//! Each entry in [`TELEMETRY_FIELDS`] is added in order to the data
//! definition; the parser reads the same order at fixed offsets. The
//! whole point of this module is that **a single rejected SimVar can
//! never shift another field's position** — every field knows its
//! width and we walk the buffer step by step. If a SimVar is rejected
//! by SimConnect, the data block is shorter than expected and `parse`
//! either returns the value or [`f64::NAN`] / `0` for the missing
//! tail; nothing prior shifts.

use chrono::Utc;
use sim_core::{clean_atc_model, AircraftProfile, SimSnapshot, Simulator};

const KG_PER_LB: f64 = 0.453_592_37;

#[derive(Debug, Clone, Copy)]
pub enum FieldKind {
    /// 8-byte IEEE 754.
    Float64,
    /// 4-byte signed integer (SimConnect bool is INT32).
    Int32,
    /// 256-byte fixed buffer, NUL-terminated.
    String256,
}

impl FieldKind {
    pub fn size(self) -> usize {
        match self {
            FieldKind::Float64 => 8,
            FieldKind::Int32 => 4,
            FieldKind::String256 => 256,
        }
    }
}

/// Static description of one telemetry field.
#[derive(Debug, Clone, Copy)]
pub struct TelemetryField {
    pub name: &'static str,
    pub unit: &'static str,
    pub kind: FieldKind,
}

/// Order matters: this is exactly the order in which SimConnect will
/// pack the bytes for us.
pub const TELEMETRY_FIELDS: &[TelemetryField] = &[
    // ---- Identity ----
    F::str("TITLE", ""),
    F::str("ATC MODEL", ""),
    F::str("ATC ID", ""),
    // ---- Position ----
    F::f64("PLANE LATITUDE", "degrees"),
    F::f64("PLANE LONGITUDE", "degrees"),
    F::f64("PLANE ALTITUDE", "feet"),
    F::f64("PLANE ALT ABOVE GROUND", "feet"),
    // v0.7.17 (B-003): zusaetzliche Altitude-SimVars um den bekannten
    // MSFS-Altimetrie-Bug zu diagnostizieren — `PLANE ALTITUDE` ist
    // geometric MSL und divergiert in arktischer Kaelte oder bei
    // hohen ISA-Abweichungen 1-2k ft vom Cockpit-PFD-Reading. Mode-C-
    // Transponder + VATSIM nutzen pressure altitude.
    //   * INDICATED ALTITUDE: was das Cockpit-PFD zeigt (mit aktuellem
    //     Baro-Setting; in cruise mit STD = pressure altitude)
    //   * PRESSURE ALTITUDE: was Mode-C/VATSIM transmittet (immer STD)
    // Refs: swift-project/pilotclient #169, MSFS DevSupport-Threads.
    F::f64("INDICATED ALTITUDE", "feet"),
    F::f64("PRESSURE ALTITUDE", "feet"),
    // ---- Attitude / motion ----
    F::f64("PLANE HEADING DEGREES TRUE", "degrees"),
    F::f64("PLANE HEADING DEGREES MAGNETIC", "degrees"),
    F::f64("PLANE PITCH DEGREES", "degrees"),
    F::f64("PLANE BANK DEGREES", "degrees"),
    F::f64("VERTICAL SPEED", "feet per minute"),
    // Body-frame velocity. Used at touchdown to derive sideslip /
    // crab natively (atan2(VEL_BODY_X, VEL_BODY_Z) × 180/π) which is
    // what GEES does. Way more accurate than computing track from
    // successive lat/lon.
    F::f64("VELOCITY BODY X", "feet per second"),
    F::f64("VELOCITY BODY Z", "feet per second"),
    // ---- Speeds ----
    F::f64("GROUND VELOCITY", "knots"),
    F::f64("AIRSPEED INDICATED", "knots"),
    F::f64("AIRSPEED TRUE", "knots"),
    F::f64("G FORCE", "GForce"),
    // Body-frame wind components. Positive AIRCRAFT WIND X = wind
    // from the aircraft's right (= crosswind from the right side).
    // Positive AIRCRAFT WIND Z = tailwind. Sign convention per MSFS
    // SDK; we surface absolute headwind/crosswind in the PIREP.
    F::f64("AIRCRAFT WIND X", "knots"),
    F::f64("AIRCRAFT WIND Z", "knots"),
    // ---- Aircraft state ----
    F::bool("SIM ON GROUND"),
    F::bool("BRAKE PARKING POSITION"),
    F::bool("STALL WARNING"),
    F::bool("OVERSPEED WARNING"),
    F::f64("GEAR POSITION", "percent over 100"),
    F::f64("FLAPS HANDLE PERCENT", "percent over 100"),
    F::bool("GENERAL ENG COMBUSTION:1"),
    F::bool("GENERAL ENG COMBUSTION:2"),
    F::bool("GENERAL ENG COMBUSTION:3"),
    F::bool("GENERAL ENG COMBUSTION:4"),
    // v0.13.17: N1 je Triebwerk als Fallback-Signal fuer engines_running.
    // Hintergrund: manche Addons (iniBuilds/Aerosoft A340-600, MSFS 2024)
    // liefern `GENERAL ENG COMBUSTION:N` konstant 0 obwohl die Triebwerke
    // laufen — die Phase-FSM blieb dadurch den ganzen Flug in Pushback
    // haengen (kein Touchdown/Score, PIREP ohne Landedaten — Live-Befund
    // IRM1140/IBE778, 2026-06-03). N1 ist eine Standard-SimVar und bleibt
    // bei diesen Addons gueltig (per Inspektor verifiziert: laufend ~0.66,
    // aus 0). Wirkt addon-agnostisch. Reihenfolge MUSS mit dem pull_f64!-
    // Block in `from_block` uebereinstimmen (Lockstep).
    F::f64("TURB ENG N1:1", "Percent"),
    F::f64("TURB ENG N1:2", "Percent"),
    F::f64("TURB ENG N1:3", "Percent"),
    F::f64("TURB ENG N1:4", "Percent"),
    // ---- Fuel & weight (SU2 EX1 + legacy fallback) ----
    F::f64("FUEL TOTAL QUANTITY WEIGHT EX1", "pounds"),
    F::f64("FUEL TOTAL QUANTITY WEIGHT", "pounds"),
    F::f64("TOTAL WEIGHT", "pounds"),
    F::f64("EMPTY WEIGHT", "pounds"),
    // ---- Environment ----
    F::f64("AMBIENT WIND DIRECTION", "degrees"),
    F::f64("AMBIENT WIND VELOCITY", "knots"),
    F::f64("KOHLSMAN SETTING MB", "millibars"),
    F::f64("AMBIENT TEMPERATURE", "celsius"),
    // Total Air Temperature — what an aircraft thermometer measures
    // (TAT > OAT in flight due to compression heating).
    F::f64("TOTAL AIR TEMPERATURE", "celsius"),
    // Mach number — current aircraft Mach. 0..1 transonic, >1 supersonic.
    F::f64("AIRSPEED MACH", "mach"),
    // ---- Avionics (Phase 5 / SU2-safe standard SimVars) ----
    // All wired by Asobo's simulation core regardless of aircraft;
    // Fenix is the documented exception — it bypasses the standard
    // COM/NAV SimVars and uses internal LVars. We surface the raw
    // values here and the snapshot mapping suppresses them for
    // Fenix to avoid the "1024 MHz" QNH-bleed garbage we saw with
    // the old crate.
    F::f64("TRANSPONDER CODE:1", "BCO16"),
    F::f64("COM ACTIVE FREQUENCY:1", "MHz"),
    F::f64("COM ACTIVE FREQUENCY:2", "MHz"),
    F::f64("NAV ACTIVE FREQUENCY:1", "MHz"),
    F::f64("NAV ACTIVE FREQUENCY:2", "MHz"),
    // ---- Exterior lights ----
    F::bool("LIGHT LANDING"),
    F::bool("LIGHT BEACON"),
    F::bool("LIGHT STROBE"),
    F::bool("LIGHT TAXI"),
    F::bool("LIGHT NAV"),
    F::bool("LIGHT LOGO"),
    // ---- Autopilot ----
    F::bool("AUTOPILOT MASTER"),
    F::bool("AUTOPILOT HEADING LOCK"),
    F::bool("AUTOPILOT ALTITUDE LOCK"),
    F::bool("AUTOPILOT NAV1 LOCK"),
    F::bool("AUTOPILOT APPROACH HOLD"),
    // ---- Powerplant (per-engine fuel flow, summed in mapping) ----
    F::f64("ENG FUEL FLOW PPH:1", "pounds per hour"),
    F::f64("ENG FUEL FLOW PPH:2", "pounds per hour"),
    F::f64("ENG FUEL FLOW PPH:3", "pounds per hour"),
    F::f64("ENG FUEL FLOW PPH:4", "pounds per hour"),

    // ---- Surfaces ----
    // 0..1, position of the spoiler / speed-brake handle.
    F::f64("SPOILERS HANDLE POSITION", "percent over 100"),
    // Auto-spoilers armed for landing (separate from physical handle).
    F::bool("SPOILERS ARMED"),

    // ---- Pushback ----
    // Enum: 0 = Straight, 1 = Left, 2 = Right, 3 = No Pushback.
    // MSFS itself drives this — we use it as the authoritative
    // "pushback finished" signal in the FSM, since the simple
    // "moving + engines on = TaxiOut" trigger fires while the tug
    // is still pushing the aircraft. Value 3 means the tug has
    // disconnected (or the pilot used Ctrl+P to stop), which is
    // when we should advance to TaxiOut.
    F::f64("PUSHBACK STATE", "Enum"),

    // ---- Systems ----
    // APU master switch (0 = off, 1 = on).
    F::bool("APU SWITCH"),
    // APU N (RPM) percentage 0..100. Useful to distinguish "starting"
    // from "running" — the switch is on long before the APU is up.
    F::f64("APU PCT RPM", "percent"),
    // Battery #1 master. Most aircraft only have one battery exposed.
    F::bool("ELECTRICAL MASTER BATTERY:1"),
    F::bool("AVIONICS MASTER SWITCH"),
    F::bool("PITOT HEAT"),
    // Engine anti-ice — sampled per engine, combined to "any-on" in
    // the snapshot mapping so the UI just shows one indicator.
    F::bool("ENG ANTI ICE:1"),
    F::bool("ENG ANTI ICE:2"),
    F::bool("ENG ANTI ICE:3"),
    F::bool("ENG ANTI ICE:4"),
    // Wing / structural deice (Airbus calls this WING ANTI ICE).
    F::bool("STRUCTURAL DEICE SWITCH"),

    // ---- FBW A32NX LVars ----
    // LVars don't get rejected by SimConnect — non-FBW aircraft just
    // read 0 from them, so adding them universally is safe. The
    // snapshot mapping only consults these when AircraftProfile
    // detects FBW. Reference:
    // https://github.com/flybywiresim/aircraft/blob/master/fbw-a32nx/docs/a320-simvars.md
    F::f64("L:A32NX_TRANSPONDER_CODE", "Number"),
    F::f64("L:A32NX_AUTOPILOT_ACTIVE", "Bool"),
    F::f64("L:A32NX_AUTOPILOT_HEADING_HOLD_MODE", "Bool"),
    F::f64("L:A32NX_AUTOPILOT_ALTITUDE_HOLD_MODE", "Bool"),
    F::f64("L:A32NX_AUTOPILOT_LOC_MODE_ACTIVE", "Bool"),
    F::f64("L:A32NX_AUTOPILOT_APPR_MODE_ACTIVE", "Bool"),
    // FBW total fuel quantity, kg — the documented "live" total.
    F::f64("L:A32NX_TOTAL_FUEL_QUANTITY", "Number"),

    // ---- Fenix A320 LVars ----
    // Names verified against the Axis-and-Ohs Fenix script bundle
    // shipped at docs/vendor/FENIX_A3XX_AxisAndOhs_Scripts.xml — each
    // LVar below appears in that file as either a read or a write
    // target, so the names are stable for Fenix Block 2.
    //
    // Naming convention (from Fenix's `Cockpit_Behavior.xml`):
    //   * `L:S_OH_*` — overhead switch *state* (instantaneous position)
    //   * `L:S_FCU_*` — FCU button *state* (push state)
    //   * `L:E_FCU_*` — FCU encoder *display value* (selected ALT/HDG/…)
    //   * `L:I_MIP_*` — MIP indicator *lamp* (Autobrake LO/MED/MAX)
    //   * `L:S_MIP_*` — MIP switch *state*
    //
    // LVars never get rejected by SimConnect; a non-Fenix aircraft
    // just reads 0 from them, so the byte-level parser stays
    // healthy. The snapshot mapping consults each LVar only when
    // AircraftProfile::FenixA320 is detected.

    // Lights overhead (already wired before this batch).
    // Beacon switch: 0 = OFF, 1 = ON.
    F::f64("L:S_OH_EXT_LT_BEACON", "Number"),
    // Strobe selector: 0 = OFF, 1 = AUTO, 2 = ON.
    F::f64("L:S_OH_EXT_LT_STROBE", "Number"),
    // Combined nav + logo: 0 = OFF, 1 = NAV only, 2 = NAV + LOGO.
    F::f64("L:S_OH_EXT_LT_NAV_LOGO", "Number"),
    // Parking brake on Fenix MIP: 0 = released, 1 = set.
    F::f64("L:S_MIP_PARKING_BRAKE", "Number"),

    // Cabin signs: real A320 has 3-pos toggles (OFF/AUTO/ON);
    // Fenix exposes them under the SIGNS namespace, NOT under
    // INT_LT as my first guess assumed.
    F::f64("L:S_OH_SIGNS", "Number"),
    F::f64("L:S_OH_SIGNS_SMOKING", "Number"),

    // APU electrical pushbuttons.
    F::f64("L:S_OH_ELEC_APU_MASTER", "Number"),
    F::f64("L:S_OH_ELEC_APU_START", "Number"),

    // Anti-ice (engine + wing). The PROBE/WINDOW HEAT switch lives
    // outside the PNEUMATIC namespace by Fenix's convention.
    F::f64("L:S_OH_PNEUMATIC_ENG1_ANTI_ICE", "Number"),
    F::f64("L:S_OH_PNEUMATIC_ENG2_ANTI_ICE", "Number"),
    F::f64("L:S_OH_PNEUMATIC_WING_ANTI_ICE", "Number"),
    F::f64("L:S_OH_PROBE_HEAT", "Number"),

    // Electric panel.
    F::f64("L:S_OH_ELEC_BAT1", "Number"),
    F::f64("L:S_OH_ELEC_BAT2", "Number"),
    F::f64("L:S_OH_ELEC_EXT_PWR", "Number"),

    // FCU button states — replace the unreliable `L:I_FCU_*` lamp
    // LVars from earlier sessions. The S_ prefix is the button
    // press state, which actually toggles cleanly.
    F::f64("L:S_FCU_AP1", "Number"),
    F::f64("L:S_FCU_AP2", "Number"),
    F::f64("L:S_FCU_APPR", "Number"),
    F::f64("L:S_FCU_ATHR", "Number"),

    // FCU encoder displays — what the pilot has selected on the
    // glareshield. Used to log "Selected ALT 36000" / "Selected
    // HDG 280" / etc. as the pilot tunes them.
    F::f64("L:E_FCU_ALTITUDE", "Number"),
    F::f64("L:E_FCU_HEADING", "Number"),
    F::f64("L:E_FCU_SPEED", "Number"),
    F::f64("L:E_FCU_VS", "Number"),

    // Autobrake setting indicators (lamp LVars on the MIP).
    F::f64("L:I_MIP_AUTOBRAKE_LO_L", "Number"),
    F::f64("L:I_MIP_AUTOBRAKE_MED_L", "Number"),
    F::f64("L:I_MIP_AUTOBRAKE_MAX_L", "Number"),

    // ---- Fenix A32x Beta LVars (v0.7.16) ----
    // Read from the verified `FNX32X_Interior.xml` shipped with
    // fnx-aircraft-320 / fnx-aircraft-319-321. All names cross-checked
    // against `<VAR_NAME>` entries in the live Fenix install. These
    // fields are always read into the parsed Telemetry block so the
    // payload layout stays stable; the mapping into SimSnapshot only
    // takes effect when `fenix_beta_enabled` is true (set via the
    // Tauri command `set_fenix_beta_enabled`, default off).
    //
    // Wing light: 0 = off, 1 = on. Boeing-typical "WING" inspection
    // light; Airbus pilots toggle it at night or when checking icing.
    F::f64("L:S_OH_EXT_LT_WING", "Number"),
    // Runway turnoff: 0 = off, 1 = on. Two separate lamps on the
    // nose gear strut; Fenix exposes them as one combined switch.
    F::f64("L:S_OH_EXT_LT_RWY_TURNOFF", "Number"),
    // Landing lights L/R: 0 = retracted, 1 = off, 2 = on. The
    // 3-position selector models the real A320 — retracted is the
    // stowed position pre-takeoff.
    F::f64("L:S_OH_EXT_LT_LANDING_L", "Number"),
    F::f64("L:S_OH_EXT_LT_LANDING_R", "Number"),
    // Composite "BOTH" selector (line 680 in FNX32X_Interior.xml):
    // Fenix wires a single switch that drives L+R together. Beta QS
    // task is to verify it stays in sync with the individual L/R
    // switches — we read it for cross-check but the mapping uses
    // L/R as the source of truth.
    F::f64("L:S_OH_EXT_LT_LANDING_BOTH", "Number"),
    // Nose light: 0 = off, 1 = taxi, 2 = T.O. Combines nose taxi
    // and nose take-off into one 3-position switch.
    F::f64("L:S_OH_EXT_LT_NOSE", "Number"),
    // Flaps lever: enum 0..5 (UP, 1, 1+F, 2, 3, FULL). Beta-only:
    // the existing `FLAPS HANDLE PERCENT` SimVar works on Fenix; this
    // adds the lever *detent* as a cross-check value the activity log
    // can pin against (e.g. "Lever 1+F" vs the percentage).
    F::f64("L:S_FC_FLAPS", "Number"),

    // ---- FSReborn Phenom 300E LVars (v0.13.13) ----
    // Pilot-Befund Michael 2026-05-26: AeroACARS Auto-Start scheiterte mit
    // "Triebwerke sind an" obwohl FSR Phenom 300E Cold&Dark stand. Standard
    // SimVar GENERAL ENG COMBUSTION:N liefert beim FSReborn vor erstem
    // Engine-Start unzuverlaessige Werte. Loesung: FSR-eigene Engine-Knob-
    // LVars lesen, die den Pilot-Befehl direkt reflektieren.
    //
    // LVar-Werte (aus HubHop-Dump verifiziert):
    //   0 = STOP   (engine commanded off)
    //   1 = RUN    (engine commanded running)
    //   2 = START  (engine in start sequence)
    //
    // Andere Telemetrie-Felder (N1/N2/Fuel/Gear/Flaps) kommen sauber
    // ueber Standard-SimVars — kein Override noetig. Siehe HubHop-Audit
    // docs/dev/lvar-discovery-hubhop.md fuer den vollen LVar-Katalog.
    F::f64("L:FSR_300E_ENGINE1_KNOB_POS", "Number"),
    F::f64("L:FSR_300E_ENGINE2_KNOB_POS", "Number"),
];

// Helper builders so the table above stays compact.
struct F;
impl F {
    const fn str(name: &'static str, unit: &'static str) -> TelemetryField {
        TelemetryField {
            name,
            unit,
            kind: FieldKind::String256,
        }
    }
    const fn f64(name: &'static str, unit: &'static str) -> TelemetryField {
        TelemetryField {
            name,
            unit,
            kind: FieldKind::Float64,
        }
    }
    const fn bool(name: &'static str) -> TelemetryField {
        TelemetryField {
            name,
            unit: "bool",
            kind: FieldKind::Int32,
        }
    }
}

/// Decoded telemetry — one snapshot's worth of values, before the
/// final mapping into [`SimSnapshot`].
#[derive(Debug, Default)]
pub struct Telemetry {
    pub title: String,
    pub atc_model: String,
    pub atc_id: String,

    pub lat: f64,
    pub lon: f64,
    pub altitude_msl_ft: f64,
    pub altitude_agl_ft: f64,
    /// v0.7.17 (B-003) — INDICATED ALTITUDE (cockpit PFD reading,
    /// baro-corrected). 0.0 when the SimVar is absent.
    pub altitude_indicated_ft: f64,
    /// v0.7.17 (B-003) — PRESSURE ALTITUDE (always STD). 0.0 when absent.
    pub altitude_pressure_ft: f64,

    pub heading_true_deg: f64,
    pub heading_magnetic_deg: f64,
    pub pitch_deg: f64,
    pub bank_deg: f64,
    pub vertical_speed_fpm: f64,
    /// Body-frame velocity components in feet per second. Used to
    /// compute sideslip / crab angle natively at touchdown.
    pub velocity_body_x_fps: f64,
    pub velocity_body_z_fps: f64,

    pub groundspeed_kt: f64,
    pub indicated_airspeed_kt: f64,
    pub true_airspeed_kt: f64,
    pub g_force: f64,
    /// Body-frame wind components in knots. Positive aircraft_wind_x
    /// = crosswind from the right; positive aircraft_wind_z = tailwind.
    pub aircraft_wind_x_kt: f64,
    pub aircraft_wind_z_kt: f64,

    pub on_ground: bool,
    pub parking_brake: bool,
    pub stall_warning: bool,
    pub overspeed_warning: bool,
    pub gear_position: f64,
    pub flaps_position: f64,
    pub eng1_firing: bool,
    pub eng2_firing: bool,
    pub eng3_firing: bool,
    pub eng4_firing: bool,
    /// v0.13.17: N1 je Triebwerk (TURB ENG N1:1..4). Fallback-Signal fuer
    /// `engines_running`, wenn `GENERAL ENG COMBUSTION` konstant 0 liefert
    /// (siehe TELEMETRY_FIELDS-Kommentar). Skala je Addon 0-1 ODER 0-100;
    /// die Auswertung normalisiert.
    pub n1_pct_1: f64,
    pub n1_pct_2: f64,
    pub n1_pct_3: f64,
    pub n1_pct_4: f64,

    pub fuel_total_lb_ex1: f64,
    pub fuel_total_lb_legacy: f64,
    pub total_weight_lb: f64,
    pub empty_weight_lb: f64,

    pub wind_direction_deg: f64,
    pub wind_speed_kt: f64,
    pub qnh_hpa: f64,
    pub oat_c: f64,
    pub tat_c: f64,
    pub mach: f64,

    pub transponder_bcd: f64,
    pub com1_mhz: f64,
    pub com2_mhz: f64,
    pub nav1_mhz: f64,
    pub nav2_mhz: f64,

    pub light_landing: bool,
    pub light_beacon: bool,
    pub light_strobe: bool,
    pub light_taxi: bool,
    pub light_nav: bool,
    pub light_logo: bool,

    pub ap_master: bool,
    pub ap_heading: bool,
    pub ap_altitude: bool,
    pub ap_nav: bool,
    pub ap_approach: bool,

    pub eng1_ff_pph: f64,
    pub eng2_ff_pph: f64,
    pub eng3_ff_pph: f64,
    pub eng4_ff_pph: f64,

    pub spoilers_handle_position: f64,
    pub spoilers_armed: bool,

    pub pushback_state: f64,

    pub apu_switch: bool,
    pub apu_pct_rpm: f64,
    pub battery_master: bool,
    pub avionics_master: bool,
    pub pitot_heat: bool,
    pub eng1_anti_ice: bool,
    pub eng2_anti_ice: bool,
    pub eng3_anti_ice: bool,
    pub eng4_anti_ice: bool,
    pub structural_deice: bool,

    // FBW A32NX LVars
    pub fbw_xpdr: f64,
    pub fbw_ap_active: f64,
    pub fbw_ap_hdg: f64,
    pub fbw_ap_alt: f64,
    pub fbw_ap_nav: f64,
    pub fbw_ap_appr: f64,
    pub fbw_total_fuel_kg: f64,

    // Fenix A320 LVars
    pub fnx_beacon: f64,
    pub fnx_strobe: f64,
    pub fnx_nav_logo: f64,
    pub fnx_park_brake: f64,
    pub fnx_signs_seatbelts: f64,
    pub fnx_signs_smoking: f64,
    pub fnx_apu_master: f64,
    pub fnx_apu_start: f64,
    pub fnx_eng1_anti_ice: f64,
    pub fnx_eng2_anti_ice: f64,
    pub fnx_wing_anti_ice: f64,
    pub fnx_probe_heat: f64,
    pub fnx_bat1: f64,
    pub fnx_bat2: f64,
    pub fnx_ext_pwr: f64,
    pub fnx_fcu_ap1: f64,
    pub fnx_fcu_ap2: f64,
    pub fnx_fcu_appr: f64,
    pub fnx_fcu_athr: f64,
    pub fnx_fcu_alt: f64,
    pub fnx_fcu_hdg: f64,
    pub fnx_fcu_spd: f64,
    pub fnx_fcu_vs: f64,
    pub fnx_autobrake_lo: f64,
    pub fnx_autobrake_med: f64,
    pub fnx_autobrake_max: f64,

    // v0.7.16 Fenix A32x extension LVars (read-only).
    pub fnx_ext_lt_wing: f64,
    pub fnx_ext_lt_rwy_turnoff: f64,
    pub fnx_ext_lt_landing_l: f64,
    pub fnx_ext_lt_landing_r: f64,
    pub fnx_ext_lt_landing_both: f64,
    pub fnx_ext_lt_nose: f64,
    pub fnx_fc_flaps_lever: f64,

    // v0.13.13: FSReborn Phenom 300E Engine-Knob-State (0=STOP, 1=RUN,
    // 2=START). Wird in `telemetry_to_snapshot` als Quelle fuer
    // engines_running genutzt wenn AircraftProfile::FsrPhenom300e
    // detected ist — Standard SimVar GENERAL ENG COMBUSTION:N ist beim
    // FSR in Cold&Dark unzuverlaessig (Pilot-Befund Michael 2026-05-26).
    pub fsr_phenom_eng1_knob: f64,
    pub fsr_phenom_eng2_knob: f64,
}

// ---- Touchdown sample (separate data definition #2) ----
//
// MSFS itself latches these the moment the gear contacts the ground;
// values stay frozen until the next takeoff. Lives in its own data
// definition so a rejection (e.g. on aircraft / sim builds that
// don't expose all of these yet) can't shift the per-tick telemetry
// layout. Verified field names + units against the MSFS 2024 SDK
// docs:
// https://docs.flightsimulator.com/msfs2024/html/6_Programming_APIs/SimVars/Aircraft_SimVars/Aircraft_Misc_Variables.htm

pub const TOUCHDOWN_FIELDS: &[TelemetryField] = &[
    F::f64("PLANE TOUCHDOWN NORMAL VELOCITY", "feet per second"),
    F::f64("PLANE TOUCHDOWN PITCH DEGREES", "degrees"),
    F::f64("PLANE TOUCHDOWN BANK DEGREES", "degrees"),
    F::f64("PLANE TOUCHDOWN HEADING DEGREES MAGNETIC", "degrees"),
    F::f64("PLANE TOUCHDOWN LATITUDE", "radians"),
    F::f64("PLANE TOUCHDOWN LONGITUDE", "radians"),
];

#[derive(Debug, Default, Clone, Copy)]
pub struct Touchdown {
    pub vs_fps: f64,
    pub pitch_deg: f64,
    pub bank_deg: f64,
    pub heading_mag_deg: f64,
    pub lat_rad: f64,
    pub lon_rad: f64,
}

impl Touchdown {
    pub fn from_block(bytes: &[u8]) -> Self {
        let mut t = Touchdown::default();
        let mut off = 0usize;
        if let Some(v) = read_f64(bytes, off) { t.vs_fps = v; }
        off += 8;
        if let Some(v) = read_f64(bytes, off) { t.pitch_deg = v; }
        off += 8;
        if let Some(v) = read_f64(bytes, off) { t.bank_deg = v; }
        off += 8;
        if let Some(v) = read_f64(bytes, off) { t.heading_mag_deg = v; }
        off += 8;
        if let Some(v) = read_f64(bytes, off) { t.lat_rad = v; }
        off += 8;
        if let Some(v) = read_f64(bytes, off) { t.lon_rad = v; }
        let _ = off;
        t
    }

    pub fn _dummy() {} // keep impl block aligned
}

// ---- Live SimVar/LVar Inspector (debug feature) ----
//
// A user-driven watchlist that registers arbitrary SimVar / LVar
// names against SimConnect at runtime. Lives behind a separate data
// definition (#3) so the user can add a name with a typo without
// breaking real telemetry.

/// Type discriminator for watched values. Matches the SimConnect
/// data type we use for the corresponding `AddToDataDefinition` call.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WatchKind {
    /// FLOAT64. Use for raw numeric SimVars and LVars (most cases).
    Number,
    /// INT32. Use for SimConnect bool SimVars (e.g. SIM ON GROUND).
    Bool,
    /// STRING256. Use for TITLE / ATC MODEL etc.
    String,
}

impl WatchKind {
    pub fn size(self) -> usize {
        match self {
            WatchKind::Number => 8,
            WatchKind::Bool => 4,
            WatchKind::String => 256,
        }
    }
}

/// Latest value for one watch entry.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum WatchValue {
    Number(f64),
    Bool(bool),
    String(String),
}

/// One entry in the inspector's watchlist. `value` is None until the
/// next dispatch tick after registration.
#[derive(Debug, Clone, serde::Serialize)]
pub struct InspectorWatch {
    pub id: u32,
    pub name: String,
    pub unit: String,
    pub kind: WatchKind,
    /// Set whenever a SIMCONNECT_RECV_EXCEPTION fires for this entry
    /// during registration, so the UI can render an error indicator
    /// instead of a stale value.
    pub error: Option<String>,
    pub value: Option<WatchValue>,
}

/// Mutable inspector state, owned by the adapter's `Shared`.
#[derive(Debug, Default)]
pub struct InspectorState {
    pub watches: Vec<InspectorWatch>,
    pub next_id: u32,
    /// Set when the watchlist has changed and the worker needs to
    /// re-register data definition #3.
    pub dirty: bool,
}

impl InspectorState {
    pub fn add(&mut self, name: String, unit: String, kind: WatchKind) -> u32 {
        self.next_id += 1;
        let id = self.next_id;
        self.watches.push(InspectorWatch {
            id,
            name,
            unit,
            kind,
            error: None,
            value: None,
        });
        self.dirty = true;
        id
    }

    pub fn remove(&mut self, id: u32) {
        let before = self.watches.len();
        self.watches.retain(|w| w.id != id);
        if self.watches.len() != before {
            self.dirty = true;
        }
    }

    /// Parse the data block returned by SimConnect for the inspector
    /// definition — fields are at fixed offsets in watchlist order,
    /// same parsing model as the main telemetry block.
    pub fn ingest(&mut self, bytes: &[u8]) {
        let mut off = 0usize;
        for w in &mut self.watches {
            match w.kind {
                WatchKind::Number => {
                    if let Some(v) = read_f64(bytes, off) {
                        w.value = Some(WatchValue::Number(v));
                    }
                    off += 8;
                }
                WatchKind::Bool => {
                    if let Some(v) = read_i32(bytes, off) {
                        w.value = Some(WatchValue::Bool(v != 0));
                    }
                    off += 4;
                }
                WatchKind::String => {
                    if let Some(v) = read_str256(bytes, off) {
                        w.value = Some(WatchValue::String(v));
                    }
                    off += 256;
                }
            }
        }
    }
}

impl Touchdown {
    /// `true` while no *real* touchdown has happened yet.
    ///
    /// MSFS populates the PLANE TOUCHDOWN * SimVars with the
    /// aircraft's current state when it's spawned on the ground —
    /// matching live position, heading, pitch — but with `vs_fps`
    /// at exactly 0. That's not a useful "touchdown" for an ACARS
    /// landing analyzer, only an actual descent ends with a
    /// non-zero touchdown rate. Filtering on `vs_fps == 0` cleanly
    /// rejects both the all-zero pre-spawn state and the
    /// spawn-on-ground state, leaving real landings to come
    /// through.
    pub fn is_uninitialised(&self) -> bool {
        self.vs_fps == 0.0
    }
}

impl Telemetry {
    fn from_block(bytes: &[u8]) -> Self {
        // Walk the buffer in TELEMETRY_FIELDS order. If the buffer is
        // shorter than expected (some SimVar got rejected and the
        // tail is missing), every later field stays at its default.
        let mut t = Telemetry::default();
        let mut off = 0usize;

        // Macro-equivalent: pull next field into `dst` if the buffer
        // is long enough. Strings copy the NUL-terminated content.
        macro_rules! pull_f64 {
            ($dst:expr) => {
                if let Some(v) = read_f64(bytes, off) {
                    $dst = v;
                }
                off += 8;
            };
        }
        macro_rules! pull_i32 {
            ($dst:expr) => {
                if let Some(v) = read_i32(bytes, off) {
                    $dst = v != 0;
                }
                off += 4;
            };
        }
        macro_rules! pull_str {
            ($dst:expr) => {
                if let Some(v) = read_str256(bytes, off) {
                    $dst = v;
                }
                off += 256;
            };
        }

        // Same order as TELEMETRY_FIELDS — keep these in lock-step.
        pull_str!(t.title);
        pull_str!(t.atc_model);
        pull_str!(t.atc_id);

        pull_f64!(t.lat);
        pull_f64!(t.lon);
        pull_f64!(t.altitude_msl_ft);
        pull_f64!(t.altitude_agl_ft);
        pull_f64!(t.altitude_indicated_ft);
        pull_f64!(t.altitude_pressure_ft);

        pull_f64!(t.heading_true_deg);
        pull_f64!(t.heading_magnetic_deg);
        pull_f64!(t.pitch_deg);
        pull_f64!(t.bank_deg);
        pull_f64!(t.vertical_speed_fpm);
        pull_f64!(t.velocity_body_x_fps);
        pull_f64!(t.velocity_body_z_fps);

        pull_f64!(t.groundspeed_kt);
        pull_f64!(t.indicated_airspeed_kt);
        pull_f64!(t.true_airspeed_kt);
        pull_f64!(t.g_force);
        pull_f64!(t.aircraft_wind_x_kt);
        pull_f64!(t.aircraft_wind_z_kt);

        pull_i32!(t.on_ground);
        pull_i32!(t.parking_brake);
        pull_i32!(t.stall_warning);
        pull_i32!(t.overspeed_warning);
        pull_f64!(t.gear_position);
        pull_f64!(t.flaps_position);
        pull_i32!(t.eng1_firing);
        pull_i32!(t.eng2_firing);
        pull_i32!(t.eng3_firing);
        pull_i32!(t.eng4_firing);
        // v0.13.17: N1 je Triebwerk — MUSS direkt nach den COMBUSTION-
        // Feldern stehen (Lockstep mit TELEMETRY_FIELDS).
        pull_f64!(t.n1_pct_1);
        pull_f64!(t.n1_pct_2);
        pull_f64!(t.n1_pct_3);
        pull_f64!(t.n1_pct_4);

        pull_f64!(t.fuel_total_lb_ex1);
        pull_f64!(t.fuel_total_lb_legacy);
        pull_f64!(t.total_weight_lb);
        pull_f64!(t.empty_weight_lb);

        pull_f64!(t.wind_direction_deg);
        pull_f64!(t.wind_speed_kt);
        pull_f64!(t.qnh_hpa);
        pull_f64!(t.oat_c);
        pull_f64!(t.tat_c);
        pull_f64!(t.mach);

        pull_f64!(t.transponder_bcd);
        pull_f64!(t.com1_mhz);
        pull_f64!(t.com2_mhz);
        pull_f64!(t.nav1_mhz);
        pull_f64!(t.nav2_mhz);

        pull_i32!(t.light_landing);
        pull_i32!(t.light_beacon);
        pull_i32!(t.light_strobe);
        pull_i32!(t.light_taxi);
        pull_i32!(t.light_nav);
        pull_i32!(t.light_logo);

        pull_i32!(t.ap_master);
        pull_i32!(t.ap_heading);
        pull_i32!(t.ap_altitude);
        pull_i32!(t.ap_nav);
        pull_i32!(t.ap_approach);

        pull_f64!(t.eng1_ff_pph);
        pull_f64!(t.eng2_ff_pph);
        pull_f64!(t.eng3_ff_pph);
        pull_f64!(t.eng4_ff_pph);

        pull_f64!(t.spoilers_handle_position);
        pull_i32!(t.spoilers_armed);

        pull_f64!(t.pushback_state);

        pull_i32!(t.apu_switch);
        pull_f64!(t.apu_pct_rpm);
        pull_i32!(t.battery_master);
        pull_i32!(t.avionics_master);
        pull_i32!(t.pitot_heat);
        pull_i32!(t.eng1_anti_ice);
        pull_i32!(t.eng2_anti_ice);
        pull_i32!(t.eng3_anti_ice);
        pull_i32!(t.eng4_anti_ice);
        pull_i32!(t.structural_deice);

        pull_f64!(t.fbw_xpdr);
        pull_f64!(t.fbw_ap_active);
        pull_f64!(t.fbw_ap_hdg);
        pull_f64!(t.fbw_ap_alt);
        pull_f64!(t.fbw_ap_nav);
        pull_f64!(t.fbw_ap_appr);
        pull_f64!(t.fbw_total_fuel_kg);

        pull_f64!(t.fnx_beacon);
        pull_f64!(t.fnx_strobe);
        pull_f64!(t.fnx_nav_logo);
        pull_f64!(t.fnx_park_brake);
        pull_f64!(t.fnx_signs_seatbelts);
        pull_f64!(t.fnx_signs_smoking);
        pull_f64!(t.fnx_apu_master);
        pull_f64!(t.fnx_apu_start);
        pull_f64!(t.fnx_eng1_anti_ice);
        pull_f64!(t.fnx_eng2_anti_ice);
        pull_f64!(t.fnx_wing_anti_ice);
        pull_f64!(t.fnx_probe_heat);
        pull_f64!(t.fnx_bat1);
        pull_f64!(t.fnx_bat2);
        pull_f64!(t.fnx_ext_pwr);
        pull_f64!(t.fnx_fcu_ap1);
        pull_f64!(t.fnx_fcu_ap2);
        pull_f64!(t.fnx_fcu_appr);
        pull_f64!(t.fnx_fcu_athr);
        pull_f64!(t.fnx_fcu_alt);
        pull_f64!(t.fnx_fcu_hdg);
        pull_f64!(t.fnx_fcu_spd);
        pull_f64!(t.fnx_fcu_vs);
        pull_f64!(t.fnx_autobrake_lo);
        pull_f64!(t.fnx_autobrake_med);
        pull_f64!(t.fnx_autobrake_max);

        // v0.7.16 Fenix A32x extension LVars — same TELEMETRY_FIELDS
        // order as registered above.
        pull_f64!(t.fnx_ext_lt_wing);
        pull_f64!(t.fnx_ext_lt_rwy_turnoff);
        pull_f64!(t.fnx_ext_lt_landing_l);
        pull_f64!(t.fnx_ext_lt_landing_r);
        pull_f64!(t.fnx_ext_lt_landing_both);
        pull_f64!(t.fnx_ext_lt_nose);
        pull_f64!(t.fnx_fc_flaps_lever);

        // v0.13.13: FSR Phenom 300E Engine-Knob LVars — gleiche
        // TELEMETRY_FIELDS-Reihenfolge wie oben registriert. Werte:
        //   0 = STOP   (engine commanded off)
        //   1 = RUN    (engine commanded running)
        //   2 = START  (engine in start sequence)
        pull_f64!(t.fsr_phenom_eng1_knob);
        pull_f64!(t.fsr_phenom_eng2_knob);

        // Silence the unused-assignment warning the last `pull_*!`
        // emits (the macro always advances `off`, but the very last
        // call doesn't read it again).
        let _ = off;

        t
    }
}

/// Convenience used by the worker: parse + remap to `SimSnapshot`.
///
/// v0.7.17 (F-001): The previous `fenix_beta_enabled` parameter is
/// removed. The Fenix-A32x LVAR overrides are now ALWAYS applied
/// when `AircraftProfile::is_fenix()` returns true. Tester-Feedback
/// during the v0.7.16 opt-in beta phase was positive — no regression
/// observed, mapping verified against the live `FNX32X_Interior.xml`.
pub fn parse(bytes: &[u8], simulator: Simulator) -> SimSnapshot {
    let t = Telemetry::from_block(bytes);
    telemetry_to_snapshot(t, simulator)
}

/// Map 0.0 → None, anything > 0 → Some. Used for SimVars where a
/// genuine zero is meaningless (frequencies, percentages) so we can
/// tell "this addon doesn't wire it" from "it's actually zero".
fn positive_or_none(v: f32) -> Option<f32> {
    if v > 0.0 { Some(v) } else { None }
}

fn read_f64(bytes: &[u8], off: usize) -> Option<f64> {
    bytes.get(off..off + 8).map(|s| {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(s);
        f64::from_le_bytes(buf)
    })
}

fn read_i32(bytes: &[u8], off: usize) -> Option<i32> {
    bytes.get(off..off + 4).map(|s| {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(s);
        i32::from_le_bytes(buf)
    })
}

fn read_str256(bytes: &[u8], off: usize) -> Option<String> {
    bytes.get(off..off + 256).map(|s| {
        let end = s.iter().position(|b| *b == 0).unwrap_or(s.len());
        String::from_utf8_lossy(&s[..end]).into_owned()
    })
}

fn telemetry_to_snapshot(t: Telemetry, simulator: Simulator) -> SimSnapshot {
    let profile = AircraftProfile::detect(&t.title, &t.atc_model);
    let is_fenix = profile.is_fenix();
    let is_fbw = matches!(profile, AircraftProfile::FbwA32nx);
    // v0.7.17 (F-001): Fenix-A32x extension LVARs are now ALWAYS applied
    // when the profile is Fenix — the v0.7.16 opt-in flag is removed
    // after a positive testing phase. The branch below is kept under
    // the same `fenix_beta` name to avoid touching every downstream
    // site; semantically it's now just "is a Fenix profile".
    let fenix_beta = is_fenix;

    // v0.13.13: FSR Phenom 300E nutzt den Engine-Knob-LVar statt
    // `GENERAL ENG COMBUSTION:N`. Hintergrund: der Standard-SimVar liefert
    // beim FSR-Phenom in Cold&Dark > 0 obwohl Engines aus — Auto-Start
    // scheiterte mit "Triebwerke sind an" (Pilot-Befund Michael 26.05.2026).
    // Knob-Werte: 0 = STOP, 1 = RUN, 2 = START. Beides >0 = Engine commanded
    // on. Real airborne Phase laeuft sowieso ueber knob=1 (RUN), also
    // konsistent mit dem GENERAL ENG COMBUSTION-Verhalten ueber den
    // ganzen Flug — nur Cold&Dark wird korrekt.
    let is_fsr_phenom = matches!(profile, AircraftProfile::FsrPhenom300e);
    let engines_running = if is_fsr_phenom {
        (if t.fsr_phenom_eng1_knob > 0.5 { 1u8 } else { 0 })
            + (if t.fsr_phenom_eng2_knob > 0.5 { 1u8 } else { 0 })
    } else {
        let combustion = (t.eng1_firing as u8)
            + (t.eng2_firing as u8)
            + (t.eng3_firing as u8)
            + (t.eng4_firing as u8);
        if combustion > 0 {
            combustion
        } else {
            // v0.13.17: `GENERAL ENG COMBUSTION` ist bei manchen Addons
            // (iniBuilds/Aerosoft A340-600, MSFS 2024) konstant 0 obwohl
            // die Triebwerke laufen → Phase-FSM blieb in Pushback haengen
            // (Live IRM1140/IBE778). Fallback: N1 ueber Idle/Windmill-
            // Schwelle = Triebwerk laeuft. N1 kommt je nach Addon als
            // 0-1-Ratio ODER 0-100 % → auf Prozent normalisieren. Greift
            // NUR wenn COMBUSTION komplett 0 ist → kein Regress fuer
            // Flieger, deren COMBUSTION-Flag funktioniert (dort ist N1
            // ohnehin 0 wenn aus). Schwelle bewusst ueber reinem
            // Windmilling (~15 %); am Boden (wo die FSM das Signal
            // braucht) gibt es kein Windmilling, also trennt es dort
            // sauber aus(0) vs laufend.
            const N1_RUNNING_PCT: f64 = 15.0;
            let n1_on = |raw: f64| {
                let pct = if raw <= 1.5 { raw * 100.0 } else { raw };
                pct > N1_RUNNING_PCT
            };
            (n1_on(t.n1_pct_1) as u8)
                + (n1_on(t.n1_pct_2) as u8)
                + (n1_on(t.n1_pct_3) as u8)
                + (n1_on(t.n1_pct_4) as u8)
        }
    };

    // Fuel pick order: FBW LVar (already in kg) > EX1 SimVar (SU2+,
    // works for modern fuel system) > legacy WEIGHT SimVar.
    let fuel_total_kg = if is_fbw && t.fbw_total_fuel_kg > 0.0 {
        t.fbw_total_fuel_kg as f32
    } else if t.fuel_total_lb_ex1 > 0.0 {
        (t.fuel_total_lb_ex1 * KG_PER_LB) as f32
    } else {
        (t.fuel_total_lb_legacy * KG_PER_LB) as f32
    };

    // Gross weight: TOTAL WEIGHT is documented as authoritative.
    let total_weight_kg = if t.total_weight_lb > 0.0 {
        Some((t.total_weight_lb * KG_PER_LB) as f32)
    } else {
        None
    };

    // ZFW = Zero Fuel Weight = gross weight minus current fuel.
    // Matches the value Airbus EFBs / FMCs display under "ZFW".
    // Only meaningful when both inputs are positive — otherwise the
    // arithmetic produces nonsense (e.g. GW=0 - fuel=4700 → -4700).
    let zfw_kg = match total_weight_kg {
        Some(gw) if gw > 0.0 && fuel_total_kg >= 0.0 && gw > fuel_total_kg => {
            Some(gw - fuel_total_kg)
        }
        _ => None,
    };

    // OEW (operating empty weight). Reject implausibly small values —
    // the Asobo A320neo default reports ~1422 kg which is clearly bogus
    // (real OEW is ~42 t). Smallest realistic transport-cat empty
    // weight is a King Air at ~3.5 t / 7700 lb, so we'd ideally clamp
    // there, but for now we just drop literal-zero readings and trust
    // the value otherwise (lets GA addons through).
    let empty_weight_kg: Option<f32> = {
        let kg = (t.empty_weight_lb * KG_PER_LB) as f32;
        if kg > 0.0 { Some(kg) } else { None }
    };

    // Payload = ZFW − OEW. No MSFS SimVar exposes payload directly
    // (Fenix and most addons leave `PAYLOAD WEIGHT` unwired) but the
    // arithmetic is exact: ZFW = OEW + Payload by definition. Skip
    // when either input is missing or the result would be negative
    // (= bogus OEW > ZFW combination).
    let payload_kg: Option<f32> = match (zfw_kg, empty_weight_kg) {
        (Some(z), Some(o)) if z > o => Some(z - o),
        _ => None,
    };

    // Total fuel flow across all running engines, kg/h. Sum the
    // per-engine PPH SimVars and convert.
    let total_ff_pph = t.eng1_ff_pph + t.eng2_ff_pph + t.eng3_ff_pph + t.eng4_ff_pph;
    let fuel_flow_kg_per_h = if total_ff_pph > 0.0 {
        Some((total_ff_pph * KG_PER_LB) as f32)
    } else {
        None
    };

    // Transponder code: FBW writes a plain decimal LVar (e.g.
    // L:A32NX_TRANSPONDER_CODE = 2523 means squawk 2523), the
    // standard SimVar is BCD-encoded (0x1234 = squawk 1234).
    //
    // v0.7.17 (B-002): Bei Fenix-Profilen liefert der Standard-
    // `TRANSPONDER CODE:1` SimVar Werte, die NICHT mit dem cockpit-
    // seitigen RMP/ATC-Display synchronisiert sind — Pilot stellt
    // 2532 ein, AeroACARS meldet weiterhin 2000 (oder einen
    // zufaelligen Pre-Power-Default). Bis ein Fenix-eigener LVAR
    // identifiziert ist, der den echten Code haelt, blenden wir
    // den Wert komplett aus, damit das Activity-Log keinen falschen
    // Squawk loggt. Siehe docs/qs/v0.7.16-fenix-beta-bugs.md B-002.
    let transponder_code = if is_fenix {
        None
    } else if is_fbw && t.fbw_xpdr > 0.0 {
        Some(t.fbw_xpdr.round().clamp(0.0, 7777.0) as u16)
    } else if t.transponder_bcd > 0.0 {
        let raw = t.transponder_bcd.round() as u32;
        let d1 = (raw >> 12) & 0xF;
        let d2 = (raw >> 8) & 0xF;
        let d3 = (raw >> 4) & 0xF;
        let d4 = raw & 0xF;
        Some((d1 * 1000 + d2 * 100 + d3 * 10 + d4) as u16)
    } else {
        None
    };

    // Autopilot:
    //   * FBW: dedicated LVars (live mode state).
    //   * Fenix: the `L:S_FCU_*` button-state LVars from the AAO
    //     script. We treat AP1 OR AP2 active as "Master engaged".
    //     Heading / altitude / NAV button-state isn't directly the
    //     same as "mode is armed", but it's a closer signal than
    //     the I_FCU_* lamp LVars from the legacy session (those
    //     flickered with unrelated cockpit switches).
    //   * Default + others: standard MSFS SimVars.
    let (ap_master, ap_hdg, ap_alt, ap_nav, ap_appr) = if is_fbw {
        (
            t.fbw_ap_active != 0.0,
            t.fbw_ap_hdg != 0.0,
            t.fbw_ap_alt != 0.0,
            t.fbw_ap_nav != 0.0,
            t.fbw_ap_appr != 0.0,
        )
    } else if is_fenix {
        // v0.7.17 (B-008): `L:S_FCU_AP1` / `L:S_FCU_AP2` sind
        // **Button-Press-Pulse** — 0→1→0 bei jedem Klick — NICHT der
        // Engagement-State. Sie sind die ueberwaeltigende Mehrheit der
        // Zeit 0, auch wenn der A320-AP tatsaechlich aktiv ist. Wir
        // lasen sie als Master-Status und meldeten dadurch "AP off"
        // mitten im FL313-Cruise (Tester-Befund Thomas K CFG 2222).
        //
        // Fix: Standard `AUTOPILOT MASTER` SimVar wie fuer alle
        // anderen Modi (HDG/ALT/NAV) verwenden. Fenix's interner
        // FCU-State spiegelt sich gemaess SimConnect-Konventionen
        // im Standard-SimVar wider, solange das Aircraft korrekt
        // wired ist. Falls QS belegt dass auch der Standard bei
        // Fenix entkoppelt ist, brauchen wir Option C aus B-008
        // (Suppression via Option<bool>).
        //
        // Approach-Mode behaelt den Pulse-OR-Standard-Pfad — die
        // APPR-LVAR ist beim Fenix in der Praxis stabiler weil
        // sie an die Mode-Flag-Latch des FMA gebunden ist; falls
        // Standard wired ist, gewinnt der.
        (
            t.ap_master,
            t.ap_heading,
            t.ap_altitude,
            t.ap_nav,
            t.fnx_fcu_appr as i32 != 0 || t.ap_approach,
        )
    } else {
        (
            t.ap_master,
            t.ap_heading,
            t.ap_altitude,
            t.ap_nav,
            t.ap_approach,
        )
    };

    // Lights: Fenix uses overhead-LVars instead of the standard
    // SimVars, with selector positions (off / auto / on; nav-only /
    // nav+logo). Translate to bools for the binary pills, plus a
    // separate `strobe_state` carrying the full 0/1/2 enum so the
    // activity log can distinguish AUTO from ON (real pilots flip
    // between those at runway entry/exit, and we'd lose the event
    // if we collapsed everything to "Strobe lights ON").
    let (light_beacon, light_strobe, light_nav, light_logo) = if is_fenix {
        (
            t.fnx_beacon as i32 != 0,
            t.fnx_strobe as i32 != 0,
            t.fnx_nav_logo as i32 >= 1,
            t.fnx_nav_logo as i32 >= 2,
        )
    } else {
        (t.light_beacon, t.light_strobe, t.light_nav, t.light_logo)
    };
    let strobe_state = if is_fenix {
        Some(t.fnx_strobe.round().clamp(0.0, 2.0) as u8)
    } else {
        None
    };

    // v0.7.16 — additive Fenix-A32x lights/lever (only when
    // `fenix_beta_enabled` is set on the adapter). All LVAR names
    // verified against `SimObjects\Airplanes\FNX_32X\model\
    // FNX32X_Interior.xml` in the live Fenix install.
    //
    //   * `L:S_OH_EXT_LT_LANDING_L`/`_R` are 3-position selectors
    //     (0 = retracted, 1 = off, 2 = on). "Light landing" is true
    //     iff at least one side is in the "on" position (= 2). The
    //     stowed/off positions both count as off — pilots leave the
    //     lights retracted on the ground for life-cycle reasons, the
    //     PIREP shouldn't treat that as a "landing lights on" event.
    //   * `L:S_OH_EXT_LT_NOSE`: 0 = off, 1 = taxi, 2 = T.O. The
    //     standard `LIGHT TAXI` SimVar is binary; Fenix's switch is
    //     tri-state, with T.O. being the brighter (full-power) mode
    //     used during takeoff/landing. Either mode counts as "taxi
    //     light on" for the binary snapshot pill.
    //   * `L:S_OH_EXT_LT_WING`: 0/1. Standard MSFS doesn't expose a
    //     wing-inspection light SimVar; we surface it only when Fenix
    //     beta is on (otherwise stays `None`).
    let fenix_beta_light_landing = if fenix_beta {
        Some(t.fnx_ext_lt_landing_l as i32 == 2 || t.fnx_ext_lt_landing_r as i32 == 2)
    } else {
        None
    };
    let fenix_beta_light_taxi = if fenix_beta {
        Some(t.fnx_ext_lt_nose as i32 >= 1)
    } else {
        None
    };
    let fenix_beta_light_wing = if fenix_beta {
        Some(t.fnx_ext_lt_wing as i32 != 0)
    } else {
        None
    };

    // Parking brake: Fenix routes through L:S_MIP_PARKING_BRAKE
    // (the MIP switch state) which is more reliable than the
    // standard SimVar on that aircraft.
    let parking_brake = if is_fenix {
        t.fnx_park_brake as i32 != 0
    } else {
        t.parking_brake
    };

    // System switch overrides for Fenix (LVar names verified against
    // the Axis-and-Ohs script bundle). Each one falls back to the
    // standard SimVar if the LVar reads exactly 0 — that way the
    // override only takes over when Fenix is actually feeding values.
    let apu_switch = if is_fenix {
        t.fnx_apu_master as i32 != 0
    } else {
        t.apu_switch
    };
    let pitot_heat = if is_fenix {
        // L:S_OH_PROBE_HEAT: 0=AUTO, 1=ON. AUTO means heating is
        // automatically active when engines are running, so we
        // treat both states as "heat available".
        t.fnx_probe_heat >= 0.0 // always considered "active" on Airbus
    } else {
        t.pitot_heat
    };
    let battery_master = if is_fenix {
        // BAT 1 OR BAT 2 in AUTO/ON position counts as "battery on".
        // 0=OFF, 1=AUTO on real Airbus.
        t.fnx_bat1 as i32 != 0 || t.fnx_bat2 as i32 != 0
    } else {
        t.battery_master
    };
    let engine_anti_ice = if is_fenix {
        t.fnx_eng1_anti_ice as i32 != 0 || t.fnx_eng2_anti_ice as i32 != 0
    } else {
        t.eng1_anti_ice || t.eng2_anti_ice || t.eng3_anti_ice || t.eng4_anti_ice
    };
    let wing_anti_ice = if is_fenix {
        t.fnx_wing_anti_ice as i32 != 0
    } else {
        t.structural_deice
    };

    // Cabin signs (Fenix only — no standard SimVar covers these).
    //
    // The AAO script reveals the value spaces are different between
    // the two signs:
    //   * `L:S_OH_SIGNS` (seat belts) is BINARY — its toggle uses
    //     the logical-NOT operator `! (>L:S_OH_SIGNS)`, which only
    //     makes sense for a 0/1 LVar. We clamp accordingly.
    //   * `L:S_OH_SIGNS_SMOKING` (no smoking) is 3-state — the toggle
    //     branches `0 == if{ 2 } els{ 0 }` and other scripts touch
    //     value 1, confirming OFF/AUTO/ON semantics.
    //
    // Keep both as `Option<u8>`; the activity-log helper picks the
    // right label set per field below.
    let seatbelts_sign = if is_fenix {
        Some(t.fnx_signs_seatbelts.round().clamp(0.0, 1.0) as u8)
    } else {
        None
    };
    let no_smoking_sign = if is_fenix {
        Some(t.fnx_signs_smoking.round().clamp(0.0, 2.0) as u8)
    } else {
        None
    };

    // FCU selected values — currently only Fenix exposes them via
    // dedicated LVars. Default-aircraft AP target SimVars (e.g.
    // AUTOPILOT ALTITUDE LOCK VAR) exist but aren't subscribed yet,
    // so for now FCU values stay None outside Fenix.
    let (fcu_alt, fcu_hdg, fcu_spd, fcu_vs) = if is_fenix {
        (
            Some(t.fnx_fcu_alt.round() as i32),
            Some(t.fnx_fcu_hdg.round() as i32),
            Some(t.fnx_fcu_spd.round() as i32),
            Some(t.fnx_fcu_vs.round() as i32),
        )
    } else {
        (None, None, None, None)
    };

    // Autobrake setting — derived from the three indicator-lamp
    // LVars (LO/MED/MAX). Only one of them is illuminated at a
    // time. Fenix exposes these as `L:I_MIP_AUTOBRAKE_*_L`.
    let autobrake = if is_fenix {
        if t.fnx_autobrake_max as i32 != 0 {
            Some("MAX".to_string())
        } else if t.fnx_autobrake_med as i32 != 0 {
            Some("MED".to_string())
        } else if t.fnx_autobrake_lo as i32 != 0 {
            Some("LO".to_string())
        } else {
            Some("OFF".to_string())
        }
    } else {
        None
    };

    // Pushback state — value 3 means MSFS reports the tug has
    // disconnected (or there was never a tug). Anything else is
    // an active push. Stored as Option<u8> so consumers can tell
    // "not wired" from "no pushback (=3)".
    let pushback_state = {
        let raw = t.pushback_state.round() as i32;
        if (0..=3).contains(&raw) {
            Some(raw as u8)
        } else {
            None
        }
    };

    SimSnapshot {
        timestamp: Utc::now(),
        lat: t.lat,
        lon: t.lon,
        altitude_msl_ft: t.altitude_msl_ft,
        altitude_agl_ft: t.altitude_agl_ft,
        // v0.7.17 (B-003): Indicated + Pressure altitude side-by-side
        // mit `altitude_msl_ft` (geometric MSL). Wenn ein SimVar
        // nicht gesetzt wurde (0.0), liefern wir `None` damit
        // Downstream zwischen "nicht gemessen" und "0 ft" unterscheiden
        // kann.
        altitude_indicated_ft: if t.altitude_indicated_ft.abs() > f64::EPSILON {
            Some(t.altitude_indicated_ft)
        } else {
            None
        },
        altitude_pressure_ft: if t.altitude_pressure_ft.abs() > f64::EPSILON {
            Some(t.altitude_pressure_ft)
        } else {
            None
        },
        heading_deg_true: t.heading_true_deg as f32,
        heading_deg_magnetic: t.heading_magnetic_deg as f32,
        // v0.5.24: MSFS-SimConnect convention is INVERTED — `PLANE PITCH
        // DEGREES` reports positive values when the nose is BELOW horizon.
        // We negate here so downstream code (FSM phase transitions,
        // tail-strike check, sampler capture, PIREP custom fields,
        // analytics) sees the universal aviation convention: positive
        // pitch = nose UP, like X-Plane reports natively.
        // Without this, every MSFS PIREP had inverted pitch (e.g. an
        // A321 rotation showed as -11.2° instead of +11.2°), which made
        // tail-strike checks rely on abs() to mask the bug, but
        // confused pilots reading the raw value in their PIREP detail.
        pitch_deg: -(t.pitch_deg as f32),
        bank_deg: t.bank_deg as f32,
        vertical_speed_fpm: t.vertical_speed_fpm as f32,
        // MSFS `VERTICAL SPEED` is true earth-frame + responsive — no separate
        // raw signal needed; the touchdown path falls back to vertical_speed_fpm.
        vertical_speed_raw_fpm: None,
        velocity_body_x_fps: Some(t.velocity_body_x_fps as f32),
        velocity_body_z_fps: Some(t.velocity_body_z_fps as f32),
        groundspeed_kt: t.groundspeed_kt as f32,
        // Clamp small negative readings to zero — MSFS pitot simulation
        // (especially with study-level addons) sometimes reports a few
        // negative knots while parked. Mirrors the X-Plane adapter's
        // identical clamp; pilots reasonably treat "−10 kt" as a bug.
        indicated_airspeed_kt: (t.indicated_airspeed_kt as f32).max(0.0),
        true_airspeed_kt: (t.true_airspeed_kt as f32).max(0.0),
        aircraft_wind_x_kt: Some(t.aircraft_wind_x_kt as f32),
        aircraft_wind_z_kt: Some(t.aircraft_wind_z_kt as f32),
        g_force: t.g_force as f32,
        on_ground: t.on_ground,
        // v0.7.19: crashed/crash_source kommen NICHT aus dem Telemetry-
        // Tick sondern aus dem SimConnect-System-Event `Crashed`. Der
        // Adapter latcht das in seinem Shared-State und der Caller
        // (build_snapshot in adapter.rs) mergt den Wert ein. Telemetry-
        // Default ist false/None — wird ggf. ueberschrieben.
        crashed: false,
        crash_source: None,
        // MSFS-Adapter liefert keinen Gear-Normal-Force-Wert; das
        // X-Plane-Pendant (sampler-side touchdown edge) ist hier
        // nicht aktiv — MSFS hat eh den separaten
        // PLANE TOUCHDOWN NORMAL VELOCITY-SimVar als Primary-Quelle.
        gear_normal_force_n: None,
        parking_brake,
        stall_warning: t.stall_warning,
        overspeed_warning: t.overspeed_warning,
        paused: false,
        slew_mode: false,
        simulation_rate: 1.0,
        gear_position: t.gear_position as f32,
        flaps_position: t.flaps_position as f32,
        engines_running,
        fuel_total_kg,
        fuel_used_kg: 0.0,
        zfw_kg,
        payload_kg,
        total_weight_kg,
        // Touchdown sample: not yet wired in raw mode; stays None
        // until we add a second data definition for them. The legacy
        // adapter also kept these None.
        touchdown_vs_fpm: None,
        touchdown_pitch_deg: None,
        touchdown_bank_deg: None,
        touchdown_heading_mag_deg: None,
        touchdown_lat: None,
        touchdown_lon: None,
        wind_direction_deg: Some(t.wind_direction_deg as f32),
        wind_speed_kt: Some(t.wind_speed_kt as f32),
        qnh_hpa: Some(t.qnh_hpa as f32),
        outside_air_temp_c: Some(t.oat_c as f32),
        total_air_temp_c: Some(t.tat_c as f32),
        mach: Some(t.mach as f32),
        empty_weight_kg,
        aircraft_title: Some(t.title).filter(|s| !s.is_empty()),
        // v0.7.17 (B-001): Bei Profilen wie Fenix kommt `ATC MODEL`
        // oft leer aus dem Sim — Pilot sah dann „Type ?" im Activity-
        // Log. Fallback auf einen Profile-eigenen kanonischen ICAO
        // (`AircraftProfile::icao_fallback()`), wenn der SimVar nichts
        // liefert. Profile ohne eindeutige Variante (FBW, Default)
        // behalten None und bleiben „?".
        //
        // v0.12.10: `clean_atc_model` statt des rohen `t.atc_model` —
        // sonst landet ein Token wie `ATCCOM.AC_MODEL C208.0.text`
        // (BlackSquare Caravan) ungereinigt in `aircraft_icao`. Liefert
        // `clean_atc_model` `None` (leer / nicht decodierbar), greift
        // weiter der Profile-Fallback.
        aircraft_icao: clean_atc_model(&t.atc_model)
            .or_else(|| profile.icao_fallback().map(str::to_string)),
        aircraft_registration: Some(t.atc_id).filter(|s| !s.is_empty()),
        simulator,
        sim_version: None,
        // Avionics: standard SimVars. Under the legacy Rust crate we
        // had to force None for Fenix because the memory layout shifted
        // and we'd read QNH-bleed garbage (e.g. "COM1 1024 MHz"). Raw
        // FFI parses each field at a fixed offset so the noise is gone
        // — emit whatever the SimVar reports. The activity-log change
        // detector skips entries that don't actually change, so an
        // aircraft that genuinely doesn't wire these just leaves them
        // at their default (0 → no log entries) without spamming.
        transponder_code,
        com1_mhz: positive_or_none(t.com1_mhz as f32),
        com2_mhz: positive_or_none(t.com2_mhz as f32),
        nav1_mhz: positive_or_none(t.nav1_mhz as f32),
        nav2_mhz: positive_or_none(t.nav2_mhz as f32),
        // v0.7.16: Fenix beta overrides landing/taxi via verified
        // overhead LVARs. Stable behavior (beta off) is `Some(t.light_*)`.
        light_landing: fenix_beta_light_landing.or(Some(t.light_landing)),
        light_beacon: Some(light_beacon),
        light_strobe: Some(light_strobe),
        light_taxi: fenix_beta_light_taxi.or(Some(t.light_taxi)),
        light_nav: Some(light_nav),
        light_logo: Some(light_logo),
        strobe_state,
        autopilot_master: Some(ap_master),
        autopilot_heading: Some(ap_hdg),
        autopilot_altitude: Some(ap_alt),
        autopilot_nav: Some(ap_nav),
        autopilot_approach: Some(ap_appr),
        fuel_flow_kg_per_h,
        spoilers_handle_position: Some(t.spoilers_handle_position as f32),
        spoilers_armed: Some(t.spoilers_armed),
        pushback_state,
        apu_switch: Some(apu_switch),
        apu_pct_rpm: Some(t.apu_pct_rpm as f32),
        battery_master: Some(battery_master),
        avionics_master: Some(t.avionics_master),
        pitot_heat: Some(pitot_heat),
        engine_anti_ice: Some(engine_anti_ice),
        wing_anti_ice: Some(wing_anti_ice),
        // v0.3.0: filled by the PMDG snapshot()-merge layer when a
        // PMDG aircraft is loaded. v0.7.16 also surfaces Fenix's
        // `L:S_OH_EXT_LT_WING` (no standard SimVar covers it). Stays
        // `None` for non-PMDG / non-Fenix-beta aircraft.
        light_wing: fenix_beta_light_wing,
        light_wheel_well: None,
        xpdr_mode_label: None,
        takeoff_config_warning: None,
        seatbelts_sign,
        no_smoking_sign,
        fcu_selected_altitude_ft: fcu_alt,
        fcu_selected_heading_deg: fcu_hdg,
        fcu_selected_speed_kt: fcu_spd,
        fcu_selected_vs_fpm: fcu_vs,
        autobrake,
        parking_name: None,
        parking_number: None,
        selected_runway: None,
        aircraft_profile: profile,
        // PMDG SDK data is filled in MsfsAdapter::snapshot() by
        // merging the latest ClientData block — not here in the
        // standard SimVar parse path.
        pmdg: None,
        // Category-aware landing: the live MSFS gear-type SimVars
        // (IS GEAR SKIDS/FLOATS/WHEELS, CONTACT POINT IS ON GROUND,
        // GEAR WATER DEPTH, WATER RUDDER HANDLE POSITION) are intentionally
        // NOT wired here. Adding SimVars is a 4-point lockstep change to the
        // fixed-offset SimConnect data definition (F:: list ↔ from_block ↔
        // Telemetry struct ↔ here) that cannot be verified without a running
        // sim, on the core telemetry path every MSFS pilot depends on. For
        // now the aircraft CATEGORY is derived from the ICAO type (covers all
        // current GSG rotorcraft + flying-boat seaplanes) and water/vertical
        // touchdowns are detected via the sim-agnostic AGL + descent-arrest
        // heuristic. Wiring these SimVars — for MSFS dual-use floatplane
        // (C208/DHC2 on floats) auto-detection — is a tracked future
        // robustness enhancement that needs in-sim verification.
        gear_is_skid: None,
        gear_is_floats: None,
        gear_is_wheels: None,
        contact_point_on_ground: None,
        gear_water_depth_m: None,
        water_rudder_present: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal Fenix-profile Telemetry with the standard
    /// SimVars showing "all lights off" and a specific set of Fenix
    /// extension LVAR values. Used by the beta-mapping tests below.
    fn fenix_telemetry(
        landing_l: f64,
        landing_r: f64,
        nose: f64,
        wing: f64,
    ) -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM SL".into();
        t.atc_model = "A320".into();
        // Standard SimVars stay false so any positive in the snapshot
        // is unambiguously sourced from the Fenix LVARs.
        t.light_landing = false;
        t.light_taxi = false;
        t.fnx_ext_lt_landing_l = landing_l;
        t.fnx_ext_lt_landing_r = landing_r;
        t.fnx_ext_lt_nose = nose;
        t.fnx_ext_lt_wing = wing;
        t
    }

    #[test]
    fn fenix_maps_landing_from_lvar_l_or_r_equals_2() {
        // v0.7.17 (F-001): Fenix-Profil immer default-on. Either
        // side at position 2 ("on") counts as landing-on. The
        // 0/1 positions (retracted/off) both stay off.
        let snap_l_only = telemetry_to_snapshot(
            fenix_telemetry(2.0, 0.0, 0.0, 0.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_l_only.light_landing, Some(true));

        let snap_r_only = telemetry_to_snapshot(
            fenix_telemetry(0.0, 2.0, 0.0, 0.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_r_only.light_landing, Some(true));

        let snap_off_with_off_position = telemetry_to_snapshot(
            fenix_telemetry(1.0, 1.0, 0.0, 0.0),
            Simulator::Msfs2024,
        );
        // Position 1 = "off" (not retracted), still no landing light.
        assert_eq!(snap_off_with_off_position.light_landing, Some(false));

        let snap_retracted = telemetry_to_snapshot(
            fenix_telemetry(0.0, 0.0, 0.0, 0.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_retracted.light_landing, Some(false));
    }

    #[test]
    fn fenix_maps_taxi_from_nose_lvar() {
        // 0 = off, 1 = taxi, 2 = T.O. — both 1 and 2 count as on
        // for the binary taxi-light snapshot pill.
        let snap_off = telemetry_to_snapshot(
            fenix_telemetry(0.0, 0.0, 0.0, 0.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_off.light_taxi, Some(false));

        let snap_taxi = telemetry_to_snapshot(
            fenix_telemetry(0.0, 0.0, 1.0, 0.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_taxi.light_taxi, Some(true));

        let snap_takeoff = telemetry_to_snapshot(
            fenix_telemetry(0.0, 0.0, 2.0, 0.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_takeoff.light_taxi, Some(true));
    }

    #[test]
    fn fenix_maps_wing_light_from_lvar() {
        let snap_off = telemetry_to_snapshot(
            fenix_telemetry(0.0, 0.0, 0.0, 0.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_off.light_wing, Some(false));

        let snap_on = telemetry_to_snapshot(
            fenix_telemetry(0.0, 0.0, 0.0, 1.0),
            Simulator::Msfs2024,
        );
        assert_eq!(snap_on.light_wing, Some(true));
    }

    #[test]
    fn b008_fenix_ap_master_uses_standard_simvar_not_lvar_pulse() {
        // v0.7.17 (B-008): `L:S_FCU_AP1` und `L:S_FCU_AP2` sind
        // Button-Press-Pulse — 0→1→0 bei jedem Klick. Sie sind die
        // meiste Zeit 0 obwohl der A320-AP aktiv ist (Tester-Befund
        // Thomas K CFG 2222 FL313 Cruise: alle AP-Status zeigten OFF).
        // Wir muessen den Standard `AUTOPILOT MASTER` SimVar lesen.

        // Case 1: Pulse ist 0 (= 99% der Zeit), Standard-SimVar sagt AP aktiv
        //   → Snapshot MUSS ap_master=true zeigen.
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM WF".into();
        t.atc_model = "A320".into();
        t.fnx_fcu_ap1 = 0.0; // Pulse zurueck auf 0
        t.fnx_fcu_ap2 = 0.0;
        t.ap_master = true; // Standard-SimVar = engaged
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert!(
            snap.autopilot_master.unwrap_or(false),
            "B-008 regression: Fenix-AP-Master MUSS auf t.ap_master mappen, nicht auf den Pulse-LVAR"
        );

        // Case 2: Pulse spiked auf 1 (= seltener Klick-Moment), Standard sagt AP off.
        //   → snapshot zeigt AP off (= echte Wahrheit), wir lassen uns vom Pulse
        //     nicht ueberreden dass AP aktiv ist.
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM WF".into();
        t.atc_model = "A320".into();
        t.fnx_fcu_ap1 = 1.0; // Pulse spike — wir ignorieren ihn
        t.fnx_fcu_ap2 = 0.0;
        t.ap_master = false;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert!(
            !snap.autopilot_master.unwrap_or(true),
            "B-008: Pulse-Spike darf nicht als AP-engaged interpretiert werden"
        );
    }

    #[test]
    fn fenix_mapping_does_not_affect_non_fenix_aircraft() {
        // v0.7.17: With Fenix mapping always-on the `is_fenix()`
        // gate is the only thing keeping a non-Fenix aircraft on
        // the standard SimVar path. Verify that gate.
        let mut t = Telemetry::default();
        t.title = "Asobo A320 Neo".into();
        t.atc_model = "A20N".into();
        t.light_landing = false;
        t.light_taxi = false;
        // Pretend the Fenix LVAR slots happened to have values:
        t.fnx_ext_lt_landing_l = 2.0;
        t.fnx_ext_lt_nose = 1.0;
        t.fnx_ext_lt_wing = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.light_landing, Some(false));
        assert_eq!(snap.light_taxi, Some(false));
        assert_eq!(snap.light_wing, None);
    }

    #[test]
    fn b001_aircraft_icao_falls_back_for_fenix_with_empty_atc_model() {
        // v0.7.17 (B-001): Wenn `ATC MODEL` leer (typisch bei Fenix
        // wo der SimVar nicht zuverlaessig gefuellt wird), muss
        // aircraft_icao auf den Profile-Fallback fallen — sonst sieht
        // der Pilot „Type ?" im Activity-Log.
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM SL".into();
        t.atc_model = "".into(); // SimVar leer
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.aircraft_icao, Some("A320".to_string()));

        let mut t = Telemetry::default();
        t.title = "FenixA319 IAE".into();
        t.atc_model = "".into();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.aircraft_icao, Some("A319".to_string()));

        let mut t = Telemetry::default();
        t.title = "FenixA321 NEO LR".into();
        t.atc_model = "".into();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.aircraft_icao, Some("A321".to_string()));
    }

    #[test]
    fn b001_aircraft_icao_prefers_sim_value_over_fallback() {
        // Wenn der SimVar einen Wert liefert, hat der Vorrang — der
        // Fallback ist nur ein Backup fuer leere SimVars.
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM SL".into();
        t.atc_model = "A20N".into(); // SimVar liefert was (selten, aber falls)
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.aircraft_icao, Some("A20N".to_string()));
    }

    #[test]
    fn b002_fenix_transponder_code_suppressed_regardless_of_simvar() {
        // v0.7.17 (B-002): Bei Fenix wird der Squawk im Snapshot
        // immer None gemeldet — der Standard-SimVar liefert dort
        // falsche/eingefrorene Werte. Auch wenn der Sim einen Wert
        // zurueck gibt, blendet AeroACARS ihn aus.
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM SL".into();
        t.atc_model = "A320".into();
        t.transponder_bcd = 0x2532 as f64; // Sim sagt 2532, wir glauben's bei Fenix nicht
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.transponder_code, None);
    }

    #[test]
    fn b002_non_fenix_transponder_still_decoded() {
        // BCD-Decoding fuer Nicht-Fenix-Profile bleibt unveraendert.
        let mut t = Telemetry::default();
        t.title = "Asobo A320 Neo".into();
        t.atc_model = "A20N".into();
        t.transponder_bcd = 0x2532 as f64;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.transponder_code, Some(2532));
    }

    #[test]
    fn b001_non_fenix_with_empty_icao_stays_none() {
        // Profile ohne icao_fallback (Default / FBW / PMDG / INI)
        // behalten None bei leerem SimVar — kein Phantasie-ICAO.
        let mut t = Telemetry::default();
        t.title = "Asobo Cessna 172".into();
        t.atc_model = "".into();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.aircraft_icao, None);
    }

    #[test]
    fn telemetry_fields_layout_matches_struct_pulls() {
        // Smoke test: the parser walks the buffer in TELEMETRY_FIELDS
        // order, so the total declared size must match the number of
        // pull_*! calls in from_block(). If someone adds a field to
        // the list without a matching pull, this test catches the
        // drift via a too-large or too-small parse.
        let total: usize = TELEMETRY_FIELDS.iter().map(|f| f.kind.size()).sum();
        // Build a buffer of exactly that size — all zeros — and
        // confirm from_block doesn't panic and returns defaults.
        let buf = vec![0u8; total];
        let t = Telemetry::from_block(&buf);
        // Sanity: a known field at the end (autobrake_max) reads 0.
        assert_eq!(t.fnx_autobrake_max, 0.0);
        // And the new beta extension fields too.
        assert_eq!(t.fnx_ext_lt_wing, 0.0);
        assert_eq!(t.fnx_fc_flaps_lever, 0.0);
    }

    // ---- v0.13.17: N1-Fallback fuer engines_running ----
    // Live-Befund IRM1140/IBE778 (iniBuilds/Aerosoft A340-600, MSFS 2024):
    // GENERAL ENG COMBUSTION konstant 0 trotz laufender Triebwerke → Phase
    // blieb in Pushback haengen. N1 (Standard-SimVar) bleibt gueltig.

    #[test]
    fn n1_fallback_counts_running_when_combustion_zero() {
        // COMBUSTION alle false (Addon-Bug), N1 ~0.66 (0-1-Skala, wie im
        // Inspektor gemessen) → alle 4 als laufend erkannt.
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346-MahanAir".into();
        t.atc_model = "A346".into();
        t.n1_pct_1 = 0.6648;
        t.n1_pct_2 = 0.6643;
        t.n1_pct_3 = 0.6645;
        t.n1_pct_4 = 0.6649;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 4);
    }

    #[test]
    fn n1_fallback_zero_when_all_off() {
        // Alles aus (COMBUSTION 0 + N1 0) → 0. Kein False-Positive.
        let t = Telemetry::default();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 0);
    }

    #[test]
    fn combustion_wins_when_it_works_no_regression() {
        // COMBUSTION funktioniert (2 Engines an) → Ergebnis 2, der
        // N1-Fallback wird NICHT genutzt (kein Doppelzaehlen/Regress),
        // auch wenn N1 fuer alle 4 hoch ist.
        let mut t = Telemetry::default();
        t.eng1_firing = true;
        t.eng2_firing = true;
        t.n1_pct_1 = 0.9;
        t.n1_pct_2 = 0.9;
        t.n1_pct_3 = 0.9;
        t.n1_pct_4 = 0.9;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 2);
    }

    #[test]
    fn n1_fallback_normalizes_percent_scale_and_rejects_windmill() {
        // Anderes Addon liefert N1 auf 0-100-Skala: 72.9 % laufend,
        // 10 % Windmill → nur das laufende zaehlt.
        let mut t = Telemetry::default();
        t.n1_pct_1 = 72.9;
        t.n1_pct_2 = 10.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 1);
    }
}
