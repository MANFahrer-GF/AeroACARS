    /// Ausschnitt der Funktion ab `start_marker` bis zum naechsten
    /// Top-Level-`fn ` ODER `async fn ` — je nachdem was zuerst kommt.
    /// ⚠ Nur nach `\nfn ` zu suchen reicht NICHT: die naechste Funktion
    /// nach einer `async fn` ist in dieser Datei oft selbst wieder eine
    /// `async fn`, und `\nfn ` matcht das nicht — das Fenster wuerde dann
    /// weit ueber das Funktionsende hinaus in spaetere, unverwandte
    /// Funktionen hineinreichen (die z. B. dieselbe Hilfsfunktion nochmal
    /// aufrufen) und einen entfernten Aufruf faelschlich als „noch da"
    /// durchgehen lassen. Bei der ersten Fassung dieses Tests genau
    /// passiert — deshalb jetzt das Minimum beider Anker.
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

    /// Befund 1: `drain_pending_bid_cleanup` hatte GAR KEINE Kontobindung —
    /// jeder Eintrag wurde mit dem gerade angemeldeten Client verarbeitet,
    /// unabhaengig davon wer ihn eingereiht hatte.
    #[test]
    fn pending_bid_cleanup_prueft_eigentuemer_vor_delete_bid() {
        const SRC: &str = include_str!("../lib.rs");
        let nadel = format!(
            "{}{}",
            "async fn drain_pending_bid_cleanup", "(app: &AppHandle"
        );
        let koerper = funktionskoerper(SRC, &nadel);
        let pruefung_pos = koerper.find("pirep_queue_eintrag_gehoert_aktuellem_piloten(");
        let delete_pos = koerper.find("client.delete_bid(e.bid_id, e.flight_id.as_deref())");
        match (pruefung_pos, delete_pos) {
            (Some(p), Some(d)) => assert!(
                p < d,
                "drain_pending_bid_cleanup muss die Eigentuemerschaft eines Eintrags \
                 pruefen, BEVOR delete_bid mit dem aktuellen Client ausgefuehrt wird — \
                 sonst kann ein Kontowechsel delete_bid mit dem FALSCHEN Account \
                 fuer einen fremden Eintrag ausloesen"
            ),
            _ => panic!(
                "Eigentuemer-Pruefung oder delete_bid-Aufruf in drain_pending_bid_cleanup \
                 nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    /// Befund 2: die phpVMS-Einreichung im Queue-Worker nutzt den am
    /// Tick-Anfang erfassten Client korrekt, die MQTT-Publish- und
    /// JSONL-Upload-Schritte DANACH lesen aber erneut den aktuellen Zustand
    /// — ein Kontowechsel in der Luecke dazwischen darf diese Best-Effort-
    /// Kanaele nicht mit dem FALSCHEN Account weiterlaufen lassen.
    ///
    /// ⚠ Dieser Doc-Block gehoert zu
    /// `queue_worker_prueft_identitaet_erneut_vor_mqtt_und_log_upload`
    /// weiter unten. Wer hier etwas einfuegt, schiebt sonst dessen
    /// `#[test]` auf die eigene Funktion — und der Waechter laeuft
    /// stillschweigend nicht mehr (Abnahme 20.09.2026, zweite Runde).
    ///
    /// Ein Flug, der vor v1.7.40 begann, bekommt seine Sprit-Planwerte
    /// nachgeladen.
    ///
    /// `sprit_an_bord_kg` kam erst mit v1.7.40. Wer mitten im Flug
    /// aktualisiert, hat gespeicherte Wegpunkte ohne dieses Feld — und die
    /// Cockpit-Tabelle blendet sich dann ganz aus (Thomas, 20.09.2026).
    #[test]
    fn ein_flug_ohne_sprit_planwerte_laedt_den_ofp_nach() {
        const SRC: &str = include_str!("../lib.rs");
        // Der Name wird zusammengesetzt, damit dieser Test nicht selbst zum
        // Treffer wird: `vor_der_server_auskunft_wird_nichts_geloescht`
        // sucht dieselbe Zeichenkette in der Quelldatei und fand sonst
        // diesen Test statt der Funktion.
        let nadel = format!("async fn try{}resume_flight(", "_");
        let koerper = funktionskoerper(SRC, &nadel);
        // Was geprueft wird, testet `ofp_nachladen_noetig` (2.0.16).
        let pruefung = koerper
            .find("ofp_nachladen_noetig(")
            .expect("die Wiederaufnahme prueft die Sprit-Planwerte nicht");
        let nachladen = koerper
            .find("flight_refresh_route_only(")
            .expect("die Wiederaufnahme laedt die Route nicht nach");
        assert!(
            !koerper.contains("flight_refresh_simbrief("),
            "der volle Refresh hat ein Phasen-Gate und greift ab Steigflug nicht — \
             und er wuerde Plan- und Gewichtswerte mitten im Flug ersetzen"
        );
        assert!(
            pruefung < nachladen,
            "erst pruefen, dann nachladen — sonst holt jeder Neustart den OFP"
        );
    }

    /// Die Landung reist in der Warteschlange mit — und geht VOR dem
    /// Flugbericht raus.
    ///
    /// Scheitert das Einreichen, wird der Flug geschlossen; der gemerkte
    /// Stand lebt nur im Arbeitsspeicher. Ohne diesen Weg kam der Bericht
    /// spaeter allein an, das Urteil lautete „keine Landung aufgezeichnet"
    /// und liess sich nicht mehr umdrehen (AAL 1331, 20.09.2026).
    #[test]
    fn die_landung_reist_in_der_warteschlange_mit_und_geht_zuerst_raus() {
        const SRC: &str = include_str!("../lib.rs");
        let nadel = format!("{}{}", "fn spawn_pirep_queue_worker", "(app: AppHandle)");
        let koerper = funktionskoerper(SRC, &nadel);
        let landung = koerper
            .find("q.landung")
            .expect("der Worker schickt die Landung aus der Warteschlange nicht");
        let bericht = koerper
            .find("client.file_pirep(&q.pirep_id")
            .expect("der Worker reicht den Bericht nicht mehr ein — Test anpassen");
        assert!(
            landung < bericht,
            "die Landung muss VOR dem Flugbericht raus: Trifft der Bericht ohne \
             Landung ein, faellt das Urteil „keine Landung aufgezeichnet\" und \
             laesst sich nicht mehr umdrehen"
        );
        // Und sie muss beim Einreihen ueberhaupt erst hineinkommen.
        assert!(
            SRC.contains("pub landung: Option<serde_json::Value>"),
            "QueuedPirep traegt die Landung nicht mehr"
        );
        assert!(
            SRC.contains("st.landung_live_bestaetigt"),
            "beim Einreihen wird nicht geprueft, ob die Landung schon bestaetigt ist"
        );
    }

    #[test]
    fn queue_worker_prueft_identitaet_erneut_vor_mqtt_und_log_upload() {
        const SRC: &str = include_str!("../lib.rs");
        let nadel = format!("{}{}", "fn spawn_pirep_queue_worker", "(app: AppHandle)");
        let koerper = funktionskoerper(SRC, &nadel);
        let pruefung_pos = koerper.find("identitaet_noch_aktuell");
        let upload_pos = koerper.find("spawn_flight_log_upload(");
        match (pruefung_pos, upload_pos) {
            (Some(p), Some(u)) => assert!(
                p < u,
                "der Queue-Worker muss VOR dem MQTT-Publish/JSONL-Upload eines \
                 gequeueten Eintrags erneut pruefen, ob die Identitaet seit dem \
                 Tick-Anfang noch dieselbe ist — sonst koennte ein zwischenzeitlicher \
                 Kontowechsel Pilot As Daten ueber Pilot Bs Kanal weitersenden"
            ),
            _ => panic!(
                "identitaet_noch_aktuell-Pruefung oder spawn_flight_log_upload-Aufruf \
                 im Queue-Worker nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }

    /// Befund 4: `try_resume_flight` nahm seinen Lifecycle-Lock erst NACH
    /// der Eigentuemer-Reklamierung (`get_user_pireps_in_progress`) — ein
    /// zeitgleicher `flight_start`/`flight_adopt` (z. B. der Auto-Start-
    /// Watcher) haette in dieser Luecke einen neuen Flug anlegen und den
    /// Ablage-Platz ueberschreiben koennen, bevor der Resume-Pfad selbst
    /// zum Zug kam.
    #[test]
    fn try_resume_flight_haelt_lifecycle_lock_vor_den_server_roundtrips() {
        const SRC: &str = include_str!("../lib.rs");
        // Der Trennpunkt liegt bewusst MITTEN im Funktionsnamen (nicht an
        // der Wortgrenze wie sonst in dieser Datei ueblich): ein anderer,
        // bereits bestehender Waechter weiter unten sucht (nach Entfernen
        // allen Whitespace) nach dem exakten Stueck "asyncfntry_resume_
        // flight" — waere DAS hier als ein zusammenhaengendes Stueck
        // vorhanden, faende sich jener Waechter selbst HIER statt bei der
        // echten Funktion (Codex-Klasse: Selbst-Treffer, siehe andere
        // Module in dieser Datei).
        let nadel = format!("{}{}", "async fn try_resume_fl", "ight(app: &AppHandle");
        let koerper = funktionskoerper(SRC, &nadel);
        let lock_pos =
            koerper.find("FlightSetupGuard::try_acquire(&state.flight_setup_in_progress)");
        let reklamier_pos = koerper.find("get_user_pireps_in_progress()");
        match (lock_pos, reklamier_pos) {
            (Some(l), Some(r)) => assert!(
                l < r,
                "try_resume_flight muss den FlightSetupGuard VOR der \
                 Eigentuemer-Reklamierung (get_user_pireps_in_progress) erwerben — \
                 sonst kann ein zeitgleicher flight_start/flight_adopt in der Luecke \
                 einen neuen Flug anlegen und die Ablage des zu wiederaufnehmenden \
                 PIREPs ueberschreiben"
            ),
            _ => panic!(
                "FlightSetupGuard-Erwerb oder get_user_pireps_in_progress-Aufruf in \
                 try_resume_flight nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }
