/// On login or session restore, check the on-disk active-flight file. If it's
/// recent enough, recreate the in-memory ActiveFlight and restart position
/// streaming — picks up exactly where the previous run left off.
/// Ist der gespeicherte Flug frisch genug, um ihn von selbst aufzunehmen?
///
/// Zwei Dinge müssen stimmen, und sie beantworten verschiedene Fragen:
///   * Der **Server** sagt, ob es den Flug noch gibt und ob er läuft.
///   * Der **Stillstand** sagt, ob der Client noch daran gearbeitet hat.
///
/// Beide werden gebraucht. Nur den Server zu fragen reicht nicht: Wenn
/// dessen eigener Aufräumer klemmt — auf GSG tut er das, `cron:hourly` lief
/// in neun Stunden null Mal —, dann steht ein längst verlassener Flug dort
/// ewig auf IN_PROGRESS, und der Client nähme ihn beim nächsten Start
/// wortlos wieder auf.
///
/// ⚠ Bewusst NICHT die Flugdauer. Ein Langstreckenflug ist nicht tot, weil
/// er lange dauert — das war der Fehler, der THY77 gekostet hat.
///
/// Eigene Funktion, damit sie prüfbar ist: Die alte Grenze lebte mitten in
/// einer async-Funktion mit AppHandle und Client und war von keinem Test
/// erreichbar.
/// Wie lange der gespeicherte Flug schon nicht mehr angefasst wurde.
///
/// Eigene Funktion, weil die Rechnung sonst an drei Stellen steht (Resume,
/// Suche, Riegel-Abfrage) und dort auseinanderlaufen kann — die eine faellt
/// bei alten Dateien auf `started_at` zurueck, die andere vergisst es.
fn stillstand_von(p: &PersistedFlight) -> chrono::Duration {
    Utc::now() - p.zuletzt_geschrieben.unwrap_or(p.started_at)
}

fn stillstand_erlaubt_wiederaufnahme(stillstand: chrono::Duration) -> bool {
    stillstand <= chrono::Duration::hours(crate::grenzwerte::grenzwerte().abgabe.resume_stillstand_max_hours.wert)
}

