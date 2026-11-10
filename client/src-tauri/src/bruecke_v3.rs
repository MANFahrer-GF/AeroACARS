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
//! Beide Befehle stehen in der LAN-Brücke auf der Verbotsliste (`DENY_SET` in
//! `remote/bridge.rs`, dort von einem Test erzwungen): vom Tablet aus darf
//! niemand installieren und beenden.

use std::path::Path;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

/// v2.1.27 — der stabile Kanal, nicht mehr der Beta-Kanal: In den schreibt
/// `scripts/beta-veroeffentlichen.sh` (v3) auch künftige Betas, die letzte
/// 2.x zwänge Nachzügler sonst auf eine Beta. Den stabilen Kanal füllt nur
/// `--final`; solange er fehlt, liefert die Abfrage `None` und nichts sperrt.
const KANAL_V3: &str =
    "https://github.com/MANFahrer-GF/AeroACARS/releases/download/aeroacars3-kanal/latest.json";

/// Windows-Installation von 3 laut NSIS-Vorlage: `<Wurzel>\<Produktname>\<Programm>.exe`.
const V3_ORDNER: &str = "AeroACARS 3";
const V3_PROGRAMM: &str = "aeroacars-app.exe";

/// 2.x bleibt auf Windows neben 3 installiert, über die alte Verknüpfung
/// startet es also weiter. Liegt 3 schon da, wird es geöffnet statt noch
/// einmal geladen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn v3_programm_in(wurzeln: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    wurzeln.iter().map(|w| w.join(V3_ORDNER).join(V3_PROGRAMM)).find(|p| p.is_file())
}

/// Wer im Installer einen eigenen Ordner gewählt hat, ist nur über den
/// `InstallLocation`-Wert des Uninstall-Schlüssels zu finden. NSIS schreibt
/// ihn in Anführungszeichen. Ein veralteter Wert (Programm fehlt) zählt nicht.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn v3_programm_aus_install_location(wert: &str) -> Option<std::path::PathBuf> {
    let ordner = wert.trim().trim_matches('"').trim();
    if ordner.is_empty() {
        return None;
    }
    Some(std::path::Path::new(ordner).join(V3_PROGRAMM)).filter(|p| p.is_file())
}

#[cfg(target_os = "windows")]
fn installiertes_v3() -> Option<std::path::PathBuf> {
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
    use winreg::RegKey;

    let schluessel = format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{V3_ORDNER}");
    let aus_registry = [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE].into_iter().find_map(|wurzel| {
        let wert: String = RegKey::predef(wurzel)
            .open_subkey(&schluessel)
            .ok()?
            .get_value("InstallLocation")
            .ok()?;
        v3_programm_aus_install_location(&wert)
    });
    let wurzeln: Vec<std::path::PathBuf> = ["LOCALAPPDATA", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(std::path::PathBuf::from)
        .collect();
    aus_registry.or_else(|| v3_programm_in(&wurzeln))
}

/// Auf macOS tauscht die Brücke das Bundle, 2.x ist danach weg.
#[cfg(not(target_os = "windows"))]
fn installiertes_v3() -> Option<std::path::PathBuf> {
    None
}

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

/// Was 3 beim ersten Start nicht sauber übernimmt (`datenuebernahme.rs` auf
/// v3): die PIREP-Warteschlange und den offenen Flug lässt es liegen — auf
/// macOS ist 2.x nach dem Bundle-Tausch weg, ein ungesendeter Flug bliebe
/// für immer liegen. Die Bahn-Nachträge (`bahn_nachtraege/<va>/<pilot>/`)
/// kopiert es dagegen mit, unter Windows sendeten sie dann beide Apps.
/// Deshalb erst umsteigen, wenn alles leer ist.
fn sperre(daten: Option<&Path>, konfig: Option<&Path>) -> Option<&'static str> {
    if konfig.is_some_and(|k| k.join("active_flight.json").exists()) {
        return Some("flug");
    }
    let wartet = daten.is_some_and(|d| {
        enthaelt_datei(&d.join("pending_pireps")) || enthaelt_datei(&d.join("bahn_nachtraege"))
    });
    wartet.then_some("warteschlange")
}

/// Liegt irgendwo darunter eine Datei? Leere Unterordner zählen nicht.
fn enthaelt_datei(ordner: &Path) -> bool {
    std::fs::read_dir(ordner).is_ok_and(|eintraege| {
        eintraege.flatten().any(|e| {
            let p = e.path();
            if p.is_dir() { enthaelt_datei(&p) } else { true }
        })
    })
}

