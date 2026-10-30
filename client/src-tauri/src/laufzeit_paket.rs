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
/// `ui-befehle-1` (P-8.11): die Befehlsliste befehle.json dieses Programms —
/// ein Paket mit Oberflaeche (`ui/`) ruft genau diese Befehle. Neuer Befehl =
/// Release + `ui-befehle-2` im Paket-Workflow.
/// `ui-befehle-2` (P-9.22): `hilfe_vorschau`, `hilfe_anfordern` dazu; die
/// Liste ist eine Obermenge von 1, ein Paket fuer 1 laeuft weiter.
/// `ui-befehle-3` (30.10.2026): 17 Befehle von „system“ nach „fachlich“
/// (Anmeldung, Fenster, lokale Schalter, Einwilligung). Eine Oberflaeche
/// dieses Stands ruft sie direkt — ein Programm ohne die Umstufung wiese sie
/// ab. **Ersetzt** `ui-befehle-1/-2` (wie `grenzwerte-2` die `-1`): Eine
/// aeltere Oberflaeche schickte die 17 noch ueber `shell_request`, und die
/// Huelle nimmt nur noch echte Systembefehle an (Pruefer 30.10.). Ein solches
/// Paket passt nicht mehr (`paket_aus_ablage` prueft auch den aktiven Stand),
/// es gilt der eingebaute Stand, bis das Paket mit `ui-befehle-3` da ist.
/// `grenzwerte-2` (P-3.55 Nachtrag): zwoelf neue Felder in fuenf Bereichen;
/// ersetzt `grenzwerte-1`, denn ein Paket ohne sie liest dieses Programm
/// nicht (`deny_unknown_fields`, fehlende Felder -> eingebauter Stand).
/// `hilfe-formular-1` (30.10.2026): `tabellen/hilfe-formular.json`
/// (Zusatzfelder der Hilfe-Seite, Namen prueft der Kern beim Senden),
/// `endpunkte.json` mit `helpdesk_pfad` und das optionale Argument `zusatz`
/// von `hilfe_anfordern`. Ein aelteres Programm laese ein solches Paket
/// nicht (`deny_unknown_fields` in `paket-endpunkte`, Datei unbekannt) — mit
/// der Faehigkeit uebergeht es das Paket sauber (P-2.30) statt am Lesen zu
/// scheitern. Kein neuer Befehl: `befehle.json` bleibt.
const FAEHIGKEITEN: &[&str] = &["ourairports-1", "grenzwerte-2", "xplane-profile-1", "cpdlc-1", "flotte-1", "texte-1", "spurweiten-1", "vref-1", "hoehengruppen-1", "klappen-1", "kabine-1", "endpunkte-1", "msfs-profile-1", "bewertung-1", "kabine-modul-1", "ui-befehle-3", "hilfe-formular-1"];

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
/// Zusatzfelder der Hilfe-Seite (P-9.22, `hilfe-formular-1`). Als einzige
/// Datei **optional**: Ein Paket von vor `hilfe-formular-1` gilt auf diesem
/// Programm weiter (P-2.30), nur ohne Zusatzfelder.
const HILFE_FORMULAR: &str = "tabellen/hilfe-formular.json";

/// Fehlt diese Datei im Paket und darf sie fehlen?
fn darf_fehlen(p: &Paket, pfad: &str) -> bool {
    pfad == HILFE_FORMULAR && p.manifest.datei(pfad).is_none()
}
const MSFS_PROFILE: &str = "profile/msfs.json";
const BEWERTUNG: &str = "module/bewertung.wasm";
const KABINE_MODUL: &str = "module/kabine.wasm";
/// Referenzfaelle des Bewertungsmoduls (P-3.21, P-13.02).
const SELBSTTEST: &str = "selftest.json";

