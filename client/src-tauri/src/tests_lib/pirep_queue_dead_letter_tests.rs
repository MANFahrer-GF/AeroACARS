    use super::*;

    #[test]
    fn ohne_wartezeit_ist_der_eintrag_sofort_faellig() {
        assert!(pirep_queue_eintrag_ist_faellig(None, Utc::now()));
    }

    #[test]
    fn vor_ablauf_der_wartezeit_ist_der_eintrag_nicht_faellig() {
        let jetzt = Utc::now();
        let not_before = jetzt + chrono::Duration::seconds(60);
        assert!(!pirep_queue_eintrag_ist_faellig(Some(not_before), jetzt));
    }

    #[test]
    fn nach_ablauf_der_wartezeit_ist_der_eintrag_wieder_faellig() {
        let jetzt = Utc::now();
        let not_before = jetzt - chrono::Duration::seconds(1);
        assert!(pirep_queue_eintrag_ist_faellig(Some(not_before), jetzt));
    }

    /// Der eigentliche Befund (Codex, externe Gegenpruefung): vorher gab
    /// der Worker hier fuer immer und still auf. Die Warnung muss GENAU
    /// beim Ueberschreiten der Schnellphase feuern.
    #[test]
    fn dead_letter_warnung_feuert_beim_ueberschreiten_der_schnellphase() {
        assert!(pirep_queue_dead_letter_warnung_faellig(
            PIREP_QUEUE_MAX_FAST_ATTEMPTS,
            false
        ));
    }

    #[test]
    fn dead_letter_warnung_feuert_nicht_vor_der_schnellphase() {
        assert!(!pirep_queue_dead_letter_warnung_faellig(
            PIREP_QUEUE_MAX_FAST_ATTEMPTS - 1,
            false
        ));
    }

    #[test]
    fn dead_letter_warnung_feuert_kein_zweites_mal() {
        assert!(!pirep_queue_dead_letter_warnung_faellig(
            PIREP_QUEUE_MAX_FAST_ATTEMPTS + 5,
            true
        ));
    }

    #[test]
    fn schnellphase_hat_keine_wartezeit() {
        assert_eq!(
            pirep_queue_slow_phase_wartezeit_sec(PIREP_QUEUE_MAX_FAST_ATTEMPTS - 1),
            None
        );
    }

    #[test]
    fn langsame_phase_wartet_den_festgelegten_takt() {
        assert_eq!(
            pirep_queue_slow_phase_wartezeit_sec(PIREP_QUEUE_MAX_FAST_ATTEMPTS),
            Some(PIREP_QUEUE_SLOW_RETRY_INTERVAL_SECS)
        );
    }
