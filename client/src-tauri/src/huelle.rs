//! v3 (Etappe 5, P-8.01/P-8.10): die eingebaute Huelle.
//!
//! Die nachgeladene Oberflaeche (Capability `ui`) darf Befehle der Stufe
//! „system“ (befehle.json) nicht selbst aufrufen. Sie stoesst sie ueber
//! `shell_request(befehl, werte)` an: der Kern legt einen Auftrag ab und
//! oeffnet das Fenster `huelle` (eingebautes Frontend, Capability `huelle`).
//! Dort bestaetigt der Pilot; die Huelle fuehrt den Befehl selbst aus und
//! meldet das Ergebnis mit `shell_auftrag_beantworten`, das die wartende
//! `shell_request` aufloest — mit genau dem Ergebnis oder Fehler des Befehls.
//! Fenster zu ohne Antwort = abgelehnt.
//!
//! Ausserdem besitzt die Huelle die Einstellungen, die die Oberflaeche bisher
//! bei jedem Start aus ihrem localStorage hochschob (Einwilligung
//! Fehlerberichte, Tray, automatisches Loeschen alter Flugprotokolle): Datei
//! `huelle-einstellungen.json`, angewandt beim Start, lesbar ueber
//! `huelle_einstellungen`. Sonst kaeme jeder Start mit Bestaetigungsdialogen.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

use crate::UiError;

/// Label des Huelle-Fensters (capabilities/huelle.json).
pub(crate) const FENSTER: &str = "huelle";

/// Tage fuer das automatische Loeschen alter Flugprotokolle (wie bisher
/// `AUTO_DELETE_LOGS_DAYS` in der Oberflaeche).
pub(crate) const FLUGPROTOKOLLE_TAGE: u32 = 30;

/// Stufe eines Befehls laut befehle.json (eine Quelle mit build.rs).
pub(crate) fn stufe(befehl: &str) -> Option<&'static str> {
    static STUFEN: OnceLock<BTreeMap<String, String>> = OnceLock::new();
    STUFEN
        .get_or_init(|| {
            let v: Value = serde_json::from_str(include_str!("../befehle.json")).expect("befehle.json");
            serde_json::from_value(v["befehle"].clone()).expect("befehle.json: befehle")
        })
        .get(befehl)
        .map(String::as_str)
}

/// Ein offener Auftrag, wie die Huelle ihn anzeigt.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct Auftrag {
    pub id: u64,
    pub befehl: String,
    pub werte: Value,
}

type Antwort = Result<Value, UiError>;

struct Auftraege {
    naechste: u64,
    offen: BTreeMap<u64, (Auftrag, oneshot::Sender<Antwort>)>,
}

static AUFTRAEGE: Mutex<Auftraege> = Mutex::new(Auftraege { naechste: 1, offen: BTreeMap::new() });

fn abgelehnt() -> UiError {
    UiError::new("huelle_abgelehnt", "In der Huelle abgelehnt oder Fenster geschlossen")
}

/// Legt einen Auftrag ab — nur fuer Befehle der Stufe „system“; alles andere
/// ruft die Oberflaeche selbst auf.
fn ablegen(befehl: &str, werte: Value) -> Result<(u64, oneshot::Receiver<Antwort>), UiError> {
    if stufe(befehl) != Some("system") {
        return Err(UiError::new("huelle_kein_systembefehl", format!("{befehl} ist kein Systembefehl")));
    }
    let (tx, rx) = oneshot::channel();
    let mut a = AUFTRAEGE.lock().expect("Auftraege");
    let id = a.naechste;
    a.naechste += 1;
    a.offen.insert(id, (Auftrag { id, befehl: befehl.to_string(), werte }, tx));
    Ok((id, rx))
}

fn offene() -> Vec<Auftrag> {
    AUFTRAEGE.lock().expect("Auftraege").offen.values().map(|(a, _)| a.clone()).collect()
}

fn erledigen(id: u64, antwort: Antwort) -> bool {
    let eintrag = AUFTRAEGE.lock().expect("Auftraege").offen.remove(&id);
    match eintrag {
        Some((_, tx)) => {
            let _ = tx.send(antwort);
            true
        }
        None => false,
    }
}

