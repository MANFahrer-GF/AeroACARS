//! MQTT publisher for AeroACARS — feeds the aeroacars-live monitor relay.
//!
//! ## Architecture
//!
//! - One spawned tokio task drives the rumqttc eventloop (so the connection
//!   stays alive, reconnects on failure, etc.).
//! - A second spawned task processes outgoing `Cmd`s from a bounded mpsc.
//! - The `Handle` exposed to callers is just a `Sender<Cmd>` wrapped in
//!   typed methods. All sends are non-blocking via `try_send`; if the
//!   channel is full (broker stalled), low-priority messages (position) are
//!   dropped, but high-priority ones (touchdown, pirep) block briefly.
//!
//! Topic schema mirrors `docs/topic-schema.md` of the aeroacars-live repo:
//!
//! ```text
//! aeroacars/<vaPrefix>/<pilotId>/{position,phase,touchdown,pirep,status}
//! ```
//!
//! `position`/`phase`/`status` are published with `retain=true` so a fresh
//! Monitor subscriber sees the latest known state immediately on connect.
//! `touchdown`/`pirep` are end-of-flight events; they are ALSO published with
//! `retain=true` so a recorder that is briefly offline at the moment of publish
//! still picks up the last one on reconnect. Re-delivery is safe because ingest
//! is idempotent (pireps UNIQUE(pirep_id); touchdown ts-window dedup); the next
//! flight on the pilot topic overwrites the retained value.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rumqttc::{
    AsyncClient, Event, LastWill, MqttOptions, Outgoing, Packet, QoS, Request, TlsConfiguration,
    Transport,
};
use serde::Serialize;
use sim_core::{FlightPhase, SimSnapshot, Simulator};
use tokio::sync::{mpsc, watch};
use tracing::{debug, error, info, warn};
use url::Url;

pub mod backup;
pub mod chat;
pub mod log_upload;
pub mod navdata;
pub mod provision;

const STATUS_ONLINE: &str = "online";
const STATUS_OFFLINE: &str = "offline";

/// Bounded queue between caller and MQTT-publisher task. ~5 s of position
/// ticks at the fastest cadence (5 s/tick → ~1 msg buffered on average,
/// burst tolerance of 200 msgs).
const CMD_BUFFER: usize = 200;

/// v1.5.7 (#mqtt-outage): Wartefrist beim Einreihen der EINMALIGEN
/// Ereignisse (Landung, Flugbericht, Block, Takeoff …).
///
/// Vorher 250–500 ms. Das reichte im Normalbetrieb, war aber genau dann
/// zu knapp, wenn es darauf ankam: Bei Michels Netzausfall (11.08.2026)
/// stand die Warteschlange voll mit Positionen, die Landung wartete 250 ms
/// und wurde verworfen ("dropping touchdown publish"). Ein Ereignis, das
/// pro Flug genau EINMAL entsteht, darf nicht an einer Viertelsekunde
/// scheitern — es gibt keinen zweiten Versuch.
///
/// Seit die Positionen einen eigenen Weg haben (siehe `Cmd`), ist diese
/// Schlange ohnehin fast immer leer; die längere Frist kostet im
/// Normalfall nichts und rettet den Ausnahmefall. Die Sendung läuft in
/// einer eigenen Aufgabe, blockiert also keinen Aufrufer.
const EVENT_ENQUEUE_TIMEOUT: Duration = Duration::from_secs(30);

/// v1.5.7 (QS-Runde 4): Was der Drive-Loop dem Publisher über die Leitung
/// meldet. Als eigene Funktion, damit die ZUORDNUNG prüfbar ist — die
/// Mutationsprobe hatte gezeigt, dass ein gelöschtes `link_tx.send(...)`
/// von keinem Test bemerkt wird.
///
/// EHRLICHE GRENZE: Geprüft ist damit die Zuordnung Ereignis → Zustand.
/// Dass der Aufruf an der richtigen Stelle in der Schleife steht, sichert
/// weiterhin nur das Lesen des Codes; ein echter Nachweis bräuchte einen
/// Broker im Test.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LinkEvent {
    /// Der Broker hat die Verbindung bestätigt.
    ConnAck,
    /// `poll()` meldete einen Fehler — Leitung gilt als weg.
    PollError,
    /// Der Stille-Wächter hat zugeschlagen.
    WatchdogTimeout,
    /// Alles andere (eingehende Nachrichten, Outgoing-Bestätigungen).
    Other,
}

/// `None` = kein Zustandswechsel zu melden.
fn link_state_for(event: LinkEvent) -> Option<bool> {
    match event {
        LinkEvent::ConnAck => Some(true),
        LinkEvent::PollError | LinkEvent::WatchdogTimeout => Some(false),
        LinkEvent::Other => None,
    }
}

/// v1.5.7 (#mqtt-outage, QS-Runde 3): Die Positions-Entscheidung als
/// eigene Funktion — klein, aber der Kern des Fixes und damit prüfbar.
///
/// Im Feldbefund füllten Positionen während des Ausfalls den
/// Auftragskanal von rumqttc (Kapazität 200) im 3-Sekunden-Takt wieder
/// auf. Die Landung, die danach kam, fand keinen Platz mehr und wurde
/// nach 20 s verworfen. Solange die Leitung liegt, hat eine Position dort
/// nichts verloren: Sie ist in Sekunden überholt, während die Landung
/// pro Flug genau einmal entsteht.
fn should_publish_position(link_up: bool) -> bool {
    link_up
}

/// v1.5.7 (#mqtt-outage): Wächter gegen die HALB OFFENE Verbindung.
///
/// Der zweite Befund aus Michels Flug — und der heimtückischere: Um
/// 23:51:14 kam nach dem Netzausfall ein CONNACK, 90 Sekunden später war
/// die Leitung wieder weg (Broker: "closed its connection"), und der
/// Client hat es NIE bemerkt. Fünf Stunden lang kein Fehler, kein neuer
/// Versuch, keine einzige Logzeile: `eventloop.poll()` wartete auf Daten,
/// die nie kamen. Ohne Fehler kein Neuaufbau — der klassische
/// halb-offene TCP-Fall, bei dem beide Seiten verschiedener Meinung sind.
///
/// Gegenmittel: eine Obergrenze für Stille. Bei stehender Verbindung
/// weckt spätestens der Keepalive (60 s) den Eventloop. Wenn also
/// 2,5 Keepalive-Perioden lang GAR NICHTS passiert, ist die Leitung tot,
/// egal was das Betriebssystem behauptet — dann wird sie verworfen.
///
/// EHRLICHE EINORDNUNG (QS-Befund): Das ist das ZWEITE Netz, nicht das
/// erste. rumqttc erkennt eine tote Leitung über den ausbleibenden
/// PINGRESP selbst (spätestens nach 2 Keepalive-Perioden) und liefert
/// dann einen Fehler; auf einem tatsächlich gepollten Eventloop feuert
/// dieser Wächter also nie zuerst. Er greift nur, wenn `poll()`
/// überhaupt nicht mehr zum Zug kommt — der eigentliche Fehler aus
/// Michels Flug lag genau dort (blockierendes `subscribe()`), ist aber
/// an der Wurzel behoben. Der Wächter bleibt als Auffangnetz für
/// Blockade-Fälle, die wir noch nicht kennen.
///
/// Bewusst großzügig: lieber ein paar Sekunden später neu verbinden als
/// eine gesunde Verbindung wegen einer Verkehrspause abzuräumen.
const POLL_SILENCE_TIMEOUT: Duration = Duration::from_secs(150);

/// v1.5.7: Frist für einen einzelnen Sendeversuch. `publish()` wartet
/// sonst unbegrenzt auf Platz im internen Vorrat von rumqttc — bei toter
/// Leitung also für immer, womit die gesamte Sende-Schleife stillsteht.
const PUBLISH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Debug)]
pub struct MqttConfig {
    /// e.g. `wss://live.kant.ovh/mqtt`
    pub broker_url: String,
    /// Mosquitto user — typically `pilot_<id>`.
    pub username: String,
    pub password: String,
    /// VA prefix for topic routing — `gsg` for German Sky Group.
    pub va_prefix: String,
    /// phpVMS pilot id as string — `42`.
    pub pilot_id: String,
}

impl MqttConfig {
    pub fn validate(&self) -> Result<()> {
        if self.broker_url.is_empty() {
            anyhow::bail!("broker_url is empty");
        }
        let u = Url::parse(&self.broker_url).with_context(|| "invalid broker_url")?;
        if !matches!(u.scheme(), "wss" | "ws" | "mqtts" | "mqtt" | "ssl" | "tcp") {
            anyhow::bail!("broker_url scheme {} not supported", u.scheme());
        }
        if self.username.is_empty() || self.password.is_empty() {
            anyhow::bail!("username and password must be set");
        }
        if self.va_prefix.is_empty() || self.pilot_id.is_empty() {
            anyhow::bail!("va_prefix and pilot_id must be set");
        }
        Ok(())
    }

    fn topic(&self, channel: &str) -> String {
        format!("aeroacars/{}/{}/{}", self.va_prefix, self.pilot_id, channel)
    }
}

#[derive(Clone, Debug)]
pub struct FlightMeta {
    pub callsign: String,
    pub aircraft_icao: String,
    pub dep_icao: String,
    pub arr_icao: String,
    /// v0.5.19: phpVMS-side aircraft registration ("D-ALEU"). Sent
    /// to the live-tracking server in preference to the simulator's
    /// own ATC-ID (which payware addons often set to a generic
    /// placeholder like "FFSTS"). Empty when the bid had no
    /// registration on file — falls back to the snap's value then.
    pub planned_registration: String,
    /// Spec sim-disconnect-auto-resume F4: phpVMS-PIREP-ID des
    /// aktiven Flugs. Wird in jeden Position-Payload mit eingebaut,
    /// damit `aeroacars-live` Server-Sessions ueber die `pirep_id`
    /// joinen kann statt nur ueber (callsign, dep, arr) + Zeitfenster.
    /// Loest den AUA-323-Fall: 23-Minuten-Positions-Luecke erzeugt
    /// keinen Session-Split mehr, solange der Client dieselbe
    /// `pirep_id` weiterschickt.
    pub pirep_id: String,
    /// v1.5.5 Stand-Erkennung: erkannter Abflug-/Ankunftsstand (OSM).
    /// In jedem Position-Payload, sobald bekannt — dauernd praesent
    /// heisst selbstheilend nach einem Recorder-Neustart (kein Retain
    /// noetig). None → Feld fehlt im JSON (Wire-additiv).
    pub dep_gate: Option<String>,
    pub arr_gate: Option<String>,
}

/// v0.5.14: rich position telemetry. Goal is "PIREP-grade analysis from
/// live data alone" — server can replay any flight, build approach
/// profiles, score touchdowns, audit FSM transitions, all without
/// needing the recorded JSONL. Sent every 5-30 s (phase-dependent).
///
/// Sizing: typical payload ~600-800 B JSON. At 30 s cadence in cruise
/// that's ~24 KB/h per pilot. At 5 s in approach: ~140 KB/h. Well
/// within Mosquitto+Caddy throughput on the VPS.
#[derive(Clone, Debug, Serialize)]
struct PositionPayload {
    ts: i64,
    /// Current FSM phase as label (PREFLIGHT, TAXI_OUT, TAKEOFF, CLIMB,
    /// CRUISE, HOLDING, DESCENT, APPROACH, FINAL, LANDING, TAXI_IN,
    /// ON_BLOCK). Inlined into every position so the Monitor never has
    /// to wait for a separate phase-topic delivery.
    phase: &'static str,
    /// v0.16.13: Phasen-Engine-v2-Schatten (live.kant.ovh zeigt "v2:"-Badge
    /// bei Abweichung). None solange der Client <0.16.12 ist oder die
    /// Engine noch im Warmup — skip_serializing haelt alte Payloads byte-
    /// identisch.
    #[serde(skip_serializing_if = "Option::is_none")]
    shadow_phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    shadow_segment: Option<String>,

    // ---- Position ----
    lat: f64,
    lon: f64,
    alt_ft: i32, // MSL altitude
    agl_ft: i32, // Above-ground (for approach/landing analysis)

    // ---- Attitude ----
    pitch_deg: f32,
    bank_deg: f32,
    hdg_true: i32,
    hdg_mag: i32,

    // ---- Speeds ----
    ias_kt: i32,
    tas_kt: i32,
    gs_kt: i32,
    vs_fpm: i32,
    mach: Option<f32>,

    // ---- Forces / state ----
    g_force: f32,
    on_ground: bool,
    parking_brake: bool,
    stall_warning: bool,
    overspeed_warning: bool,

    // ---- Configuration ----
    gear_position: f32,  // 0=up, 1=down
    flaps_position: f32, // 0..1
    spoilers_position: Option<f32>,
    spoilers_armed: Option<bool>,
    engines_running: u8,

    // ---- Lights ----
    // Fund 2026-07-26: SimSnapshot liest diese SimVars/Datarefs seit
    // langem korrekt (sim-msfs + sim-xplane befuellen sie), sie wurden
    // aber nie in dieses Payload aufgenommen — der Recorder-seitige
    // Procedure-Score (Beacon/Strobe/Landing-Light) hatte dadurch fleet-
    // weit NIE einen einzigen Wert (0 von 940 Sessions).
    #[serde(skip_serializing_if = "Option::is_none")]
    light_beacon: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    light_strobe: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    light_landing: Option<bool>,

    // ---- Fuel ----
    fuel_total_kg: f32,
    fuel_used_kg: f32,
    fuel_flow_kg_h: Option<f32>,
    total_weight_kg: Option<f32>,

    // ---- Environment ----
    wind_dir_deg: Option<f32>,
    wind_speed_kt: Option<f32>,
    oat_c: Option<f32>,
    qnh_hpa: Option<f32>,

    // ---- Autopilot (Boolean state) ----
    ap_master: Option<bool>,
    ap_hdg: Option<bool>,
    ap_alt: Option<bool>,
    ap_nav: Option<bool>,
    ap_app: Option<bool>,

    // ---- Identity ----
    //
    // v0.5.23: alle Identity-Felder sind jetzt Option<String> mit
    // skip_serializing_if. Hintergrund: phpVMS-API liefert manchmal leere
    // ICAO-Codes (Aircraft ohne ICAO-Feld in der DB). Wenn wir diese als
    // `""` serialisieren, ueberschreibt der Server-COALESCE-UPSERT den
    // vorher akkumulierten korrekten Wert mit "". Mit Option<String>+
    // skip_serializing_if = "Option::is_none" verschwindet das Feld
    // komplett aus dem JSON wenn leer → Server faellt sauber auf den
    // alten Wert zurueck. Fuer callsign/dep/arr aequivalent (defensive).
    #[serde(skip_serializing_if = "Option::is_none")]
    callsign: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aircraft_icao: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aircraft_registration: Option<String>,
    /// v0.8.3 (#5 follow-up): voller Sim-Aircraft-Title (z.B.
    /// "Black Square A36TC Bonanza Professional N920LG") aus
    /// `SimVar TITLE` / X-Plane `acf_descrip`. Bisher nirgends ueber
    /// MQTT publiziert — der Recorder konnte `flight_session_stats.
    /// aircraft_title` deshalb nie befuellen. Mit diesem Feld kann
    /// `recomputeSessionStats` ihn aus `flights.last_position_json`
    /// extrahieren. skip_if_none → alte Clients ohne Titel
    /// vergiften die DB nicht.
    #[serde(skip_serializing_if = "Option::is_none")]
    aircraft_title: Option<String>,
    simulator: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dep: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arr: Option<String>,
    /// Spec sim-disconnect-auto-resume F4: phpVMS-PIREP-ID — wird in
    /// jedem Position-Tick mitgesendet damit der Server-Splitter
    /// (`recorder/mqttSubscriber.ts:ensureSession`) Sessions ueber
    /// `pirep_id` joinen kann. Pre-MVP-Sessions ohne `pirep_id` im
    /// Payload fallen weiter in den Standard-Pfad (callsign/dep/arr
    /// + Zeitfenster) — forward-only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pirep_id: Option<String>,
    /// v0.5.24: Client-Version damit der aeroacars-live-Monitor sieht
    /// welcher Pilot mit welcher Build-Version sendet. Ermöglicht
    /// Version-Compliance-Tracking (= "Pilot X läuft noch v0.5.16-Pre-
    /// Numeric-Fix, Hard-Landing-Check failed silent").
    /// v1.5.5 Stand-Erkennung: erkannte Staende, sobald bekannt.
    /// skip_serializing haelt Payloads ohne Stand byte-identisch.
    #[serde(skip_serializing_if = "Option::is_none")]
    dep_gate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arr_gate: Option<String>,
    client_version: &'static str,
}

/// Convert empty/whitespace-only strings to None — used at the JSON-edge
/// to keep payloads clean of "" values that would muddy the server side.
fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

#[derive(Clone, Debug, Serialize)]
struct PhasePayload {
    ts: i64,
    phase: &'static str,
}

/// v0.5.14: authoritative block snapshot. Fires once when the FSM
/// transitions Preflight/Boarding → Pushback/TaxiOut (= block-off
/// is stamped). Carries fuel + planned-OFP values that are STABLE
/// at this point — `position` payloads during PREFLIGHT show LIVE
/// fuel which can still be loading and is NOT authoritative.
#[derive(Clone, Debug, Serialize)]
pub struct BlockPayload {
    pub ts: i64,
    pub block_fuel_kg: Option<f32>,
    pub planned_block_fuel_kg: Option<f32>,
    pub planned_burn_kg: Option<f32>,
    pub planned_reserve_kg: Option<f32>,
    pub planned_zfw_kg: Option<f32>,
    pub planned_tow_kg: Option<f32>,
    pub planned_ldw_kg: Option<f32>,
    pub planned_max_zfw_kg: Option<f32>,
    pub planned_max_tow_kg: Option<f32>,
    pub planned_max_ldw_kg: Option<f32>,
    pub planned_route: Option<String>,
    pub planned_alternate: Option<String>,
    pub dep_gate: Option<String>,
    pub dep_metar: Option<String>,
}

/// v0.5.14: takeoff snapshot. Fires once when the FSM stamps
/// `takeoff_at` (= aircraft has left the ground). Authoritative
/// TOW + fuel-at-takeoff values for fuel-burn / overweight analysis.
#[derive(Clone, Debug, Serialize)]
pub struct TakeoffPayload {
    pub ts: i64,
    pub takeoff_weight_kg: Option<f32>,
    pub takeoff_fuel_kg: Option<f32>,
    pub takeoff_lat: Option<f64>,
    pub takeoff_lon: Option<f64>,
    pub dep_metar: Option<String>,
    pub dep_runway: Option<String>,
}

