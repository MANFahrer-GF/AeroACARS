/// v0.7.1 Helper: actual_trip_burn = takeoff_fuel - landing_fuel
/// (1:1 wie in build_landing_record line 6527-6530). Damit sub_scores
/// + aggregate_master_score den gleichen Wert nutzen.
// ─────────────────────────────────────────────────────────────────────────
// v1.7.35 — Sprit-Auswertung ohne Note. Messung im Flug, Rechnung in
// `landing_scoring::sprit::auswerten`, Anzeige ueberall aus dem Ergebnis.
// Siehe `crates/landing-scoring/src/sprit.rs` fuer das Warum.
// ─────────────────────────────────────────────────────────────────────────

// Schwelle „tief" = Platzhoehe des Ziels + 8 000 ft. Ueber Platzhoehe,
// nicht MSL (Nairobi liegt auf 5 330 ft) und nicht Terrain-AGL (Innsbruck
// springt). Gedeckelt unter der geplanten Reiseflughoehe, damit eine C172
// nicht den ganzen Flug „tief" ist.
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_schwelle_ueber_platz_ft (v2.1.24: 8_000.0)
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_schwelle_abstand_zur_reisehoehe_ft (v2.1.24: 1_000.0)
// Ein Tick-Abstand ueber dieser Grenze ist eine Pause oder ein Neustart —
// die Zeit dazwischen wurde nicht geflogen und zaehlt nicht.
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_tick_max_s (v2.1.24: 60.0)
// Wie weit die gemessene Position neben der geplanten Route liegen darf,
// damit der Plan-Anteil noch am Ort bestimmt wird (`sprit_plan_am_ort`).
//
// 25 NM deckt Radarvektoren und Warteschleifen ab. Wer weiter draussen
// fliegt, folgt nicht mehr dem Plan — dort waere ein Plan-Anteil geraten,
// und lieber keine Phase als eine ausgedachte.
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_projektion_max_abstand_nm (v2.1.24: 25.0)

// Ab wann zwei Segmente als „gleich nah" gelten und die geplante Zeit
// entscheidet. Nur fuer den echten Mehrdeutigkeitsfall — Warteschleife,
// Prozedurkehre, eine STAR, die zweimal an derselben Stelle vorbeifuehrt.
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_projektion_gleichstand_nm (v2.1.24: 1.0)
// Ein Landesprit unter 1 % des Startsprits ist kein Flug, sondern ein
// Auslesefehler. (Der Kommentar nannte frueher KMRH→TXKF als Beispiel —
// die Korpus-Pruefung hat gezeigt, dass dieser Flug mit 2,8 % darueber
// liegt und seine Reserve-Unterschreitung echt ist: ein Leichtflugzeug,
// das als A346 verbucht wurde.)
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_tank_min_anteil (v2.1.24: 0.01)

// Wie weit unter der Reiseflughoehe der Sinkflug als begonnen gilt.
//
// Der Vergleichspunkt darf NICHT allein an der Reststrecke haengen: die
// Plan-Reststrecke ist Strecke ueber Grund entlang STAR und Transition, die
// gemessene Restdistanz eine Luftlinie. Der Umweganteil betraegt im Median
// +44 % (Korpus-Pruefung, 38 Fluege) — die Bedingung wird also weit
// draussen im Reiseflug wahr. Gemessen lag der Punkt im Median 4 Minuten
// und 23 NM vor dem echten Sinkflugbeginn, im Extremfall 16 Minuten.
// Dadurch rutschte Reiseflug in die Anflugphase und „bis Sinkflug" wurde
// systematisch zu negativ (Median −7,2 % statt −5,0 %, Einzelfaelle bis
// 18 Prozentpunkte).
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_sinkflug_unter_reisehoehe_ft (v2.1.24: 1_500.0)

/// Aus dem Navlog: geplante Reststrecke ab TOD (Summe der Segmente nach
/// dem TOD-Fix) und geplanter Verbrauch bis TOD. `None`, wenn das Navlog
/// die Felder nicht traegt (Altbestand) oder keinen TOD kennt.
fn sprit_plan_aus_navlog(fixes: &[api_client::RouteFix]) -> (Option<f64>, Option<f32>) {
    let Some(tod) = fixes
        .iter()
        .position(|f| f.ident.eq_ignore_ascii_case("TOD"))
    else {
        return (None, None);
    };
    let bis = fixes[tod].sprit_bis_hier_kg.filter(|v| *v > 0.0);
    // Summe der Segmente nach dem TOD. Ein EINZELNER Fix ohne `<distance>`
    // darf die Summe nicht ausknipsen (das waere ein stiller Totalausfall
    // von Vergleichspunkt, beiden Phasen und Zeitmessung) — solange die
    // ueberwaeltigende Mehrheit der Segmente eine Laenge hat, ist die Summe
    // brauchbar.
    let nach_tod = &fixes[tod + 1..];
    let mit_laenge = nach_tod.iter().filter(|f| f.segment_nm.is_some()).count();
    let rest: f64 = nach_tod
        .iter()
        .filter_map(|f| f.segment_nm.map(|v| v as f64))
        .sum();
    let vollstaendig_genug = !nach_tod.is_empty() && mit_laenge * 10 >= nach_tod.len() * 9;
    ((vollstaendig_genug && rest > 0.0).then_some(rest), bis)
}

/// Was der Plan an einem bestimmten ORT vorsieht.
///
/// # Warum nicht am TOD-Fix ablesen
///
/// Bis v1.7.35 wurde der Plan-Anteil immer am TOD-Fix genommen, egal wo der
/// Vergleichspunkt tatsaechlich lag. Am Korpus gemessen (18.09.2026) liegt er
/// bei 14 von 39 Fluegen unter 80 % der Plan-Reststrecke, bei vier unter
/// 45 %. Dann wird ein kurzes Ist-Stueck gegen einen VOLLEN Plan gerechnet —
/// der Anflug-Prozentwert ging bis −67,6 %. Und in der Gegenrichtung genauso:
/// Liegt der Punkt hinter dem Plan-TOD, deckt „Abheben bis Punkt" mehr Flug
/// ab als der Plan-Anteil, und „bis Sinkflug" wird systematisch zu positiv —
/// also genau die Phase, auf der die Begruendung dieses Releases ruht.
///
/// # Warum nicht entlang der Reststrecke interpolieren
///
/// Der naheliegende Weg — „suche den Punkt, an dem noch so viele NM uebrig
/// sind wie gemessen" — macht es SCHLECHTER. Am Korpus gerechnet steigt der
/// Anflug-Median dabei von +44,9 % auf +62,0 %, weil er eine Luftlinie gegen
/// ein Streckenmass haelt. Eine Route ist im Median 44 % laenger als die
/// Luftlinie; das ist derselbe Fehler, den die Sinkflug-Bedingung schon
/// einmal beheben musste.
///
/// # Was hier stattdessen passiert
///
/// Verglichen wird ORT gegen ORT. Jeder Navlog-Fix traegt `lat`/`lon`. Die
/// Flugzeugposition wird senkrecht auf den Streckenzug projiziert; am
/// Lotfusspunkt werden Plan-Sprit und Plan-Reststrecke zwischen den beiden
/// Nachbarfixen interpoliert. Damit wird nie wieder Luftlinie gegen Strecke
/// gerechnet, und der Umwegfaktor ergibt sich pro Flug und pro Ort aus der
/// Plan-Geometrie selbst.
///
/// # Mehrdeutigkeit und Plausibilitaet
///
/// Die Entfernung zum Streckenzug ist nicht monoton: Holding, Prozedurkehre,
/// eine STAR, die zunaechst vom Platz wegfuehrt, oder ein Rundflug, bei dem
/// SID und STAR uebereinander liegen, machen mehrere Segmente gleich nah.
///
/// Entschieden wird ueber die GEFLOGENE STRECKE, nicht ueber die Uhr.
///
/// **Welche Strecke:** das Groessere aus Wegzaehler und Luftlinie vom
/// ersten Fix bis hierher. Der Wegzaehler allein zaehlt zu wenig, sobald er
/// Stuecke verwirft — bei Sim-Rate ab etwa 5× im Reiseflug jedes Segment
/// ueber 2 200 kt, nach einem Neustart oder Reposition die Luecke. Die
/// Luftlinie ist eine harte UNTERgrenze der geflogenen Strecke, gleich ob
/// Zeitraffer, Neustart oder Sprung (QS-Befund E4, 18.09.2026 — die
/// fruehere Zusage „unempfindlich gegen Sim-Rate" stimmte nicht).
///
/// * **Plausibilitaet — ein Vorzug, kein Ausschluss:** Kandidaten, deren
///   Planposition mehr als doppelt so weit liegt wie die geflogene Strecke,
///   treten zurueck. Beim Rundflug projiziert eine Absenkung kurz nach dem
///   Start sonst auf ein STAR-Segment am Ende der Route, und der Punkt
///   schnappt still zu frueh zu (QS-Befund V3). Bleibt nach der Regel
///   aber KEIN Kandidat uebrig, gilt der naechstgelegene ohne sie — sonst
///   verloere ein Flug mit untergezaehlter Strecke seinen Messpunkt und
///   damit beide Phasen.
/// * **Gleichstand:** Liegen zwei Segmente gleich nah, gewinnt das, dessen
///   Planposition der geflogenen Strecke am naechsten kommt.
///
/// Gibt `None`, wenn kein Segment nah genug liegt — dann ist das Flugzeug
/// nicht auf der geplanten Route, und ein Plan-Anteil waere geraten.
fn sprit_plan_am_ort(
    fixes: &[api_client::RouteFix],
    lat: f64,
    lon: f64,
    geflogen_nm: Option<f64>,
) -> Option<SpritPlanOrt> {
    // Jedes `segment_nm` ist die Strecke ZUM jeweiligen Fix — die Summe
    // aller ist die geplante Gesamtstrecke.
    let plan_gesamt_nm: f64 = fixes
        .iter()
        .filter_map(|f| f.segment_nm.map(|v| v as f64))
        .sum();
    // Die geflogene Strecke, nach unten abgesichert durch die Luftlinie ab
    // dem ersten Fix — siehe oben.
    let luftlinie_nm = fixes
        .first()
        .map(|f| ::geo::distance_m(f.lat, f.lon, lat, lon) / 1852.0)
        .filter(|d| d.is_finite() && *d > 0.0);
    let geflogen = match (
        geflogen_nm.filter(|g| g.is_finite() && *g > 0.0),
        luftlinie_nm,
    ) {
        (Some(g), Some(l)) => Some(g.max(l)),
        (g, l) => g.or(l),
    };
    sprit_plan_am_ort_suche(fixes, lat, lon, geflogen, plan_gesamt_nm, true)
        .or_else(|| sprit_plan_am_ort_suche(fixes, lat, lon, geflogen, plan_gesamt_nm, false))
}

