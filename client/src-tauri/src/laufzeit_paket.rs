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
/// `ui-befehle-4` (30.10.2026, Analyse V1a/V1b): Obermenge von -3 mit dem
/// Lesebefehl `paket_info`; `paket_datei` liest jede Datendatei unter
/// tabellen/, texte/, grenzwerte/ (vorher feste Liste). Eine Oberflaeche,
/// die das nutzt, verlangt -4 — ein Beta-1-Programm ohne -4 uebergeht ein
/// solches Paket und nimmt den eingebauten Stand (P-2.30). -3 bleibt, weil
/// sich an den bisherigen Befehlen nichts aendert. Dazu `paket_jetzt_pruefen`
/// und `paket_neu_starten` (fachlich, Kern prueft).
/// `kabine-lage-1` (Analyse V2): Der Kabinenplaner bekommt je Takt das
/// Flugbild `Tick::lage` (Snapshot, Plan, Ist-Zeiten, Bahn). Ein
/// `kabine.wasm`, dessen Regeln darauf bauen, verlangt die Faehigkeit; ein
/// aelteres Programm uebergeht das Paket (P-2.30). Erst in `bundle.yml`
/// REQUIRES, wenn ein Modul `lage` liest.
const FAEHIGKEITEN: &[&str] = &["ourairports-1", "grenzwerte-2", "xplane-profile-1", "cpdlc-1", "flotte-1", "texte-1", "spurweiten-1", "vref-1", "hoehengruppen-1", "klappen-1", "kabine-1", "endpunkte-1", "msfs-profile-1", "bewertung-1", "kabine-modul-1", "ui-befehle-3", "ui-befehle-4", "hilfe-formular-1", "kabine-lage-1"];

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

/// Dateien, die die eingebaute Oberflaeche beim Start liest: Jedes Paket
/// muss sie tragen (Selbsttest), `hilfe-formular.json` nur ab
/// `hilfe-formular-1`.
const UI_DATEIEN: &[&str] = &[FLOTTE, "texte/de.json", "texte/en.json", "texte/it.json", HILFE_FORMULAR];

/// `ui-befehle-4` (Analyse-Vorschlag V1b): Die Oberflaeche liest ueber
/// [`paket_datei`] jede reine Datendatei dieser Ordner des geltenden Stands —
/// eine neue Tabelle braucht so keinen Installer. Module, Selbsttest,
/// Endpunkte, Profile und OurAirports bleiben gesperrt.
const LESBARE_ORDNER: &[&str] = &["tabellen/", "texte/", "grenzwerte/"];
/// Groesste lesbare Datei (die Texte haben ~0,3 MB).
const LESBAR_MAX_BYTES: u64 = 2 * 1024 * 1024;

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
    /// Das Paket dieses Stands; `None` beim eingebauten Stand (dessen
    /// Dateien liegen statisch im Programm, [`eingebaute_datei`]).
    pub paket: Option<Paket>,
    /// Manifest dieses Stands (auch des eingebauten) fuer [`paket_info`].
    pub manifest: Manifest,
    /// OurAirports-Commit aus dem Manifest (`quelle.ourairports_commit`);
    /// `None` beim eingebauten Stand. Herkunft im Eingabe-Dokument (P-9.44).
    pub ourairports_commit: Option<String>,
}

static STAND: OnceLock<AktiverStand> = OnceLock::new();

/// Herkunft des geltenden Stands (gesetzt von `starten`; ohne `starten`
/// gilt der eingebaute Stand).
static HERKUNFT: OnceLock<Herkunft> = OnceLock::new();

/// Ein Paket, das sich beim Start nicht anwenden liess (`stand_aus_paket`
/// scheiterte): Es wuerde auch beim naechsten Start nicht gelten, ist also
/// kein „neues Paket“ (sonst stuende der Hinweis fuer immer da).
static NICHT_ANWENDBAR: OnceLock<RuntimeBundleId> = OnceLock::new();

/// Einstieg der nachgeladenen Oberflaeche im Paket (Etappe 5, P-8.02).
pub(crate) const UI_EINSTIEG: &str = "ui/index.html";

/// Die Oberflaeche dieses Laufs: `Some` = `ui/` der aktiven Generation,
/// `None` = das eingebaute Frontend (`ui_lader`). Dasselbe Paket wie die
/// Daten (P-3.02), denn es haengt am Stand.
pub(crate) fn ui_paket() -> Option<&'static Paket> {
    stand().paket.as_ref().filter(|p| p.manifest.datei(UI_EINSTIEG).is_some())
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

