fn active_flight_path(app: &AppHandle) -> Result<PathBuf, UiError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(ACTIVE_FLIGHT_FILE))
        .map_err(|e| UiError::new("config_path", e.to_string()))
}

fn write_persisted_flight(app: &AppHandle, flight: &PersistedFlight) -> Result<(), UiError> {
    // ⚠ Der Zeitstempel wird HIER gesetzt, nicht beim Aufrufer. Sonst müsste
    // jede künftige Schreibstelle daran denken, und genau das vergisst man —
    // die Datei sähe dann alt aus, obwohl sie gerade geschrieben wurde, und
    // die Wiederaufnahme verwürfe einen laufenden Flug.
    let mut flight = flight.clone();
    flight.zuletzt_geschrieben = Some(Utc::now());
    let flight = &flight;
    let path = active_flight_path(app)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| UiError::new("config_write", e.to_string()))?;
    }
    let bytes = serde_json::to_vec_pretty(flight)
        .map_err(|e| UiError::new("config_serialize", e.to_string()))?;
    std::fs::write(&path, bytes).map_err(|e| UiError::new("config_write", e.to_string()))
}

fn read_persisted_flight(app: &AppHandle) -> Option<PersistedFlight> {
    let path = active_flight_path(app).ok()?;
    if !path.exists() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    serde_json::from_slice::<PersistedFlight>(&bytes).ok()
}

/// Wie der vorige Programmlauf endete, gelesen aus seiner Sentinel-Datei.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VorlaufEnde {
    /// Keine Sentinel-Datei — sauber beendet.
    Sauber,
    /// Andere Fassung — der Auto-Updater hat neu gestartet, kein Absturz.
    Update { vorher: String },
    /// Gleiche Fassung, Datei noch da — abgestuerzt oder abgeschossen.
    Abbruch { gestartet: Option<String> },
    /// Datei ohne Fassung (vor 21.09.2026 geschrieben) — nicht zu entscheiden.
    Unbekannt,
}

/// Ordnet den Inhalt der Sentinel-Datei des VORIGEN Laufs ein.
///
/// Befund 21.09.2026 (Sven M): 22 Programmstarts hintereinander ohne ein
/// sauberes Ende, keiner davon kam in GlitchTip an — die Sentinel-Datei
/// wurde nur fuer einen laufenden Flug ausgewertet, und Absturz und
/// Update-Neustart sahen gleich aus.
fn vorlauf_einordnen(sentinel: Option<&str>, aktuelle_version: &str) -> VorlaufEnde {
    let Some(inhalt) = sentinel else {
        return VorlaufEnde::Sauber;
    };
    let feld = |name: &str| {
        inhalt
            .lines()
            .find_map(|z| z.strip_prefix(name).map(|w| w.trim().to_string()))
            .filter(|w| !w.is_empty())
    };
    match feld("version=") {
        None => VorlaufEnde::Unbekannt,
        Some(v) if v != aktuelle_version => VorlaufEnde::Update { vorher: v },
        Some(_) => VorlaufEnde::Abbruch {
            gestartet: feld("started_at="),
        },
    }
}

/// Meldet einen Abbruch des vorigen Laufs: sofort ins Log (die
/// Diagnose-Auswertung auf dem Live-Server zaehlt ihn), nach 30 s an
/// GlitchTip — erst dann ist die Zustimmung des Piloten geladen, vorher
/// wuerde `before_send` die Meldung verwerfen. Bewusst OHNE Log-Zeilen:
/// darin stehen Namen und Pfade; die Einzelheiten kommen mit dem
/// Diagnose-Log des naechsten Flugs.
fn vorlauf_melden(ende: &VorlaufEnde) {
    let VorlaufEnde::Abbruch { gestartet } = ende else {
        return;
    };
    let version = env!("CARGO_PKG_VERSION");
    tracing::warn!(
        version,
        vorheriger_start = gestartet.as_deref().unwrap_or("?"),
        "Vorheriger Lauf endete ohne sauberes Ende"
    );
    let detail = format!(
        "Fassung {version}, vorheriger Lauf gestartet {}",
        gestartet.as_deref().unwrap_or("?")
    );
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        sentry_init::capture_activity(
            "App wurde nicht sauber beendet",
            Some(&detail),
            sentry::Level::Warning,
        );
    });
}

#[cfg(test)]
#[path = "tests_lib/vorlauf_tests.rs"]
mod vorlauf_tests;

fn run_sentinel_path(app: &AppHandle) -> Result<PathBuf, UiError> {
    app.path()
        .app_config_dir()
        .map(|dir| dir.join(RUN_SENTINEL_FILE))
        .map_err(|e| UiError::new("config_path", e.to_string()))
}

/// v0.20 (Process-Integrity): true if a sentinel from a PREVIOUS run is
/// still on disk — meaning that run did not exit cleanly. Must be called
/// BEFORE `write_run_sentinel` overwrites it for the current run.
fn run_sentinel_present(app: &AppHandle) -> bool {
    match run_sentinel_path(app) {
        Ok(path) => path.exists(),
        Err(_) => false,
    }
}

/// Write the current run's sentinel (PID + launch time, informational —
/// presence/absence is what matters). Best-effort: a write failure just
/// means we lose the signal for this run, never a hard error.
fn write_run_sentinel(app: &AppHandle) {
    let Ok(path) = run_sentinel_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(error = %e, "could not ensure run-sentinel parent dir");
            return;
        }
    }
    // `version` seit 21.09.2026: unterscheidet beim naechsten Start einen
    // Abbruch (gleiche Fassung) von einem Update-Neustart (andere Fassung).
    let content = format!(
        "pid={}\nstarted_at={}\nversion={}\n",
        std::process::id(),
        Utc::now().to_rfc3339(),
        env!("CARGO_PKG_VERSION")
    );
    if let Err(e) = std::fs::write(&path, content) {
        tracing::warn!(error = %e, "could not write run sentinel");
    }
}

/// Remove the current run's sentinel — call ONLY from the clean-exit path
/// (see `beendet_den_lauf_sauber`). Never called from a crash/kill, which is
/// exactly the point: absence next launch = clean exit, presence = not.
fn clear_run_sentinel(app: &AppHandle) {
    if let Ok(path) = run_sentinel_path(app) {
        if path.exists() {
            if let Err(e) = std::fs::remove_file(&path) {
                tracing::warn!(error = %e, "could not remove run sentinel");
            }
        }
    }
}

/// Welche Ereignisse einen Lauf sauber beenden.
///
/// Log-Durchsicht 26.09.2026: Beim Herunterfahren oder Abmelden in Windows
/// loest tao 0.35 ueber `WM_ENDSESSION` nur `LoopDestroyed` →
/// `RunEvent::Exit` aus, `ExitRequested` kommt dann nie (tauri-runtime-wry
/// 2.11.0, Z. 4192/4317–4370; tao `platform_impl/windows/event_loop.rs`
/// Z. 2384). Die Markierung blieb liegen, der naechste Start meldete
/// „Vorheriger Lauf endete ohne sauberes Ende" samt GlitchTip-Eintrag.
/// `Exit` kommt bei jedem echten Ende, auch nach `ExitRequested`.
fn beendet_den_lauf_sauber(event: &tauri::RunEvent) -> bool {
    matches!(
        event,
        tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
    )
}

#[cfg(test)]
#[path = "tests_lib/sauberes_ende_tests.rs"]
mod sauberes_ende_tests;

// ---- Activity-log persistence ----
//
// Dump the in-memory activity log to disk after every mutation so
// an app restart mid-flight doesn't lose the running event history.
// Read once at boot to pre-populate AppState. Cheap: 1000-entry log
// is roughly 100–200 KB, well within "instant" sync write cost.
//
// The path is resolved once on the first call (with the AppHandle
// available there) and cached in a OnceLock so the lock-free
// log_activity helper that doesn't have an AppHandle can still
// persist without re-resolving on every call.

use std::sync::OnceLock;
static ACTIVITY_LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Resolve and cache the activity-log path on first use. Called once
/// from the Tauri setup hook with the AppHandle, then read from the
/// OnceLock everywhere afterwards.
fn init_activity_log_path(app: &AppHandle) {
    let path = app
        .path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(ACTIVITY_LOG_FILE));
    let _ = ACTIVITY_LOG_PATH.set(path);
}

