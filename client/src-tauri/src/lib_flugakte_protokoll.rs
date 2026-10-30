/// Prüfstatus der eigenen PIREPs beim Live-Server (Befund DLH 880).
///
/// Der Landungs-Tab zeigt damit, ob ein Flug im Integritäts-Gate festhängt,
/// warum, und ob er inzwischen freigegeben wurde. Ohne Live-Server-Zugang
/// (nicht provisioniert) kommt eine leere Liste — die Anzeige bleibt dann
/// einfach aus, statt einen Fehler zu melden.
#[tauri::command]
async fn pirep_pruefstatus(
    app: AppHandle,
    pirep_ids: Vec<String>,
) -> Result<Vec<aeroacars_mqtt::pirep_status::PirepPruefstatus>, UiError> {
    // Codex-Abnahme 15.09.2026: Die Zugangsdaten im Schlüsselbund können
    // noch einem VORHERIGEN Piloten gehören (Kontowechsel ohne Abmelden,
    // fehlgeschlagenes Löschen). Dann würde als dieser abgefragt — seine
    // Prüfgründe erschienen beim neuen Piloten. Nur abfragen, wenn die
    // gespeicherte Piloten-ID zum angemeldeten Konto passt, und eine Antwort
    // verwerfen, wenn sich die Sitzung währenddessen geändert hat.
    let state = app.state::<AppState>();
    let epoche_vorher = *state.session_epoch.lock().expect("session_epoch lock");
    let angemeldet = *state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock");
    let Some(angemeldet) = angemeldet else {
        return Ok(Vec::new());
    };
    let (Some(username), Some(password)) = (
        secrets::load_api_key(MQTT_KEYRING_USERNAME).ok().flatten(),
        secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten(),
    ) else {
        return Ok(Vec::new());
    };
    // Befund 21.09.2026: vorher Recorder-ID gegen Pilotennummer — bei 13
    // von 17 GSG-Piloten nie gleich, der Pruefstatus blieb fuer sie immer
    // leer (siehe `MQTT_KEYRING_PHPVMS_PILOT`).
    let schluessel_nr = secrets::load_api_key(MQTT_KEYRING_PHPVMS_PILOT)
        .ok()
        .flatten();
    if !mqtt_cache_gehoert_angemeldetem(schluessel_nr.as_deref(), angemeldet) {
        tracing::debug!(
            "pirep_pruefstatus: Schlüsselbund gehört nicht zum angemeldeten Piloten — übersprungen"
        );
        return Ok(Vec::new());
    }
    let antwort =
        aeroacars_mqtt::pirep_status::pruefstatus_abrufen(&pirep_ids, &username, &password, None)
            .await
            .map_err(|e| UiError::new("pirep_pruefstatus", e.to_string()))?;
    if *state.session_epoch.lock().expect("session_epoch lock") != epoche_vorher {
        return Ok(Vec::new());
    }
    Ok(antwort)
}

/// Flugakte eines eigenen PIREPs für die Flugkarte (Etappe 3, 28.10.2026).
///
/// Der Live-Server rechnet sie aus dem hochgeladenen Flugprotokoll — dieselbe
/// Rechnung wie in der Admin-Webapp. `Ok(None)`: zu diesem Flug gibt es kein
/// Protokoll auf dem Server. Ohne Live-Server-Zugang oder bei fremdem
/// Schlüsselbund ein Fehler mit eigenem Code, damit die Karte das sagen kann.
#[tauri::command]
async fn flugakte_laden(app: AppHandle, pirep_id: String) -> Result<Option<String>, UiError> {
    // Wie `pirep_pruefstatus`: nur mit dem Zugang des angemeldeten Piloten
    // fragen und eine Antwort verwerfen, wenn die Sitzung inzwischen wechselte.
    let state = app.state::<AppState>();
    let epoche_vorher = *state.session_epoch.lock().expect("session_epoch lock");
    let angemeldet = *state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock");
    let Some(angemeldet) = angemeldet else {
        return Err(UiError::new("flugakte_kein_zugang", "nicht angemeldet"));
    };
    let (Some(username), Some(password)) = (
        secrets::load_api_key(MQTT_KEYRING_USERNAME).ok().flatten(),
        secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten(),
    ) else {
        return Err(UiError::new("flugakte_kein_zugang", "kein Live-Server-Zugang"));
    };
    let schluessel_nr = secrets::load_api_key(MQTT_KEYRING_PHPVMS_PILOT)
        .ok()
        .flatten();
    if !mqtt_cache_gehoert_angemeldetem(schluessel_nr.as_deref(), angemeldet) {
        return Err(UiError::new("flugakte_kein_zugang", "Schlüsselbund gehört nicht zum angemeldeten Piloten"));
    }
    let abruf = aeroacars_mqtt::flugakte::flugakte_abrufen(&pirep_id, &username, &password, None)
        .await
        .map_err(|e| UiError::new("flugakte_laden", e.to_string()))?;
    if *state.session_epoch.lock().expect("session_epoch lock") != epoche_vorher {
        return Err(UiError::new("flugakte_kein_zugang", "Sitzung gewechselt"));
    }
    match abruf {
        aeroacars_mqtt::flugakte::Abruf::Akte(text) => Ok(Some(text)),
        aeroacars_mqtt::flugakte::Abruf::Fehlt => Ok(None),
        // abgelaufenes oder abgelehntes Zugangspaar: derselbe Hinweis wie ohne Zugang
        aeroacars_mqtt::flugakte::Abruf::KeinZugang => Err(UiError::new("flugakte_kein_zugang", "Live-Server lehnt den Zugang ab")),
    }
}

