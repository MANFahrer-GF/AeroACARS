//! Der dauerhafte Eingang `<bilder>/.eingang/<fund_id>.json` (Vertrag, P-11.30).
//!
//! Jeder Fund im aktiven Flug wird zuerst hier abgelegt, mit dem Stand zur
//! Entdeckung. Ereignisse sind nur Wecksignale: Der Eingang ueberlebt einen
//! geschlossenen Tab, eine gedrosselte Webview und einen Neustart. Ein Eintrag
//! verschwindet erst, wenn er abgearbeitet ist (uebernommen oder verworfen).
//! `.eingang` beginnt mit einem Punkt, ist also nie eine Gruppe und fehlt in
//! der Liste der Bilder.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::beobachter::Fund;
use crate::namen::name_pruefen;
use crate::quellen::BildFormat;
use crate::sicher;
use crate::Fehler;

const ORDNER: &str = ".eingang";
const MAX_EINTRAG: u64 = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SimFenster {
    pub breite: u32,
    pub hoehe: u32,
}

/// Ein offener Fund. `herkunft` ist `datei` (neue Datei in einer Quelle) oder
/// `aufnahme` (Fensteraufnahme; die Eingangskopie liegt unter `quelle_pfad`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EingangsFund {
    pub fund_id: String,
    pub herkunft: String,
    pub entdeckt_ms: i64,
    #[serde(default)]
    pub pfad: Option<String>,
    #[serde(default)]
    pub dateizeit_ms: Option<i64>,
    #[serde(default)]
    pub erstellt_ms: Option<i64>,
    pub groesse: u64,
    pub format: BildFormat,
    pub px_breite: Option<u32>,
    pub px_hoehe: Option<u32>,
    pub uebernehmbar: bool,
    /// Quellordner (bei `datei`).
    #[serde(default)]
    pub quelle: Option<String>,
    /// `standard`, `aufgespuert`, `hand` (bei `datei`).
    #[serde(default)]
    pub art: Option<String>,
    /// `<gruppe>/<id>.quelle.png` (bei `aufnahme`).
    #[serde(default)]
    pub quelle_pfad: Option<String>,
    /// SHA-256 der Originaldatei.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub sim_vordergrund: Option<bool>,
    /// Das Simulatorfenster (Pixel), sonst greift die Pixelmass-Regel nicht.
    #[serde(default)]
    pub sim_fenster: Option<SimFenster>,
    /// Stand zur Entdeckung: `zeit_utc`, `breite`, `laenge`, `hoehe_ft`, `gs_kt`, `kurs`,
    /// `flugzeug_titel`, `flugzeug_icao`, `phase`, `pirep_id`, `rufzeichen`, `abflug`, `ziel`,
    /// `simulator`, `naechster_flugplatz`. Nichts Doppeltes daneben.
    pub meta_basis: Value,
}

impl EingangsFund {
    /// Aus einem Fund der Ordnerbeobachtung.
    pub fn aus_fund(f: &Fund, fund_id: String, entdeckt_ms: i64, sim_fenster: Option<SimFenster>, meta_basis: Value) -> Self {
        Self {
            fund_id,
            herkunft: "datei".into(),
            entdeckt_ms,
            pfad: Some(f.pfad.clone()),
            dateizeit_ms: f.dateizeit_ms,
            erstellt_ms: f.erstellt_ms,
            groesse: f.groesse,
            format: f.format,
            px_breite: f.px_breite,
            px_hoehe: f.px_hoehe,
            uebernehmbar: f.uebernehmbar,
            quelle: Some(f.quelle.clone()),
            art: f.art.clone(),
            quelle_pfad: None,
            sha256: f.sha256.clone(),
            sim_vordergrund: f.sim_vordergrund,
            sim_fenster,
            meta_basis,
        }
    }

    /// Fuer eine Fensteraufnahme (`quelle_pfad` = Eingangskopie im Bilderordner).
    #[allow(clippy::too_many_arguments)]
    pub fn aufnahme(fund_id: String, entdeckt_ms: i64, quelle_pfad: String, groesse: u64, breite: u32, hoehe: u32, sim_fenster: Option<SimFenster>, meta_basis: Value) -> Self {
        Self {
            fund_id,
            herkunft: "aufnahme".into(),
            entdeckt_ms,
            pfad: None,
            dateizeit_ms: Some(entdeckt_ms),
            erstellt_ms: None,
            groesse,
            format: BildFormat::Png,
            px_breite: Some(breite),
            px_hoehe: Some(hoehe),
            uebernehmbar: true,
            quelle: None,
            art: None,
            quelle_pfad: Some(quelle_pfad),
            sha256: None,
            sim_vordergrund: Some(true),
            sim_fenster,
            meta_basis,
        }
    }

    /// Gehoert der Fund zu einem AeroACARS-Flug? (Nur dann legt der Kern ihn an.)
    pub fn hat_flug(&self) -> bool {
        self.meta_basis.get("pirep_id").and_then(Value::as_str).is_some_and(|s| !s.trim().is_empty())
    }
}

fn ordner(wurzel: &Path) -> PathBuf {
    wurzel.join(ORDNER)
}

fn datei(wurzel: &Path, fund_id: &str) -> Result<PathBuf, Fehler> {
    name_pruefen("Fund-id", fund_id)?;
    Ok(ordner(wurzel).join(format!("{fund_id}.json")))
}

