    /// Dieselbe Technik wie in den vorigen Runden-Testmodulen.
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

    /// `setze_session_atomar` muss die Epoche als Teil DERSELBEN
    /// kritischen Sektion erhoehen wie Client/Piloten-ID — sonst waere sie
    /// kein verlaessliches Signal mehr fuer "diese Sitzung ist neu".
    #[test]
    fn setze_session_atomar_erhoeht_die_epoche() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "fn setze_session_atomar(");
        assert!(
            koerper.contains("epoch_guard += 1"),
            "setze_session_atomar erhoeht session_epoch nicht mehr — Hintergrund-\
             Tasks koennten einen abgeschlossenen Login nicht mehr von der \
             vorigen Sitzung unterscheiden"
        );
    }

    /// `leere_session_atomar` (Logout) muss die Epoche GENAUSO erhoehen —
    /// das ist seit Runde 13 das einzige verlaessliche Signal fuer einen
    /// Logout, nicht mehr ein Vergleich von API-Key oder Piloten-ID.
    #[test]
    fn leere_session_atomar_erhoeht_die_epoche() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "fn leere_session_atomar(");
        assert!(
            koerper.contains("epoch_guard += 1"),
            "leere_session_atomar erhoeht session_epoch nicht mehr — ein Logout \
             waere fuer wartende Hintergrund-Tasks nicht mehr erkennbar"
        );
        let nadel = format!("{}{}", "fn phpvms_logout", "(app: AppHandle");
        let logout_koerper = funktionskoerper(SRC, &nadel);
        assert!(
            logout_koerper.contains("leere_session_atomar(&state)"),
            "phpvms_logout ruft leere_session_atomar nicht mehr auf — state.client \
             wuerde wieder isoliert geleert, ohne die Epoche zu erhoehen"
        );
    }

    /// Befund 1: ein direkter Re-Login (ohne vorheriges Logout) liess den
    /// MQTT-Handle des VORIGEN Piloten bis zum naechsten Provisionierungs-
    /// Tick weiterlaufen — Fluege/Positionen des neuen Piloten haetten in
    /// dieser Luecke ueber den alten Handle laufen koennen. Der Stopp
    /// muss SYNCHRON passieren, nicht erst im gespawnten Task.
    #[test]
    fn login_stoppt_fremden_mqtt_handle_synchron_vor_dem_spawn() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "async fn phpvms_login(");
        let stopp_pos = koerper.find("stoppe_mqtt_publisher(&state).await");
        // Ohne die abschliessende Klammer suchen — ein einzelnes,
        // unausgeglichenes Klammerzeichen in einem String-Literal bringt
        // den Klammer-Zaehler von tests/angeschlossen.rs durcheinander.
        let spawn_pos = koerper.find("tauri::async_runtime::spawn(async move");
        match (stopp_pos, spawn_pos) {
            (Some(st), Some(sp)) => assert!(
                st < sp,
                "phpvms_login muss einen zur neuen Epoche nicht mehr passenden MQTT-\
                 Handle SYNCHRON stoppen, BEVOR die Provisionierung nur gespawnt wird \
                 — sonst kann ein Flugstart des neuen Piloten in der Luecke noch den \
                 alten Handle benutzen"
            ),
            _ => panic!(
                "synchroner MQTT-Stopp oder Provisionierungs-Spawn in phpvms_login \
                 nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }
