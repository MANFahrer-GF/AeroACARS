/// v0.6.0 — phpVMS-Position-Sync-Worker (eigener Background-Task).
///
/// **Architektur-Änderung:** Vorher war phpVMS-POST direkt im
/// Streamer-Tick (mit Critical-Window-Pause-Pattern und drain-Logik).
/// Das war der Ursprung der Bug-Klasse „Streamer hängt auf phpVMS"
/// (Pilot-Reports v0.5.45-v0.5.51).
///
/// Jetzt: separater eigenständiger Worker mit phase-aware POST-Cadence:
///
///   1. **Init-Load (einmalig beim Spawn):** items aus `position_queue.json`
///      die zur aktuellen `pirep_id` gehoeren in die Memory-Outbox laden
///      (Recovery nach App-Restart).
///   2. **Loop alle TICK=1s** (responsive Stop-Check + Backoff-Aufloesung):
///      a. Stop-Check; bei `flight.stop` final persist_outbox + break
///      b. Backoff-Check: wenn `backoff_until > now`, skip
///      c. **Phase-aware Throttle:** `position_interval(phase)` liefert
///         4-30s (Cruise=30s, Pushback=4s, Approach=8s usw). Nur posten
///         wenn `last_post_at.elapsed() >= interval`.
///      d. Bis `MAX_BATCH=50` items aus der Outbox draining
///      e. EIN Batch-POST an `client.post_positions(pirep_id, &batch)`
///         mit `BATCH_TIMEOUT=15s`
///   3. **Bei Erfolg:** `last_position_at` + `queued_position_count`
///      updaten, Backoff-Counter resetten, `last_post_at = now`.
///   4. **Bei 404:** PIREP serverseitig weg, `handle_remote_cancellation`
///      + Worker terminiert sauber.
///   5. **Bei anderem Fehler / Timeout:** kompletter Batch zurueck in die
///      Outbox VORN (chronologische Reihenfolge erhalten via `push_front`
///      in reverse-iter); `consecutive_failures += 1`.
///   6. **Persist-Trigger:** alle 30s (`PERSIST_INTERVAL`) wenn Outbox
///      nicht leer ist, `persist_outbox()` schreibt aktuellen Stand in
///      `position_queue.json` (read-modify-write, bewahrt items anderer
///      pireps). Begrenzt Crash-Verlust auf max ~30s. Im stop-branch
///      wird IMMER persistiert via `persist_outbox_clearing()` (atomar
///      clear+snapshot unter einem Lock-Hold).
///   7. **Exponential Backoff:** bei `consecutive_failures > 0` setzen wir
///      `backoff_until = now + min(60, 3 << (n-1))` Sekunden — verhindert
///      dass wir bei toter Verbindung jede TICK gegen die Wand laufen.
///      Nicht-blocking (loop-top check) damit Stop-Check responsive bleibt.
///
/// **Streamer-Tick MUSS NIE warten** — der pusht nur in die Outbox-
/// VecDeque (= ein schneller `std::sync::Mutex`-Push, ~1µs, kein
/// `.await` involviert).
///
/// **Persistenz:** beim Stop schreibt der Worker den finalen Outbox-
/// Stand in `position_queue.json` (read-modify-write). Beim naechsten
/// App-Start wird das vom neuen Worker im Init-Load (Schritt 1) wieder
/// reingeladen. Datenverlust nur wenn `phpvms_worker_spawned` panics —
/// dann sind die in-memory items weg, aber JSONL-Forensik ist komplett
/// und der Forensik-Upload nach PIREP-Filing bringt sie zurueck.
fn spawn_phpvms_position_worker(app: AppHandle, flight: Arc<ActiveFlight>, client: Client) {
    // Guard gegen Doppel-Spawn (mehrere flight_resume etc.)
    let Some(meine_generation) = startsperre_nehmen(&flight, &flight.phpvms_worker_spawned) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        // Per-Batch-Timeout: 50 Items + retries vom HTTP-Client koennen
        // schon mal 10s dauern. 15s ist grosszuegig genug, ohne den Tick
        // bei toter Verbindung minutenlang zu blockieren.
        // v1.6.14: 15 -> 30 s. Der Wert war bisher WIRKUNGSLOS — der
        // HTTP-Client darunter brach schon nach DEFAULT_TIMEOUT (10 s) ab,
        // die hier dokumentierte Toleranz existierte also nie. Jetzt hat
        // der Positions-POST seinen eigenen Timeout (25 s, siehe
        // api_client::post_positions) und dieser Wert ist die aeussere
        // Sicherung darueber: er greift nur noch, wenn der innere Timeout
        // ausfaellt. Das Warten blockiert nichts — der Stop-Check laeuft
        // waehrenddessen weiter (siehe select! unten).
        const BATCH_TIMEOUT: Duration = Duration::from_secs(30);
        // TICK ist die Loop-Frequenz (Stop-Check + Outbox-Probe). Die
        // ECHTE POST-Cadence kommt aus position_interval(phase) und kann
        // bis 30s im Cruise sein — wir tracken last_post_at und posten
        // nur wenn das Phase-Intervall verstrichen ist. Kurzer TICK
        // braucht damit der stop-Check responsive bleibt + Backoff bei
        // Failure prazise zaehlt.
        const TICK: Duration = Duration::from_secs(1);
        const MAX_BATCH: usize = 50;
        // Exponentiel-Backoff bei consecutive failures: 3s, 6s, 12s,
        // 24s, 48s, gecapped bei 60s. Reset bei erstem Erfolg.
        let mut consecutive_failures: u32 = 0;
        // v1.6.14 — Sichtbarkeit. Requeues liefen bisher NUR in
        // `tracing::warn` und tauchten damit weder im Aktivitaetslog des
        // Piloten noch in GlitchTip auf. Der Dubletten-Befund vom
        // 21.08.2026 musste deshalb rueckwaerts aus der phpVMS-Datenbank
        // rekonstruiert werden. Jetzt: eine Warnung beim ersten Mal (nicht
        // bei jedem — sonst flutet eine tote Leitung das Log) und eine
        // Bilanz beim Flugende.
        let mut requeue_events: u32 = 0;
        let mut requeue_positions: u32 = 0;
        let mut requeue_reported = false;
        // v0.6.1 — last successful POST timestamp fuer phase-aware
        // throttle (siehe position_interval). None = noch nie gepostet
        // (initial Position raus so schnell wie moeglich).
        let mut last_post_at: Option<std::time::Instant> = None;
        // Backoff-deadline: bei consecutive failures wird hier ein
        // future timestamp gesetzt; loop continuiert bis Instant::now()
        // den ueberschreitet. Verhindert dass wir bei toter Verbindung
        // jede TICK gegen die Wand laufen.
        let mut backoff_until: Option<std::time::Instant> = None;
        tracing::info!(pirep_id = %flight.pirep_id, "phpvms-position-worker started");
        // Initial: alte persistente Queue laden falls vorhanden.
        // Errors NICHT mehr silent swallowen — wenn die Datei korrupt
        // ist, soll der Pilot das im Log sehen.
        if let Some(q) = open_position_queue(&app) {
            match q.read_all() {
                Ok(items) if !items.is_empty() => {
                    let mut outbox = flight.position_outbox.lock().expect("position_outbox lock");
                    let mut recovered = 0usize;
                    let mut deserialize_failed = 0usize;
                    // Laden ist idempotent: Was schon in der Outbox steht (gleiche
                    // id), kommt nicht ein zweites Mal hinein — etwa wenn nach
                    // dem Fortsetzen ein ueberholter und der neue Worker beide
                    // laden, bevor der alte endet (Codex-Abnahme 057d5a9c).
                    let mut vorhanden: std::collections::HashSet<String> =
                        outbox.iter().map(|p| p.id.clone()).collect();
                    for item in items {
                        if item.pirep_id == flight.pirep_id {
                            match serde_json::from_value::<api_client::PositionEntry>(item.position)
                            {
                                Ok(p) if !vorhanden.insert(p.id.clone()) => {}
                                Ok(p) => {
                                    outbox.push_back(p);
                                    recovered += 1;
                                }
                                Err(_) => deserialize_failed += 1,
                            }
                        }
                    }
                    tracing::info!(
                        pirep_id = %flight.pirep_id,
                        recovered,
                        deserialize_failed,
                        "loaded persisted positions into outbox on worker-start"
                    );
                }
                Ok(_) => {} // empty queue, normal
                Err(e) => {
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        error = ?e,
                        "queue.json read failed during worker init — \
                         persisted positions for this PIREP are lost"
                    );
                }
            }
        }
        // v0.6.1 — Persist-Strategie: Abwägung zwischen Crash-Recovery
        // (oft persistieren = wenig Datenverlust bei Hardkill) vs. Disk-IO
        // (selten persistieren = weniger File-Rewrites).
        //
        // Strategie: alle PERSIST_INTERVAL=30s wenn Outbox NICHT leer ist.
        // Begrenzt Crash-Verlust auf max 30s an positions, unabhängig
        // vom Backlog. Bei großem Backlog (>500) ist das die gleiche
        // 30s-Cadence — ein zusätzlicher „Backlog-Schutz" wäre redundant
        // (würde die Cooldown verletzen oder dieselbe 30s-Cadence haben).
        //
        // Beim Stop wird IMMER persistiert (final state, ohne Cooldown,
        // via persist_outbox_clearing das atomar clear+snapshot macht).
        //
        // Worst-case Datenverlust für phpVMS Live-Map bei Hardkill:
        // ~30s an positions (= 1-10 Items je nach Phase). JSONL-SoT bleibt
        // unabhängig komplett (per-event-write).
        let mut last_persist_at: Option<std::time::Instant> = None;
        const PERSIST_INTERVAL: Duration = Duration::from_secs(30);
        loop {
            tokio::time::sleep(TICK).await;
            if flight.stop.load(Ordering::Relaxed)
                || flight.generation.load(Ordering::SeqCst) != meine_generation
            {
                // v0.6.1 KRITISCHER FIX — Orphan-Persist-Race:
                // Stop-Pfade machen `outbox.clear() -> stop=true`. Zwischen
                // diesen zwei Aufrufen kann der Streamer-Tick noch 1+ Items
                // in die Outbox geschoben haben (Race-Window klein aber real).
                //
                // Wir benutzen hier `persist_outbox_clearing` welches clear+
                // snapshot ATOMAR unter dem GLEICHEN Lock-Hold macht — kein
                // Streamer-Push zwischen clear und snapshot moeglich.
                //
                // Semantik in allen 5 stop-Paths:
                //   - flight_cancel/forget: User wollte explizit nichts mehr
                //   - flight_end/end_with_overrides: PIREP ist gefiled, JSONL
                //     hat alle Position-Events fuer Forensik-Upload
                //   - handle_remote_cancellation: PIREP serverseitig weg, POST
                //     wuerde eh 404 zurueckgeben
                // → in keinem Fall wollen wir Items aus dem aktuellen Pirep
                //   nach dem Stop noch persistieren.
                //
                // Ausnahme seit 2.0.12: Beim normalen Flugende gehen offene
                // Positionen vorher an den Nachsender (s. `positionen_uebergeben`)
                // — der Worker endet trotzdem sofort wie bisher.
                // Erst das Flag (in `outbox_zum_nachsenden`), dann die
                // Generation lesen — s. `restore_flight_for_retry`.
                let ende = {
                    let mut outbox = flight.position_outbox.lock().expect("position_outbox lock");
                    stopp_entscheidung(&flight, &mut outbox, meine_generation)
                };
                match ende {
                    StoppEnde::Abgeben(rest) => {
                        positionen_uebergeben(&flight, &client, rest);
                        // Nach der Abgabe kann in der Outbox nur noch stehen,
                        // was ein nach Fortsetzen neuer Streamer schon gebracht
                        // hat (alte Punkte gibt der Streamer selbst ab) —
                        // sichern, nicht leeren.
                        persist_outbox(&app, &flight);
                    }
                    StoppEnde::Abgeloest => {
                        // Flug nach gescheitertem Einreichen fortgesetzt: Die
                        // Outbox gehoert dem neuen Worker — nichts leeren.
                        tracing::info!(
                            pirep_id = %flight.pirep_id,
                            "phpvms-position-worker abgeloest (Flug fortgesetzt)"
                        );
                        break;
                    }
                    StoppEnde::Verwerfen => persist_outbox_clearing(&app, &flight),
                }
                if requeue_events > 0 {
                    // Bilanz ins Aktivitaetslog: ohne sie bleibt unsichtbar,
                    // wie oft die Verbindung gezuckt hat. Mit Positions-`id`
                    // sind Wiederholungen folgenlos — die Zahl ist trotzdem
                    // das Fruehwarnsignal fuer Server- oder Leitungsprobleme.
                    log_activity_handle(
                        &app,
                        ActivityLevel::Info,
                        format!(
                            "phpVMS-Positionen: {requeue_events} Wiederholung(en), \
                             {requeue_positions} Positionen erneut gesendet"
                        ),
                        Some(
                            "Doppelte Zeilen entstehen dabei nicht — jede Position traegt \
                             einen eindeutigen Schluessel."
                                .to_string(),
                        ),
                    );
                }
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    requeue_events,
                    requeue_positions,
                    "phpvms-position-worker stopped"
                );
                break;
            }
            // Backoff-Check: bei consecutive failures haben wir ein
            // backoff_until in der Zukunft gesetzt — diese Loop-Iter
            // skipped bis der Zeitpunkt erreicht ist.
            if let Some(until) = backoff_until {
                if std::time::Instant::now() < until {
                    continue;
                }
                backoff_until = None;
            }
            // Aktuelle Phase fuer phase-aware Cadence ziehen.
            let phase = {
                let stats = flight.stats.lock().expect("flight stats");
                stats.phase
            };
            let interval = position_interval(phase);
            // Phase-aware Throttle: nur posten wenn `interval` seit
            // letztem ERFOLG verstrichen ist. Im Cruise = 30s, im
            // Pushback = 4s, im Approach = 8s (siehe position_interval).
            // None = noch nie erfolgreich gepostet -> sofort versuchen.
            let due = match last_post_at {
                None => true,
                Some(t) => t.elapsed() >= interval,
            };
            if !due {
                continue;
            }
            // Bis MAX_BATCH Items aus der Outbox ziehen — std::sync::Mutex,
            // synchroner Block, kein await innen.
            let batch: Vec<api_client::PositionEntry> = {
                let mut outbox = flight.position_outbox.lock().expect("position_outbox lock");
                let n = outbox.len().min(MAX_BATCH);
                outbox.drain(..n).collect()
            };
            if batch.is_empty() {
                continue;
            }
            tracing::debug!(
                pirep_id = %flight.pirep_id,
                batch_size = batch.len(),
                phase = ?phase,
                interval_secs = interval.as_secs(),
                "phpvms-worker batch POST"
            );
            // ECHT BATCHED POST: ein HTTP-Call mit allen N positionen
            // im JSON-Body. War vorher (v0.6.0 initial) ein Loop mit
            // einem POST pro Item — bei 50 items × 50ms RTT = 2.5s pro
            // Drain statt einer 70ms-Anfrage. Der phpVMS-Endpoint
            // /acars/position akzeptiert positions: [...] als Array.
            // v1.6.14 — Der POST darf jetzt bis zu 25 s laufen (vorher brach
            // der HTTP-Client nach 10 s ab und erzeugte damit die Requeues).
            // Damit das Flugende nicht so lange haengt, wacht ein
            // Stop-Waechter parallel mit: bricht der Pilot ab, verlassen wir
            // den Aufruf sofort und die naechste Loop-Iteration geht in den
            // regulaeren Stop-Pfad. Der abgebrochene Request kann
            // serverseitig noch durchlaufen — dank der Positions-`id` ist
            // das idempotent und erzeugt keine Dublette.
            let post_fut = client.post_positions(&flight.pirep_id, &batch);
            let stop_watch = async {
                loop {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    // SeqCst: Wer hier `stop` sieht, muss danach auch das vor
                    // `stop` gesetzte Flag sehen (s. Arm unten).
                    if flight.stop.load(Ordering::SeqCst)
                        || flight.generation.load(Ordering::SeqCst) != meine_generation
                    {
                        return;
                    }
                }
            };
            let post_result = tokio::select! {
                r = tokio::time::timeout(BATCH_TIMEOUT, post_fut) => Some(r),
                _ = stop_watch => None,
            };
            let Some(post_result) = post_result else {
                // Beim normalen Flugende an den Nachsender (die Positions-`id`
                // macht es folgenlos, falls die abgebrochene Anfrage doch ankam).
                // Erst das Flag, dann die Generation (s. `restore_flight_for_retry`).
                if flight.positionen_nachsenden.load(Ordering::SeqCst) {
                    tracing::info!(
                        pirep_id = %flight.pirep_id,
                        batch_size = batch.len(),
                        "phpvms-worker: Stop waehrend laufendem POST — Batch wird nachgesendet"
                    );
                    positionen_uebergeben(&flight, &client, batch);
                } else if flight.generation.load(Ordering::SeqCst) != meine_generation {
                    // Flug fortgesetzt: zurueck in die Outbox, der neue Worker sendet ihn.
                    requeue_batch(&flight, batch);
                } else {
                    tracing::info!(
                        pirep_id = %flight.pirep_id,
                        batch_size = batch.len(),
                        "phpvms-worker: Stop waehrend laufendem POST — Batch verworfen"
                    );
                }
                continue;
            };
            match post_result {
                Ok(Ok(())) => {
                    // Success: queued_count = 0 (NICHT outbox.len()!).
                    //
                    // **WICHTIG zur Semantik:** zwischen drain und jetzt
                    // kann der Streamer was in die Outbox gepusht haben.
                    // outbox.len() waere dann > 0. Das wuerde aber heissen
                    // dass der Indikator "queued (offline)" zeigt obwohl
                    // gerade ein erfolgreicher POST gelaufen ist — exakt
                    // der UX-Bug aus dem Pilot-Report 2026-05-10.
                    //
                    // v0.5.x-Semantik war: queued_count = "stuck items"
                    // (failed POSTs in der file-queue). Im Cruise = 0,
                    // Indikator zeigt "live". Genau das wollen wir.
                    //
                    // Items die der Streamer zwischen drain und jetzt
                    // gepusht hat werden im NAECHSTEN POST mit raus.
                    {
                        let mut stats = flight.stats.lock().expect("flight stats");
                        stats.last_position_at = Some(Utc::now());
                        stats.queued_position_count = 0;
                    }
                    consecutive_failures = 0;
                    last_post_at = Some(std::time::Instant::now());
                    flight
                        .connection_state
                        .store(CONN_STATE_LIVE, Ordering::Relaxed);
                }
                // Ueberholt (Flug inzwischen fortgesetzt): keine Wirkung auf
                // den neuen Worker — Block zurueck, er erfaehrt 404/401 selbst.
                Ok(Err(ApiError::NotFound | ApiError::Unauthenticated | ApiError::Forbidden))
                    if flight.generation.load(Ordering::SeqCst) != meine_generation =>
                {
                    requeue_batch(&flight, batch);
                    return;
                }
                Ok(Err(ApiError::NotFound)) => {
                    // PIREP serverseitig geloescht — terminiert sauber.
                    handle_remote_cancellation(&app, &flight, "phpvms-worker POST");
                    flight.phpvms_worker_spawned.store(false, Ordering::SeqCst);
                    return;
                }
                Ok(Err(e @ ApiError::Unauthenticated)) | Ok(Err(e @ ApiError::Forbidden)) => {
                    // v0.7.17 (B-007): phpVMS hat 401/403 zurueckgegeben.
                    // Typische Ursachen:
                    //   * VA-Admin hat den Pilot-Account gesperrt
                    //     (suspended) oder inaktiv gesetzt
                    //   * API-Key wurde server-side rotated/revoked
                    //   * VA-Permissions wurden geaendert und der Pilot
                    //     hat den ACARS-Endpunkt nicht mehr
                    //
                    // Vor v0.7.17 wurde der Batch endlos requeued —
                    // phpVMS bekam 30 s Takt Spam von einem Account der
                    // gar nicht mehr posten darf. Jetzt: Outbox leeren,
                    // Connection-State BLOCKED, Worker terminieren,
                    // klarer Activity-Log-Eintrag. Pilot kontaktiert
                    // VA-Admin und re-logged-in nach Klaerung.
                    let http_code = match e {
                        ApiError::Unauthenticated => 401,
                        _ => 403,
                    };
                    // Schmeisse den Batch und die noch in der Outbox
                    // wartenden Items weg — retry sinnlos.
                    {
                        let mut outbox =
                            flight.position_outbox.lock().expect("position_outbox lock");
                        outbox.clear();
                    }
                    {
                        let mut stats = flight.stats.lock().expect("flight stats");
                        stats.queued_position_count = 0;
                    }
                    flight
                        .connection_state
                        .store(CONN_STATE_BLOCKED, Ordering::Relaxed);
                    tracing::error!(
                        pirep_id = %flight.pirep_id,
                        http_code = http_code,
                        batch_size = batch.len(),
                        "phpvms-worker terminating: account blocked (401/403). \
                         outbox cleared, no more retries"
                    );
                    log_activity_handle(
                        &app,
                        ActivityLevel::Error,
                        "phpVMS lehnt deine Anfragen ab — Account gesperrt oder inaktiv",
                        Some(format!(
                            "HTTP {http_code}: VA-Admin kontaktieren und nach Freigabe neu einloggen. \
                             Position-Streaming wurde gestoppt."
                        )),
                    );
                    flight.phpvms_worker_spawned.store(false, Ordering::SeqCst);
                    return;
                }
                Ok(Err(e)) => {
                    // Andere Failure — KOMPLETTER batch zurueck in die outbox.
                    // queued_count = outbox.len() (nach requeue) damit der
                    // Indikator den ECHTEN backlog zeigt = stuck items.
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        error = %e,
                        batch_size = batch.len(),
                        "phpvms-worker batch POST failed; queueing entire batch back"
                    );
                    let n = batch.len() as u32;
                    requeue_batch(&flight, batch);
                    requeue_events += 1;
                    requeue_positions += n;
                    if !requeue_reported {
                        requeue_reported = true;
                        log_activity_handle(
                            &app,
                            ActivityLevel::Warn,
                            "phpVMS hat einen Positions-Block nicht bestaetigt — wird erneut gesendet",
                            Some(format!(
                                "{n} Positionen gehen noch einmal raus. Die Aufzeichnung \
                                 laeuft normal weiter; auf der Live-Karte kann der Verlauf \
                                 kurz nachhinken."
                            )),
                        );
                    }
                    let outbox_len = flight
                        .position_outbox
                        .lock()
                        .expect("position_outbox lock")
                        .len();
                    {
                        let mut stats = flight.stats.lock().expect("flight stats");
                        stats.queued_position_count = outbox_len as u32;
                    }
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    flight
                        .connection_state
                        .store(CONN_STATE_FAILING, Ordering::Relaxed);
                }
                Err(_timeout) => {
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        batch_size = batch.len(),
                        "phpvms-worker batch POST timed out (15s); queueing back"
                    );
                    let n = batch.len() as u32;
                    requeue_batch(&flight, batch);
                    requeue_events += 1;
                    requeue_positions += n;
                    if !requeue_reported {
                        requeue_reported = true;
                        log_activity_handle(
                            &app,
                            ActivityLevel::Warn,
                            "phpVMS hat einen Positions-Block nicht bestaetigt — wird erneut gesendet",
                            Some(format!(
                                "{n} Positionen gehen noch einmal raus. Die Aufzeichnung \
                                 laeuft normal weiter; auf der Live-Karte kann der Verlauf \
                                 kurz nachhinken."
                            )),
                        );
                    }
                    let outbox_len = flight
                        .position_outbox
                        .lock()
                        .expect("position_outbox lock")
                        .len();
                    {
                        let mut stats = flight.stats.lock().expect("flight stats");
                        stats.queued_position_count = outbox_len as u32;
                    }
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    flight
                        .connection_state
                        .store(CONN_STATE_FAILING, Ordering::Relaxed);
                }
            }
            // Outbox-Groesse fuer Persist-Check (kein queued_count-Update mehr
            // hier — das wurde im match-arm korrekt mit der richtigen Semantik
            // gesetzt. Vorher war hier ein unconditional update das den
            // success-arm-0-Wert mit dem Race-Condition-Wert ueberschrieb).
            let total_after = flight
                .position_outbox
                .lock()
                .expect("position_outbox lock")
                .len();
            // Persist-Trigger: alle PERSIST_INTERVAL (30s) wenn die Outbox
            // nicht leer ist. Das begrenzt den Crash-Verlust für phpVMS
            // Live-Map auf max ~30s an Positions (= 1-10 Items je nach
            // Phase), unabhängig davon wie groß der Backlog ist.
            //
            // Wichtig: NICHT persistieren wenn outbox leer ist — sonst
            // schreiben wir bei jedem Cooldown-Ablauf sinnlos eine leere
            // file (im Cruise mit gutem Netz wäre das ständig der Fall:
            // Worker drained alle Items → outbox leer → Cooldown abgelaufen
            // → wir würden alle 30s eine leere persist machen).
            //
            // Im Stop-Branch wird via persist_outbox_clearing IMMER
            // geschrieben damit die finale leere file rauskommt + Items
            // anderer pireps erhalten bleiben.
            //
            // Worst-case Datenverlust bei Hardkill: ~30s an positions.
            // JSONL-SoT bleibt komplett (per-event-write).
            if total_after > 0 {
                let cooldown_ok = match last_persist_at {
                    None => true,
                    Some(t) => t.elapsed() >= PERSIST_INTERVAL,
                };
                if cooldown_ok {
                    persist_outbox(&app, &flight);
                    last_persist_at = Some(std::time::Instant::now());
                }
            }
            // Exponentieller Backoff bei consecutive failures (3s, 6s,
            // 12s, 24s, 48s, gecapped 60s). Statt blocking sleep setzen
            // wir backoff_until und der naechste Loop-Top-Check skipped
            // bis dahin — so bleibt der stop-Check responsive (1s Tick).
            if consecutive_failures > 0 {
                let extra_secs = (3u64 << (consecutive_failures - 1).min(5)).min(60);
                backoff_until = Some(std::time::Instant::now() + Duration::from_secs(extra_secs));
                tracing::debug!(
                    pirep_id = %flight.pirep_id,
                    consecutive_failures,
                    extra_sleep_secs = extra_secs,
                    "phpvms-worker backoff after failures"
                );
            }
        }
    });
}