fn stand_aus_paket(p: Paket) -> Result<AktiverStand, String> {
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
        hilfe_formular: if darf_fehlen(&p, HILFE_FORMULAR) {
            crate::hilfe::HilfeFormular::default()
        } else {
            crate::hilfe::hilfe_formular_lesen(text(HILFE_FORMULAR)?)?
        },
        msfs_profile: sim_core::msfs_paketprofil::lesen(text(MSFS_PROFILE)?)?,
        bewertung: Box::leak(p.datei(BEWERTUNG).map_err(|e| e.to_string())?.into_boxed_slice()),
        kabine_modul: Box::leak(p.datei(KABINE_MODUL).map_err(|e| e.to_string())?.into_boxed_slice()),
        vref: landing_scoring::vref::lesen(text(VREF)?)?,
        ourairports_commit: p.manifest.quelle.ourairports_commit.clone(),
        manifest: p.manifest.clone(),
        paket: Some(p),
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
        ourairports_commit: None,
        manifest: p.manifest,
        paket: None,
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

/// Darf die Oberflaeche diese Datei lesen? Nur `.json` direkt unter
/// [`LESBARE_ORDNER`] (eine Ebene), nur die Zeichen der Manifest-Pfade,
/// kein `..`.
fn lesbar(pfad: &str) -> bool {
    let Some(name) = LESBARE_ORDNER.iter().find_map(|o| pfad.strip_prefix(o)) else {
        return false;
    };
    name.len() > ".json".len()
        && name.ends_with(".json")
        && !name.starts_with('.')
        && !name.contains("..")
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Eine Datendatei des geltenden Stands fuer die Oberflaeche (P-3.53,
/// P-3.56; `ui-befehle-4`: alle Dateien der [`LESBARE_ORDNER`]). Aus dem
/// Paket mit Hashpruefung bei jedem Lesen; Groesse vorab aus dem Manifest.
#[tauri::command]
pub(crate) fn paket_datei(pfad: String) -> Result<String, String> {
    datei_lesen(stand(), &pfad)
}

fn datei_lesen(s: &AktiverStand, pfad: &str) -> Result<String, String> {
    let d = s
        .manifest
        .datei(pfad)
        .filter(|_| lesbar(pfad))
        .ok_or_else(|| format!("{pfad}: nicht freigegeben"))?;
    if d.size > LESBAR_MAX_BYTES {
        return Err(format!("{pfad}: zu gross"));
    }
    let bytes = match &s.paket {
        Some(p) => p.datei(pfad).map_err(|e| e.to_string())?,
        None => eingebaute_datei(pfad).ok_or_else(|| format!("{pfad}: fehlt"))?.to_vec(),
    };
    String::from_utf8(bytes).map_err(|e| format!("{pfad}: {e}"))
}

/// Name der Herkunft fuer die Oberflaeche: `paket` (aktuell oder vorher),
/// `pin` (Stand des offenen Flugs) oder `eingebaut`.
fn herkunft_name(h: Herkunft) -> (&'static str, &'static str) {
    match h {
        Herkunft::Aktuell => ("paket", "aktuell"),
        Herkunft::Vorher => ("paket", "vorher"),
        Herkunft::Pin => ("pin", "pin"),
        Herkunft::Eingebaut => ("eingebaut", "eingebaut"),
        Herkunft::EingebautStattPin => ("eingebaut", "eingebaut_statt_pin"),
    }
}

/// Das Manifest ohne `files` (die Dateiliste braucht keine Anzeige), mit
/// allen Feldern — auch denen, die dieses Programm nicht kennt
/// (`Manifest::weitere`).
fn manifest_ohne_dateien(m: &Manifest) -> serde_json::Value {
    let mut v = serde_json::to_value(m).unwrap_or_default();
    if let Some(o) = v.as_object_mut() {
        o.remove("files");
    }
    v
}

/// Der Stand, der beim naechsten Start gaelte, wenn es nicht der geltende ist.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
struct Naechster {
    id: String,
    generation: u64,
    bewertung_fassung: u32,
}

/// Inhalt von [`paket_info`] (rein, testbar).
fn info_json(
    s: &AktiverStand,
    herkunft: Herkunft,
    pin: Option<(&RuntimeBundleId, bool)>,
    naechster: Option<&Naechster>,
    letzte: Option<&Pruefung>,
) -> serde_json::Value {
    let (art, genau) = herkunft_name(herkunft);
    serde_json::json!({
        "herkunft": art,
        "herkunft_genau": genau,
        "id": s.id.to_string(),
        "generation": s.id.generation,
        "manifest_sha256": s.id.manifest_sha256,
        "bewertung_fassung": s.manifest.score_version,
        "input_schema": s.manifest.input_schema,
        "kabine_fassung": s.manifest.weitere.get("kabine_fassung"),
        "manifest": manifest_ohne_dateien(&s.manifest),
        "faehigkeiten": FAEHIGKEITEN,
        "client_version": env!("CARGO_PKG_VERSION"),
        "flug_pin": pin.map(|(id, nachgeholt)| serde_json::json!({
            "id": id.to_string(), "generation": id.generation, "nachgeholt": nachgeholt,
        })),
        "naechster_start": naechster,
        "letzte_pruefung": letzte,
    })
}

/// Zwischenspeicher fuer [`naechster_start`]: je Zeigerstand der Ablage
/// und Pin des offenen Flugs einmal laden (das prueft alle Hashes).
type NaechsterSchluessel = (Option<u64>, Option<u64>, Option<RuntimeBundleId>);
static NAECHSTER: std::sync::Mutex<Option<(NaechsterSchluessel, Option<Naechster>)>> = std::sync::Mutex::new(None);

/// Was der naechste Start laden wird — derselbe Weg wie beim Start
/// (`Ablage::laden` mit dem Pin des offenen Flugs). Nur ein Paket zaehlt,
/// das NICHT der geltende Stand ist; ein unpassendes `aktuell` (Thomas
/// 30.10.: Generation 47 ohne `ui-befehle-2`) zaehlt nicht.
fn naechster_start(app_daten: &Path, geltend: &RuntimeBundleId) -> Option<Naechster> {
    let ablage = Ablage::oeffnen(app_daten.join("laufzeit-paket")).ok()?;
    let pin = pin_des_offenen_flugs(app_daten).map(|(_, id)| id);
    let schluessel = (ablage.aktuell(), ablage.vorher(), pin.clone());
    let mut z = NAECHSTER.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((k, n)) = z.as_ref() {
        if *k == schluessel {
            return n.clone();
        }
    }
    let n = naechster_aus(&ablage, pin.as_ref(), &kontext(), geltend);
    *z = Some((schluessel, n.clone()));
    n
}

fn naechster_aus(ablage: &Ablage, pin: Option<&RuntimeBundleId>, k: &Kontext, geltend: &RuntimeBundleId) -> Option<Naechster> {
    let l = ablage.laden(pin, k, &eingebaut()).ok()?;
    let paket = !matches!(l.herkunft, Herkunft::Eingebaut | Herkunft::EingebautStattPin);
    (paket && &l.paket.id != geltend && NICHT_ANWENDBAR.get() != Some(&l.paket.id)).then(|| Naechster {
        id: l.paket.id.to_string(),
        generation: l.paket.id.generation,
        bewertung_fassung: l.paket.manifest.score_version,
    })
}

/// Der geltende Stand fuer die Oberflaeche (Analyse V1a, `ui-befehle-4`):
/// Herkunft, Generation, Bewertungsfassung, das Manifest ohne Dateiliste
/// (auch kuenftige Felder), Pin des offenen Flugs und — falls schon geholt —
/// der Stand, der beim naechsten Start gilt. Stufe „lesen“, keine Geheimnisse.
#[tauri::command]
pub(crate) async fn paket_info() -> serde_json::Value {
    let s = stand();
    let herkunft = HERKUNFT.get().copied().unwrap_or(Herkunft::Eingebaut);
    let pin = PIN.get().map(|(_, id)| (id, PIN_MODUL.get().is_some()));
    let naechster = match APP_DATEN.get().cloned() {
        Some(daten) => {
            let geltend = s.id.clone();
            tokio::task::spawn_blocking(move || naechster_start(&daten, &geltend)).await.ok().flatten()
        }
        None => None,
    };
    let mut v = info_json(s, herkunft, pin, naechster.as_ref(), letzte().as_ref());
    // Fuer den Knopf „Jetzt neu starten“ (nur ohne offenen Flug).
    v["flug_offen"] = APP_DATEN.get().is_some_and(|d| flug_offen(d)).into();
    v
}

/// Der geltende Stand in Klartext fuer die Diagnose an live (Feld
/// `paket_generation`, dort nur angezeigt): Generation und Bewertungsfassung
/// lesbar, dahinter die Kennung.
pub(crate) fn klartext() -> String {
    klartext_von(stand())
}

fn klartext_von(s: &AktiverStand) -> String {
    let teil = match s.id.generation {
        0 => "eingebauter Stand".to_string(),
        g => format!("Generation {g}"),
    };
    format!("{teil} · Landebewertung Fassung {} ({})", s.manifest.score_version, s.id)
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
    eingebaut_aus(ersetzt, ohne, &[])
}

/// Wie [`eingebaut`], dazu neue Dateien (Tests: neueres Paket).
#[cfg(test)]
fn eingebaut_mit_neu(neu: &[(&str, &[u8])]) -> Eingebaut {
    eingebaut_aus(&[], &[], neu)
}

fn eingebaut_aus(ersetzt: &[(&str, &[u8])], ohne: &[&str], neu: &[(&str, &[u8])]) -> Eingebaut {
    let inhalt: Vec<(&str, &[u8])> = eingebettet::DATEIEN
        .iter()
        .filter(|(p, _)| !ohne.contains(p))
        .map(|&(p, b)| (p, ersetzt.iter().find(|(e, _)| *e == p).map_or(b, |(_, n)| *n)))
        .chain(neu.iter().copied())
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
        // Die Fassung des eingebauten Moduls (eine Quelle, P-4.50).
        "score_version": landing_scoring::wasm::FASSUNG,
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
    starten_mit(app_daten, None);
}

/// Wie `starten`, mit Fortschrittsmeldung und Abbruch fuer das Startfenster.
/// Nur der erste Aufruf wirkt (auch `hilfe`); jeder spaetere kehrt zurueck,
/// sobald der erste fertig ist.
pub(crate) fn starten_mit(app_daten: &Path, hilfe: Option<StartHilfe>) {
    // Einmalig auch bei gleichzeitigen Aufrufern (Codex 29.10.).
    static EINMAL: std::sync::Once = std::sync::Once::new();
    EINMAL.call_once(|| starten_einmal(app_daten, hilfe));
}

/// Lohnt sich ein Startfenster mit Abruf? Nein bei offenem Flug (P-3.20:
/// kein Wechsel unter einem laufenden Flug), ohne Pruefschluessel und wenn
/// schon ein Stand gilt — dann startet das Hauptfenster sofort.
pub(crate) fn startabruf_noetig(app_daten: &Path) -> bool {
    !PRUEFSCHLUESSEL.is_empty() && STAND.get().is_none() && !flug_offen(app_daten)
}

fn starten_einmal(app_daten: &Path, hilfe: Option<StartHilfe>) {
    // Abweichung P-3.20 (Thomas 30.10.: „installieren, schliessen und erst
    // dann die neue Version haben — das funktioniert nicht“): Ein neuerer
    // freigegebener Stand wird VOR dem Festlegen geholt und gilt schon in
    // diesem Lauf. Nicht bei offenem Flug (sein Pin gilt weiter).
    beim_start_holen(app_daten, hilfe);
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
    let _ = APP_DATEN.set(app_daten.to_path_buf());
    let paket_id = paket.id.clone();
    let (neu, aus_paket) = match stand_aus_paket(paket) {
        Ok(neu) => (Some(neu), true),
        Err(e) => {
            tracing::warn!(stand = %paket_id, error = %e, "Laufzeit-Paket: Stand nicht anwendbar, eingebauter Stand gilt");
            let _ = NICHT_ANWENDBAR.set(paket_id.clone());
            (eingebauter_stand().ok(), false)
        }
    };
    // Etappe 5 (P-8.20): Die Oberflaeche kommt aus DERSELBEN Generation wie
    // die Daten (`ui_paket` haengt am Stand) — oder ganz aus dem Programm.
    if let Some(neu) = neu {
        if !uebernehmen(neu) {
            tracing::warn!("Laufzeit-Paket: ein Stand galt schon (vorher gelesen), er bleibt");
        }
    }
    let wirksam = stand().id.clone();
    // Herkunft dessen, was WIRKLICH gilt: Galt schon ein Stand oder war das
    // Paket nicht anwendbar, ist es der eingebaute.
    let _ = HERKUNFT.set(if aus_paket && wirksam == paket_id { herkunft } else { Herkunft::Eingebaut });
    if let Some((pirep, pin)) = pin {
        if pin != wirksam {
            let _ = PIN.set((pirep, pin));
        }
    }
    tracing::info!(stand = %wirksam, ?herkunft, "Laufzeit-Paket geladen");
}

/// Startabruf: Frist je Anfrage und insgesamt. Mit sichtbarem Fortschritt im
/// Startfenster (Thomas 30.10.) darf der Abruf laenger dauern: ein grosser
/// erster Download (Oberflaeche, mehrere MB) soll noch im selben Lauf
/// gelten, nicht erst nach „schliessen und neu starten“. Die Anfrage-Frist
/// gilt je Abschnitt (Verbindung, Lesen), nicht fuer den ganzen Blob. Reicht
/// auch die Gesamtfrist nicht, startet das Programm mit dem vorhandenen
/// Stand; der Abruf im Hintergrund holt den Rest, aktiv dann beim naechsten
/// Start bzw. per „Jetzt neu starten“.
const START_FRIST_ANFRAGE: std::time::Duration = std::time::Duration::from_secs(15);
const START_FRIST_GESAMT: std::time::Duration = std::time::Duration::from_secs(60);
/// Verbindungsaufbau: ohne Netz (kein Weg, Port zu) kommt der Fehler sofort;
/// nur ein Weg ins Leere wartet, dann hoechstens so lange.
const START_FRIST_VERBINDUNG: std::time::Duration = std::time::Duration::from_secs(6);

/// Wo der Startabruf gerade steht (fuer das Startfenster).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Fragt live nach dem aktuellen Stand.
    Pruefen,
    /// Laedt die fehlenden Dateien (`geladen` von `gesamt` Bytes).
    Laden,
    /// Prueft und installiert; ab hier ist kein Abbruch mehr moeglich.
    Installieren,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fortschritt {
    pub phase: Phase,
    /// Nummer des Stands, den live nennt (ab `Laden`).
    pub generation: Option<u64>,
    pub geladen: u64,
    /// Summe der Groessen der fehlenden Dateien laut Manifest.
    pub gesamt: u64,
}

pub(crate) type Melder = std::sync::Arc<dyn Fn(Fortschritt) + Send + Sync>;

/// Abbruch des Startabrufs („Ueberspringen“). Nach `sperren()` (Installieren
/// beginnt) wirkt er nicht mehr: eine halbe Installation lassen wir nicht
/// zurueck.
#[derive(Default)]
pub(crate) struct Abbruch {
    signal: tokio::sync::Notify,
    gesperrt: std::sync::atomic::AtomicBool,
}

impl Abbruch {
    /// `true` = der Abruf wird abgebrochen.
    pub(crate) fn ausloesen(&self) -> bool {
        if self.gesperrt.load(std::sync::atomic::Ordering::SeqCst) {
            return false;
        }
        // `notify_one` merkt sich den Ruf auch ohne wartenden Empfaenger.
        self.signal.notify_one();
        true
    }

    fn sperren(&self) {
        self.gesperrt.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    async fn abgerufen(&self) {
        self.signal.notified().await;
    }
}

/// Meldeweg und Abbruch des Startfensters.
#[derive(Clone)]
pub(crate) struct StartHilfe {
    pub melden: Melder,
    pub abbruch: std::sync::Arc<Abbruch>,
}

/// Zaehlt die geladenen Bytes ueber alle Dateien eines Abrufs zusammen und
/// meldet jeden Schritt (die Drosselung macht der Empfaenger).
struct Zaehler<'a> {
    hilfe: Option<&'a StartHilfe>,
    generation: u64,
    gesamt: u64,
    geladen: std::sync::atomic::AtomicU64,
}

impl Zaehler<'_> {
    fn dazu(&self, bytes: u64) {
        let alt = self.geladen.fetch_add(bytes, std::sync::atomic::Ordering::SeqCst);
        // Nie ueber 100 %: ein Server darf mehr schicken als das Manifest nennt.
        let geladen = (alt + bytes).min(self.gesamt);
        self.melden(Phase::Laden, geladen);
    }

    fn melden(&self, phase: Phase, geladen: u64) {
        if let Some(h) = self.hilfe {
            (h.melden)(Fortschritt { phase, generation: Some(self.generation), geladen, gesamt: self.gesamt });
        }
    }
}

/// Holt beim Start einen neueren Stand, bevor er festgelegt wird. Anonym
/// (P-7.03; der Schluesselspeicher ist hier noch nicht eingerichtet) und von
/// der eingebauten live-Adresse (die Quellen der Crates sind noch nicht
/// angemeldet, `stand()` darf hier nicht laufen). Eigener Faden mit eigener
/// Laufzeit: `setup` laeuft ausserhalb von tokio.
fn beim_start_holen(app_daten: &Path, hilfe: Option<StartHilfe>) {
    if PRUEFSCHLUESSEL.is_empty() || STAND.get().is_some() {
        return;
    }
    let Ok(basis) = paket_endpunkte::lesen(paket_endpunkte::EINGEBAUT).map(|e| e.live) else { return };
    let daten = app_daten.to_path_buf();
    let lauf = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().ok()?;
        let weg = Holweg {
            basis: &basis,
            token: None,
            frist: START_FRIST_ANFRAGE,
            je_abschnitt: true,
            kontext: kontext(),
            selbsttest,
            pins: Vec::new(),
            hilfe,
        };
        rt.block_on(vorab_holen(&daten, weg, START_FRIST_GESAMT))
    });
    let Some(ergebnis) = lauf.join().ok().flatten() else { return };
    match &ergebnis {
        Ok(Some(id)) => {
            pruefung_vermerken(app_daten, chrono::Utc::now());
            tracing::info!(stand = %id, "Laufzeit-Paket: neuer Stand beim Start geholt, gilt sofort");
        }
        Ok(None) => pruefung_vermerken(app_daten, chrono::Utc::now()),
        Err(e) => tracing::warn!(error = %e, "Laufzeit-Paket: Startabruf ohne Ergebnis, vorhandener Stand gilt"),
    }
    *LETZTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Pruefung::aus(&ergebnis));
}

