/// File the active PIREP as a *manual* report — bypasses the validation in
/// `flight_end` and tags the source as manual so the VA admin knows the data
/// wasn't 100% machine-collected.
///
/// Use cases:
///   * Flight ended with broken stats (e.g. resumed run that lost block_fuel)
///     but pilot still wants to file rather than cancel.
///   * Divert: planned EDDP→EDDF, actually landed EDDV. The pilot supplies
///     `divert_to = "EDDV"` and a `reason`; admin sees the divert tag in the
///     notes and adjusts the PIREP airports manually.
///   * Sim disconnect mid-flight: the FSM never saw landing, so flight_time /
///     fuel_used / landing_rate are all wrong. Pilot supplies the override
///     fields below and we ship those instead of the (broken) FSM values.
///
/// Every override field is `Option<...>`; `None` falls back to whatever the
/// FSM captured. All overrides also get tagged in the notes so the admin
/// can see WHICH fields the pilot edited by hand.
///
/// `block_off_at_iso` / `block_on_at_iso` accept any RFC-3339 timestamp
/// (typically `2026-05-02T15:30:00Z`); blank uses the FSM-captured time.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn flight_end_manual(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    notes_override: Option<String>,
    divert_to: Option<String>,
    reason: Option<String>,
    flight_time_minutes: Option<i32>,
    block_fuel_kg: Option<f32>,
    // v0.12.5 (LE4): Resttreibstoff im Tank in kg — NICHT der Verbrauch.
    // Das Backend rechnet `fuel_used_kg = block_fuel_kg − remaining_fuel_kg`.
    remaining_fuel_kg: Option<f32>,
    distance_nm: Option<f64>,
    cruise_level_ft: Option<i32>,
    landing_rate_fpm: Option<f32>,
    block_off_at_iso: Option<String>,
    block_on_at_iso: Option<String>,
) -> Result<(), UiError> {
    // v0.19.x FIX: same two fixes as `flight_end` — resolve the client
    // BEFORE taking the flight out of state (a session-expiry error must
    // not strand the flight with zero restore attempt), and hold the
    // lifecycle guard across the whole take → file → (restore) window so
    // a concurrent auto-start/manual-start can't sneak a new flight into
    // the empty `active_flight` slot while this call is still in flight.
    let client = current_client(&state)?;
    let _lifecycle_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;
    // Boarding-Musik (2.1.1): Flugende, Abbruch und Vergessen stoppen sie sofort.
    kabinen_musik::aus();

    let flight = {
        let mut guard = state.active_flight.lock().expect("active_flight lock");
        guard
            .take()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?
    };
    // Offene Positionen nachsenden — siehe `flight_end`.
    flight.positionen_nachsenden.store(true, Ordering::SeqCst);
    flight.stop.store(true, Ordering::SeqCst);

    // Same normalization as `flight_end`: a "divert" to the planned
    // destination is an arrival, not a divert. Here the ICAO can be typed by
    // hand, so this is the path where a stray keystroke would otherwise
    // overwrite `arr_airport_id` in phpVMS with the airport the flight was
    // already going to — and stamp DIVERT notes on a normal flight. Normalize
    // once, up front, before anything branches on it.
    let divert_to = divert_to
        .as_deref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty())
        .filter(|d| !d.eq_ignore_ascii_case(flight.arr_airport.trim()));

    // Parse RFC-3339 block-off/on overrides if present. Anything that
    // doesn't parse cleanly is dropped — we don't want a typo to
    // silently file a PIREP with a bogus "1970" timestamp.
    let block_off_override: Option<DateTime<Utc>> = block_off_at_iso.as_deref().and_then(|s| {
        if s.trim().is_empty() {
            None
        } else {
            DateTime::parse_from_rfc3339(s.trim())
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }
    });
    let block_on_override: Option<DateTime<Utc>> = block_on_at_iso.as_deref().and_then(|s| {
        if s.trim().is_empty() {
            None
        } else {
            DateTime::parse_from_rfc3339(s.trim())
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }
    });

    let body = {
        let mut stats = flight.stats.lock().expect("flight stats");

        // v0.16.24 (QS-6): upgrade the runway correlation to the best
        // available (Navigraph) BEFORE building the manual PIREP body, so the
        // "Landing Score" custom field (build_pirep_fields →
        // compute_aggregate_master_score) reads the upgraded LDA/rollout
        // geometry on a divert whose on-demand navdata fetch has completed.
        // Same idempotent, on-plan-byte-identical, graceful-when-not-fetched
        // contract as the auto-file path.
        apply_finalized_runway_correlation(&flight, &mut stats);
        // v1.7.36: „Triebwerke aus" festhalten, bevor der PIREP gebaut wird —
        // wie in `flight_end`, siehe `sprit_flug_abschliessen` (QS Runde 3, F2).
        let (letzter_sprit, triebwerke_aus) = (
            stats.last_fuel_kg,
            stats.sprit_triebwerke_aus_seit.is_some(),
        );
        sprit_flug_abschliessen(&mut stats, letzter_sprit, triebwerke_aus);

        // Apply block-time overrides BEFORE building the body so the
        // notes block + custom fields pick them up.
        if let Some(off) = block_off_override {
            stats.block_off_at = Some(off);
        }
        if let Some(on) = block_on_override {
            stats.block_on_at = Some(on);
        }
        if let Some(kg) = block_fuel_kg.filter(|v| *v > 0.0) {
            stats.block_fuel_kg = Some(kg);
        }

        // Same flight-time / block-fuel / level / landing-rate / score
        // mapping as the regular file path, with manual-overrides
        // taking precedence. phpVMS skips missing values cleanly
        // thanks to `skip_serializing_if`.
        //
        // Spec sim-disconnect-auto-resume F3 (Block-Time-Korrektur):
        // im Auto-Compute-Pfad (= keine Manual-Override) wird der
        // Pause-Akkumulator abgezogen. Bei Manual-Override (Pilot
        // tippt eine Flight-Time im Edit-Dialog ein) wird sein Wert
        // ungeaendert uebernommen — er hat die Pause bewusst
        // beruecksichtigt oder absichtlich ignoriert.
        let flight_time = match flight_time_minutes.filter(|m| *m >= 0) {
            Some(m) => Some(m),
            None => {
                let now_for_flight_time = Utc::now();
                let (raw_min, pause_window) = match (stats.takeoff_at, stats.landing_at) {
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
                Some(raw_min.saturating_sub(pause_min).max(0))
            }
        };
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
        // v0.12.5 (LE4): Der Pilot gibt die Restmenge im Tank (kg) ein,
        // nicht den Verbrauch. fuel_used_kg = block_fuel_kg − remaining.
        // Guard: block_fuel_kg > remaining_fuel_kg >= 0, sonst greift der
        // FSM-Fallback (block − last_fuel). Gerechnet wird in kg, ERST am
        // Ende nach lb konvertiert (phpVMS-Site-Unit = lb) — QS-R2-P2.
        // `stats.block_fuel_kg` trägt hier schon den Manual-Override
        // (oben gesetzt falls `block_fuel_kg`-Param > 0).
        let fuel_used = {
            let from_remaining = match (stats.block_fuel_kg, remaining_fuel_kg) {
                (Some(block), Some(rem)) if rem >= 0.0 && block > rem => Some((block - rem) as f64),
                _ => None,
            };
            from_remaining
                .or_else(|| match (stats.block_fuel_kg, stats.last_fuel_kg) {
                    (Some(b), Some(c)) if b > c => Some((b - c) as f64),
                    _ => None,
                })
                .map(|kg| kg.round() * KG_TO_LB)
        };
        let block_fuel = stats
            .block_fuel_kg
            .filter(|kg| *kg > 0.0)
            .map(|kg| (kg as f64).round() * KG_TO_LB);
        let level = cruise_level_ft.filter(|ft| *ft > 0).or_else(|| {
            stats.peak_altitude_ft.map(|ft| {
                let rounded = ((ft / 100.0).round() * 100.0) as i32;
                rounded.max(0)
            })
        });
        // v0.20.0: die GEMESSENE Rate schlaegt die Pilot-Eingabe. Vorher gewann
        // `landing_rate_fpm` (vom Piloten getippt) — dann stand im phpVMS-
        // Logbuch seine Zahl, im Landungs-Tab und im Custom-Field aber die
        // gemessene. Die Messung ist die Wahrheit; die Eingabe bleibt Fallback,
        // wenn wir gar keinen Touchdown erfasst haben.
        let landing_rate = stats
            .canonical_landing_rate_fpm()
            .map(|v| v as f64)
            .or_else(|| landing_rate_fpm.map(|v| v as f64));
        let resolved_distance = distance_nm
            .filter(|d| *d > 0.0)
            .unwrap_or(stats.distance_nm);
        // v0.16.24 (QS-2 FIX A): effektiver Ankunftsflughafen für die
        // "Landing Score"-Custom-Field-Geometrie-Trust-Prüfung — gleiche
        // Normalisierung (trim/uppercase/non-empty) wie der Divert-Notes-Block
        // unten und wie der `record_landing_for_filed_flight`-Aufruf weiter
        // unten. On-Plan (`divert_to`=None) == `flight.arr_airport`.
        let effective_arr_norm = divert_to
            .as_ref()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty());
        let effective_arr = effective_arr_norm.as_deref().unwrap_or(&flight.arr_airport);
        // v0.20.0: die phpVMS-`score`-Spalte bekommt die EINE Bewertung — nicht
        // mehr die Touchdown-Klasse (nur 100/80/60/30/0). Vorher stand beim
        // manuellen Filen `100` in der Spalte, waehrend Custom-Field und
        // Landungs-Tab "B (acceptable) — 78/100" zeigten.
        // Codex-Befund P-4.40: Score, Feld und Notiz aus EINER Bewertung.
        let (score, mut fields, mut notes) = pirep_bewertete_teile(&flight, &mut stats, effective_arr);
        // Same rule as the regular path: the divert airport travels in the
        // custom field phpVMS keys on (slug `diversion-airport`), never in
        // `arr_airport_id`. One semantic for both filing paths.
        if let Some(actual) = divert_to.as_deref() {
            fields.insert(
                "Diversion Airport".to_string(),
                actual.trim().to_uppercase(),
            );
        }
        notes.push_str("\n\n[MANUAL FILE — auto-validation bypassed by pilot.]");
        // Der Sprung gehoert auch hier in die Notiz: Sonst waere der
        // manuelle Weg genau die Tuer, durch die ein unterbrochener Flug
        // unbemerkt als saubere Ankunft eingeht (Cloud-QS 23.09.2026,
        // Nebenbefund). Eine Begruendung nimmt dieser Weg nicht entgegen —
        // dafuer gibt es hier das freie Notizfeld des Piloten.
        let spruenge = spruenge_fuer_notiz(&stats);
        if !spruenge.is_empty() {
            notes = sprung_notiz(&spruenge, None, &notes);
        } else if abgabe_sperre(&stats) == Some(AbgabeSperre::LandungFehlt) {
            notes = landung_fehlt_notiz(None, &notes);
        }
        if let Some(divert) = divert_to
            .as_ref()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty())
        {
            notes.push_str(&format!(
                "\n\n[DIVERT] Planned arrival: {planned}. Actual landing: {actual}.",
                planned = flight.arr_airport,
                actual = divert,
            ));
        }
        // Derselbe Riegel wie ueberall: Der manuelle Weg ist ueber die
        // LAN-Bruecke erreichbar (Cloud-QS 23.09.2026, dritte Runde).
        let reason_line = begruendungs_zeile_mit(reason.as_deref(), "Reason");
        if !reason_line.is_empty() {
            notes.push_str("\n\n");
            notes.push_str(reason_line.trim_end());
        }

        // List which fields the pilot manually overrode so the admin
        // can review at a glance. Helps avoid silently mis-attributing
        // FSM stats to manual entry.
        let mut overrides: Vec<&'static str> = Vec::new();
        if flight_time_minutes.is_some() {
            overrides.push("flight_time");
        }
        if block_fuel_kg.is_some() {
            overrides.push("block_fuel");
        }
        if remaining_fuel_kg.is_some() {
            overrides.push("fuel_used");
        }
        if distance_nm.is_some() {
            overrides.push("distance");
        }
        if cruise_level_ft.is_some() {
            overrides.push("cruise_level");
        }
        if landing_rate_fpm.is_some() {
            overrides.push("landing_rate");
        }
        if block_off_override.is_some() {
            overrides.push("block_off_time");
        }
        if block_on_override.is_some() {
            overrides.push("block_on_time");
        }
        if !overrides.is_empty() {
            notes.push_str(&format!("\n\n[MANUAL OVERRIDES] {}", overrides.join(", ")));
        }

        if let Some(extra) = notes_override
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            notes.push_str("\n\nPilot notes: ");
            notes.push_str(extra);
        }
        FileBody {
            flight_time,
            fuel_used,
            block_fuel,
            distance: Some(resolved_distance),
            level,
            landing_rate,
            score,
            source_name: Some(format!("AeroACARS/{} (manual)", env!("CARGO_PKG_VERSION"))),
            notes: Some(notes),
            fares,
            fields: Some(fields),
            // Left to phpVMS, exactly like the regular path: it rewrites the
            // arrival out of the `Diversion Airport` field above and keeps the
            // planned destination as `alt_airport_id`.
            arr_airport_id: None,
            // Block times — incl. the manual overrides applied to `stats`
            // above. Until now the overrides only annotated the notes block;
            // the values themselves never reached phpVMS.
            block_off_time: stats.block_off_at.map(|t| t.to_rfc3339()),
            block_on_time: stats
                .block_on_at
                .or(stats.landing_at)
                .map(|t| t.to_rfc3339()),
        }
    };
    // Flip the PIREP `source` to MANUAL (0) before submitting. PhpVMS's
    // PirepService::submit() decides ACCEPTED-vs-PENDING based on
    // (source, rank.auto_approve_acars, rank.auto_approve_manual). With
    // source=MANUAL and the VA having `auto_approve_manual=false` on the
    // pilot's rank, the PIREP lands in PENDING for admin review instead
    // of being auto-accepted as a normal ACARS submission.
    //
    // The Acars/UpdateRequest validation rules don't include `source`,
    // but the controller's parsePirep() forwards EVERY request input
    // to mass-assignment, and `source` is in the Pirep $fillable. So
    // the smuggle works as long as upstream phpVMS doesn't change it.
    // Verified against phpvms@dev on 2026-05-03.
    let source_marker = UpdateBody {
        source: Some(api_client::pirep_source::MANUAL),
        ..Default::default()
    };
    if let Err(e) = client.update_pirep(&flight.pirep_id, &source_marker).await {
        tracing::warn!(
            pirep_id = %flight.pirep_id,
            error = %e,
            "could not flip PIREP source to MANUAL — VA admin must filter on source_name"
        );
    } else {
        tracing::info!(pirep_id = %flight.pirep_id, "PIREP source flipped to MANUAL");
    }
    tracing::info!(pirep_id = %flight.pirep_id, "filing PIREP (manual)");
    // Bordbuch einfrieren, bevor eingereicht wird (auch offline).
    bordbuch_abschliessen(&app, &flight);
    nachtrag_vor_dem_einreichen_sichern(&app, &flight);
    landung_vor_dem_einreichen_nachschicken(&app, &flight).await;
    let ergebnis = {
        let _live = PHPVMS_LIVE_MELDUNG.lock().await;
        client.file_pirep(&flight.pirep_id, &body).await
    };
    match ergebnis {
        Ok(()) => {
            // Same landing-history snapshot as the regular file path.
            {
                // v0.15.x: manueller Divert → effektiver Landeflughafen für Trust.
                // Die Sperre nimmt die Funktion selbst (v2.0.18).
                record_landing_for_filed_flight(
                    &app,
                    &flight,
                    divert_to.as_deref().unwrap_or(&flight.arr_airport),
                );
            }
            clear_persisted_flight(&app, Some(&flight.pirep_id));
            log_activity(
                &state,
                ActivityLevel::Warn,
                {
                    let actual_arr = divert_to.as_deref().unwrap_or(&flight.arr_airport);
                    let suffix = if divert_to.is_some() {
                        format!(" (DIVERT, planned {})", flight.arr_airport)
                    } else {
                        String::new()
                    };
                    format!(
                        "Manual PIREP filed: {} {} – {}{}",
                        format_callsign(&flight.airline_icao, &flight.flight_number),
                        flight.dpt_airport,
                        actual_arr,
                        suffix
                    )
                },
                Some("Source tagged 'manual' — admin will review".into()),
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
                    outcome: FlightOutcome::Manual,
                },
            );
            // v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, Bug B/C):
            // der manuelle Filing-Pfad publisht jetzt — wie der normale
            // /file-Pfad — das MQTT-PIREP-Event und lädt das JSONL-
            // Forensik-Logfile hoch. Vorher tat dieser Pfad keins von
            // beidem → manuell gefilte Flüge (z.B. Diverts) fehlten
            // dauerhaft in den VPS-Reports und es gab kein Logfile.
            // `effective_arr` = das Divert-ICAO falls gesetzt (der Aufruf
            // normalisiert es oben auf Grossbuchstaben), sonst das
            // geplante Ziel.
            {
                let effective_arr = divert_to.as_deref().unwrap_or(&flight.arr_airport);
                let mut pirep_payload =
                    build_pirep_payload(&flight, &body, effective_arr, &flight.arr_airport);
                // v0.12.5 (LE2-QS): die manuelle Begründung auch ins
                // MQTT-Payload (notes) — konsistent mit dem
                // `flight_end`-Divert-Pfad, damit Recorder/JSONL den
                // Grund im Audit haben.
                pirep_payload.notes = reason
                    .as_deref()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());
                let pirep_payload_json =
                    serde_json::to_value(&pirep_payload).unwrap_or(serde_json::Value::Null);
                // P-7.21: Eingabe vor dem PIREP bei live.
                eingabe_vor_dem_bericht(&app).await;
                // QS-P1: JSONL-PirepFiled IMMER, MQTT nur bei Handle.
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
            // v0.12.5 (LE7-QS): LandingFinalized-JSONL-Event auch beim
            // manuellen File — vor dem Upload.
            emit_landing_finalized(&app, &flight);
            // Guard VOR dem Await binden und droppen (nicht inline im
            // Funktionsaufruf) — sonst haelt der `MutexGuard` (nicht `Send`)
            // ueber das nachfolgende `.await` hinweg, was diesen
            // Tauri-Command nicht mehr kompilieren laesst. Dieselbe
            // Identitaet geht sowohl in den JSONL-Upload als auch in den
            // Bid-Cleanup-Eintrag.
            let aktuelle_identitaet_fuer_bid_cleanup = state
                .authenticated_pilot_id
                .lock()
                .expect("authenticated_pilot_id lock")
                .map(|id| id.to_string());
            // v0.12.5 (Bug B): Forensik-Upload auch beim manuellen File.
            // Codex-Folgefund (adversarial, 05.09.2026, zehnte Runde): die
            // Identitaet geht jetzt mit — `spawn_flight_log_upload` prueft
            // sie zum Zeitpunkt der tatsaechlichen Ausfuehrung erneut.
            spawn_flight_log_upload(
                &app,
                flight.pirep_id.clone(),
                aktuelle_identitaet_fuer_bid_cleanup.clone(),
                false,
            );
            // v0.7.19 (QS-R1 Finding 1) + QS-R2 Finding 2 (flight_id fallback).
            consume_bid_best_effort(
                &app,
                &client,
                &flight.pirep_id,
                flight.bid_id,
                Some(flight.flight_id.as_str()),
                "manual_filed",
                aktuelle_identitaet_fuer_bid_cleanup,
            )
            .await;
            Ok(())
        }
        Err(e) => {
            tracing::warn!(
                pirep_id = %flight.pirep_id,
                error = %e,
                "manual PIREP file failed — restoring flight to state for retry"
            );
            restore_flight_for_retry(&app, state.inner(), &client, flight);
            Err(e.into())
        }
    }
}