/// Hoechstens so lange sendet der Nachsender offene Positionen eines
/// beendeten Flugs — ein toter Server haelt ihn nicht laenger.
const NACHSENDEN_FRIST: Duration = Duration::from_secs(60);

/// Nach dem Flugende-Stopp, unter der Outbox-Sperre: alles zum Nachsenden
/// herausnehmen. Worker (Stopp-Zweig) und Streamer (verspaeteter Punkt)
/// rufen das unter DERSELBEN Sperre auf — ein Punkt landet entweder noch in
/// der Outbox, bevor der Worker sie leert, oder der Streamer sieht den Stopp
/// und gibt ihn selbst ab. Ohne Flag (Abbruch, Verwerfen, …) `None`.
fn outbox_zum_nachsenden(
    flight: &ActiveFlight,
    outbox: &mut std::collections::VecDeque<api_client::PositionEntry>,
) -> Option<Vec<api_client::PositionEntry>> {
    if !(flight.stop.load(Ordering::SeqCst) && flight.positionen_nachsenden.load(Ordering::SeqCst))
    {
        return None;
    }
    Some(outbox.drain(..).collect())
}

/// Was der Positions-Worker beim Stopp mit der Outbox tut.
#[derive(Debug)]
enum StoppEnde {
    /// Normales Flugende: diese Punkte an den Nachsender.
    Abgeben(Vec<api_client::PositionEntry>),
    /// Flug fortgesetzt, ein neuer Worker besitzt die Outbox: nichts anfassen.
    Abgeloest,
    /// Abbruch, Verwerfen, PIREP weg: wie immer verwerfen.
    Verwerfen,
}

