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
//! 1. Erstes `ExitRequested`: Ende aufhalten, Aufräumen und Notbremse in
//!    eigenen Fäden starten, Fenster ausblenden, sofort zurück. Der
//!    Hauptfaden bleibt frei.
//! 2. Der Faden macht dieselben Schritte wie vorher. Hoppie und MQTT laufen
//!    als Task parallel zu den drei Sicher-Schritten; gewartet wird mit
//!    std-Fristen, damit auch ein stehender Tokio-Timer das Beenden nicht
//!    aufhält. Danach „fertig“ und erneut beenden; das zweite
//!    `ExitRequested` geht durch.
//! 3. Notbremse: Lebt der Prozess [`NOTBREMSE_NACH`] (wache Zeit) nach dem
//!    ersten `ExitRequested` noch, Meldung an GlitchTip und hart beenden.
//!    Die Laufmarkierung bleibt dabei liegen (ehrliches „ohne sauberes Ende“).
//!
//! Die Phase steht in einem Atomic, nicht hinter einer Sperre: Die
//! Notbremse liest sie, ohne auf den Hauptfaden oder eine Logzeile zu
//! warten (Prüfer 15.11.: das Log schreibt synchron).
//!
//! Neustart: `prevent_exit` ist bei `RESTART_EXIT_CODE` wirkungslos (tauri
//! 2.11 `ExitRequestApi::prevent_exit`). Deshalb räumt [`neu_starten`] VOR
//! dem Neustart-Wunsch auf und ruft erst danach `request_restart` (von einem
//! Nebenfaden: `ExitRequested` und `Exit` kommen an, Tauri startet bei
//! `Exit` neu). Zwischen „fertig“ und diesem `ExitRequested` hält
//! [`Phase::NeustartFolgt`] jedes fremde Ende auf, sonst ginge der Neustart
//! verloren.

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

/// Alles zusammen, vom ersten `ExitRequested` bis „fertig“.
pub(crate) const GESAMTFRIST: Duration = Duration::from_secs(20);
/// Danach (wache Zeit) beendet die Notbremse den Prozess hart.
pub(crate) const NOTBREMSE_NACH: Duration = Duration::from_secs(30);
/// Zeit nach „fertig“ für Tauris eigenen Abbau, bevor die Notbremse greift
/// (nur das Budget im Test rechnet damit).
#[cfg(test)]
pub(crate) const ABBAU_RESERVE: Duration = Duration::from_secs(5);

/// Sicher-Schritte nacheinander, je mit eigener Frist.
pub(crate) const FRIST_AKTIVITAET: Duration = Duration::from_secs(2);
/// Das Flugprotokoll ist die maßgebliche Aufzeichnung — hier lieber länger
/// warten (Codex-Gesamtabnahme 12.10.2026).
pub(crate) const FRIST_FLUGPROTOKOLL: Duration = Duration::from_secs(10);
pub(crate) const FRIST_ACARS: Duration = Duration::from_secs(2);
/// Hoppie-Abmeldung (eine Anfrage, [`crate::hoppie::HTTP_FRIST`]) plus
/// MQTT-Stopp mit 200 ms Pause, plus Reserve. Läuft parallel zu den
/// Sicher-Schritten.
pub(crate) const FRIST_HOPPIE_MQTT: Duration = Duration::from_secs(16);
/// Pause für den OFFLINE-Status nach dem MQTT-Stopp (v0.5.11).
const MQTT_OFFLINE_PAUSE: Duration = Duration::from_millis(200);

/// Takt der Notbremse; kam ein Takt um [`VERSCHLAFEN_AB`] zu spät, hat der
/// Rechner geschlafen — die Lücke zählt nicht als Hänger.
const NOTBREMSE_TAKT: Duration = Duration::from_millis(500);
const VERSCHLAFEN_AB: Duration = Duration::from_secs(5);
/// So lange darf die Meldung der Notbremse (Log, GlitchTip) dauern.
const NOTBREMSE_MELDEFRIST: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Phase {
    Laeuft = 0,
    Raeumt = 1,
    /// Aufgeräumt, `request_restart` folgt: Nur der Neustart geht durch.
    NeustartFolgt = 2,
    Fertig = 3,
}

