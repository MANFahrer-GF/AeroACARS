//! Ein Paket aus einem Quellordner bauen (CI, Pflichtenheft P-3.12).
//!
//! Ergebnis: `<ziel>/manifest.json` (unsigniert) und `<ziel>/blobs/<sha256>`.
//! Signiert wird danach in der CI mit dem geheimen Schluessel; dieses
//! Werkzeug kennt keinen Schluessel. Gleiche Quelle + gleiche Optionen =
//! byte-gleiches Manifest (Pfade sortiert, Zeit von aussen).

use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

use crate::manifest::{Datei, Manifest, Quelle};
use crate::{hex, Fehler};

pub struct Optionen {
    pub generation: u64,
    pub erstellt: DateTime<Utc>,
    pub gueltig_tage: i64,
    pub client_min: String,
    pub client_max: String,
    pub requires: Vec<String>,
    pub score_version: u32,
    pub input_schema: u32,
    pub quelle: Quelle,
}

/// Baut das Paket und gibt die Manifest-Bytes zurueck (so, wie sie signiert
/// werden muessen).
pub fn bauen(quelle: &Path, ziel: &Path, o: &Optionen) -> Result<Vec<u8>, Fehler> {
    let blobs = ziel.join("blobs");
    fs::create_dir_all(&blobs)?;
    let mut pfade = Vec::new();
    sammeln(quelle, quelle, &mut pfade)?;
    pfade.sort();
    let mut files = Vec::with_capacity(pfade.len());
    for rel in pfade {
        let bytes = fs::read(quelle.join(&rel))?;
        let sha256 = hex(&Sha256::digest(&bytes));
        let ablage = blobs.join(&sha256);
        if !ablage.exists() {
            fs::write(&ablage, &bytes)?;
        }
        let kind = rel.split('/').next().unwrap_or("").to_string();
        files.push(Datei { path: rel, sha256, size: bytes.len() as u64, kind });
    }
    let m = Manifest {
        bundle_schema: *crate::manifest::BUNDLE_SCHEMA_UNTERSTUETZT.end(),
        generation: o.generation,
        created_at: o.erstellt,
        expires_at: o.erstellt + Duration::days(o.gueltig_tage),
        client_min: o.client_min.clone(),
        client_max: o.client_max.clone(),
        requires: o.requires.clone(),
        score_version: o.score_version,
        input_schema: o.input_schema,
        quelle: o.quelle.clone(),
        files,
        weitere: Default::default(),
    };
    let bytes = serde_json::to_vec_pretty(&m).map_err(|e| Fehler::Manifest(e.to_string()))?;
    // Was wir bauen, muss der Client auch lesen koennen (Pfade, Hashes, Schema).
    Manifest::lesen(&bytes)?;
    fs::write(ziel.join("manifest.json"), &bytes)?;
    Ok(bytes)
}

/// Alle regulaeren Dateien unter `wurzel`, als Pfad mit `/`. Symbolische
/// Verweise und versteckte Dateien (`.DS_Store`, `.gitkeep`) kommen nicht ins
/// Paket.
fn sammeln(wurzel: &Path, ordner: &Path, aus: &mut Vec<String>) -> Result<(), Fehler> {
    for e in fs::read_dir(ordner)? {
        let e = e?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let typ = e.file_type()?;
        if typ.is_dir() {
            sammeln(wurzel, &e.path(), aus)?;
        } else if typ.is_file() {
            let rel = e.path().strip_prefix(wurzel).expect("unter der Wurzel").to_path_buf();
            let teile: Vec<String> =
                rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            aus.push(teile.join("/"));
        }
    }
    Ok(())
}
