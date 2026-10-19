//! Boarding-Musik (2.1.1 G1–G3, Pflichtenheft und Design vom 19.10.2026):
//! Ablage der eigenen Musik, Dekodieren, Wiedergabe mit eigenem Pegel und
//! Pause bei jeder Ansage.
//!
//! WANN Musik spielt und WAS, entscheidet `kabinenansagen` je Takt (mit
//! `cabin_announcements::musik`); hier liegt nur, was sie dann tut:
//! - Ablage `Dokumente/AeroACARS/Boarding-Musik/<ICAO>/` bzw. `Alle/` —
//!   außerhalb des App-Ordners, übersteht Update und Neuinstallation.
//! - ein eigener cpal-Strom auf dem gewählten Ausgabegerät mit eigenem Pegel
//!   (ab Werk 20 %), streamend dekodiert mit symphonia (nie die ganze Datei
//!   im Speicher).
//! - Läuft eine Ansage (`AnsageLaeuft`, gesetzt in `ausgabe::spielen`),
//!   blendet die Musik aus und hält an; danach geht sie weiter.
//! - Ein Lebenszeichen je Takt: Bleibt es aus (Flug beendet, App ohne
//!   Takt), endet die Musik von selbst.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cabin_announcements::musik;
use serde::Serialize;

use crate::UiError;

// ---------------------------------------------------------------------------
// Pegel und Ansage-Zähler
// ---------------------------------------------------------------------------

/// Musik-Lautstärke 0 … 1 als f32-Bits; ab Werk 20 % (Entscheidung 14).
static LAUTSTAERKE: AtomicU32 = AtomicU32::new(0x3E4C_CCCD); // 0.2

pub(crate) fn lautstaerke_setzen(v: f32) {
    let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.2 };
    LAUTSTAERKE.store(v.to_bits(), Ordering::Relaxed);
}

pub(crate) fn lautstaerke() -> f32 {
    f32::from_bits(LAUTSTAERKE.load(Ordering::Relaxed))
}

/// Laufende Ansagen (Ton oder Gong) und Ende der letzten (ms seit 1970).
static ANSAGEN: AtomicUsize = AtomicUsize::new(0);
static ANSAGE_ENDE_MS: AtomicU64 = AtomicU64::new(0);
/// Nach einer Ansage so lange still: Zwischen den Sprachen einer Ansage
/// liegen 0,7 s Pause, die Musik soll dort nicht kurz hochkommen.
const NACHLAUF_MS: u64 = 1_500;

fn jetzt_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Solange es lebt, läuft eine Ansage: Die Musik blendet aus und wartet.
pub(crate) struct AnsageLaeuft;

impl AnsageLaeuft {
    pub(crate) fn neu() -> Self {
        ANSAGEN.fetch_add(1, Ordering::SeqCst);
        AnsageLaeuft
    }
}

impl Drop for AnsageLaeuft {
    fn drop(&mut self) {
        ANSAGE_ENDE_MS.store(jetzt_ms(), Ordering::SeqCst);
        ANSAGEN.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Hält gerade eine Ansage die Musik an?
pub(crate) fn ansage_haelt(jetzt: u64) -> bool {
    ANSAGEN.load(Ordering::SeqCst) > 0 || jetzt.saturating_sub(ANSAGE_ENDE_MS.load(Ordering::SeqCst)) < NACHLAUF_MS
}

// ---------------------------------------------------------------------------
// Ablage
// ---------------------------------------------------------------------------

/// Höchstgröße einer eigenen Datei (Design: 50 MB).
pub(crate) const MAX_BYTES: u64 = 50 * 1024 * 1024;

/// `Dokumente/AeroACARS/Boarding-Musik`.
pub(crate) fn ablage() -> Option<PathBuf> {
    dirs::document_dir().map(|d| d.join("AeroACARS").join("Boarding-Musik"))
}

fn gueltige_datei(p: &Path) -> bool {
    p.is_file() && p.file_name().and_then(|n| n.to_str()).is_some_and(|n| musik::format_erlaubt(n) && !n.starts_with('.'))
}

/// Stücke eines Ordners (Name sortiert).
pub(crate) fn stuecke_in(ordner: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(ordner).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| gueltige_datei(p)).collect();
    v.sort();
    v
}

/// Ordner der Ablage mit mindestens einem Stück.
pub(crate) fn ordner_mit_musik(wurzel: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(wurzel)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter(|n| musik::airline_gueltig(n) && !stuecke_in(&wurzel.join(n)).is_empty())
        .collect();
    v.sort();
    v
}

/// Ist diese Datei vor dem Flug abspielbar? Ganz gelesen (ein OneDrive-
/// Platzhalter wird so geladen) und das erste Stück Ton dekodiert. Ganz
/// dekodiert wird beim Hinzufügen ([`pruefen`]); hier liefe das im Takt des
/// Positions-Streamers. Ist eine Datei erst später kaputt, spielt sie bis
/// dorthin und die nächste folgt — still bleibt es dadurch nicht.
pub(crate) fn abspielbar(p: &Path) -> bool {
    std::fs::read(p).is_ok_and(|d| !d.is_empty()) && Dekoder::oeffnen(p).ok().and_then(|mut d| d.naechstes()).is_some()
}

/// Eigene Musik für diese Airline: Ordner und abspielbare Stücke (der
/// Airline, sonst „Alle“). Wird vor jedem Flug neu gelesen; fehlt eine Datei
/// oder ist sie kaputt, wird sie übersprungen, und bleibt keine übrig, kommt
/// das Lied (Design §6).
pub(crate) fn eigene_fuer(wurzel: &Path, airline_icao: &str) -> Option<(String, Vec<PathBuf>)> {
    let mut ordner = ordner_mit_musik(wurzel);
    loop {
        let o = musik::eigener_ordner(airline_icao, &ordner)?.to_string();
        let stuecke: Vec<PathBuf> = stuecke_in(&wurzel.join(&o)).into_iter().filter(|p| abspielbar(p)).collect();
        if !stuecke.is_empty() {
            return Some((o, stuecke));
        }
        // Nichts Gültiges in diesem Ordner: den nächsten („Alle“) versuchen.
        ordner.retain(|x| *x != o);
    }
}

/// Freier Dateiname im Ordner: Originalname, sonst „Name (2).mp3“ …
fn freier_name(ordner: &Path, name: &str) -> String {
    if !ordner.join(name).exists() {
        return name.to_string();
    }
    let (stamm, endung) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..1000)
        .map(|n| if endung.is_empty() { format!("{stamm} ({n})") } else { format!("{stamm} ({n}).{endung}") })
        .find(|k| !ordner.join(k).exists())
        .unwrap_or_else(|| name.to_string())
}

