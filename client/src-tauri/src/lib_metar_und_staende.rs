/// Map our internal `FlightPhase` to the phpVMS PirepStatus code we POST
/// in `update_pirep`. Codes verified against the canonical phpVMS Core
/// enum at `phpvms/phpvms` → `app/Models/Enums/PirepStatus.php`:
///
///   INI Initiated   SCH Scheduled    BST Boarding         RDT Ready
///   PBT Pushback    OFB Off block    DIR Ready de-ice     DIC De-icing
///   GRT Ground rtn  TXI Taxi         TOF Takeoff (roll)   TKO Airborne
///   ICL Init climb  ENR Enroute      DV  Diverted
///   TEN Approach    APR Approach     FIN On final         LDG Landing
///   LAN Landed      ONB On block     ARR Arrived          DX  Cancelled
///   EMG Emerg desc  PSD Paused
///
/// Notes / pitfalls discovered while wiring this up (real bugs):
///   * Pushback must be PBT, not OFB. phpVMS labels `OFB` as
///     "departed" (German: "Abgeflogen") — sending OFB during pushback
///     made the live tracker show "Abgeflogen" before the aircraft had
///     even moved off the gate.
///   * There is no "TOD" or "FAP" or "TXG" in phpVMS — early versions
///     of this client invented those, which would have been silently
///     dropped (or echoed as the literal letters in some VA themes).
///   * `TOF` is the on-runway-takeoff-roll phase, `TKO` is the
///     airborne / climb-out phase. Their human-readable labels
///     collapse to "takeoff" and "enroute" respectively.
///   * Phases the API offers but we don't drive: SCH, DIR/DIC (de-ice),
///     GRT (ground return), DV (divert), EMG, PSD. We may emit them
///     later from manual-end paths.
fn phase_to_status(phase: FlightPhase) -> Option<&'static str> {
    match phase {
        FlightPhase::Preflight | FlightPhase::Boarding => Some("BST"),
        FlightPhase::Pushback => Some("PBT"),
        FlightPhase::TaxiOut => Some("TXI"),
        FlightPhase::TakeoffRoll => Some("TOF"),
        FlightPhase::Takeoff => Some("TKO"),
        FlightPhase::Climb => Some("ICL"),
        // No dedicated "descent" code in phpVMS — the canonical
        // taxonomy stays ENROUTE until the aircraft enters the
        // approach phase (which we cover with APR / FIN below).
        FlightPhase::Cruise | FlightPhase::Descent => Some("ENR"),
        // v0.5.11: phpVMS has no "Holding" status code. ENR is the
        // closest match (we're enroute, just circling). The Cockpit
        // tab UI has its own Holding badge that does NOT depend on
        // this status code mapping.
        FlightPhase::Holding => Some("ENR"),
        FlightPhase::Approach => Some("APR"),
        FlightPhase::Final => Some("FIN"),
        FlightPhase::Landing => Some("LDG"),
        // No separate "taxi to gate" code; phpVMS reuses TXI.
        FlightPhase::TaxiIn => Some("TXI"),
        FlightPhase::BlocksOn => Some("ONB"),
        FlightPhase::Arrived => Some("ARR"),
        FlightPhase::PirepSubmitted => None,
    }
}

