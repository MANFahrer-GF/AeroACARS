    use super::*;

    /// v1.7.17 Runde 8 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): der eigentliche Befund — ein `RateLimited`-Fehler muss
    /// die VOM SERVER diktierte Wartezeit liefern, nicht das feste
    /// 2s/8s-Schema. `30` liegt sicher innerhalb der Runde-11-Obergrenze
    /// (60s).
    #[test]
    fn rate_limited_liefert_die_servervorgegebene_wartezeit() {
        let fehler = ApiError::RateLimited {
            retry_after_seconds: 30,
        };
        assert_eq!(
            naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 0),
            Some(30)
        );
        assert_eq!(
            naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 1),
            Some(30),
            "die Servervorgabe gilt unabhaengig vom Versuchsindex, nicht das feste Schema"
        );
    }

    #[test]
    fn genau_die_obergrenze_wird_noch_voll_honoriert() {
        let fehler = ApiError::RateLimited {
            retry_after_seconds: RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC,
        };
        assert_eq!(
            naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 0),
            Some(RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC)
        );
    }

    /// v1.7.17 Runde 9 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): ein extremer, ungeprueft aus einem HTTP-Header
    /// geparster Wert darf weder ueberlaufen (`tokio::time::sleep` mit
    /// `u64::MAX` Sekunden) noch den Task praktisch fuer immer schlafen
    /// legen.
    ///
    /// ⚠ Runde 11: das Ergebnis ist jetzt `None` (aufgeben), nicht mehr
    /// `Some(Obergrenze)` (deckeln) — siehe die ausfuehrliche Begruendung
    /// an `RATE_LIMIT_WARTEZEIT_OBERGRENZE_SEC`. Ein Deckel haette bei
    /// einem echten Extremwert nur eine falsche, viel zu kurze Wartezeit
    /// vorgetaeuscht.
    #[test]
    fn extreme_rate_limit_wartezeit_fuehrt_zum_aufgeben() {
        let fehler = ApiError::RateLimited {
            retry_after_seconds: u64::MAX,
        };
        assert_eq!(naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 0), None);
    }

    /// v1.7.17 Runde 10 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): urspruenglich sollte eine LEGITIME, laengere
    /// Server-Sperre (z. B. 30 Minuten) nicht auf eine zu enge Grenze
    /// verkuerzt werden.
    ///
    /// ⚠ Runde 11 hat diese Annahme UEBERHOLT: Runde 11 zeigte, dass das
    /// blosse Verlaengern der Grenze (Runde 10: eine Stunde) einen
    /// nicht-persistenten, abgetrennten Task bis zu zwei Stunden schlafen
    /// legen konnte — ein Restart/Flugende in diesem Fenster verliert die
    /// Korrektur ersatzlos. Die korrekte Antwort auf eine derart lange
    /// Sperre ist deshalb NICHT MEHR "voll durchreichen" (Runde 10),
    /// sondern "aufgeben" (Runde 11) — dieser Test dokumentiert die
    /// Korrektur der Korrektur, statt die ueberholte Zusicherung
    /// stillschweigend zu loeschen.
    #[test]
    fn eine_lange_rate_limit_sperre_fuehrt_jetzt_zum_aufgeben_statt_zum_vollen_warten() {
        let fehler = ApiError::RateLimited {
            retry_after_seconds: 1800, // 30 Minuten
        };
        assert_eq!(
            naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 0),
            None,
            "eine Sperre ueber der Obergrenze muss zum Aufgeben fuehren, nicht zum \
             stundenlangen Schlafen eines nicht persistenten Tasks (Runde 11)"
        );
    }

    #[test]
    fn transiente_netzfehler_nutzen_das_feste_schema() {
        let fehler = ApiError::Network("timeout".into());
        assert_eq!(
            naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 0),
            Some(2)
        );
        assert_eq!(
            naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 1),
            Some(8)
        );
    }

    #[test]
    fn harte_fehler_bekommen_keine_wartezeit() {
        let fehler = ApiError::NotFound;
        assert_eq!(naechste_wartezeit_fuer_pirep_feld_retry(&fehler, 0), None);
    }
