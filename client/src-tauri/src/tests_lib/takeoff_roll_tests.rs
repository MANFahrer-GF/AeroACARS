    use super::*;

    fn taxiing(gs: f64) -> SimSnapshot {
        SimSnapshot {
            on_ground: true,
            groundspeed_kt: gs as f32,
            engines_running: 4,
            // A real position — the FSM refuses snapshots the sim hasn't placed
            // (see `snapshot_position_is_usable`), and these tests are about the
            // takeoff roll, not about that gate.
            lat: 35.2226,
            lon: -80.9431, // KCLT
            ..SimSnapshot::default()
        }
    }

    /// THE regression: a widebody rolling along the taxiway at 30-something knots
    /// at a constant speed is taxiing, not departing.
    #[test]
    fn a_heavy_taxiing_at_35_kt_is_not_a_takeoff_roll() {
        let mut stats = FlightStats::new();
        let t0 = Utc::now();
        let snap = taxiing(35.0);

        // Several ticks at a steady 35 kt — the speed a heavy actually taxis at.
        let mut fired = false;
        for i in 0..10 {
            let now = t0 + chrono::Duration::seconds(i * 2);
            fired |= takeoff_roll_detected(&mut stats, &snap, now);
        }
        assert!(
            !fired,
            "a constant 35 kt taxi must never be announced as a takeoff roll"
        );
    }

    /// And the thing it must still catch: an actual takeoff roll, where the
    /// aircraft is gaining speed every second.
    #[test]
    fn a_real_takeoff_roll_is_detected() {
        let mut stats = FlightStats::new();
        let t0 = Utc::now();

        let mut fired = false;
        // 30 → 48 kt over 6 s: 3 kt/s, a normal airliner acceleration.
        for i in 0..7 {
            let now = t0 + chrono::Duration::seconds(i);
            let snap = taxiing(30.0 + 3.0 * i as f64);
            fired |= takeoff_roll_detected(&mut stats, &snap, now);
        }
        assert!(fired, "an accelerating takeoff roll must be detected");
    }

    /// A Cessna rotates around 50 kt, so its roll must be recognised BEFORE that.
    /// Ticked at the streamer's real ground cadence (3 s) with a modest 2 kt/s:
    /// the two required windows complete at 6 s and ~42 kt — before rotation.
    #[test]
    fn a_light_aircraft_is_detected_before_rotation() {
        let mut stats = FlightStats::new();
        let t0 = Utc::now();
        let mut gs_when_fired: Option<f64> = None;
        for tick in 0..6_i64 {
            let secs = tick * 3; // the streamer's ground cadence
            let gs = 30.0 + 2.0 * secs as f64;
            let mut snap = taxiing(gs);
            snap.engines_running = 1;
            if takeoff_roll_detected(&mut stats, &snap, t0 + chrono::Duration::seconds(secs))
                && gs_when_fired.is_none()
            {
                gs_when_fired = Some(gs);
            }
        }
        let gs = gs_when_fired.expect("a GA takeoff roll must be detected");
        assert!(
            gs < 50.0,
            "must fire before rotation (~50 kt), fired at {gs} kt"
        );
    }

    /// QS round 8: windows must not be BANKED below the floor. They used to
    /// accumulate while merely accelerating onto the taxiway, so the count was
    /// already at 2 when the aircraft crossed 30 kt and the first window above
    /// the floor fired — the "two windows" rule bought nothing exactly where it
    /// mattered, and Thomas's A346 would have flip-flopped again.
    #[test]
    fn windows_are_not_banked_below_the_floor() {
        let mut stats = FlightStats::new();
        let t0 = Utc::now();
        // Accelerate gently from a standstill to a 33 kt taxi, then HOLD it.
        let mut fired = false;
        for tick in 0..12_i64 {
            let secs = tick * 3;
            let gs = (2.0 * secs as f64).min(33.0); // 0 → 33 kt, then constant
            fired |= takeoff_roll_detected(
                &mut stats,
                &taxiing(gs),
                t0 + chrono::Duration::seconds(secs),
            );
        }
        assert!(
            !fired,
            "accelerating onto the taxiway and then holding 33 kt is a taxi, not a \
             takeoff roll"
        );
    }

    /// A bush plane rotates around 45 kt and accelerates at ~5 kt/s — it is
    /// airborne six seconds after crossing the floor. Waiting for two windows
    /// would skip its TakeoffRoll entirely, so an unambiguous rate counts as
    /// proof on its own.
    #[test]
    fn a_bush_plane_is_detected_despite_its_very_short_roll() {
        let mut stats = FlightStats::new();
        let t0 = Utc::now();
        let mut gs_when_fired: Option<f64> = None;
        for tick in 0..6_i64 {
            let secs = tick * 3;
            let gs = 5.0 * secs as f64; // 5 kt/s from a standstill
            let mut snap = taxiing(gs);
            snap.engines_running = 1;
            if takeoff_roll_detected(&mut stats, &snap, t0 + chrono::Duration::seconds(secs))
                && gs_when_fired.is_none()
            {
                gs_when_fired = Some(gs);
            }
        }
        let gs = gs_when_fired.expect("a bush takeoff roll must be detected");
        assert!(
            gs < 45.0,
            "must fire before rotation (~45 kt), fired at {gs} kt"
        );
    }

    /// No taxi reaches 60 kt. No evidence required.
    #[test]
    fn speed_no_taxi_reaches_needs_no_further_evidence() {
        let mut stats = FlightStats::new();
        assert!(takeoff_roll_detected(
            &mut stats,
            &taxiing(crate::grenzwerte::grenzwerte().phasen_kern.takeoff_roll_unambiguous_kt.wert + 1.0),
            Utc::now()
        ));
    }

    /// Takeoff thrust is proof on its own, for the addons that report N1 —
    /// it fires on the first tick, without waiting for the acceleration window.
    #[test]
    fn takeoff_thrust_is_evidence_by_itself() {
        let mut stats = FlightStats::new();
        let snap = SimSnapshot {
            eng_n1_pct: Some(vec![92.0, 91.5, 92.4, 91.8]),
            ..taxiing(32.0)
        };
        assert!(takeoff_roll_detected(&mut stats, &snap, Utc::now()));
    }

    /// Idle thrust at taxi speed is not.
    #[test]
    fn idle_thrust_at_taxi_speed_is_not() {
        let mut stats = FlightStats::new();
        let snap = SimSnapshot {
            eng_n1_pct: Some(vec![24.0, 23.5, 24.2, 23.8]),
            ..taxiing(32.0)
        };
        let t0 = Utc::now();
        let mut fired = false;
        for i in 0..8 {
            fired |=
                takeoff_roll_detected(&mut stats, &snap, t0 + chrono::Duration::seconds(i * 2));
        }
        assert!(!fired, "idling at taxi speed is taxiing");
    }

    /// The one way this change could ruin a whole flight: if the roll detector
    /// stays silent AND the FSM then has no way out of TaxiOut, a pilot would
    /// fly an entire leg stuck on the ground.
    ///
    /// It cannot: the TaxiOut arm promotes straight to Takeoff on the liftoff
    /// edge (was on ground → airborne, AGL > 5 ft, VS > 100 fpm), which exists
    /// because helicopters and gliders skip TakeoffRoll anyway. Worst case, an
    /// aeroplane skips the TakeoffRoll phase too — it never gets stranded.
    #[test]
    fn a_flight_never_hangs_in_taxiout_even_if_the_roll_is_never_detected() {
        let flight = flight_in_taxi_out();
        // Airborne, climbing — but the roll detector never saw an acceleration
        // (e.g. a tick gap swallowed the entire roll).
        let snap = SimSnapshot {
            on_ground: false,
            altitude_agl_ft: 40.0,
            vertical_speed_fpm: 900.0,
            groundspeed_kt: 150.0,
            engines_running: 2,
            lat: 35.2226,
            lon: -80.9431,
            ..SimSnapshot::default()
        };
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.was_on_ground = Some(true);
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result,
            Some(FlightPhase::Takeoff),
            "an aircraft that is airborne and climbing must reach Takeoff even if \
             TakeoffRoll was never entered"
        );
    }

    fn flight_in_taxi_out() -> ActiveFlight {
        let flight = ActiveFlight {
            pirep_id: "test-pirep".into(),
            bid_id: 0,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            airline_icao: String::new(),
            planned_registration: String::new(),
            aircraft_icao: "B738".into(),
            aircraft_name: String::new(),
            flight_number: "1".into(),
            dpt_airport: "KCLT".into(),
            arr_airport: "EDDM".into(),
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
        };
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::TaxiOut;
            stats.block_off_at = Some(Utc::now() - chrono::Duration::minutes(10));
        }
        flight
    }

    /// Slowing down (taxi hold, line-up) must re-baseline, so that a later,
    /// slow drift back up cannot be mistaken for acceleration.
    #[test]
    fn slowing_down_rebaselines_the_acceleration_window() {
        let mut stats = FlightStats::new();
        let t0 = Utc::now();
        // Rolling at 34 kt…
        takeoff_roll_detected(&mut stats, &taxiing(34.0), t0);
        // …slows to 5 kt for the hold…
        takeoff_roll_detected(&mut stats, &taxiing(5.0), t0 + chrono::Duration::seconds(4));
        // …then creeps back to 33 kt over half a minute. That is 1 kt/s only if
        // measured against the stale 5 kt sample — it must not be.
        let mut fired = false;
        for i in 1..=6 {
            let now = t0 + chrono::Duration::seconds(4 + i * 5);
            fired |= takeoff_roll_detected(&mut stats, &taxiing(5.0 + 4.7 * i as f64), now);
        }
        assert!(
            !fired,
            "a slow creep back up to taxi speed is not a takeoff roll"
        );
    }
