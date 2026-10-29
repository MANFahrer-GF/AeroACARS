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
        s.landung_fenster = vec![probe(), probe()];
        s.landing_peak_vs_fpm = Some(-240.0);
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren.len(), 1);
        let (eingabe, fenster) = &s.aufsetzer_eingefroren[0];
        assert_eq!(fenster.len(), 2);
        assert!(s.landung_fenster.is_empty(), "Fenster wandert mit dem Aufsetzer");
        assert_eq!(*eingabe, landungs_bewertungseingabe(&f, &s, Some("C172"), "ZZZZ"), "Eingabe vor dem Zuruecksetzen");
    }

    #[test]
    fn ohne_fenster_wird_nichts_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert!(s.aufsetzer_eingefroren.is_empty());
    }
