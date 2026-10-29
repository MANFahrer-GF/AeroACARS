    use super::*;

    /// Der Anflug-Streifen und die Ergebnisanzeige zeigen dieselben zwei
    /// Windwerte im Abstand von zwei Minuten. Laufen die Vorzeichen
    /// auseinander, wird aus 12 Knoten Gegenwind nach der Landung plötzlich
    /// −12, und niemand merkt, welche der beiden Anzeigen lügt. Dieser Test
    /// bindet beide Ableitungen aneinander.
    #[test]
    fn wind_signs_match_the_landing_record() {
        let snap = SimSnapshot {
            // Positiv = Rückenwind laut SimVar-Konvention …
            aircraft_wind_z_kt: Some(9.0),
            // … positiv = Wind von rechts.
            aircraft_wind_x_kt: Some(6.0),
            ..Default::default()
        };

        let live = PanelLiveSnapshot::from_snapshot(&snap);
        // 9 kt Rückenwind sind −9 kt Gegenwind.
        assert_eq!(live.headwind_kt, Some(-9.0));
        assert_eq!(live.crosswind_kt, Some(6.0));

        // Exakt die Ableitung aus dem Touchdown-Pfad, hier gespiegelt:
        // weicht eine der beiden Stellen ab, fällt es hier auf.
        assert_eq!(live.headwind_kt, snap.aircraft_wind_z_kt.map(|z| -z));
        assert_eq!(live.crosswind_kt, snap.aircraft_wind_x_kt);
    }

    /// Sims und Addons ohne die beiden SimVars sollen nicht stillschweigend
    /// als „windstill“ durchgehen — die Anzeige muss den Unterschied zwischen
    /// „kein Wind“ und „kein Messwert“ treffen können.
    #[test]
    fn missing_wind_stays_absent_instead_of_becoming_zero() {
        let snap = SimSnapshot {
            aircraft_wind_z_kt: None,
            aircraft_wind_x_kt: None,
            ..Default::default()
        };
        let live = PanelLiveSnapshot::from_snapshot(&snap);
        assert_eq!(live.headwind_kt, None);
        assert_eq!(live.crosswind_kt, None);
    }
