/// How often we POST `/pireps/{id}/update` purely to bump `pireps.updated_at`
/// and keep phpVMS's `RemoveExpiredLiveFlights` cron from soft-deleting
/// the in-flight PIREP. The cron runs hourly and looks at `updated_at`,
/// NOT at the latest position row — without this heartbeat, a long
/// cruise leg with no phase changes gets killed after `acars.live_time`
/// hours (default 2h on most installs). vmsACARS uses 30 s by default
/// (`acars_update_timer`); we match.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);

/// v0.4.1: nach wie vielen Sekunden ohne brauchbaren Sim-Snapshot
/// wir den Flug als „pausiert" markieren (Sim-Crash, Quit, Timeout).
/// 30 s ist ein guter Kompromiss: lang genug dass kurze FPS-Drops oder
/// Pause-Menü-Ausflüge keinen False-Positive triggern, kurz genug dass
/// der Pilot den Disconnect zeitnah mitkriegt und re-positionieren kann
/// bevor phpVMS' Live-Tracking-Cron (default ~2h) den PIREP killt.
const SIM_DISCONNECT_THRESHOLD_S: i64 = 30;

/// v0.12.5 (Spec docs/spec/v0.12.5-divert-and-manual-pirep.md, LE9):
/// Maximales Alter eines Sim-Snapshots, ab dem das Resume-Sim-Gate den
/// Flug scharfschaltet. Bewusst ENGER als `SIM_DISCONNECT_THRESHOLD_S` —
/// letzteres ist Disconnect-Erkennung im laufenden Flug, hier geht es ums
/// Scharfschalten NACH Resume: ein alter, vor dem Neustart persistierter
/// Snapshot darf nicht zählen. ≤ 5 s ⇒ frisch genug, dass der Sim jetzt
/// gerade Daten liefert.
const SIM_GATE_FRESH_SECS: i64 = 5;
/// Ab so langer Wartezeit im Resume-Gate bekommt der Pilot einen deutlichen
/// Hinweis, woran es liegen kann (Pause, Slew, Zeitbeschleunigung).
const RESUME_GATE_WARNUNG_SECS: i64 = 120;

// Spec sim-disconnect-auto-resume F2 (Drift-Stufen):
//
// Schwellen fuer die Activity-Log-Eskalation beim Resume. Drift NM
// gemessen zwischen `paused_last_known` und dem ersten Post-Resume-
// Snapshot. Kein Level blockiert Auto-Resume (Spec-Prinzip P2:
// informieren statt blockieren).
//
//   <  1 NM : `▶ Flug automatisch fortgesetzt` (kein Distanz-Suffix)
//   1-50    : Info-Level mit `repositioniert X nm`
//   50-200  : Warn-Level (auffaellige Reposition)
//   > 200   : Warn-Level mit Hinweis „pruefe ob der richtige Flug geladen ist"
//
// Schwellen sind empirisch — typischer Autosave bei MSFS ist ~10 min
// alt, was bei 480 kt etwa 80 NM Drift produziert (= Warn-Range,
// erwartbar). >200 NM ist verdaechtig: entweder Bid-Reload am
// Departure-Airport (= falscher Flug?) oder grosser Slew-Move.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_drift_toast_nm (v2.1.24: 1.0)
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_drift_warn_nm (v2.1.24: 50.0)
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_drift_extreme_nm (v2.1.24: 200.0)

// v0.20 (Process-Integrity): ein Fuel-ANSTIEG von mehr als dieser Menge
// zwischen Pause/Neustart-Snapshot und dem ersten frischen Snapshot danach
// ist physikalisch unmöglich (Flugzeuge tanken nicht mitten im Flug nach)
// — starkes Indiz für einen Sim/App-Reload mit frischem/anderem Ladezustand
// statt einer echten durchgehenden Simulation. Bewusst nur die ZUNAHME
// bewerten, nicht den Betrag: eine normale Verbrauchs-ABNAHME (auch über
// mehrere Tonnen bei langen Pausen) ist harmlos.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_fuel_jump_impossible_kg (v2.1.24: 200.0)

// „Sprung ans Ziel": Der Sim stuerzt unterwegs ab, und danach steht das
// Flugzeug am ANKUNFTSFLUGHAFEN — nicht dort, wo der Flug abbrach.
//
// Regel (Thomas, 23.09.2026): Nach einem Sim-Absturz darf sich der Pilot in
// der Gegend der Unterbrechung neu hinstellen und weiterfliegen. Was NICHT
// geht: sich ans Ziel stellen und den Flug genehmigen lassen.
//
// Deshalb zwei Abstaende zum geplanten Ziel: War der Flug vorher weiter als
// `RESUME_ZIELSPRUNG_VORHER_NM` entfernt und steht er danach naeher als
// `RESUME_ZIELSPRUNG_NACHHER_NM` dran, ist das der Sprung ans Ziel. Wer
// kurz vor dem Platz abbricht (innerhalb der ersten Schwelle), faellt
// bewusst nicht darunter — dort ist Neuladen am Platz der normale Weg.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_zielsprung_vorher_nm (v2.1.24: 15.0)
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_zielsprung_nachher_nm (v2.1.24: 5.0)
// Schnellste Grundgeschwindigkeit, die noch als geflogen durchgeht (kt).
// Bewusst weit ueber jeder Reisegeschwindigkeit: Ein Jet im Jetstream
// schafft 600 kt ueber Grund. Wer mehr „fliegt", ist gesprungen.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_max_plausibel_kt (v2.1.24: 700.0)

/// v0.20 (Process-Integrity): Ergebnis des Vergleichs "letzter bekannter
/// Snapshot vor der Pause/dem Neustart" vs. "erster frischer Snapshot
/// danach". `fuel_delta_kg` bleibt VORZEICHENBEHAFTET (aktuell − vorher) —
/// anders als die Anzeige in `apply_pause_resume`, die nur den Betrag
/// zeigt. Wird sowohl vom Sim-Pause-Resume-Pfad als auch vom App-Neustart-
/// Resume-Pfad (`try_resume_flight`) genutzt, damit beide dieselbe Logik
/// und dieselben Schwellwerte teilen statt sie zu duplizieren.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
struct ResumeDiscontinuity {
    drift_nm: f64,
    altitude_delta_ft: f64,
    fuel_delta_kg: f64,
    /// v0.20 QS-Fix: true wenn SOWOHL der "vorher"- als auch der "jetzt"-
    /// Snapshot `on_ground` waren. Ein Fuel-ANSTIEG während das Flugzeug
    /// durchgehend am Boden stand ist ein ganz normales Nachtanken während
    /// einer Pause — KEIN unmöglicher Sprung. Ist dagegen mindestens eine
    /// Seite airborne (der tatsächliche Vorfall-Fall: airborne vorher,
    /// geparkt danach), bleibt ein Fuel-Anstieg weiter verdächtig.
    both_grounded: bool,
    /// Abstand zum geplanten Ziel VOR der Unterbrechung (nm). `None`, wenn
    /// die Zielkoordinaten noch nicht bekannt sind.
    ///
    /// `serde(default)` ist Pflicht: Ein Flug, der mit einer aelteren
    /// Fassung gespeichert wurde und schon einen Sprung vermerkt hatte,
    /// haette sonst beim Wiederaufnehmen nach einem Update nicht mehr
    /// gelesen werden koennen — ausgerechnet der Fall, fuer den das hier
    /// gebaut ist (Cloud-QS 23.09.2026).
    #[serde(default)]
    ziel_vorher_nm: Option<f64>,
    /// Abstand zum geplanten Ziel DANACH (nm).
    #[serde(default)]
    ziel_nachher_nm: Option<f64>,
    /// Stand das Flugzeug beim Wiederaufnehmen am Boden?
    #[serde(default)]
    danach_am_boden: bool,
    /// Wie lange war die Verbindung weg (Sekunden)? `None`, wenn die Dauer
    /// nicht bekannt ist — dann gilt im Zweifel der Pilot als ehrlich.
    #[serde(default)]
    luecke_secs: Option<i64>,
    /// Kein gemessener Sprung, sondern die ausdrueckliche Bestaetigung des
    /// Piloten „Trotzdem fortsetzen". Sprit- und Hoehendelta sind hier
    /// konstruktionsbedingt 0 und gehoeren NICHT in die Notiz — sonst legt
    /// sie dem Pruefer Messwerte vor, die keine sind (Cloud-QS 23.09.2026).
    #[serde(default)]
    nur_bestaetigt: bool,
}

/// Taugt eine Position als Messpunkt fuer Pause/Wiederaufnahme?
///
/// Log-Durchsicht 26.09.2026 (Recorder-Fix Befund 1): Im Lade-/Menue-
/// bildschirm liefert MSFS eine Scheinposition LAT 0,0000 / LON 90,0003 mit
/// Hoehen wie −2 109 340 ft. Wurde so ein Punkt Pausen-Referenz, meldete der
/// Client „unmoeglicher Sprung" mit +2 110 082 ft und 5 038 nm Drift
/// (n13rGYv4aLpBlZw4, aKRgO8gxYzPnLR8B) — Messfehler, kein Sprung.
fn position_plausibel(lat: f64, lon: f64, hoehe_ft: f64) -> bool {
    let menue_scheinposition = lat.abs() < 0.001 && (lon - 90.0).abs() < 0.01;
    lat.is_finite()
        && lon.is_finite()
        && lat.abs() <= 90.0
        && lon.abs() <= 180.0
        && !menue_scheinposition
        && (-2_000.0..=100_000.0).contains(&hoehe_ft)
}

fn snapshot_plausibel(s: &SimSnapshot) -> bool {
    position_plausibel(s.lat, s.lon, s.altitude_msl_ft)
}

