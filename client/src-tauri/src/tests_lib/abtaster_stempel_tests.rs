    //! LTU 154 (26.10.2026): Der FSM-Landing-Arm lief erst 292 s nach dem
    //! Aufsetzen und stempelte das stehende Flugzeug — 07L statt 25R. Die
    //! Kette laeuft hier ueber `step_flight_at`, nicht ueber den Helfer.
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    const TD: (f64, f64) = (50.04366, 8.52459);

    /// Final, vom Abtaster gestempelt — wie um 17:19:32.
    fn gestempelter_flug() -> (ActiveFlight, DateTime<Utc>) {
        let flight = crate::enroute_reconcile_replay_tests::replay_fixture();
        let td_at = utc("2026-10-26T17:19:31.158Z");
        {
            let mut st = flight.stats.lock().unwrap();
            st.phase = FlightPhase::Final;
            st.was_airborne = true;
            st.was_on_ground = Some(false);
            st.takeoff_at = Some(td_at - chrono::Duration::hours(2));
            st.sampler_touchdown_at = Some(td_at);
            st.sampler_touchdown_vs_fpm = Some(-191.1);
            st.landing_lat = Some(TD.0);
            st.landing_lon = Some(TD.1);
            st.landing_heading_true_deg = Some(247.07);
            st.landing_fuel_kg = Some(1_363.8);
            st.landing_fuel_puffer = true;
            st.landing_weight_kg = Some(77_856.0);
            st.rollout_distance_m = Some(0.0);
            st.rollout_last_lat = Some(TD.0);
            st.rollout_last_lon = Some(TD.1);
        }
        (flight, td_at)
    }

    /// Das stehende Flugzeug um 17:24:23 (Flugprotokoll).
    fn steht(gs_kt: f32) -> SimSnapshot {
        SimSnapshot {
            lat: 50.040807569984544,
            lon: 8.521866427582312,
            altitude_msl_ft: 343.4,
            altitude_agl_ft: 10.57,
            heading_deg_true: 69.64,
            heading_deg_magnetic: 66.6,
            groundspeed_kt: gs_kt,
            on_ground: true,
            fuel_total_kg: 1_279.0,
            total_weight_kg: Some(77_771.0),
            engines_running: 2,
            simulator: Simulator::Msfs2024,
            ..SimSnapshot::default()
        }
    }

    fn probe(at: DateTime<Utc>, lat: f64, lon: f64, hdg: f32, fuel: f32) -> TelemetrySample {
        TelemetrySample {
            at,
            on_ground: true,
            lat,
            lon,
            heading_true_deg: hdg,
            fuel_kg: fuel,
            ..TelemetrySample::default()
        }
    }

    #[test]
    fn nach_der_pause_bleibt_der_stempel_des_abtasters() {
        let (flight, td_at) = gestempelter_flug();
        let jetzt = td_at + chrono::Duration::seconds(292);
        // Der Puffer haelt nur noch Proben von nach der Pause.
        flight.stats.lock().unwrap().snapshot_buffer.push_back(probe(
            jetzt,
            50.0408,
            8.5219,
            69.6,
            1_279.0,
        ));
        step_flight_at(&flight, &steht(1.2), jetzt);
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.phase, FlightPhase::Landing);
        assert_eq!((st.landing_lat, st.landing_lon), (Some(TD.0), Some(TD.1)));
        assert_eq!(st.landing_heading_true_deg, Some(247.07));
        assert_eq!(st.landing_fuel_kg, Some(1_363.8));
        assert_eq!(st.landing_weight_kg, Some(77_856.0));
        assert_eq!((st.rollout_last_lat, st.rollout_last_lon), (Some(TD.0), Some(TD.1)));
        assert_eq!(st.landing_at, Some(td_at));
    }

    /// Auch mit leerem Puffer (geleert waehrend der Pause): kein Stempel aus
    /// dem stehenden Flugzeug.
    #[test]
    fn leerer_puffer_stempelt_nicht_aus_dem_schnappschuss() {
        let (flight, td_at) = gestempelter_flug();
        step_flight_at(&flight, &steht(1.2), td_at + chrono::Duration::seconds(292));
        let st = flight.stats.lock().unwrap();
        assert_eq!((st.landing_lat, st.landing_lon), (Some(TD.0), Some(TD.1)));
        assert_eq!(st.landing_heading_true_deg, Some(247.07));
    }

    /// Gegenprobe Normalfall: Der Puffer haelt den Aufsetzmoment — der Arm
    /// stempelt wie bisher aus dem Puffer neu.
    #[test]
    fn puffer_am_aufsetzmoment_stempelt_wie_bisher() {
        let (flight, td_at) = gestempelter_flug();
        let probe_td = probe(td_at + chrono::Duration::milliseconds(80), 50.04365, 8.52455, 247.1, 1_363.5);
        flight.stats.lock().unwrap().snapshot_buffer.push_back(probe_td);
        let mut snap = steht(110.0);
        snap.lat = 50.0435;
        snap.lon = 8.5238;
        snap.heading_deg_true = 247.1;
        step_flight_at(&flight, &snap, td_at + chrono::Duration::seconds(1));
        let st = flight.stats.lock().unwrap();
        assert_eq!((st.landing_lat, st.landing_lon), (Some(50.04365), Some(8.52455)));
        assert_eq!(st.landing_fuel_kg, Some(1_363.5));
        assert_eq!((st.rollout_last_lat, st.rollout_last_lon), (Some(50.0435), Some(8.5238)));
    }

    /// Gegenprobe Hopser (Codex 26.10.): Die erste Bodenprobe liegt VOR dem
    /// bestaetigten Aufsetzen — keine Luecke, der Arm stempelt wie bisher.
    #[test]
    fn hopser_davor_stempelt_wie_bisher() {
        let (flight, td_at) = gestempelter_flug();
        let erster_kontakt =
            probe(td_at - chrono::Duration::seconds(3), 50.0438, 8.5255, 247.0, 1_365.0);
        flight.stats.lock().unwrap().snapshot_buffer.push_back(erster_kontakt);
        let mut snap = steht(110.0);
        snap.lat = 50.0435;
        snap.lon = 8.5238;
        step_flight_at(&flight, &snap, td_at + chrono::Duration::seconds(1));
        let st = flight.stats.lock().unwrap();
        assert_eq!((st.landing_lat, st.landing_lon), (Some(50.0438), Some(8.5255)));
    }

    /// Gegenprobe: Ohne Stempel des Abtasters (FSM zuerst, Resume nach
    /// App-Neustart) stempelt der Arm wie bisher.
    #[test]
    fn ohne_abtaster_stempelt_der_arm() {
        let (flight, td_at) = gestempelter_flug();
        {
            let mut st = flight.stats.lock().unwrap();
            st.sampler_touchdown_at = None;
        }
        step_flight_at(&flight, &steht(1.2), td_at + chrono::Duration::seconds(292));
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.landing_lat, Some(50.040807569984544));
    }

    /// v3 (Kap. 5.2): Die gemessene Vref wird beim Aufsetzen verbraucht
    /// (`vref_gemessen.take()`), bleibt aber fuer `herkunft.vref` stehen —
    /// ueber die echte Kette `step_flight_at`. Gegenprobe: ohne Messung
    /// bleibt sie leer.
    #[test]
    fn gemessene_vref_steht_in_der_herkunft() {
        for (gemessen, erwartet) in [(Some((137.0_f32, "pmdg")), serde_json::json!(137.0)), (None, serde_json::Value::Null)] {
            let (flight, td_at) = gestempelter_flug();
            flight.stats.lock().unwrap().vref_gemessen = gemessen;
            step_flight_at(&flight, &steht(1.2), td_at + chrono::Duration::seconds(292));
            let st = flight.stats.lock().unwrap();
            assert_eq!(st.phase, FlightPhase::Landing);
            assert!(st.vref_gemessen.is_none(), "beim Aufsetzen verbraucht");
            assert_eq!(st.landing_vref_gemessen, gemessen);
            let e = landungs_bewertungseingabe(&flight, &st, Some("A320"), "EDDF");
            let h = landungs_herkunft(&flight, &st, &e, "EDDF");
            assert_eq!(h["vref"]["gemessen_kt"], erwartet);
            assert_eq!(h["vref"]["gemessen_quelle"], serde_json::json!(gemessen.map(|(_, q)| q)));
        }
    }
