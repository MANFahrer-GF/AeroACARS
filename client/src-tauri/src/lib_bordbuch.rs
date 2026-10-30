/// Einstellungen je Pilot, zwischengespeichert — der Takt liest sie alle
/// paar Sekunden, die Datei ändert sich selten.
static BORDBUCH_EINST_CACHE: std::sync::Mutex<Option<(String, bordbuch::Einstellungen)>> =
    std::sync::Mutex::new(None);

fn bordbuch_pilot(app: &AppHandle) -> String {
    app.state::<AppState>()
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock")
        .map(|id| id.to_string())
        .unwrap_or_default()
}

fn bordbuch_speicher(app: &AppHandle) -> Option<bordbuch::Speicher> {
    let dir = app.path().app_data_dir().ok()?;
    bordbuch::Speicher::oeffnen(&dir, &bordbuch_pilot(app)).ok()
}

fn bordbuch_einstellungen(app: &AppHandle) -> bordbuch::Einstellungen {
    let pilot = bordbuch_pilot(app);
    if let Some((p, e)) = BORDBUCH_EINST_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        if *p == pilot {
            return e.clone();
        }
    }
    let e = bordbuch_speicher(app)
        .map(|sp| sp.einstellungen())
        .unwrap_or_default();
    *BORDBUCH_EINST_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some((pilot, e.clone()));
    e
}

fn bordbuch_einstellungen_cache_leeren() {
    *BORDBUCH_EINST_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
}

/// Ein Takt des Bordbuchs. Pause, Slew und Replay-Verdacht zählen nicht.
fn bordbuch_tick(app: &AppHandle, flight: &ActiveFlight, snap: &SimSnapshot) {
    if snap.paused || snap.slew_mode {
        return;
    }
    let einst = bordbuch_einstellungen(app);
    let mut stats = flight.stats.lock().expect("flight stats");
    if stats.replay_verdacht {
        return;
    }
    // `protokoll_phase` statt `effective_phase`: hält eine Warteschleife
    // über den Eintrittstakt hinaus (Befund 25.09.2026, siehe dort).
    let phase = stats.phase;
    if stats.bordbuch.vfr.is_none() {
        if let Some(quelle) = stats.flight_plan_source {
            stats.bordbuch.vfr = Some(quelle == "manual");
        }
    }
    if stats.bordbuch.klasse.is_none() {
        // Erst festlegen, wenn der Sim das Flugzeug kennt — sonst sähe
        // ein Add-on im ersten Takt wie ein Standardflugzeug aus.
        if snap
            .aircraft_title
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            return;
        }
        let muster = stats
            .aufgeloestes_muster
            .clone()
            .or_else(|| snap.aircraft_icao.clone())
            .unwrap_or_else(|| flight.aircraft_icao.clone());
        let kennzeichen = snap
            .aircraft_registration
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| flight.planned_registration.clone());
        let (k, quelle) = bordbuch::klasse_fuer(
            Some(&muster),
            Some(&kennzeichen),
            snap.aircraft_profile,
            &einst,
        );
        stats.bordbuch.klasse = Some(k);
        stats.bordbuch.klasse_quelle = Some(quelle.to_string());
    }
    let kontext = bordbuch::Kontext {
        klasse: stats.bordbuch.klasse.unwrap_or(bordbuch::Klasse::Airliner),
        einstellungen: &einst,
        abflug: Some(flight.dpt_airport.as_str()),
        ziel: Some(flight.arr_airport.as_str()),
    };
    bordbuch::tick(&mut stats.bordbuch, snap, phase, &kontext);
}

