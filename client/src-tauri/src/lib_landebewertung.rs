/// v0.10.0 (#runway-utilization-score) — Helper: populiert die für den
/// neuen `sub_rollout_v2`-Algorithmus benötigten Felder am
/// `LandingScoringInput`. Wird von allen 4 Compute-Sites (build_landing_
/// record, build PirepPayload, compute_aggregate_master_score, shadow-
/// validation) konsistent gerufen, damit Pilot-Client, PIREP-Notes,
/// Webapp + Recorder denselben Score sehen.
///
/// `airport_source` proxy: wenn `stats.runway_match` Some ist, wurde
/// der Touchdown via runway-match aufgelöst → setzt `Some("runway_match")`.
/// Bei None bleibt's None → `sub_rollout_v2` skipped mit
/// `off_airport_landing` (was korrekt ist).
///
/// Geometry-Trust + pre-displaced kommen aus dem zentralen
/// `runway_geometry_trust_check` resp. `assess_touchdown` damit die
/// Logik 1:1 mit dem ist was im PIREP-Payload landet.
fn fill_v2_rollout_fields(
    input: &mut landing_scoring::LandingScoringInput,
    stats: &FlightStats,
    arr_airport: &str,
) {
    // v1.7.0: Ziel-Markierung und Aufsetzzone kommen aus `assess_touchdown` —
    // derselben Quelle, die auch den Touchdown-Payload und die Anzeige speist.
    // Die Achse rechnet sie ausdruecklich NICHT selbst nach: die Regeln (300/400 m
    // ab 2400 m Bahnlaenge; Zone min(900 m, Drittel) ab 1200 m) leben in
    // `runway_assessment`, und dort bleiben sie.
    let bewertet = assess_touchdown(stats);
    input.aim_point_m = bewertet.aim.map(|a| a.aim_point_m);
    input.tdz_end_m = bewertet.tdz.map(|t| t.tdz_length_m);

    // v1.7.0 Bahndisziplin: seitliche Lage und Overrun aus dem Rollweg,
    // fortgeschrieben von `bahndisziplin_tick`. Der Belag kommt aus dem
    // Runway-Match (OurAirports-Angabe) — `nav_runways.surface_code` ist
    // durchgaengig leer und darf nicht verwendet werden.
    input.bahn_max_querversatz_m = stats.bahn_max_querversatz_m;
    input.bahn_overrun_m = stats.bahn_overrun_m;
    input.bahn_proben = Some(stats.bahn_proben as usize);
    // Läuft die Rollspur schräg zur Bahnachse? Dann stimmt die ACHSE
    // nicht — ein rollendes Flugzeug folgt der Mittellinie. Gemessen
    // wird nur bis zum Bewertungsende; danach ist Ausschwenken, und das
    // ist kein Achsenfehler. Siehe `ACHSE_FRAGWUERDIG_AB_GRAD`.
    //
    // ⚠ Die Reihenfolge ist der Kern der Sache.
    //
    // Hier stand `bahn_kante_laengs_m.or(bahn_raeum_laengs_m)` — die
    // KANTE zuerst. Die Kante ist die Stelle, an der das Flugzeug die Bahn
    // VERLASSEN hat; die Ausgleichsgerade lief also ueber das gesamte
    // Ausschwenken zur Ausfahrt und stand dann natuerlich schraeg. Der
    // Befund lautete „Szenerie-Versatz" — an Plaetzen mit
    // Standard-Szenerie wie EDDK und EGBB nachweislich falsch.
    //
    // Am Korpus vom 27.08.2026 (46 Landungen, 37 Plaetze): 9 von 46
    // uebersprungen, davon 4 allein durch diese Reihenfolge.
    //
    // Richtig ist das MESSFENSTER: der Teil, auf dem das Flugzeug noch der
    // Bahn folgt — genau das, was der Kommentar an `achsen_befund` schon
    // immer versprochen hat. Danach das Ausschwenken (`raeum`), und erst
    // als letzter Notnagel die Kante.
    input.bahn_achsen_abweichung_grad = None;
    input.bahn_achsen_kreuzt_mitte = None;
    input.bahn_achsen_groesster_betrag_m = None;
    if let Some(bis) = landing_scoring::sub_bahndisziplin::achsen_fenster_bis_m(
        stats.bahn_fenster_zu_laengs_m.map(|v| v as f64),
        stats.bahn_raeum_laengs_m.map(|v| v as f64),
        stats.bahn_kante_laengs_m.map(|v| v as f64),
    ) {
        let proben: Vec<(f64, f64)> = stats
            .bahn_spur
            .iter()
            .map(|(lg, qr)| (*lg as f64, *qr as f64))
            .collect();
        if let Some(befund) = landing_scoring::sub_bahndisziplin::achsen_befund(&proben, bis) {
            input.bahn_achsen_abweichung_grad = Some(befund.winkel_grad);
            input.bahn_achsen_kreuzt_mitte = Some(befund.kreuzt_mitte);
            input.bahn_achsen_groesster_betrag_m = Some(befund.groesster_betrag_m);
        }
    }
    // Die Spurweite aus der Flugzeugdatei, falls gelesen — dieselbe
    // Quelle, aus der die Anzeige ihren Randabstand rechnet. Ohne diese
    // Zeile zeigt die Grafik einen anderen Wert, als die Note benutzt:
    // Bei einem Add-on mit 2 m breiterem Fahrwerk gemessen 8,8 gegen
    // 7,7 m Randabstand.
    //
    // Codex-QS-Fund (Vereinheitlichung 09/2026, BCS3317/A306): diese Zeile
    // stand als bedingungsloses Ueberschreiben da, aus einer Zeit, in der
    // `scoring_eingang` `fahrwerk_spurweite_m` noch nie selbst setzte (immer
    // None). Seit `scoring_eingang` dort die Sim-Schreibweisen-Kaskade
    // (`muster_fuer_typtabelle`) vorbelegt, hat das blinde Ueberschreiben
    // genau diese Vorbelegung wieder auf None zurueckgesetzt, sobald
    // `stats.fahrwerk_spurweite_m` (X-Plane-Datei-Wert) fehlte — der
    // Normalfall auf MSFS. Die Bewertung fiel dadurch auf den Crate-eigenen,
    // kaskadenlosen Fallback zurueck und sah "A300" statt der aufgeloesten
    // "A306". Jetzt: Datei-Wert hat weiterhin Vorrang, aber nur wenn er
    // wirklich vorliegt — sonst bleibt die schon aufgeloeste Kaskade stehen.
    input.fahrwerk_spurweite_m = stats.fahrwerk_spurweite_m.or(input.fahrwerk_spurweite_m);
    input.runway_surface = stats.runway_match.as_ref().map(|rm| rm.surface.clone());

    let rm = stats.runway_match.as_ref();
    // v1.6.7-QS: die um die Displaced Threshold KORRIGIERTE Distanz, nicht
    // die pavement-relative. `runway_length_m - displaced` bildet unten den
    // Nenner (LDA) — mit dem Rohwert im Zaehler haetten Zaehler und Nenner
    // verschiedene Bezugspunkte, und das gesperrte Vorfeld zaehlte als
    // genutzte Bahn. Auf einer 4000-m-Bahn mit 300 m Displaced Threshold
    // waren das 8 Prozentpunkte zu viel, an der 60-%-Grenze ein ganzes Band.
    // `assess_touchdown` rechnet die Korrektur ohnehin schon fuer den
    // Payload — dieselbe Quelle nutzen statt eine zweite Wahrheit zu bauen.
    // Sichtbar wird der Unterschied erst, wenn die Navdaten ueberhaupt
    // Displaced Thresholds liefern (heute: keine einzige, siehe QS-Notiz).
    // Negative Werte (Aufsetzen VOR der Schwelle) faengt `sub_rollout_v2`
    // mit `raw_used.max(rollout)` ab — dafuer ist der Riegel da.
    let assessed = assess_touchdown(stats);
    input.td_distance_from_threshold_m = assessed.td_distance_from_threshold_m;
    input.landing_float_distance_m = stats.landing_float_distance_m;
    input.runway_length_m = rm.map(|m| (m.length_ft as f32) * 0.3048);
    // v0.19.x FIX: sourced from Navigraph-only `runway_nav_geometry`
    // before — `RunwayMatch::displaced_threshold_ft` now carries the same
    // data from the OurAirports CSV too, so the LDA-based rollout-
    // utilization correction (sub_rollout_v2) no longer silently assumes
    // full runway length for every OurAirports-fallback landing.
    // v1.6.8: fuer die NUTZBARE Laenge zaehlt der echte Versatz der
    // Lande-Schwelle — auch wenn er in der Geometrie steckt statt im
    // Zahlenfeld. Seit AIRAC 2608 liefert der DFD-Export den
    // Schwellenpunkt bereits versetzt und das Feld auf 0, `length_ft`
    // blieb aber die volle Bahn. Die Bewertung rechnete dadurch gegen zu
    // viel Bahn (EDDH 33: 446 m zu viel). `geometry_hidden_displacement_ft`
    // holt die Zahl aus der eingebetteten Tabelle und prueft sie gegen die
    // Geometrie, statt ihr zu glauben.
    //
    // WICHTIG: das aendert NUR den Nenner. Die Aufsetzdistanz kommt
    // weiter aus `assess_touchdown`, das seinerseits
    // `RunwayMatch::displaced_threshold_ft` verwendet — dort steht
    // bewusst 0, wenn die Geometrie den Versatz schon enthaelt. Wer die
    // beiden Zahlen zusammenwirft, zieht den Versatz zweimal ab und
    // erklaert korrekte Landungen zu Aufsetzern vor der Schwelle (an 25
    // Fluegen des Bestands durchgerechnet, bevor es gebaut wurde).
    input.runway_displaced_threshold_ft = rm.map(effective_displaced_threshold_ft);
    // assess_touchdown(stats).dds.in_pre_threshold_zone — identisch zu
    // dem was im TouchdownPayload + LandingRecord landet (single source).
    input.pre_displaced_threshold = assessed.dds.map(|d| d.in_pre_threshold_zone);
    let (trusted, _) = runway_geometry_trust_check(
        rm.map(|m| m.airport_ident.as_str()),
        arr_airport,
        stats
            .divert_hint
            .as_ref()
            .and_then(|h| h.actual_icao.as_deref()),
        rm.map(|m| m.centerline_distance_m as f32),
        stats.landing_float_distance_m,
    );
    input.runway_geometry_trusted = Some(trusted);
    input.airport_source = rm.map(|_| "runway_match".to_string());
    input.runway_match_icao = rm.map(|m| m.airport_ident.clone());
    input.runway_match_ident = rm.map(|m| m.runway_ident.clone());
    // ── v1.6.2 Ausrichtung ───────────────────────────────────────────
    //
    // Bewusst hier und nicht an den vier Aufrufstellen: der Helfer ist die
    // einzige Stelle, an der die Bahn-Geometrie in den Score wandert —
    // alles andere waere vier Kopien, die auseinanderlaufen koennen.
    input.runway_match_centerline_offset_m = rm.map(|m| m.centerline_distance_m as f32);
    // Breite aus dem Match (beide Match-Pfade fuellen sie), nicht aus der
    // Navigraph-Geometrie: sonst haetten OurAirports-Landungen nie eine.
    input.runway_width_m = rm.map(|m| m.width_ft * 0.3048).filter(|w| *w > 0.0);
    input.landing_heading_true_deg = stats.landing_heading_true_deg;
    // Bahnkurs aus dem MATCH, nicht aus der Navigraph-Geometrie: beide
    // Match-Pfade fuellen `heading_true_deg` (Navigraph wie OurAirports),
    // `runway_nav_geometry` dagegen nur der Navigraph-Pfad. Genau dieselbe
    // Falle wie beim Versatz-Schwellenwert 30 Zeilen weiter oben (v0.19.x):
    // sonst faellt die Achse bei jedem Divert auf einen Platz ohne gecachte
    // Navdaten stillschweigend aus — also ausgerechnet dort, wo schief
    // gelandet wird (QS-Befund v1.6.2).
    input.runway_true_course_deg = rm.map(|m| m.heading_true_deg);
    // Drehfluegler/Wasserflugzeuge richten sich nicht an einer Bahnachse aus.
    input.nicht_konventionell = stats.landing_nicht_konventionell;
    // Anflug-Stabilitaet misst ein 3°-Gleitpfad-Modell. Heli/Wasserflugzeug
    // fliegen anders; ihre σ-Werte werden schon bei der Auswertung genullt
    // (siehe `category.is_non_conventional()` beim Landen), aber die
    // gefilterten v2-Felder bleiben gesetzt und `canonical_*` bevorzugt sie.
    // Ohne diesen Schnitt deckelte das Anflug-Urteil (v1.9.16) sie auf 80/45.
    if stats.landing_nicht_konventionell {
        input.approach_vs_stddev_fpm = None;
        input.approach_bank_stddev_deg = None;
        input.anflug = Default::default();
    }
    // Seitenwind-Kompensation der Ausrichtungs-Achse: rechtweisender Wind
    // beim Aufsetzen plus Bezugsgeschwindigkeit fuer den Vorhaltewinkel.
    input.landing_wind_direction_deg = stats.landing_wind_direction_deg;
    input.landing_wind_speed_kt = stats.landing_wind_speed_kt;
    // v1.6.3-QS: **Eigengeschwindigkeit, nicht Grundgeschwindigkeit.** Der
    // Vorhaltewinkel folgt dem Winddreieck sin(Vorhalt) = Seitenwind / TAS.
    // Gelandet wird fast immer gegen den Wind, die Grundgeschwindigkeit
    // liegt also systematisch unter der Eigengeschwindigkeit — als Nenner
    // faellt der "noetige" Vorhalt damit zu gross aus und die Kompensation
    // zu grosszuegig. Beispiel: 25 kt Gegenwind, TAS 130, GS 105, 8 kt
    // Seitenwind → gerechnet 4,4 statt korrekt 3,5 Grad. An der 3-Grad-
    // Stufe ist das eine ganze Bandbreite geschenkt.
    //
    // Drei Stufen, absteigend nach Verlaesslichkeit:
    //   1. die GEMESSENE Eigengeschwindigkeit — beide Simulatoren liefern
    //      sie nativ (`AIRSPEED TRUE` bzw. `true_airspeed_ms`). Sie erst
    //      aus anderen Groessen zusammenzurechnen, waere Aufwand fuer ein
    //      schlechteres Ergebnis.
    //   2. Grundgeschwindigkeit plus Gegenwind — die Rekonstruktion.
    //   3. die angezeigte Fluggeschwindigkeit. Sie liegt naeher an der
    //      Eigengeschwindigkeit als die Grundgeschwindigkeit und darf
    //      deshalb NICHT noch einmal um den Gegenwind korrigiert werden.
    //
    // Die Untergrenze liegt bei 25 kt, nicht bei 40: sie soll Standwerte
    // aussortieren, nicht langsame Flugzeuge. Ein Buschflieger, der mit
    // 35 kt aufsetzt, fiel vorher auf die Ersatzannahme von 130 kt und
    // bekam dadurch nicht einmal ein Drittel des Vorhalts zugestanden, den
    // er tatsaechlich fliegen musste — ausgerechnet die Klasse, die unter
    // Seitenwind am meisten leidet.
    // Wert seit v3: bundle/grenzwerte/anflug_kern.json -> min_bezugsgeschwindigkeit_kt (v2.1.24: 25.0)
    let brauchbar =
        |v: Option<f32>| v.filter(|x| x.is_finite() && *x > crate::grenzwerte::grenzwerte().anflug_kern.min_bezugsgeschwindigkeit_kt.wert);
    input.landing_groundspeed_kt = brauchbar(stats.landing_true_airspeed_kt)
        .or_else(|| {
            match (
                brauchbar(stats.landing_groundspeed_kt),
                stats.landing_headwind_kt.filter(|v| v.is_finite()),
            ) {
                // Auch die Rekonstruktion muss durch dieselbe Pruefung:
                // bei starkem Rueckenwind kann `gs + hw` unter die Grenze
                // fallen oder negativ werden, und ein `Some` daraus wuerde
                // den Rueckfall auf die angezeigte Fluggeschwindigkeit
                // blockieren.
                (Some(gs), Some(hw)) => brauchbar(Some(gs + hw)),
                (Some(gs), None) => Some(gs),
                (None, _) => None,
            }
        })
        .or_else(|| brauchbar(stats.landing_speed_kt));
}

