//! phpVMS 7 HTTP API client.
//!
//! Talks to:
//!   * phpVMS Core API (users, bids, flights, fleet, PIREP file, ACARS positions)
//!   * AeroACARS phpVMS module (config, version, heartbeat, landing extras) — Phase 4
//!
//! Authentication: phpVMS API key sent via the `X-API-Key` header (phpVMS standard).
//! All requests advertise `User-Agent: AeroACARS/<version>` so the server can identify us.

#![allow(dead_code)] // Some endpoints land in later phases; their wrappers are stubbed.

use std::fmt;
use std::time::Duration;

use reqwest::{header, Client as HttpClient, Response, StatusCode};
use serde::de::{self, DeserializeOwned, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;
use url::Url;

/// phpVMS sometimes encodes string-ish fields (e.g. `flight_number`) as JSON
/// numbers when the value happens to be all digits. Accept either form.
fn de_str_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = String;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("string or integer")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_owned())
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    d.deserialize_any(V)
}

/// `Option<String>` variant of `de_str_or_int`. Reuses the same string-or-int
/// visitor but accepts JSON `null` / missing as `None`. Needed because
/// phpVMS encodes optional id fields (route_code, callsign, alt_airport_id,
/// flight_type, route) as integers when the underlying value is numeric,
/// strings when alphanumeric, and null when missing. Without this we'd
/// fail the entire bids list parse on a single legacy flight whose
/// route_code was stored as an integer in the database.
fn de_opt_str_or_int<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = Option<String>;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("string, integer, or null")
        }
        fn visit_unit<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_none<E: de::Error>(self) -> Result<Option<String>, E> {
            Ok(None)
        }
        fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<Option<String>, D::Error> {
            de_str_or_int(d).map(Some)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<Option<String>, E> {
            Ok(Some(v.to_owned()))
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<Option<String>, E> {
            Ok(Some(v))
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Option<String>, E> {
            Ok(Some(v.to_string()))
        }
    }
    d.deserialize_any(V)
}

/// Mirror of `de_str_or_int` for fields we want as `i64`. phpVMS encodes
/// numeric ids inconsistently — sometimes as JSON numbers, sometimes as
/// strings (notably on the Eurowings test instance). Tolerating both
/// stops the entire bids list from failing to parse on a single bad row.
fn de_int_or_str<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    struct V;
    impl<'de> Visitor<'de> for V {
        type Value = i64;
        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("integer or numeric string")
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<i64, E> {
            Ok(v)
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<i64, E> {
            Ok(v as i64)
        }
        fn visit_f64<E: de::Error>(self, v: f64) -> Result<i64, E> {
            Ok(v as i64)
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<i64, E> {
            v.trim()
                .parse::<i64>()
                .map_err(|_| E::custom(format!("not an integer: {v:?}")))
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<i64, E> {
            self.visit_str(&v)
        }
    }
    d.deserialize_any(V)
}

// v0.5.49 — HTTP-Client-Hardening gegen "Fehler 1236" (NAT-Eviction +
// dead-Socket-Hangs). Vorher: nur DEFAULT_TIMEOUT=20s am total request,
// kein connect_timeout, kein tcp_keepalive. Eine vom Router/ISP gekillte
// TCP-Verbindung führte zu 20s blockiertem await — der Streamer-Tick
// hing 20s je Request, kein UI-Update, kein JSONL-Append, Pilot dachte
// die App ist tot.
//
// Fix-Komponenten:
// - tcp_keepalive(30s): OS schickt regelmaessig TCP-Keep-Alive-Pakete,
//   verhindert NAT-Eviction in Consumer-Routern (FritzBox, Speedport)
//   und haelt phpVMS-Server-side keep-alive warm
// - connect_timeout(5s): wenn der TCP-Handshake hängt, schnell aufgeben
//   statt 20s zu warten
// - pool_idle_timeout(60s): idle Verbindungen aus dem Pool werfen bevor
//   der Server (typisch nginx keepalive_timeout 60-75s) die Tür zumacht
// - pool_max_idle_per_host(8): mehr als 8 idle Sockets pro Host sind eh
//   Verschwendung
// - DEFAULT_TIMEOUT auf 10s reduziert: 20s war im Pilot-Use-Case immer
//   zu lang — wenn ein Call so lange braucht ist die Verbindung eh tot
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const TCP_KEEPALIVE: Duration = Duration::from_secs(30);
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// v0.16.17: server-side IN_PROGRESS filter (phpVMS PirepState 0). Const so
/// the URL test below pins the exact path + query the client sends — see
/// `get_user_pireps_in_progress` for why this must NOT be a page-1
/// fetch-then-filter.
const USER_PIREPS_IN_PROGRESS_PATH: &str = "/api/user/pireps?state=0";

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("invalid base URL: {0}")]
    InvalidUrl(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("authentication failed (HTTP 401) — check API key")]
    Unauthenticated,
    #[error("forbidden (HTTP 403) — account may lack permissions")]
    Forbidden,
    #[error("not found (HTTP 404) — endpoint missing on this phpVMS site")]
    NotFound,
    #[error("rate limited (HTTP 429), retry after {retry_after_seconds}s")]
    RateLimited { retry_after_seconds: u64 },
    #[error("server error (HTTP {status}): {body}")]
    Server { status: u16, body: String },
    #[error("unexpected response shape: {0}")]
    BadResponse(String),
}

/// v0.7.8: Spezifische Fehler-Varianten fuer den SimBrief-direct
/// Fetch-Pfad. Werden auf Frontend-Notice-Codes gemapped (Spec §8).
/// Nicht aus ApiError abgeleitet weil:
///   - Pfad ist getrennt (keine phpVMS-API-Schicht dazwischen)
///   - Notice-Wording haengt von Variante ab — Pilot soll wissen ob
///     User falsch konfiguriert ist (UserNotFound) vs Internet weg
///     (Network) vs SimBrief offline (Unavailable) vs XML kaputt
///     (ParseFailed). Pure code-Granularitaet, Wording landet
///     i18n-side.
/// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §3 + §5.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SimBriefDirectError {
    /// Settings haben weder username noch user_id gesetzt.
    /// Pure code-Variante — Pfad-Auswahl im Caller faengt das ab,
    /// dieser Variant tritt nicht in der Praxis auf.
    #[error("no SimBrief identifier configured")]
    NoIdentifier,
    /// HTTP 400 (Navigraph-Doku-Pfad) ODER `<fetch><status>Error</status>`
    /// (Live-Probe-Pfad). Pilot muss Settings pruefen.
    #[error("SimBrief user not found")]
    UserNotFound,
    /// HTTP 5xx — SimBrief offline / maintenance.
    #[error("SimBrief service unavailable")]
    Unavailable,
    /// Network-Layer-Fehler (DNS, TLS, Connection-Refused, etc.).
    /// Auch fuer unerwartete non-2xx-Codes die nicht 400/5xx sind.
    #[error("SimBrief network error")]
    Network,
    /// HTTP 200 + Status Success aber XML konnte nicht geparsed werden.
    /// Sollte praktisch nie passieren — SimBrief liefert stabile XML.
    #[error("SimBrief XML parse failed")]
    ParseFailed,
}

impl ApiError {
    /// Stable identifier surfaced to the UI for i18n key lookup.
    pub fn code(&self) -> &'static str {
        match self {
            ApiError::InvalidUrl(_) => "invalid_url",
            ApiError::Network(_) => "network",
            ApiError::Unauthenticated => "unauthenticated",
            ApiError::Forbidden => "forbidden",
            ApiError::NotFound => "not_found",
            ApiError::RateLimited { .. } => "rate_limited",
            ApiError::Server { .. } => "server",
            ApiError::BadResponse(_) => "bad_response",
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        ApiError::Network(err.to_string())
    }
}

/// Connection details for a phpVMS site.
#[derive(Clone, Debug)]
pub struct Connection {
    base_url: Url,
    api_key: String,
}

impl Connection {
    pub fn new(base_url: &str, api_key: impl Into<String>) -> Result<Self, ApiError> {
        let trimmed = base_url.trim().trim_end_matches('/');
        let url = Url::parse(trimmed).map_err(|_| ApiError::InvalidUrl(trimmed.into()))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(ApiError::InvalidUrl(format!(
                "URL must be http(s), got '{}'",
                url.scheme()
            )));
        }
        // SECURITY: plaintext `http://` is only allowed to loopback. The API key
        // travels in the X-API-Key / bearer headers, so over http to a routable
        // host it would be sent in cleartext. Production hard-locks the phpVMS URL
        // to https anyway; this stops the dev/localhost path from being aimed at a
        // remote http host (LAN or public).
        if url.scheme() == "http" {
            let host = url.host_str().unwrap_or("");
            // host_str() keeps brackets for IPv6 (e.g. "[::1]") — strip them.
            let host_ip = host.trim_start_matches('[').trim_end_matches(']');
            let is_loopback = host.eq_ignore_ascii_case("localhost")
                || host_ip
                    .parse::<std::net::IpAddr>()
                    .map(|ip| ip.is_loopback())
                    .unwrap_or(false);
            if !is_loopback {
                return Err(ApiError::InvalidUrl(format!(
                    "plaintext http:// is only allowed for localhost, got '{host}'"
                )));
            }
        }
        Ok(Self {
            base_url: url,
            api_key: api_key.into(),
        })
    }

    pub fn base_url(&self) -> &str {
        self.base_url.as_str()
    }
}

// ---- Resource types ----

