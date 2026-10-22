//! Kabinenansagen (Beta): Einstellungen, Freischaltung, Drehbuch vom
//! GSG-Server, Sprachausgabe (kostenlos auf dem GSG-Server: seit 2.1.0
//! Supertonic, eine Stimme je Rolle in allen Sprachen) und das Wiedergabe-Ereignis an
//! die Oberfläche. Azure ist seit beta.4 entfernt (Thomas 17.10.2026).
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

use api_client::Client;
use cabin_announcements::flugart::Flugart;
use cabin_announcements::{cache, Faellig, Rolle, Slot};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sim_core::{FlightPhase, SimSnapshot};
use tauri::{AppHandle, Emitter, Manager};

use crate::{ActiveFlight, AppState, FlightStats, UiError};

/// Schlüsselbund-Konto des frueheren Azure-Schlüssels (bis beta.3). Wird
/// beim ersten Laden der Einstellungen einmal geloescht, damit kein
/// Schlüssel liegen bleibt.
const ALTES_AZURE_KONTO: &str = "kabinenansagen_azure";
/// Ereignis an die Oberfläche: eine Ansage spielt jetzt.
pub(crate) const EREIGNIS_ANSAGE: &str = "kabine-ansage";
/// Stumm (Knopf oben auf der Seite „Kabine“): Was fällig wird, entfällt als
/// „stumm übersprungen“ und kommt nicht nach. Nicht gespeichert — nach einem
/// Neustart ist der Ton wieder an, sonst startete der nächste Flug stumm.
static STUMM: AtomicBool = AtomicBool::new(false);
/// Ohne Startmeldung der Oberfläche gilt eine Ansage nach so vielen Sekunden
/// als „Ausgang unbekannt“ (Ereignis verloren, Fenster weg).
const START_SPAETESTENS_S: i64 = 30;

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
    /// Sprachen: airline (Airline + Englisch) | airline_de (Airline +
    /// Deutsch). Alles andere — Stimme je Sprache, Tagesform — bestimmt der
    /// GSG-Server (Thomas 17.10.2026: „der Server bestimmt das jetzt alles“).
    /// Fruehere Felder (Anbieter je Rolle, Azure, Tagesform, eigene Sprache)
    /// werden beim Laden ignoriert.
    pub zweite_sprache: String,
    /// Echten Namen als Kapitän verwenden (geht an den GSG-Server und das Modell).
    pub echter_name: bool,
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
    /// Kapitän: auto | m | w. Geht an den GSG-Server, der die Crew danach
    /// festlegt; die Stimme folgt dem Geschlecht der Crew (Thomas 18.10.2026).
    pub kapitaen_geschlecht: String,
    /// Klang: klar | lautsprecher (Bordlautsprecher, ab Werk) | tuer (dumpf,
    /// wie durch die Cockpittür).
    pub klang: String,
    /// Abgewählte Ansagen (Slot-Namen): kommen nicht, werden nicht angefragt.
    pub aus: Vec<String>,
    /// Boarding-Musik an/aus (2.1.1, Entscheidung 15; Schalter auf der Seite
    /// „Kabine“). Aus = keine Musik und kein Lied angefragt. Ab Werk an.
    pub boarding_musik: bool,
    /// Eigener Pegel der Musik 0.0 … 1.0, ab Werk 20 % (Entscheidung 14).
    pub musik_lautstaerke: f32,
}

impl Default for Einstellungen {
    fn default() -> Self {
        Einstellungen {
            an: false,
            zweite_sprache: "airline".into(),
            echter_name: false,
            chimes: true,
            kommandos: true,
            lautstaerke: 0.8,
            ausgabe: None,
            kapitaen_geschlecht: "auto".into(),
            klang: "lautsprecher".into(),
            aus: Vec::new(),
            boarding_musik: true,
            musik_lautstaerke: 0.2,
        }
    }
}


impl Einstellungen {
    /// Unbekannte Werte auf den Standard, Zahlen in ihren Bereich.
    pub(crate) fn normalisiert(mut self) -> Self {
        let std = Einstellungen::default();
        // Fruehere Modi (keine, ziel, eigene) gibt es nicht mehr: Airline + Englisch.
        if !["airline", "airline_de"].contains(&self.zweite_sprache.as_str()) {
            self.zweite_sprache = std.zweite_sprache.clone();
        }
        self.lautstaerke = if self.lautstaerke.is_finite() { self.lautstaerke.clamp(0.0, 1.0) } else { 0.8 };
        self.musik_lautstaerke = if self.musik_lautstaerke.is_finite() { self.musik_lautstaerke.clamp(0.0, 1.0) } else { 0.2 };
        self.ausgabe = self
            .ausgabe
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty() && a.len() <= 512);
        if !["auto", "m", "w"].contains(&self.kapitaen_geschlecht.as_str()) {
            self.kapitaen_geschlecht = std.kapitaen_geschlecht.clone();
        }
        if !["klar", "lautsprecher", "tuer"].contains(&self.klang.as_str()) {
            self.klang = std.klang.clone();
        }
        // Nur bekannte Slots, jeder einmal.
        let mut aus: Vec<String> = self.aus.into_iter().filter(|n| slot_aus_name(n).is_some()).collect();
        aus.sort();
        aus.dedup();
        self.aus = aus;
        self
    }

    /// Die abgewählten Ansagen als Slots.
    pub(crate) fn abgewaehlt(&self) -> Vec<Slot> {
        self.aus.iter().filter_map(|n| slot_aus_name(n)).collect()
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
    // Azure gibt es nicht mehr: einen alten Schlüssel nicht liegen lassen.
    let _ = secrets::delete_api_key(ALTES_AZURE_KONTO);
    ausgabe::lautstaerke_setzen(geladen.lautstaerke);
    crate::kabinen_musik::lautstaerke_setzen(geladen.musik_lautstaerke);
    *zelle.write().expect("kabinen einstellungen") = Some(geladen.clone());
    geladen
}

fn einstellungen_schreiben(app: &AppHandle, neu: &Einstellungen) -> Result<(), UiError> {
    let pfad = einstellungen_pfad(app)
        .ok_or_else(|| UiError::new("kein_konfig_ordner", "no config dir"))?;
    if let Some(ordner) = pfad.parent() {
        std::fs::create_dir_all(ordner).map_err(|e| UiError::new("schreiben", e.to_string()))?;
    }
    let text = serde_json::to_string_pretty(neu).map_err(|e| UiError::new("schreiben", e.to_string()))?;
    std::fs::write(&pfad, text).map_err(|e| UiError::new("schreiben", e.to_string()))?;
    *EINSTELLUNGEN.get_or_init(|| RwLock::new(None)).write().expect("kabinen einstellungen") = Some(neu.clone());
    Ok(())
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
    herkunft: Option<String>,
) -> Result<Einstellungen, UiError> {
    let mut neu = einstellungen.normalisiert();
    // Die Lautstärke hat ihr eigenes Kommando (Regler auf der Seite und in
    // den Einstellungen): Ein älterer Stand der Einstellungsseite darf sie
    // nicht zurückdrehen (Codex beta.6).
    let bisher = self::einstellungen(&app);
    neu.lautstaerke = bisher.lautstaerke;
    // Musik-Regler und -Schalter sitzen auf der Seite „Kabine“ (Design
    // 19.10.2026) und haben eigene Kommandos.
    neu.musik_lautstaerke = bisher.musik_lautstaerke;
    neu.boarding_musik = bisher.boarding_musik;
    if !neu.an {
        // „Ansagen aus“ stoppt auch die Musik sofort.
        crate::kabinen_musik::aus();
        // Ausgeschaltet heißt aus: Was im laufenden Flug noch wartet, wird
        // verworfen und kommt auch nach einem Wiedereinschalten nicht nach.
        let flug = state.active_flight.lock().expect("active_flight lock").as_ref().map(Arc::clone);
        if let Some(f) = flug {
            f.stats.lock().expect("flight stats").kabinen_lauf.warteschlange = Default::default();
        }
        // Seit 1b spielt das Backend: eine laufende Ansage hier beenden,
        // nicht erst über die Oberfläche.
        if bisher.an {
            ausgabe::stoppen();
        }
    }
    einstellungen_schreiben(&app, &neu)?;
    // 1d: Ein/Aus mitten im Flug mit Herkunft ins Flugprotokoll (der einzige
    // Schalter, der den Takt mitten im Flug anhält).
    if bisher.an != neu.an {
        if let Some(f) = aktiver_flug(&state) {
            let art = if neu.an { "ansagen_ein" } else { "ansagen_aus" };
            let details = serde_json::json!({ "herkunft": herkunft.as_deref().unwrap_or("unbekannt") });
            notiz(&mut f.stats.lock().expect("flight stats").kabinen_lauf, art, None, None, details);
            protokoll_leeren(&app, &f);
        }
    }
    Ok(neu)
}

/// Lautstärke 0 … 1: wirkt sofort, auch mitten in einer Ansage (der Faktor
/// gilt je Puffer der Ausgabe; was schon im Gerät steckt, spielt noch mit
/// dem alten Wert). `speichern` beim Loslassen des Reglers.
#[tauri::command]
pub(crate) fn kabinen_lautstaerke(app: AppHandle, lautstaerke: f32, speichern: bool) -> Result<(), UiError> {
    let mut e = einstellungen(&app);
    e.lautstaerke = lautstaerke;
    let e = e.normalisiert();
    ausgabe::lautstaerke_setzen(e.lautstaerke);
    if speichern {
        einstellungen_schreiben(&app, &e)?;
    }
    Ok(())
}

/// Musik-Lautstärke 0 … 1 (zweiter Regler der Ton-Leiste): wirkt sofort,
/// auch auf ein laufendes Stück. `speichern` beim Loslassen.
#[tauri::command]
pub(crate) fn kabinen_musik_lautstaerke(app: AppHandle, lautstaerke: f32, speichern: bool) -> Result<(), UiError> {
    let mut e = einstellungen(&app);
    e.musik_lautstaerke = lautstaerke;
    let e = e.normalisiert();
    crate::kabinen_musik::lautstaerke_setzen(e.musik_lautstaerke);
    if speichern {
        einstellungen_schreiben(&app, &e)?;
    }
    Ok(())
}

/// Schalter „Boarding-Musik“ (Kasten auf der Seite „Kabine“). Aus stoppt
/// sofort und fragt kein Lied mehr an (Entscheidung 15).
#[tauri::command]
pub(crate) fn kabinen_boarding_musik(app: AppHandle, an: bool) -> Result<(), UiError> {
    let mut e = einstellungen(&app);
    e.boarding_musik = an;
    if !an {
        crate::kabinen_musik::aus();
    }
    einstellungen_schreiben(&app, &e.normalisiert())
}

/// Für die Einstellungsseite: was steht zur Verfügung?
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Zustand {
    /// Freigeschaltet und Monatsdeckel nicht erreicht.
    beta: Option<bool>,
    /// Freigeschaltet, aber Monatsdeckel erreicht: vorhandene Texte gehen,
    /// neue nicht (getrennt angezeigt, Punkt 10).
    kontingent_erschoepft: Option<bool>,
}

#[tauri::command]
pub(crate) async fn kabinen_zustand(
    state: tauri::State<'_, AppState>,
) -> Result<Zustand, UiError> {
    let client = state.client.lock().expect("client lock").clone();
    let status = match client {
        Some(c) => c.kabinen_status().await.ok(),
        None => None,
    };
    Ok(Zustand {
        beta: status.as_ref().map(|s| s.beta),
        kontingent_erschoepft: status.as_ref().map(|s| s.beta && !s.bereit),
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
    /// `<flugordner>/<name>.wav` im Ton-Ordner.
    datei: String,
    dauer_s: f64,
    /// Satz-Startzeiten aus dem WAV-Abschnitt `satz` (Ton-Server); leer bei
    /// älteren Tönen — dann schätzt die Seite.
    stuecke: Vec<Stueck>,
    /// Für das Auswertungs-Log: `lokal` (Zwischenspeicher), `server`, `ersatz`.
    herkunft: &'static str,
}

/// Ein gesprochenes Stück (Satz) mit Startzeit, für die Untertitel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Stueck {
    ab_s: f64,
    text: String,
}

/// Ein Teil einer gespielten Ansage (eine Sprache).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Teil {
    sprache: String,
    text: String,
    datei: String,
    dauer_s: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    stuecke: Vec<Stueck>,
}

/// Eine gespielte (oder vorgehörte) Ansage — geht so an die Oberfläche.
/// Die Oberfläche meldet das Ende mit `kabinen_wiedergabe_ende` zurück;
/// erst dann steht in `ergebnis`, ob sie wirklich lief.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Gespielt {
    #[serde(default)]
    id: u64,
    slot: Slot,
    rolle: Rolle,
    zeit: i64,
    teile: Vec<Teil>,
    /// Nur angehört (Vorhören/Nochmal): zählt nicht als gespielt.
    #[serde(default)]
    vorschau: bool,
    /// `None` = ausgegeben bzw. läuft; sonst `gespielt`, `abgebrochen`,
    /// `fehler`, `unbekannt`, `stumm` (übersprungen) oder `entfallen`.
    #[serde(default)]
    ergebnis: Option<String>,
    /// Die Oberfläche hat die Wiedergabe begonnen (erst dann „läuft“).
    #[serde(default)]
    gestartet: bool,
    /// Warum sie entfiel: `zu_wenig_zeit`, `uebersprungen`, `ton_nicht_fertig`,
    /// `zu_spaet`, `keine_stimme`, `kein_kurztext`.
    #[serde(default)]
    grund: Option<String>,
}

/// Ton-Auftrag, der gerade nicht ging (Server ausgelastet, Netz): Zahl der
/// Versuche, frühester nächster Versuch, letzte Meldung.
#[derive(Debug, Clone)]
struct TonFehler {
    versuche: u32,
    ab: i64,
    meldung: String,
}

/// Kabinenansagen eines Fluges. Texte und Ton holt der Client nach einem
/// Neustart neu (Server-Zwischenspeicher, kostenlos; Ton aus dem Ordner
/// des Fluges), der Verlauf liegt als `verlauf.json` daneben.
#[derive(Debug, Default)]
pub(crate) struct Lauf {
    beta: Beta,
    beta_naechste_pruefung: i64,
    /// Status nicht erreichbar: dann steht hier, wann wieder gefragt wird.
    beta_netzfehler: bool,
    /// Freigeschaltet, aber Monatsdeckel erreicht (`bereit` falsch).
    kontingent_erschoepft: bool,
    sprachen: Vec<String>,
    texte: HashMap<(Slot, String), String>,
    angefragt: HashSet<String>,
    fehlversuche: HashMap<String, (u8, i64)>,
    anlass: u32,
    /// `None` = für diese Sprache gibt es keine Stimme, sie entfällt.
    ton: HashMap<(Slot, String), Option<Ton>>,
    in_arbeit: HashSet<(Slot, String)>,
    /// Laufende Vorrats-Anfragen (höchstens [`TON_GLEICHZEITIG`]; fällige
    /// zählen nicht mit, siehe [`ton_auswahl`]).
    ton_laufend: usize,
    /// Vorübergehend nicht erzeugbar: wird wiederholt, bis die Ansage
    /// verfällt (keine feste Versuchszahl mehr, beta.5).
    ton_fehler: HashMap<(Slot, String), TonFehler>,
    /// Endgültig abgelehnt (400): kommt in diesem Flug nicht mehr.
    ton_endgueltig: HashMap<(Slot, String), String>,
    /// Fehler je Slot, dessen Text fehlt (Meldung des Servers).
    texte_fehler: std::collections::BTreeMap<Slot, String>,
    warteschlange: cabin_announcements::Warteschlange,
    verlauf: Vec<Gespielt>,
    verlauf_geladen: bool,
    naechste_id: u64,
    /// Läuft gerade ein Vorhören (ID)?
    vorschau: Option<u64>,
    /// Zuletzt gemeldete Ruhephase und Sim-Pause — für den Grund auf der Seite.
    ruhephase: bool,
    sim_angehalten: bool,
    /// Passagierzahl nachholen (Wiedereinstieg ohne Netz): Versuche, nächster.
    pax_versuche: u8,
    pax_naechster: i64,
    /// Der Server hat die Sicherheitsansage als Kurzfassung des Kapitäns
    /// gekennzeichnet (`rolle: kapitaen`): Stimme und Anzeige Kapitän.
    sicherheit_kapitaen: bool,
    /// Slots, die auf diesem Flug sicher nicht kommen, mit Grund (z. B.
    /// Bizjet ohne Kurzfassung der Sicherheit vom Server).
    entfaellt: std::collections::BTreeMap<Slot, String>,
    /// Crew dieses Fluges laut GSG-Server (`crew` im Drehbuch): Name des
    /// Kapitäns und Geschlecht von Kapitän und Kabine — danach die Stimme.
    crew: Option<api_client::KabinenCrew>,
    /// „Reiseflug 80“: zweiter_service | ende_bordverkauf (Server).
    art_r80: Option<String>,
    /// Seit wann im Reiseflug (für den Hinweis „Anschnallzeichen noch an“).
    reiseflug_seit: Option<i64>,
    /// Hinweis an den Piloten (z. B. „anschnallzeichen“), sonst `None`.
    hinweis: Option<&'static str>,
    /// Auswertungs-Log (v2.0.28): Einträge fürs Flugprotokoll, geschrieben
    /// im nächsten Takt außerhalb der Flugdaten-Sperre.
    protokoll: Vec<Notiz>,
    /// Zuletzt protokollierter Wartegrund je fälliger Ansage (nur Wechsel).
    wartet_zuletzt: HashMap<Slot, Option<Wartet>>,
    /// Fremde Namen je Text (Slot, Sprache) aus dem Drehbuch (2.1.0).
    fremd: HashMap<(Slot, String), Vec<api_client::KabinenFremd>>,
    /// Zuletzt protokollierte Abwahl des Piloten samt Kommando- und
    /// Gong-Schalter (nur Wechsel).
    abgewaehlt_zuletzt: Option<Vec<Slot>>,
    /// Boarding-Musik (2.1.1 G): Zeitfenster, Quelle (einmal je Flug
    /// festgelegt, Design §5) und das Lied des Flugs.
    musik_fenster: cabin_announcements::musik::MusikFenster,
    musik_quelle: Option<MusikQuelle>,
    musik_lief: bool,
    /// Abschnitt, in dem die Musik zuletzt lief (Anzeige „läuft · …“).
    musik_abschnitt: Option<cabin_announcements::musik::Abschnitt>,
    lied: LiedStand,
    /// Zuletzt protokollierte Eingangssperre des Takts (Rundumschlag 1d),
    /// `None` = frei (auch: noch nichts notiert).
    sperre_zuletzt: Option<&'static str>,
    /// Seit wann die Abgabe nach „Angekommen“ auf die Kabine wartet (1a).
    abgabe_wartet_seit: Option<i64>,
    /// Sprachen ohne Stimme, für den ganzen Flug gestrichen (Punkt 4):
    /// vom Drehbuch (`entfaellt: sprache/xx = keine_stimme`) oder vom ersten
    /// „keine Stimme“ des Ton-Servers.
    ohne_stimme: std::collections::BTreeSet<String>,
    /// Sprachen ohne Sicherheitsvorlage (`entfaellt: sicherheit/xx = keine_vorlage`):
    /// werden nicht nachgeholt, die Seite sagt es einmal.
    ohne_vorlage: std::collections::BTreeSet<String>,
    /// Texte, die auch die Nachholung nicht brachte — je Slot UND Sprache
    /// (1f), damit „Aufgegeben“ nur dort steht, wo wirklich etwas fehlt.
    texte_fehlen: Fehlen,
}

/// Fehlende Texte je (Slot, Sprache) mit Grund.
type Fehlen = std::collections::BTreeMap<(Slot, String), String>;

/// Woher die Boarding-Musik dieses Flugs kommt.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum MusikQuelle {
    /// Eigene Musik des Piloten (Ordner der Airline oder „Alle“).
    Eigene { ordner: String, dateien: Vec<PathBuf> },
    /// Das Lied je Flug (acemusic.ai über den Ton-Server).
    Lied,
    /// Nichts: Bizjet/Fracht ohne eigene Musik, oder Schalter aus.
    Keine,
}

/// Das Boarding-Lied des Flugs: Vorlage vom Modul, Datei vom Ton-Server.
#[derive(Debug, Default)]
pub(crate) struct LiedStand {
    vorlage: Option<api_client::KabinenLied>,
    datei: Option<PathBuf>,
    laeuft: bool,
    versuche: u32,
    naechster: i64,
    fehler: Option<String>,
    /// Slots der Abflug-Anfrage mit Lied; mit denselben wird nachgefragt,
    /// dann liefert das Modul das Drehbuch aus dem Zwischenspeicher.
    slots: Vec<Slot>,
    /// Das Lied wurde mit dem Abflug-Drehbuch angefragt (sonst kommt keins:
    /// nach einem Neustart ab „Türen zu“ wird nicht mehr angefragt).
    angefragt: bool,
    /// Das Modul hatte keine Zeit mehr: ab dann noch einmal fragen …
    nochmal_ab: Option<i64>,
    /// … aber nur einmal je Flug (Codex 2.1.1: auch eine späte Antwort
    /// einer anderen Anfrage plant keine zweite Nachfrage).
    nachgefragt: bool,
    /// Der Ordner des Flugs wurde nach einer schon geladenen Musikdatei
    /// durchsucht (nach einem Neustart): erst danach anfragen (Codex 2.1.3).
    platte_geprueft: bool,
}

/// Höchstens so oft die Musikdatei beim Ton-Server holen, dann „nicht geladen“
/// (Codex 2.1.3: sonst alle 60 s bis zur Landung); „Boarding-Musik laden“
/// setzt das zurück.
const LIED_TON_VERSUCHE: u32 = 5;

/// Quelle der Musik (Design §5): eigene Musik der Airline, sonst „Alle“,
/// sonst das Lied (nur Linie, nur mit Schalter an); Bizjet und Fracht nur
/// eigene Musik (Entscheidung 11).
fn musik_quelle(flugart: Flugart, haken: bool, eigene: Option<(String, Vec<PathBuf>)>) -> MusikQuelle {
    match eigene {
        Some((ordner, dateien)) if haken && !dateien.is_empty() => MusikQuelle::Eigene { ordner, dateien },
        _ if cabin_announcements::musik::lied_anfragen(flugart, haken, false) => MusikQuelle::Lied,
        _ => MusikQuelle::Keine,
    }
}

/// Ein Eintrag des Auswertungs-Logs (`FlightLogEvent::Kabine`).
#[derive(Debug, Clone)]
struct Notiz {
    zeit: DateTime<Utc>,
    art: &'static str,
    slot: Option<Slot>,
    sprache: Option<String>,
    details: serde_json::Value,
}

/// Merkt einen Schritt einer Ansage fürs Flugprotokoll vor (v2.0.28: „ob der
/// Server rechtzeitig angenommen und geliefert hat — das muss man aus den
/// Logs lesen können“, Thomas 19.10.2026).
fn notiz(lauf: &mut Lauf, art: &'static str, slot: Option<Slot>, sprache: Option<&str>, details: serde_json::Value) {
    lauf.protokoll.push(Notiz { zeit: Utc::now(), art, slot, sprache: sprache.map(str::to_string), details });
}

/// Nimmt die vorgemerkten Einträge (kurz gesperrt) und schreibt sie ohne
/// Sperre — nach jedem Takt (auch bei frühem Ende) und nach Antworten des
/// Servers, damit am Flugende nichts im Puffer liegen bleibt (Codex 2.0.28).
fn protokoll_leeren(app: &AppHandle, flight: &ActiveFlight) {
    // Entnehmen und Schreiben am Stück, damit zwei gleichzeitige Leerungen
    // (Takt und Serverantwort) die Reihenfolge im Protokoll nicht vertauschen
    // (Codex 2.0.28). Nie unter der Flugdaten-Sperre aufrufen.
    static SCHREIBEN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _reihe = SCHREIBEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let notizen = std::mem::take(&mut flight.stats.lock().expect("flight stats").kabinen_lauf.protokoll);
    if !notizen.is_empty() {
        protokoll_schreiben(app, flight, notizen);
    }
}

/// Schreibt die vorgemerkten Einträge ins Flugprotokoll (ohne Sperre).
fn protokoll_schreiben(app: &AppHandle, flight: &ActiveFlight, notizen: Vec<Notiz>) {
    for n in notizen {
        let slot = n.slot.map(slot_name);
        tracing::info!(art = n.art, slot = ?slot, sprache = ?n.sprache, details = %n.details, "kabine");
        crate::record_event(
            app,
            &flight.pirep_id,
            &recorder::FlightLogEvent::Kabine { timestamp: n.zeit, art: n.art.to_string(), slot, sprache: n.sprache, details: n.details },
        );
    }
}

/// Altes Modul: Reiseflug 80 ist das Ende des Bordverkaufs. Eine schon
/// wartende eigene „Ende des Service“-Ansage entfällt dann. `true` = Verlauf
/// geändert.
fn ende_service_bei_alter_art(lauf: &mut Lauf, jetzt: i64) -> bool {
    if zweiter_service(lauf) != Some(false) || !lauf.warteschlange.entfernen(Slot::EndeService) {
        return false;
    }
    notiz(lauf, "entfallen", Some(Slot::EndeService), None, serde_json::json!({ "grund": "uebersprungen" }));
    verbuchen(lauf, Slot::EndeService, "entfallen", Some("uebersprungen"), jetzt);
    true
}

/// Slots, die in diesem Flug schon erledigt sind (Verlauf): gespielt, aber
/// auch stumm übersprungen, entfallen, abgebrochen oder mit unbekanntem
/// Ausgang — nichts davon kommt nach einem Neustart noch einmal
/// (Codex-Review PR #160).
pub(crate) fn erledigte_slots(lauf: &Lauf) -> Vec<Slot> {
    lauf.verlauf.iter().filter(|g| !g.vorschau && g.ergebnis.is_some()).map(|g| g.slot).collect()
}

/// Wartet Reiseflug 80 als zweiter Service, obwohl weniger als
/// [`cabin_announcements::planer::ZWEITER_SERVICE_REST_MIN`] Restflug bleiben?
fn zweiter_service_zu_spaet(lauf: &Lauf, rest_min: Option<f64>) -> bool {
    zweiter_service(lauf) == Some(true)
        && lauf.warteschlange.wartend().contains(&Slot::Reiseflug80)
        && rest_min.is_some_and(|m| m <= cabin_announcements::planer::ZWEITER_SERVICE_REST_MIN)
}

/// Art von „Reiseflug 80“ laut Drehbuch: `Some(true)` = zweiter Service,
/// `Some(false)` = Ende des Bordverkaufs, `None` = noch nicht bekannt.
pub(crate) fn zweiter_service(lauf: &Lauf) -> Option<bool> {
    match lauf.art_r80.as_deref() {
        Some("zweiter_service") => Some(true),
        Some("ende_bordverkauf") => Some(false),
        _ => None,
    }
}

/// Männliche Stimme? Folgt dem Geschlecht der Crew (Server); ohne Angabe
/// Kapitän männlich, Kabine weiblich. Kommandos sprechen mit der Stimme des
/// Kapitäns (Thomas 19.10.2026, 2.1.0; vorher fest männlich).
fn stimme_maennlich(lauf: &Lauf, rolle: Rolle) -> bool {
    let crew = lauf.crew.as_ref();
    match rolle {
        Rolle::Kapitaen | Rolle::Kommando => crew.and_then(|c| c.kapitaen_geschlecht.as_deref()) != Some("w"),
        Rolle::Kabine => crew.and_then(|c| c.kabine_geschlecht.as_deref()) == Some("m"),
        Rolle::Chime => true,
    }
}

/// Rolle für den Ton-Server (`rolle`, ab 2.1.0): Mit ihr und der PIREP-Nummer
/// wählt er je Flug eine Stimme je Rolle, gleich in allen Sprachen, und
/// Kapitän und Kabine nie dieselbe — auch bei gleichem Geschlecht.
fn server_rolle(rolle: Rolle) -> Option<&'static str> {
    match rolle {
        Rolle::Kapitaen => Some("kapitaen"),
        Rolle::Kabine => Some("kabine"),
        Rolle::Kommando => Some("kommando"),
        Rolle::Chime => None,
    }
}

/// PIREP-Nummer für den Ton-Server (`pirep_id`), nur in der Form, mit der er
/// die Stimme wählt (`[A-Za-z0-9_-]{1,64}`); sonst ohne.
fn flug_kennung(pirep_id: &str) -> Option<String> {
    let gut = (1..=64).contains(&pirep_id.len()) && pirep_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    gut.then(|| pirep_id.to_string())
}

/// Wer spricht: die Rolle des Slots, außer der Server legt sie fest (die
/// Sicherheit als Kurzfassung des Kapitäns). Gilt für Stimme, Anzeige und
/// „Bisher“ — unabhängig von der Flugart im Client, damit die Kabinenstimme
/// nie „Hier spricht Ihr Kapitän“ liest (Advisor beta.6).
fn rolle_von(lauf: &Lauf, slot: Slot) -> Rolle {
    if slot == Slot::Sicherheit && lauf.sicherheit_kapitaen {
        Rolle::Kapitaen
    } else if slot == Slot::BoardingAbgeschlossen {
        // Fester englischer Text wie ein Kommando, aber die Kabine spricht.
        Rolle::Kabine
    } else {
        slot.rolle()
    }
}

/// Trägt eine Ansage in „Bisher“ ein, die nicht spielt (stumm, entfallen),
/// mit dem Text, soweit er da ist. Zählt nicht als gespielt, macht keine Pause.
fn verbuchen(lauf: &mut Lauf, slot: Slot, ergebnis: &str, grund: Option<&str>, jetzt: i64) {
    lauf.naechste_id += 1;
    let g = Gespielt {
        id: lauf.naechste_id,
        slot,
        rolle: rolle_von(lauf, slot),
        zeit: jetzt,
        teile: teile_von(lauf, slot).unwrap_or_default(),
        vorschau: false,
        ergebnis: Some(ergebnis.to_string()),
        gestartet: false,
        grund: grund.map(str::to_string),
    };
    lauf.verlauf.push(g);
}

/// Stumm: alles Wartende als „stumm“ verbuchen, die laufende Ansage auch;
/// keine Pause danach (es war ja nichts zu hören). `true` = Verlauf geändert.
fn stumm_anwenden(stats: &mut FlightStats, jetzt: i64) -> bool {
    let lauf = &mut stats.kabinen_lauf;
    let mut geaendert = false;
    let mut kapitaen = false;
    for g in lauf.verlauf.iter_mut().filter(|g| g.ergebnis.is_none() && !g.vorschau) {
        g.ergebnis = Some("stumm".into());
        kapitaen |= g.slot == Slot::KapitaenSinkflug;
        geaendert = true;
    }
    lauf.vorschau = None;
    lauf.warteschlange.abbrechen();
    for slot in lauf.warteschlange.alle_entnehmen() {
        verbuchen(lauf, slot, "stumm", None, jetzt);
        kapitaen |= slot == Slot::KapitaenSinkflug;
        geaendert = true;
    }
    if kapitaen {
        if let Some(p) = stats.kabinen_planer.as_mut() {
            p.kapitaen_sinkflug_ende(false, jetzt);
        }
    }
    geaendert
}

/// Gleichzeitige Ton-Anfragen je Flug. Zwei reichen: Der Server arbeitet
/// ohnehin nach Dringlichkeit ab; mehr hielte nur Verbindungen offen.
const TON_GLEICHZEITIG: usize = 2;
/// Ankunftstexte werden erst so kurz vor der Landung geschrieben, damit
/// Wetter, Verspätung und Ankunftszeit stimmen.
const ANKUNFT_TEXT_VOR_LANDUNG_MIN: i64 = 45;

const ABFLUG: [Slot; 8] = [
    Slot::Boarding,
    Slot::BegruessungTuerenZu,
    Slot::Sicherheit,
    Slot::KapitaenBegruessung,
    Slot::AnschnallzeichenAus,
    Slot::Reiseflug25,
    Slot::Reiseflug80,
    // 2.1.0: im Abflug-Teil, damit der Text vor dem Sinkflug sicher da ist.
    Slot::EndeService,
];
/// 2.1.1 Zusatz H (Thomas 19.10.2026): Die Halbzeit-Ansage des Kapitäns
/// schreibt das Modul erst im Reiseflug, mit Restzeit, Landezeit und
/// aktuellem Zielwetter (Teil `halbzeit`).
const HALBZEIT: [Slot; 1] = [Slot::Reiseflug50];
/// So lange vor ihrem Zeitpunkt wird die Halbzeit-Ansage angefragt (Text
/// plus Ton brauchen zusammen einige Minuten).
const HALBZEIT_VORLAUF_MIN: i64 = 15;
const ANKUNFT: [Slot; 4] = [Slot::KapitaenSinkflug, Slot::KabineSinkflug, Slot::NachDerLandung, Slot::Aussteigen];
const KOMMANDOS: [Slot; 6] = [
    Slot::BoardingAbgeschlossen,
    Slot::KommandoTuerenScharf,
    Slot::KommandoStart,
    Slot::KommandoLandungVorbereiten,
    Slot::KommandoPlaetzeLandung,
    Slot::KommandoTuerenEntschaerfen,
];
const GONGS: [Slot; 2] = [Slot::Chime10000Steigflug, Slot::Chime10000Sinkflug];

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

/// Zu welchem Text-Auftrag gehört ein Slot?
fn teil_von(s: Slot) -> Option<&'static str> {
    if ABFLUG.contains(&s) {
        Some("abflug")
    } else if HALBZEIT.contains(&s) {
        Some("halbzeit")
    } else if ANKUNFT.contains(&s) {
        Some("ankunft")
    } else {
        None
    }
}

/// Teil für den Ton-Server (Vorrat: Abflug vor Ankunft). Kommandos
/// gehören zu ihrem Flugabschnitt — ohne `teil` und ohne Zeit hielte der
/// Server sie für „bald“ und zöge sie vor den Abflug-Vorrat.
fn ton_teil(s: Slot) -> Option<&'static str> {
    match s {
        Slot::KommandoTuerenScharf | Slot::KommandoStart => Some("abflug"),
        Slot::KommandoLandungVorbereiten | Slot::KommandoPlaetzeLandung | Slot::KommandoTuerenEntschaerfen => Some("ankunft"),
        // 2.1.6: Ein Ereignis ist im Augenblick der Anfrage fällig und war
        // nie vorab zu holen — der Server zählt es nicht als „zu spät“.
        _ if ist_ereignis(s) => Some("ereignis"),
        _ => teil_von(s),
    }
}

fn slot_name(s: Slot) -> String {
    serde_json::to_value(s).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

fn slot_aus_name(n: &str) -> Option<Slot> {
    serde_json::from_value(serde_json::Value::String(n.to_string())).ok()
}

/// Darf ein Teil (wieder) angefragt werden? Nach einem Fehler höchstens
/// dreimal, mit zwei Minuten Abstand — danach nur über „Erneut versuchen“.
fn darf_anfragen(lauf: &Lauf, schluessel: &str, jetzt: i64) -> bool {
    !lauf.angefragt.contains(schluessel)
        && lauf.fehlversuche.get(schluessel).is_none_or(|(n, ab)| *n < 3 && jetzt >= *ab)
}

/// Braucht dieser Text (noch) Ton? Nicht schon da, nicht in Arbeit, nicht
/// endgültig abgelehnt, und nach einem vorübergehenden Fehler erst nach der
/// Wartezeit des Servers. Aufgegeben wird nicht: Was nicht mehr kommt,
/// fällt beim Auswählen weg ([`ton_pumpe`]).
fn ton_noetig(lauf: &Lauf, k: &(Slot, String), jetzt: i64) -> bool {
    !lauf.ton.contains_key(k)
        && !lauf.in_arbeit.contains(k)
        && !lauf.ton_endgueltig.contains_key(k)
        && lauf.ton_fehler.get(k).is_none_or(|f| jetzt >= f.ab)
}

/// Kommt dieser Slot auf diesem Flug noch (oder wartet er schon)?
fn kommt_noch(stats: &FlightStats, wartend: &[Slot], slot: Slot) -> bool {
    !stats.kabinen_lauf.entfaellt.contains_key(&slot)
        && (wartend.contains(&slot)
            || ist_ereignis(slot)
            || stats.kabinen_planer.as_ref().is_some_and(|p| p.kommt_noch(slot, stats.phase)))
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
    takt_schritt(app, flight, client, snap, now);
    musik_takt(app, flight, snap, now);
    protokoll_leeren(app, flight);
}

/// Name einer Eingangssperre des Takts fürs Flugprotokoll — eine Quelle mit
/// der Seite ([`grund`]). Der Monatsdeckel ist keine Sperre: Der Takt läuft
/// weiter, Vorhandenes kommt aus dem Zwischenspeicher.
fn sperre_name(an: bool, stats: &FlightStats) -> Option<&'static str> {
    Some(match grund(an, stats)? {
        Grund::Aus => "aus",
        Grund::Replay => "replay",
        Grund::Fracht => "fracht",
        Grund::KeinePax => "keine_pax",
        Grund::KeinePassagiere => "keine_passagiere",
        Grund::BetaPruefung => "beta_pruefung",
        Grund::BetaNichtErreichbar(_) => "beta_nicht_erreichbar",
        Grund::NichtFreigeschaltet => "nicht_freigeschaltet",
        Grund::SimAngehalten => "sim_angehalten",
        Grund::Kontingent => return None,
    })
}

/// 1d: Jeder Wechsel der Eingangssperre genau ein Eintrag `gesperrt{grund}`
/// bzw. `frei`, ohne Wechsel keiner (CDC 8939/ANZ 438: der Takt stand
/// minutenlang am Eingang, und nichts im Protokoll sagte, warum). Wird die
/// Kabine nach „aus“ wieder frei, bestellt sie sofort vorab (Entscheidung 6).
fn sperre_pruefen(stats: &mut FlightStats, an: bool, jetzt: i64) {
    let neu = sperre_name(an, stats);
    let lauf = &mut stats.kabinen_lauf;
    let alt = lauf.sperre_zuletzt;
    if alt == neu {
        return;
    }
    lauf.sperre_zuletzt = neu;
    match neu {
        Some(g) => notiz(lauf, "gesperrt", None, None, serde_json::json!({ "grund": g })),
        None => notiz(lauf, "frei", None, None, serde_json::json!({ "vorher": alt })),
    }
    if alt == Some("aus") && neu != Some("aus") {
        vorab_anstossen(lauf, jetzt);
    }
}

/// Entscheidung 6: Nach dem Wiedereinschalten Text und Ton sofort bestellen
/// statt die Wartezeit nach einem früheren Fehlversuch abzusitzen. Die
/// Bestellung selbst macht der Takt gleich danach (`holen`, `ton_pumpe`).
fn vorab_anstossen(lauf: &mut Lauf, jetzt: i64) {
    for (_, ab) in lauf.fehlversuche.values_mut() {
        *ab = (*ab).min(jetzt);
    }
    for f in lauf.ton_fehler.values_mut() {
        f.ab = f.ab.min(jetzt);
    }
}

