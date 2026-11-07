//! Verwalteter Bilderordner `<app-data>/bilder/<gruppe>/` (P-11.31).
//!
//! * Lesen: nur ueber den relativen Pfad aus [`liste`], streng zerlegt und
//!   danach noch einmal gegen die aufgeloeste Wurzel geprueft (Symlinks).
//! * Schreiben: jeder Teil zuerst vollstaendig in eine Temp-Datei im selben
//!   Ordner, dann umbenennen; das Sidecar kommt zuletzt, damit ein Bild erst
//!   mit seinem JPEG „da“ ist.
//! * Loeschen: JPEG zuerst in den Papierkorb (schlaegt das fehl, bleibt alles
//!   stehen), dann Vorschau und zuletzt das Sidecar endgueltig. Nie nur ein Teil.
//! * Die `id` ist ueber alle Gruppen eindeutig; die Gruppe (Flug oder Datum)
//!   wird beim ersten Schreiben gewaehlt, spaetere Zuordnungen stehen im Sidecar.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;

use crate::namen::{name_pruefen, rel_pfad, rel_pfad_lesen, teil_aus_dateiname, QuellFormat, Teil};
use crate::Fehler;

/// Obergrenze je JPEG/Quelle. Der Server nimmt ≤ 4 MB; lokal liegen volle
/// Spielaufnahmen (PNG 8 MB und mehr) und Vorschauen.
pub const MAX_DATEI: u64 = 64 * 1024 * 1024;
const MAX_SIDECAR: usize = 256 * 1024;
const TEMP_ALTER: Duration = Duration::from_secs(3600);

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Eintrag {
    pub id: String,
    pub gruppe: String,
    pub pfad: Option<String>,
    pub vorschau_pfad: Option<String>,
    pub quelle_pfad: Option<String>,
    pub groesse: u64,
    pub vorschau_groesse: u64,
    pub quelle_groesse: u64,
    /// Summe aller Teile auf der Platte (JPEG, Vorschau, Sidecar, Quelle).
    pub belegt_bytes: u64,
    /// Dateizeit (ms seit 1970, UTC) des JPEG, sonst der Quelle, sonst des Sidecars.
    pub dateizeit_ms: Option<i64>,
    /// Sidecar-JSON, wie gespeichert.
    pub sidecar: Option<Value>,
    pub sidecar_fehler: Option<String>,
    /// JPEG und lesbares Sidecar sind da.
    pub vollstaendig: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Liste {
    pub eintraege: Vec<Eintrag>,
    pub belegt_bytes: u64,
}

pub fn ms_seit_1970(t: SystemTime) -> Option<i64> {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).ok(),
        Err(e) => i64::try_from(e.duration().as_millis()).ok().map(|m| -m),
    }
}

pub fn ist_jpeg(b: &[u8]) -> bool {
    b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 && b[2] == 0xFF
}

struct Gefunden {
    teil: Teil,
    pfad: PathBuf,
    groesse: u64,
    zeit: Option<i64>,
}

/// Alle Gruppenordner (gueltiger Name, echtes Verzeichnis, kein Symlink).
fn gruppen(wurzel: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = fs::read_dir(wurzel) else { return Vec::new() };
    let mut v: Vec<(String, PathBuf)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !crate::namen::name_gueltig(&name) {
                return None;
            }
            let ft = e.file_type().ok()?;
            (ft.is_dir() && !ft.is_symlink()).then(|| (name, e.path()))
        })
        .collect();
    v.sort();
    v
}

/// Teile je Bild-id in einem Gruppenordner.
fn teile_im_ordner(dir: &Path) -> BTreeMap<String, Vec<Gefunden>> {
    let mut m: BTreeMap<String, Vec<Gefunden>> = BTreeMap::new();
    let Ok(rd) = fs::read_dir(dir) else { return m };
    for e in rd.flatten() {
        let Ok(name) = e.file_name().into_string() else { continue };
        let Some((id, teil)) = teil_aus_dateiname(&name) else { continue };
        let Ok(md) = e.metadata() else { continue };
        if !md.is_file() {
            continue;
        }
        m.entry(id).or_default().push(Gefunden {
            teil,
            pfad: e.path(),
            groesse: md.len(),
            zeit: md.modified().ok().and_then(ms_seit_1970),
        });
    }
    m
}

