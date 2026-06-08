//! DataRef catalog — the SimVar-equivalent SimSnapshot fields.
//!
//! Every entry maps an X-Plane DataRef name to a slot in the parsed
//! `XPlaneState` (which then converts to `SimSnapshot`). The order
//! determines our subscription index; we send one RREF request per
//! row and store the (index → field) mapping.
//!
//! All DataRef names are confirmed to exist in BOTH X-Plane 11 and
//! 12, sourced from the official DataRef list at
//! <https://developer.x-plane.com/datarefs/>. When 11 and 12 diverged
//! on a given name, we use the canonical XP12 name and accept that
//! XP11 may report a default value (so the snapshot field stays at
//! its default rather than crashing).
//!
//! ## Units
//!
//! X-Plane DataRefs document their units explicitly. We pull values
//! in their native unit and convert to `SimSnapshot` units in
//! `XPlaneState::to_snapshot`. Notable conversions:
//!
//!   * `local_vx/vz`     m/s → ft/s for body velocity
//!   * `acf_m_*`         kg → kg (no-op, but the field is float)
//!   * `latitude/longitude` deg → deg (no-op, but x-plane uses
//!     `flightmodel/position/{latitude,longitude}` not `lat_/lon_`)

use sim_core::{SimSnapshot, Simulator};

/// Field on the parsed `XPlaneState` to assign each DataRef value to.
/// Each entry corresponds to one RREF subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    Latitude,
    Longitude,
    AltitudeMslFt,
    AltitudeAglFt,
    HeadingDegTrue,
    HeadingDegMagnetic,
    PitchDeg,
    BankDeg,
    VerticalSpeedFpm,
    GroundspeedKt,
    IndicatedAirspeedKt,
    TrueAirspeedKt,
    GForce,
    OnGround,
    /// v0.4.4: Normal force on the gear (N). Used for sampler-side
    /// touchdown edge detection — fires far earlier and more reliably
    /// than `OnGround` which is a binary flight-model flag.
    GearNormalForceN,
    ParkingBrake,
    GearDeploy,
    FlapsHandle,
    Eng1Running,
    Eng2Running,
    Eng3Running,
    Eng4Running,
    FuelTotalKg,
    EmptyWeightKg,
    TotalWeightKg,
    /// Body-frame X velocity (m/s), positive right. Used for sideslip.
    LocalVxMs,
    /// Body-frame Z velocity (m/s), positive forward. Used for sideslip.
    LocalVzMs,
    /// Wind X relative to airframe (m/s).
    WindXMs,
    /// Wind Z relative to airframe (m/s).
    WindZMs,
    /// v0.5.19: meteorological wind speed at aircraft altitude (m/s).
    /// Source for SimSnapshot::wind_speed_kt — was hardcoded None
    /// in X-Plane builds before, MQTT live-tracking server reported
    /// the wind field as missing for X-Plane pilots.
    WindNowSpeedMs,
    /// v0.5.19: meteorological wind direction at aircraft altitude
    /// (degrees true). Source for SimSnapshot::wind_direction_deg.
    WindNowDirectionDegT,
    // Phase 2 additions:
    LightLanding,
    LightBeacon,
    LightStrobe,
    LightTaxi,
    LightNav,
    ApMaster,
    ApHeading,
    ApAltitude,
    ApNav,
    ApApproach,
    SpoilersHandle,
    SpoilersArmed,
    StallWarning,
    BatteryMaster,
    AvionicsMaster,
    ApuSwitch,
    PitotHeat,
    QnhInHg,
    OatC,
    Mach,
    // v0.3.0 additions (universal X-Plane standard DataRefs):
    /// Autobrake selector position: 0=RTO, 1=OFF, 2=1, 3=2, 4=3, 5=MAX.
    AutobrakeLevel,
    /// Transponder mode: 0=OFF, 1=STBY, 2=ON, 3=TEST, 4=ALT, 5=TA, 6=TARA.
    TransponderMode,
    // v0.3.0 additions (Boeing 737 family — Zibo/LevelUp/Default-B738):
    /// `laminar/B738/toggle_switch/wing_light_pos` — 1=ON, 0=OFF.
    LightWing,
    /// `laminar/B738/toggle_switch/wheel_well_light_pos` — 1=ON, 0=OFF.
    LightWheelWell,
    /// `laminar/B738/annunciator/takeoff_config` — 1=warning, 0=clear.
    TakeoffConfigWarning,
    /// Spec v0.7.15 F6: `sim/time/paused` — 1 wenn der User die
    /// Pause-Taste in X-Plane gedrueckt hat, sonst 0. Plus Replay
    /// (`sim/time/is_in_replay`) wird zur selben Pause-Logik
    /// aggregiert: aus AeroACARS-Sicht ist Replay "Flug-Telemetrie
    /// ist nicht echt, also als Pause behandeln".
    SimPaused,
    /// Spec v0.7.15 F6: `sim/time/is_in_replay` — 1 wenn der User in
    /// X-Planes Replay-Modus ist (eigene Pause-aehnliche Quelle).
    SimInReplay,
}

/// One row in the catalog: a DataRef name + which snapshot field it
/// fills. The index used over the wire equals the position in
/// `CATALOG`.
pub struct DatarefEntry {
    pub name: &'static str,
    pub field: FieldId,
}

