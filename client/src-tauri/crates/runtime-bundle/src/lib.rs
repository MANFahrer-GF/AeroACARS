//! Laufzeit-Paket von AeroACARS 3.0 (Pflichtenheft Kap. 3, 5.1).
//!
//! Ein Paket ist eine **Generation**: ein signiertes Manifest und Dateien, die
//! nach ihrem SHA-256 abgelegt sind. Dieses Crate kann ein Paket pruefen,
//! installieren, laden (mit Rueckfall) und aufraeumen. Netz, Pfade der App und
//! der Inhalt (OurAirports, Profile, Module) gehoeren nicht hierher; der
//! Aufrufer reicht Bytes und Holfunktionen herein. So bleibt alles ohne
//! Simulator und ohne Netz testbar.
//!
//! Ablage auf der Platte:
//! ```text
//! <wurzel>/blobs/<sha256>            Dateiinhalt
//! <wurzel>/gen/<generation>/manifest.json(.minisig), installiert
//! <wurzel>/zeiger/aktuell, vorher    Generationsnummer als Text
//! <wurzel>/tmp/                      Zwischenstand, nie gelesen
//! ```

pub mod bauen;
pub mod manifest;

use std::collections::{BTreeSet, HashMap};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use manifest::{Datei, Manifest};

#[derive(Debug, thiserror::Error)]
pub enum Fehler {
    #[error("Signatur ungueltig")]
    Signatur,
    #[error("Manifest ungueltig: {0}")]
    Manifest(String),
    #[error("Manifest-Schema {0} wird nicht unterstuetzt")]
    Schema(u32),
    #[error("Paket passt nicht zu diesem Programm: {0}")]
    Unpassend(String),
    #[error("Generation {neu} ist nicht neuer als die aktive {aktiv}")]
    Veraltet { neu: u64, aktiv: u64 },
    #[error("Paket abgelaufen ({0})")]
    Abgelaufen(DateTime<Utc>),
    #[error("Datei {pfad}: {grund}")]
    Datei { pfad: String, grund: String },
    #[error("Selbsttest fehlgeschlagen: {0}")]
    Selbsttest(String),
    #[error("Ablage: {0}")]
    Ablage(#[from] std::io::Error),
}

/// Kennung eines Stands, die je Flug festgehalten wird (P-3.30, N-02).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuntimeBundleId {
    pub generation: u64,
    pub manifest_sha256: String,
}

impl fmt::Display for RuntimeBundleId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.generation, &self.manifest_sha256[..12.min(self.manifest_sha256.len())])
    }
}

/// Was der Client ueber sich selbst weiss.
#[derive(Debug, Clone)]
pub struct Kontext {
    pub client_version: String,
    /// Faehigkeiten des Programms, z. B. `wasm-host-1` (P-2.15).
    pub faehigkeiten: Vec<String>,
    /// Pruefschluessel (minisign, Base64), aktiv und Reserve (P-3.10).
    pub schluessel: Vec<String>,
    pub jetzt: DateTime<Utc>,
}

/// Ein Paket zum Installieren, wie es vom Server kommt.
pub struct Eingang<'a> {
    pub manifest: &'a [u8],
    pub signatur: &'a str,
}

/// Der eingebaute Stand (P-3.22): dieselben Dateien, im Programm abgelegt.
///
/// Ohne Signatur: Er ist Teil des Programms, und das Programm selbst ist
/// ueber den Updater signiert (Klasse A). Die Hashes werden trotzdem geprueft.
pub struct Eingebaut {
    pub manifest: Vec<u8>,
    pub dateien: HashMap<String, Vec<u8>>,
}

/// Woher der geladene Stand kommt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Herkunft {
    /// Der Stand, auf den der offene Flug verweist.
    Pin,
    Aktuell,
    /// Der vorige gute Stand, weil der aktuelle nicht taugte.
    Vorher,
    Eingebaut,
    /// Der Flug verweist auf einen Stand, der lokal fehlt; bis er geholt ist,
    /// gilt der eingebaute als „vorlaeufig“ (P-3.31).
    EingebautStattPin,
}

enum Inhalt {
    Ablage(PathBuf),
    Speicher(HashMap<String, Vec<u8>>),
}

