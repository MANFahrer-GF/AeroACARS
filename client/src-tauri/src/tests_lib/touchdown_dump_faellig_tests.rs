    use super::*;

    #[test]
    fn kein_dump_vor_dem_fenster_ohne_klettern() {
        assert!(!touchdown_dump_faellig(0, 0.0));
        assert!(!touchdown_dump_faellig(crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert - 1, 5.0));
    }

    #[test]
    fn dump_nach_dem_vollen_fenster_auch_ohne_klettern() {
        assert!(touchdown_dump_faellig(crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert, 0.0));
    }

    /// Der eigentliche Fix: ein Bounce, der WEIT VOR den vollen 10 s wieder
    /// ueber die T&G-AGL-Schwelle klettert, muss den Dump sofort faellig
    /// machen. Sonst bleibt der Sampler-Edge-Detector auf dem ersten
    /// Touchdown haengen (`sampler_touchdown_at.is_none()`-Guard) und ein
    /// schnellerer, schlimmerer zweiter Aufsetzer — real: T&G bei
    /// -1499 fpm, Aufprall Sekunden spaeter bei -3007 fpm — faellt nie durch
    /// den Edge-Detector, `apply_accident_heuristic` sieht ihn nie.
    #[test]
    fn fruehes_klettern_ueber_die_tg_schwelle_macht_den_dump_sofort_faellig() {
        assert!(touchdown_dump_faellig(
            1_500,
            crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_agl_threshold_ft.wert + 1.0
        ));
    }

    #[test]
    fn genau_auf_der_schwelle_loest_noch_nicht_aus() {
        // Gegenprobe zur `>`-Grenze: exakt auf der Schwelle ist noch KEIN
        // Klettern darueber.
        assert!(!touchdown_dump_faellig(
            1_500,
            crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_agl_threshold_ft.wert
        ));
    }