async fn try_resume_flight(app: &AppHandle, state: &tauri::State<'_, AppState>) {
    let Some(mut persisted) = read_persisted_flight(app) else {
        return;
    };
    // ⚠ Hier wurde bis v1.7.8 nach der DAUER des Fluges entschieden — älter
    // als 12 h ab `started_at` hieß „verwerfen", samt Löschen der Datei.
    // Das hat THY77 gekostet (siehe `RESUME_STILLSTAND_MAX_HOURS`): ein
    // Langstreckenflug ist nicht deshalb tot, weil er lange dauert.
    //
    // Entschieden wird jetzt unten vom SERVER — er weiß es als Einziger
    // sicher, und gefragt wird er ohnehin. Vor dieser Auskunft wird nichts
    // mehr gelöscht.
    //
    // Der Stillstand seit dem letzten Schreiben ist trotzdem interessant:
    // Er ist der Notnagel für den Fall, dass der Server nicht antwortet.
    let stillstand = stillstand_von(&persisted);
    // Getrennt gefuehrt und bewusst NICHT vermischt: `flugalter` ist die Dauer
    // seit dem Abflug (geht als `age_minutes` ins JSONL, wo es diese Bedeutung
    // schon immer hatte), `stillstand` die Zeit seit der letzten
    // Aktualisierung. Nur Letzteres sagt etwas darueber, ob der Flug lebt.
    let flugalter = Utc::now() - persisted.started_at;

    // ⚠ Der Stillstand gilt IMMER, nicht nur wenn der Server schweigt.
    //
    // Erste Fassung dieser Reparatur prüfte ihn nur im Netzwerk-Fehlerzweig.
    // Damit war ein Fehler gegen einen anderen getauscht: Sagt der Server
    // „IN_PROGRESS" — und das tut er auf GSG bei jedem Geisterflug, weil
    // sein Aufräumer nicht läuft —, hätte der Client einen drei Tage alten,
    // längst aufgegebenen Flug wortlos weitergeführt.
    //
    // Gelöscht wird trotzdem nichts: Das entscheidet weiter unten allein
    // der Server. Hier wird nur nicht von selbst aufgenommen.
    if !stillstand_erlaubt_wiederaufnahme(stillstand) {
        tracing::info!(
            pirep_id = %persisted.pirep_id,
            stillstand_stunden = stillstand.num_hours(),
            "seit über 24 h keine Aktualisierung — keine automatische Wiederaufnahme"
        );
        log_activity_handle(
            app,
            ActivityLevel::Warn,
            "Gespeicherter Flug nicht automatisch aufgenommen".to_string(),
            Some(format!(
                "Der Flug wurde seit {} Stunden nicht aktualisiert und wird deshalb nicht \
                 von selbst fortgesetzt. Er bleibt gespeichert.",
                stillstand.num_hours()
            )),
        );
        return;
    }

    // Codex-Folgefund (adversarial, 05.09.2026, neunte Runde): der
    // Lifecycle-Lock (weiter unten frueher an dieser Stelle erworben) muss
    // VOR den beiden folgenden Server-Roundtrips (Eigentuemer-Reklamierung,
    // `get_pirep`) stehen, nicht erst danach. Sonst haette ein GENAU in
    // dieser Luecke gleichzeitig laufender `flight_start`/`flight_adopt`
    // (z. B. der Auto-Start-Watcher) den Lock zwischendurch bekommen, einen
    // neuen Flug angelegt und dessen `save_active_flight` den einzigen
    // Ablage-Platz ueberschrieben — der hier gerade geprüfte PIREP waere
    // danach lokal verwaist und serverseitig fuer immer IN_PROGRESS
    // steckengeblieben, ohne dass irgendein Client ihn je wieder aufgreift.
    let _setup_guard = match FlightSetupGuard::try_acquire(&state.flight_setup_in_progress) {
        Ok(g) => g,
        Err(_) => {
            tracing::debug!("resume already in progress, skipping duplicate call");
            return;
        }
    };

    // Codex-Folgefund (adversarial, 05.09.2026, fuenfte Runde): BEVOR der
    // PIREP unten direkt per ID abgefragt wird — das allein sagt nichts
    // darueber, WEM er gehoert — pruefen, ob die Ablage ueberhaupt zum
    // GERADE eingeloggten Account gehoert. `phpvms_logout` erlaubt
    // ausdruecklich, dass sich waehrend eines noch laufenden Fluges ein
    // ANDERER Pilot auf derselben Maschine anmeldet. Ohne diese Pruefung
    // haette ein 404 (falls das ID-Nachschlagen fuer den falschen Account
    // fehlschlaegt) Pilot As Ablage geloescht; ein Erfolg haette Pilot As
    // Flug unter Pilot Bs Session wiederaufgenommen.
    //
    // Codex-Folgefund (adversarial, 05.09.2026, siebte Runde): server-
    // verifizierte Piloten-ID statt eines aus dem API-Key abgeleiteten
    // Fingerabdrucks — `state.authenticated_pilot_id` ist zu diesem
    // Zeitpunkt bereits gesetzt (`phpvms_login`/`phpvms_load_session` tun
    // das VOR diesem Aufruf).
    //
    // Codex-Folgefund (adversarial, 05.09.2026, elfte Runde): `client` kam
    // bisher als Parameter vom AUFRUFER (`phpvms_login`/`phpvms_load_
    // session`), der ihn VOR dem Freigeben seines eigenen Lifecycle-Locks
    // erfasst hatte — die Identitaet hier wurde aber ERNEUT und GETRENNT
    // aus `state` gelesen. Zwischen dem `drop(setup_guard)` des Aufrufers
    // und dem Wiedererwerb hier (siehe oben) kann ein VOLLSTAENDIGER
    // Login eines anderen Piloten stattfinden — dann haette dieser Code
    // Pilot Bs frisch gelesene Identitaet mit Pilot As veraltetem,
    // uebergebenem `client` kombiniert und genau die Garantie umgangen,
    // die `aktuelle_session_atomar` eigentlich gibt. Jetzt werden Client
    // UND Identitaet gemeinsam, atomar und JETZT (nicht vom Aufrufer
    // ueberreicht) gelesen.
    let Some((client, aktuelle_pilot_id, _epoche)) = aktuelle_session_atomar(state) else {
        tracing::warn!(
            pirep_id = %persisted.pirep_id,
            "try_resume_flight: keine authentifizierte Sitzung bekannt — Resume uebersprungen"
        );
        return;
    };
    let aktuelle_identitaet = aktuelle_pilot_id.to_string();
    if !pirep_queue_eintrag_gehoert_aktuellem_piloten(
        persisted.owner_identity.as_deref(),
        &aktuelle_identitaet,
    ) {
        // Alt-Ablage (kein Eigentuemer gespeichert, vor diesem Feld) oder
        // inzwischen ein anderer eingeloggter Pilot — vor der Aufgabe
        // serverseitig pruefen, ob der PIREP TROTZDEM zum aktuellen Account
        // gehoert (`GET /api/user/pireps?state=0`, serverseitig auf den
        // eingeloggten Piloten gefiltert — derselbe Reklamier-Weg wie bei
        // `pirep_queue`, NICHT die direkte Nachfrage per ID weiter unten).
        let serverseitig_bestaetigt = client
            .get_user_pireps_in_progress()
            .await
            .map(|liste| liste.iter().any(|p| p.id == persisted.pirep_id))
            .unwrap_or(false);
        if serverseitig_bestaetigt {
            tracing::info!(
                pirep_id = %persisted.pirep_id,
                "persisted flight serverseitig als IN_PROGRESS des aktuellen Accounts \
                 bestaetigt — reklamiert (Alt-Ablage oder API-Key-Rotation)"
            );
            persisted.owner_identity = Some(aktuelle_identitaet.clone());
        } else {
            tracing::warn!(
                pirep_id = %persisted.pirep_id,
                eigentuemer_bekannt = persisted.owner_identity.is_some(),
                "persisted flight gehoert nicht dem aktuell eingeloggten Piloten — \
                 Resume uebersprungen, Ablage bleibt unangetastet"
            );
            return;
        }
    }

    // Verify the PIREP still exists server-side BEFORE rebuilding the
    // streamer. phpVMS's RemoveExpiredLiveFlights cron may have soft-deleted
    // it while the client was offline (heartbeat couldn't fire), in which
    // case adopting locally would just produce a wave of 404s. A 404 here
    // means the PIREP is gone — clear the disk record and let the user
    // re-prefile cleanly. Other errors (network, auth) are non-fatal —
    // we proceed and let the streamer's own error handling kick in.
    match client.get_pirep(&persisted.pirep_id).await {
        Ok(p) => {
            // State 0 = IN_PROGRESS. Anything else means the PIREP was
            // filed / cancelled / accepted elsewhere — there's nothing for
            // us to resume into.
            if p.state.is_some() && p.state != Some(0) {
                tracing::info!(
                    pirep_id = %persisted.pirep_id,
                    state = ?p.state,
                    status = ?p.status,
                    "persisted PIREP no longer in progress, discarding resume"
                );
                clear_persisted_flight(app, Some(&persisted.pirep_id));
                log_activity_handle(
                    app,
                    ActivityLevel::Warn,
                    "Gespeicherter Flug nicht mehr aktiv".to_string(),
                    Some(format!(
                        "PIREP {} hat serverseitig Status {:?} — Resume verworfen.",
                        persisted.pirep_id, p.status
                    )),
                );
                return;
            }
        }
        Err(ApiError::NotFound) => {
            tracing::info!(
                pirep_id = %persisted.pirep_id,
                "persisted PIREP no longer on server (likely soft-deleted by cron), discarding resume"
            );
            clear_persisted_flight(app, Some(&persisted.pirep_id));
            log_activity_handle(
                app,
                ActivityLevel::Warn,
                "Gespeicherter Flug existiert nicht mehr".to_string(),
                Some(
                    "phpVMS hat den PIREP entfernt (vermutlich Inaktivitäts-Timeout). \
                     Bitte den Flug neu starten oder als Manual-PIREP einreichen."
                        .to_string(),
                ),
            );
            return;
        }
        Err(e) => {
            // Server nicht erreichbar. Jetzt — und NUR jetzt — zählt der
            // Stillstand: Ohne Auskunft ist die Zeit seit dem letzten
            // Schreiben der beste Hinweis darauf, ob der Flug noch lebt.
            //
            // ⚠ Auch hier wird die Datei NICHT gelöscht. Ein Ausfall beim
            // Server darf einen laufenden Flug nicht kosten — beim nächsten
            // Start ist der Server vielleicht wieder da, und dann entscheidet
            // wieder er.
            tracing::warn!(
                pirep_id = %persisted.pirep_id,
                stillstand_stunden = stillstand.num_hours(),
                error = %e,
                "could not verify persisted PIREP — proceeding with resume anyway"
            );
        }
    }

    // Der Lifecycle-Lock wird seit Runde 9 bereits weiter oben gehalten
    // (vor der Eigentuemer-Reklamierung + `get_pirep`) — das deckt auch
    // den StrictMode-Doppelmount-Fall (zweiter `phpvms_load_session`-Aufruf
    // in schneller Folge) ab, ohne ihn hier ein zweites Mal erwerben zu
    // muessen (ein zweiter `try_acquire` waere hier immer fehlgeschlagen,
    // da der erste Aufruf den Lock ja noch haelt).
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if guard.is_some() {
            // Expected on second mount under React StrictMode (dev) — the
            // first mount already restored the flight. Idempotent, no WARN.
            tracing::debug!("active flight already in memory, skipping resume");
            return;
        }
    }

    // v0.20 (Process-Integrity): the setup hook already determined, before
    // writing THIS run's sentinel, whether the PREVIOUS run left one behind.
    let previous_exit_clean = *state
        .previous_run_exit_clean
        .lock()
        .expect("previous_run_exit_clean lock");
    tracing::info!(
        pirep_id = %persisted.pirep_id,
        age_minutes = flugalter.num_minutes(),
        stillstand_minuten = stillstand.num_minutes(),
        ?previous_exit_clean,
        "resuming in-progress flight"
    );
    record_event(
        app,
        &persisted.pirep_id,
        &FlightLogEvent::FlightResumed {
            timestamp: Utc::now(),
            pirep_id: persisted.pirep_id.clone(),
            age_minutes: flugalter.num_minutes(),
            previous_exit_clean,
        },
    );

    // Backfill missing airline ICAO + planned registration. PersistedFlight
    // defaults to "" when the JSON pre-dates these fields, or when the flight
    // was originally adopted from a discovered PIREP that didn't have a
    // matching bid. Try get_bids() once on resume; if nothing matches we
    // leave them empty and the UI renders without them — same as before.
    let mut airline_icao = persisted.airline_icao.clone();
    let mut planned_registration = persisted.planned_registration.clone();
    if airline_icao.is_empty() || planned_registration.is_empty() {
        if let Ok(bids) = client.get_bids().await {
            if let Some(matching) = bids
                .iter()
                .find(|b| b.flight.flight_number == persisted.flight_number)
            {
                if airline_icao.is_empty() {
                    if let Some(a) = matching.flight.airline.as_ref() {
                        airline_icao = a.icao.clone();
                    }
                }
                if planned_registration.is_empty() {
                    if let Some(id) = matching
                        .flight
                        .simbrief
                        .as_ref()
                        .and_then(|sb| sb.aircraft_id)
                    {
                        if let Ok(details) = client.get_aircraft(id).await {
                            planned_registration =
                                details.registration.unwrap_or_default().trim().to_string();
                        }
                    }
                }
            }
        }
    }

    // Restore stats so the resumed flight continues with the distance,
    // fuel-burn and phase the streamer had right before the crash.
    // Without this, every resume produces a "0 distance / 0 fuel" PIREP
    // because we'd start the stats from zero again.
    let mut restored_stats = FlightStats::new();
    // Nachgereicht (externe Gegenpruefung Codex, adversarial, 15 Runden
    // Vorgeschichte in docs/qs/v1.7.17-pruefstand.md, Runden 17-19): VOR
    // `apply_to` pruefen — die Funktion selbst loggt nur eine
    // `tracing::warn!`-Zeile (nie sichtbar fuer den Piloten) und setzt das
    // Flag danach zurueck. Eine formale Reconciliation (Server-Read-Back)
    // ist bewusst nicht gebaut (siehe Pruefstand-Dokument fuer die
    // Begruendung: kein bestehender phpVMS-Endpoint liefert Custom-Field-
    // Werte, `get_pirep`/`PirepFull` lassen sie bewusst aus) — aber die
    // Ungewissheit darf nicht laenger UNSICHTBAR bleiben, nur weil sie
    // technisch nicht automatisch aufloesbar ist.
    let dep_gate_zustellung_unklar = persisted.stats.dep_gate_withdrawal_awaiting_confirmation;
    persisted.stats.clone().apply_to(&mut restored_stats);
    if dep_gate_zustellung_unklar {
        log_activity_handle(
            app,
            ActivityLevel::Warn,
            "Abflug-Stand: Zustellung nach Neustart unklar".to_string(),
            Some(format!(
                "Vor dem Neustart wurde ein veralteter \"Departure Gate\"-Wert eventuell schon \
                 an phpVMS gemeldet — ob die Meldung ankam, laesst sich nach dem Neustart nicht \
                 mehr feststellen. Falls der Abflug-Stand im PIREP {} nicht (mehr) stimmt, bitte \
                 manuell pruefen oder beim VA-Team melden.",
                persisted.pirep_id,
            )),
        );
    }
    // Kabinenansagen (Beta): Flug aus einer Version ohne Passagierzahl.
    if crate::kabinenansagen::einstellungen(app).an {
        kabinen_eckdaten_nachholen(&client, persisted.bid_id, &mut restored_stats).await;
    }
    // v0.20 (Process-Integrity): overwrite with THIS resume's freshly-
    // determined value — `apply_to` just restored whatever an OLDER resume
    // (if any) had persisted, which says nothing about the restart that
    // just happened.
    restored_stats.app_restart_was_unclean = previous_exit_clean.map(|clean| !clean);
    // ⚠ Hier stand `age` — also die FLUGDAUER, obwohl das Feld "gap" heisst
    // und der Gesundheitsbericht es als Unterbrechung liest. Bei einem
    // Langstreckenflug meldete es 760 Minuten Luecke, wo der Client 4 Minuten
    // weg war. Jetzt steht dort die echte Unterbrechung.
    restored_stats.resume_gap_minutes = Some(stillstand.num_minutes());
    // Sekundengenau und NUR wenn die Datei sagt, wann sie zuletzt angefasst
    // wurde: Faellt `zuletzt_geschrieben` weg, ist `stillstand` die FLUGDAUER
    // — mit der als Luecke waere jede Strecke fliegbar, und die Pruefung
    // faende gar nichts mehr (Cloud-QS 23.09.2026).
    restored_stats.resume_luecke_secs = persisted
        .zuletzt_geschrieben
        .map(|z| (Utc::now() - z).num_seconds().max(0));
    restored_stats.resume_wiederhergestellt_am = Some(Utc::now());
    // Fuer die Abgabe zaehlt die LAENGSTE Luecke, nicht die letzte.
    // Ohne `zuletzt_geschrieben` die Minuten als Rueckfall — lieber einmal
    // zu oft gefragt als einen Absturz vergessen.
    let diese_luecke = restored_stats
        .resume_luecke_secs
        .or(Some(stillstand.num_seconds().max(0)));
    restored_stats.resume_luecke_max_secs =
        match (restored_stats.resume_luecke_max_secs, diese_luecke) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    tracing::info!(
        distance_nm = restored_stats.distance_nm,
        position_count = restored_stats.position_count,
        ?restored_stats.phase,
        "restored flight stats from disk"
    );

    // v0.15.25: reseed the in-app live-map track from phpVMS — the server holds
    // the authoritative SUPERSET of every position we ever posted. A mid-flight
    // app update/restart starts `FlightStats.track` empty (and a stale
    // localStorage seed-gate in the UI could hide a clean backend track), so we
    // replace the locally-persisted track with the complete server track here.
    // This runs inside the already-async fn, on a local `&mut` (no lock held
    // across the await), and strictly BEFORE `restored_stats` is moved into the
    // ActiveFlight Arc below — so there's no race with the streamer.
    match client.get_acars_positions(&persisted.pirep_id).await {
        Ok(pts) if !pts.is_empty() => {
            restored_stats.track.clear();
            for p in &pts {
                // Skip null-coordinate rows (lat/lon nullable in phpVMS).
                // on_ground=false (AIR threshold): the historical replay has no
                // reliable ground state, and the coarser AIR threshold keeps the
                // reseeded track light.
                if let (Some(lon), Some(lat)) = (p.lon, p.lat) {
                    record_track_point(&mut restored_stats, lon, lat, false);
                }
            }
            tracing::info!(
                pirep_id = %persisted.pirep_id,
                raw = pts.len(),
                thinned = restored_stats.track.len(),
                "seeded live-map track from server on resume"
            );
        }
        Ok(_) => {} // server has no rows yet → keep the persisted track from apply_to
        Err(e) => {
            tracing::warn!(
                pirep_id = %persisted.pirep_id,
                error = %e,
                "server track fetch failed — keeping local persisted track"
            );
        }
    }

    let flight = Arc::new(ActiveFlight {
        pirep_id: persisted.pirep_id.clone(),
        bid_id: persisted.bid_id,
        // v0.7.7: flight_id aus PersistedFlight wieder herholen. Bei
        // pre-v0.7.7-Resume-Files ist das Feld leer (String::new() via
        // serde(default)) — v0.7.8 muss diesen Fall als "legacy active
        // flight, direct refresh unavailable" behandeln.
        flight_id: persisted.flight_id.clone(),
        // v0.7.8 v1.5: Callsign-Felder aus PersistedFlight. None bei
        // pre-v0.7.8-Resume — Match faellt auf airline+flight_number
        // zurueck.
        bid_callsign: persisted.bid_callsign.clone(),
        pilot_callsign: persisted.pilot_callsign.clone(),
        started_at: persisted.started_at,
        airline_icao,
        // Resume-Pfad: Airline-Logo-URL aus der persisted Bid wieder
        // herholen. Wenn die Persisted-Snapshot-Version das Feld nicht
        // hatte (alter Build vor v0.4.0), bleibt None.
        airline_logo_url: persisted.airline_logo_url.clone(),
        planned_registration,
        // v0.3.0: Aircraft-Type bei Resume aus dem persisted Flight ist
        // nicht direkt verfügbar — der frühere Stand hatte das nicht.
        // Beim Resume bleibt der Custom-Field "Aircraft Type" daher
        // leer. Akzeptabel, weil Resume eh nur passiert wenn der
        // Pilot mitten im Flug die App neu startet — der ursprüngliche
        // PIREP hat die Aircraft-Daten phpVMS-seitig schon.
        aircraft_icao: String::new(),
        aircraft_name: String::new(),
        flight_number: persisted.flight_number.clone(),
        // v1.7.32: Plandistanz aus der Resume-Datei — sonst faellt die
        // Mehrweg-Gutschrift nach einem Neustart mitten im Flug weg.
        plan_strecke_nm: persisted.plan_strecke_nm,
        dpt_airport: persisted.dpt_airport.clone(),
        arr_airport: persisted.arr_airport.clone(),
        planned_flight_time_min: None, /* Resume: Planzeit nicht persistiert */
        fares: persisted.fares.clone(),
        stats: stats_sperre::GemesseneSperre::new(restored_stats),
        stop: AtomicBool::new(false),
        // Flag the UI to surface a 10-second confirmation banner before any
        // position posts go out. The streamer is intentionally NOT spawned
        // here — `flight_resume_confirm` does that once the user accepts (or
        // the timer expires), or `flight_cancel` aborts the PIREP.
        was_just_resumed: AtomicBool::new(true),
        streamer_spawned: AtomicBool::new(false),
        cancelled_remotely: AtomicBool::new(false),
        position_outbox: Mutex::new(std::collections::VecDeque::new()),
        positionen_nachsenden: AtomicBool::new(false),
        generation: std::sync::atomic::AtomicU64::new(0),
        start_sperre: Mutex::new(()),
        phpvms_worker_spawned: AtomicBool::new(false),
        touchdown_sampler_spawned: AtomicBool::new(false),
        connection_state: std::sync::atomic::AtomicU8::new(CONN_STATE_LIVE),
        // v0.8.0: Navdata-Cache startet leer — wird von
        // `fetch_navdata_for_flight` (Slice B) parallel beim Flugstart
        // populiert. Leer = OurAirports-Fallback aktiv (= Verhalten
        // vor v0.8.0).
        navdata: Mutex::new(NavdataCache::default()),
    });

    // v0.8.0: vor dem move den Navdata-Fetch spawnen.
    // Resume-Pfad: Alternate kommt aus restored_stats.planned_alternate
    // (= via persisted JSON wiederhergestellt). Bei pre-v0.8.0-Resume-
    // Files ohne planned_alternate bleibt es bei dep+arr.
    {
        // ⚠ Vor dem ersten Anmelden: Der neue Flug erbt sonst den
        // Anfragezustand des vorigen.
        szenerie_buch_zuruecksetzen(&app);
        let mut icaos = vec![flight.dpt_airport.clone(), flight.arr_airport.clone()];
        let alt = flight
            .stats
            .lock()
            .expect("flight stats lock")
            .planned_alternate
            .clone();
        if let Some(a) = alt {
            if !a.trim().is_empty() {
                icaos.push(a);
            }
        }
        spawn_navdata_fetch(app, &flight, icaos);
    }
    // v1.7.42: Traegt der wiederhergestellte Flug keine Sprit-Planwerte je
    // Wegpunkt, den OFP einmal nachladen.
    //
    // `sprit_an_bord_kg` kam erst mit v1.7.40. Ein Flug, der VOR dem Update
    // begann, hat seine Wegpunkte ohne dieses Feld gespeichert — und die
    // Tabelle „Sprit · Wegpunkt für Wegpunkt" blendet sich komplett aus,
    // wenn kein einziger Wegpunkt einen Planwert traegt. Der Pilot sah im
    // Cockpit eine leere Stelle und musste selbst auf die Idee kommen, den
    // Plan neu zu laden (Thomas, 20.09.2026, X-Plane-Flug).
    //
    // Best effort: Ohne Netz oder ohne OFP bleibt es, wie es war — der
    // naechste Flug bringt die Werte ohnehin mit.
    //
    // 2.0.16: Dasselbe fuer „Zeiten gegen Flugplan" — siehe
    // `ofp_nachladen_noetig`.
    let ofp_nachladen = {
        let st = flight.stats.lock().expect("flight stats lock");
        ofp_nachladen_noetig(&st)
    };
    {
        let mut guard = state.active_flight.lock().expect("active_flight lock");
        *guard = Some(flight);
    }
    if ofp_nachladen {
        let app_fuer_ofp = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app_fuer_ofp.state::<AppState>();
            // NUR die Route, nicht den ganzen Plan.
            //
            // `flight_refresh_simbrief` hat ein Phasen-Gate und bricht ab
            // Steigflug mit `phase_locked` ab — also ausgerechnet in dem
            // Fall, fuer den das hier gebaut ist (Abnahme 20.09.2026).
            // `flight_refresh_route_only` kennt dieses Gate nicht und
            // fasst ausdruecklich KEIN Score-Feld an: kein Block-Fuel,
            // kein ZFW/TOW/LDW, keine OFP-Kennung. Es setzt Route und
            // Wegpunkte — genau das, was hier fehlt.
            match flight_refresh_route_only(app_fuer_ofp.clone(), state).await {
                Ok(_) => {
                    tracing::info!(
                        "Sprit-Planwerte oder Planzeiten fehlten nach dem Neustart — OFP nachgeladen"
                    )
                }
                Err(e) => tracing::warn!(
                    error = ?e,
                    "Sprit-Planwerte oder Planzeiten fehlen und der OFP liess sich nicht nachladen"
                ),
            }
        });
    }
    // Codex-Folgefund (adversarial, 05.09.2026, fuenfte Runde): dieselbe
    // Eigentuemer-Erfassung wie in `flight_start`/`flight_adopt` — an
    // dieser Stelle ist bereits sichergestellt (Eigentuemer-Pruefung weiter
    // oben in dieser Funktion), dass die Ablage dem GERADE eingeloggten
    // Account gehoert.
    *state
        .active_flight_owner_identity
        .lock()
        .expect("active_flight_owner_identity lock") = Some(aktuelle_identitaet);
    // NOTE: do not spawn the streamer here. The frontend-driven
    // `flight_resume_confirm` does it after the resume banner is dismissed.
}

