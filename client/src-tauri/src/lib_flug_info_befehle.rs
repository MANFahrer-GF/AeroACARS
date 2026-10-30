#[derive(Serialize)]
pub struct AirportInfo {
    icao: String,
    name: Option<String>,
    lat: Option<f64>,
    lon: Option<f64>,
}

/// Bodendaten eines Flughafens fuer die Taxi-Karte.
///
/// v0.21: Rollwege, Haltepunkte und Standplaetze aus OpenStreetMap (ODbL), auf
/// dem VPS gespiegelt. Die Karte im Frontend zeichnet das GeoJSON direkt — Rust
/// reicht es nur durch und legt es auf die Platte.
///
/// **Der Cache ist der Punkt.** Ein Flughafen ist klein (EDDF = 71 kB gzip),
/// aber:
///   * Beim zweiten Start schickt der Client den ETag mit; antwortet der Server
///     mit 304, wird nichts uebertragen.
///   * Ohne Netz kommt die Karte aus der lokalen Kopie. Genau dann, im Cockpit
///     mit wackligem WLAN, will man sie nicht verlieren.
///
/// Fehlt der Flughafen auf dem VPS (404), ist das kein Fehler, sondern die
/// Regel: bisher sind nur eine Handvoll importiert. Die Karte zeigt dann
/// schlicht keine Rollwege, statt eine Fehlermeldung ins Cockpit zu werfen.
#[tauri::command]
async fn airport_ground_get(
    app: AppHandle,
    icao: String,
) -> Result<Option<aeroacars_mqtt::navdata::AirportGround>, UiError> {
    let key = icao.trim().to_uppercase();
    if key.len() < 3 || key.len() > 5 {
        return Err(UiError::new("invalid_icao", "ungueltiger ICAO-Code"));
    }

    let cache_path = app
        .path()
        .app_data_dir()
        .map(|d| d.join("ground").join(format!("{key}.json")))
        .ok();

    // Was liegt lokal? Der ETag daraus erspart den Neu-Download.
    let cached: Option<aeroacars_mqtt::navdata::AirportGround> = cache_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok());

    let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
    let known_etag = cached.as_ref().and_then(|c| c.etag.clone());

    match aeroacars_mqtt::navdata::get_airport_ground(
        &key,
        None,
        token.as_deref(),
        known_etag.as_deref(),
    )
    .await
    {
        // 304 → unsere Kopie ist aktuell.
        Ok(None) => Ok(cached),
        Ok(Some(fresh)) => {
            if let Some(p) = cache_path {
                if let Some(dir) = p.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                if let Ok(s) = serde_json::to_string(&fresh) {
                    let _ = std::fs::write(&p, s);
                }
            }
            Ok(Some(fresh))
        }
        // 404 heisst: die Bodendaten sind auf dem VPS NICHT (mehr) da. Eine
        // alte lokale Kopie weiterzubenutzen waere falsch — wir haetten sie
        // womoeglich zurueckgezogen, weil die Daten kaputt waren. Also weg
        // damit. Das ist der Unterschied zu einem Netzfehler, wo die Kopie
        // genau richtig ist.
        Err(aeroacars_mqtt::navdata::NavdataError::NotFound(_)) => {
            if let Some(p) = cache_path {
                let _ = std::fs::remove_file(&p);
            }
            tracing::debug!(icao = %key, "keine Bodendaten fuer diesen Flughafen");
            Ok(None)
        }
        // Kein Netz, kein Token, Server weg: die lokale Kopie ist besser als
        // nichts. Genau dann, im Cockpit mit wackligem WLAN, will man sie
        // nicht verlieren.
        Err(e) => {
            if cached.is_some() {
                tracing::debug!(icao = %key, error = %e, "Bodendaten: nutze lokale Kopie");
                Ok(cached)
            } else {
                tracing::debug!(icao = %key, error = %e, "Bodendaten nicht verfuegbar");
                Ok(None)
            }
        }
    }
}

/// Welche Flughaefen haben eine Taxi-Karte — und wo liegen sie?
///
/// v0.21.0-beta.2: Der erste Wurf lud die Karte nur fuer Start- und Ziel-
/// flughafen eines LAUFENDEN Flugs. Steht der Pilot ohne Bid am Gate — genau der
/// Fall, in dem er auf die Karte schaut — blieb sie leer. Auch nach einem
/// Ausweichflug fehlte die Karte des Platzes, an dem man tatsaechlich landet.
///
/// Jetzt zieht die Karte diesen Index und laedt die Rollwege fuer JEDEN
/// Flughafen, der gerade im Bild ist. Schiebt der Pilot die Karte woandershin,
/// erscheinen dort die Rollwege. Die Liste ist winzig (ICAO + zwei Zahlen), die
/// eigentlichen Daten kommen erst, wenn ein Flughafen wirklich sichtbar wird —
/// und liegen danach im lokalen Zwischenspeicher.
#[tauri::command]
async fn airport_ground_index() -> Result<Vec<aeroacars_mqtt::navdata::GroundIndexEntry>, UiError> {
    let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
    match aeroacars_mqtt::navdata::get_ground_index(None, token.as_deref()).await {
        Ok(i) => Ok(i),
        // Kein Netz, kein Token: dann eben keine Karte. Kein Grund, das Cockpit
        // mit einer Fehlermeldung zu stoeren.
        Err(e) => {
            tracing::debug!(error = %e, "Bodendaten-Index nicht abrufbar");
            Ok(Vec::new())
        }
    }
}

/// Fetch an airport by ICAO, caching the result so we don't re-hit the network
/// on each sim snapshot.
#[tauri::command]
async fn airport_get(
    state: tauri::State<'_, AppState>,
    icao: String,
) -> Result<AirportInfo, UiError> {
    let key = icao.trim().to_uppercase();
    // Block-scope the lock so the MutexGuard is dropped before any `await`,
    // keeping the future `Send`.
    let cached: Option<Airport> = {
        let guard = state.airports.lock().expect("airports lock");
        guard.get(&key).cloned()
    };
    if let Some(c) = cached {
        return Ok(AirportInfo {
            icao: key,
            name: c.name,
            lat: c.lat,
            lon: c.lon,
        });
    }
    let client = current_client(&state)?;
    let airport = client.get_airport(&key).await?;
    let info = AirportInfo {
        icao: key.clone(),
        name: airport.name.clone(),
        lat: airport.lat,
        lon: airport.lon,
    };
    {
        let mut guard = state.airports.lock().expect("airports lock");
        guard.insert(key, airport);
    }
    Ok(info)
}