/// Der Startabruf mit Gesamtfrist; `None` bei offenem Flug (P-3.20 gilt
/// fuer ihn weiter: kein Wechsel unter einem laufenden Flug). „Ueberspringen“
/// (`Abbruch`) beendet ihn vorzeitig mit einem Fehler.
async fn vorab_holen(app_daten: &Path, weg: Holweg<'_>, gesamt: std::time::Duration) -> Option<Result<Option<RuntimeBundleId>, String>> {
    if flug_offen(app_daten) {
        return None;
    }
    let abbruch = weg.hilfe.as_ref().map(|h| h.abbruch.clone());
    let abgebrochen = async {
        match &abbruch {
            Some(a) => a.abgerufen().await,
            None => std::future::pending().await,
        }
    };
    Some(tokio::select! {
        r = tokio::time::timeout(gesamt, holen_und_installieren(app_daten, weg)) => match r {
            Ok(r) => r,
            Err(_) => Err(format!("keine Antwort binnen {} s", gesamt.as_secs())),
        },
        () = abgebrochen => Err("uebersprungen".to_string()),
    })
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
    let _ = APP_DATEN.set(app_daten.clone());
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
        abrufen(&app_daten, false, true).await;
        if let Some(tage) = pruefung_ueberfaellig(&app_daten, chrono::Utc::now()) {
            hinweis(tage);
        }
    });
}

/// App-Datenordner, gesetzt beim ersten Abruf (fuer den Abruf nach der Anmeldung).
static APP_DATEN: OnceLock<PathBuf> = OnceLock::new();
/// Ein Abruf lief schon MIT Pilot-Token bis zur Antwort von live (Vorab-
/// Piloten haben dann ihren Vorabstand gesehen, P-3.13).
static MIT_TOKEN_GEPRUEFT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Nur ein Abruf zur Zeit: Start und „Token da“ koennen sich ueberlappen;
/// die Ablage-Sperre schuetzt nur das Installieren, nicht das Herunterladen.
static ABRUF: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
/// Wartezeit vor der einen Wiederholung nach einem gescheiterten Abruf
/// (Netz noch nicht da, 401 vom Grundtor) — nicht erst beim naechsten Start.
const WIEDERHOLEN_NACH: std::time::Duration = std::time::Duration::from_secs(20);

