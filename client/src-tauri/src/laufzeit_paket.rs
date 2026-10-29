//! v3 (Etappe 1a): Laufzeit-Paket im Programm (Pflichtenheft Kap. 3).
//!
//! Beim Start wird genau EIN Stand geladen und gilt fuer den ganzen
//! Programmlauf; ein neu geholter Stand wird erst beim naechsten Start aktiv
//! (P-3.20). Damit ist die Kennung eines Flugs der Stand des Laufs, in dem er
//! begann — `save_active_flight` traegt sie in die Flugdatei ein (P-3.30).
//!
//! Inhalt bis Etappe 1b: nur OurAirports (`runway.rs`).

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use runtime_bundle::{Ablage, Eingebaut, Herkunft, Kontext, RuntimeBundleId};
use sha2::{Digest, Sha256};

/// Pruefschluessel der Laufzeit-Pakete (minisign, Base64), aktiv + Reserve
/// (P-3.10). Solange keiner eingetragen ist, besteht kein heruntergeladener
/// Stand die Pruefung, und es gilt immer der eingebaute.
const PRUEFSCHLUESSEL: &[&str] = &[];

/// Faehigkeiten dieses Programms fuer `requires` im Manifest (P-2.15).
const FAEHIGKEITEN: &[&str] = &["ourairports-1"];

const RUNWAYS: &str = "ourairports/runways.csv";
const AIRPORTS: &str = "ourairports/airports.csv";

struct Geladen {
    id: RuntimeBundleId,
    /// Bei `EingebautStattPin`: der Stand, auf den der offene Flug verweist.
    pin: Option<RuntimeBundleId>,
}

static GELADEN: OnceLock<Geladen> = OnceLock::new();

/// Die Kennung, die ein Flug dieses Programmlaufs traegt. Fehlte der Stand
/// eines wiederaufgenommenen Flugs, bleibt dessen Kennung erhalten, damit er
/// spaeter mit dem richtigen Stand bewertet werden kann (P-3.31).
pub(crate) fn flug_pin() -> Option<RuntimeBundleId> {
    let g = GELADEN.get()?;
    Some(g.pin.clone().unwrap_or_else(|| g.id.clone()))
}

fn kontext() -> Kontext {
    Kontext {
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        faehigkeiten: FAEHIGKEITEN.iter().map(|s| s.to_string()).collect(),
        schluessel: PRUEFSCHLUESSEL.iter().map(|s| s.to_string()).collect(),
        jetzt: chrono::Utc::now(),
    }
}

/// Der eingebaute Stand: Generation 0 aus den Dateien im Programm (P-3.22).
fn eingebaut() -> Eingebaut {
    let dateien: HashMap<String, Vec<u8>> = [
        (RUNWAYS, crate::runway::EINGEBAUT_RUNWAYS_CSV),
        (AIRPORTS, crate::runway::EINGEBAUT_AIRPORTS_CSV),
    ]
    .into_iter()
    .map(|(p, t)| (p.to_string(), t.as_bytes().to_vec()))
    .collect();
    let files: Vec<_> = [RUNWAYS, AIRPORTS]
        .iter()
        .map(|p| {
            let b = &dateien[*p];
            serde_json::json!({
                "path": p,
                "sha256": runtime_bundle::hex(&Sha256::digest(b)),
                "size": b.len(),
                "kind": "ourairports",
            })
        })
        .collect();
    let manifest = serde_json::json!({
        "bundle_schema": 1,
        "generation": 0,
        "created_at": "2026-01-01T00:00:00Z",
        "expires_at": "2999-01-01T00:00:00Z",
        "client_min": "0.0.0",
        "client_max": "999.0.0",
        "requires": [],
        "score_version": 26,
        "input_schema": 0,
        "files": files,
    });
    Eingebaut { manifest: serde_json::to_vec(&manifest).expect("Manifest"), dateien }
}

/// Liest den Pin des offenen Flugs direkt aus der Flugdatei, bevor irgendein
/// anderer Teil des Programms startet (P-3.20 Schritt 1).
fn pin_des_offenen_flugs(app_daten: &Path) -> Option<RuntimeBundleId> {
    let bytes = std::fs::read(app_daten.join(crate::ACTIVE_FLIGHT_FILE)).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    serde_json::from_value(v.get("runtime_bundle_id")?.clone()).ok()
}

/// Laedt den Stand fuer diesen Lauf und gibt OurAirports an `runway.rs`.
/// Muss vor jedem Zugriff auf die Bahntabellen laufen. Scheitert alles,
/// gilt der eingebaute Stand — das Programm startet immer.
pub(crate) fn starten(app_daten: &Path) {
    if GELADEN.get().is_some() {
        return;
    }
    let pin = pin_des_offenen_flugs(app_daten);
    let eingebaut = eingebaut();
    let ergebnis = Ablage::oeffnen(app_daten.join("laufzeit-paket"))
        .and_then(|a| a.laden(pin.as_ref(), &kontext(), &eingebaut));
    let (paket, herkunft) = match ergebnis {
        Ok(l) => {
            for h in &l.hinweise {
                tracing::warn!(hinweis = %h, "Laufzeit-Paket: Rueckfall");
            }
            (l.paket, l.herkunft)
        }
        Err(e) => {
            tracing::error!(error = %e, "Laufzeit-Paket: Ablage nicht nutzbar, eingebauter Stand");
            match eingebaut.paket() {
                Ok(p) => (p, Herkunft::Eingebaut),
                Err(e) => {
                    tracing::error!(error = %e, "Laufzeit-Paket: eingebauter Stand unlesbar");
                    return;
                }
            }
        }
    };
    let text = |pfad: &str| -> Option<&'static str> {
        let bytes = paket.datei(pfad).ok()?;
        Some(Box::leak(String::from_utf8(bytes).ok()?.into_boxed_str()))
    };
    if let (Some(rw), Some(ap)) = (text(RUNWAYS), text(AIRPORTS)) {
        if !crate::runway::ourairports_setzen(rw, ap) {
            tracing::warn!("Laufzeit-Paket: Bahntabellen waren schon geladen, eingebaute gelten");
        }
    }
    tracing::info!(stand = %paket.id, ?herkunft, "Laufzeit-Paket geladen");
    let pin = matches!(herkunft, Herkunft::EingebautStattPin).then_some(pin).flatten();
    let _ = GELADEN.set(Geladen { id: paket.id, pin });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eingebauter_stand_ist_ein_gueltiges_paket_mit_ourairports() {
        let p = eingebaut().paket().expect("eingebauter Stand");
        assert_eq!(p.id.generation, 0);
        assert_eq!(p.datei(RUNWAYS).unwrap(), crate::runway::EINGEBAUT_RUNWAYS_CSV.as_bytes());
        assert_eq!(p.datei(AIRPORTS).unwrap(), crate::runway::EINGEBAUT_AIRPORTS_CSV.as_bytes());
    }

    /// Fehlt der Stand eines offenen Flugs, laeuft das Programm mit dem
    /// eingebauten — der Flug behaelt aber SEINE Kennung (P-3.31).
    #[test]
    fn offener_flug_behaelt_seine_kennung_wenn_der_stand_fehlt() {
        let dir = std::env::temp_dir().join(format!("aeroacars-laufzeit-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pin = RuntimeBundleId { generation: 7, manifest_sha256: "ab".repeat(32) };
        std::fs::write(
            dir.join(crate::ACTIVE_FLIGHT_FILE),
            serde_json::json!({ "pirep_id": "x", "runtime_bundle_id": pin }).to_string(),
        )
        .unwrap();
        starten(&dir);
        assert_eq!(flug_pin(), Some(pin));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