// ---- Flight workflow ----

/// v0.15.19: Gleitwinkel der Approach-Runway aus einer NavRunway-Liste, mit
/// derselben Plausibilitäts-Clamp (2–7,5°) wie der Scorer. Matcht den
/// Runway-Designator exakt, sonst über die numerische Bahn-Nummer (Parallel-
/// bahnen wie 27L/27R teilen den publizierten Gleitweg). Pure → unit-testbar.
fn runway_glideslope_for(
    runways: &[aeroacars_mqtt::navdata::NavRunway],
    approach_runway: &str,
) -> Option<f64> {
    let want = approach_runway
        .trim()
        .trim_start_matches("RW")
        .to_uppercase();
    if want.is_empty() {
        return None;
    }
    let want_num: String = want.chars().take_while(|c| c.is_ascii_digit()).collect();
    let nr = runways
        .iter()
        .find(|nr| nr.designator.trim().to_uppercase() == want)
        .or_else(|| {
            if want_num.is_empty() {
                return None;
            }
            runways.iter().find(|nr| {
                let dn: String = nr
                    .designator
                    .trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                dn == want_num
            })
        })?;
    Some(nr.glideslope_angle).filter(|g| (2.0..=7.5).contains(g))
}

/// v0.15.19: Live-Lookup des Approach-Gleitwinkels aus dem per-Flug-Navdata-
/// Cache (beim Flugstart befüllt, daher im Anflug ohne Netz verfügbar). Sperrt
/// NUR `navdata` — der Aufrufer hält dabei KEINE andere Sperre (siehe
/// `flight_info`), um Lock-Order-Probleme zu vermeiden.
fn resolve_approach_glideslope_deg(
    flight: &ActiveFlight,
    approach_runway: Option<&str>,
) -> Option<f64> {
    let rw = approach_runway?;
    let cache = flight.navdata.lock().ok()?;
    let apt = cache.get(&flight.arr_airport)?;
    runway_glideslope_for(&apt.runways, rw)
}