/// Ein Stück der Ablage für den Dialog.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Stueck {
    pub airline: String,
    pub datei: String,
    pub dauer_s: Option<f64>,
}

/// Prüft eine Datei vor dem Speichern: Format, Größe, abspielbar (ganz
/// gelesen, damit ein OneDrive-Platzhalter geladen wird). Liefert die Länge.
pub(crate) fn pruefen(pfad: &Path) -> Result<Option<f64>, UiError> {
    let name = pfad.file_name().and_then(|n| n.to_str()).unwrap_or_default();
    if !musik::format_erlaubt(name) {
        return Err(UiError::new("musik_format", "unsupported format"));
    }
    let groesse = std::fs::metadata(pfad).map_err(|e| UiError::new("musik_lesen", e.to_string()))?.len();
    if groesse > MAX_BYTES {
        return Err(UiError::new("musik_zu_gross", "file larger than 50 MB"));
    }
    // Ganz dekodieren (Design §4): Eine Datei, die erst in der Mitte kaputt
    // ist, bräche sonst mitten im Boarding ab.
    let mut dek = Dekoder::oeffnen(pfad).map_err(|e| UiError::new("musik_kaputt", e))?;
    let mut rahmen = 0u64;
    let mut rate = 0u32;
    while let Some((r, k, proben)) = dek.naechstes() {
        rate = r;
        rahmen += (proben.len() / k.max(1)) as u64;
    }
    // Beim Hinzufügen streng: jedes kaputte Paket lehnt die Datei ab. Beim
    // Spielen wird es nur übersprungen.
    if rahmen == 0 || dek.abgebrochen || dek.kaputte_pakete > 0 {
        return Err(UiError::new("musik_kaputt", "no audio or broken stream"));
    }
    let gelesen = rahmen as f64 / f64::from(rate.max(1));
    // Weniger als 90 % der angegebenen Länge: abgeschnitten.
    if dek.dauer_s().is_some_and(|d| gelesen < d * 0.9) {
        return Err(UiError::new("musik_kaputt", "truncated"));
    }
    Ok(dek.dauer_s().or(Some(gelesen)))
}

/// Kopiert eine Datei in die Ablage (Zwischendatei, dann umbenennen — nie
/// halb gespeichert). Das Original bleibt unangetastet.
pub(crate) fn hinzufuegen(wurzel: &Path, quelle: &Path, airline: &str) -> Result<Stueck, UiError> {
    if !musik::airline_gueltig(airline) {
        return Err(UiError::new("musik_airline", "invalid airline"));
    }
    let dauer_s = pruefen(quelle)?;
    let name = quelle.file_name().and_then(|n| n.to_str()).ok_or_else(|| UiError::new("musik_format", "no file name"))?;
    let ordner = wurzel.join(airline);
    std::fs::create_dir_all(&ordner).map_err(|e| UiError::new("musik_ordner", e.to_string()))?;
    let ziel = freier_name(&ordner, name);
    let zwischen = ordner.join(format!(".{ziel}.neu"));
    let ergebnis = std::fs::copy(quelle, &zwischen).and_then(|_| std::fs::rename(&zwischen, ordner.join(&ziel)));
    if let Err(e) = ergebnis {
        let _ = std::fs::remove_file(&zwischen);
        return Err(UiError::new("musik_ordner", e.to_string()));
    }
    Ok(Stueck { airline: airline.to_string(), datei: ziel, dauer_s })
}

/// Pfad eines Stücks der Ablage, nur innerhalb der Ablage.
pub(crate) fn pfad_von(wurzel: &Path, airline: &str, datei: &str) -> Option<PathBuf> {
    let sauber = musik::airline_gueltig(airline)
        && !datei.is_empty()
        && !datei.contains(['/', '\\'])
        && !datei.starts_with('.')
        && musik::format_erlaubt(datei);
    sauber.then(|| wurzel.join(airline).join(datei)).filter(|p| p.is_file())
}

// ---------------------------------------------------------------------------
// Befehle für den Dialog „Eigene Boarding-Musik“ (Design §3–§6)
// ---------------------------------------------------------------------------

/// Die Ablage für den Dialog: Ordner, Stücke mit Länge, Airline des Flugs.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Ablage {
    ordner: String,
    stuecke: Vec<Stueck>,
    flug_airline: Option<String>,
}

fn wurzel() -> Result<PathBuf, UiError> {
    ablage().ok_or_else(|| UiError::new("musik_ordner", "no documents folder"))
}

/// Liest die Ablage neu (bei jedem Öffnen des Dialogs, Design §6).
#[tauri::command]
pub(crate) async fn kabinen_musik_ablage(state: tauri::State<'_, crate::AppState>) -> Result<Ablage, UiError> {
    let flug_airline = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .map(|f| f.airline_icao.trim().to_ascii_uppercase())
        .filter(|a| musik::airline_gueltig(a));
    let w = wurzel()?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut stuecke = Vec::new();
        for o in std::fs::read_dir(&w).into_iter().flatten().flatten().filter(|e| e.path().is_dir()) {
            let Some(airline) = o.file_name().to_str().map(str::to_string).filter(|n| musik::airline_gueltig(n)) else { continue };
            for p in stuecke_in(&o.path()) {
                let dauer_s = Dekoder::oeffnen(&p).ok().and_then(|d| d.dauer_s());
                let datei = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
                stuecke.push(Stueck { airline: airline.clone(), datei, dauer_s });
            }
        }
        stuecke.sort_by(|a, b| (a.airline.as_str(), a.datei.as_str()).cmp(&(b.airline.as_str(), b.datei.as_str())));
        Ablage { ordner: w.to_string_lossy().to_string(), stuecke, flug_airline }
    })
    .await
    .map_err(|e| UiError::new("musik_ordner", e.to_string()))
}

/// Prüft eine gewählte Datei vor dem Hinzufügen (abspielbar, Größe, Länge).
#[tauri::command]
pub(crate) async fn kabinen_musik_pruefen(quelle: String) -> Result<Option<f64>, UiError> {
    tauri::async_runtime::spawn_blocking(move || pruefen(Path::new(&quelle)))
        .await
        .map_err(|e| UiError::new("musik_lesen", e.to_string()))?
}

/// Kopiert die Datei in den Ordner der Airline (oder „Alle“).
#[tauri::command]
pub(crate) async fn kabinen_musik_hinzufuegen(quelle: String, airline: String) -> Result<Stueck, UiError> {
    let w = wurzel()?;
    tauri::async_runtime::spawn_blocking(move || hinzufuegen(&w, Path::new(&quelle), &airline))
        .await
        .map_err(|e| UiError::new("musik_ordner", e.to_string()))?
}

/// Löscht nur die Kopie in der Ablage (Design §6).
#[tauri::command]
pub(crate) fn kabinen_musik_entfernen(airline: String, datei: String) -> Result<(), UiError> {
    let p = pfad_von(&wurzel()?, &airline, &datei).ok_or_else(|| UiError::new("musik_unbekannt", "unknown file"))?;
    std::fs::remove_file(p).map_err(|e| UiError::new("musik_ordner", e.to_string()))
}

#[tauri::command]
pub(crate) fn kabinen_musik_ordner_oeffnen(app: tauri::AppHandle) -> Result<(), UiError> {
    use tauri_plugin_opener::OpenerExt;
    let w = wurzel()?;
    std::fs::create_dir_all(&w).map_err(|e| UiError::new("musik_ordner", e.to_string()))?;
    app.opener().open_path(w.to_string_lossy(), None::<&str>).map_err(|e| UiError::new("musik_ordner", e.to_string()))
}

/// Probehören eines Stücks der Ablage (einmal, mit dem Musik-Pegel).
#[tauri::command]
pub(crate) fn kabinen_musik_probe(app: tauri::AppHandle, airline: String, datei: String) -> Result<(), UiError> {
    let p = pfad_von(&wurzel()?, &airline, &datei).ok_or_else(|| UiError::new("musik_unbekannt", "unknown file"))?;
    let geraet = crate::kabinenansagen::einstellungen(&app).ausgabe;
    spielen(Auftrag { dateien: vec![p], schleife: false, geraet });
    Ok(())
}

/// Probehören beenden (die Musik eines Flugs läuft im nächsten Takt weiter).
#[tauri::command]
pub(crate) fn kabinen_musik_probe_stopp() {
    if probe_aktiv() {
        aus();
    }
}

/// Eine Airline der VA für die Auswahl „Für welche Airline“.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AirlineEintrag {
    icao: String,
    name: String,
}

static AIRLINES: Mutex<Option<Vec<AirlineEintrag>>> = Mutex::new(None);

/// Aktive Airlines der VA (phpVMS `/api/airlines`, alle Seiten), einmal je
/// Sitzung geholt.
#[tauri::command]
pub(crate) async fn kabinen_airlines(state: tauri::State<'_, crate::AppState>) -> Result<Vec<AirlineEintrag>, UiError> {
    if let Some(v) = AIRLINES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone() {
        return Ok(v);
    }
    let client = state.client.lock().expect("client lock").clone().ok_or_else(|| UiError::new("nicht_angemeldet", "not logged in"))?;
    let mut alle = Vec::new();
    for seite in 1..=40 {
        let liste = client.get_airlines(seite).await.map_err(|e| UiError::new("airlines", e.to_string()))?;
        if liste.is_empty() {
            break;
        }
        alle.extend(liste.into_iter().filter(|a| musik::airline_gueltig(a.icao.trim())).map(|a| AirlineEintrag { icao: a.icao.trim().to_string(), name: a.name }));
    }
    alle.sort_by(|a, b| a.icao.cmp(&b.icao));
    alle.dedup_by(|a, b| a.icao == b.icao);
    *AIRLINES.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(alle.clone());
    Ok(alle)
}

// ---------------------------------------------------------------------------
// Dekodieren und Umrechnen
// ---------------------------------------------------------------------------

/// Streamender Dekoder: liefert Paket für Paket (verschachtelte f32).
pub(crate) struct Dekoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    dekoder: Box<dyn symphonia::core::codecs::Decoder>,
    spur: u32,
    dauer_s: Option<f64>,
    /// Der Strom brach mit einem Fehler ab (nicht am Dateiende).
    pub(crate) abgebrochen: bool,
    /// Pakete, die sich nicht dekodieren ließen (beim Spielen übersprungen).
    pub(crate) kaputte_pakete: u32,
}

