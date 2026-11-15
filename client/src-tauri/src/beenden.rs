//! Beenden der App, ohne den Hauptfaden zu blockieren.
//!
//! 3.1.2 (Feld, Windows 3.1.0, 15.11.2026): Tray „Beenden“ → `app.exit(0)`.
//! Der alte `ExitRequested`-Zweig räumte auf dem Hauptfaden auf und wartete
//! in `block_on` auf Hoppie, MQTT und eine 200-ms-Pause. Im Log kamen die
//! MQTT-Stopp-Zeilen, „Herunterfahren im block_on fertig“ nie; danach 2,5 h
//! kein Log, Fenster „Keine Rückmeldung“, der Wächter stumm. Dasselbe Bild
//! am 19.10. und beim macOS-Update 31.10. (`huelle.rs`).
//!
//! Jetzt:
//! 1. Erstes `ExitRequested`: Ende aufhalten, Fenster ausblenden, Aufräumen
//!    in einem eigenen Faden starten, sofort zurück. Der Hauptfaden bleibt frei.
//! 2. Der Faden macht dieselben Schritte wie vorher. Das asynchrone Stück
//!    läuft als Task; gewartet wird mit einer std-Frist, damit auch ein
//!    stehender Tokio-Timer das Beenden nicht aufhält. Danach „fertig“ und
//!    erneut beenden; das zweite `ExitRequested` geht durch.
//! 3. Notbremse: Lebt der Prozess [`NOTBREMSE_NACH`] nach dem ersten
//!    `ExitRequested` noch, Meldung an GlitchTip und hart beenden. Die
//!    Laufmarkierung bleibt dabei liegen (ehrliches „ohne sauberes Ende“).
//!
//! Neustart: `prevent_exit` ist bei `RESTART_EXIT_CODE` wirkungslos (tauri
//! 2.11 `ExitRequestApi::prevent_exit`). Deshalb räumt [`neu_starten`] VOR
//! dem Neustart-Wunsch auf und ruft erst danach `request_restart` (von einem
//! Nebenfaden: `ExitRequested` und `Exit` kommen an, Tauri startet bei
//! `Exit` neu).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

/// Alles zusammen, vom ersten `ExitRequested` bis „fertig“.
pub(crate) const GESAMTFRIST: Duration = Duration::from_secs(20);
/// Danach beendet die Notbremse den Prozess hart.
pub(crate) const NOTBREMSE_NACH: Duration = Duration::from_secs(30);
/// Hoppie-Logoff + MQTT-Stopp bisher (Logoff bis 15 s, Pause 200 ms).
const ASYNC_EINZELFRIST: Duration = Duration::from_millis(15_200);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Laeuft,
    Raeumt,
    Fertig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entscheidung {
    /// Ende aufhalten, Fenster weg, Aufräumen und Notbremse starten.
    AufhaltenUndAufraeumen,
    /// Ende aufhalten — das Aufräumen läuft schon.
    Aufhalten,
    /// Endgültig beenden lassen.
    Durchlassen,
    /// Ein Neustart, der sich nicht aufhalten lässt, kam vor „fertig“.
    /// Durchlassen, aber laut: das Aufräumen fehlt oder ist unvollständig.
    NeustartUngeordnet,
}

/// Reine Zustandslogik für ein `ExitRequested`. `neustart`: der Code ist
/// `RESTART_EXIT_CODE` (Tauri ignoriert dann `prevent_exit`).
pub(crate) fn entscheiden(phase: Phase, neustart: bool) -> Entscheidung {
    match (phase, neustart) {
        (Phase::Fertig, _) => Entscheidung::Durchlassen,
        (_, true) => Entscheidung::NeustartUngeordnet,
        (Phase::Laeuft, false) => Entscheidung::AufhaltenUndAufraeumen,
        (Phase::Raeumt, false) => Entscheidung::Aufhalten,
    }
}

