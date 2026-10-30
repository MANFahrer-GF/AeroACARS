/// v0.16.24: resolve the ICAO of the airport the wheels ACTUALLY touched on,
/// from the touchdown coordinate, for keying the navdata cache.
///
/// THE on-plan invariant (load-bearing): for a non-divert landing the nearest
/// airport to the touchdown IS the planned `arr_airport`, so this returns
/// `arr_airport` unchanged → the downstream cache lookup, NavAirport, runway
/// match, source and geometry are byte-identical to the pre-v0.16.24
/// `arr_airport`-keyed path. We additionally short-circuit to `arr_airport`
/// the moment the nearest result matches it (case-insensitive) so the
/// on-plan path never even looks at a different key.
///
/// On a DIVERT the nearest airport is the actual landing field (e.g. WMKA),
/// not the planned one (WMKK) — we return that so the cached navdata for the
/// real airport (warmed by the alternate fetch or the approach pre-fetch)
/// becomes reachable.
///
/// Tie-break: when two airports are within `CORRELATE_AIRPORT_AMBIGUITY_NM`
/// of each other and one of them is the planned `arr_airport`, prefer the
/// plan — this keeps a touchdown that is geometrically ambiguous between the
/// planned field and a stone's-throw neighbour anchored to the plan.
///
/// Returns the uppercased planned `arr_airport` when no airport is in range
/// (mid-ocean / off-grid strip) — identical to the old behaviour, where the
/// cache lookup on `arr_airport` would simply miss and fall through to
/// OurAirports.
/// Den Platz vorladen, auf den die Landung zuläuft — bei einem Divert ist
/// das nicht `arr_airport`.
///
/// Codex-Abnahme 11.10.2026: Vorgeladen wurde nur das geplante Ziel. Die
/// Bahnkorrelation fragt aber `correlate_airport_icao` am Aufsetzpunkt und
/// las einen Ausweichplatz weiter unter der Flugdaten-Sperre aus der apt.dat
/// — genau das Hängen, das `szenerie_bahn::vorladen` beseitigen soll.
/// Dieselbe Funktion hier, unter 5000 ft AGL, höchstens alle 20 s (sie
/// durchsucht die Flughafenliste).
fn landeplatz_szenerie_vorladen(arr_airport: &str, snap: &SimSnapshot) {
    static ZULETZT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    if snap.on_ground || snap.altitude_agl_ft > 5000.0 {
        return;
    }
    {
        let Ok(mut zuletzt) = ZULETZT.lock() else { return };
        if zuletzt.is_some_and(|t| t.elapsed() < Duration::from_secs(20)) {
            return;
        }
        *zuletzt = Some(std::time::Instant::now());
    }
    szenerie_bahn::vorladen(&correlate_airport_icao(arr_airport, snap.lat, snap.lon));
}

fn correlate_airport_icao(arr_airport: &str, td_lat: f64, td_lon: f64) -> String {
    let planned = arr_airport.trim().to_uppercase();
    // Pull a small candidate set (not just the nearest) so the planned
    // airport can win a near-tie even when a co-located / satellite field's
    // runway-threshold proxy is marginally closer than the planned airport's
    // own nearest threshold. 5 covers any realistic cluster of fields around
    // one touchdown.
    let nearby = runway::find_nearest_airports(
        td_lat,
        td_lon,
        crate::grenzwerte::grenzwerte().ausrollen.correlate_airport_search_radius_nm.wert * 1852.0,
        5,
    );
    let Some(nearest) = nearby.first() else {
        // No airport within range — keep the planned key (cache miss →
        // OurAirports fallback, exactly as before).
        return planned;
    };
    let nearest_icao = nearest.icao.trim().to_uppercase();
    // On-plan fast path: nearest IS the plan → byte-identical to the old
    // `arr_airport`-keyed lookup.
    if nearest_icao == planned {
        return planned;
    }
    // Ambiguity tie-break (on-plan invariant guard): if the planned airport
    // is among the candidates AND sits within the ambiguity margin of the
    // nearest, prefer the plan. This keeps a genuine on-plan landing anchored
    // to `arr_airport` even when threshold-proxy noise made a stone's-throw
    // neighbour the bare nearest — so the result stays byte-identical to the
    // old `arr_airport`-keyed path for non-diverts.
    let planned_in_candidates = nearby
        .iter()
        .find(|na| na.icao.trim().eq_ignore_ascii_case(&planned));
    if let Some(planned_cand) = planned_in_candidates {
        let gap_nm = (planned_cand.distance_m - nearest.distance_m) / 1852.0;
        if gap_nm <= crate::grenzwerte::grenzwerte().ausrollen.correlate_airport_ambiguity_nm.wert {
            return planned;
        }
    }
    // QS-1 vanished-arr guard (v0.16.24): the planned field can be MISSING
    // from OurAirports entirely — a closed/inactive strip (real surface:
    // CYWA) is dropped from the parsed runway table (closed runways are
    // skipped at parse time). When that happens an on-plan landing there
    // finds NO `arr_airport` candidate, and if a DIFFERENT open field sits
    // within the search radius the bare-nearest logic would key the cache on
    // that neighbour's ICAO — wrong. Distinguish this from a genuine divert
    // by whether `arr_airport` is findable in OurAirports at all:
    //   * NOT findable (`airport_position` == None, same parsed table the
    //     candidate set comes from) ⇒ the planned field is simply absent from
    //     our data, the wheels are most plausibly on the planned-but-unlisted
    //     strip → PREFER `arr_airport` so the cache key (and any on-demand
    //     navdata fetch / OurAirports fallback) stays anchored to the plan.
    //   * findable (`airport_position` == Some) but not within the search
    //     radius ⇒ a real, mapped planned field that is far from here ⇒ a
    //     GENUINE divert → fall through and return the actual nearest field.
    // This only ever fires when the planned ICAO is absent from OurAirports,
    // so it cannot touch the 99.5% path where `arr_airport` is present.
    if planned_in_candidates.is_none() && runway::airport_position(&planned).is_none() {
        return planned;
    }
    // Genuine divert — the wheels touched a different field than planned.
    nearest_icao
}

/// v0.16.24: per-tick decision for the approach-phase divert navdata
/// pre-fetch. Pure (no I/O): inspects the live snapshot + the planned
/// `arr_airport`, updates the stability-tracking state in `stats`, and
/// returns `Some(icao)` exactly when a stable, committed approach to a
/// DIFFERENT field has just become eligible for a navdata pre-fetch. The
/// caller fires `spawn_navdata_fetch` for that ICAO (which has its own
/// per-ICAO idempotency guard, so a duplicate is a cheap no-op).
///
/// Why it doesn't hammer the VPS:
///   * Only runs while airborne and descending (`MIN_SINK_FPM`) below
///     `MAX_AGL_FT` — i.e. an actual approach, not cruise.
///   * Only considers a field that is (a) within `MAX_DIST_NM`, (b)
///     genuinely diverged from the plan (`PLANNED_DIVERGE_NM`), and (c)
///     NOT the planned `arr_airport` (whose navdata is already fetched).
///   * Debounced: the same diverged field must stay the nearest candidate
///     for `STABLE_SECS` before we fire — a field merely overflown on a
///     vector resets the timer.
///   * Dedup: once fired for an ICAO we record it in
///     `stats.divert_prefetch_icao` and never re-fire for the same field.
///
/// Returns `None` (and clears the stability timer) whenever the conditions
/// don't hold, so the timer only accrues on a genuinely stable candidate.
fn divert_prefetch_decision(
    stats: &mut FlightStats,
    snap: &SimSnapshot,
    arr_airport: &str,
    now: DateTime<Utc>,
) -> Option<String> {
    // Gate: airborne, low enough, and actually descending.
    let committed_approach = !snap.on_ground
        && snap.altitude_agl_ft <= crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_max_agl_ft.wert
        && (snap.vertical_speed_fpm as f64) <= crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_min_sink_fpm.wert;
    if !committed_approach {
        stats.divert_prefetch_stable_since = None;
        return None;
    }

    let planned = arr_airport.trim().to_uppercase();
    // How far is the planned destination from here? If we're still near it
    // there's no divert to pre-fetch — the planned navdata covers us.
    //
    // v0.19.3: measured against the planned field's nearest runway threshold,
    // the same metric `find_nearest_airports` (below) and `arrival::locate`
    // use. It used to be the runway-layout centroid, which is a different
    // point — up to 1.5 nm away at a big field — fed into the same 5 nm
    // comparison. An unknown ICAO still yields INFINITY (⇒ "diverged"), as
    // before.
    //
    // The memoized `planned_arr_position_cache` that used to sit here (added
    // in v0.16.24 to dodge a 48k-row linear scan per approach tick) is gone:
    // `runway`'s ident index makes the lookup a hash hit plus a handful of
    // rows, so there is no scan left to dodge.
    let dist_to_planned_nmi = runway::distance_to_airport_m(&planned, snap.lat, snap.lon)
        .map(|m| m / 1852.0)
        .unwrap_or(f64::INFINITY);
    if dist_to_planned_nmi < crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_planned_diverge_nm.wert {
        stats.divert_prefetch_stable_since = None;
        return None;
    }

    // Nearest field to the current position, within the committed-approach
    // radius. ICAO-only: the result is a navdata cache key, and navdata is
    // published per ICAO.
    let nearby = runway::find_nearest_icao_airports(
        snap.lat,
        snap.lon,
        crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_max_dist_nm.wert * 1852.0,
        1,
    );
    let Some(nearest) = nearby.into_iter().next() else {
        stats.divert_prefetch_stable_since = None;
        return None;
    };
    let nearest_icao = nearest.icao.trim().to_uppercase();
    // Same airport as planned → not a divert; nothing to pre-fetch.
    if nearest_icao == planned {
        stats.divert_prefetch_stable_since = None;
        return None;
    }

    // Stable candidate? Track since-when this exact field has been the
    // committed nearest. Reset the timer whenever the candidate changes.
    match &stats.divert_prefetch_candidate {
        Some(prev) if *prev == nearest_icao => {}
        _ => {
            stats.divert_prefetch_candidate = Some(nearest_icao.clone());
            stats.divert_prefetch_stable_since = Some(now);
        }
    }
    let stable_since = stats.divert_prefetch_stable_since.get_or_insert(now);
    if (now - *stable_since).num_seconds() < crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_stable_secs.wert {
        return None;
    }

    // Already pre-fetched this field? Idempotent at this layer too.
    if stats.divert_prefetch_icao.as_deref() == Some(nearest_icao.as_str()) {
        return None;
    }
    stats.divert_prefetch_icao = Some(nearest_icao.clone());
    Some(nearest_icao)
}

/// v0.15.24: Runway correlation at the touchdown point — extracted verbatim
/// from the FSM Landing arm (see `stamp_touchdown_metadata` for why).
/// Resolves which runway the wheels actually touched on (Navigraph
/// navdata-cache with transparent OurAirports fallback), stamps
/// `runway_match` / `runway_source` / `runway_nav_cycle` /
/// `runway_nav_geometry` plus the actual threshold-crossing height.
///
/// None when no runway lies within ~3 km of the touchdown coordinate. Sits
/// next to `approach_runway` (from `ATC RUNWAY SELECTED`) — the runway_match
/// value is the authoritative one because it's derived from where the wheels
/// actually touched, not the ATC clearance. Uses the buffered TD-moment
/// lat/lon/heading (same reasoning as the `landing_lat` capture in
/// `stamp_touchdown_metadata`) so the centerline-offset sign and along-track
/// distance reflect the real touchdown point, not where the aircraft has
/// rolled to during the streamer's 5 s edge-detection lag.
///
/// Lock order: caller holds the `flight.stats` lock; this takes
/// `flight.navdata` nested inside it — same order as the pre-extraction FSM
/// arm (stats → navdata), never the reverse anywhere in the codebase.
/// Der Aufsetzpunkt, wie er gespeichert ist — vollstaendig oder gar nicht.
///
/// ⚠ Der Dreier muss KOMPLETT sein. `landing_heading_true_deg` allein
/// nuetzt nichts, und lat/lon ohne Kurs auch nicht: Die Zuordnung
/// braucht alle drei. Ein Teil-Dreier hiesse, mit einem geratenen Wert
/// zu rechnen (QS-Befund 3, erste Runde v1.7.15).
fn gespeicherter_aufsetzpunkt(stats: &FlightStats) -> Option<(f64, f64, f32)> {
    match (
        stats.landing_lat,
        stats.landing_lon,
        stats.landing_heading_true_deg,
    ) {
        (Some(la), Some(lo), Some(hd)) => Some((la, lo, hd)),
        _ => None,
    }
}

/// Die Zuordnung NACHHOLEN — ausschliesslich am gespeicherten
/// Aufsetzpunkt.
///
/// ⚠ Diese Funktion sieht `snap` nicht einmal. Sie KANN nicht auf die
/// aktuelle Lage zurueckfallen, und genau das ist ihr Zweck: Der
/// Nachholpfad laeuft, waehrend das Flugzeug rollt — oder nach einem
/// Durchstarten sogar steigt.
///
/// Der frueheste Entwurf pruefte die Bedingungen in einem Lock und rief
/// dann in einem ZWEITEN. Dazwischen kann der Touchdown-Sampler beim
/// Durchstarten `landing_lat`/`landing_lon` loeschen — der Aufruf fiel
/// dann auf `snap` zurueck und setzte womoeglich noch einen Bericht, der
/// jeden weiteren Versuch sperrt (QS-Befund 3, erste Runde v1.7.15).
///
/// Gibt zurueck, ob wirklich zugeordnet wurde.
fn bahn_am_aufsetzpunkt_nachholen(stats: &mut FlightStats, flight: &ActiveFlight) -> bool {
    let Some((la, lo, hd)) = gespeicherter_aufsetzpunkt(stats) else {
        return false;
    };
    // ⚠ KEIN Simulator von aussen. Siehe `aufsetz_simulator`.
    let Some(simulator) = stats.aufsetz_simulator else {
        return false;
    };
    if !bahnaufloesung_nachholen(
        stats.szenerie_auskunft.is_some(),
        stats.szenerie_auskunft_stand,
        stats.korrelierter_szenerie_stand,
    ) {
        return false;
    }
    let forensik_bahn_vorher = anflug_forensik_bahn_kennung(stats);
    korreliere_bahn(stats, flight, simulator, la, lo, hd, true);
    // Lernpaket AP4 (QS 29.09.2026, Befund 2): Die Szenerie kann Schwelle,
    // Gegenende und Versatz ersetzt haben — dann die Forensik nachziehen.
    if anflug_forensik_bahn_kennung(stats) != forensik_bahn_vorher {
        anflug_forensik_nachziehen(stats, flight);
    }
    true
}

fn correlate_touchdown_runway(
    stats: &mut FlightStats,
    snap: &SimSnapshot,
    flight: &ActiveFlight,
    td_buf_sample: Option<&TelemetrySample>,
) {
    // ⚠ Der AUFSETZPUNKT, nicht die aktuelle Lage.
    //
    // Im Aufsetz-Tick sind `td_buf_sample`/`snap` der Aufsetzmoment;
    // `stamp_touchdown_metadata` laeuft unmittelbar davor und speichert
    // dieselben Werte mit derselben Formel. Der Vorrang fuer die
    // gespeicherten Werte aendert hier also nichts — er macht die
    // Funktion nur wiederholbar.
    //
    // ⚠ Fuer das NACHHOLEN gibt es `bahn_am_aufsetzpunkt_nachholen`, das
    // `snap` gar nicht erst bekommt.
    let (rw_lat, rw_lon, rw_hdg_true) = gespeicherter_aufsetzpunkt(stats).unwrap_or_else(|| {
        td_buf_sample
            .map(|s| (s.lat, s.lon, s.heading_true_deg))
            .unwrap_or((snap.lat, snap.lon, snap.heading_deg_true))
    });
    // Womit gerechnet wurde — das Nachholen rechnet mit demselben.
    stats.aufsetz_simulator = Some(snap.simulator);
    korreliere_bahn(
        stats,
        flight,
        snap.simulator,
        rw_lat,
        rw_lon,
        rw_hdg_true,
        false,
    );
}

