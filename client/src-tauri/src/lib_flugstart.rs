#[derive(Serialize)]
pub struct ResumableFlight {
    pirep_id: String,
    flight_number: String,
    dpt_airport: String,
    arr_airport: String,
    status: Option<String>,
    /// Sekunden seit der letzten Meldung an phpVMS. Unter
    /// `LAEUFT_WOANDERS_SECS` laeuft der Flug gerade auf einem anderen Geraet.
    zuletzt_gemeldet_vor_s: Option<i64>,
    /// Siehe `laeuft_woanders`.
    laeuft_woanders: bool,
}

/// Meldet ein Client einen Flug juenger als das, laeuft er gerade — auf einem
/// anderen Geraet, denn dieses hat ihn nicht. Die Lebenszeichen kommen alle
/// paar Minuten, auch in der Pause (`HEARTBEAT_INTERVAL`).
///
/// ⚠ 24.10.2026 (Thomas): Flug auf Windows, AeroACARS auf dem Mac geoeffnet —
/// der Mac fand den PIREP, zeigte den 30-s-Countdown und haette den Flug von
/// selbst uebernommen.
const LAEUFT_WOANDERS_SECS: i64 = 600;

/// Sekunden seit `updated_at`; `None`, wenn phpVMS es nicht liefert.
fn zuletzt_gemeldet_vor_s(p: &api_client::PirepSummary, jetzt: DateTime<Utc>) -> Option<i64> {
    let t = DateTime::parse_from_rfc3339(p.updated_at.as_deref()?.trim()).ok()?;
    Some((jetzt - t.with_timezone(&Utc)).num_seconds().max(0))
}

fn laeuft_woanders(p: &api_client::PirepSummary, jetzt: DateTime<Utc>) -> bool {
    zuletzt_gemeldet_vor_s(p, jetzt).is_some_and(|s| s < LAEUFT_WOANDERS_SECS)
}

#[cfg(test)]
#[path = "tests_lib/laeuft_woanders_tests.rs"]
mod laeuft_woanders_tests;

/// Look on phpVMS for in-progress PIREPs the user could resume. Used to drive
/// the auto-detection banner on dashboard mount: if the answer is non-empty,
/// the UI offers to adopt with a 10s countdown.
///
/// Skipped (returns empty) if a flight is already attached locally —
/// either in memory (Disk-Resume already finished) OR still pending on
/// disk (Disk-Resume hasn't finished its async backfill yet but will
/// install the same PIREP shortly). Without the disk-file check, the
/// frontend's mount-time discovery race could find a separate
/// in-progress PIREP on phpVMS and offer it as a discovered flight,
/// then `flight_adopt` would crash with "another flight is already
/// active" the moment Disk-Resume committed.
/// Liegt ein gespeicherter Flug, der noch aufgenommen werden koennte?
///
/// ⚠ Gebaut fuer den Pflicht-Update-Riegel. Der laesst einen laufenden Flug
/// in Ruhe — aber er fragt dafuer die PHASE, und die ist nach einem
/// Neustart erst bekannt, wenn die Wiederaufnahme durch ist. Die braucht
/// einen Roundtrip zum Server; der Riegel ist sofort da. In diesem Fenster
/// saehe er `null` und wuerde sperren: Der Pilot bekaeme mitten im Flug
/// einen Zwang zum Update vorgesetzt, den es fuer ihn gar nicht geben darf.
///
/// Deshalb diese Frage, die OHNE Netz beantwortbar ist: Die Datei liegt auf
/// der Platte oder nicht.
#[tauri::command(async)]
fn flight_wiederaufnahme_steht_aus(app: AppHandle, state: tauri::State<'_, AppState>) -> bool {
    // Ein bereits aufgenommener Flug zaehlt auch — dann ist die Frage
    // ohnehin beantwortet und die Phase da.
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if guard.is_some() {
            return true;
        }
    }
    match read_persisted_flight(&app) {
        Some(p) => stillstand_erlaubt_wiederaufnahme(stillstand_von(&p)),
        None => false,
    }
}

#[tauri::command]
async fn flight_discover_resumable(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ResumableFlight>, UiError> {
    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if guard.is_some() {
            return Ok(Vec::new());
        }
    }
    // ⚠ Hier stand dieselbe 12-Stunden-Grenze wie im Resume: Nur ein "junger"
    // Flug auf der Platte durfte gewinnen. Sie ist ersatzlos weg — liegt eine
    // Datei, gehoert der Vortritt IHR, unabhaengig von der Flugdauer.
    // `try_resume_flight` entscheidet dann anhand der Server-Auskunft und
    // raeumt die Datei auf, wenn der PIREP wirklich tot ist. Danach findet
    // dieser Weg hier den verwaisten Flug beim naechsten Anlauf.
    if let Some(persisted) = read_persisted_flight(&app) {
        // ⚠ Zurueckgetreten wird nur fuer eine Datei, die auch wirklich
        // aufgenommen werden KANN. Sonst entsteht eine Sackgasse: Die
        // Wiederaufnahme lehnt wegen Stillstand ab (und loescht bewusst
        // nicht), und diese Suche tritt fuer genau diese Datei zurueck —
        // der verwaiste Flug wuerde nie mehr angeboten.
        let stillstand = stillstand_von(&persisted);
        if stillstand_erlaubt_wiederaufnahme(stillstand) {
            tracing::debug!(
                pirep_id = %persisted.pirep_id,
                "discover_resumable: disk-resume pending — returning empty so the disk flight wins"
            );
            return Ok(Vec::new());
        }
        tracing::info!(
            pirep_id = %persisted.pirep_id,
            stillstand_stunden = stillstand.num_hours(),
            "discover_resumable: Datei zu lange still — der Server darf antworten"
        );
    }
    let client = current_client(&state)?;
    // v0.16.17: server-seitig auf IN_PROGRESS gefiltert (?state=0) statt
    // page-1-fetch-then-filter — sonst sind Orphans älter als ~20 Flüge
    // unsichtbar (paginate() liefert 20/Seite, newest first).
    let pireps = client.get_user_pireps_in_progress().await?;
    Ok(pireps
        .into_iter()
        .filter(|p| p.state == Some(0)) // defensiv — Server hat bereits gefiltert
        .filter_map(|p| {
            let jetzt = Utc::now();
            let zuletzt_vor_s = zuletzt_gemeldet_vor_s(&p, jetzt);
            let woanders = laeuft_woanders(&p, jetzt);
            Some(ResumableFlight {
                pirep_id: p.id,
                flight_number: p.flight_number?,
                dpt_airport: p.dpt_airport_id.unwrap_or_default(),
                arr_airport: p.arr_airport_id.unwrap_or_default(),
                zuletzt_gemeldet_vor_s: zuletzt_vor_s,
                laeuft_woanders: woanders,
                status: p.status,
            })
        })
        .collect())
}

/// Prefer the pilot's PaxStudio-booked per-class fare split over phpVMS-core's generic
/// subfleet distribution. Claims the OFP for the PIREP (authoritative id-only link,
/// no route rewrite) and returns the booked fares. Best-effort: no OFP, no PaxStudio
/// booking, an older/absent PaxStudio module, or any error → keep `core_fares`. Never
/// blocks the flight. This is how AeroACARS files the correct load at the source; the
/// server-side correction then finds the data already correct and does nothing.
async fn resolve_paxstudio_fares(
    client: &Client,
    simbrief_id: Option<&str>,
    pirep_id: &str,
    core_fares: Vec<(i64, i32)>,
) -> Vec<(i64, i32)> {
    let Some(sb_id) = simbrief_id else {
        return core_fares;
    };
    // Best-effort enrichment must never hold up flight start: cap the call well below
    // the client's global 10s HTTP timeout so a slow/hanging VA PaxStudio endpoint
    // can't delay tracking. On timeout we keep the core fares.
    let claim = match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        client.claim_paxstudio_ofp(sb_id, pirep_id),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!(
                pirep_id = %pirep_id, simbrief_id = %sb_id,
                "PaxStudio claim timed out (4s); keeping core fares"
            );
            return core_fares;
        }
    };
    match claim {
        Ok(res) if !res.fares.is_empty() => {
            let mapped: Vec<(i64, i32)> = res.fares.iter().map(|f| (f.id, f.count)).collect();
            tracing::info!(
                pirep_id = %pirep_id, simbrief_id = %sb_id, fare_rows = mapped.len(),
                value_source = %res.value_source, linked = res.linked,
                "using PaxStudio booked fares (filed at source)"
            );
            mapped
        }
        Ok(res) => {
            tracing::info!(
                pirep_id = %pirep_id, simbrief_id = %sb_id, value_source = %res.value_source,
                "PaxStudio has no booked split for this OFP; keeping core fares"
            );
            core_fares
        }
        Err(e) => {
            tracing::warn!(
                pirep_id = %pirep_id, simbrief_id = %sb_id, error = %e,
                "PaxStudio claim failed; keeping core fares"
            );
            core_fares
        }
    }
}