// ---- Bootstrap ----

// ---- Auto-start watcher ----
//
// Polls every AUTO_START_INTERVAL_SECS while enabled. When the
// aircraft is parked at the departure airport of one of the user's
// bids AND the loaded aircraft type/registration matches the bid,
// it fires `flight_start` for that bid. Stops re-firing for the
// same bid until the resulting flight ends (tracked via
// `AppState::auto_start_last_bid_id`).
// Wert seit v3: bundle/grenzwerte/cockpit.json -> auto_start_interval_secs (v2.1.24: 3)
// v0.15.13: Cooldown nach einem fehlgeschlagenen Auto-Start-`flight_start`.
// Bevor derselbe Bid erneut gefeuert wird, müssen so viele Sekunden vergehen.
// Stoppt den 3-s-Retry-Sturm (GlitchTip #10), bleibt aber selbst-korrigierend.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> auto_start_fail_cooldown_secs (v2.1.24: 120)
// How close to the departure airport (in meters) the aircraft must
// be to trigger auto-start. 5 km is generous enough for any major
// airport's stand area; tighter and we'd miss bids where the pilot
// spawns at a distant gate.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> auto_start_proximity_m (v2.1.24: 5_000.0)

/// Das Flugzeug, mit dem ein Bid geflogen wird. phpVMS haengt es erst mit
/// dem SimBrief-OFP an den Bid; vorher gibt es keins.
///
/// `flight_start` braucht es zwingend, und der Auto-Start-Watcher fragt
/// genau diese Stelle, bevor er feuert. Beide lesen hier, damit sie nicht
/// auseinanderlaufen koennen. Vorher feuerte der Watcher Bids ohne OFP,
/// `flight_start` lehnte mit `missing_aircraft` ab, und nach 120 s Pause
/// ging es von vorn (21.09.2026, Pilot 22: dreimal an einem Nachmittag,
/// 377 GlitchTip-Meldungen seit Juni).
fn bid_flugzeug_id(bid: &Bid) -> Option<i64> {
    bid.flight.simbrief.as_ref().and_then(|sb| sb.aircraft_id)
}

/// Laeuft fuer diesen Bid noch die Pause nach einem gescheiterten
/// Auto-Start? Dann `(Restsekunden, Fehlercode des letzten Versuchs)`.
fn auto_start_pause_rest(
    bid_id: i64,
    fehlschlag: Option<&(DateTime<Utc>, i64, String)>,
    jetzt: DateTime<Utc>,
) -> Option<(i64, String)> {
    let (am, fbid, code) = fehlschlag?;
    if *fbid != bid_id {
        return None;
    }
    let rest = crate::grenzwerte::grenzwerte().cockpit.auto_start_fail_cooldown_secs.wert - (jetzt - *am).num_seconds();
    (rest > 0).then(|| (rest, code.clone()))
}

/// Was der Watcher in einem Tick ueber die Bids erfahren hat, die er NICHT
/// gestartet hat. Daraus waehlt `auto_start_hinweis` den einen Hinweis.
#[derive(Debug, Default)]
struct AutoStartBeobachtung {
    /// (Flugnummer, erwartete ICAO, Sim-ICAO): am Platz, Typ passt nicht.
    typ_passt_nicht: Option<(String, String, String)>,
    /// Flugnummer eines Bids am Platz, an dem noch kein Flugzeug haengt.
    ohne_ofp: Option<String>,
    /// (Bid, Restsekunden, Fehlercode): Pause nach gescheitertem Versuch.
    pause: Option<(i64, i64, String)>,
    /// Bid, der in dieser Parkphase schon gestartet wurde.
    schon_gestartet: Option<i64>,
    /// Bid, dessen Auto-Start-`flight_start` gerade laeuft
    /// (`AppState::auto_start_laeuft_bid`). Ist es derselbe wie
    /// `schon_gestartet`, heisst das „wird gerade gestartet".
    start_laeuft_bid: Option<i64>,
    /// Naechster Abflughafen in nm.
    naechster_nm: Option<f64>,
}

/// Der eine Hinweis `(Code, Titel, Text)`, den der Pilot sieht, wenn der
/// Watcher nicht gefeuert hat — vom konkretesten Grund zum allgemeinsten.
/// Die Codes sind zugleich die Schluessel `bids.auto_start_skip.<code>`
/// im Banner.
fn auto_start_hinweis(b: &AutoStartBeobachtung) -> (&'static str, &'static str, String) {
    // Der Start dieses Bids laeuft gerade: das ist jetzt die einzige
    // wichtige Auskunft. Vorher stand in diesen rund fuenf Sekunden „Bid N
    // wurde diese Session schon mal auto-gestartet" — technisch richtig,
    // gelesen wie ein Fehler (Thomas, 22.09.2026, Log AIB424).
    if let Some(bid) = b
        .schon_gestartet
        .filter(|id| b.start_laeuft_bid == Some(*id))
    {
        return (
            "start_running",
            "Auto-Start: Start läuft",
            format!("Bid {bid} wird gerade gestartet — der Flug wird angelegt."),
        );
    }
    if let Some((flug, erwartet, sim)) = &b.typ_passt_nicht {
        return (
            "aircraft_mismatch",
            "Auto-Start: Flugzeug passt nicht",
            format!(
                "Bid {flug} erwartet {erwartet}, im Sim ist {sim} geladen. \
                 Auto-Start greift hier nicht — bitte manuell „Trotzdem starten\" \
                 (Wetlease) oder das passende Flugzeug laden. Tipp: passt der Typ \
                 eigentlich (nur anderer Name)? Dann fehlt ein Aircraft-Type-Alias \
                 auf dem VPS."
            ),
        );
    }
    if let Some(flug) = &b.ohne_ofp {
        return (
            "ofp_missing",
            "Auto-Start: wartet auf SimBrief-OFP",
            format!(
                "Bid {flug} hat noch kein Flugzeug — phpVMS haengt es erst mit dem \
                 SimBrief-OFP an. Sobald das OFP erstellt ist, startet der Flug von selbst."
            ),
        );
    }
    if let Some((bid, rest, code)) = &b.pause {
        return (
            "retry_pending",
            "Auto-Start: neuer Versuch gleich",
            format!(
                "Der letzte Start von Bid {bid} scheiterte ({code}). Neuer Versuch in \
                 {rest} s — oder manuell „Flug starten\"."
            ),
        );
    }
    if let Some(bid) = b.schon_gestartet {
        return (
            "bid_already_started",
            "Auto-Start: kein Match",
            format!(
                "Bid {bid} wurde diese Session schon mal auto-gestartet. \
                 Neu-Starten via Manual-Toggle oder anderem Bid."
            ),
        );
    }
    let abstand = b
        .naechster_nm
        .map(|nm| format!("{:.1} nm zum naechsten Departure-Airport", nm))
        .unwrap_or_else(|| "kein Departure-Airport in Reichweite".to_string());
    (
        "no_bid_match",
        "Auto-Start: kein Match",
        format!(
            "Kein Bid passt zur aktuellen Position ({abstand}). Auto-Start \
             greift nur am Stand des Departure-Airports."
        ),
    )
}