/// Fuehrt das alte `landing_touchdown_zone` dem neuen `td_third` nach.
///
/// # Warum es das gibt
///
/// Dasselbe Drittel wurde an zwei Stellen gerechnet: roh aus Bahnlaenge
/// und Aufsetzdistanz, und in `assess_touchdown` mit Beruecksichtigung
/// der versetzten Schwelle. Zwei Webapp-Karten lesen weiter die alte
/// Zahl.
///
/// Aendert eine spaet eingetroffene Szenerie die Bahnlaenge, korrigierte
/// das Nachholen nur die neue — und die beiden Karten widersprachen
/// einander, ohne dass eine davon nach einem Fehler aussah.
///
/// ⚠ Ohne Bahntreffer wird das Feld GELEERT, nicht stehengelassen. Ein
/// Drittel ohne Bahn ist eine Behauptung ueber nichts.
fn drittel_nachfuehren(stats: &mut FlightStats) {
    // ⚠ NICHT ueber `classify_tdz`: Das meldet unter 1200 m nutzbarer
    // Laenge `None`, weil es dort keine Aufsetzzonen-MARKIERUNG gibt.
    // Das Drittel ist keine Markierung. Die erste Fassung dieser
    // Funktion haengte das Feld daran und loeschte es damit auf jedem
    // Buschplatz — lautlos, und beide Drittel-Tests hielten die
    // Bahnlaenge konstant (externe QS, 02.09.2026).
    // Die Aufsetzdistanz hinter der Schwelle haengt an derselben Bahn —
    // bis Runde 3 wurde sie nur im Aufsetz-Tick gesetzt und blieb nach
    // einer spaeten Zuordnung stehen (und fuetterte den Vertrauens-Riegel
    // mit einer alten Zahl).
    // ⚠ ROH ab Bahnanfang — absichtlich NICHT schwellenkorrigiert wie das
    // Drittel. `runway_geometry_trust_check` liest dieses Feld an sechs
    // Stellen und ist auf den rohen Wert geeicht (K5S9: −613 m vor der
    // Schwelle → nicht vertrauenswuerdig). Mit dem korrigierten Wert
    // kippte der Riegel bei 4000 ft Versatz ins Negative, und der
    // Divert-Score aenderte sich nach dem Navdaten-Upgrade nicht mehr.
    // Dass die Karte diesen rohen Wert neben dem korrigierten Drittel
    // zeigt (Runde 4, N16), ist dort zu loesen: `td_distance_from_
    // threshold_m` ist der passende Nachbar.
    stats.landing_float_distance_m = stats
        .runway_match
        .as_ref()
        .map(|rw| rw.touchdown_distance_from_threshold_ft as f32 * 0.3048);
    stats.landing_touchdown_zone = stats.runway_match.as_ref().and_then(|rm| {
        let m = bahnmasse(rm);
        // ⚠ Ohne Laenge kein Drittel. OurAirports-Zeilen ohne Laenge
        // ergeben `length_ft == 0` und kommen durch den Trefferfilter;
        // `drittel()` liefert dann 3 — eine Behauptung ueber nichts
        // (externe QS, 02.09.2026, N5).
        (m.nutzbare_laenge_m > 0.0).then(|| runway_assessment::drittel(m.td_m, m.nutzbare_laenge_m))
    });
}

/// Die Masse der Bahn, wie die Bewertung sie sieht.
struct Bahnmasse {
    /// Aufsetzdistanz ab dem physischen Bahnanfang, in Metern.
    td_raw_m: f64,
    /// Der Anteil der versetzten Schwelle, der NICHT schon in der
    /// Geometrie steckt, in Fuss.
    abzuziehen_ft: i32,
    /// Aufsetzdistanz ab der LANDE-Schwelle, in Metern.
    td_m: f64,
    /// Nutzbare Laenge ab der Lande-Schwelle, in Metern.
    nutzbare_laenge_m: f64,
}

/// Eine Rechnung fuer Aufsetzdistanz und nutzbare Laenge.
///
/// `touchdown_distance_from_threshold_ft` (aus `runway::lookup_runway`/
/// `lookup_runway_in_nav`) misst ab dem *physischen* Bahnanfang, nicht ab
/// der Lande-Schwelle — siehe `NavRunway::threshold` und
/// `runway_assessment::classify_displaced`. Bei versetzter Schwelle liegt
/// die legale Schwelle `displaced_threshold_m` weiter hinten. Das ist
/// die EINE Stelle, an der das abgezogen wird; jeder Klassifizierer
/// bekommt die korrigierte Distanz, ausser `classify_displaced`, das den
/// rohen Wert braucht.
///
/// v0.19.x: `RunwayMatch::displaced_threshold_ft` kommt aus BEIDEN
/// Quellen (Navigraph und OurAirports), die Korrektur greift also
/// einheitlich. v1.6.8-QS4: nur der Anteil, der nicht schon in der
/// Geometrie steckt (`displacement_not_in_geometry_ft`).
///
/// v1.6.8: NUTZBARE Laenge, nicht die volle Bahn. Aufsetzzone und
/// Aim-Point messen ausdruecklich ab der Lande-Schwelle; mit der vollen
/// Bahn war die Zone auf versetzten Bahnen zu lang (LOWS 15: 900 statt
/// 851 m) und der Aim-Point kippte an der 2400-m-Grenze zu frueh.
/// `effective_displaced_threshold_ft` statt des rohen Feldes, weil der
/// Versatz seit AIRAC 2608 in der Geometrie steckt.
fn bahnmasse(rm: &runway::RunwayMatch) -> Bahnmasse {
    const FT_PER_M: f64 = 3.280_839_895;
    let td_raw_m = rm.touchdown_distance_from_threshold_ft / FT_PER_M;
    let abzuziehen_ft = displacement_not_in_geometry_ft(rm);
    let td_m = td_raw_m - abzuziehen_ft as f64 / FT_PER_M;
    let length_m = (rm.length_ft as f64) / FT_PER_M;
    let nutzbare_laenge_m = length_m - (effective_displaced_threshold_ft(rm) as f64) / FT_PER_M;
    Bahnmasse {
        td_raw_m,
        abzuziehen_ft,
        td_m,
        nutzbare_laenge_m,
    }
}

/// Aufsetzpunkt fuer PIREP-Feld und Notiz: ab der Lande-Schwelle wie die
/// Bewertung (CES 442, VHHH 07L: 606 m ab Bahnanfang, 432 m ab Schwelle).
/// Vor der Schwelle steht das ausdruecklich da statt eines Minuszeichens.
/// Liefert (Aufsetzpunkt, Bahnlaenge); bei versetzter Schwelle nennt die
/// Laenge die LDA, auf die sich der Aufsetzpunkt bezieht.
fn aufsetzpunkt_texte(rm: &runway::RunwayMatch) -> (String, String) {
    let m = bahnmasse(rm);
    // `+ 0.0` macht aus einer gerundeten -0 eine 0 (sonst "-0 m").
    let td = m.td_m.round() + 0.0;
    let punkt = if td < 0.0 {
        format!("{:.0} m before threshold", -td)
    } else {
        format!("{td:.0} m")
    };
    let laenge_m = rm.length_ft as f64 * 0.3048;
    let laenge = if (laenge_m - m.nutzbare_laenge_m).abs() >= 1.0 {
        format!("LDA {:.0} m of {laenge_m:.0} m", m.nutzbare_laenge_m)
    } else {
        format!("runway {laenge_m:.0} m long")
    };
    (punkt, laenge)
}

/// Schreibt den Flug auf die Platte, BEVOR eine Revision hinausgeht.
///
/// # Warum das nicht der normale Speichertakt erledigt
///
/// `save_active_flight` laeuft alle fuenf Positionstakte — je nach
/// Takt zwischen einer halben und zweieinhalb Minuten. Das Ereignis geht
/// sofort raus.
///
/// Damit kann der Recorder Revision 3 fuehren, waehrend auf der Platte
/// noch 1 steht. Stuerzt die App dazwischen ab, faengt der fortgesetzte
/// Flug bei 1 an, die naechste Korrektur wird 2 — und der Riegel im
/// Recorder weist sie ab, weil er 3 kennt. Die Korrektur waere
/// verloren, und der Riegel haette genau das getan, was in ihm steht.
///
/// Die Regel lautet deshalb: auf der Platte steht mindestens die
/// Revision, die schon auf der Leitung war — SOWEIT die Platte sich
/// beschreiben laesst. Schlaegt das Schreiben fehl, geht das Ereignis
/// trotzdem hinaus: Es zurueckzuhalten hiesse, die Korrektur sicher zu
/// verlieren, um einen moeglichen Verlust nach einem Absturz zu
/// vermeiden. Der Riegel im Recorder (`<=`) verzeiht genau einen
/// Rueckstand. Der Fehlschlag wird als Fehler geloggt, nicht als
/// Warnung — er ist die einzige Luecke in dieser Zusicherung.
fn revision_vor_versand_sichern(app: &AppHandle, flight: &ActiveFlight) -> bool {
    let ok = save_active_flight(app, flight);
    if !ok {
        let revision = flight.stats.lock().map(|s| s.bahn_revision).unwrap_or(0);
        tracing::error!(
            revision,
            "Revision konnte VOR dem Versand nicht gesichert werden — nach einem \
             Absturz kann der Recorder die naechste Korrektur abweisen"
        );
    }
    ok
}

/// Das Drittel fuer die Leitung — vertrauens-geriegelt, EINE Ableitung.
///
/// `landing_touchdown_zone` wird an `touchdown_complete` nur
/// mitgeschickt, wenn die Bahngeometrie vertrauenswuerdig ist
/// (`runway_geometry_trust_check`: kein 3,5-km-Achsversatz wie GSG303,
/// K5S9 statt OR66). Der Nachtrag muss dieselbe Regel anwenden — sonst
/// setzt er die Spalte, die das erste Ereignis bewusst leer liess.
/// Deshalb steht die Regel hier einmal, und beide Ereignisse rufen sie
/// (externe QS, 02.09.2026, N3).
///
/// Der tatsaechliche Landeplatz wird wie im Touchdown-Payload ueber
/// `arrival::locate` bestimmt, nicht ueber die Bahnzuordnung — die zwei
/// koennen sich bei einem knappen Ausweichplatz unterscheiden, und der
/// Riegel ist auf `locate` geeicht.
fn zone_fuer_leitung(flight: &ActiveFlight, stats: &FlightStats) -> Option<u8> {
    if bahnvertrauen(flight, stats).0 {
        stats.landing_touchdown_zone
    } else {
        None
    }
}

/// Der Vertrauens-Riegel der Bahngeometrie — EINE Ableitung fuer
/// Touchdown-Payload, Nachtrag und das Drittel.
///
/// ⚠ `landing_float_distance_m` geht hier hinein. Es wurde bis Runde 3
/// nur im Aufsetz-Tick gesetzt — auf dem Nachtrags-Weg lief der Riegel
/// damit auf einer alten Eingabe. Seit `drittel_nachfuehren` es mit der
/// Bahn nachfuehrt, ist es an beiden Wegen frisch.
fn bahnvertrauen(flight: &ActiveFlight, stats: &FlightStats) -> (bool, Option<&'static str>) {
    let rwy = stats.runway_match.as_ref();
    let td_actual_icao: Option<String> = match (stats.landing_lat, stats.landing_lon) {
        (Some(la), Some(lo)) => {
            match arrival::locate(&flight.arr_airport, la, lo, stats.planned_arr_ref_pos) {
                arrival::ArrivalSite::AtOtherAirport { icao, .. } => Some(icao),
                _ => None,
            }
        }
        _ => None,
    };
    runway_geometry_trust_check(
        rwy.map(|m| m.airport_ident.as_str()),
        &flight.arr_airport,
        td_actual_icao.as_deref(),
        rwy.map(|m| m.centerline_distance_m as f32),
        stats.landing_float_distance_m,
    )
}

/// Die Bahnachse, gegen die die Rollspur bis eben projiziert war.
struct AlteAchse {
    t_lat: f64,
    t_lon: f64,
    e_lat: f64,
    e_lon: f64,
    nutzbare_laenge_m: f64,
}

/// Die aktuelle Bahnachse, festgehalten BEVOR sie ersetzt wird.
///
/// Zwei Aufrufer: `korreliere_bahn` und `bahn_upgrade_anwenden`. Der
/// zweite fehlte zuerst — das Navdaten-Upgrade beim Einreichen tauschte
/// die Bahn und liess die Spur gegen die alte Achse stehen, und der
/// Nachtrag schickte genau die Mischung nach, gegen die
/// `spur_auf_neue_achse` gebaut ist (externe QS, 02.09.2026, N2).
fn alte_achse_von(stats: &FlightStats) -> Option<AlteAchse> {
    stats.runway_match.as_ref().map(|m| AlteAchse {
        t_lat: m.threshold_lat,
        t_lon: m.threshold_lon,
        e_lat: m.end_lat,
        e_lon: m.end_lon,
        nutzbare_laenge_m: bahnmasse(m).nutzbare_laenge_m,
    })
}

// Wie weit zwei Bahnachsen seitlich auseinanderliegen duerfen, um noch
// als dieselbe zu gelten. Bahnen sind mindestens 30 m breit; eine
// Schwelle, die 5 m neben der alten Achse liegt, ist dieselbe Bahn.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> achse_toleranz_m (v2.1.24: 5.0)

/// Bringt die Rollspur in das Bezugssystem einer NEU zugeordneten Bahn.
///
/// # Warum
///
/// `bahn_spur` speichert keine Koordinaten, sondern Projektionen
/// (laengs, quer) gegen die Bahnachse, die zum Zeitpunkt des Punktes
/// galt. Wird die Bahn spaeter neu zugeordnet — die Szenerie kommt nach
/// dem Aufsetzen, die Schwelle liegt 350 m weiter —, tragen die Punkte
/// davor den alten Nullpunkt und die danach den neuen. Auf die Leitung
/// geht EIN `spur_nullpunkt_versatz_m`. Das Ergebnis saehe plausibel aus
/// und waere falsch: ein Knick mitten in der Spur, Raeum- und Kantenpunkt
/// aus zwei Bezugssystemen (externe QS, 02.09.2026, P1-C).
///
/// Neu projizieren geht nicht: Der Positionspuffer haelt fuenf Sekunden.
/// Was geht:
///
/// * **Dieselbe Achse, verschobener Nullpunkt** — der haeufige Fall
///   (versetzte Schwelle aus der Szenerie): Alle Laengswerte um den
///   Versatz verschieben. Das ist exakt, kein Naeherungswert.
/// * **Andere Achse** (andere Bahn, anderer Kurs): Die Spur ist gegen
///   etwas projiziert, das es nicht mehr gibt. Sie wird VERWORFEN — weg
///   und sichtbar, statt weg und falsch. Die letzten fuenf Sekunden
///   kommen aus dem Puffer im neuen System nach.
fn spur_auf_neue_achse(stats: &mut FlightStats, alt: Option<&AlteAchse>) {
    if stats.bahn_spur.is_empty() {
        return;
    }
    // ⚠ Eine FERTIGE Spur wird nie verworfen — nur eine, die noch
    // aufzeichnet, weil nur dann der Puffer nachfuellen kann.
    //
    // Runde 4 (N13): Das Einreich-Upgrade wechselte auf einem
    // Ausweichflug von OurAirports auf Navigraph, die Achsen lagen mehr
    // als 5 m auseinander, die fertige Spur wurde verworfen — und der
    // Nachtrag meldete `rollout_final: false` an eine Zeile, die laengst
    // endgueltig war. Die Spur misst, wo der Pilot WIRKLICH gerollt ist,
    // gegen die Bahn, die er dabei sah.
    //
    // Runde 5 (N24): Bleibt sie bei Achswechsel stehen, ist sie gegen
    // eine Bahn projiziert, die nicht mehr gilt — neben Marken aus der
    // neuen. Dann wird sie als VERALTET markiert und nicht mehr gesendet
    // (`spur_block`); beim Recorder bleibt der letzte in sich stimmige
    // Stand.
    let fertig = stats.rollout_finalized && !stats.bahn_spur_laeuft;
    let Some(alt) = alt else {
        if fertig {
            stats.bahn_spur_bezug_veraltet = true;
            tracing::warn!("Bahn ohne Vorgaenger, Spur fertig — Spur bleibt, gilt als veraltet");
            return;
        }
        spur_verwerfen(stats);
        return;
    };
    let Some(neu) = stats.runway_match.as_ref() else {
        if fertig {
            stats.bahn_spur_bezug_veraltet = true;
            return;
        }
        spur_verwerfen(stats);
        return;
    };
    let (dt_l, dt_q) = runway::projiziere_auf_bahn(
        alt.t_lat,
        alt.t_lon,
        alt.e_lat,
        alt.e_lon,
        neu.threshold_lat,
        neu.threshold_lon,
    );
    let (de_l, de_q) = runway::projiziere_auf_bahn(
        alt.t_lat,
        alt.t_lon,
        alt.e_lat,
        alt.e_lon,
        neu.end_lat,
        neu.end_lon,
    );
    let gleiche_achse =
        dt_q.abs() <= crate::grenzwerte::grenzwerte().ausrollen.achse_toleranz_m.wert && de_q.abs() <= crate::grenzwerte::grenzwerte().ausrollen.achse_toleranz_m.wert && de_l > dt_l;
    if !gleiche_achse {
        if fertig {
            stats.bahn_spur_bezug_veraltet = true;
            tracing::warn!(
                quer_schwelle_m = dt_q,
                quer_ende_m = de_q,
                "Bahnachse hat gewechselt, Spur ist fertig — Spur bleibt lokal, wird nicht mehr gesendet"
            );
            return;
        }
        tracing::info!(
            quer_schwelle_m = dt_q,
            quer_ende_m = de_q,
            "Bahnachse hat gewechselt — Rollspur verworfen, Puffer fuellt nach"
        );
        spur_verwerfen(stats);
        return;
    }
    let neue_laenge_m = bahnmasse(neu).nutzbare_laenge_m;
    let alte_laenge_m = alt.nutzbare_laenge_m;
    if dt_l.abs() < 0.5
        && dt_q.abs() < 0.05
        && de_q.abs() < 0.05
        && (neue_laenge_m - alte_laenge_m).abs() < 0.5
    {
        return;
    }
    // Laengs: der neue Nullpunkt liegt `dt_l` hinter dem alten.
    // Quer: die neue Achse liegt bei der Schwelle `dt_q` und am Ende
    // `de_q` neben der alten — dazwischen linear. Das ist fuer die
    // zugelassenen 5 m exakt genug (Runde 4, N18).
    let alte_spanne = (de_l - dt_l).max(1.0);
    let quer_bei = |laengs_alt: f64| dt_q + (de_q - dt_q) * ((laengs_alt - dt_l) / alte_spanne);

    // ⚠ Der groesste Querversatz wird NICHT aus der ganzen Spur neu
    // bestimmt. Die Spur laeuft ausdruecklich ueber das Messfenster hinaus
    // — durch die Ausfahrt, bis 80 m neben die Kante. Das Live-Maximum
    // waechst nur bei offenem Fenster. Aus der ganzen Spur gerechnet
    // wurde aus einer sauberen Landung „Rad neben der befestigten
    // Flaeche" (Runde 5, N22, P0). Stattdessen: den Fensterpunkt mit dem
    // groessten Betrag suchen und den GESPEICHERTEN Wert um den
    // Querversatz an dieser Stelle korrigieren.
    if let Some(max_quer) = stats.bahn_max_querversatz_m {
        // ⚠ Runde 6 (Befund 1): Den ALTEN Argmax zu verschieben reicht
        // nicht — nach einer seitlichen Korrektur kann ein Punkt mit
        // umgekehrtem Vorzeichen der groessere werden. Die eigene
        // Vorrichtung zeigte es: gemeldet 0,01 m, Spur bei −4,99 m. Also:
        // ueber die FENSTERPUNKTE im neuen Bezug neu bestimmen (die
        // Ausfahrt bleibt draussen, N22), und den verschobenen Altwert
        // als Untergrenze behalten — die Spur ist auf 10 m ausgeduennt
        // und hat nicht jeden Punkt gesehen, den das Live-Maximum sah.
        let fenster_ende = stats.bahn_fenster_zu_laengs_m.unwrap_or(f64::INFINITY);
        let (stelle, neu_max) = stats
            .bahn_spur
            .iter()
            .filter(|p| (p.0 as f64) <= fenster_ende)
            .map(|p| (p.0 as f64, p.1 as f64 - quer_bei(p.0 as f64)))
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .unwrap_or((0.0, 0.0));
        let alt_verschoben = max_quer - quer_bei(stelle);
        stats.bahn_max_querversatz_m = Some(if neu_max.abs() > alt_verschoben.abs() {
            neu_max
        } else {
            alt_verschoben
        });
    }
    for p in &mut stats.bahn_spur {
        let laengs_alt = p.0 as f64;
        p.0 = (laengs_alt - dt_l) as f32;
        p.1 = (p.1 as f64 - quer_bei(laengs_alt)) as f32;
    }
    for feld in [
        &mut stats.bahn_raeum_laengs_m,
        &mut stats.bahn_kante_laengs_m,
        &mut stats.bahn_fenster_zu_laengs_m,
    ] {
        if let Some(v) = feld {
            *v -= dt_l;
        }
    }
    // Ein noch unbestaetigter Kandidat traegt Werte im alten Bezug (Stelle,
    // Querversatz bis dorthin). Er faellt weg; ein bestaetigter steckt
    // schon in den Feldern oben und ist mitverschoben.
    stats.bahn_ausfahrt_kandidat = None;
    if let Some(ueber) = stats.bahn_overrun_m {
        let neu_ueber = ueber - dt_l + (alte_laenge_m - neue_laenge_m);
        stats.bahn_overrun_m = (neu_ueber > 0.0).then_some(neu_ueber);
    }
    tracing::info!(
        versatz_laengs_m = dt_l,
        versatz_quer_m = dt_q,
        punkte = stats.bahn_spur.len(),
        "Rollspur auf die neue Bahnachse verschoben"
    );
}