#[cfg(test)]
#[path = "tests_lib/scheinposition_tests.rs"]
mod scheinposition_tests;

/// Vergleicht den letzten bekannten Snapshot vor einer Pause/einem
/// Neustart (`prev`) mit dem ersten frischen Snapshot danach (`cur`).
fn compute_resume_discontinuity(
    prev: &PausedSnapshot,
    cur: &SimSnapshot,
    ziel: Option<(f64, f64)>,
    luecke_secs: Option<i64>,
) -> ResumeDiscontinuity {
    let d_m = ::geo::distance_m(prev.lat, prev.lon, cur.lat, cur.lon);
    let nm_zum_ziel = |lat: f64, lon: f64| {
        ziel.map(|(zlat, zlon)| ::geo::distance_m(lat, lon, zlat, zlon) / 1852.0)
    };
    ResumeDiscontinuity {
        drift_nm: d_m / 1852.0,
        altitude_delta_ft: cur.altitude_msl_ft - prev.altitude_ft,
        fuel_delta_kg: cur.fuel_total_kg as f64 - prev.fuel_total_kg as f64,
        both_grounded: prev.on_ground && cur.on_ground,
        ziel_vorher_nm: nm_zum_ziel(prev.lat, prev.lon),
        ziel_nachher_nm: nm_zum_ziel(cur.lat, cur.lon),
        danach_am_boden: cur.on_ground,
        luecke_secs,
        nur_bestaetigt: false,
    }
}

/// Liess sich die Strecke zwischen den beiden Punkten in der Zeit, in der
/// die Verbindung weg war, ueberhaupt fliegen? Ohne bekannte Dauer: ja
/// (im Zweifel fuer den Piloten).
fn strecke_war_fliegbar(d: &ResumeDiscontinuity) -> bool {
    match d.luecke_secs {
        Some(secs) if secs > 0 => {
            let stunden = secs as f64 / 3600.0;
            d.drift_nm / stunden <= crate::grenzwerte::grenzwerte().abgabe.resume_max_plausibel_kt.wert
        }
        // Luecke unbekannt oder null — keine Aussage moeglich.
        _ => true,
    }
}

/// Stand der Flug vorher weit weg vom Ziel und steht er danach AM Ziel?
/// Siehe `RESUME_ZIELSPRUNG_VORHER_NM`.
///
/// `danach_am_boden` ist Teil der Regel, nicht Beiwerk: Reisst SimConnect
/// unterwegs ab, waehrend der Simulator weiterlaeuft, und kommt die
/// Verbindung erst im Endanflug zurueck, sind beide Abstaende ebenfalls
/// „vorher weit, nachher nah" — geflogen ist der Pilot aber selbst. Wer
/// sich dagegen ans Ziel STELLT, steht dort (Cloud-QS 23.09.2026).
fn ist_sprung_ans_ziel(d: &ResumeDiscontinuity) -> bool {
    match (d.ziel_vorher_nm, d.ziel_nachher_nm) {
        (Some(vorher), Some(nachher))
            if vorher > crate::grenzwerte::grenzwerte().abgabe.resume_zielsprung_vorher_nm.wert && nachher < crate::grenzwerte::grenzwerte().abgabe.resume_zielsprung_nachher_nm.wert =>
        {
            // Zwei Wege zum selben Urteil, und beide brauchen ein Merkmal
            // ausser der Nachbarschaft zum Ziel — sonst traefe es jeden
            // Piloten, dem SimConnect im Anflug abreisst und der den Rest
            // selbst fliegt und landet (Cloud-QS 23.09.2026, zweite Runde):
            //
            //   * die Uhr: die Strecke war in der Zeit nicht zu fliegen;
            //   * der Sprit: es wurde ueberhaupt nichts verbraucht. Wer
            //     die Meilen wirklich geflogen ist, hat getankt — MSC1588
            //     kam mit exakt demselben Tankstand 49 nm weiter.
            !strecke_war_fliegbar(d) || (d.danach_am_boden && !sprit_wurde_verbraucht(d))
        }
        _ => false,
    }
}

/// Ist zwischen den beiden Punkten ueberhaupt Sprit verbrannt worden?
/// Ein Flugzeug, das Meilen zurueckgelegt hat, hat welchen verbraucht;
/// ein neu geladenes steht mit dem alten (oder einem hoeheren) Stand da.
fn sprit_wurde_verbraucht(d: &ResumeDiscontinuity) -> bool {
    d.fuel_delta_kg < 0.0
}

// Ab dieser Laenge zaehlt eine Unterbrechung fuer die Abgabe-Frage.
//
// Kurze Aussetzer (Ladebildschirm, Menue, ein verlorener Tick) sind
// Alltag und sollen niemanden vor eine Entscheidung stellen. Zwei Minuten
// ohne Simulator sind etwas anderes: Danach steht das Flugzeug erfahrungs-
// gemaess woanders.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> abgabe_unterbrechung_min_secs (v2.1.24: 120)

