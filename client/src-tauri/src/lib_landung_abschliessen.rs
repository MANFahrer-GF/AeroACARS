/// v0.16.6: per-episode reset of the approach-stability stats + rollout
/// fields — called from the sampler's climb-out reset (touch-and-go) right
/// next to the `landing_lat`/`landing_lon` re-arm. The FINAL touchdown of a
/// bush flight must evaluate stability over ITS OWN window and accumulate
/// ITS OWN rollout; without this the PIREP would silently carry TD#1's
/// numbers (the FSM Landing arm never runs on bush flights to overwrite
/// them). On FSM-tracked flights the Landing-entry recompute overwrites all
/// of these at the next touchdown anyway — clearing them here additionally
/// only means the fields read `None`/default between the T&G and the final
/// touchdown instead of holding stale TD#1 values.
fn clear_approach_stability_and_rollout(stats: &mut FlightStats) {
    stats.approach_vs_stddev_fpm = None;
    stats.approach_bank_stddev_deg = None;
    stats.approach_vs_deviation_fpm = None;
    stats.approach_max_vs_deviation_below_500_fpm = None;
    stats.approach_bank_stddev_filtered_deg = None;
    stats.approach_vs_stddev_filtered_fpm = None;
    stats.approach_runway_changed_late = false;
    stats.approach_stable_at_gate = None;
    stats.approach_vs_jerk_fpm = None;
    stats.approach_ias_stddev_kt = None;
    stats.approach_excessive_sink = None;
    stats.approach_stable_config = None;
    stats.approach_ias_stddev_unter_500_kt = None;
    stats.approach_stable_config_500 = None;
    stats.approach_used_hat = false;
    stats.approach_window_sample_count = None;
    stats.approach_stable_at_da = None;
    stats.approach_spaetes_final = None;
    stats.approach_stall_warning_count = 0;
    stats.approach_wind = None;
    stats.anflug_forensik = Default::default();
    stats.anflug_forensik_td = None;
    stats.anflug_forensik_fenster = None;
    stats.anflug_forensik_nachtrag_offen = false;
    stats.anflug_forensik_ab_gestempelt = None;
    // Der Forensik-Puffer selbst bleibt; der Schnitt trennt den
    // abgebrochenen Anflug vom naechsten (Befund B, siehe dort).
    anflug_forensik_schneiden(stats, None);
    stats.rollout_distance_m = None;
    // ── Bahndisziplin: ALLES, nicht nur die Zahlen ───────────────────
    //
    // Diese Funktion läuft beim erneuten Aufsetzen nach einem Durchstarten
    // oder Touch-and-Go. Bis hierher setzte sie die Kennzahlen zurück und
    // liess die **Spur** stehen: Das zweite Aufsetzen bekam die Punkte des
    // ersten mit, und dazu dessen Räumpunkt.
    //
    // In der Anzeige wäre das nicht als Fehler erkennbar gewesen, sondern
    // als eine Landung, bei der das Flugzeug zweimal über die Bahn lief.
    //
    // Vorher sichern: Die Spur gehört nicht in die Bewertung, wohl aber in
    // die Anzeige, als eigener, früherer Durchgang. Höchstens drei — wer
    // öfter durchstartet, bekommt die letzten drei gezeigt.
    if !stats.bahn_spur.is_empty() {
        if let Some(rm) = stats.runway_match.as_ref() {
            let bahn = format!("{} {}", rm.airport_ident, rm.runway_ident);
            stats
                .bahn_vorige_spuren
                .push((bahn, stats.bahn_spur.clone()));
            let zuviel = stats.bahn_vorige_spuren.len().saturating_sub(3);
            stats.bahn_vorige_spuren.drain(..zuviel);
        }
    }
    spur_verwerfen(stats);
    // Und die Nachernte darf nicht hinter diesen Moment zurueckgreifen.
    //
    // `spur_verwerfen` setzt `bahn_spur_bis` zurueck, und die Ernte
    // sucht dann ab `landing_at` — das ist hier noch das ERSTE Aufsetzen.
    // Der Ruecksetzer laeuft beim Steigen ueber hundert Fuss; der Puffer
    // haelt zu diesem Zeitpunkt noch die letzte Sekunde Startlauf am
    // Boden. Genau die landete in der Spur der zweiten Landung.
    //
    // Feldbefund EWG9503 (#1431, EDDL 23L, 19.09.2026): sechs Punkte bei
    // 2232–2313 m vor dem eigentlichen Aufsetzen bei 595 m. Die Anzeige
    // zog von dort eine Linie zurueck — eine zweite Spur, die es nicht
    // gab. Im Bestand acht von 360 Landungen.
    //
    // Das neue Aufsetzen liegt spaeter; `max(bis, landing_at)` in der
    // Ernte laesst ihm den Vortritt.
    stats.bahn_spur_bis = stats.snapshot_buffer.iter().map(|p| p.at).max();
    // `arr_ground_geojson` bleibt: Die Bodenkarte gehört zum Flughafen,
    // nicht zur Landung. Sie erneut zu holen wäre eine Netzanfrage im
    // ungünstigsten Moment — kurz nach einem Durchstarten.
    stats.rollout_finalized = false;
    stats.rollout_last_lat = None;
    stats.rollout_last_lon = None;
    stats.rollout_finalize_reason = None;
}