/// Verwirft die Rollspur samt allem, was aus ihr abgeleitet ist.
///
/// Zwei Aufrufer: das erneute Aufsetzen nach einem Durchstarten (dort
/// gehoert die Spur zur vorigen Landung) und der Achswechsel beim
/// Nachholen (dort ist sie gegen eine Bahn projiziert, die nicht mehr
/// gilt). `bahn_spur_bis` faellt mit — die Ernte beginnt danach wieder
/// ab `landing_at`, findet im Puffer aber nur die letzten fuenf Sekunden
/// (`TOUCHDOWN_BUFFER_SECS`). Deshalb wird eine FERTIGE Spur nie
/// verworfen (siehe `spur_auf_neue_achse`).
fn spur_verwerfen(stats: &mut FlightStats) {
    stats.bahn_max_querversatz_m = None;
    stats.bahn_overrun_m = None;
    stats.bahn_proben = 0;
    stats.bahn_fenster_zu = false;
    // Auch das Fensterende — `get_or_insert` weiter unten schreibt nur,
    // wenn hier None steht. Bliebe der alte Wert stehen, bekaeme die
    // zweite Landung die Messgrenze der ersten, und die Anzeige zeichnete
    // sie an einer Stelle, an der dieses Flugzeug nie gemessen hat.
    stats.bahn_fenster_zu_laengs_m = None;
    stats.bahn_spur.clear();
    stats.bahn_spur_bis = None;
    stats.bahn_spur_laeuft = false;
    stats.bahn_spur_bezug_veraltet = false;
    stats.bahn_raeum_laengs_m = None;
    stats.bahn_raeum_gs_kt = None;
    stats.bahn_raeum_seite = None;
    stats.bahn_kante_laengs_m = None;
    stats.bahn_kante_gs_kt = None;
    stats.bahn_raeum_kurs_diff = None;
    stats.bahn_ausfahrt_kandidat = None;
}

/// Der Kern: ordnet die Bahn an einer GEGEBENEN Position zu.
fn korreliere_bahn(
    stats: &mut FlightStats,
    flight: &ActiveFlight,
    simulator: sim_core::Simulator,
    rw_lat: f64,
    rw_lon: f64,
    rw_hdg_true: f32,
    // `true` NUR beim spaeten Nachholen derselben Landung
    // (`bahn_am_aufsetzpunkt_nachholen`): dort darf ein beim Aufsetzen
    // gemessener TCH-Wert stehen bleiben. Beim ersten Korrelieren eines
    // Aufsetzens (auch Touch-and-Go, Durchstart-Landung) gilt nur, was DIESES
    // Aufsetzen ergibt — sonst bekaeme die zweite Landung den Wert der ersten.
    nachholen: bool,
) {
    // ⚠ Die BISHERIGE Bahnachse, bevor sie ersetzt wird — die Rollspur
    // ist gegen sie projiziert. Siehe `spur_auf_neue_achse`.
    let alte_achse = alte_achse_von(stats);
    // Fuer TCH weiter unten: War es schon DIESE Bahn, bleibt ein einmal
    // gemessener Wert stehen, auch wenn das Nachholen die Spur nicht mehr hat.
    let alte_bahn = stats
        .runway_match
        .as_ref()
        .map(|m| (m.airport_ident.clone(), m.runway_ident.clone()));
    // v0.16.24: Navdata-Cache gegen den ECHTEN Landeflughafen befragen,
    // nicht gegen den geplanten `arr_airport`. Bei einem On-Plan-Landing
    // ist der nächste Airport zum Touchdown == `arr_airport`, also bleibt
    // `actual_icao` == `arr_airport` und der Cache-Lookup ist
    // BYTE-IDENTISCH zum vor-v0.16.24-Pfad (siehe `correlate_airport_icao`
    // — die On-Plan-Invariante). Bei einem DIVERT ist `actual_icao` das
    // tatsächliche Landefeld (z.B. WMKA statt geplant WMKK), womit dessen
    // — vom Approach-Pre-Fetch oder Alternate-Fetch — gecachte Navdata
    // erreichbar wird. `lookup_runway_with_fallback` fällt weiterhin
    // transparent auf OurAirports zurück, wenn kein NavAirport vorhanden
    // ist oder die NavRunways zu weit vom Touchdown liegen.
    let actual_icao = correlate_airport_icao(&flight.arr_airport, rw_lat, rw_lon);
    let (nav_opt, nav_cycle) = {
        let cache = flight.navdata.lock().expect("navdata lock");
        let apt = cache.get(&actual_icao).cloned();
        let cycle = cache.cycle.clone();
        (apt, cycle)
    };
    // Record the actual landing airport for the on-demand fetch + the
    // finalize-with-best re-correlation + diagnostics.
    stats.runway_correlation_icao = Some(actual_icao.clone());

    // v1.7.8: Die Bahngeometrie aus der Szenerie des Piloten, wenn er
    // X-Plane fliegt. Ergaenzt nur Kurs, Breite, Laenge, Schwellen und
    // Belag — ILS, Gleitwinkel und Schwellenhoehe bleiben aus den
    // Navdaten, die kennt die `apt.dat` nicht.
    //
    // Warum ueberhaupt: Am 28.08.2026 gegen die installierte Szenerie
    // gemessen, 3.836 Bahnen des neuesten Zyklus fuehren `true_course`
    // als 0,0 oder 360,0 — bei 3.329 davon widerspricht das der eigenen
    // Bahnnummer. Dort messen wir gegen eine Achse, die es im Simulator
    // nicht gibt.
    // DIESELBEN zwei Quellen wie bei den Ausfahrten.
    //
    // Die Korrektur vom 20.09.2026 war zunaechst nur halb gezogen: Die
    // Ausfahrten nahmen die Auskunft des geplanten Ziels als zweite
    // Quelle, die GEOMETRIE nicht. Im DLH-373-Fall stand dort weiter
    // EDML — Achse und Schwelle fielen also auf die Navdaten zurueck,
    // waehrend die Ausfahrten aus der Szenerie kamen. Zwei Stellen
    // haetten Verschiedenes ueber dieselbe Tatsache gesagt, und
    // `szenerie_status` haette den Szenerie-Weg als unbenutzt gemeldet
    // (externe Abnahme, 21.09.2026).
    //
    // `ergaenze_aus_szenerie` filtert selbst nach ICAO — die Sicherung
    // bleibt also an beiden Quellen.
    let auskunft_fuer_geometrie = stats
        .szenerie_auskunft
        .clone()
        .filter(|a| a.icao.eq_ignore_ascii_case(&actual_icao))
        .or_else(|| {
            stats
                .ziel_szenerie_auskunft
                .clone()
                .filter(|a| a.icao.eq_ignore_ascii_case(&actual_icao))
        });
    // Score-Version 25: der Navigraph-Stand VOR der Szenerie — ILS-Bezug.
    let nav_vor_szenerie = nav_opt.clone();
    let (nav_opt, szenerie_bericht, benutzte_szenerie) = szenerie_bahn::ergaenze_aus_szenerie(
        simulator,
        &actual_icao,
        nav_opt,
        auskunft_fuer_geometrie,
    );
    stats.szenerie_uebernahme = szenerie_bericht;
    // ⚠ Festhalten, MIT WELCHEM Stand zugeordnet wurde.
    //
    // Ohne diese Zeile liefe das Nachholen bei jedem Durchlauf erneut —
    // und mit einem blossen `bool` bliebe ein Bericht aus Lieferung A
    // stehen, obwohl die Ernte laengst Lieferung B angenommen hat.
    stats.korrelierter_szenerie_stand = stats.szenerie_auskunft_stand;
    // Jede Zuordnung ist eine neue Fassung der Bahnwerte. Die Zahl geht
    // auf die Leitung und entscheidet dort, welcher Nachtrag gewinnt.
    stats.bahn_revision = stats.bahn_revision.saturating_add(1);
    // Wer die Bahn aendert, setzt die Fahne — auch die Hauptstelle.
    // Ohne diese Zeile blieb eine nachgeholte Zuordnung ohne Nachtrag,
    // sobald der Streamer nie sendet (leere Spur, weil beim Aufsetzen
    // kein Treffer war) und das Einreich-Upgrade ein Nichts ist
    // (externe QS, 02.09.2026, P1-B).
    stats.bahn_nachtrag_offen = true;
    // Bei X-Plane wird die Szenerie hier erst gelesen; ohne diese Zeile
    // kaemen ihre Rollwege nie bei den Ausfahrten an — die Bahn waere
    // korrigiert, die Ausfahrten kaemen weiter aus OpenStreetMap.
    if let Some(sz) = benutzte_szenerie {
        stats.szenerie_auskunft = Some(sz);
    }

    let lookup_result =
        runway::lookup_runway_with_fallback(rw_lat, rw_lon, rw_hdg_true, nav_opt.as_ref());
    if let Some((rw, source)) = lookup_result {
        // Wenn der Match aus Navigraph kam, finde die
        // konkrete NavRunway-Geometrie (designator-Match)
        // damit die Assessment-Funktionen (TDZ/TCH/DDS)
        // den displaced_threshold_ft, tch_ft und
        // glideslope_angle ohne zweite Lookup-Runde
        // bekommen. Bei OurAirports-Fallback bleibt
        // runway_nav_geometry = None und die Assessment-
        // Funktionen werden skipped.
        let nav_geometry = if matches!(source, runway::RunwaySource::Navigraph) {
            nav_opt.as_ref().and_then(|apt| {
                apt.runways
                    .iter()
                    .find(|nr| nr.designator == rw.runway_ident)
                    .cloned()
            })
        } else {
            None
        };
        // v0.16.24: remember whether we fell back to OurAirports at the
        // touchdown instant, so the caller can fire an on-demand navdata
        // fetch for the actual landing airport and `build_landing_record`
        // can re-correlate against the now-warm cache (finalize-with-best).
        let is_fallback = matches!(source, runway::RunwaySource::OurAirportsFallback);
        stats.runway_fallback_at_touchdown = is_fallback;
        // v0.16.24: enriched diagnostics on the fallback. The actual
        // landing ICAO, whether its navdata was cached (cache-miss vs.
        // present-but-too-far), and the AIRAC cycle — so a future fallback
        // is diagnosable from the breadcrumb alone instead of "match came
        // from OurAirports, no idea why".
        if is_fallback {
            let cached = nav_opt.is_some();
            let reason = if cached {
                // NavAirport WAS cached but its runways were all too far
                // from the touchdown — pilot landed at a field we have
                // navdata for, just not on a runway it lists (rare; stale
                // geometry or a closed runway).
                "navdata_cached_but_no_runway_within_range"
            } else {
                // The actual airport's navdata was never fetched / not in
                // the active cycle — the common divert case.
                "navdata_cache_miss"
            };
            tracing::warn!(
                actual_airport = %actual_icao,
                planned_airport = %flight.arr_airport,
                divert = actual_icao != flight.arr_airport.trim().to_uppercase(),
                navdata_cached = cached,
                fallback_reason = reason,
                airac_cycle = ?nav_cycle,
                runway = %rw.runway_ident,
                "runway correlation fell back to OurAirports"
            );
        }
        tracing::info!(
            airport = %rw.airport_ident,
            runway = %rw.runway_ident,
            centerline_m = rw.centerline_distance_m,
            from_threshold_ft = rw.touchdown_distance_from_threshold_ft,
            side = %rw.side,
            source = ?source,
            cycle = ?nav_cycle,
            "touchdown correlated to runway"
        );
        let rw_bezeichner = rw.runway_ident.clone();
        stats.runway_match = Some(rw);
        stats.runway_source = Some(source);
        // ⚠ VOR dem Nachtragen: Die vorhandene Spur in das Bezugssystem
        // der neuen Bahn bringen — sonst tragen die Punkte davor den
        // alten Nullpunkt und die danach den neuen.
        spur_auf_neue_achse(stats, alte_achse.as_ref());
        // Die Sekunden zwischen Aufsetzen und Bestaetigung nachtragen —
        // sonst fehlt der Spur genau die Aufsetzzone. Siehe
        // `spur_aus_puffer_nachtragen`.
        let halbe_breite_m = stats
            .runway_match
            .as_ref()
            .map(|m| (m.width_ft as f64 * 0.3048 / 2.0).max(15.0))
            .unwrap_or(15.0);
        spur_aus_puffer_nachtragen(stats, halbe_breite_m);
        stats.runway_nav_cycle = if matches!(source, runway::RunwaySource::Navigraph) {
            nav_cycle
        } else {
            None
        };
        stats.runway_ils_navigraph = if matches!(source, runway::RunwaySource::Navigraph) {
            nav_vor_szenerie
                .as_ref()
                .and_then(|apt| IlsNavigraph::aus(apt, &rw_bezeichner))
        } else {
            None
        };
        stats.runway_nav_geometry = nav_geometry;
    } else {
        // No runway within range at all — not a navdata fallback, just no
        // match (mid-ocean / off-grid). Clear the fallback flag too.
        stats.runway_fallback_at_touchdown = false;
        stats.runway_match = None;
        stats.runway_source = None;
        stats.runway_nav_cycle = None;
        stats.runway_nav_geometry = None;
        stats.runway_ils_navigraph = None;
        // Eine Spur ohne Bahn ist gegen nichts projiziert.
        spur_auf_neue_achse(stats, alte_achse.as_ref());
    }

    // v0.8.0 F5 — TCH (Threshold-Crossing-Height) Actual: siehe
    // `tch_actual_from_buffer`.
    // Erst der 50-Hz-Puffer (genau, aber nur 5 s — reicht bei Aufsetzern kurz
    // hinter der Schwelle), sonst die langsame Schwellenspur (Inventur B1).
    let tch_vorher = stats.runway_tch_actual_ft;
    stats.runway_tch_actual_ft = stats.runway_nav_geometry.as_ref().and_then(|geom| {
        tch_actual_from_buffer(geom, &stats.snapshot_buffer)
            .or_else(|| tch_interpoliert(geom, &stats.schwellen_spur))
    });
    // Das Nachholen der Bahn (spaete Szenerie) rechnet oft MINUTEN nach dem
    // Aufsetzen — die 60-s-Spur ist dann leer. Ein beim Aufsetzen gemessener
    // Wert der SELBEN Bahn bleibt deshalb stehen, statt zu None zu werden.
    let neue_bahn = stats
        .runway_match
        .as_ref()
        .map(|m| (m.airport_ident.clone(), m.runway_ident.clone()));
    stats.runway_tch_actual_ft = tch_nach_korrelation(
        stats.runway_tch_actual_ft,
        tch_vorher,
        &alte_bahn,
        &neue_bahn,
        nachholen,
    );

    // ⚠ GANZ zum Schluss. `drittel_nachfuehren` liest den Bahntreffer,
    // der weiter oben erst entsteht — davor gerufen setzt es das Feld
    // aus der VORIGEN Bahn oder loescht es.
    drittel_nachfuehren(stats);
    // Den Status festhalten, solange die Auskunft noch da ist (N26).
    if stats.szenerie_auskunft.is_some() {
        stats.szenerie_status_fest = Some(szenerie_status(stats));
    }
}

