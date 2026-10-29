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

    /// Befund 1a: der MQTT-Provisionierungs-Task wurde bisher VOR
    /// `setze_session_atomar` gespawnt — er konnte also lostraben und
    /// `authenticated_pilot_id` lesen, bevor die neue Session ueberhaupt
    /// committed war.
    #[test]
    fn mqtt_wird_erst_nach_dem_session_commit_gespawnt() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "async fn phpvms_login(");
        let commit_pos =
            koerper.find("setze_session_atomar(&state, client.clone(), profile.pilot_id)");
        let spawn_pos = koerper.find("init_mqtt_publisher_via_provisioning(app_for_mqtt).await");
        match (commit_pos, spawn_pos) {
            (Some(c), Some(s)) => assert!(
                c < s,
                "phpvms_login spawnt die MQTT-Provisionierung wieder VOR dem \
                 Session-Commit — der Hintergrund-Task koennte dann den ALTEN \
                 Piloten lesen, obwohl der Login schon fuer den neuen lief"
            ),
            _ => panic!(
                "setze_session_atomar-Aufruf oder MQTT-Spawn in phpvms_login \
                 nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    /// Befund 1b: die abschliessende Identitaets-Pruefung vor der Handle-
    /// Installation muss INNERHALB der gehaltenen `state.mqtt`-Sperre
    /// stattfinden — `state.mqtt.lock().await` ist selbst ein Await, und
    /// eine Pruefung DAVOR laesst ein Fenster, in dem ein paralleler
    /// Logout (derselbe Lock, ueber `stoppe_mqtt_publisher`) dazwischen-
    /// funken kann.
    #[test]
    fn mqtt_installations_pruefung_liegt_innerhalb_der_sperre() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "async fn init_mqtt_publisher_via_provisioning(");
        let lock_pos = koerper.find("let mut mqtt_guard = state.mqtt.lock().await;");
        // rfind, nicht find: es gibt zwei Identitaets-Pruefungen in dieser
        // Funktion (die erste aus Runde 11, vor den Empfaenger-
        // Weiterleitungen) — hier zaehlt die SPAETERE, die innerhalb der
        // Sperre liegen muss.
        let pruefung_pos = koerper.rfind("aktuelle_epoche(&state) != epoche_bei_start");
        let install_pos = koerper.find("*mqtt_guard = Some(handle);");
        match (lock_pos, pruefung_pos, install_pos) {
            (Some(l), Some(p), Some(i)) => assert!(
                l < p && p < i,
                "die Identitaets-Pruefung vor der MQTT-Installation muss zwischen \
                 dem Lock-Erwerb und der Installation liegen (also INNERHALB der \
                 gehaltenen Sperre) — sonst kann ein paralleler Logout in der \
                 Luecke zwischen Pruefung und Lock-Erwerb hindurchschluepfen"
            ),
            _ => panic!(
                "Lock-Erwerb, Identitaets-Pruefung oder Installation in \
                 init_mqtt_publisher_via_provisioning nicht mehr gefunden"
            ),
        }
    }

    /// Befund 3: `phpvms_refresh_profile` schrieb ein verspaetetes
    /// Server-Ergebnis bisher ungeprueft zurueck — waehrend des
    /// `get_profile`-Roundtrips konnte sich ein anderer Pilot anmelden.
    #[test]
    fn refresh_profile_prueft_identitaet_nach_dem_roundtrip() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper(SRC, "async fn phpvms_refresh_profile(");
        let erfassung_pos = koerper.find("aktuelle_session_atomar(&state)");
        let roundtrip_pos = koerper.find("client.get_profile().await");
        // Seit Runde 13: Epoche statt Piloten-ID — letztere wird bei
        // Logout NICHT geleert (siehe deren Doku) und war deshalb kein
        // verlaessliches Signal fuer "die Sitzung hat sich geaendert".
        let pruefung_pos = koerper.find("aktuelle_epoche(&state) != epoche_bei_start");
        let schreiben_pos = koerper.find("cache_pilot(&state, &p);");
        match (erfassung_pos, roundtrip_pos, pruefung_pos, schreiben_pos) {
            (Some(e), Some(r), Some(p), Some(s)) => assert!(
                e < r && r < p && p < s,
                "phpvms_refresh_profile muss die Sitzungs-Epoche NACH dem Roundtrip \
                 erneut pruefen, BEVOR das Ergebnis zurueckgeschrieben wird — \
                 sonst kann ein verspaetetes Profil eine inzwischen neu \
                 angemeldete ODER abgemeldete Sitzung ueberschreiben"
            ),
            _ => panic!(
                "Identitaets-Erfassung, Roundtrip, Pruefung oder cache_pilot-Aufruf \
                 in phpvms_refresh_profile nicht mehr gefunden — Test anpassen"
            ),
        }
    }