impl Dekoder {
    pub(crate) fn oeffnen(pfad: &Path) -> Result<Dekoder, String> {
        use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
        use symphonia::core::formats::FormatOptions;
        use symphonia::core::io::MediaSourceStream;
        use symphonia::core::meta::MetadataOptions;
        use symphonia::core::probe::Hint;
        let datei = std::fs::File::open(pfad).map_err(|e| e.to_string())?;
        let mss = MediaSourceStream::new(Box::new(datei), Default::default());
        let mut hint = Hint::new();
        if let Some(e) = pfad.extension().and_then(|e| e.to_str()) {
            hint.with_extension(e);
        }
        let probe = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| e.to_string())?;
        let format = probe.format;
        let spur = format.tracks().iter().find(|t| t.codec_params.codec != CODEC_TYPE_NULL).ok_or("keine Tonspur")?;
        let p = &spur.codec_params;
        let dauer_s = p.n_frames.zip(p.sample_rate).filter(|(_, r)| *r > 0).map(|(n, r)| n as f64 / f64::from(r));
        let dekoder = symphonia::default::get_codecs().make(p, &DecoderOptions::default()).map_err(|e| e.to_string())?;
        Ok(Dekoder { spur: spur.id, format, dekoder, dauer_s, abgebrochen: false, kaputte_pakete: 0 })
    }

    pub(crate) fn dauer_s(&self) -> Option<f64> {
        self.dauer_s
    }

    /// Nächstes Paket: (Rate, Kanäle, Proben verschachtelt); `None` = Ende.
    pub(crate) fn naechstes(&mut self) -> Option<(u32, usize, Vec<f32>)> {
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::errors::Error;
        loop {
            let paket = match self.format.next_packet() {
                Ok(p) => p,
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return None,
                Err(Error::ResetRequired) => return None,
                Err(_) => {
                    self.abgebrochen = true;
                    return None;
                }
            };
            if paket.track_id() != self.spur {
                continue;
            }
            match self.dekoder.decode(&paket) {
                Ok(puffer) => {
                    let spec = *puffer.spec();
                    if puffer.frames() == 0 {
                        continue;
                    }
                    let mut sb = SampleBuffer::<f32>::new(puffer.capacity() as u64, spec);
                    sb.copy_interleaved_ref(puffer);
                    return Some((spec.rate, spec.channels.count().max(1), sb.samples().to_vec()));
                }
                // Ein kaputtes Paket überspringen, nicht das ganze Stück.
                Err(Error::DecodeError(_)) => {
                    self.kaputte_pakete += 1;
                    continue;
                }
                Err(_) => {
                    self.abgebrochen = true;
                    return None;
                }
            }
        }
    }
}

/// Kanäle und Abtastrate auf das Gerät umrechnen, über Pakete hinweg
/// (linear; für Musik im Hintergrund genügt das).
pub(crate) struct Umrechner {
    nach_rate: u32,
    nach_kanaele: usize,
    /// Letzter Rahmen des vorigen Pakets (schon in Zielkanälen).
    letzter: Option<Vec<f32>>,
    /// Lage des nächsten Ausgangs-Rahmens in Eingangs-Rahmen ab `letzter`.
    lage: f64,
}

impl Umrechner {
    pub(crate) fn neu(nach_rate: u32, nach_kanaele: usize) -> Self {
        Umrechner { nach_rate: nach_rate.max(1), nach_kanaele: nach_kanaele.max(1), letzter: None, lage: 0.0 }
    }

    fn kanaele(&self, rahmen: &[f32]) -> Vec<f32> {
        let k = self.nach_kanaele;
        match rahmen.len() {
            0 => vec![0.0; k],
            1 => vec![rahmen[0]; k],
            _ if k == 1 => vec![rahmen.iter().sum::<f32>() / rahmen.len() as f32],
            n => (0..k).map(|i| if i < n { rahmen[i] } else { 0.0 }).collect(),
        }
    }

    pub(crate) fn umrechnen(&mut self, rate: u32, kanaele: usize, proben: &[f32], aus: &mut VecDeque<f32>) {
        let kanaele = kanaele.max(1);
        let mut rahmen: Vec<Vec<f32>> = Vec::with_capacity(proben.len() / kanaele + 1);
        if let Some(l) = self.letzter.take() {
            rahmen.push(l);
        }
        rahmen.extend(proben.chunks_exact(kanaele).map(|r| self.kanaele(r)));
        if rahmen.is_empty() {
            return;
        }
        let schritt = f64::from(rate.max(1)) / f64::from(self.nach_rate);
        let letzte = rahmen.len() - 1;
        let mut p = self.lage;
        while p < letzte as f64 {
            let i = p as usize;
            let t = (p - i as f64) as f32;
            for c in 0..self.nach_kanaele {
                let a = rahmen[i][c];
                let b = rahmen[(i + 1).min(letzte)][c];
                aus.push_back(a + (b - a) * t);
            }
            p += schritt;
        }
        self.lage = p - letzte as f64;
        self.letzter = rahmen.pop();
    }
}

// ---------------------------------------------------------------------------
// Steuerung
// ---------------------------------------------------------------------------

/// Was spielen soll.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Auftrag {
    pub dateien: Vec<PathBuf>,
    /// Schleife bis zum Ende des Fensters (Flug); sonst einmal (Probehören).
    pub schleife: bool,
    /// Ausgabegerät (`cpal::DeviceId` als Text), `None` = Systemstandard.
    pub geraet: Option<String>,
}

#[derive(Debug, Default)]
struct Steuer {
    auftrag: Option<Auftrag>,
    /// Weich beenden (ausblenden), dann aus.
    ausblenden: bool,
    /// Steigt bei jedem neuen Auftrag und jedem Aus.
    fassung: u64,
}

