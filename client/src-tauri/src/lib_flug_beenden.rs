/// `GET /metar/{icao}` — fetch the current METAR for an airport.
/// Cached per-flight in `FlightStats` (departure + arrival), so the
/// dashboard / future briefing panel can reuse it without round-trips.
/// Errors map to a serializable `UiError` so the frontend can show a
/// localized message instead of a Rust enum.
#[tauri::command]
async fn metar_get(icao: String) -> Result<MetarSnapshot, UiError> {
    metar::fetch_metar(&icao).await.map_err(|e| match e {
        MetarError::NotFound(_) => UiError::new("metar_not_found", e.to_string()),
        MetarError::Network(_) => UiError::new("metar_network", e.to_string()),
        MetarError::Status(_) => UiError::new("metar_upstream", e.to_string()),
        MetarError::Parse(_) => UiError::new("metar_parse", e.to_string()),
    })
}

/// StreamDeck (Thomas 23.10.2026: „der Wind war gar nicht geladen“): METAR
/// für Start oder Ziel des aktiven Flugs jetzt holen, zur ANZEIGE.
///
/// Das Ziel-METAR holt der Streamer regulär erst im Sinkflug/Endanflug; davor
/// zeigt das Deck „kein METAR“. Hier wird bewusst nur `*_metar_snap`
/// geschrieben, NICHT `*_metar_raw`: der Rohtext geht in PIREP und
/// Landungsdatensatz — ein im Reiseflug nachgeladenes Wetter darf dort nicht
/// landen, falls der spätere Abruf scheitert. Die `*_requested`-Flags bleiben
/// unberührt, der reguläre Abruf läuft also weiter und überschreibt.
/// Höchstens ein erfolgreicher Abruf je Flughafen pro Minute
/// (aviationweather.gov schonen); ein Fehlschlag gibt die Sperre sofort frei.
pub(crate) async fn metar_nachladen(app: &AppHandle, ziel: bool) -> Result<(), UiError> {
    static ZULETZT: std::sync::OnceLock<Mutex<HashMap<String, std::time::Instant>>> = std::sync::OnceLock::new();
    let zuletzt = || ZULETZT.get_or_init(|| Mutex::new(HashMap::new())).lock().expect("metar nachladen");
    let flight = app
        .state::<AppState>()
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .cloned()
        .ok_or_else(|| UiError::new("kein_flug", "kein aktiver Flug"))?;
    let icao = if ziel { flight.arr_airport.clone() } else { flight.dpt_airport.clone() };
    {
        let mut z = zuletzt();
        if z.get(&icao).is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(60)) {
            return Ok(());
        }
        z.insert(icao.clone(), std::time::Instant::now());
    }
    let snap = match metar_get(icao.clone()).await {
        Ok(snap) => snap,
        Err(e) => {
            zuletzt().remove(&icao);
            return Err(e);
        }
    };
    let mut stats = flight.stats.lock().expect("flight stats");
    if ziel {
        stats.arr_metar_snap = Some(snap);
    } else {
        stats.dep_metar_snap = Some(snap);
    }
    Ok(())
}

/// Distance (nm) from the live sim position to the airport with the given ICAO.
/// `None` when we have no sim snapshot yet, or no coordinates for the airport
/// from any source — in which case the caller must not gate on it.
///
/// Used by `flight_end` to enforce "you have to actually be there to file".
///
/// v0.19.3: this gate has to answer "is the pilot at his destination" the same
/// way `arrival::locate` does, or the two contradict each other about the same
/// parked aircraft — and this one is the side that can BLOCK a filing.
///
/// It therefore takes the SHORTEST distance any of our sources reports, rather
/// than believing the first one that answers:
///
///   * runway thresholds (OurAirports, embedded), and
///   * the airport's reference point (Navigraph → phpVMS → OurAirports, see
///     `airport_reference_pos`).
///
/// The reason is the same asymmetry `arrival::locate` documents. Wrongly saying
/// "you are not there" locks a pilot out of ACARS filing at his own gate and
/// leaves him only a manual PIREP (which forfeits auto-approval). Wrongly saying
/// "you are there" merely lets him file a flight he flew. The runway table has
/// airports whose geometry is plainly wrong (22 real ICAO fields have surviving
/// thresholds more than 2 nm from their own reference point, 20 of them more
/// than 5 nm), and an earlier cut of this function preferred that geometry
/// whenever it existed — which would have blocked a correctly-parked pilot at
/// every one of them.
/// Where is the aircraft, for the purposes of a GATE that can refuse a pilot's
/// filing? Never from a snapshot the sim hasn't placed yet, and never from the
/// distance baseline — which a resume deliberately clears.
///
/// v0.19.3, QS round 7: the first cut of this fell back to `last_lat/lon`, which
/// `apply_pause_resume` nulls (and persists) so that a repositioning jump cannot
/// count as flown distance. Correct for the odometer, fatal here: a pilot who
/// blocked on at EDDF, lost his sim, pressed "Flug wiederaufnehmen" and then
/// filed would have had the `not_at_arrival` gate skipped entirely — a clean,
/// ACARS-auto-approved EDDM arrival, filed from Frankfurt.
fn aircraft_position_for_gates(stats: &FlightStats) -> Option<(f64, f64)> {
    /// A position the sim actually placed the aircraft at (not NaN, not Null
    /// Island). QS round 8: this filters EACH candidate rather than the result —
    /// a junk `last_known_*` used to discard the whole chain instead of falling
    /// through to the next source.
    fn usable(p: Option<(f64, f64)>) -> Option<(f64, f64)> {
        p.filter(|(la, lo)| {
            la.is_finite() && lo.is_finite() && !(la.abs() < 0.02 && lo.abs() < 0.02)
        })
    }
    usable(stats.last_known_lat.zip(stats.last_known_lon))
        // Pre-v0.19.3 flights resumed from an old snapshot have no
        // `last_known_*`; the distance baseline is the best they have.
        .or_else(|| usable(stats.last_lat.zip(stats.last_lon)))
        // Failing everything: the touchdown point. Persisted, never cleared, and
        // for an arrival gate it is exactly the right neighbourhood.
        .or_else(|| usable(stats.landing_lat.zip(stats.landing_lon)))
}

