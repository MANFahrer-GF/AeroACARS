//! Bestaetigte Quellordner und die Uebernahme einer Datei daraus (P-11.30).
//!
//! Der Kern entscheidet nicht, OB ein Bild uebernommen wird (aktiver Flug,
//! Vordergrund, Pixelmasse, Duplikat: Paket). Er erzwingt nur, was nicht dem
//! Paket ueberlassen werden darf, weil es Dateien des Piloten betrifft:
//!
//! * der Pfad liegt wirklich in einer **bestaetigten, aktiven** Quelle
//!   (aufgeloest, Symlinks und `..` helfen nicht);
//! * es ist eine JPG/PNG-Datei, deren Inhalt zur Endung passt und die
//!   **vollstaendig geschrieben** ist (Kopf-/Endkennung und stabile Groesse);
//! * **Verschieben** nur aus einer Quelle, die der Pilot dafuer freigegeben hat
//!   (`original_entfernen`); sonst bleibt das Original unangetastet;
//! * es laeuft ein **AeroACARS-Flug** (`Lage::flug_aktiv`), sonst Fehler
//!   `kein_flug` (P-11.30: ausserhalb eines Flugs bleibt alles unangetastet);
//! * Der Pfad wird genau **einmal** aufgeloest. Danach zaehlt nur noch dieses
//!   Ergebnis: jeder Teil ab der Quellwurzel darf kein Link sein, die Datei wird
//!   ohne Linkfolgen geoeffnet und **vom Handle** geprueft und gelesen
//!   ([`crate::sicher`]). „Verschieben“ ist Kopieren vom Handle und danach
//!   Entfernen, falls der Pfad noch dieselbe Datei ist.
//! * [`hinzufuegen`] ist der Weg fuer vom Piloten gewaehlte Dateien ausserhalb
//!   der Quellen und ausserhalb eines Flugs: nur kopieren, nie verschieben.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::namen::{name_pruefen, QuellFormat, Teil};
use crate::sicher;
use crate::speicher::{ms_seit_1970, MAX_DATEI};
use crate::Fehler;

pub const VORGABE_TASTENKUERZEL: &str = "Ctrl+Shift+S";
const MAX_QUELLEN: usize = 32;

/// Standard-Ablage eines Simulators (X-Plane `Output/screenshots`).
pub const ART_STANDARD: &str = "standard";
/// Aus `bild_quellen_finden` uebernommen.
pub const ART_AUFGESPUERT: &str = "aufgespuert";
/// Vom Piloten selbst gewaehlter Ordner (externes Screenshot-Werkzeug).
pub const ART_HAND: &str = "hand";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Quelle {
    pub pfad: String,
    /// Pausiert = wird nicht beobachtet und nichts daraus uebernommen.
    #[serde(default = "wahr")]
    pub aktiv: bool,
    /// Nur wenn der Pilot es fuer diese Quelle erlaubt hat, darf `verschieben` das Original entfernen.
    #[serde(default)]
    pub original_entfernen: bool,
    /// `standard`, `aufgespuert` oder `hand` (siehe `ART_*`); gespeichert und
    /// unveraendert durchgereicht, das Paket macht daran seine Regeln fest
    /// (bei `hand` entfaellt die Vordergrund-Regel). Andere Bezeichnungen des
    /// Pakets (z. B. `xplane_standard`) bleiben erlaubt.
    #[serde(default)]
    pub art: Option<String>,
}

fn wahr() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Einstellungen {
    #[serde(default)]
    pub quellen: Vec<Quelle>,
    #[serde(default = "vorgabe_taste")]
    pub tastenkuerzel: String,
    /// Yoke-/Joystick-Knopf als zweiter Ausloeser (nur Windows), sonst `None`.
    #[serde(default)]
    pub yoke: Option<crate::yoke::YokeTaste>,
}

fn vorgabe_taste() -> String {
    VORGABE_TASTENKUERZEL.to_string()
}

impl Default for Einstellungen {
    fn default() -> Self {
        Self { quellen: Vec::new(), tastenkuerzel: vorgabe_taste(), yoke: None }
    }
}

/// Liest die Einstellungen; fehlende oder kaputte Datei = Vorgabe.
pub fn einstellungen_lesen(datei: &Path) -> Einstellungen {
    fs::read(datei).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn einstellungen_schreiben(datei: &Path, e: &Einstellungen) -> Result<(), Fehler> {
    if let Some(dir) = datei.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = datei.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(e).map_err(|x| Fehler::Eingabe(x.to_string()))?;
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, datei).map_err(|x| {
        let _ = fs::remove_file(&tmp);
        Fehler::from(x)
    })
}

/// Loest einen Pfad auf, auch wenn die letzten Teile noch nicht existieren
/// (X-Plane legt `Output/screenshots` erst beim ersten Bild an).
fn aufloesen(p: &Path) -> Option<PathBuf> {
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = p.to_path_buf();
    loop {
        if let Ok(k) = fs::canonicalize(&cur) {
            let mut k = k;
            k.extend(rest.iter().rev());
            return Some(k);
        }
        let name = cur.file_name()?.to_owned();
        rest.push(name);
        if !cur.pop() {
            return None;
        }
    }
}

/// Ein vom Piloten gewaehlter Ordner: aktiv, **nur kopieren** (`original_entfernen`
/// aus; je Quelle spaeter einschaltbar), Art `hand`.
pub fn quelle_von_hand(pfad: &Path) -> Quelle {
    Quelle { pfad: pfad.to_string_lossy().into_owned(), aktiv: true, original_entfernen: false, art: Some(ART_HAND.to_string()) }
}

/// Haengt `neu` an die Liste. Steht der Ordner (aufgeloest) schon darin, bleibt
/// die Liste unveraendert (`false`). Prueft die Gesamtliste wie
/// [`einstellungen_pruefen`], damit nichts Unzulaessiges gespeichert wird.
pub fn quelle_anhaengen(e: &Einstellungen, neu: Quelle, bilder_wurzel: &Path, home: Option<&Path>) -> Result<(Einstellungen, bool), Fehler> {
    if let Some(k) = aufloesen(Path::new(&neu.pfad)) {
        if e.quellen.iter().any(|q| aufloesen(Path::new(&q.pfad)).as_deref() == Some(k.as_path())) {
            return Ok((e.clone(), false));
        }
    }
    let mut n = e.clone();
    n.quellen.push(neu);
    einstellungen_pruefen(&n, bilder_wurzel, home)?;
    Ok((n, true))
}

/// Die neue Einstellung aus dem Aufruf von `bild_quellen_setzen`: `quellen`
/// ersetzt die Liste (fehlt es, bleibt die bisherige), `taste` das Tastenkuerzel,
/// `ordner` (ein vom Piloten im Systemdialog gewaehlter Ordner) wird als Quelle
/// von Hand angehaengt. Gibt die geprueften Einstellungen zurueck.
pub fn einstellungen_neu(
    bisher: &Einstellungen,
    quellen: Option<Vec<Quelle>>,
    taste: Option<&str>,
    yoke: Option<Option<crate::yoke::YokeTaste>>,
    ordner: Option<&Path>,
    bilder_wurzel: &Path,
    home: Option<&Path>,
) -> Result<Einstellungen, Fehler> {
    let mut neu = Einstellungen {
        quellen: quellen.unwrap_or_else(|| bisher.quellen.clone()),
        tastenkuerzel: match taste {
            Some(t) => crate::tastenkuerzel::lesen(t)?.text,
            None => bisher.tastenkuerzel.clone(),
        },
        yoke: yoke.unwrap_or_else(|| bisher.yoke.clone()),
    };
    match ordner {
        Some(o) => {
            let (mit, _) = quelle_anhaengen(&neu, quelle_von_hand(o), bilder_wurzel, home)?;
            neu = mit;
        }
        None => einstellungen_pruefen(&neu, bilder_wurzel, home)?,
    }
    Ok(neu)
}