/// Was nach dem Aufräumen passiert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ende {
    Neustart,
    Beenden(i32),
}

pub(crate) fn ende_fuer(code: Option<i32>) -> Ende {
    match code {
        Some(tauri::RESTART_EXIT_CODE) => Ende::Neustart,
        Some(c) => Ende::Beenden(c),
        None => Ende::Beenden(0),
    }
}

/// Frist eines Schritts: seine eigene, höchstens der Rest der Gesamtfrist.
pub(crate) fn schrittfrist(einzel: Duration, vergangen: Duration) -> Duration {
    einzel.min(GESAMTFRIST.saturating_sub(vergangen))
}

/// Die Notbremse greift erst, wenn der Prozess so lange nach dem ersten
/// `ExitRequested` noch lebt.
pub(crate) fn notbremse_faellig(seit_erstem_wunsch: Duration) -> bool {
    seit_erstem_wunsch >= NOTBREMSE_NACH
}

struct Zustand {
    phase: Phase,
    code: Option<i32>,
}

static ZUSTAND: Mutex<Zustand> = Mutex::new(Zustand {
    phase: Phase::Laeuft,
    code: None,
});

fn zustand() -> std::sync::MutexGuard<'static, Zustand> {
    ZUSTAND.lock().unwrap_or_else(|e| e.into_inner())
}

/// Aus dem `.run`-Callback für `RunEvent::ExitRequested`. Kehrt sofort
/// zurück — hier wird nie gewartet.
pub(crate) fn exit_requested(app: &AppHandle, code: Option<i32>, api: &tauri::ExitRequestApi) {
    let neustart = code == Some(tauri::RESTART_EXIT_CODE);
    let mut z = zustand();
    match entscheiden(z.phase, neustart) {
        Entscheidung::AufhaltenUndAufraeumen => {
            if !aufraeumen_starten(app, code) {
                // Ohne Faden kein geordnetes Ende — dann lieber sofort.
                drop(z);
                durchlassen();
                return;
            }
            api.prevent_exit();
            z.phase = Phase::Raeumt;
            z.code = code;
            drop(z);
            fenster_ausblenden(app);
            tracing::info!(?code, "Beenden: aufgehalten, Aufräumen läuft im Nebenfaden");
        }
        Entscheidung::Aufhalten => {
            api.prevent_exit();
            tracing::info!(?code, "Beenden: weiterer Wunsch, Aufräumen läuft noch");
        }
        Entscheidung::Durchlassen => {
            drop(z);
            durchlassen();
        }
        Entscheidung::NeustartUngeordnet => {
            z.phase = Phase::Fertig;
            drop(z);
            tracing::warn!("Beenden: Neustart vor dem Aufräumen — wird nicht aufgehalten");
            crate::sentry_init::capture_activity(
                "Neustart ohne geordnetes Aufräumen",
                None,
                sentry::Level::Warning,
            );
            durchlassen();
        }
    }
}

fn durchlassen() {
    // Ab hier endet die Ereignisschleife; Tauris eigener Abbau darf den
    // Hauptfaden halten, ohne dass der Wächter das meldet.
    crate::hauptfaden_waechter::beenden_beginnt();
    tracing::info!("Beenden: ExitRequested durchgelassen, Ereignisschleife darf enden");
}

/// Statt `app.restart()` (Update, neues Paket): erst aufräumen, dann neu
/// starten. Kehrt sofort zurück. Läuft das Beenden schon, bleibt es dabei.
pub(crate) fn neu_starten(app: &AppHandle) {
    let mut z = zustand();
    if z.phase != Phase::Laeuft {
        tracing::info!("Beenden: Neustart gewünscht, Beenden läuft schon");
        return;
    }
    let code = Some(tauri::RESTART_EXIT_CODE);
    if !aufraeumen_starten(app, code) {
        drop(z);
        app.request_restart();
        return;
    }
    z.phase = Phase::Raeumt;
    z.code = code;
    drop(z);
    fenster_ausblenden(app);
    tracing::info!("Beenden: Neustart, Aufräumen läuft im Nebenfaden");
}