/// Trigger one-shot METAR fetches for departure (at takeoff) and arrival
/// (at touchdown). Each side fires only once per flight — the
/// `dep_metar_requested` / `arr_metar_requested` flags on FlightStats
/// gate it. Fetch runs on the Tauri async runtime so the streamer
/// keeps ticking even if aviationweather.gov is slow.
fn maybe_spawn_metar_fetch(app: &AppHandle, flight: &Arc<ActiveFlight>, new_phase: FlightPhase) {
    let kind = match new_phase {
        // v1.5.1 (F2): Boarding zusätzlich zu Takeoff — der Pilot will
        // das Abflugwetter am Gate sehen, nicht erst nach dem Abheben.
        // Das requested-Flag macht den späteren Takeoff-Arm zum No-op.
        // WICHTIG: Boarding ist die STARTphase, es gibt keinen Übergang
        // dorthin — deshalb ruft der Streamer diese Funktion seit v1.5.1
        // auch einmal pro Tick mit der aktuellen Phase auf (billig, die
        // Flags machen alles idempotent). Das deckt zugleich Resume
        // mitten im Sinkflug ab, wo der Descent-Übergang längst vorbei ist.
        FlightPhase::Boarding | FlightPhase::Takeoff => MetarKind::Departure,
        FlightPhase::Descent => MetarKind::ArrivalEarly,
        // Trigger at Final so the arrival weather is in the activity
        // log *before* the touchdown line, not after — pilots want the
        // wind for the approach, not for the rollout.
        FlightPhase::Final => MetarKind::Arrival,
        _ => return,
    };
    let icao = match kind {
        MetarKind::Departure => flight.dpt_airport.clone(),
        MetarKind::ArrivalEarly | MetarKind::Arrival => flight.arr_airport.clone(),
    };
    {
        let mut stats = flight.stats.lock().expect("flight stats");
        let already = match kind {
            MetarKind::Departure => stats.dep_metar_requested,
            MetarKind::ArrivalEarly => stats.arr_metar_early_requested,
            MetarKind::Arrival => stats.arr_metar_requested,
        };
        if already {
            return;
        }
        match kind {
            MetarKind::Departure => stats.dep_metar_requested = true,
            MetarKind::ArrivalEarly => stats.arr_metar_early_requested = true,
            MetarKind::Arrival => stats.arr_metar_requested = true,
        }
    }
    let app = app.clone();
    let flight = Arc::clone(flight);
    tauri::async_runtime::spawn(async move {
        match metar::fetch_metar(&icao).await {
            Ok(snap) => {
                let raw = snap.raw.clone();
                {
                    let mut stats = flight.stats.lock().expect("flight stats");
                    match kind {
                        MetarKind::Departure => {
                            stats.dep_metar_raw = Some(raw.clone());
                            stats.dep_metar_snap = Some(snap.clone());
                        }
                        // Early und Final schreiben dieselben Felder: Final
                        // überschreibt das ältere Descent-Wetter mit dem
                        // frischen — genau so gewollt.
                        MetarKind::ArrivalEarly | MetarKind::Arrival => {
                            stats.arr_metar_raw = Some(raw.clone());
                            stats.arr_metar_snap = Some(snap.clone());
                        }
                    }
                }
                let label = match kind {
                    MetarKind::Departure => "Departure METAR",
                    MetarKind::ArrivalEarly => "Arrival METAR (descent)",
                    MetarKind::Arrival => "Arrival METAR",
                };
                log_activity_handle(
                    &app,
                    ActivityLevel::Info,
                    format!("{label}: {icao}"),
                    Some(if raw.is_empty() {
                        "(empty)".to_string()
                    } else {
                        raw
                    }),
                );
            }
            Err(e) => {
                tracing::warn!(error = %e, %icao, "metar fetch failed");
            }
        }
    });
}

/// v1.6 Stand-Erkennung: OSM-Parkpositionen holen, sobald sie gebraucht
/// werden. Gleiches Muster wie [`maybe_spawn_metar_fetch`] — einmal pro
/// Richtung pro Flug, idempotent über die `*_stands_requested`-Latches,
/// vom Streamer jeden Tick mit der aktuellen Phase aufgerufen (deckt
/// Boarding als Startphase und Resume mitten im Sinkflug ab).
///
/// - **Boarding/Pushback** → Abflughafen: nächste Parkposition zur
///   aktuellen Flugzeugposition wird als `dep_gate` erfasst (der
///   MSFS-`parking_name`-Weg liefert nie — SimConnect befüllt das Feld
///   nicht).
/// - **Descent/Approach/Final** → Zielflughafen: die Standliste wandert
///   nach `stats.arr_stands`, wo das TaxiIn→BlocksOn-Stand-Gate sie
///   auswertet.
///
/// `airport_ground_get` cached lokal pro ICAO (ETag) — im Normalfall ist
/// das ein Plattenzugriff, kein Netz-Roundtrip.