/// Adopt a specific in-progress PIREP — creates the local ActiveFlight,
/// persists it to disk, starts the position streamer. Used by the resume
/// banner after the 10s countdown elapses (or the user clicks Resume now).
#[tauri::command]
async fn flight_adopt(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    pirep_id: String,
    // true nur nach ausdruecklicher Rueckfrage im Banner: Der Pilot weiss,
    // dass der Flug eben noch woanders gemeldet hat.
    trotz_anderem_geraet: Option<bool>,
) -> Result<ActiveFlightInfo, UiError> {
    // Reserve the setup slot atomically — released automatically (via the
    // guard's Drop) on any error path, or disarmed once we've committed the
    // new ActiveFlight into state. Without this, two concurrent adopts
    // would both see active_flight=None and one would silently overwrite
    // the other (we observed this in production logs: StrictMode +
    // resume-banner re-render produced four "flight adopted" lines for a
    // single PIREP).
    let setup_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;

    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if guard.is_some() {
            return Err(UiError::new(
                "flight_already_active",
                "another flight is already active",
            ));
        }
    }

    let client = current_client(&state)?;
    // v0.16.17: server-seitig auf IN_PROGRESS gefiltert (?state=0) — der
    // gesuchte PIREP kann älter als die ersten 20 paginierten Einträge sein.
    // Der state-Check im find() bleibt als defensive Doppelprüfung.
    let pireps = client.get_user_pireps_in_progress().await?;
    let pirep = pireps
        .into_iter()
        .find(|p| p.id == pirep_id && p.state == Some(0))
        .ok_or_else(|| {
            UiError::new(
                "pirep_not_resumable",
                "PIREP not found or no longer in progress",
            )
        })?;
    // Laeuft der Flug gerade auf einem anderen Geraet, nie stillschweigend
    // uebernehmen — zwei Clients meldeten sonst denselben PIREP.
    if laeuft_woanders(&pirep, Utc::now()) && trotz_anderem_geraet != Some(true) {
        return Err(UiError::new(
            "laeuft_woanders",
            "Dieser Flug meldet gerade von einem anderen Gerät. Erst dort beenden oder ausdrücklich übernehmen.",
        ));
    }

    // Best-effort: look for a matching bid so we can carry fares forward to
    // the eventual file. If no bid matches, file with no fares — phpVMS keeps
    // whatever was set during prefile.
    let flight_number = pirep
        .flight_number
        .clone()
        .ok_or_else(|| UiError::new("pirep_invalid", "PIREP has no flight number"))?;
    let dpt_airport = pirep.dpt_airport_id.clone().unwrap_or_default();
    let arr_airport = pirep.arr_airport_id.clone().unwrap_or_default();

    let bids = client.get_bids().await.unwrap_or_default();
    // Externe QS (Codex, 16.09.2026): Die Flugnummer allein kann
    // mehrdeutig sein (Hin- und Rueckflug, mehrere Bids). Start und Ziel
    // stehen hier zur Verfuegung — also mitpruefen, sonst kann die
    // Plandistanz vom falschen Flug stammen.
    let passt_genau = |b: &&api_client::Bid| {
        b.flight.flight_number == flight_number
            && b.flight.dpt_airport_id.eq_ignore_ascii_case(&dpt_airport)
            && b.flight.arr_airport_id.eq_ignore_ascii_case(&arr_airport)
    };
    let matching_bid = bids.iter().find(passt_genau).or_else(|| {
        bids.iter()
            .find(|b| b.flight.flight_number == flight_number)
    });
    let bid_id = matching_bid.map(|b| b.id).unwrap_or(0);
    let fares: Vec<(i64, i32)> = matching_bid
        .and_then(|b| b.flight.simbrief.as_ref())
        .and_then(|sb| sb.subfleet.as_ref())
        .map(|sf| {
            sf.fares
                .iter()
                .filter_map(|f| f.count.map(|c| (f.id, c)))
                .collect()
        })
        .unwrap_or_default();

    // Prefer the pilot's PaxStudio-booked split (and set the authoritative OFP<->PIREP
    // link) over the core subfleet distribution — same as flight_start. Best-effort.
    let fares = resolve_paxstudio_fares(
        &client,
        matching_bid
            .and_then(|b| b.flight.simbrief.as_ref())
            .map(|s| s.id.as_str()),
        &pirep.id,
        fares,
    )
    .await;

    let airline_icao = matching_bid
        .and_then(|b| b.flight.airline.as_ref())
        .map(|a| a.icao.clone())
        .unwrap_or_default();
    let airline_logo_url = matching_bid
        .and_then(|b| b.flight.airline.as_ref())
        .and_then(|a| a.logo.clone())
        .filter(|s| !s.is_empty());

    // Look up planned registration so the activity log can compare it
    // against the live `ATC ID` SimVar — pilot sees instantly if the
    // wrong tail number is loaded in MSFS.
    // Bid.flight has no direct aircraft_id; the chosen aircraft lives
    // on the SimBrief OFP. If the pilot hasn't generated an OFP, we
    // simply leave planned_registration empty.
    // v0.3.0: zusätzlich Aircraft-ICAO + Name aus dem gleichen Call
    // ziehen. Damit der PIREP-Custom-Field "Aircraft Type" gefüllt
    // werden kann ohne extra Lookup auf der phpVMS-Detail-Seite.
    let aircraft_details = match matching_bid
        .and_then(|b| b.flight.simbrief.as_ref())
        .and_then(|sb| sb.aircraft_id)
    {
        Some(id) => client.get_aircraft(id).await.ok(),
        None => None,
    };
    let planned_registration = aircraft_details
        .as_ref()
        .and_then(|a| a.registration.clone())
        .unwrap_or_default()
        .trim()
        .to_string();
    let aircraft_icao = aircraft_details
        .as_ref()
        .and_then(|a| a.icao.clone())
        .unwrap_or_default()
        .trim()
        .to_string();
    let aircraft_name = aircraft_details
        .as_ref()
        .and_then(|a| a.name.clone())
        .unwrap_or_default()
        .trim()
        .to_string();

    let flight = Arc::new(ActiveFlight {
        pirep_id: pirep.id.clone(),
        bid_id,
        // v0.7.7: flight_id aus matching_bid wenn vorhanden, sonst leer.
        // Im Adopt-Pfad kann der Bid bereits weg sein (W5: phpVMS-7
        // entfernt Bid nach Prefile) — dann ist flight_id leer und
        // v0.7.8 muss diesen Fall behandeln.
        flight_id: matching_bid
            .map(|b| b.flight_id.clone())
            .unwrap_or_default(),
        // v0.7.8 v1.5: Match-Kandidaten — Adopt-Pfad, Bid kann weg sein.
        bid_callsign: matching_bid
            .and_then(|b| b.flight.callsign.clone())
            .filter(|s| !s.trim().is_empty()),
        pilot_callsign: state
            .cached_pilot_callsign
            .lock()
            .expect("cached_pilot_callsign lock")
            .clone(),
        // We don't know the original prefile time; treat "now" as the start
        // for our counters. The PIREP's actual times are intact server-side.
        started_at: Utc::now(),
        airline_icao,
        airline_logo_url,
        planned_registration,
        aircraft_icao,
        aircraft_name,
        flight_number,
        dpt_airport,
        arr_airport,
        planned_flight_time_min: None, // Adopt: kein Bid-Kontext
        // v1.7.32: Beim Adopt gibt es oft doch einen passenden Bid —
        // dann kommt die Plandistanz von dort (externe QS, Codex).
        plan_strecke_nm: matching_bid.and_then(|b| b.flight.distance.as_ref()?.nm()),
        fares,
        stats: stats_sperre::GemesseneSperre::new(FlightStats::new()),
        stop: AtomicBool::new(false),
        // Surfaced via flight_status to trigger the resume banner.
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

    // Codex-Folgefund (adversarial, 05.09.2026, sechste Runde): VOR dem
    // ERSTEN Speichern setzen, nicht erst wenn der Flug weiter unten in
    // `state.active_flight` installiert wird — sonst traegt GENAU dieser
    // erste Checkpoint noch den Eigentuemer des VORHERIGEN Fluges (oder gar
    // keinen), weil `active_flight_owner_identity` beim Beenden eines
    // Fluges nicht geleert wird. Ein Absturz vor dem naechsten periodischen
    // Speichern haette diesen falschen Eigentuemer auf der Platte
    // eingefroren — beim naechsten Start haette `try_resume_flight` ihn als
    // massgeblich behandelt und die serverseitige Reklamier-Pruefung
    // uebersprungen.
    *state
        .active_flight_owner_identity
        .lock()
        .expect("active_flight_owner_identity lock") = state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
        .map(|id| id.to_string());
    // Kabinenansagen (Beta): ohne passenden Bid bleiben beide Felder leer.
    if let Some(b) = matching_bid {
        kabinen_eckdaten_aus_bid(&mut flight.stats.lock().expect("flight stats lock"), &b.flight);
    }
    save_active_flight(&app, &flight);
    // v0.8.0: parallel-fetch dep/arr/alt-Navdata vom VPS. Non-blocking
    // (Background-Task), Failure → OurAirports-Fallback (transparent).
    // Adopt-Pfad: matching_bid kann weg sein, dann fehlt nur der
    // Alternate — dep+arr kommen immer aus dem ActiveFlight.
    {
        // ⚠ Vor dem ersten Anmelden: Der neue Flug erbt sonst den
        // Anfragezustand des vorigen.
        szenerie_buch_zuruecksetzen(&app);
        let mut icaos = vec![flight.dpt_airport.clone(), flight.arr_airport.clone()];
        if let Some(alt) = matching_bid.and_then(|b| b.flight.alt_airport_id.clone()) {
            if !alt.trim().is_empty() {
                icaos.push(alt);
            }
        }
        spawn_navdata_fetch(&app, &flight, icaos);
    }
    {
        let mut guard = state.active_flight.lock().expect("active_flight lock");
        *guard = Some(Arc::clone(&flight));
    }
    // ActiveFlight is now committed — release the in-progress flag so the
    // active_flight mutex alone guards subsequent adopts.
    setup_guard.disarm();
    // Don't spawn the streamer yet — the resume banner is in countdown mode
    // and the user can still cancel. `flight_resume_confirm` spawns it.
    let _ = client;

    let info = flight_info(flight.as_ref(), false, None);
    log_activity(
        &state,
        ActivityLevel::Info,
        format!(
            "Adopted in-progress flight {} ({} – {})",
            format_callsign(&flight.airline_icao, &flight.flight_number),
            flight.dpt_airport,
            flight.arr_airport
        ),
        Some(format!("PIREP {}", flight.pirep_id)),
    );
    record_event(
        &app,
        &flight.pirep_id,
        &FlightLogEvent::FlightStarted {
            timestamp: Utc::now(),
            pirep_id: flight.pirep_id.clone(),
            airline_icao: flight.airline_icao.clone(),
            flight_number: flight.flight_number.clone(),
            dpt_airport: flight.dpt_airport.clone(),
            arr_airport: flight.arr_airport.clone(),
        },
    );
    paket_stand_vermerken(&app, &flight.pirep_id);
    Ok(info)
}

/// Start tracking a flight: prefile a PIREP and begin position streaming.

/// Welcher Fehlercode, wenn phpVMS den Start mit „aircraft-not-available“
/// ablehnt?
///
/// „In use“ / „in flight“ (state 1/2) heißt meist: Ein anderer Pilot hat
/// dieselbe Maschine gebucht und ist zuerst gestartet (D-AEWK, 19.09.2026).
/// Es kann aber auch ein EIGENER hängengebliebener Flugbericht sein — dann
/// wäre „anderer Pilot, neu planen“ der falsche Rat. Deshalb nur mit
/// bekannter eigener PIREP-Liste auf den anderen Piloten schließen
/// (QS/Codex 19.09.2026).
fn flugzeug_belegt_code(
    flugzeug_state: Option<i32>,
    eigene_liste_bekannt: bool,
    eigener_pirep_belegt: bool,
) -> &'static str {
    if !matches!(flugzeug_state, Some(1) | Some(2)) {
        return "aircraft_not_available";
    }
    if eigener_pirep_belegt {
        "aircraft_in_use_own"
    } else if eigene_liste_bekannt {
        "aircraft_in_use"
    } else {
        "aircraft_not_available"
    }
}

/// Darf ein noch offener Flugbericht für diesen Start wiederverwendet werden?
///
/// Die Übernahme existiert für den Wiederaufnahme-Fall: Absturz mitten im
/// Flug, der angefangene Flugbericht liegt noch auf dem Server und blockiert
/// das Flugzeug. Dann ist es richtig, ihn weiterzuverwenden.
///
/// QS 12.08.2026: Geprüft wurden dafür nur Flugnummer und Fluggesellschaft.
/// Das beschreibt den Fall nicht eindeutig — dieselbe Nummer kann für eine
/// andere Strecke oder ein anderes Flugzeug stehen (Umlaufwechsel, geänderter
/// Plan, Rückflug unter gleicher Nummer). Dann erbte der NEUE Flug die
/// Kennung des alten, während der alte Eintrag seine Strecke und sein
/// Flugzeug behielt. Und weil bei einer Übernahme das Vorfilen ganz
/// entfällt, korrigierte das auch niemand mehr.
///
/// Felder, die phpVMS je nach Version und Konfiguration nicht mitliefert,
/// sind vom Vergleich ausgenommen — sonst wäre die Wiederaufnahme auf solchen
/// Installationen komplett tot, und das wäre die schlechtere Kur.
fn pirep_ist_uebernehmbar(
    p: &api_client::PirepSummary,
    body: &PrefileBody,
    airline_id: i64,
) -> bool {
    fn passt_wenn_bekannt(vorhanden: Option<&str>, erwartet: &str) -> bool {
        match vorhanden {
            Some(v) => v.eq_ignore_ascii_case(erwartet),
            None => true, // Feld nicht geliefert → kein Ausschlussgrund
        }
    }
    // phpVMS PirepState IN_PROGRESS = 0 — der Server filtert schon, der
    // Check bleibt als defensive Doppelprüfung.
    p.state == Some(0)
        && p.flight_number.as_deref() == Some(body.flight_number.as_str())
        && (p.airline_id.is_none() || p.airline_id == Some(airline_id))
        && passt_wenn_bekannt(p.dpt_airport_id.as_deref(), body.dpt_airport_id.as_str())
        && passt_wenn_bekannt(p.arr_airport_id.as_deref(), body.arr_airport_id.as_str())
        && match (p.aircraft_id, body.aircraft_id.parse::<i64>().ok()) {
            (Some(alt), Some(neu)) => alt == neu,
            _ => true, // eine der beiden Seiten unbekannt → nicht ausschliessen
        }
}

#[tauri::command]

