//! Gespiegelter UI-Zustand (v1.5.6, #lan-bruecke-1zu1).
//!
//! Ein paar Werte, die der Pilot setzt, lagen bisher NUR im `localStorage`
//! des jeweiligen Browsers: SimBrief-Benutzername/-ID, welche Nachrichten
//! er gelesen hat, seine Transponder-Notiz. Am PC funktionierte das —
//! über die LAN-Brücke nicht: das Tablet ist ein anderer Browser mit
//! eigenem `localStorage`, sah also leere Felder und alle Nachrichten als
//! ungelesen (Feldbefund Thomas, 11.08.2026).
//!
//! Hier liegt die gemeinsame Wahrheit: eine kleine Key-Value-Ablage im
//! Konfig-Verzeichnis des Hosts, erreichbar über zwei Befehle, die (wie
//! alles andere) über die Brücke gehen. Der Host ist die Quelle; jedes
//! Gerät hydratisiert daraus beim Start und schreibt Änderungen zurück.
//!
//! Bewusst NICHT hier: reine Ansichtsvorlieben (Kartenhintergrund,
//! Track-Up, eingeklappte Navigation, zuletzt offener Reiter). Die dürfen
//! sich zwischen 27-Zoll-Monitor und Tablet unterscheiden — sie zu
//! spiegeln würde die Bedienung verschlechtern, nicht verbessern.
//!
//! Persistenz-Muster wie bei `auto_start.json`: eine JSON-Datei im
//! `app_config_dir`. Fehler sind nie fatal — schlägt Lesen oder Schreiben
//! fehl, arbeitet die UI mit ihrem lokalen Stand weiter.
//!
//! ## Sitzungs-Schlüssel (`session:`-Präfix)
//!
//! Manches soll GETEILT, aber nicht HALTBAR sein. Beispiel: der
//! Transponder-Merker im Datalink. Er lag absichtlich im `sessionStorage`,
//! weil ein dauerhafter Merker beim nächsten Flug fälschlich "schon
//! übernommen" meldet (dokumentierter Fix). Über die Brücke soll das
//! Tablet ihn trotzdem sehen. Deshalb: Schlüssel mit `session:`-Präfix
//! leben im Speicher wie alle anderen, werden aber NIE auf die Platte
//! geschrieben — geteilt, solange die App läuft, weg beim Beenden.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

/// In-Memory-Spiegel, damit ein Lesen pro Tick keine Datei anfasst.
/// `None` = noch nicht von der Platte geladen.
#[derive(Default)]
pub struct UiStateStore {
    inner: Mutex<Option<HashMap<String, String>>>,
}

fn store_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|p| p.join("ui_state.json"))
}

fn load_from_disk(app: &AppHandle) -> HashMap<String, String> {
    let Some(path) = store_path(app) else {
        return HashMap::new();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Schlüssel, die nur für die laufende Programmsitzung gelten.
pub fn is_session_key(key: &str) -> bool {
    key.starts_with("session:")
}

fn save_to_disk(app: &AppHandle, map: &HashMap<String, String>) {
    let Some(path) = store_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Sitzungs-Schlüssel bleiben im Speicher — sie dürfen den Neustart
    // NICHT überleben (siehe Modul-Kommentar).
    let persist: HashMap<&String, &String> =
        map.iter().filter(|(k, _)| !is_session_key(k)).collect();
    match serde_json::to_string_pretty(&persist) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                tracing::warn!(error = %e, "ui_state: konnte nicht schreiben");
            }
        }
        Err(e) => tracing::warn!(error = %e, "ui_state: konnte nicht serialisieren"),
    }
}

/// Alle gespiegelten Werte. Erstes Lesen holt sie von der Platte.
#[tauri::command]
pub fn ui_state_get_all(app: AppHandle) -> HashMap<String, String> {
    let store = app.state::<UiStateStore>();
    let mut guard = store.inner.lock().expect("ui_state poisoned");
    if guard.is_none() {
        *guard = Some(load_from_disk(&app));
    }
    guard.clone().unwrap_or_default()
}

/// Einen Wert setzen (`value: None` löscht ihn). Schreibt sofort durch,
/// damit ein Absturz zwischen zwei Flügen nichts verliert.
#[tauri::command]
pub fn ui_state_set(app: AppHandle, key: String, value: Option<String>) {
    if key.trim().is_empty() {
        return;
    }
    let store = app.state::<UiStateStore>();
    let snapshot = {
        let mut guard = store.inner.lock().expect("ui_state poisoned");
        if guard.is_none() {
            *guard = Some(load_from_disk(&app));
        }
        let map = guard.as_mut().expect("gerade befüllt");
        match value {
            Some(v) => {
                map.insert(key, v);
            }
            None => {
                map.remove(&key);
            }
        }
        map.clone()
    };
    save_to_disk(&app, &snapshot);
}

/// Mehrere Werte auf einmal übernehmen, OHNE vorhandene zu überschreiben.
/// Das ist der Migrationspfad: ein Gerät, das seine Werte noch nur lokal
/// hat, schiebt sie beim ersten Start hoch — ein zweites Gerät kann so
/// aber nicht die (neueren) Werte des ersten plattmachen.
#[tauri::command]
pub fn ui_state_seed(app: AppHandle, values: HashMap<String, String>) -> HashMap<String, String> {
    let store = app.state::<UiStateStore>();
    let (snapshot, changed) = {
        let mut guard = store.inner.lock().expect("ui_state poisoned");
        if guard.is_none() {
            *guard = Some(load_from_disk(&app));
        }
        let map = guard.as_mut().expect("gerade befüllt");
        let mut changed = false;
        for (k, v) in values {
            if k.trim().is_empty() || v.is_empty() {
                continue;
            }
            if !map.contains_key(&k) {
                map.insert(k, v);
                changed = true;
            }
        }
        (map.clone(), changed)
    };
    if changed {
        save_to_disk(&app, &snapshot);
    }
    snapshot
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Der Migrations-Vertrag von `ui_state_seed`, an der reinen
    /// Merge-Logik geprüft (die Befehle selbst brauchen eine App-Instanz).
    #[test]
    fn seed_never_overwrites_existing_values() {
        let mut map: HashMap<String, String> = HashMap::new();
        map.insert("simbrief_username".into(), "thomas".into());

        // Zweites Gerät bringt einen alten Wert mit — der darf NICHT gewinnen.
        let incoming: Vec<(String, String)> = vec![
            ("simbrief_username".into(), "alter-name".into()),
            ("aeroacars.readNewsIds".into(), "[1,2]".into()),
            ("leer".into(), String::new()),
        ];
        for (k, v) in incoming {
            if k.trim().is_empty() || v.is_empty() {
                continue;
            }
            map.entry(k).or_insert(v);
        }

        assert_eq!(map.get("simbrief_username").map(String::as_str), Some("thomas"));
        assert_eq!(map.get("aeroacars.readNewsIds").map(String::as_str), Some("[1,2]"));
        assert!(!map.contains_key("leer"), "leere Werte werden nicht gesät");
    }
}