/// v0.7.1 Round-2 Helper: berechnet den gewichteten Aggregate-Master-
/// Score aus stats. Single-Source-of-Truth fuer alle phpVMS-/PIREP-/
/// MQTT-/UI-Pfade — vermeidet die Inkonsistenz dass App/Web "77/100"
/// zeigt aber phpVMS "60/100" speichert.
///
/// Returns None wenn keine Sub-Scores berechnet werden konnten
/// (= Touchdown-Daten fehlen voellig). Caller sollte dann auf
/// stats.landing_score.numeric() fallback'en.
///
/// Seit dem Codex-Befund zu P-4.40 nur noch in Tests: Der PIREP holt seinen
/// Score aus [`pirep_bewertete_teile`].
#[cfg(test)]
fn compute_aggregate_master_score(
    stats: &FlightStats,
    aircraft_icao: Option<&str>,
    arr_airport: &str,
    geplantes_ziel: &str,
    plan_strecke_nm: Option<f64>,
) -> Option<u8> {
    compute_bewertung(stats, aircraft_icao, arr_airport, geplantes_ziel, plan_strecke_nm)
        .and_then(|(a, _)| a.gesamt)
}

/// Die Bewertung mit Herkunft (P-4.33): `None` = `modulfehler`. Seit P-4.10
/// nur noch fuer Tests (die Abnehmer lesen gespeicherte Rechnungen).
#[cfg(test)]
fn compute_bewertung(
    stats: &FlightStats,
    aircraft_icao: Option<&str>,
    arr_airport: &str,
    // ⚠ Das GEPLANTE Ziel — nur im Vergleich mit `arr_airport`
    // (dem eingereichten) entsteht die Aussage „Ausweichflug".
    geplantes_ziel: &str,
    // v1.7.32: geplante Streckenlaenge (NM) fuer die Mehrweg-Gutschrift
    // der OFP-Achse. `None` = keine Gutschrift.
    plan_strecke_nm: Option<f64>,
) -> Option<(landing_scoring::wasm::Ausgabe, crate::bewertung::Status)> {
    let mut scoring_input = scoring_eingang(
        stats,
        aircraft_icao,
        Some(Ausweichziel {
            eingereicht: arr_airport,
            geplant: geplantes_ziel,
        }),
        plan_strecke_nm,
    );
    // v0.10.0: v2-RolloutInput-Felder mit-füllen damit der LDA-basierte
    // Sub-Score gerechnet wird (siehe spec docs/spec/v0.10.0-runway-
    // utilization-score.md).
    fill_v2_rollout_fields(&mut scoring_input, stats, arr_airport);
    // v3 (P-4.31): Bewertung nur im WASM-Modul.
    crate::bewertung::bewerten(&scoring_input)
}

/// v0.7.1 Round-2: Aggregate-Score → Label-Mapping. Damit
/// score_label und score_numeric semantisch zueinander passen.
/// Nutzt die gleichen Schwellen wie LandingScore::numeric()
/// (100/80/60/30/0 → smooth/acceptable/firm/hard/severe).
/// v0.20.0: Klassen-Schwellen liegen jetzt AUF den Noten-Schwellen von
/// `letter_grade` (95/88/82/75/65/50). Vorher waren es zwei unabhaengige
/// Leitern (90/70/45/15) — mit absurden Kombinationen als Folge: 88 Punkte
/// ergaben "A (acceptable)", 47 Punkte "F (firm)". Note und Klasse
/// beschreiben dieselbe Landung, also duerfen sie sich nicht widersprechen.
///
/// Mapping jetzt: A+/A → smooth · B+/B → acceptable · C/D → firm ·
/// F → hard (bzw. severe unter 15). Die PUNKTE aendern sich nicht, nur
/// das Wort daneben.
/// Wort der Gesamtnote fuer phpVMS-Feld und Notizen (Englisch wie bisher die
/// Klassenwoerter dort). Dieselben Schwellen wie [`aggregate_score_label`];
/// die Oberflaeche zeigt es uebersetzt (`landing.gesamt.*`).
fn gesamtnote_wort(numeric: i32) -> &'static str {
    if numeric >= 88 {
        "excellent"
    } else if numeric >= 75 {
        "good"
    } else if numeric >= 50 {
        "sufficient"
    } else if numeric >= 15 {
        "poor"
    } else {
        "insufficient"
    }
}

fn aggregate_score_label(aggregate: u8) -> &'static str {
    // v3 (P-4.21): eine Quelle — die Leiter steht im Bewertungsmodul.
    landing_scoring::wasm::gesamt_wort(aggregate)
}

/// Die EINE Landungsbewertung.
///
/// Punkte, Klasse und Note fuer genau eine Landung — an einer Stelle
/// gerechnet, von allen Ausgaben gelesen. Vorher baute sich jede Oberflaeche
/// ihr Urteil selbst zusammen, teils aus dem Gesamt-Score, teils aus der
/// Touchdown-Klasse (`LandingScore`, nur 100/80/60/30/0). Ergebnis auf EINER
/// phpVMS-Seite: Custom-Field "A (smooth) — 92/100" neben Notiz-Block
/// "A+ (SMOOTH, 100/100)".
///
/// Die Touchdown-Klasse bleibt der Fallback, wenn kein Aggregate berechenbar
/// ist (kein Buffer, Alt-Datensatz) — aber dann fuer ALLE Ausgaben gleich.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LandingVerdict {
    pub numeric: i32,
    pub label: &'static str,
    pub grade: &'static str,
}

impl LandingVerdict {
    /// "A (excellent) — 92/100" — das Format, das phpVMS-Feld und Notizen
    /// teilen. Score-Version 19: das Wort beschreibt die GESAMTNOTE, nicht
    /// den Touchdown (vorher „F (hard)" fuer eine butterweiche Landung nach
    /// instabilem Anflug). `label` bleibt die Kennung fuer Farben/Webapp.
    fn headline(&self) -> String {
        format!(
            "{} ({}) — {}/100",
            self.grade,
            gesamtnote_wort(self.numeric),
            self.numeric
        )
    }
}