/// Huelle geschlossen: alle offenen Auftraege gelten als abgelehnt.
fn alle_ablehnen() {
    let offen = std::mem::take(&mut AUFTRAEGE.lock().expect("Auftraege").offen);
    for (_, (_, tx)) in offen {
        let _ = tx.send(Err(abgelehnt()));
    }
}

/// Fehler des ausgefuehrten Befehls so zurueckgeben, wie die Oberflaeche ihn
/// vom direkten Aufruf kennt (`{code, message, details}` oder Text).
fn fehler_aus(v: Value) -> UiError {
    match v {
        Value::Object(ref o) if o.contains_key("code") => {
            let code = o.get("code").and_then(Value::as_str).unwrap_or("fehler").to_string();
            let message = o.get("message").and_then(Value::as_str).unwrap_or("").to_string();
            match o.get("details").cloned() {
                Some(d) => UiError::new(code, message).with_details(d),
                None => UiError::new(code, message),
            }
        }
        Value::String(s) => UiError::new("fehler", s),
        andere => UiError::new("fehler", andere.to_string()),
    }
}

fn fenster_zeigen(app: &AppHandle) -> Result<(), UiError> {
    if let Some(w) = app.get_webview_window(FENSTER) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit_to(FENSTER, "huelle-auftrag", ());
        return Ok(());
    }
    let w = tauri::WebviewWindowBuilder::new(app, FENSTER, tauri::WebviewUrl::App("index.html?fenster=huelle".into()))
        .title("AeroACARS — Bestätigung")
        .inner_size(560.0, 440.0)
        .resizable(true)
        .always_on_top(true)
        .center()
        .build()
        .map_err(|e| UiError::new("window_failed", format!("Fenster ließ sich nicht öffnen: {e}")))?;
    let app = app.clone();
    w.on_window_event(move |e| {
        if let tauri::WindowEvent::Destroyed = e {
            match nach_dem_schliessen(SELBST_GESCHLOSSEN.swap(false, Ordering::SeqCst), !offene().is_empty()) {
                Nachher::Ablehnen => alle_ablehnen(),
                Nachher::NeuOeffnen => {
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if fenster_zeigen(&app).is_err() {
                            alle_ablehnen();
                        }
                    });
                }
                Nachher::Nichts => {}
            }
        }
    });
    Ok(())
}

/// Die Huelle hat sich selbst geschlossen (nichts mehr offen) — nicht der Pilot.
static SELBST_GESCHLOSSEN: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq)]
enum Nachher {
    Ablehnen,
    NeuOeffnen,
    Nichts,
}

/// Fenster zu: hat der Pilot es geschlossen, gilt alles Offene als abgelehnt.
/// Hat die Huelle sich selbst geschlossen und kam inzwischen ein neuer Auftrag
/// (Codex 30.10.: Wettlauf zwischen Leer-Pruefung und Schliessen), geht sie
/// wieder auf, statt einen Auftrag abzulehnen, den niemand gesehen hat.
fn nach_dem_schliessen(selbst: bool, offen: bool) -> Nachher {
    match (selbst, offen) {
        (false, _) => Nachher::Ablehnen,
        (true, true) => Nachher::NeuOeffnen,
        (true, false) => Nachher::Nichts,
    }
}

/// Systembefehl aus der Oberflaeche anstossen (P-8.10): Bestaetigung in der
/// Huelle, Ergebnis wie beim direkten Aufruf.
#[tauri::command]
pub(crate) async fn shell_request(app: AppHandle, befehl: String, werte: Option<Value>) -> Result<Value, UiError> {
    let (id, rx) = ablegen(&befehl, werte.unwrap_or_else(|| Value::Object(Default::default())))?;
    tracing::info!(id, befehl = %befehl, "Huelle: Bestaetigung angefragt");
    if let Err(e) = fenster_zeigen(&app) {
        erledigen(id, Err(abgelehnt()));
        return Err(e);
    }
    rx.await.unwrap_or_else(|_| Err(abgelehnt()))
}

