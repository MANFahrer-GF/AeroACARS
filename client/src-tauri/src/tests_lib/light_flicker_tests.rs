    use super::*;
    use std::collections::HashMap;

    fn at(sec: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000 + sec, 0).expect("timestamp")
    }

    /// Ein normal bedienter Schalter wird protokolliert wie bisher.
    #[test]
    fn a_normal_switch_is_still_logged() {
        let mut st = HashMap::new();
        assert_eq!(
            flicker_verdict(&mut st, "Landing", at(0)),
            FlickerVerdict::Log
        );
        // Zweiter Wechsel eine Minute später — neues Fenster.
        assert_eq!(
            flicker_verdict(&mut st, "Landing", at(60)),
            FlickerVerdict::Log
        );
        assert_eq!(
            flicker_verdict(&mut st, "Landing", at(400)),
            FlickerVerdict::Log
        );
    }

    /// Der Feldfall: FLASH-Modus auf einer Boeing. Gemessen am Flug
    /// EDDP→LTFE (25.07.2026): 50 Wechsel in fünf Minuten, Median-
    /// Halteperiode 3,0 s. Erwartung: ein paar erste Zeilen, dann EIN
    /// "Blinkmodus", danach Ruhe — statt fünfzig Zeilen.
    #[test]
    fn flash_mode_is_named_once_instead_of_flooding() {
        let mut st = HashMap::new();
        let mut logged = 0;
        let mut announced = 0;
        let mut suppressed = 0;
        // 100 Wechsel im 3-Sekunden-Takt = die gemessenen fünf Minuten.
        for i in 0..100 {
            match flicker_verdict(&mut st, "Landing", at(i * 3)) {
                FlickerVerdict::Log => logged += 1,
                FlickerVerdict::AnnounceFlashing => announced += 1,
                FlickerVerdict::Suppress => suppressed += 1,
            }
        }
        assert_eq!(announced, 1, "Blinken genau einmal benennen");
        assert!(logged <= 3, "hoechstens die ersten Flanken, war {logged}");
        assert!(
            suppressed > 90,
            "der Rest muss geschluckt werden, war {suppressed}"
        );
    }

    /// Blinken an EINER Lampe darf normale Schaltvorgaenge an einer anderen
    /// nicht mitverschlucken — ein gemeinsamer Zaehler taete genau das.
    #[test]
    fn flashing_landing_lights_do_not_mute_the_nav_lights() {
        let mut st = HashMap::new();
        for i in 0..20 {
            flicker_verdict(&mut st, "Landing", at(i * 3));
        }
        assert_eq!(
            flicker_verdict(&mut st, "Nav", at(30)),
            FlickerVerdict::Log,
            "die Nav-Lampe hat gar nicht geblinkt"
        );
    }

    /// Hoert das Blinken auf, wird der naechste echte Schaltvorgang wieder
    /// normal protokolliert — sonst bliebe die Lampe fuer den Rest des
    /// Fluges stumm.
    #[test]
    fn logging_resumes_after_the_flashing_stops() {
        let mut st = HashMap::new();
        for i in 0..20 {
            flicker_verdict(&mut st, "Landing", at(i * 3));
        }
        // Ruhe, dann ein einzelner Schaltvorgang.
        assert_eq!(
            flicker_verdict(&mut st, "Landing", at(20 * 3 + crate::grenzwerte::grenzwerte().cockpit.flicker_window_secs.wert + 5)),
            FlickerVerdict::Log
        );
    }