/// Ein Abruf mit hoechstens EINER Wiederholung nach `warten`, wenn der erste
/// scheitert. Vor der Wiederholung `noch_erlaubt` (kein Flug begonnen).
async fn mit_einer_wiederholung<F, Fut>(
    mut versuch: F,
    warten: std::time::Duration,
    noch_erlaubt: impl Fn() -> bool,
) -> Result<Option<RuntimeBundleId>, String>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Option<RuntimeBundleId>, String>>,
{
    match versuch().await {
        Err(e) => {
            tracing::warn!(error = %e, "Laufzeit-Paket: Aktualisierung nicht moeglich, ein weiterer Versuch folgt");
            tokio::time::sleep(warten).await;
            if !noch_erlaubt() {
                return Ok(None);
            }
            versuch().await
        }
        ok => ok,
    }
}

/// Ein HTTP-Fehler von live kurz und lesbar („HTTP 401“), sonst der Text.
fn http_fehler(e: reqwest::Error) -> String {
    e.status().map_or_else(|| e.to_string(), |s| format!("HTTP {}", s.as_u16()))
}

/// Ergebnis der letzten Pruefung bei live in diesem Lauf (fuer `paket_info`
/// und den Knopf „Auf neues Paket pruefen“).
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Pruefung {
    /// RFC 3339, UTC.
    pub zeit: String,
    /// `neu` (Stand geholt, aktiv ab naechstem Start), `kein_neueres`,
    /// `fehler`, `flug_offen` (im Flug wird nicht geholt).
    pub ergebnis: &'static str,
    pub generation: Option<u64>,
    pub fehler: Option<String>,
}

impl Pruefung {
    fn aus(e: &Result<Option<RuntimeBundleId>, String>) -> Self {
        let (ergebnis, generation, fehler) = match e {
            Ok(Some(id)) => ("neu", Some(id.generation), None),
            Ok(None) => ("kein_neueres", None, None),
            Err(f) => ("fehler", None, Some(f.clone())),
        };
        Self { zeit: chrono::Utc::now().to_rfc3339(), ergebnis, generation, fehler }
    }
}

static LETZTE: std::sync::Mutex<Option<Pruefung>> = std::sync::Mutex::new(None);

fn letzte() -> Option<Pruefung> {
    LETZTE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Ein Abruf bei live (seriell), mit einer Wiederholung (`wiederholen`);
/// vermerkt die erfolgreiche Pruefung (P-3.04) und das Ergebnis.
/// `nur_ohne_token`: entfaellt, wenn inzwischen (unter der Sperre geprueft)
/// ein Abruf mit Token durchging.
async fn abrufen(app_daten: &Path, nur_ohne_token: bool, wiederholen: bool) -> Option<Pruefung> {
    let _sperre = ABRUF.lock().await;
    if nur_ohne_token && MIT_TOKEN_GEPRUEFT.load(std::sync::atomic::Ordering::SeqCst) {
        return None;
    }
    let warten = if wiederholen { WIEDERHOLEN_NACH } else { std::time::Duration::ZERO };
    let ergebnis = if wiederholen {
        mit_einer_wiederholung(|| aktualisieren(app_daten), warten, || !flug_offen(app_daten)).await
    } else {
        aktualisieren(app_daten).await
    };
    match &ergebnis {
        Ok(Some(id)) => {
            pruefung_vermerken(app_daten, chrono::Utc::now());
            tracing::info!(stand = %id, "Laufzeit-Paket: neuer Stand, aktiv ab naechstem Start");
        }
        Ok(None) => pruefung_vermerken(app_daten, chrono::Utc::now()),
        Err(e) => tracing::warn!(error = %e, "Laufzeit-Paket: Aktualisierung nicht moeglich"),
    }
    let p = Pruefung::aus(&ergebnis);
    *LETZTE.lock().unwrap_or_else(|e| e.into_inner()) = Some(p.clone());
    Some(p)
}

/// Knopf „Auf neues Paket pruefen“: sofort bei live fragen (ohne
/// Wiederholung, der Pilot sieht das Ergebnis). Stufe „fachlich“: Netz nur
/// zu live (Adresse aus dem Paket, Host fest), das Token setzt der Kern.
/// Im Flug wird nicht geholt (P-3.20). Hoechstens alle 5 s (sonst die
/// letzte Antwort).
#[tauri::command]
pub(crate) async fn paket_jetzt_pruefen() -> Pruefung {
    let jetzt = || chrono::Utc::now().to_rfc3339();
    let Some(daten) = APP_DATEN.get().cloned() else {
        return Pruefung { zeit: jetzt(), ergebnis: "fehler", generation: None, fehler: Some("Paket noch nicht gestartet".into()) };
    };
    if flug_offen(&daten) {
        return Pruefung { zeit: jetzt(), ergebnis: "flug_offen", generation: None, fehler: None };
    }
    if let Some(l) = letzte().filter(|l| {
        chrono::DateTime::parse_from_rfc3339(&l.zeit).is_ok_and(|t| chrono::Utc::now() - t.with_timezone(&chrono::Utc) < chrono::Duration::seconds(5))
    }) {
        return l;
    }
    match abrufen(&daten, false, false).await {
        Some(p) => p,
        None => letzte().unwrap_or_else(|| Pruefung { zeit: jetzt(), ergebnis: "kein_neueres", generation: None, fehler: None }),
    }
}

/// Darf „Jetzt neu starten“? Nur ohne offenen Flug und nur, wenn wirklich
/// ein anderer, ladbarer Stand wartet — so kann auch eine fremde Oberflaeche
/// (P-8.12) keine Neustart-Schleife ausloesen: Nach dem Neustart gilt der
/// neue Stand, und es wartet keiner mehr.
fn neustart_erlaubt(flug_offen: bool, wartet: bool) -> Result<(), &'static str> {
    if flug_offen {
        return Err("flug_offen");
    }
    if !wartet {
        return Err("kein_neues_paket");
    }
    Ok(())
}

/// Knopf „Jetzt neu starten“ (neues Paket aktivieren, P-3.20). Stufe
/// „fachlich“ (30.10.: die eigene Oberflaeche ist die Bestaetigung, der Kern
/// prueft [`neustart_erlaubt`]); derselbe Neustart wie nach dem Update
/// (`huelle::update_installieren`).
#[tauri::command]
pub(crate) async fn paket_neu_starten(app: tauri::AppHandle) -> Result<(), crate::UiError> {
    let daten = APP_DATEN.get().cloned().ok_or_else(|| crate::UiError::new("kein_neues_paket", "Paket noch nicht gestartet"))?;
    let geltend = stand().id.clone();
    let offen = flug_offen(&daten);
    let wartet = tokio::task::spawn_blocking(move || naechster_start(&daten, &geltend).is_some()).await.unwrap_or(false);
    neustart_erlaubt(offen, wartet).map_err(|c| crate::UiError::new(c, "Neustart nicht moeglich"))?;
    tracing::info!("Laufzeit-Paket: Neustart fuer den neuen Stand");
    app.restart()
}

/// Soll nach dem Setzen des Pilot-Tokens noch einmal abgerufen werden? Nur
/// wenn noch kein Abruf MIT Token durchging und kein Flug offen ist.
fn nach_token_abrufen(mit_token_geprueft: bool, flug_offen: bool) -> bool {
    !mit_token_geprueft && !flug_offen
}

/// Das Pilot-Token ist jetzt da (`live_zugang::token_setzen`). Der Start-
/// abruf lief womoeglich noch ohne (Thomas' Protokoll 30.10.: Abfrage
/// ~0,7 s vor den Zugangsdaten → 401) — dann genau einmal mit Token.
pub(crate) fn token_bereit() {
    if PRUEFSCHLUESSEL.is_empty() {
        return;
    }
    let Some(daten) = APP_DATEN.get().cloned() else { return };
    if !nach_token_abrufen(MIT_TOKEN_GEPRUEFT.load(std::sync::atomic::Ordering::SeqCst), flug_offen(&daten)) {
        return;
    }
    // Im Test nur zaehlen — kein Netz.
    #[cfg(test)]
    {
        NACH_TOKEN_ANGESTOSSEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        drop(daten);
    }
    #[cfg(not(test))]
    tauri::async_runtime::spawn(async move {
        abrufen(&daten, true, true).await;
    });
}

#[cfg(test)]
static NACH_TOKEN_ANGESTOSSEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[derive(serde::Deserialize)]
struct AktuellAntwort {
    generation: u64,
    manifest: String,
    signatur: String,
    blobs: String,
}

