//! AeroACARS X-Plane Premium Plugin auto-install (v0.5.0+).
//!
//! Three Tauri commands exposed to the UI:
//!
//!   * `xplane_detect_install_path()` — best-effort guess where the
//!     pilot's X-Plane install lives (Win registry / common Mac paths
//!     / common Linux paths). Returns `None` when nothing plausible
//!     is found and the UI falls back to a folder-picker.
//!
//!   * `xplane_install_plugin(install_dir)` — downloads the matching
//!     `AeroACARS-XPlane-Plugin-vX.Y.Z.zip` from this release and
//!     extracts it to `<install_dir>/Resources/plugins/AeroACARS/`.
//!     Idempotent — overwrites a previous install in place.
//!
//!   * `xplane_uninstall_plugin(install_dir)` — removes the plugin
//!     folder. Available in case the pilot wants a clean uninstall.
//!
//! ## Safety
//!
//! The install command does the bare minimum that a successful
//! install requires:
//!   1. Validates the target is an X-Plane install (presence of
//!      `Resources/plugins/`).
//!   2. Creates the AeroACARS subfolder.
//!   3. Streams zip entries directly to disk via the `zip` crate's
//!      `read::ZipFile` reader — never holds the whole archive in
//!      memory.
//!   4. Refuses paths containing `..` (zip-slip mitigation).
//!   5. (AP7, ADR-0004 §8) Prueft das Paket gegen die SHA-256-Pruefsumme,
//!      die die Release-Pipeline beim Bauen einbettet
//!      (`AEROACARS_XPLANE_PLUGIN_SHA256`) — VOR dem Loeschen der alten
//!      Installation. Stimmt sie nicht, bleibt alles, wie es war.
//!   6. macOS: entfernt nach dem Entpacken `com.apple.quarantine`.
//!
//! The download is a one-shot reqwest GET with a 60 s timeout. The
//! caller (UI) shows a progress indicator; we don't surface byte
//! progress because the zip is small (~250 KB across all three .xpl
//! files).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

/// SHA-256 des Plugin-Pakets dieser Version, von der Release-Pipeline beim
/// Bauen gesetzt (baut das Plugin VOR der App). Fehlt sie (lokaler
/// Entwicklungsbuild), wird mit Warnung ohne Pruefung installiert.
const PLUGIN_SHA256: Option<&str> = option_env!("AEROACARS_XPLANE_PLUGIN_SHA256");

/// GitHub release asset URL template. We resolve the running app's
/// version (set in `Cargo.toml [workspace.package] version`) at
/// runtime via `env!("CARGO_PKG_VERSION")` and substitute it in.
/// Tag pattern matches `release.yml`'s `${{ github.ref_name }}`.
const PLUGIN_ZIP_URL_TEMPLATE: &str =
    "https://github.com/MANFahrer-GF/AeroACARS/releases/download/v{VERSION}/AeroACARS-XPlane-Plugin-v{VERSION}.zip";

/// HTTP timeout for the plugin download. The zip is small (~250 KB)
/// but the pilot might be on a slow connection — 60 s is generous.
const DOWNLOAD_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Serialize)]
pub struct PluginInstallResult {
    pub installed_at: String,
    pub bytes_written: u64,
    pub files_written: u32,
    /// Paket gegen die eingebettete SHA-256 geprueft? `false` nur in
    /// Entwicklungsbuilds ohne Pruefsumme — die Oberflaeche zeigt es an.
    pub geprueft: bool,
}

