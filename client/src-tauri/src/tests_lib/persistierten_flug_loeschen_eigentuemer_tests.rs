    use super::*;

    #[test]
    fn loescht_wenn_die_ablage_dem_erwarteten_flug_gehoert() {
        assert!(sollte_persistierten_flug_loeschen(
            Some("PIREP-A"),
            Some("PIREP-A")
        ));
    }

    /// Der eigentliche Befund: ein alter Flug (A) nimmt sich per
    /// `guard.take()` aus dem State, haengt an einem Await (Server-
    /// Roundtrip), und in dieser Luecke legt `flight_start` bereits einen
    /// NEUEN Flug (B) an, der seinen ersten Checkpoint an denselben festen
    /// Pfad schreibt. Wenn A's Teardown danach fortsetzt, MUSS die Loeschung
    /// B's frischen Checkpoint verschonen.
    #[test]
    fn loescht_nicht_wenn_die_ablage_inzwischen_einem_neueren_flug_gehoert() {
        assert!(!sollte_persistierten_flug_loeschen(
            Some("PIREP-A-alt"),
            Some("PIREP-B-neu")
        ));
    }

    #[test]
    fn loescht_wenn_kein_eigentuemer_erwartet_wird() {
        // z.B. flight_forget ohne zuvor aktiven Flug — reines
        // Aufraeumen einer verwaisten Datei ohne bekannten Besitzer.
        assert!(sollte_persistierten_flug_loeschen(None, Some("PIREP-X")));
        assert!(sollte_persistierten_flug_loeschen(None, None));
    }

    #[test]
    fn loescht_wenn_die_ablage_nicht_lesbar_oder_nicht_vorhanden_ist() {
        // Unlesbar/fehlend = kein feststellbarer fremder Eigentuemer —
        // dasselbe Prinzip wie beim Resume-Pfad (`read_persisted_flight`
        // gibt bei Parse-Fehlern ebenfalls `None` zurueck, nicht Err).
        assert!(sollte_persistierten_flug_loeschen(Some("PIREP-A"), None));
    }
