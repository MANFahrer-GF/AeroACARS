//! METAR fetch + parse.
//!
//! Snapshots weather at the moment of takeoff and touchdown for the PIREP
//! landing analysis. See requirements spec §21–§22.
//!
//! Source: aviationweather.gov (NOAA), which exposes a free JSON API and
//! doesn't require an API key. Requests are rate-limited by the upstream;
//! we don't expose this through AeroACARS more than twice per flight
//! (departure + arrival), so we'll never come close to the limits.
//!
//! Status: Phase J.2 — production-ready for the standard "snapshot once
//! at takeoff, snapshot once at touchdown" use case. The full TAF /
//! forecast set is out of scope.

#![allow(dead_code)]

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Minimal subset of the aviationweather.gov METAR JSON we care about.
/// Field names match the upstream payload (`/api/data/metar`).
#[derive(Debug, Clone, Deserialize)]
struct ApiMetar {
    #[serde(default, rename = "icaoId")]
    icao_id: Option<String>,
    /// Raw observation text — what the pilot expects to read.
    #[serde(default, rename = "rawOb")]
    raw_ob: Option<String>,
    /// Observation time as Unix timestamp (seconds since epoch).
    #[serde(default, rename = "obsTime")]
    obs_time: Option<i64>,
    /// Wind direction in degrees true. Can be `"VRB"` for variable —
    /// upstream encodes that as a string in the same field, hence the
    /// untagged enum below.
    #[serde(default, rename = "wdir")]
    wdir: Option<WindDir>,
    /// Wind speed in knots.
    #[serde(default, rename = "wspd")]
    wspd: Option<f32>,
    /// Gust in knots, when reported.
    #[serde(default, rename = "wgst")]
    wgst: Option<f32>,
    /// Temperature in °C.
    #[serde(default, rename = "temp")]
    temp: Option<f32>,
    /// Dewpoint in °C.
    #[serde(default, rename = "dewp")]
    dewp: Option<f32>,
    /// Altimeter / QNH in hPa.
    #[serde(default, rename = "altim")]
    altim: Option<f32>,
    /// Visibility — "10+" for unlimited or a number in statute miles.
    /// Upstream uses both string and integer, so accept both.
    #[serde(default, rename = "visib")]
    visib: Option<Visibility>,
}

/// Wind-direction field can be either a numeric heading or the literal
/// string `"VRB"`. Treated as `None` when variable.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum WindDir {
    Numeric(f32),
    Variable(String),
}

/// Visibility is reported either as a number (statute miles) or as the
/// string `"10+"` for "10 sm or more".
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Visibility {
    Numeric(f32),
    Text(String),
}

/// Cleaned, simulator-friendly weather snapshot we hand back to the
/// rest of the app. All optional because some METARs omit fields
/// (auto stations, missing sensors) — the activity log + PIREP simply
/// skip what we don't have.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetarSnapshot {
    pub icao: String,
    pub raw: String,
    pub time: DateTime<Utc>,
    pub wind_direction_deg: Option<f32>,
    pub wind_speed_kt: Option<f32>,
    pub gust_kt: Option<f32>,
    /// Visibility in metres. We convert from statute miles (NOAA's
    /// native unit) so the rest of AeroACARS can stay metric.
    pub visibility_m: Option<u32>,
    pub temperature_c: Option<f32>,
    pub dewpoint_c: Option<f32>,
    pub qnh_hpa: Option<f32>,
}

#[derive(Debug, Error)]
pub enum MetarError {
    #[error("network error: {0}")]
    Network(String),
    #[error("upstream returned status {0}")]
    Status(u16),
    #[error("malformed METAR response: {0}")]
    Parse(String),
    #[error("no METAR available for {0}")]
    NotFound(String),
}

const STATUTE_MILE_M: f32 = 1609.344;

