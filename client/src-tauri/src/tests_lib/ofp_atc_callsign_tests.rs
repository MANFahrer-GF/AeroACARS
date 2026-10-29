    use super::*;

    #[test]
    fn ofp_callsign_accepts_icao_identifications() {
        assert_eq!(ofp_atc_callsign("DLH4TK").as_deref(), Some("DLH4TK"));
        assert_eq!(ofp_atc_callsign("ewg9kc").as_deref(), Some("EWG9KC"));
        assert_eq!(ofp_atc_callsign(" cfg 1tk ").as_deref(), Some("CFG1TK"));
        assert_eq!(ofp_atc_callsign("GSG0001").as_deref(), Some("GSG0001"));
        assert_eq!(ofp_atc_callsign("N123AB").as_deref(), Some("N123AB"));
    }

    #[test]
    fn ofp_callsign_rejects_plain_flight_numbers_and_junk() {
        // Ohne <atc><callsign> liefert der Parser die reine Flugnummer.
        assert_eq!(ofp_atc_callsign("371"), None);
        assert_eq!(ofp_atc_callsign(""), None);
        assert_eq!(ofp_atc_callsign("DLH12345"), None);
        assert_eq!(ofp_atc_callsign("DLH-371"), None);
        assert_eq!(ofp_atc_callsign("A"), None);
        assert_eq!(ofp_atc_callsign("ÄBC12"), None);
    }

    #[test]
    fn hoppie_uses_ofp_callsign_else_airline_and_number() {
        assert_eq!(flight_atc_callsign(Some("DLH4TK"), "DLH", "371"), "DLH4TK");
        assert_eq!(flight_atc_callsign(None, "DLH", "371"), "DLH371");
        assert_eq!(flight_atc_callsign(Some("  "), "DLH", "371"), "DLH371");
    }

    #[test]
    fn ofp_callsign_survives_resume_snapshot() {
        let mut stats = FlightStats::new();
        stats.planned_atc_callsign = Some("EWG9KC".to_string());
        let json = serde_json::to_string(&PersistedFlightStats::snapshot_from(&stats)).unwrap();
        let restored: PersistedFlightStats = serde_json::from_str(&json).unwrap();
        let mut back = FlightStats::new();
        restored.apply_to(&mut back);
        assert_eq!(back.planned_atc_callsign.as_deref(), Some("EWG9KC"));
    }

    #[test]
    fn old_resume_file_without_callsign_still_loads() {
        let stats = FlightStats::new();
        let mut value = serde_json::to_value(PersistedFlightStats::snapshot_from(&stats)).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("planned_atc_callsign");
        let restored: PersistedFlightStats = serde_json::from_value(value).unwrap();
        let mut back = FlightStats::new();
        restored.apply_to(&mut back);
        assert_eq!(back.planned_atc_callsign, None);
    }
