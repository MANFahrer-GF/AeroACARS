/// Echte-Uhr-Variante für Tests; der Streamer taktet über [`phasen_takt`].
#[cfg(test)]
fn step_flight(flight: &ActiveFlight, snap: &SimSnapshot) -> Option<FlightPhase> {
    step_flight_at(flight, snap, Utc::now())
}

/// Ergebnis eines [`phasen_takt`].
struct PhasenTakt {
    /// Phasenwechsel dieses Takts (wie [`step_flight_at`]).
    wechsel: Option<FlightPhase>,
    /// v2-Sicht dieses Takts; `None` bei Pause/Slew (v2 tickt dann nicht).
    v2: Option<(FlightPhase, phase_v2::Segment)>,
}

/// Ein Phasen-Takt des Positions-Streamers: v1-Schritt, danach die v2-Engine
/// mit der Post-Step-Phase als Sync-Quelle. EINE Stelle für Streamer und
/// Korpus-Replay (`phasen_korpus_replay`), damit das Replay genau das prüft,
/// was im Betrieb läuft (17.10.2026).
fn phasen_takt(flight: &ActiveFlight, snap: &SimSnapshot, now: DateTime<Utc>) -> PhasenTakt {
    let wechsel = step_flight_at(flight, snap, now);
    // v0.16.12 (#phase-v2): Schatten-Phasen-Engine ticken — NACH
    // step_flight (braucht die autoritative Post-Step-Phase als
    // Sync-Quelle außerhalb des En-Route-Bands). Pause/Slew friert die
    // Engine ein wie die alte FSM (step_flight returnt dort früh); die
    // >5-min-Gap-Erkennung im Segmenter fängt den Resume ab.
    if snap.paused || snap.slew_mode {
        return PhasenTakt { wechsel, v2: None };
    }
    let mut stats = flight.stats.lock().expect("flight stats");
    let old_phase_post_step = stats.phase;
    // cruise_ref: FMC-Cruise-Alt (PMDG, vom Piloten gepflegt,
    // step-climb-aktuell) vor dem geplanten Bid-/Manual-Level.
    let cruise_ref_ft = snap
        .pmdg
        .as_ref()
        .and_then(|p| p.fmc_cruise_alt_ft)
        .filter(|&ft| ft > 0)
        .map(f64::from)
        .or(stats.planned_cruise_alt_ft);
    let (shadow_phase, shadow_segment) = stats.shadow_engine.step(
        now,
        snap.altitude_msl_ft,
        snap.altitude_agl_ft,
        snap.vertical_speed_fpm,
        snap.on_ground,
        old_phase_post_step,
        cruise_ref_ft,
    );
    stats.shadow_phase = Some(shadow_phase);
    stats.shadow_segment = Some(shadow_segment);
    PhasenTakt { wechsel, v2: Some((shadow_phase, shadow_segment)) }
}

/// Uhr-injizierbare Variante von [`step_flight`] — `now` ist DER Zeitpunkt
/// dieses Ticks für sämtliche Dwell-Timer und Zeitstempel im FSM-Schritt.
/// Produktion ruft immer über den [`step_flight`]-Wrapper (echte Uhr);
/// Replay-Tests füttern historische Timestamps aus echten Flug-Logs und
/// machen damit die v1-FSM erstmals offline-replayfähig (das JSONL-
/// Recorder-Format verspricht das seit jeher — "feed it back into the FSM
/// offline", crates/recorder).
/// Is this snapshot's position real, or is the sim still making it up?
///
/// MSFS reports (0, 0) — "Null Island", in the Gulf of Guinea — while a session
/// is still loading, before the aircraft has been placed. Weights and speeds in
/// those snapshots are equally meaningless (a default airframe mass, not the
/// loaded one).
///
/// v0.19.3, field evidence: DLH367's first session stamped a TAKEOFF at
/// lat 0.0084 / lon 0.00002 with a takeoff weight of 349 t — the aircraft's real
/// takeoff, an hour later, was 300 t from Charlotte. A takeoff at Null Island
/// went into the flight record, and nothing rejected it.
///
/// The 0.02° box (~2 km) is safe to refuse outright: there is no land, let alone
/// an airport, within it.
fn snapshot_position_is_usable(snap: &SimSnapshot) -> bool {
    if !snap.lat.is_finite() || !snap.lon.is_finite() {
        return false;
    }
    // Die Hoehe gehoert zur Position. Beim Laden lieferte MSFS am 24.09.2026
    // (Joel, OCN 712) eine Hoehe von 20.926.040 ft — der Client las den
    // zugehoerigen AGL-Wert als Durchstart nach der Landung und loeschte den
    // Landesprit. Kein Luftfahrzeug im Simulator fliegt ueber 150.000 ft oder
    // steht tiefer als 2.000 ft unter NN.
    if !snap.altitude_msl_ft.is_finite()
        || !(crate::grenzwerte::grenzwerte().sprit.snapshot_hoehe_min_ft.wert..=crate::grenzwerte::grenzwerte().sprit.snapshot_hoehe_max_ft.wert).contains(&snap.altitude_msl_ft)
    {
        return false;
    }
    !(snap.lat.abs() < 0.02 && snap.lon.abs() < 0.02)
}

/// Nach der Landung kann der Tank nur leerer werden — und das langsam.
///
/// Zwischen Aufsetzen und Einreichen wird Sprit verbraucht, nie getankt —
/// steigt der Wert, hat jemand neu beladen: ein Sim-Reload (OCN 712,
/// 24.09.2026: 9544 kg nach dem Neuladen in EDDN) oder der Tankwagen fuer
/// den naechsten Flug. Der PIREP bekaeme sonst diesen Wert als Endsprit.
///
/// Ein Durchstart setzt `landing_fuel_kg` zurueck
/// (`sprit_durchstart_zuruecksetzen`), danach gilt wieder der echte Wert.
fn tankstand_nach_landung_begrenzen(
    live_kg: f32,
    landing_kg: Option<f32>,
    vorher_kg: Option<f32>,
    landing_at: Option<DateTime<Utc>>,
    bodenrate_kg_pro_h: f32,
    jetzt: DateTime<Utc>,
) -> f32 {
    // Messrauschen und Rundung — kein Tankvorgang.
    // Wert seit v3: bundle/grenzwerte/sprit.json -> toleranz_kg (v2.1.24: 50.0)
    let Some(ldg) = landing_kg else {
        return live_kg;
    };
    // Der letzte echte Wert, hoechstens der Landewert.
    let referenz = vorher_kg.map_or(ldg, |v| v.min(ldg + crate::grenzwerte::grenzwerte().sprit.toleranz_kg.wert));
    if live_kg > ldg + crate::grenzwerte::grenzwerte().sprit.toleranz_kg.wert {
        // Neubeladung (Sim-Reload, Tankwagen) — nie.
        return referenz;
    }
    // Ein fast leerer Tank nach der Landung ist Muell (OCN 712: 8 kg).
    if live_kg < (ldg * 0.02).max(1.0) {
        return referenz;
    }
    // Wie viel das Flugzeug SEIT DER LANDUNG am Boden verbraucht haben kann —
    // in seiner eigenen Groesse gerechnet, nicht in festen Kilogramm.
    //
    // Drei Runden QS (24.09.2026) haben diese Grenze geformt:
    //   * „60 s stabil" mass nur den Abstand zweier Messungen und liess einen
    //     Muellwert nach einer Pause durch.
    //   * Feste 200 kg / 2000 kg/h schuetzten eine Cessna mit 150 kg
    //     Landesprit gar nicht.
    //   * Pro Messung gerechnet, summierten sich viele kleine Schritte.
    // Deshalb: kumulativ ab der Landung (`landing_at` ist gespeichert und
    // ueberlebt einen App-Neustart), Reserve und Rate nach dem Flugzeug.
    //
    // Ohne bekannten Aufsetzzeitpunkt gibt es keine Zeitbasis — dann keine
    // Zeitgrenze, im Zweifel gilt der echte Wert. Sonst durfte ein Flug, dessen
    // Landung nur der Aufsetz-Sampler kannte, nach der Landung nur die Reserve
    // verlieren, und ein echter Rollverbrauch von 649 kg wurde verworfen
    // (zwei bestehende Tests fingen das, 24.09.2026).
    let Some(landing_at) = landing_at else {
        return live_kg;
    };
    let stunden = (jetzt - landing_at).num_seconds().max(0) as f32 / 3600.0;
    let reserve_kg = (ldg * 0.05).clamp(5.0, 200.0);
    let erlaubter_abfall = reserve_kg + bodenrate_kg_pro_h * stunden;
    if ldg - live_kg > erlaubter_abfall {
        return referenz;
    }
    live_kg
}

/// Was dieses Flugzeug am Boden hoechstens pro Stunde verbraucht.
///
/// Aus dem Flug selbst: (Blocksprit - Landesprit) / Flugzeit ist der
/// Reiseverbrauch; am Boden (Rollen, APU) sind es erfahrungsgemaess unter
/// einem Viertel davon — 35 % laesst Luft. Ohne vollstaendige Flugdaten
/// der grosszuegige Rueckfall fuer vierstrahlige Jets.
fn bodenverbrauch_kg_pro_h(
    block_kg: Option<f32>,
    landing_kg: Option<f32>,
    takeoff_at: Option<DateTime<Utc>>,
    landing_at: Option<DateTime<Utc>>,
) -> f32 {
    // Wert seit v3: bundle/grenzwerte/sprit.json -> rueckfall_kg_pro_h (v2.1.24: 2_000.0)
    let (Some(block), Some(ldg), Some(ab), Some(an)) =
        (block_kg, landing_kg, takeoff_at, landing_at)
    else {
        return crate::grenzwerte::grenzwerte().sprit.rueckfall_kg_pro_h.wert;
    };
    let stunden = (an - ab).num_seconds() as f32 / 3600.0;
    if stunden < 0.1 || block <= ldg {
        return crate::grenzwerte::grenzwerte().sprit.rueckfall_kg_pro_h.wert;
    }
    ((block - ldg) / stunden * 0.35).clamp(5.0, crate::grenzwerte::grenzwerte().sprit.rueckfall_kg_pro_h.wert)
}

// Tiefster plausibler Wert (Totes Meer ~ -1.400 ft, mit Reserve).
// Wert seit v3: bundle/grenzwerte/sprit.json -> snapshot_hoehe_min_ft (v2.1.24: -2_000.0)
// Hoechster plausibler Wert. Concorde fliegt bis FL600, Addons wie der
// Darkstar hoeher — 150.000 ft laesst ihnen Luft und faengt trotzdem den
// Messmuell (20.926.040 ft am 24.09.2026).
// Wert seit v3: bundle/grenzwerte/sprit.json -> snapshot_hoehe_max_ft (v2.1.24: 150_000.0)

/// Wie ein Positions-Sprung einzuordnen ist.
///
/// Es gab diese Pruefung im Code schon einmal — inline in der Aufzeichnungs-
/// schleife, mit lokalen Konstanten, und deshalb fuer niemanden sonst
/// benutzbar. Die Streckenzaehlung und das Ausrollen hatten je eigene
/// (bzw. gar keine) Regeln. Ergebnis: Michael K. hatte EDAV->EDAZ (real
/// 42 NM) mit 4987 NM im Logbuch. Jetzt gibt es EINE Regel; die Toleranz
/// bleibt je Verwendung verschieden, weil ein Reset-Erkenner grosszuegiger
/// sein darf als ein Kilometerzaehler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentVerdict {
    /// Plausibel geflogen — zaehlen.
    Flown,
    /// Sprung: Null-Position, Hoehen- oder Streckensprung. Verwerfen und
    /// die Basislinie neu setzen.
    Jump(&'static str),
    /// Die Luecke ist zu gross, um irgendetwas zu behaupten. Nicht zaehlen,
    /// neu ansetzen — ein zu kleiner Zaehlerstand ist ehrlicher als ein
    /// erfundener.
    Gap,
}

/// Grenzwerte einer Verwendung. Eigene Struktur, damit an der Aufrufstelle
/// steht, WAS geprueft wird, statt vier nackter Zahlen.
#[derive(Debug, Clone, Copy)]
struct SegmentLimits {
    /// Groesste Luecke zwischen zwei Proben, die noch beurteilbar ist.
    max_dt_secs: f64,
    /// Groesste Strecke je Sekunde, ausgedrueckt als Geschwindigkeit.
    max_speed_kt: f64,
    /// Fester Zuschlag gegen Takt-Jitter.
    slack_nm: f64,
    /// Groesster Hoehensprung; `None` schaltet die Pruefung ab.
    max_alt_jump_ft: Option<f64>,
    /// Feste Streckengrenze statt der geschwindigkeitsabhaengigen. Der
    /// Reset-Erkenner arbeitet seit jeher so (10 NM, unabhaengig vom
    /// Abstand) und ist im Feld erprobt — das Verhalten bleibt, damit die
    /// Vereinheitlichung nichts verstellt, was sich bewaehrt hat.
    flat_max_dist_nm: Option<f64>,
}

/// Toleranz der Streckenzaehlung: eng, weil hier jeder Fehler dauerhaft im
/// Logbuch steht. Weit ueber jedem Muster (SR-71 ~1900 kt), damit kein
/// echter Flug beschnitten wird.
/// Werte seit v3: bundle/grenzwerte/abgabe.json -> odometer_* (v2.1.24:
/// `ODOMETER_LIMITS` 120 s, 2200 kt, 1 NM).
fn odometer_limits() -> SegmentLimits {
    let g = &crate::grenzwerte::grenzwerte().abgabe;
    SegmentLimits {
        max_dt_secs: g.odometer_max_luecke_s.wert,
        max_speed_kt: g.odometer_max_kt.wert,
        slack_nm: g.odometer_zuschlag_nm.wert,
        max_alt_jump_ft: None,
        flat_max_dist_nm: None,
    }
}

/// Toleranz des Reset-Erkenners — unveraendert aus der Aufzeichnungs-
/// schleife uebernommen (10 NM bei bis zu 2 s, 10.000 ft). Bewusst
/// grosszuegig: hier kostet ein Fehlalarm eine ganze Sitzung.
///
/// Werte seit v3: bundle/grenzwerte/abgabe.json -> reset_max_luecke_s,
/// reset_max_strecke_nm und das schon vorhandene reset_max_alt_jump_ft
/// (v2.1.24: `RESET_LIMITS` 2 s, 10 NM, 10.000 ft). Geschwindigkeit und
/// Zuschlag wirken hier nicht (feste Streckengrenze), deshalb kein Paketwert.
fn reset_limits() -> SegmentLimits {
    let g = &crate::grenzwerte::grenzwerte().abgabe;
    SegmentLimits {
        max_dt_secs: g.reset_max_luecke_s.wert,
        max_speed_kt: f64::INFINITY,
        slack_nm: 0.0,
        max_alt_jump_ft: Some(g.reset_max_alt_jump_ft.wert),
        flat_max_dist_nm: Some(g.reset_max_strecke_nm.wert),
    }
}

/// Null-Position: das Flugzeug ist beim Laden kurz auf 0/0, bevor der
/// Simulator es setzt. Der haeufigste Sprung ueberhaupt.
fn is_null_island(lat: f64, lon: f64) -> bool {
    lat.abs() < 0.1 && lon.abs() < 0.1
}

fn classify_segment(
    prev: (f64, f64),
    cur: (f64, f64),
    alt_jump_ft: Option<f64>,
    dt_secs: Option<f64>,
    limits: SegmentLimits,
) -> SegmentVerdict {
    classify_segment_rate(prev, cur, alt_jump_ft, dt_secs, limits, None)
}

// Obergrenze der Sim-Rate, mit der die erlaubte Strecke wachsen darf —
// dieselbe wie bei der Auto-Start-Sprungpruefung: ein Teleport ueber
// hunderte km darf auch bei Zeitraffer nie als Flug durchgehen.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> simrate_strecke_max (v2.1.24: 16.0)

// Spielraum auf die gemeldete Groundspeed bei Zeitraffer: Die GS stammt
// aus einer Probe, das Segment dauert mehrere Sekunden (Beschleunigung,
// Rundung der Sim-Rate). Wie bei der Auto-Start-Sprungpruefung (dort x2),
// hier enger, weil jeder Fehler dauerhaft im Logbuch steht.
// Wert seit v3: bundle/grenzwerte/abgabe.json -> zeitraffer_gs_spielraum (v2.1.24: 1.5)

/// Wie `classify_segment`, aber mit Zeitraffer `(sim_rate, groundspeed_kt)`.
///
/// Bei 8x legt das Flugzeug je echter Sekunde achtmal so viel Strecke
/// zurueck. Die Erweiterung haengt bewusst an der GEMELDETEN Groundspeed
/// (gedeckelt auf `RESUME_MAX_PLAUSIBEL_KT`), nicht an der festen
/// 2200-kt-Grenze: sonst liessen 16x und eine 120-s-Luecke rechnerisch
/// ~11.700 NM als Flug durch (QS 03.10.2026) — ein Teleport ueber die
/// Weltkarte bei laufender Telemetrie haette im Logbuch gestanden.
/// Ohne GS oder bei Rate <= 1 gilt die normale Grenze; sie wird nie kleiner
/// (Zeitlupe). Die Luecken-Grenze (`max_dt_secs`) bleibt in echter Zeit.
fn classify_segment_rate(
    prev: (f64, f64),
    cur: (f64, f64),
    alt_jump_ft: Option<f64>,
    dt_secs: Option<f64>,
    limits: SegmentLimits,
    zeitraffer: Option<(f64, f64)>,
) -> SegmentVerdict {
    if is_null_island(cur.0, cur.1) || is_null_island(prev.0, prev.1) {
        return SegmentVerdict::Jump("Null-Position");
    }
    let d_nm = ::geo::distance_m(prev.0, prev.1, cur.0, cur.1) / 1852.0;
    let Some(dt) = dt_secs else {
        // Ohne Zeitbezug (Sitzungen von vor diesem Feld) bleibt nur die
        // Strecke selbst. Ein einzelner Tick ueber 50 NM ist kein Flug.
        return if d_nm > 50.0 {
            SegmentVerdict::Jump("Strecke ohne Zeitbezug")
        } else {
            SegmentVerdict::Flown
        };
    };
    if !(0.0..=limits.max_dt_secs).contains(&dt) {
        return SegmentVerdict::Gap;
    }
    if let (Some(grenze), Some(sprung)) = (limits.max_alt_jump_ft, alt_jump_ft) {
        if sprung > grenze {
            return SegmentVerdict::Jump("Hoehensprung");
        }
    }
    let erlaubt = match limits.flat_max_dist_nm {
        Some(fest) => fest,
        None => {
            let normal = limits.max_speed_kt * (dt / 3600.0) + limits.slack_nm;
            match zeitraffer {
                Some((rate, gs_kt)) if rate.is_finite() && gs_kt.is_finite() && rate > 1.0 => {
                    let rate = rate.min(crate::grenzwerte::grenzwerte().abgabe.simrate_strecke_max.wert);
                    let gs = gs_kt.clamp(0.0, crate::grenzwerte::grenzwerte().abgabe.resume_max_plausibel_kt.wert);
                    let beschleunigt =
                        gs * crate::grenzwerte::grenzwerte().abgabe.zeitraffer_gs_spielraum.wert * rate * (dt / 3600.0) + limits.slack_nm;
                    normal.max(beschleunigt)
                }
                _ => normal,
            }
        }
    };
    if d_nm > erlaubt {
        SegmentVerdict::Jump("Positionssprung")
    } else {
        SegmentVerdict::Flown
    }
}