/// Reset the activity log for a fresh recording. Called from
/// `flight_start` (NOT `flight_adopt`) so each new flight starts
/// with a clean feed in the UI and on disk. Per-PIREP JSONL flight-
/// event logs are unaffected — they remain on disk forever for
/// review / debugging.
fn clear_activity_log_for_new_flight(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut log = state.activity_log.lock().expect("activity_log lock");
    log.clear();
    save_activity_log(&log);
    tracing::info!("activity log cleared for new flight");
}

/// Best-effort persist of the entire activity-log VecDeque to disk.
/// Failures are logged at warn level but never propagated — the
/// activity log is informational, not safety-critical, and we'd
/// rather drop a write than crash the streamer.
/// Den aktuellen Stand des Aktivitätsprotokolls auf die Platte bringen —
/// im Hintergrund.
///
/// # Warum nicht mehr synchron (AZU 8710, 11.10.2026)
///
/// Bis 2.0.9 schrieb jeder einzelne Eintrag (Fahrwerk, Klappen, Lichter,
/// Autopilot …) das GANZE Protokoll — bis zu 1000 Einträge als formatiertes
/// JSON — neu auf die Platte, und zwar aus `detect_telemetry_changes`, das
/// dabei die Sperre `flight.stats` hält. Auf genau diese Sperre wartet der
/// Aufsetz-Sampler in jedem 20-ms-Takt. Im Endanflug ändert sich viel auf
/// einmal; jede Zeile kostete Platten-Zeit unter der Sperre.
///
/// Jetzt geht nur eine Kopie der Einträge an einen eigenen Schreib-Faden.
/// Liegen beim Schreiben schon neuere Stände an, schreibt er nur den
/// neuesten. Reihenfolge bleibt erhalten (ein Kanal, ein Faden) — auch ein
/// Leeren (`activity_log_clear`) wird nicht von einem älteren Stand
/// überholt.
fn save_activity_log(log: &VecDeque<ActivityEntry>) {
    let kopie: Vec<ActivityEntry> = log.iter().cloned().collect();
    // Nur der neueste Stand wird gehalten — hängt die Platte, stapeln sich
    // keine Kopien (unabhängige Gesamtabnahme 12.10.2026).
    if let Ok(mut neuester) = AKTIVITAET_NEUESTER.lock() {
        *neuester = Some(kopie);
    }
    let _ = activity_log_schreiber().send(AktivitaetAuftrag::Wecken);
}

static AKTIVITAET_NEUESTER: std::sync::Mutex<Option<Vec<ActivityEntry>>> =
    std::sync::Mutex::new(None);

enum AktivitaetAuftrag {
    Wecken,
    Abwarten(std::sync::mpsc::SyncSender<()>),
}

fn activity_log_schreiber() -> std::sync::mpsc::Sender<AktivitaetAuftrag> {
    static SCHREIBER: OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<AktivitaetAuftrag>>> =
        OnceLock::new();
    SCHREIBER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<AktivitaetAuftrag>();
            std::thread::spawn(move || {
                while let Ok(erster) = rx.recv() {
                    let mut warten = Vec::new();
                    for auftrag in std::iter::once(erster).chain(rx.try_iter()) {
                        if let AktivitaetAuftrag::Abwarten(fertig) = auftrag {
                            warten.push(fertig);
                        }
                    }
                    let stand = AKTIVITAET_NEUESTER.lock().ok().and_then(|mut n| n.take());
                    if let Some(stand) = stand {
                        activity_log_schreiben(&stand);
                    }
                    for fertig in warten {
                        let _ = fertig.send(());
                    }
                }
            });
            std::sync::Mutex::new(tx)
        })
        .lock()
        .map(|tx| tx.clone())
        .expect("aktivitaetsprotokoll-schreiber lock")
}

/// Warten, bis der zuletzt gemeldete Stand auf der Platte steht (höchstens
/// 2 s) — beim Beenden der App.
fn activity_log_abwarten() {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    if activity_log_schreiber().send(AktivitaetAuftrag::Abwarten(tx)).is_ok() {
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }
}

