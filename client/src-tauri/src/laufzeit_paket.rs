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
/// (P-3.10). Der aktive signiert in CI (Secret `BUNDLE_MINISIGN_KEY`), der
/// Reserveschluessel liegt offline und wird nur beim Schluesselwechsel benutzt.
const PRUEFSCHLUESSEL: &[&str] = &[
    "RWS3XSmTUeW2CWT94kbZKuf9E8DT6aBEtdEWEgNm48HLdY3TqTT9QbWn",
    "RWRs0NG12y4cVsFVpERYP76rfw0Zo2DiQ+Ja9W1ZoX0BRtpLWeRBzk4I",
];

/// Faehigkeiten dieses Programms fuer `requires` im Manifest (P-2.15).
const FAEHIGKEITEN: &[&str] = &["ourairports-1", "grenzwerte-1", "xplane-profile-1", "cpdlc-1", "flotte-1", "texte-1", "spurweiten-1", "vref-1", "hoehengruppen-1", "klappen-1", "kabine-1", "endpunkte-1"];

const RUNWAYS: &str = "ourairports/runways.csv";
const AIRPORTS: &str = "ourairports/airports.csv";
const XPLANE_PROFILE: &str = "profile/xplane.json";
const CPDLC: &str = "tabellen/cpdlc.json";
const FLOTTE: &str = "tabellen/flotte.json";
const SPURWEITEN: &str = "tabellen/spurweiten.json";
const VREF: &str = "tabellen/vref.json";
const HOEHENGRUPPEN: &str = "tabellen/hoehengruppen.json";
const KLAPPEN: &str = "tabellen/klappen.json";
const KABINE: &str = "tabellen/kabine.json";
const ENDPUNKTE: &str = "endpunkte.json";

/// Dateien, die die Oberflaeche ueber [`paket_datei`] lesen darf: feste
/// Liste, nur lesen, nur aus dem geltenden Stand.
const UI_DATEIEN: &[&str] = &[FLOTTE, "texte/de.json", "texte/en.json", "texte/it.json"];

/// Alles, was aus dem Laufzeit-Paket kommt, als EIN Stand (P-3.02): Er wird
/// genau einmal je Programmlauf festgelegt — vollstaendig aus dem Paket oder
/// vollstaendig eingebaut, nie gemischt. `runway.rs` und `grenzwerte.rs`
/// lesen nur hieraus (Advisor 29.10.: zwei getrennte Sperren konnten mischen).
pub(crate) struct AktiverStand {
    pub id: RuntimeBundleId,
    pub runways: &'static str,
    pub airports: &'static str,
    pub grenzwerte: crate::grenzwerte::Grenzwerte,
    /// X-Plane-Flugzeugprofile; `sim-xplane` liest sie ueber die beim Start
    /// angemeldete Quelle von hier (keine eigene Sperre im Crate).
    pub xplane_profile: &'static [sim_xplane::XplaneAircraftProfile],
    /// Hoppie-/CPDLC-Elementtabelle, ebenso ueber eine angemeldete Quelle.
    pub cpdlc: &'static [hoppie_protocol::elements::ElementSpec],
    /// Flotten-/Kategorietabelle (Rust-Teil; die Oberflaeche liest dieselbe
    /// Datei ueber `paket_datei`).
    pub flotte: crate::aircraft_category::Flotte,
    /// Spurweiten/Spannweiten und Vref-Bezugswerte (`landing-scoring`, ueber
    /// angemeldete Quellen).
    pub spurweiten: &'static [landing_scoring::spurweite::Zeile],
    pub vref: &'static [landing_scoring::vref::Zeile],
    /// FAA-Hoehengruppen G1–G4 je Muster (`runway_assessment`).
    pub hoehengruppen: crate::runway_assessment::Hoehengruppen,
    /// Klappen-Beschriftung je Rastenzahl (`raster_flap_label`).
    pub klappen: Vec<KlappenRaster>,
    /// Kabinenansagen: Listen und Zeiten (`cabin-announcements`, ueber eine
    /// angemeldete Quelle).
    pub kabine: cabin_announcements::Tabelle,
    /// Adressen der Gegenstellen, nur innerhalb der festen Hostliste von
    /// `paket-endpunkte` (P-3.60).
    pub endpunkte: paket_endpunkte::Endpunkte,
    /// Rohtexte der [`UI_DATEIEN`] aus diesem Stand.
    pub ui: Vec<(&'static str, &'static str)>,
}