/// v0.13.0 Stream F (LE22-LE26): Pilot-getriggerter Position-Re-Check.
///
/// Workflow:
///   1. AeroACARS-Client startet nach Sim-Crash neu, ActiveFlight wird
///      vom Disk restored mit `was_just_resumed=true`.
///   2. Sim-Position weicht stark ab → `resume_position_suspect=true`.
///   3. ResumeFlightBanner zeigt drei Optionen, EINE davon ist
///      "Position prüfen + fortsetzen". Pilot positioniert sich erst
///      manuell im Sim wieder zur letzten gespeicherten Position, drückt
///      DANN den Button.
///   4. Dieser Command holt die aktuelle Sim-Snapshot, vergleicht die
///      Drift mit `RESUME_DRIFT_CLEAN_NM` und prüft auf airborne↔
///      on_ground-Inkonsistenz.
///   5. Wenn alles OK → setzt `was_just_resumed=false` → Frontend ruft
///      anschließend den normalen Resume-Pfad → kein untrusted-Stempel.
///   6. Wenn nicht OK → returnt drift_nm + still_suspect=true, Pilot
///      kann nochmal positionieren und nochmal prüfen.
///
/// Tolerant gegen Zeit-Gaps bis ~10min — wir prüfen NUR die Position,
/// nicht wie lange der Pilot zum Reposition gebraucht hat.
#[derive(Serialize)]
struct ResumeCheckPositionOutcome {
    ok: bool,
    drift_nm: f64,
    sim_on_ground_inconsistent: bool,
    persisted_phase: &'static str,
    detail: String,
    // v0.13.0 Stream F: aktuelle Sim-Position damit Frontend einen
    // Vergleich anzeigen kann (persisted vs current).
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sim_lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sim_lon: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sim_alt_ft: Option<i32>,
    current_sim_on_ground: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    persisted_lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    persisted_lon: Option<f64>,
    // v0.13.0 Stream F: Fuel/Weight/Aircraft-Vergleich. Frontend zeigt
    // damit "Sim hat 5 t weniger Fuel als gespeichert" oder "anderes
    // Aircraft im Sim geladen" an, sobald der Pilot auf "Position prüfen"
    // drückt.
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sim_fuel_kg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sim_zfw_kg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sim_total_weight_kg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    current_sim_aircraft_icao: Option<String>,
}

