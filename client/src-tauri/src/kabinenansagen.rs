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
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use api_client::Client;
use cabin_announcements::flugart::Flugart;
use cabin_announcements::{cache, ssml, Faellig, Rolle, Slot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sim_core::{FlightPhase, SimSnapshot};
use tauri::{AppHandle, Emitter, Manager};

use crate::{ActiveFlight, AppState, FlightStats, UiError};

/// Schlüsselbund-Konto für den Azure-Schlüssel des Piloten.
const AZURE_KONTO: &str = "kabinenansagen_azure";
/// Ereignis an die Oberfläche: eine Ansage spielt jetzt.
pub(crate) const EREIGNIS_ANSAGE: &str = "kabine-ansage";

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
    /// Sprachen: keine (nur Englisch) | airline (Airline + Englisch) |
    /// airline_de (Airline + Deutsch, Thomas 17.10.2026) | ziel | eigene.
    pub zweite_sprache: String,
    /// Für „eigene“: BCP-47, z. B. "de-DE".
    pub eigene_sprache: Option<String>,
    /// Anbieter je Rolle: "piper" (kostenlos), "kokoro" (kostenlos, nur
    /// Englisch, sonst Piper) oder "azure" (eigener Schlüssel).
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
    /// Ausgabegeraet (`cpal::DeviceId` als Text). `None` = Systemstandard.
    /// Fehlt das Geraet (abgezogen), spielt der Systemstandard.
    pub ausgabe: Option<String>,
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
            ausgabe: None,
        }
    }
}

const TAGESFORMEN: [&str; 6] = ["zufall", "sachlich", "herzlich", "charmant", "witzig", "rustikal"];