fn flight_info(
    flight: &ActiveFlight,
    resume_position_suspect: bool,
    live: Option<PanelLiveSnapshot>,
) -> ActiveFlightInfo {
    // v0.15.19: Approach-Gleitwinkel zuerst auflösen — kurzer stats-Lock nur für
    // den Runway-String, dann navdata-Lock. So werden stats + navdata NIE
    // gleichzeitig gehalten (keine Lock-Order-Inversion ggü. step_flight).
    let approach_glideslope_angle = {
        let rw = flight
            .stats
            .lock()
            .ok()
            .and_then(|s| s.approach_runway.clone());
        resolve_approach_glideslope_deg(flight, rw.as_deref())
    }
    // v1.5.0 (#msfs-hud): Fällt die ATC-Bahn aus — was bei MSFS IMMER der
    // Fall ist, `selected_runway` wird vom Adapter nicht abonniert —, dann
    // den Winkel der geschätzten Bahn nehmen. Die Reihenfolge ist bewusst so
    // herum: liegt ausnahmsweise doch eine echte ATC-Bahn vor (X-Plane, oder
    // ein künftiger MSFS-Adapter), gewinnt die gegenüber der Schätzung.
    .or_else(|| {
        flight
            .stats
            .lock()
            .ok()
            .and_then(|s| s.predicted_glideslope_deg)
    });
    let stats = flight.stats.lock().expect("flight stats");
    // Don't consume here — the flag stays true until the resume banner has
    // run its course (flight_resume_confirm or flight_cancel clears it).
    // This avoids a race with React StrictMode's double-mount where the
    // first poll consumes the flag before the UI can latch it.
    let was_just_resumed = flight.was_just_resumed.load(Ordering::Relaxed);
    ActiveFlightInfo {
        pirep_id: flight.pirep_id.clone(),
        bid_id: flight.bid_id,
        started_at: flight.started_at.to_rfc3339(),
        airline_icao: flight.airline_icao.clone(),
        planned_registration: flight.planned_registration.clone(),
        aircraft_icao: flight.aircraft_icao.clone(),
        aircraft_name: flight.aircraft_name.clone(),
        flight_number: flight.flight_number.clone(),
        callsign: flight.bid_callsign.clone(),
        dpt_airport: flight.dpt_airport.clone(),
        arr_airport: flight.arr_airport.clone(),
        distance_nm: stats.distance_nm,
        position_count: stats.position_count,
        // Seit 17.10.2026 („Stück 2") gibt es EINE Phase: im Reiseband
        // entscheidet v2, v1 übernimmt sie; Anflug/Final/Landung und der
        // Boden entscheidet v1. `shadow_segment` unten trägt das
        // „Level"-Label für Restriktionen ans Frontend.
        phase: phase_to_snake(stats.phase).to_string(),
        // v0.16.12 (#phase-v2): v2s Rohsicht — nur noch für Auswertungen;
        // die Oberfläche zeigt sie nicht.
        shadow_phase: stats.shadow_phase.map(|p| phase_to_snake(p).to_string()),
        shadow_segment: stats.shadow_segment.map(|s| s.as_snake_str().to_string()),
        block_off_at: stats.block_off_at.map(|t| t.to_rfc3339()),
        takeoff_at: stats.takeoff_at.map(|t| t.to_rfc3339()),
        landing_at: stats.landing_at.map(|t| t.to_rfc3339()),
        block_on_at: stats.block_on_at.map(|t| t.to_rfc3339()),
        flugzeiten: flugzeiten(&stats),
        landung_erwartet: stats.landung_erwartet,
        // v0.7.21 (MS713-QS): canonical_landing_rate_fpm() statt
        // direkt — sonst zeigt das Active-Flight-Panel (vor + waehrend
        // Filing) den Streamer-Tick-Snapshot, der Tab "Landung" aber
        // den Engineering-Edge.
        landing_rate_fpm: stats.canonical_landing_rate_fpm(),
        landing_g_force: stats.landing_g_force,
        landing_score_finalized: stats.landing_score_finalized,
        live,
        was_just_resumed,
        resume_position_suspect,
        // v0.13.0 Stream F: nur befüllen wenn was_just_resumed=true, sonst
        // werden die Felder via skip_serializing_if weggelassen damit das
        // Live-Active-Flight-Panel sie nicht sieht.
        // 27.09.2026 (SIA 375): der letzte Punkt kommt aus `last_known_*` —
        // `last_lat/lon` ist die Wegstrecken-Basis und wird beim Resume
        // absichtlich geleert. Dazu Hoehe, Kurs, Tempo und Zeit, damit der
        // Pilot nach einem Sim-Absturz dorthin zurueckstellen kann.
        last_known_lat: if was_just_resumed {
            stats.last_known_lat.or(stats.last_lat)
        } else {
            None
        },
        last_known_lon: if was_just_resumed {
            stats.last_known_lon.or(stats.last_lon)
        } else {
            None
        },
        last_known_alt_ft: if was_just_resumed {
            stats.last_known_alt_ft.map(|a| a.round() as i32)
        } else {
            None
        },
        last_known_heading_deg: if was_just_resumed {
            stats
                .last_known_hdg_deg
                .map(|h| (h.round() as i32).rem_euclid(360))
        } else {
            None
        },
        last_known_gs_kt: if was_just_resumed {
            stats.last_known_gs_kt.map(|g| g.round() as i32)
        } else {
            None
        },
        last_known_at: if was_just_resumed {
            stats.last_known_at.map(|t| t.to_rfc3339())
        } else {
            None
        },
        // v0.13.0 Stream F: Fuel/Weight/Aircraft aus dem letzten Sim-Snapshot
        // (vor dem Disconnect/Crash). MSFS setzt Fuel beim Reload oft auf
        // Default — der Pilot sieht jetzt den Soll-Wert und kann ihn manuell
        // nachstellen, BEVOR er auf "Position prüfen + fortsetzen" klickt.
        last_known_fuel_kg: if was_just_resumed {
            stats.last_fuel_kg
        } else {
            None
        },
        last_known_zfw_kg: if was_just_resumed {
            stats.last_zfw_kg
        } else {
            None
        },
        last_known_total_weight_kg: if was_just_resumed {
            stats.last_total_weight_kg
        } else {
            None
        },
        last_known_aircraft_icao: if was_just_resumed && !flight.aircraft_icao.is_empty() {
            Some(flight.aircraft_icao.clone())
        } else {
            None
        },
        dep_gate: stats.dep_gate.clone(),
        arr_gate: stats.arr_gate.clone(),
        approach_runway: stats.approach_runway.clone(),
        approach_glideslope_angle,
        // v1.5.1 (F2): leere METARs gar nicht erst serialisieren — die
        // Anzeige unterscheidet "kein Wetter" nur über Abwesenheit.
        dep_metar: stats.dep_metar_raw.clone().filter(|m| !m.is_empty()),
        arr_metar: stats.arr_metar_raw.clone().filter(|m| !m.is_empty()),
        dep_metar_decoded: stats.dep_metar_snap.clone(),
        arr_metar_decoded: stats.arr_metar_snap.clone(),
        predicted_runway: stats.predicted_runway.clone(),
        last_position_at: stats.last_position_at.map(|t| t.to_rfc3339()),
        last_heartbeat_at: stats.last_heartbeat_at.map(|t| t.to_rfc3339()),
        queued_position_count: stats.queued_position_count,
        connection_state: match flight.connection_state.load(Ordering::Relaxed) {
            CONN_STATE_FAILING => "failing".to_string(),
            CONN_STATE_BLOCKED => "blocked".to_string(),
            _ => "live".to_string(),
        },
        paused_since: stats.paused_since.map(|t| t.to_rfc3339()),
        paused_last_known: stats.paused_last_known.clone(),
        divert_hint: stats.divert_hint.clone(),
        umleitung_ziel: stats.umleitung_ziel.clone(),
        unmoeglicher_sprung: stats.resume_discontinuity.is_some(),
        abgabe_sperre: abgabe_sperre(&stats).map(|g| g.code().to_string()),
        resume_wartet_grund: stats.resume_wartet.as_ref().map(|(g, _)| g.clone()),
        resume_wartet_s: stats.resume_wartet.as_ref().map(|(_, s)| *s),
        resume_gate_laeuft: stats.resume_gate_laeuft,
        resume_vom_boden_moeglich: false,
        touch_and_go_count: stats
            .touchdown_events
            .iter()
            .filter(|e| matches!(e.kind, TouchdownKind::TouchAndGo))
            .count() as u32,
        go_around_count: stats.go_around_count,
        // ---- v0.3.0 — SimBrief Plan-Werte für Soll/Ist-Vergleich ----
        // Werden vom flight_start gefüllt (über fetch_simbrief_ofp).
        // None bleiben sie wenn der Pilot keine SimBrief-Verbindung
        // im phpVMS-Profil hat.
        planned_block_fuel_kg: stats.planned_block_fuel_kg,
        planned_burn_kg: stats.planned_burn_kg,
        planned_reserve_kg: stats.planned_reserve_kg,
        planned_max_zfw_kg: stats.planned_max_zfw_kg,
        planned_max_tow_kg: stats.planned_max_tow_kg,
        planned_max_ldw_kg: stats.planned_max_ldw_kg,
        // v0.3.0 — Live-Loadsheet-Werte für Boarding-Phase. Pulled
        // aus dem letzten beobachteten Sim-Snapshot — wird beim
        // Block-off "eingefroren" (last_block_fuel_kg etc.).
        sim_fuel_kg: stats.last_fuel_kg,
        sim_zfw_kg: stats.last_zfw_kg,
        sim_tow_kg: stats.last_total_weight_kg,
        planned_zfw_kg: stats.planned_zfw_kg,
        planned_tow_kg: stats.planned_tow_kg,
        planned_ldw_kg: stats.planned_ldw_kg,
        planned_route: stats.planned_route.clone(),
        planned_alternate: stats.planned_alternate.clone(),

        // v0.7.19 GAF-707 (QS-R1 Finding 3): Accident-Status fuer das
        // Frontend exponieren — sonst kann der Flight-End-Dialog nicht
        // entscheiden ob "War das ein Absturz?" gezeigt werden muss.
        accident_detected: stats.accident_detected,
        accident_kind: stats.accident_kind.clone(),
        accident_confidence: stats.accident_confidence.clone(),
        accident_reasons: stats.accident_reasons.clone(),
        accident_at: stats.accident_at.map(|t| t.to_rfc3339()),

        // v0.12.5 (LE3): Manual-Filing-Vorbefüllung aus den FSM-Stats.
        manual_filing_defaults: ManualFilingDefaults {
            distance_nm: if stats.distance_nm > 0.0 {
                Some(stats.distance_nm)
            } else {
                None
            },
            cruise_level_ft: stats
                .peak_altitude_ft
                .map(|ft| (((ft / 100.0).round() * 100.0) as i32).max(0)),
            flight_time_minutes: match (stats.takeoff_at, stats.landing_at) {
                (Some(t), Some(l)) if l > t => Some((l - t).num_minutes() as i32),
                _ => None,
            },
            landing_rate_fpm: stats.canonical_landing_rate_fpm(),
            block_off_at: stats.block_off_at.map(|t| t.to_rfc3339()),
            block_on_at: stats.block_on_at.map(|t| t.to_rfc3339()),
            block_fuel_kg: stats.block_fuel_kg.filter(|kg| *kg > 0.0),
            remaining_fuel_kg: stats.last_fuel_kg.filter(|kg| *kg >= 0.0),
            divert_to: stats
                .divert_hint
                .as_ref()
                .and_then(|h| h.actual_icao.clone()),
            planned_arr_icao: flight.arr_airport.clone(),
            requires_reason: stats.divert_hint.is_some(),
        },
    }
}