static STAND: OnceLock<AktiverStand> = OnceLock::new();

/// PIREP und Stand eines offenen Flugs, dessen Stand in diesem Lauf NICHT gilt
/// (fehlte lokal oder kam zu spaet). Gilt nur fuer genau diesen Flug (P-3.31).
static PIN: OnceLock<(String, RuntimeBundleId)> = OnceLock::new();

/// Der geltende Stand. Wer liest, bevor `starten` lief (z. B. Tests), legt
/// damit den eingebauten Stand fest.
pub(crate) fn stand() -> &'static AktiverStand {
    STAND.get_or_init(|| eingebauter_stand().expect("eingebauter Stand lesbar (Test eingebauter_stand_besteht_den_selbsttest)"))
}

fn stand_aus_paket(p: &Paket) -> Result<AktiverStand, String> {
    let text = |pfad: &str| -> Result<&'static str, String> {
        let bytes = p.datei(pfad).map_err(|e| e.to_string())?;
        Ok(Box::leak(String::from_utf8(bytes).map_err(|e| format!("{pfad}: {e}"))?.into_boxed_str()))
    };
    Ok(AktiverStand {
        id: p.id.clone(),
        runways: text(RUNWAYS)?,
        airports: text(AIRPORTS)?,
        grenzwerte: crate::grenzwerte::lesen(|x| p.datei(x).ok())?,
        xplane_profile: sim_xplane::profile_lesen(text(XPLANE_PROFILE)?)?,
        cpdlc: hoppie_protocol::elements_data::tabelle_lesen(text(CPDLC)?)?,
        flotte: crate::aircraft_category::flotte_lesen(text(FLOTTE)?)?,
        spurweiten: landing_scoring::spurweite::lesen(text(SPURWEITEN)?)?,
        hoehengruppen: crate::runway_assessment::hoehengruppen_lesen(text(HOEHENGRUPPEN)?)?,
        klappen: klappen_lesen(text(KLAPPEN)?)?,
        kabine: cabin_announcements::lesen(text(KABINE)?)?,
        endpunkte: paket_endpunkte::lesen(text(ENDPUNKTE)?)?,
        vref: landing_scoring::vref::lesen(text(VREF)?)?,
        ui: UI_DATEIEN.iter().map(|&d| Ok((d, text(d)?))).collect::<Result<_, String>>()?,
    })
}

fn eingebauter_stand() -> Result<AktiverStand, String> {
    let p = eingebaut().paket().map_err(|e| e.to_string())?;
    Ok(AktiverStand {
        id: p.id,
        runways: crate::runway::eingebaut_runways_csv(),
        airports: crate::runway::eingebaut_airports_csv(),
        grenzwerte: crate::grenzwerte::lesen(|x| eingebaute_datei(x).map(<[u8]>::to_vec))?,
        xplane_profile: sim_xplane::profile_lesen(eingebauter_text(XPLANE_PROFILE)?)?,
        cpdlc: hoppie_protocol::elements_data::tabelle_lesen(eingebauter_text(CPDLC)?)?,
        flotte: crate::aircraft_category::flotte_lesen(eingebauter_text(FLOTTE)?)?,
        spurweiten: landing_scoring::spurweite::lesen(eingebauter_text(SPURWEITEN)?)?,
        hoehengruppen: crate::runway_assessment::hoehengruppen_lesen(eingebauter_text(HOEHENGRUPPEN)?)?,
        klappen: klappen_lesen(eingebauter_text(KLAPPEN)?)?,
        kabine: cabin_announcements::lesen(eingebauter_text(KABINE)?)?,
        endpunkte: paket_endpunkte::lesen(eingebauter_text(ENDPUNKTE)?)?,
        vref: landing_scoring::vref::lesen(eingebauter_text(VREF)?)?,
        ui: UI_DATEIEN.iter().map(|&d| Ok((d, eingebauter_text(d)?))).collect::<Result<_, String>>()?,
    })
}

/// Legt den Stand fuer diesen Lauf fest. `false`, wenn schon einer gilt.
fn uebernehmen(s: AktiverStand) -> bool {
    STAND.set(s).is_ok()
}

/// Eine Zeile aus `tabellen/klappen.json`: Beschriftung der Klappenstufen
/// fuer eine Rastenzahl (ohne UP gezaehlt), optional nur fuer bestimmte Muster.
#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KlappenRaster {
    #[allow(dead_code)]
    name: String,
    pub rasten: u8,
    #[serde(default)]
    pub nur_muster: Vec<String>,
    pub stufen: Vec<String>,
}

