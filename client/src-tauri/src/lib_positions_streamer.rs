/// Die Zeitanteile eines Streamer-Takts.
///
/// # Warum (AIB 429, 12.10.2026)
///
/// „Streamer-Takt zu lang" nannte nur die Gesamtzeit. Welcher Teil die
/// 460–600 ms kostete, liess sich erst aus dem Quelltext erschliessen.
/// Jetzt traegt die Warnung die Anteile: `abschnitt(name)` schreibt die
/// Zeit seit der vorigen Marke unter `name`, `bericht` haengt den Rest
/// bis jetzt an. Eine Sperre, auf die gewartet wird, steckt im Anteil
/// des Abschnitts, der sie nimmt.
#[derive(Default)]
struct Taktuhr {
    letzte: Option<std::time::Instant>,
    anteile: Vec<(&'static str, u128)>,
}

impl Taktuhr {
    fn start(&mut self) {
        self.letzte = Some(std::time::Instant::now());
        self.anteile.clear();
    }

    fn abschnitt(&mut self, name: &'static str) {
        if let Some(davor) = self.letzte {
            let jetzt = std::time::Instant::now();
            self.anteile
                .push((name, jetzt.duration_since(davor).as_millis()));
            self.letzte = Some(jetzt);
        }
    }

    /// `name=ms` je Abschnitt ab 5 ms, in Ablaufreihenfolge, plus `rest`.
    fn bericht(&self) -> String {
        let rest = self.letzte.map_or(0, |t| t.elapsed().as_millis());
        self.anteile
            .iter()
            .copied()
            .chain(std::iter::once(("rest", rest)))
            .filter(|(_, ms)| *ms >= 5)
            .map(|(name, ms)| format!("{name}={ms}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
#[path = "tests_lib/taktuhr_tests.rs"]
mod taktuhr_tests;

fn spawn_position_streamer(app: AppHandle, flight: Arc<ActiveFlight>, client: Client) {
    let Some(meine_generation) = startsperre_nehmen(&flight, &flight.streamer_spawned) else {
        tracing::debug!(
            pirep_id = %flight.pirep_id,
            "position streamer already running, skipping spawn"
        );
        return;
    };
    // Die OurAirports-Tabellen jetzt laden, nicht beim ersten Gebrauch unter
    // der Flugdaten-Sperre (`runway::tabellen_vorwaermen_im_hintergrund`).
    let _ = runway::tabellen_vorwaermen_im_hintergrund();
    tauri::async_runtime::spawn(async move {
        tracing::info!(pirep_id = %flight.pirep_id, "position streamer started");
        // v0.19.3: attempts made to fetch the arrival airport's reference
        // position (the divert-geometry fallback). Bounded, not one-shot: a
        // single transient error on the FIRST tick — the tick most likely to hit
        // a cold connection — must not silently disable divert detection for the
        // whole flight, which is exactly the bug this fallback exists to fix.
        // Bounded so a phpVMS outage cannot become a per-tick request storm.
        let mut arr_ref_pos_attempts: u8 = 0;
        // Die Abfrage selbst läuft als eigener Auftrag (der Streamer wartete
        // sonst bis zu 10 s auf phpVMS). `unterwegs`: höchstens eine zugleich;
        // `aufgeben`: phpVMS kennt den Platz, aber ohne Koordinaten.
        let arr_ref_unterwegs = Arc::new(AtomicBool::new(false));
        let arr_ref_aufgeben = Arc::new(AtomicBool::new(false));
        // Streamer ticks since the flight started. Only used to space the
        // arrival-reference-position retries out over the flight instead of
        // burning them all in the first seconds.
        let mut tick_count: u32 = 0;
        // v1.6.14 — Ausduennung fuer den phpVMS-Pfad. Bis hierher ging JEDER
        // Streamer-Tick als eigene Zeile an phpVMS: gemessen ein Punkt alle
        // 2,9 s, ein 77-Minuten-Flug ergab 1.610 Zeilen, die groessten Tracks
        // in der GSG-Datenbank haben ueber 17.000. phpVMS' eigene Vorgabe
        // (`acars.update_interval`) steht auf 60 s.
        //
        // Die Drosselung von v0.13.8 (Cruise 30 -> 60 s) wirkte nur auf die
        // POST-Kadenz, nicht auf die Punktzahl — die Zeilen entstanden
        // weiter im Tick-Takt, sie wurden nur groesser gebuendelt. Jetzt
        // richtet sich auch der Punktabstand nach `position_interval`.
        //
        // Betrifft NUR phpVMS. Der VPS-Recorder (MQTT) und die
        // JSONL-Forensik bekommen weiterhin jeden Tick — daran haengen
        // Landebewertung, Live-Karte und Nachanalyse.
        let mut letzter_phpvms_push: Option<std::time::Instant> = None;
        let mut zuletzt_gepushte_phase: Option<FlightPhase> = None;
        // Audit 26.09.2026: Diagnose-Rohwerte unklar belegter Cockpit-
        // Schalter nur bei Aenderung ins Flug-Log (siehe
        // `SimSnapshot::cockpit_rohwerte_nur_bei_aenderung`) — sonst kaemen
        // sie mit jedem Tick erneut mit.
        let mut letzte_cockpit_rohwerte: Option<sim_core::CockpitRohwerte> = None;
        // One warning per "sim is loading" episode, not one per tick.
        let mut null_island_logged = false;
        // Heartbeat tracker: ensures `POST /pireps/{id}/update` fires at
        // least every `HEARTBEAT_INTERVAL` so phpVMS's RemoveExpiredLiveFlights
        // cron never reaches the inactivity threshold. Initialised one
        // interval in the past so the first eligible tick triggers it.
        let mut last_heartbeat: std::time::Instant = std::time::Instant::now() - HEARTBEAT_INTERVAL;
        // Lebenszeichen und ACARS-Log gehen über einen eigenen Arbeiter an
        // phpVMS — siehe `spawn_phpvms_meldungen`. Der Streamer wartet nicht
        // mehr auf die Antwort.
        let phpvms_meldungen = spawn_phpvms_meldungen(app.clone(), flight.clone(), client.clone(), meine_generation);
        // Phase-adaptive cadence: re-read the current phase on every tick so
        // the sleep matches what the aircraft is actually doing right now
        // (5 s during takeoff/landing, 8 s on approach/final, 10 s on the
        // ground / climb / descent, 30 s in cruise — capped at 30 s so the
        // live map never goes more than half a minute stale).
        // v0.4.1: track last good snapshot so we can detect Sim-Disconnect
        // (kein Snapshot mehr für > SIM_DISCONNECT_THRESHOLD_S s) und den
        // letzten bekannten Stand für Repositions-Anzeige im Cockpit-Banner
        // einfrieren.
        let mut last_good_snap: Option<SimSnapshot> = None;
        let mut last_good_snap_at: Option<std::time::Instant> = None;
        // v0.6.0: `last_phpvms_post_at` ist entfallen — der Streamer-Tick
        // postet nicht mehr selber an phpVMS (push to outbox only).
        // Die Cadence-Steuerung sitzt jetzt im phpVMS-Worker.
        //
        // v0.12.4 (Spec docs/spec/v0.12.4-score-consistency.md, LE4):
        // Edge-Detektor für das touchdown_rollout_finalized-Event. Hält den
        // `landing_at`-Zeitstempel des zuletzt gepublishten Touchdowns —
        // so wird bei Touch-and-Go / Stop-and-Go JEDER Touchdown mit seinem
        // EIGENEN Rollout gepublisht (nicht nur der erste). Loop-lokal —
        // übersteht keinen App-Neustart, was OK ist: nach einem Neustart
        // re-published der erste Tick einmal idempotent (Recorder-Patch ist
        // idempotent + touchdown-scharf), siehe LE7 Punkt 5.
        // ⚠ (Aufsetzzeitpunkt, Stand der Bahnwerte). Der Stand gehoert
        // dazu, damit eine spaet eingetroffene Szenerie den Nachtrag
        // ERNEUT ausloest — sonst behaelt der Recorder die alten Achsen-
        // und Ausfahrtsdaten (QS-Befund 1, erste Runde v1.7.15).
        // ⚠ Aus dem Flug geseedet, nicht `None`: Nach einem Neustart
        // sendete der Streamer sonst blind erneut — mit dem, was gerade im
        // Speicher stand (externe QS, Runde 3, P1).
        let mut last_published_rollout: Option<(i64, u32)> =
            flight.stats.lock().ok().and_then(|s| s.letzter_nachtrag);
        // Ausloeser des Ausfahrten-Vorwaermens — siehe
        // `ausfahrten_im_hintergrund`.
        let mut ausfahrten_ausloeser: Option<VorwaermAusloeser> = None;
        let mut ausfahrten_laeufer: Option<std::thread::JoinHandle<()>> = None;
        // Damit die Warnung „keine MQTT-Verbindung" nicht bei jedem Tick
        // kommt, solange die Sperre nicht faellt.
        let mut mqtt_fehlt_gemeldet = false;
        // v0.13.18: FOB-abgeleiteter Fuel-Flow-Fallback. Manche Addons
        // (iniBuilds/Aerosoft A340-600, ToLiss-Engine-Modell) treiben die
        // Standard-SimVar `ENG FUEL FLOW PPH` NICHT → `fuel_flow_kg_per_h`
        // bleibt None, obwohl Sprit verbrannt wird (Live IRM1140/IBE778). Der
        // Tankinhalt (FOB) läuft aber korrekt runter, also leiten wir die Rate
        // daraus ab: fuel_flow ≈ ΔFOB / Δt, neu berechnet über ein ~60-s-
        // Fenster (glättet FOB-Rauschen) und dazwischen gehalten. Loop-lokal,
        // kein FlightStats/Resume-Touch. Greift NUR wenn die direkte SimVar
        // fehlt → kein Regress. Reine Live-Map-/Telemetrie-Anzeige (der
        // Gesamt-Verbrauch wird ohnehin aus dem FOB-Delta gerechnet).
        let mut ff_last_fob_kg: Option<f32> = None;
        let mut ff_last_fob_at: Option<std::time::Instant> = None;
        let mut ff_derived_kg_per_h: Option<f32> = None;
        // Selbstmessung (AZU 8710, 11.10.2026): Bis 2.0.9 stand der Streamer
        // bei jedem Phasenwechsel und alle 30 s für 1,5–3,5 s, weil er auf
        // phpVMS wartete — gefunden erst nachträglich über Lücken im
        // Flugprotokoll. Jetzt meldet er selbst, wenn ein Takt (Arbeit ohne
        // das geplante Warten) länger als 250 ms braucht. Gedeckelt, damit
        // ein Dauerproblem das Protokoll nicht flutet.
        //
        // v2.0.17: Schwelle 30 ms statt 250 ms. Die Aufsetz-Lücke von AIB 431
        // (129 ms) lag weit darunter und blieb deshalb unsichtbar. Wer die
        // Flugdaten-Sperre hält, meldet zusätzlich `stats_sperre` selbst.
        let mut takt_beginn: Option<std::time::Instant> = None;
        let mut takt_uhr = Taktuhr::default();
        let mut lange_takte_gemeldet: u32 = 0;
        loop {
            let current_phase = {
                let stats = flight.stats.lock().expect("flight stats");
                stats.phase
            };
            if let Some(beginn) = takt_beginn.take() {
                let arbeit_ms = beginn.elapsed().as_millis();
                if arbeit_ms > 30 && lange_takte_gemeldet < 40 {
                    lange_takte_gemeldet += 1;
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        phase = ?current_phase,
                        arbeit_ms = arbeit_ms as u64,
                        anteile = %takt_uhr.bericht(),
                        "Streamer-Takt zu lang"
                    );
                }
            }
            // v0.5.35: loop runs at adaptive cadence — 3s in Cruise/Climb/etc,
            // schneller (1s, 500ms) wenn der Pilot nah am Boden ist
            // (Approach/Final/Landing/Takeoff unter 1000ft AGL).
            // Verhindert dass der Touchdown-Moment in eine Sample-Lücke
            // fällt — User-Report 2026-05-08: GA-Flug C152 hatte
            // 10s Mean-Interval und peak_vs_fpm=-33 statt realer ~-350.
            // phpVMS POST throttled separately further down (8-30s).
            let tick_interval = {
                let (phase, spur_laeuft) = {
                    let st = flight.stats.lock().expect("flight stats");
                    (st.phase, st.bahn_spur_laeuft)
                };
                let agl = last_good_snap.as_ref().map(|s| s.altitude_agl_ft);
                let og = last_good_snap.as_ref().map(|s| s.on_ground);
                let gs = last_good_snap.as_ref().map(|s| s.groundspeed_kt as f64);
                adaptive_tick_interval_v2(phase, agl, og, gs, spur_laeuft)
            };
            tokio::time::sleep(tick_interval).await;
            takt_beginn = Some(std::time::Instant::now());
            takt_uhr.start();
            // Ueberholte Generation (Flug fortgesetzt): der neue Streamer
            // uebernimmt — s. `ActiveFlight::generation`.
            if flight.stop.load(Ordering::Relaxed)
                || flight.generation.load(Ordering::SeqCst) != meine_generation
            {
                break;
            }

            // v0.13.9 Stream F (Mid-Session Sim-Crash-Recovery — LE27):
            // Wenn was_just_resumed=true gesetzt ist, pausiert der Streamer
            // den Position-Push (kein FSM-Step, kein Outbox-Push, kein
            // MQTT-Position-Publish, kein PhpVMS-Worker-Trigger). Der
            // ResumeFlightBanner (Frontend) zeigt sich via flight_status-
            // Polling und der Pilot muss aktiv bestaetigen.
            //
            // v0.13.10 (QS-Round-1 Fix): Heartbeat wird WAEHREND der Pause
            // weiterhin gesendet, sonst killt phpVMS' RemoveExpiredLiveFlights-
            // Cron den PIREP nach `acars.live_time` (default 2h) — auch
            // realistisch wenn der Pilot den Banner laengere Zeit ignoriert
            // (Telefon, Pinkelpause, Pause im Sim). Heartbeat-Body baut sich
            // aus dem letzten guten Pre-Recovery-Snapshot (last_good_snap
            // VOR diesem Tick, identisch zum SimPause/SimDisconnect-Pfad).
            if flight.was_just_resumed.load(Ordering::Relaxed) {
                if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                    if let Some(hb) = last_good_snap.as_ref() {
                        let (phase, body) = {
                            let stats = flight.stats.lock().expect("flight stats");
                            let p = stats.phase;
                            (p, build_heartbeat_body(hb, &stats, p))
                        };
                        // Über den Arbeiter — der Streamer wartet nicht
                        // auf phpVMS (siehe `spawn_phpvms_meldungen`).
                        last_heartbeat = std::time::Instant::now();
                        phpvms_meldungen.lebenszeichen(body, phase, false);
                    }
                }
                continue;
            }
            // Frisch lesen: Waehrend der Pause kann das Resume-Gate die Phase
            // umgestellt haben (Weiterflug vom Boden, GAF 278). Mit dem Wert von
            // vor dem Schlaf hielte die Absturz-Erkennung den Flug sofort
            // wieder an (Claude-Pruefer 24.10.2026).
            let current_phase = flight.stats.lock().expect("flight stats").phase;

            // v0.13.9 Stream F: Snapshot VOR Overwrite cachen damit die
            // Recovery-Detection (airborne -> on_ground mit massivem
            // Altitude-Drop) den Vergleich machen kann.
            let previous_snap_for_recovery: Option<SimSnapshot> = last_good_snap.clone();

            let snapshot = current_snapshot(&app);

            // v0.19.3: while a sim SESSION is loading — including a restart in
            // the middle of a flight — MSFS reports the aircraft at (0, 0), Null
            // Island, with a default airframe mass. Those snapshots are not
            // telemetry, they are a placeholder, and nothing may be derived from
            // them.
            //
            // Field evidence (Thomas, DLH367, 2026-07-12): he restarted the sim
            // mid-session, and the client — still running — read the loading
            // sim's placeholder and stamped a TAKEOFF at lat 0.008 / lon 0.00002
            // with a takeoff weight of 349 t. The real departure, an hour later,
            // was 300 t out of KCLT. The bogus takeoff went to the recorder and
            // into the flight record, and (0, 0) positions would have drawn his
            // track to the Gulf of Guinea on the live map.
            //
            // Dropped before ANYTHING sees them: FSM, stats, phpVMS, MQTT. The
            // existing stale-snapshot machinery then treats the gap exactly like
            // any other sim dropout, which is what a restarting sim is.
            let snapshot = match snapshot {
                // Ebenso die Menue-Scheinposition (LAT 0 / LON 90): Sie besteht
                // den Filter oben, ist aber kein Flugzustand. Als „keine Daten"
                // greift nach 30 s die Pause samt Lage-Pruefung (GAF 278).
                Some(s) if !snapshot_position_is_usable(&s) || !snapshot_plausibel(&s) => {
                    if !null_island_logged {
                        null_island_logged = true;
                        tracing::warn!(
                            lat = s.lat,
                            lon = s.lon,
                            "sim has not placed the aircraft (loading / restarting) — \
                             ignoring its telemetry until it does"
                        );
                    }
                    None
                }
                other => {
                    null_island_logged = false;
                    other
                }
            };

            // v0.20 (Process-Integrity): App-Neustart-Resume-Discontinuity-
            // Check — der eigentliche Vorfall-Pfad (Joel EWG3552). Anders als
            // die Mid-Session-Crash-Detection unten (die vergleicht Ticks
            // INNERHALB desselben laufenden Prozesses) geht es hier um einen
            // App-NEUSTART: `previous_snap_for_recovery` ist nur dann `None`,
            // wenn dieser Streamer-Lauf noch NIE einen echten Snapshot sah —
            // entweder ein brandneuer Flug (dann ist `last_persisted_snapshot`
            // ebenfalls `None`, kein Treffer) oder exakt der resumte Flug nach
            // `try_resume_flight()`. `take()` sorgt dafür, dass der Vergleich
            // nur EINMAL passiert, nicht auf jedem Tick.
            //
            // Log-Durchsicht 26.09.2026: die Menue-Scheinposition (LAT 0 /
            // LON 90) nicht vergleichen — `last_persisted_snapshot` bleibt
            // liegen, bis der erste echte Snapshot kommt.
            if previous_snap_for_recovery.is_none() {
                if let Some(curr) = snapshot.as_ref().filter(|s| snapshot_plausibel(s)) {
                    let maybe_prev = {
                        let mut stats = flight.stats.lock().expect("flight stats");
                        stats.last_persisted_snapshot.take()
                    };
                    if let Some(prev) = maybe_prev {
                        let (ziel, luecke_secs) = {
                            let stats = flight.stats.lock().expect("flight stats");
                            (stats.planned_arr_ref_pos, stats.resume_luecke_secs)
                        };
                        let discontinuity =
                            compute_resume_discontinuity(&prev, curr, ziel, luecke_secs);
                        if is_impossible_discontinuity(&discontinuity) {
                            {
                                let mut stats = flight.stats.lock().expect("flight stats");
                                stats.resume_discontinuity = Some(discontinuity);
                                stats.resume_spruenge.push(discontinuity);
                            }
                            log_activity_and_record(
                                &app,
                                &flight.pirep_id,
                                ActivityLevel::Error,
                                format!(
                                    "Unmöglicher Sprung beim Wiederaufnehmen erkannt ({})",
                                    sprung_grund(&discontinuity)
                                ),
                                Some(format!(
                                    "Fuel-Diff {:+.0} kg · Höhen-Diff {:+.0} ft · Drift {:.1} nm — \
                                     physikalisch nicht plausibel (kein normaler Verbrauch/Flug), \
                                     vermutlich Reload/neuer Ladezustand während eines App- oder \
                                     Sim-Neustarts.",
                                    discontinuity.fuel_delta_kg,
                                    discontinuity.altitude_delta_ft,
                                    discontinuity.drift_nm,
                                )),
                            );
                            tracing::warn!(
                                pirep_id = %flight.pirep_id,
                                fuel_delta_kg = discontinuity.fuel_delta_kg,
                                altitude_delta_ft = discontinuity.altitude_delta_ft,
                                drift_nm = discontinuity.drift_nm,
                                "impossible resume discontinuity detected"
                            );
                            save_active_flight(&app, &flight);
                        }
                    }
                }
            }

            if let Some(s) = snapshot.as_ref().filter(|s| snapshot_plausibel(s)) {
                last_good_snap = Some(s.clone());
                last_good_snap_at = Some(std::time::Instant::now());
            }

            // v0.13.9 Stream F (LE27): Mid-Session Sim-Crash-Detection.
            //
            // Symptom realer Pilot-Befunde (User Michel, 25.05.2026 ITY 608
            // KBOS->KJFK, 3x XP12-Reload heute beobachtet):
            //   - vor Reload: airborne in CRUISE, FL320-FL360
            //   - nach Reload: on_ground=true am naechstgelegenen Airport,
            //     Altitude springt -30k ft in einem Sample-Intervall
            //
            // Diese Kombination ist physikalisch unmoeglich (max real
            // achievable descent ~4000 fpm = 4000 ft pro Minute), also
            // eindeutig ein Sim-Reload-Artefakt.
            //
            // Wir setzen `was_just_resumed=true` — der bereits existierende
            // Stream-F-Workflow (Banner + Re-Check) greift dann automatisch
            // via flight_status-Polling. Naechster Tick laeuft in den Guard
            // oben hinein und pausiert den Streamer bis Pilot entschieden hat.
            //
            // Schwelle: 5000 ft Altitude-Drop in einem Tick UND on_ground
            // erst false dann true UND vorherige FlightPhase = airborne-Set.
            // Position-Distance ist NICHT geprueft weil XP12-Crash-Recovery
            // am NAECHSTEN Airport (typisch 10-20 km vom Cruise-Punkt) eine
            // Drift unter Schwellenwert haben kann (RESUME_DRIFT_WARN_NM=50nm).
            if let (Some(prev), Some(curr)) =
                (previous_snap_for_recovery.as_ref(), snapshot.as_ref())
            {
                if is_mid_session_sim_crash_recovery(
                    current_phase,
                    prev.on_ground,
                    prev.altitude_msl_ft,
                    am_boden_fuer_absturzerkennung(curr),
                    curr.altitude_msl_ft,
                ) {
                    let alt_drop_ft = (prev.altitude_msl_ft - curr.altitude_msl_ft).abs();
                    flight.was_just_resumed.store(true, Ordering::SeqCst);
                    // Die Wartezeit bis zur Entscheidung ist eine Pause wie
                    // bei der Sim-Trennung — sonst zaehlte sie als Flugzeit,
                    // und die Lage-Pruefung haette keinen Pausenstand
                    // (Codex-Abnahme 24.10.2026).
                    {
                        let mut st = flight.stats.lock().expect("flight stats");
                        if st.paused_since.is_none() {
                            st.paused_since = Some(Utc::now());
                            st.paused_last_known = Some(PausedSnapshot {
                                lat: prev.lat,
                                lon: prev.lon,
                                heading_deg: prev.heading_deg_true,
                                altitude_ft: prev.altitude_msl_ft,
                                fuel_total_kg: prev.fuel_total_kg,
                                zfw_kg: prev.zfw_kg,
                                on_ground: prev.on_ground,
                            });
                            st.current_pause_reason = Some(PauseReason::SimDisconnect);
                        }
                    }
                    // v0.13.11 (QS-Round-2 Fix): last_good_snap auf den Pre-
                    // Recovery-Snapshot zuruecksetzen damit der Pause-
                    // Heartbeat (Guard oben im naechsten Tick) phpVMS mit den
                    // echten airborne-Cruise-Werten beliefert statt dem
                    // falschen Ground/Reload-Snapshot, den der Adapter
                    // gerade geliefert hat.
                    last_good_snap = previous_snap_for_recovery.clone();
                    save_active_flight(&app, &flight);
                    log_activity_and_record(
                        &app,
                        &flight.pirep_id,
                        ActivityLevel::Error,
                        "Sim-Reload erkannt — Aufzeichnung pausiert".to_string(),
                        Some(format!(
                            "Vorher: FL{:.0} airborne · jetzt: {:.0} ft am Boden bei LAT {:.3}° LON {:.3}° (Drop: {:.0} ft). \
                            Bitte im ResumeFlightBanner waehlen: zurueck zur Cruise-Position positionieren + Position pruefen, \
                            oder trotzdem fortsetzen — der Sprung wird im Activity-Log dokumentiert.",
                            prev.altitude_msl_ft / 100.0,
                            curr.altitude_msl_ft,
                            curr.lat,
                            curr.lon,
                            alt_drop_ft,
                        )),
                    );
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        prev_alt_ft = prev.altitude_msl_ft,
                        curr_alt_ft = curr.altitude_msl_ft,
                        alt_drop_ft,
                        curr_on_ground = curr.on_ground,
                        prev_on_ground = prev.on_ground,
                        phase = ?current_phase,
                        "Stream F mid-session sim-crash-recovery detected — streamer paused"
                    );
                    // Skip restlichen Tick — naechster Tick faellt in den
                    // Guard oben hinein und pausiert bis Pilot bestaetigt.
                    continue;
                }
            }

            // Spec v0.7.15 F5/F6: Sim-eigene Pause-Detection.
            //
            // SimSnapshot.paused wird vom Adapter gesetzt:
            // - MSFS: aus dem SimConnect-`Paused`/`Unpaused`-Event-Atomic
            //   (siehe crates/sim-msfs/src/adapter.rs)
            // - X-Plane: aus dem Plugin `sim/time/paused` DataRef
            //   (siehe F6 unten, falls Plugin v0.7.15+)
            //
            // Wenn der Sim eigenstaendig pausiert und ACARS noch nicht im
            // Pause-State ist: paused_since setzen, last_known speichern,
            // Reason vermerken. Der bestehende `is_paused`-Block
            // (siehe unten) macht dann den Rest — Skip-Streamer-Tick +
            // Heartbeat-mit-last_good_snap.
            let snap_says_paused = snapshot.as_ref().map(|s| s.paused).unwrap_or(false);
            if snap_says_paused {
                let already_paused = {
                    let stats = flight.stats.lock().expect("flight stats");
                    stats.paused_since.is_some()
                };
                if !already_paused {
                    if let Some(ref snap) = snapshot {
                        // Referenz fuer die Sprungpruefung: der aktuelle Punkt,
                        // wenn er echt ist, sonst der letzte echte. Geht der
                        // Sim gerade ins Menue, ist der aktuelle Punkt schon
                        // die Scheinposition.
                        let referenz = Some(snap)
                            .filter(|s| snapshot_plausibel(s))
                            .or(last_good_snap.as_ref());
                        let paused = referenz.map(|r| PausedSnapshot {
                            lat: r.lat,
                            lon: r.lon,
                            heading_deg: r.heading_deg_true,
                            altitude_ft: r.altitude_indicated_ft.unwrap_or(r.altitude_msl_ft),
                            fuel_total_kg: r.fuel_total_kg,
                            zfw_kg: r.zfw_kg,
                            on_ground: r.on_ground,
                        });
                        let detail = paused.as_ref().map(|p| {
                            format!(
                                "Letzte bekannte Position: LAT {:.4}° · LON {:.4}° · ALT {:.0} ft",
                                p.lat, p.lon, p.altitude_ft,
                            )
                        });
                        {
                            let mut stats = flight.stats.lock().expect("flight stats");
                            stats.paused_since = Some(Utc::now());
                            stats.paused_last_known = paused;
                            stats.current_pause_reason = Some(PauseReason::SimPause);
                        }
                        log_activity_handle(
                            &app,
                            ActivityLevel::Info,
                            "Simulator pausiert — AeroACARS pausiert die Aufzeichnung.".to_string(),
                            detail,
                        );
                        save_active_flight(&app, &flight);
                        tracing::info!(
                            pirep_id = %flight.pirep_id,
                            "sim-pause detected via snapshot.paused flag"
                        );
                    }
                }
            }

            // Spec sim-disconnect-auto-resume F1 (Auto-Resume):
            //
            // Frueher (v0.4.1) war Pause ein strikter Block — der Streamer
            // wartete auf manuellen Klick selbst wenn der Sim wieder Daten
            // lieferte. Reale Folge im AUA-323-Incident: Pilot hat die
            // Pause erst nach der Landung gemerkt → 23 min Datenloch +
            // Server-Splitter macht zwei Sessions.
            //
            // Neu: wenn wir wieder einen Sim-Snapshot haben, wird der
            // Pause-State automatisch beendet (apply_pause_resume), die
            // Pause-Dauer in den Akkumulator akkumuliert (damit der
            // Flight-Time-Wert im PIREP korrekt bleibt) und der Streamer
            // verarbeitet den aktuellen Tick normal weiter.
            //
            // Heartbeat (= `POST /pireps/{id}/update`) laeuft auch waehrend
            // der Pause weiter — siehe Heartbeat-Code-Pfad unten. Damit
            // bleibt der phpVMS-PIREP aktiv egal wie lange der Sim weg ist.
            //
            // Manueller Resume-Button (`flight_resume_after_disconnect`)
            // bleibt als Fallback erhalten — z.B. wenn der Sim gar keine
            // Snapshots mehr liefert, der Pilot aber den PIREP weiter-
            // fuehren will.
            let is_paused = flight
                .stats
                .lock()
                .expect("flight stats")
                .paused_since
                .is_some();
            if is_paused {
                // Resume-Bedingung: Snapshot ist da UND Sim ist NICHT
                // mehr in eigener Pause (snap.paused=false). Sonst
                // bleibt der Streamer pausiert auch wenn Snapshots
                // kommen (F5: Frozen-Snapshots waehrend Esc-Pause).
                // Erst weiter, wenn der Sim wieder eine echte Position hat —
                // aus dem Ladebildschirm heraus kam sonst die Scheinposition
                // als erster Punkt „danach" (aKRgO8gxYzPnLR8B, 11:01:10).
                let can_resume = snapshot.as_ref().is_some_and(snapshot_plausibel)
                    && !snap_says_paused
                    && !aufsetzen_wird_geprueft(
                        &flight.stats.lock().expect("flight stats"),
                        Utc::now(),
                    );
                // Von selbst weiter nur, wenn die Lage zum Flug passt. Sonst
                // bleibt die Pause offen, und der Pilot bestaetigt im Banner
                // (GAF 278: Sim-Absturz im Sinkflug, danach am Boden in
                // Zagreb, Tank voll — der Client setzte fort und meldete
                // ARRIVED).
                let abweichung = if can_resume {
                    snapshot.as_ref().and_then(|s| lage_abweichung_fuer(&flight, s))
                } else {
                    None
                };
                if let (Some(abweichung), Some(jetzt)) = (abweichung, snapshot.as_ref()) {
                    flight.was_just_resumed.store(true, Ordering::SeqCst);
                    // Heartbeat und Banner zeigen den Stand VOR der Pause.
                    last_good_snap = previous_snap_for_recovery.clone();
                    save_active_flight(&app, &flight);
                    let (vorher_lat, vorher_lon) = {
                        let stats = flight.stats.lock().expect("flight stats");
                        gespeicherter_punkt(&stats).unwrap_or((jetzt.lat, jetzt.lon))
                    };
                    log_activity_and_record(
                        &app,
                        &flight.pirep_id,
                        ActivityLevel::Error,
                        "Simulator wieder da — Lage passt nicht zum Flug, Aufzeichnung pausiert"
                            .to_string(),
                        Some(format!(
                            "{}\nLetzte Position: LAT {:.4}° · LON {:.4}°\nJetzt im Sim: LAT {:.4}° · LON {:.4}° · ALT {:.0} ft\n\
                             Bitte im Banner bestätigen: zur letzten Position zurückstellen und „Position prüfen“, \
                             trotzdem fortsetzen (wird vermerkt) oder den Flug verwerfen.",
                            lage_abweichung_text(&abweichung),
                            vorher_lat,
                            vorher_lon,
                            jetzt.lat,
                            jetzt.lon,
                            jetzt.altitude_msl_ft,
                        )),
                    );
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        ?abweichung,
                        "sim back after pause — position does not fit, waiting for pilot"
                    );
                    continue;
                }
                if can_resume {
                    // Auto-Resume: Reason aus dem State (gesetzt vom
                    // Disconnect- bzw. Sim-Pause-Detect-Pfad). Helper
                    // updated State, schreibt Activity-Log, persistiert
                    // active_flight.json. Anschliessend faellt der Tick
                    // durch zum normalen Position-Stream-Pfad.
                    let resume_reason = {
                        let stats = flight.stats.lock().expect("flight stats");
                        stats
                            .current_pause_reason
                            .unwrap_or(PauseReason::SimDisconnect)
                    };
                    let _ = apply_pause_resume(&app, &flight, snapshot.as_ref(), resume_reason);
                    // fall-through zum naechsten Code-Pfad
                } else {
                    // Sim liefert keine Daten → weiter pausieren.
                    //
                    // QS-Finding HIGH (2026-05-12): Der ALTE Pfad hatte hier
                    // ein blankes `continue`, welches den Heartbeat-Code-Pfad
                    // weiter unten (Z. ~12143) komplett ueberspringt. Folge:
                    // phpVMS sieht nach dem letzten Pre-Pause-Heartbeat keine
                    // Updates mehr und der `RemoveExpiredLiveFlights`-Cron
                    // killt den PIREP nach ~6h. Die Spec-Aussage "Heartbeat
                    // haelt PIREP unbegrenzt am Leben" war damit nicht
                    // umgesetzt.
                    //
                    // Fix: bevor wir continue'n, einen Heartbeat senden wenn
                    // (a) wir je einen guten Snapshot hatten (sonst gibt's
                    // nichts zu senden) und (b) das HEARTBEAT_INTERVAL
                    // erreicht ist. Wir nutzen `last_good_snap` als
                    // Position-/Phase-Quelle — der Heartbeat-Body braucht
                    // einen Snapshot fuer level/fuel/etc. Werte sind
                    // eingefroren auf dem Pre-Pause-Stand, was korrekt ist:
                    // der Flieger steht "weiterhin" dort, bis ein Sim-
                    // Snapshot beweist dass er weiter geflogen ist.
                    // Heartbeat-Quelle: bei SimPause haben wir den
                    // aktuellen (eingefrorenen) Snapshot, bei
                    // SimDisconnect nur den letzten guten Pre-Pause-
                    // Snapshot. Beide funktionieren — der Heartbeat-
                    // Body enthaelt eingefrorene Position/Alt/Fuel-
                    // Werte was korrekt ist: der Flieger steht zu
                    // dem Zeitpunkt eingefroren.
                    let hb_snap = snapshot.as_ref().or(last_good_snap.as_ref());
                    if let Some(hb) = hb_snap {
                        if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                            let (phase, body) = {
                                let stats = flight.stats.lock().expect("flight stats");
                                let p = stats.phase;
                                (p, build_heartbeat_body(hb, &stats, p))
                            };
                            // Über den Arbeiter — der Streamer wartet nicht
                            // auf phpVMS (siehe `spawn_phpvms_meldungen`).
                            last_heartbeat = std::time::Instant::now();
                            phpvms_meldungen.lebenszeichen(body, phase, false);
                        }
                    }
                    // Gewolltes Warten in der Pause ist kein langer Takt.
                    takt_beginn = None;
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            }

            // Sim-Disconnect-Detection: hatten irgendwann mal Daten, aber
            // der letzte gute Snapshot ist > THRESHOLD her → Pause auslösen.
            let disconnect_detected = match (last_good_snap_at, snapshot.is_some()) {
                (Some(t), false) => {
                    t.elapsed() > Duration::from_secs(SIM_DISCONNECT_THRESHOLD_S as u64)
                }
                _ => false,
            };
            if disconnect_detected {
                if let Some(ref last) = last_good_snap {
                    let paused = PausedSnapshot {
                        lat: last.lat,
                        lon: last.lon,
                        heading_deg: last.heading_deg_true,
                        altitude_ft: last.altitude_msl_ft,
                        fuel_total_kg: last.fuel_total_kg,
                        zfw_kg: last.zfw_kg,
                        on_ground: last.on_ground,
                    };
                    // v0.20 (Process-Integrity): nur EINMAL pro Pause-Übergang
                    // probieren (nicht jeden Tick, während schon pausiert ist —
                    // sysinfo-Scans sind nicht gratis, und der Zustand ist eh
                    // eingefroren). Passiert VOR dem Lock unten, damit der Scan
                    // selbst den Stats-Mutex nicht blockiert.
                    let already_paused = flight
                        .stats
                        .lock()
                        .expect("flight stats")
                        .paused_since
                        .is_some();
                    let liveness = if already_paused {
                        None
                    } else {
                        let kind = read_sim_config(&app).kind;
                        Some(sim_core::process_probe::sim_process_alive(kind))
                    };
                    {
                        let mut stats = flight.stats.lock().expect("flight stats");
                        if stats.paused_since.is_none() {
                            stats.paused_since = Some(Utc::now());
                            stats.paused_last_known = Some(paused.clone());
                            // Spec v0.7.15 F5: Reason setzen damit der
                            // Resume-Helper weiss welcher Trigger es war.
                            stats.current_pause_reason = Some(PauseReason::SimDisconnect);
                            stats.disconnect_sim_liveness = liveness;
                        }
                    }
                    let liveness_line = match liveness {
                        Some(sim_core::process_probe::ProcessLiveness::Gone) => {
                            "\nSim-Prozess: NICHT MEHR gefunden (vermutlich beendet/abgestürzt)."
                        }
                        Some(sim_core::process_probe::ProcessLiveness::Alive) => {
                            "\nSim-Prozess läuft noch — nur die Datenverbindung ist weg."
                        }
                        Some(sim_core::process_probe::ProcessLiveness::Unknown) | None => "",
                    };
                    let detail = format!(
                        "Letzte bekannte Position: LAT {:.4}° · LON {:.4}° · HDG {:.0}° · ALT {:.0} ft · Fuel {:.0} kg · ZFW {}{}",
                        paused.lat,
                        paused.lon,
                        paused.heading_deg,
                        paused.altitude_ft,
                        paused.fuel_total_kg,
                        paused
                            .zfw_kg
                            .map(|v| format!("{:.0} kg", v))
                            .unwrap_or_else(|| "—".to_string()),
                        liveness_line,
                    );
                    log_activity_and_record(
                        &app,
                        &flight.pirep_id,
                        ActivityLevel::Warn,
                        "Sim getrennt — Flug pausiert. Klicke „Flug wiederaufnehmen\" sobald du repositioniert hast.".to_string(),
                        Some(detail),
                    );
                    save_active_flight(&app, &flight);
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        "sim disconnect detected, flight paused"
                    );
                }
                continue;
            }

            // Wenn weder pausiert noch frischer Snapshot da, einfach weiter
            // warten — bis zum Threshold poll'n wir geduldig.
            let Some(mut snap) = snapshot else {
                tracing::debug!(
                    pirep_id = %flight.pirep_id,
                    "no sim snapshot yet — waiting (will pause after {}s)",
                    SIM_DISCONNECT_THRESHOLD_S
                );
                continue;
            };

            // v0.13.18: Fuel-Flow-Fallback aus dem FOB-Verlauf (siehe loop-
            // lokale ff_*-Deklaration oben). Direkt nach dem snap-Binding, damit
            // ALLE nachgelagerten Verbraucher (phpVMS-Position-Payload, MQTT-
            // Live-Map) den abgeleiteten Wert sehen.
            if snap.fuel_flow_kg_per_h.is_none() {
                let now_i = std::time::Instant::now();
                match (ff_last_fob_kg, ff_last_fob_at) {
                    (Some(prev_fob), Some(prev_at)) => {
                        let dt_secs = now_i.duration_since(prev_at).as_secs_f32();
                        // Nur über ein volles Fenster neu rechnen (None = noch
                        // zu kurz → letzten abgeleiteten Wert weiter halten).
                        if let Some(rate) =
                            derive_fuel_flow_kg_per_h(prev_fob, snap.fuel_total_kg, dt_secs)
                        {
                            ff_derived_kg_per_h = Some(rate);
                            ff_last_fob_kg = Some(snap.fuel_total_kg);
                            ff_last_fob_at = Some(now_i);
                        }
                    }
                    _ => {
                        // Erste Baseline setzen (noch kein abgeleiteter Wert).
                        ff_last_fob_kg = Some(snap.fuel_total_kg);
                        ff_last_fob_at = Some(now_i);
                    }
                }
                snap.fuel_flow_kg_per_h = ff_derived_kg_per_h;
            } else {
                // Direkte SimVar liefert → Baseline frischhalten, NICHT ableiten.
                ff_last_fob_kg = Some(snap.fuel_total_kg);
                ff_last_fob_at = Some(std::time::Instant::now());
                ff_derived_kg_per_h = None;
            }

            // v0.16.10: Fuel-Used-Fallback aus dem FOB-Delta. PMDG liefert
            // FUEL TOTAL QUANTITY USED nicht (Live-Befund GLO4587
            // 2026-06-11: Cruise zeigte fuel_used_kg = 0.0 während der FOB
            // korrekt runterlief) — tatsächlich stempeln BEIDE Adapter das
            // Snapshot-Feld hart auf 0.0. Gleiche Stelle wie der Fuel-Flow-
            // Fallback oben, damit ALLE nachgelagerten Verbraucher (JSONL-
            // Flight-Log Position-Events, Forensik-Analysen) den
            // abgeleiteten Wert sehen. Die FOB-Baseline lebt PERSISTIERT in
            // den FlightStats (Resume-sicher — Fuel-Used zählt fürs PIREP-
            // Umfeld). Der PIREP-`fuel_used`-Payload selbst rechnet
            // unabhängig davon weiter block_fuel_kg − last_fuel_kg (Peak-
            // FOB-Delta, war nie von der toten SimVar abhängig).
            {
                let mut stats = flight.stats.lock().expect("flight stats");
                // v0.16.10 QS (M1): pre_takeoff + Vor-Tick-FOB für den
                // Gate-Defuel-Re-Latch. `takeoff_at` latcht beim ersten
                // →Takeoff-Übergang (inkl. Rescue-Latch-Backfill, siehe
                // latch_takeoff_stats) und bleibt danach Some — Defuel-
                // Re-Latch ist also exakt bis zum ersten Airborne aktiv.
                // `last_fuel_kg` wird erst weiter unten in
                // update_running_stats (via step_flight) auf den
                // aktuellen Tick gestempelt — hier lesen wir noch den
                // FOB des VOR-Ticks.
                let pre_takeoff = stats.takeoff_at.is_none();
                let prev_fob_kg = stats.last_fuel_kg;
                snap.fuel_used_kg = fuel_used_effective_kg(
                    &mut stats.initial_fob_kg,
                    snap.fuel_used_kg,
                    snap.fuel_total_kg,
                    pre_takeoff,
                    prev_fob_kg,
                );
            }

            // Snapshot the current phase BEFORE stepping so we can pass
            // the from→to pair to the recorder when it changes.
            // Plus snapshot Takeoff/Block-flags fuer MQTT-Snapshot-Publish
            // beim None→Some-Übergang nach step_flight.
            // v0.7.14: prev_landing_at war frueher fuer Discord-Webhook-Post
            // — Discord macht jetzt der Recorder zentral. Wir tracken nur noch
            // Takeoff + Block fuer die MQTT-Snapshot-Publishes (weiter unten).
            let (prev_phase, prev_block_off_at, prev_takeoff_at) = {
                let stats = flight.stats.lock().expect("flight stats");
                (stats.phase, stats.block_off_at, stats.takeoff_at)
            };
            // Update running stats AND step the flight-phase FSM.
            // v0.5.25: arr_airport_elevation_ft fuer HAT-basiertes
            // Stable-Approach-Gate. Lazy lookup beim ersten Tick wo wir
            // die Elevation noch nicht haben — phpVMS-API-Cache aus
            // state.airports liefert sie sobald airport_get aufgerufen
            // wurde (passiert beim Bid-Pickup ueblicherweise schon).
            // Ohne Elevation: Fallback auf AGL-Filter, used_hat=false.
            tick_count = tick_count.wrapping_add(1);
            {
                let need_elev = {
                    let stats_g = flight.stats.lock().expect("flight stats");
                    stats_g.arr_airport_elevation_ft.is_none()
                };
                if need_elev {
                    let elev = {
                        let app_state = app.state::<AppState>();
                        let airports = app_state.airports.lock().expect("airports lock");
                        airports
                            .get(&flight.arr_airport.to_uppercase())
                            .and_then(|a| a.elevation)
                    };
                    if let Some(elev) = elev {
                        let mut stats_g = flight.stats.lock().expect("flight stats");
                        stats_g.arr_airport_elevation_ft = Some(elev as f32);
                        tracing::info!(
                            arr = %flight.arr_airport,
                            elevation_ft = elev,
                            "arr airport elevation cached for HAT-based Stable-Approach-Gate"
                        );
                    }
                }

                // v0.19.3: WHERE IS THE ARRIVAL AIRPORT — the question the whole
                // divert geometry rests on. Asked of all three sources, best
                // first (see `airport_reference_pos`): Navigraph's active AIRAC
                // cycle, then phpVMS, then the embedded OurAirports table.
                //
                // Re-resolved on every tick until Navigraph answers, because the
                // navdata fetch is asynchronous and lands a few seconds into the
                // flight: a first answer from a weaker source must not lock us
                // out of the better one that is still on its way.
                //
                // The locks are taken one at a time, never nested — `step_flight`
                // holds `stats` while it runs, so taking `navdata` underneath it
                // would invert the order this block establishes.
                let have_navigraph = {
                    let stats_g = flight.stats.lock().expect("flight stats");
                    stats_g.planned_arr_ref_source == ARR_REF_SOURCE_NAVIGRAPH
                };
                if !have_navigraph {
                    let nav_pos = {
                        let cache = flight.navdata.lock().expect("navdata lock");
                        cache.reference_pos(&flight.arr_airport)
                    };
                    let phpvms_pos = {
                        let app_state = app.state::<AppState>();
                        let airports = app_state.airports.lock().expect("airports lock");
                        airports
                            .get(&flight.arr_airport.to_uppercase())
                            .and_then(|a| a.lat.zip(a.lon))
                    };
                    let (pos, source) =
                        airport_reference_pos(&flight.arr_airport, nav_pos, phpvms_pos);
                    if let Some(pos) = pos {
                        let mut stats_g = flight.stats.lock().expect("flight stats");
                        if source > stats_g.planned_arr_ref_source {
                            stats_g.planned_arr_ref_pos = Some(pos);
                            stats_g.planned_arr_ref_source = source;
                            tracing::info!(
                                arr = %flight.arr_airport,
                                lat = pos.0,
                                lon = pos.1,
                                source = match source {
                                    ARR_REF_SOURCE_NAVIGRAPH => "navigraph",
                                    ARR_REF_SOURCE_PHPVMS => "phpvms",
                                    _ => "ourairports",
                                },
                                "arrival airport reference position resolved"
                            );
                        }
                    }
                }
            }
            // v0.19.3: the block above reads the phpVMS airport CACHE only, and
            // that cache is filled by the UI (usually at bid pickup). In a
            // session where the frontend never fetched the arrival airport it
            // stays empty — and then the divert-geometry fallback silently does
            // nothing, which would make the detection work or not work depending
            // on what the pilot happened to click. Fetch it once, ourselves.
            {
                /// A transient error must not cost the whole flight; an outage
                /// must not cost a request per tick.
                const ARR_REF_POS_MAX_ATTEMPTS: u8 = 5;
                /// Ticks between attempts. Without spacing, all five attempts
                /// burn in the first seconds of the flight — the moment most
                /// likely to hit a cold connection — and a single blip at the
                /// gate would then disable the arrival geometry for the whole
                /// leg, which is exactly the failure this fetch exists to
                /// prevent. ~30 ticks is minutes, and the answer is not needed
                /// until the aircraft is on the ground again.
                const ARR_REF_POS_RETRY_EVERY_TICKS: u32 = 30;

                // QS round 8: this used to ask `planned_arr_ref_pos.is_none()` —
                // which is never true, because the cascade falls through to the
                // embedded OurAirports table and that answers for essentially
                // every ICAO. The whole phpVMS self-fetch was therefore dead
                // code, and `ARR_REF_SOURCE_PHPVMS` was only ever reachable when
                // the UI happened to have cached the airport: exactly the "works
                // or not depending on what the pilot clicked" behaviour this
                // block was written to end.
                //
                // Ask the real question: do we have anything BETTER than the
                // offline table yet?
                let missing = {
                    let stats_g = flight.stats.lock().expect("flight stats");
                    stats_g.planned_arr_ref_source < ARR_REF_SOURCE_PHPVMS
                };
                let due = arr_ref_pos_attempts == 0
                    || tick_count.is_multiple_of(ARR_REF_POS_RETRY_EVERY_TICKS);
                if missing
                    && due
                    && arr_ref_pos_attempts < ARR_REF_POS_MAX_ATTEMPTS
                    && !arr_ref_aufgeben.load(Ordering::SeqCst)
                    && !arr_ref_unterwegs.swap(true, Ordering::SeqCst)
                {
                    arr_ref_pos_attempts += 1;
                    let versuch = arr_ref_pos_attempts;
                    let key = flight.arr_airport.to_uppercase();
                    // The streamer's OWN client — reading it back out of AppState
                    // introduced a hole where a `None` there burned the attempt
                    // without a request ever being made.
                    let (app, flight, client) = (app.clone(), flight.clone(), client.clone());
                    let (unterwegs, aufgeben) = (arr_ref_unterwegs.clone(), arr_ref_aufgeben.clone());
                    tauri::async_runtime::spawn(async move {
                        match client.get_airport(&key).await {
                            Ok(airport) => {
                                {
                                    let app_state = app.state::<AppState>();
                                    let mut guard = app_state.airports.lock().expect("airports lock");
                                    guard.insert(key.clone(), airport.clone());
                                }
                                if let Some(pos) = airport.lat.zip(airport.lon).filter(|(la, lo)| {
                                    !(*la == 0.0 && *lo == 0.0) && la.is_finite() && lo.is_finite()
                                }) {
                                    let mut stats_g = flight.stats.lock().expect("flight stats");
                                    stats_g.planned_arr_ref_pos = Some(pos);
                                    // v0.19.3: record WHICH source this is. Without it
                                    // the position kept `ARR_REF_SOURCE_NONE`, and the
                                    // next tick's cascade — seeing OurAirports (rank 1)
                                    // beat rank 0 — would overwrite this phpVMS position
                                    // with a weaker one.
                                    stats_g.planned_arr_ref_source = ARR_REF_SOURCE_PHPVMS;
                                    tracing::info!(
                                        arr = %key,
                                        "arr airport reference position fetched (divert-geometry fallback)"
                                    );
                                } else {
                                    // phpVMS has the airport but no coordinates —
                                    // retrying cannot change that.
                                    aufgeben.store(true, Ordering::SeqCst);
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    arr = %key,
                                    attempt = versuch,
                                    error = %e,
                                    "could not fetch arr airport reference position — will retry"
                                );
                            }
                        }
                        unterwegs.store(false, Ordering::SeqCst);
                    });
                }
            }
            // v1.7.8 — die Szenerie-Auskunft von MSFS abholen, sobald sie
            // vollstaendig ist, und am Flug ablegen.
            //
            // Hier, weil dieser Faden beides hat: den App-Griff (und
            // damit den Adapter) und den Flug. Die Bahnauflösung beim
            // Aufsetzen liest sie dann aus `stats` — sie kann den
            // Adapter nicht selbst befragen, und ein globaler Griff
            // waere ein Umweg um genau diese Trennung.
            //
            // Billig: ein Mutex und ein Vergleich je Tick, und nur
            // solange noch nichts abgelegt ist.
            takt_uhr.abschnitt("vorlauf");
            szenerie_auskunft_uebernehmen(&app, &flight, snap.simulator);
            takt_uhr.abschnitt("szenerie_ziel");
            // v1.7.17 Runde 6 (externe Gegenpruefung, Codex, adversarial,
            // 04.09.2026): die Phase VOR `step_flight` festhalten — damit
            // der Boarding-Ende-Riegel weiter unten den ECHTEN Uebergang
            // erkennt (alte Phase war Boarding, neue ist es nicht mehr),
            // statt bei JEDEM spaeteren Tick auszuloesen, in dem die Phase
            // zufaellig nicht Boarding ist (siehe dortige Funktionsdoku).
            let phase_vor_step_flight = flight
                .stats
                .lock()
                .map(|s| s.phase)
                .unwrap_or(FlightPhase::Boarding);
            // v1.7.17 — dasselbe fuer den ABFLUGplatz, eigenstaendig und
            // unabhaengig (siehe `dep_szenerie_auskunft_uebernehmen`).
            dep_szenerie_auskunft_uebernehmen(&app, &flight, snap.simulator);
            if szenerie_bahn::quelle_fuer(snap.simulator) == szenerie_bahn::Quelle::XPlaneDatei {
                szenerie_bahn::vorladen(&flight.arr_airport);
                landeplatz_szenerie_vorladen(&flight.arr_airport, &snap);
            }
            takt_uhr.abschnitt("szenerie_abflug");

            // ⚠ Und die Bahnaufloesung NACHHOLEN, wenn die Szenerie erst
            // nach dem Aufsetzen eintraf — siehe
            // `bahn_am_aufsetzpunkt_nachholen`.
            //
            // ⚠⚠ EIN Lock fuer Pruefung UND Ausfuehrung.
            //
            // Der erste Entwurf pruefte in einem Lock und rief in einem
            // zweiten. Dazwischen laeuft der Touchdown-Sampler, der beim
            // Durchstarten `landing_lat`/`landing_lon` loescht — der
            // Aufruf fiel dann auf die aktuelle Lage zurueck (Roll- oder
            // Steigposition, genau das, was diese Fassung ausschliessen
            // soll) und konnte einen Bericht setzen, der weitere
            // Versuche sperrt (QS-Befund 3, erste Runde v1.7.15).
            //
            // Hier, direkt nach der Ernte: die einzige Stelle, an der
            // eine neue Auskunft am Flug ankommt. Billig — die Funktion
            // steigt ohne den vollstaendigen Aufsetzpunkt sofort aus.
            let mut nachgeholt = false;
            if let Ok(mut st) = flight.stats.lock() {
                if bahn_am_aufsetzpunkt_nachholen(&mut st, &flight) {
                    tracing::info!(
                        icao = ?st.runway_correlation_icao,
                        stand = st.korrelierter_szenerie_stand,
                        revision = st.bahn_revision,
                        "Szenerie kam nach dem Aufsetzen — Bahnaufloesung nachgeholt"
                    );
                    nachgeholt = true;
                }
            }
            // ⚠ Sichern, sobald die Revision WAECHST — nicht erst vor dem
            // Versand.
            //
            // Der Nachtrag geht erst raus, wenn die Rollspur steht. Bis
            // dahin kann die Revision mehrfach wachsen, ohne dass ein
            // Ereignis sie mitnimmt. Ein Absturz in diesem Fenster liesse
            // die Platte weit hinter der Leitung zurueck, und der
            // fortgesetzte Flug kaeme nie wieder an dem Riegel vorbei,
            // den der Recorder aus einem frueheren Ereignis kennt.
            //
            // Ausserhalb der Sperre: `save_active_flight` nimmt sie
            // selbst.
            if nachgeholt {
                revision_vor_versand_sichern(&app, &flight);
            }

            takt_uhr.abschnitt("bahn_nachholen");
            let takt = phasen_takt(&flight, &snap, Utc::now());
            let phase_change = takt.wechsel;
            takt_uhr.abschnitt("step_flight");
            // Kabinenansagen (Beta): hier und nicht in `phasen_takt`, das
            // laeuft auch im Korpus-Replay.
            kabinenansagen::takt(&app, &flight, &client, &snap, Utc::now());
            takt_uhr.abschnitt("kabinenansagen");

            // Die Ausfahrten zur Bahn rechnen, solange die Sperre der
            // Flugdaten FREI ist, in einem eigenen Faden — siehe
            // `ausfahrten_im_hintergrund`. Danach finden Aufsetz-Ereignis,
            // Rollout-Nachtrag und `landing_get_current` sie im Merker.
            ausfahrten_im_hintergrund(&flight, &mut ausfahrten_ausloeser, &mut ausfahrten_laeufer);
            takt_uhr.abschnitt("ausfahrten_vorwaermen");

            // v1.5.1 (F2): METAR-Trigger einmal pro Tick mit der AKTUELLEN
            // Phase — nicht nur bei Übergängen. Boarding ist die Startphase
            // (kein Übergang dorthin), und ein Resume mitten im Sinkflug
            // hat den Descent-Übergang verpasst. Die requested-Flags machen
            // den Aufruf idempotent; nach dem ersten Treffer ist er ein
            // sofortiges return.
            {
                let tick_phase = {
                    let stats = flight.stats.lock().expect("flight stats");
                    stats.phase
                };
                maybe_spawn_metar_fetch(&app, &flight, tick_phase);
                // v1.6: gleiche Mechanik für die OSM-Parkpositionen —
                // dep-Stand am Gate erfassen, arr-Stände für das
                // BlocksOn-Stand-Gate vorladen.
                maybe_spawn_stand_fetch(&app, &flight, tick_phase, tick_count);
                // Abflug-Stand nachziehen: der Erstversuch im Lade-Task
                // kann an einer noch fehlenden Position scheitern — die
                // Liste liegt dann in dep_stands, und hier greift jeder
                // weitere Tick, bis der Stand sitzt oder das Rollen beginnt.
                // Nur im Boarding und im (Fast-)Stillstand: waehrend des
                // Pushbacks steht der Flieger schon NEBEN fremden Staenden
                // (30-80 m vom eigenen), und ein einmal gestempelter
                // dep_gate wird nie korrigiert (QS-Befund).
                if tick_phase == FlightPhase::Boarding && snap.groundspeed_kt <= 2.5 {
                    let mut stats = flight.stats.lock().expect("flight stats");
                    // v1.7.17 Runde 3 (externe Gegenpruefung, Codex,
                    // adversarial, 04.09.2026): HIER, mit der FRISCHEN
                    // (nach-`step_flight`-)Phase, entscheiden, ob wegen
                    // eines Verbindungswechsels ein Ableitungsversuch
                    // lohnt — nicht in der Abflug-Ernte weiter oben, die
                    // noch die Phase des VORHERIGEN Ticks sieht.
                    //
                    // v1.7.17 Runde 4 (externe Gegenpruefung, Codex,
                    // adversarial, 04.09.2026): `dep_gate` wird NICHT MEHR
                    // vorab geloescht — nur noch ATOMAR ersetzt, wenn ein
                    // echter Kandidat gefunden wird. Stand faehig die neue
                    // Szenerie/OSM in diesem Tick noch keinen Kandidaten,
                    // bleibt der alte (moeglicherweise leicht veraltete)
                    // Wert stehen, statt ersatzlos zu verschwinden, falls
                    // der Flieger vor einem erfolgreichen Tick zurueckrollt.
                    if dep_gate_braucht_neuableitung(
                        tick_phase,
                        stats.dep_gate.is_some(),
                        stats.dep_gate_generation,
                        stats.dep_szenerie_auskunft_generation,
                    ) {
                        let position = aircraft_position_for_gates(&stats);
                        let combined = standliste_fuer(
                            &stats,
                            &flight.dpt_airport,
                            stats.dep_stands.as_deref(),
                            position,
                        );
                        if let (Some(list), Some((lat, lon))) = (combined.as_deref(), position) {
                            if let Some(name) = stands::benannter_stand_bei(list, lat, lon)
                                .and_then(|(s, _)| s.name.clone())
                            {
                                tracing::info!(stand = %name, "departure stand captured on retry tick");
                                stats.dep_gate = Some(name);
                                stats.dep_gate_generation = stats.dep_szenerie_auskunft_generation;
                                stats.dep_gate_field_posted = false;
                            }
                        }
                    }
                }
                // v1.7.17 Runde 5 (externe Gegenpruefung, Codex, adversarial,
                // 04.09.2026): letzte Chance BEIM VERLASSEN des Boardings —
                // siehe `dep_gate_muss_beim_boarding_ende_verworfen_werden`.
                // Lief das Boarding zu Ende, ohne dass ein aus der neuen
                // Generation abgeleiteter Kandidat je gefunden wurde, wird
                // der bekanntlich veraltete Wert hier verworfen statt
                // unkorrigiert an Live-Post/PIREP/Debrief weitergegeben zu
                // werden. Kein Neuversuch (der bliebe verboten) — nur
                // Ehrlichkeit statt eines bekannt falschen Werts.
                //
                // v1.7.17 Runde 6 (externe Gegenpruefung, Codex, adversarial,
                // 04.09.2026): NUR im exakten Uebergangs-Tick (alte Phase
                // war Boarding, neue ist es nicht mehr) — nicht bei jedem
                // spaeteren Tick, in dem die Phase zufaellig nicht Boarding
                // ist. Sonst haette ein voellig unrelated Reconnect Stunden
                // spaeter (Cruise, Approach) einen laengst historischen,
                // korrekten Abflug-Stand geloescht.
                else if let Ok(mut stats) = flight.stats.lock() {
                    let boarding_wird_verlassen =
                        boarding_wird_in_diesem_tick_verlassen(phase_vor_step_flight, tick_phase);
                    if dep_gate_muss_beim_boarding_ende_verworfen_werden(
                        boarding_wird_verlassen,
                        stats.dep_gate.is_some(),
                        stats.dep_gate_generation,
                        stats.dep_szenerie_auskunft_generation,
                    ) {
                        // v1.7.17 Runde 6 (externe Gegenpruefung, Codex,
                        // adversarial, 04.09.2026): war der jetzt verworfene
                        // Wert schon LIVE an phpVMS gepostet, reicht das
                        // lokale Loeschen nicht — `post_pirep_fields` ist ein
                        // Upsert, und ein WEGGELASSENES Feld ueberschreibt
                        // nichts. Ohne ausdruecklichen Rueckzug bliebe der
                        // bekanntlich veraltete Stand auf dem Server stehen,
                        // durch die gesamte spaetere PIREP-Einreichung
                        // hindurch — genau das, was dieser ganze Riegel
                        // verhindern soll.
                        // v1.7.17 Runde 14 (externe Gegenpruefung, Codex,
                        // adversarial, 04.09.2026): `dep_gate_field_post_
                        // confirmed` statt `dep_gate_field_posted` — Letzteres
                        // wird schon VOR der Antwort gesetzt und nie
                        // zurueckgesetzt; ein fehlgeschlagener Versuch haette
                        // sonst faelschlich als Beleg gegolten, dass AeroACARS
                        // wirklich etwas gepostet hat, und einen moeglichen
                        // fremden Server-Wert autorisiert geloescht.
                        let muss_zurueckgezogen_werden = stats.dep_gate_field_post_confirmed;
                        stats.dep_gate = None;
                        if muss_zurueckgezogen_werden {
                            // v1.7.17 Runde 13 (externe Gegenpruefung,
                            // Codex, adversarial, 04.09.2026): festhalten,
                            // dass AeroACARS selbst einen jetzt veralteten
                            // Wert gepostet hat — NUR dann darf die finale
                            // Einreichung spaeter das Feld explizit leeren
                            // (siehe `build_pirep_fields` und die Doku am
                            // Feld selbst).
                            stats.dep_gate_withdrawal_pending = true;
                            // v1.7.17 Runde 14: SOFORT checkpointen, bevor
                            // die Anfrage ueberhaupt losgeschickt wird — der
                            // naechste periodische Schreibzugriff koennte
                            // noch Sekunden bis Minuten entfernt sein, und
                            // ein Absturz/Neustart in diesem Fenster haette
                            // das gerade gesetzte Flag sonst verloren.
                            drop(stats);
                            // v1.7.17 Runde 16 (externe Gegenpruefung, Codex,
                            // adversarial, 04.09.2026): `flight.stop` erneut
                            // pruefen, UNMITTELBAR vor dem Schreiben — sonst
                            // koennte ein zwischenzeitliches Filing/Cancel/
                            // Forget die Ablage schon geloescht haben, und
                            // dieser Schreibzugriff wuerde sie unbeabsichtigt
                            // wiederbeleben (derselbe Schutz wie im
                            // periodischen Checkpoint weiter unten).
                            if !flight.stop.load(std::sync::atomic::Ordering::Relaxed) {
                                save_active_flight(&app, &flight);
                            }
                            let mut posts = HashMap::new();
                            posts.insert("Departure Gate".to_string(), String::new());
                            let client = client.clone();
                            let pirep_id = flight.pirep_id.clone();
                            // v1.7.17 Runde 7: mit kurzem Retry statt eines
                            // einzelnen Fire-and-Forget-Versuchs — siehe
                            // `post_pirep_fields_mit_kurzem_retry`.
                            tauri::async_runtime::spawn(async move {
                                post_pirep_fields_mit_kurzem_retry(&client, &pirep_id, &posts)
                                    .await;
                            });
                        } else {
                            // v1.7.17 Runde 16 (externe Gegenpruefung, Codex,
                            // adversarial, 04.09.2026): die Bestaetigung
                            // steht noch aus (Post noch unterwegs ODER nie
                            // gepostet) — die Entscheidung "hier war eine
                            // Rueckzugs-Pflicht" trotzdem EINFRIEREN, statt
                            // sie zu verwerfen. Der Erfolgs-Callback des
                            // Live-Posts fuehrt sie spaeter aus, OHNE die
                            // Bedingung neu zu bewerten (siehe Funktionsdoku
                            // am Feld) — das vermeidet sowohl das
                            // Runde-15-Problem (dep_gate ist hier bereits
                            // `None`, eine Neubewertung faende nie etwas)
                            // als auch eine Fehlklassifizierung durch einen
                            // spaeteren, unrelated Reconnect.
                            stats.dep_gate_withdrawal_awaiting_confirmation = true;
                            drop(stats);
                            if !flight.stop.load(std::sync::atomic::Ordering::Relaxed) {
                                save_active_flight(&app, &flight);
                            }
                        }
                    }
                }
                // Ankunfts-Gegenstueck (QS-Befund B5): kommt die Standliste
                // erst NACH der TaxiIn→BlocksOn-Kante an (Fetch-Latenz,
                // spaeter Retry), war die einzige Zuordnungsstelle schon
                // vorbei. Solange der Flieger abgestellt steht, traegt
                // jeder Tick den Namen nach. Falscher Platz ist dabei
                // ausgeschlossen: die Liste selbst gehoert zu einem ICAO,
                // und ein Stand in 60-m-Naehe IST der Ortsbeweis.
                if matches!(tick_phase, FlightPhase::BlocksOn | FlightPhase::Arrived)
                    && snap.on_ground
                    && snap.groundspeed_kt < 1.0
                {
                    let mut stats = flight.stats.lock().expect("flight stats");
                    if stats.arr_gate.is_none() {
                        // Siehe `arrival_retry_ziel`: ICAO-Eigentuemerschaft
                        // und OSM-Erfolg sind zwei verschiedene Fragen, ohne
                        // diese Trennung griff der Tick nie, wenn OSM
                        // scheiterte (externe Gegenpruefung, Codex,
                        // adversarial, 04.09.2026).
                        let fund = match arrival_retry_ziel(&stats) {
                            Some((list_icao, osm)) => {
                                let combined = standliste_fuer(
                                    &stats,
                                    &list_icao,
                                    osm,
                                    Some((snap.lat, snap.lon)),
                                );
                                combined
                                    .as_deref()
                                    .and_then(|list| {
                                        stands::benannter_stand_bei(list, snap.lat, snap.lon)
                                    })
                                    .and_then(|(st, _)| st.name.clone())
                                    .map(|n| (n, list_icao))
                            }
                            None => None,
                        };
                        if let Some((name, list_icao)) = fund {
                            tracing::info!(stand = %name, at = %list_icao, "arrival stand captured on retry tick");
                            stats.arr_gate = Some(name);
                            stats.arr_gate_icao = Some(list_icao);
                        }
                    }
                }
                // v1.5.5: erkannte Staende LIVE als PIREP-Custom-Fields an
                // phpVMS — das GSG-Dashboard zeigt den Stand damit schon im
                // Boarding (der Weg des alten vmsACARS), nicht erst nach dem
                // Filing. Latches machen jeden Post einmalig; der BlocksOn-
                // Rueckweg re-armiert den arr-Latch fuer Korrekturen.
                {
                    let mut posts: HashMap<String, String> = HashMap::new();
                    // v1.7.17 Runde 14 (externe Gegenpruefung, Codex,
                    // adversarial, 04.09.2026): merken, OB dieser Durchlauf
                    // "Departure Gate" mit einsendet — der Erfolg wird erst
                    // NACH der Antwort im Callback bestaetigt, siehe unten.
                    let dep_gate_wird_gepostet;
                    {
                        let mut stats = flight.stats.lock().expect("flight stats");
                        dep_gate_wird_gepostet = if !stats.dep_gate_field_posted {
                            if let Some(g) = stats.dep_gate.clone().filter(|s| !s.is_empty()) {
                                stats.dep_gate_field_posted = true;
                                posts.insert("Departure Gate".into(), g);
                                true
                            } else {
                                false
                            }
                        } else {
                            false
                        };
                        if !stats.arr_gate_field_posted && stats.phase == FlightPhase::BlocksOn {
                            if let Some(g) = stats.arr_gate.clone().filter(|s| !s.is_empty()) {
                                stats.arr_gate_field_posted = true;
                                posts.insert("Arrival Gate".into(), g);
                            }
                        }
                    }
                    if !posts.is_empty() {
                        let client = client.clone();
                        let pirep_id = flight.pirep_id.clone();
                        // v1.7.17 Runde 14: der Flug-Griff selbst, NICHT nur
                        // Client/Pirep-Id — der Callback muss nach Erfolg in
                        // die Statistik zurueckschreiben koennen.
                        let flight_fuer_bestaetigung = flight.clone();
                        // v1.7.17 Runde 15/16: der App-Griff, um eine
                        // bereits EINGEFRORENE Rueckzugsentscheidung
                        // (`dep_gate_withdrawal_awaiting_confirmation`)
                        // sofort zu checkpointen, sobald sie ausgefuehrt
                        // wird — dieselbe Vorsicht wie an der Boarding-
                        // Ende-Stelle selbst.
                        let app_fuer_bestaetigung = app.clone();
                        // v1.7.17 Runde 16 (externe Gegenpruefung, Codex,
                        // adversarial, 04.09.2026): der Flug-Handle selbst,
                        // um `flight.stop` im Callback erneut zu pruefen —
                        // siehe die Begruendung an den Schreibstellen unten.
                        let flight_fuer_stop_check = flight.clone();
                        tauri::async_runtime::spawn(async move {
                            match client.post_pirep_fields(&pirep_id, &posts).await {
                                Ok(()) => {
                                    // ⚠ v1.7.17 Runde 14 (externe
                                    // Gegenpruefung, Codex, adversarial,
                                    // 04.09.2026): NUR nach bestaetigtem
                                    // Erfolg — ein bloss VERSUCHTER Post
                                    // (das alte `dep_gate_field_posted`)
                                    // reicht nicht als Grundlage fuer die
                                    // spaetere DESTRUKTIVE Rueckzugs-
                                    // Entscheidung. Ein Fehlschlag hier
                                    // laesst `dep_gate_field_post_confirmed`
                                    // bewusst `false` — dann darf die
                                    // finale Einreichung spaeter NICHT
                                    // leeren, weil AeroACARS nachweislich
                                    // nie erfolgreich etwas gepostet hat.
                                    if !dep_gate_wird_gepostet {
                                        return;
                                    }
                                    // v1.7.17 Runde 16 (externe
                                    // Gegenpruefung, Codex, adversarial,
                                    // 04.09.2026): NUR eine BEREITS am
                                    // Boarding-Ende getroffene, eingefrorene
                                    // Entscheidung ausfuehren — NICHT die
                                    // Bedingung hier neu bewerten. Der
                                    // Runde-15-Versuch bewertete
                                    // `dep_gate_muss_beim_boarding_ende_
                                    // verworfen_werden` HIER neu und scheiterte
                                    // doppelt: `dep_gate` war zu diesem
                                    // Zeitpunkt immer schon `None` (der Riegel
                                    // loescht es sofort, unabhaengig von der
                                    // Bestaetigung) — die Neubewertung fand
                                    // darum NIE etwas; und ein spaeterer,
                                    // voellig unrelated Reconnect haette als
                                    // "der verpasste Uebergang" fehlklassifiziert
                                    // werden koennen. Siehe die Funktionsdoku
                                    // an `dep_gate_withdrawal_awaiting_
                                    // confirmation` fuer die volle Herleitung.
                                    let muss_nachgeholt_werden = {
                                        let Ok(mut stats) = flight_fuer_bestaetigung.stats.lock()
                                        else {
                                            return;
                                        };
                                        stats.dep_gate_field_post_confirmed = true;
                                        if stats.dep_gate_withdrawal_awaiting_confirmation {
                                            stats.dep_gate_withdrawal_awaiting_confirmation = false;
                                            stats.dep_gate_withdrawal_pending = true;
                                            true
                                        } else {
                                            false
                                        }
                                    };
                                    // v1.7.17 Runde 15 (zweiter Befund): die
                                    // Bestaetigung selbst SOFORT checkpointen
                                    // — sonst wartet ihre Haltbarkeit auf den
                                    // naechsten periodischen Checkpoint
                                    // (mehrere Ticks entfernt), und ein
                                    // Absturz genau in diesem Fenster liesse
                                    // einen Resume wieder "nie gepostet"
                                    // glauben. Unbedingt, auch wenn KEINE
                                    // nachgeholte Entscheidung noetig war.
                                    //
                                    // v1.7.17 Runde 16: `flight.stop` erneut
                                    // pruefen — dieser Task ist DETACHED und
                                    // kann noch laufen, nachdem der Flug
                                    // laengst gefiled/abgebrochen/vergessen
                                    // wurde und seine Ablage geloescht ist;
                                    // ohne diese Pruefung wuerde dieser
                                    // Schreibzugriff die geloeschte Ablage
                                    // wiederbeleben (derselbe Schutz wie beim
                                    // periodischen Checkpoint).
                                    if !flight_fuer_stop_check
                                        .stop
                                        .load(std::sync::atomic::Ordering::Relaxed)
                                    {
                                        save_active_flight(
                                            &app_fuer_bestaetigung,
                                            &flight_fuer_bestaetigung,
                                        );
                                    }
                                    if muss_nachgeholt_werden {
                                        let mut rueckzug = HashMap::new();
                                        rueckzug
                                            .insert("Departure Gate".to_string(), String::new());
                                        post_pirep_fields_mit_kurzem_retry(
                                            &client, &pirep_id, &rueckzug,
                                        )
                                        .await;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        error = %e,
                                        "live stand field post failed (non-fatal)"
                                    );
                                }
                            }
                        });
                    }
                }
            }

            // v0.16.24: approach-phase divert navdata pre-fetch. On a
            // committed, stable descending approach to a field OTHER than
            // the planned `arr_airport`, warm that field's Navigraph navdata
            // so the runway correlation at touchdown hits the cache instead
            // of falling back to the coarse OurAirports CSV. The decision is
            // pure + debounced (`divert_prefetch_decision`); the fetch is
            // idempotent per ICAO (`spawn_navdata_fetch` skips cached ones),
            // so this is bounded — it only fires on a genuine divergence.
            {
                let prefetch_icao = {
                    let mut stats = flight.stats.lock().expect("flight stats");
                    divert_prefetch_decision(&mut stats, &snap, &flight.arr_airport, Utc::now())
                };
                if let Some(icao) = prefetch_icao {
                    tracing::info!(
                        actual = %icao,
                        planned = %flight.arr_airport,
                        "divert approach detected — pre-fetching navdata for actual landing airport"
                    );
                    spawn_navdata_fetch(&app, &flight, vec![icao]);
                }
            }

            // v0.16.24: on-demand navdata fetch at touchdown. If the FSM
            // Landing arm just correlated the runway and had to fall back to
            // OurAirports (the actual landing field wasn't cached — e.g. a
            // late divert the approach pre-fetch didn't catch), kick off a
            // fetch for the real airport NOW so `build_landing_record` can
            // re-correlate against the warm cache at PIREP-filing time
            // (finalize-with-best). Non-racy: the provisional OurAirports
            // result is already stamped; this only improves what the record
            // builder sees later. Idempotent (per-ICAO guard).
            maybe_fetch_actual_landing_navdata(&app, &flight);

            // v0.16.6: rollout for sampler-path landings (bush flights — the
            // FSM never reaches Landing, so its arm never accumulates). Runs
            // AFTER step_flight so the post-step `stats.phase` gates out the
            // Landing case (the FSM arm has just ticked the same helper) and
            // BEFORE the rollout-finalized publish block below so a finalise
            // on this tick is published immediately.
            {
                let mut stats = flight.stats.lock().expect("flight stats");
                sampler_path_rollout_tick(&mut stats, &snap);
            }

            // v2s Sicht dieses Takts (gerechnet in `phasen_takt` direkt nach
            // dem v1-Schritt) für die Flug-Logs; `None` bei Pause/Slew.
            if let Some((shadow_phase, shadow_segment)) = takt.v2 {
                // Additive Telemetrie-Felder: landen via `snap.clone()` im
                // JSONL-Position-Event (lokaler Recorder → VPS-Upload).
                // Der alte VPS-Recorder speichert sie verbatim mit — NULL
                // Recorder-Änderungen nötig.
                snap.shadow_phase = Some(phase_to_snake(shadow_phase).to_string());
                snap.shadow_segment = Some(shadow_segment.as_snake_str().to_string());
            }

            // v0.12.4 (Spec docs/spec/v0.12.4-score-consistency.md, LE4):
            // Sobald `step_flight` den Rollout finalisiert hat (~40 kt /
            // Heading-Turn-off), das touchdown_rollout_finalized-Event mit
            // dem FINALEN rollout_distance_m nachschicken. `touchdown_complete`
            // bleibt davon unberührt (Live-Event, schon vorher gesendet) —
            // dieses Event patcht beim Recorder nur das Anzeige-Rohfeld.
            //
            // Touchdown-scharf: `touchdown_at` = `landing_at` (= ts des
            // zugehörigen TouchdownPayloads). Wir publishen einmal pro
            // distinktem `landing_at` — bei Touch-and-Go bekommt also jeder
            // Touchdown sein eigenes Event statt nur der erste.
            {
                takt_uhr.abschnitt("bis_nachtrag");
                // ⚠ Erst der Schluessel, dann der Bau — siehe
                // `rollout_nachtrag_faellig`. Ist der Nachtrag schon
                // gesendet, wird in diesem Takt nichts gebaut. Erneut
                // gesendet wird, wenn die Bahnwerte sich SEITHER geaendert
                // haben — nicht nur einmal je Aufsetzen.
                // v3 (P-4.10): endgueltige Rechnung nach dem Ausrollen,
                // auch ohne Verbindung.
                endgueltige_rechnung_festhalten(&flight);
                let faellig = {
                    let stats = flight.stats.lock().expect("flight stats");
                    rollout_nachtrag_faellig(&flight, &stats, last_published_rollout)
                };
                if let Some(((touchdown_at, revision), mut payload)) = faellig {
                    let rollout_m = payload.rollout_distance_m;
                    let app_state = app.state::<AppState>();
                    // ⚠ Erst PRUEFEN, ob es einen Handle gibt — und die
                    // Sperre sofort wieder freigeben. Die Flugdatei
                    // wird danach OHNE gehaltene MQTT-Sperre geschrieben
                    // (blockierende E/A unter einer geteilten async-
                    // Sperre, externe QS Runde 3). Ohne Handle wird
                    // weder geschrieben noch gesendet; Sperre und Fahne
                    // bleiben stehen, der naechste Tick versucht es
                    // erneut. Der restliche Tick laeuft in JEDEM Fall
                    // weiter — kein `continue` (N1, P0).
                    let mqtt_vorhanden = app_state.mqtt.lock().await.is_some();
                    if !mqtt_vorhanden {
                        if !mqtt_fehlt_gemeldet {
                            tracing::warn!(
                                revision,
                                "touchdown_rollout_finalized: keine MQTT-Verbindung — \
                                 der Nachtrag wartet"
                            );
                            mqtt_fehlt_gemeldet = true;
                        }
                    } else {
                        mqtt_fehlt_gemeldet = false;
                        // ⚠ ZUERST auf die Platte, DANN auf die Leitung.
                        // Siehe `revision_vor_versand_sichern`.
                        revision_vor_versand_sichern(&app, &flight);
                        let mqtt = app_state.mqtt.lock().await;
                        match mqtt.as_ref() {
                            // Zwischen Pruefung und Sperre verschwunden —
                            // naechster Tick.
                            None => {}
                            Some(handle) => {
                                // Zeitstempel des Versands, wie bisher.
                                payload.ts = Utc::now().timestamp_millis();
                                let gesendet = nachtrag_queue::senden(
                                    &app,
                                    &handle.nachtrag_sender(),
                                    payload,
                                );
                                // Sperre und Fahne fallen NUR, wenn der
                                // Nachtrag auf der Platte liegt (Runde 13,
                                // High 2). Sonst naechster Tick.
                                match gesendet {
                                    Ok(()) => {
                                        let nachtrag = last_published_rollout.is_some();
                                        last_published_rollout = Some((touchdown_at, revision));
                                        if let Ok(mut st) = flight.stats.lock() {
                                            st.bahn_nachtrag_offen = false;
                                            st.letzter_nachtrag = Some((touchdown_at, revision));
                                        }
                                        tracing::info!(
                                            pirep_id = %flight.pirep_id,
                                            touchdown_at,
                                            rollout_m,
                                            revision,
                                            nachtrag,
                                            "touchdown_rollout_finalized published"
                                        );
                                    }
                                    Err(e) => tracing::error!(
                                        pirep_id = %flight.pirep_id,
                                        error = %e,
                                        "Bahn-Nachtrag nicht abgelegt — Sperre bleibt, naechster Tick"
                                    ),
                                }
                            }
                        }
                    }
                }
            }

            takt_uhr.abschnitt("rollout_nachtrag");
            // v0.7.14: Pilot-Client-Discord-Posts fuer Takeoff + Landing
            // entfernt — Recorder postet das jetzt zentral via MQTT-Trigger
            // (`takeoff` + `touchdown` Events publishen sowieso, Recorder
            // abonniert sie). Audit Q4-2026-05 (C1).
            // v0.5.14: MQTT block + takeoff snapshots. Fire on the same
            // None→Some transitions used by the Discord webhook block
            // above. Block fires when block_off_at is stamped (=
            // pushback / first taxi-out motion); takeoff fires when
            // takeoff_at is stamped (= aircraft has lifted off).
            // Both use retain=true so a Monitor that joins mid-flight
            // sees the snapshot.
            {
                let block_payload_opt: Option<aeroacars_mqtt::BlockPayload> = {
                    let stats = flight.stats.lock().expect("flight stats");
                    if prev_block_off_at.is_none() && stats.block_off_at.is_some() {
                        Some(aeroacars_mqtt::BlockPayload {
                            ts: stats
                                .block_off_at
                                .map(|t| t.timestamp_millis())
                                .unwrap_or(0),
                            block_fuel_kg: stats.block_fuel_kg,
                            planned_block_fuel_kg: stats.planned_block_fuel_kg,
                            planned_burn_kg: stats.planned_burn_kg,
                            planned_reserve_kg: stats.planned_reserve_kg,
                            planned_zfw_kg: stats.planned_zfw_kg,
                            planned_tow_kg: stats.planned_tow_kg,
                            planned_ldw_kg: stats.planned_ldw_kg,
                            planned_max_zfw_kg: stats.planned_max_zfw_kg,
                            planned_max_tow_kg: stats.planned_max_tow_kg,
                            planned_max_ldw_kg: stats.planned_max_ldw_kg,
                            planned_route: stats.planned_route.clone(),
                            planned_alternate: stats.planned_alternate.clone(),
                            dep_gate: stats.dep_gate.clone(),
                            dep_metar: stats.dep_metar_raw.clone(),
                        })
                    } else {
                        None
                    }
                };
                let takeoff_payload_opt: Option<aeroacars_mqtt::TakeoffPayload> = {
                    let stats = flight.stats.lock().expect("flight stats");
                    if prev_takeoff_at.is_none() && stats.takeoff_at.is_some() {
                        Some(aeroacars_mqtt::TakeoffPayload {
                            ts: stats.takeoff_at.map(|t| t.timestamp_millis()).unwrap_or(0),
                            takeoff_weight_kg: stats.takeoff_weight_kg.map(|w| w as f32),
                            takeoff_fuel_kg: stats.takeoff_fuel_kg,
                            takeoff_lat: Some(snap.lat),
                            takeoff_lon: Some(snap.lon),
                            dep_metar: stats.dep_metar_raw.clone(),
                            dep_runway: snap.selected_runway.clone(),
                        })
                    } else {
                        None
                    }
                };
                if block_payload_opt.is_some() || takeoff_payload_opt.is_some() {
                    // v0.5.34: Forensik-Mitschnitt ins JSONL. Root Cause
                    // (07.09.2026, gleiche Fehlerklasse wie der Rollout-
                    // Nachtrag bei ITY 1358): vorher stand der Mitschnitt
                    // INNERHALB von `if let Some(handle) = mqtt.as_ref()` —
                    // fiel die Verbindung genau im Block-Off-/Takeoff-
                    // Moment aus, wurde der Snapshot weder gesendet noch
                    // lokal aufgezeichnet und war damit auch ueber
                    // Gap-Fill-Import nie nachholbar. Jetzt zuerst
                    // unconditional geloggt, MQTT-Publish bleibt
                    // best-effort.
                    if let Some(p) = &block_payload_opt {
                        record_event(
                            &app,
                            &flight.pirep_id,
                            &FlightLogEvent::BlockSnapshot {
                                timestamp: Utc::now(),
                                payload: serde_json::to_value(p).unwrap_or(serde_json::Value::Null),
                            },
                        );
                    }
                    if let Some(p) = &takeoff_payload_opt {
                        record_event(
                            &app,
                            &flight.pirep_id,
                            &FlightLogEvent::TakeoffSnapshot {
                                timestamp: Utc::now(),
                                payload: serde_json::to_value(p).unwrap_or(serde_json::Value::Null),
                            },
                        );
                    }
                    let app_state = app.state::<AppState>();
                    let mqtt = app_state.mqtt.lock().await;
                    if let Some(handle) = mqtt.as_ref() {
                        if let Some(p) = block_payload_opt {
                            handle.block(p);
                        }
                        if let Some(p) = takeoff_payload_opt {
                            handle.takeoff(p);
                        }
                    }
                }
            }
            // Detect when the touchdown-analyzer window has just locked
            // in a final score so we can emit the activity-log entry
            // exactly once. `landing_score` flips from None to Some
            // inside `step_flight` after TOUCHDOWN_WINDOW_SECS.
            // Returns Some(message) the first time a score is announced
            // so the streamer can mirror it into `acars/logs`.
            let landing_log_message = announce_landing_score(&app, &flight);
            // v0.5.11: MQTT touchdown event. Fires once per landing
            // when the score message gets generated (= touchdown was
            // captured + scored). Snapshot relevant fields from
            // FlightStats inside a short-lived block so the
            // std::sync::MutexGuard (not Send) doesn't span the
            // tokio Mutex `.await` below.
            if landing_log_message.is_some() {
                // v0.7.18 (B-012): Touchdown-Airport-Resolution. Lookup-
                // Closure greift in den AppState.airports-Cache.
                let airport_lookup_data: std::collections::HashMap<String, (f64, f64)> = {
                    let app_state = app.state::<AppState>();
                    let airports = app_state.airports.lock().expect("airports lock");
                    airports
                        .iter()
                        .filter_map(|(k, v)| match (v.lat, v.lon) {
                            (Some(la), Some(lo)) => Some((k.clone(), (la, lo))),
                            _ => None,
                        })
                        .collect()
                };
                let payload_opt: Option<aeroacars_mqtt::TouchdownPayload> = {
                    let stats = flight.stats.lock().expect("flight stats");
                    // v0.15.23: derive the touchdown timestamp from the sampler's
                    // validated touchdown, falling back to the FSM `landing_at`.
                    // On the short/low VFR path the sampler finalizes the score
                    // BEFORE the FSM stamps `landing_at` (which only happens at
                    // Arrived via the fallback, ~70-215 s later) — gating this live
                    // MQTT touchdown publish on `landing_at` meant the touchdown
                    // was scored but NEVER published, so the recorder saw
                    // no_touchdown → INT hold (GSG308/GSG301). `sampler_touchdown_at`
                    // is set the instant the touchdown validates, in any phase, so
                    // the publish now fires on the same tick the score is announced.
                    stats
                        .sampler_touchdown_at
                        .or(stats.landing_at)
                        .map(|td_ts| {
                            let rwy_match = stats.runway_match.as_ref();
                            // B-012: Touchdown-Airport via Cascade aufloesen.
                            let td_resolution = resolve_touchdown_airport(
                                rwy_match,
                                stats.landing_lat,
                                stats.landing_lon,
                                &flight.arr_airport,
                                |icao| airport_lookup_data.get(&icao.to_uppercase()).copied(),
                            );
                            // v0.19.3: welches Feld liegt unter den Rädern? Der
                            // Trust-Check fragt „gehört die gematchte Runway zu dem
                            // Flughafen, auf dem wir tatsächlich sind" — das ist eine
                            // Frage nach dem ECHTEN Landeplatz, nicht nach dem Plan.
                            //
                            // Bisher bekam er hier den PLANflughafen plus
                            // `stats.divert_hint`, der zum Touchdown-Zeitpunkt noch
                            // `None` ist (der Hint entsteht erst im Stillstand). Bei
                            // jedem echten Divert lief der Check deshalb auf
                            // `icao_mismatch` → `runway_geometry_trusted=false` und
                            // die Touchdown-Zone wurde unterdrückt — während derselbe
                            // Check beim Einreichen mit dem bestätigten Ausweichfeld
                            // `trusted=true` ergab. Der Recorder speicherte für EINE
                            // Landung ein sich widersprechendes Paar, und der Pilot
                            // sah für eine saubere Divert-Landung ein
                            // „Geometrie nicht vertrauenswürdig"-Pill.
                            //
                            // `arrival::locate` an den Touchdown-Koordinaten liefert
                            // dieselbe Antwort, die der Divert-Hint später liefert —
                            // nur eben schon jetzt.
                            //
                            // Codex-Befund zu P-4.40: Gerechnet wird das jetzt
                            // EINMAL in `announce_landing_score` (mit diesem
                            // Aufsetz-Flughafen, siehe `aufsetz_flughafen`); der
                            // Payload liest die festgehaltene Bewertung.
                            // v0.20.0: EIN Verdict — Punkte, Klasse und Note stammen
                            // aus demselben Aufruf. Die Webapp leitet nichts mehr ab.
                            let (payload_score_label, payload_score_grade, payload_bewertung_status) =
                                touchdown_payload_note(&stats);
                            aeroacars_mqtt::TouchdownPayload {
                                landung_nicht_bewertbar: landung_nicht_bewertbar_json(&stats),
                                // v1.7.15: die Bahn-Herkunft — EINE
                                // Ableitung, dieselbe wie am
                                // `touchdown_rollout_finalized`.
                                herkunft: bahn_herkunft(&stats),
                                ts: td_ts.timestamp_millis(),
                                // v0.11.1: Pilot-Client-Version aus dem Cargo-
                                // Manifest in jeden Touchdown mitsenden, damit
                                // die Webapp-Reports-Liste + LandingAnalysis-
                                // Header die Version direkt anzeigen koennen
                                // (vorher war client_version nur in FlightMeta,
                                // landete also nie in der Touchdown-DB-Row).
                                client_version: Some(env!("CARGO_PKG_VERSION")),
                                // v0.7.19 (QS-R2 Finding 1): Recorder/Webapp
                                // targeted Korrektur-Events per pirep_id auf
                                // den exakten Touchdown-Row.
                                pirep_id: Some(flight.pirep_id.clone()),
                                // v0.7.17 (B-015a QS-Round-2): Edge-Wert hat
                                // Vorrang — siehe `score_basis_vs_fpm()` Doc.
                                // Live-Touchdown-MQTT-Top-Level-Wert war
                                // bisher der einzige verbleibende Pfad mit
                                // der alten `landing_rate_fpm.or(peak)`-
                                // Cascade. Konsumenten (Recorder-DB
                                // `touchdowns.vs_fpm`, Discord-Touchdown-
                                // Posts, Live-Hardlanding-Notifications)
                                // bekommen jetzt durchgaengig den Edge-Wert.
                                vs_fpm: score_basis_vs_fpm(&stats)
                                    .map(|v| v.round() as i32)
                                    .unwrap_or(0),
                                ias_kt: stats
                                    .landing_speed_kt
                                    .map(|v| v.round() as i32)
                                    .unwrap_or(0),
                                // v0.5.17: GS captured separately from IAS,
                                // falls aware of head/tailwind.
                                gs_kt: stats.landing_groundspeed_kt.map(|v| v.round() as i32),
                                pitch_deg: stats.landing_pitch_deg,
                                // v0.5.17: bank at touchdown (= "Landing
                                // Roll" in maintenance-plugin lingo,
                                // captured in v0.5.16 alongside pitch).
                                bank_deg: stats.landing_bank_deg,
                                // g_load/peak_g_load bleiben roh (Forensik-Display,
                                // nicht Teil von compute_sub_scores) — unveraendert.
                                // Codex, zweite Abnahme: hier heissen die Felder
                                // `g_load`/`peak_g_load` — die erste Filterung
                                // suchte die Record-Namen und ging vorbei.
                                g_load: aus_dem_fenster(&stats, stats.landing_g_force),
                                peak_g_load: aus_dem_fenster(&stats, stats.landing_peak_g_force),
                                // v0.20 (QS-Fix, fallback_zero-Score-Bug): der
                                // "faire" EMA-Score wird None statt einer erfundenen
                                // Zahl, wenn keine echte Quelle je einen Wert lieferte.
                                scored_g_load: scored_g_fuer_punkte(&stats),
                                scored_g_method: score_g_for_stats(&stats)
                                    .map(|s| s.method.as_str().to_string()),
                                sideslip_deg: stats.touchdown_sideslip_deg,
                                headwind_kt: stats.landing_headwind_kt,
                                crosswind_kt: stats.landing_crosswind_kt,
                                // `score` bleibt die diskrete Touchdown-Klasse
                                // (100/80/60/30/0) — das ist die Wire-Semantik, auf
                                // die der Recorder und die Webapp gebaut sind. Den
                                // Composite propagiert der Recorder separat aus der
                                // PIREP-Zeile in die Spalte `landing_score`.
                                score: stats.landing_score.map(|s| s.numeric()),
                                // v0.20.0: das EINGEFRORENE Urteil zum Composite —
                                // Klasse und Note, wie der Client sie zeigt. Die
                                // Webapp leitete das Wort bisher selbst aus einer
                                // Zahl ab, mit einer eigenen Leiter (90/70/45) neben
                                // unserer (88/75/50). Zwei Kopien derselben Regel
                                // driften — genau die PIA3452-Krankheit. Jetzt
                                // klassifiziert der Client, die Webapp zeigt an.
                                score_label: payload_score_label,
                                score_grade: payload_score_grade,
                                bounce: hopser_aus_dem_fenster(&stats).map(|n| n > 0),
                                bounce_count: hopser_aus_dem_fenster(&stats).map(|n| n as _),
                                // v0.16.6 (Daten-Audit 2026-06-11): `approach_runway`
                                // ist praktisch nie gesetzt → das Top-Level-Feld war
                                // bei 407/407 VPS-Flügen None, und die Webapp-Anzeigen
                                // (Stats/Heatmap/Pilots + Live-Notifications), die
                                // `touchdowns.runway` lesen, zeigten NIE eine Runway —
                                // obwohl der Korrelations-Match (97,5 % Trefferquote)
                                // direkt daneben im Payload steht. Fallback auf den
                                // Match-Ident füllt die DB-Spalte für neue Flüge.
                                runway: stats.approach_runway.clone().or_else(|| {
                                    stats.runway_match.as_ref().map(|m| m.runway_ident.clone())
                                }),
                                // v0.7.18 (B-012): aufgelöster Touchdown-Airport
                                // statt blind auf flight.arr_airport zu setzen.
                                airport: Some(td_resolution.icao.clone()),
                                airport_source: Some(
                                    td_resolution.source.as_wire_str().to_string(),
                                ),
                                airport_distance_to_destination_nm: td_resolution
                                    .distance_to_destination_nm,
                                airport_nearest_distance_nm: td_resolution.nearest_distance_nm,
                                // v0.7.18 (R1-4): Plan-Destination mit-publishen
                                // damit die Webapp den Off-airport-Banner gegen
                                // den echten geplanten Wert vergleichen kann.
                                planned_arr_airport: Some(flight.arr_airport.clone()),
                                lat: stats.landing_lat,
                                lon: stats.landing_lon,
                                heading_true_deg: stats.landing_heading_true_deg,
                                heading_mag_deg: stats.landing_heading_deg,
                                landing_weight_kg: stats.landing_weight_kg.map(|w| w as f32),
                                landing_fuel_kg: stats.landing_fuel_kg,
                                // v0.7.17 (B-015d): OFP-Plan-Werte ans MQTT-
                                // Touchdown-Payload anhängen damit die Webapp
                                // den Loadsheet-Sub-Score genauso berechnen
                                // kann wie der Pilot-Client.
                                planned_zfw_kg: stats.planned_zfw_kg,
                                planned_tow_kg: stats.planned_tow_kg,
                                rollout_distance_m: stats.rollout_distance_m.map(|d| d as f32),
                                approach_vs_stddev_fpm: stats.canonical_vs_stddev_fpm(),
                                approach_bank_stddev_deg: stats.canonical_bank_stddev_deg(),
                                go_around_count: Some(stats.go_around_count),
                                arr_metar: stats.arr_metar_raw.clone(),
                                // v0.5.23: Touchdown-Forensik — alle Schaetzer-
                                // Zwischenergebnisse fuer Server-seitige
                                // Algorithmus-Vergleiche (siehe stats Felder
                                // landing_vs_estimate_*_fpm + landing_vs_source).
                                simulator: stats.landing_simulator.map(|s| s.to_string()),
                                vs_estimate_xp_fpm: stats.landing_vs_estimate_xp_fpm,
                                vs_estimate_msfs_fpm: stats.landing_vs_estimate_msfs_fpm,
                                vs_source: stats.landing_vs_source.map(|s| s.to_string()),
                                gear_force_peak_n: stats.landing_gear_force_peak_n,
                                estimate_window_ms: stats.landing_estimate_window_ms,
                                estimate_sample_count: stats.landing_estimate_sample_count,
                                // v0.5.25: Approach-Stability v2 — Stable-Approach-
                                // Gate-konform mit HAT-statt-AGL, V/S-Jerk,
                                // IAS-σ, Excessive-Sink, Stable-Config.
                                approach_vs_deviation_fpm: stats.approach_vs_deviation_fpm,
                                approach_max_vs_deviation_below_500_fpm: stats
                                    .approach_max_vs_deviation_below_500_fpm,
                                approach_bank_stddev_filtered_deg: stats
                                    .approach_bank_stddev_filtered_deg,
                                approach_runway_changed_late: stats.approach_runway_changed_late,
                                // Score-Version 19: dasselbe Urteil wie die Note.
                                approach_stable_at_gate: anflug_stabil_am_gate(&stats),
                                approach_window_sample_count: stats.approach_window_sample_count,
                                approach_vs_jerk_fpm: stats.approach_vs_jerk_fpm,
                                approach_ias_stddev_kt: stats.approach_ias_stddev_kt,
                                approach_excessive_sink: stats.approach_excessive_sink,
                                approach_stable_config: stats.approach_stable_config,
                                approach_used_hat: Some(stats.approach_used_hat),
                                // v0.5.26
                                landing_wing_strike_severity_pct: stats
                                    .landing_wing_strike_severity_pct,
                                // v0.7.6 P1-3: Raw-Wert bleibt im Payload
                                // (interne Diagnostik), Web blendet bei
                                // runway_geometry_trusted=false aus.
                                landing_float_distance_m: stats.landing_float_distance_m,
                                // v0.7.6 P1-3: Bei untrusted Runway-Geometrie
                                // auf None setzen (kein "TD Zone 1" auf Basis
                                // einer falschen Runway-Geometrie wie GSG303
                                // v0.7.5: K5S9 statt OR66, 3.5 km Centerline-
                                // Offset). Spec docs/spec/v0.7.6-landing-
                                // payload-consistency.md §3 P1-3.
                                landing_touchdown_zone: zone_fuer_leitung(flight.as_ref(), &stats),
                                landing_vref_deviation_kt: stats.landing_vref_deviation_kt,
                                landing_vref_source: stats
                                    .landing_vref_source
                                    .map(|s| s.to_string()),
                                landing_vref_toleranz_kt: stats.landing_vref_toleranz_kt,
                                landing_vref_formel_kt: stats.landing_vref_formel_kt,
                                landing_vref_landeklappen: stats.landing_vref_landeklappen,
                                landing_vref_muster: stats.aufgeloestes_muster.clone(),
                                approach_stable_at_da: stats.approach_stable_at_da,
                                approach_spaetes_final_ab_ft: stats
                                    .approach_spaetes_final
                                    .map(|sf| sf.ab_hoehe_ft),
                                approach_spaetes_final_kurve_deg: stats
                                    .approach_spaetes_final
                                    .map(|sf| sf.kurve_deg),
                                approach_spaetes_final_querneigung_unter_300_deg: stats
                                    .approach_spaetes_final
                                    .map(|sf| sf.querneigung_max_unter_300_deg),
                                approach_spaetes_final_rechtzeitig: stats
                                    .approach_spaetes_final
                                    .map(|sf| sf.rechtzeitig),
                                approach_stall_warning_count: Some(
                                    stats.approach_stall_warning_count,
                                ),
                                approach_wind: stats.approach_wind.clone(),
                                landing_yaw_rate_deg_per_sec: stats.landing_yaw_rate_deg_per_sec,
                                landing_brake_energy_proxy: stats.landing_brake_energy_proxy,
                                landing_decel_mps2: stats.landing_decel_mps2,
                                // v0.12.4 (Spec docs/spec/v0.12.4-score-
                                // consistency.md, LE5): prozentuale Abweichung
                                // des **tatsächlichen Trip-Burn** (takeoff_fuel −
                                // landing_fuel) vom geplanten OFP-Trip-Burn
                                // (planned_burn_kg). Positiv = Mehrverbrauch.
                                //
                                // Vorher (bis v0.12.3) fälschlich block-fuel-basiert
                                // (block_fuel − landing_fuel) — das zählt den Taxi-
                                // out-Sprit mit und wich daher vom v0.7.1-Sub-Score
                                // ab (SAS9987: -2.28% hier vs -5.0% im sub_scores;
                                // DLH 1386: +9.5% statt korrekt +4.2%). Jetzt
                                // identische Basis wie `LandingRecord.
                                // fuel_efficiency_pct` und `sub_scores[].fuel`.
                                // Eingefroren in `sprit_auswertung_einmal` — hier nur gelesen.
                                sprit: stats.sprit_auswertung.clone(),
                                fuel_efficiency_pct: trip_burn_efficiency_pct(
                                    stats.takeoff_fuel_kg,
                                    stats.landing_fuel_kg,
                                    stats.planned_burn_kg,
                                ),
                                // v0.5.39: 50-Hz-Forensik aus dem Sampler-Buffer.
                                // landing_analysis ist ein serde_json::Value das
                                // von compute_landing_analysis() im Sampler-Loop
                                // geschrieben wird. Wenn None (Sampler hat noch
                                // nicht 10s post-TD gesehen, oder Touchdown vor
                                // Buffer-Init), bleiben alle Felder None.
                                // Auch hier gilt die Sperre: Reicht die
                                // Aufzeichnung nicht, gibt es die Zahl nicht.
                                // Sonst zeigte die Webapp einen Wert, während
                                // der Client daneben "nicht bewertbar" sagt —
                                // zwei Wirklichkeiten für denselben Touchdown
                                // (QS-Befund 12.09.2026 zum eigenen Umbau; die
                                // gleiche Stelle im LandingRecord war schon
                                // gefiltert, diese hier nicht).
                                vs_at_edge_fpm: ana_f32(&stats.landing_analysis, "vs_at_edge_fpm")
                                    .filter(|_| stats.landung_abdeckung_fehlt.is_none()),
                                // Herkunft der bewerteten Sinkrate plus beide
                                // Rohwerte — damit im Feld nachrechenbar ist,
                                // welche Quelle gegriffen hat.
                                vs_at_edge_quelle: stats
                                    .landing_analysis
                                    .as_ref()
                                    .filter(|_| stats.landung_abdeckung_fehlt.is_none())
                                    .and_then(|a| a.get("vs_at_edge_quelle"))
                                    .and_then(|v| v.as_str())
                                    .map(str::to_owned),
                                vs_geometrie_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_geometrie_fpm",
                                ),
                                vs_simvar_edge_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_simvar_edge_fpm",
                                ),
                                // v1.6.9 — Bestandteile + Sim-Referenz.
                                vs_gelaende_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_gelaende_fpm",
                                ),
                                vs_eigensinken_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_eigensinken_fpm",
                                ),
                                vs_sim_referenz_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_sim_referenz_fpm",
                                ),
                                vs_smoothed_250ms_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_smoothed_250ms_fpm",
                                ),
                                vs_smoothed_500ms_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_smoothed_500ms_fpm",
                                ),
                                vs_smoothed_1000ms_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_smoothed_1000ms_fpm",
                                ),
                                vs_smoothed_1500ms_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_smoothed_1500ms_fpm",
                                ),
                                peak_g_post_500ms: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "peak_g_post_500ms"),
                                ),
                                peak_g_post_1000ms: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "peak_g_post_1000ms"),
                                ),
                                // v0.7.17 (B-009): G-Force-Forensik
                                g_at_edge: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "g_at_edge"),
                                ),
                                g_smoothed_250ms_post: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "g_smoothed_250ms_post"),
                                ),
                                g_median_post_500ms: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "g_median_post_500ms"),
                                ),
                                g_p95_post_500ms: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "g_p95_post_500ms"),
                                ),
                                max_gear_force_n: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "max_gear_force_n"),
                                ),
                                peak_vs_pre_flare_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "peak_vs_pre_flare_fpm",
                                ),
                                vs_at_flare_end_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "vs_at_flare_end_fpm",
                                ),
                                flare_reduction_fpm: ana_f32(
                                    &stats.landing_analysis,
                                    "flare_reduction_fpm",
                                ),
                                flare_dvs_dt_fpm_per_sec: ana_f32(
                                    &stats.landing_analysis,
                                    "flare_dvs_dt_fpm_per_sec",
                                ),
                                flare_quality_score: ana_i32(
                                    &stats.landing_analysis,
                                    "flare_quality_score",
                                ),
                                flare_detected: ana_bool(&stats.landing_analysis, "flare_detected"),
                                bounce_max_agl_ft: aus_dem_fenster(
                                    &stats,
                                    ana_f32(&stats.landing_analysis, "bounce_max_agl_ft"),
                                ),
                                forensic_sample_count: ana_u32(
                                    &stats.landing_analysis,
                                    "sample_count",
                                ),
                                // v0.8.3 (#8): Forensic + scored Counts mit-publizieren.
                                // Quelle: touchdown_v2::compute_landing_analysis schreibt
                                // beide ins analysis-JSON (Zeile ~12863). aeroacars-live
                                // (recorder) + LandingPanel.tsx (Pilot-Client) sehen so
                                // den UnterschiedDes zwischen forensic (5 ft) und scored
                                // (15 ft) und koennen score-freie Hopser dezent surface.
                                forensic_bounce_count: ana_u32(
                                    &stats.landing_analysis,
                                    "forensic_bounce_count",
                                )
                                .filter(|_| stats.landung_abdeckung_fehlt.is_none())
                                .map(|n| n.min(u8::MAX as u32) as u8),
                                scored_bounce_count: ana_u32(
                                    &stats.landing_analysis,
                                    "scored_bounce_count",
                                )
                                .filter(|_| stats.landung_abdeckung_fehlt.is_none())
                                .map(|n| n.min(u8::MAX as u32) as u8),
                                scored_light_bounce_count: ana_u32(
                                    &stats.landing_analysis,
                                    "scored_light_bounce_count",
                                )
                                .filter(|_| stats.landung_abdeckung_fehlt.is_none())
                                .map(|n| n.min(u8::MAX as u32) as u8),
                                // v0.7.6 P1-3: Trust-Status auch in den
                                // touchdown_complete-Payload damit aeroacars-
                                // live (Touchdown-Tab) und ggf. Pilot-Client-
                                // Replay-View den Hinweis-Pill rendern koennen.
                                // Pure-function Check, gleiche Inputs wie bei
                                // landing_touchdown_zone (siehe oben).
                                // v1.7.15 (Runde 3): EINE Ableitung mit dem
                                // Nachtrag — `bahnvertrauen`.
                                runway_geometry_trusted: Some(
                                    bahnvertrauen(flight.as_ref(), &stats).0,
                                ),
                                runway_geometry_reason: bahnvertrauen(flight.as_ref(), &stats)
                                    .1
                                    .map(|r| r.to_string()),
                                // v0.7.19 GAF-707 Accident-Detection.
                                // `accident_classifier_version` ist der
                                // Sentinel — v0.7.19+ setzt ihn IMMER, auch
                                // wenn kein Accident erkannt wurde. Webapp
                                // unterscheidet damit "Classifier lief, kein
                                // Accident" von "historischer Payload, bitte
                                // nachklassifizieren".
                                accident_classifier_version: Some(
                                    accident::ACCIDENT_CLASSIFIER_VERSION.into(),
                                ),
                                accident: if stats.accident_detected {
                                    Some(true)
                                } else {
                                    None
                                },
                                accident_kind: stats.accident_kind.clone(),
                                accident_confidence: stats.accident_confidence.clone(),
                                accident_reasons: if stats.accident_reasons.is_empty() {
                                    None
                                } else {
                                    Some(stats.accident_reasons.clone())
                                },
                                accident_at: stats.accident_at.map(|t| t.timestamp_millis()),
                                // v0.10.0 (#runway-utilization-score): Marker
                                // dass das nachgelagert berechnete sub_scores-
                                // Array via sub_rollout_v2 (LDA-basiert)
                                // entstanden ist.
                                // v0.16.21: bump 3→4 — MSFS touchdown V/S
                                // SimVar-lag corrected (g-force-gated de-lag).
                                // v0.20.x: bump 4→5 — Bahnauslastung-QS
                                // (Float-Toleranz + Banding grosszuegiger) +
                                // Sinkraten-Ziel-Korridor.
                                // v1.6.7: bump 7→8 — Minderverbrauch bis 15 %
                                // voll bepunktet; Bahn-Auslastung auf die echte
                                // genutzte Strecke umgestellt.
                                score_algorithm_version: Some(SCORE_ALGORITHMUS_VERSION),
                                // P-4.40 (Kap. 4.5): Herkunft der Note, siehe `score_grade`.
                                bewertung_status: payload_bewertung_status,
                                // v3 (P-5.10): Paketstand, Nummer des Aufsetzers
                                // und Hash der Eingabe der Sofortnote.
                                runtime_bundle_id: laufzeit_paket::flug_pin(&flight.pirep_id).map(|id| id.to_string()),
                                seq: Some(touchdown_seq(&stats)),
                                input_sha256: Some(touchdown_input_sha256(&flight, &stats)),
                                // ── v1.7.0 Bahndisziplin ─────────────────
                                //
                                // Dieselbe Ableitung wie fuer die Anzeige im
                                // Client: `bahn_felder()` ist die eine Stelle,
                                // an der aus dem Rollout-Fenster Zahlen
                                // werden. Der Server rechnet nichts nach — er
                                // bekaeme aus groberen Daten zwangslaeufig
                                // andere Werte, und zwei Zahlen fuer dieselbe
                                // Landung sind schlimmer als eine fehlende.
                                bahn: bahn_felder(
                                    &stats,
                                    // Das aufgeloeste Muster (Sim → Buchung →
                                    // Titel), nicht nur die Buchung. Hier stand
                                    // vorher allein `flight.aircraft_icao`; war
                                    // die leer, verlor die Landung Spurweite,
                                    // Spannweite und Randabstand.
                                    stats
                                        .aufgeloestes_muster
                                        .as_deref()
                                        .or(Some(flight.aircraft_icao.as_str()))
                                        .filter(|s| !s.is_empty()),
                                    // Kein Skip-Grund: Die Bewertung laeuft
                                    // erst nach diesem Publish (siehe den
                                    // Hinweis zum nachgelagerten
                                    // `sub_scores`-Array weiter oben). Hier
                                    // etwas zu behaupten waere geraten — die
                                    // Webapp liest den Grund ohnehin aus den
                                    // `sub_scores` des PIREP.
                                    None,
                                )
                                .wire(false),
                            }
                        })
                };
                if let Some(payload) = payload_opt {
                    // v3 (P-5.01): der Anflugpuffer dieses Aufsetzers ins JSONL.
                    let puffer = anflugpuffer_json(&flight.stats.lock().expect("flight stats lock"));
                    record_event(
                        &app,
                        &flight.pirep_id,
                        &FlightLogEvent::AnflugPuffer { timestamp: Utc::now(), proben: puffer },
                    );
                    // v0.5.34: gleicher Forensik-Payload landet ins JSONL.
                    // Damit kann ein Recovery-Tool die Touchdown-DB-Row
                    // 1:1 rekonstruieren falls die Server-DB Daten verliert.
                    record_event(
                        &app,
                        &flight.pirep_id,
                        &FlightLogEvent::TouchdownComplete {
                            timestamp: Utc::now(),
                            payload: serde_json::to_value(&payload)
                                .unwrap_or(serde_json::Value::Null),
                        },
                    );
                    {
                        let mut st = flight.stats.lock().expect("flight stats lock");
                        st.gemeldete_landung = Some((payload.ts, payload.vs_fpm));
                        st.landung_payload = Some(Box::new(payload.clone()));
                        st.landung_live_bestaetigt = false;
                    }
                    let app_state = app.state::<AppState>();
                    let mqtt = app_state.mqtt.lock().await;
                    if let Some(handle) = mqtt.as_ref() {
                        // Mit Zustellmeldung. Das Warten läuft nebenher —
                        // der Takt darf nicht auf das PUBACK warten.
                        let ack = handle.touchdown_bestaetigt(payload);
                        let app_fuer_ack = app.clone();
                        let pirep_fuer_ack = flight.pirep_id.clone();
                        tauri::async_runtime::spawn(async move {
                            let ok = tokio::time::timeout(std::time::Duration::from_secs(30), ack)
                                .await
                                .ok()
                                .and_then(|r| r.ok())
                                .unwrap_or(false);
                            landung_zustellung_vermerken(&app_fuer_ack, &pirep_fuer_ack, ok);
                        });
                    }
                }
            }
            // Diff cockpit knobs against last-seen values and log changes
            // to the activity feed. One entry per change, not per tick.
            detect_telemetry_changes(&app, &flight, &snap);
            // Bordbuch (26.09.2026): Cockpit-Routinen abhaken — ohne Einfluss
            // auf Landebewertung und PIREP.
            bordbuch_tick(&app, &flight, &snap);
            let position = snapshot_to_position(&snap);

            // Collect any text-log entries we want to mirror into phpVMS's
            // `/acars/logs` for the PIREP detail page. Phase changes get a
            // generic "Phase: <name>" line; landings get the touchdown
            // summary. Posted in a single batch at the end of the tick to
            // minimise HTTP round-trips.
            let mut acars_log_entries: Vec<api_client::LogEntry> = Vec::new();
            // Auslöser ist „die gültige Phase hat sich geändert", jeden Takt
            // gegen `last_logged_effective_phase` geprüft (nicht
            // `phase_change`) — fängt auch Wechsel ab, die den normalen
            // Commit-Pfad umgehen (Airborne-Rescue, Resume), und meldet
            // jeden Wechsel genau einmal. Seit 17.10.2026 („Stück 2") ist
            // die gültige Phase schlicht `stats.phase` (vorher v2s
            // Anzeige-Phase mit Holding-Sonderfall aus v1).
            let (effective_phase_changed, effective_phase_this_tick) = {
                let mut stats = flight.stats.lock().expect("flight stats");
                let effective = stats.phase;
                let changed = stats.last_logged_effective_phase != Some(effective);
                if changed {
                    stats.last_logged_effective_phase = Some(effective);
                }
                (changed, effective)
            };
            if effective_phase_changed {
                // Das lokale Aktivitaetsprotokoll schreibt DIESELBE Phase wie
                // phpVMS und die Karte. Bis v1.7.52 stand hier die alte
                // Phasenlogik: Im Stufensinkflug sprang das Protokoll bei
                // jedem Abfangen auf „Cruise", waehrend phpVMS korrekt
                // „Descent" bekam — beim Lesen der Logs nicht von einem
                // echten Fehler zu unterscheiden (Feldbefund 23.09.2026,
                // 7 von 8 gepruefte Logs).
                log_activity_handle(
                    &app,
                    ActivityLevel::Info,
                    format!("Phase: {:?}", effective_phase_this_tick),
                    Some(format!(
                        "Alt {:.0} ft, GS {:.0} kt, AGL {:.0} ft",
                        snap.altitude_msl_ft, snap.groundspeed_kt, snap.altitude_agl_ft
                    )),
                );
                acars_log_entries.push(api_client::LogEntry {
                    id: None,
                    log: format!("Phase: {}", phase_human_label(effective_phase_this_tick)),
                    lat: Some(snap.lat),
                    lon: Some(snap.lon),
                    created_at: Some(Utc::now().to_rfc3339()),
                });
                // v0.5.11: live-tracking phase publish. Retained
                // message — Monitor sees current phase on connect
                // even when joining mid-flight.
                {
                    let app_state = app.state::<AppState>();
                    let mqtt = app_state.mqtt.lock().await;
                    if let Some(handle) = mqtt.as_ref() {
                        handle.phase(effective_phase_this_tick, Utc::now());
                    }
                }
            }
            if let Some(msg) = landing_log_message {
                acars_log_entries.push(api_client::LogEntry {
                    id: None,
                    log: msg,
                    lat: Some(snap.lat),
                    lon: Some(snap.lon),
                    created_at: Some(Utc::now().to_rfc3339()),
                });
            }

            // Drain any T&G / go-around log lines that the FSM
            // queued during this tick (or accumulated since the
            // last tick — the touchdown sampler runs at 30 Hz, the
            // streamer ticks at ~5 s, so multiple events can stack
            // up between drains). Order is preserved so the PIREP
            // detail page reads chronologically.
            {
                let mut stats = flight.stats.lock().expect("flight stats");
                if !stats.pending_acars_logs.is_empty() {
                    let drained: Vec<String> = std::mem::take(&mut stats.pending_acars_logs);
                    drop(stats);
                    let now_ts = Utc::now().to_rfc3339();
                    for line in drained {
                        // Mirror to the in-app activity log so the
                        // pilot sees the T&G / go-around immediately
                        // in the cockpit dashboard, not just after the
                        // PIREP is filed and they look at phpVMS. The
                        // 5 s dedupe window is fine here — successive
                        // T&Gs are >1 min apart (FSM has to descend
                        // back to Approach in between).
                        log_activity_handle(&app, ActivityLevel::Info, line.clone(), None);
                        acars_log_entries.push(api_client::LogEntry {
                            id: None,
                            log: line,
                            lat: Some(snap.lat),
                            lon: Some(snap.lon),
                            created_at: Some(now_ts.clone()),
                        });
                    }
                }
            }

            // v0.6.0 — Critical-Window + In-Tick-Drain komplett entfernt.
            //
            // Vorher (v0.5.39-v0.5.51): Streamer-Tick hatte komplexe
            // Critical-Window-Logik die phpVMS-POSTs während Approach
            // pausierte + drain-Pattern beim Tick-Wiederholen. Beides
            // war Workaround für blockierende POSTs im Tick.
            //
            // Jetzt: phpVMS-POSTs laufen IM EIGENEN Worker
            // (`spawn_phpvms_position_worker`) — vollständig entkoppelt
            // vom Streamer-Tick. Tick muss nur in die Memory-Outbox
            // pushen (= 1µs Mutex-Push, blockt nie). Worker bedient
            // Outbox phase-aware mit Per-Item-Timeout.
            //
            // Effekt:
            // - Streamer-Tick blockiert NIE auf phpVMS (strukturell garantiert)
            // - Live-Map-Track läuft kontinuierlich bis zum Gate
            // - Sample-Cadence im Approach wird NICHT mehr durch HTTP-Latenz gestreckt

            // Diagnostics-Fund 2026-07-26: STALL-WARNING fuer den
            // Live-Payload daempfen (siehe STALL_WARNING_MQTT_DEBOUNCE_SECS-
            // Doc-Kommentar). Laeuft NACH step_flight/dem Approach-Buffer-
            // Push weiter oben in diesem Tick — Accident-Classifier hat
            // den rohen Wert also laengst gelesen, bevor wir ihn hier nur
            // fuer die Veroeffentlichung ueberschreiben.
            {
                let mut stats = flight.stats.lock().expect("flight stats");
                let now = Utc::now();
                if snap.stall_warning {
                    let since = *stats.stall_mqtt_true_since.get_or_insert(now);
                    snap.stall_warning =
                        (now - since).num_seconds() >= crate::grenzwerte::grenzwerte().cockpit.stall_warning_mqtt_debounce_secs.wert;
                } else {
                    stats.stall_mqtt_true_since = None;
                }
            }

            // v0.5.11: MQTT live-tracking publish (best-effort,
            // independent of phpVMS post). Ein hängender Broker darf
            // den Streamer NIE aufhalten.
            //
            // v1.5.7 (QS-Runde 4): Positionen laufen seither NICHT mehr
            // über die beschränkte mpsc, sondern über einen `watch`, der
            // nur den neuesten Wert hält (siehe aeroacars-mqtt: `Cmd` und
            // `should_publish_position`) — der alte Kommentar beschrieb
            // hier eine Mechanik, die es nicht mehr gibt. QoS 0 + retain
            // bleibt, damit die Live-Karte beim Verbinden sofort einen
            // Stand hat.
            {
                let app_state = app.state::<AppState>();
                let mqtt = app_state.mqtt.lock().await;
                if let Some(handle) = mqtt.as_ref() {
                    // v0.5.44: aircraft_icao mit Fallback-Cascade.
                    //
                    // flight.aircraft_icao kommt aus phpVMS GET /aircraft/<id>
                    // (siehe ActiveFlight-Init). Wenn der Pilot kein SimBrief
                    // OFP hat oder das OFP keinen aircraft_id mitbringt, ist
                    // das leer. Der Sim weiss aber das Aircraft (MSFS ATC
                    // MODEL SimVar / X-Plane DataRef) — wir extrahieren da
                    // den ICAO per Regex (z.B. "ATCCOM.AC_MODEL A321.0.text"
                    // → "A321"). Damit Live-Monitor + Heatmap auch ohne OFP
                    // den richtigen Type zeigen.
                    let resolved_icao = if !flight.aircraft_icao.trim().is_empty() {
                        flight.aircraft_icao.clone()
                    } else {
                        snap.aircraft_icao
                            .as_deref()
                            .and_then(extract_icao_code)
                            .unwrap_or_default()
                    };
                    // v1.5.5 Stand-Erkennung: erkannte Staende in jeden
                    // Position-Tick — Live-Map sieht den Ankunftsstand
                    // beim Einparken statt erst nach dem Filing.
                    let (tick_dep_gate, tick_arr_gate) = {
                        let stats = flight.stats.lock().expect("flight stats");
                        (stats.dep_gate.clone(), stats.arr_gate.clone())
                    };
                    let meta = aeroacars_mqtt::FlightMeta {
                        callsign: format_callsign(&flight.airline_icao, &flight.flight_number),
                        aircraft_icao: resolved_icao,
                        dep_icao: flight.dpt_airport.clone(),
                        arr_icao: flight.arr_airport.clone(),
                        // v0.5.19: phpVMS-side registration overrides
                        // sim's ATC-ID for the live-tracking stream.
                        planned_registration: flight.planned_registration.clone(),
                        // Spec sim-disconnect-auto-resume F4: pirep_id wird
                        // in jedem Position-Tick mitgegeben damit der Server
                        // Sessions ueber pirep_id joinen kann — verhindert
                        // den AUA-323-Split bei langer Positions-Luecke.
                        pirep_id: flight.pirep_id.clone(),
                        dep_gate: tick_dep_gate,
                        arr_gate: tick_arr_gate,
                    };
                    // v0.5.14: pass current phase so it's inlined into
                    // the position payload — Monitor doesn't have to
                    // wait for a separate phase-topic delivery.
                    let live_phase = phase_change.unwrap_or(prev_phase);
                    handle.position(&snap, &meta, live_phase);
                }
            }

            // v0.15.x: geflogenen Track im Backend bei voller Streamer-Tick-
            // Rate akkumulieren (fokus-/fenster-unabhängig → lückenlos auch bei
            // X-Plane-Vollbild). Das Frontend pollt ihn via `flight_get_track`.
            // KURZLEBIGER, SYNCHRONER Block — der std-Mutex-Guard ist nicht Send
            // und darf NICHT über ein `.await` gehalten werden (kein await im
            // Block; Guard fällt am Block-Ende). Ausdünnung 1:1 wie
            // client/src/lib/trackStore.ts (siehe record_track_point).
            {
                let mut stats = flight.stats.lock().expect("flight stats");
                record_track_point(&mut stats, snap.lon, snap.lat, snap.on_ground);
            }

            // v0.5.35: JSONL-Append jetzt PRO TICK (= 3s Cadence, oder
            // adaptive bis zu 500ms unter AGL<100ft). Vorher war der
            // Append nur INSIDE des phpVMS-OK-Branch — bei phpVMS-
            // Throttle (8-30s im Approach) verlor der Forensik-Log
            // 80% der Frames und der Touchdown-Moment fiel komplett
            // in eine Lücke. User-Report von gsg/2 GA-Flug 2026-05-08:
            // 0.1 Hz Sample-Rate, peak_vs_fpm=-33 fpm während realer TD
            // bei vermutlich -350 fpm war.
            let mut log_snap = snap.clone();
            log_snap.cockpit_rohwerte_nur_bei_aenderung(&mut letzte_cockpit_rohwerte);
            record_event(
                &app,
                &flight.pirep_id,
                &FlightLogEvent::Position {
                    timestamp: Utc::now(),
                    snapshot: log_snap,
                },
            );

            // v0.6.0/v0.6.1 — Position in Memory-Outbox pushen (Worker holt
            // sie ab). Ein simpler ~1µs Mutex-Push, blockiert NIE — der
            // phpVMS-Worker (TICK=1s + phase-aware POST-Cadence 4-30s, batch
            // bis 50 Items) bedient die Outbox entkoppelt. Bei Failure:
            // Worker queued den ganzen batch zurueck in die Outbox + bei
            // Backlog>=500 (mit 30s-Hysteresis) in `position_queue.json`.
            // v1.6.14 — nur noch ein Punkt je Phasen-Intervall an phpVMS.
            // Ein Phasenwechsel geht IMMER mit raus, damit Start, Landung
            // und Durchstarten auf der Karte an der richtigen Stelle sitzen
            // und nicht in ein Cruise-Intervall fallen.
            let phase_jetzt = {
                let stats = flight.stats.lock().expect("flight stats");
                stats.phase
            };
            if phpvms_punkt_faellig(
                phase_jetzt,
                letzter_phpvms_push.map(|t| t.elapsed()),
                zuletzt_gepushte_phase,
            ) {
                letzter_phpvms_push = Some(std::time::Instant::now());
                zuletzt_gepushte_phase = Some(phase_jetzt);
                let mut outbox = flight.position_outbox.lock().expect("position_outbox lock");
                outbox.push_back(position.clone());
                // Cap auf 5000. Vor v1.6.14 (ein Punkt je Tick) waren das ca.
                // 4 Stunden Cruise; seit der phasenabhaengigen Ausduennung
                // deckt derselbe Cap ein Vielfaches davon ab (im Reiseflug
                // ein Punkt je 60 s). Der Wert bleibt, er ist jetzt nur
                // grosszuegiger als er aussieht.
                // Wenn die Verbindung so lange tot ist: aelteste positions
                // werden aus dem Live-Stream gedroppt (Live-Map verliert
                // Anfang des Tracks), aber die JSONL-SoT hat sie alle —
                // Forensik-Upload nach PIREP-Filing zieht sie nach.
                let mut dropped_this_tick = 0u32;
                while outbox.len() > 5000 {
                    outbox.pop_front();
                    dropped_this_tick += 1;
                }
                // Nach dem Flugende-Stopp ist der Worker womoeglich schon
                // fort — dann gibt der Streamer den Punkt selbst ab, statt
                // ihn in einer Outbox liegen zu lassen, die niemand mehr leert.
                let nachsenden = outbox_zum_nachsenden(&flight, &mut outbox);
                drop(outbox);
                if let Some(rest) = nachsenden {
                    positionen_uebergeben(&flight, &client, rest);
                }
                if dropped_this_tick > 0 {
                    // Warn pro Tick — auf einem dauerhaft toten Netz koennte
                    // das jede Sekunde feuern, aber das ist genau das Signal
                    // das wir wollen: „Backlog ist voll, Daten gehen aus dem
                    // Live-Stream verloren". JSONL-Forensik bleibt komplett.
                    tracing::warn!(
                        pirep_id = %flight.pirep_id,
                        dropped_this_tick,
                        outbox_cap = 5000u32,
                        "outbox cap reached — oldest positions dropped from live-stream \
                         (JSONL forensics still complete; check phpVMS connectivity)"
                    );
                }
                // v0.6.1 — KEIN queued_position_count-Update hier!
                //
                // Semantik in v0.5.x: queued_position_count zeigte echten
                // BACKLOG (failed POSTs in der file-queue). Im normalen
                // Cruise = 0. Pilot interpretiert >0 als „Verbindungs-
                // Problem".
                //
                // Wenn wir hier outbox.len() schreiben würden: Streamer
                // pusht alle ~3s, Worker drained nur alle 30s im Cruise
                // -> outbox.len() ist 29 von 30 Sekunden > 0 -> Indikator
                // zeigt durchgehend „queued (offline)" obwohl alles ok.
                //
                // Stattdessen: nur der Worker setzt queued_position_count
                // im success-Pfad (= was nach Drain noch übrig ist) bzw.
                // failure-Pfad (= was nach Requeue drin steht). Im Cruise:
                // nach erfolgreichem Drain ist outbox leer -> count=0.
                // Bei Network-Down: Requeue lässt Backlog wachsen -> count
                // steigt korrekt.
            }

            // Periodically flush running stats to disk so a Tauri restart
            // (or crash) doesn't lose distance/fuel/phase data. The
            // identification fields are written every time too — cheap,
            // and keeps the file consistent if any of them changed via
            // backfill (airline_icao / planned_registration on resume).
            let should_persist = {
                let mut stats = flight.stats.lock().expect("flight stats");
                let due = stats.position_count % STATS_PERSIST_EVERY_TICKS == 0;
                if due && snapshot_plausibel(&snap) {
                    // v0.20 (Process-Integrity): refresh the "last known
                    // good state" independent of whether a SimDisconnect
                    // pause was ever registered — this is the comparison
                    // baseline `try_resume_flight()` uses after an app
                    // restart (see there: `last_persisted_snapshot.take()`).
                    stats.last_persisted_snapshot = Some(PausedSnapshot {
                        lat: snap.lat,
                        lon: snap.lon,
                        heading_deg: snap.heading_deg_true,
                        altitude_ft: snap.altitude_msl_ft,
                        fuel_total_kg: snap.fuel_total_kg,
                        zfw_kg: snap.zfw_kg,
                        on_ground: snap.on_ground,
                    });
                }
                due
            };
            // Re-check stop *before* writing — `flight_end` may have run
            // since the top-of-loop check, taken the flight, called
            // `clear_persisted_flight`, and we'd otherwise resurrect the
            // disk file. Without this, the resume banner pops on the next
            // launch for a flight the user already filed.
            takt_uhr.abschnitt("ereignisse");
            if should_persist && !flight.stop.load(Ordering::Relaxed) {
                save_active_flight(&app, &flight);
            }
            takt_uhr.abschnitt("persistieren");

            // On phase change, push the new status to phpVMS so the
            // live-map and PIREP detail page reflect the current phase.
            if let Some(new_phase) = phase_change {
                // Die ALTE Phasenlogik (v1) nur noch leise fuer die
                // Forensik. Ins Aktivitaetsprotokoll gehoert die Phase, die
                // phpVMS und die Karte auch sehen — siehe weiter oben bei
                // `effective_phase_changed` (Feldbefund 23.09.2026).
                tracing::debug!(
                    pirep_id = %flight.pirep_id,
                    v1_phase = ?new_phase,
                    "v1-Phasenwechsel"
                );
                // Diagnostic fuel/weight snapshot at the four phases
                // where the values get captured into PIREP fields.
                // Lets the pilot verify what the app saw — especially
                // useful with Fenix where TOTAL WEIGHT / ZFW / payload
                // sometimes report 0 and the pilot can't tell from
                // the PIREP alone whether the SimVar was missing or
                // the addon hadn't finished loading at capture time.
                log_fuel_weight_at_phase(&app, &flight, new_phase, &snap);
                record_event(
                    &app,
                    &flight.pirep_id,
                    &FlightLogEvent::PhaseChanged {
                        timestamp: Utc::now(),
                        from: prev_phase,
                        to: new_phase,
                        altitude_msl_ft: snap.altitude_msl_ft,
                        groundspeed_kt: snap.groundspeed_kt,
                        altitude_agl_ft: snap.altitude_agl_ft,
                    },
                );
                // METAR snapshot at the two phase transitions where it
                // matters most: just after takeoff for the departure
                // weather, and just before/at touchdown for the arrival
                // weather. Spawned async so a flaky aviationweather.gov
                // doesn't stall the streamer.
                maybe_spawn_metar_fetch(&app, &flight, new_phase);

                // v0.12.6: Backend-Auto-File. Latcht der FSM auf
                // `Arrived` und hat der Pilot Auto-File aktiviert, filet
                // das Backend den PIREP selbst — fenster-unabhängig.
                // Vorher lief Auto-File nur als Frontend-`useEffect`, der
                // bei Hintergrund-Fenster / nicht-aktivem Cockpit-Tab
                // nicht feuerte (Pilot-Befund: PIREP ging erst raus,
                // nachdem der Pilot AeroACARS in den Vordergrund holte).
                // Divert wird ausgelassen — den Ausweich-Airport muss der
                // Pilot über das Divert-Banner bestätigen (LE2).
                if new_phase == FlightPhase::Arrived {
                    auto_file_versuchen(&app, &flight, true);
                }
            }

            // Auch OHNE Phasenwechsel: Die Landebewertung wird erst ein paar
            // Sekunden nach dem Aufsetzen fertig. Wer nur den Uebergang nach
            // `Arrived` prueft, kann das Fenster treffen, in dem die Sperre
            // noch steht — und danach kommt kein zweiter Uebergang mehr
            // (Cloud-QS 23.09.2026). Der Merker in `auto_file_versuchen`
            // sorgt dafuer, dass trotzdem nur EIN Versuch laeuft.
            if phase_change.is_none() && current_phase == FlightPhase::Arrived {
                auto_file_versuchen(&app, &flight, false);
            }

            // Unified heartbeat-and-phase-update: POST `/pireps/{id}/update`
            // either when the phase just changed (so the live map sees the
            // new status immediately) OR after every `HEARTBEAT_INTERVAL`
            // (so phpVMS's RemoveExpiredLiveFlights cron never fires
            // mid-flight). Both purposes use the same payload — sending
            // monotonic flight_time / distance also guarantees the row is
            // dirty so Eloquent always writes UPDATE and bumps updated_at.
            let phase_for_heartbeat = phase_change.unwrap_or(current_phase);
            let due_for_heartbeat = last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL;
            // Der Arbeiter sendet immer den neuesten Stand; läuft gerade
            // ein Lebenszeichen, geht dieser hier danach raus.
            if phase_change.is_some() || due_for_heartbeat {
                let body = {
                    let stats = flight.stats.lock().expect("flight stats");
                    build_heartbeat_body(&snap, &stats, phase_for_heartbeat)
                };
                last_heartbeat = std::time::Instant::now();
                phpvms_meldungen.lebenszeichen(body, phase_for_heartbeat, phase_change.is_some());
            }

            // Mirror collected text events into phpVMS's `/acars/logs` for
            // the PIREP detail page. Best-effort: a transient network
            // error here doesn't matter — the activity log + recorder
            // already have the durable copy.
            if !acars_log_entries.is_empty() {
                phpvms_meldungen.acars(acars_log_entries);
            }
        }
        tracing::info!(pirep_id = %flight.pirep_id, "position streamer stopped");
    });
}