/// Subset of `GET /api/user` we need.
///
/// phpVMS exposes `home_airport` / `curr_airport` (the ICAO strings) NOT a
/// `_id` suffix, despite the underlying DB columns being named that way. Took
/// us one debug round to spot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: i64,
    pub pilot_id: i64,
    /// Formatted pilot identifier, e.g. `GSG0001`.
    #[serde(default)]
    pub ident: Option<String>,
    pub name: String,
    pub email: Option<String>,
    pub airline_id: Option<i64>,
    /// ICAO of the airport the pilot is currently at.
    #[serde(default, alias = "curr_airport_id")]
    pub curr_airport: Option<String>,
    /// ICAO of the pilot's home airport.
    #[serde(default, alias = "home_airport_id")]
    pub home_airport: Option<String>,
    #[serde(default)]
    pub airline: Option<Airline>,
    #[serde(default)]
    pub rank: Option<Rank>,
    /// v0.7.8 (v1.5): Pilot-personalisierter Callsign-Suffix
    /// (z.B. "4TK"). Kombiniert mit `airline.icao` ergibt das
    /// typische ATC-Callsign-Muster "CFG4TK" das viele VAs operativ
    /// nutzen — abweichend von der Bid-`flight_number`-Form "CFG1504".
    /// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §6.1.
    /// Falls Pilot-Profile das Feld nicht hat: None, kein Schaden.
    #[serde(default)]
    pub callsign: Option<String>,
    /// v0.12.1 (Stream B): phpVMS 7 `UserState` — pilot account status.
    /// 0 = PENDING, 1 = ACTIVE, 2 = REJECTED, 3 = ON_LEAVE, 4 = SUSPENDED.
    /// `/api/user` returns it; only ACTIVE pilots may use AeroACARS
    /// (see `pilot_state_block_reason`). `None` on legacy installs that
    /// don't expose the field.
    #[serde(default)]
    pub state: Option<i32>,
    /// v0.16.23: SimBrief profile username as stored on the pilot's
    /// phpVMS account. A companion phpVMS change exposes this through
    /// `/api/user` so we can auto-source the SimBrief identifier (used by
    /// `flight_refresh_route_only`) without the pilot re-typing it in
    /// Settings. `#[serde(default)]` is mandatory: older phpVMS installs
    /// (and any instance without the companion change) simply omit the
    /// field — `None` then, no error. We NEVER overwrite an explicit
    /// user-set identifier with this (see `cache_pilot`).
    #[serde(default)]
    pub simbrief_username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rank {
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Airline {
    /// phpVMS occasionally encodes integer ids as JSON strings on certain
    /// resources (Eurowings test instance observed 2026-05-02). Use the
    /// permissive int-or-string helper and convert to i64 here so the rest
    /// of the codebase keeps an integer.
    #[serde(deserialize_with = "de_int_or_str")]
    pub id: i64,
    /// May be missing on legacy installs that haven't run the airline
    /// migration — default to empty so the bid still loads.
    #[serde(default)]
    pub icao: String,
    #[serde(default)]
    pub iata: Option<String>,
    /// Same defensive default as `icao`.
    #[serde(default)]
    pub name: String,
    /// Optional URL to the airline's logo, exposed by phpVMS Airline resources.
    /// May be absent or empty depending on VA configuration.
    #[serde(default)]
    pub logo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Airport {
    /// phpVMS encodes airport ids inconsistently across instances —
    /// usually ICAO strings, sometimes integers (legacy databases).
    /// Permissive here so a single misencoded airport doesn't fail the
    /// whole bids/flight list parse.
    #[serde(deserialize_with = "de_str_or_int")]
    pub id: String,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub icao: Option<String>,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub iata: Option<String>,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub name: Option<String>,
    #[serde(default)]
    pub lat: Option<f64>,
    #[serde(default)]
    pub lon: Option<f64>,
    #[serde(default)]
    pub elevation: Option<f64>,
}

/// phpVMS exposes distance as a multi-unit object:
/// `{ "m": 483372, "km": 483.37, "mi": 300.35, "nmi": 261 }`.
/// Any of these may be missing depending on the serializer.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Distance {
    #[serde(default)]
    pub m: Option<f64>,
    #[serde(default)]
    pub mi: Option<f64>,
    #[serde(default)]
    pub km: Option<f64>,
    #[serde(default)]
    pub nmi: Option<f64>,
}

/// Subset of phpVMS's Flight resource we use in Phase 1. Permissive on optional fields.
///
/// Every `String` / `Option<String>` id-ish field uses a permissive
/// deserializer because phpVMS is inconsistent across instances: a
/// numeric flight_id stored in the DB comes back as a JSON integer on
/// some sites (legacy auto-increment IDs) and as a string on others
/// (UUID setups). Live bug from a GSG pilot 2026-05-03: `/api/user/bids`
/// returned `"flight_number": 6431` (integer, no quotes), failing the
/// entire bids parse with "invalid type: integer '6431', expected a
/// string". Lesson: assume nothing about wire types, only types we
/// fully control end-to-end deserve a strict shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flight {
    #[serde(deserialize_with = "de_str_or_int")]
    pub id: String,
    /// phpVMS encodes this as a JSON number when the value is purely numeric
    /// (e.g. `284` or `6431`) and as a string otherwise (e.g. `"VL12A"`).
    #[serde(deserialize_with = "de_str_or_int")]
    pub flight_number: String,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub route_code: Option<String>,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub route_leg: Option<String>,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub callsign: Option<String>,
    #[serde(deserialize_with = "de_str_or_int")]
    pub dpt_airport_id: String,
    #[serde(deserialize_with = "de_str_or_int")]
    pub arr_airport_id: String,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub alt_airport_id: Option<String>,
    /// Scheduled flight time in minutes.
    #[serde(default)]
    pub flight_time: Option<i32>,
    /// Cruise level (e.g. 360 == FL360).
    #[serde(default)]
    pub level: Option<i32>,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub route: Option<String>,
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub flight_type: Option<String>,
    #[serde(default)]
    pub distance: Option<Distance>,
    #[serde(default)]
    pub airline: Option<Airline>,
    #[serde(default)]
    pub dpt_airport: Option<Airport>,
    #[serde(default)]
    pub arr_airport: Option<Airport>,
    /// SimBrief OFP relation when the pilot has prepared this flight in SimBrief.
    #[serde(default)]
    pub simbrief: Option<SimBrief>,
}

/// SimBrief OFP record returned with a bid/flight when the pilot has prepared one.
/// `id` looks like `"1777622821_5F3E3B3842"` (a SimBrief-side identifier).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimBrief {
    pub id: String,
    /// JSON briefing endpoint exposed by phpVMS Core.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub aircraft_id: Option<i64>,
    /// SimBrief-derived subfleet info, including the fares (passenger / cargo
    /// counts) the OFP was generated against. We carry these forward into the
    /// final filed PIREP so the VA gets accurate load numbers.
    #[serde(default)]
    pub subfleet: Option<SimBriefSubfleet>,
}

/// Parsed SimBrief OFP weights + fuel plan, fetched from the
/// public XML endpoint. All weights / fuel quantities normalised
/// to KG regardless of the OFP's `units.wt_unit` setting.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SimBriefOfp {
    /// Block fuel (= ramp fuel, what the pilot loads at the gate).
    /// `<fuel><plan_ramp>` in the OFP.
    pub planned_block_fuel_kg: f32,
    /// Estimated trip burn — the dispatcher's planned fuel consumed
    /// from takeoff to touchdown. `<fuel><est_burn>`.
    pub planned_burn_kg: f32,
    /// Reserve fuel set aside for the alternate + holding pattern.
    /// `<fuel><reserve>`.
    pub planned_reserve_kg: f32,
    /// Zero-fuel weight as planned by the dispatcher. Used to detect
    /// over-loading at takeoff.
    pub planned_zfw_kg: f32,
    /// Takeoff weight (block fuel + ZFW − taxi fuel) per OFP.
    pub planned_tow_kg: f32,
    /// Landing weight (TOW − burn) per OFP.
    pub planned_ldw_kg: f32,
    /// ICAO-coded route string from the flight plan, e.g.
    /// `"DENUT5L DENUT M624 SUSAN ..."`. None when the OFP didn't
    /// include a route (rare).
    pub route: Option<String>,
    /// Alternate airport ICAO, if planned.
    pub alternate: Option<String>,
    /// Ordered waypoints from the OFP `<navlog>`. Posted to phpVMS via
    /// `POST /pireps/{id}/route` after prefile so the live map can show
    /// the planned track alongside the actually flown one.
    #[serde(default)]
    pub waypoints: Vec<RouteFix>,
    // ---- MAX-Werte aus dem OFP für Overweight-Detection (v0.3.0) ----
    /// Maximum Zero-Fuel Weight laut Aircraft-Performance. Pilot darf
    /// `IST-ZFW <= MAX-ZFW` haben — sonst Strukturschaden möglich.
    /// 0.0 wenn das OFP-XML kein `<max_zfw>`-Tag hatte.
    pub max_zfw_kg: f32,
    /// Maximum Takeoff Weight. Drives die Overweight-Warnung im
    /// Live-Loadsheet und zieht Punkte vom Loadsheet-Score ab wenn
    /// IST-TOW > MAX-TOW beim Takeoff.
    pub max_tow_kg: f32,
    /// Maximum Landing Weight. Bei Overshoot droht Fuel-Dumping
    /// oder Overweight-Landing-Inspektion.
    pub max_ldw_kg: f32,
    // ---- OFP-Identitätsfelder (v0.3.0) ----
    // Damit der Pilot bei Mismatch sieht WORAUF der SimBrief-OFP
    // tatsächlich basiert (Flight-Number, Origin, Destination, wann
    // erstellt). Hilft die "Plan ist von gestern"-Verwirrung
    // aufzulösen — SimBrief liefert immer den letzten OFP des
    // Pilot-Accounts, ohne dass das mit der aktuellen Buchung
    // verknüpft ist.
    /// Flight-Number aus dem OFP (z.B. "DLH123" oder "RYR100").
    /// Empty wenn das XML keine atc.callsign / general.flight_number
    /// hatte.
    pub ofp_flight_number: String,
    /// Origin-ICAO aus dem OFP (z.B. "LOWS"). Empty wenn nicht im XML.
    pub ofp_origin_icao: String,
    /// Destination-ICAO aus dem OFP (z.B. "EDDB"). Empty wenn nicht.
    pub ofp_destination_icao: String,
    /// Wann der OFP erstellt wurde (Unix-Timestamp als String aus
    /// dem XML, oder ISO-Datum). Empty wenn nicht im XML.
    pub ofp_generated_at: String,
    /// v0.7.8: `<params><request_id>` aus dem SimBrief-XML. Aendert sich
    /// bei JEDER Re-Generation auf simbrief.com — canonical changed-flag-
    /// Quelle fuer SimBrief-direct Refresh. Leer wenn Tag fehlt (sollte
    /// praktisch nie passieren laut Live-API-Probe).
    /// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §3.
    #[serde(default)]
    pub request_id: String,
    /// v0.7.12: `<weights><pax_count>` aus dem SimBrief-XML — Anzahl Pax
    /// im OFP-Plan. Bei Pre-Flight-SimBrief-direct (v0.7.10) zeigt das
    /// Frontend diesen Wert in der Bid-Card; ohne diesen Wert sah der
    /// Pilot keine Pax-Info bevor er den OFP ueber phpVMS gebunden hat.
    /// 0 wenn der Tag fehlt oder cargo-only.
    #[serde(default)]
    pub pax_count: i32,
    /// v0.7.12: Cargo-Last in kg. SimBrief liefert das in `<weights><cargo>`
    /// (default lbs, mit `units_set=kgs`-Toggle: kg). 0.0 wenn Tag fehlt.
    #[serde(default)]
    pub cargo_kg: f32,
}

/// Single navlog fix from a SimBrief OFP. `kind` carries the SimBrief
/// fix type ("apt", "wpt", "vor", "ndb"); we map it to phpVMS's
/// numeric `nav_type` at the boundary.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RouteFix {
    pub ident: String,
    pub lat: f64,
    pub lon: f64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SimBriefSubfleet {
    #[serde(default)]
    pub type_: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub fares: Vec<Fare>,
}

/// Single fare-class entry. Fields are permissive — phpVMS varies which keys
/// it returns based on context (subfleet vs. PIREP file).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fare {
    pub id: i64,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub capacity: Option<i32>,
    /// Number of passengers (or cargo units) the OFP was generated for.
    #[serde(default)]
    pub count: Option<i32>,
    #[serde(default)]
    pub price: Option<f64>,
    /// 0 = passenger fare, 1 = cargo (phpVMS convention).
    #[serde(default, rename = "type")]
    pub fare_type: Option<i32>,
}

/// `GET /api/user/bids` returns a list of these.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bid {
    #[serde(deserialize_with = "de_int_or_str")]
    pub id: i64,
    /// phpVMS sometimes returns the user id as a string (Eurowings test
    /// instance, 2026-05-02). Be permissive on the wire.
    #[serde(deserialize_with = "de_int_or_str")]
    pub user_id: i64,
    /// Always a string in canonical phpVMS, but tolerant to int just in case.
    #[serde(deserialize_with = "de_str_or_int")]
    pub flight_id: String,
    pub flight: Flight,
}

// ---- PIREP lifecycle types ----

