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
    // Shared Cockpit (B4): beim Partner die Landung des Gastgebers, nie die eigene.
    let sc = crate::panel_server::sc_anzeige_fuer_status(app, &status);
    let (sc_debrief, sc_text) = sc_vorbereiten(&mut status, &sc);
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
        && sc_debrief.is_none()
        && lage(true, Some(&status)) == Lage::Ergebnis
        && debrief.as_ref().map(|(p, _)| p) != Some(&pirep)
    {
        let state = app.state::<crate::AppState>();
        let beginn = std::time::Instant::now();
        let rec = crate::landing_get_current(app.clone(), state);
        crate::landungsabfrage_protokollieren("xplane_band", rec.as_ref(), beginn.elapsed());
        if let Ok(v) = serde_json::to_value(rec) {
            if !v.is_null() {
                *debrief = Some((pirep, crate::panel_server::debrief_fuer_anzeige(v)));
            }
        }
    }
    let akt = {
        let state = app.state::<crate::AppState>();
        let log = state.activity_log.lock().expect("activity_log lock");
        sc_aktivitaet(log.iter().rev(), &sc)
    };
    baue_band(&BandEingabe {
        verbunden: true,
        status: Some(&status),
        debrief: debrief_waehlen(&sc_debrief, debrief.as_ref().map(|(_, v)| v)),
        aktivitaet: akt.as_ref(),
        jetzt_ms: chrono::Utc::now().timestamp_millis(),
        host: "127.0.0.1",
        port: crate::panel_server::PANEL_SERVER_PORT,
        fehler_text: None,
        fehler_seit_ms: None,
        sc_text: sc_text.as_deref(),
    })
}

/// Shared Cockpit: Status umstellen, Landeprotokoll und Text fuer das Band.
/// `None` beim Protokoll = wie bisher (eigene Landung).
fn sc_vorbereiten(status: &mut Value, sc: &crate::landungs_eingabe::ScAnzeige) -> (Option<Value>, Option<String>) {
    crate::panel_server::sc_status_fuer_anzeige(status, sc);
    (crate::panel_server::sc_debrief(sc, false), sc_band_text(sc))
}

/// Juengste Aktivitaet fuer Zeile 1 (beim Partner ohne die eigene Aufsetz-Zeile).
fn sc_aktivitaet<'a>(
    neueste_zuerst: impl Iterator<Item = &'a crate::ActivityEntry>,
    sc: &crate::landungs_eingabe::ScAnzeige,
) -> Option<Value> {
    crate::aktivitaet_fuer_anzeige(neueste_zuerst, 1, crate::panel_server::sc_partner(sc))
        .first()
        .and_then(|e| serde_json::to_value(e).ok())
}

/// Beim Partner nie das eigene Protokoll; `null` heisst „noch keins“.
fn debrief_waehlen<'a>(sc: &'a Option<Value>, eigen: Option<&'a Value>) -> Option<&'a Value> {
    match sc {
        None => eigen,
        Some(v) => Some(v).filter(|v| !v.is_null()),
    }
}