/// Dateien, die die Oberflaeche ueber [`paket_datei`] lesen darf: feste
/// Liste, nur lesen, nur aus dem geltenden Stand.
const UI_DATEIEN: &[&str] = &[FLOTTE, "texte/de.json", "texte/en.json", "texte/it.json", HILFE_FORMULAR];

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
    /// Dieselbe Tabelle als Text: Das Kabinenmodul bekommt sie mit dem
    /// ersten Befehl (Etappe 4, P-3.59).
    pub kabine_json: &'static str,
    /// Adressen der Gegenstellen, nur innerhalb der festen Hostliste von
    /// `paket-endpunkte` (P-3.60).
    pub endpunkte: paket_endpunkte::Endpunkte,
    /// Zusatzfelder der Hilfe-Seite: die Namen, die `hilfe_anfordern`
    /// annimmt (die Oberflaeche liest dieselbe Datei ueber `paket_datei`).
    pub hilfe_formular: crate::hilfe::HilfeFormular,
    /// MSFS-Paketprofile fuer neue Add-ons (Etappe 2, P-3.52).
    pub msfs_profile: sim_core::msfs_paketprofil::Datei,
    /// Bewertungsmodul (WASM, P-4.31) dieses Stands.
    pub bewertung: &'static [u8],
    /// Kabinenplaner (WASM, P-3.58) dieses Stands.
    pub kabine_modul: &'static [u8],
    /// Rohtexte der [`UI_DATEIEN`] aus diesem Stand.
    pub ui: Vec<(&'static str, &'static str)>,
    /// OurAirports-Commit aus dem Manifest (`quelle.ourairports_commit`);
    /// `None` beim eingebauten Stand. Herkunft im Eingabe-Dokument (P-9.44).
    pub ourairports_commit: Option<String>,
}

static STAND: OnceLock<AktiverStand> = OnceLock::new();

/// Einstieg der nachgeladenen Oberflaeche im Paket (Etappe 5, P-8.02).
pub(crate) const UI_EINSTIEG: &str = "ui/index.html";

/// Das Paket des geltenden Stands, wenn es eine Oberflaeche (`ui/`) traegt.
/// Gesetzt nur zusammen mit dem Stand aus demselben Paket (P-3.02).
static UI_PAKET: OnceLock<Paket> = OnceLock::new();

/// Die Oberflaeche dieses Laufs: `Some` = `ui/` der aktiven Generation,
/// `None` = das eingebaute Frontend (`ui_lader`).
pub(crate) fn ui_paket() -> Option<&'static Paket> {
    UI_PAKET.get()
}

/// PIREP und Stand eines offenen Flugs, dessen Stand in diesem Lauf NICHT gilt
/// (fehlte lokal oder kam zu spaet). Gilt nur fuer genau diesen Flug (P-3.31).
static PIN: OnceLock<(String, RuntimeBundleId)> = OnceLock::new();

/// P-3.31: das Bewertungsmodul des gepinnten Stands, sobald `pin_nachholen`
/// ihn im laufenden Programm abgelegt hat (aktiv wird der Stand erst beim
/// naechsten Start, P-3.20 — die Bewertung des offenen Flugs braucht ihn
/// aber vor der Abgabe).
static PIN_MODUL: OnceLock<&'static [u8]> = OnceLock::new();

/// P-3.31: Gilt fuer den Flug sein gepinnter Stand?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PinStand {
    /// Der Stand des Flugs ist der geltende.
    Gilt,
    /// Er fehlt: gerechnet wird mit der eingebauten Fassung, „vorlaeufig“.
    Fehlt,
    /// Er wurde nachgeholt: sein Bewertungsmodul.
    Nachgeholt(&'static [u8]),
}