/// Finalisiert `stats.landing_score` einmalig, sobald `BOUNCE_WINDOW_SECS`
/// seit `stats.landing_at` verstrichen ist — UNABHAENGIG von `stats.phase`.
///
/// # Warum das eine eigene, phasenunabhaengige Funktion ist (v1.7.24)
///
/// Vorher stand diese Pruefung ausschliesslich im `FlightPhase::Landing`-
/// Match-Arm von `step_flight_at` (`if !in_bounce_window &&
/// stats.landing_score.is_none() { ... }`). Ein Match-Arm laeuft nur,
/// SOLANGE `stats.phase` noch den passenden Wert hat — wechselt die Phase
/// vorher, feuert der Code nie wieder.
///
/// Real aufgetreten bei GSG1249 (EDDW→EDHE, 2026-09-09, PA24, sehr kurze
/// Ausrollstrecke): `Final`→`Landing` um 11:26:24.04Z, `Landing`→`TaxiIn`
/// bereits um 11:26:31.46Z — 7,4 Sekunden spaeter. `BOUNCE_WINDOW_SECS`
/// ist 8 (durch die Ganzzahl-Division `elapsed_ms / 1_000` in der alten
/// `elapsed_secs`-Berechnung faktisch erst ab ~9s erfuellt) — der
/// Landing-Arm hat also nie ein `!in_bounce_window` erlebt, bevor die
/// Phase schon TaxiIn war. `stats.landing_score` blieb fuer den Rest des
/// Fluges `None`. `announce_landing_score()` (`let score =
/// stats.landing_score?;`) gab deshalb IMMER `None` zurueck — die einzige
/// Quelle fuer das `TouchdownComplete`-Ereignis (MQTT + JSONL) feuerte nie,
/// obwohl der PIREP am Ende trotzdem einen vollen, unabhaengig berechneten
/// Score bekam (`build_pirep_payload` liest `landing_scoring::
/// compute_sub_scores` frisch, nicht `stats.landing_score`). Der Server
/// markierte den PIREP deshalb als `no_touchdown_recorded`.
///
/// Dieselbe Fehlerklasse (Phasenwechsel raeumt einen an `Landing`
/// gebundenen Zustand vorzeitig ab) war an genau dieser Stelle schon
/// einmal gefunden und behoben worden (v0.7.20, GSG219 — der
/// `TouchdownEvent`-Push weiter unten im selben Match-Arm wurde damals
/// bewusst von `landing_score.is_some()` entkoppelt, mit derselben
/// Begruendung). Diese Funktion zieht dieselbe Lehre konsequent auf die
/// Score-Finalisierung selbst durch: sie haengt an nichts als
/// `stats.landing_at` (ein Zeit-/Episode-Anker, keine Flugphase) und wird
/// von `step_flight_at` bei JEDEM Tick aufgerufen, unabhaengig davon, in
/// welcher Phase sich der Flug gerade befindet.
///
/// Verhaelt sich fuer alle bestehenden Faelle byte-identisch: dieselbe
/// Bedingung (`elapsed_secs > BOUNCE_WINDOW_SECS`, `landing_score.is_none()`),
/// derselbe `canonical_landing_rate_fpm()`-Aufruf, derselbe B-005-Guard
/// (kein erfundener Score aus einem fehlenden Wert). Der einzige
/// Unterschied ist, WANN sie ueberhaupt eine Chance bekommt zu laufen.
///
/// # GAF-707-Race, strukturell geloest (Vereinheitlichung 09/2026)
///
/// Bis hierher setzte NICHT NUR diese Funktion `landing_score`, sondern
/// AUCH der Touchdown-Sampler selbst (sein eigener Dump, `spawn_touchdown_
/// sampler`) — zwei unabhaengig laufende Ticker (Sampler ~50Hz, FSM/
/// Streamer alle 750ms-1s), die beide um dieselbe Frage konkurrierten:
/// "ist diese Landung schon final?" Zwei Codex-Reviews haben zwei
/// unzureichende Zwischenstaende gefunden:
///
///   - Runde 1: ein einzelner AGL-Schnappschuss beim Dump-Zeitpunkt
///     (`climbed_out_early`) — kein Dwell, kein `!on_ground`-Check, und ein
///     Fenster-Mismatch (Sampler-Dump-Timeout 10s vs. FSM-T&G-Fenster
///     `TOUCH_AND_GO_WATCH_SECS`=30s). Ein AGL-Sensor-Spike konnte bei
///     Sampler-only-Fluegen den Score dauerhaft verhindern.
///   - Runde 2: diese Funktion als alleiniger Scorer, aber nur gegen
///     `touch_and_go_pending_since` (ein NACHLAUFENDES Signal — erst gesetzt
///     NACHDEM die FSM den Schwellenuebertritt schon bemerkt hat) und gegen
///     `touchdown_window_dumped_at` (zu FRUEH gesetzt — noch VOR den
///     eigentlichen score-relevanten Rohdaten, siehe unten) gewartet.
///
/// Diese Fassung schliesst alle drei konkret benannten Luecken:
///
///   1. **Live-Pruefung statt Nachlauf-Signal.** `agl_ft`/`on_ground` kommen
///      DIREKT vom aktuellen `SimSnapshot` (Parameter dieser Funktion, vom
///      selben Tick wie die FSM-eigene T&G-Pruefung) — nicht ueber
///      `touch_and_go_pending_since`, das erst NACH dem ersten
///      Schwellenuebertritt existiert. Steht das Flugzeug GERADE ueber der
///      T&G-Schwelle in der Luft, wird schon in DIESEM Tick nicht
///      finalisiert, nicht erst im naechsten. `touch_and_go_pending_since`
///      bleibt zusaetzlich als zweite Sicherung fuer die Dwell-Dauer selbst.
///   2. **Doppelter Episoden-Anker.** `stats.landing_at.or(stats.sampler_
///      touchdown_at)` statt nur `stats.landing_at` — Sampler-only-Fluege
///      (Busch-Landungen, FSM bleibt in Takeoff/Climb haengen, erreicht nie
///      `FlightPhase::Landing`) haben nie ein `landing_at`, hatten aber VOR
///      diesem Umbau trotzdem sofort einen Score vom Sampler-Dump bekommen.
///      Mit nur `landing_at` als Anker haette diese Funktion sie dauerhaft
///      uebersehen — derselbe Fallback wie an anderer Stelle im Code bereits
///      etabliert (`touchdown_complete` liest `sampler_touchdown_at.or(
///      landing_at)`).
///   3. **Zweiphasiges Dump-Signal.** `touchdown_window_dumped_at` wird vom
///      Sampler SEHR frueh gesetzt (Schutz vor Doppel-Dump im eigenen 50Hz-
///      Loop + Resume-Sicherheit) — LANGE bevor die score-relevanten
///      Rohdaten (VS-Kaskade, `bounce_count`) tatsaechlich geschrieben sind.
///      Diese Funktion wartet stattdessen auf das eigene, spaeter gesetzte
///      `touchdown_window_score_data_ready_at` — es wird ERST gesetzt,
///      nachdem wirklich alle score-relevanten Felder geschrieben sind
///      (siehe dessen Setzstelle im Sampler).
///
/// Kein Fall verliert dadurch dauerhaft einen Score — es wird nur
/// (wiederholt, live, jeden Tick neu geprueft) gewartet, nie geraten und nie
/// stillschweigend aufgegeben; nach `SAMPLER_DUMP_WAIT_TIMEOUT_MS` ohne
/// fertigen Dump wird trotzdem mit der vorhandenen Kanonik gescort.
fn finalize_landing_score_if_due(
    stats: &mut FlightStats,
    pirep_id: &str,
    now: DateTime<Utc>,
    agl_ft: f32,
    on_ground: bool,
) {
    let Some(touchdown) = stats.landing_at.or(stats.sampler_touchdown_at) else {
        return;
    };
    if stats.landing_score.is_some() {
        return;
    }
    let elapsed_secs = (now - touchdown).num_milliseconds() / 1_000;
    if elapsed_secs <= crate::grenzwerte::grenzwerte().landung.bounce_window_secs.wert {
        return; // Fenster noch offen — absichtlich noch nicht finalisieren.
    }
    // GAF-707-Race (1/3): Live-Check ohne Lag — das Flugzeug ist GERADE
    // ueber die T&G-Schwelle geklettert, noch bevor die FSM das ueberhaupt
    // in `touch_and_go_pending_since` festgehalten hat (das passiert im
    // selben Tick, aber im `FlightPhase::Landing`-Match-Arm VOR diesem
    // Aufruf — dieser Live-Check faengt trotzdem den allerersten Tick ab,
    // an dem noch kein Pending-Latch existiert).
    if !on_ground && agl_ft > crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_agl_threshold_ft.wert {
        return;
    }
    // GAF-707-Race (1/3, Fortsetzung): eine T&G-Dwell-Bestaetigung laeuft
    // bereits — die Episode koennte in Kuerze als Touch-and-Go
    // zurueckgesetzt werden.
    if stats.touch_and_go_pending_since.is_some() {
        return;
    }
    // GAF-707-Race (3/3): der Sampler hat einen Touchdown validiert, aber
    // die score-relevanten Rohdaten (verfeinerte VS-Kaskade, bounce_count)
    // sind noch nicht vollstaendig geschrieben. Auf das spaete, praezise
    // Signal warten statt mit der groeberen FSM-Landing-Arm-Schaetzung
    // vorzupreschen — bis zur gemeinsamen Fallback-Grenze.
    if let Some(sampler_td_at) = stats.sampler_touchdown_at {
        if stats.touchdown_window_score_data_ready_at.is_none() {
            let elapsed_since_sampler_td_ms = (now - sampler_td_at).num_milliseconds();
            if elapsed_since_sampler_td_ms < SAMPLER_DUMP_WAIT_TIMEOUT_MS {
                return; // weiter auf die Rohdaten warten.
            }
            tracing::warn!(
                pirep_id = %pirep_id,
                elapsed_since_sampler_td_ms,
                "finalize_landing_score_if_due: Sampler-Dump-Timeout — scoring mit vorhandener Kanonik ohne verfeinerte 50Hz-Kaskade (Buffer-Dump evtl. fehlgeschlagen)"
            );
        }
    }
    // Bug B (Codex, Vereinheitlichung 09/2026): der Sampler hat diesen
    // Kandidaten AKTIV abgelehnt (`FalseEdge`, z.B. zu wenig Stimmen oder
    // Gear-Force unter Schwelle) — das ist eine evidenzbasierte
    // Entscheidung, keine fehlende Information. Ohne diesen Guard wuerde
    // der FSM-Fallback (`canonical_landing_rate_fpm()`) diese Ablehnung
    // stillschweigend ueberstimmen, weil `sampler_touchdown_at` in beiden
    // Faellen (aktive Ablehnung ODER Sampler nie gelaufen) gleichermassen
    // `None` ist. `InsufficientTelemetry` landet nie in diesem Feld (siehe
    // Setzstelle) — dort bleibt der FSM-Fallback bewusst die einzige
    // Quelle.
    if let Some(reason) = stats.sampler_touchdown_rejected_reason {
        // Diese Funktion läuft bei jedem Takt. Svens DLH 880 schrieb die
        // Warnung deshalb 1141-mal ins Protokoll. Einmal je Ablehnung reicht.
        if !stats.ablehnung_gemeldet {
            stats.ablehnung_gemeldet = true;
            tracing::warn!(
                pirep_id = %pirep_id,
                reason = ?reason,
                "Sampler hat diesen Touchdown-Kandidaten aktiv abgelehnt — landing_score bleibt None (Bug B guard, keine FSM-Ueberstimmung)."
            );
        }
        return;
    }
    // ── Wurde das Aufsetzen überhaupt gemessen? ──────────────────────────
    //
    // Hier, gemeinsam für ALLE Wege in die Bewertung: nach dem Dump-Timeout
    // ebenso wie im reinen FSM-Pfad, der ohne Sampler-Kandidaten gar nicht
    // erst wartet (Codex-Abnahme 12.09.2026 — die erste Fassung sperrte nur
    // den Timeout-Zweig und liess diesen zweiten Weg offen).
    //
    // Liegt kein ausgewertetes Aufsetzfenster vor, ist nichts gemessen. Dann
    // entsteht keine Note aus der gröberen FSM-Schätzung — das war der Weg,
    // über den CFG 2090 zu 97 Punkten kam.
    //
    // Ausgenommen bleibt die vom Simulator selbst gemeldete Rate: Sie hängt
    // nicht an unserer Abtastung (29 von 1170 Landungen des Bestands haben
    // kein Fenster und trotzdem einen belastbaren Wert).
    // Codex, zweite Abnahme 13.09.2026: Die erste Fassung setzte die Markierung
    // bei `msfs_simvar_latched` gar nicht — und liess damit G-Spitze und
    // Hopser aus dem FSM-Puffer in die Bewertung. Die Markierung beschreibt
    // aber eine Tatsache über UNSER Fenster, nicht über die Sinkrate. Ob die
    // vom Simulator gemeldete Rate trotzdem gilt, entscheidet erst
    // `canonical_landing_rate_fpm`; G und Hopser fallen in jedem Fall weg.
    if stats.landung_abdeckung_fehlt.is_none()
        && stats.touchdown_window_score_data_ready_at.is_none()
    {
        tracing::warn!(
            pirep_id = %pirep_id,
            landing_source = ?stats.landing_source,
            "kein ausgewertetes Aufsetzfenster — Landung gilt als nicht messbar"
        );
        stats.landung_abdeckung_fehlt = Some(touchdown_v2::FehlendeAbdeckung {
            groesste_luecke_ms: crate::grenzwerte::grenzwerte().aufsetzen.bewertungs_fenster_vor_ms.wert
                + crate::grenzwerte::grenzwerte().aufsetzen.bewertungs_fenster_nach_ms.wert,
            proben: 0,
        });
        plugin_sinkrate_uebernehmen(stats);
    }

    // v0.7.17 (B-005): Wenn `landing_peak_vs_fpm` None ist (= Sampler hat
    // den Touchdown-VS gar nicht erfasst, typisch bei Phantom-Touchdowns
    // durch on_ground-Flicker oder Sim-Bug), waere `unwrap_or(0.0)` = 0.0
    // fpm gefallen → Smooth-Score 100 trotz nie wirklich gemessenen Werts.
    // Fix: nur klassifizieren wenn `canonical_landing_rate_fpm()`
    // tatsaechlich Some ist. Sonst bleibt `landing_score` None.
    if let Some(peak_vs) = stats.canonical_landing_rate_fpm() {
        // v0.12.3 (LE8/QS-P1): classify on the scored G (raw_fallback
        // here — no forensics window).
        let scored_g = score_g_for_stats(stats)
            .map(|sg| sg.scored_g)
            .unwrap_or(0.0);
        // Ohne gemessenes Fenster zählen die Hopser nicht — sonst verschlechterte
        // ein ungemessener Wert die Klasse (Codex, dritte Abnahme: MSFS −168 fpm
        // mit zwei Hopsern ergab "Acceptable" statt "Smooth").
        let hopser = hopser_aus_dem_fenster(stats).unwrap_or(0) as _;
        let score = LandingScore::classify(peak_vs, scored_g, hopser);
        stats.landing_score = Some(score);
        // GAF-707-Race: an dieser Stelle wurde entweder auf die Rohdaten
        // gewartet (oder es gab gar keinen Sampler-Touchdown zum Warten) —
        // der Score ist damit final. `announce_landing_score` braucht
        // deshalb NICHT mehr zusaetzlich auf `sampler_touchdown_at`+Timeout
        // zu warten (sonst wartete es effektiv zweimal hintereinander).
        stats.landing_score_finalized = true;
        stats.landing_score_announced = false;
    } else {
        // „Nicht bewertbar" (Aufzeichnung im Aufsetzfenster zu dünn) ist ein
        // Endzustand: Es kommt keine Note mehr. Als abgeschlossen melden,
        // sonst warten HUD-Band, MSFS-Panel und Flow-Widget bis zum Ende des
        // Flugs auf „Landung wird ausgewertet" (AFR 421, 06.10.2026). Nur
        // dieser Fall — ohne Abdeckungsbefund kann die Kanonik später noch
        // eine Sinkrate liefern.
        if stats.landung_abdeckung_fehlt.is_some() {
            stats.landing_score_finalized = true;
        }
        if !stats.landung_unmessbar_gemeldet {
            stats.landung_unmessbar_gemeldet = true;
            tracing::warn!(
                pirep_id = %pirep_id,
                "landing_peak_vs_fpm = None at score-finalise — keeping landing_score=None (B-005). Sampler likely missed the touchdown edge."
            );
        }
    }
}

