    use super::*;

    /// The stand from the field report: EDDF Terminal 2.
    const EDDF_TERMINAL_2: (f64, f64) = (50.0500, 8.5860);
    /// EDDP (Leipzig) — a genuinely different field, for the real-divert case.
    const EDDP_APRON: (f64, f64) = (51.4239, 12.2364);

    /// A flight planned to a real airport, armed for the fallback (has flown,
    /// is on the ground with engines off, dwell already elapsed).
    fn flight_planned_to(arr: &str, at: (f64, f64)) -> (ActiveFlight, SimSnapshot) {
        let flight = ActiveFlight {
            pirep_id: "test-pirep".into(),
            bid_id: 0,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            airline_icao: String::new(),
            planned_registration: String::new(),
            aircraft_icao: "B738".into(),
            aircraft_name: String::new(),
            flight_number: "859".into(),
            dpt_airport: "ENGM".into(),
            arr_airport: arr.into(),
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
        };
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Climb;
            stats.block_off_at = Some(Utc::now() - chrono::Duration::minutes(90));
            stats.was_airborne = true;
            stats.arrived_fallback_pending_since =
                Some(Utc::now() - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().phasen_kern.arrived_fallback_dwell_secs.wert + 1));
        }
        // parked, engines off, on the ground — see SimSnapshot::default()
        let snap = SimSnapshot {
            lat: at.0,
            lon: at.1,
            ..SimSnapshot::default()
        };
        (flight, snap)
    }

    /// THE regression. A normal arrival at a big airport is an arrival.
    #[test]
    fn parking_at_eddf_on_a_flight_planned_to_eddf_is_not_a_divert() {
        let (flight, snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        let result = step_flight(&flight, &snap);

        assert_eq!(result, Some(FlightPhase::Arrived), "the flight has arrived");
        let stats = flight.stats.lock().unwrap();
        assert!(
            stats.divert_hint.is_none(),
            "parking at the planned field must never raise a divert hint — got {:?}",
            stats.divert_hint
        );
    }

    /// The consequence pilots actually felt: a hint suppresses auto-filing
    /// (see the `new_phase == Arrived && divert_hint.is_none()` gate in the
    /// streamer). No hint ⇒ the PIREP files itself, as it should.
    #[test]
    fn a_normal_big_airport_arrival_does_not_block_auto_filing() {
        let (flight, snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        step_flight(&flight, &snap);
        assert!(
            flight.stats.lock().unwrap().divert_hint.is_none(),
            "a divert hint here would silently withhold the pilot's auto-file"
        );
    }

    /// The fallback must not become a way to lose data: it skips BlocksOn, so
    /// it has to capture what BlocksOn captures. Before v0.19.3 the arrival
    /// stand was silently dropped on every fallback arrival.
    #[test]
    fn the_fallback_captures_the_arrival_stand_that_blockson_would_have() {
        // v1.6.1: der Test uebte frueher den `snap.parking_name`-Zweig —
        // einen Pfad, der im Feld nie lief (beide Sims liefern None).
        // Jetzt uebt er den echten Weg: OSM-Standliste + Position.
        let (flight, snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.arr_stands = Some(vec![crate::stands::ParkingStand {
                name: Some("A17".into()),
                lat: EDDF_TERMINAL_2.0,
                lon: EDDF_TERMINAL_2.1,
                linie: None,
                flaeche: false,
            }]);
            stats.arr_stands_icao = Some("EDDF".into());
        }

        step_flight(&flight, &snap);

        let stats = flight.stats.lock().unwrap();
        assert_eq!(stats.phase, FlightPhase::Arrived);
        assert_eq!(
            stats.arr_gate.as_deref(),
            Some("A17"),
            "arriving via the fallback must still record the arrival stand"
        );
        assert!(stats.block_on_at.is_some(), "block-on time must be stamped");
    }

    /// The block-on fuel/weight diagnostic keys off `blocks_on_reached`, which
    /// must survive a resume. Keying it off the in-memory transition list (as
    /// the first cut of this fix did) would tell every resumed flight it had
    /// "never blocked on" and emit the line a second time.
    #[test]
    fn blocks_on_reached_survives_a_resume() {
        let (flight, snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::TaxiIn;
            stats.arrived_fallback_pending_since = None;
        }
        let mut snap_at_gate = snap.clone();
        snap_at_gate.parking_brake = true;
        snap_at_gate.groundspeed_kt = 0.0;

        // Normal arrival: TaxiIn → BlocksOn sets the flag.
        assert_eq!(
            step_flight(&flight, &snap_at_gate),
            Some(FlightPhase::BlocksOn)
        );
        assert!(flight.stats.lock().unwrap().blocks_on_reached);

        // Round-trip through persistence (what a resume does).
        let snapshot = PersistedFlightStats::snapshot_from(&flight.stats.lock().unwrap());
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        let restored: PersistedFlightStats = serde_json::from_str(&json).expect("snapshot parses");
        let mut fresh = FlightStats::new();
        restored.apply_to(&mut fresh);
        assert!(
            fresh.blocks_on_reached,
            "a resumed flight must remember that it already blocked on"
        );
    }

    /// v1.7.17 Runde 13 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): `dep_gate_withdrawal_pending` muss einen Resume
    /// ueberleben — sonst wuerde ein App-Neustart zwischen Boarding-Ende
    /// (Riegel setzt das Flag) und der finalen Einreichung das Flag
    /// stillschweigend zuruecksetzen, und ein bekanntlich veralteter,
    /// bereits live geposteter Wert bliebe in der finalen PIREP-
    /// Einreichung unkorrigiert stehen.
    #[test]
    fn dep_gate_withdrawal_pending_ueberlebt_einen_resume() {
        let (flight, _snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.dep_gate = None;
            stats.dep_gate_withdrawal_pending = true;
        }
        let snapshot = PersistedFlightStats::snapshot_from(&flight.stats.lock().unwrap());
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        let restored: PersistedFlightStats = serde_json::from_str(&json).expect("snapshot parses");
        let mut fresh = FlightStats::new();
        restored.apply_to(&mut fresh);
        assert!(
            fresh.dep_gate_withdrawal_pending,
            "ein anhaengiger Rueckzug muss einen Resume ueberleben, sonst bleibt \
             ein bekanntlich veralteter Wert in der finalen Einreichung unkorrigiert"
        );
    }

    /// Kabinenansagen (Beta): Flug mit 2.0.25 gestartet, mitten im Flug auf
    /// die Beta gewechselt. Der gespeicherte Stand kennt keine Passagiere →
    /// ohne Nachholen stumm (Gegenprobe); nach dem Nachholen aus dem Bid
    /// laufen die Ansagen, der OFP geht vor, ein fremdes Bid aendert nichts.
    #[test]
    fn kabinen_eckdaten_nach_wechsel_mitten_im_flug_nachgeholt() {
        let mut alt: serde_json::Value =
            serde_json::to_value(PersistedFlightStats::snapshot_from(&FlightStats::new())).expect("json");
        for k in ["pax_count", "max_passengers", "flight_type", "airline_name"] {
            alt.as_object_mut().unwrap().remove(k);
        }
        let alt: PersistedFlightStats = serde_json::from_value(alt).expect("Stand von 2.0.25 laedt");
        let mut stats = FlightStats::new();
        alt.apply_to(&mut stats);
        let flugart = |s: &FlightStats| kabinen_flugdaten(s, "LEPA", Some(140), "A320").flugart;
        assert_eq!(flugart(&stats), Some(cabin_announcements::Flugart::Aus), "ohne Nachholen stumm");

        let bids: Vec<api_client::Bid> = serde_json::from_value(serde_json::json!([
            { "id": 7, "user_id": 1, "flight_id": "a", "flight": {
                "id": "a", "flight_number": "100", "dpt_airport_id": "EDDF", "arr_airport_id": "LEPA",
                "flight_type": "J", "airline": { "id": 1, "icao": "CFG", "name": "Condor" },
                "simbrief": { "id": "sb7", "subfleet": { "fares": [
                    { "id": 1, "type": 0, "count": 180, "capacity": 220 }
                ] } } } }
        ]))
        .expect("bids parsen");

        // Fremdes Bid: nichts passiert.
        kabinen_eckdaten_nachholen_aus(&mut stats, &bids, 99, None);
        assert_eq!(stats.pax_count, None);

        kabinen_eckdaten_nachholen_aus(&mut stats, &bids, 7, None);
        assert_eq!(stats.pax_count, Some(180));
        assert_eq!(stats.airline_name.as_deref(), Some("Condor"));
        assert_eq!(flugart(&stats), Some(cabin_announcements::Flugart::Linie));

        // Mit OFP gehen dessen Passagiere vor, wie beim Start.
        let mut mit_ofp = FlightStats::new();
        let ofp = api_client::SimBriefOfp { pax_count: 164, ..Default::default() };
        kabinen_eckdaten_nachholen_aus(&mut mit_ofp, &bids, 7, Some(&ofp));
        assert_eq!(mit_ofp.pax_count, Some(164));

        // Schon bekannte Passagiere bleiben stehen.
        kabinen_eckdaten_nachholen_aus(&mut mit_ofp, &bids, 7, None);
        assert_eq!(mit_ofp.pax_count, Some(164));
    }

    /// Kabinenansagen (Beta): Die Eckdaten kommen aus Bid und OFP, ein
    /// Frachtflug (0 Passagiere) bleibt `Some(0)` und alles ueberlebt einen
    /// Resume. Ein altes `active_flight.json` ohne die Felder laedt mit `None`.
    #[test]
    fn kabinen_eckdaten_aus_bid_und_ofp_ueberleben_einen_resume() {
        let bid_flight: api_client::Flight = serde_json::from_value(serde_json::json!({
            "id": "4711",
            "flight_number": "8402",
            "dpt_airport_id": "EDDF",
            "arr_airport_id": "EDDH",
            "flight_type": " F ",
            "airline": { "id": 3, "icao": "BOX", "name": "AeroLogic" }
        }))
        .expect("bid parses");
        let ofp = api_client::SimBriefOfp {
            pax_count: 0,
            max_passengers: Some(0),
            ..Default::default()
        };
        let mut stats = FlightStats::new();
        kabinen_eckdaten_aus_bid(&mut stats, &bid_flight);
        kabinen_eckdaten_aus_ofp(&mut stats, &ofp);
        assert_eq!(stats.flight_type.as_deref(), Some("F"));
        assert_eq!(stats.airline_name.as_deref(), Some("AeroLogic"));
        assert_eq!(stats.pax_count, Some(0), "0 Pax ist der Frachtflug, nicht „unbekannt“");
        assert_eq!(stats.max_passengers, None);

        let snapshot = PersistedFlightStats::snapshot_from(&stats);
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        let restored: PersistedFlightStats = serde_json::from_str(&json).expect("snapshot parses");
        let mut fresh = FlightStats::new();
        restored.apply_to(&mut fresh);
        assert_eq!(fresh.pax_count, Some(0));
        assert_eq!(fresh.flight_type.as_deref(), Some("F"));
        assert_eq!(fresh.airline_name.as_deref(), Some("AeroLogic"));

        let mut alt: serde_json::Value = serde_json::from_str(&json).expect("json");
        for k in ["pax_count", "max_passengers", "flight_type", "airline_name"] {
            assert!(alt.as_object_mut().unwrap().remove(k).is_some(), "{k} fehlt im Snapshot");
        }
        let alt: PersistedFlightStats = serde_json::from_value(alt).expect("alter Stand laedt");
        let mut fresh = FlightStats::new();
        alt.apply_to(&mut fresh);
        assert_eq!(
            (fresh.pax_count, fresh.max_passengers, fresh.flight_type, fresh.airline_name),
            (None, None, None, None)
        );

        // Leerer Airline-Name und fehlende Flugart gelten als fehlend.
        let leer: api_client::Flight = serde_json::from_value(serde_json::json!({
            "id": "1", "flight_number": "1", "dpt_airport_id": "EDDF", "arr_airport_id": "EDDM",
            "airline": { "id": 1, "name": "  " }
        }))
        .expect("bid parses");
        let mut stats = FlightStats::new();
        kabinen_eckdaten_aus_bid(&mut stats, &leer);
        assert_eq!((stats.flight_type, stats.airline_name), (None, None));
        assert_eq!(stats.pax_count, None, "ohne Fares keine Passagierzahl → keine Ansagen");
    }

    /// Kabinenansagen (Beta): Ohne OFP zaehlen nur die Passagier-Fares
    /// (phpVMS `type` 0) des Bids; Fracht-Fares nicht. Der OFP ueberschreibt
    /// die Pax, laesst aber die Sitzzahl stehen, wenn er selbst keine hat.
    #[test]
    fn kabinen_pax_ohne_ofp_aus_den_passagier_fares() {
        let bid_flight: api_client::Flight = serde_json::from_value(serde_json::json!({
            "id": "1", "flight_number": "400", "dpt_airport_id": "EDDF", "arr_airport_id": "KJFK",
            "simbrief": { "id": "sb1", "subfleet": { "fares": [
                { "id": 1, "type": 0, "count": 20, "capacity": 30 },
                { "id": 2, "type": 0, "count": 250, "capacity": 260 },
                { "id": 3, "type": 1, "count": 9000, "capacity": 20000 },
                { "id": 4, "count": 7, "capacity": 7 }
            ] } }
        }))
        .expect("bid parses");
        let mut stats = FlightStats::new();
        kabinen_eckdaten_aus_bid(&mut stats, &bid_flight);
        assert_eq!(stats.pax_count, Some(270));
        assert_eq!(stats.max_passengers, Some(290));

        kabinen_eckdaten_aus_ofp(
            &mut stats,
            &api_client::SimBriefOfp { pax_count: 265, max_passengers: None, ..Default::default() },
        );
        assert_eq!((stats.pax_count, stats.max_passengers), (Some(265), Some(290)));

        // Nur Fracht-Fares: keine Passagierzahl.
        let fracht: api_client::Flight = serde_json::from_value(serde_json::json!({
            "id": "2", "flight_number": "8400", "dpt_airport_id": "EDDP", "arr_airport_id": "LEJ",
            "simbrief": { "id": "sb2", "subfleet": { "fares": [
                { "id": 3, "type": 1, "count": 9000, "capacity": 20000 }
            ] } }
        }))
        .expect("bid parses");
        let mut stats = FlightStats::new();
        kabinen_eckdaten_aus_bid(&mut stats, &fracht);
        assert_eq!((stats.pax_count, stats.max_passengers), (None, None));
    }

    /// 3.0 (Analyse V2, `kabine-lage-1`): Das Kabinenmodul bekommt mit
    /// jedem Takt das Flugbild — der Fall v2.1.4 (`planned_zfw_kg`) braucht
    /// so kein neues Kernfeld mehr. Durch den Befehlskanal unveraendert.
    /// Gegenproben: ohne Lage geht das Feld gar nicht mit (alte Module sehen
    /// dieselben Bytes wie bisher), und ein Takt ohne `lage` (alter Client)
    /// liest sich als `Null`.
    #[test]
    fn kabinen_lage_traegt_das_flugbild_zum_modul() {
        use cabin_announcements::modul::Befehl;
        let mut stats = FlightStats::new();
        stats.phase = FlightPhase::Boarding;
        stats.planned_zfw_kg = Some(61_234.0);
        let mut snap = SimSnapshot::default();
        snap.altitude_msl_ft = 1_234.0;
        let lage = kabinen_lage(&stats, &snap, "EDDH", Some(60), "A320");
        assert_eq!(lage["ofp"]["zfw_kg"], 61_234.0);
        assert_eq!(lage["snapshot"]["altitude_msl_ft"], 1_234.0);
        assert_eq!(lage["flug"]["ziel_plan_icao"], "EDDH");
        assert_eq!(lage["phase"], serde_json::to_value(FlightPhase::Boarding).unwrap());
        assert!(serde_json::to_vec(&lage).unwrap().len() <= KABINEN_LAGE_MAX_BYTES);
        let takt = |lage: serde_json::Value| Befehl::Tick { tick: cabin_announcements::Tick { lage, ..Default::default() } };
        let zurueck: Befehl = serde_json::from_str(&serde_json::to_string(&takt(lage.clone())).unwrap()).unwrap();
        let Befehl::Tick { tick } = zurueck else { panic!("kein Takt") };
        assert_eq!(tick.lage, lage);
        let ohne = serde_json::to_string(&takt(serde_json::Value::Null)).unwrap();
        assert!(!ohne.contains("lage"), "{ohne}");
        let Befehl::Tick { tick } = serde_json::from_str::<Befehl>(&ohne).unwrap() else { panic!("kein Takt") };
        assert!(tick.lage.is_null());
    }

    /// Kabinenansagen (Beta), die Kette: Eckdaten in `FlightStats` →
    /// Flugdaten → Planer → faellige Ansage. Fracht bleibt stumm, ein
    /// Linienflug bekommt im Boarding seine Ansage, sobald die Zuladung
    /// steigt, ein Wiedereinstieg im
    /// Reiseflug holt das Boarding nicht nach, und ein Durchstarten zaehlt
    /// als Flanke, nicht bei jedem Takt neu.
    #[test]
    fn kabinen_takt_kette_von_den_stats_zur_ansage() {
        use cabin_announcements::Slot;
        let jetzt = Utc::now();
        let snap = SimSnapshot::default();
        let slots = |f: Vec<cabin_announcements::Faellig>| f.into_iter().map(|f| f.slot).collect::<Vec<_>>();

        let mut fracht = FlightStats::new();
        fracht.phase = FlightPhase::Boarding;
        fracht.pax_count = Some(0);
        assert!(kabinen_schritt(&mut fracht, &snap, jetzt, "EDDH", Some(60), "A320", &[]).faellig.is_empty());

        let mut linie = FlightStats::new();
        linie.phase = FlightPhase::Boarding;
        linie.pax_count = Some(150);
        linie.max_passengers = Some(180);
        linie.flight_type = Some("J".into());
        // 2.1.1 A: Begrüßung 30 s nach dem Anstieg der Zuladung (+200 kg),
        // hier ohne OFP (kein ZFW-Plan) — nicht nach Zeit, nicht am Schalter.
        linie.last_zfw_kg = Some(43_000.0);
        assert!(!slots(kabinen_schritt(&mut linie, &snap, jetzt, "EDDH", Some(60), "A320", &[]).faellig).contains(&Slot::Boarding));
        let spaeter = jetzt + chrono::Duration::minutes(8);
        assert!(!slots(kabinen_schritt(&mut linie, &snap, spaeter, "EDDH", Some(60), "A320", &[]).faellig).contains(&Slot::Boarding));
        linie.last_zfw_kg = Some(46_000.0);
        kabinen_schritt(&mut linie, &snap, spaeter + chrono::Duration::seconds(10), "EDDH", Some(60), "A320", &[]);
        assert!(slots(kabinen_schritt(&mut linie, &snap, spaeter + chrono::Duration::seconds(40), "EDDH", Some(60), "A320", &[]).faellig).contains(&Slot::Boarding));

        let mut resume = FlightStats::new();
        resume.phase = FlightPhase::Cruise;
        resume.pax_count = Some(150);
        resume.max_passengers = Some(180);
        resume.takeoff_at = Some(jetzt - chrono::Duration::minutes(20));
        resume.go_around_count = 1;
        let erste = slots(kabinen_schritt(&mut resume, &snap, jetzt, "EDDH", Some(120), "A320", &[]).faellig);
        assert!(!erste.contains(&Slot::Boarding), "{erste:?}");
        assert_eq!(resume.kabinen_durchstarts, 1, "ein alter Durchstart ist keine neue Flanke");
        resume.go_around_count = 2;
        kabinen_schritt(&mut resume, &snap, jetzt, "EDDH", Some(120), "A320", &[]);
        assert_eq!(resume.kabinen_durchstarts, 2);
    }

    /// Kabinenansagen (Beta): Flugzeit aus dem OFP vor phpVMS, TOD als
    /// Abstand vom Abheben aus dem Navlog, Abheben aus `takeoff_at`.
    #[test]
    fn kabinen_flugdaten_aus_den_stats() {
        let mut stats = FlightStats::new();
        stats.pax_count = Some(6);
        stats.max_passengers = Some(12);
        stats.ofp_planzeiten.plan_abheben = Some(10_000);
        stats.ofp_planzeiten.plan_aufsetzen = Some(10_000 + 95 * 60);
        stats.planned_waypoints = vec![
            api_client::RouteFix { ident: "TOC".into(), zeit_bis_hier_s: Some(1_200.0), ..Default::default() },
            api_client::RouteFix { ident: "TOD".into(), zeit_bis_hier_s: Some(4_500.4), ..Default::default() },
        ];
        stats.planned_cruise_alt_ft = Some(41_000.0);
        let f = kabinen_flugdaten(&stats, " eddm ", Some(70), "C25A");
        assert_eq!(f.flugart, Some(cabin_announcements::Flugart::Bizjet));
        assert_eq!(f.geplante_flugzeit_min, Some(95), "OFP vor phpVMS");
        assert_eq!(f.tod_nach_abheben_s, Some(4_500));
        assert_eq!(f.ziel_plan_icao.as_deref(), Some("EDDM"));
        assert_eq!(f.reiseflughoehe_ft, Some(41_000));
        assert_eq!(f.abheben_epoch, None);

        let ohne_ofp = FlightStats::new();
        let f = kabinen_flugdaten(&ohne_ofp, "EDDM", Some(70), "C25A");
        assert_eq!(f.geplante_flugzeit_min, Some(70), "Rueckfall auf phpVMS");
        assert_eq!(f.flugart, Some(cabin_announcements::Flugart::Aus), "ohne Pax keine Ansagen");
        assert_eq!(f.tod_nach_abheben_s, None);
    }

    /// Umleitung (Beta): Eingabe wird normalisiert, Unsinn abgelehnt, leer
    /// oder das geplante Ziel nimmt die Umleitung zurueck.
    #[test]
    fn umleitung_eingabe_normalisieren() {
        assert_eq!(umleitung_ziel_normalisieren(Some(" lfbd "), "LFBO").unwrap().as_deref(), Some("LFBD"));
        assert_eq!(umleitung_ziel_normalisieren(Some("lfbo"), "LFBO").unwrap(), None);
        assert_eq!(umleitung_ziel_normalisieren(Some(""), "LFBO").unwrap(), None);
        assert_eq!(umleitung_ziel_normalisieren(None, "LFBO").unwrap(), None);
        assert!(umleitung_ziel_normalisieren(Some("LFB"), "LFBO").is_err());
        assert!(umleitung_ziel_normalisieren(Some("LF-D"), "LFBO").is_err());
    }

    /// Umleitung (Beta), die Kette: gemeldetes Ziel → Planer → Ansage, auch
    /// nach einem Neustart (gespeichert) und im Anflug.
    #[test]
    fn umleitung_kommt_als_ansage_und_ueberlebt_einen_resume() {
        use cabin_announcements::Slot;
        let jetzt = Utc::now();
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        let mut stats = FlightStats::new();
        stats.phase = FlightPhase::Approach;
        stats.pax_count = Some(150);
        stats.max_passengers = Some(180);
        stats.takeoff_at = Some(jetzt - chrono::Duration::minutes(90));
        stats.umleitung_ziel = Some("LFBD".into());

        let snapshot = PersistedFlightStats::snapshot_from(&stats);
        let json = serde_json::to_string(&snapshot).expect("serializes");
        let mut nach_neustart = FlightStats::new();
        serde_json::from_str::<PersistedFlightStats>(&json).expect("parses").apply_to(&mut nach_neustart);
        assert_eq!(nach_neustart.umleitung_ziel.as_deref(), Some("LFBD"));

        let slots: Vec<_> = kabinen_schritt(&mut nach_neustart, &snap, jetzt, "LFBO", Some(120), "A320", &[])
            .faellig
            .into_iter()
            .map(|f| f.slot)
            .collect();
        assert!(slots.contains(&Slot::Umleitung), "{slots:?}");

        // Ohne gemeldetes Ziel keine Umleitungs-Ansage.
        let mut ohne = FlightStats::new();
        ohne.phase = FlightPhase::Approach;
        ohne.pax_count = Some(150);
        let slots: Vec<_> = kabinen_schritt(&mut ohne, &snap, jetzt, "LFBO", Some(120), "A320", &[])
            .faellig
            .into_iter()
            .map(|f| f.slot)
            .collect();
        assert!(!slots.contains(&Slot::Umleitung), "{slots:?}");
    }

    /// v1.7.17 Runde 14 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): die BESTAETIGUNG eines erfolgreichen Live-Posts muss
    /// einen Resume ueberleben — sonst saehe ein Restart zwischen
    /// bestaetigtem Post und Boarding-Ende den Wert faelschlich als "nie
    /// gepostet" und wuerde `dep_gate_withdrawal_pending` nie setzen,
    /// obwohl der Server-Wert tatsaechlich von AeroACARS stammt.
    #[test]
    fn dep_gate_field_post_confirmed_ueberlebt_einen_resume() {
        let (flight, _snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.dep_gate_field_post_confirmed = true;
        }
        let snapshot = PersistedFlightStats::snapshot_from(&flight.stats.lock().unwrap());
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        let restored: PersistedFlightStats = serde_json::from_str(&json).expect("snapshot parses");
        let mut fresh = FlightStats::new();
        restored.apply_to(&mut fresh);
        assert!(
            fresh.dep_gate_field_post_confirmed,
            "eine bestaetigte Live-Post-Zustellung muss einen Resume ueberleben"
        );
    }

    /// v1.7.17 Runde 17 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): urspruenglich (Runde 16) sollte die am Boarding-Ende
    /// eingefrorene, noch unbestaetigte Rueckzugsentscheidung einen Resume
    /// UNVERAENDERT ueberleben.
    ///
    /// ⚠ Runde 17 hat diese Annahme UEBERHOLT: die einzige Stelle, die
    /// dieses Flag je in eine echte Rueckzugs-Meldung umsetzt, ist der
    /// Erfolgs-Callback der urspruenglichen Anfrage — und der stirbt mit
    /// dem alten Prozess. Ein blosses Ueberleben des Flags allein loest
    /// also nichts ein; es bliebe ein ewig haengender, nie erledigter
    /// Zustand. Seit Runde 17 wird das Flag beim Resume deshalb bewusst
    /// zurueckgesetzt (mit Protokollzeile) statt weitergetragen — dieser
    /// Test dokumentiert die Korrektur der Korrektur, statt die ueberholte
    /// Zusicherung stillschweigend zu loeschen.
    #[test]
    fn dep_gate_withdrawal_awaiting_confirmation_wird_beim_resume_bewusst_zurueckgesetzt() {
        let (flight, _snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.dep_gate_withdrawal_awaiting_confirmation = true;
        }
        let snapshot = PersistedFlightStats::snapshot_from(&flight.stats.lock().unwrap());
        let json = serde_json::to_string(&snapshot).expect("snapshot serializes");
        let restored: PersistedFlightStats = serde_json::from_str(&json).expect("snapshot parses");
        let mut fresh = FlightStats::new();
        restored.apply_to(&mut fresh);
        assert!(
            !fresh.dep_gate_withdrawal_awaiting_confirmation,
            "eine ueber einen Neustart hinweg nie aufloesbare Entscheidung muss beim \
             Resume zurueckgesetzt werden, nicht ewig haengen bleiben (Runde 17)"
        );
    }

    /// A pre-v0.19.3 snapshot has no `blocks_on_reached` field at all. It must
    /// be derived from the persisted phase, not silently default to "never
    /// blocked on".
    #[test]
    fn a_pre_v0193_snapshot_derives_blocks_on_from_the_phase() {
        let (flight, _snap) = flight_planned_to("EDDF", EDDF_TERMINAL_2);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::BlocksOn;
            stats.blocks_on_reached = true;
        }
        let snapshot = PersistedFlightStats::snapshot_from(&flight.stats.lock().unwrap());
        let mut json: serde_json::Value =
            serde_json::to_value(&snapshot).expect("snapshot serializes");
        // Strip the field, exactly as an old on-disk snapshot would lack it.
        json.as_object_mut().unwrap().remove("blocks_on_reached");

        let restored: PersistedFlightStats =
            serde_json::from_value(json).expect("old snapshot parses");
        let mut fresh = FlightStats::new();
        restored.apply_to(&mut fresh);
        assert!(
            fresh.blocks_on_reached,
            "phase=BlocksOn in an old snapshot means the flight had blocked on"
        );
    }

    /// A stand name only means something together with its airport. If the
    /// aircraft parked at EDDP and the pilot nonetheless files the flight as a
    /// normal EDDF arrival, "Gate A17" (an EDDP stand) must not ride along into
    /// the EDDF PIREP.
    #[test]
    fn a_stand_from_another_airport_never_reaches_the_pirep() {
        let mut stats = FlightStats::new();
        stats.arr_gate = Some("GATE A 17".to_string());
        stats.arr_gate_icao = Some("EDDP".to_string());

        assert_eq!(
            arr_gate_for(&stats, "EDDP").as_deref(),
            Some("GATE A 17"),
            "filed against EDDP — the EDDP stand belongs in the PIREP"
        );
        assert_eq!(
            arr_gate_for(&stats, "EDDF"),
            None,
            "filed against EDDF — an EDDP stand must not appear"
        );

        // A stand captured before v0.19.3 has no airport recorded. We don't
        // know better, so we keep it (old behaviour).
        stats.arr_gate_icao = None;
        assert_eq!(arr_gate_for(&stats, "EDDF").as_deref(), Some("GATE A 17"));
    }

    /// A real divert still works — the fix must not buy a quiet banner by
    /// blinding the detection.
    #[test]
    fn parking_at_eddp_on_a_flight_planned_to_eddf_is_still_a_divert() {
        let (flight, snap) = flight_planned_to("EDDF", EDDP_APRON);
        let result = step_flight(&flight, &snap);

        assert_eq!(result, Some(FlightPhase::Arrived));
        let stats = flight.stats.lock().unwrap();
        let hint = stats
            .divert_hint
            .as_ref()
            .expect("landing at a different field is a divert");
        assert_eq!(hint.actual_icao.as_deref(), Some("EDDP"));
        assert_eq!(hint.planned_arr_icao, "EDDF");
    }
