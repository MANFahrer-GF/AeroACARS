//! v0.12.2 — X-Plane Aircraft-DataRef-Profile (Study-Level-Add-ons).
//!
//! Spec: docs/spec/v0.12.2-xplane-aircraft-dataref-profiles.md
//!
//! Study-level X-Plane add-ons (Hot-Start CL650, ToLiss, FlightFactor …)
//! run cockpit/system functions through their **own** datarefs and don't
//! always drive the standard `sim/...` ones. The CL650 e.g. never drives
//! `sim/flightmodel2/controls/flap_handle_deploy_ratio`, so AeroACARS
//! could not see the flaps (GSG225 bug → LANDING CONFIG "INCOMPLETE").
//!
//! An `XplaneAircraftProfile` swaps the dataref source for specific
//! `FieldId`s when a known study-level aircraft is detected. Aircraft
//! without a profile keep the static `CATALOG` unchanged (LE5).

use crate::dataref::{FieldId, CATALOG};

/// Tolerance for the near-integer check in `ValueMapping::DetentTable`
/// (LE7). A documented integer detent must arrive close to a whole
/// number; `1.51` is not a clean detent and must not snap to `2`.
pub const DETENT_TOLERANCE: f32 = 0.05;

/// Spielraum fuer `ValueMapping::ArmedAt`. Die FF777 lieferte in zwei
/// Fluegen genau 0.5; eng gehalten, damit ein Hebel, der im Flug durch die
/// Mitte faehrt, nicht als ARMED zaehlt.
pub const ARMED_TOLERANCE: f32 = 0.01;