/// Ein Durchgang von `sprit_plan_am_ort` — mit oder ohne Plausibilitaetsregel.
fn sprit_plan_am_ort_suche(
    fixes: &[api_client::RouteFix],
    lat: f64,
    lon: f64,
    geflogen: Option<f64>,
    plan_gesamt_nm: f64,
    mit_regel: bool,
) -> Option<SpritPlanOrt> {
    let mut beste: Option<(f64, f64, SpritPlanOrt)> = None;
    for i in 0..fixes.len().saturating_sub(1) {
        let (a, b) = (&fixes[i], &fixes[i + 1]);
        let Some(t) = strecken_anteil(a.lat, a.lon, b.lat, b.lon, lat, lon) else {
            continue;
        };
        // Lotfusspunkt und sein Abstand zur gemessenen Position.
        let fuss_lat = a.lat + (b.lat - a.lat) * t;
        let fuss_lon = a.lon + (b.lon - a.lon) * t;
        let abstand_nm = ::geo::distance_m(lat, lon, fuss_lat, fuss_lon) / 1852.0;
        if abstand_nm > crate::grenzwerte::grenzwerte().sprit.sprit_projektion_max_abstand_nm.wert {
            continue;
        }
        // Plan-Sprit am Lotfusspunkt: zwischen den Nachbarfixen interpoliert.
        let (Some(fa), Some(fb)) = (a.sprit_bis_hier_kg, b.sprit_bis_hier_kg) else {
            continue;
        };
        let plan_bis_kg = fa + (fb - fa) * t as f32;
        // Plan-Reststrecke: der Rest des laufenden Segments plus alles danach.
        let Some(seg) = b.segment_nm.map(|v| v as f64) else {
            continue;
        };
        let rest_danach: f64 = fixes[i + 2..]
            .iter()
            .filter_map(|f| f.segment_nm.map(|v| v as f64))
            .sum();
        let plan_rest_nm = seg * (1.0 - t) + rest_danach;
        // Wie weit der Lotfusspunkt auf dem Plan vom Start entfernt liegt —
        // und passt das zu der Strecke, die tatsaechlich geflogen wurde?
        let plan_position_nm = (plan_gesamt_nm - plan_rest_nm).max(0.0);
        let strecken_abweichung = match geflogen {
            Some(g) => {
                // Radarvektoren machen die geflogene Strecke LAENGER als den
                // Plan, nie halb so kurz. Liegt der Kandidat mehr als doppelt
                // so weit wie das bisher Geflogene, ist er ein anderes Stueck
                // der Route (Rundflug: STAR ueber der SID).
                if mit_regel && plan_position_nm > 2.0 * g + crate::grenzwerte::grenzwerte().sprit.sprit_projektion_gleichstand_nm.wert {
                    continue;
                }
                (plan_position_nm - g).abs()
            }
            None => f64::MAX,
        };
        // „Zeiten gegen Flugplan": Planzeit am Lotfusspunkt, interpoliert.
        let plan_bis_s = match (a.zeit_bis_hier_s, b.zeit_bis_hier_s) {
            (Some(za), Some(zb)) => Some(za as f64 + (zb - za) as f64 * t),
            _ => None,
        };
        let kandidat = SpritPlanOrt {
            plan_bis_kg,
            plan_bis_s,
            plan_rest_nm,
            abstand_nm,
            segment: i,
        };
        // **Der Abstand entscheidet, die Strecke nur bei Gleichstand.**
        //
        // Umgekehrt waere es falsch: Ein Segment, das 20 NM entfernt liegt,
        // aber zufaellig die passende Planposition traegt, wuerde ein direkt
        // getroffenes verdraengen. Genau das ist beim ersten Bauen passiert —
        // der Plan-Anteil kam vom TOD statt vom Fix, auf dem das Flugzeug
        // stand. Die Strecke ist nur dafuer da, eine Warteschleife oder
        // Prozedurkehre aufzuloesen, wo mehrere Segmente GLEICH nah sind.
        let besser = match &beste {
            None => true,
            Some((bz, ba, _)) => {
                if (abstand_nm - *ba).abs() <= crate::grenzwerte::grenzwerte().sprit.sprit_projektion_gleichstand_nm.wert {
                    strecken_abweichung < *bz
                } else {
                    abstand_nm < *ba
                }
            }
        };
        if besser {
            beste = Some((strecken_abweichung, abstand_nm, kandidat));
        }
    }
    beste.map(|(_, _, k)| k)
}

/// Der Plan an einem gemessenen Ort — siehe `sprit_plan_am_ort`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SpritPlanOrt {
    /// Geplanter Verbrauch bis hierher.
    plan_bis_kg: f32,
    /// Geplante Flugzeit bis hierher (s ab Start, aus `<time_total>`).
    plan_bis_s: Option<f64>,
    /// Geplante Reststrecke ab hier, Strecke ueber Grund.
    plan_rest_nm: f64,
    /// Wie weit die gemessene Position neben der Route lag.
    abstand_nm: f64,
    /// Der Abschnitt `fixes[segment] → fixes[segment + 1]`, auf dem der
    /// Lotfusspunkt liegt. Für die Überflug-Erkennung je Wegpunkt.
    segment: usize,
}

/// Wo auf der Strecke A→B liegt der Lotfusspunkt von P? `0.0` = bei A,
/// `1.0` = bei B, dazwischen anteilig. Ausserhalb wird geklemmt, damit ein
/// Punkt vor dem ersten oder hinter dem letzten Fix nicht durchfaellt.
///
/// Gerechnet wird in einer lokalen ebenen Naeherung: Bei Segmentlaengen von
/// wenigen bis einigen zehn NM ist der Fehler der Erdkruemmung kleiner als
/// die Ungenauigkeit, die wir ohnehin haben.
fn strecken_anteil(
    a_lat: f64,
    a_lon: f64,
    b_lat: f64,
    b_lon: f64,
    p_lat: f64,
    p_lon: f64,
) -> Option<f64> {
    // Laengengrade schrumpfen zum Pol hin — ohne diesen Faktor waere die
    // Projektion in Nordeuropa deutlich verzerrt.
    let cos_lat = a_lat.to_radians().cos();
    let (ax, ay) = (a_lon * cos_lat, a_lat);
    let (bx, by) = (b_lon * cos_lat, b_lat);
    let (px, py) = (p_lon * cos_lat, p_lat);
    let (dx, dy) = (bx - ax, by - ay);
    let laenge2 = dx * dx + dy * dy;
    if !laenge2.is_finite() || laenge2 <= f64::EPSILON {
        return None;
    }
    let t = ((px - ax) * dx + (py - ay) * dy) / laenge2;
    t.is_finite().then(|| t.clamp(0.0, 1.0))
}