// So lange muss der Simulator ruhig laufen, bevor der Auto-Start ihn
// ueberhaupt bewertet.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> auto_start_sim_ruhe_secs (v2.1.24: 20)
/// Hinweis-Code, solange der Simulator noch nicht zur Ruhe gekommen ist
/// (Banner-Schluessel `bids.auto_start_skip.sim_not_ready`).
const AUTO_START_SIM_WARTET: &str = "sim_not_ready";

/// Fehlercode von `FlightSetupGuard::try_acquire`: ein anderer Start (meist
/// der Pilot per Hand) haelt gerade die Sperre.
const FLUGSTART_SPERRE_BELEGT: &str = "flight_setup_in_progress";

/// Fehler eines Auto-Starts, die kein Fehler sind: Der Watcher gibt den
/// Anspruch still frei, ohne Warnung, ohne GlitchTip, ohne Cooldown.
///
/// Log-Durchsicht 26.09.2026 (P2, 25.09. 13:46:12): Ein Handstart haelt die
/// Sperre ~9 s, bevor `active_flight` gesetzt ist. Ein Auto-Start in dieses
/// Fenster lief in `flight_setup_in_progress`, meldete WARN „Auto-Start:
/// flight_start fehlgeschlagen" (→ GlitchTip) und sperrte 120 s — obwohl der
/// Flug gerade per Hand startete.
fn auto_start_fehler_ist_kein_fehler(code: &str) -> bool {
    code == AUTO_START_SIM_WARTET || code == FLUGSTART_SPERRE_BELEGT
}

#[cfg(test)]
#[path = "tests_lib/auto_start_sperre_tests.rs"]
mod auto_start_sperre_tests;
// Weiter als das zwischen zwei Takten (3 s) ist kein Rollen, sondern
// Laden, Teleport oder Flughafenwechsel.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> auto_start_sprung_m (v2.1.24: 1_000.0)

/// Raeumt den Wartegrund des Resume-Gates weg, egal wie das Gate endet —
/// scharfgeschaltet, Flug abgebrochen, nicht mehr aktiv, oder ein Ausgang,
/// den es heute noch nicht gibt. Vorher stand das Raeumen einzeln an jedem
/// `return`; ein vergessener haette im Cockpit einen eingefrorenen Hinweis
/// hinterlassen (Cloud-QS 24.09.2026).
struct WartegrundRaeumen(Arc<ActiveFlight>);

impl Drop for WartegrundRaeumen {
    fn drop(&mut self) {
        if let Ok(mut st) = self.0.stats.lock() {
            st.resume_wartet = None;
            st.resume_gate_laeuft = false;
        }
    }
}

/// Schlaegt die Wartezeit im Resume-Gate auf die Neustart-Luecke auf.
///
/// Zwei Werte, zwei Zwecke:
///   * `resume_luecke_max_secs` dient der Abgabe-Sperre („war der Flug
///     unterbrochen?"). Dort zaehlt die GANZE Zeit ohne Aufzeichnung — auch
///     Menue und Laden.
///   * `resume_luecke_secs` dient der Sprung-Pruefung („war die Strecke in
///     der Zeit fliegbar?"). Im Menue bewegt sich aber nichts. Bekaeme sie
///     die ganze Wartezeit, galt ein Pilot, der sich nach sechs Minuten
///     Laden 3 nm vor die Bahn stellt, als ehrlich geflogen (Claude-QS
///     24.09.2026). Deshalb nur die ruhige Endphase, in der der Sim lief.
/// Eine unbekannte Luecke (Altdatei) bleibt unbekannt.
fn luecke_aufschlagen(
    st: &mut FlightStats,
    wiederhergestellt: Option<DateTime<Utc>>,
    jetzt: DateTime<Utc>,
) {
    let Some(seit) = wiederhergestellt else {
        return;
    };
    let gewartet = (jetzt - seit).num_seconds().max(0);
    // Fuer die Abgabe-Sperre zaehlt die Wartezeit IMMER. Ist die Luecke bis
    // zum Wiederherstellen unbekannt (Altdatei), steckt ihr Rueckfallwert
    // schon im Hoechstwert — darauf aufschlagen, lieber einmal zu oft fragen
    // (Codex-QS 24.09.2026: 60 s Rueckfall + 90 s Warten sind 150 s).
    let bisher_fuer_sperre = st
        .resume_luecke_secs
        .or(st.resume_luecke_max_secs)
        .unwrap_or(0);
    st.resume_luecke_max_secs = Some(
        st.resume_luecke_max_secs
            .unwrap_or(0)
            .max(bisher_fuer_sperre + gewartet),
    );
    // Die Sprung-Pruefung bekommt nur eine BEKANNTE Luecke verlaengert, und
    // nur um die ruhige Endphase.
    if let Some(bisher) = st.resume_luecke_secs {
        st.resume_luecke_secs = Some(bisher + gewartet.min(crate::grenzwerte::grenzwerte().cockpit.auto_start_sim_ruhe_secs.wert + 10));
    }
}

/// Ist das nach einem Aufsetzen ein ECHTER Steigflug (Durchstart,
/// Touch-and-Go) — oder nur ein Messwert, der einen vortaeuscht?
///
/// In der Luft, mehr als 100 ft ueber Grund, und mit einem AGL, das zur
/// Hoehe passt. MSFS meldete am 03.05.2026 waehrend eines GSX-Pushbacks
/// AGL = 53.819 ft (PMDG 737); nach einem Absturz lieferte das Menue
/// AGL 210 ft bei 20.926.040 ft MSL (OCN 712). Ohne diese Pruefung loeschte
/// so ein Wert nach der Landung den Landesprit (Codex-QS 24.09.2026).
fn echter_steigflug_nach_aufsetzen(snap: &SimSnapshot) -> bool {
    let agl = snap.altitude_agl_ft;
    // Zuerst der zentrale Filter: Ein Wert mit unmoeglicher MSL-Hoehe
    // (20.926.040 ft) ist gar keine Messung.
    //
    // Dann die Gelaendehoehe, die sich aus MSL - AGL ergibt: Sie muss es auf
    // der Erde geben, vom Toten Meer bis knapp ueber den Everest. Die erste
    // Fassung pruefte nur eine Richtung — 100.000 ft MSL mit 210 ft AGL
    // (Gelaende auf 99.790 ft) ging noch als Steigflug durch (Codex-QS
    // 24.09.2026).
    let gelaende_ft = snap.altitude_msl_ft - agl;
    snapshot_position_is_usable(snap)
        && !snap.on_ground
        && agl.is_finite()
        && agl > 100.0
        && (-2_000.0..=30_000.0).contains(&gelaende_ft)
}

/// Kennzeichen der blossen Ruhe-Zaehlstand-Meldung. Eine Konstante, damit
/// das Gate sie beim Protokollieren sicher erkennt — ueber einen zweimal
/// geschriebenen Text waere die Kopplung beim ersten Umformulieren gerissen.
const SIM_RUHE_ZAEHLSTAND: &str = "ruhig — geprüft wird nach";

/// Eine Runde des Resume-Gates: Darf der Flug jetzt scharf werden?
///
/// `Ok(snap)` = ja, mit dem Wert, der als Ausgangslage gilt. `Err(grund)` =
/// noch nicht, mit dem Grund fuer den Piloten. Reine Funktion, damit sich
/// die Folge eines echten Neustarts durchspielen laesst (OCN 712: Menue mit
/// anderem Flugzeug und 0 kg, dann ein Wert mit 20.926.040 ft, dann erst der
/// geladene Flug).
///
/// Die Ruhe waechst NUR mit frischen, brauchbaren Werten; alles andere setzt
/// sie zurueck. Sonst saehe ein im Menue eingefrorener Wert ruhig aus, und
/// der erste frische Wert danach schaltete sofort scharf.
///
/// ⚠ Die Frische prueft nur bei MSFS etwas: X-Plane stempelt jeden Abruf
/// neu (siehe `SimRuhe`). Dort schuetzen nur Filter und Ruhe-Regel.
fn resume_gate_takt(
    ruhe: &mut SimRuhe,
    snap: Option<SimSnapshot>,
    jetzt: DateTime<Utc>,
) -> Result<SimSnapshot, String> {
    // Eine ruhig stehende Menue-Scheinposition (LAT 0 / LON 90) ist kein
    // geladener Sim. Das Gate schaltete sonst darauf scharf — auch bei
    // „Trotzdem fortsetzen" —, und der erste echte Wert (GAF 278: Zagreb am
    // Boden) lief ungeprueft in die Phasen.
    let Some(snap) = snap
        .filter(snapshot_position_is_usable)
        .filter(snapshot_plausibel)
    else {
        *ruhe = SimRuhe::default();
        return Err("Der Simulator lädt noch — es gibt keine brauchbare Position.".to_string());
    };
    if (jetzt - snap.timestamp).num_seconds().abs() > SIM_GATE_FRESH_SECS {
        *ruhe = SimRuhe::default();
        return Err("Der Simulator liefert keine frischen Werte.".to_string());
    }
    match ruhe.pruefen(&snap, jetzt) {
        Some(grund) => Err(grund),
        None => Ok(snap),
    }
}

/// Ist der Simulator zur Ruhe gekommen?
///
/// Beim Laden liefert MSFS minutenlang Zwischenstaende: Weltkarte
/// (in der Luft), Teleports (1526 → 4432 → 6724 nm vom Abflughafen),
/// halb geladenes Flugzeug (Sprit 0, Triebwerke an). Der Watcher
/// bewertete jeden davon, der Hinweis sprang im Sekundentakt, und am
/// 19.09.2026 feuerte der Auto-Start 0,1 s nach einem Pause-Event mitten
/// im Ladebildschirm (Log Thomas, A380 VABB). Jetzt gilt: erst wenn der
/// Sim `AUTO_START_SIM_RUHE_SECS` lang nicht pausiert ist, nicht springt
/// und dasselbe Flugzeug zeigt, schaut der Auto-Start hin.
///
/// Bewusst OHNE Zeitstempel-Frische: X-Plane stempelt jeden Abruf neu,
/// MSFS nicht verlaesslich — ein Frische-Kriterium koennte den Auto-Start
/// dauerhaft sperren.
#[derive(Debug, Default)]
struct SimRuhe {
    seit: Option<DateTime<Utc>>,
    letzte_pos: Option<(f64, f64)>,
    letzter_titel: Option<String>,
    /// Wann die letzte Position gemessen wurde — fuer die Strecke, die ein
    /// fliegendes Flugzeug dazwischen ehrlich zuruecklegt.
    letzte_zeit: Option<DateTime<Utc>>,
    /// Wie weit sich das Flugzeug im vorigen Takt bewegt hat.
    letzter_abstand_m: Option<f64>,
}

