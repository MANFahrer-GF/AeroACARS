/// Lernpaket AP4/AP5 (29.09.2026): Gleitpfad-Abweichung und Anflugruhe
/// fuer die gerade gestempelte Landung.
///
/// Gerechnet wird gegen die Bahn aus der Aufsetz-Korrelation
/// (`runway_nav_geometry`, nur Navigraph) — dieselbe, gegen die das
/// Stabilitaets-Gate seinen Gleitwinkel nimmt. Die versetzte Schwelle kommt
/// aus `displacement_not_in_geometry_ft`, genau wie in `assess_touchdown`:
/// was schon in der Geometrie steckt, wird nicht ein zweites Mal verschoben.
///
/// Gelesen wird NUR der Forensik-Puffer (`anflug_forensik_puffer`), nie
/// `approach_buffer` — der ist fuer die Note auf 120 Proben gekappt und
/// schnitte das 1000-ft-Tor ab (QS 29.09.2026, Befund 3).
///
/// `fenster` = `None` heisst: der ganze Puffer (FSM-Pfad, die Auswertung
/// begrenzt selbst auf 5 min vor dem Aufsetzen); der Sampler-Pfad reicht
/// sein 180-s-Fenster herein. Zeitpunkt und Fenster werden gemerkt, damit
/// `anflug_forensik_nachziehen` nach einem Bahnwechsel GENAU so rechnet.
/// Keine Note, kein Gate und kein Deckel liest das Ergebnis.
fn anflug_forensik_stempeln(
    stats: &mut FlightStats,
    fenster: Option<chrono::Duration>,
    td: Option<DateTime<Utc>>,
    platzhoehe_ft: Option<f32>,
) {
    let ab = stats.anflug_forensik_ab;
    anflug_forensik_rechnen(stats, fenster, td, platzhoehe_ft, ab);
}

/// Die eigentliche Rechnung: Puffer ab dem Schnitt `ab` (Proben davor
/// gehoeren zu einem abgebrochenen Anflug), optional auf `fenster` vor
/// `td` begrenzt. Zeitpunkt, Fenster und Schnitt werden gemerkt, damit
/// `anflug_forensik_nachziehen` exakt dieselben Proben ansieht.
fn anflug_forensik_rechnen(
    stats: &mut FlightStats,
    fenster: Option<chrono::Duration>,
    td: Option<DateTime<Utc>>,
    platzhoehe_ft: Option<f32>,
    ab: Option<DateTime<Utc>>,
) {
    let bahn = anflug_forensik_bahn(stats);
    let ergebnis = {
        let proben: std::collections::VecDeque<ApproachBufferSample> = stats
            .anflug_forensik_puffer
            .iter()
            .filter(|s| ab.is_none_or(|ab| s.at > ab))
            .cloned()
            .collect();
        let proben = match (fenster, td) {
            (Some(dauer), Some(t)) => approach_buffer_window(&proben, t, dauer),
            _ => proben,
        };
        anflug_forensik::auswerten_ab_final(
            &proben,
            bahn.as_ref(),
            td,
            platzhoehe_ft.map(f64::from),
            stats.approach_spaetes_final.map(|sf| sf.ab),
        )
    };
    // Laeuft die Dump-Auswertung des Samplers schon VOR diesem Stempel,
    // steht das Analyse-JSON bereits — dann hier nachtragen, damit
    // Datensatz und Analyse dieselben Werte tragen.
    if let Some(analyse) = stats.landing_analysis.as_mut() {
        ergebnis.in_analyse_json(analyse);
    }
    stats.anflug_forensik = ergebnis;
    stats.anflug_forensik_td = td;
    stats.anflug_forensik_fenster = fenster;
    stats.anflug_forensik_ab_gestempelt = ab;
}

/// Setzt den Schnitt fuer die Anflug-Forensik (Nachpruefung 29.09.2026,
/// Befund B): Alles, was bis jetzt im Forensik-Puffer steht, gehoert zu
/// einem ABGEBROCHENEN Anflug (Durchstarten, Touch-and-Go). Der Puffer ist
/// laenger als eine Platzrunde, und der vorige Anflug liegt auf derselben
/// Bahn, mit demselben Kurs, im selben Sektor — ohne Schnitt floesse er in
/// Gleitpfad, Ruhe und Sim-Boden der naechsten Landung ein.
///
/// Warum ein Zeitpunkt und kein Leeren: Der Puffer bleibt fuer die Diagnose
/// erhalten, und das Nachziehen nach einem Bahnwechsel rechnet mit dem
/// Schnitt, der beim Stempel galt (`anflug_forensik_ab_gestempelt`) — ein
/// spaeterer Schnitt kann einen gemachten Stempel so nicht veraendern.
///
/// `jetzt` = Zeitpunkt der Erkennung; ohne ihn gilt die letzte Probe im
/// Puffer. Der spaetere der beiden wird genommen.
fn anflug_forensik_schneiden(stats: &mut FlightStats, jetzt: Option<DateTime<Utc>>) {
    let letzte = stats.anflug_forensik_puffer.back().map(|s| s.at);
    let schnitt = match (jetzt, letzte) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    if schnitt.is_some() {
        stats.anflug_forensik_ab = schnitt;
    }
}