/// Die Stopp-Entscheidung des Workers, unter der Outbox-Sperre. Erst das Flag
/// lesen (in `outbox_zum_nachsenden`), dann die Generation — s.
/// `fortsetzen_vorbereiten`.
fn stopp_entscheidung(
    flight: &ActiveFlight,
    outbox: &mut std::collections::VecDeque<api_client::PositionEntry>,
    meine_generation: u64,
) -> StoppEnde {
    if let Some(rest) = outbox_zum_nachsenden(flight, outbox) {
        return StoppEnde::Abgeben(rest);
    }
    if flight.generation.load(Ordering::SeqCst) != meine_generation {
        return StoppEnde::Abgeloest;
    }
    StoppEnde::Verwerfen
}

/// Positionen eines normal beendeten Flugs an phpVMS nachsenden.
///
/// In einer EIGENEN Aufgabe, die sie allein besitzt — keine gemeinsame
/// Outbox, keine `position_queue.json`, kein Bezug zu einem fortgesetzten
/// oder neuen Flug. Der Worker endet dadurch so schnell wie bisher (Codex-
/// Abnahme ac6a225c: ein bis zu 60 s nachsendender Worker teilte sich die
/// Outbox mit dem nach gescheitertem Einreichen neu gestarteten und die
/// Datei mit dem naechsten Flug). Endet die App vorher, fehlen die Punkte
/// nur in der phpVMS-Spur; im Flugprotokoll stehen sie.
fn positionen_uebergeben(
    flight: &ActiveFlight,
    client: &Client,
    positionen: Vec<api_client::PositionEntry>,
) {
    if positionen.is_empty() {
        return;
    }
    let (client, pirep_id) = (client.clone(), flight.pirep_id.clone());
    tauri::async_runtime::spawn(async move {
        let (nachgesendet, offen) = positionen_nachsenden(
            positionen.into(),
            NACHSENDEN_FRIST,
            Duration::from_secs(30),
            50,
            Duration::from_secs(2),
            |batch| {
                let (client, pirep_id) = (client.clone(), pirep_id.clone());
                async move { client.post_positions(&pirep_id, &batch).await }
            },
        )
        .await;
        if offen > 0 {
            tracing::warn!(
                pirep_id = %pirep_id,
                nachgesendet,
                offen,
                "phpVMS-Positionen nach Flugende nicht alle zugestellt — sie stehen im Flugprotokoll"
            );
        } else {
            tracing::info!(pirep_id = %pirep_id, nachgesendet, "phpVMS-Positionen nach Flugende nachgesendet");
        }
    });
}