async fn compute_distance_to_airport(
    app: &AppHandle,
    state: &tauri::State<'_, AppState>,
    icao: &str,
) -> Option<f64> {
    let key = icao.trim().to_uppercase();

    // WHERE IS THE AIRCRAFT — and this is a gate that can REFUSE a pilot's
    // filing, so it had better not ask a sim that is still loading.
    //
    // v0.19.3: `current_snapshot` hands back whatever the adapter last saw,
    // including the (0, 0) placeholder MSFS reports while a session loads. The
    // streamer already refuses those (see `snapshot_position_is_usable`), but
    // this gate read them raw — so a pilot who lands, parks, and restarts his sim
    // BEFORE pressing "file" (exactly the DLH367 sequence, one step later) would
    // be measured against Null Island, told he was 3,400 nm from his destination,
    // and locked out of ACARS filing at his own gate.
    //
    // The last position the FSM actually accepted is right there in the stats.
    // Use it. If we have neither, we cannot place the aircraft — return None, and
    // the caller skips the gate rather than blocking on a guess.
    let (nav_pos, pos) = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard.as_ref();
        let nav = flight.and_then(|f| {
            let cache = f.navdata.lock().expect("navdata lock");
            cache.reference_pos(&key)
        });
        let last = flight.and_then(|f| {
            let s = f.stats.lock().expect("flight stats");
            aircraft_position_for_gates(&s)
        });
        (nav, last)
    };
    let live = current_snapshot(app).filter(snapshot_position_is_usable);
    let (lat, lon) = match (live, pos) {
        (Some(s), _) => (s.lat, s.lon),
        (None, Some(p)) => p,
        (None, None) => return None,
    };

    let by_runway_nm = runway::distance_to_airport_m(&key, lat, lon).map(|m| m / 1852.0);

    // phpVMS: cache first, network only on a miss.
    let cached: Option<Airport> = {
        let guard = state.airports.lock().expect("airports lock");
        guard.get(&key).cloned()
    };
    let phpvms_airport = match cached {
        Some(a) => Some(a),
        None => {
            // Best-effort — if the lookup fails (network down, airport not in the
            // DB) we simply have one source fewer.
            let client = state.client.lock().expect("client mutex").clone();
            match client {
                Some(c) => match c.get_airport(&key).await {
                    Ok(fetched) => {
                        state
                            .airports
                            .lock()
                            .expect("airports lock")
                            .insert(key.clone(), fetched.clone());
                        Some(fetched)
                    }
                    Err(_) => None,
                },
                None => None,
            }
        }
    };
    let phpvms_pos = phpvms_airport.and_then(|a| a.lat.zip(a.lon));

    let (ref_pos, _source) = airport_reference_pos(&key, nav_pos, phpvms_pos);
    let by_ref_nm = ref_pos.map(|(la, lo)| ::geo::distance_m(lat, lon, la, lo) / 1852.0);

    match (by_runway_nm, by_ref_nm) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        // No coordinates from any source — the caller must not gate on this.
        (None, None) => None,
    }
}

/// Startsperre einer Hintergrundaufgabe nehmen und die Generation liefern,
/// mit der sie laufen soll — `None`, wenn schon eine laeuft.
///
/// Unter `start_sperre`, wie das Fortsetzen in `restore_flight_for_retry`:
/// Ein zweiter Starter (`spawn_resume_sim_gate`) nimmt die Sperre deshalb
/// entweder ganz vor dem Fortsetzen (seine Aufgabe ist dann ueberholt und
/// endet, restore startet neu) oder ganz danach (dann mit der neuen
/// Generation, und restore sieht die Sperre belegt). Weder faellt die
/// Aufgabe aus noch laeuft sie doppelt (Codex-Abnahmen 233eedb4, cd149ec1).
fn startsperre_nehmen(flight: &ActiveFlight, sperre: &AtomicBool) -> Option<u64> {
    let _start = flight.start_sperre.lock().unwrap_or_else(|e| e.into_inner());
    if sperre.swap(true, Ordering::SeqCst) {
        return None;
    }
    Some(flight.generation.load(Ordering::SeqCst))
}

/// v0.19.x FIX (finding 2): restore a flight that failed to file back into
/// `active_flight` for retry, undoing everything `flight_end` did to it
/// before the attempt. `flight.stop` was set true and — because filing can
/// take up to ~40s across the retry backoff, far longer than any worker's
/// poll interval — the position streamer, phpVMS-position worker and
/// touchdown sampler have almost certainly already exited because of it.
/// Without this, the restored flight sits in `active_flight` looking
/// active while nothing is actually recording it anymore: no streaming,
/// no phpVMS posts, no touchdown capture. Mirrors the exact re-arm
/// sequence `spawn_resume_sim_gate` uses after an app-restart resume.
/// Clears the three per-flight "already spawned" latches
/// (`streamer_spawned`/`phpvms_worker_spawned`/`touchdown_sampler_spawned`)
/// so a retry can legitimately re-arm the background tasks on a flight
/// object that already went through one spawn cycle.
///
/// v0.20.x QS fix: `restore_flight_for_retry` reuses the SAME `Arc`
/// `flight_end`/`flight_end_manual` took out of state — its guards are
/// already latched `true` from the flight's original start, and every
/// `spawn_resume_sim_gate` construction site starts a FRESH `ActiveFlight`
/// (guards default `false`), so this is the one call site that actually
/// needs an explicit re-arm. Without it, `spawn_phpvms_position_worker`/
/// `spawn_position_streamer`/`spawn_touchdown_sampler` each hit their own
/// "already running, skipping spawn" guard and silently no-op — the
/// restored flight looks active in `active_flight` with zero background
/// tasks actually running, exactly the bug this function's doc comment
/// claims to fix.
fn rearm_background_task_guards(flight: &ActiveFlight) {
    flight.streamer_spawned.store(false, Ordering::SeqCst);
    flight.phpvms_worker_spawned.store(false, Ordering::SeqCst);
    flight
        .touchdown_sampler_spawned
        .store(false, Ordering::SeqCst);
}

/// Den kritischen Teil des Fortsetzens unter `start_sperre`. Reihenfolge
/// zaehlt: erst die Generation (alte Worker/Streamer/Sampler gelten ab jetzt
/// als ueberholt), dann das Flag (kein Flugende mehr — ein spaeterer Abbruch
/// verwirft wieder), dann `stop`, zuletzt die Startsperren frei. Wer das Flag
/// als geloescht sieht, sieht deshalb auch die neue Generation.
fn fortsetzen_vorbereiten(flight: &ActiveFlight) {
    let _start = flight.start_sperre.lock().unwrap_or_else(|e| e.into_inner());
    flight.generation.fetch_add(1, Ordering::SeqCst);
    flight.positionen_nachsenden.store(false, Ordering::SeqCst);
    flight.stop.store(false, Ordering::SeqCst);
    rearm_background_task_guards(flight);
}

fn restore_flight_for_retry(
    app: &AppHandle,
    state: &AppState,
    client: &Client,
    flight: Arc<ActiveFlight>,
) {
    fortsetzen_vorbereiten(&flight);
    spawn_phpvms_position_worker(app.clone(), Arc::clone(&flight), client.clone());
    spawn_position_streamer(app.clone(), Arc::clone(&flight), client.clone());
    spawn_touchdown_sampler(app.clone(), Arc::clone(&flight));
    let mut guard = state.active_flight.lock().expect("active_flight lock");
    *guard = Some(flight);
}

#[cfg(test)]
#[path = "tests_lib/rearm_background_task_guards_tests.rs"]
mod rearm_background_task_guards_tests;

