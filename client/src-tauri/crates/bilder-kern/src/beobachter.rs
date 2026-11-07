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
//! * Ein gemeldeter Fund, den der Aufrufer nicht dauerhaft ablegen konnte (Datentraeger
//!   voll), wird mit [`zurueckstellen`] wieder unbekannt und nach 10 s erneut gemeldet.
//! * **Nur gemeldete Dateien gelten als bekannt** (mit Groesse und Aenderungszeit,
//!   nicht nur dem Pfad). Unfertige, voruebergehend unlesbare oder zwischen
//!   Pruefung und Lesen veraenderte Dateien bleiben unter Beobachtung (mit
//!   wachsendem Abstand der Pruefungen, hoechstens 30 s); eine feste Frist, nach
//!   der man sie fallen laesst, gibt es nicht. Die Grenze gegen Dauerlast ist
//!   `MAX_WARTEND`. Verschwindet ein Pfad, wird er freigegeben: ein
//!   wiederverwendeter Name gilt als neue Datei. Der Altbestand bleibt
//!   ausgenommen, solange sein Pfad besteht.
//! * Jede Quelle hat ihren eigenen Stand mit eigener Sperre. Ein haengender
//!   Ordner (Netzlaufwerk ohne Verbindung) haelt nur seine eigene Abfrage auf:
//!   Anzeige, Einstellungen und die uebrigen Quellen warten nie darauf, und die
//!   Grundlage einer neuen Quelle laeuft getrennt vom Speichern. Auch die
//!   Abfrage im Takt laeuft je Quelle in einem eigenen Faden
//!   ([`takt_ausfuehren`]): eine haengende Netzquelle haelt die lokalen nicht
//!   auf, ihr Faden bleibt einfach stehen, wird nicht doppelt gestartet und
//!   liefert nach dem Loesen nach.
//!
//! Kosten: Ein Ordner wird nur dann neu gelesen, wenn sich seine Aenderungszeit
//! geaendert hat oder frisch ist (< 6 s, wegen grober Zeitaufloesung mancher
//! Dateisysteme); die Unterordner-Liste bleibt zwischengespeichert. Ein grosser
//! Bilder-Ordner kostet im Takt deshalb ein `stat` je Ordner.
//!
//! Was beim Aufsetzen schon im Ordner liegt, ist **Altbestand**: es wird nie von
//! selbst gemeldet oder angefasst, nur als Liste angeboten.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::quellen::{abmessungen, handle_vollstaendig, BildFormat, Quelle};
use crate::sicher;
use crate::speicher::{ms_seit_1970, MAX_DATEI};
use crate::Fehler;

/// Tiefste Ebene unterhalb der Quelle, die beobachtet wird (NVIDIA: Stamm/Spiel/Datei).
const MAX_TIEFE: usize = 4;
const MAX_ALTBESTAND: usize = 5000;
const FRISCH: Duration = Duration::from_secs(6);
/// Abstand der Pruefungen einer unfertigen Datei wachst von einem Takt bis hierher.
const MAX_ABSTAND_MS: u64 = 30_000;
const TAKT_MS: u64 = 1500;
/// Obergrenze gegen Dauerlast: so viele Dateien warten hoechstens gleichzeitig.
/// Weitere werden erst aufgenommen, wenn Platz ist (der Ordner wird dann neu gelesen).
const MAX_WARTEND: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Fund {
    pub pfad: String,
    /// Aenderungszeit der Datei (ms seit 1970, UTC).
    pub dateizeit_ms: Option<i64>,
    pub erstellt_ms: Option<i64>,
    pub groesse: u64,
    pub format: BildFormat,
    /// Die Datei ist JPG/PNG und abgeschlossen. `false` bei `.jxr` (HDR): wird gemeldet, nie uebernommen.
    pub uebernehmbar: bool,
    pub px_breite: Option<u32>,
    pub px_hoehe: Option<u32>,
    /// SHA-256 der Originaldatei, fuer die Duplikaterkennung des Pakets.
    pub sha256: Option<String>,
    /// Pfad der bestaetigten Quelle, in der sie liegt.
    pub quelle: String,
    /// Art dieser Quelle (`standard`, `aufgespuert`, `hand`, sonst frei).
    pub art: Option<String>,
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
    /// Die Grundlage (Altbestand) wird noch erhoben; solange ist `beobachtet` falsch.
    pub wird_geprueft: bool,
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
    vordergrund: Option<bool>,
    format: BildFormat,
    /// Fehlversuche bei unveraendertem Stand (bestimmt den Abstand).
    versuche: u32,
    /// Fruehestens dann wieder pruefen (`u64::MAX`: erst nach einer Aenderung).
    naechster_ms: u64,
}

/// Eine gemeldete Datei (Groesse und Zeit) oder eine des Altbestands (nur der Pfad zaehlt).
#[derive(Clone)]
struct Bekannt {
    groesse: u64,
    zeit: Option<SystemTime>,
    alt: bool,
}

/// Der Stand einer Quelle, der beim Abfragen des Ordners gebraucht wird.
struct Stand {
    quelle: Quelle,
    verz: HashMap<PathBuf, VerzStand>,
    bekannt: HashMap<PathBuf, Bekannt>,
    wartend: HashMap<PathBuf, Wartend>,
    alt: Vec<AltEintrag>,
    fehler: Option<String>,
}

/// Was die Anzeige braucht; wird nach jeder Abfrage erneuert und nie waehrend
/// Ordner-Zugriffen gehalten.
#[derive(Default, Clone)]
struct Kurz {
    fehler: Option<String>,
    alt: Vec<AltEintrag>,
}

/// Eine bestaetigte Quelle: Einstellung, Anzeige und Abfragestand getrennt gesperrt.
pub struct Slot {
    pfad: String,
    konf: Mutex<Quelle>,
    kurz: Mutex<Kurz>,
    bereit: AtomicBool,
    inner: Mutex<Stand>,
    /// Ein Abfrage-Faden dieser Quelle laeuft (oder haengt) noch.
    laeuft: AtomicBool,
    /// Funde, die ein Abfrage-Faden gefunden hat und die der Takt noch abholt.
    ausgang: Mutex<Vec<Fund>>,
    /// Funde, deren Ablage im Eingang misslang: wieder unter Beobachtung (Fund, fruehestens ab ms).
    zurueck: Mutex<Vec<(Fund, u64)>>,
}

fn sperre<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
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

struct Lauf {
    baseline: bool,
    jetzt: SystemTime,
    vg: Option<bool>,
    alt: Vec<AltEintrag>,
    /// Bild-Dateien, die in einem neu gelesenen Ordner standen.
    gelistet: HashSet<PathBuf>,
    gelesen: HashSet<PathBuf>,
    besucht: HashSet<PathBuf>,
}

impl Stand {
    fn neu(quelle: Quelle) -> Self {
        Self { quelle, verz: HashMap::new(), bekannt: HashMap::new(), wartend: HashMap::new(), alt: Vec::new(), fehler: None }
    }

    fn wurzel(&self) -> PathBuf {
        PathBuf::from(&self.quelle.pfad)
    }

