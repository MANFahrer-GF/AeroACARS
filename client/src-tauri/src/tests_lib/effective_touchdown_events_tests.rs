    use super::*;

    #[test]
    fn empty_vec_stays_empty_when_no_landing_at_all() {
        let stats = FlightStats::default();
        assert!(effective_touchdown_events(&stats).is_empty());
    }

    /// Kern-Regression (Joel EWG3552): ein fallback_zero-Flug darf NIE
    /// einen synthetischen FinalLanding bekommen — sonst wuerde dieser
    /// Helper genau das Signal verdecken, das der Score-Fix sichtbar
    /// machen soll.
    #[test]
    fn empty_vec_stays_empty_for_fallback_zero() {
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(0.0);
        stats.landing_source = Some("fallback_zero".to_string());
        assert!(effective_touchdown_events(&stats).is_empty());
    }

    /// DLH2064-Regression: leerer Vec, aber ein echter (nicht-fallback)
    /// Touchdown wurde von der Kanonik erfasst → genau 1 synthetisierter
    /// FinalLanding-Eintrag.
    #[test]
    fn empty_vec_backfills_one_final_landing_for_genuine_touchdown() {
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-409.0);
        stats.landing_peak_vs_fpm = Some(-409.0);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        stats.landing_peak_g_force = Some(1.46);
        let events = effective_touchdown_events(&stats);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, TouchdownKind::FinalLanding));
        assert_eq!(events[0].peak_vs_fpm, -409.0);
        assert_eq!(events[0].peak_g, 1.46);
    }

    /// Die tiefere Luecke, die die erste Fix-Version noch hatte: ein
    /// echtes Touch-and-Go steht schon im Vec (also NICHT leer), aber die
    /// finale Landung wurde gerastet. Der Helper muss trotzdem nachlegen,
    /// statt beim ersten nicht-leeren Vec sofort "fertig" zu sagen.
    #[test]
    fn tg_present_but_final_missing_still_gets_backfilled() {
        let mut stats = FlightStats::default();
        stats.touchdown_events.push(TouchdownEvent {
            timestamp: Utc::now(),
            kind: TouchdownKind::TouchAndGo,
            peak_vs_fpm: -250.0,
            peak_g: 1.2,
            lat: 50.0,
            lon: 8.0,
            sub_bounces: 0,
        });
        stats.landing_rate_fpm = Some(-409.0);
        stats.landing_peak_vs_fpm = Some(-409.0);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        let events = effective_touchdown_events(&stats);
        assert_eq!(events.len(), 2, "T&G + backfilled final, not just the T&G");
        assert!(matches!(events[0].kind, TouchdownKind::TouchAndGo));
        assert!(matches!(events[1].kind, TouchdownKind::FinalLanding));
    }

    /// QS-Fix (Adversarial-Review): auf dem Sampler-Pfad (Bush/VFR) wird
    /// `landing_at` nie gestampt, nur `sampler_touchdown_at` — der
    /// synthetisierte Eintrag muss DIESEN Zeitstempel nehmen, nicht die
    /// PIREP-Filing-Zeit (`Utc::now()`).
    #[test]
    fn backfill_prefers_sampler_touchdown_at_over_now_when_landing_at_missing() {
        let mut stats = FlightStats::default();
        let real_touchdown = Utc::now() - chrono::Duration::minutes(4);
        stats.sampler_touchdown_at = Some(real_touchdown);
        stats.landing_rate_fpm = Some(-90.0);
        stats.landing_peak_vs_fpm = Some(-90.0);
        stats.landing_source = Some("sampler_gear_force".to_string());
        let events = effective_touchdown_events(&stats);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].timestamp, real_touchdown);
    }

    /// Wenn der Vec bereits einen FinalLanding-Eintrag hat, wird NICHTS
    /// nachgelegt — kein doppelter Eintrag.
    #[test]
    fn existing_final_landing_is_never_duplicated() {
        let mut stats = FlightStats::default();
        stats.touchdown_events.push(TouchdownEvent {
            timestamp: Utc::now(),
            kind: TouchdownKind::FinalLanding,
            peak_vs_fpm: -180.0,
            peak_g: 1.1,
            lat: 50.0,
            lon: 8.0,
            sub_bounces: 0,
        });
        stats.landing_rate_fpm = Some(-180.0);
        stats.landing_peak_vs_fpm = Some(-180.0);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        let events = effective_touchdown_events(&stats);
        assert_eq!(events.len(), 1);
    }