/// Steigt das Flugzeug nach einem Aufsetzen gerade wieder weg (20.10.2026,
/// „Stück 3“)? Eine offene Landung (Phasenlogik oder Aufsetz-Sampler), eine
/// Phase, in der gelandet werden kann, ein echter Steigflug ueber 100 ft —
/// und die Auswertung dieses Aufsetzers liegt vor (der Puffer-Dump, oder er
/// kommt nicht mehr). Bewusst OHNE Zeitgrenze ab dem Aufsetzen und ohne
/// Triebwerks-Riegel: genau so, wie der Sampler sein Wegsteigen schon immer
/// erkannt hat. Die Haltezeit legt der Aufrufer darueber.
fn wegsteigen_nach_aufsetzen(stats: &FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) -> bool {
    let offen = stats.landing_at.is_some() || stats.sampler_touchdown_at.is_some();
    if !offen || !is_touchdown_capturable_phase(stats.phase, stats.was_airborne) {
        return false;
    }
    if !wegsteigen_mit_fahrt(snap) {
        return false;
    }
    match (stats.sampler_touchdown_at, stats.touchdown_window_dumped_at) {
        (Some(td), None) => (now - td).num_milliseconds() >= SAMPLER_DUMP_WAIT_TIMEOUT_MS,
        _ => true,
    }
}

/// Ein echtes Wegsteigen nach dem Aufsetzen hat Fahrt. `echter_steigflug_
/// nach_aufsetzen` prueft Hoehe und Gelaende, aber nicht, ob sich das
/// Flugzeug bewegt — nach einem Neuladen meldet MSFS kurz eine Fantasie-
/// Position (Korpus-Fund J4dxXAPBz9BvZd0P: lat 0 / lon 90, 211 ft, Fahrt 0,
/// Triebwerke aus), und das war fuer die Pruefung ein Steigflug.
/// Hubschrauber steigen auch ohne Fahrt weg und sind ausgenommen.
fn wegsteigen_mit_fahrt(snap: &SimSnapshot) -> bool {
    // Wert seit v3: bundle/grenzwerte/phasen_kern.json -> min_fahrt_kt (v2.1.24: 30.0)
    if !echter_steigflug_nach_aufsetzen(snap) {
        return false;
    }
    let hubschrauber = aircraft_category::resolve_category(
        snap.aircraft_icao.as_deref(),
        snap.gear_is_skid,
        snap.gear_is_floats,
        snap.gear_is_wheels,
        snap.water_rudder_present,
    )
    .is_rotorcraft();
    hubschrauber
        || snap.groundspeed_kt >= crate::grenzwerte::grenzwerte().phasen_kern.min_fahrt_kt.wert
        || snap.indicated_airspeed_kt >= crate::grenzwerte::grenzwerte().phasen_kern.min_fahrt_kt.wert
}

