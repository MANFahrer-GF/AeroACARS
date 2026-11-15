    use super::{tray_text_aendern, TRAY_KOPF_LEERLAUF};

    #[test]
    fn gleicher_text_wird_nicht_neu_gesetzt() {
        // Ohne Flug bleibt die Kopfzeile Takt für Takt gleich — kein Aufruf.
        assert!(!tray_text_aendern(TRAY_KOPF_LEERLAUF, TRAY_KOPF_LEERLAUF));
    }

    #[test]
    fn geaenderter_text_wird_gesetzt() {
        assert!(tray_text_aendern(TRAY_KOPF_LEERLAUF, "DLH 41 EDDF → KJFK · Reiseflug"));
        assert!(tray_text_aendern("AeroACARS", "AeroACARS — DLH 41"));
    }