/// Wie lange die Abgabe nach „Angekommen“ höchstens auf die Kabine wartet
/// (1a, Entscheidung 1: PIREP bis zu 90 s später, Thomas 22.10.2026).
pub(crate) const ABGABE_WARTET_HOECHSTENS_S: i64 = 90;

/// 1a: Soll das Auto-File noch warten? Ja, solange eine Ansage spielt, in
/// der Schlange wartet oder „Aussteigen“ noch kommt — höchstens
/// [`ABGABE_WARTET_HOECHSTENS_S`] ab dem ersten Warten. Ansagen aus, nicht
/// freigeschaltet oder sonst gesperrt: nie (es käme ohnehin nichts).
pub(crate) fn kabine_haelt_abgabe(stats: &mut FlightStats, an: bool, jetzt: i64) -> bool {
    if stats.kabinen_lauf.beta != Beta::Ja || sperre_name(an, stats).is_some() {
        return false;
    }
    let aussteigen_kommt = stats.kabinen_planer.as_ref().is_some_and(|p| p.kommt_noch(Slot::Aussteigen, stats.phase));
    let lauf = &mut stats.kabinen_lauf;
    let wartend = lauf.warteschlange.wartend();
    let spielt = laeuft(lauf);
    if wartend.is_empty() && !spielt && !aussteigen_kommt {
        return false;
    }
    let seit = match lauf.abgabe_wartet_seit {
        Some(s) => s,
        None => {
            lauf.abgabe_wartet_seit = Some(jetzt);
            let details = serde_json::json!({ "wartend": wartend.iter().map(|s| slot_name(*s)).collect::<Vec<_>>(), "spielt": spielt, "hoechstens_s": ABGABE_WARTET_HOECHSTENS_S });
            notiz(lauf, "abgabe_wartet", None, None, details);
            jetzt
        }
    };
    jetzt - seit < ABGABE_WARTET_HOECHSTENS_S
}

/// 1a: Was beim Einreichen noch wartet (oder als Ankunftsansage noch käme),
/// fällt nicht still weg: `entfallen{grund:"flug_beendet"}` ins Protokoll und
/// in „Bisher“. Nur, wenn die Kabine überhaupt lief.
fn flug_beendet_notieren(stats: &mut FlightStats, einst: &Einstellungen, jetzt: i64) {
    if stats.kabinen_lauf.beta != Beta::Ja || sperre_name(einst.an, stats).is_some() {
        return;
    }
    let mut weg = stats.kabinen_lauf.warteschlange.alle_entnehmen();
    if let Some(p) = stats.kabinen_planer.as_ref() {
        for s in [Slot::NachDerLandung, Slot::KommandoTuerenEntschaerfen, Slot::Aussteigen] {
            let abgeschaltet = s.rolle() == Rolle::Kommando && !einst.kommandos;
            if !weg.contains(&s) && !abgeschaltet && p.kommt_noch(s, stats.phase) {
                weg.push(s);
            }
        }
    }
    let lauf = &mut stats.kabinen_lauf;
    for slot in weg {
        notiz(lauf, "entfallen", Some(slot), None, serde_json::json!({ "grund": "flug_beendet" }));
        verbuchen(lauf, slot, "entfallen", Some("flug_beendet"), jetzt);
    }
}

/// Beim Einreichen (beide Abgabewege) aufrufen, vor `pirep_filed`: Danach
/// kommt kein Takt mehr, deshalb gleich ins Protokoll schreiben.
pub(crate) fn flug_beendet(app: &AppHandle, flight: &ActiveFlight) {
    let einst = einstellungen(app);
    flug_beendet_notieren(&mut flight.stats.lock().expect("flight stats"), &einst, Utc::now().timestamp());
    protokoll_leeren(app, flight);
}

/// Fenster in den Vordergrund/Hintergrund oder sichtbar/verdeckt (1b): ins
/// Flugprotokoll, damit sich späte Enden einer schlafenden Oberfläche
/// zuordnen lassen. Nur während eines Flugs mit eingeschalteten Ansagen.
/// Eigener Faden: Der Aufruf kommt aus dem Hauptfaden (Fensterereignis), und
/// der darf nie auf die Flugdaten-Sperre warten.
pub(crate) fn fenster_notieren(app: &AppHandle, details: serde_json::Value) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !einstellungen(&app).an {
            return;
        }
        let Some(flight) = aktiver_flug(&app.state::<AppState>()) else { return };
        notiz(&mut flight.stats.lock().expect("flight stats").kabinen_lauf, "fenster", None, None, details);
        protokoll_leeren(&app, &flight);
    });
}

/// Die Oberfläche meldet, ob sie sichtbar ist (`visibilitychange`, 1b).
#[tauri::command]
pub(crate) fn kabinen_fenster(app: AppHandle, sichtbar: bool) {
    fenster_notieren(&app, serde_json::json!({ "sichtbar": sichtbar }));
}

/// Halbzeit-Text jetzt anfragen? Nur wenn die Ansage auf diesem Flug noch
/// kommt und höchstens [`HALBZEIT_VORLAUF_MIN`] vor ihrem Zeitpunkt. Im
/// Reiseflug (oder in der Warteschleife) ohne Zeitpunkt sofort; seit 1e auch
/// schon nach dem Abheben im Steigflug, dort aber nur mit bekanntem
/// Zeitpunkt (lag die Halbzeit noch im Steigflug, war sie bei Reiseflug-
/// Beginn sofort fällig, und Text plus Ton brauchten 36–39 s Stille).
fn halbzeit_anfragen(kommt: bool, phase: FlightPhase, um: Option<i64>, jetzt: i64) -> bool {
    let vorlauf = |u: i64| u - jetzt <= HALBZEIT_VORLAUF_MIN * 60;
    kommt
        && match phase {
            FlightPhase::Cruise | FlightPhase::Holding => um.is_none_or(vorlauf),
            FlightPhase::Takeoff | FlightPhase::Climb => um.is_some_and(vorlauf),
            _ => false,
        }
}

/// Lied mit dem Abflug-Drehbuch anfragen? Quelle Lied (also Linie, ohne
/// eigene Musik), Schalter an und noch vor „Türen zu“ — nach einem Neustart
/// im Reiseflug kostete es nur, gespielt würde es nicht mehr.
/// Das Lied wird gebraucht, solange noch Musik kommt: beim Boarding UND nach
/// der Landung (Zusatz I). Thomas 19.10.2026: nach einem Update mitten im Flug
/// kam keins, weil nur bis zum Pushback angefragt wurde.
fn lied_anfragen(lauf: &Lauf, haken: bool, _phase: FlightPhase) -> bool {
    haken
        && lauf.lied.platte_geprueft
        && lauf.musik_quelle == Some(MusikQuelle::Lied)
        && !lauf.musik_fenster.ankunft_vorbei()
        && lauf.lied.datei.is_none()
        && lauf.lied.vorlage.is_none()
}

/// Eigene Anfrage „lied“ (Modul ab 2.1.3) nach einem Update oder Neustart
/// mitten im Flug: noch nicht angefragt, kein Abflug-Drehbuch in diesem Takt,
/// das das Lied mitnimmt, und die Musik kommt noch.
fn lied_einzeln_anfragen(lauf: &Lauf, haken: bool, phase: FlightPhase, abflug_dabei: bool, jetzt: i64) -> bool {
    !lauf.lied.angefragt && !abflug_dabei && lied_anfragen(lauf, haken, phase) && darf_anfragen(lauf, "lied", jetzt)
}

/// Liest die eigene Musik (ganze Dateien, ohne Flugdaten-Sperre) — nur in
/// dem Takt, in dem die Quelle festgelegt wird: Freigabe da, Flugart steht
/// fest, Quelle noch offen. Sonst `None` ohne Plattenzugriff.
fn eigene_lesen(flight: &ActiveFlight) -> Option<(String, Vec<PathBuf>)> {
    let jetzt_noetig = {
        let st = flight.stats.lock().expect("flight stats");
        st.kabinen_lauf.musik_quelle.is_none()
            && st.kabinen_lauf.beta == Beta::Ja
            && cabin_announcements::flugart::eingeordnet(st.flight_type.as_deref(), st.pax_count.and_then(|n| u32::try_from(n).ok()))
    };
    if !jetzt_noetig {
        return None;
    }
    crate::kabinen_musik::ablage().and_then(|w| crate::kabinen_musik::eigene_fuer(&w, &flight.airline_icao))
}

/// Nach einer Abflug-Antwort: Lied da, später einmal nachfragen (das Modul
/// hatte bis zum Zeitlimit keine Zeit mehr), oder keins (dann still).
fn lied_nach_antwort(lied: &mut LiedStand, angefragt: bool, geliefert: bool, spaeter: bool, jetzt: i64) {
    if !angefragt || geliefert || lied.vorlage.is_some() {
        return;
    }
    if spaeter && !lied.nachgefragt {
        lied.nachgefragt = true;
        lied.nochmal_ab = Some(jetzt + 60);
    } else if !spaeter || lied.nochmal_ab.is_none() {
        lied.fehler = Some("Das Modul hat kein Lied geliefert.".into());
    }
}

/// Was die Musik nach diesem Takt tun soll.
#[derive(Debug, Clone, PartialEq)]
enum MusikTun {
    Nichts,
    Spielen(Vec<PathBuf>),
    Ausblenden,
    Aus,
}

/// Boarding-Musik je Takt (2.1.1 G): Fenster, Quelle, Lied holen. Ohne
/// AppHandle, damit testbar; `eigene` liest der Aufrufer ohne Sperre.
fn musik_schritt(stats: &mut FlightStats, snap: &SimSnapshot, muster: &str, eigene: Option<(String, Vec<PathBuf>)>, jetzt: i64) -> (MusikTun, Option<api_client::KabinenLied>) {
    use cabin_announcements::{flugart, musik};
    if stats.replay_verdacht || stats.kabinen_lauf.beta != Beta::Ja {
        return (MusikTun::Aus, None);
    }
    let pax = stats.pax_count.and_then(|n| u32::try_from(n).ok());
    // Linienflug ohne Passagierzahl: Die Einordnung kommt noch.
    if !flugart::eingeordnet(stats.flight_type.as_deref(), pax) {
        return (MusikTun::Nichts, None);
    }
    let art = flugart::einordnen(stats.flight_type.as_deref(), pax, muster);
    let begruessung = stats.kabinen_planer.as_ref().is_some_and(|p| p.erledigt(Slot::Boarding));
    // Zusatz I: Ist die Ansage „nach der Landung“ vorbei (gespielt, entfallen,
    // stumm, abgewählt) oder kommt sie nicht mehr?
    let nach_landung_vorbei = {
        let lauf = &stats.kabinen_lauf;
        let im_verlauf = |fertig: bool| lauf.verlauf.iter().any(|g| g.slot == Slot::NachDerLandung && !g.vorschau && g.ergebnis.is_some() == fertig);
        im_verlauf(true)
            || (!im_verlauf(false)
                && !lauf.warteschlange.wartend().contains(&Slot::NachDerLandung)
                && !stats.kabinen_planer.as_ref().is_none_or(|p| p.kommt_noch(Slot::NachDerLandung, stats.phase)))
    };
    let aussteigen_um = stats
        .kabinen_lauf
        .verlauf
        .iter()
        .filter(|g| g.slot == Slot::Aussteigen && !g.vorschau && g.ergebnis.as_deref() == Some("gespielt"))
        .map(|g| g.zeit + dauer_s(g.slot, &g.teile).ceil() as i64)
        .next();
    let lauf = &mut stats.kabinen_lauf;
    if lauf.musik_quelle.is_none() {
        lauf.musik_quelle = Some(musik_quelle(art, true, eigene));
    }
    let takt = musik::MusikTakt {
        phase: stats.phase,
        am_boden: snap.on_ground,
        beacon: snap.light_beacon,
        pushback_aktiv: matches!(snap.pushback_state, Some(0..=2)),
        begruessung,
        jetzt,
        nach_landung_vorbei,
        aussteigen_um,
        triebwerke_an: Some(snap.engines_running > 0),
    };
    let abschnitt = lauf.musik_fenster.tick(art, &takt);
    let soll = abschnitt.is_some();
    if abschnitt.is_some() {
        lauf.musik_abschnitt = abschnitt;
    }
    let dateien: Vec<PathBuf> = match &lauf.musik_quelle {
        Some(MusikQuelle::Eigene { dateien, .. }) => dateien.clone(),
        Some(MusikQuelle::Lied) => lauf.lied.datei.iter().cloned().collect(),
        _ => Vec::new(),
    };
    // Lied nachreichen (Entscheidung 9), solange noch Musik kommt: auch nach
    // „Türen zu“ für die Musik nach der Landung (Thomas 19.10.2026, Update im Flug).
    let holen = (lauf.musik_quelle == Some(MusikQuelle::Lied)
        && lauf.lied.datei.is_none()
        && !lauf.lied.laeuft
        && jetzt >= lauf.lied.naechster
        && lauf.lied.versuche < LIED_TON_VERSUCHE
        && !lauf.musik_fenster.ankunft_vorbei())
    .then(|| lauf.lied.vorlage.clone())
    .flatten();
    if holen.is_some() {
        lauf.lied.laeuft = true;
    }
    let tun = if soll && !dateien.is_empty() {
        lauf.musik_lief = true;
        MusikTun::Spielen(dateien)
    } else if lauf.musik_lief && !soll {
        lauf.musik_lief = false;
        MusikTun::Ausblenden
    } else {
        MusikTun::Nichts
    };
    (tun, holen)
}

fn musik_takt(app: &AppHandle, flight: &Arc<ActiveFlight>, snap: &SimSnapshot, now: DateTime<Utc>) {
    let einst = einstellungen(app);
    // Ansagen aus, Schalter aus oder Stumm: sofort still (Pflichtenheft G1).
    if !einst.an || !einst.boarding_musik || STUMM.load(Ordering::SeqCst) {
        if crate::kabinen_musik::flug_aktiv() {
            crate::kabinen_musik::aus();
        }
        return;
    }
    // Eigene Musik vor dem Flug neu lesen (Design §6), ohne Flugdaten-Sperre.
    let eigene = eigene_lesen(flight);
    // Nach einem Neustart liegt das Lied schon im Ordner des Fluges (Codex
    // 2.1.1): wieder aufnehmen, sonst bliebe es nach der Landung still.
    let lied_auf_platte = ton_wurzel(app).ok().map(|w| w.join(flug_ordner(flight)).join("boarding-lied.mp3")).filter(|p| p.is_file());
    let mut stats = flight.stats.lock().expect("flight stats");
    if stats.kabinen_lauf.lied.datei.is_none() {
        stats.kabinen_lauf.lied.datei = lied_auf_platte;
    }
    stats.kabinen_lauf.lied.platte_geprueft = true;
    let (tun, holen) = musik_schritt(&mut stats, snap, &flight.aircraft_icao, eigene, now.timestamp());
    if let Some(l) = &holen {
        let details = serde_json::json!({ "titel": l.titel, "stil": l.stil, "versuch": stats.kabinen_lauf.lied.versuche + 1 });
        notiz(&mut stats.kabinen_lauf, "lied_anfrage", None, None, details);
    }
    drop(stats);
    match tun {
        // Probehören (Kasten/Dialog) nicht überschreiben.
        MusikTun::Spielen(dateien) if !crate::kabinen_musik::probe_aktiv() => {
            crate::kabinen_musik::spielen(crate::kabinen_musik::Auftrag { dateien, schleife: true, geraet: einst.ausgabe.clone() });
        }
        MusikTun::Ausblenden if crate::kabinen_musik::flug_aktiv() => crate::kabinen_musik::ausblenden(),
        MusikTun::Aus if crate::kabinen_musik::flug_aktiv() => crate::kabinen_musik::aus(),
        _ => {}
    }
    if let Some(l) = holen {
        lied_holen(app.clone(), Arc::clone(flight), l);
    }
}

/// Holt das Lied vom Ton-Server (`POST /api/kabine/lied`) in den Ordner des
/// Fluges. Fehlschlag: nach `Retry-After` (sonst 60 s) erneut, solange noch
/// Boarding ist (Entscheidung 9).
fn lied_holen(app: AppHandle, flight: Arc<ActiveFlight>, lied: api_client::KabinenLied) {
    tauri::async_runtime::spawn(async move {
        let beginn = Utc::now();
        let ergebnis = lied_vom_server(&app, &flight, &lied).await;
        {
            let mut stats = flight.stats.lock().expect("flight stats");
            let lauf = &mut stats.kabinen_lauf;
            lauf.lied.laeuft = false;
            let details = match &ergebnis {
                Ok(p) => {
                    lauf.lied.datei = Some(p.clone());
                    lauf.lied.fehler = None;
                    serde_json::json!({ "ergebnis": "ok", "dauer_ms": (Utc::now() - beginn).num_milliseconds() })
                }
                Err((meldung, warte_s)) => {
                    lauf.lied.versuche += 1;
                    lauf.lied.naechster = Utc::now().timestamp() + *warte_s as i64;
                    lauf.lied.fehler = Some(meldung.clone());
                    serde_json::json!({ "ergebnis": "fehler", "fehler": meldung, "dauer_ms": (Utc::now() - beginn).num_milliseconds() })
                }
            };
            notiz(lauf, "lied_antwort", None, None, details);
        }
        protokoll_leeren(&app, &flight);
    });
}

async fn lied_vom_server(app: &AppHandle, flight: &ActiveFlight, lied: &api_client::KabinenLied) -> Result<PathBuf, (String, u64)> {
    let ordner = ton_ordner(app, &flug_ordner(flight)).map_err(|e| (e, 60))?;
    let ziel = ordner.join("boarding-lied.mp3");
    // Nach einem Neustart liegt es schon da.
    if std::fs::metadata(&ziel).is_ok_and(|m| m.len() > 0) {
        return Ok(ziel);
    }
    let token = crate::live_zugang::pilot_token().ok_or_else(|| ("noch kein Live-Zugang (nicht provisioniert)".to_string(), 120))?;
    let koerper = serde_json::json!({ "pirep_id": flight.pirep_id, "text": lied.text, "stil": lied.stil });
    let antwort = reqwest::Client::builder()
        // acemusic.ai braucht für 3 min Musik gut eine Minute, dazu die
        // Warteschlange des Servers.
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| (e.to_string(), 60))?
        .post(format!("{}/api/kabine/lied", crate::live_zugang::LIVE_BASIS))
        .bearer_auth(token)
        .header("User-Agent", format!("AeroACARS/{}", env!("CARGO_PKG_VERSION")))
        .json(&koerper)
        .send()
        .await
        .map_err(|e| (format!("live.kant.ovh nicht erreichbar: {e}"), 60))?;
    let status = antwort.status();
    if !status.is_success() {
        let warte = piper::warte_s(antwort.headers().get("retry-after").and_then(|v| v.to_str().ok())).unwrap_or(60);
        return Err((format!("Ton-Server antwortet {status}"), warte.max(10)));
    }
    let mp3 = antwort.bytes().await.map_err(|e| (e.to_string(), 60))?;
    if mp3.is_empty() {
        return Err(("leere Antwort".into(), 60));
    }
    let zwischen = ordner.join("boarding-lied.mp3.neu");
    std::fs::write(&zwischen, &mp3).and_then(|_| std::fs::rename(&zwischen, &ziel)).map_err(|e| (e.to_string(), 60))?;
    Ok(ziel)
}

/// Zustand der Boarding-Musik für den Kasten auf der Seite „Kabine“.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MusikAnsicht {
    /// Schalter „Boarding-Musik“.
    an: bool,
    lautstaerke: f32,
    /// aus | ohne_flug | keine | wird_erzeugt | kein_lied | bereit | laeuft |
    /// pausiert | beendet | probe
    stand: &'static str,
    /// eigene | lied
    quelle: Option<&'static str>,
    titel: Option<String>,
    strecke: Option<String>,
    /// Ordner der eigenen Musik (ICAO oder „Alle“) und Zahl der Stücke.
    ordner: Option<String>,
    stuecke: usize,
    /// Airline des laufenden Flugs (ICAO).
    airline: Option<String>,
    /// boarding | ankunft, solange die Musik läuft (Zusatz I).
    abschnitt: Option<&'static str>,
}

fn musik_ansicht(einst: &Einstellungen, lauf: Option<&Lauf>, airline: Option<String>, laeuft: bool, hoerbar: bool, probe: bool) -> MusikAnsicht {
    let mut a = MusikAnsicht {
        an: einst.boarding_musik,
        lautstaerke: einst.musik_lautstaerke,
        stand: "ohne_flug",
        quelle: None,
        titel: None,
        strecke: None,
        ordner: None,
        stuecke: 0,
        airline,
        abschnitt: None,
    };
    if let Some(lauf) = lauf {
        match &lauf.musik_quelle {
            Some(MusikQuelle::Eigene { ordner, dateien }) => {
                a.quelle = Some("eigene");
                a.ordner = Some(ordner.clone());
                a.stuecke = dateien.len();
            }
            Some(MusikQuelle::Lied) => {
                a.quelle = Some("lied");
                a.titel = lauf.lied.vorlage.as_ref().and_then(|l| l.titel.clone());
                a.strecke = lauf.lied.vorlage.as_ref().and_then(|l| l.strecke.clone());
            }
            _ => {}
        }
        if laeuft {
            a.abschnitt = lauf.musik_abschnitt.map(|x| match x {
                cabin_announcements::musik::Abschnitt::Boarding => "boarding",
                cabin_announcements::musik::Abschnitt::Ankunft => "ankunft",
            });
        }
        a.stand = match &lauf.musik_quelle {
            _ if probe => "probe",
            _ if laeuft => if hoerbar { "laeuft" } else { "pausiert" },
            // Zwischen „Türen zu“ und der Landung wartet sie auf die Ankunft.
            _ if lauf.musik_fenster.ankunft_vorbei() => "beendet",
            None => "ohne_flug",
            Some(MusikQuelle::Keine) => "keine",
            // „wird erzeugt“ nur, solange wirklich etwas unterwegs ist (Thomas
            // 19.10.2026: Update mitten im Flug zeigte es ohne Anfrage).
            Some(MusikQuelle::Lied) if lauf.lied.datei.is_none() => {
                let unterwegs = lauf.lied.fehler.is_none() && (lauf.lied.angefragt || lauf.lied.laeuft || lauf.lied.nochmal_ab.is_some());
                if unterwegs { "wird_erzeugt" } else { "kein_lied" }
            }
            Some(_) => "bereit",
        };
    } else if probe {
        a.stand = "probe";
    }
    if !einst.boarding_musik {
        a.stand = "aus";
    }
    a
}

#[tauri::command]
pub(crate) fn kabinen_musik_zustand(app: AppHandle, state: tauri::State<'_, AppState>) -> MusikAnsicht {
    let einst = einstellungen(&app);
    let flug = aktiver_flug(&state);
    let (laeuft, hoerbar, probe) = (crate::kabinen_musik::flug_aktiv(), crate::kabinen_musik::hoerbar(), crate::kabinen_musik::probe_aktiv());
    match flug {
        Some(f) => {
            let stats = f.stats.lock().expect("flight stats");
            musik_ansicht(&einst, Some(&stats.kabinen_lauf), Some(f.airline_icao.clone()).filter(|a| !a.is_empty()), laeuft, hoerbar, probe)
        }
        None => musik_ansicht(&einst, None, None, false, false, probe),
    }
}

/// Probehören im Kasten: die Musik dieses Flugs (Lied oder eigene), einmal.
#[tauri::command]
pub(crate) fn kabinen_musik_probe_flug(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), UiError> {
    let flug = aktiver_flug(&state).ok_or_else(|| UiError::new("kein_flug", "no active flight"))?;
    let dateien: Vec<PathBuf> = {
        let stats = flug.stats.lock().expect("flight stats");
        match &stats.kabinen_lauf.musik_quelle {
            Some(MusikQuelle::Eigene { dateien, .. }) => dateien.clone(),
            Some(MusikQuelle::Lied) => stats.kabinen_lauf.lied.datei.iter().cloned().collect(),
            _ => Vec::new(),
        }
    };
    if dateien.is_empty() {
        return Err(UiError::new("musik_keine", "no music for this flight yet"));
    }
    crate::kabinen_musik::spielen(crate::kabinen_musik::Auftrag { dateien, schleife: false, geraet: einstellungen(&app).ausgabe });
    Ok(())
}

/// „Boarding-Musik laden“ (Thomas 19.10.2026): Ist für diesen Flug keine
/// Boarding-Musik da, stößt der Pilot das Laden von Hand neu an. Der nächste
/// Takt fragt den Text an bzw. holt die Musikdatei.
#[tauri::command]
pub(crate) fn kabinen_musik_laden(state: tauri::State<'_, AppState>) -> Result<(), UiError> {
    let flug = aktiver_flug(&state).ok_or_else(|| UiError::new("kein_flug", "no active flight"))?;
    let mut stats = flug.stats.lock().expect("flight stats");
    musik_neu_laden(&mut stats.kabinen_lauf).map_err(|grund| UiError::new(grund, "boarding music cannot be loaded now"))
}

fn musik_neu_laden(lauf: &mut Lauf) -> Result<(), &'static str> {
    if lauf.musik_quelle != Some(MusikQuelle::Lied) {
        return Err("musik_keine");
    }
    if lauf.musik_fenster.ankunft_vorbei() {
        return Err("musik_vorbei");
    }
    if lauf.lied.datei.is_some() || lauf.lied.laeuft {
        return Ok(());
    }
    lauf.lied.fehler = None;
    lauf.lied.naechster = 0;
    lauf.lied.versuche = 0;
    if lauf.lied.vorlage.is_none() {
        // Text neu anfragen: über den Teil „lied“.
        lauf.lied.angefragt = false;
        lauf.lied.nachgefragt = false;
        lauf.lied.nochmal_ab = None;
        lauf.angefragt.remove("lied");
        lauf.fehlversuche.remove("lied");
    }
    Ok(())
}

fn takt_schritt(
    app: &AppHandle,
    flight: &Arc<ActiveFlight>,
    client: &Client,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
) {
    let einst = einstellungen(app);
    let jetzt = now.timestamp();
    if !einst.an {
        // Auch „aus“ ist eine Eingangssperre fürs Protokoll (1d).
        sperre_pruefen(&mut flight.stats.lock().expect("flight stats"), false, jetzt);
        return;
    }
    // Eigene Musik vor der Abflug-Anfrage prüfen (Design §5), ohne Sperre.
    let eigene = if einst.boarding_musik { eigene_lesen(flight) } else { None };
    // Verlaufsdatei ohne Flugdaten-Sperre lesen (v2.0.28: das Lesen hielt die
    // Sperre beim ersten Takt bis 39 ms, Diagnose Pilot 1 und 4).
    let geladen = flight.stats.lock().expect("flight stats").kabinen_lauf.verlauf_geladen;
    let alt = if geladen { None } else { Some(verlauf_datei_lesen(app, flight)) };
    let mut stats = flight.stats.lock().expect("flight stats");
    if let Some(alt) = alt {
        verlauf_uebernehmen(&mut stats.kabinen_lauf, alt);
    }
    // Sim angehalten oder im Slew: Der Takt steht, die Seite sagt es.
    stats.kabinen_lauf.sim_angehalten = snap.paused || snap.slew_mode;
    // 1d: Wechsel der Sperre (Pause, Replay, Freischaltung, Passagierzahl,
    // Flugart) ins Protokoll — vor den Rücksprüngen unten.
    sperre_pruefen(&mut stats, true, jetzt);
    if stats.kabinen_lauf.sim_angehalten || stats.replay_verdacht {
        return;
    }

    // ---- Freischaltung durch den Server (vor der Passagierzahl: die Seite
    // soll früh wissen, ob das Konto freigeschaltet ist) ----
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
                    Ok(s) if s.beta => {
                        lauf.beta = Beta::Ja;
                        lauf.beta_netzfehler = false;
                        lauf.kontingent_erschoepft = !s.bereit;
                    }
                    Ok(_) => {
                        lauf.beta = Beta::Nein;
                        lauf.beta_netzfehler = false;
                    }
                    Err(e) => {
                        // Netz weg: später noch einmal fragen, bis dahin aus.
                        tracing::info!(error = %e, "kabinenansagen: status nicht erreichbar");
                        lauf.beta = Beta::Unbekannt;
                        lauf.beta_netzfehler = true;
                        lauf.beta_naechste_pruefung = Utc::now().timestamp() + 300;
                    }
                }
            });
            return;
        }
    }

    // ---- Passagierzahl: beim Wiedereinstieg ohne Netz später nachholen ----
    if stats.pax_count.is_none() {
        let lauf = &mut stats.kabinen_lauf;
        if lauf.pax_versuche < 10 && jetzt >= lauf.pax_naechster {
            lauf.pax_versuche += 1;
            lauf.pax_naechster = jetzt + 120;
            let (flight, client) = (Arc::clone(flight), client.clone());
            tauri::async_runtime::spawn(async move {
                if let Some((bids, ofp)) = crate::kabinen_eckdaten_holen(&client, flight.bid_id).await {
                    let mut stats = flight.stats.lock().expect("flight stats");
                    crate::kabinen_eckdaten_nachholen_aus(&mut stats, &bids, flight.bid_id, ofp.as_ref());
                }
            });
        }
        return;
    }

    // ---- Planer ----
    let flugdaten = crate::kabinen_flugdaten(&stats, &flight.arr_airport, flight.planned_flight_time_min, &flight.aircraft_icao);
    let flugart = flugdaten.flugart.unwrap_or(Flugart::Aus);
    if flugart == Flugart::Aus {
        return;
    }
    // Ist der Schalter aus, bleibt die Quelle offen: Wer ihn im Boarding
    // einschaltet, bekommt noch eigene Musik.
    if stats.kabinen_lauf.musik_quelle.is_none() && einst.boarding_musik {
        stats.kabinen_lauf.musik_quelle = Some(musik_quelle(flugart, true, eigene));
    }
    let abgewaehlt = einst.abgewaehlt();
    // Abwahl ins Log (einmal je Flug und bei jeder Änderung, auch der
    // Schalter für Kommandos und Gong), damit sich „abgewählt“ von „kam
    // nicht“ unterscheiden lässt.
    let mut liste = abgewaehlt.clone();
    if !einst.kommandos {
        liste.extend(KOMMANDOS.into_iter().filter(|s| *s != Slot::BoardingAbgeschlossen));
    }
    if !einst.chimes {
        liste.extend(GONGS);
    }
    if stats.kabinen_lauf.abgewaehlt_zuletzt.as_ref() != Some(&liste) {
        let details = serde_json::json!({ "slots": liste.iter().map(|s| slot_name(*s)).collect::<Vec<_>>() });
        notiz(&mut stats.kabinen_lauf, "abgewaehlt", None, None, details);
        stats.kabinen_lauf.abgewaehlt_zuletzt = Some(liste);
    }
    let mut verlauf_neu_vorab = false;
    let schritt = crate::kabinen_schritt(&mut stats, snap, now, &flight.arr_airport, flight.planned_flight_time_min, &flight.aircraft_icao, &abgewaehlt);
    stats.kabinen_lauf.ruhephase = schritt.ruhephase;
    // Kam die Art „zweiter Service“ erst, als Reiseflug 80 schon nach der
    // 12-min-Regel wartete, und reicht die Zeit nicht mehr (unter 60 min
    // Restflug): entfällt statt kurz vor dem Sinkflug (Codex 2.0.28).
    let rest = stats.kabinen_planer.as_ref().and_then(|p| p.restflug_min(jetzt, schritt.rest_min));
    if zweiter_service_zu_spaet(&stats.kabinen_lauf, rest) && stats.kabinen_lauf.warteschlange.entfernen(Slot::Reiseflug80) {
        notiz(&mut stats.kabinen_lauf, "entfallen", Some(Slot::Reiseflug80), None, serde_json::json!({ "grund": "zu_wenig_zeit" }));
        verbuchen(&mut stats.kabinen_lauf, Slot::Reiseflug80, "entfallen", Some("zu_wenig_zeit"), jetzt);
        verlauf_neu_vorab = true;
    }
    hinweis_pruefen(&mut stats, snap, schritt.phase, jetzt);
    // Altes Modul: Ist Reiseflug 80 das Ende des Bordverkaufs, entfällt eine
    // schon wartende eigene „Ende des Service“-Ansage (keine Doppelung,
    // auch wenn die Art erst später ankommt — Codex 2.1.0).
    verlauf_neu_vorab |= ende_service_bei_alter_art(&mut stats.kabinen_lauf, jetzt);
    let mut verlauf_neu = verlauf_neu_vorab;
    // Was der Planer nicht meldet, obwohl es dran gewesen wäre: in „Bisher“.
    let entfallen = stats.kabinen_planer.as_mut().map(|p| p.entfallene_abholen()).unwrap_or_default();
    for (slot, grund) in entfallen {
        let grund = match grund {
            cabin_announcements::Entfall::ZuWenigZeit => "zu_wenig_zeit",
            cabin_announcements::Entfall::Uebersprungen => "uebersprungen",
        };
        notiz(&mut stats.kabinen_lauf, "entfallen", Some(slot), None, serde_json::json!({ "grund": grund }));
        verbuchen(&mut stats.kabinen_lauf, slot, "entfallen", Some(grund), jetzt);
        verlauf_neu = true;
    }
    let mut faellig: Vec<Faellig> = schritt
        .faellig
        .into_iter()
        .filter(|f| match f.slot.rolle() {
            Rolle::Chime => einst.chimes,
            // „Boarding completed“ ist eine Kabinenmeldung mit eigener Abwahl,
            // kein Crew-Kommando (Codex 2.0.28).
            Rolle::Kommando => einst.kommandos || f.slot == Slot::BoardingAbgeschlossen,
            _ => true,
        })
        .collect();
    for f in &faellig {
        let details = serde_json::json!({
            "phase": schritt.phase,
            "hoehe_ft": snap.altitude_msl_ft.round(),
            "agl_ft": snap.altitude_agl_ft.round(),
            "vs_fpm": snap.vertical_speed_fpm.round(),
            "am_boden": snap.on_ground,
            "anschnallzeichen": snap.seatbelts_sign,
            "beacon": snap.light_beacon,
            "landelicht": snap.light_landing,
            "fahrwerk": snap.gear_position,
            "triebwerke": snap.engines_running,
            "zfw_kg": stats.last_zfw_kg,
            "zfw_plan_kg": stats.planned_zfw_kg,
            "ruhephase": schritt.ruhephase,
            "trotz_ruhephase": f.trotz_ruhephase,
            "rest_min": schritt.rest_min.map(f64::round),
        });
        notiz(&mut stats.kabinen_lauf, "gemeldet", Some(f.slot), None, details);
    }
    // Stumm: Fälliges wird gleich als übersprungen verbucht, nicht eingereiht
    // (kommt auch nach dem Aufheben nicht nach — Thomas 18.10.2026).
    if STUMM.load(Ordering::SeqCst) {
        verlauf_neu |= stumm_anwenden(&mut stats, jetzt);
        let mut kapitaen = false;
        for f in faellig.drain(..) {
            notiz(&mut stats.kabinen_lauf, "stumm", Some(f.slot), None, serde_json::Value::Null);
            verbuchen(&mut stats.kabinen_lauf, f.slot, "stumm", None, jetzt);
            kapitaen |= f.slot == Slot::KapitaenSinkflug;
            verlauf_neu = true;
        }
        if kapitaen {
            if let Some(p) = stats.kabinen_planer.as_mut() {
                p.kapitaen_sinkflug_ende(false, jetzt);
            }
        }
    }

    // ---- Texte holen: nur, was auf diesem Flug noch kommt ----
    let mut auftraege: Vec<(String, Vec<Slot>, u32)> = Vec::new();
    {
        stats.kabinen_lauf.warteschlange.aufnehmen(faellig.clone());
        let wartend = stats.kabinen_lauf.warteschlange.wartend();
        let kommt = |s: &Slot| flugart.erlaubt(*s) && kommt_noch(&stats, &wartend, *s);
        let abflug: Vec<Slot> = ABFLUG.into_iter().filter(kommt).collect();
        let ankunft: Vec<Slot> = ANKUNFT.into_iter().filter(kommt).collect();
        let halbzeit_kommt = kommt(&Slot::Reiseflug50);
        let halbzeit_um = stats.kabinen_planer.as_ref().and_then(|p| p.zeitpunkt(Slot::Reiseflug50, stats.landung_erwartet));
        let r = cabin_announcements::planer::rang(schritt.phase);
        let kurz = stats.kabinen_planer.as_ref().is_some_and(|p| p.ist_kurzstrecke());
        // Ankunft: in der Luft und kurz vor der Landung (bzw. im Sinkflug).
        // Kurzstrecke: gleich nach dem Abheben, sonst bliebe zu wenig Zeit.
        let ankunft_jetzt = r >= cabin_announcements::planer::rang(FlightPhase::Takeoff)
            && (kurz
                || schritt.rest_min.is_some_and(|m| m <= ANKUNFT_TEXT_VOR_LANDUNG_MIN as f64)
                || r >= cabin_announcements::planer::rang(FlightPhase::Descent));
        let lauf = &mut stats.kabinen_lauf;
        if !abflug.is_empty() && darf_anfragen(lauf, "abflug", jetzt) {
            lauf.angefragt.insert("abflug".into());
            auftraege.push(("abflug".into(), abflug, 0));
        }
        // Halbzeit (Zusatz H): im Reiseflug, Vorlauf vor ihrem Zeitpunkt. Ein
        // Flug ohne Halbzeit-Ansage (Kurzstrecke) fragt nie an.
        let halbzeit_jetzt = halbzeit_anfragen(halbzeit_kommt, schritt.phase, halbzeit_um, jetzt);
        if halbzeit_jetzt && darf_anfragen(lauf, "halbzeit", jetzt) {
            lauf.angefragt.insert("halbzeit".into());
            auftraege.push(("halbzeit".into(), HALBZEIT.to_vec(), 0));
        }
        if ankunft_jetzt && !ankunft.is_empty() && darf_anfragen(lauf, "ankunft", jetzt) {
            lauf.angefragt.insert("ankunft".into());
            auftraege.push(("ankunft".into(), ankunft, 0));
        }
        // Lied nachfragen, wenn das Modul keine Zeit mehr hatte (einmal). Ohne
        // Abflug-Slots (Lied kam ueber den Teil „lied“) wieder ueber „lied“.
        if lauf.lied.nochmal_ab.is_some_and(|ab| jetzt >= ab) && lauf.lied.vorlage.is_none() {
            lauf.lied.nochmal_ab = None;
            if lauf.lied.slots.is_empty() {
                auftraege.push(("lied".into(), Vec::new(), 0));
            } else {
                auftraege.push(("abflug:lied".into(), lauf.lied.slots.clone(), 0));
            }
        }
        // Update oder Neustart mitten im Flug: Kein Abflug-Drehbuch traegt das
        // Lied mehr, also eine eigene Anfrage „lied“ (Modul ab 2.1.3), solange
        // die Musik noch kommt (Thomas 19.10.2026).
        let abflug_dabei = auftraege.iter().any(|(s, _, _)| s.starts_with("abflug"));
        if lied_einzeln_anfragen(lauf, einst.boarding_musik, schritt.phase, abflug_dabei, jetzt) {
            lauf.angefragt.insert("lied".into());
            auftraege.push(("lied".into(), Vec::new(), 0));
        }
        for f in faellig.iter().filter(|f| ist_ereignis(f.slot)) {
            lauf.anlass += 1;
            let schluessel = format!("ereignis:{}", lauf.anlass);
            lauf.angefragt.insert(schluessel.clone());
            auftraege.push((schluessel, vec![f.slot], lauf.anlass));
        }
    }
    for (schluessel, slots, anlass) in auftraege {
        let mut anfrage = anfrage_bauen(&stats, flight, &einst, &schluessel, &slots, anlass, jetzt);
        if schluessel == "halbzeit" {
            // Restzeit zum Abspielzeitpunkt, nicht zum Zeitpunkt der Anfrage
            // (sonst stimmte „noch etwa zwei Stunden“ um den Vorlauf nicht).
            let ab = stats.kabinen_planer.as_ref().and_then(|p| p.zeitpunkt(Slot::Reiseflug50, stats.landung_erwartet)).unwrap_or(jetzt).max(jetzt);
            anfrage["flug"]["rest_min"] = serde_json::json!(stats.landung_erwartet.map(|t| ((t - ab) / 60).max(0)));
            // „Früher, pünktlich, später“ gilt hier für die Ankunft, nicht für
            // den Off-Block (Codex 2.1.1): erwartete gegen geplante Ankunft.
            if let (Some(an), Some(plan)) = (anfrage["flug"]["ankunft_erwartet"].as_i64(), stats.ofp_planzeiten.plan_on_block) {
                anfrage["flug"]["abweichung_min"] = serde_json::json!((an - plan).div_euclid(60).clamp(-1440, 1440));
            }
        }
        // Boarding-Lied (2.1.1 G1): mit dem Abflug-Drehbuch, nur wenn keine
        // eigene Musik passt und der Schalter an ist — und nur bis „Türen zu“.
        if (schluessel.starts_with("abflug") || schluessel == "lied") && lied_anfragen(&stats.kabinen_lauf, einst.boarding_musik, schritt.phase) {
            anfrage["boarding_lied"] = serde_json::json!(true);
            stats.kabinen_lauf.lied.angefragt = true;
            stats.kabinen_lauf.lied.slots = slots.clone();
        }
        holen(app.clone(), Arc::clone(flight), client.clone(), schluessel, anfrage, slots, false, flugart == Flugart::Bizjet);
    }

    // ---- Ton: das Dringendste zuerst, höchstens zwei zugleich ----
    ton_pumpe(app, flight, &mut stats, &einst, flugart, jetzt);

    // ---- Abspielen ----
    // Kapitänsbegrüßung nach dem Start erst über 10.000 ft bzw. im Reiseflug
    // (auch bei Flügen ohne Ruhephase, Advisor beta.7).
    let tief_nach_start = !snap.on_ground
        && snap.altitude_msl_ft < 10_000.0
        && cabin_announcements::planer::rang(schritt.phase) < cabin_announcements::planer::rang(FlightPhase::Cruise);
    let (gespielt, neu) = abspielen_schritt(&mut stats, jetzt, schritt.phase, schritt.ruhephase, tief_nach_start);
    verlauf_neu |= neu;
    wartegruende_notieren(&mut stats.kabinen_lauf, jetzt);
    if let Some(g) = &gespielt {
        let details = serde_json::json!({
            "dauer_s": g.teile.iter().map(|t| t.dauer_s).sum::<f64>(),
            "sprachen": g.teile.iter().map(|t| t.sprache.clone()).collect::<Vec<_>>(),
            "phase": schritt.phase,
            "hoehe_ft": snap.altitude_msl_ft.round(),
        });
        notiz(&mut stats.kabinen_lauf, "spielt", Some(g.slot), None, details);
    }
    let text = verlauf_neu.then(|| verlauf_text(&stats.kabinen_lauf)).flatten();
    drop(stats);
    // Datei ohne Flugdaten-Sperre schreiben (v2.0.28); das Log schreibt `takt`.
    if let Some(text) = text {
        verlauf_schreiben(app, flight, &text);
    }
    let Some(gespielt) = gespielt else { return };
    tracing::info!(slot = ?gespielt.slot, "kabinenansage spielt");
    // 1b: Das Backend spielt die Ansage selbst und trägt ihr Ende ein; das
    // Ereignis dient nur noch der Anzeige. Eine schlafende Oberfläche (macOS
    // drosselt WKWebView im Hintergrund) hält so nichts mehr auf.
    if let Err(e) = app.emit(EREIGNIS_ANSAGE, gespielt.clone()) {
        tracing::warn!(error = %e, "kabinenansage: Anzeige-Ereignis nicht zustellbar");
    }
    ablauf_starten(app, flight, gespielt.id);
}

