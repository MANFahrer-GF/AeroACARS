//! Kabinenansagen (Beta): Einstellungen, Freischaltung, Drehbuch vom
//! GSG-Server, Sprachausgabe (Azure mit eigenem Schlüssel oder Piper
//! kostenlos) und das Wiedergabe-Ereignis an die Oberfläche.
//!
//! WANN eine Ansage dran ist, entscheidet der Baustein `cabin-announcements`
//! (Planer, Warteschlange); lib.rs füttert ihn im Positions-Streamer
//! (`kabinen_schritt`). Dieses Modul macht daraus Texte und Ton.
//!
//! Sperren in dieser Reihenfolge, alle geschlossen bei Zweifel:
//! Einstellung „an“ (Standard aus) → Server sagt `beta && bereit` →
//! Flugart nicht „Aus“. Ohne alle drei holt der Client nichts und spielt
//! nichts — sonst zahlte beim nächsten Release jeder Pilot.

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use api_client::Client;
use cabin_announcements::flugart::Flugart;
use cabin_announcements::{cache, ssml, Faellig, Rolle, Slot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sim_core::{FlightPhase, SimSnapshot};
use tauri::{AppHandle, Emitter, Manager};

use crate::{ActiveFlight, AppState, FlightStats, UiError};

/// Schlüsselbund-Konto für den Azure-Schlüssel des Piloten.
const AZURE_KONTO: &str = "kabinenansagen_azure";
/// Ereignis an die Oberfläche: eine Ansage spielt jetzt.
pub(crate) const EREIGNIS_ANSAGE: &str = "kabine-ansage";
/// Ereignis an die Oberfläche: Fortschritt der Piper-Installation.
const EREIGNIS_PIPER: &str = "kabine-piper-fortschritt";

// ---------------------------------------------------------------------------
// Einstellungen
// ---------------------------------------------------------------------------

/// Was der Pilot einstellt. Liegt als JSON im Konfigurationsordner, weil
/// die Sprachausgabe in Rust läuft und die Werte dort braucht.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Einstellungen {
    /// Hauptschalter. Standard AUS.
    pub an: bool,
    /// Zweite Sprache neben Englisch: keine | airline | ziel | eigene.
    pub zweite_sprache: String,
    /// Für „eigene“: BCP-47, z. B. "de-DE".
    pub eigene_sprache: Option<String>,
    /// Anbieter je Rolle: "piper" (kostenlos) oder "azure" (eigener Schlüssel).
    pub anbieter_kapitaen: String,
    pub anbieter_kabine: String,
    pub anbieter_kommando: String,
    /// Azure-Region des Schlüssels, z. B. "westeurope".
    pub azure_region: String,
    /// Echten Namen als Kapitän verwenden (geht an den GSG-Server und das Modell).
    pub echter_name: bool,
    /// zufall | sachlich | herzlich | charmant | witzig | rustikal
    pub tagesform_kapitaen: String,
    pub tagesform_kabine: String,
    /// Doppel-Chime bei 10.000 ft. Aus, wenn das Flugzeug eigene Chimes hat.
    pub chimes: bool,
    /// Crew-Kommandos („Cabin crew, seats for take-off“). Aus bei FBW &
    /// Co. mit eigenen Ansagen — nie zwei Systeme gleichzeitig.
    pub kommandos: bool,
    /// 0.0 … 1.0
    pub lautstaerke: f32,
}

impl Default for Einstellungen {
    fn default() -> Self {
        Einstellungen {
            an: false,
            zweite_sprache: "airline".into(),
            eigene_sprache: None,
            anbieter_kapitaen: "piper".into(),
            anbieter_kabine: "piper".into(),
            anbieter_kommando: "piper".into(),
            azure_region: "westeurope".into(),
            echter_name: false,
            tagesform_kapitaen: "zufall".into(),
            tagesform_kabine: "zufall".into(),
            chimes: true,
            kommandos: true,
            lautstaerke: 0.8,
        }
    }
}

const TAGESFORMEN: [&str; 6] = ["zufall", "sachlich", "herzlich", "charmant", "witzig", "rustikal"];

impl Einstellungen {
    /// Unbekannte Werte auf den Standard, Zahlen in ihren Bereich.
    pub(crate) fn normalisiert(mut self) -> Self {
        let std = Einstellungen::default();
        if !["keine", "airline", "ziel", "eigene"].contains(&self.zweite_sprache.as_str()) {
            self.zweite_sprache = std.zweite_sprache.clone();
        }
        self.eigene_sprache = self.eigene_sprache.filter(|s| ist_locale(s));
        for a in [&mut self.anbieter_kapitaen, &mut self.anbieter_kabine, &mut self.anbieter_kommando] {
            if a != "piper" && a != "azure" {
                *a = "piper".into();
            }
        }
        let region: String = self.azure_region.trim().to_ascii_lowercase();
        self.azure_region = if !region.is_empty() && region.chars().all(|c| c.is_ascii_alphanumeric()) {
            region
        } else {
            std.azure_region
        };
        for t in [&mut self.tagesform_kapitaen, &mut self.tagesform_kabine] {
            if !TAGESFORMEN.contains(&t.as_str()) {
                *t = "zufall".into();
            }
        }
        self.lautstaerke = if self.lautstaerke.is_finite() { self.lautstaerke.clamp(0.0, 1.0) } else { 0.8 };
        self
    }

    fn anbieter(&self, rolle: Rolle) -> &str {
        match rolle {
            Rolle::Kapitaen => &self.anbieter_kapitaen,
            Rolle::Kabine => &self.anbieter_kabine,
            Rolle::Kommando | Rolle::Chime => &self.anbieter_kommando,
        }
    }
}

fn ist_locale(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5
        && b[0].is_ascii_lowercase()
        && b[1].is_ascii_lowercase()
        && b[2] == b'-'
        && b[3].is_ascii_uppercase()
        && b[4].is_ascii_uppercase()
}

static EINSTELLUNGEN: OnceLock<RwLock<Option<Einstellungen>>> = OnceLock::new();

fn einstellungen_pfad(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|p| p.join("kabinenansagen.json"))
}

pub(crate) fn einstellungen(app: &AppHandle) -> Einstellungen {
    let zelle = EINSTELLUNGEN.get_or_init(|| RwLock::new(None));
    if let Some(e) = zelle.read().expect("kabinen einstellungen").clone() {
        return e;
    }
    let geladen = einstellungen_pfad(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Einstellungen>(&t).ok())
        .unwrap_or_default()
        .normalisiert();
    *zelle.write().expect("kabinen einstellungen") = Some(geladen.clone());
    geladen
}

#[tauri::command]
pub(crate) fn kabinen_einstellungen_lesen(app: AppHandle) -> Einstellungen {
    einstellungen(&app)
}

#[tauri::command]
pub(crate) fn kabinen_einstellungen_setzen(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    einstellungen: Einstellungen,
) -> Result<Einstellungen, UiError> {
    let neu = einstellungen.normalisiert();
    if !neu.an {
        // Ausgeschaltet heißt aus: Was im laufenden Flug noch wartet, wird
        // verworfen und kommt auch nach einem Wiedereinschalten nicht nach.
        let flug = state.active_flight.lock().expect("active_flight lock").as_ref().map(Arc::clone);
        if let Some(f) = flug {
            f.stats.lock().expect("flight stats").kabinen_lauf.warteschlange = Default::default();
        }
    }
    let pfad = einstellungen_pfad(&app)
        .ok_or_else(|| UiError::new("kein_konfig_ordner", "no config dir"))?;
    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner).map_err(|e| UiError::new("schreiben", e.to_string()))?;
    }
    let text = serde_json::to_string_pretty(&neu).map_err(|e| UiError::new("schreiben", e.to_string()))?;
    std::fs::write(&pfad, text).map_err(|e| UiError::new("schreiben", e.to_string()))?;
    *EINSTELLUNGEN.get_or_init(|| RwLock::new(None)).write().expect("kabinen einstellungen") = Some(neu.clone());
    Ok(neu)
}

/// Azure-Schlüssel speichern (`Some`) oder löschen (`None`/leer). Liefert,
/// ob danach einer hinterlegt ist. Der Schlüssel verlässt den Rechner nur
/// zu Microsoft, nie zur GSG.
#[tauri::command]
pub(crate) fn kabinen_azure_schluessel_setzen(schluessel: Option<String>) -> Result<bool, UiError> {
    match schluessel.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(s) => secrets::store_api_key(AZURE_KONTO, &s)
            .map(|_| true)
            .map_err(|e| UiError::new("schluessel", e.to_string())),
        None => {
            let _ = secrets::delete_api_key(AZURE_KONTO);
            Ok(false)
        }
    }
}

fn azure_schluessel() -> Option<String> {
    secrets::load_api_key(AZURE_KONTO).ok().flatten().filter(|s| !s.trim().is_empty())
}

/// Für die Einstellungsseite: was steht zur Verfügung?
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Zustand {
    beta: Option<bool>,
    azure_schluessel: bool,
    piper_moeglich: bool,
    piper_installiert: bool,
    /// GPL-3.0: Quellcode genau der ausgelieferten Piper-Version.
    piper_quelle: &'static str,
}

#[tauri::command]
pub(crate) async fn kabinen_zustand(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Zustand, UiError> {
    let client = state.client.lock().expect("client lock").clone();
    let beta = match client {
        Some(c) => c.kabinen_status().await.ok().map(|s| s.beta && s.bereit),
        None => None,
    };
    Ok(Zustand {
        beta,
        azure_schluessel: azure_schluessel().is_some(),
        piper_moeglich: piper::moeglich(),
        piper_installiert: piper::installiert(&app),
        piper_quelle: piper::QUELLE,
    })
}

// ---------------------------------------------------------------------------
// Zustand je Flug
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Beta {
    #[default]
    Unbekannt,
    Angefragt,
    Ja,
    Nein,
}

#[derive(Debug, Clone)]
struct Ton {
    datei: String,
    dauer_s: f64,
}

/// Ein Teil einer gespielten Ansage (eine Sprache).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Teil {
    sprache: String,
    text: String,
    datei: String,
    dauer_s: f64,
}

/// Eine gespielte Ansage — geht so an die Oberfläche.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Gespielt {
    slot: Slot,
    rolle: Rolle,
    zeit: i64,
    teile: Vec<Teil>,
    lautstaerke: f32,
}

/// Kabinenansagen eines Fluges. Nicht gespeichert: Nach einem Neustart
/// holt der Client die Texte beim Server ab (dort zwischengespeichert,
/// kostenlos) und erzeugt den Ton aus dem Zwischenspeicher auf der Platte.
#[derive(Debug, Default)]
pub(crate) struct Lauf {
    beta: Beta,
    beta_naechste_pruefung: i64,
    sprachen: Vec<String>,
    texte: HashMap<(Slot, String), String>,
    angefragt: HashSet<String>,
    fehlversuche: HashMap<String, (u8, i64)>,
    anlass: u32,
    /// `None` = für diese Sprache gibt es keine Stimme, sie entfällt.
    ton: HashMap<(Slot, String), Option<Ton>>,
    in_arbeit: HashSet<(Slot, String)>,
    warteschlange: cabin_announcements::Warteschlange,
    verlauf: Vec<Gespielt>,
}

const ABFLUG: [Slot; 8] = [
    Slot::Boarding,
    Slot::BegruessungTuerenZu,
    Slot::Sicherheit,
    Slot::KapitaenBegruessung,
    Slot::AnschnallzeichenAus,
    Slot::Reiseflug25,
    Slot::Reiseflug50,
    Slot::Reiseflug80,
];
const ANKUNFT: [Slot; 4] = [Slot::KapitaenSinkflug, Slot::KabineSinkflug, Slot::NachDerLandung, Slot::Aussteigen];
const KOMMANDOS: [Slot; 5] = [
    Slot::KommandoTuerenScharf,
    Slot::KommandoStart,
    Slot::KommandoLandungVorbereiten,
    Slot::KommandoPlaetzeLandung,
    Slot::KommandoTuerenEntschaerfen,
];
/// Diese Slots schreibt der Server erst, wenn sie fällig werden — mit den
/// Daten dieses Augenblicks.
fn ist_ereignis(s: Slot) -> bool {
    matches!(
        s,
        Slot::VerspaetungGate
            | Slot::Turbulenz
            | Slot::TurbulenzVorbei
            | Slot::Warteschleife
            | Slot::Durchstarten
            | Slot::Umleitung
    )
}