/// Flugende: Bordbuch einfrieren und lokal speichern — VOR dem Einreichen,
/// damit auch ein Flug, dessen PIREP offline in die Warteschlange geht,
/// sein Bordbuch hat. Ein zweiter Versuch (Einreichen wiederholt) behält
/// die Markierungen des Piloten.
fn bordbuch_abschliessen(app: &AppHandle, flight: &ActiveFlight) {
    let einst = bordbuch_einstellungen(app);
    let (zustand, muster) = {
        let stats = flight.stats.lock().expect("flight stats");
        (stats.bordbuch.clone(), stats.aufgeloestes_muster.clone())
    };
    if zustand.letzte_zeit.is_none() {
        return; // nie ein Takt (kein Simulator) — nichts zu zeigen
    }
    let muster = muster.unwrap_or_else(|| flight.aircraft_icao.clone());
    let flug = bordbuch::FlugInfo {
        callsign: Some(format_callsign(&flight.airline_icao, &flight.flight_number)),
        dep: Some(flight.dpt_airport.clone()),
        arr: Some(flight.arr_airport.clone()),
        muster: Some(muster.clone()).filter(|m| !m.is_empty()),
        titel: zustand.flugzeug.clone(),
        profil: zustand.profil_name.clone(),
        sim: zustand.simulator.clone(),
    };
    let rueckfall = bordbuch::klasse_fuer(
        Some(&muster),
        Some(&flight.planned_registration),
        sim_core::AircraftProfile::Default,
        &einst,
    );
    let mut eintrag = bordbuch::abschliessen(
        &zustand,
        &flight.pirep_id,
        flug,
        zustand.vfr.unwrap_or(false),
        &einst,
        rueckfall,
        env!("CARGO_PKG_VERSION"),
        Utc::now(),
    );
    eintrag.erstellt_at = flight.started_at;
    let Some(sp) = bordbuch_speicher(app) else {
        return;
    };
    if let Some(alt) = sp.holen(&flight.pirep_id) {
        for p in eintrag.punkte.iter_mut() {
            if let Some(a) = alt
                .punkte
                .iter()
                .find(|a| a.regel == p.regel && a.markiert_at.is_some())
            {
                if p.auto_status == bordbuch::Status::DiesmalOhne {
                    p.status = a.status;
                    p.markiert_at = a.markiert_at;
                }
            }
        }
    }
    if let Err(e) = sp.speichern(eintrag) {
        tracing::warn!(error = %e, "Bordbuch konnte nicht gespeichert werden");
        return;
    }
    let _ = tauri::Emitter::emit(app, "bordbuch_geaendert", flight.pirep_id.clone());
    spawn_bordbuch_abgleich(app);
}

/// Token für den Live-Server — nur, wenn der Schlüsselbund zum angemeldeten
/// Piloten gehört (gleiche Absicherung wie beim Prüfstatus: nach einem
/// Kontowechsel gehört er evtl. noch dem vorigen Piloten).
fn bordbuch_token(app: &AppHandle) -> Option<String> {
    let angemeldet = (*app
        .state::<AppState>()
        .authenticated_pilot_id
        .lock()
        .expect("authenticated_pilot_id lock"))?;
    let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD)
        .ok()
        .flatten()?;
    let nr = secrets::load_api_key(MQTT_KEYRING_PHPVMS_PILOT)
        .ok()
        .flatten();
    mqtt_cache_gehoert_angemeldetem(nr.as_deref(), angemeldet).then_some(token)
}

/// Abgleich mit dem Server: erst holen und einmischen (neuere Fassung
/// gewinnt), dann Ungesichertes hochladen, dann Einstellungen.
async fn bordbuch_abgleich(app: AppHandle) -> Result<usize, String> {
    let Some(token) = bordbuch_token(&app) else {
        return Err("kein Live-Server-Zugang für diesen Piloten".into());
    };
    let Some(sp) = bordbuch_speicher(&app) else {
        return Err("Bordbuch-Speicher nicht verfügbar".into());
    };
    let vom_server: Vec<bordbuch::Eintrag> =
        aeroacars_mqtt::bordbuch::eintraege_holen(None, &token)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
    let neu = sp.zusammenfuehren(vom_server).map_err(|e| e.to_string())?;
    for e in sp.alle().into_iter().filter(|e| !e.synced) {
        let Ok(json) = serde_json::to_value(&e) else {
            continue;
        };
        match aeroacars_mqtt::bordbuch::eintrag_sichern(None, &token, &json).await {
            Ok(()) => {
                let stand = e.updated_at;
                let _ = sp.aendern(&e.pirep_id, |x| {
                    let passt = x.updated_at == stand;
                    if passt {
                        x.synced = true;
                    }
                    passt
                });
            }
            Err(err) => {
                tracing::debug!(error = %err, pirep = %e.pirep_id, "Bordbuch-Eintrag nicht gesichert — nächster Anlass");
            }
        }
    }
    // Einstellungen: die neuere Fassung gewinnt.
    let lokal = sp.einstellungen_vorhanden();
    let server: Option<bordbuch::Einstellungen> =
        aeroacars_mqtt::bordbuch::einstellungen_holen(None, &token)
            .await
            .map_err(|e| e.to_string())?
            .and_then(|v| serde_json::from_value::<bordbuch::Einstellungen>(v).ok())
            .map(bordbuch::Einstellungen::bereinigt);
    match (lokal, server) {
        (l, Some(s))
            if l.as_ref()
                .map(|l| s.updated_at > l.updated_at)
                .unwrap_or(true) =>
        {
            sp.einstellungen_speichern(&s).map_err(|e| e.to_string())?;
            bordbuch_einstellungen_cache_leeren();
        }
        (Some(l), s)
            if s.as_ref()
                .map(|s| l.updated_at > s.updated_at)
                .unwrap_or(true) =>
        {
            if let Ok(json) = serde_json::to_value(&l) {
                aeroacars_mqtt::bordbuch::einstellungen_sichern(None, &token, &json)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        _ => {}
    }
    if neu > 0 {
        let _ = tauri::Emitter::emit(&app, "bordbuch_geaendert", String::new());
    }
    Ok(neu)
}

/// Ein geplanter Abgleich genügt: er liest den Stand erst nach der
/// Wartezeit, fasst also schnelle Aktionen hintereinander zusammen.
static BORDBUCH_ABGLEICH_GEPLANT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn spawn_bordbuch_abgleich(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    if BORDBUCH_ABGLEICH_GEPLANT.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        BORDBUCH_ABGLEICH_GEPLANT.store(false, Ordering::SeqCst);
        if let Err(e) = bordbuch_abgleich(app).await {
            tracing::debug!(error = %e, "Bordbuch-Abgleich verschoben");
        }
    });
}