/// Alles, was eine **spaet eingetroffene Szenerie** noch veraendern kann.
///
/// # Warum diese Gruppe eigen ist
///
/// Trifft die Bahndefinition des Simulators erst nach dem Aufsetzen ein
/// (YSBK, 01.09.2026: 444 Rollwege, die Lieferung lief noch), holt der
/// Client die Zuordnung nach. Damit aendern sich nicht nur die
/// Bahnmasse, sondern alles, was aus der Bahn folgt: die Herkunft, die
/// Korrekturbetraege, die Aufsetzpunkt-Einordnung und die
/// Schwellen-Ueberquerung.
///
/// Der Recorder uebernimmt vom spaeteren PIREP nur Punktzahl und Noten.
/// Die Rohwerte dieser Zeile kommen ausschliesslich aus den beiden
/// Touchdown-Ereignissen — trug der Nachtrag sie nicht mit, blieben sie
/// fuer immer auf dem Stand vor der Szenerie.
///
/// # Warum EIN Typ und nicht achtundzwanzig Zeilen
///
/// Dieselbe Begruendung wie bei [`BahnWire`]: Zwei Ereignisse, die
/// dieselben Felder je einzeln aufzaehlen, laufen auseinander — beim
/// ersten Nachtrag trugen sie genau vier gemeinsame Felder von
/// achtundzwanzig. `flatten` legt sie auf der Leitung flach ab; der
/// Server sieht keinen Unterschied zu vorher.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct BahnHerkunftWire {
    // ⚠ KEIN `skip_serializing_if` in dieser Gruppe.
    //
    // Die Felder muessen auch `null` SAGEN koennen. Der Recorder
    // aktualisiert nur Schluessel, die im Ereignis vorkommen — ein
    // fehlender Schluessel laesst den alten Wert stehen. Wird ein Wert
    // durch die spaetere Zuordnung ungueltig (etwa ein Korrekturbetrag,
    // weil jetzt gar nichts mehr uebernommen wurde, oder eine
    // Navdaten-Bewertung, die ohne Bahntreffer entfaellt), dann ist
    // "weglassen" genau die falsche Auskunft: Der Recorder behielte den
    // Wert von vor der Korrektur.
    //
    // `json_patch` (RFC 7396) loescht bei `null` den Schluessel; der
    // Recorder faengt das ab und setzt ihn ausdruecklich auf JSON-Null.
    // "Gemessen, aber unbekannt" bleibt damit von "nie gemessen"
    // unterscheidbar.
    /// Die wievielte Zuordnung der Bahn diese Werte sind.
    ///
    /// # Wozu eine Zahl noetig ist
    ///
    /// Der Client kann denselben Touchdown MEHRFACH nachtragen: einmal
    /// zum Aufsetzen und noch einmal, wenn eine neuere Lieferung
    /// desselben Platzes eintrifft. Beide Ereignisse adressieren
    /// dieselbe Zeile ueber `pirep_id` + `touchdown_at`.
    ///
    /// MQTT garantiert die Reihenfolge nur je Verbindung; ein
    /// Wiederverbinden mitten im Nachtrag kann ein aelteres Ereignis
    /// hinter ein neueres schieben. Ohne diese Zahl hat der Empfaenger
    /// kein Mittel, das zu erkennen — er wuerde die gute Zuordnung mit
    /// der alten ueberschreiben und es saehe aus wie ein Rueckschritt
    /// ohne Ursache.
    ///
    /// ⚠ Es ist ausdruecklich NICHT die SimConnect-Anfragenummer der
    /// Szenerie-Abfrage. Die ist prozesslokal und faengt nach einem
    /// App-Neustart wieder bei null an: Der Recorder haette Revision 7
    /// gespeichert, bekaeme die tatsaechlich neuere Lieferung als 1 und
    /// wiese sie ab. Es ist eine mit dem Flug PERSISTIERTE Revision, die
    /// ueber Neustarts hinweg monoton waechst.
    ///
    /// Eine Zeile darf nur von einem Ereignis mit **groesserer oder
    /// gleicher** Revision ueberschrieben werden. Fehlt sie (Client vor
    /// v1.7.15), zaehlt sie als 0 — ein Altclient-Nachtrag kann damit
    /// keine neuere Zuordnung mehr ueberschreiben.
    #[serde(default)]
    pub bahn_revision: Option<u32>,
    /// Die beim Recorder liegende Spur ist gegen eine FRUEHERE Bahn
    /// projiziert: Nach dem Ausrollen hat die Bahn die Achse gewechselt,
    /// der Client sendet die Spur seitdem nicht mehr, die alte bleibt in
    /// der Zeile stehen — neben Herkunftswerten der neuen Bahn. Die
    /// Anzeige kann sie damit ausgrauen statt sie als Messung der neuen
    /// Bahn zu zeigen (Client-QS Runde 6, Befund 3). `false` = die Spur
    /// gehoert zu dieser Bahn.
    #[serde(default)]
    pub bahn_spur_veraltet: Option<bool>,
    /// True if a runway was correlated from the touchdown coord (OurAirports CSV).
    pub runway_match_icao: Option<String>,
    pub runway_match_ident: Option<String>,
    pub runway_match_distance_m: Option<f32>,
    pub runway_match_centerline_offset_m: Option<f32>,
    /// v0.5.22: total length of the matched runway in metres (from the
    /// OurAirports CSV row). Required server-side for the "Bahn-Auslastung"
    /// sub-score (rollout / length × 100) so the live monitor can show
    /// the same breakdown the AeroACARS app shows pilots in-flight.
    pub runway_length_m: Option<f32>,

    // ─── v1.7.8 Bahngeometrie aus der Simulator-Szenerie ─────────────
    //
    // Die Bahn, gegen die gemessen wurde, kommt bei X-Plane aus der
    // installierten Szenerie des Piloten statt aus den Navdaten. Diese
    // drei Felder halten fest, OB und WIE STARK das gewirkt hat —
    // damit sich der Quellenwechsel im Bestand messen laesst statt ihn
    // zu glauben.
    //
    // Grund: 3.836 Bahnen des neuesten AIRAC-Zyklus fuehren
    // `true_course` als 0,0 oder 360,0; bei 3.329 davon widerspricht das
    // der eigenen Bahnnummer.
    /// Woher die Bahngeometrie stammt: `"szenerie"` oder `"navdaten"`.
    #[serde(default)]
    pub bahn_geometrie_quelle: Option<String>,
    /// Was aus der Szenerie-Abfrage wurde — feiner als
    /// `bahn_geometrie_quelle`, das nur "szenerie" oder "navdaten" kennt.
    /// Werte: nicht_angefordert | abgelehnt | keine_antwort | ohne_bahnen
    /// | kein_treffer | geliefert | uebernommen | keine_szenerie
    pub bahn_szenerie_status: Option<String>,
    /// Name + Version, mit denen sich der Simulator gemeldet hat.
    /// Trennt MSFS 2020 von 2024 — die Feldnamen der Facility-Abfrage
    /// stammen aus der 2024er-SDK-Doku.
    pub sim_kennung: Option<String>,
    /// Um wie viel Grad der Kurs korrigiert wurde. 0 = kein Unterschied.
    #[serde(default)]
    pub bahn_kurs_korrektur_grad: Option<f64>,
    /// Um wie viel Meter die Breite korrigiert wurde.
    #[serde(default)]
    pub bahn_breiten_korrektur_m: Option<f64>,
    /// Um wie viel Meter die versetzte Schwelle korrigiert wurde.
    ///
    /// ⚠ Der Nullpunkt der Aufsetzpunkt-Bewertung. Ein grosser Wert
    /// heisst nicht "Fehler", sondern "Szenerie und Navdaten sind sich
    /// hier uneins" — und dass wir der Szenerie gefolgt sind, weil der
    /// Pilot dort landet.
    #[serde(default)]
    pub bahn_schwellen_korrektur_m: Option<f64>,

    // ─── v0.8.0 VPS-Navdata + Runway-Awareness ────────────────────────
    //
    // Identische Felder wie in `storage::LandingRecord`. Alle
    // skip_if_none damit Recorder + Webapp die Felder nur sehen wenn
    // tatsächlich gegen VPS-Navdata bewertet wurde — pre-v0.8.0
    // Touchdowns kommen ohne diese Felder durch und der MQTT-Consumer
    // muss nichts ändern.
    /// "navigraph" | "ourairports_fallback". Welche Quelle die
    /// Runway-Match-Daten geliefert hat.
    pub navdata_source: Option<String>,
    /// AIRAC-Cycle der genutzten Navigraph-Daten (e.g. "2604"). None
    /// wenn navdata_source = "ourairports_fallback".
    pub navdata_cycle: Option<String>,
    /// True-course der Landerichtung in deg. Webapp braucht das fuer
    /// die RunwayDiagram-Achse.
    pub runway_true_course_deg: Option<f64>,
    /// Displaced-Threshold in ft (0 = keine).
    pub runway_displaced_threshold_ft: Option<i32>,
    /// Erwartete Threshold-Crossing-Height in ft (typisch 49-55).
    pub runway_tch_expected_ft: Option<i32>,
    /// Veröffentlichter Glideslope-Winkel in Grad (typisch 3.0).
    pub runway_glideslope_angle_deg: Option<f64>,
    /// Signed along-track-Distanz vom Landing-Threshold zum Touchdown,
    /// in Metern. Positiv = past, negativ = undershoot.
    pub td_distance_from_threshold_m: Option<f64>,
    /// F3 TDZ-Result: true wenn Touchdown im TDZ-Marker. None bei
    /// runways < 1200 m.
    pub td_in_tdz: Option<bool>,
    /// 1-indexed third of the runway the touchdown lies in (1/2/3).
    /// Stable wire-key gegen storage::LandingRecord — Webapp + Pilot-
    /// Client teilen die Frontend-Logik.
    pub td_third: Option<u8>,
    /// F3 TDZ-Marker-Laenge in Metern (≤ 900, ≤ length/3).
    pub td_tdz_length_m: Option<f64>,
    /// F4 Aim-Point delta in Metern (positiv = past, negativ = short).
    pub aim_delta_m: Option<f64>,
    /// F4 Aim-Point classification: "perfect" | "short_of_aim" |
    /// "past_aim" | "long_landing" | "severe".
    pub aim_class: Option<String>,
    /// F4 Aim-Point distance from threshold in Metern (300 oder 400).
    pub aim_point_m: Option<f64>,
    /// F5 actual TCH (AGL ft beim Threshold-Crossing).
    pub tch_actual_ft: Option<f64>,
    /// F5 TCH delta = actual - expected (ft). Positiv = ueber Profil.
    pub tch_delta_ft: Option<f64>,
    /// F5 TCH classification.
    pub tch_class: Option<String>,
    /// F6 Displaced-Threshold-Warning: Touchdown im Pre-Threshold-Paint.
    pub pre_displaced_threshold: Option<bool>,
}

#[derive(Default, Clone, Debug, Serialize)]
pub struct TouchdownPayload {
    pub ts: i64,
    /// v0.7.19 (QS-R2 Finding 1): PIREP-ID damit Korrektur-Events
    /// (TouchdownAccidentOverride) den exakten Touchdown-Row in der
    /// Webapp-DB targeten koennen. `skip_serializing_if=None` damit
    /// hypothetische Schema-Migrationen tolerant bleiben.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pirep_id: Option<String>,
    /// v0.11.1: Pilot-Client-Version aus `CARGO_PKG_VERSION`. Mirror
    /// vom FlightMeta-Feld, hier zusaetzlich im Touchdown-Payload damit
    /// die Webapp-Reports-Liste + Landing-Analysis-Header die Version
    /// direkt aus jeder Touchdown-Row anzeigen koennen (statt sie ueber
    /// die separate FlightMeta-Connect-Message zu joinen). Schlankerer
    /// Datenfluss + sichtbar auch fuer historische PIREPs sobald ein
    /// Pilot mit v0.11.1+ einen neuen Flug einreicht.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<&'static str>,
    pub vs_fpm: i32,
    pub ias_kt: i32,
    pub gs_kt: Option<i32>,
    pub pitch_deg: Option<f32>,
    pub bank_deg: Option<f32>,
    pub g_load: Option<f32>,
    /// Roher 50-Hz-Einzelframe-G-Peak. **Bleibt roh** (v0.12.3 LE7) —
    /// backward-kompatibel; alte Consumer lesen weiter diesen Wert.
    pub peak_g_load: Option<f32>,
    /// v0.12.3 (LE7): EMA-geglätteter Fenster-Peak (FOQA-Methode) — der
    /// gescorte G-Wert. Additiv; `skip_serializing_if`-frei, damit der
    /// Recorder das Feld zuverlässig sieht. Pre-v0.12.3-Payloads ohne
    /// das Feld deserialisieren via `serde(default)` zu `None`.
    #[serde(default)]
    pub scored_g_load: Option<f32>,
    /// v0.12.3 (LE8): `"ema_max"` | `"raw_fallback"` — wie `scored_g_load`
    /// abgeleitet wurde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scored_g_method: Option<String>,
    pub sideslip_deg: Option<f32>,
    pub headwind_kt: Option<f32>,
    pub crosswind_kt: Option<f32>,
    pub score: Option<i32>,
    /// v0.20.0: Klasse und Note zum `score` — EINGEFROREN, nicht ableitbar.
    ///
    /// Vorher trug `score` die diskrete Touchdown-Klasse (100/80/60/30/0) und
    /// die Webapp leitete das Label mit einer EIGENEN Schwellen-Leiter daraus
    /// ab (90/70/45/15). Seit `score` die echte Gesamtbewertung traegt, waere
    /// das eine zweite Wahrheit: bei 89 Punkten sagt der Client "smooth"
    /// (>= 88), die Webapp-Leiter aber "Acceptable" (< 90). Dieselbe Landung,
    /// zwei Urteile — genau die Krankheit aus PIA3452.
    ///
    /// Die Regel darf nicht zweimal existieren. Der Client klassifiziert, die
    /// Webapp zeigt an. Additiv + `serde(default)`: Alt-Payloads ohne die
    /// Felder deserialisieren zu `None`, die Webapp faellt dann auf ihre
    /// Legacy-Leiter zurueck (die fuer die alten diskreten Werte stimmt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_grade: Option<String>,
    pub bounce: Option<bool>,
    pub bounce_count: Option<u8>,
    /// v0.8.3 (#8): Forensisch erkannte Hopser >= 5 ft AGL (
    /// `touchdown_v2::BOUNCE_FORENSIC_MIN_AGL_FT`). Wird unabhaengig
    /// vom Score gezaehlt — auch „kleine" Hopser (5-14 ft), die per
    /// Spec score-frei sind, tauchen hier auf. Wenn `Some(0)` und
    /// `bounce_count > 0`: alle Hopser sind ueber 15 ft (scored).
    /// Wenn `Some(n)` und `bounce_count = 0`: ausschliesslich
    /// score-freie Hopser. None = pre-v0.8.3 PIREP / Sampler-Buffer
    /// unvollstaendig.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forensic_bounce_count: Option<u8>,
    /// v0.8.3 (#8): Score-relevante Hopser >= 15 ft AGL (
    /// `touchdown_v2::BOUNCE_SCORED_MIN_AGL_FT`). Subset von
    /// `forensic_bounce_count`. Was in den Landing-Score-Sub-Score
    /// „bounces" einfliesst — ueber `scored_bounce_count_for_score()`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scored_bounce_count: Option<u8>,
    pub runway: Option<String>,
    /// v0.7.18 (B-012): aufgeloester Touchdown-Airport.
    /// - Wenn `runway_match` zur runway korreliert wurde: dessen ICAO.
    /// - Sonst der nächste Airport innerhalb 25 nmi.
    /// - Sonst fallback auf `flight.arr_airport`.
    /// Vor v0.7.18 wurde immer `flight.arr_airport` gesetzt — Off-airport-
    /// Crashes wurden so faelschlich als "Landung bei planned ICAO"
    /// angezeigt (GAF-152 Ostsee-Crash → "EDDB").
    pub airport: Option<String>,
    /// v0.7.18 (B-012): wie der Airport aufgeloest wurde.
    /// Werte: "runway_match" / "nearest_25nm" / "planned_fallback".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub airport_source: Option<String>,
    /// v0.7.18 (B-012): Distanz vom TD-Punkt zur geplanten Destination (nmi).
    /// 0 wenn Landung am geplanten Airport, > 0 bei Divert oder Off-airport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub airport_distance_to_destination_nm: Option<f32>,
    /// v0.7.18 (B-012): Distanz vom TD-Punkt zum nearest Airport (nmi).
    /// Nur gesetzt wenn `airport_source == "nearest_25nm"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub airport_nearest_distance_nm: Option<f32>,
    /// v0.7.18 (B-012, R1-4): geplante Destination aus dem Bid. Webapp
    /// braucht das um den Off-airport-Banner zu rendern — `airport` ist
    /// schon der RESOLVED-Wert und stimmt bei Divert/Off-airport NICHT
    /// mit der Plan-Destination ueberein.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_arr_airport: Option<String>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub heading_true_deg: Option<f32>,
    pub heading_mag_deg: Option<f32>,
    pub landing_weight_kg: Option<f32>,
    pub landing_fuel_kg: Option<f32>,
    pub rollout_distance_m: Option<f32>,
    /// V/S standard deviation over the approach window (fpm) — lower = more stable.
    pub approach_vs_stddev_fpm: Option<f32>,
    /// Bank-angle standard deviation over the approach window (deg).
    pub approach_bank_stddev_deg: Option<f32>,
    pub go_around_count: Option<u32>,
    pub arr_metar: Option<String>,
    /// Prozentuale Abweichung des **tatsächlichen Trip-Burn**
    /// (`takeoff_fuel − landing_fuel`) vom geplanten OFP-Trip-Burn
    /// (`planned_burn_kg`). Positiv = Mehrverbrauch, negativ =
    /// Minderverbrauch. **Nicht** block-fuel-basiert (kein Taxi-out-Sprit).
    /// None, wenn der Bid kein SimBrief-OFP hatte (planned-burn fehlt).
    ///
    /// v0.12.4 (Spec docs/spec/v0.12.4-score-consistency.md, LE5): die
    /// Berechnungsbasis wurde von `block_fuel − landing_fuel` (inkl. Taxi-
    /// out, bis v0.12.3) auf den Trip-Burn korrigiert — jetzt identisch zu
    /// `LandingRecord.fuel_efficiency_pct` und `sub_scores[fuel]`.
    pub fuel_efficiency_pct: Option<f32>,
    // v0.7.17 (B-015d): OFP-Plan-Werte mitschicken damit die Webapp
    // den Loadsheet-Sub-Score genauso berechnen kann wie der Pilot-
    // Client (sub_loadsheet erwartet ZFW + TOW). Ohne diese Felder
    // zeigte die Webapp 6 Sub-Scores (kein Loadsheet) waehrend der
    // Pilot-Client 7 zeigte → unterschiedliche Master-Scores fuer
    // denselben Flug (Tester-Befund EIN799 2026-05-12).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_zfw_kg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_tow_kg: Option<f32>,
    // ─── v0.5.23 Touchdown-Forensik ──────────────────────────────────
    //
    // Der Client berechnet bei jeder Landung BEIDE Schaetzer (Lua-30-
    // Sample fuer X-Plane, Time-Tier fuer MSFS) parallel — vorher haben
    // wir nur den finalen Wert publiziert. Mit diesen Feldern kann der
    // Server-seitige Forensik-Workflow vergleichen wie weit die beiden
    // Algorithmen auseinanderlagen + welcher Pfad gewonnen hat. Werte
    // sind Option<...> mit skip_serializing_if damit alte Pilot-Clients
    // (v0.5.22-) ohne diese Daten weiter funktionieren.
    /// "msfs" / "xplane" / "other" — welcher Sim-Adapter den Snapshot
    /// generiert hat. Identisch zum bestehenden simulator-Feld im
    /// position-Payload, hier zusaetzlich ans Touchdown gepinnt damit
    /// die Server-touchdowns-Tabelle ohne JOIN filtern kann.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simulator: Option<String>,
    /// Lua-Style 30-Sample-AGL-Δ-Schaetzung (Volanta/LandingRate-1-aligned).
    /// Primaer fuer X-Plane, fuer MSFS als Vergleichswert mitgeschickt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_estimate_xp_fpm: Option<i32>,
    /// Time-Tier-AGL-Δ-Schaetzung (750ms/1s/1.5s/2s/3s/12s window-progression).
    /// Fallback fuer MSFS, fuer X-Plane als Vergleichswert mitgeschickt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_estimate_msfs_fpm: Option<i32>,
    /// Welcher Pfad hat den finalen `vs_fpm` geliefert? Werte:
    /// "msfs_simvar_latched" — PLANE TOUCHDOWN NORMAL VELOCITY direkt
    /// "agl_estimate_msfs" — Time-Tier-Schaetzer
    /// "agl_estimate_xp" — Lua-30-Sample-Schaetzer
    /// "sampler_gear_force" — X-Plane Gear-Sampler (50Hz)
    /// "buffer_min" — Buffer-Window-Scan (Last-Resort)
    /// "low_agl_vs_min" — Approach-Tracker-Fallback
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_source: Option<String>,
    /// X-Plane Gear-Sampler peak gear_normal_force_n im Touchdown-Frame.
    /// Liefert MSFS nicht (= None auf MSFS).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gear_force_peak_n: Option<f32>,
    /// Lua-Style-Schaetzer adaptive Window-Groesse in ms (= je nach
    /// Sample-Density 500-3000 ms typisch). None wenn der Pfad nicht
    /// gewonnen hat oder keine Samples vorhanden waren.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_window_ms: Option<i32>,
    /// Wieviele Samples lagen im Berechnungs-Fenster. <10 = sparsam =
    /// niedrige Konfidenz.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimate_sample_count: Option<u32>,
    // ─── v0.5.25 Approach-Stability v2 ────────────────────────────────
    //
    // Stable-Approach-Gate-konformes Stability-Maß (FAA AC 120-71B /
    // EASA SUPP-32). Window: AGL ≤ 1000 ft. Filter: Vector-Window
    // ausgeklammert. Ground-truth: Glide-Slope-Deviation statt
    // statistische Variance.
    /// Mittlere |actual_vs − target_vs(3°)| im 1000-ft-Gate.
    /// 0 fpm = perfekt, > 200 fpm = unstabil.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_vs_deviation_fpm: Option<f32>,
    /// Maximale Deviation unter 500 ft AGL — kritischste Phase.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_max_vs_deviation_below_500_fpm: Option<f32>,
    /// Bank-Stddev im 1000-ft-Gate, gefiltert (Vector-Windows weg).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_bank_stddev_filtered_deg: Option<f32>,
    /// True wenn unter 1500 ft AGL ATC-RWY-Wechsel beobachtet.
    /// Auf der Webapp-Seite Hinweis-Pill, Score wird neutral-justiert.
    #[serde(skip_serializing_if = "is_false")]
    pub approach_runway_changed_late: bool,
    /// Stable-Approach-Gate-Indikator: bei 1000 ft AGL erreicht?
    /// (= vs_deviation < 200 fpm AND mean_bank < 5°)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_stable_at_gate: Option<bool>,
    /// Sample-Count im 1000-ft-Window (Konfidenz-Indikator).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_window_sample_count: Option<u32>,
    /// V/S-Jerk: mean |Δvs| sample-to-sample im Gate. Sim-/Aircraft-
    /// agnostic (= jet, turboprop, GA gleichermassen). PRIMAERER
    /// Stabilitaets-Indikator. < 100 fpm/tick = stable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_vs_jerk_fpm: Option<f32>,
    /// IAS-Stddev im Gate-Window. Speed-Stability.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_ias_stddev_kt: Option<f32>,
    /// Excessive Sink: ≥1 Sample mit V/S < -1000 fpm im Gate.
    /// FAA Sink-Rate-Limit. Auto-Fail-Indikator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_excessive_sink: Option<bool>,
    /// Gear+Flaps am Gate-Eintritt in Landing-Konfig
    /// (Gear≥99% AND Flaps≥70%).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_stable_config: Option<bool>,
    /// HAT (Height Above Touchdown) statt AGL als Window-Filter genutzt.
    /// True = arr_airport_elevation_ft bekannt → Mountain-Airport-tauglich.
    /// False = AGL-Fallback (= im Tal-Anflug ueber Berge ggf. ungenau).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_used_hat: Option<bool>,
    // ─── v0.5.26 Erweiterte Landung-Metriken ──────────────────────────
    /// Wing-Strike-Severity: |bank_at_td| / aircraft_max_bank_deg × 100%.
    /// 0% = wings level, 100% = am Limit. Aircraft-spezifisch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_wing_strike_severity_pct: Option<f32>,
    /// Distanz Threshold→Touchdown in Metern. Long-Landing-Indikator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_float_distance_m: Option<f32>,
    /// Touchdown-Zone (1/2/3 nach FAA: erstes/zweites/drittes Drittel).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_touchdown_zone: Option<u8>,
    /// IAS-am-TD − Vref (positiv = zu schnell, negativ = zu langsam).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_vref_deviation_kt: Option<f32>,
    /// Vref-Quelle: "pmdg" / "icao_default" / "unknown".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_vref_source: Option<String>,
    /// Stable-Approach bei DA (= 200 ft AGL/HAT). Strenger als 1000-ft-Gate.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_stable_at_da: Option<bool>,
    /// Anzahl Stall-Warning-Samples im Approach-Buffer.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approach_stall_warning_count: Option<u32>,
    /// Yaw-Rate am Touchdown (deg/sec). Hoch = Ground-Loop-Risk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_yaw_rate_deg_per_sec: Option<f32>,
    /// Brake-Energy-Proxy in kJ/m. Hoch = brake-pack-thermal-stress.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_brake_energy_proxy: Option<f32>,