/// Liest `tabellen/klappen.json` (fail-closed): je Zeile genau `rasten + 1`
/// Stufen, Muster in Grossbuchstaben.
pub(crate) fn klappen_lesen(json: &str) -> Result<Vec<KlappenRaster>, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Datei {
        format: u32,
        #[serde(default)]
        #[allow(dead_code)]
        notiz: Vec<String>,
        raster: Vec<KlappenRaster>,
    }
    let d: Datei = serde_json::from_str(json).map_err(|e| format!("{KLAPPEN}: {e}"))?;
    if d.format != 1 {
        return Err(format!("{KLAPPEN}: Format {} unbekannt", d.format));
    }
    for r in &d.raster {
        if r.stufen.len() != r.rasten as usize + 1 || r.stufen.iter().any(String::is_empty) {
            return Err(format!("{KLAPPEN}: {} braucht {} Stufen", r.name, r.rasten as usize + 1));
        }
        if r.nur_muster.iter().any(|m| m.is_empty() || *m != m.trim().to_ascii_uppercase()) {
            return Err(format!("{KLAPPEN}: {} Muster ungueltig", r.name));
        }
    }
    Ok(d.raster)
}

/// Eine Datei des geltenden Stands fuer die Oberflaeche (nur [`UI_DATEIEN`]).
#[tauri::command]
pub(crate) fn paket_datei(pfad: String) -> Result<&'static str, String> {
    stand()
        .ui
        .iter()
        .find(|(d, _)| *d == pfad)
        .map(|(_, t)| *t)
        .ok_or_else(|| format!("{pfad}: nicht freigegeben"))
}

/// Die Kennung, die der Flug `pirep_id` traegt: der Stand dieses Laufs.
/// Ausnahme: Galt beim Start der Stand des wiederaufgenommenen Flugs nicht,
/// behaelt GENAU dieser Flug seine Kennung, damit er spaeter mit dem richtigen
/// Stand bewertet werden kann (P-3.31); jeder spaetere Flug im selben Lauf
/// traegt den tatsaechlich geltenden Stand.
pub(crate) fn flug_pin(pirep_id: &str) -> Option<RuntimeBundleId> {
    match PIN.get() {
        Some((pirep, pin)) if pirep == pirep_id => Some(pin.clone()),
        _ => Some(stand().id.clone()),
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

mod eingebettet {
    // Erzeugt von build.rs aus dem Inhalt von bundle/ — die einzige Liste.
    include!(concat!(env!("OUT_DIR"), "/eingebautes_paket.rs"));
}

/// Eine Datei des eingebauten Stands (Inhalt von `bundle/` beim Bau).
#[cfg(test)]
pub(crate) fn eingebaute_datei_text(pfad: &str) -> &'static str {
    eingebauter_text(pfad).unwrap()
}

fn eingebauter_text(pfad: &str) -> Result<&'static str, String> {
    std::str::from_utf8(eingebaute_datei(pfad).ok_or(pfad)?).map_err(|e| format!("{pfad}: {e}"))
}

pub(crate) fn eingebaute_datei(pfad: &str) -> Option<&'static [u8]> {
    eingebettet::DATEIEN.iter().find(|(p, _)| *p == pfad).map(|(_, b)| *b)
}