/// P-3.31: der Pinstand des Flugs `pirep_id`.
pub(crate) fn pin_stand(pirep_id: &str) -> PinStand {
    match PIN.get() {
        Some((pirep, _)) if pirep == pirep_id => match PIN_MODUL.get() {
            Some(m) => PinStand::Nachgeholt(m),
            None => PinStand::Fehlt,
        },
        _ => PinStand::Gilt,
    }
}

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
        kabine_json: text(KABINE)?,
        endpunkte: paket_endpunkte::lesen(text(ENDPUNKTE)?)?,
        hilfe_formular: if darf_fehlen(p, HILFE_FORMULAR) {
            crate::hilfe::HilfeFormular::default()
        } else {
            crate::hilfe::hilfe_formular_lesen(text(HILFE_FORMULAR)?)?
        },
        msfs_profile: sim_core::msfs_paketprofil::lesen(text(MSFS_PROFILE)?)?,
        bewertung: Box::leak(p.datei(BEWERTUNG).map_err(|e| e.to_string())?.into_boxed_slice()),
        kabine_modul: Box::leak(p.datei(KABINE_MODUL).map_err(|e| e.to_string())?.into_boxed_slice()),
        vref: landing_scoring::vref::lesen(text(VREF)?)?,
        ui: UI_DATEIEN
            .iter()
            .filter(|&&d| !darf_fehlen(p, d))
            .map(|&d| Ok((d, text(d)?)))
            .collect::<Result<_, String>>()?,
        ourairports_commit: p.manifest.quelle.ourairports_commit.clone(),
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
        kabine_json: eingebauter_text(KABINE)?,
        endpunkte: paket_endpunkte::lesen(eingebauter_text(ENDPUNKTE)?)?,
        hilfe_formular: crate::hilfe::hilfe_formular_lesen(eingebauter_text(HILFE_FORMULAR)?)?,
        msfs_profile: sim_core::msfs_paketprofil::lesen(eingebauter_text(MSFS_PROFILE)?)?,
        bewertung: eingebaute_datei(BEWERTUNG).ok_or(BEWERTUNG)?,
        kabine_modul: eingebaute_datei(KABINE_MODUL).ok_or(KABINE_MODUL)?,
        vref: landing_scoring::vref::lesen(eingebauter_text(VREF)?)?,
        ui: UI_DATEIEN.iter().map(|&d| Ok((d, eingebauter_text(d)?))).collect::<Result<_, String>>()?,
        ourairports_commit: None,
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

/// Das Bewertungsmodul des eingebauten Stands (Rueckfall, P-4.33).
pub(crate) fn eingebautes_bewertungsmodul() -> Option<&'static [u8]> {
    eingebaute_datei(BEWERTUNG)
}

/// Der Kabinenplaner des eingebauten Stands (Rueckfall, Etappe 4).
pub(crate) fn eingebautes_kabinenmodul() -> Option<&'static [u8]> {
    eingebaute_datei(KABINE_MODUL)
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
    eingebaut_mit(&[])
}

/// Wie [`eingebaut`], einzelne Dateien ersetzt (Tests: ein Paket, das sich
/// nur in diesen Dateien unterscheidet).
fn eingebaut_mit(ersetzt: &[(&str, &[u8])]) -> Eingebaut {
    eingebaut_mit_ohne(ersetzt, &[])
}

