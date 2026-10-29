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
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(tag = "art", content = "werte")]
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
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct DatarefOverride {
    pub field: FieldId,
    pub dataref: &'static str,
    pub mapping: ValueMapping,
}

/// A per-aircraft dataref profile (LE2).
#[derive(Debug, Clone, Copy, serde::Serialize)]
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

/// Eingebauter Stand der Profile = Datei im Paketordner (P-3.65: eine
/// Quelle).
pub const EINGEBAUT: &str = include_str!("../../../../../bundle/profile/xplane.json");

/// Woher die Profile kommen. Der Client meldet beim Start eine Quelle an, die
/// aus SEINEM einen Paketstand liest (P-3.02: keine zweite Sperre, nie
/// gemischt). Ohne Anmeldung (Tests dieses Crates) gilt der eingebaute Stand.
static QUELLE: std::sync::OnceLock<fn() -> &'static [XplaneAircraftProfile]> = std::sync::OnceLock::new();

/// Alle bekannten Profile (LE2) aus dem Laufzeit-Paket. Das erste, dessen
/// Erkennung passt, gewinnt.
pub fn profile() -> &'static [XplaneAircraftProfile] {
    match QUELLE.get() {
        Some(quelle) => quelle(),
        None => {
            static EIGEN: std::sync::OnceLock<&'static [XplaneAircraftProfile]> = std::sync::OnceLock::new();
            EIGEN.get_or_init(|| profile_lesen(EINGEBAUT).expect("eingebaute X-Plane-Profile"))
        }
    }
}

/// Meldet die Quelle der Profile an (einmal je Lauf). `false`, wenn schon
/// eine angemeldet ist.
pub fn profile_quelle_setzen(quelle: fn() -> &'static [XplaneAircraftProfile]) -> bool {
    QUELLE.set(quelle).is_ok()
}

/// Prueft `profile/xplane.json` wie [`profile_lesen`], ohne etwas dauerhaft
/// abzulegen (Selbsttest eines heruntergeladenen Stands, Codex 29.10.).
pub fn profile_pruefen(json: &str) -> Result<(), String> {
    datei_lesen(json).map(|_| ())
}

/// Liest `profile/xplane.json`. Unbekannte oder fehlende Felder sind ein
/// Lesefehler (fail-closed, P-3.65); die Notizen bleiben in der Datei.
/// Das Ergebnis lebt bis Programmende (einmal je Lauf, P-3.02).
pub fn profile_lesen(json: &str) -> Result<&'static [XplaneAircraftProfile], String> {
    fn s(x: String) -> &'static str {
        Box::leak(x.into_boxed_str())
    }
    let d = datei_lesen(json)?;
    let profile: Vec<XplaneAircraftProfile> = d
        .profile
        .into_iter()
        .map(|p| XplaneAircraftProfile {
            name: s(p.name),
            title_match: Box::leak(p.title_match.into_iter().map(s).collect()),
            probe_dataref: s(p.probe_dataref),
            titel_und_probe: p.titel_und_probe,
            overrides: Box::leak(
                p.overrides
                    .into_iter()
                    .map(|o| DatarefOverride {
                        field: o.field,
                        dataref: s(o.dataref),
                        mapping: match o.mapping {
                            AbbildungDatei::Passthrough => ValueMapping::Passthrough,
                            AbbildungDatei::DetentTable(t) => ValueMapping::DetentTable(Box::leak(t.into())),
                            AbbildungDatei::DegreeTable(t) => ValueMapping::DegreeTable(Box::leak(t.into())),
                            AbbildungDatei::ArmedAt(v) => ValueMapping::ArmedAt(v),
                            AbbildungDatei::Versatz(v) => ValueMapping::Versatz(v),
                            AbbildungDatei::DetentTableAb(a, t) => ValueMapping::DetentTableAb(a, Box::leak(t.into())),
                            AbbildungDatei::Schwelle(a, b) => ValueMapping::Schwelle(a, b),
                        },
                    })
                    .collect(),
            ),
        })
        .collect();
    Ok(Box::leak(profile.into()))
}

