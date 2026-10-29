//! Manifest eines Laufzeit-Pakets (Pflichtenheft Kap. 5.1).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::Fehler;

/// Schemafassungen des Manifests, die dieser Client lesen kann.
pub const BUNDLE_SCHEMA_UNTERSTUETZT: std::ops::RangeInclusive<u32> = 1..=1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub bundle_schema: u32,
    /// Nur steigend; ein Zurueckrollen ist eine neue, hoehere Generation.
    pub generation: u64,
    pub created_at: DateTime<Utc>,
    /// Nur fuer neue Aktivierungen; ein aktiver Stand laeuft weiter (P-3.04).
    pub expires_at: DateTime<Utc>,
    pub client_min: String,
    pub client_max: String,
    /// Faehigkeiten des Programms, die das Paket braucht (P-2.15).
    #[serde(default)]
    pub requires: Vec<String>,
    pub score_version: u32,
    pub input_schema: u32,
    #[serde(default)]
    pub quelle: Quelle,
    pub files: Vec<Datei>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quelle {
    #[serde(default)]
    pub repo_commit: Option<String>,
    #[serde(default)]
    pub ourairports_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Datei {
    /// Pfad im Paket, immer mit `/`, relativ, ohne `..`.
    pub path: String,
    /// SHA-256, 64 Kleinbuchstaben-Hex.
    pub sha256: String,
    pub size: u64,
    pub kind: String,
}

impl Manifest {
    /// Liest und prueft den Aufbau (nicht die Signatur, nicht die Dateien).
    pub fn lesen(bytes: &[u8]) -> Result<Self, Fehler> {
        let m: Manifest =
            serde_json::from_slice(bytes).map_err(|e| Fehler::Manifest(e.to_string()))?;
        m.pruefen()?;
        Ok(m)
    }

    fn pruefen(&self) -> Result<(), Fehler> {
        if !BUNDLE_SCHEMA_UNTERSTUETZT.contains(&self.bundle_schema) {
            return Err(Fehler::Schema(self.bundle_schema));
        }
        let mut gesehen = std::collections::HashSet::new();
        for d in &self.files {
            pfad_pruefen(&d.path)?;
            if !gesehen.insert(d.path.as_str()) {
                return Err(Fehler::Manifest(format!("Pfad doppelt: {}", d.path)));
            }
            if d.sha256.len() != 64
                || !d.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Fehler::Manifest(format!("Hash ungueltig: {}", d.path)));
            }
        }
        version(&self.client_min)?;
        version(&self.client_max)?;
        Ok(())
    }

    pub fn datei(&self, pfad: &str) -> Option<&Datei> {
        self.files.iter().find(|d| d.path == pfad)
    }
}

/// Ein Pfad darf den Paketordner nie verlassen und muss auf Mac und Windows
/// dasselbe bedeuten (P-3.03).
fn pfad_pruefen(p: &str) -> Result<(), Fehler> {
    let schlecht = p.is_empty()
        || p.starts_with('/')
        || p.contains('\\')
        || p.contains(':')
        || p.split('/').any(|t| t.is_empty() || t == "." || t == "..");
    if schlecht {
        return Err(Fehler::Manifest(format!("Pfad nicht erlaubt: {p}")));
    }
    Ok(())
}

/// `major.minor.patch`, Vorab-Kennung (`-beta.1`) wird ignoriert.
pub fn version(v: &str) -> Result<(u64, u64, u64), Fehler> {
    let kern = v.split(['-', '+']).next().unwrap_or("");
    let teile: Vec<_> = kern.split('.').map(str::parse::<u64>).collect();
    match teile.as_slice() {
        [Ok(a), Ok(b), Ok(c)] => Ok((*a, *b, *c)),
        _ => Err(Fehler::Manifest(format!("Version ungueltig: {v}"))),
    }
}