/// Die Client-Version, die die Landung gemessen hat (QS 06.10.2026, Runde 6).
fn aufzeichnende_client_version(stats: &FlightStats) -> String {
    match &stats.client_version_aufsetzen {
        Some((v, at)) if Some(*at) == stats.landing_at => v.clone(),
        _ => env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Die Messwerte hinter dem Anflug-Urteil (STABLE/PARTIAL/UNSTABLE) — EINE
/// Quelle fuer Note, Deckel und `approach_stable_at_gate` im Payload.
/// Gerechnet wird NUR in `landing_scoring::anflug_urteil`.
///
/// Score-Version 19: Gleitpfad (Durchschnitt in Dots, 1000–200 ft) statt der
/// beiden Sinkraten-Abweichungen gegen eine ideale 3°-Sinkrate (GSG1709).
fn anflug_werte(stats: &FlightStats) -> landing_scoring::anflug_urteil::AnflugWerte {
    landing_scoring::anflug_urteil::AnflugWerte {
        vs_jerk_fpm: stats.approach_vs_jerk_fpm,
        // Dieselbe Zahl wie Karte und Stabilitaetsachse (v2 vor Legacy).
        bank_stddev_deg: stats.canonical_bank_stddev_deg(),
        ias_stddev_kt: stats.approach_ias_stddev_kt,
        excessive_sink: stats.approach_excessive_sink,
        stable_config: stats.approach_stable_config,
        gleitpfad_dots: stats
            .anflug_forensik
            .gleitpfad
            .as_ref()
            .and_then(|g| g.gesamt.as_ref())
            .map(|t| t.mittel_abs_dots),
        ias_stddev_unter_500_kt: stats.approach_ias_stddev_unter_500_kt,
        stable_config_500: stats.approach_stable_config_500,
        spaetes_final_rechtzeitig: stats.approach_spaetes_final.map(|sf| sf.rechtzeitig),
    }
}

/// `approach_stable_at_gate` fuer Payloads: dasselbe Urteil wie die Note
/// (vorher eine eigene, lockerere Pruefung — bei 249 Landungen „stabil",
/// waehrend die Note „teilweise" sagte).
fn anflug_stabil_am_gate(stats: &FlightStats) -> Option<bool> {
    landing_scoring::anflug_urteil::anflug_urteil(&anflug_werte(stats))
        .map(|u| u == landing_scoring::anflug_urteil::AnflugUrteil::Stable)
}

/// Die Bewertung einer Landung samt Herkunft (Kap. 4.5, P-4.40): Urteil,
/// `bewertung_status` und Fassung — fuer alle Abnehmer, die den Vermerk
/// „vorlaeufig“ oder den Modulfehler zeigen muessen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LandungsBewertung {
    /// `None` bei `modulfehler` und ohne Sinkrate (keine Gesamtnote).
    pub verdict: Option<LandingVerdict>,
    /// `endgueltig`, `vorlaeufig` oder `modulfehler`.
    pub status: &'static str,
    /// Fassung des rechnenden Moduls; `None` bei `modulfehler`.
    pub fassung: Option<u32>,
}

/// `None`, solange es keinen Aufsetzer gibt. Seit P-4.10 nur noch fuer Tests
/// (`touchdown_bewertung_festhalten` rechnet dasselbe und speichert es).
#[cfg(test)]
fn canonical_landing_bewertung(
    flight: &ActiveFlight,
    stats: &FlightStats,
    effective_arr_icao: &str,
) -> Option<LandungsBewertung> {
    stats.landing_score?;
    // Score-Version 19: ohne Gesamtnote kein Urteil — die Touchdown-Klasse
    // als Rueckfall kannte keinen Deckel (Vorlage Punktesystem, Punkt 7).
    let bewertet = compute_bewertung(
        stats,
        muster_fuer_landung(stats, &flight.aircraft_icao),
        effective_arr_icao,
        &flight.arr_airport,
        flight.plan_strecke_nm,
    );
    landungs_bewertung_aus(stats, &bewertet)
}

/// Urteil, Status und Fassung aus einer Modulrechnung. `None`, solange es
/// keinen Aufsetzer gibt.
fn landungs_bewertung_aus(
    stats: &FlightStats,
    bewertet: &Option<(landing_scoring::wasm::Ausgabe, crate::bewertung::Status)>,
) -> Option<LandungsBewertung> {
    stats.landing_score?;
    let status = crate::bewertung::status_text(bewertet);
    let fassung = bewertet.as_ref().map(|(a, _)| a.fassung);
    let verdict = bewertet.as_ref().and_then(|(a, _)| a.gesamt).map(|m| {
        let (label, numeric) = (aggregate_score_label(m), m as i32);
        LandingVerdict {
            numeric,
            label,
            grade: letter_grade(numeric),
        }
    });
    Some(LandungsBewertung { verdict, status, fassung })
}

/// Die EINE Modulrechnung einer Einreichung: Eingabe und Ergebnis zusammen
/// (Codex-Befund zu P-4.40). phpVMS-Score, Feld und Notiz, MQTT-PIREP,
/// Landungsdatensatz und das Eingabe-Dokument an live lesen sie — das
/// Dokument traegt damit genau die Eingabe, zu der sein `ergebnis` gehoert.
#[derive(Debug, Clone)]
pub(crate) struct PirepRechnung {
    /// Der eingereichte Ankunftsflughafen, mit dem gerechnet wurde.
    pub arr: String,
    pub eingabe: landing_scoring::LandingScoringInput,
    pub ergebnis: Option<(landing_scoring::wasm::Ausgabe, crate::bewertung::Status)>,
}

fn pirep_rechnung_bilden(flight: &ActiveFlight, stats: &FlightStats, effective_arr_icao: &str) -> PirepRechnung {
    let eingabe = landungs_bewertungseingabe(
        flight,
        stats,
        muster_fuer_landung(stats, &flight.aircraft_icao),
        effective_arr_icao,
    );
    // P-3.31: mit dem Stand des Flugs; fehlt er, „vorlaeufig“.
    let ergebnis = crate::bewertung::bewerten_fuer(&flight.pirep_id, &eingabe);
    PirepRechnung { arr: effective_arr_icao.to_string(), eingabe, ergebnis }
}

/// Die Rechnung dieser Einreichung (festgehalten von
/// [`pirep_bewertete_teile`]); nur bei anderem Ankunftsflughafen oder ohne
/// vorangegangenen Aufbau wird neu gerechnet.
///
/// Datensatz und MQTT-PIREP entstehen erst nach dem Senden an phpVMS. Sie
/// beschreiben dieselbe Einreichung, also dieselbe Rechnung: Neu gerechnet
/// koennte ein kurzer Modulausfall dort „vorlaeufig“ schreiben, wo phpVMS
/// „endgueltig“ bekam.
fn einreichungs_rechnung(flight: &ActiveFlight, stats: &FlightStats, effective_arr_icao: &str) -> PirepRechnung {
    match &stats.pirep_rechnung {
        Some(r) if r.arr == effective_arr_icao => r.clone(),
        _ => pirep_rechnung_bilden(flight, stats, effective_arr_icao),
    }
}

/// v3 (P-5.10): Nummer des laufenden Aufsetzers — die Touch-and-Go davor sind
/// schon eingefroren (P-4.13), er bekommt die naechste Nummer wie sein
/// Eingabe-Dokument.
fn touchdown_seq(stats: &FlightStats) -> u32 {
    stats.aufsetzer_eingefroren.len() as u32 + 1
}

/// v3 (P-5.10): Hash der Eingabe, mit der die Sofortnote rechnet (am
/// Aufsetz-Flughafen, wie `touchdown_bewertung_festhalten`) — ueber dieselbe
/// Funktion wie `meta.input_sha256` des Dokuments.
fn touchdown_input_sha256(flight: &ActiveFlight, stats: &FlightStats) -> String {
    let arr = aufsetz_flughafen(flight, stats);
    let eingabe = landungs_bewertungseingabe(flight, stats, muster_fuer_landung(stats, &flight.aircraft_icao), &arr);
    landungs_eingabe::input_sha256(&eingabe, &landungs_herkunft(flight, stats, &eingabe, &arr))
}

/// Entsteht beim Einreichen ein Landungsdatensatz (und damit das
/// Eingabe-Dokument der Landung)? Dieselben Bedingungen wie
/// `build_landing_record`, das sie zuerst prueft.
fn landungsdatensatz_moeglich(stats: &FlightStats) -> bool {
    let nicht_bewertbar = stats.landung_abdeckung_fehlt.is_some();
    stats.landing_at.is_some()
        && (stats.canonical_landing_rate_fpm().is_some() || nicht_bewertbar)
        && (stats.landing_score.is_some() || nicht_bewertbar)
}

/// v3 (P-5.10): `seq` und `input_sha256` des Eingabe-Dokuments der Landung
/// dieser Einreichung — dieselbe Rechnung (`einreichungs_rechnung`) und
/// dieselbe Herkunft wie in `eingabe_teile`. Ohne Aufsetzfenster gibt es kein
/// Dokument, dann beides `None`.
fn pirep_eingabe_kennung(
    flight: &ActiveFlight,
    stats: &FlightStats,
    rechnung: &PirepRechnung,
) -> (Option<u32>, Option<String>) {
    // Genau dann gibt es ein Dokument der Landung (`eingabe_teile` mit
    // `record.is_some()`), Codex 30.10.
    if stats.landung_fenster.is_empty() || !landungsdatensatz_moeglich(stats) {
        return (None, None);
    }
    let herkunft = landungs_herkunft(flight, stats, &rechnung.eingabe, &rechnung.arr);
    (Some(touchdown_seq(stats)), Some(landungs_eingabe::input_sha256(&rechnung.eingabe, &herkunft)))
}

/// Note, Buchstabe und Herkunft fuer den Touchdown-Payload (live/Discord):
/// `vorlaeufig` reist als Vermerk mit; bei `modulfehler` gibt es keine Note
/// (P-4.40, Kap. 4.5). `None` = noch kein Aufsetzer.
fn touchdown_note_felder(
    bewertung: Option<&LandungsBewertung>,
) -> (Option<String>, Option<String>, Option<String>) {
    let v = bewertung.and_then(|b| b.verdict);
    (
        v.map(|v| v.label.to_string()),
        v.map(|v| v.grade.to_string()),
        bewertung.map(|b| b.status.to_string()),
    )
}

/// Der Flughafen, auf dem aufgesetzt wurde: das Ausweichfeld, wenn die
/// Aufsetzkoordinaten dort liegen, sonst das geplante Ziel. Eingabe der
/// Touchdown-Bewertung (Meldung und Payload) — beim Divert gibt es den
/// bestaetigten Hinweis erst im Stillstand.
fn aufsetz_flughafen(flight: &ActiveFlight, stats: &FlightStats) -> String {
    match (stats.landing_lat, stats.landing_lon) {
        (Some(la), Some(lo)) => {
            match arrival::locate(&flight.arr_airport, la, lo, stats.planned_arr_ref_pos) {
                arrival::ArrivalSite::AtOtherAirport { icao, .. } => icao,
                _ => flight.arr_airport.clone(),
            }
        }
        _ => flight.arr_airport.clone(),
    }
}

/// Rechnet die Bewertung der Landung EINMAL (am Aufsetz-Flughafen) und haelt
/// sie in `stats.landung_bewertung` fest — fuer Meldung und Payload.
///
/// v3 (P-4.10): Eingabe und Ausgabe dieser Rechnung bleiben als Sofortnote
/// (`landung_sofort`) gespeichert — dieselbe Rechnung wie
/// `canonical_landing_bewertung` (`compute_bewertung` baut dieselbe Eingabe
/// wie `landungs_bewertungseingabe`).
fn touchdown_bewertung_festhalten(
    flight: &ActiveFlight,
    stats: &mut FlightStats,
) -> Option<LandungsBewertung> {
    if stats.landing_score.is_none() {
        stats.landung_bewertung = None;
        return None;
    }
    let pin_fehlte = crate::bewertung::pin_fehlt(&flight.pirep_id);
    let rechnung = pirep_rechnung_bilden(flight, stats, &aufsetz_flughafen(flight, stats));
    let b = landungs_bewertung_aus(stats, &rechnung.ergebnis);
    stats.landung_sofort = rechnung_schluessel(stats).map(|schluessel| GespeicherteRechnung { schluessel, pin_fehlte, rechnung });
    stats.landung_bewertung = b;
    b
}

/// v3 (P-4.10): eine gespeicherte Modulrechnung je Aufsetzer. Der Schluessel
/// (Aufsetzzeit in ms, `bahn_revision`) sagt, fuer welchen Aufsetzer und
/// welchen Bahnstand sie gilt.
#[derive(Debug, Clone)]
pub(crate) struct GespeicherteRechnung {
    pub schluessel: (i64, u32),
    /// P-3.31: gerechnet, waehrend der Stand des Flugs fehlte („vorlaeufig“).
    pub pin_fehlte: bool,
    pub rechnung: PirepRechnung,
}

fn rechnung_schluessel(stats: &FlightStats) -> Option<(i64, u32)> {
    Some((aufsetzzeit(stats)?.timestamp_millis(), stats.bahn_revision))
}

/// v3 (P-4.10): Das Modul rechnet je Aufsetzer ein zweites Mal, sobald das
/// Ausrollen fertig ist (`rollout_finalized`) und keine Rollspur mehr
/// laeuft — dieselbe Schwelle wie der Rollout-Nachtrag, aber unabhaengig von
/// der Verbindung. Aendert sich die Bahn danach (`bahn_revision`), wird
/// erneut gerechnet; die Rechnung selbst laeuft ausserhalb der Sperre.
fn endgueltige_rechnung_festhalten(flight: &ActiveFlight) {
    let (schluessel, arr, eingabe) = {
        let stats = flight.stats.lock().expect("flight stats");
        if stats.landing_score.is_none() || !stats.rollout_finalized || stats.bahn_spur_laeuft {
            return;
        }
        let Some(schluessel) = rechnung_schluessel(&stats) else { return };
        if stats.landung_endgueltig.as_ref().is_some_and(|g| g.schluessel == schluessel) {
            return;
        }
        let arr = aufsetz_flughafen(flight, &stats);
        let eingabe = landungs_bewertungseingabe(flight, &stats, muster_fuer_landung(&stats, &flight.aircraft_icao), &arr);
        (schluessel, arr, eingabe)
    };
    let pin_fehlte = crate::bewertung::pin_fehlt(&flight.pirep_id);
    let ergebnis = crate::bewertung::bewerten_fuer(&flight.pirep_id, &eingabe);
    let mut stats = flight.stats.lock().expect("flight stats");
    // Nur, wenn derselbe Aufsetzer mit demselben Bahnstand noch gilt.
    if rechnung_schluessel(&stats) == Some(schluessel) {
        stats.landung_endgueltig = Some(GespeicherteRechnung { schluessel, pin_fehlte, rechnung: PirepRechnung { arr, eingabe, ergebnis } });
    }
}

/// v3 (P-4.10): die letzte gespeicherte Rechnung, die fuer diese Einreichung
/// gilt — endgueltig vor Sofortnote. Keine gilt (dann rechnet die
/// Einreichung selbst, mit Logzeile), wenn nichts gespeichert ist (Neustart,
/// Abgabe vor dem Ausrollen), ein anderer Ankunftsflughafen eingereicht wird
/// als der, auf dem aufgesetzt wurde (andere Eingabe: `diverted`), oder sich
/// die Bahnzuordnung seither geaendert hat (`bahn_revision`). P-3.31: Eine
/// Rechnung ohne den Stand des Flugs gilt nicht mehr, sobald er da ist
/// (`pin_jetzt_da`) — dann rechnet die Einreichung mit ihm neu.
fn gespeicherte_rechnung(stats: &FlightStats, effective_arr_icao: &str, pin_jetzt_da: bool) -> Option<PirepRechnung> {
    let schluessel = rechnung_schluessel(stats)?;
    [stats.landung_endgueltig.as_ref(), stats.landung_sofort.as_ref()]
        .into_iter()
        .flatten()
        .find(|g| g.schluessel == schluessel && g.rechnung.arr == effective_arr_icao && !(g.pin_fehlte && pin_jetzt_da))
        .map(|g| g.rechnung.clone())
}

/// Note, Buchstabe und Status des Touchdown-Payloads aus der in
/// [`announce_landing_score`] festgehaltenen Bewertung — der Payload wird nur
/// im selben Takt nach einer Meldung gebaut. Leer ist sie nur, wenn auch
/// `landing_score` geloescht wurde; dann haette die Kanonik ebenfalls `None`.
fn touchdown_payload_note(stats: &FlightStats) -> (Option<String>, Option<String>, Option<String>) {
    touchdown_note_felder(stats.landung_bewertung.as_ref())
}

/// Vermerk fuer Notiz und Meldung, wenn die Note nicht „endgueltig“ ist.
/// `None` im Normalfall (Modul des Pakets rechnet).
fn bewertung_vermerk(b: &LandungsBewertung) -> Option<String> {
    match b.status {
        "vorlaeufig" => Some(match b.fassung {
            Some(n) => format!("vorläufig, Fassung {n}"),
            None => "vorläufig".to_string(),
        }),
        "modulfehler" => Some("nicht bewertet (Modulfehler)".to_string()),
        _ => None,
    }
}

/// v0.7.1 Round-2: leitet GateWindow-Werte aus den ApproachSamples ab.
/// Damit die UI-Tooltip-Aussage "Bewertet werden Anflug-Samples
/// zwischen X und Y ft AGL" wirklich auf den ECHTEN bewerteten
/// Bereich verweist und nicht nur auf die Crate-Konstanten.
///
/// Returns None wenn keine Samples die `is_scored_gate=true` Flag haben
/// (z.B. fuer pre-v0.7.1-PIREPs ohne die Felder).
///
/// Tuple-Format weil es zwei verschiedene GateWindow-Structs gibt
/// (storage + aeroacars-mqtt) — Caller baut den passenden Type.
fn extract_gate_window_values(
    approach_samples: &[ApproachSample],
) -> Option<(i64, i64, f32, f32, u32)> {
    let scored: Vec<&ApproachSample> = approach_samples
        .iter()
        .filter(|s| s.is_scored_gate.unwrap_or(false))
        .collect();
    if scored.is_empty() {
        return None;
    }
    let start = scored.first()?;
    let end = scored.last()?;
    Some((
        start.t_ms.unwrap_or(0) as i64,
        end.t_ms.unwrap_or(0) as i64,
        start.agl_ft.unwrap_or(0.0),
        end.agl_ft.unwrap_or(0.0),
        scored.len() as u32,
    ))
}

fn build_storage_gate_window(approach_samples: &[ApproachSample]) -> Option<GateWindow> {
    extract_gate_window_values(approach_samples).map(|(s, e, sh, eh, n)| GateWindow {
        start_at_ms: s,
        end_at_ms: e,
        start_height_ft: sh,
        end_height_ft: eh,
        sample_count: n,
    })
}

/// v0.7.1 Round-2: GateWindow direkt aus stats.approach_buffer +
/// stats.landing_at fuer den MQTT-Pfad (build_landing_record schon
/// hat approach_samples lokal, aber der PIREP-Payload-Builder nicht).
/// Berechnet die gleichen is_scored_gate-Flags wie in
/// build_landing_record (height in 0..=1000 ft AGL minus letzte 3s
/// vor TD) und liefert das resultierende Window.
fn build_mqtt_gate_window_from_stats(stats: &FlightStats) -> Option<aeroacars_mqtt::GateWindow> {
    use landing_scoring::gate::{
        STABILITY_GATE_FLARE_CUTOFF_MS, STABILITY_GATE_MAX_HEIGHT_FT, STABILITY_GATE_MIN_HEIGHT_FT,
    };
    let td_ts = stats.landing_at?;
    let scored: Vec<(&ApproachBufferSample, i32)> = stats
        .approach_buffer
        .iter()
        .filter_map(|s| {
            let height = s.agl_ft;
            let in_height_band =
                height > STABILITY_GATE_MIN_HEIGHT_FT && height <= STABILITY_GATE_MAX_HEIGHT_FT;
            let dt_ms = (s.at - td_ts).num_milliseconds() as i32;
            let ms_before_td = (-dt_ms) as i64;
            let in_flare = ms_before_td >= 0 && ms_before_td < STABILITY_GATE_FLARE_CUTOFF_MS;
            if in_height_band && !in_flare {
                Some((s, dt_ms))
            } else {
                None
            }
        })
        .collect();
    if scored.is_empty() {
        return None;
    }
    let (start, start_t) = scored.first()?;
    let (end, end_t) = scored.last()?;
    Some(aeroacars_mqtt::GateWindow {
        start_at_ms: *start_t as i64,
        end_at_ms: *end_t as i64,
        start_height_ft: start.agl_ft,
        end_height_ft: end.agl_ft,
        sample_count: scored.len() as u32,
    })
}

/// v0.7.1 (Spec F4 + P2.2-D): atomic write of landing rate + confidence
/// + source.
///
/// Realitaet vor v0.7.1: `landing_rate_fpm` wurde an drei Stellen final
/// gesetzt (lib.rs:9362 touchdown_v2-dump, lib.rs:11532 streamer-tick,
/// lib.rs:12312 sampler-carry-over). Wenn nur am Edge VALIDATE die
/// Confidence/Source gespeichert wurden, konnten Werte und Confidence
/// auseinanderlaufen — z.B. wenn der Streamer einen Smoothed-Fallback
/// schrieb, Confidence aber noch "High" vom Edge stand.
///
/// Loesung: alle 3 Sites rufen diesen Helper. Confidence/Source
/// IMMER konsistent zum finalen `landing_rate_fpm`.
/// Stammt die Sinkrate aus einer Meldung des Simulators selbst?
///
/// # Warum das die Sperre begrenzt (Messung 12.09.2026)
///
/// MSFS meldet die Aufsetz-Sinkrate über `PLANE TOUCHDOWN NORMAL VELOCITY`
/// selbst — der Simulator misst sie in seinem eigenen Physikschritt, im
/// richtigen Moment, ganz ohne unsere Abtastung. Von 1170 Landungen des
/// Bestands haben 29 gar kein Aufsetzfenster (der Sampler kam nicht dazu)
/// und trotzdem einen belastbaren Wert — genau über diese Quelle.
///
/// Eine pauschale Sperre "keine Abdeckung, keine Note" hätte diese
/// Landungen zu Unrecht entwertet. Die Abdeckung unserer eigenen Proben
/// sagt nichts über einen Wert aus, den der Simulator gemessen hat.
///
/// Alle übrigen Quellen (`vs_at_impact_frame`, `vs_smoothed_*`,
/// `pre_flare_peak`, `vs_at_edge_50hz`, `buffer_min`, `agl_estimate_msfs`,
/// `sampler_gear_force`) hängen an unserem 50-Hz-Fenster und fallen mit ihm.
fn quelle_misst_der_simulator_selbst(source: Option<&str>) -> bool {
    matches!(source, Some("msfs_simvar_latched") | Some(PLUGIN_SINKRATE_QUELLE))
}

/// Quellenkennung der Sinkrate, die das X-Plane-Plugin beim Aufsetzen
/// selbst gemessen hat.
const PLUGIN_SINKRATE_QUELLE: &str = "xplane_plugin_touchdown";

/// Reicht unsere Aufzeichnung nicht, gilt die Sinkrate des X-Plane-Plugins.
///
/// # Der Anlass (AZU 8710, SBRF→KMCO, 11.10.2026)
///
/// X-Plane 12 auf dem Mac, AeroACARS im Hintergrund: In der Flare kamen
/// statt ~50 nur 4 Proben je Sekunde an — der ganze Client stand, X-Plane
/// lief flüssig. Das Plugin misst im Flugschleifen-Callback von X-Plane,
/// also unabhängig von unserem Takt, und hatte −140 fpm sauber erfasst.
/// Verworfen wurde die Landung trotzdem.
///
/// Gegenprobe über 37 X-Plane-Landungen mit Plugin und dichtem Fenster
/// (Diagnoselogs auf live): Abweichung zum gewerteten Wert im Median
/// 17,6 fpm, 90 % unter 62 fpm.
///
/// Wie bei MSFS gilt die Ausnahme nur der Sinkrate. G und Hopser kommen aus
/// unserem Fenster und fallen weiter weg (`canonical_peak_g_force`).
fn plugin_sinkrate_uebernehmen(stats: &mut FlightStats) -> bool {
    if stats.landung_abdeckung_fehlt.is_none() || !stats.sampler_touchdown_vs_vom_plugin {
        return false;
    }
    let Some(vs) = stats
        .sampler_touchdown_vs_fpm
        .filter(|v| v.is_finite() && *v < 0.0)
    else {
        return false;
    };
    finalize_landing_rate(stats, vs, Some("High"), Some(PLUGIN_SINKRATE_QUELLE));
    true
}

fn finalize_landing_rate(
    stats: &mut FlightStats,
    vs_fpm: f32,
    confidence: Option<&str>,
    source: Option<&str>,
) {
    // Reicht die Aufzeichnung nicht, wird die Zahl gar nicht erst
    // festgehalten — auch nicht im Protokoll. Sonst stünde dort ein Wert,
    // den die Anzeige zu Recht verschweigt, und die nächste Auswertung
    // fände ihn und hielte ihn für gültig (derselbe Riss, den die Kanonik
    // oben ausräumt). Quelle und Vertrauensstufe bleiben als Begründung.
    if stats.landung_abdeckung_fehlt.is_some() && !quelle_misst_der_simulator_selbst(source) {
        // Steht schon eine vom Simulator gemeldete Rate da, bleibt sie
        // unangetastet — samt Quellenkennung.
        //
        // Codex, zweite Abnahme 13.09.2026: Die erste Fassung überschrieb in
        // diesem Fall die Quelle mit "aufzeichnung_unzureichend". Ablauf: MSFS
        // meldet −168 fpm, danach kommt der Sampler mit −10,4 aus dem kaputten
        // Fenster. Der neue Wert wurde zu Recht verworfen, aber mit der
        // Kennung ging auch die Ausnahme verloren — die Kanonik lieferte
        // danach `None` statt −168. Verworfen wird nur der NEUE Wert.
        if quelle_misst_der_simulator_selbst(stats.landing_source.as_deref()) {
            return;
        }
        stats.landing_confidence = Some("NichtBewertbar".to_string());
        stats.landing_source = Some("aufzeichnung_unzureichend".to_string());
        return;
    }
    stats.landing_rate_fpm = Some(vs_fpm);
    stats.landing_peak_vs_fpm = Some(vs_fpm);
    stats.landing_confidence = confidence.map(|s| s.to_string());
    stats.landing_source = source.map(|s| s.to_string());
}

// Build a `LandingRecord` from the current flight + stats. Returns None
// when there's no usable touchdown captured (e.g. PIREP filed before
// landing — synthetic / bug). The record is the immutable snapshot we
// show in the Landing tab; once written it never changes.
// v0.13.15: maximales Post-TD-Fenster fuer das angezeigte V/S-Profil.
// Wert seit v3: bundle/grenzwerte/landung.json -> post_td_profile_ms (v2.1.24: 10_000)

/// v0.13.15 (Pilot-Befund ViolonC 2026-05-31): Baut das anzuzeigende V/S-
/// Touchdown-Profil. `snapshot_profile` ist der aus dem 5-s-snapshot_buffer
/// eingefrorene Teil (Pre-TD + die paar 100 ms, die der ggf. verspaetete
/// Streamer-Tick noch mitnahm). `post_buffer` enthaelt dieselben Samples PLUS
/// bis zu 10 s Post-TD (vom 50-Hz-Sampler). Wir uebernehmen den snapshot-Teil
/// 1:1 und haengen aus dem post_buffer nur die Samples NACH dem bereits
/// vorhandenen Maximum an (bis `post_td_profile_ms`), damit es im Overlap
/// keine Doppel-Punkte gibt. Vorher wurde der post_buffer beim Dump geleert
/// → der Record sah nur die ~0.5 s aus dem snapshot, obwohl der Chart
/// "inkl. 10 s nach TD" anzeigt (Discord-Befund WFL1505, 2026-05-31).
fn merge_touchdown_profile(
    snapshot_profile: &[TouchdownProfilePoint],
    post_buffer: &std::collections::VecDeque<TelemetrySample>,
    touchdown_at: DateTime<Utc>,
    post_td_profile_ms: i32,
) -> Vec<LandingProfilePoint> {
    let mut profile: Vec<LandingProfilePoint> = snapshot_profile
        .iter()
        .map(|p| LandingProfilePoint {
            t_ms: p.t_ms,
            vs_fpm: p.vs_fpm,
            g_force: p.g_force,
            agl_ft: p.agl_ft,
            on_ground: p.on_ground,
            heading_true_deg: p.heading_true_deg,
            groundspeed_kt: p.groundspeed_kt,
            indicated_airspeed_kt: p.indicated_airspeed_kt,
            pitch_deg: p.pitch_deg,
            bank_deg: p.bank_deg,
        })
        .collect();
    let existing_max_t_ms = profile.iter().map(|p| p.t_ms).max().unwrap_or(0);
    for s in post_buffer.iter() {
        let t_ms = (s.at - touchdown_at).num_milliseconds() as i32;
        if t_ms > existing_max_t_ms && t_ms <= post_td_profile_ms {
            profile.push(LandingProfilePoint {
                t_ms,
                vs_fpm: s.vs_fpm,
                g_force: s.g_force,
                agl_ft: s.agl_ft,
                on_ground: s.on_ground,
                heading_true_deg: s.heading_true_deg,
                groundspeed_kt: s.groundspeed_kt,
                indicated_airspeed_kt: s.indicated_airspeed_kt,
                pitch_deg: s.pitch_deg,
                bank_deg: s.bank_deg,
            });
        }
    }
    profile
}

/// Same precedence as the TS `resolveFlightIdent()` (`src/lib/callsign.ts`)
/// and phpVMS's own `Flight::atc()` accessor: prefer a non-empty, trimmed
/// `callsign` over the raw `flight_number`. Extracted so it's directly unit-
/// testable without needing a full `FlightStats` fixture (see
/// `build_landing_record`, the only current caller).
fn resolve_flight_ident(flight_number: &str, callsign: Option<&str>) -> String {
    callsign
        .map(str::trim)
        .filter(|cs| !cs.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| flight_number.to_string())
}

/// v1.6.8 — der Versatz der Lande-Schwelle, wie ihn Anzeige UND Bewertung
/// brauchen: der gemeldete Wert aus Navigraph/OurAirports.
///
/// v1.7.18-QS1: NICHT mehr das Maximum aus Feld und Geometrie. Ein erster
/// Entwurf hat `m.displaced_threshold_ft.max(m.geometry_implied_displaced_
/// threshold_ft)` gerechnet, um einen Versatz zurueckzuholen, den nur die
/// Geometrie zeigt (Feld = 0). Eine QS-Runde hat das an 33.390 Bahnen OHNE
/// gemeldeten Versatz nachgerechnet: bei 1.187 (3,6 %) erfindet reines
/// Koordinatenrauschen einen Versatz, den es nicht gibt — teils über
/// 500 m (LETO 22, OYSH 36), an echten Flughaefen wie OLBA oder LROP
/// zwischen 20 und 30 m. Das MAX haette diesen Rateversatz ungeprueft in
/// die nutzbare Laenge und die Anzeige uebernommen. Der Ertrag (ein
/// theoretischer "Feld=0, aber schon versetzt"-Fall, siehe `runway.rs`)
/// ist in keiner realen Bahn nachgewiesen — das Risiko war real, der
/// Nutzen nicht. Deshalb zaehlt nur noch das gemeldete Feld.
fn effective_displaced_threshold_ft(m: &runway::RunwayMatch) -> i32 {
    m.displaced_threshold_ft
}

/// v1.6.8-QS4 — der Anteil des Versatzes, der NOCH NICHT im
/// Schwellenpunkt selbst steckt. Nur dieser darf von der gemessenen
/// Aufsetzdistanz abgezogen werden.
///
/// Warum das eine eigene Funktion ist: die Aufsetzdistanz wird ab dem
/// Schwellenpunkt der Navdaten gemessen. Liegt der schon versetzt, ist
/// die Distanz bereits ab der Lande-Schwelle — ein Abzug waere ein
/// zweiter.
///
/// v1.7.18: `RunwayMatch::geometry_implied_displaced_threshold_ft` sagt,
/// wie viel Versatz die Geometrie selbst zeigt (reine Navigraph-
/// Selbstprobe, siehe deren Doku in `runway.rs` fuer die Vorgeschichte:
/// der Vorgaenger hier hat dieselbe Frage ueber die eingebettete
/// OurAirports-Tabelle geraten und lag bei 66 % der Bahnen mit echtem
/// Versatz falsch, weil OurAirports fuer dieses Feld meist keinen
/// brauchbaren Wert hat).
///
/// v1.7.18-QS1: KEIN blosses `(Feld − Geometrie).max(0)` mehr. Der erste
/// Entwurf hatte genau das gerechnet — und damit einen GEFAEHRLICHEREN
/// Fehler eingebaut als den, den er beheben sollte: Ueberschaetzt die
/// Geometrie den gemeldeten Versatz (reines Koordinatenrauschen bei
/// kleinen/schlecht vermessenen Plaetzen — an 43 von 5.369 Bahnen mit
/// echtem Versatz um mehr als das Doppelte, Extremfall 4,3-fach), wird
/// die Differenz negativ, `.max(0)` macht daraus 0 — die Sperrzonen-
/// Pruefung (`classify_displaced`) waere fuer diese Bahn KOMPLETT UND
/// STILL abgeschaltet gewesen, obwohl der Versatz nie geometrisch
/// bestaetigt wurde. Jetzt gilt: nur wenn die Geometrie den gemeldeten
/// Wert INNERHALB der Toleranz BESTAETIGT (nicht bloss uebertrifft),
/// gilt er als schon in der Geometrie — sonst wird der volle gemeldete
/// Wert abgezogen (der sichere Standardfall von vor diesem Umbau).
///
/// v1.7.18-QS2: `geometry_implied_displaced_threshold_ft == 0` heisst bei
/// `geometry_implied_displacement_ft` NIE "bestaetigt kein Versatz" —
/// jeder Ruecksprung dort (ungueltige Eingaben, keine plausible
/// Gegenschwelle, unplausibel grosser Wert, siehe deren Doku) liefert
/// ebenfalls 0. Ohne den `> 0`-Riegel unten haette ein KLEINER gemeldeter
/// Versatz (≤ 65 ft ≈ 20 m) diesen "nichts erkannt"-Nullwert als
/// Bestaetigung durchgehen lassen — die Distanz zur Toleranz waere
/// zufaellig klein genug, obwohl die Geometrie ueberhaupt nichts gemessen
/// hat. Gefangen in einer externen QS-Runde.
fn displacement_not_in_geometry_ft(m: &runway::RunwayMatch) -> i32 {
    versatz_nicht_in_geometrie_ft(
        m.displaced_threshold_ft,
        m.geometry_implied_displaced_threshold_ft,
    )
}

/// Kern von `displacement_not_in_geometry_ft` auf zwei Zahlen — auch fuer
/// den ILS-Bezug aus der unveraenderten Navigraph-Bahn (Score-Version 25).
fn versatz_nicht_in_geometrie_ft(gemeldet_ft: i32, geometrie_ft: i32) -> i32 {
    // Dieselbe Toleranz wie `runway::geometry_implied_displacement_ft`
    // (dort begruendet: 40-Bahnen-Stichprobe trennt sauber bei
    // hoechstens 13,7 m Rauschen gegen mindestens 30,8 m bei echtem,
    // aber physischem Versatz).
    // Wert seit v3: bundle/grenzwerte/ausrollen.json -> bestaetigungs_toleranz_m (v2.1.24: 20.0)
    const FT_TO_M: f64 = 0.3048;

    if gemeldet_ft <= 0 || geometrie_ft <= 0 {
        return gemeldet_ft.max(0);
    }
    let diff_m = (geometrie_ft - gemeldet_ft) as f64 * FT_TO_M;
    if diff_m.abs() <= crate::grenzwerte::grenzwerte().ausrollen.bestaetigungs_toleranz_m.wert {
        0
    } else {
        gemeldet_ft
    }
}

/// Wire-format displaced-threshold value for a persisted `LandingRunwayMatch`
/// / live `TouchdownPayload`. Reads from the runway match's OWN field
/// (`RunwayMatch::displaced_threshold_ft`), never from `runway_nav_geometry`.
///
/// v0.20.x QS fix: both wire sites used to read
/// `runway_nav_geometry.map(|g| g.displaced_threshold_ft)` — Navigraph-only,
/// so it silently stayed `None` on every OurAirports-fallback landing, even
/// though `RunwayMatch::displaced_threshold_ft` (this release's `71a6533`)
/// now correctly populates from BOTH sources and is exactly what
/// `assess_touchdown`/`fill_v2_rollout_fields` already score against.
/// Reading from the match itself instead keeps what's persisted/displayed
/// in sync with the score for the same landing. Extracted so this is
/// directly unit-testable without needing a full `FlightStats` fixture.
///
/// v1.6.8: liefert den EFFEKTIVEN Versatz (siehe
/// `effective_displaced_threshold_ft`). Ohne das zeigte die Kachel „1818 m
/// von 3250 m · 56 %", waehrend die Punkte gegen 2952 m gerechnet wurden —
/// genau das Auseinanderfallen von Anzeige und Bewertung, das v1.6.7
/// gerade beseitigt hat.
fn wire_displaced_threshold_ft(runway_match: Option<&runway::RunwayMatch>) -> Option<i32> {
    runway_match.map(effective_displaced_threshold_ft)
}

/// Die Bahndisziplin-Felder fuer den Datensatz — an **einer** Stelle
/// abgeleitet.
///
/// Warum das eine eigene Funktion ist und nicht zwoelf Zeilen im
/// Initialisierer: Dieselben Werte gehen in den lokalen Datensatz, in den
/// Server-Upload und in die Bewertung. Drei Stellen, die dasselbe rechnen,
/// driften auseinander — das ist die Fehlerklasse aus §9 der Spezifikation,
/// und sie hat in diesem Projekt schon zweimal zugeschlagen (die vier
/// Kopien der Bahnprojektion, die zwei Prozentzahlen fuer dieselbe Bahn).
/// `icao` ist der **aufgeloeste** Typcode aus der Kette Sim → Buchung →
/// Server → Titel (Schritt 1 der Bauliste), nicht der rohe Sim-Wert. Wer
/// hier `stats` erneut befragte, umginge genau die Kette, die der MPH-9-Fall
/// erzwungen hat: Die TFDi MD-11 meldet keinen Typ, und ohne Kette fiel die
/// Bewertung auf „Light" zurueck — 55 statt 80 Punkte.
/// Der Grund, aus dem die Bahndisziplin-Achse nicht gewertet hat.
///
/// Ausgelesen, nicht hergeleitet: Die Achse hat schon entschieden, und die
/// Anzeige soll genau diese Entscheidung zeigen. Eine zweite Herleitung in
/// der Grafik waere eine Zweitimplementierung des Urteils — und die driften
/// auseinander, sobald jemand eine Schwelle anfasst.
fn bahn_skip_grund(sub_scores: &[landing_scoring::SubScoreEntry]) -> Option<String> {
    sub_scores
        .iter()
        .find(|s| s.key == "rollout" && s.skipped)
        .and_then(|s| s.reason.clone())
}

/// Das Flugzeugmuster fuer eine abgeschlossene Landung.
///
/// Nimmt das waehrend des Fluges aufgeloeste Muster (Sim → Buchung →
/// Flugzeugtitel, siehe `sim_core::muster_aufloesen`) und faellt nur auf
/// die Buchung zurueck, wenn es keins gibt — etwa bei einem Flug, der vor
/// dieser Fassung begonnen hat.
///
/// # Warum das nicht jede Stelle selbst macht
///
/// Am 24.08.2026 gab es SIEBEN Stellen mit vier verschiedenen Ketten. Die
/// beiden wichtigsten — das Landeurteil und die maßgebliche
/// PIREP-Bewertung — fragten **nur die Buchung**. Bei EWG248 (EDDL) war
/// die leer: keine Spurweite, keine Spannweite, Bank-Grenze und Vref aus
/// dem Rueckfall, und die Querbewertung fiel aus.
/// `tests/musterquelle.rs` haelt fest, dass es dabei bleibt.
fn muster_fuer_landung<'a>(stats: &'a FlightStats, buchung_icao: &'a str) -> Option<&'a str> {
    stats
        .aufgeloestes_muster
        .as_deref()
        .or_else(|| Some(buchung_icao.trim()).filter(|s| !s.is_empty()))
}

/// Die Eingabe der Landungsbewertung — an EINER Stelle gebaut.
/// Die Fassung der Bewertungslogik, die an jedem Flug mitgeschrieben wird.
///
/// # Wann sie steigen MUSS
///
/// Immer, wenn sich eine Note fuer denselben Flug aendern wuerde:
/// verschobene Baender, andere Punkte, eine Achse mehr oder weniger.
/// Ohne den Sprung sind Fluege zweier Fassungen maschinell nicht mehr
/// zu trennen — und jede Auswertung ueber den Bestand mischt zwei
/// verschiedene Massstaebe, ohne es zu merken.
///
/// ⚠ Sie stand von v1.7.0 bis v1.7.11 auf 9, obwohl sich in dieser Zeit
/// mehrfach Noten verschoben haben. v1.7.12 aendert VIER Achsen auf
/// einmal (OFP-Treue, Ladepapiere, G-Kraefte, Bahndisziplin) — deshalb
/// damals 10.
///
/// **11 seit v1.7.13**: Die Stabilitaets-Achse hat neue Baender
/// (200/400/700/1000 statt 100/200/400/700). Gemessen ueber 699
/// Landungen bekommen damit **39 % der Fluege 100 statt 80 Punkte** —
/// derselbe Flug wird unter v1.7.12 und v1.7.13 verschieden benotet.
/// Ohne den Sprung mischt jede Auswertung ueber den Bestand zwei
/// Massstaebe.
///
/// **12 seit v1.7.21**: Die Bahndisziplin-Fensterschwelle
/// (`BAHN_MESS_MIN_GS_KT` / `bahn_mess_schwelle_kt`) ist nicht mehr fest bei
/// 60 kt, sondern relativ zur Aufsetz-Grundgeschwindigkeit
/// (`clamp(0.6 * aufsetz_gs, 25, 60)`). Fuer leichte GA-Muster (44-56 kt
/// Aufsetz-GS) bleibt das Messfenster jetzt laenger offen und liefert dort
/// Bahndisziplin-Proben, wo vorher `insufficient_samples` stand — derselbe
/// Flug wird unter v1.7.20 und v1.7.21 verschieden bewertet.
///
/// **13 seit v1.7.23**: GSG1249-Vereinheitlichung. Drei unabhaengige
/// Aenderungen koennen dieselbe Landung anders bewerten als zuvor:
/// (1) der Settle-Pfad in `touchdown_v2::validate_candidate` bestaetigt
/// sanfte, aber echte Touchdowns, die vorher an der starren 3-von-4-
/// Abstimmung scheiterten (kein Score statt Score); (2) die Sim-
/// Schreibweisen-Kaskade (`muster_fuer_typtabelle`) loest Baureihen ohne
/// Variante wie "A300" jetzt auf "A306" auf — Spurweite/Spannweite/
/// Bahndisziplin-Randabstand stehen jetzt, wo vorher `track_width_unknown`
/// stand; (3) `finalize_landing_score_if_due` ist nicht mehr an die
/// Flugphase gebunden, kann also einen Score noch finalisieren, wo die
/// alte, phasengebundene Pruefung nie mehr lief.
///
/// **14 seit v1.7.30**: Der Simulator darf die Buchung nur noch mit einem
/// brauchbaren Typcode schlagen (`sim_core::muster_aufloesen`). Muster,
/// deren Sim-Kennung eine Langform ist — belegt: die iniBuilds A380
/// meldet "A380-800", Buchung A388 (UAE 52, 15.09.2026) — bekommen
/// dadurch Spurweite, Spannweite und eine BEWERTETE Bahndisziplin-Achse,
/// wo vorher `track_width_unknown` stand. Derselbe Flug wird unter
/// v1.7.29 und v1.7.30 also verschieden benotet.
///
/// **15 seit v1.7.32**: Die OFP-Achse rechnet Mehrweg und
/// Durchstartmanoever heraus, bevor sie bewertet (Anlass THY 1068,
/// 16.09.2026: +13,5 % Mehrverbrauch bei 90 NM Umweg = 55 Punkte).
/// Gegengewicht: die Baender sind auf 3/8/15/25 gestrafft. Am Bestand
/// (1118 Fluege) faellt die Abzugsquote von 15,4 % auf 12,3 %, aber die
/// Verteilung verschiebt sich deutlich — derselbe Flug wird unter
/// v1.7.31 und v1.7.32 verschieden benotet.
///
/// **16 seit v1.7.35**: Die OFP-Achse ist ERSATZLOS weg. Anlass DLH 370
/// (17.09.2026, ESSA→EDDM): geplante Hoehe, geplante Route, Gewicht auf
/// 128 kg genau — und 45 Punkte Abzug. Der Anflug kostete das Dreifache
/// des Plans, aber nicht durch den Weg (182 statt 139 NM geplant), sondern
/// durch die Zeit: knapp 16 Minuten unter 8 000 ft. Alle sechs
/// A380-Fluege nach EDDM im Bestand ordnen sich lueckenlos nach genau
/// dieser Zeit. Die Achse mass damit die Dauer der Radarfuehrung — eine
/// Entscheidung der Flugsicherung, nicht des Piloten. Der Sprit wird
/// weiterhin gemessen und angezeigt (`sprit::SpritAuswertung`), aber er
/// ist keine Note mehr. Jeder Flug mit einer frueheren OFP-Abwertung wird
/// unter v1.7.35 also besser benotet als unter v1.7.34; der Goldenset
/// verschiebt sich um bis zu 2 Punkte (91→90, 74→72, 97→96, weil die
/// verbleibenden Achsen die Gewichtung neu unter sich aufteilen).
///
/// **17 seit v1.9.16**: Die Stabilitaetsachse ist an das Anflug-Urteil der
/// Karte gebunden (`landing_scoring::anflug_urteil`): PARTIAL deckelt sie
/// auf 80, UNSTABLE auf 45. Anlass DLH2248 (01.10.2026): Karte PARTIAL,
/// Achse 100 Punkte. Dazu zaehlt die Sinkgrenze erst nach 10 s am Stueck.
/// Altbuchungen werden nicht neu gerechnet.
///
/// **18 seit v1.9.25**: PARTIAL/UNSTABLE deckeln die GESAMTNOTE auf 80 bzw.
/// 45, nicht nur die Achse (Anlass QAF434, 05.10.2026: PARTIAL und trotzdem
/// 96). Altbuchungen werden nicht neu gerechnet.
///
/// **19 seit v1.9.26**: „Schwaechster Teil begrenzt" — gefaehrliches
/// Ereignis (vor der Schwelle, Overrun, neben der Bahn, ab zwei Hopsern)
/// hoechstens 40, ein Teil unter 45 Punkten hoechstens 60, unter 75
/// hoechstens 80. Kein Rueckfall mehr auf die Touchdown-Klasse. Anlass
/// QAF419 (vor der Schwelle, 92) und AIB424 (neben der Bahn, 91).
/// Dazu die Teilnote „Abfangen" (Gewicht 2, ab 50 ft bis zum Aufsetzen,
/// `landing-scoring/src/abfangen.rs`) und das Stable Gate nach Gleitpfad.
///
/// **20 seit v2.0.4**: „Spaet stabilisiert" — bei 1000 ft noch zu schnell
/// oder nicht in Landekonfiguration, ab 500 ft aber stabil: Fahrt und
/// Konfiguration nur „mittel" statt „schlecht" (Anlass RYR73, 06.10.2026:
/// UNSTABLE, Gesamtnote 45 bei sauberer Landung). `anflug_urteil.rs`.
///
/// **21 seit v2.0.5**: Abzug statt Deckel — keine Gesamtnote wird mehr auf
/// eine feste Zahl gesetzt. Vom gewichteten Mittel gehen ab: der groesste
/// Landungs-Abzug (Ueberlast −50, Gefahr −30, hart −25, schwaechster Teil
/// schlecht −10 / mittel −5) plus der Anflug (PARTIAL −5, UNSTABLE −15,
/// nicht gemessen −3). `landing-scoring` `landungs_abzug`/`anflug_abzug`.
/// Anlass: Korpus 60 Tage, 58 gute Landungen nach UNSTABLE alle exakt 45,
/// 14 gefaehrliche Ereignisse alle exakt 40.
///
/// **22 seit v2.0.8**: Schnelle Ausfahrt — eine Ausfahrt, die zwischen
/// Messschwelle (60 kt) und 90 kt beginnt und bis 80 m neben die Bahn
/// fuehrt, beendet die Bewertung am Beginn der Kurve
/// (`AusfahrtKandidat`). Anlass TUA651 (VVTS 25R, 09.10.2026): Kurve in
/// die Schnellabrollbahn bei 64 kt als „Rad neben der Bahn" gewertet.
///
/// **23 seit v2.0.15**: Spaetes Final / Kurvenanflug (`spaetes_final.rs`).
/// Wer erst unter 1000 ft dauerhaft auf der Bahnachse liegt und davor
/// mindestens 20 s daneben war (Circling, Platzrunde, Versatzanflug),
/// bekommt Querneigungs-/V/S-Streuung und Gleitpfad erst ab dem Ausrichten
/// gewertet — dafuer neuer Pruefpunkt `final`: spaetestens 300 ft auf dem
/// Final, darunter hoechstens 10° Querneigung (FSF ALAR 7.1). Anlass:
/// Thomas 13.10.2026, Korpus 1437 Landungen.
///
/// **24 (16.10.2026)**: Sinkgrenze nach Grundgeschwindigkeit, nur lockerer
/// (`sinkgrenze_fpm`, `SINK_GS_BEZUG_KT`): ab 145 kt ueber Grund waechst
/// die Grenze −1000 fpm (× Gleitwinkel-Faktor) im Verhaeltnis mit — wer mit
/// Rueckenwind schneller ueber Grund fliegt, muss staerker sinken. Gilt fuer
/// das 1000-ft-Gate und das DA-Gate. Dazu `approach_wind` (Wind im Gate,
/// Soll-Sinkrate, angewandte Grenze, laengster Sinklauf) als Anzeige.
/// Anlass: JBU1500 MDPC→MUVR (Joel), Korpus 1068 Anfluege: 0 neu mit
/// Abzug, 23 ohne. Nur neue Fluege — Altbuchungen bleiben stehen.
/// Bewusst NICHT nachgezogen: das Live-Band (`StableApproachBanner.tsx`)
/// warnt weiter ab −1000 fpm × Faktor — eine Warnung im Flug darf
/// vorsichtiger sein als das Urteil danach. Die Anfluggrafik zeichnet die
/// angewandte Grenze aus `approach_wind.sinkgrenze_fpm`.
///
/// **25 (16.10.2026)**: ILS-Gleitpfad gegen die unveraenderte Navigraph-Bahn
/// (`anflug_forensik_bahn`, `IlsNavigraph`). Der Schwellenversatz der Sim-
/// Szenerie verschob vorher das Navigraph-ILS mit: Sven, RJAA 34L (MSFS-
/// Default 751,6 m gemalt, Navigraph 0, Gleitwegsender 329 m hinter dem
/// Bahnanfang) flog das ILS auf 0,13 Dots und galt als 1,8 Dots zu tief
/// (Gate „mittel“, PARTIAL, −5). Korpus: 8 von 223 ILS-Landungen verschoben
/// gerechnet, nur diese im Urteil. Ohne ILS bleibt die Szenerie-Schwelle;
/// Aufsetzzone und Bahndisziplin rechnen weiter gegen die Szenerie.
///
/// Der Waechter `die_algorithmusversion_steht_an_allen_stellen` haelt
/// fest, dass alle Nutzlaststellen dieselbe Zahl schreiben.
///
/// 26 (P1/8b, 22.10.2026): leichter Hopser (>= 1 s ohne Bodenkontakt,
/// 2 bis unter 5 ft ueber Bodenhoehe, mit zweitem Kontakt) gibt in der
/// Teilnote „Hopser“ 85 (einer) bzw. 70 (zwei und mehr); mit einem
/// normalen Hopser gilt die alte Tabelle. Altbestand wird nicht neu gerechnet.
/// Ebenfalls 26 (B5b, Thomas 22.10.2026, dasselbe Release): War der
/// Kurswechsel ein Schlenker zur anderen Seite als der endgueltige
/// Kantenuebertritt, endet `scoring_cutoff_m` am Uebertritt (siehe
/// `bahn_felder`).
///
/// 27 (3.0, P-4.50): erste Fassung im WASM-Bewertungsmodul; dieselbe Zahl
/// steht im Modul (`landing_scoring::wasm::FASSUNG`) und kommt mit jeder
/// Ausgabe — Test `bewertung::tests::fassung_gleich_modul`.
const SCORE_ALGORITHMUS_VERSION: u8 = 27;

/// Die beiden Ziele einer Landung — geplant und eingereicht.
///
/// # Warum ein eigener Typ und nicht zwei `&str`
///
/// Zwei gleichartige Zeichenketten nebeneinander sind vertauschbar, und
/// ein vertauschtes Paar faellt nirgends auf: Der Vergleich ist
/// symmetrisch, das Ergebnis bliebe richtig — bis jemand einen zweiten
/// Wert daraus ableitet. Ausserdem kennen nicht alle Aufrufer beide
/// Werte; `Unbekannt` sagt das ausdruecklich, statt einen leeren String
/// weiterzureichen, der wie ein Ziel aussieht.
#[derive(Debug, Clone, Copy)]
struct Ausweichziel<'a> {
    /// Das Ziel, gegen das der Flug eingereicht wird.
    eingereicht: &'a str,
    /// Das urspruenglich geplante Ziel.
    geplant: &'a str,
}