impl SimRuhe {
    /// `None` = bereit. Sonst der Grund fuer den Piloten.
    fn pruefen(&mut self, snap: &SimSnapshot, jetzt: DateTime<Utc>) -> Option<String> {
        let titel = snap
            .aircraft_title
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string);
        // Am Boden gilt die feste Schwelle. In der Luft (Resume-Gate nach
        // einem App-Neustart mitten im Flug) legt ein Flugzeug zwischen zwei
        // Messungen ehrlich mehr zurueck — eine Concorde in 2 s ueber 1 km.
        // Ohne diese Reserve wuerde das Gate dort nie scharfschalten
        // (24.09.2026). Doppelte Strecke als Spielraum fuer Messabstaende.
        //
        // Die Sim-Rate zaehlt mit: Bei 4x Zeitbeschleunigung legt das
        // Flugzeug in derselben Echtzeit viermal so viel zurueck. Ohne sie
        // schaltete das Gate nach einem Neustart unter Zeitbeschleunigung
        // NIE scharf (Cloud-QS 24.09.2026). Die Geschwindigkeit ist gedeckelt
        // wie in der Sprung-Pruefung: Ein Ladewert mit unsinniger GS darf
        // keinen Teleport als ruhig durchwinken.
        let sekunden = self
            .letzte_zeit
            .map(|t| (jetzt - t).num_milliseconds().max(0) as f64 / 1000.0);
        // ⚠ Nur in der Luft. Am Boden gilt die feste Schwelle — sonst reichte
        // ein Ladewert mit gemeldeten 700 kt, um Sprünge von 20 km als ruhig
        // durchgehen zu lassen, und der Auto-Start feuerte ohne echte Ruhe
        // (Codex-QS 24.09.2026).
        let sekunden = if snap.on_ground { None } else { sekunden };
        let erlaubt_m = sekunden.map_or(crate::grenzwerte::grenzwerte().cockpit.auto_start_sprung_m.wert, |sekunden| {
            let gs_kt = f64::from(snap.groundspeed_kt.max(0.0)).min(crate::grenzwerte::grenzwerte().abgabe.resume_max_plausibel_kt.wert);
            let rate = f64::from(snap.simulation_rate).clamp(1.0, 16.0);
            let unterwegs_m = gs_kt * 0.514_444 * sekunden * rate * 2.0;
            crate::grenzwerte::grenzwerte().cockpit.auto_start_sprung_m.wert.max(unterwegs_m)
        });
        // Dazu der vorige Takt: Ein zeitbeschleunigter Flug bewegt sich
        // GLEICHMAESSIG, jeder Takt etwa gleich weit — ein Teleport ist ein
        // einzelner Ausreisser. Das wirkt bei jedem Simulator, auch wenn die
        // gemeldete Sim-Rate nicht passt: X-Plane vor v1.9.12 meldete fest
        // 1.0 (Cloud-QS 24.09.2026); seither der eingestellte Zeitraffer mal
        // Strecken-Multiplikator — Sollwerte, nicht die tatsaechlich erreichte
        // Rate. Das Dreifache laesst Luft fuer ungleiche Takte.
        //
        // Gedeckelt auf das, was ueberhaupt fliegbar ist (16-fache
        // Beschleunigung bei `RESUME_MAX_PLAUSIBEL_KT`, in 2 s rund 11 km):
        // Ein Teleport ueber hunderte Kilometer wird so nie „gleichmaessig",
        // auch nicht, wenn ihm ein gleich grosser folgt.
        let fliegbar_m = sekunden.map_or(0.0, |s| crate::grenzwerte::grenzwerte().abgabe.resume_max_plausibel_kt.wert * 0.514_444 * s * 16.0);
        // Nur bei dichten Takten: Nach einer langen Unterbrechung (Ruhezustand,
        // App Nap) wuechse der Deckel sonst mit, und ein zweiter Teleport
        // gleicher Groesse gaelte als gleichmaessig (Cloud-QS 24.09.2026).
        let dicht = sekunden.is_some_and(|s| s <= 5.0);
        let erlaubt_m = match self.letzter_abstand_m {
            Some(vorher) if dicht => erlaubt_m.max((vorher * 3.0).min(fliegbar_m)),
            _ => erlaubt_m,
        };
        let abstand_m = self
            .letzte_pos
            .map(|(lat, lon)| ::geo::distance_m(lat, lon, snap.lat, snap.lon));
        let gesprungen = abstand_m.is_some_and(|a| a > erlaubt_m);
        // Den Abstand IMMER merken: Unter Zeitbeschleunigung ohne gemeldete
        // Rate gilt schon der erste Takt als Sprung — verwerfe man ihn, entstuende
        // nie ein Vergleichswert, und das Gate hinge weiter (Test
        // `resume_gate_ohne_gemeldete_sim_rate`). Den Teleport verhindert der
        // Deckel oben.
        self.letzter_abstand_m = abstand_m;
        // Wechsel = ein Titel erscheint, der vom letzten BEKANNTEN abweicht:
        // A → B, und beim Hochfahren leer → A (der Titel kommt oft erst spät;
        // ab dann zählt die Ruhe neu). Ein kurz fehlender Titel (X-Plane-Web-
        // API liefert mal Teildaten) setzt nichts zurück: A → leer → A ist
        // kein Wechsel, weil der letzte bekannte Titel A stehen bleibt. Die
        // allererste Messung ist kein Wechsel.
        let flugzeug_gewechselt =
            self.letzte_pos.is_some() && titel.is_some() && titel != self.letzter_titel;
        self.letzte_pos = Some((snap.lat, snap.lon));
        self.letzte_zeit = Some(jetzt);
        if titel.is_some() {
            self.letzter_titel = titel;
        }
        let unruhe = if snap.paused || snap.slew_mode {
            Some("Der Simulator lädt oder ist pausiert.")
        } else if gesprungen {
            Some("Die Position springt noch (Laden oder Flughafenwechsel).")
        } else if flugzeug_gewechselt {
            Some("Das Flugzeug wurde gerade gewechselt.")
        } else {
            None
        };
        if let Some(grund) = unruhe {
            self.seit = None;
            return Some(grund.to_string());
        }
        let seit = *self.seit.get_or_insert(jetzt);
        let ruhig = (jetzt - seit).num_seconds();
        (ruhig < crate::grenzwerte::grenzwerte().cockpit.auto_start_sim_ruhe_secs.wert).then(|| {
            let auto_start_sim_ruhe_secs = crate::grenzwerte::grenzwerte().cockpit.auto_start_sim_ruhe_secs.wert;
            format!(
                "Der Simulator läuft erst {ruhig} s {SIM_RUHE_ZAEHLSTAND} {auto_start_sim_ruhe_secs} s."
            )
        })
    }
}

/// Was der Watcher mit einem einzelnen Bid tut. Reine Entscheidung ohne
/// Netz und Zustand, damit sie direkt testbar ist; die asynchrone Typ-
/// Pruefung (`get_aircraft`, Aliase) folgt nur bei `TypPruefen`.
#[derive(Debug, PartialEq)]
enum BidVorpruefung {
    /// In dieser Parkphase schon gestartet (oder der Start laeuft gerade).
    SchonGestartet,
    /// Flugzeug steht nicht am Abflughafen dieses Bids.
    NichtAmPlatz,
    /// Am Platz, aber der letzte Start scheiterte vor Kurzem.
    Pause { rest_s: i64, code: String },
    /// Am Platz, aber phpVMS kennt noch kein Flugzeug (SimBrief-OFP fehlt).
    OhneOfp,
    /// Alles da — Flugzeugtyp gegen den Sim pruefen, dann feuern.
    TypPruefen { flugzeug_id: i64 },
}

/// Reihenfolge: schon gestartet → Position → Pause → OFP. Die Pause kommt
/// bewusst NACH der Position: Ein Bid, an dessen Platz man nicht mehr steht,
/// soll nicht „neuer Versuch gleich" versprechen (QS Codex, 21.09.2026).
fn auto_start_bid_vorpruefung(
    bid: &Bid,
    letzter_bid: Option<i64>,
    am_platz: bool,
    pause: Option<(i64, String)>,
) -> BidVorpruefung {
    if letzter_bid == Some(bid.id) {
        return BidVorpruefung::SchonGestartet;
    }
    if !am_platz {
        return BidVorpruefung::NichtAmPlatz;
    }
    if let Some((rest_s, code)) = pause {
        return BidVorpruefung::Pause { rest_s, code };
    }
    match bid_flugzeug_id(bid) {
        Some(flugzeug_id) => BidVorpruefung::TypPruefen { flugzeug_id },
        None => BidVorpruefung::OhneOfp,
    }
}

/// Trennt, was der Pilot SIEHT, von dem, was ins Protokoll geht.
///
/// Das Banner zeigt einen Grund nur, solange sein Zeitstempel juenger als
/// 10 s ist (`auto_start_skip_status`). Frueher teilte es sich den
/// Zeitstempel mit der 60-s-Protokolldrossel — das Banner stand 10 s, war
/// 50 s weg und kam wieder (QS Codex, 21.09.2026). Jetzt wird der Grund
/// bei jedem Takt aufgefrischt, nur der Protokolleintrag ist gedrosselt.
#[derive(Debug, Default)]
struct HinweisProtokoll {
    zuletzt_protokolliert: Option<(DateTime<Utc>, String)>,
}

impl HinweisProtokoll {
    /// Soll dieser Grund jetzt ins Protokoll? Bei neuem Grund immer; bei
    /// gleichem nur, wenn `wiederholen_nach_s` gesetzt und verstrichen ist.
    fn protokollieren(
        &mut self,
        code: &str,
        jetzt: DateTime<Utc>,
        wiederholen_nach_s: Option<i64>,
    ) -> bool {
        let faellig = match &self.zuletzt_protokolliert {
            None => true,
            Some((_, alt)) if alt != code => true,
            Some((am, _)) => wiederholen_nach_s.is_some_and(|s| (jetzt - *am).num_seconds() >= s),
        };
        if faellig {
            self.zuletzt_protokolliert = Some((jetzt, code.to_string()));
        }
        faellig
    }
}

/// Haelt den Marker „dieser Bid wird gerade vom Auto-Start gestartet"
/// (`AppState::auto_start_laeuft_bid`) und raeumt ihn beim Ende des
/// Start-Tasks IMMER auf — auch bei Panik oder Abbruch des Tasks, und nur,
/// wenn der Eintrag noch ihm gehoert. Endet der Task, ohne dass der
/// normale Weg ihn als `erledigt` markiert hat (Panik/Abbruch), gibt er
/// auch den Anspruch `auto_start_last_bid_id` frei; sonst bliebe der Bid
/// bis zum Neustart der App gesperrt (Codex-Abnahme Runde 2, 22.09.2026).
struct AutoStartLaeuft {
    app: AppHandle,
    bid: i64,
    erledigt: bool,
}

/// Gibt `platz` nur frei, wenn dort noch `bid` steht — ein anderer
/// Eigentuemer bleibt unangetastet. Auch mit vergiftetem Mutex (Panik in
/// einem anderen Halter), denn gerade dann muss aufgeraeumt werden.
fn freigeben_wenn_eigen(platz: &Mutex<Option<i64>>, bid: i64) {
    let mut g = platz.lock().unwrap_or_else(|e| e.into_inner());
    if *g == Some(bid) {
        *g = None;
    }
}

/// Was beim Ende eines Auto-Start-Tasks aufgeraeumt wird — ohne
/// `AppHandle`, damit es direkt testbar ist.
///
/// Immer: den eigenen Marker freigeben. Nur bei NICHT normalem Ende
/// (Panik, Abbruch): auch den eigenen Anspruch freigeben UND die Pause
/// setzen wie nach einem Fehler. Ohne Pause startete der Watcher denselben
/// Bid nach 3 s neu — bei einer wiederholbaren Panik der Retry-Sturm aus
/// GlitchTip #10, womoeglich mit einem neuen Prefile je Versuch
/// (unabhaengige Pruefung, 22.09.2026).
fn auto_start_aufraeumen(
    marker: &Mutex<Option<i64>>,
    anspruch: &Mutex<Option<i64>>,
    pause: &Mutex<Option<(DateTime<Utc>, i64, String)>>,
    bid: i64,
    erledigt: bool,
    jetzt: DateTime<Utc>,
) {
    freigeben_wenn_eigen(marker, bid);
    if !erledigt {
        *pause.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((jetzt, bid, "abgebrochen".to_string()));
        freigeben_wenn_eigen(anspruch, bid);
    }
}

impl Drop for AutoStartLaeuft {
    fn drop(&mut self) {
        let state = self.app.state::<AppState>();
        auto_start_aufraeumen(
            &state.auto_start_laeuft_bid,
            &state.auto_start_last_bid_id,
            &state.auto_start_fail,
            self.bid,
            self.erledigt,
            Utc::now(),
        );
    }
}

#[cfg(test)]
#[path = "tests_lib/auto_start_vorpruefung_tests.rs"]
mod auto_start_vorpruefung_tests;