#[tauri::command]
async fn flight_resume_check_position(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ResumeCheckPositionOutcome, UiError> {
    // Arc statt Sperre halten: Beim Erfolg schliesst `apply_pause_resume`
    // die Pause (Protokoll, Ablage).
    let flight = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .map(Arc::clone)
        .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?;

    if !flight.was_just_resumed.load(Ordering::Relaxed) {
        // Flight is already live — recheck not applicable
        return Ok(ResumeCheckPositionOutcome {
            ok: true,
            drift_nm: 0.0,
            sim_on_ground_inconsistent: false,
            persisted_phase: "Live",
            detail: "Flight already live, no recheck needed".to_string(),
            current_sim_lat: None,
            current_sim_lon: None,
            current_sim_alt_ft: None,
            current_sim_on_ground: false,
            persisted_lat: None,
            persisted_lon: None,
            current_sim_fuel_kg: None,
            current_sim_zfw_kg: None,
            current_sim_total_weight_kg: None,
            current_sim_aircraft_icao: None,
        });
    }

    let snap = current_snapshot(&app).ok_or_else(|| {
        UiError::new(
            "sim_disconnected",
            "Sim ist nicht verbunden — bitte erst den Sim wieder starten und positionieren",
        )
    })?;

    let (persisted_phase, persisted_pos) = {
        let stats = flight.stats.lock().expect("flight stats");
        let pos = gespeicherter_punkt(&stats);
        (stats.phase, pos)
    };

    let drift_nm = if let Some((plat, plon)) = persisted_pos {
        ::geo::distance_m(plat, plon, snap.lat, snap.lon) / 1852.0
    } else {
        // No persisted position — can't compute drift, treat as clean
        0.0
    };

    // Dieselbe Regel, die das Banner ausgeloest hat (`lage_abweichung`) —
    // sonst saehe der Pilot ein Banner, das kein Zurueckstellen je loest.
    let abweichung = lage_abweichung_fuer(&flight, &snap);
    let sim_on_ground_inconsistent = abweichung == Some(LageAbweichung::AmBoden);
    let ok = abweichung.is_none();

    let detail = match abweichung {
        None => format!(
            "Position OK — Drift {:.2} nm. Flug läuft jetzt als sauber weiter.",
            drift_nm
        ),
        Some(LageAbweichung::AmBoden) => format!(
            "Flugzeug ist im Sim am Boden, gespeicherter Flug war in der Luft (Phase {:?}). Bitte zurück in die Luft positionieren ODER Flug verwerfen.",
            persisted_phase
        ),
        Some(a) => format!(
            "{} Bitte zur letzten Position zurückstellen und nochmal prüfen.",
            lage_abweichung_text(&a)
        ),
    };

    if ok {
        // Pilot hat sich erfolgreich repositioniert — Resume gilt als sauber.
        // Eine offene Sim-Trennung erst JETZT schliessen: Der Streamer hat
        // sie bei der unpassenden Lage bewusst offen gelassen (GAF 278).
        let _ = apply_pause_resume(&app, &flight, Some(&snap), PauseReason::ManualResume);
        // was_just_resumed=false → resume_position_suspect wird ebenfalls
        // false bei nächstem flight_status-Poll → Banner schließt sich.
        flight.was_just_resumed.store(false, Ordering::Relaxed);
        tracing::info!(
            pirep_id = %flight.pirep_id,
            drift_nm,
            "flight_resume_check_position: clean — was_just_resumed cleared"
        );
    } else {
        tracing::info!(
            pirep_id = %flight.pirep_id,
            drift_nm,
            ?abweichung,
            "flight_resume_check_position: still suspect"
        );
    }

    Ok(ResumeCheckPositionOutcome {
        ok,
        drift_nm,
        sim_on_ground_inconsistent,
        persisted_phase: phase_to_str(persisted_phase),
        detail,
        current_sim_lat: Some(snap.lat),
        current_sim_lon: Some(snap.lon),
        current_sim_alt_ft: Some(snap.altitude_msl_ft.round() as i32),
        current_sim_on_ground: snap.on_ground,
        persisted_lat: persisted_pos.map(|(la, _)| la),
        persisted_lon: persisted_pos.map(|(_, lo)| lo),
        current_sim_fuel_kg: Some(snap.fuel_total_kg),
        current_sim_zfw_kg: snap.zfw_kg,
        current_sim_total_weight_kg: snap.total_weight_kg,
        current_sim_aircraft_icao: snap.aircraft_icao.clone(),
    })
}

fn phase_to_str(phase: FlightPhase) -> &'static str {
    match phase {
        FlightPhase::Preflight => "Preflight",
        FlightPhase::Boarding => "Boarding",
        FlightPhase::Pushback => "Pushback",
        FlightPhase::TaxiOut => "TaxiOut",
        FlightPhase::TakeoffRoll => "TakeoffRoll",
        FlightPhase::Takeoff => "Takeoff",
        FlightPhase::Climb => "Climb",
        FlightPhase::Cruise => "Cruise",
        FlightPhase::Holding => "Holding",
        FlightPhase::Descent => "Descent",
        FlightPhase::Approach => "Approach",
        FlightPhase::Final => "Final",
        FlightPhase::Landing => "Landing",
        FlightPhase::TaxiIn => "TaxiIn",
        FlightPhase::BlocksOn => "BlocksOn",
        FlightPhase::Arrived => "Arrived",
        FlightPhase::PirepSubmitted => "PirepSubmitted",
    }
}

