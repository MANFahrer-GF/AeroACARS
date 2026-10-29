    use super::*;
    use sim_core::process_probe::ProcessLiveness;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// GAF 278, letzter Punkt vor dem Absturz (Recorder, 17:51:12 UTC).
    fn gaf278_vorher() -> LageReferenz {
        LageReferenz {
            phase: FlightPhase::Descent,
            lat: 46.2368800281314,
            lon: 16.4724032106752,
            alt_ft: Some(32_313.0),
            fuel_kg: Some(5_822.0),
            on_ground: false,
            gs_kt: Some(453.0),
            at: Some(utc("2026-10-24T17:51:12Z")),
            aufgesetzt: None,
        }
    }

    fn sim(lat: f64, lon: f64, msl: f64, agl: f64, on_ground: bool, fuel: f32) -> SimSnapshot {
        SimSnapshot {
            lat,
            lon,
            altitude_msl_ft: msl,
            altitude_agl_ft: agl,
            on_ground,
            fuel_total_kg: fuel,
            ..SimSnapshot::default()
        }
    }

    /// Erster Wert nach dem Laden: Zagreb, Platzhoehe, meldet noch „in der Luft".
    #[test]
    fn gaf278_erster_wert_nach_dem_laden_haelt_an() {
        let jetzt = sim(45.7490476111413, 16.080041805556, 354.0, 1.0, false, 11_800.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:07:13Z"), true),
            Some(LageAbweichung::AmBoden)
        );
    }

    #[test]
    fn gaf278_am_boden_haelt_an() {
        let jetzt = sim(45.7490476111413, 16.080041805556, 366.0, 0.0, true, 11_800.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:07:19Z"), true),
            Some(LageAbweichung::AmBoden)
        );
    }

    /// Neu geladen in der Luft, aber mit vollem Tank: haelt an.
    #[test]
    fn sprit_in_der_luft_gestiegen_haelt_an() {
        let jetzt = sim(46.2369, 16.4724, 32_000.0, 31_600.0, false, 11_800.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:07:13Z"), true),
            Some(LageAbweichung::SpritGestiegen)
        );
    }

    /// Sim-Prozess war weg: Der Flieger konnte nicht weiterfliegen — 34 nm
    /// in der Luft sind „weit weg".
    #[test]
    fn sim_abgestuerzt_und_weit_weg_haelt_an() {
        let jetzt = sim(45.7490, 16.0800, 32_000.0, 31_600.0, false, 5_800.0);
        assert!(matches!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:07:13Z"), false),
            Some(LageAbweichung::ZuWeitWeg { erlaubt_nm, .. }) if erlaubt_nm == RESUME_DRIFT_CLEAN_NM
        ));
    }

    /// Gegenprobe: nur die Datenverbindung war weg, der Sim flog weiter —
    /// 34 nm in 16 Minuten bei 453 kt sind erreichbar.
    #[test]
    fn verbindung_weg_sim_flog_weiter_laeuft_von_selbst() {
        let jetzt = sim(45.7490, 16.0800, 12_000.0, 11_600.0, false, 5_300.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:07:13Z"), true),
            None
        );
    }

    /// Gegenprobe: 60 s Verbindungsluecke bei 450 kt, 7,5 nm weiter.
    #[test]
    fn kurze_luecke_im_reiseflug_laeuft_von_selbst() {
        let jetzt = sim(46.3618, 16.4724, 32_000.0, 31_600.0, false, 5_810.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T17:52:12Z"), true),
            None
        );
    }

    /// Gegenprobe: Holding — nach 10 Minuten 3 nm vom letzten Punkt.
    #[test]
    fn holding_kurve_laeuft_von_selbst() {
        let jetzt = sim(46.2868, 16.4724, 32_000.0, 31_600.0, false, 5_500.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:01:12Z"), true),
            None
        );
    }

    /// Gegenprobe: Esc-Pause 10 Minuten, dieselbe Stelle.
    #[test]
    fn sim_pause_an_derselben_stelle_laeuft_von_selbst() {
        let jetzt = sim(46.2369, 16.4724, 32_313.0, 31_900.0, false, 5_822.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:01:12Z"), false),
            None
        );
    }

    /// In der Pause kann der Flieger nicht weiter: 20 nm sind zu weit.
    #[test]
    fn sim_pause_und_versetzt_haelt_an() {
        let jetzt = sim(46.57, 16.4724, 32_313.0, 31_900.0, false, 5_822.0);
        assert!(matches!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T18:01:12Z"), false),
            Some(LageAbweichung::ZuWeitWeg { .. })
        ));
    }

    /// Gegenprobe: am Boden nachgetankt, vorher und nachher am Boden.
    #[test]
    fn nachtanken_am_boden_laeuft_von_selbst() {
        let vorher = LageReferenz {
            phase: FlightPhase::Boarding,
            alt_ft: Some(600.0),
            on_ground: true,
            gs_kt: Some(0.0),
            ..gaf278_vorher()
        };
        let jetzt = sim(46.2369, 16.4724, 600.0, 0.0, true, 11_800.0);
        assert_eq!(
            lage_abweichung(&vorher, &jetzt, utc("2026-10-24T18:01:12Z"), true),
            None
        );
    }

    /// Ladebild-Scheinposition zaehlt nie als „passt".
    #[test]
    fn menue_scheinposition_haelt_an() {
        let jetzt = sim(-0.0, 90.0003, 228.0, 0.0, false, 5_822.0);
        assert_eq!(
            lage_abweichung(&gaf278_vorher(), &jetzt, utc("2026-10-24T17:52:12Z"), true),
            Some(LageAbweichung::KeinEchterWert)
        );
    }

    #[test]
    fn weiterfliegen_nur_ohne_pause_und_mit_laufendem_sim() {
        let mut stats = FlightStats::default();
        assert!(darf_weitergeflogen_sein(&stats), "App-Neustart: unbekannt, darf");
        stats.current_pause_reason = Some(PauseReason::SimDisconnect);
        stats.disconnect_sim_liveness = Some(ProcessLiveness::Alive);
        assert!(darf_weitergeflogen_sein(&stats));
        stats.disconnect_sim_liveness = Some(ProcessLiveness::Gone);
        assert!(!darf_weitergeflogen_sein(&stats), "Sim-Prozess weg");
        stats.disconnect_sim_liveness = None;
        stats.current_pause_reason = Some(PauseReason::SimPause);
        assert!(!darf_weitergeflogen_sein(&stats), "Sim-Pause");
    }

    /// Die Referenz kommt aus dem Stand vor der Pause — Sprit und Bodenlage
    /// aus `paused_last_known`, Zeit und Tempo aus `last_known_*`.
    #[test]
    fn referenz_aus_dem_pausenstand() {
        let mut stats = FlightStats::default();
        stats.phase = FlightPhase::Descent;
        stats.last_known_lat = Some(46.2369);
        stats.last_known_lon = Some(16.4724);
        stats.last_known_alt_ft = Some(32_313.0);
        stats.last_known_gs_kt = Some(453.0);
        stats.last_known_at = Some(utc("2026-10-24T17:51:12Z"));
        stats.paused_last_known = Some(PausedSnapshot {
            lat: 46.2369,
            lon: 16.4724,
            heading_deg: 320.0,
            altitude_ft: 32_313.0,
            fuel_total_kg: 5_822.0,
            zfw_kg: None,
            on_ground: false,
        });
        let r = lage_referenz(&stats).expect("Referenz");
        assert_eq!(r.fuel_kg, Some(5_822.0));
        assert_eq!(r.gs_kt, Some(453.0));
        assert!(!r.on_ground);
        let jetzt = sim(45.7490, 16.0800, 354.0, 1.0, false, 11_800.0);
        assert_eq!(
            lage_abweichung(&r, &jetzt, utc("2026-10-24T18:07:13Z"), true),
            Some(LageAbweichung::AmBoden)
        );
    }

    /// Der Absturz-Detektor im laufenden Flug bekommt „am Boden" auch fuer
    /// den Ladebild-Wert auf Platzhoehe.
    #[test]
    fn absturz_detektor_mit_ladebild_wert() {
        let jetzt = sim(45.7490476111413, 16.080041805556, 354.0, 1.0, false, 11_800.0);
        assert!(is_mid_session_sim_crash_recovery(
            FlightPhase::Descent,
            false,
            32_313.0,
            am_boden_fuer_absturzerkennung(&jetzt),
            jetzt.altitude_msl_ft,
        ));
    }
