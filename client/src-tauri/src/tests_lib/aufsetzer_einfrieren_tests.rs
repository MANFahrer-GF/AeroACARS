    use super::*;

    /// v3 (P-4.13): Testflug wie in `positions_idempotenz_tests`.
    fn flug() -> ActiveFlight {
        ActiveFlight {
            pirep_id: "test-pirep".into(),
            bid_id: 0,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            airline_icao: String::new(),
            planned_registration: String::new(),
            aircraft_icao: "C172".into(),
            aircraft_name: String::new(),
            flight_number: "1".into(),
            dpt_airport: "EDDF".into(),
            arr_airport: "ZZZZ".into(),
            planned_flight_time_min: None,
            plan_strecke_nm: None,
            airline_logo_url: None,
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

    fn probe() -> TouchdownWindowSample {
        TouchdownWindowSample::from(TelemetrySample::default())
    }

    /// P-4.13: Ein Touch-and-Go wird VOR dem Zuruecksetzen eingefroren —
    /// Eingabe und Fenster gehoeren zu genau diesem Aufsetzer, das Fenster
    /// wandert mit (die naechste Landung bekommt ein eigenes).
    #[test]
    fn touch_and_go_wird_vor_dem_zuruecksetzen_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        s.phase = FlightPhase::Landing;
        s.landing_at = Some(Utc::now());
        s.landung_fenster = vec![probe(), probe()];
        s.landing_peak_vs_fpm = Some(-240.0);
        s.landing_source = Some("vs_at_impact".into());
        s.bounce_count = 2;
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren.len(), 1);
        let (eingabe, fenster, begleitung) = s.aufsetzer_eingefroren[0].clone();
        let herkunft = begleitung.herkunft;
        assert_eq!(fenster.len(), 2);
        assert!(s.landung_fenster.is_empty(), "Fenster wandert mit dem Aufsetzer");
        assert_eq!(eingabe, landungs_bewertungseingabe(&f, &s, Some("C172"), "ZZZZ"), "Eingabe vor dem Zuruecksetzen");
        // P-9.44: Die Herkunft ist die DIESES Aufsetzers, eingefroren vor dem
        // Zuruecksetzen — die naechste Landung ueberschreibt sie nicht.
        assert_eq!(herkunft["sinkrate"]["landing_source"], "vs_at_impact");
        assert_eq!(herkunft["sinkrate"]["hopser"], 2);
        s.landing_source = Some("smoothed_500ms".into());
        s.bounce_count = 0;
        let jetzt = landungs_herkunft(&f, &s, &eingabe, "ZZZZ");
        assert_eq!(jetzt["sinkrate"]["landing_source"], "smoothed_500ms", "Gegenprobe: liest die Flugdaten");
        assert_eq!(s.aufsetzer_eingefroren[0].2.herkunft["sinkrate"]["landing_source"], "vs_at_impact");
    }

    /// Kap. 5.2: der Herkunftsblock traegt die Bahnquelle mit Commit, Zyklus
    /// und Szenerie sowie Spurweite, Vref, Abdeckung, Ausweichziel, Phasen.
    #[test]
    fn herkunft_nennt_die_bahnquelle() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        let e = landungs_bewertungseingabe(&f, &s, Some("C172"), "EDDM");
        let h = landungs_herkunft(&f, &s, &e, "EDDM");
        assert!(h["bahn"]["quelle"]["art"].is_null(), "ohne Zuordnung keine Quelle");
        assert_eq!(h["ausweichziel"], serde_json::json!({ "eingereicht": "EDDM", "geplant": "ZZZZ" }));
        assert_eq!(h["bahn"]["quelle"]["ourairports_commit"], serde_json::to_value(&laufzeit_paket::stand().ourairports_commit).unwrap());
        assert_eq!(h["bahn"]["quelle"]["paket"], serde_json::to_value(&laufzeit_paket::stand().id).unwrap());
        for k in ["sinkrate", "spurweite", "vref", "abdeckung_fehlt", "divert_hint", "phasenwechsel", "aufsetzer", "durchstarts"] {
            assert!(h.get(k).is_some(), "{k} fehlt");
        }
        for k in ["zuordnung", "runway_nav_geometry", "tch_ist_ft", "szenerie_stand", "korrekturen", "rollspur"] {
            assert!(h["bahn"].get(k).is_some(), "bahn.{k} fehlt");
        }
        s.runway_source = Some(runway::RunwaySource::OurAirportsFallback);
        assert_eq!(landungs_herkunft(&f, &s, &e, "EDDM")["bahn"]["quelle"]["art"], "ourairports");
        s.runway_source = Some(runway::RunwaySource::Navigraph);
        s.runway_nav_cycle = Some("2611".into());
        let h = landungs_herkunft(&f, &s, &e, "EDDM");
        assert_eq!(h["bahn"]["quelle"]["art"], "navigraph");
        assert_eq!(h["bahn"]["quelle"]["navigraph_zyklus"], "2611");
        s.szenerie_uebernahme = Some(szenerie_bahn::UebernahmeBericht { uebernommen: vec!["26R".into()], ..Default::default() });
        assert_eq!(landungs_herkunft(&f, &s, &e, "EDDM")["bahn"]["quelle"]["art"], "szenerie");
        // Spurweite: Flugzeugdatei vor Typtabelle.
        s.fahrwerk_spurweite_m = Some(3.1);
        let e2 = landungs_bewertungseingabe(&f, &s, Some("C172"), "EDDM");
        let h2 = landungs_herkunft(&f, &s, &e2, "EDDM");
        assert_eq!(h2["spurweite"]["quelle"], "flugzeugdatei");
        assert_eq!(h2["spurweite"]["m"], 3.1);
    }

    /// P-9.44 (Codex 30.10.): Die Touch-and-Go-Entscheidung DIESES Aufsetzers
    /// faellt erst nach dem Einfrieren; sie wird in seine Herkunft
    /// nachgetragen. Gegenprobe: ein fremder Aufsetzer (andere Zeit) bleibt
    /// unberuehrt.
    #[test]
    fn touch_and_go_entscheidung_steht_in_der_herkunft() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        s.phase = FlightPhase::Landing;
        s.was_airborne = true;
        s.landing_at = Some(Utc::now());
        s.landung_fenster = vec![probe()];
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren[0].2.herkunft["durchstarts"], 0, "vor der Entscheidung");
        s.landing_vref_gemessen = Some((131.0, "pmdg"));
        landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
        assert_eq!(s.landing_vref_gemessen, None, "Vref des Touch-and-Go nicht an den naechsten Aufsetzer");
        let h = &s.aufsetzer_eingefroren[0].2.herkunft;
        assert_eq!(h["durchstarts"], 1);
        assert_eq!(h["aufsetzer"].as_array().unwrap().last().unwrap()["kind"], "touch_and_go");

        // Gegenprobe: eingefroren fuer einen anderen Aufsetzer → kein Nachtrag.
        s.phase = FlightPhase::Landing;
        s.landing_at = Some(Utc::now() + chrono::Duration::seconds(60));
        s.landung_fenster = vec![probe()];
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        s.aufsetzer_eingefroren[1].2.herkunft["aufsetzen"] = serde_json::json!("anderer");
        landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
        assert_eq!(s.aufsetzer_eingefroren[1].2.herkunft["durchstarts"], 1, "fremder Aufsetzer unberuehrt");
    }

    /// P-4.13: Schliesst `landung_beim_wegsteigen_abschliessen` nicht ab
    /// (Phasenpruefung: nach dem Abrollen, oder Startlauf ohne Abheben), ist
    /// es kein Touch-and-Go. Dann wird auch nichts eingefroren — sonst ginge
    /// ein Dokument `touch_and_go` ohne Entscheidung hinaus, und die echte
    /// Landung verloere ihr Fenster. Gegenprobe: in der Landung, nach dem
    /// Fliegen, wird eingefroren und die Entscheidung nachgetragen.
    #[test]
    fn ohne_abschluss_wird_nichts_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        for (phase, in_der_luft) in [(FlightPhase::TaxiIn, true), (FlightPhase::Takeoff, false)] {
            s.phase = phase;
            s.was_airborne = in_der_luft;
            s.landing_at = Some(Utc::now());
            s.landung_fenster = vec![probe(), probe()];
            aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
            landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
            assert!(s.aufsetzer_eingefroren.is_empty(), "{phase:?}/{in_der_luft}: kein Touch-and-Go");
            assert_eq!(s.landung_fenster.len(), 2, "{phase:?}/{in_der_luft}: Fenster bleibt der Landung");
        }
        // Gegenprobe: offene Landung → eingefroren, Entscheidung steht drin.
        s.phase = FlightPhase::Landing;
        s.was_airborne = true;
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        landung_beim_wegsteigen_abschliessen(&mut s, &SimSnapshot::default(), Utc::now());
        assert_eq!(s.aufsetzer_eingefroren.len(), 1);
        assert_eq!(s.aufsetzer_eingefroren[0].2.herkunft["durchstarts"], 1);
        assert!(s.landung_fenster.is_empty());
    }

    #[test]
    fn ohne_fenster_wird_nichts_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        // Offene Landung: nur die Fensterbedingung haelt (Codex 30.10.).
        s.phase = FlightPhase::Landing;
        s.landing_at = Some(Utc::now());
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert!(s.aufsetzer_eingefroren.is_empty());
        // Gegenprobe: mit Fenster wird eingefroren.
        s.landung_fenster = vec![probe()];
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren.len(), 1);
    }

    /// Server: `seq` hoechstens `SEQ_MAX` (200, wie `landingInput.ts`). Die
    /// letzte Nummer bleibt der Landung; das Fenster des nicht mehr
    /// dokumentierten Aufsetzers ist trotzdem weg. Gegenprobe: eine Stelle
    /// darunter wird eingefroren — 199 Touch-and-Go + Landung = seq 200, ein
    /// 201. Dokument entsteht nie.
    #[test]
    fn touch_and_go_ueber_der_dokumentgrenze() {
        assert_eq!(landungs_eingabe::SEQ_MAX, 200, "Grenze des Servers");
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        s.phase = FlightPhase::Landing;
        s.landing_at = Some(Utc::now());
        let leer = (landing_scoring::LandingScoringInput::default(), vec![probe()], landungs_eingabe::Begleitung::default());
        s.aufsetzer_eingefroren = vec![leer; landungs_eingabe::SEQ_MAX as usize - 2];
        s.landung_fenster = vec![probe()];
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren.len(), landungs_eingabe::SEQ_MAX as usize - 1, "Gegenprobe: noch Platz");
        s.landung_fenster = vec![probe()];
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        assert_eq!(s.aufsetzer_eingefroren.len(), landungs_eingabe::SEQ_MAX as usize - 1, "Platz fuer die Landung");
        assert_eq!(s.aufsetzer_eingefroren.len() + 1, 200, "Landung bekommt seq 200, keine 201");
        assert!(s.landung_fenster.is_empty(), "Fenster nicht an die naechste Landung vererbt");
    }

    /// Anflugprobe aus einer echten Fensterprobe (Werte mit voller Stellenzahl).
    fn anflugprobe(w: &TouchdownWindowSample, at: DateTime<Utc>) -> ApproachBufferSample {
        ApproachBufferSample {
            at,
            agl_ft: w.agl_ft + 900.0,
            msl_ft: w.agl_ft + 1_300.0,
            gs_kt: w.groundspeed_kt,
            ias_kt: w.indicated_airspeed_kt,
            vs_fpm: w.vs_fpm,
            bank_deg: w.bank_deg,
            heading_true_deg: w.heading_true_deg,
            gear_position: 1.0,
            flaps_position: 0.75,
            selected_runway: Some("27R".into()),
            stall_warning: false,
            lat: Some(w.lat),
            lon: Some(w.lon),
            pitch_deg: Some(w.pitch_deg),
            n1_mittel_pct: Some(w.g_force * 55.0),
            headwind_kt: Some(w.bank_deg * 3.0),
            crosswind_kt: Some(w.pitch_deg * 1.7),
        }
    }

    /// P-4.13/Kap. 5.2: `meta` und `roh` (Anflugpuffer, Rollspur, METAR)
    /// werden mit dem Touch-and-Go eingefroren — der naechste Anflug
    /// ueberschreibt sie nicht. Gegenprobe: neu gebaut zeigt sie die
    /// aktuellen Flugdaten.
    #[test]
    fn meta_und_roh_werden_mit_eingefroren() {
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        s.phase = FlightPhase::Landing;
        s.landing_at = Some(Utc::now());
        s.landung_fenster = vec![probe()];
        let w = probe();
        s.approach_buffer = (0..3).map(|i| anflugprobe(&w, Utc::now() + chrono::Duration::seconds(i))).collect();
        s.anflug_forensik_puffer = (0..5).map(|i| anflugprobe(&w, Utc::now() + chrono::Duration::seconds(i))).collect();
        s.bahn_spur = vec![(0.0, 0.5), (12.0, 0.7)];
        s.arr_metar_raw = Some("EDDF 301050Z 25008KT CAVOK 12/04 Q1021".into());
        s.landing_sim_meta = Some(serde_json::json!({ "sim": "XPlane12", "kategorie": "fixed_wing" }));
        s.aufgeloestes_muster = Some("A20N".into());
        aufsetzer_einfrieren(&f, &mut s, &SimSnapshot::default());
        let b = s.aufsetzer_eingefroren[0].2.clone();
        assert_eq!(b.roh["anflugpuffer"]["note"].as_array().unwrap().len(), 3);
        assert_eq!(b.roh["anflugpuffer"]["forensik"].as_array().unwrap().len(), 5);
        assert_eq!(b.roh["rollspur"]["punkte_laengs_quer_m"].as_array().unwrap().len(), 2);
        assert_eq!(b.roh["metar"], "EDDF 301050Z 25008KT CAVOK 12/04 Q1021");
        assert_eq!(b.meta["sim"], "XPlane12");
        assert_eq!(b.meta["aircraft_icao"], "A20N", "Muster der Rechnung");

        // Naechster Anflug: neue Daten.
        s.approach_buffer.clear();
        s.anflug_forensik_puffer.clear();
        s.bahn_spur.clear();
        s.arr_metar_raw = Some("EDDF 301120Z 27015KT 9999 BKN012".into());
        s.landing_sim_meta = None;
        assert_eq!(s.aufsetzer_eingefroren[0].2.roh, b.roh, "eingefroren");
        let jetzt = landungs_begleitung(&f, &s, &landing_scoring::LandingScoringInput::default(), Some("A20N"), "ZZZZ");
        assert_eq!(jetzt.roh["anflugpuffer"]["forensik"].as_array().unwrap().len(), 0, "Gegenprobe: liest die Flugdaten");
        assert_eq!(jetzt.roh["metar"], "EDDF 301120Z 27015KT 9999 BKN012");
        assert!(jetzt.meta["sim"].is_null(), "ohne Stempel: Schluessel da, Wert null");
        assert!(jetzt.meta.get("kategorie").is_some());
    }

    /// Kap. 5.2 / Server `recorder/src/landingInput.ts`: Ein Dokument mit
    /// vollen Puffern (120 + 400 Anflugproben, 400 Spurpunkte — die Kappen des
    /// Clients) und einem echten X-Plane-Fenster (ity324, 627 Proben zu 50 Hz)
    /// bleibt deutlich unter der Grenze. Geprueft wird der schlimmste Fall,
    /// den der Server annimmt: das Fenster auf `PROBEN_MAX` = 5000 Proben
    /// gestreckt. Schranke: die HAELFTE von `GZIP_MAX_BYTES` (2 MiB) und
    /// `ENTPACKT_MAX_BYTES` (16 MiB) — Luft fuer kuenftige Felder.
    #[test]
    fn eingabe_dokument_bleibt_deutlich_unter_der_servergrenze() {
        const GZIP_MAX_BYTES: u64 = 2 * 1024 * 1024;
        const ENTPACKT_MAX_BYTES: usize = 16 * 1024 * 1024;
        const PROBEN_MAX: usize = 5000;
        let pfad = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ity324_xplane_steep_control.jsonl.gz");
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(std::fs::File::open(pfad).unwrap()), &mut text).unwrap();
        let echt: Vec<TouchdownWindowSample> = text
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["type"] == "touchdown_window")
            .map(|v| serde_json::from_value(v["samples"].clone()).unwrap())
            .expect("Fenster im Fixture");
        assert_eq!(echt.len(), 627);
        let t0 = echt[0].at;
        let f = flug();
        let mut s = f.stats.lock().unwrap();
        s.approach_buffer = (0..120).map(|i| anflugprobe(&echt[i * 5 % echt.len()], t0 + chrono::Duration::seconds(i as i64))).collect();
        s.anflug_forensik_puffer = (0..400).map(|i| anflugprobe(&echt[i * 3 % echt.len()], t0 + chrono::Duration::seconds(i as i64))).collect();
        s.bahn_spur = (0..400).map(|i| (i as f32 * 7.3137, (i as f32 * 0.913).sin() * 4.17)).collect();
        s.arr_metar_raw = Some("LFPG 091300Z 26012G24KT 230V290 9999 FEW030 BKN045 18/09 Q1015 NOSIG".into());
        let e = landungs_bewertungseingabe(&f, &s, Some("A320"), "LFPG");
        let b = landungs_begleitung(&f, &s, &e, Some("A320"), "LFPG");
        let ergebnis = crate::bewertung::bewerten(&e);
        let ordner = std::env::temp_dir().join(format!("aeroacars-groesse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ordner);
        let messen = |fenster: &[TouchdownWindowSample], seq: u32| {
            let doc = landungs_eingabe::dokument("P1", seq, landungs_eingabe::Art::Landung, None, &e, &b, fenster, &ergebnis);
            let roh = doc.to_string().len();
            let datei = landungs_eingabe::einreihen(&ordner, "k", "P1", seq, &doc).unwrap();
            (roh, std::fs::metadata(datei).unwrap().len())
        };
        let (roh_echt, gz_echt) = messen(&echt, 1);
        let gestreckt: Vec<TouchdownWindowSample> = (0..PROBEN_MAX)
            .map(|i| TouchdownWindowSample { at: t0 + chrono::Duration::milliseconds(i as i64 * 20), ..echt[i % echt.len()] })
            .collect();
        let (roh_max, gz_max) = messen(&gestreckt, 2);
        eprintln!("Eingabe-Dokument: echt {roh_echt} B roh / {gz_echt} B gzip; bei {PROBEN_MAX} Proben {roh_max} B roh / {gz_max} B gzip");
        let _ = std::fs::remove_dir_all(&ordner);
        assert!(gz_max <= GZIP_MAX_BYTES / 2, "gzip {gz_max} B");
        assert!(roh_max <= ENTPACKT_MAX_BYTES / 2, "entpackt {roh_max} B");
        // Gegenprobe der Messung: die Puffer stehen wirklich im Dokument.
        assert!(gz_echt > 20_000, "Puffer fehlen? {gz_echt} B");
    }