// Schwellenspur: nur im Anflug aufzeichnen (darueber ist die Schwelle weit).
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> schwellenspur_max_agl_ft (v2.1.24: 600.0)
// Abstand der Proben (~2,5 Hz; bei 70 m/s ≈ 28 m — die Hoehe an der Schwelle
// wird zwischen zwei Proben interpoliert, nicht abgelesen).
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> schwellenspur_takt_ms (v2.1.24: 400)
// Wie weit zurueck die Spur reicht — traegt auch langsame GA-Anfluege
// (Schwelle → Aufsetzen bis ~25 s) und einen Hopser.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> schwellenspur_secs (v2.1.24: 60)

/// Fuehrt die Schwellenspur: nimmt die aktuelle Probe auf (wenn Anflug und
/// Takt), und schneidet alles ab, was aelter als `SCHWELLENSPUR_SECS` ist.
/// Das Abschneiden laeuft IMMER — sonst blieben die Proben nach dem
/// Aufsetzen stehen und wuerden eine spaetere Landung (Touch-and-Go)
/// verfaelschen.
fn schwellenspur_fuehren(stats: &mut FlightStats, probe: TelemetrySample) {
    let im_anflug = !probe.on_ground && (probe.agl_ft as f64) < crate::grenzwerte::grenzwerte().ausrollen.schwellenspur_max_agl_ft.wert;
    let takt_ok = stats.schwellen_spur.back().map_or(true, |l| {
        (probe.at - l.at).num_milliseconds() >= crate::grenzwerte::grenzwerte().ausrollen.schwellenspur_takt_ms.wert
    });
    if im_anflug && takt_ok {
        stats.schwellen_spur.push_back(probe);
    }
    while stats
        .schwellen_spur
        .front()
        .is_some_and(|f| (probe.at - f.at).num_seconds() > crate::grenzwerte::grenzwerte().ausrollen.schwellenspur_secs.wert)
    {
        stats.schwellen_spur.pop_front();
    }
}

// Groesster Abstand zweier Proben (Laengsmaß), ueber den noch interpoliert
// wird. Bei ~2,5 Hz und 70 m/s liegen Proben ~28 m auseinander; eine Luecke
// (Sim-Pause, Aufzeichnungsloch) ueber 150 m gibt keine belastbare Hoehe.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> tch_max_klammer_m (v2.1.24: 150.0)
/// Plausibler Bereich fuer eine Ueberflughoehe an der Schwelle. Leicht
/// negativ ist erlaubt: die Hoehe ueber Grund schwankt um die Nullmarke
/// (der 5-s-Puffer nimmt sie ebenfalls ungeprueft).
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> tch_plausibel_min_ft/max_ft (v2.1.24: -5.0..=300.0)

/// Entscheidet, welcher TCH-Wert nach dem Korrelieren gilt.
///
/// * Neu gemessen → der neue Wert.
/// * Nichts neu gemessen: nur beim NACHHOLEN derselben Landung auf derselben
///   Bahn bleibt der beim Aufsetzen gemessene Wert stehen (die 60-s-Spur ist
///   Minuten spaeter leer). Bei einem frischen Aufsetzen — Touch-and-Go,
///   Durchstart-Landung — nie: Sonst bekaeme die zweite Landung den Wert der
///   ersten (QS-Review 02.10.2026).
fn tch_nach_korrelation(
    neu: Option<f32>,
    vorher: Option<f32>,
    alte_bahn: &Option<(String, String)>,
    neue_bahn: &Option<(String, String)>,
    nachholen: bool,
) -> Option<f32> {
    neu.or_else(|| {
        (nachholen && alte_bahn.is_some() && alte_bahn == neue_bahn)
            .then_some(vorher)
            .flatten()
    })
}

/// TCH aus der langsamen Spur: wie `tch_actual_from_buffer`, aber die Hoehe
/// wird zwischen der letzten Probe VOR und der ersten HINTER der Schwelle
/// linear nach der Laengsentfernung interpoliert.
///
/// Gilt der LETZTE Schwellenuebergang der Spur: nach einem Tiefanflug oder
/// Durchstart (Spur reicht 60 s zurueck) zaehlt der Ueberflug der Landung,
/// nicht der aelteste. `None` ohne Probe vor der Schwelle, ohne Probe dahinter,
/// bei einer Luecke ueber `TCH_MAX_KLAMMER_M` oder einem unplausiblen Wert.
fn tch_interpoliert(
    geom: &aeroacars_mqtt::navdata::NavRunway,
    buffer: &std::collections::VecDeque<TelemetrySample>,
) -> Option<f32> {
    let mut vorher: Option<(f64, f32)> = None;
    let mut letzter: Option<f32> = None;
    for s in buffer.iter() {
        let along = runway::along_track_m_signed(
            geom.threshold.lat,
            geom.threshold.lon,
            geom.far_end.lat,
            geom.far_end.lon,
            s.lat,
            s.lon,
        );
        if along < 0.0 {
            vorher = Some((along, s.agl_ft));
            // Wieder vor der Schwelle: ein frueherer Uebergang gilt nicht mehr.
            letzter = None;
            continue;
        }
        if let Some((a0, h0)) = vorher.take() {
            let spanne = along - a0;
            if spanne > 0.0 && spanne <= crate::grenzwerte::grenzwerte().ausrollen.tch_max_klammer_m.wert {
                let anteil = (-a0 / spanne).clamp(0.0, 1.0);
                letzter = Some(h0 + (s.agl_ft - h0) * anteil as f32);
            } else {
                letzter = None;
            }
        }
    }
    let g = &crate::grenzwerte::grenzwerte().ausrollen;
    letzter.filter(|h| h.is_finite() && (g.tch_plausibel_min_ft.wert..=g.tch_plausibel_max_ft.wert).contains(h))
}

/// v0.8.0 F5 (Threshold-Crossing-Height Actual): scan the buffer
/// chronologically and take the first sample whose along-track distance
/// from the landing threshold is ≥ 0 (= the aircraft has just crossed
/// the threshold line). That sample's `agl_ft` is the actual TCH.
///
/// v0.19.x FIX: this used to accept that first past-threshold sample
/// UNCONDITIONALLY — including when the buffer's very first entry was
/// already past the threshold (late engagement / buffer-cadence
/// mismatch, e.g. AeroACARS started recording only after the aircraft
/// was already close-in). That directly contradicts this field's own
/// doc comment (`FlightStats::runway_tch_actual_ft`), which documents
/// "no pre-threshold sample in the buffer" as a `None` case — the code
/// just never implemented that guard. A late-engaged flight could get a
/// bogus "TCH" measured well past the threshold (much lower AGL than a
/// real 40-60 ft threshold-crossing height) reported as if it were a
/// reliable measurement. Now requires having actually OBSERVED a
/// pre-threshold sample first, so the accepted sample genuinely brackets
/// the crossing instead of being an arbitrary already-past-threshold one.
fn tch_actual_from_buffer(
    geom: &aeroacars_mqtt::navdata::NavRunway,
    buffer: &std::collections::VecDeque<TelemetrySample>,
) -> Option<f32> {
    let thr_lat = geom.threshold.lat;
    let thr_lon = geom.threshold.lon;
    let end_lat = geom.far_end.lat;
    let end_lon = geom.far_end.lon;
    let mut saw_pre_threshold = false;
    for s in buffer.iter() {
        let along = runway::along_track_m_signed(thr_lat, thr_lon, end_lat, end_lon, s.lat, s.lon);
        if along < 0.0 {
            saw_pre_threshold = true;
            continue;
        }
        return if saw_pre_threshold {
            Some(s.agl_ft)
        } else {
            None
        };
    }
    None
}

/// v0.16.24: the runway-correlation fields, resolved at record-build time
/// (finalize-with-best). Mirrors the four `stats.runway_*` fields so
/// `build_landing_record` can use a possibly-UPGRADED set without mutating
/// `stats`.
struct FinalizedRunwayCorrelation {
    runway_match: Option<runway::RunwayMatch>,
    runway_source: Option<runway::RunwaySource>,
    runway_nav_cycle: Option<String>,
    runway_nav_geometry: Option<aeroacars_mqtt::navdata::NavRunway>,
    runway_ils_navigraph: Option<IlsNavigraph>,
    runway_tch_actual_ft: Option<f32>,
}

/// v0.16.24: finalize-with-best runway correlation, run at LandingRecord
/// build time (minutes after touchdown, at/just-before PIREP filing).
///
/// The touchdown-instant correlation (`correlate_touchdown_runway`)
/// produced a PROVISIONAL result. On a divert it may have fallen back to
/// OurAirports because the actual airport's navdata wasn't cached yet. By
/// record-build time the on-demand fetch (`maybe_fetch_actual_landing_navdata`)
/// and/or the approach pre-fetch have (hopefully) warmed the cache. This
/// re-runs the lookup against the now-current cache and RETURNS the better
/// result if Navigraph is now available — otherwise it returns the stamped
/// provisional values verbatim (graceful).
///
/// NON-RACY by construction: this is a synchronous read of whatever the
/// cache holds AT THE MOMENT the record is built — there is no separate
/// async "re-correlate later" path that could race the filing. If the
/// fetch hasn't completed, the cache simply still lacks the airport and we
/// keep the provisional OurAirports result.
///
/// ON-PLAN INVARIANT: for a non-divert landing the stamped source is
/// already Navigraph (the planned airport was fetched at flight start), so
/// the `is_fallback` guard below is false and we return the stamped values
/// UNCHANGED — byte-identical to pre-v0.16.24. We only ever re-run the
/// lookup when the touchdown FELL BACK, never when it already matched
/// Navigraph.
fn finalize_runway_correlation(
    flight: &ActiveFlight,
    stats: &FlightStats,
) -> FinalizedRunwayCorrelation {
    // The provisional (stamped) set — the default we keep unless we can
    // strictly improve it.
    // Score-Version 25: Ein Flug, der aus einem Zustand von VOR Version 25
    // fortgesetzt wurde, hat keinen ILS-Bezug gemerkt (serde-Standard None).
    // Der Navdaten-Cache haelt Navigraph unveraendert — dort nachbauen, sonst
    // faellt die Forensik auf den Szenerie-Versatz zurueck (Codex, 16.10.2026).
    let ils_navigraph = stats.runway_ils_navigraph.clone().or_else(|| {
        if !matches!(stats.runway_source, Some(runway::RunwaySource::Navigraph)) {
            return None;
        }
        let rm = stats.runway_match.as_ref()?;
        let cache = flight.navdata.lock().expect("navdata lock");
        IlsNavigraph::aus(cache.get(&rm.airport_ident)?, &rm.runway_ident)
    });
    let provisional = FinalizedRunwayCorrelation {
        runway_match: stats.runway_match.clone(),
        runway_source: stats.runway_source,
        runway_nav_cycle: stats.runway_nav_cycle.clone(),
        runway_nav_geometry: stats.runway_nav_geometry.clone(),
        runway_ils_navigraph: ils_navigraph,
        runway_tch_actual_ft: stats.runway_tch_actual_ft,
    };

    // Only attempt an upgrade when the touchdown correlation fell back to
    // OurAirports. If it already matched Navigraph (the on-plan case), keep
    // the stamped values verbatim → byte-identical, no re-lookup.
    if !matches!(
        stats.runway_source,
        Some(runway::RunwaySource::OurAirportsFallback)
    ) {
        return provisional;
    }

    // Need the buffered TD-moment coordinates that the touchdown
    // correlation used (stamped into landing_* by `stamp_touchdown_metadata`
    // from the SAME td_buf_sample, so re-correlation uses identical inputs).
    let (Some(rw_lat), Some(rw_lon), Some(rw_hdg_true)) = (
        stats.landing_lat,
        stats.landing_lon,
        stats.landing_heading_true_deg,
    ) else {
        return provisional;
    };

    // Resolve the actual landing airport the same way the touchdown
    // correlation did and look it up in the (now-warm) cache.
    let actual_icao = correlate_airport_icao(&flight.arr_airport, rw_lat, rw_lon);
    let (nav_opt, nav_cycle) = {
        let cache = flight.navdata.lock().expect("navdata lock");
        (cache.get(&actual_icao).cloned(), cache.cycle.clone())
    };
    // No navdata for the actual field yet (fetch not landed / 404) → keep
    // the provisional OurAirports result. Graceful.
    let Some(apt) = nav_opt else {
        return provisional;
    };
    // Re-run the Navigraph-only lookup. If it still misses (pilot landed at
    // a field we have navdata for but on a runway it doesn't list), keep
    // the provisional result — never worse than today.
    let Some(rw) = runway::lookup_runway_in_nav(rw_lat, rw_lon, rw_hdg_true, &apt) else {
        return provisional;
    };

    // We have a strictly-better Navigraph match. Rebuild the consistent
    // field set exactly as `correlate_touchdown_runway` would have, had the
    // cache been warm at touchdown.
    let nav_geometry = apt
        .runways
        .iter()
        .find(|nr| nr.designator == rw.runway_ident)
        .cloned();
    // TCH-actual: deliberately NOT recomputed here. The first-past-threshold
    // scan needs the pre-touchdown samples (~50 ft AGL at the threshold), but
    // by record-build time (minutes after touchdown) the rolling 5 s
    // `snapshot_buffer` has long evicted them — it now holds only on-ground /
    // taxi samples, so a re-scan would stamp a garbage ~0 ft "TCH". The
    // provisional value was `None` (the OurAirports fallback gates the TCH
    // scan off), and `None` (honest "couldn't measure") is the correct,
    // never-worse result for the upgrade. TCH-actual is forensic/coaching
    // only and not a scored field, so the upgrade still delivers the
    // LDA/rollout geometry that matters.
    // Hier ohne Szenerie-Uebernahme: die Navigraph-Bahn ist schon der ILS-Bezug.
    let runway_ils_navigraph = IlsNavigraph::aus(&apt, &rw.runway_ident);
    let runway_tch_actual_ft = None;

    tracing::info!(
        actual_airport = %actual_icao,
        runway = %rw.runway_ident,
        cycle = ?nav_cycle,
        "finalize-with-best: upgraded provisional OurAirports runway match to Navigraph"
    );

    FinalizedRunwayCorrelation {
        runway_match: Some(rw),
        runway_source: Some(runway::RunwaySource::Navigraph),
        runway_nav_cycle: nav_cycle,
        runway_nav_geometry: nav_geometry,
        runway_ils_navigraph,
        runway_tch_actual_ft,
    }
}

