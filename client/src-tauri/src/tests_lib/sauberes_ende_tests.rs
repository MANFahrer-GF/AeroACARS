    use super::beendet_den_lauf_sauber;

    #[test]
    fn exit_allein_beendet_den_lauf_sauber() {
        // Windows-Herunterfahren liefert nur `Exit` — ohne diesen Fall blieb
        // die Lauf-Markierung liegen.
        assert!(beendet_den_lauf_sauber(&tauri::RunEvent::Exit));
    }

    #[test]
    fn bereit_ist_kein_ende() {
        assert!(!beendet_den_lauf_sauber(&tauri::RunEvent::Ready));
    }