    // ─── v0.5.39 50-Hz-Forensik aus TouchdownWindow-Buffer ────────────
    //
    // Berechnet vom compute_landing_analysis() ueber das 5s-pre + 10s-post
    // Sample-Buffer rund um den TD-Edge. Adressiert die Volanta-/DLHv-
    // Diskrepanz: Beide Tools nehmen smoothed VS (250-1500 ms-Mittel) und
    // peak G ueber post-TD-Window — AeroACARS war bisher auf das einzelne
    // SimVar-Latched VS angewiesen, das im Fenix-A321-Fall um Faktor 2-3
    // abweichen kann. Mit diesen Feldern kann der VA-Owner im Touchdown-
    // Detail-Modal direkt sehen welcher Wert mit welcher Methode rauskommt.
    /// VS linear interpoliert auf den exakten on_ground-Edge (zwischen
    /// den zwei umschliessenden 20-ms-Samples).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_at_edge_fpm: Option<f32>,
    /// v1.6.3: welche Quelle die bewertete Sinkrate geliefert hat —
    /// `hoehenkurve` oder `simvar_fallback`. Zusammen mit den beiden
    /// Rohwerten darunter macht das die Umstellung im Feld nachrechenbar,
    /// per Datenbank-Abfrage statt per Log-Durchsuchung. Der Anlass: die
    /// Vorgaenger-Korrektur lag zwei Monate wirkungslos im Code, weil
    /// niemand es messen konnte.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_at_edge_quelle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_geometrie_fpm: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_simvar_edge_fpm: Option<f32>,
    /// v1.6.9 — woraus die gemessene Sinkrate besteht. Es gilt
    /// `vs_at_edge_fpm = vs_eigensinken_fpm + vs_gelaende_fpm`.
    ///
    /// `vs_gelaende_fpm` ist der BEITRAG des Gelaendes zur gemessenen
    /// Zahl: negativ, wenn der Boden dem Flugzeug entgegensteigt und die
    /// Landung dadurch haerter aussieht, als sie geflogen wurde.
    /// Gemessen ueber 818 Landungen: Median 32 fpm Betrag, 12 % ueber
    /// 100 fpm, Extremfall 451 fpm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_gelaende_fpm: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_eigensinken_fpm: Option<f32>,
    /// v1.6.9 — was der Simulator selbst als Aufsetzgeschwindigkeit
    /// meldet (MSFS `PLANE TOUCHDOWN NORMAL VELOCITY`). Nur Vergleich,
    /// keine Bewertung. X-Plane: None.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_sim_referenz_fpm: Option<f32>,
    /// Mean VS ueber 250 ms vor Edge (airborne-Samples).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_smoothed_250ms_fpm: Option<f32>,
    /// Mean VS ueber 500 ms vor Edge (= Volanta-Style).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_smoothed_500ms_fpm: Option<f32>,
    /// Mean VS ueber 1000 ms vor Edge (= DLHv-Style).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_smoothed_1000ms_fpm: Option<f32>,
    /// Mean VS ueber 1500 ms vor Edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_smoothed_1500ms_fpm: Option<f32>,
    /// Peak G ueber 500 ms post-Edge — der echte Gear-Compression-Spike.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_g_post_500ms: Option<f32>,
    /// Peak G ueber 1000 ms post-Edge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_g_post_1000ms: Option<f32>,
    /// v0.7.17 (B-009): G-Force-Forensik (analog vs_smoothed_*).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g_at_edge: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g_smoothed_250ms_post: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g_median_post_500ms: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub g_p95_post_500ms: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_gear_force_n: Option<f32>,
    /// Steepste Sinkrate in [-2000, -100] ms vor Edge — Pre-Flare.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_vs_pre_flare_fpm: Option<f32>,
    /// VS unmittelbar vor Edge (ts ~ -100 ms).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs_at_flare_end_fpm: Option<f32>,
    /// Reduktion durch Flare: vs_at_flare_end - peak_vs_pre_flare.
    /// Positiv = Flare hat Sinkrate verkleinert.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare_reduction_fpm: Option<f32>,
    /// dVS/dt im Flare-Window (fpm pro Sekunde).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare_dvs_dt_fpm_per_sec: Option<f32>,
    /// Flare-Score 0..100. 100 = >400 fpm Reduktion + sanfter Endwert,
    /// 0 = keine Reduktion (Pilot zog zu spaet oder gar nicht).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare_quality_score: Option<i32>,
    /// True wenn signifikante VS-Reduktion (>50 fpm) im Flare-Window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flare_detected: Option<bool>,
    /// Bounce-Hoehe (max AGL ueber alle Excursionen post-TD, >5 ft Filter).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounce_max_agl_ft: Option<f32>,
    /// Anzahl Samples im 50-Hz-Buffer (5 s pre + 10 s post). >500 = OK.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forensic_sample_count: Option<u32>,

    // ─── v0.7.6 P1-3: Runway-Geometry-Trust ──────────────────────────
    // Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-3.
    // Bei trusted=false setzt der Tauri-Client `landing_touchdown_zone`
    // auf None, behaelt aber `landing_float_distance_m` als Raw-Wert
    // im Payload (interne Diagnostik). Web blendet beide Felder im
    // UI aus und zeigt einen Hinweis-Pill mit `runway_geometry_reason`.
    /// Ist die Runway-Geometrie plausibel? Siehe `PirepPayload` fuer
    /// die ausfuehrliche Definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runway_geometry_trusted: Option<bool>,
    /// "icao_mismatch" / "centerline_offset_too_large" / "negative_float_distance"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runway_geometry_reason: Option<String>,

    // ─── v0.7.19 GAF-707 Accident-Detection ──────────────────────────
    //
    // Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md.
    //
    // `accident_classifier_version` ist der Sentinel: v0.7.19+ setzt
    // ihn IMMER (auch bei `accident=false`/None), damit die Webapp
    // "Classifier lief, kein Accident" von "historischer Payload"
    // unterscheiden kann. Pre-v0.7.19-Payloads haben das Feld nicht
    // → Webapp/VPS klassifiziert nach.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_classifier_version: Option<String>,
    /// True wenn Confirmed Accident. Suspected wird NICHT als true
    /// gesetzt; stattdessen liefert `accident_confidence="medium"`
    /// das Suspected-Signal. None bei pre-v0.7.19 oder unklassifiziert.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident: Option<bool>,
    /// "sim_crash" | "impact" | "off_airport_impact". None wenn kein
    /// Accident.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_kind: Option<String>,
    /// "high" | "medium". `high`=Confirmed, `medium`=Suspected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_confidence: Option<String>,
    /// Begruendungs-Strings, free-form lesbar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_reasons: Option<Vec<String>>,
    /// Wann der Accident detektiert wurde. Sim-Event-Pfad: kann
    /// mehrere Sekunden vor `ts` liegen (mid-air Crash). Heuristik-
    /// Pfad: gleich `ts`. None wenn kein Accident.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_at: Option<i64>,

    /// v0.10.0 (#runway-utilization-score) — Algorithmus-Version des im
    /// PIREP gespeicherten `sub_scores`-Arrays. None/Some(1) = pre-v0.10
    /// (meter-only Bahn-Auslastung); Some(2) = v0.10 (LDA-basierter
    /// Runway-Utilization-Score); Some(3) = v0.12.0 (Float-Toleranz-
    /// Refinement); Some(4) = v0.16.21 (MSFS touchdown V/S SimVar-lag
    /// corrected); Some(5) = v0.20.x (Bahnauslastung-QS: Float-Toleranz
    /// 15→20 % LDA, Banding 30/50/70/90 → 40/60/80/95; Sinkraten-Score
    /// auf Ziel-Korridor 90-250 fpm umgestellt). Renderer rendert die
    /// neuen Felder (`extra`, neue Rationale-Keys, neue Warning-Werte)
    /// nur für v2. Spec: docs/spec/v0.10.0-runway-utilization-score.md
    /// LE11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_algorithm_version: Option<u8>,

    // ── v1.7.0 Bahndisziplin ─────────────────────────────────────────
    //
    // Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.1, Vertrag:
    // `docs/spec/runway-diagram-v2.contract.md`.
    //
    // Ohne diese Felder auf der Leitung zeigt die Webapp fuer JEDE
    // Landung „fuer diesen Flug nicht erfasst" — der Pilot-Client hat die
    // Werte, der Server sieht sie nie. Genau das war der Zustand, bis
    // Schritt 10 sie hier eingetragen hat.
    //
    // # Warum EIN Feld und nicht dreizehn
    //
    // Es gibt zwei Stellen im Client, die einen `TouchdownPayload` bauen.
    // Dreizehn Einzelfelder heisst dreizehn Zeilen an jeder der beiden —
    // und irgendwann eine Zeile, die nur an einer Stelle nachgezogen
    // wird. Das ist die Fehlerklasse, an der die Bahnmathematik schon
    // viermal auseinandergelaufen ist.
    //
    // `flatten` legt die Felder auf der Leitung trotzdem flach ab, genau
    // wie der Vertrag sie beschreibt. Der Server sieht keinen Unterschied.
    #[serde(flatten)]
    pub bahn: BahnWire,

    // ── v1.7.15: die Bahn-Herkunft, nachtragsfaehig ──────────────────
    //
    // Siehe [`BahnHerkunftWire`]. Dieselbe Gruppe haengt am
    // `touchdown_rollout_finalized`, damit eine spaet eingetroffene
    // Szenerie nicht nur den Flugzustand, sondern auch die Zeile im
    // Recorder korrigiert.
    #[serde(flatten)]
    pub herkunft: BahnHerkunftWire,
}

/// Die Bahndisziplin-Werte auf der Leitung.
///
/// # Warum hier NICHTS uebersprungen wird
///
/// Die Felder trugen bis hierher `skip_serializing_if = "Option::is_none"`.
/// Das spart ein paar hundert Byte und macht den Nachtrag unmoeglich:
///
/// Der Recorder patcht die Touchdown-Zeile mit `json_patch` (RFC 7396).
/// Dort loescht ein `null` das Feld — ein FEHLENDES Feld laesst den alten
/// Wert stehen. Genau das war der Fehler: Verschiebt die Nachrechnung den
/// Kantenuebertritt um mehr als fuenfundzwanzig Meter, wird
/// `clearance_speed_kt` bewusst `None` — die Spur traegt keine
/// Geschwindigkeit fuer die neue Stelle. Uebersprungen erreicht dieses
/// `None` den Server nie, und die alte, vorlaeufige Fahrt blieb stehen.
///
/// Ein `None` muss loeschen koennen. Die Kosten sind dreizehn `null` je
/// Landung gegen dreizehn Kilobyte Rollspur.
///
/// **Der Client rechnet, der Server zeigt an.** Keine dieser Groessen wird
/// serverseitig nachgerechnet: Sie stammen aus dem 5-Hz-Rollout-Fenster,
/// das nur der Client sieht, und eine zweite Herleitung aus groberen
/// Daten kaeme zwangslaeufig auf andere Zahlen. Zwei Zahlen fuer dieselbe
/// Landung sind schlimmer als eine fehlende.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct BahnWire {
    /// Um wie viele Meter die Laengsmasse der Spur gegen die
    /// Landeschwelle verschoben sind. Siehe die ausfuehrliche
    /// Begruendung bei `BahnFelder::spur_nullpunkt_versatz_m` im Client
    /// — kurz: Der Payload fuehrt zwei Nullpunkte, und NUR dieser Wert
    /// sagt, wie weit sie auseinanderliegen. Die versetzte Schwelle ist
    /// eine andere Zahl und taugt dafuer nicht.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spur_nullpunkt_versatz_m: Option<f64>,
    /// Laengsposition beim Verlassen der Bahn (an der Kante).
    pub clearance_point_m: Option<f64>,
    /// Laengsposition, ab der nicht mehr bewertet wird — der Beginn des
    /// Ausschwenkens. NICHT dasselbe wie `clearance_point_m`.
    pub scoring_cutoff_m: Option<f64>,
    /// Bis wohin der groesste Querversatz mitgewachsen ist.
    ///
    /// Das Messfenster schliesst unter sechzig Knoten, `scoring_cutoff_m`
    /// erst beim Kurswechsel — bei DLH369 (EDDM 26L) lagen 600 Meter
    /// dazwischen. Ohne diese Zahl zeichnet die Anzeige die
    /// Bewertungsgrenze bei 2.251 m und daneben einen Hoechstwert, der
    /// nur bis 1.650 m gilt: beides fuer sich richtig, zusammen ein
    /// Widerspruch.
    ///
    /// # Warum hier KEIN `skip_serializing_if`
    ///
    /// Alle Geschwister in dieser Struktur werden immer gesendet, auch
    /// als `null`. Das ist Absicht: Der Recorder patcht per RFC 7396,
    /// dort LOESCHT ein `null` das Feld, und ein fehlendes laesst es
    /// stehen. Ein Feld, das sich nie als `null` zeigt, kann einen alt
    /// gewordenen Wert also nie mehr raeumen.
    ///
    /// Ich hatte hier zuerst `skip_serializing_if` stehen — aus Gewohnheit,
    /// nicht aus einem Grund. Damit waere dieses eine Feld das einzige
    /// gewesen, das sich nicht zuruecknehmen laesst.
    pub mess_ende_laengs_m: Option<f64>,
    pub clearance_speed_kt: Option<f64>,
    pub clearance_side: Option<String>,
    pub track_width_m: Option<f64>,
    pub track_width_source: Option<String>,
    pub wingspan_m: Option<f64>,
    /// Bahnbreite in Metern. Ohne sie laesst sich die Queransicht nicht
    /// massstaeblich zeichnen — eine geratene Breite waere eine
    /// Behauptung ueber die Kante, an der die Bewertung haengt.
    pub runway_width_m: Option<f64>,
    pub min_edge_clearance_m: Option<f64>,
    pub max_lateral_offset_m: Option<f64>,
    /// Der Spurverlauf — die groesste Nutzlast dieser Gruppe. Leer wird
    /// sie weggelassen, nicht als `[]` gesendet: Ein leeres Feld sieht in
    /// der Anzeige aus wie eine Messung, die nichts gefunden hat.
    pub lateral_samples: Option<Vec<LateralSampleWire>>,
    pub surface_paved: Option<bool>,
    pub overrun_m: Option<f64>,
    /// Warum die seitliche Bewertung entfiel. `None` = bewertet.
    ///
    /// Der Grund kommt aus der Bewertung selbst (`sub_scores`), damit die
    /// Anzeige ihn nicht ein zweites Mal herleitet. Zwei Herleitungen
    /// desselben Urteils driften auseinander.
    pub lateral_skip_reason: Option<String>,
    /// Die Ausfahrten dieser Bahn.
    ///
    /// # Warum sie über die Leitung gehen
    ///
    /// Sie stehen in der OSM-Bodenkarte, die auch der Server hat — er
    /// könnte sie also selbst rechnen. Genau das soll er nicht: Zwei
    /// Herleitungen derselben Grösse driften auseinander, und die Anzeige
    /// auf beiden Seiten muss dieselbe sein. Der Client hat die Karte im
    /// Anflug ohnehin geladen und rechnet einmal.
    ///
    /// Klein genug dafür: typisch vier bis zwölf Einträge je Bahn.
    /// ⚠ Das EINE Spur-Feld mit `skip_serializing_if`: Die Ausfahrten
    /// sind keine Messung, sondern eine Ableitung aus der Bodenkarte
    /// (Szenerie-Rollwege oder OSM). Fehlt die Karte — nach einem
    /// Neustart wird sie nicht persistiert —, ist `None` „Eingabe fehlt",
    /// nicht „keine Ausfahrten". Ein `null` haette die beim Recorder
    /// gespeicherten Ausfahrten geloescht (Runde 4, N14).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runway_exits: Option<Vec<RunwayExitWire>>,
    /// Steht das Ausrollen fest — oder ist das ein Zwischenstand?
    ///
    /// `touchdown_complete` geht rund neun Sekunden nach dem Aufsetzen
    /// raus. Zu diesem Zeitpunkt rollt das Flugzeug noch: Die Spur
    /// waechst, der Raeumpunkt ist nicht erreicht, die Ausrollstrecke
    /// steht nicht fest. Erst `touchdown_rollout_finalized` traegt das
    /// Endergebnis nach.
    ///
    /// **Kommt dieses Ereignis nicht an, sah der Bericht bisher aus wie
    /// ein fertiger.** Am 24.08.2026 traf es EDDB 06L (Landung 1079):
    /// zwoelf von dreizehn Landungen wurden finalisiert, diese eine
    /// nicht. Der Bericht zeigte 482 m Ausrollstrecke — nachgerechnet
    /// waeren das 0,42 g Verzoegerung, mehr als ein Verkehrsflugzeug
    /// bremsen kann — eine Spur, die mitten auf der Bahn aufhoert, und
    /// keinen Raeumpunkt. Alles davon war schlicht der Zwischenstand.
    ///
    /// `false` heisst: vorlaeufig. Die Anzeige muss das sagen, statt
    /// Zahlen zu zeigen, die noch nicht gelten.
    #[serde(default)]
    pub rollout_final: bool,
}

/// Eine Ausfahrt auf der Leitung. Eigener Typ, weil dieses Crate nicht von
/// `storage` abhängt — die Abhängigkeit läuft andersherum.
#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct RunwayExitWire {
    pub name: String,
    pub laengs_m: f64,
    /// `"left"` oder `"right"` in Landerichtung.
    pub seite: String,
    /// Wie der Rollweg von der Bahn wegfuehrt — in Bahnkoordinaten.
    ///
    /// Daraus zeichnet die Queransicht den Korridor unter der Spur. Leer
    /// heisst „keine Bodenkarte fuer diesen Rollweg", nicht „gerade".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verlauf: Vec<VerlaufspunktWire>,
}

/// Ein Stuetzpunkt eines Rollwegs auf der Leitung.
///
/// Eigener Typ aus demselben Grund wie `LateralSampleWire`: Dieses Crate
/// haengt nicht von `storage` ab, die Abhaengigkeit laeuft andersherum.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
pub struct VerlaufspunktWire {
    pub laengs_m: f64,
    pub quer_m: f64,
}

