//! v3 (P-14.04, P-14.08): 3.0 laeuft bis zur Freigabe fuer alle als eigene
//! App neben 2.x (`com.aeroacars.app3`, eigener Datenordner). Beim ersten
//! Start uebernimmt sie Sitzung, Einstellungen, Bordbuch, Landungen,
//! Musikablage und Hoppie-Code aus dem Ordner von 2.x — als Kopie: die
//! 2.x-Dateien bleiben unveraendert liegen (Rueckweg).
//!
//! **Nicht** kopiert wird, was beide Apps sonst doppelt an den Server
//! schicken koennten: die Warteschlangen fuer PIREPs, Nachtraege und
//! Flugprotokolle (2.x `flight_logs`, 3.0 `pending_uploads` und der fruehere
//! 3.0-Ordner `landing_inputs`, P-7.21) und der offene Flug. Dazu die Logs von 2.x (eigene Logs
//! je App). Laeuft ganz am Anfang von `run`, bevor irgendetwas den Ordner
//! liest; Fehler werden protokolliert, der Start geht weiter.

use std::path::{Path, PathBuf};

/// Identifier von 2.x (Quelle der Uebernahme).
pub(crate) const KENNUNG_2X: &str = "com.aeroacars.app";

/// Merker im 3.0-Ordner: Uebernahme ist gelaufen (auch wenn es nichts gab).
const MERKER: &str = ".von_2x_uebernommen";

/// Was nicht kopiert wird (Name auf oberster Ebene), mit Grund im Modulkopf.
const AUSGENOMMEN: &[&str] = &[
    "pending_pireps",
    "nachtrag",
    "flight_logs",
    "pending_uploads",
    "landing_inputs",
    "active_flight.json",
    "logs",
    "laufzeit-paket",
    MERKER,
];

/// Uebernimmt einmalig aus `quelle` (2.x) nach `ziel` (3.0). Bestehende
/// Dateien im Ziel werden nie ueberschrieben. Gibt die Namen der kopierten
/// Eintraege zurueck (leer, wenn schon uebernommen oder keine Quelle).
pub(crate) fn uebernehmen(quelle: &Path, ziel: &Path) -> std::io::Result<Vec<String>> {
    if ziel.join(MERKER).exists() {
        return Ok(Vec::new());
    }
    std::fs::create_dir_all(ziel)?;
    let mut kopiert = Vec::new();
    if quelle.is_dir() {
        let mut eintraege: Vec<_> = std::fs::read_dir(quelle)?.flatten().collect();
        eintraege.sort_by_key(|e| e.file_name());
        for e in eintraege {
            let name = e.file_name().to_string_lossy().into_owned();
            if AUSGENOMMEN.contains(&name.as_str()) {
                continue;
            }
            kopieren(&e.path(), &ziel.join(&name))?;
            kopiert.push(name);
        }
    }
    std::fs::write(ziel.join(MERKER), kopiert.join("\n"))?;
    Ok(kopiert)
}

fn kopieren(von: &Path, nach: &Path) -> std::io::Result<()> {
    let art = std::fs::symlink_metadata(von)?;
    if art.is_dir() {
        std::fs::create_dir_all(nach)?;
        for e in std::fs::read_dir(von)?.flatten() {
            kopieren(&e.path(), &nach.join(e.file_name()))?;
        }
    } else if art.is_file() && !nach.exists() {
        std::fs::copy(von, nach)?;
    }
    // Symlinks und Sonstiges: nicht folgen, nicht kopieren.
    Ok(())
}

/// Die Ordnerpaare 2.x → 3.0, wie Tauri sie ableitet: Daten- und
/// Konfigurationsordner (`dirs::data_dir()`/`config_dir()` plus Identifier;
/// auf Windows und macOS derselbe Ordner, dann nur ein Paar).
pub(crate) fn ordnerpaare(kennung_3: &str) -> Vec<(PathBuf, PathBuf)> {
    let mut paare: Vec<(PathBuf, PathBuf)> = Vec::new();
    for basis in [dirs::data_dir(), dirs::config_dir()].into_iter().flatten() {
        let paar = (basis.join(KENNUNG_2X), basis.join(kennung_3));
        if !paare.contains(&paar) {
            paare.push(paar);
        }
    }
    paare
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordner(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("aeroacars-uebernahme-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn kopiert_daten_ohne_warteschlangen_und_laesst_2x_unveraendert() {
        let q = ordner("q1");
        let z = ordner("z1").join("app3");
        std::fs::write(q.join("secrets.json"), "geheim").unwrap();
        std::fs::write(q.join("landings.json"), "[]").unwrap();
        std::fs::create_dir_all(q.join("bordbuch/2026")).unwrap();
        std::fs::write(q.join("bordbuch/2026/a.json"), "{}").unwrap();
        for weg in ["pending_pireps", "nachtrag", "flight_logs", "pending_uploads", "landing_inputs", "logs"] {
            std::fs::create_dir_all(q.join(weg)).unwrap();
            std::fs::write(q.join(weg).join("x"), "1").unwrap();
        }
        std::fs::write(q.join("active_flight.json"), "{}").unwrap();

        let kopiert = uebernehmen(&q, &z).unwrap();
        assert_eq!(kopiert, ["bordbuch", "landings.json", "secrets.json"]);
        assert_eq!(std::fs::read_to_string(z.join("secrets.json")).unwrap(), "geheim");
        assert!(z.join("bordbuch/2026/a.json").exists());
        for weg in ["pending_pireps", "nachtrag", "flight_logs", "pending_uploads", "landing_inputs", "logs", "active_flight.json"] {
            assert!(!z.join(weg).exists(), "{weg} darf nicht mit");
        }
        // 2.x unveraendert (Rueckweg)
        assert!(q.join("pending_pireps/x").exists() && q.join("active_flight.json").exists());
        assert_eq!(std::fs::read_to_string(q.join("secrets.json")).unwrap(), "geheim");
    }

    #[test]
    fn laeuft_nur_einmal_und_ueberschreibt_nichts() {
        let q = ordner("q2");
        let z = ordner("z2");
        std::fs::write(q.join("sim.json"), "alt").unwrap();
        std::fs::write(z.join("sim.json"), "neu").unwrap();
        assert_eq!(uebernehmen(&q, &z).unwrap(), ["sim.json"]);
        assert_eq!(std::fs::read_to_string(z.join("sim.json")).unwrap(), "neu");
        std::fs::write(q.join("ui_state.json"), "x").unwrap();
        assert!(uebernehmen(&q, &z).unwrap().is_empty());
        assert!(!z.join("ui_state.json").exists(), "zweiter Lauf kopiert nichts mehr");
    }

    #[test]
    fn ohne_2x_ordner_nur_merker() {
        let z = ordner("z3");
        assert!(uebernehmen(&z.join("gibt-es-nicht"), &z).unwrap().is_empty());
        assert!(z.join(MERKER).exists());
    }
}
