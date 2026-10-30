/// Spec sim-disconnect-auto-resume F1 (Auto-Resume) + F2 (Akkumulator)
/// + F3 (Block-Time-Korrektur).
///
/// Zentraler Resume-Helper. Wird von zwei Pfaden aufgerufen:
///   1. Streamer-Loop wenn ein Sim-Snapshot wieder verfuegbar ist
///      und `paused_since.is_some()` — *Auto-Resume*.
///   2. Tauri-Command `flight_resume_after_disconnect` wenn der
///      Pilot manuell den Pause-Button klickt — *Manual-Resume*.
///
/// Beide Pfade laufen durch denselben Code damit:
///   - Pause-Segmente und `pause_total_duration_secs` konsistent
///     akkumuliert werden (= Block-/Flight-Time-Korrektur in der
///     PIREP-Berechnung greift egal welcher Resume-Weg)
///   - `last_lat`/`last_lon` IMMER auf None gesetzt werden, damit
///     Reposition-Distanz nicht ins `distance_nm` einlaeuft
///   - Activity-Log und JSONL gleichen Inhalt sehen
///
/// Returns das fertige `PauseSegment` (falls Pause wirklich aktiv war)
/// — oder None wenn `paused_since.is_none()` (= nichts zu tun).
fn apply_pause_resume(
    app: &AppHandle,
    flight: &Arc<ActiveFlight>,
    current_snap: Option<&SimSnapshot>,
    reason: PauseReason,
) -> Option<PauseSegment> {
    let now = Utc::now();
    let (started_at, last_known, ziel_fuer_sprung) = {
        let stats = flight.stats.lock().expect("flight stats");
        let start = stats.paused_since?;
        (
            start,
            stats.paused_last_known.clone(),
            stats.planned_arr_ref_pos,
        )
    };

    let duration_secs = (now - started_at).num_seconds().max(0);
    // MVP Toggle-Debounce: Pausen unter 1s nicht im Akkumulator
    // verbuchen — verhindert dass eine Esc-Esc-Sequenz oder ein
    // einzelner verlorener Snapshot-Tick die Statistik verfaelscht.
    // Der Pause-State wird trotzdem geclearet.
    let count_toward_accumulator = duration_secs >= 1;

    // Drift-Werte berechnen (None wenn entweder Side fehlt). Anzeige bleibt
    // wie bisher der Betrag — intern (Phase 0, v0.20) wird das vorzeichen-
    // behaftete Delta über `compute_resume_discontinuity` geteilt, damit
    // derselbe Code auch den App-Neustart-Resume-Pfad bedienen kann.
    // Ohne echte Position auf beiden Seiten gibt es nichts zu messen —
    // ein Scheinwert aus dem Menue ergaebe einen Scheinsprung.
    let discontinuity = match (&last_known, current_snap) {
        (Some(prev), Some(cur))
            if position_plausibel(prev.lat, prev.lon, prev.altitude_ft)
                && snapshot_plausibel(cur) =>
        {
            Some(compute_resume_discontinuity(
                prev,
                cur,
                ziel_fuer_sprung,
                // `paused_since` steht erst, wenn der Ausfall ERKANNT ist —
                // der letzte gute Snapshot ist da schon `SIM_DISCONNECT_THRESHOLD_S`
                // alt. Ohne diesen Aufschlag rechnet die Sprung-Pruefung mit einer
                // zu kurzen Luecke und damit einer zu hohen Geschwindigkeit
                // (Cloud-QS 23.09.2026).
                Some(duration_secs + SIM_DISCONNECT_THRESHOLD_S),
            ))
        }
        _ => None,
    };
    let (drift_nm, alt_delta_ft, fuel_delta_kg) = match discontinuity {
        Some(d) => (
            Some(d.drift_nm),
            Some(d.altitude_delta_ft.abs()),
            Some(d.fuel_delta_kg.abs()),
        ),
        None => (None, None, None),
    };

    let segment = PauseSegment {
        started_at,
        ended_at: now,
        duration_secs,
        reason,
        drift_nm,
        altitude_delta_ft: alt_delta_ft,
        fuel_delta_kg,
    };

    {
        let mut stats = flight.stats.lock().expect("flight stats");
        if count_toward_accumulator {
            stats.pause_total_duration_secs = stats
                .pause_total_duration_secs
                .saturating_add(duration_secs);
            stats.pause_segments.push(segment.clone());
        }
        stats.paused_since = None;
        stats.paused_last_known = None;
        stats.current_pause_reason = None;
        // v0.20 (QS-Fix, Finding 3): wie current_pause_reason zuruecksetzen —
        // die Doku hat das immer schon behauptet, der Code tat es aber nie,
        // wodurch JEDE aufgeloeste Pause (auch ein harmloser 31s-Ladebild-
        // schirm) fuer den Rest des Fluges "client_health" mitschleppte.
        stats.disconnect_sim_liveness = None;
        // Reposition-Distanz darf nicht in distance_nm einlaufen —
        // last_lat/lon auf None setzt die Distanz-Baseline neu, der
        // naechste Tick startet frisch (entspricht dem alten Verhalten
        // von flight_resume_after_disconnect).
        stats.last_lat = None;
        stats.last_lon = None;
        stats.last_pos_at = None;
        // v0.20 (QS-Fix, Finding 1): dieser Pfad (Sim-Pause/-Disconnect-
        // Resume INNERHALB desselben laufenden Prozesses) ist der weitaus
        // haeufigere Resume-Fall — deutlich haeufiger als ein kompletter
        // App-Neustart. Bisher berechnete er `discontinuity` nur fuer die
        // Anzeige, wertete `is_impossible_discontinuity` aber nie aus,
        // sodass genau der Vorfall-Typ (Fuel-Sprung/Extrem-Drift waehrend
        // der Sim/App weiterlief) hier durchrutschte und `resume_discontinuity`
        // nie gesetzt wurde. Jetzt dieselbe Klassifikation wie in
        // `try_resume_flight` (Phase 3) — geteilte Logik, nicht dupliziert.
        if let Some(d) = discontinuity {
            if is_impossible_discontinuity(&d) {
                stats.resume_discontinuity = Some(d);
                stats.resume_spruenge.push(d);
            }
        }
    }
    save_active_flight(app, flight);
    if let Some(d) = discontinuity {
        if is_impossible_discontinuity(&d) {
            log_activity_and_record(
                app,
                &flight.pirep_id,
                ActivityLevel::Error,
                format!(
                    "Unmöglicher Sprung beim Wiederaufnehmen erkannt ({})",
                    sprung_grund(&d)
                ),
                Some(format!(
                    "Fuel-Diff {:+.0} kg · Höhen-Diff {:+.0} ft · Drift {:.1} nm — \
                     physikalisch nicht plausibel (kein normaler Verbrauch/Flug), \
                     vermutlich Reload/neuer Ladezustand während der Pause.",
                    d.fuel_delta_kg, d.altitude_delta_ft, d.drift_nm,
                )),
            );
            tracing::warn!(
                pirep_id = %flight.pirep_id,
                fuel_delta_kg = d.fuel_delta_kg,
                altitude_delta_ft = d.altitude_delta_ft,
                drift_nm = d.drift_nm,
                "impossible resume discontinuity detected (apply_pause_resume path)"
            );
        }
    }

    // Activity-Log mit Drift-Stufen aus v0.4-MVP-Spec:
    //   < 1 NM  : Info ohne Drift-Suffix
    //   1-50 NM : Info "repositioniert X nm"
    //   50-200  : Warn "auffaellige Reposition X nm"
    //   > 200   : Warn "Sehr grosse Reposition X nm — pruefe ob der richtige Flug geladen ist"
    //
    // KEIN Drift-Level blockiert Resume (Spec-Prinzip P2: informieren
    // statt blockieren). Der Pilot kann ueber PIREP-Cancel-UI
    // korrigieren wenn er einen falschen Flug erwischt hat.
    let auto_prefix = match reason {
        PauseReason::SimDisconnect => "Flug automatisch fortgesetzt",
        PauseReason::SimPause => "Sim-Pause beendet — Flug fortgesetzt",
        PauseReason::ManualResume => "Flug wiederaufgenommen",
    };
    let (level, msg) = match drift_nm {
        Some(d) if d > crate::grenzwerte::grenzwerte().abgabe.resume_drift_extreme_nm.wert => (
            ActivityLevel::Warn,
            format!(
                "{auto_prefix} — Sehr grosse Reposition {:.1} nm. Pruefe ob der richtige Flug geladen ist.",
                d
            ),
        ),
        Some(d) if d > crate::grenzwerte::grenzwerte().abgabe.resume_drift_warn_nm.wert => (
            ActivityLevel::Warn,
            format!("{auto_prefix} — Auffaellige Reposition {:.1} nm", d),
        ),
        Some(d) if d > crate::grenzwerte::grenzwerte().abgabe.resume_drift_toast_nm.wert => (
            ActivityLevel::Info,
            format!("{auto_prefix} — Repositioniert {:.1} nm", d),
        ),
        Some(_) => (ActivityLevel::Info, auto_prefix.to_string()),
        None => (ActivityLevel::Info, auto_prefix.to_string()),
    };
    let detail = match (&last_known, current_snap) {
        (Some(prev), Some(cur)) => Some(format!(
            "Pause-Dauer: {} s\nVorher: LAT {:.4}° · LON {:.4}° · ALT {:.0} ft\nJetzt:  LAT {:.4}° · LON {:.4}° · ALT {:.0} ft",
            duration_secs, prev.lat, prev.lon, prev.altitude_ft,
            cur.lat, cur.lon, cur.altitude_msl_ft,
        )),
        _ => Some(format!("Pause-Dauer: {} s", duration_secs)),
    };
    log_activity_and_record(app, &flight.pirep_id, level, msg, detail);

    // Spec v0.7.15 F7: Aircraft-Change-Detection nach Resume.
    //
    // Wenn der Sim-Snapshot nach Resume ein anderes Flugzeug-ICAO
    // meldet als der aktive Flug erwartet, ist das ein starkes
    // Signal "falsches Bid geladen". Beispiel: Pilot loadet
    // versehentlich aus dem MSFS-Menue die A320 statt der B738
    // die laut Bid eingetragen ist.
    //
    // Wir blockieren NICHT (Spec-Prinzip P2: informieren statt
    // blockieren) — aber Warnung im Activity-Log mit konkretem
    // Mismatch-Hinweis. Der Pilot kann ueber das normale Bid/PIREP-
    // Cancel-UI korrigieren wenn das wirklich der falsche Flug ist.
    //
    // Vergleich gegen `flight.aircraft_icao` (= Bid-Wert), nicht
    // gegen `paused_last_known.aircraft_icao` — der Bid-Wert ist
    // die "Wahrheit" was geflogen werden soll. Wenn das Pre-Pause-
    // Aircraft schon falsch war, hat das Activity-Log das beim
    // Flug-Start schon protokolliert.
    //
    // Sim-Coverage:
    //   - MSFS: aircraft_icao kommt aus SimConnect (ATC MODEL/ATC ID-
    //     Pfad in crates/sim-msfs/src/adapter/telemetry.rs)
    //   - X-Plane: aircraft_icao kommt aus der X-Plane Web-API ab
    //     v12.1 (sim/aircraft/view/acf_ICAO, siehe
    //     crates/sim-xplane/src/web_api.rs + adapter.rs:264). Wenn
    //     der Pilot die Web-API NICHT in X-Plane → Settings → Network
    //     aktiviert hat, ist `aircraft_icao=None` und F7 ueberspringt
    //     die Pruefung still (= keine falsch-positiven Warnungen).
    //
    // v0.15.12: alias-BEWUSSTER Check. Vorher war das ein stumpfer
    // `snap_icao != bid_icao` Direktvergleich, der WEDER die hardcoded
    // `aircraft_aliases()`-Tabelle NOCH die VPS-gepflegten Aliase las.
    // Folge: Falsch-Warnungen nach Resume trotz passendem Alias (Sim
    // „PHENOM30" vs Bid „E55P"; „B738" vs „B737" — Letzteres feuerte
    // sogar obwohl es in der hardcoded Tabelle steht). Jetzt nutzt der
    // Resume-Check denselben gemeinsamen Helper wie der flight_start-
    // Gate + der Auto-Start-Watcher (`aircraft_type_matches_sim` mit
    // `clean_atc_model`-Normalisierung + Title-Substring-Fallback + VPS-
    // Aliasen), sodass „Alias im Recorder-Admin pflegen → überall ruhig"
    // konsistent gilt. Bleibt eine reine WARNUNG (Prinzip P2: informieren
    // statt blockieren). Sync-Pfad: kein Alias-Refresh nötig — die Aliase
    // wurden beim flight_start (vor jeder Pause) bereits in den State
    // geladen; wir lesen nur den aktuellen Snapshot.
    if let Some(cur) = current_snap {
        let bid_icao = flight.aircraft_icao.trim().to_uppercase();
        let sim_icao = cur.aircraft_icao.as_deref().and_then(clean_atc_model);
        if let (false, Some(actual)) = (bid_icao.is_empty(), sim_icao.as_deref()) {
            let vps_aliases = app
                .state::<AppState>()
                .aircraft_aliases_vps
                .lock()
                .expect("aircraft_aliases_vps lock")
                .clone();
            let sim_title = cur.aircraft_title.as_deref().unwrap_or("");
            if !aircraft_type_matches_sim(
                Some(bid_icao.as_str()),
                Some(actual),
                sim_title,
                &vps_aliases,
            ) {
                log_activity_handle(
                    app,
                    ActivityLevel::Warn,
                    format!(
                        "Aircraft-Mismatch nach Resume: Sim meldet {}, Bid erwartet {}",
                        actual, bid_icao
                    ),
                    Some(format!(
                        "Falls du wirklich ein anderes Flugzeug laedst, cancel den aktuellen PIREP \
                         und starte einen neuen Flug mit dem passenden Bid. Andernfalls lade im Sim \
                         das richtige Flugzeug nach (Bid-ICAO: {bid_icao}). Ist es derselbe Flieger \
                         und der Sim meldet nur einen anderen Typ-String, pflege im Recorder-Admin \
                         einen Aircraft-Alias {bid_icao} auf {actual}."
                    )),
                );
                tracing::warn!(
                    pirep_id = %flight.pirep_id,
                    snap_icao = %actual,
                    bid_icao = %bid_icao,
                    "aircraft mismatch detected after pause-resume"
                );
            }
        }
    }

    tracing::info!(
        pirep_id = %flight.pirep_id,
        duration_secs,
        drift_nm = ?drift_nm,
        reason = ?reason,
        "pause resume applied"
    );

    Some(segment)
}

