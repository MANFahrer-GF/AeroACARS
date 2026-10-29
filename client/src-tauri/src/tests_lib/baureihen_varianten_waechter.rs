    use super::aircraft_limits_exakt;
    use landing_scoring::spurweite::{spannweite_m, spurweite_m};

    fn in_beiden(c: &str) -> bool {
        !aircraft_limits_exakt(c).is_fallback
            && spurweite_m(Some(c)).is_some()
            && spannweite_m(Some(c)).is_some()
    }

    #[test]
    fn jede_variante_steht_in_grenzwerten_und_spurweite() {
        for c in sim_core::BAUREIHEN_VARIANTEN {
            assert!(in_beiden(c), "{c} fehlt in Grenzwerten oder Spurweite");
        }
        assert!(!in_beiden("B74S"), "Gegenprobe: B74S kennt keine Tabelle");
    }