/// v0.16.24 (QS-6): run `finalize_runway_correlation` and WRITE the (possibly
/// upgraded) result back into `stats`, so every downstream consumer that reads
/// `stats.runway_*` — the phpVMS PIREP native `score`
/// (`compute_aggregate_master_score`), the "Landing Score" custom field
/// (`build_pirep_fields`), the LandingRecord, the TouchdownPayload — sees ONE
/// consistent, best-available runway-correlation set.
///
/// WHY THIS EXISTS (the bug it closes): on a divert where the touchdown-instant
/// correlation fell back to OurAirports (the actual field's navdata wasn't
/// cached yet) the on-demand fetch fires AT touchdown, but PIREP filing happens
/// minutes later (after rollout/taxi/Arrived). Before this helper the PIREP
/// score + body were built from the PROVISIONAL OurAirports stats while only
/// the LandingRecord (built inside `record_landing_for_filed_flight`, which
/// already finalized) saw Navigraph — an inconsistency, and the authoritative
/// scored PIREP record never got the LDA/rollout upgrade. Calling this at the
/// TOP of the body-build stats lock makes the score/fields read the upgraded
/// Navigraph geometry too.
///
/// IDEMPOTENT + ON-PLAN-SAFE: `finalize_runway_correlation` is a no-op whenever
/// the stamped source is already Navigraph (the on-plan case, or a second call
/// after the first upgrade), so a non-divert write-back is byte-identical and
/// the later `record_landing_for_filed_flight` finalize sees the values already
/// upgraded and returns them verbatim. Caller must hold the `stats` mutex
/// mutably.
fn apply_finalized_runway_correlation(flight: &ActiveFlight, stats: &mut FlightStats) {
    bahn_upgrade_anwenden(flight, stats);
}

/// Dasselbe — und sagt, OB sich dabei etwas an der Bahn geaendert hat.
///
/// # Warum das noetig wurde
///
/// Diese Funktion ersetzt beim Einreichen dieselben Bahnfelder wie das
/// Nachholen: Ist die Navdaten-Abfrage eines Ausweichplatzes inzwischen
/// zurueckgekommen, wird die vorlaeufige OurAirports-Geometrie durch die
/// von Navigraph ersetzt.
///
/// Bis dahin lief das an allem vorbei, was diese Version aufgebaut hat:
/// keine neue Revision, kein nachgefuehrtes Drittel, kein Nachtrag zum
/// Recorder. Der Recorder uebernimmt vom PIREP laut Vertrag nur Punkte
/// und Noten — die Touchdown-Zeile waere auf der alten Geometrie
/// stehengeblieben, obwohl der Client die bessere schon hatte.
///
/// ⚠ Die Erkennung vergleicht die WIRE-GRUPPE, nicht einzelne Felder.
/// Genau sie geht an den Recorder; aendert sie sich nicht, gibt es
/// nichts zu melden. Das haelt auch den bestehenden Vertrag ein, dass
/// ein Aufruf ohne neue Navdaten ein bitgleiches Nichts ist — ohne ihn
/// wuerde jede Einreichung die Revision hochzaehlen und einen leeren
/// Nachtrag ausloesen.
fn bahn_upgrade_anwenden(flight: &ActiveFlight, stats: &mut FlightStats) -> bool {
    // `Debug` statt `serde_json`. Der QS-Befund N8 (02.09.2026)
    // vermutete, JSON scheitere an NaN und verdecke damit Aenderungen —
    // nachgemessen: `to_value(NaN)` liefert `null`, kein Fehler, die
    // Erkennung war nie betroffen. `Debug` bleibt trotzdem: Es braucht
    // keine Serialisierung fuer einen Vergleich, und NaN bleibt als
    // Wert sichtbar statt zu `null` zu werden.
    // ⚠ Drittel und Aufsetzdistanz gehoeren zum Vergleich — sie stehen
    // nicht in der Herkunftsgruppe (Spalten, kein JSON), aendern sich
    // aber mit der Bahn. Ohne sie blieb ein wiederhergestelltes Drittel
    // unerkannt: kein Nachtrag, Spalte fuer immer leer (Runde 3, P1).
    // Der ganze Bahntreffer gehoert zum Vergleich: `bahn_felder()` liest
    // Breite und Belag daraus, die in der Herkunftsgruppe nicht stehen.
    // Ein Upgrade, das nur die Breite aendert, blieb sonst ohne Nachtrag
    // (Runde 4, N17).
    let vorher = format!(
        "{:?}|{:?}|{:?}|{:?}",
        bahn_herkunft(stats),
        stats.landing_touchdown_zone,
        stats.landing_float_distance_m,
        stats.runway_match
    );
    let alte_achse = alte_achse_von(stats);
    let forensik_bahn_vorher = anflug_forensik_bahn_kennung(stats);
    let finalized = finalize_runway_correlation(flight, stats);
    stats.runway_match = finalized.runway_match;
    stats.runway_source = finalized.runway_source;
    stats.runway_nav_cycle = finalized.runway_nav_cycle;
    stats.runway_nav_geometry = finalized.runway_nav_geometry;
    stats.runway_ils_navigraph = finalized.runway_ils_navigraph;
    stats.runway_tch_actual_ft = finalized.runway_tch_actual_ft;
    // Die Spur folgt der Bahn — hier genauso wie beim Nachholen.
    spur_auf_neue_achse(stats, alte_achse.as_ref());
    // Das Drittel haengt an der Bahn — hier genauso wie beim Nachholen.
    drittel_nachfuehren(stats);
    // Lernpaket AP4 (QS 29.09.2026, Befund 2): die Forensik folgt der Bahn.
    // Kein Einfluss auf Revision oder Nachtrag. Seit Score-Version 19 aber
    // auf die Note: der Gleitpfad (gesamt.mittel_abs_dots) ist eine Pruefung
    // des Stable Gate. Ein Bahnwechsel nach dem Aufsetzen kann das Urteil im
    // Live-Touchdown (`approach_stable_at_gate`) vom spaeteren PIREP-Urteil
    // abweichen lassen; Record und PIREP rechnen danach und tragen das
    // richtige (QS 05.10.2026).
    if anflug_forensik_bahn_kennung(stats) != forensik_bahn_vorher {
        anflug_forensik_nachziehen(stats, flight);
    }
    let nachher = format!(
        "{:?}|{:?}|{:?}|{:?}",
        bahn_herkunft(stats),
        stats.landing_touchdown_zone,
        stats.landing_float_distance_m,
        stats.runway_match
    );
    if vorher == nachher {
        return false;
    }
    stats.bahn_revision = stats.bahn_revision.saturating_add(1);
    stats.bahn_nachtrag_offen = true;
    true
}

/// Baut den Nachtrag aus dem AKTUELLEN Stand — oder nichts.
///
/// Dieselbe Zusammenstellung wie im Streamer; ohne Aufsetzzeitpunkt gibt
/// es keine Zeile, die er adressieren koennte.
/// Die Aufsetzzeit, die JEDES Ereignis dieser Landung adressiert.
///
/// `touchdown_complete` nimmt `sampler_touchdown_at.or(landing_at)` — der
/// Abtaster setzt seine Zeit im selben Tick, in dem das Aufsetzen
/// bestaetigt wird, `landing_at` kommt bei Busch-/VFR-Landungen erst mit
/// dem `Arrived`-Rueckfall Minuten spaeter oder beim manuellen Einreichen
/// gar nicht. Ein Nachtrag, der nur `landing_at` nimmt, adressiert dann
/// eine Zeile, die es nicht gibt, oder unterbleibt (Codex, 03.09.2026).
fn aufsetzzeit(stats: &FlightStats) -> Option<DateTime<Utc>> {
    stats.sampler_touchdown_at.or(stats.landing_at)
}

/// Der Spur-Block fuer die Leitung — oder nichts.
///
/// EINE Entscheidung fuer Streamer und Einreich-Nachtrag: Der Block geht
/// nur mit, wenn die Spur vollstaendig ist UND noch gegen die aktuelle
/// Bahn projiziert. Sonst fehlt er ganz — nicht `null`, das loescht beim
/// Recorder, und `rollout_final: false` draengt eine endgueltige Zeile
/// zurueck (Runde 4, N13; Runde 5, N24).
fn spur_block(flight: &ActiveFlight, stats: &FlightStats) -> Option<aeroacars_mqtt::BahnWire> {
    let spur_fertig =
        stats.rollout_finalized && !stats.bahn_spur_laeuft && !stats.bahn_spur.is_empty();
    if !spur_fertig || stats.bahn_spur_bezug_veraltet {
        return None;
    }
    Some(
        bahn_felder(
            stats,
            stats
                .aufgeloestes_muster
                .as_deref()
                .or(Some(flight.aircraft_icao.as_str()))
                .filter(|s| !s.is_empty()),
            None,
        )
        .wire(true),
    )
}

fn bahn_nachtrag_bauen(
    flight: &ActiveFlight,
    stats: &FlightStats,
) -> Option<aeroacars_mqtt::TouchdownRolloutFinalizedPayload> {
    let touchdown_at = aufsetzzeit(stats)?.timestamp_millis();
    Some(aeroacars_mqtt::TouchdownRolloutFinalizedPayload {
        ts: Utc::now().timestamp_millis(),
        pirep_id: flight.pirep_id.clone(),
        touchdown_at,
        rollout_distance_m: stats.rollout_distance_m.unwrap_or(0.0),
        finalize_reason: stats.rollout_finalize_reason.clone(),
        bahn: spur_block(flight, stats),
        landing_touchdown_zone: zone_fuer_leitung(flight, stats),
        runway_geometry_trusted: Some(bahnvertrauen(flight, stats).0),
        runway_geometry_reason: bahnvertrauen(flight, stats).1.map(|r| r.to_string()),
        herkunft: bahn_herkunft(stats),
        landing_decel_mps2: stats.landing_decel_mps2,
    })
}

/// Der Rollout-Nachtrag des Streamers — nur, wenn er noch nicht gesendet ist.
///
/// Liefert Sperrschluessel `(touchdown_at, bahn_revision)` und Ereignis,
/// sobald Ausrollen UND Rollspur fertig sind und dieser Schluessel noch
/// nicht `zuletzt` gesendet wurde.
///
/// ⚠ ERST der Schluessel, DANN der Bau. Bis 2.0.12 baute der Streamer
/// Spur-Block, Rollwegkarte und Ausfahrten in JEDEM Takt und verwarf sie
/// danach, weil der Nachtrag laengst gesendet war — bei AIB 429 (HECA,
/// 1480 Rollwege, 12.10.2026) 460–600 ms je Takt, das ganze Rollen lang.
/// Schluessel und Ereignis stammen aus EINEM Blick auf `stats`; der
/// Aufrufer haelt dafuer die Sperre.
fn rollout_nachtrag_faellig(
    flight: &ActiveFlight,
    stats: &FlightStats,
    zuletzt: Option<(i64, u32)>,
) -> Option<((i64, u32), aeroacars_mqtt::TouchdownRolloutFinalizedPayload)> {
    // Erst senden, wenn BEIDES fertig ist.
    //
    // `rollout_finalized` faellt bei vierzig Knoten — die Ausrollstrecke
    // steht dann fest. Die Rollspur laeuft weiter (bis fuenf Knoten, zur
    // Ausfahrt oder ueber den Rand), und mit ihr Raeumpunkt und
    // Kantenuebertritt. Wird zu frueh gesendet, traegt das Ereignis
    // dieselbe abgebrochene Spur wie `touchdown_complete`, nur mit dem
    // Anspruch, final zu sein.
    //
    // `bahn_spur_laeuft` wird von `spur_fortschreiben` gepflegt: true,
    // solange aufgezeichnet wird, false, sobald ein Abbruchkriterium
    // greift. Vor dem ersten Aufruf ist es ebenfalls false — deshalb
    // zusaetzlich die Bedingung, dass ueberhaupt eine Spur vorliegt.
    let spur_fertig = !stats.bahn_spur_laeuft && !stats.bahn_spur.is_empty();
    if !stats.rollout_finalized || !spur_fertig {
        return None;
    }
    // `aufsetzzeit` None (z.B. direkt nach einem T&G) → kein Nachtrag;
    // der naechste Touchdown setzt sie neu.
    //
    // ⚠ Die REVISION der Zuordnung — dieselbe Zahl, die auf die Leitung
    // geht. Frueher stand hier `korrelierter_szenerie_stand`; der aendert
    // sich nur mit einer neuen SZENERIE-Lieferung, eine Zuordnung aus
    // anderem Grund (Navdaten-Upgrade beim Einreichen) haette keinen
    // Nachtrag ausgeloest. Trifft die Szenerie NACH der Finalisierung ein
    // (YSBK, 01.09.2026), muss erneut gesendet werden, sonst bleiben beim
    // Recorder die alten Achsen- und Ausfahrtsdaten stehen (QS-Befund 1,
    // erste Runde v1.7.15). Das Ereignis ist ohnehin idempotent: Es
    // adressiert ueber `pirep_id` + `touchdown_at`.
    let schluessel = (aufsetzzeit(stats)?.timestamp_millis(), stats.bahn_revision);
    if zuletzt == Some(schluessel) {
        return None;
    }
    // Die Bahnwerte in ihrem FINALEN Stand, mit derselben Ableitung wie
    // der Einreich-Nachtrag (`spur_block`, `bahn_herkunft`,
    // `zone_fuer_leitung`, `bahnvertrauen`).
    Some((schluessel, bahn_nachtrag_bauen(flight, stats)?))
}

/// v0.15.24: Landing-weight stamp — extracted verbatim from the tail of the
/// FSM Landing arm. Prefer TOTAL WEIGHT (fuel + payload + empty); only
/// fall back to ZFW + fuel if the SimVar is missing. Fenix returns 0 for
/// both → snapshot mapping converted it to None, so the field stays unset
/// and the PIREP filter drops it.
fn stamp_landing_weight(stats: &mut FlightStats, snap: &SimSnapshot) {
    if let Some(w) = gewicht_aus_snapshot(snap) {
        stats.landing_weight_kg = Some(w);
    }
}

/// Gesamtgewicht nach der Regel von `stamp_landing_weight`.
fn gewicht_aus_snapshot(snap: &SimSnapshot) -> Option<f64> {
    if let Some(tw) = snap.total_weight_kg {
        return Some(tw as f64);
    }
    let weight = snap.zfw_kg.unwrap_or(0.0) as f64 + snap.fuel_total_kg as f64;
    (weight > 0.0).then_some(weight)
}

/// Vref aus dem Flugzeug: PMDG-FMC, sonst was der Adapter als Vref liest
/// (iniBuilds, FSS E-Jets, Zibo), sonst die Airbus-VLS, aber nur in
/// Landestellung — dort ist sie per Definition die Bezugsgröße (VLS CONF
/// FULL = Vref). Quelle `pmdg`, `fbw` für das FBW-Profil, sonst `fmc`.
fn vref_aus_dem_flugzeug(snap: &SimSnapshot) -> Option<(f32, &'static str)> {
    if let Some(v) = snap.pmdg.as_ref().and_then(|p| p.fmc_vref_kt) {
        return Some((v as f32, "pmdg"));
    }
    let landestellung =
        landing_scoring::vref::landeklappen(snap.flap_handle_index, snap.flap_num_positions) == Some(true);
    let v = snap.vref_kt.or(snap.vls_kt.filter(|_| landestellung))?;
    let quelle = if snap.aircraft_profile == sim_core::AircraftProfile::FbwA32nx {
        "fbw"
    } else {
        "fmc"
    };
    Some((v as f32, quelle))
}

// v0.16.6: per-tick rollout accumulate/finalise — extracted VERBATIM from
// the FSM Landing arm in `step_flight` so sampler-path (bush) landings can
// drive it too while the FSM never reaches Landing. The FSM Landing arm
// calls this at the exact position its inline code used to sit (first thing
// in the arm), so FSM-path behaviour is unchanged.
//
// Sums haversine deltas from the last seen position until one of the three
// finalisation triggers fires (~40 kt exit speed / >30° heading turn-off /
// 5 kt full-stop — see the `ROLLOUT_*` consts), then sets
// `rollout_finalized` + `rollout_finalize_reason`. No-op once finalised.
// Survives a Tauri restart because (last_lat, last_lon, distance,
// finalized) persist in `PersistedFlightStats`.
// Kursabweichung, ab der die Ausfahrt als eingeleitet gilt.
//
// **Gemessen, nicht gesetzt.** Drei Auswertungslaeufe ueber den Bestand waren
// noetig: ohne Trennkriterium meldeten 68,7 % der Landungen "Rad neben der
// Bahn", mit 20 Grad Kursabweichung 25,2 %, erst mit 10 Grad plus dem
// Geschwindigkeitsriegel unten 1,6 % — plausibel. Ohne die Riegel faerbt
// jedes normale Ausfahren die Messung ein.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_kurs_ausfahrt_grad (v2.1.24: 10.0)

// Obergrenze fuer eine Ausfahrt, die ueber der Messschwelle beginnt.
//
// **Gemessen** am Bestand vom 09.10.2026 (634 Landungen mit
// Flugprotokoll, 624 Ausfahrten bis 80 m neben die Bahn): Median 28 kt,
// 95 % unter 51 kt, sieben Ausfahrten zwischen 61 und 76 kt (EDDH, VVTS,
// EPWA, EDDF, LIRF, LFPG, LOWW), die schnellste echte bei 87 kt (EFHK
// 04L). Alles darueber waren falsch zugeordnete Aufsetzpunkte, keine
// Ausfahrten. Wer schneller seitlich von der Bahn geht, faehrt nicht ab.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_ausfahrt_max_gs_kt (v2.1.24: 90.0)