/// v0.4.1: Pilot klickt „Flug wiederaufnehmen" nachdem der Sim wegbrach
/// und neu positioniert wurde. Hebt den Pause-State in den FlightStats auf
/// — der Streamer fängt im nächsten Tick wieder an Position-Posts an
/// phpVMS zu schicken, der Phase-FSM nimmt die Verarbeitung auf den
/// neuen Sim-Werten wieder auf.
///
/// Loggt eine Repositions-Audit-Zeile mit der Distanz zwischen letzter
/// bekannter Position (vor Disconnect) und der aktuellen Sim-Position
/// (nach Reposition). Bei großen Sprüngen (> REPOSITION_WARN_DELTA_NM)
/// als WARN-Level damit's bei VA-Audits sichtbar bleibt.
///
/// Bewusst KEINE 5-NM/2000-ft-Restriktion wie bei smartCARS — der Pilot
/// entscheidet wo er weitermacht, der Audit-Log macht's nachvollziehbar.
#[tauri::command]
async fn flight_resume_after_disconnect(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), UiError> {
    let flight = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .map(Arc::clone)
        .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?;

    // Pre-Check: Pause-State muss aktiv sein, sonst UI-Fehler zurueck.
    // Der Helper unten returnt sonst nur None und der Caller wuesste
    // nicht warum nichts passierte.
    if flight
        .stats
        .lock()
        .expect("flight stats")
        .paused_since
        .is_none()
    {
        return Err(UiError::new(
            "not_paused",
            "flight is not currently paused — nothing to resume",
        ));
    }

    // Read the current sim snapshot (best-effort) for the delta calculation.
    let current_snap = current_snapshot(&app);

    // Derselbe Riegel wie beim Fortsetzen von selbst: Passt die Lage nicht,
    // entscheidet der Pilot im Banner — der Knopf hier ist kein Umweg. Ohne
    // Sim gibt es nichts zu pruefen; die Pause bleibt (Heartbeat laeuft),
    // sonst schloesse der Knopf sie ungeprueft (Codex-Abnahme 24.10.2026).
    let Some(snap) = current_snap.as_ref() else {
        return Err(UiError::new(
            "sim_disconnected",
            "Sim ist nicht verbunden — der Flug setzt fort, sobald der Sim wieder da ist und die Lage passt",
        ));
    };
    if let Some(abweichung) = lage_abweichung_fuer(&flight, snap) {
        flight.was_just_resumed.store(true, Ordering::SeqCst);
        save_active_flight(&app, &flight);
        log_activity_handle(
            &app,
            ActivityLevel::Warn,
            "Lage passt nicht zum Flug — bitte im Banner bestätigen".to_string(),
            Some(lage_abweichung_text(&abweichung)),
        );
        return Ok(());
    }

    // Spec sim-disconnect-auto-resume: Manual-Resume nutzt denselben
    // Code-Pfad wie Auto-Resume. Pause-Segment wird akkumuliert,
    // last_lat/lon zurueckgesetzt, Activity-Log geschrieben.
    let _segment = apply_pause_resume(
        &app,
        &flight,
        current_snap.as_ref(),
        PauseReason::ManualResume,
    );
    // state ist nur fuer den Tauri-Command-Signatur-Erhalt da — der
    // Helper schreibt das Activity-Log via log_activity_handle, was
    // den State-aware-Pfad ueberbrueckt.
    let _ = state;
    Ok(())
}

