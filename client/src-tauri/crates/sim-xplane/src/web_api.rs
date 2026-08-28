//! X-Plane 12.1+ Web API client — for string DataRefs.
//!
//! The UDP RREF protocol that the rest of this crate uses can only
//! read scalar `float32` DataRefs. Aircraft identity is stored in
//! string DataRefs (`sim/aircraft/view/acf_*`) which RREF cannot
//! handle. X-Plane 12.1 (mid-2024) introduced a built-in REST API
//! on `http://localhost:8086/api/v1/` that exposes EVERY DataRef,
//! including strings — we use it just for those few fields.
//!
//! ## Caveats
//!
//! * Only available in X-Plane **12.1 or newer**. X-Plane 11 and
//!   pre-12.1 will return connection errors → we silently fall back
//!   to leaving aircraft fields as `None` (same as today).
//! * The pilot has to enable it in **X-Plane → Settings → Network →
//!   Web Server / Web API** (off by default). When disabled the
//!   server isn't listening; same connection-refused fallback
//!   applies.
//!
//! ## Why this isn't in the UDP listener thread
//!
//! Aircraft identity rarely changes mid-flight. A 30 s polling
//! cadence is plenty — much sparser than the 50 Hz UDP stream.
//! Putting it on a separate thread keeps the hot UDP loop free of
//! HTTP latency / DNS / TLS overhead.

use std::collections::HashMap;
use std::time::Duration;

use serde::Deserialize;

/// Subset of `acf_*` DataRefs we care about. Each shows up in the
/// PIREP-detail / Activity-Log so the pilot can confirm we identified
/// the right aircraft.
const AIRCRAFT_DATAREFS: &[&str] = &[
    "sim/aircraft/view/acf_descrip",
    "sim/aircraft/view/acf_ICAO",
    "sim/aircraft/view/acf_tailnum",
    "sim/aircraft/view/acf_author",
    "sim/aircraft/view/acf_studio",
    "sim/aircraft/view/acf_relative_path",
];

/// Snapshot of what the Web API has reported so far. Empty
/// (`Default`) until the first successful poll.
#[derive(Debug, Clone, Default)]
pub struct AircraftInfo {
    pub descrip: Option<String>,
    pub icao: Option<String>,
    pub tailnum: Option<String>,
    pub author: Option<String>,
    pub studio: Option<String>,
    pub relative_path: Option<String>,
}

impl AircraftInfo {
    /// True when the Web API actually returned at least one
    /// non-empty field. Used to decide whether to log "found XYZ"
    /// vs. silently keep waiting.
    pub fn has_any(&self) -> bool {
        [
            &self.descrip,
            &self.icao,
            &self.tailnum,
            &self.author,
            &self.studio,
            &self.relative_path,
        ]
        .iter()
        .any(|v| v.as_ref().is_some_and(|s| !s.is_empty()))
    }
}

/// Cached `dref name → numeric id` mapping. The X-Plane Web API
/// returns numeric IDs from a discovery endpoint; subsequent value
/// reads use those IDs. Discovery is the slow part — caching once
/// per process is the obvious win.
#[derive(Debug, Default)]
pub struct DrefIdCache {
    ids: HashMap<&'static str, i64>,
}

#[derive(Debug, thiserror::Error)]
pub enum WebApiError {
    #[error("http: {0}")]
    Http(String),
    #[error("json: {0}")]
    Json(String),
    #[error("dataref not found in discovery response: {0}")]
    DatarefNotFound(&'static str),
}

impl From<ureq::Error> for WebApiError {
    fn from(e: ureq::Error) -> Self {
        WebApiError::Http(e.to_string())
    }
}

impl From<std::io::Error> for WebApiError {
    fn from(e: std::io::Error) -> Self {
        WebApiError::Http(e.to_string())
    }
}

impl From<serde_json::Error> for WebApiError {
    fn from(e: serde_json::Error) -> Self {
        WebApiError::Json(e.to_string())
    }
}

/// Synchronous client for the X-Plane Web API. Cheap to construct;
/// reuses one ureq agent so subsequent GETs benefit from connection
/// pooling.
pub struct WebApiClient {
    base_url: String,
    agent: ureq::Agent,
}

impl Default for WebApiClient {
    fn default() -> Self {
        Self::new()
    }
}

impl WebApiClient {
    pub fn new() -> Self {
        // 2 s timeouts on connect AND read — Web API is loopback,
        // anything longer means X-Plane is busy with a frame and a
        // late answer is worthless anyway. We just retry next tick.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(2))
            .timeout_read(Duration::from_secs(2))
            .build();
        Self {
            base_url: "http://127.0.0.1:8086".into(),
            agent,
        }
    }

