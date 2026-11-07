//! Ordnerbeobachtung der bestaetigten Quellen (P-11.30), Ereignis `bild_quelle_neu`.
//!
//! Der Kern fragt die Ordner im Takt ab (statt Dateisystem-Ereignisse) — aus
//! gutem Grund:
//! * Ein Ordner darf noch fehlen (X-Plane legt `Output/screenshots` erst beim
//!   ersten Bild an), und ein Laufwerk darf spaeter wiederkommen.
//! * Das Verhalten ist auf macOS, Windows und OneDrive-Ordnern gleich und ohne
//!   neue Abhaengigkeit pruefbar.
//! * Eine Datei zaehlt erst, wenn sie ueber zwei Takte unveraendert bleibt **und**
//!   ihr Inhalt abgeschlossen ist (PNG-Endchunk, JPEG-EOI). „Vollstaendig
//!   geschrieben“ braucht diese Wartezeit ohnehin.
//!
//! Kosten: Ein Ordner wird nur dann neu gelesen, wenn sich seine Aenderungszeit
//! geaendert hat oder frisch ist (< 6 s, wegen grober Zeitaufloesung mancher
//! Dateisysteme); die Unterordner-Liste bleibt zwischengespeichert. Ein grosser
//! Bilder-Ordner kostet im Takt deshalb ein `stat` je Ordner.
//!
//! Was beim Aufsetzen schon im Ordner liegt, ist **Altbestand**: es wird nie von
//! selbst gemeldet oder angefasst, nur als Liste angeboten.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::quellen::{abmessungen, datei_vollstaendig, BildFormat, Quelle};
use crate::speicher::{ms_seit_1970, MAX_DATEI};
use crate::Fehler;

/// Tiefste Ebene unterhalb der Quelle, die beobachtet wird (NVIDIA: Stamm/Spiel/Datei).
const MAX_TIEFE: usize = 4;
const MAX_ALTBESTAND: usize = 5000;
const FRISCH: Duration = Duration::from_secs(6);
/// Eine Datei, die so lange nicht abgeschlossen wird, gilt als Fehlschrift und wird fallen gelassen.
const AUFGEBEN_NACH_MS: u64 = 60_000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Fund {
    pub pfad: String,
    /// Aenderungszeit der Datei (ms seit 1970, UTC).
    pub dateizeit_ms: Option<i64>,
    pub erstellt_ms: Option<i64>,
    pub groesse: u64,
    pub format: BildFormat,
    /// Die Datei ist JPG/PNG und abgeschlossen. `false` bei `.jxr` (HDR): wird gemeldet, nie uebernommen.
    pub uebernehmbar: bool,
    pub breite: Option<u32>,
    pub hoehe: Option<u32>,
    /// SHA-256 der Originaldatei, fuer die Duplikaterkennung des Pakets.
    pub sha256: Option<String>,
    /// Pfad der bestaetigten Quelle, in der sie liegt.
    pub quelle: String,
    /// Stand ein Simulatorfenster im Vordergrund, als die Datei auftauchte (oder im Takt davor)?
    /// `None`, wo das System es nicht ermitteln kann.
    pub sim_vordergrund: Option<bool>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AltEintrag {
    pub pfad: String,
    pub dateizeit_ms: Option<i64>,
    pub groesse: u64,
    pub format: BildFormat,
    pub quelle: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct QuellenStatus {
    pub pfad: String,
    pub aktiv: bool,
    pub original_entfernen: bool,
    pub art: Option<String>,
    /// Ordner wird gerade abgefragt (aktiv und erreichbar).
    pub beobachtet: bool,
    /// `nicht_vorhanden` oder `zugriff_verweigert`.
    pub fehler: Option<String>,
    pub altbestand: usize,
}

pub fn oder(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), _) | (_, Some(false)) => Some(false),
        _ => None,
    }
}

struct VerzStand {
    mtime: Option<SystemTime>,
    unterordner: Vec<PathBuf>,
}

struct Wartend {
    groesse: u64,
    zeit: Option<SystemTime>,
    gleich: u32,
    /// Im selben Takt entdeckt: erst der naechste Takt vergleicht.
    gerade_entdeckt: bool,
    seit_ms: u64,
    vordergrund: Option<bool>,
    format: BildFormat,
}

struct Stand {
    quelle: Quelle,
    verz: HashMap<PathBuf, VerzStand>,
    bekannt: HashSet<PathBuf>,
    wartend: HashMap<PathBuf, Wartend>,
    alt: Vec<AltEintrag>,
    fehler: Option<String>,
}