/// v1.6.14 — Idempotenz der phpVMS-Positionen.
///
/// Feldbefund 21.08.2026 (GSG, 6 Fluege geprueft): 19-122 doppelte
/// Positionszeilen je Flug. Ursache war nicht die Erfassung — der
/// MQTT-Strom zum VPS-Recorder war bei denselben Fluegen dublettenfrei
/// (4.448 von 4.461 Punkten eindeutig) — sondern der Sendeweg zu phpVMS:
/// ein als gescheitert gewerteter Batch-POST wandert komplett zurueck in
/// die Outbox, wird erneut geschickt, und ohne Schluessel legt phpVMS
/// dabei neue Zeilen an statt die vorhandenen zu aktualisieren.
///
/// Diese Tests sichern die eine Eigenschaft, auf der die Loesung steht:
/// die `id` entsteht beim ERZEUGEN des Punktes und ueberlebt jeden
/// Requeue und den Persistenz-Umweg unveraendert.
#[cfg(test)]
#[path = "tests_lib/positions_idempotenz_tests.rs"]
mod positions_idempotenz_tests;
#[cfg(test)]
#[path = "tests_lib/aufsetzer_einfrieren_tests.rs"]
mod aufsetzer_einfrieren_tests;

/// File the active PIREP with computed final stats. Refuses to file if any
/// of the minimum-quality checks in `validate_for_filing` fail — the UI then
/// surfaces a dialog letting the user cancel the flight or file as a manual
/// PIREP via [`flight_end_manual`].
/// `divert_to` is optional and carries the ICAO of the actual landing
/// airport when the pilot is filing a divert. JS side passes this when
/// the user clicks "Submit as divert to X" on the divert-detected
/// banner. When `Some`, the proximity-to-planned-arrival validation is
/// skipped, the FileBody includes `arr_airport_id` to override the
/// bid's planned destination, and a "DIVERT: X → Y" line is prepended
/// to the notes.
#[tauri::command]
async fn flight_end(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    divert_to: Option<String>,
    divert_reason: Option<String>,
    accident_decision: Option<String>,
    // Freiwillige Begruendung, wenn beim Wiederaufnehmen ein unmoeglicher
    // Sprung erkannt wurde und der Pilot den Flug trotzdem einreicht
    // (Banner `SprungBanner`). Steht danach in den PIREP-Notizen.
    // Einfacher Kommentar, KEIN `///`: Doku-Kommentare sind an Parametern
    // nicht erlaubt (CI 23.09.2026).
    #[allow(non_snake_case)] sprungBegruendung: Option<String>,
) -> Result<(), UiError> {
    let sprung_begruendung = sprungBegruendung
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let divert_to = divert_to
        .as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty());
    // v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, LE2): die
    // Divert-Begründung. Pflichtfeld sobald `divert_to` gesetzt ist —
    // das Bestätigungs-Modal validiert frontend-seitig, dieser Guard
    // schließt den API-Contract serverseitig ab.
    let divert_reason = divert_reason
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    // NOTE: the "a divert needs a reason" guard deliberately lives further down,
    // AFTER we know the planned arrival and can tell whether this is a divert at
    // all. Demanding a reason here would force the pilot to justify a "divert"
    // to the airport he was already flying to — which we then discard.

    // v0.7.19 GAF-707 (QS-R1 Finding 3): Pilot-Override-Pfad. Werte:
    // - None / "auto"        → Stats-Werte bleiben unveraendert.
    //   Pilot bestaetigt im Dialog dass die Klassifikation stimmt.
    // - "as_accident"        → wie None, expliziter "ja, war Unfall"-
    //   Klick. Wird nicht ueberschrieben, nur fuer Audit-Notes geloggt.
    // - "as_hard_landing"    → Pilot widerspricht der Klassifikation
    //   und filed den Flug als regulaere Landung. Setzt
    //   stats.accident_detected=false, kind/confidence=None,
    //   ergaenzt reasons um "pilot_override=as_hard_landing(...)".
    //   Die Notes/Custom-Fields zeigen den Override transparent.
    //
    // Spec §Active Flight / Flight End "Nein, als harte Landung
    // behandeln".
    let accident_decision = accident_decision
        .as_deref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(decision) = accident_decision.as_deref() {
        if decision == "as_hard_landing" {
            // v0.7.19 (QS-R2 Finding 1): Override-MQTT-Event vorbereiten
            // BEVOR wir den Stats-Latch clearen — damit wir die Original-
            // Klassifikation fuer den `previous_*`-Audit-Trail kennen.
            // Webapp persistierte beim Touchdown-Edge schon `accident=true`;
            // dieser Korrektur-Event triggert den DB-Update.
            let override_payload_opt = {
                let guard = state.active_flight.lock().expect("active_flight lock");
                if let Some(flight) = guard.as_ref() {
                    let mut stats = flight.stats.lock().expect("flight stats");
                    if stats.accident_detected
                        || stats.accident_confidence.as_deref() == Some("medium")
                    {
                        let prev_kind = stats.accident_kind.clone().unwrap_or_default();
                        let prev_conf = stats.accident_confidence.clone();
                        let prev_reasons = stats.accident_reasons.join(", ");
                        stats.accident_detected = false;
                        stats.accident_kind = None;
                        stats.accident_confidence = None;
                        stats.accident_reasons = vec![format!(
                            "pilot_override=as_hard_landing (was: {prev_kind}; reasons: {prev_reasons})"
                        )];
                        // accident_at wird NICHT geloescht — Audit-Spur fuer den
                        // Override-Zeitpunkt bleibt. Notes-Builder ignoriert sie
                        // wenn accident_detected=false.
                        log_activity(
                            &state,
                            ActivityLevel::Warn,
                            "Pilot hat Accident-Klassifikation widersprochen — filing als harte Landung".to_string(),
                            Some(format!(
                                "Vorherige Klassifikation: kind={prev_kind}, reasons=[{prev_reasons}]"
                            )),
                        );

                        Some(aeroacars_mqtt::TouchdownAccidentOverridePayload {
                            ts: Utc::now().timestamp_millis(),
                            pirep_id: flight.pirep_id.clone(),
                            decision: "as_hard_landing".into(),
                            accident: false,
                            accident_kind: None,
                            accident_confidence: None,
                            accident_reasons: stats.accident_reasons.clone(),
                            previous_kind: Some(prev_kind),
                            previous_confidence: prev_conf,
                        })
                    } else {
                        None
                    }
                } else {
                    None
                }
            };

            // MQTT-Publish OHNE Locks halten. Best-effort — Recorder
            // retried den Touchdown-Row-Update unabhaengig.
            if let Some(payload) = override_payload_opt {
                let app_state = state.inner();
                let mqtt_handle = app_state.mqtt.lock().await.clone();
                if let Some(handle) = mqtt_handle {
                    handle.touchdown_accident_override(payload);
                }
            }
        }
    }

    // Pull the arrival ICAO so we can resolve its coords without holding
    // the active_flight lock across the network call.
    let arr_icao = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard
            .as_ref()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?;
        flight.arr_airport.clone()
    };

    // A "divert" to the planned destination is not a divert — it is an
    // arrival, and it must be filed as one. Dropping it HERE, before anything
    // branches on `divert_to.is_some()`, is what keeps the rest of this
    // function honest: every downstream branch (skip the not_at_arrival gate,
    // write DIVERT notes, override arr_airport_id, and above all bypass /file
    // in favour of a MANUAL/PENDING update that forfeits ACARS auto-approval)
    // is correct for a real divert and wrong for this one.
    //
    // v0.19.3: the client could previously *offer* this — a broken divert
    // detection put "file as divert to EDDF" in front of a pilot whose flight
    // was planned to EDDF. Even with the detection fixed, an ICAO typed by
    // hand in the manual dialog can land here, so the guard belongs in the
    // command, not in the UI.
    //
    // But the pilot naming his planned field IS a statement — "this is where I
    // ended up" — and we must not punish him for making it. It happens for real:
    // an off-field landing 6 nm short of EDDF raises a targetless hint
    // (`kind: "unknown"`), the pilot opens the override and picks EDDF, because
    // EDDF is in fact the field he was going to. Treat that as a confirmed
    // arrival: file it normally, and skip the distance gate below — otherwise
    // the very pilot who told us where he is gets `not_at_arrival` and no way
    // through, which is worse than the bug we set out to fix.
    //
    // The relaxation covers exactly the case its story describes — the aircraft
    // is NOT on any airport (an off-field landing near the destination). It must
    // NOT cover "the aircraft is demonstrably standing on a different airport":
    // KLGA is 9.3 nm from KJFK, LFPB 5.4 nm from LFPG, and 19 % of all
    // large/medium airports have another one within 15 nm. Without this
    // condition, a pilot parked at KLGA could pick KJFK from the override list
    // and file a clean, auto-approved KJFK arrival. If he really is at another
    // field, the honest options are the divert path or a manual PIREP.
    // Is the aircraft standing on some OTHER airport?
    //
    // Not "can we NAME the other airport" — that is what an earlier cut of this
    // guard asked, and it left the hole half open. `hint.actual_icao` is filled
    // from the runway table, which cannot name ~6,400 ICAO fields or any
    // heliport; for those the hint says "off airport", indistinguishable from a
    // meadow. There are 1,856 real ICAO pairs 5–15 nm apart whose second field is
    // unnameable that way (EDDK/EDKB 5.2 nm, LFPG/LFPH 5.9 nm), and at every one
    // of them a pilot could have confirmed the planned airport and filed a clean,
    // auto-approved arrival while parked somewhere else entirely.
    //
    // The reference points know those fields. Asking THEM keeps the distinction
    // the relaxation depends on: parked at a neighbouring airport (no
    // relaxation — file the divert, or file manually) versus genuinely off-field
    // short of the destination (relaxation — he is the one who knows where he is).
    let standing_on_another_field = {
        let (hint, pos) = {
            let guard = state.active_flight.lock().expect("active_flight lock");
            match guard.as_ref() {
                Some(f) => {
                    let s = f.stats.lock().expect("flight stats");
                    (s.divert_hint.clone(), aircraft_position_for_gates(&s))
                }
                None => (None, None),
            }
        };
        let named_other = hint
            .as_ref()
            .and_then(|h| h.actual_icao.as_deref())
            .is_some_and(|icao| !icao.eq_ignore_ascii_case(arr_icao.trim()));
        let on_unnamed_other = hint.is_some()
            && pos.is_some_and(|(la, lo)| {
                runway::nearest_airport_reference(
                    la,
                    lo,
                    crate::grenzwerte::grenzwerte().abgabe.standing_on_other_field_nm.wert,
                    arr_icao.trim(),
                )
                .is_some()
            });
        named_other || on_unnamed_other
    };
    let pilot_confirmed_planned = divert_to
        .as_deref()
        .map(|d| d.eq_ignore_ascii_case(arr_icao.trim()))
        .unwrap_or(false)
        && !standing_on_another_field;
    // Normalize away a "divert" that names the planned field, whatever the
    // reason for it (trim + case, so " eddf " cannot slip past as a divert).
    let divert_to = divert_to.filter(|d| !d.trim().eq_ignore_ascii_case(arr_icao.trim()));

    // A reason is required for a REAL divert (the one that rewrites the arrival
    // airport). Checked here, after normalization — see the note at the top of
    // this function.
    if divert_to.is_some() && divert_reason.is_none() {
        return Err(UiError::new(
            "divert_reason_required",
            "a divert PIREP requires a reason",
        ));
    }

    // Compute distance to the planned arrival. Used as an extra
    // pre-flight-filing gate ("not_at_arrival") so the pilot can't
    // file a flight from 200 nm out — they have to taxi to the gate or
    // file as a manual divert via flight_end_manual.
    //
    // SKIPPED when filing as a divert — by definition the pilot is NOT at the
    // planned arrival, that's the whole point.
    let measured_arr_nm = if divert_to.is_some() {
        None
    } else {
        compute_distance_to_airport(&app, &state, &arr_icao).await
    };

    // The pilot confirming the planned field relaxes the gate — but only within
    // reach of it. Without a bound, the confirm path is a hole straight through
    // every check we have: the divert picker lists the planned airport (with a
    // "· planned" badge), so two clicks from a cockpit 200 nm away produced a
    // clean, ACARS-auto-approved arrival PIREP with nothing anywhere recording
    // that the aircraft was nowhere near. Inside the bound we trust him (an
    // off-field landing a few miles short is a real thing, and he is the one
    // who knows); beyond it he has to file manually, which is the audited path.
    let distance_to_arr_nm = if pilot_confirmed_planned {
        match measured_arr_nm {
            Some(d) if d > crate::grenzwerte::grenzwerte().abgabe.pilot_confirmed_arrival_max_nm.wert => Some(d), // gate stays armed
            _ => None,
        }
    } else {
        measured_arr_nm
    };

    // Validate WITHOUT removing the flight from state, so a failed validation
    // leaves the user able to retry, edit-and-file-manual, or cancel.
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard
            .as_ref()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?;
        let stats = flight.stats.lock().expect("flight stats");
        let elapsed_minutes = (Utc::now() - flight.started_at).num_minutes() as i32;
        let mut missing = validate_for_filing(flight, &stats, elapsed_minutes);
        if let Some(d) = distance_to_arr_nm {
            if d > crate::grenzwerte::grenzwerte().abgabe.max_file_distance_nm.wert {
                missing.push("not_at_arrival");
            }
        }
        if !missing.is_empty() {
            tracing::warn!(
                pirep_id = %flight.pirep_id,
                missing = ?missing,
                distance_to_arr_nm,
                "rejecting PIREP filing — validation failed"
            );
            return Err(UiError::new(
                "flight_validation_failed",
                "flight does not meet minimum requirements for an ACARS PIREP",
            )
            .with_details(serde_json::json!({ "missing": missing })));
        }
    }

    // v0.19.x FIX (finding 3): resolve the client BEFORE taking the flight
    // out of state. This used to run after the take() below — a session-
    // expiry error at that exact point permanently lost the flight from
    // memory with no restore attempt at all, since nothing after the `?`
    // early-return ever runs.
    let client = current_client(&state)?;

    // v0.19.x FIX (findings 1+2): hold the SAME in-progress flag
    // `flight_start`/`flight_adopt` use, for the entire take → file →
    // (restore-on-failure) window below. Without this, a slow/hanging
    // /file call (up to ~40s across the retry backoff) leaves a window
    // where `active_flight` is empty but this call isn't done with the
    // flight yet — a concurrent auto-start or manual start sees the slot
    // free and creates a NEW flight, which this call's later restore-on-
    // error then silently clobbers. Reproduces the exact "double-submit
    // lost a flight" incident class. Auto-released on every return path
    // (success or restore) via `Drop` — no `disarm()` needed: on success
    // `active_flight` is genuinely empty afterwards (fine for a new
    // start), and on restore it holds the old flight again (also fine —
    // the existing `active_flight`-occupied check takes back over).
    let _lifecycle_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;
    // Boarding-Musik (2.1.1): Flugende, Abbruch und Vergessen stoppen sie sofort.
    kabinen_musik::aus();

    // Validation passed — now actually take the flight out of state and file.
    let flight = {
        let mut guard = state.active_flight.lock().expect("active_flight lock");
        guard
            .take()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?
    };
    // Offene Positionen NICHT verwerfen: Der Worker sendet sie beim Stopp
    // noch an phpVMS. Bis 2.0.11 wurde die Outbox hier geleert, mit der
    // Annahme, nach dem Einreichen sei weiteres Senden sinnlos — phpVMS
    // nimmt Positionen aber auch danach an (`AcarsController::acars_store`
    // lehnt nur abgebrochene PIREPs ab), und so fehlten die letzten
    // 4–30 s jedes Flugs in der phpVMS-Spur (SIA 436, 12.10.2026).
    flight.positionen_nachsenden.store(true, Ordering::SeqCst);
    flight.stop.store(true, Ordering::SeqCst);

    // Snapshot all stats inside a single short-lived guard to avoid holding
    // the Mutex across an `await`.
    let body = {
        let mut stats = flight.stats.lock().expect("flight stats");
        // v0.16.24 (QS-6): upgrade the runway correlation to the best
        // available (Navigraph) BEFORE building the PIREP body, so the native
        // `score` (compute_aggregate_master_score) and the "Landing Score"
        // custom field (build_pirep_fields) read the upgraded LDA/rollout
        // geometry on a divert whose on-demand navdata fetch has since
        // completed — keeping the authoritative scored PIREP record consistent
        // with the local LandingRecord. No-op (byte-identical) on-plan and
        // when the fetch hasn't landed (keeps the provisional OurAirports).
        apply_finalized_runway_correlation(&flight, &mut stats);
        // v1.7.36: „Triebwerke aus" festhalten, bevor der PIREP gebaut wird —
        // siehe `sprit_flug_abschliessen`.
        let (letzter_sprit, triebwerke_aus) = (
            stats.last_fuel_kg,
            stats.sprit_triebwerke_aus_seit.is_some(),
        );
        sprit_flug_abschliessen(&mut stats, letzter_sprit, triebwerke_aus);
        // Flight time = takeoff → landing (when both timestamps were
        // captured by the FSM). Falls back to the started_at → now
        // window only if takeoff/landing weren't observed (e.g.
        // manual file before the FSM advanced through Takeoff). The
        // takeoff→landing range matches what phpVMS expects in its
        // native Flt.Time column.
        //
        // Spec sim-disconnect-auto-resume F3 (Block-Time-Korrektur):
        // Pause-Akkumulator wird in Minuten umgerechnet und abgezogen,
        // damit Sim-Crash-/Esc-Pause-Pausen nicht als Flugzeit landen.
        let now_for_flight_time = Utc::now();
        let (raw_flight_time_min, pause_window) = match (stats.takeoff_at, stats.landing_at) {
            (Some(t), Some(l)) if l > t => ((l - t).num_minutes() as i32, (t, l)),
            _ => (
                ((now_for_flight_time - flight.started_at).num_minutes() as i32).max(0),
                (flight.started_at, now_for_flight_time),
            ),
        };
        let pause_secs = pause_secs_within(
            &stats.pause_segments,
            stats.pause_total_duration_secs,
            pause_window.0,
            pause_window.1,
        );
        let pause_min = (pause_secs / 60).clamp(0, i32::MAX as i64) as i32;
        let flight_time = Some(raw_flight_time_min.saturating_sub(pause_min).max(0));

        let fares = if flight.fares.is_empty() {
            None
        } else {
            Some(
                flight
                    .fares
                    .iter()
                    .map(|(id, count)| FareEntry {
                        id: *id,
                        count: *count,
                    })
                    .collect(),
            )
        };

        // Block→remaining diff in kg, converted to pounds for phpVMS.
        // We round to whole kilograms BEFORE the lb conversion so the
        // PIREP-detail page shows clean integer values (`9733 kg`
        // instead of `9733.45 kg`). Real-world refuelling has no
        // sub-kg precision anyway, and a half-kg loss is invisible
        // to pilots — what's NOT invisible is `9733.45 kg` looking
        // like a unit-mix-up bug. Same logic for fuel_used.
        let fuel_used = match (stats.block_fuel_kg, stats.last_fuel_kg) {
            (Some(b), Some(c)) if b > c => Some(((b - c) as f64).round() * KG_TO_LB),
            _ => None,
        };
        // Block fuel sent natively so phpVMS computes "Verbleibender
        // Treibstoff" correctly (= block_fuel - fuel_used). Without
        // this the dashboard shows "-fuel_used kg" because the
        // missing block_fuel defaults to 0. Same unit as fuel_used.
        let block_fuel = stats
            .block_fuel_kg
            .filter(|kg| *kg > 0.0)
            .map(|kg| (kg as f64).round() * KG_TO_LB);
        // Cruise level = peak MSL altitude observed. phpVMS column
        // `Flt.Level`. Rounded to the nearest 100 ft so the value
        // matches the conventional FL display.
        let level = stats.peak_altitude_ft.map(|ft| {
            let rounded = ((ft / 100.0).round() * 100.0) as i32;
            rounded.max(0)
        });
        // Native landing rate field — same value as the custom field
        // but on the native column phpVMS shows on the PIREP overview.
        //
        // v0.7.21 (MS713-QS): MUSS `canonical_landing_rate_fpm()` sein,
        // sonst kriegt phpVMS den Streamer-Tick-Snapshot statt des
        // Engineering-Edge-Wertes. Vorher: -204 (0.5s-Mittel via
        // landing_rate_fpm), seit Fix: -194 (vs_at_edge_fpm). Diese
        // Zeile war die letzte uebersehene Stelle aus dem v0.7.17
        // B-015a-Sweep.
        let landing_rate = stats.canonical_landing_rate_fpm().map(|v| v as f64);
        // v0.7.1 P1 (Round-2 Review): phpVMS bekommt jetzt den
        // GLEICHEN gewichteten Aggregate-Score wie App/Web/MQTT
        // (siehe build_landing_record P1.3-Fix). Vorher landete der
        // alte Touchdown-Klassifikator-Score (vs/g/bounces only) im
        // phpVMS-Datensatz waehrend App "77/100" zeigte und phpVMS
        // "60/100" — Score-Vertragsbruch.
        // Fallback: stats.landing_score.numeric() wenn Crate keinen
        // Aggregate liefert (z.B. komplett leerer Sub-Score-Vec).
        //
        // v0.16.24 (QS-2 FIX A): den EFFEKTIVEN Ankunftsflughafen
        // (`divert_to` falls bestätigt, sonst geplant) übergeben — exakt
        // das gleiche Argument das `build_pirep_payload` / `build_landing_record`
        // schon nutzen. Sonst prüfte der native Score die Runway-Geometrie
        // gegen das GEPLANTE Ziel und übersprang bei einem Divert (Aircraft
        // parkt >2 nm vom Threshold → `divert_hint.actual_icao`=None) den
        // Rollout/LDA-Sub-Score, während Payload/Record ihn bekamen — der
        // autoritative PIREP-Score wich vom Record ab. On-Plan: `divert_to`=None
        // → effective_arr == arr_airport → BYTE-IDENTISCH.
        let effective_arr = divert_to.as_deref().unwrap_or(&flight.arr_airport);
        // Score-Version 19: kein Rueckfall auf die Touchdown-Klasse.
        // Codex-Befund P-4.40: Score, Feld und Notiz aus EINER Bewertung.
        let (score, mut fields, mut notes) = pirep_bewertete_teile(&flight, &mut stats, effective_arr);
        let distance_nm = stats.distance_nm;
        // phpVMS does its own divert bookkeeping — but only if the PIREP carries
        // this custom field. `PirepService::handleDiversion()` looks it up by the
        // slug `diversion-airport`, which phpVMS derives from the field NAME via
        // `Str::slug()` — so the string below is load-bearing, not cosmetic.
        // With it, phpVMS moves the arrival to that ICAO, keeps the planned
        // destination in `alt_airport_id`, relocates aircraft and pilot and posts
        // its divert notification. Without it a divert is invisible to the server:
        // on 23.08.2026 (EJA9 EHAM->EDSB, back to EHAM) none of that happened.
        if let Some(actual) = divert_to.as_deref() {
            fields.insert("Diversion Airport".to_string(), actual.to_string());
        }
        // v0.19.3: the pilot told us he ended up at the planned field although
        // the FSM did not place him there. That statement is why this PIREP is
        // being filed normally instead of being blocked — so it goes into the
        // record, together with the distance we actually measured and the reason
        // he typed. Previously the reason was demanded by the UI and then
        // silently dropped, and nothing anywhere said the aircraft had not been
        // where the PIREP claims.
        if pilot_confirmed_planned {
            let dist_line = measured_arr_nm
                .map(|d| format!(" (gemessen: {d:.1} nm vom Ziel)"))
                .unwrap_or_default();
            // Derselbe Riegel wie bei der Sprung-Begruendung: Der Text kommt
            // ueber dieselbe LAN-Bruecke (Cloud-QS 23.09.2026).
            let reason_line = match begruendungs_zeile(divert_reason.as_deref()).as_str() {
                "" => String::new(),
                z => format!("\n{}", z.trim_end()),
            };
            notes = format!(
                "ANKUNFT VOM PILOTEN BESTÄTIGT: {arr_icao}{dist_line}\n\
                 Die automatische Erkennung hat den Flug nicht am Zielflughafen \
                 verortet; der Pilot hat {arr_icao} als tatsächlichen Landeplatz \
                 bestätigt.{reason_line}\n\n{notes}"
            );
        }
        // Unterbrochener Flug: Beim Wiederaufnehmen sprang der Zustand
        // (Reload). Das gehoert in den PIREP, sonst sieht die VA nur eine
        // normale Ankunft — genau der Fall MSC1588 (22.09.2026), wo eine
        // nie geflogene Strecke als angekommen gebucht wurde. Die
        // Begruendung des Piloten ist freiwillig und steht direkt darunter.
        let spruenge = spruenge_fuer_notiz(&stats);
        if !spruenge.is_empty() {
            notes = sprung_notiz(&spruenge, sprung_begruendung.as_deref(), &notes);
        } else if abgabe_sperre(&stats) == Some(AbgabeSperre::LandungFehlt) {
            notes = landung_fehlt_notiz(sprung_begruendung.as_deref(), &notes);
        }
        // Prepend a divert banner to the notes so the VA admin sees
        // immediately on the PIREP page that this wasn't a normal
        // arrival. Format mirrors what most ACARS clients write
        // ("DIVERT: <planned> → <actual>") so admins can grep.
        // v0.12.5 (LE2): die Pflicht-Begründung steht direkt unter dem
        // Banner — ein Audit-Eintrag pro Divert für den VA-Admin.
        if let Some(actual) = divert_to.as_deref() {
            let reason_line =
                match begruendungs_zeile_mit(divert_reason.as_deref(), "Reason").as_str() {
                    "" => String::new(),
                    z => format!("{z}\n"),
                };
            notes = format!(
                "DIVERT: {} → {} (planned destination not reached)\n{}\n{}",
                arr_icao, actual, reason_line, notes
            );
        }

        let body = FileBody {
            flight_time,
            fuel_used,
            block_fuel,
            distance: Some(distance_nm),
            level,
            landing_rate,
            score,
            source_name: Some(format!("AeroACARS/{}", env!("CARGO_PKG_VERSION"))),
            notes: Some(notes),
            fares,
            fields: Some(fields),
            // Deliberately NOT the divert ICAO: phpVMS rewrites `arr_airport_id`
            // itself out of the `Diversion Airport` field above and moves the
            // planned destination into `alt_airport_id`. Sending the divert here
            // as well would make it rewrite EHAM->EHAM and file the divert field
            // as its own alternate.
            arr_airport_id: None,
            // Block times from the FSM. phpVMS derives NEITHER on its own:
            // its own fallback in PirepService::create() is defeated by
            // CarbonCast (NULL reads back as "now", so the `if (!$pirep->
            // block_off_time)` guard never fires) — every PIREP we filed
            // without these left block_off_time NULL in the DB.
            block_off_time: stats.block_off_at.map(|t| t.to_rfc3339()),
            block_on_time: stats
                .block_on_at
                .or(stats.landing_at)
                .map(|t| t.to_rfc3339()),
        };
        body
    };
    // The airport the aircraft actually parked at: the planned arrival on every
    // normal flight, the pilot-confirmed field on a divert. phpVMS learns it from
    // the `Diversion Airport` custom field; everything local (landing record, MQTT
    // payload, activity log) reads it from here.
    let effective_arr_icao: String = divert_to
        .clone()
        .unwrap_or_else(|| flight.arr_airport.clone());
    tracing::info!(
        pirep_id = %flight.pirep_id,
        flight_time = body.flight_time.unwrap_or(0),
        distance = body.distance.unwrap_or(0.0),
        fuel_used = body.fuel_used.unwrap_or(0.0),
        fare_classes = flight.fares.len(),
        custom_fields = body.fields.as_ref().map(|f| f.len()).unwrap_or(0),
        "filing PIREP"
    );
    // ONE path for every arrival, divert included.
    //
    // Until v1.7.0 a divert took a separate route: it never called /file and
    // instead mass-assigned state=PENDING through the update endpoint, so the
    // VA admin had to release it by hand. That skipped `PirepService::submit()`
    // and with it EVERYTHING hanging off the `PirepFiled` event — phpVMS' own
    // divert handling, the bid cleanup, the integrity gate, the per-rank
    // auto-approve — and it leaned on phpVMS forwarding unvalidated request
    // input into mass-assignment, an upstream internal that could vanish in any
    // release. On 23.08.2026 a pilot sat blocked for an hour behind exactly that
    // (EJA9 EHAM->EDSB). A divert is a normal arrival at a different field; it
    // is filed like one, and the `Diversion Airport` custom field tells phpVMS
    // the rest.
    //
    // v0.5.49 — file_pirep_with_retry: 3 attempts mit 5s+30s Backoff
    // bei TRANSIENTEM Fehler (Netz, Timeout, 5xx, 429). Bei 3× Fail
    // queued der Err-Branch unten den PIREP in den persistenten
    // pirep_queue/ — Background-Worker reicht ihn ein sobald die
    // Verbindung wieder steht. Pilot sieht „PIREP queued" statt
    // „PIREP filed", kann aber sofort den nächsten Flug starten.
    // Bordbuch einfrieren, bevor eingereicht wird (auch offline).
    bordbuch_abschliessen(&app, &flight);
    nachtrag_vor_dem_einreichen_sichern(&app, &flight);
    landung_vor_dem_einreichen_nachschicken(&app, &flight).await;
    match file_pirep_with_retry(&client, &flight.pirep_id, &body).await {
        Ok(()) => {
            // Snapshot the landing into the local history file BEFORE
            // we drop the persisted flight — gives the new "Landung"
            // tab something to render even after the PIREP is filed
            // and the in-memory FlightStats goes out of scope.
            {
                // v0.15.x: tatsächlicher Landeflughafen für Trust — auf einem
                // Divert das bestätigte Ausweichfeld, sonst das geplante Ziel.
                record_landing_for_filed_flight(&app, &flight, &effective_arr_icao);
            }
            clear_persisted_flight(&app, Some(&flight.pirep_id));
            log_activity(
                &state,
                ActivityLevel::Info,
                format!(
                    "PIREP filed: {} {} – {}{}",
                    format_callsign(&flight.airline_icao, &flight.flight_number),
                    flight.dpt_airport,
                    effective_arr_icao,
                    if divert_to.is_some() {
                        format!(" (DIVERT, planned {})", flight.arr_airport)
                    } else {
                        String::new()
                    },
                ),
                {
                    let dist = body.distance.unwrap_or(0.0);
                    let fuel = body.fuel_used.unwrap_or(0.0);
                    if fuel > 0.0 {
                        Some(format!("Distance {dist:.1} nm, fuel {fuel:.0} lb"))
                    } else {
                        Some(format!("Distance {dist:.1} nm"))
                    }
                },
            );
            // 1a: Was in der Kabine noch wartet, als entfallen vermerken — vor
            // `pirep_filed`, danach kommt kein Takt mehr.
            kabinenansagen::flug_beendet(&app, &flight);
            record_event(
                &app,
                &flight.pirep_id,
                &FlightLogEvent::FlightEnded {
                    timestamp: Utc::now(),
                    pirep_id: flight.pirep_id.clone(),
                    outcome: FlightOutcome::Filed,
                },
            );
            // v0.7.14: Pilot-Client-Discord-Post fuer PirepFiled entfernt —
            // Recorder postet das jetzt zentral via `postPirep` aus dem
            // MQTT-PIREP-Event. Audit Q4-2026-05 (C1).
            // v0.5.11: MQTT live-tracking PIREP publish. Best-effort,
            // fire-and-forget. Monitor uses this to mark a flight
            // as completed in the live history.
            {
                // v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, LE1):
                // Der MQTT-PirepPayload-Build ist in `build_pirep_payload`
                // ausgelagert — eine Stelle für alle Filing-Pfade. Ohne Divert
                // ist effective == planned == flight.arr_airport, der Payload
                // also byte-gleich zu vorher; mit Divert setzt der Helper
                // `divert`/`diverted_to` und der Recorder hat den Grund.
                let mut pirep_payload =
                    build_pirep_payload(&flight, &body, &effective_arr_icao, &flight.arr_airport);
                if divert_to.is_some() {
                    pirep_payload.notes = divert_reason.clone();
                }
                // v0.12.5 (LE1): JSONL-Forensik + MQTT-Publish über den
                // zentralen Finalizer — gleicher Pfad wie Divert/Manual/Queue.
                // QS-P1: JSONL-PirepFiled IMMER, MQTT nur bei Handle.
                let pirep_payload_json =
                    serde_json::to_value(&pirep_payload).unwrap_or(serde_json::Value::Null);
                // P-7.21: Eingabe vor dem PIREP bei live.
                eingabe_vor_dem_bericht(&app).await;
                let mqtt = state.mqtt.lock().await;
                finalize_filed_pirep(
                    &app,
                    mqtt.as_ref(),
                    &flight.pirep_id,
                    pirep_payload_json,
                    Some(&flight),
                    None,
                );
            }
            // v0.7.0 — emit landing_finalized mit dem finalen Score
            // (Spec docs/spec/touchdown-forensics-v2.md Sektion 7.2).
            // v0.12.5 (LE7-QS): in `emit_landing_finalized` ausgelagert —
            // Divert- und Manual-Pfad rufen denselben Helper.
            emit_landing_finalized(&app, &flight);
            // Guard hier binden (nicht erst weiter unten) — dieselbe
            // Identitaet geht sowohl in den JSONL-Upload als auch in den
            // Bid-Cleanup-Eintrag.
            let aktuelle_identitaet_fuer_bid_cleanup = state
                .authenticated_pilot_id
                .lock()
                .expect("authenticated_pilot_id lock")
                .map(|id| id.to_string());
            // v0.5.23 Forensik-Upload: gzip + POST des kompletten JSONL-
            // Logfiles an aeroacars-live damit der VA-Owner ohne den
            // Piloten zu kontaktieren das vollstaendige SimSnapshot-Log
            // (80 Felder) + Activity-Log + PhaseChanged-Stream pro
            // Session abrufen kann. Fire-and-forget, blockt PIREP-File
            // nicht — Failure landet nur im Log.
            //
            // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): die
            // Identitaet geht jetzt mit — `spawn_flight_log_upload` prueft
            // sie zum Zeitpunkt der tatsaechlichen Ausfuehrung erneut
            // (der Task laeuft asynchron, nicht sofort).
            spawn_flight_log_upload(
                &app,
                flight.pirep_id.clone(),
                aktuelle_identitaet_fuer_bid_cleanup.clone(),
                false,
            );
            // v0.7.19 (QS-R1 Finding 1): Bei delete_bid-fail wird der Bid
            // in pending_bid_cleanup eingereiht und vom Background-Worker
            // retried — keine haengenden Aircraft/Bid mehr nach Accident-
            // FILED. Reason-String spiegelt die Filing-Variante.
            let bid_reason = if divert_to.is_some() {
                "divert_filed"
            } else if flight.stats.lock().expect("stats lock").accident_detected {
                "accident_filed"
            } else {
                "normal_filed"
            };
            consume_bid_best_effort(
                &app,
                &client,
                &flight.pirep_id,
                flight.bid_id,
                Some(flight.flight_id.as_str()),
                bid_reason,
                aktuelle_identitaet_fuer_bid_cleanup,
            )
            .await;
            Ok(())
        }
        Err(e) => {
            // v0.5.49 — Auto-Retry hat schon 3× erfolglos versucht.
            // Wenn der Fehler TRANSIENT ist (Netz, Timeout) → in den
            // persistenten Queue schieben, Flight clearen, Pilot kann
            // weiter. Background-Worker reicht den PIREP ein sobald
            // die Verbindung wieder steht.
            // Wenn der Fehler HARD ist (Validation, Auth) → wie bisher
            // Flight in State halten damit der Pilot was korrigieren
            // kann.
            if is_transient_pirep_error(&e) {
                // v0.12.5 (LE1b): MQTT-PIREP-Payload schon hier bauen und
                // mit-persistieren — der Queue-Worker publisht es nach
                // erfolgreichem /file. Normaler /file-Pfad: effective ==
                // planned == flight.arr_airport.
                let pirep_payload_json = serde_json::to_value({
                    let mut queued_payload = build_pirep_payload(
                        &flight,
                        &body,
                        &effective_arr_icao,
                        &flight.arr_airport,
                    );
                    if divert_to.is_some() {
                        queued_payload.notes = divert_reason.clone();
                    }
                    queued_payload
                })
                .unwrap_or(serde_json::Value::Null);
                // N6: Das Navdaten-Upgrade JETZT anwenden und den Nachtrag
                // mit in die Warteschlange legen. `record_landing_for_filed_
                // flight` weiter unten setzt die Fahne sonst erst, wenn
                // der Flug schon verworfen ist. Keine `stats`-Sperre
                // liegt hier an (nur eine temporaere weiter oben).
                // ⚠ Die Fahne faellt erst, wenn der Nachtrag DAUERHAFT ist —
                // nach `pirep_queue::enqueue`. Vorher fiel sie beim Bauen;
                // schlug das Einreihen fehl, wurde der Flug wiederhergestellt,
                // aber ohne offene Korrektur (Codex, 03.09.2026, P2).
                let bahn_nachtrag = {
                    let mut st = flight.stats.lock().expect("flight stats");
                    let _ = bahn_upgrade_anwenden(&flight, &mut st);
                    if st.bahn_nachtrag_offen {
                        bahn_nachtrag_bauen(&flight, &st).and_then(|p| serde_json::to_value(p).ok())
                    } else {
                        None
                    }
                };
                // Die Landung mitnehmen, wenn sie noch nicht bestaetigt
                // ist — sonst ist sie weg, sobald der Flug schliesst.
                let landung = {
                    let st = flight.stats.lock().expect("flight stats");
                    if st.landung_live_bestaetigt {
                        None
                    } else {
                        st.landung_payload
                            .as_ref()
                            .and_then(|p| serde_json::to_value(&**p).ok())
                    }
                };
                let queued = pirep_queue::QueuedPirep {
                    pirep_id: flight.pirep_id.clone(),
                    bahn_nachtrag,
                    landung,
                    bid_id: flight.bid_id,
                    airline_icao: flight.airline_icao.clone(),
                    flight_number: flight.flight_number.clone(),
                    dpt_airport: flight.dpt_airport.clone(),
                    arr_airport: flight.arr_airport.clone(),
                    body: body.clone(),
                    queued_at: Utc::now(),
                    last_attempt_at: Some(Utc::now()),
                    attempt_count: 3, // = die 3 retries die wir grade verbraucht haben
                    last_error: Some(friendly_net_error(&e)),
                    // v0.7.19 (QS-R2 Finding 2): phpVMS-flight_id mit-
                    // persistieren damit der Worker-Bid-Cleanup nach
                    // erfolgreichem queued-Filing den flight_id-Fallback
                    // hat. Empty-String fuer pre-v0.7.7-Bids ist okay.
                    flight_id: flight.flight_id.clone(),
                    pirep_payload_json,
                    dead_letter_notified: false,
                    retry_not_before: None,
                    // Codex-Folgefund (adversarial, 05.09.2026, fuenfte
                    // Runde): NICHT die Piloten-ID des GERADE eingeloggten
                    // Accounts frisch ableiten, wenn dieser Code hier laeuft —
                    // `phpvms_logout` leert `state.client`, aber nicht
                    // `state.active_flight`. Zwischen Flugstart und diesem
                    // Filing-Versuch koennte sich also bereits ein ANDERER
                    // Pilot angemeldet haben. `active_flight_owner_identity`
                    // wurde EINMAL bei Flugstart/-uebernahme gesetzt und
                    // bleibt der richtige Eigentuemer, unabhaengig davon wer
                    // gerade eingeloggt ist.
                    owner_identity: state
                        .active_flight_owner_identity
                        .lock()
                        .expect("active_flight_owner_identity lock")
                        .clone(),
                };
                if let Err(qe) = pirep_queue::enqueue(&app, &queued) {
                    // Queue selber kaputt — fallback auf altes Verhalten
                    tracing::error!(error = %qe, "PIREP-Queue enqueue failed — falling back to flight-in-state");
                    log_activity(
                        &state,
                        ActivityLevel::Error,
                        "PIREP file failed",
                        Some(format!(
                            "{} — Queue ebenfalls kaputt, Flug bleibt aktiv für Retry",
                            e
                        )),
                    );
                    restore_flight_for_retry(&app, state.inner(), &client, flight);
                    return Err(e.into());
                }
                // Erfolgreich gequeued: JETZT ist der Nachtrag dauerhaft, die
                // Fahne darf fallen.
                if queued.bahn_nachtrag.is_some() {
                    if let Ok(mut st) = flight.stats.lock() {
                        st.bahn_nachtrag_offen = false;
                    }
                }
                // Erfolgreich gequeued → Landing-Snapshot, clear, Activity-Log
                {
                    // v0.15.x: tatsächlicher Landeflughafen für Trust — auf einem
                    // Divert das bestätigte Ausweichfeld, sonst das geplante Ziel.
                    record_landing_for_filed_flight(&app, &flight, &effective_arr_icao);
                }
                // Lernpaket AP4/AP5 (Nachpruefung, Befund C): auch im
                // Warteschlangen-Zweig den Forensik-Nachtrag vor `FlightEnded`.
                anflug_forensik_nachtrag_schreiben(&app, &flight);
                clear_persisted_flight(&app, Some(&flight.pirep_id));
                log_activity(
                    &state,
                    ActivityLevel::Warn,
                    format!(
                        "PIREP queued: {} {} – {}",
                        format_callsign(&flight.airline_icao, &flight.flight_number),
                        flight.dpt_airport,
                        flight.arr_airport,
                    ),
                    Some(format!(
                        "{} — wird automatisch eingereicht sobald die Verbindung wieder steht. Du kannst den nächsten Flug starten.",
                        friendly_net_error(&e)
                    )),
                );
                // 1a: Was in der Kabine noch wartet, als entfallen vermerken — vor
                // `pirep_filed`, danach kommt kein Takt mehr.
                kabinenansagen::flug_beendet(&app, &flight);
                record_event(
                    &app,
                    &flight.pirep_id,
                    &FlightLogEvent::FlightEnded {
                        timestamp: Utc::now(),
                        pirep_id: flight.pirep_id.clone(),
                        outcome: FlightOutcome::Filed, // queued zählt als „abgeschlossen"
                    },
                );
                Ok(())
            } else {
                // Hard-Fail (Validation, Auth, …) — Pilot muss das selber
                // korrigieren. Flight bleibt in state.
                log_activity(
                    &state,
                    ActivityLevel::Error,
                    "PIREP file failed",
                    Some(format!("{} — flight kept in state for retry", e)),
                );
                restore_flight_for_retry(&app, state.inner(), &client, flight);
                Err(e.into())
            }
        }
    }
}
