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
//! `touchdown`/`pirep` are events, not state, and use `retain=false`.

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rumqttc::{AsyncClient, Event, LastWill, MqttOptions, Packet, QoS, TlsConfiguration, Transport};
use serde::Serialize;
use sim_core::{FlightPhase, SimSnapshot, Simulator};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use url::Url;

pub mod provision;
pub mod log_upload;
pub mod navdata;

const STATUS_ONLINE: &str = "online";
const STATUS_OFFLINE: &str = "offline";

/// Bounded queue between caller and MQTT-publisher task. ~5 s of position
/// ticks at the fastest cadence (5 s/tick → ~1 msg buffered on average,
/// burst tolerance of 200 msgs).
const CMD_BUFFER: usize = 200;

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

    // ---- Position ----
    lat: f64,
    lon: f64,
    alt_ft: i32,           // MSL altitude
    agl_ft: i32,           // Above-ground (for approach/landing analysis)

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
    gear_position: f32,    // 0=up, 1=down
    flaps_position: f32,   // 0..1
    spoilers_position: Option<f32>,
    spoilers_armed: Option<bool>,
    engines_running: u8,

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
    client_version: &'static str,
}

/// Convert empty/whitespace-only strings to None — used at the JSON-edge
/// to keep payloads clean of "" values that would muddy the server side.
fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
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

#[derive(Clone, Debug, Serialize)]
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

    // ─── v0.8.0 VPS-Navdata + Runway-Awareness ────────────────────────
    //
    // Identische Felder wie in `storage::LandingRecord`. Alle
    // skip_if_none damit Recorder + Webapp die Felder nur sehen wenn
    // tatsächlich gegen VPS-Navdata bewertet wurde — pre-v0.8.0
    // Touchdowns kommen ohne diese Felder durch und der MQTT-Consumer
    // muss nichts ändern.
    /// "navigraph" | "ourairports_fallback". Welche Quelle die
    /// Runway-Match-Daten geliefert hat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navdata_source: Option<String>,
    /// AIRAC-Cycle der genutzten Navigraph-Daten (e.g. "2604"). None
    /// wenn navdata_source = "ourairports_fallback".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navdata_cycle: Option<String>,
    /// True-course der Landerichtung in deg. Webapp braucht das fuer
    /// die RunwayDiagram-Achse.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runway_true_course_deg: Option<f64>,
    /// Displaced-Threshold in ft (0 = keine).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runway_displaced_threshold_ft: Option<i32>,
    /// Erwartete Threshold-Crossing-Height in ft (typisch 49-55).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runway_tch_expected_ft: Option<i32>,
    /// Veröffentlichter Glideslope-Winkel in Grad (typisch 3.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runway_glideslope_angle_deg: Option<f64>,
    /// Signed along-track-Distanz vom Landing-Threshold zum Touchdown,
    /// in Metern. Positiv = past, negativ = undershoot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub td_distance_from_threshold_m: Option<f64>,
    /// F3 TDZ-Result: true wenn Touchdown im TDZ-Marker. None bei
    /// runways < 1200 m.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub td_in_tdz: Option<bool>,
    /// 1-indexed third of the runway the touchdown lies in (1/2/3).
    /// Stable wire-key gegen storage::LandingRecord — Webapp + Pilot-
    /// Client teilen die Frontend-Logik.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub td_third: Option<u8>,
    /// F3 TDZ-Marker-Laenge in Metern (≤ 900, ≤ length/3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub td_tdz_length_m: Option<f64>,
    /// F4 Aim-Point delta in Metern (positiv = past, negativ = short).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aim_delta_m: Option<f64>,
    /// F4 Aim-Point classification: "perfect" | "short_of_aim" |
    /// "past_aim" | "long_landing" | "severe".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aim_class: Option<String>,
    /// F4 Aim-Point distance from threshold in Metern (300 oder 400).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aim_point_m: Option<f64>,
    /// F5 actual TCH (AGL ft beim Threshold-Crossing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tch_actual_ft: Option<f64>,
    /// F5 TCH delta = actual - expected (ft). Positiv = ueber Profil.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tch_delta_ft: Option<f64>,
    /// F5 TCH classification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tch_class: Option<String>,
    /// F6 Displaced-Threshold-Warning: Touchdown im Pre-Threshold-Paint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pre_displaced_threshold: Option<bool>,

    /// v0.10.0 (#runway-utilization-score) — Algorithmus-Version des im
    /// PIREP gespeicherten `sub_scores`-Arrays. None/Some(1) = pre-v0.10
    /// (meter-only Bahn-Auslastung); Some(2) = v0.10 (LDA-basierter
    /// Runway-Utilization-Score). Renderer rendert die neuen Felder
    /// (`extra`, neue Rationale-Keys, neue Warning-Werte) nur für v2.
    /// Spec: docs/spec/v0.10.0-runway-utilization-score.md LE11.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score_algorithm_version: Option<u8>,
}