/// Bezugshöhe für die Anflugauswertung („Höhe über der Schwelle").
///
/// DLH 780 (26.09.2026, Divert EDDF→EDDM statt WSSS): Die Höhe kam immer vom
/// GEPLANTEN Ziel (WSSS, ~22 ft). Im Endanflug auf München (1 487 ft) lag
/// der Flieger rechnerisch ~1 460 ft zu hoch, kam nie ins 1000-ft-Fenster,
/// und alle Anflugwerte blieben leer.
///
/// Normale Flüge: unverändert die Zielhöhe (Anzeige wie bisher; die Note
/// liest diese Anflugfelder ohnehin nicht). Divert: Schwellenhöhe der
/// tatsächlichen Bahn aus den Navdaten, sonst die Platzhöhe des
/// tatsächlichen Flughafens aus den Navdaten, sonst `None` → Höhe über Grund
/// statt einer falschen Platzhöhe.
///
/// „Divert" entscheidet `runway_correlation_icao` — das Ergebnis von
/// `correlate_airport_icao`, das bei Nachbarplätzen das geplante Ziel
/// bevorzugt. NICHT `runway_match.airport_ident`: der OurAirports-Rückfall
/// sucht global die nächste Bahn und kann bei Doppelplätzen einen anderen
/// Ident liefern, obwohl am Ziel gelandet wurde (QS 26.09.2026).
fn anflug_bezugshoehe_ft(
    stats: &FlightStats,
    geplant_arr: &str,
    ist_platzhoehe_navdaten_ft: Option<i32>,
) -> Option<f32> {
    let Some(ist) = stats.runway_correlation_icao.as_deref() else {
        return stats.arr_airport_elevation_ft;
    };
    if ist.trim().eq_ignore_ascii_case(geplant_arr.trim()) {
        return stats.arr_airport_elevation_ft;
    }
    stats
        .runway_nav_geometry
        .as_ref()
        .and_then(|g| g.threshold.elev_ft)
        .or(ist_platzhoehe_navdaten_ft)
        .map(|e| e as f32)
}

/// Platzhöhe des tatsächlich angeflogenen Flughafens aus dem Navdaten-Cache.
fn ist_platzhoehe_navdaten_ft(flight: &ActiveFlight, stats: &FlightStats) -> Option<i32> {
    let icao = stats.runway_correlation_icao.as_deref()?;
    flight
        .navdata
        .lock()
        .expect("navdata lock")
        .get(icao)
        .and_then(|a| a.elevation_ft)
}

fn sprit_schwelle_ft(stats: &FlightStats) -> Option<f64> {
    let platz = stats.arr_airport_elevation_ft? as f64;
    let mut schwelle = platz + crate::grenzwerte::grenzwerte().sprit.sprit_schwelle_ueber_platz_ft.wert;
    if let Some(cruise) = stats.planned_cruise_alt_ft {
        schwelle = schwelle.min(cruise - crate::grenzwerte::grenzwerte().sprit.sprit_schwelle_abstand_zur_reisehoehe_ft.wert);
    }
    (schwelle > platz).then_some(schwelle)
}

/// Die Sprit-Marken am Boden: wenn die Triebwerke anlaufen und wenn sie
/// ausgehen.
///
/// # Warum ein eigener Pfad
///
/// `sprit_tick` steigt bei `on_ground` aus und misst nur zwischen Abheben
/// und Aufsetzen. Genau davor und danach liegen aber die beiden Marken, die
/// den Flug erst vollstaendig machen — der Rollsprit.
///
/// # Warum nicht die vorhandenen Werte
///
/// `block_fuel_kg` ist ein laufender HOECHSTSTAND mit Betankungs-Erkennung,
/// kein Zeitpunkt: Wer den Client vor dem Tanken startet, haette einen
/// negativen Rollsprit. Am Korpus gemessen (18.09.2026) waeren 24 von 41
/// Werten negativ, einer davon −72 t. Und `last_fuel_kg` laeuft nach dem
/// Anhalten einfach weiter — APU am Gate, vergessener Client, alles landete
/// im Rollsprit nach der Landung. Beide sind fuer eine Auswertung untauglich;
/// deshalb zwei eigene, an ein EREIGNIS gerastete Marken.
///
/// # Entprellung
///
/// `engines_running` zappelt bei manchen Add-ons waehrend des Anlassens
/// (Aerosoft A340-600 und andere). Fuer das Anlaufen genuegt das trotzdem:
/// Wir nehmen den ERSTEN Tick mit laufendem Triebwerk, und ein Zappeln
/// davor verschiebt die Marke hoechstens um Sekunden mit stehendem Sprit.
/// Fuer das Abstellen zaehlt umgekehrt der erste Tick OHNE Triebwerk nach
/// der Landung — dort ist der Tank schon in Ruhe.
#[allow(clippy::too_many_arguments)]
fn sprit_boden_marken(
    stats: &mut FlightStats,
    fuel_kg: f32,
    engines_running: bool,
    on_ground: bool,
    paused: bool,
    slew: bool,
    replay_verdacht: bool,
    now: DateTime<Utc>,
) {
    // Dasselbe Tor wie ueberall: bei Pause, Slew oder Replay-Verdacht wird
    // nicht gemessen.
    if paused || slew || replay_verdacht || !on_ground {
        return;
    }
    if !(fuel_kg.is_finite() && fuel_kg > 0.0) {
        return;
    }
    // Vor dem Abheben: die erste Probe mit laufenden Triebwerken.
    if stats.takeoff_fuel_kg.is_none() {
        if engines_running && stats.engine_start_fuel_kg.is_none() {
            stats.engine_start_fuel_kg = Some(fuel_kg);
        }
        return;
    }
    // Nach der Landung: Triebwerke AUS — aber erst, wenn sie es
    // `SPRIT_TRIEBWERKE_AUS_S` lang durchgehend sind. Manche Add-ons lassen
    // `GENERAL ENG COMBUSTION` beim Rollen kurz auf 0 fallen; ein einziges
    // Zappeln haette „Triebwerke aus" sonst mitten auf dem Rollweg gerastet
    // (QS-Befund V1, 18.09.2026).
    if stats.landing_fuel_kg.is_none() || stats.engine_off_fuel_kg.is_some() {
        return;
    }
    if engines_running {
        stats.sprit_triebwerke_aus_seit = None;
        return;
    }
    let seit = *stats.sprit_triebwerke_aus_seit.get_or_insert(now);
    if (now - seit).num_milliseconds() < (crate::grenzwerte::grenzwerte().sprit.sprit_triebwerke_aus_s.wert * 1000.0) as i64 {
        return;
    }
    sprit_triebwerke_aus_rasten(stats, fuel_kg);
}

/// Die Marke „Triebwerke aus" setzen — und in eine schon eingefrorene
/// Auswertung GENAU EIN Feld nachtragen: den Rollsprit nach der Landung.
///
/// # Warum nur dieses Feld
///
/// Die Auswertung friert beim Aufsetzen ein. Bis v1.7.36-Entwurf wurde sie
/// hier vollstaendig neu gebildet — und dabei rechnete auch alles andere
/// neu, mit dem Stand von jetzt: Die Anflugstrecke enthielt das Rollen zum
/// Stand, der Anflug-Plan einen inzwischen ersetzten Navlog-Trip
/// (QS-Befunde E2/E3, 18.09.2026). „Einmal rechnen" heisst: Was beim
/// Aufsetzen feststand, bleibt. Neu ist nur, was es vorher nicht gab.
///
/// Dieselbe Funktion `landing_scoring::sprit::rollen_nach_landung` rechnet
/// hier und in `auswerten` — die Zahl ist in beiden Wegen dieselbe.
fn sprit_triebwerke_aus_rasten(stats: &mut FlightStats, fuel_kg: f32) {
    if stats.landing_fuel_kg.is_none() || stats.engine_off_fuel_kg.is_some() {
        return;
    }
    if !(fuel_kg.is_finite() && fuel_kg > 0.0) {
        return;
    }
    stats.engine_off_fuel_kg = Some(fuel_kg);
    let rollen = landing_scoring::sprit::rollen_nach_landung(stats.landing_fuel_kg, Some(fuel_kg));
    if let Some(a) = stats.sprit_auswertung.as_mut() {
        a.rollen_nach_landung_kg = rollen;
    }
}

