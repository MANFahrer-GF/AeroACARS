//! Pilot-Client → aeroacars-live Navdata-Fetch.
//!
//! Holt pro Flugstart bis zu 3 Airports (dep/arr/alt) parallel vom VPS.
//! Lebt hier (statt im api-client crate) weil der Endpoint gegen
//! `live.kant.ovh` geht — same host wie provision.rs / log_upload.rs.
//!
//! Spec: `docs/spec/v0.8.0-vps-navdata-runway-awareness.md`. Wire-Format
//! ist 1:1 das was `GET /api/navdata/airport/<ICAO>` zurückgibt.
//!
//! Auth: Pilot-Token via `Authorization: Bearer <token>`. Spec
//! `§REST-API-Spec.Auth` verlangt token-authentifizierte Reads — kein
//! anonymer Pfad. Token kommt aus dem Pilot-MQTT-Provisioning
//! (`provision.rs::ProvisionResponse.password`) oder einer separaten
//! Recorder-Konfiguration. Bei `None` wird kein Auth-Header gesendet —
//! das ist nur für Tests und gegen lokale Mocks; gegen den echten VPS
//! liefert das 401.

use std::time::Duration;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Default-Host für den Navdata-Endpoint. Override via Test-Setup
/// oder Webapp-Admin-Setting (analog provision.rs).
pub const DEFAULT_NAVDATA_BASE: &str = "https://live.kant.ovh";

/// Per-call Timeout. Größer als provision (15s) weil die VPS-Side bei
/// kaltem DB-Cache evtl. erst eine SQLite-Page faulten muss.
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Single point on the earth's surface plus optional MSL elevation.
/// Wire-shape matches the VPS JSON exactly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavPoint {
    pub lat: f64,
    pub lon: f64,
    #[serde(default)]
    pub elev_ft: Option<i32>,
}

/// ILS metadata for one runway. Optional because non-precision runways
/// (e.g. visual-only strips) just have `ils: null` in the wire-payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavIls {
    pub freq_mhz: f64,
    pub course: f64,
    pub category: i32,
}

/// One runway end. We always carry the *landing-direction* end as
/// `threshold` and the opposite end as `end` — the wire format uses
/// the same convention. `magnetic_course` matches the painted runway
/// designator (172° → "17"); `true_course` is the great-circle bearing
/// from threshold to end and is what the cross-track math uses.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavRunway {
    pub designator: String,
    pub magnetic_course: f64,
    pub true_course: f64,
    pub length_ft: i32,
    #[serde(default)]
    pub width_ft: Option<i32>,
    #[serde(default)]
    pub surface: Option<String>,
    pub threshold: NavPoint,
    #[serde(rename = "end")]
    pub far_end: NavPoint,
    /// Distance from the painted runway start to the actual landing
    /// threshold. 0 means the landing threshold is at the runway start.
    /// Non-zero is rare but matters: e.g. OLBA 35 has 2690 ft (820 m)
    /// — touching down before that point is illegal.
    #[serde(default)]
    pub displaced_threshold_ft: i32,
    #[serde(default)]
    pub ils: Option<NavIls>,
    /// ILS glide-path angle in degrees. Defaults to 3.0 (ICAO standard)
    /// when the source doesn't carry an explicit value.
    #[serde(default = "default_glideslope_angle")]
    pub glideslope_angle: f64,
    /// Threshold Crossing Height in feet (= AGL at the threshold the
    /// pilot should cross with). Typical 49–55 ft for an ILS approach.
    #[serde(default = "default_tch_ft")]
    pub tch_ft: i32,
}

fn default_glideslope_angle() -> f64 {
    3.0
}
fn default_tch_ft() -> i32 {
    50
}

/// One airport with its runways. `cycle` and `valid_to` come from the
/// VPS so the pilot-client can log which AIRAC the bewertung lief gegen
/// — important for the audit-trail when Navigraph asks „where did your
/// numbers come from".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NavAirport {
    pub cycle: String,
    pub valid_to: String,
    pub icao: String,
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(default)]
    pub elevation_ft: Option<i32>,
    pub runways: Vec<NavRunway>,
}

/// Active-cycle metadata. Only used for `GET /api/navdata/cycle`, which
/// the Pilot-Client surface in the Activity-Log ("Navdata AIRAC 2604
/// geladen").
#[derive(Debug, Clone, Deserialize)]
pub struct NavCycle {
    pub cycle: String,
    pub valid_to: String,
    #[serde(default)]
    pub airport_count: Option<i64>,
    #[serde(default)]
    pub runway_count: Option<i64>,
}

