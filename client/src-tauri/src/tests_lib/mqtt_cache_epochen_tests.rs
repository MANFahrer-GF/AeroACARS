    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Ueberholter Provisionierungs-Request: Epoche beim Start 7, waehrend
    /// des HTTP-Awaits Kontowechsel/Logout → Epoche 8. Es darf NICHTS in
    /// den Keyring geschrieben werden.
    #[test]
    fn ueberholter_request_schreibt_nichts() {
        let epoch = Mutex::new(7u64);
        let start = *epoch.lock().unwrap();
        *epoch.lock().unwrap() += 1; // Logout/Login waehrend provision().await
        let mut geschrieben = false;
        let ok = mqtt_cache_schreiben_wenn_epoche_gilt(&epoch, start, || geschrieben = true);
        assert!(!ok);
        assert!(
            !geschrieben,
            "ueberholter Request hat den Keyring beschrieben"
        );
    }

    #[test]
    fn unveraenderte_epoche_schreibt() {
        let epoch = Mutex::new(3u64);
        let mut geschrieben = false;
        assert!(mqtt_cache_schreiben_wenn_epoche_gilt(&epoch, 3, || {
            geschrieben = true
        }));
        assert!(geschrieben);
    }

    /// Das Schreiben laeuft unter dem gehaltenen Epochen-Lock: ein
    /// paralleler Logout, der die Epoche erhoehen will, kann erst NACH dem
    /// Schreiben zum Zug kommen (und loescht danach den Cache).
    #[test]
    fn schreiben_haelt_den_epochen_lock() {
        let epoch = Arc::new(Mutex::new(1u64));
        let im_schreiben = Arc::new(AtomicBool::new(false));
        let ok = mqtt_cache_schreiben_wenn_epoche_gilt(&epoch, 1, || {
            im_schreiben.store(true, Ordering::SeqCst);
            assert!(
                epoch.try_lock().is_err(),
                "Epochen-Lock waehrend des Keyring-Schreibens nicht gehalten"
            );
        });
        assert!(ok && im_schreiben.load(Ordering::SeqCst));
    }

    /// Verdrahtung: in der Provisionierung liegen die Keyring-Schreibzugriffe
    /// innerhalb des Helfers, nicht davor.
    #[test]
    fn provisionierung_schreibt_nur_ueber_den_epochen_helfer() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find("async fn init_mqtt_publisher_via_provisioning(")
            .expect("Funktion fehlt");
        let ende = start
            + SRC[start..]
                .find("\nasync fn stoppe_mqtt_publisher(")
                .expect("Ende fehlt");
        let koerper = &SRC[start..ende];
        let helfer = koerper
            .find("mqtt_cache_schreiben_wenn_epoche_gilt(")
            .expect("Provisionierung nutzt den Epochen-Helfer nicht mehr");
        let erster_store = koerper
            .find("store(MQTT_KEYRING_USERNAME")
            .expect("store-Aufruf fehlt");
        assert!(
            erster_store > helfer,
            "Keyring wird vor der Epochen-Pruefung beschrieben"
        );
    }
