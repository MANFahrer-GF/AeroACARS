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
    //
    // Update 2026-06-10: Root Cause fuer die Aerosoft A346 (ToLiss-Port)
    // ist per WASM-Strings-Analyse BESTAETIGT — das Aircraft treibt die
    // `GENERAL ENG COMBUSTION EX1:N`-Variante statt der plain SimVar.
    // EX1 wird jetzt nativ mitgelesen (siehe Tabellen-Ende) und per
    // Engine mit der plain Combustion geODERt; der N1-Fallback bleibt
    // als letzte Stufe fuer Addons, die KEINE der beiden treiben.
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

    // ---- Aerosoft A340-600 (ToLiss port) — WASM-Analyse 2026-06-10 ----
    // Strings-Analyse der `MSFS_ToLiss_Plugin.wasm` (Aerosoft A346 Pro =
    // ToLiss-Port) hat bewiesen, dass das Aircraft NICHT die plain
    // SimVars treibt, sondern die Varianten:
    //   * `GENERAL ENG COMBUSTION EX1:1..4` statt `GENERAL ENG
    //     COMBUSTION:N` → Root Cause des toten engines_running hinter
    //     dem v0.13.17-N1-Fallback.
    //   * `TURB ENG CORRECTED FF:1..4` statt `ENG FUEL FLOW PPH:N`
    //     → Root Cause des toten Fuel-Flow hinter der v0.13.18-FOB-
    //     Ableitung.
    // Beide Varianten sind Standard-MSFS-SimVars (SDK-dokumentiert) und
    // lesen auf Addons, die sie nicht treiben, schlicht 0/false → das
    // Mapping kann sie addon-agnostisch ODER-/kaskadieren, kein
    // Profile-Gate noetig. EX1 ist bool, CORRECTED FF kommt wie PPH in
    // pounds per hour.
    F::bool("GENERAL ENG COMBUSTION EX1:1"),
    F::bool("GENERAL ENG COMBUSTION EX1:2"),
    F::bool("GENERAL ENG COMBUSTION EX1:3"),
    F::bool("GENERAL ENG COMBUSTION EX1:4"),
    F::f64("TURB ENG CORRECTED FF:1", "pounds per hour"),
    F::f64("TURB ENG CORRECTED FF:2", "pounds per hour"),
    F::f64("TURB ENG CORRECTED FF:3", "pounds per hour"),
    F::f64("TURB ENG CORRECTED FF:4", "pounds per hour"),
    // AP-State liefert die A346 laut WASM-Analyse AUSSCHLIESSLICH als
    // LVars (`AB_AP_*_LIGHT_ON` — die FCU-Annunciator-Lampen); die
    // Standard-`AUTOPILOT *`-SimVars bleiben tot. Wie bei den Fenix-
    // LVars oben: Nicht-A346-Aircraft lesen 0, das Snapshot-Mapping
    // konsultiert sie nur bei AircraftProfile::AerosoftA346.
    F::f64("L:AB_AP_AP1_LIGHT_ON", "Number"),
    F::f64("L:AB_AP_AP2_LIGHT_ON", "Number"),
    F::f64("L:AB_AP_ATHR_LIGHT_ON", "Number"),
    F::f64("L:AB_AP_APPR_LIGHT_ON", "Number"),
    F::f64("L:AB_AP_LOC_LIGHT_ON", "Number"),

    // ---- Aerosoft A340-600 full profile (v0.16.4, WASM-Analyse
    // 2026-06-10) ----
    // Komfort-/System-LVars analog zur Fenix-Abdeckung weiter oben.
    // Alle Namen woertlich gegen die Strings der MSFS_ToLiss_Plugin
    // .wasm verifiziert. LVars lesen auf Nicht-A346-Addons schlicht 0;
    // das Snapshot-Mapping konsultiert sie NUR bei
    // AircraftProfile::AerosoftA346. LOCKSTEP: Reihenfolge MUSS mit
    // den pull_f64!-Aufrufen am Ende von `from_block` uebereinstimmen.
    //
    // Cabin signs (Overhead-Schalter; Wertebereich vermutlich 0/1
    // oder 0/1/2 wie beim realen Airbus OFF/AUTO/ON — Mapping clamps
    // auf 0..=2, Live-Flug-Verifikation steht aus).
    F::f64("L:AB_OVH_SEATBELT", "Number"),
    F::f64("L:AB_OVH_NO_SMOKING", "Number"),
    // Anti-Ice: 4 Engine-Schalter (L1/L2/R1/R2 — die A346 hat 4
    // Triebwerke), Wing, Probe/Window-Heat.
    F::f64("L:AB_OVH_ANTIICE_ENGL1", "Number"),
    F::f64("L:AB_OVH_ANTIICE_ENGL2", "Number"),
    F::f64("L:AB_OVH_ANTIICE_ENGR1", "Number"),
    F::f64("L:AB_OVH_ANTIICE_ENGR2", "Number"),
    F::f64("L:AB_OVH_ANTIICE_WING", "Number"),
    F::f64("L:AB_OVH_ANTIICE_PROBEWINDOW", "Number"),
    // Batterie: das sind die "OFF"-ANNUNCIATOR-LAMPEN der BAT-Push-
    // buttons (sie stehen im WASM-Strings-Block der *_OFF/*_FAULT/
    // *_AVAIL-Lampen-Legends, NICHT im *_PB-Schalter-Block) →
    // INVERTIERTE Semantik: Lampe an (1) = Batterie AUS. Das Mapping
    // invertiert explizit, siehe `telemetry_to_snapshot`.
    F::f64("L:AB_VC_OVH_ELEC_BAT1_OFF", "Number"),
    F::f64("L:AB_VC_OVH_ELEC_BAT2_OFF", "Number"),
    // Autobrake-Modus-Enum (Annahme: 0=OFF, 1=LO, 2=MED, 3=MAX wie
    // die realen A340-Stufen; unbekannte Werte mappen auf None).
    F::f64("L:AB_AutoBrake_Mode", "Number"),
    // Gear-SELECTOR-LEVER: die Standard-SimVar `GEAR POSITION` klemmt
    // bei der A346 auf "down" (v0.13.17-Befund) — der Hebel ist die
    // einzige brauchbare Quelle. Richtung (0=up/1=down) ist plausibel
    // aber unverifiziert bis zum ersten Live-Flug.
    F::f64("L:AB_MPL_LANDING_GEAR_SELECTOR_LEVER", "Number"),
    // ---- iniBuilds A350 (v0.16.8) ----
    // Quelle: HubHop-Preset-DB (IniBuilds.A350 (2024).Autopilot.* Output-
    // Presets, 2026-06-11) — Daten-Audit: 10 A350-Flüge, AP nie an, weil
    // der Standard-SimVar (wie bei allen Study-Addons) tot ist. Die
    // FCU-LED-LVars sind echte Annunciator-States:
    //   INI_ap1_on / INI_ap2_on   = AP1/AP2-LED (engaged)
    //   INI_ATHR_LIGHT            = A/THR-LED
    //   INI_MCU_LAND_LIGHT        = APPR-LED, INI_MCU_LOC_LIGHT = LOC-LED
    F::f64("L:INI_ap1_on", "Number"),
    F::f64("L:INI_ap2_on", "Number"),
    F::f64("L:INI_ATHR_LIGHT", "Number"),
    F::f64("L:INI_MCU_LAND_LIGHT", "Number"),
    F::f64("L:INI_MCU_LOC_LIGHT", "Number"),

    // ================================================================
    // v0.16.10 (#Premium): Cockpit-Tiefendaten — 5 LVar-Gruppen.
    // LOCKSTEP: append-only am Tabellen-Ende, gleiche Reihenfolge in
    // Telemetry-Struct + pull_f64!-Block. Tote LVars lesen auf fremden
    // Aircraft 0.0 → JEDES Mapping ist profile-gegated (siehe
    // premium_lvars_do_not_affect_default_profile-Test).
    // ================================================================

    // ---- Gruppe A: Fenix A32x Premium (HubHop-Output) ----
    // Quelle: HubHop-Preset-DB (Fenix.A320 Output-Presets). Achtung:
    // HubHop listet "L:L:N_MISC_PERF_TO_V1" mit Doppel-Prefix-TYPO —
    // die echten LVars tragen EIN "L:".
    // FMS-PERF-Page: eingegebene V-Speeds + FLEX-Temp (0 = noch nicht
    // eingegeben → None im Mapping).
    F::f64("L:N_MISC_PERF_TO_V1", "Number"),
    F::f64("L:N_MISC_PERF_TO_VR", "Number"),
    F::f64("L:N_MISC_PERF_TO_V2", "Number"),
    F::f64("L:N_MISC_PERF_TO_FLEX", "Number"),
    // Master-Caution/-Warning: Capt-seitige Annunciator-Lampen.
    F::f64("L:I_MIP_MASTER_CAUTION_CAPT", "Number"),
    F::f64("L:I_MIP_MASTER_WARNING_CAPT", "Number"),
    // Speedbrake-Handle (analog 0..1). NUR subscribed, KEIN Override:
    // die Standard-SimVar `SPOILERS HANDLE POSITION` ist analog
    // (percent over 100) und beim Fenix nicht als defekt/binaer
    // dokumentiert — der LVar bleibt Override-Kandidat, falls ein
    // Live-Flug zeigt, dass der Standard dort nur 0/1 liefert.
    F::f64("L:A_FC_SPEEDBRAKE", "Number"),
    // FCU-managed-Dots (Lampen neben den FCU-Fenstern) + EFIS BARO STD.
    F::f64("L:I_FCU_SPEED_MANAGED", "Number"),
    F::f64("L:I_FCU_HEADING_MANAGED", "Number"),
    F::f64("L:I_FCU_ALTITUDE_MANAGED", "Number"),
    F::f64("L:S_FCU_EFIS1_BARO_STD", "Number"),
    // Engine-Fire-Lampen: subscribed, aber bewusst OHNE eigenes
    // Snapshot-Feld — ein Fire zieht ohnehin das Master Warning;
    // das Activity-Log lebt von master_warning.
    F::f64("L:I_ENG_FIRE_1", "Number"),
    F::f64("L:I_ENG_FIRE_2", "Number"),

    // ---- Gruppe B: FBW-A32NX-Familie (FBW-Doku) ----
    // Quelle: open-source FBW-Doku (fbw-a32nx/docs/a320-simvars.md).
    // Deckt FBW A32NX + FBW A380X + Headwind A339 ab (gleiche
    // A32NX_-LVar-Familie, ein Profil FbwA32nx). Inventar-Befund:
    // die V-Speeds heissen VSPEEDS_* (nicht SPEEDS_*).
    F::f64("L:A32NX_AUTOPILOT_1_ACTIVE", "Number"),
    F::f64("L:A32NX_AUTOPILOT_2_ACTIVE", "Number"),
    // AUTOTHRUST_STATUS: 0=off, 1=armed, 2=active.
    F::f64("L:A32NX_AUTOTHRUST_STATUS", "Number"),
    F::f64("L:A32NX_AUTOTHRUST_MODE", "Number"),
    F::f64("L:A32NX_FMA_LATERAL_MODE", "Number"),
    F::f64("L:A32NX_FMA_VERTICAL_MODE", "Number"),
    // FWC-Flugphase (1..10, siehe fbw_fwc_phase_label).
    F::f64("L:A32NX_FWC_FLIGHT_PHASE", "Number"),
    F::f64("L:A32NX_VSPEEDS_V2", "Number"),
    F::f64("L:A32NX_VSPEEDS_VLS", "Number"),
    F::f64("L:A32NX_VSPEEDS_VAPP", "Number"),
    // 0=DIS, 1=LO, 2=MED, 3=MAX.
    F::f64("L:A32NX_AUTOBRAKES_ARMED_MODE", "Number"),
    // Flaps-Lever-Detent: subscribed als Cross-Check; das generische
    // `FLAPS HANDLE PERCENT` funktioniert beim FBW → kein Override.
    F::f64("L:A32NX_FLAPS_HANDLE_INDEX", "Number"),
    F::f64("L:A32NX_SPOILERS_ARMED", "Number"),
    F::f64("L:A32NX_SPOILERS_GROUND_SPOILERS_ACTIVE", "Number"),
    F::f64("L:A32NX_FCU_SPD_MANAGED_DOT", "Number"),
    F::f64("L:A32NX_FCU_HDG_MANAGED_DOT", "Number"),
    F::f64("L:A32NX_FCU_ALT_MANAGED", "Number"),

    // ---- Gruppe C: iniBuilds A350/A340 Premium (WASM-strings) ----
    // Quelle: WASM-Strings-Dump der iniBuilds A350 — ALLE Namen
    // woertlich gegen den Dump verifiziert (2026-06-11). Dieselben
    // INI_-LVars treibt auch die iniBuilds A340 (Inventar: 36/38
    // woertlich bestaetigt) → Mapping unter is_a350 ODER is_a340.
    // FMA-Mode-Enums: Belegung UNBEKANNT — Mapping reicht Roh-Werte
    // als "#{n}" durch (Decode beim ersten Live-Flug).
    F::f64("L:INI_ROLL_MODE_ACTIVE", "Number"),
    F::f64("L:INI_PITCH_MODE_ACTIVE", "Number"),
    F::f64("L:INI_THROTTLE_MODE_ACTIVE", "Number"),
    F::f64("L:INI_V1", "Number"),
    F::f64("L:INI_VR", "Number"),
    F::f64("L:INI_V2", "Number"),
    F::f64("L:INI_VLS_SPEED", "Number"),
    F::f64("L:INI_VAPP_SPEED", "Number"),
    F::f64("L:INI_VREF_SPEED", "Number"),
    F::f64("L:INI_FLEX_TEMPERATURE", "Number"),
    // Thrust-Lever-Gate-Flags (TOGA > FLX/MCT > CL Prioritaet).
    F::f64("L:INI_LEVER_IN_TOGA", "Number"),
    F::f64("L:INI_LEVER_IN_FLEX_MCT", "Number"),
    F::f64("L:INI_LEVER_IN_CL", "Number"),
    // Flaps-Detent: subscribed als Cross-Check, generisch reicht.
    F::f64("L:INI_FLAPS_HANDLE_INDEX", "Number"),
    F::f64("L:INI_SPOILERS_GROUND_SPOILERS_ACTIVE", "Number"),
    // Autobrake-ENGAGED-Flag: subscribed, noch UNGEMAPPT — liefert
    // kein Stufen-Label; Decode/Nutzen beim ersten Live-Flug klaeren.
    F::f64("L:INI_AUTOBRAKE_ENGAGED", "Number"),
    F::f64("L:INI_MASTER_CAUTION_CAPT_TOP", "Number"),
    F::f64("L:INI_MASTER_WARNING_CAPT_TOP", "Number"),
    // Per-Engine-Fuel-Flow in kg: subscribed fuer kuenftige Nutzung
    // (die generische PPH/CORRECTED-FF-Kaskade funktioniert bereits).
    // FLOW3/4 sind A340-only (4 Triebwerke) — fehlen erwartungsgemaess
    // im A350-Dump, Namen folgen dem verifizierten FLOW1/2-Muster
    // (Inventar-A340-Bestaetigung; Live-Flug-Verifikation steht aus).
    F::f64("L:INI_FUEL_FLOW1_KG", "Number"),
    F::f64("L:INI_FUEL_FLOW2_KG", "Number"),
    F::f64("L:INI_FUEL_FLOW3_KG", "Number"),
    F::f64("L:INI_FUEL_FLOW4_KG", "Number"),
    // A340-Extra: Autobrake-Selector (HubHop: 3=MED, 4=MAX, 5=LO).
    F::f64("L:INI_AUTOBRAKE_LEVEL", "Number"),

    // ---- Gruppe D: Aerosoft A346 Premium-Extras (WASM-strings) ----
    // Quelle: Strings der MSFS_ToLiss_Plugin.wasm — alle Namen
    // woertlich verifiziert (Inventar 2026-06-11).
    // FMGC-Phase-Enum: Belegung unbekannt → "#{n}"-Mapping.
    F::f64("L:TLS_FLIGHT_PHASE", "Number"),
    // FCU-managed-Dots (SPD/HDG/VS — die A346-FCU hat kein eigenes
    // ALT-Dot; VS_MANAGED naehert managed_altitude an, s. Mapping).
    F::f64("L:TLS_FCU_SPD_MANAGED", "Number"),
    F::f64("L:TLS_FCU_HDG_MANAGED", "Number"),
    F::f64("L:TLS_FCU_VS_MANAGED", "Number"),
    F::f64("L:AB_MPL_Master_Warning_Light", "Number"),
    F::f64("L:AB_MPL_Master_Caution_Light", "Number"),
    // Speedbrake-Lever-Position: subscribed, KEIN Override — die
    // generische `SPOILERS HANDLE POSITION` bleibt die Quelle.
    F::f64("L:TLS_SPD_BRK_LEVER_POS", "Number"),
    F::f64("L:TLS_SPOILER_LEVER_ARMED", "Number"),
    // Reverser-Ratio je Triebwerk (0..1) → reverser_deployed wenn
    // irgendeine Ratio > 0.05.
    F::f64("L:TLS_ENG1_REVERSER_RATIO", "Number"),
    F::f64("L:TLS_ENG2_REVERSER_RATIO", "Number"),
    F::f64("L:TLS_ENG3_REVERSER_RATIO", "Number"),
    F::f64("L:TLS_ENG4_REVERSER_RATIO", "Number"),

    // ---- Gruppe E: TFDi MD-11 (TFDi-Doku + WASM-strings) ----
    // Quelle: offizielle TFDi-Doku + WASM-Dump — alle Namen woertlich
    // gegen den Dump verifiziert (2026-06-11).
    // AP_STATE dokumentiert: 0=Off, 1=AP1, 2=AP2, 3=both.
    F::f64("L:MD11_AP_STATE", "Number"),
    // ATS_STATE: Enum undokumentiert; >0 = ATS on.
    F::f64("L:MD11_ATS_STATE", "Number"),
    // ATS-CLAMP-Mode: subscribed, ungemappt (Decode beim Live-Flug).
    F::f64("L:MD11_ATS_CLAMP", "Number"),
    // AFS-Targets — Dash-Sentinels: SPD/HDG = -999, V/S = -9999.
    F::f64("L:MD11_AFS_SPD", "Number"),
    F::f64("L:MD11_AFS_HDG", "Number"),
    F::f64("L:MD11_AFS_ALT", "Number"),
    F::f64("L:MD11_AFS_VS", "Number"),
    F::f64("L:MD11_V1", "Number"),
    F::f64("L:MD11_VR", "Number"),
    F::f64("L:MD11_V2", "Number"),
    // Display-exakte N1-Werte der 3 Triebwerke (eng_n1_pct bevorzugt
    // sie gegenueber den Standard-SimVars, wenn > 0).
    F::f64("L:MD11_ENG1_N1", "Number"),
    F::f64("L:MD11_ENG2_N1", "Number"),
    F::f64("L:MD11_ENG3_N1", "Number"),
    // Autobrake-Selector: Positions-Enum undokumentiert → "#{n}".
    F::f64("L:MD11_CTR_AUTOBRAKE_SW", "Number"),

    // ---- iFly 737 MAX 8 (v0.16.11) — WASM-strings + HubHop ----
    // Quelle: WASM-Strings-Dump des iFly-Pakets (alle Namen woertlich
    // verifiziert; die Caution-/Fire-Lampen sind dort printf-Templates
    // `VC_*_Light_%d_VAL` → Index 1 = Capt-Seite) + HubHop-Output-
    // Presets fuer die CMD-A/B-LEDs. Nur bei AircraftProfile::IflyMax8
    // gemappt.
    F::f64("L:VC_CMD_A_SW_LIGHT_VAL", "Number"),   // AP CMD A LED (HubHop-Output)
    F::f64("L:VC_CMD_B_SW_LIGHT_VAL", "Number"),   // AP CMD B LED (HubHop-Output)
    F::f64("L:VC_AT_ARM_LIGHT_VAL", "Number"),     // A/T ARM light (ARM-Semantik wie PMDG NG3)
    F::f64("L:VC_Master_Caution_Light_1_VAL", "Number"),
    F::f64("L:VC_Fire_Warning_Light_1_VAL", "Number"),  // 737: Fire-Warn = rote Master-Klasse
    F::f64("L:VC_WARNING_LIGHT_CABIN_ALTITUDE_L_VAL", "Number"),
    F::f64("L:Animation_Engine_1_Reverser_VAL", "Number"),  // 0..1 Reverser-Stellung
    F::f64("L:Animation_Engine_2_Reverser_VAL", "Number"),
    F::f64("L:VC_FLTCTRL_LIGHT_SPEEDBRAKES_EXTENDED_VAL", "Number"),
    F::f64("L:VC_Autobrake_SW_VAL", "Number"),     // Selektor-Enum unbekannt → "#n"

    // ---- FSLabs A321 (ceo+neo, v0.16.14) — HubHop-Output-Presets ----
    // FSL faehrt die Systeme in einem EXTERNEN Prozess (Paket-WASMs sind
    // Stubs) — die LVars existieren nur zur Laufzeit; HubHop ist die
    // dokumentierte Lese-Oberflaeche. _Brt_Lt = LED-Helligkeit (Button-
    // Presets nutzen ">50"-Idiom) — Schwelle >10 = "leuchtet",
    // Live-Verifikation beim ersten FSL-Flug.
    F::f64("L:VC_GSLD_FCU_AP1_Brt_Lt", "Number"),
    F::f64("L:VC_GSLD_FCU_AP2_Brt_Lt", "Number"),
    F::f64("L:VC_GSLD_FCU_ATHR_Brt_Lt", "Number"),
    F::f64("L:VC_GSLD_FCU_APPR_Brt_Lt", "Number"),
    F::f64("L:VC_GSLD_FCU_LOC_Brt_Lt", "Number"),
    F::f64("L:FSL_FCU_SPD", "Number"),
    F::f64("L:FSL_FCU_HDG", "Number"),
    F::f64("L:FSL_FCU_ALT", "Number"),
    F::f64("L:FSL_FCU_VS", "Number"),
    F::f64("L:FSL_FCU_SPD_MANAGED", "Number"),
    F::f64("L:FSL_FCU_HDG_MANAGED", "Number"),
    F::f64("L:FSL_FCU_ALT_MANAGED", "Number"),
    F::f64("L:FSL_FCU_SPD_DASHED", "Number"),
    F::f64("L:FSL_FCU_HDG_DASHED", "Number"),
    // v0.16.20: Casing-Fix. Die echte FSLabs-Variable heisst "_Button_"
    // (gemischt, vgl. WinWing-Profil /tmp/fsl_all_lvars.txt), NICHT
    // "_BUTTON_". Mit der falschen Schreibweise loesten die LVars nie auf
    // (Peters Log las Autobrake ueber den ganzen Flug None) — LVar-Namen
    // sind in MSFS case-sensitiv.
    F::f64("L:VC_MIP_BRAKES_AUTOBRK_LO_Button_BOT", "Number"),
    F::f64("L:VC_MIP_BRAKES_AUTOBRK_MED_Button_BOT", "Number"),
    F::f64("L:VC_MIP_BRAKES_AUTOBRK_MAX_Button_BOT", "Number"),

    // ---- FSLabs A321 PREMIUM (v0.16.20) — echte LVars aus dem
    //      WinWing/StreamDeck-Forum-Profil (FSLabsA3x_Scripts.xml) ----
    // Quelle: /tmp/fsl_all_lvars.txt (202 Vars) + die Read-Kontexte der
    // Skripte (z.B. `(L:VC_PED_PARK_BRAKE_Switch, Number) 0 ==`). Alle
    // Namen WOERTLICH (case-sensitiv) uebernommen. Nur bei
    // AircraftProfile::FsLabsA321 konsultiert. Hintergrund (Peters Log,
    // 0.16.18): FSL FAELSCHT die Standard-SimVars — am Gate mit echt
    // ausgeschalteten Triebwerken + gesetzter Parkbremse lasen wir
    // engines_running=2 (gefaelschte COMBUSTION), parking_brake=False
    // (toter SimVar). Die echten FSL-Schalter unten liefern die wahren
    // Signale, damit die Auto-End-FSM (TaxiIn→BlocksOn→Arrived) feuert.
    //
    // KRITISCH (Flug-Ende):
    //   PARK_BRAKE_Switch: Skript `0 == if{ released }` → !=0 = gesetzt.
    //   ENG_x_MSTR_Switch: Skript-Schwellen `20 >=`=ON, `10 <=`=OFF →
    //     Position, kein 0/1. Wir werten >= 15 (Mitte) = Master ON.
    //   FSLA320_WHEEL_CHOCKS: Bool-Toggle (`! (>L:...)`), Bonus-Signal.
    F::f64("L:VC_PED_PARK_BRAKE_Switch", "Number"),
    F::f64("L:VC_PED_ENG_1_MSTR_Switch", "Number"),
    F::f64("L:VC_PED_ENG_2_MSTR_Switch", "Number"),
    F::f64("L:FSLA320_WHEEL_CHOCKS", "Bool"),
    // PREMIUM (Mapping auf bestehende SimSnapshot-Felder):
    //   EFIS_CPT_BARO_STD: !=0 = STD-Fenster (baro_std).
    //   Caution/Warning_Button_BOT: Skript `0 != if{ MSTR ... }` → !=0
    //     = Master-Lampe gedrueckt/aktiv.
    //   SPD_BRK_LEVER: Skript `0 ==`=eingefahren, `10 ==`=armed →
    //     diskrete Stellung, NICHT 0..1. Auf 0..1 normalisiert
    //     (Annahme Range 0..~50; "Range beim naechsten Flug verifizieren").
    //   ENGFIRE_x_LT_TOP: rote Feuer-Lampe → hebt master_warning.
    //   AI_Eng_x_Anti_Ice_Button: Skript `0 !=`=an → engine_anti_ice.
    //   ATCXPDR_MODE_SWITCH: Skript `/10` → 0=STBY,1=TA,2=TARA.
    F::f64("L:FSL_EFIS_CPT_BARO_STD", "Number"),
    F::f64("L:VC_GSLD_CP_Caution_Button_BOT", "Number"),
    F::f64("L:VC_GSLD_CP_Warning_Button_BOT", "Number"),
    F::f64("L:VC_PED_SPD_BRK_LEVER", "Number"),
    F::f64("L:VC_PED_ENGFIRE_1_LT_TOP", "Number"),
    F::f64("L:VC_PED_ENGFIRE_2_LT_TOP", "Number"),
    F::f64("L:VC_OVHD_AI_Eng_1_Anti_Ice_Button", "Number"),
    F::f64("L:VC_OVHD_AI_Eng_2_Anti_Ice_Button", "Number"),
    F::f64("L:VC_PED_ATCXPDR_MODE_SWITCH", "Number"),
    // v0.16.20 (Review-Fund): Transponder ON/OFF-Schalter. Skript
    // (FSLabsA3x_Scripts.xml) liest ihn `/10` → 0=OFF, 1=AUTO, 2=ON
    // (Rohwert 0/10/20). Wird genutzt, um `xpdr_mode_label` zu
    // unterdruecken (None), wenn der Transponder AUS ist — der
    // MODE-Switch labelt sonst auch bei abgeschaltetem XPDR. LOCKSTEP:
    // append-only am Tabellen-Ende, gleiche Reihenfolge im
    // pull_f64!-Block + Telemetry-Struct.
    F::f64("L:VC_PED_ATCXPDR_ON_OFF_Switch", "Number"),
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

    // Aerosoft A340-600 (ToLiss port) — WASM-Analyse 2026-06-10.
    // `GENERAL ENG COMBUSTION EX1:1..4`: die SimVar-Variante, die die
    // A346 statt der plain Combustion treibt. Addon-agnostisch per
    // Engine mit `engN_firing` geODERt (liest auf anderen Addons false).
    pub eng1_combustion_ex1: bool,
    pub eng2_combustion_ex1: bool,
    pub eng3_combustion_ex1: bool,
    pub eng4_combustion_ex1: bool,
    // `TURB ENG CORRECTED FF:1..4` (pounds per hour): die Fuel-Flow-
    // Variante der A346. Kaskade im Mapping: PPH-Summe > 0 gewinnt,
    // sonst diese Summe, sonst None (dann greift die v0.13.18-FOB-
    // Ableitung im Position-Streamer).
    pub eng1_ff_corrected_pph: f64,
    pub eng2_ff_corrected_pph: f64,
    pub eng3_ff_corrected_pph: f64,
    pub eng4_ff_corrected_pph: f64,
    // `L:AB_AP_*_LIGHT_ON`: FCU-Annunciator-Lampen der A346 — laut
    // WASM-Analyse die EINZIGE AP-State-Quelle (Standard-SimVars tot).
    // Nur bei AircraftProfile::AerosoftA346 konsultiert.
    pub a346_ap1_light: f64,
    pub a346_ap2_light: f64,
    pub a346_athr_light: f64,
    pub a346_appr_light: f64,
    pub a346_loc_light: f64,

    // Aerosoft A340-600 full profile (v0.16.4) — Komfort-/System-
    // LVars, nur bei AircraftProfile::AerosoftA346 konsultiert.
    // Cabin signs (Schalterposition, geclamped 0..=2 im Mapping).
    pub a346_seatbelt_sw: f64,
    pub a346_no_smoking_sw: f64,
    // Anti-Ice-Schalter: 4 Engines + Wing + Probe/Window.
    pub a346_antiice_engl1: f64,
    pub a346_antiice_engl2: f64,
    pub a346_antiice_engr1: f64,
    pub a346_antiice_engr2: f64,
    pub a346_antiice_wing: f64,
    pub a346_antiice_probewindow: f64,
    // BAT-"OFF"-Annunciator-LAMPEN — INVERTIERT: 1 = Batterie aus,
    // 0 = Batterie an (oder LVar noch nicht initialisiert).
    pub a346_bat1_off_light: f64,
    pub a346_bat2_off_light: f64,
    // Autobrake-Modus-Enum (Annahme 0=OFF/1=LO/2=MED/3=MAX).
    pub a346_autobrake_mode: f64,
    // Gear-Selector-Lever (0=up/1=down angenommen, unverifiziert).
    pub a346_gear_lever: f64,
    // ---- iniBuilds A350 (v0.16.8) — nur bei AircraftProfile::IniA350 konsultiert ----
    pub a350_ap1_on: f64,
    pub a350_ap2_on: f64,
    pub a350_athr_light: f64,
    pub a350_appr_light: f64,
    pub a350_loc_light: f64,

    // ================================================================
    // v0.16.10 (#Premium) — LOCKSTEP mit dem Tabellen-Ende, gleiche
    // Gruppen-Reihenfolge (A Fenix, B FBW, C INI, D A346, E MD-11).
    // ================================================================

    // Gruppe A: Fenix A32x Premium (HubHop-Output).
    pub fnx_perf_v1: f64,
    pub fnx_perf_vr: f64,
    pub fnx_perf_v2: f64,
    pub fnx_perf_flex: f64,
    pub fnx_master_caution: f64,
    pub fnx_master_warning: f64,
    pub fnx_speedbrake_handle: f64,
    pub fnx_fcu_spd_managed: f64,
    pub fnx_fcu_hdg_managed: f64,
    pub fnx_fcu_alt_managed: f64,
    pub fnx_baro_std: f64,
    pub fnx_eng1_fire: f64,
    pub fnx_eng2_fire: f64,

    // Gruppe B: FBW-A32NX-Familie (FBW-Doku).
    pub fbw_ap1_active: f64,
    pub fbw_ap2_active: f64,
    pub fbw_athr_status: f64,
    pub fbw_athr_mode: f64,
    pub fbw_fma_lateral: f64,
    pub fbw_fma_vertical: f64,
    pub fbw_fwc_phase: f64,
    pub fbw_vspeeds_v2: f64,
    pub fbw_vspeeds_vls: f64,
    pub fbw_vspeeds_vapp: f64,
    pub fbw_autobrake_armed_mode: f64,
    pub fbw_flaps_handle_index: f64,
    pub fbw_spoilers_armed: f64,
    pub fbw_ground_spoilers_active: f64,
    pub fbw_fcu_spd_dot: f64,
    pub fbw_fcu_hdg_dot: f64,
    pub fbw_fcu_alt_managed: f64,

    // Gruppe C: iniBuilds A350/A340 Premium (WASM-strings).
    pub ini_roll_mode: f64,
    pub ini_pitch_mode: f64,
    pub ini_throttle_mode: f64,
    pub ini_v1: f64,
    pub ini_vr: f64,
    pub ini_v2: f64,
    pub ini_vls: f64,
    pub ini_vapp: f64,
    pub ini_vref: f64,
    pub ini_flex_temp: f64,
    pub ini_lever_toga: f64,
    pub ini_lever_flex_mct: f64,
    pub ini_lever_cl: f64,
    pub ini_flaps_handle_index: f64,
    pub ini_ground_spoilers: f64,
    pub ini_autobrake_engaged: f64,
    pub ini_master_caution: f64,
    pub ini_master_warning: f64,
    pub ini_fuel_flow1_kg: f64,
    pub ini_fuel_flow2_kg: f64,
    pub ini_fuel_flow3_kg: f64,
    pub ini_fuel_flow4_kg: f64,
    pub ini_autobrake_level: f64,

    // Gruppe D: Aerosoft A346 Premium-Extras (WASM-strings).
    pub a346_flight_phase: f64,
    pub a346_fcu_spd_managed: f64,
    pub a346_fcu_hdg_managed: f64,
    pub a346_fcu_vs_managed: f64,
    pub a346_master_warning_light: f64,
    pub a346_master_caution_light: f64,
    pub a346_spd_brk_lever_pos: f64,
    pub a346_spoiler_lever_armed: f64,
    pub a346_eng1_rev_ratio: f64,
    pub a346_eng2_rev_ratio: f64,
    pub a346_eng3_rev_ratio: f64,
    pub a346_eng4_rev_ratio: f64,

    // Gruppe E: TFDi MD-11 (TFDi-Doku + WASM-strings).
    pub md11_ap_state: f64,
    pub md11_ats_state: f64,
    pub md11_ats_clamp: f64,
    pub md11_afs_spd: f64,
    pub md11_afs_hdg: f64,
    pub md11_afs_alt: f64,
    pub md11_afs_vs: f64,
    pub md11_v1: f64,
    pub md11_vr: f64,
    pub md11_v2: f64,
    pub md11_eng1_n1: f64,
    pub md11_eng2_n1: f64,
    pub md11_eng3_n1: f64,
    pub md11_autobrake_sw: f64,

    // Gruppe F: iFly 737 MAX 8 (v0.16.11, WASM-strings + HubHop) —
    // nur bei AircraftProfile::IflyMax8 konsultiert.
    pub ifly_cmd_a_light: f64,
    pub ifly_cmd_b_light: f64,
    pub ifly_at_arm_light: f64,
    pub ifly_master_caution_light: f64,
    pub ifly_fire_warning_light: f64,
    pub ifly_cabin_alt_warning_light: f64,
    pub ifly_eng1_reverser: f64,
    pub ifly_eng2_reverser: f64,
    pub ifly_speedbrakes_extended_light: f64,
    pub ifly_autobrake_sw: f64,

    // Gruppe G: FSLabs A321 (v0.16.14, HubHop-Output-Presets) —
    // nur bei AircraftProfile::FsLabsA321 konsultiert.
    pub fsl_ap1_light: f64,
    pub fsl_ap2_light: f64,
    pub fsl_athr_light: f64,
    pub fsl_appr_light: f64,
    pub fsl_loc_light: f64,
    pub fsl_fcu_spd: f64,
    pub fsl_fcu_hdg: f64,
    pub fsl_fcu_alt: f64,
    pub fsl_fcu_vs: f64,
    pub fsl_fcu_spd_managed: f64,
    pub fsl_fcu_hdg_managed: f64,
    pub fsl_fcu_alt_managed: f64,
    pub fsl_fcu_spd_dashed: f64,
    pub fsl_fcu_hdg_dashed: f64,
    pub fsl_autobrake_lo_light: f64,
    pub fsl_autobrake_med_light: f64,
    pub fsl_autobrake_max_light: f64,
    // ---- FSLabs A321 PREMIUM (v0.16.20) ----
    pub fsl_park_brake_switch: f64,
    pub fsl_eng1_mstr_switch: f64,
    pub fsl_eng2_mstr_switch: f64,
    pub fsl_wheel_chocks: f64,
    pub fsl_baro_std: f64,
    pub fsl_master_caution: f64,
    pub fsl_master_warning: f64,
    pub fsl_spd_brk_lever: f64,
    pub fsl_engfire1_lt: f64,
    pub fsl_engfire2_lt: f64,
    pub fsl_eng1_anti_ice: f64,
    pub fsl_eng2_anti_ice: f64,
    pub fsl_xpdr_mode_switch: f64,
    // v0.16.20 (Review-Fund): XPDR ON/OFF-Schalter (0=OFF, 10=AUTO,
    // 20=ON per Skript-`/10`). Gated `xpdr_mode_label` auf OFF.
    pub fsl_xpdr_on_off_switch: f64,
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

        // Aerosoft A340-600 (ToLiss port) — gleiche TELEMETRY_FIELDS-
        // Reihenfolge wie am Tabellen-Ende registriert (Lockstep):
        // 4× COMBUSTION EX1 (bool), 4× TURB ENG CORRECTED FF (f64),
        // 5× AB_AP_*_LIGHT_ON LVars (f64).
        pull_i32!(t.eng1_combustion_ex1);
        pull_i32!(t.eng2_combustion_ex1);
        pull_i32!(t.eng3_combustion_ex1);
        pull_i32!(t.eng4_combustion_ex1);
        pull_f64!(t.eng1_ff_corrected_pph);
        pull_f64!(t.eng2_ff_corrected_pph);
        pull_f64!(t.eng3_ff_corrected_pph);
        pull_f64!(t.eng4_ff_corrected_pph);
        pull_f64!(t.a346_ap1_light);
        pull_f64!(t.a346_ap2_light);
        pull_f64!(t.a346_athr_light);
        pull_f64!(t.a346_appr_light);
        pull_f64!(t.a346_loc_light);

        // Aerosoft A346 full profile (v0.16.4) — gleiche TELEMETRY_
        // FIELDS-Reihenfolge wie am Tabellen-Ende registriert
        // (Lockstep): 2× Signs, 6× Anti-Ice, 2× BAT-OFF-Lampen,
        // Autobrake-Mode, Gear-Lever — alle f64.
        pull_f64!(t.a346_seatbelt_sw);
        pull_f64!(t.a346_no_smoking_sw);
        pull_f64!(t.a346_antiice_engl1);
        pull_f64!(t.a346_antiice_engl2);
        pull_f64!(t.a346_antiice_engr1);
        pull_f64!(t.a346_antiice_engr2);
        pull_f64!(t.a346_antiice_wing);
        pull_f64!(t.a346_antiice_probewindow);
        pull_f64!(t.a346_bat1_off_light);
        pull_f64!(t.a346_bat2_off_light);
        pull_f64!(t.a346_autobrake_mode);
        pull_f64!(t.a346_gear_lever);
        // ---- iniBuilds A350 (v0.16.8) ----
        pull_f64!(t.a350_ap1_on);
        pull_f64!(t.a350_ap2_on);
        pull_f64!(t.a350_athr_light);
        pull_f64!(t.a350_appr_light);
        pull_f64!(t.a350_loc_light);

        // ---- v0.16.10 (#Premium) — Lockstep mit dem Tabellen-Ende:
        // Gruppe A (13× Fenix), B (17× FBW), C (23× INI), D (12× A346),
        // E (14× MD-11), F (10× iFly, v0.16.11), G (17× FSLabs,
        // v0.16.14), alle f64.
        pull_f64!(t.fnx_perf_v1);
        pull_f64!(t.fnx_perf_vr);
        pull_f64!(t.fnx_perf_v2);
        pull_f64!(t.fnx_perf_flex);
        pull_f64!(t.fnx_master_caution);
        pull_f64!(t.fnx_master_warning);
        pull_f64!(t.fnx_speedbrake_handle);
        pull_f64!(t.fnx_fcu_spd_managed);
        pull_f64!(t.fnx_fcu_hdg_managed);
        pull_f64!(t.fnx_fcu_alt_managed);
        pull_f64!(t.fnx_baro_std);
        pull_f64!(t.fnx_eng1_fire);
        pull_f64!(t.fnx_eng2_fire);

        pull_f64!(t.fbw_ap1_active);
        pull_f64!(t.fbw_ap2_active);
        pull_f64!(t.fbw_athr_status);
        pull_f64!(t.fbw_athr_mode);
        pull_f64!(t.fbw_fma_lateral);
        pull_f64!(t.fbw_fma_vertical);
        pull_f64!(t.fbw_fwc_phase);
        pull_f64!(t.fbw_vspeeds_v2);
        pull_f64!(t.fbw_vspeeds_vls);
        pull_f64!(t.fbw_vspeeds_vapp);
        pull_f64!(t.fbw_autobrake_armed_mode);
        pull_f64!(t.fbw_flaps_handle_index);
        pull_f64!(t.fbw_spoilers_armed);
        pull_f64!(t.fbw_ground_spoilers_active);
        pull_f64!(t.fbw_fcu_spd_dot);
        pull_f64!(t.fbw_fcu_hdg_dot);
        pull_f64!(t.fbw_fcu_alt_managed);

        pull_f64!(t.ini_roll_mode);
        pull_f64!(t.ini_pitch_mode);
        pull_f64!(t.ini_throttle_mode);
        pull_f64!(t.ini_v1);
        pull_f64!(t.ini_vr);
        pull_f64!(t.ini_v2);
        pull_f64!(t.ini_vls);
        pull_f64!(t.ini_vapp);
        pull_f64!(t.ini_vref);
        pull_f64!(t.ini_flex_temp);
        pull_f64!(t.ini_lever_toga);
        pull_f64!(t.ini_lever_flex_mct);
        pull_f64!(t.ini_lever_cl);
        pull_f64!(t.ini_flaps_handle_index);
        pull_f64!(t.ini_ground_spoilers);
        pull_f64!(t.ini_autobrake_engaged);
        pull_f64!(t.ini_master_caution);
        pull_f64!(t.ini_master_warning);
        pull_f64!(t.ini_fuel_flow1_kg);
        pull_f64!(t.ini_fuel_flow2_kg);
        pull_f64!(t.ini_fuel_flow3_kg);
        pull_f64!(t.ini_fuel_flow4_kg);
        pull_f64!(t.ini_autobrake_level);

        pull_f64!(t.a346_flight_phase);
        pull_f64!(t.a346_fcu_spd_managed);
        pull_f64!(t.a346_fcu_hdg_managed);
        pull_f64!(t.a346_fcu_vs_managed);
        pull_f64!(t.a346_master_warning_light);
        pull_f64!(t.a346_master_caution_light);
        pull_f64!(t.a346_spd_brk_lever_pos);
        pull_f64!(t.a346_spoiler_lever_armed);
        pull_f64!(t.a346_eng1_rev_ratio);
        pull_f64!(t.a346_eng2_rev_ratio);
        pull_f64!(t.a346_eng3_rev_ratio);
        pull_f64!(t.a346_eng4_rev_ratio);

        pull_f64!(t.md11_ap_state);
        pull_f64!(t.md11_ats_state);
        pull_f64!(t.md11_ats_clamp);
        pull_f64!(t.md11_afs_spd);
        pull_f64!(t.md11_afs_hdg);
        pull_f64!(t.md11_afs_alt);
        pull_f64!(t.md11_afs_vs);
        pull_f64!(t.md11_v1);
        pull_f64!(t.md11_vr);
        pull_f64!(t.md11_v2);
        pull_f64!(t.md11_eng1_n1);
        pull_f64!(t.md11_eng2_n1);
        pull_f64!(t.md11_eng3_n1);
        pull_f64!(t.md11_autobrake_sw);

        pull_f64!(t.ifly_cmd_a_light);
        pull_f64!(t.ifly_cmd_b_light);
        pull_f64!(t.ifly_at_arm_light);
        pull_f64!(t.ifly_master_caution_light);
        pull_f64!(t.ifly_fire_warning_light);
        pull_f64!(t.ifly_cabin_alt_warning_light);
        pull_f64!(t.ifly_eng1_reverser);
        pull_f64!(t.ifly_eng2_reverser);
        pull_f64!(t.ifly_speedbrakes_extended_light);
        pull_f64!(t.ifly_autobrake_sw);

        pull_f64!(t.fsl_ap1_light);
        pull_f64!(t.fsl_ap2_light);
        pull_f64!(t.fsl_athr_light);
        pull_f64!(t.fsl_appr_light);
        pull_f64!(t.fsl_loc_light);
        pull_f64!(t.fsl_fcu_spd);
        pull_f64!(t.fsl_fcu_hdg);
        pull_f64!(t.fsl_fcu_alt);
        pull_f64!(t.fsl_fcu_vs);
        pull_f64!(t.fsl_fcu_spd_managed);
        pull_f64!(t.fsl_fcu_hdg_managed);
        pull_f64!(t.fsl_fcu_alt_managed);
        pull_f64!(t.fsl_fcu_spd_dashed);
        pull_f64!(t.fsl_fcu_hdg_dashed);
        pull_f64!(t.fsl_autobrake_lo_light);
        pull_f64!(t.fsl_autobrake_med_light);
        pull_f64!(t.fsl_autobrake_max_light);
        // ---- FSLabs A321 PREMIUM (v0.16.20) ----
        pull_f64!(t.fsl_park_brake_switch);
        pull_f64!(t.fsl_eng1_mstr_switch);
        pull_f64!(t.fsl_eng2_mstr_switch);
        pull_f64!(t.fsl_wheel_chocks);
        pull_f64!(t.fsl_baro_std);
        pull_f64!(t.fsl_master_caution);
        pull_f64!(t.fsl_master_warning);
        pull_f64!(t.fsl_spd_brk_lever);
        pull_f64!(t.fsl_engfire1_lt);
        pull_f64!(t.fsl_engfire2_lt);
        pull_f64!(t.fsl_eng1_anti_ice);
        pull_f64!(t.fsl_eng2_anti_ice);
        pull_f64!(t.fsl_xpdr_mode_switch);
        // v0.16.20 (Review-Fund): XPDR ON/OFF — neuer outermost-Slot.
        pull_f64!(t.fsl_xpdr_on_off_switch);

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