    /// Look up the numeric id for one DataRef name. Cached.
    fn discover_id(&self, cache: &mut DrefIdCache, name: &'static str) -> Result<i64, WebApiError> {
        if let Some(id) = cache.ids.get(name).copied() {
            return Ok(id);
        }
        let url = format!("{}/api/v1/datarefs?filter[name]={}", self.base_url, name,);
        let body: DiscoveryResponse = self
            .agent
            .get(&url)
            .call()
            .map_err(WebApiError::from)?
            .into_json()?;
        let id = body
            .data
            .into_iter()
            .find(|d| d.name == name)
            .map(|d| d.id)
            .ok_or(WebApiError::DatarefNotFound(name))?;
        cache.ids.insert(name, id);
        Ok(id)
    }

    /// Read a string DataRef value. Returns the trimmed content
    /// (X-Plane null-pads byte arrays to a fixed length).
    fn read_string(&self, id: i64) -> Result<Option<String>, WebApiError> {
        let url = format!("{}/api/v1/datarefs/{}/value", self.base_url, id);
        let body: ValueResponse = self
            .agent
            .get(&url)
            .call()
            .map_err(WebApiError::from)?
            .into_json()?;
        Ok(body.data.into_string())
    }

    /// One full pass: read every aircraft DataRef and return what we
    /// got. Best-effort per field — a single field's failure doesn't
    /// poison the whole snapshot.
    pub fn fetch_aircraft_info(
        &self,
        cache: &mut DrefIdCache,
    ) -> Result<AircraftInfo, WebApiError> {
        // Fetch (or discover) IDs for every aircraft DataRef. If
        // discovery itself fails for ALL of them, propagate the
        // error so the caller can decide to back off (Web API is
        // probably unreachable). Per-field read failures are
        // swallowed silently.
        let mut info = AircraftInfo::default();
        let mut any_succeeded = false;
        let mut last_err: Option<WebApiError> = None;
        for name in AIRCRAFT_DATAREFS {
            let id = match self.discover_id(cache, name) {
                Ok(id) => id,
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            };
            let value = match self.read_string(id) {
                Ok(v) => {
                    any_succeeded = true;
                    v
                }
                Err(e) => {
                    last_err = Some(e);
                    None
                }
            };
            match *name {
                "sim/aircraft/view/acf_descrip" => info.descrip = value,
                "sim/aircraft/view/acf_ICAO" => info.icao = value,
                "sim/aircraft/view/acf_tailnum" => info.tailnum = value,
                "sim/aircraft/view/acf_author" => info.author = value,
                "sim/aircraft/view/acf_studio" => info.studio = value,
                "sim/aircraft/view/acf_relative_path" => info.relative_path = value,
                _ => {}
            }
        }
        if !any_succeeded {
            return Err(last_err
                .unwrap_or_else(|| WebApiError::Http("no aircraft datarefs reachable".into())));
        }
        Ok(info)
    }
}

// ---- Wire types ----

#[derive(Debug, Deserialize)]
struct DiscoveryResponse {
    data: Vec<DiscoveryEntry>,
}

#[derive(Debug, Deserialize)]
struct DiscoveryEntry {
    id: i64,
    name: String,
}

#[derive(Debug, Deserialize)]
struct ValueResponse {
    data: ValueData,
}

/// X-Plane returns byte-array DataRefs as **base64-encoded strings**
/// in the `data` field (verified against X-Plane 12.1.4):
///
/// ```json
/// {"data":"QTEzOQAAAAAAAAAAAAAAAAAAAAAAAAA="}
/// ```
///
/// `QTEzOQ==` decodes to `"A139"` (the ICAO type), padded with `\0`
/// up to the byte-array's declared fixed length. We base64-decode,
/// trim trailing nulls, and lossy-UTF-8 the result.
///
/// We still accept a raw byte array (`[65, 49, 51, 57, ...]`) as a
/// fallback because the format isn't formally documented and some
/// builds may differ — the untagged enum tries variants in order.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ValueData {
    Str(String),
    Bytes(Vec<u8>),
}

