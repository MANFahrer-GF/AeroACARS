/// How good is the reference position we currently hold? Higher wins — a weaker
/// source may never overwrite a better one, and the resolver keeps looking until
/// it has the best (Navigraph's fetch is asynchronous and lands seconds into the
/// flight).
const ARR_REF_SOURCE_NONE: u8 = 0;
const ARR_REF_SOURCE_OURAIRPORTS: u8 = 1;
const ARR_REF_SOURCE_PHPVMS: u8 = 2;
const ARR_REF_SOURCE_NAVIGRAPH: u8 = 3;

/// Where is this airport? Asked of three sources, in order of how much they
/// deserve to be believed:
///
///   1. **Navigraph** (the active AIRAC cycle, cached per flight). Current,
///      aviation-grade, and the source Thomas re-uploads every cycle.
///   2. **phpVMS** — the VA's own airport record. Hand-maintained, occasionally
///      empty or (0,0), but it knows the fields the VA actually flies to.
///   3. **OurAirports** — the table embedded in the binary. Complete (85,729
///      idents, so it has the bush strips and helipads the others don't) but
///      public-domain data of uneven quality: KCLE has a threshold four miles
///      off the field, one UUMU runway is filed in Belgorod.
///
/// The order matters more than it looks. Every self-inflicted bug in this area
/// came from treating the weakest source as the truth: repairing OurAirports
/// against itself put WAJI 5 nm from where it is, and only asking phpVMS made
/// divert detection depend on whether the UI happened to have fetched an
/// airport.
/// Returns the position and WHICH source it came from, so the caller can tell
/// whether it is worth asking again later (Navigraph's fetch is asynchronous).
fn airport_reference_pos(
    icao: &str,
    nav_pos: Option<(f64, f64)>,
    phpvms_pos: Option<(f64, f64)>,
) -> (Option<(f64, f64)>, u8) {
    if let Some(pos) = nav_pos {
        return (Some(pos), ARR_REF_SOURCE_NAVIGRAPH);
    }
    if let Some((lat, lon)) = phpvms_pos {
        // phpVMS ships (0,0) for airports whose coordinates were never filled in.
        if !(lat == 0.0 && lon == 0.0) && lat.is_finite() && lon.is_finite() {
            return (Some((lat, lon)), ARR_REF_SOURCE_PHPVMS);
        }
    }
    match runway::airport_reference(icao) {
        Some(pos) => (Some(pos), ARR_REF_SOURCE_OURAIRPORTS),
        None => (None, ARR_REF_SOURCE_NONE),
    }
}

/// v0.8.0: Lädt Navdata für bis zu 3 ICAOs (dep/arr/alt) parallel vom
/// VPS und schreibt sie in `flight.navdata`. Best-effort — bei Netzwerk-
/// Fehlern bleibt der Cache leer und der Pilot-Client fällt auf
/// OurAirports zurück. Wird vom flight_start-Pfad in einer Background-
/// Task aufgerufen, blockiert den Flugstart nicht.
///
/// Behavior:
///   * Skippt leere ICAO-Strings (z.B. wenn der Bid keinen Alternate hat).
///   * Dedupe per uppercase-ICAO (dep == arr im local-pattern-flight).
///   * 404 vom VPS → ICAO nicht im aktiven Cycle → silently skip.
///   * 401/403 → loggt warn! (Token fehlt/invalid) — alle weiteren
///     Fetches landen ebenfalls in 401, restliche ICAOs werden trotzdem
///     versucht damit der Logger sie alle protokolliert.
///   * Network-Fehler → loggt warn!, restliche ICAOs werden trotzdem versucht.
///
/// `auth_token` ist der Pilot-Bearer-Token (typisch
/// `ProvisionResponse.password` aus dem MQTT-Provisioning). `None` ist
/// nur für Tests/Mocks vorgesehen — gegen den echten VPS liefert das
/// 401 und der OurAirports-Fallback greift transparent.
async fn fetch_navdata_for_flight(
    app: AppHandle,
    flight: Arc<ActiveFlight>,
    icaos: Vec<String>,
    base_override: Option<String>,
    auth_token: Option<String>,
) {
    let mut wanted: Vec<String> = icaos
        .into_iter()
        .map(|s| s.trim().to_uppercase())
        .filter(|s| !s.is_empty() && s.len() == 4)
        .collect();
    wanted.sort();
    wanted.dedup();
    if wanted.is_empty() {
        return;
    }

    // v0.8.0: parallel den Aircraft-Aliases-Fetch starten. Damit ist
    // der AppState-Cache spätestens am Touchdown-Edge warm — und ein
    // frisch vom VA-Admin im Webapp-Tab hinzugefügter Alias greift
    // beim nächsten Flight ohne Pilot-Client-Restart.
    let aliases_handle = {
        let app_aliases = app.clone();
        let base_aliases = base_override.clone();
        let token_aliases = auth_token.clone();
        tokio::spawn(async move {
            fetch_aircraft_aliases_into_state(app_aliases, base_aliases, token_aliases).await;
        })
    };

    let mut handles = Vec::with_capacity(wanted.len());
    for icao in wanted {
        let base = base_override.clone();
        let token = auth_token.clone();
        handles.push(tokio::spawn(async move {
            let res =
                aeroacars_mqtt::navdata::get_airport(&icao, base.as_deref(), token.as_deref())
                    .await;
            (icao, res)
        }));
    }

    for h in handles {
        let (icao, res) = match h.await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(error = %e, "navdata fetch task join failed");
                continue;
            }
        };
        match res {
            Ok(mut airport) => {
                let cycle = airport.cycle.clone();
                // Auf die Platte, bevor irgendetwas anderes passiert.
                // Beim naechsten Flug ohne Netz ist genau das der
                // Unterschied zwischen Navigraph und OurAirports.
                navdata_cache::ablegen(&app, &icao, &airport, &cycle);
                // Phantom-Bahn-Duplikate (siehe `runway::dedupe_near_duplicate_nav_runways`)
                // hier EINMAL pro Airport rausfiltern — vor dem Cache-Insert, damit
                // JEDER Konsument von `flight.navdata` (Touchdown-Matcher UND der
                // Approach-Gleitwinkel in `runway_glideslope_for`) automatisch die
                // bereinigte Liste sieht, statt dass jeder Aufrufer selbst dedupen muss.
                airport.runways =
                    runway::dedupe_near_duplicate_nav_runways(&airport.icao, airport.runways);
                let mut cache = flight.navdata.lock().expect("navdata lock");
                cache.airports.insert(icao.clone(), airport);
                if cache.cycle.is_none() {
                    cache.cycle = Some(cycle.clone());
                }
                tracing::info!(
                    icao = %icao,
                    cycle = %cycle,
                    "navdata: VPS-Daten geladen"
                );
            }
            Err(aeroacars_mqtt::navdata::NavdataError::NotFound(_)) => {
                // Der Platz steht nicht im aktiven Zyklus. Ein abgelegter
                // Stand hilft hier NICHT weiter: Wenn Navigraph ihn heute
                // nicht kennt, ist ein alter Eintrag keine bessere
                // Auskunft, sondern eine ueberholte.
                tracing::debug!(
                    icao = %icao,
                    "navdata: ICAO nicht im aktiven Cycle, OurAirports-Fallback aktiv"
                );
            }
            Err(e) => {
                // Server nicht erreichbar — hier zaehlt der abgelegte
                // Stand. Er ist die ZWEITE Wahl, nie die erste: Solange
                // der Server antwortet, gilt der Server, sonst haetten
                // wir einen Bestand, der sich selbst bestaetigt.
                match navdata_cache::holen(&app, &icao) {
                    Ok(eintrag) => {
                        let tage = eintrag.alter_tage_jetzt();
                        let mut airport = eintrag.airport;
                        airport.runways = runway::dedupe_near_duplicate_nav_runways(
                            &airport.icao,
                            airport.runways,
                        );
                        let zyklus = eintrag.zyklus.clone();
                        {
                            let mut cache = flight.navdata.lock().expect("navdata lock");
                            cache.airports.insert(icao.clone(), airport);
                            if cache.cycle.is_none() {
                                cache.cycle = Some(zyklus.clone());
                            }
                        }
                        // Bewusst `warn`, nicht `info`: Eine Bewertung auf
                        // altem Stand darf im Protokoll nicht so aussehen
                        // wie eine auf heutigem — sonst sucht man den
                        // Fehler spaeter ueberall ausser hier.
                        tracing::warn!(
                            icao = %icao,
                            error = %e,
                            cycle = %zyklus,
                            alter_tage = tage,
                            "navdata: Server nicht erreichbar, benutze abgelegten Stand"
                        );
                    }
                    Err(grund) => {
                        tracing::warn!(
                            icao = %icao,
                            error = %e,
                            grund = ?grund,
                            "navdata: VPS-Fetch fehlgeschlagen, OurAirports-Fallback"
                        );
                    }
                }
            }
        }
    }

    // Wait for the aliases-fetch to settle so any debug-logs are
    // ordered before the Activity-Log line below. Join errors are
    // harmless — the cache stays empty and the hardcoded table covers.
    let _ = aliases_handle.await;

    // Surface the final state in the Activity-Log so the pilot sees
    // which source backs the runway assessment for this flight.
    let (cycle, loaded): (Option<String>, Vec<String>) = {
        let cache = flight.navdata.lock().expect("navdata lock");
        (
            cache.cycle.clone(),
            cache.airports.keys().cloned().collect(),
        )
    };
    let state = app.state::<AppState>();
    if let Some(c) = cycle.as_deref() {
        let mut sorted = loaded.clone();
        sorted.sort();
        log_activity(
            &state,
            ActivityLevel::Info,
            format!("Navdata AIRAC {} geladen für {}", c, sorted.join("/")),
            None,
        );
    } else {
        log_activity(
            &state,
            ActivityLevel::Warn,
            "Navdata: OurAirports-Fallback aktiv (VPS nicht erreichbar)".to_string(),
            None,
        );
    }
}