/// Ein geprueftes, geladenes Paket.
pub struct Paket {
    pub manifest: Manifest,
    pub id: RuntimeBundleId,
    inhalt: Inhalt,
}

impl Paket {
    /// Inhalt einer Datei; der Hash wird bei jedem Lesen geprueft.
    pub fn datei(&self, pfad: &str) -> Result<Vec<u8>, Fehler> {
        let d = self.manifest.datei(pfad).ok_or_else(|| Fehler::Datei {
            pfad: pfad.into(),
            grund: "nicht im Manifest".into(),
        })?;
        let bytes = match &self.inhalt {
            Inhalt::Ablage(blobs) => fs::read(blobs.join(&d.sha256)).map_err(|e| Fehler::Datei {
                pfad: pfad.into(),
                grund: e.to_string(),
            })?,
            Inhalt::Speicher(m) => m.get(pfad).cloned().ok_or_else(|| Fehler::Datei {
                pfad: pfad.into(),
                grund: "fehlt im eingebauten Stand".into(),
            })?,
        };
        datei_pruefen(d, &bytes)?;
        Ok(bytes)
    }

    fn alle_pruefen(&self) -> Result<(), Fehler> {
        for d in &self.manifest.files {
            self.datei(&d.path)?;
        }
        Ok(())
    }
}

pub struct Ladeergebnis {
    pub paket: Paket,
    pub herkunft: Herkunft,
    /// Was unterwegs nicht taugte, fuer Log und Diagnose.
    pub hinweise: Vec<String>,
}

pub struct Ablage {
    wurzel: PathBuf,
}

const BEHALTEN_TAGE: i64 = 30;

/// Beide Zeiger in einer Datei: Sie werden immer gemeinsam und atomar
/// geschrieben, ein Absturz kann nie nur einen von beiden aendern.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Zeiger {
    aktuell: Option<u64>,
    vorher: Option<u64>,
}

impl Ablage {
    pub fn oeffnen(wurzel: impl Into<PathBuf>) -> Result<Self, Fehler> {
        let wurzel = wurzel.into();
        for d in ["blobs", "gen", "zeiger", "tmp"] {
            fs::create_dir_all(wurzel.join(d))?;
        }
        Ok(Self { wurzel })
    }

    /// Aktive Generation; `None` auch dann, wenn die Zeigerdatei kaputt ist.
    pub fn aktuell(&self) -> Option<u64> {
        self.zeiger().ok().and_then(|z| z.aktuell)
    }

    pub fn vorher(&self) -> Option<u64> {
        self.zeiger().ok().and_then(|z| z.vorher)
    }

    /// Dateien eines Manifests, die lokal fehlen oder kaputt sind — nur die
    /// muessen geholt werden (nach Inhalt adressiert, P-3.01).
    pub fn fehlende(&self, m: &Manifest) -> Vec<Datei> {
        m.files
            .iter()
            .filter(|d| {
                !fs::read(self.wurzel.join("blobs").join(&d.sha256))
                    .map(|b| datei_pruefen(d, &b).is_ok())
                    .unwrap_or(false)
            })
            .cloned()
            .collect()
    }

