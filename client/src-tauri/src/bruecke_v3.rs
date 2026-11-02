//! v2.1.25 — Brücke zu AeroACARS 3 (Thomas 02.11.2026).
//!
//! Der 2.x-Updater kennt nur `releases/latest` (2.x). Ein Umstieg auf 3 über
//! ihn taugt auf Windows nicht: Der NSIS-Installer legt im Update-Modus
//! (`/UPDATE`) keine Verknüpfungen an, „AeroACARS 3“ landet unsichtbar neben
//! 2.x (Befund 30.10.2026). Deshalb hier ein eigener Weg:
//!
//! - `bruecke_v3_pruefen` fragt den 3.x-Kanal ab (derselbe Signierschlüssel
//!   wie 2.x, `plugins.updater.pubkey`) und liefert die Version oder nichts.
//! - `bruecke_v3_installieren` lädt die Datei (Signatur prüft der Updater),
//!   - Windows: speichert den Installer und startet ihn NORMAL (ShellExecute
//!     über den Opener, wie der Updater selbst, nur ohne `/UPDATE`) — dann
//!     entstehen die Verknüpfungen. 2.x beendet sich, damit nicht beide
//!     gleichzeitig laufen.
//!   - macOS: der Updater tauscht das Bundle; geöffnet wird es danach von
//!     einem kleinen Helfer, sobald dieser Prozess weg ist (`app.restart()`
//!     nach dem Bundle-Tausch ist in 3.0 unzuverlässig, Befund 31.10.2026).
//!
//! Beide Befehle stehen bewusst NICHT in der LAN-Brücke (`remote/bridge.rs`):
//! vom Tablet aus darf niemand installieren und beenden.

use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// Der Beta-Kanal von 3.x wechselt mit dem fertigen 3.0 mit
/// (`scripts/beta-veroeffentlichen.sh` auf v3: KANAL / KANAL_FINAL).
const KANAL_V3: &str =
    "https://github.com/MANFahrer-GF/AeroACARS/releases/download/aeroacars3-beta-kanal/latest.json";

async fn update_v3(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    let url = KANAL_V3.parse().map_err(|e| format!("{e}"))?;
    app.updater_builder()
        .endpoints(vec![url])
        .map_err(|e| e.to_string())?
        .build()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())
}

/// Version von AeroACARS 3, wenn eine für dieses System bereitliegt.
/// Fehler (kein Netz, Intel-Mac ohne Datei im Kanal …) ergeben `None`.
#[tauri::command]
pub async fn bruecke_v3_pruefen(app: AppHandle) -> Option<String> {
    match update_v3(&app).await {
        Ok(u) => u.map(|u| u.version),
        Err(e) => {
            tracing::info!(fehler = %e, "Brücke v3: keine Version für dieses System");
            None
        }
    }
}

/// Lädt AeroACARS 3, startet die Installation und beendet 2.x.
#[tauri::command]
pub async fn bruecke_v3_installieren(app: AppHandle) -> Result<(), String> {
    let update = update_v3(&app)
        .await?
        .ok_or_else(|| "Keine Version von AeroACARS 3 gefunden.".to_string())?;
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    tracing::info!(version = %update.version, groesse = bytes.len(), "Brücke v3: geladen, Signatur geprüft");
    starten(&app, &update, &bytes)?;
    app.exit(0);
    Ok(())
}

#[cfg(target_os = "windows")]
fn starten(app: &AppHandle, _update: &tauri_plugin_updater::Update, bytes: &[u8]) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let pfad = std::env::temp_dir().join("AeroACARS-3-setup.exe");
    std::fs::write(&pfad, bytes).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(pfad.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    tracing::info!(pfad = %pfad.display(), "Brücke v3: Installer gestartet");
    Ok(())
}

#[cfg(target_os = "macos")]
fn starten(_app: &AppHandle, update: &tauri_plugin_updater::Update, bytes: &[u8]) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    update.install(bytes).map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = bundle_aus_exe(&exe).ok_or_else(|| format!("kein .app-Bundle über {}", exe.display()))?;
    std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.5; done; open "$2""#)
        .arg("bruecke-v3")
        .arg(std::process::id().to_string())
        .arg(&bundle)
        .process_group(0)
        .spawn()
        .map_err(|e| e.to_string())?;
    tracing::info!(bundle = %bundle.display(), "Brücke v3: Bundle getauscht, Öffnen nach dem Beenden");
    Ok(())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn starten(_app: &AppHandle, _update: &tauri_plugin_updater::Update, _bytes: &[u8]) -> Result<(), String> {
    Err("Umstieg auf AeroACARS 3 gibt es nur für Windows und macOS.".into())
}

/// `/Applications/AeroACARS.app/Contents/MacOS/aeroacars` → `/Applications/AeroACARS.app`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn bundle_aus_exe(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    let bundle = exe.parent()?.parent()?.parent()?;
    (bundle.extension()? == "app").then(|| bundle.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn bundle_aus_exe_findet_das_app_verzeichnis() {
        assert_eq!(
            bundle_aus_exe(Path::new("/Applications/AeroACARS.app/Contents/MacOS/aeroacars")),
            Some(std::path::PathBuf::from("/Applications/AeroACARS.app"))
        );
    }

    #[test]
    fn bundle_aus_exe_ohne_app_ist_none() {
        // Dev-Build aus target/: dort gibt es kein Bundle zu öffnen.
        assert_eq!(bundle_aus_exe(Path::new("/x/target/debug/deps/aeroacars")), None);
    }

    #[test]
    fn nicht_ueber_die_lan_bruecke_erreichbar() {
        let bridge = include_str!("remote/bridge.rs");
        // Gegenprobe: so sieht ein weitergeleiteter Befehl dort aus.
        assert!(bridge.contains("\"app_info\" =>"), "Muster der LAN-Brücke geändert — Test anpassen");
        assert!(!bridge.contains("\"bruecke_v3_"), "Installieren vom Tablet aus ist verboten");
    }
}