impl Einstellungen {
    /// Unbekannte Werte auf den Standard, Zahlen in ihren Bereich.
    pub(crate) fn normalisiert(mut self) -> Self {
        let std = Einstellungen::default();
        if !["keine", "airline", "airline_de", "ziel", "eigene"].contains(&self.zweite_sprache.as_str()) {
            self.zweite_sprache = std.zweite_sprache.clone();
        }
        self.eigene_sprache = self.eigene_sprache.filter(|s| ist_locale(s));
        for a in [&mut self.anbieter_kapitaen, &mut self.anbieter_kabine, &mut self.anbieter_kommando] {
            if a != "piper" && a != "kokoro" && a != "azure" {
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
        self.ausgabe = self
            .ausgabe
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty() && a.len() <= 512);
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
}

#[tauri::command]
pub(crate) async fn kabinen_zustand(
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
    /// Ton gerade nicht erzeugbar (Server ausgelastet, Netz): Versuche und
    /// frühester nächster Versuch. Anders als `ton = None` (keine Stimme für
    /// die Sprache) ist das vorübergehend.
    ton_fehler: HashMap<(Slot, String), (u8, i64)>,
    /// Fehler je noch fehlendem Text-Auftrag / Ton, fuer die Begruendung
    /// auf der Seite „Kabine“ (Thomas: „sagen warum, nicht leere Seite“).
    /// Je Slot, nicht je Auftrag: Ein Ereignis oder eine Nachholung bekommt
    /// einen neuen Auftragsnamen, der Slot bleibt derselbe (Codex-QS beta.2).
    texte_fehler: std::collections::BTreeMap<Slot, String>,
    ton_fehler_text: std::collections::BTreeMap<(Slot, String), String>,
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

/// Braucht dieser Text (noch) Ton? Nicht schon da, nicht in Arbeit, und
/// nach einem Fehler höchstens fünfmal mit einer Minute Abstand.
fn ton_noetig(lauf: &Lauf, k: &(Slot, String), jetzt: i64) -> bool {
    !lauf.ton.contains_key(k)
        && !lauf.in_arbeit.contains(k)
        && lauf.ton_fehler.get(k).is_none_or(|(n, ab)| *n < 5 && jetzt >= *ab)
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
            .filter(|(s, sp, _)| ton_noetig(&stats.kabinen_lauf, &(*s, sp.clone()), jetzt))
            .collect();
        if !offen.is_empty() {
            for (s, sp, _) in &offen {
                stats.kabinen_lauf.in_arbeit.insert((*s, sp.clone()));
            }
            sprechen_lassen(app.clone(), Arc::clone(flight), offen);
        }
    }

    // ---- Ton nachholen, der vorübergehend nicht ging ----
    {
        let lauf = &mut stats.kabinen_lauf;
        let nachholen: Vec<(Slot, String, String)> = lauf
            .texte
            .iter()
            .filter(|(k, _)| lauf.ton_fehler.contains_key(*k) && ton_noetig(lauf, k, jetzt))
            .map(|((s, sp), t)| (*s, sp.clone(), t.clone()))
            .collect();
        if !nachholen.is_empty() {
            for (s, sp, _) in &nachholen {
                lauf.in_arbeit.insert((*s, sp.clone()));
            }
            sprechen_lassen(app.clone(), Arc::clone(flight), nachholen);
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
                        lauf.texte_fehler.remove(&slot);
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
                    let meldung = kurz(&e.to_string());
                    for s in &slots {
                        lauf.texte_fehler.insert(*s, meldung.clone());
                    }
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
fn sprechen_lassen(app: AppHandle, flight: Arc<ActiveFlight>, mut auftraege: Vec<(Slot, String, String)>) {
    // Was im Flug zuerst dran ist, zuerst: Boarding nicht hinter Reiseflug80
    // warten lassen (der Server-Piper arbeitet eins nach dem anderen).
    auftraege.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    tauri::async_runtime::spawn(async move {
        let einst = einstellungen(&app);
        for (slot, sprache, text) in auftraege {
            let ergebnis = sprechen(&app, &einst, slot.rolle(), &sprache, &text).await;
            let mut stats = flight.stats.lock().expect("flight stats");
            let lauf = &mut stats.kabinen_lauf;
            let k = (slot, sprache);
            lauf.in_arbeit.remove(&k);
            match ergebnis {
                Ok(ton) => {
                    lauf.ton_fehler.remove(&k);
                    lauf.ton_fehler_text.remove(&k);
                    lauf.ton.insert(k, ton);
                }
                Err(e) => {
                    // Vorübergehend: in einer Minute noch einmal (takt).
                    tracing::warn!(slot = ?k.0, sprache = %k.1, error = %e, "kabinenansage: Ton gerade nicht erzeugbar");
                    lauf.ton_fehler_text.insert(k.clone(), kurz(&e));
                    let n = lauf.ton_fehler.get(&k).map_or(0, |(n, _)| *n) + 1;
                    lauf.ton_fehler.insert(k, (n, Utc::now().timestamp() + 60));
                }
            }
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
    // Kokoro kann nur Englisch: Deutsch u. a. faellt auf Piper zurueck.
    let reihe: &[&str] = match einst.anbieter(rolle) {
        "azure" => &["azure", "piper"],
        "kokoro" => &["kokoro", "piper", "azure"],
        _ => &["piper", "azure"],
    };
    // Ein Fehler eines Anbieters (Netz, Server) beendet die Reihe nicht: der
    // naechste darf es versuchen (Codex-QS beta.3). Klappt keiner, kommt der
    // letzte Fehler zurueck — dann versucht es der Takt spaeter wieder.
    let mut letzter_fehler: Option<String> = None;
    for anbieter in reihe {
        match *anbieter {
            "azure" => {
                let (Some(key), Some(stimme)) = (schluessel.as_deref(), azure_stimme(sprache, maennlich(rolle))) else {
                    continue;
                };
                let name = cache::dateiname("azure", stimme, sprache, text, "wav");
                let pfad = ordner.join(&name);
                if !pfad.exists() {
                    let wav = match azure_sprechen(key, &einst.azure_region, stimme, text).await {
                        Ok(w) => w,
                        Err(e) => {
                            letzter_fehler = Some(e);
                            continue;
                        }
                    };
                    std::fs::write(&pfad, &wav).map_err(|e| e.to_string())?;
                }
                return ton_aus_datei(&pfad, name).map(Some);
            }
            "kokoro" => {
                let Some(stimme) = piper::kokoro_fuer(sprache, maennlich(rolle)) else { continue };
                let name = cache::dateiname("kokoro", stimme, sprache, text, "wav");
                let pfad = ordner.join(&name);
                if !pfad.exists() {
                    // Kokoro ist ein Zusatz: Klappt es gerade nicht, spricht
                    // Piper, statt dass die Ansage ausfaellt (Codex-QS).
                    let wav = match piper::sprechen(stimme, text).await {
                        Ok(Some(w)) => w,
                        Ok(None) => continue,
                        Err(e) => {
                            tracing::info!(error = %e, "kabinenansagen: Kokoro gerade nicht, Piper spricht");
                            continue;
                        }
                    };
                    std::fs::write(&pfad, &wav).map_err(|e| e.to_string())?;
                }
                return ton_aus_datei(&pfad, name).map(Some);
            }
            _ => {
                let Some(stimme) = piper::stimme_fuer(sprache, maennlich(rolle)) else { continue };
                let name = cache::dateiname("piper", &stimme, sprache, text, "wav");
                let pfad = ordner.join(&name);
                if !pfad.exists() {
                    // Keine Stimme fuer die Sprache auf dem Server: weiter
                    // zum naechsten Anbieter (Azure), sonst entfaellt sie.
                    let wav = match piper::sprechen(&stimme, text).await {
                        Ok(Some(w)) => w,
                        Ok(None) => continue,
                        Err(e) => {
                            letzter_fehler = Some(e);
                            continue;
                        }
                    };
                    std::fs::write(&pfad, &wav).map_err(|e| e.to_string())?;
                }
                return ton_aus_datei(&pfad, name).map(Some);
            }
        }
    }
    match letzter_fehler {
        Some(e) => Err(e),
        None => Ok(None),
    }
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
    Ok(tauri::ipc::Response::new(ton_datei_lesen(&app, &datei)?))
}

fn ton_datei_lesen(app: &AppHandle, datei: &str) -> Result<Vec<u8>, UiError> {
    let sauber = !datei.is_empty()
        && datei.ends_with(".wav")
        && datei.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !datei.contains("..");
    if !sauber {
        return Err(UiError::new("ungueltig", "invalid file name"));
    }
    let pfad = ton_ordner(app).map_err(|e| UiError::new("ordner", e))?.join(datei);
    std::fs::read(pfad).map_err(|e| UiError::new("lesen", e.to_string()))
}

/// Ausgabegeraete fuer die Auswahl in den Einstellungen.
#[tauri::command]
pub(crate) async fn kabinen_ausgabegeraete() -> Result<Vec<ausgabe::Geraet>, UiError> {
    tauri::async_runtime::spawn_blocking(ausgabe::geraete)
        .await
        .map_err(|e| UiError::new("ausgabe", e.to_string()))?
        .map_err(|e| UiError::new("ausgabe", e))
}

/// Spielt eine Tondatei — oder mit `datei = None` den Doppel-Chime — auf
/// dem eingestellten Geraet. Kehrt zurueck, wenn sie zu Ende ist oder
/// `kabinen_ton_stoppen` kam.
#[tauri::command]
pub(crate) async fn kabinen_ton_spielen(app: AppHandle, datei: Option<String>, lautstaerke: f32) -> Result<(), UiError> {
    let stand = ausgabe::stopp_stand();
    let (rate, proben) = match datei {
        Some(d) => ausgabe::wav_lesen(&ton_datei_lesen(&app, &d)?)
            .ok_or_else(|| UiError::new("format", "unsupported wav"))?,
        None => (ausgabe::CHIME_RATE, ausgabe::chime(ausgabe::CHIME_RATE)),
    };
    let geraet = einstellungen(&app).ausgabe;
    tauri::async_runtime::spawn_blocking(move || ausgabe::spielen(geraet.as_deref(), rate, &proben, lautstaerke, stand))
        .await
        .map_err(|e| UiError::new("ausgabe", e.to_string()))?
        .map_err(|e| UiError::new("ausgabe", e))
}

/// Laufende Wiedergabe sofort beenden („Ansagen ausschalten“).
#[tauri::command]
pub(crate) fn kabinen_ton_stoppen() {
    ausgabe::stoppen();
}

/// Pegel einer Tondatei fuer die Anzeige, `ausgabe::HUELLE_JE_S` Werte je Sekunde.
#[tauri::command]
pub(crate) fn kabinen_ton_huellkurve(app: AppHandle, datei: String) -> Result<Vec<f32>, UiError> {
    let (rate, proben) = ausgabe::wav_lesen(&ton_datei_lesen(&app, &datei)?)
        .ok_or_else(|| UiError::new("format", "unsupported wav"))?;
    Ok(ausgabe::huellkurve(rate, &proben))
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
    /// Warum gerade keine Ansagen kommen; `None` = alles bereit.
    grund: Option<Grund>,
}

/// Warum keine Ansagen kommen — die Seite zeigt es oben statt einer leeren
/// Seite (Thomas 17.10.2026). Bewusst nicht ganz die Reihenfolge von
/// [`takt`]: Ein Frachtflug wird vor der Beta-Pruefung genannt, weil er
/// endgueltig ist (auch nach der Freischaltung kaeme nichts). Pause/Slew
/// fehlen absichtlich — dass im angehaltenen Sim nichts spricht, sieht der
/// Pilot selbst.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "grund", content = "detail", rename_all = "snake_case")]
pub(crate) enum Grund {
    Aus,
    Replay,
    Fracht,
    KeinePax,
    KeinePassagiere,
    BetaPruefung,
    NichtFreigeschaltet,
    TexteFehlen(String),
    TonFehlt(String),
}

pub(crate) fn grund(an: bool, stats: &FlightStats) -> Option<Grund> {
    if !an {
        return Some(Grund::Aus);
    }
    if stats.replay_verdacht {
        return Some(Grund::Replay);
    }
    // Flugart allein (Fracht, Positionierung …), unabhaengig von den Pax.
    if cabin_announcements::flugart::einordnen(stats.flight_type.as_deref(), Some(1), None, 0) == Flugart::Aus {
        return Some(Grund::Fracht);
    }
    match stats.pax_count {
        None => return Some(Grund::KeinePax),
        Some(n) if n <= 0 => return Some(Grund::KeinePassagiere),
        Some(_) => {}
    }
    let lauf = &stats.kabinen_lauf;
    match lauf.beta {
        Beta::Nein => return Some(Grund::NichtFreigeschaltet),
        Beta::Unbekannt | Beta::Angefragt => return Some(Grund::BetaPruefung),
        Beta::Ja => {}
    }
    if let Some(f) = lauf.texte_fehler.values().next() {
        return Some(Grund::TexteFehlen(f.clone()));
    }
    lauf.ton_fehler_text.values().next().cloned().map(Grund::TonFehlt)
}

/// Fehlertext fuer die Anzeige: eine Zeile, hoechstens 160 Zeichen.
fn kurz(text: &str) -> String {
    let zeile = text.lines().next().unwrap_or("").trim();
    zeile.chars().take(160).collect()
}

#[tauri::command]
pub(crate) fn kabinen_ueberblick(app: AppHandle, state: tauri::State<'_, AppState>) -> Option<Ueberblick> {
    let flight = state.active_flight.lock().expect("active_flight lock").as_ref().map(Arc::clone)?;
    let stats = flight.stats.lock().expect("flight stats");
    let lauf = &stats.kabinen_lauf;
    Some(Ueberblick {
        grund: grund(einstellungen(&app).an, &stats),
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
// Ausgabe: Ton auf dem gewaehlten Geraet (Lautsprecher, Headset, …)
// ---------------------------------------------------------------------------

/// Wiedergabe in Rust statt im Fenster: Nur so laesst sich das Geraet
/// waehlen. Die Mac-Oberflaeche kann Ton nicht umleiten, und Windows nennt
/// die Geraete im Fenster erst nach einer Mikrofon-Freigabe (Thomas
/// 17.10.2026: „Lautsprecher, Headset … alles was angeschlossen ist“).
pub(crate) mod ausgabe {
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Abtastrate, in der der Chime erzeugt wird (vor dem Umrechnen).
    pub(crate) const CHIME_RATE: u32 = 48_000;
    /// Pegelwerte je Sekunde fuer die Anzeige.
    pub(crate) const HUELLE_JE_S: u32 = 30;

    /// Steigt bei jedem Stopp. Eine Wiedergabe merkt sich den Stand beim
    /// Aufruf und endet, sobald er sich aendert — auch ein Stopp, der kommt,
    /// bevor das Geraet ueberhaupt geoeffnet ist, greift so.
    static STOPP: AtomicU64 = AtomicU64::new(0);

    pub(crate) fn stopp_stand() -> u64 {
        STOPP.load(Ordering::SeqCst)
    }
    pub(crate) fn stoppen() {
        STOPP.fetch_add(1, Ordering::SeqCst);
    }

    /// Ein Ausgabegeraet fuer die Auswahl in den Einstellungen.
    #[derive(Debug, Clone, serde::Serialize)]
    pub(crate) struct Geraet {
        pub id: String,
        pub name: String,
    }

    /// Mono-Proben (−1 … 1) und Abtastrate aus einer PCM-16-WAV (Piper und
    /// Azure liefern beide genau das). Mehrkanalig wird gemittelt.
    pub(crate) fn wav_lesen(d: &[u8]) -> Option<(u32, Vec<f32>)> {
        if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
            return None;
        }
        let mut i = 12;
        // (Format, Kanaele, Rate, Bits)
        let mut fmt: Option<(u16, u16, u32, u16)> = None;
        while i + 8 <= d.len() {
            let id = &d[i..i + 4];
            let groesse = u32::from_le_bytes([d[i + 4], d[i + 5], d[i + 6], d[i + 7]]) as usize;
            let inhalt = i + 8;
            if id == b"fmt " && inhalt + 16 <= d.len() {
                let zwei = |o: usize| u16::from_le_bytes([d[inhalt + o], d[inhalt + o + 1]]);
                let rate = u32::from_le_bytes([d[inhalt + 4], d[inhalt + 5], d[inhalt + 6], d[inhalt + 7]]);
                fmt = Some((zwei(0), zwei(2), rate, zwei(14)));
            }
            if id == b"data" {
                let (format, kanaele, rate, bits) = fmt?;
                // 1 = PCM, 0xFFFE = WAVE_FORMAT_EXTENSIBLE (ebenfalls PCM bei 16 Bit)
                if !(format == 1 || format == 0xFFFE) || bits != 16 || kanaele == 0 || rate == 0 {
                    return None;
                }
                // Manche Kodierer schreiben 0xFFFFFFFF; dann zaehlt, was da ist.
                let ende = inhalt + groesse.min(d.len().saturating_sub(inhalt));
                let k = usize::from(kanaele);
                let proben = d[inhalt..ende]
                    .chunks_exact(2 * k)
                    .map(|rahmen| {
                        let summe: f32 = rahmen
                            .chunks_exact(2)
                            .map(|b| f32::from(i16::from_le_bytes([b[0], b[1]])) / 32768.0)
                            .sum();
                        summe / k as f32
                    })
                    .collect();
                return Some((rate, proben));
            }
            i = inhalt + groesse + (groesse & 1);
        }
        None
    }

    /// Linear auf eine andere Abtastrate; fuer Sprache genuegt das.
    #[cfg_attr(not(any(target_os = "windows", target_os = "macos")), allow(dead_code))]
    pub(crate) fn umrechnen(p: &[f32], von: u32, nach: u32) -> Vec<f32> {
        if von == nach || p.is_empty() || von == 0 || nach == 0 {
            return p.to_vec();
        }
        let n = (p.len() as u64 * u64::from(nach) / u64::from(von)) as usize;
        let schritt = f64::from(von) / f64::from(nach);
        let letzte = p.len() - 1;
        (0..n)
            .map(|j| {
                let x = j as f64 * schritt;
                let i = (x as usize).min(letzte);
                let t = (x - i as f64) as f32;
                let a = p[i];
                let b = p[(i + 1).min(letzte)];
                a + (b - a) * t
            })
            .collect()
    }

    /// Doppel-Chime wie bisher im Fenster: 880 Hz, nach 0,55 s 698,5 Hz; je
    /// 20 ms Anstieg auf 0,35, dann exponentiell auf 0,001 bei 1,4 s, Ende
    /// bei 1,5 s. Gesamt 2,1 s (mit Ausklang).
    pub(crate) fn chime(rate: u32) -> Vec<f32> {
        let n = (2.1 * f64::from(rate)) as usize;
        let ton = |t: f64, f: f64| -> f64 {
            if !(0.0..1.5).contains(&t) {
                return 0.0;
            }
            let huelle = if t < 0.02 {
                0.35 * t / 0.02
            } else {
                0.35 * (0.001_f64 / 0.35).powf((t - 0.02) / (1.4 - 0.02))
            };
            huelle * (2.0 * std::f64::consts::PI * f * t).sin()
        };
        (0..n)
            .map(|j| {
                let t = j as f64 / f64::from(rate);
                (ton(t, 880.0) + ton(t - 0.55, 698.5)) as f32
            })
            .collect()
    }

    /// Pegel fuer die Anzeige: RMS je 1/30 s, auf 0 … 1 gestreckt (Sprache
    /// liegt bei RMS ~0,05–0,3) — dieselbe Skala wie bisher im Fenster.
    pub(crate) fn huellkurve(rate: u32, p: &[f32]) -> Vec<f32> {
        let fenster = (rate / HUELLE_JE_S).max(1) as usize;
        p.chunks(fenster)
            .map(|c| {
                let rms = (c.iter().map(|x| x * x).sum::<f32>() / c.len() as f32).sqrt();
                (rms * 4.0).min(1.0)
            })
            .collect()
    }

    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(crate) fn geraete() -> Result<Vec<Geraet>, String> {
        use cpal::traits::{DeviceTrait, HostTrait};
        let liste = cpal::default_host().output_devices().map_err(|e| e.to_string())?;
        Ok(liste
            .filter_map(|g| {
                Some(Geraet {
                    id: g.id().ok()?.to_string(),
                    name: g.description().ok()?.name().to_string(),
                })
            })
            .collect())
    }

    /// Spielt die Proben auf dem gewaehlten Geraet (sonst Systemstandard)
    /// und kehrt zurueck, wenn sie gespielt sind oder gestoppt wurde.
    /// Blockiert: nur aus `spawn_blocking` rufen — der cpal-Stream ist
    /// nicht `Send` und lebt nur in diesem Faden.
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    pub(crate) fn spielen(geraet: Option<&str>, rate: u32, proben: &[f32], lautstaerke: f32, stand: u64) -> Result<(), String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        use std::sync::Arc;

        if STOPP.load(Ordering::SeqCst) != stand {
            return Ok(());
        }
        let host = cpal::default_host();
        let gewaehlt = geraet
            .and_then(|id| id.parse::<cpal::DeviceId>().ok())
            .and_then(|id| host.device_by_id(&id));
        if geraet.is_some() && gewaehlt.is_none() {
            tracing::info!("kabinenansagen: gewaehltes Ausgabegeraet fehlt, Systemstandard");
        }
        let dev = gewaehlt
            .or_else(|| host.default_output_device())
            .ok_or("kein Ausgabegeraet")?;
        let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
        let kanaele = usize::from(cfg.channels()).max(1);
        let ziel = cfg.sample_rate();
        let v = if lautstaerke.is_finite() { lautstaerke.clamp(0.0, 1.0) } else { 0.8 };
        let mut daten: Vec<f32> = umrechnen(proben, rate, ziel).into_iter().map(|x| (x * v).clamp(-1.0, 1.0)).collect();
        // Nachlauf: Ist die letzte Probe uebergeben, spielt das Geraet seinen
        // Puffer noch — ohne Stille am Ende fehlte der Schluss.
        daten.resize(daten.len() + (ziel / 5) as usize, 0.0);
        let gesamt = daten.len();
        let daten = Arc::new(daten);
        let pos = Arc::new(AtomicUsize::new(0));
        let fehler = Arc::new(AtomicBool::new(false));
        // Was noch im Geraet steckt, wenn die letzte Probe uebergeben ist:
        // Abstand Rueckruf → Wiedergabe plus Puffer, groesster Wert (µs).
        let latenz = Arc::new(std::sync::atomic::AtomicU64::new(0));

        #[allow(clippy::too_many_arguments)]
        fn bauen<T: cpal::SizedSample + cpal::FromSample<f32>>(
            dev: &cpal::Device,
            cfg: cpal::StreamConfig,
            kanaele: usize,
            daten: Arc<Vec<f32>>,
            pos: Arc<AtomicUsize>,
            fehler: Arc<AtomicBool>,
            latenz: Arc<std::sync::atomic::AtomicU64>,
        ) -> Result<cpal::Stream, String> {
            let rate = f64::from(cfg.sample_rate.max(1));
            dev.build_output_stream(
                cfg,
                move |puffer: &mut [T], info: &cpal::OutputCallbackInfo| {
                    let ts = info.timestamp();
                    let puffer_s = (puffer.len() / kanaele) as f64 / rate;
                    let us = ts.playback.duration_since(ts.callback).as_micros() as u64 + (puffer_s * 1e6) as u64;
                    latenz.fetch_max(us, Ordering::Relaxed);
                    let mut i = pos.load(Ordering::Relaxed);
                    for rahmen in puffer.chunks_mut(kanaele) {
                        let x = daten.get(i).copied().unwrap_or(0.0);
                        i += 1;
                        for k in rahmen {
                            *k = T::from_sample(x);
                        }
                    }
                    pos.store(i, Ordering::Relaxed);
                },
                move |e| {
                    tracing::warn!(error = %e, "kabinenansagen: Ausgabegeraet meldet einen Fehler");
                    fehler.store(true, Ordering::Relaxed);
                },
                None,
            )
            .map_err(|e| e.to_string())
        }

        let (d, p, f, l, c) = (Arc::clone(&daten), Arc::clone(&pos), Arc::clone(&fehler), Arc::clone(&latenz), cfg.config());
        let stream = match cfg.sample_format() {
            cpal::SampleFormat::F32 => bauen::<f32>(&dev, c, kanaele, d, p, f, l),
            cpal::SampleFormat::I16 => bauen::<i16>(&dev, c, kanaele, d, p, f, l),
            cpal::SampleFormat::U16 => bauen::<u16>(&dev, c, kanaele, d, p, f, l),
            cpal::SampleFormat::I32 => bauen::<i32>(&dev, c, kanaele, d, p, f, l),
            andere => return Err(format!("Tonformat {andere:?} nicht unterstuetzt")),
        }?;
        stream.play().map_err(|e| e.to_string())?;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(20));
            if STOPP.load(Ordering::SeqCst) != stand {
                break;
            }
            if fehler.load(Ordering::Relaxed) {
                return Err("Ausgabegeraet meldet einen Fehler".into());
            }
            if pos.load(Ordering::Relaxed) >= gesamt {
                // Alles uebergeben; was noch im Geraet steckt (Bluetooth
                // kann deutlich ueber 200 ms liegen), ausspielen lassen —
                // hoechstens 2 s, ein Stopp beendet sofort.
                let rest = std::time::Duration::from_micros(latenz.load(Ordering::Relaxed).min(2_000_000));
                let bis = std::time::Instant::now() + rest;
                while std::time::Instant::now() < bis && STOPP.load(Ordering::SeqCst) == stand {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                break;
            }
        }
        drop(stream);
        Ok(())
    }

    /// Linux: Der Client laeuft dort nicht, nur der CI-Check.
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(crate) fn geraete() -> Result<Vec<Geraet>, String> {
        Err("nur unter Windows und macOS".into())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    pub(crate) fn spielen(_: Option<&str>, _: u32, _: &[f32], _: f32, _: u64) -> Result<(), String> {
        Err("nur unter Windows und macOS".into())
    }
}

// ---------------------------------------------------------------------------
// Piper (kostenlos) — läuft auf dem GSG-Server live.kant.ovh
// ---------------------------------------------------------------------------

/// Piper rechnet auf dem Server (Entscheidung Thomas 17.10.2026): Ein
/// fertiges Piper-Programm gibt es nur für Windows, auf dem Server läuft es
/// für Windows und Mac, und der Pilot lädt nichts herunter. Piper (GPL)
/// wird so nicht weitergegeben.
pub(crate) mod piper {
    /// Die vier Stimmen der Beta — dieselben Namen wie auf dem Server
    /// (`recorder/src/kabinenTon.ts`, STIMMEN).
    pub(crate) const STIMMEN: [&str; 4] = [
        "de_DE-thorsten-high",
        "de_DE-kerstin-low",
        "en_GB-northern_english_male-medium",
        "en_GB-jenny_dioco-medium",
    ];

    /// Deutsch (auch de-CH/de-AT, Hochdeutsch) und Englisch mit festem Namen
    /// wie bisher. Jede andere Sprache fragt `auto:<sprache>:<m|w>` an: Der
    /// Server waehlt die Stimme (`SPRACH_STIMMEN`), so kommen neue Sprachen
    /// ohne neue Client-Beta dazu (Thomas 17.10.2026). Hat er keine, liefert
    /// [`sprechen`] `Ok(None)` und die Sprache entfaellt.
    pub(crate) fn stimme_fuer(sprache: &str, m: bool) -> Option<String> {
        Some(match (sprache.split('-').next()?, m) {
            ("de", true) => STIMMEN[0].to_string(),
            ("de", false) => STIMMEN[1].to_string(),
            ("en", true) => STIMMEN[2].to_string(),
            ("en", false) => STIMMEN[3].to_string(),
            _ if super::ist_locale(sprache) => format!("auto:{sprache}:{}", if m { "m" } else { "w" }),
            _ => return None,
        })
    }

    /// `{"error":"keine_stimme"}` vom Server (recorder/src/kabinenTon.ts).
    pub(crate) fn keine_stimme(koerper: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(koerper)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|e| e == "keine_stimme"))
            .unwrap_or(false)
    }

    /// Kokoro-82M auf demselben Server (Thomas 17.10.2026: zusaetzliche Wahl
    /// neben Piper). Nur Englisch, britische Stimmen; Namen wie im Recorder
    /// (`KOKORO_STIMMEN`).
    pub(crate) fn kokoro_fuer(sprache: &str, m: bool) -> Option<&'static str> {
        (sprache.split('-').next()? == "en").then_some(if m { "kokoro-bm_george" } else { "kokoro-bf_emma" })
    }

    /// Ton vom Server (Piper oder Kokoro, je nach Stimmenname). `Ok(None)`:
    /// Der Server hat fuer diese Sprache keine Stimme (404). `Err` heißt „gerade nicht“ (kein Token, Netz,
    /// ausgelastet) — der Aufrufer versucht es später wieder.
    pub(crate) async fn sprechen(stimme: &str, text: &str) -> Result<Option<Vec<u8>>, String> {
        let token = crate::live_zugang::pilot_token().ok_or("noch kein Live-Zugang (nicht provisioniert)")?;
        let antwort = reqwest::Client::builder()
            // Lange Ansagen mit Kokoro rechnen bis ~2 min, dazu die
            // Warteschlange des Servers.
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| e.to_string())?
            .post(format!("{}/api/kabine/ton", crate::live_zugang::LIVE_BASIS))
            .bearer_auth(token)
            .header("User-Agent", format!("AeroACARS/{}", env!("CARGO_PKG_VERSION")))
            .json(&serde_json::json!({ "stimme": stimme, "text": text }))
            .send()
            .await
            .map_err(|e| format!("live.kant.ovh nicht erreichbar: {e}"))?;
        // Nur die ausdrueckliche Antwort „keine Stimme fuer diese Sprache“ ist
        // endgueltig; jeder andere 404 (Route fehlt, Proxy) ist vorlaeufig.
        if antwort.status() == reqwest::StatusCode::NOT_FOUND {
            let koerper = antwort.text().await.unwrap_or_default();
            return if keine_stimme(&koerper) {
                Ok(None)
            } else {
                Err("Piper-Server antwortet 404".into())
            };
        }
        if !antwort.status().is_success() {
            return Err(format!("Piper-Server antwortet {}", antwort.status()));
        }
        antwort.bytes().await.map(|b| Some(b.to_vec())).map_err(|e| e.to_string())
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
        let de = Einstellungen { zweite_sprache: "airline_de".into(), ..Einstellungen::default() }.normalisiert();
        assert_eq!(de.zweite_sprache, "airline_de", "Airline + Deutsch bleibt erhalten");
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
        assert_eq!(piper::stimme_fuer("de-CH", true).as_deref(), Some("de_DE-thorsten-high"));
        assert_eq!(piper::stimme_fuer("en-US", false).as_deref(), Some("en_GB-jenny_dioco-medium"));
        // Andere Sprachen: Der Server waehlt die Stimme dieser Sprache.
        assert_eq!(piper::stimme_fuer("fr-FR", true).as_deref(), Some("auto:fr-FR:m"));
        assert_eq!(piper::stimme_fuer("lv-LV", false).as_deref(), Some("auto:lv-LV:w"));
        assert_eq!(piper::stimme_fuer("frFR", true), None, "nur gueltige Sprachkennung");
        // Nur die ausdrueckliche Server-Antwort ist „keine Stimme“.
        assert!(piper::keine_stimme(r#"{"error":"keine_stimme"}"#));
        assert!(!piper::keine_stimme("<html>Not Found</html>"));
        assert!(!piper::keine_stimme(r#"{"error":"stimme_unbekannt"}"#));
        assert_eq!(azure_stimme("de-AT", false), Some("de-AT-IngridNeural"));
        assert_eq!(azure_stimme("de-CH", true), Some("de-CH-JanNeural"));
        assert_eq!(azure_stimme("xx-XX", true), None);
        for sp in ["de-DE", "de-CH", "de-AT", "en-GB", "fr-FR", "ar-AE", "lv-LV", "nl-NL", "it-IT"] {
            for m in [true, false] {
                let s = azure_stimme(sp, m).unwrap();
                assert_eq!(ssml::locale_aus_stimme(s), Some(sp), "{s}");
            }
        }
        // Dieselben vier Namen wie auf dem Server (kabinenTon.ts STIMMEN);
        // ein anderer Name gäbe dort 400.
        assert_eq!(
            piper::STIMMEN,
            ["de_DE-thorsten-high", "de_DE-kerstin-low", "en_GB-northern_english_male-medium", "en_GB-jenny_dioco-medium"]
        );
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

    /// Echter Azure-Aufruf über denselben Weg wie im Flug (SSML, WAV-Format,
    /// Dauer aus dem Kopf). Braucht einen Schlüssel, deshalb zuschaltbar:
    ///
    ///     KABINEN_AZURE_DATEI=… KABINEN_AZURE_REGION=germanywestcentral \
    ///     KABINEN_PROBEN_ORDNER=… cargo test --lib azure_hoerproben -- --ignored
    ///
    /// Die Datei: Zeile nach „SCHLÜSSEL 1“ ist der Schlüssel. Er wird nie
    /// ausgegeben.
    #[tokio::test]
    #[ignore = "braucht einen Azure-Schlüssel"]
    async fn azure_hoerproben() {
        let datei = std::env::var("KABINEN_AZURE_DATEI").expect("KABINEN_AZURE_DATEI");
        let region = std::env::var("KABINEN_AZURE_REGION").expect("KABINEN_AZURE_REGION");
        let ordner = std::path::PathBuf::from(std::env::var("KABINEN_PROBEN_ORDNER").expect("KABINEN_PROBEN_ORDNER"));
        std::fs::create_dir_all(&ordner).unwrap();
        let text = std::fs::read_to_string(datei).unwrap();
        let zeilen: Vec<&str> = text.lines().map(str::trim).filter(|z| !z.is_empty()).collect();
        let pos = zeilen.iter().position(|z| z.to_uppercase().starts_with("SCHL")).expect("Schlüsselzeile");
        let key = zeilen[pos + 1];
        let proben = [
            ("de-DE", true, "Guten Abend, liebe Gäste, hier spricht Ihr Kapitän Markus Brandt. Wir werden etwa eine Stunde und fünfundfünfzig Minuten unterwegs sein. In Toulouse erwartet uns leichter Regen bei zwölf Grad."),
            ("de-DE", false, "Liebe Gäste, willkommen in Toulouse. Es ist zwanzig Uhr einunddreißig Ortszeit. Bitte bleiben Sie angeschnallt, bis wir unsere Parkposition erreicht haben."),
            ("de-CH", false, "Grüezi und herzlich willkommen an Bord von Swiss. Wir fliegen heute nach Toulouse, die Flugzeit beträgt eine Stunde und zehn Minuten. Merci vielmal."),
            ("de-CH", true, "Liebe Gäste, hier spricht Ihr Kapitän. Wir beginnen in wenigen Minuten mit dem Sinkflug nach Zürich."),
            ("de-AT", false, "Servus und herzlich willkommen an Bord. Bitte verstauen Sie Ihr Handgepäck und nehmen Sie Platz."),
            ("de-AT", true, "Liebe Gäste, hier spricht Ihr Kapitän. In Wien erwarten uns sonnige achtzehn Grad."),
            ("en-GB", true, "Good evening, ladies and gentlemen, this is your captain speaking. Flight time to Toulouse is one hour and fifty-five minutes."),
            ("en-GB", false, "Ladies and gentlemen, welcome to Toulouse, where the local time is eight thirty-one in the evening."),
        ];
        for (sprache, m, satz) in proben {
            let stimme = azure_stimme(sprache, m).unwrap();
            let wav = azure_sprechen(key, &region, stimme, satz).await.unwrap_or_else(|e| panic!("{stimme}: {e}"));
            let dauer = wav_dauer_s(&wav).unwrap_or_else(|| panic!("{stimme}: kein WAV"));
            assert!(dauer > 2.0, "{stimme}: nur {dauer} s");
            std::fs::write(ordner.join(format!("{stimme}.wav")), &wav).unwrap();
            println!("{stimme}: {dauer:.1} s");
        }
        // Gegenprobe: falscher Schlüssel → Fehler, nicht stilles WAV.
        assert!(azure_sprechen("falsch", &region, "de-DE-KatjaNeural", "Test").await.is_err());
    }


    #[test]
    fn fehlgeschlagener_ton_wird_spaeter_neu_versucht() {
        let mut l = Lauf::default();
        let k = (Slot::Boarding, "de-DE".to_string());
        assert!(ton_noetig(&l, &k, 0));
        l.in_arbeit.insert(k.clone());
        assert!(!ton_noetig(&l, &k, 0), "läuft schon");
        l.in_arbeit.remove(&k);
        l.ton_fehler.insert(k.clone(), (1, 60));
        assert!(!ton_noetig(&l, &k, 30), "noch nicht wieder");
        assert!(ton_noetig(&l, &k, 60), "nach einer Minute erneut");
        l.ton_fehler.insert(k.clone(), (5, 0));
        assert!(!ton_noetig(&l, &k, 999), "nach fünf Fehlern Schluss");
        // Keine Stimme für die Sprache ist endgültig, kein Fehler.
        l.ton_fehler.clear();
        l.ton.insert(k.clone(), None);
        assert!(!ton_noetig(&l, &k, 999));
    }

    /// PCM-16-WAV mit echten Proben (Kanaele verschachtelt).
    fn pcm16(rate: u32, kanaele: u16, proben: &[i16]) -> Vec<u8> {
        let mut v = b"RIFF\0\0\0\0WAVE".to_vec();
        v.extend_from_slice(b"fmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&kanaele.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 2 * u32::from(kanaele)).to_le_bytes());
        v.extend_from_slice(&(2 * kanaele).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&((proben.len() * 2) as u32).to_le_bytes());
        for p in proben {
            v.extend_from_slice(&p.to_le_bytes());
        }
        v
    }

    #[test]
    fn wav_lesen_liefert_mono_proben() {
        let (rate, p) = ausgabe::wav_lesen(&pcm16(22_050, 1, &[16_384, -16_384, 0])).unwrap();
        assert_eq!(rate, 22_050);
        assert_eq!(p, vec![0.5, -0.5, 0.0]);
        // Stereo wird gemittelt: (0,5 + −0,5)/2 = 0, (0,5 + 0,5)/2 = 0,5.
        let (_, p) = ausgabe::wav_lesen(&pcm16(48_000, 2, &[16_384, -16_384, 16_384, 16_384])).unwrap();
        assert_eq!(p, vec![0.0, 0.5]);
        // Der alte Test-Kopf (mit LIST-Block und Fuellbyte) wird durchlaufen.
        assert_eq!(ausgabe::wav_lesen(&wav(44_100, 4)).map(|(r, p)| (r, p.len())), Some((22_050, 2)));
        // 8 Bit oder Unsinn: kein Ton statt Rauschen.
        let mut acht = pcm16(8_000, 1, &[0, 0]);
        acht[34] = 8;
        assert_eq!(ausgabe::wav_lesen(&acht), None);
        assert_eq!(ausgabe::wav_lesen(b"kein wav"), None);
    }

    #[test]
    fn umrechnen_haelt_laenge_und_verlauf() {
        let p: Vec<f32> = (0..22_050).map(|i| i as f32 / 22_050.0).collect();
        let q = ausgabe::umrechnen(&p, 22_050, 48_000);
        assert_eq!(q.len(), 48_000);
        // Rampe bleibt Rampe: Mitte bei ~0,5, steigend.
        assert!((q[24_000] - 0.5).abs() < 1e-3, "{}", q[24_000]);
        assert!(q.windows(2).all(|w| w[1] >= w[0]));
        // Gleiche Rate: unveraendert.
        assert_eq!(ausgabe::umrechnen(&p, 22_050, 22_050), p);
    }

    #[test]
    fn chime_wie_im_fenster() {
        let rate = ausgabe::CHIME_RATE;
        let c = ausgabe::chime(rate);
        assert_eq!(c.len(), (2.1 * f64::from(rate)) as usize);
        let spitze = |von: f64, bis: f64| {
            c[(von * f64::from(rate)) as usize..(bis * f64::from(rate)) as usize]
                .iter()
                .fold(0.0_f32, |m, x| m.max(x.abs()))
        };
        assert!(spitze(0.0, 0.3) > 0.3, "erster Ton hoerbar");
        assert!(spitze(0.6, 0.9) > 0.1, "zweiter Ton hoerbar");
        assert!(spitze(0.0, 2.1) <= 0.7, "nie uebersteuert");
        assert!(spitze(2.06, 2.1) < 1e-3, "Ausklang still");
    }

    #[test]
    fn huellkurve_folgt_dem_pegel() {
        let rate = 22_050;
        let sinus: Vec<f32> = (0..rate)
            .map(|i| 0.1 * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / rate as f32).sin())
            .collect();
        let h = ausgabe::huellkurve(rate, &sinus);
        assert_eq!(h.len(), 30, "30 je Sekunde (22.050 / 30 = 735 ohne Rest)");
        // RMS 0,1/√2 ≈ 0,071, gestreckt ×4 ≈ 0,28.
        assert!(h[..30].iter().all(|w| (w - 0.283).abs() < 0.02), "{h:?}");
        assert!(ausgabe::huellkurve(rate, &vec![0.0; 2205]).iter().all(|w| *w == 0.0));
    }

    #[test]
    fn grund_nennt_die_erste_sperre() {
        let mut st = FlightStats::new();
        assert_eq!(grund(false, &st), Some(Grund::Aus));
        assert_eq!(grund(true, &st), Some(Grund::KeinePax), "Fall Thomas: Wechsel mitten im Flug");
        st.flight_type = Some("F".into());
        assert_eq!(grund(true, &st), Some(Grund::Fracht));
        st.flight_type = Some("J".into());
        st.pax_count = Some(0);
        assert_eq!(grund(true, &st), Some(Grund::KeinePassagiere));
        st.pax_count = Some(150);
        assert_eq!(grund(true, &st), Some(Grund::BetaPruefung));
        st.kabinen_lauf.beta = Beta::Nein;
        assert_eq!(grund(true, &st), Some(Grund::NichtFreigeschaltet));
        st.kabinen_lauf.beta = Beta::Ja;
        assert_eq!(grund(true, &st), None, "alles bereit");
        st.kabinen_lauf.texte_fehler.insert(Slot::KapitaenSinkflug, kurz("api 402: budget\nzweite Zeile"));
        assert_eq!(grund(true, &st), Some(Grund::TexteFehlen("api 402: budget".into())));
        // Ein anderer Slot gelingt: Der fehlende bleibt genannt (Codex-QS).
        st.kabinen_lauf.texte_fehler.remove(&Slot::Boarding);
        st.kabinen_lauf.texte.insert((Slot::Boarding, "de-DE".into()), "Hallo".into());
        assert_eq!(grund(true, &st), Some(Grund::TexteFehlen("api 402: budget".into())));
        st.kabinen_lauf.texte_fehler.remove(&Slot::KapitaenSinkflug);
        assert_eq!(grund(true, &st), None);
        let k = (Slot::Sicherheit, "en-GB".to_string());
        st.kabinen_lauf.ton_fehler_text.insert(k.clone(), "ausgelastet".into());
        st.kabinen_lauf.ton_fehler_text.remove(&(Slot::Boarding, "de-DE".to_string()));
        assert_eq!(grund(true, &st), Some(Grund::TonFehlt("ausgelastet".into())));
        st.kabinen_lauf.ton_fehler_text.remove(&k);
        assert_eq!(grund(true, &st), None);
        st.kabinen_lauf.ton_fehler_text.insert(k, "ausgelastet".into());
        st.replay_verdacht = true;
        assert_eq!(grund(true, &st), Some(Grund::Replay));
        // So kommt es in der Oberflaeche an.
        assert_eq!(
            serde_json::to_value(Grund::TonFehlt("x".into())).unwrap(),
            serde_json::json!({ "grund": "ton_fehlt", "detail": "x" })
        );
        assert_eq!(serde_json::to_value(Grund::KeinePax).unwrap(), serde_json::json!({ "grund": "keine_pax" }));
    }

    #[test]
    fn ausgabegeraet_wird_normalisiert() {
        let mit = |a: &str| Einstellungen { ausgabe: Some(a.into()), ..Einstellungen::default() }.normalisiert().ausgabe;
        assert_eq!(mit("  CoreAudio:42  "), Some("CoreAudio:42".into()));
        assert_eq!(mit("   "), None);
        assert_eq!(mit(&"x".repeat(600)), None);
        // Alte Einstellungsdatei ohne das Feld: Systemstandard.
        let alt: Einstellungen = serde_json::from_str(r#"{"an":true}"#).unwrap();
        assert_eq!(alt.ausgabe, None);
    }

    #[test]
    fn kokoro_nur_englisch_und_als_anbieter_erlaubt() {
        assert_eq!(piper::kokoro_fuer("en-GB", true), Some("kokoro-bm_george"));
        assert_eq!(piper::kokoro_fuer("en-US", false), Some("kokoro-bf_emma"));
        // Deutsch: keine Kokoro-Stimme, also spricht Piper (Reihe kokoro → piper).
        assert_eq!(piper::kokoro_fuer("de-DE", true), None);
        assert_eq!(piper::stimme_fuer("de-DE", true).as_deref(), Some("de_DE-thorsten-high"));
        let e = Einstellungen { anbieter_kapitaen: "kokoro".into(), anbieter_kabine: "elevenlabs".into(), ..Einstellungen::default() }
            .normalisiert();
        assert_eq!(e.anbieter_kapitaen, "kokoro");
        assert_eq!(e.anbieter_kabine, "piper", "Unbekanntes faellt auf Piper");
        // Piper-Stimmen bleiben unveraendert (Kokoro ist Zusatz, kein Ersatz).
        assert_eq!(piper::stimme_fuer("en-GB", true).as_deref(), Some("en_GB-northern_english_male-medium"));
    }

    /// Echtes Geraet: listet die Ausgaben und spielt den Chime auf dem
    /// Systemstandard. Nur von Hand: `cargo test -p aeroacars-app --lib
    /// chime_auf_echtem_geraet -- --ignored --nocapture`.
    #[test]
    #[ignore = "braucht ein echtes Ausgabegeraet"]
    fn chime_auf_echtem_geraet() {
        let liste = ausgabe::geraete().expect("Geraete lesbar");
        for g in &liste {
            println!("Ausgabe: {} ({})", g.name, g.id);
        }
        assert!(!liste.is_empty());
        let rate = ausgabe::CHIME_RATE;
        let uhr = std::time::Instant::now();
        ausgabe::spielen(None, rate, &ausgabe::chime(rate), 0.6, ausgabe::stopp_stand()).expect("spielt");
        let dauer = uhr.elapsed().as_secs_f64();
        println!("gespielt in {dauer:.2} s");
        assert!(dauer >= 2.1, "nicht vorzeitig beendet: {dauer}");
        // Auf ein gewaehltes Geraet (das erste der Liste) und gleich gestoppt.
        let stand = ausgabe::stopp_stand();
        ausgabe::stoppen();
        let uhr = std::time::Instant::now();
        ausgabe::spielen(Some(&liste[0].id), rate, &ausgabe::chime(rate), 0.6, stand).expect("spielt");
        assert!(uhr.elapsed().as_secs_f64() < 0.5, "Stopp vor dem Start greift");
    }
}
