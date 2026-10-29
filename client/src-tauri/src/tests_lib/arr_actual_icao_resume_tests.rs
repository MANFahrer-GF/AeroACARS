    use super::*;

    /// Runde 3 (externe Gegenpruefung, Codex, adversarial, 04.09.2026):
    /// `arr_actual_icao` muss den Save→Restore-Rundlauf ueberleben — sonst
    /// verliert ein Divert ohne `divert_hint` sein bekanntes Ist-ICAO bei
    /// jedem App-Neustart in BlocksOn/Arrived, weil
    /// `settle_at_arrival_stand` (die einzige Setzstelle) dann nicht mehr
    /// erneut aufgerufen wird.
    #[test]
    fn arr_actual_icao_ueberlebt_snapshot_rundlauf() {
        let mut stats = FlightStats::default();
        stats.arr_actual_icao = Some("LEMG".to_string());
        let snapshot = PersistedFlightStats::snapshot_from(&stats);
        let mut restored = FlightStats::default();
        snapshot.apply_to(&mut restored);
        assert_eq!(restored.arr_actual_icao.as_deref(), Some("LEMG"));
    }

    /// 17.10.2026 („Stück 2"): die von v2 beobachtete Reiseflughöhe
    /// überlebt einen Neustart — sonst hinge die Level-off-Regel im
    /// Sinkflug danach am 240-s-Ersatz.
    #[test]
    fn v2_reiseflughoehe_ueberlebt_snapshot_rundlauf() {
        let mut stats = FlightStats::default();
        let t0 = Utc::now();
        for i in 0..5 {
            stats.shadow_engine.step(
                t0 + chrono::Duration::seconds(3 * i),
                37000.0,
                36000.0,
                0.0,
                false,
                FlightPhase::Cruise,
                None,
            );
        }
        assert_eq!(stats.shadow_engine.beobachtete_reiseflughoehe_ft(), Some(37000.0));
        let json = serde_json::to_string(&PersistedFlightStats::snapshot_from(&stats)).expect("ser");
        let snapshot: PersistedFlightStats = serde_json::from_str(&json).expect("de");
        let mut restored = FlightStats::default();
        snapshot.apply_to(&mut restored);
        assert_eq!(restored.shadow_engine.beobachtete_reiseflughoehe_ft(), Some(37000.0));
    }