fn slot_name(s: Slot) -> String {
    serde_json::to_value(s).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn slot_aus_name(n: &str) -> Option<Slot> {
    serde_json::from_value(serde_json::Value::String(n.to_string())).ok()
}

/// Darf ein Teil (wieder) angefragt werden? Nach einem Fehler höchstens
/// dreimal, mit zwei Minuten Abstand.
fn darf_anfragen(lauf: &Lauf, schluessel: &str, jetzt: i64) -> bool {
    !lauf.angefragt.contains(schluessel)
        && lauf.fehlversuche.get(schluessel).is_none_or(|(n, ab)| *n < 3 && jetzt >= *ab)
}

/// Ein Takt der Kabinenansagen. Wird vom Positions-Streamer gerufen, nach
/// dem Phasen-Takt. Hält die Sperre nie über ein `await`.
pub(crate) fn takt(
    app: &AppHandle,
    flight: &Arc<ActiveFlight>,
    client: &Client,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
) {
    let einst = einstellungen(app);
    if !einst.an || snap.paused || snap.slew_mode {
        return;
    }
    let jetzt = now.timestamp();
    let mut stats = flight.stats.lock().expect("flight stats");
    if stats.replay_verdacht || stats.pax_count.is_none() {
        return;
    }

    // ---- Freischaltung durch den Server ----
    match stats.kabinen_lauf.beta {
        Beta::Ja => {}
        Beta::Angefragt | Beta::Nein => return,
        Beta::Unbekannt => {
            if jetzt < stats.kabinen_lauf.beta_naechste_pruefung {
                return;
            }
            stats.kabinen_lauf.beta = Beta::Angefragt;
            drop(stats);
            let (flight, client) = (Arc::clone(flight), client.clone());
            tauri::async_runtime::spawn(async move {
                let ergebnis = client.kabinen_status().await;
                let mut stats = flight.stats.lock().expect("flight stats");
                let lauf = &mut stats.kabinen_lauf;
                match ergebnis {
                    // Nur `beta` zählt: Ist der Monatsdeckel erreicht
                    // (`bereit` falsch), liefert der Server bezahlte Texte
                    // weiter aus dem Zwischenspeicher; neue lehnt er ab.
                    Ok(s) if s.beta => lauf.beta = Beta::Ja,
                    Ok(_) => lauf.beta = Beta::Nein,
                    Err(e) => {
                        // Netz weg: später noch einmal fragen, bis dahin aus.
                        tracing::info!(error = %e, "kabinenansagen: status nicht erreichbar");
                        lauf.beta = Beta::Unbekannt;
                        lauf.beta_naechste_pruefung = Utc::now().timestamp() + 300;
                    }
                }
            });
            return;
        }
    }

    // ---- Planer ----
    let flugdaten = crate::kabinen_flugdaten(&stats, &flight.arr_airport, flight.planned_flight_time_min);
    let flugart = flugdaten.flugart.unwrap_or(Flugart::Aus);
    if flugart == Flugart::Aus {
        return;
    }
    let schritt = crate::kabinen_schritt(&mut stats, snap, now, &flight.arr_airport, flight.planned_flight_time_min);
    let faellig: Vec<Faellig> = schritt
        .faellig
        .into_iter()
        .filter(|f| match f.slot.rolle() {
            Rolle::Chime => einst.chimes,
            Rolle::Kommando => einst.kommandos,
            _ => true,
        })
        .collect();

    // ---- Texte holen ----
    let mut auftraege: Vec<(String, Vec<Slot>, u32)> = Vec::new();
    {
        let lauf = &mut stats.kabinen_lauf;
        if darf_anfragen(lauf, "abflug", jetzt) {
            let slots: Vec<Slot> = ABFLUG.into_iter().filter(|s| flugart.erlaubt(*s)).collect();
            if !slots.is_empty() {
                lauf.angefragt.insert("abflug".into());
                auftraege.push(("abflug".into(), slots, 0));
            }
        }
        let r = cabin_announcements::planer::rang(schritt.phase);
        let bald_sinkflug = schritt.rest_min.is_some_and(|m| m <= 45.0)
            || r >= cabin_announcements::planer::rang(FlightPhase::Descent);
        if r >= cabin_announcements::planer::rang(FlightPhase::Cruise) && bald_sinkflug && darf_anfragen(lauf, "ankunft", jetzt) {
            let slots: Vec<Slot> = ANKUNFT.into_iter().filter(|s| flugart.erlaubt(*s)).collect();
            if !slots.is_empty() {
                lauf.angefragt.insert("ankunft".into());
                auftraege.push(("ankunft".into(), slots, 0));
            }
        }
        for f in faellig.iter().filter(|f| ist_ereignis(f.slot)) {
            lauf.anlass += 1;
            let schluessel = format!("ereignis:{}", lauf.anlass);
            lauf.angefragt.insert(schluessel.clone());
            auftraege.push((schluessel, vec![f.slot], lauf.anlass));
        }
        lauf.warteschlange.aufnehmen(faellig.clone());
    }
    for (schluessel, slots, anlass) in auftraege {
        let anfrage = anfrage_bauen(&stats, flight, &einst, &schluessel, &slots, anlass, jetzt);
        holen(app.clone(), Arc::clone(flight), client.clone(), schluessel, anfrage, slots, false);
    }

    // ---- Kommandos vorbereiten (fester Wortlaut, Englisch) ----
    if einst.kommandos {
        let offen: Vec<(Slot, String, String)> = KOMMANDOS
            .into_iter()
            .filter(|s| flugart.erlaubt(*s))
            .filter_map(|s| s.kommando_text().map(|t| (s, "en-GB".to_string(), t.to_string())))
            .filter(|(s, sp, _)| {
                let k = (*s, sp.clone());
                !stats.kabinen_lauf.ton.contains_key(&k) && !stats.kabinen_lauf.in_arbeit.contains(&k)
            })
            .collect();
        if !offen.is_empty() {
            for (s, sp, _) in &offen {
                stats.kabinen_lauf.in_arbeit.insert((*s, sp.clone()));
            }
            sprechen_lassen(app.clone(), Arc::clone(flight), offen);
        }
    }

    // ---- Abspielen ----
    let lauf = &mut stats.kabinen_lauf;
    let sprachen = lauf.sprachen.clone();
    let bereit = |slot: Slot| -> bool {
        match slot.rolle() {
            Rolle::Chime => true,
            Rolle::Kommando => lauf.ton.contains_key(&(slot, "en-GB".to_string())),
            _ => {
                let mit_text: Vec<&String> = sprachen.iter().filter(|sp| lauf.texte.contains_key(&(slot, (*sp).clone()))).collect();
                !mit_text.is_empty() && mit_text.iter().all(|sp| lauf.ton.contains_key(&(slot, (*sp).clone())))
            }
        }
    };
    let Some(slot) = lauf.warteschlange.naechstes_bereit(jetzt, schritt.phase, schritt.ruhephase, bereit) else {
        for weg in lauf.warteschlange.verfallene_abholen() {
            tracing::info!(slot = ?weg, "kabinenansage verfallen");
        }
        return;
    };
    let sprachen_des_slots: Vec<String> = if slot.rolle() == Rolle::Kommando { vec!["en-GB".into()] } else { sprachen };
    let teile: Vec<Teil> = sprachen_des_slots
        .iter()
        .filter_map(|sp| {
            let ton = lauf.ton.get(&(slot, sp.clone()))?.as_ref()?;
            let text = lauf
                .texte
                .get(&(slot, sp.clone()))
                .cloned()
                .or_else(|| slot.kommando_text().map(str::to_string))?;
            Some(Teil { sprache: sp.clone(), text, datei: ton.datei.clone(), dauer_s: ton.dauer_s })
        })
        .collect();
    if teile.is_empty() && slot.rolle() != Rolle::Chime {
        tracing::info!(slot = ?slot, "kabinenansage ohne Stimme entfällt");
        return;
    }
    // Zwischen den Sprachen eine kurze Pause; ein Chime dauert gut zwei Sekunden.
    let dauer: f64 = if slot.rolle() == Rolle::Chime {
        2.5
    } else {
        teile.iter().map(|t| t.dauer_s + 0.8).sum()
    };
    lauf.warteschlange.spielt_bis(jetzt + dauer.ceil() as i64 + 1);
    let gespielt = Gespielt { slot, rolle: slot.rolle(), zeit: jetzt, teile, lautstaerke: einst.lautstaerke };
    lauf.verlauf.push(gespielt.clone());
    drop(stats);
    tracing::info!(slot = ?slot, "kabinenansage spielt");
    let _ = app.emit(EREIGNIS_ANSAGE, gespielt);
}

/// Die Anfrage an den GSG-Server. Der Server kennt Airline, Flugnummer,
/// Muster, Städte und Zeitzonen selbst; hier steht, was nur der Client weiß.
fn anfrage_bauen(
    stats: &FlightStats,
    flight: &ActiveFlight,
    einst: &Einstellungen,
    schluessel: &str,
    slots: &[Slot],
    anlass: u32,
    jetzt: i64,
) -> serde_json::Value {
    let teil = schluessel.split(':').next().unwrap_or("ereignis");
    let plan = &stats.ofp_planzeiten;
    // Abweichung vom Plan-Off-Block: am Gate die laufende, danach die echte.
    let abweichung = plan.plan_off_block.map(|p| {
        let ab = stats.block_off_at.map(|t| t.timestamp()).unwrap_or(jetzt);
        (ab - p).div_euclid(60)
    });
    let flugdaten = crate::kabinen_flugdaten(stats, &flight.arr_airport, flight.planned_flight_time_min);
    let ankunft = stats
        .landung_erwartet
        .or_else(|| plan.plan_on_block.map(|p| p + abweichung.unwrap_or(0).max(0) * 60));
    let wetter = stats.arr_metar_snap.as_ref().and_then(|m| {
        m.temperature_c.map(|t| serde_json::json!({ "temperatur_c": t, "lage": wetterlage(m) }))
    });
    serde_json::json!({
        "pirep_id": flight.pirep_id,
        "teil": teil,
        "slots": slots.iter().map(|s| slot_name(*s)).collect::<Vec<_>>(),
        "anlass": anlass,
        "zweite_sprache": einst.zweite_sprache,
        "eigene_sprache": einst.eigene_sprache,
        "echter_name": einst.echter_name,
        "tagesform_kapitaen": einst.tagesform_kapitaen,
        "tagesform_kabine": einst.tagesform_kabine,
        "flug": {
            "pax": stats.pax_count,
            "sitze": stats.max_passengers,
            "flugzeit_min": flugdaten.geplante_flugzeit_min,
            "sched_out": plan.plan_off_block,
            "sched_in": plan.plan_on_block,
            "ankunft_erwartet": ankunft,
            "abweichung_min": abweichung,
            "rest_min": stats.landung_erwartet.map(|t| ((t - jetzt) / 60).max(0)),
            "utc_versatz_abflug_min": stats.utc_versatz_abflug_min,
            "utc_versatz_ziel_min": stats.utc_versatz_ziel_min,
            "reiseflughoehe_ft": flugdaten.reiseflughoehe_ft,
            "wetter_ziel": wetter,
            "umleitung_icao": stats.umleitung_ziel,
        }
    })
}

/// Wetter am Ziel als einfache Lage für die Ansage (Leitfaden §1: keine
/// Wolkenuntergrenze, kein Seitenwind).
fn wetterlage(m: &metar::MetarSnapshot) -> Option<&'static str> {
    let wx = m.weather.as_deref().unwrap_or("").to_ascii_uppercase();
    if wx.contains("TS") {
        return Some("Gewitter");
    }
    if wx.contains("SN") {
        return Some("Schnee");
    }
    if wx.contains("RA") || wx.contains("DZ") {
        return Some(if wx.contains('-') { "leichter Regen" } else { "Regen" });
    }
    if wx.contains("FG") || wx.contains("BR") {
        return Some("Nebel oder Dunst");
    }
    let bedeckt = m.cloud_layers.iter().any(|c| matches!(c.cover.as_str(), "BKN" | "OVC"));
    if bedeckt {
        return Some("bewölkt");
    }
    if m.cloud_layers.iter().any(|c| matches!(c.cover.as_str(), "FEW" | "SCT")) {
        return Some("leicht bewölkt");
    }
    Some("klar")
}