/// Parkpositionen für EINEN Flughafen — die Szenerie zuerst, OSM als Netz.
///
/// v1.7.16: dasselbe Prinzip wie bei den Rollwegen (`boden_karte` weiter
/// unten in dieser Datei) — die Szenerie ist die erste Instanz (X-Plane
/// `apt.dat`, MSFS' `TAXI_PARKING`), OpenStreetMap füllt nur, wo die
/// Szenerie selbst nichts kennt. Anders als OSM braucht die Szenerie
/// weder Netz noch einen Server, der erreichbar sein muss.
///
/// ⚠ `icao` MUSS geprüft werden: eine Szenerie-Auskunft gehört immer zu
/// EINEM bestimmten Platz, aber ohne die Prüfung hier könnte ein Aufrufer
/// sie für den FALSCHEN Flughafen lesen — derselbe Fehler, den die
/// Bahnzuordnung mit dem Divert-Hint schon einmal hatte.
///
/// v1.7.17: zwei getrennte Quellen, beide gegen `icao` geprüft.
/// `stats.szenerie_auskunft` (ZIEL — Ankunft, Divert, oder der tatsächliche
/// Landeplatz) und `stats.dep_szenerie_auskunft` (ABFLUG, unabhängig
/// geerntet — siehe die Doku an diesem Feld für den Grund der Trennung).
/// Ein Aufrufer muss NICHT wissen, welche der beiden zuständig ist — für
/// den Abflugplatz trifft nur die zweite, für den Zielplatz nur die erste
/// (beide tragen ihr eigenes `icao`, ein Treffer in der falschen waere
/// strukturell ausgeschlossen, nicht nur unwahrscheinlich).
///
/// v1.7.20: `position`, wenn bekannt — die Szenerie gewinnt nur
/// bedingungslos ohne Positionskontext (reine Anzeige-Abfragen). Mit
/// Position muss die Szenerie tatsaechlich einen BENANNTEN Stand in der
/// Naehe haben (`benannter_stand_bei`, derselbe Radius/dieselbe Namens-
/// Vorrangregel wie am eigentlichen Verbrauchsort); liefert sie nur
/// IRGENDEINEN Stand fuer den ICAO, egal wo auf dem Flughafen, fiel OSM
/// bisher komplett aus — obwohl OSM oft naeher am tatsaechlichen
/// Parkplatz war (ELY 382, LIMC→LLBG, 07.09.2026: die Szenerie-Ernte
/// hatte einen Rampenstart fuer LLBG, aber keinen nah genug; OSMs
/// naechster benannter Stand lag nur 16 m entfernt und wurde nie
/// angefragt). Kein Rollback der v1.7.16-Entscheidung "Sim vor OSM" —
/// die Szenerie gewinnt weiterhin, sobald sie tatsaechlich etwas in der
/// Naehe hat.
fn standliste_fuer(
    stats: &FlightStats,
    icao: &str,
    osm: Option<&[stands::ParkingStand]>,
    position: Option<(f64, f64)>,
) -> Option<Vec<stands::ParkingStand>> {
    let icao_norm = icao.trim().to_uppercase();
    let treffer = |a: &&Arc<sim_core::szenerie::SzenerieFlughafen>| {
        a.icao.trim().eq_ignore_ascii_case(&icao_norm) && !a.staende.is_empty()
    };
    let szenerie = stats
        .szenerie_auskunft
        .as_ref()
        .filter(treffer)
        .or_else(|| stats.dep_szenerie_auskunft.as_ref().filter(treffer))
        .map(|a| stands::aus_szenerie(&a.staende));
    match (szenerie, position) {
        (Some(liste), Some((lat, lon)))
            if stands::benannter_stand_bei(&liste, lat, lon).is_none() =>
        {
            osm.map(|l| l.to_vec()).or(Some(liste))
        }
        (Some(liste), _) => Some(liste),
        (None, _) => osm.map(|l| l.to_vec()),
    }
}