/// Best-effort detection of the X-Plane root directory.
///
/// We check, in order:
///   * Windows: `HKCU\Software\Laminar Research\X-Plane 12\Path`
///     and the X-Plane 11 equivalent (X-Plane writes these on first
///     run since version 11.10).
///   * macOS: `/Applications/X-Plane 12/`, then `~/X-Plane 12/`,
///     then the same paths for X-Plane 11.
///   * Linux: `~/X-Plane 12/`, `~/X-Plane 11/`, `/opt/X-Plane 12/`.
///
/// Returns `None` if nothing is found — the UI then offers a folder-
/// picker so the pilot can point us at their install manually.
pub fn detect_install_path() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Some(p) = detect_windows() {
            return Some(p);
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(p) = detect_macos() {
            return Some(p);
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(p) = detect_linux() {
            return Some(p);
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn detect_windows() -> Option<PathBuf> {
    // X-Plane writes its install path to the per-user registry on
    // each launch. We don't pull in the `winreg` crate just for this
    // — `reg.exe query` runs in a sub-second and has no compile-time
    // cost.
    //
    // CREATE_NO_WINDOW (0x0800_0000) suppresses the console window
    // that would otherwise flash up every time Settings is opened
    // (v0.5.1 pilot regression: "unsichtbares Fenster" beim Settings-
    // Tab-Klick — that was a real-but-empty cmd window briefly stealing
    // focus). The flag is Windows-specific so the Cargo / std::os
    // import is gated to this cfg block.
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    for key in [
        "HKCU\\Software\\Laminar Research\\X-Plane 12",
        "HKCU\\Software\\Laminar Research\\X-Plane 11",
    ] {
        let out = Command::new("reg")
            .args(["query", key, "/v", "Path"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        if !out.status.success() {
            continue;
        }
        let s = String::from_utf8_lossy(&out.stdout);
        // reg.exe output line:  "    Path    REG_SZ    C:\X-Plane 12\"
        for line in s.lines() {
            if let Some(idx) = line.find("REG_SZ") {
                let raw = line[idx + "REG_SZ".len()..].trim();
                let path = PathBuf::from(raw);
                if looks_like_xplane_root(&path) {
                    return Some(path);
                }
            }
        }
    }
    // Common-folder fallbacks if registry was unset (fresh install
    // that the pilot has never launched yet).
    for candidate in [
        "C:\\X-Plane 12",
        "C:\\X-Plane 11",
        "D:\\X-Plane 12",
        "D:\\X-Plane 11",
    ] {
        let p = PathBuf::from(candidate);
        if looks_like_xplane_root(&p) {
            return Some(p);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn detect_macos() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let candidates: Vec<PathBuf> = vec![
        PathBuf::from("/Applications/X-Plane 12"),
        PathBuf::from("/Applications/X-Plane 11"),
        home.join("X-Plane 12"),
        home.join("X-Plane 11"),
        home.join("Applications").join("X-Plane 12"),
        home.join("Applications").join("X-Plane 11"),
    ];
    candidates.into_iter().find(|p| looks_like_xplane_root(p))
}

#[cfg(target_os = "linux")]
fn detect_linux() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let candidates: Vec<PathBuf> = vec![
        home.join("X-Plane 12"),
        home.join("X-Plane 11"),
        PathBuf::from("/opt/X-Plane 12"),
        PathBuf::from("/opt/X-Plane 11"),
    ];
    candidates.into_iter().find(|p| looks_like_xplane_root(p))
}

/// Ordner aus dem Auswahldialog zum X-Plane-Hauptordner machen. Piloten
/// waehlen oft einen Unterordner (`Resources`, `Resources/plugins`) oder auf
/// dem Mac `X-Plane.app` — deshalb bis zu drei Ebenen nach oben suchen.
/// `None`, wenn dort nirgends ein X-Plane liegt.
pub fn normalize_install_path(path: &Path) -> Option<PathBuf> {
    let mut kandidat = Some(path);
    for _ in 0..4 {
        let k = kandidat?;
        if looks_like_xplane_root(k) {
            return Some(k.to_path_buf());
        }
        kandidat = k.parent();
    }
    None
}

/// Heuristic: an X-Plane root directory contains a `Resources/plugins/`
/// folder. Every X-Plane install has this; nothing else does.
fn looks_like_xplane_root(path: &Path) -> bool {
    path.is_dir() && path.join("Resources").join("plugins").is_dir()
}

/// Download + extract the plugin zip into `<xplane_root>/Resources/
/// plugins/AeroACARS/`. Returns the absolute path of the resulting
/// folder + counts.
pub async fn install_plugin(xplane_root: &Path) -> Result<PluginInstallResult, String> {
    // ---- Validate target ----
    if !looks_like_xplane_root(xplane_root) {
        return Err(format!(
            "Path doesn't look like an X-Plane install (no Resources/plugins/ subfolder): {}",
            xplane_root.display()
        ));
    }
    let target_root = xplane_root
        .join("Resources")
        .join("plugins")
        .join("AeroACARS");

    // ---- Download ----
    let version = env!("CARGO_PKG_VERSION");
    let url = PLUGIN_ZIP_URL_TEMPLATE.replace("{VERSION}", version);
    tracing::info!(url = %url, "downloading AeroACARS X-Plane plugin zip");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS))
        // Tracking redirects is required — GitHub releases redirect
        // through a separate CDN host.
        .redirect(reqwest::redirect::Policy::limited(8))
        .build()
        .map_err(|e| format!("failed to build HTTP client: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("failed to download plugin zip: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "GitHub returned status {} when fetching plugin zip from {}. \
             Make sure this AeroACARS version has a matching release.",
            resp.status(),
            url
        ));
    }
    let body = resp
        .bytes()
        .await
        .map_err(|e| format!("failed to read plugin zip body: {e}"))?;
    tracing::info!(bytes = body.len(), "plugin zip downloaded");

    // ---- Pruefsumme (vor jedem Eingriff in die alte Installation) ----
    let geprueft = pruefsumme_pruefen(&body, PLUGIN_SHA256)?;

    // ---- Wipe previous install if present ----
    if target_root.exists() {
        if let Err(e) = fs::remove_dir_all(&target_root) {
            return Err(format!(
                "could not clean previous install at {}: {}",
                target_root.display(),
                e
            ));
        }
    }
    fs::create_dir_all(&target_root).map_err(|e| {
        format!(
            "could not create plugin folder {}: {}",
            target_root.display(),
            e
        )
    })?;

    // ---- Extract ----
    // The zip's top-level entry is `AeroACARS/...`. We strip that
    // prefix so files land directly under the user's chosen
    // `<x-plane>/Resources/plugins/AeroACARS/` folder.
    let cursor = std::io::Cursor::new(body);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| format!("plugin zip is malformed: {e}"))?;

    let mut bytes_written: u64 = 0;
    let mut files_written: u32 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("could not read zip entry #{i}: {e}"))?;
        let entry_path = match entry.enclosed_name() {
            Some(p) => p.to_path_buf(),
            None => continue, // zip-slip / malformed name → skip
        };
        // Strip the leading `AeroACARS/` if present so we don't end
        // up at `<...>/AeroACARS/AeroACARS/64/win.xpl`.
        let stripped = entry_path
            .strip_prefix("AeroACARS")
            .unwrap_or(&entry_path)
            .to_path_buf();
        if stripped.as_os_str().is_empty() {
            continue;
        }
        let dest = target_root.join(&stripped);

        // Defence-in-depth: refuse anything that escaped the target
        // (zip-slip). `enclosed_name` already strips `..` but we
        // verify with a canonical check.
        if !dest.starts_with(&target_root) {
            tracing::warn!(?dest, "refusing zip entry outside target");
            continue;
        }

        if entry.is_dir() {
            fs::create_dir_all(&dest)
                .map_err(|e| format!("could not create dir {}: {}", dest.display(), e))?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("could not create dir {}: {}", parent.display(), e))?;
        }
        let mut out = fs::File::create(&dest)
            .map_err(|e| format!("could not create file {}: {}", dest.display(), e))?;
        let n = io::copy(&mut entry, &mut out)
            .map_err(|e| format!("could not write file {}: {}", dest.display(), e))?;
        bytes_written += n;
        files_written += 1;
    }

    // ---- macOS: Quarantaene-Kennzeichen entfernen ----
    #[cfg(target_os = "macos")]
    quarantaene_entfernen(&target_root);

    tracing::info!(
        target = %target_root.display(),
        bytes = bytes_written,
        files = files_written,
        "X-Plane plugin installed successfully"
    );
    Ok(PluginInstallResult {
        installed_at: target_root.to_string_lossy().into_owned(),
        bytes_written,
        files_written,
        geprueft,
    })
}