fn holen(
    app: AppHandle,
    flight: Arc<ActiveFlight>,
    client: Client,
    schluessel: String,
    anfrage: serde_json::Value,
    slots: Vec<Slot>,
    nachholung: bool,
) {
    tauri::async_runtime::spawn(async move {
        let ergebnis = client.kabinen_drehbuch(&anfrage).await;
        let mut fehlend: Vec<Slot> = Vec::new();
        let neu: Vec<(Slot, String, String)> = {
            let mut stats = flight.stats.lock().expect("flight stats");
            let lauf = &mut stats.kabinen_lauf;
            match ergebnis {
                Ok(d) => {
                    if !d.sprachen.is_empty() {
                        lauf.sprachen = d.sprachen.clone();
                    }
                    let mut neu = Vec::new();
                    for e in d.eintraege {
                        let Some(slot) = slot_aus_name(&e.slot) else { continue };
                        let k = (slot, e.sprache.clone());
                        // Ein Ereignis (z. B. die zweite Turbulenz) bekommt neuen Ton.
                        if ist_ereignis(slot) {
                            lauf.ton.remove(&k);
                        }
                        lauf.texte.insert(k.clone(), e.text.clone());
                        if !lauf.ton.contains_key(&k) && lauf.in_arbeit.insert(k) {
                            neu.push((slot, e.sprache, e.text));
                        }
                    }
                    // Hat der Server nach seinem zweiten Versuch einen Text
                    // nicht geliefert, einmal gezielt nur diese Slots holen.
                    fehlend = slots
                        .iter()
                        .copied()
                        .filter(|s| lauf.sprachen.iter().any(|sp| !lauf.texte.contains_key(&(*s, sp.clone()))))
                        .collect();
                    neu
                }
                Err(e) => {
                    tracing::warn!(teil = %schluessel, error = %e, "kabinenansagen: drehbuch nicht erhalten");
                    lauf.angefragt.remove(&schluessel);
                    let n = lauf.fehlversuche.get(&schluessel).map_or(0, |(n, _)| *n) + 1;
                    lauf.fehlversuche.insert(schluessel.clone(), (n, Utc::now().timestamp() + 120));
                    Vec::new()
                }
            }
        };
        if !neu.is_empty() {
            sprechen_lassen(app.clone(), Arc::clone(&flight), neu);
        }
        if !nachholung && !fehlend.is_empty() {
            let mut erneut = anfrage.clone();
            erneut["slots"] = serde_json::json!(fehlend.iter().map(|s| slot_name(*s)).collect::<Vec<_>>());
            tracing::info!(teil = %schluessel, fehlend = ?fehlend, "kabinenansagen: fehlende Texte nachholen");
            holen(app, flight, client, format!("{schluessel}+"), erneut, fehlend, true);
        }
    });
}