/// Hinweis an den Piloten (Thomas 18.10.2026): Steht das Anschnallzeichen im
/// Reiseflug nach drei Minuten noch auf AN, wartet der Service — wie in echt,
/// aber die Seite und das Cockpit sagen es.
fn hinweis_pruefen(stats: &mut FlightStats, snap: &SimSnapshot, phase: FlightPhase, jetzt: i64) {
    const NACH_S: i64 = 180;
    let aus_schon = stats.kabinen_planer.as_ref().is_none_or(|p| p.erledigt(Slot::AnschnallzeichenAus));
    let lauf = &mut stats.kabinen_lauf;
    if phase == FlightPhase::Cruise {
        let seit = *lauf.reiseflug_seit.get_or_insert(jetzt);
        lauf.hinweis = (!aus_schon && snap.seatbelts_sign == Some(2) && jetzt - seit >= NACH_S).then_some("anschnallzeichen");
    } else {
        lauf.reiseflug_seit = None;
        lauf.hinweis = None;
    }
}

/// Was im Takt abgespielt wird, und alles, was dabei entfällt (in „Bisher“
/// mit Grund). `true` = Verlauf geändert. Ohne AppHandle, damit es testbar ist.
fn abspielen_schritt(stats: &mut FlightStats, jetzt: i64, phase: FlightPhase, ruhephase: bool, tief_nach_start: bool) -> (Option<Gespielt>, bool) {
    let mut verlauf_neu = false;
    let mut kapitaen_kommt_nicht = false;
    let lauf = &mut stats.kabinen_lauf;
    // Sicher entfallende Slots (Bizjet ohne Kurzfassung) nicht warten lassen.
    let mut weg: Vec<(Slot, String)> = lauf.entfaellt.iter().map(|(s, g)| (*s, g.clone())).collect();
    // 2.1.1: Begrüßung und „Boarding completed“ halten „Türen zu“ und die
    // Sicherheit auf, bis sie gespielt sind. Kommt ihr Ton sicher nie, gehen
    // sie heraus, statt alles bis zum Startlauf aufzuhalten.
    for slot in [Slot::Boarding, Slot::BoardingAbgeschlossen] {
        if lauf.warteschlange.wartend().contains(&slot) && ton_kommt_nie(lauf, slot) {
            weg.push((slot, "keine_stimme".into()));
        }
    }
    for (slot, grund) in weg {
        if lauf.warteschlange.entfernen(slot) {
            notiz(lauf, "entfallen", Some(slot), None, serde_json::json!({ "grund": grund }));
            verbuchen(lauf, slot, "entfallen", Some(&grund), jetzt);
            verlauf_neu = true;
        }
    }
    // Ohne Startmeldung der Oberfläche: Ausgang unbekannt, nicht ewig „läuft“.
    for g in lauf.verlauf.iter_mut().filter(|g| g.ergebnis.is_none() && !g.vorschau && !g.gestartet && jetzt - g.zeit > START_SPAETESTENS_S) {
        g.ergebnis = Some("unbekannt".into());
        kapitaen_kommt_nicht |= g.slot == Slot::KapitaenSinkflug;
        lauf.warteschlange.frei();
        verlauf_neu = true;
    }
    // Gestartet, aber das Backend meldet kein Ende (Gerät hängt): nach Tondauer
    // plus Frist „unbekannt“, damit die Kabine nicht für den Rest des Flugs steht.
    for g in lauf.verlauf.iter_mut().filter(|g| g.ergebnis.is_none() && !g.vorschau && g.gestartet && jetzt - g.zeit > dauer_s(g.slot, &g.teile).ceil() as i64 + ABLAUF_FRIST_S) {
        g.ergebnis = Some("unbekannt".into());
        kapitaen_kommt_nicht |= g.slot == Slot::KapitaenSinkflug;
        lauf.warteschlange.frei();
        verlauf_neu = true;
    }
    // 1b: Solange eine Ansage noch kein Ende hat, beginnt keine neue — das
    // Backend spielt sie, ihr Ende steht nach dem letzten Ton fest. Vorher
    // gab nur `belegt_bis` (geschätzte Dauer) die nächste frei, und ein
    // spätes Ende der Oberfläche setzte die Belegung der neuen Ansage zurück.
    let offen = laeuft(lauf);
    let sprachen = lauf.sprachen.clone();
    let (texte, toene, fehlt, fehlen) = (&lauf.texte, &lauf.ton, &lauf.texte_fehler, &lauf.texte_fehlen);
    let bereit = |slot: Slot| ansage_bereit(texte, toene, fehlt, fehlen, &sprachen, slot) && !(tief_nach_start && slot == Slot::KapitaenBegruessung);
    let naechstes = if offen { None } else { lauf.warteschlange.naechstes_bereit(jetzt, phase, ruhephase, bereit) };
    // Verfallene in jedem Takt abholen (nicht nur, wenn nichts spielt) und in
    // „Bisher“ mit Grund zeigen: Ton nicht rechtzeitig fertig oder zu spät dran.
    for weg in lauf.warteschlange.verfallene_abholen() {
        let grund = if ansage_bereit(&lauf.texte, &lauf.ton, &lauf.texte_fehler, &lauf.texte_fehlen, &lauf.sprachen, weg) { "zu_spaet" } else { "ton_nicht_fertig" };
        notiz(lauf, "verfallen", Some(weg), None, serde_json::json!({ "grund": grund }));
        verbuchen(lauf, weg, "entfallen", Some(grund), jetzt);
        verfallen_aufraeumen(lauf, weg);
        kapitaen_kommt_nicht |= weg == Slot::KapitaenSinkflug;
        verlauf_neu = true;
    }
    let gespielt = naechstes.and_then(|slot| match teile_von(lauf, slot) {
        Some(teile) => {
            // Zwischen den Sprachen eine kurze Pause; ein Chime dauert gut zwei Sekunden.
            let dauer = dauer_s(slot, &teile);
            let bis = jetzt + dauer.ceil() as i64 + 1;
            // Nur Durchsagen an die Gäste lösen die Pause aus, nicht Crew-
            // Kommandos und Gong (sonst wartete die Begrüßung 90 s auf ein
            // 5-s-Kommando, Pilotendurchlauf beta.6).
            if slot.loest_pause_aus() {
                lauf.warteschlange.ansage_spielt_bis(bis);
            } else {
                lauf.warteschlange.spielt_bis(bis);
            }
            lauf.naechste_id += 1;
            let g = Gespielt {
                id: lauf.naechste_id,
                slot,
                rolle: rolle_von(lauf, slot),
                zeit: jetzt,
                teile,
                vorschau: false,
                ergebnis: None,
                gestartet: false,
                grund: None,
            };
            lauf.verlauf.push(g.clone());
            verlauf_neu = true;
            Some(g)
        }
        None => {
            notiz(lauf, "entfallen", Some(slot), None, serde_json::json!({ "grund": "keine_stimme" }));
            verbuchen(lauf, slot, "entfallen", Some("keine_stimme"), jetzt);
            kapitaen_kommt_nicht |= slot == Slot::KapitaenSinkflug;
            verlauf_neu = true;
            None
        }
    });
    if kapitaen_kommt_nicht {
        if let Some(p) = stats.kabinen_planer.as_mut() {
            p.kapitaen_sinkflug_ende(false, jetzt);
        }
    }
    (gespielt, verlauf_neu)
}

/// Ton für alle Sprachen mit Text da (Kommando: Englisch, Gong: immer)?
type Texte = HashMap<(Slot, String), String>;
type Toene = HashMap<(Slot, String), Option<Ton>>;

/// Bereit heißt: Jede Sprache des Fluges hat Ton (oder keine Stimme). Eine
/// Sprache ohne Text zählt nur dann nicht mit, wenn ihr Text endgültig
/// fehlt (`texte_fehler`) — sonst spielte eine fällige Ansage einsprachig,
/// während die zweite Sprache noch nachgeholt wird (Codex-QS beta.5). Seit
/// 1f zählt auch eine einzelne Sprache, die die Nachholung nicht brachte
/// (`fehlen`), als endgültig fehlend.
fn ansage_bereit(texte: &Texte, ton: &Toene, texte_fehler: &std::collections::BTreeMap<Slot, String>, fehlen: &Fehlen, sprachen: &[String], slot: Slot) -> bool {
    match slot.rolle() {
        Rolle::Chime => true,
        Rolle::Kommando => ton.contains_key(&(slot, "en-GB".to_string())),
        _ => {
            let mit_text = sprachen.iter().filter(|sp| texte.contains_key(&(slot, (*sp).clone()))).count();
            mit_text > 0
                && sprachen.iter().all(|sp| {
                    let k = (slot, sp.clone());
                    if texte.contains_key(&k) { ton.contains_key(&k) } else { texte_fehler.contains_key(&slot) || fehlen.contains_key(&k) }
                })
        }
    }
}

/// Die Teile einer Ansage mit Ton; `None`, wenn keine Sprache eine Stimme
/// hat (dann entfällt sie). Ein Gong hat keine Teile.
fn teile_von(lauf: &Lauf, slot: Slot) -> Option<Vec<Teil>> {
    if slot.rolle() == Rolle::Chime {
        return Some(Vec::new());
    }
    let sprachen: Vec<String> = if slot.rolle() == Rolle::Kommando { vec!["en-GB".into()] } else { lauf.sprachen.clone() };
    let teile: Vec<Teil> = sprachen
        .iter()
        .filter_map(|sp| {
            let ton = lauf.ton.get(&(slot, sp.clone()))?.as_ref()?;
            let text = lauf
                .texte
                .get(&(slot, sp.clone()))
                .cloned()
                .or_else(|| slot.kommando_text().map(str::to_string))?;
            Some(Teil { sprache: sp.clone(), text, datei: ton.datei.clone(), dauer_s: ton.dauer_s, stuecke: ton.stuecke.clone() })
        })
        .collect();
    (!teile.is_empty()).then_some(teile)
}

fn dauer_s(slot: Slot, teile: &[Teil]) -> f64 {
    if slot.rolle() == Rolle::Chime {
        2.5
    } else {
        teile.iter().map(|t| t.dauer_s + 0.8).sum()
    }
}

/// Kommt für diesen Slot sicher kein Ton mehr? Je Sprache: keine Stimme,
/// endgültig abgelehnt, oder kein Text und das Abflug-Drehbuch ist nach drei
/// Versuchen aufgegeben. Ohne bekannte Sprachen: nein (es kommt noch).
fn ton_kommt_nie(lauf: &Lauf, slot: Slot) -> bool {
    let sprachen: Vec<String> = if slot.rolle() == Rolle::Kommando { vec!["en-GB".into()] } else { lauf.sprachen.clone() };
    let text_aufgegeben = lauf.texte_fehler.contains_key(&slot)
        && !lauf.angefragt.contains("abflug")
        && lauf.fehlversuche.get("abflug").is_some_and(|(n, _)| *n >= 3);
    !sprachen.is_empty()
        && sprachen.iter().all(|sp| {
            let k = (slot, sp.clone());
            match lauf.ton.get(&k) {
                Some(None) => true,
                Some(Some(_)) => false,
                None => {
                    lauf.ton_endgueltig.contains_key(&k)
                        || (!lauf.texte.contains_key(&k) && slot.rolle() != Rolle::Kommando && (text_aufgegeben || lauf.texte_fehlen.contains_key(&k)))
                }
            }
        })
}

/// Eine verfallene Ansage hinterlässt keine Fehler, die später noch die
/// Seite bestimmen (Codex-QS beta.4, Punkt 19).
fn verfallen_aufraeumen(lauf: &mut Lauf, slot: Slot) {
    lauf.texte_fehler.remove(&slot);
    lauf.texte_fehlen.retain(|(s, _), _| *s != slot);
    lauf.ton_fehler.retain(|(s, _), _| *s != slot);
}

/// Frist über die Tondauer hinaus, nach der eine gestartete Ansage ohne
/// Ende als „unbekannt“ gilt (Gerät hängt; Bluetooth-Nachlauf ≤ 2 s,
/// Pausen zwischen den Sprachen 0,7 s je Teil).
const ABLAUF_FRIST_S: i64 = 30;

/// Hat eine ausgegebene Ansage (kein Vorhören) noch kein Ende? Dann spielt
/// sie oder beginnt gleich (1b), und es darf nichts Neues anfangen (1a:
/// die Abgabe wartet darauf).
fn laeuft(lauf: &Lauf) -> bool {
    lauf.verlauf.iter().any(|g| g.ergebnis.is_none() && !g.vorschau)
}

/// Wählt die nächsten Ton-Aufträge: fällige vor Vorrat, sonst in
/// Flugreihenfolge; nur, was noch kommt. Höchstens [`TON_GLEICHZEITIG`].
fn ton_pumpe(app: &AppHandle, flight: &Arc<ActiveFlight>, stats: &mut FlightStats, einst: &Einstellungen, flugart: Flugart, jetzt: i64) {
    let wartend = stats.kabinen_lauf.warteschlange.wartend();
    let kandidaten = ton_kandidaten(stats, einst.kommandos, flugart, jetzt);
    let landung = stats.landung_erwartet;
    for (slot, sprache, text, faellig) in ton_auswahl(kandidaten, &wartend, stats.kabinen_lauf.ton_laufend) {
        let gebraucht_in_s = if faellig {
            Some(0)
        } else {
            stats.kabinen_planer.as_ref().and_then(|p| p.zeitpunkt(slot, landung)).map(|t| t - jetzt)
        };
        ton_auftrag(app, flight, stats, slot, sprache, text, gebraucht_in_s, faellig);
    }
}

/// Fällige Ansagen gehen immer sofort raus; nur der Vorrat teilt sich
/// [`TON_GLEICHZEITIG`] Plätze. Der Server hält eine Vorrats-Anfrage offen,
/// bis ihr Ton dran ist — sonst verfiele eine Turbulenz-Ansage hinter zwei
/// wartenden Reiseflug-Tönen (Advisor beta.5). `laufend` = offener Vorrat.
fn ton_auswahl(
    kandidaten: Vec<(Slot, String, String)>,
    wartend: &[Slot],
    laufend: usize,
) -> Vec<(Slot, String, String, bool)> {
    let mut frei = TON_GLEICHZEITIG.saturating_sub(laufend);
    kandidaten
        .into_iter()
        .filter_map(|(s, sp, t)| {
            let faellig = wartend.contains(&s);
            if faellig {
                return Some((s, sp, t, true));
            }
            if frei == 0 {
                return None;
            }
            frei -= 1;
            Some((s, sp, t, false))
        })
        .collect()
}

/// Was als Nächstes Ton braucht, in Reihenfolge: fällige vor Vorrat, sonst
/// Flugreihenfolge, je Ansage die Sprachen in Sprechreihenfolge. Nur, was
/// auf diesem Flug noch kommt (Wiedereinstieg: nichts Vergangenes).
fn ton_kandidaten(stats: &FlightStats, kommandos: bool, flugart: Flugart, jetzt: i64) -> Vec<(Slot, String, String)> {
    let wartend = stats.kabinen_lauf.warteschlange.wartend();
    let lauf = &stats.kabinen_lauf;
    let mut kandidaten: Vec<(Slot, String, String)> = lauf
        .texte
        .iter()
        .map(|((s, sp), t)| (*s, sp.clone(), t.clone()))
        .collect();
    {
        kandidaten.extend(
            KOMMANDOS
                .into_iter()
                .filter(|s| kommandos || *s == Slot::BoardingAbgeschlossen)
                .filter(|s| flugart.erlaubt(*s))
                .filter_map(|s| s.kommando_text().map(|t| (s, "en-GB".to_string(), t.to_string()))),
        );
    }
    let sprach_rang = |sp: &str| lauf.sprachen.iter().position(|x| x == sp).unwrap_or(usize::MAX);
    kandidaten.retain(|(s, sp, _)| ton_noetig(lauf, &(*s, sp.clone()), jetzt) && kommt_noch(stats, &wartend, *s));
    kandidaten.sort_by_key(|(s, sp, _)| (!wartend.contains(s), *s, sprach_rang(sp)));
    kandidaten
}

/// Startet eine Ton-Anfrage und trägt das Ergebnis in den Lauf ein.
fn ton_auftrag(
    app: &AppHandle,
    flight: &Arc<ActiveFlight>,
    stats: &mut FlightStats,
    slot: Slot,
    sprache: String,
    text: String,
    gebraucht_in_s: Option<i64>,
    faellig: bool,
) {
    let lauf = &mut stats.kabinen_lauf;
    lauf.in_arbeit.insert((slot, sprache.clone()));
    if !faellig {
        lauf.ton_laufend += 1;
    }
    let rolle = rolle_von(lauf, slot);
    let maennlich = stimme_maennlich(lauf, rolle);
    let flug = flug_kennung(&flight.pirep_id);
    let fremd = lauf.fremd.get(&(slot, sprache.clone())).cloned().unwrap_or_default();
    notiz(lauf, "ton_anfrage", Some(slot), Some(&sprache), serde_json::json!({ "gebraucht_in_s": gebraucht_in_s, "faellig": faellig }));
    let ordner = flug_ordner(flight);
    let (app, flight) = (app.clone(), Arc::clone(flight));
    tauri::async_runtime::spawn(async move {
        let gestartet = Utc::now();
        let wunsch = Wunsch { slot: Some(slot), gebraucht_in_s, ordner: &ordner, flug: flug.as_deref(), rolle: server_rolle(rolle), fremd: &fremd };
        let ergebnis = sprechen(&app, maennlich, &sprache, &text, wunsch).await;
        let dauer_ms = (Utc::now() - gestartet).num_milliseconds();
        let mut stats = flight.stats.lock().expect("flight stats");
        let lauf = &mut stats.kabinen_lauf;
        let (ergebnis_art, meldung, ton_s, herkunft) = match &ergebnis {
            Ok(Some(t)) => ("ok", None, Some(t.dauer_s), Some(t.herkunft)),
            Ok(None) => ("keine_stimme", None, None, None),
            Err(f) if f.endgueltig => ("abgelehnt", Some(f.meldung.clone()), None, None),
            Err(f) => ("fehler", Some(f.meldung.clone()), None, None),
        };
        let details = serde_json::json!({
            "angefragt_um": gestartet.to_rfc3339(),
            "dauer_ms": dauer_ms,
            "gebraucht_in_s": gebraucht_in_s,
            "faellig": faellig,
            "ergebnis": ergebnis_art,
            "meldung": meldung,
            "ton_s": ton_s,
            "herkunft": herkunft,
        });
        notiz(lauf, "ton_antwort", Some(slot), Some(&sprache), details);
        let k = (slot, sprache);
        lauf.in_arbeit.remove(&k);
        if !faellig {
            lauf.ton_laufend = lauf.ton_laufend.saturating_sub(1);
        }
        // Kam inzwischen ein neuer Text (zweite Turbulenz o. ä.), gehört
        // dieser Ton zum alten: verwerfen, der neue wird angefragt.
        if lauf.texte.get(&k).is_some_and(|t| *t != text) {
            drop(stats);
            protokoll_leeren(&app, &flight);
            return;
        }
        match ergebnis {
            Ok(ton) => {
                lauf.ton_fehler.remove(&k);
                let ohne = ton.is_none().then(|| k.1.clone());
                lauf.ton.insert(k, ton);
                // Punkt 4: erstes „keine Stimme“ streicht die Sprache für den Flug.
                if let Some(sp) = ohne {
                    stimme_streichen(lauf, &sp);
                }
            }
            Err(f) if f.endgueltig => {
                tracing::warn!(slot = ?k.0, sprache = %k.1, error = %f.meldung, "kabinenansage: Ton abgelehnt");
                lauf.ton_endgueltig.insert(k, f.meldung);
            }
            Err(f) => {
                tracing::info!(slot = ?k.0, sprache = %k.1, error = %f.meldung, "kabinenansage: Ton gerade nicht erzeugbar");
                let versuche = lauf.ton_fehler.get(&k).map_or(0, |x| x.versuche) + 1;
                let ab = Utc::now().timestamp() + f.warte_s.clamp(5, 300) as i64;
                lauf.ton_fehler.insert(k, TonFehler { versuche, ab, meldung: f.meldung });
            }
        }
        drop(stats);
        protokoll_leeren(&app, &flight);
    });
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
    let flugdaten = crate::kabinen_flugdaten(stats, &flight.arr_airport, flight.planned_flight_time_min, &flight.aircraft_icao);
    let ankunft = ankunft_erwartet(
        stats.landung_erwartet,
        stats.takeoff_at.map(|t| t.timestamp()),
        flugdaten.geplante_flugzeit_min,
        plan.plan_on_block,
        abweichung,
        stats.block_off_at.is_some(),
    );
    let wetter = stats.arr_metar_snap.as_ref().and_then(|m| {
        m.temperature_c.map(|t| serde_json::json!({ "temperatur_c": t, "lage": wetterlage(m) }))
    });
    let mut anfrage = serde_json::json!({
        "pirep_id": flight.pirep_id,
        "teil": teil,
        "slots": slots.iter().map(|s| slot_name(*s)).collect::<Vec<_>>(),
        "anlass": anlass,
        "zweite_sprache": einst.zweite_sprache,
        "echter_name": einst.echter_name,
        // Wirkt nur beim ersten Abruf eines Fluges (Crew wird dann fest).
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
    });
    // Kapitän/Kapitänin: nur bei fester Wahl mitschicken, „auto“ = weglassen
    // (der Server würfelt). Wirkt nur beim ersten Abruf eines Fluges.
    if einst.kapitaen_geschlecht != "auto" {
        anfrage["kapitaen_geschlecht"] = serde_json::json!(einst.kapitaen_geschlecht);
    }
    anfrage
}