/// v0.8.0: Lädt die VPS-managed Aircraft-Aliases und schreibt sie in
/// `AppState.aircraft_aliases_vps`. Best-effort — bei Netzwerk-Fehlern
/// bleibt nur die hardcoded `aircraft_aliases`-Tabelle aktiv. Wird
/// neben dem navdata-Fetch beim flight_start aufgerufen damit ein
/// frisch im VPS-Admin-Tab hinzugefügter Alias beim nächsten Flight
/// greift ohne App-Restart.
async fn fetch_aircraft_aliases_into_state(
    app: AppHandle,
    base_override: Option<String>,
    auth_token: Option<String>,
) {
    let res = aeroacars_mqtt::navdata::get_aircraft_aliases(
        base_override.as_deref(),
        auth_token.as_deref(),
    )
    .await;
    match res {
        Ok(list) => {
            let state = app.state::<AppState>();
            let mut cache = state
                .aircraft_aliases_vps
                .lock()
                .expect("aircraft_aliases_vps lock");
            cache.clear();
            for entry in list {
                let icao_up = entry.icao.trim().to_uppercase();
                let aliases: Vec<String> = entry
                    .aliases
                    .into_iter()
                    .map(|a| a.alias.trim().to_uppercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !aliases.is_empty() {
                    cache.insert(icao_up, aliases);
                }
            }
            tracing::info!(count = cache.len(), "aircraft_aliases: VPS-Liste geladen");
        }
        Err(e) => {
            tracing::debug!(
                error = %e,
                "aircraft_aliases: VPS-Fetch fehlgeschlagen, nur hardcoded Tabelle aktiv"
            );
        }
    }
}

/// Helper für die ActiveFlight-Init-Sites: spawnt den Navdata-Fetch in
/// einer Background-Task ohne den flight_start zu blockieren. Token
/// kommt aus dem MQTT-Provisioning (`MQTT_KEYRING_PASSWORD`) — dasselbe
/// Token autoirisiert auch den MQTT-Login. Bei Token-Fehlen sendet der
/// Fetcher kein Auth-Header und lädt damit bei keinem produktiven VPS
/// (= 401), aber der OurAirports-Fallback greift transparent.
///
/// `icaos` enthält die zu ladenden Airports (typisch dep/arr/alt).
/// Leere Strings + Nicht-4-Zeichen-ICAOs werden gefiltert.
///
/// **Per-ICAO Idempotency-Guard** (v0.8.0 QS): es werden nur die
/// ICAOs gefetcht, die nicht bereits im Cache sind. So bleibt der
/// Helper safe to call mehrfach — ein nach-Plan-Init-Aufruf der
/// erstmals den Alternate kennt, fetcht nur den Alternate; alle
/// schon geladenen Airports werden übersprungen.
/// Die Szenerie-Auskunft des Simulators für einen Zielflughafen anfordern.
///
/// Nur MSFS: Dort muss über SimConnect gefragt werden, und die Antwort
/// kommt asynchron über mehrere Durchläufe. X-Plane braucht das nicht —
/// dort liegt die Szenerie als Datei auf der Platte und wird beim
/// Aufsetzen in unter einer Millisekunde gelesen.
///
/// ⚠ Beim Aufsetzen anzufordern wäre zu spät. Deshalb hängt der Aufruf
/// an `spawn_navdata_fetch`, also an jedem Moment, in dem ein Ziel
/// bekannt wird.
/// Das Szenerie-Auftragsbuch fuer einen neuen Flug leeren.
///
/// ⚠ Der Anfragezustand gehoert dem FLUG. Ohne diesen Schnitt nimmt der
/// naechste Flug die verbrauchten Versuche, die dauerhaften Ablehnungen
/// und die gelieferte Szenerie des vorigen mit — und fragt fuer sein
/// eigenes Ziel nie wieder (QS-Befund 2, dritte Runde).
fn szenerie_buch_zuruecksetzen(app: &AppHandle) {
    #[cfg(target_os = "windows")]
    {
        // ⚠ Dieselbe Form wie in `szenerie_anfordern_fuer` — GENAU
        // deshalb.
        //
        // Der erste Entwurf schrieb `if let Ok(msfs) = state.msfs.lock()`.
        // Das uebersetzt hier nicht: Der Ergebnis-Zwischenwert lebt bis
        // zum Ende des `if let`, `state` faellt aber vorher. Der Mac hat
        // es nicht gesehen — der ganze Block steht hinter
        // `cfg(target_os = "windows")` —, die Windows-CI schon.
        let state = app.state::<AppState>();
        let msfs = state.msfs.lock().expect("msfs lock");
        msfs.szenerie_zuruecksetzen();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
    }
}

fn szenerie_anfordern_fuer(app: &AppHandle, icaos: &[String]) {
    #[cfg(target_os = "windows")]
    {
        let state = app.state::<AppState>();
        let msfs = state.msfs.lock().expect("msfs lock");
        for icao in icaos {
            let i = icao.trim().to_ascii_uppercase();
            if i.len() == 4 {
                msfs.szenerie_anfordern_mit_rang(&i, RANG_VORMERKUNG);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, icaos);
    }
}

/// Welchen Platz die Szenerie-Auskunft betreffen muss.
///
/// Der geplante Zielflughafen — es sei denn, ein Ausweichanflug ist
/// erkannt, dann der tatsaechliche. Beides zusammen, damit im Anflug
/// beide gefragt werden: Ein Ausweichentschluss faellt spaet, und die
/// Antwort braucht Vorlauf.
///
/// ⚠ Die Reihenfolge ist die Rangfolge. Das Ausweichziel steht vorn,
/// weil dort gelandet wird.
/// Ob jetzt ein neuer Versuchsvorrat faellig ist.
///
/// Bei JEDEM Wechsel in eine Landephase — nicht einmal je Flug.
///
/// ⚠ Warum nicht einmal: Der Vorrat reicht fuer zehn Versuche, also zehn
/// Minuten. Ein Fenster beim ersten `Descent` ist bei einem langen
/// Sinkflug, einem Holding oder einem Durchstarten laengst wieder leer,
/// bevor `Approach` erreicht ist — und dann wird im Anflug wieder kein
/// einziges Mal gefragt (QS-Befund 1, vierte Runde, P0).
///
/// Der Bestand belegt das: Der PTO705-Test faehrt
/// `Final → Climb → Descent → Approach`, also einen Doppelanflug. Mit
/// einem einzigen Fenster haette der zweite Anflug nichts mehr.
///
/// ⚠ Und warum nicht bei jedem Durchlauf: Sonst fuellte sich der Vorrat
/// staendig auf und die Obergrenze waere wirkungslos. Gebunden ist er an
/// den WECHSEL.
///
/// ⚠⚠ `vorher` ist die Phase des VORIGEN Durchlaufs — irgendeine, nicht
/// die zuletzt geoeffnete Landephase.
///
/// Der Unterschied ist nicht theoretisch: Wer die zuletzt GEOEFFNETE
/// festhaelt, verliert `Approach → Holding → Approach`. Beim Verlassen
/// nach `Holding` bliebe `Some(Approach)` stehen, und beim Wiedereintritt
/// verglichen wir Approach mit Approach — kein Fenster. Ein laengeres
/// Holding verbraucht die zehn Versuche, und der naechste Vorrat kaeme
/// erst in `Final`, also unter 700 ft (QS-Befund 1, fuenfte Runde).
///
/// Diese Folge gibt es im Bestand als Fixture.
// Nur auf Windows produktiv aufgerufen (Szenerie-Ernte im
// `cfg(target_os = "windows")`-Block). Die Funktion steht hier
// ABSICHTLICH ausserhalb davon, damit die Tests sie auf JEDER
// Plattform fahren koennen — siehe Doc-Kommentar. Auf Nicht-Windows
// fehlt dadurch der einzige Nicht-Test-Aufrufer, und `cargo check`
// (nur lib-Target, ohne Tests) meldet sie als ungenutzt. Auf Windows
// bleibt die Warnung scharf und wuerde echten toten Code zeigen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn braucht_neues_versuchsfenster(phase: FlightPhase, vorher: Option<FlightPhase>) -> bool {
    matches!(
        phase,
        FlightPhase::Descent | FlightPhase::Approach | FlightPhase::Final
    ) && vorher != Some(phase)
}

/// Entscheidung UND neuer Zustand in einem — damit beides pruefbar ist.
///
/// ⚠ Warum diese Funktion existiert: Die Fortschreibung lag im
/// `cfg(target_os = "windows")`-Block. Eine Gegenprobe, die sie kaputt
/// machte („Phase nur bei faelligem Fenster fortschreiben" — genau der
/// gemeldete Fehler), blieb auf dem Mac GRUEN, weil der Block hier gar
/// nicht uebersetzt wird. Die Entscheidung war geprueft, ihre
/// Verdrahtung nicht.
///
/// Der zweite Rueckgabewert ist IMMER `Some(phase)`: Das Feld ist die
/// vorige Phase, kein Merker fuer geoeffnete Fenster.
// Nur auf Windows produktiv aufgerufen (Szenerie-Ernte im
// `cfg(target_os = "windows")`-Block). Die Funktion steht hier
// ABSICHTLICH ausserhalb davon, damit die Tests sie auf JEDER
// Plattform fahren koennen — siehe Doc-Kommentar. Auf Nicht-Windows
// fehlt dadurch der einzige Nicht-Test-Aufrufer, und `cargo check`
// (nur lib-Target, ohne Tests) meldet sie als ungenutzt. Auf Windows
// bleibt die Warnung scharf und wuerde echten toten Code zeigen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn versuchsfenster_fortschreiben(
    phase: FlightPhase,
    vorher: Option<FlightPhase>,
) -> (bool, Option<FlightPhase>) {
    (braucht_neues_versuchsfenster(phase, vorher), Some(phase))
}

