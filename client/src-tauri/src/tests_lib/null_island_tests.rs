    use super::*;

    /// DLH367's first session (2026-07-12, ToLiss A346) stamped a TAKEOFF at
    /// lat 0.0084 / lon 0.00002 with a 349 t takeoff weight — the sim had not
    /// placed the aircraft yet, so it was sitting at Null Island with a default
    /// airframe mass. The real takeoff, an hour later, was 300 t out of KCLT.
    /// Nothing rejected the first one; it went into the flight record.
    #[test]
    fn a_snapshot_the_sim_has_not_placed_yet_is_refused() {
        for (lat, lon) in [
            (0.008367269690816801, 1.938915588084455e-05), // the actual field report
            (0.0, 0.0),
            (0.01, -0.01),
            (f64::NAN, 8.5),
            (50.0, f64::INFINITY),
        ] {
            let snap = SimSnapshot {
                lat,
                lon,
                ..SimSnapshot::default()
            };
            assert!(
                !snapshot_position_is_usable(&snap),
                "({lat}, {lon}) is not a place an aircraft can be — the FSM must not \
                 latch anything from it"
            );
        }
    }

    /// And a real position is of course fine — including one legitimately near
    /// the equator or the prime meridian, just not both at once.
    #[test]
    fn real_positions_are_accepted() {
        for (lat, lon) in [
            (50.0333, 8.5706), // EDDF
            (0.0, 32.4),       // on the equator (Entebbe-ish)
            (51.5, 0.0),       // on the prime meridian (London)
            (-33.9, 151.2),    // Sydney
        ] {
            let snap = SimSnapshot {
                lat,
                lon,
                ..SimSnapshot::default()
            };
            assert!(
                snapshot_position_is_usable(&snap),
                "({lat}, {lon}) is a real place and must be accepted"
            );
        }
    }
