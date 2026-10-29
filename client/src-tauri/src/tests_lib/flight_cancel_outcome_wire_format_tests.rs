    use super::*;

    #[test]
    fn filed_instead_serializes_with_snake_case_kind() {
        let v = FlightCancelOutcome::FiledInstead {
            pirep_id: "pirep_123".into(),
        };
        let json: serde_json::Value = serde_json::to_value(v).unwrap();
        assert_eq!(json["kind"], "filed_instead");
        assert_eq!(json["pirep_id"], "pirep_123");
    }

    #[test]
    fn queued_serializes_with_snake_case_kind() {
        let v = FlightCancelOutcome::Queued {
            pirep_id: "pirep_456".into(),
        };
        let json: serde_json::Value = serde_json::to_value(v).unwrap();
        assert_eq!(json["kind"], "queued");
        assert_eq!(json["pirep_id"], "pirep_456");
    }

    #[test]
    fn cancelled_serializes_with_snake_case_kind() {
        let v = FlightCancelOutcome::Cancelled {
            pirep_id: "pirep_789".into(),
        };
        let json: serde_json::Value = serde_json::to_value(v).unwrap();
        assert_eq!(json["kind"], "cancelled");
        assert_eq!(json["pirep_id"], "pirep_789");
    }