/// Positionen senden — bis keine mehr offen sind, phpVMS endgueltig ablehnt
/// (PIREP weg/abgebrochen, Konto gesperrt, 4xx) oder `frist` um ist.
/// Voruebergehende Fehler werden nach `pause` wiederholt. Liefert
/// (nachgesendet, offen).
async fn positionen_nachsenden<F, Fut>(
    mut offen: std::collections::VecDeque<api_client::PositionEntry>,
    frist: Duration,
    batch_timeout: Duration,
    max_batch: usize,
    pause: Duration,
    mut senden: F,
) -> (usize, usize)
where
    F: FnMut(Vec<api_client::PositionEntry>) -> Fut,
    Fut: std::future::Future<Output = Result<(), ApiError>>,
{
    let ende = std::time::Instant::now() + frist;
    let mut nachgesendet = 0usize;
    loop {
        let rest = ende.saturating_duration_since(std::time::Instant::now());
        if rest.is_zero() || offen.is_empty() {
            break;
        }
        let n = offen.len().min(max_batch);
        let batch: Vec<api_client::PositionEntry> = offen.drain(..n).collect();
        let zurueck = |offen: &mut std::collections::VecDeque<api_client::PositionEntry>,
                       batch: Vec<api_client::PositionEntry>| {
            for p in batch.into_iter().rev() {
                offen.push_front(p);
            }
        };
        match tokio::time::timeout(rest.min(batch_timeout), senden(batch.clone())).await {
            Ok(Ok(())) => nachgesendet += batch.len(),
            Ok(Err(
                e @ (ApiError::NotFound
                | ApiError::Unauthenticated
                | ApiError::Forbidden
                | ApiError::Server {
                    status: 400..=499,
                    ..
                }),
            )) => {
                tracing::warn!(error = %e, "phpVMS lehnt nachgesendete Positionen ab");
                zurueck(&mut offen, batch);
                break;
            }
            Ok(Err(_)) | Err(_) => {
                zurueck(&mut offen, batch);
                tokio::time::sleep(pause.min(ende.saturating_duration_since(std::time::Instant::now())))
                    .await;
            }
        }
    }
    (nachgesendet, offen.len())
}

