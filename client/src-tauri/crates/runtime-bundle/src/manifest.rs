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
        // Doppelt auch ohne Ruecksicht auf Gross-/Kleinschreibung: Mac und
        // Windows unterscheiden `A/x` und `a/x` nicht (P-3.03).
        let mut gesehen = std::collections::HashSet::new();
        for d in &self.files {
            pfad_pruefen(&d.path)?;
            if !gesehen.insert(d.path.to_ascii_lowercase()) {
                return Err(Fehler::Manifest(format!("Pfad doppelt: {}", d.path)));
            }
            if d.sha256.len() != 64
                || !d.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(Fehler::Manifest(format!("Hash ungueltig: {}", d.path)));
            }
        }
        if version(&self.client_min)? > version(&self.client_max)? {
            return Err(Fehler::Manifest(format!(
                "client_min {} groesser als client_max {}",
                self.client_min, self.client_max
            )));
        }
        Ok(())
    }

    pub fn datei(&self, pfad: &str) -> Option<&Datei> {
        self.files.iter().find(|d| d.path == pfad)
    }
}

/// Ein Pfad darf den Paketordner nie verlassen und muss auf Mac und Windows
/// dasselbe bedeuten (P-3.03): nur ASCII-Buchstaben, Ziffern, `.`, `_`, `-`
/// und `/` als Trenner; keine leeren, `.`- oder `..`-Teile; kein Teil mit
/// Punkt am Ende; keine reservierten Windows-Namen.
fn pfad_pruefen(p: &str) -> Result<(), Fehler> {
    let zeichen_ok = !p.is_empty()
        && p.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'));
    let teile_ok = zeichen_ok
        && p.split('/').all(|t| {
            let stamm = t.split('.').next().unwrap_or("").to_ascii_uppercase();
            let reserviert = matches!(stamm.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ((stamm.starts_with("COM") || stamm.starts_with("LPT"))
                    && stamm.len() == 4
                    && stamm.as_bytes()[3].is_ascii_digit());
            !t.is_empty() && t != "." && t != ".." && !t.ends_with('.') && !reserviert
        });
    if !teile_ok {
        return Err(Fehler::Manifest(format!("Pfad nicht erlaubt: {p}")));
    }
    Ok(())
}

/// SemVer; eine Vorabversion (`3.0.0-beta.1`) liegt vor dem Release `3.0.0`.
pub fn version(v: &str) -> Result<semver::Version, Fehler> {
    semver::Version::parse(v).map_err(|_| Fehler::Manifest(format!("Version ungueltig: {v}")))
}