impl ValueData {
    fn into_string(self) -> Option<String> {
        use base64::Engine as _;
        let bytes = match self {
            ValueData::Str(s) => {
                // Try base64 first (the actual format X-Plane 12.1+
                // uses). If that fails, fall back to treating the
                // string as already-decoded text.
                match base64::engine::general_purpose::STANDARD.decode(s.trim()) {
                    Ok(bytes) => bytes,
                    Err(_) => s.into_bytes(),
                }
            }
            ValueData::Bytes(bytes) => bytes,
        };
        // Trim trailing nulls — X-Plane pads byte-array DataRefs to
        // their declared fixed length with `\0`. Then lossy-decode
        // because we want the printable prefix even if the padding
        // bleeds non-UTF8 bytes.
        let end = bytes
            .iter()
            .rposition(|&b| b != 0)
            .map(|i| i + 1)
            .unwrap_or(0);
        let s = String::from_utf8_lossy(&bytes[..end]).into_owned();
        let trimmed = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_data_base64_real_world() {
        // Captured live from X-Plane 12.1.4 on 2026-05-03 for an
        // AgustaWestland AW139 helicopter — value of acf_ICAO.
        let v: ValueResponse = serde_json::from_str(
            r#"{"data":"QTEzOQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=="}"#,
        )
        .unwrap();
        assert_eq!(v.data.into_string(), Some("A139".to_string()));
    }

    #[test]
    fn value_data_base64_no_padding() {
        // "B738" base64-encoded = "QjczOA=="
        let v: ValueResponse = serde_json::from_str(r#"{"data":"QjczOA=="}"#).unwrap();
        assert_eq!(v.data.into_string(), Some("B738".to_string()));
    }

    #[test]
    fn value_data_raw_bytes_fallback() {
        // Legacy / hypothetical raw byte-array form — parser should
        // still cope so we don't break if X-Plane changes the wire
        // format back.
        let v: ValueResponse = serde_json::from_str(r#"{"data":[66,55,51,56,0,0,0]}"#).unwrap();
        assert_eq!(v.data.into_string(), Some("B738".to_string()));
    }

    #[test]
    fn value_data_plain_string_fallback() {
        // If a future X-Plane build returns plain (non-base64) text
        // for some DataRef we still accept it. "Hello" is not valid
        // base64, so the parser falls through to byte-cast.
        let v: ValueResponse = serde_json::from_str(r#"{"data":"Hello"}"#).unwrap();
        assert_eq!(v.data.into_string(), Some("Hello".to_string()));
    }

    #[test]
    fn value_data_empty_string() {
        let v: ValueResponse = serde_json::from_str(r#"{"data":""}"#).unwrap();
        assert_eq!(v.data.into_string(), None);
    }

    #[test]
    fn value_data_only_padding() {
        let v: ValueResponse = serde_json::from_str(r#"{"data":[0,0,0]}"#).unwrap();
        assert_eq!(v.data.into_string(), None);
    }

    #[test]
    fn discovery_response_parses() {
        let r: DiscoveryResponse =
            serde_json::from_str(r#"{"data":[{"id":12345,"name":"sim/aircraft/view/acf_ICAO"}]}"#)
                .unwrap();
        assert_eq!(r.data.len(), 1);
        assert_eq!(r.data[0].id, 12345);
    }
}