fn fenster_ausblenden(app: &AppHandle) {
    for (label, w) in app.webview_windows() {
        if let Err(e) = w.hide() {
            tracing::warn!(fenster = %label, fehler = %e, "Beenden: Fenster ließ sich nicht ausblenden");
        }
    }
}

/// Startet Aufräum-Faden und Notbremse. `false`, wenn der Aufräum-Faden
/// nicht startete.
fn aufraeumen_starten(app: &AppHandle, code: Option<i32>) -> bool {
    let start = Instant::now();
    notbremse_starten(app.clone(), code, start);
    let a = app.clone();
    match std::thread::Builder::new()
        .name("beenden-aufraeumen".into())
        .spawn(move || aufraeumen(a, code, start))
    {
        Ok(_) => true,
        Err(e) => {
            tracing::error!(fehler = %e, "Beenden: Aufräum-Faden ließ sich nicht starten");
            false
        }
    }
}

fn aufraeumen(app: AppHandle, code: Option<i32>, start: Instant) {
    tracing::info!("Beenden: Aktivitätsprotokoll sichern beginnt");
    crate::activity_log_abwarten();
    tracing::info!("Beenden: Aktivitätsprotokoll sichern fertig");
    // Das Flugprotokoll ist die maßgebliche Aufzeichnung — hier lieber
    // länger warten (Codex-Gesamtabnahme 12.10.2026).
    tracing::info!("Beenden: Flugprotokoll sichern beginnt");
    crate::flugprotokoll_abwarten_hoechstens(Duration::from_secs(10));
    tracing::info!("Beenden: Flugprotokoll sichern fertig");
    tracing::info!("Beenden: ACARS-Postausgang sichern beginnt");
    crate::acars_abwarten_beim_beenden();
    tracing::info!("Beenden: ACARS-Postausgang sichern fertig");

    let frist = schrittfrist(ASYNC_EINZELFRIST, start.elapsed());
    tracing::info!(frist_ms = frist.as_millis() as u64, "Beenden: Hoppie und MQTT beginnt");
    let (tx, rx) = std::sync::mpsc::channel();
    let a = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = a.state::<crate::AppState>();
        // v1.3.0 (#Hoppie-PDC-CPDLC): CPDLC-Sitzung beenden, sonst sieht
        // die Gegenstelle das Flugzeug weiter verbunden.
        tracing::info!("Beenden: Hoppie-Abmeldung beginnt");
        crate::hoppie::shutdown(&a, &state).await;
        tracing::info!("Beenden: Hoppie-Abmeldung fertig");
        tracing::info!("Beenden: MQTT-Abschaltung beginnt");
        let handle = state.mqtt.lock().await.take();
        if let Some(handle) = handle {
            handle.shutdown();
            // v0.5.11: kurz Zeit für den OFFLINE-Status.
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        tracing::info!("Beenden: MQTT-Abschaltung fertig");
        let _ = tx.send(());
    });
    // std-Frist, nicht Tokio: Steht der Tokio-Timer, würde ein
    // `tokio::time::timeout` nie feuern.
    match rx.recv_timeout(frist) {
        Ok(()) => tracing::info!("Beenden: Hoppie und MQTT fertig"),
        Err(_) => tracing::error!(
            frist_ms = frist.as_millis() as u64,
            "Beenden: Hoppie und MQTT nicht in der Frist fertig — Beenden geht weiter"
        ),
    }

    zustand().phase = Phase::Fertig;
    tracing::info!(dauer_ms = start.elapsed().as_millis() as u64, "Beenden: Aufräumen fertig");
    match ende_fuer(code) {
        Ende::Neustart => app.request_restart(),
        Ende::Beenden(c) => app.exit(c),
    }
}