/// Warum die App diesen Flug NICHT von selbst einreichen darf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum AbgabeSperre {
    /// Beim Wiederaufnehmen sprang der Zustand (MSC1588, 22.09.2026).
    Sprung,
    /// Der Flug war unterbrochen, und am Ende wurde nie eine Landung
    /// gemessen (GAF 9655, 23.09.2026): Der Simulator kam am Boden am Ziel
    /// zurueck, die Phasen liefen im Stand bis „Arrived" durch, und die App
    /// reichte einen Flug ein, dessen Aufsetzen sie nie gesehen hat.
    LandungFehlt,
}

impl AbgabeSperre {
    fn code(self) -> &'static str {
        match self {
            AbgabeSperre::Sprung => "sprung",
            AbgabeSperre::LandungFehlt => "landung_fehlt",
        }
    }
}

/// War der Flug laenger als `ABGABE_UNTERBRECHUNG_MIN_SECS` ohne Simulator
/// — oder hat ihn ein App-/Sim-Neustart unterbrochen?
fn flug_war_unterbrochen(stats: &FlightStats) -> bool {
    // Die Schwelle gilt fuer BEIDE Arten von Unterbrechung. `resume_gap_minutes`
    // wird bei jedem App-Neustart gesetzt, auch bei `Some(0)` — ohne Schwelle
    // zaehlte der Update-Relaunch mitten im Flug als Unterbrechung
    // (Cloud-QS 23.09.2026).
    // Die LAENGSTE Luecke, nicht die letzte: ein kurzer zweiter Neustart
    // darf einen langen ersten nicht ueberschreiben.
    let neustart_lang_genug = stats
        .resume_luecke_max_secs
        .or(stats.resume_luecke_secs)
        .or_else(|| stats.resume_gap_minutes.map(|m| m * 60))
        .is_some_and(|secs| secs >= crate::grenzwerte::grenzwerte().abgabe.abgabe_unterbrechung_min_secs.wert);
    neustart_lang_genug
        || !stats.resume_spruenge.is_empty()
        || stats
            .pause_segments
            .iter()
            .any(|p| p.duration_secs >= crate::grenzwerte::grenzwerte().abgabe.abgabe_unterbrechung_min_secs.wert)
}

/// Wurde die Landung ueberhaupt gemessen? Ohne Aufsetzfenster gibt es
/// weder Rate noch Note — dann hat die App das Aufsetzen nicht gesehen.
fn landung_wurde_gemessen(stats: &FlightStats) -> bool {
    // ⚠ NICHT `landing_rate_fpm` lesen: Dort steht auch der Notnagel
    // `fallback_zero`, und bei gesetztem `landung_abdeckung_fehlt` steht
    // eine Zahl, die nirgends gilt. `canonical_landing_rate_fpm` ist die
    // eine Stelle, die all diese Riegel kennt — sonst greift die Sperre
    // ausgerechnet in der Fehlerklasse nicht, fuer die sie gebaut ist
    // (Cloud-QS 23.09.2026).
    stats.canonical_landing_rate_fpm().is_some() || stats.landing_score.is_some()
}