fn step_flight_at(
    flight: &ActiveFlight,
    snap: &SimSnapshot,
    now: DateTime<Utc>,
) -> Option<FlightPhase> {
    let mut stats = flight.stats.lock().expect("flight stats");

    // Category-aware FSM: a rotorcraft flies a profile the fixed-wing phase
    // logic mis-reads (operates below the 500 ft Climb gate, arrests its
    // descent into a hover that looks like a go-around, lands rotors-running).
    // Resolved once per tick from the live ICAO + gear-type flags; FixedWing
    // when unknown ⇒ all of the branches below are byte-identical to before.
    let category = aircraft_category::resolve_category(
        snap.aircraft_icao.as_deref(),
        snap.gear_is_skid,
        snap.gear_is_floats,
        snap.gear_is_wheels,
        snap.water_rudder_present,
    );

    // Klappen-Spanne des Fluges mitschreiben — einmal pro Tick, zwei Zahlen.
    // Sie beantworten spaeter am 1000-ft-Tor zwei Fragen, die ein einzelner
    // Messwert nicht beantworten kann: bewegt sich der Kanal ueberhaupt, und
    // wo endet das Raster dieses Musters. Bewusst hier und nicht in
    // `push_approach_sample`: der Puffer dort ist phasen-gebunden und wirft
    // Samples weg, die Spanne braucht aber den ganzen Flug.
    merke_klappen_spanne(&mut stats, snap.flaps_position);

    // v0.7.19 GAF-707 Sim-Event-Pfad (Spec §Klassifikator-Trigger-Punkte):
    // wenn der Adapter `snap.crashed=true` liefert und der aktive Flug
    // noch nicht gelatcht war, sofort Accident setzen — KEIN Warten auf
    // Touchdown-Edge. `kind=SimCrash` ist authoritativ; spaeter
    // greifende Heuristik mergt nur ihre Reasons rein, aendert aber
    // nicht den kind.
    //
    // `CrashReset` aus dem Adapter laesst snap.crashed wieder auf false
    // fallen, aber der hier gesetzte Latch persistiert bis Flight-End/
    // Cleanup/neuem Flight-Start (Spec §Leitentscheidung 6).
    if snap.crashed && !stats.accident_detected {
        stats.accident_detected = true;
        stats.accident_at = Some(now);
        stats.accident_kind = Some(accident::AccidentKind::SimCrash.as_wire_str().to_string());
        stats.accident_confidence = Some("high".into());
        let source = snap
            .crash_source
            .clone()
            .unwrap_or_else(|| "sim_event".into());
        stats
            .accident_reasons
            .push(format!("crash_source={source}"));
        tracing::warn!(
            crash_source = %source,
            "v0.7.19: simulator crash event latched on active flight"
        );
    }

    // v0.5.40: Anti-Flicker-State + Pushback-Active-Tracking refreshen.
    // Muss am Anfang passieren damit alle nachfolgenden Phase-Handler den
    // aktuellen Stand sehen.
    if snap.engines_running > 0 {
        stats.last_engines_running_above_zero_at = Some(now);
    }
    // Der Merker rastet beim ersten Snapshot ein, auch ohne Pushback-Wert:
    // Käme erst `None` und dann 0 (Schlepper), gälte sonst die 0 als erster
    // Wert (Codex 15.10.2026).
    if !stats.pushback_status_gesehen {
        stats.pushback_status_gesehen = true;
        stats.pushback_status_klemmt = snap.pushback_state.is_some_and(|pb| pb != 3)
            && matches!(stats.phase, FlightPhase::Preflight | FlightPhase::Boarding);
    }
    if let Some(pb) = snap.pushback_state {
        if pb != 3 {
            stats.saw_pushback_state_active = true;
        }
    }

    // Distance accounting.
    if let (Some(prev_lat), Some(prev_lon)) = (stats.last_lat, stats.last_lon) {
        let d_m = ::geo::distance_m(prev_lat, prev_lon, snap.lat, snap.lon);
        let dt = stats
            .last_pos_at
            .map(|t| (now - t).num_milliseconds() as f64 / 1000.0);
        // Zeitraffer: Ein Segment gilt mit der hoeheren der beiden Raten —
        // wechselt die Rate mitten im Takt, gehoert die Strecke zur schnelleren
        // Seite (Feldbefund TGW 882: 1201 NM bei 8x/16x als „zu schnell"
        // verworfen, im PIREP 1753 statt 2956 NM).
        // Groundspeed ebenso: die hoehere aus voriger und aktueller Probe.
        let rate = f64::from(stats.last_sim_rate.max(snap.simulation_rate));
        let gs_kt = f64::from(
            stats
                .last_known_gs_kt
                .unwrap_or(0.0)
                .max(snap.groundspeed_kt),
        );
        let urteil = classify_segment_rate(
            (prev_lat, prev_lon),
            (snap.lat, snap.lon),
            None,
            dt,
            odometer_limits(),
            Some((rate, gs_kt)),
        );
        if d_m > crate::grenzwerte::grenzwerte().phasen_kern.distance_epsilon_m.wert && urteil == SegmentVerdict::Flown {
            stats.distance_nm += d_m / 1852.0;
        }
    }
    stats.last_sim_rate = snap.simulation_rate;
    let sprit_dt_s = stats
        .last_pos_at
        .map(|t| (now - t).num_milliseconds() as f64 / 1000.0);
    stats.last_lat = Some(snap.lat);
    stats.last_lon = Some(snap.lon);
    stats.last_pos_at = Some(now);
    // v0.19.3: WHERE THE AIRCRAFT LAST WAS — a different question from "what is
    // the distance baseline", which is what `last_lat/lon` above is.
    //
    // A resume deliberately nulls `last_lat/lon` so that a repositioning jump
    // doesn't count as flown distance. That is correct for the odometer and
    // fatal for anyone asking "where is this aircraft": the filing gate then
    // cannot place it at all and silently waves the flight through. A pilot who
    // blocked on at EDDF, lost his sim, hit "resume", and filed would have got a
    // clean, auto-approved EDDM arrival PIREP — from Frankfurt.
    //
    // So the last position is kept separately, and nothing clears it.
    stats.last_known_lat = Some(snap.lat);
    stats.last_known_lon = Some(snap.lon);
    stats.last_known_alt_ft = Some(snap.altitude_msl_ft);
    stats.last_known_hdg_deg = Some(snap.heading_deg_true);
    stats.last_known_gs_kt = Some(snap.groundspeed_kt);
    stats.last_known_at = Some(now);
    stats.position_count = stats.position_count.saturating_add(1);
    let prev_fuel_kg = stats.last_fuel_kg;
    // Aufsetzzeit: die der FSM, sonst die des Aufsetz-Samplers (Busch- und
    // Hubschrauberlandungen, bei denen die FSM noch nicht in Landing ist).
    // Fuer Zeitbasis UND Verbrauchsrate — sonst bekam eine Cessna, deren
    // Landung nur der Sampler kannte, die Jet-Rate (Codex-QS 24.09.2026).
    let aufgesetzt_am = stats.landing_at.or(stats.sampler_touchdown_at);
    let bodenrate = bodenverbrauch_kg_pro_h(
        stats.block_fuel_kg,
        stats.landing_fuel_kg,
        stats.takeoff_at,
        aufgesetzt_am,
    );
    stats.last_fuel_kg = Some(tankstand_nach_landung_begrenzen(
        snap.fuel_total_kg,
        stats.landing_fuel_kg,
        prev_fuel_kg,
        aufgesetzt_am,
        bodenrate,
        now,
    ));
    let sprit_replay_verdacht = stats.replay_verdacht;
    // Die beiden Boden-Marken — eigener Pfad, weil `sprit_tick` am Boden
    // aussteigt.
    // Entprellt wie jede andere Triebwerks-Abfrage hier.
    let sprit_laeuft = engines_effectively_running(&stats, &snap, now);
    // Der plausibilisierte Wert, nicht der rohe: Sonst rastete ein
    // Menue-Wert nach der Landung als „Triebwerke aus"-Marke ein, und der
    // PIREP meldete „Rollen nach der Landung: 2958 kg" (QS 24.09.2026).
    let plausibler_tank_kg = stats.last_fuel_kg.unwrap_or(snap.fuel_total_kg);
    sprit_boden_marken(
        &mut stats,
        plausibler_tank_kg,
        sprit_laeuft,
        snap.on_ground,
        snap.paused,
        snap.slew_mode,
        sprit_replay_verdacht,
        now,
    );
    sprit_tick(
        &mut stats,
        snap.lat,
        snap.lon,
        snap.altitude_msl_ft,
        snap.fuel_total_kg,
        snap.paused,
        snap.slew_mode,
        sprit_replay_verdacht,
        snap.on_ground,
        sprit_dt_s,
    );
    sprit_wegpunkt_tick(
        &mut stats,
        snap.lat,
        snap.lon,
        snap.fuel_total_kg,
        snap.paused,
        snap.slew_mode,
        sprit_replay_verdacht,
        snap.on_ground,
        now.timestamp_millis(),
    );
    // v0.3.0: ZFW + Total-Weight für Live-Loadsheet (Cockpit-Tab
    // während Boarding-Phase). Updates jeden Tick. None bleiben sie
    // wenn das Aircraft-Profil die SimVars nicht meldet (z.B. Fenix
    // ohne erweiterte Profile).
    stats.last_zfw_kg = snap.zfw_kg;
    stats.last_total_weight_kg = snap.total_weight_kg;

    // Block fuel = peak fuel observed across the flight, with a
    // defuel guard. Rationale:
    //   * Tracking the peak captures the loaded amount no matter
    //     when refuelling completes — Fenix's async EFB load,
    //     APU burn during boarding, etc. all become non-issues
    //     because the peak naturally settles on the loaded value.
    //   * BUT pure peak doesn't handle defuelling: pilot loads
    //     4000 kg, realises overload, removes 1000 kg → peak
    //     stays at 4000, Used Fuel = 4000 − landing reads ~1000
    //     kg too high. So we detect a sudden large drop (well
    //     above any realistic engine burn over a tick interval)
    //     and reset the peak to the current value. Threshold
    //     200 kg covers even A380 at takeoff thrust over a 30 s
    //     cruise tick (~210 kg max), so no false positives from
    //     normal operations.
    let live_fuel = snap.fuel_total_kg;
    // ⚠ Nur bis zum Abflug. Der Blocksprit ist der Tankinhalt beim
    // Anlassen; alles danach ist Verbrauch. Lief der Peak weiter, ueberschrieb
    // jede Neubeladung nach der Landung den echten Wert — am 24.09.2026
    // (Joel, OCN 712) die 9546 kg nach einem Sim-Reload in EDDN, und der
    // PIREP meldete 7 kg Verbrauch fuer 834 NM. Dasselbe traefe jeden, der am
    // Gate fuer den naechsten Flug nachtankt, bevor er einreicht.
    if live_fuel > 0.0 && stats.takeoff_at.is_none() {
        // Wert seit v3: bundle/grenzwerte/sprit.json -> defuel_threshold_kg (v2.1.24: 200.0)
        let is_defuel = prev_fuel_kg
            .map(|prev| prev - live_fuel > crate::grenzwerte::grenzwerte().sprit.defuel_threshold_kg.wert)
            .unwrap_or(false);
        if is_defuel {
            tracing::info!(
                prev = prev_fuel_kg,
                live = live_fuel,
                drop = prev_fuel_kg.unwrap_or(0.0) - live_fuel,
                "defuel detected — resetting block_fuel peak baseline"
            );
            stats.block_fuel_kg = Some(live_fuel);
        } else {
            stats.block_fuel_kg = Some(
                stats
                    .block_fuel_kg
                    .map_or(live_fuel, |peak| peak.max(live_fuel)),
            );
        }
    }

    // Peak altitude tracker — every tick, take the max with the live
    // MSL altitude. Reported as the PIREP `level` field.
    let alt = snap.altitude_msl_ft as f32;
    stats.peak_altitude_ft = Some(stats.peak_altitude_ft.map_or(alt, |p| p.max(alt)));

    // Mark the flight as having actually been airborne. Sticky once
    // set — even if the aircraft comes back down (final approach,
    // touch-and-go) the flag stays true. Gates the universal
    // Arrived-fallback so neither GSX repositioning nor a few-meter
    // wackeln at the gate accidentally promotes the FSM to Arrived
    // pre-flight. See WAS_AIRBORNE_AGL_FT for the threshold rationale.
    // `was_airborne` flag — three-layer defense against false positives
    // (live bug 2026-05-03: PMDG B738, MSFS reported AGL=53819 ft for
    // 60 s during scenery load and the flag flipped true; 30 min later
    // the GSX pushback fired the universal Arrived-fallback through
    // the now-poisoned gate):
    //
    //   1. AGL must be in a sane range — > 50 ft AND < 30000 ft.
    //      53819 ft would have been ignored.
    //   2. `block_off_at` must be set — chronologically the aircraft
    //      can't be airborne before it's even been block-off.
    //      Belt-and-braces against any other future loading glitch
    //      that produces sane-looking but pre-flight airborne values.
    //   3. Conditions must hold for WAS_AIRBORNE_DWELL_TICKS in a
    //      row — single-tick glitches don't poison the flag.
    let airborne_now = !snap.on_ground && stats.block_off_at.is_some() && {
        let agl = snap.altitude_agl_ft as f32;
        agl > crate::grenzwerte::grenzwerte().phasen_kern.was_airborne_agl_ft.wert && agl < crate::grenzwerte::grenzwerte().phasen_kern.was_airborne_agl_max_ft.wert
    };
    if airborne_now {
        stats.airborne_dwell_ticks = stats.airborne_dwell_ticks.saturating_add(1);
        if stats.airborne_dwell_ticks >= crate::grenzwerte::grenzwerte().phasen_kern.was_airborne_dwell_ticks.wert {
            stats.was_airborne = true;
        }
    } else {
        stats.airborne_dwell_ticks = 0;
    }

    // Touchdown ring-buffer is now populated by the dedicated 30 Hz
    // sampler (`spawn_touchdown_sampler`) — the streamer ticks every
    // 5–8 s during Final / Landing, way too sparse to reliably catch
    // the actual touchdown subframe. The sampler shares the same
    // `stats.snapshot_buffer` so the FSM still reads the buffer when
    // it transitions to Landing, just with 30× more samples to pick
    // the worst V/S and peak G from.

    let prev_phase = stats.phase;
    let mut next_phase = prev_phase;
    let was_on_ground = stats.was_on_ground.unwrap_or(snap.on_ground);
    // was_parking_brake is no longer consulted by any phase transition
    // (we removed the brake-release-only Pushback trigger), but the
    // tracking field stays — the activity-log change detector below
    // and any future transition that wants brake-state history will
    // pick it up. Acknowledge to silence the unused-variable warning.
    let _ = stats.was_parking_brake;

    // v1.6.1: der fruehere `snap.parking_name`-Weg fuer den Abflug-Stand
    // ist ersatzlos raus — beide Sim-Adapter setzen das Feld hart auf
    // None (SimConnect befuellt es nie, X-Plane hat kein Pendant). Der
    // Abflug-Stand kommt ausschliesslich aus den OSM-Bodendaten
    // (`maybe_spawn_stand_fetch` + Boarding-Retry-Tick im Streamer).

    // v0.5.11: freeze FSM transitions during sim pause / slew mode.
    //
    // Position recording, distance tracking, fuel tracking, peak-altitude
    // tracking, and the phpVMS heartbeat above all continue running.
    // Only the phase classifier pauses with the sim — otherwise:
    //
    //   * Pause: a frozen snapshot with bank > 15° and |VS| < 200 fpm
    //     would let the wall-clock dwell timer falsely classify the
    //     pilot as "in Holding" after 90 real-world seconds (pilot
    //     making coffee while paused mid-turn = falsely identified as
    //     a holding pattern).
    //   * Slew: teleporting from FL340 → 500 ft AGL produces a
    //     massive lost_altitude spike in one tick → would falsely
    //     fire Climb→Descent / Heli-vertical transitions / mid-air
    //     ground-edges. Slew is a debug/positioning tool, not real
    //     flight motion.
    //
    // When the pilot un-pauses or exits slew, FSM resumes from the
    // current state with current snapshot values. Any pending dwell
    // timers (Holding, Touch-and-Go, Go-Around) keep their last
    // value and continue from there.
    // v1.6.12 — Replay-Erkennung ueber die Telemetrie.
    //
    // Zweite Verteidigungslinie: der MSFS-Adapter meldet Replay seit v1.6.12
    // ueber die Flow-Ereignisse des SDK und faltet es in `snap.paused`. Das
    // greift aber nur, wenn der Simulator diese Ereignisse ueberhaupt sendet
    // (MSFS 2024) und das Abonnement geklappt hat. Diese Pruefung braucht
    // nichts davon — sie sieht nur, dass sich Position und gemeldete
    // Geschwindigkeit widersprechen, und gilt damit fuer JEDE Anbindung.
    //
    // Gemessen am eigenen Bestand, bevor sie gebaut wurde: 0 von 875 Fluegen
    // schlagen an. Ein Fehlalarm waere teurer als eine verpasste Erkennung.
    if snap.on_ground {
        // QS-Befund 21.08.2026, der schwerste: das Urteil wurde NUR in der Luft
        // gesetzt. Stand es einmal auf wahr, blieb es nach dem Aufsetzen fuer
        // immer stehen — und der Riegel darunter fror die Phasen-Engine fuer
        // den ganzen Rest des Fluges ein. Ein einziger Fehlalarm im Endanflug
        // haette den Flugbericht unabschliessbar gemacht: kein Uebergang mehr
        // nach Landing, TaxiIn oder Arrived, keine Blockzeit.
        //
        // Am Boden liefert das Verfahren ohnehin keine Belege — dort ist
        // "gemeldete Geschwindigkeit nahe null" der Normalfall und kein
        // Widerspruch. Also beides zuruecksetzen. Ein Replay, das am Boden
        // laeuft, faengt Stufe 1 ab (die Flow-Ereignisse des Simulators).
        stats.replay_proben.clear();
        stats.replay_verdacht = false;
    } else {
        stats
            .replay_proben
            .push_back(replay_erkennung::ReplayProbe {
                t_s: now.timestamp_millis() as f64 / 1000.0,
                lat: snap.lat,
                lon: snap.lon,
                msl_ft: snap.altitude_msl_ft as f64,
                groundspeed_kt: snap.groundspeed_kt as f64,
                ias_kt: snap.indicated_airspeed_kt as f64,
                vs_fps: snap.vertical_speed_fpm as f64 / 60.0,
                on_ground: false,
            });
        // 64 Proben sind bei der langsamsten Kadenz (3 s im Reiseflug) rund
        // drei Minuten — weit mehr als das Verfahren braucht, und immer noch
        // winzig.
        while stats.replay_proben.len() > REPLAY_PROBEN_MAX {
            stats.replay_proben.pop_front();
        }
        let proben: Vec<_> = stats.replay_proben.iter().copied().collect();
        let befund = replay_erkennung::pruefe_replay(&proben);
        if befund.ist_replay != stats.replay_verdacht {
            tracing::warn!(
                verdacht = befund.ist_replay,
                belege = befund.belege,
                errechnet_kt = ?befund.median_errechnete_gs_kt,
                gemeldet_kt = ?befund.median_gemeldete_gs_kt,
                "Replay-Verdacht aus der Telemetrie geaendert"
            );
        }
        stats.replay_verdacht = befund.ist_replay;
    }

    if snap.paused || snap.slew_mode || stats.replay_verdacht {
        // QS-Runde 1 (Pause-Loch): die Versöhner-Uhr ist Wanduhr — eine
        // Sim-Pause unterhalb der Reload-Gap-Schwelle (bis 150 s bei
        // Cruise-Kadenz!) liefe sonst voll in den Dwell ein, und der erste
        // Resume-Tick feuerte mit VOR-Pause-Evidenz (die Schatten-Engine
        // tickt erst NACH step_flight, ihre eigene Gap-Erkennung greift
        // erst > 5 min). Pause unterbricht die Beobachtung → Uhr neu.
        stats.enroute_divergence_since = None;
        return None;
    }

    // v0.12.1 (Stream A LE2): reload-gap detection. A gap between two
    // processed snapshots well above the expected tick interval means the
    // sim was reloading / repositioning — the first snapshot after the gap
    // carries glitch-prone velocity values (verified: BTX8815 EDVE, an
    // 11 s gap produced a spurious gs 2.6 kt tick with zero actual
    // displacement). The Boarding branch uses this to skip velocity-
    // derived transitions on that one suspect tick.
    let interval_secs = position_interval(prev_phase).as_secs_f64();
    let reload_gap_suspect = stats
        .last_step_at
        .map(|prev| {
            is_reload_gap(
                (now - prev).num_milliseconds() as f64 / 1000.0,
                interval_secs,
            )
        })
        .unwrap_or(false);
    stats.last_step_at = Some(now);

    // v0.13.17: Airborne-Rescue-Sicherheitsnetz. Wenn die FSM in einer
    // Boden-/Pre-Takeoff-Phase festhaengt, das Flugzeug aber nachweislich in
    // der Luft ist (on_ground=false + AGL deutlich ueber Boden), zwingen wir
    // die Phase auf Climb. Hintergrund: bei Addons mit kaputten Engine-
    // SimVars (iniBuilds/Aerosoft A340-600 / MSFS 2024 — GENERAL ENG
    // COMBUSTION konstant 0) feuerte der engines-abhaengige Pushback→TaxiOut
    // →Takeoff-Pfad nie → die Phase blieb den GANZEN Flug auf Pushback →
    // kein Approach/Landing → kein Touchdown/Score, PIREP ohne Landedaten
    // (Live IRM1140/IBE778, 2026-06-03). Der N1-Fallback im Adapter behebt
    // die Ursache; dieses Netz ist die addon-agnostische Garantie, dass NIE
    // wieder ein ganzer Flug an einer stuck-on-ground-Phase scheitert.
    // Greift NICHT im Normalfall: dort ist die Phase laengst Climb, bevor
    // AGL 500 ft ueberschreitet. reload_gap_suspect schuetzt vor einem
    // Slew-/Reload-Glitch-Tick.
    if phase_stuck_on_ground_while_airborne(
        prev_phase,
        snap.on_ground,
        snap.altitude_agl_ft,
        reload_gap_suspect,
    ) {
        tracing::warn!(
            ?prev_phase,
            agl_ft = snap.altitude_agl_ft,
            gs_kt = snap.groundspeed_kt,
            "v0.13.17 airborne-rescue: phase stuck on ground while airborne — forcing Climb"
        );
        // v0.16.9: das Rescue überspringt die Takeoff-Phase — ohne dieses
        // Netz blieben alle Takeoff-Latches leer (BCS8: takeoff_fuel_kg
        // None → OFP-Fuel-Sub-Score unbewertbar). get_or_insert-Semantik,
        // Attitude nur vom Sampler — Details am Helper.
        latch_takeoff_stats_if_missing(&mut stats, snap, now);
        // Commit identisch zum normalen Ende von step_flight (siehe unten).
        if should_reset_holding_pending(prev_phase, FlightPhase::Climb) {
            stats.kurs_verlauf.clear();
        }
        stats.phase = FlightPhase::Climb;
        stats.transitions.push((now, FlightPhase::Climb));
        stats.was_on_ground = Some(snap.on_ground);
        stats.was_parking_brake = Some(snap.parking_brake);
        return Some(FlightPhase::Climb);
    }

    // Vref aus dem Flugzeug: jeder Takt in der Luft ueberschreibt, beim
    // Aufsetzen gilt also der letzte Wert vor dem Boden. Durchstart und
    // Aufsetzen setzen ihn zurueck. (QS Codex 06.10.2026: nur im Approach
    // erfasst und PMDG nur der erste Wert — nach Durchstart, FMC-Aenderung im
    // Final oder Neustart im Final galt ein veralteter oder gar kein Wert.)
    if !snap.on_ground {
        if let Some(v) = vref_aus_dem_flugzeug(snap) {
            stats.vref_gemessen = Some(v);
        }
    }

    // v0.16.6: phase-independent approach sampling — see
    // `should_push_approach_sample`. For Approach/Final this is the exact
    // same per-tick push the two arms used to do as their first statement
    // (nothing executes between here and the old call sites); for mis-phased
    // bush flights it keeps the buffer fed on any descending segment below
    // 2000 ft AGL so the sampler-path stability evaluation has data.
    kurs_merken(&mut stats, snap, now);
    if should_push_approach_sample(
        prev_phase,
        stats.was_airborne,
        snap.on_ground,
        snap.altitude_agl_ft,
        snap.vertical_speed_fpm,
    ) {
        push_approach_sample(&mut stats, snap, now);
    }

    // v1.5.0 (#msfs-hud): Landebahn im Anflug schätzen, solange wir noch in
    // der Luft sind. Bewusst hier, an derselben phasenunabhängigen Stelle wie
    // die Approach-Sampling-Klammer darüber, statt in den Approach- und
    // Final-Armen: Bush- und VFR-Anflüge erreichen die beiden Phasen oft gar
    // nicht (siehe `should_push_approach_sample`), hätten sonst also nie eine
    // Bahnanzeige.
    //
    // Sperrreihenfolge: `stats` ist hier gehalten und `navdata` wird darin
    // genommen — dieselbe Richtung wie in `correlate_touchdown_runway`, das
    // ebenfalls aus diesem Aufruf heraus läuft. `flight_info` nimmt die beiden
    // dagegen nacheinander und nie gleichzeitig; keine Inversion.
    // v2.0.18: Zwischenstände für den Sperren-Wächter. Hält dieser Takt die
    // Flugdaten länger als 20 ms, nennt die Meldung, welcher Abschnitt die
    // Zeit gekostet hat (`stats_sperre::Gehalten::zwischenstand`).
    stats.zwischenstand("vorlauf");
    update_predicted_runway(flight, &mut stats, snap);
    stats.zwischenstand("bahn_vorhersage");

    // Match on a local Copy so the rest of the body is free to mutate `stats`.
    match prev_phase {
        FlightPhase::Boarding => {
            // Pushback / departure detection: actual movement is the
            // only reliable trigger. Brake-release alone is NOT enough
            // — pilots routinely release the brake to test controls,
            // prep for taxi, or because GSX flips it during boarding.
            // Without movement, that's still Boarding.
            //
            // Branches:
            //   * Moving with engines OFF → tug is pushing → Pushback
            //   * Moving with engines ON  → straight to TaxiOut
            //                                (powered movement, no push)
            //
            // Both flows MSFS supports get covered: the native Ctrl+P
            // push (often doesn't touch parking brake) and the GSX
            // / hand-flown push (brake released first, then truck).
            // v0.5.11: accept water surface as ground-equivalent for
            // seaplanes. Some sims (MSFS especially) report
            // on_ground=false when an aircraft is sitting on water,
            // because the gear-on-pavement contact model returns
            // false on liquid surfaces. The aircraft is functionally
            // on the surface (AGL ≈ 0, VS ≈ 0) and water-taxi
            // movement is real motion. Without this, seaplanes that
            // boot on water would stay stuck in Boarding forever.
            //
            // The AGL < 5 ft + |VS| < 50 fpm guard ensures we're
            // genuinely at surface level (not low-level approach,
            // which is past Boarding anyway, but defensive).
            let on_surface = snap.on_ground
                || (snap.altitude_agl_ft < 5.0 && snap.vertical_speed_fpm.abs() < 50.0);
            // v0.12.1 (Stream A LE1+LE2): de-glitch the Boarding→Pushback
            // / TaxiOut trigger. A single `groundspeed_kt > 0.5` sample is
            // NOT enough — a sim-reload glitch can spike groundspeed while
            // the aircraft sits still (verified BTX8815: post-reload tick
            // gs 2.6 kt, bit-identical lat/lon, parking brake set). We now
            // require BOTH: gs sustained across 2 ticks AND real position
            // displacement (> MIN_BOARDING_MOVEMENT_M) from a reference
            // point. lat/lon can't be faked by a velocity glitch. A
            // reload-suspect tick (LE2) re-baselines the reference and
            // triggers nothing.
            let gs_over_threshold = snap.groundspeed_kt > 0.5;
            let real_movement = if reload_gap_suspect {
                stats.boarding_ref_pos = Some((snap.lat, snap.lon));
                stats.prev_gs_over_threshold = false;
                false
            } else {
                if stats.boarding_ref_pos.is_none() {
                    stats.boarding_ref_pos = Some((snap.lat, snap.lon));
                }
                let moved_m = stats
                    .boarding_ref_pos
                    .map(|(rlat, rlon)| ::geo::distance_m(rlat, rlon, snap.lat, snap.lon))
                    .unwrap_or(0.0);
                let moved = boarding_real_movement(
                    on_surface,
                    gs_over_threshold,
                    stats.prev_gs_over_threshold,
                    moved_m,
                );
                stats.prev_gs_over_threshold = gs_over_threshold;
                moved
            };
            if real_movement {
                stats.block_off_at = Some(now);
                // Note: block_fuel_kg is tracked as a running peak
                // (see top of step_flight), not captured here. APU
                // burn during boarding would otherwise eat into the
                // captured value before block-off ever fires.
                // v0.3.0: Loadsheet-Snapshot ins Activity-Log. Einmalig
                // beim Block-off — gibt dem PIREP-Audit-Trail einen
                // klaren "was war geladen, als wir losrollten" Eintrag.
                if !stats.loadsheet_logged_at_blockoff {
                    let block = stats.block_fuel_kg.unwrap_or(snap.fuel_total_kg);
                    let zfw = snap.zfw_kg;
                    let tow = snap.total_weight_kg;
                    let plan_block = stats.planned_block_fuel_kg;
                    // v0.3.0: Loadsheet als kompakte Zeile in den
                    // pending_acars_logs Buffer pushen. Streamer drainet
                    // den nächsten Tick und mirrort in Activity-Log
                    // (Cockpit-UI) UND ACARS-Log (phpVMS-PIREP). Bewusst
                    // einzeilig — der Drain-Pfad unterstützt keine
                    // 2-zeiligen Display-Logs.
                    // v0.3.0: ohne Emoji-Icon — wird in phpVMS-PIREP-
                    // Detailseite als verzerrter Glyph gerendert (Font-
                    // Stack hat dort keinen Emoji-Font).
                    let mut line = String::from("Loadsheet @ Block-off — Block ");
                    line.push_str(&format!("{:.0} kg", block));
                    if let Some(p) = plan_block {
                        let delta = block - p;
                        // „Diff" statt „Δ": Die Zeile erscheint im Aktivitaetsprotokoll in
                        // B612 Mono, der das Zeichen fehlt, und geht gleichlautend als
                        // ACARS-Log an phpVMS (Entscheidung Thomas, 21.09.2026: ueberall
                        // gleich, nicht zwei Wortlaute fuer dieselbe Zeile).
                        line.push_str(&format!(" (Plan {:.0} kg, Diff {:+.0})", p, delta));
                    }
                    if let Some(z) = zfw {
                        line.push_str(&format!(" · ZFW {:.0} kg", z));
                    }
                    if let Some(t) = tow {
                        line.push_str(&format!(" · TOW {:.0} kg", t));
                    }
                    stats.pending_acars_logs.push(line);
                    // v0.3.0 Bonus 1: "Über-Tankt"-Hinweis als zweiter
                    // Eintrag, falls IST-Block > Plan-Block + Reserve +
                    // 500 kg Toleranz. Sanft, nicht blockierend.
                    if let (Some(p_block), Some(p_reserve)) =
                        (stats.planned_block_fuel_kg, stats.planned_reserve_kg)
                    {
                        let threshold = p_block + p_reserve + 500.0;
                        if block > threshold {
                            let extra = block - p_block;
                            stats.pending_acars_logs.push(format!(
                                "💡 Über-tankt — Sehr viel Sprit an Bord ({:+.0} kg über Plan + Reserve), höherer Burn unterwegs zu erwarten.",
                                extra
                            ));
                        }
                    }
                    stats.loadsheet_logged_at_blockoff = true;
                }
                // v0.15.13: Pushback wird jetzt AUCH erkannt, wenn die
                // Triebwerke bereits laufen — entscheidend ist RÜCKWÄRTS-
                // Bewegung (Tug schiebt; vbz < 0), nicht der Motorstatus. Vorher
                // sprang ein Pushback mit bereits gestarteten Triebwerken direkt
                // auf TaxiOut. Liefert der Sim kein vbz, bleibt es beim alten
                // engines==0-Kriterium. Block-Off-Zeit ist davon UNBERÜHRT (oben
                // bereits gesetzt) — es geht nur ums Phasen-Label.
                next_phase =
                    if snap.engines_running == 0 || moving_backward(snap.velocity_body_z_fps) {
                        FlightPhase::Pushback
                    } else {
                        FlightPhase::TaxiOut
                    };
            }
            // v0.5.10/11: pure-hover / glider direct-launch escape hatch.
            // Aircraft that lift off straight from the gate without
            // ever entering TaxiOut would otherwise stay in Boarding
            // forever. Detect the on_ground edge and skip straight to
            // Takeoff. Covers:
            //   * Helicopters that take off vertically from the gate
            //     (GS = 0, never triggers Boarding → TaxiOut on
            //     groundspeed alone)
            //   * Gliders that get winched from the boarding spot
            //     (engines = 0 throughout)
            //
            // v0.5.11 widening: dropped engines > 0 requirement so
            // glider winch launches fire correctly. AGL > 3 ft +
            // VS > 100 fpm remain as the safety net against sim-
            // glitch on_ground-flicker during boarding.
            // v0.12.1 (Stream A LE2): a reload-suspect tick must not drive
            // the velocity-derived vertical-takeoff hatch either.
            else if !reload_gap_suspect
                && was_on_ground
                && !snap.on_ground
                && snap.altitude_agl_ft > 3.0
                && snap.vertical_speed_fpm > 100.0
            {
                next_phase = FlightPhase::Takeoff;
                latch_takeoff_stats(&mut stats, snap, now);
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    "vertical takeoff from boarding (helicopter pure-hover) — Boarding → Takeoff"
                );
            }
        }
        FlightPhase::Pushback => {
            // Pushback → TaxiOut Übergang. Echter Ablauf am Gate:
            //   1. Tug schiebt zurück
            //   2. Tug-Crew koppelt ab (PUSHBACK STATE = 3)
            //   3. Pilot steht ~10 s mit gesetzter Bremse, hat Funk,
            //      checkt Freigabe
            //   4. Pilot löst Bremse + Schub → Flugzeug rollt an
            // Erst dann ist es "Taxi". Vorher würde der Tug-Schub bei
            // > 3 kt fälschlicherweise schon TaxiOut triggern.
            //
            // Implementierung als 3-Stufen-Gate:
            //   * `tug_done` = MSFS meldet PUSHBACK STATE == 3 — ABER nur
            //                  gültig wenn vorher mal 0/1/2 gesehen wurde
            //                  (`saw_pushback_state_active`); sonst ist 3
            //                  einfach der Default und kein Pushback war
            //                  je aktiv (GSX-Pushback / manuell)
            //   * `stopped`  = Flugzeug ist nach tug_done zum Stillstand
            //                  gekommen (gs < 0.5 kt). Wir merken uns
            //                  den Zeitpunkt in `pushback_stopped_at`.
            //   * `dwell_ok` = Stillstand mind. 10 s gehalten.
            //   * `powered_taxi` = Triebwerke + gs > 3 kt
            // Nur wenn alles stimmt, springt die Phase auf TaxiOut.
            //
            // Sims ohne PUSHBACK STATE (X-Plane / manche Add-ons) UND
            // Aircraft die den State nie auf 0/1/2 setzen (z.B. Aerosoft
            // A340-600 Pro mit GSX-Pushback) fallen auf die alte
            // Heuristik zurück — sonst würden sie für immer in Pushback
            // stecken bleiben (siehe URO 913 ZWWW→EHBK 2026-05-09:
            // 9 h hängen geblieben weil pushback_state konstant 3 war).
            // Wert seit v3: bundle/grenzwerte/phasen_kern.json -> pushback_dwell_secs (v2.1.24: 10)
            let engines_running = engines_effectively_running(&stats, snap, now);
            // v0.12.1 (Stream A LE2): gate the velocity-derived TaxiOut
            // signal on `!reload_gap_suspect` — a reload glitch tick must
            // not flip Pushback→TaxiOut any more than it may flip
            // Boarding→Pushback.
            let powered_taxi = powered_taxi_move(
                reload_gap_suspect,
                snap.on_ground,
                engines_running,
                snap.groundspeed_kt,
                snap.velocity_body_z_fps,
            );
            // tug_done greift nur wenn der State MAL aktiv (≠3) war
            let tug_done = snap.pushback_state == Some(3) && stats.saw_pushback_state_active;
            let no_tug_ever = snap.pushback_state.is_none()
                || (snap.pushback_state == Some(3) && !stats.saw_pushback_state_active);

            if tug_done {
                // Stillstand nach Tug-Ab: Timestamp setzen (einmal).
                if snap.groundspeed_kt < 0.5 && stats.pushback_stopped_at.is_none() {
                    stats.pushback_stopped_at = Some(now);
                }
                let dwell_ok = stats
                    .pushback_stopped_at
                    .map(|t| (now - t).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.pushback_dwell_secs.wert)
                    .unwrap_or(false);
                if dwell_ok && powered_taxi {
                    stats.pushback_stopped_at = None;
                    next_phase = FlightPhase::TaxiOut;
                }
            } else if no_tug_ever && powered_taxi {
                // Kein PUSHBACK STATE vom Sim verfügbar (X-Plane) ODER
                // der State war nie != 3 (GSX-Pushback / manuell).
                // Alte Fallback-Logik: bewegt sich + Triebwerke an = TaxiOut.
                next_phase = FlightPhase::TaxiOut;
            } else if stats.pushback_status_klemmt
                && pushback_haengt_aber_rollt(
                    snap.pushback_state,
                    powered_taxi,
                    snap.groundspeed_kt,
                    snap.velocity_body_z_fps,
                )
            {
                // Der dritte Fall (15.10.2026, Joel, MSFS 2020): Der Status
                // bleibt dauerhaft „Pushback läuft" (0) und erreicht nie 3 —
                // weder `tug_done` noch `no_tug_ever` greifen. Bis hier hing
                // die Phase bis zur Luftrettung auf Pushback (31 Flüge in
                // 30 Tagen). Rollt das Flugzeug aber `PUSHBACK_HAENGT_S`
                // lang vorwärts mit eigenen Triebwerken schneller als jeder
                // Schlepper schiebt, rollt es selbst.
                let seit = *stats.pushback_vorwaerts_seit.get_or_insert(now);
                if (now - seit).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.pushback_haengt_s.wert {
                    stats.pushback_vorwaerts_seit = None;
                    next_phase = FlightPhase::TaxiOut;
                }
            }
            if next_phase != FlightPhase::Pushback
                || !stats.pushback_status_klemmt
                || !pushback_haengt_aber_rollt(
                    snap.pushback_state,
                    powered_taxi,
                    snap.groundspeed_kt,
                    snap.velocity_body_z_fps,
                )
            {
                stats.pushback_vorwaerts_seit = None;
            }
        }
        FlightPhase::TaxiOut => {
            // v0.19.3: a takeoff roll is recognised by EVIDENCE OF A TAKEOFF,
            // not by taxi speed alone — see `takeoff_roll_detected`.
            //
            // Field report (Thomas, DLH367 KCLT→EDDM, ToLiss A346, 2026-07-12):
            // the FSM announced "TakeoffRoll" at 30 kt while the aircraft was
            // still taxiing, fell back to TaxiOut two minutes later, and only
            // then rolled for real. A heavy widebody simply taxis faster than 30
            // kt. The flip-flop also re-fired the block-off transition, so
            // "Fuel & Weight @ Block-off" landed in the pilot's log three times.
            if takeoff_roll_detected(&mut stats, snap, now) {
                next_phase = FlightPhase::TakeoffRoll;
                // Peak-/Plateau-State dieser Rolle frisch initialisieren
                // (Backtrack-Rückkante, s. takeoff_roll_stall_check).
                stats.takeoff_roll_peak_kt = snap.groundspeed_kt;
                stats.takeoff_roll_stall_since = None;
            }
            // v0.5.10/11: helicopter / VTOL / glider / seaplane
            // escape hatch.
            //
            // Conventional aircraft trigger TakeoffRoll via GS > 30 kt,
            // then Takeoff via the on_ground edge. Several classes of
            // aircraft skip TakeoffRoll entirely:
            //   * Helicopters / VTOL: take off vertically (GS stays
            //     below 30 kt the whole time on the ground)
            //   * Gliders on aerotow: GS > 30 kt but engines = 0
            //     (the glider's own engine is off, the towplane pulls)
            //   * Gliders on winch: very fast vertical climb, engines = 0
            //   * Seaplanes that report on_ground=true on water:
            //     same path as conventional but engines might briefly
            //     show 0 in some sims
            //
            // For all of these we want: when in TaxiOut and the
            // on_ground edge fires (ground → air), promote to Takeoff
            // directly. The AGL > 5 ft + VS > 100 fpm guards confirm
            // a REAL liftoff (not sim-glitch on_ground-flicker on
            // bumpy runways / PMDG texture quirks).
            //
            // v0.5.11 widening: dropped the engines > 0 requirement
            // so glider tow / winch launches also fire correctly.
            // The AGL/VS guards remain as the false-positive safety
            // net (a single-frame on_ground-flicker can't satisfy
            // both AGL > 5 ft AND VS > 100 fpm).
            else if (was_on_ground
                && !snap.on_ground
                && snap.altitude_agl_ft > 5.0
                && snap.vertical_speed_fpm > 100.0)
                // v0.5.11: seaplane catchall — sims that report
                // on_ground=false on water never produce the ground
                // edge above. Once the seaplane lifts off the water
                // (AGL > 50 ft + VS > 100 fpm + actively airborne),
                // fire Takeoff regardless of edge. AGL > 50 is a
                // strict guard against sim-spawn-at-altitude
                // false-fires; a real water takeoff easily exceeds
                // 50 ft AGL within seconds of breaking the surface.
                || (!snap.on_ground
                    && snap.altitude_agl_ft > 50.0
                    && snap.vertical_speed_fpm > 100.0
                    && !snap.slew_mode
                    && !snap.paused)
            {
                next_phase = FlightPhase::Takeoff;
                takeoff_latch_oder_weiterflug(&mut stats, snap, now);
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    "non-conventional takeoff detected (heli / glider / seaplane) — TaxiOut → Takeoff"
                );
            }
        }
        FlightPhase::TakeoffRoll => {
            // PMDG capture (Phase 5.6): TakeoffRoll is the right
            // moment to record V-speeds + planned TO-flaps + the
            // takeoff-config-warning-was-active flag, because the
            // pilot completed PERF-INIT before this point. We
            // capture continuously through the roll (rather than
            // once at entry) because V-speeds may not have been
            // entered until the very last moment.
            if let Some(p) = &snap.pmdg {
                if stats.pmdg_takeoff_flaps_planned.is_none() {
                    stats.pmdg_takeoff_flaps_planned = p.fmc_takeoff_flaps_deg;
                }
                if let (Some(v1), Some(vr), Some(v2)) = (p.fmc_v1_kt, p.fmc_vr_kt, p.fmc_v2_kt) {
                    if stats.pmdg_v_speeds_takeoff.is_none() {
                        stats.pmdg_v_speeds_takeoff = Some((v1, vr, v2));
                    }
                }
                if p.takeoff_config_warning {
                    stats.pmdg_takeoff_config_warning_seen = true;
                }
                if stats.pmdg_fmc_flight_number.is_none() && !p.fmc_flight_number.is_empty() {
                    stats.pmdg_fmc_flight_number = Some(p.fmc_flight_number.clone());
                }
            }

            if was_on_ground && !snap.on_ground {
                next_phase = FlightPhase::Takeoff;
                takeoff_latch_oder_weiterflug(&mut stats, snap, now);
            } else {
                // Rückkante (fehlte bisher — der Arm ging nur vorwärts):
                // nachhaltig langsam + am Boden → zurück zu TaxiOut. Behebt
                // TAKEOFF_ROLL_STUCK (Backtrack trippt TakeoffRoll, dann
                // Ausrichten/Warten). Siehe takeoff_roll_demote.
                let (demote, slow_since) = takeoff_roll_demote(
                    snap.on_ground,
                    snap.groundspeed_kt,
                    stats.takeoff_roll_slow_since,
                    now,
                );
                stats.takeoff_roll_slow_since = slow_since;
                // Zweite Rückkante: Backtrack/Fehl-Trip plateauisiert bei
                // Taxi-Speed (nie ≥ 45 kt) → zurück zu TaxiOut, lange bevor
                // die GS<20-Kante greifen würde. Siehe takeoff_roll_stall_check.
                let (stall_demote, peak, stall_since) = takeoff_roll_stall_check(
                    snap.on_ground,
                    snap.groundspeed_kt,
                    stats.takeoff_roll_peak_kt,
                    stats.takeoff_roll_stall_since,
                    now,
                );
                stats.takeoff_roll_peak_kt = peak;
                stats.takeoff_roll_stall_since = stall_since;
                if let Some(target) = demote.or(stall_demote) {
                    next_phase = target;
                }
            }
        }
        FlightPhase::Takeoff => {
            // Helicopters/VTOL routinely fly an entire leg below the 500 ft
            // fixed-wing Climb gate (pad-to-pad hops, survey, EMS), which
            // would strand them in Takeoff — a phase EXCLUDED from
            // `is_flight_phase`, so the touchdown sampler never captures their
            // landing. A low gate moves a rotorcraft into Climb (a flight
            // phase) once it is clearly airborne and translating; the sampler
            // + the category-aware validator then capture the set-down. Fixed-
            // wing keeps 500 ft unchanged.
            let climb_gate_ft = takeoff_climb_gate_ft(category);
            if snap.altitude_agl_ft > climb_gate_ft {
                next_phase = FlightPhase::Climb;
                // v0.5.9: reset climb peak so the new climb segment
                // starts fresh (handles re-takeoffs after a divert).
                stats.climb_peak_msl = None;
            }
        }
        // Reiseband (17.10.2026, „Stück 2"): Steig-, Reise- und Sinkflug
        // entscheidet die v2-Engine (phase_v2.rs) — fenster-basiert statt
        // aus Einzelwerten. v1 übernimmt ihre Entscheidung des Vortakts
        // (`reiseband_phase`, höchstens ein Takt ≈ 3 s später). Die alten
        // Einzelwert-Regeln (Climb→Cruise bei |V/S| < 200, Cruise→Descent
        // nach 5000 ft Verlust, B-003 Descent→Cruise, v0.7.17-Dwell) sind
        // entfallen: Sie ließen die Phase in 40 % der Flüge zwischen Cruise
        // und Descent flattern und den Reiseflug im Steigflug zu früh
        // beginnen (Korpus 447 Flüge, 30 Tage). v2 lief seit v0.19.0 als
        // Anzeige im Feld und entscheidet dieselben Fälle richtig.
        FlightPhase::Climb => {
            if let Some(p) = reiseband_phase(&stats) {
                next_phase = p;
            }
        }
        FlightPhase::Cruise => {
            // Schnellweg in den Sinkflug wie bisher (v0.5.4): tief und
            // deutlich sinkend. BEWUSST nicht die 5000-ft-Anflug-Kante —
            // Flüge, die knapp über 5000 ft über Gelände reisen, stünden
            // sonst bei jedem leichten Sinken eine Stunde im Anflug
            // (Korpus 17.10.2026: vier Flüge 12–53 min zu früh).
            if snap.vertical_speed_fpm < -500.0 && snap.altitude_agl_ft < 3000.0 {
                next_phase = FlightPhase::Descent;
            } else if let Some(p) = reiseband_phase(&stats) {
                next_phase = p;
            } else if check_holding_entry(&mut stats, snap, now) {
                stats.previous_phase_before_holding = Some(FlightPhase::Cruise);
                next_phase = FlightPhase::Holding;
            }
        }
        FlightPhase::Holding => {
            // v0.5.11: detect holding pattern exit. Three exit modes:
            //
            //   1. Active descent (vs < -300 fpm + AGL < 5000 ft) —
            //      ATC has cleared us out of the hold for the
            //      approach. Jump directly to Approach.
            //   2. Low-altitude descent at any rate — going down
            //      toward the runway, definitely not holding anymore.
            //   3. Sustained straight-and-level (bank < 5° for
            //      HOLDING_EXIT_DWELL_SECS) — resumed normal flight.
            //      Restore the phase we came from (Cruise or Approach).
            let descent_resumed = snap.vertical_speed_fpm < -300.0 && snap.altitude_agl_ft < 5000.0;
            if descent_resumed {
                next_phase = FlightPhase::Approach;
                stats.previous_phase_before_holding = None;
                // Reset approach VS-min tracker (v0.5.5) for fresh
                // approach measurement post-hold.
                anflug_beginnt(&mut stats);
            } else if kurs_ruhig(&stats, now) {
                // 17.10.2026: Ende erst nach 3 min (fast) geradeaus — länger
                // als jeder Schenkel. Vorher genügten 30 s Flügel waagerecht.
                {
                    let prev = stats
                        .previous_phase_before_holding
                        .unwrap_or(FlightPhase::Cruise);
                    // Zurück in die Phase vor der Warteschleife — im
                    // Reiseband aber in die AKTUELLE v2-Phase: Hat v2 während
                    // der Warteschleife z. B. vom Sinkflug auf Cruise
                    // gewechselt, gäbe es sonst einen Takt Descent und dann
                    // die Übernahme (Codex-QS 17.10.2026).
                    next_phase = match (prev, stats.shadow_phase) {
                        (
                            FlightPhase::Climb | FlightPhase::Cruise | FlightPhase::Descent,
                            Some(v2 @ (FlightPhase::Climb | FlightPhase::Cruise | FlightPhase::Descent)),
                        ) => v2,
                        _ => prev,
                    };
                    stats.previous_phase_before_holding = None;
                }
            }
        }
        FlightPhase::Descent => {
            // Approach starts when we're both low *and* still
            // descending. Without the V/S guard a brief AGL dip
            // during a mountain overflight (Alps, Rockies) wrongly
            // triggered Approach mid-cruise. The aircraft is on
            // approach when it's actually heading down toward the
            // destination, not when it happened to skim a peak.
            // Bleibt v1s eigene Kante IM SELBEN TAKT (nicht per Übernahme
            // einen Takt später): an ihr beginnt der Anflug-Puffer der
            // Stabilitätsnote.
            if anflug_kante(snap) {
                anflug_beginnt(&mut stats);
                next_phase = FlightPhase::Approach;
            } else if let Some(p) = reiseband_phase(&stats) {
                next_phase = p;
            } else if check_holding_entry(&mut stats, snap, now) {
                // Warteschleifen liegen oft im Sinkflug (ATC hält auf
                // FL150). Bis 17.10.2026 wurden sie nur erkannt, weil das
                // Level-off fälschlich auf Cruise sprang; seit v2 dort
                // Descent hält, prüft der Descent-Arm selbst.
                stats.previous_phase_before_holding = Some(FlightPhase::Descent);
                next_phase = FlightPhase::Holding;
            }
        }
        FlightPhase::Approach => {
            // Approach-stability samples (V/S + bank stddev → "Approach
            // Stability" sub-score) are pushed by the central
            // phase-independent call right before this match (v0.16.6) —
            // same per-tick cadence as the old arm-local call here.
            // Track lowest AGL during approach for go-around detection.
            update_lowest_approach_agl(&mut stats, snap);

            // PMDG capture (Phase 5.6): on Approach the pilot has
            // typically dialed in landing flaps + VREF in the FMC,
            // and chosen autobrake. Capture continuously through
            // approach so a last-minute change still gets recorded.
            if let Some(p) = &snap.pmdg {
                if stats.pmdg_landing_flaps_planned.is_none() {
                    stats.pmdg_landing_flaps_planned = p.fmc_landing_flaps_deg;
                }
                if stats.pmdg_vref_at_landing.is_none() {
                    stats.pmdg_vref_at_landing = p.fmc_vref_kt;
                }
                // Autobrake: keep capturing until touchdown so we
                // record the LAST setting before landing.
                if !p.autobrake_label.is_empty() && p.autobrake_label != "?" {
                    stats.pmdg_autobrake_at_landing = Some(p.autobrake_label.clone());
                }
            }

            // Helicopters arrest a descent into a hover before set-down
            // (collective cushion), which the fixed-wing detector reads as a
            // climb-away go-around and reverts the phase to Climb. Suppress it
            // for rotorcraft so a normal confined-area / pad approach is not
            // falsely logged as a missed approach.
            let ga_phase = if category.is_rotorcraft() {
                None
            } else {
                check_go_around(&mut stats, snap, now)
            };
            if let Some(ga_phase) = ga_phase {
                next_phase = ga_phase;
            } else if snap.altitude_agl_ft < 700.0 && final_in_platznaehe(&stats, snap) {
                // 1500 ft AGL was too eager — pilots reported a 3 min
                // "Final" segment because most aircraft intercept the
                // ILS at that altitude and still have several miles to
                // run. Real-world Final starts ~700 ft AGL (FAF crossed
                // for non-precision, decision height area for ILS).
                next_phase = FlightPhase::Final;
            } else if check_holding_entry(&mut stats, snap, now) {
                // v0.5.11: Approach-level holding (low-altitude hold).
                // VATSIM ATC frequently issues "hold over WAYPOINT"
                // during inbound sequencing — aircraft circles at
                // ~5000 ft for 5-15 minutes. Without this we'd just
                // see endless Approach phase with no signal.
                stats.previous_phase_before_holding = Some(FlightPhase::Approach);
                next_phase = FlightPhase::Holding;
            }
        }
        FlightPhase::Final => {
            // Approach-stability sampling continues through the final
            // segment too (lots of pilots stabilise late; measuring only
            // Approach would underweight the truly important last 700 ft
            // AGL) — handled by the central phase-independent call right
            // before this match (v0.16.6), same cadence as before.
            update_lowest_approach_agl(&mut stats, snap);
            // Rotorcraft hover-arrest is not a go-around (see Approach arm) —
            // suppress so a heli that stabilises into a hover before set-down
            // stays in Final and its touchdown is captured by Path B.
            let ga_phase = if category.is_rotorcraft() {
                None
            } else {
                check_go_around(&mut stats, snap, now)
            };
            if let Some(ga_phase) = ga_phase {
                next_phase = ga_phase;
            }
            // Approach runway: snapshot whatever ATC currently has us
            // cleared for. May still change before touchdown, so we
            // refresh until wheels are down.
            if let Some(rw) = snap.selected_runway.as_ref().filter(|s| !s.is_empty()) {
                stats.approach_runway = Some(rw.clone());
            }
            if !was_on_ground && snap.on_ground {
                next_phase = FlightPhase::Landing;

                // CRITICAL: anchor the window around the ACTUAL
                // touchdown moment, not `now`. The position-streamer
                // ticks every 5 s during Landing phase, so the on-
                // ground edge is detected up to 5 s after the wheels
                // actually touched. The 30 Hz touchdown-sampler has
                // been faithfully recording samples in the meantime,
                // so the first on-ground sample in the buffer IS the
                // touchdown moment. Using `now` instead would give a
                // window over the rollout, where V/S is near zero —
                // exactly the "-48 fpm" bug.
                let actual_td_at = stats
                    .snapshot_buffer
                    .iter()
                    .find(|s| s.on_ground)
                    .map(|s| s.at)
                    .unwrap_or(now);
                stats.landing_at = Some(actual_td_at);

                // Snapshot the buffer entry AT the actual touchdown
                // moment. Used below for IAS / pitch / heading
                // capture so they reflect the real TD-instant values,
                // not the post-rollout numbers that `snap` carries
                // (current snap is up to 5 s late on the streamer
                // tick, time enough for a typical airliner to bleed
                // ~17 kt IAS and derotate the nose to negative pitch).
                // Live bug 2026-05-03: pilot at 135 kt touchdown saw
                // "IAS bei TD: 118 kt" + "pitch -4.9°" because we
                // were reading post-derotation values.
                let td_buf_sample = touchdown_buffer_sample(&stats);

                // v0.5.12: SIM-AWARE touchdown capture — different
                // priority chains for MSFS vs X-Plane.
                //
                // The earlier v0.5.11 unified chain (AGL-Δ primary
                // for both sims) was a scope error — MSFS already
                // had a perfect source via the `PLANE TOUCHDOWN
                // NORMAL VELOCITY` SimVar (frame-accurate, latched
                // at the exact contact moment by the sim itself).
                // Routing MSFS through the AGL estimator with strict
                // AGL ≤ 5 guards rejected it (MSFS reports AGL ≈ 14 ft
                // even when on_ground=true — sim quirk), the chain
                // fell through to the X-Plane-style sampler which
                // captured a transient -1173 fpm spike from MSFS's
                // gear-contact rebound oscillation. Pilot's
                // 2026-05-07 LH595 DNAA→EDDF report: actual rate
                // ~-419/-560 fpm (Volanta + LHA + MSFS-latched),
                // we reported -1173. Bug class: cross-contamination
                // from a refactor that should've been X-Plane only.
                //
                // Clean architecture v0.5.12:
                //
                //   MSFS:
                //     1. snap.touchdown_vs_fpm (PRIMARY — latched
                //        SimVar, frame-accurate, gold standard)
                //     2. AGL-Δ estimator (sanity fallback if SimVar
                //        wasn't populated for some reason)
                //     3. Buffer scan
                //
                //   X-Plane:
                //     1. AGL-Δ estimator (PRIMARY — LandingRate-1
                //        algorithm, Volanta uses the same)
                //     2. sampler_touchdown_vs_fpm (FALLBACK — uses
                //        fnrml_gear edge from v0.4.4 sampler)
                //     3. Buffer scan
                //     4. low_agl_vs_min_fpm
                //
                // Both paths apply negative_only filtering.
                if let Some(sampler_at) = stats.sampler_touchdown_at {
                    stats.landing_at = Some(sampler_at);
                }
                let actual_td_at = stats.landing_at.unwrap_or(actual_td_at);

                let is_msfs = matches!(snap.simulator, Simulator::Msfs2020 | Simulator::Msfs2024);
                let is_xplane = matches!(snap.simulator, Simulator::XPlane11 | Simulator::XPlane12);

                // v0.5.13: AGL-derivative estimators run once.
                //
                //   * `agl_estimate_xp` — Lua-style 30-sample window
                //     (LandingRate-1 algorithm, Volanta-aligned).
                //     Used as PRIMARY for X-Plane.
                //   * `agl_estimate_msfs` — original time-tier window
                //     (multi-tier 750ms/1s/1.5s/2s/3s/12s with
                //     min-sample guards). Used as FALLBACK for MSFS
                //     when the latched SimVar is null. v0.5.12-validated
                //     against real pilot flights — leave alone.
                //
                // Both have identical AGL guards (TD ≤ 5 ft / on_ground
                // = true, window-start ≤ 250 ft) — they differ only
                // in WINDOW SIZING strategy. The Lua method adapts
                // to sample density automatically; the time-tier
                // method walks fixed time windows.
                let agl_estimate_xp =
                    estimate_xplane_touchdown_vs_lua_style(&stats.snapshot_buffer, actual_td_at);
                let agl_estimate_msfs =
                    estimate_xplane_touchdown_vs_from_agl(&stats.snapshot_buffer, actual_td_at);

                // Tight buffer-window scan (used by both paths as a
                // fallback — AGL ≤ 250 ft filter prevents pre-flare
                // contamination).
                let half_window = chrono::Duration::milliseconds(crate::grenzwerte::grenzwerte().landung.touchdown_vs_window_ms.wert / 2);
                let vs_window_start = actual_td_at - half_window;
                let vs_window_end = actual_td_at + half_window;
                let buffered_vs_min_raw: f32 = stats
                    .snapshot_buffer
                    .iter()
                    .filter(|s| s.at >= vs_window_start && s.at <= vs_window_end)
                    .filter(|s| s.agl_ft <= crate::grenzwerte::grenzwerte().landung.td_agl_max_at_window_start_ft.wert)
                    .map(|s| s.vs_fpm)
                    .fold(f32::INFINITY, f32::min);
                let buffered_vs_min = if buffered_vs_min_raw.is_finite() {
                    Some(buffered_vs_min_raw)
                } else {
                    None
                };

                // v1.6.9: die Referenz des Simulators separat festhalten,
                // unabhaengig davon, welcher Kandidat unten gewinnt. MSFS
                // meldet mit `PLANE TOUCHDOWN NORMAL VELOCITY` seine eigene
                // Aufsetzgeschwindigkeit; ueber 537 Landungen ohne Hopser
                // liegt unsere Hoehenkurve dagegen im Median +4 fpm (also
                // unverzerrt), streut aber deutlich — 45 % innerhalb 50 fpm,
                // 70 % innerhalb 100 fpm. Der Wert wird NUR angezeigt und
                // geht in keine Punktzahl ein; er ist die erste unabhaengige
                // Gegenprobe, die wir ueberhaupt haben. X-Plane liefert
                // nichts Vergleichbares, dort bleibt das Feld leer.
                //
                // v1.6.10 (Nachgang zur Messung): NUR einen frisch
                // gesetzten Wert nehmen. Der SimVar ist ein Latch — er
                // behaelt nach einem Touch-and-Go den Wert des vorigen
                // Aufsetzens, bis der Simulator ihn ueberschreibt. Und
                // das dauert: gemessen ueber 658 MSFS-Landungen kommt der
                // neue Wert im Median erst 0,29 s nach dem Aufsetzen an,
                // in 10 % der Faelle sogar erst nach 3 s. Wer im Moment
                // des Aufsetzens einfach liest, was dasteht, bekommt bei
                // 8 % der Landungen die Zahl der VORIGEN Landung — und
                // stellt sie dem Piloten als Referenz hin.
                //
                // Deshalb: den Stand vor dem Aufsetzen merken und erst
                // eine Aenderung uebernehmen. Aendert er sich nicht,
                // bleibt das Feld leer statt falsch. (Dasselbe macht das
                // Werkzeug von Arderos, dort mit einem 2-Sekunden-Fenster;
                // unsere Messung sagt, dass 2 s zu kurz waeren.)
                // Wert seit v3: bundle/grenzwerte/abgabe.json -> sim_referenz_fenster_s (v2.1.24: 4)
                if stats.landing_sim_referenz_fpm.is_none() {
                    if let Some(kandidat) = negative_only(snap.touchdown_vs_fpm) {
                        let frisch = match stats.sim_referenz_vor_aufsetzen {
                            Some(vorher) => (kandidat - vorher).abs() > 0.5,
                            None => true,
                        };
                        let im_fenster = stats
                            .landing_at
                            .map(|td| (now - td).num_seconds() <= crate::grenzwerte::grenzwerte().abgabe.sim_referenz_fenster_s.wert)
                            .unwrap_or(true);
                        if frisch && im_fenster {
                            stats.landing_sim_referenz_fpm = Some(kandidat);
                        }
                    }
                }

                // ⚠ EINE Entscheidung fuer Wert UND Quelle.
                //
                // Bis v1.7.12 standen hier ZWEI Ketten: eine, die den
                // Wert rechnete, und darunter ein "Spiegel der
                // Priority-Chain oben", der die Quellenangabe rechnete.
                // Zwei Implementierungen derselben Entscheidung — und
                // sie sind auseinandergelaufen.
                //
                // Aufgefallen an SRR318 (LIMJ, 31.08.2026): Die Quelle
                // meldete `agl_estimate_xp_or_last_low`, ein Zweig, der
                // `est.min(last)` rechnet und bei einem Schaetzwert von
                // −1389 fpm NIEMALS 0 ergeben kann. Gespeichert war
                // trotzdem 0 — und daran hing alles: phpVMS bekam
                // `landing_rate = NULL`, der Score wurde 0, die Wartung
                // feuerte nicht. Waehrend 12,2 g und 4 Meganewton
                // Fahrwerkskraft danebenstanden.
                //
                // Jetzt liefert jeder Zweig das Paar. Wert und Angabe
                // koennen nicht mehr verschiedene Wege beschreiben.
                let quellen = Sinkratenquellen {
                    gerastet: negative_only(snap.touchdown_vs_fpm),
                    schaetzer_xp: agl_estimate_xp.map(|e| (e.fpm, e.window_ms)),
                    schaetzer_msfs: agl_estimate_msfs.map(|e| e.fpm),
                    abtaster: negative_only(stats.sampler_touchdown_vs_fpm),
                    puffer_min: negative_only(buffered_vs_min),
                    tief_agl_min: negative_only(stats.low_agl_vs_min_fpm),
                    letztes_tief: stats
                        .last_low_agl_vs_fpm
                        .filter(|(_, ts)| (now - *ts).num_seconds() <= 15)
                        .map(|(v, _)| v),
                };
                let (touchdown_vs, vs_source) =
                    touchdown_vs_bestimmen(&quellen, is_msfs, is_xplane);
                tracing::info!(
                    sim = ?snap.simulator,
                    quellen = ?quellen,
                    gewaehlt = touchdown_vs,
                    quelle = vs_source,
                    "touchdown VS bestimmt"
                );

                // ⚠ Eine gespeicherte 0 ist eine AUSSAGE — und sie muss
                // erklaerbar sein.
                //
                // Bei SRR318 (LIMJ, 31.08.2026) stand hier 0, waehrend
                // sieben Messungen zwischen -1389 und -3307 fpm im selben
                // Datensatz lagen. phpVMS bekam `landing_rate = NULL`,
                // der Score wurde 0, die Wartung feuerte nicht.
                //
                // Die Ursache war NICHT eine fehlende Notrettung, sondern
                // dass Wert und Quellenangabe aus ZWEI Ketten kamen und
                // auseinanderliefen — die Quelle meldete einen Zweig, der
                // bei -1389 niemals 0 ergeben kann. Seit beides aus EINER
                // Entscheidung faellt, ist der Widerspruch unmoeglich:
                // Was ein Zweig liefert, wird gespeichert, und die
                // Quellenangabe beschreibt genau ihn.
                //
                // ⚠ Eine Notrettung aus den ausgeschlossenen Quellen gab
                // es hier zwischenzeitlich; sie ist wieder RAUS. Vier
                // QS-Runden haben gezeigt, dass sie ihre Belege an dieser
                // Stelle gar nicht haben kann: Die Kraftspitze
                // (`landing_analysis.max_gear_force_n`) entsteht erst
                // nach `TOUCHDOWN_POST_WINDOW_MS` — zehn Sekunden
                // spaeter. Ein Riegel, der auf Werte wartet, die es noch
                // nicht gibt, oeffnet entweder nie oder zur falschen
                // Zeit; und die Quellen, aus denen er schoepfen wollte,
                // verwirft die regulaere Kette mit Grund
                // (Rueckstoss-Spitzen beim Fahrwerkskontakt).
                //
                // Bleibt es bei 0, heisst das jetzt ehrlich: ALLE
                // zulaessigen Quellen waren leer oder positiv. Die
                // Meldung nennt sie einzeln, damit der naechste Fall in
                // einer Minute statt an einem Nachmittag zu klaeren ist.
                if touchdown_vs == 0.0 {
                    tracing::warn!(
                        sim = ?snap.simulator,
                        quelle = vs_source,
                        gerastet = ?snap.touchdown_vs_fpm,
                        schaetzer_xp = ?agl_estimate_xp.map(|e| (e.fpm, e.window_ms)),
                        schaetzer_msfs = ?agl_estimate_msfs.map(|e| e.fpm),
                        abtaster = ?stats.sampler_touchdown_vs_fpm,
                        puffer_min = ?buffered_vs_min,
                        tief_agl_min = ?stats.low_agl_vs_min_fpm,
                        letztes_tief = ?stats.last_low_agl_vs_fpm,
                        // ⚠ ALLE drei g-Quellen, nicht nur eine.
                        //
                        // `landing_peak_g_force` wird im selben Block erst
                        // WEITER UNTEN gesetzt — hier stuende beim ersten
                        // Aufsetzer `None`, ausgerechnet bei dem Fall, den
                        // die Meldung erklaeren soll. Der Abtaster-Wert und
                        // der Augenblickswert liegen dagegen schon vor.
                        g_im_augenblick = snap.g_force,
                        g_abtaster = ?stats.sampler_touchdown_g_force,
                        g_peak_bisher = ?stats.landing_peak_g_force,
                        "touchdown VS bleibt 0 — alle zulaessigen Quellen leer oder positiv"
                    );
                }
                stats.landing_simulator = Some(if is_msfs {
                    "msfs"
                } else if is_xplane {
                    "xplane"
                } else {
                    "other"
                });
                stats.landing_vs_estimate_xp_fpm = agl_estimate_xp.map(|e| e.fpm.round() as i32);
                stats.landing_vs_estimate_msfs_fpm =
                    agl_estimate_msfs.map(|e| e.fpm.round() as i32);
                stats.landing_vs_source = Some(vs_source);
                // gear_force_peak_n: nur X-Plane liefert den Wert ueber
                // den Sampler-Pfad. snap.gear_normal_force_n im Touchdown-
                // Frame ist die direkteste Quelle.
                stats.landing_gear_force_peak_n = if is_xplane {
                    snap.gear_normal_force_n
                } else {
                    None
                };
                // window_ms + sample_count: nur fuer den Pfad der wirklich
                // gewann (= Lua bei XP, Time-Tier bei MSFS). Fallback-Pfade
                // haben keine Window-Metrik.
                if vs_source == "agl_estimate_xp" {
                    if let Some(est) = agl_estimate_xp {
                        stats.landing_estimate_window_ms = Some(est.window_ms as i32);
                        stats.landing_estimate_sample_count = Some(est.sample_count as u32);
                    }
                } else if vs_source == "agl_estimate_msfs" {
                    if let Some(est) = agl_estimate_msfs {
                        stats.landing_estimate_window_ms = Some(est.window_ms as i32);
                        stats.landing_estimate_sample_count = Some(est.sample_count as u32);
                    }
                }

                // v0.7.1 P2.2-D: atomic write via finalize_landing_rate.
                // confidence ist hier nicht direkt vom touchdown_v2-Cascade
                // verfuegbar (wir sind im legacy-streamer-Pfad), daher None.
                // Wenn der touchdown_v2-Pfad in Site 1 (lib.rs ~9362) schon
                // einen Wert mit Confidence gesetzt hat und der Streamer-Tick
                // hier nochmal feuert, wird Confidence uebergeschrieben — das
                // ist OK, weil der Streamer-Tick die "endgueltige" Quelle
                // gegenueber dem Edge-Wert ist (Smoothed-Fallback).
                finalize_landing_rate(&mut stats, touchdown_vs, None, Some(vs_source));
                stats.zwischenstand("aufsetzen_sinkrate");

                // G capture: prefer sampler-side (v0.4.4) wenn verfügbar.
                // Sonst peak G from full buffer (5 s of pre-touchdown
                // history). Subsequent ticks in the Landing arm refine
                // this further until TOUCHDOWN_G_WINDOW_MS elapses.
                let touchdown_g = if let Some(sampler_g) = stats.sampler_touchdown_g_force {
                    sampler_g
                } else {
                    let buffered_g_peak: f32 = stats
                        .snapshot_buffer
                        .iter()
                        .map(|s| s.g_force)
                        .fold(0.0, f32::max);
                    buffered_g_peak.max(snap.g_force)
                };
                stats.landing_g_force = Some(touchdown_g);
                stats.landing_peak_g_force = Some(touchdown_g);

                // Touchdown metadata (IAS/pitch/bank/heading/speed/GS/fuel,
                // TD coords, profile, sideslip, wind) + runway correlation —
                // extracted to phase-independent helpers (v0.15.24) so the
                // validated sampler touchdown can stamp them too when the
                // FSM never reaches Landing (bush hops, GSG312). This call
                // is behaviour-identical to the pre-extraction inline code;
                // it deliberately OVERWRITES any earlier sampler-path stamp
                // with the FSM-tick values, exactly as before.
                // LTU 154 (26.10.2026): Lag zwischen Aufsetzen und diesem
                // Takt eine Pause, haelt der Puffer nur noch Proben von
                // danach — der Stempel kaeme aus dem stehenden Flugzeug
                // (07L statt 25R, 1279 statt 1364 kg Landesprit). Dann
                // bleibt der Stempel des Abtasters samt Ausroll-Anker.
                let abtaster_stempel_bleibt = abtaster_stempel_bleibt(&stats, td_buf_sample.as_ref());
                if !abtaster_stempel_bleibt {
                    stamp_touchdown_metadata(&mut stats, snap, actual_td_at, td_buf_sample.as_ref());
                    correlate_touchdown_runway(&mut stats, snap, flight, td_buf_sample.as_ref());
                }
                stats.zwischenstand("aufsetzen_bahn");

                // Reset bounce state for the new analyzer window.
                stats.bounce_armed_above_threshold = false;
                // Lernpaket AP3 / Codex-QS: Die Bodenhoehe gleich hier vom
                // Aufsetz-Schnappschuss nehmen. Erst beim naechsten Tick
                // gesetzt, ginge ein Hopser, der vorher beginnt, verloren.
                stats.bounce_boden_agl_ft = snap.on_ground.then_some(snap.altitude_agl_ft);
                stats.bounce_count = 0;
                stats.light_bounce_count = 0;
                stats.bounce_luft_seit_ms = None;
                stats.bounce_luft_max_ft = 0.0;

                // ---- Landing Analyzer (Stage 1) ----

                // Compute approach-stability stddev from the buffer
                // accumulated during Approach + Final. None when we
                // have fewer than 3 samples (resumed flight,
                // ultra-fast approach).
                //
                // v0.5.25: PLUS Stable-Approach-Gate-konforme v2-
                // Auswertung (1000-ft-AGL-Window, Glide-Slope-Deviation,
                // Vector-Window-Filter, Late-RWY-Change-Detection).
                // Beide Werte werden mit-publiziert — pre-v0.5.25-
                // Konsumenten weiter mit den σ-Werten, neue UI nutzt
                // approach_vs_deviation_fpm fuer das echte Gate-Maß.
                // v0.5.46: TD-Timestamp an Stability-Funktionen reichen damit
                // die letzten 3 sec (Flare-Manoever) nicht als Instabilitaet
                // gewertet werden (Adrian-Feedback). landing_at ist im
                // Landing-Phase-Handler bereits gesetzt.
                let td_ts = stats.landing_at;
                let (vs_sd, bank_sd) = compute_approach_stddev(&stats.approach_buffer, td_ts);
                // Rotorcraft / seaplane fly a vertical / non-glideslope profile
                // that the fixed-wing approach-stability model mis-reads as
                // "unstable" (high sink, large bank/IAS variance). The stability
                // sub-score is DRIVEN by these std-devs and `sub_stability`
                // returns None on None inputs (= sub-score SKIPPED, not
                // penalised), so null them for these categories. Fixed-wing
                // unchanged.
                let (vs_sd, bank_sd) = if category.is_non_conventional() {
                    (None, None)
                } else {
                    (vs_sd, bank_sd)
                };
                stats.approach_vs_stddev_fpm = vs_sd;
                stats.approach_bank_stddev_deg = bank_sd;
                // Musterquelle wie bei `category` oben: erst der Simulator,
                // dann die Buchung. Vorher fragte die Grenzen-Tabelle NUR die
                // Buchung, waehrend die Kategorie-Logik im selben Block am
                // Sim-Wert haengt — zwei Antworten auf dieselbe Frage. Wer
                // anders fliegt als gebucht, bekam Bank-Grenze und Vref des
                // gebuchten Musters und die Kategorie des geflogenen.
                // Alle drei Stufen, an EINER Stelle — und das Ergebnis
                // bleibt im Flugzustand stehen, damit die MQTT-Pfade
                // dieselbe Antwort bekommen statt einer eigenen.
                let aufgeloest = sim_core::muster_aufloesen(
                    snap.aircraft_icao.as_deref(),
                    &flight.aircraft_icao,
                    snap.aircraft_title.as_deref(),
                );
                if let Some(m) = aufgeloest.as_deref() {
                    if stats.aufgeloestes_muster.as_deref() != Some(m) {
                        stats.aufgeloestes_muster = Some(m.to_string());
                    }
                }
                let muster = aufgeloest.as_deref().unwrap_or("");
                let limits = aircraft_limits_for(muster);
                if limits.is_fallback && !muster.is_empty() && stats.unbekanntes_muster.is_none() {
                    // Einmal pro Flug — nicht je Landung.
                    tracing::warn!(
                        aircraft_icao = %muster,
                        "Muster nicht in der Grenzen-Tabelle: Wing-Strike gegen \
                         generische 8°, keine Vref-Abweichung"
                    );
                    stats.unbekanntes_muster = Some(muster.to_string());
                }
                let konfig = konfig_kontext(&stats, &limits);
                let bezug_ft = anflug_bezugshoehe_ft(
                    &stats,
                    &flight.arr_airport,
                    ist_platzhoehe_navdaten_ft(&flight, &stats),
                );
                // Score-Version 23: spaetes Final / Kurvenanflug.
                let spaetes =
                    td_ts.and_then(|td| spaetes_final_erkennen(&stats, bezug_ft, td, None));
                stats.approach_spaetes_final = spaetes;
                let stab_v2 = compute_approach_stability_v2_mit_final(
                    &stats.approach_buffer,
                    bezug_ft,
                    td_ts,
                    // v0.15.17: echter Gleitwinkel der aufgelösten Bahn (oben bei
                    // 18597 gesetzt, also vor diesem Aufruf). None bei OurAirports-
                    // Fallback / keinem Match → Faktor 1,0 (3°-Verhalten).
                    stats
                        .runway_nav_geometry
                        .as_ref()
                        .map(|g| g.glideslope_angle),
                    // v0.16.6: kein Zeit-Fenster auf dem FSM-Pfad — exakt das
                    // bisherige Verhalten (das Fenster ist nur fuer den
                    // Sampler-Pfad, dessen Buffer Low-Level-Cruise enthalten kann).
                    None,
                    konfig,
                    spaetes.as_ref(),
                );
                stats.approach_vs_deviation_fpm = stab_v2.vs_deviation_fpm;
                stats.approach_max_vs_deviation_below_500_fpm =
                    stab_v2.max_vs_deviation_below_500_fpm;
                stats.approach_bank_stddev_filtered_deg = stab_v2.bank_stddev_filtered_deg;
                stats.approach_vs_stddev_filtered_fpm = stab_v2.vs_stddev_filtered_fpm;
                stats.approach_runway_changed_late = stab_v2.runway_changed_late;
                stats.approach_stable_at_gate = stab_v2.stable_at_gate;
                stats.approach_vs_jerk_fpm = stab_v2.vs_jerk_fpm;
                stats.approach_ias_stddev_kt = stab_v2.ias_stddev_kt;
                stats.approach_excessive_sink = stab_v2.excessive_sink;
                stats.approach_stable_config = stab_v2.stable_config;
                stats.approach_ias_stddev_unter_500_kt = stab_v2.ias_stddev_unter_500_kt;
                stats.approach_stable_config_500 = stab_v2.stable_config_500;
                stats.approach_used_hat = stab_v2.used_hat;
                stats.approach_window_sample_count = Some(stab_v2.window_sample_count);
                stats.approach_stable_at_da = stab_v2.stable_at_da;
                stats.approach_stall_warning_count = stab_v2.stall_warning_count;
                stats.approach_wind = stab_v2.wind.clone();
                // Lernpaket AP4/AP5: Forensik neben dem Gate, gegen dieselbe
                // Bahn und denselben Puffer — ohne Rueckwirkung auf das Gate.
                let platzhoehe = anflug_bezugshoehe_ft(
                    &stats,
                    &flight.arr_airport,
                    ist_platzhoehe_navdaten_ft(&flight, &stats),
                );
                anflug_forensik_stempeln(&mut stats, None, td_ts, platzhoehe);

                // ─── v0.5.26 Per-Touchdown-Metriken ─────────────────
                // Wing-Strike-Severity: |bank_at_td| / max_bank × 100%.
                // A rotorcraft has no wing to strike, and the fixed-wing 8°
                // fallback bank limit would flag a normal heli touchdown bank
                // (15-20° on a slope/crosswind) as a 190-250% "wing strike" —
                // skip it for rotorcraft. (Seaplanes keep it: a float-tip /
                // wing-tip strike on water is still meaningful.)
                if !category.is_rotorcraft() {
                    if let Some(bank) = stats.landing_bank_deg {
                        let pct = (bank.abs() / limits.max_bank_landing_deg) * 100.0;
                        stats.landing_wing_strike_severity_pct = Some(pct);
                    }
                }

                // Float-Distance: Threshold-Crossing → TD.
                // Threshold-Position kommt aus runway_match. Distanz
                // direkt approximierbar als (rollout-Distanz von TD bis
                // GS<2kt = total runway-overrun) — aber wir haben das
                // nicht. Stattdessen: from-threshold-Distanz vom
                // runway_match (= TD-Punkt past threshold).
                // Float-Distanz ≈ past_threshold − typische
                // Threshold-50ft-Crossing-Distance. Vereinfacht:
                // touchdown_distance_from_threshold_ft direkt als
                // "Float" interpretieren (= Distanz die der Pilot nach
                // Threshold floated bevor er aufgesetzt hat). Pre-
                // Threshold-Approach-Pfad ist hier nicht relevant.
                // `landing_float_distance_m` setzt seit Runde 3
                // `drittel_nachfuehren` — an jeder Zuordnung, nicht nur hier.
                // ⚠ Das Drittel wird hier NICHT mehr gerechnet.
                //
                // Bis v1.7.14 stand es zweimal im Haus: einmal roh aus
                // Bahnlaenge und Aufsetzdistanz (hier), einmal als
                // `td_third` aus `assess_touchdown`, das die versetzte
                // Schwelle beruecksichtigt. Zwei Zahlen fuer dieselbe
                // Aussage — und zwei Webapp-Karten, die die aeltere
                // lesen.
                //
                // Sobald eine spaet eingetroffene Szenerie die Bahnlaenge
                // aendert, korrigierte die Nachholung nur `td_third`.
                // Dann zeigten beide Karten verschiedene Drittel
                // derselben Landung an, ohne dass eine davon falsch
                // aussah.
                //
                // Jetzt folgt das alte Feld dem neuen: `korreliere_bahn`
                // setzt beide aus derselben Rechnung. Siehe
                // `drittel_nachfuehren`.

                // Vref-Abweichung: gemessen, sonst aus dem Landegewicht
                // (`landing_scoring::vref`). Den Pauschalwert je Muster
                // (`typical_vref_kt`) nimmt sie seit 2.0.6 nicht mehr.
                // Verbraucht: ein Touch-and-go misst im naechsten Anflug neu.
                let gemessen = stats.vref_gemessen.take();
                // v3 (Kap. 5.2): fuer `herkunft.vref` festhalten.
                stats.landing_vref_gemessen = gemessen;
                // Der PIREP-Eintrag „PMDG VREF (Landing)" nennt denselben Wert
                // wie die Bewertung, nicht den aus dem ersten Anflug (QS Codex).
                // Ohne PMDG-Messung beim Aufsetzen bleibt er leer.
                stats.pmdg_vref_at_landing = match gemessen {
                    Some((v, "pmdg")) => Some(v.round() as u8),
                    _ => None,
                };
                let landeklappen =
                    landing_scoring::vref::landeklappen(snap.flap_handle_index, snap.flap_num_positions);
                let vref = landing_scoring::vref::vref_bestimmen(
                    Some(muster),
                    // Aus dem Snapshot: `landing_weight_kg` wird erst unten
                    // gestempelt und traegt bei Touch-and-go noch das alte Gewicht.
                    gewicht_aus_snapshot(snap).or(stats.landing_weight_kg),
                    gemessen,
                    landeklappen,
                );
                stats.landing_vref_formel_kt = vref.and_then(|e| e.formel_kt);
                stats.landing_vref_landeklappen = landeklappen;
                stats.landing_vref_source = Some(vref.map_or("unknown", |e| e.quelle));
                stats.landing_vref_toleranz_kt = vref.and_then(|e| e.toleranz_kt);
                if let (Some(e), Some(ias)) = (vref, stats.landing_speed_kt) {
                    stats.landing_vref_deviation_kt = Some(ias - e.vref_kt);
                }

                // Yaw-Rate am TD: heading-Aenderung pro Sekunde im
                // letzten 1-sec-Fenster vor TD.
                let yaw_window_start = actual_td_at - chrono::Duration::milliseconds(1000);
                let yaw_samples: Vec<(DateTime<Utc>, f32)> = stats
                    .snapshot_buffer
                    .iter()
                    .filter(|s| s.at >= yaw_window_start && s.at <= actual_td_at)
                    .map(|s| (s.at, s.heading_true_deg))
                    .collect();
                if yaw_samples.len() >= 2 {
                    let first = yaw_samples.first().unwrap();
                    let last = yaw_samples.last().unwrap();
                    let mut delta = last.1 - first.1;
                    while delta > 180.0 {
                        delta -= 360.0;
                    }
                    while delta < -180.0 {
                        delta += 360.0;
                    }
                    let dt_sec = (last.0 - first.0).num_milliseconds() as f32 / 1000.0;
                    if dt_sec > 0.1 {
                        stats.landing_yaw_rate_deg_per_sec = Some(delta.abs() / dt_sec);
                    }
                }

                // Verzögerung: erst am Ende des Ausrollens (`rollout_tick`),
                // siehe `verzoegerung_mps2`.
                tracing::info!(
                    pirep_id = %flight.pirep_id,
                    vs_dev_fpm = ?stab_v2.vs_deviation_fpm,
                    max_vs_dev_below_500 = ?stab_v2.max_vs_deviation_below_500_fpm,
                    bank_sd_filtered = ?stab_v2.bank_stddev_filtered_deg,
                    rwy_changed_late = stab_v2.runway_changed_late,
                    stable_at_gate = ?stab_v2.stable_at_gate,
                    samples = stab_v2.window_sample_count,
                    "approach stability v2 computed"
                );

                // Reset rollout tracking. The Landing-phase arm below
                // accumulates haversine distance from `(landing_lat,
                // landing_lon)` until the rollout finalises (~40 kt exit
                // speed / >30° heading turn-off / 5 kt full-stop), then
                // sets `rollout_finalized`.
                if !abtaster_stempel_bleibt {
                    stats.rollout_distance_m = Some(0.0);
                    stats.rollout_finalized = false;
                    stats.rollout_last_lat = Some(snap.lat);
                    stats.rollout_last_lon = Some(snap.lon);
                }
                // Landing weight — extracted helper (v0.15.24), called at
                // the SAME point in the arm as the pre-extraction inline
                // code (after the brake-energy proxy above, which must keep
                // seeing the PREVIOUS landing_weight_kg on touch-and-go).
                // Bleibt der Stempel des Abtasters, gilt auch sein Gewicht.
                if !abtaster_stempel_bleibt {
                    stamp_landing_weight(&mut stats, snap);
                }
                stats.zwischenstand("aufsetzen_anflug_note");
            }
        }
        // 20.10.2026 („Stück 3“): Die Landung ist beim Wegsteigen
        // abgeschlossen (hier im Takt oder vom Sampler zwischen zwei Takten),
        // oder es ist gar keine mehr offen und das Flugzeug steigt — dann
        // gehoert die Phase in den Steigflug, und der Landing-Arm darf
        // keine Rollstrecke und keine Spur mehr aus der Luft aufzeichnen.
        // `landing_at == None && sampler_touchdown_at == None` faengt auch
        // einen Neustart der App mitten im Steigflug ab (der Merker ist
        // nicht persistiert).
        FlightPhase::Landing
            if stats.landung_abgebrochen_phase_offen
                || (stats.landing_at.is_none()
                    && stats.sampler_touchdown_at.is_none()
                    && wegsteigen_mit_fahrt(snap)) =>
        {
            next_phase = FlightPhase::Climb;
        }
        FlightPhase::Landing => {
            // Rollout-distance accumulator: from `landing_lat/lon` until
            // the rollout finalises (~40 kt exit speed / >30° heading
            // turn-off / 5 kt full-stop). Extracted verbatim to
            // `rollout_tick` (v0.16.6) so sampler-path bush landings can
            // drive the same logic from the streamer — this call sits at
            // the exact position the inline code used to.
            rollout_tick(&mut stats, snap);

            // ── Die Rollspur, unabhaengig von der Ausrollstrecke ──────
            //
            // `rollout_tick` bricht ab, sobald `rollout_finalized` steht —
            // und das faellt bei vierzig Knoten, denn dort ist die
            // AUSROLLSTRECKE fertig. Die Spur ist es nicht: Ihr Vertrag
            // laesst sie bis fuenf Knoten laufen, bis zur Ausfahrt oder
            // bis achtzig Meter neben die Bahn.
            //
            // Der Aufruf stand bis zur QS-Runde 25 IN `rollout_tick` und
            // wurde dort herausgezogen, damit er die vierzig Knoten
            // ueberlebt. Er landete dabei nur im Sampler-Pfad — und der
            // kehrt bei `FlightPhase::Landing` ausdruecklich zurueck.
            // Folge: Bei jeder normalen Landung wurde ueberhaupt keine
            // Spur mehr aufgezeichnet. Der 39-kt-Test lief ueber `TaxiIn`
            // und sah diesen Weg nicht.
            //
            // Deshalb hier, genau einmal je Tick, nach der
            // Streckenzaehlung: Die eigenen Riegel in
            // `spur_fortschreiben` beenden die Aufzeichnung.
            bahndisziplin_tick(&mut stats, snap);

            // Touchdown analyzer windows (BeatMyLanding-aligned):
            //   * Peak G refined for TOUCHDOWN_G_WINDOW_MS (1500 ms)
            //   * Peak |V/S| refined for TOUCHDOWN_WINDOW_SECS (5 s) —
            //     wider to catch a hard secondary contact after a bounce.
            //   * Bounces tracked via AGL-edge for BOUNCE_WINDOW_SECS (8 s).
            //   * Score finalised once the bounce window closes.
            if let Some(touchdown) = stats.landing_at {
                let elapsed_ms = (now - touchdown).num_milliseconds();
                let elapsed_secs = elapsed_ms / 1_000;
                let in_vs_window = elapsed_secs <= crate::grenzwerte::grenzwerte().landung.touchdown_window_secs.wert;
                let in_g_window = elapsed_ms <= crate::grenzwerte::grenzwerte().landung.touchdown_g_window_ms.wert;
                let in_bounce_window = elapsed_secs <= crate::grenzwerte::grenzwerte().landung.bounce_window_secs.wert;

                if in_vs_window {
                    // Post-touchdown peak-|V/S| refinement is a scored value, so
                    // it uses the raw, lag-free touchdown source (not the VVI).
                    let raw_vs = snap.touchdown_vs_source_fpm();
                    let peak_vs = stats.landing_peak_vs_fpm.unwrap_or(0.0);
                    if raw_vs < peak_vs {
                        stats.landing_peak_vs_fpm = Some(raw_vs);
                    }
                }
                if in_g_window {
                    let peak_g = stats.landing_peak_g_force.unwrap_or(0.0);
                    if snap.g_force > peak_g {
                        stats.landing_peak_g_force = Some(snap.g_force);
                    }
                }

                // Bounce detection — AGL-edge state machine. Replaces
                // the noisy on-ground flicker we used pre-Tier-1, which
                // tripped on gear-strut oscillation and over-counted
                // bounces on a clean landing.
                //
                // Arm: Hoehe ueber der Bodenhoehe steigt ueber
                //      BOUNCE_AGL_THRESHOLD_FT (5 ft = touchdown_v2
                //      SCORED-Threshold; Lernpaket AP3, vorher 15 ft absolut).
                // Fire: Bodenkontakt oder zurueck unter
                //      BOUNCE_AGL_RETURN_FT (2 ft ueber Bodenhoehe).
                // Both must happen inside BOUNCE_WINDOW_SECS for a bounce
                // to count — past that we assume the pilot did a touch-
                // and-go or got airborne again deliberately.
                // Lernpaket AP3: beide Schwellen relativ zur Bodenhoehe beim
                // ersten Bodenkontakt im Fenster; vorher wird nicht scharf
                // geschaltet.
                if in_bounce_window && stats.bounce_boden_agl_ft.is_none() && snap.on_ground {
                    stats.bounce_boden_agl_ft = Some(snap.altitude_agl_ft);
                }
                if let (true, Some(boden)) = (in_bounce_window, stats.bounce_boden_agl_ft) {
                    let ueber_boden = snap.altitude_agl_ft - boden;
                    // P1/8b: leichter Hopser im 1-Hz-Vorwert — gleiche Regel
                    // wie der 50-Hz-Pfad (Dauer ohne Bodenkontakt + Hoehe,
                    // mit zweitem Kontakt, unter der Hopser-Schwelle).
                    if !snap.on_ground {
                        if stats.bounce_luft_seit_ms.is_none() {
                            stats.bounce_luft_seit_ms = Some(elapsed_ms);
                            stats.bounce_luft_max_ft = 0.0;
                        }
                        stats.bounce_luft_max_ft = stats.bounce_luft_max_ft.max(ueber_boden);
                    } else if let Some(seit) = stats.bounce_luft_seit_ms.take() {
                        if leichter_hopser_im_streamer(elapsed_ms - seit, stats.bounce_luft_max_ft) {
                            stats.light_bounce_count = stats.light_bounce_count.saturating_add(1);
                        }
                        stats.bounce_luft_max_ft = 0.0;
                    }
                    // Nur in der Luft scharfschalten (Codex-QS): Ein AGL-
                    // Sprung bei gesetztem Bodenflag (Gelaende, Ausreisser)
                    // ist kein Hopser — der 50-Hz-Pfad verlangt dasselbe.
                    if !stats.bounce_armed_above_threshold
                        && !snap.on_ground
                        && ueber_boden > crate::grenzwerte::grenzwerte().landung.bounce_agl_threshold_ft.wert
                    {
                        stats.bounce_armed_above_threshold = true;
                    } else if stats.bounce_armed_above_threshold
                        && (snap.on_ground || ueber_boden < crate::grenzwerte::grenzwerte().landung.bounce_agl_return_ft.wert)
                    {
                        stats.bounce_count = stats.bounce_count.saturating_add(1);
                        stats.bounce_armed_above_threshold = false;
                        tracing::info!(
                            count = stats.bounce_count,
                            agl_ft = snap.altitude_agl_ft,
                            elapsed_ms,
                            "bounce counted (AGL re-entered ground band)"
                        );
                    }
                }

                // v1.7.24 (GSG1249, Codex-bestaetigter Fund): die Score-
                // Finalisierung, die hier bis dahin stand (`!in_bounce_window
                // && stats.landing_score.is_none() { ... }`), ist nach
                // `finalize_landing_score_if_due()` gewandert und wird jetzt
                // JEDEN Tick UNCONDITIONAL aufgerufen (siehe Aufrufstelle kurz
                // nach dem `match stats.phase` unten) — nicht mehr nur,
                // solange `stats.phase == Landing` ist. Grund: bei einem
                // schnell ausrollenden, leichten Flugzeug kann die Phase
                // innerhalb von `BOUNCE_WINDOW_SECS` (praktisch ~9s durch
                // Ganzzahl-Rundung) zu `TaxiIn` wechseln — dieser Match-Arm
                // hier haette dann nie wieder gefeuert, `landing_score` waere
                // fuer immer `None` geblieben, `announce_landing_score()`
                // (die einzige Quelle fuer `TouchdownComplete`) haette nie
                // etwas zurueckgegeben. Exakt derselbe Fehlerklasse war
                // schon einmal in diesem Match-Arm gefunden und gefixt
                // worden (v0.7.20, GSG219, `TouchdownEvent`-Push weiter unten
                // von `landing_score.is_some()` entkoppelt) — diese
                // Ent-Kopplung zieht dieselbe Lehre konsequent auf die
                // Score-Finalisierung selbst durch.

                // Touch-and-Go: kein eigener Erkenner mehr in diesem Arm
                // (20.10.2026, „Stück 3“). Das Wegsteigen nach dem Aufsetzen
                // schliesst `landung_beim_wegsteigen_abschliessen` ab — ohne
                // Zeitgrenze, an einer Stelle fuer Sampler und Phasenlogik.
                // Aufrufstelle nach dem `match prev_phase`.

                // v0.7.20 (QS-Nebenfund GSG219): FinalLanding-Push aus dem
                // `else if`-Branch entkoppelt — vorher konnte der Push
                // verloren gehen wenn:
                //   1. der Score im selben Tick finalisiert wurde aber der
                //      `else if` durch den vorherigen `if tg_in_window`-Pfad
                //      blockiert blieb, ODER
                //   2. die Phase im selben Tick auf TaxiIn wechselte bevor
                //      ein zweiter Tick im Landing-Branch lief.
                //
                // Ausserdem nicht mehr an `landing_score.is_some()` gekoppelt:
                // bei Sampler-Fallback (vs_source=last_low_agl_vs, kein
                // Sampler-Edge) kann `landing_score` durch den B-005-Guard
                // ohne `landing_peak_vs_fpm` None bleiben — der Touchdown
                // hat aber definitiv stattgefunden (Webapp-Anzeige + Pilot-
                // Tab "Landung" + MQTT-touchdown_complete zeigen das). Der
                // TouchdownEvent gehoert ins Audit/PIREP-`touchdown_count`-
                // Feld, unabhaengig davon ob die Score-Kategorie klassifiziert
                // werden konnte.
                //
                // peak_vs_fpm fuellt sich aus dem besten verfuegbaren Wert:
                // Sampler-Peak (best) → landing_rate_fpm (Fallback aus dem
                // touchdown_v2-Cascade) → 0.0 (defensiv).
                //
                // 20.10.2026: verbucht nach `FINAL_LANDING_VERBUCHEN_NACH_SECS`
                // am Boden ODER beim Abrollen (TaxiIn folgt in diesem Takt) —
                // EWG 1599s zweite Landung rollte nach 20 s ab und stand nur
                // ueber `effective_touchdown_events` im PIREP. Steigt das
                // Flugzeug spaeter doch weg, wird aus diesem Eintrag der
                // Touch-and-Go (`landung_beim_wegsteigen_abschliessen`).
                let rollt_ab = snap.groundspeed_kt < 30.0 && snap.on_ground;
                if (elapsed_secs > crate::grenzwerte::grenzwerte().phasen_kern.final_landing_verbuchen_nach_secs.wert || rollt_ab)
                    && stats.landing_at == Some(touchdown)
                    && stats
                        .touchdown_events
                        .last()
                        .map(|e| e.timestamp != touchdown)
                        .unwrap_or(true)
                {
                    let event = TouchdownEvent {
                        timestamp: touchdown,
                        kind: TouchdownKind::FinalLanding,
                        // v0.20.0: ueber die Kanonik. Hier, direkt am Touchdown,
                        // liefert sie denselben Wert (der 50-Hz-Puffer-Dump
                        // laeuft erst ~10 s spaeter, `landing_analysis` ist noch
                        // leer) — aber die Kaskade von Hand nachzubauen ist
                        // genau das Muster, aus dem der PIA3452-Split entstand.
                        peak_vs_fpm: stats.canonical_landing_rate_fpm().unwrap_or(0.0),
                        peak_g: stats.landing_peak_g_force.unwrap_or(0.0),
                        lat: stats.landing_lat.unwrap_or(snap.lat),
                        lon: stats.landing_lon.unwrap_or(snap.lon),
                        sub_bounces: stats.bounce_count,
                    };
                    stats.touchdown_events.push(event);
                    tracing::info!(
                        total_touchdowns = stats.touchdown_events.len(),
                        "final landing recorded as TouchdownEvent"
                    );
                }
            }

            if snap.groundspeed_kt < 30.0 && snap.on_ground {
                next_phase = FlightPhase::TaxiIn;
            }
        }
        FlightPhase::TaxiIn => {
            if snap.parking_brake && snap.groundspeed_kt < 1.0 && snap.on_ground {
                // Which field are we actually standing on? Usually the planned
                // one — but a pilot who diverted also taxis in and sets the
                // brake, and his stand belongs to the field he is ON.
                let at_icao = match arrival::locate(
                    &flight.arr_airport,
                    snap.lat,
                    snap.lon,
                    stats.planned_arr_ref_pos,
                ) {
                    arrival::ArrivalSite::AtOtherAirport { icao, .. } => icao,
                    _ => flight.arr_airport.clone(),
                };
                // v1.6 Stand-Gate (RYR-1142-Befund): Parkbremse + Stillstand
                // allein ist KEIN Block-On — ein mehrminütiger Rollhalt (H4
                // in LPPT, wartend mit gesetzter Bremse) sieht identisch aus.
                // Liegen OSM-Parkpositionen für DIESEN Flughafen vor, zählt
                // der Halt nur in Standnähe; fern jedes Stands bleibt die
                // Phase TaxiIn. Ohne Daten (kleiner Platz, Divert, offline)
                // gilt das alte Kriterium — der universale Arrived-Fallback
                // fängt ohnehin jeden abgestellten Flieger.
                // Review-Fund v1.5.4: NORMALISIERT vergleichen — arr_stands_icao
                // ist beim Fetch upper-getrimmt, at_icao kommt roh aus dem Bid.
                // Ein lowercase-Bid deaktivierte das Gate sonst lautlos.
                let at_icao_norm = at_icao.trim().to_uppercase();
                let osm_stand_data = stats
                    .arr_stands
                    .as_ref()
                    .filter(|l| !l.is_empty())
                    .filter(|_| stats.arr_stands_icao.as_deref() == Some(at_icao_norm.as_str()))
                    .map(|l| l.as_slice());
                let stand_data = standliste_fuer(
                    &stats,
                    &at_icao_norm,
                    osm_stand_data,
                    Some((snap.lat, snap.lon)),
                );
                let (is_block_on, osm_stand, rejected_nearest) = match stand_data.as_deref() {
                    Some(list) => match stands::stand_at(list, snap.lat, snap.lon) {
                        // Naehe entscheidet ueber Block-On (auch ein namen-
                        // loser Stand zaehlt); der NAME kommt vom naechsten
                        // BENANNTEN Stand im Radius — sonst blockiert eine
                        // ungetaggte Position 10 m daneben das PIREP-Feld,
                        // obwohl der richtige Name 20 m weiter bereitliegt.
                        Some(_) => (
                            true,
                            stands::benannter_stand_bei(list, snap.lat, snap.lon)
                                .and_then(|(st, _)| st.name.clone()),
                            None,
                        ),
                        None => {
                            let n = stands::nearest(list, snap.lat, snap.lon)
                                .map(|(s, d)| (s.name.clone().unwrap_or_default(), d));
                            (false, None, n)
                        }
                    },
                    None => (true, None, None),
                };
                // Review-Fund v1.5.4: der abgelehnte Halt war vorher
                // unsichtbar — bei versetzten/lückigen OSM-Daten (Stand
                // fehlt, Szenerie-Offset) läuft der Flug still in den
                // Arrived-Fallback und die Blockzeit wandert auf den
                // Shutdown. EINMAL pro Flug festhalten, wie knapp es war.
                if let Some((name, dist)) = rejected_nearest {
                    if !stats.stand_gate_reject_logged {
                        stats.stand_gate_reject_logged = true;
                        tracing::info!(
                            at = %at_icao_norm,
                            nearest_stand = %name,
                            dist_m = dist.round(),
                            "stand gate: stopped with brake but no stand within radius — staying TaxiIn"
                        );
                    }
                }
                // Rollweg-Halt (EDDN/ENZV/KIAD, 27.10.2026): Lead-in-Linien
                // beginnen am Rollweg, und ein Bremshalt auf der Rollweg-
                // Mittellinie liegt oft noch im Stand-Radius. Dort zählt der
                // Halt erst, wenn die Triebwerke aus sind oder die Bremse
                // `ROLLWEG_HALT_BESTAETIGT_SECS` lang hält — die Block-On-Zeit
                // ist dann der Beginn des Halts (`settle_at_arrival_stand`).
                // Nur mit Standdaten: ohne sie gilt weiter das alte Kriterium.
                let is_block_on = is_block_on && (stand_data.is_none() || {
                    let auf_rollweg = stats
                        .arr_rollwege
                        .as_deref()
                        .filter(|_| stats.arr_rollwege_icao.as_deref() == Some(at_icao_norm.as_str()))
                        .and_then(|r| stands::rollweg_abstand_m(r, snap.lat, snap.lon))
                        .is_some_and(|d| d <= grenzwerte::grenzwerte().staende.rollweg_mittellinie_m.wert);
                    if !stats.blocks_on_kandidat.as_ref().is_some_and(|k| kandidat_hier(k, snap.lat, snap.lon)) {
                        stats.blocks_on_kandidat = None;
                    }
                    if auf_rollweg {
                        let neu = stats.blocks_on_kandidat.is_none();
                        let seit = stats.blocks_on_kandidat.get_or_insert((now, snap.lat, snap.lon)).0;
                        let ok = rollweg_halt_bestaetigt(seit, now, snap.engines_running);
                        if neu && !ok {
                            tracing::info!(
                                at = %at_icao_norm,
                                "stand gate: stopped with brake on a taxiway centreline — BlocksOn waits for shutdown or a sustained hold"
                            );
                        }
                        ok
                    } else {
                        true
                    }
                });
                if is_block_on {
                    next_phase = FlightPhase::BlocksOn;
                    stats.blocks_on_reached = true;
                    settle_at_arrival_stand(&mut stats, snap, now, &at_icao);
                    // settle_ füllt arr_gate nur aus dem (praktisch immer
                    // leeren) MSFS-parking_name — der OSM-Standname ist die
                    // verlässliche Quelle.
                    if stats.arr_gate.is_none() {
                        if let Some(name) = osm_stand {
                            stats.arr_gate = Some(name);
                            stats.arr_gate_icao = Some(at_icao_norm);
                        }
                    }
                }
            } else if !stats.blocks_on_kandidat.as_ref().is_some_and(|k| kandidat_hier(k, snap.lat, snap.lon)) {
                // Erst das Wegrollen verwirft den Haltbeginn, nicht schon das
                // Lösen der Bremse: LJLJ (BoYJe50, 11.10.2026) löste nach 82 s
                // und stellte 15 s später am selben Ort ab — der Arrived-
                // Fallback nimmt dann den Haltbeginn als Block-On-Zeit.
                stats.blocks_on_kandidat = None;
            }
        }
        FlightPhase::BlocksOn => {
            // v1.6 Sicherheitsnetz: aus BlocksOn wieder losgerollt — der
            // Halt war eine Fehldeutung (Rollhalt ohne Standdaten) oder
            // eine Repositionierung. Zurück zu TaxiIn, Block-On-Zeit und
            // erfassten Stand verwerfen; der nächste echte Halt setzt
            // beides neu. Ohne diesen Rückweg blieb RYR 1142 nach dem
            // H4-Halt 9 Minuten in BlocksOn hängen, quer rollend über
            // den halben Flughafen.
            //
            // Review-Fund v1.5.4: `!parking_brake` dazu — ein einzelner
            // GS-Spike bei GESETZTER Bremse (Slew, GSX-Ruck) ist ein
            // Artefakt und darf die Block-On-Zeit nicht verwerfen. Wer
            // wirklich weiterrollt, hat die Bremse gelöst.
            if snap.on_ground && snap.groundspeed_kt > 5.0 && !snap.parking_brake {
                next_phase = FlightPhase::TaxiIn;
                stats.blocks_on_reached = false;
                stats.block_on_at = None;
                stats.arr_gate_field_posted = false;
                stats.arr_gate = None;
                stats.arr_gate_icao = None;
            }
            // BlocksOn → Arrived once the pilot has actually shut
            // down: engines off + parking brake set + at least 30 s
            // since the wheels stopped. Real pilots routinely leave
            // engines running a minute or two after blocks-on for
            // cool-down / APU transition, so we don't flip to Arrived
            // the instant they hit the brake.
            else if let Some(block_on) = stats.block_on_at {
                let settled_secs = (now - block_on).num_seconds();
                if settled_secs >= 30
                    && snap.engines_running == 0
                    && snap.parking_brake
                    && snap.on_ground
                {
                    next_phase = FlightPhase::Arrived;
                }
            }
        }
        FlightPhase::Arrived | FlightPhase::PirepSubmitted => {
            // v0.7.17 (B-004): Sanity-Check fuer „Aircraft fliegt wieder
            // obwohl Phase ARRIVED ist". Klassisches Pattern: Pilot
            // landet, vergisst Flug zu beenden (oder Auto-File scheitert
            // silent, siehe B-006), startet im Sim einen neuen Flug.
            // Position-Stream lief vorher endlos mit der alten PIREP-ID
            // weiter und packte die neuen Positionen in den alten
            // PIREP-Recorder-Stream (= „Aircraft springt nach Landung
            // ploetzlich 750 km weg" wie bei CFG 1641 EDDM→EDDL).
            //
            // Hier ist die FSM die einzige Stelle die das mitbekommen
            // kann — der Position-Worker selbst hat keinen Zugriff auf
            // die Phase, posted blind. Wir setzen flight.stop damit der
            // Worker beim naechsten Tick sauber abdreht. PIREP bleibt
            // existing (wird nicht gefilt, bleibt prefiled — falls Pilot
            // doch zurueckkommt und manuell beenden moechte).
            let aircraft_clearly_flying =
                !snap.on_ground && snap.groundspeed_kt > 100.0 && snap.engines_running > 0;
            if aircraft_clearly_flying && !stats.arrived_stale_warning_logged {
                tracing::warn!(
                    pirep_id = %flight.pirep_id,
                    phase = ?stats.phase,
                    gs_kt = snap.groundspeed_kt,
                    agl_ft = snap.altitude_agl_ft,
                    "FSM phase=Arrived/PirepSubmitted but aircraft is flying again — stopping position stream"
                );
                stats.arrived_stale_warning_logged = true;
                stats.pending_acars_logs.push(
                    "Position-Stream gestoppt — Flugzeug fliegt wieder, aber PIREP war als 'angekommen' markiert. \
                     Bitte den alten Flug manuell beenden (PIREP-Detail) oder via 'Flug verwerfen' verwerfen, \
                     dann neuen Flug starten.".to_string(),
                );
                flight.stop.store(true, Ordering::Relaxed);
            }
        }
        FlightPhase::Preflight => {}
    }

    // Universal "we're done" fallback. The normal FSM chain assumes a
    // fixed-wing flight with a Pushback → Cruise → BlocksOn arc, and
    // a parking brake / engines-off shutdown sequence. None of that
    // applies cleanly to:
    //   * helicopters (no taxi-out / no parking brake / vertical
    //     takeoff and landing — the FSM gets stuck around TaxiOut)
    //   * very-short hops with no real cruise phase
    //   * engine-failure / emergency landings near the destination
    //   * aircraft profiles that don't wire `parking_brake` at all
    //
    // The escape hatch: once the aircraft has actually started
    // moving (block_off recorded), if it's now sitting on-ground
    // with engines off within 2 nmi of the arrival airport for at
    // least ARRIVED_FALLBACK_DWELL_SECS, we force `Arrived` regardless
    // of prior phase. Block_on_at gets back-filled here so the PIREP
    // file logic (which reads it for flight-time / chocks-on) still
    // works.
    stats.zwischenstand("phasen");
    let already_done = matches!(
        next_phase,
        FlightPhase::Arrived | FlightPhase::PirepSubmitted
    );
    let pre_block_off = matches!(next_phase, FlightPhase::Preflight | FlightPhase::Boarding);
    // GATE: only run the universal Arrived-fallback (and the divert
    // detection inside it) if the aircraft has actually been airborne
    // at some point. Without this, GSX/sim ground-handling jitter at
    // the gate can stamp `block_off_at` (any motion >0.5 kt counts),
    // and ARRIVED_FALLBACK_DWELL_SECS later the fallback fires with phase=Arrived plus a
    // bogus divert hint pointing at the departure airport — exactly
    // the bug pilots reported with NKS 833 KFLL→MKJS where GSX wackeln
    // produced "you landed at KFLL, planned was MKJS, file as divert?".
    // `snap.on_ground` is part of the gate, not just an optimisation: the
    // fallback below can only fire on the ground (every one of its paths
    // requires it), and `arrival::locate` searches the airport table when the
    // aircraft is NOT at its destination — which is every tick of the cruise.
    // Asking the question only when the answer can matter keeps that search off
    // the en-route hot path entirely.
    // Nach dem Weiterflug-Neustart am Boden (GAF 278) beendet erst eine
    // Landung NACH dem neuen Start den Flug, nicht der kalte Ausweichplatz.
    if stats.weiterflug_ab_boden_seit.is_some()
        && matches!(
            next_phase,
            FlightPhase::Takeoff
                | FlightPhase::Climb
                | FlightPhase::Cruise
                | FlightPhase::Holding
                | FlightPhase::Descent
                | FlightPhase::Approach
                | FlightPhase::Final
        )
    {
        stats.weiterflug_ab_boden_seit = None;
    }
    if !already_done
        && !pre_block_off
        && stats.block_off_at.is_some()
        && stats.was_airborne
        && snap.on_ground
        && stats.weiterflug_ab_boden_seit.is_none()
    {
        // Where are we, relative to where we were going? One question, one
        // answer, one geometry — see `crate::arrival`. `near_planned` used to
        // be derived from the runway-layout CENTROID while the divert path
        // below asked a *different* probe (nearest threshold) which field we
        // were standing on; at a big airport the two contradicted each other
        // and told pilots they had "landed in EDDF instead of planned EDDF".
        // Neither statement can now be made without the other agreeing.
        let site = arrival::locate(
            &flight.arr_airport,
            snap.lat,
            snap.lon,
            stats.planned_arr_ref_pos,
        );
        let near_planned = site.is_at_planned();

        // v0.7.5 Phase-Safety Hotfix (Spec docs/spec/flight-phase-state-
        // machine.md §13.8): siehe arrived_fallback_conditions_basic.
        //
        // Helicopters routinely keep the rotor running after touchdown (hot
        // ops: EMS offload, sightseeing turnaround, pad/rig work), so the
        // engines-off requirement would never finalise their arrival. Relax it
        // for rotorcraft — but ONLY at the planned destination (`near_planned`),
        // with the same on-ground + stationary + ARRIVED_FALLBACK_DWELL_SECS
        // guards.
        //
        // v0.19.3 briefly widened this to "any airport we can name", to stop a
        // helicopter from hanging forever at a destination whose geometry we
        // couldn't measure. That was a bad trade: the relaxed path needs neither
        // parking brake nor engines off, just 9 seconds of standing still — so a
        // helicopter making an en-route fuel stop at an intermediate field would
        // have been force-promoted to Arrived, its block time frozen at the fuel
        // stop, and its position stream killed for the remainder of the leg. The
        // hang it was meant to prevent is gone anyway: `arrival::locate` now has
        // two independent ways to recognise the planned field (see there), so
        // `near_planned` is reliable even where the runway table is silent.
        let engines_off_path = if category.is_rotorcraft() && near_planned {
            snap.on_ground && snap.groundspeed_kt < 1.0
        } else {
            arrived_fallback_conditions_basic(
                snap.on_ground,
                snap.engines_running,
                snap.groundspeed_kt,
            )
        };
        // v0.17.x (#Premium, Aircraft-Scan): Stillstands-Fallback für Addons,
        // deren `engines_running` nach dem Shutdown klemmt (Contrail FA50
        // Trijet: bleibt bei 3 → der engines-off-Pfad feuert NIE, Auto-File
        // hängt). Ein Flieger, der am Zielflughafen voll steht (< 1 kt) MIT
        // gesetzter Parkbremse, ist angekommen — unabhängig vom (falschen)
        // Triebwerkszähler. NUR `near_planned` (der Divert-Pfad unten behält
        // die strikte engines-off-Bedingung).
        //
        // v0.18.x QA-Härtung: bewusst NICHT mehr fleet-weit (Einwand Thomas —
        // an großen Hubs ziehen Piloten die Parkbremse auch bei einem langen
        // Rollhalt, z. B. beim Warten auf weitere Rollfreigabe; ohne Gate hätte
        // das JEDEN Piloten mitten auf dem Rollweg beenden können). Läuft nur
        // noch für Flugzeuge mit bestätigt kaputtem Triebwerkszähler (siehe
        // `AircraftProfile::engine_count_unreliable`) — für alle anderen bleibt
        // ausschließlich der harte engines-off-Pfad übrig, exakt wie vor v0.17.x.
        // Stays gated on the PLANNED field, for the same reason as the rotorcraft
        // path above: an FA50 tech-stopping with the brake set at an intermediate
        // airport would otherwise be declared "arrived" five minutes later, and
        // the rest of its leg would go untracked. An FA50 that genuinely diverts
        // reaches Arrived through the ordinary engines-off path or, failing that,
        // the pilot files manually — a diverted flight is an event he is already
        // dealing with.
        let standstill_path = snap.aircraft_profile.engine_count_unreliable()
            && arrived_standstill_condition(
                near_planned,
                snap.on_ground,
                snap.groundspeed_kt,
                snap.parking_brake,
            );
        let conditions_basic = engines_off_path || standstill_path;
        if conditions_basic {
            // Shared wall-clock timer for both paths (see
            // arrived_fallback_pending_since's doc comment for why it's
            // deliberately shared rather than reset on every path switch).
            // Only the dwell threshold differs by path.
            let pending_at = stats.arrived_fallback_pending_since.get_or_insert(now);
            let elapsed = (now - *pending_at).num_seconds();
            let dwell_needed = if engines_off_path {
                crate::grenzwerte::grenzwerte().phasen_kern.arrived_fallback_dwell_secs.wert
            } else {
                crate::grenzwerte::grenzwerte().phasen_kern.arrived_standstill_dwell_secs.wert
            };
            if elapsed >= dwell_needed {
                // Two paths to Arrived from here:
                //
                //   1. at the planned field → normal arrival
                //   2. anywhere else        → DIVERT — the hint lets the cockpit
                //                             ask the pilot to confirm the real
                //                             destination and file with the
                //                             correct arr_airport_id
                //
                // The hint is *derived from the same `site`* that decided
                // `near_planned` above — it is not a second, independent
                // measurement. `DivertHint::from_site` returns None for an
                // AtPlanned site, so branch 1 cannot produce a hint even if
                // some future edit lets it reach this code.
                let detected_hint = DivertHint::from_site(
                    &site,
                    &flight.arr_airport,
                    stats.planned_alternate.as_deref(),
                );
                if let Some(hint) = detected_hint.as_ref() {
                    tracing::info!(
                        planned = %flight.arr_airport,
                        actual = ?hint.actual_icao.as_deref(),
                        dist_nmi = hint.distance_to_planned_nmi,
                        kind = hint.kind,
                        "divert detected"
                    );
                }

                // The fallback fires wherever the aircraft came to rest — at
                // the planned field (normal arrival) or somewhere else (a
                // divert the pilot still has to confirm).
                tracing::info!(
                    prev_phase = ?prev_phase,
                    elapsed,
                    near_planned,
                    diverted = detected_hint.is_some(),
                    "Arrived fallback fired — forcing FSM to Arrived"
                );
                next_phase = FlightPhase::Arrived;
                // The fallback skips BlocksOn. Capture what BlocksOn would
                // have captured (block-on time, arrival stand) so the flight
                // record does not depend on WHICH path got us to Arrived —
                // see `settle_at_arrival_stand`. The stand is recorded against
                // the field we are actually on, which on a divert is not the
                // planned one.
                let at_icao = match &site {
                    arrival::ArrivalSite::AtOtherAirport { icao, .. } => icao.clone(),
                    _ => flight.arr_airport.clone(),
                };
                settle_at_arrival_stand(&mut stats, snap, now, &at_icao);
                // Helicopters often don't fire a touchdown event the
                // way the analyzer expects. If we never recorded
                // landing_at, stamp it now so the file body has a
                // sensible flight_time. (Shared helper — the enroute-
                // reconcile Landing pull needs the exact same rescue.)
                rescue_landing_at_from_sampler(&mut stats, now);
                if let Some(hint) = detected_hint {
                    stats.divert_hint = Some(hint);
                }
            }
        } else {
            stats.arrived_fallback_pending_since = None;
        }
    } else {
        stats.arrived_fallback_pending_since = None;
    }

    stats.was_on_ground = Some(snap.on_ground);
    stats.was_parking_brake = Some(snap.parking_brake);

    // En-Route-Versöhner (siehe Doc an `ENROUTE_RECONCILE_DWELL_SECS`):
    // läuft NACH allen regulären Übergangs-Armen und NUR, wenn keiner von
    // ihnen diesen Tick etwas entschieden hat — der Primärpfad behält
    // immer Vorrang. Liest `stats.shadow_phase` vom VORHERIGEN Tick
    // (die Schatten-Engine tickt im Streamer nach `step_flight`) — bei
    // 90 s Dwell irrelevant. `Insufficient` = v2 hat gerade keine echte
    // Kinematik-Evidenz (frisch resettet nach Reload/Time-Skip) → nicht
    // nur nicht feuern, sondern die Divergenz-Uhr komplett neu starten.
    if next_phase == prev_phase && !reload_gap_suspect {
        // PRIMÄRES Gate: die Kinematik muss v1s Phase aktiv WIDERLEGEN —
        // bloßes v1≠v2 genügt nicht (QS-Runde 1: sonst Oszillation auf
        // gesunden Flügen, siehe segment_contradicts_v1-Doc). Subsumiert
        // `Insufficient` (widerspricht nie → keine Evidenz, kein Urteil).
        let contradicted =
            segment_contradicts_v1(prev_phase, stats.shadow_engine.current_segment());
        match stats.shadow_phase {
            Some(v2)
                if enroute_reconcile_eligible(prev_phase, v2)
                    && contradicted
                    // QS-Runde 1: `Landing` als Ziel nur mit ROHEM
                    // on_ground dieses Ticks — v2s Ground-Mehrheit ist
                    // fensterträge; hebt der Flieger im Versatz-Tick
                    // wieder ab (Balked Landing), würde ein Luft-Zug auf
                    // Landing BEIDE Engines unlösbar festnageln (v2
                    // spiegelt Landing 1:1 zurück, v1=Landing hat ohne
                    // landing_at keinen Luft-Ausstieg, der Versöhner
                    // selbst ist raus).
                    && (v2 != FlightPhase::Landing || snap.on_ground) =>
            {
                // Uhr ist an v1s STILLSTAND gekoppelt, nicht an ein
                // stabiles (v1, v2)-Paar — v2 darf während des Dwells
                // legitim weiterwandern (siehe Feld-Doc). Gezogen wird
                // auf v2s AKTUELLE Phase (frischeste Evidenz).
                let since = match stats.enroute_divergence_since {
                    Some((p1, t)) if p1 == prev_phase => t,
                    _ => {
                        stats.enroute_divergence_since = Some((prev_phase, now));
                        now
                    }
                };
                // Asymmetrischer Dwell nach Zug-Richtung — siehe
                // ENROUTE_RECONCILE_DWELL_SECS-Doc. Die Richtung wird pro
                // Tick aus der AKTUELLEN v2-Phase bestimmt: wandert v2
                // während des Dwells vorwärts (Descent→Approach→Final),
                // gilt ab da der Vorwärts-Dwell — die Uhr selbst läuft
                // von ihrem einen Startpunkt weiter.
                let dwell_needed = if enroute_reconcile_is_forward(prev_phase, v2) {
                    crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_forward_dwell_secs.wert
                } else {
                    crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert
                };
                if (now - since).num_seconds() >= dwell_needed {
                    tracing::warn!(
                        ?prev_phase,
                        target = ?v2,
                        held_secs = (now - since).num_seconds(),
                        dwell_needed,
                        "enroute-reconcile: v1 auf Luft-Phase festgefahren, Kinematik \
                         widerlegt sie, v2-Evidenz zeigt beständig eine andere Phase — \
                         ziehe v1 nach"
                    );
                    stats.enroute_divergence_since = None;
                    // Timer-Hygiene (QS-Runde 1): armierte Dwell-Timer
                    // anderer Detektoren dürfen den Quer-Zug nicht als
                    // Alt-Zustand überleben — ein stehen gebliebener
                    // go_around_climb_pending_since könnte bei späterem
                    // Approach-Wiedereintritt via get_or_insert die 8-s-
                    // GA-Dwell sofort erfüllen (falscher Go-Around).
                    stats.descent_pending_since = None;
                    stats.go_around_climb_pending_since = None;
                    stats.touch_and_go_pending_since = None;
                    match v2 {
                        FlightPhase::Approach => {
                            // Gleiche Entry-Semantik wie die reguläre
                            // Descent→Approach-Kante: frisches Per-
                            // Anflug-Bild (Werte eines FRÜHEREN Anflugs
                            // dürfen nicht in den Touchdown-VS-Fallback
                            // sickern). Bei Ziel Final BEWUSST nicht —
                            // dort läuft der aktuelle Anflug bereits,
                            // seine Low-AGL-Samples sind die Wahrheit.
                            stats.low_agl_vs_min_fpm = None;
                            stats.last_low_agl_vs_fpm = None;
                        }
                        FlightPhase::Landing => {
                            // Zug direkt auf Landing = die Final→Landing-
                            // Kante (dort latcht normal landing_at) wurde
                            // übersprungen — ohne diese Rettung bliebe
                            // landing_at=None (aufgeblähte flight_time,
                            // kein LandingRecord, Landing-Arm-Analyzer
                            // komplett tot, is_finalizable=false).
                            rescue_landing_at_from_sampler(&mut stats, now);
                        }
                        _ => {}
                    }
                    next_phase = v2;
                }
            }
            _ => {
                stats.enroute_divergence_since = None;
            }
        }
    } else {
        // Frische v1-Transition (= lebendige FSM, wird NIE übersteuert)
        // oder Reload-Glitch-Tick: Divergenz-Uhr neu starten.
        stats.enroute_divergence_since = None;
    }

    // 20.10.2026 („Stück 3“): Wegsteigen nach einem Aufsetzen schliesst die
    // Landung ab — in jeder Phase, in der eine offen ist (auch die Busch-
    // Landung in `Takeoff`), ohne Zeitgrenze ab dem Aufsetzen. NACH dem
    // Match: Der Landing-Arm hat in diesem Takt schon Rollstrecke und Spur
    // fortgeschrieben, der Abschluss raeumt sie mit weg. VOR der
    // Note-Finalisierung: Die sieht danach keine offene Landung mehr.
    if wegsteigen_nach_aufsetzen(&stats, snap, now) {
        let seit = *stats.touch_and_go_pending_since.get_or_insert(now);
        if (now - seit).num_seconds() >= crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_dwell_secs.wert {
            aufsetzer_einfrieren(flight, &mut stats, snap);
            landung_beim_wegsteigen_abschliessen(&mut stats, snap, now);
        }
    } else {
        stats.touch_and_go_pending_since = None;
    }
    if stats.landung_abgebrochen_phase_offen {
        if prev_phase == FlightPhase::Landing {
            next_phase = FlightPhase::Climb;
        }
        stats.landung_abgebrochen_phase_offen = false;
    }

    // v1.7.24: JEDEN Tick, unabhaengig von `prev_phase`/`next_phase` — siehe
    // `finalize_landing_score_if_due`s Dokumentation (GSG1249). Muss NACH
    // dem `match prev_phase`-Block oben stehen (nicht davor), damit
    // Aenderungen an `stats.landing_at`/`stats.bounce_count` aus DIESEM Tick
    // (z.B. der `FlightPhase::Landing`-Arm) schon eingeflossen sind.
    finalize_landing_score_if_due(
        &mut stats,
        &flight.pirep_id,
        now,
        snap.altitude_agl_ft as f32,
        snap.on_ground,
    );
    stats.zwischenstand("note");

    if next_phase != prev_phase {
        // v0.7.5 Phase-Safety Hotfix (Spec §13.9): siehe should_reset_holding_pending.
        if should_reset_holding_pending(prev_phase, next_phase) {
            stats.kurs_verlauf.clear();
        }
        stats.phase = next_phase;
        stats.transitions.push((now, next_phase));
        if next_phase == FlightPhase::Arrived {
            // Vor dem Auto-Einreichen, das an genau diesem Uebergang haengt.
            //
            // Nur wenn der Sim die Triebwerke AUS meldet: `Arrived` erreicht
            // auch ein Hubschrauber mit laufendem Rotor (Stillstand am Boden
            // genuegt) oder ein Add-on mit klemmendem Triebwerkszaehler ueber
            // den Stillstands-Weg. Dort gibt es keinen Abstell-Zeitpunkt, und
            // ein geratener waere ein zu kleiner Rollsprit (QS Runde 3, F1).
            let plausibler_tank_kg = stats.last_fuel_kg.unwrap_or(snap.fuel_total_kg);
            sprit_flug_abschliessen(
                &mut stats,
                Some(plausibler_tank_kg),
                snap.engines_running == 0,
            );
        }
        Some(next_phase)
    } else {
        None
    }
}

