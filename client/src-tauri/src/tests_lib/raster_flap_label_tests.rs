    use super::raster_flap_label;

    /// X-Plane, Laminar Boeing 737-800 (gemessen 29.09.2026, X-Plane 12.4.3):
    /// 8 Rasten, Hebel 0 … 1 in Achteln → Index 0..=8 (Test
    /// `klappenraste_laminar_737_gemessen` im X-Plane-Adapter). Im
    /// Aktivitaetslog die Boeing-Namen.
    /// X-Plane, Laminar Airbus A330-300 (gemessen 29.09.2026, X-Plane
    /// 12.4.3): 4 Rasten, Hebel in Vierteln → Index 0..=4, ohne eigene
    /// 1+F-Raste. Bei Airbus-ICAO mit 4 Rasten die 4-Rasten-Namen, nicht die
    /// 5-Rasten-Tabelle mit 1+F und nicht verschoben.
    #[test]
    fn laminar_a330_vier_rasten_ohne_eins_plus_f() {
        let l: Vec<_> = (0..=4)
            .map(|i| raster_flap_label(4, i, Some("A333")))
            .collect();
        assert_eq!(l, ["0", "1", "2", "3", "FULL"].map(Some));
        assert_eq!(raster_flap_label(4, 5, Some("A333")), None);
    }

    #[test]
    fn laminar_737_acht_rasten_boeing_namen() {
        let l: Vec<_> = (0..=8)
            .map(|i| raster_flap_label(8, i, Some("B738")))
            .collect();
        let soll = ["UP", "1", "2", "5", "10", "15", "25", "30", "40"].map(Some);
        assert_eq!(l, soll);
    }

    #[test]
    fn fenix_fuenf_rasten_mit_eins_plus_f() {
        let l: Vec<_> = (0..=5)
            .map(|i| raster_flap_label(5, i, Some("A320")))
            .collect();
        let soll = ["0", "1", "1+F", "2", "3", "FULL"].map(Some);
        assert_eq!(l, soll);
        assert_eq!(raster_flap_label(5, 6, Some("A320")), None);
    }

    #[test]
    fn fuenf_rasten_ohne_airbus_bleiben_ohne_namen() {
        // CRJ: ebenfalls 5 Rasten, aber 1/8/20/30/45 — nicht raten.
        assert_eq!(raster_flap_label(5, 2, Some("CRJ7")), None);
        assert_eq!(raster_flap_label(5, 2, None), None);
    }

    #[test]
    fn bisherige_tabellen_unveraendert() {
        assert_eq!(raster_flap_label(4, 4, Some("A320")), Some("FULL"));
        assert_eq!(raster_flap_label(8, 3, Some("B738")), Some("5"));
        assert_eq!(raster_flap_label(6, 2, None), Some("5"));
    }