#[tauri::command]
fn auto_start_get_enabled(state: tauri::State<'_, AppState>) -> bool {
    state.auto_start_enabled.load(Ordering::Relaxed)
}

/// v0.3.0: Persistenz-Pfad für Auto-Start-Setting. localStorage geht im
/// Tauri-Dev-Mode bzw. nach Force-Kill der App regelmäßig verloren —
/// Backend-File überlebt jeden Restart und ist die Source-of-Truth.
fn auto_start_settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|p| p.join("auto_start.json"))
}

fn read_auto_start_persisted(app: &AppHandle) -> bool {
    let path = match auto_start_settings_path(app) {
        Some(p) => p,
        None => return false,
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return false,
    };
    // Datei-Format ist trivial: `{"enabled": true}` oder `{"enabled": false}`.
    // Kein serde-Overkill, plain regex-style match.
    text.contains("\"enabled\":true") || text.contains("\"enabled\": true")
}

fn write_auto_start_persisted(app: &AppHandle, enabled: bool) {
    let Some(path) = auto_start_settings_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let body = format!("{{\"enabled\":{}}}", enabled);
    let _ = std::fs::write(&path, body);
}

/// v0.3.0: Frontend-API für den Auto-Start-Skip-Grund. Liefert den
/// aktuellen Reason-Code (engines_on / moving / airborne) plus das
/// Alter in Sekunden seit der Watcher den letzten Skip festgestellt
/// hat. Frontend rendert daraus einen Banner im Briefing-Tab — der
/// Pilot wartet ja nicht im Settings-Log auf Hinweise.
///
/// Liefert `None` wenn alles passt (kein Skip kürzlich) oder Auto-
/// Start aus ist.
#[tauri::command(async)]
fn auto_start_skip_status(state: tauri::State<'_, AppState>) -> Option<AutoStartSkipDto> {
    if !state.auto_start_enabled.load(Ordering::Relaxed) {
        return None;
    }
    // Laeuft ein Flug, ist kein Auto-Start-Grund mehr aktuell — auch nicht
    // einer, den der Watcher im selben Augenblick noch geschrieben hat
    // (Codex-Abnahme Runde 2, 22.09.2026).
    if state
        .active_flight
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(false)
    {
        return None;
    }
    let g = state.auto_start_skip_reason.lock().unwrap();
    let (at, code) = g.as_ref()?;
    let age_secs = (Utc::now() - *at).num_seconds();
    // Älter als 10 s: vermutlich nicht mehr aktuell — Frontend soll
    // den Banner nicht zeigen.
    if age_secs > 10 {
        return None;
    }
    Some(AutoStartSkipDto {
        reason: code.clone(),
        age_secs,
    })
}

#[derive(Debug, Clone, Serialize)]
struct AutoStartSkipDto {
    reason: String,
    age_secs: i64,
}

#[tauri::command]
fn auto_start_set_enabled(
    enabled: bool,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), UiError> {
    let was = state.auto_start_enabled.swap(enabled, Ordering::Relaxed);
    // v0.3.0: Persistieren in JSON-File damit der Toggle-Stand auch
    // nach App-Restart / Force-Kill / Dev-Build erhalten bleibt.
    write_auto_start_persisted(&app, enabled);
    // The watcher task itself runs for the entire app lifetime (spawned
    // once in `.setup()`); flipping this flag is enough — no need to
    // spawn or kill anything here. Earlier versions did spawn-on-toggle,
    // which had a first-launch failure mode on Mac where the watcher
    // never came up at all and toggling off→on didn't recover (because
    // the gate `!was` blocked re-spawn after an IPC race).
    if enabled && !was {
        log_activity_handle(
            &app,
            ActivityLevel::Info,
            "Auto-Start aktiviert".to_string(),
            Some(
                "Watcher prüft alle 3 s ob ein Bid + Departure-Airport zueinander passen"
                    .to_string(),
            ),
        );
    } else if !enabled && was {
        log_activity_handle(
            &app,
            ActivityLevel::Info,
            "Auto-Start deaktiviert".to_string(),
            None,
        );
    }
    Ok(())
}

/// v0.12.6: Spiegelt das Frontend-Auto-File-Setting (localStorage) in
/// den Backend-State. Das Frontend ruft das bei Mount + bei jedem
/// Toggle. Der Position-Streamer liest das Flag beim Latch auf
/// `Arrived` und filet den PIREP dann selbst — fenster-unabhängig.
#[tauri::command]
fn set_auto_file_enabled(enabled: bool, state: tauri::State<'_, AppState>) {
    state.auto_file_enabled.store(enabled, Ordering::Relaxed);
}

/// Wie viele Flugplaetze liegen abgelegt, und wie viel Platz belegen sie?
///
/// Ein Zwischenspeicher, den niemand sehen kann, verrottet unbemerkt:
/// Man merkt erst beim naechsten Flug ohne Netz, dass er leer war.
/// Deshalb ist er abfragbar — auch wenn ihn im Alltag niemand braucht.
#[tauri::command(async)]
fn navdata_zwischenspeicher_bestand(app: AppHandle) -> serde_json::Value {
    let (anzahl, bytes) = navdata_cache::bestand(&app);
    serde_json::json!({
        "flugplaetze": anzahl,
        "bytes": bytes,
    })
}

/// Zeigt dem Piloten einen Auto-Start-Grund (Banner, jeden Takt frisch) und
/// schreibt ihn gedrosselt ins Protokoll, siehe [`HinweisProtokoll`].
fn auto_start_melden(
    app: &AppHandle,
    protokoll: &mut HinweisProtokoll,
    code: &str,
    titel: &str,
    text: String,
    wiederholen_nach_s: Option<i64>,
) {
    let jetzt = Utc::now();
    *app.state::<AppState>()
        .auto_start_skip_reason
        .lock()
        .unwrap() = Some((jetzt, code.to_string()));
    if protokoll.protokollieren(code, jetzt, wiederholen_nach_s) {
        log_activity_handle(app, ActivityLevel::Info, titel.to_string(), Some(text));
    }
}