/// Prueft eine neue Einstellung, bevor sie gespeichert wird.
pub fn einstellungen_pruefen(e: &Einstellungen, bilder_wurzel: &Path, home: Option<&Path>) -> Result<(), Fehler> {
    crate::tastenkuerzel::lesen(&e.tastenkuerzel)?;
    if let Some(y) = &e.yoke {
        y.pruefen()?;
    }
    if e.quellen.len() > MAX_QUELLEN {
        return Err(Fehler::Eingabe(format!("Hoechstens {MAX_QUELLEN} Bildquellen")));
    }
    let wurzel = aufloesen(bilder_wurzel);
    let home = home.and_then(|h| fs::canonicalize(h).ok());
    let mut gesehen: Vec<PathBuf> = Vec::new();
    for q in &e.quellen {
        let roh = Path::new(&q.pfad);
        if q.pfad.is_empty() || q.pfad.len() > 1024 || !roh.is_absolute() || roh.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(Fehler::Eingabe(format!("Bildquelle {:?}: absoluter Ordnerpfad ohne „..“ erwartet", q.pfad)));
        }
        if roh.exists() && !roh.is_dir() {
            return Err(Fehler::Eingabe(format!("Bildquelle {:?} ist kein Ordner", q.pfad)));
        }
        let Some(k) = aufloesen(roh) else {
            return Err(Fehler::Eingabe(format!("Bildquelle {:?}: Ordner nicht erreichbar", q.pfad)));
        };
        if k.parent().is_none() {
            return Err(Fehler::Eingabe("Ein Laufwerks- oder Wurzelordner ist keine Bildquelle".into()));
        }
        if home.as_deref() == Some(k.as_path()) {
            return Err(Fehler::Eingabe("Der persoenliche Ordner selbst ist keine Bildquelle".into()));
        }
        if let Some(w) = &wurzel {
            if k.starts_with(w) || w.starts_with(&k) {
                return Err(Fehler::Eingabe("Der Bilderordner von AeroACARS ist keine Bildquelle".into()));
            }
        }
        if gesehen.contains(&k) {
            return Err(Fehler::Eingabe(format!("Bildquelle {:?} steht doppelt in der Liste", q.pfad)));
        }
        if q.art.as_deref().is_some_and(|a| a.len() > 64) {
            return Err(Fehler::Eingabe("Bezeichnung der Bildquelle zu lang".into()));
        }
        gesehen.push(k);
    }
    Ok(())
}

