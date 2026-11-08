//! Nachlauf nach der Abgabe (P-11.30, Thomas 08.11.2026).
//!
//! Der Flug wird abgegeben, sobald die Triebwerke aus sind; das Aussteigen und
//! die letzten Bilder kommen erst danach. Nach einer erfolgreichen Abgabe
//! zaehlen Bilder deshalb noch eine Weile zum abgegebenen Flug. Der Nachlauf endet
//!
//! * nach Ablauf der Zeit (`grenzen.nachlauf_min` in `tabellen/bilder.json`),
//! * beim naechsten AeroACARS-Flug,
//! * sobald das Flugzeug den Abstellplatz verlaesst oder der Simulator woanders
//!   steht: Was danach entsteht, ist ein anderer Flug (P-11.30: Fluege ohne
//!   AeroACARS bleiben unangetastet).
//!
//! `.nachlauf.json` beginnt mit einem Punkt und ist keine Gruppe. Die Datei
//! sorgt dafuer, dass ein Neustart des Programms den Nachlauf nicht beendet.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::sicher;
use crate::Fehler;

/// Gilt, wenn das Paket keinen Wert nennt (aeltere Generation).
pub const VORGABE_MIN: u32 = 30;
/// Hoechstwert, auch fuer eine beschaedigte Tabelle oder Datei.
pub const MAX_MIN: u32 = 240;
/// Ab dieser Entfernung vom Abstellplatz ist der Nachlauf vorbei.
pub const MAX_WEG_M: f64 = 500.0;
/// Steht das Flugzeug bei der Abgabe weiter vom Aufsetzpunkt weg, hat der Pilot
/// schon etwas anderes geladen (DAL667): kein Nachlauf.
pub const MAX_ABSTAND_AUFSETZPUNKT_M: f64 = 20_000.0;

const DATEI: &str = ".nachlauf.json";
const MAX_DATEI: u64 = 8 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Nachlauf {
    pub pirep_id: String,
    #[serde(default)]
    pub rufzeichen: Option<String>,
    pub abflug: String,
    pub ziel: String,
    #[serde(default)]
    pub flugzeug_icao: Option<String>,
    /// Die Phase bei der Abgabe, wie sie ein Bild im Flug getragen haette.
    #[serde(default)]
    pub phase: Value,
    pub bis_ms: i64,
    pub breite: f64,
    pub laenge: f64,
}

/// Der Wert aus `tabellen/bilder.json`; fehlt er oder taugt er nicht: [`VORGABE_MIN`].
pub fn minuten_aus_tabelle(json: &str) -> u32 {
    serde_json::from_str::<Value>(json)
        .ok()
        .and_then(|v| v["grenzen"]["nachlauf_min"].as_u64())
        .filter(|m| (1..=u64::from(MAX_MIN)).contains(m))
        .map_or(VORGABE_MIN, |m| m as u32)
}

/// Eine Position, die der Simulator wirklich gesetzt hat (nicht NaN, nicht 0/0 im Menue).
fn brauchbar(p: (f64, f64)) -> bool {
    p.0.is_finite() && p.1.is_finite() && !(p.0.abs() < 0.02 && p.1.abs() < 0.02)
}

fn abstand_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (la1, la2) = (a.0.to_radians(), b.0.to_radians());
    let (d_la, d_lo) = (la2 - la1, (b.1 - a.1).to_radians());
    let h = (d_la / 2.0).sin().powi(2) + la1.cos() * la2.cos() * (d_lo / 2.0).sin().powi(2);
    2.0 * 6_371_000.0 * h.sqrt().min(1.0).asin()
}

/// Darf an dieser Position ein Nachlauf beginnen? Ohne Aufsetzpunkt (Flug aus
/// aelterem Stand fortgesetzt) entscheidet allein die Position.
pub fn darf_beginnen(position: (f64, f64), aufsetzpunkt: Option<(f64, f64)>) -> bool {
    brauchbar(position) && aufsetzpunkt.is_none_or(|a| !brauchbar(a) || abstand_m(position, a) <= MAX_ABSTAND_AUFSETZPUNKT_M)
}

impl Nachlauf {
    /// Gilt der Nachlauf noch? Ohne brauchbare Position (Simulator weg, Menue)
    /// entscheidet die Zeit: Eine fehlende Position beweist keinen Ortswechsel.
    pub fn gilt(&self, jetzt_ms: i64, position: Option<(f64, f64)>) -> bool {
        // Mehr Restzeit als der Hoechstwert: Uhr verstellt oder Datei beschaedigt.
        if jetzt_ms >= self.bis_ms || self.bis_ms - jetzt_ms > i64::from(MAX_MIN) * 60_000 {
            return false;
        }
        position.filter(|p| brauchbar(*p)).is_none_or(|p| abstand_m(p, (self.breite, self.laenge)) <= MAX_WEG_M)
    }
}

fn datei(wurzel: &Path) -> PathBuf {
    wurzel.join(DATEI)
}