/// Der Flug ist am Stand angekommen — jetzt MUSS „Triebwerke aus" feststehen.
///
/// # Warum das noetig ist
///
/// `sprit_boden_marken` rastet erst nach zehn Sekunden durchgehend
/// abgestellter Triebwerke (Entprellung). Das Auto-Einreichen feuert aber
/// schon beim Uebergang nach `Arrived`, und `flight_end` nimmt den Flug
/// sofort aus dem Zustand — danach laeuft kein Tick mehr. Wer nicht binnen
/// Sekunden nach Blocks-On abstellte, bekam den PIREP ohne „Rollen nach der
/// Landung", also gerade im Normalfall (QS-Befund E1, 18.09.2026).
///
/// Drei Aufrufer:
/// * **der Uebergang nach `Arrived`** — dort nur, wenn der Sim die
///   Triebwerke aus meldet. Auf dem normalen Weg hat die FSM das schon
///   bestaetigt (Triebwerke aus, Parkbremse, mindestens 30 s nach
///   Blocks-On) — eine staerkere Bestaetigung als die Zehn-Sekunden-Frist.
///   `Arrived` erreichen aber auch Hubschrauber mit laufendem Rotor und
///   Add-ons mit klemmendem Triebwerkszaehler; dort gibt es keinen
///   Abstell-Zeitpunkt.
/// * **`flight_end`** und **`flight_end_manual`** (Formular, z. B. nach
///   einem Ausweichflug), bevor der PIREP gebaut wird — fuer das Einreichen
///   vor `Arrived`. Dort nur, wenn die Triebwerke gerade aus sind (die
///   Frist laeuft schon). Laufen sie noch, bleibt der Wert ehrlich leer.
fn sprit_flug_abschliessen(stats: &mut FlightStats, fuel_kg: Option<f32>, triebwerke_aus: bool) {
    if !triebwerke_aus {
        return;
    }
    if let Some(f) = fuel_kg {
        sprit_triebwerke_aus_rasten(stats, f);
    }
}

/// Der geplante Trip laut Navlog: Sprit am letzten Fix.
fn sprit_navlog_trip(fixes: &[api_client::RouteFix]) -> Option<f32> {
    fixes
        .last()
        .and_then(|f| f.sprit_bis_hier_kg)
        .filter(|v| v.is_finite() && *v > 0.0)
}

/// Ein zweites Abheben — Zwischenlandung oder Touch-and-Go — beginnt einen
/// neuen Abschnitt. Die Boden-Marken des ersten gehoeren nicht zu ihm.
///
/// Ohne das rechnete „Rollen vor Start" den Tankstand beim ERSTEN Anlassen
/// gegen den Abhebe-Tankstand des ZWEITEN Starts — also den ganzen ersten
/// Abschnitt als Rollsprit (QS-Befund V2, 18.09.2026). Lieber kein Wert als
/// dieser.
///
/// Erkannt am schon gesetzten Abhebe-Tankstand: Beim ersten Abheben ist er
/// noch leer, und die Start-Marke bleibt stehen.
fn sprit_neuer_abschnitt_beim_abheben(stats: &mut FlightStats) {
    if stats.takeoff_fuel_kg.is_none() {
        return;
    }
    stats.engine_start_fuel_kg = None;
    stats.engine_off_fuel_kg = None;
    stats.sprit_triebwerke_aus_seit = None;
}

/// Die Route hat sich geaendert — der zwischengespeicherte Anker muss neu
/// gelesen werden. Nach dem Messpunkt bleibt alles stehen: Dessen Werte sind
/// eingefroren und gehoeren zur Route, auf der gemessen wurde.
fn sprit_plan_neu_lesen(stats: &mut FlightStats) {
    // Sprit je Wegpunkt: Der gemerkte Abschnitt ist ein Index in die ALTE
    // Route. Stehen lassen hiesse, nach einer kürzeren Route nichts mehr
    // zu erfassen, bis der alte Index eingeholt ist, oder nach einer
    // längeren alles dazwischen als übersprungen zu buchen. Der nächste
    // Takt setzt den Anker neu, ohne nachzutragen; die Messungen bleiben
    // und finden ihren Wegpunkt über die Kennung.
    stats.sprit_wp_segment = None;
    if stats.sprit_vergleichspunkt_kg.is_some() {
        return;
    }
    stats.sprit_plan_rest_nm = None;
    stats.sprit_plan_bis_tod_kg = None;
}

// Wie lange die Triebwerke durchgehend aus sein muessen, bis
// „Triebwerke aus" rastet. Laenger als jedes Zappeln der Verbrennungs-
// Anzeige, kuerzer als jedes echte Abstellen am Stand.
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_triebwerke_aus_s (v2.1.24: 10.0)

/// Eine Messung „Sprit beim Überflug" — Rohdaten, gerechnet wird erst in
/// `sprit_wegpunkt_zeilen`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct SpritWpMessung {
    /// Index im Navlog (`planned_waypoints`) zum Zeitpunkt der Messung.
    idx: usize,
    /// Kennung — damit die Messung nach einer Routenänderung wieder ihren
    /// Wegpunkt findet, auch wenn sich der Index verschoben hat.
    ident: String,
    zeit_ms: i64,
    /// Tankstand; `None` bei einem übersprungenen Wegpunkt.
    ist_kg: Option<f32>,
    uebersprungen: bool,
}

// Wie nah das Flugzeug beim Abschnittswechsel am Wegpunkt sein muss, damit
// der Tankstand als „beim Überflug gemessen" gilt. Fünf Meilen: Beim
// Überfliegen einer Kurve (fly-by) schneidet das Flugzeug die Ecke, und
// der Wechsel fällt auf die Winkelhalbierende, nicht auf den Fix.
// Wert seit v3: bundle/grenzwerte/sprit.json -> sprit_wp_nah_nm (v2.1.24: 5.0)

/// Sprit je Wegpunkt: erkennt den Überflug und hält den Tankstand fest.
///
/// Grundlage ist dieselbe Projektion auf die Plan-Route wie für den
/// Vergleichspunkt (`sprit_plan_am_ort`) — nicht die Luftlinie der Karte,
/// die springt. Wechselt der Abschnitt nach vorn, gilt der Start-Fix des
/// neuen Abschnitts als überflogen; liegen dazwischen weitere Fixe, wurden
/// sie übersprungen (Direct, Radarführung) und bekommen keinen Messwert.
///
/// Dieselben Tore wie `sprit_tick`: nur in der Luft, zwischen Abheben und
/// Landung, nie bei Pause, Slew oder Replay-Verdacht.
#[allow(clippy::too_many_arguments)]
fn sprit_wegpunkt_tick(
    stats: &mut FlightStats,
    lat: f64,
    lon: f64,
    fuel_kg: f32,
    paused: bool,
    slew: bool,
    replay_verdacht: bool,
    on_ground: bool,
    jetzt_ms: i64,
) {
    if stats.takeoff_fuel_kg.is_none() || stats.landing_fuel_kg.is_some() {
        return;
    }
    if paused || slew || replay_verdacht || on_ground || !fuel_kg.is_finite() {
        return;
    }
    if stats.planned_waypoints.len() < 2 {
        return;
    }
    let geflogen = Some(stats.distance_nm).filter(|d| d.is_finite() && *d > 0.0);
    let Some(ort) = sprit_plan_am_ort(&stats.planned_waypoints, lat, lon, geflogen) else {
        // Neben der Route: keine Planzeit — lieber keine Schaetzung als eine
        // veraltete.
        stats.landung_erwartet = None;
        stats.tod_erwartet = None;
        return;
    };
    stats.landung_erwartet = landung_erwartet(&stats.planned_waypoints, &ort, jetzt_ms);
    stats.tod_erwartet = tod_erwartet(&stats.planned_waypoints, &ort, jetzt_ms);
    let neu = ort.segment;
    match stats.sprit_wp_segment {
        // Erster Tick in der Luft: Was davor lag, wurde nicht gemessen — es
        // wird nichts nachgetragen (Einstieg in der Luft, Neustart ohne
        // Stand). Der Abflug selbst bekommt den Abhebe-Tankstand.
        None => stats.sprit_wp_segment = Some(neu),
        Some(alt) if neu > alt => {
            for k in (alt + 1)..=neu {
                let f = &stats.planned_waypoints[k];
                let nah = ::geo::distance_m(lat, lon, f.lat, f.lon) / 1852.0 <= crate::grenzwerte::grenzwerte().sprit.sprit_wp_nah_nm.wert;
                let gemessen = k == neu && nah;
                stats.sprit_wp_messungen.push(SpritWpMessung {
                    idx: k,
                    ident: f.ident.clone(),
                    zeit_ms: jetzt_ms,
                    ist_kg: gemessen.then_some(fuel_kg),
                    uebersprungen: !gemessen,
                });
            }
            stats.sprit_wp_segment = Some(neu);
        }
        _ => {}
    }
}