/// Die aktive Quelle, in der der **bereits aufgeloeste** Pfad `echt` liegt, samt
/// deren aufgeloester Wurzel. Der Aufrufer loest den Pfad genau einmal auf.
pub fn quelle_zu_pfad<'a>(echt: &Path, quellen: &'a [Quelle]) -> Result<(&'a Quelle, PathBuf), Fehler> {
    for q in quellen.iter().filter(|q| q.aktiv) {
        if let Ok(qk) = fs::canonicalize(&q.pfad) {
            if echt.starts_with(&qk) && echt != qk {
                return Ok((q, qk));
            }
        }
    }
    Err(Fehler::Quelle("Die Datei liegt in keiner bestaetigten, aktiven Bildquelle".into()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modus {
    Verschieben,
    Kopieren,
    /// Nur den Eingangseintrag entfernen; die Quelldatei bleibt unangetastet.
    Verwerfen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BildFormat {
    Png,
    Jpeg,
    /// HDR (Windows). Wird erkannt und gemeldet, aber nie uebernommen.
    Jxr,
}

impl BildFormat {
    pub fn aus_endung(p: &Path) -> Option<Self> {
        match p.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "png" => Some(Self::Png),
            "jpg" | "jpeg" => Some(Self::Jpeg),
            "jxr" => Some(Self::Jxr),
            _ => None,
        }
    }
    pub fn quell_format(self) -> Option<QuellFormat> {
        match self {
            Self::Png => Some(QuellFormat::Png),
            Self::Jpeg => Some(QuellFormat::Jpg),
            Self::Jxr => None,
        }
    }
}

const PNG_KOPF: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
const PNG_ENDE: [u8; 12] = [0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xAE, 0x42, 0x60, 0x82];

fn jpeg_ende_ok(ende: &[u8]) -> bool {
    // Manche Schreiber haengen Nullbytes an; EOI (FFD9) muss das letzte „echte“ Paar sein.
    let mut e = ende;
    while let [rest @ .., 0] = e {
        e = rest;
    }
    e.ends_with(&[0xFF, 0xD9])
}

/// Inhalt zur Endung passend und am Ende abgeschlossen? Reine Pruefung auf Bytes.
pub fn inhalt_vollstaendig(format: BildFormat, kopf: &[u8], ende: &[u8]) -> Result<(), Fehler> {
    match format {
        BildFormat::Png => {
            if !kopf.starts_with(&PNG_KOPF) {
                return Err(Fehler::Format("Datei ist kein PNG".into()));
            }
            if !ende.ends_with(&PNG_ENDE) {
                return Err(Fehler::Unfertig("PNG ist noch nicht vollstaendig geschrieben".into()));
            }
        }
        BildFormat::Jpeg => {
            if !(kopf.len() >= 3 && kopf[..3] == [0xFF, 0xD8, 0xFF]) {
                return Err(Fehler::Format("Datei ist kein JPEG".into()));
            }
            if !jpeg_ende_ok(ende) {
                return Err(Fehler::Unfertig("JPEG ist noch nicht vollstaendig geschrieben".into()));
            }
        }
        BildFormat::Jxr => return Err(Fehler::Format("HDR-Aufnahme (JXR) wird nicht uebernommen".into())),
    }
    Ok(())
}

/// Wie [`datei_vollstaendig`], aber an einem **schon geoeffneten** Handle (Kopf
/// und Ende werden von dort gelesen, nicht ueber den Pfad). Der Lesezeiger steht
/// danach irgendwo; wer weiterliest, setzt ihn zurueck.
pub fn handle_vollstaendig(f: &mut fs::File, format: BildFormat) -> Result<u64, Fehler> {
    if format == BildFormat::Jxr {
        return Err(Fehler::Format("HDR-Aufnahme (JXR) wird nicht uebernommen".into()));
    }
    let md = f.metadata()?;
    if !md.is_file() {
        return Err(Fehler::Format("Keine Datei".into()));
    }
    let len = md.len();
    if len < 24 {
        return Err(Fehler::Unfertig("Datei ist (noch) zu klein".into()));
    }
    if len > MAX_DATEI {
        return Err(Fehler::Gross("Datei ist zu gross".into()));
    }
    let io = |e: std::io::Error| Fehler::Unfertig(format!("Datei nicht lesbar (noch in Benutzung?): {e}"));
    f.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut kopf = [0u8; 16];
    f.read_exact(&mut kopf).map_err(io)?;
    let ende_len = len.min(64);
    f.seek(SeekFrom::Start(len - ende_len)).map_err(io)?;
    let mut ende = vec![0u8; ende_len as usize];
    f.read_exact(&mut ende).map_err(io)?;
    inhalt_vollstaendig(format, &kopf, &ende)?;
    Ok(len)
}

/// Oeffnet (ohne Linkfolgen) und prueft, ob die Datei vollstaendig ist. Eine
/// Datei, die ein anderer Prozess noch exklusiv haelt, laesst sich nicht
/// oeffnen und zaehlt als unfertig.
pub fn datei_vollstaendig(pfad: &Path) -> Result<(BildFormat, u64), Fehler> {
    let format = BildFormat::aus_endung(pfad).ok_or_else(|| Fehler::Format("Nur PNG- und JPEG-Dateien".into()))?;
    if format == BildFormat::Jxr {
        return Err(Fehler::Format("HDR-Aufnahme (JXR) wird nicht uebernommen".into()));
    }
    let mut f = sicher::lesend_ohne_folgen(pfad).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Fehler::NichtGefunden(e.to_string()),
        _ => Fehler::Unfertig(format!("Datei nicht lesbar (noch in Benutzung?): {e}")),
    })?;
    Ok((format, handle_vollstaendig(&mut f, format)?))
}

/// Groesse und Aenderungszeit bleiben ueber `pause` gleich.
pub fn groesse_stabil(pfad: &Path, pause: Duration) -> Result<(), Fehler> {
    let a = fs::metadata(pfad)?;
    std::thread::sleep(pause);
    let b = fs::metadata(pfad)?;
    if a.len() != b.len() || a.modified().ok() != b.modified().ok() {
        return Err(Fehler::Unfertig("Datei wird noch geschrieben".into()));
    }
    Ok(())
}

/// Groesse und Aenderungszeit des **geoeffneten** Handles bleiben ueber `pause` gleich.
pub fn handle_stabil(f: &fs::File, pause: Duration) -> Result<(), Fehler> {
    let a = f.metadata()?;
    std::thread::sleep(pause);
    let b = f.metadata()?;
    if a.len() != b.len() || a.modified().ok() != b.modified().ok() {
        return Err(Fehler::Unfertig("Datei wird noch geschrieben".into()));
    }
    Ok(())
}

/// Pixelmasse aus dem Dateikopf (PNG: IHDR, JPEG: erster SOF-Marker).
pub fn abmessungen(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() >= 24 && bytes.starts_with(&PNG_KOPF) && &bytes[12..16] == b"IHDR" {
        let w = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if bytes.len() > 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2usize;
        while i + 4 <= bytes.len() {
            if bytes[i] != 0xFF {
                return None;
            }
            let m = bytes[i + 1];
            if m == 0xFF {
                i += 1;
                continue;
            }
            if (0xD0..=0xD9).contains(&m) || m == 0x01 {
                i += 2;
                continue;
            }
            let seg = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            if (0xC0..=0xCF).contains(&m) && !matches!(m, 0xC4 | 0xC8 | 0xCC) {
                if i + 9 > bytes.len() {
                    return None;
                }
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
                return Some((w, h));
            }
            i += 2 + seg;
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Uebernahme {
    pub id: String,
    pub gruppe: String,
    /// `<gruppe>/<id>.quelle.<png|jpg>` im Bilderordner; mit `bild_lesen` zu holen.
    pub quelle_pfad: String,
    pub format: BildFormat,
    pub groesse: u64,
    /// Dateizeit des Originals (ms seit 1970, UTC).
    pub dateizeit_ms: Option<i64>,
    pub modus: Modus,
    pub original_entfernt: bool,
}

/// Kopiert die Bytes des geoeffneten Handles `src` in eine neue Datei `ziel`
/// (zuerst Temp, dann Umbenennen).
fn kopieren_atomar(src: &mut fs::File, dir: &Path, ziel: &Path, erwartet: u64) -> Result<(), Fehler> {
    let tmp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let r = (|| -> Result<(), Fehler> {
        src.seek(SeekFrom::Start(0))?;
        let mut dst = sicher::neu_anlegen(&tmp)?;
        let n = std::io::copy(&mut src.take(MAX_DATEI + 1), &mut dst)?;
        dst.sync_all()?;
        if n != erwartet {
            return Err(Fehler::Unfertig("Datei hat sich waehrend des Kopierens veraendert".into()));
        }
        fs::rename(&tmp, ziel)?;
        Ok(())
    })();
    if r.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    r
}

/// Lage, in der eine Uebernahme stattfindet.
#[derive(Debug, Clone, Copy)]
pub struct Lage {
    /// Laeuft ein AeroACARS-Flug? Sonst wird nichts uebernommen.
    pub flug_aktiv: bool,
    /// Wartezeit der Groessenpruefung („wird noch geschrieben?“).
    pub stabil_pause: Duration,
}

#[cfg(test)]
thread_local! {
    /// Test-Haken: laeuft zwischen Pfadpruefung und Oeffnen (Pfad austauschen).
    pub(crate) static ZWISCHEN: std::cell::RefCell<Option<Box<dyn Fn()>>> = const { std::cell::RefCell::new(None) };
}

fn zwischen_haken() {
    #[cfg(test)]
    ZWISCHEN.with(|z| {
        if let Some(f) = &*z.borrow() {
            f();
        }
    });
}

/// Der gemeinsame Teil von [`uebernehmen`] und [`hinzufuegen`]: `basis/rel` ist
/// aufgeloest und gehoert in die Quelle bzw. ist die gewaehlte Datei.
fn datei_uebernehmen(
    wurzel: &Path,
    basis: &Path,
    rel: &Path,
    modus: Modus,
    gruppe: &str,
    id: &str,
    stabil_pause: Duration,
) -> Result<Uebernahme, Fehler> {
    let echt = basis.join(rel);
    let format = BildFormat::aus_endung(&echt).ok_or_else(|| Fehler::Format("Nur PNG- und JPEG-Dateien".into()))?;
    let (mut f, _) = sicher::datei_oeffnen_mit(basis, rel, &zwischen_haken)?;
    let kennung0 = sicher::Kennung::von_handle(&f);
    let groesse = handle_vollstaendig(&mut f, format)?;
    handle_stabil(&f, stabil_pause)?;
    let md = f.metadata()?;
    if md.len() != groesse {
        return Err(Fehler::Unfertig("Datei wird noch geschrieben".into()));
    }
    let dateizeit_ms = md.modified().ok().and_then(ms_seit_1970);
    let qf = format.quell_format().ok_or_else(|| Fehler::Format("HDR-Aufnahme (JXR) wird nicht uebernommen".into()))?;
    let dir = wurzel.join(gruppe);
    crate::speicher::ordner_vorbereiten(wurzel, &dir)?;
    let ziel = dir.join(Teil::Quelle(qf).dateiname(id));
    if ziel.exists() {
        return Err(Fehler::Name(format!("Bild-id {id} hat in {gruppe} schon eine Eingangskopie")));
    }
    kopieren_atomar(&mut f, &dir, &ziel, groesse)?;
    let mut original_entfernt = false;
    if modus == Modus::Verschieben {
        // Nur entfernen, wenn der Pfad noch genau die kopierte Datei ist.
        let gleich = sicher::Kennung::gleich(kennung0, sicher::Kennung::von_pfad(&echt));
        let r = if gleich { fs::remove_file(&echt).map_err(|e| e.to_string()) } else { Err("Die Datei wurde ausgetauscht".to_string()) };
        if let Err(e) = r {
            let _ = fs::remove_file(&ziel);
            return Err(Fehler::Io(format!("Original konnte nicht entfernt werden, nichts uebernommen: {e}")));
        }
        original_entfernt = true;
    }
    Ok(Uebernahme {
        quelle_pfad: crate::namen::rel_pfad(gruppe, &Teil::Quelle(qf).dateiname(id)),
        id: id.to_string(),
        gruppe: gruppe.to_string(),
        format,
        groesse,
        dateizeit_ms,
        modus,
        original_entfernt,
    })
}

/// Hoechstens so viele Dateien je Aufruf von `bild_hinzufuegen`.
pub const MAX_HINZUFUEGEN: usize = 500;

/// Pfad und Ergebnis einer einzelnen Datei bei [`hinzufuegen_mehrere`].
pub type HinzufuegenErgebnis = (String, Result<Uebernahme, Fehler>);

/// [`hinzufuegen`] fuer mehrere Dateien; jede ergibt ihr eigenes Ergebnis, ein
/// Fehler stoppt die anderen nicht.
pub fn hinzufuegen_mehrere(wurzel: &Path, pfade: &[String], gruppe: &str, stabil_pause: Duration) -> Result<Vec<HinzufuegenErgebnis>, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    if pfade.len() > MAX_HINZUFUEGEN {
        return Err(Fehler::Eingabe(format!("Hoechstens {MAX_HINZUFUEGEN} Dateien auf einmal")));
    }
    Ok(pfade.iter().map(|p| (p.clone(), hinzufuegen(wurzel, p, gruppe, None, stabil_pause))).collect())
}

fn id_waehlen(gruppe: &str, id: Option<&str>) -> Result<String, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    Ok(match id {
        Some(i) => {
            name_pruefen("Bild-id", i)?;
            i.to_string()
        }
        None => uuid::Uuid::new_v4().to_string(),
    })
}

/// Nimmt eine Datei aus einer bestaetigten Quelle in den Bilderordner. Das
/// Paket liest sie von dort (`bild_lesen`), macht das JPEG und schreibt es mit
/// `bild_schreiben` (`quelle_entfernen`); bis dahin ist nichts verloren, auch
/// wenn das Programm abbricht. Ohne aktiven Flug: `kein_flug`. (Der Befehl
/// arbeitet ueber den Eingang, siehe [`uebernehmen_fund`]; diese Funktion ist
/// der Pfad-Teil dahinter.)
pub fn uebernehmen(
    wurzel: &Path,
    quellen: &[Quelle],
    pfad: &str,
    modus: Modus,
    gruppe: &str,
    id: Option<&str>,
    lage: Lage,
) -> Result<Uebernahme, Fehler> {
    if !lage.flug_aktiv {
        return Err(Fehler::KeinFlug("Ohne AeroACARS-Flug wird nichts uebernommen".into()));
    }
    uebernehmen_pfad(wurzel, quellen, pfad, modus, gruppe, id, lage.stabil_pause)
}

fn uebernehmen_pfad(
    wurzel: &Path,
    quellen: &[Quelle],
    pfad: &str,
    modus: Modus,
    gruppe: &str,
    id: Option<&str>,
    stabil_pause: Duration,
) -> Result<Uebernahme, Fehler> {
    if modus == Modus::Verwerfen {
        return Err(Fehler::Eingabe("Verwerfen hat keine Datei zu uebernehmen".into()));
    }
    let id = id_waehlen(gruppe, id)?;
    let p = Path::new(pfad);
    if !p.is_absolute() {
        return Err(Fehler::Quelle("Absoluter Dateipfad erwartet".into()));
    }
    // Genau eine Aufloesung; ab hier zaehlt nur `echt`.
    let echt = fs::canonicalize(p).map_err(|_| Fehler::NichtGefunden(format!("Datei nicht gefunden: {}", p.display())))?;
    let (quelle, qk) = quelle_zu_pfad(&echt, quellen)?;
    if modus == Modus::Verschieben && !quelle.original_entfernen {
        return Err(Fehler::Verboten("Aus dieser Quelle darf nicht verschoben werden, nur kopiert".into()));
    }
    let rel = echt.strip_prefix(&qk).map_err(|_| Fehler::Quelle("Pfad liegt nicht in der Quelle".into()))?;
    datei_uebernehmen(wurzel, &qk, rel, modus, gruppe, &id, stabil_pause)
}

/// Ergebnis von [`uebernehmen_fund`].
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(untagged)]
pub enum FundErgebnis {
    Uebernommen(Uebernahme),
    Verworfen { fund_id: String, verworfen: bool },
}

/// Arbeitet einen Fund des Eingangs ab. Der Fund steht nur im Eingang, wenn er
/// in einem aktiven Flug entstand (Pruefung `hat_flug`); die Uebernahme selbst
/// darf nach Flugende laufen. `Verwerfen` entfernt nur den Eintrag und fasst die
/// Quelldatei nie an (eine eigene Aufnahme-Kopie im Bilderordner wird mit
/// entfernt, sie hat kein Original). Nach Erfolg wird der Eintrag entfernt.
pub fn uebernehmen_fund(
    wurzel: &Path,
    quellen: &[Quelle],
    fund_id: &str,
    modus: Modus,
    gruppe: Option<&str>,
    id: Option<&str>,
    stabil_pause: Duration,
) -> Result<FundErgebnis, Fehler> {
    let fund = crate::eingang::lesen(wurzel, fund_id)?;
    if modus == Modus::Verwerfen {
        if fund.herkunft == "aufnahme" {
            if let Some(rel) = &fund.quelle_pfad {
                if let Ok((g, i, t)) = crate::namen::rel_pfad_lesen(rel) {
                    let p = wurzel.join(&g).join(t.dateiname(&i));
                    if fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                        fs::remove_file(&p)?;
                    }
                    let _ = fs::remove_dir(wurzel.join(g)); // nur, wenn leer
                }
            }
        }
        crate::eingang::entfernen(wurzel, fund_id)?;
        return Ok(FundErgebnis::Verworfen { fund_id: fund_id.to_string(), verworfen: true });
    }
    if !fund.hat_flug() {
        return Err(Fehler::KeinFlug("Dieser Fund entstand nicht in einem AeroACARS-Flug".into()));
    }
    if !fund.uebernehmbar {
        return Err(Fehler::Format("Dieser Fund (z. B. HDR) laesst sich nicht uebernehmen; bitte verwerfen".into()));
    }
    let gruppe = gruppe.ok_or_else(|| Fehler::Eingabe("gruppe fehlt".into()))?;
    let u = match fund.herkunft.as_str() {
        "aufnahme" => aufnahme_umhaengen(wurzel, &fund, modus, gruppe, id)?,
        _ => {
            let pfad = fund.pfad.as_deref().ok_or_else(|| Fehler::Eingabe("Fund ohne Pfad".into()))?;
            uebernehmen_pfad(wurzel, quellen, pfad, modus, gruppe, id, stabil_pause)?
        }
    };
    crate::eingang::entfernen(wurzel, fund_id)?;
    Ok(FundErgebnis::Uebernommen(u))
}