// Kursabweichung, ab der eine schnelle Ausfahrt als BEGONNEN gilt.
//
// Nicht `BAHN_KURS_AUSFAHRT_GRAD`: Bis eine Schnellabrollbahn zehn Grad
// Kurs erreicht, ist das Flugzeug schon zehn bis fuenfzehn Meter seitlich
// gerollt — Geometrie der Kurve, kein Abkommen. Bei TUA651 lag die Grenze
// damit an der Bahnkante.
//
// **Gemessen** am Bestand vom 09.10.2026 (33.595 Proben aus 1.293
// Landungen, 2–15 s nach dem Aufsetzen, 70–130 kt): Der Kurs liegt beim
// geraden Ausrollen zu 90 % innerhalb von 1,9 Grad und zu 95 % innerhalb
// von 2,7 Grad der Bahnrichtung. Drei Grad liegen knapp darueber. Ein
// Fehlstart kostet nichts: Bestaetigt wird erst die echte Ausfahrt (zehn
// Grad und `BAHN_SPUR_RAND_M` neben der Kante).
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_ausfahrt_beginn_grad (v2.1.24: 3.0)

/// Eine Ausfahrt, die UEBER der Messschwelle eingeleitet wurde.
///
/// # Warum es das gibt
///
/// Der Raeumpunkt (Schritt 2 in `bahndisziplin_tick`) gilt erst unter der
/// Messschwelle (60 kt). Wer schneller in eine Schnellabrollbahn eindreht,
/// rollt bei offenem Messfenster ueber die Bahnkante — und die Kurve in die
/// Ausfahrt wurde als „Rad neben der Bahn" gewertet. Anlass TUA651
/// (Hamadoun, VVTS 25R, 09.10.2026): bei 64 kt nach links in die
/// Abrollbahn, 42,6 m Versatz, Bahndisziplin 20 Punkte, Gesamtnote 54.
///
/// Ueber 60 kt allein laesst sich Ausfahrt und Ausbrechen nicht trennen:
/// Wer schraeg aufsetzt und sich auf die Achse dreht, weicht vom
/// Aufsetzkurs ebenfalls ab. Deshalb ist es zunaechst nur ein KANDIDAT,
/// angelegt am Beginn der Kurve (`BAHN_AUSFAHRT_BEGINN_GRAD`). Bestaetigt
/// wird er erst, wenn daraus ohne Unterbrechung eine Ausfahrt wird: ueber
/// `BAHN_KURS_AUSFAHRT_GRAD` und `BAHN_SPUR_RAND_M` neben der Kante — so
/// weit geht niemand, der auf die Bahn zurueckkehrt. Dann gilt der Beginn
/// der Kurve als Raeumpunkt, und der Querversatz zaehlt nur bis dorthin.
/// Liegt der Kurs vorher wieder auf der Bahn, verfaellt der Kandidat und
/// alles bleibt, wie es gemessen wurde.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AusfahrtKandidat {
    laengs_m: f64,
    gs_kt: f64,
    /// Groesste Kursabweichung zum Aufsetzkurs seit dem Beginn (mit
    /// Vorzeichen, positiv = rechts).
    kurs_diff: f64,
    /// Der groesste Querversatz VOR der Kurve — der Wert, der bei einem
    /// Raeumpunkt an dieser Stelle gegolten haette.
    max_querversatz_m: Option<f64>,
    proben: u32,
    /// Kantenuebertritt waehrend der Kurve, vorgemerkt von
    /// `spur_fortschreiben` (das Fenster ist dort noch offen).
    kante_laengs_m: Option<f64>,
    kante_gs_kt: Option<f64>,
}

// Untergrenze der seitlichen Messung — Deckel fuer schnelle/normale Muster.
//
// Niemand biegt mit 60 kt ab, und genau dort ist seitliches Abkommen
// gefaehrlich. Unterhalb wird nicht mehr gemessen — das ist der zweite
// Riegel gegen die Verwechslung von Abbiegen und Abkommen.
//
// **Fester Deckel seit v1.7.21, keine feste Schwelle mehr.** Fuenf
// Landungen im Korpus (1096 Landungen, alle je gespeicherten Logs)
// scheiterten an `insufficient_samples` — ausnahmslos leichte einmotorige
// GA-Muster (PA24, C152, AC11, BE24) mit 44-56 kt Aufsetz-Grundgeschwindigkeit.
// Bei denen liegt die GESAMTE Ausrollstrecke unter 60 kt: das Messfenster
// hatte strukturell nie eine Chance, offen zu bleiben.
//
// `bahn_mess_schwelle_kt` (unten) macht die Schwelle relativ zur
// tatsaechlichen Aufsetz-Grundgeschwindigkeit: `clamp(0.6 * aufsetz_gs, 25,
// 60)`. Bei >=100 kt Aufsetz-GS bleibt die Schwelle bei diesem Deckel —
// unveraendertes Verhalten fuer Jets/Airliner (1022 von 1027 normal
// bewerteten Landungen im Korpus). Der unabhaengige Kurswechsel-Detektor
// (`BAHN_KURS_AUSFAHRT_GRAD`) schliesst das Fenster weiterhin zusaetzlich
// bei jedem echten Abbiegen, unabhaengig von der Fahrt — das Absenken der
// Geschwindigkeitsschwelle fuer langsame Muster reisst diese zweite
// Sicherung nicht ein.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_mess_min_gs_kt (v2.1.24: 60.0)

// Boden der relativen Messfenster-Schwelle — echtes Rollen/Rangieren soll
// auch bei sehr langsamen Mustern nicht mehr "gewertet" werden.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_mess_min_gs_kt_boden (v2.1.24: 25.0)

// Faktor der Aufsetz-Grundgeschwindigkeit, mit dem die Fenster-Schwelle
// bei langsamen Mustern skaliert (s. `BAHN_MESS_MIN_GS_KT` oben).
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_mess_schwelle_faktor (v2.1.24: 0.6)

/// Effektive Fensterschluss-Schwelle fuer DIESE Landung: relativ zur
/// Aufsetz-Grundgeschwindigkeit, gedeckelt nach oben (60 kt, unveraendertes
/// Verhalten fuer normale/schnelle Muster) und mit einem harten Boden nach
/// unten (25 kt, gegen Schrittgeschwindigkeit).
///
/// Faellt `landing_groundspeed_kt` aus (sollte im Landing-Pfad nicht
/// vorkommen), faellt dies auf den alten festen Wert zurueck — NICHT auf 0
/// oder einen Fehler.
fn bahn_mess_schwelle_kt(stats: &FlightStats) -> f32 {
    match stats.landing_groundspeed_kt {
        // `> 0.0` schliesst NaN (jeder Vergleich mit NaN ist `false`) UND
        // negative Werte aus einem GS-Glitch mit ein — beide sollen auf den
        // sicheren festen Fallback fallen, nicht auf den 25-kt-Boden geklemmt
        // werden (QS-Fund, Codex 08.09.2026: eine negative endliche GS wurde
        // bisher stillschweigend wie eine echte langsame Landung behandelt).
        Some(aufsetz_gs) if aufsetz_gs.is_finite() && aufsetz_gs > 0.0 => {
            (crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_schwelle_faktor.wert * aufsetz_gs)
                .clamp(crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt_boden.wert, crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt.wert)
        }
        _ => crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt.wert,
    }
}

// Schreibt die seitliche Lage und einen etwaigen Overrun fort.
//
// Laeuft bei jedem Snapshot des Ausrollens. Das Messfenster schliesst sich,
// sobald die Ausfahrt eingeleitet ist — danach wird nichts mehr gewertet, aber
// der Overrun-Check laeuft weiter, weil ein Ueberschiessen des Bahnendes auch
// dann zaehlt.
//
// Siehe `docs/spec/v1.7.0-bahndisziplin.md` §5.2.
// Mindestabstand zwischen zwei gespeicherten Spurpunkten, in Metern.
//
// Zehn Meter sind fein genug, dass die Queransicht bei 3000 m Bahn rund
// dreihundert Stuetzpunkte bekommt — mehr, als ein Diagramm von 1100 px
// Breite aufloesen kann.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_spur_min_abstand_m (v2.1.24: 10.0)

// Wie stark der Querweg bei der Ausduennung zaehlt.
//
// Die Queransicht ist rund zwoelffach ueberhoeht. Ein Gewicht von vier
// naehert das an, ohne es voll auszureizen: Bei reiner Querbewegung wird
// dann alle 2,5 m ein Punkt abgelegt statt alle zehn — fein genug fuer
// eine glatte Kurve, grob genug, dass die Ablage nicht volllaeuft.
//
// Das volle Verhaeltnis waere zu fein: In der Ausfahrt kaemen dann alle
// achtzig Zentimeter Punkte, und `BAHN_SPUR_MAX_PUNKTE` waere nach der
// halben Kurve erschoepft.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_spur_quer_gewicht (v2.1.24: 4.0)

// Harte Obergrenze fuer die gespeicherte Spur.
//
// Greift, wenn der Mindestabstand nicht greift: beim Stehen auf der Bahn
// mit laufender Messung, oder wenn die Projektion springt. Ohne diese
// Grenze waechst der Datensatz unbeschraenkt.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_spur_max_punkte (v2.1.24: 400)

// Wie weit zurueck die Querbewegung fuer die Richtungsprobe gelesen wird.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_richtung_fenster_m (v2.1.24: 100.0)

// Ab welcher Querbewegung die Richtungsprobe ueberhaupt etwas aussagt.
//
// Unter zwei Metern ist der Unterschied zwischen „nach links gezogen" und
// „gerade geblieben" nicht messbar — dann bleibt die Seite leer.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_richtung_min_versatz_m (v2.1.24: 2.0)

/// Bestimmt die Ausfahrtsseite aus **zwei** unabhaengigen Groessen.
///
/// Spec §8.6: „Die Ausfahrtsrichtung wird über zwei unabhängige Größen
/// bestätigt, nie über eine: fallender Kurs *und* wachsender Querversatz in
/// dieselbe Richtung. Stimmt nur eine der beiden, liegt vermutlich ein
/// Achsenfehler vor und die Richtung wird nicht angezeigt."
///
/// `kurs_diff` ist die Kursabweichung zum Aufsetzkurs in Grad, positiv nach
/// rechts. `spur` ist der bisher gefahrene Streifen.
fn bahn_raeum_seite(kurs_diff: f64, spur: &[(f32, f32)], ab_laengs_m: f64) -> Option<String> {
    // Erste Groesse: der Kurs.
    let kurs_seite = if kurs_diff > 0.0 { "right" } else { "left" };

    // Zweite Groesse: die Querbewegung **beim Ausschwenken**.
    //
    // # Warum ab dem Raeumpunkt und nicht am Spurende
    //
    // Bis hierher verglich diese Funktion den letzten Spurpunkt mit einem
    // hundert Meter davor. Das misst die Bewegung dort, wo die Spur
    // AUFHOERT — und nach einer Ausfahrt rollt das Flugzeug oft noch
    // hunderte Meter parallel zur Bahn weiter. Dort ist kein Querversatz
    // mehr, und eine voellig eindeutige Linkskurve wurde wieder zu `None`.
    //
    // Gemessen wird jetzt genau das Segment, um das es geht: vom
    // Raeumpunkt aus die naechsten hundert Meter. Bewusst NICHT der
    // absolute Versatz — ein Flugzeug kann links der Mitte stehen und
    // trotzdem nach rechts abbiegen.
    let basis = spur
        .iter()
        .rev()
        .find(|(lg, _)| (*lg as f64) <= ab_laengs_m)
        .or_else(|| spur.first())?;
    let vergleich = spur
        .iter()
        .find(|(lg, _)| *lg - basis.0 >= crate::grenzwerte::grenzwerte().ausrollen.bahn_richtung_fenster_m.wert)
        // Reicht die Spur nicht mehr hundert Meter weit — etwa weil das
        // Flugzeug kurz danach stand —, zaehlt ihr letzter Punkt.
        .or_else(|| spur.last().filter(|(lg, _)| *lg > basis.0))?;

    let bewegung = vergleich.1 - basis.1;
    if bewegung.abs() < crate::grenzwerte::grenzwerte().ausrollen.bahn_richtung_min_versatz_m.wert {
        return None;
    }
    let quer_seite = if bewegung > 0.0 { "right" } else { "left" };

    // Nur wenn beide dasselbe sagen.
    if kurs_seite == quer_seite {
        Some(kurs_seite.to_string())
    } else {
        None
    }
}

// Wie weit neben der Bahn die Spur noch aufgezeichnet wird.
//
// # Warum achtzig Meter und nicht fuenfundzwanzig
//
// Die erste Fassung brach bei fuenfundzwanzig Metern ab — und traf damit
// mitten in die Ausfahrtskurve. Gemessen an raKOnJD1XgNbP06q (EDDH 23):
// Das Flugzeug verlaesst die Bahn ueber 1879 m (−18,6 m), 1901 m (−46,7 m),
// 1907 m (−80,0 m) und rollt dann parallel zur Bahn bei −180 m weiter. Die
// Aufzeichnung endete beim dritten dieser Punkte, und im Diagramm sah es
// aus, als loese sich die Spur in Luft auf.
//
// Achtzig Meter zeigen die Ausfahrt vollstaendig — bis zu dem Punkt, an dem
// die Kurve in den Rollweg uebergeht. Was danach kommt, ist Rollverkehr und
// gehoert nicht mehr zur Landung. Die Anzeige klemmt die Spur ohnehin am
// Bildrand; dass sie dort hinauslaeuft, IST die Aussage.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_spur_rand_m (v2.1.24: 80.0)

// Ab welcher Fahrt das Aufzeichnen endet.
//
// Unter fuenf Knoten steht das Flugzeug praktisch — jeder weitere Punkt
// waere derselbe Punkt.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> bahn_spur_stop_gs_kt (v2.1.24: 5.0)

/// Schreibt die Spur fort, ohne zu bewerten.
///
/// Laeuft NACH dem Schliessen des Messfensters weiter, damit die Anzeige
/// zeigen kann, wohin das Flugzeug die Bahn verlassen hat. Endet, sobald es
/// steht, seitlich weit genug weg ist oder die Ablage voll ist.
/// Traegt die Rollspur ab dem Aufsetzpunkt nach — aus dem Schnappschuss-Puffer.
///
/// # Warum das noetig ist
///
/// Die Spur kann erst laufen, wenn `stats.runway_match` steht. Der
/// entsteht in `correlate_touchdown_runway`, und die wird erst gerufen,
/// wenn der Aufsetzer bestaetigt ist — `elapsed_ms >= 1100`. Dazu kommen
/// zwei, drei Ticks im langsamen Takt, bis die Phase auf `Landing`
/// steht und der 200-ms-Takt greift.
///
/// Gemessen an **allen neun** Landungen der ersten v1.7.1-Nacht begann
/// die Spur dadurch konstant **185 bis 251 Meter hinter dem
/// Aufsetzpunkt** — es fehlte ausgerechnet die Aufsetzzone, der Teil, um
/// dessentwillen die Achse gebaut wurde. Die Marke „Aufsetzen" stand in
/// der Queransicht im Leeren, ohne Spur darunter.
///
/// Die Daten dafuer sind da: `stats.snapshot_buffer` haelt rund zehn
/// Sekunden Telemetrie mit Zeitstempel, Position und Geschwindigkeit.
/// Sie wurden nur nie benutzt.
///
/// Der Nachtrag laeuft durch dieselbe `spur_fortschreiben` wie der
/// Live-Takt — gleiche Ausduennung, gleiche Abbruchgruende, gleiche
/// Kantenerkennung. Eine zweite Spur-Logik waere genau die
/// Zweitimplementierung, die dieses Projekt schon mehrfach teuer
/// bezahlt hat.
fn spur_aus_puffer_nachtragen(stats: &mut FlightStats, halbe_breite_m: f64) {
    spur_aus_puffer_abschoepfen(stats, halbe_breite_m);
}