/// Ein Stuetzpunkt der Rollspur auf der Leitung.
///
/// Eigener Typ statt `storage::LateralSample`, weil dieses Crate nicht von
/// `storage` abhaengt — die Abhaengigkeit laeuft andersherum. Die Felder
/// heissen gleich, damit Vertrag und Anzeige denselben Namen sehen.
#[derive(Clone, Copy, Debug, Serialize, serde::Deserialize)]
pub struct LateralSampleWire {
    /// Distanz ab der Landeschwelle, in Metern, auf einen Dezimeter gerundet.
    pub laengs_m: f64,
    /// Versatz zur Mittellinie, in Metern, auf einen Dezimeter gerundet.
    /// Positiv = rechts in Landerichtung.
    pub quer_m: f64,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// v0.7.1: Stability-Gate-Window-Metadaten.
/// Beschreibt welche Sample-Region in `sub_stability` einging.
/// Spec §5.4 + §3.4: Werte aus `landing_scoring::gate::*`.
#[derive(Clone, Debug, Default, Serialize, serde::Deserialize)]
pub struct GateWindow {
    /// ms relativ zum Touchdown (negativ = vor TD)
    pub start_at_ms: i64,
    pub end_at_ms: i64,
    /// AGL/HAT in ft am Anfang/Ende des Windows
    pub start_height_ft: f32,
    pub end_height_ft: f32,
    /// Anzahl der Samples die `is_scored_gate == true` hatten
    pub sample_count: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct PirepPayload {
    pub ts: i64,
    pub pirep_id: String,
    pub flight_number: String,
    pub dep: String,
    pub arr: String,
    /// v0.11.1: Pilot-Client-Version. Siehe TouchdownPayload.client_version
    /// fuer Begruendung — Webapp liest die Pill aus dem PirepPayload damit
    /// die Reports-Uebersicht ohne Touchdown-Join die Version zeigen kann.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<&'static str>,
    pub block_time_min: Option<i32>,
    pub flight_time_min: Option<i32>,
    pub distance_nm: Option<f32>,
    /// **Raw** Sim-Cumulative-Counter aus dem Sim-Telemetry-Feed.
    ///
    /// **NICHT** als OFP-Vergleich nutzen! Bei MSFS ist das oft ein
    /// Cumulative-Wert seit Sim-Start (siehe SAS9987 v0.7.5: 19984 kg
    /// gemeldet bei tatsaechlich 8762 kg Trip-Burn → +117% Phantom-
    /// Abweichung). Spec docs/spec/v0.7.6-landing-payload-consistency.md.
    ///
    /// Fuer OFP-Vergleich: `actual_trip_burn_kg` benutzen, oder als
    /// Fallback `takeoff_fuel_kg - landing_fuel_kg` rechnen.
    pub fuel_used_kg: Option<f32>,
    pub planned_burn_kg: Option<f32>,
    pub block_fuel_kg: Option<f32>,
    pub takeoff_fuel_kg: Option<f32>,
    pub landing_fuel_kg: Option<f32>,
    /// v0.7.6: Trip-Burn = `takeoff_fuel_kg - landing_fuel_kg`.
    /// **Single Source of Truth fuer OFP-Vergleich** zwischen Pilot-
    /// Client, Web-Dashboard, Discord-Embed und phpVMS-Module.
    /// Replacement fuer den Raw-`fuel_used_kg`-Wert in allen Anzeigen
    /// die "Plan vs Actual"-Vergleiche zeigen.
    /// Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_trip_burn_kg: Option<f32>,
    pub takeoff_weight_kg: Option<f32>,
    pub landing_weight_kg: Option<f32>,
    pub planned_tow_kg: Option<f32>,
    pub planned_ldw_kg: Option<f32>,
    pub peak_altitude_ft: Option<i32>,
    pub landing_vs_fpm: Option<i32>,
    pub landing_score: Option<i32>,
    /// v0.20.0: Klasse und Note zum `landing_score` — eingefroren, damit die
    /// Webapp sie nicht aus der Zahl nachrechnet. Spiegel der gleichnamigen
    /// Felder im `TouchdownPayload`; beide stammen aus demselben
    /// `canonical_landing_verdict()`-Aufruf. Additiv, `serde(default)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_score_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_score_grade: Option<String>,
    pub go_around_count: Option<u32>,
    pub touchdown_count: Option<u32>,
    pub dep_gate: Option<String>,
    pub arr_gate: Option<String>,
    pub approach_runway: Option<String>,
    /// A divert that actually *happened*: the pilot confirmed it and the PIREP
    /// was filed against a different arrival airport than planned. Consumers
    /// (Discord "DIVERT filed" embed, webapp DIVERT pill) may treat this as
    /// fact.
    ///
    /// v0.19.3: this used to be set from a mere FSM *suspicion* as well, so a
    /// perfectly normal arrival that tripped the (broken) divert detection was
    /// announced to Discord as a filed divert while phpVMS recorded a normal
    /// arrival — the two systems then disagreed about the same flight forever.
    /// A suspicion now travels in `divert_suspected` below and is nobody's
    /// fact.
    pub divert: Option<bool>,
    pub diverted_to: Option<String>,
    /// The FSM *suspected* a divert (aircraft not on the planned field at
    /// shutdown) but the pilot did not file one. Diagnostic signal only —
    /// audit trails and support may read it; nothing may render it as a divert
    /// that happened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divert_suspected: Option<bool>,
    /// Field the FSM suspected the aircraft ended up on, when it could name
    /// one. `None` with `divert_suspected = Some(true)` means "off any known
    /// field".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub divert_suspected_icao: Option<String>,
    pub notes: Option<String>,
    /// v0.7.0 — Touchdown-Forensik-Version-Marker.
    /// 1 = legacy single-shot edge mit vs_at_edge override
    /// 2 = v0.7.0 pending_td_at + validate_candidate + impact_frame cascade
    /// MQTT-Consumer + aeroacars-live + zukuenftige Re-Analyzer koennen damit
    /// klar erkennen welche Auswertungs-Logik fuer den landing_vs_fpm gilt.
    /// Spec: docs/spec/touchdown-forensics-v2.md.
    #[serde(default = "default_forensics_version_v1")]
    pub forensics_version: u8,

    // ─── v0.7.1 Erweiterung (Spec §5.1) ────────────────────────────────
    // Alle Felder MUESSEN #[serde(default)] haben — alte PIREPs ohne
    // diese Felder muessen weiter deserialisieren (P3.4 Test-Anforderung).
    /// UX-Cutoff-Marker. 0 = pre-v0.7.1 PIREP (Score nicht-vergleichbar),
    /// 1 = v0.7.1+ (sub_scores aus landing-scoring Crate, Asymmetrie-
    /// Logik aktiv). UI nutzt diesen Marker um zu entscheiden ob der
    /// neue Sub-Score-Breakdown gerendert wird oder LegacyPirepNotice.
    /// Spec §3.5 Legacy-Schutz.
    #[serde(default)]
    pub ux_version: u8,

    // ─── F4: Forensik-Sichtbarkeit ────────────────────────────────────
    /// Confidence-Tagging vom Touchdown-v2-Cascade — High/Medium/Low/VeryLow.
    /// Wird parallel zu landing_rate_fpm via `finalize_landing_rate`-Helper
    /// gesetzt (siehe lib.rs:9362/11532/12312 — P2.2-D fix).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_confidence: Option<String>,
    /// Welche VS-Kette den finalen Wert geliefert hat.
    /// "vs_at_impact" | "smoothed_500ms" | "smoothed_1000ms" | "pre_flare_peak"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub landing_source: Option<String>,

    // ─── F6: Flare als eigene Zone (in PIREP exponiert, war nur in landing_history.json) ─
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flare_detected: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flare_reduction_fpm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flare_quality_score: Option<u8>,

    // ─── F7: Stability-v2-Felder (P2.1-A: bestehende Backend-Felder exponieren) ──────
    // Aliase: vs_jerk = mean |ΔVS|, NICHT max. excessive_sink = bool, NICHT count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approach_vs_stddev_fpm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approach_bank_stddev_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approach_vs_jerk_fpm: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approach_ias_stddev_kt: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approach_stable_config: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approach_excessive_sink: Option<bool>,
    /// Gate-Window-Metadaten — welche Sample-Region wirklich bewertet wurde.
    /// Spec F5 Tooltip "Bewertet werden Anflug-Samples zwischen 0 und 1000 ft AGL,
    /// die letzten 3 Sekunden vor TD ausgeschlossen". Werte aus
    /// `landing_scoring::gate::STABILITY_GATE_*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_window: Option<GateWindow>,

    // ─── Sub-Scores aus der landing-scoring Crate (Spec §3.1 SSoT, §5.4 Wire-Format) ──
    /// Voll ausgebautes `SubScoreEntry`-Format aus der landing-scoring
    /// Crate — UI/Web rendert direkt aus diesen Felder, KEIN Recompute.
    /// Bei alten PIREPs (ux_version < 1) ist der Vec leer; UI zeigt
    /// dann LegacyPirepNotice statt Breakdown.
    #[serde(default)]
    pub sub_scores: Vec<landing_scoring::SubScoreEntry>,

    // ─── v0.7.6 P1-3: Runway-Geometry-Trust ──────────────────────────
    // Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-3.
    //
    // Web/Monitor/Discord blendet Touchdown-Zone und Float-Distance
    // bei `trusted=false` aus (kein Raw-Display, weil Pilot sonst mit
    // kaputter Geometrie konfrontiert wird). Rollout-Sub-Score bleibt
    // valide (kommt aus GPS-Track, nicht aus Runway-DB).
    /// Ist die Runway-Geometrie (Match-ICAO + Centerline-Offset +
    /// Float-Distance) plausibel genug um TD-Zone + Float-Distance
    /// im UI zu zeigen?
    /// - `Some(true)` — alle Checks pass (200 m Centerline-Toleranz,
    ///   -100 m Float-Toleranz, ICAO matcht arr/divert)
    /// - `Some(false)` — mindestens ein Check failed, siehe `reason`
    /// - `None` — Feld fehlt (alte v0.7.5-PIREPs); UI behandelt das
    ///   wie `Some(true)` fuer Backward-Compat.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runway_geometry_trusted: Option<bool>,

    /// Grund warum `runway_geometry_trusted=false`:
    /// - "icao_mismatch"             — Match-ICAO != arr/divert
    /// - "centerline_offset_too_large" — > 200 m
    /// - "negative_float_distance"   — < -100 m
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runway_geometry_reason: Option<String>,

    // ─── v1.7.8 Bahngeometrie aus der Simulator-Szenerie ─────────────
    //
    // Dieselben Felder wie am TouchdownPayload — sie stehen an BEIDEN,
    // weil der Recorder die Teilnoten aus dem PIREP auf die Landezeile
    // propagiert. Stuende die Herkunft nur an einer, waere sie nach der
    // Propagation wieder weg.
    /// Woher die Bahngeometrie stammt: `"szenerie"` oder `"navdaten"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bahn_geometrie_quelle: Option<String>,
    /// Was aus der Szenerie-Abfrage wurde — feiner als
    /// `bahn_geometrie_quelle`, das nur "szenerie" oder "navdaten" kennt.
    /// Werte: nicht_angefordert | abgelehnt | keine_antwort | ohne_bahnen
    /// | kein_treffer | geliefert | uebernommen | keine_szenerie
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bahn_szenerie_status: Option<String>,
    /// Name + Version, mit denen sich der Simulator gemeldet hat.
    /// Trennt MSFS 2020 von 2024 — die Feldnamen der Facility-Abfrage
    /// stammen aus der 2024er-SDK-Doku.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sim_kennung: Option<String>,
    /// Um wie viel Grad der Kurs korrigiert wurde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bahn_kurs_korrektur_grad: Option<f64>,
    /// Um wie viel Meter die Breite korrigiert wurde.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bahn_breiten_korrektur_m: Option<f64>,
    /// Um wie viel Meter die versetzte Schwelle korrigiert wurde.
    /// Siehe die gleichnamige Erklaerung weiter oben.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bahn_schwellen_korrektur_m: Option<f64>,

    // ─── v0.7.19 GAF-707 Accident-Detection ──────────────────────────
    //
    // Spec docs/spec/v0.7.19-gaf707-crash-accident-detection.md §PIREP-
    // Payload. Webapp-PIREP-Feed muss auf PIREP-Ebene erkennen koennen
    // ob ein Flug als Accident eingestuft wurde — sonst kann die VPS-
    // History nur die einzelnen Touchdowns markieren, der PIREP-Eintrag
    // bleibt aber unauffaellig. Das ist genau der Worst-Case bei Multi-
    // Touchdown-Fluegen (T&G + finaler Crash).
    //
    // `accident_classifier_version` (Sentinel) wird IMMER gesetzt — auch
    // wenn kein Accident erkannt wurde. Webapp unterscheidet damit
    // "Classifier lief, false" von "historischer Payload".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_classifier_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_confidence: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_reasons: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accident_at: Option<i64>,

    /// v0.10.0 (#runway-utilization-score) — Algorithmus-Version des
    /// `sub_scores`-Arrays. None/Some(1) = pre-v0.10 (meter-only Bahn-
    /// Auslastung); Some(2) = v0.10 (LDA-basierter Runway-Utilization-
    /// Score); Some(3) = v0.12.0 (Float-Toleranz-Refinement); Some(4) =
    /// v0.16.21 (MSFS touchdown V/S SimVar-lag corrected); Some(5) =
    /// v0.20.x (Bahnauslastung-QS + Sinkraten-Ziel-Korridor). Spec:
    /// docs/spec/v0.10.0-runway-utilization-score.md LE11.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_algorithm_version: Option<u8>,

    /// v0.20 (Process-Integrity). Deliberately its OWN namespace, separate
    /// from `accident_kind`/`accident_reasons` above: those describe an
    /// in-sim AIRCRAFT accident/hull-loss (SimConnect's `Crashed` system
    /// event). This describes the SIM or CLIENT PROCESS itself dying —
    /// an unrelated failure mode. `None` when nothing worth reporting
    /// happened (the overwhelming majority of flights) — omitted from the
    /// wire entirely via `skip_serializing_if` so unaffected PIREPs are
    /// byte-identical to before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_health: Option<ClientHealthReport>,
}

/// v0.20 (Process-Integrity): client-self-reported OBSERVATIONS about its
/// own or the simulator's process health around a disconnect/resume. The
/// client only ever asserts facts here — the recorder's `computeScoreTrust`
/// (aeroacars-live/recorder/src/scoreTrust.ts) remains the sole place that
/// turns these into a review verdict, same as every other trust signal.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ClientHealthReport {
    /// "sim_process_gone" | "sim_process_alive" | "unknown" — from
    /// `sim_core::process_probe::ProcessLiveness::as_wire_str()`, sampled
    /// at the moment `SimDisconnect` was first detected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disconnect_sim_liveness: Option<String>,
    /// Das geflogene Muster stand nicht in der Grenzen-Tabelle des Clients
    /// (`aircraft_limits_for`), die Bewertung lief also auf generischen
    /// Ersatzwerten: Wing-Strike gegen 8° statt gegen das echte Limit, und
    /// keine Vref-Abweichung.
    ///
    /// Ohne diese Meldung bleibt so eine Luecke wochenlang unbemerkt — eine
    /// leere Kennzahl sieht aus wie "war halt nicht messbar". Gemessen am
    /// 16.08.2026 traf das 31 der 84 gebuchten Muster, 496 von 3794 Fluegen.
    /// Traegt den ICAO-Code, damit der VA-Betreiber weiss, WAS fehlt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_aircraft_icao: Option<String>,
    /// `true` if the AeroACARS run that resumed this flight (if any) did
    /// NOT exit cleanly (crash/kill/power-loss) — from the run-sentinel
    /// check in `try_resume_flight()`. `None` if the flight never went
    /// through an app restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_restart_unclean: Option<bool>,
    /// `true` if the resume-discontinuity check (fuel jump / extreme
    /// drift) fired — a physically-impossible jump between the last
    /// known state and the first fresh snapshot after a pause/restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impossible_resume_jump: Option<bool>,
    /// Signed (current − previous) — a positive value is the impossible
    /// direction (fuel increasing mid-flight).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_fuel_delta_kg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_altitude_delta_ft: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_gap_minutes: Option<i64>,
}

/// Default fuer pre-v0.7.0 PIREPs ohne den marker. Wird von serde
/// genutzt wenn der PIREP-Payload aus alten JSONL-Backups oder
/// aeroacars-live-Storage deserialisiert wird.
#[allow(dead_code)]
fn default_forensics_version_v1() -> u8 {
    1
}

/// v0.7.19 GAF-707 (QS-R2 Finding 1): Korrektur-Event fuer den Fall
/// dass ein Touchdown bereits als Accident gepublisht und in der
/// Webapp-DB persistiert wurde, der Pilot aber im Flight-End-Dialog
/// "Nein, als harte Landung filen" gewaehlt hat. Ohne diesen Event
/// blieb der Touchdown-Row server-seitig weiter `accident=true`,
/// obwohl der PIREP regulaer rausging — die Webapp-History haette
/// "Accident" gezeigt, der phpVMS-PIREP "harte Landung". Spec
/// §AeroACARS Client Tab "Landung" + QS-R2 Finding 1.
///
/// Recorder mappt `decision` zu einem DB-UPDATE auf den Touchdown
/// (matched per `pirep_id` — Webapp arbeitet pro PIREP mit dem
/// Worst-Case-Touchdown).
///   - "as_hard_landing" → accident=false + accident_kind=null +
///     accident_confidence=null + accident_reasons enthaelt nur den
///     pilot_override-Eintrag.
///   - "as_accident"     → unveraendert (expliziter "ja, Unfall"-
///     Klick; nur fuer Audit).
#[derive(Clone, Debug, Serialize)]
pub struct TouchdownAccidentOverridePayload {
    pub ts: i64,
    pub pirep_id: String,
    pub decision: String, // "as_hard_landing" | "as_accident"
    pub accident: bool,
    pub accident_kind: Option<String>,
    pub accident_confidence: Option<String>,
    pub accident_reasons: Vec<String>,
    /// Original-Klassifikations-Stand vor dem Override (Audit-Trail).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_confidence: Option<String>,
}

/// v0.12.4 (Spec docs/spec/v0.12.4-score-consistency.md, LE4): nachgelagertes
/// Finalisierungs-Event. `touchdown_complete` geht ~9 s nach dem Aufsetzen
/// raus, `rollout_distance_m` ist dort ein Mitten-im-Ausrollen-Snapshot.
/// Sobald der Rollout finalisiert ist (~40 kt / Heading-Turn-off), schickt
/// der Client dieses Event mit dem FINALEN Wert nach; der Recorder patcht
/// damit nur das Rohfeld der Touchdown-Zeile — KEIN Score-Recompute, KEINE
/// Verzögerung von `touchdown_complete`/Live-Pushes.
impl TouchdownRolloutFinalizedPayload {
    /// Liest den Nachtrag aus JSON — und stellt den fehlenden Spur-Block
    /// als `None` wieder her.
    ///
    /// ⚠ `#[serde(flatten)]` auf `Option<BahnWire>` liefert bei FEHLENDEN
    /// Schluesseln `Some(BahnWire::default())`, nicht `None` — mit
    /// `rollout_final: false` und lauter `None`-Feldern, die auf der
    /// Leitung zu `null` werden. Auf dem Warteschlangen-Weg (Offline-
    /// Einreichen) holte das N13 zurueck: Der Worker sendete einen
    /// „leeren" Block und drehte eine endgueltige Zeile auf vorlaeufig
    /// (Client-QS, Runde 5, N23). Das Erkennungsmerkmal ist
    /// `rollout_final`: Es steht in JEDEM echten Block (kein
    /// `skip_serializing_if`) und in keinem fehlenden.
    pub fn aus_json(json: serde_json::Value) -> Result<Self, serde_json::Error> {
        let block_da = json.get("rollout_final").is_some();
        let mut nachtrag: Self = serde_json::from_value(json)?;
        if !block_da {
            nachtrag.bahn = None;
        }
        Ok(nachtrag)
    }
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
pub struct TouchdownRolloutFinalizedPayload {
    /// Event-Zeitstempel (Finalisierungs-Moment), ms seit Epoch.
    pub ts: i64,
    /// PIREP-ID — grenzt die Touchdown-Zeile(n) auf den Flug ein.
    pub pirep_id: String,
    /// Touchdown-Zeitstempel (`landing_at`, ms seit Epoch) — identisch
    /// zum `ts`-Feld des `TouchdownPayload` dieses Touchdowns. Der Recorder
    /// patcht damit GENAU die zugehörige Touchdown-Zeile, nicht alle Zeilen
    /// des PIREPs (wichtig bei Touch-and-Go / Stop-and-Go — jeder Touchdown
    /// hat seinen eigenen Rollout).
    pub touchdown_at: i64,
    /// Finale Ausrollstrecke Touchdown→Rollout-Ende in Metern.
    pub rollout_distance_m: f64,
    /// Welcher Trigger die Finalisierung ausgelöst hat — Diagnose.
    /// `"exit_speed"` | `"full_stop"` | `"turned_off_runway"`. Optional:
    /// nach einem Client-Neustart mitten im Finalisierungs-Fenster ist der
    /// Grund nicht mehr bekannt (transient) — das Event geht trotzdem raus.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finalize_reason: Option<String>,

    // ── v1.7.0: die Bahndisziplin-Werte, final ───────────────────────
    //
    // # Warum sie hier noch einmal kommen
    //
    // `touchdown_complete` geht rund neun Sekunden nach dem Aufsetzen
    // raus. Zu diesem Zeitpunkt waechst die Rollspur weiter, der
    // Raeumpunkt ist noch nicht erreicht und der Kantenuebertritt erst
    // recht nicht — genau wie `rollout_distance_m`, um dessentwillen
    // dieses Event ueberhaupt existiert.
    //
    // Bis v1.7.0 trug es nur die Ausrollstrecke nach. Alle uebrigen
    // Bahnwerte blieben im Recorder und in der Webapp beim vorlaeufigen
    // Stand: eine Spur, die mitten im Ausrollen abbricht, und ein
    // Raeumpunkt, den es zu dem Zeitpunkt noch gar nicht gab.
    //
    // Die Gruppe ist dieselbe wie im `TouchdownPayload` — ein Typ, eine
    // Umrechnung (`BahnFelder::wire()`), damit der Nachtrag nicht
    // auseinanderlaufen kann.
    //
    // ⚠ OPTIONAL seit Runde 4 (N13): Der Block geht nur mit, wenn die
    // Spur vollstaendig ist. Sonst FEHLT er — er wird nicht als `null`
    // gesendet. `null` loescht beim Recorder, und `rollout_final: false`
    // draengte eine laengst endgueltige Zeile auf „vorlaeufig" zurueck.
    // Fehlt der Block, bleibt beim Recorder der letzte vollstaendige
    // Stand stehen; Herkunft, Drittel und Riegel kommen trotzdem.
    #[serde(flatten)]
    pub bahn: Option<BahnWire>,

    // ── v1.7.15: das Drittel, nachgetragen ───────────────────────────
    //
    // `landing_touchdown_zone` ist beim Recorder eine SPALTE, die nur
    // der INSERT aus `touchdown_complete` schreibt. Trifft die Szenerie
    // danach ein, korrigierte der Nachtrag `td_third` im JSON, die
    // Spalte blieb stehen — zwei Karten und ein Diagramm zeigten
    // verschiedene Drittel derselben Landung (externe QS, 02.09.2026,
    // N3). Nicht Teil von `BahnHerkunftWire`: Der Wert haengt am
    // Vertrauens-Riegel der Bahngeometrie, der den Flug braucht.
    // `None` geht als `null` hinaus und LOESCHT die Spalte — ein
    // Drittel gegen eine nicht vertrauenswuerdige Bahn ist keins.
    #[serde(default)]
    pub landing_touchdown_zone: Option<u8>,
    /// Der Vertrauens-Riegel der Bahngeometrie — dieselbe Ableitung wie
    /// am `touchdown_complete`. Ohne ihn blieben `runway_geometry_trusted`
    /// und `_reason` beim Recorder auf dem Stand des Aufsetzens, waehrend
    /// die Bahn darunter wechselte (externe QS, Runde 3).
    #[serde(default)]
    pub runway_geometry_trusted: Option<bool>,
    #[serde(default)]
    pub runway_geometry_reason: Option<String>,

    // ── v1.7.15: die Bahn-Herkunft, nachgetragen ─────────────────────
    //
    // Bis v1.7.14 trug dieses Ereignis von der Bahn nur die Spur und die
    // Ausrollstrecke. Kam die Szenerie erst nach dem Aufsetzen, holte der
    // Client die Zuordnung zwar nach — beim Recorder blieben Bahnlaenge,
    // versetzte Schwelle, Herkunft, Szenerie-Stand und die
    // Aufsetzpunkt-Einordnung aber auf dem Stand VOR der Szenerie
    // stehen. Vier gemeinsame Felder von achtundzwanzig.
    //
    // Siehe [`BahnHerkunftWire`]: ein Typ, eine Ableitung, beide
    // Ereignisse.
    #[serde(flatten)]
    pub herkunft: BahnHerkunftWire,
}

/// Was der Client sendet, wenn jemand etwas zuruft.
///
/// Rufzeichen und Klarname stehen bewusst NICHT drin — die setzt der Server
/// aus der laufenden Flugsitzung. Sonst koennte sich jemand ein fremdes
/// Rufzeichen geben.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatSenden {
    pub ts: i64,
    pub text: String,
    /// Gesetzt = Direktnachricht an genau diesen Piloten.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub an_pilot_id: Option<String>,
}

enum Cmd {
    // v1.5.7 (#mqtt-outage, Feldbefund Michel TAP58): `Position` ist hier
    // BEWUSST NICHT MEHR drin. Positionen laufen über einen eigenen
    // `watch`-Kanal (siehe `Handle::pos_tx`), der IMMER nur den neuesten
    // Wert hält und deshalb strukturell nicht volllaufen kann.
    //
    // Vorher teilten sie sich diese Warteschlange mit den Ereignissen —
    // mit fatalem Ausgang bei Michels 4,5-stündigem Netzausfall: 200 alte
    // Positionen verstopften den Kanal, 4976 neue wurden verworfen, und
    // als die Landung kam, gab sie nach 250 ms auf ("dropping touchdown
    // publish"). Genau die Nachricht, auf die es ankam, verlor gegen
    // Daten, die längst wertlos waren.
    Phase(PhasePayload),
    Block(Box<BlockPayload>),
    Takeoff(Box<TakeoffPayload>),
    Touchdown(Box<TouchdownPayload>),
    Pirep(Box<PirepPayload>),
    /// v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, LE1): vorab-
    /// serialisiertes PIREP-Payload. Der Filing-Refactor baut das
    /// Payload einmal als JSON (`build_pirep_payload` → `serde_json::Value`)
    /// und nutzt diesen Pfad für ALLE 4 Filing-Wege — inkl. dem Queue-
    /// Worker, der nur die persistierte JSON-Form besitzt.
    PirepJson(Box<serde_json::Value>),
    /// Pilotenchat: ein abgeschickter Zuruf. Bewusst in der Warteschlange
    /// der Ereignisse und NICHT im Positions-Kanal — ein Zuruf entsteht
    /// selten und darf nicht von der naechsten Position ueberschrieben
    /// werden. `retain: false`: ein Zuruf ist fluechtig, niemand soll ihn
    /// beim Verbinden nachgeliefert bekommen.
    Chat(Box<ChatSenden>),
    TouchdownAccidentOverride(Box<TouchdownAccidentOverridePayload>),
    /// Mit Zustellmeldung: `true` erst, wenn die Leitung stand UND der
    /// Publish angenommen wurde. Ein Handle allein ist kein Nachweis.
    TouchdownRolloutFinalized(
        Box<TouchdownRolloutFinalizedPayload>,
        tokio::sync::oneshot::Sender<bool>,
    ),
    Shutdown,
}

/// v0.13.0 Stream F (Slice 6) — Integrity-Flag-Event vom Recorder.
/// Wird live published auf `aeroacars/<va>/<pilot>/integrity_flag` und
/// vom Client konsumiert für DATA-INTEGRITY-Banner + Resume-Policy.
/// Ein eingehender Zuruf aus dem Pilotenchat.
///
/// Der Server stellt ihn auf dem persoenlichen Thema des Empfaengers zu
/// (`aeroacars/{va}/{pilot}/chat_in`). Direktnachrichten erreichen deshalb
/// wirklich nur einen — die Trennung ist nicht bloss Anzeige.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatNachricht {
    pub id: i64,
    pub va_prefix: String,
    pub von_pilot_id: String,
    #[serde(default)]
    pub an_pilot_id: Option<String>,
    pub ts: i64,
    pub text: String,
    #[serde(default)]
    pub callsign: Option<String>,
    #[serde(default)]
    pub anzeigename: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct IntegrityFlagEvent {
    pub session_id: i64,
    pub session_effective_severity: String,
    pub flag: serde_json::Value,
}

#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Cmd>,
    /// v1.5.7: Positions-Weg, getrennt von den Ereignissen. `watch` hält
    /// genau EINEN Wert — ein Sender überschreibt den vorigen, statt eine
    /// Schlange zu bilden. Nach einem Netzausfall geht damit sofort die
    /// AKTUELLE Position raus statt Hunderter veralteter.
    pos_tx: watch::Sender<Option<Box<PositionPayload>>>,
    /// v0.13.0: optional Broadcast-Receiver für Integrity-Flag-Events.
    /// Wird per `take_integrity_rx()` einmalig konsumiert.
    integrity_rx: Arc<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<IntegrityFlagEvent>>>>,
    chat_rx: Arc<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<ChatNachricht>>>>,
    /// v0.19.x FIX: `Cmd::Shutdown` only ever stopped the PUBLISHER task
    /// (the one draining `tx`). The reconnect-loop "drive" task — the one
    /// that owns `eventloop.poll()`, auto-reconnects on every error by
    /// rumqttc's own design, and re-subscribes to integrity_flag — had no
    /// way to learn shutdown happened at all: dropping its `JoinHandle`
    /// (bound to `_drive`, never stored) does not abort a tokio task, and
    /// `disconnect()`'s resulting poll error was itself just another
    /// "reconnect after backoff" event to it. Credentials stayed in
    /// active use on the broker seconds after an explicit local logout,
    /// leaking one task+connection per login/logout cycle. This watch
    /// channel lets `shutdown()` signal the drive loop directly.
    shutdown_tx: watch::Sender<bool>,
    /// Die Identitaet dieser Verbindung — fuer die Nachtrags-Ablage, die
    /// je Mandant getrennt liegt (Codex, 03.09.2026, Runde 12).
    va_prefix: String,
    pilot_id: String,
}