/// Eine Fensteraufnahme liegt schon im Bilderordner: sie wandert in die gewaehlte Gruppe.
fn aufnahme_umhaengen(wurzel: &Path, fund: &crate::eingang::EingangsFund, modus: Modus, gruppe: &str, id: Option<&str>) -> Result<Uebernahme, Fehler> {
    let rel = fund.quelle_pfad.as_deref().ok_or_else(|| Fehler::Eingabe("Aufnahme ohne Eingangskopie".into()))?;
    let (g0, id0, teil) = crate::namen::rel_pfad_lesen(rel)?;
    let Teil::Quelle(qf) = teil else { return Err(Fehler::Pfad("Eingangskopie erwartet".into())) };
    let neue_id = id_waehlen(gruppe, id.or(Some(&id0)))?;
    let (f, md) = sicher::datei_oeffnen(wurzel, Path::new(&g0).join(teil.dateiname(&id0)).as_path())?;
    drop(f);
    let dir = wurzel.join(gruppe);
    crate::speicher::ordner_vorbereiten(wurzel, &dir)?;
    let ziel = dir.join(Teil::Quelle(qf).dateiname(&neue_id));
    if ziel.exists() {
        return Err(Fehler::Name(format!("Bild-id {neue_id} hat in {gruppe} schon eine Eingangskopie")));
    }
    fs::rename(wurzel.join(&g0).join(teil.dateiname(&id0)), &ziel)?;
    if g0 != gruppe {
        let _ = fs::remove_dir(wurzel.join(&g0)); // nur, wenn leer
    }
    Ok(Uebernahme {
        quelle_pfad: crate::namen::rel_pfad(gruppe, &Teil::Quelle(qf).dateiname(&neue_id)),
        id: neue_id,
        gruppe: gruppe.to_string(),
        format: if qf == QuellFormat::Png { BildFormat::Png } else { BildFormat::Jpeg },
        groesse: md.len(),
        dateizeit_ms: fund.dateizeit_ms,
        modus,
        original_entfernt: false,
    })
}