// v0.7.13: `uninstall_plugin` entfernt — der einzige Caller war der
// `xplane_uninstall_plugin` Tauri-Command, der mangels UI-Button nie
// gerufen wurde. Audit Q4-2026-05.

/// SHA-256 als Kleinbuchstaben-Hex.
fn sha256_hex(daten: &[u8]) -> String {
    Sha256::digest(daten)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Paket gegen die eingebettete Pruefsumme pruefen.
///
/// * keine Pruefsumme eingebettet (Entwicklungsbuild): Warnung, `Ok` —
///   installiert wie bisher. Im Release setzt die Pipeline sie immer.
/// * eingebettet, aber kein gueltiges SHA-256 (64 Hex-Zeichen): Fehler —
///   ein kaputter Build darf nicht still ungeprueft installieren.
/// * Abweichung: Fehler, nichts wird angefasst.
///
/// `Ok(true)` = geprueft, `Ok(false)` = ungeprueft (keine Pruefsumme).
fn pruefsumme_pruefen(paket: &[u8], erwartet: Option<&str>) -> Result<bool, String> {
    let Some(erwartet) = erwartet.map(str::trim).filter(|s| !s.is_empty()) else {
        tracing::warn!(
            "X-Plane-Plugin: keine Pruefsumme eingebettet (Entwicklungsbuild) — \
             Paket wird ungeprueft installiert"
        );
        return Ok(false);
    };
    if erwartet.len() != 64 || !erwartet.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!(
            "Eingebettete Plugin-Pruefsumme ist kein SHA-256 ({erwartet:?}) — \
             Installation abgebrochen."
        ));
    }
    let ist = sha256_hex(paket);
    if !ist.eq_ignore_ascii_case(erwartet) {
        tracing::warn!(%ist, %erwartet, "X-Plane-Plugin: Pruefsumme stimmt nicht");
        return Err(format!(
            "Das heruntergeladene Plugin-Paket stimmt nicht mit dieser AeroACARS-Version \
             ueberein (SHA-256 {ist}, erwartet {erwartet}). Nichts wurde veraendert — \
             bitte spaeter erneut versuchen."
        ));
    }
    tracing::info!(sha256 = %ist, "X-Plane-Plugin: Pruefsumme stimmt");
    Ok(true)
}