/// Offene Auftraege (nur die Huelle).
#[tauri::command]
pub(crate) fn shell_auftraege() -> Vec<Auftrag> {
    offene()
}

/// Antwort der Huelle: `ok` mit dem Ergebnis des Befehls, sonst `fehler`
/// (fehlt er, gilt der Auftrag als abgelehnt).
#[tauri::command]
pub(crate) fn shell_auftrag_beantworten(
    app: AppHandle,
    id: u64,
    ok: bool,
    ergebnis: Option<Value>,
    fehler: Option<Value>,
) -> bool {
    let erledigt = beantworten(id, ok, ergebnis, fehler);
    // Nichts mehr offen: Huelle schliessen.
    if offene().is_empty() {
        if let Some(w) = app.get_webview_window(FENSTER) {
            SELBST_GESCHLOSSEN.store(true, Ordering::SeqCst);
            let _ = w.close();
        }
    }
    erledigt
}

fn beantworten(id: u64, ok: bool, ergebnis: Option<Value>, fehler: Option<Value>) -> bool {
    let antwort = if ok {
        Ok(ergebnis.unwrap_or(Value::Null))
    } else {
        Err(fehler.map(fehler_aus).unwrap_or_else(abgelehnt))
    };
    tracing::info!(id, ok, "Huelle: Auftrag beantwortet");
    erledigen(id, antwort)
}

/// Update installieren und neu starten (P-8.01: Update-Dialog in der Huelle;
/// updater/process nur hier). Holt den neuesten Stand selbst und meldet den
/// Fortschritt als Ereignis `update-fortschritt` in derselben Form wie das
/// JS-Plugin (`Started`/`Progress`/`Finished`), damit die Anzeige bleibt.
#[tauri::command]
pub(crate) async fn update_installieren(app: AppHandle) -> Result<(), UiError> {
    use tauri_plugin_updater::UpdaterExt;
    let fehler = |e: tauri_plugin_updater::Error| UiError::new("update_failed", e.to_string());
    let update = app
        .updater()
        .map_err(fehler)?
        .check()
        .await
        .map_err(fehler)?
        .ok_or_else(|| UiError::new("update_none", "Kein Update verfuegbar"))?;
    tracing::info!(version = %update.version, "Huelle: Update wird installiert");
    let mut gestartet = false;
    update
        .download_and_install(
            |stueck, gesamt| {
                if !gestartet {
                    gestartet = true;
                    let _ = app.emit("update-fortschritt", serde_json::json!({"event": "Started", "data": {"contentLength": gesamt}}));
                }
                let _ = app.emit("update-fortschritt", serde_json::json!({"event": "Progress", "data": {"chunkLength": stueck}}));
            },
            || {
                let _ = app.emit("update-fortschritt", serde_json::json!({"event": "Finished"}));
            },
        )
        .await
        .map_err(fehler)?;
    app.restart()
}

/// Einstellungen der Huelle (frueher localStorage der Oberflaeche).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Einstellungen {
    #[serde(default)]
    pub fehlerberichte: bool,
    #[serde(default)]
    pub minimize_to_tray: bool,
    /// Wie bisher in der Oberflaeche: von Haus aus an.
    #[serde(default = "an")]
    pub flugprotokolle_auto_loeschen: bool,
}

fn an() -> bool {
    true
}

impl Default for Einstellungen {
    fn default() -> Self {
        Self { fehlerberichte: false, minimize_to_tray: false, flugprotokolle_auto_loeschen: true }
    }
}

fn pfad(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|p| p.join("huelle-einstellungen.json"))
}

pub(crate) fn einstellungen(app: &AppHandle) -> Einstellungen {
    pfad(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| erste_einstellungen(hat_flugprotokolle(app)))
}

