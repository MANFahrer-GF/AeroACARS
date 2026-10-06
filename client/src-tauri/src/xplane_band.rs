//! X-Plane-HUD-Band, App-Seite (ADR-0005).
//!
//! Ein Takt (tokio-Aufgabe auf der vorhandenen Laufzeit, kein eigener Faden)
//! baut alle 500 ms das Band aus denselben Daten wie die `/panel/*`-Routen
//! und legt es in den [`sim_xplane::hud_band::BandSlot`]. Gesendet wird es
//! von der Plugin-Sitzung (Plugin-Faden), nicht von hier: die Sitzung darf
//! keine App-Zustaende anfassen (`XPlaneAdapter::stop` haelt die Sim-Sperre
//! und wartet auf diesen Faden).
//!
//! Gebaut wird nur, solange die Sitzung "bereit" meldet (Plugin angemeldet,
//! >= 1.1.0, Einstellung an) — sonst kostet der Takt nichts.
//!
//! Einstellung „X-Plane-Band senden": Datei `xplane_band.json` im
//! Konfigurationsverzeichnis, Standard an, wirkt sofort.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use sim_xplane::hud_band::{baue_band, lage, BandEingabe, BandSlot, Lage};
use tauri::{AppHandle, Manager};

const CONFIG_FILE: &str = "xplane_band.json";
const TAKT: Duration = Duration::from_millis(500);

#[derive(serde::Serialize, serde::Deserialize)]
struct BandConfig {
    enabled: bool,
}

fn config_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join(CONFIG_FILE))
}

/// Fehlende oder kaputte Datei = an (Standard).
fn read_enabled_from(path: &Path) -> bool {
    std::fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice::<BandConfig>(&b).ok())
        .map(|c| c.enabled)
        .unwrap_or(true)
}

fn write_enabled_to(path: &Path, enabled: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json =
        serde_json::to_vec_pretty(&BandConfig { enabled }).expect("BandConfig serialisiert immer");
    std::fs::write(path, json)
}

pub fn is_enabled(app: &AppHandle) -> bool {
    config_path(app)
        .map(|p| read_enabled_from(&p))
        .unwrap_or(true)
}

fn slot(app: &AppHandle) -> Arc<BandSlot> {
    let state = app.state::<crate::AppState>();
    let xp = state.xplane.lock().expect("xplane lock");
    xp.band_slot()
}

/// Schreibt die Einstellung und wendet sie sofort an.
pub fn set_enabled(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let Some(path) = config_path(app) else {
        return Err("kein Konfigurationsverzeichnis".to_string());
    };
    write_enabled_to(&path, enabled).map_err(|e| e.to_string())?;
    slot(app).set_wunsch(enabled);
    Ok(())
}

/// Einmal beim Start: Einstellung laden und den Takt starten.
pub fn spawn(app: AppHandle) {
    let slot = slot(&app);
    slot.set_wunsch(is_enabled(&app));
    tauri::async_runtime::spawn(async move {
        let mut takt = tokio::time::interval(TAKT);
        takt.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Landeprotokoll wie panel.js: genau einmal je PIREP, auf der Kante
        // zu `ergebnis`; bleibt bis der Flug endet.
        let mut debrief: Option<(String, Value)> = None;
        loop {
            takt.tick().await;
            if !slot.bereit() {
                continue;
            }
            let frame = baue(&app, &mut debrief);
            slot.setze(frame);
        }
    });
}

fn baue(app: &AppHandle, debrief: &mut Option<(String, Value)>) -> sim_xplane::hud_band::BandFrame {
    let mut status = crate::remote::current_flight_status_value(app);
    crate::panel_server::with_display_callsign(&mut status);
    let status_da = !status.is_null();
    if !status_da {
        *debrief = None;
    }
    let pirep = status
        .get("pirep_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    if status_da
        && lage(true, Some(&status)) == Lage::Ergebnis
        && debrief.as_ref().map(|(p, _)| p) != Some(&pirep)
    {
        let state = app.state::<crate::AppState>();
        let rec = crate::landing_get_current(app.clone(), state);
        if let Ok(v) = serde_json::to_value(rec) {
            if !v.is_null() {
                *debrief = Some((pirep, crate::panel_server::debrief_fuer_anzeige(v)));
            }
        }
    }
    let akt = {
        let state = app.state::<crate::AppState>();
        crate::activity_log_tail(&state, 1)
            .first()
            .and_then(|e| serde_json::to_value(e).ok())
    };
    baue_band(&BandEingabe {
        verbunden: true,
        status: Some(&status),
        debrief: debrief.as_ref().map(|(_, v)| v),
        aktivitaet: akt.as_ref(),
        jetzt_ms: chrono::Utc::now().timestamp_millis(),
        host: "127.0.0.1",
        port: crate::panel_server::PANEL_SERVER_PORT,
        fehler_text: None,
        fehler_seit_ms: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn einstellung_standard_an_und_rundlauf() {
        let dir = std::env::temp_dir().join(format!("aa-xpband-cfg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(CONFIG_FILE);
        assert!(read_enabled_from(&path), "fehlende Datei = an");
        write_enabled_to(&path, false).unwrap();
        assert!(!read_enabled_from(&path));
        write_enabled_to(&path, true).unwrap();
        assert!(read_enabled_from(&path));
        std::fs::write(&path, b"kaputt{{{").unwrap();
        assert!(read_enabled_from(&path), "kaputte Datei = an");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