/// Ob die Auskunft am Flug durch eine neuere ersetzt werden muss.
///
/// ⚠ Die alte Regel war „nur wenn noch nichts mit diesem Platz da ist".
/// Damit gewann die neueste Lieferung nur IM BUCH, nicht am Flug:
///
/// * Versuch 1 liefert verspaetet und wird kopiert; Versuch 2 liefert
///   danach aktueller und gewinnt im Buch — der Flug behaelt Versuch 1.
/// * Nach einer neuen Verbindung wird das Buch geleert und neu gefragt;
///   die alte Flugkopie bleibt, und die frische Lieferung desselben
///   Flughafens kommt nie an.
///
/// Beim Aufsetzen wird genau diese Kopie fuer Bahngeometrie und
/// Rollwege benutzt (QS-Befund 2, siebte Runde).
///
/// Der **Stand** ist die Kennung des Versuchs, aus dem die Lieferung
/// stammt. Sie waechst streng — auch ueber einen Verbindungswechsel
/// hinweg, weil `zuruecksetzen` die Kennungen absichtlich nicht
/// zuruecksetzt.
// Nur auf Windows produktiv aufgerufen (Szenerie-Ernte im
// `cfg(target_os = "windows")`-Block). Die Funktion steht hier
// ABSICHTLICH ausserhalb davon, damit die Tests sie auf JEDER
// Plattform fahren koennen — siehe Doc-Kommentar. Auf Nicht-Windows
// fehlt dadurch der einzige Nicht-Test-Aufrufer, und `cargo check`
// (nur lib-Target, ohne Tests) meldet sie als ungenutzt. Auf Windows
// bleibt die Warnung scharf und wuerde echten toten Code zeigen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn auskunft_ersetzen(alt: Option<(&str, u32)>, neu_icao: &str, neu_stand: u32) -> bool {
    match alt {
        None => true,
        Some((alt_icao, alt_stand)) => {
            !alt_icao.eq_ignore_ascii_case(neu_icao) || neu_stand > alt_stand
        }
    }
}

/// Ob die Flugkopie der Szenerie zu entwerten ist.
///
/// ⚠ Die Standnummer allein reicht nicht. Sie loest nur den Fall, dass
/// nach einem Verbindungswechsel tatsaechlich eine neue Lieferung
/// eintrifft. Bleibt sie aus — andere Simulatorfassung, gescheiterte
/// Registrierung, asynchron abgelehnte Definition, stummer Zielplatz —,
/// blieb die Kopie der ALTEN Verbindung am Flug und wurde beim Aufsetzen
/// benutzt. Das Buch gibt in diesem Zustand bewusst nichts heraus; die
/// bereits kopierte Auskunft umging den Riegel (QS-Befund 2, achte
/// Runde).
// Nur auf Windows produktiv aufgerufen (Szenerie-Ernte im
// `cfg(target_os = "windows")`-Block). Die Funktion steht hier
// ABSICHTLICH ausserhalb davon, damit die Tests sie auf JEDER
// Plattform fahren koennen — siehe Doc-Kommentar. Auf Nicht-Windows
// fehlt dadurch der einzige Nicht-Test-Aufrufer, und `cargo check`
// (nur lib-Target, ohne Tests) meldet sie als ungenutzt. Auf Windows
// bleibt die Warnung scharf und wuerde echten toten Code zeigen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn flugkopie_entwerten(alt_generation: u32, buch_generation: u32) -> bool {
    alt_generation != buch_generation
}