/// Ohne Datei (erster Start mit Huelle): Die alten Schalter lagen im
/// localStorage der Oberflaeche, den `aeroui://` nicht mehr sieht. Die
/// Loeschautomatik ist darum nur auf einer frischen Installation an (wie 2.x);
/// liegen schon Flugprotokolle da, bleibt sie aus, bis der Pilot sie
/// bestaetigt — sonst loeschte das Update Protokolle eines Piloten, der sie
/// abgeschaltet hatte (Codex 30.10.).
fn erste_einstellungen(hat_flugprotokolle: bool) -> Einstellungen {
    Einstellungen { flugprotokolle_auto_loeschen: !hat_flugprotokolle, ..Einstellungen::default() }
}

fn hat_flugprotokolle(app: &AppHandle) -> bool {
    app.path()
        .app_data_dir()
        .ok()
        .and_then(|d| std::fs::read_dir(d.join("flight_logs")).ok())
        .is_some_and(|mut e| e.next().is_some())
}

/// Aendert eine Einstellung und schreibt die Datei (aus dem jeweiligen
/// Systembefehl heraus, also erst nach Bestaetigung in der Huelle).
pub(crate) fn einstellung_setzen(app: &AppHandle, aendern: impl FnOnce(&mut Einstellungen)) {
    let mut e = einstellungen(app);
    aendern(&mut e);
    let Some(p) = pfad(app) else { return };
    if let Some(ordner) = p.parent() {
        let _ = std::fs::create_dir_all(ordner);
    }
    if let Err(err) = std::fs::write(&p, serde_json::to_vec_pretty(&e).expect("Einstellungen")) {
        tracing::warn!(error = %err, "Huelle: Einstellungen nicht speicherbar");
    }
}

/// Lesend: die Oberflaeche zeigt die Schalter danach an.
#[tauri::command]
pub(crate) fn huelle_einstellungen(app: AppHandle) -> Einstellungen {
    einstellungen(&app)
}

/// Automatisches Loeschen alter Flugprotokolle beim Start ein/aus.
/// Einschalten loescht sofort einmal (wie bisher der erste Start danach).
#[tauri::command]
pub(crate) fn flight_logs_auto_purge_set(app: AppHandle, enabled: bool) -> Result<bool, UiError> {
    einstellung_setzen(&app, |e| e.flugprotokolle_auto_loeschen = enabled);
    if enabled {
        alte_flugprotokolle_loeschen(&app);
    }
    Ok(enabled)
}

fn alte_flugprotokolle_loeschen(app: &AppHandle) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    // P-7.21: noch nicht hochgeladene Protokolle bleiben stehen.
    let behalten = crate::upload_warteschlange::offene_protokolle(&dir);
    match recorder::flight_logs_purge_older_than_ausser(dir, FLUGPROTOKOLLE_TAGE, &behalten) {
        Ok(n) if n > 0 => tracing::info!(deleted = n, days = FLUGPROTOKOLLE_TAGE, "flight logs purged (auto)"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "flight logs purge failed"),
    }
}

