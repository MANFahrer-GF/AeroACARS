    use super::*;

    fn zeile(t: &str) -> api_client::LogEntry {
        api_client::LogEntry {
            id: Some(uuid::Uuid::new_v4().to_string()),
            log: t.to_string(),
            lat: None,
            lon: None,
            created_at: Some(t.to_string()),
        }
    }

    #[test]
    fn obergrenze_verwirft_die_aelteste_zeile_ueber_alle_pireps() {
        let mut offen = AcarsOffen::new();
        let alt: Vec<_> = (0..ACARS_POSTAUSGANG_MAX / 2)
            .map(|i| zeile(&format!("2026-10-11T{i:08}")))
            .collect();
        let neu: Vec<_> = (0..ACARS_POSTAUSGANG_MAX / 2)
            .map(|i| zeile(&format!("2026-10-12T{i:08}")))
            .collect();
        acars_aufnehmen(&mut offen, "ALT".into(), alt);
        acars_aufnehmen(&mut offen, "NEU".into(), neu);
        let verdraengt =
            acars_aufnehmen(&mut offen, "NEU".into(), vec![zeile("2026-10-12T99999999")]);
        assert_eq!(verdraengt.len(), 1, "die verdrängte Zeile geht in die Quarantäne, nicht verloren");
        assert_eq!(verdraengt[0].0, "ALT");
        let gesamt: usize = offen.values().map(|q| q.len()).sum();
        assert_eq!(gesamt, ACARS_POSTAUSGANG_MAX);
        assert_eq!(offen["ALT"].len(), ACARS_POSTAUSGANG_MAX / 2 - 1, "die älteste Zeile fiel weg");
        assert_eq!(offen["NEU"].len(), ACARS_POSTAUSGANG_MAX / 2 + 1);
    }

    #[test]
    fn abgebrochen_nur_am_feld_type() {
        assert!(ist_pirep_abgebrochen(
            r#"{"type":"https://phpvms.net/errors/pirep-cancelled","title":"PIREP has been cancelled"}"#
        ));
        assert!(!ist_pirep_abgebrochen(
            r#"{"type":"https://phpvms.net/errors/validation","details":"pirep-cancelled"}"#
        ));
        assert!(!ist_pirep_abgebrochen("pirep-cancelled"));
    }

    #[test]
    fn obergrenze_gilt_auch_nach_dem_zusammenfuehren() {
        let mut offen = AcarsOffen::new();
        offen.insert(
            "A".into(),
            (0..ACARS_POSTAUSGANG_MAX + 5)
                .map(|i| zeile(&format!("2026-10-11T{i:08}")))
                .collect(),
        );
        let verdraengt = acars_begrenzen(&mut offen);
        assert_eq!(offen["A"].len(), ACARS_POSTAUSGANG_MAX);
        assert_eq!(verdraengt.len(), 5);
    }

    #[test]
    fn entfernt_nach_kennung_nicht_nach_position() {
        let mut offen = AcarsOffen::new();
        let gesendet = vec![zeile("2026-10-12T00000002"), zeile("2026-10-12T00000003")];
        offen.insert("P".into(), gesendet.iter().cloned().collect());
        // Während der Anfrage rücken ältere Zeilen nach vorn (Nachladen).
        let alt = zeile("2026-10-12T00000001");
        offen.get_mut("P").unwrap().push_front(alt.clone());
        acars_paket_entfernen(&mut offen, "P", &gesendet);
        assert_eq!(offen["P"].len(), 1);
        assert_eq!(offen["P"][0].id, alt.id, "die nie gesendete Zeile bleibt");
    }

    #[test]
    fn verdraengte_paketzeilen_nicht_zweimal_in_die_quarantaene() {
        let mut offen = AcarsOffen::new();
        let paket = vec![zeile("2026-10-12T00000001"), zeile("2026-10-12T00000002")];
        offen.insert("P".into(), paket.iter().cloned().collect());
        // Während der Anfrage füllt ein anderer PIREP den Postausgang; die
        // Obergrenze verdrängt die älteste Zeile — die erste des Pakets.
        let neu: Vec<_> = (0..ACARS_POSTAUSGANG_MAX - 1)
            .map(|i| zeile(&format!("2026-10-12T1{i:08}")))
            .collect();
        let verdraengt = acars_aufnehmen(&mut offen, "Q".into(), neu);
        assert_eq!(verdraengt.len(), 1);
        assert_eq!(verdraengt[0].1.id, paket[0].id);
        let rest = acars_paket_noch_offen(&offen, "P", &paket);
        assert_eq!(rest.len(), 1, "nur die noch offene Zeile geht in die Quarantäne");
        assert_eq!(rest[0].id, paket[1].id);
        // Bis die Quarantäne geschrieben ist, steht die verdrängte Zeile in
        // der Datei — ein Absturz in diesem Fenster verliert sie nicht.
        let datei = acars_fuer_datei(&offen, &verdraengt);
        assert_eq!(datei["P"].len(), 2);
        assert_eq!(datei["P"][0].id, paket[0].id, "die verdrängte, ältere Zeile steht vorn");
        acars_paket_entfernen(&mut offen, "P", &paket);
        assert!(acars_paket_noch_offen(&offen, "P", &paket).is_empty());
        assert_eq!(acars_fuer_datei(&offen, &verdraengt)["P"][0].id, paket[0].id);
    }

    #[test]
    fn ablauf_nur_wenn_alle_zeilen_alt_sind() {
        let mut offen = AcarsOffen::new();
        let alt = (Utc::now() - chrono::Duration::days(8)).to_rfc3339();
        let neu = Utc::now().to_rfc3339();
        offen.insert("ALT".into(), [zeile(&alt)].into_iter().collect());
        offen.insert("GEMISCHT".into(), [zeile(&alt), zeile(&neu)].into_iter().collect());
        assert!(acars_aelter_als(&offen, "ALT", ACARS_ABLAUF_TAGE));
        assert!(!acars_aelter_als(&offen, "GEMISCHT", ACARS_ABLAUF_TAGE));
        assert!(!acars_aelter_als(&offen, "FEHLT", ACARS_ABLAUF_TAGE));
    }

    #[test]
    fn kennung_geht_mit_an_phpvms() {
        let z = zeile("2026-10-12T00000001");
        let json = serde_json::to_value(&z).unwrap();
        assert_eq!(json["id"], serde_json::json!(z.id.clone().unwrap()));
        // Und überlebt die Ablage (Neustart).
        let zurueck: api_client::LogEntry = serde_json::from_value(json).unwrap();
        assert_eq!(zurueck.id, z.id);
    }
