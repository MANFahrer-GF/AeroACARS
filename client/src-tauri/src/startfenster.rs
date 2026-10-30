//! Startfenster mit Fortschrittsbalken (Thomas 30.10.2026: „installieren,
//! schliessen und neu starten ist inakzeptabel“).
//!
//! Das Hauptfenster zeigt die Oberflaeche aus dem Laufzeit-Paket — und die
//! gibt es erst, wenn der Stand fuer diesen Lauf feststeht (ein neuer Stand
//! wird beim Start geholt und gilt sofort, `laufzeit_paket::beim_start_holen`).
//! Darum:
//!
//! 1. `setup` zeigt SOFORT das Startfenster (eingebaute Seite ueber das
//!    eigene Protokoll `aerostart://`, nicht aus dem Paket) und gibt die
//!    Ereignisschleife frei;
//! 2. ein Faden holt den Stand und meldet den Fortschritt ans Startfenster
//!    (`window.aeroStart(...)` per `eval`, kein IPC, keine Capability);
//! 3. ist der Stand festgelegt, entsteht das Hauptfenster (`create: false`
//!    in `tauri.conf.json`, hier aus derselben Konfiguration gebaut), der
//!    Rest von `setup` laeuft (`nach_dem_stand` in lib.rs) und das
//!    Startfenster schliesst.
//!
//! Kein Startfenster: bei offenem Flug (P-3.20), ohne Pruefschluessel oder
//! wenn schon ein Stand gilt — dann entsteht das Hauptfenster sofort.
//! „Ueberspringen“ ist ein Link auf `aerostart://localhost/ueberspringen`,
//! den `on_navigation` abfaengt (kein Befehl, keine Rechte).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{AppHandle, Manager, Runtime, UriSchemeContext, WebviewUrl, WebviewWindowBuilder};

use crate::laufzeit_paket::{self, Abbruch, Fortschritt, Melder, Phase, StartHilfe};

pub(crate) const SCHEMA: &str = "aerostart";
const START: &str = "start";
const HAUPT: &str = "main";
const SEITE: &str = include_str!("startfenster/index.html");
/// Ein Pfad-Abschnitt der Ueberspringen-Adresse.
const UEBERSPRINGEN: &str = "/ueberspringen";
/// Hoechstens so oft schreiben wir den Fortschritt ins Fenster.
const MELDE_ABSTAND: Duration = Duration::from_millis(60);

/// Handler fuer `aerostart://`: nur die eine eingebaute Seite. Eigene CSP
/// (Skript und Stil stehen in der Seite), nichts Externes, keine Verbindungen.
pub(crate) fn protokoll<R: Runtime>(_ctx: UriSchemeContext<'_, R>, _req: Request<Vec<u8>>) -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(
            "Content-Security-Policy",
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'",
        )
        .body(SEITE.as_bytes().to_vec())
        .expect("Antwort")
}

/// Einmalig: der erste Aufruf von `erster` ist `true`, jeder weitere `false`.
/// Das Hauptfenster darf nur ein einziges Mal entstehen.
#[derive(Default)]
struct Einmal(AtomicBool);

impl Einmal {
    fn erster(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }
}

static HAUPTFENSTER_GEBAUT: Einmal = Einmal(AtomicBool::new(false));

/// JavaScript, das einen Stand ins Startfenster schreibt.
fn zustand_js(f: &Fortschritt) -> String {
    let phase = match f.phase {
        Phase::Pruefen => "pruefen",
        Phase::Laden => "laden",
        Phase::Installieren => "installieren",
    };
    let z = serde_json::json!({ "phase": phase, "generation": f.generation, "geladen": f.geladen, "gesamt": f.gesamt });
    format!("window.aeroStart&&window.aeroStart({z})")
}

/// Meldeweg ans Startfenster: drosselt (Phasenwechsel und Ende immer), merkt
/// sich den letzten Stand fuer eine noch ladende Seite.
fn melder<R: Runtime>(app: AppHandle<R>, letzter: Arc<Mutex<Option<Fortschritt>>>) -> Melder {
    let zuletzt = Mutex::new(None::<(Instant, Phase)>);
    Arc::new(move |f: Fortschritt| {
        *letzter.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
        let jetzt = Instant::now();
        let mut z = zuletzt.lock().unwrap_or_else(|e| e.into_inner());
        let senden = match *z {
            Some((t, phase)) => phase != f.phase || f.geladen >= f.gesamt || jetzt.duration_since(t) >= MELDE_ABSTAND,
            None => true,
        };
        if senden {
            *z = Some((jetzt, f.phase));
            if let Some(w) = app.get_webview_window(START) {
                let _ = w.eval(zustand_js(&f));
            }
        }
    })
}

/// Hauptfenster aus `tauri.conf.json` bauen (`create: false`): dieselben
/// Eigenschaften wie frueher, nur erst jetzt. Schon vorhanden = nichts tun.
fn hauptfenster_bauen<R: Runtime>(app: &AppHandle<R>) {
    if !HAUPTFENSTER_GEBAUT.erster() || app.get_webview_window(HAUPT).is_some() {
        return;
    }
    let Some(cfg) = app.config().app.windows.iter().find(|w| w.label == HAUPT).cloned() else {
        tracing::error!("Hauptfenster: nicht in tauri.conf.json");
        return;
    };
    if let Err(e) = WebviewWindowBuilder::from_config(app, &cfg).and_then(|b| b.build()) {
        tracing::error!(error = %e, "Hauptfenster: nicht anlegbar");
    }
}

