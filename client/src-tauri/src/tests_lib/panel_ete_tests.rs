    use super::*;

    #[test]
    fn ete_uses_distance_to_go_not_distance_flown() {
        // 60 nm vor dem (echten) Flughafen EDDB mit 120 kt: 30 Minuten.
        let (alat, alon) = runway::airport_position("EDDB").expect("EDDB in OurAirports");
        // 1° Breite = 60 nm: eine Position genau 1° südlich der Referenz.
        let snap = SimSnapshot {
            lat: alat - 1.0,
            lon: alon,
            groundspeed_kt: 120.0,
            ..Default::default()
        };
        let ete = panel_ete_min(&snap, "EDDB", None).expect("ete");
        // Großkreis 1° Breite = 60,04 nm → 30 min, Rundung ±1.
        assert!((29..=31).contains(&ete), "ete war {ete}");
    }

    #[test]
    fn ete_gates_on_taxi_speeds_and_unknown_airports() {
        let snap = SimSnapshot {
            lat: 52.0,
            lon: 13.0,
            groundspeed_kt: 20.0,
            ..Default::default()
        };
        assert_eq!(
            panel_ete_min(&snap, "EDDB", None),
            None,
            "unter 30 kt kein ETE"
        );
        let snap2 = SimSnapshot {
            groundspeed_kt: 400.0,
            ..snap
        };
        assert_eq!(
            panel_ete_min(&snap2, "XXXX", None),
            None,
            "unbekanntes Ziel"
        );
    }

    /// v1.5.3: der Plausibilitaets-Deckel — im Steigflug mit halber
    /// Reise-GS ist die Momentan-ETE keine Prognose. Erst unter dem
    /// 1,3-fachen der Planzeit erscheint eine Zahl.
    #[test]
    fn ete_hides_until_below_plan_ceiling() {
        let (alat, alon) = runway::airport_position("EDDB").expect("EDDB");
        // 600 nm vor dem Ziel mit 200 kt -> 180 min Momentan-ETE.
        let snap = SimSnapshot {
            lat: alat - 10.0,
            lon: alon,
            groundspeed_kt: 200.0,
            ..Default::default()
        };
        // Planzeit 120 min: 180 > 156 -> versteckt.
        assert_eq!(panel_ete_min(&snap, "EDDB", Some(120)), None);
        // Planzeit 150 min: 180 < 195 -> sichtbar.
        assert!(panel_ete_min(&snap, "EDDB", Some(150)).is_some());
        // Ohne Planzeit: altes Verhalten, sichtbar.
        assert!(panel_ete_min(&snap, "EDDB", None).is_some());
    }
