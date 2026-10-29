    use super::*;

    fn fixture_with_guards(latched: bool) -> ActiveFlight {
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
            stop: AtomicBool::new(true),
            was_just_resumed: AtomicBool::new(false),
            streamer_spawned: AtomicBool::new(latched),
            cancelled_remotely: AtomicBool::new(false),
            position_outbox: Mutex::new(std::collections::VecDeque::new()),
            positionen_nachsenden: AtomicBool::new(false),
            generation: std::sync::atomic::AtomicU64::new(0),
            start_sperre: Mutex::new(()),
            phpvms_worker_spawned: AtomicBool::new(latched),
            touchdown_sampler_spawned: AtomicBool::new(latched),
            connection_state: std::sync::atomic::AtomicU8::new(CONN_STATE_LIVE),
            navdata: Mutex::new(NavdataCache::default()),
        }
    }

    #[test]
    fn resets_all_three_guards_that_are_already_latched_from_the_original_spawn() {
        let flight = fixture_with_guards(true);
        rearm_background_task_guards(&flight);
        assert!(!flight.streamer_spawned.load(Ordering::SeqCst));
        assert!(!flight.phpvms_worker_spawned.load(Ordering::SeqCst));
        assert!(!flight.touchdown_sampler_spawned.load(Ordering::SeqCst));
    }

    #[test]
    fn leaves_fresh_unlatched_guards_false() {
        let flight = fixture_with_guards(false);
        rearm_background_task_guards(&flight);
        assert!(!flight.streamer_spawned.load(Ordering::SeqCst));
        assert!(!flight.phpvms_worker_spawned.load(Ordering::SeqCst));
        assert!(!flight.touchdown_sampler_spawned.load(Ordering::SeqCst));
    }