/// Einen Szenerie-Schnappschuss in den Flugzustand uebernehmen.
///
/// ⚠ EINE Funktion fuer die ganze Uebernahme — und sie steht
/// ausserhalb von `cfg(windows)`, damit ein Test die echte Kette
/// fahren kann: Buch → Schnappschuss → Flugzustand → Meldung.
///
/// Vorher lag die Uebernahme im Windows-Block, und der Test pruefte
/// Buchformatierung und Durchreichung GETRENNT: Er baute die Diagnose
/// aus dem Buch (LKTB, 243 Rollwege) und setzte danach eine separat
/// erzeugte Auskunft (DAAG, 0 Rollwege) daneben. Eine verlorene,
/// veraltete oder falsch zugeordnete Diagnose waere gruen geblieben
/// (QS-Befund 2, elfte Runde).
// Nur auf Windows produktiv aufgerufen (Szenerie-Ernte im
// `cfg(target_os = "windows")`-Block). Die Funktion steht hier
// ABSICHTLICH ausserhalb davon, damit die Tests sie auf JEDER
// Plattform fahren koennen — siehe Doc-Kommentar. Auf Nicht-Windows
// fehlt dadurch der einzige Nicht-Test-Aufrufer, und `cargo check`
// (nur lib-Target, ohne Tests) meldet sie als ungenutzt. Auf Windows
// bleibt die Warnung scharf und wuerde echten toten Code zeigen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn schnappschuss_uebernehmen(
    stats: &mut FlightStats,
    ziel: Option<&str>,
    geplantes_ziel: &str,
    schnapp: sim_core::szenerie::Schnappschuss,
) {
    // ⚠ Die Diagnose IMMER mitschreiben, auch wenn keine Auskunft kam.
    // Genau der Fall ist der interessante: Sie sagt dann, ob nie
    // gefragt wurde, ob SimConnect abgelehnt hat oder ob die Antwort
    // ausblieb. Frueher fiel das alles unter "navdaten".
    stats.szenerie_diagnose = Some(schnapp.diagnose);

    // ⚠ Das ganze `Option` ersetzen — EINSCHLIESSLICH `None`.
    //
    // Die Vorfassung schrieb `if kennung.is_some()`. Damit ueberlebte
    // die Kennung der ALTEN Verbindung jeden Wechsel, bei dem noch
    // keine neue gemeldet war — und der Flug behauptete eine
    // Simulatorfassung, die gar nicht mehr verbunden ist (QS-Befund 1,
    // elfte Runde).
    stats.sim_kennung = schnapp.kennung;

    // ⚠ Die Flugkopie darf NUR den aktuellen Erntplatz halten.
    //
    // Wechselt das Ernteziel von LEZL auf LEMG und hat LEMG noch keine
    // Lieferung, ist `schnapp.auskunft` richtig `None` — und die alte
    // LEZL-Kopie blieb liegen. Ihre ROLLWEGE gehen ohne ICAO-Pruefung in
    // die Ausfahrten. Damit entsteht ueber den Cache genau der Rueckfall
    // auf das geplante Ziel, den `szenerie_ernteziel` verbietet
    // (QS-Befund 1, zwoelfte Runde).
    //
    // Der Generationsvergleich unten faengt das NICHT: Beim Ausweichflug
    // wechselt das Ziel, nicht die Verbindung.
    let fremd = match (stats.szenerie_auskunft.as_ref(), ziel) {
        (Some(a), Some(z)) => !a.icao.eq_ignore_ascii_case(z),
        (Some(_), None) => true,
        _ => false,
    };
    if fremd {
        tracing::info!(
            alt = stats.szenerie_auskunft.as_ref().map(|a| a.icao.as_str()),
            ziel,
            "Ernteziel gewechselt — fremde Flugkopie verworfen"
        );
        // Der festgehaltene Status beschreibt DIESE Auskunft — er faellt
        // mit ihr, sonst traegt der naechste Platz den Status des vorigen
        // (Runde 6, Befund 5). Nur wenn wirklich eine da war: Nach einem
        // Neustart ist sie ohnehin weg, und dann ist der feste Status
        // genau das, was N26 retten soll.
        if stats.szenerie_auskunft.is_some() {
            stats.szenerie_status_fest = None;
        }
        stats.szenerie_auskunft = None;
        stats.szenerie_auskunft_stand = 0;
    }

    // ⚠ Generationswechsel: Die Kopie der alten Verbindung gilt nicht
    // mehr — auch wenn KEINE neue Lieferung kommt.
    if flugkopie_entwerten(stats.szenerie_auskunft_generation, schnapp.generation) {
        if stats.szenerie_auskunft.is_some() {
            tracing::info!(
                alt = stats.szenerie_auskunft_generation,
                neu = schnapp.generation,
                "Szenerie-Generation gewechselt — Flugkopie entwertet"
            );
        }
        if stats.szenerie_auskunft.is_some() {
            stats.szenerie_status_fest = None;
        }
        stats.szenerie_auskunft = None;
        stats.szenerie_auskunft_stand = 0;
        stats.szenerie_auskunft_generation = schnapp.generation;
        // Die Zweitquelle faellt MIT. Sie haelt die Auskunft des
        // geplanten Ziels ueber einen Zielwechsel hinweg — aber nicht
        // ueber einen VERBINDUNGSwechsel: Ein Sim-Neustart (MSFS
        // 2020↔2024, andere Szenerie) kann fuer denselben Platz andere
        // Rollwege mitbringen, und die ICAO-Pruefung faengt das nicht,
        // weil der Platz ja derselbe ist. Genau diese Luecke hat das
        // Projekt fuer `dep_szenerie_auskunft` am 04.09.2026 schon
        // einmal schliessen muessen; hier waere sie mit dem neuen Feld
        // zurueckgekommen (externe Abnahme, 21.09.2026).
        stats.ziel_szenerie_auskunft = None;
        stats.ziel_szenerie_auskunft_stand = 0;
    }

    let Some((neu, stand)) = schnapp.auskunft else {
        return;
    };
    // ⚠ Nach STAND entscheiden, nicht nach Anwesenheit — siehe
    // `auskunft_ersetzen`.
    let alt = stats
        .szenerie_auskunft
        .as_ref()
        .map(|a| (a.icao.as_str(), stats.szenerie_auskunft_stand));
    if auskunft_ersetzen(alt, &neu.icao, stand) {
        tracing::info!(
            icao = %neu.icao,
            stand,
            bahnen = neu.bahnen.len(),
            rollwege = neu.rollwege.len(),
            "Szenerie-Auskunft am Flug abgelegt"
        );
        // Gehoert die Auskunft zum GEPLANTEN Ziel, zusaetzlich dort
        // ablegen — dieser Platz ueberlebt einen Zielwechsel.
        if neu.icao.eq_ignore_ascii_case(geplantes_ziel) {
            let alt_ziel = stats
                .ziel_szenerie_auskunft
                .as_ref()
                .map(|a| (a.icao.as_str(), stats.ziel_szenerie_auskunft_stand));
            if auskunft_ersetzen(alt_ziel, &neu.icao, stand) {
                stats.ziel_szenerie_auskunft = Some(Arc::clone(&neu));
                stats.ziel_szenerie_auskunft_stand = stand;
            }
        }
        // Geteilt mit dem Buch (siehe `Schnappschuss::auskunft`), nicht
        // kopiert: Bis 2.0.16 stand hier eine Vollkopie aller Rollwege
        // unter der Flugdaten-Sperre (v2.0.17, AIB 431).
        stats.szenerie_auskunft = Some(neu);
        stats.szenerie_auskunft_stand = stand;
    }
}