/// Text fuer das Band beim Partner: „folgt“ bzw. das Kennzeichen.
fn sc_band_text(sc: &crate::landungs_eingabe::ScAnzeige) -> Option<String> {
    use crate::landungs_eingabe::{ScAnzeige, ScLandung};
    match sc {
        ScAnzeige::Folgt { .. } => Some(crate::panel_server::SC_FOLGT.into()),
        ScAnzeige::NoteFolgt(_) => Some(crate::panel_server::SC_NOTE_FOLGT.into()),
        ScAnzeige::Gelandet(ScLandung::Gelandet { gastgeber_name, .. }) => {
            Some(crate::panel_server::sc_kennzeichen(gastgeber_name.as_deref()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landungs_eingabe::{ScAnzeige, ScLandung, ScRolle};
    use serde_json::json;

    /// Dieselbe Kette wie `baue`, ohne App: Status → SC-Umstellung → Band.
    fn band(status: &Value, sc: &ScAnzeige, eigen: Option<&Value>) -> (Lage, String) {
        let (lage, _, text) = band_mit_log(status, sc, eigen, &[]);
        (lage, text)
    }

    /// Wie `band`, dazu das Aktivitaetslog (aelteste zuerst) fuer Zeile 1.
    fn band_mit_log(status: &Value, sc: &ScAnzeige, eigen: Option<&Value>, log: &[Value]) -> (Lage, String, String) {
        let mut status = status.clone();
        let (sc_debrief, sc_text) = sc_vorbereiten(&mut status, sc);
        let log: Vec<crate::ActivityEntry> = log.iter().map(|v| serde_json::from_value(v.clone()).unwrap()).collect();
        let akt = sc_aktivitaet(log.iter().rev(), sc);
        let f = baue_band(&BandEingabe {
            verbunden: true,
            status: Some(&status),
            debrief: debrief_waehlen(&sc_debrief, eigen),
            aktivitaet: akt.as_ref(),
            jetzt_ms: 0,
            host: "127.0.0.1",
            port: 1,
            fehler_text: None,
            fehler_seit_ms: None,
            sc_text: sc_text.as_deref(),
        });
        (f.lage, f.zeilen[0].zu_text(), f.zeilen[1].zu_text())
    }

    fn eintrag(minute: u32, message: &str, schluessel: &str) -> Value {
        json!({"timestamp": format!("2026-11-13T10:{minute:02}:00Z"), "level": "info", "message": message,
            "text": {"zeile": {"schluessel": schluessel, "werte": {}}, "detail": []}})
    }

    /// Befund 4: beim Partner steht in Zeile 1 (Ticker) nicht die eigene
    /// Aufsetz-Zeile, sondern die naechstaeltere. Gegenprobe Solo: wie bisher.
    #[test]
    fn partner_ticker_ohne_eigene_aufsetz_zeile() {
        let log = [
            eintrag(0, "Phase: Landung", "activity.lage.am_boden"),
            eintrag(1, "Touchdown: V/S -512 fpm, 1.60 G", "activity.touchdown.line"),
        ];
        let (_, ticker, _) = band_mit_log(&nach_dem_aufsetzen(), &ScAnzeige::Gelandet(gelandet()), Some(&eigene()), &log);
        assert!(ticker.contains("Phase: Landung") && !ticker.contains("512"), "{ticker}");
        let folgt = ScAnzeige::Folgt { name: None, halten: true };
        let (_, ticker, _) = band_mit_log(&nach_dem_aufsetzen(), &folgt, Some(&eigene()), &log);
        assert!(!ticker.contains("512"), "{ticker}");
        let ohne_g = [eintrag(1, "Touchdown: V/S -512 fpm, G nicht gemessen", "activity.touchdown.ohne_g")];
        let (_, ticker, _) = band_mit_log(&nach_dem_aufsetzen(), &folgt, Some(&eigene()), &ohne_g);
        assert!(!ticker.contains("512"), "{ticker}");
        let (_, ticker, _) = band_mit_log(&nach_dem_aufsetzen(), &ScAnzeige::WieBisher, Some(&eigene()), &log);
        assert!(ticker.contains("Touchdown: V/S -512"), "Solo: {ticker}");
    }

    /// Befund 5: ab `blocks_on` die normale Lage, das SC-Kennzeichen bzw.
    /// „folgt“ als Anhang. Direkt nach dem Aufsetzen wird gehalten; in
    /// `taxi_in` mit fertiger Landung zeigt das Band „Rollen“ (das MSFS-Panel
    /// holt dort kein Protokoll mehr, siehe Bericht).
    #[test]
    fn partner_am_stand_normale_lage_mit_anhang() {
        let mit_phase = |p: &str| {
            let mut s = nach_dem_aufsetzen();
            s["phase"] = json!(p);
            s
        };
        let folgt = ScAnzeige::Folgt { name: None, halten: true };
        let (lage, text) = band(&mit_phase("blocks_on"), &folgt, Some(&eigene()));
        assert_eq!(lage, Lage::AmStand);
        assert!(text.contains("Am Stand") && text.contains("Landung des Gastgebers folgt"), "{text}");
        let (lage, text) = band(&mit_phase("pirep_submitted"), &ScAnzeige::Gelandet(gelandet()), Some(&eigene()));
        assert_eq!(lage, Lage::Eingereicht);
        assert!(text.contains("SC - Landung von Juergen K."), "{text}");
        let (lage, _) = band(&mit_phase("taxi_in"), &folgt, Some(&eigene()));
        assert_eq!(lage, Lage::Auswertung);
        let (lage, text) = band(&mit_phase("taxi_in"), &ScAnzeige::Gelandet(gelandet()), Some(&eigene()));
        assert_eq!(lage, Lage::Rollen);
        assert!(text.contains("91") && !text.contains("512"), "{text}");
        // Gegenprobe Solo am Stand: kein Anhang
        let (_, text) = band(&mit_phase("blocks_on"), &ScAnzeige::WieBisher, Some(&eigene()));
        assert!(!text.contains("SC -") && !text.contains("Gastgeber"), "{text}");
    }

    /// Rate von live `null` (Webapp zeigt keine): weder 0 noch die eigene
    /// Rate, weder im Band noch in `/panel/status` (Stream Deck).
    #[test]
    fn partner_ohne_rate_des_gastgebers() {
        let mut ohne = gelandet();
        if let ScLandung::Gelandet { landing_rate_fpm, .. } = &mut ohne {
            *landing_rate_fpm = None;
        }
        let a = ScAnzeige::Gelandet(ohne);
        let (lage, text) = band(&nach_dem_aufsetzen(), &a, Some(&eigene()));
        assert_eq!(lage, Lage::Ergebnis);
        assert!(text.contains("91") && text.contains("SC - Landung von"), "{text}");
        assert!(!text.contains("512") && !text.contains("Rate\tnv0"), "{text}");
        let mut s = nach_dem_aufsetzen();
        crate::panel_server::sc_status_fuer_anzeige(&mut s, &a);
        assert!(s["landing_rate_fpm"].is_null());
        assert!(crate::panel_server::sc_debrief(&a, true).unwrap()["landing_rate_fpm"].is_null());
    }

    /// Gastgeber gelandet, Note fehlt noch: halten mit eigenem Text.
    #[test]
    fn partner_wartet_auf_die_note_im_band() {
        let mut ohne = gelandet();
        if let ScLandung::Gelandet { landing_score, .. } = &mut ohne {
            *landing_score = None;
        }
        let (lage, text) = band(&nach_dem_aufsetzen(), &ScAnzeige::NoteFolgt(ohne), Some(&eigene()));
        assert_eq!(lage, Lage::Auswertung);
        assert!(text.contains("Bewertung des Gastgebers folgt") && !text.contains("HARD"), "{text}");
    }

    fn gelandet() -> ScLandung {
        ScLandung::Gelandet {
            touchdown_id: 1,
            fuehrender_pirep_id: "H1".into(),
            gastgeber_name: Some("J\u{00FC}rgen K.".into()),
            rolle_gastgeber: Some(ScRolle::PF),
            landing_rate_fpm: Some(-180.0),
            landing_score: Some(91.0),
            g_force: Some(1.12),
            bounce_count: Some(0),
            score_text: Some("firm".into()),
        }
    }

    fn nach_dem_aufsetzen() -> Value {
        json!({"pirep_id":"P1","phase":"landing","landing_score_finalized":true,"landing_at":"2026-11-13T10:00:00Z",
            "landing_rate_fpm":-512.0,"landing_g_force":1.6,"callsign":"GSG 1"})
    }

    fn eigene() -> Value {
        json!({"score_numeric":40,"score_label":"hard","landing_rate_fpm":-512.0,"landing_scored_g_force":1.6,
            "bounce_count":2,"runway_match":{"runway_ident":"25C"}})
    }

    /// Partner, Gastgeber gelandet: Werte und Kategorie des Gastgebers, das
    /// Kennzeichen als Anhang, nichts von der eigenen Messung (-512, 25C).
    #[test]
    fn partner_sieht_die_landung_des_gastgebers_im_band() {
        let (lage, text) = band(&nach_dem_aufsetzen(), &ScAnzeige::Gelandet(gelandet()), Some(&eigene()));
        assert_eq!(lage, Lage::Ergebnis);
        assert!(text.contains("91"), "{text}");
        assert!(text.contains("FIRM"), "{text}");
        assert!(text.contains("-180"), "{text}");
        assert!(text.contains("SC - Landung von Juergen K."), "{text}");
        assert!(!text.contains("512") && !text.contains("25C") && !text.contains("HARD"), "eigene Messung im Band: {text}");
        assert!(text.chars().all(|c| c == '\t' || (' '..='~').contains(&c)), "nur ASCII: {text}");
    }

    /// Partner, Gastgeber noch nicht gelandet: „folgt“ statt einer Note.
    #[test]
    fn partner_wartet_auf_den_gastgeber_im_band() {
        let folgt = ScAnzeige::Folgt { name: Some("Anna".into()), halten: true };
        let (lage, text) = band(&nach_dem_aufsetzen(), &folgt, Some(&eigene()));
        assert_eq!(lage, Lage::Auswertung);
        assert!(text.contains("Landung des Gastgebers folgt"), "{text}");
        assert!(!text.contains("40") && !text.contains("HARD"), "{text}");
        // nach der Hoechstzahl: nicht mehr angehalten, aber weiter keine eigene Note
        let (lage, text) = band(&nach_dem_aufsetzen(), &ScAnzeige::Folgt { name: None, halten: false }, Some(&eigene()));
        assert_eq!(lage, Lage::Ergebnis);
        assert!(text.contains("Landung des Gastgebers folgt") && !text.contains("HARD") && !text.contains("512"), "{text}");
        let (lage, text) = band(&nach_dem_aufsetzen(), &ScAnzeige::Warten, Some(&eigene()));
        assert_eq!(lage, Lage::Auswertung);
        assert!(text.contains("Landung wird ausgewertet") && !text.contains("HARD"), "{text}");
    }

    /// Gegenprobe Solo/Nicht-Crew: exakt wie ohne Shared Cockpit.
    #[test]
    fn solo_band_unveraendert() {
        let status = nach_dem_aufsetzen();
        let vorher = baue_band(&BandEingabe {
            verbunden: true,
            status: Some(&status),
            debrief: Some(&eigene()),
            aktivitaet: None,
            jetzt_ms: 0,
            host: "127.0.0.1",
            port: 1,
            fehler_text: None,
            fehler_seit_ms: None,
            sc_text: None,
        });
        let (lage, text) = band(&status, &ScAnzeige::WieBisher, Some(&eigene()));
        assert_eq!(lage, vorher.lage);
        assert_eq!(text, vorher.zeilen[1].zu_text());
        assert!(text.contains("HARD") && text.contains("25C"));
    }

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