fn bild_endung(p: &Path) -> Option<BildFormat> {
    let name = p.file_name()?.to_str()?;
    if name.starts_with('.') {
        return None;
    }
    BildFormat::aus_endung(p)
}

fn fehlertext(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => "zugriff_verweigert".into(),
        _ => "nicht_vorhanden".into(),
    }
}

impl Stand {
    fn neu(quelle: Quelle) -> Self {
        Self { quelle, verz: HashMap::new(), bekannt: HashSet::new(), wartend: HashMap::new(), alt: Vec::new(), fehler: None }
    }

    fn wurzel(&self) -> PathBuf {
        PathBuf::from(&self.quelle.pfad)
    }

    fn scannen(&mut self, baseline: bool, jetzt: SystemTime, jetzt_ms: u64, vordergrund: Option<bool>) {
        let wurzel = self.wurzel();
        match fs::metadata(&wurzel) {
            Err(e) => {
                self.fehler = Some(fehlertext(&e));
                self.verz.clear();
                return;
            }
            Ok(m) if !m.is_dir() => {
                self.fehler = Some("nicht_vorhanden".into());
                return;
            }
            Ok(_) => self.fehler = None,
        }
        let mut alt: Vec<AltEintrag> = Vec::new();
        self.scanne_dir(&wurzel, 0, baseline, jetzt, jetzt_ms, vordergrund, &mut alt);
        if baseline {
            alt.sort_by(|a, b| b.dateizeit_ms.cmp(&a.dateizeit_ms).then_with(|| a.pfad.cmp(&b.pfad)));
            alt.truncate(MAX_ALTBESTAND);
            self.alt = alt;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn scanne_dir(&mut self, dir: &Path, tiefe: usize, baseline: bool, jetzt: SystemTime, jetzt_ms: u64, vg: Option<bool>, alt: &mut Vec<AltEintrag>) {
        let mtime = fs::metadata(dir).and_then(|m| m.modified()).ok();
        let frisch = mtime.is_none_or(|t| jetzt.duration_since(t).map_or(true, |d| d < FRISCH));
        if !baseline && !frisch {
            if let Some(v) = self.verz.get(dir) {
                if v.mtime == mtime {
                    for k in v.unterordner.clone() {
                        if tiefe < MAX_TIEFE {
                            self.scanne_dir(&k, tiefe + 1, baseline, jetzt, jetzt_ms, vg, alt);
                        }
                    }
                    return;
                }
            }
        }
        let rd = match fs::read_dir(dir) {
            Ok(r) => r,
            Err(e) => {
                if tiefe == 0 {
                    self.fehler = Some(fehlertext(&e));
                }
                return;
            }
        };
        let mut unter: Vec<PathBuf> = Vec::new();
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() && !ft.is_symlink() {
                unter.push(p);
            } else if ft.is_file() {
                let Some(format) = bild_endung(&p) else { continue };
                if self.bekannt.contains(&p) || self.wartend.contains_key(&p) {
                    continue;
                }
                let Ok(md) = e.metadata() else { continue };
                if baseline {
                    self.bekannt.insert(p.clone());
                    alt.push(AltEintrag {
                        pfad: p.to_string_lossy().into_owned(),
                        dateizeit_ms: md.modified().ok().and_then(ms_seit_1970),
                        groesse: md.len(),
                        format,
                        quelle: self.quelle.pfad.clone(),
                    });
                } else {
                    self.wartend.insert(
                        p,
                        Wartend { groesse: md.len(), zeit: md.modified().ok(), gleich: 0, gerade_entdeckt: true, seit_ms: jetzt_ms, vordergrund: vg, format },
                    );
                }
            }
        }
        unter.sort();
        self.verz.insert(dir.to_path_buf(), VerzStand { mtime, unterordner: unter.clone() });
        if tiefe < MAX_TIEFE {
            for k in unter {
                self.scanne_dir(&k, tiefe + 1, baseline, jetzt, jetzt_ms, vg, alt);
            }
        }
    }

    fn wartende_pruefen(&mut self, jetzt_ms: u64, funde: &mut Vec<Fund>) {
        let pfade: Vec<PathBuf> = self.wartend.keys().cloned().collect();
        for p in pfade {
            let Some(w) = self.wartend.get_mut(&p) else { continue };
            let Ok(md) = fs::metadata(&p) else {
                self.wartend.remove(&p); // weg, bevor sie fertig war
                continue;
            };
            if std::mem::take(&mut w.gerade_entdeckt) {
                continue;
            }
            if md.len() == w.groesse && md.modified().ok() == w.zeit {
                w.gleich += 1;
            } else {
                w.groesse = md.len();
                w.zeit = md.modified().ok();
                w.gleich = 0;
            }
            if w.gleich < 1 {
                continue;
            }
            let format = w.format;
            let vg = w.vordergrund;
            let seit = w.seit_ms;
            let fund = match format {
                BildFormat::Jxr => Some(Fund {
                    pfad: p.to_string_lossy().into_owned(),
                    dateizeit_ms: md.modified().ok().and_then(ms_seit_1970),
                    erstellt_ms: md.created().ok().and_then(ms_seit_1970),
                    groesse: md.len(),
                    format,
                    uebernehmbar: false,
                    breite: None,
                    hoehe: None,
                    sha256: None,
                    quelle: self.quelle.pfad.clone(),
                    sim_vordergrund: vg,
                }),
                _ => match datei_vollstaendig(&p) {
                    Ok((_, groesse)) => Self::fund_lesen(&p, format, groesse, &md, &self.quelle.pfad, vg),
                    Err(Fehler::Unfertig(_)) if jetzt_ms.saturating_sub(seit) < AUFGEBEN_NACH_MS => continue,
                    Err(_) => None, // kein gueltiges Bild: nicht melden
                },
            };
            self.wartend.remove(&p);
            self.bekannt.insert(p);
            if let Some(f) = fund {
                funde.push(f);
            }
        }
    }

    fn fund_lesen(p: &Path, format: BildFormat, groesse: u64, md: &fs::Metadata, quelle: &str, vg: Option<bool>) -> Option<Fund> {
        if groesse > MAX_DATEI {
            return None;
        }
        let mut bytes = Vec::with_capacity(groesse as usize);
        fs::File::open(p).ok()?.take(MAX_DATEI).read_to_end(&mut bytes).ok()?;
        if bytes.len() as u64 != groesse {
            return None; // zwischen Pruefung und Lesen veraendert: naechste Runde gibt es nicht, lieber nichts melden
        }
        let (b, h) = abmessungen(&bytes[..bytes.len().min(256 * 1024)]).unzip();
        Some(Fund {
            pfad: p.to_string_lossy().into_owned(),
            dateizeit_ms: md.modified().ok().and_then(ms_seit_1970),
            erstellt_ms: md.created().ok().and_then(ms_seit_1970),
            groesse,
            format,
            uebernehmbar: true,
            breite: b,
            hoehe: h,
            sha256: Some(Sha256::digest(&bytes).iter().map(|x| format!("{x:02x}")).collect()),
            quelle: quelle.to_string(),
            sim_vordergrund: vg,
        })
    }
}

#[derive(Default)]
pub struct Beobachter {
    staende: Vec<Stand>,
    vorher_vordergrund: Option<bool>,
}

impl Beobachter {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Setzt die Beobachtung auf die bestaetigte Liste. Unveraenderte, aktive
    /// Quellen behalten ihren Stand; neue oder wieder eingeschaltete Quellen
    /// bekommen eine frische Grundlage (alles, was dann schon liegt, ist Altbestand).
    pub fn aufsetzen(&mut self, quellen: &[Quelle], vordergrund: Option<bool>) {
        let mut alte: HashMap<String, Stand> = std::mem::take(&mut self.staende).into_iter().map(|s| (s.quelle.pfad.clone(), s)).collect();
        let jetzt = SystemTime::now();
        for q in quellen {
            let behalten = alte.remove(&q.pfad).filter(|s| s.quelle.aktiv && q.aktiv);
            let stand = match behalten {
                Some(mut s) => {
                    s.quelle = q.clone();
                    s
                }
                None => {
                    let mut s = Stand::neu(q.clone());
                    if q.aktiv {
                        s.scannen(true, jetzt, 0, vordergrund);
                    }
                    s
                }
            };
            self.staende.push(stand);
        }
    }

