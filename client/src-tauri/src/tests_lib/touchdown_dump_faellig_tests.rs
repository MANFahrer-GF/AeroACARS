    use super::*;

    #[test]
    fn kein_dump_vor_dem_fenster_ohne_klettern() {
        assert!(!touchdown_dump_faellig(0, 0.0, true));
        assert!(!touchdown_dump_faellig(crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert - 1, 5.0, false));
    }

    #[test]
    fn dump_nach_dem_vollen_fenster_auch_ohne_klettern() {
        assert!(touchdown_dump_faellig(crate::grenzwerte::grenzwerte().landung.touchdown_post_window_ms.wert, 0.0, true));
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
            crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_agl_threshold_ft.wert + 1.0,
            false
        ));
    }

    #[test]
    fn genau_auf_der_schwelle_loest_noch_nicht_aus() {
        // Gegenprobe zur `>`-Grenze: exakt auf der Schwelle ist noch KEIN
        // Klettern darueber.
        assert!(!touchdown_dump_faellig(
            1_500,
            crate::grenzwerte::grenzwerte().phasen_kern.touch_and_go_agl_threshold_ft.wert,
            false
        ));
    }

    /// DrmAg9Ve55BeQaV1 (07.11.2026): 3 698 ms nach dem Aufsetzen ein einzelner
    /// AGL-Wert von 715 ft bei `on_ground=true`. Das ist kein Wegsteigen.
    #[test]
    fn agl_ausreisser_am_boden_schliesst_das_fenster_nicht() {
        assert!(!touchdown_dump_faellig(3_698, 715.3, true));
        assert!(!wieder_weggestiegen(715.3, true));
        // Gegenprobe: derselbe Wert in der Luft ist ein echter Hopser.
        assert!(touchdown_dump_faellig(3_698, 715.3, false));
        assert!(wieder_weggestiegen(715.3, false));
    }
