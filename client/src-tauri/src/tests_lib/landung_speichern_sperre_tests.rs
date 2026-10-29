    /// ⚠ Die Schalter-Auswertung schreibt ihr Protokoll erst nach dem
    /// Freigeben der Flugdaten (v2.0.19).
    ///
    /// Rust raeumt lokale Werte in umgekehrter Reihenfolge auf: Nur wenn
    /// `aufgeschoben` VOR `stats` angelegt ist, wird die Sperre zuerst frei
    /// und danach geschrieben — auch bei jedem vorzeitigen `return`. Und kein
    /// Protokollaufruf darf an `aufgeschoben` vorbei direkt schreiben.
    #[test]
    fn die_schalter_auswertung_schreibt_nach_dem_freigeben() {
        let src = crate::quelle_gesamt::PRODUKTION;
        let start = src
            .find("\nfn detect_telemetry_changes(")
            .expect("Funktion nicht gefunden — Test anpassen, nicht löschen");
        let rumpf = &src[start..];
        let rumpf = &rumpf[..rumpf[1..].find("\n}\n").map_or(rumpf.len(), |i| i + 1)];
        let puffer = rumpf
            .find("let mut aufgeschoben = AufgeschobeneMeldungen::neu(app);")
            .expect("kein Puffer mehr");
        let sperre = rumpf.find("flight.stats.lock()").expect("keine Sperre");
        assert!(puffer < sperre, "der Puffer wird nach der Sperre angelegt und vor ihr aufgeraeumt");
        let code: String = rumpf
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for direkt in ["log_activity_handle(", "log_activity_mit_meldung(", "log_activity("] {
            assert!(
                !code.contains(direkt),
                "{direkt} schreibt unter der Flugdaten-Sperre — über `aufgeschoben` gehen"
            );
        }
        assert!(code.matches("aufgeschoben.handle(").count() >= 40, "Vorrichtung: Aufrufe nicht gefunden");
        // Auch die Helfer, die unter der Sperre aufgerufen werden, schreiben
        // nur in den Puffer (Codex-Abnahme 2.0.19, P2: sie schrieben noch
        // direkt — unter der Sperre und in falscher Reihenfolge).
        for helfer in ["\nfn log_bool_change(", "\nfn log_three_state_change("] {
            let h = &src[src.find(helfer).expect("Helfer fehlt")..];
            let h = &h[..h[1..].find("\n}\n").map_or(h.len(), |i| i + 1)];
            assert!(h.contains("aufgeschoben.handle("), "{helfer} schreibt nicht in den Puffer");
            for direkt in ["log_activity_handle(", "log_activity_mit_meldung(", "log_activity("] {
                assert!(!h.contains(direkt), "{helfer} schreibt direkt: {direkt}");
            }
        }
    }

    /// ⚠ Die Landungs-Historie wird NACH dem Freigeben der Flugdaten
    /// geschrieben (v2.0.18).
    ///
    /// Bis 2.0.17 lief das Lesen und Neuschreiben der ganzen Historie (bis
    /// 500 Landungen) unter `flight.stats` — der Wächter meldete es bei
    /// jedem Flug mit 140 bis 157 ms (DLH 440/441, 14.10.2026). Ein
    /// Ablauf-Test braucht eine laufende App; deshalb prüft dieser Wächter
    /// die Reihenfolge im Quelltext: Sperre nehmen, Block schliessen, erst
    /// dann schreiben. Der Feldbeleg ist die verschwundene Wächter-Meldung.
    #[test]
    fn die_landungs_historie_wird_ohne_flugdaten_sperre_geschrieben() {
        let src = crate::quelle_gesamt::PRODUKTION;
        let kopf = "\nfn record_landing_for_filed_flight(";
        let start = src.find(kopf).expect("Funktion nicht gefunden — Test anpassen, nicht löschen");
        let rumpf = &src[start..];
        let rumpf = &rumpf[..rumpf[1..].find("\nfn ").map_or(rumpf.len(), |i| i + 1)];
        let sperre = rumpf
            .find("flight.stats.lock()")
            .expect("die Funktion nimmt die Flugdaten-Sperre nicht mehr selbst");
        let schreiben = rumpf.find("store.upsert(").expect("store.upsert fehlt");
        let simulator = rumpf.find("current_snapshot(app)").expect("current_snapshot fehlt");
        assert!(
            simulator < sperre,
            "der Simulator wird unter der Flugdaten-Sperre abgefragt"
        );
        assert!(
            rumpf[sperre..schreiben].contains("\n    };\n"),
            "die Landungs-Historie wird geschrieben, solange die Flugdaten-Sperre gehalten wird"
        );
        assert!(
            !rumpf[..sperre].contains("store.upsert("),
            "Vorrichtung: Schreiben vor dem Bauen ergibt keinen Sinn"
        );
    }