/// Spawn the auto-start watcher task. Idempotent in practice:
/// the body checks `auto_start_enabled` on every tick and returns
/// when false, so spawning twice just means one drops out quickly.
fn spawn_auto_start_watcher(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tracing::info!("auto-start watcher started");
        // v0.13.16 (Option C): Cache der erwarteten Aircraft-ICAO pro
        // phpVMS-aircraft_id. Der Typ eines Aircraft-Datensatzes ist
        // immutabel, also genügt EIN get_aircraft je Flieger — kein
        // API-Spam pro 3-s-Tick. Wird für den alias-bewussten Typ-
        // Vorabcheck genutzt (nur wenn ein Bid in Reichweite + warm ist).
        let mut expected_icao_cache: std::collections::HashMap<i64, Option<String>> =
            std::collections::HashMap::new();
        // v0.15.16: Zeitpunkt des letzten VPS-Alias-Warm-Versuchs. Der Watcher
        // prüft den Aircraft-Typ alias-bewusst (Option C), der Alias-Cache wurde
        // bisher aber NUR bei flight_start/spawn_navdata_fetch geladen — vor dem
        // ersten Flug war er leer → ein aliased Flugzeug (z.B. E55P↔"PHENOM 300")
        // konnte NIE auto-starten (Henne-Ei). Wir wärmen den Cache jetzt lazy.
        let mut alias_warm_attempt_at: Option<DateTime<Utc>> = None;
        // Airports we've already warned about having no usable coordinates —
        // the loop ticks every 3 s and must not spam the activity log.
        let mut no_geometry_logged: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        // Ist der Simulator zur Ruhe gekommen? Siehe `SimRuhe`. Wird bei
        // ausgeschaltetem Auto-Start und während eines Flugs verworfen, damit
        // nach Flugende und nach dem Einschalten neu gemessen wird.
        let mut sim_ruhe = SimRuhe::default();
        let mut protokoll = HinweisProtokoll::default();
        loop {
            tokio::time::sleep(Duration::from_secs(crate::grenzwerte::grenzwerte().cockpit.auto_start_interval_secs.wert)).await;
            let state = app.state::<AppState>();
            // No-op when disabled — the watcher itself runs forever now
            // (spawned once at app start), the toggle just gates the
            // body. Used to `break` and rely on a re-spawn on next
            // toggle, which raced with first-launch IPC on Mac.
            if !state.auto_start_enabled.load(Ordering::Relaxed) {
                sim_ruhe = SimRuhe::default();
                protokoll = HinweisProtokoll::default();
                continue;
            }
            // Skip if a flight is already active.
            {
                let guard = state.active_flight.lock().expect("active_flight lock");
                if guard.is_some() {
                    sim_ruhe = SimRuhe::default();
                    protokoll = HinweisProtokoll::default();
                    continue;
                }
            }
            // Startet der Pilot gerade per Hand (Sperre belegt, `active_flight`
            // noch leer), nicht dazwischenfeuern — der Start ist unterwegs.
            if state.flight_setup_in_progress.load(Ordering::SeqCst) {
                continue;
            }
            // Laeuft gerade ein Auto-Start, startet der Watcher nichts
            // anderes: sonst konnte bei zwei passenden Bids ein zweiter Start
            // anlaufen, dessen schneller Fehler den Marker des ersten loeschte
            // (Codex-Abnahme Runde 2, 22.09.2026). Gemeldet wird nur „Start
            // läuft" fuer genau diesen Bid.
            let laeuft = *state
                .auto_start_laeuft_bid
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(bid) = laeuft {
                let (code, titel, text) = auto_start_hinweis(&AutoStartBeobachtung {
                    schon_gestartet: Some(bid),
                    start_laeuft_bid: Some(bid),
                    ..Default::default()
                });
                auto_start_melden(&app, &mut protokoll, code, titel, text, None);
                continue;
            }
            // v0.15.16: VPS-Alias-Cache lazy warm halten. Ohne das wäre er vor
            // dem ersten Flug leer → der alias-bewusste Typ-Vorabcheck unten
            // verwirft ein aliased Flugzeug (E55P↔"PHENOM 300E") fälschlich als
            // Mismatch und Auto-Start griffe NIE (Henne-Ei: Auto-Start braucht
            // den Alias, der Alias kam bisher erst bei flight_start). Nur wenn
            // der Cache leer ist + gedrosselt auf 60 s (Token evtl. erst nach
            // Provisioning verfügbar; bei Fehler/Timeout bleibt er leer und wird
            // in 60 s erneut versucht). Best-effort, 5 s-Timeout — hängt den
            // Watcher nie.
            {
                let cache_empty = state
                    .aircraft_aliases_vps
                    .lock()
                    .expect("aircraft_aliases_vps lock")
                    .is_empty();
                let now = Utc::now();
                // v0.16.19: NICHT mehr nur bei leerem Cache nachladen. Sonst
                // griff ein neu im VA-Admin angelegter Alias auf dem Auto-
                // Start-Pfad erst nach einem App-Neustart (der Cache war ja
                // schon einmal gewärmt → nie wieder gefetcht). Jetzt: leerer
                // Cache → schnelles Retry (60 s, Erst-Warmup unverändert),
                // gefüllter Cache → periodischer Refresh (5 min), damit
                // hinzugefügte/geänderte/gelöschte Aliase ohne Neustart
                // greifen. fetch_… macht cache.clear()+neu NUR bei Erfolg,
                // wischt also nie bei Netzfehler. Best-effort, 5 s-Timeout.
                let refresh_due_secs = if cache_empty { 60 } else { 300 };
                let due = alias_warm_attempt_at
                    .is_none_or(|at| (now - at).num_seconds() >= refresh_due_secs);
                if due {
                    alias_warm_attempt_at = Some(now);
                    let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
                    let _ = tokio::time::timeout(
                        Duration::from_secs(5),
                        fetch_aircraft_aliases_into_state(app.clone(), None, token),
                    )
                    .await;
                }
            }
            // Skip if no sim snapshot yet, or the aircraft is moving /
            // engines on (= already taxiing or rolling, not "ready to
            // fire" anymore). v0.3.0: bei jedem Skip einen kurzen
            // Hint im Activity-Log, damit der Pilot weiß WARUM Auto-
            // Start nicht greift. Throttled auf 1× / 60 s pro reason
            // damit der Log nicht spamt.
            let Some(snap) = current_snapshot(&app) else {
                sim_ruhe = SimRuhe::default();
                continue;
            };
            // Erst wenn der Simulator zur Ruhe gekommen ist, schaut der
            // Auto-Start überhaupt hin. Solange er lädt, pausiert ist oder
            // springt, gibt es EINEN ruhigen Hinweis statt wechselnder Gründe
            // („in der Luft" / „6724 nm" / „Sprit 0"): ins Protokoll nur beim
            // Eintritt, das Banner bleibt stehen.
            if let Some(grund) = sim_ruhe.pruefen(&snap, Utc::now()) {
                auto_start_melden(
                    &app,
                    &mut protokoll,
                    AUTO_START_SIM_WARTET,
                    "Auto-Start: wartet auf den Simulator",
                    grund,
                    None,
                );
                continue;
            }
            // v0.3.0: Race-Condition-Fix nach Sim-Reconnect. Wenn der
            // Sim gerade frisch connected ist (Pilot hat X-Plane neu
            // gestartet), hat der Snapshot kurz Default-0-Werte
            // (engines=0, on_ground=true, gs=0) — die ALLE die
            // Auto-Start-Bedingungen "scheinbar erfüllen" und der
            // Watcher fired BEVOR die echten Daten reinkommen
            // (Triebwerke an, Aircraft-Title gesetzt etc.). Pilot
            // hat dann ungewollt einen Auto-Start mit laufenden
            // Triebwerken bekommen.
            //
            // v0.7.17 (N-003a/b/c): Differenzierte Diagnose statt
            // pauschalem Skip. Bisher war der "nicht warm"-Skip stumm
            // (nur Debug-Log), was Piloten ohne X-Plane-Web-API ratlos
            // zurueckliess — siehe Michel's „Auto-Start funktioniert
            // nicht"-Report.
            //   * `aircraft_title` leer → bei XP12-Profil ist die
            //     Web-API nicht aktiviert (Settings → Network); bei
            //     MSFS der Sim noch im Boot.
            //   * `fuel_total_kg` ≤ 100 → Anti-Race-Sanity ist zu
            //     strikt fuer Light-GA. Wir lockern auf > 1 kg, was
            //     den Sim-Boot-Race weiter abfaengt (Default = 0) ohne
            //     legitime kleine Sprits auszuschliessen.
            let title_missing = snap
                .aircraft_title
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true);
            let fuel_zero = snap.fuel_total_kg <= 1.0;
            let warm_reason: Option<(&'static str, &'static str)> = if title_missing
                && matches!(
                    snap.simulator,
                    sim_core::Simulator::XPlane11 | sim_core::Simulator::XPlane12
                ) {
                Some((
                        "title_missing_xplane",
                        "Auto-Start wartet auf Sim-Daten: Aircraft-Titel fehlt. In X-Plane Settings > Network > Web API einschalten, damit AeroACARS den Flugzeug-Namen lesen kann.",
                    ))
            } else if title_missing {
                Some((
                        "title_missing",
                        "Auto-Start wartet auf Sim-Daten: Aircraft-Titel fehlt. Sim ist noch im Boot oder das Flugzeug ist nicht voll geladen.",
                    ))
            } else if fuel_zero {
                Some((
                        "fuel_zero",
                        "Auto-Start wartet auf Sim-Daten: Fuel = 0. Tank ist leer (Cold & Dark ohne Sprit) oder Sim noch im Boot.",
                    ))
            } else {
                None
            };
            let skip_reason: Option<(&'static str, &'static str)> = if let Some(r) = warm_reason {
                Some(r)
            } else if !snap.on_ground {
                Some((
                    "airborne",
                    "Du bist in der Luft — Auto-Start funktioniert nur am Boden",
                ))
            } else if snap.groundspeed_kt > 5.0 {
                Some((
                    "moving",
                    "Flugzeug rollt schon — Auto-Start greift nur am Stand",
                ))
            } else if snap.engines_running > 0 {
                Some(("engines_on", "Triebwerke sind an — Auto-Start greift nur bei kalt/stehendem Flugzeug. Manuell 'Flug starten' nutzen."))
            } else {
                None
            };
            if let Some((reason_code, reason_msg)) = skip_reason {
                // Protokoll: bei neuem Grund sofort, sonst höchstens 1×/60 s.
                auto_start_melden(
                    &app,
                    &mut protokoll,
                    reason_code,
                    "Auto-Start: nicht möglich",
                    reason_msg.to_string(),
                    Some(60),
                );
                continue;
            }
            // v0.12.11: KEIN unbedingtes `*g = None` mehr hier. Der Reset
            // bei erfüllten Voraussetzungen löschte den Throttle-State
            // direkt VOR dem „no_bids"-Check — ein geparktes Flugzeug
            // ohne gebuchten Bid loggte die „keine Bids"-Zeile dadurch
            // bei JEDEM Poll (alle paar Sekunden, siehe Live-Log Sven M /
            // FDX1636: 150+ identische Einträge). Die Code-Wechsel-
            // Erkennung im skip_reason-Zweig liefert ohnehin einen
            // frischen Hint, sobald sich der Grund tatsächlich ändert.
            // Fetch the user's bids to find a match.
            let client = match {
                let g = state.client.lock().expect("client lock");
                g.clone()
            } {
                Some(c) => c,
                None => continue, // not logged in
            };
            let bids = match client.get_bids().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            // v0.7.17 (N-003): Wenn die Bid-Liste leer ist, sagen wir
            // es dem Piloten — vorher Silent-Skip.
            if bids.is_empty() {
                // v0.12.11: reine Edge-Detektion. „Keine Bids" ist ein
                // Dauerzustand, kein Ereignis — nur loggen, wenn der
                // Grund NEU auf „no_bids" wechselt, nicht bei jedem Poll.
                auto_start_melden(
                    &app,
                    &mut protokoll,
                    "no_bids",
                    "Auto-Start: keine Bids verfuegbar",
                    "Im phpVMS-Bid-Tab steht aktuell nichts gebucht. Logged-in?".to_string(),
                    None,
                );
                continue;
            }
            // Don't re-fire for the same bid within the same parked
            // session. Cleared in `flight_end` via the active_flight
            // guard above resetting on the next tick.
            let last_bid = {
                let g = state.auto_start_last_bid_id.lock().unwrap();
                *g
            };
            let mut fired = false;
            // Der Sim war zwischen Ruheprüfung und Feuern nicht mehr ruhig.
            let mut unruhig_geworden = false;
            // Was dieser Tick über die nicht gestarteten Bids erfährt; daraus
            // wählt `auto_start_hinweis` nach der Schleife den einen Hinweis.
            let mut beobachtung = AutoStartBeobachtung::default();
            for bid in &bids {
                // v0.15.13: Fehler-Cooldown. Ist dieser Bid kürzlich an
                // flight_start gescheitert (not_at_departure / flight_already_active
                // / …), feuern wir ihn AUTO_START_FAIL_COOLDOWN_SECS lang nicht
                // erneut — sonst Retry-Sturm alle 3 s (GlitchTip #10). Gewertet
                // wird die Pause erst am Platz, siehe `auto_start_bid_vorpruefung`.
                let pause = {
                    let g = state.auto_start_fail.lock().unwrap();
                    auto_start_pause_rest(bid.id, g.as_ref(), Utc::now())
                };
                // v0.19.3: the departure airport's coordinates as phpVMS knows
                // them — the fallback for the thousands of fields the embedded
                // runway table has no threshold geometry for (see
                // `distance_to_airport_any_source`). Cache-only + best-effort
                // fetch: this runs every 3 s, so a miss must not block the tick.
                let dpt_icao = bid.flight.dpt_airport_id.trim().to_uppercase();
                let phpvms_pos: Option<(f64, f64)> = {
                    let cached = {
                        let guard = state.airports.lock().expect("airports lock");
                        guard.get(&dpt_icao).cloned()
                    };
                    let airport = match cached {
                        Some(a) => Some(a),
                        None => {
                            let client = state.client.lock().expect("client mutex").clone();
                            match client {
                                Some(c) => match c.get_airport(&dpt_icao).await {
                                    Ok(a) => {
                                        state
                                            .airports
                                            .lock()
                                            .expect("airports lock")
                                            .insert(dpt_icao.clone(), a.clone());
                                        Some(a)
                                    }
                                    Err(_) => None,
                                },
                                None => None,
                            }
                        }
                    };
                    airport.and_then(|a| a.lat.zip(a.lon))
                };

                // v0.7.17 (N-003): Track nearest-bid distance fuer
                // den Aktivity-Log-Hint, wenn am Ende KEIN Bid matcht.
                match distance_to_airport_any_source(&dpt_icao, &snap, phpvms_pos) {
                    Some(m) => {
                        let dist_nm = m / 1852.0;
                        beobachtung.naechster_nm =
                            Some(beobachtung.naechster_nm.map_or(dist_nm, |c| c.min(dist_nm)));
                    }
                    None => {
                        // No geometry from the runway table AND none from
                        // phpVMS. Auto-start cannot fire for this bid — say so
                        // once, instead of leaving the pilot staring at a gate
                        // wondering why nothing happens.
                        if !no_geometry_logged.contains(&dpt_icao) {
                            no_geometry_logged.insert(dpt_icao.clone());
                            log_activity_handle(
                                &app,
                                ActivityLevel::Warn,
                                format!("Auto-Start für {dpt_icao} nicht möglich"),
                                Some(format!(
                                    "Für {dpt_icao} liegen weder in der Runway-Datenbank noch \
                                     in phpVMS Koordinaten vor — der Auto-Start kann nicht \
                                     prüfen, ob du dort stehst. Starte den Flug bitte manuell."
                                )),
                            );
                            tracing::warn!(
                                icao = %dpt_icao,
                                "auto-start: no airport geometry from any source"
                            );
                        }
                    }
                }
                let am_platz = bid_matches_current_state(bid, &snap, phpvms_pos);
                // Am Platz, aber noch ohne SimBrief-OFP (`OhneOfp`): phpVMS hat
                // dem Bid noch kein Flugzeug zugeordnet, und `flight_start` würde
                // mit `missing_aircraft` ablehnen. Nicht feuern, nicht claimen,
                // keine Pause — die Bids kommen jeden Tick frisch, der Start
                // folgt also wenige Sekunden nach dem OFP. Scheitert es doch
                // (Vorabprüfung und flight_start teilen `bid_flugzeug_id`, das
                // sollte nicht vorkommen), greift der Fehlerpfad unten wie bisher.
                let ac_id = match auto_start_bid_vorpruefung(bid, last_bid, am_platz, pause) {
                    BidVorpruefung::SchonGestartet => {
                        beobachtung.schon_gestartet = Some(bid.id);
                        continue;
                    }
                    BidVorpruefung::NichtAmPlatz => continue,
                    BidVorpruefung::Pause { rest_s, code } => {
                        beobachtung.pause = Some((bid.id, rest_s, code));
                        continue;
                    }
                    BidVorpruefung::OhneOfp => {
                        beobachtung.ohne_ofp = Some(bid.flight.flight_number.clone());
                        continue;
                    }
                    BidVorpruefung::TypPruefen { flugzeug_id } => flugzeug_id,
                };
                // v0.13.16 (Option C): alias-bewusster Aircraft-Typ-Vorabcheck
                // BEVOR wir feuern. Vorher feuerte der Watcher blind
                // flight_start; bei Aircraft-Mismatch (z.B. Bid E55P vs Sim
                // „PHENOM 300E", oder fehlender Alias) lief er in einen
                // 3-s-Dauer-Retry, der den `flight_setup_in_progress`-Lock
                // hielt und das manuelle „Trotzdem starten" des Piloten
                // blockierte (Live-Report Michael K / DWW306, 2026-06-03).
                // Jetzt prüfen wir den Typ vorab mit DERSELBEN Logik wie das
                // flight_start-Gate (`aircraft_type_matches_sim`, inkl. VPS-
                // Aliase) und feuern nur, wenn es matchen würde — selbst-
                // korrigierend: lädt der Pilot das passende Flugzeug ODER
                // wird der VPS-Alias ergänzt, matcht der nächste Tick und der
                // Auto-Start greift automatisch. Echter Wetlease: Watcher
                // überspringt korrekt, Lock bleibt frei fürs manuelle
                // Override. get_aircraft nur hier (Bid in Reichweite + warm)
                // und pro aircraft_id gecacht → kein Per-Tick-API-Spam.
                let expected_icao: Option<String> = match expected_icao_cache.get(&ac_id) {
                    Some(cached) => cached.clone(),
                    None => match client.get_aircraft(ac_id).await {
                        Ok(ac) => {
                            let v = ac
                                .icao
                                .as_ref()
                                .map(|s| s.trim().to_uppercase())
                                .filter(|s| !s.is_empty());
                            expected_icao_cache.insert(ac_id, v.clone());
                            v
                        }
                        // Auflösung fehlgeschlagen → NICHT cachen (nächster
                        // Tick versucht's erneut), für jetzt permissiv.
                        Err(e) => {
                            tracing::debug!(
                                ?e,
                                ac_id,
                                "auto-start: get_aircraft für Typ-Vorabcheck fehlgeschlagen — permissiv"
                            );
                            None
                        }
                    },
                };
                let sim_icao = snap.aircraft_icao.as_deref().and_then(clean_atc_model);
                let sim_title = snap.aircraft_title.as_deref().unwrap_or("");
                let aliases_snapshot = state
                    .aircraft_aliases_vps
                    .lock()
                    .expect("aircraft_aliases_vps lock")
                    .clone();
                if !aircraft_type_matches_sim(
                    expected_icao.as_deref(),
                    sim_icao.as_deref(),
                    sim_title,
                    &aliases_snapshot,
                ) {
                    // Typ passt nicht → NICHT feuern, NICHT claimen. Der
                    // Post-Loop-Hint meldet es (gedrosselt). Pilot übersteuert
                    // manuell („Trotzdem starten") oder lädt das richtige
                    // Flugzeug → nächster Tick matcht.
                    beobachtung.typ_passt_nicht = Some((
                        bid.flight.flight_number.clone(),
                        expected_icao.clone().unwrap_or_else(|| "?".to_string()),
                        sim_icao.clone().unwrap_or_else(|| "?".to_string()),
                    ));
                    continue;
                }
                // Zwischen der Ruheprüfung oben und hier lagen Netzabfragen
                // (Bids, Flughafen, Flugzeug). Hat der Sim inzwischen pausiert,
                // neu geladen oder ist gesprungen, NICHT feuern — der nächste
                // Takt meldet „wartet auf den Simulator" (QS Codex, 21.09.2026).
                let noch_ruhig = current_snapshot(&app)
                    .is_some_and(|frisch| sim_ruhe.pruefen(&frisch, Utc::now()).is_none());
                if !noch_ruhig {
                    unruhig_geworden = true;
                    break;
                }
                // Match — fire flight_start.
                tracing::info!(
                    bid_id = bid.id,
                    flight = %bid.flight.flight_number,
                    "auto-start triggering flight_start"
                );
                log_activity_handle(
                    &app,
                    ActivityLevel::Info,
                    format!(
                        "Auto-Start: {} {} – {}",
                        bid.flight.flight_number,
                        bid.flight.dpt_airport_id,
                        bid.flight.arr_airport_id
                    ),
                    Some(format!(
                        "Aircraft {} matched bid {}",
                        snap.aircraft_title.as_deref().unwrap_or("(unknown)"),
                        bid.id
                    )),
                );
                {
                    let mut g = state.auto_start_last_bid_id.lock().unwrap();
                    *g = Some(bid.id);
                }
                *state.auto_start_laeuft_bid.lock().unwrap() = Some(bid.id);
                let waechter = AutoStartLaeuft {
                    app: app.clone(),
                    bid: bid.id,
                    erledigt: false,
                };
                let app_for_call = app.clone();
                let bid_id = bid.id;
                tauri::async_runtime::spawn(async move {
                    let mut waechter = waechter;
                    let state_ref = app_for_call.state::<AppState>();
                    // v0.8.3 (#7): Auto-Start liefert kein acknowledge mit —
                    // ein Aircraft-Mismatch ergäbe "aircraft_mismatch_warning"
                    // und der Pilot müsste manuell „Trotzdem starten" klicken.
                    // v0.13.16 (Option C): das fängt jetzt der Typ-Vorabcheck
                    // oben ab — wir feuern nur bei passendem (oder nicht
                    // auflösbarem) Typ. Ein Mismatch hier ist damit nur noch
                    // ein Race-Edge (Sim-Flugzeug zwischen Vorabcheck und
                    // flight_start gewechselt) → der Fehler-Pfad cleart
                    // last_bid_id, der nächste Tick prüft erneut vorab.
                    let ergebnis =
                        flight_start_mit(app_for_call.clone(), state_ref, bid_id, None, true).await;
                    {
                        let s = app_for_call.state::<AppState>();
                        // Der Start ist vorbei, so oder so: Marker frei (nur
                        // der eigene), den Anspruch regelt der Weg unten.
                        waechter.erledigt = true;
                        drop(waechter);
                        // Erfolg: „Start läuft" darf nicht noch bis zu 10 s im
                        // Banner stehen — der Flug laeuft (Codex-Abnahme
                        // 22.09.2026).
                        if ergebnis.is_ok() {
                            *s.auto_start_skip_reason.lock().unwrap() = None;
                        }
                    }
                    if let Err(e) = ergebnis {
                        let s = app_for_call.state::<AppState>();
                        // Sim wurde im letzten Moment unruhig: kein Fehler, keine
                        // Pause — Claim lösen, der Watcher wartet auf Ruhe.
                        if auto_start_fehler_ist_kein_fehler(&e.code) {
                            tracing::info!(
                                bid_id,
                                code = %e.code,
                                "auto-start: Start gerade nicht moeglich — wartet"
                            );
                            *s.auto_start_last_bid_id.lock().unwrap() = None;
                            return;
                        }
                        tracing::warn!(?e, bid_id, "auto-start: flight_start command failed");
                        // v0.15.13: Fehler-Cooldown setzen, BEVOR last_bid_id
                        // gecleart wird. Der Loop-Cooldown-Check verhindert
                        // damit ab dem nächsten Tick das sofortige Re-Fire
                        // (3-s-Retry-Sturm, GlitchTip #10). Das Log läuft dadurch
                        // nur noch ~1×/Cooldown statt ~20×/min.
                        {
                            let mut f = s.auto_start_fail.lock().unwrap();
                            *f = Some((Utc::now(), bid_id, e.code.clone()));
                        }
                        // v0.7.17 (N-003): nicht mehr silent — im Activity-Log,
                        // damit der Pilot sieht warum sein automatisch erkannter
                        // Flug nicht startete (z.B. „missing_aircraft",
                        // „not_at_departure", „flight_already_active").
                        let app_log = app_for_call.clone();
                        log_activity_handle(
                            &app_log,
                            ActivityLevel::Warn,
                            "Auto-Start: flight_start fehlgeschlagen".to_string(),
                            Some(format!("Bid {} – Fehler: {}", bid_id, e.code)),
                        );
                        // Clear the last_bid_id so the user can retry after the
                        // cooldown (or immediately by fixing the condition +
                        // toggling auto-start). The fail-cooldown above now gates
                        // the re-fire cadence, not this clear.
                        let mut g = s.auto_start_last_bid_id.lock().unwrap();
                        *g = None;
                    }
                });
                fired = true;
                break;
            }
            // v0.7.17 (N-003): Wenn die for-Schleife ohne Match endet
            // (kein break), gib dem Piloten einen Hint mit dem nähesten
            // Departure-Airport. Vorher silent.
            if !fired && !unruhig_geworden {
                // Vom konkretesten Grund zum allgemeinsten (Typ passt nicht →
                // OFP fehlt → Pause → schon gestartet → kein Bid am Platz),
                // siehe `auto_start_hinweis`.
                // Zwischen dem Tick-Anfang und hier lagen Netzabfragen. Ist der
                // Flug inzwischen angelegt, gibt es nichts mehr zu melden —
                // sonst stuende doch wieder „schon mal auto-gestartet" da
                // (Codex-Abnahme 22.09.2026).
                if state
                    .active_flight
                    .lock()
                    .expect("active_flight lock")
                    .is_some()
                {
                    continue;
                }
                let (reason_code, title, reason_msg) = auto_start_hinweis(&beobachtung);
                auto_start_melden(
                    &app,
                    &mut protokoll,
                    reason_code,
                    title,
                    reason_msg,
                    Some(60),
                );
            }
        }
    });
}