/// Cancel the active PIREP without filing it.
#[tauri::command]
async fn flight_cancel(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    force: Option<bool>,
) -> Result<FlightCancelOutcome, UiError> {
    // SCHRITT 1: Nur LESEN, damit wir is_finalizable bestimmen können.
    // guard.take() machen wir erst beim eigentlichen Wegräumen weiter
    // unten — sonst wäre der Flight bei einem zwischenzeitlichen Fehler
    // weg ohne dass irgendwas passiert ist.
    //
    // v0.7.18 (B-014): is_finalizable-Check vor File-First-Versuch.
    // Spec docs/spec/v0.7.18-orphan-flight-cleanup.md §B-014.
    let is_finalizable = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        let flight = guard
            .as_ref()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight is active"))?;
        let stats = flight.stats.lock().expect("flight stats");
        // Annahme: solange flight in state.active_flight liegt UND eine
        // gültige `landing_at`-Markierung gesetzt ist UND die FSM-Phase
        // eine TD-Phase ist, ist der Flug noch nicht gefiled (gefilte
        // Flüge räumen `state.active_flight` selber via flight_end-Pfad).
        // ⚠ Nicht bei erkanntem Sprung: Hier reicht die App von SELBST ein
        // (File-First). Genau das soll nach MSC1588 nicht mehr passieren —
        // und der Resume-Banner ruft `flight_cancel` OHNE `force`, sodass
        // ein Klick auf „Flug verwerfen" den Flug sonst einreicht statt ihn
        // wegzuwerfen (Cloud-QS 23.09.2026). Einreichen darf nur der Pilot
        // im Sprung-Banner.
        matches!(
            stats.phase,
            FlightPhase::Landing
                | FlightPhase::TaxiIn
                | FlightPhase::BlocksOn
                | FlightPhase::Arrived
        ) && stats.landing_at.is_some()
            && abgabe_sperre(&stats).is_none()
    };

    let user_forced_cancel = force.unwrap_or(false);

    if is_finalizable && !user_forced_cancel {
        // PIREP-ID merken bevor flight_end den active_flight-Slot räumt
        let pirep_id_for_outcome = {
            let guard = state.active_flight.lock().expect("active_flight lock");
            guard
                .as_ref()
                .map(|f| f.pirep_id.clone())
                .unwrap_or_default()
        };

        // Best-Effort: erst zu filen versuchen via flight_end-Pfad.
        // flight_end macht selbst:
        //   - Validation (not_at_arrival etc.)
        //   - guard.take() falls Validation OK
        //   - file_pirep_with_retry (3 attempts, transient → pirep_queue)
        //   - record_landing + activity_log + record_event + MQTT publish
        //
        // Outcome-Mapping (siehe Spec + R2-1):
        //   Ok(())                  → Filed ODER Queued (beide OK aus Cancel-Sicht)
        //   Err(code="blocked")     → Err propagieren (B-007 Account-Sperre)
        //   Err(andere Hard-Fails)  → Err mit code="file_first_failed".
        //                             Kein Auto-Cancel mehr (R2-1 UX-Fix).
        //                             active_flight bleibt erhalten, Pilot
        //                             entscheidet im Frontend nochmal.
        match flight_end(app.clone(), state.clone(), None, None, None, None).await {
            Ok(()) => {
                // flight_end hat erfolgreich gefilt ODER gequeued. In
                // beiden Faellen ist active_flight geleert, der PIREP
                // lebt server-side bzw. im Queue-Worker.
                //
                // v0.7.18 (R1-5): Filed vs Queued unterscheiden — Pilot
                // soll wissen ob der PIREP schon eingereicht ist oder
                // noch auf den naechsten Network-Recovery wartet.
                // flight_end hat keinen Outcome-Type; wir checken die
                // pirep_queue auf unsere pirep_id.
                let is_queued = pirep_queue::list_all(&app)
                    .iter()
                    .any(|q| q.pirep_id == pirep_id_for_outcome);

                if is_queued {
                    log_activity(
                        &state,
                        ActivityLevel::Warn,
                        format!(
                            "Flug statt abgebrochen in der Queue für späteren Retry — PIREP {pirep_id_for_outcome}"
                        ),
                        Some(
                            "B-014 File-First-Pfad: transient-Fehler beim Filing. Der Background-Worker reicht den PIREP ein sobald die Verbindung wieder steht.".into(),
                        ),
                    );
                    return Ok(FlightCancelOutcome::Queued {
                        pirep_id: pirep_id_for_outcome,
                    });
                }

                log_activity(
                    &state,
                    ActivityLevel::Info,
                    format!("Flug statt abgebrochen ordentlich eingereicht — PIREP {pirep_id_for_outcome}"),
                    Some("B-014 File-First-Pfad: Server hat den PIREP angenommen.".into()),
                );
                return Ok(FlightCancelOutcome::FiledInstead {
                    pirep_id: pirep_id_for_outcome,
                });
            }
            Err(e) if e.code == "blocked" => {
                // 401/403 Account-Sperre — Cancel würde ebenfalls 401/403
                // werfen, also kein Cancel-Versuch. state.active_flight
                // bleibt unverändert für Re-Try nach VA-Klärung.
                tracing::warn!(
                    code = e.code,
                    "flight_cancel: File-First abgebrochen wegen Account-Sperre"
                );
                return Err(e);
            }
            Err(e) => {
                // v0.7.18 (R2-1): Validierungs-/Hard-Fehler beim Auto-File.
                // Wir fallen NICHT mehr automatisch in den Cancel-Pfad —
                // der Pilot hat "Lieber filen versuchen" geklickt, nicht
                // "bei Fehler trotzdem verwerfen". Das wäre UX-seitig eine
                // versteckte destruktive Aktion.
                //
                // state.active_flight bleibt erhalten (flight_end macht
                // guard.take() nur bei Success), also kann der Pilot:
                //   1. den Fehler beheben + erneut filen,
                //   2. den Cancel-Button nochmal drücken und im zweiten
                //      Dialog explizit "Trotzdem verwerfen" wählen.
                //
                // Frontend (ActiveFlightPanel) catched code="file_first_failed"
                // und zeigt den zweiten Confirm-Dialog.
                tracing::warn!(
                    code = e.code,
                    "flight_cancel: File-First fehlgeschlagen, gebe an UI zurueck statt Auto-Cancel"
                );
                log_activity(
                    &state,
                    ActivityLevel::Warn,
                    format!(
                        "Auto-File vor Cancel fehlgeschlagen ({}) — Pilot muss erneut bestätigen",
                        e.code
                    ),
                    Some(format!("{e:?}")),
                );
                return Err(UiError::new(
                    "file_first_failed",
                    format!(
                        "Filen-Versuch vor Cancel fehlgeschlagen: {} ({}). \
                         Bitte beheben und erneut filen, oder explizit verwerfen.",
                        e.message, e.code
                    ),
                ));
            }
        }
    }

    // SCHRITT 2: Cancel-Pfad. Erst JETZT den Flight aus state.active_flight
    // herausnehmen (guard.take()) bevor wir destruktiv werden.
    //
    // Codex-Folgefund (adversarial, 05.09.2026): zwischen diesem take() und
    // dem `client.cancel_pirep(...).await` weiter unten ist der State-Slot
    // leer. Ohne diesen Guard koennte `flight_start`/`flight_adopt` in genau
    // dieser Luecke bereits einen NEUEN Flug anlegen — derselbe Schutz, den
    // `flight_end` fuer sein eigenes take→file-Fenster schon seit v0.20.x
    // haelt (siehe `FlightSetupGuard`-Doc-Kommentar). Kein `disarm()` noetig:
    // dieser Pfad schreibt nie wieder einen Flug in `active_flight` zurueck,
    // der Guard faellt bei JEDEM Rueckgabepfad (inkl. `result?` weiter
    // unten) per `Drop` automatisch.
    let _lifecycle_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;
    // Boarding-Musik (2.1.1): Flugende, Abbruch und Vergessen stoppen sie sofort.
    kabinen_musik::aus();
    let flight = {
        let mut guard = state.active_flight.lock().expect("active_flight lock");
        guard.take().ok_or_else(|| {
            UiError::new(
                "no_active_flight",
                "flight was concurrently removed — likely already filed",
            )
        })?
    };
    // v0.6.0: Outbox VOR stop=true leeren — sonst persistiert der
    // phpVMS-Worker beim nächsten Tick (im stop-branch) die noch
    // gepufferten Positions zurück in position_queue.json. Bei einem
    // Cancel will der Pilot aber explizit dass NICHTS mehr an phpVMS
    // gesendet wird.
    flight
        .position_outbox
        .lock()
        .expect("position_outbox lock")
        .clear();
    flight.stop.store(true, Ordering::Relaxed);
    let client = current_client(&state)?;
    let result = {
        let _live = PHPVMS_LIVE_MELDUNG.lock().await;
        client.cancel_pirep(&flight.pirep_id).await
    };
    // Clear local persistence — but only THIS flight's: the await above is
    // exactly the gap where a newly started flight could already own the
    // (single, fixed) persistence path. See `sollte_persistierten_flug_loeschen`.
    clear_persisted_flight(&app, Some(&flight.pirep_id));
    discard_queued_positions_for(&app, &flight.pirep_id);
    pirep_queue::remove(&app, &flight.pirep_id);
    // Die technischen Zeilen mitschicken — NUR die, ohne Flugprotokoll.
    //
    // Ein abgebrochener Flug lud bisher gar nichts hoch. Genau dort, wo die
    // Abbrüche das Problem sind, hatten wir deshalb keine Daten (Joel,
    // 22.09.2026: zwei Flüge nach Stunden weggeworfen, Ursache unbekannt).
    //
    // ⚠ VOR `result?`: Scheitert die Stornierung am Server (kein Netz, 5xx),
    // kehrt die Funktion mit Err zurueck — und ausgerechnet der Abbruch ohne
    // Netz, fuer den dieser Weg gebaut wurde, haette nie etwas hochgeladen
    // (Cloud-QS 23.09.2026).
    let besitzer = state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
        .map(|id| id.to_string());
    spawn_flight_log_upload(&app, flight.pirep_id.clone(), besitzer, true);
    result?;
    log_activity(
        &state,
        ActivityLevel::Warn,
        format!(
            "Flight cancelled: {} {} – {}",
            format_callsign(&flight.airline_icao, &flight.flight_number),
            flight.dpt_airport,
            flight.arr_airport
        ),
        Some(format!("PIREP {}", flight.pirep_id)),
    );
    record_event(
        &app,
        &flight.pirep_id,
        &FlightLogEvent::FlightEnded {
            timestamp: Utc::now(),
            pirep_id: flight.pirep_id.clone(),
            outcome: FlightOutcome::Cancelled,
        },
    );
    Ok(FlightCancelOutcome::Cancelled {
        pirep_id: flight.pirep_id.clone(),
    })
}