#[cfg(test)]
#[path = "tests_lib/weiterflug_vom_boden_tests.rs"]
mod weiterflug_vom_boden_tests;

#[cfg(test)]
#[path = "tests_lib/arrived_fallback_dwell_tests.rs"]
mod arrived_fallback_dwell_tests;

/// v0.19.3 — the arrived-fallback on a REAL airport, with real geometry.
///
/// The dwell tests above deliberately fly to `ZZZZ` so that the geometry
/// short-circuits and only the timer is under test. That isolation is exactly
/// why they never caught this: the fallback's geometry was wrong, and no test
/// pointed it at a real field.
///
/// Field report (Peter, 2026-07-12, DLH859 ENGM→EDDF on v0.19.2): parked at
/// EDDF after a flight planned to EDDF, the app announced
///
///     "Anderer Landeplatz erkannt — Du bist gelandet in EDDF statt geplant
///      EDDF (~2 nmi vom Ziel entfernt)"
///
/// and withheld auto-filing. Root cause: "am I near the planned field" was
/// measured against the runway-layout CENTROID (2.04 nm from a T2 stand —
/// outside the 2 nm radius) while "which field am I on" was measured against
/// the nearest RUNWAY THRESHOLD (0.30 nm — inside it). Both answers were
/// "true" at once. See `crate::arrival`.
/// v0.19.3 — the takeoff roll is recognised by evidence of a takeoff, not by
/// taxi speed.
///
/// Field report (Thomas, DLH367 KCLT→EDDM, ToLiss A346, 2026-07-12): the client
/// announced "Phase: TakeoffRoll" at 30 kt with the aircraft still taxiing,
/// dropped back to TaxiOut two minutes later, then rolled for real. A heavy
/// widebody taxis through 30 kt without trying. The flip-flop also re-fired the
/// block-off transition, which is why "Fuel & Weight @ Block-off" appeared three
/// times in his activity log.
/// QS round 7 — the filing gate must still be able to place the aircraft after a
/// resume, or it silently waves through a PIREP it should refuse.
#[cfg(test)]
#[path = "tests_lib/position_for_gates_tests.rs"]
mod position_for_gates_tests;

#[cfg(test)]
#[path = "tests_lib/null_island_tests.rs"]
mod null_island_tests;

#[cfg(test)]
#[path = "tests_lib/takeoff_roll_tests.rs"]
mod takeoff_roll_tests;

#[cfg(test)]
#[path = "tests_lib/arrived_fallback_geometry_tests.rs"]
mod arrived_fallback_geometry_tests;

#[cfg(test)]
#[path = "tests_lib/enroute_reconcile_tests.rs"]
mod enroute_reconcile_tests;

#[cfg(test)]
#[path = "tests_lib/enroute_reconcile_replay_tests.rs"]
mod enroute_reconcile_replay_tests;