/// Fuer den BlocksOn/Arrived-Nachzieh-Tick: welches ICAO und welche
/// (ggf. gar keine) OSM-Liste an `standliste_fuer` gehen.
///
/// `None`, solange weder `settle_at_arrival_stand` noch ein Ankunfts-Abruf
/// je liefen.
///
/// ⚠ DREI Felder, DREI verschiedene Fragen:
///
/// * **Wo stehen wir WIRKLICH?** `arr_actual_icao` — von
///   `settle_at_arrival_stand` bei JEDEM Aufruf frisch aus der Position
///   berechnet (`arrival::locate`), unabhaengig von OSM, Szenerie oder
///   einem etwaigen `divert_hint`. Das ist die massgebliche Quelle.
/// * **Wessen Platz haben wir zuletzt bei OSM angefragt?**
///   `arr_stands_requested_icao` — nur als Rueckfall, falls
///   `settle_at_arrival_stand` aus irgendeinem Grund noch nie lief (sollte
///   praktisch nie vorkommen, da die Phasen BlocksOn/Arrived selbst nur ueber
///   einen Aufruf von `settle_at_arrival_stand` erreicht werden — reine
///   Verteidigung).
/// * **Hat OSM fuer GENAU DIESEN Platz geliefert?** `arr_stands_icao` —
///   gesetzt nur bei Erfolg; die OSM-Liste bleibt an dieses Feld gebunden,
///   unabhaengig davon, welches der beiden ICAO-Felder oben den Namen
///   liefert.
///
/// Vor v1.7.16 R1 hing der Nachzieh-Tick ausschliesslich an
/// `arr_stands_icao` — scheiterte OSM, blieb es fuer immer `None`
/// (externe Gegenpruefung, Codex, adversarial, 04.09.2026, Runde 1).
///
/// R1 loeste das ueber `arr_stands_requested_icao`, das aber seinerseits an
/// `divert_hint` haengt (`maybe_spawn_stand_fetch`) — und `divert_hint` wird
/// laut eigenem Kommentar dort „erst beim Arrived-Fallback gemintet". Ein
/// Divert, der die FSM normal ueber TaxiIn→BlocksOn erreicht (kein
/// Fallback noetig), mintet u. U. NIE einen `divert_hint`, und
/// `arr_stands_requested_icao` zeigte dann dauerhaft auf den GEPLANTEN
/// statt den echten Platz (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026, Runde 2). `arr_actual_icao` umgeht das: es kommt aus
/// derselben `arrival::locate`-Berechnung, die auch `at_icao` fuer
/// `settle_at_arrival_stand` liefert, nie aus `divert_hint`.
fn arrival_retry_ziel(stats: &FlightStats) -> Option<(String, Option<&[stands::ParkingStand]>)> {
    let list_icao = stats
        .arr_actual_icao
        .clone()
        .or_else(|| stats.arr_stands_requested_icao.clone())?;
    let osm = stats
        .arr_stands
        .as_deref()
        .filter(|_| stats.arr_stands_icao.as_deref() == Some(list_icao.as_str()));
    Some((list_icao, osm))
}

/// Fuer welchen Flughafen `maybe_spawn_stand_fetch` die Ankunfts-
/// Standliste bei OSM anfragt.
///
/// ⚠ Dieselbe Prioritaet wie `arrival_retry_ziel`, aus demselben Grund:
/// `arr_actual_icao` (frisch aus der Position, unabhaengig von
/// `divert_hint`) VOR `divert_hint.actual_icao` VOR dem geplanten
/// `arr_airport`. Vor v1.7.16 R3 fragte diese Stelle AUSSCHLIESSLICH
/// `divert_hint` — bei einem Divert, der die FSM normal ueber
/// TaxiIn→BlocksOn erreicht (`divert_hint` mintet u. U. NIE), waere der
/// OSM-Abruf fuer immer auf den GEPLANTEN Platz gerichtet geblieben, selbst
/// wenn die Szenerie fuer den echten Platz nichts kennt und OSM die
/// einzige Quelle waere (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026, Runde 3).
fn arrival_fetch_ziel_icao(stats: &FlightStats, geplant: &str) -> String {
    stats
        .arr_actual_icao
        .as_deref()
        .or_else(|| {
            stats
                .divert_hint
                .as_ref()
                .and_then(|h| h.actual_icao.as_deref())
        })
        .unwrap_or(geplant)
        .trim()
        .to_uppercase()
}