fn activity_log_schreiben(entries: &[ActivityEntry]) {
    let Some(Some(path)) = ACTIVITY_LOG_PATH.get() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!(error = %e, "could not ensure activity-log parent dir");
            return;
        }
    }
    match serde_json::to_vec_pretty(entries) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(path, bytes) {
                tracing::warn!(error = %e, "could not write activity log");
            }
        }
        Err(e) => tracing::warn!(error = %e, "could not serialize activity log"),
    }
}

/// Read the persisted activity log from disk — used to seed the AppState's
/// VecDeque in the Tauri setup hook (after `init_activity_log_path`), so the
/// pilot sees the full flight history when re-opening Tauri mid-flight.
/// Resolves via the SAME cached `app_config_dir` path that `save_activity_log`
/// writes to → cross-platform. (Previously resolved via the `APPDATA` env var,
/// which is Windows-only, so on macOS/Linux the log restored empty every launch
/// — the "flight log gone after restart" bug.) Best-effort: any failure → empty.
fn load_persisted_activity_log() -> VecDeque<ActivityEntry> {
    let Some(Some(path)) = ACTIVITY_LOG_PATH.get() else {
        return VecDeque::new();
    };
    if !path.exists() {
        return VecDeque::new();
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(error = %e, "could not read persisted activity log");
            return VecDeque::new();
        }
    };
    match serde_json::from_slice::<Vec<ActivityEntry>>(&bytes) {
        Ok(entries) => {
            tracing::info!(restored = entries.len(), "activity log restored from disk");
            entries.into_iter().collect()
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not parse persisted activity log");
            VecDeque::new()
        }
    }
}

/// Drop any queued positions for the given PIREP. Called when the user
/// cancels or forgets a flight — replaying those rows would attach
/// them to a PIREP that's already gone or moved on, so we clear them
/// out cleanly. Other flights' queued rows stay put.
fn discard_queued_positions_for(app: &AppHandle, pirep_id: &str) {
    let Some(q) = open_position_queue(app) else {
        return;
    };
    let items = match q.read_all() {
        Ok(v) => v,
        Err(_) => return,
    };
    let kept: Vec<_> = items
        .into_iter()
        .filter(|i| i.pirep_id != pirep_id)
        .collect();
    let _ = q.replace(&kept);
}

/// Reine Eigentuemer-Entscheidung fuer `clear_persisted_flight`: darf die
/// Ablage geloescht werden?
///
/// Codex-Folgefund (adversarial, 05.09.2026, gegen den Commit dieser
/// Sitzung): der `persistence_lock` aus [[schreiben_falls_aktiv]] serialisiert
/// Schreiben/Loeschen NUR gegeneinander — er sagt nichts darueber, WESSEN
/// Flug gerade an dem einen festen Pfad (`active_flight_path`) liegt.
/// `flight_cancel` nimmt den alten Flug per `guard.take()` aus
/// `state.active_flight`, setzt `stop`, und haengt DANACH an einem echten
/// Await (`client.cancel_pirep(...).await`, ein Server-Roundtrip). In
/// dieser Luecke ist der Slot leer — `flight_start` kann in dieser Zeit
/// bereits einen NEUEN Flug anlegen und dessen ersten Checkpoint an
/// denselben Pfad schreiben. Wenn der alte Cancel danach fortsetzt und
/// diese Funktion aufruft, loeschte sie bisher blind — und riss damit den
/// gerade erst gestarteten neuen Flug weg. Derselbe Spalt existiert bei
/// jedem anderen Aufrufer mit einem Await zwischen "alten Flug aus dem
/// State nehmen" und "Ablage loeschen" (z. B. `try_resume_flight`s
/// Verwerfen-Pfade nach `client.get_pirep(...).await`).
///
/// Nur loeschen, wenn entweder gar kein Eigentuemer erwartet wird
/// (`erwartete_pirep_id: None`, fuer die seltenen Faelle ohne bekannten
/// Flug), die Ablage denselben PIREP traegt, oder die Ablage gar nicht
/// (mehr) lesbar ist (dann laesst sich kein fremder Eigentuemer feststellen
/// — dasselbe „unlesbar = wie nicht vorhanden"-Prinzip wie beim Resume-Pfad).
fn sollte_persistierten_flug_loeschen(
    erwartete_pirep_id: Option<&str>,
    tatsaechliche_pirep_id_auf_platte: Option<&str>,
) -> bool {
    match erwartete_pirep_id {
        None => true,
        Some(erwartet) => match tatsaechliche_pirep_id_auf_platte {
            Some(tatsaechlich) => tatsaechlich == erwartet,
            None => true,
        },
    }
}