/// Erwartete Landung (UTC-Epoch) für die Texte (Michael BER181: dreimal
/// „sechzehn Uhr“ = Planankunft, gelandet 15:24). Reihenfolge: Hochrechnung
/// aus der Route; in der Luft Abheben + geplante Flugzeit; am Boden Plan-
/// Ankunft plus Abweichung — vor dem Off-Block nur Verspätung (eine
/// Verfrühung ist am Gate noch nicht sicher), danach in beide Richtungen.
fn ankunft_erwartet(
    route: Option<i64>,
    abheben: Option<i64>,
    flugzeit_min: Option<u32>,
    plan_on_block: Option<i64>,
    abweichung_min: Option<i64>,
    off_block: bool,
) -> Option<i64> {
    if route.is_some() {
        return route;
    }
    if let (Some(ab), Some(min)) = (abheben, flugzeit_min) {
        return Some(ab + i64::from(min) * 60);
    }
    let abw = abweichung_min.unwrap_or(0);
    plan_on_block.map(|p| p + if off_block { abw } else { abw.max(0) } * 60)
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

/// Übernimmt ein Drehbuch in den Lauf und liefert, was noch fehlt — je Slot
/// UND Sprache (1f). Bizjet: Die Sicherheit gilt nur als Kurzfassung des
/// Kapitäns (`rolle: kapitaen`); fehlt sie, entfällt der Slot sicher.
/// `nachholung`: Antwort auf den gezielten zweiten Abruf — nur fehlende
/// Sprachen übernehmen, vorhandene Texte (und ihr Ton) bleiben (vorher
/// ersetzte die Nachholung die ganze Ansage). Meldet der Server `entfaellt`
/// (Punkt 4), wird die Sprache bzw. die Sicherheit dieser Sprache einmal für
/// den ganzen Flug gestrichen und nicht nachgeholt.
fn drehbuch_uebernehmen(lauf: &mut Lauf, d: api_client::KabinenDrehbuch, slots: &[Slot], bizjet: bool, nachholung: bool) -> Vec<(Slot, String)> {
    // Boarding-Lied (2.1.1 G1): die erste Vorlage gilt für den Flug.
    if let Some(l) = d.boarding_lied.clone().filter(|l| !l.text.trim().is_empty()) {
        lauf.lied.vorlage.get_or_insert(l);
    }
    if !d.sprachen.is_empty() {
        lauf.sprachen = d.sprachen.clone();
    }
    if d.crew.is_some() {
        lauf.crew = d.crew.clone();
    }
    for (schluessel, grund) in &d.entfaellt {
        match (schluessel.split_once('/'), grund.as_str()) {
            (Some(("sprache", sp)), "keine_stimme") => stimme_streichen(lauf, sp),
            (Some(("sicherheit", sp)), _) => {
                if lauf.ohne_vorlage.insert(sp.to_string()) {
                    notiz(lauf, "entfaellt", Some(Slot::Sicherheit), Some(sp), serde_json::json!({ "grund": grund }));
                }
                lauf.texte_fehlen.insert((Slot::Sicherheit, sp.to_string()), grund.clone());
            }
            _ => {}
        }
    }
    for e in d.eintraege {
        let Some(slot) = slot_aus_name(&e.slot) else { continue };
        if slot == Slot::Sicherheit {
            let kapitaen = e.rolle.as_deref() == Some("kapitaen");
            // Bizjet: nur die Kurzfassung des Kapitäns, nie ein
            // Kabinentext (ältere Module kennen das Kennzeichen nicht).
            if bizjet && !kapitaen {
                continue;
            }
            lauf.sicherheit_kapitaen |= kapitaen;
        }
        if slot == Slot::Reiseflug80 && e.art.is_some() {
            lauf.art_r80 = e.art.clone();
        }
        let k = (slot, e.sprache.clone());
        // 1f: Die Nachholung ergänzt nur, was fehlte.
        if nachholung && lauf.texte.contains_key(&k) {
            continue;
        }
        lauf.texte_fehler.remove(&slot);
        lauf.texte_fehlen.remove(&k);
        // Ein Ereignis (z. B. die zweite Turbulenz) bekommt neuen Ton.
        if ist_ereignis(slot) {
            lauf.ton.remove(&k);
            lauf.ton_endgueltig.remove(&k);
        }
        if e.fremd.is_empty() {
            lauf.fremd.remove(&k);
        } else {
            lauf.fremd.insert(k.clone(), e.fremd.clone());
        }
        // Sprache ohne Stimme: kein Ton anfragen, die Ansage wartet nicht darauf.
        if lauf.ohne_stimme.contains(&e.sprache) {
            lauf.ton.entry(k.clone()).or_insert(None);
        }
        lauf.texte.insert(k, e.text);
    }
    // Bizjet ohne Kurzfassung der Sicherheit: entfällt sicher,
    // statt nachgeholt und als „aufgegeben“ gezeigt zu werden.
    if bizjet
        && slots.contains(&Slot::Sicherheit)
        && !lauf.texte.keys().any(|(s, _)| *s == Slot::Sicherheit)
    {
        lauf.entfaellt.insert(Slot::Sicherheit, "kein_kurztext".into());
    }
    // Den Ton erzeugt `ton_pumpe` im nächsten Takt, in der
    // Reihenfolge der Dringlichkeit.
    let mut fehlend = Vec::new();
    for s in slots.iter().copied().filter(|s| !lauf.entfaellt.contains_key(s)) {
        for sp in &lauf.sprachen {
            let k = (s, sp.clone());
            let gestrichen = lauf.ohne_stimme.contains(sp) || (s == Slot::Sicherheit && lauf.ohne_vorlage.contains(sp));
            if !lauf.texte.contains_key(&k) && !gestrichen {
                fehlend.push(k);
            }
        }
    }
    fehlend
}

/// Punkt 4: Für diese Sprache gibt es keine Stimme — einmal für den ganzen
/// Flug streichen (alle vorhandenen und künftigen Texte ohne Ton), statt je
/// Ansage neu zu fragen (CDC 8939: 14 × 404). Englisch nie (Kommandos).
fn stimme_streichen(lauf: &mut Lauf, sprache: &str) {
    if sprache.starts_with("en-") || !lauf.ohne_stimme.insert(sprache.to_string()) {
        return;
    }
    notiz(lauf, "entfaellt", None, Some(sprache), serde_json::json!({ "grund": "keine_stimme" }));
    let keys: Vec<(Slot, String)> = lauf.texte.keys().filter(|(_, sp)| sp == sprache).cloned().collect();
    for k in keys {
        lauf.ton.entry(k).or_insert(None);
    }
}

fn holen(
    app: AppHandle,
    flight: Arc<ActiveFlight>,
    client: Client,
    schluessel: String,
    anfrage: serde_json::Value,
    slots: Vec<Slot>,
    nachholung: bool,
    bizjet: bool,
) {
    tauri::async_runtime::spawn(async move {
        let gestartet = Utc::now();
        {
            let details = serde_json::json!({ "teil": schluessel, "slots": slots.iter().map(|s| slot_name(*s)).collect::<Vec<_>>(), "nachholung": nachholung });
            notiz(&mut flight.stats.lock().expect("flight stats").kabinen_lauf, "text_anfrage", None, None, details);
        }
        protokoll_leeren(&app, &flight);
        let ergebnis = client.kabinen_drehbuch(&anfrage).await;
        let dauer_ms = (Utc::now() - gestartet).num_milliseconds();
        let mut fehlend: Vec<(Slot, String)> = Vec::new();
        {
            let mut stats = flight.stats.lock().expect("flight stats");
            let lauf = &mut stats.kabinen_lauf;
            let fehler = ergebnis.as_ref().err().map(|e| kurz(&e.to_string()));
            match ergebnis {
                Ok(d) => {
                    // Lied angefragt, aber keins gekommen: still, „kein Lied“.
                    let angefragt = anfrage.get("boarding_lied").and_then(|v| v.as_bool()) == Some(true);
                    let (geliefert, spaeter) = (d.boarding_lied.is_some(), d.boarding_lied_spaeter);
                    fehlend = drehbuch_uebernehmen(lauf, d, &slots, bizjet, nachholung);
                    lied_nach_antwort(&mut lauf.lied, angefragt, geliefert, spaeter, Utc::now().timestamp());
                    if nachholung {
                        // Auch der gezielte zweite Abruf brachte nichts: auf
                        // der Seite sagen, statt ewig „wird geschrieben“ —
                        // nur für diese Sprache dieser Ansage (1f).
                        for k in &fehlend {
                            lauf.texte_fehlen.insert(k.clone(), "Der Server hat keinen Text geliefert.".into());
                        }
                    }
                }
                Err(e) if schluessel == "abflug:lied" => {
                    // Nur das Lied fehlte: die Texte sind schon da.
                    lauf.lied.fehler = Some(kurz(&e.to_string()));
                }
                Err(e) if schluessel == "lied" => {
                    // Netz oder Server: spaeter noch einmal (hoechstens drei Versuche
                    // ueber `darf_anfragen`), erst dann „nicht geladen“. Lehnt das
                    // Modul ab (4xx, z. B. altes Modul ohne Teil „lied“), nicht
                    // wiederholen (Codex 2.1.3).
                    lauf.angefragt.remove("lied");
                    lauf.lied.angefragt = false;
                    let abgelehnt = matches!(&e, api_client::ApiError::Server { status, .. } if (400..500).contains(status))
                        || matches!(&e, api_client::ApiError::Forbidden | api_client::ApiError::NotFound);
                    let n = if abgelehnt { 3 } else { lauf.fehlversuche.get("lied").map_or(0, |(n, _)| *n) + 1 };
                    lauf.fehlversuche.insert("lied".into(), (n, Utc::now().timestamp() + 120));
                    if n >= 3 {
                        lauf.lied.fehler = Some(kurz(&e.to_string()));
                    }
                }
                Err(e) => {
                    tracing::warn!(teil = %schluessel, error = %e, "kabinenansagen: drehbuch nicht erhalten");
                    let meldung = kurz(&e.to_string());
                    for s in &slots {
                        lauf.texte_fehler.insert(*s, meldung.clone());
                    }
                    // Scheitert die Nachholung („abflug+“), gibt sie den
                    // Hauptteil frei: Den fragt `takt` wieder an, und der
                    // Server liefert Fertiges aus dem Zwischenspeicher
                    // (Codex-QS beta.4; „abflug+“ selbst fragt niemand neu an).
                    let basis = schluessel.trim_end_matches('+').to_string();
                    lauf.angefragt.remove(&basis);
                    let n = lauf.fehlversuche.get(&basis).map_or(0, |(n, _)| *n) + 1;
                    lauf.fehlversuche.insert(basis, (n, Utc::now().timestamp() + 120));
                }
            }
            let details = serde_json::json!({
                "teil": schluessel,
                "angefragt_um": gestartet.to_rfc3339(),
                "dauer_ms": dauer_ms,
                "slots": slots.iter().map(|s| slot_name(*s)).collect::<Vec<_>>(),
                "fehlend": fehlend.iter().map(|(s, sp)| format!("{}/{sp}", slot_name(*s))).collect::<Vec<_>>(),
                "nachholung": nachholung,
                "fehler": fehler,
            });
            notiz(lauf, "text_antwort", None, None, details);
        }
        protokoll_leeren(&app, &flight);
        if !nachholung && !fehlend.is_empty() {
            // Der Server schreibt je Slot; übernommen wird nur, was fehlte.
            let mut nach: Vec<Slot> = fehlend.iter().map(|(s, _)| *s).collect();
            nach.dedup();
            let mut erneut = anfrage.clone();
            erneut["slots"] = serde_json::json!(nach.iter().map(|s| slot_name(*s)).collect::<Vec<_>>());
            tracing::info!(teil = %schluessel, fehlend = ?fehlend, "kabinenansagen: fehlende Texte nachholen");
            holen(app, flight, client, format!("{schluessel}+"), erneut, nach, true, bizjet);
        }
    });
}

// ---------------------------------------------------------------------------
// Sprachausgabe und Ton-Ordner
// ---------------------------------------------------------------------------

/// Unterordner für Hörproben (Einstellungen).
const PROBE_ORDNER: &str = "probe";

/// Ordnername eines Fluges: die PIREP-Kennung, nur sichere Zeichen.
fn flug_ordner(flight: &ActiveFlight) -> String {
    let n: String = flight.pirep_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').take(64).collect();
    if n.is_empty() { "flug".into() } else { n }
}

fn ton_wurzel(app: &AppHandle) -> Result<PathBuf, String> {
    let d = app.path().app_cache_dir().map_err(|e| e.to_string())?.join("kabinenansagen").join("ton");
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    static AUFGERAEUMT: OnceLock<()> = OnceLock::new();
    AUFGERAEUMT.get_or_init(|| altes_aufraeumen(&d));
    Ok(d)
}

/// Ordner eines Fluges (oder der Hörproben). Der erste Ton eines neuen
/// Fluges räumt die Ordner aller anderen Flüge weg: Töne gibt es nur für
/// den laufenden Flug (Thomas 17.10.2026: „werden die gelöscht nach dem
/// Flug?“). Ein Punkt für alle vier Wege, einen Flug zu beenden.
fn ton_ordner(app: &AppHandle, unter: &str) -> Result<PathBuf, String> {
    let wurzel = ton_wurzel(app)?;
    let d = wurzel.join(unter);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    if unter != PROBE_ORDNER {
        static AKTUELL: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
        let mut aktuell = AKTUELL.lock().expect("ton ordner");
        if aktuell.as_deref() != Some(unter) {
            andere_fluege_weg(&wurzel, unter);
            *aktuell = Some(unter.to_string());
        }
    }
    Ok(d)
}

/// Löscht die Ordner aller Flüge außer `bleibt` (Hörproben bleiben).
fn andere_fluege_weg(wurzel: &Path, bleibt: &str) {
    let Ok(eintraege) = std::fs::read_dir(wurzel) else { return };
    for e in eintraege.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if e.path().is_dir() && name != bleibt && name != PROBE_ORDNER {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Einmal je Start: lose Dateien der früheren Ablage (bis beta.4 lag alles
/// in einem Ordner, ohne je gelöscht zu werden) und Hörproben älter als ein
/// Tag. Flug-Ordner nicht nach Alter: Der Start weiß noch nicht, welcher
/// Flug gleich fortgesetzt wird (Codex-QS beta.5); sie räumt der erste Ton
/// des nächsten Fluges weg ([`ton_ordner`]).
fn altes_aufraeumen(wurzel: &Path) {
    let alter = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    let Ok(eintraege) = std::fs::read_dir(wurzel) else { return };
    for e in eintraege.flatten() {
        let p = e.path();
        if p.is_file() {
            let _ = std::fs::remove_file(&p);
        } else if e.file_name() == PROBE_ORDNER {
            for f in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                if alter(&f.path()).is_some_and(|s| s > 86_400) {
                    let _ = std::fs::remove_file(f.path());
                }
            }
        }
    }
}

/// Belegter Platz des Ton-Ordners (Bytes) — für die Einstellungen.
fn ordner_groesse(p: &Path) -> u64 {
    std::fs::read_dir(p)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| {
            let p = e.path();
            if p.is_dir() { ordner_groesse(&p) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }
        })
        .sum()
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Speicher {
    bytes: u64,
    ordner: String,
}

#[tauri::command]
pub(crate) fn kabinen_speicher(app: AppHandle) -> Result<Speicher, UiError> {
    let wurzel = ton_wurzel(&app).map_err(|e| UiError::new("ordner", e))?;
    Ok(Speicher { bytes: ordner_groesse(&wurzel), ordner: wurzel.to_string_lossy().to_string() })
}

/// Wofür ein Ton gebraucht wird — geht an den Server (Abarbeiten nach
/// Dringlichkeit, S2) und bestimmt den Ordner.
struct Wunsch<'a> {
    slot: Option<Slot>,
    /// Sekunden bis zur Wiedergabe; 0 = fällig; `None` = unbekannt.
    gebraucht_in_s: Option<i64>,
    ordner: &'a str,
    /// PIREP-Nummer und Rolle: Der Server wählt danach die Stimme des Flugs.
    /// Ohne (Hörprobe): erste Stimme des Kreises bzw. für die Kabine die zweite.
    flug: Option<&'a str>,
    rolle: Option<&'static str>,
    /// Fremde Namen im Text (2.1.0), gehen unverändert an den Ton-Server.
    fremd: &'a [api_client::KabinenFremd],
}

/// Fehler einer Ton-Anfrage.
#[derive(Debug, Clone)]
pub(crate) struct TonFehlerAntwort {
    meldung: String,
    /// Wartezeit bis zum nächsten Versuch (`Retry-After`, sonst 30 s).
    warte_s: u64,
    /// 400: Diese Anfrage wird nie gehen.
    endgueltig: bool,
}

impl TonFehlerAntwort {
    fn spaeter(meldung: impl Into<String>, warte_s: Option<u64>) -> Self {
        TonFehlerAntwort { meldung: meldung.into(), warte_s: warte_s.unwrap_or(30), endgueltig: false }
    }
}

/// Text zu Ton ueber den GSG-Server. Gibt es für die Sprache keine Stimme,
/// entfällt sie (`Ok(None)`) — nie eine Stimme einer anderen Sprache.
async fn sprechen(app: &AppHandle, maennlich: bool, sprache: &str, text: &str, wunsch: Wunsch<'_>) -> Result<Option<Ton>, TonFehlerAntwort> {
    // Der Server waehlt die Stimme zu Sprache, Flug und Rolle (Supertonic,
    // ab 2.1.0). Hat er keine, entfaellt die Sprache; ein Fehler (Netz,
    // ausgelastet) kommt zurueck und der Takt versucht es spaeter wieder.
    let Some(anfrage) = piper::stimme_fuer(sprache, maennlich) else { return Ok(None) };
    // Gemerkt je Flug und Rolle: Im naechsten Flug oder fuer die andere Rolle
    // waehlt der Server eine andere Stimme, und keine gespeicherte Ansage darf
    // in der Stimme des Vorflugs oder der anderen Rolle kommen.
    let gemerkt = gewaehlt_schluessel(&anfrage, wunsch.flug, wunsch.rolle);
    let ordner = ton_ordner(app, wunsch.ordner).map_err(|e| TonFehlerAntwort::spaeter(e, None))?;
    let relativ = |name: &str| format!("{}/{name}", wunsch.ordner);
    // Gespeichert wird unter der Stimme, die der Server gewaehlt hat — so
    // verdeckt der Zwischenspeicher keinen Stimmwechsel auf dem Server
    // (Codex-QS beta.4). Je Sitzung fragt der Client einmal nach.
    // Fremde Namen ändern den Ton bei gleichem Text: Sie gehören in den Namen
    // der zwischengespeicherten Datei (2.1.0).
    let schluessel = if wunsch.fremd.is_empty() {
        text.to_string()
    } else {
        format!("{text}|{}", serde_json::to_string(wunsch.fremd).unwrap_or_default())
    };
    let bekannt = GEWAEHLT.lock().expect("gewaehlt").get(&gemerkt).cloned();
    if let Some(stimme) = &bekannt {
        let name = cache::dateiname("server", stimme, sprache, &schluessel, "wav");
        if ordner.join(&name).exists() {
            return ton_aus_datei(&ordner.join(&name), relativ(&name)).map(Some).map_err(|e| TonFehlerAntwort::spaeter(e, None));
        }
    }
    let teil = wunsch.slot.and_then(ton_teil);
    let wahl = piper::Wahl { flug: wunsch.flug, rolle: wunsch.rolle, fremd: wunsch.fremd };
    let Some(ton) = piper::sprechen(&anfrage, text, wunsch.gebraucht_in_s, teil, wunsch.slot.map(slot_name), wahl).await? else {
        return Ok(None);
    };
    let name = if ton.ersatz {
        // Ersatzstimme (Server bis 2.0.x, Kokoro gerade aus): nicht als diese Stimme merken.
        cache::dateiname("ersatz", &format!("{anfrage}|{}", Utc::now().timestamp_millis()), sprache, text, "wav")
    } else {
        let stimme = ton.stimme.unwrap_or(anfrage.clone());
        GEWAEHLT.lock().expect("gewaehlt").insert(gemerkt, stimme.clone());
        cache::dateiname("server", &stimme, sprache, &schluessel, "wav")
    };
    let pfad = ordner.join(&name);
    std::fs::write(&pfad, &ton.wav).map_err(|e| TonFehlerAntwort::spaeter(e.to_string(), None))?;
    let herkunft = if ton.ersatz { "ersatz" } else { "server" };
    ton_aus_datei(&pfad, relativ(&name)).map(|t| Some(Ton { herkunft, ..t })).map_err(|e| TonFehlerAntwort::spaeter(e, None))
}

/// `auto:<sprache>:<m|w>` je Flug und Rolle → vom Server gewaehlte Stimme,
/// je Sitzung (Schluessel aus `gewaehlt_schluessel`).
static GEWAEHLT: std::sync::LazyLock<std::sync::Mutex<HashMap<String, String>>> =
    std::sync::LazyLock::new(Default::default);

fn gewaehlt_schluessel(anfrage: &str, flug: Option<&str>, rolle: Option<&str>) -> String {
    format!("{anfrage}|{}|{}", flug.unwrap_or(""), rolle.unwrap_or(""))
}

fn ton_aus_datei(pfad: &Path, name: String) -> Result<Ton, String> {
    let daten = std::fs::read(pfad).map_err(|e| e.to_string())?;
    let dauer_s = wav_dauer_s(&daten).ok_or_else(|| {
        let _ = std::fs::remove_file(pfad);
        "kein gültiges WAV".to_string()
    })?;
    Ok(Ton { datei: name, dauer_s, stuecke: wav_stuecke(&daten), herkunft: "lokal" })
}

/// Satz-Startzeiten aus dem RIFF-Abschnitt `satz` (UTF-8-JSON
/// `[{"ab_s", "text"}]`, Ton-Server ab Paket 3 S1). Leer, wenn es keinen gibt
/// oder er nicht lesbar ist — dann schätzt die Seite.
pub(crate) fn wav_stuecke(d: &[u8]) -> Vec<Stueck> {
    if d.len() < 12 || &d[0..4] != b"RIFF" || &d[8..12] != b"WAVE" {
        return Vec::new();
    }
    let mut i = 12;
    while i + 8 <= d.len() {
        let id = &d[i..i + 4];
        let groesse = u32::from_le_bytes([d[i + 4], d[i + 5], d[i + 6], d[i + 7]]) as usize;
        let inhalt = i + 8;
        if id == b"satz" {
            let Some(roh) = d.get(inhalt..inhalt.saturating_add(groesse)) else { return Vec::new() };
            let mut s: Vec<Stueck> = serde_json::from_slice(roh).unwrap_or_default();
            s.retain(|x| x.ab_s.is_finite() && x.ab_s >= 0.0);
            return s;
        }
        i = inhalt.saturating_add(groesse).saturating_add(groesse & 1);
    }
    Vec::new()
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

/// Ist das ein Dateiname aus dem eigenen Ton-Ordner (`<ordner>/<name>.wav`)?
/// Keine Pfade, kein `..`.
fn datei_sauber(datei: &str) -> bool {
    let teile: Vec<&str> = datei.split('/').collect();
    let teil_ok = |t: &str| !t.is_empty() && t != "." && t != ".." && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    teile.len() == 2 && teile.iter().all(|t| teil_ok(t)) && datei.ends_with(".wav") && !datei.contains("..")
}

/// Liefert eine erzeugte Tondatei an die Oberfläche.
#[tauri::command]
pub(crate) fn kabinen_audio(app: AppHandle, datei: String) -> Result<tauri::ipc::Response, UiError> {
    Ok(tauri::ipc::Response::new(ton_datei_lesen(&app, &datei)?))
}

fn ton_datei_lesen(app: &AppHandle, datei: &str) -> Result<Vec<u8>, UiError> {
    if !datei_sauber(datei) {
        return Err(UiError::new("ungueltig", "invalid file name"));
    }
    let pfad = ton_wurzel(app).map_err(|e| UiError::new("ordner", e))?.join(datei);
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
pub(crate) async fn kabinen_ton_spielen(app: AppHandle, datei: Option<String>) -> Result<(), UiError> {
    let stand = ausgabe::stopp_stand();
    let (rate, proben) = match datei {
        Some(d) => ausgabe::wav_lesen(&ton_datei_lesen(&app, &d)?)
            .ok_or_else(|| UiError::new("format", "unsupported wav"))?,
        None => (ausgabe::CHIME_RATE, ausgabe::chime(ausgabe::CHIME_RATE)),
    };
    let einst = einstellungen(&app);
    let (geraet, klang) = (einst.ausgabe, ausgabe::Klang::aus_name(&einst.klang));
    tauri::async_runtime::spawn_blocking(move || ausgabe::spielen(geraet.as_deref(), rate, &proben, stand, klang))
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

// ---------------------------------------------------------------------------
// Verlauf: bleibt über einen Neustart der App erhalten
// ---------------------------------------------------------------------------

fn verlauf_pfad(app: &AppHandle, flight: &ActiveFlight) -> Option<PathBuf> {
    ton_ordner(app, &flug_ordner(flight)).ok().map(|d| d.join("verlauf.json"))
}

/// Liest die Verlaufsdatei (ohne Sperre aufrufen).
fn verlauf_datei_lesen(app: &AppHandle, flight: &ActiveFlight) -> Option<String> {
    verlauf_pfad(app, flight).and_then(|p| std::fs::read_to_string(p).ok())
}

/// Übernimmt den gelesenen Verlauf einmal je Flug.
fn verlauf_uebernehmen(lauf: &mut Lauf, text: Option<String>) {
    if lauf.verlauf_geladen {
        return;
    }
    lauf.verlauf_geladen = true;
    let Some(text) = text else { return };
    if let Ok(mut alt) = serde_json::from_str::<Vec<Gespielt>>(&text) {
        for g in &mut alt {
            // Lief beim Beenden der App noch: Ausgang unbekannt.
            if g.ergebnis.is_none() {
                g.ergebnis = Some("unbekannt".into());
            }
        }
        lauf.naechste_id = alt.iter().map(|g| g.id).max().unwrap_or(0);
        alt.append(&mut lauf.verlauf);
        lauf.verlauf = alt;
    }
}

/// Fortlaufende Nummer jeder Verlaufs-Fassung. Vergeben, solange der Aufrufer
/// die Flugdaten-Sperre hält — die Nummer folgt also dem Stand.
static VERLAUF_FASSUNG: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Eine Verlaufs-Fassung zum Schreiben (Nummer, JSON). Unter der Sperre bauen.
struct VerlaufText(u64, String);

fn verlauf_text(lauf: &Lauf) -> Option<VerlaufText> {
    let text = serde_json::to_string(&lauf.verlauf).ok()?;
    Some(VerlaufText(VERLAUF_FASSUNG.fetch_add(1, Ordering::SeqCst) + 1, text))
}

/// Schreibt ohne Flugdaten-Sperre, aber geordnet: Eine ältere Fassung
/// überschreibt nie eine neuere (Takt und Befehle der Oberfläche laufen
/// gleichzeitig), und die Datei wird über eine Zwischendatei ersetzt, damit
/// nie halbes JSON liegt (Codex-Review PR #158).
fn verlauf_schreiben(app: &AppHandle, flight: &ActiveFlight, text: &VerlaufText) {
    static GESCHRIEBEN: std::sync::Mutex<u64> = std::sync::Mutex::new(0);
    let Some(p) = verlauf_pfad(app, flight) else { return };
    let mut zuletzt = GESCHRIEBEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if text.0 <= *zuletzt {
        return;
    }
    let zwischen = p.with_extension("json.neu");
    if std::fs::write(&zwischen, &text.1).is_ok() && std::fs::rename(&zwischen, &p).is_ok() {
        *zuletzt = text.0;
    }
}

// ---------------------------------------------------------------------------
// Wiedergabe: Ende melden, Vorhören, Erneut versuchen
// ---------------------------------------------------------------------------

fn aktiver_flug(state: &AppState) -> Option<Arc<ActiveFlight>> {
    state.active_flight.lock().expect("active_flight lock").as_ref().map(Arc::clone)
}

/// Die Oberfläche meldet, wie eine Wiedergabe endete — seit 1b nur noch
/// für Vorhören und Hörprobe, die sie selbst spielt. Für Ansagen des Fluges
/// trägt das Backend das Ende selbst ein; kommt trotzdem eine Meldung, ist
/// sie nur Bestätigung (die Ansage hat dann schon ihr Ende, nichts ändert sich).
#[tauri::command]
pub(crate) fn kabinen_wiedergabe_ende(app: AppHandle, state: tauri::State<'_, AppState>, id: u64, ergebnis: String) {
    let Some(flight) = aktiver_flug(&state) else { return };
    let mut stats = flight.stats.lock().expect("flight stats");
    let text = if ende_melden(&mut stats, id, &ergebnis, Utc::now().timestamp(), "oberflaeche") { verlauf_text(&stats.kabinen_lauf) } else { None };
    // Gleich schreiben (die letzte Ansage des Fluges hat keinen Takt mehr
    // danach), Datei und Protokoll ohne Flugdaten-Sperre.
    drop(stats);
    if let Some(text) = text {
        verlauf_schreiben(&app, &flight, &text);
    }
    protokoll_leeren(&app, &flight);
}

/// Wie [`wiedergabe_ende`], dazu erfährt der Planer, ob die Sinkflug-Ansage
/// des Kapitäns wirklich lief (danach richtet sich die Kabine). `quelle`
/// steht im Flugprotokoll: `backend` (Wiedergabeende, 1b) oder `oberflaeche`.
fn ende_melden(stats: &mut FlightStats, id: u64, ergebnis: &str, jetzt: i64, quelle: &str) -> bool {
    if !wiedergabe_ende(&mut stats.kabinen_lauf, id, ergebnis, jetzt) {
        return false;
    }
    let ende = stats.kabinen_lauf.verlauf.iter().find(|g| g.id == id).map(|g| (g.slot, g.ergebnis.as_deref() == Some("gespielt")));
    if let Some((slot, _)) = ende {
        notiz(&mut stats.kabinen_lauf, "ende", Some(slot), None, serde_json::json!({ "ergebnis": ergebnis, "quelle": quelle }));
    }
    if let (Some((Slot::KapitaenSinkflug, gespielt)), Some(p)) = (ende, stats.kabinen_planer.as_mut()) {
        p.kapitaen_sinkflug_ende(gespielt, jetzt);
    }
    true
}

/// Anzeige-Ereignis: Teil `teil` (Sprache) einer Ansage beginnt.
pub(crate) const EREIGNIS_TEIL: &str = "kabine-teil";
/// Anzeige-Ereignis: Eine Ansage ist zu Ende (`ergebnis`, `fehler`).
pub(crate) const EREIGNIS_ENDE: &str = "kabine-ende";

#[derive(Debug, Clone, Serialize)]
struct TeilBeginnt {
    id: u64,
    teil: usize,
}

#[derive(Debug, Clone, Serialize)]
struct AnsageEnde {
    id: u64,
    ergebnis: &'static str,
    fehler: Option<String>,
}

/// Gerät und Uhr des Backend-Ablaufs (1b): im Betrieb cpal und die
/// Systemuhr, im Test eingeschleust.
struct Spieler<'a> {
    /// Spielt einen Teil bis zu seinem Ende (`None` = Gong); `Err` = Gerätefehler.
    spielen: &'a mut dyn FnMut(Option<&Teil>) -> Result<(), String>,
    /// Gestoppt (Ausschalten, Stumm, Anhören beenden)?
    gestoppt: &'a dyn Fn() -> bool,
    warten_ms: &'a dyn Fn(u64),
    jetzt: &'a dyn Fn() -> i64,
    teil_beginnt: &'a mut dyn FnMut(usize),
}

/// Spielt eine ausgegebene Ansage Sprache für Sprache und trägt ihr Ende
/// selbst ein (1b: vorher steuerte die Oberfläche den Ablauf und meldete das
/// Ende; im Hintergrund schlief sie auf dem Mac bis zu 24 min). Ist die
/// Ansage schon erledigt (stumm, unbekannt) oder kam sie zu spät an die
/// Reihe, wird nichts nachgespielt: `None`. Sonst Ergebnis, letzter
/// Gerätefehler und der neue Verlauf.
fn ablauf(stats: &crate::stats_sperre::GemesseneSperre<FlightStats>, id: u64, sp: Spieler<'_>) -> Option<(&'static str, Option<String>, Option<VerlaufText>)> {
    let (teile, gong) = {
        let mut st = stats.lock().expect("flight stats");
        let jetzt = (sp.jetzt)();
        let g = st.kabinen_lauf.verlauf.iter_mut().find(|g| g.id == id && g.ergebnis.is_none() && !g.vorschau)?;
        // Kam der Ablauf erst nach der Startfrist dran, ist sie verpasst
        // (der Takt verbucht sie als „unbekannt“) — nicht verspätet spielen.
        if !g.gestartet && jetzt - g.zeit > START_SPAETESTENS_S {
            return None;
        }
        g.gestartet = true;
        (g.teile.clone(), g.rolle == Rolle::Chime)
    };
    let offen = || stats.lock().expect("flight stats").kabinen_lauf.verlauf.iter().any(|g| g.id == id && g.ergebnis.is_none());
    let mut fehler: Option<String> = None;
    let mut abgebrochen = (sp.gestoppt)();
    if !abgebrochen && gong {
        (sp.teil_beginnt)(0);
        if let Err(e) = (sp.spielen)(None) {
            fehler = Some(e);
        }
    }
    for (i, teil) in teile.iter().enumerate() {
        if gong {
            break;
        }
        // Stumm oder Ausschalten zwischen den Sprachen: Rest nicht mehr.
        if (sp.gestoppt)() || !offen() {
            abgebrochen = true;
            break;
        }
        (sp.teil_beginnt)(i);
        if let Err(e) = (sp.spielen)(Some(teil)) {
            tracing::warn!(error = %e, sprache = %teil.sprache, "kabinenansage: Ton nicht abspielbar");
            fehler = Some(e);
        }
        if i + 1 < teile.len() {
            (sp.warten_ms)(700);
        }
    }
    abgebrochen |= (sp.gestoppt)();
    let ergebnis = if abgebrochen { "abgebrochen" } else if fehler.is_some() { "fehler" } else { "gespielt" };
    let mut st = stats.lock().expect("flight stats");
    let text = if ende_melden(&mut st, id, ergebnis, (sp.jetzt)(), "backend") { verlauf_text(&st.kabinen_lauf) } else { None };
    Some((ergebnis, fehler, text))
}

/// Startet den Backend-Ablauf einer ausgegebenen Ansage in einem eigenen
/// Faden (cpal blockiert). Immer nur einer zugleich: Ein zweiter wartet,
/// bis der erste sein Gerät freigegeben hat.
fn ablauf_starten(app: &AppHandle, flight: &Arc<ActiveFlight>, id: u64) {
    let (app, flight) = (app.clone(), Arc::clone(flight));
    tauri::async_runtime::spawn_blocking(move || {
        static EINER: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _einer = EINER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let stand = ausgabe::stopp_stand();
        let einst = einstellungen(&app);
        let (geraet, klang) = (einst.ausgabe.clone(), ausgabe::Klang::aus_name(&einst.klang));
        let mut spielen = |teil: Option<&Teil>| -> Result<(), String> {
            let (rate, proben) = match teil {
                Some(t) => ausgabe::wav_lesen(&ton_datei_lesen(&app, &t.datei).map_err(|e| e.message)?).ok_or_else(|| "unsupported wav".to_string())?,
                None => (ausgabe::CHIME_RATE, ausgabe::chime(ausgabe::CHIME_RATE)),
            };
            ausgabe::spielen(geraet.as_deref(), rate, &proben, stand, klang)
        };
        let gestoppt = || ausgabe::stopp_stand() != stand || STUMM.load(Ordering::SeqCst) || !einstellungen(&app).an;
        let warten_ms = |ms: u64| std::thread::sleep(std::time::Duration::from_millis(ms));
        let jetzt = || Utc::now().timestamp();
        let app_teil = app.clone();
        let mut teil_beginnt = |teil: usize| {
            let _ = app_teil.emit(EREIGNIS_TEIL, TeilBeginnt { id, teil });
        };
        let sp = Spieler { spielen: &mut spielen, gestoppt: &gestoppt, warten_ms: &warten_ms, jetzt: &jetzt, teil_beginnt: &mut teil_beginnt };
        let Some((ergebnis, fehler, text)) = ablauf(&flight.stats, id, sp) else { return };
        if let Some(text) = text {
            verlauf_schreiben(&app, &flight, &text);
        }
        protokoll_leeren(&app, &flight);
        let _ = app.emit(EREIGNIS_ENDE, AnsageEnde { id, ergebnis, fehler });
    });
}

/// Die Oberfläche beginnt die Wiedergabe einer Ansage: erst jetzt „läuft“ sie.
/// Seit 1b setzt das Backend den Start selbst; bleibt für ältere Oberflächen.
#[tauri::command]
pub(crate) fn kabinen_wiedergabe_start(state: tauri::State<'_, AppState>, id: u64) {
    let Some(flight) = aktiver_flug(&state) else { return };
    let mut stats = flight.stats.lock().expect("flight stats");
    if let Some(g) = stats.kabinen_lauf.verlauf.iter_mut().find(|g| g.id == id && g.ergebnis.is_none()) {
        g.gestartet = true;
    }
}

/// Stumm schalten oder aufheben (Knopf oben auf der Seite „Kabine“). Beim
/// Stummschalten endet die laufende Wiedergabe sofort, und alles Wartende
/// wird als „stumm übersprungen“ verbucht.
#[tauri::command]
pub(crate) fn kabinen_stumm(app: AppHandle, state: tauri::State<'_, AppState>, stumm: bool) {
    STUMM.store(stumm, Ordering::SeqCst);
    if !stumm {
        return;
    }
    ausgabe::stoppen();
    // Stumm stoppt auch die Boarding-Musik sofort (Pflichtenheft G1).
    if crate::kabinen_musik::flug_aktiv() {
        crate::kabinen_musik::aus();
    }
    let Some(flight) = aktiver_flug(&state) else { return };
    let mut stats = flight.stats.lock().expect("flight stats");
    let text = if stumm_anwenden(&mut stats, Utc::now().timestamp()) { verlauf_text(&stats.kabinen_lauf) } else { None };
    // Datei ohne Flugdaten-Sperre schreiben (2.1.1 D, Punkt 11).
    drop(stats);
    if let Some(text) = text {
        verlauf_schreiben(&app, &flight, &text);
    }
}

/// `true` = der Verlauf hat sich geändert (speichern).
fn wiedergabe_ende(lauf: &mut Lauf, id: u64, ergebnis: &str, jetzt: i64) -> bool {
    let ergebnis = match ergebnis {
        "gespielt" | "abgebrochen" | "fehler" | "stumm" => ergebnis,
        _ => "fehler",
    };
    if lauf.vorschau == Some(id) {
        lauf.vorschau = None;
        lauf.warteschlange.frei();
        return false;
    }
    match lauf.verlauf.iter_mut().find(|g| g.id == id && g.ergebnis.is_none()) {
        Some(g) => {
            g.ergebnis = Some(ergebnis.to_string());
            if ergebnis == "stumm" {
                // Nichts zu hören gewesen: keine Pause.
                lauf.warteschlange.abbrechen();
            } else if !g.slot.loest_pause_aus() {
                // Kommando, Gong: frei, eine laufende Pause bleibt.
                lauf.warteschlange.frei();
            } else {
                // Echte Ansage zu Ende: Pause bis zur nächsten regulären
                // (Thomas 18.10.2026); das Vorhören oben macht keine.
                lauf.warteschlange.ansage_zu_ende_nach(jetzt, g.slot);
            }
            true
        }
        None => false,
    }
}

/// Eine Ansage anhören, ohne sie zu verbrauchen: eine kommende, deren Ton
/// fertig ist, oder eine aus „Bisher“. Gesperrt, solange etwas spielt; was
/// in der Zeit fällig wird, wartet, bis das Anhören vorbei ist.
#[tauri::command]
pub(crate) fn kabinen_anhoeren(app: AppHandle, state: tauri::State<'_, AppState>, slot: Slot, id: Option<u64>) -> Result<Gespielt, UiError> {
    let flight = aktiver_flug(&state).ok_or_else(|| UiError::new("kein_flug", "no active flight"))?;
    let mut stats = flight.stats.lock().expect("flight stats");
    let _ = app;
    anhoeren(&mut stats.kabinen_lauf, slot, id, Utc::now().timestamp())
}

fn anhoeren(lauf: &mut Lauf, slot: Slot, id: Option<u64>, jetzt: i64) -> Result<Gespielt, UiError> {
    if lauf.warteschlange.belegt(jetzt) {
        return Err(UiError::new("laeuft", "an announcement is playing"));
    }
    let teile = match id {
        Some(id) => lauf.verlauf.iter().find(|g| g.id == id && !g.vorschau && !g.teile.is_empty()).map(|g| g.teile.clone()),
        None => teile_von(lauf, slot).filter(|_| ansage_bereit(&lauf.texte, &lauf.ton, &lauf.texte_fehler, &lauf.texte_fehlen, &lauf.sprachen, slot)),
    }
    .ok_or_else(|| UiError::new("nicht_bereit", "no audio for this announcement yet"))?;
    lauf.naechste_id += 1;
    let g = Gespielt {
        id: lauf.naechste_id,
        slot,
        rolle: rolle_von(lauf, slot),
        zeit: jetzt,
        teile,
        vorschau: true,
        ergebnis: None,
        gestartet: false,
        grund: None,
    };
    lauf.vorschau = Some(g.id);
    lauf.warteschlange.spielt_bis(jetzt + dauer_s(slot, &g.teile).ceil() as i64 + 1);
    Ok(g)
}

/// „Erneut versuchen“: aufgegebene Texte und abgelehnten Ton freigeben.
#[tauri::command]
pub(crate) fn kabinen_erneut(state: tauri::State<'_, AppState>) {
    let Some(flight) = aktiver_flug(&state) else { return };
    let mut stats = flight.stats.lock().expect("flight stats");
    let lauf = &mut stats.kabinen_lauf;
    lauf.fehlversuche.clear();
    lauf.texte_fehler.clear();
    // Was der Server als entfallen gemeldet hat, bleibt gestrichen (Punkt 4).
    let ohne_vorlage = lauf.ohne_vorlage.clone();
    lauf.texte_fehlen.retain(|(s, sp), _| *s == Slot::Sicherheit && ohne_vorlage.contains(sp));
    lauf.ton_endgueltig.clear();
    lauf.ton_fehler.clear();
    // Ein Teil, dessen Abruf gelang, dem aber Slots fehlen, wird neu gefragt.
    let fehlt = |teil: &str| {
        let slots: &[Slot] = match teil {
            "abflug" => &ABFLUG,
            "halbzeit" => &HALBZEIT,
            _ => &ANKUNFT,
        };
        slots.iter().any(|s| lauf.sprachen.iter().any(|sp| !lauf.texte.contains_key(&(*s, sp.clone())) && !lauf.texte_fehlen.contains_key(&(*s, sp.clone())) && !lauf.ohne_stimme.contains(sp)))
    };
    let neu: Vec<String> = ["abflug", "halbzeit", "ankunft"].into_iter().filter(|t| fehlt(t)).map(str::to_string).collect();
    for t in neu {
        lauf.angefragt.remove(&t);
    }
    lauf.beta_naechste_pruefung = 0;
}

/// Hörprobe für die Einstellungen: eine Rolle in einer Sprache.
#[tauri::command]
pub(crate) async fn kabinen_probe(app: AppHandle, rolle: Rolle, sprache: String) -> Result<Gespielt, UiError> {
    if !ist_locale(&sprache) {
        return Err(UiError::new("ungueltig", "invalid language"));
    }
    // Die Hörprobe folgt der Einstellung Kapitän/Kapitänin.
    let kapitaenin = rolle == Rolle::Kapitaen && einstellungen(&app).kapitaen_geschlecht == "w";
    let maennlich = match rolle {
        Rolle::Kapitaen => !kapitaenin,
        Rolle::Kabine => false,
        _ => true,
    };
    let text = match (sprache.split('-').next().unwrap_or(""), rolle) {
        ("de", Rolle::Kapitaen) if kapitaenin => "Guten Tag, liebe Gäste, hier spricht Ihre Kapitänin. Dies ist eine Hörprobe.",
        ("de", Rolle::Kapitaen) => "Guten Tag, liebe Gäste, hier spricht Ihr Kapitän. Dies ist eine Hörprobe.",
        ("de", _) => "Herzlich willkommen an Bord. Dies ist eine Hörprobe Ihrer Kabinenbesatzung.",
        (_, Rolle::Kapitaen) => "Good afternoon, ladies and gentlemen, this is your captain speaking. This is a voice sample.",
        _ => "Welcome on board. This is a voice sample from your cabin crew.",
    };
    let wunsch = Wunsch { slot: None, gebraucht_in_s: Some(0), ordner: PROBE_ORDNER, flug: None, rolle: server_rolle(rolle), fremd: &[] };
    let ton = sprechen(&app, maennlich, &sprache, text, wunsch)
        .await
        .map_err(|e| UiError::new("sprachausgabe", e.meldung))?
        .ok_or_else(|| UiError::new("keine_stimme", "no voice for this language and provider"))?;
    Ok(Gespielt {
        id: 0,
        slot: Slot::Boarding,
        rolle,
        zeit: Utc::now().timestamp(),
        teile: vec![Teil { sprache, text: text.to_string(), datei: ton.datei, dauer_s: ton.dauer_s, stuecke: ton.stuecke }],
        vorschau: true,
        ergebnis: None,
        gestartet: false,
        grund: None,
    })
}

// ---------------------------------------------------------------------------
// Überblick für die Seite „Kabine“
// ---------------------------------------------------------------------------

/// Wo eine Ansage gerade steht — eine Zeile je Ansage, ehrlich statt
/// „wird vorbereitet“ (Thomas 17.10.2026).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub(crate) enum Stand {
    /// Ankunftstext wird erst kurz vor der Landung geschrieben; `um` =
    /// voraussichtlich ab dann (unbekannt ohne erwartete Landung).
    TextAb { um: Option<i64>, nach_abheben: bool },
    TextWirdGeschrieben,
    TextFehler { um: i64, meldung: String },
    TextAufgegeben { meldung: String },
    TonWirdErzeugt { fertig: usize, von: usize },
    TonWartet { um: i64, meldung: String },
    TonAufgegeben { meldung: String },
    Bereit,
}

/// Warum eine fällige Ansage noch nicht spielt.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Wartet {
    /// Eine andere Ansage (oder ein Vorhören) läuft.
    AndereAnsage,
    /// Kurze Pause nach der letzten Ansage, damit nicht alles hintereinander kommt.
    Pause,
    /// Start, Landung, unter 10.000 ft: nur Kommandos und Gong.
    Ruhephase,
    /// Der Ton ist noch nicht fertig (Stand nennt den Grund).
    Ton,
    /// Simulator angehalten.
    SimAngehalten,
    /// Anschnallzeichen im Reiseflug noch an: Service wartet (Pilot).
    Anschnallzeichen,
}

/// Warum eine fällige Ansage gerade nicht spielt (für Seite und Log).
fn wartegrund(lauf: &Lauf, slot: Slot, jetzt: i64) -> Option<Wartet> {
    // „Anschnallzeichen aus“ wartet auf den Piloten (Hinweis).
    if slot == Slot::AnschnallzeichenAus && lauf.hinweis == Some("anschnallzeichen") {
        return Some(Wartet::Anschnallzeichen);
    }
    if lauf.sim_angehalten {
        Some(Wartet::SimAngehalten)
    } else if lauf.warteschlange.belegt(jetzt) {
        Some(Wartet::AndereAnsage)
    } else if lauf.warteschlange.in_pause(jetzt) && slot.wartet_pause() {
        Some(Wartet::Pause)
    } else if lauf.ruhephase && !slot.in_ruhephase_erlaubt() && !lauf.warteschlange.trotz_ruhephase(slot) {
        Some(Wartet::Ruhephase)
    } else if !ansage_bereit(&lauf.texte, &lauf.ton, &lauf.texte_fehler, &lauf.texte_fehlen, &lauf.sprachen, slot) {
        Some(Wartet::Ton)
    } else {
        None // spielt im nächsten Takt
    }
}

/// Protokolliert, wenn sich der Wartegrund einer fälligen Ansage ändert.
fn wartegruende_notieren(lauf: &mut Lauf, jetzt: i64) {
    let wartend = lauf.warteschlange.wartend();
    lauf.wartet_zuletzt.retain(|s, _| wartend.contains(s));
    for slot in wartend {
        let grund = wartegrund(lauf, slot, jetzt);
        if lauf.wartet_zuletzt.get(&slot) != Some(&grund) {
            lauf.wartet_zuletzt.insert(slot, grund);
            notiz(lauf, "wartet", Some(slot), None, serde_json::json!({ "grund": grund }));
        }
    }
}

/// Text einer Sprache für die aufklappbare Zeile.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct TeilAnsicht {
    sprache: String,
    text: Option<String>,
    ton: bool,
    keine_stimme: bool,
    /// Text dieser Sprache endgültig ausgeblieben (1f), mit Grund.
    fehlt: Option<String>,
}

/// Eine noch kommende oder fällige Ansage.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Geplant {
    slot: Slot,
    rolle: Rolle,
    /// Schon fällig (steht in der Warteschlange).
    faellig: bool,
    wartet: Option<Wartet>,
    /// Voraussichtliche Uhrzeit (UTC-Epoch), nur wo es eine Quelle gibt.
    wann: Option<i64>,
    stand: Stand,
    teile: Vec<TeilAnsicht>,
    /// Nur Reiseflug 80: zweiter_service | ende_bordverkauf (Server).
    art: Option<String>,
}

/// Stand eines Text-Auftrags (Abflug, Ankunft) für die Ablaufzeile.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub(crate) enum TeilStand {
    /// Nichts mehr davon kommt (z. B. Abflug nach dem Wiedereinstieg im Reiseflug).
    Entfaellt,
    Offen { um: Option<i64> },
    Laeuft,
    Fertig,
    Fehler { um: i64, meldung: String },
    Aufgegeben { meldung: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "art", rename_all = "snake_case")]
pub(crate) enum Freigabe {
    Ja,
    Nein,
    Pruefung,
    NichtErreichbar { um: i64 },
    Kontingent,
}

/// Die Ablaufzeile oben: Freigabe → Texte → Ton → nächste Ansage.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Ablauf {
    freigabe: Freigabe,
    abflug: TeilStand,
    /// Zusatz H: Halbzeit-Ansage, im Reiseflug geschrieben.
    halbzeit: TeilStand,
    ankunft: TeilStand,
    ton_fertig: usize,
    ton_gesamt: usize,
}

/// „Warum diese Ansagen“: Flugart, Muster, Flugzeit, Sprachen.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Profil {
    /// linie | bizjet
    flugart: Option<Flugart>,
    muster: String,
    flugzeit_min: Option<u32>,
    kurzstrecke: bool,
    flugzeit_bekannt: bool,
    sprachen_ohne_stimme: Vec<String>,
    /// Sprachen ohne Sicherheitsvorlage (Server `entfaellt`, Punkt 4).
    sicherheit_ohne_vorlage: Vec<String>,
}

/// Für die Ansage-Seite: was lief und was wartet im aktuellen Flug.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct Ueberblick {
    verlauf: Vec<Gespielt>,
    geplant: Vec<Geplant>,
    sprachen: Vec<String>,
    beta: Option<bool>,
    /// Warum gerade gar keine Ansagen kommen; `None` = es läuft.
    grund: Option<Grund>,
    ablauf: Ablauf,
    profil: Profil,
    /// Läuft gerade eine Ansage oder ein Vorhören (Knöpfe sperren)?
    belegt: bool,
    /// Stumm (bis zum Aufheben oder Neustart).
    stumm: bool,
    /// 0 … 1, wie gerade eingestellt (Regler oben auf der Seite).
    lautstaerke: f32,
    /// Name des Kapitäns laut GSG-Server.
    kapitaen: Option<String>,
    /// Hinweis an den Piloten (z. B. „anschnallzeichen“).
    hinweis: Option<&'static str>,
}

