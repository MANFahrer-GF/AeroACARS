    //! LTU 154 (Michael K, 26.10.2026, EDDF 25R): echte Werte aus Flug- und
    //! Diagnoseprotokoll 7pyj7BezQarnok2V. GAF 278 bleibt die Gegenprobe.
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// Letzter Punkt vor der Pause: 17:19:31.127, Final, 11,5 ft AGL.
    const VORHER: (f64, f64) = (50.043669717982496, 8.524638167352773);
    /// Erster Wert danach (17:19:37): am Boden, 0,2 nm weiter.
    const JETZT: (f64, f64) = (50.0425, 8.5201);

    fn ltu154_stats() -> FlightStats {
        let mut st = FlightStats::default();
        st.phase = FlightPhase::Final;
        st.last_known_lat = Some(VORHER.0);
        st.last_known_lon = Some(VORHER.1);
        st.last_known_alt_ft = Some(346.45);
        st.last_known_gs_kt = Some(121.4);
        st.last_known_at = Some(utc("2026-10-26T17:19:31.127Z"));
        st.current_pause_reason = Some(PauseReason::SimPause);
        st.paused_last_known = Some(PausedSnapshot {
            lat: VORHER.0,
            lon: VORHER.1,
            heading_deg: 247.07,
            altitude_ft: 346.45,
            fuel_total_kg: 1_363.8,
            zfw_kg: Some(76_492.0),
            on_ground: false,
        });
        // Vom Abtaster bestaetigt (17:19:32.27, Aufsetzen 17:19:31.158).
        st.sampler_touchdown_at = Some(utc("2026-10-26T17:19:31.158Z"));
        st.landing_lat = Some(50.04366);
        st.landing_lon = Some(8.52459);
        st.landing_heading_true_deg = Some(247.07);
        st
    }

    fn sim_am_boden(lat: f64, lon: f64, fuel: f32) -> SimSnapshot {
        SimSnapshot {
            lat,
            lon,
            altitude_msl_ft: 340.0,
            altitude_agl_ft: 10.5,
            on_ground: true,
            fuel_total_kg: fuel,
            ..SimSnapshot::default()
        }
    }

    fn urteil(st: &FlightStats, jetzt: &SimSnapshot) -> Option<LageAbweichung> {
        lage_abweichung(
            &lage_referenz(st).unwrap(),
            jetzt,
            utc("2026-10-26T17:19:37Z"),
            darf_weitergeflogen_sein(st),
        )
    }

    #[test]
    fn ltu154_landung_in_der_pause_laeuft_von_selbst() {
        let st = ltu154_stats();
        assert_eq!(urteil(&st, &sim_am_boden(JETZT.0, JETZT.1, 1_362.0)), None);
    }

    /// Gegenprobe: ohne bestaetigtes Aufsetzen bleibt es beim Anhalten.
    #[test]
    fn ohne_aufsetzen_bleibt_am_boden_ein_halt() {
        let mut st = ltu154_stats();
        st.sampler_touchdown_at = None;
        assert_eq!(
            urteil(&st, &sim_am_boden(JETZT.0, JETZT.1, 1_362.0)),
            Some(LageAbweichung::AmBoden)
        );
    }

    /// Ein Aufsetzer Minuten vor der Luecke (Durchstarten) zaehlt nicht.
    #[test]
    fn altes_aufsetzen_vor_der_luecke_zaehlt_nicht() {
        let mut st = ltu154_stats();
        st.sampler_touchdown_at = Some(utc("2026-10-26T17:12:00Z"));
        assert_eq!(aufsetzen_der_luecke(&st, VORHER.0, VORHER.1), None);
        assert_eq!(
            urteil(&st, &sim_am_boden(JETZT.0, JETZT.1, 1_362.0)),
            Some(LageAbweichung::AmBoden)
        );
    }

    /// Ein Aufsetzpunkt weit vom letzten Punkt gehoert nicht zur Luecke.
    #[test]
    fn aufsetzen_weit_vom_letzten_punkt_zaehlt_nicht() {
        let mut st = ltu154_stats();
        st.landing_lat = Some(50.5);
        assert_eq!(aufsetzen_der_luecke(&st, VORHER.0, VORHER.1), None);
    }

    /// Neu geladen auf einem anderen Platz, 30 nm weiter: haelt an, auch
    /// mit Aufsetzen in der Luecke.
    #[test]
    fn neu_geladen_weit_weg_haelt_trotz_aufsetzen_an() {
        let st = ltu154_stats();
        assert_eq!(
            urteil(&st, &sim_am_boden(50.5, 8.52, 1_362.0)),
            Some(LageAbweichung::AmBoden)
        );
    }

    /// Neu geladen mit vollem Tank an derselben Stelle: Sprit faellt auf.
    #[test]
    fn neu_geladen_mit_vollem_tank_haelt_an() {
        let st = ltu154_stats();
        assert_eq!(
            urteil(&st, &sim_am_boden(JETZT.0, JETZT.1, 9_000.0)),
            Some(LageAbweichung::SpritGestiegen)
        );
    }

    /// GAF 278: Absturz auf FL323, danach am Boden in Zagreb. Ein dort
    /// gestempeltes „Aufsetzen" liegt zeitlich und raeumlich ausserhalb.
    #[test]
    fn gaf278_bleibt_ein_halt() {
        let mut st = FlightStats::default();
        st.phase = FlightPhase::Descent;
        st.last_known_lat = Some(46.2368800281314);
        st.last_known_lon = Some(16.4724032106752);
        st.last_known_alt_ft = Some(32_313.0);
        st.last_known_gs_kt = Some(453.0);
        st.last_known_at = Some(utc("2026-10-24T17:51:12Z"));
        st.sampler_touchdown_at = Some(utc("2026-10-24T18:07:13Z"));
        st.landing_lat = Some(45.7490476111413);
        st.landing_lon = Some(16.080041805556);
        let jetzt = SimSnapshot {
            lat: 45.7490476111413,
            lon: 16.080041805556,
            altitude_msl_ft: 366.0,
            on_ground: true,
            fuel_total_kg: 11_800.0,
            ..SimSnapshot::default()
        };
        assert_eq!(
            lage_abweichung(&lage_referenz(&st).unwrap(), &jetzt, utc("2026-10-24T18:07:19Z"), true),
            Some(LageAbweichung::AmBoden)
        );
    }

    /// Codex 26.10.: Ein Aufsetzen, das erst nach der Unterbrechung entsteht
    /// (Neuladen nahe der Bahn), gehoert nicht zur Luecke.
    #[test]
    fn aufsetzen_erst_nach_der_luecke_zaehlt_nicht() {
        let mut st = ltu154_stats();
        st.sampler_touchdown_at = Some(utc("2026-10-26T17:20:15Z"));
        assert_eq!(aufsetzen_der_luecke(&st, VORHER.0, VORHER.1), None);
        assert_eq!(
            urteil(&st, &sim_am_boden(JETZT.0, JETZT.1, 1_362.0)),
            Some(LageAbweichung::AmBoden)
        );
    }

    #[test]
    fn laufende_aufsetzpruefung_haelt_das_urteil_zurueck() {
        let mut st = FlightStats::default();
        let t = utc("2026-10-26T17:19:31.158Z");
        assert!(!aufsetzen_wird_geprueft(&st, t));
        st.pending_td_at = Some(t);
        assert!(aufsetzen_wird_geprueft(&st, t + chrono::Duration::milliseconds(1_100)));
        assert!(
            !aufsetzen_wird_geprueft(&st, t + chrono::Duration::seconds(AUFSETZ_PRUEFUNG_ABWARTEN_S)),
            "ein haengender Kandidat sperrt nicht fuer immer"
        );
    }
