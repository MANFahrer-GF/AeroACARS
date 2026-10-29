    use super::{ausgereizter_bid_eintrag, AusgereizterBidEintrag::*};

    const ICH: &str = "https://german-sky-group.eu|5";
    const ANDERER: &str = "https://german-sky-group.eu|23";

    fn bids() -> Vec<(i64, String)> {
        vec![(4711, "flug-a".to_string()), (4712, "flug-b".to_string())]
    }

    /// Der Fall aus den Diagnose-Logs: Alt-Eintrag ohne Eigentuemer, Bid
    /// laengst weg — darf nicht mehr fuer immer liegen bleiben.
    #[test]
    fn alter_eintrag_ohne_bid_wird_verworfen() {
        let b = bids();
        assert_eq!(
            ausgereizter_bid_eintrag(None, ICH, Some(99), Some("flug-x"), Some(&b)),
            Verwerfen
        );
        assert_eq!(
            ausgereizter_bid_eintrag(Some(ICH), ICH, Some(99), None, Some(&b)),
            Verwerfen
        );
        assert_eq!(
            ausgereizter_bid_eintrag(None, ICH, Some(99), None, Some(&[])),
            Verwerfen
        );
    }

    #[test]
    fn noch_vorhandener_bid_bleibt() {
        let b = bids();
        assert_eq!(
            ausgereizter_bid_eintrag(Some(ICH), ICH, Some(4711), None, Some(&b)),
            Behalten
        );
        // Ohne bid_id haelt die flight_id den Eintrag fest.
        assert_eq!(
            ausgereizter_bid_eintrag(None, ICH, None, Some("flug-b"), Some(&b)),
            Behalten
        );
    }

    /// Gegenprobe: ein Netzfehler beim Abruf darf nichts verwerfen.
    #[test]
    fn gescheiterter_abruf_verwirft_nichts() {
        assert_eq!(
            ausgereizter_bid_eintrag(None, ICH, Some(99), None, None),
            Behalten
        );
        assert_eq!(
            ausgereizter_bid_eintrag(Some(ICH), ICH, None, Some("flug-x"), None),
            Behalten
        );
    }

    /// Die Bid-Liste zeigt nur das aktuelle Konto — ueber den Bid eines
    /// anderen Kontos sagt sie nichts, der Eintrag bleibt in Quarantaene.
    #[test]
    fn fremder_eintrag_bleibt_auch_ohne_treffer() {
        let b = bids();
        assert_eq!(
            ausgereizter_bid_eintrag(Some(ANDERER), ICH, Some(99), None, Some(&b)),
            Behalten
        );
    }

    /// bid_id hat Vorrang: passt nur die flight_id, der Bid mit der
    /// gespeicherten Nummer ist aber weg, ist der Eintrag erledigt.
    #[test]
    fn bid_id_entscheidet_vor_flight_id() {
        let b = bids();
        assert_eq!(
            ausgereizter_bid_eintrag(None, ICH, Some(99), Some("flug-a"), Some(&b)),
            Verwerfen
        );
    }