/// A346 (ToLiss-Port): Gear-Position aus dem SELECTOR LEVER ableiten.
///
/// Die Standard-SimVar `GEAR POSITION` klemmt bei der Aerosoft A346
/// dauerhaft auf "down" (dokumentierter v0.13.17-Aera-Befund) — das
/// Activity-Log zeigte nie "Gear UP" und der Stable-Config-Check sah
/// das Gear immer als ausgefahren. Der Hebel-LVar
/// `L:AB_MPL_LANDING_GEAR_SELECTOR_LEVER` ist die einzige brauchbare
/// Quelle.
///
/// Richtungs-ANNAHME (unverifiziert bis zum ersten Live-Flug):
/// 0 = Lever UP, != 0 = Lever DOWN — die uebliche Konvention solcher
/// Lever-LVars und konsistent damit, dass ein Cold&Dark-Spawn mit
/// uninitialisiertem LVar (0) zwar kurz "up" lesen wuerde, das Log
/// aber nur TRANSITIONEN loggt (der erste Wert latcht still).
/// Sollte der Live-Flug die Gegenrichtung zeigen, ist genau diese
/// eine Funktion zu invertieren.
///
/// Konsumenten von `gear_position` (alle display-/analysefeld-seitig,
/// NICHT score-kritisch — auditiert 2026-06-10):
///   * Activity-Log "Gear UP/DOWN" (lib.rs detect_telemetry_changes)
///   * Stable-Config `gear_ok` im Approach-Buffer (informatives
///     PIREP-/MQTT-Feld; fliesst NICHT in compute_sub_scores ein)
///   * MQTT-Live-Map-Payload (Anzeige)
fn a346_gear_position_from_lever(lever: f64) -> f32 {
    if lever != 0.0 {
        1.0 // down
    } else {
        0.0 // up
    }
}