/// Das Zustellbuch: ordnet jedem Publish sein PUBACK zu.
///
/// # Warum (Codex, 03.09.2026, Runde 13, High 1)
///
/// `AsyncClient::publish` meldet nur die Uebernahme in rumqttcs Auftragskanal.
/// Ein `true` daraus war keine Zustellung — die Ablage-Datei fiel, bevor der
/// Broker das Paket je gesehen hatte. rumqttc 0.25 gibt dem Aufrufer keine
/// Paketkennung; sie entsteht erst im Eventloop (`state.outgoing_publish`).
/// Was der Eventloop aber sichtbar macht: `Outgoing::Publish(pkid)` in GENAU
/// der Reihenfolge, in der die Auftraege in den Kanal kamen, und
/// `Incoming::PubAck(pkid)`.
///
/// # Wie
///
/// 1. JEDER Publish geht durch `publish_registriert`: unter einem Schloss
///    `try_publish` und, wenn angenommen, ein Eintrag hinten ins Buch —
///    `Some(meldung)` fuer einen Nachtrag, `None` fuer alles andere. Kein
///    `.await` im Schloss (darum `try_publish`, mit eigener Warteschleife
///    statt `publish().await`), damit zwei Aufgaben sich nicht ueberholen.
/// 2. Der Drive-Loop nimmt bei `Outgoing::Publish(pkid)` den VORDERSTEN
///    Eintrag: pkid 0 (QoS 0) → weg; sonst → wartet unter dieser pkid.
/// 3. `Incoming::PubAck(pkid)` → der Eintrag unter der pkid meldet `true`.
/// 4. Faellt die Leitung, laufen wir mit `clean_session`: rumqttc leert beim
///    naechsten CONNACK alles Anstehende (unbestaetigte Publishes UND die
///    Auftraege, die beim Abriss im Kanal lagen). `verbindung_weg` meldet
///    ihnen allen `false` — und nimmt genau so viele frische Eintraege vom
///    Buchanfang, wie `eventloop.pending` Publishes mit pkid 0 traegt.
///    Sonst stuende das Buch ab dem naechsten Publish um eins versetzt.
///
/// # Was es nicht abdeckt
///
/// Eine Kollision (`Outgoing::AwaitAck`) haelt den Eventloop an, bis das
/// alte Paket bestaetigt ist; das kollidierte geht danach als naechstes
/// raus — Reihenfolge bleibt. Kommt sein `Outgoing::Publish` VOR dem
/// `PubAck` des alten durch die Ereignisschlange, stehen unter der pkid
/// zwei Eintraege; sie werden in Reihenfolge bestaetigt.
#[derive(Default)]
struct Zustellbuch {
    /// Angenommene Publishes, noch ohne `Outgoing::Publish`.
    frisch: VecDeque<Option<tokio::sync::oneshot::Sender<bool>>>,
    /// Rausgegangen, wartet auf PUBACK — je pkid in Reihenfolge.
    unterwegs: HashMap<u16, VecDeque<Option<tokio::sync::oneshot::Sender<bool>>>>,
}

impl Zustellbuch {
    fn registrieren(&mut self, meldung: Option<tokio::sync::oneshot::Sender<bool>>) {
        self.frisch.push_back(meldung);
    }

    /// `Outgoing::Publish(pkid)` gesehen.
    fn ausgegangen(&mut self, pkid: u16) {
        let Some(eintrag) = self.frisch.pop_front() else {
            // Ein Publish, den niemand registriert hat — das Buch ist
            // versetzt. Laut sagen; die Ablage faengt den Nachtrag ueber
            // die Frist.
            warn!("Zustellbuch: Outgoing::Publish({pkid}) ohne Eintrag — Buch versetzt?");
            return;
        };
        if pkid == 0 {
            if let Some(m) = eintrag {
                let _ = m.send(false);
            }
            return;
        }
        self.unterwegs.entry(pkid).or_default().push_back(eintrag);
    }

    /// `Incoming::PubAck(pkid)` gesehen.
    fn bestaetigt(&mut self, pkid: u16) {
        let Some(schlange) = self.unterwegs.get_mut(&pkid) else {
            return;
        };
        if let Some(Some(m)) = schlange.pop_front() {
            let _ = m.send(true);
        }
        if schlange.is_empty() {
            self.unterwegs.remove(&pkid);
        }
    }

    /// Leitung weg: alles Unterwegs ist verloren (clean_session), und die
    /// `verschluckte` frischen Auftraege aus dem Kanal ebenfalls.
    fn verbindung_weg(&mut self, verschluckte: usize) {
        for (_, schlange) in self.unterwegs.drain() {
            for m in schlange.into_iter().flatten() {
                let _ = m.send(false);
            }
        }
        for _ in 0..verschluckte {
            if let Some(Some(m)) = self.frisch.pop_front() {
                let _ = m.send(false);
            }
        }
    }

    fn offen(&self) -> (usize, usize) {
        (
            self.frisch.len(),
            self.unterwegs.values().map(|q| q.len()).sum(),
        )
    }
}

type Buch = Arc<Mutex<Zustellbuch>>;

/// DER Publish. Unter dem Schloss `try_publish` + Eintrag ins Buch; die
/// `meldung` (wenn eine da ist) bekommt ihr `true` erst mit dem PUBACK.
/// Wird der Auftrag nicht angenommen, meldet sie sofort `false`.
fn publish_registriert(
    client: &AsyncClient,
    buch: &Buch,
    topic: &str,
    qos: QoS,
    retain: bool,
    body: Vec<u8>,
    meldung: Option<tokio::sync::oneshot::Sender<bool>>,
) -> Result<(), rumqttc::ClientError> {
    let mut b = buch.lock().unwrap_or_else(|e| e.into_inner());
    match client.try_publish(topic, qos, retain, body) {
        Ok(()) => {
            b.registrieren(meldung);
            Ok(())
        }
        Err(e) => {
            if let Some(m) = meldung {
                let _ = m.send(false);
            }
            Err(e)
        }
    }
}

/// Wie `publish_registriert`, wartet aber bei vollem Kanal bis zur Frist —
/// das ist, was `publish().await` vorher tat, nur ohne `.await` im Schloss.
async fn publish_registriert_wartend(
    client: &AsyncClient,
    buch: &Buch,
    topic: &str,
    qos: QoS,
    retain: bool,
    body: Vec<u8>,
    mut meldung: Option<tokio::sync::oneshot::Sender<bool>>,
) -> bool {
    let bis = tokio::time::Instant::now() + PUBLISH_TIMEOUT;
    loop {
        // Die Meldung wandert nur bei Annahme ins Buch; bei „voll" behalten
        // wir sie fuer den naechsten Versuch.
        let versuch = {
            let mut b = buch.lock().unwrap_or_else(|e| e.into_inner());
            match client.try_publish(topic, qos, retain, body.clone()) {
                Ok(()) => {
                    b.registrieren(meldung.take());
                    Ok(())
                }
                Err(e) => Err(e),
            }
        };
        // `ClientError::TryRequest` heisst „voll ODER geschlossen" — rumqttc
        // unterscheidet das nicht. Beides: warten bis zur Frist; ein
        // geschlossener Kanal laeuft dann in die Frist (nur beim Abbau).
        match versuch {
            Ok(()) => return true,
            Err(_) => {
                if tokio::time::Instant::now() >= bis {
                    warn!(
                        "publish {topic} nach {} s abgebrochen — Leitung blockiert",
                        PUBLISH_TIMEOUT.as_secs()
                    );
                    if let Some(m) = meldung.take() {
                        let _ = m.send(false);
                    }
                    return false;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

/// Der Sendeweg fuer Bahn-Nachtraege — klonbar, damit ihn niemand unter
/// gehaltenem `state.mqtt`-Schloss benutzen muss.
///
/// # Warum kein `touchdown_rollout_finalized(&self)` mehr
///
/// Die alte Methode war fire-and-forget: Sie startete eine Aufgabe und
/// meldete nichts zurueck. Der Aufrufer nahm „Handle vorhanden" als
/// „zugestellt", loeschte seine Fahne und die Ablage-Datei — bei liegender
/// Leitung (Handle bleibt fuer den Reconnect bestehen) war der Nachtrag
/// weg (Codex, 03.09.2026, P1). Jetzt liefert `senden` eine Zustellmeldung:
/// `true` nur, wenn die Leitung stand und der Publish angenommen wurde.
/// Alles andere — Kanal voll, Leitung liegt, Frist abgelaufen — ist
/// `false`, und der Aufrufer laesst seine Ablage-Datei liegen.
///
/// Seit Runde 13 heisst `true`: **PUBACK des Brokers** fuer genau dieses
/// Paket (siehe `Zustellbuch`), nicht mehr nur „im Kanal angenommen".
#[derive(Clone)]
pub struct NachtragSender {
    tx: mpsc::Sender<Cmd>,
    pub va_prefix: String,
    pub pilot_id: String,
}

impl NachtragSender {
    pub fn senden(
        &self,
        payload: TouchdownRolloutFinalizedPayload,
    ) -> tokio::sync::oneshot::Receiver<bool> {
        let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let (weiter_tx, weiter_rx) = tokio::sync::oneshot::channel();
            let eingereiht = tokio::time::timeout(
                EVENT_ENQUEUE_TIMEOUT,
                tx.send(Cmd::TouchdownRolloutFinalized(Box::new(payload), weiter_tx)),
            )
            .await;
            match eingereiht {
                Ok(Ok(())) => {
                    // Der Publisher meldet ueber `weiter_rx`; ist er weg,
                    // kommt Err → false.
                    let ok = weiter_rx.await.unwrap_or(false);
                    let _ = ack_tx.send(ok);
                }
                Ok(Err(e)) => {
                    warn!("touchdown_rollout_finalized: Auftragskanal geschlossen: {e}");
                    let _ = ack_tx.send(false);
                }
                Err(_) => {
                    warn!("touchdown_rollout_finalized: Auftragskanal voll — nicht eingereiht");
                    let _ = ack_tx.send(false);
                }
            }
        });
        ack_rx
    }
}

impl Handle {
    /// Der bestaetigte Sendeweg fuer Bahn-Nachtraege — siehe `NachtragSender`.
    pub fn nachtrag_sender(&self) -> NachtragSender {
        NachtragSender {
            tx: self.tx.clone(),
            va_prefix: self.va_prefix.clone(),
            pilot_id: self.pilot_id.clone(),
        }
    }

    /// v0.13.0 Slice 6: Konsumiert den einmaligen Receiver für
    /// Integrity-Flag-Events vom Recorder. Caller (Tauri-Main) ruft
    /// das genau einmal nach `connect()` und forwarded die Events
    /// als Tauri-Events an die React-UI.
    ///
    /// Returns None wenn der Receiver bereits genommen wurde.
    /// Der Empfaenger fuer eingehende Zurufe. Wie beim Integritaets-Kanal
    /// nur EINMAL zu holen — danach liefert der Aufruf None.
    pub async fn take_chat_rx(&self) -> Option<mpsc::UnboundedReceiver<ChatNachricht>> {
        self.chat_rx.lock().await.take()
    }

    pub async fn take_integrity_rx(&self) -> Option<mpsc::UnboundedReceiver<IntegrityFlagEvent>> {
        self.integrity_rx.lock().await.take()
    }

    pub fn position(&self, snap: &SimSnapshot, meta: &FlightMeta, phase: FlightPhase) {
        let payload = PositionPayload {
            ts: snap.timestamp.timestamp_millis(),
            phase: phase_label(phase),
            // v0.16.13: vom Streamer auf den Snapshot gestempelt (lib.rs,
            // direkt nach der Schatten-Engine — Reihenfolge verifiziert).
            shadow_phase: snap.shadow_phase.clone(),
            shadow_segment: snap.shadow_segment.clone(),

            // Position
            lat: snap.lat,
            lon: snap.lon,
            // v0.16.15: Live-Map zeigt die Altimeter-Hoehe (FR24-Konvention,
            // Piloten-Erwartung); geometrisches MSL nur als Fallback.
            alt_ft: snap
                .altitude_indicated_ft
                .unwrap_or(snap.altitude_msl_ft)
                .round() as i32,
            agl_ft: snap.altitude_agl_ft.round() as i32,

            // Attitude
            pitch_deg: snap.pitch_deg,
            bank_deg: snap.bank_deg,
            hdg_true: snap.heading_deg_true.round() as i32,
            hdg_mag: snap.heading_deg_magnetic.round() as i32,

            // Speeds
            ias_kt: snap.indicated_airspeed_kt.round() as i32,
            tas_kt: snap.true_airspeed_kt.round() as i32,
            gs_kt: snap.groundspeed_kt.round() as i32,
            vs_fpm: snap.vertical_speed_fpm.round() as i32,
            mach: snap.mach,

            // Forces / state
            g_force: snap.g_force,
            on_ground: snap.on_ground,
            parking_brake: snap.parking_brake,
            stall_warning: snap.stall_warning,
            overspeed_warning: snap.overspeed_warning,

            // Config
            gear_position: snap.gear_position,
            flaps_position: snap.flaps_position,
            spoilers_position: snap.spoilers_handle_position,
            spoilers_armed: snap.spoilers_armed,
            engines_running: snap.engines_running,

            // Lights
            light_beacon: snap.light_beacon,
            light_strobe: snap.light_strobe,
            light_landing: snap.light_landing,

            // Fuel
            fuel_total_kg: snap.fuel_total_kg,
            fuel_used_kg: snap.fuel_used_kg,
            fuel_flow_kg_h: snap.fuel_flow_kg_per_h,
            total_weight_kg: snap.total_weight_kg,

            // Environment
            wind_dir_deg: snap.wind_direction_deg,
            wind_speed_kt: snap.wind_speed_kt,
            oat_c: snap.outside_air_temp_c,
            qnh_hpa: snap.qnh_hpa,

            // AP
            ap_master: snap.autopilot_master,
            ap_hdg: snap.autopilot_heading,
            ap_alt: snap.autopilot_altitude,
            ap_nav: snap.autopilot_nav,
            ap_app: snap.autopilot_approach,

            // Identity — alle non_empty(): leere Strings werden zu None und
            // verschwinden aus dem JSON statt "" zu serialisieren. Server-
            // seitige COALESCE-UPSERTs bleiben so frei von Empty-String-
            // Vergiftung der flights-Tabelle.
            callsign: non_empty(&meta.callsign),
            aircraft_icao: non_empty(&meta.aircraft_icao),
            // v0.5.19: prefer phpVMS-side registration (from the bid)
            // over what the sim reports — payware addons often put
            // a placeholder ("FFSTS") in the SimConnect ATC-ID.
            // Falls back to the sim value if the bid had nothing.
            aircraft_registration: if !meta.planned_registration.trim().is_empty() {
                Some(meta.planned_registration.trim().to_string())
            } else {
                snap.aircraft_registration.as_deref().and_then(non_empty)
            },
            // v0.8.3 (#5 follow-up): Sim-Aircraft-Title fuer Recorder-
            // Stats-Recompute. Quelle: SimVar TITLE (MSFS) /
            // acf_descrip (XP12). non_empty() filtert leere Strings.
            aircraft_title: snap.aircraft_title.as_deref().and_then(non_empty),
            simulator: simulator_label(snap.simulator),
            dep: non_empty(&meta.dep_icao),
            arr: non_empty(&meta.arr_icao),
            pirep_id: non_empty(&meta.pirep_id),
            // v1.5.5 Stand-Erkennung (live): Recorder haengt sie per
            // setFlightGates an den Live-Flug — Live-Map zeigt den
            // Ankunftsstand damit schon beim Einparken, nicht erst
            // nach dem PIREP-Filing.
            dep_gate: meta.dep_gate.as_deref().and_then(non_empty),
            arr_gate: meta.arr_gate.as_deref().and_then(non_empty),
            client_version: env!("CARGO_PKG_VERSION"),
        };
        // v1.5.7 (#mqtt-outage): kann NICHT fehlschlagen und NICHTS
        // blockieren — `send` auf einem `watch` überschreibt den vorigen
        // Wert. Ein Netzausfall staut hier nichts mehr auf; sobald die
        // Leitung zurück ist, geht die zu DIESEM Zeitpunkt aktuelle
        // Position raus, nicht die von vor drei Stunden.
        if self.pos_tx.send(Some(Box::new(payload))).is_err() {
            debug!("mqtt position channel closed — publisher down");
        }
    }

    pub fn phase(&self, phase: FlightPhase, ts: DateTime<Utc>) {
        let payload = PhasePayload {
            ts: ts.timestamp_millis(),
            phase: phase_label(phase),
        };
        let _ = self.tx.try_send(Cmd::Phase(payload));
    }

    pub fn block(&self, payload: BlockPayload) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(
                EVENT_ENQUEUE_TIMEOUT,
                tx.send(Cmd::Block(Box::new(payload))),
            )
            .await;
        });
    }

    pub fn takeoff(&self, payload: TakeoffPayload) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(
                EVENT_ENQUEUE_TIMEOUT,
                tx.send(Cmd::Takeoff(Box::new(payload))),
            )
            .await;
        });
    }

    /// Einen Zuruf abschicken. `an_pilot_id` gesetzt = Direktnachricht.
    ///
    /// `try_send`: schlaegt der Kanal fehl, geht der Zuruf verloren statt die
    /// Oberflaeche zu blockieren. Das ist bei einem Chat richtig — anders als
    /// bei einer Landung, die pro Flug genau einmal entsteht.
    pub fn chat(&self, text: String, an_pilot_id: Option<String>) -> bool {
        self.tx
            .try_send(Cmd::Chat(Box::new(ChatSenden {
                ts: chrono::Utc::now().timestamp_millis(),
                text,
                an_pilot_id,
            })))
            .is_ok()
    }

    pub fn touchdown(&self, payload: TouchdownPayload) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                EVENT_ENQUEUE_TIMEOUT,
                tx.send(Cmd::Touchdown(Box::new(payload))),
            )
            .await
            {
                warn!("dropping touchdown publish: {e}");
            }
        });
    }

    pub fn pirep(&self, payload: PirepPayload) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                EVENT_ENQUEUE_TIMEOUT,
                tx.send(Cmd::Pirep(Box::new(payload))),
            )
            .await
            {
                warn!("dropping pirep publish: {e}");
            }
        });
    }

    /// v0.12.5 (LE1): publisht ein bereits als JSON serialisiertes
    /// PIREP-Payload aufs `pirep`-Topic. Gleiches Wire-Format wie
    /// `pirep()` — der Recorder sieht keinen Unterschied. Genutzt vom
    /// Filing-Refactor (`finalize_filed_pirep`) für alle Filing-Pfade.
    pub fn pirep_json(&self, payload: serde_json::Value) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                EVENT_ENQUEUE_TIMEOUT,
                tx.send(Cmd::PirepJson(Box::new(payload))),
            )
            .await
            {
                warn!("dropping pirep_json publish: {e}");
            }
        });
    }

    /// v0.7.19 GAF-707 (QS-R2 Finding 1): Korrektur-Publish nach Pilot-
    /// Override im Flight-End-Dialog. Recorder/VPS aktualisiert den
    /// bereits persistierten Touchdown-Row entsprechend.
    pub fn touchdown_accident_override(&self, payload: TouchdownAccidentOverridePayload) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                EVENT_ENQUEUE_TIMEOUT,
                tx.send(Cmd::TouchdownAccidentOverride(Box::new(payload))),
            )
            .await
            {
                warn!("dropping touchdown_accident_override publish: {e}");
            }
        });
    }

    /// v0.12.4 (Spec LE4): Publish des FINALEN `rollout_distance_m` nach
    /// Rollout-Finalisierung (~40 kt / Heading-Turn-off). Der Recorder patcht
    /// damit nur das Anzeige-/Forensik-Rohfeld der Touchdown-Zeile.
    pub fn shutdown(&self) {
        let _ = self.tx.try_send(Cmd::Shutdown);
        // v0.19.x FIX: also stop the reconnect-loop drive task — see the
        // field doc comment on `shutdown_tx`.
        let _ = self.shutdown_tx.send(true);
    }
}