/// Spawn the background task that pushes the latest sim snapshot to phpVMS at
/// `POSITION_INTERVAL_SECS`. Stops when `flight.stop` is set or the active
/// flight is replaced.
///
/// CAS-guarded: at most one streamer task per ActiveFlight. Repeat calls are
/// no-ops, which makes it safe to invoke from multiple UI paths
/// (`flight_resume_confirm` racing with itself, StrictMode double-mount, etc.).
/// Spawn a high-rate (~30 Hz) sampler dedicated to populating the
/// touchdown ring buffer in `FlightStats`. Runs alongside the
/// position streamer (which posts to phpVMS at 1–30 s cadence,
/// way too slow for sub-second touchdown capture).
///
/// The actual telemetry rate is bounded by SimConnect's
/// `SIMCONNECT_PERIOD_VISUAL_FRAME` updates (~30 Hz typical) and
/// the adapter worker's 50 ms drain sleep — so in practice the
/// buffer accumulates ~20 samples/second, which still gives 100
/// entries in the 5-second look-back window. That is more than
/// enough to capture the actual touchdown subframe instead of
/// the bounce rebound that the single 1 Hz sample was catching.
///
/// Exits when `flight.stop` is set, just like the streamer.
/// Pfad des Flugprotokolls `pirep_id` (gleiche Abbildung wie der Recorder).
fn flugprotokoll_pfad(app_daten: &std::path::Path, pirep_id: &str) -> std::path::PathBuf {
    let stamm = upload_warteschlange::protokoll_stamm(pirep_id);
    app_daten.join("flight_logs").join(format!("{stamm}.jsonl"))
}

