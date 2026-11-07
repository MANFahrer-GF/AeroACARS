//! Verwalteter Bilderordner `<app-data>/bilder/<gruppe>/` (P-11.31).
//!
//! * Lesen: nur ueber den relativen Pfad aus [`liste`], streng zerlegt; jeder
//!   Pfadteil ab der Wurzel darf kein Link sein, geoeffnet wird ohne Linkfolgen
//!   und gelesen vom Handle (siehe [`crate::sicher`]).
//! * Wurzel und Gruppenordner muessen echte Ordner sein, sonst schreibt nichts.
//! * Schreiben: jeder Teil zuerst vollstaendig in eine Temp-Datei im selben
//!   Ordner, dann umbenennen; das Sidecar kommt zuletzt, damit ein Bild erst
//!   mit seinem JPEG „da“ ist.
//! * Loeschen als Vorgang mit Rueckweg: Vorschau und Sidecar wandern zuerst
//!   atomar (Umbenennen) nach `<wurzel>/.loeschen/<id>/<gruppe>/`; scheitert das,
//!   kommt alles zurueck. Dann Eingangskopie und JPEG in den Papierkorb (aus
//!   ihrem Ort, damit „Zuruecklegen“ im Papierkorb dorthin zurueckfuehrt);
//!   scheitert er, kommt der Rest zurueck. Erst danach geht der Rest endgueltig.
//!   Eine Marke `FERTIG` im Stagingordner trennt „zurueckrollen“ (davor) von
//!   „abschliessen“ (danach); [`loeschreste_abschliessen`] erledigt das beim Start.
//!   `.loeschen` beginnt mit einem Punkt, ist also nie eine Gruppe und fehlt in
//!   [`liste`].
//! * Die `id` ist ueber alle Gruppen eindeutig; die Gruppe (Flug oder Datum)
//!   wird beim ersten Schreiben gewaehlt, spaetere Zuordnungen stehen im Sidecar.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;

use crate::namen::{name_pruefen, rel_pfad, rel_pfad_lesen, teil_aus_dateiname, QuellFormat, Teil};
use crate::sicher;
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