/// Die beim Aufsetzen gemessenen Abfang-Werte aus dem Analyse-JSON —
/// eine Quelle für Bewertung, Datensatz und PIREP.
fn abfangen_aus_analyse(stats: &FlightStats) -> Option<landing_scoring::abfangen::Abfangen> {
    stats
        .landing_analysis
        .as_ref()
        .and_then(|v| v.get("abfangen"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        // Nach einer Wiederaufnahme fehlt `landing_analysis` — die
        // gesicherte Messung trägt (QS 06.10.2026).
        .or_else(|| stats.abfangen_gesichert.clone())
}

/// Misst das Abfangen und legt es unter `abfangen` ins Analyse-JSON —
/// von dort lesen Bewertung, Datensatz und PIREP (`abfangen_aus_analyse`).
/// Eigene Funktion, damit der Test die Kette prüft und nicht nur die Messung
/// (QS 06.10.2026: fiel das Einfügen weg, fehlte die Achse still).
fn abfangen_in_analyse(
    stats: &FlightStats,
    fenster: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
    analysis: &mut serde_json::Value,
) -> landing_scoring::abfangen::Abfangen {
    let abfangen = abfangen_messen(
        stats,
        fenster,
        edge_at,
        analysis
            .get("vs_at_edge_fpm")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32)
            // Dieselben Riegel wie der Datensatz für `vs_at_edge_fpm`:
            // unplausibel (positiv oder unter −3000 fpm, Flug 804) oder
            // Aufsetzfenster unzureichend → keine Zahl statt einer, die der
            // Kopf nicht zeigt (QS 06.10.2026, Runde 4).
            .filter(|v| landing_rate_is_plausible(*v) && stats.landung_abdeckung_fehlt.is_none()),
    );
    if let (Some(obj), Ok(v)) = (analysis.as_object_mut(), serde_json::to_value(&abfangen)) {
        obj.insert("abfangen".to_string(), v);
    }
    abfangen
}

/// Wie lange vom ersten 50-ft-Durchgang (im letzten Anflug) bis zum
/// Aufsetzen abgefangen wurde (05.10.2026, Teilnote `abfangen`).
///
/// Höhe = wahre Höhe der Anflugprobe minus wahre Höhe beim ersten
/// Bodenkontakt im 50-Hz-Fenster — dieselbe Rechnung wie im Korpus, aus dem
/// die Bänder stammen (Server: `alt_ft` minus `alt_ft` am ersten
/// Bodenpunkt). So fällt der Versatz des Bezugspunkts weg (B772 meldet am
/// Aufsetzen ~16 ft über Grund). Ohne wahre Höhe im Fenster gilt die Höhe
/// über Grund, ebenfalls gegen den Wert am ersten Bodenkontakt.
///
/// Proben nur aus dem laufenden Anflug: ab dem Schnitt eines Durchstartens
/// (`anflug_forensik_ab`), höchstens `ABFANG_FENSTER_S` geflogene Sekunden
/// (ohne Sim-Pausen) vor dem Aufsetzen.
fn abfangen_messen(
    stats: &FlightStats,
    fenster: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
    vs_aufsetzen_fpm: Option<f32>,
) -> landing_scoring::abfangen::Abfangen {
    let boden = fenster.iter().find(|s| s.on_ground && s.at >= edge_at);
    let bezug: Option<(bool, f32)> = match boden {
        Some(b) => match b.msl_ft.filter(|m| m.is_finite()) {
            Some(m) => Some((true, m)),
            None => Some((false, b.agl_ft)),
        },
        None => None,
    };
    let Some((mit_msl, bezug_ft)) = bezug else {
        return landing_scoring::abfangen::Abfangen {
            vs_aufsetzen_fpm,
            grund_ohne_werte: Some("kein_bodenbezug".to_string()),
            ..Default::default()
        };
    };
    let ab = stats.anflug_forensik_ab;
    let punkte: Vec<landing_scoring::abfangen::AbfangPunkt> = stats
        .anflug_forensik_puffer
        .iter()
        .filter(|s| ab.is_none_or(|ab| s.at > ab))
        .filter(|s| s.at <= edge_at)
        .map(|s| landing_scoring::abfangen::AbfangPunkt {
            // Pausen im Sim zählen nicht als Abfangzeit (QS 06.10.2026).
            t_ms: (s.at - edge_at).num_milliseconds()
                + pausen_ms_zwischen(&stats.pause_segments, s.at, edge_at),
            hoehe_ft: if mit_msl { s.msl_ft } else { s.agl_ft } - bezug_ft,
            vs_fpm: s.vs_fpm,
            gs_kt: s.gs_kt,
        })
        // 180 s geflogene Zeit (QS 06.10.2026, vorher 60 s Wanduhr): wer
        // länger als 60 s unter 50 ft blieb, bekam gar keine Teilnote statt
        // der schlechtesten. Nach dem Pausen-Abzug gefiltert — sonst ließ eine
        // lange Pause im Abfangen die Teilnote ganz verschwinden.
        .filter(|p| -p.t_ms <= crate::grenzwerte::grenzwerte().landung.abfang_fenster_s.wert * 1000)
        .collect();
    landing_scoring::abfangen::messen(&punkte, vs_aufsetzen_fpm)
}

/// Querneigung beim Aufsetzen, wie der Datensatz sie zeigt: der Profilpunkt
/// am nächsten an t = 0. Eine Quelle für Datensatz und PIREP (QS 06.10.2026:
/// die Webapp nahm `bank_deg` des Touchdowns, eine andere Messung).
fn aufsetz_bank_deg(stats: &FlightStats) -> Option<f32> {
    stats
        .touchdown_profile
        .iter()
        .min_by_key(|p| p.t_ms.abs())
        .map(|p| p.bank_deg)
}

// Wie weit vor dem Aufsetzen Proben fürs Abfangen gelesen werden (s).
// Wert seit v3: bundle/grenzwerte/landung.json -> abfang_fenster_s (v2.1.24: 180)

/// Millisekunden Sim-Pause zwischen `von` und `bis` (Überlappung mit den
/// aufgezeichneten Pause-Blöcken).
fn pausen_ms_zwischen(pausen: &[PauseSegment], von: DateTime<Utc>, bis: DateTime<Utc>) -> i64 {
    pausen
        .iter()
        .map(|p| {
            let a = p.started_at.max(von);
            let b = p.ended_at.min(bis);
            (b - a).num_milliseconds().max(0)
        })
        .sum()
}

/// „Zeiten gegen Flugplan", Cockpit: erwartete Landung (Unix-s) = jetzt plus
/// die geplante Restzeit ab dem Lotfusspunkt auf der Route. Die Planzeiten
/// der Wegpunkte kennen Steig- und Sinkflug — genauer als Luftlinie durch
/// Groundspeed. `None` ohne Zeiten im Navlog.
fn landung_erwartet(
    fixes: &[api_client::RouteFix],
    ort: &SpritPlanOrt,
    jetzt_ms: i64,
) -> Option<i64> {
    let bis = ort.plan_bis_s?;
    let ende = fixes.last()?.zeit_bis_hier_s? as f64;
    let rest = (ende - bis).max(0.0);
    Some(jetzt_ms.div_euclid(1000) + rest.round() as i64)
}

/// Kabinenansagen: erwarteter Sinkflugbeginn wie [`landung_erwartet`], bis
/// zum TOD-Fix. Liegt er schon hinter uns, ist es „jetzt“.
fn tod_erwartet(fixes: &[api_client::RouteFix], ort: &SpritPlanOrt, jetzt_ms: i64) -> Option<i64> {
    let bis = ort.plan_bis_s?;
    let tod = fixes.iter().find(|f| f.ident.eq_ignore_ascii_case("TOD"))?.zeit_bis_hier_s? as f64;
    Some(jetzt_ms.div_euclid(1000) + (tod - bis).max(0.0).round() as i64)
}

#[cfg(test)]
#[path = "tests_lib/tod_erwartet_tests.rs"]
mod tod_erwartet_tests;

/// „Zeiten gegen Flugplan": die vier Planzeiten eines OFP und seine
/// Erstellzeit. Null oder negativ (leeres Feld) gilt als fehlend.
fn ofp_planzeiten(ofp: &api_client::SimBriefOfp) -> landing_scoring::flugzeiten::FlugZeiten {
    let echt = |v: Option<i64>| v.filter(|&t| t > 0);
    landing_scoring::flugzeiten::FlugZeiten {
        plan_off_block: echt(ofp.sched_out_epoch),
        plan_abheben: echt(ofp.sched_off_epoch),
        plan_aufsetzen: echt(ofp.sched_on_epoch),
        plan_on_block: echt(ofp.sched_in_epoch),
        // `time_generated` kommt als Text (Unix-Sekunden).
        ofp_erstellt: echt(ofp.ofp_generated_at.trim().parse().ok()),
        ..Default::default()
    }
}

/// Kabinenansagen (Beta): Flugart und Airline-Name aus dem Bid. Leere Texte
/// gelten als fehlend — phpVMS liefert den Namen notfalls als "".
fn kabinen_eckdaten_aus_bid(stats: &mut FlightStats, flight: &api_client::Flight) {
    let text = |s: &str| Some(s.trim().to_string()).filter(|s| !s.is_empty());
    stats.flight_type = flight.flight_type.as_deref().and_then(text);
    stats.airline_name = flight.airline.as_ref().and_then(|a| text(&a.name));
    // Rueckfall fuer Fluege ohne OFP (Entscheidung Thomas, 17.10.2026): die
    // Passagier-Fares (phpVMS `type` 0) des Bids. Ohne solche Fares bleibt
    // es `None`, und der Ansage-Baustein schaltet ab. Der OFP ueberschreibt.
    let pax_fares: Vec<_> = flight
        .simbrief
        .as_ref()
        .and_then(|sb| sb.subfleet.as_ref())
        .map(|sf| sf.fares.iter().filter(|f| f.fare_type == Some(0)).collect())
        .unwrap_or_default();
    if !pax_fares.is_empty() {
        stats.pax_count = Some(pax_fares.iter().filter_map(|f| f.count).map(|c| c.max(0)).sum());
        let sitze: i32 = pax_fares.iter().filter_map(|f| f.capacity).map(|c| c.max(0)).sum();
        stats.max_passengers = Some(sitze).filter(|&n| n > 0);
    }
}

/// Kabinenansagen (Beta): Passagiere und Sitzplaetze aus dem OFP. Eine 0 bei
/// den Passagieren bleibt stehen: Das ist der Frachtflug ohne Ansagen.
fn kabinen_eckdaten_aus_ofp(stats: &mut FlightStats, ofp: &api_client::SimBriefOfp) {
    stats.pax_count = Some(ofp.pax_count.max(0));
    // Fehlt die Sitzzahl im OFP, bleibt die aus den Fares stehen.
    if let Some(n) = ofp.max_passengers.filter(|&n| n > 0) {
        stats.max_passengers = Some(n);
    }
    stats.utc_versatz_abflug_min = ofp.orig_utc_offset_min;
    stats.utc_versatz_ziel_min = ofp.dest_utc_offset_min;
}

/// Kabinenansagen (Beta): Eckdaten fuer einen Flug nachholen, der sie beim
/// Start nicht erfasst hat — typisch: mit 2.0.25 gestartet, mitten im Flug
/// auf die Beta gewechselt (Thomas 17.10.2026: „das Schweigen ist bloed“).
/// Nur wenn die Passagierzahl fehlt; Bid zuerst, der OFP ueberschreibt wie
/// beim Start. Ohne passendes Bid bleibt alles, wie es ist.
fn kabinen_eckdaten_nachholen_aus(
    stats: &mut FlightStats,
    bids: &[api_client::Bid],
    bid_id: i64,
    ofp: Option<&api_client::SimBriefOfp>,
) {
    if stats.pax_count.is_some() {
        return;
    }
    let Some(bid) = bids.iter().find(|b| b.id == bid_id) else {
        return;
    };
    kabinen_eckdaten_aus_bid(stats, &bid.flight);
    if let Some(ofp) = ofp {
        kabinen_eckdaten_aus_ofp(stats, ofp);
    }
}

/// Bids und OFP fuer [`kabinen_eckdaten_nachholen_aus`] ohne Sperre holen —
/// fuer den Takt, der es bei fehlender Passagierzahl alle 2 min erneut
/// versucht (Wiedereinstieg ohne Netz blieb sonst bis zum Neustart stumm).
async fn kabinen_eckdaten_holen(client: &Client, bid_id: i64) -> Option<(Vec<api_client::Bid>, Option<api_client::SimBriefOfp>)> {
    let bids = match client.get_bids().await {
        Ok(b) => b,
        Err(e) => {
            tracing::info!(error = %e, "kabinenansagen: Bids fuer Nachholen nicht erreichbar");
            return None;
        }
    };
    let sb_id = bids
        .iter()
        .find(|b| b.id == bid_id)
        .and_then(|b| b.flight.simbrief.as_ref())
        .map(|s| s.id.clone());
    let ofp = match sb_id {
        Some(id) => client.fetch_simbrief_ofp(&id).await.ok().flatten(),
        None => None,
    };
    Some((bids, ofp))
}

/// Netzteil zu [`kabinen_eckdaten_nachholen_aus`]: einmal beim Wiedereinstieg.
/// Ohne Netz bleibt der Flug stumm wie bisher — kein Wiederholen im Takt.
async fn kabinen_eckdaten_nachholen(client: &Client, bid_id: i64, stats: &mut FlightStats) {
    if stats.pax_count.is_some() {
        return;
    }
    let bids = match client.get_bids().await {
        Ok(b) => b,
        Err(e) => {
            tracing::info!(error = %e, "kabinenansagen: Bids fuer Nachholen nicht erreichbar");
            return;
        }
    };
    let sb_id = bids
        .iter()
        .find(|b| b.id == bid_id)
        .and_then(|b| b.flight.simbrief.as_ref())
        .map(|s| s.id.clone());
    let ofp = match sb_id {
        Some(id) => client.fetch_simbrief_ofp(&id).await.ok().flatten(),
        None => None,
    };
    kabinen_eckdaten_nachholen_aus(stats, &bids, bid_id, ofp.as_ref());
}

/// Kabinenansagen (Beta): was der Planer einmal zu Beginn vom Flug wissen
/// muss. `ziel` ist das geplante Ziel, `planzeit_min` die Flugzeit aus
/// phpVMS als Rueckfall, wenn der OFP keine Planzeiten traegt.
fn kabinen_flugdaten(
    stats: &FlightStats,
    ziel: &str,
    planzeit_min: Option<i32>,
    muster: &str,
) -> cabin_announcements::Flugdaten {
    use cabin_announcements::flugart;
    let u = |v: Option<i32>| v.and_then(|n| u32::try_from(n).ok());
    let plan = &stats.ofp_planzeiten;
    let ofp_min = match (plan.plan_abheben, plan.plan_aufsetzen) {
        (Some(ab), Some(auf)) if auf > ab => Some((auf - ab) / 60),
        _ => None,
    };
    cabin_announcements::Flugdaten {
        flugart: Some(flugart::einordnen(stats.flight_type.as_deref(), u(stats.pax_count), muster)),
        geplante_flugzeit_min: ofp_min
            .and_then(|m| u32::try_from(m).ok())
            .or(u(planzeit_min))
            .filter(|&m| m > 0),
        // Navlog `time_total` zaehlt ab dem Abheben (Steigflug-Fixe der
        // Testdaten: 23.600 ft nach 513 s).
        tod_nach_abheben_s: stats
            .planned_waypoints
            .iter()
            .find(|f| f.ident.eq_ignore_ascii_case("TOD"))
            .and_then(|f| f.zeit_bis_hier_s)
            .filter(|s| s.is_finite() && *s > 0.0)
            .map(|s| s.round() as i64),
        ziel_plan_icao: Some(ziel.trim().to_ascii_uppercase()).filter(|s| !s.is_empty()),
        reiseflughoehe_ft: stats
            .planned_cruise_alt_ft
            .filter(|f| f.is_finite() && *f > 0.0)
            .map(|f| f.round() as u32),
        abheben_epoch: stats.takeoff_at.map(|t| t.timestamp()),
    }
}

/// Kabinenansagen (Beta): Ergebnis eines Planer-Takts.
struct KabinenSchritt {
    faellig: Vec<cabin_announcements::Faellig>,
    /// Ruhephase (Sterile Cockpit): nur Kommandos und Chimes.
    ruhephase: bool,
    phase: FlightPhase,
    rest_min: Option<f64>,
}

/// Kabinenansagen (Beta): ein Takt des Planers mit der effektiven Phase
/// (`stats.phase`). Der Aufrufer (`kabinenansagen::takt`) haelt die Sperre,
/// prueft Einstellung, Freischaltung, Pause/Slew/Replay und dass eine
/// Passagierzahl bekannt ist; hier ohne Sperre, damit testbar.
fn kabinen_schritt(
    stats: &mut FlightStats,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
    ziel: &str,
    planzeit_min: Option<i32>,
    muster: &str,
    abgewaehlt: &[cabin_announcements::Slot],
) -> KabinenSchritt {
    let jetzt = now.timestamp();
    // Kein Weg konnte den Stand uebernehmen (Etappe 4): neu aufbauen wie
    // nach einem Neustart.
    if stats.kabinen_planer.as_ref().is_some_and(kabinenplaner::Planer::unbrauchbar) {
        stats.kabinen_planer = None;
    }
    if stats.kabinen_planer.is_none() {
        // `fortsetzen` gilt auch fuer den normalen Start: Vor dem Boarding
        // loest kein Slot aus, also wird dort nichts uebersprungen.
        let flug = kabinen_flugdaten(stats, ziel, planzeit_min, muster);
        stats.kabinen_durchstarts = stats.go_around_count;
        // Untergrenze aus den Ist-Zeiten: Ist schon abgehoben (bzw. vom
        // Gate weg), gelten die Boden-Ansagen als vorbei — auch wenn die
        // Phase nach dem Neustart noch unsicher ist (Codex-QS beta.4, 13).
        let r = cabin_announcements::planer::rang;
        let phase = if stats.takeoff_at.is_some() && r(stats.phase) < r(FlightPhase::Climb) {
            FlightPhase::Climb
        } else if stats.block_off_at.is_some() && r(stats.phase) < r(FlightPhase::Pushback) {
            FlightPhase::Pushback
        } else {
            stats.phase
        };
        let mut planer = kabinenplaner::Planer::fortsetzen(flug, phase);
        planer.schon_gespielt(kabinenansagen::erledigte_slots(&stats.kabinen_lauf));
        if kabinenansagen::service_gespielt(&stats.kabinen_lauf) {
            planer.service_ende(true);
        } else if kabinenansagen::service_ausgefallen(&stats.kabinen_lauf) {
            planer.service_ende(false);
            if kabinenansagen::service_entfallen(&stats.kabinen_lauf) {
                planer.service_entfallen();
            }
        }
        stats.kabinen_planer = Some(planer);
    }
    let durchgestartet = stats.go_around_count > stats.kabinen_durchstarts;
    stats.kabinen_durchstarts = stats.go_around_count;
    let tick = cabin_announcements::Tick {
        jetzt,
        phase: stats.phase,
        hoehe_ft: snap.altitude_msl_ft,
        vs_fpm: f64::from(snap.vertical_speed_fpm),
        am_boden: snap.on_ground,
        anschnallzeichen: snap.seatbelts_sign,
        // 0/1/2 = schiebt, 3 = kein Pushback.
        pushback_aktiv: matches!(snap.pushback_state, Some(0..=2)),
        rest_min: stats
            .landung_erwartet
            .map(|t| (t - jetzt) as f64 / 60.0)
            .filter(|m| *m >= 0.0),
        // Das neue Ziel meldet der Pilot im Flug (`flight_umleitung_setzen`);
        // `divert_hint` kommt erst nach der Landung.
        ziel_icao: stats.umleitung_ziel.clone(),
        abweichung_off_block_min: if stats.block_off_at.is_none() {
            stats
                .ofp_planzeiten
                .plan_off_block
                .and_then(|p| i32::try_from((jetzt - p).div_euclid(60)).ok())
        } else {
            None
        },
        durchgestartet,
        // Cockpit-Schalter nur als Flanke, Phase bleibt Rückfall (Planer).
        beacon: snap.light_beacon,
        landelicht: snap.light_landing,
        fahrwerk_unten: Some(snap.gear_position >= 0.9),
        hoehe_agl_ft: Some(snap.altitude_agl_ft),
        platznah: Some(final_in_platznaehe(&stats, snap)),
        triebwerke_an: Some(snap.engines_running > 0),
        tod_erwartet: stats.tod_erwartet,
        zfw_kg: stats.last_zfw_kg.map(f64::from),
        planned_zfw_kg: stats.planned_zfw_kg.map(f64::from),
        // „Rollt“ für „Plätze für den Start“ (2.1.1 B, ab 3 kt).
        bodengeschwindigkeit_kt: Some(f64::from(snap.groundspeed_kt)),
    };
    let zweiter_service = kabinenansagen::zweiter_service(&stats.kabinen_lauf);
    let (faellig, ruhephase) = stats
        .kabinen_planer
        .as_mut()
        .map(|p| {
            p.abwaehlen(abgewaehlt.iter().copied());
            p.reiseflug80_art(zweiter_service);
            let f = p.tick(&tick);
            (f, p.ruhephase(&tick))
        })
        .unwrap_or_default();
    KabinenSchritt { faellig, ruhephase, phase: tick.phase, rest_min: tick.rest_min }
}

/// „Zeiten gegen Flugplan": Plan des OFP plus die Ist-Zeiten dieses Fluges.
/// Immer `Some` — ohne Plan sagt die Anzeige „kein SimBrief-OFP" (Altbestand
/// ohne das Feld blendet den Abschnitt dagegen ganz aus).
fn flugzeiten(stats: &FlightStats) -> landing_scoring::flugzeiten::FlugZeiten {
    landing_scoring::flugzeiten::FlugZeiten {
        off_block: stats.block_off_at.map(|t| t.timestamp()),
        abheben: stats.takeoff_at.map(|t| t.timestamp()),
        aufsetzen: stats.landing_at.map(|t| t.timestamp()),
        on_block: stats.block_on_at.map(|t| t.timestamp()),
        ..stats.ofp_planzeiten
    }
}

/// Score-Version 23: spaetes Final / Kurvenanflug erkennen. Aus dem
/// Forensik-Puffer (~5 min) ab dem Durchstart-Schnitt — der Gate-Puffer reicht
/// mit 120 Proben (~2 min) nicht bis vor das Final. Ohne Navdaten-Bahn keine
/// Erkennung (dann gilt das bisherige Gate).
///
/// Wie das uebrige Gate (auch der Gleitwinkel-Faktor) wird das Ergebnis am
/// Aufsetzen eingefroren: Ein spaeterer Bahnwechsel (Szenerie nachgereicht,
/// Navigraph-Upgrade beim Einreichen) rechnet es nicht neu, und
/// `anflug_forensik_nachziehen` filtert die Gleitpfad-Tore weiter ab dem
/// eingefrorenen `ab` — passend zur eingefrorenen Streuung. Eine
/// Szenerie-Korrektur verschiebt die Achse um Meter und Hundertstel Grad,
/// weit unter den Toleranzen (300 m, 20°). Kam die Navdaten-Bahn erst nach
/// dem Aufsetzen, bleibt es beim geraden Anflug (bisheriges Verhalten).
/// Codex-QS 13.10.2026, bewusst so belassen.
fn spaetes_final_erkennen(
    stats: &FlightStats,
    bezug_ft: Option<f32>,
    td: DateTime<Utc>,
    fenster: Option<chrono::Duration>,
) -> Option<spaetes_final::SpaetesFinal> {
    let nav = stats.runway_nav_geometry.as_ref()?;
    let bahn = spaetes_final::Bahnachse {
        schwelle_lat: nav.threshold.lat,
        schwelle_lon: nav.threshold.lon,
        kurs_wahr_deg: nav.true_course,
    };
    let schnitt = stats.anflug_forensik_ab;
    let proben: Vec<&ApproachBufferSample> = stats
        .anflug_forensik_puffer
        .iter()
        .filter(|s| schnitt.is_none_or(|ab| s.at > ab))
        .filter(|s| fenster.is_none_or(|d| s.at >= td - d))
        .collect();
    spaetes_final::erkennen(
        &proben,
        |s| match bezug_ft {
            Some(e) => s.msl_ft - e,
            None => s.agl_ft,
        },
        td,
        &bahn,
        crate::grenzwerte::grenzwerte().anflug_kern.approach_flare_cutoff_ms.wert,
    )
}

/// Score-Version 25: Die gelandete Bahn, wie Navigraph sie liefert — VOR der
/// Uebernahme aus der Sim-Szenerie — und deren eigener Versatz, soweit er
/// nicht schon im Schwellenpunkt steckt. Bezug des ILS-Gleitpfads.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct IlsNavigraph {
    bahn: aeroacars_mqtt::navdata::NavRunway,
    versatz_ft: i32,
}

impl IlsNavigraph {
    fn aus(apt: &aeroacars_mqtt::navdata::NavAirport, bezeichner: &str) -> Option<Self> {
        let bahn = apt.runways.iter().find(|r| r.designator == bezeichner)?.clone();
        let (gemeldet, geometrie) = runway::navigraph_versatz_ft(apt, &bahn);
        Some(Self {
            versatz_ft: versatz_nicht_in_geometrie_ft(gemeldet, geometrie),
            bahn,
        })
    }
}

/// Die Bahn der Forensik: Navdaten-Geometrie plus versetzte Schwelle.
///
/// Score-Version 25 (Sven, RJAA 34L, 16.10.2026): Hat die Bahn ein ILS, gilt
/// die UNVERAENDERTE Navigraph-Bahn mit ihrem eigenen Versatz. Der ILS-
/// Gleitpfad (TCH, Gleitwegsender) ist dort definiert; die Szenerie kennt ihn
/// nicht (siehe `szenerie_bahn::uebernimm_szenerie`: „ILS bleibt, wo es
/// ist"). Vorher wanderte die Landeschwelle des ILS ueber den Szenerie-
/// Versatz mit — MSFS-Default RJAA 34L malt 751,6 m Versatz, Navigraph hat 0
/// und den Gleitwegsender 329 m hinter dem Bahnanfang: ein exakt geflogenes
/// ILS galt als 1,8 Dots zu tief. Ohne ILS (Sichtanflug, PAPI an der
/// gemalten Schwelle) bleibt die Szenerie massgeblich. Aufsetzzone und
/// Bahndisziplin rechnen weiter gegen die Szenerie — die sieht der Pilot.
///
/// Nur der VERSATZ kommt aus Navigraph, die Geometrie bleibt die der
/// Zuordnung: Unter X-Plane bleibt die Forensik so auf derselben Achse wie
/// Spur, Aufsetzpunkt und `spaetes_final` (Codex, 16.10.2026).
///
/// ⚠ P-4.14: Unter MSFS wird die Navdaten-Achse seit 3.0 durch die
/// Facility-Mitte gelegt (verschoben, ggf. gedreht). Das ILS ist in den
/// Navdaten definiert und darf damit NICHT mitwandern (Regel aus dem Fall
/// Sven, RJAA 34L) — dort gilt die Navigraph-Bahn vor der Übernahme
/// (`ils.bahn`) samt Geometrie.
fn anflug_forensik_bahn(stats: &FlightStats) -> Option<anflug_forensik::Bahnbezug> {
    if let Some(ils) = stats
        .runway_ils_navigraph
        .as_ref()
        .filter(|i| i.bahn.ils.is_some())
    {
        let achse_verschoben = stats.szenerie_uebernahme.as_ref().is_some_and(|b| {
            b.achsen.iter().any(|a| {
                a.bahn == ils.bahn.designator
                    && a.herkunft == szenerie_bahn::AchsenHerkunft::SzenerieMitte
            })
        });
        let bahn = if achse_verschoben {
            &ils.bahn
        } else {
            stats.runway_nav_geometry.as_ref().unwrap_or(&ils.bahn)
        };
        return Some(anflug_forensik::Bahnbezug::aus_navdaten(
            bahn,
            ils.versatz_ft as f64,
        ));
    }
    stats.runway_nav_geometry.as_ref().map(|nav| {
        let versatz_ft = stats
            .runway_match
            .as_ref()
            .map(displacement_not_in_geometry_ft)
            .unwrap_or(0);
        anflug_forensik::Bahnbezug::aus_navdaten(nav, versatz_ft as f64)
    })
}