    fn scannen(&mut self, baseline: bool, jetzt: SystemTime, vordergrund: Option<bool>) {
        let wurzel = self.wurzel();
        match fs::metadata(&wurzel) {
            Err(e) => {
                // Ordner fehlt oder Laufwerk offline: nichts vergessen, nur melden.
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
        let mut lauf = Lauf {
            baseline,
            jetzt,
            vg: vordergrund,
            alt: Vec::new(),
            gelistet: HashSet::new(),
            gelesen: HashSet::new(),
            besucht: HashSet::new(),
        };
        self.scanne_dir(&wurzel, 0, &mut lauf);
        // Verschwundene Pfade freigeben: in neu gelesenen Ordnern, was nicht mehr
        // dasteht; in Ordnern, die es nicht mehr gibt, alles.
        if self.fehler.is_none() {
            self.bekannt.retain(|p, _| match p.parent() {
                Some(d) if lauf.gelesen.contains(d) => lauf.gelistet.contains(p),
                Some(d) => lauf.besucht.contains(d),
                None => false,
            });
        }
        if baseline {
            let mut alt = lauf.alt;
            alt.sort_by(|a, b| b.dateizeit_ms.cmp(&a.dateizeit_ms).then_with(|| a.pfad.cmp(&b.pfad)));
            alt.truncate(MAX_ALTBESTAND);
            self.alt = alt;
        }
    }

    fn scanne_dir(&mut self, dir: &Path, tiefe: usize, lauf: &mut Lauf) {
        lauf.besucht.insert(dir.to_path_buf());
        let mtime = fs::metadata(dir).and_then(|m| m.modified()).ok();
        let frisch = mtime.is_none_or(|t| lauf.jetzt.duration_since(t).map_or(true, |d| d < FRISCH));
        if !lauf.baseline && !frisch {
            if let Some(v) = self.verz.get(dir) {
                if v.mtime == mtime {
                    for k in v.unterordner.clone() {
                        if tiefe < MAX_TIEFE {
                            self.scanne_dir(&k, tiefe + 1, lauf);
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
        lauf.gelesen.insert(dir.to_path_buf());
        let mut unter: Vec<PathBuf> = Vec::new();
        let mut voll = false;
        for e in rd.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            let p = e.path();
            if ft.is_dir() && !ft.is_symlink() {
                unter.push(p);
            } else if ft.is_file() {
                let Some(format) = bild_endung(&p) else { continue };
                lauf.gelistet.insert(p.clone());
                if self.wartend.contains_key(&p) {
                    continue;
                }
                let Ok(md) = e.metadata() else { continue };
                if let Some(b) = self.bekannt.get(&p) {
                    if b.alt || (b.groesse == md.len() && b.zeit == md.modified().ok()) {
                        continue;
                    }
                    // gleicher Name, anderer Inhalt: eine neue Datei
                    self.bekannt.remove(&p);
                }
                if lauf.baseline {
                    self.bekannt.insert(p.clone(), Bekannt { groesse: md.len(), zeit: md.modified().ok(), alt: true });
                    lauf.alt.push(AltEintrag {
                        pfad: p.to_string_lossy().into_owned(),
                        dateizeit_ms: md.modified().ok().and_then(ms_seit_1970),
                        groesse: md.len(),
                        format,
                        quelle: self.quelle.pfad.clone(),
                    });
                } else if self.wartend.len() >= MAX_WARTEND {
                    voll = true; // spaeter noch einmal lesen, wenn Platz ist
                } else {
                    self.wartend.insert(
                        p,
                        Wartend {
                            groesse: md.len(),
                            zeit: md.modified().ok(),
                            gleich: 0,
                            gerade_entdeckt: true,
                            vordergrund: lauf.vg,
                            format,
                            versuche: 0,
                            naechster_ms: 0,
                        },
                    );
                }
            }
        }
        unter.sort();
        if voll {
            self.verz.remove(dir);
        } else {
            self.verz.insert(dir.to_path_buf(), VerzStand { mtime, unterordner: unter.clone() });
        }
        if tiefe < MAX_TIEFE {
            for k in unter {
                self.scanne_dir(&k, tiefe + 1, lauf);
            }
        }
    }

    fn wartende_pruefen(&mut self, jetzt_ms: u64, funde: &mut Vec<Fund>) {
        let pfade: Vec<PathBuf> = self.wartend.keys().cloned().collect();
        for p in pfade {
            let Some(w) = self.wartend.get_mut(&p) else { continue };
            let Ok(md) = fs::metadata(&p) else {
                self.wartend.remove(&p); // weg, bevor sie fertig war: Pfad ist frei
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
                w.versuche = 0;
                w.naechster_ms = 0;
            }
            if w.gleich < 1 || jetzt_ms < w.naechster_ms {
                continue;
            }
            let (format, vg) = (w.format, w.vordergrund);
            let _ = w;
            let ergebnis = match format {
                BildFormat::Jxr => Ok(Fund {
                    pfad: p.to_string_lossy().into_owned(),
                    dateizeit_ms: md.modified().ok().and_then(ms_seit_1970),
                    erstellt_ms: md.created().ok().and_then(ms_seit_1970),
                    groesse: md.len(),
                    format,
                    uebernehmbar: false,
                    px_breite: None,
                    px_hoehe: None,
                    sha256: None,
                    quelle: self.quelle.pfad.clone(),
                    art: self.quelle.art.clone(),
                    sim_vordergrund: vg,
                }),
                _ => Self::fund_lesen(&p, format, &self.quelle.pfad, self.quelle.art.clone(), vg),
            };
            match ergebnis {
                Ok(fund) => {
                    self.wartend.remove(&p);
                    self.bekannt.insert(p, Bekannt { groesse: fund.groesse, zeit: md.modified().ok(), alt: false });
                    funde.push(fund);
                }
                Err(Fehler::Format(_) | Fehler::Gross(_)) => {
                    // Kein (brauchbares) Bild in dieser Fassung: nicht melden, nicht bekannt
                    // machen, erst nach einer Aenderung der Datei wieder ansehen.
                    if let Some(w) = self.wartend.get_mut(&p) {
                        w.naechster_ms = u64::MAX;
                    }
                }
                Err(_) => {
                    // unfertig, gesperrt, zwischendurch veraendert: weiter beobachten
                    if let Some(w) = self.wartend.get_mut(&p) {
                        w.versuche = w.versuche.saturating_add(1);
                        let abstand = (TAKT_MS << w.versuche.min(5)).min(MAX_ABSTAND_MS);
                        w.naechster_ms = jetzt_ms.saturating_add(abstand);
                    }
                }
            }
        }
    }

    /// Ein gemeldeter Fund konnte nicht dauerhaft abgelegt werden: der Pfad gilt
    /// wieder als unbekannt und wird nach `ab_ms` erneut gelesen und gemeldet.
    fn fund_zurueckstellen(&mut self, f: &Fund, ab_ms: u64) {
        let p = PathBuf::from(&f.pfad);
        let Some(b) = self.bekannt.get(&p).filter(|b| !b.alt).cloned() else { return };
        self.bekannt.remove(&p);
        self.wartend.insert(
            p,
            Wartend {
                groesse: b.groesse,
                zeit: b.zeit,
                gleich: 1,
                gerade_entdeckt: false,
                vordergrund: f.sim_vordergrund,
                format: f.format,
                versuche: 0,
                naechster_ms: ab_ms,
            },
        );
    }

    /// Oeffnet die Datei einmal (ohne Linkfolgen), prueft am Handle, dass sie
    /// vollstaendig ist, und liest sie von dort; Hash und Masse kommen aus
    /// denselben Bytes. Jede Abweichung ist ein Fehler, nie ein stilles „nichts“.
    fn fund_lesen(p: &Path, format: BildFormat, quelle: &str, art: Option<String>, vg: Option<bool>) -> Result<Fund, Fehler> {
        let mut f = sicher::lesend_ohne_folgen(p).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Fehler::NichtGefunden(e.to_string()),
            _ => Fehler::Unfertig(format!("Datei nicht lesbar (noch in Benutzung?): {e}")),
        })?;
        let groesse = handle_vollstaendig(&mut f, format)?;
        f.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::with_capacity(groesse as usize);
        (&mut f).take(MAX_DATEI).read_to_end(&mut bytes)?;
        let md = f.metadata()?;
        if bytes.len() as u64 != groesse || md.len() != groesse {
            return Err(Fehler::Unfertig("Datei hat sich waehrend des Lesens veraendert".into()));
        }
        let (b, h) = abmessungen(&bytes[..bytes.len().min(256 * 1024)]).unzip();
        Ok(Fund {
            pfad: p.to_string_lossy().into_owned(),
            dateizeit_ms: md.modified().ok().and_then(ms_seit_1970),
            erstellt_ms: md.created().ok().and_then(ms_seit_1970),
            groesse,
            format,
            uebernehmbar: true,
            px_breite: b,
            px_hoehe: h,
            sha256: Some(Sha256::digest(&bytes).iter().map(|x| format!("{x:02x}")).collect()),
            quelle: quelle.to_string(),
            art,
            sim_vordergrund: vg,
        })
    }
}

impl Slot {
    fn neu(q: Quelle) -> Self {
        Self {
            pfad: q.pfad.clone(),
            konf: Mutex::new(q.clone()),
            kurz: Mutex::new(Kurz::default()),
            bereit: AtomicBool::new(false),
            inner: Mutex::new(Stand::neu(q)),
            laeuft: AtomicBool::new(false),
            ausgang: Mutex::new(Vec::new()),
            zurueck: Mutex::new(Vec::new()),
        }
    }

    fn kurz_erneuern(&self, st: &Stand, mit_alt: bool) {
        let mut k = sperre(&self.kurz);
        k.fehler = st.fehler.clone();
        if mit_alt {
            k.alt = st.alt.clone();
        }
    }

    /// Die Grundlage einer neuen oder wieder eingeschalteten Quelle: alles, was
    /// jetzt schon im Ordner liegt, ist Altbestand. Kann bei einem haengenden
    /// Laufwerk lange dauern und blockiert deshalb nur diese Quelle.
    pub fn grundlage(&self, vordergrund: Option<bool>) {
        let mut st = sperre(&self.inner);
        let k = sperre(&self.konf).clone();
        st.quelle = k.clone();
        if k.aktiv {
            st.scannen(true, SystemTime::now(), vordergrund);
        }
        self.kurz_erneuern(&st, true);
        self.bereit.store(true, Ordering::SeqCst);
    }

    fn takt(&self, jetzt_ms: u64, vg: Option<bool>, funde: &mut Vec<Fund>) {
        let k = sperre(&self.konf).clone();
        if !k.aktiv || !self.bereit.load(Ordering::SeqCst) {
            return;
        }
        // Gerade belegt (Grundlage laeuft): in diesem Takt auslassen, nie warten.
        let Ok(mut st) = self.inner.try_lock() else { return };
        st.quelle = k;
        for (f, ab_ms) in std::mem::take(&mut *sperre(&self.zurueck)) {
            st.fund_zurueckstellen(&f, ab_ms);
        }
        #[cfg(test)]
        tests::haengen(&self.pfad);
        st.scannen(false, SystemTime::now(), vg);
        st.wartende_pruefen(jetzt_ms, funde);
        self.kurz_erneuern(&st, false);
    }

    pub fn ist_bereit(&self) -> bool {
        self.bereit.load(Ordering::SeqCst)
    }
}

/// So lange bleibt ein Fund zurueckgestellt, dessen Ablage misslang (voller Datentraeger).
const ABLAGE_WIEDER_MS: u64 = 10_000;

/// Meldet, dass `fund` nicht dauerhaft abgelegt werden konnte: er wird nach
/// [`ABLAGE_WIEDER_MS`] erneut gemeldet statt nie wieder. Blockiert nie (auch
/// nicht bei einer haengenden Quelle); die Quelle uebernimmt es im naechsten Takt.
pub fn zurueckstellen(slots: &[Arc<Slot>], fund: &Fund, jetzt_ms: u64) {
    for s in slots.iter().filter(|s| s.pfad == fund.quelle) {
        sperre(&s.zurueck).push((fund.clone(), jetzt_ms.saturating_add(ABLAGE_WIEDER_MS)));
    }
}

/// So lange wartet ein Takt hoechstens auf die Abfragen; was dann noch laeuft,
/// liefert beim naechsten Takt nach.
const ABFRAGE_FRIST: Duration = Duration::from_secs(1);

/// Gibt die Quelle frei, wenn ihr Abfrage-Faden endet (auch bei einem Absturz).
struct LaufEnde(Arc<Slot>);

impl Drop for LaufEnde {
    fn drop(&mut self) {
        self.0.laeuft.store(false, Ordering::SeqCst);
    }
}

/// Ein Takt ueber die gegebenen Quellen. Jede Quelle wird in einem eigenen Faden
/// abgefragt; eine haengende (Netzlaufwerk ohne Verbindung) haelt weder die
/// anderen noch diesen Aufruf laenger als [`ABFRAGE_FRIST`] auf. Ihr Faden
/// bleibt stehen, bis das Laufwerk antwortet, wird nicht ein zweites Mal
/// gestartet, und was er dann findet, kommt mit einem spaeteren Takt.
pub fn takt_ausfuehren(slots: &[Arc<Slot>], jetzt_ms: u64, vordergrund: Option<bool>) -> Vec<Fund> {
    let (tx, rx) = mpsc::channel::<()>();
    let mut gestartet = 0;
    for s in slots {
        if s.laeuft.swap(true, Ordering::SeqCst) {
            continue; // der vorige Faden dieser Quelle haengt noch
        }
        let ende = LaufEnde(Arc::clone(s));
        let tx = tx.clone();
        let start = std::thread::Builder::new().name("bilder-abfrage".into()).spawn(move || {
            let mut funde = Vec::new();
            ende.0.takt(jetzt_ms, vordergrund, &mut funde);
            sperre(&ende.0.ausgang).extend(funde);
            drop(ende);
            let _ = tx.send(());
        });
        if start.is_ok() {
            gestartet += 1;
        } else {
            s.laeuft.store(false, Ordering::SeqCst);
        }
    }
    drop(tx);
    let frist = std::time::Instant::now() + ABFRAGE_FRIST;
    for _ in 0..gestartet {
        if rx.recv_timeout(frist.saturating_duration_since(std::time::Instant::now())).is_err() {
            break;
        }
    }
    slots.iter().flat_map(|s| std::mem::take(&mut *sperre(&s.ausgang))).collect()
}

#[derive(Default)]
pub struct Beobachter {
    slots: Vec<Arc<Slot>>,
    vorher_vordergrund: Option<bool>,
}

impl Beobachter {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Schnell und ohne Ordner-Zugriff: setzt die Liste der Quellen. Unveraenderte,
    /// aktive Quellen behalten ihren Stand (nur die Einstellung wird erneuert);
    /// neue oder wieder eingeschaltete Quellen bekommen einen frischen Stand,
    /// dessen Grundlage der Aufrufer **ausserhalb** der Sperre mit
    /// [`Slot::grundlage`] erhebt. Gibt genau diese Quellen zurueck.
    pub fn einsetzen(&mut self, quellen: &[Quelle]) -> Vec<Arc<Slot>> {
        let mut alte: HashMap<String, Arc<Slot>> = std::mem::take(&mut self.slots).into_iter().map(|s| (s.pfad.clone(), s)).collect();
        let mut neue = Vec::new();
        for q in quellen {
            let behalten = alte.remove(&q.pfad).filter(|s| sperre(&s.konf).aktiv && q.aktiv);
            let slot = match behalten {
                Some(s) => {
                    *sperre(&s.konf) = q.clone();
                    s
                }
                None => {
                    let s = Arc::new(Slot::neu(q.clone()));
                    neue.push(Arc::clone(&s));
                    s
                }
            };
            self.slots.push(slot);
        }
        neue
    }

    /// Wie [`Self::einsetzen`], erhebt die Grundlagen aber gleich (Tests, einfache Aufrufer).
    pub fn aufsetzen(&mut self, quellen: &[Quelle], vordergrund: Option<bool>) {
        for s in self.einsetzen(quellen) {
            s.grundlage(vordergrund);
        }
    }

    /// Fuer einen Takt ausserhalb der Sperre: die Quellen und der Vordergrund
    /// „jetzt oder im Takt davor“.
    pub fn takt_vorbereiten(&mut self, vordergrund: Option<bool>) -> (Vec<Arc<Slot>>, Option<bool>) {
        let vg_neu = oder(self.vorher_vordergrund, vordergrund);
        self.vorher_vordergrund = vordergrund;
        (self.slots.clone(), vg_neu)
    }

    /// Ein Takt: neue Dateien finden, wartende pruefen, fertige melden.
    /// `jetzt_ms` ist eine monotone Uhr des Aufrufers.
    pub fn takt(&mut self, jetzt_ms: u64, vordergrund: Option<bool>) -> Vec<Fund> {
        let (slots, vg) = self.takt_vorbereiten(vordergrund);
        takt_ausfuehren(&slots, jetzt_ms, vg)
    }

    pub fn status(&self) -> Vec<QuellenStatus> {
        self.slots
            .iter()
            .map(|s| {
                let k = sperre(&s.konf).clone();
                let ku = sperre(&s.kurz).clone();
                let bereit = s.ist_bereit();
                QuellenStatus {
                    pfad: s.pfad.clone(),
                    aktiv: k.aktiv,
                    original_entfernen: k.original_entfernen,
                    art: k.art,
                    beobachtet: k.aktiv && bereit && ku.fehler.is_none(),
                    wird_geprueft: k.aktiv && !bereit,
                    fehler: if k.aktiv { ku.fehler } else { None },
                    altbestand: ku.alt.len(),
                }
            })
            .collect()
    }

    pub fn altbestand(&self) -> Vec<AltEintrag> {
        let mut v: Vec<AltEintrag> = self
            .slots
            .iter()
            .filter(|s| sperre(&s.konf).aktiv)
            .flat_map(|s| sperre(&s.kurz).alt.clone())
            .collect();
        v.sort_by(|a, b| b.dateizeit_ms.cmp(&a.dateizeit_ms).then_with(|| a.pfad.cmp(&b.pfad)));
        v.truncate(MAX_ALTBESTAND);
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quellen::tests::{jpeg_bytes, png_bytes};

    /// Test-Haken: laesst die Abfrage einer Quelle haengen, bis sie freigegeben wird (hoechstens 5 s).
    static HAENGT: Mutex<Vec<(String, Arc<AtomicBool>)>> = Mutex::new(Vec::new());

    pub(super) fn haengen(pfad: &str) {
        let frei = sperre(&HAENGT).iter().find(|(p, _)| p == pfad).map(|(_, f)| Arc::clone(f));
        if let Some(frei) = frei {
            let start = std::time::Instant::now();
            while !frei.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }

    fn q(dir: &Path) -> Quelle {
        Quelle { pfad: dir.to_string_lossy().into(), aktiv: true, original_entfernen: false, art: None }
    }

    fn takte(b: &mut Beobachter, n: u64, vg: Option<bool>) -> Vec<Fund> {
        (0..n).flat_map(|i| b.takt(i * 1500, vg)).collect()
    }

    /// Fund 3: eine haengende Netzquelle haelt die lokalen nicht auf.
    #[test]
    fn haengende_quelle_haelt_die_anderen_nicht_auf_und_wird_nicht_doppelt_gestartet() {
        let netz = tempfile::tempdir().unwrap();
        let lokal = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        // Die haengende Quelle steht zuerst: ein einzelner Faden bliebe an ihr haengen.
        b.aufsetzen(&[q(netz.path()), q(lokal.path())], None);
        let frei = Arc::new(AtomicBool::new(false));
        sperre(&HAENGT).push((netz.path().to_string_lossy().into_owned(), Arc::clone(&frei)));
        fs::write(lokal.path().join("neu.png"), png_bytes(10, 10)).unwrap();
        fs::write(netz.path().join("spaet.png"), png_bytes(10, 10)).unwrap();

        let t = std::time::Instant::now();
        assert!(b.takt(0, None).is_empty(), "erster Takt: nur entdeckt");
        assert!(t.elapsed() < Duration::from_secs(3), "der Takt wartet nicht auf die haengende Quelle: {:?}", t.elapsed());
        let t = std::time::Instant::now();
        let f = b.takt(1500, None);
        assert_eq!(f.len(), 1, "die lokale Quelle meldet trotz haengender Nachbarin");
        assert!(f[0].pfad.ends_with("neu.png"));
        assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
        assert!(b.slots[0].laeuft.load(Ordering::SeqCst), "der haengende Faden steht noch und wird nicht doppelt gestartet");

        frei.store(true, Ordering::SeqCst); // das Laufwerk antwortet wieder
        let mut spaeter = Vec::new();
        for i in 2..10 {
            spaeter.extend(b.takt(i * 1500, None));
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(spaeter.len(), 1, "die zuvor haengende Quelle liefert nach");
        assert!(spaeter[0].pfad.ends_with("spaet.png"));
    }

    /// Fund 5: misslingt die Ablage im Eingang, wird die Datei erneut gemeldet.
    #[test]
    fn zurueckgestellter_fund_wird_nach_der_wartezeit_erneut_gemeldet_und_dann_nicht_mehr() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        fs::write(t.path().join("a.png"), png_bytes(10, 10)).unwrap();
        assert!(b.takt(0, None).is_empty());
        let f = b.takt(1500, None);
        assert_eq!(f.len(), 1);
        assert!(b.takt(3000, None).is_empty(), "ohne Rueckstellung gilt sie als bekannt");
        zurueckstellen(&b.slots, &f[0], 3000); // Ablage misslang
        assert!(b.takt(4500, None).is_empty(), "nicht sofort wieder");
        assert!(b.takt(12_000, None).is_empty());
        let again = b.takt(14_000, None);
        assert_eq!(again.len(), 1, "nach der Wartezeit erneut gemeldet");
        assert_eq!(again[0].pfad, f[0].pfad);
        assert_eq!(again[0].sha256, f[0].sha256);
        assert!(takte(&mut b, 4, None).is_empty(), "wurde die Ablage bestaetigt, bleibt sie still");
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
        assert_eq!((f[0].px_breite, f[0].px_hoehe), (Some(2560), Some(1440)));
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

    fn wartende(b: &Beobachter) -> usize {
        sperre(&b.slots[0].inner).wartend.len()
    }

    #[test]
    fn unfertige_datei_bleibt_beobachtet_auch_nach_minuten_und_wird_spaeter_gemeldet() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        let p = t.path().join("k.jpg");
        let voll = jpeg_bytes(10, 10);
        fs::write(&p, &voll[..20]).unwrap();
        assert!(b.takt(0, None).is_empty());
        assert!(b.takt(1500, None).is_empty());
        assert!(b.takt(61_000, None).is_empty());
        assert!(b.takt(62_500, None).is_empty());
        assert!(b.takt(600_000, None).is_empty());
        assert_eq!(wartende(&b), 1, "nach 10 Minuten immer noch unter Beobachtung (keine 60-s-Frist)");
        fs::write(&p, &voll).unwrap(); // jetzt fertig
        let mut f = Vec::new();
        for i in 1..=3 {
            f.extend(b.takt(600_000 + i * 1500, None));
        }
        assert_eq!(f.len(), 1, "die spaet fertige Datei erscheint");
        assert_eq!(wartende(&b), 0);
    }

    #[cfg(unix)]
    #[test]
    fn voruebergehend_unlesbare_datei_wird_nach_dem_freigeben_gemeldet() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        let p = t.path().join("gesperrt.png");
        fs::write(&p, png_bytes(40, 30)).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::File::open(&p).is_ok() {
            return; // Benutzer ohne Zugriffsschutz (root): die Lage laesst sich nicht nachstellen
        }
        assert!(b.takt(0, None).is_empty());
        assert!(b.takt(1500, None).is_empty());
        assert!(b.takt(3000, None).is_empty());
        assert_eq!(wartende(&b), 1, "unlesbar = weiter beobachten, nicht verworfen");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap(); // aendert die Groesse/Zeit nicht
        let f: Vec<Fund> = (0..30).flat_map(|i| b.takt(10_000 + i * 1500, None)).collect();
        assert_eq!(f.len(), 1, "nach dem freigeben gemeldet");
    }

    #[test]
    fn nur_gemeldete_dateien_gelten_als_bekannt_ein_wiederverwendeter_name_ist_neu() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        let p = t.path().join("shot.png");
        fs::write(&p, png_bytes(10, 10)).unwrap();
        assert_eq!(takte(&mut b, 3, None).len(), 1);
        assert!(b.takt(10_000, None).is_empty(), "nicht doppelt");
        // Name wird frei (Verschieben/Loeschen) und fuer ein neues Bild benutzt
        fs::remove_file(&p).unwrap();
        b.takt(11_500, None);
        fs::write(&p, png_bytes(77, 55)).unwrap();
        let f: Vec<Fund> = (0..4).flat_map(|i| b.takt(13_000 + i * 1500, None)).collect();
        assert_eq!(f.len(), 1, "gleicher Name, neues Bild: wird gemeldet");
        assert_eq!((f[0].px_breite, f[0].px_hoehe), (Some(77), Some(55)));
    }

    #[test]
    fn auch_der_name_einer_altbestand_datei_wird_frei_wenn_sie_verschwindet() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("alt.png");
        fs::write(&p, png_bytes(10, 10)).unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        assert!(takte(&mut b, 3, None).is_empty());
        fs::remove_file(&p).unwrap();
        b.takt(10_000, None);
        fs::write(&p, png_bytes(20, 20)).unwrap();
        let f: Vec<Fund> = (0..4).flat_map(|i| b.takt(11_500 + i * 1500, None)).collect();
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn kein_bild_unter_dem_namen_wird_nicht_gemeldet_aber_nach_einer_aenderung_neu_angesehen() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        let p = t.path().join("falsch.png");
        fs::write(&p, jpeg_bytes(10, 10)).unwrap(); // JPEG unter .png
        assert!(takte(&mut b, 4, None).is_empty());
        assert_eq!(wartende(&b), 1);
        fs::write(&p, png_bytes(31, 21)).unwrap();
        let f: Vec<Fund> = (0..4).flat_map(|i| b.takt(10_000 + i * 1500, None)).collect();
        assert_eq!(f.len(), 1);
    }

    #[test]
    fn die_obergrenze_gegen_dauerlast_haelt_und_nichts_geht_verloren() {
        let t = tempfile::tempdir().unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(t.path())], None);
        let n = MAX_WARTEND + 20;
        for i in 0..n {
            fs::write(t.path().join(format!("u{i}.png")), &png_bytes(5, 5)[..30]).unwrap(); // alle unfertig
        }
        takte(&mut b, 2, None);
        assert!(wartende(&b) <= MAX_WARTEND, "Obergrenze");
        // Alle werden fertig: erst die wartenden, dann die zurueckgestellten
        for i in 0..n {
            fs::write(t.path().join(format!("u{i}.png")), png_bytes(5 + i as u32 % 3, 6)).unwrap();
        }
        let mut gemeldet = 0;
        for i in 0..400u64 {
            gemeldet += b.takt(100_000 + i * 1500, None).len();
        }
        assert_eq!(gemeldet, n, "jede Datei genau einmal");
    }

    #[test]
    fn ein_haengender_ordner_blockiert_weder_die_anzeige_noch_die_anderen_quellen() {
        // Eine belegte Quelle (z. B. Laufwerk haengt in read_dir) darf weder Takt noch Status aufhalten.
        let t = tempfile::tempdir().unwrap();
        let (a, c) = (t.path().join("a"), t.path().join("c"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&c).unwrap();
        let mut b = Beobachter::neu();
        b.aufsetzen(&[q(&a), q(&c)], None);
        let haelt = Arc::clone(&b.slots[0]);
        let _sperre = sperre(&haelt.inner); // „haengt“
        fs::write(c.join("x.png"), png_bytes(9, 9)).unwrap();
        let f: Vec<Fund> = (0..4).flat_map(|i| b.takt(i * 1500, None)).collect();
        assert_eq!(f.len(), 1, "die andere Quelle meldet weiter");
        assert_eq!(b.status().len(), 2, "Status wartet nicht");
        let _ = b.altbestand();
    }

    #[test]
    fn neue_quelle_von_hand_meldet_ihren_altbestand_nicht_und_blockiert_nicht() {
        let t = tempfile::tempdir().unwrap();
        let hand = t.path().join("meine_screens");
        fs::create_dir_all(&hand).unwrap();
        fs::write(hand.join("schon_da.png"), png_bytes(10, 10)).unwrap();
        let fehlt = t.path().join("netz_offline"); // Laufwerk gerade nicht da
        let mut b = Beobachter::neu();
        let mut neu_q = q(&hand);
        neu_q.art = Some("hand".into());
        let neue = b.einsetzen(&[neu_q, q(&fehlt)]);
        assert_eq!(neue.len(), 2);
        assert!(b.status()[0].wird_geprueft && !b.status()[0].beobachtet, "Grundlage laeuft noch");
        assert!(b.takt(0, None).is_empty(), "vor der Grundlage nichts");
        for s in &neue {
            s.grundlage(None);
        }
        let st = b.status();
        assert!(st[0].beobachtet && !st[0].wird_geprueft && st[0].art.as_deref() == Some("hand"));
        assert_eq!(st[0].altbestand, 1);
        assert_eq!(st[1].fehler.as_deref(), Some("nicht_vorhanden"), "fehlender Ordner = Status, kein Absturz");
        assert!(takte(&mut b, 4, None).is_empty(), "Altbestand der neuen Quelle wird nie gemeldet");
        fs::write(hand.join("neu.png"), png_bytes(11, 11)).unwrap();
        let f: Vec<Fund> = (0..4).flat_map(|i| b.takt(10_000 + i * 1500, None)).collect();
        assert_eq!(f.len(), 1);
        assert!(f[0].pfad.ends_with("neu.png"));
        // der Ordner erscheint spaeter: ab dann beobachtet (Altbestand nur, was beim Aufsetzen da war)
        fs::create_dir_all(&fehlt).unwrap();
        fs::write(fehlt.join("spaet.png"), png_bytes(12, 12)).unwrap();
        let f: Vec<Fund> = (0..4).flat_map(|i| b.takt(30_000 + i * 1500, None)).collect();
        assert_eq!(f.len(), 1, "Laufwerk kommt wieder: neue Dateien werden gemeldet");
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
        assert_eq!((f[0].px_breite, f[0].px_hoehe), (Some(1470), Some(896)));
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