/// Haelt fest, dass der Pilot den Knopf „Trotzdem fortsetzen" gedrueckt
/// hat, obwohl die Lage im Simulator nicht zum gespeicherten Flug passte.
///
/// ⚠ NUR von dort aufrufen. Der Countdown im Resume-Banner bestaetigt von
/// selbst, und wer danach den Simulator neu laedt, steht beim ersten
/// Snapshot regelmaessig am Boden — ohne diese Bedingung bekaeme jeder
/// ehrliche Neustart einen Sprung-Vermerk (Cloud-QS 23.09.2026).
fn sprung_beim_fortsetzen_vermerken(app: &AppHandle, flight: &ActiveFlight, snap: &SimSnapshot) {
    let (referenz, darf_weitergeflogen, ziel, luecke_secs) = {
        let stats = flight.stats.lock().expect("flight stats");
        (
            // Dauerhafter Punkt: `last_lat/lon` ist nach einem Resume leer,
            // `paused_last_known` ueberlebt keinen Neustart — sonst fehlte
            // der Vermerk genau nach Absturz + „Trotzdem fortsetzen".
            lage_referenz(&stats),
            darf_weitergeflogen_sein(&stats),
            stats.planned_arr_ref_pos,
            stats.resume_luecke_secs,
        )
    };
    // Dieselbe Regel wie das Banner: Was dort ohne Klick weiterlaufen
    // duerfte, ist kein Sprung.
    let Some(referenz) = referenz else {
        return;
    };
    if lage_abweichung(&referenz, snap, Utc::now(), darf_weitergeflogen).is_none() {
        return;
    }
    let (plat, plon) = (referenz.lat, referenz.lon);
    // Menue-Scheinposition (LAT 0 / LON 90) auf einer der beiden Seiten
    // ist kein Sprung, sondern ein Messfehler (Log-Durchsicht 26.09.2026).
    // Die Hoehe der Referenz kennen wir hier nicht — sie ist die aktuelle.
    if !snapshot_plausibel(snap) || !position_plausibel(plat, plon, snap.altitude_msl_ft) {
        return;
    }
    let vorher = PausedSnapshot {
        lat: plat,
        lon: plon,
        heading_deg: 0.0,
        altitude_ft: snap.altitude_msl_ft,
        fuel_total_kg: snap.fuel_total_kg,
        zfw_kg: None,
        on_ground: false,
    };
    let mut d = compute_resume_discontinuity(&vorher, snap, ziel, luecke_secs);
    d.nur_bestaetigt = true;
    {
        let mut stats = flight.stats.lock().expect("flight stats");
        stats.resume_discontinuity = Some(d);
        stats.resume_spruenge.push(d);
    }
    save_active_flight(app, flight);
    log_activity_and_record(
        app,
        &flight.pirep_id,
        ActivityLevel::Warn,
        "Trotz erkannter Abweichung fortgesetzt".to_string(),
        Some(format!(
            "Der Simulator passte beim Fortsetzen nicht zum gespeicherten Flug \
             (Position {:.1} nm, am Boden: {}). Der Flug läuft weiter, aber die \
             App reicht ihn nicht mehr von selbst ein — du entscheidest am Ende.",
            d.drift_nm, snap.on_ground
        )),
    );
    tracing::warn!(
        pirep_id = %flight.pirep_id,
        drift_nm = d.drift_nm,
        on_ground = snap.on_ground,
        "Resume trotz Abweichung bestaetigt — als Sprung vermerkt"
    );
}

/// Backend-Auto-File: Latcht der FSM auf `Arrived` und hat der Pilot
/// Auto-File aktiviert, reicht das Backend selbst ein — fenster-unabhaengig
/// (v0.12.6; vorher ein Frontend-`useEffect`, der bei nicht-aktivem Cockpit
/// nicht feuerte).
///
/// ⚠ Wird bei JEDEM Takt in `Arrived` aufgerufen, nicht nur beim Uebergang:
/// Die Landebewertung wird Sekunden nach dem Aufsetzen fertig, und wer nur
/// den Uebergang prueft, trifft womoeglich das Fenster, in dem die Sperre
/// noch steht. Danach kaeme kein zweiter Uebergang — der Flug bliebe
/// haengen (Cloud-QS 23.09.2026). Der Merker `auto_file_gestartet` sorgt
/// dafuer, dass trotzdem nur EIN Versuch laeuft.
fn auto_file_versuchen(app: &AppHandle, flight: &Arc<ActiveFlight>, beim_wechsel: bool) {
    if !app
        .state::<AppState>()
        .auto_file_enabled
        .load(Ordering::Relaxed)
    {
        return;
    }
    // 1a (Rundumschlag 22.10.2026): Kabinen-Einstellung ausserhalb der
    // Flugdaten-Sperre lesen.
    let ansagen_an = kabinenansagen::einstellungen(app).an;
    let entscheidung = {
        let mut st = flight.stats.lock().expect("flight stats");
        if st.auto_file_gestartet {
            return;
        }
        let is_divert = st.divert_hint.is_some();
        let sperre = abgabe_sperre(&st);
        if is_divert || sperre.is_some() {
            Err((is_divert, sperre))
        } else if kabinenansagen::kabine_haelt_abgabe(&mut st, ansagen_an, Utc::now().timestamp()) {
            // 1a: Erst die Kabine zu Ende spielen lassen (Tueren entschaerfen,
            // Aussteigen), hoechstens 90 s — sonst fiel „Aussteigen“ in 8 von
            // 17 Fluegen weg, und das Protokoll sagte es nicht. Der Takt ruft
            // hier in `Arrived` jede Sekunde wieder auf.
            return;
        } else {
            st.auto_file_gestartet = true;
            Ok(())
        }
    };
    match entscheidung {
        // Beim Wechsel nach `Arrived` EINMAL sichtbar ins Log, danach im Takt
        // nur noch leise — sonst laesst sich im Feld nicht nachvollziehen,
        // warum nicht eingereicht wurde (Cloud-QS 23.09.2026, dritte Runde).
        Err((true, _)) => {
            // Den Ausweich-Airport bestaetigt der Pilot im Divert-Banner (LE2).
            if beim_wechsel {
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    "backend auto-file skipped — divert detected, pilot confirms via banner"
                );
            }
        }
        Err((false, Some(grund))) => {
            // „Arrived" sagt hier nichts: Entweder sprang der Zustand beim
            // Wiederaufnehmen (MSC1588), oder der Flug war unterbrochen und
            // die Landung wurde nie gemessen (GAF 9655). Der Pilot
            // entscheidet im Banner — verwerfen oder bewusst einreichen.
            if beim_wechsel {
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    grund = grund.code(),
                    "backend auto-file ausgesetzt — der Pilot entscheidet im Banner"
                );
            } else {
                tracing::debug!(
                    pirep_id = %flight.pirep_id,
                    grund = grund.code(),
                    "backend auto-file weiter ausgesetzt"
                );
            }
        }
        Err((false, None)) => {}
        Ok(()) => {
            tracing::info!(pirep_id = %flight.pirep_id, "Arrived — backend auto-file");
            let app_af = app.clone();
            let flight_af = Arc::clone(flight);
            tauri::async_runtime::spawn(async move {
                let st = app_af.state::<AppState>();
                match flight_end(app_af.clone(), st, None, None, None, None).await {
                    Ok(()) => {
                        // LE7-Erfolgs-Banner: das Frontend hoert auf dieses
                        // Event und zeigt das gruene ✅ — sonst verschwaende
                        // der Flug beim Backend-Auto-File kommentarlos.
                        let payload = serde_json::json!({
                            "callsign": format_callsign(
                                &flight_af.airline_icao,
                                &flight_af.flight_number,
                            ),
                            "dpt": flight_af.dpt_airport,
                            "arr": flight_af.arr_airport,
                        });
                        let _ = tauri::Emitter::emit(&app_af, "pirep_auto_filed", payload.clone());
                        // v0.16.0 (#LAN-Remote): fan out to LAN WS.
                        app_af
                            .state::<AppState>()
                            .remote_events
                            .send(remote::RemoteEvent::new("pirep_auto_filed", payload));
                    }
                    Err(e) => {
                        // Der Versuch ist gelaufen; ein zweiter waere nur ein
                        // zweiter Fehlschlag. Der Pilot uebernimmt von Hand.
                        log_activity_handle(
                            &app_af,
                            ActivityLevel::Warn,
                            "Auto-File fehlgeschlagen — bitte manuell „Flug beenden\" klicken"
                                .to_string(),
                            Some(format!("{}: {}", e.code, e.message)),
                        );
                    }
                }
            });
        }
    }
}

