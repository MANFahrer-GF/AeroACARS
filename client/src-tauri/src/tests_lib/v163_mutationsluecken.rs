    use super::*;

    /// **Der Crash-Schutz — bis hierher komplett ungeprueft.**
    /// `apply_accident_heuristic` wurde von keinem einzigen Test aufgerufen.
    /// Sowohl das Umdrehen des Vergleichs (weichster statt haertester Wert)
    /// als auch der vollstaendige Rueckbau blieben unsichtbar. Genau der
    /// Riegel, der verhindert, dass die neue Messquelle einen Aufprall unter
    /// die Schwelle spuelt.
    #[test]
    fn qs_m56_unfallerkennung_nimmt_den_haertesten_messwert() {
        let mut stats = FlightStats {
            landing_simulator: Some("Msfs2024"),
            landing_peak_g_force: Some(3.6),
            runway_match: None,
            ..Default::default()
        };
        // Der gescorte Wert ist harmlos, die SimVar-Quelle reisst die Schwelle.
        let analysis = serde_json::json!({
            "vs_at_edge_fpm": -400.0,
            "vs_geometrie_fpm": -400.0,
            "vs_simvar_edge_fpm": -1600.0,
        });
        apply_accident_heuristic(&mut stats, &analysis);
        assert!(
            stats.accident_kind.is_some() || stats.accident_detected,
            "eine Sinkrate von 1600 fpm darf nicht durchrutschen, nur weil die \
             gescorte Quelle weicher liest"
        );
    }

    /// **Die Herkunft der Sinkrate erreicht die Datenbank.**
    ///
    /// Ohne dieses Feld liesse sich nach dem Release nur per Durchsuchen
    /// der Flugprotokoll-Dateien feststellen, ob die neue Messquelle
    /// ueberhaupt greift. Genau das ist der Vorgaenger-Korrektur zum
    /// Verhaengnis geworden: sie lag zwei Monate wirkungslos im Code, und
    /// niemand konnte es sehen. Der Test haelt fest, dass die drei
    /// Schluessel den Weg in den Payload finden.
    #[test]
    fn die_herkunft_der_sinkrate_steht_im_payload() {
        // Der Payload wird nur mit vorhandener Analyse gebaut; hier reicht
        // die Gegenprobe ueber den Quelltext, weil die Bau-Stelle tief im
        // Streamer sitzt.
        const SRC: &str = include_str!("../lib.rs");
        let ohne_kommentare: String = SRC
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for schluessel in [
            "vs_at_edge_quelle",
            "vs_geometrie_fpm",
            "vs_simvar_edge_fpm",
        ] {
            assert!(
                ohne_kommentare.contains(&format!("{schluessel}:")),
                "{schluessel} erreicht den Touchdown-Payload nicht mehr — \
                 damit ist im Feld nicht mehr messbar, ob die Hoehenkurve \
                 wirklich greift"
            );
        }
    }

    /// **Beide Episoden-Enden rufen den Rücksetzer wirklich auf.**
    ///
    /// Der Test daneben prüft, was `landung_episode_zuruecksetzen` TUT.
    /// Dieser hier prüft, dass sie überhaupt aufgerufen wird — beide
    /// Aufrufe ließen sich einzeln ersatzlos streichen, ohne dass ein
    /// einziger von 1276 Tests rot wurde (QS-Befund Runde 6).
    ///
    /// Dieselbe Fehlerklasse, die dieses Release schon zweimal getroffen
    /// hat, nur eine Ebene höher: in Runde 4 stand ein Grenzwert an zweiter
    /// Stelle unverändert, in Runde 5 sprang ein Test über die
    /// Erzeuger-Kette. Hier war es der Schluss „beide Stellen rufen die
    /// Funktion auf, also deckt der Test beide ab" — genau das war die
    /// ungeprüfte Annahme.
    ///
    /// Ein Wächter über den Quelltext statt ein Ablauf-Test, weil beide
    /// Stellen tief in der Phasen-Logik bzw. in einem Hintergrund-Task
    /// sitzen. Grob, aber er fängt das ersatzlose Streichen — und darum
    /// geht es.
    #[test]
    fn beide_episoden_enden_raeumen_die_landung_ab() {
        const SRC: &str = include_str!("../lib.rs");
        let ohne_kommentare: String = SRC
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        // 20.10.2026 (Stück 3): Beide Episoden-Enden rufen dieselbe Funktion
        // `landung_beim_wegsteigen_abschliessen` — dort stehen die Aufraeumer
        // genau einmal (Ablauf-Test: `landung_episoden_tests::
        // abschluss_raeumt_wind_und_sprit_ab`). Gewacht wird hier, dass keiner
        // der beiden Aufrufer sie verliert.
        for funktion in [
            "fn spawn_touchdown_sampler",
            "fn step_flight_at",
            "fn landung_beim_wegsteigen_abschliessen",
        ] {
            let start = ohne_kommentare.find(funktion).unwrap_or_else(|| {
                panic!("{funktion} nicht mehr gefunden — Test anpassen, nicht löschen")
            });
            let rest = &ohne_kommentare[start..];
            let ende = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
            let aufraeumer: &[&str] = if funktion == "fn landung_beim_wegsteigen_abschliessen" {
                // v1.7.35: BEIDE Aufraeumer. Der Sprit-Teil wurde aus
                // `landung_episode_zuruecksetzen` herausgeloest (die greift
                // auch bei einem blossen AGL-Ausreisser im Ausrollen) — und
                // prompt fehlte er an einer der beiden Stellen, ohne dass
                // dieser Waechter fiel: er kannte nur den einen Namen.
                &["landung_episode_zuruecksetzen(", "sprit_durchstart_zuruecksetzen("]
            } else {
                &["landung_beim_wegsteigen_abschliessen("]
            };
            for aufraeumer in aufraeumer {
                assert!(
                    rest[..ende].contains(aufraeumer),
                    "{funktion} ruft {aufraeumer} nicht mehr — nach einem \
                     Touch-and-Go rechnet die naechste Landung dann mit den \
                     Werten der vorigen (Wind, Geschwindigkeit, Landesprit, \
                     Sprit-Auswertung)"
                );
            }
        }
    }

    /// **Die Unfall-Erkennung liest die verworfene Höhenkurve wirklich.**
    /// Der Test daneben belegt, dass der Wert im Datensatz landet — dieser
    /// hier, dass er auch benutzt wird. Ohne ihn ließ sich der Kandidat aus
    /// der Liste streichen, ohne dass etwas rot wurde (QS-Befund Runde 5).
    /// Aufgebaut ist er so, dass ausschließlich dieser eine Kandidat die
    /// Schwelle reißt.
    #[test]
    fn unfallerkennung_liest_auch_die_verworfene_hoehenkurve() {
        let mut stats = FlightStats {
            landing_simulator: Some("Msfs2024"),
            landing_peak_g_force: Some(3.6),
            ..Default::default()
        };
        let analysis = serde_json::json!({
            "vs_at_edge_fpm": -300.0,
            "vs_geometrie_fpm": serde_json::Value::Null,
            "vs_simvar_edge_fpm": -300.0,
            "vs_geometrie_ungefiltert_fpm": -1700.0,
        });
        apply_accident_heuristic(&mut stats, &analysis);
        assert!(
            stats.accident_kind.is_some() || stats.accident_detected,
            "1700 fpm dürfen nicht durchrutschen, nur weil der Score diese \
             Quelle verworfen hat"
        );
    }

    /// **Der Landewind: ueberschreiben ja, loeschen nein.** Beide Haelften
    /// sind je einmal falsch gebaut worden — erst als `is_none()`-Latch (der
    /// nach einem Touch-and-Go mit dem Wind des ERSTEN Aufsetzens rechnete),
    /// dann als blindes Zuweisen (das einen guten Wert loeschte, sobald ein
    /// Tick die Windfelder nicht trug). Der Test haelt beide Haelften fest.
    #[test]
    fn qs_m7_landewind_wird_ueberschrieben_nicht_gelatcht() {
        let mut stats = FlightStats::default();
        let erst = SimSnapshot {
            wind_direction_deg: Some(10.0),
            wind_speed_kt: Some(12.0),
            ..SimSnapshot::default()
        };
        stamp_touchdown_metadata(&mut stats, &erst, Utc::now(), None);
        assert_eq!(stats.landing_wind_direction_deg, Some(10.0));
        let zweit = SimSnapshot {
            wind_direction_deg: Some(250.0),
            wind_speed_kt: Some(18.0),
            ..SimSnapshot::default()
        };
        stamp_touchdown_metadata(&mut stats, &zweit, Utc::now(), None);
        assert_eq!(
            stats.landing_wind_direction_deg,
            Some(250.0),
            "der zweite Stempel muss den Wind ueberschreiben (kein is_none()-Latch)"
        );
        // Gegenprobe: ein Tick OHNE Windmeldung darf den guten Wert nicht loeschen.
        let leer = SimSnapshot::default();
        stamp_touchdown_metadata(&mut stats, &leer, Utc::now(), None);
        assert_eq!(stats.landing_wind_direction_deg, Some(250.0));
    }