    /// Prueft und installiert einen neuen Stand und macht ihn zum aktuellen.
    ///
    /// Reihenfolge nach P-3.20: Signatur → Manifest → passt zum Programm →
    /// nicht abgelaufen → neuer als der aktive → fehlende Dateien holen und
    /// pruefen → Selbsttest → Zeiger umschalten. Scheitert ein Schritt, bleibt
    /// der bisherige Stand unberuehrt aktiv. Ein zweiter Aufruf (auch aus
    /// einem anderen Prozess) wartet an der Sperre.
    pub fn installieren(
        &self,
        eingang: Eingang<'_>,
        kontext: &Kontext,
        hole: &mut dyn FnMut(&Datei) -> Result<Vec<u8>, String>,
        selbsttest: &dyn Fn(&Paket) -> Result<(), String>,
    ) -> Result<RuntimeBundleId, Fehler> {
        let _sperre = self.sperre(true)?;
        signatur_pruefen(eingang.manifest, eingang.signatur, &kontext.schluessel)?;
        let m = Manifest::lesen(eingang.manifest)?;
        passt(&m, kontext)?;
        if m.expires_at <= kontext.jetzt {
            return Err(Fehler::Abgelaufen(m.expires_at));
        }
        // Kaputter Zeiger: nicht raten, sonst koennte eine alte Generation
        // durchrutschen. Die Wiederherstellung macht das Laden (Rueckfall).
        let zeiger = self.zeiger()?;
        if let Some(aktiv) = zeiger.aktuell {
            if m.generation <= aktiv {
                return Err(Fehler::Veraltet { neu: m.generation, aktiv });
            }
        }
        for d in &m.files {
            let ziel = self.wurzel.join("blobs").join(&d.sha256);
            if fs::read(&ziel).map(|b| datei_pruefen(d, &b).is_ok()).unwrap_or(false) {
                continue;
            }
            let bytes = hole(d).map_err(|grund| Fehler::Datei { pfad: d.path.clone(), grund })?;
            datei_pruefen(d, &bytes)?;
            self.atomar_schreiben(&ziel, &bytes)?;
        }
        let gen_ordner = self.gen_ordner(m.generation);
        let selbst_angelegt = if gen_ordner.exists() {
            // Ein vorhandener Ordner dieser Generation wird nie geloescht: Er
            // koennte gepinnt sein. Nur derselbe Stand darf ihn weiterverwenden.
            if fs::read(gen_ordner.join("manifest.json"))? != eingang.manifest {
                return Err(Fehler::Manifest(format!(
                    "Generation {} liegt schon mit anderem Inhalt vor",
                    m.generation
                )));
            }
            false
        } else {
            let tmp = self.tmp_pfad(&format!("gen-{}", m.generation));
            fs::create_dir_all(&tmp)?;
            dauerhaft_schreiben(&tmp.join("manifest.json"), eingang.manifest)?;
            dauerhaft_schreiben(&tmp.join("manifest.json.minisig"), eingang.signatur.as_bytes())?;
            dauerhaft_schreiben(&tmp.join("installiert"), kontext.jetzt.to_rfc3339().as_bytes())?;
            ordner_sichern(&tmp)?;
            fs::rename(&tmp, &gen_ordner)?;
            ordner_sichern(&self.wurzel.join("gen"))?;
            true
        };

        let paket = self.paket_aus_ablage(m.generation, kontext)?;
        if let Err(grund) = selbsttest(&paket) {
            if selbst_angelegt {
                let _ = fs::remove_dir_all(&gen_ordner);
            }
            return Err(Fehler::Selbsttest(grund));
        }
        self.zeiger_setzen(Zeiger { aktuell: Some(m.generation), vorher: zeiger.aktuell })?;
        Ok(paket.id)
    }

    /// Legt GENAU den Stand `pin` ab, auf den ein offener Flug verweist und der
    /// lokal fehlt (P-3.31). Kein Zeiger wird umgestellt, die Regel „neuer als
    /// der aktive“ und der Ablauf gelten hier nicht (ein alter Stand darf
    /// abgelaufen sein); Signatur, Manifest-Hash = Pin und alle Dateien werden
    /// geprueft. Danach findet `laden(Some(pin))` ihn.
    pub fn nachinstallieren(
        &self,
        eingang: Eingang<'_>,
        pin: &RuntimeBundleId,
        kontext: &Kontext,
        hole: &mut dyn FnMut(&Datei) -> Result<Vec<u8>, String>,
    ) -> Result<RuntimeBundleId, Fehler> {
        let _sperre = self.sperre(true)?;
        if hex(&Sha256::digest(eingang.manifest)) != pin.manifest_sha256 {
            return Err(Fehler::Manifest(format!("Manifest passt nicht zum Pin {pin}")));
        }
        signatur_pruefen(eingang.manifest, eingang.signatur, &kontext.schluessel)?;
        let m = Manifest::lesen(eingang.manifest)?;
        if m.generation != pin.generation {
            return Err(Fehler::Manifest(format!("Generation {} statt {}", m.generation, pin.generation)));
        }
        passt(&m, kontext)?;
        for d in self.fehlende(&m) {
            let bytes = hole(&d).map_err(|grund| Fehler::Datei { pfad: d.path.clone(), grund })?;
            datei_pruefen(&d, &bytes)?;
            self.atomar_schreiben(&self.wurzel.join("blobs").join(&d.sha256), &bytes)?;
        }
        let gen_ordner = self.gen_ordner(m.generation);
        if !gen_ordner.exists() {
            let tmp = self.tmp_pfad(&format!("gen-{}", m.generation));
            fs::create_dir_all(&tmp)?;
            dauerhaft_schreiben(&tmp.join("manifest.json"), eingang.manifest)?;
            dauerhaft_schreiben(&tmp.join("manifest.json.minisig"), eingang.signatur.as_bytes())?;
            dauerhaft_schreiben(&tmp.join("installiert"), kontext.jetzt.to_rfc3339().as_bytes())?;
            ordner_sichern(&tmp)?;
            fs::rename(&tmp, &gen_ordner)?;
            ordner_sichern(&self.wurzel.join("gen"))?;
        }
        Ok(self.paket_aus_ablage(m.generation, kontext)?.id)
    }