/// Body for `POST /api/pireps/prefile`.
/// phpVMS validates these; only the listed fields are required, the rest are
/// dropped if `None` (skipped via `skip_serializing_if`).
#[derive(Debug, Clone, Serialize, Default)]
pub struct PrefileBody {
    pub airline_id: i64,
    pub aircraft_id: String,
    /// v0.16.18: phpVMS-Flight-ID aus dem aktiven Bid. phpVMS validiert
    /// sie im PrefileRequest ('sometimes|nullable|exists:flights,id') und
    /// haengt den PIREP damit an den geplanten Flug — wichtig fuers
    /// Event-/Tour-Matching (SkyAdventures: Etappen mit festem Flug).
    /// Vorher fehlte sie systematisch -> pireps.flight_id blieb leer.
    /// Bei Diversion nullt phpVMS sie bewusst selbst (PirepService:778).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_id: Option<String>,
    pub flight_number: String,
    pub dpt_airport_id: String,
    pub arr_airport_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt_airport_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route_leg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_distance: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub planned_flight_time: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub route: Option<String>,
    pub source_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

/// Single position entry posted to `POST /api/pireps/{id}/acars/position`.
/// We map our internal `SimSnapshot` to this on the way out.
///
/// Field set tracks the phpVMS-Core `acars` model (lat/lon/heading/altitude/
/// altitude_agl/altitude_msl/gs/ias/vs/fuel/fuel_flow/transponder/autopilot/
/// distance/log/sim_time/source). Anything outside that schema (lights,
/// COM/NAV freqs, autopilot mode detail) goes into `log` as a compact JSON
/// blob so the live map / PIREP detail page can surface it without a custom
/// field per item.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PositionEntry {
    pub lat: f64,
    pub lon: f64,
    pub altitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude_agl: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude_msl: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<f32>,
    /// Groundspeed in knots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gs: Option<f32>,
    /// Vertical speed in fpm.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vs: Option<f32>,
    /// Indicated airspeed in knots.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ias: Option<f32>,
    /// Total fuel on board, kilograms (phpVMS-Core column).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel: Option<f32>,
    /// Total fuel flow, kg/h (phpVMS-Core column).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel_flow: Option<f32>,
    /// 4-digit transponder / squawk code (phpVMS-Core column).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transponder: Option<u16>,
    /// Autopilot master on/off (phpVMS-Core column, stored as int 0/1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autopilot: Option<bool>,
    /// Distance to the destination in nautical miles (phpVMS-Core column).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Free-form log line shown on the live map. We pack telemetry that
    /// phpVMS doesn't have first-class columns for (lights, com/nav,
    /// autopilot modes) here as compact JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log: Option<String>,
    /// ISO-8601 UTC timestamp.
    pub sim_time: String,
}

/// Body for `POST /api/pireps/{id}/file` — final flight stats at submission.
//
// v0.5.49: Deserialize hinzugefügt damit der PIREP-Queue-Worker das
// JSON aus der persistenten Queue zurücklesen kann.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileBody {
    /// Total flight time in minutes (takeoff → landing).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_time: Option<i32>,
    /// Fuel used (units configured site-side; phpVMS default is pounds).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel_used: Option<f64>,
    /// Total fuel on board at block-off, same unit as `fuel_used`.
    /// Without this phpVMS shows "Verbleibender Treibstoff = -fuel_used"
    /// because it derives remaining = block_fuel - fuel_used and the
    /// missing field defaults to 0.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_fuel: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Cruise level in feet (e.g. 36000). Native phpVMS column on the
    /// PIREP details page (`Flt.Level`); we report the highest steady
    /// altitude the aircraft held during the flight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    /// Touchdown vertical speed in fpm (negative on a real landing).
    /// phpVMS displays this as "Landing Rate" on the PIREP overview.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_rate: Option<f64>,
    /// Numeric landing score 0..100 (phpVMS convention; we map our
    /// LandingScore enum into roughly equivalent ranges).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// Final passenger / cargo loads per fare class.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fares: Option<Vec<FareEntry>>,
    /// Custom PIREP fields keyed by name. The VA admin configures the fields
    /// in their phpVMS / ACARS module; we send everything we can compute.
    /// Spec §24.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<std::collections::HashMap<String, String>>,
    /// Override the planned arrival airport when filing — used for
    /// diverts. phpVMS' FileRequest.rules() validates this field, and
    /// the controller writes it through to the Pirep model on file.
    /// When set, the PIREP shows up in the VA's PIREP list with the
    /// ACTUAL landing airport instead of the bid's planned one.
    /// Pair with a `notes` block that explains the divert (we do that
    /// in `flight_end_with_divert`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arr_airport_id: Option<String>,
    /// ISO-8601 / RFC-3339 off-block time (first movement out of the gate).
    ///
    /// phpVMS will NOT derive this on its own. `PirepService::create()` tries
    /// (`block_off_time = block_on_time - flight_time`), but the guard
    /// `if (!$pirep->block_off_time)` never fires: `block_off_time` is cast
    /// with `App\Casts\CarbonCast`, whose `get()` turns a NULL column into
    /// `new Carbon(null)` — i.e. the CURRENT time, which is always truthy.
    /// So a PIREP we don't send this for keeps `block_off_time = NULL` in the
    /// DB forever, while the model reports "now" to every reader.
    ///
    /// Consequence before this was sent: every AeroACARS PIREP on GSG had a
    /// NULL off-block time (vmsACARS/smartCARS PIREPs all have one), which
    /// broke SkyAdventures' event-window matching for long-haul flights.
    /// `block_off_time` is in the Pirep `$fillable` list and `/file` mass-
    /// assigns the raw request input, so sending it is enough.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_off_time: Option<String>,
    /// ISO-8601 / RFC-3339 on-block time (parked at the gate).
    ///
    /// phpVMS otherwise falls back to the submission timestamp, which is when
    /// the pilot clicked "file" — on a long flight that can be far from the
    /// actual arrival. Same mass-assignment path as `block_off_time`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_on_time: Option<String>,
}

/// Minimal fare entry for filing — phpVMS uses `id` to look up the fare class
/// and `count` for the loaded amount.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FareEntry {
    pub id: i64,
    pub count: i32,
}

/// Body for `POST /api/pireps/{id}/update`. Used both for explicit phase /
/// state transitions AND as the periodic "heartbeat" that keeps the PIREP
/// alive against phpVMS's `RemoveExpiredLiveFlights` cron — that cron looks
/// at `pireps.updated_at`, NOT at the latest position row, so without a
/// regular call here a long cruise leg gets soft-deleted after the
/// `acars.live_time` window (default 2h on most installs).
///
/// vmsACARS sends this every `acars_update_timer` seconds (default 30) with
/// monotonically growing `flight_time` / `distance` fields, which
/// guarantees Eloquent sees the model as dirty and bumps `updated_at`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct UpdateBody {
    /// phpVMS PirepState. 0 = IN_PROGRESS, 1 = PENDING, 2 = ACCEPTED, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<i32>,
    /// phpVMS PirepSource. 0 = ACARS, 1 = MANUAL. Smuggled through the
    /// update endpoint (rules() doesn't validate it but parsePirep
    /// passes everything to mass-assignment, and `source` is in the
    /// Pirep $fillable). Used by `flight_end_manual` to flip the source
    /// to MANUAL before /file so PirepService::submit() routes the
    /// PIREP through the manual auto-approve path on the pilot's rank.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<i32>,
    /// phpVMS PirepStatus code (e.g. "BST" boarding, "OFB" pushback,
    /// "TKO" takeoff, "ENR" enroute, "APP" approach).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Seconds since block-off. Monotonically growing → guarantees dirty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flight_time: Option<i32>,
    /// Distance flown so far (nmi). Also used by the live-map sidebar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance: Option<f64>,
    /// Fuel burned so far (units configured site-side; phpVMS default lbs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fuel_used: Option<f64>,
    /// Total fuel on board at block-off (units configured site-side).
    /// v0.3.0: NEW in the live heartbeat. phpVMS' live tracking page
    /// derives "Verbleibender Treibstoff = block_fuel − fuel_used";
    /// without this field the missing column defaults to 0 and the
    /// remaining-fuel display reads as "−<fuel_used>" for the entire
    /// flight. We send the value once it's known (= once block-off
    /// has been timestamped) and on every subsequent heartbeat.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_fuel: Option<f64>,
    /// Current cruise level / altitude in feet (e.g. 34000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<i32>,
    /// Free-form ACARS source identifier shown in the PIREP detail.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// ISO-8601 / RFC-3339 timestamp. Smuggled through `parsePirep`
    /// (which Carbon-converts the field) → mass-assigned onto the
    /// Pirep model where `updated_at` is in the $fillable list.
    /// Used by the heartbeat as the guaranteed-dirty marker so
    /// Eloquent always emits an UPDATE: without this, a heartbeat
    /// sent while the aircraft is still parked would pass distance=0
    /// and flight_time=0 — those aren't dirty after the first call,
    /// no UPDATE fires, and `pireps.updated_at` doesn't bump → cron
    /// kills the PIREP after `acars.live_time` hours.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    /// Override the arrival airport. Used by the divert-finalize path
    /// in `flight_end` to mass-assign the actual landing airport when
    /// the pilot diverted. `arr_airport_id` is in the Pirep `$fillable`
    /// list, so this is mass-assigned through `parsePirep` like every
    /// other field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arr_airport_id: Option<String>,
    /// Touchdown vertical speed in fpm (negative on a real landing).
    /// Smuggled the same way as `source` — `landing_rate` is in
    /// `$fillable`, the Acars\\UpdateRequest doesn't validate it but
    /// parsePirep passes the raw input through to mass-assignment.
    /// Used by the divert-finalize path so the PIREP detail shows the
    /// landing rate even though we never call `/file`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub landing_rate: Option<f64>,
    /// Numeric landing score 0..100. Same smuggle path as
    /// `landing_rate` — `score` is in the Pirep $fillable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<i32>,
    /// ISO-8601 / RFC-3339 timestamp marking when the PIREP was
    /// submitted. Normally set by `Acars\\PirepController::file()`;
    /// since the divert-finalize path skips `/file`, we set it here
    /// so admin queue ordering works.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submitted_at: Option<String>,
    /// ISO-8601 / RFC-3339 block-on time. Same reason as
    /// `submitted_at` — `/file` would normally set this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_on_time: Option<String>,
}

/// phpVMS PirepSource enum values, mirrored here so call sites don't
/// have to remember the magic numbers. Values match the upstream
/// `App\Models\Enums\PirepSource` constants.
pub mod pirep_source {
    pub const ACARS: i32 = 0;
    pub const MANUAL: i32 = 1;
}

/// Single text log line posted to `POST /api/pireps/{id}/acars/logs`. Used
/// for sub-events that don't map to a phpVMS PirepStatus code (TOC, TOD,
/// V1 / VR / V2, touchdown vertical speed, engine start/stop, etc.) — vmsACARS
/// uses this same endpoint to write the chronological "story" pilots see
/// in the PIREP detail page.
#[derive(Debug, Clone, Serialize, Default)]
pub struct LogEntry {
    pub log: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lon: Option<f64>,
    /// ISO-8601 UTC timestamp.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Single waypoint posted to `POST /api/pireps/{id}/route`. Order is
/// 0-based; nav_type follows the phpVMS Navdata enum (1 = waypoint,
/// 2 = NDB, 3 = VOR, 4 = airport). When unsure, omit nav_type — phpVMS
/// renders it generically.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RouteWaypoint {
    pub name: String,
    pub order: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nav_type: Option<i32>,
    pub lat: f64,
    pub lon: f64,
}

/// Full PIREP record returned by `GET /api/pireps/{id}`. We use this for
/// recovery / diagnose when the live POST endpoints suddenly start
/// returning 404 — we can distinguish "soft-deleted by cron" from
/// "pirep was filed elsewhere" from "auth lost".
#[derive(Debug, Clone, Deserialize)]
pub struct PirepFull {
    pub id: String,
    #[serde(default)]
    pub state: Option<i32>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub flight_number: Option<String>,
    #[serde(default)]
    pub dpt_airport_id: Option<String>,
    #[serde(default)]
    pub arr_airport_id: Option<String>,
    #[serde(default)]
    pub flight_time: Option<i32>,
    // `distance` intentionally omitted — phpVMS sometimes returns it as
    // a `{value, unit}` object (e.g. on `/api/pireps/{id}`) and sometimes
    // as a bare `f64` (other endpoints), and we don't need it for any
    // current call site. Serde ignores unknown fields by default, so
    // dropping it makes the GET resilient to both shapes.
}

