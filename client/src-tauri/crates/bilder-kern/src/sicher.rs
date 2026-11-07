//! Dateien oeffnen, ohne einem Link zu folgen (Befund 3).
//!
//! Grundsatz: nie einen Pfad pruefen und spaeter erneut per Pfad oeffnen. Hier
//! wird jeder Pfadteil ab der Wurzel mit `symlink_metadata` geprueft, die Datei
//! dann mit `O_NOFOLLOW` (Unix) bzw. `FILE_FLAG_OPEN_REPARSE_POINT` (Windows)
//! geoeffnet, und danach gilt nur noch das **geoeffnete Handle**: dessen
//! Metadaten muessen eine echte Datei zeigen und dieselbe Datei sein, die die
//! Pfadpruefung gesehen hat.

use std::ffi::OsString;
use std::fs::{self, File, Metadata, OpenOptions};
use std::path::{Component, Path, PathBuf};

use crate::Fehler;

#[cfg(windows)]
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

/// Offnet zum Lesen und folgt am letzten Pfadteil keinem Link.
pub fn lesend_ohne_folgen(pfad: &Path) -> std::io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    o.open(pfad)
}

/// Neue Datei, die nie eine vorhandene (auch keinen Link) ueberschreibt.
pub fn neu_anlegen(pfad: &Path) -> std::io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(pfad)
}

/// `pfad` ist ein echter Ordner, kein Link (auch kein Windows-Junction).
pub fn ist_echter_ordner(pfad: &Path) -> bool {
    fs::symlink_metadata(pfad).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
}

/// Ordner, der angelegt werden darf, falls er fehlt, aber nie ein Link sein darf.
pub fn echten_ordner_sichern(pfad: &Path, was: &str) -> Result<(), Fehler> {
    match fs::symlink_metadata(pfad) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(Fehler::Pfad(format!("{was} ist kein echter Ordner (Link oder Datei)"))),
        Err(e) => Err(e.into()),
    }
}

/// Identitaet einer Datei auf dem Datentraeger: Unix `dev`/`ino`, Windows
/// Volume-Seriennummer und File-Index. Laenge und Aenderungszeit genuegen nicht
/// (eine ersetzte Datei kann beides teilen).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Kennung {
    datentraeger: u64,
    datei: u64,
}

impl Kennung {
    pub(crate) fn neu(datentraeger: u64, datei: u64) -> Self {
        Self { datentraeger, datei }
    }

    /// Nur zwei bekannte, gleiche Kennungen gelten als dieselbe Datei; ist eine
    /// unbekannt, ist es nie dieselbe (dann wird nichts geloescht).
    pub(crate) fn gleich(a: Option<Kennung>, b: Option<Kennung>) -> bool {
        matches!((a, b), (Some(x), Some(y)) if x == y)
    }

    /// Kennung des geoeffneten Handles.
    pub(crate) fn von_handle(f: &File) -> Option<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            f.metadata().ok().map(|m| Self::neu(m.dev(), m.ino()))
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION};
            // SAFETY: `f` haelt ein gueltiges Handle; die Struktur wird vollstaendig gefuellt oder verworfen.
            let mut i: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { GetFileInformationByHandle(f.as_raw_handle() as _, &mut i) } == 0 {
                return None;
            }
            Some(Self::neu(u64::from(i.dwVolumeSerialNumber), (u64::from(i.nFileIndexHigh) << 32) | u64::from(i.nFileIndexLow)))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = f;
            None
        }
    }

    /// Kennung dessen, worauf `pfad` jetzt zeigt (ohne einem Link zu folgen).
    pub(crate) fn von_pfad(pfad: &Path) -> Option<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let m = fs::symlink_metadata(pfad).ok()?;
            if m.file_type().is_symlink() {
                return None;
            }
            Some(Self::neu(m.dev(), m.ino()))
        }
        #[cfg(not(unix))]
        {
            if fs::symlink_metadata(pfad).ok()?.file_type().is_symlink() {
                return None;
            }
            Self::von_handle(&lesend_ohne_folgen(pfad).ok()?)
        }
    }
}

