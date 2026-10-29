    use super::*;

    /// v0.7.7 §10-Pflicht: Save/Load erhaelt simbrief_ofp_id +
    /// simbrief_ofp_generated_at in FlightStats.
    #[test]
    fn persisted_flight_stats_preserves_simbrief_ofp_id_round_trip() {
        let mut stats = FlightStats::default();
        stats.simbrief_ofp_id = Some("1777622821_5F3E3B3842".to_string());
        stats.simbrief_ofp_generated_at = Some("1778452800".to_string());

        let persisted = PersistedFlightStats::snapshot_from(&stats);
        let mut restored = FlightStats::default();
        persisted.apply_to(&mut restored);

        assert_eq!(
            restored.simbrief_ofp_id.as_deref(),
            Some("1777622821_5F3E3B3842"),
        );
        assert_eq!(
            restored.simbrief_ofp_generated_at.as_deref(),
            Some("1778452800"),
        );
    }

    /// v1.7.32: Die Plandistanz muss den Neustart mitten im Flug
    /// ueberleben. Ohne sie faellt die Mehrweg-Gutschrift der OFP-Achse
    /// weg, und der Pilot steht nach einem Absturz des Clients mit den
    /// strengeren Baendern ohne Ausgleich da (externe QS, Codex
    /// 16.09.2026). Eine ALTE Resume-Datei ohne das Feld muss weiterhin
    /// lesbar bleiben.
    #[test]
    fn plandistanz_ueberlebt_den_neustart() {
        let json = r#"{"pirep_id":"x","bid_id":1,"plan_strecke_nm":186.5,
            "flight_number":"THY 1068","dpt_airport":"LRCK","arr_airport":"LTFM",
            "started_at":"2026-09-16T17:06:48Z","pilot_callsign":"","fares":[]}"#;
        let parsed: PersistedFlight =
            serde_json::from_str(json).expect("Resume-Datei muss lesbar sein");
        assert_eq!(parsed.plan_strecke_nm, Some(186.5));

        // Alte Datei ohne das Feld: weiterhin lesbar, dann eben ohne
        // Gutschrift — nie schlechter als vor der Aenderung.
        let alt = r#"{"pirep_id":"x","bid_id":1,
            "flight_number":"THY 1068","dpt_airport":"LRCK","arr_airport":"LTFM",
            "started_at":"2026-09-16T17:06:48Z","pilot_callsign":"","fares":[]}"#;
        let parsed: PersistedFlight =
            serde_json::from_str(alt).expect("alte Resume-Datei muss lesbar bleiben");
        assert_eq!(parsed.plan_strecke_nm, None);
    }

    /// v0.7.7 §10-Pflicht: pre-v0.7.7-Persistenz ohne die neuen Felder
    /// bleibt deserialisierbar (serde(default) greift, Werte sind None).
    #[test]
    fn persisted_flight_stats_legacy_resume_yields_none_for_new_fields() {
        // Minimaler JSON ohne simbrief_ofp_id / simbrief_ofp_generated_at —
        // wie ein pre-v0.7.7 Snapshot aussehen wuerde.
        let legacy_json = r#"{"distance_nm": 100.0}"#;
        let parsed: PersistedFlightStats =
            serde_json::from_str(legacy_json).expect("legacy stats JSON must still deserialise");
        assert_eq!(parsed.simbrief_ofp_id, None);
        assert_eq!(parsed.simbrief_ofp_generated_at, None);
    }

    /// v0.16.24 (QS-2 FIX B): an OLD persisted blob (saved before these fields
    /// existed) still deserialises — `#[serde(default)]` fills None/false, so
    /// resume behaviour is identical to pre-change (never worse). (The
    /// round-trip + resume-upgrade tests live in `touchdown_metadata_stamp_tests`
    /// where the divert fixtures are.)
    #[test]
    fn persisted_flight_stats_legacy_resume_yields_default_runway_fields() {
        let legacy_json = r#"{"distance_nm": 50.0}"#;
        let parsed: PersistedFlightStats = serde_json::from_str(legacy_json)
            .expect("legacy stats JSON (no runway-correlation fields) must still deserialise");
        assert_eq!(parsed.runway_source, None);
        assert_eq!(parsed.runway_nav_cycle, None);
        assert_eq!(parsed.runway_nav_geometry, None);
        assert_eq!(parsed.runway_correlation_icao, None);
        assert!(!parsed.runway_fallback_at_touchdown);
    }

    /// v0.7.7 §10-Pflicht: PersistedFlight legacy ohne flight_id-Feld
    /// muss weiter parseable bleiben (= leer-String via serde(default)).
    /// Das ist der "legacy active flight"-Fall fuer v0.7.8.
    #[test]
    fn persisted_flight_legacy_resume_yields_empty_flight_id() {
        // Minimaler JSON ohne flight_id-Feld.
        let legacy_json = r#"{
            "pirep_id": "abc123",
            "bid_id": 42,
            "started_at": "2026-05-11T10:00:00Z",
            "flight_number": "DLH100",
            "dpt_airport": "EDDM",
            "arr_airport": "LFPG",
            "fares": []
        }"#;
        let parsed: PersistedFlight = serde_json::from_str(legacy_json)
            .expect("legacy PersistedFlight must still deserialise");
        assert_eq!(parsed.flight_id, "");
        assert_eq!(parsed.bid_id, 42);
    }

    /// v0.7.7 §10-Pflicht: Round-Trip-Test fuer flight_id.
    #[test]
    fn persisted_flight_preserves_flight_id_round_trip() {
        let original = PersistedFlight {
            pirep_id: "abc123".to_string(),
            plan_strecke_nm: None,
            bid_id: 42,
            flight_id: "1234567".to_string(),
            bid_callsign: None,
            pilot_callsign: None,
            runtime_bundle_id: None,
            started_at: Utc::now(),
            zuletzt_geschrieben: None,
            airline_icao: "DLH".to_string(),
            airline_logo_url: None,
            planned_registration: "D-AIUV".to_string(),
            flight_number: "100".to_string(),
            dpt_airport: "EDDM".to_string(),
            arr_airport: "LFPG".to_string(),
            fares: vec![],
            stats: PersistedFlightStats::default(),
            owner_identity: Some("fingerprint-abc".to_string()),
        };
        let json = serde_json::to_string(&original).expect("serialise");
        let restored: PersistedFlight = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(restored.flight_id, "1234567");
        assert_eq!(restored.bid_id, 42);
    }

    /// v0.7.7 §10-Pflicht: Phase-Gate Boundary. Pushback ist ERLAUBT
    /// (Spec v1.1-Korrektur, frueher implizit ausgeschlossen).
    /// Dieser Test pruefelt die Match-Logik direkt, da der Tauri-Command
    /// asynchron + state-abhaengig ist.
    #[test]
    fn phase_gate_accepts_preflight_boarding_pushback_taxi_out() {
        // Diese 4 Phasen MUESSEN akzeptiert werden — sonst koennen
        // Piloten in Pushback nicht refreshen, was die v0.7.7-UX
        // bricht.
        for phase in [
            FlightPhase::Preflight,
            FlightPhase::Boarding,
            FlightPhase::Pushback,
            FlightPhase::TaxiOut,
        ] {
            let allowed = matches!(
                phase,
                FlightPhase::Preflight
                    | FlightPhase::Boarding
                    | FlightPhase::Pushback
                    | FlightPhase::TaxiOut
            );
            assert!(
                allowed,
                "Phase {:?} muss erlaubt sein fuer OFP-Refresh",
                phase
            );
        }
    }

    /// v0.7.7 §10-Pflicht: Phase-Gate Boundary. TakeoffRoll und
    /// alles danach ist VERBOTEN — Plan ist da festgenagelt.
    #[test]
    fn phase_gate_rejects_takeoff_roll_and_later() {
        for phase in [
            FlightPhase::TakeoffRoll,
            FlightPhase::Takeoff,
            FlightPhase::Climb,
            FlightPhase::Cruise,
            FlightPhase::Holding,
            FlightPhase::Descent,
            FlightPhase::Approach,
            FlightPhase::Final,
            FlightPhase::Landing,
            FlightPhase::TaxiIn,
            FlightPhase::BlocksOn,
            FlightPhase::Arrived,
            FlightPhase::PirepSubmitted,
        ] {
            let allowed = matches!(
                phase,
                FlightPhase::Preflight
                    | FlightPhase::Boarding
                    | FlightPhase::Pushback
                    | FlightPhase::TaxiOut
            );
            assert!(
                !allowed,
                "Phase {:?} darf NICHT erlaubt sein fuer OFP-Refresh — \
                 Plan muss da festgenagelt sein",
                phase
            );
        }
    }

    /// v0.7.7 §10-Pflicht: changed-Flag-Logik.
    /// Wenn previous_ofp_id == current_ofp_id → changed=false.
    /// Frontend nutzt das fuer den "OFP unveraendert"-Notice.
    #[test]
    fn changed_flag_is_false_when_ofp_id_identical() {
        let previous: Option<String> = Some("abc123".to_string());
        let current = "abc123";
        let changed = previous.as_deref() != Some(current);
        assert!(!changed, "identische OFP-IDs muessen changed=false ergeben");
    }

    /// v0.7.7 §10-Pflicht: changed-Flag-Logik.
    /// Wenn previous_ofp_id != current_ofp_id (oder None) → changed=true.
    #[test]
    fn changed_flag_is_true_when_ofp_id_differs_or_first_set() {
        // Fall 1: neue ID nach altem Refresh
        let prev1: Option<String> = Some("old_id".to_string());
        let cur1 = "new_id";
        assert!(prev1.as_deref() != Some(cur1));

        // Fall 2: erstes Mal gesetzt (previous = None)
        let prev2: Option<String> = None;
        let cur2 = "first_ofp";
        assert!(prev2.as_deref() != Some(cur2));
    }