async fn flight_start(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    bid_id: i64,
    // v0.8.3 (#7): Optional acknowledge-Flag fuer Aircraft-Mismatch.
    // Bei None / Some(false) liefert der Mismatch-Check eine WARNUNG
    // (Error-Code "aircraft_mismatch_warning"), bei Some(true) wird der
    // Check uebersprungen — analog `flight_start_manual`. Erlaubt
    // Wetlease-Workflows (PaxStudio-Loadsheet) ohne Hard-Block.
    #[allow(non_snake_case)] acknowledgeAircraftMismatch: Option<bool>,
) -> Result<ActiveFlightInfo, UiError> {
    flight_start_mit(app, state, bid_id, acknowledgeAircraftMismatch, false).await
}

/// `flight_start` mit der Angabe, ob der Auto-Start ihn ausloest. Nur dann
/// muss der Simulator auch in dem Moment, in dem die Startwerte gelesen
/// werden, ungestoert sein: Zwischen der Ruhepruefung des Watchers und
/// diesem Snapshot liegen Netzabfragen, in denen MSFS pausieren oder neu
/// laden kann (QS Codex, 21.09.2026). Der manuelle Start bleibt, wie er ist.
async fn flight_start_mit(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    bid_id: i64,
    acknowledge_aircraft_mismatch: Option<bool>,
    von_auto_start: bool,
) -> Result<ActiveFlightInfo, UiError> {
    let ack_aircraft_mismatch = acknowledge_aircraft_mismatch.unwrap_or(false);
    // Same race protection as flight_adopt: a double-click on "Start flight"
    // would otherwise prefile two PIREPs against the same bid.
    let setup_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;

    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if guard.is_some() {
            return Err(UiError::new(
                "flight_already_active",
                "another flight is already active",
            ));
        }
    }

    // v0.13.16: VPS-Aircraft-Aliase JETZT auffrischen — nebenläufig, damit
    // ein vom VA-Admin frisch eingetragener Alias (z.B. E55P→PHENOM 300)
    // schon auf DIESEN Startversuch wirkt statt erst nächste Session. Vorher
    // wurden die Aliase nur lazy in `spawn_navdata_fetch` NACH dem Mismatch-
    // Gate gezogen (und bei bereits gecachten Airports ganz übersprungen).
    // Der Fetch überlappt mit get_bids/get_airport/get_aircraft unten → quasi
    // keine Mehrlatenz; awaited direkt vor dem Gate. Best-effort + 5 s-Timeout:
    // hängt den Start nie, bei Fehler/Timeout bleibt der vorhandene Cache gültig.
    let alias_refresh = {
        let app_aliases = app.clone();
        let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                fetch_aircraft_aliases_into_state(app_aliases, None, token),
            )
            .await;
        })
    };

    let client = current_client(&state)?;
    let bids = client.get_bids().await?;
    let bid = bids
        .into_iter()
        .find(|b| b.id == bid_id)
        .ok_or_else(|| UiError::new("bid_not_found", "bid not found in current bids"))?;

    // ---- Pre-flight gating: must be on the ground at the departure airport ----
    let snapshot = current_snapshot(&app).ok_or_else(|| {
        UiError::new(
            "no_sim_snapshot",
            "no sim snapshot yet — is the simulator connected?",
        )
    })?;
    if von_auto_start && (snapshot.paused || snapshot.slew_mode) {
        return Err(UiError::new(
            AUTO_START_SIM_WARTET,
            "simulator paused or loading — auto-start waits until it is settled",
        ));
    }
    if !snapshot.on_ground {
        return Err(UiError::new(
            "not_on_ground",
            "you must be on the ground to start a flight",
        ));
    }

    // Cached or live fetch of the departure airport. The lock is taken in a
    // narrow scope each time so the MutexGuard never crosses an `await`.
    let dpt_icao = bid.flight.dpt_airport_id.trim().to_uppercase();
    let cached_dpt: Option<Airport> = {
        let guard = state.airports.lock().expect("airports lock");
        guard.get(&dpt_icao).cloned()
    };
    let dpt_airport = match cached_dpt {
        Some(a) => a,
        None => {
            let fetched = client.get_airport(&dpt_icao).await?;
            let mut guard = state.airports.lock().expect("airports lock");
            guard.insert(dpt_icao.clone(), fetched.clone());
            fetched
        }
    };
    if let (Some(lat), Some(lon)) = (dpt_airport.lat, dpt_airport.lon) {
        let distance_nm = ::geo::distance_m(snapshot.lat, snapshot.lon, lat, lon) / 1852.0;
        if distance_nm > crate::grenzwerte::grenzwerte().abgabe.max_start_distance_nm.wert {
            return Err(UiError::new(
                "not_at_departure",
                format!(
                    "you are {:.1} nm from {} — start the flight at the departure airport",
                    distance_nm, dpt_icao
                ),
            ));
        }
        tracing::info!(
            dpt = %dpt_icao,
            distance_nm,
            "preflight gate passed"
        );
    } else {
        tracing::warn!(
            dpt = %dpt_icao,
            "no coordinates for departure airport — skipping distance check"
        );
    }

    let airline_id = bid
        .flight
        .airline
        .as_ref()
        .map(|a| a.id)
        .ok_or_else(|| UiError::new("missing_airline", "bid has no airline relation"))?;
    let aircraft_id = bid_flugzeug_id(&bid).ok_or_else(|| {
        UiError::new(
            "missing_aircraft",
            "no aircraft on this bid — please prepare a SimBrief OFP first",
        )
    })?;

    // ---- Aircraft-mismatch gate (spec §7) ----
    // Compare the aircraft type the bid expects (from get_aircraft) to what's
    // loaded in the simulator (parsed ATC MODEL or, as a backup, the TITLE).
    // Permissive: only block when both sides resolve to clearly different codes.
    let expected_aircraft = client.get_aircraft(aircraft_id).await?;
    let expected_icao = expected_aircraft
        .icao
        .as_ref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty());
    let sim_icao = snapshot.aircraft_icao.as_deref().and_then(clean_atc_model);
    let sim_title = snapshot.aircraft_title.as_deref().unwrap_or("").to_string();
    if let (Some(expected), Some(actual)) = (expected_icao.as_ref(), sim_icao.as_ref()) {
        // v0.13.16: sicherstellen, dass der frische VPS-Alias-Pull (oben
        // nebenläufig gestartet) abgeschlossen ist, BEVOR das Gate
        // `aircraft_aliases_vps` liest.
        let _ = alias_refresh.await;
        // v0.8.0: VPS-Aliases-Snapshot fuer alias-aware compare —
        // erlaubt VA-Admin pro Recorder neue Bid↔Sim-Alias-Paare zu
        // pflegen ohne Pilot-Client-Release.
        let vps_aliases = state
            .aircraft_aliases_vps
            .lock()
            .expect("aircraft_aliases_vps lock")
            .clone();
        // v0.13.16: alias-bewusster Vergleich über den gemeinsamen Helper
        // (identisch zum Auto-Start-Watcher). Matcht z.B. "A359" (ICAO)
        // gegen "A350-900" (Sim-Langform) und nutzt VPS-Aliase + Title-
        // Substring-Fallback. Live bug 2026-05-04: Emirates UAE770 A359-Bid
        // geblockt weil Sim "A350-900 (No Cabin)" lud — gleicher Flieger.
        if !aircraft_type_matches_sim(
            Some(expected.as_str()),
            Some(actual.as_str()),
            &sim_title,
            &vps_aliases,
        ) {
            let registration = expected_aircraft.registration.as_deref().unwrap_or("?");
            // v0.8.3 (#7): von Hard-Block zu Warning-with-Acknowledge.
            // Vorher: Aircraft-Mismatch im Standard-Flow (SimBrief) war
            // ein harter „aircraft_mismatch"-Error, der den Flugstart
            // abbrach. Pilot konnte nur via VFR-Mode (`flight_start_manual`)
            // umgehen. Problem: Wetlease-Flows (PaxStudio-Loadsheet
            // erlaubt jedes aktive Aircraft) hatten keine Chance — Pilot
            // musste auf manual switchen.
            //
            // Neuer Default: Warnung mit „Trotzdem starten"-Button,
            // identisch zu `flight_start_manual`. Auf Zweit-Aufruf mit
            // acknowledgeAircraftMismatch=true wird der Check
            // uebersprungen. phpVMS macht sowieso nur einen laxen
            // Title-Substring-Check beim PIREP-File — kein Bedarf,
            // strenger zu sein als der Server.
            if !ack_aircraft_mismatch {
                tracing::warn!(
                    expected = %expected,
                    actual = %actual,
                    title = %sim_title,
                    registration = %registration,
                    "aircraft type mismatch — returning warning (acknowledge to proceed)"
                );
                return Err(UiError::new(
                    "aircraft_mismatch_warning",
                    format!(
                        "Aircraft passt nicht: gewaehlt {expected} ({registration}), Sim hat {actual}. Wetlease? Dann 'Trotzdem starten'. Falsches Flugzeug geladen? Im Sim umladen.",
                    ),
                ));
            }
            tracing::warn!(
                expected = %expected,
                actual = %actual,
                title = %sim_title,
                registration = %registration,
                "aircraft mismatch acknowledged by pilot — proceeding (Wetlease or manual override)"
            );
        }
    }

    let body = PrefileBody {
        // v0.16.18: Flight-ID aus dem Bid mitschicken — phpVMS haengt den
        // PIREP damit an den geplanten Flug (Event-/Tour-Matching, SkyAdventures).
        flight_id: Some(bid.flight.id.clone()),
        airline_id,
        aircraft_id: aircraft_id.to_string(),
        flight_number: bid.flight.flight_number.clone(),
        dpt_airport_id: bid.flight.dpt_airport_id.clone(),
        arr_airport_id: bid.flight.arr_airport_id.clone(),
        alt_airport_id: bid.flight.alt_airport_id.clone(),
        flight_type: bid.flight.flight_type.clone(),
        route_code: bid.flight.route_code.clone(),
        route_leg: bid.flight.route_leg.clone(),
        level: bid.flight.level.filter(|&l| l > 0),
        planned_distance: bid.flight.distance.as_ref().and_then(|d| d.nm()),
        planned_flight_time: bid.flight.flight_time,
        route: bid.flight.route.clone().filter(|s| !s.is_empty()),
        source_name: format!("AeroACARS/{}", env!("CARGO_PKG_VERSION")),
        notes: None,
    };

    // Before trying a fresh prefile, see if the user already has an in-progress
    // PIREP for this flight. This handles the "client crashed / persistence
    // file gone, but phpVMS still has the active PIREP" case — we adopt the
    // existing PIREP instead of trying to create a new one (which would fail
    // with aircraft-not-available because the aircraft is already "in use" by
    // the orphaned PIREP).
    //
    // v0.16.17: server-seitig auf IN_PROGRESS gefiltert (?state=0) — das
    // paginierte get_user_pireps() sah nur die letzten 20 PIREPs, ältere
    // Leichen blieben unsichtbar und blockierten Aircraft dauerhaft.
    let (existing, eigene_liste_bekannt) = match client.get_user_pireps_in_progress().await {
        Ok(list) => (list, true),
        Err(e) => {
            tracing::warn!(error = %e, "could not list user PIREPs to check for resume");
            (Vec::new(), false)
        }
    };
    // v0.16.17 Selbstheilung: >24-h-alte Orphans best-effort wegcanceln,
    // BEVOR der Adopt-/Collision-Check läuft — gibt die Überlebenden zurück.
    let existing = sweep_stale_orphans_before_prefile(&app, &client, existing).await;
    // Belegt ein eigener offener Flugbericht genau dieses Flugzeug? Nur für
    // die Fehlermeldung unten, falls phpVMS den Start ablehnt.
    let eigener_pirep_belegt = existing.iter().any(|p| p.aircraft_id == Some(aircraft_id));
    let adoptable = existing
        .into_iter()
        .find(|p| pirep_ist_uebernehmbar(p, &body, airline_id));

    if let Some(p) = &adoptable {
        tracing::info!(pirep_id = %p.id, "adopting existing in-progress PIREP");
    }

    tracing::info!(
        airline_id,
        aircraft_id,
        flight_number = body.flight_number.as_str(),
        adopting = adoptable.is_some(),
        "prefiling PIREP"
    );
    let pirep = if let Some(adopt) = adoptable {
        api_client::PirepCreated { id: adopt.id }
    } else {
        match client.prefile_pirep(&body).await {
            Ok(p) => p,
            Err(ApiError::Server {
                status: 400,
                body: err_body,
            }) if err_body.contains("aircraft-not-available") => {
                // Diagnose: fetch aircraft details to tell the user *why* it's
                // unavailable (wrong airport, "in use" by an orphan PIREP, etc.).
                // In use / in flight gets its own code — see flugzeug_belegt_code.
                let mut code = "aircraft_not_available";
                let detail = match client.get_aircraft(aircraft_id).await {
                    Ok(a) => {
                        code = flugzeug_belegt_code(
                            a.state,
                            eigene_liste_bekannt,
                            eigener_pirep_belegt,
                        );
                        let reg = a
                            .registration
                            .as_deref()
                            .or(a.name.as_deref())
                            .unwrap_or("?");
                        let where_ = a.airport_id.as_deref().unwrap_or("?");
                        let state = match a.state {
                            Some(0) => "parked",
                            Some(1) => "in use",
                            Some(2) => "in flight",
                            _ => "unknown",
                        };
                        format!(
                        "{reg} (id {}): currently at {where_}, state '{state}'. Wanted at {dpt_icao}.",
                        a.id
                    )
                    }
                    Err(e) => format!("could not fetch aircraft {} details: {e}", aircraft_id),
                };
                tracing::warn!(aircraft_id, %detail, code, "aircraft not available");
                return Err(UiError::new(
                    code,
                    format!("Aircraft not available — {detail}"),
                ));
            }
            Err(ApiError::Server {
                status,
                body: err_body,
            }) => {
                // Try to extract a human-readable message from a phpVMS JSON error body.
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&err_body) {
                    if let Some(title) = json.get("title").and_then(|v| v.as_str()) {
                        return Err(UiError::new(
                            "phpvms_error",
                            format!("phpVMS rejected the flight (HTTP {status}): {title}"),
                        ));
                    }
                }
                return Err(UiError::new(
                    "phpvms_error",
                    format!("phpVMS rejected the flight (HTTP {status})"),
                ));
            }
            Err(e) => return Err(e.into()),
        }
    };

    // Advance the PIREP status to BOARDING and ensure state is IN_PROGRESS so
    // it appears in phpVMS's "Aktive Flüge" view.
    //
    // phpVMS 7 PirepState values: REJECTED = -1, IN_PROGRESS = 0, PENDING = 1,
    // ACCEPTED = 2. We send 0 explicitly so this also recovers any PIREP that
    // accidentally got pushed to PENDING earlier (e.g. by a buggier client).
    let update_body = UpdateBody {
        state: Some(0),
        status: Some("BST".to_string()),
        source_name: Some(format!("AeroACARS/{}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    if let Err(e) = client.update_pirep(&pirep.id, &update_body).await {
        tracing::warn!(
            pirep_id = %pirep.id,
            error = %e,
            "could not advance PIREP status to BOARDING (flight will still be tracked)"
        );
    } else {
        tracing::info!(pirep_id = %pirep.id, "PIREP status set to BOARDING");
    }

    // Capture fares from the SimBrief OFP so we can file accurate loads
    // even if the bid is gone by the time we end the flight.
    let fares: Vec<(i64, i32)> = bid
        .flight
        .simbrief
        .as_ref()
        .and_then(|sb| sb.subfleet.as_ref())
        .map(|sf| {
            sf.fares
                .iter()
                .filter_map(|f| f.count.map(|c| (f.id, c)))
                .collect()
        })
        .unwrap_or_default();

    // Prefer the pilot's actual PaxStudio-booked split (and set the authoritative
    // OFP<->PIREP link server-side) over the core subfleet distribution. Best-effort.
    let fares = resolve_paxstudio_fares(
        &client,
        bid.flight.simbrief.as_ref().map(|s| s.id.as_str()),
        &pirep.id,
        fares,
    )
    .await;

    let planned_registration = expected_aircraft
        .registration
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();
    // v0.3.0: Aircraft-ICAO + Name aus dem expected_aircraft ziehen für
    // den PIREP-Custom-Field "Aircraft Type".
    //
    // v1.7.0 (Spec §8.1b): Der Server ist die beste Quelle — er hat das
    // Flugzeug über seine ID aufgelöst und kennt den Typ aus dem Datensatz
    // der Registrierung. Liefert er ihn ausnahmsweise NICHT, darf hier kein
    // leerer String stehenbleiben: `flight.aircraft_icao` ist die Quelle für
    // die gesamte Bewertung, und leer bedeutet dort „Light-Kategorie" statt
    // „unbekannt". MPH 9 (22.08.2026) kostete das 25 Punkte — PH-MCU steht in
    // der Datenbank mit `icao = MD11`, im aktiven Flug kam trotzdem nichts an.
    //
    // Deshalb zwei Dinge: der Titel des tatsächlich geladenen Flugzeugs als
    // Netz, und eine sichtbare Meldung im Aktivitätsprotokoll. Ein stiller
    // Rückfall ist genau der Fehler, den wir gerade beheben.
    let aircraft_icao = match expected_aircraft
        .icao
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(vom_server) => vom_server.to_string(),
        None => {
            let aus_titel = sim_core::icao_aus_titel(&sim_title);
            match &aus_titel {
                Some(typ) => {
                    tracing::warn!(
                        aircraft_id,
                        registration = %planned_registration,
                        abgeleitet = %typ,
                        "phpVMS liefert keinen ICAO-Typ — aus dem Flugzeug-Titel abgeleitet"
                    );
                    log_activity_handle(
                        &app,
                        ActivityLevel::Warn,
                        format!(
                            "Flugzeugtyp fehlt in phpVMS für {planned_registration} — \
                             aus dem Simulator-Titel abgeleitet: {typ}"
                        ),
                        None,
                    );
                }
                None => {
                    tracing::warn!(
                        aircraft_id,
                        registration = %planned_registration,
                        "phpVMS liefert keinen ICAO-Typ und der Titel gibt nichts her — \
                         die Landebewertung rechnet ohne Musterbezug"
                    );
                    log_activity_handle(
                        &app,
                        ActivityLevel::Warn,
                        format!(
                            "Flugzeugtyp unbekannt für {planned_registration} — \
                             die Landebewertung rechnet ohne Musterbezug"
                        ),
                        None,
                    );
                }
            }
            aus_titel.unwrap_or_default()
        }
    };

    let aircraft_name = expected_aircraft
        .name
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_string();

    // ---- Stage 2: SimBrief OFP fetch for planned-fuel comparison ----
    // The bid carries a SimBrief id when the pilot has prepared an OFP.
    // We fetch the OFP XML now (best-effort, never blocks the flight) so
    // the PIREP can compare actual fuel burn against the dispatcher's
    // plan.
    //
    // v0.4.2: Fetch-Fehler werden zusätzlich ins Activity-Log geschrieben
    // (vorher nur Tracing-Log → unsichtbar für Pilot). Pilot-Report von
    // heute: Flug hatte OFP, aber Landung-Tab zeigte trotzdem keine SOLL-
    // Werte → Fetch war silently fehlgeschlagen, niemand merkte was. Mit
    // Activity-Log-Eintrag sieht der Pilot's beim nächsten Mal sofort.
    //
    // v0.7.17 (N-001): SimBrief-Direct-First. Wenn der Pilot in Settings
    // einen SimBrief-Identifier (User-ID oder Username) hat, holen wir
    // zuerst den AKTUELLSTEN SimBrief-OFP direkt von simbrief.com.
    // Damit greift die "Aktualisieren"-Geste in My-Flights wirklich —
    // bisher hat flight_start die alte Bid-OFP-ID aus phpVMS gezogen,
    // selbst wenn der Pilot in SimBrief gerade einen neuen Plan
    // generiert hatte. Fallback auf den Bid-Pointer wenn Direct
    // fehlschlaegt oder kein Identifier konfiguriert ist.
    let simbrief_settings = state
        .simbrief_settings
        .lock()
        .expect("simbrief_settings lock")
        .clone();
    let mut planned_ofp: Option<api_client::SimBriefOfp> = None;
    if has_simbrief_identifier(&simbrief_settings) {
        let active_dpt = bid.flight.dpt_airport_id.clone();
        let active_arr = bid.flight.arr_airport_id.clone();
        let active_airline_icao = bid
            .flight
            .airline
            .as_ref()
            .map(|a| a.icao.clone())
            .unwrap_or_default();
        let active_flight_number = bid.flight.flight_number.clone();
        let active_bid_callsign = bid.flight.callsign.clone();
        let active_pilot_callsign = state
            .cached_pilot_callsign
            .lock()
            .expect("cached_pilot_callsign lock")
            .clone();
        match try_simbrief_direct_with_match(
            &client,
            &simbrief_settings,
            &active_dpt,
            &active_arr,
            &active_airline_icao,
            &active_flight_number,
            active_bid_callsign.as_deref(),
            active_pilot_callsign.as_deref(),
        )
        .await
        {
            DirectOutcome::Match { ofp } | DirectOutcome::MatchWithCallsignWarning { ofp, .. } => {
                tracing::info!(
                    sb_id = %ofp.request_id,
                    plan_burn_kg = ofp.planned_burn_kg,
                    plan_tow_kg = ofp.planned_tow_kg,
                    "SimBrief direct OFP fetched at flight_start",
                );
                log_activity(
                    &state,
                    ActivityLevel::Info,
                    "SimBrief OFP geladen (direct, aktueller Stand)".to_string(),
                    Some(format!(
                        "Plan-Block {:.0} kg · Trip {:.0} kg · TOW {:.0} kg · OFP {}",
                        ofp.planned_block_fuel_kg,
                        ofp.planned_burn_kg,
                        ofp.planned_tow_kg,
                        ofp.request_id
                    )),
                );
                planned_ofp = Some(ofp);
            }
            DirectOutcome::Mismatch { .. } => {
                // Direct lieferte einen OFP, aber der ist fuer einen
                // anderen Flug (DEP/ARR-Mismatch). Bid-Pointer als
                // Fallback nutzen — ist wahrscheinlich der richtige
                // Plan fuer diesen konkreten Flug.
                tracing::warn!(
                    "SimBrief direct returned mismatching DEP/ARR — falling back to phpVMS bid pointer"
                );
            }
            DirectOutcome::Error(_) => {
                // Netzwerk/Auth-Fehler bei direct — Bid-Pointer als
                // Fallback. Pilot kriegt unten den Pointer-Activity-
                // Log-Eintrag, der reicht.
                tracing::warn!(
                    "SimBrief direct fetch failed at flight_start — falling back to phpVMS bid pointer"
                );
            }
        }
    }
    // Fallback (Direct nicht versucht oder fehlgeschlagen): klassischer
    // Bid-Pointer-Pfad. Identisches Verhalten wie vor v0.7.17.
    let planned_ofp = if planned_ofp.is_some() {
        planned_ofp
    } else if let Some(sb) = bid.flight.simbrief.as_ref() {
        match client.fetch_simbrief_ofp(&sb.id).await {
            Ok(Some(ofp)) => {
                tracing::info!(
                    sb_id = %sb.id,
                    plan_burn_kg = ofp.planned_burn_kg,
                    plan_block_kg = ofp.planned_block_fuel_kg,
                    plan_tow_kg = ofp.planned_tow_kg,
                    "SimBrief OFP fetched"
                );
                log_activity(
                    &state,
                    ActivityLevel::Info,
                    "SimBrief OFP geladen".to_string(),
                    Some(format!(
                        "Plan-Block {:.0} kg · Trip {:.0} kg · TOW {:.0} kg",
                        ofp.planned_block_fuel_kg, ofp.planned_burn_kg, ofp.planned_tow_kg
                    )),
                );
                Some(ofp)
            }
            Ok(None) => {
                tracing::warn!(sb_id = %sb.id, "SimBrief OFP fetch returned no usable data");
                log_activity(
                    &state,
                    ActivityLevel::Warn,
                    "SimBrief-OFP konnte nicht geladen werden".to_string(),
                    Some(format!(
                        "OFP-ID {} liefert keine brauchbaren Daten — Landung-Tab zeigt deshalb keine SOLL-Werte. Bitte einen frischen OFP auf simbrief.com erstellen.",
                        sb.id
                    )),
                );
                None
            }
            Err(e) => {
                tracing::warn!(sb_id = %sb.id, error = %e, "SimBrief OFP fetch failed");
                log_activity(
                    &state,
                    ActivityLevel::Warn,
                    "SimBrief-OFP-Fetch fehlgeschlagen".to_string(),
                    Some(format!(
                        "OFP-ID {} konnte nicht geladen werden ({}). Landung-Tab zeigt deshalb keine SOLL-Werte. Probier später nochmal Refresh in My-Flights, oder generier einen neuen OFP.",
                        sb.id, e
                    )),
                );
                None
            }
        }
    } else {
        // Bid hatte überhaupt keine SimBrief-Verbindung — kein Fehler,
        // aber wir sagen es dem Piloten damit er weiß warum keine
        // SOLL-Werte kommen.
        log_activity(
            &state,
            ActivityLevel::Info,
            "Kein SimBrief-OFP für diesen Flug".to_string(),
            Some("Der Bid hat keinen SimBrief-OFP gelinkt. Du kannst trotzdem fliegen — der Landung-Tab zeigt dann nur die IST-Werte ohne SOLL-IST-Vergleich.".to_string()),
        );
        None
    };

    let flight = Arc::new(ActiveFlight {
        pirep_id: pirep.id.clone(),
        bid_id,
        // v0.7.7: flight_id aus dem Bid extrahieren BEVOR der Bid
        // serverseitig nach Prefile entfernt wird (W5). Diese Stelle
        // sitzt nach `client.prefile_pirep().await` — der Bid kann
        // theoretisch schon weg sein wenn phpVMS-7 ihn synchron loescht,
        // aber wir haben den lokalen `bid` aus dem fruehen `get_bids()`
        // (lib.rs:5151-5155) noch im Scope. flight_id aus dem lokalen
        // Bid-Snapshot ist daher zuverlaessig — egal was server-side
        // mit dem Bid passiert.
        flight_id: bid.flight_id.clone(),
        // v0.7.8 v1.5: Bid.flight.callsign + cached Profile.callsign
        // fuer SimBrief-direct Match-Kandidatenliste (Spec §6.1.2).
        bid_callsign: bid.flight.callsign.clone().filter(|s| !s.trim().is_empty()),
        pilot_callsign: state
            .cached_pilot_callsign
            .lock()
            .expect("cached_pilot_callsign lock")
            .clone(),
        started_at: Utc::now(),
        airline_icao: bid
            .flight
            .airline
            .as_ref()
            .map(|a| a.icao.clone())
            .unwrap_or_default(),
        airline_logo_url: bid
            .flight
            .airline
            .as_ref()
            .and_then(|a| a.logo.clone())
            .filter(|s| !s.is_empty()),
        planned_registration,
        aircraft_icao,
        aircraft_name,
        flight_number: bid.flight.flight_number.clone(),
        dpt_airport: bid.flight.dpt_airport_id.clone(),
        arr_airport: bid.flight.arr_airport_id.clone(),
        planned_flight_time_min: bid.flight.flight_time,
        plan_strecke_nm: bid.flight.distance.as_ref().and_then(|d| d.nm()),
        fares,
        stats: stats_sperre::GemesseneSperre::new(FlightStats::new()),
        stop: AtomicBool::new(false),
        // Fresh start triggered by user — no banner needed.
        was_just_resumed: AtomicBool::new(false),
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

    // Codex-Folgefund (adversarial, 05.09.2026, sechste Runde): VOR dem
    // ERSTEN Speichern setzen, nicht erst wenn der Flug weiter unten in
    // `state.active_flight` installiert wird — sonst traegt GENAU dieser
    // erste Checkpoint noch den Eigentuemer des VORHERIGEN Fluges (oder gar
    // keinen), weil `active_flight_owner_identity` beim Beenden eines
    // Fluges nicht geleert wird. Ein Absturz vor dem naechsten periodischen
    // Speichern haette diesen falschen Eigentuemer auf der Platte
    // eingefroren — beim naechsten Start haette `try_resume_flight` ihn als
    // massgeblich behandelt und die serverseitige Reklamier-Pruefung
    // uebersprungen.
    *state
        .active_flight_owner_identity
        .lock()
        .expect("active_flight_owner_identity lock") = state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
        .map(|id| id.to_string());
    // Kabinenansagen (Beta): Flugart und Airline-Name, auch ohne OFP.
    kabinen_eckdaten_aus_bid(&mut flight.stats.lock().expect("flight stats lock"), &bid.flight);
    save_active_flight(&app, &flight);

    // v1.7.0 Schritt 11 — Spurweite aus der Flugzeugdatei (Spec §5.3 B).
    //
    // EINMAL hier, nicht je Tick: Der Weg dorthin geht ueber einen
    // Verzeichnis-Scan der Flugzeug-Pakete, und der kostet je nach
    // Installation Sekunden. Am Flugbeginn faellt das nicht auf; in der
    // Schleife waere es untragbar. Deshalb im Hintergrund, damit der
    // Flugbeginn nicht darauf wartet.
    //
    // Schlaegt es fehl — kein eindeutiges Paket, verschluesselte Datei,
    // unplausibler Wert — bleibt es bei der Typtabelle. Das ist der
    // Normalfall und kein Mangel: Die Tabelle deckt 98 % des Bestands ab.
    {
        let titel = sim_title.clone();
        let app_fuer_meldung = app.clone();
        // `ActiveFlight` liegt als `Arc` vor — der Klon ist der Zeiger,
        // nicht der Zustand. Denselben Weg nimmt `spawn_navdata_fetch`.
        let flug = flight.clone();
        tauri::async_runtime::spawn(async move {
            let gelesen = tauri::async_runtime::spawn_blocking(move || {
                aircraft_scan::paket_zu_titel(&titel)
                    .and_then(|dir| fahrwerk::spurweite_aus_paket(&dir))
            })
            .await
            .ok()
            .flatten();
            if let Some(f) = gelesen {
                if let Ok(mut s) = flug.stats.lock() {
                    s.fahrwerk_spurweite_m = Some(f.spurweite_m);
                }
                tracing::info!(
                    spurweite_m = f.spurweite_m,
                    quelle = ?f.quelle,
                    "Spurweite aus der Flugzeugdatei gelesen"
                );
                log_activity_handle(
                    &app_fuer_meldung,
                    ActivityLevel::Info,
                    format!("Spurweite aus der Flugzeugdatei: {:.2} m", f.spurweite_m),
                    None,
                );
            }
        });
    }
    // v0.8.0: parallel-fetch dep/arr/alt-Navdata vom VPS. Non-blocking
    // (Background-Task), Failure → OurAirports-Fallback (transparent).
    // flight_start (SimBrief): Alternate kommt aus dem Bid; spätere
    // OFP-Aktualisierungen können einen anderen Alternate setzen —
    // der wird im post-OFP-spawn unten nachgeholt (per-ICAO-guard).
    {
        // ⚠ Vor dem ersten Anmelden: Der neue Flug erbt sonst den
        // Anfragezustand des vorigen.
        szenerie_buch_zuruecksetzen(&app);
        let mut icaos = vec![flight.dpt_airport.clone(), flight.arr_airport.clone()];
        if let Some(alt) = bid.flight.alt_airport_id.clone() {
            if !alt.trim().is_empty() {
                icaos.push(alt);
            }
        }
        spawn_navdata_fetch(&app, &flight, icaos);
    }

    {
        let mut guard = state.active_flight.lock().expect("active_flight lock");
        *guard = Some(Arc::clone(&flight));
    }
    // ActiveFlight is committed; release the setup-in-progress flag.
    setup_guard.disarm();

    // v0.16.12 (#phase-v2): geplante Cruise-Altitude aus dem phpVMS-Flight
    // als `cruise_ref` für die Schatten-Engine. phpVMS-VAs pflegen das
    // Feld mal als Flight-Level (360) und mal in Fuß (36000) — Werte
    // < 1000 werden als FL interpretiert (×100). Unabhängig vom OFP
    // gesetzt (auch Flüge ohne SimBrief haben oft ein Bid-Level).
    if let Some(level) = bid.flight.level.filter(|&l| l > 0) {
        let ft = if level < 1000 {
            f64::from(level) * 100.0
        } else {
            f64::from(level)
        };
        flight
            .stats
            .lock()
            .expect("flight stats lock")
            .planned_cruise_alt_ft = Some(ft);
    }

    // Stage 2: write the SimBrief OFP plan into FlightStats so the
    // streamer + PIREP builder can read it later. Done after commit
    // so stats already exists; the lock is short-held and never
    // crosses an await.
    if let Some(ofp) = planned_ofp {
        // Pull the navlog out before moving the rest into FlightStats so
        // we can post it to phpVMS without holding a lock across an await.
        let waypoints = ofp.waypoints.clone();
        let mut stats = flight.stats.lock().expect("flight stats lock");
        // v0.13.x: Navlog-Fixes für die In-App-Live-Map vorhalten (Dots + TOC/TOD).
        stats.planned_waypoints = waypoints.clone();
        // Fuer die Abflugzeile der Sprit-Tabelle: Das Navlog beginnt oft
        // erst am ersten Streckenpunkt, nicht am Flughafen.
        stats.abflug_icao = Some(
            dpt_airport
                .icao
                .clone()
                .unwrap_or_else(|| dpt_airport.id.clone())
                .trim()
                .to_ascii_uppercase(),
        );
        // v1.7.36: Der Anker (Plan-Reststrecke ab TOD) stammte aus den ALTEN
        // Wegpunkten. Anker und Projektion muessen aus derselben Route kommen
        // (QS-Befund V4, 18.09.2026).
        sprit_plan_neu_lesen(&mut stats);
        stats.ofp_planzeiten = ofp_planzeiten(&ofp);
        kabinen_eckdaten_aus_ofp(&mut stats, &ofp);
        stats.planned_block_fuel_kg = Some(ofp.planned_block_fuel_kg).filter(|&v| v > 0.0);
        stats.planned_burn_kg = Some(ofp.planned_burn_kg).filter(|&v| v > 0.0);
        stats.planned_reserve_kg = Some(ofp.planned_reserve_kg).filter(|&v| v > 0.0);
        // v1.7.35: Sprit-Leiter mitnehmen.
        //
        // Nur Taxi filtert auf `> 0` — dort heisst eine Null „nicht
        // geplant". Contingency, Alternate und Extra filtern auf `>= 0`,
        // weil die Leiter ALLE Bloecke braucht: Ein Flug ohne Extra hat
        // dort echte 0 kg, und die gehoeren in die Grafik, sonst fehlte ein
        // Block ohne Erklaerung. (Der Kommentar sagte bis v1.7.36 pauschal
        // „0 = None" und beschrieb damit nur den Taxi-Fall.)
        stats.planned_taxi_kg = Some(ofp.planned_taxi_kg).filter(|&v| v > 0.0);
        stats.planned_contingency_kg = Some(ofp.planned_contingency_kg).filter(|&v| v >= 0.0);
        stats.planned_alternate_burn_kg = Some(ofp.planned_alternate_burn_kg).filter(|&v| v >= 0.0);
        stats.planned_extra_kg = Some(ofp.planned_extra_kg).filter(|&v| v >= 0.0);
        stats.planned_zfw_kg = Some(ofp.planned_zfw_kg).filter(|&v| v > 0.0);
        stats.planned_tow_kg = Some(ofp.planned_tow_kg).filter(|&v| v > 0.0);
        stats.planned_ldw_kg = Some(ofp.planned_ldw_kg).filter(|&v| v > 0.0);
        // v0.21.x (#phase-v2 Fix A): SimBrief-Reiseflughöhe als Cruise-Ref
        // für die Phasen-v2-Engine — aber NUR als Fallback. Das explizite
        // Bid-Level (oben aus `bid.flight.level` gesetzt) hat Vorrang, und
        // die live PMDG-FMC-Höhe übersteuert erst im Streamer beides. So
        // bleibt die Referenz-Hierarchie sauber:
        // live PMDG-FMC > Bid-Level > SimBrief-OFP > 240s-Zeitgeber.
        if stats.planned_cruise_alt_ft.is_none() {
            stats.planned_cruise_alt_ft = ofp.planned_cruise_alt_ft;
        }
        stats.planned_route = ofp.route;
        stats.planned_atc_callsign = ofp_atc_callsign(&ofp.ofp_flight_number);
        // v0.8.0: OFP-Alternate kann sich vom Bid-Alternate
        // unterscheiden — wenn ja, fetch ihn jetzt nach. Der
        // per-ICAO-Guard im `spawn_navdata_fetch` macht das zu einem
        // No-Op wenn der Wert dem Bid-Alternate gleicht (= schon im
        // Cache).
        let ofp_alt_for_navdata = ofp.alternate.clone();
        stats.planned_alternate = ofp.alternate;
        // v0.3.0: MAX-Werte für Overweight-Detection im Live-Loadsheet
        // + Score-Penalty im Landung-Tab. Filter Null-Werte aus —
        // Custom-Subfleets ohne MAX-Daten kriegen einfach keine
        // Overweight-Anzeige.
        stats.planned_max_zfw_kg = Some(ofp.max_zfw_kg).filter(|&v| v > 0.0);
        stats.planned_max_tow_kg = Some(ofp.max_tow_kg).filter(|&v| v > 0.0);
        stats.planned_max_ldw_kg = Some(ofp.max_ldw_kg).filter(|&v| v > 0.0);
        stats.flight_plan_source = Some("simbrief");
        // v0.7.7: OFP-Identifier persistieren fuer "OFP unveraendert"-
        // Erkennung beim spaeteren `flight_refresh_simbrief`. sb.id ist
        // die SimBrief-OFP-ID die wir gerade geholt haben; ofp_generated_at
        // ist der raw <params><time_generated>-String aus dem XML.
        // Spec docs/spec/ofp-refresh-during-boarding.md §6.1.
        stats.simbrief_ofp_id = bid.flight.simbrief.as_ref().map(|sb| sb.id.clone());
        stats.simbrief_ofp_generated_at = if ofp.ofp_generated_at.is_empty() {
            None
        } else {
            Some(ofp.ofp_generated_at.clone())
        };
        drop(stats);
        // Persist immediately so a Tauri restart mid-flight doesn't
        // lose the plan.
        save_active_flight(&app, &flight);

        // v0.8.0: post-OFP-Alternate-Fetch (per-ICAO-Guard im Helper
        // macht das zum no-op wenn der OFP-Alt dem Bid-Alt entspricht).
        if let Some(alt) = ofp_alt_for_navdata {
            if !alt.trim().is_empty() {
                spawn_navdata_fetch(&app, &flight, vec![alt]);
            }
        }

        // Push the planned waypoints to phpVMS so the live map / PIREP
        // detail can draw the planned track alongside the flown one.
        // Best-effort: failure here doesn't abort the flight setup.
        if !waypoints.is_empty() {
            // v0.16.23: shared Helper (auch von flight_refresh_route_only
            // genutzt) — garantiert bit-identische Route-Form an phpVMS.
            let route = build_route_waypoints(&waypoints);
            let pirep_id = pirep.id.clone();
            let route_client = client.clone();
            tauri::async_runtime::spawn(async move {
                match route_client.post_route(&pirep_id, &route).await {
                    Ok(()) => tracing::info!(
                        pirep_id = %pirep_id,
                        waypoint_count = route.len(),
                        "planned route uploaded"
                    ),
                    Err(e) => tracing::warn!(
                        pirep_id = %pirep_id,
                        error = %e,
                        "planned route upload failed"
                    ),
                }
            });
        }
    }

    // v0.6.0/v0.6.1 — phpVMS-Worker zuerst aufrufen als Hint an den
    // Scheduler dass sein Init-Load aus position_queue.json moeglichst
    // vor dem ersten Streamer-Tick laeuft. **Keine harte Garantie** —
    // tauri::async_runtime::spawn returnt sofort, beide tasks racen
    // dann in der Tauri-Runtime. Ordering-Korrektheit fuer den Live-
    // Map-Track auf phpVMS basiert eh auf dem `at`-Timestamp im
    // PositionEntry-Body (server-seitig sortiert), nicht auf der
    // Reihenfolge in der Memory-Outbox.
    spawn_phpvms_position_worker(app.clone(), Arc::clone(&flight), client.clone());
    spawn_position_streamer(app.clone(), Arc::clone(&flight), client.clone());
    spawn_touchdown_sampler(app.clone(), Arc::clone(&flight));

    // Fire an explicit "Phase: Boarding" entry into phpVMS' acars/logs
    // for the PIREP detail's Fluglogbuch tab. The streamer only emits
    // log lines on phase TRANSITIONS, but Boarding is the initial
    // state — there's no transition INTO it, so without this kick
    // the Fluglogbuch tab starts at "Phase: Pushback" with no record
    // of the boarding period. Best-effort, never blocks flight start.
    // Über den ACARS-Postausgang — bleibt gesichert bis bestätigt.
    acars_einreihen(
        &flight.pirep_id,
        vec![api_client::LogEntry {
            id: None,
            log: "Phase: Boarding".to_string(),
            lat: None,
            lon: None,
            created_at: Some(Utc::now().to_rfc3339()),
        }],
    );

    // New flight = fresh activity log. Pilots were getting confused
    // when the previous flight's events (or yesterday's) lingered at
    // the top of the log. The JSONL flight-event log on disk still
    // captures everything per-PIREP for review, but the in-app feed
    // resets here for a clean recording. Resume (`flight_adopt`)
    // skips this on purpose — there the existing log is the
    // continuation history we want to keep.
    clear_activity_log_for_new_flight(&app);

    let info = flight_info(flight.as_ref(), false, None);
    log_activity(
        &state,
        ActivityLevel::Info,
        format!(
            "Flight started: {} {} – {}",
            format_callsign(&flight.airline_icao, &flight.flight_number),
            flight.dpt_airport,
            flight.arr_airport
        ),
        Some(format!("PIREP prefiled as {}", flight.pirep_id)),
    );
    // Announce the initial Boarding phase explicitly. Without this
    // entry the activity log shows the flight jumping straight from
    // "Flight started" to "Phase: Pushback" later, leaving the
    // Boarding stage of the timeline unrepresented in the textual
    // log even though the UI marks it as the active checkpoint.
    log_activity(
        &state,
        ActivityLevel::Info,
        "Phase: Boarding".to_string(),
        None,
    );
    record_event(
        &app,
        &flight.pirep_id,
        &FlightLogEvent::FlightStarted {
            timestamp: Utc::now(),
            pirep_id: flight.pirep_id.clone(),
            airline_icao: flight.airline_icao.clone(),
            flight_number: flight.flight_number.clone(),
            dpt_airport: flight.dpt_airport.clone(),
            arr_airport: flight.arr_airport.clone(),
        },
    );
    paket_stand_vermerken(&app, &flight.pirep_id);
    Ok(info)
}

// ─── v0.5.27 VFR/Manual-Mode ───────────────────────────────────────────
//
// Fuer VFR-Fluege oder kleine Pisten wo SimBrief keinen OFP erstellen
// kann. Pilot waehlt Aircraft manuell aus dem Fleet + traegt Block-Fuel,
// erwartete Flugzeit + optional Cruise/Route/Alternate ein.

#[derive(Debug, Clone, Serialize)]
pub struct AircraftPickerEntry {
    pub id: i64,
    pub registration: String,
    pub icao: String,
    pub name: String,
    pub airport_id: String,
    /// 0=parked / 1=in_use / 2=in_flight (phpVMS AircraftState).
    pub state: i32,
    /// Searchable display-string ("CL60 D-A3MD Bombardier Challenger 605").
    pub display: String,
}

/// v0.5.30: Liste ALLER verfuegbaren Aircraft fuer den Piloten (= keine
/// Airport-/State-Einschraenkung im Frontend). Pilot waehlt frei aus
/// dem gesamten Fleet, Suche im UI filtert Live.
///
/// `icao`-Parameter wird nur fuer SORT-Priority genutzt: Aircraft am
/// Departure-Airport stehen oben in der Liste, Rest folgt alphabetisch.
/// Phpvms enforced Subfleet-Rank-Restriktion server-seitig (= nur was
/// der Pilot fliegen darf kommt vom /api/fleet zurueck).
///
/// State-Filter entfernt: Pilot sieht auch in-use / maintenance Aircraft
/// (mit visuellem Indikator). Falls ausgewaehlte Aircraft nicht
/// verfuegbar ist, lehnt phpVMS-Prefile mit klarer Fehlermeldung ab.
#[tauri::command]
async fn fleet_list_at_airport(
    state: tauri::State<'_, AppState>,
    icao: String,
) -> Result<Vec<AircraftPickerEntry>, UiError> {
    let client = current_client(&state)?;
    let icao_upper = icao.trim().to_uppercase();

    let raw = client.get_all_aircraft().await?;
    let total_before_filter = raw.len();

    // v0.5.33: Filter — nur Aircraft die der Pilot tatsaechlich
    // jetzt fliegen kann:
    //   • state == 0 (PARKED — nicht IN_USE/IN_AIR)
    //   • status == "A" (ACTIVE — nicht MAINTENANCE/STORED/RETIRED/etc.)
    // phpVMS-Enums:
    //   AircraftState:  0=PARKED, 1=IN_USE, 2=IN_AIR
    //   AircraftStatus: A=ACTIVE, M=MAINTENANCE, S=STORED, R=RETIRED,
    //                   C=SCRAPPED, W=WRITTEN_OFF
    // Aircraft ohne `state`/`status` (legacy/incomplete data) werden
    // konservativ DURCHGELASSEN — phpVMS lehnt beim Prefile ab falls
    // die Aircraft tatsaechlich nicht verfuegbar ist.
    let raw: Vec<_> = raw
        .into_iter()
        .filter(|a| {
            let state_ok = a.state.map(|s| s == 0).unwrap_or(true);
            let status_ok = a
                .status
                .as_deref()
                .map(|s| s.eq_ignore_ascii_case("A"))
                .unwrap_or(true);
            state_ok && status_ok
        })
        .collect();
    tracing::info!(
        before = total_before_filter,
        after = raw.len(),
        "fleet_list_at_airport: state==0 && status==A filter applied"
    );

    let mut entries: Vec<AircraftPickerEntry> = raw
        .into_iter()
        .map(|a| {
            let icao = a.icao.clone().unwrap_or_default();
            let reg = a.registration.clone().unwrap_or_default();
            let name = a.name.clone().unwrap_or_default();
            let display = format!(
                "{} {}{}",
                icao,
                reg,
                if !name.is_empty() && name != icao {
                    format!(" — {name}")
                } else {
                    String::new()
                },
            );
            AircraftPickerEntry {
                id: a.id,
                registration: reg,
                icao,
                name,
                airport_id: a.airport_id.unwrap_or_default(),
                state: a.state.unwrap_or(0),
                display,
            }
        })
        .collect();

    // Sortieren: Aircraft am Departure-Airport zuerst, dann nach State
    // (parked vor in-use), dann alphabetisch nach ICAO+Reg.
    entries.sort_by(|a, b| {
        let a_here = a.airport_id.eq_ignore_ascii_case(&icao_upper);
        let b_here = b.airport_id.eq_ignore_ascii_case(&icao_upper);
        match (a_here, b_here) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a
                .state
                .cmp(&b.state)
                .then_with(|| a.icao.cmp(&b.icao))
                .then_with(|| a.registration.cmp(&b.registration)),
        }
    });

    Ok(entries)
}

/// VFR/Manual-Mode-Flugplan vom Frontend.
#[derive(Debug, Clone, Deserialize)]
pub struct ManualFlightPlan {
    /// MUST: Aircraft-ID aus dem fleet_list_at_airport-Picker.
    pub aircraft_id: i64,
    /// MUST: geplanter Block-Fuel (Ramp Fuel) in kg. Pilot's Schaetzung
    /// fuer den Trip + Reserve. Brauchen wir fuer Fuel-Efficiency-Score.
    pub planned_block_fuel_kg: f32,
    /// MUST: Erwartete Flugzeit in Minuten. Fuer ETA-Anzeige + Janitor-
    /// Heuristik wann ein Flug "abgelaufen" ist.
    pub planned_flight_time_min: i32,
    /// Optional: geplanter Reise-Level in ft (z.B. 4500 fuer VFR).
    pub cruise_level_ft: Option<i32>,
    /// Optional: free-text Route ("direct", "via LMV VFR", waypoint-list).
    pub planned_route: Option<String>,
    /// Optional: Alternate-Airport ICAO.
    pub alt_airport_id: Option<String>,
    /// MUST (v0.5.42+): Geplantes Zero-Fuel-Weight in kg.
    /// Ohne ZFW können wir keinen Loadsheet-Score und kein Mass-Compare
    /// gegen Sim-Live-ZFW berechnen — VFR-Mode verliert sonst seinen
    /// Mehrwert ggü. einem reinen Manual-PIREP. Frontend rendert das
    /// Feld als required, Backend verifiziert nochmal defensiv.
    pub planned_zfw_kg: Option<f32>,
    /// Optional: geplanter Trip-Burn in kg (= planned_block - reserve - alternate).
    /// Wenn nicht gesetzt, fallback: 90% des block_fuel als Trip-Schaetzung.
    pub planned_burn_kg: Option<f32>,
    /// v0.5.36: Pilot hat den Aircraft-Type-Mismatch akzeptiert
    /// (= "trotzdem starten"-Button im Modal nach gelbem Warn-Banner).
    /// Default false → erste Anfrage triggert die Warnung; nach Bestätigung
    /// schickt das Frontend die gleiche Anfrage mit acknowledge=true.
    #[serde(default)]
    pub acknowledge_aircraft_mismatch: bool,
}

/// VFR/Manual-Flight-Start. Parallele Funktion zu flight_start aber
/// OHNE SimBrief-OFP-Pflicht. Pilot bringt aircraft_id + minimale
/// Plan-Daten manuell mit. Rest des Flows identisch (PIREP-Prefile,
/// Streamer-Spawn, Touchdown-Sampler, Activity-Log).
#[tauri::command]
async fn flight_start_manual(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    bid_id: i64,
    plan: ManualFlightPlan,
) -> Result<ActiveFlightInfo, UiError> {
    // v0.5.42: Validierung der Pflicht-Felder VOR dem try_acquire ziehen.
    // Vorher passierte das nach dem Lock — Effekt: wenn parallel ein anderer
    // Flow den Lock hielt (z.B. try_resume_flight beim App-Boot, Auto-Start-
    // Watcher, StrictMode-Doppelmount), bekam der User die kryptische Meldung
    // "another flight start or adopt is already in progress" obwohl tatsächlich
    // nur ein Pflichtfeld fehlte. Validation muss zuerst laufen damit
    // Form-Fehler immer als Form-Fehler erscheinen.
    if plan.planned_block_fuel_kg <= 0.0 {
        return Err(UiError::new(
            "invalid_block_fuel",
            "Block-Fuel muss > 0 kg sein",
        ));
    }
    if plan.planned_flight_time_min <= 0 {
        return Err(UiError::new(
            "invalid_flight_time",
            "Erwartete Flugzeit muss > 0 Minuten sein",
        ));
    }
    // v0.7.1 Phase 2 F1 (Spec docs/spec/v0.7.1-landing-ux-fairness.md
    // P1.1-A + P1.3-B): ZFW ist NICHT mehr Pflicht. VFR-/Manual-Mode-
    // Piloten ohne Dispatch-Daten koennen jetzt starten.
    //   - None      → Loadsheet-Sub-Score wird skipped (siehe sub_loadsheet
    //                 in landing-scoring Crate)
    //   - Some(0.0) oder negativ → Eingabefehler-Schutz, klarer Error-Code
    //                 invalid_zfw_value (Frontend mapped diesen via
    //                 ManualFlightModal knownCodes-Map auf i18n)
    //   - Some(>0)  → wie bisher, planned_tow = zfw + block_fuel
    let planned_zfw: Option<f32> = match plan.planned_zfw_kg {
        Some(v) if v > 0.0 => Some(v),
        Some(v) if v <= 0.0 => {
            return Err(UiError::new(
                "invalid_zfw_value",
                "ZFW muss > 0 sein wenn angegeben. Lass das Feld leer fuer VFR/Manual ohne Loadsheet-Wertung.",
            ));
        }
        _ => None,
    };
    let _ = planned_zfw; // Wird unten bei Manual-TOW-Zuweisung benutzt
                         // Noisy-cast-Hinweis: NaN-Faelle werden vom is-pattern-match
                         // in der `Some(v)` arm abgedeckt (NaN ist niemals > 0.0 noch <= 0.0,
                         // also Some-NaN faellt in den catchall `_` und wird wie None behandelt).

    let setup_guard = FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)?;

    {
        let guard = state.active_flight.lock().expect("active_flight lock");
        if guard.is_some() {
            return Err(UiError::new(
                "flight_already_active",
                "another flight is already active",
            ));
        }
    }

    // v0.13.16: VPS-Aircraft-Aliase nebenläufig auffrischen (analog
    // flight_start), damit ein frisch eingetragener Alias auf DIESEN
    // VFR/Manual-Startversuch wirkt. Best-effort + 5 s-Timeout, awaited
    // direkt vor dem Mismatch-Gate.
    let alias_refresh = {
        let app_aliases = app.clone();
        let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(
                Duration::from_secs(5),
                fetch_aircraft_aliases_into_state(app_aliases, None, token),
            )
            .await;
        })
    };

    let client = current_client(&state)?;
    let bids = client.get_bids().await?;
    let bid = bids
        .into_iter()
        .find(|b| b.id == bid_id)
        .ok_or_else(|| UiError::new("bid_not_found", "bid not found in current bids"))?;

    // Pre-flight gating: ground + departure-airport (gleicher Code wie flight_start)
    let snapshot = current_snapshot(&app).ok_or_else(|| {
        UiError::new(
            "no_sim_snapshot",
            "no sim snapshot yet — is the simulator connected?",
        )
    })?;
    if !snapshot.on_ground {
        return Err(UiError::new(
            "not_on_ground",
            "you must be on the ground to start a flight",
        ));
    }
    let dpt_icao = bid.flight.dpt_airport_id.trim().to_uppercase();
    let cached_dpt: Option<Airport> = {
        let guard = state.airports.lock().expect("airports lock");
        guard.get(&dpt_icao).cloned()
    };
    let dpt_airport = match cached_dpt {
        Some(a) => a,
        None => {
            let fetched = client.get_airport(&dpt_icao).await?;
            let mut guard = state.airports.lock().expect("airports lock");
            guard.insert(dpt_icao.clone(), fetched.clone());
            fetched
        }
    };
    if let (Some(lat), Some(lon)) = (dpt_airport.lat, dpt_airport.lon) {
        let distance_nm = ::geo::distance_m(snapshot.lat, snapshot.lon, lat, lon) / 1852.0;
        if distance_nm > crate::grenzwerte::grenzwerte().abgabe.max_start_distance_nm.wert {
            return Err(UiError::new(
                "not_at_departure",
                format!(
                    "you are {:.1} nm from {} — start the flight at the departure airport",
                    distance_nm, dpt_icao
                ),
            ));
        }
    }

    let airline_id = bid
        .flight
        .airline
        .as_ref()
        .map(|a| a.id)
        .ok_or_else(|| UiError::new("missing_airline", "bid has no airline relation"))?;
    let aircraft_id = plan.aircraft_id;

    // Aircraft-mismatch-Gate gegen Sim-loaded Aircraft
    let expected_aircraft = client.get_aircraft(aircraft_id).await?;
    let expected_icao = expected_aircraft
        .icao
        .as_ref()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty());
    let sim_icao = snapshot.aircraft_icao.as_deref().and_then(clean_atc_model);
    let sim_title = snapshot.aircraft_title.as_deref().unwrap_or("").to_string();
    // v0.5.36: VFR/Manual-Mode → Aircraft-Mismatch ist eine WARNUNG,
    // kein Hard-Block. Pilot hat im Picker bewusst eine Aircraft gewählt;
    // wir vertrauen ihm. Auf Erst-Anfrage liefern wir den Mismatch als
    // dedicated Error-Code "aircraft_mismatch_warning" zurück damit das
    // Frontend ein gelbes Banner mit "Trotzdem starten"-Button zeigt.
    // Beim zweiten Versuch mit acknowledge=true wird der Check übersprungen.
    if let (Some(expected), Some(actual)) = (expected_icao.as_ref(), sim_icao.as_ref()) {
        // v0.13.16: frischen VPS-Alias-Pull abwarten, bevor das Gate liest.
        let _ = alias_refresh.await;
        let vps_aliases = state
            .aircraft_aliases_vps
            .lock()
            .expect("aircraft_aliases_vps lock")
            .clone();
        // v0.13.16: alias-bewusster Vergleich über den gemeinsamen Helper.
        if !aircraft_type_matches_sim(
            Some(expected.as_str()),
            Some(actual.as_str()),
            &sim_title,
            &vps_aliases,
        ) {
            if !plan.acknowledge_aircraft_mismatch {
                let registration = expected_aircraft.registration.as_deref().unwrap_or("?");
                return Err(UiError::new(
                    "aircraft_mismatch_warning",
                    format!(
                        "Aircraft passt nicht: gewählt {expected} ({registration}), Sim hat {actual}. Trotzdem starten? (VFR-Mode vertraut deiner Wahl — der Sim-Type beeinflusst aber Score-Limits.)",
                    ),
                ));
            }
            // Acknowledged — log it for diagnostics but proceed
            tracing::warn!(
                expected = %expected,
                actual = %actual,
                "VFR/Manual: aircraft mismatch acknowledged by pilot — proceeding"
            );
        }
    }

    // PIREP prefile (gleiche Logik wie flight_start, ohne SB-Felder)
    let body = PrefileBody {
        // v0.16.18: Flight-ID aus dem Bid mitschicken — phpVMS haengt den
        // PIREP damit an den geplanten Flug (Event-/Tour-Matching, SkyAdventures).
        flight_id: Some(bid.flight.id.clone()),
        airline_id,
        aircraft_id: aircraft_id.to_string(),
        flight_number: bid.flight.flight_number.clone(),
        dpt_airport_id: bid.flight.dpt_airport_id.clone(),
        arr_airport_id: bid.flight.arr_airport_id.clone(),
        alt_airport_id: plan
            .alt_airport_id
            .clone()
            .or(bid.flight.alt_airport_id.clone()),
        flight_type: bid.flight.flight_type.clone(),
        route_code: bid.flight.route_code.clone(),
        route_leg: bid.flight.route_leg.clone(),
        level: plan.cruise_level_ft.filter(|&l| l > 0),
        planned_distance: bid.flight.distance.as_ref().and_then(|d| d.nm()),
        planned_flight_time: Some(plan.planned_flight_time_min),
        route: plan.planned_route.clone().filter(|s| !s.is_empty()),
        source_name: format!("AeroACARS/{}", env!("CARGO_PKG_VERSION")),
        notes: Some(format!(
            "Manual/VFR-Mode (kein SimBrief-OFP). Block: {:.0} kg, ETA: {} min",
            plan.planned_block_fuel_kg, plan.planned_flight_time_min,
        )),
    };

    // v0.16.17: server-seitig auf IN_PROGRESS gefiltert (?state=0) statt
    // page-1-fetch-then-filter (siehe flight_start) + Selbstheilung: alte
    // Orphans (>24 h) werden vor dem Adopt-/Collision-Check weggeräumt.
    let existing = client
        .get_user_pireps_in_progress()
        .await
        .unwrap_or_default();
    let existing = sweep_stale_orphans_before_prefile(&app, &client, existing).await;
    let adoptable = existing.into_iter().find(|p| {
        // state-Check defensiv — Server hat bereits gefiltert.
        p.state == Some(0)
            && p.flight_number.as_deref() == Some(body.flight_number.as_str())
            && (p.airline_id.is_none() || p.airline_id == Some(airline_id))
    });
    let pirep = if let Some(adopt) = adoptable {
        api_client::PirepCreated { id: adopt.id }
    } else {
        client.prefile_pirep(&body).await.map_err(|e| match e {
            ApiError::Server { status: 400, body: ref err_body }
                if err_body.contains("aircraft-not-available") =>
            {
                UiError::new(
                    "aircraft_not_available",
                    "Aircraft nicht verfügbar — anderer Pilot fliegt es vielleicht oder es ist in Maintenance.",
                )
            }
            other => other.into(),
        })?
    };

    let update_body = UpdateBody {
        state: Some(0),
        status: Some("BST".to_string()),
        source_name: Some(format!("AeroACARS/{}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    };
    if let Err(e) = client.update_pirep(&pirep.id, &update_body).await {
        tracing::warn!(pirep_id = %pirep.id, error = %e, "update_pirep (set BST/state 0) failed (non-fatal)");
    }

    // Aircraft-Daten + Airline-Logo holen
    let aircraft_details = client.get_aircraft(aircraft_id).await.ok();
    let planned_registration = aircraft_details
        .as_ref()
        .and_then(|a| a.registration.clone())
        .unwrap_or_default()
        .trim()
        .to_string();
    // v1.7.0 (Spec §8.1b): wie im Haupt-Startpfad — ein leerer Typ bedeutet
    // in der Bewertung "Light-Kategorie", nicht "unbekannt". Deshalb der
    // Titel als Netz und eine sichtbare Meldung statt eines stillen Rückfalls.
    let aircraft_icao = match aircraft_details
        .as_ref()
        .and_then(|a| a.icao.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(vom_server) => vom_server.to_string(),
        None => {
            let titel = snapshot.aircraft_title.as_deref().unwrap_or("");
            let aus_titel = sim_core::icao_aus_titel(titel);
            match &aus_titel {
                Some(typ) => log_activity_handle(
                    &app,
                    ActivityLevel::Warn,
                    format!(
                        "Flugzeugtyp fehlt in phpVMS für {planned_registration} — \
                         aus dem Simulator-Titel abgeleitet: {typ}"
                    ),
                    None,
                ),
                None => log_activity_handle(
                    &app,
                    ActivityLevel::Warn,
                    format!(
                        "Flugzeugtyp unbekannt für {planned_registration} — \
                         die Landebewertung rechnet ohne Musterbezug"
                    ),
                    None,
                ),
            }
            aus_titel.unwrap_or_default()
        }
    };
    let aircraft_name = aircraft_details
        .as_ref()
        .and_then(|a| a.name.clone())
        .unwrap_or_default()
        .trim()
        .to_string();
    let airline = bid.flight.airline.as_ref();

    // VFR/Manual-Mode: keine SimBrief-Fares verfuegbar. Empty-list ist
    // OK fuer phpVMS-PIREP-File (= keine Pax/Cargo-Loads).
    let fares: Vec<(i64, i32)> = Vec::new();

    let flight = Arc::new(ActiveFlight {
        pirep_id: pirep.id.clone(),
        bid_id,
        // v0.7.7: flight_id aus Bid (Manual-Mode-Variante).
        flight_id: bid.flight_id.clone(),
        // v0.7.8 v1.5: Match-Kandidaten (Manual-Mode-Variante).
        bid_callsign: bid.flight.callsign.clone().filter(|s| !s.trim().is_empty()),
        pilot_callsign: state
            .cached_pilot_callsign
            .lock()
            .expect("cached_pilot_callsign lock")
            .clone(),
        started_at: Utc::now(),
        airline_icao: airline.map(|a| a.icao.clone()).unwrap_or_default(),
        airline_logo_url: airline
            .and_then(|a| a.logo.clone())
            .filter(|s| !s.is_empty()),
        planned_registration,
        aircraft_icao,
        aircraft_name,
        flight_number: bid.flight.flight_number.clone(),
        dpt_airport: bid.flight.dpt_airport_id.clone(),
        arr_airport: bid.flight.arr_airport_id.clone(),
        planned_flight_time_min: bid.flight.flight_time,
        plan_strecke_nm: bid.flight.distance.as_ref().and_then(|d| d.nm()),
        fares,
        stats: stats_sperre::GemesseneSperre::new(FlightStats::new()),
        stop: AtomicBool::new(false),
        was_just_resumed: AtomicBool::new(false),
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

    // Codex-Folgefund (adversarial, 05.09.2026, sechste Runde): VOR dem
    // ERSTEN Speichern setzen, nicht erst wenn der Flug weiter unten in
    // `state.active_flight` installiert wird — sonst traegt GENAU dieser
    // erste Checkpoint noch den Eigentuemer des VORHERIGEN Fluges (oder gar
    // keinen), weil `active_flight_owner_identity` beim Beenden eines
    // Fluges nicht geleert wird. Ein Absturz vor dem naechsten periodischen
    // Speichern haette diesen falschen Eigentuemer auf der Platte
    // eingefroren — beim naechsten Start haette `try_resume_flight` ihn als
    // massgeblich behandelt und die serverseitige Reklamier-Pruefung
    // uebersprungen.
    *state
        .active_flight_owner_identity
        .lock()
        .expect("active_flight_owner_identity lock") = state
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
        .map(|id| id.to_string());
    // Kabinenansagen (Beta): Flugart und Airline-Name, auch ohne OFP.
    kabinen_eckdaten_aus_bid(&mut flight.stats.lock().expect("flight stats lock"), &bid.flight);
    save_active_flight(&app, &flight);
    // v0.8.0: parallel-fetch dep/arr/alt-Navdata vom VPS. Non-blocking
    // (Background-Task), Failure → OurAirports-Fallback (transparent).
    // Manual-Pfad: Alternate kommt aus der Plan-Eingabe.
    {
        // ⚠ Vor dem ersten Anmelden: Der neue Flug erbt sonst den
        // Anfragezustand des vorigen.
        szenerie_buch_zuruecksetzen(&app);
        let mut icaos = vec![flight.dpt_airport.clone(), flight.arr_airport.clone()];
        if let Some(alt) = plan.alt_airport_id.clone() {
            if !alt.trim().is_empty() {
                icaos.push(alt);
            }
        }
        spawn_navdata_fetch(&app, &flight, icaos);
    }
    {
        let mut guard = state.active_flight.lock().expect("active_flight lock");
        *guard = Some(Arc::clone(&flight));
    }
    setup_guard.disarm();

    // Manual-Plan in FlightStats schreiben (analog zu SB-OFP-Path)
    {
        let mut stats = flight.stats.lock().expect("flight stats lock");
        // Auch hier die Kennung fuer die Abflugzeile — sonst steht dort
        // „Abflug" statt des Flughafens (Abnahme 20.09.2026).
        if stats.abflug_icao.is_none() {
            stats.abflug_icao = Some(flight.dpt_airport.trim().to_ascii_uppercase());
        }
        stats.planned_block_fuel_kg = Some(plan.planned_block_fuel_kg);
        // v0.7.1 Phase 2 F2 (Spec docs/spec/v0.7.1-landing-ux-fairness.md):
        // KEIN Fallback mehr. Wenn der Pilot keinen geplanten Trip-Burn aus
        // dem OFP hat, wird kein Fuel-Score vergeben (sub_fuel skipped).
        // Pilot wird nicht mehr fuer eine 90%-Annahme bewertet die er nie
        // selbst geplant hat.
        stats.planned_burn_kg = plan.planned_burn_kg;
        // v0.7.1 Phase 2 F1 (P1.3-B): ZFW ist Option<f32>, planned_tow nur
        // wenn ZFW vorhanden. planned_ldw bleibt None im Manual-Pfad
        // (kein Trip-Burn-Plan ohne OFP).
        stats.planned_zfw_kg = planned_zfw;
        stats.planned_tow_kg = planned_zfw.map(|zfw| zfw + plan.planned_block_fuel_kg);
        stats.planned_ldw_kg = None;
        stats.planned_route = plan.planned_route;
        stats.planned_alternate = plan.alt_airport_id;
        stats.flight_plan_source = Some("manual");
        // v0.16.12 (#phase-v2): VFR-/Manual-Reise-Level (bereits in ft)
        // als `cruise_ref` für die Schatten-Engine.
        stats.planned_cruise_alt_ft = plan.cruise_level_ft.filter(|&l| l > 0).map(f64::from);
        drop(stats);
        save_active_flight(&app, &flight);
    }

    // v0.6.0/v0.6.1 — phpVMS-Worker zuerst spawnen (Hint, keine
    // Garantie — siehe ausfuehrlichen Kommentar in flight_start).
    // Ordering ist server-seitig durch PositionEntry.at sichergestellt.
    spawn_phpvms_position_worker(app.clone(), Arc::clone(&flight), client.clone());
    spawn_position_streamer(app.clone(), Arc::clone(&flight), client.clone());
    spawn_touchdown_sampler(app.clone(), Arc::clone(&flight));
    clear_activity_log_for_new_flight(&app);

    let info = flight_info(flight.as_ref(), false, None);
    log_activity(
        &state,
        ActivityLevel::Info,
        format!(
            "Manual-Mode-Flug gestartet: {} {} – {} (Block {:.0} kg, ETA {} min)",
            format_callsign(&flight.airline_icao, &flight.flight_number),
            flight.dpt_airport,
            flight.arr_airport,
            plan.planned_block_fuel_kg,
            plan.planned_flight_time_min,
        ),
        Some(format!("PIREP prefiled as {}", flight.pirep_id)),
    );
    log_activity(
        &state,
        ActivityLevel::Info,
        "Phase: Boarding".to_string(),
        None,
    );
    record_event(
        &app,
        &flight.pirep_id,
        &FlightLogEvent::FlightStarted {
            timestamp: Utc::now(),
            pirep_id: flight.pirep_id.clone(),
            airline_icao: flight.airline_icao.clone(),
            flight_number: flight.flight_number.clone(),
            dpt_airport: flight.dpt_airport.clone(),
            arr_airport: flight.arr_airport.clone(),
        },
    );
    paket_stand_vermerken(&app, &flight.pirep_id);
    Ok(info)
}

// Minimum thresholds a flight has to pass before we'll let it file as a clean
// ACARS PIREP. Anything below these is either a buggy run, a half-baked test,
// or a flight that never actually flew — phpVMS shouldn't get those.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> min_flight_time_min (v2.1.24: 1)
// Wert seit v3: bundle/grenzwerte/abgabe.json -> min_distance_nm (v2.1.24: 1.0)
// Wert seit v3: bundle/grenzwerte/abgabe.json -> min_position_count (v2.1.24: 5)
