    use super::*;

    #[test]
    fn gehoert_dem_aktuellen_piloten() {
        assert!(pirep_queue_eintrag_gehoert_aktuellem_piloten(
            Some("fingerprint-a"),
            "fingerprint-a"
        ));
    }

    /// Der eigentliche Befund: ein Eintrag, der jemand anderem gehoert,
    /// darf NICHT mit den Credentials des aktuell eingeloggten Piloten
    /// bearbeitet werden.
    #[test]
    fn gehoert_nicht_einem_anderen_piloten() {
        assert!(!pirep_queue_eintrag_gehoert_aktuellem_piloten(
            Some("fingerprint-a"),
            "fingerprint-b"
        ));
    }

    /// Alt-Eintraege (vor diesem Feld geschrieben) oder ein fehlender Client
    /// beim Einreihen ergeben `None` — das MUSS als "unbekannt" gelten, nie
    /// als "niemandes, also frei fuer jeden".
    #[test]
    fn unbekannter_eigentuemer_gilt_nicht_als_frei() {
        assert!(!pirep_queue_eintrag_gehoert_aktuellem_piloten(
            None,
            "irgendein-fingerprint"
        ));
    }