/// Errors fetching navdata. Distinguished so the caller can log them
/// differently — `NotFound` is normal (small airport not in the cycle),
/// `Network` and `Server` are loud.
#[derive(Debug, thiserror::Error)]
pub enum NavdataError {
    #[error("airport {0} not in active cycle")]
    NotFound(String),
    #[error("Pilot-Token missing or invalid (HTTP 401/403)")]
    Unauthorized,
    #[error("VPS unreachable: {0}")]
    Network(String),
    #[error("VPS error {status}: {body}")]
    Server { status: u16, body: String },
    #[error("response not parseable: {0}")]
    BadResponse(String),
}

impl From<reqwest::Error> for NavdataError {
    fn from(err: reqwest::Error) -> Self {
        NavdataError::Network(err.to_string())
    }
}

pub(crate) fn build_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("build navdata reqwest client")
}

fn airport_url(base: &str, icao: &str) -> String {
    format!(
        "{}/api/navdata/airport/{}",
        base.trim_end_matches('/'),
        icao.trim().to_uppercase()
    )
}

/// Fetch one airport's navdata. Returns `NavdataError::NotFound` for
/// HTTP 404 (= small airport not in the cycle) so the caller can fall
/// back to OurAirports without treating it as a network failure.
/// HTTP 401/403 land in `NavdataError::Unauthorized` so the caller can
/// log a clear "Pilot-Token missing/invalid" hint.
///
/// `auth_token` is the Pilot-Bearer-Token from the MQTT-provisioning
/// response. Pass `None` only in tests against local mocks — the real
/// VPS rejects unauthenticated reads with 401.
pub async fn get_airport(
    icao: &str,
    base: Option<&str>,
    auth_token: Option<&str>,
) -> std::result::Result<NavAirport, NavdataError> {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    let url = airport_url(base, icao);
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;

    let mut req = client.get(&url);
    if let Some(token) = auth_token {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = req.send().await?;
    let status = response.status();

    if status.as_u16() == 404 {
        return Err(NavdataError::NotFound(icao.to_uppercase()));
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(NavdataError::Unauthorized);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(NavdataError::Server {
            status: status.as_u16(),
            body: body.chars().take(400).collect(),
        });
    }

    let body = response
        .text()
        .await
        .map_err(|e| NavdataError::BadResponse(format!("read body: {e}")))?;
    serde_json::from_str::<NavAirport>(&body)
        .map_err(|e| NavdataError::BadResponse(format!("parse NavAirport: {e}")))
}

fn ground_url(base: &str, icao: &str) -> String {
    format!(
        "{}/api/airports/{}/ground",
        base.trim_end_matches('/'),
        icao.trim().to_uppercase()
    )
}

/// Ein Flughafen im Bodendaten-Index (nur ICAO + Position).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroundIndexEntry {
    pub icao: String,
    pub lat: f64,
    pub lon: f64,
}