/// `slots`: die Slots dieses Teils, die auf diesem Flug noch kommen.
fn teil_stand(lauf: &Lauf, teil: &str, slots: &[Slot], landung: Option<i64>, kurz: bool) -> TeilStand {
    if slots.is_empty() {
        return TeilStand::Entfaellt;
    }
    // 1f: Eine einzelne Sprache, die auch die Nachholung nicht brachte (oder
    // die gestrichen ist), macht den Teil nicht „aufgegeben“ — das steht nur
    // an der Ansage und Sprache selbst.
    let erledigt = |s: Slot, sp: &String| {
        let k = (s, sp.clone());
        lauf.texte.contains_key(&k) || lauf.texte_fehlen.contains_key(&k) || lauf.ohne_stimme.contains(sp)
    };
    let fertig = !lauf.sprachen.is_empty() && slots.iter().all(|s| lauf.sprachen.iter().all(|sp| erledigt(*s, sp)));
    if fertig {
        let mit_text = slots.iter().any(|s| lauf.sprachen.iter().any(|sp| lauf.texte.contains_key(&(*s, sp.clone()))));
        let meldung = slots.iter().find_map(|s| lauf.texte_fehlen.iter().find(|((x, _), _)| x == s).map(|(_, m)| m.clone()));
        return match meldung.filter(|_| !mit_text) {
            Some(meldung) => TeilStand::Aufgegeben { meldung },
            None => TeilStand::Fertig,
        };
    }
    if let Some(m) = slots.iter().find_map(|s| lauf.texte_fehler.get(s)).cloned() {
        match lauf.fehlversuche.get(teil) {
            Some((n, _)) if *n >= 3 => return TeilStand::Aufgegeben { meldung: m },
            Some((_, ab)) if !lauf.angefragt.contains(teil) => return TeilStand::Fehler { um: *ab, meldung: m },
            Some(_) => {} // läuft gerade wieder
            // Abruf gelang, einzelne Slots fehlten auch nach dem zweiten Abruf.
            None => return TeilStand::Aufgegeben { meldung: m },
        }
    }
    if lauf.angefragt.contains(teil) {
        return TeilStand::Laeuft;
    }
    if teil == "ankunft" && !kurz {
        return TeilStand::Offen { um: landung.map(|l| l - ANKUNFT_TEXT_VOR_LANDUNG_MIN * 60) };
    }
    if teil == "halbzeit" {
        // Hier steht in `landung` der Zeitpunkt der Halbzeit-Ansage.
        return TeilStand::Offen { um: landung.map(|h| h - HALBZEIT_VORLAUF_MIN * 60) };
    }
    TeilStand::Offen { um: None }
}

fn slot_stand(lauf: &Lauf, slot: Slot, landung: Option<i64>, kurz: bool, halbzeit_um: Option<i64>) -> (Stand, Vec<TeilAnsicht>) {
    if slot.rolle() == Rolle::Chime {
        return (Stand::Bereit, Vec::new());
    }
    let sprachen: Vec<String> = if slot.rolle() == Rolle::Kommando { vec!["en-GB".into()] } else { lauf.sprachen.clone() };
    let text_von = |sp: &String| lauf.texte.get(&(slot, sp.clone())).cloned().or_else(|| slot.kommando_text().map(str::to_string));
    let teile: Vec<TeilAnsicht> = sprachen
        .iter()
        .map(|sp| {
            let k = (slot, sp.clone());
            TeilAnsicht {
                sprache: sp.clone(),
                text: text_von(sp),
                ton: matches!(lauf.ton.get(&k), Some(Some(_))),
                keine_stimme: matches!(lauf.ton.get(&k), Some(None)) || lauf.ohne_stimme.contains(sp),
                fehlt: lauf.texte_fehlen.get(&k).filter(|_| !lauf.texte.contains_key(&k)).cloned(),
            }
        })
        .collect();
    let hat_text = teile.iter().any(|t| t.text.is_some());
    if !hat_text {
        let teil = teil_von(slot).unwrap_or("ereignis");
        if let Some(m) = lauf.texte_fehler.get(&slot) {
            return match lauf.fehlversuche.get(teil) {
                Some((n, _)) if *n >= 3 => (Stand::TextAufgegeben { meldung: m.clone() }, teile),
                Some((_, ab)) => (Stand::TextFehler { um: *ab, meldung: m.clone() }, teile),
                // Abruf gelang, nur dieser Slot fehlte (auch nach dem gezielten zweiten Abruf).
                None => (Stand::TextAufgegeben { meldung: m.clone() }, teile),
            };
        }
        // 1f: Keine Sprache dieser Ansage kam, auch nicht nach der Nachholung.
        if let Some(m) = teile.iter().find_map(|t| t.fehlt.clone()) {
            if teile.iter().all(|t| t.fehlt.is_some() || t.keine_stimme) {
                return (Stand::TextAufgegeben { meldung: m }, teile);
            }
        }
        if teil == "halbzeit" && !lauf.angefragt.contains("halbzeit") {
            return (Stand::TextAb { um: halbzeit_um.map(|h| h - HALBZEIT_VORLAUF_MIN * 60), nach_abheben: false }, teile);
        }
        if teil == "ankunft" && !lauf.angefragt.contains("ankunft") {
            // Kurzstrecke: Der Ankunftstext kommt gleich nach dem Abheben.
            let um = if kurz { None } else { landung.map(|l| l - ANKUNFT_TEXT_VOR_LANDUNG_MIN * 60) };
            return (Stand::TextAb { um, nach_abheben: kurz }, teile);
        }
        return (Stand::TextWirdGeschrieben, teile);
    }
    let keys: Vec<(Slot, String)> = sprachen.iter().filter(|sp| text_von(sp).is_some()).map(|sp| (slot, sp.clone())).collect();
    if let Some(m) = keys.iter().find_map(|k| lauf.ton_endgueltig.get(k)) {
        return (Stand::TonAufgegeben { meldung: m.clone() }, teile);
    }
    let fertig = keys.iter().filter(|k| lauf.ton.contains_key(*k)).count();
    if fertig == keys.len() {
        return (Stand::Bereit, teile);
    }
    let fehler = keys
        .iter()
        .filter(|k| !lauf.in_arbeit.contains(*k) && !lauf.ton.contains_key(*k))
        .filter_map(|k| lauf.ton_fehler.get(k))
        .min_by_key(|f| f.ab);
    if let Some(f) = fehler {
        return (Stand::TonWartet { um: f.ab, meldung: f.meldung.clone() }, teile);
    }
    (Stand::TonWirdErzeugt { fertig, von: keys.len() }, teile)
}

/// Fällige und noch kommende Ansagen in Flugreihenfolge: Abflug, Ankunft,
/// Kommandos und Gong (nur, wenn eingeschaltet).
fn geplant(stats: &FlightStats, einst: &Einstellungen, jetzt: i64) -> Vec<Geplant> {
    let lauf = &stats.kabinen_lauf;
    let wartend = lauf.warteschlange.wartend();
    let landung = stats.landung_erwartet;
    let Some(planer) = stats.kabinen_planer.as_ref() else {
        return Vec::new();
    };
    let mut slots: Vec<Slot> = ABFLUG.into_iter().chain(HALBZEIT).chain(ANKUNFT).collect();
    let halbzeit_um = planer.zeitpunkt(Slot::Reiseflug50, landung);
    if einst.kommandos {
        slots.extend(KOMMANDOS);
    } else {
        slots.push(Slot::BoardingAbgeschlossen);
    }
    if einst.chimes {
        slots.extend(GONGS);
    }
    let mut liste: Vec<Geplant> = slots
        .into_iter()
        .chain(wartend.iter().copied().filter(|s| ist_ereignis(*s)))
        .filter(|s| !lauf.entfaellt.contains_key(s))
        .filter(|s| wartend.contains(s) || planer.kommt_noch(*s, stats.phase))
        .filter_map(|slot| {
            let (stand, teile) = slot_stand(lauf, slot, landung, planer.ist_kurzstrecke(), halbzeit_um);
            // Für keine Sprache eine Stimme: kommt nie, steht nicht in der Liste.
            if !teile.is_empty() && teile.iter().all(|t| t.keine_stimme) {
                return None;
            }
            let faellig = wartend.contains(&slot);
            let wartet = if faellig {
                wartegrund(lauf, slot, jetzt)
            } else if slot == Slot::AnschnallzeichenAus && lauf.hinweis == Some("anschnallzeichen") {
                // Wartet auf den Piloten, auch bevor sie fällig ist (Hinweis).
                Some(Wartet::Anschnallzeichen)
            } else {
                None
            };
            let art = (slot == Slot::Reiseflug80).then(|| lauf.art_r80.clone()).flatten();
            Some(Geplant { slot, rolle: rolle_von(lauf, slot), faellig, wartet, wann: planer.zeitpunkt(slot, landung), stand, teile, art })
        })
        .collect();
    // Fällige zuerst, dann in Flugreihenfolge.
    liste.sort_by_key(|g| (!g.faellig, g.slot));
    liste.dedup_by_key(|g| g.slot);
    liste
}

/// Warum keine Ansagen kommen — die Seite zeigt es oben statt einer leeren
/// Seite (Thomas 17.10.2026). Nur, was ALLE Ansagen sperrt; was eine
/// einzelne Ansage aufhält, steht in ihrer Zeile ([`Stand`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "grund", content = "detail", rename_all = "snake_case")]
pub(crate) enum Grund {
    Aus,
    Replay,
    Fracht,
    KeinePax,
    KeinePassagiere,
    BetaPruefung,
    /// Status nicht erreichbar; nächster Versuch um (UTC-Epoch).
    BetaNichtErreichbar(i64),
    NichtFreigeschaltet,
    /// Freigeschaltet, Monatsdeckel erreicht: Vorhandenes geht, Neues nicht.
    Kontingent,
    SimAngehalten,
}

pub(crate) fn grund(an: bool, stats: &FlightStats) -> Option<Grund> {
    if !an {
        return Some(Grund::Aus);
    }
    if stats.replay_verdacht {
        return Some(Grund::Replay);
    }
    // Flugart allein (Fracht, Positionierung …), unabhaengig von den Pax.
    if cabin_announcements::flugart::einordnen(stats.flight_type.as_deref(), Some(1), "") == Flugart::Aus {
        return Some(Grund::Fracht);
    }
    let lauf = &stats.kabinen_lauf;
    match lauf.beta {
        Beta::Nein => return Some(Grund::NichtFreigeschaltet),
        Beta::Unbekannt if lauf.beta_netzfehler => return Some(Grund::BetaNichtErreichbar(lauf.beta_naechste_pruefung)),
        Beta::Unbekannt | Beta::Angefragt => return Some(Grund::BetaPruefung),
        Beta::Ja => {}
    }
    match stats.pax_count {
        None => return Some(Grund::KeinePax),
        Some(n) if n <= 0 => return Some(Grund::KeinePassagiere),
        Some(_) => {}
    }
    if lauf.sim_angehalten {
        return Some(Grund::SimAngehalten);
    }
    lauf.kontingent_erschoepft.then_some(Grund::Kontingent)
}

/// Fehlertext fuer die Anzeige: eine Zeile, hoechstens 160 Zeichen.
fn kurz(text: &str) -> String {
    let zeile = text.lines().next().unwrap_or("").trim();
    zeile.chars().take(160).collect()
}

fn ueberblick_bauen(stats: &FlightStats, einst: &Einstellungen, flug: &cabin_announcements::Flugdaten, muster: &str, jetzt: i64) -> Ueberblick {
    let lauf = &stats.kabinen_lauf;
    let geplant = geplant(stats, einst, jetzt);
    let landung = stats.landung_erwartet;
    let planer = stats.kabinen_planer.as_ref();
    let wartend = lauf.warteschlange.wartend();
    let kommend = |slots: &[Slot]| -> Vec<Slot> {
        slots
            .iter()
            .copied()
            .filter(|s| !lauf.entfaellt.contains_key(s))
            .filter(|s| wartend.contains(s) || planer.is_some_and(|p| p.kommt_noch(*s, stats.phase)))
            .collect()
    };
    let freigabe = match lauf.beta {
        Beta::Ja if lauf.kontingent_erschoepft => Freigabe::Kontingent,
        Beta::Ja => Freigabe::Ja,
        Beta::Nein => Freigabe::Nein,
        Beta::Unbekannt if lauf.beta_netzfehler => Freigabe::NichtErreichbar { um: lauf.beta_naechste_pruefung },
        _ => Freigabe::Pruefung,
    };
    let mit_ton: Vec<&Geplant> = geplant.iter().filter(|g| g.rolle != Rolle::Chime).collect();
    let ablauf = Ablauf {
        freigabe,
        abflug: teil_stand(lauf, "abflug", &kommend(&ABFLUG), landung, false),
        halbzeit: teil_stand(lauf, "halbzeit", &kommend(&HALBZEIT), planer.and_then(|p| p.zeitpunkt(Slot::Reiseflug50, landung)), false),
        ankunft: teil_stand(lauf, "ankunft", &kommend(&ANKUNFT), landung, planer.is_some_and(|p| p.ist_kurzstrecke())),
        ton_fertig: mit_ton.iter().filter(|g| g.stand == Stand::Bereit).count(),
        ton_gesamt: mit_ton.len(),
    };
    // Flugweit gestrichene Sprachen (Punkt 4) zuerst, auch wenn der Server
    // sie gar nicht mehr in `sprachen` führt.
    let mut ohne_stimme: Vec<String> = lauf.ohne_stimme.iter().cloned().collect();
    ohne_stimme.extend(
        lauf.sprachen
            .iter()
            .filter(|sp| !lauf.ohne_stimme.contains(*sp) && lauf.ton.iter().any(|((_, s), t)| s == *sp && t.is_none()))
            .cloned(),
    );
    let flugdaten = stats.kabinen_planer.as_ref();
    Ueberblick {
        grund: grund(einst.an, stats),
        verlauf: lauf.verlauf.iter().filter(|g| !g.vorschau).cloned().collect(),
        geplant,
        sprachen: lauf.sprachen.clone(),
        beta: match lauf.beta {
            Beta::Ja => Some(true),
            Beta::Nein => Some(false),
            _ => None,
        },
        ablauf,
        profil: Profil {
            flugart: flug.flugart,
            muster: muster.to_string(),
            flugzeit_min: flug.geplante_flugzeit_min,
            kurzstrecke: flugdaten.is_some_and(|p| p.ist_kurzstrecke()),
            flugzeit_bekannt: flugdaten.is_some_and(|p| p.flugzeit_bekannt()),
            sprachen_ohne_stimme: ohne_stimme,
            sicherheit_ohne_vorlage: lauf.ohne_vorlage.iter().cloned().collect(),
        },
        belegt: lauf.warteschlange.belegt(jetzt),
        stumm: STUMM.load(Ordering::SeqCst),
        lautstaerke: ausgabe::lautstaerke(),
        kapitaen: lauf.crew.as_ref().and_then(|c| c.kapitaen.clone()),
        hinweis: lauf.hinweis,
    }
}

