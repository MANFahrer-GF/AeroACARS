    use super::*;

    // v0.20.x QS fix (finding from `2bf9f48`'s flight-lifecycle race guard):
    // that commit made `flight_end`/`flight_end_manual` hold this guard
    // across their whole take -> file -> (restore-on-failure) window,
    // specifically so a concurrent `flight_start`/`flight_adopt` can't grab
    // the just-emptied `active_flight` slot while a slow `/file` call is
    // still in flight. No test proved that concurrency semantic — this
    // does, at the unit level the guard itself lives at.

    #[test]
    fn a_second_acquire_is_rejected_while_the_first_guard_is_still_held() {
        let flag = AtomicBool::new(false);
        let _first = FlightSetupGuard::try_acquire(&flag).expect("first acquire succeeds");

        // This is the exact race: e.g. `flight_end` is mid-flight (still
        // holding `_first`, standing in for its `_lifecycle_guard`) when a
        // concurrent auto-start/manual-start tries to begin a new flight.
        let second = FlightSetupGuard::try_acquire(&flag);
        assert!(
            second.is_err(),
            "a concurrent setup must be rejected while the lifecycle guard is held"
        );
    }

    #[test]
    fn dropping_the_guard_releases_the_flag_for_a_later_acquire() {
        let flag = AtomicBool::new(false);
        {
            let _guard = FlightSetupGuard::try_acquire(&flag).expect("first acquire succeeds");
            assert!(flag.load(Ordering::SeqCst));
        }
        assert!(
            !flag.load(Ordering::SeqCst),
            "flag must be cleared once the guard is dropped"
        );
        assert!(
            FlightSetupGuard::try_acquire(&flag).is_ok(),
            "a fresh acquire must succeed once the previous guard is gone"
        );
    }

    #[test]
    fn disarm_releases_the_flag_immediately_without_waiting_for_drop() {
        let flag = AtomicBool::new(false);
        let guard = FlightSetupGuard::try_acquire(&flag).expect("first acquire succeeds");
        guard.disarm();
        assert!(
            !flag.load(Ordering::SeqCst),
            "disarm() must release the flag right away, same as flight_start/flight_adopt rely on"
        );
    }