pub fn start(cfg: MqttConfig) -> Result<Handle> {
    cfg.validate()?;
    let identitaet = (cfg.va_prefix.clone(), cfg.pilot_id.clone());
    let buch: Buch = Arc::new(Mutex::new(Zustellbuch::default()));
    let buch_drive = buch.clone();
    let buch_pub = buch.clone();

    let (tx, mut rx) = mpsc::channel::<Cmd>(CMD_BUFFER);
    // v1.5.7 (#mqtt-outage): eigener Weg für Positionen — siehe `Cmd`.
    let (pos_tx, mut pos_rx) = watch::channel::<Option<Box<PositionPayload>>>(None);
    // v1.5.7: Der Drive-Loop weiß als Einziger, ob die Leitung steht.
    // Ohne diese Auskunft schob der Publisher weiter Positionen in den
    // Auftragskanal, bis der voll war. Jetzt: Positionen nur bei
    // stehender Verbindung.
    let (link_tx, link_rx) = watch::channel::<bool>(false);

    let url = Url::parse(&cfg.broker_url)?;
    let port = url.port_or_known_default().unwrap_or(443);
    let scheme = url.scheme().to_string();

    // rumqttc 0.24: für WS/WSS muss broker_addr die VOLLSTÄNDIGE URL sein
    // (mit Scheme + Pfad), nicht nur der Hostname. split_url() liest das
    // Scheme um den Default-Port zu resolven. Bei TCP/TLS dagegen: nur Host.
    let broker_addr: String = match scheme.as_str() {
        "ws" | "wss" => cfg.broker_url.clone(),
        _ => url.host_str().context("no host in broker_url")?.to_string(),
    };

    // v0.5.14: client_id eindeutig pro start()-Aufruf (PID + ms-Timestamp).
    // Falls die Idempotency-Guard im Caller versehentlich umgangen wird
    // (Race zwischen check und insert in `state.mqtt`), würden zwei
    // Clients mit gleichem client_id sich gegenseitig vom Broker kicken
    // (MQTT-Spec: "Client X already connected, closing old connection").
    // Belt-and-suspenders: unterschiedliche IDs → koexistierende Clients
    // wären zwar unschön (doppelte Pubs), aber kein Connection-Drop.
    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let client_id = format!(
        "aeroacars-pilot-{}-{}-{}-{}",
        cfg.va_prefix,
        cfg.pilot_id,
        std::process::id(),
        now_ms
    );
    let status_topic = cfg.topic("status");

    let mut opts = MqttOptions::new(&client_id, &broker_addr, port);
    opts.set_credentials(&cfg.username, &cfg.password);
    opts.set_keep_alive(Duration::from_secs(60));
    opts.set_clean_session(true);
    opts.set_last_will(LastWill::new(
        &status_topic,
        STATUS_OFFLINE,
        QoS::AtLeastOnce,
        true,
    ));

    let transport = match scheme.as_str() {
        "wss" => Transport::Wss(default_tls_config()),
        "ws" => Transport::Ws,
        "mqtts" | "ssl" => Transport::Tls(default_tls_config()),
        "mqtt" | "tcp" => Transport::Tcp,
        s => anyhow::bail!("unsupported scheme: {s}"),
    };
    opts.set_transport(transport);

    info!(client_id = %client_id, broker = %broker_addr, port, "starting MQTT publisher");

    let (client, mut eventloop) = AsyncClient::new(opts, CMD_BUFFER);

    // v0.13.0 Stream F (Slice 6): Unbounded mpsc für Integrity-Flag-Events
    // vom Broker. Hat Eigenrate-Begrenzung (Recorder published nur bei
    // tatsächlichen Flags — < 1/min im normalen Cruise).
    let (integrity_tx, integrity_rx) = mpsc::unbounded_channel::<IntegrityFlagEvent>();
    // Pilotenchat: zweiter Rueckkanal, gleiche Mechanik.
    let (chat_tx, chat_rx) = mpsc::unbounded_channel::<ChatNachricht>();
    let integrity_topic = format!(
        "aeroacars/{}/{}/integrity_flag",
        cfg.va_prefix, cfg.pilot_id
    );
    let chat_topic = format!("aeroacars/{}/{}/chat_in", cfg.va_prefix, cfg.pilot_id);
    let subscribe_client = client.clone();
    let subscribe_topic = integrity_topic.clone();
    let subscribe_chat_topic = chat_topic.clone();

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let _drive = tokio::spawn(async move {
        let mut subscribed = false;
        let mut chat_subscribed = false;
        loop {
            // v0.19.x FIX: race the eventloop poll against the shutdown
            // signal so a logout actually stops this loop instead of
            // leaving it to auto-reconnect forever (rumqttc's poll()
            // treats a deliberate disconnect the same as any other
            // transient error — "reconnect after backoff").
            tokio::select! {
                biased;
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        info!("MQTT drive loop: shutdown signaled, exiting");
                        break;
                    }
                }
                // v1.5.7: poll() bekommt eine Obergrenze für Stille. Ein
                // Timeout ist KEIN Fehler von rumqttc — er bedeutet: von
                // dieser Verbindung kommt nichts mehr, auch keine
                // Fehlermeldung. Genau Michels Fall.
                poll_result = tokio::time::timeout(POLL_SILENCE_TIMEOUT, eventloop.poll()) => {
                    let poll_result = match poll_result {
                        Ok(r) => r,
                        Err(_) => {
                            warn!(
                                "MQTT: {} s ohne jedes Lebenszeichen — Verbindung gilt als tot, \
                                 Neuaufbau erzwungen",
                                POLL_SILENCE_TIMEOUT.as_secs()
                            );
                            // `clean()` verschiebt Inflight-Nachrichten und
                            // den Auftragskanal nach `pending` — GEDACHT zur
                            // Wiederholung. Weil wir mit `clean_session`
                            // verbinden, meldet der Broker aber nie eine
                            // fortgesetzte Sitzung, und rumqttc leert
                            // `pending` beim nächsten CONNACK. Netto wird
                            // also verworfen — aber über diesen Umweg, nicht
                            // durch `clean()` selbst (QS-Befund: der frühere
                            // Kommentar behauptete das Falsche).
                            //
                            // Für uns ist das Verwerfen richtig: Was hier
                            // liegt, sind fast ausschliesslich Positionen,
                            // die längst überholt sind. Landung und
                            // Flugbericht liegen als `retain` beim Broker,
                            // sobald sie einmal durch sind.
                            eventloop.clean();
                            for p in zustellbuch_leitung_weg(&buch_drive, &mut eventloop) {
                                eingehendes_publish_zustellen(
                                    &p,
                                    &subscribe_topic,
                                    &subscribe_chat_topic,
                                    &integrity_tx,
                                    &chat_tx,
                                );
                            }
                            subscribed = false;
                            chat_subscribed = false;
                            if let Some(auf) = link_state_for(LinkEvent::WatchdogTimeout) {
                                let _ = link_tx.send(auf);
                            }
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    };
                    match poll_result {
                        Ok(Event::Incoming(Packet::ConnAck(_))) => {
                            info!("MQTT CONNACK received");
                            // v1.5.7 (#mqtt-outage, QS-Befund): NICHT-blockierend.
                            //
                            // Hier lag der eigentliche Fünf-Stunden-Hänger aus
                            // Michels Flug — nicht in einer halb offenen Leitung,
                            // wie zuerst vermutet. `subscribe().await` wartet auf
                            // Platz im internen Auftragskanal von rumqttc. Nach
                            // einem längeren Ausfall ist der voll (bei 3-s-Takt
                            // nach ~10 Minuten), und geleert wird er ausgerechnet
                            // von DIESER Schleife. Der Aufruf blockierte also die
                            // Stelle, die ihn hätte freimachen müssen: eine
                            // Selbstverklemmung. Danach ging nichts mehr raus,
                            // nicht einmal ein Lebenspuls — deshalb warf der
                            // Broker den Client nach 90 s (1,5 × Keepalive)
                            // hinaus, und der Client bemerkte fünf Stunden nichts.
                            //
                            // `try_subscribe` gibt bei vollem Kanal sofort auf;
                            // `subscribed` bleibt dann false, und der Versuch
                            // wiederholt sich beim nächsten Ereignis (siehe
                            // Nachzieh-Block unter dem `match`), sobald wieder
                            // Platz ist.
                            try_subscribe_once(
                                &subscribe_client,
                                &subscribe_topic,
                                &mut subscribed,
                            );
                            try_subscribe_once(
                                &subscribe_client,
                                &subscribe_chat_topic,
                                &mut chat_subscribed,
                            );
                            if let Some(auf) = link_state_for(LinkEvent::ConnAck) {
                                let _ = link_tx.send(auf);
                            }
                        }
                        Ok(Event::Incoming(Packet::Publish(publish))) => {
                            eingehendes_publish_zustellen(
                                &publish,
                                &subscribe_topic,
                                &subscribe_chat_topic,
                                &integrity_tx,
                                &chat_tx,
                            );
                        }
                        Ok(Event::Outgoing(Outgoing::Publish(pkid))) => {
                            buch_drive
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .ausgegangen(pkid);
                            if let Some(auf) = link_state_for(LinkEvent::Other) {
                                let _ = link_tx.send(auf);
                            }
                        }
                        Ok(Event::Incoming(Packet::PubAck(ack))) => {
                            buch_drive
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .bestaetigt(ack.pkid);
                            if let Some(auf) = link_state_for(LinkEvent::Other) {
                                let _ = link_tx.send(auf);
                            }
                        }
                        Ok(_) => {
                            // Ausgehende Bestätigungen, PINGRESP, sonstiges
                            // Eingehendes: kein Zustandswechsel. Läuft
                            // trotzdem durch `link_state_for`, damit die
                            // Entscheidung „was ändert den Leitungszustand"
                            // an genau EINER Stelle steht.
                            if let Some(auf) = link_state_for(LinkEvent::Other) {
                                let _ = link_tx.send(auf);
                            }
                        }
                        Err(e) => {
                            warn!("MQTT poll error: {e} — backing off 5 s");
                            // rumqttc hat `clean()` schon gefahren: Unbestaetigtes
                            // und die Auftraege aus dem Kanal liegen in `pending`
                            // und fallen beim naechsten CONNACK (clean_session).
                            for p in zustellbuch_leitung_weg(&buch_drive, &mut eventloop) {
                                eingehendes_publish_zustellen(
                                    &p,
                                    &subscribe_topic,
                                    &subscribe_chat_topic,
                                    &integrity_tx,
                                    &chat_tx,
                                );
                            }
                            subscribed = false;  // re-subscribe on reconnect
                            chat_subscribed = false;
                            // Leitung weg → keine Positionen mehr in den
                            // Auftragskanal schieben (siehe `link_tx`).
                            if let Some(auf) = link_state_for(LinkEvent::PollError) {
                                let _ = link_tx.send(auf);
                            }
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                    // v1.5.7: Nachziehen. Scheiterte `try_subscribe` beim CONNACK
                    // am vollen Kanal, holen wir es hier nach — bei jedem
                    // Ereignis, kostenlos, solange es offen ist. Ohne das bliebe
                    // die Anmeldung bis zum nächsten Verbindungsaufbau aus.
                    if !subscribed {
                        try_subscribe_once(
                            &subscribe_client,
                            &subscribe_topic,
                            &mut subscribed,
                        );
                    }
                    if !chat_subscribed {
                        try_subscribe_once(
                            &subscribe_client,
                            &subscribe_chat_topic,
                            &mut chat_subscribed,
                        );
                    }
                }
            }
        }
        debug!("MQTT drive loop exiting");
    });

    let cfg_for_pub = cfg.clone();
    let pub_client = client.clone();
    let _publisher = tokio::spawn(async move {
        if !publish_registriert_wartend(
            &pub_client,
            &buch_pub,
            &cfg_for_pub.topic("status"),
            QoS::AtLeastOnce,
            true,
            STATUS_ONLINE.as_bytes().to_vec(),
            None,
        )
        .await
        {
            warn!("initial status publish failed");
        }

        // v0.6.2 — Initial Phase-Publish ENTFERNT. Vorher wurde hier
        // unconditional `FlightPhase::Preflight` retained gepublisht.
        // Das überschreibt die echte Phase im Broker beim App-Restart
        // (Pilot war im CLIMB → quittete → restartete → MQTT-Handle init
        // sendete PREFLIGHT → Live-Map zeigte für ~5s PREFLIGHT bis der
        // Streamer den ersten position-payload mit echter Phase sendet).
        //
        // Pilot-Report 2026-05-10 (Test-Flight CFG 785 EDDV->EDDB):
        // Indikator zeigte „PREFLIGHT" auf Live-Map nach Resume bei
        // 12k ft im Climb.
        //
        // Stattdessen: KEIN initial publish. Der Streamer sendet beim
        // ersten Tick die ECHTE Phase im position-payload (das embed
        // wurde in v0.5.14 nachgezogen). Wenn kein Flug aktiv → Monitor
        // zeigt „—" (korrekt, kein Flug = keine Phase).
        //
        // Der retained-message vom letzten Flug bleibt im Broker bis
        // der nächste Streamer-Tick eine neue Phase sendet — das ist
        // OK weil der Subscriber den position-payload schneller sieht
        // als ein Monitor connected.

        // v1.5.7 (#mqtt-outage): Ereignisse und Positionen aus ZWEI Quellen,
        // `biased` = Ereignisse haben Vorrang. Damit kann ein Positionsstrom
        // eine Landung oder ein PIREP nie wieder aushungern — der Fall, der
        // Michels Flug die halbe Auswertung gekostet hat.
        loop {
            let cmd = tokio::select! {
                biased;
                maybe = rx.recv() => match maybe {
                    Some(c) => c,
                    None => break, // Sender weg → Ende
                },
                changed = pos_rx.changed() => {
                    if changed.is_err() {
                        break; // Sender weg → Ende
                    }
                    // v1.5.7 (QS-Runde 2): KEINE Positionen in den
                    // Auftragskanal, solange die Leitung liegt. Genau das
                    // hat ihn im Feldbefund nach ~10 Minuten wieder
                    // vollgeschoben und die Landung ausgesperrt.
                    if !should_publish_position(*link_rx.borrow()) {
                        continue;
                    }
                    // Momentaufnahme ziehen und den Borrow SOFORT beenden —
                    // über ein `.await` darf er nicht gehalten werden.
                    let snapshot = pos_rx.borrow_and_update().clone();
                    if let Some(p) = snapshot {
                        // NICHT `publish_json`: Positionen duerfen den
                        // Auftragskanal nicht belegen (siehe dort).
                        publish_position_lossy(
                            &pub_client,
                            &buch_pub,
                            &cfg_for_pub.topic("position"),
                            &p,
                        );
                    }
                    continue;
                }
            };
            match cmd {
                Cmd::Chat(c) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("chat"),
                        &c,
                        QoS::AtLeastOnce,
                        false,
                    )
                    .await
                }
                Cmd::Phase(p) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("phase"),
                        &p,
                        QoS::AtLeastOnce,
                        true,
                    )
                    .await
                }
                Cmd::Block(p) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("block"),
                        &p,
                        QoS::AtLeastOnce,
                        true,
                    )
                    .await
                }
                Cmd::Takeoff(p) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("takeoff"),
                        &p,
                        QoS::AtLeastOnce,
                        true,
                    )
                    .await
                }
                // retain=true (was false): the end-of-flight touchdown + pirep
                // are each published exactly once. If the recorder is offline at
                // that instant (restart, mosquitto reload, network blip) a
                // non-retained QoS-1 message is lost for good — that is how ~7
                // historical flights ended up with a touchdown but no linked
                // PIREP (→ empty score breakdown). Retaining the last one per
                // pilot lets a reconnecting recorder pick it up. Re-delivery is
                // safe: ingest is idempotent (pireps UNIQUE(pirep_id); touchdown
                // dedups on va/pilot/ts±2s/vs±5fpm with a stable ts), so a
                // retained replay matches the existing row instead of
                // duplicating. The next flight on the topic overwrites it.
                Cmd::Touchdown(p) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("touchdown"),
                        &p,
                        QoS::AtLeastOnce,
                        true,
                    )
                    .await
                }
                Cmd::Pirep(p) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("pirep"),
                        &p,
                        QoS::AtLeastOnce,
                        true,
                    )
                    .await
                }
                Cmd::PirepJson(p) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("pirep"),
                        &p,
                        QoS::AtLeastOnce,
                        true,
                    )
                    .await
                }
                Cmd::TouchdownAccidentOverride(p) => {
                    publish_json(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("touchdown_accident_override"),
                        &p,
                        QoS::AtLeastOnce,
                        false,
                    )
                    .await
                }
                Cmd::TouchdownRolloutFinalized(p, ack) => {
                    // ⚠ Zustellmeldung: Bei liegender Leitung wird NICHT
                    // versucht — der Auftrag wuerde im rumqttc-Vorrat
                    // haengen und nach 20 s verworfen, und der Aufrufer
                    // haette „gesendet" gehoert. `false` heisst: Datei
                    // bleibt in der Ablage, der Worker versucht es wieder.
                    // `ack` wandert ins Zustellbuch und meldet erst mit dem
                    // PUBACK `true` (Runde 13). Bei liegender Leitung oder
                    // abgelehntem Auftrag meldet der Weg selbst `false`.
                    if !*link_rx.borrow() {
                        warn!("touchdown_rollout_finalized: Leitung liegt — nicht gesendet");
                        let _ = ack.send(false);
                    } else {
                        publish_json_bestaetigt(
                            &pub_client,
                            &buch_pub,
                            &cfg_for_pub.topic("touchdown_rollout_finalized"),
                            &p,
                            QoS::AtLeastOnce,
                            false,
                            Some(ack),
                        )
                        .await;
                    }
                }
                Cmd::Shutdown => {
                    let _ = publish_registriert_wartend(
                        &pub_client,
                        &buch_pub,
                        &cfg_for_pub.topic("status"),
                        QoS::AtLeastOnce,
                        true,
                        STATUS_OFFLINE.as_bytes().to_vec(),
                        None,
                    )
                    .await;
                    let _ = pub_client.disconnect().await;
                    break;
                }
            }
        }
        debug!("MQTT cmd loop exiting");
    });

    Ok(Handle {
        tx,
        pos_tx,
        integrity_rx: Arc::new(tokio::sync::Mutex::new(Some(integrity_rx))),
        chat_rx: Arc::new(tokio::sync::Mutex::new(Some(chat_rx))),
        shutdown_tx,
        va_prefix: identitaet.0,
        pilot_id: identitaet.1,
    })
}

/// v1.5.7 (#mqtt-outage): Anmeldung am integrity_flag-Kanal, ohne je zu
/// blockieren. Siehe die ausführliche Begründung am CONNACK-Zweig.
///
/// Diese Funktion ist bewusst SYNCHRON. Das ist der eigentliche Schutz
/// gegen den Rückfall: Wer hier je wieder ein `await` einbaut, bekommt
/// keinen roten Test, sondern einen Übersetzungsfehler — und muss die
/// Signatur ändern, also bewusst hinsehen. Ein Test könnte das nicht
/// besser absichern (QS-Runde 3: eine Mutation ohne `await` verändert das
/// Verhalten gar nicht, eine mit `await` kompiliert nicht).
fn try_subscribe_once(client: &AsyncClient, topic: &str, subscribed: &mut bool) {
    match client.try_subscribe(topic, QoS::AtLeastOnce) {
        Ok(()) => {
            info!(topic = %topic, "Rueckkanal abonniert");
            *subscribed = true;
        }
        Err(e) => {
            // Voller Auftragskanal ist der Normalfall nach einem Ausfall —
            // kein Fehler, nur "später nochmal".
            debug!("Abo auf {topic} zurueckgestellt: {e}");
        }
    }
}

async fn publish_json<T: Serialize>(
    client: &AsyncClient,
    buch: &Buch,
    topic: &str,
    payload: &T,
    qos: QoS,
    retain: bool,
) {
    let _ = publish_json_bestaetigt(client, buch, topic, payload, qos, retain, None).await;
}

/// Wie `publish_json`, mit Zustellmeldung: `meldung` bekommt `true` mit dem
/// PUBACK (ueber das Zustellbuch), `false` wenn der Auftrag nicht angenommen
/// wurde oder die Leitung vorher faellt. Rueckgabe: angenommen ja/nein.
async fn publish_json_bestaetigt<T: Serialize>(
    client: &AsyncClient,
    buch: &Buch,
    topic: &str,
    payload: &T,
    qos: QoS,
    retain: bool,
    meldung: Option<tokio::sync::oneshot::Sender<bool>>,
) -> bool {
    let body = match serde_json::to_vec(payload) {
        Ok(b) => b,
        Err(e) => {
            error!("serialize {topic} failed: {e}");
            if let Some(m) = meldung {
                let _ = m.send(false);
            }
            return false;
        }
    };
    publish_registriert_wartend(client, buch, topic, qos, retain, body, meldung).await
}

/// Leitung weg: alles Unterwegs verloren, und so viele frische Auftraege,
/// wie in `pending` als Publish mit pkid 0 liegen (die kamen nie raus und
/// fallen beim naechsten CONNACK). Danach `pending` leeren — rumqttc taete
/// es beim CONNACK ohnehin (clean_session), und ein zweiter Abriss vor dem
/// CONNACK darf dieselben Auftraege nicht nochmal zaehlen.
/// Stellt ein empfangenes Publish an Integritaets- oder Chat-Empfaenger zu.
///
/// EINE Stelle fuer beide Wege: den normalen `poll()`-Arm und die
/// Ereignisse, die bei einem Leitungsabriss gepuffert in `state.events`
/// liegen. Die zweite Quelle fehlte bis Runde 16 ganz — die Nachricht war
/// weg (Runde 15 legte sie nur zurueck in eine Schlange, die rumqttc erst
/// nach einem geglueckten Reconnect ausliefert; bei anhaltendem Ausfall
/// also nie).
fn eingehendes_publish_zustellen(
    publish: &rumqttc::Publish,
    integrity_topic: &str,
    chat_topic: &str,
    integrity_tx: &mpsc::UnboundedSender<IntegrityFlagEvent>,
    chat_tx: &mpsc::UnboundedSender<ChatNachricht>,
) {
    if publish.topic == integrity_topic {
        match serde_json::from_slice::<IntegrityFlagEvent>(&publish.payload) {
            Ok(evt) => {
                if integrity_tx.send(evt).is_err() {
                    debug!("integrity_flag receiver dropped — discarding");
                }
            }
            Err(e) => warn!("integrity_flag JSON decode failed: {e}"),
        }
    } else if publish.topic == chat_topic {
        match serde_json::from_slice::<ChatNachricht>(&publish.payload) {
            Ok(n) => {
                if chat_tx.send(n).is_err() {
                    debug!("Chat-Empfaenger weg — Zuruf verworfen");
                }
            }
            Err(e) => warn!("Chat-Nachricht nicht lesbar: {e}"),
        }
    }
}