/// Was aus der Szenerie-Abfrage geworden ist — in einem Wort, das
/// ohne Erklaerung lesbar ist.
///
/// ⚠ Bis v1.7.9 stand am Flug nur `bahn_geometrie_quelle` mit zwei
/// moeglichen Werten. "navdaten" bedeutete gleichzeitig: nie gefragt,
/// abgelehnt, keine Antwort, Antwort ohne Bahnen, oder Antwort ohne
/// passende Bahn. Am 29.08.2026 war deshalb aus dem Bestand nicht zu
/// sagen, warum die MSFS-Haelfte bei 5 von 5 Landungen nichts lieferte —
/// ein Feature ohne Messpunkt.
fn szenerie_status(stats: &FlightStats) -> String {
    // ⚠ Nach einem Neustart fehlt `szenerie_auskunft` (nicht persistiert)
    // — der Status wuerde auf eine vagere Aussage kippen und die
    // genauere beim Recorder ueberschreiben (Runde 5, N26). Dann gilt der
    // Status, der bei der letzten Zuordnung festgehalten wurde.
    // BEIDE Quellen ansehen — dieselben, aus denen Geometrie und
    // Ausfahrten schoepfen.
    //
    // Sonst meldet der Status den Szenerie-Weg als unbenutzt, waehrend
    // Achse und Ausfahrten gerade daraus stammen. Dieser Status ist der
    // Messpunkt, mit dem die naechste Felddiagnose dieser Klasse
    // eingekreist wird — er darf nicht in die Irre fuehren (externe
    // Abnahme, 21.09.2026).
    //
    // MIT demselben ICAO-Filter wie Geometrie und Ausfahrten. Ohne ihn
    // meldete der Status beim Ausweichflug eine einschlaegige
    // Szenerie-Antwort, wo keine benutzt wurde: Gelandet in EDDM, in der
    // Zweitquelle liegt das geplante Ziel EDDF — Geometrie und
    // Ausfahrten verwerfen es korrekt, der Status haette
    // `auskunft_ohne_vergleich(platz=?, …)` behauptet. Genau die
    // Irrefuehrung, die dieser Messpunkt vermeiden soll (externe
    // Nachpruefung, 21.09.2026).
    let landeplatz = stats
        .runway_match
        .as_ref()
        .map(|m| m.airport_ident.as_str());
    let passend = |a: &&Arc<sim_core::szenerie::SzenerieFlughafen>| match landeplatz {
        Some(icao) => a.icao.eq_ignore_ascii_case(icao),
        // Ohne gematchte Bahn gibt es nichts zu vergleichen — dann zaehlt
        // wie bisher die Primaerquelle, die Zweitquelle bleibt draussen.
        None => false,
    };
    let auskunft = stats
        .szenerie_auskunft
        .as_ref()
        .or_else(|| stats.ziel_szenerie_auskunft.as_ref().filter(passend));
    if auskunft.is_none() {
        if let Some(fest) = stats.szenerie_status_fest.as_ref() {
            return fest.clone();
        }
    }
    if stats
        .szenerie_uebernahme
        .as_ref()
        .is_some_and(|b| !b.uebernommen.is_empty())
    {
        return "uebernommen".to_string();
    }
    // Eine Auskunft lag vor, aber keine ihrer Bahnen wurde uebernommen.
    // Das ist etwas ganz anderes als "keine Auskunft" — es deutet auf
    // einen Lage-Unterschied, nicht auf einen toten Weg.
    //
    // ⚠ Und es zerfaellt in ZWEI Ursachen, die verschiedene Fixes
    // brauchen. Bis v1.7.12 hiessen beide nur "kein_treffer":
    //
    //   `ohne_treffer` — der Bezeichner passte bei keiner Szenerie-Bahn.
    //                    Dann stimmen die Kennungen nicht (Nummer oder
    //                    Buchstabe), nicht die Lage.
    //   `verworfen`    — der Bezeichner passte, die Lage nicht. Dann
    //                    stimmen die Koordinaten nicht.
    //
    // Am 30./31.08.2026 sind drei MSFS-Fluege daran gescheitert (EDDF
    // 25L, YBWW 12, YBCG 14) und die Meldung sagte bei allen dasselbe.
    // Die Zahl der gelieferten Bahnen kommt mit dazu: Eine Auskunft mit
    // einer einzigen Bahn ist ein anderer Fall als eine mit vierzig.
    if let Some(anzahl) = auskunft.map(|a| a.bahnen.len()).filter(|n| *n > 0) {
        // ⚠ NUR melden, wenn ein Vergleich wirklich gelaufen ist.
        //
        // Ohne Uebernahmebericht gab es keinen — etwa weil die Navdaten
        // fehlten oder die Auskunft zu einem anderen Platz gehoerte
        // (nach einem Divert liegt die des geplanten Ziels da). Ein
        // `ohne_treffer=0, verworfen=0` waere dann eine erfundene
        // Nullmessung und wuerde ausgerechnet die Ursache verdecken,
        // die diese Meldung sichtbar machen soll.
        return match stats.szenerie_uebernahme.as_ref() {
            Some(b) => format!(
                "kein_treffer(bahnen={anzahl}, ohne_treffer={}, verworfen={})",
                b.ohne_treffer.len(),
                b.verworfen.len()
            ),
            // ⚠ MIT dem Platz und der Diagnose.
            //
            // Am 01.09.2026 stand hier `auskunft_ohne_vergleich(bahnen=4)`
            // und sonst nichts. Dass die vier Bahnen der START flughafen
            // waren und nicht das Ziel, liess sich erst ueber die
            // Bahnzahl der beiden Plaetze erschliessen — LEZL hat eine,
            // EDDF hat vier. Der Platz gehoert in die Zeile, sonst
            // kostet dieselbe Frage wieder eine halbe Untersuchung.
            None => format!(
                "auskunft_ohne_vergleich(platz={}, bahnen={anzahl}, diagnose={})",
                stats
                    .szenerie_auskunft
                    .as_ref()
                    .map(|a| a.icao.as_str())
                    .unwrap_or("?"),
                stats.szenerie_diagnose.as_deref().unwrap_or("?")
            ),
        };
    }
    match stats.szenerie_diagnose.as_deref() {
        Some(d) => d.to_string(),
        // Kein MSFS-Adapter im Spiel (X-Plane liest die Datei direkt):
        // Dann sagt die Anwesenheit eines Berichts, ob gelesen wurde.
        None if stats.szenerie_uebernahme.is_some() => "kein_treffer".to_string(),
        None => "keine_szenerie".to_string(),
    }
}

