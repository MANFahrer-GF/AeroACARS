    use super::*;
    use chrono::TimeZone;

    fn td() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 5, 12, 0, 0).unwrap()
    }

    /// Anflugprobe `t_ms` vor dem Aufsetzen auf `hoehe_ft` über der Bahn.
    /// Die Bahn liegt auf 300 ft; die Höhe über Grund trägt wie beim B772
    /// 16 ft Bezugspunkt-Versatz, den die Messung herausrechnen muss.
    fn probe(t_ms: i64, hoehe_ft: f32, vs_fpm: f32) -> ApproachBufferSample {
        ApproachBufferSample {
            at: td() + chrono::Duration::milliseconds(t_ms),
            agl_ft: hoehe_ft + 16.0,
            msl_ft: 300.0 + 16.0 + hoehe_ft,
            gs_kt: 133.0,
            ias_kt: 135.0,
            vs_fpm,
            bank_deg: 0.0,
            heading_true_deg: 0.0,
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

    fn boden() -> TouchdownWindowSample {
        TouchdownWindowSample {
            at: td() + chrono::Duration::milliseconds(20),
            vs_fpm: -40.0,
            g_force: 1.05,
            g_semibody: None,
            on_ground: true,
            agl_ft: 16.0,
            msl_ft: Some(316.0),
            heading_true_deg: 0.0,
            groundspeed_kt: 130.0,
            indicated_airspeed_kt: 130.0,
            true_airspeed_kt: 130.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg: 3.0,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: None,
        }
    }

    /// 14 s ab 50 ft: sehr lang für ein Linienflugzeug → 50 Punkte (nach
    /// Score-Version 19 deckelt das die Gesamtnote auf 80).
    #[test]
    fn langes_schweben_kommt_als_teilnote_an() {
        let mut stats = FlightStats::default();
        // Ein Anflug von 60 ft, gleichmäßig in 16,8 s auf 0 ft.
        for i in 0..=34 {
            let t = -16_800 + i * 500;
            let h = 60.0 * (-(t as f32)) / 16_800.0;
            stats.anflug_forensik_puffer.push_back(probe(t, h, -210.0));
        }
        // Die Kette des Samplers: messen und ins Analyse-JSON legen, von dort
        // lesen Bewertung, Datensatz und PIREP.
        let mut analysis = serde_json::json!({ "vs_at_edge_fpm": -40.0 });
        abfangen_in_analyse(&stats, &[boden()], td(), &mut analysis);
        stats.landing_analysis = Some(analysis);
        let a = abfangen_aus_analyse(&stats).expect("abfangen im Analyse-JSON");
        assert_eq!(a.grund_ohne_werte, None, "{a:?}");
        assert_eq!(a.dauer_ab_50ft_s, Some(14.0));
        assert_eq!(a.vs_aufsetzen_fpm, Some(-40.0));

        let eingang = scoring_eingang(&stats, Some("A320"), None, None);
        let subs = landing_scoring::compute_sub_scores(&eingang);
        let s = subs
            .iter()
            .find(|s| s.key == "abfangen")
            .expect("Teilnote abfangen");
        assert_eq!(s.points, 50);

        // Gegenprobe: ohne Messung keine Teilnote.
        stats.landing_analysis = Some(serde_json::json!({}));
        let eingang = scoring_eingang(&stats, Some("A320"), None, None);
        assert!(!landing_scoring::compute_sub_scores(&eingang)
            .iter()
            .any(|s| s.key == "abfangen"));
    }

    #[test]
    fn ohne_bodenkontakt_keine_werte() {
        let stats = FlightStats::default();
        let a = abfangen_messen(&stats, &[], td(), Some(-100.0));
        assert_eq!(a.grund_ohne_werte.as_deref(), Some("kein_bodenbezug"));
        assert_eq!(a.dauer_ab_50ft_s, None);
    }

    /// QS 06.10.2026: eine Sim-Pause zwischen 50 ft und Aufsetzen ist keine
    /// Abfangzeit. 8 s geflogen, dazwischen 20 s Pause → 8 s, nicht 28 s.
    #[test]
    fn pause_zaehlt_nicht_als_abfangzeit() {
        let mut stats = FlightStats::default();
        // 60 ft bei −30 s, 50 ft bei −28 s (Wanduhr), Pause −24…−4 s auf
        // 30 ft, danach weiter bis zum Aufsetzen.
        for (t, h) in [
            (-30_000, 60.0),
            (-28_000, 50.0),
            (-26_000, 40.0),
            (-24_000, 30.0),
            (-4_000, 28.0),
            (-2_000, 12.0),
            (-500, 2.0),
        ] {
            stats.anflug_forensik_puffer.push_back(probe(t, h, -300.0));
        }
        stats.pause_segments.push(PauseSegment {
            started_at: td() - chrono::Duration::seconds(24),
            ended_at: td() - chrono::Duration::seconds(4),
            duration_secs: 20,
            reason: PauseReason::SimPause,
            drift_nm: None,
            altitude_delta_ft: None,
            fuel_delta_kg: None,
        });
        let a = abfangen_messen(&stats, &[boden()], td(), Some(-60.0));
        assert_eq!(a.dauer_ab_50ft_s, Some(8.0), "{a:?}");
        // Gegenprobe: ohne Pause-Block zählt die Wanduhr 28 s.
        stats.pause_segments.clear();
        let b = abfangen_messen(&stats, &[boden()], td(), Some(-60.0));
        assert_eq!(b.dauer_ab_50ft_s, Some(28.0));
    }

    /// QS 06.10.2026 (Runde 3): Das Fenster gilt für geflogene Zeit. Eine
    /// lange Pause im Abfangen ließ vorher alle Proben vor ihr herausfallen —
    /// die Teilnote verschwand. 50 ft bei −200 s Wanduhr, 190 s Pause → 10 s.
    #[test]
    fn lange_pause_laesst_die_teilnote_nicht_verschwinden() {
        let mut stats = FlightStats::default();
        for (t, h) in [
            (-202_000, 60.0),
            (-200_000, 50.0),
            (-196_000, 30.0),
            (-4_000, 25.0),
            (-500, 2.0),
        ] {
            stats.anflug_forensik_puffer.push_back(probe(t, h, -300.0));
        }
        stats.pause_segments.push(PauseSegment {
            started_at: td() - chrono::Duration::seconds(195),
            ended_at: td() - chrono::Duration::seconds(5),
            duration_secs: 190,
            reason: PauseReason::SimPause,
            drift_nm: None,
            altitude_delta_ft: None,
            fuel_delta_kg: None,
        });
        let a = abfangen_messen(&stats, &[boden()], td(), Some(-60.0));
        assert_eq!(a.dauer_ab_50ft_s, Some(10.0), "{a:?}");
    }

    /// QS 06.10.2026: länger als 60 s unter 50 ft war „keine Messung" (keine
    /// Teilnote) statt der schlechtesten Stufe.
    #[test]
    fn langes_schweben_ueber_60_s_wird_gemessen() {
        let mut stats = FlightStats::default();
        for (t, h) in [
            (-95_000, 70.0),
            (-90_000, 50.0),
            (-60_000, 20.0),
            (-30_000, 10.0),
            (-1_000, 1.0),
        ] {
            stats.anflug_forensik_puffer.push_back(probe(t, h, -40.0));
        }
        let a = abfangen_messen(&stats, &[boden()], td(), Some(-40.0));
        assert_eq!(a.dauer_ab_50ft_s, Some(90.0), "{a:?}");
    }

    /// QS 06.10.2026: Neustart zwischen Aufsetzen und Einreichen — die
    /// gesicherte Messung trägt, wenn `landing_analysis` fehlt; eine positive
    /// Aufsetzrate (unplausibel) wird nicht als Messwert gezeigt.
    #[test]
    fn abfangen_ueberlebt_neustart_und_positive_rate_faellt_weg() {
        let mut stats = FlightStats::default();
        for i in 0..=34 {
            let t = -16_800 + i * 500;
            let h = 60.0 * (-(t as f32)) / 16_800.0;
            stats.anflug_forensik_puffer.push_back(probe(t, h, -210.0));
        }
        let mut analysis = serde_json::json!({ "vs_at_edge_fpm": 24.47 });
        let a = abfangen_in_analyse(&stats, &[boden()], td(), &mut analysis);
        assert_eq!(a.vs_aufsetzen_fpm, None, "positive Rate ist kein Messwert");
        assert_eq!(a.reduktion_fpm, None);
        // Unter −3000 fpm ebenso (wie der Datensatz, QS Runde 4) …
        let mut glitch = serde_json::json!({ "vs_at_edge_fpm": -3500.0 });
        assert_eq!(
            abfangen_in_analyse(&stats, &[boden()], td(), &mut glitch).vs_aufsetzen_fpm,
            None
        );
        // … Gegenprobe: eine plausible Rate bleibt.
        let mut gut = serde_json::json!({ "vs_at_edge_fpm": -120.0 });
        assert_eq!(
            abfangen_in_analyse(&stats, &[boden()], td(), &mut gut).vs_aufsetzen_fpm,
            Some(-120.0)
        );
        stats.abfangen_gesichert = Some(a);
        stats.approach_stable_at_da = Some(true);
        stats.landing_vref_source = Some("pmdg");
        stats.arr_metar_raw = Some("LTFM 052020Z 06009KT".to_string());
        // Rundreise über die Sicherung, `landing_analysis` fehlt danach.
        let gesichert = PersistedFlightStats::snapshot_from(&stats);
        let mut neu = FlightStats::default();
        gesichert.apply_to(&mut neu);
        assert!(neu.landing_analysis.is_none());
        // QS Runde 4: auch die übrigen Aufsetzwerte des Datensatzes.
        assert_eq!(neu.approach_stable_at_da, Some(true));
        assert_eq!(neu.landing_vref_source, Some("pmdg"));
        assert_eq!(neu.arr_metar_raw.as_deref(), Some("LTFM 052020Z 06009KT"));
        assert_eq!(
            abfangen_aus_analyse(&neu).and_then(|a| a.dauer_ab_50ft_s),
            Some(14.0)
        );
    }