/// f64-Pendant zu `positive_or_none` — fuer die Premium-V-Speeds und
/// FLEX-Temp: 0.0 heisst "noch nicht im FMS eingegeben" → None.
fn positive_f64_or_none(v: f64) -> Option<f64> {
    if v > 0.0 { Some(v) } else { None }
}

/// Roh-Enum-Durchreicher fuer Mode-/Phase-LVars mit UNBEKANNTER
/// Belegung (iniBuilds `INI_*_MODE_ACTIVE`, A346 `TLS_FLIGHT_PHASE`):
/// 0 → None (kein Mode / uninitialisiert), n → "#{n}". So liefert
/// EIN Live-Flug die Decode-Tabelle (Roh-Wert gegen das PFD ablesen),
/// ohne dass ein erfundenes Label im Log landet.
fn raw_enum_label(v: f64) -> Option<String> {
    let n = v.round() as i32;
    if n == 0 {
        None
    } else {
        Some(format!("#{n}"))
    }
}

/// FBW A32NX FMA lateral mode → PFD-Label. Quelle: FBW-Doku
/// (fbw-a32nx/docs/a320-simvars.md, `A32NX_FMA_LATERAL_MODE`).
/// 0 = kein Mode → None; unbekannte Enum-Werte werden als "#{n}"
/// durchgereicht statt verworfen (neue FBW-Versionen koennten das
/// Enum erweitern — der Roh-Wert bleibt dann analysierbar).
fn fbw_fma_lateral_label(n: i32) -> Option<String> {
    Some(match n {
        0 => return None,
        10 => "HDG".to_string(),
        11 => "TRACK".to_string(),
        20 => "NAV".to_string(),
        30 => "LOC*".to_string(),
        31 => "LOC".to_string(),
        32 => "LAND".to_string(),
        33 => "FLARE".to_string(),
        34 => "ROLLOUT".to_string(),
        40 => "RWY".to_string(),
        41 => "RWY TRK".to_string(),
        50 => "GA TRK".to_string(),
        other => format!("#{other}"),
    })
}

/// FBW A32NX FMA vertical mode → PFD-Label (FBW-Doku). Gleiche
/// "#{n}"-Fallback-Semantik wie lateral.
fn fbw_fma_vertical_label(n: i32) -> Option<String> {
    Some(match n {
        0 => return None,
        10 => "ALT".to_string(),
        11 => "ALT CST".to_string(),
        14 => "VS".to_string(),
        15 => "FPA".to_string(),
        20 => "OP CLB".to_string(),
        21 => "OP DES".to_string(),
        22 => "VS".to_string(),
        23 => "FPA".to_string(),
        30 => "SRS".to_string(),
        31 => "SRS GA".to_string(),
        40 => "CLB".to_string(),
        41 => "DES".to_string(),
        50 => "G/S*".to_string(),
        51 => "G/S".to_string(),
        52 => "LAND".to_string(),
        53 => "FLARE".to_string(),
        54 => "ROLLOUT".to_string(),
        other => format!("#{other}"),
    })
}

/// FBW `A32NX_AUTOTHRUST_MODE` → FMA-Thrust-Label (FBW-Doku).
fn fbw_fma_thrust_label(n: i32) -> Option<String> {
    Some(match n {
        0 => return None,
        1 => "MAN TOGA".to_string(),
        2 => "MAN GA SOFT".to_string(),
        3 => "MAN FLX".to_string(),
        4 => "MAN DTO".to_string(),
        5 => "MAN MCT".to_string(),
        6 => "MAN THR".to_string(),
        7 => "SPEED".to_string(),
        8 => "MACH".to_string(),
        9 => "THR MCT".to_string(),
        10 => "THR CLB".to_string(),
        11 => "THR LVR".to_string(),
        12 => "THR IDLE".to_string(),
        13 => "A.FLOOR".to_string(),
        14 => "TOGA LK".to_string(),
        other => format!("#{other}"),
    })
}

/// FBW `A32NX_FWC_FLIGHT_PHASE` → Phasen-Label (FBW-Doku, FWC-Enum
/// 1..10). 0 → None (FWC noch nicht initialisiert); unbekannte Werte
/// als "#{n}" durchgereicht.
fn fbw_fwc_phase_label(n: i32) -> Option<String> {
    Some(match n {
        0 => return None,
        1 => "ELEC PWR".to_string(),
        2 => "1ST ENG".to_string(),
        3 => "T/O 80KT".to_string(),
        4 => "LIFTOFF".to_string(),
        5 => "CLIMB <1500".to_string(),
        6 => "CRUISE".to_string(),
        7 => "DESCENT".to_string(),
        8 => "APPROACH".to_string(),
        9 => "TOUCHDOWN".to_string(),
        10 => "ROLLOUT <80KT".to_string(),
        other => format!("#{other}"),
    })
}

