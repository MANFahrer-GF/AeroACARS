    /// Dieselbe Technik wie in den Runde-9/10-Testmodulen.
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

    /// Befund 2: der Idempotenz-Guard pruefte bisher nur "laeuft schon
    /// einer", nicht "gehoert er dem GERADE eingeloggten Account" — ein
    /// Re-Login OHNE vorheriges Logout (ausdruecklich erlaubt, solange
    /// kein Flug aktiv ist) haette einen fremden, noch laufenden Publisher
    /// als "schon da, nichts zu tun" durchgehen lassen.
    #[test]
    fn mqtt_idempotenz_guard_prueft_eigentuemer_nicht_nur_existenz() {
        const SRC: &str = include_str!("../lib.rs");
        let koerper = funktionskoerper(SRC, "async fn init_mqtt_publisher_via_provisioning(");
        assert!(
            koerper.contains("mqtt_owner_epoch"),
            "init_mqtt_publisher_via_provisioning prueft den Eigentuemer des laufenden \
             Publishers nicht mehr (seit Runde 13 ueber die Sitzungs-Epoche) — ein \
             Re-Login eines anderen Piloten liesse den vorigen Publisher unbemerkt \
             weiterlaufen"
        );
    }

    /// Befund 2b: gecachte MQTT-Credentials tragen die pilot_id von
    /// damals — ohne Filter gegen den aktuellen Account waeren sie bei
    /// einem Re-Login blind uebernommen worden.
    #[test]
    fn mqtt_cache_wird_gegen_aktuellen_piloten_gefiltert() {
        const SRC: &str = include_str!("../lib.rs");
        let koerper = funktionskoerper(SRC, "async fn init_mqtt_publisher_via_provisioning(");
        let filter_pos = koerper.find("cached.filter(");
        // Ohne die abschliessende Klammer suchen — ein einzelnes,
        // unausgeglichenes Klammerzeichen in einem String-Literal bringt
        // den Klammer-Zaehler von tests/angeschlossen.rs durcheinander.
        let cfg_pos = koerper.find("let cfg = if let Some(c) = cached");
        match (filter_pos, cfg_pos) {
            (Some(f), Some(c)) => assert!(
                f < c,
                "der Cache-Filter gegen den aktuellen Piloten muss VOR der \
                 cfg-Entscheidung stehen, sonst wirkt er nicht"
            ),
            _ => panic!(
                "cached.filter(...)-Aufruf oder cfg-Entscheidung in \
                 init_mqtt_publisher_via_provisioning nicht mehr gefunden"
            ),
        }
    }

    /// Befund 3: `phpvms_logout` konnte vorzeitig mit `?` zurueckkehren,
    /// WAEHREND der MQTT-Publisher und die SimBrief-Identitaet des
    /// abgemeldeten Piloten noch standen — das Frontend zeigt trotzdem den
    /// Login-Screen. Die In-Memory-Aufraeumung muss deshalb VOR jeder
    /// fehlschlagfaehigen Operation (`delete_api_key`, `clear_site_config`)
    /// laufen, nicht danach.
    #[test]
    fn logout_raeumt_vor_den_fehlschlagfaehigen_schritten_auf() {
        const SRC: &str = include_str!("../lib.rs");
        let koerper = funktionskoerper(SRC, "async fn phpvms_logout(");
        let mqtt_stop_pos = koerper.find("stoppe_mqtt_publisher(&state).await;");
        let simbrief_pos = koerper.find("SimBriefSettings::default();");
        let fehlschlag_pos = koerper
            .find("secrets::delete_api_key(KEYRING_ACCOUNT).map_err(|e| UiError::new(\"keyring\"");
        match (mqtt_stop_pos, simbrief_pos, fehlschlag_pos) {
            (Some(m), Some(s), Some(f)) => assert!(
                m < f && s < f,
                "phpvms_logout muss den MQTT-Publisher stoppen und simbrief_settings \
                 leeren, BEVOR delete_api_key mit `?` fehlschlagen kann — sonst bleibt \
                 bei einem Keyring-Fehler sicherheitsrelevanter Altzustand stehen, \
                 waehrend das Frontend trotzdem den Login-Screen zeigt"
            ),
            _ => panic!(
                "MQTT-Stopp, SimBrief-Reset oder delete_api_key-Aufruf in phpvms_logout \
                 nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    /// Befund 4: `try_resume_flight` kombinierte einen vom Aufrufer
    /// UEBERGEBENEN (potenziell veralteten) Client mit einer separat aus
    /// `state` gelesenen Identitaet — zwischen dem Freigeben des
    /// Lifecycle-Locks beim Aufrufer und dem Wiedererwerb hier kann ein
    /// vollstaendiger Login eines anderen Piloten stattfinden. Beide
    /// muessen jetzt gemeinsam ueber `aktuelle_session_atomar` gelesen
    /// werden, nicht mehr getrennt.
    #[test]
    fn try_resume_flight_liest_client_und_identitaet_gemeinsam() {
        const SRC: &str = include_str!("../lib.rs");
        let nadel = format!("{}{}", "async fn try_resume_fl", "ight(app: &AppHandle");
        let signatur_start = SRC
            .find(&nadel)
            .expect("try_resume_flight-Signatur nicht mehr gefunden");
        // Bewusst OHNE das Zeilenende der Signatur (dessen erste geschweifte
        // Klammer) zu suchen: ein einzelnes, unausgeglichenes Klammerzeichen
        // in einem String-Literal bringt den naiven Klammer-Zaehler von
        // tests/angeschlossen.rs durcheinander (dieselbe Fehlerklasse wie
        // ein unausgeglichenes Zeichen in einem Kommentar — nur hier in
        // einem String statt einem Kommentar). Der klammerfreie Anker
        // "AppState>)" markiert das Signatur-Ende eindeutig genug.
        let signatur_ende = SRC[signatur_start..]
            .find("AppState>)")
            .map(|i| signatur_start + i + "AppState>)".len())
            .expect("Signatur-Ende nicht gefunden");
        let signatur = &SRC[signatur_start..signatur_ende];
        assert!(
            !signatur.contains("client: &Client"),
            "try_resume_flight nimmt wieder einen eigenen client-Parameter entgegen — \
             der koennte vom Aufrufer stammen und beim tatsaechlichen Ausfuehren \
             bereits veraltet sein"
        );
        let koerper = funktionskoerper(SRC, &nadel);
        assert!(
            koerper.contains("aktuelle_session_atomar(state)"),
            "try_resume_flight liest Client und Identitaet nicht mehr gemeinsam \
             ueber aktuelle_session_atomar"
        );
    }

    /// Befund 5: der Auth-Fehler-Zweig (`Unauthenticated`/`Forbidden`) in
    /// `phpvms_load_session` stoppte bisher KEINEN eventuell vom
    /// Setup-Hook parallel gestarteten Publisher — anders als der
    /// benachbarte Status-Gate-Zweig, der das schon tut.
    #[test]
    fn load_session_stoppt_mqtt_auch_im_auth_fehler_zweig() {
        const SRC: &str = include_str!("../lib.rs");
        let koerper = funktionskoerper(SRC, "async fn phpvms_load_session(");
        // Seit Runde 13 DREI Stellen: Status-Gate, Auth-Fehler (beide
        // Runde 10/11) UND der neue synchrone Stopp direkt nach
        // `setze_session_atomar` (Runde 13, schliesst die Luecke bevor
        // der Restore ueberhaupt als abgeschlossen gilt).
        assert_eq!(
            koerper
                .matches("stoppe_mqtt_publisher(&state).await")
                .count(),
            3,
            "phpvms_load_session muss den MQTT-Publisher an ALLEN DREI Stellen stoppen, \
             die eine Sitzung beenden oder wechseln (Status-Gate, Auth-Fehler, \
             synchron nach dem Session-Commit) — sonst bleibt an einer Stelle ein \
             fremder Publisher aktiv"
        );
    }