/// The subscription catalog. Order is the wire index.
pub const CATALOG: &[DatarefEntry] = &[
    // --- Position ---
    DatarefEntry {
        name: "sim/flightmodel/position/latitude",
        field: FieldId::Latitude,
    },
    DatarefEntry {
        name: "sim/flightmodel/position/longitude",
        field: FieldId::Longitude,
    },
    DatarefEntry {
        // INDICATED altitude (the value the pilot reads off the
        // altimeter / flies AP-ALT to). Already in feet, NO conversion
        // needed at snapshot boundary.
        //
        // Why not `sim/flightmodel/position/elevation`?  That is TRUE
        // MSL (geographic height above sea level), which differs from
        // indicated altitude whenever the air mass is non-ISA (warmer
        // than ISA → indicated reads lower than true → live bug
        // 2026-05-05: pilot at FL390 mit OAT −46 °C sah AeroACARS
        // 40,009 ft melden während das PFD korrekt 39,000 ft zeigte;
        // Differenz 1.000 ft ist exakt die ISA-deviation × Faustformel
        // 4 ft/°C × ~10 °C über ISA bei FL390).
        //
        // `altitude_ft_pilot` matches the cockpit altimeter exactly
        // and converges with TRUE MSL on descent (where QNH ≈ STD ≈
        // ambient). FieldId still says `_ft` — semantically correct
        // now (used to lie about meters; see `value_setters`).
        name: "sim/cockpit2/gauges/indicators/altitude_ft_pilot",
        field: FieldId::AltitudeMslFt,
    },
    DatarefEntry {
        // X-Plane reports AGL in METERS via `y_agl`. We convert at
        // the snapshot boundary.
        name: "sim/flightmodel/position/y_agl",
        field: FieldId::AltitudeAglFt,
    },
    // --- Attitude / motion ---
    DatarefEntry {
        name: "sim/flightmodel/position/psi",
        field: FieldId::HeadingDegTrue,
    },
    DatarefEntry {
        name: "sim/flightmodel/position/magpsi",
        field: FieldId::HeadingDegMagnetic,
    },
    DatarefEntry {
        name: "sim/flightmodel/position/theta",
        field: FieldId::PitchDeg,
    },
    DatarefEntry {
        name: "sim/flightmodel/position/phi",
        field: FieldId::BankDeg,
    },
    DatarefEntry {
        // RAW vertical velocity (m/s, ohne VSI-Smoothing/Lag).
        //
        // Wir lasen früher `vh_ind_fpm` — das ist die Vertical-Speed-
        // Indicator-Anzeige wie im echten Cockpit, mit absichtlichem
        // Damping (mehrere Sekunden Lag). Real-life VSIs sind gefiltert
        // damit sie nicht zappeln.
        //
        // Live-Bug X-Plane Pilot-Test 2026-05-05 (EWL6822 LEPA→EDDG):
        // pilot landed mit ca. -350 fpm aber AeroACARS scorte
        // "smooth, peak_vs_fpm: +5.7" — der VSI-Wert hatte zum Touch-
        // down-Moment schon auf nahe 0 gemittelt, der echte Sinkflug
        // war im 500ms-Buffer-Window nicht mehr erkennbar.
        //
        // `local_vy` ist die rohe Z-Achsen-Geschwindigkeit in m/s,
        // realtime, ohne Smoothing. Wir konvertieren m/s → fpm im
        // value-setter (× 196.85 für ft/min). Vorzeichen ist umgekehrt
        // zur fpm-Konvention: `local_vy > 0` = aufsteigend (X-Planes
        // OpenGL-Y-Achse zeigt nach oben), während fpm > 0 = climb.
        // Die haben dasselbe Vorzeichen — `local_vy` braucht keine
        // Vorzeichen-Umkehrung.
        name: "sim/flightmodel/position/local_vy",
        field: FieldId::VerticalSpeedFpm,
    },
    // --- Speeds ---
    DatarefEntry {
        // groundspeed is in m/s native; we convert to knots.
        name: "sim/flightmodel/position/groundspeed",
        field: FieldId::GroundspeedKt,
    },
    DatarefEntry {
        name: "sim/flightmodel/position/indicated_airspeed",
        field: FieldId::IndicatedAirspeedKt,
    },
    DatarefEntry {
        name: "sim/flightmodel/position/true_airspeed",
        field: FieldId::TrueAirspeedKt,
    },
    // --- Forces ---
    DatarefEntry {
        name: "sim/flightmodel2/misc/gforce_normal",
        field: FieldId::GForce,
    },
    DatarefEntry {
        // bool 0/1
        name: "sim/flightmodel/failures/onground_any",
        field: FieldId::OnGround,
    },
    DatarefEntry {
        // Normal force on the gear (N). Nonzero exactly at the physical
        // moment of wheel-runway contact. xgs (etabliertes X-Plane-
        // Landing-Speed-Plugin, ~10 Jahre in production) nutzt das als
        // Touchdown-Edge-Trigger statt `onground_any` weil letzteres
        // im Flight-Model-Frame laggt und bouncen kann. Wir trigger
        // unsere Sampler-Side-Edge-Detection auf einen rising edge
        // hier: in der Luft = ~0 N, beim Touchdown spikt's auf
        // mehrere kN für die Aircraft-Masse × Touchdown-G.
        name: "sim/flightmodel/forces/fnrml_gear",
        field: FieldId::GearNormalForceN,
    },
    DatarefEntry {
        // 0..1 ratio
        name: "sim/cockpit2/controls/parking_brake_ratio",
        field: FieldId::ParkingBrake,
    },
    // --- Gear / flaps (just gear[0] = nose-gear deploy ratio 0..1).
    //     IMPORTANT: explicit `[0]` suffix required! X-Plane's RREF
    //     protocol returns unreliable values (often 0.0) for array
    //     DataRefs without a bracket index — same issue we hit on
    //     ENGN_running below. Live-bug 2026-05-04: pilot saw "Gear
    //     UP" while parked at AMS in a LevelUp 737, even though all
    //     three legs were on the ground. Adding `[0]` returns the
    //     nose-gear ratio, which is a reliable on-ground proxy. ---
    DatarefEntry {
        name: "sim/flightmodel2/gear/deploy_ratio[0]",
        field: FieldId::GearDeploy,
    },
    DatarefEntry {
        name: "sim/flightmodel2/controls/flap_handle_deploy_ratio",
        field: FieldId::FlapsHandle,
    },
    // --- Engines: explicit array index per engine. The unbracketed
    //     `ENGN_running` was unreliable (returned 0 even when engine 1
    //     was running on a 2-engine heavy — verified via live test).
    //     Bracket-suffix syntax IS supported by RREF: X-Plane parses
    //     `[N]` and returns just that array slot.
    DatarefEntry {
        name: "sim/flightmodel/engine/ENGN_running[0]",
        field: FieldId::Eng1Running,
    },
    // --- Weight & fuel (kg native).
    //     `acf_m_fuel_total` is the MAX TANK CAPACITY, not the
    //     current onboard fuel weight (verified via live test: full
    //     tank reported as 0 kg). Use the live `flightmodel/weight`
    //     DataRef instead. ---
    DatarefEntry {
        name: "sim/flightmodel/weight/m_fuel_total",
        field: FieldId::FuelTotalKg,
    },
    DatarefEntry {
        name: "sim/aircraft/weight/acf_m_empty",
        field: FieldId::EmptyWeightKg,
    },
    DatarefEntry {
        name: "sim/flightmodel/weight/m_total",
        field: FieldId::TotalWeightKg,
    },
    // --- Body velocity (m/s) — for native sideslip ---
    DatarefEntry {
        name: "sim/flightmodel/forces/local_vx",
        field: FieldId::LocalVxMs,
    },
    DatarefEntry {
        name: "sim/flightmodel/forces/local_vz",
        field: FieldId::LocalVzMs,
    },
    // ---- Phase 2 DataRefs ----
    // Multi-engine: array indices via `[N]` syntax (X-Plane parses
    // the bracket and returns just that element). Engine #1 is [0].
    DatarefEntry {
        name: "sim/flightmodel/engine/ENGN_running[1]",
        field: FieldId::Eng2Running,
    },
    DatarefEntry {
        name: "sim/flightmodel/engine/ENGN_running[2]",
        field: FieldId::Eng3Running,
    },
    DatarefEntry {
        name: "sim/flightmodel/engine/ENGN_running[3]",
        field: FieldId::Eng4Running,
    },
    // Lights — bool 0/1.
    DatarefEntry {
        name: "sim/cockpit2/switches/landing_lights_on",
        field: FieldId::LightLanding,
    },
    DatarefEntry {
        name: "sim/cockpit2/switches/beacon_on",
        field: FieldId::LightBeacon,
    },
    DatarefEntry {
        name: "sim/cockpit2/switches/strobe_lights_on",
        field: FieldId::LightStrobe,
    },
    DatarefEntry {
        name: "sim/cockpit2/switches/taxi_light_on",
        field: FieldId::LightTaxi,
    },
    DatarefEntry {
        name: "sim/cockpit2/switches/navigation_lights_on",
        field: FieldId::LightNav,
    },
    // Logo light: X-Plane uses the same nav-light dataref by
    // convention; some payware breaks this out separately. We
    // alias to nav for now (Phase 3 if a payware author asks).
    // Autopilot — XP exposes per-mode "engaged" status as int 0..2
    // (off / armed / engaged). We treat >0 as "on".
    DatarefEntry {
        name: "sim/cockpit2/autopilot/servos_on",
        field: FieldId::ApMaster,
    },
    DatarefEntry {
        name: "sim/cockpit2/autopilot/heading_status",
        field: FieldId::ApHeading,
    },
    DatarefEntry {
        name: "sim/cockpit2/autopilot/altitude_hold_status",
        field: FieldId::ApAltitude,
    },
    DatarefEntry {
        name: "sim/cockpit2/autopilot/nav_status",
        field: FieldId::ApNav,
    },
    DatarefEntry {
        name: "sim/cockpit2/autopilot/approach_status",
        field: FieldId::ApApproach,
    },
    // Surfaces — speedbrake is a 0..1 ratio.
    DatarefEntry {
        name: "sim/cockpit2/controls/speedbrake_ratio",
        field: FieldId::SpoilersHandle,
    },
    DatarefEntry {
        name: "sim/cockpit2/annunciators/speedbrake",
        field: FieldId::SpoilersArmed,
    },
    // Wind components in airframe-relative coords (m/s). Used for
    // headwind/crosswind reporting in the PIREP. Same DataRefs in
    // X-Plane 11 and 12.
    DatarefEntry {
        name: "sim/weather/aircraft/wind_now_x_msc",
        field: FieldId::WindXMs,
    },
    DatarefEntry {
        name: "sim/weather/aircraft/wind_now_z_msc",
        field: FieldId::WindZMs,
    },
    // v0.5.19: meteorological wind (absolute, not airframe-relative).
    // Same DataRef family in XP11 + XP12. Live-tracking server uses
    // these for the wind column in the monitor; was hardcoded None
    // before so X-Plane pilots showed "—" for wind.
    DatarefEntry {
        name: "sim/weather/aircraft/wind_now_speed_msc",
        field: FieldId::WindNowSpeedMs,
    },
    DatarefEntry {
        name: "sim/weather/aircraft/wind_now_direction_degt",
        field: FieldId::WindNowDirectionDegT,
    },
    // Stall warning — annunciator (bool).
    DatarefEntry {
        name: "sim/cockpit2/annunciators/stall_warning",
        field: FieldId::StallWarning,
    },
    // Systems — battery / avionics / APU / pitot heat.
    DatarefEntry {
        name: "sim/cockpit2/electrical/battery_on[0]",
        field: FieldId::BatteryMaster,
    },
    DatarefEntry {
        name: "sim/cockpit2/electrical/avionics_on",
        field: FieldId::AvionicsMaster,
    },
    DatarefEntry {
        name: "sim/cockpit2/electrical/APU_running",
        field: FieldId::ApuSwitch,
    },
    DatarefEntry {
        name: "sim/cockpit2/ice/ice_pitot_heat_on_pilot",
        field: FieldId::PitotHeat,
    },
    // QNH (hPa) and ambient temp — rewired several times after live
    // bug reports:
    //
    // 2026-05-03: switched OAT from `temperatures_aloft_deg_c[0]`
    // (= SURFACE temp, not aircraft altitude — pilot at FL180 sah
    // "+22°C" während Cockpit-PFD korrekt SAT −18°C zeigte) to
    // `cockpit2/temperature/outside_air_temp_degc` (aircraft-level
    // ambient = SAT in modern X-Plane).
    //
    // 2026-05-05: switched QNH from `barometer_current_inhg` to
    // `altimeter_setting_in_hg_pilot`. The former is the AMBIENT
    // air pressure at aircraft altitude (~5.85 inHg / 198 hPa at
    // FL390), NOT what the pilot dials into the Kollsman window.
    // Live bug: pilot at FL390 saw "QNH 198 hPa" — physically
    // impossible at sea level, but exactly the static pressure at
    // cruise altitude. The new DataRef is the actual altimeter-
    // setting (1013.25 hPa with STD selected, ~29.92 inHg).
    DatarefEntry {
        // BAROMETER, nicht altimeter — verwirrend benannt in X-Plane.
        // Die "altimeter setting" heißt im DataRef-Namespace
        // `barometer_setting_in_hg_*`. Quellen: FlyWithLua-Skripte,
        // X-RAAS-Plugin, developer.x-plane.com referenzieren
        // konsistent `barometer_setting_in_hg_alt_preselector` /
        // `_pilot` / `_copilot`. Verifiziert 2026-05-05 vor Release —
        // ein Tippfehler hier wäre stillschweigend tot (kein RREF
        // mehr, QNH bleibt bei 0).
        name: "sim/cockpit2/gauges/actuators/barometer_setting_in_hg_pilot",
        field: FieldId::QnhInHg,
    },
    DatarefEntry {
        name: "sim/cockpit2/temperature/outside_air_temp_degc",
        field: FieldId::OatC,
    },
    // Mach number.
    DatarefEntry {
        name: "sim/flightmodel/misc/machno",
        field: FieldId::Mach,
    },
    // ---- v0.3.0 additions (universal X-Plane standard) ----
    // Autobrake position: 0=RTO, 1=OFF, 2=1, 3=2, 4=3, 5=MAX.
    DatarefEntry {
        name: "sim/cockpit2/switches/auto_brake_level",
        field: FieldId::AutobrakeLevel,
    },
    // Transponder mode: 0=OFF, 1=STBY, 2=ON, 3=TEST, 4=ALT, 5=TA, 6=TARA.
    DatarefEntry {
        name: "sim/cockpit2/radios/actuators/transponder_mode",
        field: FieldId::TransponderMode,
    },
    // ---- v0.3.0 additions (Boeing 737 family — Zibo/LevelUp/B738) ----
    // These use the laminar/B738/* namespace shared by the default
    // Laminar 737, Zibo Mod, and LevelUp 737NG. On non-737 aircraft
    // the DataRef simply doesn't exist and X-Plane returns 0 — no
    // error, no spam in the activity log (the consumer code checks
    // `is_some()` before logging anything).
    DatarefEntry {
        name: "laminar/B738/toggle_switch/wing_light_pos",
        field: FieldId::LightWing,
    },
    DatarefEntry {
        name: "laminar/B738/toggle_switch/wheel_well_light_pos",
        field: FieldId::LightWheelWell,
    },
    // Spec v0.7.15 F6: X-Plane-Pause + Replay sind als Pause-aequivalent
    // an AeroACARS gemeldet (siehe FieldId::SimPaused / SimInReplay).
    // Die existierende `paused: false`-Konstante in to_snapshot() weiter
    // unten wird damit ersetzt durch den dynamischen Wert.
    DatarefEntry {
        name: "sim/time/paused",
        field: FieldId::SimPaused,
    },
    DatarefEntry {
        // QS-Finding (2026-05-12): Dataref-Name war fruehe Iteration mit
        // `sim_in_replay`-Tippfehler. X-Plane SDK + eigenes Plugin
        // (`xplane-plugin/src/plugin.cpp:649`) nutzen `is_in_replay`.
        name: "sim/time/is_in_replay",
        field: FieldId::SimInReplay,
    },
    DatarefEntry {
        name: "laminar/B738/annunciator/takeoff_config",
        field: FieldId::TakeoffConfigWarning,
    },
];