/// Woher und wie ein Stand geholt wird: im laufenden Programm von der
/// Adresse des Stands mit Pilot-Token, beim Start (vor dem Festlegen des
/// Stands) von der eingebauten Adresse, anonym und mit kurzer Frist.
struct Holweg<'a> {
    basis: &'a str,
    token: Option<String>,
    frist: std::time::Duration,
    /// `true`: `frist` gilt je Abschnitt (Verbindung/Lesen), nicht fuer die
    /// ganze Anfrage — ein grosser Blob darf laenger laden (Startabruf).
    je_abschnitt: bool,
    kontext: Kontext,
    selbsttest: fn(&Paket) -> Result<(), String>,
    /// Staende, die das Aufraeumen behalten muss.
    pins: Vec<u64>,
    /// Startfenster: Fortschritt und Abbruch.
    hilfe: Option<StartHilfe>,
}

async fn aktualisieren(app_daten: &Path) -> Result<Option<RuntimeBundleId>, String> {
    let pins: Vec<u64> = [Some(stand().id.generation), PIN.get().map(|(_, p)| p.generation)].into_iter().flatten().collect();
    // Mit Pilot-Token: Vorab-Piloten bekommen den neueren Vorabstand (P-3.13).
    let weg = Holweg {
        basis: crate::live_zugang::live_basis(),
        token: crate::live_zugang::pilot_token(),
        frist: std::time::Duration::from_secs(60),
        je_abschnitt: false,
        kontext: kontext(),
        selbsttest,
        pins,
        hilfe: None,
    };
    holen_und_installieren(app_daten, weg).await
}

/// Liest die Antwort stueckweise: jedes Stueck zaehlt in den Fortschritt.
/// Mit Token: Vorabstaende gibt es auch ueber die Nummer nur fuer
/// Vorab-Piloten (P-3.13).
async fn stueckweise_holen(
    client: &reqwest::Client,
    adresse: String,
    token: Option<&str>,
    zaehler: Option<&Zaehler<'_>>,
) -> Result<Vec<u8>, String> {
    let mut anfrage = client.get(adresse);
    if let Some(token) = token {
        anfrage = anfrage.bearer_auth(token);
    }
    let mut r = anfrage.send().await.map_err(|e| e.to_string())?.error_for_status().map_err(http_fehler)?;
    let mut bytes = Vec::new();
    while let Some(stueck) = r.chunk().await.map_err(|e| e.to_string())? {
        if let Some(z) = zaehler {
            z.dazu(stueck.len() as u64);
        }
        bytes.extend_from_slice(&stueck);
    }
    Ok(bytes)
}

