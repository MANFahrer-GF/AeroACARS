//! v3 Etappe 2 (P-3.52): MSFS-Add-ons aus dem Laufzeit-Paket
//! (`profile/msfs.json`).
//!
//! Zwischenstand: Ein Paketprofil greift nur, wenn die eingebaute Erkennung
//! ([`crate::AircraftProfile::detect_mit_pfad`]) `Default` ergibt — ein
//! **neues** Add-on laesst sich so ohne Release anbinden. Die eingebauten
//! Profile bleiben vorerst Code.
//!
//! Was ein Paket darf, ist hier festgelegt (Klasse A, P-2.10): nur die Felder
//! aus [`Feld`] setzen, nur aus `L:`-Variablen, nur mit den Arten aus [`Art`].
//! Ein neues Feld oder eine neue Art ist eine neue Faehigkeit (P-2.15).
//!
//! **Schutz gegen Tippfehler** (Advisor 29.10.): MSFS liefert fuer eine
//! unbekannte `L:`-Variable einfach 0 statt eines Fehlers. Eine Regel greift
//! deshalb erst, wenn eine ihrer Variablen in dieser Sitzung einmal ungleich
//! 0 war; bis dahin bleibt der Standardwert stehen. Angewendet wird nach der
//! eingebauten Logik und nur auf Werte eines gueltigen Blocks (der Adapter
//! reicht verworfene Bloecke gar nicht erst herein).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::SimSnapshot;

/// Felder, die ein Paketprofil setzen darf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feld {
    LichtLanding,
    LichtBeacon,
    LichtStrobe,
    LichtTaxi,
    LichtNav,
    LichtLogo,
    LichtWing,
    Parkbremse,
    ApMaster,
    ApHeading,
    ApAltitude,
    ApNav,
    ApApproach,
    ApuSchalter,
    /// Anschnallzeichen 0 = aus, 1 = auto, 2 = an.
    Anschnallzeichen,
    /// Rauchverbot 0 = aus, 1 = auto, 2 = an.
    Rauchverbot,
    Autobrake,
    XpdrModus,
}