impl Phase {
    fn aus(wert: u8) -> Phase {
        match wert {
            0 => Phase::Laeuft,
            1 => Phase::Raeumt,
            2 => Phase::NeustartFolgt,
            _ => Phase::Fertig,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entscheidung {
    /// Ende aufhalten, Fenster weg, Aufräumen und Notbremse starten.
    AufhaltenUndAufraeumen,
    /// Ende aufhalten — das Aufräumen läuft schon (oder der Neustart folgt).
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
        (Phase::NeustartFolgt, true) => Entscheidung::Durchlassen,
        (Phase::NeustartFolgt, false) => Entscheidung::Aufhalten,
        (_, true) => Entscheidung::NeustartUngeordnet,
        (Phase::Laeuft, false) => Entscheidung::AufhaltenUndAufraeumen,
        (Phase::Raeumt, false) => Entscheidung::Aufhalten,
    }
}

/// Entscheiden und den Übergang setzen, in einem Schritt und ohne Sperre.
/// Verliert der Übergang gegen einen anderen Faden, wird neu entschieden.
pub(crate) fn uebernehmen(phase: &AtomicU8, neustart: bool) -> Entscheidung {
    let mut jetzt = Phase::aus(phase.load(Ordering::Acquire));
    loop {
        let e = entscheiden(jetzt, neustart);
        let nach = match e {
            Entscheidung::AufhaltenUndAufraeumen => Phase::Raeumt,
            Entscheidung::NeustartUngeordnet => Phase::Fertig,
            Entscheidung::Aufhalten | Entscheidung::Durchlassen => return e,
        };
        match phase.compare_exchange(jetzt as u8, nach as u8, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => return e,
            Err(wert) => jetzt = Phase::aus(wert),
        }
    }
}

/// Von `Laeuft` auf `Raeumt`, wenn noch niemand beendet. `false`: Beenden
/// läuft schon.
pub(crate) fn beenden_uebernehmen(phase: &AtomicU8) -> bool {
    phase
        .compare_exchange(Phase::Laeuft as u8, Phase::Raeumt as u8, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
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

/// Phase nach dem Aufräumen: beim Neustart hält sie fremde Enden auf, bis
/// das eigene Neustart-`ExitRequested` da ist.
pub(crate) fn phase_nach_dem_aufraeumen(ende: Ende) -> Phase {
    match ende {
        Ende::Neustart => Phase::NeustartFolgt,
        Ende::Beenden(_) => Phase::Fertig,
    }
}

/// Frist eines Schritts: seine eigene, höchstens der Rest der Gesamtfrist.
pub(crate) fn schrittfrist(einzel: Duration, vergangen: Duration) -> Duration {
    einzel.min(GESAMTFRIST.saturating_sub(vergangen))
}

/// Die Notbremse greift erst, wenn der Prozess so lange (wach) nach dem
/// ersten `ExitRequested` noch lebt.
pub(crate) fn notbremse_faellig(wach: Duration) -> bool {
    wach >= NOTBREMSE_NACH
}

/// Wache Zeit nach einem Takt. Eine Lücke ab [`VERSCHLAFEN_AB`] gehört dem
/// Rechnerschlaf und zählt nur wie ein Takt (Muster `hauptfaden_waechter`).
pub(crate) fn wachzeit_nach_takt(bisher: Duration, luecke: Duration) -> Duration {
    if luecke >= VERSCHLAFEN_AB {
        bisher + NOTBREMSE_TAKT
    } else {
        bisher + luecke
    }
}

static PHASE: AtomicU8 = AtomicU8::new(Phase::Laeuft as u8);

fn phase() -> Phase {
    Phase::aus(PHASE.load(Ordering::Acquire))
}

fn phase_setzen(p: Phase) {
    PHASE.store(p as u8, Ordering::Release);
}

/// Darf ein Fenster gezeigt werden? Nicht, solange beendet wird — sonst
/// stünde ein Fenster ohne MQTT und Hoppie da (Tray „Anzeigen“, Zweitstart).
pub(crate) fn fenster_zeigen_erlaubt() -> bool {
    zeigen_erlaubt_in(phase())
}

pub(crate) fn zeigen_erlaubt_in(p: Phase) -> bool {
    p == Phase::Laeuft
}

/// Aus dem `.run`-Callback für `RunEvent::ExitRequested`. Kehrt sofort
/// zurück — hier wird nie gewartet und keine Sperre gehalten.
pub(crate) fn exit_requested(app: &AppHandle, code: Option<i32>, api: &tauri::ExitRequestApi) {
    let neustart = code == Some(tauri::RESTART_EXIT_CODE);
    match uebernehmen(&PHASE, neustart) {
        Entscheidung::AufhaltenUndAufraeumen => {
            if !aufraeumen_starten(app, code) {
                // Ohne Faden kein geordnetes Ende — dann lieber sofort.
                phase_setzen(Phase::Fertig);
                durchlassen();
                return;
            }
            api.prevent_exit();
            tracing::info!(?code, "Beenden: aufgehalten, Aufräumen läuft im Nebenfaden");
            fenster_ausblenden(app);
        }
        Entscheidung::Aufhalten => {
            api.prevent_exit();
            tracing::info!(?code, "Beenden: weiterer Wunsch, Aufräumen läuft noch");
        }
        Entscheidung::Durchlassen => {
            durchlassen();
        }
        Entscheidung::NeustartUngeordnet => {
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

/// Statt `app.restart()` (Update unter macOS/Linux, neues Paket): erst
/// aufräumen, dann neu starten. Kehrt sofort zurück. Läuft das Beenden
/// schon, bleibt es dabei.
pub(crate) fn neu_starten(app: &AppHandle) {
    if !beenden_uebernehmen(&PHASE) {
        tracing::info!("Beenden: Neustart gewünscht, Beenden läuft schon");
        return;
    }
    let code = Some(tauri::RESTART_EXIT_CODE);
    if !aufraeumen_starten(app, code) {
        // Ungeordnet neu starten; `exit_requested` meldet das laut.
        phase_setzen(Phase::Laeuft);
        app.request_restart();
        return;
    }
    tracing::info!("Beenden: Neustart, Aufräumen läuft im Nebenfaden");
    fenster_ausblenden(app);
}

/// Windows-Update: Der Updater startet das Installationsprogramm und
/// beendet den Prozess selbst mit `std::process::exit(0)`
/// (tauri-plugin-updater 2.10.1, `install_inner`). Kein `ExitRequested`,
/// kein Aufräumen — deshalb hier synchron dieselben Schritte, aus dem
/// `on_before_exit`-Haken unmittelbar davor. Läuft im Faden des Updaters,
/// nie auf dem Hauptfaden.
pub(crate) fn vor_dem_installieren(app: &AppHandle) {
    if !beenden_uebernehmen(&PHASE) {
        tracing::info!("Beenden: Update-Installation, Beenden läuft schon");
        return;
    }
    let start = Instant::now();
    tracing::info!("Beenden: Update-Installation, Aufräumen vor dem Installationsprogramm");
    sichern_und_abmelden(app, start);
    phase_setzen(Phase::Fertig);
    tracing::info!(dauer_ms = start.elapsed().as_millis() as u64, "Beenden: Aufräumen vor der Installation fertig");
}

fn fenster_ausblenden(app: &AppHandle) {
    for (label, w) in app.webview_windows() {
        if let Err(e) = w.hide() {
            tracing::warn!(fenster = %label, fehler = %e, "Beenden: Fenster ließ sich nicht ausblenden");
        }
    }
}

/// Startet Notbremse und Aufräum-Faden. `false`, wenn der Aufräum-Faden
/// nicht startete.
fn aufraeumen_starten(app: &AppHandle, code: Option<i32>) -> bool {
    let start = Instant::now();
    notbremse_starten(app.clone(), code);
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
    sichern_und_abmelden(&app, start);
    let ende = ende_fuer(code);
    tracing::info!(dauer_ms = start.elapsed().as_millis() as u64, ?ende, "Beenden: Aufräumen fertig");
    phase_setzen(phase_nach_dem_aufraeumen(ende));
    match ende {
        Ende::Neustart => app.request_restart(),
        Ende::Beenden(c) => app.exit(c),
    }
}

/// Die Schritte des alten Hauptfaden-Aufräumens, alle mit Frist. Höchstens
/// [`GESAMTFRIST`] ab `start`.
fn sichern_und_abmelden(app: &AppHandle, start: Instant) {
    // Hoppie und MQTT zuerst anstoßen: Sie laufen parallel zu den
    // Sicher-Schritten, die nur auf die Platte schreiben.
    tracing::info!("Beenden: Hoppie und MQTT beginnt");
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
            tokio::time::sleep(MQTT_OFFLINE_PAUSE).await;
        }
        tracing::info!("Beenden: MQTT-Abschaltung fertig");
        let _ = tx.send(());
    });

    tracing::info!("Beenden: Aktivitätsprotokoll sichern beginnt");
    crate::activity_log_abwarten(schrittfrist(FRIST_AKTIVITAET, start.elapsed()));
    tracing::info!("Beenden: Aktivitätsprotokoll sichern fertig");
    tracing::info!("Beenden: Flugprotokoll sichern beginnt");
    crate::flugprotokoll_abwarten_hoechstens(schrittfrist(FRIST_FLUGPROTOKOLL, start.elapsed()));
    tracing::info!("Beenden: Flugprotokoll sichern fertig");
    tracing::info!("Beenden: ACARS-Postausgang sichern beginnt");
    crate::acars_abwarten_beim_beenden(schrittfrist(FRIST_ACARS, start.elapsed()));
    tracing::info!("Beenden: ACARS-Postausgang sichern fertig");

    // Rest der eigenen Frist (seit `start` läuft der Task schon).
    let frist = schrittfrist(FRIST_HOPPIE_MQTT.saturating_sub(start.elapsed()), start.elapsed());
    // std-Frist, nicht Tokio: Steht der Tokio-Timer, würde ein
    // `tokio::time::timeout` nie feuern.
    match rx.recv_timeout(frist) {
        Ok(()) => tracing::info!("Beenden: Hoppie und MQTT fertig"),
        Err(_) => tracing::error!(
            frist_ms = frist.as_millis() as u64,
            "Beenden: Hoppie und MQTT nicht in der Frist fertig — Beenden geht weiter"
        ),
    }
}

fn notbremse_starten(app: AppHandle, code: Option<i32>) {
    let gestartet = std::thread::Builder::new()
        .name("beenden-notbremse".into())
        .spawn(move || notbremse(app, code));
    if let Err(e) = gestartet {
        tracing::error!(fehler = %e, "Beenden: Notbremse ließ sich nicht starten");
    }
}

/// Wartet [`NOTBREMSE_NACH`] wache Zeit, dann hart beenden. Liest nur das
/// Atomic; die Meldung läuft in einem eigenen Faden mit Frist, damit ein
/// stockendes Log die Notbremse nicht aufhält.
fn notbremse(app: AppHandle, code: Option<i32>) {
    let mut wach = Duration::ZERO;
    let mut zuletzt = Instant::now();
    while !notbremse_faellig(wach) {
        std::thread::sleep(NOTBREMSE_TAKT);
        let jetzt = Instant::now();
        wach = wachzeit_nach_takt(wach, jetzt.duration_since(zuletzt));
        zuletzt = jetzt;
    }
    let phase = phase();
    let (tx, rx) = std::sync::mpsc::channel();
    let gemeldet = std::thread::Builder::new()
        .name("beenden-notbremse-meldung".into())
        .spawn(move || {
            tracing::error!(?phase, "Beenden: Prozess lebt nach 30 s noch — Notbremse");
            crate::sentry_init::capture_activity(
                "Beenden hing — Notbremse",
                Some(&format!("Phase {phase:?}, Code {code:?}")),
                sentry::Level::Error,
            );
            if let Some(c) = sentry::Hub::current().client() {
                c.flush(Some(Duration::from_secs(2)));
            }
            let _ = tx.send(());
        });
    if gemeldet.is_ok() {
        let _ = rx.recv_timeout(NOTBREMSE_MELDEFRIST);
    }
    // Laufmarkierung bleibt bewusst liegen.
    match ende_fuer(code) {
        Ende::Neustart => tauri::process::restart(&app.env()),
        Ende::Beenden(c) => std::process::exit(c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atomar(p: Phase) -> AtomicU8 {
        AtomicU8::new(p as u8)
    }

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
    fn uebernehmen_setzt_die_phase_im_selben_schritt() {
        let p = atomar(Phase::Laeuft);
        assert_eq!(uebernehmen(&p, false), Entscheidung::AufhaltenUndAufraeumen);
        assert_eq!(Phase::aus(p.load(Ordering::Acquire)), Phase::Raeumt);
        // Der zweite Wunsch startet kein zweites Aufräumen.
        assert_eq!(uebernehmen(&p, false), Entscheidung::Aufhalten);
        assert_eq!(uebernehmen(&p, true), Entscheidung::NeustartUngeordnet);
        assert_eq!(Phase::aus(p.load(Ordering::Acquire)), Phase::Fertig);
        assert_eq!(uebernehmen(&p, false), Entscheidung::Durchlassen);
    }

    #[test]
    fn beenden_wird_nur_einmal_uebernommen() {
        let p = atomar(Phase::Laeuft);
        assert!(beenden_uebernehmen(&p));
        assert!(!beenden_uebernehmen(&p), "zweites Aufräumen parallel");
        assert_eq!(uebernehmen(&p, false), Entscheidung::Aufhalten);
    }

    #[test]
    fn schluss_beendet_oder_startet_neu() {
        // Beenden: danach geht jedes Ende durch.
        let f = phase_nach_dem_aufraeumen(ende_fuer(Some(0)));
        assert_eq!(f, Phase::Fertig);
        assert_eq!(entscheiden(f, false), Entscheidung::Durchlassen);
        // Neustart (H2): ein fremdes Ende vor dem eigenen Neustart-Wunsch
        // wird aufgehalten, der Neustart selbst geht durch.
        let n = phase_nach_dem_aufraeumen(ende_fuer(Some(tauri::RESTART_EXIT_CODE)));
        assert_eq!(n, Phase::NeustartFolgt);
        assert_eq!(entscheiden(n, false), Entscheidung::Aufhalten);
        assert_eq!(entscheiden(n, true), Entscheidung::Durchlassen);
    }

    #[test]
    fn ende_behaelt_den_code() {
        assert_eq!(ende_fuer(Some(tauri::RESTART_EXIT_CODE)), Ende::Neustart);
        assert_eq!(ende_fuer(Some(3)), Ende::Beenden(3));
        assert_eq!(ende_fuer(Some(0)), Ende::Beenden(0));
        assert_eq!(ende_fuer(None), Ende::Beenden(0));
    }

    #[test]
    fn fenster_nur_ohne_beenden() {
        assert!(zeigen_erlaubt_in(Phase::Laeuft));
        for p in [Phase::Raeumt, Phase::NeustartFolgt, Phase::Fertig] {
            assert!(!zeigen_erlaubt_in(p), "{p:?}");
        }
    }

    #[test]
    fn phase_ueberlebt_den_umweg_ueber_u8() {
        for p in [Phase::Laeuft, Phase::Raeumt, Phase::NeustartFolgt, Phase::Fertig] {
            assert_eq!(Phase::aus(p as u8), p);
        }
    }

    #[test]
    fn schrittfrist_ist_hoechstens_der_rest() {
        let e = FRIST_HOPPIE_MQTT;
        assert_eq!(schrittfrist(e, Duration::ZERO), e);
        assert_eq!(schrittfrist(e, Duration::from_secs(14)), Duration::from_secs(6));
        assert_eq!(schrittfrist(e, Duration::from_secs(25)), Duration::ZERO);
    }

    #[test]
    fn schlimmster_fall_bleibt_unter_der_notbremse() {
        // Ungekappt aus den echten Fristen: Die Sicher-Schritte laufen
        // nacheinander, Hoppie/MQTT parallel dazu. Erhöht jemand eine Frist
        // über die Gesamtfrist hinaus, wird das hier rot.
        let sicher = FRIST_AKTIVITAET + FRIST_FLUGPROTOKOLL + FRIST_ACARS;
        assert!(sicher <= GESAMTFRIST, "Sicher-Schritte {sicher:?} > Gesamtfrist");
        assert!(FRIST_HOPPIE_MQTT <= GESAMTFRIST, "Hoppie/MQTT > Gesamtfrist");
        assert!(sicher.max(FRIST_HOPPIE_MQTT) <= GESAMTFRIST);
        assert!(
            GESAMTFRIST + ABBAU_RESERVE <= NOTBREMSE_NACH,
            "Gesamtfrist + Abbau erreicht die Notbremse"
        );
        assert!(!notbremse_faellig(GESAMTFRIST + ABBAU_RESERVE - Duration::from_millis(1)));
    }

    #[test]
    fn hoppie_mqtt_frist_hat_reserve() {
        // H1: eine Hoppie-Anfrage plus OFFLINE-Pause, dazu mindestens 500 ms.
        let bedarf = crate::hoppie::HTTP_FRIST + MQTT_OFFLINE_PAUSE + Duration::from_millis(500);
        assert!(FRIST_HOPPIE_MQTT >= bedarf, "{FRIST_HOPPIE_MQTT:?} < {bedarf:?}");
    }

    #[test]
    fn notbremse_greift_ab_30_s() {
        assert!(!notbremse_faellig(Duration::from_millis(29_999)));
        assert!(notbremse_faellig(Duration::from_secs(30)));
    }

    #[test]
    fn rechnerschlaf_zaehlt_nicht_als_haenger() {
        // 10 s wach, dann 2 h Schlaf: weiter nur ~10,5 s wach, keine Notbremse.
        let mut wach = Duration::ZERO;
        for _ in 0..20 {
            wach = wachzeit_nach_takt(wach, NOTBREMSE_TAKT);
        }
        wach = wachzeit_nach_takt(wach, Duration::from_secs(7_200));
        assert!(!notbremse_faellig(wach), "{wach:?}");
        assert_eq!(wach, Duration::from_millis(10_500));
        // Wach weiterzählen: nach insgesamt 30 s greift sie doch.
        while !notbremse_faellig(wach) {
            wach = wachzeit_nach_takt(wach, Duration::from_millis(600));
        }
        assert!(wach < NOTBREMSE_NACH + Duration::from_secs(1));
    }

    #[test]
    fn langsame_takte_zaehlen_voll() {
        // Unter der Schlafschwelle zählt jede Lücke ganz (stockende Fäden
        // sind ein Hänger, kein Schlaf).
        assert_eq!(
            wachzeit_nach_takt(Duration::ZERO, Duration::from_millis(4_900)),
            Duration::from_millis(4_900)
        );
    }
}

#[cfg(test)]
#[path = "tests_lib/beenden_hauptfaden_wiring_tests.rs"]
mod beenden_hauptfaden_wiring_tests;