/// A single ACARS position row from `GET /api/pireps/{id}/acars/position`.
///
/// CRITICAL: decode ONLY `lat`/`lon`. The phpVMS `Acars` API resource returns
/// `distance` and `fuel` as **unit-objects** (e.g. `{"nmi": 12.3}`), not bare
/// scalars, so a struct that named those fields with a scalar type would fail
/// to decode the whole envelope. Serde drops unknown fields by default, so we
/// keep only the two coordinates — same resilience pattern as `PirepFull`,
/// which drops `distance` for the same reason (see above).
#[derive(Debug, Clone, Deserialize)]
pub struct AcarsPosition {
    // Option: lat/lon are nullable in the phpVMS acars schema. A single null
    // row must NOT poison the whole-batch decode (it would silently skip the
    // track reseed) — null rows are filtered out at the consumer.
    pub lat: Option<f64>,
    pub lon: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PirepCreated {
    pub id: String,
}

/// Lightweight PIREP record returned by `GET /api/user/pireps`. Used to find
/// an existing in-progress flight so we can resume it instead of doing a
/// fresh prefile (which would fail with aircraft-not-available because the
/// existing PIREP holds the aircraft "in use").
#[derive(Debug, Clone, Deserialize)]
pub struct PirepSummary {
    pub id: String,
    #[serde(default)]
    pub airline_id: Option<i64>,
    #[serde(default)]
    pub flight_number: Option<String>,
    #[serde(default)]
    pub aircraft_id: Option<i64>,
    /// phpVMS PirepState: 0=IN_PROGRESS, 1=PENDING, 2=ACCEPTED, 3=CANCELLED, …
    #[serde(default)]
    pub state: Option<i32>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub dpt_airport_id: Option<String>,
    #[serde(default)]
    pub arr_airport_id: Option<String>,
    // v0.7.18 (B-011): Felder fuer den Orphan-Cleanup-Pfad. phpVMS liefert
    // diese je nach Version + VA-Plugin-Config nicht garantiert mit, daher
    // alle Option + serde(default). Spec docs/spec/v0.7.18-orphan-flight-
    // cleanup.md §B-011 Datenpfad-Hinweis (Option 1: Schema erweitern und
    // schauen was kommt; bei Bedarf via Fleet-Lookup nachreichen).
    //
    // v0.7.18 (R1-3): `flight_id` über `de_opt_str_or_int` deserialisieren —
    // phpVMS liefert je nach Installation als String ("flightid_1") ODER
    // als Zahl (numeric flight_id). Ohne den Deserializer würde
    // get_user_pireps() bei einem einzigen numeric-flight_id-PIREP komplett
    // failen.
    #[serde(default, deserialize_with = "de_opt_str_or_int")]
    pub flight_id: Option<String>,
    #[serde(default)]
    pub aircraft_icao: Option<String>,
    #[serde(default)]
    pub aircraft_registration: Option<String>,
    /// `created_at` aus phpVMS PirepResource — ISO-8601 String. Wird
    /// vom Frontend zu „vor X h Y min" konvertiert.
    #[serde(default)]
    pub created_at: Option<String>,
}

/// v0.5.33: Subfleet mit nested Aircraft-Liste, wie phpVMS-V7
/// SubfleetResource sie liefert (`/api/fleet`, `/api/user/fleet`).
///
/// **Korrektur ggü. v0.5.32**: Wir hatten faelschlich angenommen es
/// gaebe einen `/api/fleet/{id}/aircraft`-Endpoint — den gibt es nicht.
/// SubfleetResource enthaelt das Aircraft-Array bereits inline:
///
/// ```php
/// // SubfleetResource::toArray
/// $res['aircraft'] = AircraftResource::collection($this->aircraft);
/// ```
///
/// Also: einmal `/api/user/fleet` abrufen, ueber alle Subfleets iterieren,
/// `aircraft` flatten — fertig. Kein N+1.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubfleetWithAircraft {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icao: Option<String>,
    #[serde(default, rename = "type")]
    pub subfleet_type: Option<String>,
    /// Nested Aircraft-Liste — bereits in der Subfleet-Response
    /// enthalten. Default = leer falls phpVMS-Version das Feld
    /// nicht liefert.
    #[serde(default)]
    pub aircraft: Vec<AircraftDetails>,
}

/// Subset of `GET /api/fleet/aircraft/{id}` we use for diagnostic purposes,
/// e.g. when phpVMS rejects a prefile with `aircraft-not-available`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AircraftDetails {
    pub id: i64,
    #[serde(default)]
    pub registration: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub icao: Option<String>,
    /// ICAO of the airport where the aircraft is currently parked.
    #[serde(default)]
    pub airport_id: Option<String>,
    /// 0 = parked, 1 = in use, 2 = in flight (phpVMS AircraftState).
    #[serde(default)]
    pub state: Option<i32>,
    /// "A" active, "S" stored, etc.
    #[serde(default)]
    pub status: Option<String>,
}

// v0.12.12-dev: VA-News-Post aus `GET /api/news`. phpVMS liefert je
// nach Version + Plugin-Config unterschiedliche Felder — wir bleiben
// permissiv (alle optionalen Felder via `serde(default)`), damit ein
// fehlendes `updated_at`/`author` nicht die ganze Liste killt.
// `body` ist HTML — Frontend sanitiziert vor dem Render.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    #[serde(deserialize_with = "de_int_or_str")]
    pub id: i64,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    /// Optional flat author field (manche Installs liefern das so).
    #[serde(default)]
    pub author: Option<String>,
    /// Optional nested user-Objekt — phpVMS-Standard liefert das
    /// via Resource-Include. Wir nehmen `name` daraus wenn der flat
    /// `author`-Pfad nicht greift (mapping macht das Frontend).
    #[serde(default)]
    pub user: Option<NewsUser>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsUser {
    #[serde(default)]
    pub name: Option<String>,
}

// phpVMS resource responses are wrapped: `{ "data": {...} }`.
#[derive(Deserialize)]
struct DataEnvelope<T> {
    data: T,
}

// ---- Client ----

/// A reusable client. `Clone` is cheap because the inner reqwest client is
/// `Arc`-backed and `Connection` only holds a URL + API key string.
#[derive(Clone)]
pub struct Client {
    http: HttpClient,
    conn: Connection,
}

impl Client {
    pub fn new(conn: Connection) -> Result<Self, ApiError> {
        let user_agent = format!("AeroACARS/{}", env!("CARGO_PKG_VERSION"));
        let http = HttpClient::builder()
            .user_agent(user_agent)
            .timeout(DEFAULT_TIMEOUT)
            // v0.5.49 — siehe Konstanten-Block oben für Begründung jedes
            // einzelnen Settings. tl;dr: gegen NAT-Eviction + tote
            // TCP-Verbindungen die den Streamer-Tick blockieren.
            .connect_timeout(CONNECT_TIMEOUT)
            .tcp_keepalive(TCP_KEEPALIVE)
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .pool_max_idle_per_host(8)
            .build()
            .map_err(ApiError::from)?;
        Ok(Self { http, conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    fn endpoint(&self, path: &str) -> Result<Url, ApiError> {
        let path = path.trim_start_matches('/');
        let joined = format!(
            "{}/{}",
            self.conn.base_url.as_str().trim_end_matches('/'),
            path
        );
        Url::parse(&joined).map_err(|_| ApiError::InvalidUrl(joined))
    }

    /// GET a `{ "data": T }` resource and decode it.
    async fn get_data<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let url = self.endpoint(path)?;
        let response = self
            .http
            .get(url)
            .header("X-API-Key", &self.conn.api_key)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(ApiError::from)?;

        let response = check_status(response, path).await?;
        // Read the body as text so we can log a snippet on decode failure —
        // the default error from `response.json()` doesn't include the offending
        // JSON, which makes API-shape mismatches very hard to diagnose.
        let body = response
            .text()
            .await
            .map_err(|e| ApiError::BadResponse(format!("read body for {path}: {e}")))?;
        // Logged at DEBUG so it's silent by default but available for diagnosing
        // schema mismatches on a new VA via `RUST_LOG=api_client=debug`.
        if tracing::enabled!(tracing::Level::DEBUG) {
            let head: String = body.chars().take(2000).collect();
            tracing::debug!(path = %path, body_len = body.len(), head = %head, "response body");
        }
        match serde_json::from_str::<DataEnvelope<T>>(&body) {
            Ok(envelope) => Ok(envelope.data),
            Err(e) => {
                let snippet: String = body.chars().take(800).collect();
                tracing::warn!(
                    path = %path,
                    error = %e,
                    body_len = body.len(),
                    body_snippet = %snippet,
                    "JSON decode failed"
                );
                Err(ApiError::BadResponse(format!(
                    "JSON decode failed for {path}: {e}"
                )))
            }
        }
    }

    /// `GET /api/user`
    pub async fn get_profile(&self) -> Result<Profile, ApiError> {
        self.get_data("/api/user").await
    }

    /// `GET /api/user/bids`
    pub async fn get_bids(&self) -> Result<Vec<Bid>, ApiError> {
        self.get_data("/api/user/bids").await
    }

    /// `GET /api/user/pireps` — pilot's PIREPs (any state).
    ///
    /// **Pagination warning**: phpVMS returns `paginate()` here — 20 per
    /// page, newest first — and this method only ever reads page 1. So the
    /// result is "the 20 most recent PIREPs of any state", NOT the complete
    /// list. Use this only for callers that genuinely want *recent* PIREPs.
    /// Anything that hunts for IN_PROGRESS entries must use
    /// [`get_user_pireps_in_progress`](Self::get_user_pireps_in_progress)
    /// instead — see the 2026-06-13 incident note there.
    pub async fn get_user_pireps(&self) -> Result<Vec<PirepSummary>, ApiError> {
        self.get_data("/api/user/pireps").await
    }

    /// `GET /api/user/pireps?state=0` — ONLY the pilot's IN_PROGRESS PIREPs,
    /// filtered **server-side**.
    ///
    /// Why a dedicated method: `/api/user/pireps` is paginated (20 per page,
    /// newest first) and `get_user_pireps()` only reads page 1. Any stuck /
    /// orphaned IN_PROGRESS PIREP older than the pilot's last ~20 flights was
    /// therefore INVISIBLE to every fetch-then-filter consumer (orphan-cleanup
    /// panel, resume discovery, prefile collision check) — pilots could not
    /// self-clean and an admin had to delete by hand (24 such corpses were
    /// cleaned manually in production on 2026-06-13). The server-side filter
    /// beats pagination: phpVMS `UserController::pireps` applies
    /// `$where['state']` when the query param is present, and a pilot has at
    /// most a handful of IN_PROGRESS PIREPs at once, so page 1 is always the
    /// complete set.
    pub async fn get_user_pireps_in_progress(
        &self,
    ) -> Result<Vec<PirepSummary>, ApiError> {
        self.get_data(USER_PIREPS_IN_PROGRESS_PATH).await
    }

    /// `GET /api/airports/{icao}` — single airport lookup with coordinates.
    pub async fn get_airport(&self, icao: &str) -> Result<Airport, ApiError> {
        let path = format!("/api/airports/{}", icao.trim().to_uppercase());
        self.get_data(&path).await
    }

    /// v0.13.x (In-App-Live-Map, VA-Übersicht): öffentlicher Live-ACARS-Feed
    /// (alle aktiven Flüge der VA mit Position) als rohe JSON. Nutzt den
    /// konfigurierten HTTP-Client (gleicher TLS-Pfad wie alle anderen Calls) —
    /// vermeidet den rustls-CryptoProvider-Stolperstein eines frisch gebauten
    /// reqwest::Client. `/api/acars` ist public (kein API-Key nötig).
    pub async fn get_acars_live(&self) -> Result<serde_json::Value, ApiError> {
        let url = self.endpoint("/api/acars")?;
        let response = self
            .http
            .get(url)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(ApiError::from)?;
        let response = check_status(response, "/api/acars").await?;
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| ApiError::BadResponse(format!("read /api/acars: {e}")))
    }

    /// StratosLogbook-Modul (GSG phpVMS): liest das Pilot-Logbuch LIVE über die
    /// API — nichts gespeichert. Auth läuft über den phpVMS-`api_key` als
    /// Bearer-Token (StratosAuth prüft `User::where('api_key', bearerToken)`).
    /// Wir senden zusätzlich X-API-Key, schadet nicht.
    async fn get_logbook_json(&self, path: &str) -> Result<serde_json::Value, ApiError> {
        let url = self.endpoint(path)?;
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.conn.api_key)
            .header("X-API-Key", &self.conn.api_key)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(ApiError::from)?;
        let response = check_status(response, path).await?;
        response
            .json::<serde_json::Value>()
            .await
            .map_err(|e| ApiError::BadResponse(format!("read {path}: {e}")))
    }