/// macOS: `com.apple.quarantine` rekursiv entfernen, damit X-Plane das
/// ad-hoc signierte `mac.xpl` ohne Gatekeeper-Sperre laedt. Fehler nur
/// protokollieren — ohne Kennzeichen (unser Prozess schreibt die Dateien
/// selbst) meldet `xattr` nichts, und ein Fehlschlag verhindert die
/// Installation nicht.
#[cfg(target_os = "macos")]
fn quarantaene_entfernen(ordner: &Path) {
    match std::process::Command::new("/usr/bin/xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(ordner)
        .output()
    {
        Ok(out) if out.status.success() => {
            tracing::info!(ordner = %ordner.display(), "X-Plane-Plugin: Quarantaene entfernt");
        }
        Ok(out) => tracing::warn!(
            code = ?out.status.code(),
            stderr = %String::from_utf8_lossy(&out.stderr),
            "X-Plane-Plugin: xattr meldet einen Fehler"
        ),
        Err(e) => tracing::warn!(error = %e, "X-Plane-Plugin: xattr nicht ausfuehrbar"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn gewaehlter_unterordner_fuehrt_zum_hauptordner() {
        let basis = std::env::temp_dir().join(format!("aa-xp-wahl-{}", std::process::id()));
        let root = basis.join("X-Plane 12");
        std::fs::create_dir_all(root.join("Resources").join("plugins")).unwrap();
        std::fs::create_dir_all(root.join("X-Plane.app").join("Contents")).unwrap();
        for gewaehlt in [
            root.clone(),
            root.join("Resources"),
            root.join("Resources").join("plugins"),
            root.join("X-Plane.app"),
        ] {
            assert_eq!(
                super::normalize_install_path(&gewaehlt).as_deref(),
                Some(root.as_path()),
                "{gewaehlt:?}"
            );
        }
        // Ein Ordner ohne X-Plane (auch nicht darueber) bleibt ohne Treffer.
        assert_eq!(super::normalize_install_path(&basis), None);
        let _ = std::fs::remove_dir_all(&basis);
    }

    use super::*;

    /// Bekannter Pruefwert (FIPS 180-2, „abc").
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn sha256_bekannter_wert() {
        assert_eq!(sha256_hex(b"abc"), ABC);
    }

    #[test]
    fn pruefsumme_stimmt_oder_bricht_ab() {
        assert_eq!(pruefsumme_pruefen(b"abc", Some(ABC)), Ok(true));
        assert!(pruefsumme_pruefen(b"abc", Some(&ABC.to_uppercase())).is_ok());
        assert!(pruefsumme_pruefen(b"abc", Some(&format!("  {ABC}\n"))).is_ok());
        let falsch = pruefsumme_pruefen(b"abd", Some(ABC)).unwrap_err();
        assert!(falsch.contains("erwartet"), "{falsch}");
    }

    /// Entwicklungsbuild ohne Pruefsumme: installiert wie bisher.
    #[test]
    fn ohne_pruefsumme_wie_bisher() {
        assert_eq!(pruefsumme_pruefen(b"egal", None), Ok(false));
        assert_eq!(pruefsumme_pruefen(b"egal", Some("")), Ok(false));
    }

    /// Kaputt eingebettete Pruefsumme: nie still ungeprueft installieren.
    #[test]
    fn kaputte_pruefsumme_bricht_ab() {
        assert!(pruefsumme_pruefen(b"abc", Some("abc123")).is_err());
        assert!(pruefsumme_pruefen(b"abc", Some(&"g".repeat(64))).is_err());
    }

    /// Echt am Dateisystem: Kennzeichen setzen, entfernen, weg.
    #[cfg(target_os = "macos")]
    #[test]
    fn quarantaene_wird_entfernt() {
        let ordner =
            std::env::temp_dir().join(format!("aeroacars-quarantaene-{}", std::process::id()));
        let unter = ordner.join("64");
        std::fs::create_dir_all(&unter).unwrap();
        let datei = unter.join("mac.xpl");
        std::fs::write(&datei, b"plugin").unwrap();
        let gesetzt = std::process::Command::new("/usr/bin/xattr")
            .args(["-w", "com.apple.quarantine", "0081;00000000;Test;"])
            .arg(&datei)
            .status()
            .unwrap();
        assert!(gesetzt.success());
        let lesen = || {
            std::process::Command::new("/usr/bin/xattr")
                .args(["-p", "com.apple.quarantine"])
                .arg(&datei)
                .output()
                .unwrap()
                .status
                .success()
        };
        assert!(lesen(), "Kennzeichen nicht gesetzt");
        quarantaene_entfernen(&ordner);
        assert!(!lesen(), "Kennzeichen noch da");
        let _ = std::fs::remove_dir_all(&ordner);
    }
}