/// Der Reststueck-Pfad unter `basis` besteht nur aus normalen Namen.
fn teile(rel: &Path) -> Result<Vec<OsString>, Fehler> {
    let mut v = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(n) => v.push(n.to_owned()),
            _ => return Err(Fehler::Pfad(format!("Pfad ungueltig: {}", rel.display()))),
        }
    }
    if v.is_empty() {
        return Err(Fehler::Pfad("Pfad ungueltig: leer".into()));
    }
    Ok(v)
}

/// Oeffnet `basis/rel` sicher: jeder Teil ab `basis` ist kein Link, die Datei
/// wird ohne Linkfolgen geoeffnet, das Handle ist eine echte Datei und dieselbe,
/// die die Pruefung sah. `basis` selbst gilt als bereits aufgeloest (kanonisch
/// oder vom Aufrufer als echter Ordner geprueft).
pub fn datei_oeffnen(basis: &Path, rel: &Path) -> Result<(File, Metadata), Fehler> {
    datei_oeffnen_mit(basis, rel, &|| {})
}

/// Wie [`datei_oeffnen`]; `zwischen` laeuft nach der Pfadpruefung, vor dem
/// Oeffnen. Zweck: Tests tauschen dort den Pfad aus.
pub fn datei_oeffnen_mit(basis: &Path, rel: &Path, zwischen: &dyn Fn()) -> Result<(File, Metadata), Fehler> {
    let namen = teile(rel)?;
    let mut p: PathBuf = basis.to_path_buf();
    let mut letzte: Option<Metadata> = None;
    for (i, n) in namen.iter().enumerate() {
        p.push(n);
        let m = fs::symlink_metadata(&p).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Fehler::NichtGefunden(format!("{} nicht gefunden", rel.display())),
            _ => Fehler::from(e),
        })?;
        if m.file_type().is_symlink() {
            return Err(Fehler::Pfad(format!("Link im Pfad nicht erlaubt: {}", rel.display())));
        }
        let ende = i + 1 == namen.len();
        if ende {
            if !m.is_file() {
                return Err(Fehler::NichtGefunden(format!("{} ist keine Datei", rel.display())));
            }
            letzte = Some(m);
        } else if !m.is_dir() {
            return Err(Fehler::Pfad(format!("Pfad ungueltig: {}", rel.display())));
        }
    }
    letzte.ok_or_else(|| Fehler::Pfad("Pfad ungueltig".into()))?;
    let vorher = Kennung::von_pfad(&p);
    zwischen();
    let f = lesend_ohne_folgen(&p).map_err(|e| {
        if fs::symlink_metadata(&p).is_ok_and(|m| m.file_type().is_symlink()) {
            Fehler::Pfad(format!("Link im Pfad nicht erlaubt: {}", rel.display()))
        } else {
            Fehler::from(e)
        }
    })?;
    let md = f.metadata()?;
    if md.file_type().is_symlink() || !md.is_file() || !Kennung::gleich(vorher, Kennung::von_handle(&f)) {
        return Err(Fehler::Pfad(format!("Datei hat sich waehrend der Pruefung veraendert: {}", rel.display())));
    }
    Ok((f, md))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn link_am_ende_und_mitten_im_pfad_wird_abgelehnt() {
        let t = tempfile::tempdir().unwrap();
        let basis = t.path().join("b");
        fs::create_dir_all(basis.join("g")).unwrap();
        let aussen = t.path().join("aussen");
        fs::create_dir_all(&aussen).unwrap();
        fs::write(aussen.join("x.jpg"), b"geheim").unwrap();
        fs::write(basis.join("g/echt.jpg"), b"ok").unwrap();
        std::os::unix::fs::symlink(aussen.join("x.jpg"), basis.join("g/link.jpg")).unwrap();
        std::os::unix::fs::symlink(&aussen, basis.join("h")).unwrap();
        assert_eq!(datei_oeffnen(&basis, Path::new("g/echt.jpg")).map(|(_, m)| m.len()).unwrap(), 2);
        assert!(matches!(datei_oeffnen(&basis, Path::new("g/link.jpg")), Err(Fehler::Pfad(_))));
        assert!(matches!(datei_oeffnen(&basis, Path::new("h/x.jpg")), Err(Fehler::Pfad(_))));
        assert!(matches!(datei_oeffnen(&basis, Path::new("g/fehlt.jpg")), Err(Fehler::NichtGefunden(_))));
        assert!(matches!(datei_oeffnen(&basis, Path::new("../aussen/x.jpg")), Err(Fehler::Pfad(_))));
        assert!(matches!(datei_oeffnen(&basis, Path::new("g")), Err(Fehler::NichtGefunden(_))));
    }

    /// Der Pfad wird zwischen Pruefung und Oeffnen gegen einen Link getauscht:
    /// das Oeffnen ohne Linkfolgen faengt es ab.
    #[cfg(unix)]
    #[test]
    fn ausgetauschter_pfad_zwischen_pruefung_und_oeffnen_wird_abgefangen() {
        let t = tempfile::tempdir().unwrap();
        let basis = t.path().join("b");
        fs::create_dir_all(&basis).unwrap();
        let geheim = t.path().join("geheim.jpg");
        fs::write(&geheim, b"geheim").unwrap();
        fs::write(basis.join("a.jpg"), b"harmlos").unwrap();
        let ziel = basis.join("a.jpg");
        let tausch = || {
            fs::remove_file(&ziel).unwrap();
            std::os::unix::fs::symlink(&geheim, &ziel).unwrap();
        };
        let r = datei_oeffnen_mit(&basis, Path::new("a.jpg"), &tausch);
        assert!(matches!(r, Err(Fehler::Pfad(_))), "{r:?}");
    }

    #[cfg(unix)]
    #[test]
    fn oeffnen_ohne_folgen_scheitert_an_einem_link() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("echt"), b"x").unwrap();
        std::os::unix::fs::symlink(t.path().join("echt"), t.path().join("link")).unwrap();
        assert!(lesend_ohne_folgen(&t.path().join("echt")).is_ok());
        assert!(lesend_ohne_folgen(&t.path().join("link")).is_err(), "ein Link wird nicht geoeffnet");
    }

    /// Fund 1: gleiche Laenge und gleiche Aenderungszeit machen eine ersetzte
    /// Datei nicht zur selben (unter Windows war das die ganze Pruefung).
    #[test]
    fn ersetzte_datei_mit_gleicher_laenge_und_zeit_ist_nicht_dieselbe() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("a.png");
        fs::write(&p, b"AAAA").unwrap();
        let zeit = fs::metadata(&p).unwrap().modified().unwrap();
        let geoeffnet = lesend_ohne_folgen(&p).unwrap();
        let vorher = Kennung::von_handle(&geoeffnet);
        assert!(Kennung::gleich(vorher, Kennung::von_pfad(&p)), "unveraendert = dieselbe");
        // Ersetzen: andere Datei, gleiche Laenge, gleiche Zeit.
        let neu = t.path().join("neu.tmp");
        fs::write(&neu, b"BBBB").unwrap();
        fs::File::options().write(true).open(&neu).unwrap().set_modified(zeit).unwrap();
        drop(geoeffnet);
        fs::remove_file(&p).unwrap();
        fs::rename(&neu, &p).unwrap();
        let jetzt = fs::metadata(&p).unwrap();
        assert_eq!((jetzt.len(), jetzt.modified().unwrap()), (4, zeit), "Laenge und Zeit stimmen ueberein");
        assert!(!Kennung::gleich(vorher, Kennung::von_pfad(&p)), "trotzdem eine andere Datei");
    }

    #[test]
    fn kennungen_sind_nur_bei_voller_uebereinstimmung_gleich() {
        let a = Some(Kennung::neu(7, 100));
        assert!(Kennung::gleich(a, Some(Kennung::neu(7, 100))));
        assert!(!Kennung::gleich(a, Some(Kennung::neu(7, 101))), "anderer File-Index");
        assert!(!Kennung::gleich(a, Some(Kennung::neu(8, 100))), "anderes Volume");
        assert!(!Kennung::gleich(a, None), "unbekannt = nie gleich");
        assert!(!Kennung::gleich(None, None), "zweimal unbekannt ist auch nicht gleich");
    }

    #[test]
    fn echter_ordner_und_neu_anlegen() {
        let t = tempfile::tempdir().unwrap();
        assert!(ist_echter_ordner(t.path()));
        assert!(!ist_echter_ordner(&t.path().join("nix")));
        let f = t.path().join("f");
        fs::write(&f, b"x").unwrap();
        assert!(!ist_echter_ordner(&f));
        assert!(neu_anlegen(&f).is_err(), "ueberschreibt nie");
        assert!(neu_anlegen(&t.path().join("neu")).is_ok());
    }
}