fn sperre_app(app: &AppHandle) -> Option<&'static str> {
    let daten = app.path().app_data_dir().ok();
    let konfig = app.path().app_config_dir().ok();
    sperre(daten.as_deref(), konfig.as_deref())
}

/// Beim ersten Start legt 3 den Merker `.von_2x_uebernommen` in seinen
/// Datenordner neben dem von 2.x (`datenuebernahme.rs` auf v3). Damit lässt
/// sich ein gelungenes Öffnen von einem unterscheiden, das 3 nie gestartet hat.
fn v3_lief_schon(daten_2x: Option<&Path>) -> bool {
    daten_2x
        .and_then(Path::parent)
        .is_some_and(|wurzel| wurzel.join("com.aeroacars.app3").join(".von_2x_uebernommen").exists())
}

#[derive(Serialize)]
pub struct BrueckeV3 {
    version: String,
    /// "flug" | "warteschlange", solange der Umstieg Daten verlieren würde.
    sperre: Option<&'static str>,
    /// 3 liegt schon auf diesem Rechner (nur Windows): öffnen statt laden.
    installiert: bool,
    /// 3 ist installiert und schon einmal gestartet: Öffnen trägt also.
    lief_schon: bool,
}

/// Version von AeroACARS 3, wenn eine für dieses System bereitliegt.
/// Fehler (kein Netz, Intel-Mac ohne Datei im Kanal …) ergeben `None`.
#[tauri::command]
pub async fn bruecke_v3_pruefen(app: AppHandle) -> Option<BrueckeV3> {
    match update_v3(&app).await {
        Ok(u) => u.map(|u| {
            let installiert = installiertes_v3().is_some();
            BrueckeV3 {
                version: u.version,
                sperre: sperre_app(&app),
                installiert,
                lief_schon: installiert && v3_lief_schon(app.path().app_data_dir().ok().as_deref()),
            }
        }),
        Err(e) => {
            tracing::info!(fehler = %e, "Brücke v3: keine Version für dieses System");
            None
        }
    }
}

