/// Drop local active-flight state without contacting phpVMS. Useful when the
/// stored PIREP is orphaned/dead on the server side and the user wants a
/// clean slate.
#[tauri::command]
async fn flight_forget(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<(), UiError> {
    // Codex-Folgefund (adversarial, 05.09.2026): "kein await zwischen take()
    // und clear_persisted_flight, also keine Race" war falsch — Tauri-
    // Kommandos laufen auf einem MEHR-Thread-Runtime, ein `flight_start` auf
    // einem ANDEREN Thread kann diese ganze Funktion in echter Parallelitaet
    // ueberlappen, unabhaengig von Awaits. Derselbe Schutz wie in
    // `flight_cancel`/`flight_end`: den Guard fuer die GESAMTE Teardown-
    // Dauer halten, damit ein gleichzeitiger `flight_start`/`flight_adopt`
    // entweder klar VOR oder klar NACH dieser Funktion laeuft, nie waehrend.
    let _lifecycle_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;
    // Boarding-Musik (2.1.1): Flugende, Abbruch und Vergessen stoppen sie sofort.
    kabinen_musik::aus();
    let forgotten_pirep_id = if let Some(flight) = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .take()
    {
        // v0.6.0: Outbox VOR stop=true leeren — siehe flight_cancel.
        flight
            .position_outbox
            .lock()
            .expect("position_outbox lock")
            .clear();
        flight.stop.store(true, Ordering::Relaxed);
        tracing::info!(pirep_id = %flight.pirep_id, "active flight forgotten (no phpVMS call)");
        discard_queued_positions_for(&app, &flight.pirep_id);
        record_event(
            &app,
            &flight.pirep_id,
            &FlightLogEvent::FlightEnded {
                timestamp: Utc::now(),
                pirep_id: flight.pirep_id.clone(),
                outcome: FlightOutcome::Forgotten,
            },
        );
        Some(flight.pirep_id.clone())
    } else {
        None
    };
    // `_lifecycle_guard` above already rules out a concurrent flight_start/
    // adopt/end during this whole function — the ownership check here is
    // defense in depth (see `sollte_persistierten_flug_loeschen`). `None`
    // (no flight was active) keeps the previous unconditional-clear
    // behavior for genuinely orphaned files with no known owner.
    clear_persisted_flight(&app, forgotten_pirep_id.as_deref());
    Ok(())
}

// ───────────────────────────────────────────────────────────────────
// v0.7.18 (B-011) Orphan-Flight-Cleanup
//
// Spec: docs/spec/v0.7.18-orphan-flight-cleanup.md
//
// Drei Tauri-Commands:
//   - flight_list_orphans       → IN_PROGRESS PIREPs des Piloten, ohne den
//                                 aktiven lokalen Flight
//   - flight_cancel_orphan      → cancel_pirep + optional delete_bid (mit
//                                 bid_id ODER flight_id Body) + lokale
//                                 Aufraeumung
//   - flight_forget_remote      → nur lokale Aufraeumung, kein API-Call
//
// v0.16.17 ergänzt zwei Dinge:
//   1. Alle Orphan-Konsumenten holen IN_PROGRESS-PIREPs jetzt über
//      `get_user_pireps_in_progress()` (server-seitiges `?state=0`) statt
//      fetch-then-filter über das paginierte `get_user_pireps()` (20/Seite,
//      newest first) — Orphans älter als ~20 Flüge waren vorher unsichtbar
//      (24 Leichen am 2026-06-13 manuell in Prod gelöscht).
//   2. Selbstheilung beim Flugstart: `sweep_stale_orphans_before_prefile`
//      räumt >24-h-alte Leichen automatisch ab, bevor ein neuer PIREP
//      geprefiled wird (siehe unten).
// ───────────────────────────────────────────────────────────────────

/// v0.16.17: Mindestalter, ab dem ein fremder IN_PROGRESS-PIREP beim
/// Flugstart automatisch gecancelt wird (Selbstheilung).
///
/// Warum 24 h: das schützt einen Piloten, der JETZT GERADE auf einem
/// zweiten PC fliegt — ein lebender Parallel-Flug hat zwingend ein
/// frisches `created_at` (Stunden, nicht Tage) und bleibt damit
/// unangetastet. Jüngere Orphans bleiben ebenfalls liegen: für die ist
/// der Resume-Banner-Flow (`flight_discover_resumable`) bzw. das manuelle
/// Clean-Up-Panel zuständig (unverändert).
const ORPHAN_AUTO_CLEANUP_MIN_AGE_HOURS: i64 = 24;

/// Pure Entscheidung: darf der Kandidaten-PIREP beim Flugstart automatisch
/// aufgeräumt (gecancelt) werden?
///
/// Regeln (konservativ — im Zweifel NICHT canceln):
///   - Kandidat == lokal aktiver/resumender Flug → false (nie anfassen)
///   - `created_at` fehlt oder unparsebar       → false (Alter nicht
///     beweisbar; der PIREP bleibt für Resume-Banner + Clean-Up-Panel
///     sichtbar, der Pilot entscheidet)
///   - Alter > [`ORPHAN_AUTO_CLEANUP_MIN_AGE_HOURS`] → true
fn should_auto_cleanup(
    created_at: Option<&str>,
    now: DateTime<Utc>,
    active_pirep_id: Option<&str>,
    candidate_id: &str,
) -> bool {
    if active_pirep_id == Some(candidate_id) {
        return false;
    }
    let Some(created) = created_at.and_then(|s| DateTime::parse_from_rfc3339(s).ok()) else {
        return false;
    };
    now - created.with_timezone(&Utc) > chrono::Duration::hours(ORPHAN_AUTO_CLEANUP_MIN_AGE_HOURS)
}

/// v0.16.17 (Selbstheilung beim Flugstart): cancelt best-effort alle
/// verwaisten IN_PROGRESS-PIREPs älter als 24 h, BEVOR der neue PIREP
/// geprefiled wird. Vorher blieben solche Leichen liegen, bis ein Admin
/// sie löschte — der Pilot sah nur „aircraft not available".
///
/// Nimmt die bereits geholte IN_PROGRESS-Liste entgegen und gibt die
/// **Überlebenden** zurück, damit der Aufrufer seinen Adopt-/Collision-
/// Check auf demselben Fetch fahren kann (kein zweiter API-Call).
///
/// Fehlerphilosophie: jeder Cancel ist best-effort. Ein Fehlschlag wird
/// geloggt, der PIREP bleibt in der Überlebenden-Liste (→ weiterhin im
/// Clean-Up-Panel sichtbar / ggf. adoptierbar) — die Selbstheilung darf
/// den Flugstart NIEMALS blockieren.
async fn sweep_stale_orphans_before_prefile(
    app: &AppHandle,
    client: &Client,
    in_progress: Vec<api_client::PirepSummary>,
) -> Vec<api_client::PirepSummary> {
    // Geschützt: der lokal aktive Flug ODER ein Disk-Resume-Kandidat, den
    // try_resume_flight noch einsammeln will. Beim Flugstart ist
    // active_flight praktisch immer None (der Guard im Command hätte sonst
    // schon abgebrochen), aber der Check kostet nichts; der Disk-Kandidat
    // dagegen ist real — sein PIREP gehört dem Resume-Flow.
    let protected: Option<String> = {
        let state = app.state::<AppState>();
        let guard = state.active_flight.lock().expect("active_flight lock");
        guard.as_ref().map(|f| f.pirep_id.clone())
    }
    .or_else(|| read_persisted_flight(app).map(|p| p.pirep_id));

    let now = Utc::now();
    let mut survivors = Vec::with_capacity(in_progress.len());
    for p in in_progress {
        if !should_auto_cleanup(p.created_at.as_deref(), now, protected.as_deref(), &p.id) {
            survivors.push(p);
            continue;
        }
        match client.cancel_pirep(&p.id).await {
            Ok(()) => {
                // Alter ist hier immer parsebar — should_auto_cleanup hätte
                // sonst false geliefert. Fallback trotzdem defensiv.
                let alter = p
                    .created_at
                    .as_deref()
                    .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                    .map(|dt| {
                        let h = (now - dt.with_timezone(&Utc)).num_hours();
                        if h >= 48 {
                            format!("{} Tage alt", h / 24)
                        } else {
                            format!("{h} h alt")
                        }
                    })
                    .unwrap_or_else(|| "Alter unbekannt".to_string());
                let nr = p.flight_number.as_deref().unwrap_or("?");
                let dep = p.dpt_airport_id.as_deref().unwrap_or("?");
                let arr = p.arr_airport_id.as_deref().unwrap_or("?");
                log_activity_handle(
                    app,
                    ActivityLevel::Info,
                    format!("Verwaister Flug {nr} ({dep}–{arr}, {alter}) automatisch aufgeräumt"),
                    Some(format!(
                        "PIREP {} server-seitig gecancelt (Selbstheilung beim Flugstart)",
                        p.id
                    )),
                );
            }
            Err(e) => {
                tracing::warn!(
                    pirep_id = %p.id,
                    error = %e,
                    "auto-cleanup: cancel_pirep failed — orphan bleibt liegen, Flugstart läuft weiter"
                );
                survivors.push(p);
            }
        }
    }
    survivors
}

#[cfg(test)]
#[path = "tests_lib/orphan_auto_cleanup_tests.rs"]
mod orphan_auto_cleanup_tests;

#[derive(Debug, Clone, Serialize)]
pub struct OrphanFlight {
    pub pirep_id: String,
    pub bid_id: Option<i64>,
    /// phpVMS `flight_id` (alphanumerisch, z.B. "flightid_1") — wird
    /// fuer den `{ flight_id }`-Fallback-Body in `delete_bid` benutzt.
    pub flight_id: Option<String>,
    pub flight_number: Option<String>,
    pub airline_id: Option<i64>,
    pub dpt_airport: Option<String>,
    pub arr_airport: Option<String>,
    pub aircraft_id: Option<i64>,
    pub aircraft_icao: Option<String>,
    pub aircraft_registration: Option<String>,
    /// ISO-8601 UTC timestamp from phpVMS `created_at`. Frontend
    /// konvertiert zu „vor X h Y min".
    pub started_at: Option<String>,
    /// Berechnet vom Backend wenn `started_at` gesetzt ist, sonst None.
    pub age_minutes: Option<i64>,
}

/// `flight_list_orphans` — listet verwaiste PIREPs (state=IN_PROGRESS)
/// auf phpVMS, ohne den lokal aktiven Flight (falls vorhanden).
///
/// Spec: docs/spec/v0.7.18-orphan-flight-cleanup.md §B-011.
#[tauri::command]
async fn flight_list_orphans(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<OrphanFlight>, UiError> {
    let client = current_client(&state)?;

    // Aktiv-Flight-PIREP-ID (falls vorhanden) — diesen filtern wir raus.
    let active_pirep_id: Option<String> = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        guard.as_ref().map(|f| f.pirep_id.clone())
    };

    // 1) IN_PROGRESS-PIREPs holen — v0.16.17: server-seitig gefiltert
    //    (?state=0). Vorher fetch-then-filter über das paginierte
    //    get_user_pireps() (20/Seite, newest first) — Orphans älter als
    //    ~20 Flüge waren im Clean-Up-Panel unsichtbar (24 Leichen am
    //    2026-06-13 manuell in Prod gelöscht).
    let all_pireps = client.get_user_pireps_in_progress().await.map_err(|e| {
        tracing::warn!(error = %e, "flight_list_orphans: get_user_pireps_in_progress failed");
        UiError::from(e)
    })?;
    let in_progress: Vec<_> = all_pireps
        .into_iter()
        .filter(|p| p.state == Some(0)) // defensiv — Server hat bereits gefiltert
        .filter(|p| {
            active_pirep_id
                .as_deref()
                .map(|active| active != p.id)
                .unwrap_or(true)
        })
        .collect();

    if in_progress.is_empty() {
        return Ok(Vec::new());
    }

    // 2) Bids holen fuer Cross-Reference (bid_id / flight_id zu PIREPs zuordnen).
    //    Best-Effort: wenn Bids-API failt, machen wir die Liste ohne Bid-Info.
    let bids = client.get_bids().await.unwrap_or_else(|e| {
        tracing::info!(error = %e, "flight_list_orphans: get_bids failed — continuing without bid info");
        Vec::new()
    });

    // 3) Fleet-Lookup-Map fuer Aircraft-ICAO + Registration via aircraft_id.
    //    Auch best-effort.
    let fleet = client.get_fleet().await.unwrap_or_else(|e| {
        tracing::info!(error = %e, "flight_list_orphans: get_fleet failed — continuing without aircraft info");
        Vec::new()
    });
    let aircraft_lookup: std::collections::HashMap<i64, (Option<String>, Option<String>)> = fleet
        .iter()
        .flat_map(|sf| sf.aircraft.iter())
        .map(|ac| (ac.id, (ac.icao.clone(), ac.registration.clone())))
        .collect();

    // 4) OrphanFlight bauen
    let now = Utc::now();
    let orphans: Vec<OrphanFlight> = in_progress
        .into_iter()
        .map(|p| {
            // Bid-Match via flight_id (PirepSummary.flight_id ↔ Bid.flight.id).
            // Bid.flight.id ist canonical String, PirepSummary.flight_id ist
            // Option<String>.
            let matching_bid = bids.iter().find(|b| {
                p.flight_id
                    .as_ref()
                    .map(|fid| &b.flight.id == fid)
                    .unwrap_or(false)
            });

            // Aircraft-Anreicherung: erst aus PIREP-Summary direkt
            // probieren, dann Fleet-Lookup als Fallback.
            let (aircraft_icao, aircraft_registration) =
                if p.aircraft_icao.is_some() || p.aircraft_registration.is_some() {
                    (p.aircraft_icao.clone(), p.aircraft_registration.clone())
                } else if let Some(aid) = p.aircraft_id {
                    aircraft_lookup.get(&aid).cloned().unwrap_or((None, None))
                } else {
                    (None, None)
                };

            // Age berechnen wenn created_at parsebar ist
            let (started_at, age_minutes) = match p
                .created_at
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            {
                Some(dt) => {
                    let utc_dt = dt.with_timezone(&Utc);
                    let age = (now - utc_dt).num_minutes();
                    (Some(utc_dt.to_rfc3339()), Some(age))
                }
                None => (p.created_at.clone(), None),
            };

            OrphanFlight {
                pirep_id: p.id.clone(),
                bid_id: matching_bid.map(|b| b.id),
                flight_id: p.flight_id.clone(),
                flight_number: p.flight_number.clone(),
                airline_id: p.airline_id,
                dpt_airport: p.dpt_airport_id.clone(),
                arr_airport: p.arr_airport_id.clone(),
                aircraft_id: p.aircraft_id,
                aircraft_icao,
                aircraft_registration,
                started_at,
                age_minutes,
            }
        })
        .collect();

    Ok(orphans)
}

/// `flight_cancel_orphan` — cancelt einen verwaisten PIREP server-side
/// und droppt optional den dazugehoerigen Bid.
///
/// Spec: docs/spec/v0.7.18-orphan-flight-cleanup.md §B-011 Backend-API.
#[tauri::command]
async fn flight_cancel_orphan(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    pirep_id: String,
    bid_id: Option<i64>,
    flight_id: Option<String>,
) -> Result<(), UiError> {
    let client = current_client(&state)?;

    // Sicherheits-Check: NICHT den aktiv laufenden Flight versehentlich
    // ueber den Orphan-Pfad cancellen. Der hat `flight_cancel`.
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if let Some(active) = guard.as_ref() {
            if active.pirep_id == pirep_id {
                return Err(UiError::new(
                    "is_active_flight",
                    "Dieser PIREP ist der aktive Flug — benutze stattdessen den Cancel-Knopf im Cockpit-Tab.",
                ));
            }
        }
    }

    // 1) PIREP cancellen
    match client.cancel_pirep(&pirep_id).await {
        Ok(()) => {
            tracing::info!(pirep_id = %pirep_id, "orphan PIREP cancelled");
        }
        Err(ApiError::NotFound) => {
            // PIREP ist serverseitig schon weg — kein Problem, weiter.
            tracing::info!(
                pirep_id = %pirep_id,
                "orphan PIREP already gone on server (404) — continuing to bid cleanup"
            );
        }
        Err(ApiError::Unauthenticated) | Err(ApiError::Forbidden) => {
            return Err(UiError::new(
                "blocked",
                "phpVMS lehnt deine Anfragen ab — Account gesperrt oder inaktiv. VA-Admin kontaktieren.",
            ));
        }
        Err(e) => {
            tracing::warn!(pirep_id = %pirep_id, error = %e, "cancel_pirep failed");
            return Err(e.into());
        }
    }

    // 2) Bid droppen — nur wenn bid_id ODER flight_id vorhanden ist.
    if bid_id.is_some() || flight_id.is_some() {
        match client.delete_bid(bid_id, flight_id.as_deref()).await {
            Ok(()) => {
                tracing::info!(
                    bid_id = ?bid_id,
                    flight_id = ?flight_id,
                    "orphan bid dropped"
                );
            }
            Err(ApiError::NotFound) => {
                // Bid schon weg — ok
                tracing::info!(bid_id = ?bid_id, "orphan bid already gone on server (404)");
            }
            Err(e) => {
                // PIREP-Cancel war erfolgreich, Bid-Drop nicht — Pilot kann
                // den Bid via phpVMS-UI manuell loeschen. Aircraft sollte
                // schon durch cancel_pirep frei sein.
                tracing::warn!(
                    bid_id = ?bid_id,
                    flight_id = ?flight_id,
                    error = %e,
                    "delete_bid failed after orphan PIREP cancel — Aircraft sollte trotzdem frei sein"
                );
            }
        }
    }

    // 3) Lokale Aufraeumung. Wichtig: NICHT den aktiven Flight anfassen
    // (Sicherheits-Check oben hat schon verhindert dass das hier der
    // aktive PIREP wird).
    discard_queued_positions_for(&app, &pirep_id);
    pirep_queue::remove(&app, &pirep_id);

    log_activity(
        &state,
        ActivityLevel::Warn,
        format!("Verwaister Flug abgebrochen — PIREP {pirep_id}"),
        Some(format!(
            "PIREP cancelled, Bid {:?} dropped, lokale Queues bereinigt.",
            bid_id
        )),
    );
    record_event(
        &app,
        &pirep_id,
        &FlightLogEvent::FlightEnded {
            timestamp: Utc::now(),
            pirep_id: pirep_id.clone(),
            outcome: FlightOutcome::Cancelled,
        },
    );

    Ok(())
}