/// Konto der Eingabe-Warteschlange: das angemeldete phpVMS-Konto (stabil
/// ueber Token-Erneuerungen, Codex 30.10.).
fn upload_konto(app: &AppHandle) -> Option<String> {
    app.state::<AppState>()
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
        .map(|id| landungs_eingabe::konto(&id.to_string()))
}

/// Stoesst den Arbeiter der Upload-Warteschlange an (sofortiger Durchlauf).
fn upload_warteschlange_anstossen() {
    upload_anstoss().notify_one();
}

fn upload_anstoss() -> &'static tokio::sync::Notify {
    static N: std::sync::OnceLock<tokio::sync::Notify> = std::sync::OnceLock::new();
    N.get_or_init(tokio::sync::Notify::new)
}

/// P-7.23: immer nur EIN Upload von Protokoll oder Diagnose gleichzeitig.
static UPLOAD_SPERRE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// v3 (P-7.21): Dauer-Arbeiter der Upload-Warteschlange (`pending_uploads/`,
/// siehe `upload_warteschlange`). Laeuft ab dem Start alle 60 s und sofort
/// nach jedem Einreihen; ersetzt den einmaligen Versuch beim Abgeben.
fn spawn_upload_arbeiter(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            uploads_durchlauf(&app).await;
            let _ = tokio::time::timeout(upload_warteschlange::TAKT, upload_anstoss().notified()).await;
        }
    });
}