/// Der eingebaute Stand: Generation 0 aus den Dateien im Programm (P-3.22).
fn eingebaut() -> Eingebaut {
    let dateien: HashMap<String, Vec<u8>> =
        eingebettet::DATEIEN.iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    let files: Vec<_> = eingebettet::DATEIEN
        .iter()
        .map(|(p, b)| {
            serde_json::json!({
                "path": p,
                "sha256": runtime_bundle::hex(&Sha256::digest(b)),
                "size": b.len(),
                "kind": p.split('/').next().unwrap_or(""),
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
    // Vor allem anderen: X-Plane-Profile kommen ab jetzt aus DIESEM Stand.
    sim_xplane::profile_quelle_setzen(|| stand().xplane_profile);
    hoppie_protocol::elements_data::tabelle_quelle_setzen(|| stand().cpdlc);
    landing_scoring::spurweite::quelle_setzen(|| stand().spurweiten);
    landing_scoring::vref::quelle_setzen(|| stand().vref);
    cabin_announcements::quelle_setzen(|| &stand().kabine);
    paket_endpunkte::quelle_setzen(|| &stand().endpunkte);
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
    // Erst den ganzen Stand lesen, dann EINMAL festlegen (P-3.02). Laesst
    // sich ein Teil nicht lesen, gilt der eingebaute Stand.
    let neu = match stand_aus_paket(&paket) {
        Ok(neu) => Some(neu),
        Err(e) => {
            tracing::warn!(stand = %paket.id, error = %e, "Laufzeit-Paket: Stand nicht anwendbar, eingebauter Stand gilt");
            eingebauter_stand().ok()
        }
    };
    if let Some(neu) = neu {
        if !uebernehmen(neu) {
            tracing::warn!("Laufzeit-Paket: ein Stand galt schon (vorher gelesen), er bleibt");
        }
    }
    let wirksam = stand().id.clone();
    if let Some((pirep, pin)) = pin {
        if pin != wirksam {
            let _ = PIN.set((pirep, pin));
        }
    }
    tracing::info!(stand = %wirksam, ?herkunft, "Laufzeit-Paket geladen");
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
        if let Some((_, pin)) = PIN.get().cloned() {
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
    let basis = crate::live_zugang::live_basis();
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
            // Mit Token: Vorabstaende gibt es auch ueber die Nummer nur fuer
            // Vorab-Piloten (P-3.13).
            let mut anfrage = client.get(format!("{basis}{pfad}"));
            if let Some(token) = crate::live_zugang::pilot_token() {
                anfrage = anfrage.bearer_auth(token);
            }
            let r = anfrage.send().await.map_err(|e| e.to_string())?;
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
    let pins: Vec<u64> = [Some(stand().id.generation), PIN.get().map(|(_, p)| p.generation)]
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
    let basis = crate::live_zugang::live_basis();
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let holen = |pfad: String| {
        let client = client.clone();
        async move {
            // Mit Token: Vorabstaende gibt es auch ueber die Nummer nur fuer
            // Vorab-Piloten (P-3.13).
            let mut anfrage = client.get(format!("{basis}{pfad}"));
            if let Some(token) = crate::live_zugang::pilot_token() {
                anfrage = anfrage.bearer_auth(token);
            }
            let r = anfrage.send().await.map_err(|e| e.to_string())?;
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
    crate::grenzwerte::lesen(|pfad| p.datei(pfad).ok())?;
    let profile = p.datei(XPLANE_PROFILE).map_err(|e| e.to_string())?;
    sim_xplane::profile_pruefen(std::str::from_utf8(&profile).map_err(|e| e.to_string())?)?;
    let flotte = p.datei(FLOTTE).map_err(|e| e.to_string())?;
    crate::aircraft_category::flotte_lesen(std::str::from_utf8(&flotte).map_err(|e| e.to_string())?)?;
    for (pfad, pruefen) in [
        (SPURWEITEN, landing_scoring::spurweite::pruefen as fn(&str) -> Result<(), String>),
        (VREF, landing_scoring::vref::pruefen),
        (HOEHENGRUPPEN, |t| crate::runway_assessment::hoehengruppen_lesen(t).map(|_| ())),
        (KLAPPEN, |t| klappen_lesen(t).map(|_| ())),
        (KABINE, |t| cabin_announcements::lesen(t).map(|_| ())),
        (ENDPUNKTE, |t| paket_endpunkte::lesen(t).map(|_| ())),
    ] {
        let roh = p.datei(pfad).map_err(|e| e.to_string())?;
        pruefen(std::str::from_utf8(&roh).map_err(|e| format!("{pfad}: {e}"))?)?;
    }
    for d in UI_DATEIEN {
        p.datei(d).map_err(|e| e.to_string())?;
    }
    let cpdlc = p.datei(CPDLC).map_err(|e| e.to_string())?;
    hoppie_protocol::elements_data::tabelle_pruefen(std::str::from_utf8(&cpdlc).map_err(|e| e.to_string())?)?;
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

    /// Beide eingetragenen Schluessel sind echte minisign-Schluessel und
    /// passen zu ihren geheimen Teilen: je eine Probesignatur (erzeugt mit
    /// `minisign -S`) besteht die Pruefung, ein veraenderter Text nicht.
    #[test]
    fn pruefschluessel_aktiv_und_reserve_pruefen_echte_signaturen() {
        let schluessel: Vec<String> = PRUEFSCHLUESSEL.iter().map(|s| s.to_string()).collect();
        assert_eq!(schluessel.len(), 2);
        for sig in [PROBE_AKTIV, PROBE_RESERVE] {
            assert!(runtime_bundle::signatur_pruefen(b"AeroACARS Schluesselprobe", sig, &schluessel).is_ok());
            assert!(runtime_bundle::signatur_pruefen(b"AeroACARS Schluesselprobe!", sig, &schluessel).is_err());
        }
    }

    const PROBE_AKTIV: &str = r"untrusted comment: signature from minisign secret key
RUS3XSmTUeW2CdEtCcDhIqYPqMPcm5kxNRuW89n/+UsVd59RG1XqzbYhlw+KUDk5y8HWBjZypCORBWLaIsY3cw9ZhqKH8O3Qpgc=
trusted comment: probe
RIZYY+6Vi9hE0sy++DEtpxMjwCRJzE8x+z9xOAEQsZLFChAUC3YIL7DTFJwgtPjdzVQ8fAHdh21gOdg9yKsDBg==
";
    const PROBE_RESERVE: &str = r"untrusted comment: signature from minisign secret key
RURs0NG12y4cVkAofIm/1xLPjtwC0kUUrvUHcpCsQ5B7Hb3Vr227UeoRQ8dqozQ4y1EqiSVK1uQEoRdh/n1Xu978dRq6xxb31Ac=
trusted comment: probe
aEu/MqmKECj2KRoMCUN4vPp/tna5e5vMHgCJUby+H4hRcGGSg0YLQtsc+uMfQ5bBhOmKm/UsiuqF+IkKH3K7Cg==
";

    /// Die eingebauten Profile des Clients sind dieselbe Datei, die
    /// `sim-xplane` ohne angemeldete Quelle nimmt (eine Quelle, P-3.65).
    #[test]
    fn eingebaute_xplane_profile_sind_die_des_crates() {
        assert_eq!(eingebaute_datei(XPLANE_PROFILE).unwrap(), sim_xplane::PROFILE_EINGEBAUT.as_bytes());
        assert_eq!(stand().xplane_profile.len(), sim_xplane::profile_lesen(sim_xplane::PROFILE_EINGEBAUT).unwrap().len());
    }

    #[test]
    fn eingebaute_cpdlc_tabelle_ist_die_des_crates() {
        assert_eq!(eingebaute_datei(CPDLC).unwrap(), hoppie_protocol::elements_data::EINGEBAUT.as_bytes());
        assert_eq!(stand().cpdlc.len(), hoppie_protocol::elements_data::tabelle_lesen(hoppie_protocol::elements_data::EINGEBAUT).unwrap().len());
    }

    #[test]
    fn eingebaute_spurweiten_und_vref_sind_die_des_crates() {
        assert_eq!(eingebaute_datei(SPURWEITEN).unwrap(), landing_scoring::spurweite::EINGEBAUT.as_bytes());
        assert_eq!(eingebaute_datei(VREF).unwrap(), landing_scoring::vref::EINGEBAUT.as_bytes());
        assert_eq!(stand().spurweiten.len(), landing_scoring::spurweite::lesen(landing_scoring::spurweite::EINGEBAUT).unwrap().len());
        assert_eq!(stand().vref.len(), landing_scoring::vref::lesen(landing_scoring::vref::EINGEBAUT).unwrap().len());
    }

    fn abdruck(text: &str) -> (u64, usize) {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in text.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        (h, text.len())
    }

    /// Hoehengruppen und Klappenraster wie 2.1.24: vor dem Entfernen der
    /// alten Funktionen fuer 450 Kennungen und alle Rastenfaelle (0..=12 x
    /// 0..=12) einmal gegen sie verglichen (gleich; verkuerzte G2-Liste bzw.
    /// geaenderte Stufe -> 5 Tests rot). Abdruck der gelesenen Tabellen;
    /// bewusste Aenderung: mit Begruendung nachziehen.
    #[test]
    fn hoehengruppen_und_klappen_wie_2_1_24() {
        let h = format!("{:?}", crate::runway_assessment::hoehengruppen_lesen(eingebauter_text(HOEHENGRUPPEN).unwrap()).unwrap());
        let k = format!("{:?}", klappen_lesen(eingebauter_text(KLAPPEN).unwrap()).unwrap());
        assert_eq!((abdruck(&h), abdruck(&k)), ((3041705867902806275, 1612), (14911868732692658409, 571)));
    }

    #[test]
    fn klappen_und_hoehengruppen_regelverstoss_ist_ein_lesefehler() {
        let k: serde_json::Value = serde_json::from_str(eingebauter_text(KLAPPEN).unwrap()).unwrap();
        let mut zu_wenig = k.clone();
        zu_wenig["raster"][0]["stufen"].as_array_mut().unwrap().pop();
        assert!(klappen_lesen(&zu_wenig.to_string()).is_err());
        let h: serde_json::Value = serde_json::from_str(eingebauter_text(HOEHENGRUPPEN).unwrap()).unwrap();
        let mut klein = h.clone();
        klein["gruppen"][0]["praefix"][0] = "b74".into();
        assert!(crate::runway_assessment::hoehengruppen_lesen(&klein.to_string()).is_err());
        let mut fallend = h;
        fallend["gleitpfad_zu_rad_ft"]["G4"] = 5.0.into();
        assert!(crate::runway_assessment::hoehengruppen_lesen(&fallend.to_string()).is_err());
    }

    /// Kabinen-Tabelle wie 2.1.24: alle 41 Werte und 4 Listen vor dem
    /// Entfernen der Konstanten per Skript gegen `git show` verglichen (0
    /// Abweichungen). Abdruck der gelesenen Tabelle; Datei = die des Crates.
    #[test]
    fn kabine_wie_2_1_24() {
        assert_eq!(eingebaute_datei(KABINE).unwrap(), cabin_announcements::EINGEBAUT.as_bytes());
        let t = format!("{:?}", cabin_announcements::lesen(eingebauter_text(KABINE).unwrap()).unwrap());
        assert_eq!(abdruck(&t), (12188610868671356069, 8442));
    }

    #[test]
    fn eingebaute_endpunkte_sind_die_des_crates() {
        assert_eq!(eingebaute_datei(ENDPUNKTE).unwrap(), paket_endpunkte::EINGEBAUT.as_bytes());
        assert_eq!(stand().endpunkte.live, "https://live.kant.ovh");
    }

    #[test]
    fn paket_datei_gibt_nur_freigegebene_dateien() {
        assert_eq!(paket_datei(FLOTTE.into()).unwrap().as_bytes(), eingebaute_datei(FLOTTE).unwrap());
        for sprache in ["de", "en", "it"] {
            let text = paket_datei(format!("texte/{sprache}.json")).unwrap();
            assert!(serde_json::from_str::<serde_json::Value>(text).unwrap().is_object());
        }
        assert!(paket_datei(CPDLC.into()).is_err());
        assert!(paket_datei("../geheim".into()).is_err());
    }

    #[test]
    fn eingebauter_stand_besteht_den_selbsttest() {
        assert_eq!(selbsttest(&eingebaut().paket().unwrap()), Ok(()));
    }

    /// Nie gemischt (P-3.02): Gilt schon ein Stand, wird ein zweiter
    /// vollstaendig abgewiesen — Grenzwerte UND Bahntabellen bleiben.
    #[test]
    fn ein_stand_gilt_ganz_oder_gar_nicht() {
        let vorher = stand();
        let mut anders = eingebauter_stand().unwrap();
        anders.grenzwerte.phasen.window_secs.wert = 5.0;
        anders.runways = "\"id\"";
        assert!(!uebernehmen(anders));
        assert_eq!(crate::grenzwerte::grenzwerte().phasen.window_secs.wert, 60.0);
        assert!(std::ptr::eq(stand(), vorher));
        assert_ne!(stand().runways, "\"id\"");
    }

    #[test]
    fn eingebauter_stand_ist_ein_gueltiges_paket_mit_ourairports() {
        let p = eingebaut().paket().expect("eingebauter Stand");
        assert_eq!(p.id.generation, 0);
        assert_eq!(p.datei(RUNWAYS).unwrap(), crate::runway::eingebaut_runways_csv().as_bytes());
        assert_eq!(p.datei(AIRPORTS).unwrap(), crate::runway::eingebaut_airports_csv().as_bytes());
        for d in crate::grenzwerte::DATEIEN {
            assert!(p.datei(d).is_ok(), "{d} fehlt im eingebauten Stand");
        }
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
        // Wie auf Windows beobachtet: Ein anderer Teil hat die Bahntabellen
        // schon gelesen, bevor `starten` laeuft. Der Pin darf trotzdem bleiben.
        let _ = stand();
        starten(&dir);
        assert_eq!(flug_pin("x"), Some(pin.clone()));
        // Ein spaeterer Flug im selben Lauf flog mit dem eingebauten Stand.
        let anderer = flug_pin("y").unwrap();
        assert_eq!(anderer.generation, 0);
        assert_ne!(anderer, pin);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