    /// Logbuch-Flugliste (paginiert): `{ items, total, limit, offset }`.
    pub async fn get_logbook_pireps(&self, limit: u32, offset: u32) -> Result<serde_json::Value, ApiError> {
        self.get_logbook_json(&format!("/api/stratos/logbook/pireps?limit={limit}&offset={offset}"))
            .await
    }

    /// Logbuch-Summen (Flüge, Stunden, Distanz, Ø-Landung, Rang).
    pub async fn get_logbook_stats(&self) -> Result<serde_json::Value, ApiError> {
        self.get_logbook_json("/api/stratos/logbook/stats").await
    }

    /// Logbuch-Detail eines PIREP inkl. `route` (Track) + `log` (Fluglogbuch).
    pub async fn get_logbook_pirep(&self, id: &str) -> Result<serde_json::Value, ApiError> {
        self.get_logbook_json(&format!("/api/stratos/logbook/pireps/{id}")).await
    }

    /// POST a JSON body and decode the response envelope `{ data: T }`.
    async fn post_data<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ApiError> {
        let url = self.endpoint(path)?;
        let response = self
            .http
            .post(url)
            .header("X-API-Key", &self.conn.api_key)
            .header(header::ACCEPT, "application/json")
            .json(body)
            .send()
            .await
            .map_err(ApiError::from)?;
        let response = check_status(response, path).await?;
        let text = response
            .text()
            .await
            .map_err(|e| ApiError::BadResponse(format!("read body for {path}: {e}")))?;
        if tracing::enabled!(tracing::Level::DEBUG) {
            let head: String = text.chars().take(2000).collect();
            tracing::debug!(path = %path, body_len = text.len(), head = %head, "post response");
        }
        match serde_json::from_str::<DataEnvelope<T>>(&text) {
            Ok(envelope) => Ok(envelope.data),
            Err(e) => {
                tracing::warn!(path = %path, error = %e, "JSON decode failed for POST response");
                Err(ApiError::BadResponse(format!(
                    "JSON decode failed for {path}: {e}"
                )))
            }
        }
    }

    /// POST and ignore the response body (status-check only).
    async fn post_void<B: Serialize>(&self, path: &str, body: &B) -> Result<(), ApiError> {
        let url = self.endpoint(path)?;
        let response = self
            .http
            .post(url)
            .header("X-API-Key", &self.conn.api_key)
            .header(header::ACCEPT, "application/json")
            .json(body)
            .send()
            .await
            .map_err(ApiError::from)?;
        let _ = check_status(response, path).await?;
        Ok(())
    }

    /// DELETE and ignore the response body (status-check only).
    async fn delete_void(&self, path: &str) -> Result<(), ApiError> {
        let url = self.endpoint(path)?;
        let response = self
            .http
            .delete(url)
            .header("X-API-Key", &self.conn.api_key)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(ApiError::from)?;
        let _ = check_status(response, path).await?;
        Ok(())
    }

    /// `DELETE /api/user/bids` — drop a bid after its PIREP was filed
    /// (or to give it back). phpVMS does NOT auto-consume bids when the
    /// PIREP is filed unless we explicitly remove them.
    ///
    /// phpVMS routes ALL bid CRUD through `/api/user/bids` (kein `{id}` im
    /// Pfad). Die ID reist im JSON-Body als `{ "bid_id": ... }` ODER
    /// `{ "flight_id": ... }`. v0.7.18 (B-011) erlaubt beide Eingaben.
    ///
    /// **Body-Auswahl-Reihenfolge** (siehe Spec §B-011 Backend-API):
    ///   - `bid_id` Some → erster Versuch `{ bid_id }`.
    ///     Bei 422/400 UND `flight_id` Some → zweiter Versuch
    ///     `{ flight_id }`.
    ///   - `bid_id` None, `flight_id` Some → direkt `{ flight_id }`.
    ///   - beide None → `ApiError::InvalidUrl` (Caller-Bug).
    ///
    /// Aus Spec-§B-011 Decision: **kein 405-Fallback hier** (anders als bei
    /// `cancel_pirep`). Wenn ein VA-Install POST mit `_method=DELETE`
    /// braucht, machen wir das als separate Aufgabe nach v0.7.18.
    pub async fn delete_bid(
        &self,
        bid_id: Option<i64>,
        flight_id: Option<&str>,
    ) -> Result<(), ApiError> {
        // phpVMS routes ALL bid CRUD through `/api/user/bids` — there
        // is NO `/api/user/bids/{id}` or `/api/bids/{id}` route, despite
        // what previous reverse-engineering of vmsACARS' binary
        // suggested (the strings we saw there are likely alternative
        // routes added by VA installs, or just unused).
        //
        // Verified against the canonical phpVMS source at
        // github.com/nabeelio/phpvms (`Api/UserController::bids`):
        //
        //     if ($request->isMethod('DELETE')) {
        //         if ($request->filled('bid_id')) {
        //             $bid = Bid::findOrFail($request->input('bid_id'));
        //             ...
        //             $this->bidSvc->removeBid($flight, $user);
        //         } elseif ($request->filled('flight_id')) {
        //             ...
        //         }
        //     }
        //
        // → entweder `bid_id` ODER `flight_id` reisen im Body. JSON ist
        // fine weil Laravel's `$request->input(...)` JSON-Body / Form-
        // Body / Query-String transparent liest.
        //
        // v0.7.18 (B-011): Signatur akzeptiert beide IDs als Option.
        // Aufruf-Logik:
        //   - bid_id Some → erster Versuch `{ bid_id }`. Bei 400/404/422
        //     UND flight_id Some → zweiter Versuch `{ flight_id }`.
        //   - bid_id None, flight_id Some → direkt `{ flight_id }`.
        //   - beide None → InvalidUrl-Error (Caller-Bug).
        //
        // v0.7.18 (R2-2): 404 in den Fallback aufgenommen. Wenn die VA
        // den bid_id-Wert serverseitig schon nicht mehr kennt (verwaister
        // PIREP, Bid wurde manuell gedroppt etc.), liefert phpVMS 404 statt
        // 422. Der flight_id-Pfad ist dann der einzige Weg den Bid zu
        // droppen — genau der Orphan-Cleanup-Fall den B-011 fixt.
        //
        // Manche VAs liefern in `PirepSummary` kein `bid_id`, dann ist
        // der flight_id-Pfad der einzige Weg den Bid serverseitig zu
        // droppen.
        let path = "/api/user/bids";

        #[derive(serde::Serialize)]
        struct BidIdBody {
            bid_id: i64,
        }
        #[derive(serde::Serialize)]
        struct FlightIdBody<'a> {
            flight_id: &'a str,
        }

        // Versuch 1: bid_id wenn vorhanden.
        if let Some(bid_id) = bid_id {
            let url = self.endpoint(path)?;
            let response = self
                .http
                .delete(url)
                .header("X-API-Key", &self.conn.api_key)
                .header(header::ACCEPT, "application/json")
                .json(&BidIdBody { bid_id })
                .send()
                .await
                .map_err(ApiError::from)?;
            let status = response.status();
            // Bei 400/404/422 (Validation-Fehler oder „bid nicht gefunden"
            // vom phpVMS-Plugin) und wenn ein flight_id-Fallback verfuegbar
            // ist: Versuch 2. Bei allen anderen Status-Codes (inkl. 2xx)
            // durch check_status laufen lassen.
            if matches!(status.as_u16(), 400 | 404 | 422) && flight_id.is_some() {
                tracing::info!(
                    bid_id,
                    status = status.as_u16(),
                    "delete_bid: bid_id-Body abgelehnt, versuche flight_id-Fallback"
                );
                // Body verbrauchen damit kein Connection-Reuse-Glitch
                let _ = response.text().await;
            } else {
                let _ = check_status(response, path).await?;
                return Ok(());
            }
        }

        // Versuch 2 (oder direkt wenn bid_id=None): flight_id-Body.
        if let Some(flight_id) = flight_id {
            let url = self.endpoint(path)?;
            let response = self
                .http
                .delete(url)
                .header("X-API-Key", &self.conn.api_key)
                .header(header::ACCEPT, "application/json")
                .json(&FlightIdBody { flight_id })
                .send()
                .await
                .map_err(ApiError::from)?;
            let _ = check_status(response, path).await?;
            return Ok(());
        }