fn eintrag_bauen(gruppe: &str, id: &str, teile: &[Gefunden]) -> Eintrag {
    let finde = |gesucht: fn(&Teil) -> bool| teile.iter().find(|t| gesucht(&t.teil));
    let jpeg = finde(|t| matches!(t, Teil::Jpeg));
    let vorschau = finde(|t| matches!(t, Teil::Vorschau));
    let sidecar = finde(|t| matches!(t, Teil::Sidecar));
    let quelle = finde(|t| matches!(t, Teil::Quelle(_)));
    let rel = |g: Option<&Gefunden>| {
        g.and_then(|g| g.pfad.file_name()).and_then(|n| n.to_str()).map(|n| rel_pfad(gruppe, n))
    };
    let (sc_wert, sc_fehler) = match sidecar {
        None => (None, None),
        Some(s) if s.groesse as usize > MAX_SIDECAR => (None, Some("Sidecar zu gross".to_string())),
        Some(s) => match fs::read(&s.pfad) {
            Ok(b) => match serde_json::from_slice::<Value>(&b) {
                Ok(v) => (Some(v), None),
                Err(e) => (None, Some(format!("Sidecar nicht lesbar: {e}"))),
            },
            Err(e) => (None, Some(format!("Sidecar nicht lesbar: {e}"))),
        },
    };
    Eintrag {
        id: id.to_string(),
        gruppe: gruppe.to_string(),
        pfad: rel(jpeg),
        vorschau_pfad: rel(vorschau),
        quelle_pfad: rel(quelle),
        groesse: jpeg.map_or(0, |g| g.groesse),
        vorschau_groesse: vorschau.map_or(0, |g| g.groesse),
        quelle_groesse: quelle.map_or(0, |g| g.groesse),
        belegt_bytes: teile.iter().map(|t| t.groesse).sum(),
        dateizeit_ms: jpeg.or(quelle).or(sidecar).and_then(|g| g.zeit),
        vollstaendig: jpeg.is_some() && sc_wert.is_some(),
        sidecar: sc_wert,
        sidecar_fehler: sc_fehler,
    }
}

/// Alle Eintraege des verwalteten Ordners, ohne Filter.
pub fn liste(wurzel: &Path) -> Liste {
    let mut eintraege = Vec::new();
    for (gruppe, dir) in gruppen(wurzel) {
        for (id, teile) in teile_im_ordner(&dir) {
            eintraege.push(eintrag_bauen(&gruppe, &id, &teile));
        }
    }
    let belegt_bytes = eintraege.iter().map(|e| e.belegt_bytes).sum();
    Liste { eintraege, belegt_bytes }
}

pub fn eintrag(wurzel: &Path, gruppe: &str, id: &str) -> Result<Eintrag, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    name_pruefen("Bild-id", id)?;
    let teile = teile_im_ordner(&wurzel.join(gruppe)).remove(id);
    match teile {
        Some(t) if !t.is_empty() => Ok(eintrag_bauen(gruppe, id, &t)),
        _ => Err(Fehler::NichtGefunden(format!("Bild {gruppe}/{id} nicht gefunden"))),
    }
}

/// Bytes eines Bildes, seiner Vorschau oder der Eingangskopie. Das Sidecar
/// liefert [`liste`]. Pfad-Ausbruch ist doppelt ausgeschlossen: die Zerlegung
/// in [`rel_pfad_lesen`] und der Vergleich der aufgeloesten Pfade.
pub fn lesen(wurzel: &Path, rel: &str) -> Result<Vec<u8>, Fehler> {
    let (gruppe, id, teil) = rel_pfad_lesen(rel)?;
    if teil == Teil::Sidecar {
        return Err(Fehler::Pfad("Das Sidecar steht in der Liste, nicht als Datei".into()));
    }
    let ziel = wurzel.join(&gruppe).join(teil.dateiname(&id));
    let basis = fs::canonicalize(wurzel).map_err(|_| Fehler::NichtGefunden("Bilderordner fehlt".into()))?;
    let aufgeloest = fs::canonicalize(&ziel).map_err(|_| Fehler::NichtGefunden(format!("{rel} nicht gefunden")))?;
    if !aufgeloest.starts_with(&basis) {
        return Err(Fehler::Pfad(format!("Pfad ausserhalb des Bilderordners: {rel}")));
    }
    let md = fs::metadata(&aufgeloest)?;
    if !md.is_file() {
        return Err(Fehler::NichtGefunden(format!("{rel} ist keine Datei")));
    }
    if md.len() > MAX_DATEI {
        return Err(Fehler::Gross(format!("{rel} ist zu gross")));
    }
    Ok(fs::read(aufgeloest)?)
}

