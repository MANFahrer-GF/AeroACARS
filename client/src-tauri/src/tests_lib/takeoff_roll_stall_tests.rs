    use super::*;
    use chrono::Duration;

    /// Backtrack/Fehl-Trip: plateauisiert bei ~30 kt (über der GS<20-Kante),
    /// erreicht nie 45 kt → nach dem Dwell zurück zu TaxiOut.
    #[test]
    fn backtrack_plateau_demotes_after_dwell() {
        let t0 = Utc::now();
        let peak = 30.0f32; // Eintritt bei 30 kt
                            // Plateau: GS setzt keine neuen Höchstwerte → Dwell startet.
        let (d, peak, stall) = takeoff_roll_stall_check(true, 29.0, peak, None, t0);
        assert_eq!(d, None, "erster Plateau-Tick demotet nicht sofort");
        assert!(stall.is_some(), "Plateau startet den Dwell");
        // Innerhalb des Dwells: weiter TakeoffRoll.
        let (d, peak, stall) =
            takeoff_roll_stall_check(true, 30.0, peak, stall, t0 + Duration::seconds(6));
        assert_eq!(d, None);
        // Nach dem Dwell (12 s) ohne neuen Peak → Backtrack erkannt.
        let (d, _p, _s) =
            takeoff_roll_stall_check(true, 30.0, peak, stall, t0 + Duration::seconds(12));
        assert_eq!(
            d,
            Some(FlightPhase::TaxiOut),
            "Backtrack nach Dwell → TaxiOut"
        );
    }

    /// Echte Startrolle: jeder Tick ein neuer GS-Höchstwert → Dwell wird nie
    /// voll; ab 45 kt committed → Kante ganz aus. NIE demote.
    #[test]
    fn real_takeoff_new_peaks_never_demote() {
        let t0 = Utc::now();
        let mut peak = 30.0f32;
        let mut stall = None;
        for (i, gs) in [32.0f32, 36.0, 41.0, 47.0].iter().enumerate() {
            let (d, p, s) =
                takeoff_roll_stall_check(true, *gs, peak, stall, t0 + Duration::seconds(i as i64));
            assert_eq!(d, None, "Startrolle darf nie plateau-demoten (gs={gs})");
            peak = p;
            stall = s;
        }
        assert!(peak >= TAKEOFF_ROLL_CONFIRM_KT, "Peak hat CONFIRM erreicht");
        // Committed: ein späterer GS-Dip demotet NICHT mehr über diese Kante
        // (dafür ist die GS<20-Abbruchkante zuständig).
        let (d, _p, _s) =
            takeoff_roll_stall_check(true, 25.0, peak, None, t0 + Duration::seconds(30));
        assert_eq!(d, None, "nach CONFIRM ist die Plateau-Kante aus");
    }

    /// Bulletproof gegen TRÄGE echte Starts: nur ~1 kt/s, 15 s unter 45 kt —
    /// aber stetig neue Höchstwerte → Dwell wird nie voll, nie demote.
    #[test]
    fn slow_but_steady_takeoff_never_demotes() {
        let t0 = Utc::now();
        let mut peak = 30.0f32;
        let mut stall = None;
        for i in 1..=15i64 {
            let gs = 30.0 + i as f32; // 31,32,…,45
            let (d, p, s) =
                takeoff_roll_stall_check(true, gs, peak, stall, t0 + Duration::seconds(i));
            assert_eq!(
                d, None,
                "träge aber stetige Startrolle darf nie demoten (t={i}s)"
            );
            peak = p;
            stall = s;
        }
    }

    /// Airborne verwirft die Plateau-Kante (Vorwärtskante zu Takeoff greift).
    #[test]
    fn airborne_disables_plateau_demote() {
        let t0 = Utc::now();
        let (d, _p, s) =
            takeoff_roll_stall_check(false, 30.0, 30.0, Some(t0), t0 + Duration::seconds(60));
        assert_eq!(d, None);
        assert_eq!(s, None);
    }
