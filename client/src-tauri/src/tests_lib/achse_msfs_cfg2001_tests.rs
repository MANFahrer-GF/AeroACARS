    //! P-4.14 Korpustest: Joel, CFG2001 EDDN–FQNP (A339, MSFS, Client
    //! 2.1.24, 30.10.2026), Landung auf Bahn 23.
    //!
    //! 2.1.24 legte die Mittellinie bei MSFS aus Navigraph. Navigraph führt
    //! FQNP 05/23 auf ganze Bogensekunden; die Achse lag 15,6 m (Schwelle
    //! 23) bis 17,1 m (Schwelle 05) neben der Bahn. Gespeichert: „neben der
    //! Bahn", Bahndisziplin 20, Ausrichtung 65, Note 25. Richtig ist 58 (D).
    //!
    //! # Die Kette
    //!
    //! Echter Client-Code von der Szenerie bis zur Note:
    //! `sim_msfs::facility::bahn_paar` (wie der Adapter die Auskunft baut) →
    //! `uebernimm_szenerie` → `lookup_runway_in_nav` → `bahndisziplin_tick`
    //! je Position aus dem Flugprotokoll → `scoring_eingang` +
    //! `fill_v2_rollout_fields` → `bewertung::bewerten` (WASM-Modul).
    //!
    //! Was nicht an der Bahn hängt (Sinkrate, G, Hopser, Anflug, Sprit,
    //! Abfangen, Wind), steht im Protokoll nur verdichtet; es wird so in die
    //! Eingabe gelegt, wie es der Gleichheitslauf tut
    //! (`bewertung-host/tests/gleichheitslauf.rs`, `nachbauen`).
    //!
    //! # ⚠ Ersatz für die Facility-Mitte
    //!
    //! Die von MSFS gemeldete Mitte steht NICHT im Protokoll von 2.1.24 —
    //! genau das behebt P-4.14 (c). Als Ersatz dient die Mitte der
    //! OSM-Bahnachse FQNP 05/23 (Way 05/23; Rollspur und OSM decken sich auf
    //! 0,2 m), mit dem Kurs der OSM-Achse (225,795°) als Ersatz für
    //! `HEADING`. Länge und Breite wie von MSFS gemeldet. Ein zweiter Lauf
    //! nimmt das echte MSFS-`HEADING` für 23 (225,941°, Diagnose-Log,
    //! Schattenmodus) zur OSM-Mitte — eine Mischung, deshalb dort nur Note und
    //! Teilnoten, keine Meter-Grenze.
    //! Navigraph: Zyklus 2611, `nav_runways` auf live (lesend abgefragt).
    use super::*;
    use serde_json::Value;

    /// Mitte der OSM-Achse FQNP 05/23 — Ersatz für die MSFS-Facility-Mitte.
    const OSM_MITTE: (f64, f64) = (-15.105_524_0, 39.281_771_2);
    /// Kurs der OSM-Achse, Landerichtung 05 (Grosskreis Schwelle 05 → 23).
    const OSM_KURS_05: f64 = 45.795;
    /// MSFS-`HEADING` FQNP 05 (23: 225,941°, Diagnose-Log 30.10.2026).
    const MSFS_HEADING_05: f64 = 45.941;

    fn protokoll() -> Value {
        serde_json::from_str(include_str!("../../tests/fixtures/cfg2001_fqnp23_achse.json"))
            .expect("Fixture lesbar")
    }

    fn f32_(v: &Value) -> Option<f32> {
        v.as_f64().map(|x| x as f32)
    }

    fn punkt(lat: f64, lon: f64, elev: i32) -> aeroacars_mqtt::navdata::NavPoint {
        aeroacars_mqtt::navdata::NavPoint { lat, lon, elev_ft: Some(elev) }
    }

    /// FQNP 05/23, Navigraph 2611 (`nav_runways` auf live, 30.10.2026).
    fn fqnp_navigraph() -> aeroacars_mqtt::navdata::NavAirport {
        let bahn = |d: &str, mag: f64, wahr: f64, t: (f64, f64, i32), e: (f64, f64, i32)| {
            aeroacars_mqtt::navdata::NavRunway {
                designator: d.into(),
                magnetic_course: mag,
                true_course: wahr,
                length_ft: 6562,
                width_ft: Some(148),
                surface: Some("ASP".into()),
                threshold: punkt(t.0, t.1, t.2),
                far_end: punkt(e.0, e.1, e.2),
                displaced_threshold_ft: 0,
                ils: None,
                glideslope_angle: 3.0,
                tch_ft: 50,
            }
        };
        let s23 = (-15.099_166_67, 39.288_333_33, 1365);
        let s05 = (-15.111_666_67, 39.275, 1427);
        aeroacars_mqtt::navdata::NavAirport {
            cycle: "2611".into(),
            valid_to: "2026-11-26".into(),
            icao: "FQNP".into(),
            name: "NAMPULA".into(),
            latitude: -15.103_055_56,
            longitude: 39.284_444_44,
            elevation_ft: Some(1444),
            runways: vec![bahn("05", 54.0, 45.843, s05, s23), bahn("23", 234.0, 225.84, s23, s05)],
        }
    }

    /// Die Auskunft, wie der MSFS-Adapter sie baut (`bahn_paar`).
    fn fqnp_szenerie(mitte: (f64, f64), kurs_05: f64) -> sim_core::szenerie::SzenerieFlughafen {
        let [a, b] = sim_msfs::facility::bahn_paar(mitte, kurs_05, 1997.049_682_617_187_5, 39.9288, 1, (5, 0, 0.0), (23, 0, 0.0));
        sim_core::szenerie::SzenerieFlughafen {
            icao: "FQNP".into(),
            quelle: "MSFS Facility (Test)".into(),
            bahnen: vec![a, b],
            ..Default::default()
        }
    }

    /// Die Kette ab der Zuordnung bis zur Note.
    fn rechnen(
        nav: &aeroacars_mqtt::navdata::NavAirport,
        bericht: Option<szenerie_bahn::UebernahmeBericht>,
    ) -> (FlightStats, landing_scoring::LandingScoringInput, landing_scoring::wasm::Ausgabe) {
        let p = protokoll();
        let (td, ro, pf, tick) = (&p["touchdown_complete"], &p["rollout_finalized"], &p["pirep_filed"], &p["aufsetz_tick"]);
        let mut stats = FlightStats::default();
        stats.szenerie_uebernahme = bericht;
        stats.landing_heading_true_deg = f32_(&td["heading_true_deg"]);
        stats.landing_groundspeed_kt = f32_(&td["gs_kt"]);
        stats.landing_headwind_kt = f32_(&td["headwind_kt"]);
        stats.landing_speed_kt = f32_(&td["ias_kt"]);
        stats.landing_true_airspeed_kt = f32_(&tick["true_airspeed_kt"]);
        stats.landing_wind_direction_deg = f32_(&tick["wind_direction_deg"]);
        stats.landing_wind_speed_kt = f32_(&tick["wind_speed_kt"]);
        stats.landing_float_distance_m = f32_(&td["landing_float_distance_m"]);
        stats.rollout_distance_m = ro["rollout_distance_m"].as_f64();

        let rm = runway::lookup_runway_in_nav(
            td["lat"].as_f64().unwrap(),
            td["lon"].as_f64().unwrap(),
            td["heading_true_deg"].as_f64().unwrap() as f32,
            nav,
        )
        .expect("Bahn zugeordnet");
        assert_eq!(rm.runway_ident, "23");
        stats.runway_match = Some(rm);
        stats.runway_source = Some(runway::RunwaySource::Navigraph);

        for z in p["positionen"].as_array().unwrap() {
            let z = z.as_array().unwrap();
            if z[0].as_f64().unwrap() < 0.0 {
                continue; // vor der Aufsetzkante
            }
            let mut snap = SimSnapshot::default();
            snap.lat = z[1].as_f64().unwrap();
            snap.lon = z[2].as_f64().unwrap();
            snap.groundspeed_kt = z[3].as_f64().unwrap() as f32;
            snap.heading_deg_true = z[4].as_f64().unwrap() as f32;
            snap.on_ground = z[5].as_bool().unwrap();
            bahndisziplin_tick(&mut stats, &snap);
        }

        let mut e = scoring_eingang(
            &stats,
            Some("A339"),
            Some(Ausweichziel { eingereicht: "FQNP", geplant: "FQNP" }),
            None,
        );
        // Was nicht an der Bahn hängt: aus dem Protokoll wie im Gleichheitslauf.
        let abgabe = |k: &str| f32_(&pf[k]).or_else(|| f32_(&td[k]));
        e.vs_fpm = f32_(&p["landing_analysis"]["vs_at_edge_fpm"]);
        e.peak_g_load = f32_(&td["peak_g_load"]);
        e.scored_g_load = f32_(&pf["landing_scored_g_force"]);
        e.bounce_count = td["bounce_count"].as_u64().map(|n| n as u32);
        e.light_bounce_count = Some(td["scored_light_bounce_count"].as_u64().unwrap_or(0) as u32);
        e.approach_vs_stddev_fpm = abgabe("approach_vs_stddev_fpm");
        e.approach_bank_stddev_deg = abgabe("approach_bank_stddev_deg");
        e.anflug = landing_scoring::anflug_urteil::AnflugWerte {
            vs_jerk_fpm: abgabe("approach_vs_jerk_fpm"),
            bank_stddev_deg: abgabe("approach_bank_stddev_deg"),
            ias_stddev_kt: abgabe("approach_ias_stddev_kt"),
            excessive_sink: pf["approach_excessive_sink"].as_bool(),
            stable_config: pf["approach_stable_config"].as_bool(),
            gleitpfad_dots: f32_(&pf["gleitpfad_mittel_abs_dots"]),
            ias_stddev_unter_500_kt: None,
            stable_config_500: None,
            spaetes_final_rechtzeitig: td["approach_spaetes_final_rechtzeitig"].as_bool(),
        };
        e.planned_burn_kg = f32_(&pf["planned_burn_kg"]);
        e.actual_trip_burn_kg = f32_(&pf["actual_trip_burn_kg"]);
        e.planned_zfw_kg = f32_(&td["planned_zfw_kg"]);
        e.planned_tow_kg = f32_(&td["planned_tow_kg"]);
        e.geflogene_strecke_nm = f32_(&pf["distance_nm"]);
        e.durchstarts = pf["go_around_count"].as_u64().unwrap_or(0) as u32;
        e.fahrwerk_spurweite_m = td["track_width_m"].as_f64();
        e.abfangen = serde_json::from_value(pf["abfangen"].clone()).ok();
        fill_v2_rollout_fields(&mut e, &stats, "FQNP");
        let (ausgabe, _) = crate::bewertung::bewerten(&e).expect("Modul rechnet");
        (stats, e, ausgabe)
    }

    fn teil(a: &landing_scoring::wasm::Ausgabe, key: &str) -> u8 {
        a.teilnoten.iter().find(|t| t.key == key).unwrap_or_else(|| panic!("Teilnote {key}")).score
    }

    /// Heutiger Stand ohne P-4.14 (a): Navigraph-Achse, nur Breite/Länge/
    /// Belag aus der Szenerie — so hat 2.1.24 gerechnet.
    fn ohne_umbau() -> (aeroacars_mqtt::navdata::NavAirport, szenerie_bahn::UebernahmeBericht) {
        let nav = fqnp_navigraph();
        let (mut aus, mut b) =
            szenerie_bahn::uebernimm_szenerie(&nav, &fqnp_szenerie(OSM_MITTE, OSM_KURS_05), szenerie_bahn::Quelle::MsfsFacility);
        for (r, n) in aus.runways.iter_mut().zip(&nav.runways) {
            r.threshold = n.threshold.clone();
            r.far_end = n.far_end.clone();
            r.true_course = n.true_course;
        }
        // Ein Bericht wie vor P-4.14: ohne Achsen-Herkunft.
        b.achsen.clear();
        (aus, b)
    }

    #[test]
    fn cfg2001_fqnp23_mit_der_achse_durch_die_facility_mitte_58() {
        let nav = fqnp_navigraph();
        let (aus, bericht) =
            szenerie_bahn::uebernimm_szenerie(&nav, &fqnp_szenerie(OSM_MITTE, OSM_KURS_05), szenerie_bahn::Quelle::MsfsFacility);
        let achse = bericht.achsen.iter().find(|a| a.bahn == "23").expect("Achse 23");
        assert_eq!(achse.herkunft, szenerie_bahn::AchsenHerkunft::SzenerieMitte);
        assert_eq!(achse.kurs_quelle, szenerie_bahn::KursHerkunft::Heading);
        // Navigraph liegt rechts der echten Achse (in Landerichtung 23), die
        // Mitte also 16,3 m links: Mittel aus 15,6 und 17,1 m.
        let quer = achse.verschiebung_quer_m.unwrap();
        assert!((quer + 16.4).abs() < 0.5, "Verschiebung {quer:.2} m");
        assert_eq!(achse.szenerie_mitte.map(|m| ((m.0 * 1e6).round(), (m.1 * 1e6).round())), Some(((OSM_MITTE.0 * 1e6).round(), (OSM_MITTE.1 * 1e6).round())));

        let (stats, e, a) = rechnen(&aus, Some(bericht));
        let td_quer = stats.runway_match.as_ref().unwrap().centerline_distance_m;
        let max = stats.bahn_max_querversatz_m.expect("Versatz gemessen");
        eprintln!(
            "CFG2001 neu: TD {td_quer:+.2} m, max {max:+.2} m, Proben {}, Bahndisziplin {}, Ausrichtung {}, Note {:?} {:?}, Abzuege {:?}/{:?}",
            stats.bahn_proben, teil(&a, "rollout"), teil(&a, "alignment"), a.gesamt, a.gesamt_buchstabe, a.landungs_abzug, a.anflug_abzug
        );
        assert_eq!(e.bahn_geometrie_aus_szenerie, Some(true), "Achse aus der Szenerie (P-4.14 b)");
        assert!(td_quer.abs() <= 3.0, "Aufsetzen {td_quer:.2} m neben der Mitte");
        assert!(max.abs() <= 3.0, "groesster Versatz {max:.2} m");
        assert_eq!(teil(&a, "rollout"), 100, "Bahndisziplin");
        assert_eq!(teil(&a, "alignment"), 100, "Ausrichtung");
        assert_eq!(a.gesamt, Some(58));
        assert_eq!(a.gesamt_buchstabe.as_deref(), Some("D"));
    }

    /// Dieselbe Kette mit dem echten MSFS-`HEADING` (225,941°) an der
    /// OSM-Mitte: 0,15° gegen OSM gedreht. Die Note bleibt 58.
    #[test]
    fn cfg2001_fqnp23_mit_msfs_heading_bleibt_58() {
        let nav = fqnp_navigraph();
        let (aus, bericht) = szenerie_bahn::uebernimm_szenerie(
            &nav,
            &fqnp_szenerie(OSM_MITTE, MSFS_HEADING_05),
            szenerie_bahn::Quelle::MsfsFacility,
        );
        assert_eq!(bericht.achsen[1].kurs_quelle, szenerie_bahn::KursHerkunft::Heading);
        let (stats, _, a) = rechnen(&aus, Some(bericht));
        eprintln!(
            "CFG2001 HEADING: TD {:+.2} m, max {:+.2} m, Note {:?}",
            stats.runway_match.as_ref().unwrap().centerline_distance_m,
            stats.bahn_max_querversatz_m.unwrap(),
            a.gesamt
        );
        assert_eq!((teil(&a, "rollout"), teil(&a, "alignment"), a.gesamt), (100, 100, Some(58)));
    }

    /// Gegenprobe: dieselbe Kette mit der Navigraph-Achse — ergibt genau den
    /// gespeicherten Altwert. Damit ist belegt, dass die Kette Joels Flug
    /// nachstellt, und dass erst (a) die Note ändert.
    #[test]
    fn cfg2001_fqnp23_gegenprobe_ohne_umbau_25() {
        let p = protokoll();
        let gespeichert = p["pirep_filed"]["landing_score"].as_u64().unwrap();
        assert_eq!(gespeichert, 25);
        let (aus, bericht) = ohne_umbau();
        let (stats, e, a) = rechnen(&aus, Some(bericht.clone()));
        let td_quer = stats.runway_match.as_ref().unwrap().centerline_distance_m;
        let max = stats.bahn_max_querversatz_m.unwrap();
        eprintln!(
            "CFG2001 alt: TD {td_quer:+.2} m, max {max:+.2} m, Bahndisziplin {}, Ausrichtung {}, Note {:?}",
            teil(&a, "rollout"), teil(&a, "alignment"), a.gesamt
        );
        // Die Kette trifft die gespeicherten Querwerte des Clients 2.1.24
        // (Endstand `rollout_finalized`; `touchdown_complete` trägt einen
        // Zwischenstand mitten im Ausrollen, −18,61 m).
        let alt_td = p["rollout_finalized"]["runway_match_centerline_offset_m"].as_f64().unwrap();
        let alt_max = p["rollout_finalized"]["max_lateral_offset_m"].as_f64().unwrap();
        assert!((td_quer - alt_td).abs() < 0.01, "TD {td_quer:.3} gegen gespeichert {alt_td:.3}");
        assert!((max as f64 - alt_max).abs() < 0.01, "max {max:.3} gegen gespeichert {alt_max:.3}");
        // P-4.14 (b): nur Breite übernommen → Achse gilt NICHT als bestätigt.
        assert_eq!(e.bahn_geometrie_aus_szenerie, Some(false));
        assert_eq!(teil(&a, "rollout"), 20);
        assert_eq!(teil(&a, "alignment"), 65);
        assert_eq!(a.gesamt, Some(gespeichert as u8));

        // Mit dem Schalter von 2.1.24 (true, weil die Breite übernommen war)
        // dasselbe: Das Netz `achse_fragwuerdig` prüft nur den Winkel und
        // fängt die parallele Verschiebung (0,21°) ohnehin nicht.
        let mut alt_schalter = e.clone();
        alt_schalter.bahn_geometrie_aus_szenerie = Some(true);
        let (b, _) = crate::bewertung::bewerten(&alt_schalter).unwrap();
        assert_eq!(b.gesamt, Some(25));
    }

    /// P-4.14 (c): Was ins Eingabe-Dokument geht (`herkunft.bahn.korrekturen`
    /// ist der Übernahmebericht): Facility-Mitte, Verschiebung, Herkunft.
    #[test]
    fn cfg2001_achse_wird_mitgeschrieben() {
        let nav = fqnp_navigraph();
        let (_, b) = szenerie_bahn::uebernimm_szenerie(&nav, &fqnp_szenerie(OSM_MITTE, OSM_KURS_05), szenerie_bahn::Quelle::MsfsFacility);
        let v = serde_json::to_value(&b).unwrap();
        let a23 = v["achsen"].as_array().unwrap().iter().find(|a| a["bahn"] == "23").unwrap();
        assert_eq!(a23["herkunft"], "szenerie_mitte");
        assert_eq!(a23["kurs_quelle"], "heading");
        assert!(a23["szenerie_mitte"][0].as_f64().unwrap() < -15.1);
        assert!(a23["verschiebung_quer_m"].as_f64().unwrap() < -15.0);
        assert!(a23["verschiebung_laengs_m"].as_f64().unwrap().abs() < 2.0);
    }

    /// ⚠ Das ILS wandert NICHT mit der Achse (Regel aus dem Fall Sven,
    /// RJAA 34L, v2.0.23). Das ILS ist in den Navdaten definiert; wird die
    /// Achse unter MSFS durch die Facility-Mitte gelegt, rechnet der
    /// Gleitpfad weiter gegen die unveränderte Navigraph-Bahn.
    #[test]
    fn ils_bleibt_bei_verschobener_msfs_achse_auf_navigraph() {
        let mut nav = fqnp_navigraph();
        for r in nav.runways.iter_mut() {
            r.ils = Some(aeroacars_mqtt::navdata::NavIls { freq_mhz: 110.3, course: 234.0, category: 1 });
        }
        let (aus, bericht) =
            szenerie_bahn::uebernimm_szenerie(&nav, &fqnp_szenerie(OSM_MITTE, OSM_KURS_05), szenerie_bahn::Quelle::MsfsFacility);
        let roh = nav.runways.iter().find(|r| r.designator == "23").unwrap();
        let verschoben = aus.runways.iter().find(|r| r.designator == "23").unwrap().clone();
        let mut stats = FlightStats::default();
        stats.runway_nav_geometry = Some(verschoben.clone());
        stats.runway_ils_navigraph = IlsNavigraph::aus(&nav, "23");
        let abstand = |b: &anflug_forensik::Bahnbezug, p: &aeroacars_mqtt::navdata::NavPoint| {
            runway::distance_m(b.schwelle_lat, b.schwelle_lon, p.lat, p.lon)
        };

        // Gegenprobe ohne Achsen-Herkunft (Stand vor P-4.14): Die Forensik
        // nimmt die Geometrie der Zuordnung — hier also die verschobene, das
        // ILS wanderte 16 m mit.
        let alt = anflug_forensik_bahn(&stats).unwrap();
        assert!(abstand(&alt, &roh.threshold) > 15.0, "Gegenprobe greift nicht: {:.2} m", abstand(&alt, &roh.threshold));

        // Mit P-4.14: Navigraph-Schwelle, bit-gleich.
        stats.szenerie_uebernahme = Some(bericht);
        let neu = anflug_forensik_bahn(&stats).unwrap();
        assert_eq!((neu.schwelle_lat, neu.schwelle_lon), (roh.threshold.lat, roh.threshold.lon));
        assert_eq!((neu.ende_lat, neu.ende_lon), (roh.far_end.lat, roh.far_end.lon));
        assert!(abstand(&neu, &verschoben.threshold) > 15.0, "die Zuordnung ist trotzdem verschoben");
    }