async fn holen_und_installieren(app_daten: &Path, weg: Holweg<'_>) -> Result<Option<RuntimeBundleId>, String> {
    let Holweg { basis, token: mit_token, frist, je_abschnitt, kontext: k, selbsttest: test, pins, hilfe } = weg;
    let mut bauer = reqwest::Client::builder();
    bauer = if je_abschnitt {
        bauer.connect_timeout(frist.min(START_FRIST_VERBINDUNG)).read_timeout(frist)
    } else {
        bauer.timeout(frist)
    };
    let client = bauer.build().map_err(|e| e.to_string())?;
    if let Some(h) = &hilfe {
        (h.melden)(Fortschritt { phase: Phase::Pruefen, generation: None, geladen: 0, gesamt: 0 });
    }
    let mut anfrage = client.get(format!("{basis}/api/bundle/current"));
    if let Some(token) = &mit_token {
        anfrage = anfrage.bearer_auth(token);
    }
    let antwort = anfrage.send().await.map_err(|e| e.to_string())?;
    if antwort.status() == reqwest::StatusCode::NOT_FOUND {
        if mit_token.is_some() {
            MIT_TOKEN_GEPRUEFT.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        return Ok(None);
    }
    let a: AktuellAntwort =
        antwort.error_for_status().map_err(http_fehler)?.json().await.map_err(|e| e.to_string())?;
    // live hat geantwortet: Mit Token war das der Stand fuer DIESEN Piloten.
    if mit_token.is_some() {
        MIT_TOKEN_GEPRUEFT.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    let ablage = Ablage::oeffnen(app_daten.join("laufzeit-paket")).map_err(|e| e.to_string())?;
    if ablage.aktuell().is_some_and(|g| g >= a.generation) {
        return Ok(None);
    }
    let token = mit_token.as_deref();
    let manifest = stueckweise_holen(&client, format!("{basis}{}", a.manifest), token, None).await?;
    let signatur = String::from_utf8(stueckweise_holen(&client, format!("{basis}{}", a.signatur), token, None).await?)
        .map_err(|e| e.to_string())?;
    let m = Manifest::lesen(&manifest).map_err(|e| e.to_string())?;
    let fehlend = ablage.fehlende(&m);
    let zaehler = Zaehler {
        hilfe: hilfe.as_ref(),
        generation: a.generation,
        gesamt: fehlend.iter().map(|d| d.size).sum(),
        geladen: std::sync::atomic::AtomicU64::new(0),
    };
    zaehler.melden(Phase::Laden, 0);
    let mut geholt = HashMap::new();
    for d in fehlend {
        let adresse = format!("{basis}{}{}", a.blobs, d.sha256);
        geholt.insert(d.sha256.clone(), stueckweise_holen(&client, adresse, token, Some(&zaehler)).await?);
    }
    // Ab hier ist der Abruf nicht mehr abbrechbar (siehe `Abbruch`).
    if let Some(h) = &hilfe {
        h.abbruch.sperren();
    }
    zaehler.melden(Phase::Installieren, zaehler.gesamt);
    let daten = app_daten.to_path_buf();
    tokio::task::spawn_blocking(move || {
        // Erneut unmittelbar davor: Ein Flug kann waehrend des Ladens begonnen haben.
        if flug_offen(&daten) {
            return Ok(None);
        }
        let id = ablage
            .installieren(
                Eingang { manifest: &manifest, signatur: &signatur },
                &k,
                &mut |d| geholt.get(&d.sha256).cloned().ok_or_else(|| "nicht geholt".to_string()),
                &test,
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
            r.error_for_status().map_err(http_fehler)?.bytes().await.map_err(|e| e.to_string())
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
        assert!(liste.split(',').any(|f| f == "ui-befehle-4"), "Paket mit ui/ ohne ui-befehle-4 (paket_info)");
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
        let aus = |ersetzt: &[(&str, &[u8])]| stand_aus_paket(eingebaut_mit(ersetzt).paket().unwrap()).unwrap();
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

    /// `ui-befehle-4` (V1b): jede Datendatei unter tabellen/, texte/,
    /// grenzwerte/ des geltenden Stands ist lesbar — auch eine, die das
    /// Programm nicht kennt. Gegenproben: Module, Selbsttest, Endpunkte,
    /// Profile, OurAirports, `..`, Unterordner, Nicht-JSON bleiben gesperrt.
    #[test]
    fn paket_datei_liest_alle_datendateien_und_sonst_nichts() {
        assert_eq!(paket_datei(FLOTTE.into()).unwrap().as_bytes(), eingebaute_datei(FLOTTE).unwrap());
        for sprache in ["de", "en", "it"] {
            let text = paket_datei(format!("texte/{sprache}.json")).unwrap();
            assert!(serde_json::from_str::<serde_json::Value>(&text).unwrap().is_object());
        }
        assert_eq!(paket_datei(HILFE_FORMULAR.into()).unwrap().as_bytes(), eingebaute_datei(HILFE_FORMULAR).unwrap());
        // Frueher gesperrt, jetzt offen: jede Tabelle und jeder Grenzwert.
        assert_eq!(paket_datei(CPDLC.into()).unwrap().as_bytes(), eingebaute_datei(CPDLC).unwrap());
        assert!(paket_datei("grenzwerte/landung.json".into()).is_ok());
        for gesperrt in [
            BEWERTUNG, KABINE_MODUL, SELBSTTEST, ENDPUNKTE, XPLANE_PROFILE, MSFS_PROFILE, RUNWAYS, AIRPORTS,
            "../geheim", "tabellen/../endpunkte.json", "tabellen/..json", "tabellen/.json", "tabellen/x/y.json",
            "texte/de.json/", "/texte/de.json", "tabellen\\flotte.json", "ui/index.html", "tabellen/fehlt.json",
        ] {
            assert!(paket_datei(gesperrt.into()).is_err(), "{gesperrt}");
        }
    }

    /// Eine neue Tabelle im Paket ist ohne Programmaenderung lesbar — aus
    /// dem Paket, mit Hashpruefung; eine zu grosse wird abgewiesen.
    #[test]
    fn neue_tabelle_im_paket_ist_lesbar() {
        let neu = br#"{"format":1,"zeilen":[1,2,3]}"#;
        let p = eingebaut_mit_neu(&[("tabellen/neu.json", neu)]).paket().unwrap();
        let s = stand_aus_paket(p).unwrap();
        assert_eq!(datei_lesen(&s, "tabellen/neu.json").unwrap().as_bytes(), neu);
        // Gegenprobe: der eingebaute Stand kennt sie nicht.
        assert!(paket_datei("tabellen/neu.json".into()).is_err());
        let gross = vec![b' '; LESBAR_MAX_BYTES as usize + 1];
        let s = stand_aus_paket(eingebaut_mit_neu(&[("tabellen/gross.json", &gross)]).paket().unwrap()).unwrap();
        assert_eq!(datei_lesen(&s, "tabellen/gross.json").unwrap_err(), "tabellen/gross.json: zu gross");
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
        assert_eq!(stand_aus_paket(p).unwrap().hilfe_formular.namen, ["flugnummer"]);
        let boese = br#"{"format":1,"felder":[{"name":"api_key","typ":"text"}]}"#;
        let fehler = selbsttest(&eingebaut_mit(&[(HILFE_FORMULAR, boese)]).paket().unwrap()).unwrap_err();
        assert!(fehler.contains("hilfe-formular.json"), "{fehler}");
        // Pfad aus dem Paket: anderer Pfad geht, fremder Host nicht.
        let mut e: serde_json::Value = serde_json::from_slice(eingebaute_datei(ENDPUNKTE).unwrap()).unwrap();
        e["helpdesk_pfad"] = "/api/helpdesk/v2/tickets".into();
        let p = eingebaut_mit(&[(ENDPUNKTE, e.to_string().as_bytes())]).paket().unwrap();
        assert_eq!(stand_aus_paket(p).unwrap().endpunkte.helpdesk_pfad(), "/api/helpdesk/v2/tickets");
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
        let stand = stand_aus_paket(alt).unwrap();
        assert!(stand.hilfe_formular.namen.is_empty());
        assert_eq!(stand.endpunkte.helpdesk_pfad(), "/api/helpdesk/tickets");
        assert!(stand.manifest.datei(HILFE_FORMULAR).is_none());
        assert!(stand.manifest.datei(FLOTTE).is_some());
        let ohne_klappen = eingebaut_mit_ohne(&[], &[KLAPPEN]).paket().unwrap();
        assert!(selbsttest(&ohne_klappen).is_err());
        assert!(stand_aus_paket(ohne_klappen).is_err());
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

    /// V1a: `paket_info` im eingebauten Stand nennt die Fassung des
    /// eingebauten Moduls (frueher stand im Manifest 26, das Modul rechnet 27).
    #[test]
    fn paket_info_eingebaut_nennt_die_modulfassung() {
        let v = info_json(stand(), Herkunft::Eingebaut, None, None, None);
        assert_eq!(v["herkunft"], "eingebaut");
        assert_eq!(v["generation"], 0);
        assert_eq!(v["bewertung_fassung"], landing_scoring::wasm::FASSUNG);
        assert_eq!(v["manifest"]["score_version"], landing_scoring::wasm::FASSUNG);
        assert!(v["manifest"].get("files").is_none(), "Dateiliste bleibt draussen");
        assert!(v["kabine_fassung"].is_null());
        assert!(v["naechster_start"].is_null() && v["flug_pin"].is_null());
        assert!(v["faehigkeiten"].as_array().unwrap().iter().any(|f| f == "ui-befehle-4"));
    }

    /// Abweichung P-3.20 (Thomas 30.10.): Beim Start holt der Client einen
    /// neueren Stand und dieser gilt sofort — derselbe Ladeweg wie
    /// `starten_einmal` (`Ablage::laden`) findet ihn. Gegenproben: offener
    /// Flug → kein Abruf, Stand bleibt; Server nicht erreichbar → Fehler,
    /// vorhandener (hier eingebauter) Stand gilt.
    #[tokio::test(flavor = "multi_thread")]
    async fn startabruf_nutzt_neuen_stand_sofort_ausser_im_flug() {
        use std::io::Cursor;
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let k = || Kontext {
            client_version: "3.0.0".into(),
            faehigkeiten: vec!["probe-1".into()],
            schluessel: vec![kp.pk.to_base64()],
            jetzt: chrono::Utc::now(),
        };
        let datei = br#"{"probe":1}"#.to_vec();
        let sha = runtime_bundle::hex(&Sha256::digest(&datei));
        let m = serde_json::to_vec(&serde_json::json!({
            "bundle_schema": 1, "generation": 54, "created_at": chrono::Utc::now(),
            "expires_at": chrono::Utc::now() + chrono::Duration::days(30),
            "client_min": "3.0.0", "client_max": "3.99.0", "requires": ["probe-1"],
            "score_version": 27, "input_schema": 1,
            "files": [{"path": "tabellen/probe.json", "sha256": sha, "size": datei.len(), "kind": "tabellen"}],
        }))
        .unwrap();
        let sig = minisign::sign(Some(&kp.pk), &kp.sk, Cursor::new(&m), None, None).unwrap().into_string();
        let mut inhalt: HashMap<String, Vec<u8>> = HashMap::new();
        inhalt.insert("/api/bundle/current".into(), br#"{"generation":54,"manifest":"/m","signatur":"/s","blobs":"/b/"}"#.to_vec());
        inhalt.insert("/m".into(), m);
        inhalt.insert("/s".into(), sig.into_bytes());
        inhalt.insert(format!("/b/{sha}"), datei);
        let inhalt = std::sync::Arc::new(inhalt);
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri| {
            let inhalt = inhalt.clone();
            async move {
                match inhalt.get(uri.path()) {
                    Some(b) => (axum::http::StatusCode::OK, b.clone()),
                    None => (axum::http::StatusCode::NOT_FOUND, Vec::new()),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let basis = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let ok: fn(&Paket) -> Result<(), String> = |_| Ok(());
        let weg = |basis: &str| Holweg { basis: Box::leak(basis.to_string().into_boxed_str()), token: None, frist: START_FRIST_ANFRAGE, je_abschnitt: true, kontext: k(), selbsttest: ok, pins: Vec::new(), hilfe: None };
        let ordner = |n: &str| {
            let d = std::env::temp_dir().join(format!("aeroacars-startabruf-{n}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        let geladen = |d: &Path| {
            let l = Ablage::oeffnen(d.join("laufzeit-paket")).unwrap().laden(None, &k(), &eingebaut()).unwrap();
            (l.herkunft, l.paket.id.generation)
        };
        // Frische Ablage: der erste Start nutzt die geholte Generation.
        let frisch = ordner("frisch");
        let r = vorab_holen(&frisch, weg(&basis), START_FRIST_GESAMT).await.expect("kein Flug offen");
        assert_eq!(r.unwrap().map(|id| id.generation), Some(54));
        assert_eq!(geladen(&frisch), (Herkunft::Aktuell, 54));
        // Gegenprobe: offener Flug → nichts geholt, es bleibt beim Stand.
        let flug = ordner("flug");
        std::fs::write(flug.join(crate::ACTIVE_FLIGHT_FILE), "{}").unwrap();
        assert!(vorab_holen(&flug, weg(&basis), START_FRIST_GESAMT).await.is_none());
        assert_eq!(geladen(&flug), (Herkunft::Eingebaut, 0));
        // Gegenprobe: Server nicht erreichbar → Fehler, eingebauter Stand.
        let offline = ordner("offline");
        let r = vorab_holen(&offline, weg("http://127.0.0.1:9"), START_FRIST_GESAMT).await.expect("kein Flug offen");
        assert!(r.is_err(), "{r:?}");
        assert_eq!(geladen(&offline), (Herkunft::Eingebaut, 0));
        for d in [frisch, flug, offline] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    /// Startfenster (Thomas 30.10.): Der Abruf meldet seinen Fortschritt —
    /// die Bytes ueber alle Dateien summieren sich genau auf die Groesse laut
    /// Manifest, nie darueber, nie rueckwaerts. Gegenproben: offener Flug →
    /// kein Abruf und keine Meldung; kein Netz → sofort Fehler (nicht erst
    /// nach der Frist); Ueberspringen und Gesamtfrist beenden einen
    /// haengenden Abruf; nach Beginn der Installation wirkt der Abbruch nicht.
    #[tokio::test(flavor = "multi_thread")]
    async fn startabruf_meldet_fortschritt_und_bricht_ab() {
        use std::io::Cursor;
        use std::sync::{Arc, Mutex};
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let k = || Kontext {
            client_version: "3.0.0".into(),
            faehigkeiten: vec!["probe-1".into()],
            schluessel: vec![kp.pk.to_base64()],
            jetzt: chrono::Utc::now(),
        };
        // Zwei Dateien, gross genug fuer mehrere Stuecke je Datei.
        let gross = vec![7u8; 700_000];
        let klein = vec![9u8; 300_000];
        let eintrag = |pfad: &str, b: &[u8]| {
            serde_json::json!({"path": pfad, "sha256": runtime_bundle::hex(&Sha256::digest(b)), "size": b.len(), "kind": "tabellen"})
        };
        let m = serde_json::to_vec(&serde_json::json!({
            "bundle_schema": 1, "generation": 55, "created_at": chrono::Utc::now(),
            "expires_at": chrono::Utc::now() + chrono::Duration::days(30),
            "client_min": "3.0.0", "client_max": "3.99.0", "requires": ["probe-1"],
            "score_version": 27, "input_schema": 1,
            "files": [eintrag("tabellen/gross.bin", &gross), eintrag("tabellen/klein.bin", &klein)],
        }))
        .unwrap();
        let sig = minisign::sign(Some(&kp.pk), &kp.sk, Cursor::new(&m), None, None).unwrap().into_string();
        let mut inhalt: HashMap<String, Vec<u8>> = HashMap::new();
        inhalt.insert("/api/bundle/current".into(), br#"{"generation":55,"manifest":"/m","signatur":"/s","blobs":"/b/"}"#.to_vec());
        inhalt.insert("/m".into(), m);
        inhalt.insert("/s".into(), sig.into_bytes());
        inhalt.insert(format!("/b/{}", runtime_bundle::hex(&Sha256::digest(&gross))), gross.clone());
        inhalt.insert(format!("/b/{}", runtime_bundle::hex(&Sha256::digest(&klein))), klein.clone());
        let inhalt = Arc::new(inhalt);
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri| {
            let inhalt = inhalt.clone();
            async move {
                match inhalt.get(uri.path()) {
                    Some(b) => (axum::http::StatusCode::OK, b.clone()),
                    None => (axum::http::StatusCode::NOT_FOUND, Vec::new()),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let basis = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        // Ein Ziel, das Verbindungen annimmt und nie antwortet.
        let stumm = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let stumm_basis = format!("http://{}", stumm.local_addr().unwrap());
        tokio::spawn(async move {
            let mut gehalten = Vec::new();
            while let Ok((s, _)) = stumm.accept().await {
                gehalten.push(s);
            }
        });
        let ok: fn(&Paket) -> Result<(), String> = |_| Ok(());
        let hilfe = || {
            let ereignisse = Arc::new(Mutex::new(Vec::<Fortschritt>::new()));
            let sammler = ereignisse.clone();
            let melden: Melder = Arc::new(move |f| sammler.lock().unwrap().push(f));
            (StartHilfe { melden, abbruch: Arc::new(Abbruch::default()) }, ereignisse)
        };
        let weg = |basis: &str, h: StartHilfe| Holweg {
            basis: Box::leak(basis.to_string().into_boxed_str()),
            token: None,
            frist: START_FRIST_ANFRAGE,
            je_abschnitt: true,
            kontext: k(),
            selbsttest: ok,
            pins: Vec::new(),
            hilfe: Some(h),
        };
        let ordner = |n: &str| {
            let d = std::env::temp_dir().join(format!("aeroacars-startmeldung-{n}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            d
        };
        // 1. Erfolg: Summe der Bytes = Manifest, monoton, Phasen in Reihenfolge.
        let d1 = ordner("ok");
        let (h, ereignisse) = hilfe();
        let abbruch = h.abbruch.clone();
        let r = vorab_holen(&d1, weg(&basis, h), START_FRIST_GESAMT).await.expect("kein Flug offen");
        assert_eq!(r.unwrap().map(|id| id.generation), Some(55));
        let e = ereignisse.lock().unwrap().clone();
        assert_eq!(e.first().map(|f| f.phase), Some(Phase::Pruefen));
        assert_eq!(e.last().map(|f| f.phase), Some(Phase::Installieren));
        let laden: Vec<_> = e.iter().filter(|f| f.phase == Phase::Laden).collect();
        assert!(laden.len() > 3, "mehrere Stuecke erwartet: {}", laden.len());
        assert!(laden.iter().all(|f| f.gesamt == 1_000_000 && f.generation == Some(55)));
        assert!(laden.windows(2).all(|w| w[0].geladen <= w[1].geladen), "Fortschritt geht nie zurueck");
        assert_eq!(laden.first().unwrap().geladen, 0);
        assert_eq!(laden.last().unwrap().geladen, 1_000_000, "Summe der Bytes beider Dateien");
        assert_eq!(e.last().unwrap().geladen, 1_000_000);
        assert!(!abbruch.ausloesen(), "nach Beginn der Installation kein Abbruch mehr");
        // 2. Offener Flug: kein Abruf, keine Meldung.
        let d2 = ordner("flug");
        std::fs::write(d2.join(crate::ACTIVE_FLIGHT_FILE), "{}").unwrap();
        let (h, ereignisse) = hilfe();
        assert!(vorab_holen(&d2, weg(&basis, h), START_FRIST_GESAMT).await.is_none());
        assert!(ereignisse.lock().unwrap().is_empty());
        // 3. Kein Netz: Fehler sofort, weit vor der Frist.
        let d3 = ordner("offline");
        let (h, _) = hilfe();
        let t0 = std::time::Instant::now();
        let r = vorab_holen(&d3, weg("http://127.0.0.1:9", h), START_FRIST_GESAMT).await.expect("kein Flug offen");
        assert!(r.is_err());
        assert!(t0.elapsed() < std::time::Duration::from_secs(5), "ohne Netz nicht auf die Frist warten: {:?}", t0.elapsed());
        // 4. Ueberspringen beendet einen haengenden Abruf.
        let d4 = ordner("ueberspringen");
        let (h, _) = hilfe();
        assert!(h.abbruch.ausloesen(), "vor der Installation wirkt der Abbruch");
        let t0 = std::time::Instant::now();
        let r = vorab_holen(&d4, weg(&stumm_basis, h), START_FRIST_GESAMT).await.expect("kein Flug offen");
        assert_eq!(r, Err("uebersprungen".to_string()));
        assert!(t0.elapsed() < std::time::Duration::from_secs(5));
        // 5. Gesamtfrist beendet einen haengenden Abruf.
        let d5 = ordner("frist");
        let (h, _) = hilfe();
        let r = vorab_holen(&d5, weg(&stumm_basis, h), std::time::Duration::from_millis(300)).await.expect("kein Flug offen");
        assert!(r.unwrap_err().starts_with("keine Antwort binnen"));
        for d in [d1, d2, d3, d4, d5] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn startfristen_wie_vereinbart() {
        assert_eq!(START_FRIST_GESAMT.as_secs(), 60);
        assert_eq!(START_FRIST_ANFRAGE.as_secs(), 15);
        assert!(START_FRIST_VERBINDUNG < START_FRIST_ANFRAGE);
    }

    /// „Jetzt neu starten“ nur ohne Flug und nur mit wartendem Stand
    /// (keine Neustart-Schleife durch eine fremde Oberflaeche).
    #[test]
    fn neustart_nur_ohne_flug_und_mit_wartendem_paket() {
        assert_eq!(neustart_erlaubt(false, true), Ok(()));
        assert_eq!(neustart_erlaubt(true, true), Err("flug_offen"));
        assert_eq!(neustart_erlaubt(false, false), Err("kein_neues_paket"));
    }

    /// Ergebnis fuer den Knopf „Auf neues Paket pruefen“.
    #[test]
    fn pruefung_nennt_ergebnis_generation_und_fehler() {
        let id = RuntimeBundleId { generation: 54, manifest_sha256: "ab".repeat(32) };
        let p = Pruefung::aus(&Ok(Some(id)));
        assert_eq!((p.ergebnis, p.generation, p.fehler), ("neu", Some(54), None));
        assert_eq!(Pruefung::aus(&Ok(None)).ergebnis, "kein_neueres");
        let f = Pruefung::aus(&Err("HTTP 401".into()));
        assert_eq!((f.ergebnis, f.fehler.as_deref()), ("fehler", Some("HTTP 401")));
    }

    /// Die Diagnose nennt Generation und Fassung lesbar (Thomas 30.10.).
    #[test]
    fn klartext_fuer_die_diagnose() {
        let k = klartext_von(stand());
        assert!(k.starts_with(&format!("eingebauter Stand · Landebewertung Fassung {} (0-", landing_scoring::wasm::FASSUNG)), "{k}");
        let mut s = stand_aus_paket(eingebaut().paket().unwrap()).unwrap();
        s.id = RuntimeBundleId { generation: 53, manifest_sha256: "ab".repeat(32) };
        assert_eq!(klartext_von(&s), format!("Generation 53 · Landebewertung Fassung {} (53-abababababab)", landing_scoring::wasm::FASSUNG));
        assert!(klartext_von(&s).chars().count() <= 120, "META_FELD_MAX");
    }

    /// Gegenprobe Paket: Herkunft, Generation und Fassung kommen aus DIESEM
    /// Manifest, ein kuenftiges Feld (`kabine_fassung`, `notiz`) reicht der
    /// Kern unveraendert durch; Pin und naechster Start erscheinen.
    #[test]
    fn paket_info_reicht_das_manifest_durch() {
        let mut s = stand_aus_paket(eingebaut().paket().unwrap()).unwrap();
        s.id = RuntimeBundleId { generation: 53, manifest_sha256: "ab".repeat(32) };
        s.manifest.score_version = 28;
        s.manifest.weitere.insert("kabine_fassung".into(), 4.into());
        s.manifest.weitere.insert("notiz".into(), "Probe".into());
        let pin = RuntimeBundleId { generation: 52, manifest_sha256: "cd".repeat(32) };
        let n = Naechster { id: "54-efefefefefef".into(), generation: 54, bewertung_fassung: 28 };
        let v = info_json(&s, Herkunft::Aktuell, Some((&pin, false)), Some(&n), Some(&Pruefung { zeit: "t".into(), ergebnis: "fehler", generation: None, fehler: Some("HTTP 401".into()) }));
        assert_eq!(v["herkunft"], "paket");
        assert_eq!(v["id"], "53-abababababab");
        assert_eq!(v["bewertung_fassung"], 28);
        assert_eq!(v["kabine_fassung"], 4);
        assert_eq!(v["manifest"]["notiz"], "Probe");
        assert_eq!(v["flug_pin"]["generation"], 52);
        assert_eq!(v["naechster_start"]["generation"], 54);
        assert_eq!(v["letzte_pruefung"]["fehler"], "HTTP 401");
        assert_eq!(info_json(&s, Herkunft::Pin, None, None, None)["herkunft"], "pin");
        assert_eq!(info_json(&s, Herkunft::Vorher, None, None, None)["herkunft_genau"], "vorher");
    }

    /// „Neues Paket wird beim naechsten Start aktiv“: derselbe Ladeweg wie
    /// beim Start. Ein geholtes, passendes Paket zaehlt; Gegenproben: leere
    /// Ablage, das Paket gilt schon, und ein unpassendes `aktuell` (Thomas
    /// 30.10.: 47 ohne Faehigkeit → eingebaut) zaehlt nicht.
    #[test]
    fn naechster_start_nur_fuer_ein_ladbares_anderes_paket() {
        use std::io::Cursor;
        let dir = std::env::temp_dir().join(format!("aeroacars-naechster-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ablage = Ablage::oeffnen(&dir).unwrap();
        let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let k = |f: &[&str]| Kontext {
            client_version: "3.0.0".into(),
            faehigkeiten: f.iter().map(|s| s.to_string()).collect(),
            schluessel: vec![kp.pk.to_base64()],
            jetzt: chrono::Utc::now(),
        };
        let eingebaut_id = eingebaut().paket().unwrap().id;
        assert_eq!(naechster_aus(&ablage, None, &k(&[]), &eingebaut_id), None, "leere Ablage");
        let datei = b"{}";
        let m = serde_json::json!({
            "bundle_schema": 1, "generation": 54, "created_at": chrono::Utc::now(),
            "expires_at": chrono::Utc::now() + chrono::Duration::days(30),
            "client_min": "3.0.0", "client_max": "3.99.0", "requires": ["probe-1"],
            "score_version": 27, "input_schema": 1,
            "files": [{"path": "tabellen/x.json", "sha256": runtime_bundle::hex(&Sha256::digest(datei)), "size": 2, "kind": "tabellen"}],
        });
        let bytes = serde_json::to_vec(&m).unwrap();
        let sig = minisign::sign(Some(&kp.pk), &kp.sk, Cursor::new(&bytes), None, None).unwrap().into_string();
        let id = ablage
            .installieren(Eingang { manifest: &bytes, signatur: &sig }, &k(&["probe-1"]), &mut |_| Ok(datei.to_vec()), &|_| Ok(()))
            .unwrap();
        let n = naechster_aus(&ablage, None, &k(&["probe-1"]), &eingebaut_id).expect("geholtes Paket");
        assert_eq!((n.generation, n.bewertung_fassung, n.id.clone()), (54, 27, id.to_string()));
        assert_eq!(naechster_aus(&ablage, None, &k(&["probe-1"]), &id), None, "gilt schon");
        assert_eq!(naechster_aus(&ablage, None, &k(&[]), &eingebaut_id), None, "unpassend: eingebaut");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Zusatz 30.10. (Thomas' Protokoll: Startabruf ~0,7 s vor dem Token,
    /// 401): Ein gescheiterter Abruf wird genau EINMAL wiederholt, nicht erst
    /// beim naechsten Start. Gegenproben: Erfolg → kein zweiter Versuch; zwei
    /// Fehler → Schluss nach dem zweiten; Flug begonnen → keine Wiederholung.
    #[tokio::test]
    async fn gescheiterter_abruf_wird_einmal_wiederholt() {
        use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
        let lauf = |antworten: Vec<Result<Option<RuntimeBundleId>, String>>, erlaubt: bool| async move {
            let n = AtomicUsize::new(0);
            let a = std::sync::Mutex::new(antworten.into_iter());
            let erg = mit_einer_wiederholung(
                || {
                    n.fetch_add(1, SeqCst);
                    let x = a.lock().unwrap().next().expect("zu viele Versuche");
                    async move { x }
                },
                std::time::Duration::ZERO,
                || erlaubt,
            )
            .await;
            (n.load(SeqCst), erg)
        };
        let id = RuntimeBundleId { generation: 54, manifest_sha256: "cd".repeat(32) };
        assert_eq!(lauf(vec![Err("401".into()), Ok(Some(id.clone()))], true).await, (2, Ok(Some(id))));
        assert_eq!(lauf(vec![Ok(None)], true).await, (1, Ok(None)), "Gegenprobe: Erfolg, kein zweiter Versuch");
        assert_eq!(lauf(vec![Err("a".into()), Err("b".into())], true).await, (2, Err("b".into())), "hoechstens einmal");
        assert_eq!(lauf(vec![Err("a".into())], false).await, (1, Ok(None)), "Flug begonnen: keine Wiederholung");
    }

    /// Kommt das Pilot-Token, wird genau einmal nachgefragt — solange noch
    /// kein Abruf mit Token durchging. Gegenproben: Leeren des Tokens (Abmelden)
    /// stoesst nichts an; nach einem Abruf mit Token auch kein neues Token.
    #[test]
    fn token_stoesst_den_abruf_genau_einmal_an() {
        use std::sync::atomic::Ordering::SeqCst;
        assert!(nach_token_abrufen(false, false));
        assert!(!nach_token_abrufen(true, false));
        assert!(!nach_token_abrufen(false, true), "offener Flug: kein Abruf");
        let dir = std::env::temp_dir().join(format!("aeroacars-token-abruf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = APP_DATEN.set(dir.clone());
        let vorher = NACH_TOKEN_ANGESTOSSEN.load(SeqCst);
        crate::live_zugang::token_setzen(None);
        assert_eq!(NACH_TOKEN_ANGESTOSSEN.load(SeqCst), vorher, "Abmelden stoesst nichts an");
        // `token_setzen(Some)` ruft `token_bereit` (live_zugang.rs); das
        // globale Token setzt der Test nicht — andere Tests lesen es.
        token_bereit();
        assert_eq!(NACH_TOKEN_ANGESTOSSEN.load(SeqCst), vorher + 1);
        MIT_TOKEN_GEPRUEFT.store(true, SeqCst);
        token_bereit();
        assert_eq!(NACH_TOKEN_ANGESTOSSEN.load(SeqCst), vorher + 1, "schon mit Token geprueft");
        MIT_TOKEN_GEPRUEFT.store(false, SeqCst);
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
