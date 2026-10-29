    /// Dieselbe Technik wie in `konten_isolierung_runde_neun_wiring_tests`
    /// — Minimum aus `\nfn ` und `\nasync fn ` als Endboundary, siehe
    /// dortiger Kommentar fuer die Begruendung.
    fn funktionskoerper(quelle: &str, start_marker: &str) -> String {
        let start = quelle
            .find(start_marker)
            .unwrap_or_else(|| panic!("{start_marker} nicht mehr gefunden — Test anpassen"));
        let rest = &quelle[start..];
        let naechstes_fn = rest[1..].find("\nfn ").map(|i| i + 1);
        let naechstes_async_fn = rest[1..].find("\nasync fn ").map(|i| i + 1);
        let ende = [naechstes_fn, naechstes_async_fn]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(rest.len());
        rest[..ende].to_string()
    }

    /// Befund 1: `phpvms_load_session` nahm bisher KEINEN Lifecycle-Lock —
    /// ein gleichzeitiger Login/Flugstart waehrend des Restore-Roundtrips
    /// (`get_profile`) haette einen fremden Flug installieren koennen,
    /// bevor diese Funktion `state.client` ueberschreibt. `phpvms_login`/
    /// `phpvms_logout`/`try_resume_flight` liegen VOR dieser Funktion im
    /// Quelltext — direkte Literale sind hier sicher (kein Selbst-Treffer).
    #[test]
    fn phpvms_load_session_haelt_lifecycle_lock_vor_get_profile() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "async fn phpvms_load_session(");
        let lock_pos =
            koerper.find("FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)");
        let roundtrip_pos = koerper.find("client.get_profile().await");
        match (lock_pos, roundtrip_pos) {
            (Some(l), Some(r)) => assert!(
                l < r,
                "phpvms_load_session muss den FlightSetupGuard VOR dem \
                 get_profile-Roundtrip erwerben — sonst kann ein zeitgleicher \
                 Login/Flugstart in der Luecke einen fremden Flug installieren"
            ),
            _ => panic!(
                "FlightSetupGuard-Erwerb oder get_profile-Aufruf in \
                 phpvms_load_session nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    /// Befund 2: `phpvms_login`/`phpvms_load_session` schrieben `state.
    /// client` und `state.authenticated_pilot_id` bisher mit ZWEI
    /// getrennten Lock/Unlock-Paaren — keine gemeinsame Atomaritaets-
    /// Garantie gegenueber echter Thread-Nebenlaeufigkeit. Beide muessen
    /// jetzt ueber `setze_session_atomar` schreiben.
    #[test]
    fn login_und_load_session_schreiben_ueber_setze_session_atomar() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let login_koerper = funktionskoerper(SRC, "async fn phpvms_login(");
        assert!(
            login_koerper
                .contains("setze_session_atomar(&state, client.clone(), profile.pilot_id)"),
            "phpvms_login schreibt state.client/authenticated_pilot_id nicht mehr ueber \
             setze_session_atomar — zwei getrennte Schreibvorgaenge waeren keine echte \
             Atomaritaets-Garantie mehr"
        );
        let load_session_koerper = funktionskoerper(SRC, "async fn phpvms_load_session(");
        assert!(
            load_session_koerper
                .contains("setze_session_atomar(&state, client.clone(), profile.pilot_id)"),
            "phpvms_load_session schreibt state.client/authenticated_pilot_id nicht mehr ueber \
             setze_session_atomar"
        );
    }

    /// Befund 3: die Bid-Reklamierung akzeptierte bisher auch einen
    /// Treffer ueber `flight_id` als Eigentumsnachweis — `flight_id`
    /// bezeichnet aber den GEPLANTEN Flug, nicht den Bid, und mehrere
    /// Piloten koennen legitim je einen eigenen Bid auf denselben Flug
    /// haben. Nur `bid_id` beweist Eigentum.
    #[test]
    fn bid_reklamierung_beweist_eigentum_nur_ueber_bid_id() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let nadel = format!(
            "{}{}",
            "async fn drain_pending_bid_cleanup", "(app: &AppHandle"
        );
        let koerper = funktionskoerper(SRC, &nadel);
        assert!(
            koerper.contains("e.bid_id.is_some_and(|bid_id|"),
            "die Reklamierung prueft nicht mehr ausschliesslich per bid_id — \
             Test anpassen falls die Struktur sich geaendert hat"
        );
        assert!(
            !koerper.contains("fid == b.flight_id"),
            "die Reklamierung akzeptiert wieder einen flight_id-Treffer als \
             Eigentumsnachweis — mehrere Piloten koennen denselben Flug bieten, \
             das ist KEIN Beweis dass der Cleanup-Eintrag dem aktuellen Account gehoert"
        );
    }

    /// Befund 4: `init_mqtt_publisher_via_provisioning` installierte den
    /// frisch provisionierten Handle bisher OHNE erneute Pruefung, ob der
    /// Account seit Provisionierungs-Start noch derselbe ist — ein
    /// `phpvms_logout`/erneuter Login waehrend der Netzwerk-Awaits haette
    /// das nicht gestoppt.
    #[test]
    fn mqtt_provisionierung_prueft_epoche_vor_der_installation() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "async fn init_mqtt_publisher_via_provisioning(");
        // Seit Runde 13: `epoche_bei_start` (Sitzungs-Epoche) statt
        // API-Key — siehe `mqtt_installations_pruefung_liegt_innerhalb_
        // der_sperre` fuer die Reihenfolge INNERHALB der Sperre. Seit
        // Runde 12 liegt die Installation in `*mqtt_guard = ...`
        // (innerhalb der gehaltenen Sperre) statt direkt in
        // `*state.mqtt.lock().await = ...`.
        let start_erfassung_pos = koerper.find("let epoche_bei_start = aktuelle_epoche(&state);");
        let install_pos = koerper.find("*mqtt_guard = Some(handle);");
        match (start_erfassung_pos, install_pos) {
            (Some(s), Some(i)) => assert!(
                s < i,
                "die Provisionierung muss die Sitzungs-Epoche VOR der Netzwerk-\
                 Provisionierung erfassen UND vor der Installation erneut pruefen — \
                 sonst installiert ein verspaeteter Task einen Handle fuer eine \
                 laengst geaenderte Sitzung"
            ),
            _ => panic!(
                "epoche_bei_start-Erfassung oder Handle-Installation \
                 in init_mqtt_publisher_via_provisioning nicht mehr gefunden"
            ),
        }
    }

    /// Befund 5: `spawn_flight_log_upload` las die MQTT-Keyring-Credentials
    /// bisher ohne jede Identitaets-Pruefung zum Ausfuehrungszeitpunkt — der
    /// Task laeuft asynchron, oft Sekunden nach dem Spawn; eine Pruefung
    /// AM AUFRUFORT sagt nichts darueber, wer noch angemeldet ist, WENN
    /// der Task tatsaechlich laeuft.
    #[test]
    fn flight_log_upload_prueft_identitaet_vor_dem_keyring_lesen() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        // Am Zeilenanfang verankert statt an der einzeiligen Signatur: Seit
        // dem Parameter `nur_diagnose` (23.09.2026) steht sie mehrzeilig, und
        // der Waechter fand sie nicht mehr. Der eigene Suchtext hier ist
        // eingerueckt, trifft also nicht auf sich selbst.
        let nadel = format!("{}{}", "\nfn spawn_flight_log_upload", "(");
        let koerper = funktionskoerper(SRC, &nadel);
        let pruefung_pos = koerper.find("owner_identity.as_deref()");
        let keyring_pos = koerper.find("secrets::load_api_key(MQTT_KEYRING_USERNAME)");
        match (pruefung_pos, keyring_pos) {
            (Some(p), Some(k)) => assert!(
                p < k,
                "spawn_flight_log_upload muss die Identitaet VOR dem Keyring-Lesen \
                 pruefen — sonst laedt ein spaeter angemeldeter anderer Pilot \
                 versehentlich das Flugprotokoll eines fremden Accounts hoch"
            ),
            _ => panic!(
                "Identitaets-Pruefung oder Keyring-Lesen in spawn_flight_log_upload \
                 nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    /// Befund 6: `enqueue_pending_bid_cleanup` und `drain_pending_bid_
    /// cleanup` machten bisher UNSYNCHRONISIERTE Lese-Aendere-Schreibe-
    /// Zyklen auf dieselbe JSON-Datei — ein `enqueue` genau zwischen dem
    /// `read_all()` und dem `replace()` des Workers haette den frisch
    /// hinzugekommenen Eintrag wieder verloren.
    #[test]
    fn pending_bid_cleanup_datei_ist_gegen_verlorene_updates_gesperrt() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let enqueue_nadel = format!("{}{}", "fn enqueue_pending_bid_cleanup", "(\n    app");
        let enqueue_koerper = funktionskoerper(SRC, &enqueue_nadel);
        assert!(
            enqueue_koerper.contains("pending_bid_cleanup_lock"),
            "enqueue_pending_bid_cleanup nimmt das Datei-Schloss nicht mehr — \
             ein gleichzeitiger Drain-Tick koennte den neuen Eintrag verlieren"
        );
        let drain_nadel = format!(
            "{}{}",
            "async fn drain_pending_bid_cleanup", "(app: &AppHandle"
        );
        let drain_koerper = funktionskoerper(SRC, &drain_nadel);
        let lock_pos = drain_koerper.find("pending_bid_cleanup_lock");
        let read_pos = drain_koerper.find("queue.read_all()");
        match (lock_pos, read_pos) {
            (Some(l), Some(r)) => assert!(
                l < r,
                "drain_pending_bid_cleanup muss das Datei-Schloss VOR read_all() \
                 nehmen und bis nach replace() halten"
            ),
            _ => panic!(
                "pending_bid_cleanup_lock-Erwerb oder read_all-Aufruf in \
                 drain_pending_bid_cleanup nicht mehr gefunden"
            ),
        }
    }

    /// Befund 7: `simbrief_settings` (auto-gesourct oder manuell gesetzt)
    /// ueberlebte einen Logout — der naechste Pilot ohne eigenen
    /// localStorage-Identifier haette stillschweigend den SimBrief-OFP
    /// des vorigen Piloten angeboten bekommen.
    #[test]
    fn logout_leert_simbrief_settings() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "async fn phpvms_logout(");
        assert!(
            koerper.contains("simbrief_settings")
                && koerper.contains("SimBriefSettings::default()"),
            "phpvms_logout setzt simbrief_settings nicht mehr auf Default zurueck — \
             der naechste Pilot ohne eigenen Identifier saehe den vorigen SimBrief-OFP"
        );
    }