    /// Laedt den Stand fuer diesen Programmstart (P-3.20, P-3.31).
    ///
    /// Mit `pin` (offener Flug) gilt nur genau dieser Stand (Generation UND
    /// Manifest-Hash); fehlt er, der eingebaute als `EingebautStattPin`. Ohne
    /// Pin: aktuell → vorher → eingebaut. Ablauf spielt beim Laden keine
    /// Rolle (P-3.04).
    pub fn laden(
        &self,
        pin: Option<&RuntimeBundleId>,
        kontext: &Kontext,
        eingebaut: &Eingebaut,
    ) -> Result<Ladeergebnis, Fehler> {
        let _sperre = self.sperre(false)?;
        let mut hinweise = Vec::new();
        if let Some(pin) = pin {
            match self.paket_aus_ablage(pin.generation, kontext) {
                Ok(p) if &p.id == pin => {
                    return Ok(Ladeergebnis { paket: p, herkunft: Herkunft::Pin, hinweise })
                }
                Ok(p) => hinweise.push(format!("Pin {pin}: lokal liegt {}", p.id)),
                Err(e) => hinweise.push(format!("Pin {pin}: {e}")),
            }
            let paket = eingebautes_paket(eingebaut)?;
            return Ok(Ladeergebnis { paket, herkunft: Herkunft::EingebautStattPin, hinweise });
        }
        let zeiger = match self.zeiger() {
            Ok(z) => z,
            Err(e) => {
                hinweise.push(format!("Zeiger: {e}"));
                Zeiger::default()
            }
        };
        for (g, name, herkunft) in [
            (zeiger.aktuell, "aktuell", Herkunft::Aktuell),
            (zeiger.vorher, "vorher", Herkunft::Vorher),
        ] {
            let Some(g) = g else { continue };
            match self.paket_aus_ablage(g, kontext) {
                Ok(paket) => return Ok(Ladeergebnis { paket, herkunft, hinweise }),
                Err(e) => hinweise.push(format!("{name} ({g}): {e}")),
            }
        }
        let paket = eingebautes_paket(eingebaut)?;
        Ok(Ladeergebnis { paket, herkunft: Herkunft::Eingebaut, hinweise })
    }

    /// Entfernt Staende, die weder aktuell noch vorher noch in `pins` und
    /// aelter als 30 Tage sind, und danach nicht mehr benutzte Dateien
    /// (P-3.41). `pins` sind alle Staende, die gerade jemand braucht: der
    /// offene Flug und der im laufenden Programm geladene.
    ///
    /// Bricht ab, ohne etwas zu loeschen, sobald der Zeiger oder das Manifest
    /// eines behaltenen Stands nicht lesbar ist — sonst gingen genau dessen
    /// Dateien verloren.
    pub fn aufraeumen(&self, pins: &[u64], jetzt: DateTime<Utc>) -> Result<usize, Fehler> {
        let _sperre = self.sperre(true)?;
        let zeiger = self.zeiger()?;
        let mut behalten: BTreeSet<u64> = pins.iter().copied().collect();
        behalten.extend(zeiger.aktuell);
        behalten.extend(zeiger.vorher);
        let mut weg = Vec::new();
        for g in self.generationen()? {
            if behalten.contains(&g) {
                continue;
            }
            let installiert = fs::read_to_string(self.gen_ordner(g).join("installiert"))
                .ok()
                .and_then(|t| DateTime::parse_from_rfc3339(t.trim()).ok())
                .map(|t| t.with_timezone(&Utc));
            match installiert {
                Some(t) if jetzt - t < Duration::days(BEHALTEN_TAGE) => {
                    behalten.insert(g);
                }
                _ => weg.push(g),
            }
        }
        let mut gebraucht = BTreeSet::new();
        for g in &behalten {
            let ordner = self.gen_ordner(*g);
            if !ordner.exists() {
                continue; // gepinnt, aber hier nicht vorhanden: nichts zu schuetzen
            }
            let bytes = fs::read(ordner.join("manifest.json"))?;
            let m: Manifest =
                serde_json::from_slice(&bytes).map_err(|e| Fehler::Manifest(format!("gen {g}: {e}")))?;
            gebraucht.extend(m.files.into_iter().map(|d| d.sha256));
        }
        for g in &weg {
            fs::remove_dir_all(self.gen_ordner(*g))?;
        }
        for eintrag in fs::read_dir(self.wurzel.join("blobs"))? {
            let eintrag = eintrag?;
            let name = eintrag.file_name().to_string_lossy().into_owned();
            let ist_blob = eintrag.file_type()?.is_file()
                && name.len() == 64
                && name.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if ist_blob && !gebraucht.contains(&name) {
                fs::remove_file(eintrag.path())?;
            }
        }
        Ok(weg.len())
    }

    fn generationen(&self) -> Result<Vec<u64>, Fehler> {
        let mut v = Vec::new();
        for e in fs::read_dir(self.wurzel.join("gen"))? {
            if let Ok(g) = e?.file_name().to_string_lossy().parse::<u64>() {
                v.push(g);
            }
        }
        Ok(v)
    }

    fn gen_ordner(&self, g: u64) -> PathBuf {
        self.wurzel.join("gen").join(g.to_string())
    }

    fn paket_aus_ablage(&self, g: u64, kontext: &Kontext) -> Result<Paket, Fehler> {
        let ordner = self.gen_ordner(g);
        let bytes = fs::read(ordner.join("manifest.json"))?;
        let sig = fs::read_to_string(ordner.join("manifest.json.minisig"))?;
        signatur_pruefen(&bytes, &sig, &kontext.schluessel)?;
        let manifest = Manifest::lesen(&bytes)?;
        if manifest.generation != g {
            return Err(Fehler::Manifest(format!("Ordner {g} enthaelt Generation {}", manifest.generation)));
        }
        passt(&manifest, kontext)?;
        let paket = Paket {
            id: RuntimeBundleId { generation: g, manifest_sha256: hex(&Sha256::digest(&bytes)) },
            manifest,
            inhalt: Inhalt::Ablage(self.wurzel.join("blobs")),
        };
        paket.alle_pruefen()?;
        Ok(paket)
    }

    /// Fehlt die Datei: noch nie installiert. Ist sie kaputt: Fehler.
    fn zeiger(&self) -> Result<Zeiger, Fehler> {
        match fs::read(self.wurzel.join("zeiger").join("stand.json")) {
            Ok(b) => serde_json::from_slice(&b).map_err(|e| Fehler::Manifest(format!("Zeiger kaputt: {e}"))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Zeiger::default()),
            Err(e) => Err(e.into()),
        }
    }

    fn zeiger_setzen(&self, z: Zeiger) -> Result<(), Fehler> {
        let bytes = serde_json::to_vec(&z).map_err(|e| Fehler::Manifest(e.to_string()))?;
        self.atomar_schreiben(&self.wurzel.join("zeiger").join("stand.json"), &bytes)?;
        ordner_sichern(&self.wurzel.join("zeiger"))
    }

    /// Exklusiv fuer Installieren und Aufraeumen, geteilt fuer Laden; gilt
    /// auch zwischen Prozessen. Wird beim Fallenlassen der Datei geloest.
    fn sperre(&self, exklusiv: bool) -> Result<fs::File, Fehler> {
        let f = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.wurzel.join("sperre"))?;
        if exklusiv {
            f.lock()?;
        } else {
            f.lock_shared()?;
        }
        Ok(f)
    }

    fn tmp_pfad(&self, name: &str) -> PathBuf {
        static ZAEHLER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = ZAEHLER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.wurzel.join("tmp").join(format!("{name}.{}.{n}", std::process::id()))
    }

    /// Schreiben in eine Zwischendatei und Umbenennen: Ein Absturz mittendrin
    /// hinterlaesst nie eine halbe Datei am Ziel.
    fn atomar_schreiben(&self, ziel: &Path, bytes: &[u8]) -> Result<(), Fehler> {
        let name = ziel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = self.tmp_pfad(&name);
        dauerhaft_schreiben(&tmp, bytes)?;
        fs::rename(&tmp, ziel)?;
        Ok(())
    }
}