/// Ein Durchlauf: erst die Eingaben, dann die Flugprotokolle (Reihenfolge
/// je Flug P-7.21).
async fn uploads_durchlauf(app: &AppHandle) {
    let Ok(daten) = app.path().app_data_dir() else { return };
    landungs_eingabe::nachsenden(daten.clone(), upload_konto(app), std::time::Duration::from_secs(30), false).await;
    flugprotokolle_senden(app, &daten).await;
}

/// P-7.21: Vor dem PIREP bei live die Eingaben dieses Laufs senden — ein
/// Versuch, hoechstens 15 s (dieselbe Schranke wie
/// `landung_vor_dem_einreichen_nachschicken`). Scheitert er, bleibt alles in
/// der Warteschlange; der PIREP wartet nicht auf live.
async fn eingabe_vor_dem_bericht(app: &AppHandle) {
    let Ok(daten) = app.path().app_data_dir() else { return };
    let senden = landungs_eingabe::nachsenden(daten, upload_konto(app), std::time::Duration::from_secs(10), true);
    if tokio::time::timeout(std::time::Duration::from_secs(15), senden).await.is_err() {
        tracing::info!("Eingabe vor dem Bericht: live antwortet nicht — die Warteschlange sendet spaeter");
    }
}

/// Sendet die faelligen Flugprotokolle der Warteschlange, je eins (P-7.21,
/// P-7.22). Ein Eintrag wartet, bis sein Besitzer angemeldet ist, sein PIREP
/// die PIREP-Warteschlange verlassen hat und keine Eingabe desselben Flugs
/// mehr offen ist. Fehlt das Protokoll (z. B. von Hand geloescht), gibt es
/// nichts mehr zu senden.
async fn flugprotokolle_senden(app: &AppHandle, daten: &std::path::Path) {
    use upload_warteschlange::{self as uw, Protokoll};
    let eintraege = uw::protokolle(daten);
    if eintraege.is_empty() {
        return;
    }
    let state = app.state::<AppState>();
    let epoche = aktuelle_epoche(&state);
    let angemeldet = *state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock");
    let angemeldet_text = angemeldet.map(|id| id.to_string());
    let aktiv = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .map(|f| f.pirep_id.clone());
    let pireps_offen: std::collections::HashSet<String> =
        pirep_queue::list_all(app).into_iter().map(|q| q.pirep_id).collect();
    let konto = angemeldet.map(|id| landungs_eingabe::konto(&id.to_string()));
    let eingaben_offen = uw::eingaben_offen(daten, konto.as_deref());
    // Nur mit Zugangsdaten DIESES Kontos (siehe `MQTT_KEYRING_PHPVMS_PILOT`).
    let zugang = angemeldet.and_then(|id| {
        let nr = secrets::load_api_key(MQTT_KEYRING_PHPVMS_PILOT).ok().flatten();
        if !mqtt_cache_gehoert_angemeldetem(nr.as_deref(), id) {
            return None;
        }
        Some((
            secrets::load_api_key(MQTT_KEYRING_USERNAME).ok().flatten()?,
            secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten()?,
        ))
    });
    for (pfad, e) in eintraege {
        if !uw::faellig(&pfad, Utc::now().timestamp_millis()) {
            continue;
        }
        let entscheidung = uw::protokoll_entscheiden(
            &e,
            angemeldet_text.as_deref(),
            aktiv.as_deref(),
            &pireps_offen,
            &eingaben_offen,
        );
        if entscheidung != Protokoll::Senden {
            tracing::debug!(pirep_id = %e.pirep_id, ?entscheidung, "Flugprotokoll wartet");
            continue;
        }
        let Some((username, password)) = zugang.as_ref() else {
            continue;
        };
        let log_path = flugprotokoll_pfad(daten, &e.pirep_id);
        // Erst lesen, wenn alles geschrieben ist (Schreib-Faden, siehe
        // `record_event`) — sonst fehlten Aufsetzfenster und Analyse.
        let _ = tokio::task::spawn_blocking(flugprotokoll_abwarten).await;
        // Nur ein sicher fehlendes Protokoll beendet den Eintrag; ein
        // Lesefehler ist ein Fehlversuch (Codex 30.10.).
        match std::fs::metadata(&log_path) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                tracing::warn!(pirep_id = %e.pirep_id, "Flugprotokoll fehlt lokal — Eintrag entfernt");
                uw::erledigt(&pfad);
                continue;
            }
            Err(err) => {
                uw::fehlschlag(&pfad, Utc::now().timestamp_millis(), &format!("Protokoll nicht lesbar: {err}"));
                continue;
            }
        }
        let _sperre = UPLOAD_SPERRE.lock().await;
        // Kontowechsel waehrend des Durchlaufs: nichts mit fremden Daten senden.
        if aktuelle_epoche(&state) != epoche {
            tracing::info!("Flugprotokolle: Sitzung gewechselt — Durchlauf abgebrochen");
            return;
        }
        match aeroacars_mqtt::log_upload::upload_flight_log(
            &log_path, &e.pirep_id, username, password,
            None, // default endpoint = https://live.kant.ovh/api/flight-logs/upload
        )
        .await
        {
            Ok(stats) => {
                uw::erledigt(&pfad);
                tracing::info!(
                    pirep_id = %e.pirep_id,
                    raw_kb = stats.raw_size / 1024,
                    gzip_kb = stats.compressed_size / 1024,
                    "flight log uploaded",
                );
                // v0.5.23: Pilot-UI-Feedback im Activity-Log damit der
                // Pilot sieht dass der Upload geklappt hat. Detail-Spalte
                // zeigt die Groessen-Statistik fuer Debugging.
                log_activity_handle(
                    app,
                    ActivityLevel::Info,
                    "Flight log uploaded to live-tracking server",
                    Some(format!(
                        "{} KB raw, {} KB gzip ({}% Kompression)",
                        stats.raw_size / 1024,
                        stats.compressed_size / 1024,
                        ((stats.compressed_size as f64 / stats.raw_size as f64) * 100.0) as i32,
                    )),
                );
            }
            Err(err) => {
                let erstmals = uw::versuch(&pfad).versuche == 0;
                uw::fehlschlag(&pfad, Utc::now().timestamp_millis(), &err.to_string());
                tracing::warn!(
                    pirep_id = %e.pirep_id,
                    error = %err,
                    "flight log upload failed — Warteschlange versucht es erneut",
                );
                // Nur beim ersten Fehlschlag ins Aktivitaetsprotokoll; die
                // Anzeige „Upload ausstehend“ (P-7.24) zeigt den Rest.
                if erstmals {
                    log_activity_handle(
                        app,
                        ActivityLevel::Warn,
                        "Flight log upload failed — wird automatisch wiederholt",
                        Some(format!(
                            "{} — Log liegt lokal in flight_logs/{}.jsonl",
                            err, e.pirep_id
                        )),
                    );
                }
            }
        }
    }
}

