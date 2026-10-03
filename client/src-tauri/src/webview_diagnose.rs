//! Start-Diagnose der Anzeigekomponente (WebView2).
//!
//! 03.10.2026, Health-Report: Bei einem neuen Piloten (Max G., erster Start
//! ueberhaupt) scheiterte das Anlegen des Hauptfensters mehrfach mit
//! `failed to create webview … 0x8007139F` (Windows: ERROR_INVALID_STATE).
//! Das Log sagte nichts darueber, in welchem Zustand der Rechner dabei war.
//! Diese Zeile haelt ihn fest, damit der naechste Fall ohne Rueckfrage beim
//! Piloten einzuordnen ist. Sie aendert kein Verhalten.
//!
//! Gemessen wird in einem eigenen Faden und gleich beim Start: Der Start
//! wartet nicht darauf, und die Prozesszahl zeigt den Zustand VOR dem Fenster
//! (Ueberbleibsel eines frueheren Laufs), soweit der Faden schneller ist als
//! die Fensteranlage.

use std::path::Path;

/// Prozessname der Anzeigekomponente. Windows haengt `.exe` an, die
/// anderen Systeme nicht; der Vergleich ignoriert Gross-/Kleinschreibung.
const WEBVIEW_PROZESS: &str = "msedgewebview2";

/// Zaehlt die Prozesse der Anzeigekomponente in einer Liste von Namen.
fn webview_prozesse<S: AsRef<str>>(namen: &[S]) -> usize {
    namen
        .iter()
        .filter(|n| {
            let n = n.as_ref().to_ascii_lowercase();
            n == WEBVIEW_PROZESS || n == format!("{WEBVIEW_PROZESS}.exe")
        })
        .count()
}

/// Zaehlt andere AeroACARS-Prozesse (ohne den eigenen): Ueberbleibsel eines
/// frueheren Laufs, die den Datenordner der Anzeigekomponente festhalten
/// koennten. `eigene_pid` wird nicht mitgezaehlt.
fn andere_aeroacars_prozesse<S: AsRef<str>>(prozesse: &[(u32, S)], eigene_pid: u32) -> usize {
    prozesse
        .iter()
        .filter(|(pid, name)| {
            *pid != eigene_pid && name.as_ref().to_ascii_lowercase().starts_with("aeroacars")
        })
        .count()
}

/// Zustand des Datenordners der Anzeigekomponente: legt Windows beim ersten
/// erfolgreichen Start an. Nach einer Neuinstallation fehlt er.
fn datenordner_zustand(ordner: &Path) -> &'static str {
    if !ordner.is_dir() {
        "fehlt"
    } else if ordner.join("Default").is_dir() {
        "vorhanden"
    } else {
        "angelegt, noch ohne Profil"
    }
}

/// Schreibt eine Zeile ins Log. Kehrt sofort zurueck.
pub fn protokollieren(kennung: &'static str) {
    let _ = std::thread::Builder::new()
        .name("webview-diagnose".into())
        .spawn(move || {
            let version = tauri::webview_version().unwrap_or_else(|e| format!("nicht lesbar ({e})"));

            let mut system = sysinfo::System::new();
            system.refresh_processes_specifics(
                sysinfo::ProcessesToUpdate::All,
                true,
                sysinfo::ProcessRefreshKind::new(),
            );
            let prozesse: Vec<(u32, String)> = system
                .processes()
                .iter()
                .filter_map(|(pid, p)| p.name().to_str().map(|n| (pid.as_u32(), n.to_owned())))
                .collect();
            let namen: Vec<&str> = prozesse.iter().map(|(_, n)| n.as_str()).collect();

            // Windows legt die Daten der Anzeigekomponente unter
            // %LOCALAPPDATA%\<Kennung>\EBWebView ab.
            let datenordner = std::env::var_os("LOCALAPPDATA")
                .map(|d| Path::new(&d).join(kennung).join("EBWebView"))
                .map(|p| datenordner_zustand(&p))
                .unwrap_or("unbekannt");

            tracing::info!(
                webview_version = %version,
                webview_prozesse = webview_prozesse(&namen),
                andere_aeroacars_prozesse = andere_aeroacars_prozesse(&prozesse, std::process::id()),
                datenordner,
                "Anzeigekomponente beim Start"
            );
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zaehlt_nur_die_anzeigekomponente_unabhaengig_von_endung_und_schreibweise() {
        let namen = [
            "msedgewebview2.exe",
            "MSEDGEWEBVIEW2.EXE",
            "msedgewebview2",
            "msedge.exe",
            "msedgewebview2helper.exe",
            "aeroacars.exe",
        ];
        assert_eq!(webview_prozesse(&namen), 3);
        assert_eq!(webview_prozesse::<&str>(&[]), 0);
    }

    #[test]
    fn eigener_prozess_zaehlt_nicht_als_ueberbleibsel() {
        let prozesse = [
            (100u32, "aeroacars.exe"),
            (200, "AeroACARS.exe"),
            (300, "aeroacars-live-tool.exe"),
            (400, "explorer.exe"),
        ];
        assert_eq!(andere_aeroacars_prozesse(&prozesse, 100), 2);
        assert_eq!(andere_aeroacars_prozesse(&prozesse, 999), 3);
    }

    #[test]
    fn datenordner_wird_nach_seinem_inhalt_eingeordnet() {
        let basis = std::env::temp_dir().join(format!("aeroacars-wv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&basis);
        assert_eq!(datenordner_zustand(&basis), "fehlt");
        std::fs::create_dir_all(&basis).unwrap();
        assert_eq!(datenordner_zustand(&basis), "angelegt, noch ohne Profil");
        std::fs::create_dir_all(basis.join("Default")).unwrap();
        assert_eq!(datenordner_zustand(&basis), "vorhanden");
        let _ = std::fs::remove_dir_all(&basis);
    }
}