/// Mutable parsed state — populated as RREF responses arrive. Held
/// behind a Mutex by the adapter; on every snapshot request we copy
/// it out and convert to `SimSnapshot`.
#[derive(Debug, Clone, Default)]
pub struct XPlaneState {
    pub lat: f64,
    pub lon: f64,
    /// Stored in METERS (X-Plane native — `sim/flightmodel/position/elevation`
    /// reports meters MSL, not feet, despite the historic field name).
    /// Convert to feet at snapshot time alongside AGL.
    pub altitude_msl_m: f64,
    /// Stored in METERS (X-Plane native). Convert at snapshot time.
    pub altitude_agl_m: f64,
    pub heading_true_deg: f32,
    pub heading_magnetic_deg: f32,
    pub pitch_deg: f32,
    pub bank_deg: f32,
    pub vertical_speed_fpm: f32,
    /// Stored in M/S (X-Plane native). Convert at snapshot time.
    pub groundspeed_ms: f32,
    pub indicated_airspeed_kt: f32,
    pub true_airspeed_kt: f32,
    pub g_force: f32,
    pub on_ground: bool,
    /// v0.4.4: Normal force on the gear (N). 0 in air, spikes on
    /// touchdown. Used by Sampler-Side-Edge-Detection im Main-Crate.
    pub gear_normal_force_n: f32,
    pub parking_brake_ratio: f32,
    pub gear_deploy: f32,
    pub flaps_handle: f32,
    pub eng1_running: bool,
    pub fuel_total_kg: f32,
    pub empty_weight_kg: f32,
    pub total_weight_kg: f32,
    pub local_vx_ms: f32,
    pub local_vz_ms: f32,
    pub wind_x_ms: f32,
    pub wind_z_ms: f32,
    /// v0.5.19: absolute wind speed (m/s) at aircraft altitude.
    pub wind_now_speed_ms: f32,
    /// v0.5.19: absolute wind direction (degrees true) at aircraft altitude.
    pub wind_now_direction_degt: f32,
    // Phase 2: multi-engine, lights, AP, surfaces, systems, environment.
    pub eng2_running: bool,
    pub eng3_running: bool,
    pub eng4_running: bool,
    pub light_landing: bool,
    pub light_beacon: bool,
    pub light_strobe: bool,
    pub light_taxi: bool,
    pub light_nav: bool,
    pub ap_master: bool,
    pub ap_heading: bool,
    pub ap_altitude: bool,
    pub ap_nav: bool,
    pub ap_approach: bool,
    pub spoilers_handle: f32,
    pub spoilers_armed: bool,
    pub stall_warning: bool,
    pub battery_master: bool,
    pub avionics_master: bool,
    pub apu_switch: bool,
    pub pitot_heat: bool,
    pub qnh_inhg: f32,
    pub oat_c: f32,
    pub mach: f32,
    // v0.3.0 additions (universal):
    /// 0=RTO, 1=OFF, 2=1, 3=2, 4=3, 5=MAX. Stored as f32 from the
    /// RREF feed; mapped to label string at snapshot boundary.
    pub autobrake_level: f32,
    /// 0=OFF, 1=STBY, 2=ON, 3=TEST, 4=ALT, 5=TA, 6=TARA. Same.
    pub transponder_mode: f32,
    // v0.3.0 additions (Boeing 737 family):
    pub light_wing: bool,
    pub light_wheel_well: bool,
    pub takeoff_config_warning: bool,
    /// Spec v0.7.15 F6: `sim/time/paused`-Wert. true wenn der User die
    /// Pause-Taste in X-Plane gedrueckt hat. SimSnapshot.paused wird
    /// daraus gespeist.
    pub sim_paused: bool,
    /// Spec v0.7.15 F6: `sim/time/is_in_replay`-Wert. AeroACARS
    /// behandelt Replay als Pause-aequivalent (kein echter Flug).
    pub sim_in_replay: bool,
    /// True once we've received at least one RREF packet — drives
    /// the connection state machine's transition into `Connected`.
    pub got_first_packet: bool,
}

