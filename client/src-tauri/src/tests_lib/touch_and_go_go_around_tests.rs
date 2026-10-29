    use super::*;
    use chrono::TimeZone;
    use sim_core::SimSnapshot;

    fn t0() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 3, 12, 0, 0).unwrap()
    }

    /// 1 NM vor der Schwelle EDDF 25C (50.0451 N, 8.5870 E, Bahnkurs 249,6°
    /// laut OurAirports-Tabelle), Kurs auf die Bahn.
    fn endanflug_eddf_25c(agl_ft: f64, vs_fpm: f32) -> SimSnapshot {
        let mut s = snap_at(agl_ft, vs_fpm, false);
        s.lat = 50.0509;
        s.lon = 8.6113;
        s.heading_deg_true = 249.6;
        s
    }

    /// 17.10.2026: Dasselbe Steigen aus dem Anflug fern jeder Bahn ist kein
    /// Durchstarter — die Phase geht in den Steigflug, gezählt wird nichts.
    #[test]
    fn climb_away_without_runway_is_climb_but_no_go_around() {
        let mut stats = FlightStats::default();
        let mut tief = snap_at(400.0, -600.0, false);
        // Mitten über der Lüneburger Heide, kein Platz in 3 NM.
        tief.lat = 53.10;
        tief.lon = 10.10;
        tief.heading_deg_true = 90.0;
        update_lowest_approach_agl(&mut stats, &tief);
        let now = t0();
        let climb = snap_at(700.0, 1200.0, false);
        assert!(check_go_around(&mut stats, &climb, now).is_none());
        let out = check_go_around(
            &mut stats,
            &climb,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1),
        );
        assert!(matches!(out, Some(FlightPhase::Climb)), "phase still leaves the approach");
        assert_eq!(stats.go_around_count, 0, "no runway ahead — not a go-around");
        assert!(stats.pending_acars_logs.is_empty());
        assert_eq!(stats.lowest_agl_during_approach_ft, None);
        assert_eq!(stats.tiefster_anflugpunkt, None);
    }

    /// Kurs quer zu allen Bahnen (Überflug des Platzes) zählt nicht.
    /// EDDW: Bahnen 09/27 und 05/23 (88/268°, 52/232°), Kurs 160°.
    #[test]
    fn crossing_the_field_is_no_go_around() {
        let mut stats = FlightStats::default();
        let mut tief = snap_at(400.0, -600.0, false);
        tief.lat = 53.0465;
        tief.lon = 8.742;
        tief.heading_deg_true = 160.0;
        update_lowest_approach_agl(&mut stats, &tief);
        let now = t0();
        let climb = snap_at(700.0, 1200.0, false);
        check_go_around(&mut stats, &climb, now);
        let out = check_go_around(&mut stats, &climb, now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1));
        assert!(matches!(out, Some(FlightPhase::Climb)));
        assert_eq!(stats.go_around_count, 0);
    }

    fn snap_at(agl_ft: f64, vs_fpm: f32, on_ground: bool) -> SimSnapshot {
        let mut s = SimSnapshot::default();
        s.altitude_agl_ft = agl_ft;
        s.vertical_speed_fpm = vs_fpm;
        s.on_ground = on_ground;
        s.engines_running = 2;
        s
    }

    // ---- update_lowest_approach_agl ----

    #[test]
    fn lowest_agl_starts_unset_and_takes_first_sample() {
        let mut stats = FlightStats::default();
        assert_eq!(stats.lowest_agl_during_approach_ft, None);
        update_lowest_approach_agl(&mut stats, &snap_at(2500.0, -800.0, false));
        assert_eq!(stats.lowest_agl_during_approach_ft, Some(2500.0));
    }

    #[test]
    fn lowest_agl_only_decreases_never_increases() {
        let mut stats = FlightStats::default();
        update_lowest_approach_agl(&mut stats, &snap_at(2500.0, -800.0, false));
        update_lowest_approach_agl(&mut stats, &snap_at(1200.0, -700.0, false));
        update_lowest_approach_agl(&mut stats, &snap_at(800.0, -600.0, false));
        // A climb-correction during approach must NOT erase the minimum.
        update_lowest_approach_agl(&mut stats, &snap_at(900.0, 200.0, false));
        assert_eq!(stats.lowest_agl_during_approach_ft, Some(800.0));
    }

    #[test]
    fn lowest_agl_ignores_negative_glitch_samples() {
        let mut stats = FlightStats::default();
        update_lowest_approach_agl(&mut stats, &snap_at(1200.0, -700.0, false));
        // Brief sim/terrain-mesh glitch reporting -50 ft AGL — must not
        // poison the minimum, otherwise every later sample looks like a
        // 200 ft go-around climb.
        update_lowest_approach_agl(&mut stats, &snap_at(-50.0, -700.0, false));
        assert_eq!(stats.lowest_agl_during_approach_ft, Some(1200.0));
    }

    // ---- check_go_around ----

    #[test]
    fn go_around_does_nothing_without_a_prior_minimum() {
        let mut stats = FlightStats::default();
        // No update_lowest_approach_agl called → no minimum tracked.
        let out = check_go_around(&mut stats, &snap_at(2000.0, 1500.0, false), t0());
        assert!(out.is_none());
        assert_eq!(stats.go_around_count, 0);
    }

    #[test]
    fn go_around_fires_after_dwell_seconds() {
        let mut stats = FlightStats::default();
        // Establish approach minimum at 400 ft AGL — 1 NM vor der Schwelle
        // EDDF 25C, auf Bahnkurs (seit 17.10.2026 zählt ein Durchstarter
        // nur mit einer Landeschwelle voraus).
        update_lowest_approach_agl(&mut stats, &endanflug_eddf_25c(400.0, -600.0));
        let now = t0();
        // Climb-back: 700 ft AGL with V/S +1200 fpm — meets all
        // conditions (700 > 400+200, 1200 > 500, airborne, engines on).
        let climb = snap_at(700.0, 1200.0, false);
        // First tick: arms the dwell, doesn't classify yet.
        let out = check_go_around(&mut stats, &climb, now);
        assert!(out.is_none());
        assert!(stats.go_around_climb_pending_since.is_some());
        // After GO_AROUND_DWELL_SECS the classification fires.
        let out2 = check_go_around(
            &mut stats,
            &climb,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1),
        );
        assert!(matches!(out2, Some(FlightPhase::Climb)));
        assert_eq!(stats.go_around_count, 1);
        // Lowest tracker reset so the next descent starts a fresh window.
        assert_eq!(stats.lowest_agl_during_approach_ft, None);
        assert_eq!(stats.go_around_climb_pending_since, None);
        // ACARS log line was queued for the streamer.
        assert_eq!(stats.pending_acars_logs.len(), 1);
        assert!(stats.pending_acars_logs[0].contains("Go-around"));
    }

    /// Karte, Anflug-Urteil und Stabilitaetsachse muessen dieselben sechs
    /// Werte aus denselben Feldern lesen (Bank-Streuung: v2 vor Legacy).
    /// Jedes Feld bekommt einen eigenen Wert — vertauschte Felder fallen auf.
    #[test]
    fn anflug_urteil_liest_dieselben_felder_wie_die_karte() {
        let mut stats = FlightStats::default();
        stats.approach_vs_jerk_fpm = Some(11.0);
        stats.approach_bank_stddev_filtered_deg = None;
        stats.approach_bank_stddev_deg = Some(7.5);
        stats.approach_ias_stddev_kt = Some(33.0);
        stats.approach_excessive_sink = Some(true);
        stats.approach_stable_config = Some(false);
        // Score-Version 19: Gleitpfad statt der Sinkraten-Abweichungen.
        stats.anflug_forensik.gleitpfad = Some(landing_scoring::anflug_forensik::AnflugGleitpfad {
            gesamt: Some(landing_scoring::anflug_forensik::GleitpfadTor {
                mittel_abs_dots: 0.66,
                ..Default::default()
            }),
            ..Default::default()
        });
        let a = scoring_eingang(&stats, None, None, None).anflug;
        assert_eq!(a.vs_jerk_fpm, Some(11.0));
        assert_eq!(a.bank_stddev_deg, Some(7.5), "Legacy, wenn v2 fehlt");
        assert_eq!(a.ias_stddev_kt, Some(33.0));
        assert_eq!(a.excessive_sink, Some(true));
        assert_eq!(a.stable_config, Some(false));
        assert_eq!(a.gleitpfad_dots, Some(0.66));
        stats.approach_bank_stddev_filtered_deg = Some(1.0);
        let a = scoring_eingang(&stats, None, None, None).anflug;
        assert_eq!(a.bank_stddev_deg, Some(1.0), "v2 gewinnt");
    }

    /// QS 06.10.2026 (Runde 6): Update mit Neustart zwischen Aufsetzen und
    /// Einreichen — der Datensatz nennt die Version, die gemessen hat (wie
    /// der Touchdown in der Webapp), eine spätere Landung die laufende.
    #[test]
    fn client_version_der_messung_ueberlebt_neustart() {
        let jetzt = env!("CARGO_PKG_VERSION");
        let at = Utc::now();
        let mut stats = FlightStats::default();
        assert_eq!(aufzeichnende_client_version(&stats), jetzt);
        // Vor dem Aufsetzen sichert nichts eine Version.
        assert_eq!(
            PersistedFlightStats::snapshot_from(&stats).client_version_aufsetzen,
            None
        );
        // Die Landung hat eine ältere Version gemessen (Sicherung von vorher).
        stats.landing_at = Some(at);
        stats.client_version_aufsetzen = Some(("1.0.0".to_string(), at));
        let mut neu = FlightStats::default();
        PersistedFlightStats::snapshot_from(&stats).apply_to(&mut neu);
        assert_eq!(aufzeichnende_client_version(&neu), "1.0.0");
        // Gegenprobe: ohne ältere Sicherung zählt die laufende Version …
        let mut frisch = FlightStats::default();
        frisch.landing_at = Some(at);
        let mut neu2 = FlightStats::default();
        PersistedFlightStats::snapshot_from(&frisch).apply_to(&mut neu2);
        assert_eq!(aufzeichnende_client_version(&neu2), jetzt);
        // … und eine spätere Landung (Touch-and-Go) misst die laufende.
        neu.landing_at = Some(at + chrono::Duration::seconds(300));
        assert_eq!(aufzeichnende_client_version(&neu), jetzt);
    }

    /// QS 06.10.2026 (Runde 5): Neustart zwischen Aufsetzen und Einreichen —
    /// das Anflug-Urteil (Note, Deckel, `approach_stable_at_gate`) bleibt
    /// dasselbe, und Gleitpfad/Anflugruhe stehen weiter im Datensatz.
    #[test]
    fn anflug_urteil_ueberlebt_neustart() {
        let mut stats = FlightStats::default();
        stats.approach_vs_jerk_fpm = Some(20.0);
        stats.approach_bank_stddev_deg = Some(1.0);
        stats.approach_ias_stddev_kt = Some(1.0);
        stats.approach_excessive_sink = Some(false);
        stats.approach_stable_config = Some(true);
        stats.anflug_forensik.gleitpfad = Some(landing_scoring::anflug_forensik::AnflugGleitpfad {
            gesamt: Some(landing_scoring::anflug_forensik::GleitpfadTor {
                mittel_abs_dots: 1.69,
                ..Default::default()
            }),
            ..Default::default()
        });
        stats.anflug_forensik.ruhe = Some(landing_scoring::anflug_forensik::AnflugRuhe {
            hoehenbezug: "schwelle".to_string(),
            ..Default::default()
        });
        let vorher = anflug_werte(&stats);
        // Gegenprobe: ohne Gleitpfad fiele das Urteil anders aus.
        let mut ohne = vorher;
        ohne.gleitpfad_dots = None;
        assert_ne!(
            landing_scoring::anflug_urteil::anflug_urteil(&vorher),
            landing_scoring::anflug_urteil::anflug_urteil(&ohne)
        );
        let mut neu = FlightStats::default();
        PersistedFlightStats::snapshot_from(&stats).apply_to(&mut neu);
        assert_eq!(anflug_werte(&neu), vorher);
        assert_eq!(anflug_stabil_am_gate(&neu), anflug_stabil_am_gate(&stats));
        assert_eq!(neu.anflug_forensik, stats.anflug_forensik);
    }

    /// Score-Version 19 (QS 05.10.2026): `approach_stable_at_gate` im
    /// Payload ist dasselbe Urteil wie in der Note — vorher eine eigene,
    /// lockerere Pruefung (GSG1709: Payload „stabil", Note „teilweise").
    #[test]
    fn stable_at_gate_ist_das_urteil_der_note() {
        let mut stats = FlightStats::default();
        assert_eq!(anflug_stabil_am_gate(&stats), None, "nichts gemessen");
        stats.approach_vs_jerk_fpm = Some(20.0);
        stats.approach_bank_stddev_deg = Some(1.0);
        stats.approach_ias_stddev_kt = Some(1.0);
        stats.approach_excessive_sink = Some(false);
        stats.approach_stable_config = Some(true);
        stats.anflug_forensik.gleitpfad = Some(landing_scoring::anflug_forensik::AnflugGleitpfad {
            gesamt: Some(landing_scoring::anflug_forensik::GleitpfadTor {
                mittel_abs_dots: 0.24,
                ..Default::default()
            }),
            ..Default::default()
        });
        assert_eq!(anflug_stabil_am_gate(&stats), Some(true), "GSG1709");
        // QAF434: Gleitpfad 1,69 Dots → teilweise → nicht stabil.
        stats
            .anflug_forensik
            .gleitpfad
            .as_mut()
            .unwrap()
            .gesamt
            .as_mut()
            .unwrap()
            .mittel_abs_dots = 1.69;
        assert_eq!(anflug_stabil_am_gate(&stats), Some(false), "QAF434");
        assert_eq!(
            landing_scoring::anflug_urteil::anflug_urteil(&anflug_werte(&stats)),
            Some(landing_scoring::anflug_urteil::AnflugUrteil::Partial)
        );
    }

    /// Das Wort im phpVMS-Feld beschreibt die Gesamtnote (gleiche Schwellen
    /// wie die Kategorie), nicht den Touchdown.
    #[test]
    fn gesamtnote_wort_folgt_den_schwellen() {
        assert_eq!(gesamtnote_wort(100), "excellent");
        assert_eq!(gesamtnote_wort(88), "excellent");
        assert_eq!(gesamtnote_wort(87), "good");
        assert_eq!(gesamtnote_wort(75), "good");
        assert_eq!(gesamtnote_wort(74), "sufficient");
        assert_eq!(gesamtnote_wort(50), "sufficient");
        assert_eq!(gesamtnote_wort(45), "poor");
        assert_eq!(gesamtnote_wort(14), "insufficient");
        for n in [0, 14, 15, 49, 50, 74, 75, 87, 88, 100] {
            // Gleiche Stufe wie die Kategorie-Kennung.
            let k = aggregate_score_label(n as u8);
            let w = gesamtnote_wort(n);
            let erwartet = match k {
                "smooth" => "excellent",
                "acceptable" => "good",
                "firm" => "sufficient",
                "hard" => "poor",
                _ => "insufficient",
            };
            assert_eq!(w, erwartet, "{n}");
        }
    }

    /// Hubschrauber/Wasserflugzeug: kein Anflug-Urteil, keine Stabilitaets-
    /// achse — sonst deckelte das 3°-Modell sie auf 80/45.
    #[test]
    fn nicht_konventionell_bekommt_kein_anflug_urteil() {
        let mut stats = FlightStats::default();
        stats.landing_nicht_konventionell = true;
        stats.approach_vs_stddev_filtered_fpm = Some(900.0);
        stats.approach_bank_stddev_filtered_deg = Some(12.0);
        stats.approach_vs_jerk_fpm = Some(400.0);
        stats.approach_excessive_sink = Some(true);
        let mut eingang = scoring_eingang(&stats, None, None, None);
        fill_v2_rollout_fields(&mut eingang, &stats, "EDDM");
        assert_eq!(eingang.approach_vs_stddev_fpm, None);
        assert_eq!(eingang.approach_bank_stddev_deg, None);
        assert_eq!(eingang.anflug, Default::default());
        let subs = landing_scoring::compute_sub_scores(&eingang);
        assert!(!subs.iter().any(|s| s.key == "stability"));
    }

    /// DLH2248 (LFLL, 01.10.2026): Pilot faengt auf 1200 ft ueber der Bahn
    /// ab. Das Gelaende darunter schwankt, die AGL springt von 709 auf 1359 ft
    /// — die Hoehe ueber der Bahn (HAT) kam nie unter 1164 ft. Kein Durchstart.
    #[test]
    fn go_around_ignoriert_gelaendesprung_ueber_dem_gate() {
        let mut stats = FlightStats::default();
        stats.arr_airport_elevation_ft = Some(814.0);
        let mut tief = snap_at(709.0, -560.0, false);
        tief.altitude_msl_ft = 814.0 + 1164.0;
        update_lowest_approach_agl(&mut stats, &tief);
        assert_eq!(stats.lowest_agl_during_approach_ft, Some(1164.0));
        let mut hoch = snap_at(1359.0, 500.0, false);
        hoch.altitude_msl_ft = 814.0 + 1372.0;
        let now = t0();
        assert!(check_go_around(&mut stats, &hoch, now).is_none());
        assert!(check_go_around(
            &mut stats,
            &hoch,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 5)
        )
        .is_none());
        assert_eq!(stats.go_around_count, 0);
    }

    /// Dasselbe Gelaende, aber ein echter Durchstart unter dem Gate: Der
    /// Detektor muss weiter greifen — gemessen in HAT.
    /// Platzhoehe um 900 ft falsch (Ausweichplatz, falsche phpVMS-Hoehe):
    /// ein echter Durchstart aus 150 ft AGL darf nicht still verschwinden —
    /// die enge 1000-ft-Grenze gilt nur, wenn HAT und AGL zusammenpassen.
    #[test]
    fn go_around_bleibt_bei_platzhoehenfehler_von_900_ft_erkennbar() {
        let mut stats = FlightStats::default();
        stats.arr_airport_elevation_ft = Some(100.0);
        let mut tief = snap_at(150.0, -600.0, false);
        tief.altitude_msl_ft = 1_000.0 + 150.0; // Platz liegt in Wahrheit auf 1000 ft
        update_lowest_approach_agl(&mut stats, &tief);
        assert_eq!(stats.lowest_agl_during_approach_ft, Some(1_050.0));
        let mut hoch = snap_at(450.0, 1200.0, false);
        hoch.altitude_msl_ft = 1_000.0 + 450.0;
        let now = t0();
        check_go_around(&mut stats, &hoch, now);
        let out = check_go_around(
            &mut stats,
            &hoch,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1),
        );
        assert!(matches!(out, Some(FlightPhase::Climb)));
    }

    #[test]
    fn go_around_greift_unter_dem_gate_in_hat() {
        let mut stats = FlightStats::default();
        stats.arr_airport_elevation_ft = Some(814.0);
        let mut tief = snap_at(300.0, -600.0, false);
        tief.altitude_msl_ft = 814.0 + 400.0;
        update_lowest_approach_agl(&mut stats, &tief);
        let mut hoch = snap_at(900.0, 1200.0, false);
        hoch.altitude_msl_ft = 814.0 + 700.0;
        let now = t0();
        assert!(check_go_around(&mut stats, &hoch, now).is_none());
        let out = check_go_around(
            &mut stats,
            &hoch,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1),
        );
        assert!(matches!(out, Some(FlightPhase::Climb)));
    }

    /// Umgekehrt: AGL springt hoch, die Hoehe ueber der Bahn aber nicht —
    /// unter dem Gate darf das kein Durchstart sein.
    #[test]
    fn go_around_ignoriert_agl_sprung_unter_dem_gate() {
        let mut stats = FlightStats::default();
        stats.arr_airport_elevation_ft = Some(814.0);
        let mut tief = snap_at(300.0, -600.0, false);
        tief.altitude_msl_ft = 814.0 + 400.0;
        update_lowest_approach_agl(&mut stats, &tief);
        let mut gelaende_fall = snap_at(900.0, 500.0, false);
        gelaende_fall.altitude_msl_ft = 814.0 + 420.0;
        let now = t0();
        check_go_around(&mut stats, &gelaende_fall, now);
        assert!(check_go_around(
            &mut stats,
            &gelaende_fall,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 5)
        )
        .is_none());
    }

    #[test]
    fn go_around_does_not_fire_below_recovery_threshold() {
        let mut stats = FlightStats::default();
        update_lowest_approach_agl(&mut stats, &snap_at(400.0, -600.0, false));
        let now = t0();
        // Only +150 ft above lowest — below GO_AROUND_AGL_RECOVERY_FT.
        // Even with V/S +1500 we should NOT classify.
        let weak = snap_at(550.0, 1500.0, false);
        check_go_around(&mut stats, &weak, now);
        let out = check_go_around(
            &mut stats,
            &weak,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1),
        );
        assert!(out.is_none());
        assert_eq!(stats.go_around_count, 0);
    }

    #[test]
    fn go_around_dwell_resets_when_conditions_break() {
        let mut stats = FlightStats::default();
        update_lowest_approach_agl(&mut stats, &snap_at(400.0, -600.0, false));
        let now = t0();
        // Brief climb arms the dwell.
        check_go_around(&mut stats, &snap_at(700.0, 1200.0, false), now);
        assert!(stats.go_around_climb_pending_since.is_some());
        // V/S drops back below threshold mid-dwell — reset expected.
        check_go_around(
            &mut stats,
            &snap_at(700.0, 200.0, false),
            now + chrono::Duration::seconds(3),
        );
        assert!(stats.go_around_climb_pending_since.is_none());
        assert_eq!(stats.go_around_count, 0);
    }

    #[test]
    fn go_around_does_not_fire_when_aircraft_caught_glideslope_high() {
        let mut stats = FlightStats::default();
        // Pilot intercepted the GS from above — minimum is still very
        // high (3000 ft AGL during approach intercept). A brief +800
        // fpm during the level-off MUST NOT trigger GA.
        update_lowest_approach_agl(&mut stats, &snap_at(3000.0, -800.0, false));
        let now = t0();
        let blip = snap_at(3300.0, 800.0, false);
        check_go_around(&mut stats, &blip, now);
        let out = check_go_around(
            &mut stats,
            &blip,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1),
        );
        // lowest > 1500 ft → detector is gated off entirely.
        assert!(out.is_none());
        assert_eq!(stats.go_around_count, 0);
    }

    #[test]
    fn go_around_requires_engines_running() {
        let mut stats = FlightStats::default();
        update_lowest_approach_agl(&mut stats, &snap_at(400.0, -600.0, false));
        let now = t0();
        let mut climb = snap_at(700.0, 1200.0, false);
        climb.engines_running = 0; // engines off → not really going around
        check_go_around(&mut stats, &climb, now);
        let out = check_go_around(
            &mut stats,
            &climb,
            now + chrono::Duration::seconds(GO_AROUND_DWELL_SECS + 1),
        );
        assert!(out.is_none());
        assert_eq!(stats.go_around_count, 0);
    }

    // ---- v0.7.17 (B-015a QS-Fix): score_basis_vs_fpm ----

    #[test]
    fn score_basis_uses_edge_when_negative() {
        // EIN799-Regression: vs_at_edge_fpm = -265 muss gewinnen gegen
        // landing_peak_vs_fpm = -311.
        let mut stats = FlightStats::default();
        stats.landing_peak_vs_fpm = Some(-311.0);
        stats.landing_rate_fpm = Some(-311.0);
        stats.landing_analysis = Some(serde_json::json!({
            "vs_at_edge_fpm": -265.0,
        }));
        assert_eq!(score_basis_vs_fpm(&stats), Some(-265.0));
    }

    #[test]
    fn score_basis_falls_back_when_edge_missing() {
        let mut stats = FlightStats::default();
        stats.landing_peak_vs_fpm = Some(-311.0);
        stats.landing_rate_fpm = Some(-358.0);
        // No landing_analysis at all.
        assert_eq!(score_basis_vs_fpm(&stats), Some(-311.0));
        // landing_analysis ohne vs_at_edge_fpm-Key.
        stats.landing_analysis = Some(serde_json::json!({
            "other_field": 42,
        }));
        assert_eq!(score_basis_vs_fpm(&stats), Some(-311.0));
    }

    #[test]
    fn score_basis_ignores_positive_edge() {
        // Bumping/Ballooning kurz vor TD kann positive Werte produzieren —
        // das ist kein echter Aufsetz-Moment, fallback auf peak.
        let mut stats = FlightStats::default();
        stats.landing_peak_vs_fpm = Some(-311.0);
        stats.landing_analysis = Some(serde_json::json!({
            "vs_at_edge_fpm": 12.0,
        }));
        assert_eq!(score_basis_vs_fpm(&stats), Some(-311.0));
    }

    #[test]
    fn score_basis_full_fallback_chain() {
        // peak None → rate.
        let mut stats = FlightStats::default();
        stats.landing_peak_vs_fpm = None;
        stats.landing_rate_fpm = Some(-358.0);
        assert_eq!(score_basis_vs_fpm(&stats), Some(-358.0));
        // alle None → None.
        let stats2 = FlightStats::default();
        assert_eq!(score_basis_vs_fpm(&stats2), None);
    }

    // ---- v0.15.x: Backend-Track-Akkumulation (Live-Map-Lücken-Fix) ----

    #[test]
    fn record_track_point_thins_like_frontend() {
        let mut stats = FlightStats::default();
        assert!(stats.track.is_empty());

        // Erster Punkt wird IMMER aufgenommen.
        record_track_point(&mut stats, 10.0, 50.0, false);
        assert_eq!(stats.track, vec![[10.0, 50.0]]);

        // Winziges Luft-Delta (< 0,002°) → verworfen (grobe Luft-Schwelle).
        record_track_point(&mut stats, 10.0005, 50.0005, false);
        assert_eq!(stats.track, vec![[10.0, 50.0]]);

        // Ground-Delta von 0,0002° (> 0,00015°) → aufgenommen (feine Boden-Schwelle).
        record_track_point(&mut stats, 10.0007, 50.0007, true);
        assert_eq!(stats.track, vec![[10.0, 50.0], [10.0007, 50.0007]]);

        // Nicht-endliche Koordinaten sind ein No-op.
        record_track_point(&mut stats, f64::NAN, 50.0, false);
        record_track_point(&mut stats, 10.0, f64::INFINITY, false);
        assert_eq!(stats.track.len(), 2);

        // Kappe drained die ÄLTESTEN Punkte. Wir füllen über MAX_TRACK_POINTS
        // mit garantiert über der Luft-Schwelle liegenden Schritten (0,01°).
        let mut stats2 = FlightStats::default();
        let total = MAX_TRACK_POINTS + 3;
        for i in 0..total {
            let lon = i as f64 * 0.01;
            record_track_point(&mut stats2, lon, 0.0, false);
        }
        assert_eq!(stats2.track.len(), MAX_TRACK_POINTS);
        // Die ersten 3 (0.0, 0.01, 0.02) sind rausgefallen → ältester ist jetzt 0.03.
        assert_eq!(stats2.track.first().unwrap()[0], 3.0 * 0.01);
        assert_eq!(stats2.track.last().unwrap()[0], (total - 1) as f64 * 0.01);
    }

    // v0.15.25: der Resume-Reseed replayt die kompletten Server-Positionen
    // (sim_time ASC) durch `record_track_point` mit on_ground=false. Dieser
    // Test spiegelt diesen Replay: Reihenfolge bleibt erhalten, der erste Punkt
    // wird immer behalten, unter der Luft-Schwelle liegende Punkte fallen raus.
    #[test]
    fn record_track_point_replays_server_track_thinned_and_ordered() {
        // Server-style Positionen (lon, lat), sim_time ASC. Manche Schritte sind
        // bewusst kleiner als die Luft-Schwelle (0,002°) → müssen ausgedünnt werden.
        let server_pts: Vec<(f64, f64)> = vec![
            (10.0000, 50.0000), // erster Punkt → immer behalten
            (10.0005, 50.0005), // Δ < 0,002° → verworfen
            (10.0100, 50.0100), // Δ > 0,002° → behalten
            (10.0105, 50.0105), // Δ < 0,002° → verworfen
            (10.0300, 50.0300), // Δ > 0,002° → behalten
        ];

        let mut stats = FlightStats::default();
        assert!(stats.track.is_empty());
        for &(lon, lat) in &server_pts {
            // on_ground=false = AIR-Schwelle (kein verlässlicher Ground-State im Replay).
            record_track_point(&mut stats, lon, lat, false);
        }

        // Ausgedünnt: 5 rohe Punkte → 3 behaltene.
        assert_eq!(
            stats.track,
            vec![[10.0000, 50.0000], [10.0100, 50.0100], [10.0300, 50.0300]]
        );
        // Reihenfolge erhalten: erster behaltener = ältester Server-Punkt,
        // letzter behaltener = neuester Server-Punkt.
        assert_eq!(stats.track.first().unwrap(), &[10.0000, 50.0000]);
        assert_eq!(stats.track.last().unwrap(), &[10.0300, 50.0300]);
    }