impl Ausweichziel<'_> {
    /// Wurde ausgewichen?
    ///
    /// ⚠ Als `Option` gefuehrt, weil nicht jeder Aufrufer beide Ziele
    /// kennt (Vorschau, Diagnose, Test). `None` heisst „unbekannt",
    /// nicht „kein Divert" — die Sprit-Achse behandelt beides gleich,
    /// aber der Unterschied gehoert nicht verwischt.
    fn ist_ausweichflug(ziel: Option<Self>) -> Option<bool> {
        ziel.map(|z| !z.eingereicht.trim().eq_ignore_ascii_case(z.geplant.trim()))
    }
}

///
/// # Warum das eine Funktion ist
///
/// Dieser Block stand am 24.08.2026 **viermal** im Quelltext: in
/// `build_pirep_payload`, `compute_aggregate_master_score`,
/// `build_landing_record` und `build_pirep_notes`. Zwölf Felder, in allen
/// vier dieselben, in allen vier dieselben Werte — abgeschrieben.
///
/// Die Kommentare dort sagten es selbst: „muss identische Inputs nutzen
/// wie der echte PIREP-Pfad". Die Absicht war immer EINE Eingabe. Vier
/// Kopien halten diese Absicht nur so lange, wie jemand alle vier
/// mitpflegt — und genau daran ist v1.7.0 gescheitert, an anderer Stelle
/// derselben Art: Die Musterauflösung lag an sieben Stellen mit vier
/// verschiedenen Ketten, und die beiden wichtigsten fragten die Buchung
/// allein.
///
/// Was hier fehlt, fehlt überall; was hier dazukommt, kommt überall an.
/// `tests/musterquelle.rs` hält fest, dass niemand daneben eine eigene
/// Eingabe baut.
fn scoring_eingang(
    stats: &FlightStats,
    muster: Option<&str>,
    ausweichziel: Option<Ausweichziel<'_>>,
    plan_strecke_nm: Option<f64>,
) -> landing_scoring::LandingScoringInput {
    landing_scoring::LandingScoringInput {
        // v0.7.17 (B-015a QS-Fix): Edge-Wert hat Vorrang — siehe
        // `score_basis_vs_fpm()` Doc.
        vs_fpm: score_basis_vs_fpm(stats),
        // v0.20 (QS-Fix, fallback_zero-Score-Bug): beide zusammen gaten.
        peak_g_load: stats.canonical_peak_g_force(),
        scored_g_load: scored_g_fuer_punkte(stats),
        // v0.7.6 P2-B: zentraler Helper statt direkten Read.
        bounce_count: if stats.landung_abdeckung_fehlt.is_some() {
            None
        } else {
            Some(scored_bounce_count_for_score(stats))
        },
        // P1/8b (Score-Version 26): leichte Hopser, gleiche Sperre.
        light_bounce_count: if stats.landung_abdeckung_fehlt.is_some() {
            None
        } else {
            Some(stats.light_bounce_count as u32)
        },
        approach_vs_stddev_fpm: stats.canonical_vs_stddev_fpm(),
        approach_bank_stddev_deg: stats.canonical_bank_stddev_deg(),
        // v1.9.16: Messwerte hinter dem Anflug-Urteil (STABLE/PARTIAL/UNSTABLE).
        // Gerechnet wird NUR in `landing_scoring::anflug_urteil`.
        anflug: anflug_werte(stats),
        rollout_distance_m: stats.rollout_distance_m.map(|m| m as f32),
        planned_burn_kg: stats.planned_burn_kg,
        actual_trip_burn_kg: actual_burn_for_record(stats),
        planned_zfw_kg: stats.planned_zfw_kg,
        planned_tow_kg: stats.planned_tow_kg,
        // v1.7.32: Grundlage der Mehrweg-/Durchstart-Gutschrift auf der
        // OFP-Achse. Gerechnet wird NUR in `landing_scoring::sub_fuel` —
        // hier werden ausschliesslich die Messwerte durchgereicht, damit
        // Client, Server und Webansicht dieselbe Zahl sehen.
        geflogene_strecke_nm: Some(stats.distance_nm as f32).filter(|v| *v > 0.0),
        plan_strecke_nm: plan_strecke_nm.map(|v| v as f32).filter(|v| *v > 0.0),
        durchstarts: stats.go_around_count,
        // ⚠ Nach einem Ausweichflug ist die OFP-Treue nicht bewertbar —
        // es wurde eine ANDERE Strecke geflogen als die geplante. Ein
        // Vergleich gegen den urspruenglichen Verbrauch misst den Umweg,
        // nicht den Piloten.
        //
        // ⚠⚠ NICHT `divert_hint`: Das ist der VERDACHT, nicht die
        // Tatsache. Der Unterschied ist im Haus lange festgehalten
        // (`divert_payload_markers`, v0.19.3) und geht in BEIDE
        // Richtungen schief:
        //
        //   * Ein echter, vom Piloten eingereichter Divert OHNE Hinweis
        //     (der Regelfall bei einem geplanten Ausweichen) wuerde
        //     bewertet — gegen einen Plan, der nie geflogen wurde.
        //   * Ein Verdacht, den der Pilot abgelehnt hat (er reicht als
        //     Planflug ein), wuerde uebersprungen — obwohl genau die
        //     geplante Strecke geflogen wurde.
        //
        // Massgeblich ist dieselbe Regel wie im Nutzlast-Marker:
        // eingereichtes Ziel != geplantes Ziel.
        diverted: Ausweichziel::ist_ausweichflug(ausweichziel),
        // ⚠ Entscheidet, ob eine schraege Rollspur unserer Achse oder dem
        // Piloten angelastet wird. Ist die Geometrie aus der Szenerie
        // bestaetigt, ist eine Landung am Bahnrand genau das, was
        // bewertet werden soll — siehe die Begruendung an der
        // Achsenpruefung in sub_bahndisziplin.
        //
        // ⚠ P-4.14 (b): Nur, wenn die ACHSE der gelandeten Bahn aus der
        // Szenerie stammt. Bis 3.0 genuegte irgendeine uebernommene Bahn —
        // auch wenn nur ihre Breite kam (MSFS) —, und damit war das Netz
        // `achse_fragwuerdig` gerade dann aus, wenn die Achse Navigraph war
        // (CFG2001, FQNP 23). Ein OurAirports-Rueckfall rechnet nie gegen
        // die Szenerie-Achse.
        bahn_geometrie_aus_szenerie: Some(
            matches!(stats.runway_source, Some(runway::RunwaySource::Navigraph))
                && match (stats.szenerie_uebernahme.as_ref(), stats.runway_match.as_ref()) {
                    (Some(b), Some(rm)) => b.achse_aus_szenerie(&rm.runway_ident),
                    _ => false,
                },
        ),
        // v0.7.17 (N-002): Aircraft-ICAO fuer den aircraft-category-
        // sensitiven Bahn-Score. None → Light-Default-Schwellen
        // (konservativ). Das Muster kommt aus `muster_fuer_landung`,
        // nicht aus der Buchung allein — siehe dort.
        aircraft_icao: muster.map(str::to_string),
        // BCS3317/A306-Folgefund (Vereinheitlichung 09/2026): die Crate
        // selbst faellt bei fehlendem `fahrwerk_spurweite_m` intern auf
        // `spurweite::spurweite_m(aircraft_icao)` zurueck (siehe deren
        // Feld-Doc, QS-Runde 20) — ein DIREKTER Tabellenschlag ohne die
        // Sim-Schreibweisen-Kaskade (`normalize_icao_type`/
        // `muster_kandidaten`), die `aircraft_limits_for` und `bahn_felder`
        // schon nutzen. Baureihen ohne Variante ("A300" statt "A306", z.B.
        // Payware-A300F) fielen dadurch in der BEWERTUNG auf
        // `track_width_unknown`, obwohl die Kaskade "A306" laengst kennt
        // und die ANZEIGE (`bahn_felder`) korrekt war — zwei Antworten auf
        // dieselbe Frage. Hier vorab mit derselben Kaskade aufloesen, damit
        // die Crate ihn als "aus der Datei" vorfindet und ihren eigenen,
        // kaskadenlosen Fallback nie mehr braucht.
        fahrwerk_spurweite_m: stats.fahrwerk_spurweite_m.or_else(|| {
            muster_fuer_typtabelle(muster, |m| landing_scoring::spurweite::spurweite_m(m))
        }),
        // 05.10.2026: Abfangen über die Höhe, gemessen beim Aufsetzen
        // (`abfangen_messen`). Fehlt es (Altbestand), gibt es die Achse nicht.
        abfangen: abfangen_aus_analyse(stats),
        ..Default::default()
    }
}