/// Die Tabelle „Sprit · Wegpunkt für Wegpunkt" — EINE Rechnung für das
/// Cockpit (live) und die Landeauswertung (eingefroren mit der
/// Sprit-Auswertung). Leer, wenn das Navlog keine An-Bord-Werte trägt.
fn sprit_wegpunkt_zeilen(
    stats: &FlightStats,
    // Die Kennung des Abflughafens fuer die eigene Abflugzeile. `None`
    // schreibt „Abflug" — lieber ein schlichtes Wort als eine geratene
    // Kennung.
    abflug_ident: Option<&str>,
    // Ausweichflug: gelandet wurde woanders — der Landesprit gehört nicht
    // in die Zeile des geplanten Ziels.
    ausweichflug: bool,
) -> Vec<landing_scoring::sprit::Wegpunkt> {
    use landing_scoring::sprit::{Wegpunkt, WegpunktZustand};
    let fixes = &stats.planned_waypoints;
    if fixes.len() < 2 || fixes.iter().all(|f| f.sprit_an_bord_kg.is_none()) {
        return Vec::new();
    }
    let mut zeilen: Vec<Wegpunkt> = fixes
        .iter()
        .map(|f| Wegpunkt {
            ident: f.ident.clone(),
            hoehe_ft: f.hoehe_ft,
            plan_an_bord_kg: f.sprit_an_bord_kg,
            min_an_bord_kg: f.sprit_min_an_bord_kg,
            ..Default::default()
        })
        .collect();
    // Der Abflug bekommt eine EIGENE Zeile.
    //
    // # Warum nicht einfach die erste Zeile des Navlogs
    //
    // SimBrief beginnt das Navlog NICHT am Abflughafen, sondern am ersten
    // Streckenpunkt (A380 nach EDDM, 20.09.2026: „BETTE", laut FMS um
    // 11:56 überflogen, während das Abheben um 11:55 war; bei DLH 369 war
    // es „DER24"). Bis hierher trug diese Zeile trotzdem die Abhebezeit
    // und den Abhebe-Tankstand — verglichen wurde also der Tankstand an
    // EINEM Ort mit dem Plan an einem ANDEREN, und die Differenz enthielt
    // den Verbrauch dazwischen. Bei „3.865 kg mehr getankt" ist das
    // wenig, bei einem Navlog, das erst am Steigflugende beginnt, viel.
    //
    // Die Abflugzeile steht deshalb für sich: Plan an Bord beim Abheben
    // ist Block minus Taxi, Ist ist der gemessene Tankstand. Beide gehören
    // zum selben Ort, und der erste Streckenpunkt wird wieder als das
    // behandelt, was er ist — ein Überflug.
    //
    // Fehlt eine der beiden Plan-Zahlen, gibt es keine Abflugzeile: Ein
    // erfundener Planwert wäre schlimmer als eine fehlende Zeile.
    let erster_ist_der_flughafen = fixes
        .first()
        .is_some_and(|f| f.kind.eq_ignore_ascii_case("apt"));
    if !stats.sprit_einstieg_in_der_luft && !erster_ist_der_flughafen {
        if let (Some(block), Some(ist)) = (
            stats
                .planned_block_fuel_kg
                .zip(stats.planned_taxi_kg)
                .map(|(b, t)| b - t)
                .filter(|v| v.is_finite() && *v > 0.0),
            stats.takeoff_fuel_kg,
        ) {
            zeilen.insert(
                0,
                Wegpunkt {
                    ident: abflug_ident.unwrap_or("Abflug").to_string(),
                    plan_an_bord_kg: Some(block),
                    ist_an_bord_kg: Some(ist),
                    zustand: WegpunktZustand::Gemessen,
                    zeit_ms: stats.takeoff_at.map(|t| t.timestamp_millis()),
                    ..Default::default()
                },
            );
        }
    } else if let (false, Some(z), Some(t)) = (
        stats.sprit_einstieg_in_der_luft,
        zeilen.first_mut(),
        stats.takeoff_fuel_kg,
    ) {
        // Der Navlog beginnt am Flughafen — dann ist die erste Zeile der
        // Abflug, und Plan und Ist gehören zum selben Ort.
        z.zustand = WegpunktZustand::Gemessen;
        z.ist_an_bord_kg = Some(t);
        z.zeit_ms = stats.takeoff_at.map(|t| t.timestamp_millis());
    }
    // Überflüge: über den Index, und wenn der nach einer Routenänderung
    // nicht mehr zur Kennung passt, über die Kennung (der nächstgelegene
    // Treffer, damit ein Fix, der zweimal vorkommt, nicht vertauscht wird).
    let n = zeilen.len();
    // Die Messungen tragen Indizes in `planned_waypoints`. Steht vorn eine
    // eigene Abflugzeile, ist die Tabelle um eins laenger — ohne diesen
    // Versatz trifft der schnelle Weg systematisch daneben, und nur der
    // Rueckfall ueber die Kennung rettet die Zuordnung. Bei einer Kennung,
    // die zweimal vorkommt (SID/STAR-Ueberschneidung, Warteschleife),
    // waehlt er dann sogar die falsche Zeile (Abnahme 20.09.2026).
    let versatz = n.saturating_sub(fixes.len());
    for m in &stats.sprit_wp_messungen {
        let idx = m.idx + versatz;
        let ziel = if idx < n && zeilen[idx].ident == m.ident {
            Some(idx)
        } else {
            (0..n)
                .filter(|&i| zeilen[i].ident == m.ident)
                .min_by_key(|&i| i.abs_diff(idx))
        };
        let Some(i) = ziel else { continue };
        if i == 0 || i + 1 == n {
            continue;
        }
        zeilen[i].zeit_ms = Some(m.zeit_ms);
        if m.uebersprungen {
            zeilen[i].zustand = WegpunktZustand::Uebersprungen;
        } else {
            zeilen[i].zustand = WegpunktZustand::Gemessen;
            zeilen[i].ist_an_bord_kg = m.ist_kg;
        }
    }
    // Ziel: der Tankstand beim Aufsetzen.
    if let (false, Some(z), Some(l)) = (ausweichflug, zeilen.last_mut(), stats.landing_fuel_kg) {
        z.zustand = WegpunktZustand::Gemessen;
        z.ist_an_bord_kg = Some(l);
        z.zeit_ms = stats.landing_at.map(|t| t.timestamp_millis());
    }
    landing_scoring::sprit::wegpunkte_auswerten(&mut zeilen, stats.planned_contingency_kg);
    zeilen
}

