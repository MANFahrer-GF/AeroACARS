    use super::{bid_aufraeum_entscheidung, BidAufraeumen::*};

    /// Alter PIREP auf `flug-a`, vorgefiled um 10:00 (Serveruhr).
    const PIREP: Option<&str> = Some("2026-11-13T10:00:00.000000Z");

    fn bid(id: i64, flug: &str, angelegt: Option<&str>) -> api_client::Bid {
        let zeit = angelegt.map(|t| format!(r#", "created_at": "{t}""#)).unwrap_or_default();
        serde_json::from_str(&format!(
            r#"{{"id": {id}, "user_id": 5, "flight_id": "{flug}",
                "flight": {{"id": "{flug}", "flight_number": "1",
                  "dpt_airport_id": "EDDF", "arr_airport_id": "EDDM"}}{zeit}}}"#
        ))
        .expect("Bid-JSON")
    }

    /// Der Fehler: alte Buchung 4711 ist weg, der Pilot hat `flug-a` neu
    /// gebucht (4800). Weder per `bid_id` noch per `flight_id` loeschen.
    #[test]
    fn neue_buchung_auf_demselben_flug_bleibt_mit_bid_id() {
        let b = [bid(4800, "flug-a", Some("2026-11-13T12:00:00Z"))];
        assert_eq!(bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), Some(&b), None), Erledigt);
    }

    /// Dasselbe ohne `bid_id`: die Buchung ist juenger als der alte PIREP.
    #[test]
    fn neue_buchung_auf_demselben_flug_bleibt_ohne_bid_id() {
        let b = [bid(4800, "flug-a", Some("2026-11-13T12:00:00Z"))];
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), PIREP), Erledigt);
    }

    /// Berechtigter Orphan-Fall: die alte Buchung liegt noch — loeschen, mit
    /// `flight_id`-Rueckfall (auf dem Flug liegt nur sie).
    #[test]
    fn alte_buchung_wird_geloescht() {
        let b = [
            bid(4711, "flug-a", Some("2026-11-13T09:00:00Z")),
            bid(4712, "flug-b", Some("2026-11-13T11:00:00Z")),
        ];
        assert_eq!(
            bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), Some(&b), None),
            Loeschen(Some(4711), Some("flug-a".into()))
        );
        assert_eq!(
            bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), PIREP),
            Loeschen(Some(4711), Some("flug-a".into())),
            "ohne bid_id: die VOR dem PIREP angelegte Buchung ist die alte"
        );
    }

    /// Keine Buchung mehr auf dem Flug: Ziel erreicht.
    #[test]
    fn nichts_mehr_da_ist_erledigt() {
        let b = [bid(4712, "flug-b", None)];
        assert_eq!(bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), Some(&b), None), Erledigt);
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), None), Erledigt);
    }

    /// Bid-Liste nicht abrufbar: per `bid_id` (404 = erledigt, sicher), nie
    /// per `flight_id`; ohne `bid_id` gar nicht.
    #[test]
    fn ohne_bid_liste_nie_ueber_flight_id() {
        assert_eq!(
            bid_aufraeum_entscheidung(Some(4711), Some("flug-a"), None, PIREP),
            Loeschen(Some(4711), None)
        );
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), None, PIREP), Liegenlassen);
    }

    /// Ohne `bid_id` und ohne lesbare Zeiten ist die Buchung nicht
    /// zuzuordnen — nichts loeschen.
    #[test]
    fn ohne_zeiten_liegenlassen() {
        let b = [bid(4711, "flug-a", None)];
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), PIREP), Liegenlassen);
        let b = [bid(4711, "flug-a", Some("2026-11-13T09:00:00Z"))];
        assert_eq!(bid_aufraeum_entscheidung(None, Some("flug-a"), Some(&b), None), Liegenlassen);
    }

    mod laufender_flug {
        use super::super::bid_trifft_laufenden_flug;

        const LAUFEND: (&str, i64, &str) = ("pirep-neu", 4711, "flug-a");

        /// Pilot hat denselben Flug mit derselben, nie geloeschten Buchung neu
        /// gestartet: Aufraeumen des alten PIREP darf sie nicht anfassen.
        #[test]
        fn gleiche_bid_id_anderer_pirep_trifft() {
            assert!(bid_trifft_laufenden_flug("pirep-alt", Some(4711), None, LAUFEND));
        }

        #[test]
        fn gleiche_flight_id_anderer_pirep_trifft() {
            assert!(bid_trifft_laufenden_flug("pirep-alt", None, Some("flug-a"), LAUFEND));
            assert!(bid_trifft_laufenden_flug("pirep-alt", Some(99), Some("flug-a"), LAUFEND));
        }

        #[test]
        fn derselbe_pirep_oder_anderer_flug_trifft_nicht() {
            assert!(!bid_trifft_laufenden_flug("pirep-neu", Some(4711), Some("flug-a"), LAUFEND));
            assert!(!bid_trifft_laufenden_flug("pirep-alt", Some(99), Some("flug-b"), LAUFEND));
            assert!(!bid_trifft_laufenden_flug("pirep-alt", None, Some(""), ("pirep-neu", 0, "")));
        }
    }

    /// Wiring: in beiden Aufraeumwegen stehen Laufend-Sperre und Entscheidung
    /// VOR dem `delete_bid`. Fenster bis zur naechsten Top-Level-Funktion.
    #[test]
    fn sperre_und_entscheidung_stehen_vor_delete_bid() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        for start in [
            "async fn drain_pending_bid_cleanup(",
            "async fn flight_cancel_orphan(",
        ] {
            let ab = SRC.find(start).unwrap_or_else(|| panic!("{start} nicht gefunden — Test anpassen"));
            let rest = &SRC[ab..];
            let ende = ["\nfn ", "\nasync fn ", "\n#[tauri::command]"]
                .iter()
                .filter_map(|m| rest[1..].find(m).map(|i| i + 1))
                .min()
                .unwrap_or(rest.len());
            let koerper = &rest[..ende];
            let pos = |n: &str| koerper.find(n).unwrap_or_else(|| panic!("{n} fehlt in {start}"));
            let delete = pos("client.delete_bid(");
            assert!(pos("bid_gehoert_laufendem_flug(") < delete, "{start}: Laufend-Sperre nach delete_bid");
            assert!(pos("bid_aufraeum_entscheidung(") < delete, "{start}: Entscheidung nach delete_bid");
        }
    }