/// Erzeugt den Ton nacheinander (nicht parallel: Piper braucht die CPU, die
/// der Simulator auch will) und trägt ihn in den Lauf ein.
fn sprechen_lassen(app: AppHandle, flight: Arc<ActiveFlight>, auftraege: Vec<(Slot, String, String)>) {
    tauri::async_runtime::spawn(async move {
        let einst = einstellungen(&app);
        for (slot, sprache, text) in auftraege {
            let ton = match sprechen(&app, &einst, slot.rolle(), &sprache, &text).await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(slot = ?slot, sprache = %sprache, error = %e, "kabinenansage: Ton nicht erzeugt");
                    None
                }
            };
            let mut stats = flight.stats.lock().expect("flight stats");
            let k = (slot, sprache);
            stats.kabinen_lauf.in_arbeit.remove(&k);
            stats.kabinen_lauf.ton.insert(k, ton);
        }
    });
}

// ---------------------------------------------------------------------------
// Sprachausgabe
// ---------------------------------------------------------------------------

/// Kapitän und Kommandos männlich, Kabine weiblich.
fn maennlich(rolle: Rolle) -> bool {
    !matches!(rolle, Rolle::Kabine)
}

/// Azure-Stimmen je Locale (männlich, weiblich). Nur gesicherte Neural-Stimmen.
fn azure_stimme(sprache: &str, m: bool) -> Option<&'static str> {
    let (mann, frau) = match sprache {
        "de-DE" => ("de-DE-ConradNeural", "de-DE-KatjaNeural"),
        "de-CH" => ("de-CH-JanNeural", "de-CH-LeniNeural"),
        "de-AT" => ("de-AT-JonasNeural", "de-AT-IngridNeural"),
        "en-GB" => ("en-GB-RyanNeural", "en-GB-SoniaNeural"),
        "en-US" => ("en-US-GuyNeural", "en-US-JennyNeural"),
        "fr-FR" => ("fr-FR-HenriNeural", "fr-FR-DeniseNeural"),
        "it-IT" => ("it-IT-DiegoNeural", "it-IT-ElsaNeural"),
        "nl-NL" => ("nl-NL-MaartenNeural", "nl-NL-ColetteNeural"),
        "es-ES" => ("es-ES-AlvaroNeural", "es-ES-ElviraNeural"),
        "pt-PT" => ("pt-PT-DuarteNeural", "pt-PT-RaquelNeural"),
        "pt-BR" => ("pt-BR-AntonioNeural", "pt-BR-FranciscaNeural"),
        "lv-LV" => ("lv-LV-NilsNeural", "lv-LV-EveritaNeural"),
        "ar-AE" => ("ar-AE-HamdanNeural", "ar-AE-FatimaNeural"),
        "pl-PL" => ("pl-PL-MarekNeural", "pl-PL-ZofiaNeural"),
        "tr-TR" => ("tr-TR-AhmetNeural", "tr-TR-EmelNeural"),
        "da-DK" => ("da-DK-JeppeNeural", "da-DK-ChristelNeural"),
        "sv-SE" => ("sv-SE-MattiasNeural", "sv-SE-SofieNeural"),
        "nb-NO" => ("nb-NO-FinnNeural", "nb-NO-PernilleNeural"),
        _ => return None,
    };
    Some(if m { mann } else { frau })
}

fn ton_ordner(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("kabinenansagen").join("ton");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

/// Text zu Ton. Reihenfolge: der gewählte Anbieter der Rolle; fehlt dessen
/// Schlüssel oder Stimme, der andere. Gibt es für die Sprache keine Stimme,
/// entfällt sie (`Ok(None)`) — nie eine Stimme einer anderen Sprache.
async fn sprechen(
    app: &AppHandle,
    einst: &Einstellungen,
    rolle: Rolle,
    sprache: &str,
    text: &str,
) -> Result<Option<Ton>, String> {
    let ordner = ton_ordner(app)?;
    let schluessel = azure_schluessel();
    let reihe: [&str; 2] = if einst.anbieter(rolle) == "azure" { ["azure", "piper"] } else { ["piper", "azure"] };
    for anbieter in reihe {
        match anbieter {
            "azure" => {
                let (Some(key), Some(stimme)) = (schluessel.as_deref(), azure_stimme(sprache, maennlich(rolle))) else {
                    continue;
                };
                let name = cache::dateiname("azure", stimme, sprache, text, "wav");
                let pfad = ordner.join(&name);
                if !pfad.exists() {
                    let wav = azure_sprechen(key, &einst.azure_region, stimme, text).await?;
                    std::fs::write(&pfad, &wav).map_err(|e| e.to_string())?;
                }
                return ton_aus_datei(&pfad, name).map(Some);
            }
            _ => {
                let Some(stimme) = piper::stimme_fuer(sprache, maennlich(rolle)) else { continue };
                if !piper::installiert(app) {
                    continue;
                }
                let name = cache::dateiname("piper", stimme.name, sprache, text, "wav");
                let pfad = ordner.join(&name);
                if !pfad.exists() {
                    let (app2, text2, ziel) = (app.clone(), text.to_string(), pfad.clone());
                    tauri::async_runtime::spawn_blocking(move || piper::sprechen(&app2, stimme, &text2, &ziel))
                        .await
                        .map_err(|e| e.to_string())??;
                }
                return ton_aus_datei(&pfad, name).map(Some);
            }
        }
    }
    Ok(None)
}

fn ton_aus_datei(pfad: &Path, name: String) -> Result<Ton, String> {
    let daten = std::fs::read(pfad).map_err(|e| e.to_string())?;
    let dauer_s = wav_dauer_s(&daten).ok_or_else(|| {
        let _ = std::fs::remove_file(pfad);
        "kein gültiges WAV".to_string()
    })?;
    Ok(Ton { datei: name, dauer_s })
}

/// Dauer aus dem WAV-Kopf: Größe des `data`-Blocks durch Bytes je Sekunde.
pub(crate) fn wav_dauer_s(d: &[u8]) -> Option<f64> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    let mut byterate: Option<u32> = None;
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let groesse = u32::from_le_bytes([d[i + 4], d[i + 5], d[i + 6], d[i + 7]]) as usize;
        let inhalt = i + 8;
        if id == b"fmt " && inhalt + 12 <= d.len() {
            byterate = Some(u32::from_le_bytes([d[inhalt + 8], d[inhalt + 9], d[inhalt + 10], d[inhalt + 11]]));
        }
        if id == b"data" {
            let rate = byterate.filter(|r| *r > 0)?;
            // Manche Kodierer schreiben 0xFFFFFFFF; dann zählt, was da ist.
            let n = groesse.min(d.len().saturating_sub(inhalt));
            return Some(n as f64 / f64::from(rate));
        }
        i = inhalt + groesse + (groesse & 1);
    }
    None
}