/// Ein Tick in der Luft: Vergleichspunkt setzen, danach Zeit unter der
/// Schwelle zaehlen. Nur zwischen Abheben und Aufsetzen, nie bei Pause,
/// nie am Boden, nie ueber eine Neustart-Luecke hinweg.
#[allow(clippy::too_many_arguments)]
fn sprit_tick(
    stats: &mut FlightStats,
    lat: f64,
    lon: f64,
    alt_msl_ft: f64,
    fuel_kg: f32,
    paused: bool,
    slew: bool,
    replay_verdacht: bool,
    on_ground: bool,
    dt_s: Option<f64>,
) {
    if stats.takeoff_fuel_kg.is_none() || stats.landing_fuel_kg.is_some() {
        return;
    }
    // Dasselbe Tor, das alle anderen Auswerter respektieren: bei Pause, Slew
    // oder Replay-Verdacht wird nichts gemessen. Ohne das koennte der
    // Vergleichspunkt an einer Sprungposition gesetzt werden und die Zeit
    // unter der Schwelle beim Slewen mitlaufen. Die Anflugstrecke kommt aus
    // `distance_nm`, das gegen Teleports geschuetzt ist — der Ausloeser des
    // Vergleichspunkts muss es ebenso sein.
    if paused || slew || replay_verdacht || on_ground {
        return;
    }
    // Hoechste erreichte Hoehe mitfuehren — Rueckfall fuer den
    // Sinkflug-Test, wenn das OFP keine Reiseflughoehe nennt.
    if alt_msl_ft.is_finite()
        && stats
            .sprit_hoechste_hoehe_ft
            .map_or(true, |h| alt_msl_ft > h)
    {
        stats.sprit_hoechste_hoehe_ft = Some(alt_msl_ft);
    }
    if stats.sprit_vergleichspunkt_kg.is_none() {
        if stats.sprit_plan_rest_nm.is_none() {
            let (rest, bis) = sprit_plan_aus_navlog(&stats.planned_waypoints);
            stats.sprit_plan_rest_nm = rest;
            stats.sprit_plan_bis_tod_kg = bis;
        }
        let Some(plan_rest_gesamt) = stats.sprit_plan_rest_nm else {
            return;
        };
        // Der Sinkflug muss tatsaechlich begonnen haben.
        //
        // Die TATSAECHLICH erreichte Hoehe zaehlt, nicht die geplante: Wer
        // FL360 statt geplanter FL380 zugewiesen bekommt, waere sonst vom
        // Steigflugende an „im Sinkflug". Der Plan dient nur als Deckel.
        let reise = match (stats.sprit_hoechste_hoehe_ft, stats.planned_cruise_alt_ft) {
            (Some(ist), Some(plan)) => ist.min(plan),
            (Some(ist), None) => ist,
            (None, Some(plan)) => plan,
            (None, None) => f64::MAX,
        };
        let sinkt = alt_msl_ft <= reise - crate::grenzwerte::grenzwerte().sprit.sprit_sinkflug_unter_reisehoehe_ft.wert;
        if !sinkt {
            return;
        }
        // Wo auf dem Plan stehen wir? Das ist die zweite Bedingung — und
        // seit v1.7.36 in PLAN-Koordinaten.
        //
        // # Warum der Anker bleiben MUSS
        //
        // `sinkt` allein genuegt nicht: Jede Zwischenabsenkung im Reiseflug
        // erfuellt es — Verkehr, Turbulenz, ein Step-down von FL400 auf
        // FL380. Der Punkt schnappte dann dort zu, wo es das erste Mal
        // 1 500 ft runtergeht. Die Prozentwerte blieben sogar richtig (dafuer
        // sorgt die Projektion), aber die Phasen hiessen falsch: „Anflug"
        // enthielte zwei Stunden Reiseflug und `strecke_anflug_nm` staende
        // bei 800 statt 182 NM.
        //
        // # Warum er jetzt trifft
        //
        // Bis v1.7.35 hiess der Anker „Luftlinie zum Ziel ≤ geplante
        // Reststrecke" — Luftlinie gegen Streckenmass, und eine Route ist im
        // Median 44 % laenger. Deshalb brauchte es das 80-%-Fenster und die
        // Scharfschaltung als Kruecken, und deshalb verlor ein Drittel des
        // Bestands seine Phasen. Jetzt steht auf beiden Seiten Strecke ueber
        // Grund: die Plan-Reststrecke am Lotfusspunkt gegen die geplante
        // Reststrecke ab TOD. Kein Fenster, keine Scharfschaltung.
        // Die geflogene Strecke, nicht die Uhr — siehe `sprit_plan_am_ort`.
        let geflogen = Some(stats.distance_nm).filter(|d| d.is_finite() && *d > 0.0);
        let Some(ort) = sprit_plan_am_ort(&stats.planned_waypoints, lat, lon, geflogen) else {
            return;
        };
        if ort.plan_rest_nm > plan_rest_gesamt {
            // Noch vor dem geplanten Sinkflugbeginn — der Sinkflug hier ist
            // eine Zwischenabsenkung, kein Anflug.
            return;
        }
        stats.sprit_vergleichspunkt_kg = Some(fuel_kg);
        stats.sprit_vergleichspunkt_odo_nm = Some(stats.distance_nm);
        // Plan-Anteil und Plan-Reststrecke gehoeren zu GENAU diesem Ort —
        // nicht mehr zum TOD-Fix. Damit vergleichen `strecke_anflug_nm` und
        // `plan_strecke_anflug_nm` endlich denselben Startpunkt.
        stats.sprit_plan_bis_vergleichspunkt_kg = Some(ort.plan_bis_kg);
        stats.sprit_plan_rest_ab_vergleichspunkt_nm = Some(ort.plan_rest_nm);
        // Der Trip aus DEMSELBEN Navlog, im selben Augenblick — siehe das Feld.
        stats.sprit_plan_trip_am_vergleichspunkt_kg = sprit_navlog_trip(&stats.planned_waypoints);
        // Einstieg in der Luft: der Plan am Einstiegsort, ebenfalls jetzt.
        if let Some((ein_lat, ein_lon)) = stats.sprit_einstieg_ort {
            stats.sprit_plan_bis_einstieg_kg =
                sprit_plan_am_ort(&stats.planned_waypoints, ein_lat, ein_lon, None)
                    .map(|o| o.plan_bis_kg);
        }
        stats.sprit_schwelle_ft = sprit_schwelle_ft(stats);
        return;
    }
    let Some(dt) = dt_s.filter(|d| *d > 0.0 && *d <= crate::grenzwerte::grenzwerte().sprit.sprit_tick_max_s.wert) else {
        return;
    };
    if stats.sprit_schwelle_ft.is_none() {
        stats.sprit_schwelle_ft = sprit_schwelle_ft(stats);
    }
    let Some(schwelle) = stats.sprit_schwelle_ft else {
        return;
    };
    if alt_msl_ft < schwelle {
        stats.sprit_zeit_unter_schwelle_s += dt;
    }
}

/// Ist der Tankverlauf glaubwuerdig? Sonst gibt es „nicht pruefbar" statt
/// einer falschen Reserve-Warnung.
fn sprit_tank_plausibel(stats: &FlightStats) -> bool {
    let (Some(to), Some(ldg)) = (stats.takeoff_fuel_kg, stats.landing_fuel_kg) else {
        return false;
    };
    if !(to.is_finite() && ldg.is_finite()) || to <= 0.0 || ldg < 0.0 || ldg > to {
        return false;
    }
    if ldg < to * crate::grenzwerte::grenzwerte().sprit.sprit_tank_min_anteil.wert {
        return false;
    }
    match stats.sprit_vergleichspunkt_kg {
        Some(vp) => vp <= to && vp >= ldg,
        None => true,
    }
}

/// Nach einem Durchstart gehoert der Sprit der ABGEBROCHENEN Landung weg.
///
/// Ein Touch-and-Go hat `landing_fuel_kg` gesetzt und die Auswertung
/// eingefroren. Bliebe das stehen, wuerde `sprit_tick` fuer den echten
/// Anflug gar nicht mehr messen (es steigt bei gesetztem `landing_fuel_kg`
/// aus), und beim echten Aufsetzen stuende die Auswertung des Durchstarts
/// im PIREP — samt dessen Reserve-Quote. Ein echtes Reserve-Unterschreiten
/// waere als „intakt" erschienen.
///
/// Bewusst NICHT in `landung_episode_zuruecksetzen`: die raeumt auch bei
/// einem blossen AGL-Ausreisser im Ausrollen auf, und dann waeren Landesprit
/// und Trip-Burn des PIREP weg, ohne dass ein neuer Stempel folgt.
///
/// Der Vergleichspunkt bleibt stehen: er wurde auf dem Weg zum Ziel gesetzt
/// und gilt fuer den zweiten Anflug weiter.
fn sprit_durchstart_zuruecksetzen(stats: &mut FlightStats) {
    stats.landing_fuel_kg = None;
    // Mit dem Wert geht seine Herkunft: Sonst gaelte fuer die ECHTE Landung
    // noch der Puffer-Vorrang der abgebrochenen, und ein Stempel ohne
    // Puffer wuerde stillschweigend ihren Wert behalten.
    stats.landing_fuel_puffer = false;
    stats.sprit_landung_odo_nm = None;
    stats.sprit_auswertung = None;
}

#[cfg(test)]
impl FlightStats {
    /// Nur für Tests: den Plan-Verbrauch bis TOD setzen, ohne ein ganzes
    /// Navlog bauen zu müssen.
    fn plan_bis_setzen(&mut self, kg: f32) {
        self.sprit_plan_bis_tod_kg = Some(kg);
        self.sprit_plan_rest_nm = Some(139.0);
    }
}

/// Ist der Flieger woanders aufgesetzt als geplant?
///
/// v1.7.35: Die Frage wird BEIM AUFSETZEN aus der Landeposition beantwortet
/// (`arrival::locate`), nicht erst beim Einreichen. Nur so steht die Fassung
/// fest, bevor der erste Leser sie sieht — Touchdown-Payload, PIREP-Felder,
/// LandingRecord und PIREP-Payload werden zu verschiedenen Zeitpunkten
/// gebaut. Entschiede man erst beim Einreichen, saehen sie drei verschiedene
/// Staende, und genau das soll der Umbau verhindern.
///
/// Der spaeter vom Piloten bestaetigte Divert ist nur die Formalisierung
/// desselben Sachverhalts; die Position weiss es vorher.
fn sprit_ist_ausweichflug(stats: &FlightStats) -> bool {
    let (Some(la), Some(lo), Some((rlat, rlon))) = (
        stats.landing_lat,
        stats.landing_lon,
        stats.planned_arr_ref_pos,
    ) else {
        // Ohne Referenzposition laesst sich nichts sagen — dann lieber die
        // normale Auswertung als eine erfundene Ausweich-Erkennung.
        return false;
    };
    if (rlat == 0.0 && rlon == 0.0) || !rlat.is_finite() || !rlon.is_finite() {
        return false;
    }
    // Derselbe Radius, mit dem `arrival::locate` ueber die Referenzposition
    // entscheidet — eine Quelle fuer die Frage „ist das noch der geplante
    // Platz?", nicht zwei.
    runway::distance_m(la, lo, rlat, rlon) / 1852.0 > crate::grenzwerte::grenzwerte().ankunft.on_field_fallback_radius_nm.wert
}