/// Legt einen Fund atomar ab (Temp, dann Umbenennen). Ohne Flug im Stand: `kein_flug`.
pub fn ablegen(wurzel: &Path, f: &EingangsFund) -> Result<(), Fehler> {
    if !f.hat_flug() {
        return Err(Fehler::KeinFlug("Ohne AeroACARS-Flug entsteht kein Fund".into()));
    }
    let ziel = datei(wurzel, &f.fund_id)?;
    let dir = ordner(wurzel);
    if fs::symlink_metadata(wurzel).is_ok() {
        sicher::echten_ordner_sichern(wurzel, "Der Bilderordner")?;
    }
    if fs::symlink_metadata(&dir).is_ok() {
        sicher::echten_ordner_sichern(&dir, "Der Eingang")?;
    }
    fs::create_dir_all(&dir)?;
    sicher::echten_ordner_sichern(&dir, "Der Eingang")?;
    let bytes = serde_json::to_vec_pretty(f).map_err(|e| Fehler::Eingabe(e.to_string()))?;
    let tmp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let r = (|| -> std::io::Result<()> {
        let mut t = sicher::neu_anlegen(&tmp)?;
        t.write_all(&bytes)?;
        t.sync_all()?;
        fs::rename(&tmp, &ziel)
    })();
    if let Err(e) = r {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

pub fn lesen(wurzel: &Path, fund_id: &str) -> Result<EingangsFund, Fehler> {
    let p = datei(wurzel, fund_id)?;
    let b = crate::speicher::sicher_lesen(&p, MAX_EINTRAG).map_err(|e| match e {
        Fehler::NichtGefunden(_) => Fehler::NichtGefunden(format!("Fund {fund_id} nicht gefunden")),
        anders => anders,
    })?;
    serde_json::from_slice(&b).map_err(|e| Fehler::Eingabe(format!("Fund {fund_id} unlesbar: {e}")))
}

/// Alle offenen Funde, aelteste zuerst. Unlesbare Eintraege werden uebergangen.
pub fn liste(wurzel: &Path) -> Vec<EingangsFund> {
    let Ok(rd) = fs::read_dir(ordner(wurzel)) else { return Vec::new() };
    let mut v: Vec<EingangsFund> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?;
            lesen(wurzel, id).ok()
        })
        .collect();
    v.sort_by(|a, b| a.entdeckt_ms.cmp(&b.entdeckt_ms).then_with(|| a.fund_id.cmp(&b.fund_id)));
    v
}

pub fn entfernen(wurzel: &Path, fund_id: &str) -> Result<(), Fehler> {
    match fs::remove_file(datei(wurzel, fund_id)?) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    let _ = fs::remove_dir(ordner(wurzel)); // nur, wenn leer
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn fund(id: &str, ms: i64, pirep: &str) -> EingangsFund {
        EingangsFund::aufnahme(id.into(), ms, format!("eingang/{id}.quelle.png"), 100, 2560, 1440, Some(SimFenster { breite: 2560, hoehe: 1440 }), json!({"pirep_id": pirep, "phase": "Cruise", "breite": 50.1}))
    }

    #[test]
    fn ablegen_liste_lesen_entfernen_ueberleben_den_neustart() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("bilder");
        ablegen(&w, &fund("b", 20, "P1")).unwrap();
        ablegen(&w, &fund("a", 10, "P1")).unwrap();
        // „Neustart“: nichts im Speicher, nur die Dateien
        let l = liste(&w);
        assert_eq!(l.iter().map(|f| f.fund_id.as_str()).collect::<Vec<_>>(), ["a", "b"], "aelteste zuerst");
        assert_eq!(lesen(&w, "a").unwrap().meta_basis["phase"], "Cruise");
        assert_eq!(lesen(&w, "a").unwrap().sim_fenster, Some(SimFenster { breite: 2560, hoehe: 1440 }));
        entfernen(&w, "a").unwrap();
        assert_eq!(liste(&w).len(), 1);
        assert!(matches!(lesen(&w, "a"), Err(Fehler::NichtGefunden(_))));
        entfernen(&w, "b").unwrap();
        assert!(!w.join(".eingang").exists(), "leerer Eingang wird entfernt");
        entfernen(&w, "b").unwrap(); // doppelt ist kein Fehler
    }

    #[test]
    fn ohne_flug_entsteht_kein_fund() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("bilder");
        let ohne = fund("x", 1, "  ");
        assert!(matches!(ablegen(&w, &ohne), Err(Fehler::KeinFlug(_))));
        let mut ohne2 = fund("y", 1, "P");
        ohne2.meta_basis = json!({});
        assert!(matches!(ablegen(&w, &ohne2), Err(Fehler::KeinFlug(_))));
        assert!(!w.exists() || liste(&w).is_empty());
    }

    #[test]
    fn die_liste_der_bilder_kennt_den_eingang_nicht_und_kaputtes_wird_uebergangen() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("bilder");
        ablegen(&w, &fund("a", 1, "P")).unwrap();
        fs::write(w.join(".eingang/kaputt.json"), b"{nein").unwrap();
        assert_eq!(liste(&w).len(), 1);
        assert!(crate::speicher::liste(&w).eintraege.is_empty());
        assert!(matches!(lesen(&w, "../x"), Err(Fehler::Name(_))));
    }

    #[cfg(unix)]
    #[test]
    fn ein_linkordner_als_eingang_wird_nicht_beschrieben() {
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("bilder");
        let aussen = t.path().join("aussen");
        fs::create_dir_all(&aussen).unwrap();
        fs::create_dir_all(&w).unwrap();
        std::os::unix::fs::symlink(&aussen, w.join(".eingang")).unwrap();
        assert!(matches!(ablegen(&w, &fund("a", 1, "P")), Err(Fehler::Pfad(_))));
        assert!(fs::read_dir(&aussen).unwrap().next().is_none());
    }
}
