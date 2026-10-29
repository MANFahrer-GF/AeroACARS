    use super::*;

    #[test]
    fn der_bericht_nennt_die_teuren_abschnitte_in_reihenfolge() {
        let mut uhr = Taktuhr::default();
        uhr.abschnitt("vor_dem_start");
        assert!(uhr.anteile.is_empty(), "ohne Start wird nichts gezaehlt");
        uhr.start();
        uhr.anteile = vec![("billig", 2), ("szenerie_ziel", 420), ("step_flight", 12)];
        // Rest < 5 ms faellt heraus wie jeder billige Abschnitt.
        assert_eq!(uhr.bericht(), "szenerie_ziel=420 step_flight=12");
        uhr.start();
        assert!(uhr.anteile.is_empty(), "der neue Takt beginnt leer");
    }
