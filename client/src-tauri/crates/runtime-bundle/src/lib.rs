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

impl Ablage {
    pub fn oeffnen(wurzel: impl Into<PathBuf>) -> Result<Self, Fehler> {
        let wurzel = wurzel.into();
        for d in ["blobs", "gen", "zeiger", "tmp"] {
            fs::create_dir_all(wurzel.join(d))?;
        }
        Ok(Self { wurzel })
    }

    pub fn aktuell(&self) -> Option<u64> {
        self.zeiger("aktuell")
    }

    pub fn vorher(&self) -> Option<u64> {
        self.zeiger("vorher")
    }

    /// Prueft und installiert einen neuen Stand und macht ihn zum aktuellen.
    ///
    /// Reihenfolge nach P-3.20: Signatur → Manifest → passt zum Programm →
    /// nicht abgelaufen → neuer als der aktive → fehlende Dateien holen und
    /// pruefen → Selbsttest → Zeiger umschalten. Scheitert ein Schritt, bleibt
    /// der bisherige Stand unberuehrt aktiv.
    pub fn installieren(
        &self,
        eingang: Eingang<'_>,
        kontext: &Kontext,
        hole: &mut dyn FnMut(&Datei) -> Result<Vec<u8>, String>,
        selbsttest: &dyn Fn(&Paket) -> Result<(), String>,
    ) -> Result<RuntimeBundleId, Fehler> {
        signatur_pruefen(eingang.manifest, eingang.signatur, &kontext.schluessel)?;
        let m = Manifest::lesen(eingang.manifest)?;
        passt(&m, kontext)?;
        if m.expires_at <= kontext.jetzt {
            return Err(Fehler::Abgelaufen(m.expires_at));
        }
        if let Some(aktiv) = self.aktuell() {
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
        let tmp = self.wurzel.join("tmp").join(format!("gen-{}", m.generation));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp)?;
        fs::write(tmp.join("manifest.json"), eingang.manifest)?;
        fs::write(tmp.join("manifest.json.minisig"), eingang.signatur)?;
        fs::write(tmp.join("installiert"), kontext.jetzt.to_rfc3339())?;
        let _ = fs::remove_dir_all(&gen_ordner);
        fs::rename(&tmp, &gen_ordner)?;

        let paket = self.paket_aus_ablage(m.generation, kontext)?;
        if let Err(grund) = selbsttest(&paket) {
            let _ = fs::remove_dir_all(&gen_ordner);
            return Err(Fehler::Selbsttest(grund));
        }
        if let Some(alt) = self.aktuell() {
            self.zeiger_setzen("vorher", alt)?;
        }
        self.zeiger_setzen("aktuell", m.generation)?;
        Ok(paket.id)
    }

    /// Laedt den Stand fuer diesen Programmstart (P-3.20, P-3.31).
    ///
    /// Mit `pin` (offener Flug) gilt nur genau dieser Stand; fehlt er, der
    /// eingebaute als `EingebautStattPin`. Ohne Pin: aktuell → vorher →
    /// eingebaut. Ablauf spielt beim Laden keine Rolle (P-3.04).
    pub fn laden(
        &self,
        pin: Option<&RuntimeBundleId>,
        kontext: &Kontext,
        eingebaut: &Eingebaut,
    ) -> Result<Ladeergebnis, Fehler> {
        let mut hinweise = Vec::new();
        if let Some(pin) = pin {
            match self.paket_aus_ablage(pin.generation, kontext) {
                Ok(p) if &p.id == pin => {
                    return Ok(Ladeergebnis { paket: p, herkunft: Herkunft::Pin, hinweise })
                }
                Ok(p) => hinweise.push(format!("Pin {pin}: lokal liegt {} ", p.id)),
                Err(e) => hinweise.push(format!("Pin {pin}: {e}")),
            }
            let paket = eingebautes_paket(eingebaut, kontext)?;
            return Ok(Ladeergebnis { paket, herkunft: Herkunft::EingebautStattPin, hinweise });
        }
        for (zeiger, herkunft) in [("aktuell", Herkunft::Aktuell), ("vorher", Herkunft::Vorher)] {
            let Some(g) = self.zeiger(zeiger) else { continue };
            match self.paket_aus_ablage(g, kontext) {
                Ok(paket) => return Ok(Ladeergebnis { paket, herkunft, hinweise }),
                Err(e) => hinweise.push(format!("{zeiger} ({g}): {e}")),
            }
        }
        let paket = eingebautes_paket(eingebaut, kontext)?;
        Ok(Ladeergebnis { paket, herkunft: Herkunft::Eingebaut, hinweise })
    }

    /// Entfernt Staende, die weder aktuell noch vorher noch gepinnt und aelter
    /// als 30 Tage sind, und danach nicht mehr benutzte Dateien (P-3.41).
    pub fn aufraeumen(&self, pins: &[u64], jetzt: DateTime<Utc>) -> Result<usize, Fehler> {
        let mut behalten: BTreeSet<u64> = pins.iter().copied().collect();
        behalten.extend(self.aktuell());
        behalten.extend(self.vorher());
        let mut entfernt = 0;
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
                _ => {
                    fs::remove_dir_all(self.gen_ordner(g))?;
                    entfernt += 1;
                }
            }
        }
        let mut gebraucht = BTreeSet::new();
        for g in &behalten {
            if let Ok(bytes) = fs::read(self.gen_ordner(*g).join("manifest.json")) {
                if let Ok(m) = serde_json::from_slice::<Manifest>(&bytes) {
                    gebraucht.extend(m.files.into_iter().map(|d| d.sha256));
                }
            }
        }
        for eintrag in fs::read_dir(self.wurzel.join("blobs"))? {
            let eintrag = eintrag?;
            let name = eintrag.file_name().to_string_lossy().into_owned();
            if !gebraucht.contains(&name) {
                fs::remove_file(eintrag.path())?;
            }
        }
        Ok(entfernt)
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

    fn zeiger(&self, name: &str) -> Option<u64> {
        fs::read_to_string(self.wurzel.join("zeiger").join(name)).ok()?.trim().parse().ok()
    }

    fn zeiger_setzen(&self, name: &str, g: u64) -> Result<(), Fehler> {
        self.atomar_schreiben(&self.wurzel.join("zeiger").join(name), g.to_string().as_bytes())
    }

    /// Schreiben in eine Zwischendatei und Umbenennen: Ein Absturz mittendrin
    /// hinterlaesst nie eine halbe Datei am Ziel.
    fn atomar_schreiben(&self, ziel: &Path, bytes: &[u8]) -> Result<(), Fehler> {
        let name = ziel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = self.wurzel.join("tmp").join(format!("{name}.{}", std::process::id()));
        {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
        }
        fs::rename(&tmp, ziel)?;
        Ok(())
    }
}

fn eingebautes_paket(e: &Eingebaut, _kontext: &Kontext) -> Result<Paket, Fehler> {
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