/// `flight_forget_remote` — lokale Aufraeumung ohne phpVMS-Call.
/// Fuer Faelle wo `cancel_pirep` mit 404 antwortet aber der Pilot
/// sicher ist dass der PIREP wirklich weg ist.
///
/// Spec: docs/spec/v0.7.18-orphan-flight-cleanup.md §B-011.
#[tauri::command]
async fn flight_forget_remote(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    pirep_id: String,
) -> Result<(), UiError> {
    // Selbe Sicherheit wie `flight_cancel_orphan`: nicht den aktiven
    // Flight versehentlich anfassen.
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if let Some(active) = guard.as_ref() {
            if active.pirep_id == pirep_id {
                return Err(UiError::new(
                    "is_active_flight",
                    "Dieser PIREP ist der aktive Flug — benutze stattdessen den Forget-Knopf im Cockpit-Tab.",
                ));
            }
        }
    }

    discard_queued_positions_for(&app, &pirep_id);
    pirep_queue::remove(&app, &pirep_id);

    log_activity(
        &state,
        ActivityLevel::Info,
        format!("Verwaister Flug lokal vergessen — PIREP {pirep_id}"),
        Some("Keine Server-Anfrage gemacht. PIREP-Status auf phpVMS unveraendert.".to_string()),
    );

    Ok(())
}
