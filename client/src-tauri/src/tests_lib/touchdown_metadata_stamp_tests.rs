    //! v0.15.24 — tests for the phase-independent touchdown metadata stamp
    //! (`stamp_touchdown_metadata` / `correlate_touchdown_runway` /
    //! `stamp_landing_weight`), extracted from the FSM Landing arm so the
    //! validated 50 Hz sampler touchdown can stamp the same fields when the
    //! FSM never reaches Landing (bush hops — GSG312 97OG).

    use super::*;
    use chrono::TimeZone;
    use sim_core::SimSnapshot;

    fn td_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 10, 18, 0, 0).unwrap()
    }

    /// Buffer sample with clearly-recognisable TD-instant values, distinct
    /// from everything in `rollout_snap()` so each assertion can tell which
    /// source won.
    fn buf_sample(at: DateTime<Utc>, on_ground: bool) -> TelemetrySample {
        TelemetrySample {
            at,
            vs_fpm: -350.0,
            g_force: 1.4,
            g_semibody: None,
            on_ground,
            agl_ft: if on_ground { 0.0 } else { 12.0 },
            msl_ft: if on_ground { 0.0 } else { 12.0 } + 500.0,
            heading_true_deg: 270.0,
            groundspeed_kt: 95.0,
            indicated_airspeed_kt: 100.0,
            true_airspeed_kt: 100.0,
            lat: 51.0,
            lon: 12.0,
            pitch_deg: 4.5,
            bank_deg: -1.5,
            gear_normal_force_n: None,
            total_weight_kg: Some(1000.0),
            // Beim Aufsetzen ist mehr Sprit da als beim spaeteren
            // Schnappschuss (88,5) — dazwischen liegt der Umkehrschub.
            fuel_kg: 92.0,
        }
    }

    /// Live snap as it would look on a (late) tick AFTER touchdown —
    /// post-derotation pitch, bled-off IAS, drifted position. If any of
    /// these leak into the stamped landing_* fields while the buffer holds
    /// a TD sample, the fallback chain is broken.
    /// Auch vom Sprit-Test ueber die echte Kette benutzt.
    pub(super) fn rollout_snap() -> SimSnapshot {
        SimSnapshot {
            lat: 51.001,
            lon: 12.001,
            heading_deg_true: 280.0,
            heading_deg_magnetic: 277.0, // magvar = +3.0
            pitch_deg: -2.0,
            bank_deg: 0.5,
            indicated_airspeed_kt: 55.0,
            true_airspeed_kt: 55.0,
            groundspeed_kt: 50.0,
            fuel_total_kg: 88.5,
            velocity_body_x_fps: Some(5.0),
            velocity_body_z_fps: Some(50.0),
            aircraft_wind_x_kt: Some(7.0),
            aircraft_wind_z_kt: Some(-9.0),
            total_weight_kg: Some(1043.0),
            ..SimSnapshot::default()
        }
    }

    /// Stats with a 3-sample buffer: airborne → TD (first on-ground) →
    /// rollout. The FIRST on-ground sample is the touchdown moment.
    fn stats_with_buffer() -> FlightStats {
        let mut stats = FlightStats::default();
        stats.snapshot_buffer.push_back(buf_sample(
            td_at() - chrono::Duration::milliseconds(500),
            false,
        ));
        stats.snapshot_buffer.push_back(buf_sample(td_at(), true));
        stats.snapshot_buffer.push_back(buf_sample(
            td_at() + chrono::Duration::milliseconds(500),
            true,
        ));
        stats
    }

    /// Ein Ausbrechen bei hoher Fahrt ist keine Ausfahrt.
    ///
    /// # Der Befund
    ///
    /// Ein Flugzeug setzt bei Seitenwind schraeg auf und richtet sich
    /// danach auf die Bahnachse aus. Gegenueber dem AUFSETZKURS bleibt
    /// dadurch eine dauerhafte Differenz von zehn Grad und mehr — kein
    /// Abbiegen, sondern dessen Korrektur.
    ///
    /// Am Korpus gemessen (854 Landungen mit 50-Hz-Fenster): 13
    /// ueberschreiten die Schwelle, waehrend sie noch ueber sechzig Knoten
    /// schnell sind, bei bis zu 141 kt und 100 von 100 folgenden Proben.
    /// Dort biegt niemand auf einen Rollweg ab — und die Bewertung waere
    /// beendet gewesen, bevor sie angefangen hat.
    ///
    /// Ueber sechzig Knoten ist eine seitliche Abweichung ein Ausbrechen,
    /// und das ist genau das, was diese Achse bewerten SOLL.
    #[test]
    fn ausbrechen_bei_hoher_fahrt_setzt_keinen_raeumpunkt() {
        let flight = flight_fixture("EDDH");
        {
            let mut stats = flight.stats.lock().expect("stats");
            stats.phase = FlightPhase::Landing;
            stats.landing_at = Some(Utc::now());
            stats.landing_lat = Some(53.636_011);
            stats.landing_lon = Some(9.999_656);
            stats.rollout_last_lat = Some(53.636_011);
            stats.rollout_last_lon = Some(9.999_656);
            stats.landing_heading_true_deg = Some(230.21);
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0,
                surface: "ASP".to_string(),
                threshold_lat: 53.636_011,
                threshold_lon: 9.999_656,
                end_lat: 53.619_958,
                end_lon: 9.967_167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
        }

        // 115 kt, Nase 14 Grad schraeg — der Crab-Ausgleich nach dem
        // Aufsetzen, gemessen am echten Bestand.
        let snap = SimSnapshot {
            lat: 53.630_234,
            lon: 9.988_032,
            groundspeed_kt: 115.0,
            heading_deg_true: 216.0,
            on_ground: true,
            ..Default::default()
        };
        step_flight_at(&flight, &snap, Utc::now());

        let stats = flight.stats.lock().expect("stats");
        assert!(
            stats.bahn_raeum_laengs_m.is_none(),
            "bei 115 kt wurde ein Raeumpunkt gesetzt — das ist ein \
             Ausbrechen, keine Ausfahrt"
        );
        assert!(
            !stats.bahn_fenster_zu,
            "die Bewertung wurde beendet, bevor sie angefangen hat"
        );
        // Und der Versatz wird weiter gemessen — genau darum geht es.
        assert!(
            stats.bahn_max_querversatz_m.is_some(),
            "das Ausbrechen wird gar nicht bewertet"
        );
    }

    /// Die Ausfahrten kommen AUCH aus der Auskunft des geplanten Ziels.
    ///
    /// Das ist die eigentliche Korrektur aus DLH 373 (20.09.2026): Im
    /// Sinkflug korrelierte die Bahn kurz mit EDML (Landshut), das
    /// Ernteziel wechselte, und `szenerie_auskunft` wurde verworfen —
    /// richtig, denn fremde Rollwege duerfen nicht in die Ausfahrten
    /// geraten. Beim Aufsetzen in EDDM war damit keine Bodenkarte da.
    ///
    /// Der Waechter `ein_ueberflugplatz_raeumt_die_auskunft_des_ziels_nicht_weg`
    /// belegt nur, dass das FELD den Zielwechsel ueberlebt. Ob es dann
    /// auch benutzt wird, entscheidet `bahn_felder` — und genau dieser
    /// Zweig war bis zur externen Abnahme am 21.09.2026 von keinem Test
    /// erreichbar: Wer ihn loescht, bekam 1411 gruene Tests.
    #[test]
    fn ausfahrten_kommen_auch_aus_der_zielauskunft() {
        let flight = flight_fixture("EDDM");
        let rollweg = |name: &str, lat: f64, lon: f64| sim_core::szenerie::SzenerieRollweg {
            name: name.to_string(),
            // Zwei Punkte quer zur Bahn, damit die Ausfahrt sie trifft.
            punkte: vec![(lat, lon), (lat + 0.004, lon + 0.004)],
        };
        {
            let mut stats = flight.stats.lock().expect("stats");
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDM".to_string(),
                runway_ident: "26R".to_string(),
                heading_true_deg: 263.4,
                length_ft: 13123.0,
                width_ft: 197.0,
                surface: "ASP".to_string(),
                threshold_lat: 48.3694,
                threshold_lon: 11.8306,
                end_lat: 48.3617,
                end_lon: 11.7766,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 1656.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            // Genau die Lage aus dem Flug: Das Ernteziel steht auf dem
            // Ueberflugplatz, das Ziel liegt nur noch in der Zweitquelle.
            stats.szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
                icao: "EDML".to_string(),
                ..Default::default()
            });
            stats.ziel_szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
                icao: "EDDM".to_string(),
                rollwege: vec![
                    rollweg("A8", 48.3660, 11.8050),
                    rollweg("A10", 48.3640, 11.7930),
                ],
                ..Default::default()
            });
        }
        let stats = flight.stats.lock().expect("stats");
        let felder = bahn_felder(&stats, Some("A388"), None);
        assert!(
            !felder.runway_exits.is_empty(),
            "die Auskunft des Ziels wird nicht als Quelle benutzt — \
             genau der Fehler aus DLH 373",
        );
    }

    /// EDDM 26R mit zwei Szenerie-Rollwegen; A8 trifft die Bahnkante.
    fn eddm_26r_mit_rollwegen(flight: &ActiveFlight) -> runway::RunwayMatch {
        let rollweg = |name: &str, lat: f64, lon: f64| sim_core::szenerie::SzenerieRollweg {
            name: name.to_string(),
            punkte: vec![(lat, lon), (lat + 0.004, lon + 0.004)],
        };
        let bahn = runway::RunwayMatch {
            airport_ident: "EDDM".to_string(),
            runway_ident: "26R".to_string(),
            heading_true_deg: 263.4,
            length_ft: 13123.0,
            width_ft: 197.0,
            surface: "ASP".to_string(),
            threshold_lat: 48.3694,
            threshold_lon: 11.8306,
            end_lat: 48.3617,
            end_lon: 11.7766,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 1656.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        };
        let mut stats = flight.stats.lock().expect("stats");
        stats.runway_match = Some(bahn.clone());
        stats.szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
            icao: "EDDM".to_string(),
            rollwege: vec![
                rollweg("A8", 48.3660, 11.8050),
                rollweg("A10", 48.3640, 11.7930),
            ],
            ..Default::default()
        });
        bahn
    }

    /// B5 (#1662, ESSA 19L, Client 2.0.2): Räumseite „links“, die Spur
    /// verlässt die Bahn aber bei 2031 m rechts und bleibt draussen. Der
    /// Kurswechsel bei 1535 m war ein Linksschlenker. Mit den echten Proben
    /// aus `fixtures/1662.json`: die Seite kommt aus dem Kantenübertritt.
    /// Alter Stand: „left“ (rot).
    #[test]
    fn raeumseite_folgt_dem_kantenuebertritt_nicht_dem_schlenker() {
        let fall: serde_json::Value =
            serde_json::from_str(include_str!("../../../src/dev/bahnansicht/fixtures/1662.json")).unwrap();
        let mut stats = FlightStats::default();
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "ESSA".into(),
            runway_ident: "19L".into(),
            heading_true_deg: 190.0,
            length_ft: 8208.0,
            width_ft: 150.0,
            surface: "ASP".into(),
            threshold_lat: 59.66,
            threshold_lon: 17.96,
            end_lat: 59.64,
            end_lon: 17.95,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 0.0,
            side: "left".into(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.bahn_spur = fall["lateral_samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["laengs_m"].as_f64().unwrap() as f32, p["quer_m"].as_f64().unwrap() as f32))
            .collect();
        stats.bahn_raeum_laengs_m = fall["scoring_cutoff_m"].as_f64();
        stats.bahn_raeum_kurs_diff = Some(-12.0);
        // So hatte der Client 2.0.2 sie live gesetzt (Schlenker links).
        stats.bahn_raeum_seite = Some("left".into());
        let f = bahn_felder(&stats, Some("A320"), None);
        assert_eq!(f.clearance_side.as_deref(), Some("right"));
        assert!((f.clearance_point_m.unwrap() - 2031.0).abs() < 15.0, "{:?}", f.clearance_point_m);
        // B5b: der bewertete Teil endet am endgueltigen Uebertritt (~2031 m),
        // nicht am Schlenker (1535 m). Alter Stand: 1534,6 (rot).
        assert!((f.scoring_cutoff_m.unwrap() - 2031.0).abs() < 15.0, "{:?}", f.scoring_cutoff_m);
        // Gegenprobe: bleibt die Spur auf der Bahn, gilt weiter die Kurs-Probe.
        stats.bahn_spur.retain(|(l, _)| *l < 1900.0);
        let f = bahn_felder(&stats, Some("A320"), None);
        assert_eq!(f.clearance_side.as_deref(), Some("left"));
        // ... und der bewertete Teil endet wie bisher am Ausschwenken.
        assert_eq!(f.scoring_cutoff_m, fall["scoring_cutoff_m"].as_f64());
        // Gleiche Seite wie der Uebertritt: unveraendert am Ausschwenken.
        let mut gleich = FlightStats::default();
        gleich.runway_match = stats.runway_match.clone();
        gleich.bahn_spur = fall["lateral_samples"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["laengs_m"].as_f64().unwrap() as f32, p["quer_m"].as_f64().unwrap() as f32))
            .collect();
        gleich.bahn_raeum_laengs_m = Some(1900.0);
        gleich.bahn_raeum_seite = Some("right".into());
        let f = bahn_felder(&gleich, Some("A320"), None);
        assert_eq!(f.scoring_cutoff_m, Some(1900.0));
    }

    /// ⚠ Die Ausfahrten entstehen je Bodenkarte und Bahn EINMAL.
    ///
    /// AIB 429 (HECA, 1480 Rollwege, 12.10.2026): `bahn_felder` laeuft je
    /// Landung viele Male, und jedes Mal wurde die ganze Rollwegkarte als
    /// GeoJSON formatiert, wieder geparst und projiziert. Gegenproben:
    /// Eine andere Bahn oder eine andere Karte rechnet neu, und das
    /// Ergebnis folgt ihr.
    #[test]
    fn ausfahrten_werden_je_karte_und_bahn_nur_einmal_gerechnet() {
        let flight = flight_fixture("EDDM");
        let bahn = eddm_26r_mit_rollwegen(&flight);
        let mut stats = flight.stats.lock().expect("stats");
        let laengs = |f: &BahnFelder| -> Vec<(String, i64)> {
            f.runway_exits
                .iter()
                .map(|a| (a.name.clone(), a.laengs_m.round() as i64))
                .collect()
        };
        let gerechnet = || AUSFAHRTEN_GERECHNET.with(|z| z.get());

        AUSFAHRTEN_GERECHNET.with(|z| z.set(0));
        let erst = bahn_felder(&stats, Some("A388"), None);
        let dann = bahn_felder(&stats, Some("A388"), None);
        assert!(
            erst.runway_exits.iter().any(|a| a.name == "A8"),
            "Vorrichtung: A8 ist eine Ausfahrt"
        );
        assert_eq!(laengs(&erst), laengs(&dann), "gemerkt heisst gleich");
        assert_eq!(
            gerechnet(),
            1,
            "dieselbe Karte und Bahn, trotzdem wurden die Ausfahrten erneut gerechnet"
        );

        // Gegenprobe 1: andere Bahn (Schwelle rund 300 m weiter) → neu.
        let mut verschoben = bahn.clone();
        verschoben.threshold_lat += (bahn.end_lat - bahn.threshold_lat) * 0.075;
        verschoben.threshold_lon += (bahn.end_lon - bahn.threshold_lon) * 0.075;
        stats.runway_match = Some(verschoben);
        let neu_bahn = bahn_felder(&stats, Some("A388"), None);
        assert_eq!(gerechnet(), 2, "eine andere Bahn kam aus dem Merker");
        assert_ne!(
            laengs(&neu_bahn),
            laengs(&erst),
            "die Ausfahrten folgen der neuen Schwelle nicht"
        );

        // Gegenprobe 2: dieselbe Bahn, aber die Karte ohne A8 → neu.
        stats.runway_match = Some(bahn);
        if let Some(a) = stats.szenerie_auskunft.as_mut().map(Arc::make_mut) {
            a.rollwege.remove(0);
        }
        let neu_karte = bahn_felder(&stats, Some("A388"), None);
        assert_eq!(gerechnet(), 3, "eine andere Karte kam aus dem Merker");
        assert!(
            neu_karte.runway_exits.iter().all(|a| a.name != "A8"),
            "der entfernte Rollweg ist noch da"
        );
    }

    /// ⚠ Das Vorwaermen rechnet genau die Ausfahrten, die `bahn_felder`
    /// danach braucht.
    ///
    /// Es laeuft im Streamer-Takt ohne gehaltene Sperre der Flugdaten
    /// (AIB 429, 12.10.2026). Benutzte es eine andere Auswahl oder einen
    /// anderen Schluessel als `bahn_felder`, fuellte es einen Merker, den
    /// niemand liest — und `bahn_felder` rechnete wieder unter der Sperre.
    #[test]
    fn das_vorwaermen_fuellt_den_merker_fuer_bahn_felder() {
        let flight = flight_fixture("EDDM");
        let bahn = eddm_26r_mit_rollwegen(&flight);
        let gerechnet = || AUSFAHRTEN_GERECHNET.with(|z| z.get());
        AUSFAHRTEN_GERECHNET.with(|z| z.set(0));

        let mut ausloeser = None;
        ausfahrten_vorwaermen(&flight, &mut ausloeser);
        assert_eq!(gerechnet(), 1, "das Vorwaermen hat nichts gerechnet");
        ausfahrten_vorwaermen(&flight, &mut ausloeser);
        assert_eq!(gerechnet(), 1, "gleicher Ausloeser, trotzdem neu gerechnet");
        {
            let stats = flight.stats.lock().expect("stats");
            let felder = bahn_felder(&stats, Some("A388"), None);
            assert!(
                felder.runway_exits.iter().any(|a| a.name == "A8"),
                "die vorgewaermten Ausfahrten kommen nicht an"
            );
            assert_eq!(
                gerechnet(),
                1,
                "`bahn_felder` rechnet selbst — Vorwaermen und `bahn_felder` \
                 benutzen nicht denselben Schluessel"
            );
        }

        // Gegenprobe: eine neue Bahn → das Vorwaermen rechnet neu.
        {
            let mut stats = flight.stats.lock().expect("stats");
            let mut neu = bahn;
            neu.threshold_lat += (neu.end_lat - neu.threshold_lat) * 0.075;
            neu.threshold_lon += (neu.end_lon - neu.threshold_lon) * 0.075;
            stats.runway_match = Some(neu);
            stats.bahn_revision += 1;
        }
        ausfahrten_vorwaermen(&flight, &mut ausloeser);
        assert_eq!(gerechnet(), 2, "die neue Bahn wurde nicht vorgewaermt");
    }

    /// ⚠ Der Streamer-Takt rechnet die Ausfahrten NICHT selbst — ein
    /// eigener Faden tut es, und `bahn_felder` findet das Ergebnis danach
    /// im Merker (AIB 430, 13.10.2026: 299 ms im Takt nach dem Aufsetzen).
    ///
    /// `AUSFAHRTEN_GERECHNET` zaehlt je Faden: Im aufrufenden Faden muss
    /// es bei 0 bleiben. Mit dem Aufruf von 2.0.13/2.0.14 im Takt
    /// (`ausfahrten_vorwaermen`) stuende es hier auf 1.
    #[test]
    fn der_streamer_takt_rechnet_die_ausfahrten_nicht_selbst() {
        let flight = Arc::new(flight_fixture("EDDM"));
        let bahn = eddm_26r_mit_rollwegen(&flight);
        let gerechnet = || AUSFAHRTEN_GERECHNET.with(|z| z.get());
        AUSFAHRTEN_GERECHNET.with(|z| z.set(0));

        let mut ausloeser = None;
        let mut laeufer = None;
        ausfahrten_im_hintergrund(&flight, &mut ausloeser, &mut laeufer);
        assert_eq!(gerechnet(), 0, "der Streamer-Takt hat selbst gerechnet");
        laeufer
            .take()
            .expect("kein Faden gestartet")
            .join()
            .expect("Faden");
        {
            let stats = flight.stats.lock().expect("stats");
            let felder = bahn_felder(&stats, Some("A388"), None);
            assert!(
                felder.runway_exits.iter().any(|a| a.name == "A8"),
                "die Ausfahrten aus dem Faden kommen nicht an"
            );
            assert_eq!(
                gerechnet(),
                0,
                "`bahn_felder` musste selbst rechnen — der Faden hat den \
                 Merker nicht gefuellt"
            );
        }

        // Laeuft noch ein Faden, darf der Takt den Ausloeser nicht
        // verbrauchen — sonst ginge eine Aenderung waehrend des Rechnens
        // verloren, und `bahn_felder` rechnete wieder unter der Sperre.
        let (los, warten) = std::sync::mpsc::channel::<()>();
        laeufer = Some(std::thread::spawn(move || {
            let _ = warten.recv();
        }));
        {
            let mut stats = flight.stats.lock().expect("stats");
            let mut neu = bahn;
            neu.threshold_lat += (neu.end_lat - neu.threshold_lat) * 0.075;
            neu.threshold_lon += (neu.end_lon - neu.threshold_lon) * 0.075;
            stats.runway_match = Some(neu);
            stats.bahn_revision += 1;
        }
        let vorher = ausloeser.clone();
        ausfahrten_im_hintergrund(&flight, &mut ausloeser, &mut laeufer);
        assert_eq!(
            ausloeser, vorher,
            "Ausloeser verbraucht, obwohl ein Faden lief"
        );
        los.send(()).expect("Faden freigeben");
        laeufer.take().expect("Faden").join().expect("Faden");

        ausfahrten_im_hintergrund(&flight, &mut ausloeser, &mut laeufer);
        laeufer
            .take()
            .expect("die neue Bahn startete keinen Faden")
            .join()
            .expect("Faden");
        let stats = flight.stats.lock().expect("stats");
        let _ = bahn_felder(&stats, Some("A388"), None);
        assert_eq!(gerechnet(), 0, "die neue Bahn lag nicht im Merker");
    }

    /// ⚠ Ein veraltetes Ergebnis ueberschreibt den Merker NICHT (Codex,
    /// PR #138).
    ///
    /// Ablauf: Der Faden nimmt den Auftrag fuer Bahn A. Waehrend er rechnet,
    /// wechselt die Bahn auf B, und `bahn_felder` legt B in den Merker.
    /// Legte der Faden danach A ab, muesste `bahn_felder` B erneut unter
    /// der Sperre rechnen.
    #[test]
    fn ein_veraltetes_vorwaermen_ueberschreibt_den_merker_nicht() {
        let flight = flight_fixture("EDDM");
        let bahn = eddm_26r_mit_rollwegen(&flight);
        let gerechnet = || AUSFAHRTEN_GERECHNET.with(|z| z.get());

        let mut ausloeser = None;
        let auftrag_a = vorwaerm_auftrag(&flight, &mut ausloeser).expect("Auftrag A");
        {
            let mut stats = flight.stats.lock().expect("stats");
            let mut neu = bahn;
            neu.threshold_lat += (neu.end_lat - neu.threshold_lat) * 0.075;
            neu.threshold_lon += (neu.end_lon - neu.threshold_lon) * 0.075;
            stats.runway_match = Some(neu);
            stats.bahn_revision += 1;
            let _ = bahn_felder(&stats, Some("A388"), None);
        }
        vorwaerm_ausfuehren(&flight, auftrag_a);

        AUSFAHRTEN_GERECHNET.with(|z| z.set(0));
        let stats = flight.stats.lock().expect("stats");
        let _ = bahn_felder(&stats, Some("A388"), None);
        assert_eq!(
            gerechnet(),
            0,
            "das veraltete Ergebnis fuer Bahn A hat Bahn B im Merker ersetzt"
        );
    }

    /// Und die Zweitquelle darf KEINEN fremden Platz durchlassen.
    ///
    /// Gegenprobe zum Test darueber: Passt die ICAO nicht zur gematchten
    /// Bahn, bleibt die Liste leer. Sonst haette die Korrektur genau das
    /// Loch aufgerissen, das der Zielwechsel schliessen soll.
    #[test]
    fn die_zielauskunft_eines_fremden_platzes_bleibt_draussen() {
        let flight = flight_fixture("EDDM");
        {
            let mut stats = flight.stats.lock().expect("stats");
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDM".to_string(),
                runway_ident: "26R".to_string(),
                heading_true_deg: 263.4,
                length_ft: 13123.0,
                width_ft: 197.0,
                surface: "ASP".to_string(),
                threshold_lat: 48.3694,
                threshold_lon: 11.8306,
                end_lat: 48.3617,
                end_lon: 11.7766,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 1656.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            // Ausweichflug: gelandet wird in EDDM, das geplante Ziel war
            // ein anderer Platz — dessen Rollwege gehoeren hier nicht hin.
            stats.ziel_szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
                icao: "EDDF".to_string(),
                rollwege: vec![sim_core::szenerie::SzenerieRollweg {
                    name: "N1".to_string(),
                    punkte: vec![(48.3660, 11.8050), (48.3700, 11.8090)],
                }],
                ..Default::default()
            });
        }
        let stats = flight.stats.lock().expect("stats");
        let felder = bahn_felder(&stats, Some("A388"), None);
        assert!(
            felder.runway_exits.is_empty(),
            "die Rollwege eines FREMDEN Platzes sind in die Ausfahrten geraten",
        );
    }

    /// Die Ausfahrtsseite ueberlebt paralleles Weiterrollen.
    ///
    /// # Der Befund
    ///
    /// Die Richtungsprobe verglich den LETZTEN Spurpunkt mit einem
    /// hundert Meter davor — also die Bewegung dort, wo die Spur aufhoert.
    /// Nach einer Ausfahrt rollt ein Flugzeug aber oft noch hunderte Meter
    /// parallel zur Bahn weiter. Dort ist kein Querversatz mehr, und eine
    /// voellig eindeutige Linkskurve wurde wieder zu `null`.
    ///
    /// Gemessen wird jetzt das Segment ab dem Raeumpunkt.
    #[test]
    fn ausfahrtsseite_ueberlebt_paralleles_weiterrollen() {
        let mut stats = FlightStats::default();
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDH".to_string(),
            runway_ident: "23".to_string(),
            heading_true_deg: 230.21,
            length_ft: 10663.0,
            width_ft: 151.0,
            surface: "ASP".to_string(),
            threshold_lat: 53.636_011,
            threshold_lon: 9.999_656,
            end_lat: 53.619_958,
            end_lon: 9.967_167,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 720.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        // Ausschwenken bei 1700 m nach LINKS (quer wird negativ), danach
        // ab 1850 m parallel weiter — dort aendert sich nichts mehr.
        stats.bahn_spur = vec![
            (1500.0, -2.0),
            (1600.0, -3.0),
            (1700.0, -5.0),
            (1750.0, -18.0),
            (1800.0, -34.0),
            (1850.0, -46.0),
            (1950.0, -47.0),
            (2050.0, -47.5),
            (2150.0, -48.0),
        ];
        stats.bahn_raeum_laengs_m = Some(1700.0);
        stats.bahn_raeum_kurs_diff = Some(-35.0); // Linkskurve
                                                  // Die Live-Bestimmung ist leer — genau der Normalfall.
        stats.bahn_raeum_seite = None;

        let f = bahn_felder(&stats, Some("A320"), None);
        assert_eq!(
            f.clearance_side.as_deref(),
            Some("left"),
            "die eindeutige Linkskurve geht im parallelen Weiterrollen verloren"
        );

        // Gegenprobe zur Gegenprobe: Ohne Ausschwenken bleibt es leer.
        //
        // `FlightStats` ist nicht `Clone` (es haelt Kanaele), deshalb wird
        // hier dieselbe Struktur weiterverwendet und nur die Spur getauscht.
        stats.bahn_spur = vec![
            (1700.0, -5.0),
            (1800.0, -5.2),
            (1900.0, -5.4),
            (2000.0, -5.6),
        ];
        assert!(
            bahn_felder(&stats, Some("A320"), None)
                .clearance_side
                .is_none(),
            "ohne Querbewegung darf keine Richtung behauptet werden"
        );
    }

    /// Eine langsame Ausfahrt bekommt ihren Bewertungsendpunkt.
    ///
    /// # Der Befund
    ///
    /// `bahn_fenster_zu` fiel, sobald die Fahrt unter sechzig Knoten sank.
    /// Jeder spaetere Tick kehrte danach zurueck, BEVOR der Kurswechsel
    /// geprueft wurde.
    ///
    /// Eine ganz normale Folge — 65 kt geradeaus, 55 kt geradeaus, bei
    /// 25 kt abbiegen — setzte damit nie `bahn_raeum_laengs_m`.
    /// `scoring_cutoff_m` und die Ausfahrtsseite blieben leer, und die
    /// Anzeige nahm ersatzweise die spaetere Bahnkante: genau den Wert,
    /// gegen den `scoring_cutoff_m` ueberhaupt gebaut wurde.
    ///
    /// Das ist der Normalfall, nicht die Ausnahme: Wer unter sechzig
    /// Knoten faellt und erst danach abbiegt, tut das auf jedem grossen
    /// Platz.
    #[test]
    fn langsame_ausfahrt_bekommt_ihren_raeumpunkt() {
        let flight = flight_fixture("EDDH");
        {
            let mut stats = flight.stats.lock().expect("stats");
            stats.phase = FlightPhase::Landing;
            stats.landing_at = Some(Utc::now());
            stats.landing_lat = Some(53.636_011);
            stats.landing_lon = Some(9.999_656);
            stats.rollout_last_lat = Some(53.636_011);
            stats.rollout_last_lon = Some(9.999_656);
            // Der Kurs beim Aufsetzen — die Ausfahrt misst sich daran.
            stats.landing_heading_true_deg = Some(230.21);
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0,
                surface: "ASP".to_string(),
                threshold_lat: 53.636_011,
                threshold_lon: 9.999_656,
                end_lat: 53.619_958,
                end_lon: 9.967_167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
        }

        // 65 kt geradeaus → 55 kt geradeaus (Fenster schliesst) → 25 kt
        // abbiegen. Der Kurswechsel kommt also ERST NACH dem 60-kt-Tick.
        let folge = [
            (53.630_234_f64, 9.988_032_f64, 65.0_f32, 230.21_f32),
            (53.627_925, 9.983_381, 55.0, 230.21),
            (53.625_854, 9.979_381, 25.0, 195.0), // 35 Grad Kursaenderung
        ];
        for (lat, lon, gs, kurs) in folge {
            let snap = SimSnapshot {
                lat,
                lon,
                groundspeed_kt: gs,
                heading_deg_true: kurs,
                on_ground: true,
                ..Default::default()
            };
            step_flight_at(&flight, &snap, Utc::now());
        }

        let stats = flight.stats.lock().expect("stats");
        let raeum = stats
            .bahn_raeum_laengs_m
            .expect("kein Raeumpunkt — der Kurswechsel nach dem 60-kt-Tick wurde nie geprueft");
        assert!(
            (1600.0..1900.0).contains(&raeum),
            "Raeumpunkt bei {raeum} m — erwartet um 1750 m"
        );
        assert!(
            stats.bahn_raeum_gs_kt.is_some(),
            "die Fahrt beim Ausschwenken fehlt"
        );

        // Die SEITE wird abgeleitet, nicht roh gespeichert: Im Moment des
        // Kurswechsels ist die Querbewegung erst wenige Meter gross, und
        // die Bestimmung verlangt zwei uebereinstimmende Groessen. Deshalb
        // ueber `bahn_felder`, das die ganze Spur sieht.
        //
        // Kurs 230 -> 195 Grad ist eine LINKSkurve, und der Querversatz
        // geht ins Negative — beide sagen links. Der erste Anlauf dieses
        // Tests erwartete „right" und hatte schlicht die Vorzeichen
        // verwechselt.
        let felder = bahn_felder(&stats, Some("A320"), None);
        assert_eq!(
            felder.clearance_side.as_deref(),
            Some("left"),
            "die Ausfahrtsseite fehlt oder zeigt falsch"
        );
        assert!(
            felder.scoring_cutoff_m.is_some(),
            "das Bewertungsende fehlt — genau der gemeldete Fehler"
        );
    }

    /// Der NORMALE Landeweg schreibt die Rollspur fort — bis zum Ende.
    ///
    /// # Der Regress, den dieser Test faengt
    ///
    /// `bahndisziplin_tick` stand urspruenglich IN `rollout_tick`. Damit
    /// endete die Spur bei vierzig Knoten, wo `rollout_finalized` faellt —
    /// obwohl ihr Vertrag sie bis fuenf Knoten laufen laesst.
    ///
    /// Beim Herausziehen landete der Aufruf nur im **Sampler-Pfad**, und
    /// der kehrt bei `FlightPhase::Landing` ausdruecklich zurueck. Folge:
    /// Bei jeder normalen Landung wurde ueberhaupt keine Spur mehr
    /// aufgezeichnet — schlimmer als der Zustand davor.
    ///
    /// Die damalige Gegenprobe lief ueber `TaxiIn` und sah diesen Weg
    /// nicht. Dieser Test geht durch `step_flight_at`, also den echten
    /// Hauptpfad, und prueft alle drei Fahrtbereiche.
    #[test]
    fn normale_landung_zeichnet_die_spur_bis_zum_ende_auf() {
        let flight = flight_fixture("EDDH");
        {
            let mut stats = flight.stats.lock().expect("stats");
            stats.phase = FlightPhase::Landing;
            stats.landing_at = Some(Utc::now());
            stats.landing_lat = Some(53.636_011);
            stats.landing_lon = Some(9.999_656);
            stats.rollout_last_lat = Some(53.636_011);
            stats.rollout_last_lon = Some(9.999_656);
            stats.runway_match = Some(runway::RunwayMatch {
                airport_ident: "EDDH".to_string(),
                runway_ident: "23".to_string(),
                heading_true_deg: 230.21,
                length_ft: 10663.0,
                width_ft: 151.0,
                surface: "ASP".to_string(),
                threshold_lat: 53.636_011,
                threshold_lon: 9.999_656,
                end_lat: 53.619_958,
                end_lon: 9.967_167,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 720.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
        }

        // Drei Punkte entlang EDDH 23, jeweils leicht links der Mitte —
        // ausgerechnet, weil das geo-Crate nur Abstaende kann.
        //   1000 m / 80 kt   — Messfenster offen
        //   1500 m / 39 kt   — unter ROLLOUT_EXIT_GS_KT, Spur laeuft weiter
        //   1800 m / 12 kt   — kurz vor dem Ende
        let punkte = [
            (53.630_236_f64, 9.988_105_f64, 80.0_f32),
            (53.627_335, 9.982_235, 39.0),
            (53.625_594, 9.978_713, 12.0),
        ];
        let mut gesehen = Vec::new();
        for (lat, lon, gs) in punkte {
            let snap = SimSnapshot {
                lat,
                lon,
                groundspeed_kt: gs,
                on_ground: true,
                ..Default::default()
            };
            step_flight_at(&flight, &snap, Utc::now());
            let stats = flight.stats.lock().expect("stats");
            gesehen.push((gs, stats.bahn_spur.len()));
        }

        // Nach dem ersten Punkt muss aufgezeichnet sein.
        assert!(
            gesehen[0].1 > 0,
            "ueber 40 kt wird nichts aufgezeichnet: {gesehen:?}"
        );
        // Und danach WEITER — hier hing der Regress.
        assert!(
            gesehen[1].1 > gesehen[0].1,
            "bei 39 kt bricht die Spur ab (normaler Landeweg): {gesehen:?}"
        );
        assert!(
            gesehen[2].1 > gesehen[1].1,
            "bei 12 kt bricht die Spur ab: {gesehen:?}"
        );
    }

    /// Minimal ActiveFlight for `correlate_touchdown_runway` (only
    /// `navdata` + `arr_airport` are read by it).
    /// Eine Szenerie mit `rollwege` Rollwegen zu je 24 Stuetzpunkten.
    /// OMDB hat in der MSFS-Szenerie 3137 (AIB 431, 13.10.2026).
    fn szenerie_mit_rollwegen(icao: &str, rollwege: usize) -> sim_core::szenerie::SzenerieFlughafen {
        sim_core::szenerie::SzenerieFlughafen {
            icao: icao.to_string(),
            rollwege: (0..rollwege)
                .map(|i| sim_core::szenerie::SzenerieRollweg {
                    name: format!("T{i}"),
                    punkte: (0..24)
                        .map(|k| (25.24 + i as f64 * 1e-6, 55.34 + k as f64 * 1e-4))
                        .collect(),
                })
                .collect(),
            quelle: "msfs".into(),
            ..Default::default()
        }
    }

    /// Der Aufsetz-Takt von `step_flight_at` an einem Platz mit `rollwege`
    /// Rollwegen: die laengste Haltezeit der Flugdaten-Sperre (bestes von
    /// drei Laeufen, gegen Ausreisser der Maschine).
    fn aufsetz_takt_haltezeit(rollwege: usize) -> std::time::Duration {
        let mut beste = std::time::Duration::MAX;
        for _ in 0..3 {
            let flight = flight_fixture("OMDB");
            let sz = Arc::new(szenerie_mit_rollwegen("OMDB", rollwege));
            {
                let mut st = flight.stats.lock().expect("stats");
                st.phase = FlightPhase::Final;
                st.was_on_ground = Some(false);
                st.szenerie_auskunft = Some(Arc::clone(&sz));
                st.ziel_szenerie_auskunft = Some(Arc::clone(&sz));
            }
            let snap = SimSnapshot {
                lat: 25.2528,
                lon: 55.3644,
                heading_deg_true: 300.0,
                groundspeed_kt: 140.0,
                on_ground: true,
                simulator: sim_core::Simulator::Msfs2024,
                ..Default::default()
            };
            stats_sperre::test_messung::beginnen();
            let neue_phase = step_flight_at(&flight, &snap, Utc::now());
            let abschnitte = stats_sperre::test_messung::abschnitte();
            let gemessen = stats_sperre::test_messung::beenden();
            // v2.0.18: Der Aufsetz-Takt meldet seine Abschnitte — sonst
            // nennt der Waechter bei einer langen Haltezeit nur
            // `step_flight_at`, eine Funktion ueber 2500 Zeilen.
            assert!(
                abschnitte.iter().any(|a| {
                    ["aufsetzen_sinkrate", "aufsetzen_bahn", "note"]
                        .iter()
                        .all(|name| a.contains(name))
                }),
                "der Aufsetz-Takt meldet seine Zwischenstaende nicht: {abschnitte:?}"
            );
            assert_eq!(
                neue_phase,
                Some(FlightPhase::Landing),
                "Vorrichtung: das war nicht der Aufsetz-Takt"
            );
            let st = flight.stats.lock().expect("stats");
            assert!(
                st.szenerie_auskunft.as_ref().is_some_and(|a| Arc::ptr_eq(a, &sz)),
                "die Szenerie wurde beim Aufsetzen ersetzt oder kopiert statt geteilt"
            );
            let laengste = gemessen
                .iter()
                .map(|(_, d)| *d)
                .max()
                .expect("Vorrichtung: keine Haltezeit gemessen");
            beste = beste.min(laengste);
        }
        beste
    }

    /// ⚠ Der Aufsetz-Takt haelt die Flugdaten-Sperre nicht in
    /// Flughafengroesse.
    ///
    /// AIB 431 (OMDB, 13.10.2026): Seit 2.0.14 hatte jeder Flug genau eine
    /// Luecke im Aufsetz-Abtaster kurz nach dem Aufsetzen, und sie wuchs mit
    /// dem Platz — Mumbai 58 ms, Dubai 129 ms. Unter der Sperre wurde unter
    /// anderem die ganze Szenerie kopiert. Verglichen wird ein kleiner Platz
    /// mit einem zehnmal Dubai grossen: Waechst die Haltezeit mit, arbeitet
    /// jemand in Flughafengroesse unter der Sperre. Der Vergleich statt
    /// einer festen Grenze haelt den Test unabhaengig von der Maschine.
    #[test]
    fn der_aufsetz_takt_haelt_die_flugdaten_sperre_nicht_in_flughafengroesse() {
        let klein = aufsetz_takt_haltezeit(10);
        let riesig = aufsetz_takt_haltezeit(32_000);
        eprintln!("Haltezeit Aufsetz-Takt: {klein:?} bei 10 Rollwegen, {riesig:?} bei 32.000");
        // Gegenprobe am Stand 2.0.16 (Debug-Bau): 0,3 ms → 4,8 ms. Erlaubt
        // ist hier das Doppelte plus 1 ms Rauschen.
        assert!(
            riesig < klein * 2 + std::time::Duration::from_millis(1),
            "Haltezeit waechst mit dem Platz: {klein:?} bei 10 Rollwegen, \
             {riesig:?} bei 32.000 — unter der Flugdaten-Sperre wird in \
             Flughafengroesse gearbeitet"
        );
    }

    /// ⚠ Ein schon abgeholter Plugin-Aufsetzer geht nicht verloren, wenn
    /// keine Auswertung mehr annimmt (Codex-Nachpruefung 2.0.17).
    ///
    /// Der Adapter gibt ihn nur einmal heraus. Hier ist die Auswertung schon
    /// weg (Empfaenger verworfen), als der Messfaden ihn abholt: Er muss fuer
    /// denselben Flug zurueckgelegt werden und beim naechsten Messfaden als
    /// Erstes wieder herauskommen — und nur fuer DIESEN Flug.
    #[test]
    fn ein_abgeholter_plugin_aufsetzer_wird_ohne_auswertung_zurueckgelegt() {
        let mut flight = flight_fixture("OMDB");
        flight.pirep_id = "zuruecklegen-test".into();
        let flight = Arc::new(flight);
        let diagnose = Mutex::new(SamplerDiagnose::default());
        let (an_auswertung, messungen) = std::sync::mpsc::channel::<AufsetzMessung>();
        drop(messungen);
        let lebt = AtomicBool::new(true);
        let mut abgeholt = Some(
            serde_json::from_str::<sim_xplane::PremiumTouchdown>(
                r#"{"captured_vs_fpm":-212.0,"captured_g_normal":1.31}"#,
            )
            .expect("Aufsetzpaket"),
        );
        aufsetz_messen(
            &flight,
            0,
            &diagnose,
            &an_auswertung,
            &lebt,
            || {
                Some(SimSnapshot {
                    lat: 25.2528,
                    lon: 55.3644,
                    ..Default::default()
                })
            },
            || abgeholt.take(),
        );
        assert!(abgeholt.is_none(), "Vorrichtung: der Aufsetzer wurde nie abgeholt");
        assert!(
            zurueckgelegten_aufsetzer_nehmen("anderer-flug").is_none(),
            "ein zurueckgelegter Aufsetzer kommt bei einem anderen Flug heraus"
        );
        let zurueck = zurueckgelegten_aufsetzer_nehmen("zuruecklegen-test")
            .expect("der abgeholte Plugin-Aufsetzer ist verloren");
        assert_eq!(zurueck.captured_vs_fpm, -212.0);
        assert!(
            zurueckgelegten_aufsetzer_nehmen("zuruecklegen-test").is_none(),
            "zurueckgelegt heisst einmal, nicht dauerhaft"
        );
    }

    /// ⚠ Der Messfaden misst weiter, waehrend die Flugdaten-Sperre gehalten
    /// wird (v2.0.17).
    ///
    /// Bis 2.0.16 nahm der Abtaster fuer jeden Messwert `flight.stats` — wer
    /// sie hielt, hielt die Messung an. Hier haelt der Test die Sperre, bis
    /// zehn Messwerte da sind (hoechstens 2 s; Soll: 20 je 400 ms).
    #[test]
    fn der_messfaden_misst_weiter_waehrend_die_flugdaten_sperre_gehalten_wird() {
        let flight = Arc::new(flight_fixture("OMDB"));
        let diagnose = Arc::new(Mutex::new(SamplerDiagnose::default()));
        let (an_auswertung, messungen) = std::sync::mpsc::channel();
        let gehalten = flight.stats.lock().expect("stats");
        let (f, d) = (Arc::clone(&flight), Arc::clone(&diagnose));
        let lebt = Arc::new(AtomicBool::new(true));
        let l = Arc::clone(&lebt);
        let faden = std::thread::spawn(move || {
            aufsetz_messen(
                &f,
                0,
                &d,
                &an_auswertung,
                &l,
                || {
                    Some(SimSnapshot {
                        lat: 25.2528,
                        lon: 55.3644,
                        altitude_agl_ft: 40.0,
                        ..Default::default()
                    })
                },
                || None,
            )
        });
        // Startschranke: Die Uhr laeuft erst, wenn der Faden misst — ein
        // spaet eingeplanter Faden auf einer langsamen CI ist kein Befund
        // (Codex-Abnahme 2.0.17, P3).
        let erster = messungen
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("der Messfaden liefert gar nichts, waehrend die Sperre gehalten wird");
        // Bis zu 2 s (Soll: 20 Messwerte je 400 ms); ein ausgelasteter
        // macOS-Laeufer schaffte in festen 400 ms nur 9 (CI 30.10.2026).
        // Wartete der Faden auf die Sperre, kaeme hier kein zweiter Wert.
        let mut waehrenddessen: Vec<AufsetzMessung> = vec![erster];
        let frist = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while waehrenddessen.len() < 10 && std::time::Instant::now() < frist {
            if let Ok(m) = messungen.recv_timeout(std::time::Duration::from_millis(50)) {
                waehrenddessen.push(m);
            }
        }
        drop(gehalten);
        // Ende durch Abmelden der Auswertung, NICHT durch `stop`: Ist die
        // Auswertung weg, muss der Messfaden enden, statt weiter Plugin-
        // Aufsetzer abzuholen (Codex-Abnahme 2.0.17, P2).
        lebt.store(false, Ordering::SeqCst);
        let abgemeldet = std::time::Instant::now();
        while !faden.is_finished() && abgemeldet.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let beendet = faden.is_finished();
        flight.stop.store(true, Ordering::Relaxed);
        faden.join().expect("Messfaden abgestuerzt");
        assert!(beendet, "der Messfaden laeuft weiter, obwohl die Auswertung abgemeldet ist");
        assert!(
            waehrenddessen.len() >= 10,
            "nur {} Messwerte in 2 s gehaltener Sperre — der Messfaden \
             wartet auf die Flugdaten",
            waehrenddessen.len()
        );
        assert!(
            waehrenddessen.windows(2).all(|w| w[0].now < w[1].now),
            "die Messwerte tragen ihren eigenen Zeitpunkt nicht in der Reihenfolge der Messung"
        );
    }

    /// Pushback-Phase durch die ECHTE Tickfunktion, Takt alle 5 s (wie im
    /// Feld; kürzere Abstände sähen nach Neuladen aus). `ticks`: (gs, vz,
    /// Triebwerke, Pushback-Status). Liefert die Phase nach jedem Takt.
    fn pushback_ablauf(
        ticks: &[(f32, Option<f32>, u8, Option<u8>)],
        saw_active: bool,
        klemmt: bool,
    ) -> Vec<FlightPhase> {
        let flight = flight_fixture("DAAJ");
        let t0 = Utc::now() - chrono::Duration::hours(1);
        {
            let mut st = flight.stats.lock().expect("stats");
            st.phase = FlightPhase::Pushback;
            st.block_off_at = Some(t0);
            st.saw_pushback_state_active = saw_active;
            st.pushback_status_klemmt = klemmt;
            st.pushback_status_gesehen = true;
            st.was_on_ground = Some(true);
        }
        ticks
            .iter()
            .enumerate()
            .map(|(i, &(gs, vz, eng, push))| {
                let snap = SimSnapshot {
                    lat: 49.4987,
                    lon: 11.0669,
                    on_ground: true,
                    groundspeed_kt: gs,
                    velocity_body_z_fps: vz,
                    engines_running: eng,
                    pushback_state: push,
                    parking_brake: true,
                    fuel_total_kg: 14_900.0,
                    ..SimSnapshot::default()
                };
                step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i as i64));
                flight.stats.lock().expect("stats").phase
            })
            .collect()
    }

    /// ⚠ Joel, MSFS 2020 (15.10.2026): Der Pushback-Status bleibt dauerhaft 0
    /// („Pushback läuft"). Bis hier hing die Phase bis zur Luftrettung auf
    /// Pushback — 31 Flüge in 30 Tagen. Die Werte sind die echten aus
    /// CFG 3230 EDDN → DAAJ: Zurückschieben mit 2,7 kt (vz −4,5), Anrollen
    /// mit 3,5 kt, dann Rollen mit 19 kt (vz +32,5).
    #[test]
    fn haengender_pushback_status_rollen_mit_eigenen_triebwerken_wird_taxi_out() {
        let mut ticks = vec![
            (2.7, Some(-4.5), 0, Some(0)),
            (2.6, Some(-4.4), 3, Some(0)),
            (0.0, Some(0.0), 4, Some(0)),
            (3.5, Some(5.9), 4, Some(0)),
        ];
        ticks.extend(std::iter::repeat((19.3, Some(32.5), 4, Some(0))).take(6));
        let phasen = pushback_ablauf(&ticks, true, true);
        // Zurückschieben und langsames Anrollen bleiben Pushback.
        assert!(phasen[..4].iter().all(|p| *p == FlightPhase::Pushback), "{phasen:?}");
        // Ab 15 s eigenem Vorwärtsrollen über 8 kt: Rollen.
        assert_eq!(phasen.last(), Some(&FlightPhase::TaxiOut), "{phasen:?}");
        let erst = phasen.iter().position(|p| *p == FlightPhase::TaxiOut).unwrap();
        assert!(erst >= 4 + 3, "zu früh umgeschaltet: {phasen:?}");
    }

    /// Gegenproben: Ohne bekannte Fahrtrichtung, beim schnellen
    /// Zurückschieben und bei Status 3 (das regelt `tug_done` mit Stillstand
    /// und Wartezeit) greift der neue Weg NICHT.
    #[test]
    fn haengender_pushback_status_gegenproben() {
        let ohne_richtung = pushback_ablauf(&[(19.3, None, 4, Some(0)); 12], true, true);
        assert!(ohne_richtung.iter().all(|p| *p == FlightPhase::Pushback), "{ohne_richtung:?}");
        let rueckwaerts = pushback_ablauf(&[(10.0, Some(-17.0), 4, Some(0)); 12], true, true);
        assert!(rueckwaerts.iter().all(|p| *p == FlightPhase::Pushback), "{rueckwaerts:?}");
        // MSFS 2024: Status 3 nach aktivem Pushback, aber noch kein
        // Stillstand — der bisherige Weg wartet, der neue mischt sich nicht ein.
        let status_drei = pushback_ablauf(&[(19.3, Some(32.5), 4, Some(3)); 12], true, true);
        assert!(status_drei.iter().all(|p| *p == FlightPhase::Pushback), "{status_drei:?}");
        // Echter Vorwärts-Schlepp auf einem funktionierenden Simulator: Der
        // Status war am Gate 3 (klemmt nicht), der Schlepper zieht mit
        // laufenden Triebwerken vorwärts — bleibt Pushback (Codex 15.10.2026).
        let schlepp = pushback_ablauf(&[(10.0, Some(17.0), 2, Some(0)); 12], true, false);
        assert!(schlepp.iter().all(|p| *p == FlightPhase::Pushback), "{schlepp:?}");
    }

    /// Klemmt der Status? Nur wenn schon der ERSTE Wert am Gate aktiv war —
    /// nicht, wenn der Schlepper später (noch in Boarding) ankoppelt.
    #[test]
    fn pushback_status_klemmt_nur_wenn_schon_der_erste_wert_aktiv_ist() {
        let am_gate = |folge: &[Option<u8>]| {
            let flight = flight_fixture("DAAJ");
            flight.stats.lock().expect("stats").phase = FlightPhase::Boarding;
            let t0 = Utc::now() - chrono::Duration::minutes(10);
            for (i, status) in folge.iter().enumerate() {
                let snap = SimSnapshot {
                    lat: 49.4987,
                    lon: 11.0669,
                    on_ground: true,
                    pushback_state: *status,
                    parking_brake: true,
                    fuel_total_kg: 14_900.0,
                    ..SimSnapshot::default()
                };
                step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i as i64));
            }
            let klemmt = flight.stats.lock().expect("stats").pushback_status_klemmt;
            klemmt
        };
        assert!(am_gate(&[Some(0), Some(0)]), "MSFS 2020 bei Joel: von Anfang an 0");
        assert!(
            !am_gate(&[Some(3), Some(3), Some(0)]),
            "funktionierender Simulator: erst 3, dann koppelt der Schlepper an"
        );
        assert!(!am_gate(&[None, None]), "ohne Pushback-Status (X-Plane)");
        assert!(
            !am_gate(&[None, Some(0), Some(0)]),
            "erst kein Wert, dann koppelt der Schlepper an"
        );
    }

    fn flight_fixture(arr: &str) -> ActiveFlight {
        ActiveFlight {
            pirep_id: "test-pirep".into(),
            bid_id: 0,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            airline_icao: String::new(),
            airline_logo_url: None,
            planned_registration: String::new(),
            aircraft_icao: "C172".into(),
            aircraft_name: String::new(),
            flight_number: "1".into(),
            dpt_airport: "EDDF".into(),
            arr_airport: arr.to_string(),
            planned_flight_time_min: None,
            plan_strecke_nm: None,
            fares: Vec::new(),
            stats: stats_sperre::GemesseneSperre::new(FlightStats::new()),
            stop: AtomicBool::new(false),
            was_just_resumed: AtomicBool::new(false),
            streamer_spawned: AtomicBool::new(false),
            cancelled_remotely: AtomicBool::new(false),
            position_outbox: Mutex::new(std::collections::VecDeque::new()),
            positionen_nachsenden: AtomicBool::new(false),
            generation: std::sync::atomic::AtomicU64::new(0),
            start_sperre: Mutex::new(()),
            phpvms_worker_spawned: AtomicBool::new(false),
            touchdown_sampler_spawned: AtomicBool::new(false),
            connection_state: std::sync::atomic::AtomicU8::new(CONN_STATE_LIVE),
            navdata: Mutex::new(NavdataCache::default()),
        }
    }

    // ---- flight_info() callsign exposure (pilot report Ralf T., GSG0016,
    // 2026-07-28: Personal/Free-Flight bookings with flight_number=0
    // rendered "CFG0" because the UI never saw the real callsign) ----

    #[test]
    fn flight_info_exposes_bid_callsign() {
        let mut flight = flight_fixture("GCLP");
        flight.flight_number = "0".into();
        flight.bid_callsign = Some("7ME".into());
        let info = flight_info(&flight, false, None);
        assert_eq!(info.callsign, Some("7ME".to_string()));
        // flight_number itself is untouched — callsign is an ADDITIONAL
        // field, the UI decides the precedence (resolveFlightIdent()).
        assert_eq!(info.flight_number, "0");
    }

    #[test]
    fn flight_info_callsign_none_for_ordinary_scheduled_flights() {
        let flight = flight_fixture("GCLP"); // bid_callsign: None (fixture default)
        let info = flight_info(&flight, false, None);
        assert_eq!(info.callsign, None);
    }

    // ---- resolve_flight_ident() (used by build_landing_record — mirrors
    // the TS resolveFlightIdent() in src/lib/callsign.ts / callsign.test.ts) ----

    #[test]
    fn resolve_flight_ident_prefers_non_empty_callsign() {
        assert_eq!(resolve_flight_ident("0", Some("7ME")), "7ME");
    }

    /// QS 06.10.2026: Der PIREP trägt die drei Werte, die der Client-
    /// Datensatz zeigt und die Webapp sonst nicht hatte — aus denselben
    /// Quellen wie `build_landing_record`.
    #[test]
    fn pirep_traegt_flugkennung_spitzen_vs_und_gemessene_g() {
        let mut flight = flight_fixture("GCLP");
        flight.airline_icao = "CFG".into();
        flight.flight_number = "0".into();
        flight.bid_callsign = Some("7ME".into());
        flight.planned_registration = "EC-MHA".into();
        {
            let mut st = flight.stats.lock().unwrap();
            st.landing_peak_vs_fpm = Some(-204.0);
            // Gemessene bewertete G (Analyse) — die Zahl, die der Datensatz zeigt.
            st.landing_analysis = Some(serde_json::json!({ "scored_g": 1.42 }));
            for (t_ms, bank_deg) in [(-400, 3.0), (-60, -1.5), (300, 4.0)] {
                st.touchdown_profile.push(TouchdownProfilePoint {
                    t_ms,
                    vs_fpm: -150.0,
                    g_force: 1.1,
                    agl_ft: 0.0,
                    on_ground: t_ms >= 0,
                    heading_true_deg: 90.0,
                    groundspeed_kt: 130.0,
                    indicated_airspeed_kt: 132.0,
                    pitch_deg: 4.0,
                    bank_deg,
                });
            }
        }
        let body = api_client::FileBody::default();
        let p = build_pirep_payload(&flight, &body, "GCLP", "GCLP");
        assert_eq!(p.flight_ident.as_deref(), Some("7ME"));
        // `flight_number` bleibt wie bisher („CFG 0") — additiv.
        assert_eq!(p.flight_number, format_callsign("CFG", "0"));
        assert_eq!(p.landing_peak_vs_fpm, Some(-204.0));
        // Querneigung: der Profilpunkt am nächsten an t = 0, wie im Datensatz.
        assert_eq!(p.landing_bank_deg, Some(-1.5));
        // Fenster ausreichend → Feld fehlt (nur `true` geht hinaus).
        assert_eq!(p.fenster_unzureichend, None);
        // Ein fester Wert, nicht „dieselbe Funktion wie das Feld" (QS:
        // sonst prüfte der Test None == None).
        assert_eq!(p.landing_scored_g_force, Some(1.42));
        // Runde 6: Kennzeichen wie im Datensatz (aus der Buchung).
        assert_eq!(p.aircraft_registration.as_deref(), Some("EC-MHA"));
        // Ohne Werte: Felder fehlen im JSON (ältere Empfänger unverändert).
        let mut leer = flight_fixture("GCLP");
        leer.flight_number = String::new();
        leer.planned_registration = String::new();
        let j = serde_json::to_value(build_pirep_payload(&leer, &body, "GCLP", "GCLP")).unwrap();
        assert!(j.get("flight_ident").is_none() && j.get("landing_peak_vs_fpm").is_none());
        assert!(j.get("aircraft_registration").is_none());
    }

    /// P1/9 (22.10.2026): Der MQTT-PIREP trägt die Landerate GERUNDET, wie
    /// Client-Anzeige, MQTT-Touchdown und phpVMS-Feld. Auf dem alten Stand
    /// (`r as i32`) kam hier −559 statt −560 heraus.
    #[test]
    fn pirep_landing_vs_fpm_ist_gerundeter_anzeigewert() {
        let flight = flight_fixture("GCLP");
        {
            let mut st = flight.stats.lock().unwrap();
            st.landing_rate_fpm = Some(-559.6);
            st.landing_peak_vs_fpm = Some(-559.6);
        }
        let erwartet = flight
            .stats
            .lock()
            .unwrap()
            .canonical_landing_rate_fpm()
            .map(|v| v.round() as i32);
        assert_eq!(erwartet, Some(-560), "Vorrichtung: kanonische Rate muss −559,6 sein");
        let mut body = api_client::FileBody::default();
        body.landing_rate = Some(-559.6);
        let p = build_pirep_payload(&flight, &body, "GCLP", "GCLP");
        assert_eq!(p.landing_vs_fpm, Some(-560));
    }

    #[test]
    fn resolve_flight_ident_falls_back_when_callsign_none() {
        assert_eq!(resolve_flight_ident("1434", None), "1434");
    }

    #[test]
    fn resolve_flight_ident_falls_back_when_callsign_empty() {
        assert_eq!(resolve_flight_ident("1434", Some("")), "1434");
    }

    #[test]
    fn resolve_flight_ident_falls_back_when_callsign_whitespace_only() {
        assert_eq!(resolve_flight_ident("1434", Some("   ")), "1434");
    }

    #[test]
    fn resolve_flight_ident_trims_padded_callsign() {
        // Regression: an earlier inline version of this filter kept the
        // UNTRIMMED string on success, diverging from the TS helper's
        // `callsign?.trim() || flightNumber` for padded input.
        assert_eq!(resolve_flight_ident("0", Some("  7ME  ")), "7ME");
    }

    // ---- stamp_touchdown_metadata ----

    /// **Über die echte Aufrufkette**, nicht per Handaufruf: Ein Ausweichflug
    /// muss beim Stempeln als solcher erkannt werden.
    ///
    /// Der Prüfer der vierten Runde hat belegt, dass die Erkennung vorher NIE
    /// feuerte: Die Auswertung fror ein, bevor `landing_lat`/`landing_lon`
    /// gesetzt waren, und `sprit_ist_ausweichflug` liest genau diese Felder.
    /// Ein Ausweichflug wäre als Normalflug eingefroren worden — mit Phasen
    /// gegen einen Plan, der nie geflogen wurde. Die bisherigen Tests riefen
    /// die Funktionen von Hand auf und bewiesen darüber nichts.

    /// Die Kachel „Landesprit" und die Sprit-Auswertung tragen dieselbe Zahl.
    ///
    /// Der FSM-Stempel überschreibt `landing_fuel_kg` absichtlich mit seinem
    /// späteren Tick, während die Auswertung schon eingefroren ist. Ohne das
    /// Zurückschreiben stünden im selben Tab zwei Zahlen für denselben
    /// Landesprit — getrennt durch den Verbrauch im Umkehrschub (QS-Abnahme).
    #[test]
    fn sprit_landesprit_ist_ueberall_dieselbe_zahl() {
        let mut st = FlightStats::default();
        st.takeoff_fuel_kg = Some(40_919.0);
        st.planned_burn_kg = Some(21_218.0);
        st.planned_reserve_kg = Some(4_916.0);
        st.plan_bis_setzen(19_353.0);
        st.sprit_vergleichspunkt_kg = Some(22_231.0);
        st.planned_arr_ref_pos = Some((48.3538, 11.7861));
        // Geht als `zeit_unter_schwelle_min` in die Auswertung ein — der
        // Hebel fuer die Gegenprobe ganz unten.
        st.sprit_schwelle_ft = Some(9_487.0);
        st.sprit_zeit_unter_schwelle_s = 948.0;

        // Erster Stempel (Sampler) friert ein.
        let mut snap = rollout_snap();
        snap.lat = 48.3538;
        snap.lon = 11.7861;
        snap.fuel_total_kg = 16_770.0;
        stamp_touchdown_metadata(&mut st, &snap, td_at(), None);
        let eingefroren = st.sprit_auswertung.clone().expect("Auswertung");
        assert_eq!(eingefroren.landing_fuel_kg, Some(16_770.0));

        // Zweiter Stempel (FSM) mit spaeterem Tick — Umkehrschub hat weiter
        // verbrannt. Der letzte Stempel gilt, die Auswertung wird neu
        // gebildet; sonst truege sie den Landesprit des ersten Ticks,
        // waehrend die Kachel den zweiten zeigt.
        snap.fuel_total_kg = 16_680.0;
        stamp_touchdown_metadata(&mut st, &snap, td_at(), None);
        let danach = st.sprit_auswertung.clone().expect("Auswertung");
        assert_eq!(st.landing_fuel_kg, Some(16_680.0), "Rohwert ungerundet");
        assert_ne!(danach, eingefroren, "der zweite Stempel muss durchschlagen");

        // Gegenprobe: Ein dritter Stempel mit demselben Landesprit rechnet
        // NICHT neu.
        //
        // Damit die Zusicherung etwas taugt, muss sich zwischendurch ETWAS
        // aendern, das in die Rechnung eingeht — sonst laege eine unbedingte
        // Neuberechnung aus unveraenderten `stats` bei exakt demselben Wert,
        // und der Test bliebe auch ohne den Riegel gruen (QS-Befund F3).
        // `sprit_zeit_unter_schwelle_s` laeuft beim Ausrollen weiter und
        // geht als `zeit_unter_schwelle_min` in die Auswertung ein.
        let vorher_min = st
            .sprit_auswertung
            .as_ref()
            .and_then(|a| a.zeit_unter_schwelle_min)
            .expect("Zeit unter Schwelle");
        assert_eq!(vorher_min, 15.8, "Vorbedingung: 948 s = 15,8 min");
        st.sprit_zeit_unter_schwelle_s += 600.0;
        stamp_touchdown_metadata(&mut st, &snap, td_at(), None);
        assert_eq!(
            st.sprit_auswertung
                .as_ref()
                .and_then(|a| a.zeit_unter_schwelle_min),
            Some(15.8),
            "gleicher Landesprit = eingefroren; 25,8 min hiesse: unbedingt neu gerechnet"
        );

        // Und die Kachel zeigt dieselbe Zahl wie „gelandet mit …".
        assert_eq!(
            st.landing_fuel_kg, danach.landing_fuel_kg,
            "Kachel und Sprit-Auswertung zeigen verschiedene Landespritwerte"
        );
    }

    #[test]
    fn sprit_ausweichflug_wird_beim_stempeln_erkannt() {
        let bauen = |ziel: (f64, f64), landung: (f64, f64)| {
            let mut st = FlightStats::default();
            st.takeoff_fuel_kg = Some(40_919.0);
            st.planned_burn_kg = Some(21_218.0);
            st.planned_reserve_kg = Some(4_916.0);
            st.plan_bis_setzen(19_353.0);
            st.sprit_vergleichspunkt_kg = Some(22_231.0);
            st.sprit_vergleichspunkt_odo_nm = Some(600.0);
            st.distance_nm = 782.0;
            // Vorbesetzen, sonst sind die Zusicherungen auf `schwelle_ft`
            // und `zeit_unter_schwelle_min` wirkungslos: `auswerten` filtert
            // beide ohnehin auf None, und der Test bliebe gruen, wenn der
            // Ausweich-Zweig sie gar nicht mehr nullte (QS-Abnahme, P3).
            st.sprit_schwelle_ft = Some(9_487.0);
            st.sprit_zeit_unter_schwelle_s = 948.0;
            st.planned_arr_ref_pos = Some(ziel);
            let mut snap = rollout_snap();
            snap.lat = landung.0;
            snap.lon = landung.1;
            snap.fuel_total_kg = 16_770.0;
            stamp_touchdown_metadata(&mut st, &snap, td_at(), None);
            st.sprit_auswertung.clone().expect("Auswertung fehlt")
        };

        // Auf dem geplanten Platz: Phasen und Plan-Reststrecke da.
        let normal = bauen((48.3538, 11.7861), (48.3538, 11.7861));
        assert!(normal.bis_sinkflug.is_some(), "Normalflug ohne Phasen");
        // Gegenprobe: im Normalfall sind Schwelle und Zeit DA — erst damit
        // sagt ihr Fehlen beim Ausweichflug etwas aus.
        assert_eq!(normal.schwelle_ft, Some(9_487.0));
        assert_eq!(normal.zeit_unter_schwelle_min, Some(15.8));

        // Woanders aufgesetzt (EDDN statt EDDM, ~60 NM): keine Phasen gegen
        // einen Plan, der nicht geflogen wurde — und keine Schwelle, die zur
        // Höhe des geplanten Platzes gehört.
        let ausweich = bauen((48.3538, 11.7861), (49.4987, 11.0780));
        assert!(
            ausweich.bis_sinkflug.is_none() && ausweich.anflug.is_none(),
            "Ausweichflug zeigt Phasen gegen den nicht geflogenen Plan"
        );
        assert_eq!(ausweich.plan_strecke_anflug_nm, None);
        assert_eq!(ausweich.schwelle_ft, None);
        assert_eq!(ausweich.zeit_unter_schwelle_min, None);
        // Tatsachen bleiben.
        assert_eq!(ausweich.landing_fuel_kg, Some(16_770.0));
        assert!(matches!(
            ausweich.reserve,
            landing_scoring::sprit::Reserve::Intakt { .. }
        ));
    }

    /// Ein Wert aus dem Aufsetz-Puffer wird NIE durch einen spaeteren
    /// Schnappschuss ersetzt — unabhaengig davon, welcher Stempel zuerst
    /// laeuft.
    ///
    /// # Warum das ein eigener Fall ist
    ///
    /// Der Puffer haelt fuenf Sekunden. `stamp_touchdown_metadata` laeuft
    /// zweimal: erst aus dem Sampler, dann aus der FSM. Beim zweiten Mal
    /// kann die Aufsetz-Probe schon aus dem Ring gerollt sein, oder der
    /// Pfad uebergibt gar keinen Puffer (Resume nach Neustart). Ohne den
    /// Herkunfts-Merker wuerde der gute Wert des ersten Stempels dann still
    /// durch den Schnappschuss des zweiten ersetzt — also durch genau den
    /// Wert, wegen dem es den Puffer ueberhaupt gibt.
    #[test]
    fn puffer_schlaegt_snap_auch_beim_zweiten_stempel() {
        let mut stats = stats_with_buffer();
        let snap = rollout_snap();

        // Erster Stempel MIT Puffer: 92,0 kg aus der Aufsetz-Probe.
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        assert_eq!(stats.landing_fuel_kg, Some(92.0));
        assert!(stats.landing_fuel_puffer);

        // Zweiter Stempel OHNE Puffer (herausgerollt oder Resume). Der
        // Schnappschuss traegt 88,5 kg — der Puffer-Wert bleibt.
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), None);
        assert_eq!(
            stats.landing_fuel_kg,
            Some(92.0),
            "der spaetere Schnappschuss hat den Puffer-Wert verdraengt"
        );

        // Und die Auswertung wurde nicht angefasst, weil sich nichts
        // geaendert hat.
        let vorher = stats.sprit_auswertung.clone();
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), None);
        assert_eq!(stats.sprit_auswertung, vorher);
    }

    /// Ein Add-on mit toter Sprit-SimVar liefert 0,0 kg — das ist kein
    /// Messwert, sondern eine Fehlanzeige, und darf den brauchbaren
    /// Schnappschuss nicht verdraengen.
    ///
    /// Ein Flugzeug landet nicht mit leeren Tanks. Stuende hier `>= 0.0`,
    /// waere die 0 ein gueltiger Puffer-Wert mit Vorrang — und der Pilot
    /// saehe „gelandet mit 0 kg" samt unterschrittener Reserve, auf einem
    /// Flug, der in Wirklichkeit mit vollen Reserven aufgesetzt hat.
    #[test]
    fn puffer_mit_null_kg_verdraengt_den_snap_nicht() {
        let mut stats = FlightStats::default();
        // Puffer mit toter SimVar: alle drei Proben melden 0 kg.
        for (ms, boden) in [(-500i64, false), (0, true), (500, true)] {
            let mut probe = buf_sample(td_at() + chrono::Duration::milliseconds(ms), boden);
            probe.fuel_kg = 0.0;
            stats.snapshot_buffer.push_back(probe);
        }
        let snap = rollout_snap();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        assert_eq!(
            stats.landing_fuel_kg,
            Some(88.5),
            "0 kg aus dem Puffer hat den brauchbaren Schnappschuss verdraengt"
        );
        assert!(
            !stats.landing_fuel_puffer,
            "eine Fehlanzeige darf nicht als Puffer-Herkunft gelten"
        );
    }

    /// Gegenprobe: Ohne Puffer-Wert gilt der Schnappschuss — sonst haette
    /// ein wiederaufgenommener Flug gar keinen Landesprit.
    #[test]
    fn puffer_schlaegt_snap_aber_ohne_puffer_gilt_der_snap() {
        let mut stats = FlightStats::default();
        let snap = rollout_snap();
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), None);
        assert_eq!(stats.landing_fuel_kg, Some(88.5), "Rueckfall fehlt");
        assert!(
            !stats.landing_fuel_puffer,
            "ohne Puffer darf die Herkunft nicht behauptet werden"
        );

        // Und wenn der Puffer SPAETER doch noch kommt, gewinnt er.
        let mit_puffer = stats_with_buffer();
        let td_buf = touchdown_buffer_sample(&mit_puffer);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        assert_eq!(stats.landing_fuel_kg, Some(92.0));
        assert!(stats.landing_fuel_puffer);
    }

    #[test]
    fn stamp_uses_buffer_td_sample_not_live_snap() {
        let mut stats = stats_with_buffer();
        let snap = rollout_snap();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());

        // TD-instant values from the buffered first on-ground sample.
        assert_eq!(stats.landing_pitch_deg, Some(4.5));
        assert_eq!(stats.landing_bank_deg, Some(-1.5));
        assert_eq!(stats.landing_speed_kt, Some(100.0));
        assert_eq!(stats.landing_groundspeed_kt, Some(95.0));
        assert_eq!(stats.landing_lat, Some(51.0));
        assert_eq!(stats.landing_lon, Some(12.0));
        assert_eq!(stats.landing_heading_true_deg, Some(270.0));
        // Magnetic heading = buffer-true − live magvar (280 − 277 = 3).
        assert_eq!(stats.landing_heading_deg, Some(267.0));
        // v1.7.36: Auch der Sprit kommt jetzt aus dem Puffer. Bis dahin
        // stand hier „Fuel always from the live snap (no buffered fuel
        // field)" — der Test sicherte die Luecke zu, statt sie zu melden.
        // Der Schnappschuss zeigt 88,5 kg, also 3,5 kg weniger: das ist
        // Umkehrschub, verbrannt NACH dem Aufsetzen.
        assert_eq!(stats.landing_fuel_kg, Some(92.0));
        assert!(stats.landing_fuel_puffer, "Herkunft muss vermerkt sein");
        // Sideslip = atan2(5, 50) in degrees.
        let slip = stats.touchdown_sideslip_deg.expect("sideslip");
        assert!((slip - 5.0f32.atan2(50.0).to_degrees()).abs() < 1e-4);
        // Headwind = −Z (Z negative = wind from ahead), crosswind = X.
        assert_eq!(stats.landing_headwind_kt, Some(9.0));
        assert_eq!(stats.landing_crosswind_kt, Some(7.0));
        // Profile reframed to ms relative to the TD instant.
        assert_eq!(stats.touchdown_profile.len(), 3);
        assert_eq!(stats.touchdown_profile[0].t_ms, -500);
        assert_eq!(stats.touchdown_profile[1].t_ms, 0);
        assert_eq!(stats.touchdown_profile[2].t_ms, 500);
    }

    #[test]
    fn stamp_falls_back_to_live_snap_when_buffer_empty() {
        // Resumed flight: snapshot_buffer empty → everything that has no
        // MSFS latch falls back to the live snap. Must not panic.
        let mut stats = FlightStats::default();
        let snap = rollout_snap();
        let td_buf = touchdown_buffer_sample(&stats);
        assert!(td_buf.is_none());
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());

        assert_eq!(stats.landing_pitch_deg, Some(-2.0));
        assert_eq!(stats.landing_bank_deg, Some(0.5));
        assert_eq!(stats.landing_heading_deg, Some(277.0));
        assert_eq!(stats.landing_speed_kt, Some(55.0));
        assert_eq!(stats.landing_groundspeed_kt, Some(50.0));
        assert_eq!(stats.landing_lat, Some(51.001));
        assert_eq!(stats.landing_lon, Some(12.001));
        assert_eq!(stats.landing_heading_true_deg, Some(280.0));
        assert!(stats.touchdown_profile.is_empty());
    }

    #[test]
    fn msfs_latched_touchdown_simvars_win_over_buffer() {
        let mut stats = stats_with_buffer();
        let mut snap = rollout_snap();
        snap.touchdown_pitch_deg = Some(3.1);
        snap.touchdown_bank_deg = Some(-0.4);
        snap.touchdown_heading_mag_deg = Some(265.0);
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());

        assert_eq!(stats.landing_pitch_deg, Some(3.1));
        assert_eq!(stats.landing_bank_deg, Some(-0.4));
        assert_eq!(stats.landing_heading_deg, Some(265.0));
        // IAS/GS/coords have no MSFS latch — still from the buffer.
        assert_eq!(stats.landing_speed_kt, Some(100.0));
        assert_eq!(stats.landing_lat, Some(51.0));
    }

    /// The sampler-path call site guards on `landing_lat.is_none()`
    /// (first-touchdown-wins). That guard is only sound if the stamp
    /// ALWAYS sets landing_lat — buffer or not. And a second (FSM-arm)
    /// stamp must overwrite, because on normal flights the Landing arm
    /// runs after the sampler stamp and its values win, exactly as
    /// before the extraction.
    #[test]
    fn stamp_sets_once_guard_marker_and_second_stamp_overwrites() {
        // Empty buffer — worst case for the marker.
        let mut stats = FlightStats::default();
        let snap = rollout_snap();
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), None);
        assert!(stats.landing_lat.is_some(), "guard marker must be set");

        // Call-site once-guard: a second sampler-path stamp is skipped.
        assert!(
            stats.landing_lat.is_some(),
            "sampler call site would now skip (landing_lat.is_none() == false)"
        );

        // FSM-arm semantics: an UNGUARDED second stamp overwrites.
        let mut late_snap = rollout_snap();
        late_snap.lat = 48.0;
        late_snap.lon = 11.0;
        stamp_touchdown_metadata(&mut stats, &late_snap, td_at(), None);
        assert_eq!(stats.landing_lat, Some(48.0));
        assert_eq!(stats.landing_lon, Some(11.0));
    }

    // ---- stamp_landing_weight ----

    #[test]
    fn weight_prefers_total_weight_then_zfw_plus_fuel() {
        let snap = rollout_snap();
        let mut stats = FlightStats::default();
        stamp_landing_weight(&mut stats, &snap);
        assert_eq!(stats.landing_weight_kg, Some(1043.0));

        // No TOTAL WEIGHT → ZFW + fuel.
        let mut snap2 = rollout_snap();
        snap2.total_weight_kg = None;
        snap2.zfw_kg = Some(900.0);
        let mut stats2 = FlightStats::default();
        stamp_landing_weight(&mut stats2, &snap2);
        assert_eq!(stats2.landing_weight_kg, Some(900.0 + 88.5f32 as f64));

        // Fenix case: both unset/zero → field stays None.
        let mut snap3 = rollout_snap();
        snap3.total_weight_kg = None;
        snap3.zfw_kg = None;
        snap3.fuel_total_kg = 0.0;
        let mut stats3 = FlightStats::default();
        stamp_landing_weight(&mut stats3, &snap3);
        assert_eq!(stats3.landing_weight_kg, None);
    }

    // ---- correlate_touchdown_runway ----

    // EDDP/26R from the bundled OurAirports CSV (same fixture as
    // runway::tests): threshold 51.4336.., 12.2674.., heading 265.7.
    const EDDP_26R_THR_LAT: f64 = 51.433_601_379_392_15;
    const EDDP_26R_THR_LON: f64 = 12.267_399_787_902_832;
    const EDDP_26R_HEADING: f32 = 265.7;
    // 08L end from the bundled CSV (le="08L" 51.43119812, 12.21580028) —
    // das Gegenende der 26R. Auf Modulebene, weil Nav-Vorrichtung UND
    // Szenerie-Vorrichtung dieselbe Achse brauchen (`eddp_szenerie`).
    const EDDP_08L_THR_LAT: f64 = 51.431_198_120_117_19;
    const EDDP_08L_THR_LON: f64 = 12.215_800_285_339_355;

    #[test]
    fn correlate_resolves_runway_via_ourairports_fallback() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        let mut td = buf_sample(td_at(), true);
        td.lat = EDDP_26R_THR_LAT;
        td.lon = EDDP_26R_THR_LON;
        td.heading_true_deg = EDDP_26R_HEADING;
        stats.snapshot_buffer.push_back(td);
        let snap = rollout_snap();

        let td_buf = touchdown_buffer_sample(&stats);
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        let m = stats.runway_match.as_ref().expect("runway match");
        assert_eq!(m.airport_ident, "EDDP");
        assert_eq!(m.runway_ident, "26R");
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        // OurAirports fallback ⇒ no Navigraph geometry / cycle / TCH.
        assert!(stats.runway_nav_geometry.is_none());
        assert!(stats.runway_nav_cycle.is_none());
        assert!(stats.runway_tch_actual_ft.is_none());
    }

    /// ⚠ **Der Befund vom 01.09.2026 (YSBK), als Test — ueber die ECHTE
    /// Kette.**
    ///
    /// Der erste Entwurf dieses Tests rief nur zweimal
    /// `correlate_touchdown_runway` auf: keine Szenerieauskunft, kein
    /// Aufruf von `bahn_am_aufsetzpunkt_nachholen`, kein geprueftfer
    /// Uebernahmebericht. Der ganze Produktionsblock haette entfernt
    /// werden koennen, ohne dass er rot wird (QS-Befund 4, erste Runde
    /// v1.7.15).
    ///
    /// Jetzt faehrt er die Funktion, die auch die Produktion ruft:
    /// Score-Version 25, Ende zu Ende ueber den echten Zuordnungsweg:
    /// Navigraph-Bahn MIT ILS und ohne Versatz, MSFS-Szenerie malt 751,6 m
    /// Versatz (wie RJAA 34L). Die Bahn der Zuordnung traegt den Szenerie-
    /// Versatz (Aufsetzzone), der ILS-Gleitpfad nicht.
    #[test]
    fn ils_gleitpfad_bleibt_beim_szenerie_versatz_auf_navigraph() {
        let flight = flight_fixture("EDDP");
        let mut nav = eddp_nav_fixture();
        for r in nav.runways.iter_mut() {
            r.ils = Some(aeroacars_mqtt::navdata::NavIls {
                freq_mhz: 111.9,
                course: 265.0,
                category: 1,
            });
        }
        flight.navdata.lock().unwrap().airports.insert("EDDP".to_string(), nav);
        flight.navdata.lock().unwrap().cycle = Some("2610".to_string());

        let (mut stats, mut snap) = eddp_26r_touchdown_stats();
        snap.simulator = sim_core::Simulator::Msfs2024;
        stats.szenerie_auskunft = arc_some(eddp_szenerie(751.6));
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        let geo = stats.runway_nav_geometry.as_ref().expect("Navigraph-Bahn");
        assert_eq!(geo.displaced_threshold_ft, 2466, "Szenerie uebernommen (Vorrichtung)");
        let ils = stats.runway_ils_navigraph.as_ref().expect("ILS-Bezug gemerkt");
        assert_eq!(ils.bahn.displaced_threshold_ft, 0, "VOR der Szenerie gemerkt");
        assert_eq!(
            anflug_forensik_bahn(&stats).expect("Bahnbezug").versatz_ft,
            0.0,
            "der ILS-Gleitpfad wandert mit dem Szenerie-Versatz"
        );
        assert_eq!(
            stats.runway_match.as_ref().map(effective_displaced_threshold_ft),
            Some(2466),
            "die Aufsetzzone rechnet weiter gegen die gemalte Schwelle"
        );
    }

    /// Aufsetzen ohne Szenerie → Auskunft trifft ein → nachholen →
    /// Bericht.
    #[test]
    fn die_spaete_szenerie_erzeugt_beim_nachholen_einen_bericht() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        let (mut stats, mut snap) = eddp_26r_touchdown_stats();
        // ⚠ Der Schnappschuss der Vorrichtung meldet `Other`. Solange der
        // Simulator als Parameter hereinkam, liess sich das ueberdecken —
        // der Test reichte `Msfs2024` durch, waehrend das Aufsetzen mit
        // `Other` rechnete. Jetzt stammt beides aus derselben Quelle, und
        // die Vorrichtung muss sagen, was sie meint.
        snap.simulator = sim_core::Simulator::Msfs2024;
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        // Aufsetzen ohne Szenerie: Bahn da, aber KEIN Uebernahmebericht.
        assert!(stats.runway_match.is_some(), "Bahn beim Aufsetzen");
        assert!(
            stats.szenerie_uebernahme.is_none(),
            "ohne Auskunft darf es keinen Bericht geben"
        );
        assert_eq!(stats.korrelierter_szenerie_stand, 0);

        // ⚠ Jetzt trifft die Szenerie ein — so wie bei YSBK, nach dem
        // Aufsetzen.
        //
        // ⚠ Sie muss sich von den Navdaten UNTERSCHEIDEN. Die erste
        // Fassung dieses Tests nahm dieselben Werte — dann wird nichts
        // uebernommen, der Bericht ist leer, und `is_some()` war trotzdem
        // gruen: Der Test bewies nur, dass verglichen wurde, nicht dass
        // die Szenerie zaehlt (externe QS, 02.09.2026, P2-J).
        stats.szenerie_auskunft = arc_some(eddp_szenerie(350.0));
        stats.szenerie_auskunft_stand = 5;

        // ⚠ MSFS — sonst greift der X-Plane-Zweig und liest von der
        // Platte statt aus der Auskunft. Der Simulator kommt NICHT mehr
        // als Parameter herein: er stammt aus dem Aufsetz-Moment, den
        // `correlate_touchdown_runway` oben festgehalten hat.
        assert_eq!(
            stats.aufsetz_simulator,
            Some(sim_core::Simulator::Msfs2024),
            "der Aufsetz-Simulator wurde beim Aufsetzen nicht festgehalten \
             — dann rechnet das Nachholen im falschen Zweig"
        );

        // Nachholen — genau der Aufruf, den der Tick macht.
        let gelaufen = bahn_am_aufsetzpunkt_nachholen(&mut stats, &flight);

        assert!(gelaufen, "die spaete Szenerie wird nicht nachgeholt");
        // ⚠ Nicht `is_some()`: Ein LEERER Bericht ist auch `Some` — und
        // genau der Zustand, in dem die Herkunft weiter „navdaten" sagt.
        assert_eq!(
            bahn_herkunft(&stats).bahn_geometrie_quelle.as_deref(),
            Some("szenerie"),
            "nach dem Nachholen ist die Szenerie nicht uebernommen — genau das \
             stand bei YSBK am Flug"
        );
        assert_eq!(
            stats.korrelierter_szenerie_stand, 5,
            "der Stand wird nicht mitgeschrieben — dann laeuft es bei \
             JEDEM Durchlauf erneut"
        );
        // Runde 6, Befund 2: Die Schreibstelle von `szenerie_status_fest`
        // war von keinem Test gedeckt — wer sie loescht, blieb gruen.
        assert_eq!(
            stats.szenerie_status_fest.as_deref(),
            Some("uebernommen"),
            "der Szenerie-Status wird bei der Zuordnung nicht festgehalten — nach \
             einem Neustart kippt er dann auf eine vagere Aussage"
        );
        // Und die Bahn ist dieselbe geblieben.
        assert_eq!(
            stats.runway_match.as_ref().map(|m| m.runway_ident.clone()),
            Some("26R".to_string())
        );

        // Zweiter Aufruf ohne neue Lieferung: nichts zu tun.
        assert!(
            !bahn_am_aufsetzpunkt_nachholen(&mut stats, &flight),
            "es wird erneut zugeordnet, obwohl die Lieferung dieselbe ist"
        );

        // ⚠ Eine NEUERE Lieferung wird sehr wohl wieder zugeordnet.
        stats.szenerie_auskunft_stand = 9;
        assert!(
            bahn_am_aufsetzpunkt_nachholen(&mut stats, &flight),
            "eine neuere Lieferung erreicht die Zuordnung nicht"
        );
        assert_eq!(stats.korrelierter_szenerie_stand, 9);
    }

    /// ⚠ Das alte Drittel folgt der KORRIGIERTEN Rechnung, nicht der rohen.
    ///
    /// `landing_touchdown_zone` wurde bis v1.7.14 roh aus Bahnlaenge und
    /// Aufsetzdistanz gerechnet, `td_third` dagegen in
    /// `assess_touchdown` — mit Abzug der versetzten Schwelle. Zwei
    /// Zahlen fuer dieselbe Aussage, und zwei Webapp-Karten, die die
    /// aeltere lesen.
    ///
    /// ⚠ Die Vorrichtung ist so gewaehlt, dass die beiden Rechnungen
    /// AUSEINANDERGEHEN — sonst zeigte der Test nichts:
    ///
    /// * roh: 4984 ft = 1519 m auf 3600 m Bahn, Drittel bei 1200 m → **2**
    /// * korrigiert: 4984 − 4000 = 984 ft = 300 m auf 2381 m nutzbarer
    ///   Laenge, Drittel bei 794 m → **1**
    #[test]
    fn das_alte_drittel_folgt_der_korrigierten_rechnung() {
        const FT_PER_M: f64 = 3.280_839_895;
        let raw_ft = 4000.0 + 300.0 * FT_PER_M;

        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td_and_displacement(raw_ft, 4000));

        // Die rohe Rechnung von frueher, zum Vergleich nachgestellt.
        let roh = {
            let past_m = raw_ft * 0.3048;
            let total_m = 11_811.0 * 0.3048;
            let third = total_m / 3.0;
            if past_m <= third {
                1u8
            } else if past_m <= 2.0 * third {
                2
            } else {
                3
            }
        };
        assert_eq!(roh, 2, "die Vorrichtung trennt die beiden Rechnungen nicht");

        drittel_nachfuehren(&mut stats);
        assert_eq!(
            stats.landing_touchdown_zone,
            Some(1),
            "`landing_touchdown_zone` traegt weiter die rohe Zahl — dann \
             zeigen zwei Webapp-Karten verschiedene Drittel derselben \
             Landung"
        );
        assert_eq!(
            stats.landing_touchdown_zone,
            bahn_herkunft(&stats).td_third,
            "die beiden Drittel stammen nicht aus derselben Rechnung"
        );
    }

    /// Und ohne Bahntreffer wird das Feld GELEERT, nicht stehengelassen.
    ///
    /// Ein Drittel ohne Bahn ist eine Behauptung ueber nichts — und beim
    /// Ausweichen auf einen Platz ohne Bahndaten stuende sonst das
    /// Drittel des vorigen Anflugs da.
    #[test]
    fn ohne_bahntreffer_verschwindet_das_drittel() {
        let mut stats = FlightStats::default();
        stats.landing_touchdown_zone = Some(3);
        drittel_nachfuehren(&mut stats);
        assert_eq!(stats.landing_touchdown_zone, None);
    }

    /// ⚠ Auf einem kurzen Platz gibt es weiter ein Drittel.
    ///
    /// Die erste Fassung von `drittel_nachfuehren` haengte das Feld an
    /// `classify_tdz`, und das meldet unter 1200 m `None` — weil es dort
    /// keine Aufsetzzonen-MARKIERUNG gibt. Das Drittel ist keine
    /// Markierung. Buschplaetze verloren die Anzeige lautlos, und beide
    /// Drittel-Tests hielten die Bahnlaenge konstant (externe QS,
    /// 02.09.2026, P1-A).
    #[test]
    fn auf_einem_kurzen_platz_bleibt_das_drittel() {
        const FT_PER_M: f64 = 3.280_839_895;
        let mut stats = FlightStats::default();
        // 900 m Bahn, Aufsetzen 100 m hinter der Schwelle.
        let mut rm = eddp_26r_match_with_raw_td(100.0 * FT_PER_M);
        rm.length_ft = (900.0 * FT_PER_M) as f32;
        stats.runway_match = Some(rm);

        // Die Markierung gibt es hier zu Recht nicht …
        assert!(
            assess_touchdown(&stats).tdz.is_none(),
            "die Vorrichtung liegt nicht unter der 1200-m-Grenze — dann \
             prueft dieser Test nichts"
        );
        // … das Drittel aber schon.
        drittel_nachfuehren(&mut stats);
        assert_eq!(
            stats.landing_touchdown_zone,
            Some(1),
            "auf dem kurzen Platz verschwindet das Drittel — die \
             Aufsetzzonen-Anzeige faellt dann auf jedem Buschplatz weg"
        );
        // Zweites Drittel, damit die Rechnung selbst geprueft ist.
        let mut rm = eddp_26r_match_with_raw_td(400.0 * FT_PER_M);
        rm.length_ft = (900.0 * FT_PER_M) as f32;
        stats.runway_match = Some(rm);
        drittel_nachfuehren(&mut stats);
        assert_eq!(stats.landing_touchdown_zone, Some(2));
    }

    /// Eine Spur, die schon Punkte hat, gegen eine bekannte Achse.
    fn stats_mit_spur(rm: runway::RunwayMatch) -> FlightStats {
        let mut stats = FlightStats::default();
        stats.runway_match = Some(rm);
        stats.bahn_spur = vec![(0.0, 1.0), (500.0, -2.0), (1200.0, 3.0)];
        stats.bahn_raeum_laengs_m = Some(1500.0);
        stats.bahn_kante_laengs_m = Some(1550.0);
        stats.bahn_fenster_zu_laengs_m = Some(1400.0);
        stats.bahn_max_querversatz_m = Some(3.0);
        stats.bahn_spur_bis = Some(td_at());
        stats
    }

    /// Dieselbe Bahn, Schwelle um `meter` in Landerichtung verschoben.
    fn schwelle_verschoben(rm: &runway::RunwayMatch, meter: f64) -> runway::RunwayMatch {
        let laenge_m = rm.length_ft as f64 / 3.280_839_895;
        let f = meter / laenge_m;
        let mut neu = rm.clone();
        neu.threshold_lat = rm.threshold_lat + (rm.end_lat - rm.threshold_lat) * f;
        neu.threshold_lon = rm.threshold_lon + (rm.end_lon - rm.threshold_lon) * f;
        neu
    }

    /// ⚠ Nach dem Nachholen liegt die ganze Spur in EINEM Bezugssystem.
    ///
    /// Die Spur speichert Projektionen gegen die Bahnachse, nicht
    /// Koordinaten. Kommt die Szenerie nach dem Aufsetzen mit einer
    /// versetzten Schwelle, trugen die Punkte davor den alten Nullpunkt
    /// und die danach den neuen — auf der Leitung stand EIN Versatz. Ein
    /// Knick mitten in der Spur, der plausibel aussah (externe QS,
    /// 02.09.2026, P1-C).
    #[test]
    fn die_spur_rueckt_mit_der_verschobenen_schwelle() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        let neu = schwelle_verschoben(&alt, 350.0);
        let alte_achse = AlteAchse {
            t_lat: alt.threshold_lat,
            t_lon: alt.threshold_lon,
            e_lat: alt.end_lat,
            e_lon: alt.end_lon,
            nutzbare_laenge_m: bahnmasse(&alt).nutzbare_laenge_m,
        };
        // Der Versatz, wie die Produktion ihn misst — die lineare
        // Interpolation der Vorrichtung trifft die 350 m nur ungefaehr
        // (348,7 m); geprueft wird die EXAKTHEIT der Verschiebung, nicht
        // die Meterzahl der Vorrichtung.
        let (versatz, dt_q_rest) = runway::projiziere_auf_bahn(
            alt.threshold_lat,
            alt.threshold_lon,
            alt.end_lat,
            alt.end_lon,
            neu.threshold_lat,
            neu.threshold_lon,
        );
        let dt_q_rest = dt_q_rest.abs() as f32;
        // ⚠ Der Rest muss KLEIN sein — sonst wuerde die Toleranz unten
        // einen echten Fehler verdecken (Runde 5: „von der Toleranz
        // erzwungen").
        assert!(dt_q_rest < 0.3, "Fixture-Rest zu gross: {dt_q_rest} m");
        assert!(
            (versatz - 350.0).abs() < 5.0,
            "die Vorrichtung verschiebt nicht um ~350 m: {versatz}"
        );
        stats.runway_match = Some(neu);

        spur_auf_neue_achse(&mut stats, Some(&alte_achse));

        assert_eq!(
            stats.bahn_spur.len(),
            3,
            "die Spur wurde verworfen statt verschoben"
        );
        for ((lg, qr), (lg_alt, qr_alt)) in
            stats
                .bahn_spur
                .iter()
                .zip([(0.0f32, 1.0f32), (500.0, -2.0), (1200.0, 3.0)])
        {
            assert!(
                (*lg as f64 - (lg_alt as f64 - versatz)).abs() < 0.01,
                "Laengswert nicht exakt verschoben: {lg} statt {}",
                lg_alt as f64 - versatz
            );
            // Die lineare Interpolation der Vorrichtung erzeugt einen
            // kleinen Querrest gegen die Grosskreis-Projektion; der
            // affine Ausgleich zieht ihn mit. Fuer die ECHTE Quer-Zusicherung
            // gibt es `die_spur_rueckt_quer_mit_der_parallel_versetzten_achse`.
            // Der Rest wird fuer Punkte VOR der neuen Schwelle extrapoliert
            // (Faktor 1 + dt_l/Spanne, hier ~1,1) — deshalb das Doppelte.
            assert!(
                (qr - qr_alt).abs() <= 2.0 * dt_q_rest + 0.01,
                "Querwert um mehr als den Fixture-Rest veraendert: {qr} statt {qr_alt} \
                 (Rest {dt_q_rest})"
            );
        }
        assert!((stats.bahn_raeum_laengs_m.unwrap() - (1500.0 - versatz)).abs() < 0.01);
        assert!((stats.bahn_kante_laengs_m.unwrap() - (1550.0 - versatz)).abs() < 0.01);
        assert!((stats.bahn_fenster_zu_laengs_m.unwrap() - (1400.0 - versatz)).abs() < 0.01);
        // Neu aus der verschobenen Spur bestimmt — bis auf den Fixture-Rest.
        let m = stats.bahn_max_querversatz_m.expect("max quer");
        assert!(
            (m - 3.0).abs() <= 2.0 * dt_q_rest as f64 + 0.01,
            "groesster Querversatz veraendert: {m}"
        );
        assert!(
            stats.bahn_spur_bis.is_some(),
            "die Ernte darf nicht von vorn beginnen"
        );
    }

    /// Und gegen eine ANDERE Achse wird sie verworfen — nicht gemischt.
    #[test]
    fn gegen_eine_andere_achse_wird_die_spur_verworfen() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        // Die Gegenrichtung: Schwelle und Ende vertauscht.
        let mut neu = alt.clone();
        neu.threshold_lat = alt.end_lat;
        neu.threshold_lon = alt.end_lon;
        neu.end_lat = alt.threshold_lat;
        neu.end_lon = alt.threshold_lon;
        let alte_achse = AlteAchse {
            t_lat: alt.threshold_lat,
            t_lon: alt.threshold_lon,
            e_lat: alt.end_lat,
            e_lon: alt.end_lon,
            nutzbare_laenge_m: bahnmasse(&alt).nutzbare_laenge_m,
        };
        stats.runway_match = Some(neu);

        spur_auf_neue_achse(&mut stats, Some(&alte_achse));

        assert!(
            stats.bahn_spur.is_empty(),
            "die Spur steht noch — gegen eine Achse, die es nicht mehr gibt"
        );
        assert_eq!(stats.bahn_raeum_laengs_m, None);
        assert_eq!(stats.bahn_kante_laengs_m, None);
        assert_eq!(stats.bahn_max_querversatz_m, None);
        assert_eq!(
            stats.bahn_spur_bis, None,
            "die Ernte muss von vorn beginnen"
        );
    }

    /// ⚠ Die Parallelbahn: gleiche Richtung, seitlich versetzt (26R → 26L).
    ///
    /// Das ist der Fall, fuer den `ACHSE_TOLERANZ_M` ueberhaupt da ist —
    /// und der einzige, der die beiden Quer-Pruefungen braucht. Der
    /// Gegenrichtungs-Test oben kommt ohne sie aus (externe QS,
    /// 02.09.2026: „ein Wegfall der Quer-Pruefungen macht keinen Test rot").
    #[test]
    fn gegen_eine_parallelbahn_wird_die_spur_verworfen() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        // ~100 m nach Norden — die 26R laeuft nach Westen, das ist quer.
        let mut neu = alt.clone();
        neu.threshold_lat += 0.0009;
        neu.end_lat += 0.0009;
        let alte_achse = alte_achse_von(&stats).expect("alte Achse");
        stats.runway_match = Some(neu);
        spur_auf_neue_achse(&mut stats, Some(&alte_achse));
        assert!(
            stats.bahn_spur.is_empty(),
            "die Spur der 26R haengt jetzt an der 26L — 100 m daneben"
        );
    }

    /// ⚠ Die Aufsetzdistanz folgt der Bahn — an JEDER Zuordnung.
    ///
    /// Sie wurde nur im Aufsetz-Tick gesetzt; nach einer spaeten
    /// Zuordnung stand sie von der alten Bahn neben dem korrigierten
    /// Drittel — und fuetterte den Vertrauens-Riegel (externe QS,
    /// Runde 3, P2).
    #[test]
    fn die_aufsetzdistanz_folgt_der_bahn() {
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(1000.0));
        drittel_nachfuehren(&mut stats);
        let vorher = stats.landing_float_distance_m.expect("Distanz");
        assert!(
            (vorher - 304.8).abs() < 0.5,
            "1000 ft sind 304,8 m: {vorher}"
        );
        // Die Bahn wird neu zugeordnet, das Aufsetzen liegt jetzt 2000 ft
        // hinter der Schwelle.
        stats.runway_match = Some(eddp_26r_match_with_raw_td(2000.0));
        drittel_nachfuehren(&mut stats);
        let nachher = stats.landing_float_distance_m.expect("Distanz");
        assert!(
            (nachher - 609.6).abs() < 0.5,
            "die Aufsetzdistanz folgt der neuen Bahn nicht: {nachher}"
        );
        stats.runway_match = None;
        drittel_nachfuehren(&mut stats);
        assert_eq!(stats.landing_float_distance_m, None);
    }

    /// ⚠ Ohne Laenge kein Drittel — auch nicht „3".
    #[test]
    fn ohne_bahnlaenge_gibt_es_kein_drittel() {
        let mut stats = FlightStats::default();
        let mut rm = eddp_26r_match_with_raw_td(100.0);
        rm.length_ft = 0.0;
        stats.runway_match = Some(rm);
        drittel_nachfuehren(&mut stats);
        assert_eq!(
            stats.landing_touchdown_zone, None,
            "eine Bahn ohne Laenge bekommt ein Drittel — eine Behauptung \
             ueber nichts"
        );
    }

    /// Lernpaket AP4 (QS 29.09.2026): Die versetzte Schwelle der Forensik
    /// kommt ueber `displacement_not_in_geometry_ft` aus dem echten
    /// Bahntreffer — voller Versatz, solange die Geometrie ihn nicht
    /// bestaetigt; 0, wenn er schon in der Geometrie steckt.
    #[test]
    fn forensik_versatz_folgt_dem_bahntreffer() {
        let mut stats = FlightStats::default();
        stats.runway_nav_geometry = Some(eddp_nav_fixture().runways[0].clone());
        stats.runway_match = Some(eddp_26r_match_with_raw_td_and_displacement(500.0, 1000));
        assert_eq!(anflug_forensik_bahn(&stats).unwrap().versatz_ft, 1000.0);
        let mut bestaetigt = eddp_26r_match_with_raw_td_and_displacement(500.0, 1000);
        bestaetigt.geometry_implied_displaced_threshold_ft = 1000;
        stats.runway_match = Some(bestaetigt);
        assert_eq!(anflug_forensik_bahn(&stats).unwrap().versatz_ft, 0.0);
    }

    /// Score-Version 25 (Sven, RJAA 34L, 16.10.2026): Hat die Bahn ein ILS,
    /// zaehlt fuer den Gleitpfad die UNVERAENDERTE Navigraph-Bahn — der
    /// Versatz aus der Sim-Szenerie verschiebt das ILS nicht mehr.
    #[test]
    fn ils_gleitpfad_folgt_navigraph_nicht_dem_szenerie_versatz() {
        let ils = aeroacars_mqtt::navdata::NavIls {
            freq_mhz: 111.9,
            course: 337.0,
            category: 1,
        };
        // Navigraph: ILS, kein Versatz (wie RJAA 34L in AIRAC 2610).
        let mut roh = eddp_nav_fixture();
        for r in roh.runways.iter_mut() {
            r.ils = Some(ils.clone());
        }
        // Szenerie: dieselbe Bahn mit 2466 ft gemaltem Versatz.
        let mut szenerie = roh.runways[0].clone();
        szenerie.displaced_threshold_ft = 2466;

        let mut stats = FlightStats::default();
        stats.runway_nav_geometry = Some(szenerie);
        stats.runway_match = Some(eddp_26r_match_with_raw_td_and_displacement(500.0, 2466));
        // Vor Version 25: der Szenerie-Versatz verschob das ILS.
        assert_eq!(anflug_forensik_bahn(&stats).unwrap().versatz_ft, 2466.0);

        let bezeichner = roh.runways[0].designator.clone();
        stats.runway_ils_navigraph = IlsNavigraph::aus(&roh, &bezeichner);
        let bahn = anflug_forensik_bahn(&stats).unwrap();
        assert_eq!(bahn.versatz_ft, 0.0, "ILS-Bezug ohne Szenerie-Versatz");
        assert_eq!(bahn.schwelle_lat, roh.runways[0].threshold.lat);

        // Navigraph mit eigenem Versatz (wie VIDP 11L): der bleibt.
        let mut mit_versatz = roh.clone();
        mit_versatz.runways[0].displaced_threshold_ft = 1444;
        stats.runway_ils_navigraph = IlsNavigraph::aus(&mit_versatz, &bezeichner);
        assert_eq!(anflug_forensik_bahn(&stats).unwrap().versatz_ft, 1444.0);
    }

    /// Score-Version 25 (Codex): Ein aus einem Zustand von vor Version 25
    /// fortgesetzter Flug hat keinen ILS-Bezug gemerkt — das Einreichen baut
    /// ihn aus dem (unveraenderten) Navdaten-Cache nach.
    #[test]
    fn einreichen_baut_den_ils_bezug_fuer_alte_zustaende_nach() {
        let flight = flight_fixture("EDDP");
        let mut nav = eddp_nav_fixture();
        for r in nav.runways.iter_mut() {
            r.ils = Some(aeroacars_mqtt::navdata::NavIls {
                freq_mhz: 111.9,
                course: 265.0,
                category: 1,
            });
        }
        let bezeichner = nav.runways[0].designator.clone();
        flight.navdata.lock().unwrap().airports.insert("EDDP".to_string(), nav.clone());
        let mut szenerie = nav.runways[0].clone();
        szenerie.displaced_threshold_ft = 2466;
        let mut stats = FlightStats::default();
        stats.runway_source = Some(runway::RunwaySource::Navigraph);
        stats.runway_nav_geometry = Some(szenerie);
        let mut rm = eddp_26r_match_with_raw_td_and_displacement(500.0, 2466);
        rm.runway_ident = bezeichner;
        stats.runway_match = Some(rm);
        assert!(stats.runway_ils_navigraph.is_none(), "Altzustand");
        assert_eq!(anflug_forensik_bahn(&stats).unwrap().versatz_ft, 2466.0);

        let f = finalize_runway_correlation(&flight, &stats);
        let ils = f.runway_ils_navigraph.expect("nachgebaut");
        assert_eq!(ils.versatz_ft, 0);
        stats.runway_ils_navigraph = Some(ils);
        assert_eq!(anflug_forensik_bahn(&stats).unwrap().versatz_ft, 0.0);
    }

    /// Ohne ILS bleibt die Szenerie massgeblich — der Sichtanflug zielt auf
    /// die gemalte Schwelle und die PAPI dort.
    #[test]
    fn ohne_ils_bleibt_der_szenerie_versatz() {
        let roh = eddp_nav_fixture();
        assert!(roh.runways[0].ils.is_none(), "Fixture ohne ILS");
        let mut szenerie = roh.runways[0].clone();
        szenerie.displaced_threshold_ft = 2466;
        let mut stats = FlightStats::default();
        stats.runway_nav_geometry = Some(szenerie);
        stats.runway_match = Some(eddp_26r_match_with_raw_td_and_displacement(500.0, 2466));
        stats.runway_ils_navigraph = IlsNavigraph::aus(&roh, &roh.runways[0].designator);
        assert_eq!(anflug_forensik_bahn(&stats).unwrap().versatz_ft, 2466.0);
    }

    /// Ein Anflug auf die EDDP 26R (Navigraph-Schwelle), 1000 → 200 ft,
    /// in den Sekunden vor `td_at()`.
    fn eddp_26r_anflug_in_den_forensik_puffer(stats: &mut FlightStats) {
        let (tl, tn) = (EDDP_26R_THR_LAT, EDDP_26R_THR_LON);
        let nav = eddp_nav_fixture();
        let (el, en) = (nav.runways[0].far_end.lat, nav.runways[0].far_end.lon);
        // Einheitsrichtung Ende → Schwelle, lokal flach (wenige km).
        let cos_lat = tl.to_radians().cos();
        let (dy, dx) = (tl - el, (tn - en) * cos_lat);
        let n = dy.hypot(dx);
        let m_je_grad = 6_371_000.0 * std::f64::consts::PI / 180.0;
        for i in 0..=40 {
            let h = 1000.0 - 20.0 * i as f64;
            let d_m = (h - 50.0) / 3.0_f64.to_radians().tan() / 3.280_839_895;
            let mut s = ApproachBufferSample {
                at: td_at() - chrono::Duration::seconds(50 - i as i64),
                agl_ft: h as f32,
                msl_ft: (465.0 + h) as f32,
                gs_kt: 140.0,
                ias_kt: 140.0,
                vs_fpm: -700.0,
                bank_deg: 0.0,
                heading_true_deg: 265.7,
                gear_position: 1.0,
                flaps_position: 1.0,
                selected_runway: None,
                stall_warning: false,
                lat: None,
                lon: None,
                pitch_deg: Some(2.5),
                n1_mittel_pct: None,
                headwind_kt: None,
                crosswind_kt: None,
            };
            s.lat = Some(tl + dy / n * d_m / m_je_grad);
            s.lon = Some(tn + dx / n * d_m / (m_je_grad * cos_lat));
            stats.anflug_forensik_puffer.push_back(s);
        }
    }

    /// Befund 2: Gestempelt wurde gegen die vorlaeufige OurAirports-Bahn —
    /// ohne Navdaten, also ohne Werte. Beim Einreichen kommt die
    /// Navigraph-Bahn: die Forensik zieht nach und hat jetzt Werte.
    #[test]
    fn forensik_zieht_nach_dem_navdaten_upgrade_nach() {
        let flight = flight_fixture("EDDF");
        let (mut stats, snap) = divert_score_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        eddp_26r_anflug_in_den_forensik_puffer(&mut stats);
        anflug_forensik_stempeln(&mut stats, None, Some(td_at()), None);
        let vorher = stats.anflug_forensik.gleitpfad.clone().unwrap();
        assert_eq!(vorher.grund_ohne_werte.as_deref(), Some("keine_bahn"));

        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());
        assert!(bahn_upgrade_anwenden(&flight, &mut stats));

        let nachher = stats.anflug_forensik.gleitpfad.clone().unwrap();
        assert_eq!(nachher.quelle, "navigraph_bahn");
        assert_eq!(nachher.grund_ohne_werte, None);
        assert!(nachher.gesamt.is_some(), "{nachher:?}");
        // Die 4000 ft versetzte Schwelle der Navdaten sind beruecksichtigt.
        assert_eq!(nachher.versatz_ft, Some(4000.0));

        // Das Flug-Log bekommt den korrigierten Stand — genau einmal.
        let ereignis = anflug_forensik_nachtrag_ereignis(&mut stats).expect("Nachtrag");
        let zeile = serde_json::to_value(&ereignis).unwrap();
        assert_eq!(zeile["type"], "landing_analysis_nachtrag");
        assert_eq!(
            zeile["payload"]["anflug_gleitpfad"]["quelle"],
            "navigraph_bahn"
        );
        assert!(zeile["payload"]["edge_at"].is_string());
        assert!(anflug_forensik_nachtrag_ereignis(&mut stats).is_none());
    }

    /// Riegel: Nach einem Durchstart-Reset gibt es nichts nachzuziehen — ein
    /// Bahnwechsel darf keinen Befund fuer eine Landung erfinden, die noch
    /// nicht stattgefunden hat.
    #[test]
    fn forensik_zieht_nach_einem_durchstart_reset_nicht_nach() {
        let flight = flight_fixture("EDDF");
        let (mut stats, snap) = divert_score_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        eddp_26r_anflug_in_den_forensik_puffer(&mut stats);
        anflug_forensik_stempeln(&mut stats, None, Some(td_at()), None);
        clear_approach_stability_and_rollout(&mut stats);

        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());
        bahn_upgrade_anwenden(&flight, &mut stats);
        assert_eq!(
            stats.anflug_forensik,
            anflug_forensik::AnflugForensik::default()
        );
    }

    /// ⚠ Das Einreich-Upgrade rebasiert die Spur wie das Nachholen.
    ///
    /// Sonst schickt `bahn_nachtrag_bauen` die Spur gegen die alte Achse
    /// zusammen mit `runway_length_m` aus der neuen — genau die Mischung,
    /// gegen die P1-C gebaut wurde (externe QS, 02.09.2026, N2).
    #[test]
    fn das_navdaten_upgrade_rebasiert_die_spur() {
        let flight = flight_fixture("EDDF");
        let (mut stats, snap) = divert_score_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        // Eine Spur mit Ueberrollen gegen die vorlaeufige Bahn.
        stats.bahn_spur = vec![(0.0, 1.0), (500.0, -2.0), (1200.0, 3.0)];
        stats.bahn_overrun_m = Some(100.0);
        let alte = alte_achse_von(&stats).expect("Bahn");

        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());
        assert!(bahn_upgrade_anwenden(&flight, &mut stats));

        // Entweder verschoben (gleiche Achse) oder verworfen (andere) —
        // aber NIE unveraendert gegen eine Bahn, die es nicht mehr gibt.
        let neu = stats.runway_match.as_ref().expect("Navigraph-Bahn");
        let (_, dt_q) = runway::projiziere_auf_bahn(
            alte.t_lat,
            alte.t_lon,
            alte.e_lat,
            alte.e_lon,
            neu.threshold_lat,
            neu.threshold_lon,
        );
        // ⚠ Die Vorrichtung wird FESTGENAGELT, nicht abgefragt: Ein
        // `if/else` liesse den Test lautlos in den Verwerfen-Zweig
        // wandern, sobald jemand die Vorrichtung aendert — und die
        // Ueberroll-Zusicherung verschwaende (externe QS, Runde 3,
        // Klasse 8).
        assert!(
            dt_q.abs() <= crate::grenzwerte::grenzwerte().ausrollen.achse_toleranz_m.wert,
            "die Vorrichtung liegt nicht mehr auf derselben Achse ({dt_q} m quer) — \
             dieser Test prueft das VERSCHIEBEN; fuer das Verwerfen gibt es eigene"
        );
        assert_eq!(stats.bahn_spur.len(), 3, "gleiche Achse, Spur verworfen");
        // 4000 ft versetzte Schwelle: die nutzbare Laenge schrumpft um
        // 1219 m, das Ueberrollen waechst um denselben Betrag.
        let ueber = stats.bahn_overrun_m.expect("Ueberrollen");
        assert!(
            (ueber - (100.0 + 4000.0 * 0.3048)).abs() < 2.0,
            "Ueberrollen nicht gegen die neue Laenge gerechnet: {ueber}"
        );
    }

    /// ⚠ Ein wiederhergestelltes Drittel ist eine Aenderung.
    ///
    /// Der Fall aus Runde 3: Nach dem Neustart fehlte das Drittel, das
    /// Upgrade beim Einreichen stellte es ueber `drittel_nachfuehren`
    /// wieder her — aber die Aenderungserkennung sah nur die
    /// Herkunftsgruppe, in der es nicht steht. Kein Nachtrag, Spalte fuer
    /// immer leer.
    #[test]
    fn ein_wiederhergestelltes_drittel_ist_eine_aenderung() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        assert!(stats.landing_touchdown_zone.is_some());
        stats.bahn_nachtrag_offen = false;
        let revision = stats.bahn_revision;

        // Wie nach einem Neustart ohne persistiertes Drittel: weg.
        stats.landing_touchdown_zone = None;
        assert!(
            bahn_upgrade_anwenden(&flight, &mut stats),
            "das wiederhergestellte Drittel gilt nicht als Aenderung — dann \
             bleibt die Spalte beim Recorder fuer immer leer"
        );
        assert!(stats.landing_touchdown_zone.is_some());
        assert_eq!(stats.bahn_revision, revision + 1);
        assert!(stats.bahn_nachtrag_offen);
    }

    /// ⚠ Der Nachtrag adressiert dieselbe Aufsetzzeit wie `touchdown_complete`.
    ///
    /// Busch-/VFR-Landung: Der Abtaster setzt `sampler_touchdown_at` sofort,
    /// `landing_at` kommt erst mit dem `Arrived`-Rueckfall — oder beim
    /// manuellen Einreichen gar nicht. Ein Nachtrag nur ueber `landing_at`
    /// traf dann keine Zeile oder unterblieb (Codex, 03.09.2026).
    #[test]
    fn der_nachtrag_nimmt_die_abtaster_aufsetzzeit() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(0.0));
        let abtaster = chrono::DateTime::<Utc>::from_timestamp(1_755_000_100, 0).expect("Zeit");
        stats.sampler_touchdown_at = Some(abtaster);
        stats.landing_at = None;
        let n = bahn_nachtrag_bauen(&flight, &stats).expect(
            "ohne `landing_at` unterbleibt der Nachtrag — die Busch-Landung bleibt unkorrigiert",
        );
        assert_eq!(n.touchdown_at, abtaster.timestamp_millis());
        let spaeter = chrono::DateTime::<Utc>::from_timestamp(1_755_000_300, 0).expect("Zeit");
        stats.landing_at = Some(spaeter);
        let n = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        assert_eq!(
            n.touchdown_at,
            abtaster.timestamp_millis(),
            "der Nachtrag adressiert eine andere Zeile als das Aufsetz-Ereignis"
        );
    }

    /// ⚠ Die Ablage liest ueber `aus_json`: Ein Nachtrag OHNE Spur-Block
    /// kommt ohne Spur-Block zurueck (nicht als `Some(default)`), und der
    /// Dateiname traegt Kennung + Aufsetzzeit, damit zwei Landungen
    /// derselben PIREP sich nicht ueberschreiben.
    #[test]
    fn die_ablage_liest_zurueck_was_sie_schrieb() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(0.0));
        stats.landing_at =
            Some(chrono::DateTime::<Utc>::from_timestamp(1_755_000_100, 0).expect("Zeit"));
        stats.bahn_revision = 3;
        let n = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        assert!(n.bahn.is_none(), "Fixture ohne Spur muss ohne Block gehen");
        let text = nachtrag_queue::serialisieren("GSG", "42", &n).expect("JSON");
        let zurueck = nachtrag_queue::lesen(&text, "GSG", "42").expect("lesbar");
        assert!(
            nachtrag_queue::lesen(&text, "GSG", "43").is_err(),
            "die Datei gehoert Pilot 42 — Pilot 43 darf sie nicht lesen"
        );
        assert_eq!(zurueck.pirep_id, n.pirep_id);
        assert_eq!(zurueck.touchdown_at, n.touchdown_at);
        assert_eq!(zurueck.herkunft.bahn_revision, Some(3));
        assert!(
            zurueck.bahn.is_none(),
            "flatten+Option liest den fehlenden Block als Some(default) — die Ablage muss ueber aus_json gehen"
        );
        assert_eq!(
            nachtrag_queue::dateiname("42/../x", 1_755_000_100_000, Some(3)).as_deref(),
            Some("42x-1755000100000-r000003.json")
        );
        assert_eq!(
            nachtrag_queue::dateiname("a", 1, None).as_deref(),
            Some("a-1-r000000.json"),
            "ohne Revision: r000000, damit ein Altstand vor jeder echten Revision sortiert"
        );
        assert_eq!(nachtrag_queue::dateiname("/../", 1, Some(1)), None);
    }

    fn temp_ablage(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!(
            "aeroacars-ablage-{name}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    /// ⚠ Die Ablage trennt die Mandanten (Codex, 03.09.2026, Runde 12).
    ///
    /// Zwei Piloten auf demselben Rechner: `list_in` fuer den einen liefert
    /// nie den Nachtrag des anderen — weder ueber den Pfad noch ueber eine
    /// Datei, deren Inhalt einem anderen gehoert.
    /// Baut einen Nachtrag mit einer Spur der gewünschten Länge.
    fn nachtrag_mit_spur(punkte: usize) -> aeroacars_mqtt::TouchdownRolloutFinalizedPayload {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(0.0));
        stats.landing_at =
            Some(chrono::DateTime::<Utc>::from_timestamp(1_755_000_100, 0).expect("Zeit"));
        let mut n = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        let spur: Vec<_> = (0..punkte)
            .map(|i| aeroacars_mqtt::LateralSampleWire {
                laengs_m: i as f64 * 1.5,
                quer_m: (i % 7) as f64 * 0.25,
            })
            .collect();
        // Die Bahn-Gruppe ausdrücklich setzen: `bahn_nachtrag_bauen` füllt
        // sie nur, wenn die Bewertung schon gelaufen ist.
        n.bahn = Some(aeroacars_mqtt::BahnWire {
            clearance_point_m: Some(1_831.6),
            lateral_samples: Some(spur),
            ..Default::default()
        });
        n
    }

    #[test]
    fn eine_zu_grosse_korrektur_wird_ausgeduennt_statt_die_leitung_zu_kosten() {
        // Feldbefund 20.09.2026 (Pilot 5): 10.379 Byte gegen die
        // rumqttc-Vorgabe von 10.240 — jede Minute ein Verbindungsabriss,
        // 625 Mal an einem Tag. Der Nachtrag darf danach SENDBAR sein,
        // und die Korrektur selbst muss erhalten bleiben.
        let mut n = nachtrag_mit_spur(40_000);
        let kennung = n.pirep_id.clone();
        let vorher = serde_json::to_vec(&n).expect("json").len();
        assert!(
            vorher > 60 * 1024,
            "Gegenprobe: {vorher} Byte sind nicht zu gross"
        );
        assert!(nachtrag_queue::auf_sendegroesse_bringen(&mut n));
        let nachher = serde_json::to_vec(&n).expect("json").len();
        assert!(nachher <= 60 * 1024, "immer noch {nachher} Byte");
        assert_eq!(n.pirep_id, kennung, "die Korrektur selbst bleibt");
        assert!(
            n.bahn.as_ref().expect("Bahn").clearance_point_m.is_some(),
            "der Raeumpunkt darf beim Ausduennen nicht verloren gehen"
        );
    }

    #[test]
    fn das_aufraeumen_beim_start_macht_die_ablage_sendbar() {
        // Thomas, 20.09.2026: Der Pilot soll nichts loeschen muessen —
        // der Client raeumt beim Start selbst auf.
        let base = temp_ablage("aufraeumen");
        let n = nachtrag_mit_spur(40_000);
        let pfad = nachtrag_queue::enqueue_in(&base, "GSG", "42", &n).expect("Ablage");
        let vorher = std::fs::metadata(&pfad).expect("Datei").len();
        assert!(vorher > 60 * 1024, "Gegenprobe: {vorher} Byte");

        let (geschrumpft, aussortiert) = nachtrag_queue::aufraeumen_in(&base);
        assert_eq!((geschrumpft, aussortiert), (1, 0));

        let gelesen = nachtrag_queue::list_in(&base, "GSG", "42");
        assert_eq!(gelesen.len(), 1, "die Korrektur bleibt in der Ablage");
        let gross = serde_json::to_vec(&gelesen[0].1).expect("json").len();
        assert!(gross <= 60 * 1024, "nach dem Aufraeumen noch {gross} Byte");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn die_ablage_trennt_die_mandanten() {
        let base = temp_ablage("mandanten");
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(0.0));
        stats.landing_at =
            Some(chrono::DateTime::<Utc>::from_timestamp(1_755_000_100, 0).expect("Zeit"));
        let n = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        nachtrag_queue::enqueue_in(&base, "GSG", "42", &n).expect("Ablage 42");
        nachtrag_queue::enqueue_in(&base, "GSG", "43", &n).expect("Ablage 43");
        // Eine Datei im Ordner von 42, deren Inhalt 43 gehoert — Pfad luegt.
        let fremd = nachtrag_queue::serialisieren("GSG", "43", &n).expect("JSON");
        std::fs::write(base.join("GSG").join("42").join("fremd-1.json"), fremd).expect("schreiben");
        let fuer_42 = nachtrag_queue::list_in(&base, "GSG", "42");
        assert_eq!(
            fuer_42.len(),
            1,
            "Pilot 42 sieht etwas, das ihm nicht gehoert"
        );
        assert_eq!(nachtrag_queue::list_in(&base, "GSG", "43").len(), 1);
        assert_eq!(
            nachtrag_queue::list_in(&base, "XYZ", "42").len(),
            0,
            "andere VA, gleiche Pilotennummer"
        );
        assert_eq!(
            nachtrag_queue::list_in(&base, "", "42").len(),
            0,
            "leere Identitaet liefert nichts"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    /// ⚠ Jede Revision ist ihre eigene Datei (Runde 14, High 2).
    ///
    /// Der Streamer kann waehrend des Sendens einen neueren Nachtrag
    /// schreiben. Frueher landete er in derselben Datei, und der ACK-Task
    /// las erst die Revision und loeschte dann — ein Fenster, in dem die
    /// neuere Datei unter ihm weggetauscht werden konnte. Jetzt loescht das
    /// PUBACK genau die gesendete Datei; die neuere bleibt, und der Ordner
    /// sortiert nach Revision.
    #[test]
    fn jede_revision_ist_ihre_eigene_datei() {
        let base = temp_ablage("revision");
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(0.0));
        stats.landing_at =
            Some(chrono::DateTime::<Utc>::from_timestamp(1_755_000_100, 0).expect("Zeit"));
        stats.bahn_revision = 2;
        let alt = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        let pfad2 = nachtrag_queue::enqueue_in(&base, "GSG", "42", &alt).expect("Ablage");
        stats.bahn_revision = 3;
        let neu = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        let pfad3 = nachtrag_queue::enqueue_in(&base, "GSG", "42", &neu).expect("Ablage");
        assert_ne!(
            pfad2, pfad3,
            "zwei Revisionen in derselben Datei — das Lese-dann-Loesche-Fenster ist zurueck"
        );
        // Atomar geschrieben: keine Temp-Datei bleibt liegen.
        let ordner = pfad2.parent().expect("Ordner");
        assert_eq!(
            std::fs::read_dir(ordner).expect("lesen").count(),
            2,
            "eine Temp-Datei liegt neben der Ablage"
        );
        let liste = nachtrag_queue::list_in(&base, "GSG", "42");
        assert_eq!(liste.len(), 2);
        assert_eq!(
            liste[0].1.herkunft.bahn_revision,
            Some(2),
            "aelteste Revision muss zuerst gehen"
        );
        assert_eq!(liste[1].1.herkunft.bahn_revision, Some(3));
        // Das PUBACK von Revision 2 nimmt NUR Revision 2.
        assert!(nachtrag_queue::entfernen(&pfad2));
        assert!(!pfad2.exists());
        assert!(
            pfad3.exists(),
            "die Zustellung von Revision 2 hat Revision 3 geloescht"
        );
        // Unlesbares bleibt liegen: `list_in` liefert es nicht, also wird es
        // nie gesendet und nie entfernt.
        std::fs::write(ordner.join("x-1-r000009.json"), "{ halb geschrie").expect("schreiben");
        assert_eq!(
            nachtrag_queue::list_in(&base, "GSG", "42").len(),
            1,
            "unlesbare Datei wurde geliefert"
        );
        assert!(ordner.join("x-1-r000009.json").exists());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// ⚠ Der Nachtrag traegt das Drittel — vertrauens-geriegelt wie der
    /// Touchdown.
    #[test]
    fn der_nachtrag_traegt_das_drittel() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        assert!(
            stats.landing_touchdown_zone.is_some(),
            "Vorrichtung ohne Drittel"
        );
        // Der Nachtrag adressiert ueber den Aufsetzzeitpunkt — ohne ihn
        // gibt es keine Zeile, die er treffen koennte.
        stats.landing_at = Some(td_at());

        let nachtrag = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        // Der Riegel selbst faehrt mit — sonst bleibt er beim Recorder auf
        // dem Stand des Aufsetzens, waehrend die Bahn wechselt.
        // Konkret, nicht gegen dieselbe Ableitung: Auf dem geplanten Platz,
        // mittig aufgesetzt, IST die Bahn vertrauenswuerdig.
        assert_eq!(
            nachtrag.runway_geometry_trusted,
            Some(true),
            "der Vertrauens-Riegel erreicht den Nachtrag nicht oder ist falsch"
        );
        assert_eq!(nachtrag.runway_geometry_reason, None);
        assert_eq!(
            nachtrag.landing_touchdown_zone,
            Some(1),
            "die Vorrichtung setzt an der Schwelle auf"
        );
        assert_eq!(
            nachtrag.landing_touchdown_zone, stats.landing_touchdown_zone,
            "das Drittel erreicht den Nachtrag nicht — die Spalte beim \
             Recorder bleibt dann auf dem Stand des Aufsetzens"
        );
        // Und derselbe Riegel wie am Touchdown: Ein Treffer 3,5 km neben
        // der Achse traegt kein Drittel.
        if let Some(rm) = stats.runway_match.as_mut() {
            rm.centerline_distance_m = 3500.0;
        }
        assert_eq!(
            zone_fuer_leitung(&flight, &stats),
            None,
            "eine nicht vertrauenswuerdige Bahn bekommt ein Drittel"
        );
    }

    /// ⚠ Ein Querversatz innerhalb der Toleranz wird MITGEZOGEN.
    ///
    /// Runde 4 (N18): Bis 5 m quer gilt die Achse als dieselbe — dieser
    /// Versatz wanderte aber unkorrigiert in `max_lateral_offset_m` und
    /// `min_edge_clearance_m`, auf einer 45-m-Bahn 22 % der Halbbreite,
    /// beides scorewirksam. Die Achse liegt jetzt 3 m rechts (Norden bei
    /// der westlaufenden 26R): Jeder Punkt muss 3 m weiter LINKS stehen.
    #[test]
    fn die_spur_rueckt_quer_mit_der_parallel_versetzten_achse() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        let mut neu = alt.clone();
        // ~3 m nach Norden, an beiden Enden — parallel, innerhalb der 5 m.
        let d = 3.0 / 111_320.0;
        neu.threshold_lat += d;
        neu.end_lat += d;
        let alte_achse = alte_achse_von(&stats).expect("alte Achse");
        let (_, dq) = runway::projiziere_auf_bahn(
            alt.threshold_lat,
            alt.threshold_lon,
            alt.end_lat,
            alt.end_lon,
            neu.threshold_lat,
            neu.threshold_lon,
        );
        assert!(
            (dq.abs() - 3.0).abs() < 0.2 && dq.abs() <= crate::grenzwerte::grenzwerte().ausrollen.achse_toleranz_m.wert,
            "die Vorrichtung versetzt nicht um ~3 m quer: {dq}"
        );
        stats.runway_match = Some(neu);
        spur_auf_neue_achse(&mut stats, Some(&alte_achse));
        assert_eq!(stats.bahn_spur.len(), 3, "verworfen statt verschoben");
        for ((_, qr), (_, qr_alt)) in
            stats
                .bahn_spur
                .iter()
                .zip([(0.0f32, 1.0f32), (500.0, -2.0), (1200.0, 3.0)])
        {
            assert!(
                ((qr - qr_alt) as f64 + dq).abs() < 0.05,
                "Querwert nicht um den Achsversatz korrigiert: {qr} statt {}",
                qr_alt as f64 - dq
            );
        }
        // ⚠ Runde 6, Befund 1: Nach der Verschiebung ist NICHT mehr der
        // Punkt bei 1200 m (3,0 → 0,01) der groesste, sondern der bei
        // 500 m (−2,0 → −4,99). Meine erste Fassung schob den alten
        // Argmax und meldete 0,01 m neben einer Spur bei −4,99 — ein Rad
        // neben der Bahn, verschwiegen. Genau diese Vorrichtung ist die
        // Gegenprobe.
        let m = stats.bahn_max_querversatz_m.expect("max quer");
        assert!(
            (m - (-2.0 - dq)).abs() < 0.05,
            "max quer: {m}, erwartet {} — der Argmax ist gekippt und wurde nicht \
             neu bestimmt",
            -2.0 - dq
        );
    }

    /// ⚠ Eine GEDREHTE Achse — der Interpolationsterm von `quer_bei`.
    ///
    /// Runde 7 (N2): In allen acht Vorrichtungen war `dt_q ≈ de_q`, die
    /// Korrektur also konstant — der lineare Term zwischen Schwelle und
    /// Ende war von keinem Test beruehrt. Ersetzt man `quer_bei` durch die
    /// Konstante `dt_q`, blieb alles gruen. Hier: Schwelle +4 m, Ende −4 m
    /// (beides in der Toleranz, ~0,15° auf 3,6 km — die Szenerie liefert
    /// solche Kurskorrekturen). Bei 500 m muss die Korrektur ~2,9 m sein,
    /// nicht 4.
    #[test]
    fn eine_gedrehte_achse_korrigiert_quer_entlang_der_bahn() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        stats.bahn_fenster_zu_laengs_m = Some(1400.0);
        let d = 4.0 / 111_320.0;
        let mut neu = alt.clone();
        neu.threshold_lat += d;
        neu.end_lat -= d;
        let alte_achse = alte_achse_von(&stats).expect("alte Achse");
        let (dt_l, dt_q) = runway::projiziere_auf_bahn(
            alt.threshold_lat,
            alt.threshold_lon,
            alt.end_lat,
            alt.end_lon,
            neu.threshold_lat,
            neu.threshold_lon,
        );
        let (de_l, de_q) = runway::projiziere_auf_bahn(
            alt.threshold_lat,
            alt.threshold_lon,
            alt.end_lat,
            alt.end_lon,
            neu.end_lat,
            neu.end_lon,
        );
        assert!(
            dt_q > 3.5
                && de_q < -3.5
                && dt_q.abs() <= crate::grenzwerte::grenzwerte().ausrollen.achse_toleranz_m.wert
                && de_q.abs() <= crate::grenzwerte::grenzwerte().ausrollen.achse_toleranz_m.wert,
            "die Vorrichtung dreht nicht ({dt_q} / {de_q}) — dann prueft dieser Test die Konstante"
        );
        // Die Definition der Korrektur, aus den GEMESSENEN Achsen:
        let erwartet_bei = |l: f64| dt_q + (de_q - dt_q) * ((l - dt_l) / (de_l - dt_l));
        stats.runway_match = Some(neu);
        spur_auf_neue_achse(&mut stats, Some(&alte_achse));
        assert_eq!(stats.bahn_spur.len(), 3, "verworfen statt verschoben");
        for ((lg, qr), (lg_alt, qr_alt)) in
            stats
                .bahn_spur
                .iter()
                .zip([(0.0f64, 1.0f64), (500.0, -2.0), (1200.0, 3.0)])
        {
            let soll = qr_alt - erwartet_bei(lg_alt);
            assert!(
                (*qr as f64 - soll).abs() < 0.05,
                "bei {lg} m: {qr} statt {soll} — die Korrektur ist konstant statt entlang der Bahn"
            );
        }
        // Der groesste Querversatz: bei 500 m, −2,0 − 2,9 ≈ −4,9 — und NICHT
        // −6,0, was die konstante Korrektur ergaebe.
        let m = stats.bahn_max_querversatz_m.expect("max quer");
        let soll = -2.0 - erwartet_bei(500.0);
        assert!((m - soll).abs() < 0.05, "max quer: {m}, erwartet {soll}");
        assert!(
            (m - (-2.0 - dt_q)).abs() > 0.5,
            "das ist die KONSTANTE Korrektur: {m}"
        );
    }

    /// ⚠ Die Ausfahrt gehoert NICHT zum groessten Querversatz.
    ///
    /// Die Spur laeuft ueber das Messfenster hinaus, bis 80 m neben die
    /// Kante. Das Live-Maximum waechst nur bei offenem Fenster. Aus der
    /// ganzen Spur neu bestimmt, wurde aus einer sauberen Landung „Rad
    /// neben der befestigten Flaeche" — im Score und in
    /// `min_edge_clearance_m` (Runde 5, N22, P0).
    #[test]
    fn die_ausfahrt_zaehlt_nicht_zum_groessten_querversatz() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        // Fenster zu bei 1400 m; danach die Ausfahrt: 60 m neben der Bahn.
        stats.bahn_spur.push((1900.0, -60.0));
        stats.bahn_fenster_zu_laengs_m = Some(1400.0);
        stats.bahn_max_querversatz_m = Some(3.0);
        let neu = schwelle_verschoben(&alt, 350.0);
        let alte_achse = alte_achse_von(&stats).expect("alte Achse");
        stats.runway_match = Some(neu);
        spur_auf_neue_achse(&mut stats, Some(&alte_achse));
        let m = stats.bahn_max_querversatz_m.expect("max quer");
        assert!(
            (m - 3.0).abs() < 0.5,
            "der groesste Querversatz ist {m} — die Ausfahrt (−60 m) ist in den \
             Score gerutscht"
        );
    }

    /// ⚠ Eine fertige Spur gegen eine andere Achse wird nicht mehr GESENDET.
    ///
    /// Sie bleibt lokal (Runde 4, N13), aber sie ist gegen eine Bahn
    /// projiziert, die nicht mehr gilt — neben Marken aus der neuen. Also
    /// „veraltet": kein Spur-Block mehr, beim Recorder bleibt der letzte
    /// in sich stimmige Stand (Runde 5, N24).
    #[test]
    fn eine_veraltete_spur_wird_nicht_mehr_gesendet() {
        let flight = flight_fixture("EDDP");
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        stats.rollout_finalized = true;
        stats.bahn_spur_laeuft = false;
        assert!(
            spur_block(&flight, &stats).is_some(),
            "Vorrichtung: fertig und gueltig"
        );
        let mut neu = alt.clone();
        neu.threshold_lat += 0.0009;
        neu.end_lat += 0.0009;
        let alte_achse = alte_achse_von(&stats).expect("alte Achse");
        stats.runway_match = Some(neu);
        spur_auf_neue_achse(&mut stats, Some(&alte_achse));
        assert_eq!(stats.bahn_spur.len(), 3, "lokal bleibt sie");
        assert!(
            stats.bahn_spur_bezug_veraltet,
            "nicht als veraltet markiert"
        );
        assert!(
            spur_block(&flight, &stats).is_none(),
            "eine Spur im alten Bezug geht auf die Leitung — neben Marken aus der neuen Bahn"
        );
    }

    /// ⚠ Ist der Nachtrag gesendet, baut der Streamer beim Rollen NICHTS mehr.
    ///
    /// AIB 429 (HECA, 12.10.2026): Bis 2.0.12 entstanden Spur-Block,
    /// Rollwegkarte und Ausfahrten in jedem Takt und wurden erst danach
    /// verworfen — 460–600 ms je Takt, das ganze Rollen lang. Gezaehlt wird
    /// an `bahn_felder`, der teuren Stelle selbst. Die Gegenproben (noch nie
    /// gesendet, neue Revision) bauen und senden weiterhin.
    #[test]
    fn nach_dem_nachtrag_baut_der_streamer_nichts_mehr() {
        let flight = flight_fixture("EDDP");
        let mut stats = stats_mit_spur(eddp_26r_match_with_raw_td(0.0));
        stats.rollout_finalized = true;
        stats.bahn_spur_laeuft = false;
        stats.sampler_touchdown_at = Some(td_at());
        stats.bahn_revision = 3;
        let td_ms = td_at().timestamp_millis();
        let aufrufe = || BAHN_FELDER_AUFRUFE.with(|z| z.get());

        BAHN_FELDER_AUFRUFE.with(|z| z.set(0));
        assert!(
            rollout_nachtrag_faellig(&flight, &stats, Some((td_ms, 3))).is_none(),
            "ein schon gesendeter Nachtrag geht erneut raus"
        );
        assert_eq!(
            aufrufe(),
            0,
            "der Nachtrag ist gesendet, trotzdem wurde die Rollwegkarte gebaut"
        );

        BAHN_FELDER_AUFRUFE.with(|z| z.set(0));
        let (schluessel, nachtrag) =
            rollout_nachtrag_faellig(&flight, &stats, None).expect("noch nie gesendet");
        assert_eq!(schluessel, (td_ms, 3));
        assert_eq!(nachtrag.touchdown_at, td_ms);
        assert!(nachtrag.bahn.is_some(), "der Spur-Block fehlt im Nachtrag");
        assert_eq!(aufrufe(), 1, "Gegenprobe: der erste Nachtrag wird gebaut");

        BAHN_FELDER_AUFRUFE.with(|z| z.set(0));
        let (schluessel, _) = rollout_nachtrag_faellig(&flight, &stats, Some((td_ms, 2)))
            .expect("eine neue Revision wird erneut gesendet");
        assert_eq!(schluessel, (td_ms, 3));
        assert_eq!(aufrufe(), 1, "Gegenprobe: die neue Revision wird gebaut");
    }

    /// ⚠ Eine FERTIGE Spur wird nie verworfen — auch nicht bei Achswechsel.
    ///
    /// Runde 4 (N13): Das Einreich-Upgrade verwarf auf einem Ausweichflug
    /// die fertige Spur und meldete danach `rollout_final: false` an eine
    /// laengst endgueltige Zeile.
    #[test]
    fn eine_fertige_spur_ueberlebt_den_achswechsel() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        stats.rollout_finalized = true;
        stats.bahn_spur_laeuft = false;
        let mut neu = alt.clone();
        neu.threshold_lat += 0.0009;
        neu.end_lat += 0.0009;
        let alte_achse = alte_achse_von(&stats).expect("alte Achse");
        stats.runway_match = Some(neu);
        spur_auf_neue_achse(&mut stats, Some(&alte_achse));
        assert_eq!(
            stats.bahn_spur.len(),
            3,
            "die fertige Spur wurde verworfen — danach meldet der Nachtrag \
             `rollout_final: false` an eine endgueltige Zeile"
        );
        assert_eq!(stats.bahn_raeum_laengs_m, Some(1500.0));
        assert!(stats.rollout_finalized);
        assert!(stats.bahn_spur_bezug_veraltet, "sie gilt aber als veraltet");
    }

    /// ⚠ Der Nachtrag laesst den Spur-Block WEG, wenn die Spur nicht fertig ist.
    ///
    /// Nicht `null` — `null` loescht beim Recorder, und `rollout_final:
    /// false` dreht eine endgueltige Zeile zurueck.
    #[test]
    fn der_nachtrag_laesst_eine_unfertige_spur_weg() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        stats.landing_at = Some(td_at());

        // Unfertig: nichts finalisiert, Spur leer.
        let n = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        assert!(
            n.bahn.is_none(),
            "unfertige Spur wird als Block gesendet — als null"
        );
        let json = serde_json::to_value(&n).expect("json");
        assert!(
            json.get("rollout_final").is_none() && json.get("lateral_samples").is_none(),
            "Spur-Felder stehen auf der Leitung, obwohl der Block fehlen soll"
        );
        assert!(
            json.get("bahn_revision").is_some(),
            "die Herkunft muss trotzdem mit"
        );

        // Fertig: Block dabei, endgueltig.
        stats.rollout_finalized = true;
        stats.bahn_spur_laeuft = false;
        stats.bahn_spur = vec![(0.0, 1.0), (500.0, -2.0)];
        let n = bahn_nachtrag_bauen(&flight, &stats).expect("Nachtrag");
        assert!(n.bahn.as_ref().is_some_and(|b| b.rollout_final));
    }

    /// Gleiche Achse, gleicher Nullpunkt: nichts passiert.
    #[test]
    fn dieselbe_achse_laesst_die_spur_in_ruhe() {
        let alt = eddp_26r_match_with_raw_td(0.0);
        let mut stats = stats_mit_spur(alt.clone());
        let alte_achse = AlteAchse {
            t_lat: alt.threshold_lat,
            t_lon: alt.threshold_lon,
            e_lat: alt.end_lat,
            e_lon: alt.end_lon,
            nutzbare_laenge_m: bahnmasse(&alt).nutzbare_laenge_m,
        };
        spur_auf_neue_achse(&mut stats, Some(&alte_achse));
        assert_eq!(
            stats.bahn_spur,
            vec![(0.0, 1.0), (500.0, -2.0), (1200.0, 3.0)]
        );
        assert_eq!(stats.bahn_raeum_laengs_m, Some(1500.0));
    }

    /// ⚠ Jede Zuordnung erhoeht die Revision — die Zahl, die beim
    /// Recorder entscheidet, welcher Nachtrag gewinnt.
    #[test]
    fn jede_zuordnung_erhoeht_die_revision() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());

        let (mut stats, mut snap) = eddp_26r_touchdown_stats();
        snap.simulator = sim_core::Simulator::Msfs2024;
        assert_eq!(stats.bahn_revision, 0, "vor dem Aufsetzen keine Fassung");

        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        assert_eq!(stats.bahn_revision, 1);
        assert_eq!(bahn_herkunft(&stats).bahn_revision, Some(1));
        // ⚠ Wer die Bahn aendert, setzt die Fahne — auch beim Aufsetzen.
        // Ohne sie bleibt eine Zuordnung ohne Nachtrag, wenn der Streamer
        // nie sendet (externe QS, 02.09.2026, P1-B).
        assert!(
            stats.bahn_nachtrag_offen,
            "das Aufsetzen meldet keinen Nachtrag an"
        );
        stats.bahn_nachtrag_offen = false;

        stats.szenerie_auskunft = arc_some(eddp_szenerie(0.0));
        stats.szenerie_auskunft_stand = 5;
        assert!(bahn_am_aufsetzpunkt_nachholen(&mut stats, &flight));
        assert_eq!(
            stats.bahn_revision, 2,
            "die Revision waechst beim Nachholen nicht — dann kann der \
             Recorder die neuere Zuordnung nicht von der alten trennen"
        );
        assert!(
            stats.bahn_nachtrag_offen,
            "das Nachholen meldet keinen Nachtrag an — wo der Streamer nie \
             sendet, erreicht die nachgeholte Bahn den Recorder dann nie"
        );

        // ⚠ Und sie ist NICHT die Anfragenummer der Szenerie. Die stand
        // hier auf 5.
        assert_ne!(
            bahn_herkunft(&stats).bahn_revision,
            Some(stats.korrelierter_szenerie_stand),
            "auf der Leitung steht die prozesslokale Anfragenummer — die \
             faengt nach einem App-Neustart wieder bei null an"
        );
    }

    /// ⚠ Auch das Navdaten-Upgrade beim Einreichen zaehlt als Zuordnung.
    ///
    /// Es ersetzt dieselben Bahnfelder wie das Nachholen. Bis zur dritten
    /// QS-Runde lief es an allem vorbei: keine neue Revision, kein
    /// nachgefuehrtes Drittel, kein Nachtrag zum Recorder — und der
    /// uebernimmt vom PIREP nur Punkte und Noten. Die Touchdown-Zeile
    /// waere auf der vorlaeufigen OurAirports-Geometrie stehengeblieben.
    #[test]
    fn das_navdaten_upgrade_erhoeht_die_revision_und_meldet_sich() {
        let flight = flight_fixture("EDDF"); // Divert nach EDDP
        let (mut stats, snap) = divert_score_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        // Vorlaeufig: OurAirports, weil die Abfrage noch laeuft.
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        let revision_vorher = stats.bahn_revision;
        stats.bahn_nachtrag_offen = false;

        // Die Abfrage kommt zurueck — mit versetzter Schwelle.
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        assert!(
            bahn_upgrade_anwenden(&flight, &mut stats),
            "das Upgrade meldet keine Aenderung, obwohl es die Quelle wechselt"
        );
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
        assert_eq!(
            stats.bahn_revision,
            revision_vorher + 1,
            "die Revision waechst nicht — dann sieht der Recorder keine \
             Kante und behaelt die alte Geometrie"
        );
        assert!(
            stats.bahn_nachtrag_offen,
            "der Nachtrag wird nicht angemeldet — dann geht die bessere \
             Geometrie nie hinaus"
        );
        assert_eq!(
            stats.landing_touchdown_zone,
            bahn_herkunft(&stats).td_third,
            "das Drittel folgt dem Upgrade nicht"
        );

        // Und ein zweiter Lauf ohne neue Navdaten ist ein Nichts.
        stats.bahn_nachtrag_offen = false;
        let revision_nach_upgrade = stats.bahn_revision;
        assert!(
            !bahn_upgrade_anwenden(&flight, &mut stats),
            "ein Aufruf ohne neue Navdaten meldet eine Aenderung"
        );
        assert_eq!(stats.bahn_revision, revision_nach_upgrade);
        assert!(
            !stats.bahn_nachtrag_offen,
            "ein Nichts loest einen leeren Nachtrag aus"
        );
    }

    /// ⚠ Das Nachholen rechnet mit dem AUFSETZPUNKT — und faellt niemals
    /// auf die aktuelle Lage zurueck.
    ///
    /// Fehlt der Aufsetzpunkt (Durchstarten loescht ihn), passiert GAR
    /// NICHTS: kein Bericht, keine Zuordnung. Der erste Entwurf pruefte
    /// in einem Lock und rief in einem zweiten — dazwischen konnte der
    /// Sampler loeschen, und der Aufruf fiel auf die Steigposition
    /// zurueck (QS-Befund 3, erste Runde v1.7.15).
    #[test]
    fn ohne_aufsetzpunkt_holt_das_nachholen_nichts_nach() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());

        let (mut stats, _snap) = eddp_26r_touchdown_stats();
        stats.szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
            staende: Vec::new(),
            icao: "EDDP".to_string(),
            bahnen: Vec::new(),
            rollwege: Vec::new(),
            quelle: "msfs".to_string(),
        });
        stats.szenerie_auskunft_stand = 5;

        // Der Aufsetz-Simulator ist gesetzt — sonst prueft dieser Test
        // nur, dass er fehlt, und der Aufsetzpunkt bliebe unbeteiligt.
        stats.aufsetz_simulator = Some(sim_core::Simulator::Msfs2024);

        // Durchstarten: der Aufsetzpunkt ist weg.
        stats.landing_lat = None;
        stats.landing_lon = None;

        assert!(
            !bahn_am_aufsetzpunkt_nachholen(&mut stats, &flight),
            "ohne Aufsetzpunkt wird nachgeholt — das rechnet mit der \
             aktuellen Lage"
        );
        assert!(
            stats.runway_match.is_none(),
            "es wurde eine Bahn gesetzt, obwohl es keinen Aufsetzpunkt gibt"
        );
        assert!(stats.szenerie_uebernahme.is_none());
        assert_eq!(stats.korrelierter_szenerie_stand, 0);
    }

    #[test]
    fn correlate_clears_match_fields_when_no_runway_nearby() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        // Pretend an earlier correlation already matched something.
        let mut td = buf_sample(td_at(), true);
        td.lat = EDDP_26R_THR_LAT;
        td.lon = EDDP_26R_THR_LON;
        td.heading_true_deg = EDDP_26R_HEADING;
        stats.snapshot_buffer.push_back(td);
        let snap = rollout_snap();
        let td_buf = touchdown_buffer_sample(&stats);
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        assert!(stats.runway_match.is_some());

        // Now correlate a mid-ocean touchdown — all match fields reset.
        let mut stats2 = FlightStats::default();
        let mut ocean = buf_sample(td_at(), true);
        ocean.lat = 0.0;
        ocean.lon = -30.0;
        stats2.snapshot_buffer.push_back(ocean);
        let td_buf2 = touchdown_buffer_sample(&stats2);
        correlate_touchdown_runway(&mut stats2, &snap, &flight, td_buf2.as_ref());
        assert!(stats2.runway_match.is_none());
        assert!(stats2.runway_source.is_none());
        assert!(stats2.runway_nav_cycle.is_none());
        assert!(stats2.runway_nav_geometry.is_none());
    }

    // ---- v0.16.24: divert runway-navdata correlation ----

    /// Minimal EDDP NavAirport fixture whose 26R threshold sits exactly on
    /// the bundled OurAirports coordinates, so a touchdown at
    /// (EDDP_26R_THR_LAT, EDDP_26R_THR_LON) matches by both sources. Lets us
    /// prove the on-plan path is unchanged AND that a divert reaches the
    /// cached navdata. (08L is the opposite end; same geometry as the CSV.)
    /// Die MSFS-Szenerie zu `eddp_nav_fixture` — GEOMETRISCH PASSEND.
    ///
    /// ⚠ Die erste Fassung schrieb `gegenende` als Schwelle + 0,05° Ost.
    /// Die 26R laeuft aber nach Westen: Die Bahnmitte lag damit ~3,5 km
    /// neben der Nav-Achse, `passende_szenerie_bahn` verwarf die Bahn,
    /// und der Uebernahmebericht war leer — was ein `is_some()`-Test
    /// nicht sah (externe QS, 02.09.2026, P2-J). Das Gegenende ist jetzt
    /// die 08L-Schwelle, wie in den Navdaten.
    fn eddp_szenerie(versetzte_schwelle_m: f64) -> sim_core::szenerie::SzenerieFlughafen {
        sim_core::szenerie::SzenerieFlughafen {
            staende: Vec::new(),
            icao: "EDDP".to_string(),
            bahnen: vec![sim_core::szenerie::SzenerieBahn {
                bezeichner: "26R".to_string(),
                kurs_grad: 265.7,
                breite_m: 45.0,
                laenge_m: 3600.0,
                versetzte_schwelle_m,
                schwelle: (EDDP_26R_THR_LAT, EDDP_26R_THR_LON),
                gegenende: (EDDP_08L_THR_LAT, EDDP_08L_THR_LON),
                belag_code: 1,
                kurs_bestaetigt_grad: None,
                kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
            }],
            rollwege: Vec::new(),
            quelle: "msfs".to_string(),
        }
    }

    fn eddp_nav_fixture() -> aeroacars_mqtt::navdata::NavAirport {
        use aeroacars_mqtt::navdata::{NavAirport, NavPoint, NavRunway};
        NavAirport {
            cycle: "2604".to_string(),
            valid_to: "2026-05-14".to_string(),
            icao: "EDDP".to_string(),
            name: "Leipzig/Halle".to_string(),
            latitude: 51.432,
            longitude: 12.241,
            elevation_ft: Some(465),
            runways: vec![
                NavRunway {
                    designator: "26R".to_string(),
                    magnetic_course: 265.7,
                    true_course: 265.7,
                    length_ft: 11_811,
                    width_ft: Some(148),
                    surface: Some("ASP".to_string()),
                    threshold: NavPoint {
                        lat: EDDP_26R_THR_LAT,
                        lon: EDDP_26R_THR_LON,
                        elev_ft: Some(465),
                    },
                    far_end: NavPoint {
                        lat: EDDP_08L_THR_LAT,
                        lon: EDDP_08L_THR_LON,
                        elev_ft: Some(465),
                    },
                    displaced_threshold_ft: 0,
                    ils: None,
                    glideslope_angle: 3.0,
                    tch_ft: 50,
                },
                NavRunway {
                    designator: "08L".to_string(),
                    magnetic_course: 85.7,
                    true_course: 85.7,
                    length_ft: 11_811,
                    width_ft: Some(148),
                    surface: Some("ASP".to_string()),
                    threshold: NavPoint {
                        lat: EDDP_08L_THR_LAT,
                        lon: EDDP_08L_THR_LON,
                        elev_ft: Some(465),
                    },
                    far_end: NavPoint {
                        lat: EDDP_26R_THR_LAT,
                        lon: EDDP_26R_THR_LON,
                        elev_ft: Some(465),
                    },
                    displaced_threshold_ft: 0,
                    ils: None,
                    glideslope_angle: 3.0,
                    tch_ft: 50,
                },
            ],
        }
    }

    // ---- tch_actual_from_buffer ----

    /// Point at fractional distance `frac` along the threshold→far_end
    /// line (0 = threshold, 1 = far_end, negative = before threshold).
    /// Linear lat/lon interpolation — plenty accurate over one runway
    /// length for these tests.
    fn point_along(thr: (f64, f64), end: (f64, f64), frac: f64) -> (f64, f64) {
        (
            thr.0 + (end.0 - thr.0) * frac,
            thr.1 + (end.1 - thr.1) * frac,
        )
    }

    fn tch_sample(lat: f64, lon: f64, agl_ft: f32) -> TelemetrySample {
        let mut s = buf_sample(td_at(), false);
        s.lat = lat;
        s.lon = lon;
        s.agl_ft = agl_ft;
        s
    }

    fn eddp_26r_runway() -> aeroacars_mqtt::navdata::NavRunway {
        eddp_nav_fixture()
            .runways
            .into_iter()
            .next()
            .expect("26R fixture")
    }

    /// v0.19.x FIX — the exact bug: the buffer's chronologically FIRST
    /// entry is already past the threshold (late engagement / buffer-
    /// cadence mismatch). The old code accepted it anyway; this must
    /// return None, matching `runway_tch_actual_ft`'s own doc comment.
    #[test]
    fn tch_actual_from_buffer_none_when_no_pre_threshold_sample_was_ever_seen() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        let mut buffer = std::collections::VecDeque::new();
        // Every sample already past the threshold — no bracket ever observed.
        let (lat, lon) = point_along(thr, end, 0.005);
        buffer.push_back(tch_sample(lat, lon, 45.0));
        let (lat2, lon2) = point_along(thr, end, 0.01);
        buffer.push_back(tch_sample(lat2, lon2, 20.0));

        assert_eq!(tch_actual_from_buffer(&rw, &buffer), None);
    }

    /// Genuine bracket: at least one pre-threshold sample, then the first
    /// post-threshold one — that sample's AGL is the actual TCH.
    #[test]
    fn tch_actual_from_buffer_returns_first_post_threshold_agl_when_bracketed() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        let mut buffer = std::collections::VecDeque::new();
        let (lat0, lon0) = point_along(thr, end, -0.01);
        buffer.push_back(tch_sample(lat0, lon0, 62.0)); // pre-threshold
        let (lat1, lon1) = point_along(thr, end, 0.002);
        buffer.push_back(tch_sample(lat1, lon1, 48.0)); // first past-threshold — this one wins
        let (lat2, lon2) = point_along(thr, end, 0.01);
        buffer.push_back(tch_sample(lat2, lon2, 20.0)); // later — must NOT win

        assert_eq!(tch_actual_from_buffer(&rw, &buffer), Some(48.0));
    }

    /// Inventur B1: Die Hoehe an der Schwelle wird zwischen der Probe davor und
    /// der Probe dahinter interpoliert, nicht von der ersten dahinter gelesen.
    #[test]
    fn tch_interpoliert_zwischen_den_proben_vor_und_hinter_der_schwelle() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        let mut buffer = std::collections::VecDeque::new();
        let (a0, o0) = point_along(thr, end, -0.01);
        buffer.push_back(tch_sample(a0, o0, 62.0));
        let (a1, o1) = point_along(thr, end, 0.002);
        buffer.push_back(tch_sample(a1, o1, 48.0));
        // Anteil 0,01 / 0,012 = 5/6 des Weges von 62 nach 48 ft.
        let tch = tch_interpoliert(&rw, &buffer).expect("Schwelle ueberflogen");
        assert!((tch - (62.0 - 14.0 * 5.0 / 6.0)).abs() < 0.2, "TCH {tch}");
        // Gegenprobe P-3.55: Obergrenze aus dem Paket unter den Wert -> unplausibel.
        crate::grenzwerte::mit_ersatz(|g| g.ausrollen.tch_plausibel_max_ft.wert = 50.0, || {
            assert_eq!(tch_interpoliert(&rw, &buffer), None);
        });
        // Der 5-s-Puffer-Weg liefert hier den Rohwert der ersten Probe dahinter —
        // die Interpolation ist genauer und darf davon abweichen.
        assert_eq!(tch_actual_from_buffer(&rw, &buffer), Some(48.0));
    }

    /// QS-Review: Nach einem Tiefanflug/Durchstart reicht die 60-s-Spur ueber
    /// zwei Schwellenuebergaenge — es zaehlt der LETZTE (die Landung).
    #[test]
    fn tch_interpoliert_nimmt_den_letzten_schwellenuebergang() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        let mut buffer = std::collections::VecDeque::new();
        // Erster Ueberflug (Tiefanflug, 20 ft), dann wieder vor die Schwelle.
        for (frac, agl) in [
            (-0.0004, 25.0),
            (0.0004, 15.0),
            (-0.0004, 90.0),
            (-0.0001, 70.0),
            (0.0001, 52.0),
        ] {
            let (a, o) = point_along(thr, end, frac);
            buffer.push_back(tch_sample(a, o, agl));
        }
        let tch = tch_interpoliert(&rw, &buffer).expect("zweiter Ueberflug");
        assert!(
            tch > 50.0 && tch < 70.0,
            "es zaehlt der letzte Uebergang, war {tch}"
        );
    }

    /// QS-Review: Eine Luecke ueber die Schwelle (Sim-Pause) oder ein
    /// unplausibler Wert gibt kein TCH — lieber leer als erfunden.
    #[test]
    fn tch_interpoliert_verwirft_luecken_und_unplausibles() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        let (a0, o0) = point_along(thr, end, -0.2);
        let (a1, o1) = point_along(thr, end, 0.2);
        let mut luecke = std::collections::VecDeque::new();
        luecke.push_back(tch_sample(a0, o0, 60.0));
        luecke.push_back(tch_sample(a1, o1, 50.0));
        assert_eq!(
            tch_interpoliert(&rw, &luecke),
            None,
            "Luecke ueber ~hunderte Meter"
        );
        let (b0, p0) = point_along(thr, end, -0.0005);
        let (b1, p1) = point_along(thr, end, 0.0005);
        let mut hoch = std::collections::VecDeque::new();
        hoch.push_back(tch_sample(b0, p0, 900.0));
        hoch.push_back(tch_sample(b1, p1, 880.0));
        assert_eq!(
            tch_interpoliert(&rw, &hoch),
            None,
            "900 ft ist keine Ueberflughoehe"
        );
    }

    /// Inventur B3: Die vier nachgetragenen Muster erreichen die Tabelle —
    /// mit Vref (Abweichung wird bewertet) und Klassen-Bank-Grenze.
    #[test]
    fn nachgetragene_muster_haben_vref_und_bankgrenze() {
        for (icao, vref, bank) in [
            ("PC12", 85.0, 12.0),
            ("LJ35", 125.0, 8.0),
            ("H25B", 115.0, 8.0),
            ("BE60", 85.0, 15.0),
            ("E13L", 128.0, 8.0),
        ] {
            let l = aircraft_limits_for(icao);
            assert!(!l.is_fallback, "{icao} steht nicht in der Tabelle");
            assert_eq!(l.typical_vref_kt, Some(vref), "{icao}");
            assert_eq!(l.max_bank_landing_deg, bank, "{icao}");
        }
        // Gegenprobe: unbelegte Muster bleiben ehrlich im Rueckfall.
        assert!(aircraft_limits_for("DA50").is_fallback);
        assert!(aircraft_limits_for("YK18").is_fallback);
    }

    /// Inventur (LJ35): Das Learjet-35A-Addon meldet „35A". Mit der
    /// Familienregel loesen Vref, Bank-Grenze und Spannweite auf LJ35 auf.
    #[test]
    fn learjet_35a_loest_ueber_die_familienregel_auf() {
        let l = aircraft_limits_for("35A");
        assert!(!l.is_fallback, "35A muss in der Tabelle landen");
        assert_eq!(l.typical_vref_kt, Some(125.0));
        let spann =
            muster_fuer_typtabelle(Some("35A"), |m| landing_scoring::spurweite::spannweite_m(m));
        assert_eq!(spann, Some(12.04));
        // Gegenprobe: die direkte Schreibweise bleibt unveraendert.
        assert_eq!(aircraft_limits_for("LJ35").typical_vref_kt, Some(125.0));
    }

    /// QS-Review (zweite Pruefung): Beim ersten Korrelieren eines Aufsetzens
    /// zaehlt nur dieses Aufsetzen. Beim Nachholen derselben Landung bleibt der
    /// Wert; auf einer anderen Bahn nie.
    #[test]
    fn tch_wert_bleibt_nur_beim_nachholen_derselben_bahn_stehen() {
        let eddp = Some(("EDDP".to_string(), "26R".to_string()));
        let andere = Some(("EDDP".to_string(), "08L".to_string()));
        // Frisches Aufsetzen (Touch-and-Go) auf derselben Bahn, nichts gemessen: kein alter Wert.
        assert_eq!(
            tch_nach_korrelation(None, Some(48.0), &eddp, &eddp, false),
            None
        );
        // Nachholen derselben Landung: Wert bleibt.
        assert_eq!(
            tch_nach_korrelation(None, Some(48.0), &eddp, &eddp, true),
            Some(48.0)
        );
        // Nachholen, aber die Bahn hat gewechselt: kein Wert der falschen Bahn.
        assert_eq!(
            tch_nach_korrelation(None, Some(48.0), &eddp, &andere, true),
            None
        );
        // Ohne vorherige Bahn nichts zu behalten.
        assert_eq!(
            tch_nach_korrelation(None, Some(48.0), &None, &eddp, true),
            None
        );
        // Ein neu gemessener Wert gewinnt immer.
        assert_eq!(
            tch_nach_korrelation(Some(55.0), Some(48.0), &eddp, &eddp, true),
            Some(55.0)
        );
        assert_eq!(
            tch_nach_korrelation(Some(55.0), Some(48.0), &eddp, &eddp, false),
            Some(55.0)
        );
    }

    /// Leicht negative Hoehe ueber Grund an der Schwelle (Messrauschen) ist plausibel.
    #[test]
    fn tch_interpoliert_laesst_minimal_negative_hoehe_zu() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        let (a0, o0) = point_along(thr, end, -0.0005);
        let (a1, o1) = point_along(thr, end, 0.0005);
        let mut buffer = std::collections::VecDeque::new();
        buffer.push_back(tch_sample(a0, o0, 1.0));
        buffer.push_back(tch_sample(a1, o1, -2.0));
        let tch = tch_interpoliert(&rw, &buffer).expect("-0,5 ft ist plausibel");
        assert!(tch < 0.0 && tch > -5.0, "{tch}");
    }

    #[test]
    fn tch_interpoliert_ohne_klammer_gibt_nichts() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        // Spur beginnt schon hinter der Schwelle.
        let mut buffer = std::collections::VecDeque::new();
        let (a, o) = point_along(thr, end, 0.005);
        buffer.push_back(tch_sample(a, o, 40.0));
        assert_eq!(tch_interpoliert(&rw, &buffer), None);
        // Schwelle nie erreicht.
        let mut buffer = std::collections::VecDeque::new();
        let (a, o) = point_along(thr, end, -0.02);
        buffer.push_back(tch_sample(a, o, 90.0));
        assert_eq!(tch_interpoliert(&rw, &buffer), None);
        assert_eq!(
            tch_interpoliert(&rw, &std::collections::VecDeque::new()),
            None
        );
    }

    /// Die Schwellenspur nimmt nur Anflug-Proben im Takt auf und schneidet
    /// Altes ab — auch dann, wenn die neue Probe selbst nicht aufgenommen wird.
    #[test]
    fn schwellenspur_nimmt_anflug_proben_im_takt_und_schneidet_ab() {
        let mut stats = FlightStats::default();
        let t0 = DateTime::<Utc>::from_timestamp(1_790_200_000, 0).unwrap();
        let probe = |ms: i64, agl: f32, boden: bool| TelemetrySample {
            at: t0 + chrono::Duration::milliseconds(ms),
            agl_ft: agl,
            on_ground: boden,
            ..Default::default()
        };
        schwellenspur_fuehren(&mut stats, probe(0, 300.0, false));
        schwellenspur_fuehren(&mut stats, probe(100, 295.0, false)); // zu dicht
        schwellenspur_fuehren(&mut stats, probe(400, 290.0, false));
        schwellenspur_fuehren(&mut stats, probe(800, 900.0, false)); // zu hoch
        schwellenspur_fuehren(&mut stats, probe(1200, 5.0, true)); // am Boden
        assert_eq!(stats.schwellen_spur.len(), 2);
        // Nach 61 s faellt alles heraus, auch ohne neue Aufnahme.
        schwellenspur_fuehren(&mut stats, probe(61_500, 5.0, true));
        assert!(stats.schwellen_spur.is_empty());
    }

    #[test]
    fn tch_actual_from_buffer_none_for_empty_buffer() {
        let rw = eddp_26r_runway();
        assert_eq!(
            tch_actual_from_buffer(&rw, &std::collections::VecDeque::new()),
            None
        );
    }

    #[test]
    fn tch_actual_from_buffer_none_when_touchdown_never_reaches_threshold() {
        let rw = eddp_26r_runway();
        let thr = (rw.threshold.lat, rw.threshold.lon);
        let end = (rw.far_end.lat, rw.far_end.lon);
        let mut buffer = std::collections::VecDeque::new();
        let (lat, lon) = point_along(thr, end, -0.02);
        buffer.push_back(tch_sample(lat, lon, 90.0));
        let (lat2, lon2) = point_along(thr, end, -0.01);
        buffer.push_back(tch_sample(lat2, lon2, 65.0));

        assert_eq!(tch_actual_from_buffer(&rw, &buffer), None);
    }

    /// Build the stats + snap for a touchdown at EDDP/26R, ready for
    /// `correlate_touchdown_runway`.
    fn eddp_26r_touchdown_stats() -> (FlightStats, SimSnapshot) {
        let mut stats = FlightStats::default();
        let mut td = buf_sample(td_at(), true);
        td.lat = EDDP_26R_THR_LAT;
        td.lon = EDDP_26R_THR_LON;
        td.heading_true_deg = EDDP_26R_HEADING;
        stats.snapshot_buffer.push_back(td);
        (stats, rollout_snap())
    }

    /// HARD CONSTRAINT — on-plan landings byte-identical.
    ///
    /// For a non-divert landing the nearest airport IS the planned
    /// `arr_airport`, so the actual-airport-keyed v0.16.24 path must produce
    /// the EXACT same NavAirport lookup, runway match, source, geometry,
    /// cycle and TCH as the old `arr_airport`-keyed path. We prove it by
    /// comparing the v0.16.24 correlation against a hand-rolled OLD-style
    /// `arr_airport`-keyed correlation over the same inputs.
    #[test]
    fn correlate_on_plan_is_byte_identical_to_arr_keyed_path() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        // --- v0.16.24 path (keys on the ACTUAL landing airport) ---
        let (mut stats_new, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats_new);
        correlate_touchdown_runway(&mut stats_new, &snap, &flight, td_buf.as_ref());

        // --- OLD path (keys explicitly on flight.arr_airport) ---
        let (rw_lat, rw_lon, rw_hdg) = td_buf
            .as_ref()
            .map(|s| (s.lat, s.lon, s.heading_true_deg))
            .unwrap();
        let nav_opt = flight
            .navdata
            .lock()
            .unwrap()
            .get(&flight.arr_airport)
            .cloned();
        let old = runway::lookup_runway_with_fallback(rw_lat, rw_lon, rw_hdg, nav_opt.as_ref());
        let (old_match, old_source) = old.expect("old path matched");

        // Source identical.
        assert_eq!(stats_new.runway_source, Some(old_source));
        assert!(matches!(old_source, runway::RunwaySource::Navigraph));
        // RunwayMatch identical field-by-field (RunwayMatch is PartialEq via
        // its derive; compare the whole struct).
        assert_eq!(stats_new.runway_match.as_ref(), Some(&old_match));
        // Nav cycle + geometry identical to what the old path would stamp.
        assert_eq!(stats_new.runway_nav_cycle.as_deref(), Some("2604"));
        let old_geom = nav_opt
            .as_ref()
            .and_then(|a| {
                a.runways
                    .iter()
                    .find(|r| r.designator == old_match.runway_ident)
            })
            .cloned();
        assert_eq!(stats_new.runway_nav_geometry, old_geom);
        // The fallback flag must be false on an on-plan Navigraph match.
        assert!(!stats_new.runway_fallback_at_touchdown);
    }

    /// On-plan with NO navdata cached behaves exactly like the pre-existing
    /// `correlate_resolves_runway_via_ourairports_fallback` test — empty
    /// cache → OurAirports, regardless of the new actual-airport keying.
    #[test]
    fn correlate_on_plan_empty_cache_still_ourairports() {
        let flight = flight_fixture("EDDP");
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        let m = stats.runway_match.as_ref().expect("runway match");
        assert_eq!(m.airport_ident, "EDDP");
        assert_eq!(m.runway_ident, "26R");
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        assert!(stats.runway_fallback_at_touchdown);
        // The actual landing airport was resolved to the planned one.
        assert_eq!(stats.runway_correlation_icao.as_deref(), Some("EDDP"));
    }

    /// DIVERT — the planned `arr_airport` is EDDF but the wheels touch at
    /// EDDP. With EDDP's navdata cached (warmed by the alternate fetch or
    /// the approach pre-fetch), the actual-airport keying makes it reachable
    /// → Navigraph match instead of the OurAirports fallback the old
    /// `arr_airport`-keyed path would have produced.
    #[test]
    fn correlate_divert_uses_navigraph_for_actual_airport_when_cached() {
        // Planned EDDF, but EDDP navdata is what's cached (the divert field).
        let flight = flight_fixture("EDDF");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        let m = stats.runway_match.as_ref().expect("runway match");
        assert_eq!(m.airport_ident, "EDDP");
        assert_eq!(m.runway_ident, "26R");
        // The actual landing airport was resolved to EDDP, not planned EDDF.
        assert_eq!(stats.runway_correlation_icao.as_deref(), Some("EDDP"));
        // And because EDDP's navdata WAS cached, we got Navigraph — the whole
        // point of v0.16.24.
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
        assert!(stats.runway_nav_geometry.is_some());
        assert_eq!(stats.runway_nav_cycle.as_deref(), Some("2604"));
        assert!(!stats.runway_fallback_at_touchdown);
    }

    /// DIVERT with the actual field NOT cached → provisional OurAirports
    /// fallback (never worse than today), the fallback flag is set, and the
    /// actual ICAO is recorded so the on-demand fetch + finalize-with-best
    /// can run.
    #[test]
    fn correlate_divert_falls_back_when_actual_airport_not_cached() {
        let flight = flight_fixture("EDDF"); // planned EDDF, nothing cached
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        let m = stats.runway_match.as_ref().expect("runway match");
        assert_eq!(m.airport_ident, "EDDP");
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        assert!(stats.runway_fallback_at_touchdown);
        // Recorded the ACTUAL landing airport (EDDP), not planned EDDF — this
        // is what drives the on-demand fetch + finalize-with-best.
        assert_eq!(stats.runway_correlation_icao.as_deref(), Some("EDDP"));
    }

    /// finalize-with-best: a touchdown that fell back to OurAirports (actual
    /// field wasn't cached yet) is UPGRADED to Navigraph once the navdata
    /// fetch lands, at record-build time.
    #[test]
    fn finalize_upgrades_provisional_fallback_when_fetch_completes() {
        let flight = flight_fixture("EDDF"); // divert to EDDP, nothing cached
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        // Stamp landing_* coords (finalize reads these) the same way
        // stamp_touchdown_metadata would have.
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        // Provisional: OurAirports.
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));

        // Simulate the on-demand fetch completing: EDDP navdata is now cached.
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        let finalized = finalize_runway_correlation(&flight, &stats);
        // Upgraded to Navigraph.
        assert!(matches!(
            finalized.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
        assert_eq!(
            finalized
                .runway_match
                .as_ref()
                .map(|m| m.runway_ident.as_str()),
            Some("26R")
        );
        assert!(finalized.runway_nav_geometry.is_some());
        assert_eq!(finalized.runway_nav_cycle.as_deref(), Some("2604"));
        // TCH-actual is deliberately NOT recomputed at finalize time (the
        // pre-threshold samples are evicted from the rolling buffer by then),
        // so it stays None — honest "couldn't measure", never a garbage value.
        assert!(finalized.runway_tch_actual_ft.is_none());
    }

    /// finalize-with-best is GRACEFUL: if the fetch never completes (cache
    /// still empty), it returns the provisional OurAirports result verbatim.
    #[test]
    fn finalize_keeps_provisional_when_navdata_unavailable() {
        let flight = flight_fixture("EDDF");
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        // Cache still empty — finalize must keep the provisional fallback.
        let finalized = finalize_runway_correlation(&flight, &stats);
        assert!(matches!(
            finalized.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        assert_eq!(
            finalized
                .runway_match
                .as_ref()
                .map(|m| m.airport_ident.as_str()),
            Some("EDDP")
        );
    }

    /// finalize-with-best is a NO-OP for an on-plan Navigraph match — the
    /// on-plan byte-identical invariant at finalization time. It must return
    /// the stamped values unchanged and never re-run the lookup.
    #[test]
    fn finalize_is_noop_for_on_plan_navigraph_match() {
        let flight = flight_fixture("EDDP");
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        let before_match = stats.runway_match.clone();
        let before_geom = stats.runway_nav_geometry.clone();
        let finalized = finalize_runway_correlation(&flight, &stats);
        assert_eq!(finalized.runway_match, before_match);
        assert_eq!(finalized.runway_nav_geometry, before_geom);
        assert!(matches!(
            finalized.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
    }

    // ---- v0.16.24 (QS-1/QS-3): correlate_airport_icao direct ----
    //
    // These exercise `correlate_airport_icao` against the REAL bundled
    // OurAirports CSV (the same table `find_nearest_airports` reads), so they
    // pin the actual cache-keying decision the whole divert fix turns on.

    /// On-plan at a major airport: the nearest field IS the planned `arr` →
    /// returns `arr` verbatim (the load-bearing on-plan invariant; this is the
    /// fast path that keeps non-diverts byte-identical to the pre-v0.16.24
    /// `arr_airport`-keyed lookup).
    #[test]
    fn correlate_airport_on_plan_returns_arr() {
        let icao = correlate_airport_icao("EDDP", EDDP_26R_THR_LAT, EDDP_26R_THR_LON);
        assert_eq!(icao, "EDDP");
        // Case-insensitive / whitespace-tolerant input still anchors to the
        // uppercased plan.
        let icao2 = correlate_airport_icao("  eddp ", EDDP_26R_THR_LAT, EDDP_26R_THR_LON);
        assert_eq!(icao2, "EDDP");
    }

    /// Near-neighbour pair within 1.5 nm: the wheels touch at LTBJ but the
    /// plan was LTBK (1023 m / 0.55 nm away, a real co-located pair in the
    /// bundled CSV). The nearest field is LTBJ (≠ plan), yet LTBK is in the
    /// candidate set within the ambiguity margin → the tie-break keeps it
    /// anchored to the planned LTBK rather than flipping to the marginally
    /// closer neighbour. Proves an on-plan landing survives threshold-proxy
    /// noise from a stone's-throw neighbour.
    #[test]
    fn correlate_airport_near_neighbour_prefers_planned_arr() {
        // LTBJ RWY le-threshold from the bundled CSV.
        const LTBJ_THR_LAT: f64 = 38.306_598_663_330_08;
        const LTBJ_THR_LON: f64 = 27.152_700_424_194_336;
        let icao = correlate_airport_icao("LTBK", LTBJ_THR_LAT, LTBJ_THR_LON);
        let correlate_airport_ambiguity_nm = crate::grenzwerte::grenzwerte().ausrollen.correlate_airport_ambiguity_nm.wert;
        assert_eq!(
            icao, "LTBK",
            "near-tie within {correlate_airport_ambiguity_nm} nm must keep the planned arr"
        );
    }

    /// Co-located identical-coordinate field: LKER and LKRA share a threshold
    /// to within 12 m in the bundled CSV. A touchdown at LKER planned as LKRA
    /// must resolve to the planned LKRA (the tie-break absorbs the ~0 m gap),
    /// never flip to the co-located twin.
    #[test]
    fn correlate_airport_colocated_returns_planned_arr() {
        // LKER RWY le-threshold from the bundled CSV.
        const LKER_THR_LAT: f64 = 50.406_299_591_064_45;
        const LKER_THR_LON: f64 = 13.741_800_308_227_539;
        let icao = correlate_airport_icao("LKRA", LKER_THR_LAT, LKER_THR_LON);
        assert_eq!(icao, "LKRA");
    }

    /// FIX 2 (QS-1 vanished-arr): the planned field is CLOSED/absent from
    /// OurAirports (a real bush strip like CYWA gets dropped from the parsed
    /// runway table) but a DIFFERENT open airport (EDDP) sits within the
    /// search radius. The bare-nearest logic would key the cache on EDDP — the
    /// wrong field. Because the planned `arr` is unfindable in OurAirports
    /// (`airport_position` == None), the vanished-arr guard PREFERS the planned
    /// ICAO so an on-plan landing at a field missing from our data still keys
    /// on the plan (not the nearby different field). We model the closed strip
    /// with `ZZZZ` (reserved "no airport" code, guaranteed absent from
    /// OurAirports).
    #[test]
    fn correlate_airport_vanished_arr_prefers_planned_not_neighbour() {
        // ZZZZ is not in OurAirports → airport_position is None → the guard
        // fires. EDDP is the only field within 6 nm of this touchdown.
        assert!(
            runway::airport_position("ZZZZ").is_none(),
            "test premise: ZZZZ must be absent from OurAirports"
        );
        let icao = correlate_airport_icao("ZZZZ", EDDP_26R_THR_LAT, EDDP_26R_THR_LON);
        assert_eq!(
            icao, "ZZZZ",
            "a vanished/closed planned arr must NOT resolve to a nearby different field"
        );
    }

    /// Genuine divert (the FIX-2 counter-case): the wheels touch at EDDP but
    /// the plan was EDDF — a real, mapped field ~75 nm away. EDDF is findable
    /// in OurAirports (`airport_position` == Some) but far outside the 6 nm
    /// candidate set, so the vanished-arr guard does NOT fire and we correctly
    /// resolve to the ACTUAL landing field EDDP. This proves FIX 2 doesn't
    /// regress genuine diverts.
    #[test]
    fn correlate_airport_genuine_divert_returns_actual_field() {
        assert!(
            runway::airport_position("EDDF").is_some(),
            "test premise: EDDF must be present in OurAirports"
        );
        let icao = correlate_airport_icao("EDDF", EDDP_26R_THR_LAT, EDDP_26R_THR_LON);
        assert_eq!(
            icao, "EDDP",
            "a genuine divert to a different findable field must resolve to the actual field"
        );
    }

    // ---- v0.16.24 (QS-6): divert PIREP score gets the finalize upgrade ----

    /// EDDP NavAirport whose 26R has a LARGE displaced threshold, so the
    /// Navigraph LDA (length − displaced) is materially shorter than the
    /// OurAirports LDA (no displaced data → full length). The runway-
    /// utilisation sub-score lands in a DIFFERENT band between the two
    /// sources, which is what lets the test observe whether the PIREP score
    /// used the provisional OurAirports geometry or the upgraded Navigraph
    /// geometry. Threshold sits on the bundled CSV coords so both sources
    /// resolve to 26R for the same touchdown.
    fn eddp_nav_fixture_displaced() -> aeroacars_mqtt::navdata::NavAirport {
        let mut apt = eddp_nav_fixture();
        for r in apt.runways.iter_mut() {
            if r.designator == "26R" {
                // 4000 ft displaced → LDA 11811−4000 = 7811 ft (~2381 m) vs
                // the OurAirports full 11811 ft (~3600 m).
                r.displaced_threshold_ft = 4000;
            }
        }
        apt
    }

    /// Runway-match fixture with an explicit RAW (pavement-start-relative)
    /// touchdown distance, for exercising `assess_touchdown`'s displaced-
    /// threshold correction directly.
    fn eddp_26r_match_with_raw_td(
        touchdown_distance_from_threshold_ft: f64,
    ) -> runway::RunwayMatch {
        eddp_26r_match_with_raw_td_and_displacement(touchdown_distance_from_threshold_ft, 0)
    }

    /// v0.19.x FIX: `displaced_threshold_ft` parametrized separately —
    /// `assess_touchdown` now sources the displacement from `RunwayMatch`
    /// itself (available regardless of source) rather than exclusively
    /// from the Navigraph-only `runway_nav_geometry`.
    fn eddp_26r_match_with_raw_td_and_displacement(
        touchdown_distance_from_threshold_ft: f64,
        displaced_threshold_ft: i32,
    ) -> runway::RunwayMatch {
        runway::RunwayMatch {
            airport_ident: "EDDP".to_string(),
            runway_ident: "26R".to_string(),
            heading_true_deg: 265.7,
            length_ft: 11_811.0,
            width_ft: 148.0,
            surface: "ASP".to_string(),
            threshold_lat: EDDP_26R_THR_LAT,
            threshold_lon: EDDP_26R_THR_LON,
            end_lat: 51.431_198,
            end_lon: 12.215_800,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft,
            side: "CENTER".to_string(),
            displaced_threshold_ft,
            // Diese Fixture testet ausdruecklich den Korrektur-Pfad (der
            // Rohwert liegt noch am physischen Bahnanfang) — siehe die
            // Doku der Funktion.
            geometry_implied_displaced_threshold_ft: 0,
        }
    }

    /// **Die 25-kt-Grenze, diesmal im Erzeuger geprüft.** Runde 4 fand,
    /// dass die Senkung im Score-Kern wirkungslos war, weil dieselbe
    /// Grenze im Erzeuger unverändert stand. Runde 5 fand das Spiegelbild:
    /// die Grenze im Erzeuger ließ sich zurückdrehen, ohne dass ein Test
    /// rot wurde — der vorhandene Test ruft die Bewertung direkt auf und
    /// überspringt genau diese Kette. Beide Seiten sind jetzt belegt.
    #[test]
    fn langsame_eigengeschwindigkeit_ueberlebt_den_erzeuger() {
        let stats = FlightStats {
            runway_match: Some(eddp_26r_match_with_raw_td(1500.0)),
            landing_true_airspeed_kt: Some(30.0),
            ..Default::default()
        };
        let mut input = landing_scoring::LandingScoringInput::default();
        fill_v2_rollout_fields(&mut input, &stats, "EDDP");
        assert_eq!(
            input.landing_groundspeed_kt,
            Some(30.0),
            "30 kt müssen den Erzeuger passieren — sonst rechnet die \
             Ausrichtungs-Achse mit der Ersatzannahme von 130 kt"
        );
    }

    /// **Die Eigengeschwindigkeit kommt aus dem Aufsetz-Puffer.** Der
    /// Schnappschuss zeigt beim Stempeln unter Umständen schon das
    /// Ausrollen — im schlechtesten Prozent über vier Sekunden danach.
    /// Der Test setzt beide Quellen weit auseinander, damit ein Rückbau
    /// auf den Schnappschuss sofort auffällt.
    #[test]
    fn eigengeschwindigkeit_kommt_aus_dem_aufsetz_puffer() {
        let mut stats = FlightStats::default();
        let puffer = TelemetrySample {
            true_airspeed_kt: 140.0,
            ..buf_sample(Utc::now(), true)
        };
        let snap = SimSnapshot {
            true_airspeed_kt: 60.0,
            ..SimSnapshot::default()
        };
        stamp_touchdown_metadata(&mut stats, &snap, Utc::now(), Some(&puffer));
        assert_eq!(
            stats.landing_true_airspeed_kt,
            Some(140.0),
            "der gepufferte Aufsetz-Wert muss den späten Schnappschuss schlagen"
        );

        // Ohne Puffer bleibt der Schnappschuss die Rückfallebene — sonst
        // hätten fortgesetzte Flüge gar keine Eigengeschwindigkeit.
        let mut ohne = FlightStats::default();
        stamp_touchdown_metadata(&mut ohne, &snap, Utc::now(), None);
        assert_eq!(ohne.landing_true_airspeed_kt, Some(60.0));
    }

    /// **Die Eigengeschwindigkeit wird beim Touch-and-Go abgeräumt.** Sie
    /// ist das einzige Feld ihrer Kette, das latcht statt zu überschreiben
    /// — ohne Rücksetzer rechnete die zweite Landung mit der
    /// Geschwindigkeit der ersten. Für den Landewind wurde genau das zwei
    /// Runden zuvor repariert; die Eigengeschwindigkeit kam später dazu und
    /// war zunächst ungedeckt (QS-Befund Runde 5).
    #[test]
    fn eigengeschwindigkeit_ueberlebt_kein_touch_and_go() {
        let mut stats = FlightStats::default();
        let snap = SimSnapshot {
            true_airspeed_kt: 140.0,
            wind_direction_deg: Some(270.0),
            wind_speed_kt: Some(18.0),
            ..SimSnapshot::default()
        };
        stamp_touchdown_metadata(&mut stats, &snap, Utc::now(), None);
        assert_eq!(stats.landing_true_airspeed_kt, Some(140.0));
        assert_eq!(stats.landing_wind_direction_deg, Some(270.0));

        // Der ECHTE Rücksetzer, nicht nachgestellt. Beide Aufrufstellen
        // (Touch-and-Go und Mehrfach-Aufsetzen) gehen durch diese eine
        // Funktion — deshalb deckt der Test hier beide ab.
        landung_episode_zuruecksetzen(&mut stats);

        // Zweites Aufsetzen ohne Messwert: es darf nichts von der ersten
        // Landung übrig sein.
        let leer = SimSnapshot::default();
        stamp_touchdown_metadata(&mut stats, &leer, Utc::now(), None);
        assert_eq!(
            stats.landing_true_airspeed_kt, None,
            "nach dem Rücksetzer darf die alte Eigengeschwindigkeit nicht \
             wieder auftauchen"
        );
        // Der Landewind gehört zur selben Episode und muss mit weg —
        // sonst rechnet die Ausrichtungs-Achse der zweiten Landung mit
        // dem Wetter der ersten.
        assert_eq!(stats.landing_wind_direction_deg, None);
        assert_eq!(stats.landing_wind_speed_kt, None);
    }

    /// **Die Wind-Kette über den echten Pfad.** Das ist die Prüfung, die
    /// der MSFS-Entlagung zwei Monate lang gefehlt hat: sie hatte 20 grüne
    /// Tests gegen ihre eigene Rechenfunktion und lief im Feld nie, weil
    /// der Weg dorthin nicht stimmte. Hier geht der Test denselben Weg wie
    /// echte Flugdaten — Momentaufnahme aus dem Simulator, Stempel beim
    /// Aufsetzen, Übergabe an die Bewertung — statt `AlignmentInput` von
    /// Hand zu füllen.
    ///
    /// EDDP 26R (Bahnkurs 265,7°), Wind aus 195° mit 20 kt: das sind rund
    /// 19,7 kt von links. Die Nase muss also nach LINKS vorhalten, und
    /// genau dieser Vorhalt darf keine Punkte kosten.
    #[test]
    fn seitenwind_kompensation_wirkt_ueber_die_echte_kette() {
        use landing_scoring::sub_alignment::{sub_alignment, AlignmentInput};

        let mut stats = FlightStats {
            // Zwei Meter neben der Mittellinie — der Versatz ist hier nicht
            // das Thema, die Kursabweichung ist es.
            runway_match: Some(runway::RunwayMatch {
                centerline_distance_m: 2.0,
                ..eddp_26r_match_with_raw_td(1500.0)
            }),
            ..Default::default()
        };

        // Der Weg, den echte Daten nehmen: ALLES kommt aus der Momentauf-
        // nahme. Vorbelegte `stats`-Felder würde der Stempel überschreiben
        // — genau das hat dieser Test beim Schreiben schon aufgedeckt.
        let snap = SimSnapshot {
            wind_direction_deg: Some(195.0),
            wind_speed_kt: Some(20.0),
            heading_deg_true: 250.0,
            groundspeed_kt: 120.0,
            // Positiv = Rückenwind, also sind −4 kt vier Knoten Gegenwind.
            aircraft_wind_z_kt: Some(-4.0),
            ..SimSnapshot::default()
        };
        stamp_touchdown_metadata(&mut stats, &snap, Utc::now(), None);
        assert_eq!(
            stats.landing_wind_direction_deg,
            Some(195.0),
            "der Wind muss beim Aufsetzen überhaupt gestempelt werden"
        );

        let mut input = landing_scoring::LandingScoringInput::default();
        fill_v2_rollout_fields(&mut input, &stats, "EDDP");
        assert_eq!(
            input.landing_wind_direction_deg,
            Some(195.0),
            "der gestempelte Wind muss die Bewertung erreichen"
        );
        // Ohne gemessene Eigengeschwindigkeit wird sie rekonstruiert:
        // 120 kt über Grund plus 4 kt Gegenwind.
        assert_eq!(input.landing_groundspeed_kt, Some(124.0));

        // Liegt sie gemessen vor, hat sie Vorrang — beide Simulatoren
        // liefern sie nativ, die Rekonstruktion ist nur die Rückfallebene.
        let mit_tas = SimSnapshot {
            true_airspeed_kt: 131.0,
            ..snap
        };
        stamp_touchdown_metadata(&mut stats, &mit_tas, Utc::now(), None);
        let mut input2 = landing_scoring::LandingScoringInput::default();
        fill_v2_rollout_fields(&mut input2, &stats, "EDDP");
        assert_eq!(
            input2.landing_groundspeed_kt,
            Some(131.0),
            "die gemessene Eigengeschwindigkeit muss die Rekonstruktion schlagen"
        );

        let mit_wind = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(2.0),
            runway_width_m: Some(45.0),
            heading_true_deg: Some(250.0),
            runway_true_course_deg: Some(265.7),
            airport_source: Some("runway_match".into()),
            runway_geometry_trusted: Some(true),
            wind_direction_deg: input.landing_wind_direction_deg,
            wind_speed_kt: input.landing_wind_speed_kt,
            bezugsgeschwindigkeit_kt: input.landing_groundspeed_kt,
            ..Default::default()
        });
        let ohne_wind = sub_alignment(&AlignmentInput {
            centerline_offset_m: Some(2.0),
            runway_width_m: Some(45.0),
            heading_true_deg: Some(250.0),
            runway_true_course_deg: Some(265.7),
            airport_source: Some("runway_match".into()),
            runway_geometry_trusted: Some(true),
            ..Default::default()
        });
        assert!(
            mit_wind.points > ohne_wind.points,
            "der windbedingte Vorhalt muss Punkte zurückgeben: mit {:?}, ohne {:?}",
            mit_wind.points,
            ohne_wind.points
        );
        // Der Pilot muss seinen GEMESSENEN Winkel wiederfinden, nicht den
        // heruntergerechneten — sonst widerspricht die Kachel der Karte.
        assert!(
            mit_wind
                .value
                .as_deref()
                .is_some_and(|v| v.contains("15.7°")),
            "die Anzeige muss den gemessenen Winkel auf die Nachkommastelle tragen, war {:?}",
            mit_wind.value
        );
        assert!(
            mit_wind.value.as_deref().is_some_and(|v| v.contains("XW")),
            "das gewährte Zugeständnis muss sprachneutral ausgewiesen sein, war {:?}",
            mit_wind.value
        );
    }

    /// Gegenprobe: derselbe Schiefstand OHNE Wind bekommt nichts geschenkt.
    /// Ohne diesen Test wäre nicht belegt, dass die Kompensation an den
    /// Wind gebunden ist statt die Leiter generell aufzuweichen.
    #[test]
    fn ohne_wind_gibt_es_kein_zugestaendnis() {
        let mut stats = FlightStats {
            runway_match: Some(eddp_26r_match_with_raw_td(1500.0)),
            ..Default::default()
        };
        let snap = SimSnapshot {
            groundspeed_kt: 120.0,
            ..SimSnapshot::default()
        };
        stamp_touchdown_metadata(&mut stats, &snap, Utc::now(), None);

        let mut input = landing_scoring::LandingScoringInput::default();
        fill_v2_rollout_fields(&mut input, &stats, "EDDP");
        assert!(
            input.landing_wind_direction_deg.is_none(),
            "ohne Windmeldung darf nichts gestempelt werden"
        );
    }

    /// v0.19.x FIX: `touchdown_distance_from_threshold_ft` is measured from
    /// the physical runway-pavement start, not the landing threshold (see
    /// `runway_assessment::classify_displaced`'s doc comment). On a runway
    /// with a displaced threshold, `assess_touchdown` must subtract the
    /// displacement before feeding TDZ/Aim — otherwise a textbook landing
    /// right on the standard aim point gets misclassified as a long/severe
    /// landing by the full displacement distance.
    ///
    /// v1.6.8: der Aufsetzpunkt dieser Bahn liegt bei 300 m, nicht bei
    /// 400 m. Die Markierung haengt nach ICAO Annex 14 an der NUTZBAREN
    /// Laenge, und die ist hier 3600 − 1219 = 2381 m, also unter der
    /// 2400-m-Schwelle. Vorher rechnete die Einstufung mit der vollen
    /// Bahn und erwartete die Markierung 100 m weiter hinten, als sie in
    /// Wirklichkeit liegt. Die Aussage des Tests bleibt dieselbe: wer
    /// genau auf der Markierung aufsetzt, darf nicht wegen der versetzten
    /// Schwelle als „zu lang" gelten.
    #[test]
    fn assess_touchdown_corrects_tdz_and_aim_for_displaced_threshold() {
        const FT_PER_M: f64 = 3.280_839_895;
        // 4000 ft displaced (per eddp_nav_fixture_displaced) + der
        // Aufsetzpunkt dieser Bahn (300 m bei 2381 m nutzbarer Laenge),
        // ausgedrueckt als rohe Distanz ab Bahnanfang.
        let raw_ft = 4000.0 + 300.0 * FT_PER_M;
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td_and_displacement(raw_ft, 4000));
        stats.runway_nav_geometry = eddp_nav_fixture_displaced()
            .runways
            .into_iter()
            .find(|r| r.designator == "26R");

        let assessed = assess_touchdown(&stats);

        let td_m = assessed
            .td_distance_from_threshold_m
            .expect("runway matched");
        assert!(
            (td_m - 300.0).abs() < 0.5,
            "expected ~300 m from the LANDING threshold, got {td_m}"
        );
        let aim = assessed.aim.expect("aim computed");
        assert_eq!(
            aim.class,
            runway_assessment::AimClass::Perfect,
            "a textbook landing on the aim point must not be misclassified just \
             because the runway has a displaced threshold (got delta={})",
            aim.delta_m
        );
        assert!(
            (aim.aim_point_m - 300.0).abs() < 0.5,
            "Markierung folgt der nutzbaren Laenge (2381 m < 2400 m → 300 m), \
             nicht der vollen Bahn: {}",
            aim.aim_point_m
        );
        let tdz = assessed.tdz.expect("runway long enough for TDZ");
        assert!(
            tdz.in_tdz,
            "300 m past the landing threshold is inside the TDZ"
        );
        assert!(
            (tdz.tdz_length_m - 793.0).abs() < 2.0,
            "Aufsetzzone = min(900, 2381/3) = 794 m, nicht min(900, 3600/3) = 900 m: {}",
            tdz.tdz_length_m
        );
    }

    /// v1.6.8-QS4, ueberarbeitet v1.7.18-QS1: bestaetigt die Geometrie den
    /// gemeldeten Versatz (innerhalb der Toleranz), darf er nicht zweimal
    /// wirken — ein Aufsetzen 650 m hinter der Schwelle bleibt 650 m,
    /// unabhaengig davon, ob der Schwellenpunkt selbst schon versetzt ist
    /// (Feld=0, weil der Versatz in der Geometrie steckt) oder das Feld
    /// zusaetzlich denselben Wert traegt.
    ///
    /// v1.7.18-QS1 hat den ersten Entwurf dieses Tests korrigiert: der
    /// nahm an, dass `feld_ft=0` weiterhin ~650 m ergeben MUSS, weil die
    /// Geometrie den Versatz "sowieso zeigt" — das war der Kern eines
    /// echten Sicherheitsfehlers (siehe `displacement_not_in_geometry_ft`):
    /// ohne gemeldeten Versatz gibt es nichts zu bestaetigen, und ein
    /// Feld=0 zieht seither IMMER 0 ab, ganz gleich was die Geometrie
    /// zeigt. Das ist hier weiterhin richtig (0 gemeldet → 0 abgezogen →
    /// 650 m bleiben 650 m), nur aus einem anderen Grund als vorher
    /// angenommen.
    #[test]
    fn versatz_wirkt_nie_zweimal_wenn_die_geometrie_ihn_bestaetigt() {
        // Schwellenpunkt und Bahnende wie im aktiven Zyklus (echte EDDH-
        // 33/15-Koordinaten): der Versatz steckt in der Geometrie
        // (gemessen 3214,7 m gegen erwartete 3219,9 m bei 1464 ft Feld).
        const THRESHOLD_LAT: f64 = 53.628681;
        const THRESHOLD_LON: f64 = 9.997447;
        const END_LAT: f64 = 53.654411;
        const END_LON: f64 = 9.975211;
        const LENGTH_FT: f32 = 12028.0;
        let geometrie_versatz_ft = runway::geometry_implied_displacement_ft(
            THRESHOLD_LAT,
            THRESHOLD_LON,
            END_LAT,
            END_LON,
            LENGTH_FT,
            0,
        );
        assert!(
            (geometrie_versatz_ft - 1464).abs() <= 20,
            "Selbstprobe soll ~1464 ft aus der Geometrie lesen (Koordinaten-\
             Rundung erlaubt ein paar Fuss Abweichung), war {geometrie_versatz_ft}"
        );
        let bahn = |feld_ft: i32| runway::RunwayMatch {
            airport_ident: "EDDH".to_string(),
            runway_ident: "33".to_string(),
            heading_true_deg: 323.0,
            length_ft: LENGTH_FT,
            width_ft: 150.0,
            surface: "ASP".to_string(),
            threshold_lat: THRESHOLD_LAT,
            threshold_lon: THRESHOLD_LON,
            end_lat: END_LAT,
            end_lon: END_LON,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            // 650 m hinter der Lande-Schwelle aufgesetzt — bewusst weg
            // von der 200-m-Bandgrenze der Aufsetzpunkt-Einstufung.
            touchdown_distance_from_threshold_ft: 2132.5,
            side: "CENTER".to_string(),
            displaced_threshold_ft: feld_ft,
            geometry_implied_displaced_threshold_ft: geometrie_versatz_ft,
        };
        // feld=0: nichts gemeldet, nichts abzuziehen (unabhaengig von der
        // Geometrie). feld=1464: gemeldet UND von der Geometrie bestaetigt
        // (Differenz 17 ft = 5,2 m, weit innerhalb der 20-m-Toleranz) —
        // beide Faelle duerfen die Aufsetzdistanz nicht verschieben.
        for feld in [0, 1464] {
            let mut stats = FlightStats::default();
            stats.runway_match = Some(bahn(feld));
            let a = assess_touchdown(&stats);
            let td = a.td_distance_from_threshold_m.expect("Bahn gematcht");
            assert!(
                (td - 650.0).abs() < 1.0,
                "Feld={feld}: Aufsetzdistanz muss 650 m bleiben, war {td:.0} m"
            );
            assert!(
                !a.dds.expect("dds").in_pre_threshold_zone,
                "Feld={feld}: 650 m hinter der Schwelle ist kein Sperrbereich"
            );
            assert_eq!(
                a.aim.expect("aim").class,
                runway_assessment::AimClass::LongLanding,
                "Feld={feld}: 650 m gegen 400 m Markierung = 250 m zu lang"
            );
        }
        // Fuer Anzeige/nutzbare Laenge zaehlt nur der gemeldete Wert (siehe
        // `effective_displaced_threshold_ft`-Doku) — Feld=0 zeigt hier
        // absichtlich 0, nicht den geometrisch vermuteten Wert.
        assert_eq!(effective_displaced_threshold_ft(&bahn(0)), 0);
        assert_eq!(effective_displaced_threshold_ft(&bahn(1464)), 1464);
    }

    /// v1.7.18-QS1 — der Sicherheitsfehler, den die vorige Fassung dieses
    /// Tests fast uebersehen haette: Koordinatenrauschen an kleinen/
    /// schlecht vermessenen Bahnen kann `geometry_implied_displacement_ft`
    /// weit UEBER den gemeldeten Versatz treiben (nicht nur knapp daneben).
    /// Ein `(Feld − Geometrie).max(0)`-Ansatz haette daraus 0 gemacht — die
    /// Sperrzonen-Pruefung waere fuer diese Bahn KOMPLETT UND STILL
    /// abgeschaltet gewesen, obwohl der Versatz nie bestaetigt wurde.
    ///
    /// Reale Bahn aus dem QS-Befund: 25TS Piste 17 (Texas), gemeldeter
    /// Versatz 685 ft, Geometrie legt (durch Rauschen) 2927 ft nahe — mehr
    /// als das Vierfache. Die Bestaetigung darf hier NICHT zuschlagen: die
    /// Differenz (2242 ft = 683 m) liegt weit ausserhalb der 20-m-Toleranz,
    /// also muss der volle gemeldete Wert (685 ft) abgezogen werden.
    #[test]
    fn eine_geometrie_die_den_versatz_weit_uebertrifft_bestaetigt_ihn_nicht() {
        const THRESHOLD_LAT: f64 = 36.101_111_11;
        const THRESHOLD_LON: f64 = -102.417_266_67;
        const END_LAT: f64 = 36.091_316_67;
        const END_LON: f64 = -102.417_222_22;
        const LENGTH_FT: f32 = 6500.0;
        const FELD_FT: i32 = 685;
        let geometrie_versatz_ft = runway::geometry_implied_displacement_ft(
            THRESHOLD_LAT,
            THRESHOLD_LON,
            END_LAT,
            END_LON,
            LENGTH_FT,
            0,
        );
        assert!(
            geometrie_versatz_ft > FELD_FT * 2,
            "dieser Test braucht eine Bahn, deren Geometrie den Versatz \
             deutlich UEBERSCHAETZT, war {geometrie_versatz_ft} gegen {FELD_FT}"
        );
        let m = runway::RunwayMatch {
            airport_ident: "25TS".to_string(),
            runway_ident: "17".to_string(),
            heading_true_deg: 170.0,
            length_ft: LENGTH_FT,
            width_ft: 60.0,
            surface: "ASP".to_string(),
            threshold_lat: THRESHOLD_LAT,
            threshold_lon: THRESHOLD_LON,
            end_lat: END_LAT,
            end_lon: END_LON,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            // 100 m hinter dem PHYSISCHEN Bahnanfang, also klar VOR der
            // echten (versetzten) Lande-Schwelle bei 685 ft = 208,8 m.
            touchdown_distance_from_threshold_ft: 100.0 / 0.3048,
            side: "CENTER".to_string(),
            displaced_threshold_ft: FELD_FT,
            geometry_implied_displaced_threshold_ft: geometrie_versatz_ft,
        };
        let mut stats = FlightStats::default();
        stats.runway_match = Some(m);
        let a = assess_touchdown(&stats);
        assert!(
            a.dds.expect("dds").in_pre_threshold_zone,
            "100 m hinter dem physischen Bahnanfang, aber vor der bei \
             685 ft versetzten Lande-Schwelle, muss als Sperrzonen-\
             Verstoss erkannt werden — eine ueberschaetzende Geometrie \
             darf diese Pruefung nicht stillschweigend abschalten"
        );
    }

    /// v1.7.18-QS2 (externe Pruefung): `geometry_implied_displaced_
    /// threshold_ft == 0` heisst bei `geometry_implied_displacement_ft`
    /// IMMER "nichts erkannt" (ungueltige Eingaben, keine plausible
    /// Gegenschwelle, unplausibler Wert — siehe deren Doku), NIE
    /// "bestaetigt kein Versatz". Ohne den `> 0`-Riegel in
    /// `displacement_not_in_geometry_ft` haette ein KLEINER gemeldeter
    /// Versatz (hier 50 ft = 15,2 m, unter der 20-m-Toleranz) diesen
    /// Nullwert als Bestaetigung durchgehen lassen — obwohl die Geometrie
    /// dieser Bahn ueberhaupt nichts gemessen hat (synthetische, nicht
    /// plausible Koordinaten).
    #[test]
    fn ein_nicht_erkannter_wert_bestaetigt_keinen_kleinen_versatz() {
        let m = runway::RunwayMatch {
            airport_ident: "XXXX".to_string(),
            runway_ident: "09".to_string(),
            heading_true_deg: 90.0,
            length_ft: 5000.0,
            width_ft: 100.0,
            surface: "ASP".to_string(),
            threshold_lat: 0.0,
            threshold_lon: 0.0,
            // Schwelle == Gegenschwelle: `geometry_implied_displacement_ft`
            // liefert hier immer 0 (siehe deren eigenen Test
            // `ein_geometrisch_unplausibler_versatz_ueber_halber_bahn_
            // wird_verworfen` in `runway.rs`) — ein Stand-in fuer JEDEN
            // Fall, in dem die Geometrie nichts Verwertbares liefert.
            end_lat: 0.0,
            end_lon: 0.0,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            // 10 m hinter dem physischen Bahnanfang — klar VOR der bei
            // 50 ft (15,2 m) versetzten Lande-Schwelle, also im
            // Sperrbereich.
            touchdown_distance_from_threshold_ft: 10.0 / 0.3048,
            side: "CENTER".to_string(),
            displaced_threshold_ft: 50,
            geometry_implied_displaced_threshold_ft: 0,
        };
        let mut stats = FlightStats::default();
        stats.runway_match = Some(m);
        let a = assess_touchdown(&stats);
        let td = a.td_distance_from_threshold_m.expect("Bahn gematcht");
        assert!(
            (td - (10.0 - 50.0 * 0.3048)).abs() < 0.1,
            "ohne Bestaetigung durch die Geometrie muss der volle \
             gemeldete Versatz (50 ft = 15,2 m) abgezogen werden, \
             also 10 m − 15,2 m = −5,2 m, war {td:.1} m"
        );
        assert!(
            a.dds.expect("dds").in_pre_threshold_zone,
            "10 m hinter dem physischen Bahnanfang, aber vor der \
             (unbestaetigt physischen) Lande-Schwelle bei 50 ft, ist \
             ein Sperrzonen-Verstoss — ein nicht-erkannter \
             Geometriewert darf einen kleinen Versatz nicht als \
             \"bestaetigt schon versetzt\" durchgehen lassen"
        );
    }

    /// v1.6.8-QS2: die 1200-m-Grenze fuer Aufsetzzonen-Markierungen gilt
    /// fuer die LANDEBAHN, nicht fuer die Bahnflaeche. Eine 1300-m-Bahn
    /// mit 150 m versetzter Schwelle hat 1150 m Landebahn — dort ist
    /// keine Aufsetzzone gemalt, also darf der Client auch keine melden.
    ///
    /// Im Bestand betrifft das drei Landungen (TFFG 30, BIRK 31, EDWE 07):
    /// die Angabe „in der Aufsetzzone" entfaellt dort kuenftig, statt eine
    /// Markierung zu behaupten, die es nicht gibt.
    #[test]
    fn aufsetzzone_gilt_fuer_die_landebahn_nicht_die_bahnflaeche() {
        let bahn = |displaced_ft: i32| runway::RunwayMatch {
            airport_ident: "XXXX".to_string(),
            runway_ident: "07".to_string(),
            heading_true_deg: 70.0,
            length_ft: 4265.0, // 1300 m
            width_ft: 100.0,
            surface: "ASP".to_string(),
            threshold_lat: 50.0,
            threshold_lon: 8.0,
            end_lat: 50.0,
            end_lon: 8.018,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 1300.0,
            side: "CENTER".to_string(),
            displaced_threshold_ft: displaced_ft,
            // Synthetische Koordinaten (keine echte Gegenschwelle) — die
            // Geometrie zeigt hier nichts, dieser Test prueft ausdruecklich
            // den feldbasierten Pfad.
            geometry_implied_displaced_threshold_ft: 0,
        };
        // Ohne Versatz: 1300 m Landebahn → Aufsetzzone existiert.
        let mut stats = FlightStats::default();
        stats.runway_match = Some(bahn(0));
        assert!(
            assess_touchdown(&stats).tdz.is_some(),
            "1300 m Landebahn traegt eine Aufsetzzone"
        );
        // Mit 492 ft (150 m) Versatz: 1150 m Landebahn → keine mehr.
        let mut stats = FlightStats::default();
        stats.runway_match = Some(bahn(492));
        assert!(
            assess_touchdown(&stats).tdz.is_none(),
            "1150 m Landebahn traegt keine Aufsetzzone — auch wenn die \
             Bahnflaeche 1300 m misst"
        );
    }

    /// v0.19.x FIX: a touchdown that lands ON the displaced-threshold
    /// paint (before the legal landing threshold, but past the physical
    /// runway start) must be flagged by `classify_displaced` — the bug
    /// this replaces could NEVER detect a real violation, only fire on
    /// undershoots that never touched pavement at all.
    #[test]
    fn assess_touchdown_detects_real_dds_violation_before_true_threshold() {
        const FT_PER_M: f64 = 3.280_839_895;
        // 600 m from the physical pavement start — well inside the 1219.5 m
        // (4000 ft) displaced zone, i.e. before the legal landing threshold.
        let raw_ft = 600.0 * FT_PER_M;
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td_and_displacement(raw_ft, 4000));
        stats.runway_nav_geometry = eddp_nav_fixture_displaced()
            .runways
            .into_iter()
            .find(|r| r.designator == "26R");

        let assessed = assess_touchdown(&stats);

        let dds = assessed.dds.expect("nav geometry present");
        assert!(
            dds.in_pre_threshold_zone,
            "touchdown 600 m from pavement start, 1219.5 m displaced threshold \
             → must be flagged as landing before the legal threshold"
        );
    }

    /// v0.19.x FIX — the actual end-to-end bug: a PURE OurAirports-fallback
    /// match (no `runway_nav_geometry` at all — the real-world case for any
    /// divert or unlisted-in-Navigraph field) used to make DDS
    /// classification silently unavailable (`assessed.dds == None`)
    /// EVEN THOUGH `RunwayMatch::displaced_threshold_ft` now carries the
    /// exact same CSV-sourced displacement data. This proves DDS violation
    /// detection works from the CSV alone, with zero Navigraph data.
    #[test]
    fn assess_touchdown_detects_dds_violation_from_ourairports_csv_alone() {
        const FT_PER_M: f64 = 3.280_839_895;
        // 600 m from the physical pavement start, well inside a 1219.5 m
        // (4000 ft) displaced zone — same scenario as the Navigraph test
        // above, but with NO nav geometry at all.
        let raw_ft = 600.0 * FT_PER_M;
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td_and_displacement(raw_ft, 4000));
        stats.runway_source = Some(runway::RunwaySource::OurAirportsFallback);
        assert!(
            stats.runway_nav_geometry.is_none(),
            "this test's whole point is zero nav geometry"
        );

        let assessed = assess_touchdown(&stats);

        let dds = assessed.dds.expect(
            "DDS must be classifiable from RunwayMatch alone, no Navigraph geometry needed",
        );
        assert!(
            dds.in_pre_threshold_zone,
            "OurAirports-only match with a real displaced threshold must still detect the violation"
        );
    }

    /// Companion to the above: a PURE OurAirports match on an undisplaced
    /// runway must report a clean `Some(false)`, not `None` — the data
    /// source alone is no longer a reason to withhold the classification.
    #[test]
    fn assess_touchdown_reports_clean_dds_from_ourairports_csv_alone() {
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(980.0));
        stats.runway_source = Some(runway::RunwaySource::OurAirportsFallback);
        assert!(stats.runway_nav_geometry.is_none());

        let assessed = assess_touchdown(&stats);

        let dds = assessed
            .dds
            .expect("DDS classifiable without Navigraph geometry");
        assert!(!dds.in_pre_threshold_zone);
    }

    /// v0.20.x QS fix: the persisted/live wire value for displaced-threshold
    /// must come from the runway match itself (works for OurAirports too),
    /// not from `runway_nav_geometry` (Navigraph-only) — otherwise the score
    /// (which already reads the match) and the displayed diagram/LDA (which
    /// used to read nav geometry) silently disagree for the same landing.
    #[test]
    fn wire_displaced_threshold_ft_reads_from_runway_match_not_nav_geometry() {
        let m = eddp_26r_match_with_raw_td_and_displacement(600.0, 4000);
        assert_eq!(
            wire_displaced_threshold_ft(Some(&m)),
            Some(4000),
            "must report the real displaced threshold even with zero Navigraph geometry \
             (this fixture has none) — an OurAirports-only match"
        );
    }

    /// v1.7.18-QS1: ein Feld=0 meldet KEINEN Versatz im Wire-Wert, auch
    /// wenn die Geometrie (durch Koordinatenrauschen) einen nahelegt.
    ///
    /// Ersetzt den frueheren Test `..._findet_den_versatz_auch_in_der_
    /// geometrie`, der das genaue Gegenteil verlangte (Feld=0 → Wire-Wert
    /// = geometrisch vermuteter Versatz). Eine QS-Runde hat nachgerechnet:
    /// an 33.390 Bahnen OHNE gemeldeten Versatz erfindet
    /// `geometry_implied_displacement_ft` bei 3,6 % einen Versatz, den es
    /// nicht gibt (Rauschen bis ueber 500 m an OYSH/LETO, 20-30 m schon an
    /// echten Flughaefen wie OLBA/LROP). Die alte Erwartung haette diesen
    /// Rateversatz ungeprueft in die Kachel ("1818 m von X m") und die
    /// nutzbare Laenge uebernommen — echte Bahnen ohne jeden Versatz
    /// haetten plötzlich eine verkuerzte Landebahn gezeigt.
    #[test]
    fn wire_displaced_threshold_ft_ignoriert_geometrie_ohne_gemeldeten_versatz() {
        // Dieselbe EDDH-33-Geometrie wie oben — die Geometrie legt hier
        // durchaus ~1464 ft nahe, aber das Feld schweigt (0).
        const THRESHOLD_LAT: f64 = 53.628681;
        const THRESHOLD_LON: f64 = 9.997447;
        const END_LAT: f64 = 53.654411;
        const END_LON: f64 = 9.975211;
        const LENGTH_FT: f32 = 12028.0;
        let geometrie_versatz_ft = runway::geometry_implied_displacement_ft(
            THRESHOLD_LAT,
            THRESHOLD_LON,
            END_LAT,
            END_LON,
            LENGTH_FT,
            0,
        );
        assert!(
            geometrie_versatz_ft > 0,
            "dieser Test braucht eine Bahn, deren Geometrie einen Versatz \
             nahelegt, sonst prueft er nichts"
        );
        let m = runway::RunwayMatch {
            airport_ident: "EDDH".to_string(),
            runway_ident: "33".to_string(),
            heading_true_deg: 323.0,
            length_ft: LENGTH_FT,
            width_ft: 150.0,
            surface: "ASP".to_string(),
            threshold_lat: THRESHOLD_LAT,
            threshold_lon: THRESHOLD_LON,
            end_lat: END_LAT,
            end_lon: END_LON,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 1000.0,
            side: "CENTER".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: geometrie_versatz_ft,
        };
        assert_eq!(
            wire_displaced_threshold_ft(Some(&m)),
            Some(0),
            "Feld=0 muss 0 im Wire-Wert bleiben, auch wenn die Geometrie \
             (moeglicherweise nur durch Rauschen) einen Versatz nahelegt"
        );
        // Und die Aufsetzdistanz bleibt unangetastet: `assess_touchdown`
        // zieht bei Feld=0 nie etwas ab.
        assert_eq!(m.displaced_threshold_ft, 0);
    }

    #[test]
    fn wire_displaced_threshold_ft_is_none_without_a_runway_match_at_all() {
        assert_eq!(wire_displaced_threshold_ft(None), None);
    }

    /// Regression: for the overwhelming majority of runways (no displaced
    /// threshold), the correction must be a complete no-op — the corrected
    /// distance equals the raw one, byte for byte.
    #[test]
    fn assess_touchdown_unaffected_when_no_displaced_threshold() {
        let mut stats = FlightStats::default();
        stats.runway_match = Some(eddp_26r_match_with_raw_td(980.0));
        stats.runway_nav_geometry = eddp_nav_fixture()
            .runways
            .into_iter()
            .find(|r| r.designator == "26R");

        let assessed = assess_touchdown(&stats);

        let td_m = assessed
            .td_distance_from_threshold_m
            .expect("runway matched");
        assert!((td_m - 980.0 / 3.280_839_895).abs() < 1e-9);
        let dds = assessed.dds.expect("nav geometry present");
        assert!(
            !dds.in_pre_threshold_zone,
            "no displaced threshold → never flagged"
        );
    }

    /// Populate a divert touchdown (planned EDDF, wheels at EDDP/26R) with
    /// enough touchdown forensics that `compute_aggregate_master_score`
    /// returns Some AND the runway-utilisation sub-score actually computes
    /// (not skipped): a trusted-geometry divert hint to EDDP, a rollout/float
    /// distance, vs + g for the dominant sub-scores. The (td_dist, rollout)
    /// pair is chosen so the rollout band differs between the OurAirports LDA
    /// and the displaced Navigraph LDA.
    fn divert_score_stats() -> (FlightStats, SimSnapshot) {
        let (mut stats, snap) = eddp_26r_touchdown_stats();
        // Dominant sub-scores (weight 3 each) — stable across both sources.
        stats.landing_rate_fpm = Some(-150.0);
        stats.landing_peak_g_force = Some(1.25);
        stats.bounce_count = 0;
        // Gates the "Landing Score" custom field on (its presence is required
        // for `landing_score_field` to surface a value to compare).
        stats.landing_score = Some(LandingScore::Acceptable);
        // Runway-utilisation inputs. td_dist ~300 m past threshold, rollout
        // 1400 m. Against OurAirports LDA 3600 m → ratio ~39 % (excellent_margin,
        // v0.20.x bands). Against the displaced Navigraph LDA 2381 m → ratio
        // ~59 % (good_stop). The test only asserts the two scores DIFFER
        // (assert_ne!), not the literal rationale, so this comment is
        // documentation only — no assertion depends on these band names.
        stats.rollout_distance_m = Some(1400.0);
        stats.landing_float_distance_m = Some(0.0);
        // Trust the geometry on this divert: the matched ICAO (EDDP) must
        // equal the divert hint's actual_icao, else the sub-score skips with
        // `untrusted_geometry` and the source can't influence the score.
        stats.divert_hint = DivertHint::from_site(
            &crate::arrival::ArrivalSite::AtOtherAirport {
                icao: "EDDP".to_string(),
                distance_from_planned_nm: 75.0,
            },
            "EDDF",
            None,
        );
        (stats, snap)
    }

    /// FIX 1: on a divert where the actual field's navdata becomes available
    /// by filing time, the PIREP score path (`compute_aggregate_master_score`
    /// + the "Landing Score" custom field) must reflect the UPGRADED Navigraph
    /// LDA — exactly the value the local LandingRecord uses — not the
    /// provisional OurAirports geometry the touchdown fell back to.
    #[test]
    fn divert_pirep_score_gets_finalize_upgrade() {
        let flight = flight_fixture("EDDF"); // planned EDDF, nothing cached yet
        let (mut stats, snap) = divert_score_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        // v0.16.24 (QS-2 FIX A): the REAL call sites (`flight_end` /
        // `flight_end_manual`) pass the EFFECTIVE arrival (`divert_to`=EDDP),
        // not the planned `arr_airport`. Model that here so the trust check
        // passes via the effective arrival exactly as in production.
        let eff = "EDDP";

        // Provisional: fell back to OurAirports (EDDP navdata not cached).
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        // The PIREP score the OLD ordering would have filed (provisional).
        let provisional_score =
            compute_aggregate_master_score(&stats, Some("A320"), eff, "EDDF", None);
        let provisional_field = landing_score_field(&flight, &stats, eff);

        // The on-demand fetch completes: EDDP navdata (with a displaced
        // threshold) is now cached.
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        // What `flight_end` now does BEFORE building the PIREP body.
        apply_finalized_runway_correlation(&flight, &mut stats);
        // Upgraded to Navigraph with the displaced-threshold geometry.
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
        assert_eq!(
            stats
                .runway_nav_geometry
                .as_ref()
                .map(|g| g.displaced_threshold_ft),
            Some(4000)
        );

        // The PIREP score now reflects the Navigraph LDA — strictly different
        // from the provisional OurAirports score (the displaced threshold
        // shortened the LDA → the rollout sub-score dropped a band).
        let upgraded_score =
            compute_aggregate_master_score(&stats, Some("A320"), eff, "EDDF", None);
        let upgraded_field = landing_score_field(&flight, &stats, eff);
        assert_ne!(
            upgraded_score, provisional_score,
            "the divert PIREP score must change when the Navigraph LDA upgrade lands \
             (provisional={provisional_score:?}, upgraded={upgraded_score:?})"
        );
        assert_ne!(upgraded_field, provisional_field);

        // PIREP score == local-record score: the LandingRecord is built from
        // the SAME finalized stats, and `record_landing_for_filed_flight` runs
        // the SAME finalize again (idempotent no-op once Navigraph). Model the
        // second finalize and assert the score is unchanged → both surfaces
        // agree.
        let record_score_before = upgraded_score;
        apply_finalized_runway_correlation(&flight, &mut stats); // the no-op
        let record_score_after =
            compute_aggregate_master_score(&stats, Some("A320"), eff, "EDDF", None);
        assert_eq!(
            record_score_before, record_score_after,
            "the second finalize (inside record_landing_for_filed_flight) must be a no-op \
             so the PIREP score equals the local-record score"
        );
        // And the Navigraph geometry is still in place after the no-op.
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
    }

    /// v0.16.24 (QS-2 FIX A) — the regression guard the ORIGINAL bug slipped
    /// past. The sibling above hand-sets `divert_hint.actual_icao = EDDP`, so
    /// its trust check passed via the divert hint even when the score was fed
    /// the PLANNED `arr_airport` — exactly the masking that hid the bug. This
    /// test does NOT set `divert_hint`. On a real divert where the aircraft
    /// parks > ARRIVED_FALLBACK_RADIUS_NM (2 nm) from the threshold, the
    /// passive `divert_hint` resolver yields None, so the ONLY thing that can
    /// make the geometry trusted is passing the EFFECTIVE arrival (the
    /// pilot-confirmed divert, EDDP) — which `build_pirep_payload` /
    /// `build_landing_record` already do and which FIX A now makes the native
    /// PIREP score + "Landing Score" custom field do too.
    #[test]
    fn divert_pirep_score_trusts_geometry_via_effective_arr_without_divert_hint() {
        let flight = flight_fixture("EDDF"); // planned EDDF
                                             // Warm the EDDP navdata (with a displaced threshold) so the runway
                                             // correlation lands on Navigraph — the rollout/LDA sub-score can only
                                             // differ from the no-geometry case when the geometry is TRUSTED.
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        let (mut stats, snap) = divert_score_stats();
        // The crux: NO divert hint. The passive resolver can't help here.
        stats.divert_hint = None;
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        // The touchdown correlated to EDDP/26R via Navigraph (warm cache).
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
        // Sanity: the actual-landing-airport resolver agrees it's a divert.
        assert_eq!(stats.runway_correlation_icao.as_deref(), Some("EDDP"));

        // OLD (buggy) behaviour: feeding the PLANNED arr with no divert hint →
        // the trust check sees matched=EDDP != arr=EDDF and no divert match →
        // icao_mismatch → the rollout/LDA sub-score is SKIPPED.
        let buggy_planned_arr_score =
            compute_aggregate_master_score(&stats, Some("A320"), "EDDF", "EDDF", None);
        // FIXED behaviour: feeding the EFFECTIVE arrival (the divert EDDP) →
        // matched=EDDP == effective=EDDP → geometry trusted → the rollout/LDA
        // sub-score COMPUTES. This is what the native score now passes.
        let fixed_effective_arr_score =
            compute_aggregate_master_score(&stats, Some("A320"), "EDDP", "EDDF", None);

        // The fix must change the scored value on this divert (the rollout
        // sub-score went from skipped → computed against the Navigraph LDA).
        assert_ne!(
            buggy_planned_arr_score, fixed_effective_arr_score,
            "feeding the effective arrival must let the trusted-geometry rollout \
             sub-score compute (buggy/planned={buggy_planned_arr_score:?}, \
             fixed/effective={fixed_effective_arr_score:?})"
        );

        // native PIREP score == "Landing Score" custom field == local record:
        // all three derive from the SAME finalized stats via the SAME effective
        // arrival, so they must agree. (The custom field is the numeric in the
        // "G (label) — N/100" string.)
        let native_score = fixed_effective_arr_score.expect("aggregate computes");
        let field =
            landing_score_field(&flight, &stats, "EDDP").expect("Landing Score field present");
        assert!(
            field.contains(&format!("{native_score}/100")),
            "the Landing Score custom field ({field}) must carry the same numeric \
             as the native PIREP score ({native_score})"
        );
        // The local record path runs the SAME finalize (idempotent no-op once
        // Navigraph) and the SAME effective arrival → identical score.
        apply_finalized_runway_correlation(&flight, &mut stats);
        let record_score =
            compute_aggregate_master_score(&stats, Some("A320"), "EDDP", "EDDF", None);
        assert_eq!(
            Some(native_score),
            record_score,
            "native PIREP score must equal the local-record score on a divert"
        );
    }

    /// v0.16.24 (QS-2 FIX B): the five runway-correlation fields survive a
    /// snapshot→apply (incl. JSON) round-trip, so a divert that suspends/resumes
    /// between touchdown and PIREP filing keeps its source/geometry/correlation
    /// state.
    #[test]
    fn persisted_flight_stats_preserves_runway_correlation_round_trip() {
        let mut stats = FlightStats::default();
        stats.runway_match = Some(runway::RunwayMatch {
            airport_ident: "EDDP".to_string(),
            runway_ident: "26R".to_string(),
            heading_true_deg: 265.7,
            length_ft: 11_811.0,
            width_ft: 148.0,
            surface: "ASP".to_string(),
            threshold_lat: EDDP_26R_THR_LAT,
            threshold_lon: EDDP_26R_THR_LON,
            end_lat: 51.431_198,
            end_lon: 12.215_800,
            centerline_distance_m: 7.5,
            centerline_distance_abs_ft: 24.6,
            touchdown_distance_from_threshold_ft: 980.0,
            side: "LEFT".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        });
        stats.runway_source = Some(runway::RunwaySource::OurAirportsFallback);
        stats.runway_nav_cycle = Some("2604".to_string());
        stats.runway_nav_geometry = eddp_nav_fixture_displaced().runways.first().cloned();
        stats.runway_correlation_icao = Some("EDDP".to_string());
        stats.runway_fallback_at_touchdown = true;

        // Score-Version 25: der ILS-Bezug ueberlebt den Neustart ebenso.
        stats.runway_ils_navigraph = IlsNavigraph::aus(&eddp_nav_fixture_displaced(), "26R");
        assert!(stats.runway_ils_navigraph.is_some(), "Vorrichtung");
        let persisted = PersistedFlightStats::snapshot_from(&stats);
        // Round-trip through JSON too, to prove (de)serialisation preserves them.
        let json = serde_json::to_string(&persisted).expect("PersistedFlightStats must serialise");
        let reparsed: PersistedFlightStats = serde_json::from_str(&json).expect("re-deserialise");

        let mut restored = FlightStats::default();
        reparsed.apply_to(&mut restored);

        assert_eq!(
            restored.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        );
        assert_eq!(restored.runway_nav_cycle.as_deref(), Some("2604"));
        assert_eq!(restored.runway_nav_geometry, stats.runway_nav_geometry);
        assert_eq!(
            restored.runway_ils_navigraph.as_ref().map(|i| (i.bahn.clone(), i.versatz_ft)),
            stats.runway_ils_navigraph.as_ref().map(|i| (i.bahn.clone(), i.versatz_ft)),
            "der ILS-Bezug geht beim Fortsetzen verloren"
        );
        assert_eq!(restored.runway_correlation_icao.as_deref(), Some("EDDP"));
        assert!(restored.runway_fallback_at_touchdown);
        assert_eq!(
            restored
                .runway_match
                .as_ref()
                .map(|m| m.airport_ident.as_str()),
            Some("EDDP")
        );
    }

    /// v0.16.24 (QS-2 FIX B): a DIVERT that fell back to OurAirports at
    /// touchdown, then SUSPENDED and RESUMED, still upgrades to Navigraph at
    /// finalize once the cache is warm. The resume rebuilds stats via
    /// `FlightStats::new()` + `apply_to`; without persisting the five fields the
    /// restored `runway_source` would be None → `finalize_runway_correlation`'s
    /// "not OurAirportsFallback → provisional verbatim" guard would WRONGLY fire
    /// and the divert would file with coarse OurAirports geometry.
    #[test]
    fn resumed_divert_still_upgrades_at_finalize_when_cache_warm() {
        // ── Pre-suspend: divert touchdown that fell back to OurAirports. ──
        let flight = flight_fixture("EDDF"); // planned EDDF, nothing cached
        let (mut pre_stats, snap) = divert_score_stats();
        let td_buf = touchdown_buffer_sample(&pre_stats);
        stamp_touchdown_metadata(&mut pre_stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut pre_stats, &snap, &flight, td_buf.as_ref());
        assert!(matches!(
            pre_stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        assert!(pre_stats.runway_fallback_at_touchdown);
        assert_eq!(pre_stats.runway_correlation_icao.as_deref(), Some("EDDP"));

        // ── Suspend → resume: persist, then rebuild stats from scratch. ──
        let persisted = PersistedFlightStats::snapshot_from(&pre_stats);
        let json = serde_json::to_string(&persisted).expect("serialise");
        let reparsed: PersistedFlightStats = serde_json::from_str(&json).expect("deserialise");
        let mut resumed_stats = FlightStats::new(); // fresh, as a real resume does
        reparsed.apply_to(&mut resumed_stats);
        // The fallback state survived the restart (this is what FIX B adds).
        assert!(matches!(
            resumed_stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));

        // ── The on-demand fetch landed while suspended: EDDP now cached. ──
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());

        // ── Finalize at filing: the resumed divert upgrades to Navigraph. ──
        apply_finalized_runway_correlation(&flight, &mut resumed_stats);
        assert!(
            matches!(
                resumed_stats.runway_source,
                Some(runway::RunwaySource::Navigraph)
            ),
            "a resumed divert must still upgrade to Navigraph at finalize when the cache is warm"
        );
        assert_eq!(
            resumed_stats
                .runway_nav_geometry
                .as_ref()
                .map(|g| g.displaced_threshold_ft),
            Some(4000)
        );
    }

    /// FIX 1 graceful: on a divert where the fetch NEVER lands by filing time,
    /// the PIREP score keeps the provisional OurAirports geometry — never
    /// worse than pre-v0.16.24, byte-identical to today.
    #[test]
    fn divert_pirep_score_graceful_when_fetch_incomplete() {
        let flight = flight_fixture("EDDF"); // nothing ever cached
        let (mut stats, snap) = divert_score_stats();
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());

        // v0.16.24 (QS-2 FIX A): effective arrival == EDDP (the divert field).
        let before = compute_aggregate_master_score(&stats, Some("A320"), "EDDP", "EDDF", None);
        // Empty cache → finalize keeps the provisional fallback.
        apply_finalized_runway_correlation(&flight, &mut stats);
        let after = compute_aggregate_master_score(&stats, Some("A320"), "EDDP", "EDDF", None);
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::OurAirportsFallback)
        ));
        assert_eq!(
            before, after,
            "graceful: score unchanged when the fetch never lands"
        );
    }

    /// FIX 1 on-plan: applying the finalize write-back before the PIREP body
    /// is a no-op for a non-divert (the touchdown already matched Navigraph),
    /// so the on-plan PIREP score is byte-identical to pre-v0.16.24.
    #[test]
    fn on_plan_pirep_score_unchanged_by_finalize() {
        let flight = flight_fixture("EDDP"); // on-plan, navdata cached
        flight
            .navdata
            .lock()
            .unwrap()
            .airports
            .insert("EDDP".to_string(), eddp_nav_fixture_displaced());
        flight.navdata.lock().unwrap().cycle = Some("2604".to_string());
        let (mut stats, snap) = divert_score_stats();
        // On-plan: the matched field IS the plan, so trust without a divert
        // hint (matched EDDP == arr EDDP).
        stats.divert_hint = None;
        let td_buf = touchdown_buffer_sample(&stats);
        stamp_touchdown_metadata(&mut stats, &snap, td_at(), td_buf.as_ref());
        correlate_touchdown_runway(&mut stats, &snap, &flight, td_buf.as_ref());
        // Touchdown already Navigraph (planned airport fetched at flight start).
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));

        // On-plan: effective arrival == planned arrival == EDDP.
        let before = compute_aggregate_master_score(&stats, Some("A320"), "EDDP", "EDDF", None);
        let before_field = landing_score_field(&flight, &stats, "EDDP");
        apply_finalized_runway_correlation(&flight, &mut stats); // must be a no-op
        let after = compute_aggregate_master_score(&stats, Some("A320"), "EDDP", "EDDF", None);
        let after_field = landing_score_field(&flight, &stats, "EDDP");
        assert_eq!(
            before, after,
            "on-plan PIREP score must be unchanged by finalize"
        );
        assert_eq!(before_field, after_field);
        assert!(matches!(
            stats.runway_source,
            Some(runway::RunwaySource::Navigraph)
        ));
    }

    /// Helper: the exact "Landing Score" custom-field string `build_pirep_fields`
    /// would emit, so the score-path tests assert the field the PIREP carries.
    /// `effective_arr` mirrors the real call sites (`flight_end` /
    /// `flight_end_manual` pass `divert_to.unwrap_or(arr)`).
    fn landing_score_field(
        flight: &ActiveFlight,
        stats: &FlightStats,
        effective_arr: &str,
    ) -> Option<String> {
        build_pirep_fields(flight, stats, effective_arr)
            .get("Landing Score")
            .cloned()
    }

    /// v1.7.17 Runde 12 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): ohne `dep_gate` muss die finale Einreichung "Departure
    /// Gate" trotzdem ALS SCHLUESSEL fuehren (leerer String), nicht
    /// weglassen — sonst ueberlebt ein fruehes, spaeter als veraltet
    /// erkanntes Live-Post im Upsert unkorrigiert bis in den fertigen
    /// PIREP. Die Einreichung laeuft ueber den bereits dauerhaften
    /// `file_pirep_with_retry`/`pirep_queue`-Weg — hier, nicht in der
    /// Live-Rueckzugs-Meldung, muss die Korrektur garantiert ankommen.
    #[test]
    fn departure_gate_feld_wird_bei_fehlendem_stand_explizit_geleert() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.dep_gate = None;
        stats.dep_gate_withdrawal_pending = true;
        let fields = build_pirep_fields(&flight, &stats, "EDDP");
        assert_eq!(
            fields.get("Departure Gate").map(String::as_str),
            Some(""),
            "bei anhaengigem Rueckzug muss das Feld als leerer String vorhanden sein, nicht fehlen"
        );
    }

    /// Beta 2: Paketstand als eigenes PIREP-Feld; `source_name`/„Source" bleibt
    /// reine Client-Version (IntegrityGate, Logbuecher und SkyOps lesen sie).
    #[test]
    fn paketstand_steht_als_eigenes_pirep_feld() {
        assert_eq!(paket_feldtext(55, "55-867b805a584d"), "G55 (55-867b805a584d)");
        assert_eq!(paket_feldtext(0, "0-8792671037a3"), "G0 eingebaut (0-8792671037a3)");
        let flight = flight_fixture("EDDP");
        let f = build_pirep_fields(&flight, &FlightStats::default(), "EDDP");
        let pin = laufzeit_paket::flug_pin(&flight.pirep_id).unwrap();
        assert_eq!(f.get("AeroACARS Paket"), Some(&paket_feldtext(pin.generation, &pin.to_string())));
        assert_eq!(
            f.get("Source").map(String::as_str),
            Some(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        );
    }

    /// P1/8b: leichter Hopser als PIREP-Feld (GSG-Seite), nicht neben einem
    /// normalen Hopser; ohne beides kein Feld.
    #[test]
    fn leichter_hopser_steht_als_pirep_feld() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        assert!(build_pirep_fields(&flight, &stats, "EDDP").get("Light Bounces").is_none());
        stats.light_bounce_count = 1;
        let f = build_pirep_fields(&flight, &stats, "EDDP");
        assert_eq!(f.get("Light Bounces").map(String::as_str), Some("1"));
        assert!(f.get("Bounces").is_none());
        stats.bounce_count = 1;
        let f = build_pirep_fields(&flight, &stats, "EDDP");
        assert!(f.get("Light Bounces").is_none(), "neben einem normalen Hopser zaehlt der leichte nicht");
        assert_eq!(f.get("Bounces").map(String::as_str), Some("1"));
    }

    /// CES 442 (VHHH 07L, 27.10.2026): 568 ft versetzte Schwelle. Die
    /// Bewertung rechnete 432 m ab der Lande-Schwelle, das PIREP-Feld
    /// zeigte 606 m ab dem physischen Bahnanfang. Feld und Notiz muessen
    /// denselben Wert tragen wie die Bewertung (`bahnmasse().td_m`).
    #[test]
    fn aufsetzpunkt_im_pirep_ab_lande_schwelle_bei_versetzter_schwelle() {
        let flight = flight_fixture("EDDP");
        // (Rohwert ab Bahnanfang in m, gemeldeter Versatz ft, Versatz in
        // der Geometrie ft, erwartetes Feld, erwartete Notiz)
        for (roh_m, versatz_ft, geometrie_ft, feld, notiz) in [
            // CES 442: Versatz nicht in der Geometrie → abziehen.
            (605.5, 568, 0, "432 m (LDA 3427 m of 3600 m)", "Past thresh   432 m\n"),
            // Versatz steckt schon im Schwellenpunkt → nichts abziehen.
            (432.4, 568, 568, "432 m (LDA 3427 m of 3600 m)", "Past thresh   432 m\n"),
            // Im Sperrbereich vor der Lande-Schwelle aufgesetzt.
            (10.0, 50, 0, "5 m before threshold (LDA 3585 m of 3600 m)", "Past thresh   5 m before threshold\n"),
            // Knapp vor der Schwelle (-0,3 m) rundet auf 0, nicht auf "-0".
            (14.9, 50, 0, "0 m (LDA 3585 m of 3600 m)", "Past thresh   0 m\n"),
            // Ohne Versatz unveraendert.
            (388.0, 0, 0, "388 m (runway 3600 m long)", "Past thresh   388 m\n"),
        ] {
            let mut rm = eddp_26r_match_with_raw_td_and_displacement(roh_m * 3.280_839_895, versatz_ft);
            rm.geometry_implied_displaced_threshold_ft = geometrie_ft;
            let mut stats = FlightStats::default();
            stats.runway_match = Some(rm);
            // Dieselbe Zahl wie die Bewertung, nicht nur ein fester Text.
            let bewertung = assess_touchdown(&stats).td_distance_from_threshold_m.expect("Bahn gematcht");
            assert!(feld.starts_with(&format!("{:.0} m", bewertung.abs())), "{feld} vs Bewertung {bewertung}");
            let f = build_pirep_fields(&flight, &stats, "EDDP");
            assert_eq!(f.get("Touchdown Past Threshold").map(String::as_str), Some(feld));
            let notes = build_pirep_notes(&flight, &stats, "EDDP");
            assert!(notes.contains(notiz), "Notiz: {notes}");
        }
    }

    #[test]
    fn departure_gate_feld_traegt_den_echten_stand_wenn_vorhanden() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.dep_gate = Some("A12".to_string());
        let fields = build_pirep_fields(&flight, &stats, "EDDP");
        assert_eq!(
            fields.get("Departure Gate").map(String::as_str),
            Some("A12")
        );
    }

    /// v1.7.17 Runde 13 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): der eigentliche Befund — der weit ueberwiegende
    /// Normalfall (kein Stand je erkannt, KEIN anhaengiger Rueckzug) darf
    /// das Feld NICHT leeren, sonst wuerde ein moeglicherweise von
    /// ANDERSWO gesetzter Wert zerstoert, ohne dass AeroACARS je etwas
    /// Falsches gepostet hat.
    #[test]
    fn departure_gate_feld_bleibt_ohne_anhaengigen_rueckzug_unangetastet() {
        let flight = flight_fixture("EDDP");
        let mut stats = FlightStats::default();
        stats.dep_gate = None;
        stats.dep_gate_withdrawal_pending = false;
        let fields = build_pirep_fields(&flight, &stats, "EDDP");
        assert_eq!(
            fields.get("Departure Gate"),
            None,
            "ohne anhaengigen Rueckzug darf ein fremder Wert nicht ueberschrieben werden"
        );
    }

    // ---- v0.16.24: approach-phase divert pre-fetch decision ----

    /// A committed, stable, descending approach to a field OTHER than the
    /// planned one fires the pre-fetch exactly once (idempotent).
    #[test]
    fn divert_prefetch_fires_once_on_stable_divergent_approach() {
        let mut stats = FlightStats::default();
        // Position right at EDDP/26R threshold, descending, low, airborne.
        let mut snap = rollout_snap();
        snap.lat = EDDP_26R_THR_LAT;
        snap.lon = EDDP_26R_THR_LON;
        snap.on_ground = false;
        snap.altitude_agl_ft = 1500.0;
        snap.vertical_speed_fpm = -700.0;
        // Planned EDDF (far from EDDP → genuine divergence).
        let t0 = td_at();
        // First tick: candidate registered, not yet stable.
        assert_eq!(
            divert_prefetch_decision(&mut stats, &snap, "EDDF", t0),
            None
        );
        assert_eq!(stats.divert_prefetch_candidate.as_deref(), Some("EDDP"));
        // Still within the stability window.
        assert_eq!(
            divert_prefetch_decision(
                &mut stats,
                &snap,
                "EDDF",
                t0 + chrono::Duration::seconds(crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_stable_secs.wert - 1)
            ),
            None
        );
        // Past the window → fires for EDDP.
        let fired = divert_prefetch_decision(
            &mut stats,
            &snap,
            "EDDF",
            t0 + chrono::Duration::seconds(crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_stable_secs.wert),
        );
        assert_eq!(fired.as_deref(), Some("EDDP"));
        // Idempotent — a subsequent tick does NOT re-fire.
        assert_eq!(
            divert_prefetch_decision(
                &mut stats,
                &snap,
                "EDDF",
                t0 + chrono::Duration::seconds(crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_stable_secs.wert + 5)
            ),
            None
        );
    }

    /// An ON-PLAN approach (nearest field == planned) never pre-fetches.
    #[test]
    fn divert_prefetch_silent_on_plan_approach() {
        let mut stats = FlightStats::default();
        let mut snap = rollout_snap();
        snap.lat = EDDP_26R_THR_LAT;
        snap.lon = EDDP_26R_THR_LON;
        snap.on_ground = false;
        snap.altitude_agl_ft = 1500.0;
        snap.vertical_speed_fpm = -700.0;
        let t0 = td_at();
        // Planned EDDP — nearest == planned → no divert, never fires.
        for i in 0..20 {
            assert_eq!(
                divert_prefetch_decision(
                    &mut stats,
                    &snap,
                    "EDDP",
                    t0 + chrono::Duration::seconds(i)
                ),
                None
            );
        }
        assert!(stats.divert_prefetch_icao.is_none());
    }

    /// A level overflight (not descending) never pre-fetches, and an
    /// on-ground state resets the stability timer.
    #[test]
    fn divert_prefetch_requires_descending_approach() {
        let mut stats = FlightStats::default();
        let mut snap = rollout_snap();
        snap.lat = EDDP_26R_THR_LAT;
        snap.lon = EDDP_26R_THR_LON;
        snap.on_ground = false;
        snap.altitude_agl_ft = 1500.0;
        // Level (not descending past the sink threshold).
        snap.vertical_speed_fpm = 0.0;
        let t0 = td_at();
        assert_eq!(
            divert_prefetch_decision(&mut stats, &snap, "EDDF", t0),
            None
        );
        assert!(stats.divert_prefetch_stable_since.is_none());
    }

    /// A committed approach into a field far from the plan tracks that field as
    /// the prefetch candidate and fires once it has been stable.
    ///
    /// v0.19.3: this used to assert the *memoization* of the planned airport's
    /// position (`planned_arr_position_cache`, added in v0.16.24 to dodge a 48k-row
    /// scan per tick). The cache is gone — `runway`'s ident index made the lookup
    /// cheap — so the test now asserts the behaviour the cache was there to
    /// preserve, which is the thing we actually care about.
    #[test]
    fn divert_prefetch_tracks_a_divergent_candidate_and_fires_when_stable() {
        let mut stats = FlightStats::default();

        // A committed descending approach over EDDP (far from planned EDDF).
        let mut snap = rollout_snap();
        snap.lat = EDDP_26R_THR_LAT;
        snap.lon = EDDP_26R_THR_LON;
        snap.on_ground = false;
        snap.altitude_agl_ft = 1500.0;
        snap.vertical_speed_fpm = -700.0;
        let t0 = td_at();

        // EDDP must be a genuine divergence from EDDF under the metric the
        // decision actually uses (nearest threshold, same as `arrival::locate`).
        let dist_nmi = runway::distance_to_airport_m("EDDF", snap.lat, snap.lon)
            .expect("EDDF is in the table")
            / 1852.0;
        assert!(
            dist_nmi >= crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_planned_diverge_nm.wert,
            "EDDP must be a genuine divergence from EDDF (dist={dist_nmi} nm)"
        );

        let _ = divert_prefetch_decision(&mut stats, &snap, "EDDF", t0);
        assert_eq!(
            stats.divert_prefetch_candidate.as_deref(),
            Some("EDDP"),
            "the divergent candidate is tracked on the first committed-approach tick"
        );

        let fired = divert_prefetch_decision(
            &mut stats,
            &snap,
            "EDDF",
            t0 + chrono::Duration::seconds(crate::grenzwerte::grenzwerte().abgabe.divert_prefetch_stable_secs.wert),
        );
        assert_eq!(
            fired.as_deref(),
            Some("EDDP"),
            "fires once the candidate is stable"
        );
    }

    /// An unknown planned ICAO keeps the INFINITY-distance fallback: "never near
    /// the (unmeasurable) plan" → a divergent candidate is still tracked, so a
    /// pilot flying to an off-DB strip still gets navdata pre-fetched for the
    /// field he is actually approaching.
    #[test]
    fn divert_prefetch_handles_an_absent_planned_airport() {
        let unknown = "ZZZZ"; // not a real OurAirports ICAO
        assert!(
            runway::distance_to_airport_m(unknown, 50.0, 8.0).is_none(),
            "test precondition: ZZZZ must be absent from the table"
        );
        let mut stats = FlightStats::default();
        let mut snap = rollout_snap();
        snap.lat = EDDP_26R_THR_LAT;
        snap.lon = EDDP_26R_THR_LON;
        snap.on_ground = false;
        snap.altitude_agl_ft = 1500.0;
        snap.vertical_speed_fpm = -700.0;
        let t0 = td_at();

        let _ = divert_prefetch_decision(&mut stats, &snap, unknown, t0);
        assert_eq!(stats.divert_prefetch_candidate.as_deref(), Some("EDDP"));
    }

    // ---- FSM-path equivalence (regression guard for the extraction) ----

    /// Byte-for-byte copy of the PRE-extraction FSM Landing-arm inline code
    /// (metadata + weight, v0.15.23). If the extracted helpers ever drift
    /// from this, normal flights would no longer be behaviour-identical.
    fn old_inline_stamp(stats: &mut FlightStats, snap: &SimSnapshot, actual_td_at: DateTime<Utc>) {
        let td_buf_sample = stats.snapshot_buffer.iter().find(|s| s.on_ground).cloned();
        stats.landing_pitch_deg = Some(
            snap.touchdown_pitch_deg
                .or_else(|| td_buf_sample.as_ref().map(|s| s.pitch_deg))
                .unwrap_or(snap.pitch_deg),
        );
        stats.landing_bank_deg = Some(
            snap.touchdown_bank_deg
                .or_else(|| td_buf_sample.as_ref().map(|s| s.bank_deg))
                .unwrap_or(snap.bank_deg),
        );
        stats.landing_heading_deg = Some(
            snap.touchdown_heading_mag_deg
                .or_else(|| {
                    td_buf_sample.as_ref().map(|s| {
                        let magvar = snap.heading_deg_true - snap.heading_deg_magnetic;
                        s.heading_true_deg - magvar
                    })
                })
                .unwrap_or(snap.heading_deg_magnetic),
        );
        stats.landing_speed_kt = Some(
            td_buf_sample
                .as_ref()
                .map(|s| s.indicated_airspeed_kt)
                .unwrap_or(snap.indicated_airspeed_kt),
        );
        stats.landing_groundspeed_kt = Some(
            td_buf_sample
                .as_ref()
                .map(|s| s.groundspeed_kt)
                .unwrap_or(snap.groundspeed_kt),
        );
        // v1.7.36: Auch der Sprit kommt aus dem Puffer — wie jede andere
        // Groesse hier. Diese Nachbildung der alten Inline-Sequenz muss der
        // Aenderung folgen, sonst vergleicht der Test zwei verschiedene
        // Regeln und meldet einen Unterschied, den es gar nicht gibt.
        stats.landing_fuel_kg = Some(
            td_buf_sample
                .as_ref()
                .map(|s| s.fuel_kg)
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(snap.fuel_total_kg),
        );
        stats.landing_lat = Some(td_buf_sample.as_ref().map(|s| s.lat).unwrap_or(snap.lat));
        stats.landing_lon = Some(td_buf_sample.as_ref().map(|s| s.lon).unwrap_or(snap.lon));
        stats.landing_heading_true_deg = Some(
            td_buf_sample
                .as_ref()
                .map(|s| s.heading_true_deg)
                .unwrap_or(snap.heading_deg_true),
        );
        stats.touchdown_profile = stats
            .snapshot_buffer
            .iter()
            .map(|s| TouchdownProfilePoint {
                t_ms: (s.at - actual_td_at).num_milliseconds() as i32,
                vs_fpm: s.vs_fpm,
                g_force: s.g_force,
                agl_ft: s.agl_ft,
                on_ground: s.on_ground,
                heading_true_deg: s.heading_true_deg,
                groundspeed_kt: s.groundspeed_kt,
                indicated_airspeed_kt: s.indicated_airspeed_kt,
                pitch_deg: s.pitch_deg,
                bank_deg: s.bank_deg,
            })
            .collect();
        stats.touchdown_sideslip_deg = match (snap.velocity_body_x_fps, snap.velocity_body_z_fps) {
            (Some(x), Some(z)) if z.abs() > 3.0 => Some(x.atan2(z).to_degrees()),
            _ => None,
        };
        stats.landing_headwind_kt = snap.aircraft_wind_z_kt.map(|z| -z);
        stats.landing_crosswind_kt = snap.aircraft_wind_x_kt;
        if let Some(tw) = snap.total_weight_kg {
            stats.landing_weight_kg = Some(tw as f64);
        } else {
            let zfw = snap.zfw_kg.unwrap_or(0.0);
            let weight = zfw as f64 + snap.fuel_total_kg as f64;
            if weight > 0.0 {
                stats.landing_weight_kg = Some(weight);
            }
        }
    }

    #[test]
    fn extracted_helpers_match_old_inline_fsm_sequence() {
        // Run the same snap over both paths in several configurations:
        // full buffer, empty buffer, MSFS latches present, body-velocity
        // below the sideslip floor.
        let configs: Vec<(FlightStats, SimSnapshot)> = vec![
            (stats_with_buffer(), rollout_snap()),
            (FlightStats::default(), rollout_snap()),
            (stats_with_buffer(), {
                let mut s = rollout_snap();
                s.touchdown_pitch_deg = Some(2.2);
                s.touchdown_bank_deg = Some(0.1);
                s.touchdown_heading_mag_deg = Some(269.0);
                s
            }),
            (stats_with_buffer(), {
                let mut s = rollout_snap();
                s.velocity_body_z_fps = Some(1.0); // below 3 fps floor
                s.total_weight_kg = None;
                s.zfw_kg = Some(950.0);
                s
            }),
        ];

        for (i, (proto_stats, snap)) in configs.into_iter().enumerate() {
            // Two identical stats instances (FlightStats is not Clone —
            // rebuild from the same constructor and copy the buffer).
            let mut old_stats = FlightStats {
                snapshot_buffer: proto_stats.snapshot_buffer.clone(),
                ..FlightStats::default()
            };
            let mut new_stats = FlightStats {
                snapshot_buffer: proto_stats.snapshot_buffer.clone(),
                ..FlightStats::default()
            };

            old_inline_stamp(&mut old_stats, &snap, td_at());

            let td_buf = touchdown_buffer_sample(&new_stats);
            stamp_touchdown_metadata(&mut new_stats, &snap, td_at(), td_buf.as_ref());
            stamp_landing_weight(&mut new_stats, &snap);

            assert_eq!(
                old_stats.landing_pitch_deg, new_stats.landing_pitch_deg,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_bank_deg, new_stats.landing_bank_deg,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_heading_deg, new_stats.landing_heading_deg,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_speed_kt, new_stats.landing_speed_kt,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_groundspeed_kt, new_stats.landing_groundspeed_kt,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_fuel_kg, new_stats.landing_fuel_kg,
                "cfg {i}"
            );
            assert_eq!(old_stats.landing_lat, new_stats.landing_lat, "cfg {i}");
            assert_eq!(old_stats.landing_lon, new_stats.landing_lon, "cfg {i}");
            assert_eq!(
                old_stats.landing_heading_true_deg, new_stats.landing_heading_true_deg,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.touchdown_sideslip_deg, new_stats.touchdown_sideslip_deg,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_headwind_kt, new_stats.landing_headwind_kt,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_crosswind_kt, new_stats.landing_crosswind_kt,
                "cfg {i}"
            );
            assert_eq!(
                old_stats.landing_weight_kg, new_stats.landing_weight_kg,
                "cfg {i}"
            );
            // Profile: same length + identical serialised content
            // (TouchdownProfilePoint has no PartialEq).
            assert_eq!(
                serde_json::to_value(&old_stats.touchdown_profile).unwrap(),
                serde_json::to_value(&new_stats.touchdown_profile).unwrap(),
                "cfg {i}"
            );
        }
    }

    /// v3 (Kap. 5.2): Der Aufsetzstempel haelt Sim, Simfassung, Profil und
    /// Kategorie fuer `meta` des Eingabe-Dokuments fest — X-Plane mit der
    /// Fassung aus dem Plugin. Gegenprobe: MSFS ohne `sim_version` nimmt die
    /// SimConnect-Kennung; ein Hubschrauber (Kufen) wird `helicopter`.
    #[test]
    fn stempel_haelt_sim_profil_und_kategorie_fest() {
        let mut st = stats_with_buffer();
        st.sim_kennung = Some("KittyHawk 12.1".into());
        let mut snap = rollout_snap();
        snap.simulator = Simulator::XPlane12;
        snap.sim_version = Some("12100".into());
        snap.aircraft_profile = sim_core::AircraftProfile::FenixA320;
        stamp_touchdown_metadata(&mut st, &snap, td_at(), None);
        let m = st.landing_sim_meta.clone().expect("meta gestempelt");
        assert_eq!(m["sim"], "XPlane12");
        assert_eq!(m["sim_fassung"], "12100");
        assert_eq!(m["profil"], "fenix_a320");
        assert_eq!(m["kategorie"], "fixed_wing");

        snap.simulator = Simulator::Msfs2024;
        snap.sim_version = None;
        snap.gear_is_skid = Some(true);
        stamp_touchdown_metadata(&mut st, &snap, td_at(), None);
        let m = st.landing_sim_meta.clone().unwrap();
        assert_eq!(m["sim"], "Msfs2024");
        assert_eq!(m["sim_fassung"], "KittyHawk 12.1");
        assert_eq!(m["kategorie"], "helicopter");
    }
