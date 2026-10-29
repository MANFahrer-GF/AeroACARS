    use super::*;

    /// Nach dem Resume ist `last_lat/lon` leer — der Vergleich muss den
    /// dauerhaften Punkt nehmen, sonst gilt jede Sim-Position als passend.
    #[test]
    fn resume_vergleicht_mit_dem_dauerhaften_punkt() {
        let mut stats = FlightStats::default();
        stats.last_known_lat = Some(21.2929);
        stats.last_known_lon = Some(86.2577);
        stats.last_lat = None;
        stats.last_lon = None;
        assert_eq!(gespeicherter_punkt(&stats), Some((21.2929, 86.2577)));
        // Sim nach dem Absturz in Singapur, in der Luft gespeichert → verdächtig.
        stats.phase = FlightPhase::Cruise;
        let jetzt = SimSnapshot {
            lat: 1.3502,
            lon: 103.984,
            altitude_msl_ft: 22.0,
            on_ground: true,
            ..SimSnapshot::default()
        };
        let referenz = lage_referenz(&stats).expect("Referenz");
        assert_eq!((referenz.lat, referenz.lon), (21.2929, 86.2577));
        assert!(lage_abweichung(&referenz, &jetzt, Utc::now(), true).is_some());
        // Ohne dauerhaften Punkt: die alte Basis als Rückfall.
        let mut alt = FlightStats::default();
        alt.last_lat = Some(50.0);
        alt.last_lon = Some(8.5);
        assert_eq!(gespeicherter_punkt(&alt), Some((50.0, 8.5)));
    }
