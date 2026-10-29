//! v3 (Etappe 1a): Laufzeit-Paket im Programm (Pflichtenheft Kap. 3).
//!
//! Beim Start wird genau EIN Stand geladen und gilt fuer den ganzen
//! Programmlauf; ein neu geholter Stand wird erst beim naechsten Start aktiv
//! (P-3.20). Damit ist die Kennung eines Flugs der Stand des Laufs, in dem er
//! begann — `save_active_flight` traegt sie in die Flugdatei ein (P-3.30).
//!
//! Inhalt bis Etappe 1b: nur OurAirports (`runway.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use runtime_bundle::{Ablage, Eingang, Eingebaut, Herkunft, Kontext, Manifest, Paket, RuntimeBundleId};
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
    /// Bei `EingebautStattPin`: PIREP und Stand des offenen Flugs, dessen
    /// Stand lokal fehlte. Gilt nur fuer genau diesen Flug.
    pin: Option<(String, RuntimeBundleId)>,
}

static GELADEN: OnceLock<Geladen> = OnceLock::new();

/// Die Kennung, die der Flug `pirep_id` traegt: der Stand dieses Laufs.
/// Ausnahme: Fehlte beim Start der Stand des wiederaufgenommenen Flugs, behaelt
/// GENAU dieser Flug seine Kennung, damit er spaeter mit dem richtigen Stand
/// bewertet werden kann (P-3.31); jeder spaetere Flug im selben Lauf traegt
/// den tatsaechlich geladenen Stand.
pub(crate) fn flug_pin(pirep_id: &str) -> Option<RuntimeBundleId> {
    let g = GELADEN.get()?;
    match &g.pin {
        Some((pirep, pin)) if pirep == pirep_id => Some(pin.clone()),
        _ => Some(g.id.clone()),
    }
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

/// Liest PIREP und Pin des offenen Flugs direkt aus der Flugdatei, bevor
/// irgendein anderer Teil des Programms startet (P-3.20 Schritt 1).
fn pin_des_offenen_flugs(app_daten: &Path) -> Option<(String, RuntimeBundleId)> {
    let bytes = std::fs::read(app_daten.join(crate::ACTIVE_FLIGHT_FILE)).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let pirep = v.get("pirep_id")?.as_str()?.to_string();
    Some((pirep, serde_json::from_value(v.get("runtime_bundle_id")?.clone()).ok()?))
}

/// Offen ist ein Flug, solange seine Datei existiert — auch eine alte
/// 2.x-Datei ohne Kennung oder eine kaputte (Codex 29.10.).
fn flug_offen(app_daten: &Path) -> bool {
    app_daten.join(crate::ACTIVE_FLIGHT_FILE).exists()
}

/// Laedt den Stand fuer diesen Lauf und gibt OurAirports an `runway.rs`.
/// Muss vor jedem Zugriff auf die Bahntabellen laufen. Scheitert alles,
/// gilt der eingebaute Stand — das Programm startet immer.
pub(crate) fn starten(app_daten: &Path) {
    // Einmalig auch bei gleichzeitigen Aufrufern (Codex 29.10.).
    static EINMAL: std::sync::Once = std::sync::Once::new();
    EINMAL.call_once(|| starten_einmal(app_daten));
}

fn starten_einmal(app_daten: &Path) {
    let pin = pin_des_offenen_flugs(app_daten);
    let eingebaut = eingebaut();
    let ergebnis = Ablage::oeffnen(app_daten.join("laufzeit-paket"))
        .and_then(|a| a.laden(pin.as_ref().map(|(_, id)| id), &kontext(), &eingebaut));
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
    // Der Flug traegt die Kennung des Stands, der WIRKLICH gilt: Laesst sich
    // OurAirports aus dem Paket nicht setzen, gilt der eingebaute (Codex 29.10.).
    let text = |pfad: &str| -> Option<&'static str> {
        let bytes = paket.datei(pfad).ok()?;
        Some(Box::leak(String::from_utf8(bytes).ok()?.into_boxed_str()))
    };
    let gesetzt = match (text(RUNWAYS), text(AIRPORTS)) {
        (Some(rw), Some(ap)) => crate::runway::ourairports_setzen(rw, ap),
        _ => false,
    };
    let (id, herkunft) = if gesetzt {
        (paket.id, herkunft)
    } else {
        tracing::warn!(stand = %paket.id, "Laufzeit-Paket: OurAirports nicht anwendbar, eingebauter Stand gilt");
        match eingebaut.paket() {
            Ok(p) => (p.id, Herkunft::Eingebaut),
            Err(_) => return,
        }
    };
    tracing::info!(stand = %id, ?herkunft, "Laufzeit-Paket geladen");
    let pin = matches!(herkunft, Herkunft::EingebautStattPin).then_some(pin).flatten();
    let _ = GELADEN.set(Geladen { id, pin });
}

