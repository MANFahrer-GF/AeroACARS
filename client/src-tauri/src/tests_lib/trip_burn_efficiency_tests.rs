    use super::trip_burn_efficiency_pct;

    /// v0.12.4 (Spec LE5): Golden-Case DLH 1386 (PIREP jJ0kojW7kD9kK0ke).
    /// Trip-Burn = takeoff 7341.383 − landing 3862.3726 = 3479.01 kg,
    /// planned 3340 → +4.16 %. Der alte block-fuel-basierte Wert wäre
    /// +9.47 % gewesen (block 7518.789 − landing 3862.3726 = 3656.42) —
    /// diese Regression darf NICHT zurückkommen.
    #[test]
    fn dlh1386_uses_trip_burn_not_block_fuel() {
        let pct = trip_burn_efficiency_pct(Some(7341.383), Some(3862.3726), Some(3340.0))
            .expect("alle Inputs vorhanden");
        assert!((pct - 4.16).abs() < 0.15, "erwartet ~+4.2 %, war {pct}");
        assert!(pct < 6.0, "block-fuel-Regression (+9.5 %) zurück: {pct}");
    }

    #[test]
    fn none_when_input_missing_or_planned_non_positive() {
        assert!(trip_burn_efficiency_pct(Some(7000.0), Some(3000.0), None).is_none());
        assert!(trip_burn_efficiency_pct(Some(7000.0), Some(3000.0), Some(0.0)).is_none());
        assert!(trip_burn_efficiency_pct(None, Some(3000.0), Some(3000.0)).is_none());
        assert!(trip_burn_efficiency_pct(Some(7000.0), None, Some(3000.0)).is_none());
    }

    /// v0.12.4 P2: Guard identisch zu `actual_burn_for_record` — bei
    /// Refuel / Sim-Anomalie (`takeoff <= landing`) → None statt negativem
    /// Phantom-Trip-Burn.
    #[test]
    fn none_when_takeoff_not_greater_than_landing() {
        // Refuel mid-flight: mehr Sprit bei Landung als bei Takeoff.
        assert!(trip_burn_efficiency_pct(Some(3000.0), Some(3500.0), Some(2000.0)).is_none());
        // Gleichstand zählt auch nicht (takeoff > landing strikt gefordert).
        assert!(trip_burn_efficiency_pct(Some(3000.0), Some(3000.0), Some(2000.0)).is_none());
    }

    /// Minderverbrauch ergibt einen negativen Prozent-Wert.
    #[test]
    fn under_burn_is_negative() {
        // Trip-Burn 2900 vs planned 3000 → −3.33 %.
        let pct = trip_burn_efficiency_pct(Some(6000.0), Some(3100.0), Some(3000.0)).unwrap();
        assert!((pct - (-3.333)).abs() < 0.05, "war {pct}");
    }
