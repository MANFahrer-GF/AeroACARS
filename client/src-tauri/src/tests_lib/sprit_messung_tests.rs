    use super::*;

    /// Ein Flug nach EDDM (48.3538 N, 11.7861 E) mit TOD 142 NM vor dem Ziel.
    ///
    /// Die Fixe liegen auf einer Geraden westlich von EDDM, mit echten
    /// Abstaenden: Seit v1.7.36 wird der Plan-Anteil per Lotprojektion am ORT
    /// bestimmt, und fuenf Fixe auf derselben Koordinate ergaeben keinen
    /// Streckenzug. Bei 48,35° Breite ist eine Seemeile rund 0,0251°
    /// Laengengrad.
    fn stats_im_flug() -> FlightStats {
        let mut st = FlightStats::default();
        st.takeoff_fuel_kg = Some(40_919.0);
        st.takeoff_at = Some(Utc::now() - chrono::Duration::minutes(90));
        st.planned_arr_ref_pos = Some((48.3538, 11.7861));
        st.arr_airport_elevation_ft = Some(1_487.0);
        st.planned_cruise_alt_ft = Some(40_000.0);
        st.planned_waypoints = vec![
            // ident, Segmentlaenge zu diesem Fix, Plan-Sprit bis hier,
            // Laengengrad (Breite ueberall 48,3538), geplante Zeit ab Start.
            fix_ort("NOSLI", 37.0, 4_634.0, lon_nm_westlich(151.0), 1_200.0),
            fix_ort("TOD", 62.0, 19_353.0, lon_nm_westlich(142.0), 5_400.0),
            fix_ort("VESUB", 20.0, 19_546.0, lon_nm_westlich(122.0), 5_700.0),
            fix_ort("MIQ", 90.0, 20_388.0, lon_nm_westlich(32.0), 6_600.0),
            fix_ort("EDDM", 32.0, 21_218.0, 11.7861, 7_000.0),
        ];
        st.distance_nm = 600.0;
        st
    }

    /// Reiseflughoehe aufbauen, weit vor dem Plan-TOD.
    ///
    /// Ohne Historie gibt es keinen Sinkflug: `sprit_hoechste_hoehe_ft` wird
    /// beim ersten Tick auf die aktuelle Hoehe gesetzt, und `sinkt` misst
    /// gegen genau diesen Wert. Im echten Flug tickt der Client ab dem
    /// Abheben, hier muss man es hinschreiben.
    fn reiseflug_aufbauen(st: &mut FlightStats) {
        sprit_tick(
            st,
            48.3538,
            lon_nm_westlich(400.0),
            40_000.0,
            30_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
    }

    /// Laengengrad so viele Seemeilen westlich von EDDM.
    fn lon_nm_westlich(nm: f64) -> f64 {
        11.7861 - nm / (60.0 * 48.3538_f64.to_radians().cos())
    }

    /// Ein Navlog-Fix mit Ort und geplanter Zeit.
    fn fix_ort(ident: &str, nm: f32, kg: f32, lon: f64, zeit_s: f32) -> api_client::RouteFix {
        api_client::RouteFix {
            ident: ident.into(),
            lat: 48.3538,
            lon,
            kind: "wpt".into(),
            sprit_bis_hier_kg: Some(kg),
            segment_nm: Some(nm),
            hoehe_ft: Some(40_000.0),
            zeit_bis_hier_s: Some(zeit_s),
            ..Default::default()
        }
    }

    /// Die beiden Boden-Marken rasten an ein EREIGNIS, nicht an einen
    /// laufenden Wert — und „Triebwerke aus" erst nach einer Frist.
    #[test]
    fn sprit_triebwerk_sprit_rastet_an_das_ereignis() {
        let t0 = Utc::now();
        let t = |s: i64| t0 + chrono::Duration::seconds(s);
        let mut st = stats_im_flug();
        st.takeoff_fuel_kg = None;

        // Am Stand, Triebwerke aus, es wird noch getankt: nichts rastet.
        sprit_boden_marken(&mut st, 20_000.0, false, true, false, false, false, t(0));
        sprit_boden_marken(&mut st, 35_000.0, false, true, false, false, false, t(60));
        assert!(st.engine_start_fuel_kg.is_none(), "ohne Triebwerk gerastet");

        // Triebwerke an: JETZT. Genau dieser Wert, nicht der Hoechststand.
        sprit_boden_marken(&mut st, 41_644.0, true, true, false, false, false, t(120));
        assert_eq!(st.engine_start_fuel_kg, Some(41_644.0));
        sprit_boden_marken(&mut st, 41_200.0, true, true, false, false, false, t(300));
        assert_eq!(st.engine_start_fuel_kg, Some(41_644.0), "Marke verschoben");

        // Abgehoben, gelandet.
        st.takeoff_fuel_kg = Some(40_919.0);
        st.landing_fuel_kg = Some(16_770.0);

        // Rollen zum Stand. Ein Zappeln auf 0 fuer drei Sekunden darf NICHT
        // rasten (QS-Befund V1).
        sprit_boden_marken(&mut st, 16_700.0, false, true, false, false, false, t(1000));
        sprit_boden_marken(&mut st, 16_690.0, false, true, false, false, false, t(1003));
        sprit_boden_marken(&mut st, 16_680.0, true, true, false, false, false, t(1004));
        assert!(
            st.engine_off_fuel_kg.is_none(),
            "Zappeln hat „aus“ gerastet"
        );

        // Echtes Abstellen: erst nach der Frist.
        sprit_boden_marken(&mut st, 16_121.0, false, true, false, false, false, t(1200));
        assert!(
            st.engine_off_fuel_kg.is_none(),
            "vor Ablauf der Frist gerastet"
        );
        sprit_boden_marken(&mut st, 16_121.0, false, true, false, false, false, t(1211));
        assert_eq!(st.engine_off_fuel_kg, Some(16_121.0));

        // Danach laeuft nichts mehr weiter — APU am Gate, vergessener Client.
        sprit_boden_marken(&mut st, 15_000.0, false, true, false, false, false, t(3000));
        assert_eq!(st.engine_off_fuel_kg, Some(16_121.0), "Marke lief weiter");
    }

    /// Die Marken ruhen bei Pause, Slew, Replay-Verdacht und in der Luft.
    #[test]
    fn sprit_triebwerk_sprit_ruht_bei_slew_und_pause() {
        let setzen = |paused, slew, replay, boden| {
            let mut st = stats_im_flug();
            st.takeoff_fuel_kg = None;
            sprit_boden_marken(
                &mut st,
                41_644.0,
                true,
                boden,
                paused,
                slew,
                replay,
                Utc::now(),
            );
            st.engine_start_fuel_kg
        };
        assert!(setzen(true, false, false, true).is_none(), "Pause");
        assert!(setzen(false, true, false, true).is_none(), "Slew");
        assert!(
            setzen(false, false, true, true).is_none(),
            "Replay-Verdacht"
        );
        assert!(setzen(false, false, false, false).is_none(), "in der Luft");
        assert_eq!(
            setzen(false, false, false, true),
            Some(41_644.0),
            "Gegenprobe"
        );
    }

    /// **Ueber die echte Kette:** Aufsetzen friert die Auswertung ein, die
    /// Triebwerke gehen Minuten spaeter aus — und erst DANN ist der Flug
    /// vollstaendig. Die eingefrorene Auswertung muss den Rollsprit nach der
    /// Landung trotzdem tragen, und die vier Abschnitte muessen den Flug
    /// lueckenlos abdecken.
    ///
    /// Der Vorgaenger dieses Tests rief die Rechnung direkt auf und setzte
    /// die Marken von Hand — er umging das Einfrieren und war deshalb gruen,
    /// waehrend „Rollen nach der Landung" in keinem echten Flug ankam
    /// (QS-Befund F2, 18.09.2026).
    #[test]
    fn sprit_vier_abschnitte_ueber_die_echte_kette() {
        let t0 = Utc::now();
        let mut st = stats_im_flug();
        st.takeoff_fuel_kg = None;
        st.planned_burn_kg = Some(21_218.0);
        st.planned_taxi_kg = Some(998.0);
        st.planned_reserve_kg = Some(4_916.0);

        // Anlassen am Stand.
        sprit_boden_marken(&mut st, 41_644.0, true, true, false, false, false, t0);
        // Abheben (wie `latch_takeoff_stats` es setzt).
        st.takeoff_fuel_kg = Some(40_919.0);
        // Reiseflug, Sinkflug am Plan-TOD.
        reiseflug_aufbauen(&mut st);
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(142.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        st.distance_nm = 782.0;

        // Aufsetzen — durch den ECHTEN Stempel, der die Auswertung einfriert.
        let mut snap = crate::touchdown_metadata_stamp_tests::rollout_snap();
        snap.lat = 48.3538;
        snap.lon = 11.7861;
        snap.fuel_total_kg = 16_770.0;
        st.planned_arr_ref_pos = Some((48.3538, 11.7861));
        stamp_touchdown_metadata(&mut st, &snap, t0, None);
        let beim_aufsetzen = st.sprit_auswertung.clone().expect("eingefroren");
        assert!(
            beim_aufsetzen.rollen_nach_landung_kg.is_none(),
            "Vorbedingung: beim Aufsetzen gibt es noch keinen Rollsprit danach"
        );

        // Rollen zum Stand: Der Wegzaehler laeuft am Boden weiter.
        st.distance_nm += 4.0;
        // Und das OFP aendert sich noch (SimBrief-Refresh am Stand). Eine
        // vollstaendige Neubildung beim Abstellen truege das in die
        // Auswertung — das Nachtragen darf es nicht (QS Runde 3: jede der
        // beiden E2-Absicherungen braucht einen eigenen Biss).
        st.planned_burn_kg = Some(25_000.0);
        st.planned_reserve_kg = Some(6_000.0);
        // Triebwerke aus — nach der Frist.
        let aus = t0 + chrono::Duration::minutes(8);
        sprit_boden_marken(&mut st, 16_121.0, false, true, false, false, false, aus);
        sprit_boden_marken(
            &mut st,
            16_121.0,
            false,
            true,
            false,
            false,
            false,
            aus + chrono::Duration::seconds(11),
        );

        // Die EINGEFRORENE Auswertung traegt jetzt den Rollsprit.
        let a = st.sprit_auswertung.clone().expect("Auswertung");
        let nach = a
            .rollen_nach_landung_kg
            .expect("Rollen nach der Landung kam nie an");
        assert_eq!(nach, 649.0);
        // Und NUR dieses Feld ist neu: Alles andere steht, wie es beim
        // Aufsetzen gerechnet wurde — insbesondere die Anflugstrecke ohne
        // die 4 NM zum Stand (QS-Befund E2, 18.09.2026).
        let mut ohne_nachtrag = a.clone();
        ohne_nachtrag.rollen_nach_landung_kg = None;
        assert_eq!(
            ohne_nachtrag, beim_aufsetzen,
            "Nachtragen hat mehr als den Rollsprit geaendert"
        );
        assert!(
            beim_aufsetzen.strecke_anflug_nm.is_some(),
            "Vorbedingung: Anflugstrecke gemessen"
        );

        // Und die vier Abschnitte decken den Flug lueckenlos (Rundung je
        // Abschnitt: hoechstens 2 kg Summenfehler).
        let vor = a
            .rollen_vor_start
            .as_ref()
            .expect("Rollen vor Start")
            .ist_kg;
        let bis = a.bis_sinkflug.as_ref().expect("bis Sinkflug").ist_kg;
        let an = a.anflug.as_ref().expect("Anflug").ist_kg;
        let summe = vor + bis + an + nach;
        let gesamt = 41_644.0 - 16_121.0;
        assert!(
            (summe - gesamt).abs() <= 2.0,
            "Summe {summe} statt {gesamt} kg"
        );
    }

    /// Eine Zwischenlandung mit Abstellen beginnt einen NEUEN Abschnitt —
    /// die Marken des ersten duerfen nicht in den zweiten wandern.
    #[test]
    fn sprit_zweiter_abschnitt_nimmt_nicht_die_marken_des_ersten() {
        let mut st = stats_im_flug();
        st.engine_start_fuel_kg = Some(41_644.0);
        st.takeoff_fuel_kg = Some(40_919.0);
        st.landing_fuel_kg = Some(30_000.0);
        st.engine_off_fuel_kg = Some(29_800.0);
        // Zweites Abheben — durch den ECHTEN Latch, nicht die Hilfsfunktion.
        // Der Vorgaenger rief `sprit_neuer_abschnitt_beim_abheben` direkt auf
        // und blieb gruen, als deren Aufruf im Latch fehlte (QS-Befund E5).
        let mut snap = crate::touchdown_metadata_stamp_tests::rollout_snap();
        snap.fuel_total_kg = 29_700.0;
        latch_takeoff_stats(&mut st, &snap, Utc::now());
        assert!(
            st.engine_start_fuel_kg.is_none(),
            "Start-Marke aus Abschnitt 1 blieb stehen"
        );
        assert!(
            st.engine_off_fuel_kg.is_none(),
            "Abstell-Marke aus Abschnitt 1 blieb stehen"
        );

        // Gegenprobe: Beim ERSTEN Abheben (noch nie abgestellt) bleibt die
        // Start-Marke — sonst gaebe es nie ein „Rollen vor Start".
        let mut erst = stats_im_flug();
        erst.takeoff_fuel_kg = None;
        erst.engine_start_fuel_kg = Some(41_644.0);
        latch_takeoff_stats(&mut erst, &snap, Utc::now());
        assert_eq!(erst.engine_start_fuel_kg, Some(41_644.0));
    }

    /// Rundflug: SID und STAR liegen uebereinander. Eine Absenkung kurz
    /// nach dem Start darf NICHT auf ein STAR-Segment am Routenende
    /// projiziert werden (QS-Befund V3).
    #[test]
    fn sprit_projektion_rundflug_springt_nicht_ans_routenende() {
        // Hin und zurueck auf derselben Linie: A (0 NM) -> B (60 NM) -> A.
        let fx = |id: &str, nm: f32, kg: f32, lon: f64| api_client::RouteFix {
            ident: id.into(),
            lat: 48.0,
            lon,
            kind: "wpt".into(),
            sprit_bis_hier_kg: Some(kg),
            segment_nm: Some(nm),
            hoehe_ft: Some(20_000.0),
            zeit_bis_hier_s: None,
            ..Default::default()
        };
        let ost = |nm: f64| 11.0 + nm / (60.0 * 48.0_f64.to_radians().cos());
        let route = vec![
            fx("DEP", 0.0, 0.0, ost(0.0)),
            fx("WEND", 60.0, 1_000.0, ost(60.0)),
            fx("ARR", 60.0, 1_600.0, ost(0.0)),
        ];
        // Bei 10 NM hinaus, nach 12 NM geflogener Strecke: Hinweg, nicht Rueckweg.
        let ort = sprit_plan_am_ort(&route, 48.0, ost(10.0), Some(12.0)).expect("Projektion");
        assert!(
            ort.plan_rest_nm > 100.0,
            "auf den Rueckweg gesprungen: Rest {}",
            ort.plan_rest_nm
        );
        // Gegenprobe: nach 110 NM geflogener Strecke ist es der Rueckweg.
        let spaet = sprit_plan_am_ort(&route, 48.0, ost(10.0), Some(110.0)).expect("Projektion");
        assert!(
            spaet.plan_rest_nm < 20.0,
            "Rueckweg nicht erkannt: Rest {}",
            spaet.plan_rest_nm
        );
    }

    /// Rundflug, bei dem der Rueckweg an dieser Stelle ECHT naeher liegt als
    /// der Hinweg (mehr als der Gleichstand von 1 NM). Hier entscheidet die
    /// Plausibilitaetsregel allein — der Vorgaenger-Test hatte Hin- und
    /// Rueckweg exakt gleich nah, dort entschied schon der Gleichstand, und
    /// die Regel war ungeprueft (QS-Befund E5, 18.09.2026).
    #[test]
    fn sprit_projektion_regel_entscheidet_wenn_der_rueckweg_naeher_liegt() {
        let fx = |id: &str, nm: f32, kg: f32, lat: f64, lon: f64| api_client::RouteFix {
            ident: id.into(),
            lat,
            lon,
            kind: "wpt".into(),
            sprit_bis_hier_kg: Some(kg),
            segment_nm: Some(nm),
            hoehe_ft: Some(20_000.0),
            zeit_bis_hier_s: None,
            ..Default::default()
        };
        let ost = |nm: f64| 11.0 + nm / (60.0 * 48.0_f64.to_radians().cos());
        let nord = |nm: f64| 48.0 + nm / 60.0;
        // Hinweg auf 48,0°, Rueckweg 3 NM noerdlich davon.
        let route = vec![
            fx("DEP", 0.0, 0.0, nord(0.0), ost(0.0)),
            fx("WEND", 60.0, 1_000.0, nord(0.0), ost(60.0)),
            fx("WEND2", 3.0, 1_050.0, nord(3.0), ost(60.0)),
            fx("ARR", 60.0, 1_600.0, nord(3.0), ost(0.0)),
        ];
        // 10 NM hinaus, 2,5 NM noerdlich des Hinwegs: Rueckweg 0,5 NM,
        // Hinweg 2,5 NM entfernt. Geflogen: 12 NM.
        let (lat, lon) = (nord(2.5), ost(10.0));
        let ort = sprit_plan_am_ort(&route, lat, lon, Some(12.0)).expect("Projektion");
        assert!(
            ort.plan_rest_nm > 100.0,
            "auf den Rueckweg gesprungen: Rest {}",
            ort.plan_rest_nm
        );
        // Gegenprobe: Ohne die Regel gewinnt der naehere Rueckweg — die Regel
        // ist es, die hier entscheidet.
        let ohne = sprit_plan_am_ort_suche(&route, lat, lon, Some(12.0), 123.0, false)
            .expect("Projektion ohne Regel");
        assert!(
            ohne.plan_rest_nm < 20.0,
            "Gegenprobe: ohne Regel Rest {}",
            ohne.plan_rest_nm
        );
    }

    /// Untergezaehlter Wegzaehler (Sim-Rate, Neustart-Luecke): Der Flug darf
    /// seinen Messpunkt deshalb NICHT verlieren (QS-Befund E4, 18.09.2026).
    #[test]
    fn sprit_messpunkt_trotz_untergezaehlter_strecke() {
        let mut st = stats_im_flug();
        reiseflug_aufbauen(&mut st);
        // Der Wegzaehler sagt 5 NM — echte Strecke mehrere hundert.
        st.distance_nm = 5.0;
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(142.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert_eq!(
            st.sprit_vergleichspunkt_kg,
            Some(22_231.0),
            "Messpunkt verloren"
        );
        assert_eq!(st.sprit_plan_bis_vergleichspunkt_kg, Some(19_353.0));
        // Und ganz ohne Wegzaehler (0 nach Neustart) ebenso.
        let mut null = stats_im_flug();
        reiseflug_aufbauen(&mut null);
        null.distance_nm = 0.0;
        sprit_tick(
            &mut null,
            48.3538,
            lon_nm_westlich(142.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert_eq!(
            null.sprit_vergleichspunkt_kg,
            Some(22_231.0),
            "Messpunkt ohne Wegzaehler verloren"
        );
    }

    /// Ein Routen-Update NACH dem Messpunkt aendert den Anflug-Plan nicht —
    /// der Trip ist mit dem Plan-Anteil eingefroren (QS-Befund E3).
    #[test]
    fn sprit_routen_update_nach_dem_messpunkt_aendert_den_plan_nicht() {
        let mut st = stats_im_flug();
        reiseflug_aufbauen(&mut st);
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(142.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert_eq!(st.sprit_plan_trip_am_vergleichspunkt_kg, Some(21_218.0));
        // Route-Sync im Anflug: neuer Navlog mit anderem Trip.
        let mut neu = st.planned_waypoints.clone();
        if let Some(letzter) = neu.last_mut() {
            letzter.sprit_bis_hier_kg = Some(25_000.0);
        }
        st.planned_waypoints = neu;
        sprit_plan_neu_lesen(&mut st);
        st.landing_fuel_kg = Some(16_770.0);
        let a = sprit_auswertung_aus(&st, false);
        assert_eq!(
            a.anflug.as_ref().expect("Anflug").plan_kg,
            1_865.0,
            "21 218 − 19 353"
        );
    }

    /// Der Einstieg in der Luft (Airborne-Rescue): „bis Sinkflug" rechnet
    /// ab dem Einstiegsort, nicht ab dem Abflug — ueber den echten Latch.
    #[test]
    fn sprit_einstieg_in_der_luft_ueber_den_echten_latch() {
        let mut st = stats_im_flug();
        st.takeoff_fuel_kg = None;
        st.takeoff_at = None;
        // Einstieg 148 NM vor EDDM, zwischen NOSLI (151) und TOD (142).
        let mut snap = crate::touchdown_metadata_stamp_tests::rollout_snap();
        snap.lat = 48.3538;
        snap.lon = lon_nm_westlich(148.0);
        snap.on_ground = false;
        snap.fuel_total_kg = 30_000.0;
        latch_takeoff_stats_if_missing(&mut st, &snap, Utc::now());
        assert!(st.sprit_einstieg_in_der_luft);
        assert_eq!(st.takeoff_fuel_kg, Some(30_000.0));
        reiseflug_aufbauen(&mut st);
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(142.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        let ein = st.sprit_plan_bis_einstieg_kg.expect("Plan am Einstiegsort");
        // 4 634 + (19 353 − 4 634) · 3/9 — auf wenige kg, die Projektion
        // rechnet in der Ebene.
        assert!((ein - 9_540.0).abs() < 30.0, "Plan am Einstieg {ein}");
        st.landing_fuel_kg = Some(16_770.0);
        let bis = sprit_auswertung_aus(&st, false)
            .bis_sinkflug
            .expect("bis Sinkflug");
        assert_eq!(bis.ist_kg, 7_769.0, "30 000 − 22 231");
        assert!(
            (bis.plan_kg - (19_353.0 - ein).round()).abs() < 1.0,
            "Plan ab Einstieg: {}",
            bis.plan_kg
        );
        // Gegenprobe: ein echtes Abheben loescht den Einstieg.
        latch_takeoff_stats(&mut st, &snap, Utc::now());
        assert!(!st.sprit_einstieg_in_der_luft && st.sprit_einstieg_ort.is_none());
    }

    /// **Auto-Einreichen ueber die echte FSM:** Der Uebergang BlocksOn →
    /// Arrived haelt „Triebwerke aus" fest — auch wenn die Zehn-Sekunden-
    /// Frist noch laeuft. Das Auto-Einreichen haengt an genau diesem
    /// Uebergang, und danach laeuft kein Tick mehr (QS-Befund E1).
    #[test]
    fn sprit_ankommen_rastet_triebwerke_aus_vor_dem_einreichen() {
        let flight = crate::enroute_reconcile_replay_tests::replay_fixture();
        {
            let mut st = flight.stats.lock().unwrap();
            st.phase = FlightPhase::BlocksOn;
            st.blocks_on_reached = true;
            st.block_on_at = Some(Utc::now() - chrono::Duration::seconds(31));
            st.takeoff_fuel_kg = Some(40_919.0);
            st.landing_fuel_kg = Some(16_770.0);
            sprit_auswertung_einmal(&mut st).expect("eingefroren");
        }
        let snap = SimSnapshot {
            lat: 38.7646843,
            lon: -9.1368409,
            on_ground: true,
            parking_brake: true,
            groundspeed_kt: 0.0,
            engines_running: 0,
            fuel_total_kg: 16_121.0,
            ..SimSnapshot::default()
        };
        let change = step_flight_at(&flight, &snap, Utc::now());
        assert_eq!(
            change,
            Some(FlightPhase::Arrived),
            "Vorbedingung: FSM kommt an"
        );
        let st = flight.stats.lock().unwrap();
        assert_eq!(
            st.engine_off_fuel_kg,
            Some(16_121.0),
            "Triebwerke aus nicht festgehalten"
        );
        assert_eq!(
            st.sprit_auswertung
                .as_ref()
                .and_then(|a| a.rollen_nach_landung_kg),
            Some(649.0),
            "Rollsprit nicht in die eingefrorene Auswertung getragen"
        );
    }

    /// Manuelles Einreichen vor `Arrived`: Sind die Triebwerke gerade aus,
    /// rastet die Marke; laufen sie noch, bleibt der Wert ehrlich leer.
    #[test]
    fn sprit_einreichen_haelt_triebwerke_aus_nur_fest_wenn_sie_aus_sind() {
        let mut st = stats_im_flug();
        st.landing_fuel_kg = Some(16_770.0);
        sprit_auswertung_einmal(&mut st);
        sprit_flug_abschliessen(&mut st, Some(16_300.0), false);
        assert!(
            st.engine_off_fuel_kg.is_none(),
            "bei laufenden Triebwerken gerastet"
        );
        sprit_flug_abschliessen(&mut st, Some(16_300.0), true);
        assert_eq!(st.engine_off_fuel_kg, Some(16_300.0));
        assert_eq!(
            st.sprit_auswertung
                .as_ref()
                .and_then(|a| a.rollen_nach_landung_kg),
            Some(470.0)
        );

        // Und beide Einreich-Wege — Knopf und Formular — rufen es, BEVOR der
        // PIREP gebaut wird (QS Runde 3, F2: das Formular fehlte).
        const SRC: &str = include_str!("../lib.rs");
        for (anfang, schluss) in [
            ("async fn flight_end(", "async fn flight_end_manual("),
            (
                "async fn flight_end_manual(",
                "async fn flight_resume_check_position(",
            ),
        ] {
            let start = SRC.find(anfang).expect("Anfang");
            let ende = SRC[start..].find(schluss).expect("Ende") + start;
            let koerper = &SRC[start..ende];
            let ruf = koerper
                .find("sprit_flug_abschliessen(&mut stats")
                .unwrap_or_else(|| panic!("{anfang} ruft sprit_flug_abschliessen nicht"));
            let pirep = koerper
                .find("let now_for_flight_time")
                .expect("PIREP-Aufbau");
            assert!(
                ruf < pirep,
                "{anfang}: sprit_flug_abschliessen kommt erst nach dem PIREP-Aufbau"
            );
        }
        // Das Ankommen mit laufenden Triebwerken prueft ein Verhaltenstest:
        // `arrived_mit_laufenden_triebwerken_rastet_kein_abstellen`.
    }

    /// Die Phasen-Plananteile summieren sich auf den Navlog-Trip — auch
    /// wenn der OFP-Kopf einen anderen Trip nennt (Routen-Update, zwei
    /// Quellen). Sonst driftet der Schnitt zwischen den Phasen.
    #[test]
    fn sprit_phasen_summieren_sich_auf_den_navlog_trip() {
        let mut st = stats_im_flug();
        // OFP-Kopf nennt einen anderen Trip als das Navlog (21 218 kg).
        st.planned_burn_kg = Some(22_500.0);
        reiseflug_aufbauen(&mut st);
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(77.0),
            30_000.0,
            21_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        st.landing_fuel_kg = Some(19_000.0);
        let a = sprit_auswertung_aus(&st, false);
        let bis = a.bis_sinkflug.as_ref().expect("bis").plan_kg;
        let an = a.anflug.as_ref().expect("an").plan_kg;
        assert!(
            (bis + an - 21_218.0).abs() <= 1.0,
            "Phasen {bis} + {an} ergeben nicht den Navlog-Trip 21 218"
        );
    }

    /// Rollsprit-Grenzfaelle: kein Unsinn statt eines Wertes.
    #[test]
    fn sprit_rollen_grenzfaelle() {
        let basis = || landing_scoring::sprit::SpritEingang {
            takeoff_fuel_kg: Some(40_919.0),
            landing_fuel_kg: Some(16_770.0),
            planned_taxi_kg: Some(998.0),
            engine_start_fuel_kg: Some(41_644.0),
            engine_off_fuel_kg: Some(16_121.0),
            tank_plausibel: true,
            ..Default::default()
        };
        // Normalfall: beide da.
        let a = landing_scoring::sprit::auswerten(&basis());
        assert!(
            a.rollen_vor_start.is_some() && a.rollen_nach_landung_kg.is_some(),
            "Normalfall"
        );
        // Anlassen mit WENIGER als beim Abheben (nachgetankt): kein Wert.
        let a = landing_scoring::sprit::auswerten(&landing_scoring::sprit::SpritEingang {
            engine_start_fuel_kg: Some(40_000.0),
            ..basis()
        });
        assert!(
            a.rollen_vor_start.is_none(),
            "negativer Rollsprit angezeigt"
        );
        // Taxi-Plan unter 30 kg: Quote waere ohne Aussage.
        let a = landing_scoring::sprit::auswerten(&landing_scoring::sprit::SpritEingang {
            planned_taxi_kg: Some(20.0),
            ..basis()
        });
        assert!(
            a.rollen_vor_start.is_none(),
            "Quote gegen 20 kg Plan angezeigt"
        );
        // Abstellen mit MEHR als beim Landen: kein Wert.
        let a = landing_scoring::sprit::auswerten(&landing_scoring::sprit::SpritEingang {
            engine_off_fuel_kg: Some(17_000.0),
            ..basis()
        });
        assert!(
            a.rollen_nach_landung_kg.is_none(),
            "negativer Rollsprit nach der Landung"
        );
    }

    #[test]
    fn sprit_messung_plan_aus_navlog() {
        let st = stats_im_flug();
        let (rest, bis) = sprit_plan_aus_navlog(&st.planned_waypoints);
        assert_eq!(rest, Some(142.0));
        assert_eq!(bis, Some(19_353.0));
        // Altbestand ohne Felder: nichts, nie 0.
        //
        // Eine echte `<distance>0</distance>` ist dagegen eine ANGABE, keine
        // Fehlanzeige — zwei Fixe koennen auf derselben Position liegen. Sie
        // zaehlt deshalb zum Vollstaendigkeitsquorum und mit 0 NM zur Summe;
        // beides ist richtig.
        let alt = vec![api_client::RouteFix {
            ident: "TOD".into(),
            lat: 1.0,
            lon: 2.0,
            kind: "wpt".into(),
            ..Default::default()
        }];
        assert_eq!(sprit_plan_aus_navlog(&alt), (None, None));
    }

    /// Der Sinkflug darf beliebig spaet kommen — der Plan-Anteil wird am
    /// gemessenen ORT bestimmt, nicht am TOD-Fix.
    ///
    /// Das ist der Fall DLH 370 und damit ein Drittel des Bestands
    /// (Korpus-Pruefung 18.09.2026: 14 von 43 Fluegen, alle vier A380
    /// darunter). Solange Fenster UND Sinkflug im selben Tick gelten mussten,
    /// bekamen genau die Fluege mit langer Radarfuehrung — die, fuer die
    /// diese Auswertung gebaut wurde — ueberhaupt keinen Vergleichspunkt.
    #[test]
    fn sprit_messung_plan_am_ort_spaeter_sinkflug() {
        let mut st = stats_im_flug();
        reiseflug_aufbauen(&mut st);
        // 151 NM draussen, noch auf Reiseflughoehe: kein Sinkflug.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(151.0),
            40_000.0,
            25_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(
            st.sprit_vergleichspunkt_kg.is_none(),
            "ohne Sinkflug kein Punkt"
        );

        // 122 NM — also DEUTLICH hinter dem Plan-TOD (142 NM) — und jetzt
        // beginnt der Sinkflug. Frueher lag das unter 80 % und bekam gar
        // keinen Punkt.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(122.0),
            37_000.0,
            21_500.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert_eq!(
            st.sprit_vergleichspunkt_kg,
            Some(21_500.0),
            "spaeter Sinkflug ohne Punkt"
        );

        // Und der Plan-Anteil gehoert zu DIESEM Ort: VESUB liegt genau dort,
        // mit 19 546 kg bis hier und 122 NM Rest (90 + 32).
        assert_eq!(st.sprit_plan_bis_vergleichspunkt_kg, Some(19_546.0));
        let rest = st.sprit_plan_rest_ab_vergleichspunkt_nm.expect("Plan-Rest");
        assert!(
            (rest - 122.0).abs() < 0.5,
            "Plan-Reststrecke am Ort: {rest}"
        );
    }

    /// Zwischen zwei Fixen wird interpoliert — sonst spraenge der Plan-Anteil
    /// in Stufen und traefe nur zufaellig.
    #[test]
    fn sprit_messung_plan_am_ort_interpoliert_zwischen_fixen() {
        let mut st = stats_im_flug();
        reiseflug_aufbauen(&mut st);
        // Genau in der Mitte zwischen VESUB (122 NM, 19 546 kg) und
        // MIQ (32 NM, 20 388 kg): 77 NM vor dem Ziel.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(77.0),
            30_000.0,
            20_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        let bis = st.sprit_plan_bis_vergleichspunkt_kg.expect("Plan bis");
        let mitte = (19_546.0 + 20_388.0) / 2.0;
        assert!(
            (bis - mitte).abs() < 5.0,
            "Plan-Sprit in der Mitte: {bis} statt {mitte}"
        );
        let rest = st.sprit_plan_rest_ab_vergleichspunkt_nm.expect("Plan-Rest");
        assert!((rest - 77.0).abs() < 1.0, "Plan-Rest in der Mitte: {rest}");
    }

    /// Gegenprobe zur Projektion: Am TOD-Fix projiziert kommen exakt die
    /// alten Werte heraus.
    ///
    /// Ohne diese Zusicherung koennte die Projektion systematisch daneben
    /// liegen, und niemand saehe es — die Prozentwerte blieben ja plausibel.
    #[test]
    fn sprit_messung_plan_am_ort_trifft_am_tod_die_alten_werte() {
        let st = stats_im_flug();
        let (alt_rest, alt_bis) = sprit_plan_aus_navlog(&st.planned_waypoints);
        let ort = sprit_plan_am_ort(&st.planned_waypoints, 48.3538, lon_nm_westlich(142.0), None)
            .expect("Projektion am TOD");
        assert_eq!(
            ort.plan_bis_kg,
            alt_bis.expect("alt bis"),
            "Plan-Sprit am TOD"
        );
        let rest_alt = alt_rest.expect("alt rest");
        assert!(
            (ort.plan_rest_nm - rest_alt).abs() < 0.5,
            "Plan-Rest am TOD: {} statt {rest_alt}",
            ort.plan_rest_nm
        );
    }

    /// **Der Anker in Plan-Koordinaten.** Eine Zwischenabsenkung im
    /// Reiseflug darf den Vergleichspunkt NICHT ausloesen.
    ///
    /// # Warum das der gefaehrlichste Fall ist
    ///
    /// `sinkt` allein ist erfuellt, sobald es 1 500 ft unter die hoechste
    /// erreichte Hoehe geht — Verkehr, Turbulenz, ein Step-down von FL400 auf
    /// FL380. Ohne Anker schnappte der Punkt dort zu. Die PROZENTWERTE
    /// blieben dabei sogar richtig, weil die Projektion den Plan am selben
    /// Ort abliest — aber „Anflug" enthielte dann zwei Stunden Reiseflug,
    /// und `strecke_anflug_nm` staende bei 800 statt 182 NM. Ein Test, der
    /// nur Prozente prueft, sieht das nie.
    #[test]
    fn sprit_messung_anker_haelt_zwischenabsenkung_im_reiseflug_ab() {
        let mut st = stats_im_flug();
        // Reiseflughoehe erreichen.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(400.0),
            40_000.0,
            30_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        // 300 NM vor dem Ziel — also weit VOR dem Plan-TOD (142 NM) — sinkt
        // das Flugzeug wegen Verkehr von FL400 auf FL380.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(300.0),
            38_000.0,
            28_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(
            st.sprit_vergleichspunkt_kg.is_none(),
            "eine Zwischenabsenkung im Reiseflug hat den Punkt ausgeloest"
        );
        // Auch tiefer, solange wir vor dem Plan-TOD sind.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(200.0),
            34_000.0,
            26_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(
            st.sprit_vergleichspunkt_kg.is_none(),
            "immer noch vor dem Plan-TOD"
        );

        // Erst ab dem geplanten Sinkflugbeginn zaehlt es.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(140.0),
            33_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert_eq!(
            st.sprit_vergleichspunkt_kg,
            Some(22_231.0),
            "am Plan-TOD kein Punkt"
        );
    }

    /// Wer nicht auf der geplanten Route fliegt, bekommt keinen Plan-Anteil —
    /// er waere geraten.
    #[test]
    fn sprit_messung_abseits_der_route_kein_punkt() {
        let mut st = stats_im_flug();
        reiseflug_aufbauen(&mut st);
        // 140 NM vor dem Ziel, im Sinkflug — aber 60 NM neben der Route.
        let weit_ab = 48.3538 + 60.0 / 60.0;
        sprit_tick(
            &mut st,
            weit_ab,
            lon_nm_westlich(140.0),
            37_000.0,
            22_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(
            st.sprit_vergleichspunkt_kg.is_none(),
            "abseits der Route darf kein Plan-Anteil geraten werden"
        );
        // Zurueck auf der Route: jetzt schon.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(130.0),
            36_000.0,
            21_800.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert_eq!(st.sprit_vergleichspunkt_kg, Some(21_800.0));
    }

    #[test]
    fn sprit_messung_vergleichspunkt_im_fenster() {
        let mut st = stats_im_flug();
        // 300 NM vor dem Ziel, Reiseflug: noch nichts.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(300.0),
            40_000.0,
            25_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(st.sprit_vergleichspunkt_kg.is_none());
        // Am Plan-TOD (142 NM) und im Sinkflug: jetzt.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(140.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert_eq!(st.sprit_vergleichspunkt_kg, Some(22_231.0));
        assert_eq!(st.sprit_vergleichspunkt_odo_nm, Some(600.0));
        assert_eq!(st.sprit_plan_rest_nm, Some(142.0));
        assert_eq!(st.sprit_schwelle_ft, Some(9_487.0));
    }

    /// Wer spaet einsteigt, bekommt jetzt eine EHRLICHE Auswertung statt gar
    /// keiner.
    ///
    /// Bis v1.7.35 hiess dieser Test „Vergleichspunkt verpasst bleibt leer":
    /// Wer den Client erst 40 NM vor dem Ziel startete, bekam keine Phasen,
    /// weil der Plan-Anteil nur am TOD-Fix abgelesen werden konnte und
    /// „Abheben bis Punkt" dann gegen den kompletten Plan gerechnet haette.
    /// Seit der Plan AM ORT bestimmt wird, ist das kein Problem mehr — die
    /// Phase wird gegen die 40 NM gerechnet, die der Plan dort noch vorsah.
    #[test]
    fn sprit_messung_spaeter_einstieg_rechnet_gegen_den_plan_am_ort() {
        let mut st = stats_im_flug();
        st.planned_burn_kg = Some(21_218.0);
        reiseflug_aufbauen(&mut st);
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(40.0),
            20_000.0,
            18_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(
            st.sprit_vergleichspunkt_kg.is_some(),
            "spaeter Einstieg ohne Punkt"
        );
        let rest = st.sprit_plan_rest_ab_vergleichspunkt_nm.expect("Plan-Rest");
        assert!(
            (rest - 40.0).abs() < 1.0,
            "Plan-Rest am Einstiegsort: {rest}"
        );

        st.landing_fuel_kg = Some(16_770.0);
        let a = sprit_auswertung_aus(&st, false);
        let an = a.anflug.expect("Anflugphase");
        // Der Plan-Anteil fuer den Anflug ist `planned_burn − plan_bis_ort`.
        // Bei 40 NM vor dem Ziel liegt der Lotfusspunkt zwischen MIQ
        // (20 388 kg) und EDDM (21 218 kg), also rund 20 180 kg bis hier —
        // der Anflug-Plan ist damit ein kleiner Rest, nicht der volle
        // Sinkflug-Plan von 1 865 kg.
        assert!(
            an.plan_kg < 1_200.0,
            "Anflug-Plan gegen den ORT, nicht gegen den TOD: {} kg",
            an.plan_kg
        );
        // Gegenprobe: Mit der alten Rechnung (Plan vom TOD-Fix) waeren es
        // die vollen 1 865 kg gewesen.
        let alt_plan = st.planned_burn_kg.expect("burn") - st.sprit_plan_bis_tod_kg.expect("tod");
        assert!(
            (alt_plan - 1_865.0).abs() < 1.0,
            "Vorbedingung: der alte Plan-Anteil war 1 865 kg"
        );
        assert!(
            an.plan_kg < alt_plan,
            "der Ort muss einen kleineren Rest ergeben"
        );
    }

    #[test]
    fn sprit_messung_zeit_zaehlt_nur_tief_und_in_der_luft() {
        let mut st = stats_im_flug();
        st.sprit_vergleichspunkt_kg = Some(22_231.0);
        st.sprit_vergleichspunkt_odo_nm = Some(600.0);
        st.sprit_schwelle_ft = Some(9_487.0);
        let t = |st: &mut FlightStats, alt: f64, paused: bool, ground: bool, dt: Option<f64>| {
            sprit_tick(
                st, 48.3, 11.5, alt, 20_000.0, paused, false, false, ground, dt,
            )
        };
        t(&mut st, 12_000.0, false, false, Some(10.0)); // ueber Schwelle
        t(&mut st, 7_000.0, false, false, Some(10.0)); // zaehlt
        t(&mut st, 7_000.0, true, false, Some(10.0)); // Pause
        t(&mut st, 7_000.0, false, true, Some(10.0)); // Boden
        t(&mut st, 7_000.0, false, false, Some(900.0)); // Neustart-Luecke
        t(&mut st, 7_000.0, false, false, None); // kein dt
        t(&mut st, 7_000.0, false, false, Some(5.0)); // zaehlt
        assert_eq!(st.sprit_zeit_unter_schwelle_s, 15.0);
    }

    /// Ein Durchstart darf die Auswertung der ABGEBROCHENEN Landung nicht
    /// als Ergebnis der echten Landung hinterlassen (QS-Befund P1-2).
    #[test]
    fn sprit_durchstart_loescht_die_abgebrochene_auswertung() {
        let mut st = stats_im_flug();
        st.planned_burn_kg = Some(21_218.0);
        st.planned_reserve_kg = Some(4_916.0);
        // Reiseflug auf FL400 (setzt die Hoechsthoehe), dann im Sinkflug
        // auf 37 000 ft am Plan-TOD.
        reiseflug_aufbauen(&mut st);
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(142.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(
            st.sprit_vergleichspunkt_kg.is_some(),
            "Vergleichspunkt fehlt"
        );
        // Durchstart: Landesprit gesetzt, Auswertung eingefroren.
        st.landing_fuel_kg = Some(20_000.0);
        let beim_durchstart = sprit_auswertung_einmal(&mut st).expect("durchstart");
        assert!(
            beim_durchstart.reserve
                != landing_scoring::sprit::Reserve::NichtPruefbar {
                    grund: "kein_ofp".into()
                }
        );

        // Der Durchstart-Reset raeumt beides weg — und NUR der; der
        // Episoden-Reset fasst den Sprit bewusst nicht mehr an.
        landung_episode_zuruecksetzen(&mut st);
        assert!(
            st.landing_fuel_kg.is_some(),
            "Episoden-Reset darf den Sprit nicht anfassen"
        );
        sprit_durchstart_zuruecksetzen(&mut st);
        assert!(
            st.landing_fuel_kg.is_none(),
            "Landesprit des Durchstarts blieb stehen"
        );
        assert!(
            st.sprit_auswertung.is_none(),
            "Auswertung des Durchstarts blieb stehen"
        );

        // Der Vergleichspunkt gilt weiter — er lag auf dem Weg zum Ziel.
        assert_eq!(st.sprit_vergleichspunkt_kg, Some(22_231.0));

        // Und die echte Landung rechnet frisch.
        st.landing_fuel_kg = Some(16_770.0);
        let echt = sprit_auswertung_einmal(&mut st).expect("echte landung");
        assert_eq!(echt.landing_fuel_kg, Some(16_770.0));
        assert_ne!(echt, beim_durchstart);
    }

    /// Nach einem Durchstart misst `sprit_tick` weiter — vorher stieg es
    /// wegen des gesetzten Landesprits fuer immer aus.
    #[test]
    fn sprit_durchstart_misst_danach_weiter() {
        let mut st = stats_im_flug();
        st.sprit_vergleichspunkt_kg = Some(22_231.0);
        st.sprit_schwelle_ft = Some(9_487.0);
        st.landing_fuel_kg = Some(20_000.0);
        sprit_tick(
            &mut st,
            48.3,
            11.5,
            7_000.0,
            19_000.0,
            false,
            false,
            false,
            false,
            Some(10.0),
        );
        assert_eq!(
            st.sprit_zeit_unter_schwelle_s, 0.0,
            "mit Landesprit wird nicht gemessen"
        );
        sprit_durchstart_zuruecksetzen(&mut st);
        sprit_tick(
            &mut st,
            48.3,
            11.5,
            7_000.0,
            19_000.0,
            false,
            false,
            false,
            false,
            Some(10.0),
        );
        assert_eq!(
            st.sprit_zeit_unter_schwelle_s, 10.0,
            "nach dem Reset wieder"
        );
    }

    /// Die Messung ruht bei Slew und Replay-Verdacht — sonst koennte der
    /// Vergleichspunkt an einer Sprungposition gesetzt werden (QS-Befund P2-1).
    #[test]
    fn sprit_messung_ruht_bei_slew_und_replay() {
        let mut st = stats_im_flug();
        // Ein Ort, der im Normalfall einen Punkt setzen WUERDE: am Plan-TOD
        // und im Sinkflug. Genau das macht die Zusicherungen aussagekraeftig
        // — ohne diese Vorbedingung waeren sie auch ohne die Tore erfuellt.
        reiseflug_aufbauen(&mut st);
        let am_punkt =
            |st: &mut FlightStats, paused: bool, slew: bool, replay: bool, boden: bool| {
                sprit_tick(
                    st,
                    48.3538,
                    lon_nm_westlich(140.0),
                    37_000.0,
                    22_231.0,
                    paused,
                    slew,
                    replay,
                    boden,
                    Some(1.0),
                );
            };
        am_punkt(&mut st, false, true, false, false);
        assert!(st.sprit_vergleichspunkt_kg.is_none(), "Slew hat gemessen");
        am_punkt(&mut st, false, false, true, false);
        assert!(
            st.sprit_vergleichspunkt_kg.is_none(),
            "Replay-Verdacht hat gemessen"
        );
        am_punkt(&mut st, false, false, false, true);
        assert!(st.sprit_vergleichspunkt_kg.is_none(), "am Boden gemessen");
        am_punkt(&mut st, true, false, false, false);
        assert!(
            st.sprit_vergleichspunkt_kg.is_none(),
            "in der Pause gemessen"
        );
        // Gegenprobe: ohne Tor faellt der Punkt sofort.
        am_punkt(&mut st, false, false, false, false);
        assert_eq!(
            st.sprit_vergleichspunkt_kg,
            Some(22_231.0),
            "Normalfall ohne Punkt"
        );
    }

    /// Der Vergleichspunkt schnappt nicht im Steigflug zu, nur weil die
    /// Luftlinie kurz ist (kurzer Hopser mit langem Routing) — er braucht
    /// einen begonnenen Sinkflug (QS-Befund P2-2).
    ///
    /// Seit v1.7.36 traegt das die Plan-Koordinate: 100 NM Luftlinie sind
    /// hier NICHT 100 NM Plan-Reststrecke, und im Steigflug gibt es ohnehin
    /// keinen Sinkflug.
    #[test]
    fn sprit_vergleichspunkt_nicht_im_steigflug() {
        let mut st = stats_im_flug();
        // Direkt nach dem Abheben, noch im Steigflug auf 5 000 ft, aber
        // raeumlich schon nah am Ziel.
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(100.0),
            5_000.0,
            40_000.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        assert!(
            st.sprit_vergleichspunkt_kg.is_none(),
            "im Steigflug zugeschnappt"
        );
    }

    #[test]
    fn sprit_messung_schwelle_ueber_platz_und_unter_reiseflug() {
        let mut st = FlightStats::default();
        assert_eq!(sprit_schwelle_ft(&st), None);
        st.arr_airport_elevation_ft = Some(5_330.0); // Nairobi
        assert_eq!(sprit_schwelle_ft(&st), Some(13_330.0));
        st.planned_cruise_alt_ft = Some(9_500.0); // C172
        assert_eq!(sprit_schwelle_ft(&st), Some(8_500.0));
        st.planned_cruise_alt_ft = Some(5_500.0); // Platzrunde: keine Schwelle
        assert_eq!(sprit_schwelle_ft(&st), None);
    }

    #[test]
    fn sprit_messung_tank_plausibel() {
        let mut st = FlightStats::default();
        st.takeoff_fuel_kg = Some(100_000.0);
        st.landing_fuel_kg = Some(7.0); // KMRH→TXKF
        assert!(!sprit_tank_plausibel(&st));
        st.landing_fuel_kg = Some(16_770.0);
        assert!(sprit_tank_plausibel(&st));
        st.sprit_vergleichspunkt_kg = Some(150_000.0); // mehr als beim Start
        assert!(!sprit_tank_plausibel(&st));
    }

    #[test]
    fn sprit_messung_dlh370_ende_zu_ende() {
        let mut st = stats_im_flug();
        st.planned_burn_kg = Some(21_218.0);
        st.planned_block_fuel_kg = Some(41_644.0);
        st.planned_reserve_kg = Some(4_916.0);
        st.planned_taxi_kg = Some(998.0);
        st.planned_contingency_kg = Some(1_061.0);
        st.planned_alternate_burn_kg = Some(8_273.0);
        st.planned_extra_kg = Some(5_178.0);
        // Reiseflug auf FL400, dann im Sinkflug auf 37 000 ft — GENAU am
        // Plan-TOD (142 NM vor dem Ziel).
        //
        // Dort liefert die Ortsbestimmung dieselben Werte wie das frueher
        // uebliche Ablesen am TOD-Fix; die Golden-Zahlen dieses Fluges
        // bleiben also vergleichbar. Dass ein anderer Ort andere Werte
        // ergibt, pruefen `sprit_messung_plan_am_ort_*`.
        reiseflug_aufbauen(&mut st);
        sprit_tick(
            &mut st,
            48.3538,
            lon_nm_westlich(142.0),
            37_000.0,
            22_231.0,
            false,
            false,
            false,
            false,
            Some(1.0),
        );
        // Gegenprobe: Der Plan-Anteil am TOD ist der des TOD-Fixes.
        assert_eq!(st.sprit_plan_bis_vergleichspunkt_kg, Some(19_353.0));
        st.distance_nm = 782.0;
        st.sprit_zeit_unter_schwelle_s = 948.0;
        st.landing_fuel_kg = Some(16_770.0);
        let a = sprit_auswertung_aus(&st, false);
        assert_eq!(
            a.bis_sinkflug.as_ref().map(|p| p.abweichung_pct),
            Some(-3.4)
        );
        assert_eq!(a.anflug.as_ref().map(|p| p.abweichung_pct), Some(192.8));
        assert_eq!(a.strecke_anflug_nm, Some(182.0));
        assert_eq!(a.zeit_unter_schwelle_min, Some(15.8));
        assert_eq!(a.badge, landing_scoring::sprit::Badge::Gruen);
        // Ohne Anlass-Marke gilt der Rueckfall „Abheben plus Plan-Taxi" als
        // Tank beim Anlassen (41 917 kg) — daher 1 870, nicht die 1 597 des
        // Fluges mit Marke (siehe `sprit.rs`, Leiter).
        assert_eq!(a.extra_genutzt_kg, Some(1_870.0));
    }
