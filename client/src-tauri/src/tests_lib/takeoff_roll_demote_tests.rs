    use super::*;
    use chrono::Duration;

    /// Backtrack trippt TakeoffRoll, dann Ausrichten (GS → 0): nach dem
    /// Dwell zurück zu TaxiOut, nicht ewig „Takeoff Roll".
    #[test]
    fn demotes_to_taxi_after_dwell() {
        let t0 = Utc::now();
        // Erster langsamer Tick am Boden: Dwell startet, noch KEIN Demote.
        let (demote, slow) = takeoff_roll_demote(true, 0.0, None, t0);
        assert_eq!(
            demote, None,
            "erster langsamer Tick darf nicht sofort demoten"
        );
        assert!(slow.is_some(), "Dwell-Start muss gestempelt sein");
        // Innerhalb des Dwells (2 s): weiter TakeoffRoll.
        let (demote, slow) = takeoff_roll_demote(true, 3.0, slow, t0 + Duration::seconds(2));
        assert_eq!(demote, None);
        // Nach dem Dwell (4 s ≥ 3 s): zurück zu TaxiOut + Dwell zurückgesetzt.
        let (demote, slow2) = takeoff_roll_demote(true, 1.0, slow, t0 + Duration::seconds(4));
        assert_eq!(demote, Some(FlightPhase::TaxiOut));
        assert_eq!(slow2, None, "Dwell nach Demote zurückgesetzt");
    }

    /// Echte Startrolle: GS steigt monoton, nie unter 20 → NIE demote
    /// (der Kern-Regressionsschutz), Dwell bleibt leer.
    #[test]
    fn real_takeoff_roll_never_demotes() {
        let t0 = Utc::now();
        let mut slow = None;
        for (i, gs) in [35.0f32, 60.0, 90.0, 140.0].iter().enumerate() {
            let (demote, s) =
                takeoff_roll_demote(true, *gs, slow, t0 + Duration::seconds(i as i64));
            assert_eq!(demote, None, "Startrolle darf nie demoten (gs={gs})");
            slow = s;
        }
        assert_eq!(slow, None, "über der Schwelle bleibt der Dwell leer");
    }

    /// Ein einzelner GS-Glitch-Tick (<20) reicht NICHT: erholt sich die GS
    /// vor Ablauf des Dwells, wird der Dwell verworfen.
    #[test]
    fn single_glitch_tick_does_not_demote() {
        let t0 = Utc::now();
        let (demote, slow) = takeoff_roll_demote(true, 5.0, None, t0); // Glitch
        assert_eq!(demote, None);
        assert!(slow.is_some());
        // Nächster Tick: GS wieder hoch → Dwell verworfen, kein Demote.
        let (demote, slow) = takeoff_roll_demote(true, 80.0, slow, t0 + Duration::seconds(1));
        assert_eq!(demote, None);
        assert_eq!(slow, None, "GS-Erholung verwirft den Dwell");
    }

    /// Liftoff (!on_ground) verwirft den Dwell — die Vorwärtskante zu
    /// Takeoff übernimmt separat im Arm.
    #[test]
    fn airborne_resets_dwell() {
        let t0 = Utc::now();
        let (demote, slow) = takeoff_roll_demote(false, 5.0, Some(t0), t0);
        assert_eq!(demote, None);
        assert_eq!(slow, None);
    }
