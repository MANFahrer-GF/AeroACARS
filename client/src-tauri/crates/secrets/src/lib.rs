//! File-based secrets storage for AeroACARS (phpVMS API key, MQTT creds).
//!
//! v0.5.15: replaces the OS-keyring backend (Apple Keychain / Windows
//! Credential Manager) with a single JSON file in the app's data dir.
//!
//! ## Why we left the keyring behind
//!
//! Mac users hit a wall of "AeroACARS möchte auf den Schlüsselbund
//! zugreifen" prompts on every update — the OS treats each unsigned
//! ad-hoc-signed app build as a different binary, so "Always allow"
//! never sticks across versions. Once we added 5 MQTT keychain entries
//! in v0.5.11, this exploded to 6+ prompts per update; the v0.5.13
//! multi-init bug pushed it past 20 prompts.
//!
//! ## What we do instead
//!
//! Single JSON file at `<app_data_dir>/secrets.json` — same API
//! surface (`store_api_key` / `load_api_key` / `delete_api_key`) so
//! the call sites in `src/lib.rs` don't change. Permissions: 0600 on
//! Unix (user-only read/write), default-user-only on Windows (the
//! `%APPDATA%` path is already restricted to the current user).
//!
//! ## Security note
//!
//! For VA-pilot-tools storing phpVMS-API-key and MQTT broker creds
//! this is on par with what smartCARS / vmsACARS / FsAcars do.
//! Apple-Keychain-grade per-app-ACL would only matter on a shared
//! Mac with another *user-level malicious* app — by then they'd
//! likely have process-injection access to the running AeroACARS
//! anyway, which trivially bypasses Keychain ACLs too.
//!
//! ## Migration
//!
//! Callers should run `migrate_from_keyring(accounts)` once at
//! startup, after `init`. It reads each old keyring entry, writes
//! it to the JSON file, then deletes the keyring entry. Pilots
//! see their last batch of Keychain prompts on the migration run,
//! then never again.

#![allow(dead_code)]

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const FILE_NAME: &str = "secrets.json";

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("storage not initialized — call secrets::init() first")]
    NotInitialized,
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Default, Serialize, Deserialize, Clone)]
struct Store {
    /// Account-name → secret. Account-names are the constants used by
    /// the call sites (e.g. "primary" for the phpVMS API key,
    /// "mqtt-username", "mqtt-password", etc.). Stored as a flat map
    /// so adding new credential types doesn't change the file shape.
    #[serde(default)]
    entries: HashMap<String, String>,
}

struct StorageState {
    path: PathBuf,
    cache: Mutex<Option<Store>>,
}

static STATE: OnceLock<StorageState> = OnceLock::new();

/// Initialize file-based storage. Should be called exactly once at
/// app startup with the resolved app-data-dir path. Subsequent calls
/// are no-ops (so re-init after a hot-reload doesn't error).
pub fn init(app_data_dir: &Path) -> Result<(), SecretError> {
    if STATE.get().is_some() {
        return Ok(());
    }
    std::fs::create_dir_all(app_data_dir)?;
    let path = app_data_dir.join(FILE_NAME);
    let state = StorageState {
        path,
        cache: Mutex::new(None),
    };
    let _ = STATE.set(state);
    tracing::info!(
        path = %STATE.get().expect("just-set").path.display(),
        "secrets file storage initialized"
    );
    Ok(())
}

fn state() -> Result<&'static StorageState, SecretError> {
    STATE.get().ok_or(SecretError::NotInitialized)
}

fn read_store() -> Result<Store, SecretError> {
    let s = state()?;
    let mut cache = s.cache.lock().expect("secrets cache mutex");
    if let Some(cached) = cache.as_ref() {
        return Ok(cached.clone());
    }
    let store = if s.path.exists() {
        let bytes = std::fs::read(&s.path)?;
        if bytes.is_empty() {
            Store::default()
        } else {
            serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                tracing::warn!(error = %e, "secrets.json malformed — starting fresh");
                Store::default()
            })
        }
    } else {
        Store::default()
    };
    *cache = Some(store.clone());
    Ok(store)
}

fn write_store(store: &Store) -> Result<(), SecretError> {
    let s = state()?;
    let bytes = serde_json::to_vec_pretty(store)?;
    // Atomic write: temp file in same dir, then rename.
    let tmp = s.path.with_extension("json.tmp");
    std::fs::write(&tmp, bytes)?;
    apply_owner_only_permissions(&tmp)?;
    std::fs::rename(&tmp, &s.path)?;
    apply_owner_only_permissions(&s.path)?;
    *s.cache.lock().expect("secrets cache mutex") = Some(store.clone());
    Ok(())
}

#[cfg(unix)]
fn apply_owner_only_permissions(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = std::fs::metadata(path)?;
    let mut perms = metadata.permissions();
    perms.set_mode(0o600); // rw for owner only
    std::fs::set_permissions(path, perms)
}

#[cfg(windows)]
fn apply_owner_only_permissions(_path: &Path) -> std::io::Result<()> {
    // Windows: files in %APPDATA%\<vendor>\<app>\ are already
    // user-private by default thanks to the parent ACL inheritance.
    // No extra work needed for our use case.
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn apply_owner_only_permissions(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

pub fn store_api_key(account: &str, value: &str) -> Result<(), SecretError> {
    let mut store = read_store()?;
    store.entries.insert(account.to_string(), value.to_string());
    write_store(&store)?;
    tracing::debug!(account, "stored credential to file");
    Ok(())
}

pub fn load_api_key(account: &str) -> Result<Option<String>, SecretError> {
    let store = read_store()?;
    Ok(store.entries.get(account).cloned())
}

pub fn delete_api_key(account: &str) -> Result<(), SecretError> {
    let mut store = read_store()?;
    if store.entries.remove(account).is_some() {
        write_store(&store)?;
        tracing::debug!(account, "deleted credential from file");
    }
    Ok(())
}

// v0.7.13: `migrate_from_keyring(accounts)` entfernt — die einmalige
// v0.5.15-Migration vom OS-Keyring zur JSON-File-Storage ist 30+ Releases
// her. Wer noch nicht v0.5.15+ gestartet hat, wuerde sich ohnehin frisch
// einloggen muessen (Keyring-Eintrag laeuft seit Monaten ungenutzt).
// Audit Q4-2026-05 (CC1).

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;
    static INIT: Once = Once::new();

    fn setup() {
        INIT.call_once(|| {
            let dir = std::env::temp_dir().join("aeroacars-secrets-test");
            let _ = std::fs::remove_dir_all(&dir);
            init(&dir).unwrap();
        });
    }

    #[test]
    fn store_load_delete_roundtrip() {
        setup();
        store_api_key("test-1", "secret-value").unwrap();
        assert_eq!(
            load_api_key("test-1").unwrap().as_deref(),
            Some("secret-value")
        );
        delete_api_key("test-1").unwrap();
        assert!(load_api_key("test-1").unwrap().is_none());
    }

    #[test]
    fn missing_returns_none() {
        setup();
        assert!(load_api_key("does-not-exist").unwrap().is_none());
    }
}