/// Die eine Stelle, an der entschieden wird, ob der Pilot gefragt werden
/// muss. Beide Abgabewege (Auto-File im Streamer, File-First in
/// `flight_cancel`) fragen hier — sonst wandert die Regel wieder
/// auseinander.
fn abgabe_sperre(stats: &FlightStats) -> Option<AbgabeSperre> {
    if stats.resume_discontinuity.is_some() {
        return Some(AbgabeSperre::Sprung);
    }
    if flug_war_unterbrochen(stats) && !landung_wurde_gemessen(stats) {
        return Some(AbgabeSperre::LandungFehlt);
    }
    None
}

/// Der Notiz-Block, den ein bewusst eingereichter Sprung-Flug im PIREP
/// bekommt. Ohne ihn sieht die VA nur eine normale Ankunft — genau der Fall
/// MSC1588 (22.09.2026), wo eine nie geflogene Strecke als angekommen
/// gebucht wurde. Die Begruendung des Piloten ist freiwillig; Leerraum
/// allein ist keine (die LAN-Bruecke kann schicken, was sie will).
/// So lang darf die freiwillige Begruendung im PIREP werden. Gleicher Wert
/// wie `maxLength` im Eingabefeld.
const SPRUNG_BEGRUENDUNG_MAX_ZEICHEN: usize = 500;

/// Die Begruendungszeile des Piloten — getrimmt, einzeilig, begrenzt.
///
/// Das Eingabefeld begrenzt auf 500 Zeichen, die LAN-Bruecke aber nicht:
/// Mit Zeilenumbruechen liesse sich sonst eine zweite „Begründung:"- oder
/// „DIVERT:"-Zeile in die Notiz faelschen (Cloud-QS 23.09.2026). Der Riegel
/// stand schon einmal hier und ging beim Umbau auf mehrere Spruenge still
/// verloren — deshalb jetzt an EINER Stelle, mit eigenem Test.
fn begruendungs_zeile(begruendung: Option<&str>) -> String {
    begruendungs_zeile_mit(begruendung, "Begründung")
}

/// Wie `begruendungs_zeile`, aber mit eigener Beschriftung — die
/// Divert-Begruendung kommt ueber dieselbe LAN-Bruecke und braucht
/// denselben Riegel (Cloud-QS 23.09.2026).
fn begruendungs_zeile_mit(begruendung: Option<&str>, beschriftung: &str) -> String {
    begruendung
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(|r| {
            let einzeilig: String = r
                .chars()
                // Nicht nur \n und \r: U+2028/U+2029, vertikaler Tabulator
                // und Formfeed brechen die Zeile je nach Anzeige ebenfalls
                // (Cloud-QS 23.09.2026).
                .map(|c| {
                    if c.is_control() || c == '\u{2028}' || c == '\u{2029}' {
                        ' '
                    } else {
                        c
                    }
                })
                .take(SPRUNG_BEGRUENDUNG_MAX_ZEICHEN)
                .collect();
            format!("{beschriftung}: {einzeilig}\n")
        })
        .unwrap_or_default()
}