/// Haltezeit fuer das Wegsteigen nach einem Aufsetzen: `true`, sobald
/// `steigt` seit `TOUCH_AND_GO_DWELL_SECS` ununterbrochen gilt. Faellt die
/// Bedingung weg, beginnt die Uhr neu. Fuer den Sampler-Faden, der sonst auf
/// einen einzelnen Messwert hin abschloesse.
fn steigflug_gehalten(seit: &mut Option<DateTime<Utc>>, steigt: bool, now: DateTime<Utc>) -> bool {
    if !steigt {
        *seit = None;
        return false;
    }
    let beginn = *seit.get_or_insert(now);
    (now - beginn).num_milliseconds() >= crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_dwell_secs.wert * 1_000
}

/// v3 (P-4.13): Eingabe-Dokument eines Touch-and-Go einfrieren, bevor
/// `landung_beim_wegsteigen_abschliessen` die Landungsfelder zuruecksetzt.
/// Ohne Fenster nichts (der Server verlangt mindestens eine Probe), und
/// nichts, wenn der Abschluss nicht stattfindet (`offene_landung_beim_
/// wegsteigen`): Sonst ginge ein `touch_and_go` ohne Entscheidung hinaus, und
/// die echte Landung verloere ihr Fenster.
fn aufsetzer_einfrieren(flight: &ActiveFlight, stats: &mut FlightStats, snap: &SimSnapshot) {
    if stats.landung_fenster.is_empty() || offene_landung_beim_wegsteigen(stats).is_none() {
        return;
    }
    let muster = stats.aufgeloestes_muster.clone().or_else(|| {
        sim_core::muster_aufloesen(snap.aircraft_icao.as_deref(), &flight.aircraft_icao, snap.aircraft_title.as_deref())
    });
    let eingabe = landungs_bewertungseingabe(flight, stats, muster.as_deref(), &flight.arr_airport);
    let begleitung = landungs_begleitung(flight, stats, &eingabe, muster.as_deref(), &flight.arr_airport);
    let fenster = std::mem::take(&mut stats.landung_fenster);
    // Der Server nimmt `seq` 1..=SEQ_MAX; die letzte Nummer bleibt der
    // Landung (Codex 30.10.). Das Fenster ist trotzdem weg — sonst bekaeme
    // eine spaetere Landung ohne eigenes Fenster das dieses Aufsetzers.
    if stats.aufsetzer_eingefroren.len() + 1 >= landungs_eingabe::SEQ_MAX as usize {
        tracing::warn!(eingefroren = stats.aufsetzer_eingefroren.len(), "Touch-and-Go ueber der Dokumentgrenze — kein Eingabe-Dokument");
        return;
    }
    stats.aufsetzer_eingefroren.push((eingabe, fenster, begleitung));
}

/// Die Landung, die `landung_beim_wegsteigen_abschliessen` abschliesst: ihr
/// Aufsetzzeitpunkt — `None`, wenn keine offen ist. Eine Bedingung fuer
/// Abschliessen und Einfrieren (`aufsetzer_einfrieren`).
fn offene_landung_beim_wegsteigen(stats: &FlightStats) -> Option<DateTime<Utc>> {
    // Nach dem Abrollen ist die Landung fertig. Ein Slew, ein Neuladen oder
    // ein Versetzen in die Hoehe danach ist kein Touch-and-Go — hier und
    // nicht beim Aufrufer, weil der Sampler ohne Phasenpruefung ruft.
    if !is_touchdown_capturable_phase(stats.phase, stats.was_airborne) {
        return None;
    }
    stats.landing_at.or(stats.sampler_touchdown_at)
}