/// Legt den Nachlauf atomar ab (Temp, dann Umbenennen).
pub fn speichern(wurzel: &Path, n: &Nachlauf) -> Result<(), Fehler> {
    if fs::symlink_metadata(wurzel).is_ok() {
        sicher::echten_ordner_sichern(wurzel, "Der Bilderordner")?;
    }
    fs::create_dir_all(wurzel)?;
    sicher::echten_ordner_sichern(wurzel, "Der Bilderordner")?;
    let bytes = serde_json::to_vec(n).map_err(|e| Fehler::Eingabe(e.to_string()))?;
    let tmp = wurzel.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let r = (|| -> std::io::Result<()> {
        let mut f = sicher::neu_anlegen(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, datei(wurzel))
    })();
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r.map_err(Fehler::from)
}

/// Der abgelegte Nachlauf; alles Unlesbare zaehlt als keiner.
pub fn lesen(wurzel: &Path) -> Option<Nachlauf> {
    let f = sicher::lesend_ohne_folgen(&datei(wurzel)).ok()?;
    let mut bytes = Vec::new();
    f.take(MAX_DATEI + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_DATEI {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

pub fn entfernen(wurzel: &Path) {
    let _ = fs::remove_file(datei(wurzel));
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAND: (f64, f64) = (50.0379, 8.5622);
    const JETZT: i64 = 1_800_000_000_000;

    fn nachlauf() -> Nachlauf {
        Nachlauf {
            pirep_id: "P1".into(),
            rufzeichen: Some("GSG123".into()),
            abflug: "EDDM".into(),
            ziel: "EDDF".into(),
            flugzeug_icao: Some("A320".into()),
            phase: Value::String("Arrived".into()),
            bis_ms: JETZT + 30 * 60_000,
            breite: STAND.0,
            laenge: STAND.1,
        }
    }

    #[test]
    fn gilt_bis_zum_ablauf_und_keine_millisekunde_laenger() {
        let n = nachlauf();
        assert!(n.gilt(JETZT, Some(STAND)));
        assert!(n.gilt(n.bis_ms - 1, Some(STAND)));
        assert!(!n.gilt(n.bis_ms, Some(STAND)));
    }

    #[test]
    fn am_abstellplatz_gilt_er_weggerollt_oder_woanders_nicht() {
        let n = nachlauf();
        assert!(n.gilt(JETZT, Some((STAND.0 + 0.003, STAND.1))), "gut 300 m: Kamera und Pushback-Ruck beenden nichts");
        assert!(!n.gilt(JETZT, Some((STAND.0 + 0.006, STAND.1))), "gut 660 m: er rollt, das ist ein anderer Flug");
        assert!(!n.gilt(JETZT, Some((-22.8, -43.25))), "Simulator steht in Rio");
    }

    #[test]
    fn ohne_brauchbare_position_entscheidet_die_zeit() {
        let n = nachlauf();
        assert!(n.gilt(JETZT, None), "Simulator weg");
        assert!(n.gilt(JETZT, Some((0.0, 0.0))), "Menue meldet 0/0");
        assert!(n.gilt(JETZT, Some((f64::NAN, 8.0))));
        assert!(!n.gilt(n.bis_ms, None));
    }

    #[test]
    fn verstellte_uhr_oder_beschaedigter_wert_verlaengert_nichts() {
        let mut n = nachlauf();
        n.bis_ms = JETZT + (i64::from(MAX_MIN) + 1) * 60_000;
        assert!(!n.gilt(JETZT, Some(STAND)));
    }

    #[test]
    fn beginnt_nur_nahe_am_aufsetzpunkt() {
        let bahn = (50.0333, 8.5706);
        assert!(darf_beginnen(STAND, Some(bahn)), "Abstellplatz am Landeplatz");
        assert!(darf_beginnen(STAND, None), "Flug ohne Aufsetzpunkt");
        assert!(!darf_beginnen((-22.8, -43.25), Some(bahn)), "DAL667: geladen woanders");
        assert!(!darf_beginnen((0.0, 0.0), Some(bahn)), "keine Position, kein Anker");
    }

    #[test]
    fn wert_aus_der_tabelle_sonst_die_vorgabe() {
        assert_eq!(minuten_aus_tabelle(r#"{"grenzen":{"nachlauf_min":45}}"#), 45);
        assert_eq!(minuten_aus_tabelle(r#"{"grenzen":{}}"#), VORGABE_MIN, "aeltere Generation");
        assert_eq!(minuten_aus_tabelle(r#"{"grenzen":{"nachlauf_min":0}}"#), VORGABE_MIN);
        assert_eq!(minuten_aus_tabelle(r#"{"grenzen":{"nachlauf_min":9999}}"#), VORGABE_MIN);
        assert_eq!(minuten_aus_tabelle("kein json"), VORGABE_MIN);
    }

    #[test]
    fn ueberlebt_den_neustart_und_laesst_sich_entfernen() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("bilder");
        assert_eq!(lesen(&w), None);
        speichern(&w, &nachlauf()).unwrap();
        assert_eq!(lesen(&w), Some(nachlauf()));
        assert!(crate::speicher::liste(&w).eintraege.is_empty(), "die Datei ist keine Gruppe");
        entfernen(&w);
        assert_eq!(lesen(&w), None);
        entfernen(&w); // doppelt ist kein Fehler
    }

    #[test]
    fn unlesbare_datei_ist_kein_nachlauf() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join(DATEI), b"{kaputt").unwrap();
        assert_eq!(lesen(t.path()), None);
    }
}