/// Die Auswertung holen — **einmal rechnen, dann einfrieren**.
///
/// Die Ausweich-Erkennung laeuft beim allerersten Aufruf ueber die
/// Landeposition; danach wird der eingefrorene Wert zurueckgegeben, ganz
/// gleich wer fragt.
/// Die Auswertung holen — **einmal rechnen, dann einfrieren**.
///
/// Der Touchdown-Payload entsteht im Positions-Streamer, die Messung im
/// Touchdown-Sampler — zwei Threads, deren Reihenfolge nicht feststeht.
/// Wer zuerst kommt, rechnet; alle weiteren bekommen das eingefrorene
/// Ergebnis. Damit zeigen Client, Bericht und Live-Uebersicht nie
/// unterschiedliche Zahlen (Muster wie beim Landeurteil, 13.07.2026).
///
/// **Die eine Ausnahme**, und warum sie nach aussen unsichtbar bleibt:
/// `stamp_touchdown_metadata` kann zweimal laufen — aus dem Sampler und
/// danach aus der FSM. Traegt der zweite Stempel einen ANDEREN Landesprit,
/// verwirft er die Auswertung, damit sie nicht den Wert des ersten Ticks
/// traegt, waehrend die Kachel „Landesprit" den zweiten zeigt. Es gilt also
/// genauer: einmal je Stempel, der letzte gilt. Beide Stempel gehoeren zu
/// DERSELBEN Landung und liegen beide VOR jedem Versand — der Payload haengt
/// an `announce_landing_score`, das erst eine fertige Note braucht. Kein
/// Leser sieht darum je einen revidierten Stand.
///
/// Ohne Landesprit gibt es nichts zu rechnen — dann bleibt es `None`,
/// und die Anzeigen lassen die Sektion weg.
fn sprit_auswertung_einmal(
    stats: &mut FlightStats,
) -> Option<landing_scoring::sprit::SpritAuswertung> {
    if stats.sprit_auswertung.is_none() && stats.landing_fuel_kg.is_some() {
        let ausweich = sprit_ist_ausweichflug(stats);
        let mut a = sprit_auswertung_aus(stats, ausweich);
        if ausweich {
            // `arr_airport_elevation_ft` wird nur einmal gesetzt und bleibt
            // nach einem Ausweichflug die Hoehe des GEPLANTEN Platzes. Die
            // Schwelle und die Zeit darunter gehoerten damit zu einem
            // Flughafen, an dem nicht gelandet wurde.
            a.schwelle_ft = None;
            a.zeit_unter_schwelle_min = None;
        }
        // v1.7.40: Die Tabelle je Wegpunkt friert mit der Auswertung ein —
        // so tragen Landungsdatensatz und Nutzlast dieselben Zeilen, die das
        // Cockpit zuletzt zeigte.
        let abflug = stats.abflug_icao.clone();
        a.wegpunkte = sprit_wegpunkt_zeilen(stats, abflug.as_deref(), ausweich);
        stats.sprit_auswertung = Some(a);
    }
    stats.sprit_auswertung.clone()
}

/// Die eine Rechnung — siehe `sprit_auswertung_einmal`, die sie einfriert.
fn sprit_auswertung_aus(
    stats: &FlightStats,
    ausweichflug: bool,
) -> landing_scoring::sprit::SpritAuswertung {
    // Bis zum Aufsetzen, nicht bis jetzt: Der Wegzaehler laeuft am Boden
    // weiter. Ohne Aufsetz-Marke (Altbestand) bleibt es beim alten Stand.
    let ende_nm = stats.sprit_landung_odo_nm.unwrap_or(stats.distance_nm);
    let anflug_nm = match stats.sprit_vergleichspunkt_odo_nm {
        Some(odo) if ende_nm > odo => Some((ende_nm - odo) as f32),
        _ => None,
    };
    landing_scoring::sprit::auswerten(&landing_scoring::sprit::SpritEingang {
        planned_burn_kg: stats.planned_burn_kg,
        // v1.7.36: Der Plan AM GEMESSENEN ORT, nicht am TOD-Fix.
        //
        // Der Rueckfall auf den TOD-Fix gilt nur fuer Altbestand: Fluege,
        // die vor v1.7.36 gemessen und persistiert wurden, tragen die neuen
        // Felder nicht. Fuer sie bleibt die Rechnung, die sie schon hatten —
        // nicht rueckwirkend eine andere Zahl fuer denselben Flug.
        plan_bis_vergleichspunkt_kg: stats
            .sprit_plan_bis_vergleichspunkt_kg
            .or(stats.sprit_plan_bis_tod_kg),
        plan_rest_nm: stats
            .sprit_plan_rest_ab_vergleichspunkt_nm
            .or(stats.sprit_plan_rest_nm)
            .map(|v| v as f32),
        takeoff_fuel_kg: stats.takeoff_fuel_kg,
        sprit_bei_vergleichspunkt_kg: stats.sprit_vergleichspunkt_kg,
        landing_fuel_kg: stats.landing_fuel_kg,
        strecke_ab_vergleichspunkt_nm: anflug_nm,
        zeit_unter_schwelle_s: Some(stats.sprit_zeit_unter_schwelle_s as f32).filter(|v| *v > 0.0),
        schwelle_ft: stats.sprit_schwelle_ft.map(|v| v as f32),
        planned_taxi_kg: stats.planned_taxi_kg,
        planned_contingency_kg: stats.planned_contingency_kg,
        planned_alternate_kg: stats.planned_alternate_burn_kg,
        planned_reserve_kg: stats.planned_reserve_kg,
        planned_extra_kg: stats.planned_extra_kg,
        planned_block_fuel_kg: stats.planned_block_fuel_kg,
        tank_plausibel: sprit_tank_plausibel(stats),
        engine_start_fuel_kg: stats.engine_start_fuel_kg,
        engine_off_fuel_kg: stats.engine_off_fuel_kg,
        // Der Trip aus DEMSELBEN Navlog wie der Plan-Anteil am Messpunkt —
        // am Messpunkt eingefroren. Der Rueckfall auf das aktuelle Navlog
        // gilt nur ohne Messpunkt (dann gibt es keinen Anteil, gegen den er
        // driften koennte) und fuer Altbestand.
        plan_trip_navlog_kg: stats
            .sprit_plan_trip_am_vergleichspunkt_kg
            .or_else(|| sprit_navlog_trip(&stats.planned_waypoints)),
        einstieg_in_der_luft: stats.sprit_einstieg_in_der_luft,
        plan_bis_einstieg_kg: stats.sprit_plan_bis_einstieg_kg,
        ausweichflug,
    })
}