fn clear_persisted_flight(app: &AppHandle, erwartete_pirep_id: Option<&str>) {
    let Ok(path) = active_flight_path(app) else {
        return;
    };
    let state = app.state::<AppState>();
    {
        // Nachgereicht (externe Gegenpruefung Codex, adversarial, waehrend
        // der v1.7.17-QS-Runde entdeckt): denselben kritischen Abschnitt
        // nehmen wie `save_active_flight` (`AppState::persistence_lock`),
        // BEVOR die Ablage geloescht wird. An JEDER Aufrufstelle dieser
        // Funktion ist `flight.stop` zu diesem Zeitpunkt bereits `true`
        // (entweder direkt davor gesetzt oder frueher in derselben
        // Funktion) — ein Schreibversuch, der seinen eigenen `stop`-Check
        // schon VOR dieser Loeschung bestanden hatte und noch mitten im
        // Schreiben ist, haelt denselben Lock; diese Loeschung wartet
        // darauf und gewinnt danach garantiert (die naechste Pruefung
        // sieht `stop == true` und schreibt nicht mehr) — ohne das Schloss
        // haette eine solche verspaetete Schreibung die Loeschung
        // rueckgaengig machen koennen.
        //
        // Nachgereicht #2 (Codex-Folgefund, 05.09.2026): der Lock allein
        // reicht nicht — siehe `sollte_persistierten_flug_loeschen`. Die
        // Eigentuemer-Pruefung MUSS innerhalb desselben kritischen
        // Abschnitts liegen, sonst koennte zwischen Lesen und Loeschen
        // wieder ein neuer Schreiber dazwischenkommen.
        let _guard = state
            .persistence_lock
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        let tatsaechliche_pirep_id = read_persisted_flight(app).map(|p| p.pirep_id);
        let soll_loeschen = sollte_persistierten_flug_loeschen(
            erwartete_pirep_id,
            tatsaechliche_pirep_id.as_deref(),
        );
        if soll_loeschen && path.exists() {
            let _ = std::fs::remove_file(&path);
        } else if !soll_loeschen {
            tracing::warn!(
                erwartet = ?erwartete_pirep_id,
                gefunden = ?tatsaechliche_pirep_id,
                "clear_persisted_flight: Ablage gehoert einem anderen (neueren) Flug — NICHT geloescht"
            );
        }
    }
    // v0.8.3 (#2): Auto-Start-Lock zuruecksetzen. Vorher: `auto_start_last_bid_id`
    // wurde nur bei FEHLGESCHLAGENEM flight_start gecleart ([:20616]), bei
    // erfolgreichem Flugstart blieb es bis App-Neustart gesetzt. Effekt: ein
    // zweiter Flug mit dem GLEICHEN bid_id (z.B. Roundtrip-Hin- und Rueckflug
    // ueber denselben Bid, oder Re-Acceptance nach Cancel) wurde stumm vom
    // Auto-Start-Watcher uebersprungen mit der Activity-Log-Meldung
    // "Bid X wurde diese Session schon mal auto-gestartet". Reported von
    // Svenny 2026-05-17 (BE36 TAPA→TRPG → TRPG→TDCF Roundtrip).
    //
    // Beim Aufraeumen des aktiven Flugs ist die Session aus Auto-Start-Sicht
    // abgeschlossen — der naechste Tick darf wieder matchen, egal welcher Bid.
    let mut g = state
        .auto_start_last_bid_id
        .lock()
        .expect("auto_start_last_bid_id lock");
    *g = None;
}