/// Was an der Bahn die Forensik bestimmt — zum Vergleich vor/nach einem
/// Bahnwechsel.
fn anflug_forensik_bahn_kennung(stats: &FlightStats) -> String {
    format!("{:?}", anflug_forensik_bahn(stats))
}

/// Rechnet die Forensik neu, nachdem sich die Bahn geaendert hat
/// (QS 29.09.2026, Befund 2): Navigraph-Upgrade beim Einreichen
/// (`bahn_upgrade_anwenden`) oder spaet eingetroffene Szenerie
/// (`bahn_am_aufsetzpunkt_nachholen`, ersetzt Schwelle/Ende/Versatz).
///
/// Nur, wenn DIESE Landung schon gestempelt wurde — nach einem
/// Durchstart-Reset ist `anflug_forensik` leer und bleibt es — und der
/// Puffer Proben vor dem Aufsetzen hat (nach einem Neustart ist er leer:
/// dann bleibt der alte Wert stehen statt „keine Proben" zu behaupten).
/// Gleicher Zeitpunkt, gleiches Fenster und dieselbe Platzhoehe wie beim
/// Stempel. Sperrreihenfolge stats → navdata wie in
/// `finalize_runway_correlation`.
fn anflug_forensik_nachziehen(stats: &mut FlightStats, flight: &ActiveFlight) {
    if stats.anflug_forensik.gleitpfad.is_none() {
        return;
    }
    let Some(td) = stats
        .anflug_forensik_td
        .or(stats.landing_at)
        .or(stats.sampler_touchdown_at)
    else {
        return;
    };
    if !stats.anflug_forensik_puffer.iter().any(|s| s.at < td) {
        return;
    }
    let platzhoehe = anflug_bezugshoehe_ft(
        stats,
        &flight.arr_airport,
        ist_platzhoehe_navdaten_ft(flight, stats),
    );
    let fenster = stats.anflug_forensik_fenster;
    let ab = stats.anflug_forensik_ab_gestempelt;
    let vorher = stats.anflug_forensik.clone();
    anflug_forensik_rechnen(stats, fenster, Some(td), platzhoehe, ab);
    if stats.anflug_forensik != vorher {
        // Das Flug-Log traegt noch den alten Stand — beim Einreichen
        // nachtragen (`emit_landing_finalized`).
        stats.anflug_forensik_nachtrag_offen = true;
    }
}

