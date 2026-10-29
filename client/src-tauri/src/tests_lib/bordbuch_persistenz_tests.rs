    use super::*;

    /// SWR 269 (26.09.2026): nach einem Absturz im Steigflug war das
    /// Bordbuch leer. Rundreise Speichern → Laden muss den Zustand behalten.
    #[test]
    fn bordbuch_ueberlebt_speichern_und_laden() {
        let mut stats = FlightStats::default();
        let e = bordbuch::Einstellungen::default();
        let k = bordbuch::Kontext {
            klasse: bordbuch::Klasse::Airliner,
            einstellungen: &e,
            abflug: Some("LSZH"),
            ziel: Some("EDDC"),
        };
        let mut s = SimSnapshot::default();
        s.timestamp = Utc::now();
        s.on_ground = true;
        s.engines_running = 2;
        s.aircraft_title = Some("A220-300".into());
        s.light_beacon = Some(true);
        bordbuch::tick(&mut stats.bordbuch, &s, FlightPhase::Pushback, &k);
        s.timestamp += chrono::Duration::seconds(3);
        s.light_beacon = Some(false);
        bordbuch::tick(&mut stats.bordbuch, &s, FlightPhase::TaxiOut, &k);
        assert!(!stats.bordbuch.punkte.is_empty());
        let vorher = serde_json::to_value(&stats.bordbuch).unwrap();

        let snap = PersistedFlightStats::snapshot_from(&stats);
        let json = serde_json::to_string(&snap).unwrap();
        let zurueck: PersistedFlightStats = serde_json::from_str(&json).unwrap();
        let mut neu = FlightStats::default();
        zurueck.apply_to(&mut neu);
        assert_eq!(serde_json::to_value(&neu.bordbuch).unwrap(), vorher);
    }