/// Fuehrt `schreiben` nur aus, wenn `stop` noch nicht gesetzt ist —
/// PRUEFUNG und AUSFUEHRUNG in EINEM kritischen Abschnitt unter `lock`.
/// `None` heisst: `stop` war zum Pruef-Zeitpunkt bereits `true`, es wurde
/// gar nichts ausgefuehrt.
///
/// ⚠ Nachgereicht (externe Gegenpruefung Codex, adversarial, waehrend der
/// v1.7.17-QS-Runde entdeckt): losgeloest von `AppHandle`/Tauri, damit die
/// eigentliche Synchronisations-Eigenschaft unabhaengig von Mock-
/// Infrastruktur getestet werden kann — siehe `persistence_lock_tests`.
/// Der Fehler, den diese Funktion behebt: vorher lagen die Pruefung
/// (`!flight.stop.load(...)`, beim Aufrufer) und das Schreiben
/// (`write_persisted_flight`, hier) unsynchronisiert auseinander. Ein
/// `clear_persisted_flight`-Aufruf DAZWISCHEN konnte die Ablage loeschen,
/// und der laengst "erlaubte" Schreibzugriff hatte sie hinterher
/// wiederbelebt — oder schlimmer, die Ablage eines inzwischen
/// gestarteten NEUEN Fluges ueberschrieben (derselbe feste Dateipfad).
fn schreiben_falls_aktiv<R>(
    lock: &Mutex<()>,
    stop: &AtomicBool,
    schreiben: impl FnOnce() -> R,
) -> Option<R> {
    let _guard = lock.lock().unwrap_or_else(|poison| poison.into_inner());
    if stop.load(Ordering::Relaxed) {
        return None;
    }
    Some(schreiben())
}

fn save_active_flight(app: &AppHandle, flight: &ActiveFlight) -> bool {
    let state = app.state::<AppState>();
    schreiben_falls_aktiv(&state.persistence_lock, &flight.stop, || {
        // Snapshot stats inside a short-lived guard so we don't hold the
        // mutex while doing I/O.
        let stats_snapshot = {
            let guard = flight.stats.lock().expect("flight stats");
            PersistedFlightStats::snapshot_from(&guard)
        };
        let persisted = PersistedFlight {
            pirep_id: flight.pirep_id.clone(),
            bid_id: flight.bid_id,
            flight_id: flight.flight_id.clone(),
            plan_strecke_nm: flight.plan_strecke_nm,
            bid_callsign: flight.bid_callsign.clone(),
            pilot_callsign: flight.pilot_callsign.clone(),
            runtime_bundle_id: laufzeit_paket::flug_pin(&flight.pirep_id),
            started_at: flight.started_at,
            // Wird von `write_persisted_flight` gesetzt — hier bewusst None.
            zuletzt_geschrieben: None,
            airline_icao: flight.airline_icao.clone(),
            airline_logo_url: flight.airline_logo_url.clone(),
            planned_registration: flight.planned_registration.clone(),
            flight_number: flight.flight_number.clone(),
            dpt_airport: flight.dpt_airport.clone(),
            arr_airport: flight.arr_airport.clone(),
            fares: flight.fares.clone(),
            stats: stats_snapshot,
            owner_identity: state
                .active_flight_owner_identity
                .lock()
                .expect("active_flight_owner_identity lock")
                .clone(),
        };
        if let Err(e) = write_persisted_flight(app, &persisted) {
            tracing::warn!(error = ?e, "could not persist active flight");
            return false;
        }
        true
    })
    .unwrap_or(false)
}

#[cfg(test)]
#[path = "tests_lib/persistence_lock_tests.rs"]
mod persistence_lock_tests;

#[cfg(test)]
/// Codex-Folgefund (adversarial, 05.09.2026): der `persistence_lock` allein
/// verhindert nur, dass Schreiben und Loeschen sich zeitlich ueberlappen —
/// er sagt nichts darueber, WESSEN Flug an dem einen festen Pfad liegt.
/// `sollte_persistierten_flug_loeschen` ist die daraus extrahierte, reine
/// Eigentuemer-Entscheidung.
#[path = "tests_lib/persistierten_flug_loeschen_eigentuemer_tests.rs"]
mod persistierten_flug_loeschen_eigentuemer_tests;

// ---- Airport cache ----