/// Wie die Rohwerte zum Feldwert werden.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "art", rename_all = "snake_case", deny_unknown_fields)]
pub enum Art {
    /// Ja/Nein: irgendeine Quelle ungleich 0.
    UngleichNull,
    /// Ja/Nein: irgendeine Quelle ueber `ab` (LED-Helligkeit u. ae.).
    Schwelle { ab: f64 },
    /// Schild 0/1/2 aus der ersten Quelle: `roh` → `stufe` (Raste auf ±0,25,
    /// sonst kein Wert — wie `bbs757_schild`).
    Stufen { tabelle: Vec<RohStufe> },
    /// Beschriftung aus der ersten Quelle: `roh` → `text`.
    Beschriftung { tabelle: Vec<RohText> },
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RohStufe {
    pub roh: f64,
    pub stufe: u8,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RohText {
    pub roh: f64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Regel {
    pub feld: Feld,
    /// `L:`-Variablen; bei Ja/Nein-Arten per ODER verknuepft.
    pub quellen: Vec<String>,
    pub abbildung: Art,
    /// Ja/Nein-Felder: der Standardwert zaehlt per ODER mit (Standard-SimVar
    /// bleibt verdrahtet, falls das Add-on sie doch bedient).
    #[serde(default)]
    pub standard_oder: bool,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Erkennung {
    /// Alle Teile muessen im Titel stehen (Gross/klein egal).
    #[serde(default)]
    pub titel_enthaelt_alle: Vec<String>,
    /// ICAO-Typ (`ATC MODEL`, bereinigt) muss einer davon sein.
    #[serde(default)]
    pub icao: Vec<String>,
    /// Einer davon muss im Pfad der `aircraft.cfg` stehen (Gross/klein egal).
    #[serde(default)]
    pub pfad_enthaelt: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profil {
    pub name: String,
    pub erkennung: Erkennung,
    pub regeln: Vec<Regel>,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Datei {
    format: u32,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
    pub profile: Vec<Profil>,
}

/// Liest `profile/msfs.json` (fail-closed): Format 1, je Regel mindestens eine
/// Quelle und nur `L:`-Variablen, Tabellen nicht leer, jedes Profil mit
/// mindestens einem Erkennungsmerkmal, Namen eindeutig.
pub fn lesen(json: &str) -> Result<Datei, String> {
    let d: Datei = serde_json::from_str(json).map_err(|e| format!("profile/msfs.json: {e}"))?;
    if d.format != 1 {
        return Err(format!("profile/msfs.json: Format {} unbekannt", d.format));
    }
    let mut namen = HashSet::new();
    for p in &d.profile {
        if !namen.insert(p.name.as_str()) {
            return Err(format!("profile/msfs.json: Profil {} doppelt", p.name));
        }
        let e = &p.erkennung;
        if e.titel_enthaelt_alle.is_empty() && e.icao.is_empty() && e.pfad_enthaelt.is_empty() {
            return Err(format!("profile/msfs.json: {} ohne Erkennung", p.name));
        }
        for r in &p.regeln {
            if r.quellen.is_empty() || r.quellen.iter().any(|q| !q.starts_with("L:") || q.len() < 3) {
                return Err(format!("profile/msfs.json: {} {:?}: nur L:-Variablen", p.name, r.feld));
            }
            let ja_nein = matches!(r.abbildung, Art::UngleichNull | Art::Schwelle { .. });
            let passt = match r.feld {
                Feld::Anschnallzeichen | Feld::Rauchverbot => matches!(&r.abbildung, Art::Stufen { tabelle } if !tabelle.is_empty()),
                Feld::Autobrake | Feld::XpdrModus => matches!(&r.abbildung, Art::Beschriftung { tabelle } if !tabelle.is_empty()),
                _ => ja_nein,
            };
            if !passt || (r.standard_oder && !ja_nein) {
                return Err(format!("profile/msfs.json: {} {:?}: Art passt nicht zum Feld", p.name, r.feld));
            }
        }
    }
    Ok(d)
}

/// Eingebauter Stand = Datei im Paketordner.
pub const EINGEBAUT: &str = include_str!("../../../../../bundle/profile/msfs.json");

static QUELLE: OnceLock<fn() -> &'static Datei> = OnceLock::new();

/// Die geltenden Paketprofile (Quelle aus dem einen Paketstand des Clients,
/// sonst eingebaut).
pub fn profile() -> &'static [Profil] {
    let d = match QUELLE.get() {
        Some(q) => q(),
        None => {
            static EIGEN: OnceLock<Datei> = OnceLock::new();
            EIGEN.get_or_init(|| lesen(EINGEBAUT).expect("eingebaute profile/msfs.json"))
        }
    };
    &d.profile
}

/// Meldet die Quelle an (einmal je Lauf).
pub fn quelle_setzen(quelle: fn() -> &'static Datei) -> bool {
    QUELLE.set(quelle).is_ok()
}

impl Erkennung {
    pub fn passt(&self, titel: &str, icao: &str, pfad: Option<&str>) -> bool {
        let t = titel.to_lowercase();
        let i = icao.trim().to_ascii_uppercase();
        let pfad = pfad.map(str::to_lowercase);
        (self.titel_enthaelt_alle.iter().all(|x| t.contains(&x.to_lowercase())))
            && (self.icao.is_empty() || self.icao.iter().any(|x| x.eq_ignore_ascii_case(&i)))
            && (self.pfad_enthaelt.is_empty()
                || pfad.as_deref().is_some_and(|p| self.pfad_enthaelt.iter().any(|x| p.contains(&x.to_lowercase()))))
    }
}

/// Erstes passendes Paketprofil aus `profile`.
pub fn erkennen<'a>(profile: &'a [Profil], titel: &str, icao: &str, pfad: Option<&str>) -> Option<&'a Profil> {
    profile.iter().find(|p| p.erkennung.passt(titel, icao, pfad))
}

/// Alle `L:`-Variablen eines Profils (ohne Doppelte, in Reihenfolge) — fuer
/// die Datendefinition des Adapters.
pub fn variablen(p: &Profil) -> Vec<&str> {
    let mut v: Vec<&str> = Vec::new();
    for q in p.regeln.iter().flat_map(|r| &r.quellen) {
        if !v.contains(&q.as_str()) {
            v.push(q);
        }
    }
    v
}

/// Laufzustand eines aktiven Paketprofils in einer Sitzung.
#[derive(Debug, Default)]
pub struct Lauf {
    /// Variablen, die in dieser Sitzung schon einmal ungleich 0 waren.
    belebt: HashSet<String>,
}

impl Lauf {
    /// Wendet `p` mit den Rohwerten `werte` (Name → Wert) auf `s` an. Fehlende
    /// Werte zaehlen als nicht geliefert; eine Regel greift erst, wenn eine
    /// ihrer Quellen in dieser Sitzung einmal ungleich 0 war.
    pub fn anwenden(&mut self, p: &Profil, werte: &HashMap<String, f64>, s: &mut SimSnapshot) {
        for (name, v) in werte {
            if *v != 0.0 && v.is_finite() {
                self.belebt.insert(name.clone());
            }
        }
        for r in &p.regeln {
            if !r.quellen.iter().any(|q| self.belebt.contains(q)) {
                continue;
            }
            let roh: Vec<f64> = r.quellen.iter().filter_map(|q| werte.get(q).copied()).collect();
            if roh.is_empty() {
                continue;
            }
            match &r.abbildung {
                Art::UngleichNull | Art::Schwelle { .. } => {
                    let an = roh.iter().any(|v| match r.abbildung {
                        Art::Schwelle { ab } => *v > ab,
                        _ => *v != 0.0,
                    });
                    ja_nein_setzen(s, r.feld, an, r.standard_oder);
                }
                Art::Stufen { tabelle } => {
                    if let Some(t) = tabelle.iter().find(|t| (t.roh - roh[0]).abs() < 0.25) {
                        match r.feld {
                            Feld::Anschnallzeichen => s.seatbelts_sign = Some(t.stufe),
                            Feld::Rauchverbot => s.no_smoking_sign = Some(t.stufe),
                            _ => {}
                        }
                    }
                }
                Art::Beschriftung { tabelle } => {
                    if let Some(t) = tabelle.iter().find(|t| (t.roh - roh[0]).abs() < 0.25) {
                        match r.feld {
                            Feld::Autobrake => s.autobrake = Some(t.text.clone()),
                            Feld::XpdrModus => s.xpdr_mode_label = Some(t.text.clone()),
                            _ => {}
                        }
                    }
                }
            }
        }
    }
}

fn ja_nein_setzen(s: &mut SimSnapshot, feld: Feld, an: bool, standard_oder: bool) {
    let oder = |alt: Option<bool>| Some(an || (standard_oder && alt == Some(true)));
    match feld {
        Feld::LichtLanding => s.light_landing = oder(s.light_landing),
        Feld::LichtBeacon => s.light_beacon = oder(s.light_beacon),
        Feld::LichtStrobe => s.light_strobe = oder(s.light_strobe),
        Feld::LichtTaxi => s.light_taxi = oder(s.light_taxi),
        Feld::LichtNav => s.light_nav = oder(s.light_nav),
        Feld::LichtLogo => s.light_logo = oder(s.light_logo),
        Feld::LichtWing => s.light_wing = oder(s.light_wing),
        Feld::Parkbremse => s.parking_brake = an || (standard_oder && s.parking_brake),
        Feld::ApMaster => s.autopilot_master = oder(s.autopilot_master),
        Feld::ApHeading => s.autopilot_heading = oder(s.autopilot_heading),
        Feld::ApAltitude => s.autopilot_altitude = oder(s.autopilot_altitude),
        Feld::ApNav => s.autopilot_nav = oder(s.autopilot_nav),
        Feld::ApApproach => s.autopilot_approach = oder(s.autopilot_approach),
        Feld::ApuSchalter => s.apu_switch = oder(s.apu_switch),
        Feld::Anschnallzeichen | Feld::Rauchverbot | Feld::Autobrake | Feld::XpdrModus => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: &str = r#"{"format":1,"profile":[{"name":"Probe 757","erkennung":{"titel_enthaelt_alle":["probe","757"],"icao":["B752"]},"regeln":[
        {"feld":"licht_beacon","quellen":["L:P_BEACON"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"licht_landing","quellen":["L:P_LDG_L","L:P_LDG_R"],"abbildung":{"art":"ungleich_null"}},
        {"feld":"ap_master","quellen":["L:P_CMD"],"abbildung":{"art":"schwelle","ab":10.0}},
        {"feld":"anschnallzeichen","quellen":["L:P_SEATBELT"],"abbildung":{"art":"stufen","tabelle":[{"roh":0,"stufe":0},{"roh":1,"stufe":1},{"roh":2,"stufe":2}]}},
        {"feld":"autobrake","quellen":["L:P_AB"],"abbildung":{"art":"beschriftung","tabelle":[{"roh":0,"text":"RTO"},{"roh":1,"text":"OFF"}]}}
    ]}]}"#;

    fn w(paare: &[(&str, f64)]) -> HashMap<String, f64> {
        paare.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn eingebaute_datei_ist_lesbar() {
        lesen(EINGEBAUT).unwrap();
    }

    #[test]
    fn erkennung_titel_icao_pfad() {
        let d = lesen(PROBE).unwrap();
        assert!(erkennen(&d.profile, "Probe Sim 757-200 LTU", "b752", None).is_some());
        assert!(erkennen(&d.profile, "Probe Sim 757-200", "B763", None).is_none());
        assert!(erkennen(&d.profile, "Anderes 757", "B752", None).is_none());
        assert_eq!(variablen(&d.profile[0]).len(), 6);
    }

    #[test]
    fn regeln_setzen_die_felder() {
        let d = lesen(PROBE).unwrap();
        let p = &d.profile[0];
        let mut lauf = Lauf::default();
        let mut s = SimSnapshot::default();
        lauf.anwenden(
            p,
            &w(&[("L:P_BEACON", 1.0), ("L:P_LDG_L", 0.0), ("L:P_LDG_R", 1.0), ("L:P_CMD", 50.0), ("L:P_SEATBELT", 2.0), ("L:P_AB", 0.0)]),
            &mut s,
        );
        assert_eq!(s.light_beacon, Some(true));
        assert_eq!(s.light_landing, Some(true));
        assert_eq!(s.autopilot_master, Some(true));
        assert_eq!(s.seatbelts_sign, Some(2));
        // L:P_AB war nie ungleich 0 → Regel greift (noch) nicht
        assert_eq!(s.autobrake, None);
        lauf.anwenden(p, &w(&[("L:P_AB", 1.0)]), &mut s);
        assert_eq!(s.autobrake.as_deref(), Some("OFF"));
        lauf.anwenden(p, &w(&[("L:P_AB", 0.0)]), &mut s);
        assert_eq!(s.autobrake.as_deref(), Some("RTO"), "einmal belebt, dann gilt auch 0");
    }

    /// Advisor 29.10.: Eine falsch geschriebene LVar liefert in MSFS immer 0.
    /// Dann darf das Profil die Standardwerte nicht ueberschreiben.
    #[test]
    fn tippfehler_laesst_standardwerte_stehen() {
        let d = lesen(PROBE).unwrap();
        let mut lauf = Lauf::default();
        let mut s = SimSnapshot { light_beacon: Some(true), light_landing: Some(true), autopilot_master: Some(true), ..Default::default() };
        let vorher = s.clone();
        for _ in 0..100 {
            lauf.anwenden(&d.profile[0], &w(&[("L:P_BEACON", 0.0), ("L:P_LDG_L", 0.0), ("L:P_LDG_R", 0.0), ("L:P_CMD", 0.0), ("L:P_SEATBELT", 0.0), ("L:P_AB", 0.0)]), &mut s);
        }
        assert_eq!(format!("{s:?}"), format!("{vorher:?}"));
    }

    #[test]
    fn standard_oder_haelt_den_standardwert() {
        let d = lesen(PROBE).unwrap();
        let mut lauf = Lauf::default();
        let mut s = SimSnapshot::default();
        lauf.anwenden(&d.profile[0], &w(&[("L:P_BEACON", 1.0)]), &mut s);
        s.light_beacon = Some(true);
        lauf.anwenden(&d.profile[0], &w(&[("L:P_BEACON", 0.0)]), &mut s);
        assert_eq!(s.light_beacon, Some(true), "Standard an, LVar aus → an (ODER)");
    }

    #[test]
    fn falsches_format_ist_ein_lesefehler() {
        let gut: serde_json::Value = serde_json::from_str(PROBE).unwrap();
        let mut keine_lvar = gut.clone();
        keine_lvar["profile"][0]["regeln"][0]["quellen"][0] = "A:LIGHT BEACON".into();
        assert!(lesen(&keine_lvar.to_string()).is_err());
        let mut falsche_art = gut.clone();
        falsche_art["profile"][0]["regeln"][3]["abbildung"] = serde_json::json!({"art": "ungleich_null"});
        assert!(lesen(&falsche_art.to_string()).is_err());
        let mut ohne_erkennung = gut.clone();
        ohne_erkennung["profile"][0]["erkennung"] = serde_json::json!({});
        assert!(lesen(&ohne_erkennung.to_string()).is_err());
        let mut fremd = gut;
        fremd["profile"][0]["regeln"][0]["extra"] = 1.into();
        assert!(lesen(&fremd.to_string()).is_err());
    }

    /// Ausdrucksnachweis (Advisor 29.10.): das eingebaute BlueBird-757-Profil
    /// (`sim-msfs` `telemetry.rs`, Hersteller-Liste 21.10.2026) im
    /// Paketformat — dieselben Erwartungen wie die Windows-Tests
    /// `bluebird_757_*` mit `runde2`. Unterschied mit Absicht: Vor der ersten
    /// LVar ungleich 0 bleibt der Standardwert stehen (Tippfehler-Schutz).
    const BLUEBIRD: &str = r#"{"format":1,"profile":[{"name":"BlueBird 757 (Paket)","erkennung":{"titel_enthaelt_alle":["bluebird","757"]},"regeln":[
        {"feld":"licht_beacon","quellen":["L:B757_OVHD_BEACON_LIGHT"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"licht_strobe","quellen":["L:B757_OVHD_STROBE"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"licht_nav","quellen":["L:B757_OVHD_NAV_LIGHTS"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"licht_logo","quellen":["L:B757_OVHD_LOGO_BUTTON"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"licht_wing","quellen":["L:B757_OVHD_WING_LIGHTS"],"abbildung":{"art":"ungleich_null"}},
        {"feld":"licht_landing","quellen":["L:B757_OVHD_LEFT_LANDING_LIGHT","L:B757_OVHD_RIGHT_LANDING_LIGHT","L:B757_OVHD_NOSE_LANDING_LIGHT"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"apu_schalter","quellen":["L:B757_OVHD_APU_SWITCH"],"abbildung":{"art":"schwelle","ab":0.49}},
        {"feld":"ap_master","quellen":["L:B757_MCP_CMD_1_BUTTON","L:B757_MCP_CMD_2_BUTTON","L:B757_MCP_CMD_3_BUTTON"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"ap_heading","quellen":["L:B757_MCP_HOLD_1_BUTTON"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"ap_altitude","quellen":["L:B757_MCP_HOLD_2_BUTTON"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"ap_nav","quellen":["L:B757_MCP_LNAV_BUTTON"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"ap_approach","quellen":["L:B757_MCP_LOC_BUTTON","L:B757_MCP_APP_BUTTON"],"abbildung":{"art":"ungleich_null"},"standard_oder":true},
        {"feld":"anschnallzeichen","quellen":["L:B757_OVHD_SEATBELTS_SWITCH"],"abbildung":{"art":"stufen","tabelle":[{"roh":0,"stufe":0},{"roh":1,"stufe":1},{"roh":2,"stufe":2}]}},
        {"feld":"rauchverbot","quellen":["L:B757_OVHD_NO_SMOKING_SWITCH"],"abbildung":{"art":"stufen","tabelle":[{"roh":0,"stufe":0},{"roh":1,"stufe":1},{"roh":2,"stufe":2}]}},
        {"feld":"autobrake","quellen":["L:BBS_AUTOBRAKE_KNOB"],"abbildung":{"art":"beschriftung","tabelle":[{"roh":0,"text":"RTO"},{"roh":1,"text":"OFF"},{"roh":2,"text":"DISARM"},{"roh":3,"text":"1"},{"roh":4,"text":"2"},{"roh":5,"text":"3"},{"roh":6,"text":"4"},{"roh":7,"text":"MAX"}]}},
        {"feld":"xpdr_modus","quellen":["L:B757_PED_TCAS_MODE_KNOB"],"abbildung":{"art":"beschriftung","tabelle":[{"roh":0,"text":"STBY"},{"roh":1,"text":"ALT-OFF"},{"roh":2,"text":"XPNDR"},{"roh":3,"text":"TA"},{"roh":4,"text":"TA-RA"}]}}
    ]}]}"#;

    const ALLE_BBS: [&str; 22] = [
        "L:B757_OVHD_BEACON_LIGHT", "L:B757_OVHD_STROBE", "L:B757_OVHD_NAV_LIGHTS", "L:B757_OVHD_LOGO_BUTTON",
        "L:B757_OVHD_WING_LIGHTS", "L:B757_OVHD_LEFT_LANDING_LIGHT", "L:B757_OVHD_RIGHT_LANDING_LIGHT",
        "L:B757_OVHD_NOSE_LANDING_LIGHT", "L:B757_OVHD_APU_SWITCH", "L:B757_MCP_CMD_1_BUTTON",
        "L:B757_MCP_CMD_2_BUTTON", "L:B757_MCP_CMD_3_BUTTON", "L:B757_MCP_HOLD_1_BUTTON",
        "L:B757_MCP_HOLD_2_BUTTON", "L:B757_MCP_LNAV_BUTTON", "L:B757_MCP_LOC_BUTTON", "L:B757_MCP_APP_BUTTON",
        "L:B757_OVHD_SEATBELTS_SWITCH", "L:B757_OVHD_NO_SMOKING_SWITCH", "L:BBS_AUTOBRAKE_KNOB",
        "L:B757_PED_TCAS_MODE_KNOB", "L:BBS757_UNBENUTZT",
    ];

    /// Wie `runde2`: alle LVars 0, ausser den genannten — nach einer Sitzung,
    /// in der jede LVar schon einmal ungleich 0 war.
    fn bbs(lauf: &mut Lauf, p: &Profil, gesetzt: &[(&str, f64)]) -> SimSnapshot {
        let mut werte: HashMap<String, f64> = ALLE_BBS.iter().map(|k| (k.to_string(), 0.0)).collect();
        for (k, v) in gesetzt {
            werte.insert(k.to_string(), *v);
        }
        let mut s = SimSnapshot::default();
        lauf.anwenden(p, &werte, &mut s);
        s
    }

    #[test]
    fn bluebird_757_als_paketprofil_wie_eingebaut() {
        let d = lesen(BLUEBIRD).unwrap();
        let p = erkennen(&d.profile, "BlueBird 757-200 RR LTU D-AMUZ", "B752", None).unwrap();
        let mut lauf = Lauf::default();
        let belebt: Vec<(&str, f64)> = ALLE_BBS.iter().map(|k| (*k, 1.0)).collect();
        bbs(&mut lauf, p, &belebt);

        let aus = bbs(&mut lauf, p, &[]);
        assert_eq!((aus.light_beacon, aus.light_wing, aus.apu_switch), (Some(false), Some(false), Some(false)));
        assert_eq!(aus.autobrake.as_deref(), Some("RTO"));
        assert_eq!(aus.xpdr_mode_label.as_deref(), Some("STBY"));
        let an = bbs(&mut lauf, p, &[
            ("L:B757_OVHD_BEACON_LIGHT", 1.0), ("L:B757_OVHD_STROBE", 1.0), ("L:B757_OVHD_NAV_LIGHTS", 1.0),
            ("L:B757_OVHD_LOGO_BUTTON", 1.0), ("L:B757_OVHD_WING_LIGHTS", 1.0), ("L:B757_OVHD_NOSE_LANDING_LIGHT", 1.0),
            ("L:B757_OVHD_APU_SWITCH", 2.0), ("L:BBS_AUTOBRAKE_KNOB", 7.0), ("L:B757_PED_TCAS_MODE_KNOB", 4.0),
        ]);
        for (feld, wert) in [("beacon", an.light_beacon), ("strobe", an.light_strobe), ("nav", an.light_nav),
            ("logo", an.light_logo), ("wing", an.light_wing), ("landing", an.light_landing), ("apu", an.apu_switch)] {
            assert_eq!(wert, Some(true), "{feld}");
        }
        assert_eq!(an.autobrake.as_deref(), Some("MAX"));
        assert_eq!(an.xpdr_mode_label.as_deref(), Some("TA-RA"));
        for (knopf, soll) in [(1.0, "OFF"), (2.0, "DISARM"), (3.0, "1"), (6.0, "4")] {
            assert_eq!(bbs(&mut lauf, p, &[("L:BBS_AUTOBRAKE_KNOB", knopf)]).autobrake.as_deref(), Some(soll), "{knopf}");
        }
        let ap = bbs(&mut lauf, p, &[("L:B757_MCP_CMD_3_BUTTON", 1.0), ("L:B757_MCP_HOLD_2_BUTTON", 1.0), ("L:B757_MCP_LNAV_BUTTON", 1.0)]);
        assert_eq!((ap.autopilot_master, ap.autopilot_altitude, ap.autopilot_nav), (Some(true), Some(true), Some(true)));
        assert_eq!((ap.autopilot_heading, ap.autopilot_approach), (Some(false), Some(false)));
        let loc = bbs(&mut lauf, p, &[("L:B757_MCP_LOC_BUTTON", 1.0)]);
        assert_eq!((loc.autopilot_approach, loc.autopilot_master), (Some(true), Some(false)), "LOC = Anflug");
        let schild = bbs(&mut lauf, p, &[("L:B757_OVHD_SEATBELTS_SWITCH", 2.0), ("L:B757_OVHD_NO_SMOKING_SWITCH", 1.0)]);
        assert_eq!((schild.seatbelts_sign, schild.no_smoking_sign), (Some(2), Some(1)));
        assert_eq!(bbs(&mut lauf, p, &[("L:B757_OVHD_SEATBELTS_SWITCH", 1.5)]).seatbelts_sign, None, "keine Raste, kein Wert");
        // Andere Muster bleiben unberuehrt.
        assert!(erkennen(&d.profile, "Baron G58", "BE58", None).is_none());
    }

}