/// v0.7.18 (B-014) — Outcome eines `flight_cancel`-Aufrufs.
///
/// Spec docs/spec/v0.7.18-orphan-flight-cleanup.md §B-014. Frontend
/// rendert pro Variante eine andere Bestätigung.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlightCancelOutcome {
    /// File-First-Versuch hat geklappt — PIREP wurde direkt an phpVMS
    /// abgeschickt und akzeptiert.
    FiledInstead { pirep_id: String },
    /// File-First-Versuch hat transienten Fehler (Netz, 5xx, Timeout)
    /// produziert — PIREP liegt jetzt in der pirep_queue für Background-
    /// Retry. Cancel hat NICHT stattgefunden (sonst Datenverlust).
    Queued { pirep_id: String },
    /// Pilot hat explizit „Trotzdem verwerfen" gewählt (force=true ODER
    /// nicht-finalisierbarer Flug). Regulärer Cancel-Pfad ist gelaufen,
    /// PIREP ist CANCELLED auf phpVMS. (R2-1: File-First-HardFail führt
    /// hier NICHT mehr automatisch hin — der Pilot muss explizit zweiten
    /// Confirm-Dialog bestätigen.)
    Cancelled { pirep_id: String },
}

/// Confirm an auto-resumed flight: now actually spawn the position streamer.
/// Called by the resume banner when its 10-second countdown elapses (or the
/// user clicks "Resume now"). Until this fires, no position posts go out, so
/// the user can still cancel without leaving footprints on phpVMS.
#[tauri::command]
async fn flight_resume_confirm(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    // true nur vom Knopf „Trotzdem fortsetzen": Der Pilot hat die Warnung
    // gelesen und fuehrt den Flug bewusst weiter. Der Countdown und der
    // normale Weg schicken das NICHT (Cloud-QS 23.09.2026).
    force: Option<bool>,
    // true nur vom Knopf „Vom Boden weiterfliegen" (GAF 278).
    #[allow(non_snake_case)] vomBoden: Option<bool>,
) -> Result<(), UiError> {
    let flight = {
        let guard = state.active_flight.lock().expect("active_flight lock");
        guard
            .as_ref()
            .cloned()
            .ok_or_else(|| UiError::new("no_active_flight", "no flight to resume"))?
    };
    let client = current_client(&state)?;
    // v0.12.5 (Spec docs/spec/v0.12.5-divert-and-manual-pirep.md, LE9):
    // NICHT mehr bedingungslos scharfschalten. Bisher spawnte
    // flight_resume_confirm Streamer/phpVMS-Worker/Sampler sofort — nach
    // einem App-/Mac-Neustart mitten im Flug lief der Flug damit OHNE
    // Simulator (eingefrorene Stats wurden an phpVMS gepostet). Stattdessen
    // wartet ein Sim-Gate-Task auf einen FRISCHEN Sim-Snapshot und schaltet
    // erst dann scharf. `was_just_resumed` bleibt true bis dahin → die UI
    // zeigt weiter den Resume-/Warte-Zustand statt einen scheinbar laufenden
    // Flug. Sim-agnostisch (X-Plane + MSFS via `current_snapshot`).
    // Nur EIN Gate je Flug; ein zweites Bestaetigen ist ein klarer No-op.
    // Auch NACH dem Gate: Ein verspaeteter zweiter Aufruf haette sonst ein
    // neues Gate gestartet — Wartehinweis und Heartbeats neben dem laufenden
    // Flug (Codex-QS 25.09.2026).
    if fortsetzen_laeuft_schon(&flight) {
        tracing::info!(pirep_id = %flight.pirep_id, "Flug laeuft schon — Bestaetigen ignoriert");
        return Ok(());
    }
    {
        let mut st = flight.stats.lock().expect("flight stats");
        if st.resume_gate_laeuft {
            tracing::info!(pirep_id = %flight.pirep_id, "resume gate laeuft schon — zweites Bestaetigen ignoriert");
            return Ok(());
        }
        st.resume_gate_laeuft = true;
    }
    spawn_resume_sim_gate(
        app,
        Arc::clone(&flight),
        client,
        force.unwrap_or(false),
        vomBoden.unwrap_or(false),
    );
    Ok(())
}

