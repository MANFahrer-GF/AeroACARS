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
//!   (`original_entfernen`); sonst bleibt das Original unangetastet.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::namen::{name_pruefen, QuellFormat, Teil};
use crate::speicher::{ms_seit_1970, MAX_DATEI};
use crate::Fehler;

pub const VORGABE_TASTENKUERZEL: &str = "Ctrl+Shift+S";
const MAX_QUELLEN: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Quelle {
    pub pfad: String,
    /// Pausiert = wird nicht beobachtet und nichts daraus uebernommen.
    #[serde(default = "wahr")]
    pub aktiv: bool,
    /// Nur wenn der Pilot es fuer diese Quelle erlaubt hat, darf `verschieben` das Original entfernen.
    #[serde(default)]
    pub original_entfernen: bool,
    /// Freie Bezeichnung des Pakets (z. B. `xplane_standard`), unveraendert durchgereicht.
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
}

fn vorgabe_taste() -> String {
    VORGABE_TASTENKUERZEL.to_string()
}

impl Default for Einstellungen {
    fn default() -> Self {
        Self { quellen: Vec::new(), tastenkuerzel: vorgabe_taste() }
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

/// Prueft eine neue Einstellung, bevor sie gespeichert wird.
pub fn einstellungen_pruefen(e: &Einstellungen, bilder_wurzel: &Path, home: Option<&Path>) -> Result<(), Fehler> {
    crate::tastenkuerzel::lesen(&e.tastenkuerzel)?;
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

/// Die aktive Quelle, in der `datei` wirklich liegt.
pub fn quelle_zu_datei<'a>(datei: &Path, quellen: &'a [Quelle]) -> Result<&'a Quelle, Fehler> {
    let k = fs::canonicalize(datei).map_err(|_| Fehler::NichtGefunden(format!("Datei nicht gefunden: {}", datei.display())))?;
    for q in quellen.iter().filter(|q| q.aktiv) {
        if let Ok(qk) = fs::canonicalize(&q.pfad) {
            if k.starts_with(&qk) && k != qk {
                return Ok(q);
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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

/// Liest Kopf und Ende einer Datei und prueft, ob sie vollstaendig ist.
/// Eine Datei, die ein anderer Prozess noch exklusiv haelt, lässt sich nicht
/// oeffnen und zaehlt als unfertig.
pub fn datei_vollstaendig(pfad: &Path) -> Result<(BildFormat, u64), Fehler> {
    let format = BildFormat::aus_endung(pfad).ok_or_else(|| Fehler::Format("Nur PNG- und JPEG-Dateien".into()))?;
    if format == BildFormat::Jxr {
        return Err(Fehler::Format("HDR-Aufnahme (JXR) wird nicht uebernommen".into()));
    }
    let mut f = fs::File::open(pfad).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Fehler::NichtGefunden(e.to_string()),
        _ => Fehler::Unfertig(format!("Datei nicht lesbar (noch in Benutzung?): {e}")),
    })?;
    let len = f.metadata()?.len();
    if len < 24 {
        return Err(Fehler::Unfertig("Datei ist (noch) zu klein".into()));
    }
    if len > MAX_DATEI {
        return Err(Fehler::Gross("Datei ist zu gross".into()));
    }
    let mut kopf = [0u8; 16];
    f.read_exact(&mut kopf)?;
    let ende_len = len.min(64);
    f.seek(SeekFrom::Start(len - ende_len))?;
    let mut ende = vec![0u8; ende_len as usize];
    f.read_exact(&mut ende)?;
    inhalt_vollstaendig(format, &kopf, &ende)?;
    Ok((format, len))
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

fn kopieren_atomar(von: &Path, dir: &Path, ziel: &Path, erwartet: u64) -> Result<(), Fehler> {
    fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let r = (|| -> Result<(), Fehler> {
        let mut src = fs::File::open(von)?;
        let mut dst = fs::File::create(&tmp)?;
        let n = std::io::copy(&mut src, &mut dst)?;
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

/// Nimmt eine Datei aus einer bestaetigten Quelle in den Bilderordner. Das
/// Paket liest sie von dort (`bild_lesen`), macht das JPEG und schreibt es mit
/// `bild_schreiben` (`quelle_entfernen`); bis dahin ist nichts verloren, auch
/// wenn das Programm abbricht.
pub fn uebernehmen(
    wurzel: &Path,
    quellen: &[Quelle],
    pfad: &str,
    modus: Modus,
    gruppe: &str,
    id: Option<&str>,
    stabil_pause: Duration,
) -> Result<Uebernahme, Fehler> {
    name_pruefen("Gruppe", gruppe)?;
    let id = match id {
        Some(i) => {
            name_pruefen("Bild-id", i)?;
            i.to_string()
        }
        None => uuid::Uuid::new_v4().to_string(),
    };
    let p = Path::new(pfad);
    if !p.is_absolute() {
        return Err(Fehler::Quelle("Absoluter Dateipfad erwartet".into()));
    }
    let quelle = quelle_zu_datei(p, quellen)?;
    if modus == Modus::Verschieben && !quelle.original_entfernen {
        return Err(Fehler::Verboten("Aus dieser Quelle darf nicht verschoben werden, nur kopiert".into()));
    }
    let echt = fs::canonicalize(p)?;
    let (format, groesse) = datei_vollstaendig(&echt)?;
    groesse_stabil(&echt, stabil_pause)?;
    let md = fs::metadata(&echt)?;
    if md.len() != groesse {
        return Err(Fehler::Unfertig("Datei wird noch geschrieben".into()));
    }
    let dateizeit_ms = md.modified().ok().and_then(ms_seit_1970);
    let qf = format.quell_format().ok_or_else(|| Fehler::Format("HDR-Aufnahme (JXR) wird nicht uebernommen".into()))?;
    let dir = wurzel.join(gruppe);
    let ziel = dir.join(Teil::Quelle(qf).dateiname(&id));
    if ziel.exists() {
        return Err(Fehler::Name(format!("Bild-id {id} hat in {gruppe} schon eine Eingangskopie")));
    }
    let mut original_entfernt = false;
    match modus {
        Modus::Kopieren => kopieren_atomar(&echt, &dir, &ziel, groesse)?,
        Modus::Verschieben => {
            fs::create_dir_all(&dir)?;
            if fs::rename(&echt, &ziel).is_ok() {
                original_entfernt = true;
            } else {
                // anderes Laufwerk oder gesperrt: kopieren, pruefen, erst dann entfernen
                kopieren_atomar(&echt, &dir, &ziel, groesse)?;
                if let Err(e) = fs::remove_file(&echt) {
                    let _ = fs::remove_file(&ziel);
                    return Err(Fehler::Io(format!("Original konnte nicht entfernt werden, nichts uebernommen: {e}")));
                }
                original_entfernt = true;
            }
        }
    }
    Ok(Uebernahme {
        quelle_pfad: crate::namen::rel_pfad(gruppe, &Teil::Quelle(qf).dateiname(&id)),
        id,
        gruppe: gruppe.to_string(),
        format,
        groesse,
        dateizeit_ms,
        modus,
        original_entfernt,
    })
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
        let r = uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Kopieren, "g", Some("b1"), PAUSE).unwrap();
        assert!(a.exists(), "Kopieren laesst das Original");
        assert!(!r.original_entfernt);
        assert_eq!(r.quelle_pfad, "g/b1.quelle.png");
        assert_eq!(fs::read(wurzel.join("g/b1.quelle.png")).unwrap(), png_bytes(100, 50));

        let b = src.join("unter").join("b.jpg");
        fs::write(&b, jpeg_bytes(640, 480)).unwrap();
        let r = uebernehmen(&wurzel, &q, b.to_str().unwrap(), Modus::Verschieben, "g", Some("b2"), PAUSE).unwrap();
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
        let e = uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Verschieben, "g", None, PAUSE).unwrap_err();
        assert!(matches!(e, Fehler::Verboten(_)));
        assert!(a.exists());
        assert!(!wurzel.exists() || crate::speicher::liste(&wurzel).eintraege.is_empty());
    }

    #[test]
    fn datei_ausserhalb_jeder_quelle_wird_abgelehnt() {
        let (t, wurzel, src, q) = aufbau(true);
        let fremd = t.path().join("fremd.png");
        fs::write(&fremd, png_bytes(10, 10)).unwrap();
        let e = uebernehmen(&wurzel, &q, fremd.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE).unwrap_err();
        assert!(matches!(e, Fehler::Quelle(_)));
        // „..“ aus der Quelle heraus
        let ausbruch = format!("{}/../fremd.png", src.display());
        assert!(matches!(uebernehmen(&wurzel, &q, &ausbruch, Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Quelle(_))));
        // Ordner mit aehnlichem Namensanfang ist keine Unterordner-Beziehung
        let nachbar = t.path().join("screens-privat");
        fs::create_dir_all(&nachbar).unwrap();
        fs::write(nachbar.join("x.png"), png_bytes(10, 10)).unwrap();
        let p = nachbar.join("x.png");
        assert!(matches!(uebernehmen(&wurzel, &q, p.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Quelle(_))));
        // relativ
        assert!(matches!(uebernehmen(&wurzel, &q, "a.png", Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Quelle(_))));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_aus_der_quelle_heraus_hilft_nicht() {
        let (t, wurzel, src, q) = aufbau(true);
        let fremd = t.path().join("privat.png");
        fs::write(&fremd, png_bytes(10, 10)).unwrap();
        std::os::unix::fs::symlink(&fremd, src.join("link.png")).unwrap();
        let p = src.join("link.png");
        assert!(matches!(uebernehmen(&wurzel, &q, p.to_str().unwrap(), Modus::Verschieben, "g", None, PAUSE), Err(Fehler::Quelle(_))));
        assert!(fremd.exists());
    }

    #[test]
    fn pausierte_quelle_zaehlt_nicht() {
        let (_t, wurzel, src, mut q) = aufbau(true);
        q[0].aktiv = false;
        let a = src.join("a.png");
        fs::write(&a, png_bytes(10, 10)).unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, a.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Quelle(_))));
    }

    #[test]
    fn nur_vollstaendig_geschriebene_jpg_und_png() {
        let (_t, wurzel, src, q) = aufbau(true);
        let voll = png_bytes(10, 10);
        let abgeschnitten = src.join("halb.png");
        fs::write(&abgeschnitten, &voll[..voll.len() - 6]).unwrap();
        let p = abgeschnitten.to_str().unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, p, Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Unfertig(_))));
        let jpg = jpeg_bytes(10, 10);
        let halb_jpg = src.join("halb.jpg");
        fs::write(&halb_jpg, &jpg[..jpg.len() - 5]).unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, halb_jpg.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Unfertig(_))));
        // Nullbytes am Ende eines fertigen JPEG sind erlaubt
        let mit_nullen = src.join("null.jpg");
        let mut j = jpg.clone();
        j.extend_from_slice(&[0; 8]);
        fs::write(&mit_nullen, &j).unwrap();
        assert!(uebernehmen(&wurzel, &q, mit_nullen.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE).is_ok());
        // Endung luegt
        let falsch = src.join("falsch.png");
        fs::write(&falsch, jpeg_bytes(10, 10)).unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, falsch.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Format(_))));
        // fremde Formate, HDR, zu klein
        for name in ["x.gif", "x.jxr", "x.txt", "x"] {
            let f = src.join(name);
            fs::write(&f, png_bytes(10, 10)).unwrap();
            assert!(matches!(uebernehmen(&wurzel, &q, f.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Format(_))), "{name}");
        }
        let klein = src.join("klein.png");
        fs::write(&klein, b"\x89PNG").unwrap();
        assert!(matches!(uebernehmen(&wurzel, &q, klein.to_str().unwrap(), Modus::Kopieren, "g", None, PAUSE), Err(Fehler::Unfertig(_))));
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
        let e = Einstellungen { quellen: vec![quelle(t.path(), true)], tastenkuerzel: "Alt+F9".into() };
        einstellungen_schreiben(&d, &e).unwrap();
        assert_eq!(einstellungen_lesen(&d), e);
        fs::write(&d, b"{kaputt").unwrap();
        assert_eq!(einstellungen_lesen(&d), Einstellungen::default());
        assert_eq!(Einstellungen::default().tastenkuerzel, "Ctrl+Shift+S");
    }
}
