    use super::*;

    /// Spiegel der `step_flight`-Bedingung fuer den FinalLanding-Push.
    /// Seit 20.10.2026 heisst der erste Teil `(elapsed_secs >
    /// FINAL_LANDING_VERBUCHEN_NACH_SECS || rollt_ab)`; `tg_in_window` hier
    /// steht fuer dessen Gegenteil („noch nicht faellig“):
    ///
    /// ```ignore
    /// if (elapsed_secs > FINAL_LANDING_VERBUCHEN_NACH_SECS || rollt_ab)
    ///     && stats.landing_at == Some(touchdown)
    ///     && stats.touchdown_events.last()
    ///         .map(|e| e.timestamp != touchdown)
    ///         .unwrap_or(true)
    /// { push }
    /// ```
    fn should_push(
        tg_in_window: bool,
        touchdown: DateTime<Utc>,
        landing_at: Option<DateTime<Utc>>,
        last_event_ts: Option<DateTime<Utc>>,
    ) -> bool {
        !tg_in_window
            && landing_at == Some(touchdown)
            && last_event_ts.map(|ts| ts != touchdown).unwrap_or(true)
    }

    fn t(secs: i64) -> DateTime<Utc> {
        DateTime::<Utc>::from_timestamp(secs, 0).expect("valid ts")
    }

    #[test]
    fn pushes_when_tg_window_expired_and_landing_active_and_no_prior_event() {
        // GSG219-Fall: kein T&G im Window, landing_at zeigt auf den
        // aktiven Touchdown, touchdown_events ist leer → push.
        assert!(should_push(false, t(100), Some(t(100)), None));
    }

    #[test]
    fn pushes_when_prior_event_has_different_timestamp() {
        // Dedup-Bedingung: ein frueheres TouchdownEvent (z.B. ein
        // verworfener T&G-Versuch) darf den FinalLanding-Push nicht
        // blocken, solange der Timestamp anders ist.
        assert!(should_push(false, t(200), Some(t(200)), Some(t(180))));
    }

    #[test]
    fn does_not_push_inside_tg_window() {
        // Klassischer T&G-Schutz: solange das T&G-Watch-Fenster offen
        // ist, darf der FinalLanding-Push nicht feuern (sonst wuerde
        // ein T&G faelschlich als FinalLanding klassifiziert).
        assert!(!should_push(true, t(100), Some(t(100)), None));
    }

    #[test]
    fn does_not_push_when_landing_at_is_none() {
        // Z.B. nach einem T&G das `stats.landing_at = None` setzt.
        assert!(!should_push(false, t(100), None, None));
    }

    #[test]
    fn does_not_push_when_landing_at_belongs_to_a_different_touchdown() {
        // Defensiv: falls landing_at irgendwie auf einen anderen
        // Touchdown zeigt (race condition), nicht pushen.
        assert!(!should_push(false, t(100), Some(t(99)), None));
    }

    #[test]
    fn does_not_double_push_on_same_timestamp() {
        // Wenn der gleiche Touchdown schon mal gepusht wurde, kein
        // zweiter Push im selben Tick oder Folge-Tick.
        assert!(!should_push(false, t(100), Some(t(100)), Some(t(100))));
    }
