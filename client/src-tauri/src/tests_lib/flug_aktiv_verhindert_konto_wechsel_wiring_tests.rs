    /// rustfmt darf die mehrteilige Bedingung im Flug-aktiv-Riegel jederzeit
    /// auf mehrere Zeilen umbrechen (ist bei diesem genauen Fund schon
    /// einmal passiert) — Whitespace (inkl. Zeilenumbrueche) rausfiltern,
    /// BEVOR gesucht wird, macht die Suche robust dagegen. Dieselbe Technik
    /// wie die ohne_leerraum-Helfer in den anderen Testmodulen dieser Datei.
    fn ohne_leerraum(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    /// Funktionskoerper-Grenzen werden auf dem ROHEN Text bestimmt (der
    /// `\nasync fn `-Zeilenumbruch-Anker braucht echte Zeilenumbrueche) —
    /// erst danach wird der ausgeschnittene Abschnitt von Whitespace befreit.
    fn funktionskoerper_ohne_leerraum(quelle: &str, start_marker: &str) -> String {
        let start = quelle.find(start_marker).unwrap_or_else(|| {
            panic!("{start_marker} nicht mehr gefunden — Test anpassen, nicht loeschen")
        });
        let rest = &quelle[start..];
        let ende = rest[1..]
            .find("\nasync fn ")
            .map(|i| i + 1)
            .unwrap_or(rest.len());
        ohne_leerraum(&rest[..ende])
    }

    #[test]
    fn logout_prueft_aktiven_flug_vor_dem_leeren_von_state_client() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper_ohne_leerraum(SRC, "async fn phpvms_logout(");
        let pruefung_pos = koerper.find(&ohne_leerraum("state.active_flight.lock()"));
        // Seit Runde 13: `leere_session_atomar` statt der direkten
        // Zuweisung — leert state.client UND erhoeht session_epoch in
        // einer atomaren Operation.
        let leeren_pos = koerper.find(&ohne_leerraum("leere_session_atomar(&state);"));
        match (pruefung_pos, leeren_pos) {
            (Some(p), Some(l)) => assert!(
                p < l,
                "phpvms_logout muss auf einen aktiven Flug pruefen, BEVOR \
                 state.client geleert wird — sonst kann sich waehrend eines \
                 laufenden Fluges ein anderer Pilot anmelden, dessen \
                 Credentials dann fuer den fremden Flug benutzt werden"
            ),
            _ => panic!(
                "Flug-aktiv-Pruefung oder state.client-Leerung in phpvms_logout \
                 nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    #[test]
    fn login_prueft_den_flug_eigentuemer_vor_dem_ueberschreiben_von_state_client() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper_ohne_leerraum(SRC, "async fn phpvms_login(");
        let pruefung_pos = koerper.find(&ohne_leerraum("active_flight_owner_identity"));
        let ueberschreiben_pos = koerper.find(&ohne_leerraum(
            "setze_session_atomar(&state, client.clone(), profile.pilot_id);",
        ));
        match (pruefung_pos, ueberschreiben_pos) {
            (Some(p), Some(u)) => assert!(
                p < u,
                "phpvms_login muss den Eigentuemer des aktiven Fluges pruefen, \
                 BEVOR state.client ueberschrieben wird — sonst kann ein Re-Login \
                 (auch ohne vorheriges Logout, z. B. ein neu eingegebener Key) \
                 den Logout-Riegel umgehen"
            ),
            _ => panic!(
                "Eigentuemer-Pruefung oder state.client-Ueberschreiben in \
                 phpvms_login nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    /// Codex-Folgefund (adversarial, 05.09.2026, siebte Runde): der
    /// Eigentuemer-Riegel darf NICHT allein auf `active_flight_owner_
    /// identity` pruefen — dieses Feld bleibt nach einem abgeschlossenen
    /// Flug stehen (wird beim Beenden/Abbrechen/Logout nicht geleert). Ohne
    /// eine GLEICHZEITIGE `state.active_flight.is_some()`-Bedingung haette
    /// JEDER Login nach dem allerersten abgeschlossenen Flug fuer immer den
    /// naechsten Piloten ausgesperrt, obwohl gar kein Flug mehr aktiv ist.
    #[test]
    fn login_prueft_zusaetzlich_ob_ueberhaupt_ein_flug_aktiv_ist() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let koerper = funktionskoerper_ohne_leerraum(SRC, "async fn phpvms_login(");
        assert!(
            koerper.contains(&ohne_leerraum("state.active_flight.lock()")),
            "phpvms_login prueft nicht mehr, ob ueberhaupt ein Flug aktiv ist, \
             bevor es den Eigentuemer vergleicht — ein abgeschlossener Flug \
             wuerde dann jeden folgenden Login-Versuch fuer immer blockieren"
        );
    }
