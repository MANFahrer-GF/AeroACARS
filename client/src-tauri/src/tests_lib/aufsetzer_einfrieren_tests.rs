    use super::*;

    /// v3 (P-4.13): Testflug wie in `positions_idempotenz_tests`.
    fn flug() -> ActiveFlight {
        ActiveFlight {
            pirep_id: "test-pirep".into(),
            bid_id: 0,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            airline_icao: String::new(),
            planned_registration: String::new(),
            aircraft_icao: "C172".into(),
            aircraft_name: String::new(),
            flight_number: "1".into(),
            dpt_airport: "EDDF".into(),
            arr_airport: "ZZZZ".into(),
            planned_flight_time_min: None,
            plan_strecke_nm: None,
            airline_logo_url: None,
            fares: Vec::new(),
            stats: stats_sperre::GemesseneSperre::new(FlightStats::new()),
            stop: AtomicBool::new(false),
            was_just_resumed: AtomicBool::new(false),
            streamer_spawned: AtomicBool::new(false),
            cancelled_remotely: AtomicBool::new(false),
            position_outbox: Mutex::new(std::collections::VecDeque::new()),
            positionen_nachsenden: AtomicBool::new(false),
            generation: std::sync::atomic::AtomicU64::new(0),
            start_sperre: Mutex::new(()),
            phpvms_worker_spawned: AtomicBool::new(false),
            touchdown_sampler_spawned: AtomicBool::new(false),
            connection_state: std::sync::atomic::AtomicU8::new(CONN_STATE_LIVE),
            navdata: Mutex::new(NavdataCache::default()),
        }
    }

    fn probe() -> TouchdownWindowSample {
        TouchdownWindowSample::from(TelemetrySample::default())
    }

    /// P-4.13: Ein Touch-and-Go wird VOR dem Zuruecksetzen eingefroren —
    /// Eingabe und Fenster gehoeren zu genau diesem Aufsetzer, das Fenster
    /// wandert mit (die naechste Landung bekommt ein eigenes).
    #[test]
    fn touch_and_go_wird_vor_dem_zuruecksetzen_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        s.phase = FlightPhase::Landing;
        s.landing_at = Some(Utc::now());
        s.landung_fenster = vec![probe(), probe()];
        s.landing_peak_vs_fpm = Some(-240.0);
        s.landing_source = Some("vs_at_impact".into());
        s.bounce_count = 2;
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren.len(), 1);
        let (eingabe, fenster, herkunft) = s.aufsetzer_eingefroren[0].clone();
        assert_eq!(fenster.len(), 2);
        assert!(s.landung_fenster.is_empty(), "Fenster wandert mit dem Aufsetzer");
        assert_eq!(eingabe, landungs_bewertungseingabe(&f, &s, Some("C172"), "ZZZZ"), "Eingabe vor dem Zuruecksetzen");
        // P-9.44: Die Herkunft ist die DIESES Aufsetzers, eingefroren vor dem
        // Zuruecksetzen — die naechste Landung ueberschreibt sie nicht.
        assert_eq!(herkunft["sinkrate"]["landing_source"], "vs_at_impact");
        assert_eq!(herkunft["sinkrate"]["hopser"], 2);
        s.landing_source = Some("smoothed_500ms".into());
        s.bounce_count = 0;
        let jetzt = landungs_herkunft(&f, &s, &eingabe, "ZZZZ");
        assert_eq!(jetzt["sinkrate"]["landing_source"], "smoothed_500ms", "Gegenprobe: liest die Flugdaten");
        assert_eq!(s.aufsetzer_eingefroren[0].2["sinkrate"]["landing_source"], "vs_at_impact");
    }

    /// Kap. 5.2: der Herkunftsblock traegt die Bahnquelle mit Commit, Zyklus
    /// und Szenerie sowie Spurweite, Vref, Abdeckung, Ausweichziel, Phasen.
    #[test]
    fn herkunft_nennt_die_bahnquelle() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        let e = landungs_bewertungseingabe(&f, &s, Some("C172"), "EDDM");
        let h = landungs_herkunft(&f, &s, &e, "EDDM");
        assert!(h["bahn"]["quelle"]["art"].is_null(), "ohne Zuordnung keine Quelle");
        assert_eq!(h["ausweichziel"], serde_json::json!({ "eingereicht": "EDDM", "geplant": "ZZZZ" }));
        assert_eq!(h["bahn"]["quelle"]["ourairports_commit"], serde_json::to_value(&laufzeit_paket::stand().ourairports_commit).unwrap());
        assert_eq!(h["bahn"]["quelle"]["paket"], serde_json::to_value(&laufzeit_paket::stand().id).unwrap());
        for k in ["sinkrate", "spurweite", "vref", "abdeckung_fehlt", "divert_hint", "phasenwechsel", "aufsetzer", "durchstarts"] {
            assert!(h.get(k).is_some(), "{k} fehlt");
        }
        for k in ["zuordnung", "runway_nav_geometry", "tch_ist_ft", "szenerie_stand", "korrekturen", "rollspur"] {
            assert!(h["bahn"].get(k).is_some(), "bahn.{k} fehlt");
        }
        s.runway_source = Some(runway::RunwaySource::OurAirportsFallback);
        assert_eq!(landungs_herkunft(&f, &s, &e, "EDDM")["bahn"]["quelle"]["art"], "ourairports");
        s.runway_source = Some(runway::RunwaySource::Navigraph);
        s.runway_nav_cycle = Some("2611".into());
        let h = landungs_herkunft(&f, &s, &e, "EDDM");
        assert_eq!(h["bahn"]["quelle"]["art"], "navigraph");
        assert_eq!(h["bahn"]["quelle"]["navigraph_zyklus"], "2611");
        s.szenerie_uebernahme = Some(szenerie_bahn::UebernahmeBericht { uebernommen: vec!["26R".into()], ..Default::default() });
        assert_eq!(landungs_herkunft(&f, &s, &e, "EDDM")["bahn"]["quelle"]["art"], "szenerie");
        // Spurweite: Flugzeugdatei vor Typtabelle.
        s.fahrwerk_spurweite_m = Some(3.1);
        let e2 = landungs_bewertungseingabe(&f, &s, Some("C172"), "EDDM");
        let h2 = landungs_herkunft(&f, &s, &e2, "EDDM");
        assert_eq!(h2["spurweite"]["quelle"], "flugzeugdatei");
        assert_eq!(h2["spurweite"]["m"], 3.1);
    }

    /// P-9.44 (Codex 30.10.): Die Touch-and-Go-Entscheidung DIESES Aufsetzers
    /// faellt erst nach dem Einfrieren; sie wird in seine Herkunft
    /// nachgetragen. Gegenprobe: ein fremder Aufsetzer (andere Zeit) bleibt
    /// unberuehrt.
    #[test]
    fn touch_and_go_entscheidung_steht_in_der_herkunft() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        s.phase = FlightPhase::Landing;
        s.was_airborne = true;
        s.landing_at = Some(Utc::now());
        s.landung_fenster = vec![probe()];
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren[0].2["durchstarts"], 0, "vor der Entscheidung");
        landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
        let h = &s.aufsetzer_eingefroren[0].2;
        assert_eq!(h["durchstarts"], 1);
        assert_eq!(h["aufsetzer"].as_array().unwrap().last().unwrap()["kind"], "touch_and_go");

        // Gegenprobe: eingefroren fuer einen anderen Aufsetzer → kein Nachtrag.
        s.phase = FlightPhase::Landing;
        s.landing_at = Some(Utc::now() + chrono::Duration::seconds(60));
        s.landung_fenster = vec![probe()];
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        s.aufsetzer_eingefroren[1].2["aufsetzen"] = serde_json::json!("anderer");
        landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
        assert_eq!(s.aufsetzer_eingefroren[1].2["durchstarts"], 1, "fremder Aufsetzer unberuehrt");
    }

    /// P-4.13: Schliesst `landung_beim_wegsteigen_abschliessen` nicht ab
    /// (Phasenpruefung: nach dem Abrollen, oder Startlauf ohne Abheben), ist
    /// es kein Touch-and-Go. Dann wird auch nichts eingefroren — sonst ginge
    /// ein Dokument `touch_and_go` ohne Entscheidung hinaus, und die echte
    /// Landung verloere ihr Fenster. Gegenprobe: in der Landung, nach dem
    /// Fliegen, wird eingefroren und die Entscheidung nachgetragen.
    #[test]
    fn ohne_abschluss_wird_nichts_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        for (phase, in_der_luft) in [(FlightPhase::TaxiIn, true), (FlightPhase::Takeoff, false)] {
            s.phase = phase;
            s.was_airborne = in_der_luft;
            s.landing_at = Some(Utc::now());
            s.landung_fenster = vec![probe(), probe()];
            aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
            landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
            assert!(s.aufsetzer_eingefroren.is_empty(), "{phase:?}/{in_der_luft}: kein Touch-and-Go");
            assert_eq!(s.landung_fenster.len(), 2, "{phase:?}/{in_der_luft}: Fenster bleibt der Landung");
        }
        // Gegenprobe: offene Landung → eingefroren, Entscheidung steht drin.
        s.phase = FlightPhase::Landing;
        s.was_airborne = true;
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
        assert_eq!(s.aufsetzer_eingefroren.len(), 1);
        assert_eq!(s.aufsetzer_eingefroren[0].2["durchstarts"], 1);
        assert!(s.landung_fenster.is_empty());
    }

    #[test]
    fn ohne_fenster_wird_nichts_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert!(s.aufsetzer_eingefroren.is_empty());
    }