fn phase_to_snake(phase: FlightPhase) -> &'static str {
    match phase {
        FlightPhase::Preflight => "preflight",
        FlightPhase::Boarding => "boarding",
        FlightPhase::Pushback => "pushback",
        FlightPhase::TaxiOut => "taxi_out",
        FlightPhase::TakeoffRoll => "takeoff_roll",
        FlightPhase::Takeoff => "takeoff",
        FlightPhase::Climb => "climb",
        FlightPhase::Cruise => "cruise",
        FlightPhase::Holding => "holding",
        FlightPhase::Descent => "descent",
        FlightPhase::Approach => "approach",
        FlightPhase::Final => "final",
        FlightPhase::Landing => "landing",
        FlightPhase::TaxiIn => "taxi_in",
        FlightPhase::BlocksOn => "blocks_on",
        FlightPhase::Arrived => "arrived",
        FlightPhase::PirepSubmitted => "pirep_submitted",
    }
}

/// v0.13.x (In-App-Live-Map): geplante Navlog-Fixes des aktiven Flugs für die
/// Karten-Ansicht (Dots der geplanten Route + TOC/TOD). Leer, wenn kein
/// aktiver Flug / kein OFP (VFR/Manual/Resume) — die Map fällt dann auf eine
/// Great-Circle-Linie Dep→Arr zurück.
#[tauri::command(async)]
fn flight_get_route_fixes(state: tauri::State<'_, AppState>) -> Vec<api_client::RouteFix> {
    let guard = state.active_flight.lock().expect("active_flight lock");
    match guard.as_ref() {
        Some(flight) => flight
            .stats
            .lock()
            .expect("flight stats lock")
            .planned_waypoints
            .clone(),
        None => Vec::new(),
    }
}

/// v1.7.40: Die Tabelle „Sprit · Wegpunkt für Wegpunkt" für das Cockpit.
///
/// Dieselbe Rechnung (`sprit_wegpunkt_zeilen`), die nach der Landung mit der
/// Sprit-Auswertung eingefroren wird — Cockpit und Landeauswertung zeigen
/// nie verschiedene Zahlen. Leer ohne aktiven Flug oder ohne Navlog-Werte.
#[derive(Debug, Clone, Default, Serialize)]
struct SpritWegpunkteDto {
    zeilen: Vec<landing_scoring::sprit::Wegpunkt>,
    /// Der nächste noch nicht überflogene Wegpunkt.
    naechster: Option<usize>,
    /// Luftlinie dorthin, NM.
    naechster_nm: Option<f64>,
}

/// Luftlinie zur Zeile `i` der Tabelle, in NM.
///
/// # Warum das nicht einfach `planned_waypoints[i]` ist
///
/// Die Tabelle traegt vorn eine eigene Abflugzeile, sobald das Navlog
/// nicht am Flughafen beginnt — dann sind beide Listen um eins versetzt,
/// und der rohe Index zeigte auf den Fix DAVOR: eine Entfernung, die
/// waechst statt zu schrumpfen (Abnahme 20.09.2026).
///
/// Gesucht wird ueber die Kennung. Kommt sie mehrfach vor (Warteschleife,
/// SID/STAR-Ueberschneidung), entscheidet die Naehe — aber erst, nachdem
/// der Zeilen-Index in einen Wegpunkt-Index uebersetzt wurde. Ohne diese
/// Uebersetzung waehlt er bei zwei gleichen Kennungen direkt
/// hintereinander die falsche.
fn entfernung_zur_zeile(
    zeilen: &[landing_scoring::sprit::Wegpunkt],
    fixes: &[api_client::RouteFix],
    i: usize,
    lat: f64,
    lon: f64,
) -> Option<f64> {
    let z = zeilen.get(i)?;
    let versatz = zeilen.len().saturating_sub(fixes.len());
    let ziel = i.saturating_sub(versatz);
    fixes
        .iter()
        .enumerate()
        .filter(|(_, f)| f.ident == z.ident)
        .min_by_key(|(k, _)| k.abs_diff(ziel))
        .map(|(_, f)| ::geo::distance_m(lat, lon, f.lat, f.lon) / 1852.0)
}

#[tauri::command(async)]
fn flight_sprit_wegpunkte(state: tauri::State<'_, AppState>) -> SpritWegpunkteDto {
    let guard = state.active_flight.lock().expect("active_flight lock");
    let Some(flight) = guard.as_ref() else {
        return SpritWegpunkteDto::default();
    };
    let stats = flight.stats.lock().expect("flight stats lock");
    let abflug = stats.abflug_icao.clone();
    // Nach der Landung die eingefrorene Fassung — sie ist die, die in den
    // Bericht geht.
    let zeilen = match stats.sprit_auswertung.as_ref() {
        Some(a) if !a.wegpunkte.is_empty() => a.wegpunkte.clone(),
        _ => sprit_wegpunkt_zeilen(&stats, abflug.as_deref(), false),
    };
    use landing_scoring::sprit::WegpunktZustand;
    let letzter = zeilen
        .iter()
        .rposition(|z| z.zustand != WegpunktZustand::Offen);
    let naechster = match letzter {
        Some(i) if i + 1 < zeilen.len() => Some(i + 1),
        None if !zeilen.is_empty() => Some(0),
        _ => None,
    };
    // Ueber die KENNUNG, nicht ueber den Index.
    //
    // `naechster` zaehlt in den ZEILEN, die Koordinaten stehen in
    // `planned_waypoints`. Sobald vorn eine eigene Abflugzeile steht, sind
    // beide Listen verschieden lang, und der Index zeigte auf den Fix
    // DAVOR — eine Entfernung, die waechst statt zu schrumpfen (Abnahme
    // 20.09.2026). Die Kennung sucht den Fix, sie stimmt immer.
    // Kommt eine Kennung zweimal vor (Warteschleife, SID/STAR-
    // Ueberschneidung), entscheidet erst dann die Naehe zum uebersetzten
    // Zeilen-Index — genau wie bei der Zuordnung der Messungen. Diese
    // Uebersetzung ist ein Laengenvergleich und damit nur so gut wie die
    // Annahme, dass die Zeilen vorn um die Abflugzeile laenger sind;
    // stimmt sie einmal nicht, faellt die Wahl auf den ersten Treffer
    // zurueck statt daneben (`saturating_sub`). Entscheider bleibt die
    // Kennung, der Index ist nur der Stichentscheid (Abnahme 20.09.2026).
    let naechster_nm = match (naechster, stats.last_known_lat, stats.last_known_lon) {
        (Some(i), Some(la), Some(lo)) => {
            entfernung_zur_zeile(&zeilen, &stats.planned_waypoints, i, la, lo)
        }
        _ => None,
    };
    SpritWegpunkteDto {
        zeilen,
        naechster,
        naechster_nm,
    }
}

