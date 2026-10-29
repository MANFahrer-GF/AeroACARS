    use super::*;

    #[test]
    fn normalize_callsign_strips_hyphens_and_uppercases() {
        assert_eq!(normalize_callsign("DLH-100"), "DLH100");
        assert_eq!(normalize_callsign("gsg 100"), "GSG100");
        assert_eq!(normalize_callsign("dlh_100"), "DLH100");
        assert_eq!(normalize_callsign("  DLH100  "), "DLH100");
        assert_eq!(normalize_callsign(""), "");
    }

    #[test]
    fn split_callsign_separates_prefix_and_number() {
        assert_eq!(
            split_callsign("DLH100"),
            ("DLH".to_string(), "100".to_string())
        );
        assert_eq!(split_callsign("100"), ("".to_string(), "100".to_string()));
        assert_eq!(
            split_callsign("GSG100A"),
            ("GSG".to_string(), "100A".to_string())
        );
        assert_eq!(split_callsign(""), ("".to_string(), "".to_string()));
        assert_eq!(split_callsign("DLH"), ("DLH".to_string(), "".to_string()));
    }

    #[test]
    fn ofp_matches_when_callsigns_exact() {
        assert!(ofp_matches_active_flight(
            "EDDM", "LFPG", "DLH100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    #[test]
    fn ofp_matches_when_simbrief_callsign_is_number_only() {
        // SimBrief liefert nur "100", AeroACARS aktiv "DLH100" → match
        assert!(ofp_matches_active_flight(
            "EDDM", "LFPG", "100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    #[test]
    fn ofp_matches_case_insensitive_icao() {
        assert!(ofp_matches_active_flight(
            "eddm", "lfpg", "DLH100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
        assert!(ofp_matches_active_flight(
            "EDDM", "LFPG", "DLH100", "eddm", "LfPg", "DLH", "100", None, None,
        ));
    }

    /// KRITISCHER v1.1-Regression-Test: "DLH1100" darf NICHT mit "100"
    /// matchen weil die Numbers verschieden sind (1100 != 100).
    /// v1.5: bleibt korrekt — Kandidatenliste enthaelt "DLH100"/"100"
    /// aber NICHT "1100".
    #[test]
    fn ofp_rejects_when_callsign_numbers_overlap_but_differ() {
        // SimBrief-OFP = "DLH1100" fuer einen anderen Flug
        // AeroACARS-aktiv = "DLH100"
        assert!(!ofp_matches_active_flight(
            "EDDM", "LFPG", "DLH1100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
        // Auch umgekehrt
        assert!(!ofp_matches_active_flight(
            "EDDM", "LFPG", "DLH100", "EDDM", "LFPG", "DLH", "1100", None, None,
        ));
        // Und auch wenn SimBrief number-only liefert
        assert!(!ofp_matches_active_flight(
            "EDDM", "LFPG", "1100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    #[test]
    fn ofp_rejects_when_callsign_completely_different() {
        assert!(!ofp_matches_active_flight(
            "EDDM", "LFPG", "AFR300", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    #[test]
    fn ofp_rejects_when_dpt_wrong() {
        assert!(!ofp_matches_active_flight(
            "EDDF", "LFPG", "DLH100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    #[test]
    fn ofp_rejects_when_arr_wrong() {
        assert!(!ofp_matches_active_flight(
            "EDDM", "LFRS", "DLH100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    #[test]
    fn ofp_tolerates_empty_simbrief_callsign() {
        // SimBrief OFP ohne <atc><callsign> → dpt+arr genuegen.
        assert!(ofp_matches_active_flight(
            "EDDM", "LFPG", "", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    #[test]
    fn ofp_matches_with_hyphen_in_active() {
        // "DLH-100" wird normalisiert zu "DLH100", matched dann
        // SimBrief "DLH100" exakt.
        assert!(ofp_matches_active_flight(
            "EDDM", "LFPG", "DLH100", "EDDM", "LFPG", "DLH",
            "-100", // Pilot hat Bindestrich getippt
            None, None,
        ));
    }

    #[test]
    fn ofp_matches_with_whitespace_in_simbrief() {
        // SimBrief liefert "DLH 100" mit Leerzeichen — normalisiert auf "DLH100"
        assert!(ofp_matches_active_flight(
            "EDDM", "LFPG", "DLH 100", "EDDM", "LFPG", "DLH", "100", None, None,
        ));
    }

    // ─── v1.5 NEU: Pilot-Callsign-Kandidaten (CFG4TK-Case) ─────────────
    // Spec §6.1.2 — Real-Pilot-Beispiel von Thomas/GSG.

    /// CRITICAL v1.5: Pilot mit Bid CFG1504 und persoenlichem Callsign "4TK"
    /// generiert OFP "CFG4TK" → muss matchen via Kandidat 4 (airline +
    /// pilot_callsign).
    #[test]
    fn ofp_matches_pilot_callsign_with_airline_prefix() {
        assert!(ofp_matches_active_flight(
            "EDDF",
            "GCTS",
            "CFG4TK",
            "EDDF",
            "GCTS",
            "CFG",
            "1504",
            None,        // bid_callsign leer
            Some("4TK"), // pilot_callsign aus Profile
        ));
    }

    /// CRITICAL v1.5: SimBrief liefert nur "4TK" (number-only-mode) →
    /// muss matchen via Kandidat 5 (pilot_callsign allein).
    #[test]
    fn ofp_matches_pilot_callsign_number_only() {
        assert!(ofp_matches_active_flight(
            "EDDF",
            "GCTS",
            "4TK",
            "EDDF",
            "GCTS",
            "CFG",
            "1504",
            None,
            Some("4TK"),
        ));
    }

    /// v1.5: phpVMS-Bid.flight.callsign = "CFG4TK" → muss matchen
    /// via Kandidat 3 (bid.flight.callsign direkt). Pilot_callsign-Quelle
    /// optional/leer (= manche VAs nutzen die nicht).
    #[test]
    fn ofp_matches_bid_callsign_directly() {
        assert!(ofp_matches_active_flight(
            "EDDF",
            "GCTS",
            "CFG4TK",
            "EDDF",
            "GCTS",
            "CFG",
            "1504",
            Some("CFG4TK"), // bid.flight.callsign befuellt
            None,           // pilot_callsign leer
        ));
    }

    /// v1.5: SimBrief-OFP fuer einen ANDEREN Pilot-Flug (CFG mit anderer
    /// Number) darf trotz Pilot-Callsign nicht matchen — Sicherheits-Anker.
    #[test]
    fn ofp_rejects_other_flight_even_with_pilot_callsign() {
        // Pilot fliegt CFG1504, hat "4TK"-Callsign. SimBrief-OFP
        // gehoert zu CFG2000 (anderer Flug, gleiche Airline, KEIN
        // 4TK-Callsign) → MISMATCH.
        assert!(!ofp_matches_active_flight(
            "EDDF",
            "GCTS",
            "CFG2000",
            "EDDF",
            "GCTS",
            "CFG",
            "1504",
            None,
            Some("4TK"),
        ));
    }

    /// v1.5: Andere Airline mit zufaellig gleichem Suffix darf nicht matchen.
    #[test]
    fn ofp_rejects_other_airline_with_same_pilot_suffix() {
        // Pilot "4TK" bei CFG, aber SimBrief-OFP fuer Lufthansa mit "4TK" →
        // MISMATCH weil airline-prefix in den Kandidaten "CFG4TK"/"4TK"
        // ist, NICHT "DLH4TK".
        assert!(!ofp_matches_active_flight(
            "EDDF",
            "GCTS",
            "DLH4TK",
            "EDDF",
            "GCTS",
            "CFG",
            "1504",
            None,
            Some("4TK"),
        ));
    }

    /// v1.5: build_candidate_callsigns-Test direkt.
    #[test]
    fn build_candidate_callsigns_full_set() {
        let candidates = build_candidate_callsigns("CFG", "1504", Some("CFG4TK"), Some("4TK"));
        // Sollte enthalten: CFG1504, 1504, CFG4TK (Kand 3+4 dedupe), 4TK
        assert!(candidates.contains(&"CFG1504".to_string()));
        assert!(candidates.contains(&"1504".to_string()));
        assert!(candidates.contains(&"CFG4TK".to_string()));
        assert!(candidates.contains(&"4TK".to_string()));
        // Dedupe: "CFG4TK" sollte nur einmal vorkommen
        assert_eq!(
            candidates.iter().filter(|c| *c == "CFG4TK").count(),
            1,
            "candidates must dedupe duplicate forms",
        );
    }

    /// v1.5: Bei leerer Pilot-Callsign-Quelle faellt die Liste auf
    /// v1.1-Kandidaten (airline+flight_number, flight_number) zurueck.
    #[test]
    fn build_candidate_callsigns_v1_1_fallback() {
        let candidates = build_candidate_callsigns("CFG", "1504", None, None);
        assert!(candidates.contains(&"CFG1504".to_string()));
        assert!(candidates.contains(&"1504".to_string()));
        assert_eq!(candidates.len(), 2, "no pilot callsign → only 2 candidates");
    }