/// Liest eine Datei des verwalteten Ordners vom geoeffneten Handle, ohne Linkfolgen.
fn sicher_lesen(pfad: &Path, max: u64) -> Result<Vec<u8>, Fehler> {
    let (Some(dir), Some(name)) = (pfad.parent(), pfad.file_name()) else {
        return Err(Fehler::Pfad("Pfad ungueltig".into()));
    };
    let (f, md) = sicher::datei_oeffnen(dir, Path::new(name))?;
    if md.len() > max {
        return Err(Fehler::Gross(format!("{} ist zu gross", pfad.display())));
    }
    let mut b = Vec::with_capacity(md.len() as usize);
    f.take(max + 1).read_to_end(&mut b)?;
    if b.len() as u64 > max {
        return Err(Fehler::Gross(format!("{} ist zu gross", pfad.display())));
    }
    Ok(b)
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
        Some(s) => match sicher_lesen(&s.pfad, MAX_SIDECAR as u64) {
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
/// liefert [`liste`]. Pfad-Ausbruch ist ausgeschlossen: die Zerlegung in
/// [`rel_pfad_lesen`], kein Link auf dem Weg ab der Wurzel und Lesen vom
/// geoeffneten Handle statt eines zweiten Oeffnens per Pfad.
pub fn lesen(wurzel: &Path, rel: &str) -> Result<Vec<u8>, Fehler> {
    let (gruppe, id, teil) = rel_pfad_lesen(rel)?;
    if teil == Teil::Sidecar {
        return Err(Fehler::Pfad("Das Sidecar steht in der Liste, nicht als Datei".into()));
    }
    match fs::symlink_metadata(wurzel) {
        Err(_) => return Err(Fehler::NichtGefunden("Bilderordner fehlt".into())),
        Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
            return Err(Fehler::Pfad("Der Bilderordner ist kein echter Ordner".into()));
        }
        Ok(_) => {}
    }
    let (f, md) = sicher::datei_oeffnen(wurzel, &Path::new(&gruppe).join(teil.dateiname(&id)))
        .map_err(|e| match e {
            Fehler::NichtGefunden(_) => Fehler::NichtGefunden(format!("{rel} nicht gefunden")),
            Fehler::Pfad(_) => Fehler::Pfad(format!("Pfad ausserhalb des Bilderordners oder Link: {rel}")),
            anders => anders,
        })?;
    if md.len() > MAX_DATEI {
        return Err(Fehler::Gross(format!("{rel} ist zu gross")));
    }
    let mut b = Vec::with_capacity(md.len() as usize);
    f.take(MAX_DATEI + 1).read_to_end(&mut b)?;
    if b.len() as u64 > MAX_DATEI {
        return Err(Fehler::Gross(format!("{rel} ist zu gross")));
    }
    Ok(b)
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
    let mut f = sicher::neu_anlegen(pfad)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Wurzel und Gruppenordner muessen echte Ordner sein, kein Link: sonst
/// landeten Temp- und Zieldateien ausserhalb des verwalteten Ordners.
pub(crate) fn ordner_vorbereiten(wurzel: &Path, dir: &Path) -> Result<(), Fehler> {
    // Vor dem Anlegen: ein vorhandener Link darf nicht „durchgeschrieben“ werden.
    for (p, was) in [(wurzel, "Der Bilderordner"), (dir, "Der Gruppenordner")] {
        if fs::symlink_metadata(p).is_ok() {
            sicher::echten_ordner_sichern(p, was)?;
        }
    }
    fs::create_dir_all(dir)?;
    sicher::echten_ordner_sichern(wurzel, "Der Bilderordner")?;
    sicher::echten_ordner_sichern(dir, "Der Gruppenordner")
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
    ordner_vorbereiten(wurzel, &dir)?;
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

const STAGING: &str = ".loeschen";
/// Liegt diese Datei im Stagingordner, sind Eingangskopie und JPEG schon im
/// Papierkorb; es fehlt nur das endgueltige Entfernen von Vorschau und Sidecar.
const MARKE: &str = "FERTIG";

fn staging_wurzel(wurzel: &Path) -> PathBuf {
    wurzel.join(STAGING)
}

/// Bringt gestellte Teile an ihren Platz zurueck (`<stage>/<gruppe>/<datei>` ->
/// `<wurzel>/<gruppe>/<datei>`) und entfernt den Stagingordner. Ein Ziel, das
/// inzwischen wieder existiert, gewinnt (die gestellte Datei ist dann die aeltere).
/// Scheitert ein Zurueckbewegen, bleibt der Stagingordner mit dem Rest stehen.
fn zurueckrollen(wurzel: &Path, stage: &Path) -> Result<(), Fehler> {
    let mut fehler: Option<String> = None;
    if let Ok(gruppen) = fs::read_dir(stage) {
        for g in gruppen.flatten() {
            let gname = g.file_name();
            let Some(gn) = gname.to_str() else { continue };
            if !crate::namen::name_gueltig(gn) || !g.path().is_dir() {
                continue;
            }
            let Ok(dateien) = fs::read_dir(g.path()) else { continue };
            let ziel_dir = wurzel.join(gn);
            if let Err(e) = fs::create_dir_all(&ziel_dir) {
                fehler = Some(e.to_string());
                continue;
            }
            for d in dateien.flatten() {
                let ziel = ziel_dir.join(d.file_name());
                let r = if ziel.exists() { fs::remove_file(d.path()) } else { fs::rename(d.path(), &ziel) };
                if let Err(e) = r {
                    fehler = Some(e.to_string());
                }
            }
        }
    }
    if let Some(e) = fehler {
        return Err(Fehler::Io(format!("Loeschen nicht rueckgaengig gemacht, Reste in {STAGING}: {e}")));
    }
    fs::remove_dir_all(stage)?;
    if let Some(eltern) = stage.parent() {
        let _ = fs::remove_dir(eltern); // `.loeschen`, nur wenn leer
    }
    Ok(())
}

/// Ein liegengebliebener Vorgang: mit Marke zu Ende bringen, ohne Marke zurueckrollen.
fn stage_erledigen(wurzel: &Path, stage: &Path) -> Result<(), Fehler> {
    if stage.join(MARKE).exists() {
        fs::remove_dir_all(stage)?;
        if let Some(eltern) = stage.parent() {
            let _ = fs::remove_dir(eltern);
        }
        Ok(())
    } else {
        zurueckrollen(wurzel, stage)
    }
}

/// Beim Start: Reste von `.loeschen` abschliessen oder zurueckrollen. Sicher,
/// weil nur zwei Zustaende vorkommen: ohne Marke ist noch nichts endgueltig
/// weg (Zurueckrollen stellt Vorschau und Sidecar wieder her); mit Marke liegen
/// JPEG und Eingangskopie schon im Papierkorb, und was bleibt, ist genau das,
/// was der Pilot endgueltig loswerden wollte. Gibt die erledigten ids zurueck.
pub fn loeschreste_abschliessen(wurzel: &Path) -> Vec<String> {
    let st = staging_wurzel(wurzel);
    if !sicher::ist_echter_ordner(wurzel) || !sicher::ist_echter_ordner(&st) {
        return Vec::new();
    }
    let mut erledigt = Vec::new();
    if let Ok(rd) = fs::read_dir(&st) {
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() {
                continue;
            }
            let Some(id) = e.file_name().to_str().map(str::to_string) else { continue };
            if stage_erledigen(wurzel, &p).is_ok() {
                erledigt.push(id);
            }
        }
    }
    let _ = fs::remove_dir(&st); // nur, wenn leer
    erledigt.sort();
    erledigt
}

/// Entfernt ein Bild mit allen Teilen, als Vorgang mit Rueckweg (siehe Kopf der
/// Datei). Idempotent fuer Teile, die schon fehlen; `NichtGefunden`, wenn es in
/// der Gruppe gar nichts zu loeschen gibt. Scheitert etwas, ist nichts verloren:
/// was nicht im Papierkorb liegt, steht wieder an seinem Platz.
pub fn loeschen(wurzel: &Path, gruppe: &str, id: &str, papierkorb: Papierkorb<'_>) -> Result<Vec<String>, Fehler> {
    loeschen_mit(wurzel, gruppe, id, papierkorb, &|von, nach| fs::rename(von, nach))
}

/// Wie [`loeschen`], mit austauschbarem Umbenennen (Tests lassen es scheitern).
fn loeschen_mit(
    wurzel: &Path,
    gruppe: &str,
    id: &str,
    papierkorb: Papierkorb<'_>,
    umbenennen: &dyn Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<Vec<String>, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    name_pruefen("Bild-id", id)?;
    sicher::echten_ordner_sichern(wurzel, "Der Bilderordner").map_err(|e| match e {
        Fehler::Io(_) | Fehler::NichtGefunden(_) => Fehler::NichtGefunden(format!("Bild {gruppe}/{id} nicht gefunden")),
        anders => anders,
    })?;
    let stage = staging_wurzel(wurzel).join(id);
    if fs::symlink_metadata(&stage).is_ok() {
        stage_erledigen(wurzel, &stage)?; // Rest eines frueheren Vorgangs derselben id
    }
    let teile = teile_von(wurzel, id);
    if !teile.iter().any(|(g, _)| g == gruppe) {
        return Err(Fehler::NichtGefunden(format!("Bild {gruppe}/{id} nicht gefunden")));
    }
    let rel = |g: &str, t: &Gefunden| t.pfad.file_name().and_then(|n| n.to_str()).map(|n| rel_pfad(g, n)).unwrap_or_default();
    let mut entfernt: Vec<String> = teile.iter().map(|(g, t)| rel(g, t)).collect();
    entfernt.sort();

    // 1. Vorschau und Sidecar atomar beiseite stellen (Umbenennen im selben Dateisystem).
    sicher::echten_ordner_sichern(&staging_wurzel(wurzel), ".loeschen").or_else(|_| {
        fs::create_dir_all(staging_wurzel(wurzel))?;
        sicher::echten_ordner_sichern(&staging_wurzel(wurzel), ".loeschen")
    })?;
    fs::create_dir_all(&stage)?;
    let mut zu_stellen: Vec<&(String, Gefunden)> = teile.iter().filter(|(_, t)| matches!(t.teil, Teil::Vorschau | Teil::Sidecar)).collect();
    zu_stellen.sort_by_key(|(g, t)| (matches!(t.teil, Teil::Sidecar), g.clone()));
    for (g, t) in zu_stellen {
        let name = t.pfad.file_name().map(|n| n.to_owned()).unwrap_or_default();
        let r = fs::create_dir_all(stage.join(g)).and_then(|_| umbenennen(&t.pfad, &stage.join(g).join(&name)));
        if let Err(e) = r {
            return Err(match zurueckrollen(wurzel, &stage) {
                Ok(()) => Fehler::Io(format!("Loeschen abgebrochen, nichts veraendert: {e}")),
                Err(r) => r,
            });
        }
    }
    // 2. Eingangskopie(n), dann JPEG in den Papierkorb. Das JPEG zuletzt: scheitert
    //    es, bleibt das Hauptbild samt Beschreibung vollstaendig erhalten.
    let mut im_korb: Vec<String> = Vec::new();
    let mut reihe: Vec<&(String, Gefunden)> = teile.iter().filter(|(_, t)| matches!(t.teil, Teil::Quelle(_) | Teil::Jpeg)).collect();
    reihe.sort_by_key(|(g, t)| (matches!(t.teil, Teil::Jpeg), g.clone()));
    for (g, t) in reihe {
        if let Err(e) = papierkorb(&t.pfad) {
            let zusatz = if im_korb.is_empty() { String::new() } else { format!(" (schon im Papierkorb, dort zuruecklegbar: {})", im_korb.join(", ")) };
            let ende = zurueckrollen(wurzel, &stage);
            return Err(match ende {
                Ok(()) => Fehler::Io(format!("Papierkorb: {e}{zusatz}")),
                Err(r) => r,
            });
        }
        im_korb.push(rel(g, t));
    }
    // 3. Alles Rueckholbare liegt im Papierkorb: Marke setzen, Rest endgueltig entfernen.
    let _ = fs::write(stage.join(MARKE), b"");
    let _ = fs::remove_dir_all(&stage); // bleibt ein Rest, schliesst ihn der naechste Start ab
    let _ = fs::remove_dir(staging_wurzel(wurzel)); // nur, wenn leer
    for (g, _) in &teile {
        let _ = fs::remove_dir(wurzel.join(g)); // nur, wenn leer
    }
    Ok(entfernt)
}

/// Legt eine Eingangskopie ab (`<gruppe>/<id>.quelle.<ext>`) aus Bytes; fuer die Fensteraufnahme.
pub fn quelle_schreiben(wurzel: &Path, gruppe: &str, id: &str, format: QuellFormat, bytes: &[u8]) -> Result<Eintrag, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    name_pruefen("Bild-id", id)?;
    let dir = wurzel.join(gruppe);
    ordner_vorbereiten(wurzel, &dir)?;
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
    let _ = fs::remove_file(&tmp); // Rest eines frueheren Abbruchs (entfernt auch einen Link, nie sein Ziel)
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

    #[cfg(unix)]
    #[test]
    fn schreiben_verweigert_einen_linkordner_als_gruppe_und_als_wurzel() {
        let t = tempfile::tempdir().unwrap();
        let aussen = t.path().join("aussen");
        fs::create_dir_all(&aussen).unwrap();
        let wurzel = t.path().join("bilder");
        fs::create_dir_all(&wurzel).unwrap();
        std::os::unix::fs::symlink(&aussen, wurzel.join("g")).unwrap();
        assert!(matches!(schreiben(&wurzel, anfrage("g", "b1")), Err(Fehler::Pfad(_))));
        assert!(matches!(quelle_schreiben(&wurzel, "g", "b2", QuellFormat::Png, b"png"), Err(Fehler::Pfad(_))));
        assert!(dateien(&aussen).is_empty(), "ausserhalb wurde nichts geschrieben");
        // Wurzel selbst ein Link
        let wl = t.path().join("wurzel_link");
        std::os::unix::fs::symlink(&aussen, &wl).unwrap();
        assert!(matches!(schreiben(&wl, anfrage("h", "b1")), Err(Fehler::Pfad(_))));
        assert!(dateien(&aussen).is_empty());
        // Gegenprobe: ein echter Ordner geht
        assert!(schreiben(&wurzel, anfrage("echt", "b3")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn lesen_ueber_einen_linkordner_oder_eine_linkwurzel_scheitert() {
        let t = tempfile::tempdir().unwrap();
        let aussen = t.path().join("aussen");
        fs::create_dir_all(&aussen).unwrap();
        fs::write(aussen.join("x1.jpg"), jpeg(9)).unwrap();
        let wurzel = t.path().join("bilder");
        fs::create_dir_all(&wurzel).unwrap();
        std::os::unix::fs::symlink(&aussen, wurzel.join("g")).unwrap();
        assert!(matches!(lesen(&wurzel, "g/x1.jpg"), Err(Fehler::Pfad(_))));
        let wl = t.path().join("wl");
        std::os::unix::fs::symlink(t.path(), &wl).unwrap();
        assert!(matches!(lesen(&wl, "aussen/x1.jpg"), Err(Fehler::Pfad(_))));
    }

    fn bild_mit_quelle(wurzel: &Path) {
        schreiben(wurzel, anfrage("g", "b1")).unwrap();
        quelle_schreiben(wurzel, "eingang", "b1", QuellFormat::Png, b"png").unwrap();
    }

    fn komplett(wurzel: &Path) -> bool {
        dateien(&wurzel.join("g")) == ["b1.jpg", "b1.json", "b1.vorschau.jpg"]
    }

    /// Papierkorb, der erst beim n-ten Aufruf scheitert.
    fn korb_scheitert_bei(ziel: &Path, n: usize) -> impl Fn(&Path) -> std::io::Result<()> {
        fs::create_dir_all(ziel).unwrap();
        let ziel = ziel.to_path_buf();
        let zaehler = std::cell::Cell::new(0usize);
        move |p: &Path| {
            zaehler.set(zaehler.get() + 1);
            if zaehler.get() == n {
                return Err(std::io::Error::other("Papierkorb voll"));
            }
            fs::rename(p, ziel.join(p.file_name().unwrap()))
        }
    }

    #[test]
    fn scheitert_der_zweite_papierkorb_aufruf_ist_nichts_verloren_und_alles_zurueck() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        bild_mit_quelle(&wurzel);
        // Reihenfolge: Eingangskopie (1. Aufruf), JPEG (2. Aufruf) scheitert.
        let korb = korb_scheitert_bei(&t.path().join("korb"), 2);
        let e = loeschen(&wurzel, "g", "b1", &korb).unwrap_err();
        assert!(matches!(&e, Fehler::Io(m) if m.contains("Papierkorb")), "{e:?}");
        assert!(komplett(&wurzel), "JPEG, Sidecar und Vorschau stehen wieder an ihrem Platz: {:?}", dateien(&wurzel.join("g")));
        assert_eq!(dateien(&t.path().join("korb")), ["b1.quelle.png"], "die Eingangskopie ist im Papierkorb (rueckholbar)");
        assert!(!wurzel.join(".loeschen").exists(), "kein Rest im Staging");
        let l = liste(&wurzel);
        assert_eq!(l.eintraege.len(), 1);
        assert!(l.eintraege[0].vollstaendig, "das Bild ist ganz und noch loeschbar");
        // erneut, diesmal geht es
        let korb2 = TestKorb::neu(&t.path().join("korb2"), false);
        assert!(loeschen(&wurzel, "g", "b1", &|p| korb2.tue(p)).is_ok());
        assert!(liste(&wurzel).eintraege.is_empty());
    }

    #[test]
    fn scheitert_schon_der_erste_aufruf_bleibt_alles_wie_es_war() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        bild_mit_quelle(&wurzel);
        let korb = korb_scheitert_bei(&t.path().join("korb"), 1);
        assert!(loeschen(&wurzel, "g", "b1", &korb).is_err());
        assert!(komplett(&wurzel));
        assert_eq!(dateien(&wurzel.join("eingang")), ["b1.quelle.png"]);
        assert!(dateien(&t.path().join("korb")).is_empty());
        assert!(!wurzel.join(".loeschen").exists());
    }

    #[test]
    fn scheitert_das_beiseitestellen_kommt_alles_zurueck() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        schreiben(&wurzel, anfrage("g", "b1")).unwrap();
        let korb = TestKorb::neu(&t.path().join("korb"), false);
        // Die Vorschau wandert, das Sidecar scheitert.
        let r = loeschen_mit(&wurzel, "g", "b1", &|p| korb.tue(p), &|von, nach| {
            if von.file_name().unwrap() == "b1.json" {
                Err(std::io::Error::other("gesperrt"))
            } else {
                fs::rename(von, nach)
            }
        });
        assert!(matches!(r, Err(Fehler::Io(_))), "{r:?}");
        assert!(komplett(&wurzel), "Vorschau ist zurueck: {:?}", dateien(&wurzel.join("g")));
        assert!(korb.aufrufe.borrow().is_empty(), "nichts im Papierkorb");
        assert!(!wurzel.join(".loeschen").exists());
    }

    #[test]
    fn abbruch_mittendrin_vor_der_marke_wird_beim_start_zurueckgerollt() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        schreiben(&wurzel, anfrage("g", "b1")).unwrap();
        // Zustand nach einem Absturz: die Vorschau ist beiseite, der Rest noch da, keine Marke.
        let stage = wurzel.join(".loeschen/b1/g");
        fs::create_dir_all(&stage).unwrap();
        fs::rename(wurzel.join("g/b1.vorschau.jpg"), stage.join("b1.vorschau.jpg")).unwrap();
        assert_eq!(liste(&wurzel).eintraege.len(), 1, ".loeschen taucht in der Liste nie auf");
        assert_eq!(loeschreste_abschliessen(&wurzel), ["b1"]);
        assert!(komplett(&wurzel), "{:?}", dateien(&wurzel.join("g")));
        assert!(!wurzel.join(".loeschen").exists());
    }

    #[test]
    fn abbruch_nach_der_marke_wird_beim_start_abgeschlossen() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        fs::create_dir_all(wurzel.join("g")).unwrap();
        // JPEG und Eingangskopie liegen im Papierkorb; Vorschau und Sidecar stehen noch im Staging.
        let stage = wurzel.join(".loeschen/b1/g");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("b1.json"), b"{}").unwrap();
        fs::write(stage.join("b1.vorschau.jpg"), jpeg(1)).unwrap();
        fs::write(wurzel.join(".loeschen/b1/FERTIG"), b"").unwrap();
        // ein zweiter, unabhaengiger Vorgang ohne Marke
        let stage2 = wurzel.join(".loeschen/b2/g");
        fs::create_dir_all(&stage2).unwrap();
        fs::write(stage2.join("b2.json"), br#"{"bild_id":"b2"}"#).unwrap();
        let erledigt = loeschreste_abschliessen(&wurzel);
        assert_eq!(erledigt, ["b1", "b2"]);
        assert!(!wurzel.join(".loeschen").exists());
        assert_eq!(dateien(&wurzel.join("g")), ["b2.json"], "b1 endgueltig weg, b2 zurueckgerollt");
        // ohne Reste ist der Aufruf ein Nichts
        assert!(loeschreste_abschliessen(&wurzel).is_empty());
    }

    #[test]
    fn die_liste_zeigt_nie_etwas_aus_dem_staging() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        schreiben(&wurzel, anfrage("g", "b1")).unwrap();
        let stage = wurzel.join(".loeschen/b9/g");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("b9.jpg"), jpeg(3)).unwrap();
        fs::write(stage.join("b9.json"), b"{}").unwrap();
        let l = liste(&wurzel);
        assert_eq!(l.eintraege.len(), 1);
        assert!(l.eintraege.iter().all(|e| e.id != "b9"));
        assert!(matches!(lesen(&wurzel, ".loeschen/b9.jpg"), Err(Fehler::Pfad(_))));
    }
}