fn sprung_notiz(
    spruenge: &[ResumeDiscontinuity],
    begruendung: Option<&str>,
    notes: &str,
) -> String {
    let zeile = begruendungs_zeile(begruendung);
    // Jeder Sprung mit eigener Zeile: Ein Flug kann mehrfach unterbrochen
    // werden, und der zweite macht den ersten nicht ungeschehen.
    let liste: String = spruenge
        .iter()
        .map(|d| {
            if d.nur_bestaetigt {
                // Hier gibt es nur einen gemessenen Wert: den Abstand.
                format!("· {} (Position {:.1} nm)\n", sprung_grund(d), d.drift_nm)
            } else {
                format!(
                    "· {} (Sprit {:+.0} kg · Höhe {:+.0} ft · Position {:.1} nm)\n",
                    sprung_grund(d),
                    d.fuel_delta_kg,
                    d.altitude_delta_ft,
                    d.drift_nm
                )
            }
        })
        .collect();
    let nur_bestaetigt = spruenge.iter().all(|d| d.nur_bestaetigt);
    // Ohne gemessenen Sprung heisst es „Abweichungen" — der Kopf darf nicht
    // behaupten, was der Satz dahinter zuruecknimmt (Cloud-QS 23.09.2026).
    let kopf = match (spruenge.len(), nur_bestaetigt) {
        (n, true) if n > 1 => format!("UNTERBROCHENER FLUG ({n} Abweichungen)"),
        (n, false) if n > 1 => format!("UNTERBROCHENER FLUG ({n} Sprünge)"),
        _ => "UNTERBROCHENER FLUG".to_string(),
    };
    // Ist KEIN Sprung gemessen worden, sondern nur die Bestaetigung des
    // Piloten da, darf der Kopf keinen Sprung behaupten (Cloud-QS
    // 23.09.2026, dritte Runde).
    let erklaerung = if nur_bestaetigt {
        "Beim Wiederaufnehmen passte der Simulator nicht zum gespeicherten \
         Flug, und der Pilot hat trotzdem fortgesetzt."
    } else {
        "Beim Wiederaufnehmen sprang der Zustand. Der Simulator wurde neu \
         geladen; der Flug ging so nicht weiter."
    };
    format!("{kopf}: {erklaerung} Vom Piloten bewusst eingereicht.\n{liste}{zeile}\n{notes}")
}

/// Notiz fuer den Fall „unterbrochen und keine Landung gemessen".
///
/// Ohne sie sieht die VA einen Bericht wie jeden anderen — mit Strecke,
/// Zeiten und Sprit, aber ohne den einen Satz, auf den es ankommt: Das
/// Aufsetzen hat niemand gesehen (GAF 9655, 23.09.2026).
fn landung_fehlt_notiz(begruendung: Option<&str>, notes: &str) -> String {
    let zeile = begruendungs_zeile(begruendung);
    format!(
        "LANDUNG NICHT GEMESSEN: Der Flug war unterbrochen, und beim \
         Aufsetzen lagen der App keine Daten vor — es gibt weder Sinkrate \
         noch Note. Vom Piloten bewusst eingereicht.\n{zeile}\n{notes}"
    )
}

/// Die Spruenge fuer die Notiz: die Liste, und fuer Fluege aus einer
/// aelteren Fassung (leere Liste) das alte Einzelfeld.
fn spruenge_fuer_notiz(stats: &FlightStats) -> Vec<ResumeDiscontinuity> {
    if !stats.resume_spruenge.is_empty() {
        stats.resume_spruenge.clone()
    } else {
        stats.resume_discontinuity.into_iter().collect()
    }
}

/// Welche Regel hat angeschlagen? Fuer Protokoll und PIREP-Notiz — eine
/// Zahlenreihe allein sagt dem Pruefer nicht, WARUM das unmoeglich war
/// (Cloud-QS 23.09.2026, Befund 8).
fn sprung_grund(d: &ResumeDiscontinuity) -> &'static str {
    if d.nur_bestaetigt {
        return "Fortsetzen trotz Abweichung bestätigt";
    }
    if ist_sprung_ans_ziel(d) {
        "ans Ziel gesprungen"
    } else if d.drift_nm > crate::grenzwerte::grenzwerte().abgabe.resume_drift_extreme_nm.wert {
        "Position um mehr als 200 nm versetzt"
    } else if d.fuel_delta_kg > crate::grenzwerte::grenzwerte().abgabe.resume_fuel_jump_impossible_kg.wert && !d.both_grounded {
        "Sprit in der Luft gestiegen"
    } else {
        // Kein Zweig passt: ein Flug, der mit einer aelteren Fassung
        // gespeichert wurde. Lieber ehrlich unbestimmt als eine Ursache
        // behaupten, die nicht zutrifft (Cloud-QS 23.09.2026).
        "Zustand beim Wiederaufnehmen nicht plausibel"
    }
}