/// Map an X-Plane autobrake-level (0..5) to the cockpit-readable label.
/// Mirrors the `sim/cockpit2/switches/auto_brake_level` semantics.
pub fn xplane_autobrake_label(level: u8) -> &'static str {
    match level {
        0 => "RTO",
        1 => "OFF",
        2 => "1",
        3 => "2",
        4 => "3",
        5 => "MAX",
        _ => "",
    }
}

/// Map an X-Plane transponder-mode (0..6) to the cockpit-readable label.
/// Mirrors the `sim/cockpit2/radios/actuators/transponder_mode` semantics.
pub fn xplane_xpdr_mode_label(mode: u8) -> &'static str {
    match mode {
        0 => "OFF",
        1 => "STBY",
        2 => "XPNDR", // X-Plane "ON" = transponder broadcasting
        3 => "TEST",
        4 => "ALT",
        5 => "TA",
        6 => "TA-RA",
        _ => "",
    }
}

impl XPlaneState {
    /// Apply one decoded value to its `FieldId`.
    ///
    /// v0.12.2: the caller (the UDP listener) resolves the RREF index →
    /// active-catalog entry → `FieldId`, and applies any profile
    /// `ValueMapping`, before calling this. This function just writes
    /// the already-mapped value onto the right field.
    pub fn apply_field(&mut self, field: FieldId, value: f32) {
        self.got_first_packet = true;
        match field {
            FieldId::Latitude => self.lat = value as f64,
            FieldId::Longitude => self.lon = value as f64,
            // MSL: DataRef now delivers FEET (`altitude_ft_pilot`).
            // We still store internally in meters to keep the
            // snapshot conversion uniform with AGL — convert ft→m here.
            // 0.3048 mirrors the M_PER_FT constant in `to_snapshot()`.
            FieldId::AltitudeMslFt => self.altitude_msl_m = (value as f64) * 0.3048,
            FieldId::AltitudeAglFt => self.altitude_agl_m = value as f64, // y_agl: native meters
            FieldId::HeadingDegTrue => self.heading_true_deg = value,
            FieldId::HeadingDegMagnetic => self.heading_magnetic_deg = value,
            FieldId::PitchDeg => self.pitch_deg = value,
            FieldId::BankDeg => self.bank_deg = value,
            // local_vy ist in m/s (X-Plane native), konvertieren zu
            // fpm: 1 m/s = 196.8504 ft/min. Vorzeichen passt direkt
            // (positive Y = climb in beiden Konventionen).
            FieldId::VerticalSpeedFpm => self.vertical_speed_fpm = value * 196.8504,
            FieldId::GroundspeedKt => self.groundspeed_ms = value, // m/s native
            FieldId::IndicatedAirspeedKt => self.indicated_airspeed_kt = value,
            FieldId::TrueAirspeedKt => self.true_airspeed_kt = value,
            FieldId::GForce => self.g_force = value,
            FieldId::OnGround => self.on_ground = value > 0.5,
            FieldId::GearNormalForceN => self.gear_normal_force_n = value,
            FieldId::ParkingBrake => self.parking_brake_ratio = value,
            FieldId::GearDeploy => self.gear_deploy = value,
            FieldId::FlapsHandle => self.flaps_handle = value,
            FieldId::Eng1Running => self.eng1_running = value > 0.5,
            FieldId::Eng2Running => self.eng2_running = value > 0.5,
            FieldId::Eng3Running => self.eng3_running = value > 0.5,
            FieldId::Eng4Running => self.eng4_running = value > 0.5,
            FieldId::FuelTotalKg => self.fuel_total_kg = value,
            FieldId::EmptyWeightKg => self.empty_weight_kg = value,
            FieldId::TotalWeightKg => self.total_weight_kg = value,
            FieldId::LocalVxMs => self.local_vx_ms = value,
            FieldId::LocalVzMs => self.local_vz_ms = value,
            FieldId::WindXMs => self.wind_x_ms = value,
            FieldId::WindZMs => self.wind_z_ms = value,
            FieldId::WindNowSpeedMs => self.wind_now_speed_ms = value,
            FieldId::WindNowDirectionDegT => self.wind_now_direction_degt = value,
            FieldId::LightLanding => self.light_landing = value > 0.5,
            FieldId::LightBeacon => self.light_beacon = value > 0.5,
            FieldId::LightStrobe => self.light_strobe = value > 0.5,
            FieldId::LightTaxi => self.light_taxi = value > 0.5,
            FieldId::LightNav => self.light_nav = value > 0.5,
            FieldId::ApMaster => self.ap_master = value > 0.5,
            FieldId::ApHeading => self.ap_heading = value > 0.5,
            FieldId::ApAltitude => self.ap_altitude = value > 0.5,
            FieldId::ApNav => self.ap_nav = value > 0.5,
            FieldId::ApApproach => self.ap_approach = value > 0.5,
            FieldId::SpoilersHandle => self.spoilers_handle = value,
            FieldId::SpoilersArmed => self.spoilers_armed = value > 0.5,
            FieldId::StallWarning => self.stall_warning = value > 0.5,
            FieldId::BatteryMaster => self.battery_master = value > 0.5,
            FieldId::AvionicsMaster => self.avionics_master = value > 0.5,
            FieldId::ApuSwitch => self.apu_switch = value > 0.5,
            FieldId::PitotHeat => self.pitot_heat = value > 0.5,
            FieldId::QnhInHg => self.qnh_inhg = value,
            FieldId::OatC => self.oat_c = value,
            FieldId::Mach => self.mach = value,
            // v0.3.0 — universal additions
            FieldId::AutobrakeLevel => self.autobrake_level = value,
            FieldId::TransponderMode => self.transponder_mode = value,
            // v0.3.0 — 737 family additions
            FieldId::LightWing => self.light_wing = value > 0.5,
            FieldId::LightWheelWell => self.light_wheel_well = value > 0.5,
            FieldId::TakeoffConfigWarning => self.takeoff_config_warning = value > 0.5,
            // Spec v0.7.15 F6
            FieldId::SimPaused => self.sim_paused = value > 0.5,
            FieldId::SimInReplay => self.sim_in_replay = value > 0.5,
        }
    }