/// Holt einen neueren Stand von live und installiert ihn; aktiv wird er erst
/// beim naechsten Programmstart (P-3.20). Nicht bei offenem Flug, nicht ohne
/// Pruefschluessel (dann bestuende ohnehin kein Stand die Pruefung).
pub(crate) fn aktualisieren_im_hintergrund(app_daten: PathBuf) {
    if PRUEFSCHLUESSEL.is_empty() {
        return;
    }
    if flug_offen(&app_daten) {
        // Kein Update im Flug — aber fehlte beim Start der Stand des offenen
        // Flugs, genau diesen nachholen (P-3.31).
        if let Some((_, pin)) = GELADEN.get().and_then(|g| g.pin.clone()) {
            tauri::async_runtime::spawn(async move {
                match pin_nachholen(&app_daten, &pin).await {
                    Ok(()) => tracing::info!(stand = %pin, "Laufzeit-Paket: Stand des offenen Flugs nachgeholt"),
                    Err(e) => tracing::warn!(stand = %pin, error = %e, "Laufzeit-Paket: Stand des Flugs nicht nachholbar"),
                }
            });
        }
        return;
    }
    tauri::async_runtime::spawn(async move {
        match aktualisieren(&app_daten).await {
            Ok(Some(id)) => tracing::info!(stand = %id, "Laufzeit-Paket: neuer Stand, aktiv ab naechstem Start"),
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "Laufzeit-Paket: Aktualisierung nicht moeglich"),
        }
    });
}

#[derive(serde::Deserialize)]
struct AktuellAntwort {
    generation: u64,
    manifest: String,
    signatur: String,
    blobs: String,
}

async fn aktualisieren(app_daten: &Path) -> Result<Option<RuntimeBundleId>, String> {
    let basis = crate::live_zugang::LIVE_BASIS;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    // Mit Pilot-Token: Vorab-Piloten bekommen den neueren Vorabstand (P-3.13).
    let mut anfrage = client.get(format!("{basis}/api/bundle/current"));
    if let Some(token) = crate::live_zugang::pilot_token() {
        anfrage = anfrage.bearer_auth(token);
    }
    let antwort = anfrage.send().await.map_err(|e| e.to_string())?;
    if antwort.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let a: AktuellAntwort =
        antwort.error_for_status().map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
    let ablage = Ablage::oeffnen(app_daten.join("laufzeit-paket")).map_err(|e| e.to_string())?;
    if ablage.aktuell().is_some_and(|g| g >= a.generation) {
        return Ok(None);
    }
    let holen = |pfad: String| {
        let client = client.clone();
        async move {
            let r = client.get(format!("{basis}{pfad}")).send().await.map_err(|e| e.to_string())?;
            r.error_for_status().map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())
        }
    };
    let manifest = holen(a.manifest).await?.to_vec();
    let signatur = String::from_utf8(holen(a.signatur).await?.to_vec()).map_err(|e| e.to_string())?;
    let m = Manifest::lesen(&manifest).map_err(|e| e.to_string())?;
    let mut geholt = HashMap::new();
    for d in ablage.fehlende(&m) {
        geholt.insert(d.sha256.clone(), holen(format!("{}{}", a.blobs, d.sha256)).await?.to_vec());
    }
    let pins: Vec<u64> = [GELADEN.get().map(|g| g.id.generation), GELADEN.get().and_then(|g| g.pin.as_ref()).map(|(_, p)| p.generation)]
        .into_iter()
        .flatten()
        .collect();
    let daten = app_daten.to_path_buf();
    tokio::task::spawn_blocking(move || {
        // Erneut unmittelbar davor: Ein Flug kann waehrend des Ladens begonnen haben.
        if flug_offen(&daten) {
            return Ok(None);
        }
        let id = ablage
            .installieren(
                Eingang { manifest: &manifest, signatur: &signatur },
                &kontext(),
                &mut |d| geholt.get(&d.sha256).cloned().ok_or_else(|| "nicht geholt".to_string()),
                &selbsttest,
            )
            .map_err(|e| e.to_string())?;
        if let Err(e) = ablage.aufraeumen(&pins, chrono::Utc::now()) {
            tracing::warn!(error = %e, "Laufzeit-Paket: Aufraeumen ausgelassen");
        }
        Ok(Some(id))
    })
    .await
    .map_err(|e| e.to_string())?
}