fn notbremse_starten(app: AppHandle, code: Option<i32>, start: Instant) {
    let gestartet = std::thread::Builder::new()
        .name("beenden-notbremse".into())
        .spawn(move || {
            while !notbremse_faellig(start.elapsed()) {
                std::thread::sleep(Duration::from_millis(500));
            }
            let phase = zustand().phase;
            tracing::error!(?phase, "Beenden: Prozess lebt nach 30 s noch — Notbremse");
            crate::sentry_init::capture_activity(
                "Beenden hing — Notbremse",
                Some(&format!("Phase {phase:?}, Code {code:?}")),
                sentry::Level::Error,
            );
            if let Some(c) = sentry::Hub::current().client() {
                c.flush(Some(Duration::from_secs(2)));
            }
            // Laufmarkierung bleibt bewusst liegen.
            match ende_fuer(code) {
                Ende::Neustart => tauri::process::restart(&app.env()),
                Ende::Beenden(c) => std::process::exit(c),
            }
        });
    if let Err(e) = gestartet {
        tracing::error!(fehler = %e, "Beenden: Notbremse ließ sich nicht starten");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erster_wunsch_haelt_auf_und_raeumt() {
        assert_eq!(entscheiden(Phase::Laeuft, false), Entscheidung::AufhaltenUndAufraeumen);
    }

    #[test]
    fn weitere_wuensche_waehrend_des_aufraeumens_halten_nur_auf() {
        assert_eq!(entscheiden(Phase::Raeumt, false), Entscheidung::Aufhalten);
    }

    #[test]
    fn nach_dem_aufraeumen_geht_es_durch() {
        assert_eq!(entscheiden(Phase::Fertig, false), Entscheidung::Durchlassen);
        assert_eq!(entscheiden(Phase::Fertig, true), Entscheidung::Durchlassen);
    }

    #[test]
    fn neustart_vor_fertig_laesst_sich_nicht_aufhalten() {
        // Tauri ignoriert prevent_exit bei RESTART_EXIT_CODE.
        assert_eq!(entscheiden(Phase::Laeuft, true), Entscheidung::NeustartUngeordnet);
        assert_eq!(entscheiden(Phase::Raeumt, true), Entscheidung::NeustartUngeordnet);
    }

    #[test]
    fn ende_behaelt_den_code() {
        assert_eq!(ende_fuer(Some(tauri::RESTART_EXIT_CODE)), Ende::Neustart);
        assert_eq!(ende_fuer(Some(3)), Ende::Beenden(3));
        assert_eq!(ende_fuer(Some(0)), Ende::Beenden(0));
        assert_eq!(ende_fuer(None), Ende::Beenden(0));
    }

    #[test]
    fn schrittfrist_ist_hoechstens_der_rest() {
        let e = ASYNC_EINZELFRIST;
        assert_eq!(schrittfrist(e, Duration::ZERO), e);
        assert_eq!(schrittfrist(e, Duration::from_secs(14)), Duration::from_secs(6));
        assert_eq!(schrittfrist(e, Duration::from_secs(25)), Duration::ZERO);
    }

    #[test]
    fn schlimmster_fall_bleibt_unter_der_notbremse() {
        // Synchrone Schritte höchstens 2 + 10 + 2 s, danach der Rest.
        let sync = Duration::from_secs(14);
        let gesamt = sync + schrittfrist(ASYNC_EINZELFRIST, sync);
        assert!(gesamt <= GESAMTFRIST);
        assert!(!notbremse_faellig(gesamt + Duration::from_secs(5)));
    }

    #[test]
    fn notbremse_greift_ab_30_s() {
        assert!(!notbremse_faellig(Duration::from_millis(29_999)));
        assert!(notbremse_faellig(Duration::from_secs(30)));
    }
}

#[cfg(test)]
#[path = "tests_lib/beenden_hauptfaden_wiring_tests.rs"]
mod beenden_hauptfaden_wiring_tests;
