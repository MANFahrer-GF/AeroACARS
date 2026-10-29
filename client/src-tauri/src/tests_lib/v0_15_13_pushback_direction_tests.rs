    use super::{moving_backward, moving_forward, powered_taxi_move};

    #[test]
    fn vbz_direction_classification() {
        assert!(moving_backward(Some(-6.5))); // Tug schiebt rückwärts
        assert!(!moving_backward(Some(9.4))); // Vorwärts-Taxi
        assert!(!moving_backward(Some(0.0))); // Stand
        assert!(!moving_backward(None)); // unbekannt → nicht rückwärts
        assert!(moving_forward(Some(9.4)));
        assert!(!moving_forward(Some(-6.5)));
        assert!(!moving_forward(Some(0.0)));
        assert!(moving_forward(None)); // Fallback: altes Verhalten
    }

    #[test]
    fn powered_taxi_stays_pushback_while_rolling_backward() {
        // BTX2222: Triebwerk startet beim Schieben, gs=3.9, vbz=-6.52 → KEIN Taxi.
        assert!(!powered_taxi_move(false, true, true, 3.9, Some(-6.52)));
    }

    #[test]
    fn powered_taxi_fires_on_genuine_forward_roll() {
        // echtes Vorwärts-Taxi: gs=5.6, vbz=+9.44, Triebwerke an → TaxiOut.
        assert!(powered_taxi_move(false, true, true, 5.6, Some(9.44)));
    }

    #[test]
    fn powered_taxi_falls_back_when_no_vbz() {
        // Sim ohne vbz → altes Verhalten (Triebwerk + gs>3 reicht).
        assert!(powered_taxi_move(false, true, true, 5.0, None));
    }

    #[test]
    fn powered_taxi_needs_engines_speed_and_ground() {
        assert!(!powered_taxi_move(false, true, false, 5.0, Some(9.0))); // Triebwerke aus
        assert!(!powered_taxi_move(false, true, true, 2.0, Some(9.0))); // zu langsam
        assert!(!powered_taxi_move(true, true, true, 5.0, Some(9.0))); // reload glitch
        assert!(!powered_taxi_move(false, false, true, 5.0, Some(9.0))); // nicht am Boden
    }