/// Ob `icao` (bereits normalisiert) noch das Ankunfts-Ziel ist, fuer das
/// zuletzt eine OSM-Anfrage gestartet wurde — die EINE Stale-Spawn-Pruefung
/// fuer alle drei Stellen, die vom Zielwechsel betroffen sind:
///
/// * `maybe_spawn_stand_fetch`s Zaehler-Reset (negiert: hat sich das Ziel
///   GEAENDERT) — Runde 4.
/// * die Uebernahme von `arr_ground_geojson` — Runde 4.
/// * die Uebernahme von `arr_stands`/`arr_stands_icao` — schon seit dem
///   Erstbau vorhanden, hier nur dieselbe Funktion statt eines eigenen
///   Vergleichs.
///
/// Alle drei benutzten VOR Runde 4 denselben Vergleich an drei getrennten
/// Stellen — zwei davon fehlerhaft oder fehlend (siehe Runde-4-Befunde):
/// der Zaehler-Reset hing zusaetzlich an einem Flag, das ein Fehlversuch
/// vorher zuruecksetzt, und `arr_ground_geojson` hatte GAR KEINEN Riegel
/// (externe Gegenpruefung, Codex, adversarial, 04.09.2026, Runde 4).
fn passt_zum_angefragten_ankunftsziel(stats: &FlightStats, icao: &str) -> bool {
    stats.arr_stands_requested_icao.as_deref() == Some(icao)
}

#[cfg(test)]
#[path = "tests_lib/standliste_fuer_tests.rs"]
mod standliste_fuer_tests;

