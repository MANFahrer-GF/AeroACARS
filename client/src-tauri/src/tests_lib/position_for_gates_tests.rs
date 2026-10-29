    use super::*;

    /// The failure this exists to prevent: a pilot blocks on at EDDF on a flight
    /// planned to EDDM, his sim dies, he presses "Flug wiederaufnehmen" (which
    /// nulls the distance baseline `last_lat/lon` — deliberately, so the
    /// repositioning jump cannot count as flown distance), and then files. If the
    /// gate can no longer place him, it is skipped, and Frankfurt gets filed as
    /// Munich, ACARS-auto-approved.
    #[test]
    fn a_resume_does_not_erase_where_the_aircraft_is() {
        let mut stats = FlightStats::new();
        stats.last_lat = Some(50.0333);
        stats.last_lon = Some(8.5706);
        stats.last_known_lat = Some(50.0333);
        stats.last_known_lon = Some(8.5706);

        // What `apply_pause_resume` does: clears the distance baseline.
        stats.last_lat = None;
        stats.last_lon = None;

        let pos = aircraft_position_for_gates(&stats)
            .expect("the gate must still know where the aircraft is");
        assert!((pos.0 - 50.0333).abs() < 0.001 && (pos.1 - 8.5706).abs() < 0.001);
    }

    /// A flight resumed from a pre-v0.19.3 snapshot has no `last_known_*` — the
    /// distance baseline, and failing that the touchdown point, still place it.
    #[test]
    fn older_flights_fall_back_to_what_they_have() {
        let mut stats = FlightStats::new();
        stats.last_lat = Some(50.0333);
        stats.last_lon = Some(8.5706);
        assert!(aircraft_position_for_gates(&stats).is_some());

        let mut stats = FlightStats::new();
        stats.landing_lat = Some(50.0333);
        stats.landing_lon = Some(8.5706);
        assert!(
            aircraft_position_for_gates(&stats).is_some(),
            "the touchdown point is a perfectly good answer for an arrival gate"
        );
    }

    /// And a position the sim never placed is not an answer at all.
    #[test]
    fn null_island_is_not_a_position() {
        let mut stats = FlightStats::new();
        stats.last_known_lat = Some(0.008);
        stats.last_known_lon = Some(0.00002);
        assert_eq!(aircraft_position_for_gates(&stats), None);
    }