/// Kennung des laufenden Fluges, oder `None`, wenn gerade keiner laeuft.
///
/// Das VDGS-Band braucht beides: die Wache („laeuft ueberhaupt ein
/// Flug?") und einen Schluessel fuer seinen Zwischenspeicher. Ueber das
/// Rufzeichen allein ginge das nicht — zwei Fluege koennen dasselbe
/// tragen, und dann stuenden die Zeiten des vorigen im neuen Cockpit
/// (Codex-Abnahme 20.09.2026). `pirep_id` ist je Flug eindeutig.
pub(crate) fn vdgs_flug_kennung(app: &AppHandle) -> Option<String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let guard = state.active_flight.lock().ok()?;
    let flight = guard.as_ref()?;
    // Und nur VOR dem Abheben. Die Anzeige haelt sich daran, das
    // Backend tat es nicht — ueber die LAN-Bruecke war der Befehl also
    // auch im Reiseflug erreichbar, entgegen der Zusage (Codex-Abnahme,
    // zweite Runde, 20.09.2026). Nach dem Abheben gibt es nichts mehr
    // zu sequenzieren.
    if flight.stats.lock().ok()?.takeoff_at.is_some() {
        return None;
    }
    Some(flight.pirep_id.clone())
}

/// Abflugplatz des laufenden Fluges — fuer die Pruefung, ob ein VDGS-
/// Eintrag zu genau diesem Flug gehoert (`vdgs::passt_zum_flug`).
pub(crate) fn vdgs_flug_abflug(app: &AppHandle) -> Option<String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    let guard = state.active_flight.lock().ok()?;
    Some(guard.as_ref()?.dpt_airport.clone())
}

/// Ein Wegpunkt einer fremden Route, wie die Karte ihn braucht.
#[derive(Debug, Clone, serde::Serialize)]
struct FremdePunkt {
    lon: f64,
    lat: f64,
    /// Kann leer sein — dann zeichnet die Karte nur den Punkt.
    name: String,
}

/// Die geplante Route eines KOLLEGEN — fuer die Karte.
///
/// Thomas, 20.09.2026: „in der Map auf die anderen Flieger klicken und die
/// Route sehen, bei erneutem Klick wieder aus." Das Ein- und Ausblenden
/// macht die Anzeige; hier kommen nur die Punkte her.
///
/// Leere Liste heisst: Es gibt keine Route (Flug ohne SimBrief-Plan) oder
/// sie gehoert einer anderen VA. Beides ist kein Fehler — die Anzeige
/// sagt es dem Piloten.
#[tauri::command]
async fn fremde_flugroute(pirep_id: String) -> Result<Vec<FremdePunkt>, UiError> {
    if pirep_id.trim().is_empty() {
        return Ok(Vec::new());
    }
    let (Ok(Some(username)), Ok(Some(password))) = (
        secrets::load_api_key(MQTT_KEYRING_USERNAME),
        secrets::load_api_key(MQTT_KEYRING_PASSWORD),
    ) else {
        // Fehlender Zugang ist NICHT dasselbe wie „keine Route": Sonst
        // suchte der Pilot den Fehler beim Kollegen, obwohl sein eigener
        // Client nicht angemeldet ist (Abnahme 20.09.2026).
        return Err(UiError::new(
            "kein_live_zugang",
            "Kein Zugang zum Live-Server — Route nicht abrufbar.",
        ));
    };
    match aeroacars_mqtt::log_upload::fremde_route(pirep_id.trim(), &username, &password, None)
        .await
    {
        // Den NAMEN mitnehmen. Er kommt vom Server mit und wurde hier
        // weggeworfen — die Karte zeigte dann Punkte ohne Beschriftung,
        // und man konnte nicht lesen, wo der Kollege seine Fixe hat
        // (Thomas, 20.09.2026).
        Ok(punkte) => Ok(punkte
            .into_iter()
            .filter(|p| p.lat.is_finite() && p.lon.is_finite())
            .map(|p| FremdePunkt {
                lon: p.lon,
                lat: p.lat,
                name: p.name.unwrap_or_default(),
            })
            .collect()),
        Err(e) => {
            tracing::warn!(pirep_id = %pirep_id, error = %e, "fremde Route nicht geladen");
            Err(UiError::new("route_unavailable", e.to_string()))
        }
    }
}

/// v0.15.x (In-App-Live-Map): der im Backend akkumulierte, ausgedünnte
/// geflogene Track [lon, lat] des aktiven Flugs. Im Streamer bei voller Tick-
/// Rate gefüllt (fokus-/fenster-unabhängig → lückenlos auch bei X-Plane-
/// Vollbild, anders als der gedrosselte Webview-Snapshot-Poll). Leer, wenn
/// kein aktiver Flug. Das Frontend spiegelt das via `setTrack` in den
/// trackStore und rendert die Linie daraus.
#[tauri::command(async)]
fn flight_get_track(state: tauri::State<'_, AppState>) -> Vec<[f64; 2]> {
    let guard = state.active_flight.lock().expect("active_flight lock");
    match guard.as_ref() {
        Some(flight) => flight
            .stats
            .lock()
            .expect("flight stats lock")
            .track
            .clone(),
        None => Vec::new(),
    }
}