#[must_use = "die geretteten Nachrichten muessen zugestellt werden"]
fn zustellbuch_leitung_weg(
    buch: &Buch,
    eventloop: &mut rumqttc::EventLoop,
) -> Vec<rumqttc::Publish> {
    let mut b = buch.lock().unwrap_or_else(|e| e.into_inner());
    // ⚠ Zuerst die GEPUFFERTEN Ereignisse (Runde 14, High 3): rumqttc legt
    // `Outgoing::Publish(pkid)` in `state.events`, BEVOR es schreibt.
    // Scheitert das Schreiben, laeuft `clean()`, aber das Ereignis bleibt
    // liegen und kaeme nach dem Reconnect als Erstes zurueck — dann saehe
    // das Buch einen frischen Eintrag als unterwegs, obwohl das Paket nie
    // wieder gesendet wird, und ein spaeteres PUBACK derselben pkid traefe
    // den Falschen. Also: jetzt eintragen, dann als verloren melden.
    //
    // ⚠ NUR die zwei buchrelevanten Sorten werden entnommen. Alles andere —
    // allen voran ein bereits empfangenes `Incoming::Publish`, also eine
    // Integritaets- oder Chat-Nachricht — wandert zurueck in die Schlange und
    // wird vom Drive-Loop normal verarbeitet. Die erste Fassung leerte die
    // Schlange ganz und verwarf sie damit lautlos (Codex, 03.09.2026,
    // Runde 15): Bei QoS 0 oder schon gesendetem ACK kommt sie nie wieder.
    let mut gepuffert = 0usize;
    let mut behalten = VecDeque::new();
    let mut gerettet = Vec::new();
    for ev in eventloop.state.events.drain(..) {
        match ev {
            Event::Outgoing(Outgoing::Publish(pkid)) => {
                b.ausgegangen(pkid);
                gepuffert += 1;
            }
            // Ein PUBACK, das schon da war: das ist eine echte Zustellung.
            Event::Incoming(Packet::PubAck(ack)) => b.bestaetigt(ack.pkid),
            // ⚠ Eine bereits EMPFANGENE Nachricht wird SOFORT zugestellt,
            // nicht zurueckgelegt (Runde 16): `poll()` liefert die Schlange
            // erst nach einem geglueckten Reconnect aus — bei anhaltendem
            // Ausfall also nie, und beim Beenden der App gar nicht mehr.
            // Runde 15 hatte sie nur vor dem Wegwerfen bewahrt.
            Event::Incoming(Packet::Publish(p)) => gerettet.push(p),
            andere => behalten.push_back(andere),
        }
    }
    eventloop.state.events.extend(behalten);
    let verschluckte = eventloop
        .pending
        .iter()
        .filter(|r| matches!(r, Request::Publish(p) if p.pkid == 0))
        .count();
    let (frisch, unterwegs) = b.offen();
    if frisch + unterwegs + gepuffert > 0 {
        info!(
            "Zustellbuch: Leitung weg — {unterwegs} unbestaetigt ({gepuffert} davon nur gepuffert), \
             {verschluckte} von {frisch} frischen verschluckt"
        );
    }
    b.verbindung_weg(verschluckte);
    eventloop.pending.clear();
    gerettet
}

/// v1.5.7 (#mqtt-outage, QS-Befund): Positionen NICHT-blockierend senden.
///
/// Der QS-Review hat gezeigt, dass die erste Fassung den Engpass nur
/// verschoben hat: Positionen liefen zwar nicht mehr in der eigenen
/// Warteschlange auf, belegten dafuer aber den internen Auftragskanal von
/// rumqttc (Kapazitaet 200). Eine Landung, die danach kam, wartete auf
/// Platz, der nie frei wurde — und wurde nach 20 s verworfen. Dasselbe
/// Ergebnis wie vorher, nur eine Ebene tiefer.
///
/// Deshalb hier `try_publish`: Ist kein Platz, wird DIESE Position
/// verworfen — sofort und ohne zu warten. Das ist genau richtig, denn
/// drei Sekunden spaeter kommt die naechste, und der Kanal bleibt fuer
/// das frei, was zaehlt: Landung, Flugbericht, Phasenwechsel.
fn publish_position_lossy<T: Serialize>(
    client: &AsyncClient,
    buch: &Buch,
    topic: &str,
    payload: &T,
) {
    let body = match serde_json::to_vec(payload) {
        Ok(b) => b,
        Err(e) => {
            error!("serialize {topic} failed: {e}");
            return;
        }
    };
    // QoS 0 + retain: der Recorder braucht nur den jeweils neuesten Stand.
    // Auch QoS 0 geht durchs Buch: rumqttc meldet `Outgoing::Publish(0)`.
    if let Err(e) = publish_registriert(client, buch, topic, QoS::AtMostOnce, true, body, None) {
        debug!("position publish skipped (Auftragskanal voll): {e}");
    }
}

fn default_tls_config() -> TlsConfiguration {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let cfg = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsConfiguration::Rustls(Arc::new(cfg))
}

fn simulator_label(sim: Simulator) -> &'static str {
    match sim {
        Simulator::Msfs2020 => "MSFS_2020",
        Simulator::Msfs2024 => "MSFS_2024",
        Simulator::XPlane11 => "XP11",
        Simulator::XPlane12 => "XP12",
        Simulator::Other => "OTHER",
    }
}

fn phase_label(p: FlightPhase) -> &'static str {
    // v0.5.18: granular 1:1 mapping of all 17 internal FSM phases to
    // distinct MQTT labels. Pre-v0.5.18 we collapsed 5 pairs/triples
    // (Preflight+Boarding → PREFLIGHT, Pushback+TaxiOut → TAXI_OUT,
    // TakeoffRoll+Takeoff → TAKEOFF, BlocksOn+Arrived+PirepSubmitted
    // → ON_BLOCK) for "simpler live-map" — but this lost data the
    // server needs for proper flight-phase analytics, rotation
    // timing, post-landing state distinction etc. The server-side
    // mapping table is being updated in lockstep.
    match p {
        FlightPhase::Preflight => "PREFLIGHT",
        FlightPhase::Boarding => "BOARDING",
        FlightPhase::Pushback => "PUSHBACK",
        FlightPhase::TaxiOut => "TAXI_OUT",
        FlightPhase::TakeoffRoll => "TAKEOFF_ROLL",
        FlightPhase::Takeoff => "TAKEOFF",
        FlightPhase::Climb => "CLIMB",
        FlightPhase::Cruise => "CRUISE",
        FlightPhase::Holding => "HOLDING",
        FlightPhase::Descent => "DESCENT",
        FlightPhase::Approach => "APPROACH",
        FlightPhase::Final => "FINAL",
        FlightPhase::Landing => "LANDING",
        FlightPhase::TaxiIn => "TAXI_IN",
        FlightPhase::BlocksOn => "BLOCKS_ON",
        FlightPhase::Arrived => "ARRIVED",
        FlightPhase::PirepSubmitted => "PIREP_SUBMITTED",
    }
}

// v0.19.x FIX: `start()`'s drive loop races `eventloop.poll()` (real
// network I/O, not mockable without a live broker) against a
// `watch::Receiver::changed()` shutdown signal inside `tokio::select!`.
// The actual eventloop can't be unit-tested here, but the shutdown
// mechanism itself — the exact correctness property this fix depends on
// — is fully isolatable: does a `biased` select! between "shutdown
// signaled" and "a long-pending branch" (standing in for poll() with no
// events) exit promptly on shutdown, and does it NOT exit spuriously
// when nothing has been signaled?
#[cfg(test)]
mod herkunft_auf_der_leitung {
    use super::*;

    /// ⚠ `#[serde(flatten)]` ist die einzige Zusicherung, dass die
    /// Feldgruppe auf der Leitung genauso aussieht wie vorher.
    ///
    /// Ohne sie waeren die Felder in ein Unterobjekt `herkunft` gerutscht
    /// — der Recorder liest sie flach und haette sie schlicht nicht mehr
    /// gefunden. Nicht mit einem Fehler, sondern als „der Client schickt
    /// sie halt nicht": genau die Stille, aus der der Befund kam.
    ///
    /// Der Test prueft es am ECHTEN JSON, nicht am Attribut im Quelltext.
    #[test]
    fn die_herkunft_liegt_flach_im_json() {
        let herkunft = BahnHerkunftWire {
            bahn_revision: Some(7),
            runway_length_m: Some(3666.0),
            bahn_geometrie_quelle: Some("szenerie".to_string()),
            runway_displaced_threshold_ft: Some(1150),
            aim_class: Some("on_aim".to_string()),
            ..Default::default()
        };
        let nachtrag = TouchdownRolloutFinalizedPayload {
            ts: 1,
            pirep_id: "p1".to_string(),
            touchdown_at: 2,
            rollout_distance_m: 1831.6,
            finalize_reason: None,
            bahn: Some(BahnWire::default()),
            landing_touchdown_zone: None,
            runway_geometry_trusted: None,
            runway_geometry_reason: None,
            herkunft,
        };
        let json = serde_json::to_value(&nachtrag).expect("serialisiert");
        let obj = json.as_object().expect("Objekt");
        assert!(
            obj.get("herkunft").is_none(),
            "die Gruppe steht als Unterobjekt auf der Leitung — der \
             Recorder liest sie flach und findet sie nicht"
        );
        assert_eq!(obj.get("bahn_revision").and_then(|v| v.as_u64()), Some(7));
        assert_eq!(
            obj.get("runway_length_m").and_then(|v| v.as_f64()),
            Some(3666.0)
        );
        assert_eq!(
            obj.get("bahn_geometrie_quelle").and_then(|v| v.as_str()),
            Some("szenerie")
        );
        assert_eq!(
            obj.get("runway_displaced_threshold_ft")
                .and_then(|v| v.as_i64()),
            Some(1150)
        );
        assert_eq!(
            obj.get("aim_class").and_then(|v| v.as_str()),
            Some("on_aim")
        );
        // Und das Drittel geht auch als `null` hinaus — es muss die
        // Spalte loeschen koennen.
        assert!(
            obj.contains_key("landing_touchdown_zone"),
            "`landing_touchdown_zone` fehlt bei None — dann bleibt die Spalte \
             beim Recorder stehen"
        );
        assert!(obj["landing_touchdown_zone"].is_null());
    }

    /// ⚠ Ein leeres Feld der Gruppe muss als `null` auf die Leitung —
    /// nicht fehlen.
    ///
    /// Der Recorder aktualisiert nur Schluessel, die im Ereignis
    /// vorkommen. Wird ein Wert durch die spaetere Zuordnung ungueltig
    /// (ein Korrekturbetrag, weil jetzt nichts mehr uebernommen wurde;
    /// eine Navdaten-Bewertung, die ohne Bahntreffer entfaellt), waere
    /// Weglassen die falsche Auskunft: Der alte Wert bliebe stehen, und
    /// die Zeile zeigte eine Korrektur an, die es nicht mehr gibt.
    ///
    /// Ein einzelnes `skip_serializing_if` an einem dieser Felder
    /// genuegt, um genau dieses eine Feld unkorrigierbar zu machen —
    /// lautlos. Deshalb zaehlt der Test die Schluessel, statt Stichproben
    /// zu nehmen.
    #[test]
    fn ein_leeres_feld_der_gruppe_geht_als_null_hinaus() {
        let json = serde_json::to_value(BahnHerkunftWire::default()).expect("serialisiert");
        let obj = json.as_object().expect("Objekt");
        const NAMEN: [&str; 30] = [
            "bahn_revision",
            "bahn_spur_veraltet",
            "runway_match_icao",
            "runway_match_ident",
            "runway_match_distance_m",
            "runway_match_centerline_offset_m",
            "runway_length_m",
            "bahn_geometrie_quelle",
            "bahn_szenerie_status",
            "sim_kennung",
            "bahn_kurs_korrektur_grad",
            "bahn_breiten_korrektur_m",
            "bahn_schwellen_korrektur_m",
            "navdata_source",
            "navdata_cycle",
            "runway_true_course_deg",
            "runway_displaced_threshold_ft",
            "runway_tch_expected_ft",
            "runway_glideslope_angle_deg",
            "td_distance_from_threshold_m",
            "td_in_tdz",
            "td_third",
            "td_tdz_length_m",
            "aim_delta_m",
            "aim_class",
            "aim_point_m",
            "tch_actual_ft",
            "tch_delta_ft",
            "tch_class",
            "pre_displaced_threshold",
        ];
        // ⚠ ZUERST die Anzahl. Ein NEUES Feld mit `skip_serializing_if`
        // steht nicht in dieser Liste — die Namensprobe unten faende es
        // nie. Die Anzahl faengt es: Sie muss mit der Liste wachsen, und
        // wer ein Feld anlegt, muss es hier eintragen (externe QS,
        // 02.09.2026, P2-F).
        assert_eq!(
            obj.len(),
            NAMEN.len(),
            "die Gruppe hat {} Schluessel auf der Leitung, die Liste kennt {} — \
             entweder fehlt ein neues Feld in der Liste, oder eines faellt bei \
             `None` weg",
            obj.len(),
            NAMEN.len()
        );
        let fehlend: Vec<&str> = NAMEN
            .into_iter()
            .filter(|k| !obj.contains_key(*k))
            .collect();
        assert!(
            fehlend.is_empty(),
            "diese Felder fallen bei `None` von der Leitung und lassen \
             damit den alten Wert beim Recorder stehen: {fehlend:?}"
        );
        // Und sie stehen wirklich als JSON-Null da, nicht als "".
        assert!(
            obj.values().all(|v| v.is_null()),
            "ein leeres Feld traegt einen Ersatzwert statt null"
        );
    }

    /// ⚠ Der Nachtrag ueberlebt den Weg durch die PIREP-Warteschlange.
    ///
    /// Beim Offline-Einreichen wird er als JSON aufgehoben und vom Worker
    /// wieder aufgebaut — der einzige neue Deserialisierungspfad dieses
    /// Releases, mit ZWEI `#[serde(flatten)]` nebeneinander. Scheitert
    /// er, geht die Korrektur mit einer Warnzeile verloren (externe QS,
    /// Runde 3).
    #[test]
    fn der_nachtrag_ueberlebt_die_warteschlange() {
        let hin = TouchdownRolloutFinalizedPayload {
            ts: 1,
            pirep_id: "p1".to_string(),
            touchdown_at: 2,
            rollout_distance_m: 1831.6,
            finalize_reason: Some("exit_speed".to_string()),
            bahn: Some(BahnWire {
                rollout_final: true,
                clearance_point_m: Some(1889.4),
                lateral_samples: Some(vec![LateralSampleWire {
                    laengs_m: 720.0,
                    quer_m: -2.1,
                }]),
                ..Default::default()
            }),
            landing_touchdown_zone: Some(2),
            runway_geometry_trusted: Some(true),
            runway_geometry_reason: None,
            herkunft: BahnHerkunftWire {
                bahn_revision: Some(7),
                runway_length_m: Some(3666.0),
                runway_displaced_threshold_ft: Some(1150),
                aim_class: Some("on_aim".to_string()),
                ..Default::default()
            },
        };
        let json = serde_json::to_value(&hin).expect("hin");
        let zurueck = TouchdownRolloutFinalizedPayload::aus_json(json.clone())
            .expect("zurueck — der Nachtrag ist nicht lesbar");
        let json2 = serde_json::to_value(&zurueck).expect("erneut");
        assert_eq!(
            json, json2,
            "der Nachtrag veraendert sich auf dem Weg durch die Warteschlange"
        );
        assert_eq!(zurueck.herkunft.bahn_revision, Some(7));
        assert_eq!(zurueck.landing_touchdown_zone, Some(2));
        assert_eq!(
            zurueck
                .bahn
                .and_then(|b| b.lateral_samples)
                .map(|v| v.len()),
            Some(1)
        );
    }

    /// ⚠ Ohne Spur-Block stehen KEINE Spur-Felder auf der Leitung.
    ///
    /// `rollout_final` und `lateral_samples` duerfen nicht als `false`
    /// bzw. `null` erscheinen — der Recorder liest beides als Aussage
    /// und ueberschreibt eine endgueltige Zeile (Runde 4, N13).
    #[test]
    fn ohne_spur_block_fehlen_die_spur_felder() {
        let n = TouchdownRolloutFinalizedPayload {
            ts: 1,
            pirep_id: "p1".to_string(),
            touchdown_at: 2,
            rollout_distance_m: 1831.6,
            finalize_reason: None,
            bahn: None,
            landing_touchdown_zone: Some(1),
            runway_geometry_trusted: Some(true),
            runway_geometry_reason: None,
            herkunft: BahnHerkunftWire {
                bahn_revision: Some(2),
                ..Default::default()
            },
        };
        let json = serde_json::to_value(&n).expect("json");
        let obj = json.as_object().expect("Objekt");
        for k in [
            "rollout_final",
            "lateral_samples",
            "clearance_point_m",
            "overrun_m",
        ] {
            assert!(
                !obj.contains_key(k),
                "`{k}` steht auf der Leitung, obwohl der Spur-Block fehlt — der \
                 Recorder liest das als Aussage"
            );
        }
        assert_eq!(obj["bahn_revision"], 2);
        assert_eq!(obj["landing_touchdown_zone"], 1);
        // Und zurueck ueber den Warteschlangen-Weg: der fehlende Block
        // liest sich als `None` — NICHT als Block voller Nullen mit
        // `rollout_final: false`, was `from_value` allein liefert (Runde 5,
        // N23). Die erste Fassung dieser Zusicherung war `is_none() ||
        // !rollout_final` — bei `Some(default)` immer wahr, also nichts.
        let roh: TouchdownRolloutFinalizedPayload =
            serde_json::from_value(json.clone()).expect("roh");
        assert!(
            roh.bahn.is_some(),
            "die Vorrichtung zeigt das serde-Verhalten nicht mehr — dann prueft \
             dieser Test nichts"
        );
        let zurueck = TouchdownRolloutFinalizedPayload::aus_json(json).expect("aus_json");
        assert!(
            zurueck.bahn.is_none(),
            "der fehlende Spur-Block wird als leerer Block gelesen — der Worker \
             sendet dann `rollout_final: false` und lauter null"
        );
        let json2 = serde_json::to_value(&zurueck).expect("erneut");
        assert!(
            json2.get("rollout_final").is_none(),
            "nach dem Rundlauf steht `rollout_final` wieder auf der Leitung"
        );
    }

    /// Und dieselbe Gruppe liegt genauso flach am `touchdown_complete`.
    ///
    /// ⚠ Die Gegenprobe zur Gegenprobe: Der Nachtrag koennte flach sein
    /// und das erste Ereignis verschachtelt — dann waere die Zeile beim
    /// Recorder nach dem Nachtrag richtig und vorher leer. Beide Wege
    /// muessen dieselbe Form haben.
    #[test]
    fn dieselbe_gruppe_liegt_am_touchdown_ebenso_flach() {
        let mut td = TouchdownPayload::default();
        td.herkunft.runway_length_m = Some(3666.0);
        td.herkunft.bahn_revision = Some(7);
        let json = serde_json::to_value(&td).expect("serialisiert");
        let obj = json.as_object().expect("Objekt");
        assert!(obj.get("herkunft").is_none(), "verschachtelt statt flach");
        assert_eq!(
            obj.get("runway_length_m").and_then(|v| v.as_f64()),
            Some(3666.0)
        );
        assert_eq!(obj.get("bahn_revision").and_then(|v| v.as_u64()), Some(7));
    }
}

#[cfg(test)]
mod zustellbuch_tests {
    use super::*;

    fn meldung() -> (
        tokio::sync::oneshot::Sender<bool>,
        tokio::sync::oneshot::Receiver<bool>,
    ) {
        tokio::sync::oneshot::channel()
    }
    fn stand(rx: &mut tokio::sync::oneshot::Receiver<bool>) -> Option<bool> {
        rx.try_recv().ok()
    }

    /// Reihenfolge: QoS-0-Positionen nehmen ihren Eintrag mit, der Nachtrag
    /// wartet unter seiner pkid und meldet erst mit dem PUBACK.
    #[test]
    fn der_nachtrag_meldet_erst_mit_dem_puback() {
        let mut b = Zustellbuch::default();
        let (tx, mut rx) = meldung();
        b.registrieren(None); // Position (QoS 0)
        b.registrieren(Some(tx)); // Nachtrag
        b.registrieren(None); // Phase
        b.ausgegangen(0);
        assert_eq!(
            stand(&mut rx),
            None,
            "die Position hat die Nachtragsmeldung genommen"
        );
        b.ausgegangen(7);
        assert_eq!(stand(&mut rx), None, "Outgoing ist kein PUBACK");
        b.ausgegangen(8);
        b.bestaetigt(8);
        assert_eq!(
            stand(&mut rx),
            None,
            "ein fremdes PUBACK hat den Nachtrag bestaetigt"
        );
        b.bestaetigt(7);
        assert_eq!(stand(&mut rx), Some(true));
        assert_eq!(b.offen(), (0, 0));
    }

    /// Leitung weg: Unterwegs → false; und genau die verschluckten frischen
    /// Eintraege fallen vorn weg, damit das Buch nicht versetzt weiterlaeuft.
    #[test]
    fn leitung_weg_meldet_false_und_haelt_das_buch_in_reihe() {
        let mut b = Zustellbuch::default();
        let (t1, mut r1) = meldung();
        let (t2, mut r2) = meldung();
        let (t3, mut r3) = meldung();
        b.registrieren(Some(t1));
        b.ausgegangen(3); // unterwegs, unbestaetigt
        b.registrieren(Some(t2)); // lag beim Abriss im Kanal → verschluckt
        b.registrieren(Some(t3)); // kam nach clean() in den Kanal → ueberlebt
        b.verbindung_weg(1);
        assert_eq!(
            stand(&mut r1),
            Some(false),
            "unbestaetigt ist verloren (clean_session)"
        );
        assert_eq!(
            stand(&mut r2),
            Some(false),
            "der verschluckte Auftrag meldet nicht"
        );
        assert_eq!(
            stand(&mut r3),
            None,
            "der ueberlebende Auftrag wurde faelschlich abgeschrieben"
        );
        b.ausgegangen(1);
        b.bestaetigt(1);
        assert_eq!(
            stand(&mut r3),
            Some(true),
            "das Buch ist versetzt — der Ueberlebende bekam sein PUBACK nicht"
        );
    }

    /// Kollision: dieselbe pkid zweimal unterwegs, PUBACKs in Reihenfolge.
    #[test]
    fn kollision_bestaetigt_in_reihenfolge() {
        let mut b = Zustellbuch::default();
        let (ta, mut ra) = meldung();
        let (tb, mut rb) = meldung();
        b.registrieren(Some(ta));
        b.registrieren(Some(tb));
        b.ausgegangen(2);
        b.ausgegangen(2);
        b.bestaetigt(2);
        assert_eq!(stand(&mut ra), Some(true));
        assert_eq!(stand(&mut rb), None);
        b.bestaetigt(2);
        assert_eq!(stand(&mut rb), Some(true));
    }