fn is_false(b: &bool) -> bool { !*b }

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
    pub go_around_count: Option<u32>,
    pub touchdown_count: Option<u32>,
    pub dep_gate: Option<String>,
    pub arr_gate: Option<String>,
    pub approach_runway: Option<String>,
    pub divert: Option<bool>,
    pub diverted_to: Option<String>,
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
    /// Score). Spec: docs/spec/v0.10.0-runway-utilization-score.md LE11.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score_algorithm_version: Option<u8>,
}

/// Default fuer pre-v0.7.0 PIREPs ohne den marker. Wird von serde
/// genutzt wenn der PIREP-Payload aus alten JSONL-Backups oder
/// aeroacars-live-Storage deserialisiert wird.
#[allow(dead_code)]
fn default_forensics_version_v1() -> u8 { 1 }

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
#[derive(Clone, Debug, Serialize)]
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
}

enum Cmd {
    Position(Box<PositionPayload>),
    Phase(PhasePayload),
    Block(Box<BlockPayload>),
    Takeoff(Box<TakeoffPayload>),
    Touchdown(Box<TouchdownPayload>),
    Pirep(Box<PirepPayload>),
    TouchdownAccidentOverride(Box<TouchdownAccidentOverridePayload>),
    TouchdownRolloutFinalized(Box<TouchdownRolloutFinalizedPayload>),
    Shutdown,
}

#[derive(Clone)]
pub struct Handle {
    tx: mpsc::Sender<Cmd>,
}

impl Handle {
    pub fn position(&self, snap: &SimSnapshot, meta: &FlightMeta, phase: FlightPhase) {
        let payload = PositionPayload {
            ts: snap.timestamp.timestamp_millis(),
            phase: phase_label(phase),

            // Position
            lat: snap.lat,
            lon: snap.lon,
            alt_ft: snap.altitude_msl_ft.round() as i32,
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
                snap.aircraft_registration
                    .as_deref()
                    .and_then(non_empty)
            },
            // v0.8.3 (#5 follow-up): Sim-Aircraft-Title fuer Recorder-
            // Stats-Recompute. Quelle: SimVar TITLE (MSFS) /
            // acf_descrip (XP12). non_empty() filtert leere Strings.
            aircraft_title: snap.aircraft_title.as_deref().and_then(non_empty),
            simulator: simulator_label(snap.simulator),
            dep: non_empty(&meta.dep_icao),
            arr: non_empty(&meta.arr_icao),
            pirep_id: non_empty(&meta.pirep_id),
            client_version: env!("CARGO_PKG_VERSION"),
        };
        match self.tx.try_send(Cmd::Position(Box::new(payload))) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                debug!("mqtt cmd channel full — dropping position tick");
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                debug!("mqtt cmd channel closed — publisher down");
            }
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
                Duration::from_millis(500),
                tx.send(Cmd::Block(Box::new(payload))),
            )
            .await;
        });
    }

    pub fn takeoff(&self, payload: TakeoffPayload) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(
                Duration::from_millis(500),
                tx.send(Cmd::Takeoff(Box::new(payload))),
            )
            .await;
        });
    }

    pub fn touchdown(&self, payload: TouchdownPayload) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                Duration::from_millis(250),
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
            if let Err(e) =
                tokio::time::timeout(Duration::from_millis(500), tx.send(Cmd::Pirep(Box::new(payload)))).await
            {
                warn!("dropping pirep publish: {e}");
            }
        });
    }

    /// v0.7.19 GAF-707 (QS-R2 Finding 1): Korrektur-Publish nach Pilot-
    /// Override im Flight-End-Dialog. Recorder/VPS aktualisiert den
    /// bereits persistierten Touchdown-Row entsprechend.
    pub fn touchdown_accident_override(
        &self,
        payload: TouchdownAccidentOverridePayload,
    ) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                Duration::from_millis(500),
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
    pub fn touchdown_rollout_finalized(
        &self,
        payload: TouchdownRolloutFinalizedPayload,
    ) {
        let tx = self.tx.clone();
        tokio::spawn(async move {
            if let Err(e) = tokio::time::timeout(
                Duration::from_millis(500),
                tx.send(Cmd::TouchdownRolloutFinalized(Box::new(payload))),
            )
            .await
            {
                warn!("dropping touchdown_rollout_finalized publish: {e}");
            }
        });
    }

    pub fn shutdown(&self) {
        let _ = self.tx.try_send(Cmd::Shutdown);
    }
}