static STEUER: Mutex<Steuer> = Mutex::new(Steuer { auftrag: None, ausblenden: false, fassung: 0 });
static LEBENSZEICHEN_MS: AtomicU64 = AtomicU64::new(0);
/// Ohne Lebenszeichen so lange (Flug beendet, Takt steht): Musik aus.
const LEBENSZEICHEN_MAX_MS: u64 = 5_000;
/// Gerade hörbar (für die Anzeige „läuft“ / „pausiert für Ansage“).
static HOERBAR: AtomicBool = AtomicBool::new(false);
static FADEN: std::sync::OnceLock<()> = std::sync::OnceLock::new();

fn steuer() -> std::sync::MutexGuard<'static, Steuer> {
    STEUER.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Spielt den Auftrag (gleicher Auftrag: läuft weiter). Hält das
/// Lebenszeichen frisch.
pub(crate) fn spielen(auftrag: Auftrag) {
    LEBENSZEICHEN_MS.store(jetzt_ms(), Ordering::SeqCst);
    {
        let mut s = steuer();
        let gleich = s.auftrag.as_ref().is_some_and(|a| a.dateien == auftrag.dateien && a.schleife == auftrag.schleife);
        if gleich && !s.ausblenden {
            // Nur das Gerät gewechselt: die Wiedergabe öffnet es neu und
            // spielt an derselben Stelle weiter.
            s.auftrag = Some(auftrag);
        } else {
            s.auftrag = Some(auftrag);
            s.ausblenden = false;
            s.fassung += 1;
        }
    }
    FADEN.get_or_init(|| {
        let _ = std::thread::Builder::new().name("boarding-musik".into()).spawn(schleife);
    });
}

/// Weich beenden (ausblenden): „Türen zu“.
pub(crate) fn ausblenden() {
    let mut s = steuer();
    if s.auftrag.is_some() {
        s.ausblenden = true;
    }
}

/// Sofort still: Flugende, Ansagen aus, Stumm, Haken aus.
pub(crate) fn aus() {
    let mut s = steuer();
    if s.auftrag.is_some() {
        s.auftrag = None;
        s.ausblenden = false;
        s.fassung += 1;
    }
}

/// Läuft die Musik eines Flugs (Schleife, auch pausiert)?
pub(crate) fn flug_aktiv() -> bool {
    steuer().auftrag.as_ref().is_some_and(|a| a.schleife)
}

/// Läuft ein Probehören (einmal, ohne Takt)?
pub(crate) fn probe_aktiv() -> bool {
    steuer().auftrag.as_ref().is_some_and(|a| !a.schleife)
}

/// Ist gerade Musik zu hören (nicht pausiert, nicht leise)?
pub(crate) fn hoerbar() -> bool {
    HOERBAR.load(Ordering::Relaxed)
}

/// Gilt der Auftrag dieser Fassung noch? `None` = weiter, sonst warum nicht.
fn noch_gueltig(fassung: u64, schleife: bool) -> bool {
    let s = steuer();
    if s.fassung != fassung || s.auftrag.is_none() {
        return false;
    }
    // Lebenszeichen nur im Flug; das Probehören läuft ohne Takt.
    !(schleife && jetzt_ms().saturating_sub(LEBENSZEICHEN_MS.load(Ordering::SeqCst)) > LEBENSZEICHEN_MAX_MS)
}

