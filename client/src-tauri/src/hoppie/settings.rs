//! Non-secret settings persistence for the Hoppie PDC/CPDLC feature
//! (v1.3.0, #Hoppie-PDC-CPDLC). Mirrors `lib.rs`'s `site_config_*`
//! helpers / `remote/mod.rs`'s `PersistedRemoteSettings` pattern: a
//! small JSON file in the app config dir. The logon code itself is a
//! credential and lives in `crates/secrets` instead (see `mod.rs`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "hoppie.json";

/// Records the facility we currently hold a CPDLC session with, so a run
/// that dies without logging off (crash, kill, power loss) can be cleaned
/// up on the next connect.
///
/// Deliberately its OWN file rather than a field in [`HoppieSettings`]:
/// the settings struct is round-tripped through the UI, which sends back
/// the whole object and would silently drop a field it doesn't know.
const SESSION_FILE: &str = "hoppie_session.json";

/// Default `to=` addressee for CPDLC/PDC requests when the pilot
/// hasn't chosen a specific station. The official docs
/// (`hoppie.nl/acars/system/tech.html`) explicitly name `"SERVER"` as
/// the placeholder for requests that aren't addressed to another ACARS
/// station.
pub const DEFAULT_STATION_ID: &str = "SERVER";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HoppieSettings {
    /// Master opt-in toggle. Default `false` — the feature is
    /// completely inert until the pilot switches it on: no poller, no
    /// requests to hoppie.nl, no notifications. Pilots who don't want
    /// PDC/CPDLC at all are entirely unaffected.
    #[serde(default)]
    pub enabled: bool,
    /// Overrides the callsign used as `from=`. `None`/empty falls back
    /// to the active flight's callsign (wired in Phase 2, once a
    /// command actually needs that context).
    #[serde(default)]
    pub callsign_override: Option<String>,
    #[serde(default = "default_station_id")]
    pub station_id: String,
    /// OS-native toast + platform sound on a new inbound message that
    /// needs attention, even when the app isn't focused / not on the
    /// CPDLC tab. See `mod.rs` docs.
    #[serde(default = "default_true")]
    pub notify_os: bool,
    /// Independent of `notify_os` — lets a pilot keep the visual toast
    /// but mute the sound (or vice versa, platform permitting).
    #[serde(default = "default_true")]
    pub notify_sound: bool,
}

fn default_station_id() -> String {
    DEFAULT_STATION_ID.to_string()
}

fn default_true() -> bool {
    true
}

impl Default for HoppieSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            callsign_override: None,
            station_id: default_station_id(),
            notify_os: true,
            notify_sound: true,
        }
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|p| p.join(SETTINGS_FILE))
}

/// Read the persisted settings, falling back to the defaults (opt-out)
/// if the file is unset / unreadable / malformed.
pub fn read_settings(app: &AppHandle) -> HoppieSettings {
    let Some(path) = settings_path(app) else {
        return HoppieSettings::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return HoppieSettings::default();
    };
    parse_settings(&text)
}

/// Persist the settings. Best-effort — a write failure is logged but
/// not fatal (the chosen values still apply to the running process,
/// they just won't survive a restart).
pub fn write_settings(app: &AppHandle, settings: &HoppieSettings) {
    let Some(path) = settings_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_vec_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!(error = %e, "hoppie: failed to persist settings");
            }
        }
        Err(e) => tracing::warn!(error = %e, "hoppie: failed to serialize settings"),
    }
}

/// Parse the on-disk JSON body, falling back to the defaults on
/// malformed/empty input. Pure (testable).
fn parse_settings(text: &str) -> HoppieSettings {
    serde_json::from_str(text).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_fully_opt_out() {
        let s = HoppieSettings::default();
        assert!(!s.enabled);
        assert!(s.notify_os);
        assert!(s.notify_sound);
        assert_eq!(s.station_id, "SERVER");
        assert_eq!(s.callsign_override, None);
    }

    #[test]
    fn settings_round_trip_through_json() {
        let s = HoppieSettings {
            enabled: true,
            callsign_override: Some("GSG123".into()),
            station_id: "EDDF".into(),
            notify_os: false,
            notify_sound: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(parse_settings(&json), s);
    }

    #[test]
    fn garbage_falls_back_to_defaults() {
        for garbage in ["", "not json", "[1,2]", "null"] {
            assert_eq!(
                parse_settings(garbage),
                HoppieSettings::default(),
                "garbage {garbage:?} must yield the defaults"
            );
        }
    }

    #[test]
    fn legacy_file_missing_new_fields_gets_sensible_defaults() {
        // A hand-written / future-legacy file carrying only `enabled`.
        let parsed = parse_settings(r#"{"enabled":true}"#);
        assert!(parsed.enabled);
        assert!(
            parsed.notify_os,
            "notify_os must default true on legacy files"
        );
        assert!(
            parsed.notify_sound,
            "notify_sound must default true on legacy files"
        );
        assert_eq!(parsed.station_id, "SERVER");
    }
}

// ----------------------------------------------------------------------
// Open-session marker
// ----------------------------------------------------------------------

fn session_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|p| p.join(SESSION_FILE))
}

/// A CPDLC session left open by a run that ended without logging off
/// (crash, kill, power loss) — the facility that still holds us AND the
/// callsign it holds us under, so a crash-recovery LOGOFF names the same
/// participant Hoppie's server actually has registered.
///
/// QS round 8 (07.09.2026, #pdc-session-model, external QS Finding 3):
/// this used to be a bare station-name `String` — the crash-recovery
/// cleanup in `hoppie_connect` sent its stale LOGOFF under THIS run's
/// freshly-resolved callsign, not the crashed run's. A pilot who changed
/// a callsign override, or switched to a different active flight,
/// between the crash and the next connect had the stale LOGOFF silently
/// address the WRONG participant — Hoppie's server would not recognize
/// it as belonging to the actually-open session, so the real stale
/// session stayed open server-side (still queueing messages, still
/// showing the aircraft connected) while our own marker was cleared
/// regardless, losing all further track of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSession {
    pub station: String,
    pub callsign: String,
}

/// The facility we last logged on to and have not logged off from, if
/// any — with the callsign that logon was made under. `None` means there
/// is nothing to clean up — and in that case a connect must NOT send a
/// stray LOGOFF: on the controller's side an unsolicited LOGOFF matches
/// no filter and shows up as an unread message from an aircraft they
/// have never spoken to.
///
/// A marker left over from BEFORE round 8 (a bare JSON string, no
/// callsign) fails to parse as [`OpenSession`] and is simply ignored —
/// the same "nothing to clean up" behavior as a missing/corrupt file.
/// The one-time cost is skipping a single stale-LOGOFF attempt across
/// the exact version boundary; the alternative (guessing a callsign for
/// an old marker) would be exactly the bug this closes.
pub fn open_session(app: &AppHandle) -> Option<OpenSession> {
    let path = session_path(app)?;
    let text = std::fs::read_to_string(path).ok()?;
    let session = serde_json::from_str::<OpenSession>(&text).ok()?;
    let station = session.station.trim();
    let callsign = session.callsign.trim();
    (!station.is_empty() && !callsign.is_empty()).then(|| OpenSession {
        station: station.to_string(),
        callsign: callsign.to_string(),
    })
}

/// Remember that a CPDLC session is open with `station`, under `callsign`.
///
/// QS round 9 (07.09.2026, #pdc-session-model, following external QS
/// round 8): writes atomically — temp file in the same directory, then
/// `rename` — matching the established pattern this project already
/// uses for exactly this kind of durability-sensitive marker
/// (`crates/secrets`, `crates/storage`, `navdata_cache.rs`). A plain
/// `fs::write` truncates the target file in place; a crash (power loss,
/// kill -9) landing mid-write — the EXACT scenario this marker exists to
/// recover from — could leave it half-written. `open_session`'s
/// `serde_json::from_str(..).ok()?` already turns a corrupt file into
/// "nothing to clean up" rather than a crash of its own, but that's a
/// silent loss of the one thing this file is for, precisely when a
/// crash makes it matter most. `rename` within one directory is atomic
/// on every platform this app ships for — the reader never observes a
/// partially-written file.
pub fn set_open_session(app: &AppHandle, callsign: &str, station: &str) {
    let Some(path) = session_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let session = OpenSession {
        station: station.to_string(),
        callsign: callsign.to_string(),
    };
    match serde_json::to_vec(&session) {
        Ok(json) => {
            let tmp = path.with_extension("json.tmp");
            if let Err(e) = std::fs::write(&tmp, json).and_then(|()| std::fs::rename(&tmp, &path)) {
                tracing::warn!(error = %e, "hoppie: could not record open session");
            }
        }
        Err(e) => tracing::warn!(error = %e, "hoppie: could not serialize open session"),
    }
}

/// Forget the open session — after a successful LOGOFF, or once a stale
/// one has been cleaned up.
pub fn clear_open_session(app: &AppHandle) {
    let Some(path) = session_path(app) else {
        return;
    };
    if path.exists() {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(error = %e, "hoppie: could not clear open session");
        }
    }
}

#[cfg(test)]
mod session_tests {
    use super::OpenSession;

    /// The marker is `{station, callsign}`; these guard the shape the
    /// reader expects, without needing a Tauri AppHandle (the full
    /// `open_session`/`set_open_session` functions need one and aren't
    /// unit-tested here — same limitation the pre-round-8 tests had).
    #[test]
    fn round_trips_with_both_station_and_callsign() {
        let session = OpenSession {
            station: "EDGG".to_string(),
            callsign: "DLH123".to_string(),
        };
        let encoded = serde_json::to_string(&session).unwrap();
        let decoded: OpenSession = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded.station, "EDGG");
        assert_eq!(decoded.callsign, "DLH123");
    }

    #[test]
    fn blank_station_or_callsign_reads_as_blank_through_the_round_trip() {
        // The actual "not a valid session" gate lives in `open_session`
        // itself (needs an AppHandle) — this only guards that a blank
        // value survives the JSON round trip as blank, not silently
        // dropped or defaulted to something non-empty.
        for (station, callsign) in [("", "DLH123"), ("EDGG", ""), ("   ", "DLH123")] {
            let session = OpenSession {
                station: station.to_string(),
                callsign: callsign.to_string(),
            };
            let encoded = serde_json::to_string(&session).unwrap();
            let decoded: OpenSession = serde_json::from_str(&encoded).unwrap();
            assert!(decoded.station.trim().is_empty() || decoded.callsign.trim().is_empty());
        }
    }

    #[test]
    fn a_pre_round_8_plain_string_marker_fails_to_parse_rather_than_being_misread() {
        // QS round 8 (07.09.2026, #pdc-session-model, external QS
        // Finding 3): the marker used to be a bare JSON string with no
        // callsign at all. A leftover file from before this change must
        // be ignored outright (`open_session` treats a parse failure the
        // same as a missing file — nothing to clean up), never
        // misinterpreted as a session with a guessed or empty callsign,
        // which would silently reintroduce the exact bug this closed.
        let old_format = serde_json::to_string("EDGG").unwrap();
        assert!(serde_json::from_str::<OpenSession>(&old_format).is_err());
    }
}