/// Alles aus dem 50-Hz-Puffer in die Spur legen, was noch nicht drin ist.
///
/// # Warum das fortlaufend passiert und nicht nur einmal
///
/// Die Spur hing am **Veroeffentlichungstakt** des Streamers, nicht am
/// Abtaster. Der Streamer tickt vor dem Aufsetzen mit zwei Hertz und
/// erst im erkannten Ausrollen mit fuenf — bei 140 Knoten sind das
/// sechsunddreissig Meter zwischen zwei Punkten, und zwar genau in der
/// Aufsetzzone.
///
/// Gemessen an fuenfzehn Landungen des Live-Korpus (25.08.2026), Abstand
/// der Stuetzpunkte in den ersten dreihundert Metern nach dem Aufsetzen:
///
/// ```text
/// Median ueber alle           13–14 m
/// groesste Luecke je Fassung  73–96 m
/// Ausreisser KDAL/13L (#1081) 42,9 m im Mittel, groesste Luecke 222 m
/// ```
///
/// Der Puffer, aus dem der Aufsetzer selbst erkannt wird, laeuft mit
/// fuenfzig Hertz — bei 140 Knoten alle anderthalb Meter ein Wert. Die
/// Daten waren also die ganze Zeit da; sie wurden nur EINMAL angezapft,
/// beim Nachtrag nach der Bahnzuordnung, und der deckt kaum eine
/// Sekunde ab.
///
/// Diese Fassung schoepft bei jedem Tick nach. Kein neuer Takt, keine
/// zusaetzlichen Positionsmeldungen, kein Eingriff in die
/// Veroeffentlichung — nur die Aufloesung, die der Abtaster ohnehin
/// misst.
///
/// Doppelte Punkte kann es dabei nicht geben: `spur_fortschreiben` nimmt
/// einen Wert erst ab `BAHN_SPUR_MIN_ABSTAND_M` Abstand zum letzten.
/// Der Merker `bahn_spur_bis` sorgt nur dafuer, dass nicht bei jedem Tick
/// fuenf Sekunden Puffer neu durchgerechnet werden.
fn spur_aus_puffer_abschoepfen(stats: &mut FlightStats, halbe_breite_m: f64) {
    let Some(td_at) = stats.landing_at else {
        return;
    };
    let Some(rm) = stats.runway_match.as_ref() else {
        return;
    };
    // Die Bahnachse einmal kopieren — `stats` wird gleich veraendert.
    let (t_lat, t_lon, e_lat, e_lon) = (rm.threshold_lat, rm.threshold_lon, rm.end_lat, rm.end_lon);

    // Ab dem Aufsetzzeitpunkt — oder ab dem, was beim letzten Mal schon
    // verarbeitet war. Was frueher liegt, ist entweder Anflug oder
    // laengst gezeichnet.
    //
    // ⚠ Nach dem Merker STRENG groesser, nicht `>=`. Der Merker ist der
    // Zeitstempel der zuletzt geernteten Probe; mit `>=` kommt sie bei
    // jedem Takt wieder, solange der Puffer nichts Neueres hat. Das war
    // unsichtbar, solange nur geerntet wurde (Abstand null zum letzten
    // Punkt, der Mindestabstand verwirft sie). Schreibt aber der Live-Takt
    // dazwischen einen Punkt weiter vorn, liegt die alte Probe wieder mehr
    // als zehn Meter entfernt — und wird erneut abgelegt. Feldbefund LPPT
    // 02 (#1403, v1.7.34): 976 m, 1117 m, 976 m, 1130 m, 976 m … — die
    // Anzeige zeichnete einen Saegezahn. Im Bestand sieben von 360.
    let neu = |at: DateTime<Utc>| match stats.bahn_spur_bis {
        Some(bis) if bis >= td_at => at > bis,
        _ => at >= td_at,
    };
    let proben: Vec<(f64, f64, f32, DateTime<Utc>)> = stats
        .snapshot_buffer
        .iter()
        .filter(|p| neu(p.at) && p.on_ground)
        .map(|p| (p.lat, p.lon, p.groundspeed_kt, p.at))
        .collect();

    for (lat, lon, gs, at) in proben {
        let (laengs_m, quer_m) = runway::projiziere_auf_bahn(t_lat, t_lon, e_lat, e_lon, lat, lon);
        spur_fortschreiben(stats, gs, laengs_m, quer_m, halbe_breite_m);
        stats.bahn_spur_bis = Some(at);
    }
}

fn spur_fortschreiben(
    stats: &mut FlightStats,
    // Nur die Geschwindigkeit statt des ganzen Schnappschusses: Diese
    // Funktion laeuft seit dem Nachtrag auch ueber Puffer-Eintraege, und
    // die sind kein `SimSnapshot`.
    groundspeed_kt: f32,
    laengs_m: f64,
    quer_m: f64,
    // Die halbe Bahnbreite statt des ganzen `RunwayMatch`: Der liegt in
    // `stats`, und ihn hineinzureichen waehrend `stats` veraendert wird,
    // laesst sich nicht ausleihen. Ein f64 loest das ohne Kopie.
    halbe_breite_m: f64,
) {
    // Jedes dieser drei Kriterien beendet die Aufzeichnung — und mit ihr
    // den feinen Takt. Das Flag ist die einzige Stelle, an der beides
    // zusammenhaengt; deshalb wird es hier gesetzt und nirgends sonst.
    if groundspeed_kt < crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_stop_gs_kt.wert
        || quer_m.abs() > halbe_breite_m + crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_rand_m.wert
        || laengs_m < -50.0
        || stats.bahn_spur.len() >= crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_max_punkte.wert
    {
        stats.bahn_spur_laeuft = false;
        return;
    }
    stats.bahn_spur_laeuft = true;

    // Der Kantenuebertritt — aber nur, wenn die Ausfahrt schon eingeleitet
    // ist.
    //
    // # Warum die Bedingung dazugehoert
    //
    // Diese Funktion laeuft auch bei OFFENEM Messfenster, also waehrend das
    // Flugzeug noch auf der Bahn gewertet wird. Ohne den Riegel bekaeme
    // jede Landung, die kurz ueber die Kante geraet und zurueckkommt, einen
    // „Bahn geraeumt"-Punkt mitten im Ausrollen — an einer Stelle, an der
    // das Flugzeug die Bahn gar nicht verlassen hat.
    //
    // Im Korpus sind das die neunzehn Landungen mit „Rad neben der Bahn":
    // Genau dort waere die Marke falsch gesetzt worden, und zwar VOR dem
    // Bewertungsende. Die Anzeige haette die gestrichelte Linie rueckwaerts
    // gezeichnet.
    //
    // Interpoliert wird zwischen dem letzten Punkt innerhalb und diesem
    // hier — sonst laege die Marke bis zu zehn Meter daneben, und zwar
    // immer ausserhalb, weil die Aufzeichnung erst nach dem Uebertritt
    // misst.
    let interpoliert = || kante_zwischen(stats.bahn_spur.last(), laengs_m, quer_m, halbe_breite_m);
    // Waehrend einer schnellen Ausfahrt (`AusfahrtKandidat`) ist das
    // Fenster beim Kantenuebertritt noch offen. Die Stelle wird am
    // Kandidaten vorgemerkt und erst mit seiner Bestaetigung zur Kante —
    // verfaellt er, war es keine Ausfahrt, und es bleibt beim Riegel oben.
    if !stats.bahn_fenster_zu && quer_m.abs() > halbe_breite_m {
        if stats.bahn_ausfahrt_kandidat.as_ref().is_some_and(|k| k.kante_laengs_m.is_none()) {
            let stelle = interpoliert();
            if let Some(k) = stats.bahn_ausfahrt_kandidat.as_mut() {
                k.kante_laengs_m = Some(stelle);
                k.kante_gs_kt = Some(groundspeed_kt as f64);
            }
        }
    }
    if stats.bahn_fenster_zu && stats.bahn_kante_laengs_m.is_none() && quer_m.abs() > halbe_breite_m
    {
        let interpoliert = interpoliert();
        // Nicht hinter das Bewertungsende zurueck.
        //
        // Die Interpolation greift auf den letzten abgelegten Spurpunkt
        // zurueck, und der kann durch die Ausduennung hunderte Meter
        // zurueckliegen. Ueberbrueckt sie diese Luecke, landet der
        // Kantenuebertritt vor dem Ausschwenken — also vor dem Punkt, an
        // dem die Ausfahrt ueberhaupt begann. Beim Pruefen kam so 1557 m
        // heraus, waehrend der Raeumpunkt bei 1600 m lag.
        //
        // Das ist keine Rundungsfrage: Die Marke „Bahn geraeumt" saesse
        // damit VOR der gestrichelten Linie, die sie beenden soll.
        stats.bahn_kante_laengs_m = Some(match stats.bahn_raeum_laengs_m {
            Some(raeum) => interpoliert.max(raeum),
            None => interpoliert,
        });
        stats.bahn_kante_gs_kt = Some(groundspeed_kt as f64);
    }
    // Der Abstand, wie er GEZEICHNET wird — nicht wie er gefahren wurde.
    //
    // Zwei Fehler stecken hier hintereinander, beide von Thomas gefunden:
    //
    // 1. Nur laengs zu messen verwirft in der Ausfahrt jeden Punkt. Dort
    //    faehrt das Flugzeug quer: Die Laengsposition aendert sich ueber
    //    zwanzig Meter kaum, der Querversatz um dutzende. Gemessen an
    //    raKOnJD1XgNbP06q (EDDH 23) fielen die Punkte bei 1907 m (−80 m
    //    quer) und 1898 m (−109 m quer) beide weg, und die Spur brach an
    //    der Bahnkante ab.
    //
    // 2. Auch der echte Weg reicht nicht. Die Queransicht ist rund
    //    zwoelffach ueberhoeht — zehn Meter laengs sind dort 3,4 Pixel,
    //    zehn Meter quer aber vierzig. Bei einer 30-Grad-Ausfahrt liegen
    //    Punkte im Abstand von zehn Metern Weg in der Zeichnung
    //    neunundzwanzig Pixel auseinander, auf der Geraden nur 3,4. Genau
    //    dort, wo die Kurve am staerksten kruemmt, wird sie also am
    //    groebsten — und sie sieht eckig aus.
    //
    // Deshalb wird der Querweg mit `BAHN_SPUR_QUER_GEWICHT` gewichtet: Die
    // Ausduennung misst damit annaehernd den Abstand im Bild statt auf dem
    // Boden. Auf der Geraden aendert das nichts; in der Kurve legt sie
    // mehr Punkte ab, dort wo sie gebraucht werden.
    let weit_genug = stats.bahn_spur.last().is_none_or(|(lg, qr)| {
        let d_laengs = laengs_m - *lg as f64;
        let d_quer = (quer_m - *qr as f64) * crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_quer_gewicht.wert;
        d_laengs.hypot(d_quer) >= crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_min_abstand_m.wert
    });
    if weit_genug {
        stats.bahn_spur.push((laengs_m as f32, quer_m as f32));
    }
}

/// Wo die Bahnkante zwischen dem letzten Spurpunkt und dieser Position
/// gequert wurde. Liegt der letzte Punkt schon draussen (oder fehlt), gilt
/// die Position selbst.
fn kante_zwischen(letzter: Option<&(f32, f32)>, laengs_m: f64, quer_m: f64, halbe_breite_m: f64) -> f64 {
    match letzter {
        Some((lg, qr)) if (*qr as f64).abs() <= halbe_breite_m => {
            let spanne = quer_m.abs() - (*qr as f64).abs();
            if spanne > 1e-6 {
                let t = (halbe_breite_m - (*qr as f64).abs()) / spanne;
                *lg as f64 + t * (laengs_m - *lg as f64)
            } else {
                laengs_m
            }
        }
        _ => laengs_m,
    }
}

