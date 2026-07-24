//! Non-secret settings persistence for the Hoppie PDC/CPDLC feature
//! (v1.3.0, #Hoppie-PDC-CPDLC). Mirrors `lib.rs`'s `site_config_*`
//! helpers / `remote/mod.rs`'s `PersistedRemoteSettings` pattern: a
//! small JSON file in the app config dir. The logon code itself is a
//! credential and lives in `crates/secrets` instead (see `mod.rs`).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "hoppie.json";

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
    /// Simulation mode — `hoppie/mod.rs`'s `HoppieHttp` never touches
    /// the real network when this is on; it fabricates canned
    /// responses instead (ping always succeeds, a PDC request gets a
    /// synthetic `[SIMULATION]`-tagged reply on the next poll). Lets a
    /// pilot exercise the whole connect/PDC/thread UI without an
    /// account on the real Hoppie network or without risking traffic
    /// on it. Default `false` — real network — once a pilot HAS
    /// enabled the feature for real use.
    #[serde(default)]
    pub mock_mode: bool,
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
            mock_mode: false,
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
        assert!(!s.mock_mode);
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
            mock_mode: true,
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
        assert!(parsed.notify_os, "notify_os must default true on legacy files");
        assert!(parsed.notify_sound, "notify_sound must default true on legacy files");
        assert_eq!(parsed.station_id, "SERVER");
    }
}