/// Lädt AeroACARS 3, startet die Installation und beendet 2.x.
#[tauri::command]
pub async fn bruecke_v3_installieren(app: AppHandle) -> Result<(), String> {
    if let Some(grund) = sperre_app(&app) {
        return Err(format!("gesperrt:{grund}"));
    }
    if let Some(programm) = installiertes_v3() {
        oeffnen(&app, &programm)?;
        app.exit(0);
        return Ok(());
    }
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
fn oeffnen(app: &AppHandle, programm: &Path) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(programm.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    tracing::info!(programm = %programm.display(), "Brücke v3: vorhandene Installation geöffnet");
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn oeffnen(_app: &AppHandle, _programm: &Path) -> Result<(), String> {
    Err("AeroACARS 3 lässt sich nur unter Windows direkt öffnen.".into())
}

#[cfg(target_os = "windows")]
fn starten(app: &AppHandle, _update: &tauri_plugin_updater::Update, bytes: &[u8]) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    // Eigener Name je Lauf: ein noch offener älterer Installer sperrt seine Datei.
    let pfad = std::env::temp_dir().join(format!("AeroACARS-3-setup-{}.exe", std::process::id()));
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
    // Erst das Bundle finden, dann tauschen: ohne `.app` (Dev-Build) würde der
    // Updater sonst das Verzeichnis der Programmdatei ersetzen.
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let bundle = bundle_aus_exe(&exe).ok_or_else(|| format!("kein .app-Bundle über {}", exe.display()))?;
    update.install(bytes).map_err(|e| e.to_string())?;
    // Ab hier ist 3 installiert: ein Fehler beim Helfer darf nicht als
    // „fehlgeschlagen“ erscheinen, AeroACARS muss nur von Hand geöffnet werden.
    let helfer = std::process::Command::new("/bin/sh")
        .arg("-c")
        .arg(r#"while kill -0 "$1" 2>/dev/null; do sleep 0.5; done; open "$2""#)
        .arg("bruecke-v3")
        .arg(std::process::id().to_string())
        .arg(&bundle)
        .process_group(0)
        .spawn();
    match helfer {
        Ok(_) => tracing::info!(bundle = %bundle.display(), "Brücke v3: Bundle getauscht, Öffnen nach dem Beenden"),
        Err(e) => tracing::warn!(fehler = %e, "Brücke v3: Bundle getauscht, Helfer nicht gestartet — von Hand öffnen"),
    }
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
    fn v3_programm_wird_in_allen_wurzeln_gesucht() {
        let leer = ordner("wurzel-leer");
        let voll = ordner("wurzel-voll");
        assert_eq!(v3_programm_in(&[leer.clone(), voll.clone()]), None);
        // Nur der Ordner ohne Programm (abgebrochene Installation) zählt nicht.
        std::fs::create_dir_all(voll.join(V3_ORDNER)).unwrap();
        assert_eq!(v3_programm_in(&[leer.clone(), voll.clone()]), None);
        let programm = voll.join(V3_ORDNER).join(V3_PROGRAMM);
        std::fs::write(&programm, b"x").unwrap();
        assert_eq!(v3_programm_in(&[leer, voll]), Some(programm));
    }

    #[test]
    fn install_location_mit_und_ohne_anfuehrungszeichen() {
        let eigener = ordner("eigener ordner");
        std::fs::create_dir_all(&eigener).unwrap();
        let mit = format!("\"{}\"", eigener.display());
        let ohne = eigener.display().to_string();
        assert_eq!(v3_programm_aus_install_location(&mit), None, "Programm fehlt noch");

        let programm = eigener.join(V3_PROGRAMM);
        std::fs::write(&programm, b"x").unwrap();
        assert_eq!(v3_programm_aus_install_location(&mit), Some(programm.clone()));
        assert_eq!(v3_programm_aus_install_location(&ohne), Some(programm));
        assert_eq!(v3_programm_aus_install_location(""), None);
        assert_eq!(v3_programm_aus_install_location("\"\""), None);
    }

    #[test]
    fn v3_lief_schon_liest_den_merker_neben_dem_2x_ordner() {
        let wurzel = ordner("merker");
        let daten_2x = wurzel.join("com.aeroacars.app");
        assert!(!v3_lief_schon(Some(&daten_2x)));
        let daten_3 = wurzel.join("com.aeroacars.app3");
        std::fs::create_dir_all(&daten_3).unwrap();
        assert!(!v3_lief_schon(Some(&daten_2x)), "Ordner ohne Merker zählt nicht");
        std::fs::write(daten_3.join(".von_2x_uebernommen"), "").unwrap();
        assert!(v3_lief_schon(Some(&daten_2x)));
        assert!(!v3_lief_schon(None));
    }

    #[test]
    fn die_letzte_2x_folgt_dem_stabilen_kanal() {
        assert!(KANAL_V3.contains("/aeroacars3-kanal/"), "{KANAL_V3}");
    }

    fn ordner(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bruecke-v3-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sperre_bei_aktivem_flug_und_wartenden_pireps() {
        let daten = ordner("daten");
        let konfig = ordner("konfig");
        assert_eq!(sperre(Some(&daten), Some(&konfig)), None);
        // Leerer Warteschlangen-Ordner sperrt nicht (der Worker legt ihn an).
        std::fs::create_dir_all(daten.join("pending_pireps")).unwrap();
        assert_eq!(sperre(Some(&daten), Some(&konfig)), None);
        std::fs::write(daten.join("pending_pireps/1.json"), "{}").unwrap();
        assert_eq!(sperre(Some(&daten), Some(&konfig)), Some("warteschlange"));
        std::fs::remove_file(daten.join("pending_pireps/1.json")).unwrap();
        // Bahn-Nachträge liegen eine Ebene tiefer je VA/Pilot.
        std::fs::create_dir_all(daten.join("bahn_nachtraege/gsg/1")).unwrap();
        assert_eq!(sperre(Some(&daten), Some(&konfig)), None);
        std::fs::write(daten.join("bahn_nachtraege/gsg/1/n.json"), "{}").unwrap();
        assert_eq!(sperre(Some(&daten), Some(&konfig)), Some("warteschlange"));
        std::fs::write(konfig.join("active_flight.json"), "{}").unwrap();
        assert_eq!(sperre(Some(&daten), Some(&konfig)), Some("flug"));
        let _ = std::fs::remove_dir_all(&daten);
        let _ = std::fs::remove_dir_all(&konfig);
    }

}
