    use super::*;

    fn szenerie_stand(
        icao: &str,
        name: &str,
        lat: f64,
        lon: f64,
    ) -> sim_core::szenerie::SzenerieFlughafen {
        sim_core::szenerie::SzenerieFlughafen {
            icao: icao.to_string(),
            bahnen: Vec::new(),
            rollwege: Vec::new(),
            staende: vec![sim_core::szenerie::SzenerieStand {
                name: Some(name.to_string()),
                lat,
                lon,
            }],
            quelle: "test".to_string(),
        }
    }

    fn osm_stand(name: &str) -> stands::ParkingStand {
        stands::ParkingStand {
            name: Some(name.to_string()),
            lat: 1.0,
            lon: 1.0,
            linie: None,
            flaeche: false,
        }
    }

    #[test]
    fn szenerie_gewinnt_gegen_osm_bei_gleichem_flughafen() {
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(szenerie_stand("EDDF", "V168", 50.0, 8.0));
        let osm = vec![osm_stand("OSM-STAND")];
        let liste = standliste_fuer(&stats, "EDDF", Some(&osm), None).expect("Liste da");
        assert_eq!(liste.len(), 1);
        assert_eq!(
            liste[0].name.as_deref(),
            Some("V168"),
            "Szenerie muss OSM schlagen"
        );
    }

    /// ELY 382, LIMC→LLBG, 07.09.2026: die Szenerie-Ernte hatte einen
    /// Rampenstart fuer LLBG (also nicht-leer, `treffer` griff), aber weit
    /// weg vom tatsaechlichen Parkplatz. OSMs naechster benannter Stand
    /// lag nur wenige Meter entfernt — wurde aber nie angefragt, weil die
    /// Szenerie schon "irgendetwas" fuer den ICAO hatte. Mit Position
    /// muss jetzt OSM gewinnen.
    #[test]
    fn szenerie_weit_weg_faellt_mit_position_auf_naeheres_osm_zurueck() {
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(szenerie_stand("LLBG", "WEIT-WEG", 0.01, 0.0));
        let osm = vec![stands::ParkingStand {
            name: Some("C6".to_string()),
            lat: 0.0,
            lon: 0.0,
            linie: None,
            flaeche: false,
        }];
        let liste = standliste_fuer(&stats, "LLBG", Some(&osm), Some((0.0, 0.0)))
            .expect("OSM als naeheres Netz");
        assert_eq!(
            liste[0].name.as_deref(),
            Some("C6"),
            "die weit entfernte Szenerie-Auskunft darf das nahe OSM nicht blockieren"
        );
    }

    /// Gegenprobe: hat die Szenerie tatsaechlich etwas IN DER NAEHE,
    /// gewinnt sie weiterhin — keine Abkehr von der v1.7.16-Entscheidung
    /// "Sim vor OSM".
    #[test]
    fn szenerie_in_der_naehe_gewinnt_weiterhin_mit_position() {
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(szenerie_stand("LLBG", "C6-SIM", 0.0, 0.0));
        let osm = vec![stands::ParkingStand {
            name: Some("C6-OSM".to_string()),
            lat: 0.0,
            lon: 0.0,
            linie: None,
            flaeche: false,
        }];
        let liste =
            standliste_fuer(&stats, "LLBG", Some(&osm), Some((0.0, 0.0))).expect("Liste da");
        assert_eq!(
            liste[0].name.as_deref(),
            Some("C6-SIM"),
            "eine Szenerie mit echtem Nahtreffer muss weiterhin OSM schlagen"
        );
    }

    #[test]
    fn osm_bleibt_das_netz_ohne_szenerie() {
        let stats = FlightStats::default();
        let osm = vec![osm_stand("OSM-STAND")];
        let liste = standliste_fuer(&stats, "EDDF", Some(&osm), None).expect("OSM als Rueckfall");
        assert_eq!(liste[0].name.as_deref(), Some("OSM-STAND"));
    }

    #[test]
    fn nichts_wenn_keine_der_beiden_quellen_etwas_hat() {
        let stats = FlightStats::default();
        assert!(standliste_fuer(&stats, "EDDF", None, None).is_none());
    }

    /// ⚠ Der eigentliche Zweck dieses Tests: eine Szenerie-Auskunft für
    /// einen ANDEREN Flughafen darf nie einspringen — sonst bekäme ein
    /// Abflug-Stand die Rampen des Ziels (oder umgekehrt), sobald die
    /// Bahnzuordnung längst zum nächsten Platz weitergezogen ist.
    #[test]
    fn veraltete_szenerie_auskunft_eines_anderen_flughafens_wird_ignoriert() {
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(szenerie_stand("EDNY", "203", 47.0, 9.0));
        let osm = vec![osm_stand("OSM-EDDF-STAND")];
        let liste = standliste_fuer(&stats, "EDDF", Some(&osm), None).expect("OSM als Rueckfall");
        assert_eq!(
            liste[0].name.as_deref(),
            Some("OSM-EDDF-STAND"),
            "die EDNY-Auskunft ist fuer EDDF nicht zustaendig"
        );
    }

    #[test]
    fn icao_vergleich_ignoriert_gross_klein_und_leerzeichen() {
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(szenerie_stand(" eddf ", "V168", 50.0, 8.0));
        let liste = standliste_fuer(&stats, "eddf", None, None).expect("Liste da");
        assert_eq!(liste[0].name.as_deref(), Some("V168"));
    }

    #[test]
    fn leere_szenerie_staendeliste_faellt_auf_osm_zurueck() {
        // Die Bahnzuordnung kann fuer EDDF stehen, ohne dass die Szenerie
        // ueberhaupt Staende kennt (z. B. eine sehr alte apt.dat ohne
        // 1300-Zeilen) — dann zaehlt OSM, nicht eine leere Liste.
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
            icao: "EDDF".to_string(),
            bahnen: Vec::new(),
            rollwege: Vec::new(),
            staende: Vec::new(),
            quelle: "test".to_string(),
        });
        let osm = vec![osm_stand("OSM-STAND")];
        let liste = standliste_fuer(&stats, "EDDF", Some(&osm), None).expect("OSM als Rueckfall");
        assert_eq!(liste[0].name.as_deref(), Some("OSM-STAND"));
    }

    /// v1.7.17: `dep_szenerie_auskunft` ist eine zusaetzliche Quelle,
    /// unabhaengig von `szenerie_auskunft` — der eigentliche Zweck der
    /// Abflug-Ernte ueberhaupt.
    #[test]
    fn dep_szenerie_auskunft_wird_als_quelle_erkannt() {
        let mut stats = FlightStats::default();
        stats.dep_szenerie_auskunft = arc_some(szenerie_stand("EDDS", "V42", 48.0, 9.0));
        let liste = standliste_fuer(&stats, "EDDS", None, None).expect("Abflug-Szenerie greift");
        assert_eq!(liste[0].name.as_deref(), Some("V42"));
    }

    /// Beide Felder gleichzeitig, fuer VERSCHIEDENE Flughaefen — jede
    /// Anfrage muss NUR die zu ihr passende treffen, nie die andere.
    #[test]
    fn dep_und_ziel_szenerie_bleiben_getrennt() {
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(szenerie_stand("LEPA", "42", 39.5, 2.7));
        stats.dep_szenerie_auskunft = arc_some(szenerie_stand("EDDS", "V42", 48.0, 9.0));

        let abflug = standliste_fuer(&stats, "EDDS", None, None).expect("Abflug-Treffer");
        assert_eq!(abflug[0].name.as_deref(), Some("V42"));

        let ziel = standliste_fuer(&stats, "LEPA", None, None).expect("Ziel-Treffer");
        assert_eq!(ziel[0].name.as_deref(), Some("42"));
    }

    /// `dep_szenerie_auskunft` fuer den FALSCHEN Platz darf so wenig
    /// einspringen wie `szenerie_auskunft` fuer den falschen Platz —
    /// dieselbe Zusicherung wie
    /// `veraltete_szenerie_auskunft_eines_anderen_flughafens_wird_ignoriert`,
    /// jetzt fuer das neue Feld.
    #[test]
    fn dep_szenerie_auskunft_eines_anderen_flughafens_wird_ignoriert() {
        let mut stats = FlightStats::default();
        stats.dep_szenerie_auskunft = arc_some(szenerie_stand("EDDS", "V42", 48.0, 9.0));
        let osm = vec![osm_stand("OSM-EDDM-STAND")];
        let liste = standliste_fuer(&stats, "EDDM", Some(&osm), None).expect("OSM als Rueckfall");
        assert_eq!(
            liste[0].name.as_deref(),
            Some("OSM-EDDM-STAND"),
            "die EDDS-Abflug-Auskunft ist fuer EDDM nicht zustaendig"
        );
    }

    /// Eine leere Staende-Liste in `dep_szenerie_auskunft` faellt auf OSM
    /// zurueck — dieselbe Regel wie bei `szenerie_auskunft`.
    #[test]
    fn leere_dep_szenerie_staendeliste_faellt_auf_osm_zurueck() {
        let mut stats = FlightStats::default();
        stats.dep_szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
            icao: "EDDS".to_string(),
            bahnen: Vec::new(),
            rollwege: Vec::new(),
            staende: Vec::new(),
            quelle: "test".to_string(),
        });
        let osm = vec![osm_stand("OSM-STAND")];
        let liste = standliste_fuer(&stats, "EDDS", Some(&osm), None).expect("OSM als Rueckfall");
        assert_eq!(liste[0].name.as_deref(), Some("OSM-STAND"));
    }

    #[test]
    fn retry_ziel_nichts_ohne_angefragtes_icao() {
        let stats = FlightStats::default();
        assert!(arrival_retry_ziel(&stats).is_none());
    }

    /// Der eigentliche Gegenprobe-Fall: OSM ist NIE erfolgreich gewesen
    /// (`arr_stands_icao` leer, `arr_stands` leer) — trotzdem muss die
    /// Funktion das angefragte ICAO liefern, damit `standliste_fuer` die
    /// Szenerie noch pruefen kann.
    #[test]
    fn retry_ziel_liefert_icao_auch_wenn_osm_nie_geliefert_hat() {
        let mut stats = FlightStats::default();
        stats.arr_stands_requested_icao = Some("EDDF".to_string());
        let (icao, osm) = arrival_retry_ziel(&stats).expect("ICAO trotz fehlendem OSM");
        assert_eq!(icao, "EDDF");
        assert!(
            osm.is_none(),
            "keine OSM-Liste vorhanden — muss None bleiben"
        );
    }

    #[test]
    fn retry_ziel_reicht_osm_durch_wenn_es_zum_selben_icao_gehoert() {
        let mut stats = FlightStats::default();
        stats.arr_stands_requested_icao = Some("EDDF".to_string());
        stats.arr_stands_icao = Some("EDDF".to_string());
        stats.arr_stands = Some(vec![osm_stand("OSM-STAND")]);
        let (icao, osm) = arrival_retry_ziel(&stats).expect("ICAO da");
        assert_eq!(icao, "EDDF");
        assert_eq!(osm.expect("OSM da")[0].name.as_deref(), Some("OSM-STAND"));
    }

    /// Divert-Fall: `arr_stands` traegt noch die Liste des ALTEN Ziels
    /// (EDNY), waehrend `arr_stands_requested_icao` schon das neue (EDDF)
    /// fordert — die veraltete OSM-Liste darf NICHT als EDDF-Liste
    /// durchgereicht werden.
    #[test]
    fn retry_ziel_verwirft_osm_eines_anderen_angefragten_flughafens() {
        let mut stats = FlightStats::default();
        stats.arr_stands_requested_icao = Some("EDDF".to_string());
        stats.arr_stands_icao = Some("EDNY".to_string());
        stats.arr_stands = Some(vec![osm_stand("EDNY-STAND")]);
        let (icao, osm) = arrival_retry_ziel(&stats).expect("ICAO da");
        assert_eq!(icao, "EDDF");
        assert!(
            osm.is_none(),
            "die EDNY-Liste ist fuer das angefragte EDDF nicht zustaendig"
        );
    }

    /// Der Runde-2-Fall (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): ein Divert, der NIE einen `divert_hint` mintet (normale
    /// TaxiIn→BlocksOn-Kante, kein Arrived-Fallback noetig). Dann bleibt
    /// `arr_stands_requested_icao` dauerhaft beim GEPLANTEN Platz (EDNY),
    /// waehrend `arr_actual_icao` — von `settle_at_arrival_stand` aus der
    /// echten Position berechnet — bereits den WIRKLICHEN Platz (EDDF)
    /// kennt. `arr_actual_icao` MUSS gewinnen.
    #[test]
    fn retry_ziel_bevorzugt_arr_actual_icao_vor_dem_angefragten_icao() {
        let mut stats = FlightStats::default();
        stats.arr_stands_requested_icao = Some("EDNY".to_string()); // geplant, nie korrigiert
        stats.arr_actual_icao = Some("EDDF".to_string()); // wirklich dort gelandet
        let (icao, _osm) = arrival_retry_ziel(&stats).expect("ICAO da");
        assert_eq!(
            icao, "EDDF",
            "arr_actual_icao muss arr_stands_requested_icao schlagen"
        );
    }

    #[test]
    fn retry_ziel_faellt_auf_angefragtes_icao_zurueck_ohne_arr_actual_icao() {
        // settle_at_arrival_stand lief noch nie (arr_actual_icao leer) —
        // reine Verteidigung, sollte praktisch nicht vorkommen, siehe Doku
        // an arrival_retry_ziel.
        let mut stats = FlightStats::default();
        stats.arr_stands_requested_icao = Some("EDDF".to_string());
        let (icao, _osm) = arrival_retry_ziel(&stats).expect("ICAO da");
        assert_eq!(icao, "EDDF");
    }

    fn hint_fuer(actual: &str) -> crate::arrival::DivertHint {
        crate::arrival::DivertHint::from_site(
            &crate::arrival::ArrivalSite::AtOtherAirport {
                icao: actual.to_string(),
                distance_from_planned_nm: 120.0,
            },
            "EDDF",
            None,
        )
        .expect("non-planned site yields a hint")
    }

    #[test]
    fn fetch_ziel_bevorzugt_arr_actual_icao_vor_divert_hint_und_geplant() {
        let mut stats = FlightStats::default();
        stats.arr_actual_icao = Some("LEMG".to_string());
        stats.divert_hint = Some(hint_fuer("EDNY"));
        assert_eq!(arrival_fetch_ziel_icao(&stats, "EDDF"), "LEMG");
    }

    /// Der Runde-3-Fall: `arr_actual_icao` fehlt noch (kein Aufruf von
    /// `settle_at_arrival_stand` bisher), aber `divert_hint` ist schon
    /// gemintet — dann gilt weiterhin `divert_hint`, nicht der geplante
    /// Flughafen.
    #[test]
    fn fetch_ziel_faellt_auf_divert_hint_zurueck_ohne_arr_actual_icao() {
        let mut stats = FlightStats::default();
        stats.divert_hint = Some(hint_fuer("EDNY"));
        assert_eq!(arrival_fetch_ziel_icao(&stats, "EDDF"), "EDNY");
    }

    #[test]
    fn fetch_ziel_faellt_auf_geplanten_flughafen_zurueck_ohne_beides() {
        let stats = FlightStats::default();
        assert_eq!(arrival_fetch_ziel_icao(&stats, "EDDF"), "EDDF");
    }

    #[test]
    fn passt_zum_ziel_wenn_icao_uebereinstimmt() {
        let mut stats = FlightStats::default();
        stats.arr_stands_requested_icao = Some("EDDF".to_string());
        assert!(passt_zum_angefragten_ankunftsziel(&stats, "EDDF"));
    }

    /// Der eigentliche Runde-4-Fall (extern Gegenpruefung, Codex,
    /// adversarial, 04.09.2026): eine Antwort/ein Zaehler fuer den ALTEN
    /// (geplanten) Platz darf nach einem Zielwechsel nicht mehr als
    /// „passend" gelten — weder fuer die Standliste noch fuer die
    /// Bodenkarte noch fuer den Zaehler-Reset.
    #[test]
    fn passt_nicht_zum_ziel_nach_einem_wechsel() {
        let mut stats = FlightStats::default();
        stats.arr_stands_requested_icao = Some("EDNY".to_string()); // alt, geplant
        assert!(
            !passt_zum_angefragten_ankunftsziel(&stats, "EDDF"), // neu, tatsaechlich
            "eine Antwort fuer EDNY darf nicht als Antwort fuer EDDF durchgehen"
        );
    }

    #[test]
    fn passt_nicht_zum_ziel_ohne_je_eine_anfrage() {
        let stats = FlightStats::default();
        assert!(!passt_zum_angefragten_ankunftsziel(&stats, "EDDF"));
    }

    /// v1.7.17: Text-Waechter — `dep_szenerie_auskunft_uebernehmen` muss
    /// tatsaechlich aus dem Haupt-Tick heraus aufgerufen werden. Die Logik
    /// selbst (`standliste_fuer`) ist oben verhaltensgeprueft; hier geht
    /// es nur um die VERDRAHTUNG, die eine Verhaltensprobe nicht sehen
    /// kann, weil der Aufrufer (`step_flight`) tief in async/Tauri-Code
    /// haengt und nicht isoliert ausfuehrbar ist — derselbe Grund, aus dem
    /// `maybe_spawn_stand_fetch` schon ohne eigene Aufrufstellen-Probe
    /// auskommen musste.
    #[test]
    fn dep_szenerie_auskunft_uebernehmen_wird_im_tick_aufgerufen() {
        // ⚠ NICHT den vollen Aufruf als EIN Literal suchen — dieser Test
        // selbst nennt ihn (in dieser Zeilendoku), und `include_str!`
        // liest auch die eigene Datei. Ein Literal, das den Aufruf exakt
        // nachbildet, faende sich selbst und bliebe gruen, auch wenn der
        // echte Aufruf im Tick verschwaende. Deshalb: Vorkommen ZAEHLEN —
        // eines ist immer dieser Kommentar/die Doku an der Funktion,
        // mindestens ein zweites muss der echte Aufruf sein.
        // v3 (Etappe 0): Dieser Test liegt jetzt in `tests_lib/`, `lib.rs`
        // enthaelt ihn nicht mehr. Gezaehlt werden nur Code-Zeilen ohne
        // Kommentar — mindestens eine muss der echte Aufruf sein.
        let quelle = crate::quelle_gesamt::PRODUKTION;
        let vorkommen: usize = quelle
            .lines()
            .filter(|z| !z.trim_start().starts_with("//"))
            .map(|z| z.matches("dep_szenerie_auskunft_uebernehmen(&app, &flight, snap.simulator)").count())
            .sum();
        assert!(
            vorkommen >= 1,
            "die Abflug-Ernte haengt an keinem Tick mehr (kein Aufruf ausserhalb von \
             Kommentaren in lib.rs)"
        );
    }

    /// v1.7.17 Runde 1: Text-Waechter — die ZIEL-Ernte muss die
    /// GESCHUETZTE Variante von `szenerie_anflug_ausrichten` aufrufen,
    /// nicht die ungeschuetzte. Dieselbe Selbsttreffer-Falle wie oben:
    /// dieser Kommentar nennt den Funktionsnamen, also zaehlt
    /// `include_str!` ihn mit — Vorkommen zaehlen (`>= 2`), nicht nur
    /// Existenz prüfen.
    #[test]
    fn ziel_ernte_ruft_die_geschuetzte_ausrichtung_auf() {
        // v3 (Etappe 0): nur Code-Zeilen ohne Kommentar zaehlen (siehe oben).
        let quelle = crate::quelle_gesamt::PRODUKTION;
        let vorkommen: usize = quelle
            .lines()
            .filter(|z| !z.trim_start().starts_with("//"))
            .map(|z| {
                z.matches("szenerie_anflug_ausrichten_mit_schutz(&raenge, &geschuetzt, fenster_faellig)")
                    .count()
            })
            .sum();
        assert!(
            vorkommen >= 1,
            "die Ziel-Ernte ruft nicht mehr die geschuetzte Ausrichtung auf (kein Aufruf \
             ausserhalb von Kommentaren in lib.rs) — der Abflugplatz waere wieder \
             ungeschuetzt gegen Rang-Reset und vorzeitige Freigabe"
        );
    }

    /// v1.7.17 Runde 2: nur waehrend des Boardings darf ein Verbindungswechsel
    /// einen bereits abgeleiteten `dep_gate` mitsamt Post-Riegel loeschen —
    /// siehe Begruendung an `dep_gate_bei_szenerie_wechsel_zuruecksetzen`.
    #[test]
    fn dep_gate_wird_nur_waehrend_boarding_zurueckgesetzt() {
        assert!(dep_gate_bei_szenerie_wechsel_zuruecksetzen(
            FlightPhase::Boarding
        ));
    }

    #[test]
    fn dep_gate_bleibt_ausserhalb_boarding_stehen() {
        assert!(!dep_gate_bei_szenerie_wechsel_zuruecksetzen(
            FlightPhase::Pushback
        ));
        assert!(!dep_gate_bei_szenerie_wechsel_zuruecksetzen(
            FlightPhase::TaxiOut
        ));
        assert!(!dep_gate_bei_szenerie_wechsel_zuruecksetzen(
            FlightPhase::Cruise
        ));
    }

    /// v1.7.17 Runde 3 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): die VOLLE Ableitungsbedingung bei vorhandenem
    /// `dep_gate`, nicht nur ihre Boarding-Teilbedingung. Runde 4:
    /// umbenannt von `dep_gate_muss_verworfen_werden`, weil die Funktion
    /// seit Runde 4 nichts mehr loescht — sie sagt nur noch, ob ein
    /// Ableitungsversuch lohnt (siehe Funktionsdoku).
    #[test]
    fn dep_gate_braucht_neuableitung_bei_generationswechsel_waehrend_boarding() {
        assert!(dep_gate_braucht_neuableitung(
            FlightPhase::Boarding,
            true,
            1,
            2
        ));
    }

    #[test]
    fn dep_gate_braucht_keine_neuableitung_ohne_generationswechsel() {
        assert!(!dep_gate_braucht_neuableitung(
            FlightPhase::Boarding,
            true,
            2,
            2
        ));
    }

    #[test]
    fn dep_gate_braucht_ausserhalb_boarding_keine_neuableitung_trotz_generationswechsel() {
        assert!(!dep_gate_braucht_neuableitung(
            FlightPhase::Pushback,
            true,
            1,
            2
        ));
    }

    /// v1.7.17 Runde 4: ohne vorhandenen `dep_gate` lohnt der Versuch
    /// IMMER (die alte Funktion sagte hier faelschlich "nichts zu tun" —
    /// das war vor Runde 4 richtig, weil die Aufrufstelle den Erstversuch
    /// noch selbst per `is_none()` auffing; seit Runde 4 traegt DIESE
    /// Funktion die ganze Entscheidung).
    #[test]
    fn ohne_vorhandenen_dep_gate_lohnt_immer_ein_versuch() {
        assert!(dep_gate_braucht_neuableitung(
            FlightPhase::Boarding,
            false,
            1,
            2
        ));
        assert!(dep_gate_braucht_neuableitung(
            FlightPhase::Pushback,
            false,
            1,
            2
        ));
    }

    /// v1.7.17 Runde 5 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): verlaesst das Boarding einen bekanntlich veralteten
    /// `dep_gate` unkorrigiert, muss die letzte-Chance-Pruefung das
    /// erkennen — sonst gaelte er in Live-Post/PIREP/Debrief als aktuell.
    ///
    /// Runde 6: der erste Parameter heisst jetzt `boarding_wird_verlassen`
    /// und wird DIREKT verwendet (kein `!` mehr) — er muss vom Aufrufer aus
    /// dem ECHTEN Uebergang gebildet werden (siehe Funktionsdoku).
    #[test]
    fn dep_gate_wird_beim_verlassen_des_boardings_mit_veralteter_generation_verworfen() {
        assert!(dep_gate_muss_beim_boarding_ende_verworfen_werden(
            true, true, 1, 2
        ));
    }

    #[test]
    fn dep_gate_bleibt_beim_verlassen_des_boardings_ohne_generationswechsel_stehen() {
        assert!(!dep_gate_muss_beim_boarding_ende_verworfen_werden(
            true, true, 2, 2
        ));
    }

    #[test]
    fn dep_gate_wird_waehrend_des_boardings_selbst_nicht_ueber_diesen_riegel_verworfen() {
        // Waehrend des Boardings entscheidet `dep_gate_braucht_neuableitung`
        // (atomarer Ersatz) — dieser Riegel greift erst BEIM Verlassen.
        // "Waehrend" heisst jetzt: kein Uebergang (`boarding_wird_verlassen
        // = false`), nicht mehr "aktuelle Phase ist Boarding".
        assert!(!dep_gate_muss_beim_boarding_ende_verworfen_werden(
            false, true, 1, 2
        ));
    }

    /// v1.7.17 Runde 6 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): der eigentliche Befund — `boarding_wird_verlassen`
    /// darf NICHT aus der aktuellen Phase allein kommen, sonst feuert der
    /// Riegel bei JEDEM spaeteren Tick, in dem die Phase zufaellig nicht
    /// Boarding ist (z. B. Stunden spaeter waehrend Cruise, ausgeloest von
    /// einem voellig unrelated Reconnect). Nur der ECHTE Uebergang zaehlt.
    #[test]
    fn boarding_wird_nur_beim_echten_uebergang_als_verlassen_erkannt() {
        assert!(boarding_wird_in_diesem_tick_verlassen(
            FlightPhase::Boarding,
            FlightPhase::Pushback
        ));
    }

    #[test]
    fn boarding_gilt_stunden_spaeter_nicht_mehr_als_gerade_verlassen() {
        // Alte Phase war schon LAENGST nicht mehr Boarding (z. B. Cruise)
        // — ein Reconnect in diesem Tick darf den laengst historischen
        // dep_gate nicht anfassen.
        assert!(!boarding_wird_in_diesem_tick_verlassen(
            FlightPhase::Cruise,
            FlightPhase::Cruise
        ));
    }

    #[test]
    fn boarding_gilt_waehrend_des_boardings_selbst_nicht_als_verlassen() {
        assert!(!boarding_wird_in_diesem_tick_verlassen(
            FlightPhase::Boarding,
            FlightPhase::Boarding
        ));
    }

    #[test]
    fn ohne_vorhandenen_dep_gate_gibt_es_beim_boarding_ende_nichts_zu_verwerfen() {
        assert!(!dep_gate_muss_beim_boarding_ende_verworfen_werden(
            true, false, 1, 2
        ));
    }

    /// Nachgereicht (externe Gegenpruefung Codex, adversarial — 15 Runden
    /// Vorgeschichte, docs/qs/v1.7.17-pruefstand.md Runden 17-19): die
    /// eigentliche Reconciliation (Server-Read-Back) ist bewusst nicht
    /// gebaut, aber eine unklare Zustellung nach einem Resume darf nicht
    /// laenger unsichtbar bleiben. Text-Waechter statt Verhaltensprobe,
    /// weil `try_resume_flight` (der einzige Aufrufer) ein grosser,
    /// nicht isoliert testbarer async-Pfad ist — derselbe Grund, aus dem
    /// `maybe_spawn_stand_fetch` und `dep_szenerie_auskunft_uebernehmen`
    /// schon ohne eigene Verhaltensprobe auskommen mussten.
    ///
    /// ⚠ NICHT den vollen Text als EIN Literal suchen — dieser Kommentar
    /// nennt ihn bereits, und `include_str!` liest auch die eigene Datei.
    /// Vorkommen ZAEHLEN (`>= 2`): eines ist diese Doku, mindestens ein
    /// zweites muss der echte Aufruf sein.
    #[test]
    fn resume_meldet_unklare_dep_gate_zustellung_sichtbar() {
        // v3 (Etappe 0): nur Code-Zeilen ohne Kommentar zaehlen (siehe oben).
        let quelle = crate::quelle_gesamt::PRODUKTION;
        let vorkommen: usize = quelle
            .lines()
            .filter(|z| !z.trim_start().starts_with("//"))
            .map(|z| z.matches("Abflug-Stand: Zustellung nach Neustart unklar").count())
            .sum();
        assert!(
            vorkommen >= 1,
            "die sichtbare Warnung bei unklarer dep_gate-Zustellung nach Resume fehlt \
             oder wurde entfernt (kein Vorkommen ausserhalb von Kommentaren in lib.rs)"
        );
    }