        // Beide None → Caller-Bug.
        Err(ApiError::InvalidUrl(
            "delete_bid called with both bid_id and flight_id = None".into(),
        ))
    }

    /// `POST /api/pireps/prefile` — create an in-flight PIREP.
    pub async fn prefile_pirep(&self, body: &PrefileBody) -> Result<PirepCreated, ApiError> {
        self.post_data("/api/pireps/prefile", body).await
    }

    /// Fetch the public SimBrief OFP XML for a given OFP id and
    /// extract the weights + fuel plan we care about. The endpoint
    /// is `https://www.simbrief.com/ofp/flightplans/xml/{id}.xml` —
    /// no auth needed; SimBrief OFPs are public-by-id.
    ///
    /// Weights / fuel are normalised to KG regardless of the OFP's
    /// `<units><wt_unit>` setting (kgs vs lbs).
    ///
    /// Returns `Ok(None)` when the OFP is missing or malformed —
    /// flight_start should treat that as "no plan, no comparison"
    /// rather than refusing to start.
    pub async fn fetch_simbrief_ofp(
        &self,
        ofp_id: &str,
    ) -> Result<Option<SimBriefOfp>, ApiError> {
        let url = format!(
            "https://www.simbrief.com/ofp/flightplans/xml/{}.xml",
            urlencoding_escape(ofp_id)
        );
        let response = match self.http.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, ofp_id, "SimBrief OFP fetch failed (network)");
                return Ok(None);
            }
        };
        if !response.status().is_success() {
            tracing::warn!(
                status = %response.status(),
                ofp_id,
                "SimBrief OFP fetch returned non-2xx",
            );
            return Ok(None);
        }
        let xml = match response.text().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "SimBrief OFP body read failed");
                return Ok(None);
            }
        };
        Ok(parse_simbrief_ofp(&xml))
    }

    /// v0.7.8: SimBrief-direct OFP-Fetch via `xml.fetcher.php?username=X`
    /// oder `?userid=X` (= "Fetching a User's Latest OFP Data" laut
    /// Navigraph-Doku). Bypasst den phpVMS-Bid-Pointer → funktioniert
    /// auch wenn der Bid nach Prefile entfernt wurde (W5).
    ///
    /// Pfad-Auswahl:
    /// - `user_id` (numerisch) → `?userid={user_id}` (Navigraph-Empfehlung,
    ///   robuster weil unveraenderlich)
    /// - sonst `username` → `?username={username}` (offiziell ebenfalls
    ///   unterstuetzt, einfacher zu finden im Profile-URL)
    ///
    /// Error-Detection (Spec §3.3 v1.3 — beide Pfade abdecken):
    /// - HTTP 400 ODER `<fetch><status>Error</status>` → `UserNotFound`
    /// - HTTP 5xx → `Unavailable`
    /// - andere non-2xx → `Network`
    /// - Parse-Fehler nach Status-OK → `ParseFailed`
    /// - Network/IO-Error vor Response → `Network`
    ///
    /// Spec docs/spec/ofp-refresh-simbrief-direct-v0.7.8.md §3.
    pub async fn fetch_simbrief_direct(
        &self,
        user_id: Option<&str>,
        username: Option<&str>,
    ) -> Result<SimBriefOfp, SimBriefDirectError> {
        // user_id > username Prioritaet (Spec §4.1 — robuster, unveraenderlich)
        let url = if let Some(uid) = user_id.filter(|s| !s.is_empty()) {
            format!(
                "https://www.simbrief.com/api/xml.fetcher.php?userid={}",
                urlencoding_escape(uid),
            )
        } else if let Some(un) = username.filter(|s| !s.is_empty()) {
            format!(
                "https://www.simbrief.com/api/xml.fetcher.php?username={}",
                urlencoding_escape(un),
            )
        } else {
            return Err(SimBriefDirectError::NoIdentifier);
        };

        let response = self.http.get(&url).send().await.map_err(|e| {
            tracing::warn!(error = %e, "SimBrief-direct network error");
            SimBriefDirectError::Network
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::BAD_REQUEST {
            // Navigraph-Doku-Pfad: invalid user → HTTP 400 + small XML error
            tracing::warn!(%status, "SimBrief-direct: HTTP 400, treating as UserNotFound");
            return Err(SimBriefDirectError::UserNotFound);
        }
        if status.is_server_error() {
            tracing::warn!(%status, "SimBrief-direct: server error, Unavailable");
            return Err(SimBriefDirectError::Unavailable);
        }
        if !status.is_success() {
            tracing::warn!(%status, "SimBrief-direct: unexpected non-2xx status");
            return Err(SimBriefDirectError::Network);
        }

        let xml = response.text().await.map_err(|e| {
            tracing::warn!(error = %e, "SimBrief-direct body read failed");
            SimBriefDirectError::Network
        })?;

        // Live-Probe-Pfad: HTTP 200 + <fetch><status>Error</status>
        // moeglich. Status MUSS gepruefped werden, nicht nur HTTP-Code.
        let fetch_status = extract_tag(&xml, "fetch")
            .and_then(|inner| extract_tag(inner, "status"))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if fetch_status != "Success" {
            tracing::warn!(
                fetch_status = %fetch_status,
                "SimBrief-direct: <fetch><status> not Success, treating as UserNotFound",
            );
            return Err(SimBriefDirectError::UserNotFound);
        }

        parse_simbrief_ofp(&xml).ok_or_else(|| {
            tracing::warn!("SimBrief-direct: XML parse failed");
            SimBriefDirectError::ParseFailed
        })
    }

    /// `POST /api/pireps/{pirep_id}/acars/position` — push a batch of positions.
    pub async fn post_positions(
        &self,
        pirep_id: &str,
        positions: &[PositionEntry],
    ) -> Result<(), ApiError> {
        #[derive(Serialize)]
        struct Body<'a> {
            positions: &'a [PositionEntry],
        }
        let path = format!("/api/pireps/{pirep_id}/acars/position");
        self.post_void(&path, &Body { positions }).await
    }

    /// `POST /api/pireps/{pirep_id}/file` — submit the PIREP.
    pub async fn file_pirep(&self, pirep_id: &str, body: &FileBody) -> Result<(), ApiError> {
        let path = format!("/api/pireps/{pirep_id}/file");
        self.post_void(&path, body).await
    }

    /// `DELETE /api/pireps/{pirep_id}/cancel` — cancel an in-flight PIREP,
    /// with `PUT` as fallback for VAs that have stricter method routing.
    ///
    /// v0.7.18 (R1-1, B-011): vorher `POST`. Standard-phpVMS-Core erwartet
    /// DELETE; manche VA-Installs (oder hinter Restrictive-Reverse-Proxies)
    /// liefern dafür 405 und akzeptieren PUT. POST war im Pilot-Client-Code
    /// ein Pre-v0.7.18-Drift gegen die Server-Spec.
    pub async fn cancel_pirep(&self, pirep_id: &str) -> Result<(), ApiError> {
        let path = format!("/api/pireps/{pirep_id}/cancel");
        let url = self.endpoint(&path)?;

        // 1. Versuch: DELETE — phpVMS-Core-Standard.
        let response = self
            .http
            .delete(url.clone())
            .header("X-API-Key", &self.conn.api_key)
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(ApiError::from)?;

        let status = response.status();
        if status.is_success() {
            let _ = response.text().await; // Body verbrauchen
            return Ok(());
        }
        // 405 Method Not Allowed → PUT-Fallback
        // (manche VA-Installs blockieren DELETE auf API-Routes)
        if status == StatusCode::METHOD_NOT_ALLOWED {
            tracing::info!(
                pirep_id,
                "cancel_pirep: DELETE returned 405 — versuche PUT-Fallback"
            );
            let _ = response.text().await;
            let response_put = self
                .http
                .put(url)
                .header("X-API-Key", &self.conn.api_key)
                .header(header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(ApiError::from)?;
            let _ = check_status(response_put, &path).await?;
            return Ok(());
        }
        // Andere Fehler durch check_status → typisierte ApiError
        let _ = check_status(response, &path).await?;
        Ok(())
    }

    /// `POST /api/pireps/{pirep_id}/update` — change PIREP status/state during flight.
    /// Also serves as the periodic heartbeat (see `UpdateBody` doc).
    pub async fn update_pirep(&self, pirep_id: &str, body: &UpdateBody) -> Result<(), ApiError> {
        let path = format!("/api/pireps/{pirep_id}/update");
        self.post_void(&path, body).await
    }

    /// `POST /api/pireps/{pirep_id}/acars/logs` — push a batch of free-form
    /// text log lines that show up in the PIREP detail. Used for sub-events
    /// without a PirepStatus equivalent (TOC, TOD, V1/VR/V2, touchdown VS,
    /// engine start/stop, etc.).
    pub async fn post_acars_logs(
        &self,
        pirep_id: &str,
        logs: &[LogEntry],
    ) -> Result<(), ApiError> {
        #[derive(Serialize)]
        struct Body<'a> {
            logs: &'a [LogEntry],
        }
        let path = format!("/api/pireps/{pirep_id}/acars/logs");
        self.post_void(&path, &Body { logs }).await
    }

    /// `POST /api/pireps/{pirep_id}/route` — upload the planned flight-plan
    /// waypoints (e.g. from the SimBrief OFP). phpVMS draws these as a
    /// dotted line on the PIREP map alongside the actually flown track.
    /// Send once shortly after prefile.
    pub async fn post_route(
        &self,
        pirep_id: &str,
        route: &[RouteWaypoint],
    ) -> Result<(), ApiError> {
        #[derive(Serialize)]
        struct Body<'a> {
            route: &'a [RouteWaypoint],
        }
        let path = format!("/api/pireps/{pirep_id}/route");
        self.post_void(&path, &Body { route }).await
    }

    /// `POST /api/pireps/{pirep_id}/fields` — upsert custom PIREP fields the
    /// VA admin defined in the phpVMS admin panel (e.g. "Block fuel",
    /// "Pilot remarks", "Diversion?"). Map keys are the field NAMES (as
    /// shown in the admin), values are stringified.
    pub async fn post_pirep_fields(
        &self,
        pirep_id: &str,
        fields: &std::collections::HashMap<String, String>,
    ) -> Result<(), ApiError> {
        #[derive(Serialize)]
        struct Body<'a> {
            fields: &'a std::collections::HashMap<String, String>,
        }
        let path = format!("/api/pireps/{pirep_id}/fields");
        self.post_void(&path, &Body { fields }).await
    }

    /// `GET /api/pireps/{pirep_id}` — fetch the current server-side state of
    /// a single PIREP. Used for resume-after-restart and to disambiguate
    /// 404s on the live POST endpoints (soft-deleted by cron vs. cancelled
    /// elsewhere vs. auth lost).
    pub async fn get_pirep(&self, pirep_id: &str) -> Result<PirepFull, ApiError> {
        let path = format!("/api/pireps/{pirep_id}");
        self.get_data(&path).await
    }

    /// `GET /api/pireps/{pirep_id}/acars/position` — the complete server-side
    /// ACARS position track for a PIREP, ordered by sim_time ASC (oldest →
    /// newest) and unpaginated. This endpoint is under `api.auth` (same
    /// `X-API-Key` we already send) and returns the standard `{ data: [...] }`
    /// envelope.
    ///
    /// Used on flight-resume to reseed the in-app live-map track from the
    /// authoritative superset, so a mid-flight update/restart can't leave the
    /// local track empty or frozen behind a stale localStorage seed-gate.
    pub async fn get_acars_positions(
        &self,
        pirep_id: &str,
    ) -> Result<Vec<AcarsPosition>, ApiError> {
        self.get_data(&format!("/api/pireps/{pirep_id}/acars/position"))
            .await
    }

    /// `GET /api/fleet/aircraft/{id}` — single aircraft, used for diagnostics.
    /// We also try `/api/aircraft/{id}` as a fallback because phpVMS deployments
    /// vary on this exact path.
    pub async fn get_aircraft(&self, id: i64) -> Result<AircraftDetails, ApiError> {
        match self
            .get_data::<AircraftDetails>(&format!("/api/fleet/aircraft/{id}"))
            .await
        {
            Ok(a) => Ok(a),
            Err(ApiError::NotFound) => {
                self.get_data(&format!("/api/aircraft/{id}")).await
            }
            Err(e) => Err(e),
        }
    }

    /// v0.5.27: `GET /api/airports/{icao}/aircraft` — Aircraft die aktuell
    /// am Departure-Airport stehen. Fuer den VFR/Manual-Mode-Aircraft-
    /// Picker. Filter `?status=active` damit Maintenance-Aircraft nicht
    /// in der Liste landen.
    ///
    /// phpVMS gibt nur Aircraft zurueck die der Pilot per Subfleet-Rank
    /// fliegen darf — Server-side enforcement, kein Client-Filter noetig.
    pub async fn get_aircraft_at_airport(&self, icao: &str) -> Result<Vec<AircraftDetails>, ApiError> {
        let path = format!("/api/airports/{}/aircraft", icao.to_uppercase());
        self.get_data(&path).await
    }

    /// v0.5.33: `GET /api/fleet` — **alle** Subfleets ohne Rank-Filter.
    ///
    /// Wir verwenden bewusst NICHT `/api/user/fleet` — das wuerde via
    /// `UserService::getAllowableSubfleets` nur Subfleets liefern die
    /// der aktuelle Pilot-Rank fliegen darf. Stattdessen vertrauen wir
    /// dem Pilot bei der Auswahl, und phpVMS lehnt beim Prefile ab
    /// wenn er eine Aircraft ausserhalb seines Ranks waehlt.
    ///
    /// Jeder Subfleet enthaelt bereits eine nested `aircraft`-Liste
    /// (siehe SubfleetResource::toArray). Paginiert — wir folgen
    /// den Pages bis wir eine non-volle Page bekommen.
    ///
    /// **Korrektur ggü. v0.5.32**: Der vorherige Fix versuchte
    /// `/api/fleet/{id}/aircraft` — diesen Endpoint gibt es in phpVMS-V7
    /// **nicht** (nur `/api/fleet/aircraft/{id}` fuer ein einzelnes
    /// Aircraft). Resultat war eine leere Picker-Liste.
    pub async fn get_fleet(&self) -> Result<Vec<SubfleetWithAircraft>, ApiError> {
        self.get_paginated_subfleets("/api/fleet").await
    }

    /// Hilfsfunktion: paginierten Subfleet-Endpoint komplett durchziehen.
    ///
    /// Phpvms `paginate_limit` clamped `?limit=` auf
    /// `phpvms.pagination.max` (default 100). Wir fragen mit limit=100
    /// per Page und folgen den Pages so lange wir volle Pages bekommen.
    /// Sicherheits-Cap bei 50 Pages = 5000 Subfleets.
    async fn get_paginated_subfleets(
        &self,
        base_path: &str,
    ) -> Result<Vec<SubfleetWithAircraft>, ApiError> {
        const PAGE_LIMIT: usize = 100;
        const MAX_PAGES: u32 = 50;
        let mut all = Vec::new();
        for page in 1..=MAX_PAGES {
            let path = format!("{base_path}?limit={PAGE_LIMIT}&page={page}");
            let chunk: Vec<SubfleetWithAircraft> = self.get_data(&path).await?;
            let n = chunk.len();
            tracing::debug!(
                page,
                page_size = n,
                cumulative = all.len() + n,
                "fetched subfleet page"
            );
            all.extend(chunk);
            if n < PAGE_LIMIT {
                break; // letzte Page erreicht
            }
        }
        Ok(all)
    }

    /// v0.5.32 (gefixt v0.5.33): alle einzelnen Aircraft die der Pilot
    /// fliegen darf — flach. Liest `aircraft` aus jedem Subfleet
    /// (bereits nested in der phpVMS-Response, kein N+1).
    pub async fn get_all_aircraft(&self) -> Result<Vec<AircraftDetails>, ApiError> {
        let subfleets = self.get_fleet().await?;
        let total_subfleets = subfleets.len();
        let mut all = Vec::new();
        for sf in subfleets {
            all.extend(sf.aircraft);
        }
        tracing::info!(
            subfleets = total_subfleets,
            aircraft = all.len(),
            "get_all_aircraft completed"
        );
        Ok(all)
    }

    /// v0.12.12-dev: `GET /api/news` — VA-News-Posts. phpVMS liefert
    /// das paginiert mit `{ data: [...], meta: {...} }`. Wir nehmen
    /// nur `data` und verzichten auf Meta — der Client-Tab zeigt
    /// Seite 1 mit `per_page` Items (Default 20) und braucht keine
    /// Pagination.
    pub async fn get_news(&self, per_page: u32) -> Result<Vec<NewsItem>, ApiError> {
        let path = format!("/api/news?per_page={}", per_page.max(1).min(100));
        self.get_data(&path).await
    }
}