/// Fetch the latest METAR for an airport from aviationweather.gov.
/// Returns `MetarError::NotFound` if upstream has no current report
/// (stale stations, military fields). Times out after ~10 s — we
/// don't want a flaky weather server to hang the takeoff banner.
///
/// The aviationweather.gov endpoint is occasionally flaky (intermittent
/// 5xx, timeouts, dropped connections). To avoid showing the user a red
/// error banner for every transient hiccup we retry on those classes
/// of failures up to 2 extra times with a short backoff. NotFound and
/// Parse failures are *not* retried — those are deterministic answers.
pub async fn fetch_metar(icao: &str) -> Result<MetarSnapshot, MetarError> {
    let mut last_err = None;
    for attempt in 0..3 {
        match fetch_metar_once(icao).await {
            Ok(m) => return Ok(m),
            Err(e) => {
                let retryable = matches!(
                    &e,
                    MetarError::Network(_) | MetarError::Status(_)
                );
                last_err = Some(e);
                if !retryable || attempt == 2 {
                    break;
                }
                // 250 ms, 750 ms backoff — keeps total wait under 1 s
                // even if both retries fire, so the takeoff banner
                // doesn't visibly hang.
                let delay = std::time::Duration::from_millis(250 * (1 + 2 * attempt as u64));
                tokio::time::sleep(delay).await;
            }
        }
    }
    Err(last_err.expect("at least one attempt"))
}

async fn fetch_metar_once(icao: &str) -> Result<MetarSnapshot, MetarError> {
    let icao = icao.trim().to_uppercase();
    let url = format!(
        "https://aviationweather.gov/api/data/metar?ids={icao}&format=json&hours=2"
    );
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| MetarError::Network(e.to_string()))?;
    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| MetarError::Network(e.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(MetarError::Status(status.as_u16()));
    }
    // 204 No Content = NOAA has no METAR for this ICAO right now (small
    // / non-reporting fields like TUPJ in the British Virgin Islands).
    // The body is empty, so attempting `response.json()` would die with
    // a "malformed METAR response" parse error and the UI would render
    // it as a server fault. Treat it explicitly as NotFound instead.
    if status == reqwest::StatusCode::NO_CONTENT {
        return Err(MetarError::NotFound(icao.clone()));
    }
    let body = response
        .bytes()
        .await
        .map_err(|e| MetarError::Network(e.to_string()))?;
    // Defensive: some intermediaries (proxies, edge caches) may return
    // an empty 200 body even though we'd expect an array. Don't bother
    // parsing — same NotFound semantics.
    if body.iter().all(|b| b.is_ascii_whitespace()) {
        return Err(MetarError::NotFound(icao.clone()));
    }
    let list: Vec<ApiMetar> = serde_json::from_slice(&body)
        .map_err(|e| MetarError::Parse(e.to_string()))?;
    let raw = pick_latest(list).ok_or_else(|| MetarError::NotFound(icao.clone()))?;

    let time = raw
        .obs_time
        .and_then(|s| Utc.timestamp_opt(s, 0).single())
        .unwrap_or_else(Utc::now);

    let wind_direction_deg = match raw.wdir {
        Some(WindDir::Numeric(n)) => Some(n),
        _ => None,
    };
    let visibility_m = decode_visibility(raw.visib.as_ref(), raw.raw_ob.as_deref());

    Ok(MetarSnapshot {
        icao: raw.icao_id.unwrap_or(icao),
        raw: raw.raw_ob.unwrap_or_default(),
        time,
        wind_direction_deg,
        wind_speed_kt: raw.wspd,
        gust_kt: raw.wgst,
        visibility_m,
        temperature_c: raw.temp,
        dewpoint_c: raw.dewp,
        qnh_hpa: raw.altim,
    })
}

