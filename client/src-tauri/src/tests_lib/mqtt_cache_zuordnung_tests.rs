    use super::mqtt_cache_gehoert_angemeldetem;

    /// Svens Fall: Recorder-ID 23, Pilotennummer 19. Gespeichert wird die
    /// Nummer — der Cache gehoert ihm.
    #[test]
    fn pilotennummer_passt() {
        assert!(mqtt_cache_gehoert_angemeldetem(Some("19"), 19));
        assert!(mqtt_cache_gehoert_angemeldetem(Some(" 5 "), 5));
    }

    /// Gegenprobe: anderes Konto oder fehlende Zuordnung (Cache von vor
    /// diesem Feld) — nicht verwenden, neu provisionieren.
    #[test]
    fn fremde_oder_fehlende_nummer_passt_nicht() {
        assert!(!mqtt_cache_gehoert_angemeldetem(Some("23"), 19));
        assert!(!mqtt_cache_gehoert_angemeldetem(None, 19));
        assert!(!mqtt_cache_gehoert_angemeldetem(Some(""), 19));
    }

    /// Verdrahtung: Provisionierung UND Pruefstatus vergleichen ueber die
    /// gespeicherte Pilotennummer, nicht mehr ueber die Recorder-ID.
    #[test]
    fn beide_vergleiche_nutzen_die_pilotennummer() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        // Nadeln zusammengesetzt — als ganzes Literal staenden sie HIER vor
        // der echten `pirep_pruefstatus` und der Waechter faende sich selbst.
        for kopf in [
            format!("{}{}", "async fn init_mqtt_publisher", "_via_provisioning("),
            format!("{}{}", "async fn pirep_", "pruefstatus("),
            format!("{}{}", "async fn flugakte_", "laden("),
        ] {
            let start = SRC.find(&kopf).expect("Funktion nicht gefunden");
            let rest = &SRC[start..];
            let ende = rest[1..].find("\n#[").map(|e| e + 1).unwrap_or(rest.len());
            let koerper = &rest[..ende];
            assert!(
                koerper.contains("mqtt_cache_gehoert_angemeldetem("),
                "{kopf} prueft die Zuordnung nicht ueber die Pilotennummer"
            );
            assert!(
                !koerper.contains(&format!("{}{}", "c.pilot_id == ", "id.to_string()")),
                "{kopf} vergleicht wieder Recorder-ID mit Pilotennummer"
            );
        }
    }