/// v0.6.0 — Helper: failed batch zurueck in die Outbox VORN, mit
/// erhaltener chronologischer Reihenfolge. Wird vom phpVMS-Worker
/// aufgerufen wenn ein Batch-POST scheitert.
///
/// **Wichtig:** `batch.into_iter().rev() + push_front` ergibt die
/// originale Reihenfolge: batch=[a,b,c] -> push_front c, b, a ->
/// outbox = [a, b, c, ...].
fn requeue_batch(flight: &ActiveFlight, batch: Vec<api_client::PositionEntry>) {
    let mut outbox = flight.position_outbox.lock().expect("position_outbox lock");
    for position in batch.into_iter().rev() {
        outbox.push_front(position);
    }
}

/// v0.6.0 — Helper: persistiert die Outbox in `position_queue.json`.
/// Wird beim Worker-Stop und bei großem Outbox-Backlog aufgerufen.
///
/// **Read-modify-write Pattern:** queue.json kann items von anderen
/// pirep_ids enthalten (z.B. nach App-Crash mid-flight eines prior
/// flights, der noch nicht aufgeräumt wurde). Wir lesen alles, filtern
/// die items des aktuellen pirep raus (= „outbox ist die einzige
/// Wahrheit für diesen pirep"), hängen die aktuelle outbox-snapshot
/// dran, und schreiben das kombinierte Set zurück.
///
/// **Empty-outbox-Behandlung:** WICHTIG — wenn die outbox leer ist,
/// MÜSSEN wir trotzdem die queue.json updaten (= aktuelle pirep-items
/// rauslöschen), sonst werden ältere persistierte items beim nächsten
/// App-Start re-loaded und re-posted (Duplikate auf der phpVMS Karte).
fn persist_outbox(app: &AppHandle, flight: &ActiveFlight) {
    persist_outbox_inner(app, flight, false);
}