/// Beim Start (`setup`): Einwilligung, Tray und Loeschautomatik anwenden —
/// ohne dass die Oberflaeche etwas hochschieben muss.
pub(crate) fn beim_start_anwenden(app: &AppHandle) {
    let e = einstellungen(app);
    // Erster Start: die eben bestimmten Werte festhalten, damit sie nicht mit
    // den ersten eigenen Protokollen kippen.
    if pfad(app).is_some_and(|p| !p.exists()) {
        einstellung_setzen(app, |x| *x = e.clone());
    }
    crate::sentry_init::set_consent(e.fehlerberichte);
    app.state::<crate::AppState>()
        .minimize_to_tray_enabled
        .store(e.minimize_to_tray, std::sync::atomic::Ordering::Relaxed);
    if e.flugprotokolle_auto_loeschen {
        let app = app.clone();
        std::thread::spawn(move || alte_flugprotokolle_loeschen(&app));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Auftraege sind ein gemeinsamer Zustand: Tests, die ihn leeren,
    /// laufen nicht neben anderen.
    static SPERRE: Mutex<()> = Mutex::new(());

    #[test]
    fn nur_systembefehle_gehen_ueber_die_huelle() {
        assert_eq!(stufe("flight_logs_delete_all"), Some("system"));
        // 30.10.: umgestuft — die Huelle nimmt sie nicht mehr an.
        assert_eq!(stufe("phpvms_login"), Some("fachlich"));
        assert_eq!(ablegen("phpvms_login", Value::Null).unwrap_err().code, "huelle_kein_systembefehl");
        assert_eq!(stufe("shell_request"), Some("fachlich"));
        let e = ablegen("app_info", Value::Null).unwrap_err();
        assert_eq!(e.code, "huelle_kein_systembefehl");
        assert!(ablegen("gibt_es_nicht", Value::Null).is_err());
        // Die Huelle-eigenen Befehle selbst sind system, aber nie Ziel eines Auftrags
        // ueber die Oberflaeche hinaus: sie laufen nur im Fenster huelle.
        assert_eq!(stufe("shell_auftraege"), Some("system"));
        assert_eq!(stufe("shell_auftrag_beantworten"), Some("system"));
    }

    #[test]
    fn auftrag_wird_mit_ergebnis_oder_fehler_aufgeloest() {
        let _s = SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let (id, rx) = ablegen("activity_log_clear", serde_json::json!({})).unwrap();
        assert!(offene().iter().any(|a| a.id == id && a.befehl == "activity_log_clear"));
        assert!(beantworten(id, true, Some(serde_json::json!(3)), None));
        assert_eq!(rt.block_on(rx).unwrap().unwrap(), serde_json::json!(3));
        assert!(!offene().iter().any(|a| a.id == id));
        // zweite Antwort auf denselben Auftrag: nichts mehr offen
        assert!(!beantworten(id, true, None, None));

        let (id, rx) = ablegen("hoppie_clear_logon_code", serde_json::json!({})).unwrap();
        let f = serde_json::json!({"code": "flight_active", "message": "Flug laeuft"});
        assert!(beantworten(id, false, None, Some(f)));
        let e = rt.block_on(rx).unwrap().unwrap_err();
        assert_eq!((e.code.as_str(), e.message.as_str()), ("flight_active", "Flug laeuft"));

        let (id, rx) = ablegen("landing_delete", serde_json::json!({})).unwrap();
        assert!(beantworten(id, false, None, None));
        assert_eq!(rt.block_on(rx).unwrap().unwrap_err().code, "huelle_abgelehnt");
    }

    #[test]
    fn geschlossene_huelle_lehnt_alles_ab() {
        let _s = SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let (_, rx) = ablegen("remote_server_start", serde_json::json!({})).unwrap();
        alle_ablehnen();
        assert_eq!(rt.block_on(rx).unwrap().unwrap_err().code, "huelle_abgelehnt");
    }

    #[test]
    fn huelle_geht_wieder_auf_statt_ungesehen_abzulehnen() {
        assert_eq!(nach_dem_schliessen(false, true), Nachher::Ablehnen);
        assert_eq!(nach_dem_schliessen(false, false), Nachher::Ablehnen);
        assert_eq!(nach_dem_schliessen(true, true), Nachher::NeuOeffnen);
        assert_eq!(nach_dem_schliessen(true, false), Nachher::Nichts);
    }

    #[test]
    fn loeschautomatik_beim_ersten_start_nur_ohne_protokolle() {
        assert!(erste_einstellungen(false).flugprotokolle_auto_loeschen);
        assert!(!erste_einstellungen(true).flugprotokolle_auto_loeschen);
        assert!(!erste_einstellungen(true).fehlerberichte);
    }

    #[test]
    fn einstellungen_fehlende_felder_wie_bisher() {
        // Einwilligung und Tray von Haus aus aus, Loeschautomatik an (wie 2.x).
        let e: Einstellungen = serde_json::from_str("{\"minimize_to_tray\": true}").unwrap();
        assert_eq!(e, Einstellungen { minimize_to_tray: true, fehlerberichte: false, flugprotokolle_auto_loeschen: true });
        assert_eq!(serde_json::from_str::<Einstellungen>("{}").unwrap(), Einstellungen::default());
    }
}