/// Welche Flughaefen haben ueberhaupt Bodendaten — und wo liegen sie?
///
/// v0.21.0-beta.2: Der erste Wurf lud die Taxi-Karte nur fuer Start- und
/// Zielflughafen eines LAUFENDEN Flugs. Steht der Pilot ohne Bid am Gate (oder
/// weicht auf einen dritten Platz aus), blieb die Karte leer — ausgerechnet
/// dann, wenn er sie braucht. Mit diesem Index findet der Client den Flughafen,
/// auf dem er tatsaechlich STEHT.
///
/// Die Liste ist winzig (ICAO + zwei Zahlen pro Flughafen).
pub async fn get_ground_index(
    base: Option<&str>,
    auth_token: Option<&str>,
) -> std::result::Result<Vec<GroundIndexEntry>, NavdataError> {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    let url = format!("{}/api/airports/ground/index", base.trim_end_matches('/'));
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;

    let mut req = client.get(&url);
    if let Some(token) = auth_token {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = req.send().await?;
    if !response.status().is_success() {
        return Err(NavdataError::Server {
            status: response.status().as_u16(),
            body: String::new(),
        });
    }

    #[derive(Deserialize)]
    struct Wrapper {
        airports: Vec<GroundIndexEntry>,
    }
    let w: Wrapper = response
        .json()
        .await
        .map_err(|e| NavdataError::BadResponse(format!("parse index: {e}")))?;
    Ok(w.airports)
}

/// Bodendaten eines Flughafens fuer die Taxi-Karte (GeoJSON, roh).
///
/// v0.21: Rollwege, Vorfeld-Rollmarkierungen, Bahnen, Haltepunkte und
/// Standplaetze — Quelle OpenStreetMap (ODbL), auf dem VPS gespiegelt.
///
/// Wird als **roher String** zurueckgegeben und nicht geparst: Rust muss die
/// Geometrie nicht verstehen, die Karte im Frontend zeichnet sie direkt. Ein
/// Parse hier waere reine Arbeit ohne Nutzen — und eine weitere Stelle, an der
/// sich das Format einschleichen koennte.
///
/// `etag` erspart den Neu-Download: der Aufrufer schickt den ETag der Fassung,
/// die er schon hat. Antwortet der Server mit 304, ist die lokale Kopie aktuell
/// (`Ok(None)`). Ein Flughafen ist zwar klein (EDDF = 71 kB gzip), aber ihn bei
/// jedem Flugstart erneut zu ziehen waere Verschwendung — und ohne Netz waere
/// die Karte dann weg.
pub async fn get_airport_ground(
    icao: &str,
    base: Option<&str>,
    auth_token: Option<&str>,
    known_etag: Option<&str>,
) -> std::result::Result<Option<AirportGround>, NavdataError> {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    let url = ground_url(base, icao);
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;

    let mut req = client.get(&url);
    if let Some(token) = auth_token {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(tag) = known_etag {
        req = req.header(reqwest::header::IF_NONE_MATCH, tag);
    }

    let response = req.send().await?;
    let status = response.status();

    // 304 = unsere lokale Kopie ist noch aktuell.
    if status.as_u16() == 304 {
        return Ok(None);
    }
    if status.as_u16() == 404 {
        return Err(NavdataError::NotFound(icao.to_uppercase()));
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(NavdataError::Unauthorized);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(NavdataError::Server {
            status: status.as_u16(),
            body: body.chars().take(400).collect(),
        });
    }

    let etag = response
        .headers()
        .get(reqwest::header::ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);

    let geojson = response
        .text()
        .await
        .map_err(|e| NavdataError::BadResponse(format!("read body: {e}")))?;

    // Nur die grobe Form pruefen — nicht die Geometrie. Wenn hier kein
    // GeoJSON ankommt, soll das auffallen, bevor die Karte es zu zeichnen
    // versucht.
    if !geojson.contains("\"FeatureCollection\"") {
        return Err(NavdataError::BadResponse(
            "keine FeatureCollection".to_string(),
        ));
    }

    Ok(Some(AirportGround {
        icao: icao.trim().to_uppercase(),
        geojson,
        etag,
    }))
}

/// Bodendaten eines Flughafens, wie sie der Client zwischenspeichert.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AirportGround {
    pub icao: String,
    /// Rohes GeoJSON — die Karte zeichnet es direkt.
    pub geojson: String,
    /// Fassung, die wir haben. Beim naechsten Mal mitschicken → 304 statt 71 kB.
    pub etag: Option<String>,
}

/// One VPS-managed aircraft-type-alias mapping (one Bid-ICAO to one
/// substring the sim might report). v0.8.0 Erweiterung — bisher waren
/// Aliases hardcoded in `runway::aircraft_aliases`; jetzt kann der
/// VA-Admin pro VPS-Setup zusätzliche Mappings pflegen ohne Pilot-
/// Client-Release.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AircraftAliasEntry {
    pub icao: String,
    pub aliases: Vec<AircraftAliasSingle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AircraftAliasSingle {
    pub alias: String,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct AircraftAliasesResponse {
    aliases: Vec<AircraftAliasEntry>,
}

/// Fetch the VPS-managed aircraft-type-aliases. Pilot-Client merged
/// das Ergebnis additive zur hardcoded Tabelle in `runway::aircraft_
/// aliases`. Bei VPS-Outage bleibt nur die hardcoded Liste aktiv.
pub async fn get_aircraft_aliases(
    base: Option<&str>,
    auth_token: Option<&str>,
) -> std::result::Result<Vec<AircraftAliasEntry>, NavdataError> {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    let url = format!("{}/api/aircraft-aliases", base.trim_end_matches('/'));
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;

    let mut req = client.get(&url);
    if let Some(token) = auth_token {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = req.send().await?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(NavdataError::Unauthorized);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(NavdataError::Server {
            status: status.as_u16(),
            body: body.chars().take(200).collect(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|e| NavdataError::BadResponse(format!("read body: {e}")))?;
    let parsed = serde_json::from_str::<AircraftAliasesResponse>(&body)
        .map_err(|e| NavdataError::BadResponse(format!("parse aliases: {e}")))?;
    Ok(parsed.aliases)
}

/// Fetch the currently-active cycle metadata. Used by the Activity-Log
/// to surface "Navdata AIRAC 2604 geladen". Failures are silent —
/// the cycle string is informational, not load-bearing.
pub async fn get_cycle(
    base: Option<&str>,
    auth_token: Option<&str>,
) -> std::result::Result<NavCycle, NavdataError> {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    let url = format!("{}/api/navdata/cycle", base.trim_end_matches('/'));
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;

    let mut req = client.get(&url);
    if let Some(token) = auth_token {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = req.send().await?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(NavdataError::Unauthorized);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(NavdataError::Server {
            status: status.as_u16(),
            body: body.chars().take(200).collect(),
        });
    }
    let body = response
        .text()
        .await
        .map_err(|e| NavdataError::BadResponse(format!("read body: {e}")))?;
    serde_json::from_str::<NavCycle>(&body)
        .map_err(|e| NavdataError::BadResponse(format!("parse NavCycle: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MS713-Anchor (OLBA 17). Exact threshold from Aerosoft DFD cycle
    /// 2604; cross-track math against this in the assessment module.
    /// This test only asserts the JSON wire-shape parses, not the math.
    const OLBA_FIXTURE: &str = r#"{
        "cycle": "2604",
        "valid_to": "2026-05-14",
        "icao": "OLBA",
        "name": "Rafic Hariri Intl",
        "latitude": 33.819050,
        "longitude": 35.490031,
        "elevation_ft": 85,
        "runways": [
            {
                "designator": "17",
                "magnetic_course": 172.0,
                "true_course": 176.94,
                "length_ft": 10663,
                "width_ft": 148,
                "surface": "ASP",
                "threshold": { "lat": 33.838364, "lon": 35.486978, "elev_ft": 85 },
                "end":       { "lat": 33.809288, "lon": 35.488861, "elev_ft": 36 },
                "displaced_threshold_ft": 0,
                "ils": { "freq_mhz": 109.5, "course": 172.0, "category": 1 },
                "glideslope_angle": 3.0,
                "tch_ft": 49
            },
            {
                "designator": "35",
                "magnetic_course": 352.0,
                "true_course": 356.94,
                "length_ft": 10663,
                "width_ft": 148,
                "surface": "ASP",
                "threshold": { "lat": 33.809288, "lon": 35.488861, "elev_ft": 36 },
                "end":       { "lat": 33.838364, "lon": 35.486978, "elev_ft": 85 },
                "displaced_threshold_ft": 2690,
                "ils": null,
                "glideslope_angle": 3.0,
                "tch_ft": 50
            }
        ]
    }"#;

    #[test]
    fn parses_olba_wire_payload() {
        let apt: NavAirport = serde_json::from_str(OLBA_FIXTURE).expect("parse");
        assert_eq!(apt.icao, "OLBA");
        assert_eq!(apt.cycle, "2604");
        assert_eq!(apt.runways.len(), 2);
        let rwy17 = &apt.runways[0];
        assert_eq!(rwy17.designator, "17");
        assert!((rwy17.threshold.lat - 33.838364).abs() < 1e-6);
        assert!((rwy17.threshold.lon - 35.486978).abs() < 1e-6);
        assert_eq!(rwy17.length_ft, 10663);
        assert_eq!(rwy17.displaced_threshold_ft, 0);
        assert_eq!(rwy17.tch_ft, 49);
        let ils = rwy17.ils.as_ref().expect("ILS present on RWY 17");
        assert!((ils.freq_mhz - 109.5).abs() < 1e-9);
        assert_eq!(ils.category, 1);

        let rwy35 = &apt.runways[1];
        assert_eq!(rwy35.displaced_threshold_ft, 2690);
        assert!(rwy35.ils.is_none());
    }

    #[test]
    fn defaults_glideslope_and_tch_when_missing() {
        let minimal = r#"{
            "cycle": "2604",
            "valid_to": "2026-05-14",
            "icao": "XYZ",
            "name": "Tiny Strip",
            "latitude": 0.0,
            "longitude": 0.0,
            "runways": [
                {
                    "designator": "09",
                    "magnetic_course": 90.0,
                    "true_course": 90.0,
                    "length_ft": 3000,
                    "threshold": { "lat": 0.0, "lon": 0.0 },
                    "end": { "lat": 0.0, "lon": 0.01 }
                }
            ]
        }"#;
        let apt: NavAirport = serde_json::from_str(minimal).expect("parse");
        let rwy = &apt.runways[0];
        assert_eq!(rwy.glideslope_angle, 3.0);
        assert_eq!(rwy.tch_ft, 50);
        assert_eq!(rwy.displaced_threshold_ft, 0);
        assert!(rwy.width_ft.is_none());
        assert!(rwy.surface.is_none());
    }

    #[test]
    fn airport_url_uppercases_and_trims() {
        assert_eq!(
            airport_url("https://live.kant.ovh", "olba"),
            "https://live.kant.ovh/api/navdata/airport/OLBA"
        );
        assert_eq!(
            airport_url("https://live.kant.ovh/", "  edddf  "),
            "https://live.kant.ovh/api/navdata/airport/EDDDF"
        );
    }
}