    /// ⚠ Ein Schreibfehler laesst `Outgoing::Publish` in `state.events`
    /// liegen (Runde 14, High 3). Die Bereinigung muss es EINTRAGEN, bevor
    /// sie alles Unterwegs abschreibt — sonst bleibt ein Geist im Buch, und
    /// das naechste Paket mit derselben pkid bekommt ein falsches PUBACK.
    #[tokio::test]
    async fn gepufferte_ereignisse_landen_vor_der_bereinigung_im_buch() {
        let buch: Buch = Arc::new(Mutex::new(Zustellbuch::default()));
        let mut el = rumqttc::EventLoop::new(MqttOptions::new("t", "localhost", 1883), 10);
        let (ta, mut ra) = meldung();
        let (tb, mut rb) = meldung();
        let (tc, mut rc) = meldung();
        {
            let mut b = buch.lock().unwrap();
            b.registrieren(Some(ta)); // pkid 3 vergeben, Outgoing nur gepuffert
            b.registrieren(Some(tb)); // lag im Kanal → pending, pkid 0
            b.registrieren(Some(tc)); // kam nach clean() → ueberlebt
        }
        el.state
            .events
            .push_back(Event::Outgoing(Outgoing::Publish(3)));
        // Eine bereits EMPFANGENE Nachricht (Chat/Integritaet) liegt daneben —
        // sie muss die Bereinigung ueberleben (Runde 15).
        el.state
            .events
            .push_back(Event::Incoming(Packet::Publish(rumqttc::Publish::new(
                "chat",
                QoS::AtMostOnce,
                "hallo",
            ))));
        el.pending.push_back(Request::Publish(rumqttc::Publish::new(
            "t",
            QoS::AtLeastOnce,
            "x",
        )));
        let gerettet = zustellbuch_leitung_weg(&buch, &mut el);
        assert_eq!(
            stand(&mut ra),
            Some(false),
            "das nur gepufferte Paket gilt nicht als verloren"
        );
        assert_eq!(
            stand(&mut rb),
            Some(false),
            "der verschluckte Auftrag meldet nicht"
        );
        assert_eq!(stand(&mut rc), None, "der Ueberlebende wurde abgeschrieben");
        // Die empfangene Nachricht wird HERAUSGEREICHT — nicht in der
        // Schlange geparkt, die `poll()` erst nach einem geglueckten
        // Reconnect ausliefert (Runde 16).
        assert_eq!(
            gerettet.len(),
            1,
            "die empfangene Nachricht wurde nicht herausgereicht"
        );
        assert_eq!(gerettet[0].topic, "chat");
        assert!(
            el.state.events.is_empty(),
            "das Geist-Ereignis liegt noch — es kaeme nach dem Reconnect zurueck"
        );
        assert!(el.pending.is_empty());
        // Nach dem Reconnect: pkid 3 wird neu vergeben — und trifft den Richtigen.
        let mut b = buch.lock().unwrap();
        b.ausgegangen(3);
        b.bestaetigt(3);
        assert_eq!(
            stand(&mut rc),
            Some(true),
            "das Buch ist versetzt — der Geist hat das PUBACK genommen"
        );
        assert_eq!(b.offen(), (0, 0));
    }

    /// ⚠ Die gerettete Nachricht erreicht ihren Empfaenger OHNE Reconnect
    /// (Runde 16). Zurueckgelegt in `state.events` haette sie bei
    /// anhaltendem Ausfall nie jemand gesehen.
    #[tokio::test]
    async fn die_gerettete_nachricht_erreicht_den_empfaenger_ohne_verbindung() {
        let buch: Buch = Arc::new(Mutex::new(Zustellbuch::default()));
        let mut el = rumqttc::EventLoop::new(MqttOptions::new("t", "localhost", 1883), 10);
        assert!(el.network.is_none(), "Ausgangslage: keine Verbindung");
        let (integrity_tx, mut integrity_rx) = mpsc::unbounded_channel::<IntegrityFlagEvent>();
        let (chat_tx, mut chat_rx) = mpsc::unbounded_channel::<ChatNachricht>();
        let chat_json = serde_json::to_vec(&serde_json::json!({
            "id": 1,
            "va_prefix": "GSG",
            "von_pilot_id": "42",
            "ts": 1_755_000_000_000i64,
            "text": "moin",
        }))
        .expect("JSON");
        el.state
            .events
            .push_back(Event::Incoming(Packet::Publish(rumqttc::Publish::new(
                "aeroacars/GSG/42/chat_in",
                QoS::AtMostOnce,
                chat_json,
            ))));
        for p in zustellbuch_leitung_weg(&buch, &mut el) {
            eingehendes_publish_zustellen(
                &p,
                "aeroacars/GSG/42/integrity_flag",
                "aeroacars/GSG/42/chat_in",
                &integrity_tx,
                &chat_tx,
            );
        }
        assert!(
            el.network.is_none(),
            "der Test darf keine Verbindung aufbauen — genau darum geht es"
        );
        let n = chat_rx
            .try_recv()
            .expect("der Zuruf erreichte den Empfaenger nicht — er wartet auf einen Reconnect, der ausbleiben kann");
        assert_eq!(n.text, "moin");
        assert!(
            integrity_rx.try_recv().is_err(),
            "der Zuruf ging an den falschen Kanal"
        );
    }

    /// Ein Outgoing ohne Eintrag bringt das Buch nicht zum Absturz und nimmt
    /// niemandem die Meldung.
    #[test]
    fn outgoing_ohne_eintrag_ist_laut_aber_harmlos() {
        let mut b = Zustellbuch::default();
        b.ausgegangen(5);
        b.bestaetigt(5);
        assert_eq!(b.offen(), (0, 0));
    }
}

#[cfg(test)]
mod nachtrag_sender_tests {
    use super::*;

    /// ⚠ Ohne Publisher (Kanal geschlossen) meldet `senden` `false` —
    /// nicht „nichts". Der Aufrufer wartet sonst bis zur Frist und haelt
    /// die Datei trotzdem; aber die Meldung muss kommen, damit der Worker
    /// den naechsten Versuch planen kann.
    #[tokio::test]
    async fn ohne_publisher_kommt_false() {
        let (tx, rx) = mpsc::channel::<Cmd>(4);
        drop(rx);
        let s = NachtragSender {
            tx,
            va_prefix: "GSG".into(),
            pilot_id: "42".into(),
        };
        let n = TouchdownRolloutFinalizedPayload::aus_json(serde_json::json!({
            "ts": 1, "pirep_id": "p", "touchdown_at": 1, "rollout_distance_m": 1500.0, "finalize_reason": "test"
        }))
        .expect("Testnachtrag");
        let ok = tokio::time::timeout(std::time::Duration::from_secs(5), s.senden(n))
            .await
            .expect("Meldung bleibt aus — der Aufrufer wuerde bis zur Frist warten")
            .unwrap_or(false);
        assert!(
            !ok,
            "ein geschlossener Kanal darf nie als Zustellung gelten"
        );
    }

    /// ⚠ Der Publisher meldet `false`, wenn er den Auftrag faellen laesst
    /// — der Sender reicht das durch, statt es als Zustellung zu lesen.
    #[tokio::test]
    async fn der_publisher_entscheidet() {
        let (tx, mut rx) = mpsc::channel::<Cmd>(4);
        let s = NachtragSender {
            tx,
            va_prefix: "GSG".into(),
            pilot_id: "42".into(),
        };
        let n = TouchdownRolloutFinalizedPayload::aus_json(serde_json::json!({
            "ts": 1, "pirep_id": "p", "touchdown_at": 1, "rollout_distance_m": 1500.0, "finalize_reason": "test"
        }))
        .expect("Testnachtrag");
        let ack = s.senden(n);
        let cmd = rx.recv().await.expect("Auftrag");
        match cmd {
            Cmd::TouchdownRolloutFinalized(_, weiter) => {
                let _ = weiter.send(false);
            }
            _ => panic!("falscher Auftrag"),
        }
        assert_eq!(ack.await.unwrap_or(true), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration as StdDuration;

    /// Mirrors the exact `tokio::select! { biased; changed = ...,
    /// poll_result = ... }` shape used in `start()`'s drive loop, with
    /// `eventloop.poll()` stood in by a long sleep (never resolves within
    /// the test's timeout unless shutdown wins first).
    async fn drive_loop_shape(mut shutdown_rx: watch::Receiver<bool>) {
        loop {
            tokio::select! {
                biased;
                changed = shutdown_rx.changed() => {
                    if changed.is_err() || *shutdown_rx.borrow() {
                        break;
                    }
                }
                _ = tokio::time::sleep(StdDuration::from_secs(3600)) => {
                    // stand-in for an eventloop.poll() that never returns
                    // within the test's own timeout
                }
            }
        }
    }

    #[tokio::test]
    async fn shutdown_signal_breaks_the_drive_loop_promptly() {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(drive_loop_shape(shutdown_rx));

        shutdown_tx.send(true).expect("receiver still alive");

        let result = tokio::time::timeout(StdDuration::from_secs(2), handle).await;
        assert!(
            result.is_ok(),
            "drive loop must exit promptly once shutdown is signaled — \
             this is the exact mechanism that used to leak the reconnect \
             task forever after a local logout"
        );
    }

    #[tokio::test]
    async fn drive_loop_does_not_exit_spuriously_without_a_shutdown_signal() {
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(drive_loop_shape(shutdown_rx));

        // No signal sent — the loop must still be running after a short
        // wait (proves `changed()` doesn't fire on its own / on the
        // initial `false` value).
        let result = tokio::time::timeout(StdDuration::from_millis(200), handle).await;
        assert!(
            result.is_err(),
            "drive loop must NOT exit on its own — only an explicit shutdown() may stop it"
        );
    }

    #[tokio::test]
    async fn dropping_the_sender_also_breaks_the_drive_loop() {
        // Handle::shutdown() is the intended path, but a Handle drop
        // (all senders gone) must not leave the drive loop spinning
        // forever either — `changed()` returns Err once the sender side
        // is gone, and the loop treats that the same as an explicit true.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let handle = tokio::spawn(drive_loop_shape(shutdown_rx));

        drop(shutdown_tx);

        let result = tokio::time::timeout(StdDuration::from_secs(2), handle).await;
        assert!(
            result.is_ok(),
            "a dropped sender must also unblock the drive loop"
        );
    }

    // ---------------------------------------------------------------
    // v1.5.7 (#mqtt-outage) — Feldbefund Michel, TAP58 SBBR→LPPT.
    //
    // Hergang: 4,5 h Netzausfall über dem Atlantik. Der interne
    // Auftragskanal von rumqttc (Kapazität CMD_BUFFER) lief mit Positionen
    // voll. Folge 1: `subscribe().await` beim Wiederverbinden blockierte
    // auf genau diesem Kanal — und blockierte damit die Schleife, die ihn
    // hätte leeren müssen (Selbstverklemmung, 5 h Stille). Folge 2: die
    // Landung wartete auf Platz, der nie frei wurde, und wurde verworfen.
    //
    // Diese Tests arbeiten gegen einen ECHTEN `AsyncClient` mit winzigem
    // Kanal und ungepolltem Eventloop — also gegen dieselbe Mechanik, die
    // Michels Flug zerlegt hat. Die erste Test-Fassung prüfte nur
    // Tokio-Bausteine und wäre grün geblieben, wenn man die halbe
    // Publisher-Schleife löscht (QS-Befund).
    // ---------------------------------------------------------------

    // QS-Runden 2 und 3 haben die Tests hier zweimal als Selbstbestätigung
    // entlarvt (synchrone Funktion in `timeout`; `include_str!`, das den
    // eigenen Assert-Text findet). Die Lehre: Prüfe eine ENTSCHEIDUNG mit
    // echten Eingaben, nicht die Anwesenheit von Quelltext.
    //
    // Deshalb sind die beiden Entscheidungen als eigene Funktionen
    // herausgezogen. Was diese Tests NICHT leisten: nachzuweisen, dass die
    // Funktionen an der richtigen Stelle der Publisher-Schleife aufgerufen
    // werden. Das sichert nur das Lesen des Codes — ein echter Nachweis
    // bräuchte einen MQTT-Broker im Test.

    /// Die Zuordnung Ereignis → gemeldeter Leitungszustand. Ohne sie ist
    /// die Positions-Sperre wirkungslos: Meldet niemand "Leitung weg",
    /// läuft der Auftragskanal wieder voll; meldet niemand "Leitung da",
    /// gehen nie wieder Positionen raus.
    #[test]
    fn the_link_state_follows_the_drive_loop_events() {
        assert_eq!(link_state_for(LinkEvent::ConnAck), Some(true));
        assert_eq!(link_state_for(LinkEvent::PollError), Some(false));
        assert_eq!(link_state_for(LinkEvent::WatchdogTimeout), Some(false));
        assert_eq!(
            link_state_for(LinkEvent::Other),
            None,
            "normaler Verkehr darf den Zustand nicht umschalten"
        );
    }

    /// Der Kern der Positions-Sperre: Bei liegender Leitung darf keine
    /// Position in den Auftragskanal. Genau das hat ihn im Feldbefund
    /// gefüllt und die Landung ausgesperrt.
    #[test]
    fn positions_only_go_out_while_the_link_is_up() {
        assert!(
            should_publish_position(true),
            "steht die Leitung, wird gesendet"
        );
        assert!(
            !should_publish_position(false),
            "liegt die Leitung, darf NICHTS in den Auftragskanal — sonst \
             verstopft er und die Landung kommt nicht mehr durch"
        );
    }

    /// Einmalige Ereignisse (Landung, Flugbericht) bekommen eine
    /// großzügige Einreih-Frist — sie entstehen pro Flug genau einmal.
    #[test]
    fn one_shot_events_get_a_generous_deadline() {
        assert!(
            EVENT_ENQUEUE_TIMEOUT >= Duration::from_secs(10),
            "eine Viertelsekunde hat Michels Landung gekostet — nie wieder"
        );
    }

    /// Die Fristen müssen zueinander passen: Ein einzelner Sendeversuch
    /// muss vor dem Wächter aufgeben, sonst greift dieser nie.
    #[test]
    fn the_timeouts_are_ordered_sensibly() {
        assert!(PUBLISH_TIMEOUT >= Duration::from_secs(10));
        assert!(PUBLISH_TIMEOUT < POLL_SILENCE_TIMEOUT);
        // Der Wächter ist das ZWEITE Netz: rumqttc erkennt eine tote
        // Leitung über den Keepalive selbst (≤2 Perioden). Er darf deshalb
        // später greifen — aber nicht so spät, dass ein Flug darunter
        // leidet.
        assert!(POLL_SILENCE_TIMEOUT > Duration::from_secs(120));
        assert!(POLL_SILENCE_TIMEOUT <= Duration::from_secs(300));
    }
    /// Die Bahndisziplin-Werte muessen auf der Leitung FLACH liegen.
    ///
    /// `flatten` ist bequem, aber es ist auch die Art von Bequemlichkeit,
    /// die man erst bemerkt, wenn sie schiefgeht: Ohne das Attribut lande
    /// alles unter einem Schluessel `bahn`, der Webapp-Mapper faende
    /// nichts, und die Anzeige zeigte fuer jede Landung „nicht erfasst" —
    /// ohne dass irgendwo ein Fehler auftaucht.
    #[test]
    fn bahnfelder_liegen_flach_auf_der_leitung() {
        let w = BahnWire {
            rollout_final: true,
            // EDDH 23: 156 m versetzte Schwelle, die NICHT in der
            // Geometrie steckt — die Spurwerte laufen also 156 m vor
            // der Landeschwelle los.
            spur_nullpunkt_versatz_m: Some(156.0),
            clearance_point_m: Some(1831.6),
            scoring_cutoff_m: Some(1642.0),
            // Das Messfenster schliesst frueher als der Kurswechsel —
            // siehe die Herleitung am Feld selbst.
            mess_ende_laengs_m: Some(1210.0),
            clearance_speed_kt: Some(24.0),
            clearance_side: Some("left".to_string()),
            track_width_m: Some(7.59),
            track_width_source: Some("aircraft_file".to_string()),
            wingspan_m: Some(35.8),
            runway_width_m: Some(46.0),
            min_edge_clearance_m: Some(9.2),
            max_lateral_offset_m: Some(-13.4),
            lateral_samples: Some(vec![
                LateralSampleWire {
                    laengs_m: 523.2,
                    quer_m: -5.7,
                },
                LateralSampleWire {
                    laengs_m: 561.0,
                    quer_m: -6.1,
                },
            ]),
            surface_paved: Some(true),
            overrun_m: None,
            lateral_skip_reason: None,
            runway_exits: None,
        };
        let j: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&w).unwrap()).unwrap();

        assert!(
            j.get("bahn").is_none(),
            "die Gruppe darf nicht verschachtelt sein"
        );
        assert_eq!(j["clearance_point_m"], 1831.6);
        assert_eq!(j["scoring_cutoff_m"], 1642.0);
        // Und das Fensterende daneben. Ohne diese Zeile faellt nur auf,
        // dass das Feld existiert — nicht, dass es die Leitung erreicht.
        assert_eq!(j["mess_ende_laengs_m"], 1210.0);
        // Und als `null`, wenn es keinen Wert gibt — sonst kann der
        // Recorder einen alt gewordenen Wert nie mehr raeumen (RFC 7396:
        // `null` loescht, fehlend laesst stehen). Diese Zeile haelt das
        // Feld bei seinen Geschwistern.
        {
            let mut leer = w.clone();
            leer.mess_ende_laengs_m = None;
            let jl: serde_json::Value =
                serde_json::from_str(&serde_json::to_string(&leer).unwrap()).unwrap();
            assert!(
                jl.get("mess_ende_laengs_m").is_some(),
                "Feld faellt bei None ganz aus der Nachricht",
            );
            assert!(jl["mess_ende_laengs_m"].is_null());
        }
        assert_eq!(j["clearance_side"], "left");
        assert_eq!(j["track_width_source"], "aircraft_file");
        assert_eq!(j["lateral_samples"][1]["quer_m"], -6.1);
        // Nicht erfasst heisst `null`, nicht FEHLT.
        //
        // Hier stand das Gegenteil, mit der Begruendung „der Datensatz
        // bleibt kleiner". Der Preis dafuer war, dass ein Nachtrag nichts
        // loeschen kann: Der Recorder patcht mit `json_patch` (RFC 7396),
        // dort loescht `null` das Feld und ein fehlendes Feld laesst den
        // alten Wert stehen.
        //
        // Konkret blieb `clearance_speed_kt` auf dem vorlaeufigen Wert
        // stehen, obwohl die Nachrechnung ihn bewusst verworfen hatte.
        assert!(
            j.get("overrun_m").is_some_and(|v| v.is_null()),
            "leere Felder muessen als null gehen, sonst koennen sie nichts loeschen"
        );
    }

    /// Der Payload muss durch den Broker passen.
    ///
    /// `max_packet_size 65536` steht in der mosquitto-Konfiguration auf
    /// dem Live-Server. Was darueber liegt, wird verworfen — die Landung
    /// kaeme nie an, und zwar ohne Fehlermeldung beim Piloten.
    ///
    /// Gemessen am 23.08.2026: Die bisherigen Touchdown-Payloads sind im
    /// Mittel 3,9 KB gross, im schlimmsten Fall 5,0 KB (1022 Landungen).
    /// Eine volle Spur mit 400 Punkten kommt gerundet auf 13 KB dazu, die
    /// Ausfahrten auf 0,6 KB. Zusammen bleibt genug Luft — aber wer die
    /// Punktzahl erhoeht oder die Rundung wieder herausnimmt, sollte diese
    /// Rechnung sehen.
    #[test]
    fn volle_spur_passt_durch_den_broker() {
        const BROKER_GRENZE: usize = 65_536;
        let spur: Vec<LateralSampleWire> = (0..400)
            .map(|i| LateralSampleWire {
                laengs_m: (5232 + i * 73) as f64 / 10.0,
                quer_m: -(57 + i * 37) as f64 / 10.0,
            })
            .collect();
        let w = BahnWire {
            rollout_final: true,
            lateral_samples: Some(spur),
            runway_exits: Some(
                (0..12)
                    .map(|i| RunwayExitWire {
                        name: format!("S{i}"),
                        laengs_m: 1831.6,
                        seite: "left".to_string(),
                        verlauf: vec![VerlaufspunktWire {
                            laengs_m: 1820.0,
                            quer_m: 2.0,
                        }],
                    })
                    .collect(),
            ),
            clearance_point_m: Some(1831.6),
            ..Default::default()
        };
        let bytes = serde_json::to_string(&w).unwrap().len();
        // Grosszuegig gerechnet: der Rest des Payloads obendrauf.
        let gesamt = bytes + 6 * 1024;
        assert!(
            gesamt < BROKER_GRENZE,
            "Bahndaten {} KB + 6 KB Rest = {} KB, der Broker nimmt {} KB",
            bytes / 1024,
            gesamt / 1024,
            BROKER_GRENZE / 1024
        );

        // Und die Rundung muss wirken: ungerundet waeren es rund 23 KB.
        assert!(
            bytes < 16 * 1024,
            "die Spur ist {} KB gross — wird sie noch auf Dezimeter gerundet?",
            bytes / 1024
        );
    }

    /// Eine leere Spur geht als `null`, nicht als `[]`.
    ///
    /// Ein leeres Array sieht in der Anzeige aus wie eine Messung, die
    /// nichts gefunden hat — von „fuer diesen Flug nicht erfasst" ist es
    /// nicht zu unterscheiden. `null` dagegen loescht beim Nachtrag einen
    /// vorlaeufigen Wert, statt ihn stehen zu lassen.
    #[test]
    fn leere_spur_geht_als_null_auf_die_leitung() {
        let j: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&BahnWire::default()).unwrap()).unwrap();
        let o = j.as_object().unwrap();
        assert!(!o.is_empty(), "die Gruppe darf nicht leer serialisieren");
        // `rollout_final` ist eine KENNZEICHNUNG, keine Messung: `false`
        // heisst „Zwischenstand", und das ist eine Aussage, kein
        // fehlender Wert. Alle uebrigen Felder sind Messwerte — dort ist
        // `null` das ehrliche „nicht gemessen".
        let messwerte: Vec<(&String, &serde_json::Value)> =
            o.iter().filter(|(k, _)| *k != "rollout_final").collect();
        assert!(
            messwerte.iter().all(|(_, v)| v.is_null()),
            "ein leeres BahnWire darf ausser der Kennzeichnung nur \
             Nullwerte tragen: {j}"
        );
        assert_eq!(
            o.get("rollout_final"),
            Some(&serde_json::Value::Bool(false)),
            "ein leeres BahnWire muss sich als Zwischenstand ausweisen"
        );
        assert!(
            o["lateral_samples"].is_null(),
            "eine leere Spur darf nicht als [] gehen"
        );
    }

    /// Ein Datensatz von VOR v1.7.0 muss weiter lesbar sein.
    #[test]
    fn alter_payload_ohne_bahnfelder_bleibt_lesbar() {
        let w: BahnWire = serde_json::from_str("{}").unwrap();
        assert!(w.clearance_point_m.is_none());
        assert!(w.lateral_samples.is_none());
    }
}