async fn azure_sprechen(key: &str, region: &str, stimme: &str, text: &str) -> Result<Vec<u8>, String> {
    let ssml = ssml::azure(text, stimme, "").ok_or("ungültige Stimme")?;
    let url = format!("https://{region}.tts.speech.microsoft.com/cognitiveservices/v1");
    let antwort = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?
        .post(url)
        .header("Ocp-Apim-Subscription-Key", key)
        .header("Content-Type", "application/ssml+xml")
        .header("X-Microsoft-OutputFormat", "riff-24khz-16bit-mono-pcm")
        .header("User-Agent", format!("AeroACARS/{}", env!("CARGO_PKG_VERSION")))
        .body(ssml)
        .send()
        .await
        .map_err(|e| format!("Azure nicht erreichbar: {e}"))?;
    if !antwort.status().is_success() {
        return Err(format!("Azure antwortet {}", antwort.status()));
    }
    antwort.bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string())
}

/// Liefert eine erzeugte Tondatei an die Oberfläche. Nur Dateinamen aus dem
/// eigenen Ton-Ordner, keine Pfade.
#[tauri::command]
pub(crate) fn kabinen_audio(app: AppHandle, datei: String) -> Result<tauri::ipc::Response, UiError> {
    let sauber = !datei.is_empty()
        && datei.ends_with(".wav")
        && datei.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !datei.contains("..");
    if !sauber {
        return Err(UiError::new("ungueltig", "invalid file name"));
    }
    let pfad = ton_ordner(&app).map_err(|e| UiError::new("ordner", e))?.join(datei);
    let daten = std::fs::read(pfad).map_err(|e| UiError::new("lesen", e.to_string()))?;
    Ok(tauri::ipc::Response::new(daten))
}

/// Hörprobe für die Einstellungen: eine Rolle in einer Sprache.
#[tauri::command]
pub(crate) async fn kabinen_probe(app: AppHandle, rolle: Rolle, sprache: String) -> Result<Gespielt, UiError> {
    if !ist_locale(&sprache) {
        return Err(UiError::new("ungueltig", "invalid language"));
    }
    let text = match (sprache.split('-').next().unwrap_or(""), rolle) {
        ("de", Rolle::Kapitaen) => "Guten Tag, liebe Gäste, hier spricht Ihr Kapitän. Dies ist eine Hörprobe.",
        ("de", _) => "Herzlich willkommen an Bord. Dies ist eine Hörprobe Ihrer Kabinenbesatzung.",
        (_, Rolle::Kapitaen) => "Good afternoon, ladies and gentlemen, this is your captain speaking. This is a voice sample.",
        _ => "Welcome on board. This is a voice sample from your cabin crew.",
    };
    let einst = einstellungen(&app);
    let ton = sprechen(&app, &einst, rolle, &sprache, text)
        .await
        .map_err(|e| UiError::new("sprachausgabe", e))?
        .ok_or_else(|| UiError::new("keine_stimme", "no voice for this language and provider"))?;
    Ok(Gespielt {
        slot: Slot::Boarding,
        rolle,
        zeit: Utc::now().timestamp(),
        teile: vec![Teil { sprache, text: text.to_string(), datei: ton.datei, dauer_s: ton.dauer_s }],
        lautstaerke: einst.lautstaerke,
    })
}

/// Für die Ansage-Seite: was lief und was wartet im aktuellen Flug.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Ueberblick {
    verlauf: Vec<Gespielt>,
    wartend: Vec<Slot>,
    sprachen: Vec<String>,
    beta: Option<bool>,
}

#[tauri::command]
pub(crate) fn kabinen_ueberblick(state: tauri::State<'_, AppState>) -> Option<Ueberblick> {
    let flight = state.active_flight.lock().expect("active_flight lock").as_ref().map(Arc::clone)?;
    let stats = flight.stats.lock().expect("flight stats");
    let lauf = &stats.kabinen_lauf;
    Some(Ueberblick {
        verlauf: lauf.verlauf.clone(),
        wartend: lauf.warteschlange.wartend(),
        sprachen: lauf.sprachen.clone(),
        beta: match lauf.beta {
            Beta::Ja => Some(true),
            Beta::Nein => Some(false),
            _ => None,
        },
    })
}

// ---------------------------------------------------------------------------
// Piper (kostenlos, getrenntes Programm unter GPL-3.0)
// ---------------------------------------------------------------------------

pub(crate) mod piper {
    use super::*;

    /// rhasspy/piper 2023.11.14-2 (archiviert, letzter Stand mit fertigem
    /// Windows-Programm). Für macOS gibt es kein brauchbares Archiv (das
    /// „aarch64“-Paket ist x86_64 ohne Bibliotheken) — dort nur Azure.
    pub(crate) const PROGRAMM_URL: &str =
        "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip";
    pub(crate) const PROGRAMM_SHA256: &str = "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea";
    pub(crate) const QUELLE: &str = "https://github.com/rhasspy/piper/tree/2023.11.14-2";
    /// Fester Stand des Stimmen-Repos, damit sich keine Datei unbemerkt ändert.
    const STIMMEN_STAND: &str = "c10ece1aade47bb51c153c893d14e5bf8e5b7117";

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Stimme {
        pub(crate) name: &'static str,
        pfad: &'static str,
        pub(crate) onnx_sha256: &'static str,
        pub(crate) json_sha256: &'static str,
    }

    /// Die vier Stimmen der Beta. Thorsten, Kerstin und Jenny hat Thomas nach
    /// Gehör freigegeben (17.10.2026); Alan („All Rights Reserved“) ist durch
    /// northern_english_male ersetzt (CC BY-SA 4.0) — noch nicht angehört.
    pub(crate) const STIMMEN: [Stimme; 4] = [
        Stimme {
            name: "de_DE-thorsten-high",
            pfad: "de/de_DE/thorsten/high",
            onnx_sha256: "9df1c43c61149ef9b39e618e2b861fbe41e1fcea9390b2dac62e8761573ea4f1",
            json_sha256: "6de734444e4c3f9e33b7ebe2746dbc19b71e85f613e79c65acf623200b99a76a",
        },
        Stimme {
            name: "de_DE-kerstin-low",
            pfad: "de/de_DE/kerstin/low",
            onnx_sha256: "d352a7641892cebf2903859af94e9ba81a141110215fe3943bcda7f7da401b7a",
            json_sha256: "56e708556b7b9b7a53c4f8957e021421e69f11a600962bba554cffbe72cf2d47",
        },
        Stimme {
            name: "en_GB-northern_english_male-medium",
            pfad: "en/en_GB/northern_english_male/medium",
            onnx_sha256: "57a219ae8e638873db7d18893304be5069c42868f392bb95c3ff17f0690d0689",
            json_sha256: "69557ed3d974463453e9b0c09dd99a7ed0e52b8b87b64b357dbeeb2540a97d47",
        },
        Stimme {
            name: "en_GB-jenny_dioco-medium",
            pfad: "en/en_GB/jenny_dioco/medium",
            onnx_sha256: "469c630d209e139dd392a66bf4abde4ab86390a0269c1e47b4e5d7ce81526b01",
            json_sha256: "a9a7a93a317c9a3cb6563e37eb057df9ef09c06188a8a4341b0fcb58cba54dd4",
        },
    ];