pub fn start(cfg: MqttConfig) -> Result<Handle> {
    cfg.validate()?;

    let (tx, mut rx) = mpsc::channel::<Cmd>(CMD_BUFFER);

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

    let _drive = tokio::spawn(async move {
        loop {
            match eventloop.poll().await {
                Ok(Event::Incoming(Packet::ConnAck(_))) => {
                    info!("MQTT CONNACK received");
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("MQTT poll error: {e} — backing off 5 s");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }
    });

    let cfg_for_pub = cfg.clone();
    let pub_client = client.clone();
    let _publisher = tokio::spawn(async move {
        if let Err(e) = pub_client
            .publish(
                cfg_for_pub.topic("status"),
                QoS::AtLeastOnce,
                true,
                STATUS_ONLINE.as_bytes(),
            )
            .await
        {
            warn!("initial status publish failed: {e}");
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

        while let Some(cmd) = rx.recv().await {
            match cmd {
                Cmd::Position(p) => publish_json(&pub_client, &cfg_for_pub.topic("position"), &p, QoS::AtMostOnce, true).await,
                Cmd::Phase(p) => publish_json(&pub_client, &cfg_for_pub.topic("phase"), &p, QoS::AtLeastOnce, true).await,
                Cmd::Block(p) => publish_json(&pub_client, &cfg_for_pub.topic("block"), &p, QoS::AtLeastOnce, true).await,
                Cmd::Takeoff(p) => publish_json(&pub_client, &cfg_for_pub.topic("takeoff"), &p, QoS::AtLeastOnce, true).await,
                Cmd::Touchdown(p) => publish_json(&pub_client, &cfg_for_pub.topic("touchdown"), &p, QoS::AtLeastOnce, false).await,
                Cmd::Pirep(p) => publish_json(&pub_client, &cfg_for_pub.topic("pirep"), &p, QoS::AtLeastOnce, false).await,
                Cmd::TouchdownAccidentOverride(p) => publish_json(
                    &pub_client,
                    &cfg_for_pub.topic("touchdown_accident_override"),
                    &p,
                    QoS::AtLeastOnce,
                    false,
                ).await,
                Cmd::TouchdownRolloutFinalized(p) => publish_json(
                    &pub_client,
                    &cfg_for_pub.topic("touchdown_rollout_finalized"),
                    &p,
                    QoS::AtLeastOnce,
                    false,
                ).await,
                Cmd::Shutdown => {
                    let _ = pub_client
                        .publish(cfg_for_pub.topic("status"), QoS::AtLeastOnce, true, STATUS_OFFLINE.as_bytes())
                        .await;
                    let _ = pub_client.disconnect().await;
                    break;
                }
            }
        }
        debug!("MQTT cmd loop exiting");
    });

    Ok(Handle { tx })
}

async fn publish_json<T: Serialize>(client: &AsyncClient, topic: &str, payload: &T, qos: QoS, retain: bool) {
    let body = match serde_json::to_vec(payload) {
        Ok(b) => b,
        Err(e) => {
            error!("serialize {topic} failed: {e}");
            return;
        }
    };
    if let Err(e) = client.publish(topic, qos, retain, body).await {
        warn!("publish {topic} failed: {e}");
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
