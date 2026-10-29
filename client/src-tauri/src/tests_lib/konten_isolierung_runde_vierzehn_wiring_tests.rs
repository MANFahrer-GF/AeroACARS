    /// Dieselbe Technik wie in den vorigen Runden-Testmodulen — hier
    /// zusaetzlich um `\nmod ` als Grenze erweitert.
    ///
    /// Codex-Folgefund (adversarial, dieselbe Runde, aber selbst
    /// entdeckt beim Schreiben dieses Tests): `\nfn `/`\nasync fn `
    /// allein reicht nicht als Endgrenze, wenn direkt NACH der Zielfunktion
    /// ein `#[cfg(test)] mod ... { }`-Block folgt (wie bei `phpvms_load_
    /// session`, gefolgt vom Runde-6/7-Testmodul) — die Suche laeuft dann
    /// mitten HINEIN in dieses Testmodul und findet dort eine eigene
    /// Test-Zeile, die zufaellig denselben String zitiert. Ergebnis: ein
    /// massives Ueberschiessen (hier ueber 14000 Zeichen statt der
    /// erwarteten ~700). `\nmod ` stoppt die Suche VOR einem solchen
    /// Block.
    fn funktionskoerper(quelle: &str, start_marker: &str) -> String {
        let start = quelle
            .find(start_marker)
            .unwrap_or_else(|| panic!("{start_marker} nicht mehr gefunden — Test anpassen"));
        let rest = &quelle[start..];
        let naechstes_fn = rest[1..].find("\nfn ").map(|i| i + 1);
        let naechstes_async_fn = rest[1..].find("\nasync fn ").map(|i| i + 1);
        let naechstes_mod = rest[1..].find("\nmod ").map(|i| i + 1);
        let ende = [naechstes_fn, naechstes_async_fn, naechstes_mod]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(rest.len());
        rest[..ende].to_string()
    }

    /// `phpvms_get_bids` las Client und Epoche bisher GETRENNT
    /// (`current_client` dann `aktuelle_epoche`) — ein Kontowechsel genau
    /// dazwischen haette Pilot As Client mit Pilot Bs (schon aktueller)
    /// Epoche gepaart und den spaeteren Vergleich damit wertlos gemacht.
    #[test]
    fn get_bids_liest_client_und_epoche_gemeinsam() {
        const SRC: &str = include_str!("../lib.rs");
        let koerper = funktionskoerper(SRC, "async fn phpvms_get_bids(");
        assert!(
            koerper.contains("aktuelle_session_atomar(&state)"),
            "phpvms_get_bids liest client/epoche nicht mehr gemeinsam ueber \
             aktuelle_session_atomar — ein Kontowechsel zwischen zwei getrennten \
             Lesungen koennte den Epochen-Vergleich wieder wertlos machen"
        );
        assert!(
            !koerper.contains("current_client(&state)"),
            "phpvms_get_bids ruft wieder current_client isoliert auf — das war \
             genau die Luecke aus Runde 14"
        );
    }

    /// Befund 1: beide Restore-Ablehnungs-Zweige in `phpvms_load_session`
    /// (Status-Gate, Auth-Fehler) muessen die Sitzungs-Epoche erhoehen —
    /// sonst kann ein Setup-Hook-Provisionierungs-Task mit der ALTEN
    /// Epoche trotzdem noch erfolgreich installieren, NACHDEM diese
    /// Funktion den Restore bereits abgelehnt hat.
    #[test]
    fn load_session_erhoeht_epoche_in_beiden_ablehnungs_zweigen() {
        const SRC: &str = include_str!("../lib.rs");
        let koerper = funktionskoerper(SRC, "async fn phpvms_load_session(");
        assert_eq!(
            koerper.matches("leere_session_atomar(&state)").count(),
            2,
            "phpvms_load_session muss in BEIDEN Ablehnungs-Zweigen (Status-Gate \
             UND Auth-Fehler) leere_session_atomar aufrufen — sonst bleibt die \
             Sitzungs-Epoche fuer einen wartenden Provisionierungs-Task \
             unveraendert, obwohl der Restore gerade abgelehnt wurde"
        );
    }
