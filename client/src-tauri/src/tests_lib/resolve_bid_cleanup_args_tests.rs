    use super::*;

    #[test]
    fn both_empty_returns_none() {
        // Caller hat weder bid noch flight_id → consume_bid_best_effort
        // skipt komplett (kein delete_bid, kein Queue-Eintrag).
        assert_eq!(resolve_bid_cleanup_args(0, None), None);
        assert_eq!(resolve_bid_cleanup_args(-5, Some("")), None);
        assert_eq!(resolve_bid_cleanup_args(0, Some("")), None);
    }

    #[test]
    fn only_bid_id_passes_bid_id() {
        let r = resolve_bid_cleanup_args(42, None);
        assert_eq!(r, Some((Some(42), None)));
    }

    #[test]
    fn only_flight_id_passes_flight_id() {
        // QS-R3 Finding 1 anchor: bid_id<=0 darf NICHT mehr early-out
        // sein wenn flight_id present ist.
        let r = resolve_bid_cleanup_args(0, Some("flightid_42"));
        assert_eq!(r, Some((None, Some("flightid_42".to_string()))));
    }

    #[test]
    fn negative_bid_id_with_flight_id_passes_flight_only() {
        let r = resolve_bid_cleanup_args(-1, Some("flightid_42"));
        assert_eq!(r, Some((None, Some("flightid_42".to_string()))));
    }

    #[test]
    fn both_ids_pass_both() {
        // delete_bid baut intern die Cascade (bid_id zuerst, bei 400/
        // 404/422 → flight_id-Fallback). Hier nur die Path-Selection
        // pruefen.
        let r = resolve_bid_cleanup_args(7, Some("flightid_7"));
        assert_eq!(r, Some((Some(7), Some("flightid_7".to_string()))));
    }

    #[test]
    fn empty_string_flight_id_treated_as_none() {
        // Pre-v0.7.7-Resume-Files persistieren `flight_id: String`
        // ohne Inhalt. Der wird als None behandelt.
        let r = resolve_bid_cleanup_args(0, Some(""));
        assert_eq!(r, None);
        let r2 = resolve_bid_cleanup_args(42, Some(""));
        assert_eq!(r2, Some((Some(42), None)));
    }