#[derive(Debug, Clone, Default)]
pub struct SchreibAnfrage {
    pub gruppe: String,
    pub id: String,
    pub jpeg: Option<Vec<u8>>,
    pub vorschau: Option<Vec<u8>>,
    pub sidecar: Option<Value>,
    /// Eingangskopie dieser id (aus `bild_uebernehmen`/`bild_aufnehmen`) nach dem Schreiben entfernen.
    pub quelle_entfernen: bool,
}

fn temp_name(id: &str, teil: &str) -> String {
    format!(".{id}.{teil}.{}.tmp", uuid::Uuid::new_v4().simple())
}

/// Temp-Dateien, die ein abgebrochener Lauf liegen liess.
fn alte_temp_dateien_raeumen(dir: &Path) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if !(name.starts_with('.') && name.ends_with(".tmp")) {
            continue;
        }
        let alt = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| SystemTime::now().duration_since(t).ok())
            .is_some_and(|d| d > TEMP_ALTER);
        if alt {
            let _ = fs::remove_file(e.path());
        }
    }
}

fn temp_schreiben(pfad: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = fs::File::create(pfad)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Alle Teile mit dieser id in allen Gruppen.
fn teile_von(wurzel: &Path, id: &str) -> Vec<(String, Gefunden)> {
    let mut v = Vec::new();
    for (gruppe, dir) in gruppen(wurzel) {
        if let Some(teile) = teile_im_ordner(&dir).remove(id) {
            v.extend(teile.into_iter().map(|t| (gruppe.clone(), t)));
        }
    }
    v
}

pub fn schreiben(wurzel: &Path, a: SchreibAnfrage) -> Result<Eintrag, Fehler> {
    name_pruefen("Gruppe", &a.gruppe)?;
    name_pruefen("Bild-id", &a.id)?;
    for (was, b) in [("JPEG", &a.jpeg), ("Vorschau", &a.vorschau)] {
        if let Some(b) = b {
            if !ist_jpeg(b) {
                return Err(Fehler::Format(format!("{was} ist kein JPEG")));
            }
            if b.len() as u64 > MAX_DATEI {
                return Err(Fehler::Gross(format!("{was} ist zu gross")));
            }
        }
    }
    let sidecar_bytes = match &a.sidecar {
        None => None,
        Some(v @ Value::Object(_)) => {
            let b = serde_json::to_vec_pretty(v).map_err(|e| Fehler::Eingabe(e.to_string()))?;
            if b.len() > MAX_SIDECAR {
                return Err(Fehler::Gross("Sidecar ist zu gross".into()));
            }
            Some(b)
        }
        Some(_) => return Err(Fehler::Eingabe("Sidecar muss ein JSON-Objekt sein".into())),
    };

    let dir = wurzel.join(&a.gruppe);
    let hat_jpeg = dir.join(Teil::Jpeg.dateiname(&a.id)).exists();
    let hat_sidecar = dir.join(Teil::Sidecar.dateiname(&a.id)).exists();
    let neu = !hat_jpeg && !hat_sidecar;
    if neu {
        if a.jpeg.is_none() || sidecar_bytes.is_none() {
            return Err(Fehler::Eingabe("Ein neues Bild braucht JPEG und Sidecar".into()));
        }
        let belegt = teile_von(wurzel, &a.id)
            .into_iter()
            .any(|(g, t)| g != a.gruppe && matches!(t.teil, Teil::Jpeg | Teil::Sidecar | Teil::Vorschau));
        if belegt {
            return Err(Fehler::Name(format!("Bild-id {} ist in einer anderen Gruppe schon vergeben", a.id)));
        }
    }
    fs::create_dir_all(&dir)?;
    alte_temp_dateien_raeumen(&dir);

    // Reihenfolge ist die Festlegung: JPEG, Vorschau, Sidecar zuletzt.
    let mut vorhaben: Vec<(Teil, &str, &[u8])> = Vec::new();
    if let Some(b) = &a.jpeg {
        vorhaben.push((Teil::Jpeg, "jpg", b));
    }
    if let Some(b) = &a.vorschau {
        vorhaben.push((Teil::Vorschau, "vorschau", b));
    }
    if let Some(b) = &sidecar_bytes {
        vorhaben.push((Teil::Sidecar, "json", b));
    }

    let mut temps: Vec<(Teil, PathBuf)> = Vec::new();
    for (teil, marke, bytes) in &vorhaben {
        let t = dir.join(temp_name(&a.id, marke));
        if let Err(e) = temp_schreiben(&t, bytes) {
            let _ = fs::remove_file(&t);
            temps.iter().for_each(|(_, p)| {
                let _ = fs::remove_file(p);
            });
            return Err(e.into());
        }
        temps.push((*teil, t));
    }
    let mut fertig: Vec<PathBuf> = Vec::new();
    for (i, (teil, t)) in temps.iter().enumerate() {
        let ziel = dir.join(teil.dateiname(&a.id));
        if let Err(e) = fs::rename(t, &ziel) {
            temps[i..].iter().for_each(|(_, p)| {
                let _ = fs::remove_file(p);
            });
            if neu {
                fertig.iter().for_each(|p| {
                    let _ = fs::remove_file(p);
                });
            }
            return Err(e.into());
        }
        fertig.push(ziel);
    }
    if a.quelle_entfernen {
        for (_, g) in teile_von(wurzel, &a.id) {
            if matches!(g.teil, Teil::Quelle(_)) {
                let _ = fs::remove_file(&g.pfad);
            }
        }
    }
    eintrag(wurzel, &a.gruppe, &a.id)
}

/// Verschiebt eine Datei in den Papierkorb des Systems (oder, in Tests, anderswohin).
pub type Papierkorb<'a> = &'a dyn Fn(&Path) -> std::io::Result<()>;

/// Entfernt ein Bild mit allen Teilen. Idempotent fuer Teile, die schon fehlen;
/// `NichtGefunden`, wenn es in der Gruppe gar nichts zu loeschen gibt.
pub fn loeschen(wurzel: &Path, gruppe: &str, id: &str, papierkorb: Papierkorb<'_>) -> Result<Vec<String>, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    name_pruefen("Bild-id", id)?;
    let teile = teile_von(wurzel, id);
    if !teile.iter().any(|(g, _)| g == gruppe) {
        return Err(Fehler::NichtGefunden(format!("Bild {gruppe}/{id} nicht gefunden")));
    }
    let rel = |g: &str, t: &Gefunden| {
        t.pfad.file_name().and_then(|n| n.to_str()).map(|n| rel_pfad(g, n)).unwrap_or_default()
    };
    let mut entfernt = Vec::new();
    // 1. JPEG: rueckholbar. Scheitert das, ist noch nichts angefasst.
    for (g, t) in teile.iter().filter(|(_, t)| t.teil == Teil::Jpeg) {
        papierkorb(&t.pfad).map_err(|e| Fehler::Io(format!("Papierkorb: {e}")))?;
        entfernt.push(rel(g, t));
    }
    // 2. Eingangskopie des Originals (ebenfalls Nutzerdaten): Papierkorb.
    for (g, t) in teile.iter().filter(|(_, t)| matches!(t.teil, Teil::Quelle(_))) {
        papierkorb(&t.pfad).map_err(|e| Fehler::Io(format!("Papierkorb: {e}")))?;
        entfernt.push(rel(g, t));
    }
    // 3. Vorschau, 4. Sidecar zuletzt: bleibt ein Rest, ist er noch sichtbar und loeschbar.
    for teil in [Teil::Vorschau, Teil::Sidecar] {
        for (g, t) in teile.iter().filter(|(_, t)| t.teil == teil) {
            fs::remove_file(&t.pfad)?;
            entfernt.push(rel(g, t));
        }
    }
    let _ = fs::remove_dir(wurzel.join(gruppe)); // nur, wenn leer
    Ok(entfernt)
}

/// Legt eine Eingangskopie ab (`<gruppe>/<id>.quelle.<ext>`) aus Bytes; fuer die Fensteraufnahme.
pub fn quelle_schreiben(wurzel: &Path, gruppe: &str, id: &str, format: QuellFormat, bytes: &[u8]) -> Result<Eintrag, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    name_pruefen("Bild-id", id)?;
    let dir = wurzel.join(gruppe);
    fs::create_dir_all(&dir)?;
    alte_temp_dateien_raeumen(&dir);
    let t = dir.join(temp_name(id, "quelle"));
    if let Err(e) = temp_schreiben(&t, bytes) {
        let _ = fs::remove_file(&t);
        return Err(e.into());
    }
    if let Err(e) = fs::rename(&t, dir.join(Teil::Quelle(format).dateiname(id))) {
        let _ = fs::remove_file(&t);
        return Err(e.into());
    }
    eintrag(wurzel, gruppe, id)
}