/// Liste für Logbuch/Routine — ohne Höhenprofil und Rohwerte (die holt die
/// Einzelansicht), damit auch hunderte Flüge schnell ankommen.
#[tauri::command(async)]
fn bordbuch_liste(app: AppHandle) -> Vec<bordbuch::Eintrag> {
    bordbuch_speicher(&app)
        .map(|sp| sp.alle())
        .unwrap_or_default()
        .into_iter()
        .map(|mut e| {
            e.profil.clear();
            for p in e.punkte.iter_mut() {
                p.beleg.clear();
            }
            e
        })
        .collect()
}

#[tauri::command(async)]
fn bordbuch_eintrag(app: AppHandle, pirep_id: String) -> Option<bordbuch::Eintrag> {
    bordbuch_speicher(&app)?.holen(&pirep_id)
}

/// Laufender Flug: was bisher abgehakt ist, dazu ein leiser Hinweis.
#[tauri::command(async)]
fn bordbuch_live(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Option<bordbuch::LiveAnsicht> {
    let flight = state
        .active_flight
        .lock()
        .expect("active_flight lock")
        .as_ref()
        .cloned()?;
    let einst = bordbuch_einstellungen(&app);
    let stats = flight.stats.lock().expect("flight stats");
    let z = &stats.bordbuch;
    let vfr = z
        .vfr
        .or_else(|| stats.flight_plan_source.map(|q| q == "manual"))
        .unwrap_or(false);
    Some(bordbuch::live_ansicht(z, &einst, vfr))
}

/// Pilot tippt einen Punkt an: „nach ATC-Anweisung" (oder zurück).
#[tauri::command(async)]
fn bordbuch_markieren(
    app: AppHandle,
    pirep_id: String,
    regel: bordbuch::Regel,
    nach_atc: bool,
) -> Result<Option<bordbuch::Eintrag>, UiError> {
    let sp = bordbuch_speicher(&app)
        .ok_or_else(|| UiError::new("bordbuch_speicher", "nicht verfügbar".to_string()))?;
    let neu = sp
        .aendern(&pirep_id, |e| e.markieren(regel, nach_atc, Utc::now()))
        .map_err(|e| UiError::new("bordbuch_speichern", e.to_string()))?;
    if neu.is_some() {
        spawn_bordbuch_abgleich(&app);
    }
    Ok(neu)
}

#[tauri::command]
fn bordbuch_einstellungen_holen(app: AppHandle) -> bordbuch::Einstellungen {
    bordbuch_einstellungen(&app)
}

#[tauri::command]
fn bordbuch_einstellungen_setzen(
    app: AppHandle,
    einstellungen: bordbuch::Einstellungen,
) -> Result<bordbuch::Einstellungen, UiError> {
    let mut e = einstellungen.bereinigt();
    e.updated_at = Some(Utc::now());
    let sp = bordbuch_speicher(&app)
        .ok_or_else(|| UiError::new("bordbuch_speicher", "nicht verfügbar".to_string()))?;
    sp.einstellungen_speichern(&e)
        .map_err(|err| UiError::new("bordbuch_speichern", err.to_string()))?;
    bordbuch_einstellungen_cache_leeren();
    spawn_bordbuch_abgleich(&app);
    Ok(e)
}

/// Beim Anmelden: Stand vom Server holen (Neuinstallation, zweiter Rechner).
#[tauri::command]
async fn bordbuch_wiederherstellen(app: AppHandle) -> Result<usize, UiError> {
    bordbuch_einstellungen_cache_leeren();
    bordbuch_abgleich(app)
        .await
        .map_err(|e| UiError::new("bordbuch_abgleich", e))
}