/// Laeuft der Flug schon scharf? Dann gibt es nichts mehr fortzusetzen.
/// Ein Streamer, der wegen eines Sim-Absturzes mitten im Flug pausiert
/// (`was_just_resumed` wieder gesetzt), zaehlt NICHT — den muss das
/// Bestaetigen weiter freigeben koennen.
fn fortsetzen_laeuft_schon(flight: &ActiveFlight) -> bool {
    flight.streamer_spawned.load(Ordering::SeqCst)
        && !flight.was_just_resumed.load(Ordering::Relaxed)
}

/// v0.12.5 (LE9): Resume-Sim-Gate. Wartet nach einem Flight-Resume auf
/// einen frischen Sim-Snapshot (≤ `SIM_GATE_FRESH_SECS`) und schaltet den
/// Flug erst dann scharf — Streamer + phpVMS-Worker + Touchdown-Sampler.
/// Bis dahin: kein Streaming, kein phpVMS-Post. Bricht ab, wenn der Flug
/// nicht mehr aktiv ist (Cancel/Forget/End) oder `stop` gesetzt wurde.
fn spawn_resume_sim_gate(
    app: AppHandle,
    flight: Arc<ActiveFlight>,
    client: Client,
    trotzdem_fortsetzen: bool,
    vom_boden: bool,
) {
    tauri::async_runtime::spawn(async move {
        log_activity_handle(
            &app,
            ActivityLevel::Warn,
            "Warte auf Simulator — Flug wird scharfgeschaltet, sobald der Sim geladen und ruhig ist"
                .to_string(),
            Some(
                "Resume nach App-/Sim-Neustart: kein Streaming und kein phpVMS-Post, \
                 bis der Simulator geladen ist und ruhig liefert."
                    .to_string(),
            ),
        );
        // ⚠ Nicht beim ERSTEN frischen Wert scharfschalten. Nach einem Neustart
        // zeigt MSFS minutenlang Menue, Weltkarte und Ladezustaende — am
        // 24.09.2026 (Joel, OCN 712) ein anderes Flugzeug mit 0 kg Sprit an
        // einem anderen Ort, und spaeter 20.926.040 ft Hoehe. Das Gate schaltete
        // beim ersten dieser Werte scharf; daraus wurden drei „Spruenge", ein
        // vorgetaeuschter Durchstart, und der Landesprit war weg.
        //
        // Dieselbe Regel wie beim Auto-Start (`SimRuhe`): nicht pausiert, keine
        // springende Position, dasselbe Flugzeug — und das eine Weile lang.
        // Dazu der Filter, der auch im Streamer vor allem anderen steht.
        let _wartegrund_raeumen = WartegrundRaeumen(Arc::clone(&flight));
        let mut ruhe = SimRuhe::default();
        let mut wartet_seit: Option<DateTime<Utc>> = None;
        let mut letzter_grund: Option<String> = None;
        let mut gewarnt = false;
        let mut letzter_heartbeat = std::time::Instant::now();
        loop {
            // Flug abgebrochen / beendet / vergessen → Gate beenden.
            if flight.stop.load(Ordering::Relaxed) {
                flight.stats.lock().expect("flight stats").resume_wartet = None;
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    "resume sim-gate: flight stopped — exiting without arming"
                );
                return;
            }
            let still_active = {
                let st = app.state::<AppState>();
                let guard = st.active_flight.lock().expect("active_flight lock");
                guard
                    .as_ref()
                    .map(|f| f.pirep_id == flight.pirep_id)
                    .unwrap_or(false)
            };
            if !still_active {
                flight.stats.lock().expect("flight stats").resume_wartet = None;
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    "resume sim-gate: flight no longer active — exiting"
                );
                return;
            }
            let jetzt = Utc::now();
            match resume_gate_takt(&mut ruhe, current_snapshot(&app), jetzt) {
                Err(grund) => {
                    // phpVMS will auch waehrend des Wartens ein Lebenszeichen,
                    // sonst loescht sein Cron den PIREP nach `acars.live_time`
                    // (~2 h) — etwa wenn der Pilot den geladenen Sim pausiert
                    // stehen laesst (Claude-QS 24.09.2026). Der Body kommt aus
                    // dem GESPEICHERTEN Stand, nie aus einem Menue-Wert.
                    if letzter_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                        // Die Uhr laeuft ab dem VERSUCH, nicht ab dem Erfolg —
                        // sonst hagelte es bei einem fehlerhaften phpVMS alle
                        // 2 s einen neuen Versuch.
                        letzter_heartbeat = std::time::Instant::now();
                        let body = {
                            let st = flight.stats.lock().expect("flight stats");
                            let hb = SimSnapshot {
                                altitude_msl_ft: st
                                    .last_persisted_snapshot
                                    .as_ref()
                                    .map_or(0.0, |p| p.altitude_ft),
                                ..SimSnapshot::default()
                            };
                            // Dieselbe Phase wie der Streamer meldet.
                            build_heartbeat_body(&hb, &st, st.phase)
                        };
                        // In einem EIGENEN Task: Ein haengendes phpVMS (bis zu
                        // 10 s) haette sonst jeden Gate-Takt von 2 auf 12 s
                        // gestreckt — das Gate saehe Ladezwischenstaende gar
                        // nicht mehr (Codex + Claude-QS 24.09.2026).
                        let app_hb = app.clone();
                        let flight_hb = Arc::clone(&flight);
                        let client_hb = client.clone();
                        tauri::async_runtime::spawn(async move {
                            let _live = PHPVMS_LIVE_MELDUNG.lock().await;
                            if flight_hb.stop.load(Ordering::Relaxed) {
                                return;
                            }
                            match client_hb.update_pirep(&flight_hb.pirep_id, &body).await {
                                Ok(()) => {
                                    flight_hb
                                        .stats
                                        .lock()
                                        .expect("flight stats")
                                        .last_heartbeat_at = Some(Utc::now());
                                }
                                // Nur fuer einen Flug, der noch LAEUFT: Wurde er
                                // inzwischen beendet, abgebrochen oder vergessen,
                                // loeste ein verspaeteter 404 sonst die Meldung
                                // „vom Server gecancelt" aus — und die Oberflaeche
                                // leerte kurz einen schon NEUEN Flug (Claude-QS
                                // 24.09.2026).
                                Err(ApiError::NotFound)
                                    if !flight_hb.stop.load(Ordering::Relaxed) =>
                                {
                                    // PIREP auf dem Server geloescht — wie im
                                    // Streamer. Setzt `flight.stop`; das Gate
                                    // endet damit beim naechsten Takt.
                                    handle_remote_cancellation(
                                        &app_hb,
                                        &flight_hb,
                                        "POST update (resume gate)",
                                    );
                                }
                                Err(e) => tracing::warn!(
                                    pirep_id = %flight_hb.pirep_id,
                                    error = ?e,
                                    "resume sim-gate: Heartbeat fehlgeschlagen"
                                ),
                            }
                        });
                    }
                    let seit = *wartet_seit.get_or_insert(jetzt);
                    let gewartet_s = (jetzt - seit).num_seconds();
                    // Der Pilot SIEHT, worauf gewartet wird — vorher stand der
                    // Grund nur im Debug-Log, und das Gate konnte ohne ein
                    // Wort unbegrenzt warten (Cloud-QS 24.09.2026).
                    {
                        let mut st = flight.stats.lock().expect("flight stats");
                        st.resume_wartet = Some((grund.clone(), gewartet_s));
                    }
                    // Nur die ART des Grundes zaehlt, nicht der Sekundenzaehler —
                    // sonst schrieb das Gate alle paar Sekunden einen Eintrag
                    // (Cloud-QS 24.09.2026). Den Zaehlstand selbst zeigt das
                    // Cockpit; ins Protokoll gehoeren nur echte Gruende.
                    let art: String = grund.chars().filter(|c| !c.is_ascii_digit()).collect();
                    let nur_zaehlstand = grund.contains(SIM_RUHE_ZAEHLSTAND);
                    if !nur_zaehlstand && letzter_grund.as_deref() != Some(art.as_str()) {
                        log_activity_handle(
                            &app,
                            ActivityLevel::Info,
                            "Warte auf den Simulator".to_string(),
                            Some(grund.clone()),
                        );
                        letzter_grund = Some(art);
                    }
                    if gewartet_s >= RESUME_GATE_WARNUNG_SECS && !gewarnt {
                        gewarnt = true;
                        log_activity_handle(
                            &app,
                            ActivityLevel::Warn,
                            "Der Flug wartet ungewöhnlich lange auf den Simulator".to_string(),
                            Some(
                                "Ist der Simulator pausiert, im Slew-Modus oder läuft \
                                 Zeitbeschleunigung? Solange zeichnet AeroACARS nichts \
                                 auf. Alternativ den Flug verwerfen."
                                    .to_string(),
                            ),
                        );
                    }
                }
                Ok(snap) => {
                    // Zwischen Banner-Pruefung und ruhigem Sim kann der Sim
                    // woanders neu geladen haben. Ohne „Trotzdem fortsetzen"
                    // wird dann nicht scharfgeschaltet: Das Gate endet, und
                    // der Banner fragt erneut (Codex-Abnahme 24.10.2026).
                    if !trotzdem_fortsetzen {
                        if let Some(abweichung) = lage_abweichung_fuer(&flight, &snap) {
                            log_activity_handle(
                                &app,
                                ActivityLevel::Warn,
                                "Lage passt nicht zum Flug — nicht fortgesetzt, bitte im Banner bestätigen"
                                    .to_string(),
                                Some(lage_abweichung_text(&abweichung)),
                            );
                            tracing::warn!(
                                pirep_id = %flight.pirep_id,
                                ?abweichung,
                                "resume sim-gate: position does not fit — not arming"
                            );
                            return;
                        }
                    }
                    // ⚠ Bevor der Flug scharf wird: Passt die Lage im Sim
                    // ueberhaupt zum gespeicherten Flug? Der Pilot kann hier
                    // ueber „Trotzdem fortsetzen" hergekommen sein (GAF 9655,
                    // 23.09.2026) — dann steht das im Bericht, und er
                    // entscheidet am Ende selbst ueber die Abgabe.
                    if trotzdem_fortsetzen {
                        // VOR dem Schliessen der Pause: Danach fehlt der
                        // Pausenstand, an dem „am Boden" gemessen wird.
                        let abweichung = lage_abweichung_fuer(&flight, &snap);
                        sprung_beim_fortsetzen_vermerken(&app, &flight, &snap);
                        // Der Pilot hat entschieden: Eine offene Sim-Trennung
                        // hier schliessen. Sonst hielte der neue Streamer die
                        // unpassende Lage erneut an, und das Banner kaeme
                        // sofort wieder (GAF 278).
                        let _ = apply_pause_resume(
                            &app,
                            &flight,
                            Some(&snap),
                            PauseReason::ManualResume,
                        );
                        // Auf einem anderen Platz am Boden neu geladen: von dort
                        // weiterfliegen statt sofort „Arrived" (GAF 278).
                        let am_ziel = {
                            let ref_pos =
                                flight.stats.lock().expect("flight stats").planned_arr_ref_pos;
                            arrival::locate(&flight.arr_airport, snap.lat, snap.lon, ref_pos)
                                .is_at_planned()
                        };
                        let vorher = {
                            let mut st = flight.stats.lock().expect("flight stats");
                            let vorher = st.phase;
                            if vom_boden && bodenneustart_moeglich(&st, abweichung, am_ziel) {
                                bodenneustart_vorbereiten(&mut st, Utc::now());
                                Some(vorher)
                            } else {
                                None
                            }
                        };
                        if vom_boden && vorher.is_none() {
                            log_activity_and_record(
                                &app,
                                &flight.pirep_id,
                                ActivityLevel::Warn,
                                "Weiterflug vom Boden nicht möglich — Flug läuft in der bisherigen Phase weiter"
                                    .to_string(),
                                Some(format!(
                                    "Beim Scharfschalten passte die Lage nicht mehr (nicht am Boden, am Ziel oder \
                                     Landung schon begonnen): {}",
                                    abweichung.as_ref().map_or_else(
                                        || "Lage passt".to_string(),
                                        lage_abweichung_text
                                    ),
                                )),
                            );
                        }
                        if let Some(vorher) = vorher {
                            record_event(
                                &app,
                                &flight.pirep_id,
                                &FlightLogEvent::PhaseChanged {
                                    timestamp: Utc::now(),
                                    from: vorher,
                                    to: FlightPhase::TaxiOut,
                                    altitude_msl_ft: snap.altitude_msl_ft,
                                    groundspeed_kt: snap.groundspeed_kt,
                                    altitude_agl_ft: snap.altitude_agl_ft,
                                },
                            );
                            log_activity_and_record(
                                &app,
                                &flight.pirep_id,
                                ActivityLevel::Warn,
                                format!("Weiterflug vom Boden — neuer Start, Ziel bleibt {}", flight.arr_airport),
                                Some(format!(
                                    "Nach dem Sim-Neustart am Boden bei LAT {:.4}° · LON {:.4}°. Der Flug steht auf \
                                     Rollen; Abflugzeit und Startwerte bleiben die ursprünglichen. Der Sprung ist \
                                     für die Prüfung vermerkt.",
                                    snap.lat, snap.lon,
                                )),
                            );
                        }
                    }
                    // Die Wartezeit war eine Luecke wie jede andere: In ihr lief
                    // weder Streamer noch Aufsetz-Sampler. Ohne diesen Aufschlag
                    // haette ein Neustart im Endanflug die Landung verpasst, ohne
                    // dass die Abgabe-Sperre davon wuesste (Cloud-QS 24.09.2026).
                    {
                        let mut st = flight.stats.lock().expect("flight stats");
                        st.resume_wartet = None;
                        // Nur eine BEKANNTE Luecke verlaengern: Eine unbekannte
                        // (Altdatei ohne `zuletzt_geschrieben`) bleibt unbekannt —
                        // sonst wuerde aus „im Zweifel fliegbar" eine kurze,
                        // strenge Luecke (Cloud-QS 24.09.2026).
                        let seit = st.resume_wiederhergestellt_am.take();
                        luecke_aufschlagen(&mut st, seit, jetzt);
                    }
                    save_active_flight(&app, &flight);
                    flight.was_just_resumed.store(false, Ordering::Relaxed);
                    spawn_phpvms_position_worker(app.clone(), Arc::clone(&flight), client.clone());
                    spawn_position_streamer(app.clone(), Arc::clone(&flight), client.clone());
                    spawn_touchdown_sampler(app.clone(), Arc::clone(&flight));
                    tracing::info!(
                        pirep_id = %flight.pirep_id,
                        gewartet_s = wartet_seit.map_or(0, |t| (jetzt - t).num_seconds()),
                        "resume sim-gate: Simulator ruhig — flight armed"
                    );
                    log_activity_handle(
                        &app,
                        ActivityLevel::Info,
                        "Simulator bereit — Flug scharfgeschaltet".to_string(),
                        None,
                    );
                    return;
                }
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}