/// Stand steht fest: Hauptfenster, Rest von `setup`, Startfenster zu.
/// Laeuft auf der Ereignisschleife.
fn abschliessen(app: &AppHandle, daten: Option<&Path>) {
    if let Some(d) = daten {
        // Wirkt nur, wenn der Stand noch nicht feststeht (z. B. ohne Startfenster).
        laufzeit_paket::starten(d);
    }
    hauptfenster_bauen(app);
    crate::nach_dem_stand(app, daten);
    if let Some(w) = app.get_webview_window(START) {
        let _ = w.destroy();
    }
}

/// Aus `setup`: Startfenster zeigen und den Stand im Hintergrund holen, oder
/// — wenn kein Abruf noetig ist — gleich das Hauptfenster bauen.
pub(crate) fn beginnen(app: &AppHandle, daten: Option<PathBuf>) {
    let Some(ordner) = daten.clone().filter(|d| laufzeit_paket::startabruf_noetig(d)) else {
        return abschliessen(app, daten.as_deref());
    };
    let abbruch = Arc::new(Abbruch::default());
    let letzter = Arc::new(Mutex::new(None::<Fortschritt>));
    let fenster = WebviewWindowBuilder::new(app, START, WebviewUrl::CustomProtocol(adresse()))
        .title("AeroACARS 3")
        .inner_size(460.0, 230.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .center()
        .on_navigation({
            let abbruch = abbruch.clone();
            move |url| {
                if url.path() == UEBERSPRINGEN {
                    tracing::info!(laeuft = abbruch.ausloesen(), "Startfenster: Ueberspringen");
                    return false;
                }
                true
            }
        })
        .on_page_load({
            let letzter = letzter.clone();
            move |w, nutzlast| {
                // Kam der Fortschritt vor dem Ende des Ladens, jetzt nachreichen.
                if matches!(nutzlast.event(), tauri::webview::PageLoadEvent::Finished) {
                    if let Some(f) = *letzter.lock().unwrap_or_else(|e| e.into_inner()) {
                        let _ = w.eval(zustand_js(&f));
                    }
                }
            }
        })
        .build();
    if let Err(e) = fenster {
        // Ohne Startfenster wie frueher: Stand holen, dann alles andere.
        tracing::warn!(error = %e, "Startfenster nicht anlegbar, Start ohne Fortschrittsanzeige");
        return abschliessen(app, daten.as_deref());
    }
    let hilfe = StartHilfe { melden: melder(app.clone(), letzter), abbruch };
    let app = app.clone();
    std::thread::spawn(move || {
        laufzeit_paket::starten_mit(&ordner, Some(hilfe));
        let app2 = app.clone();
        if let Err(e) = app.run_on_main_thread(move || abschliessen(&app2, Some(&ordner))) {
            tracing::error!(error = %e, "Start: Ereignisschleife nicht erreichbar");
        }
    });
}

fn adresse() -> tauri::Url {
    format!("{SCHEMA}://localhost/").parse().expect("feste Adresse")
}

/// Fokus fuer einen zweiten Programmstart, solange nur das Startfenster steht.
pub(crate) fn nach_vorn<R: Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window(START) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Das Hauptfenster entsteht genau einmal, egal wie oft der Weg dorthin
    /// durchlaufen wird.
    #[test]
    fn hauptfenster_nur_einmal() {
        let e = Einmal::default();
        assert!(e.erster());
        assert!(!e.erster());
        assert!(!e.erster());
    }

    #[test]
    fn zustand_geht_als_json_ans_fenster() {
        let f = Fortschritt { phase: Phase::Laden, generation: Some(55), geladen: 1024, gesamt: 4096 };
        let js = zustand_js(&f);
        assert!(js.contains(r#""phase":"laden""#) && js.contains(r#""generation":55"#), "{js}");
        assert!(js.contains(r#""geladen":1024"#) && js.contains(r#""gesamt":4096"#), "{js}");
        let p = Fortschritt { phase: Phase::Pruefen, generation: None, geladen: 0, gesamt: 0 };
        assert!(zustand_js(&p).contains(r#""generation":null"#));
    }

    /// Die Seite ist selbststaendig (keine Pakete, kein IPC) und bietet die
    /// vereinbarten Sprachen und den Ueberspringen-Link an.
    #[test]
    fn seite_ist_eingebaut_und_dreisprachig() {
        for s in ["Paket wird geladen", "Loading package", "Caricamento del pacchetto"] {
            assert!(SEITE.contains(s), "{s}");
        }
        assert!(SEITE.contains("aerostart://localhost/ueberspringen"));
        assert!(!SEITE.contains("__TAURI") && !SEITE.contains("aeroui://"));
    }

    /// Der Link der Seite trifft den Pfad, den `on_navigation` abfaengt.
    #[test]
    fn ueberspringen_adresse_passt() {
        let url: tauri::Url = "aerostart://localhost/ueberspringen".parse().unwrap();
        assert_eq!(url.path(), UEBERSPRINGEN);
        assert_eq!(adresse().scheme(), SCHEMA);
    }
}
