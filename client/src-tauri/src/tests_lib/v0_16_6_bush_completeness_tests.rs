    //! v0.16.6 — tests for the two final VFR/bush landing-completeness
    //! pieces: phase-independent approach sampling + sampler-path stability
    //! window (`should_push_approach_sample` / the
    //! `compute_approach_stability_v2` time-window parameter) and the
    //! phase-independent rollout (`rollout_tick` /
    //! `sampler_path_rollout_tick` / the T&G episode reset
    //! `clear_approach_stability_and_rollout`).

    use super::*;
    use chrono::TimeZone;
    use sim_core::SimSnapshot;

    fn td() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 11, 12, 0, 0).unwrap()
    }

    fn sample_at(at: DateTime<Utc>, agl: f32, vs: f32) -> ApproachBufferSample {
        ApproachBufferSample {
            at,
            agl_ft: agl,
            msl_ft: agl,
            gs_kt: 100.0,
            ias_kt: 105.0,
            vs_fpm: vs,
            bank_deg: 1.0,
            heading_true_deg: 90.0,
            gear_position: 1.0,
            flaps_position: 1.0,
            selected_runway: None,
            stall_warning: false,
            lat: None,
            lon: None,
            pitch_deg: None,
            n1_mittel_pct: None,
            headwind_kt: None,
            crosswind_kt: None,
        }
    }

    // ─── (1) central-sampling equivalence: Approach/Final ────────────────

    #[test]
    fn central_gate_always_pushes_in_approach_and_final() {
        // The old arm-local calls pushed UNCONDITIONALLY on every
        // Approach/Final tick. The central gate must do the same regardless
        // of airborne state / AGL / V/S — including pathological values.
        for phase in [FlightPhase::Approach, FlightPhase::Final] {
            for was_airborne in [false, true] {
                for on_ground in [false, true] {
                    for agl in [0.0, 500.0, 5000.0] {
                        for vs in [-1500.0, 0.0, 1500.0] {
                            assert!(
                                should_push_approach_sample(
                                    phase,
                                    was_airborne,
                                    on_ground,
                                    agl,
                                    vs
                                ),
                                "Approach/Final must always sample \
                                 ({phase:?}, wa={was_airborne}, og={on_ground}, \
                                 agl={agl}, vs={vs})"
                            );
                        }
                    }
                }
            }
        }
    }

    // ─── (2) bush AGL-sampling: descending below 2000 ft only ────────────

    #[test]
    fn central_gate_bush_descent_fires_outside_approach_final() {
        // GSG312-class: FSM stuck in Takeoff/Climb on a short bush hop —
        // airborne, below 2000 ft AGL, sinking → sample. The second gate
        // half is deliberately phase-independent.
        for phase in [
            FlightPhase::Takeoff,
            FlightPhase::Climb,
            FlightPhase::Cruise,
            FlightPhase::Descent,
            FlightPhase::Landing,
        ] {
            assert!(
                should_push_approach_sample(phase, true, false, 1500.0, -300.0),
                "descending low-level segment must sample in {phase:?}"
            );
        }
    }

    #[test]
    fn central_gate_bush_blocks_level_climbing_ground_high_or_never_airborne() {
        let p = FlightPhase::Climb;
        // level (above the -100 fpm descent gate)
        assert!(!should_push_approach_sample(p, true, false, 1500.0, -50.0));
        // climbing
        assert!(!should_push_approach_sample(p, true, false, 1500.0, 300.0));
        // on ground
        assert!(!should_push_approach_sample(p, true, true, 0.0, -300.0));
        // too high
        assert!(!should_push_approach_sample(p, true, false, 2500.0, -300.0));
        // never airborne (taxi gear-bumps / pre-flight)
        assert!(!should_push_approach_sample(
            p, false, false, 1500.0, -300.0
        ));
    }

    // ─── (3) stability-v2 time window ─────────────────────────────────────

    /// Buffer: 3 gate-band samples inside the 180 s window, 3 gate-band
    /// samples 1000 s before touchdown (the "38-min low-level bush cruise"),
    /// 1 sample after the touchdown.
    fn windowed_buf() -> std::collections::VecDeque<ApproachBufferSample> {
        let mut buf = std::collections::VecDeque::new();
        // poison: old low-level manoeuvring, wildly unstable V/S
        buf.push_back(sample_at(
            td() - chrono::Duration::seconds(1010),
            900.0,
            -1500.0,
        ));
        // v1.9.16: die Sinkgrenze zaehlt erst nach 10 s am Stueck — der
        // Gift-Lauf muss 10 s lang und lueckenlos (≤ 4 s Abstand) sein.
        for (sek, vs) in [(1007, -1400.0), (1004, -1300.0), (1001, -1200.0)] {
            buf.push_back(sample_at(td() - chrono::Duration::seconds(sek), 600.0, vs));
        }
        buf.push_back(sample_at(
            td() - chrono::Duration::seconds(1000),
            300.0,
            -1200.0,
        ));
        // the actual approach, inside the window, calm
        buf.push_back(sample_at(
            td() - chrono::Duration::seconds(120),
            900.0,
            -600.0,
        ));
        buf.push_back(sample_at(
            td() - chrono::Duration::seconds(60),
            600.0,
            -610.0,
        ));
        buf.push_back(sample_at(
            td() - chrono::Duration::seconds(20),
            300.0,
            -590.0,
        ));
        // post-touchdown sample (e.g. T&G climb-out) — must never count
        buf.push_back(sample_at(
            td() + chrono::Duration::seconds(10),
            200.0,
            -400.0,
        ));
        buf
    }

    #[test]
    fn stability_window_excludes_samples_outside_window() {
        let buf = windowed_buf();
        // td_ts = None isolates the new window parameter from the
        // pre-existing flare-cutoff filter.
        let windowed = compute_approach_stability_v2(
            &buf,
            None,
            None,
            None,
            Some((td(), chrono::Duration::seconds(180))),
            Default::default(),
        );
        assert_eq!(
            windowed.window_sample_count, 3,
            "only the 3 in-window approach samples may count"
        );
        // the calm in-window approach must NOT look excessive — the old
        // -1500 fpm poison sample is outside the window
        assert_eq!(windowed.excessive_sink, Some(false));

        let unwindowed =
            compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(
            unwindowed.window_sample_count, 9,
            "None = old behaviour: every gate-band sample counts"
        );
        assert_eq!(
            unwindowed.excessive_sink,
            Some(true),
            "without the window the poison sample leaks in"
        );
    }

    #[test]
    fn stability_window_none_equals_covering_window() {
        // A window that covers every pre-TD sample must be bit-identical to
        // `None` — proves the filter-then-recurse path changes nothing but
        // membership. (Buffer without the post-TD sample, since a covering
        // window still excludes samples after the touchdown.)
        let mut buf = windowed_buf();
        buf.pop_back();
        let a = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        let b = compute_approach_stability_v2(
            &buf,
            None,
            None,
            None,
            Some((td(), chrono::Duration::days(365))),
            Default::default(),
        );
        assert_eq!(a.window_sample_count, b.window_sample_count);
        assert_eq!(a.vs_jerk_fpm, b.vs_jerk_fpm);
        assert_eq!(a.ias_stddev_kt, b.ias_stddev_kt);
        assert_eq!(a.bank_stddev_filtered_deg, b.bank_stddev_filtered_deg);
        assert_eq!(a.vs_deviation_fpm, b.vs_deviation_fpm);
        assert_eq!(
            a.max_vs_deviation_below_500_fpm,
            b.max_vs_deviation_below_500_fpm
        );
        assert_eq!(a.excessive_sink, b.excessive_sink);
        assert_eq!(a.stable_config, b.stable_config);
        assert_eq!(a.stable_at_gate, b.stable_at_gate);
        assert_eq!(a.stable_at_da, b.stable_at_da);
        assert_eq!(a.runway_changed_late, b.runway_changed_late);
        assert_eq!(a.used_hat, b.used_hat);
        assert_eq!(a.stall_warning_count, b.stall_warning_count);
    }

    #[test]
    fn approach_buffer_window_drops_post_touchdown_samples() {
        let buf = windowed_buf();
        let w = approach_buffer_window(&buf, td(), chrono::Duration::seconds(180));
        assert_eq!(w.len(), 3);
        assert!(w.iter().all(|s| s.at <= td()));
    }

    // ─── (4+5) rollout helper: accumulate + finalise ──────────────────────

    fn rollout_snap(lat: f64, lon: f64, gs: f32, hdg: f32, on_ground: bool) -> SimSnapshot {
        SimSnapshot {
            lat,
            lon,
            groundspeed_kt: gs,
            heading_deg_true: hdg,
            on_ground,
            ..SimSnapshot::default()
        }
    }

    /// Stats as the sampler stamp initialises them: distance 0, last =
    /// touchdown coords, TD heading stamped.
    fn rollout_stats(lat: f64, lon: f64) -> FlightStats {
        let mut stats = FlightStats::default();
        stats.rollout_distance_m = Some(0.0);
        stats.rollout_finalized = false;
        stats.rollout_last_lat = Some(lat);
        stats.rollout_last_lon = Some(lon);
        stats.landing_heading_true_deg = Some(90.0);
        stats
    }

    // ---- segment_is_flown(): Sprung vs. Flug -------------------------
    //
    // Feldbefund Michael K. (PIREP 6QgxNLQP8b62XaKq, 12.07.2026): EDAV->EDAZ,
    // real 42 NM, im Logbuch 4987 NM bei 40 Minuten Flugzeit. Ueber alle
    // ACARS-Clients hinweg traf die Klasse 20 PIREPs.

    /// Zwei echte Punkte, ihre Strecke entscheidet. Start bewusst NICHT
    /// bei 0/0 — das waere die Null-Position und traefe eine andere Regel.
    const BER: (f64, f64) = (52.36, 13.50);
    /// ~0,55 NM oestlich: eine Reiseflug-Probe im 4-Sekunden-Takt.
    const BER_NAH: (f64, f64) = (52.36, 13.515);
    /// Frankfurt, ~232 NM entfernt — als Tick unmoeglich.
    const FRA: (f64, f64) = (50.033, 8.570);
    const NULL_ISLAND: (f64, f64) = (0.0, 0.0);

    fn urteil(von: (f64, f64), nach: (f64, f64), dt: Option<f64>) -> SegmentVerdict {
        classify_segment(von, nach, None, dt, ODOMETER_LIMITS)
    }

    /// Reiseflug-GS fuer die Zeitraffer-Tests.
    const GS_REISE: f64 = 450.0;

    fn urteil_rate(von: (f64, f64), nach: (f64, f64), dt: f64, rate: f64) -> SegmentVerdict {
        classify_segment_rate(
            von,
            nach,
            None,
            Some(dt),
            ODOMETER_LIMITS,
            Some((rate, GS_REISE)),
        )
    }

    /// ~5 NM oestlich von BER: in 5 s real nur mit Zeitraffer fliegbar
    /// (Reiseflug 450 kt x 8 x 5 s = 5 NM). Feldbefund TGW 882.
    const BER_5NM: (f64, f64) = (52.36, 13.6363);

    #[test]
    fn zeitraffer_strecke_wird_gezaehlt_statt_als_sprung_verworfen() {
        assert_eq!(
            urteil_rate(BER, BER_5NM, 5.0, 1.0),
            SegmentVerdict::Jump("Positionssprung"),
            "ohne Zeitraffer bleibt das ein Sprung"
        );
        assert_eq!(urteil_rate(BER, BER_5NM, 5.0, 8.0), SegmentVerdict::Flown);
    }

    /// Kantentest der Formel (Abnahme 03.10.2026: Faktor 1,5, +1 NM und die
    /// Kappung der Groundspeed waren nur in der Richtung getestet; ein Faktor
    /// 3 oder 1 haette alle anderen Tests bestanden).
    /// Bei 8x, 60 s und 450 kt: 450 x 1,5 x 8 x 60/3600 + 1 = 91 NM erlaubt.
    /// Ein Breitengrad sind 60,04 NM, 1,49 Grad also 89,5 und 1,55 Grad 93,1 NM.
    #[test]
    fn zeitraffer_grenze_liegt_bei_faktor_eineinhalb_plus_einer_meile() {
        let von = (52.0, 13.5);
        assert_eq!(
            urteil_rate(von, (52.0 + 1.49, 13.5), 60.0, 8.0),
            SegmentVerdict::Flown,
            "89,5 NM liegen unter der Grenze von 91 NM"
        );
        assert_eq!(
            urteil_rate(von, (52.0 + 1.55, 13.5), 60.0, 8.0),
            SegmentVerdict::Jump("Positionssprung"),
            "93,1 NM liegen ueber der Grenze von 91 NM"
        );
    }

    /// Die Groundspeed wird auf 700 kt gekappt: 700 x 1,5 x 8 x 60/3600 + 1 =
    /// 141 NM. 2,2 Grad sind 132 NM, 2,4 Grad 144 NM — auch wenn der Sim
    /// 1000 kt meldet.
    #[test]
    fn zeitraffer_kappt_die_groundspeed_bei_700_knoten() {
        let von = (52.0, 13.5);
        let urteil = |dlat: f64| {
            classify_segment_rate(
                von,
                (52.0 + dlat, 13.5),
                None,
                Some(60.0),
                ODOMETER_LIMITS,
                Some((8.0, 1000.0)),
            )
        };
        assert_eq!(urteil(2.2), SegmentVerdict::Flown);
        assert_eq!(urteil(2.4), SegmentVerdict::Jump("Positionssprung"));
    }

    #[test]
    fn teleport_bleibt_auch_bei_zeitraffer_ein_sprung() {
        for rate in [8.0, 16.0, 1000.0] {
            assert_eq!(
                urteil_rate(BER, FRA, 4.0, rate),
                SegmentVerdict::Jump("Positionssprung"),
                "232 NM in 4 s darf bei Rate {rate} nie als Flug zaehlen"
            );
        }
    }

    #[test]
    fn rate_ueber_der_obergrenze_wird_gedeckelt() {
        // 5 NM in 0,1 s waeren nur bei Rate > 1000 moeglich.
        assert_eq!(
            urteil_rate(BER, BER_5NM, 0.1, 1000.0),
            SegmentVerdict::Jump("Positionssprung")
        );
    }

    #[test]
    fn rate_unter_eins_oder_unsinn_verkleinert_die_grenze_nie() {
        for rate in [0.25, 0.5, 0.0, -3.0, f64::NAN, f64::INFINITY] {
            assert_eq!(
                urteil_rate(BER, BER_NAH, 4.0, rate),
                urteil(BER, BER_NAH, Some(4.0)),
                "Rate {rate} darf das Urteil gegenueber 1x nicht aendern"
            );
        }
        assert_eq!(urteil_rate(BER, BER_NAH, 4.0, 0.25), SegmentVerdict::Flown);
    }

    #[test]
    fn lange_luecke_bei_16x_laesst_keinen_weltkarten_sprung_durch() {
        // QS 03.10.2026: Mit 2200 kt x 16 haetten 60 s ~587 NM erlaubt.
        // Mit der gemeldeten GS (450 kt x 1,5 x 16) sind es ~181 NM —
        // BER->FRA (232 NM) bleibt ein Sprung, auch bei Unsinns-GS.
        assert_eq!(
            urteil_rate(BER, FRA, 60.0, 16.0),
            SegmentVerdict::Jump("Positionssprung")
        );
        assert_eq!(
            classify_segment_rate(
                BER,
                FRA,
                None,
                Some(30.0),
                ODOMETER_LIMITS,
                Some((16.0, 50_000.0))
            ),
            SegmentVerdict::Jump("Positionssprung"),
            "GS wird auf 700 kt gedeckelt (30 s x 16 x 700 x 1,5 = 140 NM)"
        );
    }

    #[test]
    fn zeitraffer_ohne_groundspeed_bleibt_bei_der_normalen_grenze() {
        assert_eq!(
            classify_segment_rate(
                BER,
                BER_5NM,
                None,
                Some(5.0),
                ODOMETER_LIMITS,
                Some((8.0, 0.0))
            ),
            SegmentVerdict::Jump("Positionssprung")
        );
        assert_eq!(
            classify_segment_rate(BER, BER_5NM, None, Some(5.0), ODOMETER_LIMITS, None),
            SegmentVerdict::Jump("Positionssprung")
        );
    }

    #[test]
    fn zeitraffer_verlaengert_keine_funkstille() {
        assert_eq!(urteil_rate(BER, BER_NAH, 300.0, 16.0), SegmentVerdict::Gap);
    }

    // ---- RESET_LIMITS: das erprobte Verhalten der Aufzeichnungsschleife
    //      muss die gemeinsame Regel unveraendert wiedergeben.

    #[test]
    fn reset_toleranz_bleibt_bei_zehn_meilen() {
        let reset = |von, nach, alt, dt| classify_segment(von, nach, alt, Some(dt), RESET_LIMITS);
        // 0,55 NM: weit unter der flachen Grenze, egal wie kurz der Abstand.
        assert_eq!(reset(BER, BER_NAH, None, 0.02), SegmentVerdict::Flown);
        assert_eq!(reset(BER, BER_NAH, None, 2.0), SegmentVerdict::Flown);
        // Berlin -> Frankfurt sind 232 NM — deutlich ueber 10.
        assert!(matches!(
            reset(BER, FRA, None, 1.0),
            SegmentVerdict::Jump(_)
        ));
    }

    #[test]
    fn reset_toleranz_haengt_nicht_am_abstand() {
        // Anders als die Streckenzaehlung: die flache Grenze gilt
        // unabhaengig vom Abstand, solange er unter 2 s bleibt. Genau so
        // arbeitete die Schleife vorher auch.
        let reset = |dt| classify_segment(BER, FRA, None, Some(dt), RESET_LIMITS);
        assert!(matches!(reset(0.02), SegmentVerdict::Jump(_)));
        assert!(matches!(reset(1.99), SegmentVerdict::Jump(_)));
        // Darueber wird nicht mehr geurteilt, sondern neu angesetzt.
        assert_eq!(reset(2.5), SegmentVerdict::Gap);
    }

    #[test]
    fn reset_hoehensprung_wird_erkannt() {
        let u = classify_segment(BER, BER_NAH, Some(12_000.0), Some(1.0), RESET_LIMITS);
        assert!(matches!(u, SegmentVerdict::Jump(_)));
        // Die Streckenzaehlung prueft die Hoehe bewusst NICHT — ein
        // Hoehensprung allein verfaelscht keinen Kilometerzaehler.
        assert_eq!(
            classify_segment(BER, BER_NAH, Some(12_000.0), Some(1.0), ODOMETER_LIMITS),
            SegmentVerdict::Flown
        );
    }

    #[test]
    fn segment_normaler_reiseflug_zaehlt() {
        // ~0,55 NM in 4 s = rund 500 kt. Muss durchgehen.
        assert_eq!(urteil(BER, BER_NAH, Some(4.0)), SegmentVerdict::Flown);
    }

    #[test]
    fn segment_kleiner_zuschlag_deckt_takt_jitter() {
        // Der feste Zuschlag von 1 NM dominiert bei sehr kurzen Abstaenden:
        // dieselbe halbe Meile gilt auch bei 0,2 s noch als geflogen,
        // obwohl das rechnerisch rund 9900 kt waeren. Das ist gewollt — ein
        // schwankender Takt darf keine Strecke verschlucken, und 1 NM
        // Unschaerfe faellt im Logbuch nicht ins Gewicht. Die Sprungfaelle,
        // um die es geht, liegen um Groessenordnungen darueber.
        assert_eq!(urteil(BER, BER_NAH, Some(0.2)), SegmentVerdict::Flown);
        assert_eq!(urteil(BER, BER_NAH, Some(4.0)), SegmentVerdict::Flown);
        // Ein echter Sprung faellt auch im selben kurzen Abstand durch.
        assert!(matches!(
            urteil(BER, FRA, Some(0.2)),
            SegmentVerdict::Jump(_)
        ));
    }

    #[test]
    fn segment_versetzen_zaehlt_nicht() {
        // Michaels Fall in klein: Berlin -> Frankfurt in einem Tick.
        assert!(matches!(
            urteil(BER, FRA, Some(4.0)),
            SegmentVerdict::Jump(_)
        ));
    }

    #[test]
    fn segment_null_position_sprung_zaehlt_nicht() {
        // Null Island -> Berlin, der haeufigste Fall vor v0.19.3 — und
        // ebenso der Rueckweg, wenn der Simulator kurz zurueckfaellt.
        assert!(matches!(
            urteil(NULL_ISLAND, BER, Some(2.0)),
            SegmentVerdict::Jump(_)
        ));
        assert!(matches!(
            urteil(BER, NULL_ISLAND, Some(2.0)),
            SegmentVerdict::Jump(_)
        ));
    }

    #[test]
    fn segment_zu_grosse_luecke_zaehlt_nicht() {
        // Was in fuenf Minuten geschah, wissen wir nicht — lieber zu wenig
        // zaehlen als eine Strecke erfinden.
        assert_eq!(urteil(BER, BER_NAH, Some(300.0)), SegmentVerdict::Gap);
    }

    #[test]
    fn segment_rueckwaerts_laufende_zeit_zaehlt_nicht() {
        assert_eq!(urteil(BER, BER_NAH, Some(-10.0)), SegmentVerdict::Gap);
    }

    #[test]
    fn segment_ohne_zeitbezug_faellt_auf_die_strecke_zurueck() {
        // Sitzungen, die vor diesem Feld begannen, tragen keine Zeit.
        assert_eq!(urteil(BER, BER_NAH, None), SegmentVerdict::Flown);
        assert!(matches!(urteil(BER, FRA, None), SegmentVerdict::Jump(_)));
    }

    #[test]
    fn rollout_tick_accumulates_haversine_distance() {
        let mut stats = rollout_stats(50.0, 8.0);
        let snap = rollout_snap(50.0, 8.001, 80.0, 90.0, true);
        rollout_tick(&mut stats, &snap);
        let expected = ::geo::distance_m(50.0, 8.0, 50.0, 8.001);
        let got = stats.rollout_distance_m.expect("distance accumulated");
        assert!(
            (got - expected).abs() < 0.01,
            "got {got}, expected {expected}"
        );
        assert!(!stats.rollout_finalized, "gs 80 kt must not finalise");
        assert_eq!(stats.rollout_last_lat, Some(50.0));
        assert_eq!(stats.rollout_last_lon, Some(8.001));
    }

    #[test]
    fn rollout_tick_finalizes_exit_speed() {
        let mut stats = rollout_stats(50.0, 8.0);
        let snap = rollout_snap(50.0, 8.0005, 35.0, 90.0, true);
        rollout_tick(&mut stats, &snap);
        assert!(stats.rollout_finalized);
        assert_eq!(stats.rollout_finalize_reason.as_deref(), Some("exit_speed"));
    }

    /// 06.10.2026: Die Verzögerung entsteht erst am Ende des Ausrollens —
    /// beim Aufsetzen ist die Strecke 0, dort blieb die Bremsenergie leer.
    #[test]
    fn verzoegerung_entsteht_am_ende_des_ausrollens() {
        let mut stats = rollout_stats(50.0, 8.0);
        stats.landing_groundspeed_kt = Some(135.0);
        stats.rollout_distance_m = Some(1_200.0);
        // Noch im Ausrollen (80 kt): kein Wert.
        rollout_tick(&mut stats, &rollout_snap(50.0, 8.0001, 80.0, 90.0, true));
        assert_eq!(stats.landing_decel_mps2, None);
        // Abschluss bei 35 kt: (69,4² − 18,0²) / (2 × ~1236 m) ≈ 1,82 m/s².
        rollout_tick(&mut stats, &rollout_snap(50.0, 8.0005, 35.0, 90.0, true));
        assert!(stats.rollout_finalized);
        let a = stats.landing_decel_mps2.expect("Verzögerung gesetzt");
        assert!((1.75..1.90).contains(&a), "Verzögerung {a}");
        // Die alte Bremsenergie wird nicht mehr gesetzt.
        assert_eq!(stats.landing_brake_energy_proxy, None);
    }

    #[test]
    fn verzoegerung_unabhaengig_vom_gewicht_und_plausibel() {
        // Bis zum Stand: 135 kt auf 1200 m ≈ 2,0 m/s² — Gewicht spielt keine Rolle.
        let a = verzoegerung_mps2(Some(135.0), 0.0, Some(1_200.0)).unwrap();
        assert!((1.95..2.05).contains(&a), "Verzögerung {a}");
        // Zu kurze Strecke, fehlende Werte, keine Abnahme: leer.
        assert_eq!(verzoegerung_mps2(Some(135.0), 0.0, Some(40.0)), None);
        assert_eq!(verzoegerung_mps2(None, 0.0, Some(1_200.0)), None);
        assert_eq!(verzoegerung_mps2(Some(135.0), 0.0, None), None);
        assert_eq!(verzoegerung_mps2(Some(30.0), 35.0, Some(400.0)), None);
        // Messlücke (135 kt auf 150 m ≈ 16 m/s²): kein Wert statt Unsinn.
        assert_eq!(verzoegerung_mps2(Some(135.0), 0.0, Some(150.0)), None);
    }

    #[test]
    fn rollout_tick_finalizes_full_stop_below_5kt() {
        let mut stats = rollout_stats(50.0, 8.0);
        let snap = rollout_snap(50.0, 8.0005, 3.0, 90.0, true);
        rollout_tick(&mut stats, &snap);
        assert!(stats.rollout_finalized);
        assert_eq!(stats.rollout_finalize_reason.as_deref(), Some("full_stop"));
    }

    #[test]
    fn rollout_tick_finalizes_on_runway_turnoff() {
        let mut stats = rollout_stats(50.0, 8.0);
        // 40° off the TD heading, still fast — turned onto a taxiway.
        let snap = rollout_snap(50.0, 8.0005, 100.0, 130.0, true);
        rollout_tick(&mut stats, &snap);
        assert!(stats.rollout_finalized);
        assert_eq!(
            stats.rollout_finalize_reason.as_deref(),
            Some("turned_off_runway")
        );
    }

    #[test]
    fn rollout_tick_slow_but_airborne_does_not_finalize() {
        // exit-speed / full-stop triggers require on_ground (a slow airborne
        // bounce sample must not end the rollout).
        let mut stats = rollout_stats(50.0, 8.0);
        let snap = rollout_snap(50.0, 8.0005, 3.0, 90.0, false);
        rollout_tick(&mut stats, &snap);
        assert!(!stats.rollout_finalized);
    }

    #[test]
    fn rollout_tick_noop_once_finalized() {
        let mut stats = rollout_stats(50.0, 8.0);
        stats.rollout_finalized = true;
        stats.rollout_distance_m = Some(450.0);
        let snap = rollout_snap(51.0, 9.0, 80.0, 90.0, true);
        rollout_tick(&mut stats, &snap);
        assert_eq!(stats.rollout_distance_m, Some(450.0));
        assert_eq!(stats.rollout_last_lat, Some(50.0), "last coords frozen");
    }

    // ─── (5) sampler-path rollout: gate + accumulate-to-full-stop ────────

    #[test]
    fn die_spur_reisst_in_der_ausfahrt_nicht_ab() {
        // Der Fall aus dem Feld (raKOnJD1XgNbP06q, EDDH 23): In der
        // Ausfahrt faehrt das Flugzeug quer. Die Laengsposition aendert
        // sich ueber zwanzig Meter kaum, der Querversatz um dutzende.
        //
        // Mit einer Ausduennung, die nur laengs misst, faellt dort JEDER
        // Punkt weg -- die Spur endet an der Bahnkante, und im Diagramm
        // sieht es aus, als loese sie sich auf. Der Abstand muss deshalb
        // ueber beide Achsen gehen.
        let echte_punkte: &[(f64, f64)] = &[
            (1848.0, 0.6),
            (1879.0, -18.6),
            (1901.0, -46.7),
            (1907.0, -80.0),  // nur 6 m laengs weiter -- aber 33 m quer
            (1898.0, -109.4), // laengs sogar rueckwaerts, 30 m quer
        ];
        let mut abgelegt: Vec<(f32, f32)> = Vec::new();
        for (lg, qr) in echte_punkte {
            let weit_genug = abgelegt.last().is_none_or(|(l, q)| {
                (lg - *l as f64).hypot(qr - *q as f64) >= crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_min_abstand_m.wert
            });
            if weit_genug {
                abgelegt.push((*lg as f32, *qr as f32));
            }
        }
        assert_eq!(
            abgelegt.len(),
            echte_punkte.len(),
            "kein Punkt der Ausfahrt darf verloren gehen"
        );

        // Und in der Kurve muss es DICHTER werden als auf der Geraden.
        //
        // Die Queransicht ist ueberhoeht: Zehn Meter laengs sind dort 3,4
        // Pixel, zehn Meter quer vierzig. Ohne das Quergewicht liegen die
        // Punkte genau dort am weitesten auseinander, wo die Kurve am
        // staerksten kruemmt.
        let quer_weg: Vec<(f64, f64)> = (0..12)
            .map(|i| (1900.0 + i as f64 * 2.0, i as f64 * -8.0))
            .collect();
        let mut in_kurve = 0;
        let mut letzter: Option<(f64, f64)> = None;
        for (lg, qr) in &quer_weg {
            let nimm = letzter.is_none_or(|(l, q)| {
                (lg - l).hypot((qr - q) * crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_quer_gewicht.wert) >= crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_min_abstand_m.wert
            });
            if nimm {
                in_kurve += 1;
                letzter = Some((*lg, *qr));
            }
        }
        // Dieselben zwoelf Punkte auf der Geraden: dort greift die
        // Ausduennung staerker, weil zwei Meter Laengsweg wenig sind.
        let mut auf_gerade = 0;
        let mut letzter: Option<f64> = None;
        for i in 0..12 {
            let lg = 1900.0 + i as f64 * 2.0;
            if letzter.is_none_or(|l| (lg - l as f64).abs() >= crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_min_abstand_m.wert) {
                auf_gerade += 1;
                letzter = Some(lg);
            }
        }
        assert!(
            in_kurve > auf_gerade,
            "in der Kurve {in_kurve} Punkte, auf der Geraden {auf_gerade} — \
             das Quergewicht wirkt nicht"
        );

        // Die Spur muss auf der LEITUNG gerundet ankommen.
        //
        // Der Groessentest im mqtt-Crate prueft nur die Werte, die er
        // selbst erzeugt — er wuerde nicht merken, wenn `wire()` das
        // Runden vergisst. Diese Pruefung geht durch die echte
        // Umrechnung.
        //
        // Ein `f32` mit dem Wert 523,2 serialisiert als
        // `523.2000122070312`: achtzehn Zeichen fuer eine Groesse, die
        // auf zehn Zentimeter gemeint ist. Ueber 400 Punkte sind das
        // 23 KB statt 13 — und derselbe Payload liegt danach in der
        // Datenbank und im Flugprotokoll.
        {
            let mut stats = FlightStats::default();
            stats.bahn_spur = vec![(523.2, -5.7), (1907.35, -80.04)];
            let wire = bahn_felder(&stats, None, None).wire(true);
            let spur = wire.lateral_samples.expect("Spur liegt vor");
            let text = serde_json::to_string(&spur).unwrap();
            assert!(
                !text.contains("0000") && !text.contains("9999"),
                "die Spur geht ungerundet auf die Leitung: {text}"
            );
            assert_eq!(spur[0].laengs_m, 523.2);
            assert_eq!(spur[1].quer_m, -80.0, "35 mm werden nicht uebertragen");
            // Und die Rundung darf nichts Sichtbares kosten: ein Dezimeter
            // ist in der Zeichnung weniger als ein Pixel.
            assert!((spur[1].laengs_m - 1907.35).abs() <= 0.05);
        }

        // Entartete Eingaben duerfen keine Zahlen erfinden.
        //
        // Runde 3 der QS, mit dem Winkel „was passiert bei Unsinn".
        // Die Werte hier kommen aus dem Simulator und aus Navdaten; beide
        // liefern gelegentlich Nullen, und eine Rechnung, die daraus einen
        // Randabstand macht, stellt eine Behauptung neben echte Messwerte,
        // die von ihnen nicht zu unterscheiden ist.
        {
            let mut stats = FlightStats::default();

            // Gar keine Bahn: Alles leer, nichts geraten.
            let f = bahn_felder(&stats, Some("A320"), None);
            assert!(f.runway_width_m.is_none(), "Bahnbreite ohne Bahntreffer");
            assert!(f.min_edge_clearance_m.is_none(), "Randabstand ohne Bahn");
            assert!(f.runway_exits.is_empty());

            // Bahnbreite null — kommt in OurAirports vor.
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "XXXX".to_string(),
                runway_ident: "09".to_string(),
                heading_true_deg: 90.0,
                length_ft: 4000.0,
                width_ft: 0.0,
                surface: "ASP".to_string(),
                threshold_lat: 50.0,
                threshold_lon: 8.0,
                end_lat: 50.0,
                end_lon: 8.017,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 300.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            stats.bahn_max_querversatz_m = Some(3.0);
            let f = bahn_felder(&stats, Some("A320"), None);
            assert!(
                f.runway_width_m.is_none(),
                "Breite 0 muss als unbekannt durchgehen, nicht als Bahn ohne Breite"
            );
            assert!(
                f.min_edge_clearance_m.is_none(),
                "ohne Breite darf kein Randabstand entstehen — er stuende \
                 neben echten Messwerten und waere nicht davon zu unterscheiden"
            );

            // Unbekanntes Muster: keine Spurweite, also auch kein Randabstand.
            stats.runway_match.as_mut().unwrap().width_ft = 151.0;
            let f = bahn_felder(&stats, Some("ZZZZ"), None);
            assert!(f.track_width_m.is_none(), "erfundene Spurweite fuer ZZZZ");
            assert!(f.track_width_source.is_none(), "Quelle ohne Wert");
            assert!(f.min_edge_clearance_m.is_none());

            // Mit Muster: jetzt darf gerechnet werden — und das Ergebnis
            // muss zur Geometrie passen.
            let f = bahn_felder(&stats, Some("A320"), None);
            let spur = f.track_width_m.expect("A320 steht in der Tabelle");
            let rand = f.min_edge_clearance_m.expect("alle drei Groessen da");
            let halbe = 151.0 * 0.3048 / 2.0;
            // Bis zur AUSSENKANTE des aeusseren Reifens, nicht zur
            // Bein-Mitte: Die Herstellerspurweite misst von Bein zu Bein,
            // das Radpaket steht noch eine halbe Breite weiter draussen.
            let aussen = landing_scoring::spurweite::aussenkante_halb_aus_spur(spur);
            assert!(
                (rand - (halbe - (3.0 + aussen))).abs() < 0.01,
                "Randabstand {rand} passt nicht zu halber Breite {halbe} und Aussenkante {aussen}"
            );
            assert!(
                aussen > spur / 2.0,
                "die Aussenkante liegt nicht weiter aussen als die Bein-Mitte — \
                 rechnet die Anzeige noch mit `spur / 2.0`?"
            );
        }

        // Die Spur beginnt am AUFSETZPUNKT, nicht zweihundert Meter danach.
        //
        // # Der Befund
        //
        // Gemessen an allen neun Landungen der ersten v1.7.1-Nacht:
        //
        //   LGAV 218 m · LEPA 251 · SGAS 213 · EDDK 216 · LOWW 213
        //   EPWA 222 · EDDN 439 · EDDB 185 · LSZH 228
        //
        // Konstant zweihundert Meter hinter dem Aufsetzpunkt — es fehlte
        // die Aufsetzzone, also genau der Teil, um dessentwillen die Achse
        // gebaut wurde. In der Queransicht stand die Marke „Aufsetzen" im
        // Leeren, ohne Spur darunter.
        //
        // Ursache: Die Spur braucht `runway_match`, und der entsteht erst,
        // wenn der Aufsetzer nach 1,1 s bestaetigt ist — plus zwei, drei
        // Ticks im langsamen Takt. Bei 130 kt sind das gut zweihundert
        // Meter.
        {
            let td = Utc::now();
            let mut stats = FlightStats::default();
            stats.landing_at = Some(td);
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "XXXX".to_string(),
                runway_ident: "09".to_string(),
                heading_true_deg: 90.0,
                length_ft: 10000.0,
                width_ft: 148.0,
                surface: "ASP".to_string(),
                threshold_lat: 50.0,
                threshold_lon: 8.0,
                end_lat: 50.0,
                end_lon: 8.0426,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 1600.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });

            // Fuenf Sekunden Telemetrie ab dem Aufsetzen, 200 ms Takt,
            // rund 67 m/s — so schnell rollt eine Verkehrsmaschine kurz
            // nach dem Aufsetzen. Start bei 500 m hinter der Schwelle.
            let meter_je_grad_lon = 111_320.0 * (50.0f64).to_radians().cos();
            for i in 0..25 {
                let laengs = 500.0 + 13.4 * i as f64;
                stats.snapshot_buffer.push_back(TelemetrySample {
                    at: td + chrono::Duration::milliseconds(200 * i),
                    on_ground: true,
                    groundspeed_kt: 130.0,
                    lat: 50.0,
                    lon: 8.0 + laengs / meter_je_grad_lon,
                    ..Default::default()
                });
            }

            let halbe = 148.0 * 0.3048 / 2.0;
            spur_aus_puffer_nachtragen(&mut stats, halbe);

            let spur = &stats.bahn_spur;
            assert!(
                !spur.is_empty(),
                "der Nachtrag hat nichts geschrieben — die Spur beginnt \
                 weiterhin erst beim ersten Live-Tick, gut zweihundert \
                 Meter hinter dem Aufsetzpunkt"
            );
            let erster = spur[0].0;
            assert!(
                (erster - 500.0).abs() < 25.0,
                "die Spur beginnt bei {erster:.0} m statt am Aufsetzpunkt \
                 (500 m) — es fehlen {:.0} m",
                erster - 500.0
            );
            // Und die Ausduennung greift wie im Live-Takt: bei 13,4 m
            // Probenabstand und 10 m Mindestabstand bleibt fast jede.
            assert!(
                spur.len() >= 20,
                "nur {} von 25 Proben uebernommen — die Ausduennung wirft \
                 im Nachtrag mehr weg als im Live-Takt",
                spur.len()
            );
        }

        // Anzeige und Bewertung müssen dieselben Zahlen sehen.
        //
        // # Warum das eine eigene Prüfung braucht
        //
        // Beide leiten Spurweite, Bahnbreite, Belag und Versatz aus
        // demselben `stats` ab — aber über ZWEI Ausdrücke an zwei Stellen:
        // `bahn_felder` für die Anzeige, `BahndisziplinInput` für die Note.
        // Heute stimmen sie überein. Das ist kein Zustand, den man
        // absichern kann, indem man ihn einmal prüft: Wer eine der beiden
        // Formeln anfasst, merkt vom anderen Weg nichts.
        //
        // Live gesehen am 24.08.2026 bei LGAV 03R (v1.7.1): Der Datensatz
        // trug `track_width_m: 7.59`, die Bewertung sagte
        // `track_width_unknown`. Zwei Antworten auf dieselbe Frage — und
        // niemand hat es bemerkt, bis ein Pilot den Bericht aufmachte.
        //
        // Diese Prüfung geht über viele Eingaben statt über ein Beispiel:
        // Breiten von null bis absurd, Spurweiten mit und ohne
        // Flugzeugdatei, jeder Belag, jeder Versatz.
        {
            let belaege = ["ASP", "CON", "GRS", "WATER", "", "TURF-G"];
            let breiten_ft = [0.0f32, 20.0, 98.0, 148.0, 197.0, 500.0];
            // "A300" (Codex-QS-Fund, BCS3317-Folgefund Vereinheitlichung
            // 09/2026): einzig ueber die `muster_kandidaten`-Kaskade
            // aufloesbar ("A300" -> "A306"), nicht ueber einen direkten
            // Tabellentreffer wie A320/BCS3 oder das Fallback-Verhalten von
            // ZZZZ/None. Deckt genau die Luecke ab, die vorher gruen blieb,
            // weil kein Kandidat hier ausschliesslich auf die Kaskade
            // angewiesen war.
            let muster = [Some("A320"), Some("BCS3"), Some("A300"), Some("ZZZZ"), None];
            let aus_datei = [None, Some(9.9f64)];

            let mut geprueft = 0;
            for belag in belaege {
                for breite in breiten_ft {
                    for m in muster {
                        for datei in aus_datei {
                            let mut stats = FlightStats::default();
                            stats.runway_match = Some(runway::RunwayMatch {
                                airport_ident: "XXXX".to_string(),
                                runway_ident: "09".to_string(),
                                heading_true_deg: 90.0,
                                length_ft: 8000.0,
                                width_ft: breite,
                                surface: belag.to_string(),
                                threshold_lat: 50.0,
                                threshold_lon: 8.0,
                                end_lat: 50.0,
                                end_lon: 8.034,
                                centerline_distance_m: 0.0,
                                centerline_distance_abs_ft: 0.0,
                                touchdown_distance_from_threshold_ft: 300.0,
                                side: "left".to_string(),
                                displaced_threshold_ft: 0,
                                geometry_implied_displaced_threshold_ft: 0,
                            });
                            stats.bahn_max_querversatz_m = Some(4.2);
                            stats.fahrwerk_spurweite_m = datei;
                            stats.bahn_proben = 40;

                            // Weg 1: die Anzeige.
                            let anzeige = bahn_felder(&stats, m, None);

                            // Weg 2: die Bewertung, über denselben
                            // Eingang, den der Betrieb benutzt.
                            let mut eingang = scoring_eingang(&stats, m, None, None);
                            fill_v2_rollout_fields(&mut eingang, &stats, "XXXX");

                            let b_spur = eingang.fahrwerk_spurweite_m.or_else(|| {
                                landing_scoring::spurweite::spurweite_m(
                                    eingang.aircraft_icao.as_deref(),
                                )
                            });
                            let b_belag = landing_scoring::belag::belag_aus_angabe(
                                eingang.runway_surface.as_deref(),
                            );
                            let a_belag = landing_scoring::belag::belag_aus_angabe(Some(belag));

                            let lage = format!(
                                "Belag={belag:?} Breite={breite} Muster={m:?} Datei={datei:?}"
                            );
                            assert_eq!(
                                anzeige.track_width_m, b_spur,
                                "Spurweite weicht ab — {lage}"
                            );
                            // ⚠ Die Anzeige fuehrt die Breite als `f64`,
                            // die Bewertung als `f32` — beim Schreiben
                            // dieser Pruefung aufgefallen, weil der
                            // Vergleich sich nicht uebersetzen liess.
                            // Fachlich harmlos (eine Bahnbreite braucht
                            // keine achtzehn Stellen), aber es sind zwei
                            // Zahlen fuer dieselbe Groesse. Verglichen
                            // wird deshalb mit der Genauigkeit von `f32`.
                            match (anzeige.runway_width_m, eingang.runway_width_m) {
                                (Some(a), Some(b)) => assert!(
                                    (a - b as f64).abs() < 0.001,
                                    "Bahnbreite weicht ab: {a} gegen {b} — {lage}"
                                ),
                                (a, b) => assert_eq!(
                                    a.is_none(),
                                    b.is_none(),
                                    "Bahnbreite: einer kennt sie, der andere nicht — {lage}"
                                ),
                            }
                            assert_eq!(b_belag, a_belag, "Belag weicht ab — {lage}");
                            assert_eq!(
                                anzeige.max_lateral_offset_m, eingang.bahn_max_querversatz_m,
                                "Versatz weicht ab — {lage}"
                            );
                            geprueft += 1;
                        }
                    }
                }
            }
            assert_eq!(
                geprueft,
                belaege.len() * breiten_ft.len() * muster.len() * aus_datei.len(),
                "die Schleife hat nicht alle Kombinationen durchlaufen"
            );
        }

        // Bei 39 Knoten ist die Spur noch NICHT fertig.
        //
        // `rollout_finalized` faellt bei vierzig Knoten — dort steht die
        // Ausrollstrecke fest. Die Spur laeuft weiter: bis fuenf Knoten,
        // bis zur Ausfahrt oder bis achtzig Meter neben die Bahn.
        //
        // Bis zur QS-Runde 25 hing `bahndisziplin_tick` im selben `if` wie
        // die Streckenzaehlung. Die „finale" Spur brach damit bei vierzig
        // Knoten ab, mitten auf der Bahn — und der Raeumpunkt, der typisch
        // bei zwanzig bis dreissig Knoten erreicht wird, blieb leer.
        {
            let mut stats = FlightStats::default();
            stats.sampler_touchdown_at = Some(Utc::now());
            stats.phase = FlightPhase::TaxiIn;
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0,
                surface: "ASP".to_string(),
                threshold_lat: 53.636011,
                threshold_lon: 9.999656,
                end_lat: 53.619958,
                end_lon: 9.967167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            // Die Ausrollstrecke ist schon finalisiert (unter 40 kt).
            stats.rollout_finalized = true;
            stats.bahn_fenster_zu = true;
            let vorher = stats.bahn_spur.len();

            // 39 Knoten, noch auf der Bahn: 1500 m entlang, 6 m links.
            let mut snap = SimSnapshot {
                groundspeed_kt: 39.0,
                on_ground: true,
                ..Default::default()
            };
            // 1500 m entlang der Bahn, 6 m links der Mittellinie —
            // ausgerechnet, weil das geo-Crate nur Abstaende kann.
            snap.lat = 53.627_335;
            snap.lon = 9.982_235;
            sampler_path_rollout_tick(&mut stats, &snap);

            assert!(
                stats.bahn_spur.len() > vorher,
                "bei 39 kt wird nicht mehr aufgezeichnet — die Spur bricht \
                 mitten auf der Bahn ab"
            );
        }

        // Das Fensterende wird festgehalten — und liegt VOR dem
        // Kurswechsel, wenn die Fahrt zuerst faellt.
        //
        // # Warum das eine eigene Zahl braucht
        //
        // Bei DLH369 (EDDM 26L, 25.08.2026) meldete der Client einen
        // groessten Querversatz von 12,9 m, waehrend die Spur innerhalb
        // der Bewertungsgrenze noch bis 15,2 m lief. Beides stimmte: der
        // Hoechstwert waechst nur, solange das Messfenster offen ist, und
        // das schliesst unter sechzig Knoten — rund 600 Meter vor dem
        // Kurswechsel, an dem `scoring_cutoff_m` haengt. Die Anzeige
        // zeichnete die Grenze an der falschen Stelle und stellte damit
        // zwei richtige Zahlen als Widerspruch dar.
        //
        // # Warum der Test den echten Tick faehrt
        //
        // Ein erster Entwurf setzte `bahn_fenster_zu_laengs_m` von Hand
        // und prueste dann, dass `bahn_felder` es durchreicht. Der war
        // gruen, ohne eine einzige Zeile der Messung zu beruehren — er
        // haette auch gehalten, wenn `bahndisziplin_tick` die Zahl nie
        // schreibt. Deshalb laeuft hier der echte Weg.
        {
            // Bahn entlang des Aequators nach Osten, 4.000 m lang: ein
            // Grad Laenge sind dort 111.320 m, das rechnet sich im Kopf
            // nach und macht die Sollwerte pruefbar.
            const GRAD_M: f64 = 111_320.0;
            let mut stats = FlightStats::default();
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDM".to_string(),
                runway_ident: "26L".to_string(),
                heading_true_deg: 90.0,
                length_ft: (4000.0 * 3.280_839_895) as f32,
                width_ft: 197.0,
                surface: "ASP".to_string(),
                threshold_lat: 0.0,
                threshold_lon: 0.0,
                end_lat: 0.0,
                end_lon: 4000.0 / GRAD_M,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 2300.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            stats.landing_heading_true_deg = Some(90.0);

            let tick = |stats: &mut FlightStats, laengs: f64, gs: f32, kurs: f32| {
                let mut snap = SimSnapshot::default();
                snap.lat = 0.0;
                snap.lon = laengs / GRAD_M;
                snap.groundspeed_kt = gs;
                snap.heading_deg_true = kurs;
                bahndisziplin_tick(stats, &snap);
            };

            // Auf der Bahn, schnell, geradeaus — Fenster offen.
            tick(&mut stats, 1000.0, 80.0, 90.0);
            assert!(
                stats.bahn_fenster_zu_laengs_m.is_none(),
                "bei 80 kt darf noch nichts geschlossen sein",
            );

            // Fahrt faellt unter sechzig, Kurs noch gerade: HIER endet
            // die Messung, nicht erst beim Abbiegen.
            tick(&mut stats, 1662.0, 55.0, 90.0);
            let fenster = stats
                .bahn_fenster_zu_laengs_m
                .expect("Fensterende muss beim Unterschreiten stehen");
            assert!(
                (fenster - 1662.0).abs() < 5.0,
                "Fenster schloss bei {fenster:.0} m, erwartet rund 1.662 m",
            );

            // Erst deutlich spaeter der Kurswechsel — der setzt den
            // Raeumpunkt und darf das Fensterende NICHT verschieben.
            tick(&mut stats, 2251.0, 48.0, 60.0);
            assert_eq!(
                stats.bahn_fenster_zu_laengs_m.map(|v| (v as i64) / 10),
                Some(166),
                "der Kurswechsel hat das Fensterende ueberschrieben",
            );

            let f = bahn_felder(&stats, Some("BCS3"), None);
            let durchgereicht = f
                .mess_ende_laengs_m
                .expect("Fensterende muss in den Feldern stehen");
            assert!(
                (durchgereicht - fenster).abs() < 0.001,
                "Feld traegt {durchgereicht:.1}, gemessen war {fenster:.1}",
            );
            if let Some(cut) = f.scoring_cutoff_m {
                assert!(
                    durchgereicht < cut,
                    "Fensterende {durchgereicht:.0} m muss vor dem \
                     Kurswechsel {cut:.0} m liegen",
                );
            }
        }

        // Ein Ausbrechen ueber die Kante ist kein Raeumen der Bahn.
        //
        // Live laesst sich nicht wissen, ob das Flugzeug draussen BLEIBT.
        // `bahn_felder` rechnet die Stelle darum aus der ganzen Spur nach —
        // dieselbe Regel, die auch das Pruefwerkzeug benutzt.
        //
        // Am Korpus gemessen: 763 von 772 Landungen stimmen mit der
        // Live-Regel ueberein, neun weichen ab. Bei HAGN 35 um 1533 Meter.
        {
            let mut stats = FlightStats::default();
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0, // halbe Breite 23,0 m
                surface: "ASP".to_string(),
                threshold_lat: 53.636011,
                threshold_lon: 9.999656,
                end_lat: 53.619958,
                end_lon: 9.967167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            // Ausbrechen bei 800 m (26 m quer), zurueck auf die Bahn,
            // echte Ausfahrt erst bei 1600 m.
            stats.bahn_spur = vec![
                (600.0, -2.0),
                (800.0, -26.0), // Ausbrechen — jenseits der Kante
                (900.0, -8.0),  // wieder drauf
                (1500.0, -12.0),
                (1600.0, -25.0), // ab hier bleibt es draussen
                (1650.0, -60.0),
            ];
            // Die Live-Messung haette das Ausbrechen erwischt.
            stats.bahn_kante_laengs_m = Some(800.0);
            stats.bahn_kante_gs_kt = Some(70.0);
            // UND der Kurswechsel hat seine eigene Fahrt.
            //
            // Ohne diese Zeile ist der Test blind fuer den eigentlichen
            // Fehler: `clearance_speed_kt` fiel still auf den Wert des
            // Kurswechsels zurueck, sobald die Nachrechnung den Uebertritt
            // verschob. Mit `bahn_raeum_gs_kt = None` kam trotzdem `None`
            // heraus, und die Pruefung war gruen. Thomas' Gegenpruefung hat
            // genau das gefunden.
            stats.bahn_raeum_laengs_m = Some(1500.0);
            stats.bahn_raeum_gs_kt = Some(45.0);

            let f = bahn_felder(&stats, Some("A320"), None);
            assert_eq!(
                f.clearance_point_m,
                Some(1600.0),
                "das Ausbrechen bei 800 m wird als Raeumpunkt gemeldet"
            );
            assert!(
                f.clearance_speed_kt.is_none(),
                "die Fahrt gehoert nicht an die nachgerechnete Stelle — \
                 gemeldet wurde {:?}",
                f.clearance_speed_kt
            );
        }

        // Ein Ausbrechen VOR dem Ausschwenken darf die Reihenfolge nicht
        // verdrehen.
        //
        // Die Nachrechnung suchte den Uebertritt ueber die ganze Spur. Ein
        // flacher Ausritt, der vor dem Kurswechsel beginnt und danach
        // draussen bleibt, wurde damit zum Raeumpunkt — und lag VOR dem
        // Bewertungsende. Der Live-Pfad hat den Riegel seit Runde 18; der
        // Test von damals lief durch den Live-Pfad und sah es nicht.
        {
            let mut stats = FlightStats::default();
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0, // halbe Breite 23,0 m
                surface: "ASP".to_string(),
                threshold_lat: 53.636011,
                threshold_lon: 9.999656,
                end_lat: 53.619958,
                end_lon: 9.967167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            // Ueber die Kante schon bei 700 m — und nie wieder zurueck.
            stats.bahn_spur = vec![
                (600.0, -10.0),
                (700.0, -25.0),
                (1200.0, -28.0),
                (1600.0, -40.0),
                (1700.0, -70.0),
            ];
            // Das Ausschwenken beginnt aber erst bei 1500 m.
            stats.bahn_raeum_laengs_m = Some(1500.0);
            stats.bahn_raeum_gs_kt = Some(40.0);

            let f = bahn_felder(&stats, Some("A320"), None);
            let cut = f.scoring_cutoff_m.expect("Bewertungsende");
            let clear = f.clearance_point_m.expect("Raeumpunkt");
            assert!(
                cut <= clear,
                "Bewertungsende {cut} liegt hinter dem Raeumpunkt {clear}"
            );
            // Und der GLEICHLAUF mit dem Pruefwerkzeug: Dort liefert
            // `kante_index(punkte, halbe, ab_laengs_m=1500)` fuer dieselbe
            // Spur `None`, weil es ab dem Ausschwenkpunkt keinen Uebertritt
            // von innen nach aussen mehr gibt — die Spur ist da laengst
            // draussen. Der Raeumpunkt faellt deshalb beidseits auf den
            // Ausschwenkpunkt zurueck.
            //
            // `tools/korpus/spuren_export.py --selbsttest` prueft dieselbe
            // Aussage von der anderen Seite.
            assert_eq!(
                clear, 1500.0,
                "Client und Pruefwerkzeug muessen hier denselben Wert liefern"
            );
        }

        // Der Raeumpunkt darf nicht VOR dem Bewertungsende liegen.
        //
        // Beides kommt aus derselben Landung, aber aus zwei Quellen:
        // `scoring_cutoff_m` vom Kurswechsel (Ausschwenken beginnt),
        // `clearance_point_m` vom Kantenuebertritt (Bahn verlassen). Das
        // Ausschwenken geht dem Verlassen immer voraus — eine andere
        // Reihenfolge gibt es in der Wirklichkeit nicht.
        //
        // Ohne Riegel setzte `spur_fortschreiben` die Kante auch bei noch
        // OFFENEM Messfenster: Ein Flugzeug, das kurz ueber die Kante
        // geraet und zurueckkommt, bekam einen „Bahn geraeumt"-Punkt
        // mitten im Ausrollen, hunderte Meter vor dem Abbiegen. Die
        // Anzeige haette die gestrichelte Linie rueckwaerts gezeichnet,
        // und die Marke behauptet eine Ausfahrt, die es nicht gab.
        {
            let halbe = 23.0;
            let mut stats = FlightStats::default();
            let mut snap = SimSnapshot {
                groundspeed_kt: 80.0,
                ..Default::default()
            };

            // Fenster OFFEN, Flugzeug kurz ueber der Kante.
            spur_fortschreiben(&mut stats, snap.groundspeed_kt, 900.0, 24.0, halbe);
            assert!(
                stats.bahn_kante_laengs_m.is_none(),
                "Kante bei offenem Messfenster gesetzt — das waere ein \
                 Raeumpunkt mitten auf der Bahn"
            );

            // Wieder auf der Bahn, dann Ausfahrt eingeleitet.
            spur_fortschreiben(&mut stats, snap.groundspeed_kt, 1000.0, 5.0, halbe);
            stats.bahn_fenster_zu = true;
            stats.bahn_raeum_laengs_m = Some(1600.0);
            snap.groundspeed_kt = 30.0;
            spur_fortschreiben(&mut stats, snap.groundspeed_kt, 1650.0, 26.0, halbe);

            let kante = stats.bahn_kante_laengs_m.expect("Kante nach der Ausfahrt");
            assert!(
                kante >= stats.bahn_raeum_laengs_m.unwrap(),
                "Raeumpunkt {kante} liegt vor dem Bewertungsende {:?}",
                stats.bahn_raeum_laengs_m
            );

            // Und die abgeleiteten Felder halten die Reihenfolge ein.
            let f = bahn_felder(&stats, Some("A320"), None);
            match (f.scoring_cutoff_m, f.clearance_point_m) {
                (Some(cut), Some(clear)) => assert!(
                    cut <= clear,
                    "Bewertungsende {cut} liegt hinter dem Raeumpunkt {clear}"
                ),
                _ => panic!("beide Punkte muessen hier vorliegen"),
            }
        }

        // Bewertung und Anzeige benutzen DIESELBE Spurweite.
        //
        // Die Anzeige nimmt seit Schritt 11 die Spurweite aus der
        // Flugzeugdatei, wenn eine gelesen wurde — sie beschreibt das
        // tatsaechlich geflogene Add-on. Die Bewertung nahm bis zur
        // QS-Runde 20 unverdrossen `spurweite_m(icao)`, also die Tabelle.
        //
        // Gemessen an einem Add-on mit 2 m breiterem Fahrwerk: Die Note
        // meldete 8,8 m Randabstand, die Anzeige 7,7 m. Dazu behauptete
        // die Herkunftsangabe daneben „aus der Flugzeugdatei". Genau die
        // Falle, wegen der `aussenkante_halb_m(icao)` entfernt wurde —
        // sie steckte an dieser Stelle weiter.
        {
            let mut stats = FlightStats::default();
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0,
                surface: "ASP".to_string(),
                threshold_lat: 53.636011,
                threshold_lon: 9.999656,
                end_lat: 53.619958,
                end_lon: 9.967167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            stats.bahn_max_querversatz_m = Some(10.0);
            stats.bahn_proben = 30;
            let aus_tabelle = landing_scoring::spurweite::spurweite_m(Some("A320")).expect("A320");
            let aus_datei = aus_tabelle + 2.0;
            stats.fahrwerk_spurweite_m = Some(aus_datei);

            let f = bahn_felder(&stats, Some("A320"), None);
            assert_eq!(f.track_width_m, Some(aus_datei), "Anzeige nimmt die Datei");
            assert_eq!(f.track_width_source.as_deref(), Some("aircraft_file"));

            // Und die BEWERTUNG muss denselben Wert benutzen. Der Weg geht
            // durch `LandingScoringInput`, so wie im Betrieb.
            let mut eingang = landing_scoring::LandingScoringInput::default();
            eingang.aircraft_icao = Some("A320".to_string());
            eingang.bahn_max_querversatz_m = stats.bahn_max_querversatz_m;
            eingang.bahn_proben = Some(30);
            eingang.runway_width_m = Some((151.0 * 0.3048) as f32);
            eingang.runway_surface = Some("ASP".to_string());
            eingang.airport_source = Some("runway_match".to_string());
            eingang.runway_geometry_trusted = Some(true);
            eingang.fahrwerk_spurweite_m = stats.fahrwerk_spurweite_m;

            let erwartet_rand = 151.0 * 0.3048 / 2.0
                - (10.0 + landing_scoring::spurweite::aussenkante_halb_aus_spur(aus_datei));
            assert!(
                (f.min_edge_clearance_m.expect("Randabstand") - erwartet_rand).abs() < 0.01,
                "die Anzeige rechnet mit {:?}, erwartet {erwartet_rand:.2}",
                f.min_edge_clearance_m
            );

            let mit_datei = landing_scoring::sub_bahndisziplin::sub_bahndisziplin(
                &landing_scoring::sub_bahndisziplin::BahndisziplinInput {
                    achsen_abweichung_grad: None,
                    achsen_kreuzt_mitte: None,
                    achsen_groesster_betrag_m: None,
                    max_querversatz_m: eingang.bahn_max_querversatz_m,
                    bahnbreite_m: eingang.runway_width_m.map(|w| w as f64),
                    spurweite_m: eingang
                        .fahrwerk_spurweite_m
                        .or_else(|| landing_scoring::spurweite::spurweite_m(Some("A320"))),
                    overrun_m: None,
                    belag: Some(landing_scoring::belag::Belag::Befestigt),
                    airport_source: Some("runway_match"),
                    runway_geometry_trusted: Some(true),
                    bahn_geometrie_aus_szenerie: None,
                    proben: Some(30),
                },
            );
            let wert = mit_datei.value.clone().unwrap_or_default();
            assert!(
                wert.contains(&format!("{erwartet_rand:.1}")),
                "die Note meldet {wert:?}, erwartet war ein Randabstand von {erwartet_rand:.1} m aus der Datei-Spurweite {aus_datei:.2} m"
            );
        }

        // Alle Kombinationen der optionalen Eingaenge — nicht nur die,
        // an die ich beim Bauen gedacht habe.
        //
        // # Warum das eine eigene Pruefung ist
        //
        // Thomas' Gegenpruefung von `75abdc6` fand einen Fehler, den mein
        // Test nicht sah: Er setzte `bahn_raeum_gs_kt` nicht, und genau
        // ueber dieses Feld lief der stille Rueckfall. Der Test war nicht
        // falsch — er war **blind**, weil sein Ausgangszustand unvollstaendig
        // war.
        //
        // Ein Test mit halb gesetztem Zustand prueft nur die Haelfte, die
        // er kennt. Deshalb hier alle acht Kombinationen, und fuer jede die
        // Regeln, die immer gelten muessen.
        {
            let rm = || runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0, // halbe Breite 23,0 m
                surface: "ASP".to_string(),
                threshold_lat: 53.636011,
                threshold_lon: 9.999656,
                end_lat: 53.619958,
                end_lon: 9.967167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            };
            // Eine Spur, die bei 1600 m nach aussen geht und draussen bleibt.
            let spur_mit_uebertritt = vec![
                (600.0f32, -2.0f32),
                (1200.0, -8.0),
                (1600.0, -25.0),
                (1700.0, -60.0),
            ];
            // Und eine, die auf der Bahn bleibt.
            let spur_ohne = vec![(600.0f32, -2.0f32), (1200.0, -8.0), (1600.0, -12.0)];

            for hat_spur_uebertritt in [false, true] {
                for hat_live_kante in [false, true] {
                    for hat_raeum in [false, true] {
                        let mut stats = FlightStats::default();
                        stats.runway_match = Some(rm());
                        stats.bahn_spur = if hat_spur_uebertritt {
                            spur_mit_uebertritt.clone()
                        } else {
                            spur_ohne.clone()
                        };
                        if hat_live_kante {
                            stats.bahn_kante_laengs_m = Some(1610.0);
                            stats.bahn_kante_gs_kt = Some(30.0);
                        }
                        if hat_raeum {
                            stats.bahn_raeum_laengs_m = Some(1500.0);
                            stats.bahn_raeum_gs_kt = Some(45.0);
                        }
                        let f = bahn_felder(&stats, Some("A320"), None);
                        let lage = format!(
                            "Spur-Uebertritt={hat_spur_uebertritt} \
                             Live-Kante={hat_live_kante} Raeumpunkt={hat_raeum}"
                        );

                        // 1. Reihenfolge: Das Ausschwenken geht dem
                        //    Verlassen voraus.
                        if let (Some(cut), Some(clear)) = (f.scoring_cutoff_m, f.clearance_point_m)
                        {
                            assert!(
                                cut <= clear + 0.001,
                                "{lage}: Bewertungsende {cut} hinter Raeumpunkt {clear}"
                            );
                        }

                        // 2. Keine Fahrt ohne Punkt.
                        if f.clearance_point_m.is_none() {
                            assert!(
                                f.clearance_speed_kt.is_none(),
                                "{lage}: Fahrt ohne Raeumpunkt"
                            );
                        }

                        // 3. Die Fahrt gehoert zu IHREM Punkt. Sie darf nur
                        //    aus einer Messung stammen, deren Stelle mit dem
                        //    gemeldeten Punkt zusammenfaellt.
                        if let (Some(gs), Some(punkt)) = (f.clearance_speed_kt, f.clearance_point_m)
                        {
                            let passt_zur_kante = stats
                                .bahn_kante_laengs_m
                                .is_some_and(|k| (k - punkt).abs() < 25.0)
                                && stats.bahn_kante_gs_kt == Some(gs);
                            let passt_zum_raeumen = stats
                                .bahn_raeum_laengs_m
                                .is_some_and(|r| (r - punkt).abs() < 0.001)
                                && stats.bahn_raeum_gs_kt == Some(gs);
                            assert!(
                                passt_zur_kante || passt_zum_raeumen,
                                "{lage}: Fahrt {gs} kt gehoert zu keinem \
                                 gemeldeten Punkt {punkt} m"
                            );
                        }
                    }
                }
            }
        }

        // Eine Bodenkarte vom FALSCHEN Flughafen darf nichts liefern.
        //
        // Der Fall ist real: Bei einem Divert liegt die Karte des
        // geplanten Ziels im Zwischenspeicher, waehrend das Flugzeug
        // woanders aufsetzt. Ausfahrten aus Hamburg an einer Frankfurter
        // Bahn waeren nicht als Fehler erkennbar — sie saehen aus wie
        // Rollwege, die es dort gibt.
        //
        // Der Schutz steckt in den Filtern von `ausfahrten_fuer_bahn`
        // (Laengsbereich und Kantenabstand); diese Pruefung haelt fest,
        // dass er wirkt, statt sich darauf zu verlassen.
        {
            let mut stats = FlightStats::default();
            // Bahn in EDDF, Karte aus EDDH — 400 km entfernt. Als „fuer
            // EDDF zustaendig" markiert (Runde-5-Feld), damit dieser Test
            // weiterhin die INHALTLICHE Pruefung in `ausfahrten_fuer_bahn`
            // trifft, statt schon an der (davon unabhaengigen) ICAO-
            // Zugehoerigkeitspruefung abgewiesen zu werden.
            stats.runway_correlation_icao = Some("EDDF".to_string());
            stats.arr_ground_geojson_icao = Some("EDDF".to_string());
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDF".to_string(),
                runway_ident: "07C".to_string(),
                heading_true_deg: 69.0,
                length_ft: 13123.0,
                width_ft: 197.0,
                surface: "ASP".to_string(),
                threshold_lat: 50.034_9,
                threshold_lon: 8.523_2,
                end_lat: 50.040_5,
                end_lon: 8.579_1,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 400.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            stats.arr_ground_geojson = Some(
                r#"{"features":[
                  {"properties":{"k":"taxiway","r":"S4"},
                   "geometry":{"type":"LineString",
                     "coordinates":[[9.982400,53.627218],[9.983079,53.626734]]}}
                ]}"#
                .into(),
            );
            assert!(
                bahn_felder(&stats, Some("A320"), None)
                    .runway_exits
                    .is_empty(),
                "Rollwege aus Hamburg landen an einer Frankfurter Bahn"
            );
        }

        // Ein Durchstarten darf nichts vom ersten Aufsetzen mitnehmen.
        //
        // `clear_approach_stability_and_rollout` setzte die Kennzahlen
        // zurueck und liess die SPUR stehen. Das zweite Aufsetzen bekam die
        // Punkte des ersten mit, dazu dessen Raeumpunkt. In der Anzeige
        // haette das nicht nach einem Fehler ausgesehen, sondern nach einer
        // Landung, bei der das Flugzeug zweimal ueber die Bahn lief.
        {
            let mut stats = FlightStats::default();
            stats.bahn_spur = vec![(500.0, -2.0), (900.0, -4.0), (1400.0, -30.0)];
            stats.bahn_spur_laeuft = true;
            stats.bahn_raeum_laengs_m = Some(1400.0);
            stats.bahn_raeum_gs_kt = Some(28.0);
            stats.bahn_raeum_seite = Some("left".to_string());
            stats.bahn_kante_laengs_m = Some(1460.0);
            stats.bahn_kante_gs_kt = Some(22.0);
            stats.bahn_max_querversatz_m = Some(-30.0);
            stats.bahn_overrun_m = Some(12.0);
            stats.bahn_proben = 91;
            stats.bahn_fenster_zu = true;
            stats.bahn_fenster_zu_laengs_m = Some(1180.0);

            clear_approach_stability_and_rollout(&mut stats);

            assert!(
                stats.bahn_spur.is_empty(),
                "die Spur des ersten Aufsetzens bleibt stehen"
            );
            assert!(!stats.bahn_spur_laeuft);
            assert!(
                stats.bahn_raeum_laengs_m.is_none(),
                "alter Raeumpunkt bleibt"
            );
            assert!(stats.bahn_raeum_gs_kt.is_none());
            assert!(stats.bahn_raeum_seite.is_none());
            assert!(
                stats.bahn_kante_laengs_m.is_none(),
                "alter Kantenuebertritt bleibt"
            );
            assert!(stats.bahn_kante_gs_kt.is_none());
            assert!(stats.bahn_max_querversatz_m.is_none());
            assert!(stats.bahn_overrun_m.is_none());
            assert_eq!(stats.bahn_proben, 0);
            assert!(!stats.bahn_fenster_zu);
            // ⚠ Diese Pruefung zaehlt jedes Feld einzeln auf — sie ist
            // deshalb nur so vollstaendig wie ihre Liste. Genau hier fiel
            // `bahn_fenster_zu_laengs_m` am 26.08.2026 durch: Das Feld war
            // neu, der Ruecksetzer kannte es nicht, und dieser Test auch
            // nicht. Die zweite Landung haette die Messgrenze der ersten
            // bekommen, und `get_or_insert` haette sie nie ueberschrieben.
            //
            // Wer ein `bahn_*`-Feld ergaenzt, ergaenzt BEIDE Listen.
            assert!(
                stats.bahn_fenster_zu_laengs_m.is_none(),
                "das Fensterende des ersten Aufsetzens bleibt stehen",
            );
        }

        // Die Ausfahrten muessen den ganzen Weg gehen.
        //
        // `ausfahrten::ausfahrten_fuer_bahn` war gebaut, mit sieben Tests
        // abgedeckt — und wurde **nirgends aufgerufen**. Die Anzeige hatte
        // den Platz dafuer, beide Mapper lasen das Feld, und der Client
        // fuellte es nie. Gefunden erst beim Abgleich der ganzen Feldkette;
        // kein Test war rot, kein Bau schlug fehl.
        //
        // Diese Pruefung geht durch `bahn_felder` — die Verdrahtung, nicht
        // die Funktion. Ein Test, der `ausfahrten_fuer_bahn` direkt aufruft,
        // waere waehrend des ganzen Fehlers gruen geblieben.
        {
            let mut stats = FlightStats::default();
            // Runde-5-Felder: die Karte gehoert zu EDDH, und EDDH ist der
            // tatsaechliche Landeplatz — ohne beides wuerde die neue
            // ICAO-Zugehoerigkeitspruefung die Karte gar nicht erst
            // durchlassen (siehe `bahn_felder`).
            stats.runway_correlation_icao = Some("EDDH".to_string());
            stats.arr_ground_geojson_icao = Some("EDDH".to_string());
            // EDDH 23 — echte Schwelle und echtes Bahnende, damit die
            // Projektion der Testkarte etwas Sinnvolles ergibt.
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                // Die ECHTE Geometrie aus den Navdaten (AIRAC 2608).
                // Der erste Anlauf hatte geratene Koordinaten — sie
                // ergaben einen Bahnkurs von 131 statt 230 Grad, also gar
                // nicht EDDH 23. Ein Test auf einer erfundenen Bahn prueft
                // die Rechnung, nicht die Wirklichkeit.
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0,
                surface: "ASP".to_string(),
                threshold_lat: 53.636011,
                threshold_lon: 9.999656,
                end_lat: 53.619958,
                end_lon: 9.967167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            // Rollweg D4 aus der ECHTEN Bodenkarte von EDDH.
            //
            // Vorher stand hier eine erfundene Zwei-Punkte-Linie. Sie
            // reichte, solange nur „kommen Ausfahrten an?" geprueft
            // wurde — fuer den VERLAUF taugt sie nicht: Sie lag ganz
            // ausserhalb des Streifens, den die Queransicht zeigt, und
            // wurde damit zu Recht verworfen. Ein Test auf erfundener
            // Geometrie prueft die Rechnung, nicht die Wirklichkeit.
            //
            // D4 trifft die Bahn bei rund 1.950 m und laeuft von der
            // Mittellinie nach aussen — genau der Fall, den die Anzeige
            // zeichnet.
            //
            // ⚠ OSM teilt Rollwege in Stuecke, die ALLE denselben Namen
            // tragen. „D4" gibt es in Hamburg dreimal; zwei Segmente
            // liegen ganz ausserhalb des gezeichneten Streifens. Wer das
            // erste nimmt, das er findet, bekommt eine Linie 64 bis 201
            // Meter neben der Mittellinie — mir genau so passiert.
            stats.arr_ground_geojson = Some(
                r#"{"features":[
                  {"properties":{"k":"taxiway","r":"D4"},
                   "geometry":{"type":"LineString",
                     "coordinates":[[9.976905,53.624765],[9.976979,53.624791],
                       [9.977071,53.624815],[9.977195,53.624835],
                       [9.977291,53.624842],[9.977414,53.62484],
                       [9.97753,53.624826],[9.977643,53.624802],
                       [9.977733,53.624771],[9.97781,53.62474],
                       [9.977883,53.624694],[9.978041,53.62458]]}}
                ]}"#
                .into(),
            );
            let felder = bahn_felder(&stats, Some("A320"), None);
            assert!(
                !felder.runway_exits.is_empty(),
                "die Bodenkarte liegt vor und die Bahn ist gematcht, \
                 trotzdem kommen keine Ausfahrten an — der Aufruf in \
                 `bahn_felder` fehlt"
            );

            // Und der VERLAUF muss mitkommen — fuer die GENOMMENE
            // Ausfahrt.
            //
            // Er wird bewusst nur dort behalten: Die Queransicht
            // zeichnet den Korridor nur fuer sie, alle anderen
            // Verlaeufe reisten ungezeichnet mit (in Muenchen rund 36
            // Kilobyte je Landung). Ohne Raeumpunkt ist deshalb KEIN
            // Verlauf zu erwarten — das ist kein Fehler, sondern der
            // Zuschnitt.
            assert!(
                felder.runway_exits.iter().all(|a| a.verlauf.is_empty()),
                "ohne Raeumpunkt reisen Verlaeufe mit, die nie \
                 gezeichnet werden"
            );

            // Jetzt mit Raeumpunkt auf der Ausfahrt selbst.
            let ziel = felder.runway_exits[0].clone();
            stats.bahn_raeum_laengs_m = Some(ziel.laengs_m);
            stats.bahn_raeum_seite = Some(ziel.seite.clone());
            let felder = bahn_felder(&stats, Some("A320"), None);

            //
            // Genau derselbe Fehler, eine Ebene tiefer: `ausfahrten.rs`
            // rechnete ihn aus, zwoelf Tests deckten ihn ab — und beim
            // Umkopieren nach `storage::RunwayExit` stand er in keiner
            // Feldliste. Die Queransicht zeichnete den Korridor deshalb
            // NUR in der Vorschau, wo er von Hand gesetzt war, und bei
            // keinem einzigen echten Flug.
            //
            // Auch das faellt nur auf, wenn man durch `bahn_felder` geht:
            // `ausfahrten_fuer_bahn` allein war die ganze Zeit gruen.
            assert!(
                felder.runway_exits.iter().any(|a| !a.verlauf.is_empty()),
                "die Ausfahrten kommen an, aber ohne Verlauf — der \
                 Korridor der Queransicht bleibt leer"
            );
            // Und er muss in Bahnkoordinaten stehen, nicht in Graden.
            let v = &felder
                .runway_exits
                .iter()
                .find(|a| !a.verlauf.is_empty())
                .expect("Ausfahrt mit Verlauf")
                .verlauf;
            assert!(
                v.iter()
                    .all(|p| p.laengs_m.abs() < 20_000.0 && p.quer_m.abs() < 2_000.0),
                "der Verlauf traegt keine Bahnkoordinaten: {:?}",
                &v[..v.len().min(3)],
            );

            // Und ohne Karte bleibt es leer, statt etwas zu behaupten.
            stats.arr_ground_geojson = None;
            assert!(
                bahn_felder(&stats, Some("A320"), None)
                    .runway_exits
                    .is_empty(),
                "ohne Bodenkarte duerfen keine Ausfahrten entstehen"
            );
        }

        // Und der Takt darf in der Ausfahrt nicht einbrechen.
        //
        // Gemessen an neun echten Landungen fiel die Kadenz bei dreissig
        // Knoten von 0,51 s auf 3,01 s — 34,9 Meter zwischen zwei Punkten,
        // mitten in der Kurve. Ursache war nicht die Fahrt, sondern die
        // Phase: Beim Abbiegen springt sie von `Landing` auf `TaxiIn`,
        // und damit fiel jede Sonderregel weg.
        for gs in [55.0, 30.0, 15.0, 8.0] {
            let t = adaptive_tick_interval_v2(
                FlightPhase::TaxiIn,
                Some(0.0),
                Some(true),
                Some(gs),
                true, // die Aufzeichnung laeuft noch
            );
            let rollout_tick_ms = crate::grenzwerte::grenzwerte().ausrollen.rollout_tick_ms.wert;
            assert_eq!(
                t,
                Duration::from_millis(crate::grenzwerte::grenzwerte().ausrollen.rollout_tick_ms.wert),
                "bei {gs} kt in der Ausfahrt taktet die Aufzeichnung mit \
                 {t:?} statt {rollout_tick_ms} ms — die Spur wird grob"
            );
        }
        // Rollt das Flugzeug zum Gate, ist die Spur zu Ende — und mit ihr
        // der feine Takt. Sonst liefe er die ganze Rollzeit weiter.
        assert!(
            adaptive_tick_interval_v2(
                FlightPhase::TaxiIn,
                Some(0.0),
                Some(true),
                Some(20.0),
                false,
            ) > Duration::from_millis(crate::grenzwerte::grenzwerte().ausrollen.rollout_tick_ms.wert),
            "auf dem Rollweg laeuft der 5-Hz-Takt weiter"
        );

        // Gegenprobe: Mit reiner Laengsmessung waeren es weniger -- genau
        // die beiden letzten faehlen dann.
        let mut nur_laengs: Vec<f64> = Vec::new();
        for (lg, _) in echte_punkte {
            if nur_laengs
                .last()
                .is_none_or(|l| (lg - l).abs() >= crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_min_abstand_m.wert)
            {
                nur_laengs.push(*lg);
            }
        }
        assert!(
            nur_laengs.len() < echte_punkte.len(),
            "die Gegenprobe muss Punkte verlieren, sonst prueft der Test nichts"
        );
    }

    /// v1.7.21: `bahn_mess_schwelle_kt` — die relative Fensterschwelle
    /// selbst, ohne den Umweg ueber einen ganzen Tick.
    ///
    /// Befund (Live-Korpus, 1096 Landungen): 5 Landungen scheiterten an
    /// `insufficient_samples`, ausnahmslos leichte einmotorige GA-Muster
    /// (PA24, C152, AC11, BE24) mit 44-56 kt Aufsetz-Grundgeschwindigkeit —
    /// dort lag die GESAMTE Ausrollstrecke unter der alten festen 60-kt-
    /// Schwelle.
    #[test]
    fn bahn_mess_schwelle_ist_relativ_zur_aufsetz_gs() {
        let mut stats = FlightStats::default();
        // Kein Aufsetz-GS bekannt (sollte im Landing-Pfad nicht vorkommen)
        // — Rueckfall auf den alten festen Wert, NICHT auf 0 oder Fehler.
        assert_eq!(bahn_mess_schwelle_kt(&stats), crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt.wert);

        // Deckel: ab 100 kt Aufsetz-GS bleibt es beim alten Wert (0.6*100
        // = 60) — unveraendertes Verhalten fuer Jets/Airliner.
        stats.landing_groundspeed_kt = Some(140.0);
        assert_eq!(bahn_mess_schwelle_kt(&stats), 60.0);
        stats.landing_groundspeed_kt = Some(100.0);
        assert_eq!(bahn_mess_schwelle_kt(&stats), 60.0);

        // Zwischenbereich: 0.6 * Aufsetz-GS — genau das Beispiel aus der
        // Spezifikation (55 kt Aufsetz-GS -> 33 kt Schwelle).
        stats.landing_groundspeed_kt = Some(55.0);
        assert!(
            (bahn_mess_schwelle_kt(&stats) - 33.0).abs() < 0.001,
            "erwartet 33 kt (0.6 * 55), war {}",
            bahn_mess_schwelle_kt(&stats)
        );

        // Boden: 0.6 * 30 = 18, das waere Schrittgeschwindigkeit — der
        // harte Boden haelt bei 25 kt.
        stats.landing_groundspeed_kt = Some(30.0);
        assert_eq!(bahn_mess_schwelle_kt(&stats), 25.0);

        // Ein nicht-endlicher Wert (sollte praktisch nie vorkommen) faellt
        // ebenfalls auf den festen Wert zurueck, NICHT auf 0.
        stats.landing_groundspeed_kt = Some(f32::NAN);
        assert_eq!(bahn_mess_schwelle_kt(&stats), crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt.wert);

        // +/- unendlich: `is_finite()` verwirft beides, Ruecckfall auf den
        // festen Wert (QS-Fund, Codex 08.09.2026: nicht nur NaN testen).
        stats.landing_groundspeed_kt = Some(f32::INFINITY);
        assert_eq!(bahn_mess_schwelle_kt(&stats), crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt.wert);
        stats.landing_groundspeed_kt = Some(f32::NEG_INFINITY);
        assert_eq!(bahn_mess_schwelle_kt(&stats), crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt.wert);

        // Eine negative, aber ENDLICHE GS (Sensor-Glitch, physikalisch
        // unmoeglich) darf NICHT wie eine echte langsame Landung auf den
        // 25-kt-Boden geklemmt werden, sondern faellt auf den festen Wert
        // zurueck — genau wie NaN (QS-Fund, Codex 08.09.2026).
        stats.landing_groundspeed_kt = Some(-5.0);
        assert_eq!(bahn_mess_schwelle_kt(&stats), crate::grenzwerte::grenzwerte().ausrollen.bahn_mess_min_gs_kt.wert);
    }

    /// v1.7.21: Ein leichtes GA-Muster mit niedriger Aufsetzfahrt bekommt
    /// jetzt Bahndisziplin-Proben, wo die alte feste 60-kt-Schwelle das
    /// Messfenster nie geoeffnet haette (55 kt Aufsetz-GS < 60 kt fest —
    /// das Fenster waere ab dem ERSTEN Tick zu gewesen, `bahn_proben`
    /// waere bei 0 geblieben).
    #[test]
    fn ga_landung_mit_niedriger_aufsetz_gs_bekommt_proben() {
        const GRAD_M: f64 = 111_320.0;
        let mut stats = FlightStats::default();
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDM".to_string(),
            runway_ident: "26L".to_string(),
            heading_true_deg: 90.0,
            length_ft: (4000.0 * 3.280_839_895) as f32,
            width_ft: 197.0,
            surface: "ASP".to_string(),
            threshold_lat: 0.0,
            threshold_lon: 0.0,
            end_lat: 0.0,
            end_lon: 4000.0 / GRAD_M,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 500.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.landing_heading_true_deg = Some(90.0);
        // Aufsetz-Grundgeschwindigkeit einer leichten GA-Maschine — genau
        // die Gruppe aus dem Korpus-Befund (PA24 u.ae., 44-56 kt).
        stats.landing_groundspeed_kt = Some(55.0);

        let tick = |stats: &mut FlightStats, laengs: f64, gs: f32| {
            let mut snap = SimSnapshot::default();
            snap.lat = 0.0;
            snap.lon = laengs / GRAD_M;
            snap.groundspeed_kt = gs;
            // Geradeaus, kein Kurswechsel — sonst schliesst der
            // unabhaengige Ausfahrt-Detektor das Fenster VOR der
            // Geschwindigkeitspruefung.
            snap.heading_deg_true = 90.0;
            bahndisziplin_tick(stats, &snap);
        };

        // 5 Hz-Takt (ROLLOUT_TICK_MS), langsam von 55 auf 25 kt fallend,
        // ueber eine kurze, plausible Rollstrecke.
        for (laengs, gs) in [
            (1000.0, 55.0),
            (1020.0, 50.0),
            (1040.0, 45.0),
            (1060.0, 40.0),
            (1080.0, 35.0),
            (1100.0, 30.0),
            (1120.0, 25.0),
        ] {
            tick(&mut stats, laengs, gs);
        }

        // Effektive Schwelle bei 55 kt Aufsetz-GS: 0.6*55 = 33 kt. Die
        // Ticks bei 55/50/45/40/35 kt liegen darueber und werden gezaehlt;
        // die Ticks bei 30/25 kt liegen darunter und schliessen das
        // Fenster. Vorher (feste 60-kt-Schwelle) waere das Fenster schon
        // beim allerersten Tick (55 < 60) zu gewesen -- 0 Proben.
        assert!(
            stats.bahn_proben >= 3,
            "erwartet mindestens 3 Proben bei einer relativen Schwelle \
             von 33 kt, gezaehlt wurden {}",
            stats.bahn_proben
        );
    }

    /// v1.7.21: Ein Airliner/Jet mit normaler Aufsetzfahrt (>=100 kt) darf
    /// sich NICHT aendern — die relative Schwelle ist dort vom 60-kt-Deckel
    /// begrenzt, exakt wie die alte feste Schwelle.
    #[test]
    fn jet_landung_mit_hoher_aufsetz_gs_bleibt_unveraendert() {
        const GRAD_M: f64 = 111_320.0;
        let mut stats = FlightStats::default();
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDM".to_string(),
            runway_ident: "26L".to_string(),
            heading_true_deg: 90.0,
            length_ft: (4000.0 * 3.280_839_895) as f32,
            width_ft: 197.0,
            surface: "ASP".to_string(),
            threshold_lat: 0.0,
            threshold_lon: 0.0,
            end_lat: 0.0,
            end_lon: 4000.0 / GRAD_M,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 500.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.landing_heading_true_deg = Some(90.0);
        // Aufsetz-Grundgeschwindigkeit eines Airliners — weit ueber dem
        // 60-kt-Deckel (0.6*140 = 84, gedeckelt auf 60).
        stats.landing_groundspeed_kt = Some(140.0);

        let tick = |stats: &mut FlightStats, laengs: f64, gs: f32| {
            let mut snap = SimSnapshot::default();
            snap.lat = 0.0;
            snap.lon = laengs / GRAD_M;
            snap.groundspeed_kt = gs;
            snap.heading_deg_true = 90.0;
            bahndisziplin_tick(stats, &snap);
        };

        for (laengs, gs) in [
            (1000.0, 140.0),
            (1200.0, 100.0),
            (1400.0, 80.0),
            (1600.0, 65.0),
            (1700.0, 58.0), // unter dem 60-kt-Deckel: Fenster schliesst hier
            (1750.0, 55.0),
        ] {
            tick(&mut stats, laengs, gs);
        }

        // Genau wie unter der alten festen 60-kt-Schwelle: gezaehlt werden
        // die vier Ticks bei/ueber 60 kt (140/100/80/65), das Fenster
        // schliesst beim ersten Tick darunter (58 kt).
        assert_eq!(
            stats.bahn_proben, 4,
            "die Anzahl der Proben darf sich fuer einen Aufsetz-GS >= 100 kt \
             nicht aendern (60-kt-Deckel)",
        );
        let fenster = stats
            .bahn_fenster_zu_laengs_m
            .expect("Fensterende muss beim Unterschreiten der 60 kt stehen");
        assert!(
            (fenster - 1700.0).abs() < 5.0,
            "Fenster schloss bei {fenster:.0} m, erwartet rund 1.700 m \
             (derselbe Punkt wie unter der alten festen 60-kt-Schwelle)",
        );
    }

    /// Runde 5 (externe Gegenpruefung, Codex, adversarial, 04.09.2026):
    /// `arr_ground_geojson` traegt seit Runde 5 eine eigene ICAO-Markierung
    /// (`arr_ground_geojson_icao`). Stimmt sie NICHT mit dem tatsaechlichen
    /// Landeplatz (`runway_correlation_icao`) ueberein — z. B. weil die
    /// Karte urspruenglich fuer einen ANDEREN, inzwischen ueberholten
    /// Zielwechsel abgelegt wurde — darf `bahn_felder` sie NICHT
    /// verwenden, selbst wenn ihr Inhalt (wie hier: die echte EDDH-D4-
    /// Geometrie) fuer sich genommen gueltige Ausfahrten ergeben wuerde.
    #[test]
    fn bodenkarte_mit_veralteter_icao_markierung_wird_verworfen() {
        let mut stats = FlightStats::default();
        // Landeplatz laut Touchdown-Korrelation: EDDH.
        stats.runway_correlation_icao = Some("EDDH".to_string());
        // Karte traegt aber noch die Markierung eines FRUEHEREN Ziels.
        stats.arr_ground_geojson_icao = Some("EDDY".to_string());
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDH".to_string(),
            runway_ident: "23".to_string(),
            heading_true_deg: 230.21,
            length_ft: 10663.0,
            width_ft: 151.0,
            surface: "ASP".to_string(),
            threshold_lat: 53.636011,
            threshold_lon: 9.999656,
            end_lat: 53.619958,
            end_lon: 9.967167,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 720.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        // Dieselbe (an sich gueltige) EDDH-D4-Geometrie wie im Test
        // darueber.
        stats.arr_ground_geojson = Some(
            r#"{"features":[
              {"properties":{"k":"taxiway","r":"D4"},
               "geometry":{"type":"LineString",
                 "coordinates":[[9.976905,53.624765],[9.976979,53.624791],
                   [9.977071,53.624815],[9.977195,53.624835],
                   [9.977291,53.624842],[9.977414,53.62484],
                   [9.97753,53.624826],[9.977643,53.624802],
                   [9.977733,53.624771],[9.97781,53.62474],
                   [9.977883,53.624694],[9.978041,53.62458]]}}
            ]}"#
            .into(),
        );
        let felder = bahn_felder(&stats, Some("A320"), None);
        assert!(
            felder.runway_exits.is_empty(),
            "eine Bodenkarte mit veralteter ICAO-Markierung darf keine Ausfahrten liefern"
        );
    }

    /// Runde 6 (externe Gegenpruefung, Codex, adversarial, 04.09.2026):
    /// `szenerie_karte` hatte VOR dieser Runde gar keine ICAO-Pruefung —
    /// eine veraltete `szenerie_auskunft` (z. B. ein frueherer Divert-
    /// Kandidat, dessen Ernte noch nicht durch die des tatsaechlichen
    /// Landeplatzes ersetzt wurde) haette Vorrang vor einer bereits
    /// korrekt geprueften Serverkarte gehabt UND deren Ausfahrten
    /// unterdrueckt, obwohl sie selbst nichts Brauchbares fuer DIESEN
    /// Landeplatz beitraegt. Die Serverkarte (dieselbe gueltige
    /// EDDH-D4-Geometrie wie oben) muss trotzdem durchkommen.
    #[test]
    fn veraltete_szenerie_unterdrueckt_die_gueltige_serverkarte_nicht() {
        let mut stats = FlightStats::default();
        stats.runway_correlation_icao = Some("EDDH".to_string());
        // Szenerie-Auskunft eines ANDEREN, frueheren Ziels — nicht leer,
        // haette also ohne ICAO-Pruefung Vorrang gehabt.
        stats.szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
            icao: "EDDY".to_string(),
            bahnen: Vec::new(),
            rollwege: vec![sim_core::szenerie::SzenerieRollweg {
                name: "STALE".to_string(),
                punkte: vec![(52.13, 11.63), (52.14, 11.64)],
            }],
            staende: Vec::new(),
            quelle: "test".to_string(),
        });
        // Korrekt zugeordnete Serverkarte fuer den echten Landeplatz.
        stats.arr_ground_geojson_icao = Some("EDDH".to_string());
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDH".to_string(),
            runway_ident: "23".to_string(),
            heading_true_deg: 230.21,
            length_ft: 10663.0,
            width_ft: 151.0,
            surface: "ASP".to_string(),
            threshold_lat: 53.636011,
            threshold_lon: 9.999656,
            end_lat: 53.619958,
            end_lon: 9.967167,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 720.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.arr_ground_geojson = Some(
            r#"{"features":[
              {"properties":{"k":"taxiway","r":"D4"},
               "geometry":{"type":"LineString",
                 "coordinates":[[9.976905,53.624765],[9.976979,53.624791],
                   [9.977071,53.624815],[9.977195,53.624835],
                   [9.977291,53.624842],[9.977414,53.62484],
                   [9.97753,53.624826],[9.977643,53.624802],
                   [9.977733,53.624771],[9.97781,53.62474],
                   [9.977883,53.624694],[9.978041,53.62458]]}}
            ]}"#
            .into(),
        );
        let felder = bahn_felder(&stats, Some("A320"), None);
        assert!(
            !felder.runway_exits.is_empty(),
            "die veraltete Szenerie-Auskunft eines anderen Flughafens darf \
             die gueltige Serverkarte nicht verdraengen"
        );
    }

    /// Codex 22.10.2026 (Befund 1): Liegt die Bodenkarte vor, findet sich
    /// aber keine genutzte Ausfahrt (etwa nach einer Bahnkorrektur), muss
    /// `clearance_exit` als `null` auf die Leitung — nur dann loescht der
    /// Recorder (RFC 7396) den Namen der vorigen Revision. Fehlt die Karte,
    /// bleibt das Feld weg (Eingabe fehlt, nichts loeschen).
    #[test]
    fn clearance_exit_wird_bei_karte_ohne_treffer_geloescht() {
        let mut stats = FlightStats::default();
        stats.runway_correlation_icao = Some("EDDH".to_string());
        stats.arr_ground_geojson_icao = Some("EDDH".to_string());
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDH".to_string(),
            runway_ident: "23".to_string(),
            heading_true_deg: 230.21,
            length_ft: 10663.0,
            width_ft: 151.0,
            surface: "ASP".to_string(),
            threshold_lat: 53.636011,
            threshold_lon: 9.999656,
            end_lat: 53.619958,
            end_lon: 9.967167,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 720.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.arr_ground_geojson = Some(
            r#"{"features":[
              {"properties":{"k":"taxiway","r":"D4"},
               "geometry":{"type":"LineString",
                 "coordinates":[[9.976905,53.624765],[9.976979,53.624791],
                   [9.977071,53.624815],[9.977195,53.624835],
                   [9.977291,53.624842],[9.977414,53.62484],
                   [9.97753,53.624826],[9.977643,53.624802],
                   [9.977733,53.624771],[9.97781,53.62474],
                   [9.977883,53.624694],[9.978041,53.62458]]}}
            ]}"#
            .into(),
        );
        // Karte da, kein Raeumpunkt → keine genutzte Ausfahrt → `null`.
        let j = serde_json::to_value(bahn_felder(&stats, Some("A320"), None).wire(true)).unwrap();
        assert!(
            j.get("clearance_exit").is_some_and(|v| v.is_null()),
            "clearance_exit muss als null kommen, nicht fehlen: {:?}",
            j.get("clearance_exit")
        );
        // Ohne Karte: das Feld fehlt ganz.
        stats.arr_ground_geojson = None;
        let ohne = serde_json::to_value(bahn_felder(&stats, Some("A320"), None).wire(true)).unwrap();
        assert!(ohne.get("clearance_exit").is_none(), "{:?}", ohne.get("clearance_exit"));
    }

    /// Runde 7 (externe Gegenpruefung, Codex, adversarial, 04.09.2026):
    /// `runway_correlation_icao` und `runway_match.airport_ident` sind
    /// NICHT garantiert derselbe Platz — `correlate_airport_icao` bevorzugt
    /// bei mehrdeutigen Nachbarflughaefen den GEPLANTEN Platz, waehrend die
    /// tatsaechlich gematchte Bahn (`lookup_runway_with_fallback`) von
    /// einem naheliegenden NACHBARN stammen kann. Eine gegen
    /// `runway_correlation_icao` allein geprueften Karte waere hier auf die
    /// Bahn des Nachbarn projiziert worden. Die Pruefung muss gegen
    /// `runway_match.airport_ident` laufen.
    #[test]
    fn karte_fuer_einen_abweichenden_korrelations_platz_wird_verworfen() {
        let mut stats = FlightStats::default();
        // Korrelation sagt EDDF (das GEPLANTE Ziel bei einer mehrdeutigen
        // Nachbarschaft) — die tatsaechlich gematchte Bahn gehoert aber zu
        // EDDH.
        stats.runway_correlation_icao = Some("EDDF".to_string());
        stats.arr_ground_geojson_icao = Some("EDDF".to_string());
        stats.arr_ground_geojson = Some(
            r#"{"features":[
              {"properties":{"k":"taxiway","r":"D4"},
               "geometry":{"type":"LineString",
                 "coordinates":[[9.976905,53.624765],[9.976979,53.624791],
                   [9.977071,53.624815],[9.977195,53.624835],
                   [9.977291,53.624842],[9.977414,53.62484],
                   [9.97753,53.624826],[9.977643,53.624802],
                   [9.977733,53.624771],[9.97781,53.62474],
                   [9.977883,53.624694],[9.978041,53.62458]]}}
            ]}"#
            .into(),
        );
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDH".to_string(),
            runway_ident: "23".to_string(),
            heading_true_deg: 230.21,
            length_ft: 10663.0,
            width_ft: 151.0,
            surface: "ASP".to_string(),
            threshold_lat: 53.636011,
            threshold_lon: 9.999656,
            end_lat: 53.619958,
            end_lon: 9.967167,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 720.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        let felder = bahn_felder(&stats, Some("A320"), None);
        assert!(
            felder.runway_exits.is_empty(),
            "eine fuer EDDF markierte Karte darf nicht auf die EDDH-Bahn projiziert werden, \
             nur weil runway_correlation_icao EDDF sagt"
        );
    }

    #[test]
    fn ausrollen_wird_fuenfmal_so_fein_abgetastet() {
        // Der Rollweg lief bisher mit denselben 2 Hz wie der Flare: bei
        // 100 kt ein Punkt alle 25 m. Eine Spur aus fuenfzehn Punkten kann
        // nicht zeigen, wie sich das Flugzeug auf der Bahn verhalten hat --
        // ein Ausbrechen und Zurueckziehen faellt zwischen zwei Messungen.
        let ausrollen = adaptive_tick_interval_v2(
            FlightPhase::Landing,
            Some(0.0),
            Some(true),
            Some(100.0),
            false,
        );
        assert_eq!(ausrollen, Duration::from_millis(200), "5 Hz beim Ausrollen");

        // Bei 100 kt sind 200 ms genau zehn Meter -- der Mindestabstand,
        // mit dem `bahndisziplin_tick` die Spur ohnehin ausduennt. Feiner
        // abzutasten brächte nichts, weil die Punkte dann verworfen werden.
        let meter_je_tick = 100.0 * 0.514_444 * 0.2;
        let bahn_spur_min_abstand_m = crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_min_abstand_m.wert;
        assert!(
            (meter_je_tick - crate::grenzwerte::grenzwerte().ausrollen.bahn_spur_min_abstand_m.wert).abs() < 1.0,
            "{meter_je_tick} m je Tick gegen {bahn_spur_min_abstand_m} m Mindestabstand"
        );
    }

    #[test]
    fn feine_abtastung_nur_beim_ausrollen() {
        // Sie darf NICHT im Flare greifen -- dort ist die Hoehe das
        // Kriterium, und der 50-Hz-Sampler liefert die Feinheit ohnehin.
        assert_eq!(
            adaptive_tick_interval_v2(
                FlightPhase::Landing,
                Some(20.0),
                Some(false),
                Some(140.0),
                false
            ),
            Duration::from_millis(500),
            "in der Luft bleibt es bei 2 Hz"
        );
        // Frueher stand hier: „Unter 60 kt zurueck auf 2 Hz — was nicht
        // mehr bewertet wird, braucht keine feine Spur."
        //
        // Das war falsch, und der Test hat den Fehler festgehalten statt
        // ihn zu finden. Bewertet wird bis 60 kt, GEZEICHNET bis 5 kt.
        // Die Ausfahrt liegt dazwischen — sie ist der Teil der Spur, der
        // sich am staerksten kruemmt, und bekam die wenigsten Punkte.
        //
        // Massgeblich ist jetzt, ob die Aufzeichnung laeuft.
        assert_eq!(
            adaptive_tick_interval_v2(
                FlightPhase::Landing,
                Some(0.0),
                Some(true),
                Some(30.0),
                true
            ),
            Duration::from_millis(200),
            "solange die Spur laeuft, bleibt es bei 5 Hz"
        );
        // Drei Sekunden, nicht 500 ms: `TaxiIn` steht in keiner der
        // Hoehenregeln, es gilt der Grundtakt. Genau dieser Wert wurde im
        // Korpus gemessen — 3,01 s Median unter dreissig Knoten, ueber
        // 311 Messpunkte. Das war der Einbruch, um den es hier geht.
        assert_eq!(
            adaptive_tick_interval_v2(
                FlightPhase::TaxiIn,
                Some(0.0),
                Some(true),
                Some(30.0),
                false
            ),
            Duration::from_secs(MQTT_PUBLISH_INTERVAL_SECS),
            "ist die Spur zu Ende, faellt der Takt auf den Grundtakt zurueck"
        );
        // Und nicht in anderen Phasen -- ein Startlauf ist kein Ausrollen.
        assert_eq!(
            adaptive_tick_interval_v2(
                FlightPhase::TakeoffRoll,
                Some(0.0),
                Some(true),
                Some(100.0),
                false,
            ),
            Duration::from_millis(500),
            "der Startlauf bleibt bei 2 Hz"
        );
    }

    #[test]
    fn sampler_path_rollout_accumulates_and_finalizes_at_full_stop() {
        // Bush flight: FSM stuck in Climb, sampler stamped the touchdown and
        // initialised the rollout. Three streamer ticks: rolling, slowing,
        // stopped.
        let mut stats = rollout_stats(50.0, 8.0);
        stats.phase = FlightPhase::Climb;
        stats.sampler_touchdown_at = Some(td());

        sampler_path_rollout_tick(&mut stats, &rollout_snap(50.0, 8.001, 60.0, 90.0, true));
        sampler_path_rollout_tick(&mut stats, &rollout_snap(50.0, 8.002, 45.0, 90.0, true));
        sampler_path_rollout_tick(&mut stats, &rollout_snap(50.0, 8.003, 4.0, 90.0, true));

        let expected = ::geo::distance_m(50.0, 8.0, 50.0, 8.001)
            + ::geo::distance_m(50.0, 8.001, 50.0, 8.002)
            + ::geo::distance_m(50.0, 8.002, 50.0, 8.003);
        let got = stats.rollout_distance_m.expect("distance accumulated");
        assert!(
            (got - expected).abs() < 0.01,
            "got {got}, expected {expected}"
        );
        assert!(stats.rollout_finalized, "GS < 5 kt must finalise");
        assert_eq!(stats.rollout_finalize_reason.as_deref(), Some("full_stop"));
    }

    #[test]
    fn sampler_path_rollout_skips_when_fsm_in_landing() {
        // No double-tick: when the FSM IS in Landing its arm has just driven
        // rollout_tick inside step_flight on this very tick.
        let mut stats = rollout_stats(50.0, 8.0);
        stats.phase = FlightPhase::Landing;
        stats.sampler_touchdown_at = Some(td());
        sampler_path_rollout_tick(&mut stats, &rollout_snap(50.0, 8.001, 60.0, 90.0, true));
        assert_eq!(
            stats.rollout_distance_m,
            Some(0.0),
            "Landing phase owns the tick"
        );
        assert_eq!(stats.rollout_last_lat, Some(50.0));
    }

    #[test]
    fn sampler_path_rollout_skips_without_sampler_touchdown() {
        let mut stats = rollout_stats(50.0, 8.0);
        stats.phase = FlightPhase::Climb;
        stats.sampler_touchdown_at = None;
        sampler_path_rollout_tick(&mut stats, &rollout_snap(50.0, 8.001, 60.0, 90.0, true));
        assert_eq!(stats.rollout_distance_m, Some(0.0));
        assert_eq!(stats.rollout_last_lat, Some(50.0));
    }

    #[test]
    fn sampler_path_rollout_skips_once_finalized() {
        let mut stats = rollout_stats(50.0, 8.0);
        stats.phase = FlightPhase::Climb;
        stats.sampler_touchdown_at = Some(td());
        stats.rollout_finalized = true;
        stats.rollout_distance_m = Some(450.0);
        sampler_path_rollout_tick(&mut stats, &rollout_snap(50.0, 8.001, 60.0, 90.0, true));
        assert_eq!(stats.rollout_distance_m, Some(450.0));
    }

    // ─── (6) T&G climb-out reset clears stability + rollout ──────────────

    #[test]
    fn climbout_reset_clears_stability_and_rollout_fields() {
        let mut stats = FlightStats::default();
        // populate everything the sampler-path evaluation writes
        stats.approach_vs_stddev_fpm = Some(120.0);
        stats.approach_bank_stddev_deg = Some(3.0);
        stats.approach_vs_deviation_fpm = Some(80.0);
        stats.approach_max_vs_deviation_below_500_fpm = Some(400.0);
        stats.approach_bank_stddev_filtered_deg = Some(2.5);
        stats.approach_runway_changed_late = true;
        stats.approach_stable_at_gate = Some(false);
        stats.approach_vs_jerk_fpm = Some(60.0);
        stats.approach_ias_stddev_kt = Some(4.0);
        stats.approach_excessive_sink = Some(true);
        stats.approach_stable_config = Some(true);
        stats.approach_used_hat = true;
        stats.approach_window_sample_count = Some(42);
        stats.approach_stable_at_da = Some(true);
        stats.approach_stall_warning_count = 2;
        stats.rollout_distance_m = Some(512.0);
        stats.rollout_finalized = true;
        stats.rollout_last_lat = Some(50.0);
        stats.rollout_last_lon = Some(8.0);
        stats.rollout_finalize_reason = Some("full_stop".into());

        clear_approach_stability_and_rollout(&mut stats);

        assert_eq!(stats.approach_vs_stddev_fpm, None);
        assert_eq!(stats.approach_bank_stddev_deg, None);
        assert_eq!(stats.approach_vs_deviation_fpm, None);
        assert_eq!(stats.approach_max_vs_deviation_below_500_fpm, None);
        assert_eq!(stats.approach_bank_stddev_filtered_deg, None);
        assert!(!stats.approach_runway_changed_late);
        assert_eq!(stats.approach_stable_at_gate, None);
        assert_eq!(stats.approach_vs_jerk_fpm, None);
        assert_eq!(stats.approach_ias_stddev_kt, None);
        assert_eq!(stats.approach_excessive_sink, None);
        assert_eq!(stats.approach_stable_config, None);
        assert!(!stats.approach_used_hat);
        assert_eq!(stats.approach_window_sample_count, None);
        assert_eq!(stats.approach_stable_at_da, None);
        assert_eq!(stats.approach_stall_warning_count, 0);
        assert_eq!(stats.rollout_distance_m, None, "next TD re-accumulates");
        assert!(!stats.rollout_finalized);
        assert_eq!(stats.rollout_last_lat, None);
        assert_eq!(stats.rollout_last_lon, None);
        assert_eq!(stats.rollout_finalize_reason, None);
    }