#[tauri::command]
pub(crate) fn kabinen_ueberblick(app: AppHandle, state: tauri::State<'_, AppState>) -> Option<Ueberblick> {
    let flight = aktiver_flug(&state)?;
    let einst = einstellungen(&app);
    let geladen = flight.stats.lock().expect("flight stats").kabinen_lauf.verlauf_geladen;
    let alt = if geladen { None } else { Some(verlauf_datei_lesen(&app, &flight)) };
    let mut stats = flight.stats.lock().expect("flight stats");
    if let Some(alt) = alt {
        verlauf_uebernehmen(&mut stats.kabinen_lauf, alt);
    }
    let flug = crate::kabinen_flugdaten(&stats, &flight.arr_airport, flight.planned_flight_time_min, &flight.aircraft_icao);
    Some(ueberblick_bauen(&stats, &einst, &flug, &flight.aircraft_icao, Utc::now().timestamp()))
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

    /// Klang der Wiedergabe (Thomas 18.10.2026): klar, Bordlautsprecher (ab
    /// Werk) oder dumpf wie durch die Cockpittür (Michael: „klingt zu nah“).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Klang {
        Klar,
        Lautsprecher,
        Tuer,
    }

    impl Klang {
        pub(crate) fn aus_name(n: &str) -> Klang {
            match n {
                "klar" => Klang::Klar,
                "tuer" => Klang::Tuer,
                _ => Klang::Lautsprecher,
            }
        }
    }

    /// Biquad (RBJ-Kochbuch), 2. Ordnung, Q = 0,707.
    struct Biquad {
        b: [f32; 3],
        a: [f32; 2],
        z: [f32; 2],
    }

    impl Biquad {
        fn neu(hochpass: bool, f: f32, rate: u32) -> Biquad {
            let w = 2.0 * std::f32::consts::PI * f / rate.max(1) as f32;
            let (sin, cos) = w.sin_cos();
            let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
            let a0 = 1.0 + alpha;
            let b = if hochpass {
                [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
            } else {
                [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
            };
            Biquad { b: [b[0] / a0, b[1] / a0, b[2] / a0], a: [-2.0 * cos / a0, (1.0 - alpha) / a0], z: [0.0; 2] }
        }
        fn schritt(&mut self, x: f32) -> f32 {
            let y = self.b[0] * x + self.z[0];
            self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
            self.z[1] = self.b[2] * x - self.a[1] * y;
            y
        }
    }

    /// Färbt die Proben je nach Klang: Bordlautsprecher = Band 300–3.400 Hz
    /// (wie eine Kabinen-Durchsage), Tür = zusätzlich dumpf (Tiefpass
    /// 1.200 Hz). Etwas angehoben, damit es nicht leiser wirkt; begrenzt auf ±1.
    pub(crate) fn klang_anwenden(p: &mut [f32], rate: u32, klang: Klang) {
        let (unten, oben, verstaerkung) = match klang {
            Klang::Klar => return,
            Klang::Lautsprecher => (300.0, 3_400.0, 1.3),
            Klang::Tuer => (250.0, 1_200.0, 1.6),
        };
        let mut hp = [Biquad::neu(true, unten, rate), Biquad::neu(true, unten, rate)];
        let mut tp = [Biquad::neu(false, oben, rate), Biquad::neu(false, oben, rate)];
        for x in p.iter_mut() {
            let mut y = *x;
            for f in hp.iter_mut().chain(tp.iter_mut()) {
                y = f.schritt(y);
            }
            *x = (y * verstaerkung).clamp(-1.0, 1.0);
        }
    }

    /// Lautstärke 0 … 1 als f32-Bits: eine Quelle für jede Wiedergabe, auch
    /// die laufende (der Rückruf liest sie je Puffer).
    static LAUTSTAERKE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3F4C_CCCD); // 0.8

    pub(crate) fn lautstaerke_setzen(v: f32) {
        let v = if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.8 };
        LAUTSTAERKE.store(v.to_bits(), Ordering::Relaxed);
    }
    pub(crate) fn lautstaerke() -> f32 {
        f32::from_bits(LAUTSTAERKE.load(Ordering::Relaxed))
    }

    /// Ein Ausgabegeraet fuer die Auswahl in den Einstellungen.
    #[derive(Debug, Clone, serde::Serialize)]
    pub(crate) struct Geraet {
        pub id: String,
        pub name: String,
    }

    /// Mono-Proben (−1 … 1) und Abtastrate aus einer PCM-16-WAV (der
    /// Ton-Server liefert genau das). Mehrkanalig wird gemittelt.
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
    pub(crate) fn spielen(geraet: Option<&str>, rate: u32, proben: &[f32], stand: u64, klang: Klang) -> Result<(), String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        use std::sync::atomic::{AtomicBool, AtomicUsize};
        use std::sync::Arc;

        if STOPP.load(Ordering::SeqCst) != stand {
            return Ok(());
        }
        // Boarding-Musik pausiert, solange eine Ansage (auch der Gong) läuft.
        let _ansage = crate::kabinen_musik::AnsageLaeuft::neu();
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
        // Die Lautstärke kommt erst im Rückruf dazu, damit der Regler auch
        // eine laufende Ansage lauter oder leiser macht.
        let mut daten: Vec<f32> = umrechnen(proben, rate, ziel);
        klang_anwenden(&mut daten, ziel, klang);
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
                    let v = lautstaerke();
                    for rahmen in puffer.chunks_mut(kanaele) {
                        let x = (daten.get(i).copied().unwrap_or(0.0) * v).clamp(-1.0, 1.0);
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
    pub(crate) fn spielen(_: Option<&str>, _: u32, _: &[f32], _: u64, _: Klang) -> Result<(), String> {
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
    /// Jede Sprache fragt `auto:<sprache>:<m|w>` an; der Server waehlt die
    /// Stimme (`SPRACHEN`, Supertonic, nach Flug und Rolle). So kommen Sprachen und
    /// Stimmwechsel ohne neue Client-Beta (Thomas 17.10.2026). Hat er keine,
    /// liefert [`sprechen`] `Ok(None)` und die Sprache entfaellt.
    pub(crate) fn stimme_fuer(sprache: &str, m: bool) -> Option<String> {
        super::ist_locale(sprache).then(|| format!("auto:{sprache}:{}", if m { "m" } else { "w" }))
    }

    /// `{"error":"keine_stimme"}` vom Server (recorder/src/kabinenTon.ts).
    pub(crate) fn keine_stimme(koerper: &str) -> bool {
        serde_json::from_str::<serde_json::Value>(koerper)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|e| e == "keine_stimme"))
            .unwrap_or(false)
    }

    /// Ton vom Server. `Ok(None)`:
    /// Der Server hat fuer diese Sprache keine Stimme (404). `Err` heißt „gerade nicht“ (kein Token, Netz,
    /// ausgelastet) — der Aufrufer versucht es später wieder.
    /// Antwort des Servers: Ton, die tatsaechlich gewaehlte Stimme
    /// (`X-Stimme`) und ob ein Ersatz sprach (`X-Ersatz`).
    pub(crate) struct ServerTon {
        pub wav: Vec<u8>,
        pub stimme: Option<String>,
        pub ersatz: bool,
    }

    /// Wonach der Server die Stimme waehlt (ab 2.1.0, siehe `super::Wunsch`).
    #[derive(Clone, Copy, Default)]
    pub(crate) struct Wahl<'a> {
        pub flug: Option<&'a str>,
        pub rolle: Option<&'static str>,
        pub fremd: &'a [api_client::KabinenFremd],
    }

    /// Koerper der Anfrage an /api/kabine/ton.
    pub(crate) fn koerper(stimme: &str, text: &str, gebraucht_in_s: Option<i64>, teil: Option<&str>, slot: Option<String>, wahl: Wahl<'_>) -> serde_json::Value {
        // Wofuer der Ton gebraucht wird: Der Server arbeitet danach ab
        // (faellig vor Vorrat, Abflug vor Ankunft, reihum je Pilot).
        let mut koerper = serde_json::json!({ "stimme": stimme, "text": text });
        if let Some(s) = gebraucht_in_s {
            koerper["gebraucht_in_s"] = serde_json::json!(s);
        }
        if let Some(t) = teil {
            koerper["teil"] = serde_json::json!(t);
        }
        if let Some(s) = slot {
            koerper["slot"] = serde_json::json!(s);
        }
        // `pirep_id`: waehlt die Stimmen des Flugs und steht im Protokoll
        // des Servers (Zeile `kabine_ton`).
        if let Some(f) = wahl.flug {
            koerper["pirep_id"] = serde_json::json!(f);
        }
        if let Some(r) = wahl.rolle {
            koerper["rolle"] = serde_json::json!(r);
        }
        // Fremde Namen (2.1.0): Der Server spricht sie in ihrer Sprache.
        if !wahl.fremd.is_empty() {
            koerper["fremd"] = serde_json::json!(wahl.fremd);
        }
        koerper
    }

    pub(crate) async fn sprechen(
        stimme: &str,
        text: &str,
        gebraucht_in_s: Option<i64>,
        teil: Option<&str>,
        slot: Option<String>,
        wahl: Wahl<'_>,
    ) -> Result<Option<ServerTon>, super::TonFehlerAntwort> {
        use super::TonFehlerAntwort as F;
        let token = crate::live_zugang::pilot_token().ok_or_else(|| F::spaeter("noch kein Live-Zugang (nicht provisioniert)", Some(120)))?;
        let koerper = koerper(stimme, text, gebraucht_in_s, teil, slot, wahl);
        let antwort = reqwest::Client::builder()
            // Lange Ansagen rechnen bis ~1,5 min, dazu die
            // Warteschlange des Servers.
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| F::spaeter(e.to_string(), None))?
            .post(format!("{}/api/kabine/ton", crate::live_zugang::LIVE_BASIS))
            .bearer_auth(token)
            .header("User-Agent", format!("AeroACARS/{}", env!("CARGO_PKG_VERSION")))
            .json(&koerper)
            .send()
            .await
            .map_err(|e| F::spaeter(format!("live.kant.ovh nicht erreichbar: {e}"), None))?;
        let status = antwort.status();
        // Nur die ausdrueckliche Antwort „keine Stimme fuer diese Sprache“ ist
        // endgueltig; jeder andere 404 (Route fehlt, Proxy) ist vorlaeufig.
        if status == reqwest::StatusCode::NOT_FOUND {
            let koerper = antwort.text().await.unwrap_or_default();
            return if keine_stimme(&koerper) { Ok(None) } else { Err(F::spaeter("Piper-Server antwortet 404", Some(120))) };
        }
        if !status.is_success() {
            let warte = warte_s(antwort.headers().get("retry-after").and_then(|v| v.to_str().ok()));
            let meldung = format!("Piper-Server antwortet {status}");
            // 400: Diese Anfrage geht nie (Text zu lang, Stimme unbekannt).
            return Err(if status == reqwest::StatusCode::BAD_REQUEST {
                F { meldung, warte_s: 0, endgueltig: true }
            } else {
                F::spaeter(meldung, warte)
            });
        }
        let kopf = |n: &str| antwort.headers().get(n).and_then(|v| v.to_str().ok()).map(str::to_string);
        let gewaehlt = kopf("x-stimme");
        let ersatz = kopf("x-ersatz").is_some();
        let wav = antwort.bytes().await.map_err(|e| F::spaeter(e.to_string(), None))?.to_vec();
        Ok(Some(ServerTon { wav, stimme: gewaehlt, ersatz }))
    }

    /// `Retry-After` in Sekunden (die Datumsform schickt der Server nicht).
    pub(crate) fn warte_s(kopf: Option<&str>) -> Option<u64> {
        kopf.and_then(|k| k.trim().parse::<u64>().ok())
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
        let kaputt = Einstellungen { zweite_sprache: "klingonisch".into(), lautstaerke: 7.0, ..Einstellungen::default() }.normalisiert();
        assert_eq!(kaputt.zweite_sprache, "airline");
        assert_eq!(kaputt.lautstaerke, 1.0);
        let de = Einstellungen { zweite_sprache: "airline_de".into(), ..Einstellungen::default() }.normalisiert();
        assert_eq!(de.zweite_sprache, "airline_de", "Airline + Deutsch bleibt erhalten");
        // Fruehere Modi (keine/ziel/eigene) gibt es nicht mehr: Airline + Englisch.
        for alt in ["keine", "ziel", "eigene"] {
            let e = Einstellungen { zweite_sprache: alt.into(), ..Einstellungen::default() }.normalisiert();
            assert_eq!(e.zweite_sprache, "airline", "{alt}");
        }
        // Alte Datei mit entfernten Feldern (Azure, Anbieter, Tagesform) laedt.
        let alt: Einstellungen = serde_json::from_str(
            r#"{"an": true, "anbieter_kapitaen": "azure", "azure_region": "westeurope", "tagesform_kabine": "witzig", "eigene_sprache": "fr-FR", "zweite_sprache": "eigene"}"#,
        )
        .unwrap();
        assert!(alt.an && alt.kommandos && alt.chimes);
        assert_eq!(alt.normalisiert().zweite_sprache, "airline");
        // Nichts davon wird wieder geschrieben.
        let json = serde_json::to_string(&Einstellungen::default()).unwrap();
        for weg in ["azure", "anbieter", "tagesform", "eigene_sprache", "englisch"] {
            assert!(!json.contains(weg), "{weg} steckt noch in {json}");
        }
    }

    #[test]
    fn server_waehlt_die_stimme_jeder_sprache() {
        // Jede Sprache, auch Deutsch und Englisch, fragt nur Sprache + Rolle an.
        assert_eq!(piper::stimme_fuer("de-DE", true).as_deref(), Some("auto:de-DE:m"));
        assert_eq!(piper::stimme_fuer("en-GB", false).as_deref(), Some("auto:en-GB:w"));
        assert_eq!(piper::stimme_fuer("lv-LV", false).as_deref(), Some("auto:lv-LV:w"));
        assert_eq!(piper::stimme_fuer("frFR", true), None, "nur gueltige Sprachkennung");
        // Nur die ausdrueckliche Server-Antwort ist „keine Stimme“.
        assert!(piper::keine_stimme(r#"{"error":"keine_stimme"}"#));
        assert!(!piper::keine_stimme("<html>Not Found</html>"));
        assert!(!piper::keine_stimme(r#"{"error":"stimme_unbekannt"}"#));
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



    #[test]
    fn ton_wird_wiederholt_bis_er_kommt_nur_400_ist_endgueltig() {
        let mut l = Lauf::default();
        let k = (Slot::Boarding, "de-DE".to_string());
        assert!(ton_noetig(&l, &k, 0));
        l.in_arbeit.insert(k.clone());
        assert!(!ton_noetig(&l, &k, 0), "läuft schon");
        l.in_arbeit.remove(&k);
        let fehler = |versuche, ab| TonFehler { versuche, ab, meldung: "503".into() };
        l.ton_fehler.insert(k.clone(), fehler(1, 60));
        assert!(!ton_noetig(&l, &k, 30), "Retry-After abwarten");
        assert!(ton_noetig(&l, &k, 60));
        // Früher war nach fünf Fehlern Schluss — die Ansage kam dann nie.
        l.ton_fehler.insert(k.clone(), fehler(25, 0));
        assert!(ton_noetig(&l, &k, 999), "keine feste Versuchszahl mehr");
        l.ton_endgueltig.insert(k.clone(), "400".into());
        assert!(!ton_noetig(&l, &k, 999), "400 ist endgültig");
        // Keine Stimme für die Sprache ist endgültig, kein Fehler.
        let k2 = (Slot::Sicherheit, "lv-LV".to_string());
        l.ton.insert(k2.clone(), None);
        assert!(!ton_noetig(&l, &k2, 999));
        // Retry-After des Servers.
        assert_eq!(piper::warte_s(Some("20")), Some(20));
        assert_eq!(piper::warte_s(Some("Wed, 21 Oct 2026 07:28:00 GMT")), None);
        assert_eq!(piper::warte_s(None), None);
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

    fn planer_mit(flugart: Flugart, phase: crate::FlightPhase) -> cabin_announcements::Planer {
        cabin_announcements::Planer::fortsetzen(
            cabin_announcements::Flugdaten {
                flugart: Some(flugart),
                geplante_flugzeit_min: Some(120),
                tod_nach_abheben_s: None,
                ziel_plan_icao: None,
                reiseflughoehe_ft: None,
                abheben_epoch: None,
            },
            phase,
        )
    }

    fn ton() -> Option<Ton> {
        Some(Ton { datei: "f/x.wav".into(), dauer_s: 1.0, stuecke: vec![], herkunft: "server" })
    }

    fn einst() -> Einstellungen {
        Einstellungen { an: true, ..Einstellungen::default() }
    }

    fn zeile(st: &FlightStats, slot: Slot) -> Option<Geplant> {
        geplant(st, &einst(), 0).into_iter().find(|g| g.slot == slot)
    }

    /// Thomas 17.10.2026: „die anderen stehen immer noch auf wird
    /// vorbereitet“ — jede Zeile sagt, was wirklich los ist.
    #[test]
    fn jede_ansage_sagt_ehrlich_wo_sie_steht() {
        let mut st = FlightStats::new();
        assert!(geplant(&st, &einst(), 0).is_empty(), "vor dem ersten Takt nichts");
        st.kabinen_planer = Some(cabin_announcements::Planer::fortsetzen(
            cabin_announcements::Flugdaten {
                flugart: Some(Flugart::Linie),
                geplante_flugzeit_min: Some(120),
                abheben_epoch: Some(0),
                ..Default::default()
            },
            crate::FlightPhase::Cruise,
        ));
        st.phase = crate::FlightPhase::Cruise;
        st.landung_erwartet = Some(10_000);
        st.kabinen_lauf.sprachen = vec!["fr-FR".into(), "de-DE".into()];

        // Ankunft noch nicht angefragt: „Text ab ca. …“ (Landung − 45 min), nicht „wird vorbereitet“.
        assert_eq!(zeile(&st, Slot::KapitaenSinkflug).unwrap().stand, Stand::TextAb { um: Some(10_000 - 45 * 60), nach_abheben: false });
        st.kabinen_lauf.angefragt.insert("ankunft".into());
        assert_eq!(zeile(&st, Slot::KapitaenSinkflug).unwrap().stand, Stand::TextWirdGeschrieben);

        // Text da, Ton für eine von zwei Sprachen.
        let r50 = Slot::Reiseflug50;
        for sp in ["fr-FR", "de-DE"] {
            st.kabinen_lauf.texte.insert((r50, sp.into()), format!("Text {sp}"));
        }
        st.kabinen_lauf.ton.insert((r50, "fr-FR".into()), ton());
        let z = zeile(&st, r50).unwrap();
        assert_eq!(z.stand, Stand::TonWirdErzeugt { fertig: 1, von: 2 });
        assert_eq!(z.teile.len(), 2, "beide Sprachen aufklappbar");
        assert_eq!(z.teile[1].text.as_deref(), Some("Text de-DE"));
        assert!(z.wann.is_some(), "Reiseflug hat eine Uhrzeit");
        // Server ausgelastet: neuer Versuch um …
        st.kabinen_lauf.ton_fehler.insert((r50, "de-DE".into()), TonFehler { versuche: 2, ab: 777, meldung: "503".into() });
        assert_eq!(zeile(&st, r50).unwrap().stand, Stand::TonWartet { um: 777, meldung: "503".into() });
        // Fertig.
        st.kabinen_lauf.ton.insert((r50, "de-DE".into()), ton());
        assert_eq!(zeile(&st, r50).unwrap().stand, Stand::Bereit);
        // Keine Stimme für eine Sprache: trotzdem bereit, und so markiert.
        let r80 = Slot::Reiseflug80;
        for sp in ["fr-FR", "de-DE"] {
            st.kabinen_lauf.texte.insert((r80, sp.into()), "x".into());
        }
        st.kabinen_lauf.ton.insert((r80, "fr-FR".into()), None);
        st.kabinen_lauf.ton.insert((r80, "de-DE".into()), ton());
        let z = zeile(&st, r80).unwrap();
        assert_eq!(z.stand, Stand::Bereit);
        assert!(z.teile[0].keine_stimme);
        // Für keine Sprache eine Stimme: kommt nie, steht nicht in der Liste.
        st.kabinen_lauf.ton.insert((r80, "de-DE".into()), None);
        assert!(zeile(&st, r80).is_none());
        // 400 vom Server: aufgegeben, Grund steht da.
        st.kabinen_lauf.texte.insert((Slot::Reiseflug25, "fr-FR".into()), "x".into());
        st.kabinen_lauf.ton_endgueltig.insert((Slot::Reiseflug25, "fr-FR".into()), "400".into());
        assert_eq!(zeile(&st, Slot::Reiseflug25).unwrap().stand, Stand::TonAufgegeben { meldung: "400".into() });

        // Gong und Kommandos stehen da, wenn eingeschaltet; Gong ist immer „bereit“.
        assert_eq!(zeile(&st, Slot::Chime10000Sinkflug).unwrap().stand, Stand::Bereit);
        assert!(zeile(&st, Slot::KommandoPlaetzeLandung).is_some());
        let ohne = Einstellungen { chimes: false, kommandos: false, ..einst() };
        assert!(!geplant(&st, &ohne, 0).iter().any(|g| matches!(g.rolle, Rolle::Chime | Rolle::Kommando)));

        // Im Reiseflug ist der Abflug am Boden vorbei.
        assert!(zeile(&st, Slot::Boarding).is_none());
        assert!(zeile(&st, Slot::Chime10000Steigflug).is_none());
    }

    #[test]
    fn faellige_ansage_nennt_den_grund_des_wartens() {
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, crate::FlightPhase::Cruise));
        st.phase = crate::FlightPhase::Cruise;
        st.kabinen_lauf.sprachen = vec!["de-DE".into()];
        st.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::Reiseflug50, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        let z = zeile(&st, Slot::Reiseflug50).unwrap();
        assert!(z.faellig);
        assert_eq!(z.wartet, Some(Wartet::Ton), "noch kein Text/Ton");
        st.kabinen_lauf.texte.insert((Slot::Reiseflug50, "de-DE".into()), "x".into());
        st.kabinen_lauf.ton.insert((Slot::Reiseflug50, "de-DE".into()), ton());
        assert_eq!(zeile(&st, Slot::Reiseflug50).unwrap().wartet, None, "spielt im nächsten Takt");
        st.kabinen_lauf.ruhephase = true;
        assert_eq!(zeile(&st, Slot::Reiseflug50).unwrap().wartet, Some(Wartet::Ruhephase));
        st.kabinen_lauf.warteschlange.spielt_bis(100);
        assert_eq!(zeile(&st, Slot::Reiseflug50).unwrap().wartet, Some(Wartet::AndereAnsage));
        // Nach einer echten Ansage: eigener Grund „Pause“, nicht „läuft gerade“.
        st.kabinen_lauf.warteschlange.ansage_zu_ende(-10);
        assert_eq!(zeile(&st, Slot::Reiseflug50).unwrap().wartet, Some(Wartet::Pause));
        st.kabinen_lauf.warteschlange.ansage_zu_ende(-1_000);
        assert_ne!(zeile(&st, Slot::Reiseflug50).unwrap().wartet, Some(Wartet::Pause), "Pause vorbei");
        st.kabinen_lauf.sim_angehalten = true;
        assert_eq!(zeile(&st, Slot::Reiseflug50).unwrap().wartet, Some(Wartet::SimAngehalten));
        // Fällige stehen oben.
        assert_eq!(geplant(&st, &einst(), 0)[0].slot, Slot::Reiseflug50);
    }

    /// Codex-QS beta.5: Fehlt der Text einer Sprache noch (Nachholung
    /// läuft), spielt die Ansage nicht einsprachig; fehlt er endgültig, mit
    /// den übrigen Sprachen.
    #[test]
    fn ansage_erst_bereit_wenn_jede_sprache_da_oder_endgueltig_fehlt() {
        let mut l = Lauf::default();
        let sp = vec!["fr-FR".to_string(), "de-DE".to_string()];
        let s = Slot::Reiseflug50;
        l.texte.insert((s, "fr-FR".into()), "x".into());
        l.ton.insert((s, "fr-FR".into()), ton());
        assert!(!ansage_bereit(&l.texte, &l.ton, &l.texte_fehler, &l.texte_fehlen, &sp, s), "Deutsch wird noch nachgeholt");
        l.texte_fehler.insert(s, "kein Text".into());
        assert!(ansage_bereit(&l.texte, &l.ton, &l.texte_fehler, &l.texte_fehlen, &sp, s), "Deutsch fehlt endgültig");
        l.texte_fehler.clear();
        l.texte.insert((s, "de-DE".into()), "y".into());
        assert!(!ansage_bereit(&l.texte, &l.ton, &l.texte_fehler, &l.texte_fehlen, &sp, s), "Ton fehlt noch");
        l.ton.insert((s, "de-DE".into()), None);
        assert!(ansage_bereit(&l.texte, &l.ton, &l.texte_fehler, &l.texte_fehlen, &sp, s), "keine Stimme zählt als erledigt");
        assert!(!ansage_bereit(&l.texte, &l.ton, &l.texte_fehler, &l.texte_fehlen, &sp, Slot::Reiseflug80), "ohne Text nie");
    }

    /// beta.6 (Thomas 18.10.2026): Bizjets bekommen den Gong und die
    /// Sicherheit als Kurzfassung des Kapitäns — keine Kabine, keine Kommandos.
    #[test]
    fn bizjet_kapitaen_gong_und_sicherheit_nur_als_kurzfassung() {
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Bizjet, crate::FlightPhase::Boarding));
        st.kabinen_lauf.sprachen = vec!["de-DE".into()];
        let mut slots: Vec<_> = geplant(&st, &einst(), 0).into_iter().map(|g| g.slot).collect();
        slots.sort();
        let mut erwartet = vec![Slot::Sicherheit, Slot::KapitaenBegruessung, Slot::Chime10000Steigflug, Slot::KapitaenSinkflug, Slot::Chime10000Sinkflug];
        erwartet.sort();
        assert_eq!(slots, erwartet);
        // Liefert der Server keine Kurzfassung, steht die Sicherheit nicht mehr da.
        st.kabinen_lauf.entfaellt.insert(Slot::Sicherheit, "kein_kurztext".into());
        assert!(zeile(&st, Slot::Sicherheit).is_none());
    }

    fn eintrag(slot: &str, sprache: &str, rolle: Option<&str>) -> api_client::KabinenEintrag {
        api_client::KabinenEintrag { slot: slot.into(), sprache: sprache.into(), text: format!("{slot} {sprache}"), rolle: rolle.map(str::to_string), art: None, fremd: Vec::new() }
    }

    #[test]
    fn sicherheit_mit_kennzeichen_spricht_der_kapitaen() {
        let abflug = [Slot::Sicherheit, Slot::KapitaenBegruessung];
        // Bizjet, neues Modul: Kurzfassung mit Kennzeichen → Kapitän spricht.
        let mut l = Lauf::default();
        let d = api_client::KabinenDrehbuch {
            sprachen: vec!["de-DE".into(), "en-GB".into()],
            eintraege: vec![eintrag("sicherheit", "de-DE", Some("kapitaen")), eintrag("sicherheit", "en-GB", Some("kapitaen")), eintrag("kapitaen_begruessung", "de-DE", None), eintrag("kapitaen_begruessung", "en-GB", None)], crew: None, boarding_lied: None, boarding_lied_spaeter: false, entfaellt: Default::default() };
        assert!(drehbuch_uebernehmen(&mut l, d, &abflug, true, false).is_empty());
        assert_eq!(rolle_von(&l, Slot::Sicherheit), Rolle::Kapitaen);
        assert!(l.entfaellt.is_empty());
        // Bizjet, altes Modul: Kabinentext ohne Kennzeichen → nicht übernommen,
        // entfällt sicher und wird nicht nachgeholt.
        let mut alt = Lauf::default();
        let d = api_client::KabinenDrehbuch {
            sprachen: vec!["de-DE".into()],
            eintraege: vec![eintrag("sicherheit", "de-DE", None), eintrag("kapitaen_begruessung", "de-DE", None)], crew: None, boarding_lied: None, boarding_lied_spaeter: false, entfaellt: Default::default() };
        assert!(drehbuch_uebernehmen(&mut alt, d, &abflug, true, false).is_empty(), "nichts nachholen");
        assert!(!alt.texte.contains_key(&(Slot::Sicherheit, "de-DE".to_string())));
        assert_eq!(alt.entfaellt.get(&Slot::Sicherheit).map(String::as_str), Some("kein_kurztext"));
        // Linie ohne Kennzeichen: Kabine wie bisher.
        let mut linie = Lauf::default();
        let d = api_client::KabinenDrehbuch { sprachen: vec!["de-DE".into()], eintraege: vec![eintrag("sicherheit", "de-DE", None)], crew: None, boarding_lied: None, boarding_lied_spaeter: false, entfaellt: Default::default() };
        drehbuch_uebernehmen(&mut linie, d, &[Slot::Sicherheit], false, false);
        assert_eq!(rolle_von(&linie, Slot::Sicherheit), Rolle::Kabine);
        assert!(linie.texte.contains_key(&(Slot::Sicherheit, "de-DE".to_string())));
    }

    fn lauf_mit_ton(slots: &[Slot]) -> Lauf {
        let mut l = Lauf { sprachen: vec!["de-DE".into()], ..Lauf::default() };
        for s in slots {
            l.texte.insert((*s, "de-DE".into()), format!("{s:?}"));
            l.ton.insert((*s, "de-DE".into()), ton());
        }
        l
    }

    /// Thomas 18.10.2026: Stumm überspringt, holt nicht nach; die laufende
    /// Ansage gilt als stumm, und es gibt keine Pause (nichts war zu hören).
    #[test]
    fn stumm_verbucht_laufendes_und_wartendes_ohne_pause() {
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, crate::FlightPhase::Cruise));
        st.kabinen_lauf = lauf_mit_ton(&[Slot::Reiseflug50, Slot::Reiseflug80, Slot::Turbulenz]);
        st.kabinen_lauf.verlauf.push(Gespielt { id: 1, slot: Slot::Reiseflug50, rolle: Rolle::Kapitaen, zeit: 0, teile: vec![], vorschau: false, ergebnis: None, gestartet: true, grund: None });
        st.kabinen_lauf.naechste_id = 1;
        st.kabinen_lauf.warteschlange.ansage_spielt_bis(100);
        st.kabinen_lauf.warteschlange.aufnehmen([
            Faellig { slot: Slot::Reiseflug80, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false },
            Faellig { slot: Slot::Turbulenz, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false },
        ]);
        assert!(stumm_anwenden(&mut st, 10));
        let l = &st.kabinen_lauf;
        let ergebnisse: Vec<(Slot, Option<&str>)> = l.verlauf.iter().map(|g| (g.slot, g.ergebnis.as_deref())).collect();
        assert_eq!(ergebnisse, vec![(Slot::Reiseflug50, Some("stumm")), (Slot::Reiseflug80, Some("stumm")), (Slot::Turbulenz, Some("stumm"))]);
        assert!(l.warteschlange.ist_leer());
        assert!(!l.warteschlange.belegt(11) && !l.warteschlange.in_pause(11), "keine Pause nach Stumm");
        assert_eq!(l.verlauf[1].teile.len(), 1, "Text bleibt zum Nachlesen");
        // Gegenprobe: Rückmeldung „stumm“ der Oberfläche macht auch keine Pause,
        // „gespielt“ schon.
        let mut l = lauf_mit_ton(&[]);
        l.verlauf.push(Gespielt { id: 5, slot: Slot::Reiseflug25, rolle: Rolle::Kabine, zeit: 0, teile: vec![], vorschau: false, ergebnis: None, gestartet: true, grund: None });
        assert!(wiedergabe_ende(&mut l, 5, "stumm", Utc::now().timestamp()));
        assert!(!l.warteschlange.in_pause(Utc::now().timestamp()));
        l.verlauf.push(Gespielt { id: 6, slot: Slot::Reiseflug50, rolle: Rolle::Kapitaen, zeit: 0, teile: vec![], vorschau: false, ergebnis: None, gestartet: true, grund: None });
        assert!(wiedergabe_ende(&mut l, 6, "gespielt", Utc::now().timestamp()));
        assert!(l.warteschlange.in_pause(Utc::now().timestamp()));
    }

    /// Codex beta.6: Was verfällt oder nicht gespielt werden kann, steht in
    /// „Bisher“ mit Grund, statt still zu verschwinden.
    #[test]
    fn verfallene_und_entfallene_ansagen_stehen_in_bisher() {
        use crate::FlightPhase::*;
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, Cruise));
        // R50: Ton fertig, aber Ruhephase bis zum Verfall → „zu spät“.
        // R80: Ton fehlt bis zum Verfall → „Ton nicht fertig“.
        st.kabinen_lauf = lauf_mit_ton(&[Slot::Reiseflug50]);
        st.kabinen_lauf.warteschlange.aufnehmen([
            Faellig { slot: Slot::Reiseflug50, verfall: cabin_announcements::Verfall::AbPhase(Descent), trotz_ruhephase: false },
            Faellig { slot: Slot::Reiseflug80, verfall: cabin_announcements::Verfall::AbPhase(Descent), trotz_ruhephase: false },
        ]);
        let (g, neu) = abspielen_schritt(&mut st, 100, Cruise, true, false);
        assert!(g.is_none() && !neu, "Ruhephase: wartet noch");
        let (g, neu) = abspielen_schritt(&mut st, 200, Descent, false, false);
        assert!(g.is_none() && neu);
        let gruende: Vec<(Slot, Option<&str>, Option<&str>)> = st.kabinen_lauf.verlauf.iter().map(|g| (g.slot, g.ergebnis.as_deref(), g.grund.as_deref())).collect();
        assert_eq!(gruende, vec![(Slot::Reiseflug50, Some("entfallen"), Some("zu_spaet")), (Slot::Reiseflug80, Some("entfallen"), Some("ton_nicht_fertig"))]);
        // Keine Stimme für keine Sprache: entfällt mit Grund statt nur im Log.
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, Cruise));
        st.kabinen_lauf = lauf_mit_ton(&[]);
        st.kabinen_lauf.texte.insert((Slot::Reiseflug25, "de-DE".into()), "x".into());
        st.kabinen_lauf.ton.insert((Slot::Reiseflug25, "de-DE".into()), None);
        st.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::Reiseflug25, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        let (g, _) = abspielen_schritt(&mut st, 10, Cruise, false, false);
        assert!(g.is_none());
        assert_eq!(st.kabinen_lauf.verlauf[0].grund.as_deref(), Some("keine_stimme"));
        // Sicher entfallend (Bizjet ohne Kurzfassung): aus der Warteschlange raus.
        st.kabinen_lauf.entfaellt.insert(Slot::Sicherheit, "kein_kurztext".into());
        st.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::Sicherheit, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        abspielen_schritt(&mut st, 20, Cruise, false, false);
        assert!(st.kabinen_lauf.warteschlange.ist_leer());
        assert_eq!(st.kabinen_lauf.verlauf.last().and_then(|g| g.grund.as_deref()), Some("kein_kurztext"));
    }

    // ---- Pilotendurchlauf: ganze Flüge durch Planer, Warteschlange und
    // Abspielen, mit Rückmeldungen wie von der Oberfläche (Pflicht vor dem
    // Release, Gedächtnis „Kapazität + Nutzerdurchlauf“). ----

    struct Durchlauf {
        /// (Startzeit relativ zum Boarding in s, Slot, Rolle, Ende)
        gespielt: Vec<(i64, Slot, Rolle, i64)>,
        verlauf: Vec<Gespielt>,
    }

    /// Ein Flug von `min` Minuten Flugzeit, Takt alle 5 s. `einstieg` = Neustart
    /// des Clients mitten im Reiseflug (Planer setzt dort fort).
    fn fliegen(flugart: Flugart, min: u32, reise_ft: f64, einstieg: bool) -> Durchlauf {
        use crate::FlightPhase::*;
        let start = 1_800_000_000i64;
        let mut st = FlightStats::new();
        st.kabinen_lauf.sprachen = vec!["fr-FR".into(), "de-DE".into()];
        for s in Slot::ALLE {
            for sp in ["fr-FR", "de-DE", "en-GB"] {
                st.kabinen_lauf.texte.insert((s, sp.into()), format!("{s:?}"));
                // Echte Längen je Sprache: Sicherheit gut 1,5 min, Kommando
                // ein Satz, sonst eine halbe Minute.
                let dauer_s = match s.rolle() {
                    _ if s == Slot::Sicherheit => 95.0,
                    Rolle::Kommando => 4.0,
                    _ => 30.0,
                };
                st.kabinen_lauf.ton.insert((s, sp.into()), Some(Ton { datei: "f/x.wav".into(), dauer_s, stuecke: vec![], herkunft: "server" }));
            }
        }
        st.kabinen_lauf.sicherheit_kapitaen = flugart == Flugart::Bizjet;
        let flug = |ab: Option<i64>| cabin_announcements::Flugdaten {
            flugart: Some(flugart),
            geplante_flugzeit_min: Some(min),
            tod_nach_abheben_s: None,
            ziel_plan_icao: Some("EDDF".into()),
            reiseflughoehe_ft: Some(reise_ft as u32),
            abheben_epoch: ab,
        };
        st.kabinen_planer = Some(cabin_announcements::Planer::neu(flug(None)));
        // Profil: Boden 18 min, Steigflug 2.000 ft/min, Sinkflug 1.800 ft/min,
        // Anflug 8 min; Reiseflug füllt die Planzeit auf.
        let steig = (reise_ft / 2_000.0 * 60.0) as i64;
        let sink = ((reise_ft - 3_000.0) / 1_800.0 * 60.0) as i64;
        let anflug = 8 * 60;
        let reise = (i64::from(min) * 60 - steig - sink - anflug).max(60);
        let abheben = start + 18 * 60 + 60;
        let landung = abheben + i64::from(min) * 60;
        let mut ticks: Vec<(i64, FlightPhase, f64, f64, Option<u8>)> = Vec::new();
        let mut t = start;
        let mut schieben = |ph: FlightPhase, dauer: i64, h0: f64, h1: f64, zeichen: Option<u8>, ticks: &mut Vec<_>| {
            let n = (dauer / 5).max(1);
            for i in 0..n {
                let h = h0 + (h1 - h0) * (i as f64 / n as f64);
                let vs = (h1 - h0) / (dauer as f64 / 60.0);
                ticks.push((t, ph, h, vs, zeichen));
                t += 5;
            }
        };
        schieben(Boarding, 10 * 60, 0.0, 0.0, Some(2), &mut ticks);
        schieben(Pushback, 3 * 60, 0.0, 0.0, Some(2), &mut ticks);
        schieben(TaxiOut, 5 * 60, 0.0, 0.0, Some(2), &mut ticks);
        schieben(TakeoffRoll, 40, 0.0, 0.0, Some(2), &mut ticks);
        schieben(Takeoff, 20, 0.0, 500.0, Some(2), &mut ticks);
        schieben(Climb, steig, 500.0, reise_ft, Some(1), &mut ticks);
        schieben(Cruise, reise, reise_ft, reise_ft, Some(0), &mut ticks);
        schieben(Descent, sink, reise_ft, 3_000.0, Some(2), &mut ticks);
        schieben(Approach, anflug - 180, 3_000.0, 1_500.0, Some(2), &mut ticks);
        schieben(Final, 180, 1_500.0, 0.0, Some(2), &mut ticks);
        schieben(Landing, 30, 0.0, 0.0, Some(2), &mut ticks);
        schieben(TaxiIn, 5 * 60, 0.0, 0.0, Some(2), &mut ticks);
        schieben(BlocksOn, 60, 0.0, 0.0, Some(0), &mut ticks);
        schieben(Arrived, 3 * 60, 0.0, 0.0, Some(0), &mut ticks);

        let mitte = abheben + steig + reise / 2;
        let zfw = |t: i64| 50_000.0 + 22_000.0 * ((t - start - 120) as f64 / 360.0).clamp(0.0, 1.0);
        let mut gespielt = Vec::new();
        let mut laufend: Option<(u64, i64)> = None;
        for (jetzt, phase, h, vs, zeichen) in ticks {
            if einstieg && jetzt < mitte {
                continue;
            }
            // Neustart: Planer einmal frisch aus Phase und echtem Abheben.
            if einstieg && st.kabinen_lauf.angefragt.is_empty() {
                st.kabinen_planer = Some(cabin_announcements::Planer::fortsetzen(flug(Some(abheben)), phase));
                st.kabinen_lauf.angefragt.insert("neustart".into());
            }
            if let Some((id, ende)) = laufend {
                if jetzt >= ende {
                    ende_melden(&mut st, id, "gespielt", jetzt, "backend");
                    laufend = None;
                }
            }
            let tk = cabin_announcements::Tick {
                jetzt,
                phase,
                hoehe_ft: h,
                vs_fpm: vs,
                am_boden: matches!(phase, Boarding | Pushback | TaxiOut | TakeoffRoll | TaxiIn | BlocksOn | Arrived),
                anschnallzeichen: zeichen,
                rest_min: Some((landung - jetzt) as f64 / 60.0).filter(|m| *m >= 0.0),
                ziel_icao: None,
                // Einsteigen von Minute 2 bis 8 (50 → 72 t).
                zfw_kg: Some(zfw(jetzt)),
                triebwerke_an: Some(!matches!(phase, Boarding | Pushback | BlocksOn | Arrived)),
                ..Default::default()
            };
            let planer = st.kabinen_planer.as_mut().expect("planer");
            let faellig = planer.tick(&tk);
            let ruhe = planer.ruhephase(&tk);
            for (slot, _) in planer.entfallene_abholen() {
                verbuchen(&mut st.kabinen_lauf, slot, "entfallen", Some("planer"), jetzt);
            }
            st.phase = phase;
            st.kabinen_lauf.warteschlange.aufnehmen(faellig);
            let tief = !tk.am_boden && h < 10_000.0 && cabin_announcements::planer::rang(phase) < cabin_announcements::planer::rang(Cruise);
            let (g, _) = abspielen_schritt(&mut st, jetzt, phase, ruhe, tief);
            if let Some(g) = g {
                // Wie die Oberfläche: Start melden, nach der Dauer „gespielt“.
                if let Some(e) = st.kabinen_lauf.verlauf.iter_mut().find(|x| x.id == g.id) {
                    e.gestartet = true;
                }
                let ende = jetzt + dauer_s(g.slot, &g.teile).ceil() as i64;
                gespielt.push((jetzt - start, g.slot, g.rolle, ende - start));
                laufend = Some((g.id, ende));
            }
        }
        Durchlauf { gespielt, verlauf: st.kabinen_lauf.verlauf }
    }

    fn zeit(s: i64) -> String {
        format!("{:>3}:{:02}", s / 60, s % 60)
    }

    fn protokoll(name: &str, d: &Durchlauf) {
        println!("\n=== {name} ===");
        for (t, slot, rolle, ende) in &d.gespielt {
            println!("  {} – {}  {slot:?} ({rolle:?})", zeit(*t), zeit(*ende));
        }
        for g in d.verlauf.iter().filter(|g| g.ergebnis.as_deref() != Some("gespielt")) {
            println!("  nicht gespielt: {:?} {:?} {:?}", g.slot, g.ergebnis, g.grund);
        }
    }

    /// Reguläre Ansagen halten 90 s Pause zueinander, die Kabine kommt
    /// frühestens zwei Minuten nach dem Ende der Kapitänsansage im Sinkflug.
    fn abstaende_pruefen(name: &str, d: &Durchlauf) {
        let regulaer: Vec<&(i64, Slot, Rolle, i64)> = d.gespielt.iter().filter(|(_, s, _, _)| s.wartet_pause()).collect();
        for paar in regulaer.windows(2) {
            // Ausnahmen: Sicherheit folgt der Begrüßung nach 10 s (Thomas 18.10.2026) …
            let mindestens = if (paar[0].1, paar[1].1) == (Slot::BegruessungTuerenZu, Slot::Sicherheit) {
                cabin_announcements::warteschlange::PAUSE_BEGRUESSUNG_SICHERHEIT_S
            } else if (paar[0].1, paar[1].1) == (Slot::Sicherheit, Slot::KapitaenBegruessung) {
                // Und der Kapitän der Sicherheit nach 30 s (v2.0.28, QAF446).
                cabin_announcements::warteschlange::PAUSE_SICHERHEIT_KAPITAEN_S
            } else {
                cabin_announcements::warteschlange::PAUSE_NACH_ANSAGE_S
            };
            assert!(paar[1].0 - paar[0].3 >= mindestens, "{name}: {:?} zu dicht nach {:?}", paar[1].1, paar[0].1);
        }
        // Sicherheit kommt höchstens ~15 s nach der Begrüßung (Takt 5 s).
        if let (Some(b), Some(si)) = (d.gespielt.iter().find(|x| x.1 == Slot::BegruessungTuerenZu), d.gespielt.iter().find(|x| x.1 == Slot::Sicherheit)) {
            assert!(si.0 - b.3 <= 15, "{name}: Sicherheit {} s nach der Begrüßung", si.0 - b.3);
        }
        let ende_kapitaen = d.gespielt.iter().find(|x| x.1 == Slot::KapitaenSinkflug).map(|x| x.3);
        let start_kabine = d.gespielt.iter().find(|x| x.1 == Slot::KabineSinkflug).map(|x| x.0);
        if let (Some(k), Some(c)) = (ende_kapitaen, start_kabine) {
            assert!(c - k >= cabin_announcements::planer::KABINE_NACH_KAPITAEN_MIN_S, "{name}: Kabine {}s nach dem Kapitän", c - k);
        }
        // Nichts überlappt.
        for paar in d.gespielt.windows(2) {
            assert!(paar[1].0 >= paar[0].3, "{name}: {:?} überlappt {:?}", paar[1].1, paar[0].1);
        }
    }

    #[test]
    fn pilotendurchlauf_ganzer_flug() {
        let slots = |d: &Durchlauf| d.gespielt.iter().map(|x| x.1).collect::<Vec<_>>();

        // Linie, 2 h: der volle Ablauf in Reihenfolge, nichts entfällt.
        let lang = fliegen(Flugart::Linie, 120, 35_000.0, false);
        protokoll("Linie 2 h (A320)", &lang);
        abstaende_pruefen("Linie", &lang);
        let erwartet = [
            Slot::Boarding, Slot::BoardingAbgeschlossen, Slot::KommandoTuerenScharf, Slot::BegruessungTuerenZu, Slot::Sicherheit, Slot::KapitaenBegruessung,
            Slot::KommandoStart, Slot::Chime10000Steigflug, Slot::AnschnallzeichenAus, Slot::Reiseflug25, Slot::Reiseflug50,
            Slot::Reiseflug80, Slot::KapitaenSinkflug, Slot::KommandoLandungVorbereiten, Slot::KabineSinkflug,
            Slot::Chime10000Sinkflug, Slot::KommandoPlaetzeLandung, Slot::NachDerLandung, Slot::KommandoTuerenEntschaerfen, Slot::Aussteigen,
        ];
        let mut ist = slots(&lang);
        ist.retain(|s| erwartet.contains(s));
        let mut soll = erwartet.to_vec();
        soll.sort();
        let mut sortiert = ist.clone();
        sortiert.sort();
        assert_eq!(sortiert, soll, "jede Ansage genau einmal: {ist:?}");
        assert!(lang.verlauf.iter().all(|g| g.ergebnis.as_deref() == Some("gespielt")), "nichts entfällt");
        let pos = |d: &Durchlauf, s: Slot| d.gespielt.iter().position(|x| x.1 == s).expect("gespielt");
        assert!(pos(&lang, Slot::Boarding) < pos(&lang, Slot::BoardingAbgeschlossen));
        assert!(pos(&lang, Slot::BoardingAbgeschlossen) < pos(&lang, Slot::BegruessungTuerenZu));
        assert!(pos(&lang, Slot::BegruessungTuerenZu) < pos(&lang, Slot::Sicherheit) && pos(&lang, Slot::Sicherheit) < pos(&lang, Slot::KapitaenBegruessung));
        assert!(pos(&lang, Slot::Reiseflug80) < pos(&lang, Slot::KapitaenSinkflug) && pos(&lang, Slot::KapitaenSinkflug) < pos(&lang, Slot::KabineSinkflug));
        assert!(pos(&lang, Slot::NachDerLandung) < pos(&lang, Slot::Aussteigen));

        // Kurzstrecke, 50 min: Service ja, Zwischenstände nein.
        let kurz = fliegen(Flugart::Linie, 50, 24_000.0, false);
        protokoll("Kurzstrecke 50 min", &kurz);
        abstaende_pruefen("Kurzstrecke", &kurz);
        let k = slots(&kurz);
        assert!(k.contains(&Slot::Reiseflug25) && !k.contains(&Slot::Reiseflug50) && !k.contains(&Slot::Reiseflug80), "{k:?}");
        assert!(pos(&kurz, Slot::AnschnallzeichenAus) < pos(&kurz, Slot::Reiseflug25) && pos(&kurz, Slot::Reiseflug25) < pos(&kurz, Slot::KapitaenSinkflug));
        assert!(kurz.verlauf.iter().all(|g| g.ergebnis.as_deref() == Some("gespielt")), "nichts entfällt");

        // Bizjet, 90 min: Kapitän mit Sicherheits-Kurzfassung und Gong, keine Kabine.
        let biz = fliegen(Flugart::Bizjet, 90, 41_000.0, false);
        protokoll("Bizjet 90 min (C25A)", &biz);
        abstaende_pruefen("Bizjet", &biz);
        let b = slots(&biz);
        assert_eq!(b, vec![Slot::Sicherheit, Slot::KapitaenBegruessung, Slot::Chime10000Steigflug, Slot::KapitaenSinkflug, Slot::Chime10000Sinkflug]);
        assert!(biz.gespielt.iter().filter(|x| x.1 != Slot::Chime10000Steigflug && x.1 != Slot::Chime10000Sinkflug).all(|x| x.2 == Rolle::Kapitaen), "nur der Kapitän spricht");

        // Einstieg mitten im Reiseflug (Neustart des Clients): nichts vom Boden,
        // keine zweite Begrüßung, der Rest des Fluges läuft normal.
        let mitte = fliegen(Flugart::Linie, 120, 35_000.0, true);
        protokoll("Linie 2 h, Neustart mitten im Reiseflug", &mitte);
        abstaende_pruefen("Neustart", &mitte);
        let m = slots(&mitte);
        for vorbei in [Slot::Boarding, Slot::Sicherheit, Slot::KapitaenBegruessung, Slot::AnschnallzeichenAus, Slot::Chime10000Steigflug] {
            assert!(!m.contains(&vorbei), "{vorbei:?} nach dem Neustart");
        }
        for kommt in [Slot::KapitaenSinkflug, Slot::KabineSinkflug, Slot::NachDerLandung, Slot::Aussteigen] {
            assert!(m.contains(&kommt), "{kommt:?} fehlt nach dem Neustart");
        }
        assert!(mitte.verlauf.iter().all(|g| g.ergebnis.as_deref() == Some("gespielt")), "nach dem Neustart kein falsches „entfallen“");
        // Bizjet: Die Kurzfassung spricht der Kapitän.
        assert_eq!(biz.gespielt[0].2, Rolle::Kapitaen);
    }

    /// Thomas 18.10.2026 (Punkt 8/16): Die Stimme folgt dem Geschlecht der
    /// Crew laut Server; ohne Angabe Kapitän männlich, Kabine weiblich.
    fn musik_stats(flight_type: &str, pax: Option<i32>) -> FlightStats {
        let mut st = FlightStats::new();
        st.phase = crate::FlightPhase::Boarding;
        st.flight_type = Some(flight_type.into());
        st.pax_count = pax;
        st.kabinen_lauf.beta = Beta::Ja;
        st.kabinen_planer = Some(cabin_announcements::Planer::fortsetzen(
            cabin_announcements::Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(45), ..Default::default() },
            crate::FlightPhase::Boarding,
        ));
        st
    }

    fn gate_snap(beacon: bool) -> SimSnapshot {
        SimSnapshot { on_ground: true, light_beacon: Some(beacon), pushback_state: Some(3), ..SimSnapshot::default() }
    }

    /// Spielt Peters Boarding durch Planer und Musik: Strom → Anschnall-
    /// zeichen → nach 8 min Zuladung → Begrüßung → Musik (Lied kommt später)
    /// → Beacon: Musik blendet aus.
    #[test]
    fn peter_musik_mit_der_begruessung_lied_nachgereicht_ende_beim_beacon() {
        let mut st = musik_stats("J", Some(150));
        let lied = api_client::KabinenLied { text: "[Verse]\nFly with Lufthansa".into(), stil: "elegant piano".into(), titel: None, strecke: None };
        st.kabinen_lauf.lied.vorlage = Some(lied.clone());
        let mut tun = Vec::new();
        let mut holen = 0;
        for s in (0..=12 * 60).step_by(10) {
            let zfw = if s > 8 * 60 { 46_000.0 } else { 43_000.0 };
            let tk = cabin_announcements::Tick { jetzt: s, phase: crate::FlightPhase::Boarding, am_boden: true, anschnallzeichen: Some(if s >= 60 { 2 } else { 0 }), beacon: Some(false), zfw_kg: Some(zfw), ..Default::default() };
            st.kabinen_planer.as_mut().unwrap().tick(&tk);
            let (t, h) = musik_schritt(&mut st, &gate_snap(false), "A320", None, s);
            holen += usize::from(h.is_some());
            // Der Ton-Server liefert das Lied 3 min nach der ersten Anfrage.
            if h.is_some() {
                st.kabinen_lauf.lied.laeuft = false;
                st.kabinen_lauf.lied.naechster = s + 60;
                if s >= 180 {
                    st.kabinen_lauf.lied.datei = Some(PathBuf::from("/tmp/boarding-lied.mp3"));
                }
            }
            tun.push((s, t));
        }
        assert_eq!(st.kabinen_lauf.musik_quelle, Some(MusikQuelle::Lied));
        assert!(holen >= 2, "nachgereicht: mehrere Versuche bis es kommt");
        let erstes = tun.iter().find(|(_, t)| matches!(t, MusikTun::Spielen(_))).map(|(s, _)| *s).expect("Musik lief");
        assert_eq!(erstes, 8 * 60 + 10 + 30, "mit der Begrüßung, nicht beim Anschnallzeichen");
        // Beacon: einmal ausblenden, danach nichts mehr.
        let (t, h) = musik_schritt(&mut st, &gate_snap(true), "A320", None, 12 * 60 + 10);
        assert_eq!((t, h), (MusikTun::Ausblenden, None));
        let (t, _) = musik_schritt(&mut st, &gate_snap(true), "A320", None, 12 * 60 + 20);
        assert_eq!(t, MusikTun::Nichts);
    }

    /// Zusatz I: nach der Ansage „nach der Landung“ läuft die Musik des
    /// Fluges wieder, bis 15 min nach „Aussteigen“.
    #[test]
    fn musik_nach_der_landung_bis_nach_dem_aussteigen() {
        let mut st = musik_stats("J", Some(150));
        let eigene = Some(("DLH".to_string(), vec![PathBuf::from("/m/DLH/a.mp3")]));
        st.phase = crate::FlightPhase::TaxiOut;
        musik_schritt(&mut st, &gate_snap(true), "A320", eigene, 0);
        st.kabinen_planer = Some(cabin_announcements::Planer::fortsetzen(
            cabin_announcements::Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(45), ..Default::default() },
            crate::FlightPhase::TaxiIn,
        ));
        st.phase = crate::FlightPhase::TaxiIn;
        let mut rollen = gate_snap(true);
        rollen.engines_running = 2;
        // Die Ansage „nach der Landung“ ist fällig und wartet: noch still.
        st.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::NachDerLandung, verfall: cabin_announcements::Verfall::AbPhase(crate::FlightPhase::BlocksOn), trotz_ruhephase: false }]);
        st.kabinen_planer.as_mut().unwrap().schon_gespielt([Slot::NachDerLandung]);
        assert_eq!(musik_schritt(&mut st, &rollen, "A320", None, 100).0, MusikTun::Nichts);
        // Gespielt: Musik an.
        st.kabinen_lauf.warteschlange.entfernen(Slot::NachDerLandung);
        verbuchen(&mut st.kabinen_lauf, Slot::NachDerLandung, "gespielt", None, 110);
        assert!(matches!(musik_schritt(&mut st, &rollen, "A320", None, 140).0, MusikTun::Spielen(_)));
        assert_eq!(musik_ansicht(&Einstellungen::default(), Some(&st.kabinen_lauf), None, true, true, false).abschnitt, Some("ankunft"));
        // Aussteigen gespielt, 15 min später aus.
        st.phase = crate::FlightPhase::Arrived;
        verbuchen(&mut st.kabinen_lauf, Slot::Aussteigen, "gespielt", None, 600);
        assert!(matches!(musik_schritt(&mut st, &gate_snap(true), "A320", None, 600 + 14 * 60).0, MusikTun::Spielen(_)));
        assert_eq!(musik_schritt(&mut st, &gate_snap(true), "A320", None, 600 + 15 * 60).0, MusikTun::Ausblenden);
    }

    /// Kein Lied da (Ausfall): still, die Ansagen laufen ungestört (Nr. 9).
    #[test]
    fn ohne_lied_bleibt_es_still() {
        let mut st = musik_stats("J", Some(150));
        st.kabinen_planer.as_mut().unwrap().schon_gespielt([Slot::Boarding]);
        let (t, h) = musik_schritt(&mut st, &gate_snap(false), "A320", None, 0);
        assert_eq!((t, h), (MusikTun::Nichts, None), "noch keine Vorlage, nichts zu holen");
    }

    /// Eigene Musik geht vor (kein Lied), Fracht und Bizjet nur eigene Musik
    /// ab Beginn des Boardings (Nr. 11, abgeleitet).
    #[test]
    fn eigene_musik_vor_lied_fracht_nur_eigene() {
        let eigene = Some(("DLH".to_string(), vec![PathBuf::from("/m/DLH/a.mp3")]));
        let mut st = musik_stats("J", Some(150));
        musik_schritt(&mut st, &gate_snap(false), "A320", eigene.clone(), 0);
        assert!(matches!(st.kabinen_lauf.musik_quelle, Some(MusikQuelle::Eigene { .. })));
        assert!(!lied_anfragen(&st.kabinen_lauf, true, crate::FlightPhase::Boarding), "eigene Musik: kein Lied anfragen");
        let mut l = musik_stats("J", Some(150));
        musik_schritt(&mut l, &gate_snap(false), "A320", None, 0);
        assert!(!lied_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::Boarding), "erst den Ordner nach schon geladener Musik durchsuchen (Neustart)");
        l.kabinen_lauf.lied.platte_geprueft = true;
        assert!(lied_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::Boarding), "Linie ohne eigene Musik: Lied");
        assert!(!lied_anfragen(&l.kabinen_lauf, false, crate::FlightPhase::Boarding), "Schalter aus: kein Lied (Nr. 15)");
        // Thomas 19.10.2026: Update mitten im Flug braucht das Lied fuer die
        // Musik nach der Landung (vorher fest „kein Lied mehr“, das war falsch).
        assert!(lied_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::Cruise), "Update im Reiseflug: Lied noch anfragen");
        assert!(lied_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::TaxiIn), "auch nach der Landung, solange die Musik kommt");
        // „Boarding-Musik laden“ setzt einen Fehlschlag zurück, die Anfrage geht wieder raus.
        l.kabinen_lauf.lied.angefragt = true;
        l.kabinen_lauf.lied.fehler = Some("x".into());
        assert!(musik_neu_laden(&mut l.kabinen_lauf).is_ok());
        assert!(l.kabinen_lauf.lied.fehler.is_none() && !l.kabinen_lauf.lied.angefragt);
        // Eigene Anfrage „lied“ nach dem Update: nur ohne Abflug-Drehbuch im Takt, nur einmal.
        assert!(lied_einzeln_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::Cruise, false, 0), "Update im Flug: eigene Lied-Anfrage");
        assert!(!lied_einzeln_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::Cruise, true, 0), "Abflug-Drehbuch nimmt das Lied mit");
        assert!(!lied_einzeln_anfragen(&l.kabinen_lauf, false, crate::FlightPhase::Cruise, false, 0), "Schalter aus: nichts");
        l.kabinen_lauf.lied.angefragt = true;
        assert!(!lied_einzeln_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::Cruise, false, 0), "schon angefragt: nicht doppelt");
        l.kabinen_lauf.lied.angefragt = false;
        // Musik nach der Landung vorbei: 15 min nach „Aussteigen“ (echter Ablauf ueber tick).
        let ende = cabin_announcements::musik::MusikTakt {
            phase: crate::FlightPhase::TaxiIn, am_boden: true, nach_landung_vorbei: true,
            aussteigen_um: Some(0), jetzt: cabin_announcements::musik::ANKUNFT_NACH_AUSSTEIGEN_S + 1, ..Default::default()
        };
        assert_eq!(l.kabinen_lauf.musik_fenster.tick(cabin_announcements::flugart::Flugart::Linie, &ende), None);
        assert!(l.kabinen_lauf.musik_fenster.ankunft_vorbei());
        assert!(!lied_einzeln_anfragen(&l.kabinen_lauf, true, crate::FlightPhase::BlocksOn, false, 0), "Musik nach der Landung vorbei: kein Lied mehr");
        // Fracht ohne Passagierzahl: steht fest, eigene Musik sofort im Boarding.
        let mut f = musik_stats("F", None);
        assert!(matches!(musik_schritt(&mut f, &gate_snap(false), "B77F", eigene.clone(), 0).0, MusikTun::Spielen(_)));
        let mut ohne = musik_stats("F", None);
        assert_eq!(musik_schritt(&mut ohne, &gate_snap(false), "B77F", None, 0).0, MusikTun::Nichts);
        assert_eq!(ohne.kabinen_lauf.musik_quelle, Some(MusikQuelle::Keine), "Fracht ohne eigene Musik: kein Lied");
        let mut biz = musik_stats("J", Some(6));
        musik_schritt(&mut biz, &gate_snap(false), "C25A", None, 0);
        assert_eq!(biz.kabinen_lauf.musik_quelle, Some(MusikQuelle::Keine), "Bizjet: kein Lied");
        // Linie ohne Passagierzahl: noch nichts entscheiden.
        let mut offen = musik_stats("J", None);
        musik_schritt(&mut offen, &gate_snap(false), "A320", eigene, 0);
        assert_eq!(offen.kabinen_lauf.musik_quelle, None);
        // Ohne Freischaltung: aus.
        let mut nein = musik_stats("J", Some(150));
        nein.kabinen_lauf.beta = Beta::Nein;
        assert_eq!(musik_schritt(&mut nein, &gate_snap(false), "A320", None, 0).0, MusikTun::Aus);
    }

    /// 2.1.1 (Codex): Kommt der Ton der Begrüßung sicher nie, hält sie
    /// „Türen zu“ nicht bis zum Startlauf auf; fehlt er nur noch, wartet alles.
    #[test]
    fn begruessung_ohne_ton_haelt_tueren_zu_nur_solange_er_noch_kommt() {
        let mut st = FlightStats::new();
        st.kabinen_lauf.sprachen = vec!["de-DE".into()];
        for s in [Slot::BegruessungTuerenZu, Slot::Sicherheit] {
            st.kabinen_lauf.texte.insert((s, "de-DE".into()), "x".into());
            st.kabinen_lauf.ton.insert((s, "de-DE".into()), Some(Ton { datei: "f/x.wav".into(), dauer_s: 3.0, stuecke: vec![], herkunft: "server" }));
        }
        st.kabinen_lauf.texte.insert((Slot::Boarding, "de-DE".into()), "Willkommen".into());
        let f = |slot| Faellig { slot, verfall: cabin_announcements::Verfall::AbPhase(crate::FlightPhase::TakeoffRoll), trotz_ruhephase: false };
        st.kabinen_lauf.warteschlange.aufnehmen([f(Slot::Boarding), f(Slot::BegruessungTuerenZu), f(Slot::Sicherheit)]);
        let (g, _) = abspielen_schritt(&mut st, 0, crate::FlightPhase::Boarding, false, false);
        assert!(g.is_none(), "Ton der Begrüßung kommt noch: alles wartet");
        st.kabinen_lauf.ton_endgueltig.insert((Slot::Boarding, "de-DE".into()), "400".into());
        let (g, _) = abspielen_schritt(&mut st, 1, crate::FlightPhase::Boarding, false, false);
        assert_eq!(g.map(|g| g.slot), Some(Slot::BegruessungTuerenZu));
        assert!(st.kabinen_lauf.verlauf.iter().any(|g| g.slot == Slot::Boarding && g.grund.as_deref() == Some("keine_stimme")));
        assert!(!ton_kommt_nie(&Lauf::default(), Slot::Boarding), "ohne Sprachen: kommt noch");
    }

    /// Advisor 2.1.1: Hatte das Modul keine Zeit mehr, einmal nachfragen,
    /// nicht gleich „kein Lied“; die Nachfrage selbst fragt nicht weiter.
    #[test]
    fn lied_spaeter_wird_einmal_nachgefragt() {
        let mut l = LiedStand::default();
        lied_nach_antwort(&mut l, true, false, true, 100);
        assert_eq!((l.nochmal_ab, l.fehler.is_none()), (Some(160), true));
        // Späte Antwort einer anderen Anfrage, Nachfrage steht noch aus: nichts Neues.
        lied_nach_antwort(&mut l, true, false, true, 110);
        assert_eq!((l.nochmal_ab, l.fehler.is_none()), (Some(160), true));
        // Die Nachfrage selbst (nochmal_ab beim Senden geleert) kommt wieder ohne Lied.
        l.nochmal_ab = None;
        lied_nach_antwort(&mut l, true, false, true, 200);
        assert!(l.fehler.is_some() && l.nochmal_ab.is_none(), "nur eine Nachfrage, dann still");
        let mut g = LiedStand::default();
        lied_nach_antwort(&mut g, true, true, false, 0);
        assert!(g.fehler.is_none() && g.nochmal_ab.is_none());
        let mut o = LiedStand::default();
        lied_nach_antwort(&mut o, false, false, false, 0);
        assert!(o.fehler.is_none(), "nicht angefragt (eigene Musik): kein Fehler");
    }

    /// Zusatz H: Reiseflug 50 gehört zum Teil „halbzeit“, nicht mehr zum Abflug.
    #[test]
    fn halbzeit_ist_ein_eigener_teil() {
        assert_eq!(teil_von(Slot::Reiseflug50), Some("halbzeit"));
        assert_eq!(ton_teil(Slot::Reiseflug50), Some("halbzeit"));
        assert!(!ABFLUG.contains(&Slot::Reiseflug50) && !ANKUNFT.contains(&Slot::Reiseflug50));
        assert_eq!(teil_von(Slot::Reiseflug25), Some("abflug"));
        assert_eq!(teil_stand(&Lauf::default(), "halbzeit", &HALBZEIT, Some(10_000), false), TeilStand::Offen { um: Some(10_000 - 900) });
    }

    /// 2.1.6: Ereignisse sagen dem Server, dass sie sofort fällig sind
    /// (SWR 255, 20.10.2026: die Gate-Verspätung zählte als „zu spät“).
    #[test]
    fn ereignis_ton_traegt_seinen_teil() {
        assert_eq!(ton_teil(Slot::VerspaetungGate), Some("ereignis"));
        assert_eq!(ton_teil(Slot::Turbulenz), Some("ereignis"));
        assert_eq!(ton_teil(Slot::Boarding), Some("abflug"));
        assert_eq!(ton_teil(Slot::KommandoStart), Some("abflug"));
        let k = piper::koerper("auto:de-DE:w", "Hallo", Some(0), ton_teil(Slot::VerspaetungGate), Some("verspaetung_gate".into()), piper::Wahl::default());
        assert_eq!(k["teil"], "ereignis");
    }

    /// Zusatz H: angefragt im Reiseflug, 15 min vor dem Zeitpunkt; nie auf
    /// der Kurzstrecke, nie im Steigflug.
    #[test]
    fn halbzeit_anfrage_zeitpunkt_und_kurzstrecke() {
        use crate::FlightPhase::*;
        assert!(!halbzeit_anfragen(true, Cruise, Some(10_000), 10_000 - 16 * 60));
        assert!(halbzeit_anfragen(true, Cruise, Some(10_000), 10_000 - 15 * 60));
        assert!(halbzeit_anfragen(true, Holding, None, 0), "ohne Zeitpunkt sofort im Reiseflug");
        // 1e (Rundumschlag 22.10.2026): auch schon im Steigflug, wenn der Zeitpunkt nah ist.
        assert!(halbzeit_anfragen(true, Climb, Some(10_000), 9_999));
        assert!(!halbzeit_anfragen(true, Climb, Some(10_000), 10_000 - 16 * 60));
        assert!(!halbzeit_anfragen(false, Cruise, Some(10_000), 9_999), "kommt nicht (Kurzstrecke)");
        let kurz = cabin_announcements::Planer::neu(cabin_announcements::Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(50), ..Default::default() });
        assert!(!kurz.kommt_noch(Slot::Reiseflug50, Cruise), "Kurzstrecke hat keine Halbzeit");
    }

    /// Zusatz H, Nachspiel AFR218 (510 min): Die Anfrage kommt im Reiseflug,
    /// 15 min bevor der Planer die Halbzeit meldet.
    #[test]
    fn halbzeit_anfrage_im_echten_flug_afr218() {
        #[derive(serde::Deserialize)]
        struct Daten {
            ticks: Vec<(i64, crate::FlightPhase, f64, f64, f64, u8, Option<u8>, u8, u8, f64, u8, u8, f64, Option<u8>)>,
            landung: i64,
        }
        let d: Daten = serde_json::from_str(include_str!("../crates/cabin-announcements/tests/daten/afr218.json")).unwrap();
        let erste = d.ticks[0].1;
        let mut p = cabin_announcements::Planer::fortsetzen(
            cabin_announcements::Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(510), reiseflughoehe_ft: Some(38_000), ..Default::default() },
            erste,
        );
        let (mut angefragt, mut gemeldet) = (None, None);
        for &(t, phase, alt, _agl, vs, boden, zeichen, ..) in &d.ticks {
            let tk = cabin_announcements::Tick { jetzt: t, phase, hoehe_ft: alt, vs_fpm: vs, am_boden: boden == 1, anschnallzeichen: zeichen, rest_min: Some((d.landung - t) as f64 / 60.0), ..Default::default() };
            let um = p.zeitpunkt(Slot::Reiseflug50, Some(d.landung));
            if angefragt.is_none() && halbzeit_anfragen(p.kommt_noch(Slot::Reiseflug50, phase), phase, um, t) {
                angefragt = Some((t, phase));
            }
            if p.tick(&tk).iter().any(|f| f.slot == Slot::Reiseflug50) {
                gemeldet = Some(t);
            }
        }
        let (a, phase) = angefragt.expect("angefragt");
        let g = gemeldet.expect("Halbzeit gemeldet");
        assert_eq!(phase, crate::FlightPhase::Cruise);
        assert!((g - a - 15 * 60).abs() <= 120, "Vorlauf {} s", g - a);
    }

    #[test]
    fn musik_ansicht_nennt_den_zustand() {
        let e = Einstellungen::default();
        let mut l = Lauf::default();
        assert_eq!(musik_ansicht(&e, None, None, false, false, false).stand, "ohne_flug");
        l.musik_quelle = Some(MusikQuelle::Lied);
        assert_eq!(musik_ansicht(&e, Some(&l), None, false, false, false).stand, "kein_lied", "nichts angefragt (Neustart nach Türen zu)");
        l.lied.angefragt = true;
        assert_eq!(musik_ansicht(&e, Some(&l), None, false, false, false).stand, "wird_erzeugt");
        l.lied.fehler = Some("x".into());
        assert_eq!(musik_ansicht(&e, Some(&l), None, false, false, false).stand, "kein_lied");
        l.lied.datei = Some(PathBuf::from("a.mp3"));
        assert_eq!(musik_ansicht(&e, Some(&l), None, false, false, false).stand, "bereit");
        assert_eq!(musik_ansicht(&e, Some(&l), None, true, true, false).stand, "laeuft");
        assert_eq!(musik_ansicht(&e, Some(&l), None, true, false, false).stand, "pausiert");
        let aus = Einstellungen { boarding_musik: false, ..Einstellungen::default() };
        assert_eq!(musik_ansicht(&aus, Some(&l), None, false, false, false).stand, "aus");
    }

    /// v2.0.28: „Boarding completed“ ist ein fester englischer Text (wie ein
    /// Kommando), gesprochen von der Kabine.
    #[test]
    fn boarding_completed_spricht_die_kabine() {
        let l = Lauf::default();
        assert_eq!(rolle_von(&l, Slot::BoardingAbgeschlossen), Rolle::Kabine);
        assert!(!stimme_maennlich(&l, rolle_von(&l, Slot::BoardingAbgeschlossen)), "Kabine ohne Angabe: Frauenstimme");
        assert_eq!(Slot::BoardingAbgeschlossen.kommando_text(), Some("Boarding completed."));
        assert!(KOMMANDOS.contains(&Slot::BoardingAbgeschlossen), "Ton wird wie bei Kommandos vorab erzeugt");
    }

    /// Codex 2.0.28: Kommt `zweiter_service` erst, als Reiseflug 80 schon
    /// nach der 12-min-Regel wartet, und bleiben unter 60 min — entfällt.
    #[test]
    fn spaet_erkannter_zweiter_service_entfaellt() {
        let mut l = Lauf::default();
        l.warteschlange.aufnehmen([Faellig { slot: Slot::Reiseflug80, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        assert!(!zweiter_service_zu_spaet(&l, Some(30.0)), "Art unbekannt: 12-min-Regel gilt");
        l.art_r80 = Some("ende_bordverkauf".into());
        assert!(!zweiter_service_zu_spaet(&l, Some(30.0)), "Bordverkauf darf kurz vor dem Sinkflug");
        l.art_r80 = Some("zweiter_service".into());
        assert!(zweiter_service_zu_spaet(&l, Some(30.0)));
        assert!(!zweiter_service_zu_spaet(&l, Some(95.0)), "noch genug Zeit");
    }

    /// Codex-Review PR #160: Nach einem Neustart gilt jeder abgeschlossene
    /// Ausgang als erledigt, nicht nur „gespielt“ (stumm kommt nicht nach).
    #[test]
    fn neustart_uebernimmt_jeden_abgeschlossenen_ausgang() {
        let mut l = Lauf::default();
        verbuchen(&mut l, Slot::EndeService, "stumm", None, 1);
        verbuchen(&mut l, Slot::Reiseflug80, "entfallen", Some("zu_wenig_zeit"), 2);
        let e = erledigte_slots(&l);
        assert!(e.contains(&Slot::EndeService) && e.contains(&Slot::Reiseflug80), "{e:?}");
    }

    /// Codex 2.1.0: Kommt die alte Art „Ende des Bordverkaufs“ erst, als
    /// „Ende des Service“ schon wartet, entfällt Letztere (keine Doppelung).
    #[test]
    fn spaete_alte_art_nimmt_ende_service_heraus() {
        let mut l = Lauf::default();
        l.warteschlange.aufnehmen([Faellig { slot: Slot::EndeService, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        // Neues Modul (zweiter Service): bleibt.
        l.art_r80 = Some("zweiter_service".into());
        assert!(!ende_service_bei_alter_art(&mut l, 100));
        assert!(l.warteschlange.wartend().contains(&Slot::EndeService));
        // Altes Modul (Ende des Bordverkaufs): entfällt mit Grund in „Bisher“.
        l.art_r80 = Some("ende_bordverkauf".into());
        assert!(ende_service_bei_alter_art(&mut l, 100));
        assert!(!l.warteschlange.wartend().contains(&Slot::EndeService));
        let g = l.verlauf.last().expect("verbucht");
        assert_eq!((g.slot, g.ergebnis.as_deref(), g.grund.as_deref()), (Slot::EndeService, Some("entfallen"), Some("uebersprungen")));
        assert!(!ende_service_bei_alter_art(&mut l, 101), "nur einmal");
    }

    /// 2.1.0: `fremd` aus dem Drehbuch wird je Text gemerkt und mit einem
    /// neuen Text ohne Fremdwörter wieder vergessen.
    #[test]
    fn fremdwoerter_werden_je_text_gemerkt() {
        let mut l = Lauf::default();
        let fremd = vec![api_client::KabinenFremd { wort: "Times Square".into(), sprache: "en".into() }];
        let eintrag = |text: &str, fremd: Vec<api_client::KabinenFremd>| api_client::KabinenEintrag { slot: "reiseflug25".into(), sprache: "de-DE".into(), text: text.into(), rolle: None, art: None, fremd };
        drehbuch_uebernehmen(&mut l, api_client::KabinenDrehbuch { sprachen: vec!["de-DE".into()], eintraege: vec![eintrag("Am Times Square", fremd.clone())], crew: None, boarding_lied: None, boarding_lied_spaeter: false, entfaellt: Default::default() }, &[Slot::Reiseflug25], false, false);
        assert_eq!(l.fremd.get(&(Slot::Reiseflug25, "de-DE".into())), Some(&fremd));
        drehbuch_uebernehmen(&mut l, api_client::KabinenDrehbuch { sprachen: vec!["de-DE".into()], eintraege: vec![eintrag("Am Hafen", Vec::new())], crew: None, boarding_lied: None, boarding_lied_spaeter: false, entfaellt: Default::default() }, &[Slot::Reiseflug25], false, false);
        assert!(l.fremd.get(&(Slot::Reiseflug25, "de-DE".into())).is_none());
    }

    /// v2.0.28 (Punkt 9): Der Wartegrund geht nur beim Wechsel ins Log.
    #[test]
    fn wartegrund_nur_beim_wechsel_im_protokoll() {
        let mut l = Lauf::default();
        l.warteschlange.aufnehmen([Faellig { slot: Slot::Reiseflug25, verfall: cabin_announcements::Verfall::Um(1_000), trotz_ruhephase: false }]);
        l.ruhephase = true;
        wartegruende_notieren(&mut l, 0);
        wartegruende_notieren(&mut l, 5);
        l.ruhephase = false;
        wartegruende_notieren(&mut l, 10);
        let arten: Vec<_> = l.protokoll.iter().map(|n| (n.art, n.details["grund"].clone())).collect();
        assert_eq!(arten, vec![("wartet", serde_json::json!("ruhephase")), ("wartet", serde_json::json!("ton"))]);
    }

    #[test]
    fn stimme_folgt_der_crew() {
        let mut l = Lauf::default();
        assert!(stimme_maennlich(&l, Rolle::Kapitaen) && !stimme_maennlich(&l, Rolle::Kabine), "ohne Angabe wie bisher");
        let d = api_client::KabinenDrehbuch {
            sprachen: vec!["de-DE".into()],
            eintraege: vec![api_client::KabinenEintrag { slot: "reiseflug80".into(), sprache: "de-DE".into(), text: "x".into(), rolle: None, art: Some("ende_bordverkauf".into()), fremd: Vec::new() }],
            crew: Some(api_client::KabinenCrew { kapitaen: Some("Julia Lange".into()), kapitaen_geschlecht: Some("w".into()), kabine_geschlecht: Some("m".into()) }),
            boarding_lied: None,
            boarding_lied_spaeter: false,
            entfaellt: Default::default(),
        };
        drehbuch_uebernehmen(&mut l, d, &[Slot::Reiseflug80], false, false);
        assert!(!stimme_maennlich(&l, Rolle::Kapitaen), "Kapitänin spricht mit Frauenstimme");
        assert!(stimme_maennlich(&l, Rolle::Kabine), "Purser mit Männerstimme");
        assert!(!stimme_maennlich(&l, Rolle::Kommando), "Kommandos mit der Stimme der Kapitänin (2.1.0)");
        assert_eq!(l.art_r80.as_deref(), Some("ende_bordverkauf"));
        assert_eq!(l.crew.as_ref().and_then(|c| c.kapitaen.as_deref()), Some("Julia Lange"));
        // Eine spätere Antwort ohne `crew` (älteres Modul) vergisst die Crew nicht.
        drehbuch_uebernehmen(&mut l, api_client::KabinenDrehbuch { sprachen: vec![], eintraege: vec![], crew: None, boarding_lied: None, boarding_lied_spaeter: false, entfaellt: Default::default() }, &[], false, false);
        assert!(!stimme_maennlich(&l, Rolle::Kapitaen));
    }

    /// 2.1.0: Ohne Crew-Angabe sprechen Kommandos männlich wie der Kapitän.
    #[test]
    fn kommando_folgt_dem_kapitaen() {
        let l = Lauf::default();
        assert!(stimme_maennlich(&l, Rolle::Kommando));
        assert_eq!(stimme_maennlich(&l, Rolle::Kommando), stimme_maennlich(&l, Rolle::Kapitaen));
    }

    /// 2.1.0: Flug und Rolle gehen an den Ton-Server; die gemerkte Stimme
    /// gilt nur für denselben Flug und dieselbe Rolle.
    #[test]
    fn flug_und_rolle_fuer_den_ton_server() {
        assert_eq!(server_rolle(Rolle::Kapitaen), Some("kapitaen"));
        assert_eq!(server_rolle(Rolle::Kabine), Some("kabine"));
        assert_eq!(server_rolle(Rolle::Kommando), Some("kommando"));
        assert_eq!(server_rolle(Rolle::Chime), None);
        assert_eq!(flug_kennung("7xKq2A-b_9").as_deref(), Some("7xKq2A-b_9"));
        for falsch in ["", "a/b", "ab cd", "ä", &"x".repeat(65)] {
            assert_eq!(flug_kennung(falsch), None, "{falsch}");
        }
        let k = |f, r| gewaehlt_schluessel("auto:de-DE:w", f, r);
        assert_ne!(k(Some("A1"), Some("kapitaen")), k(Some("A2"), Some("kapitaen")), "anderer Flug");
        assert_ne!(k(Some("A1"), Some("kapitaen")), k(Some("A1"), Some("kabine")), "andere Rolle, gleiches Geschlecht");
        assert_eq!(k(Some("A1"), Some("kabine")), k(Some("A1"), Some("kabine")));
        let mit = piper::koerper("auto:de-DE:w", "Hallo", Some(0), None, None, piper::Wahl { flug: Some("A1"), rolle: Some("kabine"), fremd: &[] });
        assert_eq!(mit["pirep_id"], "A1");
        assert_eq!(mit["rolle"], "kabine");
        assert!(mit.get("fremd").is_none(), "ohne Fremdwörter kein Feld (alte Server)");
        // 2.1.0: Fremde Namen gehen unverändert an den Ton-Server.
        let fremd = [api_client::KabinenFremd { wort: "Times Square".into(), sprache: "en".into() }];
        let mf = piper::koerper("auto:de-DE:w", "Am Times Square", Some(0), None, None, piper::Wahl { flug: Some("A1"), rolle: Some("kabine"), fremd: &fremd });
        assert_eq!(mf["fremd"], serde_json::json!([{ "wort": "Times Square", "sprache": "en" }]));
        let ohne = piper::koerper("auto:de-DE:w", "Hallo", None, None, None, piper::Wahl::default());
        assert!(ohne.get("pirep_id").is_none() && ohne.get("rolle").is_none(), "Hörprobe ohne Flug");
    }

    fn wav_mit_satz(json: &str) -> Vec<u8> {
        let mut d = Vec::new();
        let daten = vec![0u8; 3200];
        let satz = json.as_bytes();
        d.extend_from_slice(b"RIFF");
        d.extend_from_slice(&((4 + 24 + 8 + daten.len() + 8 + satz.len() + (satz.len() & 1)) as u32).to_le_bytes());
        d.extend_from_slice(b"WAVEfmt ");
        d.extend_from_slice(&16u32.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&1u16.to_le_bytes());
        d.extend_from_slice(&16_000u32.to_le_bytes());
        d.extend_from_slice(&32_000u32.to_le_bytes());
        d.extend_from_slice(&2u16.to_le_bytes());
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(b"data");
        d.extend_from_slice(&(daten.len() as u32).to_le_bytes());
        d.extend_from_slice(&daten);
        d.extend_from_slice(b"satz");
        d.extend_from_slice(&(satz.len() as u32).to_le_bytes());
        d.extend_from_slice(satz);
        if satz.len() & 1 == 1 {
            d.push(0);
        }
        d
    }

    /// Ton-Server Paket 3 S1: Satz-Startzeiten im Abschnitt `satz` hinter `data`.
    #[test]
    fn satzzeiten_aus_der_wav() {
        let d = wav_mit_satz(r#"[{"ab_s":0.0,"text":"Guten Tag."},{"ab_s":1.25,"text":"Willkommen an Bord."}]"#);
        let st = wav_stuecke(&d);
        assert_eq!(st.len(), 2);
        assert_eq!(st[1].ab_s, 1.25);
        assert_eq!(st[1].text, "Willkommen an Bord.");
        assert!((wav_dauer_s(&d).unwrap() - 0.1).abs() < 1e-6, "Dauer zählt nur data");
        assert_eq!(ausgabe::wav_lesen(&d).unwrap().1.len(), 1600, "Abschnitt dahinter ist kein Ton");
        // Ohne Abschnitt oder kaputt: leer, die Seite schätzt.
        assert!(wav_stuecke(&wav(32_000, 3200)).is_empty());
        assert!(wav_stuecke(&wav_mit_satz("kein json")).is_empty());
    }

    /// Michael BER181: dreimal „sechzehn Uhr“ = Planankunft 14:00 UTC, gelandet
    /// 13:24. In der Luft zählt jetzt Abheben + Flugzeit, früher Abflug auch.
    #[test]
    fn ankunft_erwartet_nicht_mehr_nur_der_plan() {
        let plan_ein = 1_000_000;
        // Hochrechnung aus der Route hat Vorrang.
        assert_eq!(ankunft_erwartet(Some(5), Some(1), Some(27), Some(plan_ein), Some(-27), true), Some(5));
        // In der Luft ohne Route: Abheben + geplante Flugzeit.
        assert_eq!(ankunft_erwartet(None, Some(900_000), Some(27), Some(plan_ein), Some(-27), true), Some(900_000 + 27 * 60));
        // Nach dem Off-Block, noch am Boden: 27 min früher los → 27 min früher an.
        assert_eq!(ankunft_erwartet(None, None, Some(27), Some(plan_ein), Some(-27), true), Some(plan_ein - 27 * 60));
        // Am Gate: nur Verspätung zählt, eine „Verfrühung“ ist noch nicht sicher.
        assert_eq!(ankunft_erwartet(None, None, Some(27), Some(plan_ein), Some(-27), false), Some(plan_ein));
        assert_eq!(ankunft_erwartet(None, None, Some(27), Some(plan_ein), Some(20), false), Some(plan_ein + 20 * 60));
    }

    fn sinus(f: f32, rate: u32) -> Vec<f32> {
        (0..rate).map(|i| (2.0 * std::f32::consts::PI * f * i as f32 / rate as f32).sin() * 0.5).collect()
    }
    fn pegel(p: &[f32]) -> f32 {
        let rest = &p[p.len() / 2..];
        (rest.iter().map(|x| x * x).sum::<f32>() / rest.len() as f32).sqrt()
    }

    /// Hörprobe für Thomas (Punkt 11) mit genau dem ausgelieferten Filter:
    /// `KLANG_PROBE_WAV=<eingabe.wav> KLANG_PROBE_AUS=<ordner> cargo test … klang_hoerprobe -- --ignored`
    #[test]
    #[ignore = "erzeugt Hörproben-Dateien, braucht eine echte WAV"]
    fn klang_hoerprobe() {
        let ein = std::env::var("KLANG_PROBE_WAV").expect("KLANG_PROBE_WAV");
        let aus = std::path::PathBuf::from(std::env::var("KLANG_PROBE_AUS").expect("KLANG_PROBE_AUS"));
        let (rate, proben) = ausgabe::wav_lesen(&std::fs::read(ein).unwrap()).expect("PCM-16-WAV");
        let schreiben = |name: &str, rate: u32, p: &[f32]| {
            let mut d = b"RIFF".to_vec();
            d.extend_from_slice(&(36 + p.len() as u32 * 2).to_le_bytes());
            d.extend_from_slice(b"WAVEfmt ");
            d.extend_from_slice(&16u32.to_le_bytes());
            d.extend_from_slice(&1u16.to_le_bytes());
            d.extend_from_slice(&1u16.to_le_bytes());
            d.extend_from_slice(&rate.to_le_bytes());
            d.extend_from_slice(&(rate * 2).to_le_bytes());
            d.extend_from_slice(&2u16.to_le_bytes());
            d.extend_from_slice(&16u16.to_le_bytes());
            d.extend_from_slice(b"data");
            d.extend_from_slice(&(p.len() as u32 * 2).to_le_bytes());
            for x in p {
                d.extend_from_slice(&((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
            }
            std::fs::write(aus.join(name), d).unwrap();
        };
        let chime = ausgabe::chime(ausgabe::CHIME_RATE);
        for (name, k) in [("klar", ausgabe::Klang::Klar), ("lautsprecher", ausgabe::Klang::Lautsprecher), ("tuer", ausgabe::Klang::Tuer)] {
            let mut p = proben.clone();
            ausgabe::klang_anwenden(&mut p, rate, k);
            schreiben(&format!("ansage-{name}.wav"), rate, &p);
            let mut c = chime.clone();
            ausgabe::klang_anwenden(&mut c, ausgabe::CHIME_RATE, k);
            schreiben(&format!("gong-{name}.wav"), ausgabe::CHIME_RATE, &c);
        }
    }

    /// Punkt 11: Bordlautsprecher schneidet Tiefen und Höhen, die Tür klingt
    /// dumpf (Höhen weg), klar bleibt unverändert.
    #[test]
    fn klangstufen() {
        let r = 48_000;
        for (f, klang, soll_leise) in [
            (80.0, ausgabe::Klang::Lautsprecher, true),
            (1_000.0, ausgabe::Klang::Lautsprecher, false),
            (3_000.0, ausgabe::Klang::Tuer, true),
            (600.0, ausgabe::Klang::Tuer, false),
        ] {
            let mut p = sinus(f, r);
            let vorher = pegel(&p);
            ausgabe::klang_anwenden(&mut p, r, klang);
            let verhaeltnis = pegel(&p) / vorher;
            assert_eq!(verhaeltnis < 0.4, soll_leise, "{f} Hz {klang:?}: {verhaeltnis}");
        }
        let mut p = sinus(80.0, r);
        let vorher = p.clone();
        ausgabe::klang_anwenden(&mut p, r, ausgabe::Klang::Klar);
        assert_eq!(p, vorher);
        assert!(sinus(1_000.0, r).iter().all(|x| x.abs() <= 1.0));
    }

    #[test]
    fn neue_einstellungen_normalisiert_und_alte_datei_lesbar() {
        let alt: Einstellungen = serde_json::from_str(r#"{"an":true,"lautstaerke":0.5}"#).unwrap();
        let alt = alt.normalisiert();
        assert_eq!((alt.kapitaen_geschlecht.as_str(), alt.klang.as_str()), ("auto", "lautsprecher"));
        assert!(alt.aus.is_empty());
        let kaputt = Einstellungen { kapitaen_geschlecht: "x".into(), klang: "laut".into(), aus: vec!["boarding".into(), "boarding".into(), "quatsch".into()], ..Einstellungen::default() }.normalisiert();
        assert_eq!((kaputt.kapitaen_geschlecht.as_str(), kaputt.klang.as_str()), ("auto", "lautsprecher"));
        assert_eq!(kaputt.aus, vec!["boarding".to_string()]);
        assert_eq!(kaputt.abgewaehlt(), vec![Slot::Boarding]);
        // 2.1.1 G3: Altdatei ohne Musik-Felder → Musik an, eigener Pegel 20 %.
        assert!(alt.boarding_musik, "ab Werk an (Entscheidung 15)");
        assert!((alt.musik_lautstaerke - 0.2).abs() < 1e-6, "ab Werk 20 % (Entscheidung 14)");
        assert!((alt.lautstaerke - 0.5).abs() < 1e-6, "getrennte Regler");
        let laut = Einstellungen { musik_lautstaerke: f32::INFINITY, ..Einstellungen::default() }.normalisiert();
        assert!((laut.musik_lautstaerke - 0.2).abs() < 1e-6);
        let aus: Einstellungen = serde_json::from_str(r#"{"an":true,"boarding_musik":false,"musik_lautstaerke":0.35}"#).unwrap();
        assert!(!aus.boarding_musik && (aus.musik_lautstaerke - 0.35).abs() < 1e-6);
    }

    /// Punkt 5b: Anschnallzeichen im Reiseflug nach 3 min noch an → Hinweis,
    /// und „Anschnallzeichen aus“ wartet sichtbar auf den Piloten.
    #[test]
    fn hinweis_anschnallzeichen_im_reiseflug() {
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(cabin_announcements::Planer::neu(cabin_announcements::Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(120), ..Default::default() }));
        st.phase = crate::FlightPhase::Cruise;
        let mut snap = SimSnapshot { seatbelts_sign: Some(2), ..SimSnapshot::default() };
        hinweis_pruefen(&mut st, &snap, crate::FlightPhase::Cruise, 0);
        assert_eq!(st.kabinen_lauf.hinweis, None, "nicht sofort");
        hinweis_pruefen(&mut st, &snap, crate::FlightPhase::Cruise, 180);
        assert_eq!(st.kabinen_lauf.hinweis, Some("anschnallzeichen"));
        assert_eq!(zeile(&st, Slot::AnschnallzeichenAus).unwrap().wartet, Some(Wartet::Anschnallzeichen));
        snap.seatbelts_sign = Some(0);
        hinweis_pruefen(&mut st, &snap, crate::FlightPhase::Cruise, 190);
        assert_eq!(st.kabinen_lauf.hinweis, None, "Zeichen aus: weg");
        hinweis_pruefen(&mut st, &snap, crate::FlightPhase::Descent, 200);
        assert_eq!(st.kabinen_lauf.reiseflug_seit, None);
    }

    /// Codex beta.6: Turbulenz wartet nicht, macht danach aber Pause; ein
    /// Kommando macht keine.
    #[test]
    fn nach_turbulenz_pause_nach_kommando_nicht() {
        use crate::FlightPhase::*;
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, Cruise));
        st.kabinen_lauf = lauf_mit_ton(&[Slot::Turbulenz, Slot::KommandoLandungVorbereiten]);
        st.kabinen_lauf.texte.insert((Slot::KommandoLandungVorbereiten, "en-GB".into()), "x".into());
        st.kabinen_lauf.ton.insert((Slot::KommandoLandungVorbereiten, "en-GB".into()), ton());
        st.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::Turbulenz, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        let g = abspielen_schritt(&mut st, 100, Cruise, false, false).0.expect("Turbulenz");
        assert!(ende_melden(&mut st, g.id, "gespielt", 105, "backend"));
        assert!(st.kabinen_lauf.warteschlange.in_pause(110), "nach der Turbulenz Pause");
        let mut k = FlightStats::new();
        k.kabinen_planer = Some(planer_mit(Flugart::Linie, Descent));
        k.kabinen_lauf = lauf_mit_ton(&[]);
        k.kabinen_lauf.ton.insert((Slot::KommandoLandungVorbereiten, "en-GB".into()), ton());
        k.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::KommandoLandungVorbereiten, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        let g = abspielen_schritt(&mut k, 100, Descent, false, false).0.expect("Kommando");
        assert!(ende_melden(&mut k, g.id, "gespielt", 105, "backend"));
        assert!(!k.kabinen_lauf.warteschlange.in_pause(110), "nach einem Kommando keine Pause");
    }

    /// Codex beta.6: „läuft“ erst nach der Startmeldung der Oberfläche; bleibt
    /// sie aus, steht die Ansage nach 30 s auf „Ausgang unbekannt“.
    #[test]
    fn ohne_startmeldung_ausgang_unbekannt() {
        use crate::FlightPhase::*;
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, Cruise));
        st.kabinen_lauf = lauf_mit_ton(&[Slot::Reiseflug50]);
        st.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::Reiseflug50, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        let (g, _) = abspielen_schritt(&mut st, 100, Cruise, false, false);
        let g = g.expect("spielt");
        assert!(!g.gestartet && g.ergebnis.is_none());
        assert_eq!(g.rolle, Rolle::Kapitaen);
        abspielen_schritt(&mut st, 100 + START_SPAETESTENS_S, Cruise, false, false);
        assert_eq!(st.kabinen_lauf.verlauf[0].ergebnis, None, "noch innerhalb der Frist");
        abspielen_schritt(&mut st, 101 + START_SPAETESTENS_S, Cruise, false, false);
        assert_eq!(st.kabinen_lauf.verlauf[0].ergebnis.as_deref(), Some("unbekannt"));
        // Gegenprobe: mit Startmeldung bleibt sie „läuft“.
        let mut st2 = FlightStats::new();
        st2.kabinen_planer = Some(planer_mit(Flugart::Linie, Cruise));
        st2.kabinen_lauf = lauf_mit_ton(&[Slot::Reiseflug50]);
        st2.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::Reiseflug50, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        abspielen_schritt(&mut st2, 100, Cruise, false, false);
        st2.kabinen_lauf.verlauf[0].gestartet = true;
        abspielen_schritt(&mut st2, 120, Cruise, false, false);
        assert_eq!(st2.kabinen_lauf.verlauf[0].ergebnis, None);
        // 1b: Ohne Ende vom Backend auch gestartet nicht ewig „läuft“ — nach
        // Tondauer (1,8 s) plus Frist „unbekannt“ (vorher: für immer offen).
        abspielen_schritt(&mut st2, 100 + 2 + ABLAUF_FRIST_S, Cruise, false, false);
        assert_eq!(st2.kabinen_lauf.verlauf[0].ergebnis, None, "noch in der Frist");
        abspielen_schritt(&mut st2, 101 + 2 + ABLAUF_FRIST_S, Cruise, false, false);
        assert_eq!(st2.kabinen_lauf.verlauf[0].ergebnis.as_deref(), Some("unbekannt"));
    }

    #[test]
    fn text_fehler_aufgegeben_und_verfall() {
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, crate::FlightPhase::Boarding));
        st.kabinen_lauf.sprachen = vec!["de-DE".into()];
        let l = &mut st.kabinen_lauf;
        l.texte_fehler.insert(Slot::Boarding, "api 503".into());
        l.fehlversuche.insert("abflug".into(), (1, 500));
        assert_eq!(zeile(&st, Slot::Boarding).unwrap().stand, Stand::TextFehler { um: 500, meldung: "api 503".into() });
        st.kabinen_lauf.fehlversuche.insert("abflug".into(), (3, 500));
        assert_eq!(zeile(&st, Slot::Boarding).unwrap().stand, Stand::TextAufgegeben { meldung: "api 503".into() });
        assert_eq!(
            teil_stand(&st.kabinen_lauf, "abflug", &[Slot::Boarding], None, false),
            TeilStand::Aufgegeben { meldung: "api 503".into() }
        );
        // „Erneut versuchen“ (ohne Flug: Logik direkt) gibt frei.
        // Verfallen: Fehler weg, sie bestimmen nichts mehr.
        st.kabinen_lauf.ton_fehler.insert((Slot::Boarding, "de-DE".into()), TonFehler { versuche: 1, ab: 0, meldung: "x".into() });
        verfallen_aufraeumen(&mut st.kabinen_lauf, Slot::Boarding);
        assert!(st.kabinen_lauf.texte_fehler.is_empty() && st.kabinen_lauf.ton_fehler.is_empty());
        // Ankunft noch nicht angefragt: offen ab Landung − 45 min; kommt nichts mehr: entfällt.
        assert_eq!(teil_stand(&st.kabinen_lauf, "ankunft", &ANKUNFT, Some(9_000), false), TeilStand::Offen { um: Some(9_000 - 2_700) });
        assert_eq!(teil_stand(&st.kabinen_lauf, "ankunft", &ANKUNFT, Some(9_000), true), TeilStand::Offen { um: None }, "Kurzstrecke: nach dem Abheben");
        assert_eq!(teil_stand(&st.kabinen_lauf, "abflug", &[], None, false), TeilStand::Entfaellt);
    }

    #[test]
    fn bisher_erst_nach_rueckmeldung_und_vorhoeren_zaehlt_nicht() {
        let mut l = Lauf::default();
        l.sprachen = vec!["de-DE".into()];
        let k = (Slot::Reiseflug50, "de-DE".to_string());
        // Noch kein Ton: nichts zum Anhören.
        assert!(anhoeren(&mut l, Slot::Reiseflug50, None, 0).is_err());
        l.texte.insert(k.clone(), "Halbzeit".into());
        l.ton.insert(k.clone(), Some(Ton { datei: "f/a.wav".into(), dauer_s: 4.0, stuecke: vec![], herkunft: "server" }));
        let v = anhoeren(&mut l, Slot::Reiseflug50, None, 0).unwrap();
        assert!(v.vorschau && v.teile[0].text == "Halbzeit");
        assert!(l.warteschlange.belegt(3), "während des Anhörens spielt nichts anderes");
        assert!(anhoeren(&mut l, Slot::Reiseflug50, None, 1).is_err(), "gesperrt, solange etwas läuft");
        assert!(!wiedergabe_ende(&mut l, v.id, "gespielt", Utc::now().timestamp()), "Vorhören kommt nicht in den Verlauf");
        assert!(!l.warteschlange.belegt(1) && l.verlauf.is_empty());
        assert!(!l.warteschlange.in_pause(Utc::now().timestamp()), "Vorhören löst keine Pause aus");
        // Echte Ansage: erst nach der Rückmeldung „gespielt“.
        l.verlauf.push(Gespielt { id: 7, slot: Slot::Reiseflug50, rolle: Rolle::Kapitaen, zeit: 0, teile: v.teile.clone(), vorschau: false, ergebnis: None, gestartet: true, grund: None });
        l.warteschlange.spielt_bis(50);
        assert!(wiedergabe_ende(&mut l, 7, "abgebrochen", Utc::now().timestamp()));
        assert_eq!(l.verlauf[0].ergebnis.as_deref(), Some("abgebrochen"));
        assert!(!l.warteschlange.belegt(1), "nichts läuft mehr");
        assert!(l.warteschlange.in_pause(Utc::now().timestamp()), "aber erst nach der Pause die nächste reguläre");
        assert!(!wiedergabe_ende(&mut l, 7, "gespielt", Utc::now().timestamp()), "nur einmal");
        // Nochmal aus „Bisher“, auch wenn der Ton nicht mehr im Lauf steht.
        l.ton.clear();
        let n = anhoeren(&mut l, Slot::Reiseflug50, Some(7), 100).unwrap();
        assert_eq!(n.teile[0].datei, "f/a.wav");
    }

    #[test]
    fn ton_reihenfolge_faellig_zuerst_dann_flugreihenfolge_nur_was_noch_kommt() {
        let mut st = FlightStats::new();
        st.kabinen_planer = Some(planer_mit(Flugart::Linie, crate::FlightPhase::Cruise));
        st.phase = crate::FlightPhase::Cruise;
        st.kabinen_lauf.sprachen = vec!["fr-FR".into(), "de-DE".into()];
        for s in [Slot::Boarding, Slot::Reiseflug25, Slot::Reiseflug80, Slot::KapitaenSinkflug] {
            for sp in ["de-DE", "fr-FR"] {
                st.kabinen_lauf.texte.insert((s, sp.into()), "x".into());
            }
        }
        st.kabinen_lauf.warteschlange.aufnehmen([Faellig { slot: Slot::Reiseflug80, verfall: cabin_announcements::Verfall::Um(9_999), trotz_ruhephase: false }]);
        let reihe: Vec<(Slot, String)> = ton_kandidaten(&st, false, Flugart::Linie, 0).into_iter().map(|(s, sp, _)| (s, sp)).collect();
        assert_eq!(
            reihe,
            vec![
                (Slot::Reiseflug80, "fr-FR".into()),
                (Slot::Reiseflug80, "de-DE".into()),
                (Slot::Reiseflug25, "fr-FR".into()),
                (Slot::Reiseflug25, "de-DE".into()),
                (Slot::KapitaenSinkflug, "fr-FR".into()),
                (Slot::KapitaenSinkflug, "de-DE".into()),
            ],
            "Boarding ist vorbei (Wiedereinstieg im Reiseflug): kein Ton mehr dafür"
        );
        // Kommandos reihen sich in Flugreihenfolge ein, Englisch.
        let mit: Vec<Slot> = ton_kandidaten(&st, true, Flugart::Linie, 0).into_iter().map(|(s, _, _)| s).collect();
        assert!(mit.contains(&Slot::KommandoLandungVorbereiten) && !mit.contains(&Slot::KommandoStart));
    }

    /// Advisor beta.5: Zwei wartende Vorrats-Töne dürfen eine fällige
    /// Ansage (Turbulenz, Kommando) nicht aufhalten.
    #[test]
    fn faellige_ansage_geht_auch_bei_vollem_vorrat_raus() {
        let k = |s: Slot| (s, "de-DE".to_string(), "x".to_string());
        let kandidaten = vec![k(Slot::Turbulenz), k(Slot::Reiseflug80), k(Slot::KapitaenSinkflug)];
        let gewaehlt: Vec<(Slot, bool)> = ton_auswahl(kandidaten.clone(), &[Slot::Turbulenz], 2).into_iter().map(|(s, _, _, f)| (s, f)).collect();
        assert_eq!(gewaehlt, vec![(Slot::Turbulenz, true)], "Vorrat voll, die fällige geht trotzdem");
        let gewaehlt: Vec<Slot> = ton_auswahl(kandidaten, &[], 1).into_iter().map(|(s, _, _, _)| s).collect();
        assert_eq!(gewaehlt, vec![Slot::Turbulenz], "ohne Fälliges höchstens die freien Plätze");
    }

    #[test]
    fn grund_nennt_die_erste_sperre() {
        let mut st = FlightStats::new();
        assert_eq!(grund(false, &st), Some(Grund::Aus));
        st.flight_type = Some("F".into());
        assert_eq!(grund(true, &st), Some(Grund::Fracht));
        st.flight_type = Some("J".into());
        // Die Freischaltung kommt jetzt vor der Passagierzahl (früh wissen).
        assert_eq!(grund(true, &st), Some(Grund::BetaPruefung));
        st.kabinen_lauf.beta_netzfehler = true;
        st.kabinen_lauf.beta_naechste_pruefung = 1234;
        assert_eq!(grund(true, &st), Some(Grund::BetaNichtErreichbar(1234)));
        st.kabinen_lauf.beta = Beta::Nein;
        assert_eq!(grund(true, &st), Some(Grund::NichtFreigeschaltet));
        st.kabinen_lauf.beta = Beta::Ja;
        assert_eq!(grund(true, &st), Some(Grund::KeinePax), "Fall Thomas: Wechsel mitten im Flug");
        st.pax_count = Some(0);
        assert_eq!(grund(true, &st), Some(Grund::KeinePassagiere));
        st.pax_count = Some(150);
        assert_eq!(grund(true, &st), None, "alles bereit");
        st.kabinen_lauf.sim_angehalten = true;
        assert_eq!(grund(true, &st), Some(Grund::SimAngehalten));
        st.kabinen_lauf.sim_angehalten = false;
        st.kabinen_lauf.kontingent_erschoepft = true;
        assert_eq!(grund(true, &st), Some(Grund::Kontingent));
        st.replay_verdacht = true;
        assert_eq!(grund(true, &st), Some(Grund::Replay));
        // So kommt es in der Oberflaeche an.
        assert_eq!(
            serde_json::to_value(Grund::BetaNichtErreichbar(5)).unwrap(),
            serde_json::json!({ "grund": "beta_nicht_erreichbar", "detail": 5 })
        );
        assert_eq!(serde_json::to_value(Grund::KeinePax).unwrap(), serde_json::json!({ "grund": "keine_pax" }));
        assert_eq!(
            serde_json::to_value(Stand::TonWartet { um: 1, meldung: "m".into() }).unwrap(),
            serde_json::json!({ "art": "ton_wartet", "um": 1, "meldung": "m" })
        );
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


    /// Thomas 17.10.2026: „Bleiben die auf dem Rechner? Werden die
    /// gelöscht nach dem Flug?“ — Töne nur für den laufenden Flug.
    #[test]
    fn toene_bleiben_nur_fuer_den_laufenden_flug() {
        let d = std::env::temp_dir().join(format!("kabine-aufraeumen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for (ordner, datei) in [("ALT1", "a.wav"), ("ALT2", "b.wav"), ("NEU", "c.wav"), (PROBE_ORDNER, "p.wav")] {
            std::fs::create_dir_all(d.join(ordner)).unwrap();
            std::fs::write(d.join(ordner).join(datei), b"x").unwrap();
        }
        // Lose Dateien der früheren Ablage (bis beta.4).
        std::fs::write(d.join("server-1.wav"), b"x").unwrap();
        std::fs::write(d.join("piper-2.wav"), b"x").unwrap();
        let namen = |p: &Path| {
            let mut v: Vec<String> = std::fs::read_dir(p).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
            v.sort();
            v
        };
        // App-Start: lose Dateien weg; Flug-Ordner bleiben (einer davon
        // kann der gleich fortgesetzte Flug sein), frische Proben auch.
        altes_aufraeumen(&d);
        assert_eq!(namen(&d), ["ALT1", "ALT2", "NEU", "probe"]);
        // Erster Ton eines neuen Fluges: andere Flüge weg, Proben bleiben.
        andere_fluege_weg(&d, "NEU");
        assert_eq!(namen(&d), ["NEU", "probe"]);
        std::fs::remove_file(d.join("NEU").join("c.wav")).unwrap();
        assert_eq!(ordner_groesse(&d), 1, "nur noch die Probe");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nur_eigene_tondateien() {
        assert!(datei_sauber("ARO6rb14EmQw6OVx/server-833a5f35159f503b.wav"));
        assert!(datei_sauber("probe/server-1.wav"));
        assert!(!datei_sauber("server-1.wav"), "ohne Ordner (alte Ablage) nicht mehr");
        assert!(!datei_sauber("../x/server-1.wav"));
        assert!(!datei_sauber("a/../b.wav"));
        assert!(!datei_sauber("a/b/c.wav"));
        assert!(!datei_sauber("/etc/passwd.wav"));
        assert!(!datei_sauber("a/b.mp3"));
        assert!(!datei_sauber("a/b c.wav"));
    }

    #[test]
    fn verlauf_uebersteht_den_neustart() {
        let g = Gespielt { id: 3, slot: Slot::Boarding, rolle: Rolle::Kabine, zeit: 5, teile: vec![], vorschau: false, ergebnis: Some("gespielt".into()), gestartet: true, grund: None };
        let text = serde_json::to_string(&vec![g]).unwrap();
        let zurueck: Vec<Gespielt> = serde_json::from_str(&text).unwrap();
        assert_eq!(zurueck[0].id, 3);
        assert_eq!(zurueck[0].ergebnis.as_deref(), Some("gespielt"));
        // Verlauf aus beta.4 (ohne id/ergebnis) lädt auch.
        let alt: Vec<Gespielt> = serde_json::from_str(r#"[{"slot":"boarding","rolle":"kabine","zeit":1,"teile":[],"lautstaerke":0.8}]"#).unwrap();
        assert_eq!(alt[0].ergebnis, None);
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
        ausgabe::spielen(None, rate, &ausgabe::chime(rate), ausgabe::stopp_stand(), ausgabe::Klang::Lautsprecher).expect("spielt");
        let dauer = uhr.elapsed().as_secs_f64();
        println!("gespielt in {dauer:.2} s");
        assert!(dauer >= 2.1, "nicht vorzeitig beendet: {dauer}");
        // Auf ein gewaehltes Geraet (das erste der Liste) und gleich gestoppt.
        let stand = ausgabe::stopp_stand();
        ausgabe::stoppen();
        let uhr = std::time::Instant::now();
        ausgabe::spielen(Some(&liste[0].id), rate, &ausgabe::chime(rate), stand, ausgabe::Klang::Lautsprecher).expect("spielt");
        assert!(uhr.elapsed().as_secs_f64() < 0.5, "Stopp vor dem Start greift");
    }
}

/// Rundumschlag 22.10.2026, Paket P1 Kabine: je Befund (1a–1f, 4) ein Test,
/// der auf v2.1.6 rot ist — Abnahmekriterien aus
/// `handoff/2026-10-22-rundumschlag.md`, Punkt 1.
#[cfg(test)]
mod rundumschlag_p1_tests {
    use super::*;
    use crate::FlightPhase::*;
    use cabin_announcements::Verfall;

    fn ton_s(dauer_s: f64) -> Option<Ton> {
        Some(Ton { datei: "f/x.wav".into(), dauer_s, stuecke: vec![], herkunft: "server" })
    }

    /// Ein freigeschalteter Linienflug mit Passagieren; Ton für `slots` in
    /// Deutsch und Englisch (Kommandos nur Englisch).
    fn flug(phase: FlightPhase, slots: &[Slot]) -> FlightStats {
        let mut st = FlightStats::new();
        st.pax_count = Some(150);
        st.phase = phase;
        st.kabinen_planer = Some(cabin_announcements::Planer::fortsetzen(
            cabin_announcements::Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(120), reiseflughoehe_ft: Some(36_000), ..Default::default() },
            phase,
        ));
        let l = &mut st.kabinen_lauf;
        l.beta = Beta::Ja;
        l.sprachen = vec!["de-DE".into(), "en-GB".into()];
        for s in slots {
            let sprachen: &[&str] = if s.rolle() == Rolle::Kommando { &["en-GB"] } else { &["de-DE", "en-GB"] };
            for sp in sprachen {
                l.texte.insert((*s, (*sp).into()), format!("{s:?} {sp}"));
                l.ton.insert((*s, (*sp).into()), ton_s(20.0));
            }
        }
        st
    }

    fn faellig(slot: Slot) -> Faellig {
        Faellig { slot, verfall: Verfall::Um(99_999), trotz_ruhephase: false }
    }

    /// Spielt eine ausgegebene Ansage wie das Backend, mit eingeschleustem
    /// Gerät und Uhr (`start` beim Beginn, danach `ende`): liefert Ergebnis,
    /// gespielte Sprachen und Pausen.
    fn backend_spielt(st: FlightStats, id: u64, start: i64, ende: i64, stopp_nach: Option<usize>) -> (crate::stats_sperre::GemesseneSperre<FlightStats>, Option<&'static str>, Vec<String>, Vec<u64>) {
        let sperre = crate::stats_sperre::GemesseneSperre::new(st);
        let gespielt = std::cell::RefCell::new(Vec::<String>::new());
        let pausen = std::cell::RefCell::new(Vec::<u64>::new());
        let uhr_gefragt = std::cell::Cell::new(false);
        let ergebnis = {
            let mut spielen = |t: Option<&Teil>| -> Result<(), String> {
                gespielt.borrow_mut().push(t.map_or("gong".to_string(), |t| t.sprache.clone()));
                Ok(())
            };
            let gestoppt = || stopp_nach.is_some_and(|n| gespielt.borrow().len() >= n);
            let warten = |ms: u64| pausen.borrow_mut().push(ms);
            let uhr = || if uhr_gefragt.replace(true) { ende } else { start };
            let mut teil = |_: usize| {};
            let sp = Spieler { spielen: &mut spielen, gestoppt: &gestoppt, warten_ms: &warten, jetzt: &uhr, teil_beginnt: &mut teil };
            ablauf(&sperre, id, sp).map(|(e, _, _)| e)
        };
        (sperre, ergebnis, gespielt.into_inner(), pausen.into_inner())
    }

    fn arten(lauf: &Lauf) -> Vec<(String, Option<String>)> {
        lauf.protokoll.iter().map(|n| (n.art.to_string(), n.details.get("grund").and_then(|g| g.as_str()).map(str::to_string))).collect()
    }

    // ---- 1b ----

    /// 1b: Das Backend spielt alle Sprachteile und trägt `ende` selbst ein —
    /// die Oberfläche meldet nichts. Rot auf v2.1.6: Ablauf und Ende lagen in
    /// `kabinenansagen.ts`, ohne Meldung blieb die Ansage offen.
    #[test]
    fn backend_spielt_alle_teile_und_traegt_das_ende_ein() {
        let mut st = flug(Boarding, &[Slot::Boarding]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Boarding)]);
        let g = abspielen_schritt(&mut st, 100, Boarding, false, false).0.expect("Boarding spielt");
        let (sperre, ergebnis, gespielt, pausen) = backend_spielt(st, g.id, 101, 145, None);
        assert_eq!(ergebnis, Some("gespielt"));
        assert_eq!(gespielt, vec!["de-DE", "en-GB"], "beide Sprachen, in Reihenfolge");
        assert_eq!(pausen, vec![700], "eine Pause zwischen den Sprachen");
        let st = sperre.lock().unwrap();
        let e = st.kabinen_lauf.verlauf.iter().find(|x| x.id == g.id).unwrap();
        assert_eq!(e.ergebnis.as_deref(), Some("gespielt"));
        assert!(e.gestartet, "Start setzt das Backend selbst");
        let ende: Vec<&Notiz> = st.kabinen_lauf.protokoll.iter().filter(|n| n.art == "ende").collect();
        assert_eq!(ende.len(), 1);
        assert_eq!(ende[0].details["quelle"], "backend");
        assert!(!st.kabinen_lauf.warteschlange.belegt(145), "nach dem Ende frei");
    }

    /// 1b: Eine abgelaufene (schon erledigte) Ansage wird nicht nachgespielt,
    /// eine, die die Startfrist verpasst hat, auch nicht.
    #[test]
    fn abgelaufene_ansage_wird_nicht_nachgespielt() {
        let mut st = flug(Boarding, &[Slot::Boarding]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Boarding)]);
        let g = abspielen_schritt(&mut st, 100, Boarding, false, false).0.unwrap();
        st.kabinen_lauf.verlauf.iter_mut().find(|x| x.id == g.id).unwrap().ergebnis = Some("unbekannt".into());
        let (_, ergebnis, gespielt, _) = backend_spielt(st, g.id, 101, 101, None);
        assert_eq!((ergebnis, gespielt.len()), (None, 0), "erledigt: nichts spielt");

        let mut st = flug(Boarding, &[Slot::Boarding]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Boarding)]);
        let g = abspielen_schritt(&mut st, 100, Boarding, false, false).0.unwrap();
        let (_, ergebnis, gespielt, _) = backend_spielt(st, g.id, 100 + START_SPAETESTENS_S + 1, 200, None);
        assert_eq!((ergebnis, gespielt.len()), (None, 0), "Startfrist verpasst: nicht verspätet spielen");
        // Gegenprobe: rechtzeitig spielt sie.
        let mut st = flug(Boarding, &[Slot::Boarding]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Boarding)]);
        let g = abspielen_schritt(&mut st, 100, Boarding, false, false).0.unwrap();
        assert_eq!(backend_spielt(st, g.id, 101, 145, None).1, Some("gespielt"));
    }

    /// 1b: Stopp zwischen den Sprachen bricht den Rest ab.
    #[test]
    fn stopp_zwischen_den_sprachen_bricht_ab() {
        let mut st = flug(Boarding, &[Slot::Boarding]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Boarding)]);
        let g = abspielen_schritt(&mut st, 100, Boarding, false, false).0.unwrap();
        let (_, ergebnis, gespielt, _) = backend_spielt(st, g.id, 101, 130, Some(1));
        assert_eq!(ergebnis, Some("abgebrochen"));
        assert_eq!(gespielt, vec!["de-DE"]);
    }

    /// 1b: Solange die vorige Ansage kein Ende hat, beginnt keine neue — auch
    /// wenn ihre geschätzte Dauer (`belegt_bis`) vorbei ist. Rot auf v2.1.6:
    /// dort gab `belegt_bis` die nächste frei, beide spielten übereinander.
    #[test]
    fn keine_neue_ansage_solange_die_alte_kein_ende_hat() {
        let mut st = flug(Boarding, &[Slot::Boarding, Slot::BoardingAbgeschlossen]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Boarding)]);
        let a = abspielen_schritt(&mut st, 100, Boarding, false, false).0.unwrap();
        st.kabinen_lauf.verlauf.iter_mut().find(|x| x.id == a.id).unwrap().gestartet = true;
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::BoardingAbgeschlossen)]);
        // Geschätzte Dauer ~43 s; bei 160 läuft A laut Backend noch.
        assert!(abspielen_schritt(&mut st, 160, Boarding, false, false).0.is_none(), "B wartet auf das Ende von A");
        assert!(ende_melden(&mut st, a.id, "gespielt", 161, "backend"));
        assert_eq!(abspielen_schritt(&mut st, 162, Boarding, false, false).0.map(|g| g.slot), Some(Slot::BoardingAbgeschlossen));
    }

    /// 1b: Ein spätes Ende einer älteren Ansage verändert die laufende nicht.
    /// Rot auf v2.1.6: Das alte Ende fand die offene Ansage und gab die
    /// Belegung der neuen frei.
    #[test]
    fn spaetes_altes_ende_aendert_die_laufende_nicht() {
        let mut st = flug(Boarding, &[Slot::Boarding, Slot::BoardingAbgeschlossen]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Boarding)]);
        let a = abspielen_schritt(&mut st, 100, Boarding, false, false).0.unwrap();
        st.kabinen_lauf.verlauf.iter_mut().find(|x| x.id == a.id).unwrap().gestartet = true;
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::BoardingAbgeschlossen)]);
        // Nach Tondauer + Frist ohne Ende: A gilt als unbekannt, B beginnt.
        let t = 100 + dauer_s(a.slot, &a.teile).ceil() as i64 + ABLAUF_FRIST_S + 1;
        let b = abspielen_schritt(&mut st, t, Boarding, false, false).0.expect("B nach der Frist");
        assert_eq!(st.kabinen_lauf.verlauf.iter().find(|x| x.id == a.id).unwrap().ergebnis.as_deref(), Some("unbekannt"));
        assert!(st.kabinen_lauf.warteschlange.belegt(t + 1));
        // Jetzt kommt das alte Ende doch noch.
        assert!(!ende_melden(&mut st, a.id, "gespielt", t + 1, "oberflaeche"), "altes Ende wird nicht verbucht");
        assert!(st.kabinen_lauf.warteschlange.belegt(t + 1), "B bleibt belegt");
        assert!(st.kabinen_lauf.verlauf.iter().find(|x| x.id == b.id).unwrap().ergebnis.is_none());
    }

    // ---- 1c ----

    /// 1c: Hängende Oberfläche — Kabine Sinkflug zählt ab dem Tonende des
    /// Backends + 120 s. Rot auf v2.1.6: ohne Ende der Oberfläche blieb der
    /// Kapitän „gemeldet“, die Kabine hatte keinen Zeitpunkt.
    #[test]
    fn kabine_sinkflug_ab_backend_tonende() {
        let mut st = flug(Cruise, &[Slot::KapitaenSinkflug]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::KapitaenSinkflug)]);
        let g = abspielen_schritt(&mut st, 1_000, Cruise, false, false).0.expect("Kapitän spielt");
        let (sperre, ergebnis, _, _) = backend_spielt(st, g.id, 1_001, 1_046, None);
        assert_eq!(ergebnis, Some("gespielt"));
        let st = sperre.lock().unwrap();
        assert_eq!(st.kabinen_planer.as_ref().unwrap().zeitpunkt(Slot::KabineSinkflug, None), Some(1_046 + cabin_announcements::planer::KABINE_NACH_KAPITAEN_MIN_S));
    }

    // ---- 1a ----

    /// 1a: Arrived mit wartendem „Türen entschärfen“ + „Aussteigen“ — die
    /// Abgabe wartet bis zum Ende von „Aussteigen“; Ansagen aus → sofort.
    #[test]
    fn abgabe_wartet_bis_aussteigen_gespielt_ist() {
        let mut st = flug(TaxiIn, &[Slot::KommandoTuerenEntschaerfen, Slot::Aussteigen]);
        let tick = cabin_announcements::Tick { jetzt: 1_000, phase: Arrived, am_boden: true, triebwerke_an: Some(false), ..Default::default() };
        let f = st.kabinen_planer.as_mut().unwrap().tick(&tick);
        assert!(f.iter().any(|x| x.slot == Slot::Aussteigen), "Aussteigen fällig bei Arrived");
        st.phase = Arrived;
        st.kabinen_lauf.warteschlange.aufnehmen(f);
        assert!(!kabine_haelt_abgabe(&mut st, false, 1_000), "Ansagen aus: sofort einreichen");
        assert!(kabine_haelt_abgabe(&mut st, true, 1_000), "wartet auf die Kabine");
        let mut t = 1_000;
        let mut gespielt = Vec::new();
        while t < 1_000 + ABGABE_WARTET_HOECHSTENS_S {
            if let Some(g) = abspielen_schritt(&mut st, t, Arrived, false, false).0 {
                gespielt.push(g.slot);
                let ende = t + dauer_s(g.slot, &g.teile).ceil() as i64;
                assert!(kabine_haelt_abgabe(&mut st, true, ende - 1), "{:?} spielt noch", g.slot);
                ende_melden(&mut st, g.id, "gespielt", ende, "backend");
                t = ende;
                if g.slot == Slot::Aussteigen {
                    break;
                }
            }
            t += 1;
        }
        assert_eq!(gespielt, vec![Slot::KommandoTuerenEntschaerfen, Slot::Aussteigen]);
        assert!(!kabine_haelt_abgabe(&mut st, true, t), "nach dem Ende von Aussteigen: einreichen");
        assert!(arten(&st.kabinen_lauf).iter().any(|(a, _)| a == "abgabe_wartet"));
    }

    /// 1a: Obergrenze 90 s, und was beim Einreichen noch wartet, steht als
    /// `entfallen{flug_beendet}` im Protokoll und in „Bisher“ (nie still).
    #[test]
    fn abgabe_obergrenze_und_rest_als_flug_beendet() {
        let mut st = flug(Arrived, &[]);
        st.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Aussteigen)]);
        assert!(kabine_haelt_abgabe(&mut st, true, 1_000));
        assert!(kabine_haelt_abgabe(&mut st, true, 1_000 + ABGABE_WARTET_HOECHSTENS_S - 1));
        assert!(!kabine_haelt_abgabe(&mut st, true, 1_000 + ABGABE_WARTET_HOECHSTENS_S), "Obergrenze greift");
        let einst = Einstellungen { an: true, kommandos: true, ..Einstellungen::default() };
        flug_beendet_notieren(&mut st, &einst, 1_090);
        assert!(st.kabinen_lauf.warteschlange.ist_leer());
        let g = st.kabinen_lauf.verlauf.iter().find(|g| g.slot == Slot::Aussteigen).expect("in Bisher");
        assert_eq!((g.ergebnis.as_deref(), g.grund.as_deref()), (Some("entfallen"), Some("flug_beendet")));
        let n = arten(&st.kabinen_lauf).into_iter().filter(|(a, g)| a == "entfallen" && g.as_deref() == Some("flug_beendet")).count();
        assert_eq!(n, 1);
        // Gegenprobe: nichts wartet, nichts kommt mehr → kein Eintrag.
        let mut leer = flug(Arrived, &[]);
        leer.kabinen_planer.as_mut().unwrap().schon_gespielt([Slot::NachDerLandung, Slot::KommandoTuerenEntschaerfen, Slot::Aussteigen]);
        flug_beendet_notieren(&mut leer, &einst, 1_090);
        assert!(leer.kabinen_lauf.protokoll.is_empty() && leer.kabinen_lauf.verlauf.is_empty());
        // Ansagen aus: nichts vermerken (es lief nichts).
        let mut aus = flug(Arrived, &[]);
        aus.kabinen_lauf.warteschlange.aufnehmen([faellig(Slot::Aussteigen)]);
        flug_beendet_notieren(&mut aus, &Einstellungen::default(), 1_090);
        assert!(aus.kabinen_lauf.protokoll.is_empty());
    }

    // ---- 1d ----

    /// 1d: Jeder Wechsel einer Eingangssperre erzeugt genau einen Eintrag,
    /// ohne Wechsel keinen. Rot auf v2.1.6: es gab keinen (CDC 8939).
    #[test]
    fn jeder_sperrwechsel_genau_ein_eintrag() {
        let mut st = flug(Boarding, &[]);
        sperre_pruefen(&mut st, true, 0);
        sperre_pruefen(&mut st, true, 1);
        assert!(st.kabinen_lauf.protokoll.is_empty(), "frei ohne Wechsel: nichts");
        sperre_pruefen(&mut st, false, 2);
        sperre_pruefen(&mut st, false, 3);
        sperre_pruefen(&mut st, true, 4);
        st.kabinen_lauf.sim_angehalten = true;
        sperre_pruefen(&mut st, true, 5);
        sperre_pruefen(&mut st, true, 6);
        st.kabinen_lauf.sim_angehalten = false;
        sperre_pruefen(&mut st, true, 7);
        st.kabinen_lauf.beta = Beta::Angefragt;
        sperre_pruefen(&mut st, true, 8);
        st.kabinen_lauf.beta = Beta::Ja;
        sperre_pruefen(&mut st, true, 9);
        st.pax_count = None;
        sperre_pruefen(&mut st, true, 10);
        st.pax_count = Some(150);
        sperre_pruefen(&mut st, true, 11);
        st.flight_type = Some("F".into());
        sperre_pruefen(&mut st, true, 12);
        sperre_pruefen(&mut st, true, 13);
        let erwartet: Vec<(String, Option<String>)> = [
            ("gesperrt", Some("aus")),
            ("frei", None),
            ("gesperrt", Some("sim_angehalten")),
            ("frei", None),
            ("gesperrt", Some("beta_pruefung")),
            ("frei", None),
            ("gesperrt", Some("keine_pax")),
            ("frei", None),
            ("gesperrt", Some("fracht")),
        ]
        .into_iter()
        .map(|(a, g)| (a.to_string(), g.map(str::to_string)))
        .collect();
        assert_eq!(arten(&st.kabinen_lauf), erwartet);
    }

    /// 1d / Entscheidung 6: Nach dem Wiedereinschalten Text und Ton sofort
    /// bestellen — keine Wartezeit aus einem früheren Fehlversuch, und der
    /// Ton der Sicherheit geht als Vorrat raus, nicht erst beim Beacon.
    #[test]
    fn nach_wiedereinschalten_sofort_vorab_bestellt() {
        let mut st = flug(Boarding, &[]);
        st.kabinen_lauf.texte.insert((Slot::Sicherheit, "de-DE".into()), "Sicherheit".into());
        st.kabinen_lauf.fehlversuche.insert("ankunft".into(), (1, 500));
        st.kabinen_lauf.ton_fehler.insert((Slot::Sicherheit, "de-DE".into()), TonFehler { versuche: 1, ab: 500, meldung: "503".into() });
        sperre_pruefen(&mut st, false, 300);
        assert!(!darf_anfragen(&st.kabinen_lauf, "ankunft", 301) && !ton_noetig(&st.kabinen_lauf, &(Slot::Sicherheit, "de-DE".into()), 301), "Vorbedingung: Wartezeit läuft");
        sperre_pruefen(&mut st, true, 301);
        assert!(darf_anfragen(&st.kabinen_lauf, "ankunft", 301), "Text sofort");
        assert!(ton_noetig(&st.kabinen_lauf, &(Slot::Sicherheit, "de-DE".into()), 301), "Ton sofort");
        let auswahl = ton_auswahl(ton_kandidaten(&st, false, Flugart::Linie, 301), &[], 0);
        assert!(auswahl.iter().any(|(s, sp, _, faellig)| *s == Slot::Sicherheit && sp == "de-DE" && !faellig), "Sicherheit als Vorrat: {auswahl:?}");
    }

    // ---- 1e ----

    /// 1e: Kurzer Flug, Halbzeit vor dem Ende des Steigflugs — der Text wird
    /// schon im Steigflug angefragt, 15 min vor dem Zeitpunkt. Rot auf v2.1.6:
    /// erst bei Reiseflug-Beginn, dann sofort fällig (36–39 s Stille).
    #[test]
    fn halbzeit_text_schon_im_steigflug() {
        let f = cabin_announcements::Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(70), reiseflughoehe_ft: Some(38_000), abheben_epoch: Some(1_000), ..Default::default() };
        let p = cabin_announcements::Planer::fortsetzen(f, Climb);
        let um = p.zeitpunkt(Slot::Reiseflug50, None).expect("Halbzeit hat einen Zeitpunkt");
        let toc = um + 300; // Reiseflug beginnt erst 5 min nach der Halbzeit
        let erste = (1_000..toc).step_by(10).find(|&t| halbzeit_anfragen(p.kommt_noch(Slot::Reiseflug50, Climb), Climb, Some(um), t));
        let a = erste.expect("im Steigflug angefragt");
        assert!(a < toc && (um - a - HALBZEIT_VORLAUF_MIN * 60).abs() <= 10, "Vorlauf {} s", um - a);
        assert!(!halbzeit_anfragen(true, Climb, None, 0), "im Steigflug nur mit bekanntem Zeitpunkt");
        assert!(!halbzeit_anfragen(true, TaxiOut, Some(10), 0), "nicht vor dem Abheben");
    }

    // ---- 1f ----

    fn drehbuch(eintraege: &[(&str, &str, &str)]) -> api_client::KabinenDrehbuch {
        api_client::KabinenDrehbuch {
            sprachen: vec!["de-DE".into(), "en-GB".into()],
            eintraege: eintraege
                .iter()
                .map(|(s, sp, t)| api_client::KabinenEintrag { slot: (*s).into(), sprache: (*sp).into(), text: (*t).into(), rolle: None, art: None, fremd: Vec::new() })
                .collect(),
            crew: None,
            boarding_lied: None,
            boarding_lied_spaeter: false,
            entfaellt: Default::default(),
        }
    }

    /// 1f: Nachholung je Slot UND Sprache — vorhandene Texte (und ihr Ton)
    /// bleiben; fehlt eine Sprache auch danach, spielt die Ansage in der
    /// anderen, und „Aufgegeben“ steht nur an dieser Sprache. Rot auf v2.1.6:
    /// Die Nachholung ersetzte den deutschen Text, und der ganze Abflug-Teil
    /// stand auf „Aufgegeben“.
    #[test]
    fn nachholung_je_slot_und_sprache() {
        let mut l = Lauf::default();
        let f = drehbuch_uebernehmen(&mut l, drehbuch(&[("boarding", "de-DE", "Alt"), ("sicherheit", "de-DE", "S"), ("sicherheit", "en-GB", "S")]), &[Slot::Boarding, Slot::Sicherheit], false, false);
        assert_eq!(f, vec![(Slot::Boarding, "en-GB".to_string())], "nur die fehlende Sprache");
        l.ton.insert((Slot::Boarding, "de-DE".into()), ton_s(5.0));
        let f2 = drehbuch_uebernehmen(&mut l, drehbuch(&[("boarding", "de-DE", "Neu")]), &[Slot::Boarding], false, true);
        assert_eq!(l.texte[&(Slot::Boarding, "de-DE".to_string())], "Alt", "Nachholung ersetzt nichts");
        assert!(l.ton.contains_key(&(Slot::Boarding, "de-DE".to_string())), "Ton bleibt");
        assert_eq!(f2, vec![(Slot::Boarding, "en-GB".to_string())]);
        // Wie `holen` nach der erfolglosen Nachholung:
        for k in f2 {
            l.texte_fehlen.insert(k, "Der Server hat keinen Text geliefert.".into());
        }
        assert!(ansage_bereit(&l.texte, &l.ton, &l.texte_fehler, &l.texte_fehlen, &l.sprachen, Slot::Boarding), "spielt auf Deutsch");
        assert_eq!(teile_von(&l, Slot::Boarding).unwrap().len(), 1);
        assert_eq!(teil_stand(&l, "abflug", &[Slot::Boarding, Slot::Sicherheit], None, false), TeilStand::Fertig, "Teil nicht aufgegeben");
        let (stand, teile) = slot_stand(&l, Slot::Boarding, None, false, None);
        assert!(!matches!(stand, Stand::TextAufgegeben { .. }));
        assert!(teile.iter().find(|t| t.sprache == "en-GB").unwrap().fehlt.is_some());
        assert!(teile.iter().find(|t| t.sprache == "de-DE").unwrap().fehlt.is_none());
        // Gegenprobe: fehlt eine Ansage in allen Sprachen, steht sie selbst auf „Aufgegeben“.
        l.texte_fehlen.insert((Slot::Reiseflug25, "de-DE".into()), "x".into());
        l.texte_fehlen.insert((Slot::Reiseflug25, "en-GB".into()), "x".into());
        assert!(matches!(slot_stand(&l, Slot::Reiseflug25, None, false, None).0, Stand::TextAufgegeben { .. }));
    }

    // ---- 4 ----

    /// 4: Server meldet `entfaellt` — Sprache ohne Stimme und Sicherheit ohne
    /// Vorlage werden für den Flug gestrichen, nicht nachgeholt; die Seite
    /// nennt sie. Rot auf v2.1.6: das Feld wurde nicht gelesen.
    #[test]
    fn server_entfaellt_streicht_einmal_fuer_den_flug() {
        let json = r#"{"sprachen":["sv-SE","en-GB"],"eintraege":[{"slot":"sicherheit","sprache":"en-GB","text":"S"}],
            "entfaellt":{"sicherheit/sv-SE":"keine_vorlage","sprache/zh-CN":"keine_stimme"}}"#;
        let d: api_client::KabinenDrehbuch = serde_json::from_str(json).unwrap();
        let mut l = Lauf::default();
        let fehlend = drehbuch_uebernehmen(&mut l, d, &[Slot::Sicherheit], false, false);
        assert!(fehlend.is_empty(), "Sicherheit sv-SE nicht nachholen: {fehlend:?}");
        assert!(l.ohne_stimme.contains("zh-CN") && l.ohne_vorlage.contains("sv-SE"));
        assert!(ansage_bereit(&l.texte, &[((Slot::Sicherheit, "en-GB".to_string()), ton_s(5.0))].into_iter().collect(), &l.texte_fehler, &l.texte_fehlen, &l.sprachen, Slot::Sicherheit));
        assert_eq!(arten(&l).iter().filter(|(a, _)| a == "entfaellt").count(), 2, "je Fund ein Eintrag");
        // Eine leere PHP-Liste statt Objekt bricht das Drehbuch nicht.
        let leer: api_client::KabinenDrehbuch = serde_json::from_str(r#"{"sprachen":[],"eintraege":[],"entfaellt":[]}"#).unwrap();
        assert!(leer.entfaellt.is_empty());
    }

    /// 4: Das erste „keine Stimme“ des Ton-Servers streicht die Sprache für
    /// alle Ansagen — genau eine Anfrage statt einer je Ansage (CDC 8939:
    /// 14 × 404). Englisch nie.
    #[test]
    fn erstes_keine_stimme_streicht_die_sprache_fuer_alle_ansagen() {
        let mut st = flug(Boarding, &[]);
        st.kabinen_lauf.sprachen = vec!["zh-CN".into(), "en-GB".into()];
        for s in [Slot::Boarding, Slot::Sicherheit, Slot::KapitaenBegruessung] {
            for sp in ["zh-CN", "en-GB"] {
                st.kabinen_lauf.texte.insert((s, sp.into()), "x".into());
            }
        }
        let zh = |st: &FlightStats| ton_kandidaten(st, false, Flugart::Linie, 0).iter().filter(|(_, sp, _)| sp == "zh-CN").count();
        assert_eq!(zh(&st), 3, "Vorbedingung: drei chinesische Anfragen offen");
        // Antwort auf die erste: keine Stimme (wie `ton_auftrag`).
        st.kabinen_lauf.ton.insert((Slot::Boarding, "zh-CN".into()), None);
        stimme_streichen(&mut st.kabinen_lauf, "zh-CN");
        assert_eq!(zh(&st), 0, "keine weitere Anfrage für zh-CN");
        assert!(ton_kandidaten(&st, false, Flugart::Linie, 0).iter().any(|(_, sp, _)| sp == "en-GB"), "Englisch bleibt");
        st.kabinen_lauf.ton.insert((Slot::Sicherheit, "en-GB".into()), ton_s(5.0));
        assert!(ansage_bereit(&st.kabinen_lauf.texte, &st.kabinen_lauf.ton, &st.kabinen_lauf.texte_fehler, &st.kabinen_lauf.texte_fehlen, &st.kabinen_lauf.sprachen, Slot::Sicherheit), "wartet nicht auf Chinesisch");
        stimme_streichen(&mut st.kabinen_lauf, "en-GB");
        assert!(!st.kabinen_lauf.ohne_stimme.contains("en-GB"));
        assert_eq!(arten(&st.kabinen_lauf).iter().filter(|(a, _)| a == "entfaellt").count(), 1, "einmal für den Flug");
    }
}
