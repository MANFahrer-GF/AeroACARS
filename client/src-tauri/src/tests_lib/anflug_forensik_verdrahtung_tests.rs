    use super::*;
    use chrono::TimeZone;
    use sim_core::SimSnapshot;

    fn td() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap()
    }

    #[test]
    fn puffer_sammelt_position_nick_und_mittleres_n1() {
        let mut stats = FlightStats::default();
        let mut snap = SimSnapshot::default();
        snap.lat = 50.1;
        snap.lon = 8.2;
        snap.pitch_deg = 2.5;
        snap.eng_n1_pct = Some(vec![60.0, 62.0]);
        push_approach_sample(&mut stats, &snap, td());
        let s = stats.approach_buffer.back().expect("Probe im Puffer");
        assert_eq!(s.lat, Some(50.1));
        assert_eq!(s.lon, Some(8.2));
        assert_eq!(s.pitch_deg, Some(2.5));
        assert_eq!(s.n1_mittel_pct, Some(61.0));

        // Kaputte Position / kein N1 (X-Plane): die Probe BLEIBT, nur die
        // neuen Felder sind leer — die Stabilitaets-Auswertung braucht sie
        // nicht und darf keine Probe dadurch verlieren.
        snap.lat = f64::NAN;
        snap.eng_n1_pct = None;
        push_approach_sample(&mut stats, &snap, td());
        assert_eq!(stats.approach_buffer.len(), 2);
        let s = stats.approach_buffer.back().unwrap();
        assert_eq!(s.lat, None);
        assert_eq!(s.n1_mittel_pct, None);

        // Ein stehendes Triebwerk (N1 ≈ 0) halbiert das Mittel nicht.
        snap.lat = 50.1;
        snap.eng_n1_pct = Some(vec![62.0, 0.3]);
        push_approach_sample(&mut stats, &snap, td());
        assert_eq!(
            stats.approach_buffer.back().unwrap().n1_mittel_pct,
            Some(62.0)
        );
    }

    /// Befund 3: Der Forensik-Puffer bekommt dieselben Proben, haelt aber
    /// 400 statt 120 — die Notenkappe bleibt unberuehrt.
    #[test]
    fn forensik_puffer_haelt_laenger_als_der_notenpuffer() {
        let mut stats = FlightStats::default();
        let mut snap = SimSnapshot::default();
        snap.lat = 50.0;
        snap.lon = 8.0;
        for i in 0..450 {
            snap.altitude_msl_ft = 2000.0 - i as f64;
            push_approach_sample(&mut stats, &snap, td() + chrono::Duration::seconds(i));
        }
        assert_eq!(stats.approach_buffer.len(), APPROACH_BUFFER_MAX);
        assert_eq!(
            stats.anflug_forensik_puffer.len(),
            ANFLUG_FORENSIK_PUFFER_MAX
        );
        // Beide enden mit derselben Probe.
        assert_eq!(
            stats.anflug_forensik_puffer.back().unwrap().at,
            stats.approach_buffer.back().unwrap().at
        );
        // Eine verworfene (unplausible) Probe landet in KEINEM der beiden.
        snap.vertical_speed_fpm = f32::NAN;
        push_approach_sample(&mut stats, &snap, td() + chrono::Duration::seconds(999));
        assert_ne!(
            stats.anflug_forensik_puffer.back().unwrap().at,
            td() + chrono::Duration::seconds(999)
        );
    }

    /// Bahn 36 bei 50°N/8°E, Schwelle auf 300 ft, ILS 3°, TCH 50 ft.
    fn nav_36() -> aeroacars_mqtt::navdata::NavRunway {
        serde_json::from_value(serde_json::json!({
            "designator": "36", "magnetic_course": 360.0, "true_course": 360.0,
            "length_ft": 9000, "width_ft": 148,
            "threshold": {"lat": 50.0, "lon": 8.0, "elev_ft": 300},
            "end": {"lat": 50.03, "lon": 8.0, "elev_ft": 310},
            "ils": {"freq_mhz": 110.1, "course": 360.0, "category": 1},
            "glideslope_angle": 3.0, "tch_ft": 50
        }))
        .unwrap()
    }

    /// Ein Anflug 1000 → 200 ft ueber `push_approach_sample`, die letzte
    /// Probe bei `ende`, `abw_deg` ueber dem 3°-Pfad (vom GPI aus).
    fn anflug_bis(stats: &mut FlightStats, ende: DateTime<Utc>, abw_deg: f64) {
        const M_JE_GRAD: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;
        let ist = (3.0 + abw_deg).to_radians();
        let gpi = 50.0 / 3.0_f64.to_radians().tan();
        for i in 0..=40 {
            let h = 1000.0 - 20.0 * i as f64;
            let d_m = (h / ist.tan() - gpi) / 3.280_839_895;
            let mut snap = SimSnapshot::default();
            snap.lat = 50.0 - d_m / M_JE_GRAD;
            snap.lon = 8.0;
            snap.altitude_msl_ft = 300.0 + h;
            snap.altitude_agl_ft = h;
            snap.vertical_speed_fpm = -700.0;
            let at = ende - chrono::Duration::seconds(40 - i as i64);
            push_approach_sample(stats, &snap, at);
        }
    }

    fn gesamt(stats: &FlightStats) -> storage::GleitpfadTor {
        let g = stats.anflug_forensik.gleitpfad.clone().expect("Gleitpfad");
        g.gesamt.expect("Werte")
    }

    /// Befund B: Touch-and-Go, 240 s spaeter die Landung. Der erste Anflug
    /// lag 1 Dot zu hoch, der zweite genau auf dem Pfad — er allein zaehlt.
    #[test]
    fn touch_and_go_schneidet_den_vorigen_anflug_ab() {
        let mut stats = FlightStats::default();
        stats.runway_nav_geometry = Some(nav_36());
        anflug_bis(&mut stats, td() - chrono::Duration::seconds(250), 0.35);
        // Wegsteigen nach dem Aufsetzen: die Episode wird zurueckgesetzt.
        clear_approach_stability_and_rollout(&mut stats);
        anflug_bis(&mut stats, td() - chrono::Duration::seconds(10), 0.0);

        anflug_forensik_stempeln(&mut stats, None, Some(td()), None);
        let ges = gesamt(&stats);
        assert!(ges.max_dots.abs() < 0.03, "{}", ges.max_dots);
        assert!(ges.proben <= 41, "nur der zweite Anflug: {}", ges.proben);

        // Gegenprobe: ohne Schnitt mischt sich der erste Anflug hinein.
        stats.anflug_forensik_ab = None;
        anflug_forensik_stempeln(&mut stats, None, Some(td()), None);
        let ges = gesamt(&stats);
        assert!(ges.max_dots > 0.9, "{}", ges.max_dots);
    }

    /// Befund B, Durchstart ohne Bodenberuehrung: `check_go_around` setzt
    /// den Schnitt.
    #[test]
    fn durchstart_schneidet_den_vorigen_anflug_ab() {
        let mut stats = FlightStats::default();
        stats.runway_nav_geometry = Some(nav_36());
        let ga = td() - chrono::Duration::seconds(240);
        anflug_bis(&mut stats, ga - chrono::Duration::seconds(20), 0.35);
        stats.lowest_agl_during_approach_ft = Some(250.0);
        stats.go_around_climb_pending_since = Some(ga - chrono::Duration::seconds(20));
        let mut snap = SimSnapshot::default();
        snap.altitude_agl_ft = 800.0;
        snap.vertical_speed_fpm = 1500.0;
        snap.engines_running = 2;
        snap.on_ground = false;
        let phase = check_go_around(&mut stats, &snap, ga);
        assert_eq!(phase, Some(FlightPhase::Climb));
        assert_eq!(stats.anflug_forensik_ab, Some(ga));

        anflug_bis(&mut stats, td() - chrono::Duration::seconds(10), 0.0);
        anflug_forensik_stempeln(&mut stats, None, Some(td()), None);
        let ges = gesamt(&stats);
        assert!(ges.max_dots.abs() < 0.03, "{}", ges.max_dots);
    }

    #[test]
    fn stempel_rechnet_gegen_die_gelandete_bahn_und_traegt_ins_analyse_json() {
        const M_JE_GRAD: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;
        let mut stats = FlightStats::default();
        stats.runway_nav_geometry = Some(
            serde_json::from_value(serde_json::json!({
                "designator": "36", "magnetic_course": 360.0, "true_course": 360.0,
                "length_ft": 9000,
                "threshold": {"lat": 50.0, "lon": 8.0, "elev_ft": 300},
                "end": {"lat": 50.03, "lon": 8.0, "elev_ft": 310},
                "ils": {"freq_mhz": 110.1, "course": 360.0, "category": 1},
                "glideslope_angle": 3.0, "tch_ft": 50
            }))
            .unwrap(),
        );
        // Anflug 1 Dot (0,35°) UEBER dem Pfad, 1000 → 200 ft.
        let ist = 3.35_f64.to_radians();
        let gpi = 50.0 / 3.0_f64.to_radians().tan();
        for i in 0..=40 {
            let h = 1000.0 - 20.0 * i as f64;
            let d_m = (h / ist.tan() - gpi) / 3.280_839_895;
            let mut snap = SimSnapshot::default();
            snap.lat = 50.0 - d_m / M_JE_GRAD;
            snap.lon = 8.0;
            snap.altitude_msl_ft = 300.0 + h;
            snap.altitude_agl_ft = h;
            snap.vertical_speed_fpm = -700.0;
            push_approach_sample(
                &mut stats,
                &snap,
                td() - chrono::Duration::seconds(50 - i as i64),
            );
        }
        stats.landing_analysis = Some(serde_json::json!({ "sample_count": 12 }));

        anflug_forensik_stempeln(&mut stats, None, Some(td()), None);

        let g = stats.anflug_forensik.gleitpfad.clone().expect("Gleitpfad");
        assert_eq!(g.quelle, "navigraph_ils");
        let ges = g.gesamt.expect("Werte");
        assert!((ges.max_dots - 1.0).abs() < 0.03, "{}", ges.max_dots);
        let analyse = stats.landing_analysis.as_ref().unwrap();
        assert_eq!(analyse["sample_count"], 12, "Bestand bleibt");
        assert_eq!(analyse["anflug_gleitpfad"]["quelle"], "navigraph_ils");

        // Durchstarten/Touch-and-Go: der naechste Anflug beginnt leer.
        clear_approach_stability_and_rollout(&mut stats);
        assert_eq!(
            stats.anflug_forensik,
            anflug_forensik::AnflugForensik::default()
        );
    }