/// v0.6.1 — Variante von persist_outbox die VOR dem Snapshot die Outbox
/// atomar leert. Wird vom Worker-Stop-Branch aufgerufen damit es keinen
/// Race gibt zwischen `outbox.clear()` und `persist_outbox()`-Lock-
/// Reacquire (Streamer könnte zwischendurch noch was pushen → orphan).
/// Mit `clear_first=true` passiert clear+snapshot unter EINEM Lock-Hold.
fn persist_outbox_clearing(app: &AppHandle, flight: &ActiveFlight) {
    persist_outbox_inner(app, flight, true);
}

fn persist_outbox_inner(app: &AppHandle, flight: &ActiveFlight, clear_first: bool) {
    let Some(queue) = open_position_queue(app) else {
        return;
    };
    // Aktuelle Outbox in Vec snapshotten — Lock so kurz wie möglich.
    // Bei clear_first=true: erst clear, DANN snapshot — beides unter
    // demselben Lock-Hold. Damit kann der Streamer zwischendurch nichts
    // pushen das danach als orphan in queue.json landet.
    // Defensive: wenn serde_json::to_value tatsaechlich mal scheitern
    // sollte (PositionEntry derives Serialize, also extrem unwahrscheinlich),
    // skippen wir den Eintrag und loggen — besser als ein null-Item das
    // beim Re-Load deserialize-Failure produziert.
    let outbox_items: Vec<QueuedPosition> = {
        let mut outbox = flight.position_outbox.lock().expect("position_outbox lock");
        if clear_first {
            outbox.clear();
        }
        outbox
            .iter()
            .filter_map(|p| match serde_json::to_value(p) {
                Ok(v) => Some(QueuedPosition {
                    pirep_id: flight.pirep_id.clone(),
                    position: v,
                }),
                Err(e) => {
                    tracing::error!(
                        pirep_id = %flight.pirep_id,
                        error = ?e,
                        "could not serialize PositionEntry for persist_outbox; skipping"
                    );
                    None
                }
            })
            .collect()
    };
    // Bestehende queue.json lesen, items des aktuellen pirep
    // ausfiltern (= überschreiben), Items anderer pireps behalten.
    let existing_other_pirep: Vec<QueuedPosition> = match queue.read_all() {
        Ok(items) => items
            .into_iter()
            .filter(|i| i.pirep_id != flight.pirep_id)
            .collect(),
        Err(e) => {
            tracing::warn!(error = ?e, "could not read existing queue; will overwrite");
            Vec::new()
        }
    };
    let outbox_count = outbox_items.len();
    let other_count = existing_other_pirep.len();
    let mut combined = existing_other_pirep;
    combined.extend(outbox_items);
    if let Err(e) = queue.replace(&combined) {
        tracing::warn!(error = ?e, "could not persist outbox to position_queue.json");
    } else {
        tracing::info!(
            pirep_id = %flight.pirep_id,
            outbox_persisted = outbox_count,
            kept_from_other_pireps = other_count,
            total_in_file = combined.len(),
            "outbox persisted to position_queue.json"
        );
    }
}

// v0.6.0: spawn_position_queue_drain ist entfallen — `spawn_phpvms_position_worker`
// ist der einzige phpVMS-POST-Pfad, der direkt aus der Memory-Outbox liest.
// Persistenz in `position_queue.json` passiert nur lazy (bei Backlog>500 oder
// Worker-Stop), nicht mehr als primärer Drain-Mechanismus.