/// v3 (P-3.04): Hinweis, wenn das Laufzeit-Paket seit mehr als 30 Tagen
/// nicht erfolgreich bei live geprueft wurde — im Aktivitaetsprotokoll (mit
/// Systemmeldung). Der aktive Stand laeuft weiter.
fn paket_pruefung_hinweis(app: &AppHandle) -> impl FnOnce(i64) + Send + 'static {
    let app = app.clone();
    move |tage| {
        tracing::warn!(tage, "Laufzeit-Paket seit ueber 30 Tagen nicht erfolgreich geprueft");
        log_activity_handle(
            &app,
            ActivityLevel::Warn,
            format!("Bewertungsregeln seit {tage} Tagen nicht geprüft"),
            Some(
                "AeroACARS konnte den Stand der Bewertungsregeln nicht beim Live-Server abfragen und \
                 rechnet mit dem vorhandenen Stand weiter. Bitte Internetverbindung prüfen; \
                 hält das an, beim GSG-Team melden."
                    .to_string(),
            ),
        );
    }
}

/// v3 (P-7.24): ausstehende Uploads des angemeldeten Kontos — je Flug Art,
/// „seit“, Zahl der Fehlversuche und Grund des letzten Fehlschlags. Fremde
/// Konten bleiben verborgen.
#[tauri::command]
fn uploads_ausstehend(app: AppHandle) -> Vec<upload_warteschlange::Ausstehend> {
    let Ok(daten) = app.path().app_data_dir() else { return Vec::new() };
    let besitzer = app
        .state::<AppState>()
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
        .map(|id| id.to_string());
    let konto = besitzer.as_deref().map(landungs_eingabe::konto);
    upload_warteschlange::ausstehend(&daten, konto.as_deref(), besitzer.as_deref())
}