    /// Ein Takt: neue Dateien finden, wartende pruefen, fertige melden.
    /// `jetzt_ms` ist eine monotone Uhr des Aufrufers.
    pub fn takt(&mut self, jetzt_ms: u64, vordergrund: Option<bool>) -> Vec<Fund> {
        let jetzt = SystemTime::now();
        let vg_neu = oder(self.vorher_vordergrund, vordergrund);
        let mut funde = Vec::new();
        for s in self.staende.iter_mut().filter(|s| s.quelle.aktiv) {
            s.scannen(false, jetzt, jetzt_ms, vg_neu);
            s.wartende_pruefen(jetzt_ms, &mut funde);
        }
        self.vorher_vordergrund = vordergrund;
        funde
    }

    pub fn status(&self) -> Vec<QuellenStatus> {
        self.staende
            .iter()
            .map(|s| QuellenStatus {
                pfad: s.quelle.pfad.clone(),
                aktiv: s.quelle.aktiv,
                original_entfernen: s.quelle.original_entfernen,
                art: s.quelle.art.clone(),
                beobachtet: s.quelle.aktiv && s.fehler.is_none(),
                fehler: if s.quelle.aktiv { s.fehler.clone() } else { None },
                altbestand: s.alt.len(),
            })
            .collect()
    }