fn dauerhaft_schreiben(pfad: &Path, bytes: &[u8]) -> Result<(), Fehler> {
    let mut f = fs::File::create(pfad)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

/// Macht Umbenennungen in einem Ordner dauerhaft (Unix). Unter Windows laesst
/// sich ein Ordner nicht so oeffnen; dort gilt NTFS-Journaling.
fn ordner_sichern(ordner: &Path) -> Result<(), Fehler> {
    #[cfg(unix)]
    fs::File::open(ordner)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = ordner;
    Ok(())
}

impl Eingebaut {
    /// Der eingebaute Stand als Paket, Hashes geprueft.
    pub fn paket(&self) -> Result<Paket, Fehler> {
        eingebautes_paket(self)
    }
}

fn eingebautes_paket(e: &Eingebaut) -> Result<Paket, Fehler> {
    let manifest = Manifest::lesen(&e.manifest)?;
    let paket = Paket {
        id: RuntimeBundleId {
            generation: manifest.generation,
            manifest_sha256: hex(&Sha256::digest(&e.manifest)),
        },
        manifest,
        inhalt: Inhalt::Speicher(e.dateien.clone()),
    };
    paket.alle_pruefen()?;
    Ok(paket)
}

/// Gilt, wenn irgendeiner der Pruefschluessel passt (aktiv oder Reserve).
pub fn signatur_pruefen(daten: &[u8], signatur: &str, schluessel: &[String]) -> Result<(), Fehler> {
    let sig = minisign_verify::Signature::decode(signatur).map_err(|_| Fehler::Signatur)?;
    for k in schluessel {
        if let Ok(pk) = minisign_verify::PublicKey::from_base64(k) {
            if pk.verify(daten, &sig, false).is_ok() {
                return Ok(());
            }
        }
    }
    Err(Fehler::Signatur)
}

/// P-2.30: Wie ein Client mit dieser Fassung und diesen Faehigkeiten ein
/// Paket behandelt (Signatur und Dateien prueft erst die Installation):
/// `Ok` = uebernommen, `Err(Fehler::Unpassend)` = sauber uebergangen (der
/// Client behaelt seinen Stand), jeder andere Fehler = Manifest unlesbar.
pub fn vertraeglich(manifest: &[u8], client_version: &str, faehigkeiten: &[String]) -> Result<(), Fehler> {
    let m = Manifest::lesen(manifest)?;
    let k = Kontext {
        client_version: client_version.to_string(),
        faehigkeiten: faehigkeiten.to_vec(),
        schluessel: Vec::new(),
        jetzt: chrono::Utc::now(),
    };
    passt(&m, &k)
}

fn passt(m: &Manifest, k: &Kontext) -> Result<(), Fehler> {
    let v = manifest::version(&k.client_version)?;
    if v < manifest::version(&m.client_min)? || v > manifest::version(&m.client_max)? {
        return Err(Fehler::Unpassend(format!(
            "Client {} ausserhalb {}..={}",
            k.client_version, m.client_min, m.client_max
        )));
    }
    if let Some(f) = m.requires.iter().find(|r| !k.faehigkeiten.contains(r)) {
        return Err(Fehler::Unpassend(format!("Faehigkeit fehlt: {f}")));
    }
    Ok(())
}

fn datei_pruefen(d: &Datei, bytes: &[u8]) -> Result<(), Fehler> {
    if bytes.len() as u64 != d.size {
        return Err(Fehler::Datei { pfad: d.path.clone(), grund: "Groesse falsch".into() });
    }
    if hex(&Sha256::digest(bytes)) != d.sha256 {
        return Err(Fehler::Datei { pfad: d.path.clone(), grund: "Hash falsch".into() });
    }
    Ok(())
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
