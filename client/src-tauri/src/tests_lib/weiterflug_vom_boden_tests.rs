    //! GAF 278 (24.10.2026): Nach dem Sim-Absturz auf einem anderen Platz am
    //! Boden neu geladen — der Pilot will von dort starten und zum Ziel
    //! weiterfliegen („Vom Boden weiterfliegen"). Geprueft an `step_flight`.
    use super::*;

    fn abflug() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-24T12:03:44Z").unwrap().with_timezone(&Utc)
    }

    /// Ein Flug im Sinkflug, wie GAF 278 vor dem Absturz.
    fn sinkflug() -> ActiveFlight {
        let flight = crate::enroute_reconcile_replay_tests::replay_fixture();
        {
            let mut st = flight.stats.lock().unwrap();
            st.phase = FlightPhase::Descent;
            st.block_off_at = Some(abflug() - chrono::Duration::minutes(24));
            st.takeoff_at = Some(abflug());
            st.takeoff_fuel_kg = Some(22_900.0);
            st.takeoff_weight_kg = Some(85_343.0);
            st.was_airborne = true;
        }
        flight
    }

    /// Kalt und dunkel am Ausweichplatz: `SimSnapshot::default()` ist „am
    /// Boden, Triebwerke aus, steht".
    fn kalt_am_boden() -> SimSnapshot {
        SimSnapshot::default()
    }

    #[test]
    fn kalt_am_ausweichplatz_bleibt_der_flug_beim_rollen() {
        let flight = sinkflug();
        {
            let mut st = flight.stats.lock().unwrap();
            bodenneustart_vorbereiten(&mut st, Utc::now());
            // Schon eine Minute kalt gestanden.
            st.arrived_fallback_pending_since = Some(Utc::now() - chrono::Duration::seconds(60));
        }
        let ergebnis = step_flight(&flight, &kalt_am_boden());
        assert_ne!(ergebnis, Some(FlightPhase::Arrived));
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::TaxiOut);
    }

    /// Gegenprobe: Ohne den Weiterflug-Merker beendet derselbe Stand den Flug.
    #[test]
    fn ohne_merker_beendet_der_rueckfall_den_flug() {
        let flight = sinkflug();
        {
            let mut st = flight.stats.lock().unwrap();
            bodenneustart_vorbereiten(&mut st, Utc::now());
            st.weiterflug_ab_boden_seit = None;
            st.arrived_fallback_pending_since = Some(Utc::now() - chrono::Duration::seconds(60));
        }
        assert_eq!(step_flight(&flight, &kalt_am_boden()), Some(FlightPhase::Arrived));
    }

    fn abheben() -> SimSnapshot {
        SimSnapshot {
            on_ground: false,
            altitude_agl_ft: 60.0,
            altitude_msl_ft: 420.0,
            vertical_speed_fpm: 1_800.0,
            groundspeed_kt: 160.0,
            fuel_total_kg: 11_700.0,
            total_weight_kg: Some(74_000.0),
            ..SimSnapshot::default()
        }
    }

    /// Der neue Start ueberschreibt Abflugzeit und Startgewicht von Dubai
    /// nicht; der Sprit bleibt ungemessen, und der Merker ist danach weg.
    #[test]
    fn neuer_start_behaelt_die_abflugwerte() {
        let flight = sinkflug();
        {
            let mut st = flight.stats.lock().unwrap();
            bodenneustart_vorbereiten(&mut st, Utc::now());
        }
        assert_eq!(step_flight(&flight, &abheben()), Some(FlightPhase::Takeoff));
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.takeoff_at, Some(abflug()));
        assert_eq!(st.takeoff_weight_kg, Some(85_343.0));
        // Neu geladener Tank: kein Abhebe-Sprit, also kein erfundener Verbrauch.
        assert_eq!(st.takeoff_fuel_kg, None);
        assert!(st.weiterflug_ab_boden_seit.is_none(), "nach dem Abheben wieder scharf");
    }

    /// Gegenprobe: Ein gewoehnlicher Start setzt die Abflugwerte neu.
    #[test]
    fn gewoehnlicher_start_setzt_die_abflugwerte() {
        let flight = sinkflug();
        {
            let mut st = flight.stats.lock().unwrap();
            bodenneustart_vorbereiten(&mut st, Utc::now());
            st.weiterflug_ab_boden_seit = None;
        }
        assert_eq!(step_flight(&flight, &abheben()), Some(FlightPhase::Takeoff));
        assert_ne!(flight.stats.lock().unwrap().takeoff_at, Some(abflug()));
    }

    #[test]
    fn nur_aus_der_luft_am_boden_und_nicht_am_ziel() {
        let mut st = FlightStats::new();
        st.phase = FlightPhase::Descent;
        let am_boden = Some(LageAbweichung::AmBoden);
        assert!(bodenneustart_moeglich(&st, am_boden, false));
        assert!(!bodenneustart_moeglich(&st, am_boden, true), "am Ziel: Arrived + Ziel-Sperre");
        assert!(!bodenneustart_moeglich(&st, Some(LageAbweichung::SpritGestiegen), false));
        assert!(!bodenneustart_moeglich(&st, None, false));
        st.landing_at = Some(Utc::now());
        assert!(!bodenneustart_moeglich(&st, am_boden, false), "Landung angefangen");
        st.landing_at = None;
        st.phase = FlightPhase::TaxiIn;
        assert!(!bodenneustart_moeglich(&st, am_boden, false));
    }

    /// Der Merker ueberlebt einen App-Neustart.
    #[test]
    fn merker_wird_gespeichert() {
        let mut st = FlightStats::new();
        st.weiterflug_ab_boden_seit = Some(abflug());
        let p = PersistedFlightStats::snapshot_from(&st);
        let json = serde_json::to_string(&p).unwrap();
        let zurueck: PersistedFlightStats = serde_json::from_str(&json).unwrap();
        let mut neu = FlightStats::new();
        zurueck.apply_to(&mut neu);
        assert_eq!(neu.weiterflug_ab_boden_seit, Some(abflug()));
    }