fn telemetry_to_snapshot(t: Telemetry, simulator: Simulator) -> SimSnapshot {
    let profile = AircraftProfile::detect(&t.title, &t.atc_model);
    let is_fenix = profile.is_fenix();
    let is_fbw = matches!(profile, AircraftProfile::FbwA32nx);
    // v0.16.4: A346-Komfort-LVars (Signs, Anti-Ice, BAT, Autobrake,
    // Gear-Lever, A/THR) sind ausschliesslich unter diesem Gate
    // gemappt — Nicht-A346-Aircraft bleiben byte-fuer-byte auf dem
    // Status quo (siehe a346_full_profile_does_not_affect_other_
    // profiles-Test).
    let is_a346 = matches!(profile, AircraftProfile::AerosoftA346);
    let is_a350 = matches!(profile, AircraftProfile::IniA350);
    // v0.16.10 (#Premium): zusaetzliche Profile-Gates. Die INI_-LVars
    // teilen sich A350 und A340 (gleiche Namen laut Inventar), daher
    // das kombinierte `is_ini`-Gate fuer Gruppe C.
    let is_a340 = matches!(profile, AircraftProfile::IniA340);
    let is_md11 = matches!(profile, AircraftProfile::TfdiMd11);
    // v0.16.11: iFly 737 MAX 8 — `L:VC_*_VAL`-Annunciator-LVars
    // (WASM-strings + HubHop), strikt profile-gegated wie alle
    // Premium-Gruppen.
    let is_ifly = matches!(profile, AircraftProfile::IflyMax8);
    // v0.16.14: FSLabs A321 (ceo + neo) — HubHop-Output-Presets,
    // strikt profile-gegated. Master Caution/Warning, Reverser- und
    // Engine-LVars sind fuer FSL extern, nicht katalogisiert —
    // bleiben ehrlich None bzw. auf den Standard-SimVars + Kaskaden
    // (EX1/N1-Fallbacks greifen addon-agnostisch).
    let is_fsl = matches!(profile, AircraftProfile::FsLabsA321);
    // FSL-LED-Schwelle: die `_Brt_Lt`-LVars tragen LED-HELLIGKEIT,
    // kein 0/1-Flag — HubHop-Button-Presets pruefen ">50", wir werten
    // konservativer > 10 als "leuchtet" (faengt gedimmte Cockpits;
    // Live-Verifikation beim ersten FSL-Flug).
    const FSL_LED_LIT: f64 = 10.0;
    let is_ini = is_a350 || is_a340;
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
    // v0.16.20: FSLabs FAELSCHT `GENERAL ENG COMBUSTION` → der Standard-
    // Pfad las am Gate (Triebwerke aus) konstant 2 (Peters Log, 0.16.18).
    // Die echten Engine-Master-Schalter sind die wahre On/Off-Quelle.
    // Skript-Schwellen: `20 >=` = ON, `10 <=` = OFF → Stellung, kein 0/1;
    // Mitte (>= 15) trennt sauber. Master AUS = Shutdown = das echte
    // Ankunftssignal, das die Auto-End-FSM braucht.
    const FSL_MSTR_ON: f64 = 15.0;
    let engines_running = if is_fsl {
        (if t.fsl_eng1_mstr_switch >= FSL_MSTR_ON { 1u8 } else { 0 })
            + (if t.fsl_eng2_mstr_switch >= FSL_MSTR_ON { 1u8 } else { 0 })
    } else if is_fsr_phenom {
        (if t.fsr_phenom_eng1_knob > 0.5 { 1u8 } else { 0 })
            + (if t.fsr_phenom_eng2_knob > 0.5 { 1u8 } else { 0 })
    } else {
        // 2026-06-10: per Engine plain Combustion ODER die EX1-Variante.
        // Die Aerosoft A346 (ToLiss-Port) treibt laut WASM-Strings-
        // Analyse NUR `GENERAL ENG COMBUSTION EX1:N` — die plain SimVar
        // bleibt 0 (Root Cause des v0.13.17-Befunds). EX1 liest auf
        // Addons, die sie nicht treiben, schlicht false → das ODER ist
        // addon-agnostisch sicher, kein Profile-Gate noetig.
        let combustion = ((t.eng1_firing || t.eng1_combustion_ex1) as u8)
            + ((t.eng2_firing || t.eng2_combustion_ex1) as u8)
            + ((t.eng3_firing || t.eng3_combustion_ex1) as u8)
            + ((t.eng4_firing || t.eng4_combustion_ex1) as u8);
        if combustion > 0 {
            combustion
        } else {
            // v0.13.17: `GENERAL ENG COMBUSTION` ist bei manchen Addons
            // (iniBuilds/Aerosoft A340-600, MSFS 2024) konstant 0 obwohl
            // die Triebwerke laufen → Phase-FSM blieb in Pushback haengen
            // (Live IRM1140/IBE778). Root Cause fuer die Aerosoft A346
            // ist inzwischen bestaetigt (EX1-Variante, siehe oben) und
            // nativ abgedeckt; der N1-Fallback bleibt als letzte Stufe
            // fuer Addons, die WEDER plain NOCH EX1 treiben. Fallback:
            // N1 ueber Idle/Windmill-Schwelle = Triebwerk laeuft. N1
            // kommt je nach Addon als 0-1-Ratio ODER 0-100 % → auf
            // Prozent normalisieren. Greift NUR wenn COMBUSTION (incl.
            // EX1) komplett 0 ist → kein Regress fuer Flieger, deren
            // COMBUSTION-Flag funktioniert (dort ist N1 ohnehin 0 wenn
            // aus). Schwelle bewusst ueber reinem Windmilling (~15 %);
            // am Boden (wo die FSM das Signal braucht) gibt es kein
            // Windmilling, also trennt es dort sauber aus(0) vs laufend.
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
    //
    // 2026-06-10: Kaskade statt Single-Source. Die Aerosoft A346
    // (ToLiss-Port) treibt laut WASM-Strings-Analyse NUR die Variante
    // `TURB ENG CORRECTED FF:N` — `ENG FUEL FLOW PPH` bleibt 0 (Root
    // Cause der toten Fuel-Flow-Anzeige, wegen der v0.13.18 die FOB-
    // Ableitung einfuehrte). Reihenfolge: PPH-Summe > 0 gewinnt (kein
    // Regress), sonst CORRECTED-FF-Summe (gleiche pounds-per-hour-
    // Einheit, gleiche Konversion), sonst None — dann greift weiterhin
    // die v0.13.18-FOB-Ableitung im Position-Streamer als letzte Stufe.
    // Addon-agnostisch: Aircraft ohne CORRECTED FF lesen dort 0.
    let total_ff_pph = t.eng1_ff_pph + t.eng2_ff_pph + t.eng3_ff_pph + t.eng4_ff_pph;
    let total_ff_corrected_pph = t.eng1_ff_corrected_pph
        + t.eng2_ff_corrected_pph
        + t.eng3_ff_corrected_pph
        + t.eng4_ff_corrected_pph;
    let fuel_flow_kg_per_h = if total_ff_pph > 0.0 {
        Some((total_ff_pph * KG_PER_LB) as f32)
    } else if total_ff_corrected_pph > 0.0 {
        Some((total_ff_corrected_pph * KG_PER_LB) as f32)
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
    //   * Aerosoft A346 (ToLiss port): `L:AB_AP_*_LIGHT_ON` annunciator
    //     LVars — per WASM strings analysis (2026-06-10) the ONLY AP
    //     state source on that aircraft, the standard SimVars stay dead.
    //   * Default + others: standard MSFS SimVars.
    let (ap_master, ap_hdg, ap_alt, ap_nav, ap_appr) = if is_fbw {
        (
            // v0.16.10 (#Premium): AP1/AP2 einzeln (FBW-Doku
            // `A32NX_AUTOPILOT_{1,2}_ACTIVE`) ODER der kombinierte
            // Active-LVar ODER der Standard-SimVar — jede Quelle
            // genuegt, keine Regression gegen den bisherigen Pfad.
            t.fbw_ap_active != 0.0
                || t.fbw_ap1_active != 0.0
                || t.fbw_ap2_active != 0.0
                || t.ap_master,
            // v0.16.10 QS (M4) Defense-in-Depth: Sub-Modes ebenfalls
            // ODER Standard-SimVar. Faengt einen marker-losen Nicht-
            // FBW-A339, der per ICAO-Fallback hier landet: dessen
            // A32NX_-LVars sind tot (0.0) — ohne das ODER waeren
            // HDG/ALT/NAV/APPR permanent false, obwohl die Standard-
            // SimVars die echten Modes liefern. Auf echten FBW kostet
            // das nichts (deren Standard-SimVars sind konsistent oder
            // tot-false — false veraendert ein ODER nie).
            t.fbw_ap_hdg != 0.0 || t.ap_heading,
            t.fbw_ap_alt != 0.0 || t.ap_altitude,
            t.fbw_ap_nav != 0.0 || t.ap_nav,
            t.fbw_ap_appr != 0.0 || t.ap_approach,
        )
    } else if is_md11 {
        // TFDi MD-11 (v0.16.10 #Premium, TFDi-Doku): `L:MD11_AP_STATE`
        // ist dokumentiert 0=Off, 1=AP1, 2=AP2, 3=both → jeder Wert
        // > 0 heisst Master engaged. Standard-SimVar gewinnt per ODER,
        // falls das Addon ihn doch treibt. HDG/ALT/NAV/APPR bleiben
        // konservativ auf den Standard-SimVars.
        (
            t.md11_ap_state > 0.5 || t.ap_master,
            t.ap_heading,
            t.ap_altitude,
            t.ap_nav,
            t.ap_approach,
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
    } else if is_a346 {
        // Aerosoft A346: AP1- oder AP2-Lampe an = Master engaged;
        // Approach-Mode aus APPR- oder LOC-Lampe (LOC = lateraler
        // Approach-Capture ohne Glideslope). Die Lampen-LVars sind
        // echte Annunciator-States (kein Button-Pulse wie Fenix
        // `S_FCU_*`). HDG/ALT/NAV bleiben auf den Standard-SimVars —
        // konservativ, nur das verifiziert Vorhandene mappen. Falls
        // der Standard doch mal wired ist, gewinnt er per ODER.
        (
            t.a346_ap1_light as i32 != 0
                || t.a346_ap2_light as i32 != 0
                || t.ap_master,
            t.ap_heading,
            t.ap_altitude,
            t.ap_nav,
            t.a346_appr_light as i32 != 0
                || t.a346_loc_light as i32 != 0
                || t.ap_approach,
        )
    } else if is_a350 {
        // iniBuilds A350 (v0.16.8): FCU-LED-LVars aus der HubHop-DB —
        // INI_ap1_on/INI_ap2_on sind die AP1/AP2-LEDs (engaged),
        // APPR/LOC analog zur A346-Semantik. HDG/ALT/NAV konservativ
        // auf den Standard-SimVars; Standard gewinnt per ODER.
        (
            t.a350_ap1_on as i32 != 0
                || t.a350_ap2_on as i32 != 0
                || t.ap_master,
            t.ap_heading,
            t.ap_altitude,
            t.ap_nav,
            t.a350_appr_light as i32 != 0
                || t.a350_loc_light as i32 != 0
                || t.ap_approach,
        )
    } else if is_ifly {
        // iFly 737 MAX 8 (v0.16.11): CMD-A-/CMD-B-LEDs am MCP
        // (`L:VC_CMD_{A,B}_SW_LIGHT_VAL`, HubHop-Output) — echte
        // Annunciator-States wie die PMDG-CMD-Lampen. Eine der beiden
        // an = Master engaged; Standard-SimVar gewinnt per ODER, falls
        // das Addon ihn doch treibt. AP-Sub-Modes (HDG/ALT/NAV/APPR)
        // bleiben konservativ auf den Standard-SimVars — auf dem iFly
        // ungetestet, nicht raten.
        (
            t.ifly_cmd_a_light != 0.0
                || t.ifly_cmd_b_light != 0.0
                || t.ap_master,
            t.ap_heading,
            t.ap_altitude,
            t.ap_nav,
            t.ap_approach,
        )
    } else if is_fsl {
        // FSLabs A321 (v0.16.14): FCU-LED-Helligkeits-LVars
        // (`L:VC_GSLD_FCU_*_Brt_Lt`, HubHop-Output-Presets) — > 10
        // (FSL_LED_LIT) = LED leuchtet. AP1 ODER AP2 an = Master
        // engaged; APPR ODER LOC = Approach-Mode (LOC = lateraler
        // Approach-Capture ohne Glideslope, gleiche Semantik wie
        // A346/A350). Standard-SimVar gewinnt jeweils per ODER, falls
        // das Addon ihn doch treibt. HDG/ALT/NAV bleiben konservativ
        // auf den Standard-SimVars — fuer FSL nicht katalogisiert,
        // nicht raten.
        (
            t.fsl_ap1_light > FSL_LED_LIT
                || t.fsl_ap2_light > FSL_LED_LIT
                || t.ap_master,
            t.ap_heading,
            t.ap_altitude,
            t.ap_nav,
            t.fsl_appr_light > FSL_LED_LIT
                || t.fsl_loc_light > FSL_LED_LIT
                || t.ap_approach,
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
    } else if is_fsl {
        // v0.16.20: FSL faelscht den Standard-Parkbremsen-SimVar (Peters
        // Log: parking_brake=False bei real gesetzter Bremse). Skript
        // `(L:VC_PED_PARK_BRAKE_Switch) 0 == if{ released }` → !=0 = SET.
        t.fsl_park_brake_switch != 0.0
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
    } else if is_a346 {
        // A346 `L:AB_OVH_ANTIICE_PROBEWINDOW`: gleiche Airbus-Semantik
        // wie der Fenix PROBE/WINDOW-HEAT-Pushbutton (0=AUTO, 1=ON) —
        // AUTO heizt automatisch sobald Triebwerke laufen, also wie
        // beim Fenix beide Stellungen als "heat available" werten.
        t.a346_antiice_probewindow >= 0.0
    } else {
        t.pitot_heat
    };
    let battery_master = if is_fenix {
        // BAT 1 OR BAT 2 in AUTO/ON position counts as "battery on".
        // 0=OFF, 1=AUTO on real Airbus.
        t.fnx_bat1 as i32 != 0 || t.fnx_bat2 as i32 != 0
    } else if is_a346 {
        // INVERTIERT: `L:AB_VC_OVH_ELEC_BAT{1,2}_OFF` sind die weissen
        // "OFF"-ANNUNCIATOR-LAMPEN der BAT-Pushbuttons (WASM-Strings-
        // Block der *_OFF/*_FAULT-Lampen-Legends — die *_OFF_PB-
        // Schalter-LVars existieren separat, deren Wertesemantik ist
        // aber unbekannt). Lampe AN (1) = Batterie AUS; Lampe AUS (0)
        // = Batterie AN — wie beim realen Airbus, wo die OFF-Lampe
        // vom Battery-Hot-Bus selbst gespeist auch Cold&Dark leuchtet.
        // "Battery master on" im Fenix-Sinn (mind. eine Batterie an):
        // mindestens eine OFF-Lampe ist NICHT erleuchtet.
        // Caveat (Live-Flug-Verifikation aussteht): vor der WASM-
        // Initialisierung lesen beide LVars 0 → kurzzeitig "an"; das
        // Log latcht den ersten Wert still und loggt nur Transitionen.
        t.a346_bat1_off_light as i32 == 0 || t.a346_bat2_off_light as i32 == 0
    } else {
        t.battery_master
    };
    let engine_anti_ice = if is_fenix {
        t.fnx_eng1_anti_ice as i32 != 0 || t.fnx_eng2_anti_ice as i32 != 0
    } else if is_fsl {
        // v0.16.20: `VC_OVHD_AI_Eng_{1,2}_Anti_Ice_Button` — Skript
        // `0 !=`=an. Kombiniertes "irgendein Triebwerk Anti-Ice an".
        t.fsl_eng1_anti_ice != 0.0 || t.fsl_eng2_anti_ice != 0.0
    } else if is_a346 {
        // 4 Engine-Anti-Ice-Schalter (L1/L2/R1/R2). Der Snapshot
        // fuehrt nur das kombinierte "any engine anti-ice on" — wie
        // der Standard-SimVar-Pfad (eng1..eng4 geODERt), also alle
        // vier Schalter ODERn.
        t.a346_antiice_engl1 as i32 != 0
            || t.a346_antiice_engl2 as i32 != 0
            || t.a346_antiice_engr1 as i32 != 0
            || t.a346_antiice_engr2 as i32 != 0
    } else {
        t.eng1_anti_ice || t.eng2_anti_ice || t.eng3_anti_ice || t.eng4_anti_ice
    };
    let wing_anti_ice = if is_fenix {
        t.fnx_wing_anti_ice as i32 != 0
    } else if is_a346 {
        t.a346_antiice_wing as i32 != 0
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
    } else if is_a346 {
        // `L:AB_OVH_SEATBELT`: Overhead-Schalterposition. Der reale
        // A340-Schalter ist 3-stufig (OFF/AUTO/ON) — wir clampen auf
        // den Feld-Kontrakt 0..=2 (deckt auch einen binaeren 0/1-LVar
        // identisch ab). Das Log wertet jede Nicht-Null als "ON";
        // exakter Wertebereich braucht Live-Flug-Verifikation.
        Some(t.a346_seatbelt_sw.round().clamp(0.0, 2.0) as u8)
    } else {
        None
    };
    let no_smoking_sign = if is_fenix {
        Some(t.fnx_signs_smoking.round().clamp(0.0, 2.0) as u8)
    } else if is_a346 {
        // `L:AB_OVH_NO_SMOKING`: wie Fenix' SMOKING-Sign auf 0..=2
        // geclamped (OFF/AUTO/ON-Labels im Activity-Log).
        Some(t.a346_no_smoking_sw.round().clamp(0.0, 2.0) as u8)
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
    } else if is_md11 {
        // TFDi MD-11 (v0.16.10 #Premium): AFS-Glareshield-Targets aus
        // `L:MD11_AFS_*`. Dash-Sentinels laut Doku: SPD/HDG zeigen
        // -999 wenn das Fenster "---" (dashed/managed) anzeigt, V/S
        // -9999. Wir filtern <= Sentinel-Schwelle (faengt auch noch
        // negativere Varianten ab). ALT kennt kein Dash — nur > 0 ist
        // plausibel (0 = LVar uninitialisiert).
        (
            if t.md11_afs_alt > 0.0 {
                Some(t.md11_afs_alt.round() as i32)
            } else {
                None
            },
            // v0.16.10 QS (Minor 8): zusaetzlich Plausibilitaets-Gates —
            // ein uninitialisierter LVar liest 0.0 und liegt damit
            // ZWISCHEN den Dash-Sentinels und echten Werten. HDG-Fenster
            // zeigt 1-360, SPD-Fenster nie unter ~100 kt → HDG nur in
            // 1..=360, SPD nur >= 80 mappen.
            {
                let hdg = t.md11_afs_hdg.round() as i32;
                if t.md11_afs_hdg <= -998.0 || !(1..=360).contains(&hdg) {
                    None
                } else {
                    Some(hdg)
                }
            },
            {
                let spd = t.md11_afs_spd.round() as i32;
                if t.md11_afs_spd <= -998.0 || spd < 80 {
                    None
                } else {
                    Some(spd)
                }
            },
            if t.md11_afs_vs <= -9998.0 {
                None
            } else {
                Some(t.md11_afs_vs.round() as i32)
            },
        )
    } else if is_fsl {
        // FSLabs A321 (v0.16.14): FCU-Fenster-Werte aus `L:FSL_FCU_*`
        // (HubHop). SPD/HDG haben dokumentierte `*_DASHED`-Flags:
        // Fenster zeigt "---" (managed) → None statt eines gestalten
        // Restwerts. ALT kennt kein Dash — nur > 0 ist plausibel
        // (0 = LVar uninitialisiert). `FSL_FCU_VS` traegt je nach
        // FCU-Modus V/S (fpm) ODER FPA (Grad) — wir mappen den
        // Roh-Wert ohne Umrechnung; ein dediziertes VS-Dashed-Flag
        // ist nicht katalogisiert → nur != 0 mappen (0 = dashed/
        // Level-off/uninitialisiert, besser ehrlich None als ein
        // Phantom-"V/S 0").
        (
            if t.fsl_fcu_alt > 0.0 {
                Some(t.fsl_fcu_alt.round() as i32)
            } else {
                None
            },
            if t.fsl_fcu_hdg_dashed == 0.0 && t.fsl_fcu_hdg > 0.0 {
                Some(t.fsl_fcu_hdg.round() as i32)
            } else {
                None
            },
            if t.fsl_fcu_spd_dashed == 0.0 && t.fsl_fcu_spd > 0.0 {
                Some(t.fsl_fcu_spd.round() as i32)
            } else {
                None
            },
            if t.fsl_fcu_vs != 0.0 {
                Some(t.fsl_fcu_vs.round() as i32)
            } else {
                None
            },
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
    } else if is_a346 {
        // `L:AB_AutoBrake_Mode` ist ein Modus-Enum (ein LVar statt der
        // drei Fenix-Lampen). Konservative Annahme entlang der realen
        // A340-Stufen: 0=OFF, 1=LO, 2=MED, 3=MAX — auf dieselben
        // Labels gemappt wie beim Fenix. Werte ausserhalb 0..=3 →
        // None ("wissen wir nicht"), damit kein erfundenes Label im
        // Log landet. Enum-Belegung braucht Live-Flug-Verifikation.
        match t.a346_autobrake_mode.round() as i32 {
            0 => Some("OFF".to_string()),
            1 => Some("LO".to_string()),
            2 => Some("MED".to_string()),
            3 => Some("MAX".to_string()),
            _ => None,
        }
    } else if is_fbw {
        // v0.16.10 (#Premium): FBW `A32NX_AUTOBRAKES_ARMED_MODE`
        // (FBW-Doku): 0=DIS, 1=LO, 2=MED, 3=MAX. 0 behaelt die
        // bisherige None-Semantik des Profils (kein erfundenes
        // "OFF"-Label); unbekannte Werte ebenfalls None.
        match t.fbw_autobrake_armed_mode.round() as i32 {
            1 => Some("LO".to_string()),
            2 => Some("MED".to_string()),
            3 => Some("MAX".to_string()),
            _ => None,
        }
    } else if is_a340 {
        // v0.16.10 (#Premium): iniBuilds A340 `L:INI_AUTOBRAKE_LEVEL`
        // — Selector-Enum laut HubHop: 3=MED, 4=MAX, 5=LO. 0 =
        // uninitialisiert/aus → None; andere Werte als "#{n}"
        // durchreichen (Decode beim ersten Live-Flug, vermutlich
        // 1/2 = OFF/DISARM-Stellungen).
        match t.ini_autobrake_level.round() as i32 {
            0 => None,
            3 => Some("MED".to_string()),
            4 => Some("MAX".to_string()),
            5 => Some("LO".to_string()),
            n => Some(format!("#{n}")),
        }
    } else if is_md11 {
        // v0.16.10 (#Premium): TFDi MD-11 `L:MD11_CTR_AUTOBRAKE_SW` —
        // Selector-Positions-Enum, Belegung undokumentiert → Roh-Wert
        // "#{n}" fuer n != 0 (Decode beim ersten Live-Flug).
        raw_enum_label(t.md11_autobrake_sw)
    } else if is_ifly {
        // v0.16.11: iFly 737 MAX 8 `L:VC_Autobrake_SW_VAL` —
        // Selektor-Enum unbekannt → "#{n}" wie beim MD-11 (Decode
        // beim ersten Live-Flug; der reale MAX-Selektor kennt
        // RTO/OFF/1/2/3/MAX).
        raw_enum_label(t.ifly_autobrake_sw)
    } else if is_fsl {
        // FSLabs A321: die drei AUTO-BRK-Tasten-LVars
        // (`L:VC_MIP_BRAKES_AUTOBRK_{LO,MED,MAX}_Button_BOT`,
        // BOT = untere Tastenhaelfte = Mode selected). Review-Fund
        // v0.16.20: das FSLabsA3x_Scripts.xml behandelt sie als STATE
        // (`(L:...) 0 != if{ ... selected }`), NICHT als Helligkeit —
        // also `!= 0` statt der frueheren `>50`-Schwelle (gleiches
        // Idiom wie Parkbremse / Anti-Ice / Master-Caution beim FSL).
        // Real leuchtet genau eine; sollten (theoretisch) mehrere
        // gleichzeitig lesen, gewinnt die erste (LO → MED → MAX).
        // Keine selektiert → None ("wissen wir nicht"), kein erfundenes
        // OFF-Label.
        if t.fsl_autobrake_lo_light != 0.0 {
            Some("LO".to_string())
        } else if t.fsl_autobrake_med_light != 0.0 {
            Some("MED".to_string())
        } else if t.fsl_autobrake_max_light != 0.0 {
            Some("MAX".to_string())
        } else {
            None
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

    // Gear: nur die A346 leitet aus dem Selector-Lever ab (die
    // Standard-SimVar klemmt dort auf "down", v0.13.17-Befund) —
    // alle anderen Profile bleiben auf `GEAR POSITION`. Siehe
    // `a346_gear_position_from_lever` fuer Richtungs-Annahme und
    // Konsumenten-Audit.
    let gear_position = if is_a346 {
        a346_gear_position_from_lever(t.a346_gear_lever)
    } else {
        t.gear_position as f32
    };

    // A/THR (v0.16.4): nur Profile mit verifizierter State-Quelle.
    //   * A346: `L:AB_AP_ATHR_LIGHT_ON` — FCU-Annunciator-Lampe,
    //     echter Engagement-State (seit v0.16.3 subscribed, bislang
    //     ungemappt).
    //   * Fenix: `L:S_FCU_ATHR` — Button-State-LVar. Achtung: die
    //     Schwester-LVars S_FCU_AP1/AP2 erwiesen sich als Button-
    //     PULSE (B-008); ob ATHR latcht oder pulst braucht Live-
    //     Verifikation. Der A/THR-Log-Eintrag ist deshalb wie der
    //     AP-Master debounced (AP_DEBOUNCE_SECS) — ein Puls fuehrt
    //     dann schlimmstenfalls zu gar keinem Eintrag, nie zu Spam.
    //   * Alle anderen (+ X-Plane): None → kein Log-Eintrag.
    let autothrottle_on = if is_a346 {
        Some(t.a346_athr_light as i32 != 0)
    } else if is_a350 {
        // iniBuilds A350: A/THR-LED (INI_ATHR_LIGHT, HubHop-Output-Preset).
        Some(t.a350_athr_light as i32 != 0)
    } else if is_fenix {
        Some(t.fnx_fcu_athr as i32 != 0)
    } else if is_fbw {
        // v0.16.10 (#Premium): FBW `A32NX_AUTOTHRUST_STATUS` (FBW-Doku):
        // 0=off, 1=armed, 2=active. "On" heisst hier AKTIV (>= 2) —
        // armed (1) ist der TOGA-Takeoff-Zustand, in dem die Lever
        // noch manuell stehen; den als "A/THR on" zu loggen waere
        // irrefuehrend.
        //
        // v0.16.10 QS (M4) Defense-in-Depth: Status 0 → None statt
        // Some(false). Auf einem marker-losen Nicht-FBW-A339 (ICAO-
        // Fallback) ist der LVar tot (liest permanent 0.0) — ein
        // hartes Some(false) waere dort ein Phantom-"A/THR OFF" den
        // ganzen Flug. Trade-off: ein echter FBW mit wirklich
        // abgeschaltetem A/THR meldet ebenfalls None (kein OFF-Log) —
        // akzeptiert, None erzeugt nie Spam.
        if t.fbw_athr_status > 0.5 {
            Some(t.fbw_athr_status >= 2.0)
        } else {
            None
        }
    } else if is_md11 {
        // v0.16.10 (#Premium): TFDi MD-11 `L:MD11_ATS_STATE` — Enum
        // undokumentiert, > 0 = ATS engaged (TFDi-Doku-Konvention).
        Some(t.md11_ats_state > 0.5)
    } else if is_ifly {
        // v0.16.11: iFly 737 MAX 8 `L:VC_AT_ARM_LIGHT_VAL` — die
        // A/T-ARM-Lampe am MCP. 737-Semantik (dokumentiert identisch
        // zum PMDG-NG3-AT-ARM, den der pmdg-Mapper als `at_armed`
        // fuehrt): Lampe an = A/T armed ODER engaged, geht nach
        // Disconnect aus. "Armed" als "on" zu werten ist hier die
        // 737-Konvention — anders als beim FBW-AUTOTHRUST_STATUS gibt
        // es keine getrennte Engaged-Quelle.
        Some(t.ifly_at_arm_light != 0.0)
    } else if is_fsl {
        // FSLabs A321 (v0.16.14): A/THR-LED am FCU
        // (`L:VC_GSLD_FCU_ATHR_Brt_Lt`, HubHop-Output) — Helligkeits-
        // LVar, > 10 (FSL_LED_LIT) = leuchtet. Airbus-FCU-Semantik:
        // LED an = A/THR armed ODER active (wie die echte ATHR-Taste);
        // eine getrennte Engaged-Quelle ist nicht katalogisiert.
        Some(t.fsl_athr_light > FSL_LED_LIT)
    } else {
        None
    };

    // ================================================================
    // v0.16.10 (#Premium): Cockpit-Tiefendaten-Mappings.
    // Alle Zweige strikt profile-gegated — tote LVars lesen auf
    // fremden Aircraft 0.0, ohne Gate entstuenden Phantom-Werte
    // (siehe premium_lvars_do_not_affect_default_profile-Test).
    // PMDG-Premium fliesst NICHT hier, sondern ueber den `pmdg`-
    // Struct-Merge in sim-core (`apply_pmdg_premium_override`).
    // ================================================================

    // FMA-Spalten. FBW: dokumentierte Enums → PFD-Labels. INI: Enum-
    // Belegung unbekannt → Roh-Wert "#{n}" (Decode beim Live-Flug).
    let fma_lateral_mode = if is_fbw {
        fbw_fma_lateral_label(t.fbw_fma_lateral.round() as i32)
    } else if is_ini {
        raw_enum_label(t.ini_roll_mode)
    } else {
        None
    };
    let fma_vertical_mode = if is_fbw {
        fbw_fma_vertical_label(t.fbw_fma_vertical.round() as i32)
    } else if is_ini {
        raw_enum_label(t.ini_pitch_mode)
    } else {
        None
    };
    let fma_thrust_mode = if is_fbw {
        fbw_fma_thrust_label(t.fbw_athr_mode.round() as i32)
    } else if is_ini {
        raw_enum_label(t.ini_throttle_mode)
    } else {
        None
    };

    // Aircraft-eigene Flugphase. FBW: FWC-Enum dokumentiert. A346:
    // FMGC-Phase-Enum, Decode beim ersten Live-Flug → "#{n}".
    let flight_phase_aircraft = if is_fbw {
        fbw_fwc_phase_label(t.fbw_fwc_phase.round() as i32)
    } else if is_a346 {
        raw_enum_label(t.a346_flight_phase)
    } else {
        None
    };

    // V-Speeds + FLEX: 0 = noch nicht im FMS eingegeben → None.
    let v1_kt = if is_fenix {
        positive_f64_or_none(t.fnx_perf_v1)
    } else if is_ini {
        positive_f64_or_none(t.ini_v1)
    } else if is_md11 {
        positive_f64_or_none(t.md11_v1)
    } else {
        None
    };
    let vr_kt = if is_fenix {
        positive_f64_or_none(t.fnx_perf_vr)
    } else if is_ini {
        positive_f64_or_none(t.ini_vr)
    } else if is_md11 {
        positive_f64_or_none(t.md11_vr)
    } else {
        None
    };
    let v2_kt = if is_fenix {
        positive_f64_or_none(t.fnx_perf_v2)
    } else if is_fbw {
        positive_f64_or_none(t.fbw_vspeeds_v2)
    } else if is_ini {
        positive_f64_or_none(t.ini_v2)
    } else if is_md11 {
        positive_f64_or_none(t.md11_v2)
    } else {
        None
    };
    let vapp_kt = if is_fbw {
        positive_f64_or_none(t.fbw_vspeeds_vapp)
    } else if is_ini {
        positive_f64_or_none(t.ini_vapp)
    } else {
        None
    };
    let vls_kt = if is_fbw {
        positive_f64_or_none(t.fbw_vspeeds_vls)
    } else if is_ini {
        positive_f64_or_none(t.ini_vls)
    } else {
        None
    };
    // VREF liefert nur die INI-Familie (PMDG via pmdg-Merge).
    let vref_kt = if is_ini {
        positive_f64_or_none(t.ini_vref)
    } else {
        None
    };
    let flex_temp_c = if is_fenix {
        // > 0 heisst FLEX eingegeben; das Gate-Label (thrust_gate)
        // bleibt beim Fenix trotzdem None — nur die INI-Familie
        // liefert die Lever-Gate-Flags.
        positive_f64_or_none(t.fnx_perf_flex)
    } else if is_ini {
        positive_f64_or_none(t.ini_flex_temp)
    } else {
        None
    };

    // Thrust-Lever-Gate (nur INI): Prioritaet TOGA > FLX/MCT > CL —
    // bei (theoretisch) mehreren gesetzten Flags gewinnt das hoechste.
    let thrust_gate = if is_ini {
        if t.ini_lever_toga != 0.0 {
            Some("TOGA".to_string())
        } else if t.ini_lever_flex_mct != 0.0 {
            Some("FLX/MCT".to_string())
        } else if t.ini_lever_cl != 0.0 {
            Some("CL".to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Master Caution / Warning — echte Annunciator-Lampen-LVars.
    // (Fenix-Engine-Fire ist subscribed, aber bewusst ungemappt: ein
    // Fire zieht ohnehin das Master Warning.)
    let master_caution = if is_fenix {
        Some(t.fnx_master_caution != 0.0)
    } else if is_ini {
        Some(t.ini_master_caution != 0.0)
    } else if is_a346 {
        Some(t.a346_master_caution_light != 0.0)
    } else if is_ifly {
        // v0.16.11: iFly `L:VC_Master_Caution_Light_1_VAL` (Capt-Seite).
        Some(t.ifly_master_caution_light != 0.0)
    } else if is_fsl {
        // v0.16.20: `VC_GSLD_CP_Caution_Button_BOT` — Skript `0 !=`=aktiv.
        Some(t.fsl_master_caution != 0.0)
    } else {
        None
    };
    let master_warning = if is_fenix {
        Some(t.fnx_master_warning != 0.0)
    } else if is_ini {
        Some(t.ini_master_warning != 0.0)
    } else if is_a346 {
        Some(t.a346_master_warning_light != 0.0)
    } else if is_ifly {
        // v0.16.11: die 737 hat KEINE MASTER-WARNING-Lampe — die rote
        // Master-Klasse ist die FIRE-WARN-Lampe (`L:VC_Fire_Warning_
        // Light_1_VAL`, Capt-Seite). Gleiche Konvention wie der
        // PMDG-Mapper (dort speist fire_warn → master_warning).
        Some(t.ifly_fire_warning_light != 0.0)
    } else if is_fsl {
        // v0.16.20: `VC_GSLD_CP_Warning_Button_BOT` (rote Master-Lampe,
        // Skript `0 !=`=aktiv) ODER eine Triebwerks-Feuer-Lampe
        // (`VC_PED_ENGFIRE_{1,2}_LT_TOP` — Feuer ist eine rote
        // Master-Klasse-Bedingung).
        Some(
            t.fsl_master_warning != 0.0
                || t.fsl_engfire1_lt != 0.0
                || t.fsl_engfire2_lt != 0.0,
        )
    } else {
        None
    };

    // Kabinenhoehen-Warnung: hier liefert sie nur das iFly-Profil
    // nativ (`L:VC_WARNING_LIGHT_CABIN_ALTITUDE_L_VAL`, linke Lampe);
    // PMDG kommt weiter ueber den pmdg-Struct-Merge.
    let cabin_altitude_warning = if is_ifly {
        Some(t.ifly_cabin_alt_warning_light != 0.0)
    } else {
        None
    };

    // FCU-managed-Dots. A346-Besonderheit: die FCU hat kein eigenes
    // ALT-Dot — `TLS_FCU_VS_MANAGED` (V/S-Fenster managed = FMGC
    // fuehrt das vertikale Profil) ist die naechstliegende
    // APPROXIMATION fuer managed_altitude.
    let managed_speed = if is_fenix {
        Some(t.fnx_fcu_spd_managed != 0.0)
    } else if is_fbw {
        Some(t.fbw_fcu_spd_dot != 0.0)
    } else if is_a346 {
        Some(t.a346_fcu_spd_managed != 0.0)
    } else if is_fsl {
        // FSLabs A321 (v0.16.14): `L:FSL_FCU_*_MANAGED` (HubHop) —
        // echte managed-Dot-Flags der FCU-Fenster (hier hat FSL,
        // anders als die A346, auch ein dediziertes ALT-Flag).
        Some(t.fsl_fcu_spd_managed != 0.0)
    } else {
        None
    };
    let managed_heading = if is_fenix {
        Some(t.fnx_fcu_hdg_managed != 0.0)
    } else if is_fbw {
        Some(t.fbw_fcu_hdg_dot != 0.0)
    } else if is_a346 {
        Some(t.a346_fcu_hdg_managed != 0.0)
    } else if is_fsl {
        Some(t.fsl_fcu_hdg_managed != 0.0)
    } else {
        None
    };
    let managed_altitude = if is_fenix {
        Some(t.fnx_fcu_alt_managed != 0.0)
    } else if is_fbw {
        Some(t.fbw_fcu_alt_managed != 0.0)
    } else if is_a346 {
        Some(t.a346_fcu_vs_managed != 0.0)
    } else if is_fsl {
        Some(t.fsl_fcu_alt_managed != 0.0)
    } else {
        None
    };

    // Reverser: A346 + iFly liefern Ratio-/Animations-LVars (PMDG via
    // pmdg-Merge). > 0.05 filtert Idle-Jitter der 4 A346-Ratios.
    let reverser_deployed = if is_a346 {
        Some(
            t.a346_eng1_rev_ratio > 0.05
                || t.a346_eng2_rev_ratio > 0.05
                || t.a346_eng3_rev_ratio > 0.05
                || t.a346_eng4_rev_ratio > 0.05,
        )
    } else if is_ifly {
        // v0.16.11: iFly `L:Animation_Engine_{1,2}_Reverser_VAL` —
        // analoge 0..1-Reverser-Stellung (Animations-LVar). > 0.1
        // filtert Stowed-Jitter und zaehlt erst echtes Ausfahren.
        Some(t.ifly_eng1_reverser > 0.1 || t.ifly_eng2_reverser > 0.1)
    } else {
        None
    };

    // Ground-Spoiler aktiv (FBW + INI + iFly; A346 hat keinen direkten
    // Active-Flag — nur den Lever, der bleibt ungemappt).
    let ground_spoilers_active = if is_fbw {
        Some(t.fbw_ground_spoilers_active != 0.0)
    } else if is_ini {
        Some(t.ini_ground_spoilers != 0.0)
    } else if is_ifly {
        // v0.16.11: iFly `L:VC_FLTCTRL_LIGHT_SPEEDBRAKES_EXTENDED_VAL`
        // — die Lampe leuchtet bei JEDER ausgefahrenen Speedbrake,
        // auch in der Luft (dort ist das Flight-Spoiler, kein Ground-
        // Spoiler). Deshalb NUR am Boden als "Ground-Spoiler aktiv"
        // werten; in der Luft bleibt das Feld ehrlich false.
        Some(t.on_ground && t.ifly_speedbrakes_extended_light != 0.0)
    } else {
        None
    };

    // Spoilers HANDLE: KEIN FSL-Override (Review-Fund v0.16.20).
    // Frueher ueberschrieb das FSL-Profil `spoilers_handle_position`
    // mit einer aus `VC_PED_SPD_BRK_LEVER` erfundenen `/50`-Skala. Das
    // war FALSCH und ein Rollout-Scoring-Regress: FSL TREIBT die
    // Standard-SimVar `SPOILERS HANDLE POSITION` KORREKT (Peters Log:
    // 0.0 im Final → 1.0 beim Touchdown-Rollout). Der LVar kennt nur
    // 0=eingefahren / 10=armed (KEINEN deployed-Bereich), taugt also
    // nicht als Handle-Quelle. Also: Standard-SimVar unveraendert
    // durchreichen wie bei jedem anderen Aircraft.
    let spoilers_handle_position = t.spoilers_handle_position as f32;
    // Spoilers ARMED: Standard-SimVar ODER Profil-LVar — jede Quelle
    // genuegt, kein Regress fuer Aircraft mit funktionierendem
    // Standard.
    //
    // v0.16.21: FSLabs hat KEINEN Lever-basierten Override mehr. Der
    // v0.16.20-Versuch las `VC_PED_SPD_BRK_LEVER` 5..=15 als armed —
    // dieses Fenster faengt aber die NEUTRALE Lever-Stellung, sodass
    // `spoilers_armed` den GANZEN Flug True las (Live-Befund: 985/985
    // airborne-Samples). Kosmetisch (kein FSM/Scoring-Einfluss), aber
    // falsch — also faellt FSL auf die Standard-SimVar zurueck wie
    // jedes andere Aircraft ohne eigenes ARMED-LVar.
    let spoilers_armed = if is_fbw {
        t.spoilers_armed || t.fbw_spoilers_armed != 0.0
    } else if is_a346 {
        t.spoilers_armed || t.a346_spoiler_lever_armed != 0.0
    } else {
        t.spoilers_armed
    };

    // v0.16.20: FSL-Transponder-Modus. `VC_PED_ATCXPDR_MODE_SWITCH` —
    // Skript `/10` → 0=STBY, 1=TA, 2=TARA (Rohwert 0/10/20). Auf das
    // Panel-Label gemappt (None, wenn kein FSL-Profil).
    // Review-Fund v0.16.20: gegated auf den XPDR-ON/OFF-Schalter
    // (`VC_PED_ATCXPDR_ON_OFF_Switch`, Skript `/10` → 0=OFF, 1=AUTO,
    // 2=ON). Bei OFF (Rohwert 0) liefert der MODE-Switch kein
    // sinnvolles Label → None statt eines erfundenen STBY.
    let xpdr_mode_label = if is_fsl {
        let on_off = (t.fsl_xpdr_on_off_switch / 10.0).round() as i32;
        if on_off == 0 {
            None // Transponder AUS → kein Modus-Label
        } else {
            let mode = (t.fsl_xpdr_mode_switch / 10.0).round() as i32;
            Some(
                match mode {
                    0 => "STBY",
                    1 => "TA",
                    2 => "TA-RA",
                    _ => "STBY",
                }
                .to_string(),
            )
        }
    } else {
        None
    };

    // Baro STD (Fenix: EFIS1-Schalterstellung; FSL v0.16.20:
    // `FSL_EFIS_CPT_BARO_STD` !=0 = STD-Fenster gewaehlt).
    let baro_std = if is_fenix {
        Some(t.fnx_baro_std != 0.0)
    } else if is_fsl {
        Some(t.fsl_baro_std != 0.0)
    } else {
        None
    };

    // eng_n1_pct: generisches N1-Array ueber die Standard-SimVars
    // `TURB ENG N1:1..4` (alle Aircraft). Engine-Count-Heuristik:
    // hoechster Index, dessen Combustion (plain ODER EX1) an ist
    // ODER dessen normalisiertes N1 > 5 % liegt — das Praefix 1..k
    // bleibt positionserhaltend (Single-Engine-Taxi auf Engine 2
    // liefert [0, n1_2], nicht [n1_2]). Alles aus → None. Skala je
    // Addon 0-1 ODER 0-100 → auf Prozent normalisiert (wie der
    // N1-Fallback fuer engines_running oben).
    // MD-11-Ausnahme: display-exakte `MD11_ENG1..3_N1`-LVars
    // bevorzugen, sobald irgendeine > 0 liest; sonst Standard-Pfad.
    // v0.16.10 QS (Minor 9): zusaetzlich max(N1) >= 5 % verlangt —
    // Addons mit toten N1-SimVars aber lebenden Combustion-Bits
    // lieferten sonst ein Bogus-Array aus Nullen ([0.0, 0.0]).
    // Idle-N1 liegt bei ~20 %, das Gate kostet keine echten Werte.
    let eng_n1_pct: Option<Vec<f64>> = {
        let alive = |n1: &[f64]| n1.iter().cloned().fold(0.0_f64, f64::max) >= 5.0;
        let md11_n1 = if is_md11 {
            let n1 = [t.md11_eng1_n1, t.md11_eng2_n1, t.md11_eng3_n1];
            if alive(&n1) {
                Some(n1.to_vec())
            } else {
                None
            }
        } else {
            None
        };
        md11_n1.or_else(|| {
            let normalize = |raw: f64| if raw <= 1.5 { raw * 100.0 } else { raw };
            let n1 = [
                normalize(t.n1_pct_1),
                normalize(t.n1_pct_2),
                normalize(t.n1_pct_3),
                normalize(t.n1_pct_4),
            ];
            let combustion = [
                t.eng1_firing || t.eng1_combustion_ex1,
                t.eng2_firing || t.eng2_combustion_ex1,
                t.eng3_firing || t.eng3_combustion_ex1,
                t.eng4_firing || t.eng4_combustion_ex1,
            ];
            let k = (0..4)
                .filter(|&i| combustion[i] || n1[i] > 5.0)
                .map(|i| i + 1)
                .max()
                .unwrap_or(0);
            if k == 0 || !alive(&n1[..k]) {
                None
            } else {
                Some(n1[..k].to_vec())
            }
        })
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
        gear_position,
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
        autothrottle_on,
        fuel_flow_kg_per_h,
        // Spoilers-Handle: bleibt fuer ALLE Profile die analoge
        // Standard-SimVar — auch beim Fenix (dessen `L:A_FC_SPEEDBRAKE`
        // ist nur als Override-Kandidat subscribed, s. Tabellen-
        // Kommentar Gruppe A) und beim FSL (Review-Fund v0.16.20: FSL
        // treibt die Standard-SimVar korrekt, 0→1 beim Rollout — der
        // SPD_BRK_LEVER-Override war ein Rollout-Scoring-Regress).
        spoilers_handle_position: Some(spoilers_handle_position),
        // v0.16.10 (#Premium): Standard ODER Profil-LVar (FBW/A346/FSL).
        spoilers_armed: Some(spoilers_armed),
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
        // v0.16.20: FSL liefert den XPDR-Modus ueber den MODE-Switch
        // (STBY/TA/TA-RA); sonst None.
        xpdr_mode_label,
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
        // v0.16.10 (#Premium): Cockpit-Tiefendaten — profile-gegated
        // aus den LVar-Gruppen A-E gemappt (siehe Premium-Block oben).
        // PMDG-Premium fliesst zusaetzlich ueber den `pmdg`-Struct-
        // Merge in sim-core (`apply_pmdg_premium_override`).
        fma_lateral_mode,
        fma_vertical_mode,
        fma_thrust_mode,
        flight_phase_aircraft,
        v1_kt,
        vr_kt,
        v2_kt,
        vapp_kt,
        vls_kt,
        vref_kt,
        flex_temp_c,
        thrust_gate,
        master_caution,
        master_warning,
        managed_speed,
        managed_heading,
        managed_altitude,
        reverser_deployed,
        ground_spoilers_active,
        eng_n1_pct,
        baro_std,
        // Per-Tank-Fuel: bewusst None — die INI-Tank-Liste ist gross,
        // die generische fuel_total-Quelle reicht (PMDG via Merge).
        fuel_per_tank_kg: None,
        // Die folgenden drei liefert nur der PMDG-SDK-Pfad (Merge);
        // cabin_altitude_warning zusaetzlich nativ vom iFly-Profil
        // (v0.16.11, siehe Mapping oben).
        below_gs_alert: None,
        cabin_altitude_warning,
        stab_out_of_trim: None,
        minimums_baro_ft: None,
        // v0.16.12 (#phase-v2): Schatten-Felder stempelt NUR der
        // Streamer (post-Engine) — Adapter liefern immer None.
        shadow_phase: None,
        shadow_segment: None,
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
        // 2026-06-10: the A346 appends at the very end of the layout.
        assert!(!t.eng4_combustion_ex1);
        assert_eq!(t.eng4_ff_corrected_pph, 0.0);
        assert_eq!(t.a346_loc_light, 0.0);
        // v0.16.4: A346 full-profile tail (signs → gear lever).
        assert_eq!(t.a346_seatbelt_sw, 0.0);
        assert_eq!(t.a346_autobrake_mode, 0.0);
        assert_eq!(t.a346_gear_lever, 0.0);
        // v0.16.10 (#Premium): the very end of the new tail.
        assert_eq!(t.fnx_perf_v1, 0.0);
        assert_eq!(t.fbw_fcu_alt_managed, 0.0);
        assert_eq!(t.ini_autobrake_level, 0.0);
        assert_eq!(t.a346_eng4_rev_ratio, 0.0);
        assert_eq!(t.md11_autobrake_sw, 0.0);
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

    // ---- 2026-06-10: Aerosoft A346 (ToLiss-Port) — native Reads ----
    // WASM-Strings-Analyse der MSFS_ToLiss_Plugin.wasm: das Aircraft
    // treibt `GENERAL ENG COMBUSTION EX1:N`, `TURB ENG CORRECTED FF:N`
    // und AP-State NUR als `L:AB_AP_*_LIGHT_ON` LVars. Engines + Fuel-
    // Flow lesen addon-agnostisch (Varianten lesen 0/false auf Addons,
    // die sie nicht treiben), AP ist Profile-gegated.

    #[test]
    fn ex1_only_combustion_counts_engines_running() {
        // A346-Szenario: plain COMBUSTION alle false, EX1 alle true →
        // alle 4 als laufend erkannt, OHNE den N1-Fallback zu brauchen.
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346 Pro".into();
        t.atc_model = "A346".into();
        t.eng1_firing = false;
        t.eng2_firing = false;
        t.eng3_firing = false;
        t.eng4_firing = false;
        t.eng1_combustion_ex1 = true;
        t.eng2_combustion_ex1 = true;
        t.eng3_combustion_ex1 = true;
        t.eng4_combustion_ex1 = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 4);
    }

    #[test]
    fn plain_combustion_aircraft_unaffected_by_ex1() {
        // Aircraft mit funktionierender plain COMBUSTION (EX1 liest
        // dort false): Ergebnis unveraendert — kein Doppelzaehlen.
        let mut t = Telemetry::default();
        t.title = "Asobo A320 Neo".into();
        t.atc_model = "A20N".into();
        t.eng1_firing = true;
        t.eng2_firing = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 2);

        // Per-Engine-ODER: plain auf Engine 1, EX1 auf Engine 2 →
        // 2 Engines, nicht 1 und nicht 4.
        let mut t = Telemetry::default();
        t.eng1_firing = true;
        t.eng2_combustion_ex1 = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 2);
    }

    #[test]
    fn corrected_ff_only_maps_fuel_flow() {
        // A346-Szenario: ENG FUEL FLOW PPH alle 0, TURB ENG CORRECTED
        // FF liefert (A346-typisch ~2200 pph je Engine im Cruise) →
        // fuel_flow = konvertierte Summe.
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346 Pro".into();
        t.atc_model = "A346".into();
        t.eng1_ff_corrected_pph = 2200.0;
        t.eng2_ff_corrected_pph = 2210.0;
        t.eng3_ff_corrected_pph = 2190.0;
        t.eng4_ff_corrected_pph = 2200.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        let expected = (8800.0 * KG_PER_LB) as f32;
        let got = snap.fuel_flow_kg_per_h.expect("corrected FF must map");
        assert!(
            (got - expected).abs() < 0.5,
            "expected ≈{expected} kg/h, got {got}"
        );
    }

    #[test]
    fn pph_wins_over_corrected_ff_when_present() {
        // Aircraft, das BEIDE Quellen treibt: die direkte PPH-SimVar
        // gewinnt (kein Regress, kein Doppelzaehlen).
        let mut t = Telemetry::default();
        t.eng1_ff_pph = 1000.0;
        t.eng2_ff_pph = 1000.0;
        t.eng1_ff_corrected_pph = 9999.0;
        t.eng2_ff_corrected_pph = 9999.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        let expected = (2000.0 * KG_PER_LB) as f32;
        let got = snap.fuel_flow_kg_per_h.expect("PPH must map");
        assert!(
            (got - expected).abs() < 0.5,
            "PPH muss gewinnen: expected ≈{expected} kg/h, got {got}"
        );
    }

    #[test]
    fn no_fuel_flow_source_stays_none_for_fob_derivation() {
        // Weder PPH noch CORRECTED FF → None, damit die v0.13.18-FOB-
        // Ableitung im Position-Streamer als letzte Stufe greift.
        let t = Telemetry::default();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fuel_flow_kg_per_h, None);
    }

    #[test]
    fn a346_ap_master_from_ap_light_lvars() {
        // AP1-Lampe an, Standard-SimVar tot (A346-Realitaet) →
        // autopilot_master MUSS true melden.
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346 Pro".into();
        t.atc_model = "A346".into();
        t.ap_master = false; // Standard-SimVar tot
        t.a346_ap1_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // AP2 allein reicht ebenfalls (Master = AP1 ODER AP2).
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346-MahanAir".into();
        t.atc_model = "A346".into();
        t.a346_ap2_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // Beide Lampen aus → Master off.
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346 Pro".into();
        t.atc_model = "A346".into();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));
    }

    #[test]
    fn a346_approach_from_appr_or_loc_light() {
        // APPR-Lampe → Approach-Hold an.
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346 Pro".into();
        t.atc_model = "A346".into();
        t.a346_appr_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_approach, Some(true));

        // LOC-Lampe allein (lateraler Capture) zaehlt ebenfalls.
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346 Pro".into();
        t.atc_model = "A346".into();
        t.a346_loc_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_approach, Some(true));
    }

    #[test]
    fn a346_ap_lvars_do_not_affect_other_profiles() {
        // Profile-Gate: ein Nicht-A346-Aircraft mit (theoretisch)
        // gesetzten AB_AP-LVar-Slots bleibt auf dem Standard-Pfad.
        let mut t = Telemetry::default();
        t.title = "Asobo A320 Neo".into();
        t.atc_model = "A20N".into();
        t.ap_master = false;
        t.a346_ap1_light = 1.0;
        t.a346_appr_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));
        assert_eq!(snap.autopilot_approach, Some(false));
    }

    /// Definitive byte-level lockstep proof (QS v0.16.3): write a distinct
    /// pattern per table index (f64 = 1000+i, i32 = i, str = "S{i}") and
    /// assert each struct field parses exactly its own index value. Any
    /// offset drift / kind mismatch / mid-table insertion fails loudly —
    /// unlike the all-zero layout test above, which defaults can satisfy.
    #[test]
    fn pattern_buffer_proves_field_offsets() {
        let mut buf: Vec<u8> = Vec::new();
        for (i, f) in TELEMETRY_FIELDS.iter().enumerate() {
            match f.kind {
                FieldKind::Float64 => buf.extend_from_slice(&(1000.0 + i as f64).to_le_bytes()),
                FieldKind::Int32 => buf.extend_from_slice(&(i as i32).to_le_bytes()),
                FieldKind::String256 => {
                    let mut s = [0u8; 256];
                    let txt = format!("S{i}");
                    s[..txt.len()].copy_from_slice(txt.as_bytes());
                    buf.extend_from_slice(&s);
                }
            }
        }
        assert_eq!(buf.len(), 2780, "total block size");
        let t = Telemetry::from_block(&buf);

        // Identity / head sentinels.
        assert_eq!(t.title, "S0");
        assert_eq!(t.atc_model, "S1");
        assert_eq!(t.atc_id, "S2");
        assert_eq!(t.lat, 1003.0);
        assert_eq!(t.vertical_speed_fpm, 1013.0);

        // Mid-table sentinels around the bool clusters.
        assert_eq!(t.gear_position, 1026.0); // idx 26
        assert_eq!(t.n1_pct_1, 1032.0); // idx 32
        assert_eq!(t.fuel_total_lb_ex1, 1036.0); // idx 36 (EX1 precedent)
        assert_eq!(t.eng1_ff_pph, 1062.0); // idx 62
        assert_eq!(t.pushback_state, 1068.0); // idx 68
        assert_eq!(t.fbw_xpdr, 1079.0); // idx 79
        assert_eq!(t.fnx_autobrake_max, 1111.0); // idx 111
        assert_eq!(t.fnx_fc_flaps_lever, 1118.0); // idx 118
        assert_eq!(t.fsr_phenom_eng1_knob, 1119.0); // idx 119
        assert_eq!(t.fsr_phenom_eng2_knob, 1120.0); // idx 120

        // ---- the 13 new A346 tail fields (idx 121..133) ----
        assert!(t.eng1_combustion_ex1); // idx 121 (121 != 0)
        assert!(t.eng2_combustion_ex1); // idx 122
        assert!(t.eng3_combustion_ex1); // idx 123
        assert!(t.eng4_combustion_ex1); // idx 124
        assert_eq!(t.eng1_ff_corrected_pph, 1125.0); // idx 125
        assert_eq!(t.eng2_ff_corrected_pph, 1126.0); // idx 126
        assert_eq!(t.eng3_ff_corrected_pph, 1127.0); // idx 127
        assert_eq!(t.eng4_ff_corrected_pph, 1128.0); // idx 128
        assert_eq!(t.a346_ap1_light, 1129.0); // idx 129
        assert_eq!(t.a346_ap2_light, 1130.0); // idx 130
        assert_eq!(t.a346_athr_light, 1131.0); // idx 131
        assert_eq!(t.a346_appr_light, 1132.0); // idx 132
        assert_eq!(t.a346_loc_light, 1133.0); // idx 133

        // ---- v0.16.4: the 12 A346 full-profile tail fields
        //      (idx 134..145, all f64) ----
        assert_eq!(t.a346_seatbelt_sw, 1134.0); // idx 134
        assert_eq!(t.a346_no_smoking_sw, 1135.0); // idx 135
        assert_eq!(t.a346_antiice_engl1, 1136.0); // idx 136
        assert_eq!(t.a346_antiice_engl2, 1137.0); // idx 137
        assert_eq!(t.a346_antiice_engr1, 1138.0); // idx 138
        assert_eq!(t.a346_antiice_engr2, 1139.0); // idx 139
        assert_eq!(t.a346_antiice_wing, 1140.0); // idx 140
        assert_eq!(t.a346_antiice_probewindow, 1141.0); // idx 141
        assert_eq!(t.a346_bat1_off_light, 1142.0); // idx 142
        assert_eq!(t.a346_bat2_off_light, 1143.0); // idx 143
        assert_eq!(t.a346_autobrake_mode, 1144.0); // idx 144
        assert_eq!(t.a346_gear_lever, 1145.0); // idx 145

        // ---- the 5 new A350 tail fields (idx 146..150, v0.16.8) ----
        assert_eq!(t.a350_ap1_on, 1146.0); // idx 146
        assert_eq!(t.a350_ap2_on, 1147.0); // idx 147
        assert_eq!(t.a350_athr_light, 1148.0); // idx 148
        assert_eq!(t.a350_appr_light, 1149.0); // idx 149
        assert_eq!(t.a350_loc_light, 1150.0); // idx 150

        // ---- v0.16.10 (#Premium): 79 tail fields (idx 151..229) ----
        // Gruppe A: Fenix Premium (idx 151..163).
        assert_eq!(t.fnx_perf_v1, 1151.0); // idx 151
        assert_eq!(t.fnx_perf_vr, 1152.0); // idx 152
        assert_eq!(t.fnx_perf_v2, 1153.0); // idx 153
        assert_eq!(t.fnx_perf_flex, 1154.0); // idx 154
        assert_eq!(t.fnx_master_caution, 1155.0); // idx 155
        assert_eq!(t.fnx_master_warning, 1156.0); // idx 156
        assert_eq!(t.fnx_speedbrake_handle, 1157.0); // idx 157
        assert_eq!(t.fnx_fcu_spd_managed, 1158.0); // idx 158
        assert_eq!(t.fnx_fcu_hdg_managed, 1159.0); // idx 159
        assert_eq!(t.fnx_fcu_alt_managed, 1160.0); // idx 160
        assert_eq!(t.fnx_baro_std, 1161.0); // idx 161
        assert_eq!(t.fnx_eng1_fire, 1162.0); // idx 162
        assert_eq!(t.fnx_eng2_fire, 1163.0); // idx 163

        // Gruppe B: FBW-Familie (idx 164..180).
        assert_eq!(t.fbw_ap1_active, 1164.0); // idx 164
        assert_eq!(t.fbw_ap2_active, 1165.0); // idx 165
        assert_eq!(t.fbw_athr_status, 1166.0); // idx 166
        assert_eq!(t.fbw_athr_mode, 1167.0); // idx 167
        assert_eq!(t.fbw_fma_lateral, 1168.0); // idx 168
        assert_eq!(t.fbw_fma_vertical, 1169.0); // idx 169
        assert_eq!(t.fbw_fwc_phase, 1170.0); // idx 170
        assert_eq!(t.fbw_vspeeds_v2, 1171.0); // idx 171
        assert_eq!(t.fbw_vspeeds_vls, 1172.0); // idx 172
        assert_eq!(t.fbw_vspeeds_vapp, 1173.0); // idx 173
        assert_eq!(t.fbw_autobrake_armed_mode, 1174.0); // idx 174
        assert_eq!(t.fbw_flaps_handle_index, 1175.0); // idx 175
        assert_eq!(t.fbw_spoilers_armed, 1176.0); // idx 176
        assert_eq!(t.fbw_ground_spoilers_active, 1177.0); // idx 177
        assert_eq!(t.fbw_fcu_spd_dot, 1178.0); // idx 178
        assert_eq!(t.fbw_fcu_hdg_dot, 1179.0); // idx 179
        assert_eq!(t.fbw_fcu_alt_managed, 1180.0); // idx 180

        // Gruppe C: INI A350/A340 (idx 181..203).
        assert_eq!(t.ini_roll_mode, 1181.0); // idx 181
        assert_eq!(t.ini_pitch_mode, 1182.0); // idx 182
        assert_eq!(t.ini_throttle_mode, 1183.0); // idx 183
        assert_eq!(t.ini_v1, 1184.0); // idx 184
        assert_eq!(t.ini_vr, 1185.0); // idx 185
        assert_eq!(t.ini_v2, 1186.0); // idx 186
        assert_eq!(t.ini_vls, 1187.0); // idx 187
        assert_eq!(t.ini_vapp, 1188.0); // idx 188
        assert_eq!(t.ini_vref, 1189.0); // idx 189
        assert_eq!(t.ini_flex_temp, 1190.0); // idx 190
        assert_eq!(t.ini_lever_toga, 1191.0); // idx 191
        assert_eq!(t.ini_lever_flex_mct, 1192.0); // idx 192
        assert_eq!(t.ini_lever_cl, 1193.0); // idx 193
        assert_eq!(t.ini_flaps_handle_index, 1194.0); // idx 194
        assert_eq!(t.ini_ground_spoilers, 1195.0); // idx 195
        assert_eq!(t.ini_autobrake_engaged, 1196.0); // idx 196
        assert_eq!(t.ini_master_caution, 1197.0); // idx 197
        assert_eq!(t.ini_master_warning, 1198.0); // idx 198
        assert_eq!(t.ini_fuel_flow1_kg, 1199.0); // idx 199
        assert_eq!(t.ini_fuel_flow2_kg, 1200.0); // idx 200
        assert_eq!(t.ini_fuel_flow3_kg, 1201.0); // idx 201
        assert_eq!(t.ini_fuel_flow4_kg, 1202.0); // idx 202
        assert_eq!(t.ini_autobrake_level, 1203.0); // idx 203

        // Gruppe D: A346 Premium-Extras (idx 204..215).
        assert_eq!(t.a346_flight_phase, 1204.0); // idx 204
        assert_eq!(t.a346_fcu_spd_managed, 1205.0); // idx 205
        assert_eq!(t.a346_fcu_hdg_managed, 1206.0); // idx 206
        assert_eq!(t.a346_fcu_vs_managed, 1207.0); // idx 207
        assert_eq!(t.a346_master_warning_light, 1208.0); // idx 208
        assert_eq!(t.a346_master_caution_light, 1209.0); // idx 209
        assert_eq!(t.a346_spd_brk_lever_pos, 1210.0); // idx 210
        assert_eq!(t.a346_spoiler_lever_armed, 1211.0); // idx 211
        assert_eq!(t.a346_eng1_rev_ratio, 1212.0); // idx 212
        assert_eq!(t.a346_eng2_rev_ratio, 1213.0); // idx 213
        assert_eq!(t.a346_eng3_rev_ratio, 1214.0); // idx 214
        assert_eq!(t.a346_eng4_rev_ratio, 1215.0); // idx 215

        // Gruppe E: TFDi MD-11 (idx 216..229).
        assert_eq!(t.md11_ap_state, 1216.0); // idx 216
        assert_eq!(t.md11_ats_state, 1217.0); // idx 217
        assert_eq!(t.md11_ats_clamp, 1218.0); // idx 218
        assert_eq!(t.md11_afs_spd, 1219.0); // idx 219
        assert_eq!(t.md11_afs_hdg, 1220.0); // idx 220
        assert_eq!(t.md11_afs_alt, 1221.0); // idx 221
        assert_eq!(t.md11_afs_vs, 1222.0); // idx 222
        assert_eq!(t.md11_v1, 1223.0); // idx 223
        assert_eq!(t.md11_vr, 1224.0); // idx 224
        assert_eq!(t.md11_v2, 1225.0); // idx 225
        assert_eq!(t.md11_eng1_n1, 1226.0); // idx 226
        assert_eq!(t.md11_eng2_n1, 1227.0); // idx 227
        assert_eq!(t.md11_eng3_n1, 1228.0); // idx 228
        assert_eq!(t.md11_autobrake_sw, 1229.0); // idx 229

        // Gruppe F: iFly 737 MAX 8 (idx 230..239, v0.16.11).
        assert_eq!(t.ifly_cmd_a_light, 1230.0); // idx 230
        assert_eq!(t.ifly_cmd_b_light, 1231.0); // idx 231
        assert_eq!(t.ifly_at_arm_light, 1232.0); // idx 232
        assert_eq!(t.ifly_master_caution_light, 1233.0); // idx 233
        assert_eq!(t.ifly_fire_warning_light, 1234.0); // idx 234
        assert_eq!(t.ifly_cabin_alt_warning_light, 1235.0); // idx 235
        assert_eq!(t.ifly_eng1_reverser, 1236.0); // idx 236
        assert_eq!(t.ifly_eng2_reverser, 1237.0); // idx 237
        assert_eq!(t.ifly_speedbrakes_extended_light, 1238.0); // idx 238
        assert_eq!(t.ifly_autobrake_sw, 1239.0); // idx 239

        // Gruppe G: FSLabs A321 (idx 240..256, v0.16.14).
        assert_eq!(t.fsl_ap1_light, 1240.0); // idx 240
        assert_eq!(t.fsl_ap2_light, 1241.0); // idx 241
        assert_eq!(t.fsl_athr_light, 1242.0); // idx 242
        assert_eq!(t.fsl_appr_light, 1243.0); // idx 243
        assert_eq!(t.fsl_loc_light, 1244.0); // idx 244
        assert_eq!(t.fsl_fcu_spd, 1245.0); // idx 245
        assert_eq!(t.fsl_fcu_hdg, 1246.0); // idx 246
        assert_eq!(t.fsl_fcu_alt, 1247.0); // idx 247
        assert_eq!(t.fsl_fcu_vs, 1248.0); // idx 248
        assert_eq!(t.fsl_fcu_spd_managed, 1249.0); // idx 249
        assert_eq!(t.fsl_fcu_hdg_managed, 1250.0); // idx 250
        assert_eq!(t.fsl_fcu_alt_managed, 1251.0); // idx 251
        assert_eq!(t.fsl_fcu_spd_dashed, 1252.0); // idx 252
        assert_eq!(t.fsl_fcu_hdg_dashed, 1253.0); // idx 253
        assert_eq!(t.fsl_autobrake_lo_light, 1254.0); // idx 254
        assert_eq!(t.fsl_autobrake_med_light, 1255.0); // idx 255
        assert_eq!(t.fsl_autobrake_max_light, 1256.0); // idx 256

        // ---- v0.16.20: FSLabs A321 PREMIUM tail (idx 257..269) ----
        assert_eq!(t.fsl_park_brake_switch, 1257.0); // idx 257
        assert_eq!(t.fsl_eng1_mstr_switch, 1258.0); // idx 258
        assert_eq!(t.fsl_eng2_mstr_switch, 1259.0); // idx 259
        assert_eq!(t.fsl_wheel_chocks, 1260.0); // idx 260
        assert_eq!(t.fsl_baro_std, 1261.0); // idx 261
        assert_eq!(t.fsl_master_caution, 1262.0); // idx 262
        assert_eq!(t.fsl_master_warning, 1263.0); // idx 263
        assert_eq!(t.fsl_spd_brk_lever, 1264.0); // idx 264
        assert_eq!(t.fsl_engfire1_lt, 1265.0); // idx 265
        assert_eq!(t.fsl_engfire2_lt, 1266.0); // idx 266
        assert_eq!(t.fsl_eng1_anti_ice, 1267.0); // idx 267
        assert_eq!(t.fsl_eng2_anti_ice, 1268.0); // idx 268
        assert_eq!(t.fsl_xpdr_mode_switch, 1269.0); // idx 269
        assert_eq!(t.fsl_xpdr_on_off_switch, 1270.0); // idx 270 (Review-Fund)
    }

    /// Truncated block (e.g. all 12 new tail LVars rejected by an older
    /// sim build): everything before stays correct, the tail parses to
    /// safe defaults — no offset shift bleeds into pre-existing fields.
    #[test]
    fn truncated_tail_leaves_existing_fields_intact() {
        let mut buf: Vec<u8> = Vec::new();
        for (i, f) in TELEMETRY_FIELDS.iter().enumerate() {
            match f.kind {
                FieldKind::Float64 => buf.extend_from_slice(&(1000.0 + i as f64).to_le_bytes()),
                FieldKind::Int32 => buf.extend_from_slice(&(i as i32).to_le_bytes()),
                FieldKind::String256 => buf.extend_from_slice(&[0u8; 256]),
            }
        }
        // v0.16.20: drop the 14 FSLabs PREMIUM tail fields (14*8) — the
        // new outermost layer (13 original + the XPDR ON/OFF Review-Fund
        // field). Everything up to the v0.16.14 FSL group stays intact,
        // the new premium slots parse to safe defaults.
        buf.truncate(buf.len() - 112);
        let t = Telemetry::from_block(&buf);
        assert_eq!(t.fsl_autobrake_max_light, 1256.0); // last v0.16.14 field intact
        assert_eq!(t.ifly_autobrake_sw, 1239.0); // v0.16.11 layer intact
        assert_eq!(t.fsl_park_brake_switch, 0.0); // premium tail = safe defaults
        assert_eq!(t.fsl_eng1_mstr_switch, 0.0);
        assert_eq!(t.fsl_xpdr_mode_switch, 0.0);
        assert_eq!(t.fsl_xpdr_on_off_switch, 0.0);

        // v0.16.14: drop the 17 FSLabs tail fields (17*8).
        buf.truncate(buf.len() - 136);
        let t = Telemetry::from_block(&buf);
        assert_eq!(t.ifly_autobrake_sw, 1239.0); // last v0.16.11 field intact
        assert_eq!(t.md11_autobrake_sw, 1229.0); // v0.16.10 layer intact
        assert_eq!(t.fsl_ap1_light, 0.0); // FSL tail = safe defaults
        assert_eq!(t.fsl_fcu_vs, 0.0);
        assert_eq!(t.fsl_autobrake_max_light, 0.0);

        // v0.16.11: drop the 10 iFly tail fields (10*8).
        buf.truncate(buf.len() - 80);
        let t = Telemetry::from_block(&buf);
        assert_eq!(t.md11_autobrake_sw, 1229.0); // last v0.16.10 field intact
        assert_eq!(t.fnx_perf_v1, 1151.0); // first premium field intact
        assert_eq!(t.ifly_cmd_a_light, 0.0); // iFly tail = safe defaults
        assert_eq!(t.ifly_cabin_alt_warning_light, 0.0);
        assert_eq!(t.ifly_autobrake_sw, 0.0);

        // v0.16.10 (#Premium): drop the 79 premium tail fields (79*8).
        buf.truncate(buf.len() - 632);
        let t = Telemetry::from_block(&buf);
        assert_eq!(t.a350_loc_light, 1150.0); // last v0.16.8 field intact
        assert_eq!(t.a346_gear_lever, 1145.0); // v0.16.4 layer intact
        // Premium tail = safe defaults (first + last of each group).
        assert_eq!(t.fnx_perf_v1, 0.0);
        assert_eq!(t.fnx_eng2_fire, 0.0);
        assert_eq!(t.fbw_ap1_active, 0.0);
        assert_eq!(t.fbw_fcu_alt_managed, 0.0);
        assert_eq!(t.ini_roll_mode, 0.0);
        assert_eq!(t.ini_autobrake_level, 0.0);
        assert_eq!(t.a346_flight_phase, 0.0);
        assert_eq!(t.a346_eng4_rev_ratio, 0.0);
        assert_eq!(t.md11_ap_state, 0.0);
        assert_eq!(t.md11_autobrake_sw, 0.0);

        // v0.16.8: drop the 5 new A350 tail fields (5*8).
        buf.truncate(buf.len() - 40);
        let t = Telemetry::from_block(&buf);
        assert_eq!(t.a346_gear_lever, 1145.0); // last v0.16.4 field intact
        assert_eq!(t.a350_ap1_on, 0.0); // A350 tail = safe defaults
        assert_eq!(t.a350_loc_light, 0.0);

        // v0.16.4: drop the 12 A346 full-profile tail fields (12*8).
        buf.truncate(buf.len() - 96);
        let t = Telemetry::from_block(&buf);
        assert_eq!(t.fsr_phenom_eng2_knob, 1120.0); // pre-v0.16.3 field intact
        assert_eq!(t.a346_loc_light, 1133.0); // last pre-existing field intact
        assert_eq!(t.a346_seatbelt_sw, 0.0); // new tail = safe defaults
        assert_eq!(t.a346_bat1_off_light, 0.0);
        assert_eq!(t.a346_gear_lever, 0.0);

        // Deeper truncation (both A346 tails gone, = a v0.16.2-era
        // block): the v0.13.x layout still parses intact.
        buf.truncate(buf.len() - 88); // also drop the v0.16.3 tail (4*4 + 9*8)
        let t = Telemetry::from_block(&buf);
        assert_eq!(t.fsr_phenom_eng2_knob, 1120.0);
        assert!(!t.eng1_combustion_ex1); // v0.16.3 tail = safe defaults
        assert_eq!(t.eng1_ff_corrected_pph, 0.0);
        assert_eq!(t.a346_loc_light, 0.0);
        assert_eq!(t.a346_gear_lever, 0.0);
    }

    // ---- v0.16.4: Aerosoft A346 full profile (Signs, Anti-Ice, BAT,
    // Autobrake, Gear-Lever, A/THR) ----

    /// Minimal A346-profile Telemetry. Standard SimVars stay at their
    /// defaults so any mapped value is unambiguously LVar-sourced.
    fn a346_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "Aerosoft A346 Pro".into();
        t.atc_model = "A346".into();
        t
    }

    #[test]
    fn a346_signs_map_from_overhead_switches() {
        // Seatbelt 1 / No-Smoking 2 → beide Some, geclamped 0..=2.
        let mut t = a346_telemetry();
        t.a346_seatbelt_sw = 1.0;
        t.a346_no_smoking_sw = 2.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.seatbelts_sign, Some(1));
        assert_eq!(snap.no_smoking_sign, Some(2));

        // Clamp: Werte ausserhalb des Kontrakts werden eingefangen.
        let mut t = a346_telemetry();
        t.a346_seatbelt_sw = 7.0;
        t.a346_no_smoking_sw = -3.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.seatbelts_sign, Some(2));
        assert_eq!(snap.no_smoking_sign, Some(0));

        // Beide aus → Some(0), nicht None (das Profil LIEFERT Signs).
        let snap = telemetry_to_snapshot(a346_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.seatbelts_sign, Some(0));
        assert_eq!(snap.no_smoking_sign, Some(0));
    }

    #[test]
    fn a346_engine_anti_ice_any_of_four_switches() {
        // Jeder einzelne der 4 Schalter reicht fuer "any on".
        for set in [
            |t: &mut Telemetry| t.a346_antiice_engl1 = 1.0,
            |t: &mut Telemetry| t.a346_antiice_engl2 = 1.0,
            |t: &mut Telemetry| t.a346_antiice_engr1 = 1.0,
            |t: &mut Telemetry| t.a346_antiice_engr2 = 1.0,
        ] {
            let mut t = a346_telemetry();
            set(&mut t);
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(snap.engine_anti_ice, Some(true));
        }

        // Alle aus → Some(false). Die Standard-SimVars (eng1_anti_ice
        // etc.) duerfen unter A346 NICHT durchschlagen.
        let mut t = a346_telemetry();
        t.eng1_anti_ice = true; // Standard-SimVar-Slot (tot auf der A346)
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engine_anti_ice, Some(false));
    }

    #[test]
    fn a346_wing_anti_ice_from_lvar() {
        let mut t = a346_telemetry();
        t.a346_antiice_wing = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.wing_anti_ice, Some(true));

        let snap = telemetry_to_snapshot(a346_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.wing_anti_ice, Some(false));
    }

    #[test]
    fn a346_pitot_heat_mirrors_fenix_always_available() {
        // Wie beim Fenix: PROBE/WINDOW 0=AUTO heizt automatisch →
        // beide Stellungen gelten als "heat available".
        let snap = telemetry_to_snapshot(a346_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.pitot_heat, Some(true));
    }

    #[test]
    fn a346_battery_master_inverted_from_off_lights() {
        // KERN-INVERSION: `AB_VC_OVH_ELEC_BAT{1,2}_OFF` sind die
        // "OFF"-Annunciator-Lampen — Lampe AN (1) heisst Batterie AUS.

        // Cold & Dark: beide OFF-Lampen leuchten → Batterien AUS.
        let mut t = a346_telemetry();
        t.a346_bat1_off_light = 1.0;
        t.a346_bat2_off_light = 1.0;
        t.battery_master = true; // Standard-SimVar darf nicht durchschlagen
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.battery_master, Some(false));

        // Eine Batterie an (deren OFF-Lampe erloschen) → Master an.
        let mut t = a346_telemetry();
        t.a346_bat1_off_light = 0.0;
        t.a346_bat2_off_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.battery_master, Some(true));

        // Beide an (beide Lampen aus) → Master an.
        let snap = telemetry_to_snapshot(a346_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.battery_master, Some(true));
    }

    #[test]
    fn a346_autobrake_mode_enum_maps_to_fenix_labels() {
        let cases = [
            (0.0, Some("OFF")),
            (1.0, Some("LO")),
            (2.0, Some("MED")),
            (3.0, Some("MAX")),
            // Unbekannte Enum-Werte → None, kein erfundenes Label.
            (4.0, None),
            (-1.0, None),
        ];
        for (mode, expected) in cases {
            let mut t = a346_telemetry();
            t.a346_autobrake_mode = mode;
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(
                snap.autobrake.as_deref(),
                expected,
                "AB_AutoBrake_Mode={mode}"
            );
        }
    }

    #[test]
    fn a346_gear_position_from_selector_lever() {
        // v0.13.17-Befund: die Standard-SimVar klemmt bei der A346 auf
        // "down" — unter dem A346-Profil zaehlt NUR der Lever-LVar.
        let mut t = a346_telemetry();
        t.gear_position = 1.0; // Standard-SimVar klemmt auf down
        t.a346_gear_lever = 0.0; // Lever = UP
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.gear_position, 0.0, "lever up must win over stuck SimVar");

        let mut t = a346_telemetry();
        t.gear_position = 1.0;
        t.a346_gear_lever = 1.0; // Lever = DOWN
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.gear_position, 1.0);
    }

    #[test]
    fn gear_lever_does_not_affect_other_profiles() {
        // Nicht-A346: Standard `GEAR POSITION` bleibt die Quelle, der
        // (dort tote) Lever-LVar-Slot wird ignoriert.
        let mut t = Telemetry::default();
        t.title = "Asobo A320 Neo".into();
        t.atc_model = "A20N".into();
        t.gear_position = 0.0; // Standard sagt UP
        t.a346_gear_lever = 1.0; // LVar-Slot (theoretisch) gesetzt
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.gear_position, 0.0);

        let mut t = Telemetry::default();
        t.gear_position = 1.0;
        t.a346_gear_lever = 0.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.gear_position, 1.0);
    }

    #[test]
    fn autothrottle_maps_for_a346_and_fenix_none_elsewhere() {
        // A346: ATHR-Annunciator-Lampe (seit v0.16.3 subscribed).
        let mut t = a346_telemetry();
        t.a346_athr_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(true));
        let snap = telemetry_to_snapshot(a346_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(false));

        // Fenix: `L:S_FCU_ATHR`.
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM SL".into();
        t.atc_model = "A320".into();
        t.fnx_fcu_athr = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(true));

        // Default-Profil: None — auch wenn die (toten) LVar-Slots
        // zufaellig Werte tragen.
        let mut t = Telemetry::default();
        t.title = "Asobo A320 Neo".into();
        t.atc_model = "A20N".into();
        t.a346_athr_light = 1.0;
        t.fnx_fcu_athr = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, None);
    }

    #[test]
    fn a346_full_profile_does_not_affect_other_profiles() {
        // Profile-Gate-Beweis: ein Default-Profil-Aircraft mit (theore-
        // tisch) gesetzten A346-LVar-Slots bleibt byte-fuer-byte auf
        // dem Standard-SimVar-Pfad.
        let mut t = Telemetry::default();
        t.title = "Asobo A320 Neo".into();
        t.atc_model = "A20N".into();
        t.a346_seatbelt_sw = 2.0;
        t.a346_no_smoking_sw = 2.0;
        t.a346_antiice_engl1 = 1.0;
        t.a346_antiice_wing = 1.0;
        t.a346_antiice_probewindow = 1.0;
        t.a346_bat1_off_light = 1.0;
        t.a346_bat2_off_light = 1.0;
        t.a346_autobrake_mode = 3.0;
        // Standard-SimVars in einen definierten Zustand setzen.
        t.battery_master = true;
        t.pitot_heat = false;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.seatbelts_sign, None);
        assert_eq!(snap.no_smoking_sign, None);
        assert_eq!(snap.engine_anti_ice, Some(false)); // Standard eng1..4
        assert_eq!(snap.wing_anti_ice, Some(false)); // Standard deice
        assert_eq!(snap.pitot_heat, Some(false)); // Standard SimVar
        assert_eq!(snap.battery_master, Some(true)); // Standard, NICHT invertiert
        assert_eq!(snap.autobrake, None);
        assert_eq!(snap.autothrottle_on, None);
    }

    // ---- v0.16.8: iniBuilds A350 AP/A-THR (HubHop-LVars) ----

    #[test]
    fn a350_ap_master_from_fcu_leds() {
        let mut t = Telemetry::default();
        t.title = "iniBuilds A350-900".into();
        t.atc_model = "A359".into();
        t.a350_ap1_on = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        let mut t = Telemetry::default();
        t.title = "iniBuilds A350-900".into();
        t.atc_model = "A359".into();
        t.a350_ap2_on = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // beide LEDs aus + Standard tot → false (Status quo)
        let mut t = Telemetry::default();
        t.title = "iniBuilds A350-900".into();
        t.atc_model = "A359".into();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));
    }

    #[test]
    fn a350_athr_and_approach_from_leds() {
        let mut t = Telemetry::default();
        t.title = "iniBuilds A350-900".into();
        t.atc_model = "A359".into();
        t.a350_athr_light = 1.0;
        t.a350_loc_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(true));
        assert_eq!(snap.autopilot_approach, Some(true));
    }

    #[test]
    fn a350_lvars_do_not_affect_other_profiles() {
        // Profil-Gate: ein Default-Aircraft mit (theoretisch) gesetzten
        // A350-LVar-Slots bleibt auf dem Standard-Pfad.
        let mut t = Telemetry::default();
        t.title = "Asobo C172".into();
        t.atc_model = "C172".into();
        t.a350_ap1_on = 1.0;
        t.a350_athr_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));
        assert_eq!(snap.autothrottle_on, None);
    }

    // ==== v0.16.10 (#Premium): Cockpit-Tiefendaten (Gruppen A-E) ====

    fn fbw_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "FlyByWire A32NX".into();
        t.atc_model = "A20N".into();
        t
    }

    fn fenix_premium_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "FenixA320 CFM SL".into();
        t.atc_model = "A320".into();
        t
    }

    fn ini_a350_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "iniBuilds A350-900".into();
        t.atc_model = "A359".into();
        t
    }

    fn ini_a340_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "iniBuilds A340-300".into();
        t.atc_model = "A343".into();
        t
    }

    fn md11_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "TFDi Design MD-11".into();
        t.atc_model = "MD11".into();
        t
    }

    #[test]
    fn fbw_fma_labels_documented_enum_and_unknown_fallback() {
        // Dokumentierte Enum-Werte → PFD-Labels.
        let mut t = fbw_telemetry();
        t.fbw_fma_lateral = 20.0; // NAV
        t.fbw_fma_vertical = 50.0; // G/S*
        t.fbw_athr_mode = 7.0; // SPEED
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fma_lateral_mode.as_deref(), Some("NAV"));
        assert_eq!(snap.fma_vertical_mode.as_deref(), Some("G/S*"));
        assert_eq!(snap.fma_thrust_mode.as_deref(), Some("SPEED"));

        // Unbekannte Enum-Werte → "#{n}" (nicht verworfen).
        let mut t = fbw_telemetry();
        t.fbw_fma_lateral = 99.0;
        t.fbw_fma_vertical = 77.0;
        t.fbw_athr_mode = 42.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fma_lateral_mode.as_deref(), Some("#99"));
        assert_eq!(snap.fma_vertical_mode.as_deref(), Some("#77"));
        assert_eq!(snap.fma_thrust_mode.as_deref(), Some("#42"));

        // 0 = kein Mode → None.
        let snap = telemetry_to_snapshot(fbw_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.fma_lateral_mode, None);
        assert_eq!(snap.fma_vertical_mode, None);
        assert_eq!(snap.fma_thrust_mode, None);
    }

    #[test]
    fn fbw_athr_status_active_vs_armed_semantics() {
        // Status 1 = ARMED (TOGA-Takeoff, Lever manuell) — zaehlt
        // NICHT als "A/THR on". Erst Status >= 2 (ACTIVE) ist on.
        let mut t = fbw_telemetry();
        t.fbw_athr_status = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(false));

        let mut t = fbw_telemetry();
        t.fbw_athr_status = 2.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(true));

        // v0.16.10 QS (M4): Status 0 → None (toter LVar auf einem
        // marker-losen Nicht-FBW-A339 darf kein Phantom-Some(false)
        // produzieren).
        let snap = telemetry_to_snapshot(fbw_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, None);
    }

    /// v0.16.10 QS (M4) Defense-in-Depth: ein Nicht-FBW-A339 (ICAO-
    /// Fallback, A32NX_-LVars tot) muss AP-Sub-Modes weiterhin ueber
    /// die Standard-SimVars melden — das ODER faengt den Fall.
    #[test]
    fn fbw_ap_submodes_or_standard_simvars() {
        let mut t = fbw_telemetry();
        t.ap_heading = true;
        t.ap_altitude = true;
        t.ap_nav = true;
        t.ap_approach = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_heading, Some(true));
        assert_eq!(snap.autopilot_altitude, Some(true));
        assert_eq!(snap.autopilot_nav, Some(true));
        assert_eq!(snap.autopilot_approach, Some(true));

        // FBW-LVars allein genuegen weiterhin (kein Regress).
        let mut t = fbw_telemetry();
        t.fbw_ap_hdg = 1.0;
        t.fbw_ap_appr = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_heading, Some(true));
        assert_eq!(snap.autopilot_approach, Some(true));
        assert_eq!(snap.autopilot_altitude, Some(false));
    }

    #[test]
    fn fbw_ap_master_from_ap1_or_ap2_lvars() {
        // AP1-LVar allein reicht (kombinierter Active-LVar + Standard
        // beide tot).
        let mut t = fbw_telemetry();
        t.fbw_ap1_active = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        let mut t = fbw_telemetry();
        t.fbw_ap2_active = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // Bisheriger Pfad (kombinierter LVar) bleibt gueltig.
        let mut t = fbw_telemetry();
        t.fbw_ap_active = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        let snap = telemetry_to_snapshot(fbw_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));
    }

    #[test]
    fn fbw_fwc_phase_labels() {
        let mut t = fbw_telemetry();
        t.fbw_fwc_phase = 6.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.flight_phase_aircraft.as_deref(), Some("CRUISE"));

        let mut t = fbw_telemetry();
        t.fbw_fwc_phase = 10.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(
            snap.flight_phase_aircraft.as_deref(),
            Some("ROLLOUT <80KT")
        );

        // 0 = FWC nicht initialisiert → None; unbekannt → "#{n}".
        let snap = telemetry_to_snapshot(fbw_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.flight_phase_aircraft, None);
        let mut t = fbw_telemetry();
        t.fbw_fwc_phase = 12.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.flight_phase_aircraft.as_deref(), Some("#12"));
    }

    #[test]
    fn fbw_vspeeds_zero_to_none_and_values_map() {
        // 0 = noch nicht berechnet/eingegeben → None.
        let snap = telemetry_to_snapshot(fbw_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.v2_kt, None);
        assert_eq!(snap.vls_kt, None);
        assert_eq!(snap.vapp_kt, None);
        // FBW liefert kein V1/VR/VREF → bleibt None.
        assert_eq!(snap.v1_kt, None);
        assert_eq!(snap.vr_kt, None);
        assert_eq!(snap.vref_kt, None);

        let mut t = fbw_telemetry();
        t.fbw_vspeeds_v2 = 145.0;
        t.fbw_vspeeds_vls = 118.0;
        t.fbw_vspeeds_vapp = 136.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.v2_kt, Some(145.0));
        assert_eq!(snap.vls_kt, Some(118.0));
        assert_eq!(snap.vapp_kt, Some(136.0));
    }

    #[test]
    fn fbw_managed_dots_autobrake_and_spoilers() {
        let mut t = fbw_telemetry();
        t.fbw_fcu_spd_dot = 1.0;
        t.fbw_fcu_hdg_dot = 0.0;
        t.fbw_fcu_alt_managed = 1.0;
        t.fbw_autobrake_armed_mode = 2.0;
        t.fbw_spoilers_armed = 1.0;
        t.fbw_ground_spoilers_active = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.managed_speed, Some(true));
        assert_eq!(snap.managed_heading, Some(false));
        assert_eq!(snap.managed_altitude, Some(true));
        assert_eq!(snap.autobrake.as_deref(), Some("MED"));
        // ARMED: LVar ODER Standard — hier nur der LVar.
        assert_eq!(snap.spoilers_armed, Some(true));
        assert_eq!(snap.ground_spoilers_active, Some(true));

        // Autobrake 0 = DIS → None (bisherige Profil-Semantik).
        let snap = telemetry_to_snapshot(fbw_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.autobrake, None);
        assert_eq!(snap.ground_spoilers_active, Some(false));
    }

    #[test]
    fn fenix_premium_vspeeds_and_flex_zero_to_none() {
        // PERF-Page leer (0) → None, kein Phantom "V1 0 kt".
        let snap =
            telemetry_to_snapshot(fenix_premium_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.v1_kt, None);
        assert_eq!(snap.vr_kt, None);
        assert_eq!(snap.v2_kt, None);
        assert_eq!(snap.flex_temp_c, None);

        let mut t = fenix_premium_telemetry();
        t.fnx_perf_v1 = 142.0;
        t.fnx_perf_vr = 145.0;
        t.fnx_perf_v2 = 151.0;
        t.fnx_perf_flex = 62.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.v1_kt, Some(142.0));
        assert_eq!(snap.vr_kt, Some(145.0));
        assert_eq!(snap.v2_kt, Some(151.0));
        assert_eq!(snap.flex_temp_c, Some(62.0));
        // FLEX eingegeben heisst NICHT thrust_gate (nur INI liefert
        // die Lever-Gate-Flags).
        assert_eq!(snap.thrust_gate, None);
    }

    #[test]
    fn fenix_premium_caution_warning_managed_and_baro() {
        let mut t = fenix_premium_telemetry();
        t.fnx_master_caution = 1.0;
        t.fnx_master_warning = 0.0;
        t.fnx_fcu_spd_managed = 1.0;
        t.fnx_fcu_hdg_managed = 0.0;
        t.fnx_fcu_alt_managed = 1.0;
        t.fnx_baro_std = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.master_caution, Some(true));
        assert_eq!(snap.master_warning, Some(false));
        assert_eq!(snap.managed_speed, Some(true));
        assert_eq!(snap.managed_heading, Some(false));
        assert_eq!(snap.managed_altitude, Some(true));
        assert_eq!(snap.baro_std, Some(true));

        // Fenix LIEFERT die Lampen → Some(false), nicht None.
        let snap =
            telemetry_to_snapshot(fenix_premium_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.master_caution, Some(false));
        assert_eq!(snap.master_warning, Some(false));
        assert_eq!(snap.baro_std, Some(false));
    }

    #[test]
    fn ini_thrust_gate_priority_toga_over_flx_over_cl() {
        // TOGA gewinnt ueber alles.
        let mut t = ini_a350_telemetry();
        t.ini_lever_toga = 1.0;
        t.ini_lever_flex_mct = 1.0;
        t.ini_lever_cl = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.thrust_gate.as_deref(), Some("TOGA"));

        // FLX/MCT vor CL.
        let mut t = ini_a350_telemetry();
        t.ini_lever_flex_mct = 1.0;
        t.ini_lever_cl = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.thrust_gate.as_deref(), Some("FLX/MCT"));

        let mut t = ini_a350_telemetry();
        t.ini_lever_cl = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.thrust_gate.as_deref(), Some("CL"));

        // Kein Gate-Flag → None.
        let snap = telemetry_to_snapshot(ini_a350_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.thrust_gate, None);
    }

    #[test]
    fn ini_raw_fma_passthrough_as_hash_labels() {
        // Enum-Belegung unbekannt → Roh-Wert "#{n}", 0 → None. Gilt
        // fuer BEIDE INI-Profile (A350 + A340, gleiche LVars).
        for make in [ini_a350_telemetry, ini_a340_telemetry] {
            let mut t = make();
            t.ini_roll_mode = 3.0;
            t.ini_pitch_mode = 7.0;
            t.ini_throttle_mode = 2.0;
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(snap.fma_lateral_mode.as_deref(), Some("#3"));
            assert_eq!(snap.fma_vertical_mode.as_deref(), Some("#7"));
            assert_eq!(snap.fma_thrust_mode.as_deref(), Some("#2"));

            let snap = telemetry_to_snapshot(make(), Simulator::Msfs2024);
            assert_eq!(snap.fma_lateral_mode, None);
            assert_eq!(snap.fma_vertical_mode, None);
            assert_eq!(snap.fma_thrust_mode, None);
        }
    }

    #[test]
    fn ini_premium_vspeeds_warnings_and_ground_spoilers() {
        let mut t = ini_a340_telemetry();
        t.ini_v1 = 138.0;
        t.ini_vr = 142.0;
        t.ini_v2 = 148.0;
        t.ini_vls = 121.0;
        t.ini_vapp = 139.0;
        t.ini_vref = 134.0;
        t.ini_flex_temp = 58.0;
        t.ini_master_caution = 1.0;
        t.ini_master_warning = 0.0;
        t.ini_ground_spoilers = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.v1_kt, Some(138.0));
        assert_eq!(snap.vr_kt, Some(142.0));
        assert_eq!(snap.v2_kt, Some(148.0));
        assert_eq!(snap.vls_kt, Some(121.0));
        assert_eq!(snap.vapp_kt, Some(139.0));
        assert_eq!(snap.vref_kt, Some(134.0));
        assert_eq!(snap.flex_temp_c, Some(58.0));
        assert_eq!(snap.master_caution, Some(true));
        assert_eq!(snap.master_warning, Some(false));
        assert_eq!(snap.ground_spoilers_active, Some(true));

        // FMS leer → alles None (0-Suppression).
        let snap = telemetry_to_snapshot(ini_a350_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.v1_kt, None);
        assert_eq!(snap.vref_kt, None);
        assert_eq!(snap.flex_temp_c, None);
    }

    #[test]
    fn ini_a340_autobrake_level_hubhop_enum() {
        // HubHop: 3=MED, 4=MAX, 5=LO; 0 → None; unbekannt → "#{n}".
        let cases = [
            (0.0, None),
            (3.0, Some("MED")),
            (4.0, Some("MAX")),
            (5.0, Some("LO")),
            (7.0, Some("#7")),
        ];
        for (level, expected) in cases {
            let mut t = ini_a340_telemetry();
            t.ini_autobrake_level = level;
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(
                snap.autobrake.as_deref(),
                expected,
                "INI_AUTOBRAKE_LEVEL={level}"
            );
        }

        // A350-Profil nutzt den (A340-only) Level-LVar NICHT.
        let mut t = ini_a350_telemetry();
        t.ini_autobrake_level = 3.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autobrake, None);
    }

    #[test]
    fn md11_ap_state_enum_and_ats() {
        // AP_STATE dokumentiert: 0=Off, 1=AP1, 2=AP2, 3=both.
        for state in [1.0, 2.0, 3.0] {
            let mut t = md11_telemetry();
            t.md11_ap_state = state;
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(
                snap.autopilot_master,
                Some(true),
                "MD11_AP_STATE={state}"
            );
        }
        let snap = telemetry_to_snapshot(md11_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));

        // ATS: > 0 = engaged.
        let mut t = md11_telemetry();
        t.md11_ats_state = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(true));
        let snap = telemetry_to_snapshot(md11_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(false));
    }

    #[test]
    fn md11_afs_dash_sentinels_to_none() {
        // SPD/HDG dashen mit -999, V/S mit -9999 → None.
        let mut t = md11_telemetry();
        t.md11_afs_spd = -999.0;
        t.md11_afs_hdg = -999.0;
        t.md11_afs_vs = -9999.0;
        t.md11_afs_alt = 35000.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_speed_kt, None);
        assert_eq!(snap.fcu_selected_heading_deg, None);
        assert_eq!(snap.fcu_selected_vs_fpm, None);
        assert_eq!(snap.fcu_selected_altitude_ft, Some(35000));

        // Echte Werte mappen — inkl. negativem V/S (Descent).
        let mut t = md11_telemetry();
        t.md11_afs_spd = 250.0;
        t.md11_afs_hdg = 180.0;
        t.md11_afs_vs = -1500.0;
        t.md11_afs_alt = 12000.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_speed_kt, Some(250));
        assert_eq!(snap.fcu_selected_heading_deg, Some(180));
        assert_eq!(snap.fcu_selected_vs_fpm, Some(-1500));
        assert_eq!(snap.fcu_selected_altitude_ft, Some(12000));

        // ALT 0 = uninitialisiert → None.
        let snap = telemetry_to_snapshot(md11_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_altitude_ft, None);
    }

    /// v0.16.10 QS (Minor 8): AFS-Plausibilitaets-Gates — ein
    /// uninitialisierter LVar liest 0.0 (zwischen Dash-Sentinel und
    /// echtem Wert). HDG nur 1..=360, SPD nur >= 80.
    #[test]
    fn md11_afs_implausible_values_to_none() {
        // Uninitialisiert (0.0): HDG 0 und SPD 0 → None.
        let snap = telemetry_to_snapshot(md11_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_heading_deg, None);
        assert_eq!(snap.fcu_selected_speed_kt, None);

        // Unter den Plausibilitaets-Schwellen → None.
        let mut t = md11_telemetry();
        t.md11_afs_spd = 79.0;
        t.md11_afs_hdg = 361.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_speed_kt, None);
        assert_eq!(snap.fcu_selected_heading_deg, None);

        // Grenzwerte sind plausibel: SPD 80, HDG 360 und HDG 1.
        let mut t = md11_telemetry();
        t.md11_afs_spd = 80.0;
        t.md11_afs_hdg = 360.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_speed_kt, Some(80));
        assert_eq!(snap.fcu_selected_heading_deg, Some(360));
        let mut t = md11_telemetry();
        t.md11_afs_hdg = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_heading_deg, Some(1));
    }

    #[test]
    fn md11_vspeeds_and_raw_autobrake() {
        let mut t = md11_telemetry();
        t.md11_v1 = 148.0;
        t.md11_vr = 155.0;
        t.md11_v2 = 163.0;
        t.md11_autobrake_sw = 2.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.v1_kt, Some(148.0));
        assert_eq!(snap.vr_kt, Some(155.0));
        assert_eq!(snap.v2_kt, Some(163.0));
        // Selector-Enum undokumentiert → Roh-Wert "#{n}".
        assert_eq!(snap.autobrake.as_deref(), Some("#2"));

        let snap = telemetry_to_snapshot(md11_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.v1_kt, None);
        assert_eq!(snap.autobrake, None);
    }

    #[test]
    fn a346_reverser_any_of_four_ratios() {
        // Jede einzelne Ratio > 0.05 reicht.
        for set in [
            |t: &mut Telemetry| t.a346_eng1_rev_ratio = 0.8,
            |t: &mut Telemetry| t.a346_eng2_rev_ratio = 0.8,
            |t: &mut Telemetry| t.a346_eng3_rev_ratio = 0.8,
            |t: &mut Telemetry| t.a346_eng4_rev_ratio = 0.8,
        ] {
            let mut t = a346_telemetry();
            set(&mut t);
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(snap.reverser_deployed, Some(true));
        }

        // Idle-Jitter unterhalb der Schwelle zaehlt nicht.
        let mut t = a346_telemetry();
        t.a346_eng1_rev_ratio = 0.04;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.reverser_deployed, Some(false));
    }

    #[test]
    fn a346_premium_managed_flags_warnings_phase_and_spoilers() {
        let mut t = a346_telemetry();
        t.a346_fcu_spd_managed = 1.0;
        t.a346_fcu_hdg_managed = 0.0;
        t.a346_fcu_vs_managed = 1.0; // → managed_altitude (Approximation)
        t.a346_master_caution_light = 1.0;
        t.a346_master_warning_light = 0.0;
        t.a346_flight_phase = 4.0;
        t.a346_spoiler_lever_armed = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.managed_speed, Some(true));
        assert_eq!(snap.managed_heading, Some(false));
        assert_eq!(snap.managed_altitude, Some(true));
        assert_eq!(snap.master_caution, Some(true));
        assert_eq!(snap.master_warning, Some(false));
        // FMGC-Phase-Enum unbekannt → Roh-Wert "#{n}".
        assert_eq!(snap.flight_phase_aircraft.as_deref(), Some("#4"));
        // ARMED: Standard ODER Lever-LVar.
        assert_eq!(snap.spoilers_armed, Some(true));
        // Kein direkter Ground-Spoiler-Active-Flag auf der A346.
        assert_eq!(snap.ground_spoilers_active, None);
    }

    #[test]
    fn eng_n1_pct_generic_combustion_gated() {
        // Twin laeuft (plain Combustion), N1 auf 0-1-Skala → 2-er
        // Vektor, auf Prozent normalisiert.
        let mut t = Telemetry::default();
        t.eng1_firing = true;
        t.eng2_firing = true;
        t.n1_pct_1 = 0.66;
        t.n1_pct_2 = 0.65;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, Some(vec![66.0, 65.0]));

        // Combustion tot, aber N1 > 5 % (EX1-/N1-only-Addons) →
        // trotzdem erfasst; 0-100-Skala bleibt unveraendert.
        let mut t = Telemetry::default();
        t.n1_pct_1 = 72.9;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, Some(vec![72.9]));

        // Positionserhaltung: nur Engine 2 laeuft → Praefix [0, n1_2].
        let mut t = Telemetry::default();
        t.eng2_firing = true;
        t.n1_pct_2 = 0.4;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, Some(vec![0.0, 40.0]));

        // v0.16.10 QS (Minor 9): Combustion an, aber ALLE N1 unter
        // 5 % (tote N1-SimVars bei lebenden Combustion-Bits) → None
        // statt Bogus-[0.0]-Array.
        let mut t = Telemetry::default();
        t.eng1_combustion_ex1 = true;
        t.n1_pct_1 = 0.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, None);

        // Sobald EINE Engine >= 5 % liest, bleibt das Praefix-Array
        // inkl. der (noch) spulenden Engine erhalten.
        let mut t = Telemetry::default();
        t.eng1_combustion_ex1 = true;
        t.eng2_firing = true;
        t.n1_pct_1 = 0.02; // Spool-up-Beginn → 2 %
        t.n1_pct_2 = 0.4; // 40 %
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, Some(vec![2.0, 40.0]));

        // Alles aus → None (kein leerer/Null-Vektor).
        let snap = telemetry_to_snapshot(Telemetry::default(), Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, None);
    }

    #[test]
    fn eng_n1_pct_md11_prefers_display_exact_lvars() {
        // MD-11: display-exakte LVars gewinnen, sobald eine > 0 liest
        // — auch wenn die Standard-SimVars parallel liefern.
        let mut t = md11_telemetry();
        t.md11_eng1_n1 = 85.2;
        t.md11_eng2_n1 = 84.9;
        t.md11_eng3_n1 = 85.5;
        t.eng1_firing = true;
        t.n1_pct_1 = 80.0; // Standard weicht ab → LVar gewinnt
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, Some(vec![85.2, 84.9, 85.5]));

        // LVars (noch) tot → Standard-Heuristik als Fallback.
        let mut t = md11_telemetry();
        t.eng1_firing = true;
        t.eng2_firing = true;
        t.eng3_firing = true;
        t.n1_pct_1 = 81.0;
        t.n1_pct_2 = 80.0;
        t.n1_pct_3 = 82.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.eng_n1_pct, Some(vec![81.0, 80.0, 82.0]));
    }

    /// DIE kritische Phantom-Absicherung: ein Default-Profil-Aircraft,
    /// bei dem (theoretisch) ALLE neuen Premium-LVar-Slots Werte
    /// tragen, laesst saemtliche 26 Premium-Felder auf None und alle
    /// bestehenden Felder auf dem Standard-Pfad.
    #[test]
    fn premium_lvars_do_not_affect_default_profile() {
        let mut t = Telemetry::default();
        t.title = "Asobo C172".into();
        t.atc_model = "C172".into();
        // Gruppe A (Fenix).
        t.fnx_perf_v1 = 142.0;
        t.fnx_perf_vr = 145.0;
        t.fnx_perf_v2 = 151.0;
        t.fnx_perf_flex = 62.0;
        t.fnx_master_caution = 1.0;
        t.fnx_master_warning = 1.0;
        t.fnx_speedbrake_handle = 0.7;
        t.fnx_fcu_spd_managed = 1.0;
        t.fnx_fcu_hdg_managed = 1.0;
        t.fnx_fcu_alt_managed = 1.0;
        t.fnx_baro_std = 1.0;
        t.fnx_eng1_fire = 1.0;
        t.fnx_eng2_fire = 1.0;
        // Gruppe B (FBW).
        t.fbw_ap1_active = 1.0;
        t.fbw_ap2_active = 1.0;
        t.fbw_athr_status = 2.0;
        t.fbw_athr_mode = 7.0;
        t.fbw_fma_lateral = 20.0;
        t.fbw_fma_vertical = 51.0;
        t.fbw_fwc_phase = 6.0;
        t.fbw_vspeeds_v2 = 145.0;
        t.fbw_vspeeds_vls = 118.0;
        t.fbw_vspeeds_vapp = 136.0;
        t.fbw_autobrake_armed_mode = 3.0;
        t.fbw_flaps_handle_index = 2.0;
        t.fbw_spoilers_armed = 1.0;
        t.fbw_ground_spoilers_active = 1.0;
        t.fbw_fcu_spd_dot = 1.0;
        t.fbw_fcu_hdg_dot = 1.0;
        t.fbw_fcu_alt_managed = 1.0;
        // Gruppe C (INI).
        t.ini_roll_mode = 3.0;
        t.ini_pitch_mode = 7.0;
        t.ini_throttle_mode = 2.0;
        t.ini_v1 = 140.0;
        t.ini_vr = 144.0;
        t.ini_v2 = 150.0;
        t.ini_vls = 120.0;
        t.ini_vapp = 138.0;
        t.ini_vref = 132.0;
        t.ini_flex_temp = 55.0;
        t.ini_lever_toga = 1.0;
        t.ini_lever_flex_mct = 1.0;
        t.ini_lever_cl = 1.0;
        t.ini_flaps_handle_index = 3.0;
        t.ini_ground_spoilers = 1.0;
        t.ini_autobrake_engaged = 1.0;
        t.ini_master_caution = 1.0;
        t.ini_master_warning = 1.0;
        t.ini_fuel_flow1_kg = 2200.0;
        t.ini_fuel_flow2_kg = 2200.0;
        t.ini_fuel_flow3_kg = 2200.0;
        t.ini_fuel_flow4_kg = 2200.0;
        t.ini_autobrake_level = 3.0;
        // Gruppe D (A346).
        t.a346_flight_phase = 4.0;
        t.a346_fcu_spd_managed = 1.0;
        t.a346_fcu_hdg_managed = 1.0;
        t.a346_fcu_vs_managed = 1.0;
        t.a346_master_warning_light = 1.0;
        t.a346_master_caution_light = 1.0;
        t.a346_spd_brk_lever_pos = 0.8;
        t.a346_spoiler_lever_armed = 1.0;
        t.a346_eng1_rev_ratio = 0.9;
        t.a346_eng2_rev_ratio = 0.9;
        t.a346_eng3_rev_ratio = 0.9;
        t.a346_eng4_rev_ratio = 0.9;
        // Gruppe E (MD-11).
        t.md11_ap_state = 3.0;
        t.md11_ats_state = 1.0;
        t.md11_ats_clamp = 1.0;
        t.md11_afs_spd = 250.0;
        t.md11_afs_hdg = 180.0;
        t.md11_afs_alt = 35000.0;
        t.md11_afs_vs = -1500.0;
        t.md11_v1 = 148.0;
        t.md11_vr = 155.0;
        t.md11_v2 = 163.0;
        t.md11_eng1_n1 = 85.0;
        t.md11_eng2_n1 = 85.0;
        t.md11_eng3_n1 = 85.0;
        t.md11_autobrake_sw = 2.0;
        // Gruppe F (iFly, v0.16.11).
        t.ifly_cmd_a_light = 1.0;
        t.ifly_cmd_b_light = 1.0;
        t.ifly_at_arm_light = 1.0;
        t.ifly_master_caution_light = 1.0;
        t.ifly_fire_warning_light = 1.0;
        t.ifly_cabin_alt_warning_light = 1.0;
        t.ifly_eng1_reverser = 0.9;
        t.ifly_eng2_reverser = 0.9;
        t.ifly_speedbrakes_extended_light = 1.0;
        t.ifly_autobrake_sw = 3.0;
        // on_ground = true, damit auch das Boden-Gate des iFly-Ground-
        // Spoiler-Mappings beheizt ist (haerterer Negativ-Fall).
        t.on_ground = true;
        // Gruppe G (FSLabs, v0.16.14) — alle 17 Slots beheizt.
        t.fsl_ap1_light = 80.0;
        t.fsl_ap2_light = 80.0;
        t.fsl_athr_light = 80.0;
        t.fsl_appr_light = 80.0;
        t.fsl_loc_light = 80.0;
        t.fsl_fcu_spd = 250.0;
        t.fsl_fcu_hdg = 180.0;
        t.fsl_fcu_alt = 35000.0;
        t.fsl_fcu_vs = -1500.0;
        t.fsl_fcu_spd_managed = 1.0;
        t.fsl_fcu_hdg_managed = 1.0;
        t.fsl_fcu_alt_managed = 1.0;
        // Die *_DASHED-Slots bleiben BEWUSST 0.0 — fuer das Profile-
        // Gate ist das der HEISSE Fall: nicht-dashed + Wert > 0
        // wuerde bei kaputtem Gate Some(250)/Some(180) liefern (ein
        // beheiztes dashed=1 wuerde den Leak unsichtbar machen).
        t.fsl_autobrake_lo_light = 80.0;
        t.fsl_autobrake_med_light = 80.0;
        t.fsl_autobrake_max_light = 80.0;
        // v0.16.20: alle 13 neuen FSL-PREMIUM-Slots beheizt — der
        // kritischste Phantom-Schutz: parking_brake/engines_running
        // duerfen NICHT vom FSL-Pfad uebernommen werden, wenn das Profil
        // ein C172 ist.
        t.fsl_park_brake_switch = 1.0; // wuerde parking_brake=true erzwingen
        t.fsl_eng1_mstr_switch = 20.0; // wuerde engines_running=2 erzwingen
        t.fsl_eng2_mstr_switch = 20.0;
        t.fsl_wheel_chocks = 1.0;
        t.fsl_baro_std = 1.0; // wuerde baro_std=Some leaken
        t.fsl_master_caution = 1.0; // wuerde master_caution=Some leaken
        t.fsl_master_warning = 1.0; // wuerde master_warning=Some leaken
        t.fsl_spd_brk_lever = 10.0; // wuerde spoilers_armed/handle leaken
        t.fsl_engfire1_lt = 1.0; // wuerde master_warning leaken
        t.fsl_engfire2_lt = 1.0;
        t.fsl_eng1_anti_ice = 1.0; // wuerde engine_anti_ice leaken
        t.fsl_eng2_anti_ice = 1.0;
        t.fsl_xpdr_mode_switch = 20.0; // wuerde xpdr_mode_label leaken
        t.fsl_xpdr_on_off_switch = 20.0; // ON — wuerde das Gate beheizen (Review-Fund)

        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);

        // Alle 26 Premium-Felder bleiben None.
        assert_eq!(snap.fma_lateral_mode, None);
        assert_eq!(snap.fma_vertical_mode, None);
        assert_eq!(snap.fma_thrust_mode, None);
        assert_eq!(snap.flight_phase_aircraft, None);
        assert_eq!(snap.v1_kt, None);
        assert_eq!(snap.vr_kt, None);
        assert_eq!(snap.v2_kt, None);
        assert_eq!(snap.vapp_kt, None);
        assert_eq!(snap.vls_kt, None);
        assert_eq!(snap.vref_kt, None);
        assert_eq!(snap.flex_temp_c, None);
        assert_eq!(snap.thrust_gate, None);
        assert_eq!(snap.master_caution, None);
        assert_eq!(snap.master_warning, None);
        assert_eq!(snap.managed_speed, None);
        assert_eq!(snap.managed_heading, None);
        assert_eq!(snap.managed_altitude, None);
        assert_eq!(snap.reverser_deployed, None);
        assert_eq!(snap.ground_spoilers_active, None);
        assert_eq!(snap.eng_n1_pct, None);
        assert_eq!(snap.baro_std, None);
        assert_eq!(snap.fuel_per_tank_kg, None);
        assert_eq!(snap.below_gs_alert, None);
        assert_eq!(snap.cabin_altitude_warning, None);
        assert_eq!(snap.stab_out_of_trim, None);
        assert_eq!(snap.minimums_baro_ft, None);

        // Bestehende Felder bleiben auf dem Standard-Pfad.
        assert_eq!(snap.autopilot_master, Some(false)); // MD11/FBW/iFly/FSL-Slots leaken nicht
        assert_eq!(snap.autopilot_approach, Some(false)); // FSL-APPR/LOC-LEDs leaken nicht
        assert_eq!(snap.autothrottle_on, None);
        assert_eq!(snap.autobrake, None);
        assert_eq!(snap.spoilers_armed, Some(false)); // FBW/A346-ARMED leakt nicht
        assert_eq!(snap.fcu_selected_speed_kt, None); // MD11-AFS/FSL-FCU leaken nicht
        assert_eq!(snap.fcu_selected_altitude_ft, None);
        assert_eq!(snap.fcu_selected_heading_deg, None);
        assert_eq!(snap.fcu_selected_vs_fpm, None);

        // v0.16.20: die kritischen FSL-PREMIUM-Override-Felder bleiben
        // auf dem Standard-Pfad — KEIN Leak in ein C172.
        assert!(!snap.parking_brake); // FSL-PARK_BRAKE_Switch leakt nicht
        assert_eq!(snap.engines_running, 0); // FSL-ENG_MSTR leakt nicht
        assert_eq!(snap.engine_anti_ice, Some(false)); // FSL-Anti-Ice leakt nicht
        assert_eq!(snap.spoilers_armed, Some(false)); // FSL-SPD_BRK leakt nicht
        assert_eq!(snap.xpdr_mode_label, None); // FSL-XPDR-Modus leakt nicht
        // master_caution/master_warning/baro_std bereits oben als None
        // geprueft — die FSL-Slots duerfen sie nicht auf Some heben.
    }

    // ---- v0.16.11: iFly 737 MAX 8 Premium-Mappings ----

    /// Minimal iFly-Profil-Telemetry. Standard-SimVars bleiben auf
    /// ihren Defaults, damit jeder gemappte Wert eindeutig aus den
    /// `VC_*`-LVars stammt. atc_model traegt den nutzlosen generischen
    /// ATCCOM-B737-Token aus dem echten Paket.
    fn ifly_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "iFly 737-MAX8 (178Seat)".into();
        t.atc_model = "ATCCOM.AC_MODEL B737.0.text".into();
        t
    }

    #[test]
    fn ifly_ap_master_from_cmd_lights_with_standard_tiebreaker() {
        // CMD A allein → Master engaged.
        let mut t = ifly_telemetry();
        t.ifly_cmd_a_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // CMD B allein zaehlt ebenfalls.
        let mut t = ifly_telemetry();
        t.ifly_cmd_b_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // Beide LEDs aus → Master off.
        let t = ifly_telemetry();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));

        // Standard-SimVar gewinnt per ODER, falls das Addon ihn doch
        // treibt (gleiche Tiebreaker-Semantik wie A346/A350/MD-11).
        let mut t = ifly_telemetry();
        t.ap_master = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));
    }

    #[test]
    fn ifly_autothrottle_from_at_arm_light() {
        // ARM-Lampe an = armed ODER engaged (PMDG-NG3-AT-ARM-Semantik).
        let mut t = ifly_telemetry();
        t.ifly_at_arm_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(true));

        // Lampe aus (nach Disconnect) → ehrliches Some(false) — das
        // Profil LIEFERT die Quelle, anders als Default (None).
        let t = ifly_telemetry();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(false));
    }

    #[test]
    fn ifly_master_caution_and_fire_as_red_master_class() {
        // Caution-Lampe → master_caution, NICHT master_warning.
        let mut t = ifly_telemetry();
        t.ifly_master_caution_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.master_caution, Some(true));
        assert_eq!(snap.master_warning, Some(false));

        // 737 hat keine MASTER-WARNING-Lampe — Fire-Warn ist die rote
        // Master-Klasse (PMDG-Mapper-Konvention).
        let mut t = ifly_telemetry();
        t.ifly_fire_warning_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.master_warning, Some(true));
        assert_eq!(snap.master_caution, Some(false));
    }

    #[test]
    fn ifly_cabin_altitude_warning_maps_natively() {
        let mut t = ifly_telemetry();
        t.ifly_cabin_alt_warning_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.cabin_altitude_warning, Some(true));

        let t = ifly_telemetry();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.cabin_altitude_warning, Some(false));
    }

    #[test]
    fn ifly_reverser_threshold_filters_stowed_jitter() {
        // Unter der 0.1-Schwelle (stowed/Jitter) → nicht deployed.
        let mut t = ifly_telemetry();
        t.ifly_eng1_reverser = 0.05;
        t.ifly_eng2_reverser = 0.05;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.reverser_deployed, Some(false));

        // EIN Triebwerk ueber der Schwelle genuegt.
        let mut t = ifly_telemetry();
        t.ifly_eng2_reverser = 0.6;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.reverser_deployed, Some(true));
    }

    #[test]
    fn ifly_ground_spoilers_only_count_on_ground() {
        // Lampe an + am Boden → Ground-Spoiler aktiv.
        let mut t = ifly_telemetry();
        t.ifly_speedbrakes_extended_light = 1.0;
        t.on_ground = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.ground_spoilers_active, Some(true));

        // Lampe an, aber in der Luft = Flight-Spoiler/Speedbrake,
        // KEIN Ground-Spoiler.
        let mut t = ifly_telemetry();
        t.ifly_speedbrakes_extended_light = 1.0;
        t.on_ground = false;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.ground_spoilers_active, Some(false));

        // Am Boden ohne Lampe → false.
        let mut t = ifly_telemetry();
        t.on_ground = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.ground_spoilers_active, Some(false));
    }

    #[test]
    fn ifly_autobrake_passes_raw_enum_through() {
        // Selektor-Enum unbekannt → "#{n}" (Decode beim ersten
        // Live-Flug), 0 = uninitialisiert/OFF-Default → None.
        let mut t = ifly_telemetry();
        t.ifly_autobrake_sw = 3.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autobrake, Some("#3".to_string()));

        let t = ifly_telemetry();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autobrake, None);
    }

    // ---- v0.16.14: FSLabs A321 Premium-Mappings ----

    /// Minimal FSL-Profil-Telemetry. Standard-SimVars bleiben auf
    /// ihren Defaults, damit jeder gemappte Wert eindeutig aus den
    /// HubHop-LVars stammt. neo-Title aus der aircraft.cfg; der
    /// ICAO-Designator spielt fuer die Detection keine Rolle.
    fn fsl_telemetry() -> Telemetry {
        let mut t = Telemetry::default();
        t.title = "FSLabs A321-NEO LEAP DLH D-AIOA".into();
        t.atc_model = "A21N".into();
        t
    }

    #[test]
    fn fsl_ap_master_from_fcu_led_brightness_with_threshold() {
        // AP1-LED hell (> 10) → Master engaged.
        let mut t = fsl_telemetry();
        t.fsl_ap1_light = 80.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // AP2 allein zaehlt ebenfalls.
        let mut t = fsl_telemetry();
        t.fsl_ap2_light = 80.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));

        // Rest-Glimmen UNTER der Schwelle (<= 10) zaehlt nicht.
        let mut t = fsl_telemetry();
        t.fsl_ap1_light = 5.0;
        t.fsl_ap2_light = 5.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));

        // Beide LEDs aus → Master off.
        let t = fsl_telemetry();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(false));

        // Standard-SimVar gewinnt per ODER, falls das Addon ihn doch
        // treibt (gleiche Tiebreaker-Semantik wie A346/A350/iFly).
        let mut t = fsl_telemetry();
        t.ap_master = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_master, Some(true));
    }

    #[test]
    fn fsl_autothrottle_from_athr_led() {
        // A/THR-LED hell → on (armed ODER active, Airbus-FCU-Semantik).
        let mut t = fsl_telemetry();
        t.fsl_athr_light = 80.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(true));

        // Unter der Schwelle / aus → ehrliches Some(false) — das
        // Profil LIEFERT die Quelle, anders als Default (None).
        let mut t = fsl_telemetry();
        t.fsl_athr_light = 5.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autothrottle_on, Some(false));
    }

    #[test]
    fn fsl_approach_from_appr_or_loc_led() {
        // APPR-LED → Approach-Mode.
        let mut t = fsl_telemetry();
        t.fsl_appr_light = 80.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_approach, Some(true));

        // LOC allein (lateraler Capture ohne Glideslope) zaehlt auch.
        let mut t = fsl_telemetry();
        t.fsl_loc_light = 80.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_approach, Some(true));

        // Beide aus → false; Standard-SimVar gewinnt per ODER.
        let t = fsl_telemetry();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_approach, Some(false));
        let mut t = fsl_telemetry();
        t.ap_approach = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autopilot_approach, Some(true));
    }

    #[test]
    fn fsl_managed_flags_from_fcu_lvars() {
        // Jedes Flag mappt einzeln; ungesetzte bleiben ehrlich false
        // (das Profil liefert die Quelle).
        let mut t = fsl_telemetry();
        t.fsl_fcu_spd_managed = 1.0;
        t.fsl_fcu_alt_managed = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.managed_speed, Some(true));
        assert_eq!(snap.managed_heading, Some(false));
        assert_eq!(snap.managed_altitude, Some(true));

        let mut t = fsl_telemetry();
        t.fsl_fcu_hdg_managed = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.managed_speed, Some(false));
        assert_eq!(snap.managed_heading, Some(true));
        assert_eq!(snap.managed_altitude, Some(false));
    }

    #[test]
    fn fsl_fcu_dashed_windows_map_to_none() {
        // Selected-Werte: SPD/HDG nur ohne Dash, ALT > 0, V/S != 0.
        let mut t = fsl_telemetry();
        t.fsl_fcu_spd = 250.0;
        t.fsl_fcu_hdg = 180.0;
        t.fsl_fcu_alt = 35000.0;
        t.fsl_fcu_vs = -1500.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_speed_kt, Some(250));
        assert_eq!(snap.fcu_selected_heading_deg, Some(180));
        assert_eq!(snap.fcu_selected_altitude_ft, Some(35000));
        assert_eq!(snap.fcu_selected_vs_fpm, Some(-1500));

        // Dash-Flags gesetzt (Fenster zeigt "---"/managed) → None,
        // auch wenn der Werte-LVar noch einen Restwert traegt.
        let mut t = fsl_telemetry();
        t.fsl_fcu_spd = 250.0;
        t.fsl_fcu_spd_dashed = 1.0;
        t.fsl_fcu_hdg = 180.0;
        t.fsl_fcu_hdg_dashed = 1.0;
        t.fsl_fcu_alt = 35000.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_speed_kt, None);
        assert_eq!(snap.fcu_selected_heading_deg, None);
        // ALT kennt kein Dash — bleibt gemappt.
        assert_eq!(snap.fcu_selected_altitude_ft, Some(35000));

        // Uninitialisiert (alles 0): ALT 0 → None, V/S 0 → None
        // (kein dediziertes VS-Dash-Flag katalogisiert — 0 ist
        // dashed/Level-off/uninitialisiert, ehrlich None).
        let t = fsl_telemetry();
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.fcu_selected_speed_kt, None);
        assert_eq!(snap.fcu_selected_heading_deg, None);
        assert_eq!(snap.fcu_selected_altitude_ft, None);
        assert_eq!(snap.fcu_selected_vs_fpm, None);
    }

    #[test]
    fn fsl_autobrake_from_button_state_with_priority() {
        // Review-Fund v0.16.20: die AUTO-BRK-LVars sind STATE, nicht
        // Helligkeit — `!= 0` = selektiert (Skript-Idiom `0 != if{ ... }`).
        // Genau ein State != 0 → sein Label. Auch ein kleiner Wert (1)
        // zaehlt jetzt (frueher unter der >50-Schwelle verworfen).
        for (lo, med, max, want) in [
            (1.0, 0.0, 0.0, "LO"),
            (0.0, 1.0, 0.0, "MED"),
            (0.0, 0.0, 1.0, "MAX"),
        ] {
            let mut t = fsl_telemetry();
            t.fsl_autobrake_lo_light = lo;
            t.fsl_autobrake_med_light = med;
            t.fsl_autobrake_max_light = max;
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(snap.autobrake, Some(want.to_string()));
        }

        // (Theoretisch) mehrere gleichzeitig → die erste gewinnt
        // (LO → MED → MAX).
        let mut t = fsl_telemetry();
        t.fsl_autobrake_lo_light = 1.0;
        t.fsl_autobrake_max_light = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.autobrake, Some("LO".to_string()));

        // Alles 0 (keine Stufe selektiert) → None (kein erfundenes
        // OFF-Label).
        let snap = telemetry_to_snapshot(fsl_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.autobrake, None);
    }

    #[test]
    fn fsl_undocumented_warning_fields_stay_none() {
        // Reverser, Ground-Spoiler und Cabin-Alt sind fuer FSL extern +
        // nicht katalogisiert — bleiben ehrlich None (kein erfundenes
        // Some(false)). Master Caution/Warning werden ab v0.16.20 ueber
        // die echten Lampen-LVars gemappt → sind nun Some (s. eigene
        // Tests), nicht mehr None.
        let snap = telemetry_to_snapshot(fsl_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.reverser_deployed, None);
        assert_eq!(snap.ground_spoilers_active, None);
        assert_eq!(snap.cabin_altitude_warning, None);
    }

    // ---- v0.16.20: FSLabs A321 PREMIUM-Mappings (echte LVars) ----

    #[test]
    fn fsl_parking_brake_from_real_switch_overrides_faked_simvar() {
        // FSL faelscht den Standard-SimVar; der echte Schalter gewinnt.
        // Skript: `0 ==` = released → !=0 = SET.
        let mut t = fsl_telemetry();
        t.fsl_park_brake_switch = 0.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert!(!snap.parking_brake); // 0 → released

        let mut t = fsl_telemetry();
        t.fsl_park_brake_switch = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert!(snap.parking_brake); // !=0 → set

        // Selbst wenn der gefaelschte Standard-SimVar False liefert
        // (parking_brake-Default), gewinnt der echte Schalter.
        let mut t = fsl_telemetry();
        t.parking_brake = false; // FSL-Fake
        t.fsl_park_brake_switch = 1.0; // Realitaet: gesetzt
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert!(snap.parking_brake);
    }

    #[test]
    fn fsl_engines_running_from_masters_overrides_faked_combustion() {
        // FSL faelscht COMBUSTION → Standard-Pfad las am Gate konstant 2.
        // Master-Schalter: Skript-Schwellen 20>=ON, 10<=OFF → >= 15 = ON.
        // Beide Master an → 2.
        let mut t = fsl_telemetry();
        t.fsl_eng1_mstr_switch = 20.0;
        t.fsl_eng2_mstr_switch = 20.0;
        // gefaelschte COMBUSTION wie in Peters Log:
        t.eng1_firing = true;
        t.eng2_firing = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 2);

        // Nur Engine 1 an → 1.
        let mut t = fsl_telemetry();
        t.fsl_eng1_mstr_switch = 20.0;
        t.fsl_eng2_mstr_switch = 0.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 1);

        // Beide Master AUS (am Gate) → 0, OBWOHL COMBUSTION faked 2 meldet.
        // Das ist das echte Ankunftssignal fuer die Auto-End-FSM.
        let mut t = fsl_telemetry();
        t.fsl_eng1_mstr_switch = 0.0;
        t.fsl_eng2_mstr_switch = 0.0;
        t.eng1_firing = true; // FSL-Fake
        t.eng2_firing = true; // FSL-Fake
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 0);

        // OFF-Stellung als Restwert <= 10 zaehlt NICHT als laufend.
        let mut t = fsl_telemetry();
        t.fsl_eng1_mstr_switch = 10.0;
        t.fsl_eng2_mstr_switch = 10.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engines_running, 0);
    }

    #[test]
    fn fsl_baro_std_from_efis_lvar() {
        let mut t = fsl_telemetry();
        t.fsl_baro_std = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.baro_std, Some(true));

        let t = fsl_telemetry(); // 0 → QNH/QFE
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.baro_std, Some(false));
    }

    #[test]
    fn fsl_master_caution_and_warning_from_buttons() {
        let mut t = fsl_telemetry();
        t.fsl_master_caution = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.master_caution, Some(true));
        assert_eq!(snap.master_warning, Some(false));

        let mut t = fsl_telemetry();
        t.fsl_master_warning = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.master_caution, Some(false));
        assert_eq!(snap.master_warning, Some(true));

        // Beide aus → ehrliches Some(false), nicht None.
        let snap = telemetry_to_snapshot(fsl_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.master_caution, Some(false));
        assert_eq!(snap.master_warning, Some(false));
    }

    #[test]
    fn fsl_engine_fire_lamp_raises_master_warning() {
        // Triebwerks-Feuer ist eine rote Master-Klasse-Bedingung →
        // hebt master_warning, auch ohne die Warning-Button-Lampe.
        let mut t = fsl_telemetry();
        t.fsl_engfire1_lt = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.master_warning, Some(true));

        let mut t = fsl_telemetry();
        t.fsl_engfire2_lt = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.master_warning, Some(true));
    }

    #[test]
    fn fsl_engine_anti_ice_from_buttons() {
        let mut t = fsl_telemetry();
        t.fsl_eng1_anti_ice = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engine_anti_ice, Some(true));

        let mut t = fsl_telemetry();
        t.fsl_eng2_anti_ice = 1.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.engine_anti_ice, Some(true));

        // Beide aus → Some(false).
        let snap = telemetry_to_snapshot(fsl_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.engine_anti_ice, Some(false));
    }

    #[test]
    fn fsl_speedbrake_handle_and_armed_use_standard_simvar() {
        // v0.16.21: das FSL-Profil hat KEINEN Lever-basierten Override
        // mehr — weder fuer `spoilers_handle_position` (schon v0.16.20)
        // noch fuer `spoilers_armed`. Beide kommen jetzt aus der
        // Standard-SimVar. Grund (Live-Befund): das v0.16.20-Fenster
        // `VC_PED_SPD_BRK_LEVER` 5..=15 fing die NEUTRALE Lever-Stellung
        // → `spoilers_armed` las den ganzen Flug True (985/985 Samples).
        // Kosmetisch (kein FSM/Scoring-Einfluss), aber falsch.

        // Lever eingefahren (0), Standard-Handle 0 → Handle 0, nicht armed.
        let snap = telemetry_to_snapshot(fsl_telemetry(), Simulator::Msfs2024);
        assert_eq!(snap.spoilers_handle_position, Some(0.0));
        assert_eq!(snap.spoilers_armed, Some(false));

        // Lever in der frueher als "armed" gewerteten Stellung (10) darf
        // `spoilers_armed` jetzt NICHT mehr setzen — nur die Standard-
        // SimVar entscheidet. Standard-armed=false → armed=false.
        let mut t = fsl_telemetry();
        t.fsl_spd_brk_lever = 10.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.spoilers_armed, Some(false)); // Lever armt NICHT mehr
        assert_eq!(snap.spoilers_handle_position, Some(0.0)); // Standard, NICHT 0.2

        // Standard-SimVar `SPOILERS ARMED` true → armed true (Quelle
        // ist jetzt ausschliesslich der Standard).
        let mut t = fsl_telemetry();
        t.spoilers_armed = true;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.spoilers_armed, Some(true));

        // Standard-Handle deployed (z.B. 1.0 beim Rollout) fliesst
        // unveraendert durch — der Lever-Wert aendert daran nichts.
        let mut t = fsl_telemetry();
        t.spoilers_handle_position = 1.0; // FSL treibt das korrekt
        t.fsl_spd_brk_lever = 0.0;
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.spoilers_handle_position, Some(1.0));
    }

    #[test]
    fn fsl_xpdr_mode_from_switch_enum() {
        // Skript: Rohwert /10 → 0=STBY, 1=TA, 2=TA-RA. Der XPDR muss
        // dazu eingeschaltet sein (ON/OFF-Switch != OFF) — sonst gated
        // das Label auf None (s. eigener Test).
        for (raw, want) in [(0.0, "STBY"), (10.0, "TA"), (20.0, "TA-RA")] {
            let mut t = fsl_telemetry();
            t.fsl_xpdr_on_off_switch = 20.0; // ON
            t.fsl_xpdr_mode_switch = raw;
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(snap.xpdr_mode_label, Some(want.to_string()));
        }
    }

    #[test]
    fn fsl_xpdr_label_gated_off_when_transponder_off() {
        // Review-Fund v0.16.20: bei XPDR OFF (ON/OFF-Switch Rohwert 0)
        // liefert das Label None, auch wenn der MODE-Switch eine
        // Stellung haelt.
        let mut t = fsl_telemetry();
        t.fsl_xpdr_on_off_switch = 0.0; // OFF
        t.fsl_xpdr_mode_switch = 20.0; // MODE = TA-RA, aber XPDR aus
        let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
        assert_eq!(snap.xpdr_mode_label, None);

        // AUTO (10) und ON (20) labeln beide.
        for on_off in [10.0, 20.0] {
            let mut t = fsl_telemetry();
            t.fsl_xpdr_on_off_switch = on_off;
            t.fsl_xpdr_mode_switch = 0.0; // STBY
            let snap = telemetry_to_snapshot(t, Simulator::Msfs2024);
            assert_eq!(snap.xpdr_mode_label, Some("STBY".to_string()));
        }
    }
}