fn maybe_spawn_stand_fetch(
    app: &AppHandle,
    flight: &Arc<ActiveFlight>,
    phase: FlightPhase,
    tick_count: u32,
) {
    #[derive(Clone, Copy, PartialEq)]
    enum Dir {
        Departure,
        Arrival,
    }
    let dir = match phase {
        FlightPhase::Boarding | FlightPhase::Pushback => Dir::Departure,
        // TaxiIn ist mit dabei (Review-Fund v1.5.4): nach einem Resume
        // MITTEN im Einrollen wäre der Descent-Trigger längst vorbei und
        // das Stand-Gate bliebe ohne Daten — der RYR-1142-Fehlschluss
        // käme durch die Hintertür zurück. (Resume in BlocksOn läuft über
        // den >5-kt-Rückweg ebenfalls nach TaxiIn und damit hierher.)
        FlightPhase::Descent
        | FlightPhase::Approach
        | FlightPhase::Final
        | FlightPhase::TaxiIn
        // BlocksOn/Arrived dazu (QS-Befunde v1.6.1): (a) der Divert-Hint
        // wird erst beim Arrived-Fallback gemintet — ohne diese Phasen
        // koennte der Refetch fuers TATSAECHLICHE Ziel nie feuern;
        // (b) ein Resume direkt in BlocksOn/Arrived haette sonst nie
        // eine Standliste. Der requested-Latch haelt Normalfluege bei
        // genau einem Abruf.
        | FlightPhase::BlocksOn
        | FlightPhase::Arrived => Dir::Arrival,
        _ => return,
    };
    let icao = match dir {
        Dir::Departure => flight.dpt_airport.trim().to_uppercase(),
        // Divert: der Stand gehört zum Platz, auf dem der Flieger WIRKLICH
        // landet. Ohne das lädt eine Ausweichlandung für immer die Liste
        // des geplanten Ziels — und das BlocksOn-Stand-Gate filtert sie
        // (zu Recht) weg, der Stand bleibt leer.
        //
        // ⚠ `arr_actual_icao` VOR `divert_hint`: Letzteres wird laut
        // eigenem Kommentar an seiner Setzstelle „erst beim
        // Arrived-Fallback gemintet" — ein Divert, der die FSM normal
        // ueber TaxiIn→BlocksOn erreicht, mintet u. U. NIE einen
        // `divert_hint`, und dieser OSM-Abruf haette dann fuer immer den
        // GEPLANTEN statt den echten Platz angefragt, selbst wenn die
        // Szenerie fuer den echten Platz keine Staende kennt und OSM die
        // einzige Quelle waere (externe Gegenpruefung, Codex, adversarial,
        // 04.09.2026, Runde 3). `arr_actual_icao` kommt aus derselben
        // `arrival::locate`-Berechnung wie `at_icao` in
        // `settle_at_arrival_stand` — unabhaengig von `divert_hint`.
        Dir::Arrival => {
            let stats = flight.stats.lock().expect("flight stats");
            arrival_fetch_ziel_icao(&stats, &flight.arr_airport)
        }
    };
    if icao.len() < 3 {
        return;
    }
    /// Nach so vielen Fehlversuchen geben wir auf — ein dauerhaft toter
    /// Server soll nicht endlos angefragt werden.
    const STAND_FETCH_MAX_ATTEMPTS: u8 = 5;
    /// Ticks Abstand zwischen Wiederholungen. Ohne Spacing verbrennen
    /// alle fünf Versuche in den ersten ~15 Sekunden des Fluges — genau
    /// dann, wenn WLAN/VPN am wahrscheinlichsten noch nicht steht
    /// (dasselbe Muster wie `ARR_REF_POS_RETRY_EVERY_TICKS`). 30 Ticks
    /// sind ~90 s am Boden; die Antwort wird erst am Stand gebraucht.
    const STAND_FETCH_RETRY_EVERY_TICKS: u32 = 30;
    {
        let mut stats = flight.stats.lock().expect("flight stats");
        // Divert-Wechsel: angefragt wurde ein anderer Platz als der, auf
        // dem wir landen → Anfrage-Latch und Zähler zurücksetzen.
        //
        // ⚠ OHNE `stats.arr_stands_requested`-Bedingung, absichtlich: ein
        // Fehlversuch setzt `arr_stands_requested` bereits VOR diesem
        // Vergleich auf `false` zurück (siehe `fetch_failed` unten), damit
        // der naechste Tick es erneut probiert. Stand die Bedingung noch
        // hier, griff dieser Reset NICHT, wenn genau in dem Tick, in dem
        // sich das Ziel aendert, kein Abruf mehr "in Flug" war — der neue
        // (tatsaechliche) Flughafen erbte dann das Zaehler-Budget des
        // ALTEN (geplanten): nach vier Fehlversuchen fuer den geplanten
        // Platz blieb dem echten Platz nur noch EIN Versuch, und ein
        // einziger Netzwackler haette seinen OSM-Rueckfall dauerhaft
        // stillgelegt — exakt der Fall, den `arr_actual_icao` eigentlich
        // reparieren sollte (externe Gegenpruefung, Codex, adversarial,
        // 04.09.2026, Runde 4).
        if dir == Dir::Arrival && !passt_zum_angefragten_ankunftsziel(&stats, &icao) {
            stats.arr_stands_requested = false;
            stats.arr_stand_fetch_attempts = 0;
        }
        let (already, attempts) = match dir {
            Dir::Departure => (stats.dep_stands_requested, stats.dep_stand_fetch_attempts),
            Dir::Arrival => (stats.arr_stands_requested, stats.arr_stand_fetch_attempts),
        };
        if already {
            return;
        }
        // Wiederholungen (attempts > 0) laufen nur im Raster — der
        // Erstversuch sofort.
        if attempts > 0
            && (attempts >= STAND_FETCH_MAX_ATTEMPTS
                || !tick_count.is_multiple_of(STAND_FETCH_RETRY_EVERY_TICKS))
        {
            return;
        }
        match dir {
            Dir::Departure => stats.dep_stands_requested = true,
            Dir::Arrival => {
                stats.arr_stands_requested = true;
                stats.arr_stands_requested_icao = Some(icao.clone());
            }
        }
    }
    let app = app.clone();
    let flight = Arc::clone(flight);
    tauri::async_runtime::spawn(async move {
        // JEDER erfolglose Ausgang gibt das requested-Flag zurueck, damit
        // der naechste Raster-Tick es erneut probiert. Wichtig dabei:
        // `airport_ground_get` maskiert Netzfehler ohne lokale Kopie als
        // `Ok(None)` — ein reiner Err-Arm waere fuer den realen Fehlerfall
        // unerreichbar (QS-Befund B1 zur ersten Fassung dieses Fixes).
        // Ein echtes „dieser Platz hat keine Bodendaten" (404) kostet so
        // maximal STAND_FETCH_MAX_ATTEMPTS gerasterte Anfragen, dann Ruhe.
        let fetch_failed = |flight: &Arc<ActiveFlight>, grund: &str| {
            let mut stats = flight.stats.lock().expect("flight stats");
            // Stale-Spawn-Guard (QS-Befund): ein Divert-Reset kann diesen
            // Spawn ueberholt haben — dann gehoert der Fehlversuch einem
            // ICAO, das niemand mehr angefragt hat. Zaehler/Latch des
            // NEUEN Ziels nicht anfassen.
            if dir == Dir::Arrival && !passt_zum_angefragten_ankunftsziel(&stats, &icao) {
                return;
            }
            let attempts = match dir {
                Dir::Departure => {
                    stats.dep_stand_fetch_attempts =
                        stats.dep_stand_fetch_attempts.saturating_add(1);
                    stats.dep_stand_fetch_attempts
                }
                Dir::Arrival => {
                    stats.arr_stand_fetch_attempts =
                        stats.arr_stand_fetch_attempts.saturating_add(1);
                    stats.arr_stand_fetch_attempts
                }
            };
            if attempts < STAND_FETCH_MAX_ATTEMPTS {
                match dir {
                    Dir::Departure => stats.dep_stands_requested = false,
                    Dir::Arrival => stats.arr_stands_requested = false,
                }
                tracing::info!(%grund, attempts, "stand-detection ground fetch failed — will retry");
            } else {
                tracing::warn!(%grund, "stand-detection ground fetch gave up after {attempts} attempts");
            }
        };
        let ground = match airport_ground_get(app, icao.clone()).await {
            Ok(Some(g)) => g,
            Ok(None) => {
                fetch_failed(&flight, "keine Daten (404 oder Netzfehler ohne Cache)");
                return;
            }
            Err(e) => {
                fetch_failed(&flight, &e.message);
                return;
            }
        };
        // Dieselbe Karte trägt die Rollwege — und damit die Ausfahrten der
        // Bahn. Sie wird für die Ankunft aufgehoben, weil die gematchte Bahn
        // erst beim Aufsetzen feststeht (siehe `arr_ground_geojson`).
        //
        // ⚠ Denselben Stale-Spawn-Riegel wie unten fuer `arr_stands`
        // ANWENDEN, nicht nur dort: diese Zuweisung lief vorher
        // BEDINGUNGSLOS, bevor der Riegel geprueft wird. Aendert sich das
        // Ziel waehrend eine Anfrage fuer den ALTEN (geplanten) Platz noch
        // unterwegs ist — genau der Fall, den `arr_actual_icao`
        // ueberhaupt erst ermoeglicht — konnte deren VERSPAETETE Antwort
        // die inzwischen fuer den ECHTEN Platz geladene Bodenkarte
        // ueberschreiben. `bahn_felder` liest diese Karte fuer die
        // Rollweg-Ausfahrten ohne eigene ICAO-Pruefung; eine falsch
        // zugeordnete Karte haette falsche oder fehlende Ausfahrten
        // erzeugt, ohne dass irgendwo ein Fehler sichtbar wuerde (externe
        // Gegenpruefung, Codex, adversarial, 04.09.2026, Runde 4).
        if matches!(dir, Dir::Arrival) {
            // Vor der Sperre geteilt angelegt — die Karte kann mehrere
            // Megabyte gross sein (v2.0.17: keine Arbeit in Flughafengroesse
            // unter der Flugdaten-Sperre).
            let karte: Arc<str> = Arc::from(ground.geojson.as_str());
            // Auch ohne OSM-Parkpositionen: die Stände können aus der
            // Szenerie kommen, die Rollwege nur von hier (Codex-QS 27.10.).
            let rollwege = stands::parse_rollwege(&ground.geojson);
            let mut stats = flight.stats.lock().expect("flight stats");
            if passt_zum_angefragten_ankunftsziel(&stats, &icao) {
                // ⚠ Karte UND ihre Zugehoerigkeit in DERSELBEN
                // Lock-Haltung setzen — sonst koennte ein Zielwechsel
                // zwischen beiden Zeilen eine Karte ohne (oder mit
                // falscher) Kennzeichnung hinterlassen.
                stats.arr_ground_geojson = Some(karte);
                stats.arr_ground_geojson_icao = Some(icao.clone());
                stats.arr_rollwege = Some(rollwege);
                stats.arr_rollwege_icao = Some(icao.clone());
            } else {
                tracing::info!(%icao, "stale ground map dropped (target changed mid-fetch)");
            }
        }

        let list = stands::parse_stands(&ground.geojson);
        tracing::info!(%icao, stands = list.len(), "parking stands loaded for stand detection");
        if list.is_empty() {
            fetch_failed(&flight, "Bodendaten ohne Parkpositionen");
            return;
        }
        let mut stats = flight.stats.lock().expect("flight stats");
        match dir {
            Dir::Departure => {
                // Erstversuch sofort — aber NUR im Boarding: da steht der
                // Flieger auf seinem Stand. Ein spaet antwortender Fetch
                // (Retry nach Netzfehler) darf nach Pushback/Rollbeginn
                // keinen fremden Nachbar-Stand mehr stempeln (QS-Befund).
                // Nicht der einzige Versuch: die Liste bleibt liegen, der
                // Boarding-Tick probiert weiter (Befund OCN 1408).
                if stats.phase == FlightPhase::Boarding && stats.dep_gate.is_none() {
                    if let Some((lat, lon)) = aircraft_position_for_gates(&stats) {
                        if let Some((s, _)) = stands::benannter_stand_bei(&list, lat, lon) {
                            if let Some(name) = &s.name {
                                stats.dep_gate = Some(name.clone());
                                tracing::info!(%icao, stand = %name, "departure stand captured from OSM ground data");
                            }
                        }
                    }
                }
                stats.dep_stands = Some(list);
            }
            Dir::Arrival => {
                // Stale-Spawn-Guard (QS-Befund): nach einem Divert-Reset
                // darf die VERSPAETETE Antwort des alten Ziels die schon
                // geladene Liste des echten Ziels nicht ueberschreiben.
                if passt_zum_angefragten_ankunftsziel(&stats, &icao) {
                    stats.arr_stands = Some(list);
                    stats.arr_stands_icao = Some(icao);
                } else {
                    tracing::info!(%icao, "stale stand list dropped (target changed mid-fetch)");
                }
            }
        }
    });
}

/// Internal discriminator for the METAR fetch directions.
///
/// v1.5.1 (#hud-pilotenfeedback F2): `ArrivalEarly` kam dazu — das
/// In-Sim-HUD zeigt das Zielwetter schon ab dem Sinkflug. Bewusst eine
/// EIGENE Variante mit eigenem Flag statt den `Arrival`-Fetch
/// vorzuziehen: das früh geholte METAR ist beim Aufsetzen 20–40 Minuten
/// alt, deshalb holt `Final` weiterhin FRISCH nach und überschreibt.
#[derive(Debug, Clone, Copy)]
enum MetarKind {
    Departure,
    ArrivalEarly,
    Arrival,
}