/// Physikalisch unmöglicher Sprung beim Wiederaufsetzen: entweder eine
/// extreme Reposition (siehe `RESUME_DRIFT_EXTREME_NM`) oder ein Fuel-
/// Anstieg über `RESUME_FUEL_JUMP_IMPOSSIBLE_KG` (Nachtanken mitten im
/// Flug gibt es nicht — das ist ein Reload/neuer Ladezustand, keine
/// durchgehende Simulation).
fn is_impossible_discontinuity(d: &ResumeDiscontinuity) -> bool {
    let impossible_fuel_jump = d.fuel_delta_kg > crate::grenzwerte::grenzwerte().abgabe.resume_fuel_jump_impossible_kg.wert && !d.both_grounded;
    d.drift_nm > crate::grenzwerte::grenzwerte().abgabe.resume_drift_extreme_nm.wert || impossible_fuel_jump || ist_sprung_ans_ziel(d)
}

#[cfg(test)]
#[path = "tests_lib/resume_discontinuity_tests.rs"]
mod resume_discontinuity_tests;

// v0.13.0 Stream F (LE22-LE26): Pilot-getriggerter Re-Check-Workflow.
// Wenn `resume_position_suspect=true`, kann der Pilot sich erst manuell im
// Sim wieder zur gespeicherten Position bringen und dann via
// `flight_resume_check_position`-Command einen Re-Check auslösen. Wenn
// die aktuelle Drift kleiner als diese Schwelle ist UND keine
// airborne↔on_ground-Inkonsistenz besteht, gilt der Resume als sauber
// (kein untrusted-Stempel) und der Flug läuft normal weiter.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> resume_drift_clean_nm (v2.1.24: 5.0)

// How close (in nautical miles) the aircraft must be to the departure airport
// to start the flight. Generous enough to cover taxi positions and remote
// stands; tight enough to reject "I'm at EDDF instead of EDDP".
// Wert seit v3: bundle/grenzwerte/abgabe.json -> max_start_distance_nm (v2.1.24: 5.0)

// Symmetric to MAX_START_DISTANCE_NM but applied at touchdown / file:
// the pilot has to actually be on (or at) the planned arrival airport,
// not somewhere random. Diverts are still possible via
// `flight_end_manual` with a divert ICAO + reason. EDDP→EDDP plans are
// fine — same airport, same check.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> max_file_distance_nm (v2.1.24: 5.0)

// v0.19.3: how far from the planned field a pilot's own confirmation ("yes,
// this is where I ended up", via the divert banner's override) may relax the
// `not_at_arrival` gate.
//
// The confirmation exists for a real case: an off-field landing a few miles
// short of the destination raises a targetless divert hint, and the pilot then
// picks the planned airport from the override list — correctly, because that IS
// the field he was going to. Without the relaxation he would be told
// `not_at_arrival` with no way through.
//
// But it must not become a bypass. The override list includes the planned
// airport, so two clicks from a cockpit 200 nm away would otherwise produce a
// clean, auto-approved arrival PIREP with nothing recording the distance.
// Beyond this bound the pilot files manually — the path that is audited and
// goes to admin review.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> pilot_confirmed_arrival_max_nm (v2.1.24: 15.0)

// How close to another airport's reference point an aircraft has to be before
// we say it is STANDING ON that airport (and therefore may not confirm the
// planned one — see `standing_on_another_field`).
//
// v0.19.3: deliberately tight, and deliberately not the on-field radius used
// elsewhere. The question here is not "is there an airport nearby" but "is the
// aircraft on it". Measured against the airports table: a 3 nm radius answers
// "yes" for 59 % of plausible off-field spots around a major airport, and even
// counting only landable fields, 36 % at 2 nm — which would have blocked the
// very pilot this relaxation exists for, the one who put it down in a field
// short of his destination and said so. At 1 nm it is 11 %, and those are spots
// that genuinely sit on top of another field.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> standing_on_other_field_nm (v2.1.24: 1.0)

// v0.12.10: `clean_atc_model` ist nach `sim-core` gezogen, damit der
// MSFS-Telemetrie-Adapter den `ATC MODEL` schon bei der Erfassung
// bereinigt (BlackSquare-Caravan-Bug: roher Token `ATCCOM.AC_MODEL
// C208.0.text` landete ungereinigt in `aircraft_icao` → „Type ?" →
// kaputtes phpVMS-Filing). Re-Import hier, damit die bestehenden
// Aufruf-Stellen unverändert weiterlaufen.
use sim_core::{clean_atc_model, normalize_icao_type};