fn bahndisziplin_tick(stats: &mut FlightStats, snap: &SimSnapshot) {
    // ── Warum diese Funktion aus unabhaengigen Schritten besteht ─────
    //
    // Sie war eine Kaskade aus sechs `return`-Pfaden, und jeder davon
    // uebersprang etwas anderes. Drei Fehler in Folge hatten dieselbe
    // Ursache:
    //
    // * Der Tick, der das Messfenster schloss, verwarf seine eigene
    //   Spurposition — `return` stand vor `spur_fortschreiben`.
    // * War das Fenster einmal zu, wurde der Kurswechsel **nie mehr**
    //   geprueft. Eine ganz normale Ausfahrt (65 kt geradeaus, 55 kt
    //   geradeaus, dann bei 25 kt abbiegen) bekam damit keinen
    //   Raeumpunkt: `scoring_cutoff_m` und die Ausfahrtsseite blieben
    //   leer, und die Anzeige nahm ersatzweise die spaetere Bahnkante —
    //   also genau den Wert, gegen den `scoring_cutoff_m` gebaut wurde.
    // * Und beim Herausziehen der Spur landete der Aufruf im falschen
    //   Pfad.
    //
    // Die Schritte laufen jetzt hintereinander und jeder fuer sich. Kein
    // `return` mehr ausser dem einen, wenn es gar keine Bahn gibt: Was
    // ein Schritt nicht tun darf, entscheidet er selbst — nicht ein
    // Sprung weiter oben.
    let Some(rm) = stats.runway_match.as_ref() else {
        return;
    };
    let (laengs_m, quer_m) = runway::projiziere_auf_bahn(
        rm.threshold_lat,
        rm.threshold_lon,
        rm.end_lat,
        rm.end_lon,
        snap.lat,
        snap.lon,
    );

    // Die nutzbare Laenge endet an der versetzten Schwelle (ICAO Annex 14).
    let nutzbare_laenge_m =
        (rm.length_ft as f64 - effective_displaced_threshold_ft(rm) as f64) / 3.280_839_895;
    let halbe_breite_m = (rm.width_ft as f64 * 0.3048 / 2.0).max(15.0);
    let bahn_kurs_deg = rm.heading_true_deg;

    // Relativ zur Aufsetz-Grundgeschwindigkeit, gedeckelt bei 60 kt (s.
    // `bahn_mess_schwelle_kt` oben) — fuer leichte GA-Muster mit niedriger
    // Aufsetzfahrt bleibt das Messfenster damit laenger offen, ohne die
    // Sicherung fuer normale/schnelle Muster zu veraendern.
    let mess_schwelle_kt = bahn_mess_schwelle_kt(stats);

    // ── 1. Ueberrollen ───────────────────────────────────────────────
    //
    // Overrun heisst: beim AUSROLLEN geradeaus ueber das Ende geschossen —
    // nicht irgendwann spaeter hinter dem Bahnende gerollt. Ohne die
    // seitliche Schranke meldete der Korpus-Export 506 von 802 Landungen
    // als Overrun (jedes Rollen zum Terminal), mit reiner Seitenschranke
    // immer noch 72.
    if !stats.bahn_fenster_zu
        && nutzbare_laenge_m > 300.0
        && laengs_m > nutzbare_laenge_m
        && snap.groundspeed_kt > 15.0
        && quer_m.abs() <= halbe_breite_m + 10.0
    {
        let ueber = laengs_m - nutzbare_laenge_m;
        stats.bahn_overrun_m = Some(stats.bahn_overrun_m.unwrap_or(0.0).max(ueber));
    }

    // ── 2. Der Raeumpunkt — unabhaengig vom Messfenster ──────────────
    //
    // Das Ausschwenken zur Ausfahrt beginnt oft LANGE nachdem die
    // Bewertung geschlossen hat: Wer unter sechzig Knoten faellt und erst
    // danach abbiegt, ist der Normalfall, nicht die Ausnahme.
    //
    // Deshalb haengt dieser Schritt nicht am Messfenster, sondern an
    // seiner eigenen Verriegelung: Er greift genau einmal, beim ERSTEN
    // Kurswechsel ueber die Schwelle.
    //
    // Er schliesst das Messfenster mit — ein Ausschwenken beendet die
    // Bewertung, egal bei welcher Fahrt.
    // Und erst unterhalb der Bewertungsschwelle.
    //
    // # Warum eine Kursaenderung ueber sechzig Knoten keine Ausfahrt ist
    //
    // Ein Flugzeug setzt bei Seitenwind schraeg auf und richtet sich danach
    // auf die Bahnachse aus. Gegenueber dem AUFSETZKURS bleibt dadurch eine
    // dauerhafte Differenz — kein Abbiegen, sondern dessen Korrektur.
    //
    // Am Korpus gemessen (854 Landungen mit 50-Hz-Fenster): 13 ueberschreiten
    // die zehn Grad, waehrend sie noch ueber sechzig Knoten schnell sind —
    // bei bis zu 141 kt und 100 von 100 folgenden Proben. Dort biegt niemand
    // auf einen Rollweg ab.
    //
    // Gegen die BAHNRICHTUNG zu messen waere naheliegend und ist schlechter:
    // Dieselbe Messung ergab damit 27 statt 13 Faelle, weil manche Landungen
    // dauerhaft schraeg zur gematchten Bahn rollen. Der Aufsetzkurs ist der
    // stabilere Bezug.
    //
    // Sechzig Knoten sind keine neue Zahl: Es ist dieselbe Schwelle, unter
    // der das Messfenster ohnehin schliesst. Darueber ist eine seitliche
    // Abweichung ein Ausbrechen — und das ist genau das, was die Achse
    // bewerten SOLL, nicht ihr Ende. High-Speed-Exits werden mit bis zu
    // sechzig Knoten genommen und liegen damit im Bereich.
    //
    // Seit v1.7.21 dieselbe RELATIVE Schwelle wie Schritt 3 (`mess_schwelle_kt`)
    // — nicht mehr fest 60 kt: Fuer normale/schnelle Muster aendert sich
    // nichts (Deckel bei 60 kt), fuer leichte GA-Muster mit niedriger
    // Aufsetzfahrt bleibt es dieselbe Schwelle wie die des Messfensters,
    // dessen Schliessen dieser Schritt beschreibt.
    let kurs_diff = stats.landing_heading_true_deg.map(|td_heading| {
        let mut diff = snap.heading_deg_true - td_heading;
        while diff > 180.0 {
            diff -= 360.0;
        }
        while diff <= -180.0 {
            diff += 360.0;
        }
        diff
    });

    // ── 2a. Schnelle Ausfahrt — siehe `AusfahrtKandidat` ─────────────
    //
    // Vor Schritt 2: Wird der Kandidat in diesem Tick bestaetigt, steht
    // der Raeumpunkt am Beginn der Kurve, nicht dort, wo die Fahrt unter
    // die Schwelle fiel.
    //
    // Aufsetzkurs UND Bahnrichtung muessen verlassen sein: Wer schraeg
    // aufgesetzt hat und sich auf die Achse dreht, liegt danach dauerhaft
    // neben dem Aufsetzkurs — aber auf der Bahnrichtung. Wer umgekehrt
    // gegen eine leicht verdrehte Bahngeometrie rollt, liegt neben der
    // Bahnrichtung, aber auf dem Aufsetzkurs. Beides ist keine Kurve, und
    // ein so angelegter Kandidat verlegte die Bewertung bei einer spaeteren
    // Ausfahrt rueckwirkend an eine Stelle, an der nichts geschah.
    let zur_bahn = {
        let mut d = snap.heading_deg_true - bahn_kurs_deg;
        while d > 180.0 {
            d -= 360.0;
        }
        while d <= -180.0 {
            d += 360.0;
        }
        d
    };
    match kurs_diff {
        Some(diff)
            if diff.abs() > crate::grenzwerte::grenzwerte().ausrollen.bahn_ausfahrt_beginn_grad.wert
                && zur_bahn.abs() > crate::grenzwerte::grenzwerte().ausrollen.bahn_ausfahrt_beginn_grad.wert =>
        {
            match stats.bahn_ausfahrt_kandidat.as_mut() {
                Some(k) => {
                    if diff.abs() as f64 > k.kurs_diff.abs() {
                        k.kurs_diff = diff as f64;
                    }
                }
                None => {
                    if !stats.bahn_fenster_zu
                        && stats.bahn_raeum_laengs_m.is_none()
                        && snap.groundspeed_kt >= mess_schwelle_kt
                        && snap.groundspeed_kt <= crate::grenzwerte::grenzwerte().ausrollen.bahn_ausfahrt_max_gs_kt.wert
                    {
                        stats.bahn_ausfahrt_kandidat = Some(AusfahrtKandidat {
                            laengs_m,
                            gs_kt: snap.groundspeed_kt as f64,
                            kurs_diff: diff as f64,
                            max_querversatz_m: stats.bahn_max_querversatz_m,
                            proben: stats.bahn_proben,
                            kante_laengs_m: None,
                            kante_gs_kt: None,
                        });
                    }
                }
            }
        }
        // Zurueck auf Kurs (oder kein Aufsetzkurs): keine Ausfahrt.
        _ => stats.bahn_ausfahrt_kandidat = None,
    }
    // Nur solange die Spur laeuft: Ist sie fertig (etwa unter fuenf Knoten
    // in der Abrollbahn stehengeblieben), ist der Nachtrag an den Recorder
    // schon unterwegs. Eine spaetere Bestaetigung aenderte die Note, aber
    // nicht mehr die Bahnwerte, die dort stehen.
    let bestaetigt = stats.bahn_spur_laeuft
        && quer_m.abs() > halbe_breite_m + crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_rand_m.wert
        && stats
            .bahn_ausfahrt_kandidat
            .as_ref()
            .is_some_and(|k| k.kurs_diff.abs() > crate::grenzwerte::grenzwerte().ausrollen.bahn_kurs_ausfahrt_grad.wert as f64);
    if bestaetigt {
        if let Some(k) = stats.bahn_ausfahrt_kandidat.take() {
            stats.bahn_fenster_zu = true;
            stats.bahn_fenster_zu_laengs_m = Some(k.laengs_m);
            stats.bahn_max_querversatz_m = k.max_querversatz_m;
            stats.bahn_proben = k.proben;
            stats.bahn_raeum_laengs_m = Some(k.laengs_m);
            stats.bahn_raeum_gs_kt = Some(k.gs_kt);
            stats.bahn_raeum_kurs_diff = Some(k.kurs_diff);
            stats.bahn_raeum_seite = bahn_raeum_seite(k.kurs_diff, &stats.bahn_spur, k.laengs_m);
            // Nicht vorgemerkt heisst: Die Kante wurde zwischen dem letzten
            // Spurpunkt und diesem Tick gequert — ein Sprung, etwa nach einem
            // Haenger des Simulators. `spur_fortschreiben` sieht ihn nicht
            // mehr, weil es bei dieser Position gleich abbricht.
            let (kante, kante_gs) = match k.kante_laengs_m {
                Some(kante) => (kante, k.kante_gs_kt),
                None => (
                    kante_zwischen(stats.bahn_spur.last(), laengs_m, quer_m, halbe_breite_m),
                    Some(snap.groundspeed_kt as f64),
                ),
            };
            stats.bahn_kante_laengs_m = Some(kante.max(k.laengs_m));
            stats.bahn_kante_gs_kt = kante_gs;
            tracing::info!(
                laengs_m = k.laengs_m,
                gs_kt = k.gs_kt,
                kurs_diff = k.kurs_diff,
                "Schnelle Ausfahrt bestaetigt — Bewertung endet am Beginn der Kurve"
            );
        }
    }

    if stats.bahn_raeum_laengs_m.is_none() && snap.groundspeed_kt < mess_schwelle_kt {
        if let Some(diff) = kurs_diff {
            if diff.abs() > crate::grenzwerte::grenzwerte().ausrollen.bahn_kurs_ausfahrt_grad.wert {
                // Das ist eine Ausfahrt, kein Abbremsen — hier gehoert der
                // Raeumpunkt hin. Beim Schliessen wegen zu geringer Fahrt
                // dagegen NICHT: dort rollt das Flugzeug noch auf der Bahn.
                stats.bahn_fenster_zu = true;
                stats.bahn_fenster_zu_laengs_m.get_or_insert(laengs_m);
                stats.bahn_raeum_laengs_m = Some(laengs_m);
                stats.bahn_raeum_gs_kt = Some(snap.groundspeed_kt as f64);
                stats.bahn_raeum_kurs_diff = Some(diff as f64);
                // Live-Versuch; er gelingt nur, wenn die Querbewegung
                // schon sichtbar ist. `bahn_felder` rechnet sie sonst aus
                // der ganzen Spur nach.
                stats.bahn_raeum_seite = bahn_raeum_seite(diff as f64, &stats.bahn_spur, laengs_m);
            }
        }
    }

    // ── 3. Das Messfenster schliessen ────────────────────────────────
    //
    // Zwei Gruende ausser dem Ausschwenken: zu geringe Fahrt, oder
    // ausserhalb der Bahn laengs. In beiden Faellen ist nichts mehr zu
    // BEWERTEN — die Spur laeuft weiter (Schritt 5).
    if !stats.bahn_fenster_zu
        && (snap.groundspeed_kt < mess_schwelle_kt
            || laengs_m < 0.0
            || (nutzbare_laenge_m > 300.0 && laengs_m > nutzbare_laenge_m))
    {
        stats.bahn_fenster_zu = true;
        stats.bahn_fenster_zu_laengs_m.get_or_insert(laengs_m);
    }

    // ── 4. Bewerten, solange das Fenster offen ist ───────────────────
    if !stats.bahn_fenster_zu {
        let bisher = stats.bahn_max_querversatz_m.unwrap_or(0.0);
        if quer_m.abs() > bisher.abs() {
            stats.bahn_max_querversatz_m = Some(quer_m);
        } else if stats.bahn_max_querversatz_m.is_none() {
            stats.bahn_max_querversatz_m = Some(quer_m);
        }
        stats.bahn_proben = stats.bahn_proben.saturating_add(1);
    }

    // ── 5. Die Spur — IMMER ──────────────────────────────────────────
    //
    // Messen und bewerten sind zwei verschiedene Dinge. Die Aufzeichnung
    // laeuft weiter, wenn die Bewertung schliesst: Sonst endet die Spur im
    // Diagramm mitten auf der Bahn, waehrend die Marke „Bahn geraeumt"
    // dreihundert Meter weiter an der Kante sitzt — dazwischen nichts. Das
    // Flugzeug waere dorthin gesprungen.
    //
    // `spur_fortschreiben` hat seine eigenen Riegel: unter fuenf Knoten,
    // jenseits des Randes, oder wenn die Ablage voll ist.
    // ERST den 50-Hz-Puffer nachschoepfen, DANN den Wert dieses Ticks.
    //
    // Die Reihenfolge ist keine Geschmacksfrage: Der Puffer traegt die
    // Werte ZWISCHEN dem letzten Tick und jetzt. Kaemen sie danach,
    // liefen die Punkte in der Ablage rueckwaerts, und die Zeichnung
    // zeigte einen Zickzack, den es nie gab.
    //
    // Was das bringt: Die Spur hing bis hierher am Veroeffentlichungstakt
    // (zwei Hertz vor dem Aufsetzen, fuenf im erkannten Ausrollen). Bei
    // 140 Knoten sind zwei Hertz sechsunddreissig Meter — genau in der
    // Aufsetzzone, dem wichtigsten Stueck. Der Abtaster misst dort
    // ohnehin mit fuenfzig Hertz.
    spur_aus_puffer_abschoepfen(stats, halbe_breite_m);
    spur_fortschreiben(stats, snap.groundspeed_kt, laengs_m, quer_m, halbe_breite_m);
}

/// Mittlere Verzögerung beim Ausrollen in m/s²: (v_Aufsetzen² − v_Ende²) /
/// (2 × Ausrollstrecke), beide über die Bodengeschwindigkeit — die IAS
/// enthielte den Wind. v_Ende ist die Geschwindigkeit am Ende des
/// Ausrollens (40 kt, Stand oder Abbiegen), nicht pauschal 0.
///
/// Ersetzt seit 2.0.1 die Bremsenergie (kJ/m): Die war Masse × Verzögerung
/// und bildete das Gewicht ab — 30 Tage live: ab 150 t 41 % gelb/rot bei
/// derselben Verzögerung wie leichte Muster (Median 1,93 gegen 1,91 m/s²).
/// Sie stand zudem im Aufsetz-Zweig, wo die Ausrollstrecke noch 0 ist, und
/// blieb fast immer leer (406 Landungen mit Strecke, 5 mit Wert).
///
/// Über 6 m/s² (0,6 g) bremst kein Verkehrsflugzeug — volle Autobrake mit
/// Umkehrschub liegt um 4–5 m/s². Ein größerer Wert ist eine Messlücke
/// (fehlende Strecke), keine Bremsung; dann bleibt das Feld leer.
fn verzoegerung_mps2(
    gs_aufsetzen_kt: Option<f32>,
    gs_ende_kt: f32,
    rollout_distance_m: Option<f64>,
) -> Option<f32> {
    let (gs0, strecke) = (gs_aufsetzen_kt?, rollout_distance_m?);
    if strecke <= 50.0 {
        return None;
    }
    let v0 = f64::from(gs0) * 0.5144; // kt → m/s
    let v1 = f64::from(gs_ende_kt.max(0.0)) * 0.5144;
    if v0 <= v1 {
        return None;
    }
    let a = (v0 * v0 - v1 * v1) / (2.0 * strecke);
    (a <= 6.0).then_some(a as f32)
}

fn rollout_tick(stats: &mut FlightStats, snap: &SimSnapshot) {
    if !stats.rollout_finalized {
        if let (Some(prev_lat), Some(prev_lon)) = (stats.rollout_last_lat, stats.rollout_last_lon) {
            let delta = ::geo::distance_m(prev_lat, prev_lon, snap.lat, snap.lon);
            // Dieselbe Regel wie die Streckenzaehlung: hier gab es GAR keinen
            // Riegel, nicht einmal die 5-Meter-Untergrenze. Ein Versetzen
            // beim Ausrollen ging voll in die Rollstrecke — und die fliesst
            // in die Landebewertung. Ohne Zeitbezug greift die Strecken-
            // schranke aus `classify_segment`.
            if classify_segment(
                (prev_lat, prev_lon),
                (snap.lat, snap.lon),
                None,
                None,
                odometer_limits(),
            ) == SegmentVerdict::Flown
            {
                stats.rollout_distance_m = Some(stats.rollout_distance_m.unwrap_or(0.0) + delta);
            }
        }
        stats.rollout_last_lat = Some(snap.lat);
        stats.rollout_last_lon = Some(snap.lon);

        // Three independent finalisation triggers — whichever
        // fires first wins. See the constants near the top of
        // this file for the rationale on each threshold.
        let exit_speed_reached = snap.groundspeed_kt < crate::grenzwerte::grenzwerte().ausrollen.rollout_exit_gs_kt.wert && snap.on_ground;
        let full_stop = snap.groundspeed_kt < crate::grenzwerte::grenzwerte().ausrollen.rollout_stop_gs_kt.wert && snap.on_ground;
        let turned_off_runway = match stats.landing_heading_true_deg {
            Some(td_heading) => {
                // Wrap-around-safe signed delta in (-180, 180].
                let mut diff = snap.heading_deg_true - td_heading;
                while diff > 180.0 {
                    diff -= 360.0;
                }
                while diff <= -180.0 {
                    diff += 360.0;
                }
                diff.abs() > crate::grenzwerte::grenzwerte().ausrollen.rollout_heading_deviation_deg.wert
            }
            None => false,
        };

        if exit_speed_reached || full_stop || turned_off_runway {
            stats.rollout_finalized = true;
            // v0.12.4 (Spec LE4): wire-form reason — the streamer
            // loop copies it into the `touchdown_rollout_finalized`
            // MQTT event.
            let reason = if turned_off_runway {
                "turned_off_runway"
            } else if full_stop {
                "full_stop"
            } else {
                "exit_speed"
            };
            stats.rollout_finalize_reason = Some(reason.to_string());
            stats.landing_decel_mps2 = verzoegerung_mps2(
                stats.landing_groundspeed_kt,
                snap.groundspeed_kt,
                stats.rollout_distance_m,
            );
            tracing::info!(
                meters = stats.rollout_distance_m.unwrap_or(0.0),
                gs_kt = snap.groundspeed_kt,
                reason,
                "rollout finalised"
            );
        }
    }
}

/// v0.16.6: streamer-driven rollout for sampler-path landings. Bush flights
/// (GSG312 97OG) touch down via the validated 50 Hz sampler while the FSM is
/// still in Takeoff/Climb — the FSM Landing arm (which owns the per-tick
/// rollout) never runs, so `rollout_distance_m` stayed `Some(0.0)` forever.
/// Called once per streamer tick right after `step_flight`:
///
///   * `sampler_touchdown_at` Some → a validated touchdown initialised the
///     rollout fields (sampler stamp block).
///   * `!rollout_finalized` → still accumulating.
///   * FSM NOT in Landing → no double-tick: when the FSM IS in Landing its
///     arm has just driven `rollout_tick` inside `step_flight` on this very
///     tick. On normal flights the Final→Landing transition re-initialises
///     the rollout fields with the FSM's own values, so this pre-Landing
///     accumulation is discarded there — byte-identical to before.
fn sampler_path_rollout_tick(stats: &mut FlightStats, snap: &SimSnapshot) {
    if stats.sampler_touchdown_at.is_none() || matches!(stats.phase, FlightPhase::Landing) {
        return;
    }
    if !stats.rollout_finalized {
        rollout_tick(stats, snap);
    }

    // ── Die Spur laeuft weiter, die Ausrollstrecke nicht ─────────────
    //
    // `rollout_finalized` faellt bei **vierzig** Knoten
    // (`ROLLOUT_EXIT_GS_KT`) — das ist der Moment, in dem die
    // Ausrollstrecke feststeht: Wer so langsam ist, bremst nicht mehr,
    // sondern rollt.
    //
    // Die Rollspur endet dort NICHT. Ihr eigener Vertrag laesst sie bis
    // fuenf Knoten laufen, bis zur Ausfahrt oder bis achtzig Meter neben
    // die Bahn (`BAHN_SPUR_STOP_GS_KT`, `BAHN_SPUR_RAND_M`) — und genau
    // dazwischen liegt alles, was sie zeigen soll: Der Raeumpunkt wird
    // typisch bei zwanzig bis dreissig Knoten erreicht, der
    // Kantenuebertritt noch spaeter.
    //
    // Bis hierher hing `bahndisziplin_tick` im selben `if` wie die
    // Streckenzaehlung. Die „finale" Spur brach damit bei vierzig Knoten
    // ab, mitten auf der Bahn, und `clearance_point_m` blieb in den
    // meisten Faellen leer — obwohl das Flugzeug kurz darauf abgebogen
    // ist.
    //
    // Die eigenen Riegel in `spur_fortschreiben` beenden sie: unter fuenf
    // Knoten, jenseits des Randes, oder wenn die Ablage voll ist.
    bahndisziplin_tick(stats, snap);
}