/// v0.13.x (In-App-Live-Map, VA-Übersicht): proxyt den öffentlichen phpVMS-
/// Endpoint `GET {base}/api/acars` (alle aktiven Flüge der VA mit Position) und
/// gibt das JSON 1:1 zurück. Backend-Proxy statt Frontend-fetch, um CSP/CORS im
/// Tauri-Webview zu umgehen. Best-effort: bei Fehler `{ "flights": [] }`.
#[tauri::command]
async fn va_live_flights(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, UiError> {
    // Über den bereits konfigurierten api-client-HTTP-Client (gleicher TLS-Pfad
    // wie alle anderen Calls) — NICHT über einen frisch gebauten reqwest::Client,
    // der auf manchen Builds am rustls-CryptoProvider scheitern würde.
    match current_client(&state) {
        Ok(c) => c.get_acars_live().await.map_err(UiError::from),
        Err(_) => Ok(serde_json::json!({ "flights": [] })),
    }
}

/// Logbuch: Flugliste (paginiert) — LIVE aus dem StratosLogbook-Modul, nichts
/// gespeichert. Auth über den phpVMS-api_key (Bearer) im api-client.
#[tauri::command]
async fn logbook_pireps(
    state: tauri::State<'_, AppState>,
    limit: u32,
    offset: u32,
) -> Result<serde_json::Value, UiError> {
    current_client(&state)?
        .get_logbook_pireps(limit, offset)
        .await
        .map_err(UiError::from)
}

// ─── Pilotenchat ────────────────────────────────────────────────────────
//
// Drei Befehle, mehr braucht die Oberfläche nicht. Der laufende Betrieb
// kommt über MQTT herein (Ereignis "chat-nachricht"), diese hier sind für
// Einstieg und Absenden.

/// Einen Zuruf abschicken. `an_pilot_id` gesetzt = Direktnachricht.
///
/// Absender und Rufzeichen setzt der SERVER aus der laufenden Flugsitzung —
/// hier geht bewusst nur der Text raus. Und wer keine offene Sitzung hat,
/// wird serverseitig abgewiesen; das ist kein Fehler der Oberfläche, sondern
/// die Regel "nur wer fliegt, redet mit".
#[tauri::command]
async fn chat_senden(
    state: tauri::State<'_, AppState>,
    text: String,
    an_pilot_id: Option<String>,
) -> Result<bool, UiError> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Ok(false);
    }
    let mqtt = state.mqtt.lock().await;
    match mqtt.as_ref() {
        Some(h) => Ok(h.chat(text, an_pilot_id)),
        // Kein Live-Kanal: der Pilot ist nicht verbunden. Kein Fehlerdialog —
        // die Oberfläche zeigt das ohnehin am Verbindungszustand.
        None => Ok(false),
    }
}

/// Was lief, bevor ich mich verbunden habe. Zwölf Stunden, mehr nicht.
#[tauri::command]
async fn chat_verlauf() -> Result<serde_json::Value, UiError> {
    let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
    match aeroacars_mqtt::chat::verlauf(None, token.as_deref()).await {
        Ok((nachrichten, fenster_stunden)) => Ok(serde_json::json!({
            "nachrichten": nachrichten,
            "fenster_stunden": fenster_stunden,
        })),
        // "Du fliegst gerade nicht" ist kein Fehler, sondern der
        // Normalzustand am Boden. Er wird durchgereicht, damit die
        // Oberfläche ihn benennen kann, statt ein Eingabefeld anzubieten,
        // aus dem nichts herausgeht (Feldbefund Thomas, 12.08.2026:
        // "kommt keine durch — gut so, aber transparent ist das nicht").
        Err(aeroacars_mqtt::chat::ChatFehler::KeinFlug) => Ok(serde_json::json!({
            "nachrichten": [], "fenster_stunden": 12, "kein_laufender_flug": true,
        })),
        // Alles andere best-effort: lieber leer starten und über MQTT
        // nachfüllen, als den Piloten mit einem Fehler zu behelligen, den
        // er nicht beheben kann.
        Err(e) => {
            tracing::warn!(error = %e, "Chat-Verlauf nicht abrufbar");
            Ok(serde_json::json!({ "nachrichten": [], "fenster_stunden": 12 }))
        }
    }
}

// ─── VATSIM CDM ─────────────────────────────────────────────────────────
//
// Öffnet `vats.im/vdgs` in einem EIGENEN Fenster der App — nicht in einem
// Rahmen innerhalb der Oberfläche.
//
// Warum kein Rahmen: gemessen am 12.08.2026 setzt `auth.vatsim.net` den
// Kopf `X-Frame-Options: SAMEORIGIN`. Eine Anmeldeseite, die sich einbetten
// liesse, waere auch ein Sicherheitsproblem — man koennte eine falsche
// davorlegen. Ein eigenes Fenster ist dagegen normale Navigation und
// funktioniert; die Anmeldung bleibt darin bestehen, weil es dieselbe
// WebView-Sitzung nutzt.
//
// Zweimal aufgerufen wird kein zweites Fenster gebaut, sondern das
// vorhandene nach vorn geholt.
#[tauri::command]
async fn vdgs_fenster_oeffnen(app: AppHandle) -> Result<(), UiError> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    if let Some(vorhanden) = app.get_webview_window("vdgs") {
        let _ = vorhanden.unminimize();
        let _ = vorhanden.show();
        let _ = vorhanden.set_focus();
        return Ok(());
    }

    let url: tauri::Url = paket_endpunkte::endpunkte()
        .vatsim_vdgs
        .parse()
        .map_err(|e| UiError::new("bad_url", format!("VDGS-Adresse ungültig: {e}")))?;

    WebviewWindowBuilder::new(&app, "vdgs", WebviewUrl::External(url))
        .title("VATSIM CDM — vats.im/vdgs")
        .inner_size(1160.0, 820.0)
        .min_inner_size(700.0, 520.0)
        .resizable(true)
        .build()
        .map_err(|e| {
            UiError::new(
                "window_failed",
                format!("Fenster ließ sich nicht öffnen: {e}"),
            )
        })?;

    Ok(())
}

/// Ist das VDGS-Fenster gerade offen? Die Oberfläche beschriftet ihren
/// Knopf danach ("öffnen" gegen "nach vorn holen").
#[tauri::command]
async fn vdgs_fenster_offen(app: AppHandle) -> Result<bool, UiError> {
    Ok(app.get_webview_window("vdgs").is_some())
}

