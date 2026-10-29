    use super::*;

    #[test]
    fn gemeldet_wird_ab_der_zweiten_abfrage_bei_zweierpotenzen() {
        let gemeldet: Vec<u32> = (1..=20).filter(|&n| abfrage_melden(n)).collect();
        assert_eq!(gemeldet, vec![2, 4, 8, 16]);
    }

    #[test]
    fn gezaehlt_wird_je_anzeige_und_landung() {
        assert_eq!(landungsabfrage_zaehlen("test_band", "P1@1"), 1);
        assert_eq!(landungsabfrage_zaehlen("test_band", "P1@1"), 2);
        assert_eq!(
            landungsabfrage_zaehlen("test_panel", "P1@1"),
            1,
            "andere Anzeige"
        );
        assert_eq!(
            landungsabfrage_zaehlen("test_band", "P1@2"),
            1,
            "andere Landung"
        );
    }