// Dateiform von `profile/xplane.json`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Datei {
    format: u32,
    profile: Vec<ProfilDatei>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfilDatei {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
    title_match: Vec<String>,
    probe_dataref: String,
    titel_und_probe: bool,
    overrides: Vec<ErsetzungDatei>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ErsetzungDatei {
    field: FieldId,
    dataref: String,
    mapping: AbbildungDatei,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
}
#[derive(serde::Deserialize)]
#[serde(tag = "art", content = "werte", deny_unknown_fields)]
enum AbbildungDatei {
    Passthrough,
    DetentTable(Vec<f32>),
    DegreeTable(Vec<(f32, f32)>),
    ArmedAt(f32),
    Versatz(f32),
    DetentTableAb(f32, Vec<f32>),
    Schwelle(f32, f32),
}

fn datei_lesen(json: &str) -> Result<Datei, String> {
    let d: Datei = serde_json::from_str(json).map_err(|e| format!("profile/xplane.json: {e}"))?;
    if d.format != 1 {
        return Err(format!("profile/xplane.json: Format {} unbekannt", d.format));
    }
    Ok(d)
}

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
    let Some(p) = alt.and_then(|pi| profile().get(pi)) else {
        return;
    };
    let weiter: &[DatarefOverride] = neu.and_then(|pi| profile().get(pi)).map_or(&[], |n| n.overrides);
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
    profile()
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

    /// Die Profile im Paket sind genau die der Konstante `profile()` aus
    /// 2.1.24 (Etappe 1b, P-3.66): Abdruck (FNV-1a) der Debug-Ausgabe, vor
    /// dem Entfernen der Konstante gegen sie verglichen. Aendert sich ein
    /// Profil bewusst, wird der Abdruck mit Begruendung nachgezogen.
    #[test]
    fn profile_wie_2_1_24() {
        let text = format!("{:?}", profile_lesen(EINGEBAUT).unwrap());
        let mut h: u64 = 0xcbf29ce484222325;
        for b in text.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        assert_eq!((h, text.len()), (0x7c32f08235675c96, 7838));
    }

    #[test]
    fn fremdes_oder_fehlendes_feld_ist_ein_lesefehler() {
        let gut: serde_json::Value = serde_json::from_str(EINGEBAUT).unwrap();
        let mut fremd = gut.clone();
        fremd["profile"][0]["unbekannt"] = serde_json::json!(1);
        assert!(profile_lesen(&fremd.to_string()).is_err());
        let mut ohne = gut.clone();
        ohne["profile"][0].as_object_mut().unwrap().remove("probe_dataref");
        assert!(profile_lesen(&ohne.to_string()).is_err());
        let mut format = gut;
        format["format"] = serde_json::json!(2);
        assert!(profile_lesen(&format.to_string()).is_err());
    }

    /// Umrechnung eines Profils aus dem Paket (statt der frueheren Konstanten).
    fn abbildung(name: &str, feld: FieldId) -> ValueMapping {
        profile()
            .iter()
            .find(|p| p.name == name)
            .and_then(|p| p.overrides.iter().find(|o| o.field == feld))
            .map(|o| o.mapping)
            .expect(name)
    }

    fn cl650() -> &'static XplaneAircraftProfile {
        &profile()[0]
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
        let felis = profile().iter().position(|p| p.name == "Felis 747-200");
        assert!(felis.is_some());
        assert_eq!(profile_index_for_title("Felis 747-200"), felis);
        assert_eq!(profile_index_for_title("Boeing 747-400"), None);
    }

    #[test]
    fn flaps_detent_table_maps_lever_to_ratio() {
        let m = abbildung("Hot Start CL650", FieldId::FlapsHandle);
        assert_eq!(m.map(0.0), Some(0.0));
        assert_eq!(m.map(1.0), Some(0.45));
        assert_eq!(m.map(2.0), Some(0.80));
        assert_eq!(m.map(3.0), Some(1.0));
    }

    #[test]
    fn flaps_lever_0_and_1_not_landing_config() {
        let m = abbildung("Hot Start CL650", FieldId::FlapsHandle);
        assert!(m.map(0.0).unwrap() < 0.70);
        assert!(m.map(1.0).unwrap() < 0.70);
    }

    #[test]
    fn flaps_lever_2_and_3_are_landing_config() {
        let m = abbildung("Hot Start CL650", FieldId::FlapsHandle);
        assert!(m.map(2.0).unwrap() >= 0.70);
        assert!(m.map(3.0).unwrap() >= 0.70);
    }

    #[test]
    fn detent_table_nonfinite_yields_no_value() {
        let m = abbildung("Hot Start CL650", FieldId::FlapsHandle);
        assert_eq!(m.map(f32::NAN), None);
        assert_eq!(m.map(f32::INFINITY), None);
    }

    #[test]
    fn detent_table_rejects_non_integer_value() {
        let m = abbildung("Hot Start CL650", FieldId::FlapsHandle);
        // 1.51 must NOT snap onto detent 2 (LE7 / QS-R2-P2).
        assert_eq!(m.map(1.51), None);
        assert_eq!(m.map(1.49), None);
        // within tolerance is still accepted
        assert_eq!(m.map(2.03), Some(0.80));
    }

    #[test]
    fn detent_table_out_of_range_yields_no_value() {
        let m = abbildung("Hot Start CL650", FieldId::FlapsHandle);
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
        let m = abbildung("Rotate MD-11", FieldId::FlapsHandle);
        assert_eq!(m.map(0.0), Some(0.0));
        assert_eq!(m.map(15.0), Some(0.30));
        assert_eq!(m.map(28.0), Some(0.55));
        assert_eq!(m.map(35.0), Some(0.80));
        assert_eq!(m.map(50.0), Some(1.0));
    }

    #[test]
    fn md11_flaps_35_and_50_are_landing_config() {
        // LANDING-CONFIG-Check ist `flaps_position >= 0.70`.
        let m = abbildung("Rotate MD-11", FieldId::FlapsHandle);
        assert!(m.map(35.0).unwrap() >= 0.70);
        assert!(m.map(50.0).unwrap() >= 0.70);
    }

    #[test]
    fn md11_flaps_0_15_28_are_not_landing_config() {
        let m = abbildung("Rotate MD-11", FieldId::FlapsHandle);
        assert!(m.map(0.0).unwrap() < 0.70);
        assert!(m.map(15.0).unwrap() < 0.70);
        assert!(m.map(28.0).unwrap() < 0.70);
    }

    #[test]
    fn md11_degree_table_snaps_to_nearest_detent() {
        // Ein leichter Float-Wackler um eine Raste herum snappt sauber.
        let m = abbildung("Rotate MD-11", FieldId::FlapsHandle);
        assert_eq!(m.map(34.6), Some(0.80));
        assert_eq!(m.map(15.4), Some(0.30));
        // Mitten zwischen 28 und 35 → näher an 28.
        assert_eq!(m.map(30.0), Some(0.55));
    }

    #[test]
    fn degree_table_nonfinite_yields_no_value() {
        let m = abbildung("Rotate MD-11", FieldId::FlapsHandle);
        assert_eq!(m.map(f32::NAN), None);
        assert_eq!(m.map(f32::INFINITY), None);
    }

    #[test]
    fn md11_profile_matches_title() {
        let md11 = profile()
            .iter()
            .find(|p| p.name == "Rotate MD-11")
            .expect("MD-11 profile present");
        // Echter Title aus Michels Log: "MD11 Rotate MD-11P".
        assert!(md11.matches_title("MD11 Rotate MD-11P"));
        assert!(!md11.matches_title("Boeing 737-800"));
    }

    #[test]
    fn md11_active_catalog_overrides_flaps() {
        let md11 = profile()
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
        let z = profile()
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
        let z = profile()
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
        profile().iter().find(|p| p.name == name).expect(name)
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
        let pi = profile().iter().position(|p| p.name == "FlightFactor 757").unwrap();
        let mut s = crate::dataref::XPlaneState::default();
        s.apply_field(FieldId::FfFluegellicht, 1.0);
        s.apply_field(FieldId::FfAutobrake, 3.0);
        s.apply_field(FieldId::TolissFluegellicht, 1.0);
        profil_ersetzungen_leeren(&mut s, Some(pi), Some(pi));
        assert_eq!(s.ff_fluegellicht, Some(true), "gleiches Profil: nichts");
        // FF757 → FF767: dieselben Ersetzungen, der Wert bleibt (Codex-QS
        // Runde 3, Rueckfall Plugin → RREF).
        let pi767 = profile().iter().position(|p| p.name == "FlightFactor 767").unwrap();
        profil_ersetzungen_leeren(&mut s, Some(pi), Some(pi767));
        assert_eq!(s.ff_fluegellicht, Some(true));
        assert_eq!(s.ff_autobrake, Some(3.0));
        // FF757 → FF777: das FF777-Profil ersetzt nur den Speedbrake —
        // Fluegellicht/Autobrake aus anim/* muessen weg.
        let pi777 = profile().iter().position(|p| p.name == "FlightFactor 777").unwrap();
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
        let ab = abbildung("Hot Start CL650", FieldId::FfAnschnallzeichen);
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

    /// Erkennung (Codex-QS 28.10.2026): Titel und Probe waehlen das richtige
    /// neue Profil, der ATR nur ueber die Probe.
    #[test]
    fn neue_profile_werden_erkannt() {
        let idx = |n: &str| profile().iter().position(|p| p.name == n).unwrap();
        let keine = vec![false; profile().len()];
        let wahl = |titel: Option<&str>, probe: Option<usize>| {
            let mut frisch = keine.clone();
            if let Some(pi) = probe {
                frisch[pi] = true;
            }
            crate::adapter::desired_profile(titel, &frisch, &frisch)
        };
        assert_eq!(wahl(Some("Felis 747-200 Cargo GE"), None), Some(idx("Felis 747-200")));
        assert_eq!(wahl(Some("Dash 8 Q400"), None), Some(idx("FlyJSim Q4XP")));
        assert_eq!(wahl(Some("ATR 72-600"), None), None);
        for n in ["DeltaWing ATR 72-600", "Felis 747-200", "FlyJSim Q4XP"] {
            assert_eq!(wahl(None, Some(idx(n))), Some(idx(n)), "{n}");
        }
        // Jede Probe gehoert nur einem Profil (ausser FF757/767, die sich
        // ueber den Titel trennen).
        for (i, p) in profile().iter().enumerate() {
            for q in &profile()[i + 1..] {
                if p.probe_dataref == q.probe_dataref {
                    assert!(p.titel_und_probe && q.titel_und_probe, "{} / {}", p.name, q.name);
                }
            }
        }
        // Bestehende Titel bleiben bei ihren Profilen.
        assert_eq!(profile_index_for_title("MD11 Rotate MD-11P"), Some(idx("Rotate MD-11")));
        assert_eq!(
            profile_index_for_title("Challenger 650 published by X-Aviation"),
            Some(idx("Hot Start CL650"))
        );
        assert_eq!(profile_index_for_title("Boeing 737-800X"), Some(idx("Laminar/Zibo 737-800")));
    }

    /// Profilwechsel: die neuen Add-on-Ersetzungen (Lampen, Schilder,
    /// Fluegellicht) werden beim Wechsel weg vom Profil geleert.
    #[test]
    fn neue_profile_leeren_ihre_ersetzungen() {
        for name in ["Felis 747-200", "FlyJSim Q4XP", "DeltaWing ATR 72-600", "Hot Start CL650"] {
            let pi = profile().iter().position(|p| p.name == name).unwrap();
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