/// Reiht das Flugprotokoll ein (P-7.21; der Arbeiter sendet es) und laedt
/// das DIAGNOSE-Log hoch.
///
/// Besitzer des Eintrags ist `owner_identity` (wer einreichte), sonst der
/// gerade Angemeldete. Die Diagnose geht wie bisher sofort und best effort.
/// `nur_diagnose`: nur die technischen Zeilen hochladen, NICHT das
/// Flugprotokoll. Für abgebrochene Flüge: Deren Protokoll darf nicht in den
/// Import, sonst entstünde eine Sitzung für einen Flug, den phpVMS so nie
/// gesehen hat — die Diagnose brauchen wir aber gerade dort (23.09.2026).
fn spawn_flight_log_upload(
    app: &AppHandle,
    pirep_id: String,
    owner_identity: Option<String>,
    nur_diagnose: bool,
) {
    if !nur_diagnose {
        let besitzer = owner_identity.clone().or_else(|| {
            app.state::<AppState>()
                .authenticated_pilot_id
                .lock()
                .expect("authenticated_pilot_id lock")
                .map(|id| id.to_string())
        });
        match (app.path().app_data_dir(), besitzer) {
            (Ok(daten), Some(besitzer)) if flugprotokoll_pfad(&daten, &pirep_id).exists() => {
                match upload_warteschlange::protokoll_einreihen(&daten, &pirep_id, &besitzer) {
                    Ok(_) => upload_warteschlange_anstossen(),
                    Err(e) => tracing::warn!(pirep_id = %pirep_id, error = %e, "Flugprotokoll nicht einreihbar"),
                }
            }
            (Ok(_), Some(_)) => tracing::debug!(pirep_id = %pirep_id, "kein Flugprotokoll — nur Diagnose"),
            _ => tracing::warn!(pirep_id = %pirep_id, "Flugprotokoll ohne Besitzer — nicht eingereiht"),
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): dieser
        // Task laeuft ERST NACH `tauri::async_runtime::spawn` irgendwann
        // spaeter — jede Identitaets-Pruefung am AUFRUFORT (spawn-Zeit)
        // sagt nichts darueber, wer noch angemeldet ist, WENN dieser Task
        // tatsaechlich die Keyring-Credentials unten liest. Erst hier, im
        // Moment des eigentlichen Lesens, nachpruefen — sonst haette ein
        // Kontowechsel zwischen Spawn und Ausfuehrung Pilot As Diagnose
        // mit Pilot Bs MQTT-Credentials hochladen koennen.
        if let Some(erwartet) = owner_identity.as_deref() {
            let aktuell = app
                .state::<AppState>()
                .authenticated_pilot_id
                .lock()
                .expect("authenticated_pilot_id lock")
                .map(|id| id.to_string());
            if aktuell.as_deref() != Some(erwartet) {
                tracing::warn!(
                    pirep_id = %pirep_id,
                    "log-upload: Konto wechselte seit dem Filing — Diagnose uebersprungen"
                );
                return;
            }
        }

        // MQTT-Credentials aus Keyring (= identisch zu HTTP-Basic).
        let username = match secrets::load_api_key(MQTT_KEYRING_USERNAME) {
            Ok(Some(u)) => u,
            _ => {
                tracing::warn!("log-upload: no MQTT username in keyring — skip");
                return;
            }
        };
        let password = match secrets::load_api_key(MQTT_KEYRING_PASSWORD) {
            Ok(Some(p)) => p,
            _ => {
                tracing::warn!("log-upload: no MQTT password in keyring — skip");
                return;
            }
        };

        // v1.7.41: das DIAGNOSE-Log gleich mit.
        //
        // Am 20.09.2026 riss bei mehreren Piloten die Verbindung im
        // Minutentakt. Serverseitig sah das wie ein Netzproblem aus; die
        // Zeile, die den Fehler benannte ("Cannot send packet of size
        // 10379 ... maximum 10240"), stand ausschliesslich auf dem
        // Rechner des Piloten. Die Suche dauerte zwei Tage und endete
        // erst, als jemand seine Logdatei schickte.
        //
        // Best effort: Fehlt die Datei oder scheitert der Upload, bleibt
        // es bei einer Logzeile — der Flugbericht ist wichtiger.
        //
        // NICHT bei selbst eingeschalteter Fehlersuche: Mit `RUST_LOG=trace`
        // schreibt rumqttc seine Pakete mit, und im CONNECT stehen Name und
        // Passwort. Wer so debuggt, schickt seine Datei bewusst von Hand
        // (Abnahme 20.09.2026).
        if std::env::var_os("RUST_LOG").is_some() {
            tracing::info!(
                "Diagnose-Log NICHT hochgeladen: RUST_LOG ist gesetzt, die Datei \
                 koennte Zugangsdaten enthalten"
            );
        } else {
            let dateien = diagnose_logs_fuer_upload();
            if !dateien.is_empty() {
                let _sperre = UPLOAD_SPERRE.lock().await;
                match aeroacars_mqtt::log_upload::upload_diagnose_logs(
                    &dateien, &pirep_id, &username, &password, None,
                )
                .await
                {
                    Ok(stats) => {
                        tracing::info!(
                            pirep_id = %pirep_id,
                            gzip_kb = stats.compressed_size / 1024,
                            "Diagnose-Log mitgeschickt"
                        );
                        // Sichtbar machen, was den Server erreicht.
                        log_activity_handle(
                            &app,
                            ActivityLevel::Info,
                            "Diagnose-Protokoll mitgeschickt",
                            Some(format!(
                                "{} KB — technische Meldungen des Clients, keine Zugangsdaten",
                                stats.compressed_size / 1024
                            )),
                        );
                    }
                    Err(e) => tracing::warn!(
                        pirep_id = %pirep_id,
                        error = %e,
                        "Diagnose-Log nicht hochgeladen (nicht schlimm)"
                    ),
                }
            }
        }
    });
}
