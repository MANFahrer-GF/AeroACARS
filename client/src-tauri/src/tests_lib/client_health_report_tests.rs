    use super::*;

    #[test]
    fn returns_none_for_unaffected_flight() {
        let stats = FlightStats::new();
        assert!(build_client_health_report(&stats).is_none());
    }

    #[test]
    fn returns_none_when_only_gap_minutes_is_set() {
        // A resume with no unclean exit and no discontinuity is boring —
        // nothing worth reporting, even though resume_gap_minutes is Some.
        let mut stats = FlightStats::new();
        stats.resume_gap_minutes = Some(5);
        assert!(build_client_health_report(&stats).is_none());
    }

    /// QS-Fix (Finding 3b): a CLEAN app restart (`Some(false)`) must be
    /// treated the same as `None` — before the fix, `Some(false)` alone
    /// defeated the early-return guard, so every uneventful resumed
    /// flight shipped a non-`None` `client_health`.
    #[test]
    fn returns_none_when_app_restart_was_clean() {
        let mut stats = FlightStats::new();
        stats.app_restart_was_unclean = Some(false);
        stats.resume_gap_minutes = Some(3);
        assert!(build_client_health_report(&stats).is_none());
    }

    #[test]
    fn incident_replay_produces_full_report() {
        let mut stats = FlightStats::new();
        stats.disconnect_sim_liveness = Some(sim_core::process_probe::ProcessLiveness::Unknown);
        stats.app_restart_was_unclean = Some(true);
        stats.resume_gap_minutes = Some(98);
        stats.resume_discontinuity = Some(ResumeDiscontinuity {
            drift_nm: 0.0,
            altitude_delta_ft: 0.0,
            fuel_delta_kg: 7683.3,
            both_grounded: false,
            ziel_vorher_nm: None,
            ziel_nachher_nm: None,
            danach_am_boden: false,
            luecke_secs: None,
            nur_bestaetigt: false,
        });
        let report = build_client_health_report(&stats).expect("must be Some");
        assert_eq!(report.disconnect_sim_liveness.as_deref(), Some("unknown"));
        assert_eq!(report.app_restart_unclean, Some(true));
        assert_eq!(report.impossible_resume_jump, Some(true));
        assert_eq!(report.resume_fuel_delta_kg, Some(7683.3));
        assert_eq!(report.resume_gap_minutes, Some(98));
    }

    /// Bei blosser Bestaetigung („Trotzdem fortsetzen") gibt es keine
    /// gemessenen Deltas — der Bericht darf keine erfundenen Nullwerte
    /// melden. Die Abweichung selbst bleibt gemeldet (Cloud-QS 23.09.2026).
    #[test]
    fn bestaetigung_ohne_messwerte_meldet_keine_deltas() {
        let mut stats = FlightStats::new();
        stats.resume_gap_minutes = Some(11);
        stats.resume_discontinuity = Some(ResumeDiscontinuity {
            drift_nm: 1.4,
            altitude_delta_ft: 0.0,
            fuel_delta_kg: 0.0,
            both_grounded: false,
            ziel_vorher_nm: None,
            ziel_nachher_nm: None,
            danach_am_boden: true,
            luecke_secs: Some(660),
            nur_bestaetigt: true,
        });
        let report = build_client_health_report(&stats).expect("must be Some");
        assert_eq!(report.impossible_resume_jump, Some(true));
        assert_eq!(report.resume_fuel_delta_kg, None);
        assert_eq!(report.resume_altitude_delta_ft, None);
    }