/// Ob die Bahnaufloesung nachgeholt werden muss.
///
/// ⚠ Der Befund vom 01.09.2026 (YSBK, erster MSFS-Flug unter v1.7.14):
/// Die Abfrage lieferte endlich den RICHTIGEN Platz — YSBK, sechs Bahnen,
/// 444 Rollwege — aber die Antwort traf **nach** dem Aufsetzen ein. Der
/// Vergleich lief nur im Aufsetz-Tick, an zwei Stellen, und wurde nie
/// nachgeholt. Am Flug stand deshalb
/// `auskunft_ohne_vergleich(platz=YSBK, bahnen=6, diagnose=geliefert(...))`
/// — die Diagnose sagte „geliefert", der Vergleich fehlte trotzdem.
///
/// Bei 444 Rollwegen kommt die Lieferung stueckweise; ein kurzer Anflug
/// auf einen kleinen Platz reicht dafuer nicht.
///
/// Die drei Bedingungen:
///
/// * **Aufsetzpunkt vorhanden** — ohne ihn gibt es nichts zuzuordnen, und
///   ein Nachholen wuerde mit der Rollposition rechnen. (Ein Durchstarten
///   setzt `landing_lat`/`landing_lon` zurueck, damit greift es dort
///   richtig NICHT.)
/// * **Auskunft vorhanden** — sonst aendert ein zweiter Lauf nichts.
/// * **die Auskunft ist NEUER als die zuletzt zugeordnete** — nicht
///   „es gibt noch keinen Bericht". Der erste Entwurf sperrte auf
///   `szenerie_uebernahme.is_some()`; damit blieb ein Bericht aus
///   Lieferung A stehen, obwohl die Ernte laengst Lieferung B
///   angenommen hatte (QS-Befund 2, erste Runde v1.7.15).
///
/// Damit laeuft es genau einmal je LIEFERUNG statt genau einmal je Flug.
fn bahnaufloesung_nachholen(
    hat_auskunft: bool,
    auskunft_stand: u32,
    korrelierter_stand: u32,
) -> bool {
    // ⚠ Kein `hat_aufsetzpunkt` mehr: Die einzige Aufrufstelle prueft ihn
    // selbst und uebergab hart `true` — die Testfaelle mit `false`
    // deckten einen Zweig, den die Produktion nie erreichen konnte
    // (externe QS, 02.09.2026, N11).
    hat_auskunft && auskunft_stand > korrelierter_stand
}

/// Rang einer blossen Vormerkung.
///
/// ⚠ Absichtlich SCHLECHTER als jeder Rang, den die Ernte vergibt.
///
/// Diese Funktion meldet beim Flugbeginn Start, Ziel und Ausweichplatz
/// gemeinsam an — sie kennt die Rollen nicht, sie hat nur eine Liste.
/// Traegt sie alle mit Rang 0 ein, ist die Rangfolge tot: `wunsch_mit_rang`
/// nimmt das Minimum, ein Rang kann also nie schlechter werden, und beim
/// Ausweichflug behielte das geplante Ziel seine 0. Bei gleicher
/// Versuchszahl entschiede dann wieder das Alphabet (QS-Befund 3,
/// zweite Runde).
///
/// Mit einem schlechten Vormerkungsrang gewinnt immer die Ernte, die
/// jeden Durchlauf die richtige Rangfolge nachtraegt.
// Nur im `cfg(target_os = "windows")`-Block verwendet
// (`szenerie_anfordern_mit_rang`). Kein Test braucht sie, deshalb faellt
// sie auf Nicht-Windows als ungenutzt auf, obwohl sie produktiv wirkt.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const RANG_VORMERKUNG: u8 = 200;

// Nur auf Windows produktiv aufgerufen (Szenerie-Ernte im
// `cfg(target_os = "windows")`-Block). Die Funktion steht hier
// ABSICHTLICH ausserhalb davon, damit die Tests sie auf JEDER
// Plattform fahren koennen — siehe Doc-Kommentar. Auf Nicht-Windows
// fehlt dadurch der einzige Nicht-Test-Aufrufer, und `cargo check`
// (nur lib-Target, ohne Tests) meldet sie als ungenutzt. Auf Windows
// bleibt die Warnung scharf und wuerde echten toten Code zeigen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn szenerie_ziele(
    geplant: &str,
    ausweich: Option<&str>,
    tatsaechlich: Option<&str>,
) -> Vec<String> {
    let mut ziele: Vec<String> = Vec::new();
    for kandidat in tatsaechlich
        .into_iter()
        .chain(ausweich)
        .chain(std::iter::once(geplant))
    {
        let k = kandidat.trim().to_ascii_uppercase();
        if k.len() == 4 && !ziele.contains(&k) {
            ziele.push(k);
        }
    }
    ziele
}

/// Von welchem Platz die Auskunft GEERNTET werden darf.
///
/// ⚠ Genau einer, ohne Rueckfall. Rangfolge:
///
/// ```text
/// runway_correlation_icao  →  divert_prefetch_icao  →  geplantes Ziel
/// ```
///
/// Der erste ist der Platz, ueber dem die Raeder wirklich aufgesetzt
/// haben. Er kommt spaet — aber er kommt auch dann, wenn die
/// Anflugerkennung den Ausweichflug VERPASST hat. Fehlte er hier, bliebe
/// das Ernteziel in genau diesem Fall das geplante: Die nachgeforderte
/// Antwort des tatsaechlichen Platzes laege im Buch, erreichte aber
/// weder den Flug noch die abschliessende Neuberechnung (QS-Befund 2,
/// zweite Runde).
///
/// `szenerie_ziele` liefert beide, weil beide ANGEFORDERT werden sollen —
/// ein Ausweichentschluss faellt spaet und die Antwort braucht Vorlauf.
/// Beim ERNTEN waere dieselbe Liste ein Fehler: Ein `find_map` ueber sie
/// nimmt das geplante Ziel, sobald dessen Antwort zuerst da ist, und legt
/// sie am Flug ab. Die Bahnuebernahme weist sie spaeter am ICAO-Vergleich
/// ab — die **Rollwege** nicht, die gehen ohne Platzpruefung in die
/// Ausfahrten. Dann liegen die Ausfahrten des geplanten Platzes an der
/// tatsaechlichen Ausweichlandung (QS-Befund 2, 01.09.2026).
// Nur auf Windows produktiv aufgerufen (Szenerie-Ernte im
// `cfg(target_os = "windows")`-Block). Die Funktion steht hier
// ABSICHTLICH ausserhalb davon, damit die Tests sie auf JEDER
// Plattform fahren koennen — siehe Doc-Kommentar. Auf Nicht-Windows
// fehlt dadurch der einzige Nicht-Test-Aufrufer, und `cargo check`
// (nur lib-Target, ohne Tests) meldet sie als ungenutzt. Auf Windows
// bleibt die Warnung scharf und wuerde echten toten Code zeigen.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
fn szenerie_ernteziel(
    geplant: &str,
    ausweich: Option<&str>,
    tatsaechlich: Option<&str>,
) -> Option<String> {
    let sauber = |s: &str| {
        let k = s.trim().to_ascii_uppercase();
        (k.len() == 4).then_some(k)
    };
    tatsaechlich
        .and_then(sauber)
        .or_else(|| ausweich.and_then(sauber))
        .or_else(|| sauber(geplant))
}