async fn check_status(response: Response, path: &str) -> Result<Response, ApiError> {
    let status = response.status();
    // v0.7.18 (R1-2): alle 2xx akzeptieren, nicht nur 200 OK.
    // DELETE-Endpoints (cancel_pirep, delete_bid) liefern oft 204
    // No Content. PUT/POST koennen 201 Created liefern.
    // Vorher wurde 204 als ApiError::Server { status: 204 } behandelt
    // → B-011 Cancel-Flow scheiterte still gegen Standard-phpVMS.
    if status.is_success() {
        return Ok(response);
    }
    match status {
        StatusCode::UNAUTHORIZED => Err(ApiError::Unauthenticated),
        StatusCode::FORBIDDEN => Err(ApiError::Forbidden),
        StatusCode::NOT_FOUND => Err(ApiError::NotFound),
        StatusCode::TOO_MANY_REQUESTS => {
            let retry_after_seconds = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(60);
            Err(ApiError::RateLimited { retry_after_seconds })
        }
        s => {
            let body = response.text().await.unwrap_or_default();
            tracing::warn!(%s, %path, body_len = body.len(), "phpVMS returned non-OK");
            Err(ApiError::Server {
                status: s.as_u16(),
                body,
            })
        }
    }
}

// ---- SimBrief OFP XML parser ----

/// Lazy URL-encode helper for the OFP id. SimBrief ids are
/// `[A-Za-z0-9_-]` so we technically don't need to encode anything,
/// but we'll be defensive in case they ever change format.
fn urlencoding_escape(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// Pull the inner text of the FIRST `<tag>...</tag>` occurrence.
/// Naive but adequate for SimBrief's well-formed OFP XML — every
/// field we want is a leaf scalar, no nested duplicates.
fn extract_tag<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let rest = &xml[start..];
    let end = rest.find(&close)?;
    Some(&rest[..end])
}