/// How far is the aircraft from an airport, in meters? `None` = we have no
/// coordinates for that field from ANY source and genuinely cannot tell.
///
/// Two sources, in order:
///   1. the embedded runway table (nearest threshold — the same metric
///      `arrival::locate` uses, so departure and arrival agree on what "at the
///      airport" means);
///   2. the airport coordinates phpVMS ships with the bid, cached in
///      `state.airports`.
///
/// v0.19.3: (2) is new here, and it matters. The runway CSV parser drops every
/// runway whose thresholds have no coordinates, so `airport_position` returns
/// `None` for **6,446 real ICAO airports** — 183 German ED** fields, 176 French
/// LF**, 85 UK EG**, plus airline-served fields like KECP. Auto-start treated
/// that as "does not match" and simply never fired there, with no message: a
/// pilot booking a Falcon out of EDAH or EGBK sat at the gate forever. The same
/// missing-data condition on the arrival side means "give the pilot the benefit
/// of the doubt" — the same table, the same question, and the opposite answer.
/// phpVMS knows where its own airports are, so ask it.
fn distance_to_airport_any_source(
    icao: &str,
    snap: &SimSnapshot,
    phpvms_pos: Option<(f64, f64)>,
) -> Option<f64> {
    if let Some(m) = runway::distance_to_airport_m(icao, snap.lat, snap.lon) {
        return Some(m);
    }
    let (lat, lon) = phpvms_pos?;
    // phpVMS ships (0,0) for airports whose coordinates were never filled in —
    // that is "unknown", not "off the coast of Africa".
    if lat == 0.0 && lon == 0.0 {
        return None;
    }
    Some(::geo::distance_m(snap.lat, snap.lon, lat, lon))
}

/// Does the loaded aircraft + position match this bid's expectations?
///
/// Airport-proximity match: the pilot must be parked within
/// `AUTO_START_PROXIMITY_M` of the bid's departure airport. (Aircraft-type
/// matching happens separately in the watcher, which has the API access to
/// resolve the bid's planned aircraft.) If multiple bids match the same
/// airport, the first one wins; the pilot can cancel the auto-started flight to
/// fall through to the next.
///
/// `phpvms_pos`: the departure airport's coordinates as phpVMS knows them, when
/// the caller has them cached — the fallback for fields the embedded runway
/// table has no geometry for. When BOTH sources come up empty we return false
/// (we cannot place the aircraft, and auto-starting a flight for a pilot who
/// might be somewhere else entirely is worse than not auto-starting) — but the
/// watcher logs that case, so it is never silent again.
fn bid_matches_current_state(
    bid: &Bid,
    snap: &SimSnapshot,
    phpvms_pos: Option<(f64, f64)>,
) -> bool {
    match distance_to_airport_any_source(&bid.flight.dpt_airport_id, snap, phpvms_pos) {
        Some(dist) => dist <= crate::grenzwerte::grenzwerte().cockpit.auto_start_proximity_m.wert,
        None => false,
    }
}