/// Schliesst die laufende Landung ab, weil das Flugzeug wieder wegsteigt
/// (Touch-and-Go, abgebrochene Landung, Durchstart nach Bodenlauf) — die
/// EINE Stelle dafuer (20.10.2026, „Stück 3“).
///
/// Bis 2.1.4 gab es zwei Haelften: Der Aufsetz-Sampler setzte beim
/// Wegsteigen seine Messfelder zurueck, die Phasenlogik ihre eigenen — aber
/// nur innerhalb von 30 s ab dem Aufsetzen. EWG 1599 (Pilot 15, 20.10.2026)
/// lief durch die Luecke: Bodenlauf 26 s, 100 ft nach 35,9 s. Der Sampler
/// schaltete sich neu scharf und erfasste den zweiten Aufsetzer, die Note,
/// die Live-Meldung und die Phase blieben beim ersten. Live zeigte −667 fpm,
/// phpVMS −55, die Gesamtnote mischte beide, und 15 Minuten stand die Phase
/// in 4000 ft auf `Landing` — mit einer „Ausrollstrecke“ von 6.639 m auf einer
/// 3.405-m-Bahn aus dem Steigflug.
///
/// Reihenfolge: erst den Eintrag mit den Werten DIESES Aufsetzers sichern,
/// dann alles zuruecksetzen, was zu ihm gehoert. Aufgerufen vom Sampler
/// (sofort, er braucht die Re-Armierung fuer einen Aufprall Sekunden spaeter,
/// JGXrGwlwa4aOGZpn) und von `step_flight_at` (nach der Haltezeit, wenn kein
/// Sampler laeuft). Wer zuerst kommt, schliesst ab; danach ist keine Landung
/// mehr offen.
fn landung_beim_wegsteigen_abschliessen(stats: &mut FlightStats, snap: &SimSnapshot, now: DateTime<Utc>) {
    let Some(aufsetzen) = offene_landung_beim_wegsteigen(stats) else {
        return;
    };
    let event = TouchdownEvent {
        timestamp: aufsetzen,
        kind: TouchdownKind::TouchAndGo,
        // Ueber die Kanonik — dieselbe Zahl, die Live-Meldung und PIREP fuer
        // diesen Aufsetzer tragen.
        peak_vs_fpm: stats.canonical_landing_rate_fpm().unwrap_or(0.0),
        peak_g: stats
            .landing_peak_g_force
            .or(stats.sampler_touchdown_g_force)
            .unwrap_or(0.0),
        lat: stats.landing_lat.unwrap_or(snap.lat),
        lon: stats.landing_lon.unwrap_or(snap.lon),
        sub_bounces: stats.bounce_count,
    };
    // War der Aufsetzer schon vorlaeufig als finale Landung verbucht
    // (`FINAL_LANDING_VERBUCHEN_NACH_SECS`), wird aus genau diesem Eintrag
    // der Touch-and-Go — kein zweiter Eintrag fuer denselben Aufsetzer.
    match stats.touchdown_events.last_mut() {
        Some(letzter) if letzter.timestamp == aufsetzen => *letzter = event.clone(),
        _ => stats.touchdown_events.push(event.clone()),
    }
    // JBU327-Fix: ein Touch-and-Go / Balked-Landing IST ein Go-Around — der
    // Anflug wurde abgebrochen und neu angesetzt. Reporting-only, KEIN
    // Score-Einfluss (`go_around_count` fliesst nicht in `landing_scoring`).
    stats.go_around_count = stats.go_around_count.saturating_add(1);
    // P-9.44: Die eingefrorene Herkunft DIESES Aufsetzers (vorher in
    // `aufsetzer_einfrieren`) bekommt seine eigene Entscheidung nach.
    let aufsetzer = serde_json::json!(stats.touchdown_events);
    let durchstarts = stats.go_around_count;
    if let Some((_, _, b)) = stats.aufsetzer_eingefroren.last_mut() {
        let h = &mut b.herkunft;
        if h["aufsetzen"] == serde_json::json!(aufsetzen) {
            h["aufsetzer"] = aufsetzer;
            h["durchstarts"] = durchstarts.into();
        }
    }
    stats.vref_gemessen = None;
    // Kap. 5.2 (Codex 30.10.): Ein Aufsetzer, der den FSM-Landing-Arm nie
    // erreicht, setzt sie nicht neu — sonst dokumentierte er die Vref
    // dieses Touch-and-Go.
    stats.landing_vref_gemessen = None;
    // Lernpaket AP4/AP5: Schnitt fuer die Forensik.
    anflug_forensik_schneiden(stats, Some(now));
    let ga_count = stats.go_around_count;
    tracing::info!(
        go_around_count = ga_count,
        peak_vs = event.peak_vs_fpm,
        peak_g = event.peak_g,
        sub_bounces = event.sub_bounces,
        agl_ft = snap.altitude_agl_ft,
        "touch-and-go classified (counts as go-around) — landing closed on climb-out"
    );
    stats.pending_acars_logs.push(format!(
        "Go-around #{} (touch-and-go) — V/S {:.0} fpm, G {:.2}",
        ga_count, event.peak_vs_fpm, event.peak_g
    ));

    // ── Die Landung der Phasenlogik ───────────────────────────────────
    stats.landing_at = None;
    stats.landing_peak_vs_fpm = None;
    stats.landing_peak_g_force = None;
    stats.landing_score = None;
    stats.landing_score_announced = false;
    stats.landing_lat = None;
    stats.landing_lon = None;
    stats.bounce_count = 0;
    stats.light_bounce_count = 0;
    stats.bounce_luft_seit_ms = None;
    stats.bounce_luft_max_ft = 0.0;
    stats.bounce_armed_above_threshold = false;
    stats.bounce_boden_agl_ft = None;
    // v0.20 (QS-Fix, /code-review high): ohne diesen Reset ueberlebt die
    // Forensik der T&G-Landung bis zum naechsten Sampler-Dump — faellt der
    // echte finale Touchdown auf `fallback_zero` zurueck, laese die Kanonik
    // sonst den Edge-Wert aus dem ALTEN `landing_analysis`.
    stats.landing_rate_fpm = None;
    stats.landing_analysis = None;
    stats.abfangen_gesichert = None;
    stats.landing_source = None;
    stats.touch_and_go_pending_since = None;
    stats.sampler_steigflug_seit = None;
    // Die Live-Meldung dieses Aufsetzers ist nicht die Landung des Flugs:
    // weder vor dem Einreichen nachschicken (sonst kaeme der abgebrochene
    // Aufsetzer als „die Landung“ an, wenn seine Meldung nicht bestaetigt
    // war) noch als Vergleichswert fuer den PIREP nehmen. Den Touch-and-Go
    // traegt der PIREP in `aufsetzer`.
    stats.landung_payload = None;
    stats.landung_live_bestaetigt = false;
    stats.gemeldete_landung = None;
    stats.landung_bewertung = None;
    stats.landung_sofort = None;
    stats.landung_endgueltig = None;

    // ── Die Landung des Aufsetz-Samplers ──────────────────────────────
    // Re-armiert den Sampler fuer den naechsten Aufsetzer (sein
    // Einmal-Riegel ist `sampler_touchdown_at.is_none()`).
    stats.sampler_touchdown_at = None;
    stats.sampler_touchdown_vs_fpm = None;
    stats.sampler_touchdown_vs_vom_plugin = false;
    stats.sampler_touchdown_g_force = None;
    stats.pending_td_at = None;
    stats.touchdown_window_dumped_at = None;
    // GAF-707-Race: neue Episode, alte Rohdaten sind nicht mehr gueltig.
    stats.touchdown_window_score_data_ready_at = None;
    stats.post_touchdown_buffer.clear();
    stats.landing_score_finalized = false;
    // Auch die Merkung „Landung nicht messbar" — sonst schwiege die Warnung
    // fuer die ECHTE Landung danach (Cloud-QS 23.09.2026).
    stats.landung_unmessbar_gemeldet = false;

    // ── Was zu jeder Landung gehoert ──────────────────────────────────
    // Wind, Eigengeschwindigkeit, Ablehnungsgrund (v1.6.3), Sprit (v1.7.35),
    // Anflug-Stabilitaet + Rollstrecke + Spur (v0.16.6). Die Rollstrecke
    // fehlte bis hierher im Pfad der Phasenlogik: Bei einem Touch-and-Go
    // ohne Sampler blieb die Strecke des Steigflugs stehen.
    landung_episode_zuruecksetzen(stats);
    sprit_durchstart_zuruecksetzen(stats);
    clear_approach_stability_and_rollout(stats);

    // ── Der naechste Anflug beginnt frisch ────────────────────────────
    // Ohne diesen Reset waere der ~50-ft-Tiefpunkt dieses Aufsetzers die
    // Messlatte des naechsten Durchstart-Detektors — jeder spaetere Anflug
    // loeste beim Steigen ueber 250 ft sofort einen falschen Durchstart aus.
    stats.lowest_agl_during_approach_ft = None;
    stats.tiefster_anflugpunkt = None;
    stats.go_around_climb_pending_since = None;
    // v0.5.11: frischer Steigflug-Gipfel, sonst kippte der Steigflug nach dem
    // Touch-and-Go ueber Altdaten sofort in den Sinkflug.
    stats.climb_peak_msl = None;
    stats.landung_abgebrochen_phase_offen = true;
}