/// Parse the SimBrief OFP XML into our `SimBriefOfp` struct. All
/// weights returned in KG; if the OFP was generated in lbs we
/// convert. Returns None when the document is too malformed to be
/// useful (no weights block at all).
fn parse_simbrief_ofp(xml: &str) -> Option<SimBriefOfp> {
    // Conversion factor: SimBrief reports either kgs or lbs.
    let unit_is_lb = matches!(extract_tag(xml, "wt_unit"), Some("lbs"));
    let to_kg = |v: f32| -> f32 {
        if unit_is_lb { v * 0.453_592_37 } else { v }
    };

    let parse_f = |tag: &str| -> Option<f32> {
        extract_tag(xml, tag)
            .and_then(|s| s.trim().parse::<f32>().ok())
    };

    // Required: at least one weight field has to be present, else
    // the OFP is too broken to use.
    let zfw = parse_f("est_zfw").map(to_kg).unwrap_or(0.0);
    let tow = parse_f("est_tow").map(to_kg).unwrap_or(0.0);
    let ldw = parse_f("est_ldw").map(to_kg).unwrap_or(0.0);
    if zfw == 0.0 && tow == 0.0 && ldw == 0.0 {
        return None;
    }
    let plan_ramp = parse_f("plan_ramp").map(to_kg).unwrap_or(0.0);
    // Trip-Burn: SimBrief liefert das als `<enroute_burn>` unter
    // `<fuel>` — der reine Verbrauch von Takeoff bis Touchdown.
    // Fallback auf `<est_burn>` falls ein älteres SimBrief-Schema
    // den Tag anders genannt hat (sicherheitshalber).
    let est_burn = parse_f("enroute_burn")
        .or_else(|| parse_f("est_burn"))
        .map(to_kg)
        .unwrap_or(0.0);
    let reserve = parse_f("reserve").map(to_kg).unwrap_or(0.0);
    // v0.3.0: MAX-Werte aus dem Aircraft-Performance-Block des OFP.
    // SimBrief liefert die in `<max_zfw>` / `<max_tow>` / `<max_ldw>`
    // unter `<weights>`. Bei Custom-Subfleets können die fehlen — dann
    // bleibt's bei 0.0 und Frontend skipped die Overweight-Anzeige.
    let max_zfw = parse_f("max_zfw").map(to_kg).unwrap_or(0.0);
    let max_tow = parse_f("max_tow").map(to_kg).unwrap_or(0.0);
    let max_ldw = parse_f("max_ldw").map(to_kg).unwrap_or(0.0);
    // v0.3.0: OFP-Identitätsfelder. SimBrief liefert always den
    // letzten OFP des Pilot-Accounts — wenn der nicht zur aktuellen
    // Buchung passt, wollen wir das im Frontend deutlich anzeigen.
    // Flight-Number meist als `<atc><callsign>`, Origin/Destination
    // direkt als nested `<origin><icao_code>` und `<destination>...`.
    let extract_str = |tag: &str| -> String {
        extract_tag(xml, tag)
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };
    let ofp_flight_number = extract_tag(xml, "atc")
        .and_then(|inner| extract_tag(inner, "callsign"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| extract_str("flight_number"));
    let ofp_origin_icao = extract_tag(xml, "origin")
        .and_then(|inner| extract_tag(inner, "icao_code"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let ofp_destination_icao = extract_tag(xml, "destination")
        .and_then(|inner| extract_tag(inner, "icao_code"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let ofp_generated_at = extract_tag(xml, "params")
        .and_then(|inner| extract_tag(inner, "time_generated"))
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| extract_str("time_generated"));
    // v0.7.8: <params><request_id> als canonical changed-flag-Quelle
    // fuer SimBrief-direct Refresh-Pfad. Aendert sich bei JEDER
    // Re-Generation. Spec §3.
    let request_id = extract_tag(xml, "params")
        .and_then(|inner| extract_tag(inner, "request_id"))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let route = extract_tag(xml, "route")
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // SimBrief's <alternate>...</alternate> element is a NESTED XML
    // block (children: icao_code / iata_code / faa_code / icao_region
    // / elevation / pos_lat / pos_long / ...) — earlier we returned
    // the raw inner XML which made the PIREP-detail show
    // "<icao_code>LFBO</icao_code> <iata_code>TLS</iata_code> ..."
    // as the alternate string. Drill in to grab just the ICAO.
    let alternate = extract_tag(xml, "alternate")
        .and_then(|inner| extract_tag(inner, "icao_code"))
        .or_else(|| extract_tag(xml, "icao_code"))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let waypoints = extract_navlog_fixes(xml);

    // v0.7.12: Pax + Cargo aus <weights>. SimBrief liefert pax_count als
    // Integer + cargo als float (in kg wenn die XML mit units_set=kgs
    // angefordert wurde — was wir tun). Bei Cargo-Only-Flights ist
    // pax_count = 0, bei Pax-Only-Flights cargo = 0. Bei Mixed-Loads
    // beide > 0.
    let pax_count: i32 = extract_tag(xml, "weights")
        .and_then(|inner| extract_tag(inner, "pax_count"))
        .and_then(|s| s.trim().parse().ok())
        .or_else(|| {
            extract_tag(xml, "weights")
                .and_then(|inner| extract_tag(inner, "pax_count_actual"))
                .and_then(|s| s.trim().parse().ok())
        })
        .unwrap_or(0);
    let cargo_kg: f32 = extract_tag(xml, "weights")
        .and_then(|inner| extract_tag(inner, "cargo"))
        .and_then(|s| s.trim().parse().ok())
        .map(to_kg)
        .unwrap_or(0.0);

    Some(SimBriefOfp {
        planned_block_fuel_kg: plan_ramp,
        planned_burn_kg: est_burn,
        planned_reserve_kg: reserve,
        planned_zfw_kg: zfw,
        planned_tow_kg: tow,
        planned_ldw_kg: ldw,
        route,
        alternate,
        waypoints,
        max_zfw_kg: max_zfw,
        max_tow_kg: max_tow,
        max_ldw_kg: max_ldw,
        ofp_flight_number,
        ofp_origin_icao,
        ofp_destination_icao,
        ofp_generated_at,
        request_id,
        pax_count,
        cargo_kg,
    })
}

/// Walk `<navlog>...<fix>...</fix>...</navlog>` and pull every fix.
/// Resilient to missing `<navlog>` (older SimBrief OFP variants put fixes
/// at the document root) — falls back to scanning the whole document.
fn extract_navlog_fixes(xml: &str) -> Vec<RouteFix> {
    let scope = extract_tag(xml, "navlog").unwrap_or(xml);
    let mut out = Vec::new();
    let mut cursor = 0;
    while let Some(start) = scope[cursor..].find("<fix>") {
        let abs_start = cursor + start + "<fix>".len();
        let Some(rel_end) = scope[abs_start..].find("</fix>") else {
            break;
        };
        let block = &scope[abs_start..abs_start + rel_end];
        cursor = abs_start + rel_end + "</fix>".len();
        let ident = extract_tag(block, "ident").unwrap_or("").trim().to_string();
        let lat: Option<f64> = extract_tag(block, "pos_lat")
            .and_then(|s| s.trim().parse().ok());
        let lon: Option<f64> = extract_tag(block, "pos_long")
            .and_then(|s| s.trim().parse().ok());
        let kind = extract_tag(block, "type").unwrap_or("").trim().to_string();
        if let (true, Some(la), Some(lo)) = (!ident.is_empty(), lat, lon) {
            out.push(RouteFix { ident, lat: la, lon: lo, kind });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_scheme() {
        let err = Connection::new("ftp://example.com", "k").unwrap_err();
        assert!(matches!(err, ApiError::InvalidUrl(_)));
    }

    #[test]
    fn accepts_https() {
        Connection::new("https://example.com", "k").unwrap();
    }

    #[test]
    fn accepts_http_localhost() {
        Connection::new("http://localhost:8000", "k").unwrap();
        // loopback IPs (v4 + v6) must also pass
        Connection::new("http://127.0.0.1:8000", "k").unwrap();
        Connection::new("http://[::1]:8000", "k").unwrap();
    }

    #[test]
    fn rejects_http_to_routable_host() {
        // C2c: plaintext http to a routable host would leak the API key.
        for url in [
            "http://german-sky-group.eu",
            "http://example.com:8000",
            "http://192.168.1.5:8000", // even a LAN host: only loopback is exempt
        ] {
            let err = Connection::new(url, "k").unwrap_err();
            assert!(
                matches!(err, ApiError::InvalidUrl(_)),
                "expected InvalidUrl for {url}, got {err:?}"
            );
        }
        // https to the same hosts stays fine.
        Connection::new("https://german-sky-group.eu", "k").unwrap();
    }

    #[test]
    fn strips_trailing_slash() {
        let c = Connection::new("https://example.com/", "k").unwrap();
        assert!(!c.base_url().ends_with("//"));
    }

    /// v0.16.17: `get_user_pireps_in_progress` must hit
    /// `/api/user/pireps?state=0` — the server-side state filter is the
    /// whole point (paginate() returns only 20/page newest-first, so a
    /// client-side filter over page 1 misses older orphans; 24 corpses
    /// cleaned manually in prod 2026-06-13). Guards both that the query
    /// survives `endpoint()` URL-joining and that the path const doesn't
    /// silently drift.
    #[test]
    fn in_progress_pireps_endpoint_keeps_state_query() {
        let conn = Connection::new("https://example.com/", "k").unwrap();
        let client = Client::new(conn).unwrap();
        let url = client.endpoint(USER_PIREPS_IN_PROGRESS_PATH).unwrap();
        assert_eq!(url.as_str(), "https://example.com/api/user/pireps?state=0");
        assert_eq!(url.query(), Some("state=0"));
        assert_eq!(url.path(), "/api/user/pireps");
    }

    #[test]
    fn simbrief_alternate_extracts_clean_icao_from_nested_xml() {
        // Captured shape from a real SimBrief OFP — the <alternate>
        // element is a wrapper around child fields. Pre-fix we
        // returned the raw inner XML soup as the alternate string.
        let xml = r#"
            <ofp>
                <fuel>
                    <plan_ramp>9733</plan_ramp>
                    <est_burn>5800</est_burn>
                    <reserve>1300</reserve>
                </fuel>
                <weights>
                    <est_zfw>71242</est_zfw>
                    <est_tow>80700</est_tow>
                    <est_ldw>75380</est_ldw>
                </weights>
                <alternate>
                    <icao_code>LFBO</icao_code>
                    <iata_code>TLS</iata_code>
                    <faa_code/>
                    <icao_region>LF</icao_region>
                    <elevation>499</elevation>
                    <pos_lat>43.635000</pos_lat>
                    <pos_long>1.367778</pos_long>
                </alternate>
            </ofp>
        "#;
        let ofp = parse_simbrief_ofp(xml).expect("parses");
        assert_eq!(ofp.alternate.as_deref(), Some("LFBO"));
    }

    /// Reproduces the live bug from a GSG pilot 2026-05-03: the
    /// `/api/user/bids` response contained `"flight_number": 6431`
    /// (integer, no quotes), failing the entire bids parse with
    /// "invalid type: integer '6431', expected a string at line 1
    /// column 289". After the fix all id-ish String fields tolerate
    /// integers via de_str_or_int / de_opt_str_or_int.
    #[test]
    fn flight_parses_integer_flight_number_and_id() {
        let json = r#"{
            "id": 6431,
            "flight_number": 6431,
            "dpt_airport_id": 17,
            "arr_airport_id": "LFMN",
            "alt_airport_id": null,
            "route_code": 42,
            "callsign": 6431
        }"#;
        let f: Flight = serde_json::from_str(json).expect("parses");
        assert_eq!(f.id, "6431");
        assert_eq!(f.flight_number, "6431");
        assert_eq!(f.dpt_airport_id, "17");
        assert_eq!(f.arr_airport_id, "LFMN");
        assert_eq!(f.alt_airport_id, None);
        assert_eq!(f.route_code.as_deref(), Some("42"));
        assert_eq!(f.callsign.as_deref(), Some("6431"));
    }

    /// Strings still parse as strings (regression guard — the
    /// permissive deserializer must not break the canonical phpVMS
    /// shape where everything is a string).
    #[test]
    fn flight_still_parses_canonical_string_shape() {
        let json = r#"{
            "id": "VL12A",
            "flight_number": "VL12A",
            "dpt_airport_id": "EDDF",
            "arr_airport_id": "LEBL",
            "alt_airport_id": "LEMD",
            "route_code": "STAR1",
            "callsign": "DLH123"
        }"#;
        let f: Flight = serde_json::from_str(json).expect("parses");
        assert_eq!(f.id, "VL12A");
        assert_eq!(f.flight_number, "VL12A");
        assert_eq!(f.alt_airport_id.as_deref(), Some("LEMD"));
    }

    /// Bid wrapping a Flight — full path the live bug took.
    #[test]
    fn bid_with_integer_flight_number_parses() {
        let json = r#"{
            "id": 7,
            "user_id": 42,
            "flight_id": 6431,
            "flight": {
                "id": 6431,
                "flight_number": 6431,
                "dpt_airport_id": "LFMN",
                "arr_airport_id": "EDLE"
            }
        }"#;
        let b: Bid = serde_json::from_str(json).expect("parses");
        assert_eq!(b.id, 7);
        assert_eq!(b.flight_id, "6431");
        assert_eq!(b.flight.flight_number, "6431");
    }

    #[test]
    fn simbrief_alternate_falls_back_to_root_icao_when_no_wrapper() {
        // Defensive — if a future SimBrief variant flattens the
        // alternate to a sibling icao_code we still pick it up.
        let xml = r#"
            <ofp>
                <weights>
                    <est_zfw>71242</est_zfw>
                </weights>
                <icao_code>LFBO</icao_code>
            </ofp>
        "#;
        let ofp = parse_simbrief_ofp(xml).expect("parses");
        assert_eq!(ofp.alternate.as_deref(), Some("LFBO"));
    }

    /// v0.7.8: <params><request_id> ist die canonical changed-flag-
    /// Quelle fuer SimBrief-direct Refresh. Spec §3.
    #[test]
    fn simbrief_parser_extracts_request_id_from_params() {
        let xml = r#"
            <ofp>
                <params>
                    <request_id>172403072</request_id>
                    <static_id></static_id>
                    <time_generated>1778461205</time_generated>
                </params>
                <weights>
                    <est_zfw>71242</est_zfw>
                </weights>
            </ofp>
        "#;
        let ofp = parse_simbrief_ofp(xml).expect("parses");
        assert_eq!(ofp.request_id, "172403072");
    }

    /// v0.7.8: Wenn <request_id> fehlt, bekommen wir leeren String
    /// statt Parse-Fehler. (Should-not-happen-in-praxis, aber defensiv.)
    #[test]
    fn simbrief_parser_handles_missing_request_id_with_empty_string() {
        let xml = r#"
            <ofp>
                <params>
                    <time_generated>1778461205</time_generated>
                </params>
                <weights>
                    <est_zfw>71242</est_zfw>
                </weights>
            </ofp>
        "#;
        let ofp = parse_simbrief_ofp(xml).expect("parses");
        assert_eq!(ofp.request_id, "");
    }

    // ── v0.7.18 (B-011 / R2-3): PirepSummary Regression-Guards ───────
    //
    // get_user_pireps() ist der Datenpfad für den Orphan-Cleanup-Cluster
    // (B-011). Wenn nur ein einziger PIREP im Response-Array beim Parsen
    // failt, schlägt der ganze Aufruf fehl → Pilot sieht keine Orphans.
    // Diese Tests sichern die drei Shape-Varianten ab, die wir in der
    // Wild bei phpVMS-Installationen gesehen haben.

    /// flight_id als String — canonical phpVMS-Shape.
    #[test]
    fn pirep_summary_parses_string_flight_id() {
        let json = r#"{
            "id": "pirep_abc",
            "flight_id": "flightid_42"
        }"#;
        let p: PirepSummary = serde_json::from_str(json).expect("parses");
        assert_eq!(p.id, "pirep_abc");
        assert_eq!(p.flight_id.as_deref(), Some("flightid_42"));
    }

    /// flight_id als Integer — manche VAs liefern numeric IDs (R1-3
    /// QS-Fix: deserialize_with = "de_opt_str_or_int"). Ohne den
    /// Deserializer wäre der ganze Array-Parse aus get_user_pireps()
    /// gebrochen.
    #[test]
    fn pirep_summary_parses_integer_flight_id() {
        let json = r#"{
            "id": "pirep_abc",
            "flight_id": 4711
        }"#;
        let p: PirepSummary = serde_json::from_str(json).expect("parses");
        assert_eq!(p.flight_id.as_deref(), Some("4711"));
    }

    /// flight_id fehlend — Forward-/Backward-Compat für VAs ohne das
    /// Feld. Darf nicht failen, sondern Option::None liefern.
    #[test]
    fn pirep_summary_parses_missing_flight_id_as_none() {
        let json = r#"{
            "id": "pirep_abc"
        }"#;
        let p: PirepSummary = serde_json::from_str(json).expect("parses");
        assert_eq!(p.flight_id, None);
    }

    /// Voller IN_PROGRESS-Orphan wie ihn flight_list_orphans erwartet —
    /// state=0, alle B-011-Felder dabei. Stellt sicher dass das Frontend
    /// genug Kontext hat um den Cancel-Button anzuzeigen.
    #[test]
    fn pirep_summary_parses_in_progress_orphan_shape() {
        let json = r#"{
            "id": "pirep_orphan_1",
            "state": 0,
            "status": "ENR",
            "flight_id": "flight_xyz",
            "airline_id": 1,
            "flight_number": "GSG123",
            "dpt_airport_id": "EDDF",
            "arr_airport_id": "LEBL",
            "aircraft_icao": "B738",
            "aircraft_registration": "D-AGSG"
        }"#;
        let p: PirepSummary = serde_json::from_str(json).expect("parses");
        assert_eq!(p.state, Some(0));
        assert_eq!(p.status.as_deref(), Some("ENR"));
        assert_eq!(p.flight_id.as_deref(), Some("flight_xyz"));
        assert_eq!(p.flight_number.as_deref(), Some("GSG123"));
        assert_eq!(p.aircraft_icao.as_deref(), Some("B738"));
        assert_eq!(p.aircraft_registration.as_deref(), Some("D-AGSG"));
    }

    /// v0.16.18: flight_id wird gesendet wenn vorhanden, sonst komplett
    /// weggelassen (skip_serializing_if) — alte phpVMS-Installs sehen
    /// keinen neuen Key, neue haengen den PIREP an den geplanten Flug.
    #[test]
    fn prefile_body_flight_id_serialization() {
        let mut body = PrefileBody {
            airline_id: 1,
            aircraft_id: "170".into(),
            flight_number: "434".into(),
            dpt_airport_id: "EDDF".into(),
            arr_airport_id: "KORD".into(),
            source_name: "AeroACARS/test".into(),
            ..Default::default()
        };
        let js = serde_json::to_string(&body).unwrap();
        assert!(!js.contains("flight_id"), "None muss weggelassen werden: {js}");

        body.flight_id = Some("N43EeJppON5wr3Rm".into());
        let js = serde_json::to_string(&body).unwrap();
        assert!(js.contains("\"flight_id\":\"N43EeJppON5wr3Rm\""), "{js}");
    }
}