#[cfg(test)]
thread_local! {
    /// Wie oft `bahn_felder` in diesem Test-Faden lief — fuer Tests, die
    /// belegen, dass die teure Rechnung unterbleibt (AIB 429, 12.10.2026).
    static BAHN_FELDER_AUFRUFE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// Wie oft die Ausfahrten in diesem Test-Faden wirklich GERECHNET
    /// wurden, also nicht aus `ausfahrten_merker` kamen.
    static AUSFAHRTEN_GERECHNET: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Woraus die Ausfahrten kommen: Szenerie-Rollwege oder die Serverkarte.
///
/// Beide GETEILT (`Arc`), nicht geliehen: Wer die Quelle aus den Flugdaten
/// holt, haelt danach einen eigenen Zeiger und rechnet ohne Sperre. Bis
/// 2.0.16 wurden dafuer unter der Sperre alle Rollwege kopiert und gehasht
/// (v2.0.17, AIB 431).
#[derive(Clone)]
enum Bodenquelle {
    Szenerie(Arc<sim_core::szenerie::SzenerieFlughafen>),
    Karte(Arc<str>),
}

impl Bodenquelle {
    /// Dieselbe Karte — am Zeiger erkannt, ohne den Inhalt anzusehen.
    ///
    /// ⚠ Das ist nur richtig, weil der Vergleich IMMER gegen einen Wert
    /// laeuft, der seinen eigenen `Arc` haelt (`AusfahrtenSchluessel` im
    /// Merker): Solange er lebt, kann dieselbe Adresse keinen anderen
    /// Inhalt tragen, und hinter einem geteilten `Arc` aendert niemand die
    /// Daten. Eine neu gelieferte Karte mit gleichem Inhalt gilt als
    /// andere — dann wird einmal zu viel gerechnet, nie falsch.
    fn dieselbe(&self, andere: &Self) -> bool {
        match (self, andere) {
            (Self::Szenerie(a), Self::Szenerie(b)) => Arc::ptr_eq(a, b),
            (Self::Karte(a), Self::Karte(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// Die Bodenkarte zur gematchten Bahn — EINE Auswahl fuer `bahn_felder`
/// und das Vorwaermen (`ausfahrten_vorwaermen`).
///
/// Ohne Landeplatz der Bahn gibt es keine (FAIL CLOSED, siehe
/// `bahn_felder`).
fn bodenquelle(stats: &FlightStats, landeplatz_der_bahn: Option<&str>) -> Option<Bodenquelle> {
    // ZWEI Quellen: die Auskunft des aktuellen Ernteziels und die des
    // geplanten Ziels.
    //
    // Die erste kann von einem Ueberflugplatz stammen — bei DLH 373 am
    // 20.09.2026 korrelierte die Bahn im Sinkflug kurz mit EDML
    // (Landshut, liegt auf der Anflugstrecke nach EDDM), das Ernteziel
    // wechselte dorthin, und die gute EDDM-Auskunft mit 682 Rollwegen
    // wurde verworfen. EDML lieferte 0. Beim Aufsetzen in Muenchen war
    // deshalb keine Bodenkarte da: keine Ausfahrten, keine Bemassung.
    //
    // Die ICAO-Pruefung bleibt an BEIDEN Quellen — sie ist die
    // Sicherung, die verhindert, dass Rollwege eines fremden Platzes in
    // die Ausfahrten geraten. Bei einem Ausweichflug passt das geplante
    // Ziel nicht und faellt heraus.
    let passt = |a: &&Arc<sim_core::szenerie::SzenerieFlughafen>| match landeplatz_der_bahn {
        Some(landeplatz) => a.icao.eq_ignore_ascii_case(landeplatz),
        None => false,
    };
    let szenerie = stats
        .szenerie_auskunft
        .as_ref()
        .filter(passt)
        .filter(|a| !a.rollwege.is_empty())
        .or_else(|| {
            stats
                .ziel_szenerie_auskunft
                .as_ref()
                .filter(passt)
                .filter(|a| !a.rollwege.is_empty())
        })
        .map(|a| Bodenquelle::Szenerie(Arc::clone(a)));
    let arr_karte = match (&stats.arr_ground_geojson, landeplatz_der_bahn) {
        (Some(karte), Some(landeplatz)) => stats
            .arr_ground_geojson_icao
            .as_deref()
            .filter(|gi| gi.eq_ignore_ascii_case(landeplatz))
            .map(|_| Bodenquelle::Karte(Arc::clone(karte))),
        _ => None,
    };
    szenerie.or(arr_karte)
}

/// Der Schluessel des Merkers: Bodenkarte und Bahn.
///
/// ⚠ Die Karte steckt SELBST im Schluessel (siehe `Bodenquelle::dieselbe`):
/// Ein Stand, der nicht mitgezaehlt wurde, oder ein Wechsel zwischen
/// `szenerie_auskunft` und `ziel_szenerie_auskunft` kann so nie Ausfahrten
/// einer anderen Karte liefern — schlimmstenfalls wird einmal zu viel
/// gerechnet.
///
/// Bis 2.0.16 war der Schluessel ein Hash ueber den INHALT, also ueber jeden
/// Stuetzpunkt jedes Rollwegs — unter der Flugdaten-Sperre, bei jedem
/// `bahn_felder` und zweimal je Vorwaermen (v2.0.17, AIB 431, Dubai mit
/// 3137 Rollwegen).
#[derive(Clone)]
struct AusfahrtenSchluessel {
    quelle: Bodenquelle,
    bahn: [u64; 5],
}

impl AusfahrtenSchluessel {
    fn neu(quelle: Bodenquelle, m: &runway::RunwayMatch) -> Self {
        Self {
            quelle,
            bahn: [
                m.threshold_lat.to_bits(),
                m.threshold_lon.to_bits(),
                m.end_lat.to_bits(),
                m.end_lon.to_bits(),
                u64::from(m.width_ft.to_bits()),
            ],
        }
    }

    fn gleich(&self, anderer: &Self) -> bool {
        self.bahn == anderer.bahn && self.quelle.dieselbe(&anderer.quelle)
    }
}

/// Die Ausfahrten zu Bodenkarte und Bahn, frisch gerechnet.
///
/// Das Teure: Szenerie-Rollwege als GeoJSON formatieren, wieder parsen,
/// jeden Stuetzpunkt auf die Bahn projizieren — bei HECA 1480 Rollwege.
fn ausfahrten_rechnen(quelle: &Bodenquelle, m: &runway::RunwayMatch) -> Vec<ausfahrten::Ausfahrt> {
    #[cfg(test)]
    AUSFAHRTEN_GERECHNET.with(|z| z.set(z.get() + 1));
    let szenerie_karte;
    let karte = match quelle {
        Bodenquelle::Szenerie(sz) => {
            szenerie_karte = szenerie_bahn::rollwege_als_bodenkarte(&sz.rollwege);
            szenerie_karte.as_str()
        }
        Bodenquelle::Karte(karte) => karte,
    };
    ausfahrten::ausfahrten_fuer_bahn(
        karte,
        m.threshold_lat,
        m.threshold_lon,
        m.end_lat,
        m.end_lon,
        m.width_ft as f64 * 0.3048,
    )
}

/// Die Ausfahrten zu Bodenkarte und Bahn — einmal gerechnet, danach
/// aus `ausfahrten_merker`.
///
/// # Warum (AIB 429, 12.10.2026)
///
/// `bahn_felder` laeuft je Landung viele Male: Aufsetz-Ereignis,
/// Rollout-Nachtrag, Landungen-Reiter alle 5 s, X-Plane-HUD-Band, Panel.
/// Jedes Mal entstanden die Ausfahrten neu, obwohl Karte und Bahn
/// dieselben waren. Sie sind eine reine Funktion beider; der Merker
/// gibt eine KOPIE heraus, das Einkuerzen der Verlaeufe in
/// `bahn_felder` aendert ihn nicht.
fn ausfahrten_gemerkt(
    stats: &FlightStats,
    quelle: Bodenquelle,
    m: &runway::RunwayMatch,
) -> Vec<ausfahrten::Ausfahrt> {
    let schluessel = AusfahrtenSchluessel::neu(quelle, m);
    if let Ok(merker) = stats.ausfahrten_merker.lock() {
        if let Some((k, gemerkt)) = merker.as_ref() {
            if k.gleich(&schluessel) {
                return gemerkt.clone();
            }
        }
    }
    let gerechnet = ausfahrten_rechnen(&schluessel.quelle, m);
    if let Ok(mut merker) = stats.ausfahrten_merker.lock() {
        *merker = Some((schluessel, gerechnet.clone()));
    }
    gerechnet
}

/// Was sich aendern muss, damit eine andere Bodenkarte oder Bahn
/// vorliegen kann — der billige Ausloeser von `ausfahrten_vorwaermen`.
type VorwaermAusloeser = (u32, u32, u32, Option<String>, usize, Option<[u64; 5]>);

fn vorwaerm_ausloeser(stats: &FlightStats) -> VorwaermAusloeser {
    (
        stats.bahn_revision,
        stats.szenerie_auskunft_stand,
        stats.ziel_szenerie_auskunft_stand,
        stats.arr_ground_geojson_icao.clone(),
        stats.arr_ground_geojson.as_ref().map_or(0, |k| k.len()),
        stats.runway_match.as_ref().map(|m| {
            [
                m.threshold_lat.to_bits(),
                m.threshold_lon.to_bits(),
                m.end_lat.to_bits(),
                m.end_lon.to_bits(),
                u64::from(m.width_ft.to_bits()),
            ]
        }),
    )
}

/// Die Ausfahrten vorwaermen — AUSSERHALB der Sperre der Flugdaten.
///
/// # Warum (AIB 429, 12.10.2026)
///
/// Das erste Rechnen je Karte und Bahn ist teuer (siehe
/// `ausfahrten_rechnen`) und lief bisher in `bahn_felder`, also unter
/// `flight.stats` — waehrenddessen wartet der Aufsetz-Abtaster. Hier wird
/// unter kurzer Sperre nur geprueft, ob der Merker schon passt; wenn
/// nicht, werden Karte und Bahn KOPIERT, ohne Sperre gerechnet und das
/// Ergebnis kurz gesperrt abgelegt. `bahn_felder` findet es danach im
/// Merker — dieselbe Auswahl (`bodenquelle`), derselbe Schluessel.
///
/// `zuletzt` ist der Ausloeser des vorigen Takts: Solange er gleich
/// bleibt, wird nicht einmal der Fingerabdruck gebildet. Verpasst er eine
/// Aenderung, rechnet `bahn_felder` wie bisher selbst — langsamer, nie
/// falsch.
#[cfg(test)]
fn ausfahrten_vorwaermen(flight: &ActiveFlight, zuletzt: &mut Option<VorwaermAusloeser>) {
    if let Some(auftrag) = vorwaerm_auftrag(flight, zuletzt) {
        vorwaerm_ausfuehren(flight, auftrag);
    }
}

/// Was das Vorwaermen rechnen soll — Schluessel (mit geteilter Karte) und
/// Bahn.
struct VorwaermAuftrag {
    schluessel: AusfahrtenSchluessel,
    bahn: runway::RunwayMatch,
}

/// Der billige Teil des Vorwaermens, unter kurzer Sperre: Ausloeser
/// pruefen, Merker pruefen, Karte (geteilt, ohne Kopie) und Bahn nehmen.
fn vorwaerm_auftrag(
    flight: &ActiveFlight,
    zuletzt: &mut Option<VorwaermAusloeser>,
) -> Option<VorwaermAuftrag> {
    let stats = flight.stats.lock().ok()?;
    let ausloeser = vorwaerm_ausloeser(&stats);
    if zuletzt.as_ref() == Some(&ausloeser) {
        return None;
    }
    *zuletzt = Some(ausloeser);
    let m = stats.runway_match.as_ref().filter(|m| m.width_ft > 0.0)?;
    let quelle = bodenquelle(&stats, Some(m.airport_ident.as_str()))?;
    let schluessel = AusfahrtenSchluessel::neu(quelle, m);
    let schon_da = stats
        .ausfahrten_merker
        .lock()
        .map(|g| g.as_ref().is_some_and(|(k, _)| k.gleich(&schluessel)))
        .unwrap_or(false);
    if schon_da {
        return None;
    }
    Some(VorwaermAuftrag {
        schluessel,
        bahn: m.clone(),
    })
}

/// Der teure Teil: ohne Sperre rechnen, kurz gesperrt ablegen.
fn vorwaerm_ausfuehren(flight: &ActiveFlight, auftrag: VorwaermAuftrag) {
    let beginn = std::time::Instant::now();
    let rollwege = match &auftrag.schluessel.quelle {
        Bodenquelle::Szenerie(sz) => Some(sz.rollwege.len()),
        Bodenquelle::Karte(_) => None,
    };
    let gerechnet = ausfahrten_rechnen(&auftrag.schluessel.quelle, &auftrag.bahn);
    tracing::info!(
        pirep_id = %flight.pirep_id,
        platz = %auftrag.bahn.airport_ident,
        bahn = %auftrag.bahn.runway_ident,
        szenerie_rollwege = ?rollwege,
        ausfahrten = gerechnet.len(),
        dauer_ms = beginn.elapsed().as_millis() as u64,
        "Ausfahrten vorgewaermt"
    );
    // ⚠ Nur ablegen, wenn Karte und Bahn noch dieselben sind. Hat sich
    // waehrend des Rechnens etwas geaendert, kann `bahn_felder` den Merker
    // schon mit dem NEUEN Schluessel gefuellt haben — ein veraltetes
    // Ergebnis darueber zwaenge es, erneut unter der Sperre zu rechnen
    // (Codex, PR #138).
    if let Ok(stats) = flight.stats.lock() {
        let aktuell = stats
            .runway_match
            .as_ref()
            .filter(|m| m.width_ft > 0.0)
            .and_then(|m| {
                bodenquelle(&stats, Some(m.airport_ident.as_str()))
                    .map(|q| AusfahrtenSchluessel::neu(q, m))
            });
        if aktuell.is_some_and(|k| k.gleich(&auftrag.schluessel)) {
            if let Ok(mut merker) = stats.ausfahrten_merker.lock() {
                *merker = Some((auftrag.schluessel, gerechnet));
            }
        }
    }
}

/// Das Vorwaermen im EIGENEN Faden — der Streamer-Takt wartet nicht.
///
/// # Warum (AIB 430, 13.10.2026)
///
/// Seit 2.0.13 lief das Rechnen zwar ohne Sperre, aber noch im
/// Streamer-Takt: in Mumbai 299 ms von 341 ms des Takts direkt nach dem
/// Aufsetzen. Jetzt nimmt der Takt nur den Auftrag (kurz gesperrt) und
/// uebergibt ihn einem Faden.
///
/// Immer nur EIN Faden: Laeuft noch einer, bleibt `zuletzt` unberuehrt,
/// und der naechste Takt fragt erneut — eine Aenderung waehrend des
/// Rechnens (neue Bahn, spaete Szenerie) geht so nicht verloren.
///
/// Fragt `bahn_felder` in den ~300 ms, bevor das Ergebnis liegt, rechnet
/// es wie bisher selbst — langsamer, nie falsch.
fn ausfahrten_im_hintergrund(
    flight: &Arc<ActiveFlight>,
    zuletzt: &mut Option<VorwaermAusloeser>,
    laeufer: &mut Option<std::thread::JoinHandle<()>>,
) {
    if laeufer.as_ref().is_some_and(|h| !h.is_finished()) {
        return;
    }
    *laeufer = None;
    let Some(auftrag) = vorwaerm_auftrag(flight, zuletzt) else {
        return;
    };
    let f = Arc::clone(flight);
    match std::thread::Builder::new()
        .name("ausfahrten-vorwaermen".into())
        .spawn(move || vorwaerm_ausfuehren(&f, auftrag))
    {
        Ok(h) => *laeufer = Some(h),
        Err(e) => tracing::warn!(
            pirep_id = %flight.pirep_id,
            error = %e,
            "Ausfahrten-Faden nicht startbar — bahn_felder rechnet selbst"
        ),
    }
}