/// Schreibt fertige ZIP-Bytes atomar an den vom Piloten gewaehlten Ort
/// (`bild_export`). Prueft die ZIP-Kennung, erzwingt die Endung `.zip`.
pub fn export_schreiben(ziel: &Path, bytes: &[u8]) -> Result<PathBuf, Fehler> {
    if bytes.len() < 22 || &bytes[..2] != b"PK" {
        return Err(Fehler::Format("Das ist keine ZIP-Datei".into()));
    }
    let mut ziel = ziel.to_path_buf();
    if ziel.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() != Some("zip") {
        ziel.set_extension("zip");
    }
    let tmp = ziel.with_extension("zip.tmp");
    let r = temp_schreiben(&tmp, bytes).and_then(|_| fs::rename(&tmp, &ziel));
    if let Err(e) = r {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(ziel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    pub(crate) fn jpeg(n: u8) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, n];
        v.extend_from_slice(&[0xFF, 0xD9]);
        v
    }

    fn anfrage(gruppe: &str, id: &str) -> SchreibAnfrage {
        SchreibAnfrage {
            gruppe: gruppe.into(),
            id: id.into(),
            jpeg: Some(jpeg(1)),
            vorschau: Some(jpeg(2)),
            sidecar: Some(json!({"bild_id": id, "text": "Hallo"})),
            quelle_entfernen: false,
        }
    }

    fn dateien(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        v.sort();
        v
    }

    /// Papierkorb fuer Tests: verschiebt in einen Nachbarordner, protokolliert.
    struct TestKorb {
        ziel: PathBuf,
        aufrufe: RefCell<Vec<PathBuf>>,
        scheitert: bool,
    }
    impl TestKorb {
        fn neu(ziel: &Path, scheitert: bool) -> Self {
            fs::create_dir_all(ziel).unwrap();
            Self { ziel: ziel.into(), aufrufe: RefCell::new(vec![]), scheitert }
        }
        fn tue(&self, p: &Path) -> std::io::Result<()> {
            if self.scheitert {
                return Err(std::io::Error::other("kein Papierkorb"));
            }
            self.aufrufe.borrow_mut().push(p.into());
            fs::rename(p, self.ziel.join(p.file_name().unwrap()))
        }
    }

    #[test]
    fn schreiben_legt_drei_teile_an_und_liste_zeigt_sie() {
        let t = tempfile::tempdir().unwrap();
        let e = schreiben(t.path(), anfrage("PIREP1", "b1")).unwrap();
        assert_eq!(e.pfad.as_deref(), Some("PIREP1/b1.jpg"));
        assert_eq!(e.vorschau_pfad.as_deref(), Some("PIREP1/b1.vorschau.jpg"));
        assert!(e.vollstaendig);
        assert_eq!(e.sidecar.as_ref().unwrap()["text"], "Hallo");
        assert_eq!(dateien(&t.path().join("PIREP1")), ["b1.jpg", "b1.json", "b1.vorschau.jpg"], "keine Temp-Reste");
        let l = liste(t.path());
        assert_eq!(l.eintraege.len(), 1);
        assert_eq!(l.belegt_bytes, l.eintraege[0].belegt_bytes);
        assert!(l.belegt_bytes > 0);
        assert_eq!(lesen(t.path(), "PIREP1/b1.jpg").unwrap(), jpeg(1));
        assert_eq!(lesen(t.path(), "PIREP1/b1.vorschau.jpg").unwrap(), jpeg(2));
    }

    #[test]
    fn lesen_laesst_keinen_pfad_ausbruch_zu() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        schreiben(&wurzel, anfrage("g", "b1")).unwrap();
        // Geheimnis neben dem Bilderordner
        fs::write(t.path().join("geheim.jpg"), jpeg(9)).unwrap();
        for rel in ["../geheim.jpg", "g/../../geheim.jpg", "/etc/passwd", "g/b1.jpg/../../../geheim.jpg", "g\\..\\geheim.jpg", "g/b1.json"] {
            assert!(matches!(lesen(&wurzel, rel), Err(Fehler::Pfad(_))), "{rel} muss als Pfadfehler scheitern");
        }
        assert!(matches!(lesen(&wurzel, "g/nix.jpg"), Err(Fehler::NichtGefunden(_))));
    }

    #[cfg(unix)]
    #[test]
    fn lesen_folgt_keinem_symlink_aus_dem_ordner() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        fs::create_dir_all(wurzel.join("g")).unwrap();
        fs::write(t.path().join("geheim.jpg"), jpeg(9)).unwrap();
        std::os::unix::fs::symlink(t.path().join("geheim.jpg"), wurzel.join("g").join("x1.jpg")).unwrap();
        assert!(matches!(lesen(&wurzel, "g/x1.jpg"), Err(Fehler::Pfad(_))));
    }

    #[test]
    fn schreiben_prueft_namen_und_inhalt() {
        let t = tempfile::tempdir().unwrap();
        for (g, i) in [("..", "b"), ("g", ".."), ("g/h", "b"), ("g", "a/b"), ("", "b"), ("g", "b.jpg"), ("C:", "b")] {
            assert!(matches!(schreiben(t.path(), anfrage(g, i)), Err(Fehler::Name(_))), "{g:?}/{i:?}");
        }
        let mut a = anfrage("g", "b1");
        a.jpeg = Some(b"PNG!".to_vec());
        assert!(matches!(schreiben(t.path(), a), Err(Fehler::Format(_))));
        let mut a = anfrage("g", "b1");
        a.sidecar = Some(json!([1, 2]));
        assert!(matches!(schreiben(t.path(), a), Err(Fehler::Eingabe(_))));
        let mut a = anfrage("g", "b1");
        a.jpeg = None;
        assert!(matches!(schreiben(t.path(), a), Err(Fehler::Eingabe(_))), "neues Bild ohne JPEG");
        assert!(liste(t.path()).eintraege.is_empty(), "nach Fehlern nichts angelegt");
        assert!(!t.path().join("g").exists() || dateien(&t.path().join("g")).is_empty());
    }

    #[test]
    fn schreiben_nur_sidecar_aendert_den_rest_nicht() {
        let t = tempfile::tempdir().unwrap();
        schreiben(t.path(), anfrage("g", "b1")).unwrap();
        let e = schreiben(
            t.path(),
            SchreibAnfrage { gruppe: "g".into(), id: "b1".into(), sidecar: Some(json!({"bild_id": "b1", "favorit": true})), ..Default::default() },
        )
        .unwrap();
        assert_eq!(e.sidecar.unwrap()["favorit"], true);
        assert_eq!(lesen(t.path(), "g/b1.jpg").unwrap(), jpeg(1));
        assert_eq!(dateien(&t.path().join("g")).len(), 3);
    }

    #[test]
    fn bild_id_ist_ueber_gruppen_eindeutig() {
        let t = tempfile::tempdir().unwrap();
        schreiben(t.path(), anfrage("g1", "b1")).unwrap();
        assert!(matches!(schreiben(t.path(), anfrage("g2", "b1")), Err(Fehler::Name(_))));
        assert!(schreiben(t.path(), anfrage("g1", "b1")).is_ok(), "gleiche Gruppe = Ersetzen");
    }

    #[test]
    fn quelle_wird_beim_schreiben_entfernt_wenn_verlangt() {
        let t = tempfile::tempdir().unwrap();
        quelle_schreiben(t.path(), "eingang", "b1", QuellFormat::Png, b"png").unwrap();
        let mut a = anfrage("PIREP1", "b1");
        a.quelle_entfernen = false;
        schreiben(t.path(), a).unwrap();
        assert!(t.path().join("eingang/b1.quelle.png").exists(), "ohne Auftrag bleibt die Quelle");
        let mut a = anfrage("PIREP1", "b1");
        a.quelle_entfernen = true;
        schreiben(t.path(), a).unwrap();
        assert!(!t.path().join("eingang/b1.quelle.png").exists());
    }

    #[test]
    fn loeschen_entfernt_alle_drei_teile_und_schont_andere() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        schreiben(&wurzel, anfrage("g", "b1")).unwrap();
        schreiben(&wurzel, anfrage("g", "b2")).unwrap();
        let korb = TestKorb::neu(&t.path().join("korb"), false);
        let weg = loeschen(&wurzel, "g", "b1", &|p| korb.tue(p)).unwrap();
        assert_eq!(weg.len(), 3);
        assert_eq!(dateien(&wurzel.join("g")), ["b2.jpg", "b2.json", "b2.vorschau.jpg"]);
        // nur das JPEG ging in den Papierkorb, Sidecar und Vorschau endgueltig
        assert_eq!(dateien(&t.path().join("korb")), ["b1.jpg"]);
        assert_eq!(korb.aufrufe.borrow().len(), 1);
        assert!(matches!(loeschen(&wurzel, "g", "b1", &|p| korb.tue(p)), Err(Fehler::NichtGefunden(_))));
    }

    #[test]
    fn loeschen_fasst_nichts_an_wenn_der_papierkorb_scheitert() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        schreiben(&wurzel, anfrage("g", "b1")).unwrap();
        let korb = TestKorb::neu(&t.path().join("korb"), true);
        assert!(matches!(loeschen(&wurzel, "g", "b1", &|p| korb.tue(p)), Err(Fehler::Io(_))));
        assert_eq!(dateien(&wurzel.join("g")), ["b1.jpg", "b1.json", "b1.vorschau.jpg"], "alle drei Teile noch da");
    }

    #[test]
    fn loeschen_raeumt_auch_einen_halben_rest_und_die_eingangskopie() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        schreiben(&wurzel, anfrage("g", "b1")).unwrap();
        quelle_schreiben(&wurzel, "eingang", "b1", QuellFormat::Png, b"png").unwrap();
        fs::remove_file(wurzel.join("g/b1.jpg")).unwrap(); // Rest eines frueheren Abbruchs
        let korb = TestKorb::neu(&t.path().join("korb"), false);
        loeschen(&wurzel, "g", "b1", &|p| korb.tue(p)).unwrap();
        assert!(liste(&wurzel).eintraege.is_empty());
        assert_eq!(dateien(&t.path().join("korb")), ["b1.quelle.png"]);
    }

    #[test]
    fn liste_meldet_unvollstaendige_und_kaputte_sidecars() {
        let t = tempfile::tempdir().unwrap();
        schreiben(t.path(), anfrage("g", "b1")).unwrap();
        fs::write(t.path().join("g/b1.json"), b"{kaputt").unwrap();
        let e = &liste(t.path()).eintraege[0];
        assert!(!e.vollstaendig);
        assert!(e.sidecar.is_none() && e.sidecar_fehler.is_some());
        fs::remove_file(t.path().join("g/b1.jpg")).unwrap();
        assert!(e_pfad_fehlt(t.path()));
    }
    fn e_pfad_fehlt(w: &Path) -> bool {
        liste(w).eintraege[0].pfad.is_none()
    }

    #[test]
    fn export_nur_zip_atomar_mit_endung() {
        let t = tempfile::tempdir().unwrap();
        let mut zip = b"PK\x03\x04".to_vec();
        zip.extend_from_slice(&[0; 30]);
        let p = export_schreiben(&t.path().join("bilder"), &zip).unwrap();
        assert_eq!(p.file_name().unwrap(), "bilder.zip");
        assert_eq!(fs::read(&p).unwrap(), zip);
        assert_eq!(dateien(t.path()), ["bilder.zip"], "kein Temp-Rest");
        assert!(matches!(export_schreiben(&t.path().join("x.zip"), b"kein zip hier, nur Text drin!"), Err(Fehler::Format(_))));
        assert!(!t.path().join("x.zip").exists());
        assert!(export_schreiben(&t.path().join("nix").join("x.zip"), &zip).is_err());
    }

    #[test]
    fn alte_temp_dateien_werden_beim_schreiben_geraeumt_frische_nicht() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("g");
        fs::create_dir_all(&dir).unwrap();
        let alt = dir.join(".x.jpg.1.tmp");
        let frisch = dir.join(".y.jpg.2.tmp");
        fs::write(&alt, b"a").unwrap();
        fs::write(&frisch, b"b").unwrap();
        let f = fs::File::options().write(true).open(&alt).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        drop(f);
        schreiben(t.path(), anfrage("g", "b1")).unwrap();
        assert!(!alt.exists());
        assert!(frisch.exists());
    }
}