/// Die MSFS-Szenerie-Auskunft vom Adapter an den Flug reichen.
///
/// Der Adapter fuellt sie asynchron, sobald die Lieferung vollstaendig
/// ist. Hier wird sie einmal uebernommen und liegt dann beim Aufsetzen
/// bereit.
///
/// ⚠ Nur uebernehmen, wenn noch nichts da ist ODER ein anderer Platz
/// geliefert wurde. Sonst wuerde eine spaetere, leere Auskunft eine
/// gute ueberschreiben.
fn szenerie_auskunft_uebernehmen(
    app: &AppHandle,
    flight: &Arc<ActiveFlight>,
    simulator: sim_core::Simulator,
) {
    // ⚠ NUR wenn wirklich MSFS fliegt.
    //
    // Der Block hier haengt an `cfg(target_os = "windows")`, und darunter
    // faellt auch ein X-Plane-Pilot auf einem Windows-Rechner. Ohne diese
    // Abfrage bekaeme dessen Flug die Diagnose des UNTAETIGEN
    // MSFS-Adapters angeheftet — typisch "keine_antwort". Das waere nicht
    // nur falsch, es wuerde ausgerechnet die Statistik verfaelschen, mit
    // der das MSFS-Problem eingekreist wird: X-Plane-Fluege zaehlten als
    // MSFS-Fehlschlaege.
    if szenerie_bahn::quelle_fuer(simulator) != szenerie_bahn::Quelle::MsfsFacility {
        return;
    }
    #[cfg(target_os = "windows")]
    {
        // Wohin es geht — das Ausweichziel zuerst, falls eins erkannt ist.
        let (ausweich, tatsaechlich) = flight
            .stats
            .lock()
            .ok()
            .map(|s| {
                (
                    s.divert_prefetch_icao.clone(),
                    s.runway_correlation_icao.clone(),
                )
            })
            .unwrap_or((None, None));
        let ziele = szenerie_ziele(
            &flight.arr_airport,
            ausweich.as_deref(),
            tatsaechlich.as_deref(),
        );

        // ⚠ Beim Eintritt in den Anflug den Versuchsvorrat neu oeffnen.
        // Ohne das ist er am Gate verbraucht — siehe
        // `braucht_neues_versuchsfenster`.
        let fenster_faellig = {
            let mut stats = match flight.stats.lock() {
                Ok(s) => s,
                Err(_) => return,
            };
            // ⚠ Entscheidung und Fortschreibung kommen aus EINER
            // Funktion — siehe `versuchsfenster_fortschreiben`. Hier
            // steht nur noch die Zuweisung; alles andere ist geprueft.
            let (faellig, neue_phase) =
                versuchsfenster_fortschreiben(stats.phase, stats.szenerie_vorige_phase);
            stats.szenerie_vorige_phase = neue_phase;
            faellig
        };

        // ⚠ VOR dem Block. `ernteziel` wird auch bei der Uebernahme
        // gebraucht; stand es im Block, war es dort nicht mehr sichtbar
        // — und das sieht auf dem Mac niemand, weil der ganze Abschnitt
        // hinter `cfg(target_os = "windows")` steht. Vierter
        // Windows-Uebersetzungsfehler dieser Fassung.
        let ernteziel = szenerie_ernteziel(
            &flight.arr_airport,
            ausweich.as_deref(),
            tatsaechlich.as_deref(),
        );

        let schnapp_aussen = {
            let state = app.state::<AppState>();
            let Ok(msfs) = state.msfs.lock() else { return };
            // ⚠ Bei JEDEM Durchlauf nachmelden. Das Buch entscheidet,
            // ob daraus wirklich eine Anfrage wird — genau so wird aus
            // dem einen Versuch am Gate eine Wiederholung im Anflug,
            // wo die Szenerie des Ziels geladen ist. Bis v1.7.13 gab es
            // den zweiten Versuch nicht, und der Flug EDDF-LEZL am
            // 01.09.2026 landete mit der Szenerie Frankfurts.
            // ⚠ EINE Buchoperation fuer Fenster, Zielmenge und Raenge.
            //
            // Vorher waren es drei mit drei getrennten Sperren, und
            // dazwischen konnte der Verbindungsfaden einen veralteten
            // Rang-0-Platz auswaehlen — der dann bis zu 60 Sekunden lief,
            // weil eine spaetere Rangkorrektur einen LAUFENDEN Auftrag
            // nicht beendet (QS-Befund 2, fuenfte Runde).
            let raenge: Vec<(String, u8)> = ziele
                .iter()
                .enumerate()
                .map(|(rang, z)| (z.clone(), rang as u8))
                .collect();
            // ⚠ v1.7.17: den Abflugplatz VOR dieser Aufraeumung schuetzen.
            // Ohne den Schutz setzte diese Stelle — sie laeuft ab dem
            // ERSTEN Tick des Fluges, nicht erst ab dem Sinkflug — seinen
            // Rang bei JEDEM Durchlauf auf `RANG_UNBETEILIGT` zurueck
            // (`raenge_setzen` kennt nur die Anflug-Ziele) und riss einen
            // laufenden Abflug-Auftrag sofort wieder aus `Laeuft`, weil er
            // kein Anflug-Ziel ist — beides noch bevor die Abflug-Ernte
            // (`dep_szenerie_auskunft_uebernehmen`) im selben Tick ueberhaupt
            // zum Zug kam (externe Gegenpruefung, Codex, adversarial,
            // 04.09.2026).
            let geschuetzt = vec![flight.dpt_airport.trim().to_ascii_uppercase()];
            let betroffen =
                msfs.szenerie_anflug_ausrichten_mit_schutz(&raenge, &geschuetzt, fenster_faellig);
            if fenster_faellig {
                tracing::info!(
                    betroffen,
                    "Landephase gewechselt — neuer Versuchsvorrat fuer die Szenerie"
                );
            }
            // ⚠ Geerntet wird GENAU EIN Platz, ohne Rueckfall — siehe
            // `szenerie_ernteziel`. Ein `find_map` ueber die Zielliste
            // nahm das geplante Ziel, sobald dessen Antwort zuerst da
            // war, und dessen Rollwege landeten ungeprueft in den
            // Ausfahrten der Ausweichlandung.
            //
            // ⚠ Generation, Auskunft, Stand, Diagnose und Kennung aus
            // EINEM Zugriff — siehe `Schnappschuss`.
            //
            // Getrennt gelesen konnte dazwischen ein Verbindungswechsel
            // liegen: Die Flugkopie wurde auf die neue Generation
            // entwertet und danach mit der ALTEN Auskunft neu gefuellt,
            // die nun die neue Generation trug. Der naechste Durchlauf
            // sah keinen Wechsel mehr, und genau die Auskunft, welche
            // die Generation entwerten sollte, war dauerhaft zurueck
            // (QS-Befund 1, neunte Runde).
            ernteziel
                .as_deref()
                .map(|z| msfs.szenerie_schnappschuss(z))
                .unwrap_or_else(|| sim_core::szenerie::Schnappschuss {
                    generation: 0,
                    auskunft: None,
                    diagnose: "kein_ziel".to_string(),
                    kennung: None,
                })
        };
        // ⚠ EINE Uebernahme, EINE Sperre, und sie steht in einer
        // pruefbaren Funktion — siehe `schnappschuss_uebernehmen`.
        if let Ok(mut stats) = flight.stats.lock() {
            schnappschuss_uebernehmen(
                &mut stats,
                ernteziel.as_deref(),
                &flight.arr_airport,
                schnapp_aussen,
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (app, flight);
    }
}

/// Die Szenerie-Auskunft für den ABFLUGplatz übernehmen — v1.7.17.
///
/// # Warum das eigenständig neben `szenerie_auskunft_uebernehmen` steht
///
/// Der Abflugplatz ändert sich nie über einen Flug hinweg (anders als
/// das Ziel, das durch einen Divert wechseln kann) — er braucht deshalb
/// keine RANGFOLGE zwischen mehreren Kandidaten, keine STAND-Zählung
/// („wessen Lieferung ist neuer"), kein Versuchsfenster, das bei einem
/// Phasenwechsel neu geöffnet wird. Eine GENERATIONS-Prüfung braucht sie
/// trotzdem — siehe `dep_szenerie_auskunft_generation` für den Grund
/// (Verbindungswechsel, nicht Zielwechsel) und die Doku an
/// `dep_szenerie_auskunft` dafür, warum das ein eigenes Feld statt
/// desselben Slots wie `szenerie_auskunft` ist.
///
/// ⚠ Runde 1 (externe Gegenprüfung, Codex, adversarial, 04.09.2026): Der
/// Aufruf hier ist NICHT die einzige Stelle, die dieses Buch berührt —
/// die ZIEL-Ernte (`szenerie_auskunft_uebernehmen`) räumt das Buch bei
/// JEDEM Tick auf und muss den Abflugplatz dabei aktiv schützen (siehe
/// dort, `szenerie_anflug_ausrichten_mit_schutz`), sonst setzt sie
/// dessen Rang UND einen laufenden Auftrag zurück, bevor diese Funktion
/// überhaupt zum Zug kommt.
///
/// # MSFS braucht hier KEINE neue Registrierung
///
/// `spawn_navdata_fetch` (beim Flugbeginn) meldet über
/// `szenerie_anfordern_fuer` bereits ALLE bekannten Plätze an — Abflug,
/// Ziel, Alternate — mit `RANG_VORMERKUNG` (niedrigste Prioritaet, „darf
/// mitlaufen, nimmt niemandem den Vortritt"). Der Abflugplatz steht also
/// schon im Auftragsbuch, sobald der Flug beginnt; hier wird sein Rang
/// nur noch verbessert (er soll nicht bis zum Sankt-Nimmerleins-Tag hinter
/// jedem neuen Zielkandidaten zurückstehen) und die Lieferung abgeholt.
///
/// Ob ein bereits abgeleiteter `dep_gate` (samt Post-Riegel) geloescht
/// werden muss, wenn die Abflug-Szenerie durch einen Verbindungswechsel
/// entwertet wurde (Runde 2, Codex adversarial, 04.09.2026). Nur waehrend
/// des Boardings — siehe die ausfuehrliche Begruendung an der Aufrufstelle
/// im Nachzieh-Tick.
fn dep_gate_bei_szenerie_wechsel_zuruecksetzen(phase: FlightPhase) -> bool {
    phase == FlightPhase::Boarding
}

/// Ob der Nachzieh-Tick versuchen soll, `dep_gate` (neu) abzuleiten:
/// entweder fehlt er noch, oder es steht ein Generationswechsel waehrend
/// des Boardings aus.
///
/// ⚠ v1.7.17 Runde 3: als EIGENE, pure Funktion — nicht nur die
/// Boarding-Teilbedingung wie in Runde 2 — damit die Zusicherung, die
/// eine Gegenprobe treffen muss, den GANZEN Riegel abdeckt, nicht nur ein
/// Drittel davon.
///
/// ⚠ v1.7.17 Runde 4 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): Runde 3 loeschte `dep_gate` HIER schon, sobald diese
/// Funktion (damals `dep_gate_muss_verworfen_werden`) wahr war — noch
/// BEVOR ein Ersatzkandidat feststand. War in genau diesem Tick weder aus
/// der neuen Szenerie noch aus OSM ein Kandidat zu gewinnen (Positions-
/// oder Datenlage noch nicht bereit), blieb `dep_gate` `None` — und wenn
/// der Flieger VOR dem naechsten erfolgreichen Boarding-Tick zurueckrollt
/// (Pushback), war der Stand ERSATZLOS verloren, obwohl der ALTE Wert
/// (aus der vorigen Generation) oft noch richtig gewesen waere. Diese
/// Funktion loescht deshalb nichts mehr — sie sagt nur noch, ob ein
/// VERSUCH lohnt. Die Aufrufstelle ersetzt `dep_gate` erst ATOMAR
/// zusammen mit einem echten Kandidaten; ohne Kandidaten bleibt der alte
/// Wert (moeglicherweise leicht veraltet, aber nie leer) stehen, bis ein
/// spaeterer Tick einen findet oder das Boarding endet.
fn dep_gate_braucht_neuableitung(
    phase: FlightPhase,
    dep_gate_vorhanden: bool,
    dep_gate_generation: u32,
    aktuelle_generation: u32,
) -> bool {
    !dep_gate_vorhanden
        || (dep_gate_bei_szenerie_wechsel_zuruecksetzen(phase)
            && dep_gate_generation != aktuelle_generation)
}

/// Ob ein noch aus einer VERALTETEN Generation stammender `dep_gate` beim
/// VERLASSEN des Boardings endgueltig verworfen werden muss, statt
/// unkorrigiert in Pushback und jede Ausgabe (Live-Post, PIREP-Felder,
/// Debrief) mitgenommen zu werden.
///
/// ⚠ v1.7.17 Runde 5 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): Runde 4s atomarer Ersatz liess einen unkorrigierten,
/// bekannt veralteten Wert stehen, wenn waehrend des GESAMTEN restlichen
/// Boardings nie ein Kandidat aus der neuen Generation gefunden wurde
/// (z. B. Szenerie- oder Positionsdaten kamen einfach nicht rechtzeitig).
/// Alle nachgelagerten Verbraucher lesen `dep_gate` OHNE die Generation zu
/// pruefen — ein bekannt falscher Wert waere als aktuell durchgegangen,
/// gerade nach einem 2020↔2024-Wechsel mit anderem Standlayout. Das ist
/// KEINE neue Korrektur waehrend/nach Pushback (die bleibt durch die
/// bestehende Boarding-Bedingung verboten) — nur das ehrliche Eingestehen
/// "kein verlaesslicher Stand bekannt", ein letztes Mal geprueft in der
/// Sekunde, in der das Boarding endet.
///
/// ⚠ v1.7.17 Runde 6 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): Runde 5s Parameter (`boarding_phase: bool`, aus der
/// AKTUELLEN Phase allein) erkannte keinen UEBERGANG, sondern jeden Tick,
/// in dem die Phase zufaellig nicht Boarding ist — also auch Stunden
/// spaeter waehrend Cruise oder Approach, wenn ein voellig unrelated
/// MSFS-Reconnect die Generation weiterzaehlt. Der Abflug-Stand ist zu
/// diesem Zeitpunkt laengst historische Tatsache (der Flieger hat das Gate
/// verlassen) und darf NIE wieder geloescht werden. Der Parameter heisst
/// deshalb jetzt `boarding_wird_verlassen` und muss vom Aufrufer aus ZWEI
/// Phasen gebildet werden (VOR und NACH `step_flight`) — wahr nur in der
/// EINEN Tick-Ausfuehrung, in der Boarding tatsaechlich endet.
fn dep_gate_muss_beim_boarding_ende_verworfen_werden(
    boarding_wird_verlassen: bool,
    dep_gate_vorhanden: bool,
    dep_gate_generation: u32,
    aktuelle_generation: u32,
) -> bool {
    boarding_wird_verlassen && dep_gate_vorhanden && dep_gate_generation != aktuelle_generation
}

/// Ob DIESER Tick der EXAKTE Uebergang aus dem Boarding ist — nicht nur
/// "die aktuelle Phase ist gerade nicht Boarding".
///
/// ⚠ v1.7.17 Runde 6 (externe Gegenpruefung, Codex, adversarial,
/// 04.09.2026): als EIGENE Funktion ausgelagert, weil genau diese
/// Unterscheidung der Kern des Runde-6-Befundes war — die Aufrufstelle
/// bildete den Uebergang vorher inline und ungetestet aus der aktuellen
/// Phase allein.
fn boarding_wird_in_diesem_tick_verlassen(
    phase_vor_step_flight: FlightPhase,
    tick_phase: FlightPhase,
) -> bool {
    phase_vor_step_flight == FlightPhase::Boarding && tick_phase != FlightPhase::Boarding
}

fn dep_szenerie_auskunft_uebernehmen(
    app: &AppHandle,
    flight: &Arc<ActiveFlight>,
    simulator: sim_core::Simulator,
) {
    let icao = flight.dpt_airport.trim().to_ascii_uppercase();
    if icao.len() != 4 {
        return;
    }
    match szenerie_bahn::quelle_fuer(simulator) {
        szenerie_bahn::Quelle::XPlaneDatei => {
            // ⚠ X-Plane hat kein Verbindungs-/Generationskonzept wie das
            // MSFS-Auftragsbuch — der Dateilesezugriff ist immer aktuell,
            // "schon da" reicht als einzige Bedingung.
            let schon_da = flight
                .stats
                .lock()
                .map(|s| s.dep_szenerie_auskunft.is_some())
                .unwrap_or(true);
            if schon_da {
                return;
            }
            // ⚠ Synchron und billig (Index-Nachschlag, siehe
            // `szenerie_bahn::szenerie_flughafen`) — kein Anfrage-
            // Antwort-Zyklus wie bei MSFS nötig.
            if let Some(sz) = szenerie_bahn::szenerie_flughafen(&icao) {
                if let Ok(mut stats) = flight.stats.lock() {
                    if stats.dep_szenerie_auskunft.is_none() {
                        stats.dep_szenerie_auskunft = Some(sz);
                    }
                }
            }
        }
        szenerie_bahn::Quelle::MsfsFacility => {
            #[cfg(target_os = "windows")]
            {
                let state = app.state::<AppState>();
                let Ok(msfs) = state.msfs.lock() else { return };
                // Rang verbessern (siehe Funktionsdoku) — bereits
                // vorregistriert, dieser Aufruf hebt nur die Prioritaet.
                msfs.szenerie_anfordern_mit_rang(&icao, 0);
                let schnapp = msfs.szenerie_schnappschuss(&icao);
                drop(msfs);
                let Ok(mut stats) = flight.stats.lock() else {
                    return;
                };
                // ⚠ GENERATIONS-Pruefung VOR dem "schon da"-Kurzschluss —
                // siehe `dep_szenerie_auskunft_generation`. Eine neue
                // Verbindung (Neustart, 2020↔2024-Wechsel) entwertet eine
                // laengst abgelegte Abflug-Kopie, auch wenn der Abflugplatz
                // selbst derselbe bleibt.
                if flugkopie_entwerten(stats.dep_szenerie_auskunft_generation, schnapp.generation) {
                    stats.dep_szenerie_auskunft = None;
                    stats.dep_szenerie_auskunft_generation = schnapp.generation;
                    // ⚠ Runde 2→3 (externe Gegenpruefung, Codex, adversarial,
                    // 04.09.2026): Ein bereits aus der ALTEN Generation
                    // abgeleiteter `dep_gate` wird HIER NICHT mehr geloescht.
                    // Runde 2 tat das noch an dieser Stelle, gegen
                    // `stats.phase` — aber diese Funktion laeuft VOR
                    // `step_flight` im selben Tick, liest also die Phase des
                    // VORHERIGEN Ticks. Genau im Tick des Boarding→Pushback-
                    // Uebergangs haette sie den Riegel geloescht, kurz bevor
                    // keine Neuableitung mehr moeglich war (der Nachzieh-Tick
                    // prueft die FRISCHE, nach-`step_flight`-Phase und haette
                    // dann schon Pushback gesehen). Die Entscheidung, ob
                    // `dep_gate` verworfen wird, liegt jetzt einzig beim
                    // Nachzieh-Tick — der vergleicht `dep_gate_generation`
                    // gegen `dep_szenerie_auskunft_generation` mit der
                    // frischen Phase. Hier wird nur noch die Generation
                    // selbst fortgeschrieben (oben).
                }
                if stats.dep_szenerie_auskunft.is_some() {
                    return;
                }
                let Some((auskunft, _stand)) = schnapp.auskunft else {
                    return;
                };
                // ⚠ Dieselbe ICAO-Pruefung wie bei der Zielauskunft — eine
                // Lieferung traegt IMMER den Platz, der sie beschreibt
                // (Buch-Zusicherung), aber die Pruefung hier kostet nichts
                // und macht die Annahme explizit statt implizit.
                if !auskunft.icao.eq_ignore_ascii_case(&icao) {
                    return;
                }
                stats.dep_szenerie_auskunft = Some(auskunft);
            }
            #[cfg(not(target_os = "windows"))]
            {
                let _ = app;
            }
        }
        szenerie_bahn::Quelle::Keine => {}
    }
}

fn spawn_navdata_fetch(app: &AppHandle, flight: &Arc<ActiveFlight>, icaos: Vec<String>) {
    // v1.7.8 — die Szenerie-Auskunft am selben Auslöser anfordern.
    //
    // Hier, weil diese Funktion für ALLE Fälle läuft, in denen ein
    // Zielflughafen bekannt wird: Flugbeginn, Anflug-Vorabruf,
    // Divert, Ausweichflughafen. Ein eigener Auslöser wäre eine
    // zweite Stelle, die man vergessen kann.
    //
    // Vor dem Filter gegen den Zwischenspeicher: Der gilt für die
    // Navdaten, nicht für die Szenerie. Ein Platz, dessen Navdaten
    // schon liegen, braucht die Szenerie unter Umständen trotzdem
    // noch — sonst bekäme ausgerechnet der geplante Zielflughafen
    // keine.
    szenerie_anfordern_fuer(app, &icaos);

    // Normalize + filter against the per-flight cache. Only ICAOs that
    // are missing trigger an actual HTTP roundtrip.
    let missing: Vec<String> = {
        let cache = flight.navdata.lock().expect("navdata lock");
        let mut want: Vec<String> = icaos
            .into_iter()
            .map(|s| s.trim().to_uppercase())
            .filter(|s| !s.is_empty() && s.len() == 4)
            .filter(|s| !cache.airports.contains_key(s))
            .collect();
        want.sort();
        want.dedup();
        want
    };
    if missing.is_empty() {
        tracing::debug!(
            pirep_id = %flight.pirep_id,
            "navdata: all requested ICAOs already cached, skipping fetch"
        );
        return;
    }
    let icaos = missing;
    let token = secrets::load_api_key(MQTT_KEYRING_PASSWORD).ok().flatten();
    let app_clone = app.clone();
    let flight_arc = Arc::clone(flight);
    tokio::spawn(async move {
        fetch_navdata_for_flight(app_clone, flight_arc, icaos, None, token).await;
    });
}

/// v0.16.24: on-demand navdata fetch for the ACTUAL landing airport when
/// the touchdown runway correlation fell back to OurAirports (= the real
/// field wasn't in the per-flight navdata cache at touchdown — typically a
/// divert the approach pre-fetch didn't catch). Fires `spawn_navdata_fetch`
/// for that airport so the cache is (hopefully) warm by the time
/// `build_landing_record` re-correlates at PIREP-filing time
/// (finalize-with-best). Cheap + safe to call every tick: it only acts
/// while `runway_fallback_at_touchdown` is set, and `spawn_navdata_fetch`'s
/// per-ICAO guard makes a duplicate a no-op. Once the fetch lands the field
/// is cached, so even a re-run does nothing.
fn maybe_fetch_actual_landing_navdata(app: &AppHandle, flight: &Arc<ActiveFlight>) {
    let icao = {
        let mut stats = flight.stats.lock().expect("flight stats");
        if !stats.runway_fallback_at_touchdown {
            return;
        }
        let Some(icao) = stats.runway_correlation_icao.clone() else {
            return;
        };
        // One-shot per ICAO: a divert airport not in the active AIRAC cycle
        // returns 404 and never populates the cache, so without this guard
        // we'd re-issue the GET every tick until filing. Fire exactly once
        // per actual-landing ICAO. (Re-targets cleanly if the correlation
        // later resolves a different field — e.g. a touch-and-go elsewhere.)
        if stats.divert_ondemand_fetch_attempted.as_deref() == Some(icao.as_str()) {
            return;
        }
        stats.divert_ondemand_fetch_attempted = Some(icao.clone());
        icao
    };
    // Don't re-issue for the planned airport — that's already fetched at
    // flight start; a fallback there means a genuine VPS/cycle gap that a
    // retry won't fix.
    if icao != flight.arr_airport.trim().to_uppercase() {
        spawn_navdata_fetch(app, flight, vec![icao]);
    }
}