/// Wer gerade fliegt — Name, Rufzeichen, Strecke.
#[tauri::command]
async fn chat_teilnehmer() -> Result<serde_json::Value, UiError> {
    let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
    match aeroacars_mqtt::chat::teilnehmer(None, token.as_deref()).await {
        Ok(t) => Ok(serde_json::json!({ "teilnehmer": t })),
        Err(aeroacars_mqtt::chat::ChatFehler::KeinFlug) => Ok(serde_json::json!({
            "teilnehmer": [], "kein_laufender_flug": true,
        })),
        Err(e) => {
            tracing::warn!(error = %e, "Chat-Teilnehmer nicht abrufbar");
            Ok(serde_json::json!({ "teilnehmer": [] }))
        }
    }
}

/// Logbuch: Summen (Flüge/Stunden/Distanz/Ø-Landung/Rang).
#[tauri::command]
async fn logbook_stats(state: tauri::State<'_, AppState>) -> Result<serde_json::Value, UiError> {
    current_client(&state)?
        .get_logbook_stats()
        .await
        .map_err(UiError::from)
}

/// Logbuch: Detail eines PIREP (inkl. Track `route` + Fluglogbuch `log`).
#[tauri::command]
async fn logbook_pirep(
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, UiError> {
    current_client(&state)?
        .get_logbook_pirep(&id)
        .await
        .map_err(UiError::from)
}

#[tauri::command(async)]
fn flight_status(app: AppHandle, state: tauri::State<'_, AppState>) -> Option<ActiveFlightInfo> {
    let guard = state.active_flight.lock().expect("active_flight lock");
    let flight = guard.as_ref()?;
    // v0.12.1 (Stream E): while the resume banner is up, check on every
    // poll whether the current sim position matches the persisted flight.
    // If the sim crashed and reloaded the aircraft on the ground (or far
    // away), the banner must NOT auto-confirm — the pilot has to click.
    let mut vom_boden_moeglich = false;
    let resume_position_suspect = if flight.was_just_resumed.load(Ordering::Relaxed) {
        match current_snapshot(&app) {
            Some(snap) => {
                let abweichung = lage_abweichung_fuer(flight, &snap);
                if abweichung == Some(LageAbweichung::AmBoden) {
                    let st = flight.stats.lock().expect("flight stats");
                    let am_ziel = arrival::locate(
                        &flight.arr_airport,
                        snap.lat,
                        snap.lon,
                        st.planned_arr_ref_pos,
                    )
                    .is_at_planned();
                    vom_boden_moeglich = bodenneustart_moeglich(&st, abweichung, am_ziel);
                }
                abweichung.is_some()
            }
            // Ohne Sim zeigt das Banner „Warte auf den Simulator" (kein
            // Countdown ohne Sim). Bleibt eine Sim-Trennung offen, prueft der
            // Streamer die Lage ohnehin selbst, bevor er weiterlaeuft.
            None => false,
        }
    } else {
        false
    };
    // v0.21 (#msfs-panel): live telemetry for the MSFS panel's
    // approach-monitor card, piggybacked onto the same 1Hz status the LAN
    // remote-control ticker already broadcasts — see PanelLiveSnapshot.
    // `current_snapshot` was already the pattern used above for the
    // resume-suspect check; calling it unconditionally here mirrors
    // `landing_get_current`, which does the same on every poll.
    let live = current_snapshot(&app).map(|snap| {
        let mut l = PanelLiveSnapshot::from_snapshot(&snap);
        // F3: braucht den Zielflughafen, darum hier statt in from_snapshot.
        l.ete_min = panel_ete_min(&snap, &flight.arr_airport, flight.planned_flight_time_min);
        l
    });
    let mut info = flight_info(flight.as_ref(), resume_position_suspect, live);
    info.resume_vom_boden_moeglich = vom_boden_moeglich;
    Some(info)
}

#[derive(Serialize)]
struct FlightLogStatsDto {
    count: u32,
    total_bytes: u64,
}

/// Disk usage of the per-flight JSONL recorder files. Powers the
/// Settings → Speicher section so the user knows how much is on disk
/// before they hit "alle löschen".
#[tauri::command(async)]
fn flight_logs_stats(app: AppHandle) -> Result<FlightLogStatsDto, UiError> {
    // Läuft im Befehls-Faden, nicht im Hauptfaden; kurz halten.
    flugprotokoll_abwarten_hoechstens(Duration::from_secs(2));
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| UiError::new("no_app_data_dir", "no app data dir"))?;
    let s = recorder::flight_logs_stats(dir).map_err(|e| {
        UiError::new(
            "flight_logs_stats_failed",
            format!("could not read flight logs dir: {e}"),
        )
    })?;
    Ok(FlightLogStatsDto {
        count: s.count,
        total_bytes: s.total_bytes,
    })
}

#[derive(Serialize)]
struct DeletedDto {
    deleted: u32,
}

/// Delete every per-flight JSONL recorder file. Triggered manually from
/// Settings — never automatically — so the active flight's log (if it
/// exists) gets removed too. The streamer just recreates it on the
/// next event append.
#[tauri::command(async)]
fn flight_logs_delete_all(app: AppHandle) -> Result<DeletedDto, UiError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| UiError::new("no_app_data_dir", "no app data dir"))?;
    let n = recorder::flight_logs_delete_all(dir).map_err(|e| {
        UiError::new(
            "flight_logs_delete_failed",
            format!("could not delete flight logs: {e}"),
        )
    })?;
    tracing::info!(deleted = n, "flight logs purged (manual)");
    Ok(DeletedDto { deleted: n })
}

/// Delete per-flight JSONL files older than `older_than_days` (mtime).
/// Called from the JS layer once per app launch when the user has the
/// auto-purge toggle on (default 30 days).
#[tauri::command(async)]
fn flight_logs_purge_older_than(
    app: AppHandle,
    older_than_days: u32,
) -> Result<DeletedDto, UiError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|_| UiError::new("no_app_data_dir", "no app data dir"))?;
    // P-7.21: noch nicht hochgeladene Protokolle bleiben stehen.
    let behalten = upload_warteschlange::offene_protokolle(&dir);
    let n = recorder::flight_logs_purge_older_than_ausser(dir, older_than_days, &behalten).map_err(|e| {
        UiError::new(
            "flight_logs_purge_failed",
            format!("could not purge flight logs: {e}"),
        )
    })?;
    if n > 0 {
        tracing::info!(
            deleted = n,
            days = older_than_days,
            "flight logs purged (auto)"
        );
    }
    Ok(DeletedDto { deleted: n })
}