    /// Convert the accumulated state to a fresh `SimSnapshot`. The
    /// timestamp is stamped at conversion time (UTC now). Fields
    /// without an X-Plane equivalent stay at SimSnapshot's `Default`
    /// (None for Options, sensible zeros for required fields).
    pub fn to_snapshot(&self, simulator: Simulator) -> SimSnapshot {
        const M_PER_FT: f64 = 0.3048;
        const KT_PER_MS: f32 = 1.9438445; // 1 m/s = 1.9438 knots

        // Derive payload from ZFW-OEW like we do on the MSFS side.
        // ZFW = total weight - fuel.
        let zfw_kg = if self.total_weight_kg > 0.0 && self.total_weight_kg > self.fuel_total_kg {
            Some(self.total_weight_kg - self.fuel_total_kg)
        } else {
            None
        };
        let oew = if self.empty_weight_kg > 0.0 {
            Some(self.empty_weight_kg)
        } else {
            None
        };
        let payload_kg = match (zfw_kg, oew) {
            (Some(z), Some(o)) if z > o => Some(z - o),
            _ => None,
        };

        SimSnapshot {
            timestamp: chrono::Utc::now(),
            lat: self.lat,
            lon: self.lon,
            altitude_msl_ft: self.altitude_msl_m / M_PER_FT,
            altitude_agl_ft: self.altitude_agl_m / M_PER_FT,
            // v0.7.17 (B-003): X-Plane stellt diese MSFS-spezifischen
            // Pendants nicht direkt zur Verfuegung (X-Plane berechnet
            // pressure altitude implizit aus baro+QNH); wir lassen sie
            // hier None und debuggen ausschliesslich den MSFS-Pfad.
            altitude_indicated_ft: None,
            altitude_pressure_ft: None,
            heading_deg_true: self.heading_true_deg,
            heading_deg_magnetic: self.heading_magnetic_deg,
            pitch_deg: self.pitch_deg,
            bank_deg: self.bank_deg,
            vertical_speed_fpm: self.vertical_speed_fpm,
            velocity_body_x_fps: Some((self.local_vx_ms / 0.3048) as f32),
            velocity_body_z_fps: Some((self.local_vz_ms / 0.3048) as f32),
            groundspeed_kt: self.groundspeed_ms * KT_PER_MS,
            // X-Plane's pitot simulation produces small negative IAS/TAS
            // readings when the aircraft is stationary on the ground
            // (sensor noise, residual ram pressure modelling). Clamp to
            // zero at the source so neither the cockpit gauges nor
            // downstream consumers (PIREP, activity log) ever surface
            // a "−10 kt" — pilots reasonably treat that as a bug.
            indicated_airspeed_kt: self.indicated_airspeed_kt.max(0.0),
            true_airspeed_kt: self.true_airspeed_kt.max(0.0),
            aircraft_wind_x_kt: Some(self.wind_x_ms * KT_PER_MS),
            aircraft_wind_z_kt: Some(self.wind_z_ms * KT_PER_MS),
            g_force: self.g_force,
            on_ground: self.on_ground,
            // v0.7.19: X-Plane setzt `crashed` in v0.7.19 NICHT (kein
            // verifizierter Crash-DataRef). Die gemeinsame Heuristik
            // greift bei harten Aufschlaegen. Spec §Leitentscheidung 3.
            crashed: false,
            crash_source: None,
            gear_normal_force_n: Some(self.gear_normal_force_n),
            parking_brake: self.parking_brake_ratio > 0.5,
            stall_warning: self.stall_warning,
            overspeed_warning: false, // X-Plane has no direct overspeed annunciator
            // Spec v0.7.15 F6: aus sim/time/paused + sim/time/is_in_replay
            // ableiten. Beide werden als Pause-aequivalent behandelt — Replay
            // ist keine echte Flugaufzeichnung, der Streamer-Loop pausiert
            // also fuer beide Faelle und der Pause-Akkumulator zaehlt.
            paused: self.sim_paused || self.sim_in_replay,
            slew_mode: false,
            simulation_rate: 1.0,
            gear_position: self.gear_deploy,
            flaps_position: self.flaps_handle,
            engines_running: [
                self.eng1_running,
                self.eng2_running,
                self.eng3_running,
                self.eng4_running,
            ]
            .iter()
            .filter(|&&r| r)
            .count() as u8,
            fuel_total_kg: self.fuel_total_kg,
            fuel_used_kg: 0.0,
            zfw_kg,
            payload_kg,
            total_weight_kg: if self.total_weight_kg > 0.0 {
                Some(self.total_weight_kg)
            } else {
                None
            },
            // No latched-touchdown DataRef in X-Plane; the buffer-
            // based fallback in src/lib.rs takes over.
            touchdown_vs_fpm: None,
            touchdown_pitch_deg: None,
            touchdown_bank_deg: None,
            touchdown_heading_mag_deg: None,
            touchdown_lat: None,
            touchdown_lon: None,
            // v0.5.19: was hardcoded None — server-side live-tracking
            // monitor showed "—" for wind on every X-Plane pilot.
            // Now reads `wind_now_speed_msc` (m/s → kt) and
            // `wind_now_direction_degt` (deg true) DataRefs. We treat
            // 0/0 as "no wind data yet" (DataRefs not populated on
            // first tick) → None.
            wind_direction_deg: if self.wind_now_speed_ms > 0.0 {
                Some(self.wind_now_direction_degt)
            } else {
                None
            },
            wind_speed_kt: if self.wind_now_speed_ms > 0.0 {
                Some(self.wind_now_speed_ms * KT_PER_MS)
            } else {
                None
            },
            qnh_hpa: if self.qnh_inhg > 0.0 {
                // X-Plane reports altimeter setting in inHg natively;
                // convert to hPa (1 inHg = 33.8639 hPa).
                Some(self.qnh_inhg * 33.8639)
            } else {
                None
            },
            outside_air_temp_c: Some(self.oat_c),
            total_air_temp_c: None,
            mach: if self.mach > 0.0 { Some(self.mach) } else { None },
            empty_weight_kg: oew,
            aircraft_title: None,
            aircraft_icao: None,
            aircraft_registration: None,
            simulator,
            sim_version: None,
            // Avionics — X-Plane exposes COM/NAV via separate
            // DataRefs but addons disagree on conventions; keep
            // None for Phase 2, revisit if a payware author asks.
            transponder_code: None,
            com1_mhz: None,
            com2_mhz: None,
            nav1_mhz: None,
            nav2_mhz: None,
            light_landing: Some(self.light_landing),
            light_beacon: Some(self.light_beacon),
            light_strobe: Some(self.light_strobe),
            light_taxi: Some(self.light_taxi),
            light_nav: Some(self.light_nav),
            // X-Plane's nav-light DataRef covers logo on most payware.
            light_logo: Some(self.light_nav),
            strobe_state: None,
            autopilot_master: Some(self.ap_master),
            autopilot_heading: Some(self.ap_heading),
            autopilot_altitude: Some(self.ap_altitude),
            autopilot_nav: Some(self.ap_nav),
            autopilot_approach: Some(self.ap_approach),
            fuel_flow_kg_per_h: None,
            spoilers_handle_position: Some(self.spoilers_handle),
            spoilers_armed: Some(self.spoilers_armed),
            // Pushback isn't a sim-managed thing in X-Plane.
            pushback_state: None,
            apu_switch: Some(self.apu_switch),
            apu_pct_rpm: None,
            battery_master: Some(self.battery_master),
            avionics_master: Some(self.avionics_master),
            pitot_heat: Some(self.pitot_heat),
            engine_anti_ice: None,
            wing_anti_ice: None,
            // v0.3.0 — Boeing 737-family lights via laminar/B738/...
            // DataRef. Some(...) when the value is non-zero in the
            // RREF feed; None when the DataRef doesn't exist on the
            // loaded aircraft (X-Plane returns 0, so we'd report
            // "wing OFF" forever — to dodge that we only mark the
            // field Some(...) when at least one tick actually saw
            // a non-zero value, but for now we always wrap so the
            // generic activity-log path can compare. This matches
            // the existing light-handling in this file.
            light_wing: Some(self.light_wing),
            light_wheel_well: Some(self.light_wheel_well),
            // v0.3.0 — Universal XPDR mode label.
            xpdr_mode_label: {
                let label = xplane_xpdr_mode_label(self.transponder_mode as u8);
                if label.is_empty() {
                    None
                } else {
                    Some(label.to_string())
                }
            },
            // v0.3.0 — 737 takeoff-config annunciator. Same caveat
            // as light_wing — non-737 aircraft just stay false.
            takeoff_config_warning: Some(self.takeoff_config_warning),
            seatbelts_sign: None,
            no_smoking_sign: None,
            fcu_selected_altitude_ft: None,
            fcu_selected_heading_deg: None,
            fcu_selected_speed_kt: None,
            fcu_selected_vs_fpm: None,
            // v0.3.0 — Universal autobrake label.
            autobrake: {
                let label = xplane_autobrake_label(self.autobrake_level as u8);
                if label.is_empty() {
                    None
                } else {
                    Some(label.to_string())
                }
            },
            parking_name: None,
            parking_number: None,
            selected_runway: None,
            aircraft_profile: sim_core::AircraftProfile::default(),
            // PMDG SDK is MSFS-only; X-Plane never fills this.
            pmdg: None,
            // Category-aware landing: the static X-Plane gear-type descriptors
            // (acf_gear_is_skid, acf_water_rud_*) are not wired through this
            // path yet — they'd flow via the plugin/Web-API dataref set and
            // can't be verified without a running sim. The aircraft CATEGORY
            // is derived from the ICAO type when available (X-Plane 12 Web
            // API); on XP11 / Web-API-off it falls back to FixedWing, i.e.
            // unchanged behaviour (no regression). Wiring these datarefs is a
            // tracked future enhancement (needs in-sim verification).
            gear_is_skid: None,
            gear_is_floats: None,
            gear_is_wheels: None,
            contact_point_on_ground: None,
            gear_water_depth_m: None,
            water_rudder_present: None,
        }
    }
}