/// PIREP-Felder fuer die Webseite — reine Anzeige der fertigen Auswertung.
fn sprit_pirep_felder(
    a: Option<&landing_scoring::sprit::SpritAuswertung>,
    f: &mut HashMap<String, String>,
) {
    use landing_scoring::sprit::Reserve;
    let Some(a) = a else { return };
    // Diese drei Helfer sind die Gegenstuecke zu `kg()` und `pct()` in
    // client/src/lib/sprit.ts. Dieselbe Zahl darf nicht viererlei aussehen —
    // die Webseite ist die vierte Flaeche neben Client, Bericht und
    // Live-Uebersicht.
    //
    // Ziffern mit schmalem Leerzeichen gruppiert, ohne Vorzeichen — das
    // Gegenstueck zu `kg()` in client/src/lib/sprit.ts.
    let ziffern = |v: f32| -> String {
        let s = (v.abs().round() as i64).to_string();
        let mut aus = String::new();
        for (i, c) in s.chars().enumerate() {
            if i > 0 && (s.len() - i) % 3 == 0 {
                aus.push('\u{202f}');
            }
            aus.push(c);
        }
        aus
    };
    // Mit Vorzeichen, Minus als echtes U+2212 — nicht als ASCII-Bindestrich.
    // Kein Vorzeichen bei null — wie `diffKg()` in der Anzeige. Bis v1.7.36
    // stand hier „+0 kg", dort „0 kg".
    let mit_trenner = |v: f32| -> String {
        let vz = if v.round() == 0.0 {
            ""
        } else if v < 0.0 {
            "\u{2212}"
        } else {
            "+"
        };
        format!("{vz}{}", ziffern(v))
    };
    // „+192,8 %" / „−3,4 %" — Komma, nicht Punkt. Ohne das stand hier als
    // einziger der vier Oberflaechen „+192.8 %".
    let prozent = |v: f32| -> String {
        let z = format!("{:.1}", v.abs()).replace('.', ",");
        let vz = if v > 0.0 {
            "+"
        } else if v < 0.0 {
            "\u{2212}"
        } else {
            ""
        };
        format!("{vz}{z} %")
    };
    // v1.7.36: Die Entscheidung „Kilogramm statt Prozent" trifft die
    // RECHNUNG (`Phase::als_kg`), nicht diese Anzeige.
    //
    // Bis dahin stand dieselbe Regel an vier Stellen — hier, im Client, in
    // der Live-Uebersicht und im Bericht. Wer eine aendert, laesst die
    // anderen lautlos zurueck, und der Pilot liest im Client „−12 kg" und
    // auf der GSG-Webseite „−8,3 %".
    let hauptzahl = |p: &landing_scoring::sprit::Phase| -> String {
        if p.als_kg {
            format!("{} kg", mit_trenner(p.ist_kg - p.plan_kg))
        } else {
            prozent(p.abweichung_pct)
        }
    };
    if let Some(p) = &a.bis_sinkflug {
        f.insert(
            "Sprit bis Sinkflug".into(),
            format!(
                "{} ({} kg / Plan {} kg)",
                hauptzahl(p),
                ziffern(p.ist_kg),
                ziffern(p.plan_kg)
            ),
        );
    }
    if let Some(p) = &a.anflug {
        f.insert(
            "Sprit Anflug".into(),
            format!(
                "{} ({} kg / Plan {} kg)",
                hauptzahl(p),
                ziffern(p.ist_kg),
                ziffern(p.plan_kg)
            ),
        );
    }
    // v1.7.36: die Anflugstrecke und der Rollsprit — bis dahin fehlten sie
    // auf der GSG-Webseite, waehrend Client und Live-Uebersicht sie zeigten.
    if let (Some(ist), Some(plan)) = (a.strecke_anflug_nm, a.plan_strecke_anflug_nm) {
        f.insert(
            "Anflugstrecke".into(),
            format!("{} NM / Plan {} NM", ziffern(ist), ziffern(plan)),
        );
    }
    if let Some(p) = &a.rollen_vor_start {
        f.insert(
            "Sprit Rollen vor Start".into(),
            format!("{} kg / Plan {} kg", ziffern(p.ist_kg), ziffern(p.plan_kg)),
        );
    }
    if let Some(kg) = a.rollen_nach_landung_kg {
        f.insert(
            "Sprit Rollen nach Landung".into(),
            format!("{} kg", ziffern(kg)),
        );
    }
    if let (Some(min), Some(ft)) = (a.zeit_unter_schwelle_min, a.schwelle_ft) {
        f.insert(
            "Zeit unter Schwelle".into(),
            // v1.7.38: ganze Minuten — „14,3 min" las niemand als 14 min 18 s.
            // Dieselbe Regel wie `minuten()` in client/src/lib/sprit.ts:
            // gemessene Kurzzeit unter einer halben Minute ist „< 1", nie „0".
            format!(
                "{} min unter {} ft",
                if min > 0.0 && min < 0.5 {
                    "< 1".to_string()
                } else {
                    (min.round() as i64).to_string()
                },
                ziffern(ft)
            ),
        );
    }
    // v1.7.37: der Abstand in kg statt der Quote — „(338 %)" las sich, als
    // haette die Reserve 338 %. Die Quote bleibt nur fuer Altbestand ohne
    // `reserve_abstand_kg`.
    //
    // Die Rechnung steht ausdruecklich da — „1 744 kg ueber der Final Reserve
    // (732 kg)" liess offen, ob die 732 noch abzuziehen sind (Thomas, BIT348).
    // Ueber `reserve_abstand`: auch eine vor v1.7.37 eingefrorene Auswertung
    // (Update zwischen Landung und Einreichen) bekommt den Abstand in kg —
    // sonst zeigte das PIREP-Feld die Quote und der Client die kg (QS v1.7.37).
    match (
        &a.reserve,
        landing_scoring::sprit::reserve_abstand(a),
        a.reserve_kg,
        a.landing_fuel_kg,
    ) {
        (Reserve::Intakt { .. }, Some(ab), Some(res), Some(ldg)) => f.insert(
            "Final Reserve".into(),
            format!(
                "intakt — {} kg gelandet = {} kg Final Reserve + {} kg darüber",
                ziffern(ldg),
                ziffern(res),
                ziffern(ab.abs())
            ),
        ),
        (Reserve::Unterschritten { .. }, Some(ab), Some(res), Some(ldg)) => f.insert(
            "Final Reserve".into(),
            format!(
                "UNTERSCHRITTEN — {} kg gelandet = {} kg Final Reserve − {} kg",
                ziffern(ldg),
                ziffern(res),
                ziffern(ab.abs())
            ),
        ),
        (Reserve::Intakt { quote_pct }, _, _, _) => {
            f.insert("Final Reserve".into(), format!("intakt ({quote_pct:.0} %)"))
        }
        (Reserve::Unterschritten { quote_pct }, _, _, _) => f.insert(
            "Final Reserve".into(),
            format!("UNTERSCHRITTEN ({quote_pct:.0} %)"),
        ),
        (Reserve::NichtPruefbar { grund }, _, _, _) => {
            f.insert("Final Reserve".into(), format!("nicht pruefbar ({grund})"))
        }
    };
    // v1.7.38: die genutzte Contingency in kg — „unberuehrt" stand auch da,
    // wo sie angebrochen war (#1417: 86 von 238 kg).
    if let (Some(n), Some(l)) = (
        landing_scoring::sprit::contingency_genutzt(a),
        a.leiter.as_ref(),
    ) {
        let text = if l.contingency_kg <= 0.0 {
            "keine geplant".to_string()
        } else if n <= 0.0 {
            "unberührt".to_string()
        } else if n >= l.contingency_kg {
            format!(
                "aufgebraucht — alle {} kg genutzt",
                ziffern(l.contingency_kg)
            )
        } else {
            format!(
                "angebrochen — {} von {} kg genutzt",
                ziffern(n),
                ziffern(l.contingency_kg)
            )
        };
        f.insert("Contingency".into(), text);
    }
    if let (Some(g), Some(n), Some(u)) =
        (a.extra_getankt_kg, a.extra_genutzt_kg, a.extra_ungenutzt_kg)
    {
        f.insert(
            "Extra Fuel".into(),
            // Ohne geplantes Extra keine Nullen-Zeile (v1.7.38).
            if g <= 0.0 {
                "kein Extra getankt".to_string()
            } else {
                format!(
                    "{} kg getankt · {} kg genutzt · {} kg ungenutzt",
                    ziffern(g),
                    ziffern(n),
                    ziffern(u)
                )
            },
        );
    }
}

fn actual_burn_for_record(stats: &FlightStats) -> Option<f32> {
    match (stats.takeoff_fuel_kg, stats.landing_fuel_kg) {
        (Some(toff), Some(land)) if toff > land && toff > 0.0 && land >= 0.0 => Some(toff - land),
        _ => None,
    }
}

/// v0.12.4 (Spec docs/spec/v0.12.4-score-consistency.md, LE5): OFP-Trip-
/// Burn-Abweichung in Prozent — `(trip_burn − planned) / planned × 100`,
/// mit `trip_burn = takeoff_fuel − landing_fuel`. Positiv = Mehrverbrauch.
/// `None`, wenn `planned <= 0` oder ein Input fehlt.
///
/// **Bewusst Trip-Burn, nicht Block-Fuel:** Block-Fuel enthält den Taxi-out-
/// Sprit und ergäbe einen anderen Prozent-Wert als der v0.7.1-Sub-Score
/// (`sub_scores[fuel]`). Diese Funktion hält das MQTT-`TouchdownPayload`-Feld
/// `fuel_efficiency_pct` auf derselben Basis wie `LandingRecord`.
fn trip_burn_efficiency_pct(
    takeoff_fuel_kg: Option<f32>,
    landing_fuel_kg: Option<f32>,
    planned_burn_kg: Option<f32>,
) -> Option<f32> {
    match (takeoff_fuel_kg, landing_fuel_kg, planned_burn_kg) {
        // Guard identisch zu `actual_burn_for_record` — bei Refuel /
        // Sim-Anomalie (`takeoff <= landing`) liefern wir None statt eines
        // negativen Phantom-Trip-Burns, sonst driftet das MQTT-Feld wieder
        // vom LandingRecord + sub_scores[fuel] weg.
        (Some(takeoff), Some(landing), Some(plan))
            if plan > 0.0 && takeoff > landing && takeoff > 0.0 && landing >= 0.0 =>
        {
            let trip_burn = takeoff - landing;
            Some(((trip_burn - plan) / plan) * 100.0)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests_lib/trip_burn_efficiency_tests.rs"]
mod trip_burn_efficiency_tests;