/// Eine vom Piloten gewaehlte oder hineingezogene Datei (auch ausserhalb der
/// Quellen und ausserhalb eines Flugs, z. B. fuer die Gruppe „Ohne Flug“).
/// Nur **kopieren**, nie verschieben. Nur JPG/PNG, vollstaendig und stabil.
/// Dateien im Bilderordner selbst sind keine Quelle.
pub fn hinzufuegen(wurzel: &Path, pfad: &str, gruppe: &str, id: Option<&str>, stabil_pause: Duration) -> Result<Uebernahme, Fehler> {
    let id = id_waehlen(gruppe, id)?;
    let p = Path::new(pfad);
    if !p.is_absolute() {
        return Err(Fehler::Quelle("Absoluter Dateipfad erwartet".into()));
    }
    let echt = fs::canonicalize(p).map_err(|_| Fehler::NichtGefunden(format!("Datei nicht gefunden: {}", p.display())))?;
    if let Some(w) = aufloesen(wurzel) {
        if echt.starts_with(&w) {
            return Err(Fehler::Quelle("Dateien aus dem Bilderordner von AeroACARS lassen sich nicht noch einmal hinzufuegen".into()));
        }
    }
    let (Some(eltern), Some(name)) = (echt.parent(), echt.file_name()) else {
        return Err(Fehler::Quelle("Dateipfad ungueltig".into()));
    };
    datei_uebernehmen(wurzel, eltern, Path::new(name), Modus::Kopieren, gruppe, &id, stabil_pause)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut v = PNG_KOPF.to_vec();
        v.extend_from_slice(&[0, 0, 0, 13]);
        v.extend_from_slice(b"IHDR");
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]); // Rest IHDR + CRC (Pruefsumme egal)
        v.extend_from_slice(&[0, 0, 0, 4]);
        v.extend_from_slice(b"IDAT");
        v.extend_from_slice(&[1, 2, 3, 4, 0, 0, 0, 0]);
        v.extend_from_slice(&PNG_ENDE);
        v
    }

    pub(crate) fn jpeg_bytes(w: u16, h: u16) -> Vec<u8> {
        let mut v = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0];
        v.extend_from_slice(&[0xFF, 0xC0, 0, 11, 8]);
        v.extend_from_slice(&h.to_be_bytes());
        v.extend_from_slice(&w.to_be_bytes());
        v.extend_from_slice(&[1, 1, 0x11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0xD9]);
        v
    }

    fn quelle(dir: &Path, entfernen: bool) -> Quelle {
        Quelle { pfad: dir.to_string_lossy().into(), aktiv: true, original_entfernen: entfernen, art: None }
    }

    const PAUSE: Duration = Duration::from_millis(20);
    const LAGE: Lage = Lage { flug_aktiv: true, stabil_pause: PAUSE };

    fn aufbau(entfernen: bool) -> (tempfile::TempDir, PathBuf, PathBuf, Vec<Quelle>) {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        let src = t.path().join("screens");
        fs::create_dir_all(src.join("unter")).unwrap();
        let q = vec![quelle(&src, entfernen)];
        (t, wurzel, src, q)
    }

    #[test]
    fn kopieren_laesst_das_original_verschieben_entfernt_es() {
        let (_t, wurzel, src, q) = aufbau(true);
        let a = src.join("a.png");
        fs::write(&a, png_bytes(100, 50)).unwrap();
        let r = uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Kopieren, "g", Some("b1"), LAGE).unwrap();
        assert!(a.exists(), "Kopieren laesst das Original");
        assert!(!r.original_entfernt);
        assert_eq!(r.quelle_pfad, "g/b1.quelle.png");
        assert_eq!(fs::read(wurzel.join("g/b1.quelle.png")).unwrap(), png_bytes(100, 50));

        let b = src.join("unter").join("b.jpg");
        fs::write(&b, jpeg_bytes(640, 480)).unwrap();
        let r = uebernehmen(&wurzel, &q, b.to_str().unwrap(), Modus::Verschieben, "g", Some("b2"), LAGE).unwrap();
        assert!(!b.exists(), "Verschieben entfernt das Original");
        assert!(r.original_entfernt);
        assert_eq!(r.format, BildFormat::Jpeg);
        assert_eq!(fs::read(wurzel.join("g/b2.quelle.jpg")).unwrap(), jpeg_bytes(640, 480));
    }

    #[test]
    fn verschieben_nur_mit_freigabe_der_quelle() {
        let (_t, wurzel, src, q) = aufbau(false);
        let a = src.join("a.png");
        fs::write(&a, png_bytes(10, 10)).unwrap();
        let e = uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Verschieben, "g", None, LAGE).unwrap_err();
        assert!(matches!(e, Fehler::Verboten(_)));
        assert!(a.exists());
        assert!(!wurzel.exists() || crate::speicher::liste(&wurzel).eintraege.is_empty());
    }

    #[test]
    fn datei_ausserhalb_jeder_quelle_wird_abgelehnt() {
        let (t, wurzel, src, q) = aufbau(true);
        let fremd = t.path().join("fremd.png");
        fs::write(&fremd, png_bytes(10, 10)).unwrap();
        let e = uebernehmen(&wurzel, &q, fremd.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE).unwrap_err();
        assert!(matches!(e, Fehler::Quelle(_)));
        // „..“ aus der Quelle heraus
        let ausbruch = format!("{}/../fremd.png", src.display());
        assert!(matches!(uebernehmen(&wurzel, &q, &ausbruch, Modus::Kopieren, "g", None, LAGE), Err(Fehler::Quelle(_))));
        // Ordner mit aehnlichem Namensanfang ist keine Unterordner-Beziehung
        let nachbar = t.path().join("screens-privat");
        fs::create_dir_all(&nachbar).unwrap();
        fs::write(nachbar.join("x.png"), png_bytes(10, 10)).unwrap();
        let p = nachbar.join("x.png");
        assert!(matches!(uebernehmen(&wurzel, &q, p.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE), Err(Fehler::Quelle(_))));
        // relativ
        assert!(matches!(uebernehmen(&wurzel, &q, "a.png", Modus::Kopieren, "g", None, LAGE), Err(Fehler::Quelle(_))));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aus_der_quelle_heraus_hilft_nicht() {
        let (t, wurzel, src, q) = aufbau(true);
        let fremd = t.path().join("privat.png");
        fs::write(&fremd, png_bytes(10, 10)).unwrap();
        std::os::unix::fs::symlink(&fremd, src.join("link.png")).unwrap();
        let p = src.join("link.png");
        assert!(matches!(uebernehmen(&wurzel, &q, p.to_str().unwrap(), Modus::Verschieben, "g", None, LAGE), Err(Fehler::Quelle(_))));
        assert!(fremd.exists());
    }

    #[test]
    fn pausierte_quelle_zaehlt_nicht() {
        let (_t, wurzel, src, mut q) = aufbau(true);
        q[0].aktiv = false;
        let a = src.join("a.png");
        fs::write(&a, png_bytes(10, 10)).unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE), Err(Fehler::Quelle(_))));
    }

    #[test]
    fn nur_vollstaendig_geschriebene_jpg_und_png() {
        let (_t, wurzel, src, q) = aufbau(true);
        let voll = png_bytes(10, 10);
        let abgeschnitten = src.join("halb.png");
        fs::write(&abgeschnitten, &voll[..voll.len() - 6]).unwrap();
        let p = abgeschnitten.to_str().unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, p, Modus::Kopieren, "g", None, LAGE), Err(Fehler::Unfertig(_))));
        let jpg = jpeg_bytes(10, 10);
        let halb_jpg = src.join("halb.jpg");
        fs::write(&halb_jpg, &jpg[..jpg.len() - 5]).unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, halb_jpg.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE), Err(Fehler::Unfertig(_))));
        // Nullbytes am Ende eines fertigen JPEG sind erlaubt
        let mit_nullen = src.join("null.jpg");
        let mut j = jpg.clone();
        j.extend_from_slice(&[0; 8]);
        fs::write(&mit_nullen, &j).unwrap();
        assert!(uebernehmen(&wurzel, &q, mit_nullen.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE).is_ok());
        // Endung luegt
        let falsch = src.join("falsch.png");
        fs::write(&falsch, jpeg_bytes(10, 10)).unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, falsch.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE), Err(Fehler::Format(_))));
        // fremde Formate, HDR, zu klein
        for name in ["x.gif", "x.jxr", "x.txt", "x"] {
            let f = src.join(name);
            fs::write(&f, png_bytes(10, 10)).unwrap();
            assert!(matches!(uebernehmen(&wurzel, &q, f.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE), Err(Fehler::Format(_))), "{name}");
        }
        let klein = src.join("klein.png");
        fs::write(&klein, b"\x89PNG").unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, klein.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE), Err(Fehler::Unfertig(_))));
        assert!(abgeschnitten.exists() && halb_jpg.exists(), "abgelehnte Dateien bleiben unangetastet");
    }

    #[test]
    fn eine_waehrend_der_pruefung_wachsende_datei_ist_unfertig() {
        let (_t, _wurzel, src, _q) = aufbau(true);
        let f = src.join("waechst.png");
        let voll = png_bytes(10, 10);
        fs::write(&f, &voll).unwrap();
        let f2 = f.clone();
        let h = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            let mut a = fs::OpenOptions::new().append(true).open(&f2).unwrap();
            a.write_all(&voll).unwrap();
        });
        let r = groesse_stabil(&f, Duration::from_millis(200));
        h.join().unwrap();
        assert!(matches!(r, Err(Fehler::Unfertig(_))));
        assert!(groesse_stabil(&f, Duration::from_millis(30)).is_ok(), "danach ruhig = stabil");
    }

    #[test]
    fn abmessungen_png_und_jpeg() {
        assert_eq!(abmessungen(&png_bytes(2560, 1440)), Some((2560, 1440)));
        assert_eq!(abmessungen(&jpeg_bytes(1470, 896)), Some((1470, 896)));
        assert_eq!(abmessungen(b"nix"), None);
    }

    #[test]
    fn einstellungen_pruefen_lehnt_gefaehrliche_quellen_ab() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        fs::create_dir_all(wurzel.join("g")).unwrap();
        let home = t.path().join("home");
        fs::create_dir_all(home.join("Bilder")).unwrap();
        let e = |pfade: &[&Path]| Einstellungen {
            quellen: pfade.iter().map(|p| quelle(p, false)).collect(),
            ..Default::default()
        };
        let ok = home.join("Bilder");
        assert!(einstellungen_pruefen(&e(&[&ok]), &wurzel, Some(&home)).is_ok());
        let noch_nicht_da = home.join("X-Plane").join("Output").join("screenshots");
        assert!(einstellungen_pruefen(&e(&[&noch_nicht_da]), &wurzel, Some(&home)).is_ok(), "darf erst beim ersten Bild entstehen");
        let wurzelordner = if cfg!(windows) { PathBuf::from("C:\\") } else { PathBuf::from("/") };
        for schlecht in [&wurzelordner, &home, &wurzel, &wurzel.join("g"), t.path()] {
            assert!(einstellungen_pruefen(&e(&[schlecht]), &wurzel, Some(&home)).is_err(), "{schlecht:?}");
        }
        assert!(einstellungen_pruefen(&e(&[Path::new("relativ/ordner")]), &wurzel, None).is_err());
        let mit_punkten = format!("{}/../home/Bilder", t.path().display());
        let mut q = e(&[&ok]);
        q.quellen[0].pfad = mit_punkten;
        assert!(einstellungen_pruefen(&q, &wurzel, None).is_err());
        assert!(einstellungen_pruefen(&e(&[&ok, &ok]), &wurzel, None).is_err(), "doppelt");
        let datei = home.join("datei.txt");
        fs::write(&datei, b"x").unwrap();
        assert!(einstellungen_pruefen(&e(&[&datei]), &wurzel, None).is_err(), "kein Ordner");
        let mut s = e(&[&ok]);
        s.tastenkuerzel = "S".into();
        assert!(einstellungen_pruefen(&s, &wurzel, None).is_err(), "Tastenkuerzel ohne Modifikator");
    }

    #[test]
    fn einstellungen_speichern_und_lesen() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("conf").join("bilder-einstellungen.json");
        assert_eq!(einstellungen_lesen(&d), Einstellungen::default());
        let e = Einstellungen { quellen: vec![quelle(t.path(), true)], tastenkuerzel: "Alt+F9".into(), yoke: None };
        einstellungen_schreiben(&d, &e).unwrap();
        assert_eq!(einstellungen_lesen(&d), e);
        fs::write(&d, b"{kaputt").unwrap();
        assert_eq!(einstellungen_lesen(&d), Einstellungen::default());
        assert_eq!(Einstellungen::default().tastenkuerzel, "Ctrl+Shift+S");
    }

    #[test]
    fn ohne_aktiven_flug_wird_nichts_uebernommen() {
        let (_t, wurzel, src, q) = aufbau(true);
        let a = src.join("a.png");
        fs::write(&a, png_bytes(100, 50)).unwrap();
        let kein_flug = Lage { flug_aktiv: false, stabil_pause: PAUSE };
        for modus in [Modus::Kopieren, Modus::Verschieben] {
            let e = uebernehmen(&wurzel, &q, a.to_str().unwrap(), modus, "g", Some("b1"), kein_flug).unwrap_err();
            assert!(matches!(e, Fehler::KeinFlug(_)), "{e:?}");
            assert_eq!(e.code(), "kein_flug");
        }
        assert!(a.exists(), "das Original bleibt unangetastet");
        assert!(!wurzel.exists(), "nichts im Bilderordner angelegt");
        assert!(uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Kopieren, "g", Some("b1"), LAGE).is_ok(), "Gegenprobe: mit Flug geht es");
    }

    #[test]
    fn ein_zwischen_pruefung_und_oeffnen_ausgetauschter_pfad_wird_nicht_uebernommen() {
        let (t, wurzel, src, q) = aufbau(true);
        let privat = t.path().join("privat.png");
        fs::write(&privat, png_bytes(7, 7)).unwrap();
        let a = src.join("a.png");
        fs::write(&a, png_bytes(100, 50)).unwrap();
        let (a2, privat2) = (a.clone(), privat.clone());
        ZWISCHEN.with(|z| {
            *z.borrow_mut() = Some(Box::new(move || {
                fs::remove_file(&a2).unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(&privat2, &a2).unwrap();
                #[cfg(not(unix))]
                {
                    let _ = &privat2;
                }
            }));
        });
        let r = uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Kopieren, "g", Some("b1"), LAGE);
        ZWISCHEN.with(|z| *z.borrow_mut() = None);
        #[cfg(unix)]
        {
            assert!(r.is_err(), "{r:?}");
            assert!(privat.exists(), "die fremde Datei bleibt");
            assert!(!wurzel.join("g/b1.quelle.png").exists(), "nichts uebernommen");
        }
        #[cfg(not(unix))]
        let _ = r;
    }

    #[cfg(unix)]
    #[test]
    fn eine_quelle_die_selbst_ein_link_ist_funktioniert_der_pfad_wird_einmal_aufgeloest() {
        let (t, wurzel, src, _q) = aufbau(true);
        let link = t.path().join("link_zur_quelle");
        std::os::unix::fs::symlink(&src, &link).unwrap();
        let q = vec![quelle(&link, false)];
        let a = src.join("a.png");
        fs::write(&a, png_bytes(100, 50)).unwrap();
        let ueber_link = link.join("a.png");
        let r = uebernehmen(&wurzel, &q, ueber_link.to_str().unwrap(), Modus::Kopieren, "g", Some("b1"), LAGE).unwrap();
        assert_eq!(r.format, BildFormat::Png);
        assert!(a.exists());
    }

    #[test]
    fn hinzufuegen_geht_ausserhalb_der_quellen_und_ohne_flug_und_kopiert_immer() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        let fremd = t.path().join("Desktop");
        fs::create_dir_all(&fremd).unwrap();
        let a = fremd.join("urlaub.jpg");
        fs::write(&a, jpeg_bytes(640, 480)).unwrap();
        let r = hinzufuegen(&wurzel, a.to_str().unwrap(), "ohne-flug", Some("h1"), PAUSE).unwrap();
        assert_eq!(r.quelle_pfad, "ohne-flug/h1.quelle.jpg");
        assert_eq!(r.modus, Modus::Kopieren);
        assert!(!r.original_entfernt && a.exists(), "das Original bleibt immer");
        assert_eq!(fs::read(wurzel.join("ohne-flug/h1.quelle.jpg")).unwrap(), jpeg_bytes(640, 480));
        // nur JPG/PNG, vollstaendig
        let txt = fremd.join("x.txt");
        fs::write(&txt, png_bytes(5, 5)).unwrap();
        assert!(matches!(hinzufuegen(&wurzel, txt.to_str().unwrap(), "g", None, PAUSE), Err(Fehler::Format(_))));
        let halb = fremd.join("halb.png");
        let voll = png_bytes(5, 5);
        fs::write(&halb, &voll[..voll.len() - 6]).unwrap();
        assert!(matches!(hinzufuegen(&wurzel, halb.to_str().unwrap(), "g", None, PAUSE), Err(Fehler::Unfertig(_))));
        assert!(matches!(hinzufuegen(&wurzel, "relativ.png", "g", None, PAUSE), Err(Fehler::Quelle(_))));
        assert!(matches!(hinzufuegen(&wurzel, a.to_str().unwrap(), "g/..", None, PAUSE), Err(Fehler::Name(_))));
        // Datei aus dem Bilderordner selbst: nein
        let innen = wurzel.join("ohne-flug/h1.quelle.jpg");
        assert!(matches!(hinzufuegen(&wurzel, innen.to_str().unwrap(), "g", None, PAUSE), Err(Fehler::Quelle(_))));
        // gleiche id nicht ueberschreiben
        assert!(matches!(hinzufuegen(&wurzel, a.to_str().unwrap(), "ohne-flug", Some("h1"), PAUSE), Err(Fehler::Name(_))));
    }

    #[test]
    fn quelle_von_hand_kopiert_nur_und_laesst_sich_anhaengen_ohne_doppelte() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        let home = t.path().join("home");
        let ordner = home.join("MeineScreens");
        fs::create_dir_all(&ordner).unwrap();
        let hand = quelle_von_hand(&ordner);
        assert_eq!(hand.art.as_deref(), Some(ART_HAND));
        assert!(hand.aktiv && !hand.original_entfernen, "Vorgabe: nur kopieren");
        let leer = Einstellungen::default();
        let (mit, neu) = quelle_anhaengen(&leer, hand.clone(), &wurzel, Some(&home)).unwrap();
        assert!(neu && mit.quellen == vec![hand.clone()]);
        let (noch, neu) = quelle_anhaengen(&mit, hand.clone(), &wurzel, Some(&home)).unwrap();
        assert!(!neu && noch == mit, "derselbe Ordner noch einmal aendert nichts");
        // unzulaessige Wahl bleibt unzulaessig
        assert!(quelle_anhaengen(&leer, quelle_von_hand(&home), &wurzel, Some(&home)).is_err(), "persoenlicher Ordner");
        assert!(quelle_anhaengen(&leer, quelle_von_hand(&wurzel), &wurzel, Some(&home)).is_err(), "Bilderordner selbst");
        // Uebernahme aus einer Quelle von Hand: wie jede bestaetigte Quelle, Flug bleibt Pflicht
        let a = ordner.join("s.png");
        fs::write(&a, png_bytes(50, 50)).unwrap();
        let kein = Lage { flug_aktiv: false, stabil_pause: PAUSE };
        assert!(matches!(uebernehmen(&wurzel, &mit.quellen, a.to_str().unwrap(), Modus::Kopieren, "g", None, kein), Err(Fehler::KeinFlug(_))));
        assert!(uebernehmen(&wurzel, &mit.quellen, a.to_str().unwrap(), Modus::Kopieren, "g", None, LAGE).is_ok());
        assert!(matches!(uebernehmen(&wurzel, &mit.quellen, a.to_str().unwrap(), Modus::Verschieben, "g", None, LAGE), Err(Fehler::Verboten(_))), "Verschieben nur nach Freigabe");
    }

    #[test]
    fn einstellungen_neu_haengt_den_gewaehlten_ordner_an_und_aendert_sonst_nichts() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        let home = t.path().join("home");
        let (a, b) = (home.join("A"), home.join("B"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        let bisher = Einstellungen { quellen: vec![quelle(&a, true)], tastenkuerzel: "Alt+F9".into(), yoke: None };
        let neu = einstellungen_neu(&bisher, None, None, None, Some(&b), &wurzel, Some(&home)).unwrap();
        assert_eq!(neu.quellen.len(), 2);
        assert_eq!(neu.quellen[0], bisher.quellen[0], "die bisherige Quelle bleibt unveraendert");
        assert_eq!(neu.quellen[1].art.as_deref(), Some(ART_HAND));
        assert!(!neu.quellen[1].original_entfernen);
        assert_eq!(neu.tastenkuerzel, "Alt+F9");
        // ohne alles: unveraendert; Taste allein; Liste ersetzt plus Ordner
        assert_eq!(einstellungen_neu(&bisher, None, None, None, None, &wurzel, Some(&home)).unwrap(), bisher);
        assert_eq!(einstellungen_neu(&bisher, None, Some("Ctrl+F5"), None, None, &wurzel, Some(&home)).unwrap().tastenkuerzel, "Ctrl+F5");
        let ersetzt = einstellungen_neu(&bisher, Some(vec![]), None, None, Some(&b), &wurzel, Some(&home)).unwrap();
        assert_eq!(ersetzt.quellen.len(), 1);
        assert!(einstellungen_neu(&bisher, None, None, None, Some(&home), &wurzel, Some(&home)).is_err());
        assert!(einstellungen_neu(&bisher, None, Some("S"), None, None, &wurzel, Some(&home)).is_err());
        // Yoke-Knopf: fehlt = unveraendert, Some(Some) setzt, Some(None) loescht, ungueltig scheitert
        let y = crate::yoke::YokeTaste { geraet_name: "Yoke".into(), hersteller_id: 1, produkt_id: 2, knopf: 3 };
        let mit = einstellungen_neu(&bisher, None, None, Some(Some(y.clone())), None, &wurzel, Some(&home)).unwrap();
        assert_eq!(mit.yoke, Some(y.clone()));
        assert_eq!(einstellungen_neu(&mit, None, None, None, None, &wurzel, Some(&home)).unwrap().yoke, Some(y.clone()));
        assert_eq!(einstellungen_neu(&mit, None, None, Some(None), None, &wurzel, Some(&home)).unwrap().yoke, None);
        let kaputt = crate::yoke::YokeTaste { knopf: 0, ..y };
        assert!(einstellungen_neu(&bisher, None, None, Some(Some(kaputt)), None, &wurzel, Some(&home)).is_err());
    }

    #[test]
    fn hinzufuegen_mehrere_meldet_jede_datei_einzeln() {
        let t = tempfile::tempdir().unwrap();
        let wurzel = t.path().join("bilder");
        let gut = t.path().join("gut.png");
        fs::write(&gut, png_bytes(9, 9)).unwrap();
        let schlecht = t.path().join("schlecht.txt");
        fs::write(&schlecht, b"x").unwrap();
        let pfade = vec![gut.to_string_lossy().into_owned(), schlecht.to_string_lossy().into_owned()];
        let r = hinzufuegen_mehrere(&wurzel, &pfade, "ohne-flug", PAUSE).unwrap();
        assert!(r[0].1.is_ok() && matches!(r[1].1, Err(Fehler::Format(_))));
        let viele = vec!["/x.png".to_string(); MAX_HINZUFUEGEN + 1];
        assert!(matches!(hinzufuegen_mehrere(&wurzel, &viele, "g", PAUSE), Err(Fehler::Eingabe(_))));
        assert!(matches!(hinzufuegen_mehrere(&wurzel, &pfade, "..", PAUSE), Err(Fehler::Name(_))));
    }

    fn fund_ablegen(wurzel: &Path, id: &str, pfad: &Path, pirep: &str) {
        use crate::eingang::{ablegen, EingangsFund};
        let f = crate::beobachter::Fund {
            pfad: pfad.to_string_lossy().into(),
            dateizeit_ms: Some(1),
            erstellt_ms: None,
            groesse: 10,
            format: BildFormat::Png,
            uebernehmbar: true,
            px_breite: Some(100),
            px_hoehe: Some(50),
            sha256: Some("ab".into()),
            quelle: pfad.parent().unwrap().to_string_lossy().into(),
            art: Some("hand".into()),
            sim_vordergrund: Some(true),
        };
        ablegen(wurzel, &EingangsFund::aus_fund(&f, id.into(), 5, None, serde_json::json!({"pirep_id": pirep, "phase": "Cruise"}))).unwrap();
    }

    #[test]
    fn fund_uebernehmen_auch_nach_flugende_und_der_eintrag_verschwindet() {
        let (_t, wurzel, src, q) = aufbau(true);
        let a = src.join("a.png");
        fs::write(&a, png_bytes(100, 50)).unwrap();
        fund_ablegen(&wurzel, "f1", &a, "P1");
        // kein aktiver Flug mehr noetig: der Fund steht im Eingang
        let r = uebernehmen_fund(&wurzel, &q, "f1", Modus::Kopieren, Some("g"), Some("b1"), PAUSE).unwrap();
        let FundErgebnis::Uebernommen(u) = r else { panic!("Uebernahme erwartet") };
        assert_eq!(u.quelle_pfad, "g/b1.quelle.png");
        assert!(a.exists());
        assert!(crate::eingang::liste(&wurzel).is_empty(), "abgearbeitet = Eintrag entfernt");
        // der Eintrag war einmalig
        assert!(matches!(uebernehmen_fund(&wurzel, &q, "f1", Modus::Kopieren, Some("g"), Some("b2"), PAUSE), Err(Fehler::NichtGefunden(_))));
    }

    #[test]
    fn fund_verwerfen_fasst_die_quelldatei_nie_an() {
        let (_t, wurzel, src, q) = aufbau(true);
        let a = src.join("a.png");
        fs::write(&a, png_bytes(100, 50)).unwrap();
        fund_ablegen(&wurzel, "f1", &a, "P1");
        let r = uebernehmen_fund(&wurzel, &q, "f1", Modus::Verwerfen, None, None, PAUSE).unwrap();
        assert_eq!(r, FundErgebnis::Verworfen { fund_id: "f1".into(), verworfen: true });
        assert!(a.exists(), "Quelldatei bleibt");
        assert!(crate::eingang::liste(&wurzel).is_empty());
        assert!(!wurzel.join("g").exists());
    }

    #[test]
    fn fund_regeln_verschieben_nur_mit_freigabe_gruppe_pflicht_und_flug_im_eintrag() {
        let (_t, wurzel, src, q) = aufbau(false);
        let a = src.join("a.png");
        fs::write(&a, png_bytes(100, 50)).unwrap();
        fund_ablegen(&wurzel, "f1", &a, "P1");
        assert!(matches!(uebernehmen_fund(&wurzel, &q, "f1", Modus::Verschieben, Some("g"), None, PAUSE), Err(Fehler::Verboten(_))));
        assert!(matches!(uebernehmen_fund(&wurzel, &q, "f1", Modus::Kopieren, None, None, PAUSE), Err(Fehler::Eingabe(_))));
        assert_eq!(crate::eingang::liste(&wurzel).len(), 1, "Fehler lassen den Eintrag stehen");
        assert!(a.exists());
        // ein Eintrag ohne Flug (von Hand verfaelscht) wird nicht uebernommen
        let mut f = crate::eingang::lesen(&wurzel, "f1").unwrap();
        f.meta_basis = serde_json::json!({});
        fs::write(wurzel.join(".eingang/f1.json"), serde_json::to_vec(&f).unwrap()).unwrap();
        assert!(matches!(uebernehmen_fund(&wurzel, &q, "f1", Modus::Kopieren, Some("g"), None, PAUSE), Err(Fehler::KeinFlug(_))));
        // Datei ausserhalb einer aktiven Quelle: weiter abgelehnt
        let mut q2 = q.clone();
        q2[0].aktiv = false;
        let mut f2 = crate::eingang::lesen(&wurzel, "f1").unwrap();
        f2.meta_basis = serde_json::json!({"pirep_id": "P"});
        fs::write(wurzel.join(".eingang/f1.json"), serde_json::to_vec(&f2).unwrap()).unwrap();
        assert!(matches!(uebernehmen_fund(&wurzel, &q2, "f1", Modus::Kopieren, Some("g"), None, PAUSE), Err(Fehler::Quelle(_))));
    }

    #[test]
    fn aufnahme_fund_wandert_in_die_gruppe_und_verwerfen_raeumt_die_eigene_kopie() {
        use crate::eingang::{ablegen, EingangsFund};
        let t = tempfile::tempdir().unwrap();
        let w = t.path().join("bilder");
        for id in ["a1", "a2"] {
            crate::speicher::quelle_schreiben(&w, "eingang", id, QuellFormat::Png, &png_bytes(30, 20)).unwrap();
            ablegen(&w, &EingangsFund::aufnahme(id.into(), 1, format!("eingang/{id}.quelle.png"), 50, 30, 20, None, serde_json::json!({"pirep_id": "P1"}))).unwrap();
        }
        let r = uebernehmen_fund(&w, &[], "a1", Modus::Kopieren, Some("P1"), None, PAUSE).unwrap();
        let FundErgebnis::Uebernommen(u) = r else { panic!() };
        assert_eq!((u.quelle_pfad.as_str(), u.id.as_str()), ("P1/a1.quelle.png", "a1"));
        assert!(w.join("P1/a1.quelle.png").exists() && !w.join("eingang/a1.quelle.png").exists());
        uebernehmen_fund(&w, &[], "a2", Modus::Verwerfen, None, None, PAUSE).unwrap();
        assert!(!w.join("eingang").exists(), "keine Waise der Aufnahme");
        assert!(crate::eingang::liste(&w).is_empty());
    }
}
