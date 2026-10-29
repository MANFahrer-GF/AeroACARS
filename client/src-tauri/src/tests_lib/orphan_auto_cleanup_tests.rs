    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-13T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// Älter als 24 h → aufräumen.
    #[test]
    fn cleans_up_orphan_older_than_24h() {
        assert!(should_auto_cleanup(
            Some("2026-06-12T11:00:00+00:00"), // 25 h alt
            now(),
            None,
            "pirep_old",
        ));
    }

    /// Frischer Orphan (< 24 h) → liegen lassen (Resume-Banner-Flow).
    #[test]
    fn keeps_fresh_orphan() {
        assert!(!should_auto_cleanup(
            Some("2026-06-13T10:00:00+00:00"), // 2 h alt
            now(),
            None,
            "pirep_fresh",
        ));
    }

    /// Exakt 24 h → noch NICHT aufräumen (strikt „älter als").
    #[test]
    fn keeps_orphan_at_exactly_24h() {
        assert!(!should_auto_cleanup(
            Some("2026-06-12T12:00:00+00:00"),
            now(),
            None,
            "pirep_boundary",
        ));
    }

    /// Kandidat == lokal aktiver/resumender Flug → nie anfassen,
    /// egal wie alt.
    #[test]
    fn never_touches_active_pirep() {
        assert!(!should_auto_cleanup(
            Some("2026-06-01T00:00:00+00:00"), // uralt
            now(),
            Some("pirep_active"),
            "pirep_active",
        ));
    }

    /// created_at fehlt → konservativ liegen lassen.
    #[test]
    fn keeps_orphan_without_created_at() {
        assert!(!should_auto_cleanup(None, now(), None, "pirep_no_ts"));
    }

    /// created_at unparsebar → konservativ liegen lassen.
    #[test]
    fn keeps_orphan_with_unparseable_created_at() {
        assert!(!should_auto_cleanup(
            Some("gestern Mittag"),
            now(),
            None,
            "pirep_bad_ts",
        ));
    }

    /// Anderer PIREP als der aktive, alt genug → aufräumen (der
    /// active-Schutz greift nur bei ID-Gleichheit).
    #[test]
    fn cleans_up_old_orphan_while_other_flight_is_active() {
        assert!(should_auto_cleanup(
            Some("2026-06-10T00:00:00+00:00"),
            now(),
            Some("pirep_active"),
            "pirep_other",
        ));
    }
