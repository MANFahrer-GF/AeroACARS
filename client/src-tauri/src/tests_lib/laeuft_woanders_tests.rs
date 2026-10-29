    use super::*;

    fn pirep(updated_at: Option<&str>) -> api_client::PirepSummary {
        api_client::PirepSummary {
            id: "DqO0MADWPx7qr01d".into(),
            updated_at: updated_at.map(str::to_string),
            ..Default::default()
        }
    }

    fn jetzt() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-24T19:02:00Z").unwrap().with_timezone(&Utc)
    }

    /// QFA 442 meldete 40 s vorher von Windows — der Mac darf nicht übernehmen.
    #[test]
    fn frisch_meldender_flug_laeuft_woanders() {
        let p = pirep(Some("2026-10-24T19:01:20.000000Z"));
        assert_eq!(zuletzt_gemeldet_vor_s(&p, jetzt()), Some(40));
        assert!(laeuft_woanders(&p, jetzt()));
    }

    #[test]
    fn lange_stiller_flug_ist_verwaist() {
        assert!(!laeuft_woanders(&pirep(Some("2026-10-24T17:00:00Z")), jetzt()));
    }

    /// Ohne Zeitstempel keine Aussage — dann bleibt es beim Klick ohne Countdown.
    #[test]
    fn ohne_zeitstempel_keine_aussage() {
        assert!(!laeuft_woanders(&pirep(None), jetzt()));
        assert!(!laeuft_woanders(&pirep(Some("kaputt")), jetzt()));
    }