    /// Deutsch (auch de-CH/de-AT, Hochdeutsch) und Englisch; alles andere
    /// nur mit Azure.
    pub(crate) fn stimme_fuer(sprache: &str, m: bool) -> Option<Stimme> {
        let name = match (sprache.split('-').next()?, m) {
            ("de", true) => "de_DE-thorsten-high",
            ("de", false) => "de_DE-kerstin-low",
            ("en", true) => "en_GB-northern_english_male-medium",
            ("en", false) => "en_GB-jenny_dioco-medium",
            _ => return None,
        };
        STIMMEN.into_iter().find(|s| s.name == name)
    }

    pub(crate) fn moeglich() -> bool {
        cfg!(target_os = "windows")
    }

    fn basis(app: &AppHandle) -> Option<PathBuf> {
        app.path().app_data_dir().ok().map(|p| p.join("kabinenansagen"))
    }

    fn programm(app: &AppHandle) -> Option<PathBuf> {
        basis(app).map(|b| b.join("piper").join("piper").join("piper.exe"))
    }

    fn stimm_datei(app: &AppHandle, s: &Stimme, endung: &str) -> Option<PathBuf> {
        basis(app).map(|b| b.join("stimmen").join(format!("{}{endung}", s.name)))
    }

    pub(crate) fn installiert(app: &AppHandle) -> bool {
        moeglich()
            && programm(app).is_some_and(|p| p.exists())
            && STIMMEN.iter().all(|s| {
                stimm_datei(app, s, ".onnx").is_some_and(|p| p.exists())
                    && stimm_datei(app, s, ".onnx.json").is_some_and(|p| p.exists())
            })
    }

    /// Piper als eigener Prozess: Text über stdin, WAV als Datei.
    pub(crate) fn sprechen(app: &AppHandle, s: Stimme, text: &str, ziel: &Path) -> Result<(), String> {
        let exe = programm(app).ok_or("kein Datenordner")?;
        let modell = stimm_datei(app, &s, ".onnx").ok_or("kein Datenordner")?;
        let teil = ziel.with_extension("part.wav");
        let mut cmd = std::process::Command::new(&exe);
        cmd.arg("-m")
            .arg(&modell)
            .arg("-f")
            .arg(&teil)
            .arg("--sentence_silence")
            .arg("0.3")
            .current_dir(exe.parent().ok_or("kein Ordner")?)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            // Kein schwarzes Konsolenfenster über dem Simulator.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let mut kind = cmd.spawn().map_err(|e| format!("Piper startet nicht: {e}"))?;
        // Zeilenumbrüche würde Piper als Sätze lesen; ein Text, eine Zeile.
        let eine_zeile: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        kind.stdin
            .take()
            .ok_or("kein stdin")?
            .write_all(eine_zeile.as_bytes())
            .map_err(|e| e.to_string())?;
        let status = kind.wait().map_err(|e| e.to_string())?;
        if !status.success() {
            let _ = std::fs::remove_file(&teil);
            return Err(format!("Piper endete mit {status}"));
        }
        std::fs::rename(&teil, ziel).map_err(|e| e.to_string())
    }

    #[derive(Debug, Clone, Serialize)]
    struct Fortschritt {
        schritt: String,
        bytes: u64,
        gesamt: u64,
    }

    async fn laden(
        app: &AppHandle,
        client: &reqwest::Client,
        url: &str,
        sha256: &str,
        ziel: &Path,
        schritt: &str,
    ) -> Result<Vec<u8>, String> {
        let mut antwort = client.get(url).send().await.map_err(|e| format!("{schritt}: {e}"))?;
        if !antwort.status().is_success() {
            return Err(format!("{schritt}: HTTP {}", antwort.status()));
        }
        let gesamt = antwort.content_length().unwrap_or(0);
        let mut hasher = Sha256::new();
        let mut puffer = Vec::new();
        let mut zuletzt = 0u64;
        while let Some(stueck) = antwort.chunk().await.map_err(|e| format!("{schritt}: {e}"))? {
            hasher.update(&stueck);
            puffer.extend_from_slice(&stueck);
            let n = puffer.len() as u64;
            if n - zuletzt > 2_000_000 || n == gesamt {
                zuletzt = n;
                let _ = app.emit(EREIGNIS_PIPER, Fortschritt { schritt: schritt.into(), bytes: n, gesamt });
            }
        }
        let ist = hex(&hasher.finalize());
        if ist != sha256 {
            return Err(format!("{schritt}: Prüfsumme stimmt nicht"));
        }
        if let Some(ordner) = ziel.parent() {
            std::fs::create_dir_all(ordner).map_err(|e| e.to_string())?;
        }
        Ok(puffer)
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    fn schreiben(ziel: &Path, daten: &[u8]) -> Result<(), String> {
        let teil = ziel.with_extension("part");
        std::fs::write(&teil, daten).map_err(|e| e.to_string())?;
        std::fs::rename(&teil, ziel).map_err(|e| e.to_string())
    }

    /// Lädt Piper und die vier Stimmen (~330 MB) nach ausdrücklichem Klick.
    /// Jede Datei wird vor der Nutzung gegen ihre SHA-256 geprüft.
    #[tauri::command]
    pub(crate) async fn kabinen_piper_installieren(app: AppHandle) -> Result<bool, UiError> {
        if !moeglich() {
            return Err(UiError::new("nur_windows", "Piper is available on Windows only"));
        }
        let fehler = |e: String| UiError::new("piper_installation", e);
        let basis = basis(&app).ok_or_else(|| fehler("kein Datenordner".into()))?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(600))
            .redirect(reqwest::redirect::Policy::limited(8))
            .build()
            .map_err(|e| fehler(e.to_string()))?;

        if !programm(&app).is_some_and(|p| p.exists()) {
            let zip = laden(&app, &client, PROGRAMM_URL, PROGRAMM_SHA256, &basis.join("piper.zip"), "piper").await.map_err(fehler)?;
            let ziel = basis.join("piper");
            let mut archiv = zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| fehler(e.to_string()))?;
            for i in 0..archiv.len() {
                let mut eintrag = archiv.by_index(i).map_err(|e| fehler(e.to_string()))?;
                let Some(rel) = eintrag.enclosed_name().map(|p| p.to_path_buf()) else { continue };
                let pfad = ziel.join(rel);
                if !pfad.starts_with(&ziel) {
                    continue;
                }
                if eintrag.is_dir() {
                    std::fs::create_dir_all(&pfad).map_err(|e| fehler(e.to_string()))?;
                    continue;
                }
                if let Some(o) = pfad.parent() {
                    std::fs::create_dir_all(o).map_err(|e| fehler(e.to_string()))?;
                }
                let mut aus = std::fs::File::create(&pfad).map_err(|e| fehler(e.to_string()))?;
                std::io::copy(&mut eintrag, &mut aus).map_err(|e| fehler(e.to_string()))?;
            }
        }

        for s in STIMMEN {
            for (endung, sha) in [(".onnx", s.onnx_sha256), (".onnx.json", s.json_sha256)] {
                let ziel = stimm_datei(&app, &s, endung).ok_or_else(|| fehler("kein Datenordner".into()))?;
                if ziel.exists() {
                    continue;
                }
                let url = format!(
                    "https://huggingface.co/rhasspy/piper-voices/resolve/{STIMMEN_STAND}/{}/{}{endung}",
                    s.pfad, s.name
                );
                let daten = laden(&app, &client, &url, sha, &ziel, s.name).await.map_err(fehler)?;
                schreiben(&ziel, &daten).map_err(fehler)?;
            }
        }
        Ok(installiert(&app))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(byterate: u32, daten: usize) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WAVE".to_vec();
        v.extend_from_slice(b"fmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes()); // PCM
        v.extend_from_slice(&1u16.to_le_bytes()); // mono
        v.extend_from_slice(&(byterate / 2).to_le_bytes()); // Abtastrate
        v.extend_from_slice(&byterate.to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"LIST");
        v.extend_from_slice(&3u32.to_le_bytes());
        v.extend_from_slice(b"abc\0"); // ungerade Größe + Füllbyte
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(daten as u32).to_le_bytes());
        v.extend(std::iter::repeat_n(0u8, daten));
        v
    }

    #[test]
    fn wav_dauer_aus_dem_kopf() {
        // Azure riff-24khz-16bit-mono: 48.000 Bytes je Sekunde.
        assert_eq!(wav_dauer_s(&wav(48_000, 96_000)), Some(2.0));
        // Piper 22,05 kHz.
        let d = wav_dauer_s(&wav(44_100, 44_100)).unwrap();
        assert!((d - 1.0).abs() < 1e-9);
        assert_eq!(wav_dauer_s(b"kein wav"), None);
        assert_eq!(wav_dauer_s(&[]), None);
    }

    #[test]
    fn einstellungen_standard_aus_und_normalisiert() {
        let e = Einstellungen::default();
        assert!(!e.an, "Standard aus — sonst zahlt jeder Pilot");
        assert_eq!(e.zweite_sprache, "airline");
        let kaputt = Einstellungen {
            zweite_sprache: "klingonisch".into(),
            eigene_sprache: Some("deutsch".into()),
            anbieter_kabine: "elevenlabs".into(),
            azure_region: "west europe/../".into(),
            tagesform_kapitaen: "albern".into(),
            lautstaerke: 7.0,
            ..Einstellungen::default()
        }
        .normalisiert();
        assert_eq!(kaputt.zweite_sprache, "airline");
        assert_eq!(kaputt.eigene_sprache, None);
        assert_eq!(kaputt.anbieter_kabine, "piper");
        assert_eq!(kaputt.azure_region, "westeurope");
        assert_eq!(kaputt.tagesform_kapitaen, "zufall");
        assert_eq!(kaputt.lautstaerke, 1.0);
        // Alte Datei ohne neue Felder lädt mit Standardwerten.
        let alt: Einstellungen = serde_json::from_str(r#"{"an": true}"#).unwrap();
        assert!(alt.an && alt.kommandos && alt.chimes);
    }

    #[test]
    fn stimmen_nur_in_ihrer_sprache() {
        // Feste Regel (Thomas 17.10.): eine Sprache spricht nur eine Stimme
        // dieser Sprache — keine deutsche Stimme liest Englisch.
        assert_eq!(piper::stimme_fuer("de-CH", true).unwrap().name, "de_DE-thorsten-high");
        assert_eq!(piper::stimme_fuer("en-US", false).unwrap().name, "en_GB-jenny_dioco-medium");
        assert!(piper::stimme_fuer("fr-FR", true).is_none());
        assert!(piper::stimme_fuer("lv-LV", false).is_none());
        assert_eq!(azure_stimme("de-AT", false), Some("de-AT-IngridNeural"));
        assert_eq!(azure_stimme("de-CH", true), Some("de-CH-JanNeural"));
        assert_eq!(azure_stimme("xx-XX", true), None);
        for sp in ["de-DE", "de-CH", "de-AT", "en-GB", "fr-FR", "ar-AE", "lv-LV", "nl-NL", "it-IT"] {
            for m in [true, false] {
                let s = azure_stimme(sp, m).unwrap();
                assert_eq!(ssml::locale_aus_stimme(s), Some(sp), "{s}");
            }
        }
        // Prüfsummen sind 64 Hex-Zeichen (ein Tippfehler ließe jede
        // Installation an der Prüfung scheitern).
        let hex = |h: &str| h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit());
        for s in piper::STIMMEN {
            assert!(hex(s.onnx_sha256) && hex(s.json_sha256), "{}", s.name);
        }
        assert!(hex(piper::PROGRAMM_SHA256));
    }

    #[test]
    fn slot_namen_hin_und_zurueck() {
        for s in Slot::ALLE {
            assert_eq!(slot_aus_name(&slot_name(s)), Some(s));
        }
        assert_eq!(slot_aus_name("gibt_es_nicht"), None);
    }

    #[test]
    fn nach_fehler_hoechstens_dreimal_mit_abstand() {
        let mut l = Lauf::default();
        assert!(darf_anfragen(&l, "abflug", 0));
        l.angefragt.insert("abflug".into());
        assert!(!darf_anfragen(&l, "abflug", 0), "läuft schon");
        l.angefragt.remove("abflug");
        l.fehlversuche.insert("abflug".into(), (1, 120));
        assert!(!darf_anfragen(&l, "abflug", 60), "zu früh");
        assert!(darf_anfragen(&l, "abflug", 120));
        l.fehlversuche.insert("abflug".into(), (3, 0));
        assert!(!darf_anfragen(&l, "abflug", 999), "nach drei Fehlern Schluss");
    }
}