fn schleife() {
    loop {
        let (auftrag, fassung) = {
            let s = steuer();
            (s.auftrag.clone(), s.fassung)
        };
        match auftrag {
            Some(a) => {
                if let Err(e) = wiedergabe::abspielen(&a, fassung) {
                    tracing::warn!(error = %e, "boarding-musik: Wiedergabe beendet");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                HOERBAR.store(false, Ordering::Relaxed);
                // Durchgelaufen (einmal) oder abgelaufen: Auftrag dieser Fassung löschen.
                let mut s = steuer();
                if s.fassung == fassung && (!a.schleife || s.ausblenden || !noch_gueltig_ohne_sperre(&s, a.schleife)) {
                    s.auftrag = None;
                    s.ausblenden = false;
                    s.fassung += 1;
                }
            }
            None => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

fn noch_gueltig_ohne_sperre(s: &Steuer, schleife: bool) -> bool {
    s.auftrag.is_some() && !(schleife && jetzt_ms().saturating_sub(LEBENSZEICHEN_MS.load(Ordering::SeqCst)) > LEBENSZEICHEN_MAX_MS)
}

fn ausblenden_gewuenscht(fassung: u64) -> bool {
    let s = steuer();
    s.fassung == fassung && s.ausblenden
}

/// Spielfolge eines Auftrags: Zufallsreihenfolge (Nr. 13), danach von vorn.
fn spielfolge(a: &Auftrag) -> Vec<usize> {
    if a.schleife {
        let saat = a.dateien.iter().map(|p| p.to_string_lossy().len() as u64).sum::<u64>() ^ jetzt_ms();
        musik::reihenfolge(a.dateien.len(), saat)
    } else {
        (0..a.dateien.len()).collect()
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod wiedergabe {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    /// So viel liegt höchstens dekodiert bereit (Sekunden).
    const VORRAT_S: f32 = 1.5;
    /// Ein- und Ausblenden bei Ansagen (s) und am Ende (s).
    const RAMPE_ANSAGE_S: f32 = 0.3;
    const RAMPE_ENDE_S: f32 = 3.0;

    struct Ausgabe {
        _strom: cpal::Stream,
        puffer: Arc<Mutex<VecDeque<f32>>>,
        rate: u32,
        kanaele: usize,
        geraet: Option<String>,
        fehler: Arc<AtomicBool>,
        ende: Arc<AtomicBool>,
        still: Arc<AtomicBool>,
    }

    fn oeffnen(geraet: Option<&str>) -> Result<Ausgabe, String> {
        let host = cpal::default_host();
        let gewaehlt = geraet.and_then(|id| id.parse::<cpal::DeviceId>().ok()).and_then(|id| host.device_by_id(&id));
        let dev = gewaehlt.or_else(|| host.default_output_device()).ok_or("kein Ausgabegeraet")?;
        let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
        let kanaele = usize::from(cfg.channels()).max(1);
        let rate = cfg.sample_rate();
        let puffer = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let fehler = Arc::new(AtomicBool::new(false));
        let ende = Arc::new(AtomicBool::new(false));
        let still = Arc::new(AtomicBool::new(true));

        #[allow(clippy::too_many_arguments)]
        fn bauen<T: cpal::SizedSample + cpal::FromSample<f32>>(
            dev: &cpal::Device,
            cfg: cpal::StreamConfig,
            kanaele: usize,
            puffer: Arc<Mutex<VecDeque<f32>>>,
            fehler: Arc<AtomicBool>,
            ende: Arc<AtomicBool>,
            still: Arc<AtomicBool>,
        ) -> Result<cpal::Stream, String> {
            let rate = cfg.sample_rate.max(1) as f32;
            let mut pegel = 0.0f32;
            let mut versuche = 0u32;
            dev.build_output_stream(
                cfg,
                move |aus: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let halten = ansage_haelt(jetzt_ms());
                    let am_ende = ende.load(Ordering::Relaxed);
                    let ziel = if halten || am_ende { 0.0 } else { lautstaerke() };
                    let schritt = 1.0 / (rate * if am_ende { RAMPE_ENDE_S } else { RAMPE_ANSAGE_S });
                    // Nie blockierend auf den Dekoder warten (Echtzeit-Faden).
                    // Er hält die Sperre nur für ein `extend` (Mikrosekunden):
                    // kurz erneut versuchen, erst dann diesen Block still.
                    let mut q = loop {
                        match puffer.try_lock() {
                            Ok(q) => break Some(q),
                            Err(std::sync::TryLockError::Poisoned(p)) => break Some(p.into_inner()),
                            Err(std::sync::TryLockError::WouldBlock) => {
                                versuche += 1;
                                if versuche > 2_000 {
                                    break None;
                                }
                                std::hint::spin_loop();
                            }
                        }
                    };
                    versuche = 0;
                    let Some(q) = q.as_mut() else {
                        for k in aus.iter_mut() {
                            *k = T::from_sample(0.0);
                        }
                        pegel = 0.0;
                        return;
                    };
                    for rahmen in aus.chunks_mut(kanaele) {
                        pegel = if pegel < ziel { (pegel + schritt).min(ziel) } else { (pegel - schritt).max(ziel) };
                        // Ansage: ausgeblendet anhalten, die Stelle bleibt.
                        if halten && pegel <= 0.0 {
                            for k in rahmen {
                                *k = T::from_sample(0.0);
                            }
                            continue;
                        }
                        for k in rahmen {
                            let x = q.pop_front().unwrap_or(0.0);
                            *k = T::from_sample((x * pegel).clamp(-1.0, 1.0));
                        }
                    }
                    still.store(pegel <= 0.0, Ordering::Relaxed);
                },
                move |e| {
                    tracing::warn!(error = %e, "boarding-musik: Ausgabegeraet meldet einen Fehler");
                    fehler.store(true, Ordering::Relaxed);
                },
                None,
            )
            .map_err(|e| e.to_string())
        }

        let (p, f, e, s, c) = (Arc::clone(&puffer), Arc::clone(&fehler), Arc::clone(&ende), Arc::clone(&still), cfg.config());
        let strom = match cfg.sample_format() {
            cpal::SampleFormat::F32 => bauen::<f32>(&dev, c, kanaele, p, f, e, s),
            cpal::SampleFormat::I16 => bauen::<i16>(&dev, c, kanaele, p, f, e, s),
            cpal::SampleFormat::U16 => bauen::<u16>(&dev, c, kanaele, p, f, e, s),
            cpal::SampleFormat::I32 => bauen::<i32>(&dev, c, kanaele, p, f, e, s),
            andere => return Err(format!("Tonformat {andere:?} nicht unterstuetzt")),
        }?;
        strom.play().map_err(|e| e.to_string())?;
        Ok(Ausgabe { _strom: strom, puffer, rate, kanaele, geraet: geraet.map(str::to_string), fehler, ende, still })
    }

    /// Spielt den Auftrag, bis er zu Ende ist, ersetzt oder beendet wird.
    pub(super) fn abspielen(a: &Auftrag, fassung: u64) -> Result<(), String> {
        if a.dateien.is_empty() {
            return Err("keine Dateien".into());
        }
        let mut ausgabe = oeffnen(a.geraet.as_deref())?;
        let folge = spielfolge(a);
        let mut fehlgeschlagen = 0usize;
        'runden: loop {
            for &i in &folge {
                let datei = &a.dateien[i];
                let mut dek = match Dekoder::oeffnen(datei) {
                    Ok(d) => d,
                    Err(e) => {
                        // Fehlt oder kaputt: überspringen (Design §6).
                        tracing::info!(datei = %datei.display(), error = %e, "boarding-musik: Stueck uebersprungen");
                        fehlgeschlagen += 1;
                        if fehlgeschlagen >= a.dateien.len() {
                            return Err("kein Stueck abspielbar".into());
                        }
                        continue;
                    }
                };
                fehlgeschlagen = 0;
                let mut um = Umrechner::neu(ausgabe.rate, ausgabe.kanaele);
                loop {
                    if !noch_gueltig(fassung, a.schleife) {
                        return Ok(());
                    }
                    if ausblenden_gewuenscht(fassung) {
                        ausgabe.ende.store(true, Ordering::Relaxed);
                        // Ausklingen lassen, dann aus.
                        let bis = std::time::Instant::now() + std::time::Duration::from_secs_f32(RAMPE_ENDE_S + 0.5);
                        while std::time::Instant::now() < bis && !ausgabe.still.load(Ordering::Relaxed) && noch_gueltig(fassung, a.schleife) {
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        return Ok(());
                    }
                    // Gerät gewechselt oder abgezogen: neu öffnen, Stelle verloren
                    // ist hinnehmbar (Hintergrundmusik).
                    let gewuenscht = steuer().auftrag.as_ref().and_then(|x| x.geraet.clone());
                    if ausgabe.fehler.load(Ordering::Relaxed) || gewuenscht != ausgabe.geraet {
                        ausgabe = oeffnen(gewuenscht.as_deref())?;
                        um = Umrechner::neu(ausgabe.rate, ausgabe.kanaele);
                    }
                    HOERBAR.store(!ausgabe.still.load(Ordering::Relaxed), Ordering::Relaxed);
                    let vorrat = ausgabe.puffer.lock().map(|q| q.len()).unwrap_or(0);
                    if vorrat as f32 >= VORRAT_S * ausgabe.rate as f32 * ausgabe.kanaele as f32 {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    match dek.naechstes() {
                        Some((rate, k, proben)) => {
                            // Umrechnen ohne Sperre, dann kurz anhängen.
                            let mut neu = VecDeque::new();
                            um.umrechnen(rate, k, &proben, &mut neu);
                            ausgabe.puffer.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(neu);
                        }
                        None => break,
                    }
                }
            }
            if !a.schleife {
                break 'runden;
            }
        }
        // Einmal (Probehören): Rest ausspielen.
        while noch_gueltig(fassung, a.schleife) && ausgabe.puffer.lock().map(|q| !q.is_empty()).unwrap_or(false) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Ok(())
    }
}

/// Linux: Der Client läuft dort nicht, nur der CI-Check.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod wiedergabe {
    use super::*;
    pub(super) fn abspielen(_: &Auftrag, _: u64) -> Result<(), String> {
        let _ = (ausblenden_gewuenscht(0), noch_gueltig(0, false), spielfolge as fn(&Auftrag) -> Vec<usize>);
        Err("nur unter Windows und macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, kanaele: u16, proben: &[i16]) -> Vec<u8> {
        let daten: Vec<u8> = proben.iter().flat_map(|p| p.to_le_bytes()).collect();
        let mut d = Vec::new();
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&(36 + daten.len() as u32).to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&kanaele.to_le_bytes());
        d.extend_from_slice(&rate.to_le_bytes());
        d.extend_from_slice(&(rate * u32::from(kanaele) * 2).to_le_bytes());
        d.extend_from_slice(&(kanaele * 2).to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(b"data");
        d.extend_from_slice(&(daten.len() as u32).to_le_bytes());
        d.extend_from_slice(&daten);
        d
    }

    fn ordner(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("aeroacars-musik-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn dekoder_liest_wav_mit_laenge() {
        let d = ordner("dek");
        let p = d.join("ton.wav");
        let proben: Vec<i16> = (0..44_100).map(|i| ((i as f32 * 0.05).sin() * 8_000.0) as i16).collect();
        std::fs::write(&p, wav(44_100, 1, &proben)).unwrap();
        let mut dek = Dekoder::oeffnen(&p).expect("wav");
        assert!((dek.dauer_s().expect("Laenge") - 1.0).abs() < 0.01);
        let mut n = 0;
        while let Some((rate, k, s)) = dek.naechstes() {
            assert_eq!((rate, k), (44_100, 1));
            n += s.len();
        }
        assert_eq!(n, 44_100);
        assert_eq!(pruefen(&p).unwrap().map(|s| s.round()), Some(1.0));
    }

    #[test]
    fn kaputte_oder_falsche_datei_wird_abgelehnt() {
        let d = ordner("kaputt");
        let p = d.join("kaputt.mp3");
        std::fs::write(&p, b"kein mp3, nur Text").unwrap();
        assert_eq!(pruefen(&p).unwrap_err().code, "musik_kaputt");
        let b = d.join("bild.png");
        std::fs::write(&b, b"x").unwrap();
        assert_eq!(pruefen(&b).unwrap_err().code, "musik_format");
    }

    /// Design §4: kopieren (Original bleibt), Unterordner je Airline, gleicher
    /// Name „ (2)“, keine halben Dateien, nur gültige Ordner.
    #[test]
    fn hinzufuegen_kopiert_in_den_airline_ordner() {
        let d = ordner("ablage");
        let quelle = d.join("Mein Lied.wav");
        std::fs::write(&quelle, wav(22_050, 2, &[0, 0, 1000, -1000, 0, 0])).unwrap();
        let wurzel = d.join("Boarding-Musik");
        let a = hinzufuegen(&wurzel, &quelle, "DLH").unwrap();
        let b = hinzufuegen(&wurzel, &quelle, "DLH").unwrap();
        assert_eq!((a.datei.as_str(), b.datei.as_str()), ("Mein Lied.wav", "Mein Lied (2).wav"));
        assert!(quelle.exists(), "Original bleibt");
        assert!(hinzufuegen(&wurzel, &quelle, "../x").is_err());
        hinzufuegen(&wurzel, &quelle, musik::ALLE).unwrap();
        assert_eq!(ordner_mit_musik(&wurzel), vec!["Alle".to_string(), "DLH".to_string()]);
        assert_eq!(eigene_fuer(&wurzel, "DLH").map(|(o, s)| (o, s.len())), Some(("DLH".into(), 2)));
        assert_eq!(eigene_fuer(&wurzel, "KLM").map(|(o, _)| o), Some("Alle".into()));
        assert!(pfad_von(&wurzel, "DLH", "../Alle/Mein Lied.wav").is_none());
        assert!(pfad_von(&wurzel, "DLH", "Mein Lied.wav").is_some());
        assert!(std::fs::read_dir(wurzel.join("DLH")).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().ends_with(".neu")));
    }

    /// Codex 2.1.1: Eine erst später kaputte (abgeschnittene) Datei wird beim
    /// Hinzufügen abgelehnt; eine kaputte in der Ablage gilt nicht als eigene
    /// Musik, dann kommt „Alle“ bzw. das Lied.
    #[test]
    fn abgeschnittene_und_kaputte_dateien_zaehlen_nicht() {
        let d = ordner("schnitt");
        let ganz = wav(8_000, 1, &vec![1000i16; 8_000 * 4]);
        // Kopf sagt 4 s, Daten reichen nur 1 s.
        let kurz = ganz[..44 + 8_000 * 2].to_vec();
        let p = d.join("abgeschnitten.wav");
        std::fs::write(&p, &kurz).unwrap();
        assert_eq!(pruefen(&p).unwrap_err().code, "musik_kaputt");
        let wurzel = d.join("Boarding-Musik");
        std::fs::create_dir_all(wurzel.join("DLH")).unwrap();
        std::fs::create_dir_all(wurzel.join("Alle")).unwrap();
        std::fs::write(wurzel.join("DLH").join("kaputt.mp3"), b"kein mp3").unwrap();
        assert_eq!(eigene_fuer(&wurzel, "DLH"), None, "nur Kaputtes: keine eigene Musik → Lied");
        std::fs::write(wurzel.join("Alle").join("gut.wav"), &ganz).unwrap();
        assert_eq!(eigene_fuer(&wurzel, "DLH").map(|(o, s)| (o, s.len())), Some(("Alle".into(), 1)));
    }

    #[test]
    fn umrechner_haelt_laenge_und_kanaele_ueber_pakete() {
        // 1 s Mono 44,1 kHz in zwei Paketen → 48 kHz Stereo.
        let mut u = Umrechner::neu(48_000, 2);
        let mut aus = VecDeque::new();
        let proben: Vec<f32> = (0..44_100).map(|i| i as f32 / 44_100.0).collect();
        u.umrechnen(44_100, 1, &proben[..20_000], &mut aus);
        u.umrechnen(44_100, 1, &proben[20_000..], &mut aus);
        let rahmen = aus.len() / 2;
        assert!((rahmen as i64 - 48_000).abs() <= 2, "{rahmen}");
        let v: Vec<f32> = aus.iter().copied().collect();
        assert!(v.chunks(2).all(|r| r[0] == r[1]), "Mono auf beide Kanäle");
        assert!(v.chunks(2).map(|r| r[0]).collect::<Vec<_>>().windows(2).all(|w| w[1] >= w[0] - 1e-6), "stetig über die Paketgrenze");
        // Stereo → Mono mittelt.
        let mut m = Umrechner::neu(10, 1);
        let mut aus = VecDeque::new();
        m.umrechnen(10, 2, &[1.0, 0.0, 1.0, 0.0, 1.0, 0.0], &mut aus);
        assert!(aus.iter().all(|x| (*x - 0.5).abs() < 1e-6));
    }

    #[test]
    fn ansage_haelt_die_musik_mit_nachlauf() {
        let jetzt = jetzt_ms();
        {
            let _a = AnsageLaeuft::neu();
            assert!(ansage_haelt(jetzt));
        }
        assert!(ansage_haelt(jetzt_ms()), "kurz nach dem Ende noch still (Pause zwischen den Sprachen)");
        assert!(!ansage_haelt(jetzt_ms() + NACHLAUF_MS + 10));
    }

    /// Von Hand: echte Datei dekodieren und hörbar spielen (5 s, dann aus).
    /// `MUSIK_PROBE=/pfad/lied.mp3 cargo test -p aeroacars-app echte_datei -- --ignored`
    #[test]
    #[ignore]
    fn echte_datei_dekodieren_und_spielen() {
        let p = PathBuf::from(std::env::var("MUSIK_PROBE").expect("MUSIK_PROBE"));
        let dauer = pruefen(&p).expect("abspielbar");
        println!("Laenge laut Kopf: {dauer:?}");
        let mut dek = Dekoder::oeffnen(&p).unwrap();
        let (rate, k, _) = dek.naechstes().unwrap();
        println!("Rate {rate}, Kanaele {k}");
        spielen(Auftrag { dateien: vec![p], schleife: false, geraet: None });
        std::thread::sleep(std::time::Duration::from_secs(4));
        println!("hoerbar: {}", hoerbar());
        {
            let _ansage = AnsageLaeuft::neu();
            std::thread::sleep(std::time::Duration::from_secs(1));
            println!("waehrend Ansage hoerbar: {}", hoerbar());
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
        println!("nach Ansage hoerbar: {}", hoerbar());
        aus();
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    #[test]
    fn eigener_pegel_ab_werk_zwanzig_prozent() {
        assert!((f32::from_bits(0x3E4C_CCCD) - 0.2).abs() < 1e-6);
        lautstaerke_setzen(f32::NAN);
        assert!((lautstaerke() - 0.2).abs() < 1e-6);
        lautstaerke_setzen(3.0);
        assert_eq!(lautstaerke(), 1.0);
        lautstaerke_setzen(0.2);
    }
}