// Groundspeed-Schwelle, unter der TakeoffRoll (nachhaltig) als „rollt
// nicht" gilt. 10 kt unter dem Vorwärts-Trigger (GS > 30 kt) → Hysterese
// gegen Flip-Flop an der Schwelle.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_abort_kt (v2.1.24: 20.0)
// Dwell, den die GS unter der Schwelle bleiben muss, bevor demotet wird
// (frisst GS-Einzeltick-Glitches; braucht ≥ 2 Ticks über ≥ 3 s).
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_abort_dwell_secs (v2.1.24: 3)

// „Committed-Takeoff"-Schwelle (v0.21.x, Backtrack-Erkennung): Peak-GS,
// ab der eine Rolle zweifelsfrei eine echte Startrolle ist. Über 673
// GSG-Flüge erreicht JEDE echte Startrolle ≥ 52 kt Peak (min), während
// die 4 Backtrack-/Fehl-Trips bei ≤ 33 kt plateauisieren — 45 kt liegt
// sauber in der Lücke. Hat der Peak diese Marke erreicht, ist die Plateau-
// Rückkante deaktiviert (nur noch die GS<20-Abbruchkante greift).
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_confirm_kt (v2.1.24: 45.0)
// Dwell für die Plateau-Rückkante: bleibt der Peak-GS unter
// `TAKEOFF_ROLL_CONFIRM_KT` UND setzt die GS keine neuen Höchstwerte mehr
// (Rolle entwickelt sich nicht weiter = Backtrack/fast-Taxi), so lange →
// zurück zu TaxiOut. Eine echte Startrolle setzt laufend neue GS-Höchst-
// werte → der Dwell wird nie voll (Reset bei jedem neuen Peak), unabhängig
// davon, wie zügig sie beschleunigt.
// Wert seit v3: bundle/grenzwerte/phasen_kern.json -> takeoff_roll_stall_dwell_secs (v2.1.24: 12)