/// Wie [`eingebaut_mit`], dazu ohne die Dateien `ohne` (Tests: aelteres Paket).
fn eingebaut_mit_ohne(ersetzt: &[(&str, &[u8])], ohne: &[&str]) -> Eingebaut {
    let inhalt: Vec<(&str, &[u8])> = eingebettet::DATEIEN
        .iter()
        .filter(|(p, _)| !ohne.contains(p))
        .map(|&(p, b)| (p, ersetzt.iter().find(|(e, _)| *e == p).map_or(b, |(_, n)| *n)))
        .collect();
    let dateien: HashMap<String, Vec<u8>> = inhalt.iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    let files: Vec<_> = inhalt
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
    sim_core::msfs_paketprofil::quelle_setzen(|| &stand().msfs_profile);
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
    let (neu, aus_paket) = match stand_aus_paket(&paket) {
        Ok(neu) => (Some(neu), true),
        Err(e) => {
            tracing::warn!(stand = %paket.id, error = %e, "Laufzeit-Paket: Stand nicht anwendbar, eingebauter Stand gilt");
            (eingebauter_stand().ok(), false)
        }
    };
    if let Some(neu) = neu {
        if !uebernehmen(neu) {
            tracing::warn!("Laufzeit-Paket: ein Stand galt schon (vorher gelesen), er bleibt");
        } else if aus_paket && paket.manifest.datei(UI_EINSTIEG).is_some() {
            // Etappe 5 (P-8.20): Die Oberflaeche kommt aus DERSELBEN
            // Generation wie die Daten — oder ganz aus dem Programm.
            let _ = UI_PAKET.set(paket);
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

/// P-3.04: Zeitpunkt der letzten erfolgreichen Pruefung bei live (Datei in
/// der Ablage, RFC 3339).
const LETZTE_PRUEFUNG: &str = "letzte_pruefung";
/// P-3.04: Nach so vielen Tagen ohne erfolgreiche Pruefung gibt es einen Hinweis.
const PRUEFUNG_HINWEIS_TAGE: i64 = 30;

fn pruefung_datei(app_daten: &Path) -> PathBuf {
    app_daten.join("laufzeit-paket").join(LETZTE_PRUEFUNG)
}

/// P-3.04: Eine Pruefung war erfolgreich — live hat geantwortet und der Stand
/// ist bekannt: neuer Stand installiert, schon aktuell (auch 404 „kein Stand
/// auf live“) oder im Laden durch einen Flugbeginn abgebrochen.
fn pruefung_vermerken(app_daten: &Path, jetzt: chrono::DateTime<chrono::Utc>) {
    let datei = pruefung_datei(app_daten);
    if let Some(ordner) = datei.parent() {
        let _ = std::fs::create_dir_all(ordner);
    }
    if let Err(e) = std::fs::write(&datei, jetzt.to_rfc3339()) {
        tracing::warn!(error = %e, "Laufzeit-Paket: Zeitpunkt der Pruefung nicht speicherbar");
    }
}

/// P-3.04: Tage seit der letzten erfolgreichen Pruefung, wenn es mehr als 30
/// sind (sonst `None`). Ohne Vermerk (erster Start, noch nie geprueft) beginnt
/// die Zaehlung jetzt — der Vermerk wird angelegt.
pub(crate) fn pruefung_ueberfaellig(app_daten: &Path, jetzt: chrono::DateTime<chrono::Utc>) -> Option<i64> {
    let letzte = std::fs::read_to_string(pruefung_datei(app_daten))
        .ok()
        .and_then(|t| chrono::DateTime::parse_from_rfc3339(t.trim()).ok())
        .map(|t| t.with_timezone(&chrono::Utc));
    let Some(letzte) = letzte else {
        pruefung_vermerken(app_daten, jetzt);
        return None;
    };
    let seit = jetzt - letzte;
    (seit > chrono::Duration::days(PRUEFUNG_HINWEIS_TAGE)).then(|| seit.num_days())
}

/// Holt einen neueren Stand von live und installiert ihn; aktiv wird er erst
/// beim naechsten Programmstart (P-3.20). Nicht bei offenem Flug, nicht ohne
/// Pruefschluessel (dann bestuende ohnehin kein Stand die Pruefung).
///
/// P-3.04: Danach — oder sofort, wenn im Flug nicht geprueft wird — ruft es
/// `hinweis(tage)`, falls die letzte erfolgreiche Pruefung mehr als 30 Tage
/// zurueckliegt. Der aktive Stand laeuft trotzdem weiter.
pub(crate) fn aktualisieren_im_hintergrund(app_daten: PathBuf, hinweis: impl FnOnce(i64) + Send + 'static) {
    if PRUEFSCHLUESSEL.is_empty() {
        return;
    }
    if flug_offen(&app_daten) {
        if let Some(tage) = pruefung_ueberfaellig(&app_daten, chrono::Utc::now()) {
            hinweis(tage);
        }
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
            Ok(Some(id)) => {
                pruefung_vermerken(&app_daten, chrono::Utc::now());
                tracing::info!(stand = %id, "Laufzeit-Paket: neuer Stand, aktiv ab naechstem Start");
            }
            Ok(None) => pruefung_vermerken(&app_daten, chrono::Utc::now()),
            Err(e) => tracing::warn!(error = %e, "Laufzeit-Paket: Aktualisierung nicht moeglich"),
        }
        if let Some(tage) = pruefung_ueberfaellig(&app_daten, chrono::Utc::now()) {
            hinweis(tage);
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
            .map_err(|e| {
                // P-3.21: Ein Stand, der seinen Selbsttest nicht besteht, wird
                // nicht aktiviert — und live (GlitchTip) erfaehrt es.
                if let runtime_bundle::Fehler::Selbsttest(grund) = &e {
                    crate::sentry_init::capture_activity(
                        "Laufzeit-Paket: Selbsttest gescheitert",
                        Some(&format!("Generation {}: {grund}", a.generation)),
                        sentry::Level::Error,
                    );
                }
                e.to_string()
            })?;
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
            .map_err(|e| e.to_string())?;
        // P-3.31: ab jetzt rechnet der offene Flug mit dem Modul seines Stands
        // (Neurechnung vor der Abgabe, `bewertung::bewerten_fuer`).
        let l = ablage.laden(Some(&pin), &kontext(), &eingebaut()).map_err(|e| e.to_string())?;
        if l.herkunft != Herkunft::Pin {
            return Err(format!("nachgeholter Stand {pin} nicht ladbar: {:?}", l.hinweise));
        }
        let modul = l.paket.datei(BEWERTUNG).map_err(|e| e.to_string())?;
        let _ = PIN_MODUL.set(Box::leak(modul.into_boxed_slice()));
        Ok(())
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
        (HILFE_FORMULAR, |t| crate::hilfe::hilfe_formular_lesen(t).map(|_| ())),
        (MSFS_PROFILE, |t| sim_core::msfs_paketprofil::lesen(t).map(|_| ())),
    ] {
        if darf_fehlen(p, pfad) {
            continue;
        }
        let roh = p.datei(pfad).map_err(|e| e.to_string())?;
        pruefen(std::str::from_utf8(&roh).map_err(|e| format!("{pfad}: {e}"))?)?;
    }
    // Bewertungsmodul: muss laden, die Exporte haben und jeden Fall aus
    // selftest.json exakt treffen (P-3.21, P-13.02).
    let modul = p.datei(BEWERTUNG).map_err(|e| e.to_string())?;
    let faelle = p.datei(SELBSTTEST).map_err(|e| e.to_string())?;
    bewertung_selbsttest(&modul, &faelle)?;
    // Kabinenplaner: muss laden und mit der Tabelle DIESES Stands einen
    // Planer anlegen (Etappe 4).
    let modul = p.datei(KABINE_MODUL).map_err(|e| e.to_string())?;
    let tabelle = p.datei(KABINE).map_err(|e| e.to_string())?;
    crate::kabinenplaner::pruefen(&modul, std::str::from_utf8(&tabelle).map_err(|e| format!("{KABINE}: {e}"))?)
        .map_err(|e| format!("{KABINE_MODUL}: {e}"))?;
    for d in UI_DATEIEN.iter().filter(|&&d| !darf_fehlen(p, d)) {
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

#[derive(serde::Deserialize)]
struct Selbsttest {
    faelle: Vec<SelbsttestFall>,
}

#[derive(serde::Deserialize)]
struct SelbsttestFall {
    name: String,
    eingabe: String,
    ausgabe: String,
}

/// P-3.21: Das Modul des Pakets rechnet jeden Referenzfall mit der Grenze
/// des Betriebs (`FUEL_GRENZE`) und muss die erwartete Ausgabe Zeichen fuer
/// Zeichen treffen (P-12.03: byte-gleich). Verglichen wird roh, nicht ueber
/// die Ausgabe-Struktur dieses Programms — ein neueres Modul darf Felder
/// haben, die dieses Programm nicht kennt.
fn bewertung_selbsttest(modul: &[u8], selftest: &[u8]) -> Result<(), String> {
    let s: Selbsttest = serde_json::from_slice(selftest).map_err(|e| format!("{SELBSTTEST}: {e}"))?;
    if s.faelle.is_empty() {
        return Err(format!("{SELBSTTEST}: keine Faelle"));
    }
    // P-4.21: Auch das Soll-Band kommt aus dem Modul — ohne den Export
    // `sollband` fiele die Anzeige still auf das eingebaute zurueck (Codex 30.10.).
    let host = bewertung_host::Host::neu_mit(modul, &["memory", "alloc", "bewerte", "sollband"])
        .map_err(|e| format!("{BEWERTUNG}: {e}"))?;
    let band = host
        .sollband(r#"{"proben":[{"gs_kt":140.0}],"winkel_grad":3.0}"#, crate::bewertung::FUEL_GRENZE)
        .map_err(|e| format!("{BEWERTUNG}: sollband: {e}"))?;
    if serde_json::from_str::<Vec<landing_scoring::sollband::Punkt>>(&band).map_or(true, |p| p.len() != 1) {
        return Err(format!("{BEWERTUNG}: sollband rechnet nicht: {}", band.chars().take(120).collect::<String>()));
    }
    for f in &s.faelle {
        let aus = host
            .bewerten(&f.eingabe, crate::bewertung::FUEL_GRENZE)
            .map_err(|e| format!("{SELBSTTEST}: {}: {e}", f.name))?;
        if aus != f.ausgabe {
            return Err(format!("{SELBSTTEST}: {} weicht ab", f.name));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    /// Ein Paket aus dem Paket-Workflow verlangt nur, was dieses Programm
    /// kann — sonst lehnte der Client sein eigenes Paket ab (P-2.15). Mit
    /// Oberflaeche (ui/) gehoert die Befehlsliste dieses Stands dazu (P-8.11):
    /// seit der Hilfe-Funktion `ui-befehle-2` — eine Oberflaeche, die
    /// `hilfe_anfordern` ruft, darf nicht in einen Client ohne den Befehl.
    #[test]
    fn paket_workflow_verlangt_nur_bekannte_faehigkeiten() {
        let wf = include_str!("../../../.github/workflows/bundle.yml");
        let liste = wf.split("REQUIRES: ").nth(1).expect("REQUIRES").split_whitespace().next().unwrap();
        // Jeder Bau-Laeufer nimmt genau diese Liste (P-3.03: gleiche Pakete).
        let bauten: Vec<&str> = wf.split("--requires ").skip(1).collect();
        assert!(bauten.len() >= 2 && bauten.iter().all(|r| r.starts_with("\"$REQUIRES\"")), "jeder Bau mit $REQUIRES");
        for f in liste.split(',') {
            assert!(FAEHIGKEITEN.contains(&f), "bundle.yml verlangt {f}, das Programm kennt es nicht");
        }
        assert!(liste.split(',').any(|f| f == "ui-befehle-3"), "Paket mit ui/ ohne ui-befehle-3");
        // Gegenprobe: eine aeltere Oberflaeche (ui-befehle-1/-2) passt nicht mehr.
        assert!(!FAEHIGKEITEN.contains(&"ui-befehle-2") && !FAEHIGKEITEN.contains(&"ui-befehle-1"));
        assert!(liste.split(',').any(|f| f == "grenzwerte-2"), "Paket mit den Grenzwerten dieses Stands ohne grenzwerte-2");
    }

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
        // 29.10. nachgezogen: nur `quelle` korrigiert (Zeilen gegen v2.1.24,
        // phase_sinkflug_hoechstens_rest_min stammt aus e073b562), Werte gleich.
        assert_eq!(abdruck(&t), (13780505455500687198, 8453));
    }

    /// Etappe 4, Abnahme „Kabinenablauf ohne Release geaendert“: Ein Paket,
    /// das sich nur in `tabellen/kabine.json` bzw. `module/kabine.wasm`
    /// unterscheidet, plant anders — gelesen ueber `stand_aus_paket` wie
    /// beim Programmstart, ohne Aenderung am Client.
    #[test]
    fn kabinenablauf_aendert_sich_mit_dem_paket() {
        use cabin_announcements::{modul::Befehl, Flugart, Flugdaten, Slot};
        let aus = |ersetzt: &[(&str, &[u8])]| stand_aus_paket(&eingebaut_mit(ersetzt).paket().unwrap()).unwrap();
        // 165-min-Flug im Reiseflug: Halbzeit-Ansage und Weg.
        let plane = |s: &AktiverStand| {
            let flug = Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(165), abheben_epoch: Some(1_000), ..Default::default() };
            let anlegen = Befehl::Fortsetzen { tabelle: s.kabine_json.into(), flug, phase: crate::FlightPhase::Cruise };
            let p = crate::kabinenplaner::Planer::starten(s.kabine_modul, eingebautes_kabinenmodul(), s.kabine_json, anlegen);
            (p.zeitpunkt(Slot::Reiseflug50, Some(1_000 + 165 * 60)), p.wechsel_abholen())
        };
        let (halbzeit, weg) = plane(&aus(&[]));
        assert!(halbzeit.is_some(), "Bezug: Halbzeit geplant");
        assert_eq!(weg, vec![("paket", None)]);
        // Tabelle: Kurzstrecke bis 200 min — der Flug hat keine Halbzeit mehr.
        let mut t: serde_json::Value = serde_json::from_str(eingebauter_text(KABINE).unwrap()).unwrap();
        t["werte"]["kurzstrecke_min"]["wert"] = 200.into();
        let t = t.to_string();
        let (halbzeit, weg) = plane(&aus(&[(KABINE, t.as_bytes())]));
        assert_eq!(halbzeit, None, "Tabelle aus dem Paket");
        assert_eq!(weg, vec![("paket", None)]);
        // Modul: ein Paketmodul, das nichts plant (antwortet immer `null`).
        let stumm = wat::parse_str(
            r#"(module (memory (export "memory") 1)
            (data (i32.const 16) "{\"Ok\":null}")
            (func (export "alloc") (param i32) (result i32) i32.const 1024)
            (func (export "kabine") (param i32 i32) (result i64) i64.const 0x000000100000000B))"#,
        )
        .unwrap();
        let (halbzeit, weg) = plane(&aus(&[(KABINE_MODUL, &stumm)]));
        assert_eq!(halbzeit, None, "Modul aus dem Paket");
        assert_eq!(weg, vec![("paket", None)]);
    }

    #[test]
    fn eingebaute_msfs_profile_sind_die_des_crates() {
        assert_eq!(eingebaute_datei(MSFS_PROFILE).unwrap(), sim_core::msfs_paketprofil::EINGEBAUT.as_bytes());
        assert_eq!(stand().msfs_profile, sim_core::msfs_paketprofil::lesen(sim_core::msfs_paketprofil::EINGEBAUT).unwrap());
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
        // Die Hilfe-Seite liest ihre Zusatzfelder aus demselben Stand.
        assert_eq!(paket_datei(HILFE_FORMULAR.into()).unwrap().as_bytes(), eingebaute_datei(HILFE_FORMULAR).unwrap());
        assert!(paket_datei(CPDLC.into()).is_err());
        assert!(paket_datei("../geheim".into()).is_err());
    }

    #[test]
    fn eingebauter_stand_besteht_den_selbsttest() {
        assert_eq!(selbsttest(&eingebaut().paket().unwrap()), Ok(()));
    }

    /// Ein Zusatzfeld im Paket aendert, was der Kern annimmt — ohne
    /// Programmaenderung; ein Feld mit Geheimnis-Namen verwirft das Paket.
    #[test]
    fn hilfe_formular_kommt_aus_dem_paket() {
        let neu = br#"{"format":1,"felder":[{"name":"flugnummer","typ":"text","bezeichnung":{"de":"Flugnummer"}}]}"#;
        let p = eingebaut_mit(&[(HILFE_FORMULAR, neu)]).paket().unwrap();
        assert_eq!(selbsttest(&p), Ok(()));
        assert_eq!(stand_aus_paket(&p).unwrap().hilfe_formular.namen, ["flugnummer"]);
        let boese = br#"{"format":1,"felder":[{"name":"api_key","typ":"text"}]}"#;
        let fehler = selbsttest(&eingebaut_mit(&[(HILFE_FORMULAR, boese)]).paket().unwrap()).unwrap_err();
        assert!(fehler.contains("hilfe-formular.json"), "{fehler}");
        // Pfad aus dem Paket: anderer Pfad geht, fremder Host nicht.
        let mut e: serde_json::Value = serde_json::from_slice(eingebaute_datei(ENDPUNKTE).unwrap()).unwrap();
        e["helpdesk_pfad"] = "/api/helpdesk/v2/tickets".into();
        let p = eingebaut_mit(&[(ENDPUNKTE, e.to_string().as_bytes())]).paket().unwrap();
        assert_eq!(stand_aus_paket(&p).unwrap().endpunkte.helpdesk_pfad(), "/api/helpdesk/v2/tickets");
        e["helpdesk_pfad"] = "https://evil.example/api/t".into();
        assert!(selbsttest(&eingebaut_mit(&[(ENDPUNKTE, e.to_string().as_bytes())]).paket().unwrap()).is_err());
    }

    /// P-2.30: Ein Paket von vor `hilfe-formular-1` (ohne die Datei, ohne
    /// `helpdesk_pfad`) gilt weiter — ohne Zusatzfelder, mit dem heutigen Pfad.
    /// Gegenprobe: Jede andere Pflichtdatei darf weiterhin nicht fehlen.
    #[test]
    fn aelteres_paket_ohne_hilfe_formular_gilt_weiter() {
        let mut e: serde_json::Value = serde_json::from_slice(eingebaute_datei(ENDPUNKTE).unwrap()).unwrap();
        e.as_object_mut().unwrap().remove("helpdesk_pfad");
        let alt = eingebaut_mit_ohne(&[(ENDPUNKTE, e.to_string().as_bytes())], &[HILFE_FORMULAR]).paket().unwrap();
        assert!(alt.manifest.datei(HILFE_FORMULAR).is_none());
        assert_eq!(selbsttest(&alt), Ok(()));
        let stand = stand_aus_paket(&alt).unwrap();
        assert!(stand.hilfe_formular.namen.is_empty());
        assert_eq!(stand.endpunkte.helpdesk_pfad(), "/api/helpdesk/tickets");
        assert!(stand.ui.iter().all(|(d, _)| *d != HILFE_FORMULAR));
        assert!(stand.ui.iter().any(|(d, _)| *d == FLOTTE));
        let ohne_klappen = eingebaut_mit_ohne(&[], &[KLAPPEN]).paket().unwrap();
        assert!(selbsttest(&ohne_klappen).is_err());
        assert!(stand_aus_paket(&ohne_klappen).is_err());
    }

    /// P-3.21/P-13.02: Das Paket traegt den ganzen Korpus (je Simulator
    /// mindestens 20 Faelle), und eine einzige abweichende Erwartung
    /// verwirft das Paket — dann bleibt der bisherige Stand (P-3.20).
    #[test]
    fn selbsttest_verwirft_ein_paket_mit_abweichender_erwartung() {
        let roh = eingebaute_datei(SELBSTTEST).expect("selftest.json im Paket");
        let mut v: serde_json::Value = serde_json::from_slice(roh).unwrap();
        let namen: Vec<&str> = v["faelle"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
        for sim in ["msfs_", "xplane_"] {
            assert!(namen.iter().filter(|n| n.starts_with(sim)).count() >= 20, "{sim}");
        }
        let letzter = v["faelle"].as_array().unwrap().len() - 1;
        let aus = v["faelle"][letzter]["ausgabe"].as_str().unwrap().replacen("\"gesamt\":", "\"gesamt\":1", 1);
        v["faelle"][letzter]["ausgabe"] = aus.into();
        let falsch = serde_json::to_vec(&v).unwrap();
        let fehler = selbsttest(&eingebaut_mit(&[(SELBSTTEST, &falsch)]).paket().unwrap()).unwrap_err();
        assert!(fehler.contains("weicht ab"), "{fehler}");
        // Ein Modul ohne Export `sollband` (P-4.21) wird abgelehnt (Codex 30.10.).
        let ohne_sollband = wat::parse_str(
            r#"(module (memory (export "memory") 1)
                 (func (export "alloc") (param i32) (result i32) i32.const 0)
                 (func (export "bewerte") (param i32 i32) (result i64) i64.const 0))"#,
        )
        .unwrap();
        let fehler = selbsttest(&eingebaut_mit(&[(BEWERTUNG, &ohne_sollband)]).paket().unwrap()).unwrap_err();
        assert!(fehler.contains("sollband"), "{fehler}");
        // Ein Paket ohne einen einzigen Fall prueft nichts — abgelehnt.
        let leer = br#"{"format":1,"faelle":[]}"#;
        assert!(selbsttest(&eingebaut_mit(&[(SELBSTTEST, leer)]).paket().unwrap()).is_err());
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

    /// P-3.04: Hinweis erst NACH 30 Tagen ohne erfolgreiche Pruefung: nie
    /// geprueft beginnt die Zaehlung (kein Hinweis), 31 Tage → Hinweis,
    /// Gegenprobe 29 Tage → keiner; eine erfolgreiche Pruefung setzt zurueck.
    #[test]
    fn hinweis_nach_dreissig_tagen_ohne_pruefung() {
        let dir = std::env::temp_dir().join(format!("aeroacars-pruefung-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let jetzt = chrono::Utc::now();
        let tag = chrono::Duration::days(1);
        assert_eq!(pruefung_ueberfaellig(&dir, jetzt), None, "nie geprueft: Zaehlung beginnt");
        assert_eq!(pruefung_ueberfaellig(&dir, jetzt + tag * 29), None, "29 Tage: noch kein Hinweis");
        assert_eq!(pruefung_ueberfaellig(&dir, jetzt + tag * 31), Some(31), "nie erfolgreich, 31 Tage");
        pruefung_vermerken(&dir, jetzt - tag * 31);
        assert_eq!(pruefung_ueberfaellig(&dir, jetzt), Some(31), "letzter Erfolg vor 31 Tagen");
        pruefung_vermerken(&dir, jetzt - tag * 29);
        assert_eq!(pruefung_ueberfaellig(&dir, jetzt), None, "Gegenprobe: 29 Tage");
        let _ = std::fs::remove_dir_all(&dir);
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