    pub fn altbestand(&self) -> Vec<AltEintrag> {
        let mut v: Vec<AltEintrag> = self.staende.iter().filter(|s| s.quelle.aktiv).flat_map(|s| s.alt.iter().cloned()).collect();
        v.sort_by(|a, b| b.dateizeit_ms.cmp(&a.dateizeit_ms).then_with(|| a.pfad.cmp(&b.pfad)));
        v.truncate(MAX_ALTBESTAND);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quellen::tests::{jpeg_bytes, png_bytes};

    fn q(dir: &Path) -> Quelle {
        Quelle { pfad: dir.to_string_lossy().into(), aktiv: true, original_entfernen: false, art: None }
    }

    fn takte(b: &mut Beobachter, n: u64, vg: Option<bool>) -> Vec<Fund> {
        (0..n).flat_map(|i| b.takt(i * 1500, vg)).collect()
    }

    #[test]
    fn altbestand_wird_nie_gemeldet_neues_erst_wenn_es_ruht_und_fertig_ist() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("alt.png"), png_bytes(10, 10)).unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        assert_eq!(b.altbestand().len(), 1);
        assert_eq!(b.status()[0].altbestand, 1);
        assert!(takte(&mut b, 4, None).is_empty(), "Altbestand bleibt still");

        let neu = t.path().join("neu.png");
        fs::write(&neu, png_bytes(2560, 1440)).unwrap();
        assert!(b.takt(0, None).is_empty(), "beim ersten Sehen noch nicht");
        let f = b.takt(1500, None);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].pfad, neu.to_string_lossy());
        assert_eq!((f[0].breite, f[0].hoehe), (Some(2560), Some(1440)));
        assert!(f[0].uebernehmbar && f[0].sha256.as_ref().unwrap().len() == 64);
        assert!(b.takt(3000, None).is_empty(), "nur einmal");
        assert_eq!(b.altbestand().len(), 1, "Altbestand bleibt die Grundlage vom Aufsetzen");
    }

    #[test]
    fn eine_noch_wachsende_oder_unfertige_datei_wird_nicht_gemeldet() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        let p = t.path().join("a.png");
        let voll = png_bytes(100, 100);
        fs::write(&p, &voll[..voll.len() - 12]).unwrap(); // Endchunk fehlt
        assert!(b.takt(0, None).is_empty());
        assert!(b.takt(1500, None).is_empty(), "ruht, aber nicht abgeschlossen");
        assert!(b.takt(3000, None).is_empty());
        fs::write(&p, &voll).unwrap(); // jetzt fertig
        assert!(b.takt(4500, None).is_empty(), "Aenderung setzt die Wartezeit zurueck");
        assert_eq!(b.takt(6000, None).len(), 1);
    }

    #[test]
    fn dauerhaft_kaputte_datei_wird_nach_einer_minute_fallengelassen() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        fs::write(t.path().join("k.jpg"), &jpeg_bytes(10, 10)[..20]).unwrap();
        assert!(b.takt(0, None).is_empty());
        assert!(b.takt(1500, None).is_empty());
        assert!(b.takt(61_000, None).is_empty());
        assert!(b.takt(62_500, None).is_empty());
        assert!(b.staende[0].wartend.is_empty(), "nicht mehr beobachtet");
    }

    #[test]
    fn unterordner_und_spaeter_entstehender_ordner() {
        let t = tempfile::tempdir().unwrap();
        let quelle = t.path().join("Output").join("screenshots"); // existiert noch nicht
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(&quelle)], None);
        assert_eq!(b.status()[0].fehler.as_deref(), Some("nicht_vorhanden"));
        assert!(!b.status()[0].beobachtet);
        assert!(b.takt(0, None).is_empty());
        fs::create_dir_all(quelle.join("Spiel").join("tief")).unwrap();
        fs::write(quelle.join("Spiel").join("tief").join("x.jpg"), jpeg_bytes(1470, 896)).unwrap();
        takte(&mut b, 1, None);
        let f = (1..4).flat_map(|i| b.takt(i * 1500, None)).collect::<Vec<_>>();
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].breite, f[0].hoehe), (Some(1470), Some(896)));
        assert!(b.status()[0].beobachtet && b.status()[0].fehler.is_none());
    }

    #[test]
    fn zu_tiefe_ordner_und_fremde_dateien_zaehlen_nicht() {
        let t = tempfile::tempdir().unwrap();
        let tief = t.path().join("a/b/c/d/e/f");
        fs::create_dir_all(&tief).unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        fs::write(tief.join("zu_tief.png"), png_bytes(5, 5)).unwrap();
        fs::write(t.path().join("a.txt"), b"x").unwrap();
        fs::write(t.path().join(".versteckt.png"), png_bytes(5, 5)).unwrap();
        fs::write(t.path().join("halb.png.tmp"), png_bytes(5, 5)).unwrap();
        fs::write(t.path().join("echt.PNG"), png_bytes(5, 5)).unwrap();
        let f = takte(&mut b, 4, None);
        assert_eq!(f.len(), 1, "{f:?}");
        assert!(f[0].pfad.ends_with("echt.PNG"));
    }

    #[test]
    fn jxr_wird_gemeldet_aber_nie_uebernehmbar() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        fs::write(t.path().join("hdr.jxr"), vec![1u8; 100]).unwrap();
        let f = takte(&mut b, 4, None);
        assert_eq!(f.len(), 1);
        assert!(!f[0].uebernehmbar && f[0].format == BildFormat::Jxr && f[0].sha256.is_none());
    }

    #[test]
    fn pausierte_quelle_bleibt_still_und_beim_einschalten_gilt_neu_als_altbestand() {
        let t = tempfile::tempdir().unwrap();
        let mut pausiert = q(t.path());
        pausiert.aktiv = false;
        let mut b = Beobachter::neu();
        b.aufsetzen(&[pausiert.clone()], None);
        fs::write(t.path().join("waehrend_pause.png"), png_bytes(5, 5)).unwrap();
        assert!(takte(&mut b, 4, None).is_empty());
        assert!(!b.status()[0].beobachtet);
        b.aufsetzen(&[q(t.path())], None);
        assert!(takte(&mut b, 4, None).is_empty(), "was in der Pause entstand, wird nicht nachtraeglich uebernommen");
        assert_eq!(b.altbestand().len(), 1);
    }

    #[test]
    fn unveraenderte_quelle_behaelt_ihren_stand_beim_neu_aufsetzen() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        fs::write(t.path().join("a.png"), png_bytes(5, 5)).unwrap();
        b.takt(0, None);
        b.aufsetzen(&[q(t.path())], None); // z. B. „Ordner pruefen“ mitten in der Wartezeit
        assert_eq!(b.takt(1500, None).len(), 1, "die wartende Datei geht nicht verloren");
    }

    #[test]
    fn datei_die_vorher_verschwindet_wird_nicht_gemeldet() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        let p = t.path().join("a.png");
        fs::write(&p, png_bytes(5, 5)).unwrap();
        b.takt(0, None);
        fs::remove_file(&p).unwrap();
        assert!(takte(&mut b, 3, None).is_empty());
    }

    #[test]
    fn vordergrund_beim_auftauchen_oder_im_takt_davor() {
        assert_eq!(oder(None, None), None);
        assert_eq!(oder(Some(false), None), Some(false));
        assert_eq!(oder(Some(false), Some(true)), Some(true));
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        b.takt(0, Some(true)); // Sim vorn, noch keine Datei
        fs::write(t.path().join("a.png"), png_bytes(5, 5)).unwrap();
        b.takt(1500, Some(false)); // Datei taucht auf, Fenster inzwischen weg
        let f = b.takt(3000, Some(false));
        assert_eq!(f[0].sim_vordergrund, Some(true), "Takt davor zaehlt (Game Bar schreibt kurz nach dem Tastendruck)");
        fs::write(t.path().join("b.png"), png_bytes(5, 5)).unwrap();
        b.takt(4500, Some(false));
        let g = b.takt(6000, Some(false));
        assert_eq!(g[0].sim_vordergrund, Some(false));
        let mut c = Beobachter::neu();
        let t2 = tempfile::tempdir().unwrap();
        c.aufsetzen(&[q(t2.path())], None);
        fs::write(t2.path().join("c.png"), png_bytes(5, 5)).unwrap();
        let h = takte(&mut c, 3, None);
        assert_eq!(h[0].sim_vordergrund, None, "unbekannt bleibt unbekannt");
    }
}