/// Rückkante der TakeoffRoll-Phase — behebt den flottenweiten
/// TAKEOFF_ROLL_STUCK (5 % der Flüge, 614-Flug-Scan 2026-07-05).
///
/// `TaxiOut → TakeoffRoll` triggert bei GS > 30 kt am Boden. Das trippt
/// auch ein schneller BACKTRACK (z. B. die kurze EGLC-Bahn zum Schwellen-
/// kopf runterrollen). Bremst der Flieger dann zum Ausrichten/Warten wieder
/// ab, hing die Phase bisher für IMMER auf „Takeoff Roll" — der TakeoffRoll-
/// Arm kannte nur die Vorwärtskante (on_ground → airborne = Takeoff).
///
/// Fällt die Groundspeed nachhaltig (Dwell) unter `TAKEOFF_ROLL_ABORT_KT`
/// während der Flieger am Boden ist, zurück zu TaxiOut. Während einer
/// ECHTEN Startrolle ist die Bedingung nie erfüllt (GS steigt dort monoton
/// von > 30 auf > 150 kt, dippt nie unter 20).
///
/// Pur + uhrlos (Caller liefert `now`) → direkt testbar. Rückgabe:
/// (Ziel-Phase falls demotet, neuer `slow_since`-Wert für die Stats).
fn takeoff_roll_demote(
    on_ground: bool,
    groundspeed_kt: f32,
    slow_since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> (Option<FlightPhase>, Option<DateTime<Utc>>) {
    if on_ground && groundspeed_kt < crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_abort_kt.wert {
        let since = slow_since.unwrap_or(now);
        if (now - since).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_abort_dwell_secs.wert {
            (Some(FlightPhase::TaxiOut), None) // Dwell erfüllt → demote + Reset
        } else {
            (None, Some(since)) // Dwell läuft noch → Phase halten
        }
    } else {
        (None, None) // GS zurück über Schwelle / airborne → Dwell verwerfen
    }
}

/// Zweite Rückkante der TakeoffRoll-Phase (v0.21.x) — BACKTRACK-ERKENNUNG.
///
/// Die GS<20-Abbruchkante ([`takeoff_roll_demote`]) fängt nur den vollen
/// Stillstand. Ein schneller BACKTRACK / Fehl-Trip plateauisiert aber bei
/// Taxi-Speed (~30 kt, ÜBER der 20-kt-Schwelle) und hing dadurch lange auf
/// „TakeoffRoll" (Feld: 39–131 s). Über 673 Flüge trennt sich echt vs.
/// falsch sauber am Peak-GS: echte Startrollen ≥ 52 kt, Fehl-Trips ≤ 33 kt.
///
/// Diese Kante demotet, wenn der Peak-GS `TAKEOFF_ROLL_CONFIRM_KT` NIE
/// erreicht UND die GS keine neuen Höchstwerte mehr setzt (Plateau) für
/// `TAKEOFF_ROLL_STALL_DWELL_SECS`. Bulletproof gegen echte (auch träge)
/// Startrollen: die setzen monoton neue GS-Höchstwerte → `stall_since` wird
/// bei JEDEM neuen Peak zurückgesetzt, der Dwell also nie voll; und sobald
/// der Peak ≥ CONFIRM_KT war, ist die Kante ganz aus („committed").
///
/// Pur + uhrlos → direkt testbar. Rückgabe:
/// (Ziel-Phase falls demotet, neuer Peak-GS, neuer `stall_since`).
fn takeoff_roll_stall_check(
    on_ground: bool,
    groundspeed_kt: f32,
    peak_kt: f32,
    stall_since: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> (Option<FlightPhase>, f32, Option<DateTime<Utc>>) {
    let new_peak = peak_kt.max(groundspeed_kt);
    // Airborne oder bereits echte Startgeschwindigkeit gesehen → committed,
    // keine Plateau-Demote mehr (die GS<20-Abbruchkante bleibt aktiv).
    if !on_ground || new_peak >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_confirm_kt.wert {
        return (None, new_peak, None);
    }
    if groundspeed_kt > peak_kt {
        // Frischer Höchstwert → Rolle entwickelt sich → Dwell verwerfen.
        (None, new_peak, None)
    } else {
        // Plateau unter CONFIRM_KT → Dwell zählen.
        let since = stall_since.unwrap_or(now);
        if (now - since).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_stall_dwell_secs.wert {
            (Some(FlightPhase::TaxiOut), new_peak, None) // Backtrack erkannt
        } else {
            (None, new_peak, Some(since))
        }
    }
}