/// How a raw RREF value is mapped onto the internal field value.
#[derive(Debug, Clone, Copy)]
pub enum ValueMapping {
    /// Take the raw value 1:1 (e.g. int 0/1 booleans).
    Passthrough,
    /// Map an integer detent index → internal `f32` via a lookup table.
    /// Out-of-range / non-integer / non-finite → no value (LE7).
    DetentTable(&'static [f32]),
    /// v0.12.6: Map a raw value in **degrees** onto the internal field
    /// via a nearest-degree lookup `(degrees, ratio)`. For add-ons whose
    /// flaps dataref reports the commanded angle directly (Rotate MD-11:
    /// `flaps_cmd_pos_deg` = 0/15/28/35/50) instead of a lever index or
    /// a 0..1 ratio. Non-finite → no value; empty table → no value.
    DegreeTable(&'static [(f32, f32)]),
    /// 05.10.2026: Hebel, dessen ARMED-Raste auf diesem Wert steht statt
    /// auf X-Planes -0.5. Ein Wert genau dort wird zu -0.5 (ARMED nach
    /// DataRefs.txt), alles andere bleibt wie geliefert.
    ArmedAt(f32),
    /// 25.10.2026: Rohwert plus festen Versatz — fuer Waehler, deren Skala
    /// gegenueber dem Grundeintrag verschoben ist (FF757/767-Autobrake
    /// beginnt bei RTO = -1, die FF777 bei 0).
    Versatz(f32),
    /// 28.10.2026: wie `DetentTable`, die Tabelle beginnt aber bei diesem
    /// Rohwert statt bei 0 — fuer Dreifachschalter mit -1/0/1 (CL650-
    /// Schilder: -1 ON, 0 OFF, 1 AUTO laut P_-Profilen v2.0).
    DetentTableAb(f32, &'static [f32]),
    /// 28.10.2026: Lampe als Schalter — Rohwert ueber der Schwelle ergibt
    /// den zweiten Wert, sonst 0 (Felis-Schilder: Leuchtkraft > 0.01 = an,
    /// Schwelle aus den P_-Profilen).
    Schwelle(f32, f32),
}

impl ValueMapping {
    /// Map a raw RREF `f32` to the internal field value. Returns `None`
    /// when the override should yield **no** value — the caller then
    /// leaves the field at its standard/`None` state (LE7: no panic,
    /// no silent `1.0`).
    pub fn map(&self, raw: f32) -> Option<f32> {
        match self {
            ValueMapping::Passthrough => Some(raw),
            ValueMapping::DetentTable(table) => {
                if !raw.is_finite() {
                    tracing::warn!(raw, "profile DetentTable: non-finite value ignored");
                    return None;
                }
                let rounded = raw.round();
                if (raw - rounded).abs() > DETENT_TOLERANCE {
                    tracing::debug!(raw, "profile DetentTable: not a clean integer detent");
                    return None;
                }
                let idx = rounded as i32;
                if idx < 0 || idx as usize >= table.len() {
                    tracing::debug!(raw, idx, "profile DetentTable: detent out of range");
                    return None;
                }
                Some(table[idx as usize])
            }
            ValueMapping::DegreeTable(table) => {
                if !raw.is_finite() {
                    tracing::warn!(raw, "profile DegreeTable: non-finite value ignored");
                    return None;
                }
                // Nearest documented detent — `flaps_cmd_pos_deg` is the
                // commanded angle, so it sits on a detent value; this
                // also tolerates a tiny float wobble.
                table
                    .iter()
                    .min_by(|a, b| {
                        (raw - a.0)
                            .abs()
                            .partial_cmp(&(raw - b.0).abs())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|&(_, ratio)| ratio)
            }
            ValueMapping::ArmedAt(raste) => {
                if !raw.is_finite() {
                    return None;
                }
                Some(if (raw - raste).abs() < ARMED_TOLERANCE {
                    -0.5
                } else {
                    raw
                })
            }
            ValueMapping::Versatz(v) => raw.is_finite().then_some(raw + v),
            ValueMapping::DetentTableAb(ab, table) => ValueMapping::DetentTable(table).map(raw - ab),
            ValueMapping::Schwelle(schwelle, an) => {
                raw.is_finite().then_some(if raw > *schwelle { *an } else { 0.0 })
            }
        }
    }
}

/// One dataref-source override: read `field` from `dataref` (an
/// aircraft-specific dataref) instead of the standard `CATALOG` entry,
/// applying `mapping`.
#[derive(Debug, Clone, Copy)]
pub struct DatarefOverride {
    pub field: FieldId,
    pub dataref: &'static str,
    pub mapping: ValueMapping,
}

/// A per-aircraft dataref profile (LE2).
#[derive(Debug, Clone, Copy)]
pub struct XplaneAircraftProfile {
    /// Human-readable name (logging only).
    pub name: &'static str,
    /// Substrings that must **all** appear (case-insensitive) in the
    /// `aircraft_title` for the title-match stage (LE1 stage 1).
    pub title_match: &'static [&'static str],
    /// Signature dataref for the probe stage (LE1 stage 2): if X-Plane
    /// returns a value for it, the aircraft is this profile's aircraft.
    pub probe_dataref: &'static str,
    /// Nur mit Titel UND bestaetigter Probe aktiv. Fuer Profile, deren
    /// Probe zu einer ganzen Herstellerfamilie gehoert (FlightFactor
    /// `1-sim/…`) und deren Titel auch fremde Muster tragen.
    pub titel_und_probe: bool,
    /// Dataref-source overrides applied while this profile is active.
    pub overrides: &'static [DatarefOverride],
}

impl XplaneAircraftProfile {
    /// LE1 stage 1: does `title` contain every `title_match` substring?
    pub fn matches_title(&self, title: &str) -> bool {
        if self.title_match.is_empty() {
            return false;
        }
        let lower = title.to_lowercase();
        self.title_match
            .iter()
            .all(|needle| lower.contains(&needle.to_lowercase()))
    }
}

/// CL650 flap-lever detent → internal `flaps_position`. Verified against
/// the Hot-Start CL650 documentation (`Wires.txt`, module `FLAP_IND`):
/// lever 0/1/2/3 = flaps 0°/20°/30°/45°. The internal LANDING-CONFIG
/// check is `flaps_position >= 0.70`, so lever 2 (Flaps 30) and 3
/// (Flaps 45) count as landing config, lever 0/1 (Flaps 0/20) do not.
pub const CL650_FLAP_DETENTS: &[f32] = &[0.0, 0.45, 0.80, 1.0];

/// v0.12.6: Rotate MD-11 flap detents → internal `flaps_position` ratio.
/// `Rotate/aircraft/systems/flaps_cmd_pos_deg` reports the commanded
/// flap angle in degrees (0/15/28/35/50). Die MD-11 treibt — wie der
/// CL650 — die Standard-Flaps-DataRef nicht; ohne dieses Mapping bleibt
/// `flaps_position` den ganzen Flug 0.0 → LANDING-CONFIG „INCOMPLETE".
/// Der interne Check ist `flaps_position >= 0.70`: Flaps 35 + 50 (die
/// MD-11-Landeklappen) zählen als Landing-Config, 0/15/28 nicht.
pub const MD11_FLAP_DEGREES: &[(f32, f32)] = &[
    (0.0, 0.0),
    (15.0, 0.30),
    (28.0, 0.55),
    (35.0, 0.80),
    (50.0, 1.0),
];

/// All known aircraft profiles (LE2). The first one whose detection
/// matches wins.
pub const PROFILES: &[XplaneAircraftProfile] = &[
    // Hot-Start Challenger 650 (X-Aviation). GSG225-Befund: treibt die
    // Standard-Flaps-DataRef nicht.
    XplaneAircraftProfile {
        name: "Hot Start CL650",
        title_match: &["challenger 650", "x-aviation"],
        probe_dataref: "abus/CL650/ARINC429/L-DCU-7/words/FCTL/0/FLAPS_LVR",
        titel_und_probe: false,
        overrides: &[
            DatarefOverride {
                field: FieldId::FlapsHandle,
                dataref: "abus/CL650/ARINC429/L-DCU-7/words/FCTL/0/FLAPS_LVR",
                mapping: ValueMapping::DetentTable(CL650_FLAP_DETENTS),
            },
            DatarefOverride {
                field: FieldId::BatteryMaster,
                dataref: "abus/CL650/modules/DC_ELEC/0/wires/BATT_CTRL_PWR",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::LightBeacon,
                dataref: "CL650/overhead/ext_lts/beacon",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::LightTaxi,
                dataref: "CL650/overhead/land_lts/recog_taxi",
                mapping: ValueMapping::Passthrough,
            },
            // 28.10.2026 — P_-Profile v2.0 (Michel). Live-Abgleich: im
            // letzten CL650-Flug (19.10.) blieben Parkbremse und alle AP-
            // Felder den ganzen Flug aus. Parkbremse = Hebelstellung 0/1;
            // AP aus den FCP-Lampen der Kapitaensseite (> 0.01 = an, ueber
            // die Lampenfelder der FF777, die als ODER in die AP-Felder
            // gehen). Schilder -1 ON / 0 OFF / 1 AUTO (Tastenbilder; laut
            // Michel vom baugleichen Notlichtschalter abgeleitet, am Zeichen
            // selbst noch nicht bestaetigt).
            DatarefOverride {
                field: FieldId::ParkingBrake,
                dataref: "CL650/pedestal/park_brake_value",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777ApL,
                dataref: "CL650/lamps/glareshield/FCP/ap_eng_1",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777HdgHold,
                dataref: "CL650/lamps/glareshield/FCP/hdg_1",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777AltHold,
                dataref: "CL650/lamps/glareshield/FCP/alt_1",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777Lnav,
                dataref: "CL650/lamps/glareshield/FCP/nav_1",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777App,
                dataref: "CL650/lamps/glareshield/FCP/appr_1",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777At,
                dataref: "CL650/lamps/glareshield/ats_L",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::FfAnschnallzeichen,
                dataref: "CL650/overhead/signs/seatbelt_value",
                mapping: ValueMapping::DetentTableAb(-1.0, CL650_SCHILDER),
            },
            DatarefOverride {
                field: FieldId::FfRauchverbot,
                dataref: "CL650/overhead/signs/no_smoking_value",
                mapping: ValueMapping::DetentTableAb(-1.0, CL650_SCHILDER),
            },
        ],
    },
    // v0.12.6 — Rotate MD-11 (X-Plane 12). Pilot-Befund (Michel/GSG):
    // treibt die Standard-Flaps-DataRef nicht → `flaps_position` blieb
    // den ganzen Flug 0.0. Eigene DataRef in Grad statt Hebel-Index.
    XplaneAircraftProfile {
        name: "Rotate MD-11",
        title_match: &["rotate", "md-11"],
        probe_dataref: "Rotate/aircraft/systems/flaps_cmd_pos_deg",
        titel_und_probe: false,
        overrides: &[
            DatarefOverride {
                field: FieldId::FlapsHandle,
                dataref: "Rotate/aircraft/systems/flaps_cmd_pos_deg",
                mapping: ValueMapping::DegreeTable(MD11_FLAP_DEGREES),
            },
            // 28.10.2026 — P_-Profile v2.0 (Michel), Beta, alles noch nicht
            // im Sim bestaetigt. Schilder 0 OFF / 1 AUTO / 2 ON und Nose 0 OFF
            // / 1 TAXI / 2 T/O aus den Tastenbildern; Taxilicht brennt in
            // TAXI und T/O. AP: das FCP hat keine Modus-Lampen; Michel nennt
            // aus `MD-11-core.xpl` `fgs_autoflight` (AUTOFLIGHT) und
            // `afs_appr_engaged` (APPR/LAND), Werte ungemessen — > 0.01 = an
            // ueber die Lampenfelder.
            DatarefOverride {
                field: FieldId::FfAnschnallzeichen,
                dataref: "Rotate/aircraft/controls/seatbelts_lts",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::FfRauchverbot,
                dataref: "Rotate/aircraft/controls/nosmoke_lts",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::LightTaxi,
                dataref: "Rotate/aircraft/controls/nose_lts",
                mapping: ValueMapping::DetentTable(&[0.0, 1.0, 1.0]),
            },
            DatarefOverride {
                field: FieldId::Ff777ApL,
                dataref: "Rotate/aircraft/controls/fgs_autoflight",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777App,
                dataref: "Rotate/aircraft/systems/afs_appr_engaged",
                mapping: ValueMapping::Passthrough,
            },
        ],
    },
    // v0.17.x (#aircraft-scan): Laminar/Zibo Boeing 737-800(X). Die
    // `laminar/B738/autopilot/*_status`-Datarefs sind X-Plane-KERN (Laminars
    // Default-737) — sie gelten fuer den Standard-737 UND den Zibo (der Zibo
    // erbt X-Planes 737-Systeme). Belegt ueber das Scan-Tool: der Standard-
    // 737-Upload zeigt sie NICHT als Ordner-Strings (stecken im XP-Binary),
    // waehrend der Zibo sie in `zibomod.xpl` referenziert. Das Paket bringt
    // `B738_Datarefs.txt` mit ("AUTOPILOT LIGHTS (read only)") als Doku.
    //
    // Wir biegen die AP-MODUS-Felder auf die 737-echten MCP-Annunciator-
    // Lights (0/1) — authentischer als die generischen sim/cockpit2-Status.
    //
    // ApMaster (01.10.2026, Michel THY 372, Zibo, Plugin 1.0): die alte
    // Annahme „`servos_on` wird vom XP-Kern-Autopilot getrieben" war falsch —
    // der Zibo fliegt mit eigenem Autopiloten, `servos_on` blieb 2,5 h auf 0,
    // obwohl die Hoehe auf FL360 ueber 70 min nur 8,5 ft streute. Jetzt die
    // CMD-A-Lampe (im Zibo-Scan belegt, `B738_Datarefs.txt` „AUTOPILOT
    // LIGHTS"). CMD B kommt als eigene Quelle `B738CmdB` dazu
    // (`dataref.rs`); der Autopilot gilt als an, wenn A ODER B leuchtet —
    // auch CMD B allein (Copilot fliegt). Die CMD-A-Lampe gilt als Profil-
    // Ersetzung sofort; die Katalogquelle CMD B wie jeder `laminar/…`-Name
    // erst nach ihrem ersten Wert ≠ 0 (`plugin2_ziel::ist_kern`). Der
    // Standard-737 von X-Plane 12 hat dieselben Lua-Datarefs (geprueft
    // 01.10.2026: cmd_a/b_status, taxi_light_brightness_pos,
    // capt_probes_pos; kein logo_light → Logo bleibt dort = Nav).
    XplaneAircraftProfile {
        name: "Laminar/Zibo 737-800",
        title_match: &["boeing", "737-800"],
        probe_dataref: "laminar/B738/autopilot/cmd_a_status",
        titel_und_probe: false,
        overrides: &[
            DatarefOverride {
                field: FieldId::ApMaster,
                dataref: "laminar/B738/autopilot/cmd_a_status",
                mapping: ValueMapping::Passthrough,
            },
            // Zibo-Audit 01.10.2026 (Michel THY 372): Taxilicht und
            // Sonden-Heizung standen den ganzen Flug auf „aus“ — der Zibo
            // setzt die Standard-Schalter nicht. Taxilicht: Helligkeits-
            // stellung, an bei 2 (wie vmsACARS `Zibo738.js`); Sonden-Heizung:
            // Kapitaensschalter (Doku „PROBES ANTI ICE CAPTAIN“, 0/1).
            DatarefOverride {
                field: FieldId::LightTaxi,
                dataref: "laminar/B738/toggle_switch/taxi_light_brightness_pos",
                mapping: ValueMapping::DetentTable(&[0.0, 0.0, 1.0]),
            },
            DatarefOverride {
                field: FieldId::PitotHeat,
                dataref: "laminar/B738/toggle_switch/capt_probes_pos",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::ApHeading,
                dataref: "laminar/B738/autopilot/hdg_sel_status",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::ApAltitude,
                dataref: "laminar/B738/autopilot/alt_hld_status",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::ApNav,
                dataref: "laminar/B738/autopilot/lnav_status",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::ApApproach,
                dataref: "laminar/B738/autopilot/app_status",
                mapping: ValueMapping::Passthrough,
            },
        ],
    },
    // 05.10.2026 — FlightFactor 777v2 (X-Plane 12). Pilot-Befund Michel
    // (SIA375 27.09., NWS419 05.10.): der Speedbrake-Hebel steht in ARMED
    // auf +0.5 in `speedbrake_ratio` statt auf -0.5 — beide Fluege 0 → 0.5
    // im Anflug, Sprung auf 1.0 genau beim Aufsetzen (Auto-Ausfahren,
    // cockpit.pdf S. 72: „ARMED — after landing, automatically moves speed
    // brake lever to UP“). `spoilers_armed` blieb dadurch immer false.
    // Probe aus `cduDatarefs.txt` des Pakets („/ok int does cdu work“);
    // `1-sim/…` tragen alle FlightFactor-Muster, der Titel „777“ auch die
    // freie Stratosphere-777 — deshalb nur mit beidem.
    XplaneAircraftProfile {
        name: "FlightFactor 777",
        title_match: &["777"],
        probe_dataref: "1-sim/cduL/ok",
        titel_und_probe: true,
        overrides: &[DatarefOverride {
            field: FieldId::SpoilersHandle,
            dataref: "sim/cockpit2/controls/speedbrake_ratio",
            mapping: ValueMapping::ArmedAt(0.5),
        }],
    },
    // 25.10.2026 — FlightFactor 757 und 767 (X-Plane 12), Quelle: P_-Profile
    // von Michel (GSG), alle neun Varianten mit denselben Datarefs.
    // Fluegellicht und Autobrake liegen dort auf allgemeinen Namen
    // (`anim/46/button`, `anim/rhotery/25`), die andere Add-ons anders
    // belegen — deshalb nur hier, mit Titel UND Probe. Probe: Landelicht-
    // Schalter links, in allen neun Varianten vorhanden.
    XplaneAircraftProfile {
        name: "FlightFactor 757",
        title_match: &["757"],
        probe_dataref: "1-sim/lights/landingL/switch",
        titel_und_probe: true,
        overrides: FF757_767_OVERRIDES,
    },
    XplaneAircraftProfile {
        name: "FlightFactor 767",
        title_match: &["767"],
        probe_dataref: "1-sim/lights/landingL/switch",
        titel_und_probe: true,
        overrides: FF757_767_OVERRIDES,
    },
    // 28.10.2026 — Felis 747-200 / 747-200F, Quelle: P_-Profile v2.0 von
    // Michel. Live-Abgleich: in beiden Oktober-Fluegen (02./13.10.) blieben
    // alle AP-Felder, Autobrake und Fluegellicht den ganzen Flug aus.
    // AP-Wahlschalter A/B 0 OFF / 1 MAN / 2 COMMAND; MAN zaehlt als an — der
    // Autopilot ist dort eingekuppelt, der Pilot steuert ueber ihn. A und B
    // gehen ueber die beiden Lampenfelder der FF777 (> 0.01) als ODER in
    // `autopilot_master`. Autobrake (live gemessen) 0 OFF / 1 DISARM /
    // 2 MIN / 3 MED / 4 MAX auf die FF777-Skala (MIN = „1", MED = „2").
    // Der NAV-MODE-Waehler (0 INS … 4 LAND) bleibt aussen vor: er steht am
    // Boden auf HDG, auch bei ausgeschaltetem Autopiloten.
    XplaneAircraftProfile {
        name: "Felis 747-200",
        title_match: &["felis", "747"],
        probe_dataref: "B742/AP_panel/AP_engage_A",
        titel_und_probe: false,
        overrides: &[
            DatarefOverride {
                field: FieldId::Ff777ApL,
                dataref: "B742/AP_panel/AP_engage_A",
                mapping: ValueMapping::DetentTable(&[0.0, 1.0, 1.0]),
            },
            DatarefOverride {
                field: FieldId::Ff777ApR,
                dataref: "B742/AP_panel/AP_engage_B",
                mapping: ValueMapping::DetentTable(&[0.0, 1.0, 1.0]),
            },
            DatarefOverride {
                field: FieldId::Ff777AltHold,
                dataref: "B742/front_lamps/AP_alt_hold_lit",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::Ff777At,
                dataref: "B742/front_lamps/AP_AT_on_lit",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::FfAutobrake,
                dataref: "B742/OVHD/auto_brake_sel",
                mapping: ValueMapping::DetentTable(&[1.0, 2.0, 3.0, 4.0, 7.0]),
            },
            DatarefOverride {
                field: FieldId::FfFluegellicht,
                dataref: "B742/ext_light/wing_sw",
                mapping: ValueMapping::Passthrough,
            },
            // Schilder nur als Lampe (kein AUTO) → an = ON.
            DatarefOverride {
                field: FieldId::FfAnschnallzeichen,
                dataref: "B742/ovhd_lamps/fasten_sbelts_but_lit",
                mapping: ValueMapping::Schwelle(0.01, 2.0),
            },
            DatarefOverride {
                field: FieldId::FfRauchverbot,
                dataref: "B742/ovhd_lamps/no_smoking_but_lit",
                mapping: ValueMapping::Schwelle(0.01, 2.0),
            },
        ],
    },
    // 28.10.2026 — FlyJSim Dash 8-400 (Q4XP), P_-Profile v2.0. Kollisions-
    // licht kommt schon ueber `Q4xpKollisionslicht` (dataref.rs). Hier die
    // Kippschalter 0/1 fuer Fluegel (WING INSP), Anschnall- und Rauchverbots-
    // zeichen (an = ON). Klappen laufen ueber den Standard (im Flug vom
    // 28.09. belegt). Fuer AP-Modi gibt es keine Quelle: das FGCP hat keine
    // Lampen, das FMA (`FJS/Q4XP/FMA/*`) liefert Zahlencodes, deren
    // Bedeutung Michel noch misst.
    XplaneAircraftProfile {
        name: "FlyJSim Q4XP",
        title_match: &["dash 8", "q400"],
        probe_dataref: "FJS/Q4XP/FlightControls/FlapsHandle",
        titel_und_probe: false,
        overrides: &[
            DatarefOverride {
                field: FieldId::FfFluegellicht,
                dataref: "FJS/Q4XP/Manips/TwoSwitch_Anim[22]",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::FfAnschnallzeichen,
                dataref: "FJS/Q4XP/Manips/TwoSwitch_Anim[34]",
                mapping: ValueMapping::Schwelle(0.5, 2.0),
            },
            DatarefOverride {
                field: FieldId::FfRauchverbot,
                dataref: "FJS/Q4XP/Manips/TwoSwitch_Anim[35]",
                mapping: ValueMapping::Schwelle(0.5, 2.0),
            },
        ],
    },
    // 28.10.2026 — DeltaWing ATR 72-600 (`DWSim/ATR_Series`), P_-Profile
    // v2.0, Beta: noch kein Flug mit AeroACARS, Werte aus den Tasten-
    // bildern. Lichter und Schilder 0/1; Parkbremse 0 OFF / 0.5 HALB /
    // 1 PARK (nur PARK zaehlt, `parking_brake` > 0.5); Klappenhebel in Grad
    // 0/15/30, Landeklappen 30; AP aus der AP-Lampe links. Der acf-Titel
    // ist unbekannt — Erkennung nur ueber die Probe.
    XplaneAircraftProfile {
        name: "DeltaWing ATR 72-600",
        title_match: &[],
        probe_dataref: "DWSim/ATR_Series/datarefs/overhead/extLt/beaconSwitch",
        titel_und_probe: false,
        overrides: &[
            DatarefOverride {
                field: FieldId::LightBeacon,
                dataref: "DWSim/ATR_Series/datarefs/overhead/extLt/beaconSwitch",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::LightStrobe,
                dataref: "DWSim/ATR_Series/datarefs/overhead/extLt/strobeSwitch",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::LightNav,
                dataref: "DWSim/ATR_Series/datarefs/overhead/extLt/navSwitch",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::LightTaxi,
                dataref: "DWSim/ATR_Series/datarefs/overhead/extLt/taxiToSwitch",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::LightLanding,
                dataref: "DWSim/ATR_Series/datarefs/overhead/extLt/landingLSwitch",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::FfFluegellicht,
                dataref: "DWSim/ATR_Series/datarefs/overhead/extLt/wingSwitch",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::FfAnschnallzeichen,
                dataref: "DWSim/ATR_Series/datarefs/overhead/signs/seatBeltsSwitch",
                mapping: ValueMapping::Schwelle(0.5, 2.0),
            },
            DatarefOverride {
                field: FieldId::FfRauchverbot,
                dataref: "DWSim/ATR_Series/datarefs/overhead/signs/noDeviceSwitch",
                mapping: ValueMapping::Schwelle(0.5, 2.0),
            },
            DatarefOverride {
                field: FieldId::Ff777ApL,
                dataref: "DWSim/ATR_Series/lit/front/afcs/apLeft",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::ParkingBrake,
                dataref: "DWSim/ATR_Series/datarefs/throttleQuadrant/parkingBrake/movement",
                mapping: ValueMapping::Passthrough,
            },
            DatarefOverride {
                field: FieldId::FlapsHandle,
                dataref: "DWSim/ATR_Series/datarefs/throttleQuadrant/flapsLever/rotation",
                mapping: ValueMapping::DegreeTable(ATR72_FLAP_DEGREES),
            },
        ],
    },
];

/// CL650-Schilder ab Rohwert -1: -1 ON, 0 OFF, 1 AUTO → 2/0/1.
const CL650_SCHILDER: &[f32] = &[2.0, 0.0, 1.0];

/// ATR 72-600 Klappenhebel in Grad → interner Anteil; 30 = Landeklappen
/// (Landing-Config ab 0.70), 15 nicht.
pub const ATR72_FLAP_DEGREES: &[(f32, f32)] = &[(0.0, 0.0), (15.0, 0.5), (30.0, 1.0)];

/// Gemeinsame Umbiegungen der FF757/767 (Werte „live bestaetigt" laut
/// P_-Profilen: Autobrake RTO = -1 … MAX AUTO = 6).
const FF757_767_OVERRIDES: &[DatarefOverride] = &[
    DatarefOverride {
        field: FieldId::FfFluegellicht,
        dataref: "anim/46/button",
        mapping: ValueMapping::Passthrough,
    },
    DatarefOverride {
        field: FieldId::FfAutobrake,
        dataref: "anim/rhotery/25",
        mapping: ValueMapping::Versatz(1.0),
    },
];

/// Profilwechsel: Add-on-Felder, die das bisherige Profil aus einem eigenen
/// Namen las (FF757/767-Fluegellicht), vergessen — sonst bliebe der letzte
/// Wert des alten Flugzeugs stehen, weil der Grundname ihn nie ueberschreibt
/// (Codex-QS 25.10.2026). Fuehrt das neue Profil dieselbe Ersetzung weiter
/// (FF757 ↔ FF767), bleibt der Wert — er stammt aus derselben Quelle.
pub fn profil_ersetzungen_leeren(
    parsed: &mut crate::dataref::XPlaneState,
    alt: Option<usize>,
    neu: Option<usize>,
) {
    if alt == neu {
        return;
    }
    let Some(p) = alt.and_then(|pi| PROFILES.get(pi)) else {
        return;
    };
    let weiter: &[DatarefOverride] = neu.and_then(|pi| PROFILES.get(pi)).map_or(&[], |n| n.overrides);
    for o in p.overrides {
        let bleibt = weiter.iter().any(|n| n.field == o.field && n.dataref == o.dataref);
        if crate::dataref::addon_quelle(o.field) && !bleibt {
            parsed.addon_leeren(o.field);
        }
    }
}

/// LE1 stage 1: first profile whose title-match accepts `title`. Profile,
/// die zusaetzlich die Probe brauchen (`titel_und_probe`), zaehlen hier
/// nicht.
pub fn profile_index_for_title(title: &str) -> Option<usize> {
    PROFILES
        .iter()
        .position(|p| !p.titel_und_probe && p.matches_title(title))
}

/// One row of the **active catalog** (LE6): the dataref the adapter
/// actually subscribes for a given `FieldId`, plus its mapping. Built
/// from the static `CATALOG` with the active profile's overrides
/// applied. Index = wire index = position in the active catalog.
#[derive(Debug, Clone, Copy)]
pub struct ActiveEntry {
    pub name: &'static str,
    pub field: FieldId,
    pub mapping: ValueMapping,
}

/// Build the active catalog (LE6). Starts from the static `CATALOG`;
/// when `profile` is `Some`, every override **replaces** the base entry
/// of the same `FieldId` (same index, new dataref + mapping). All
/// current profile overrides target `FieldId`s that already exist in
/// the base catalog, so the active catalog has the **same length and
/// indices** as `CATALOG` — `seen`/`last_values` need no resize.
pub fn build_active_catalog(profile: Option<&XplaneAircraftProfile>) -> Vec<ActiveEntry> {
    CATALOG
        .iter()
        .map(|base| {
            if let Some(p) = profile {
                if let Some(ovr) = p.overrides.iter().find(|o| o.field == base.field) {
                    return ActiveEntry {
                        name: ovr.dataref,
                        field: base.field,
                        mapping: ovr.mapping,
                    };
                }
            }
            ActiveEntry {
                name: base.name,
                field: base.field,
                mapping: ValueMapping::Passthrough,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cl650() -> &'static XplaneAircraftProfile {
        &PROFILES[0]
    }

    #[test]
    fn cl650_profile_matches_title() {
        assert!(cl650().matches_title("Challenger 650 published by X-Aviation"));
        // case-insensitive
        assert!(cl650().matches_title("CHALLENGER 650 — X-AVIATION"));
    }

    #[test]
    fn default_challenger_does_not_match_title() {
        // A default / non-Hot-Start Challenger lacks "X-Aviation".
        assert!(!cl650().matches_title("Bombardier Challenger 650"));
        assert!(!cl650().matches_title("Cessna 172"));
    }

    #[test]
    fn profile_index_for_title_picks_cl650() {
        assert_eq!(
            profile_index_for_title("Challenger 650 published by X-Aviation"),
            Some(0),
        );
        // Seit 28.10.2026 hat die Felis ein eigenes Profil, nicht das CL650.
        let felis = PROFILES.iter().position(|p| p.name == "Felis 747-200");
        assert!(felis.is_some());
        assert_eq!(profile_index_for_title("Felis 747-200"), felis);
        assert_eq!(profile_index_for_title("Boeing 747-400"), None);
    }

    #[test]
    fn flaps_detent_table_maps_lever_to_ratio() {
        let m = ValueMapping::DetentTable(CL650_FLAP_DETENTS);
        assert_eq!(m.map(0.0), Some(0.0));
        assert_eq!(m.map(1.0), Some(0.45));
        assert_eq!(m.map(2.0), Some(0.80));
        assert_eq!(m.map(3.0), Some(1.0));
    }

    #[test]
    fn flaps_lever_0_and_1_not_landing_config() {
        let m = ValueMapping::DetentTable(CL650_FLAP_DETENTS);
        assert!(m.map(0.0).unwrap() < 0.70);
        assert!(m.map(1.0).unwrap() < 0.70);
    }

    #[test]
    fn flaps_lever_2_and_3_are_landing_config() {
        let m = ValueMapping::DetentTable(CL650_FLAP_DETENTS);
        assert!(m.map(2.0).unwrap() >= 0.70);
        assert!(m.map(3.0).unwrap() >= 0.70);
    }

    #[test]
    fn detent_table_nonfinite_yields_no_value() {
        let m = ValueMapping::DetentTable(CL650_FLAP_DETENTS);
        assert_eq!(m.map(f32::NAN), None);
        assert_eq!(m.map(f32::INFINITY), None);
    }

    #[test]
    fn detent_table_rejects_non_integer_value() {
        let m = ValueMapping::DetentTable(CL650_FLAP_DETENTS);
        // 1.51 must NOT snap onto detent 2 (LE7 / QS-R2-P2).
        assert_eq!(m.map(1.51), None);
        assert_eq!(m.map(1.49), None);
        // within tolerance is still accepted
        assert_eq!(m.map(2.03), Some(0.80));
    }

    #[test]
    fn detent_table_out_of_range_yields_no_value() {
        let m = ValueMapping::DetentTable(CL650_FLAP_DETENTS);
        assert_eq!(m.map(-1.0), None);
        assert_eq!(m.map(4.0), None);
    }

    #[test]
    fn passthrough_keeps_raw_value() {
        assert_eq!(ValueMapping::Passthrough.map(1.0), Some(1.0));
        assert_eq!(ValueMapping::Passthrough.map(0.0), Some(0.0));
    }

    // ── v0.12.6 — Rotate MD-11 flaps (DegreeTable) ──────────────────

    #[test]
    fn md11_degree_table_maps_each_detent() {
        let m = ValueMapping::DegreeTable(MD11_FLAP_DEGREES);
        assert_eq!(m.map(0.0), Some(0.0));
        assert_eq!(m.map(15.0), Some(0.30));
        assert_eq!(m.map(28.0), Some(0.55));
        assert_eq!(m.map(35.0), Some(0.80));
        assert_eq!(m.map(50.0), Some(1.0));
    }

    #[test]
    fn md11_flaps_35_and_50_are_landing_config() {
        // LANDING-CONFIG-Check ist `flaps_position >= 0.70`.
        let m = ValueMapping::DegreeTable(MD11_FLAP_DEGREES);
        assert!(m.map(35.0).unwrap() >= 0.70);
        assert!(m.map(50.0).unwrap() >= 0.70);
    }

    #[test]
    fn md11_flaps_0_15_28_are_not_landing_config() {
        let m = ValueMapping::DegreeTable(MD11_FLAP_DEGREES);
        assert!(m.map(0.0).unwrap() < 0.70);
        assert!(m.map(15.0).unwrap() < 0.70);
        assert!(m.map(28.0).unwrap() < 0.70);
    }

    #[test]
    fn md11_degree_table_snaps_to_nearest_detent() {
        // Ein leichter Float-Wackler um eine Raste herum snappt sauber.
        let m = ValueMapping::DegreeTable(MD11_FLAP_DEGREES);
        assert_eq!(m.map(34.6), Some(0.80));
        assert_eq!(m.map(15.4), Some(0.30));
        // Mitten zwischen 28 und 35 → näher an 28.
        assert_eq!(m.map(30.0), Some(0.55));
    }

    #[test]
    fn degree_table_nonfinite_yields_no_value() {
        let m = ValueMapping::DegreeTable(MD11_FLAP_DEGREES);
        assert_eq!(m.map(f32::NAN), None);
        assert_eq!(m.map(f32::INFINITY), None);
    }

    #[test]
    fn md11_profile_matches_title() {
        let md11 = PROFILES
            .iter()
            .find(|p| p.name == "Rotate MD-11")
            .expect("MD-11 profile present");
        // Echter Title aus Michels Log: "MD11 Rotate MD-11P".
        assert!(md11.matches_title("MD11 Rotate MD-11P"));
        assert!(!md11.matches_title("Boeing 737-800"));
    }

    #[test]
    fn md11_active_catalog_overrides_flaps() {
        let md11 = PROFILES
            .iter()
            .find(|p| p.name == "Rotate MD-11")
            .expect("MD-11 profile present");
        let active = build_active_catalog(Some(md11));
        let flaps = active
            .iter()
            .find(|e| e.field == FieldId::FlapsHandle)
            .expect("FlapsHandle in catalog");
        assert_eq!(flaps.name, "Rotate/aircraft/systems/flaps_cmd_pos_deg");
        assert!(matches!(flaps.mapping, ValueMapping::DegreeTable(_)));
    }

    #[test]
    fn zibo737_profile_matches_both_variants() {
        let z = PROFILES
            .iter()
            .find(|p| p.name == "Laminar/Zibo 737-800")
            .expect("Zibo 737 profile present");
        // Standard-Laminar UND Zibo teilen den Basistitel.
        assert!(z.matches_title("Boeing 737-800"), "Standard-737");
        assert!(z.matches_title("Boeing 737-800X"), "Zibo-737");
        // Fremde Muster NICHT.
        assert!(!z.matches_title("MD11 Rotate MD-11P"));
        assert!(!z.matches_title("IXEG 737 Classic"));
    }

    #[test]
    fn zibo737_active_catalog_overrides_ap_modes_and_master() {
        let z = PROFILES
            .iter()
            .find(|p| p.name == "Laminar/Zibo 737-800")
            .expect("Zibo 737 profile present");
        let active = build_active_catalog(Some(z));
        // AP-Modus-Felder auf die 737-echten laminar/B738-Datarefs gebogen.
        let hdg = active
            .iter()
            .find(|e| e.field == FieldId::ApHeading)
            .unwrap();
        assert_eq!(hdg.name, "laminar/B738/autopilot/hdg_sel_status");
        let app = active
            .iter()
            .find(|e| e.field == FieldId::ApApproach)
            .unwrap();
        assert_eq!(app.name, "laminar/B738/autopilot/app_status");
        // ApMaster auf die CMD-A-Lampe (01.10.2026): `servos_on` treibt der
        // Zibo nicht, es blieb einen ganzen Flug mit Autopilot auf 0.
        let master = active
            .iter()
            .find(|e| e.field == FieldId::ApMaster)
            .unwrap();
        assert_eq!(master.name, "laminar/B738/autopilot/cmd_a_status");
        // Gegenprobe: ohne Profil bleibt der Standard-Autopilot.
        let basis = build_active_catalog(None);
        assert_eq!(
            basis
                .iter()
                .find(|e| e.field == FieldId::ApMaster)
                .unwrap()
                .name,
            "sim/cockpit2/autopilot/servos_on"
        );
        // Zibo-Audit 01.10.2026: Taxilicht (an bei Stellung 2) und
        // Sonden-Heizung aus den Zibo-Schaltern.
        let taxi = active
            .iter()
            .find(|e| e.field == FieldId::LightTaxi)
            .unwrap();
        assert_eq!(
            taxi.name,
            "laminar/B738/toggle_switch/taxi_light_brightness_pos"
        );
        assert_eq!(taxi.mapping.map(0.0), Some(0.0));
        assert_eq!(taxi.mapping.map(1.0), Some(0.0));
        assert_eq!(taxi.mapping.map(2.0), Some(1.0));
        let pitot = active
            .iter()
            .find(|e| e.field == FieldId::PitotHeat)
            .unwrap();
        assert_eq!(pitot.name, "laminar/B738/toggle_switch/capt_probes_pos");
        // Lockstep: gleiche Katalog-Laenge (nur bestehende Felder gebogen).
        assert_eq!(active.len(), CATALOG.len());
    }

    #[test]
    fn active_catalog_without_profile_equals_base() {
        let active = build_active_catalog(None);
        assert_eq!(active.len(), CATALOG.len());
        for (a, base) in active.iter().zip(CATALOG.iter()) {
            assert_eq!(a.name, base.name);
            assert_eq!(a.field, base.field);
            assert!(matches!(a.mapping, ValueMapping::Passthrough));
        }
    }

    #[test]
    fn active_catalog_override_replaces_same_fieldid() {
        let active = build_active_catalog(Some(cl650()));
        // same length + indices as base (LE6).
        assert_eq!(active.len(), CATALOG.len());
        // the FlapsHandle slot now points at the CL650 dataref.
        let flaps = active
            .iter()
            .find(|e| e.field == FieldId::FlapsHandle)
            .expect("FlapsHandle in catalog");
        assert_eq!(
            flaps.name,
            "abus/CL650/ARINC429/L-DCU-7/words/FCTL/0/FLAPS_LVR"
        );
        assert!(matches!(flaps.mapping, ValueMapping::DetentTable(_)));
        // a non-overridden field keeps its base dataref.
        let lat = active
            .iter()
            .find(|e| e.field == FieldId::Latitude)
            .expect("Latitude in catalog");
        assert_eq!(lat.name, "sim/flightmodel/position/latitude");
    }

    fn profil(name: &str) -> &'static XplaneAircraftProfile {
        PROFILES.iter().find(|p| p.name == name).expect(name)
    }

    /// Titel wie in den GSG-Flugprotokollen (25.10.2026).
    #[test]
    fn ff757_767_titel_und_probe() {
        for (name, titel) in [
            ("FlightFactor 757", "Boeing 757-RF"),
            ("FlightFactor 767", "Boeing 767 300ER"),
            ("FlightFactor 767", "Boeing 767-Freighter"),
        ] {
            let p = profil(name);
            assert!(p.matches_title(titel), "{titel}");
            assert!(p.titel_und_probe);
            // Ohne Probe nie aktiv — der Titel allein reicht nicht.
            assert_eq!(profile_index_for_title(titel), None, "{titel}");
        }
        assert!(!profil("FlightFactor 757").matches_title("Boeing 777-300ER"));
        assert!(!profil("FlightFactor 767").matches_title("Boeing 757-200"));
    }

    #[test]
    fn ff757_767_biegen_fluegellicht_und_autobrake_um() {
        for name in ["FlightFactor 757", "FlightFactor 767"] {
            let aktiv = build_active_catalog(Some(profil(name)));
            let ab = aktiv.iter().find(|e| e.field == FieldId::FfAutobrake).unwrap();
            assert_eq!(ab.name, "anim/rhotery/25");
            // RTO = -1 („live bestaetigt") → FF777-Skala 0; MAX AUTO 6 → 7.
            assert_eq!(ab.mapping.map(-1.0), Some(0.0));
            assert_eq!(ab.mapping.map(6.0), Some(7.0));
            assert_eq!(ab.mapping.map(f32::NAN), None);
            let wing = aktiv.iter().find(|e| e.field == FieldId::FfFluegellicht).unwrap();
            assert_eq!(wing.name, "anim/46/button");
        }
        // Profilwechsel (RREF und Plugin) leert die Ersetzungen des alten
        // Profils, nicht die Felder anderer Quellen.
        let pi = PROFILES.iter().position(|p| p.name == "FlightFactor 757").unwrap();
        let mut s = crate::dataref::XPlaneState::default();
        s.apply_field(FieldId::FfFluegellicht, 1.0);
        s.apply_field(FieldId::FfAutobrake, 3.0);
        s.apply_field(FieldId::TolissFluegellicht, 1.0);
        profil_ersetzungen_leeren(&mut s, Some(pi), Some(pi));
        assert_eq!(s.ff_fluegellicht, Some(true), "gleiches Profil: nichts");
        // FF757 → FF767: dieselben Ersetzungen, der Wert bleibt (Codex-QS
        // Runde 3, Rueckfall Plugin → RREF).
        let pi767 = PROFILES.iter().position(|p| p.name == "FlightFactor 767").unwrap();
        profil_ersetzungen_leeren(&mut s, Some(pi), Some(pi767));
        assert_eq!(s.ff_fluegellicht, Some(true));
        assert_eq!(s.ff_autobrake, Some(3.0));
        // FF757 → FF777: das FF777-Profil ersetzt nur den Speedbrake —
        // Fluegellicht/Autobrake aus anim/* muessen weg.
        let pi777 = PROFILES.iter().position(|p| p.name == "FlightFactor 777").unwrap();
        let mut t = crate::dataref::XPlaneState::default();
        t.apply_field(FieldId::FfFluegellicht, 1.0);
        profil_ersetzungen_leeren(&mut t, Some(pi), Some(pi777));
        assert_eq!(t.ff_fluegellicht, None);
        profil_ersetzungen_leeren(&mut s, Some(pi), None);
        assert_eq!(s.ff_fluegellicht, None);
        assert_eq!(s.ff_autobrake, None);
        assert_eq!(s.toliss_fluegellicht, Some(true));
        // Ohne Profil liest der Eintrag den FF777-Dataref.
        let grund = build_active_catalog(None);
        let ab = grund.iter().find(|e| e.field == FieldId::FfAutobrake).unwrap();
        assert_eq!(ab.name, "1-sim/ckpt/autobrakeSwitch/anim");
    }

    #[test]
    fn neue_umrechnungen_ab_und_schwelle() {
        let ab = ValueMapping::DetentTableAb(-1.0, CL650_SCHILDER);
        assert_eq!(ab.map(-1.0), Some(2.0));
        assert_eq!(ab.map(0.0), Some(0.0));
        assert_eq!(ab.map(1.0), Some(1.0));
        assert_eq!(ab.map(2.0), None);
        assert_eq!(ab.map(-0.5), None);
        assert_eq!(ab.map(f32::NAN), None);
        let s = ValueMapping::Schwelle(0.01, 2.0);
        assert_eq!(s.map(0.0), Some(0.0));
        assert_eq!(s.map(0.01), Some(0.0));
        assert_eq!(s.map(0.4), Some(2.0));
        assert_eq!(s.map(f32::NAN), None);
        assert_eq!(s.map(f32::INFINITY), None);
    }

    /// Ganze Kette: Rohwert durch die Umrechnung des aktiven Katalogs in den
    /// Zustand, dann in den Snapshot.
    fn snapshot_mit(name: &str, werte: &[(&str, f32)]) -> sim_core::SimSnapshot {
        let aktiv = build_active_catalog(Some(profil(name)));
        let mut s = crate::dataref::XPlaneState::default();
        for (dataref, roh) in werte {
            let e = aktiv.iter().find(|e| e.name == *dataref).expect(dataref);
            if let Some(v) = e.mapping.map(*roh) {
                s.apply_field(e.field, v);
            }
        }
        s.to_snapshot(sim_core::Simulator::XPlane12)
    }

    #[test]
    fn felis_ap_autobrake_schilder_fluegel() {
        let snap = snapshot_mit(
            "Felis 747-200",
            &[
                ("B742/AP_panel/AP_engage_A", 0.0),
                ("B742/AP_panel/AP_engage_B", 2.0),
                ("B742/front_lamps/AP_alt_hold_lit", 0.8),
                ("B742/front_lamps/AP_AT_on_lit", 0.8),
                ("B742/OVHD/auto_brake_sel", 3.0),
                ("B742/ext_light/wing_sw", 1.0),
                ("B742/ovhd_lamps/fasten_sbelts_but_lit", 0.6),
                ("B742/ovhd_lamps/no_smoking_but_lit", 0.0),
            ],
        );
        assert_eq!(snap.autopilot_master, Some(true), "B in COMMAND");
        assert_eq!(snap.autopilot_altitude, Some(true));
        assert_eq!(snap.autothrottle_on, Some(true));
        assert_eq!(snap.autobrake.as_deref(), Some("2"), "MED");
        assert_eq!(snap.light_wing, Some(true));
        assert_eq!(snap.seatbelts_sign, Some(2));
        assert_eq!(snap.no_smoking_sign, Some(0));
        // MAN zaehlt als an, OFF als aus; Autobrake OFF/DISARM/MAX.
        let man = snapshot_mit("Felis 747-200", &[("B742/AP_panel/AP_engage_A", 1.0)]);
        assert_eq!(man.autopilot_master, Some(true));
        let aus = snapshot_mit(
            "Felis 747-200",
            &[("B742/AP_panel/AP_engage_A", 0.0), ("B742/AP_panel/AP_engage_B", 0.0)],
        );
        assert_eq!(aus.autopilot_master, Some(false));
        for (roh, label) in [(0.0, "OFF"), (1.0, "DISARM"), (2.0, "1"), (4.0, "MAX")] {
            let s = snapshot_mit("Felis 747-200", &[("B742/OVHD/auto_brake_sel", roh)]);
            assert_eq!(s.autobrake.as_deref(), Some(label), "{roh}");
        }
        assert!(profil("Felis 747-200").matches_title("Felis 747-200 Cargo GE"));
    }

    #[test]
    fn cl650_parkbremse_ap_lampen_schilder() {
        let snap = snapshot_mit(
            "Hot Start CL650",
            &[
                ("CL650/pedestal/park_brake_value", 1.0),
                ("CL650/lamps/glareshield/FCP/ap_eng_1", 0.9),
                ("CL650/lamps/glareshield/FCP/hdg_1", 0.9),
                ("CL650/lamps/glareshield/FCP/alt_1", 0.0),
                ("CL650/lamps/glareshield/FCP/nav_1", 0.0),
                ("CL650/lamps/glareshield/FCP/appr_1", 0.9),
                ("CL650/overhead/signs/seatbelt_value", -1.0),
                ("CL650/overhead/signs/no_smoking_value", 1.0),
            ],
        );
        assert!(snap.parking_brake);
        assert_eq!(snap.autopilot_master, Some(true));
        assert_eq!(snap.autopilot_heading, Some(true));
        assert_eq!(snap.autopilot_altitude, Some(false));
        assert_eq!(snap.autopilot_approach, Some(true));
        assert_eq!(snap.seatbelts_sign, Some(2), "-1 = ON");
        assert_eq!(snap.no_smoking_sign, Some(1), "1 = AUTO");
        let aus = snapshot_mit(
            "Hot Start CL650",
            &[
                ("CL650/pedestal/park_brake_value", 0.0),
                ("CL650/overhead/signs/seatbelt_value", 0.0),
            ],
        );
        assert!(!aus.parking_brake);
        assert_eq!(aus.seatbelts_sign, Some(0));
        // Die Klappen-Ersetzung des GSG225-Befunds bleibt.
        let aktiv = build_active_catalog(Some(cl650()));
        let flaps = aktiv.iter().find(|e| e.field == FieldId::FlapsHandle).unwrap();
        assert_eq!(flaps.name, "abus/CL650/ARINC429/L-DCU-7/words/FCTL/0/FLAPS_LVR");
    }

    #[test]
    fn md11_schilder_nose_und_ap() {
        let snap = snapshot_mit(
            "Rotate MD-11",
            &[
                ("Rotate/aircraft/controls/seatbelts_lts", 1.0),
                ("Rotate/aircraft/controls/nosmoke_lts", 2.0),
                ("Rotate/aircraft/controls/nose_lts", 2.0),
                ("Rotate/aircraft/controls/fgs_autoflight", 1.0),
                ("Rotate/aircraft/systems/afs_appr_engaged", 1.0),
            ],
        );
        assert_eq!(snap.seatbelts_sign, Some(1), "AUTO");
        assert_eq!(snap.no_smoking_sign, Some(2), "ON");
        assert_eq!(snap.light_taxi, Some(true), "T/O");
        assert_eq!(snap.autopilot_master, Some(true));
        assert_eq!(snap.autopilot_approach, Some(true));
        for (roh, an) in [(0.0, false), (1.0, true)] {
            let s = snapshot_mit("Rotate MD-11", &[("Rotate/aircraft/controls/nose_lts", roh)]);
            assert_eq!(s.light_taxi, Some(an), "{roh}");
        }
    }

    #[test]
    fn q4xp_fluegel_und_schilder() {
        let p = profil("FlyJSim Q4XP");
        assert!(p.matches_title("Dash 8 Q400"));
        let snap = snapshot_mit(
            "FlyJSim Q4XP",
            &[
                ("FJS/Q4XP/Manips/TwoSwitch_Anim[22]", 1.0),
                ("FJS/Q4XP/Manips/TwoSwitch_Anim[34]", 1.0),
                ("FJS/Q4XP/Manips/TwoSwitch_Anim[35]", 0.0),
            ],
        );
        assert_eq!(snap.light_wing, Some(true));
        assert_eq!(snap.seatbelts_sign, Some(2));
        assert_eq!(snap.no_smoking_sign, Some(0));
        // Klappen bleiben auf dem Standard.
        let aktiv = build_active_catalog(Some(p));
        let grund = build_active_catalog(None);
        let f = |k: &[ActiveEntry]| k.iter().find(|e| e.field == FieldId::FlapsHandle).unwrap().name;
        assert_eq!(f(&aktiv), f(&grund));
    }

    #[test]
    fn atr72_lichter_parkbremse_klappen_ap() {
        let p = profil("DeltaWing ATR 72-600");
        assert!(!p.matches_title("ATR 72-600"), "nur ueber die Probe");
        let x = "DWSim/ATR_Series/datarefs/";
        let snap = snapshot_mit(
            "DeltaWing ATR 72-600",
            &[
                (&format!("{x}overhead/extLt/beaconSwitch"), 1.0),
                (&format!("{x}overhead/extLt/strobeSwitch"), 0.0),
                (&format!("{x}overhead/extLt/navSwitch"), 1.0),
                (&format!("{x}overhead/extLt/taxiToSwitch"), 1.0),
                (&format!("{x}overhead/extLt/landingLSwitch"), 1.0),
                (&format!("{x}overhead/extLt/wingSwitch"), 1.0),
                (&format!("{x}overhead/signs/seatBeltsSwitch"), 1.0),
                (&format!("{x}overhead/signs/noDeviceSwitch"), 0.0),
                (&format!("{x}throttleQuadrant/parkingBrake/movement"), 1.0),
                (&format!("{x}throttleQuadrant/flapsLever/rotation"), 30.0),
                ("DWSim/ATR_Series/lit/front/afcs/apLeft", 0.7),
            ],
        );
        assert_eq!(snap.light_beacon, Some(true));
        assert_eq!(snap.light_strobe, Some(false));
        assert_eq!(snap.light_nav, Some(true));
        assert_eq!(snap.light_taxi, Some(true));
        assert_eq!(snap.light_landing, Some(true));
        assert_eq!(snap.light_wing, Some(true));
        assert_eq!(snap.seatbelts_sign, Some(2));
        assert_eq!(snap.no_smoking_sign, Some(0));
        assert!(snap.parking_brake);
        assert_eq!(snap.autopilot_master, Some(true));
        assert!(snap.flaps_position >= 0.70, "Klappen 30 = Landeklappen");
        let halb = snapshot_mit(
            "DeltaWing ATR 72-600",
            &[
                (&format!("{x}throttleQuadrant/parkingBrake/movement"), 0.5),
                (&format!("{x}throttleQuadrant/flapsLever/rotation"), 15.0),
            ],
        );
        assert!(!halb.parking_brake, "HALB ist nicht PARK");
        assert!(halb.flaps_position < 0.70);
    }

    /// Profilwechsel: die neuen Add-on-Ersetzungen (Lampen, Schilder,
    /// Fluegellicht) werden beim Wechsel weg vom Profil geleert.
    #[test]
    fn neue_profile_leeren_ihre_ersetzungen() {
        for name in ["Felis 747-200", "FlyJSim Q4XP", "DeltaWing ATR 72-600", "Hot Start CL650"] {
            let pi = PROFILES.iter().position(|p| p.name == name).unwrap();
            let mut s = crate::dataref::XPlaneState::default();
            s.apply_field(FieldId::FfFluegellicht, 1.0);
            s.apply_field(FieldId::FfAnschnallzeichen, 2.0);
            s.apply_field(FieldId::Ff777ApL, 1.0);
            profil_ersetzungen_leeren(&mut s, Some(pi), None);
            assert_eq!(s.ff_anschnallzeichen, None, "{name}");
            let fluegel = profil(name).overrides.iter().any(|o| o.field == FieldId::FfFluegellicht);
            assert_eq!(s.ff_fluegellicht.is_none(), fluegel, "{name}");
            let ap = profil(name).overrides.iter().any(|o| o.field == FieldId::Ff777ApL);
            assert_eq!(s.ff777_ap_l.is_none(), ap, "{name}");
        }
    }
}