/// Schreibt den Forensik-Nachtrag ins Flug-Log, falls einer offen ist —
/// aus JEDEM Abschluss-Pfad (eingereicht, manuell, Warteschlange). Die
/// „nur einmal"-Fahne sitzt in `anflug_forensik_nachtrag_ereignis`.
fn anflug_forensik_nachtrag_schreiben(app: &AppHandle, flight: &ActiveFlight) {
    let nachtrag = {
        let mut s = flight.stats.lock().expect("flight stats");
        anflug_forensik_nachtrag_ereignis(&mut s)
    };
    if let Some(ereignis) = nachtrag {
        record_event(app, &flight.pirep_id, &ereignis);
    }
}

/// Das Nachtrags-Ereignis fuer das Flug-Log — einmal, und nur wenn
/// `anflug_forensik_nachziehen` die Werte wirklich geaendert hat.
fn anflug_forensik_nachtrag_ereignis(stats: &mut FlightStats) -> Option<FlightLogEvent> {
    if !stats.anflug_forensik_nachtrag_offen {
        return None;
    }
    stats.anflug_forensik_nachtrag_offen = false;
    let mut payload = serde_json::json!({
        "edge_at": stats.anflug_forensik_td.map(|t| t.to_rfc3339()),
    });
    stats.anflug_forensik.in_analyse_json(&mut payload);
    Some(FlightLogEvent::LandingAnalysisNachtrag {
        timestamp: Utc::now(),
        payload,
    })
}
