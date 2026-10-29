    use super::*;

    #[test]
    fn belegte_startsperre_ist_kein_auto_start_fehler() {
        // Der Code muss exakt der sein, den die Sperre wirklich liefert —
        // sonst griffe die Ausnahme nie.
        let flag = AtomicBool::new(false);
        let _haelt = FlightSetupGuard::try_acquire(&flag).expect("erste Sperre");
        let fehler = FlightSetupGuard::try_acquire(&flag)
            .err()
            .expect("zweite Sperre muss scheitern");
        assert!(auto_start_fehler_ist_kein_fehler(&fehler.code));
    }

    #[test]
    fn echte_fehler_bleiben_fehler() {
        for code in [
            "missing_aircraft",
            "not_at_departure",
            "flight_already_active",
        ] {
            assert!(!auto_start_fehler_ist_kein_fehler(code), "{code}");
        }
        assert!(auto_start_fehler_ist_kein_fehler(AUTO_START_SIM_WARTET));
    }