async fn pin_nachholen(app_daten: &Path, pin: &RuntimeBundleId) -> Result<(), String> {
    let basis = crate::live_zugang::LIVE_BASIS;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let holen = |pfad: String| {
        let client = client.clone();
        async move {
            let r = client.get(format!("{basis}{pfad}")).send().await.map_err(|e| e.to_string())?;
            r.error_for_status().map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())
        }
    };
    let g = pin.generation;
    let manifest = holen(format!("/api/bundle/gen/{g}/manifest.json")).await?.to_vec();
    let signatur = String::from_utf8(holen(format!("/api/bundle/gen/{g}/manifest.json.minisig")).await?.to_vec())
        .map_err(|e| e.to_string())?;
    let ablage = Ablage::oeffnen(app_daten.join("laufzeit-paket")).map_err(|e| e.to_string())?;
    let m = Manifest::lesen(&manifest).map_err(|e| e.to_string())?;
    let mut geholt = HashMap::new();
    for d in ablage.fehlende(&m) {
        geholt.insert(d.sha256.clone(), holen(format!("/api/bundle/blobs/{}", d.sha256)).await?.to_vec());
    }
    let pin = pin.clone();
    tokio::task::spawn_blocking(move || {
        ablage
            .nachinstallieren(
                Eingang { manifest: &manifest, signatur: &signatur },
                &pin,
                &kontext(),
                &mut |d| geholt.get(&d.sha256).cloned().ok_or_else(|| "nicht geholt".to_string()),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Ein Stand, dessen OurAirports-Dateien nicht das erwartete Format haben,
/// wird nicht aktiviert (P-3.21).
fn selbsttest(p: &Paket) -> Result<(), String> {
    for (pfad, kopf) in [
        (RUNWAYS, "\"id\",\"airport_ref\",\"airport_ident\""),
        (AIRPORTS, "ident,type,latitude_deg,longitude_deg"),
    ] {
        let text = p.datei(pfad).map_err(|e| e.to_string())?;
        if !text.starts_with(kopf.as_bytes()) {
            return Err(format!("{pfad}: Kopfzeile unerwartet"));
        }
        if text.iter().filter(|&&b| b == b'\n').count() < 10_000 {
            return Err(format!("{pfad}: zu wenige Zeilen"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eingebauter_stand_besteht_den_selbsttest() {
        assert_eq!(selbsttest(&eingebaut().paket().unwrap()), Ok(()));
    }

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
        assert_eq!(flug_pin("x"), Some(pin.clone()));
        // Ein spaeterer Flug im selben Lauf flog mit dem eingebauten Stand.
        let anderer = flug_pin("y").unwrap();
        assert_eq!(anderer.generation, 0);
        assert_ne!(anderer, pin);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