#[tauri::command]
async fn landing_backup_now(app: AppHandle) -> Result<usize, UiError> {
    upload_landing_backup(app)
        .await
        .map_err(|e| UiError::new("landing_backup_failed", e))
}

#[tauri::command]
async fn landing_backup_restore(app: AppHandle) -> Result<usize, UiError> {
    restore_landing_backup(app)
        .await
        .map_err(|e| UiError::new("landing_restore_failed", e))
}

/// Best-effort append to the flight log. Swallows errors so a missing
/// disk doesn't surface as a UI failure — the log is purely for replay
/// and post-flight analysis.
/// Einen Eintrag ins Flugprotokoll (`flight_logs/<pirep>.jsonl`) schreiben —
/// über einen eigenen Schreib-Faden, in der Reihenfolge der Aufrufe.
///
/// Bis 2.0.9 öffnete, schrieb und schloss jeder Aufruf die Datei synchron —
/// im Streamer bei jedem Takt, im Aufsetz-Sampler nach dem Aufsetzen mehrere
/// hundert KB (AZU 8710, 11.10.2026). Wer die Datei LIEST, wartet vorher mit
/// `flugprotokoll_abwarten`, bis alles geschrieben ist (Upload, Nachreichen,
/// App-Ende).
/// v3 (P-5.01, P-3.30): der gepinnte Paketstand ins Flugprotokoll — bei
/// Flugbeginn, gleich nach `FlightStarted`.
fn paket_stand_vermerken(app: &AppHandle, pirep_id: &str) {
    record_event(
        app,
        pirep_id,
        &FlightLogEvent::PaketStand {
            timestamp: Utc::now(),
            pirep_id: pirep_id.to_string(),
            runtime_bundle_id: laufzeit_paket::flug_pin(pirep_id).map(|id| id.to_string()),
        },
    );
}

/// Der Anflugpuffer, wie er beim Aufsetzen steht: 120 Proben fuer die Note
/// (`compute_approach_stddev`), 400 fuer die Anflug-Forensik. Eine Stelle
/// fuer `roh.anflugpuffer` des Eingabe-Dokuments und das Ereignis
/// `AnflugPuffer` (P-5.01).
fn anflugpuffer_json(stats: &FlightStats) -> serde_json::Value {
    serde_json::json!({
        "note": stats.approach_buffer,
        "forensik": stats.anflug_forensik_puffer,
    })
}

fn record_event(app: &AppHandle, pirep_id: &str, event: &FlightLogEvent) {
    // Stau melden (Platte hängt?) — einmal je 20.000 wartende Einträge.
    // Verworfen wird nichts (Codex-Gesamtabnahme 12.10.2026).
    let offen = PROTOKOLL_OFFEN.fetch_add(1, Ordering::SeqCst) + 1;
    if offen % 20_000 == 0 {
        tracing::warn!(offen, "Flugprotokoll: Schreib-Faden staut sich");
    }
    let _ = flugprotokoll_schreiber().send(ProtokollAuftrag::Eintrag(
        app.clone(),
        pirep_id.to_string(),
        Box::new(event.clone()),
    ));
}

/// Gemeldete, noch nicht geschriebene Flugprotokoll-Einträge.
static PROTOKOLL_OFFEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

enum ProtokollAuftrag {
    Eintrag(AppHandle, String, Box<FlightLogEvent>),
    Abwarten(std::sync::mpsc::SyncSender<()>),
}

fn flugprotokoll_schreiber() -> std::sync::mpsc::Sender<ProtokollAuftrag> {
    static SCHREIBER: OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<ProtokollAuftrag>>> =
        OnceLock::new();
    SCHREIBER
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<ProtokollAuftrag>();
            std::thread::spawn(move || {
                while let Ok(auftrag) = rx.recv() {
                    match auftrag {
                        ProtokollAuftrag::Eintrag(app, pirep_id, event) => {
                            if let Some(rec) = open_flight_recorder(&app, &pirep_id) {
                                if let Err(e) = rec.append(&event) {
                                    tracing::warn!(error = ?e, "could not append to flight log");
                                }
                            }
                            PROTOKOLL_OFFEN.fetch_sub(1, Ordering::SeqCst);
                        }
                        ProtokollAuftrag::Abwarten(fertig) => {
                            let _ = fertig.send(());
                        }
                    }
                }
            });
            std::sync::Mutex::new(tx)
        })
        .lock()
        .map(|tx| tx.clone())
        .expect("flugprotokoll-schreiber lock")
}

/// Warten, bis alle bisher gemeldeten Einträge in der Datei stehen
/// (höchstens 10 s). Blockiert — aus asynchronem Code über `spawn_blocking`.
fn flugprotokoll_abwarten() {
    flugprotokoll_abwarten_hoechstens(Duration::from_secs(10));
}

fn flugprotokoll_abwarten_hoechstens(grenze: Duration) {
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    if flugprotokoll_schreiber().send(ProtokollAuftrag::Abwarten(tx)).is_ok() {
        let _ = rx.recv_timeout(grenze);
    }
}

// v0.6.0: drain_position_queue ist entfallen — der phpVMS-Worker macht
// den Drain direkt aus der Memory-Outbox. Die persistente Queue
// (`position_queue.json`) wird nur noch beim Worker-Start eingelesen
// (für Recovery nach App-Restart) und beim Worker-Stop / Backlog>500
// rausgeschrieben. Kein in-Tick-Drain-Pattern mehr — strukturell
// unmöglich dass irgendwas den Streamer-Tick blockiert.