/// Picks the freshest observation from the upstream list. Pure helper,
/// extracted so we can regression-test the "always the most recent
/// report, not just whichever one NOAA happened to list first" invariant
/// without a network call.
///
/// `?hours=2` on the upstream request can return more than one report
/// for the same station (a routine hourly observation plus a SPECI
/// amendment, for example), and nothing in the API contract guarantees
/// the array is ordered newest-first. Blindly taking the first element
/// risked showing a stale observation as "the" current METAR for a
/// takeoff/touchdown snapshot. Entries with a missing `obsTime` sort
/// last (never preferred over a dated one) but still fall back to being
/// picked if that's all upstream sent.
fn pick_latest(list: Vec<ApiMetar>) -> Option<ApiMetar> {
    list.into_iter().max_by_key(|m| m.obs_time.unwrap_or(i64::MIN))
}

/// Decode the aviationweather.gov `visib` field (and the raw METAR
/// fallback) into metres. Pure helper, extracted so we can regression-
/// test CAVOK parsing without going through the network.
///
/// The API is inconsistent about how it surfaces CAVOK:
///   * sometimes `visib` is omitted entirely (None)
///   * sometimes it comes through as the literal string `"CAVOK"`
///   * sometimes as `"10+"` or just the number `10` (statute miles)
///
/// We string-match the raw METAR for `CAVOK` as the source of truth and
/// use it as a fallback whenever the structured field can't be decoded.
/// Returns `9999 m` (the canonical ICAO ≥10 km marker) for any CAVOK
/// case so the UI can render it as `≥ 10 km`.
fn decode_visibility(visib: Option<&Visibility>, raw_ob: Option<&str>) -> Option<u32> {
    let cavok = raw_ob.map(|s| s.contains("CAVOK")).unwrap_or(false);
    match visib {
        Some(Visibility::Numeric(n)) => {
            // 10+ sm reports often come through as exactly 10.0 — clamp
            // to the canonical ≥10 km marker so the UI shows the same
            // string regardless of which branch we came down.
            if *n >= 10.0 {
                Some(9999)
            } else {
                Some((*n * STATUTE_MILE_M) as u32)
            }
        }
        Some(Visibility::Text(t)) => {
            let trimmed = t.trim();
            if trimmed.starts_with("10") {
                Some(9999)
            } else if trimmed.eq_ignore_ascii_case("CAVOK") {
                Some(9999)
            } else if let Ok(n) = trimmed.parse::<f32>() {
                Some((n * STATUTE_MILE_M) as u32)
            } else if cavok {
                // Unknown text token (e.g. "P6SM", "M1/4SM") — but the
                // raw METAR says CAVOK, so we know visibility is fine.
                Some(9999)
            } else {
                None
            }
        }
        None if cavok => Some(9999),
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metar_at(obs_time: Option<i64>) -> ApiMetar {
        ApiMetar {
            icao_id: None,
            raw_ob: None,
            obs_time,
            wdir: None,
            wspd: None,
            wgst: None,
            temp: None,
            dewp: None,
            altim: None,
            visib: None,
        }
    }

    /// The exact bug: upstream lists the older report first. Taking
    /// `.next()` used to silently hand back a stale observation.
    #[test]
    fn picks_the_newest_observation_even_when_listed_first_is_older() {
        let list = vec![metar_at(Some(1_000)), metar_at(Some(2_000))];
        let picked = pick_latest(list).expect("non-empty list");
        assert_eq!(picked.obs_time, Some(2_000));
    }

    #[test]
    fn picks_the_newest_observation_when_listed_last() {
        let list = vec![metar_at(Some(2_000)), metar_at(Some(1_000))];
        let picked = pick_latest(list).expect("non-empty list");
        assert_eq!(picked.obs_time, Some(2_000));
    }

    /// A dated report is always preferred over one with no timestamp at
    /// all, regardless of list order.
    #[test]
    fn dated_report_beats_undated_report() {
        let list = vec![metar_at(None), metar_at(Some(500))];
        let picked = pick_latest(list).expect("non-empty list");
        assert_eq!(picked.obs_time, Some(500));
    }

    /// If nothing has a timestamp there's no ordering signal at all —
    /// still returns *something* rather than NotFound.
    #[test]
    fn falls_back_to_any_entry_when_none_have_a_timestamp() {
        let list = vec![metar_at(None), metar_at(None)];
        assert!(pick_latest(list).is_some());
    }

    #[test]
    fn empty_list_is_none() {
        assert!(pick_latest(vec![]).is_none());
    }

    /// Real EDDW METAR observed 2026-05-02: visibility column was
    /// rendering "—" in the UI because CAVOK wasn't being decoded.
    #[test]
    fn cavok_eddw_visibility_is_at_least_10km() {
        let raw_ob =
            "METAR EDDW 021520Z AUTO 23015KT CAVOK 25/09 Q1015 TEMPO 23015G25KT";
        // No structured visib field returned by upstream.
        let v = decode_visibility(None, Some(raw_ob));
        assert_eq!(v, Some(9999), "CAVOK without structured visib must map to ≥10 km");
    }

    /// Same EDDM CAVOK report — checks the same fallback path holds.
    #[test]
    fn cavok_eddm_visibility() {
        let raw_ob =
            "METAR EDDM 021520Z AUTO 09004KT 050V150 CAVOK 24/03 Q1019 NOSIG";
        let v = decode_visibility(None, Some(raw_ob));
        assert_eq!(v, Some(9999));
    }

    /// API sometimes echoes CAVOK as the literal text in the visib
    /// field — we must not let that fall through to None.
    #[test]
    fn cavok_as_text_field() {
        let v = decode_visibility(
            Some(&Visibility::Text("CAVOK".into())),
            Some("METAR LFPG 011200Z 24010KT CAVOK 18/05 Q1018"),
        );
        assert_eq!(v, Some(9999));
    }

    /// "10+" — at least 10 sm — gets the same ≥10 km treatment as CAVOK.
    #[test]
    fn ten_plus_sm_text_is_ge_10km() {
        let v = decode_visibility(
            Some(&Visibility::Text("10+".into())),
            Some("METAR KORD 011200Z 24010KT 10SM CLR 18/05 A2992"),
        );
        assert_eq!(v, Some(9999));
    }

    /// Numeric 10 sm exactly — also ≥10 km.
    #[test]
    fn numeric_10sm_is_ge_10km() {
        let v = decode_visibility(Some(&Visibility::Numeric(10.0)), Some("METAR KJFK ..."));
        assert_eq!(v, Some(9999));
    }

    /// Sub-10 sm value should still convert to metres normally.
    #[test]
    fn numeric_3sm_converts_to_metres() {
        let v = decode_visibility(Some(&Visibility::Numeric(3.0)), Some("METAR KJFK ... 3SM ..."));
        // 3 * 1609.344 = 4828.032 → 4828 as u32
        assert_eq!(v, Some(4828));
    }

    /// No info at all → None.
    #[test]
    fn missing_visibility_is_none() {
        let v = decode_visibility(None, Some("METAR KXYZ 011200Z AUTO 24010KT"));
        assert_eq!(v, None);
    }

    /// Unknown text + no CAVOK → None (don't fabricate values).
    #[test]
    fn unparseable_text_without_cavok_is_none() {
        let v = decode_visibility(
            Some(&Visibility::Text("P6SM".into())),
            Some("METAR KORD 011200Z 24010KT P6SM CLR 18/05 A2992"),
        );
        assert_eq!(v, None);
    }

    /// Unknown text + CAVOK in raw → fallback to 9999.
    #[test]
    fn unparseable_text_with_cavok_falls_back() {
        let v = decode_visibility(
            Some(&Visibility::Text("?weird?".into())),
            Some("METAR EDDF 011200Z 24010KT CAVOK 18/05 Q1018"),
        );
        assert_eq!(v, Some(9999));
    }
}
