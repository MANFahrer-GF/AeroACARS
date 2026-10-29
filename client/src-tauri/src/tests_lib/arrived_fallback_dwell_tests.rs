    //! v0.19.1 — tests for the universal Arrived-fallback's `engines_off_path`
    //! dwell, lowered from a flat 30 s to `ARRIVED_FALLBACK_DWELL_SECS = 9 s`
    //! (field report: GSG22 EDLN→EDDL, pilot perceived the app as frozen on
    //! "Final" for 3-4 min after touchdown — see docs/release-notes/v0.19.1.md).
    //!
    //! Deliberately still wall-clock (not a tick count): `step_flight` reads
    //! real time via `Utc::now()` with no injectable clock, and — more
    //! importantly — a tick-count threshold would be wrong on its own terms
    //! here, because `adaptive_tick_interval` drops the ground/near-ground
    //! tick cadence to 500 ms exactly in the "stuck on Final near/at 0 AGL"
    //! state this fallback rescues (an earlier version of this fix used tick
    //! counting and was caught doing this in code review before release —
    //! see the ARRIVED_FALLBACK_DWELL_SECS doc comment). These tests seed
    //! `arrived_fallback_pending_since` directly to simulate "N seconds have
    //! already elapsed" without sleeping in the test.
    //!
    //! `standstill_path` (engine_count_unreliable addons, e.g. Contrail
    //! FA50) shares the same `arrived_fallback_pending_since` timer with a
    //! separate, much longer dwell (`ARRIVED_STANDSTILL_DWELL_SECS`) — see
    //! `standstill_path_uses_its_own_longer_dwell_via_the_shared_timer`.

    use super::*;

    /// Minimal ActiveFlight for exercising `step_flight` directly. Uses a
    /// non-existent arrival ICAO so `runway::airport_position` returns
    /// `None` and the fallback's `near_planned` check short-circuits to
    /// `true` — isolates these tests to the dwell mechanic itself, not
    /// runway-distance geometry (covered elsewhere).
    fn fixture() -> ActiveFlight {
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

    /// Arms the "has genuinely flown" gate the universal fallback requires
    /// (`block_off_at.is_some() && was_airborne`). Starts from `Climb`, not
    /// a ground/terminal phase: this is deliberately the real-world
    /// scenario that triggers the escape hatch (legacy FSM stuck on an
    /// en-route phase, e.g. the GSG22 flight this fix is based on) — and,
    /// as a test-isolation bonus, `Climb`'s own match arm only reacts to
    /// altitude, so it can't independently transition on the ground-
    /// condition snapshots these tests feed in (a ground/terminal starting
    /// phase like `TaxiIn` would confound the fallback's own dwell with the
    /// *normal* FSM's blocks-on transition, which reacts to the exact same
    /// conditions).
    ///
    /// `dwell_started_secs_ago`: pre-seeds `arrived_fallback_pending_since`
    /// so the very first `step_flight` call already sees that much elapsed
    /// time, without sleeping. `None` leaves it unset (fresh timer, starts
    /// on the first qualifying tick — real `now()`).
    fn armed_flight(dwell_started_secs_ago: Option<i64>) -> ActiveFlight {
        let flight = fixture();
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Climb;
            stats.block_off_at = Some(Utc::now() - chrono::Duration::minutes(20));
            stats.was_airborne = true;
            if let Some(secs) = dwell_started_secs_ago {
                stats.arrived_fallback_pending_since =
                    Some(Utc::now() - chrono::Duration::seconds(secs));
            }
        }
        flight
    }

    /// `SimSnapshot::default()` is already "parked, engines off, on the
    /// ground" (see its doc comment) — exactly `arrived_fallback_
    /// conditions_basic`'s satisfied state.
    fn arrived_conditions_snap() -> SimSnapshot {
        SimSnapshot::default()
    }

    #[test]
    fn does_not_fire_before_dwell_elapsed() {
        let flight = armed_flight(Some(ARRIVED_FALLBACK_DWELL_SECS - 3));
        let snap = arrived_conditions_snap();
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None);
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Climb);
        assert!(
            flight
                .stats
                .lock()
                .unwrap()
                .arrived_fallback_pending_since
                .is_some(),
            "the timer keeps running, just hasn't reached the dwell yet"
        );
    }

    #[test]
    fn fires_once_dwell_elapsed() {
        let flight = armed_flight(Some(ARRIVED_FALLBACK_DWELL_SECS + 1));
        let snap = arrived_conditions_snap();
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Arrived));
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Arrived);
    }

    #[test]
    fn fires_exactly_at_the_dwell_boundary() {
        // elapsed >= dwell_needed — the boundary itself must fire, not just
        // strictly-greater.
        let flight = armed_flight(Some(ARRIVED_FALLBACK_DWELL_SECS));
        let snap = arrived_conditions_snap();
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Arrived));
    }

    #[test]
    fn an_interrupting_tick_resets_the_dwell_timer() {
        let flight = armed_flight(Some(ARRIVED_FALLBACK_DWELL_SECS + 1));
        let mut snap_engine_on = arrived_conditions_snap();
        snap_engine_on.engines_running = 1; // stray "still running" sample

        // The condition breaks before the (already-elapsed) dwell is ever
        // checked against a qualifying tick — must reset, not fire.
        let result = step_flight(&flight, &snap_engine_on);
        assert_eq!(result, None);
        assert!(
            flight
                .stats
                .lock()
                .unwrap()
                .arrived_fallback_pending_since
                .is_none(),
            "condition broke — timer must reset"
        );

        // A fresh qualifying tick starts a NEW timer at `now` — does not
        // fire immediately even though the aircraft was "ready" a moment
        // ago, because the interruption discarded that progress.
        let snap_ok = arrived_conditions_snap();
        let result = step_flight(&flight, &snap_ok);
        assert_eq!(result, None);
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Climb);
    }

    #[test]
    fn standstill_path_uses_its_own_longer_dwell_via_the_shared_timer() {
        // engine_count_unreliable aircraft (Contrail FA50 pattern): stock
        // engines_running is stuck NONZERO, so engines_off_path is false
        // and only standstill_path (no engines_running requirement) can
        // fire — gated by the much longer ARRIVED_STANDSTILL_DWELL_SECS.
        let flight = armed_flight(Some(ARRIVED_FALLBACK_DWELL_SECS + 1));
        let mut snap = arrived_conditions_snap();
        snap.aircraft_profile = sim_core::AircraftProfile::ContrailFa50;
        snap.engines_running = 3; // stuck nonzero — the documented FA50 bug

        // Only ARRIVED_FALLBACK_DWELL_SECS (9s) has elapsed — far short of
        // the standstill path's 300s. Must NOT fire.
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result, None,
            "standstill_path must not borrow engines_off_path's short dwell"
        );

        // Now simulate the full standstill dwell having elapsed.
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.arrived_fallback_pending_since =
                Some(Utc::now() - chrono::Duration::seconds(ARRIVED_STANDSTILL_DWELL_SECS + 1));
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Arrived));
    }

    /// v1.7.36 (QS Runde 3, F1) — ueber den ECHTEN Arrived-Uebergang: Wer
    /// `Arrived` mit laufenden Triebwerken erreicht (hier das FA50-Muster
    /// mit klemmendem Zaehler; ein Hubschrauber mit laufendem Rotor nimmt
    /// denselben Weg ohne Triebwerks-Bedingung), bekommt KEINE Abstell-Marke.
    /// Es gibt keinen Abstell-Zeitpunkt; ein geratener waere ein zu kleiner
    /// Rollsprit. Gegenprobe: derselbe Flug mit abgestellten Triebwerken
    /// rastet sie.
    #[test]
    fn arrived_mit_laufenden_triebwerken_rastet_kein_abstellen() {
        let bereit = |engines: u8, dwell: i64| {
            let flight = armed_flight(Some(dwell));
            {
                let mut st = flight.stats.lock().unwrap();
                st.takeoff_fuel_kg = Some(2_000.0);
                st.landing_fuel_kg = Some(1_300.0);
            }
            let mut snap = arrived_conditions_snap();
            snap.aircraft_profile = sim_core::AircraftProfile::ContrailFa50;
            snap.engines_running = engines;
            snap.fuel_total_kg = 1_200.0;
            (flight, snap)
        };
        let (flight, snap) = bereit(3, ARRIVED_STANDSTILL_DWELL_SECS + 1);
        assert_eq!(
            step_flight(&flight, &snap),
            Some(FlightPhase::Arrived),
            "Vorbedingung"
        );
        assert_eq!(
            flight.stats.lock().unwrap().engine_off_fuel_kg,
            None,
            "Abstell-Marke bei laufenden Triebwerken gerastet"
        );

        let (flight, snap) = bereit(0, ARRIVED_FALLBACK_DWELL_SECS + 1);
        assert_eq!(
            step_flight(&flight, &snap),
            Some(FlightPhase::Arrived),
            "Vorbedingung"
        );
        assert_eq!(
            flight.stats.lock().unwrap().engine_off_fuel_kg,
            Some(1_200.0),
            "Gegenprobe"
        );
    }

    #[test]
    fn switching_between_paths_does_not_discard_accumulated_dwell_time() {
        // Regression guard: an aircraft that satisfies engines_off_path on
        // most ticks but standstill_path on an occasional flaky tick (e.g.
        // an unreliable engine counter that isn't permanently stuck, just
        // flaky) must not have its progress wiped by the path switch —
        // both paths share one timer that only resets when BOTH are false.
        // Seeded comfortably PAST the short engines_off_path dwell (9s) but
        // nowhere near the long standstill_path dwell (300s) — tick 1 must
        // stay None (standstill_path's own threshold isn't met), tick 2
        // must fire immediately once engines_off_path becomes active,
        // proving the accumulated time carried over rather than restarting.
        let flight = armed_flight(Some(ARRIVED_FALLBACK_DWELL_SECS + 1));
        let mut snap = arrived_conditions_snap();
        snap.aircraft_profile = sim_core::AircraftProfile::ContrailFa50;
        snap.engines_running = 3; // this tick: only standstill_path qualifies

        // Not enough elapsed yet for standstill_path's 300s dwell.
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None);
        assert!(
            flight
                .stats
                .lock()
                .unwrap()
                .arrived_fallback_pending_since
                .is_some(),
            "standstill_path being active must not clear the shared timer"
        );

        // Next tick: engines_running flickers to 0 (engines_off_path now
        // qualifies instead). The ORIGINAL start time must be preserved —
        // total elapsed is now comfortably over the short engines_off_path
        // dwell, so it fires immediately rather than restarting from 0.
        snap.engines_running = 0;
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result,
            Some(FlightPhase::Arrived),
            "switching to engines_off_path must use the time already accumulated, not restart"
        );
    }
