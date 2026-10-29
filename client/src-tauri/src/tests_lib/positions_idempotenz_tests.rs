    use super::*;

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

    fn punkt(lat: f64) -> api_client::PositionEntry {
        let mut snap = SimSnapshot::default();
        snap.lat = lat;
        snapshot_to_position(&snap)
    }

    #[test]
    fn jeder_erzeugte_punkt_bekommt_eine_eigene_id() {
        let a = punkt(50.0);
        let b = punkt(50.0);
        assert_ne!(a.id, b.id, "zwei Punkte duerfen nie dieselbe id tragen");
        // phpVMS' `acars`.`id` ist varchar(36) — eine UUID passt exakt.
        assert_eq!(a.id.len(), 36, "id muss in varchar(36) passen: {}", a.id);
    }

    fn nachsenden_mit(
        punkte: Vec<api_client::PositionEntry>,
        frist: Duration,
        antworten: Vec<Result<(), ApiError>>,
    ) -> ((usize, usize), Vec<Vec<String>>) {
        let gesendet = std::sync::Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
        let antworten = std::sync::Arc::new(Mutex::new(std::collections::VecDeque::from(antworten)));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        let ergebnis = rt.block_on(positionen_nachsenden(
            punkte.into(),
            frist,
            Duration::from_secs(5),
            50,
            Duration::from_millis(5),
            |batch| {
                let (gesendet, antworten) = (gesendet.clone(), antworten.clone());
                async move {
                    gesendet
                        .lock()
                        .unwrap()
                        .push(batch.iter().map(|p| p.id.clone()).collect());
                    antworten.lock().unwrap().pop_front().unwrap_or(Ok(()))
                }
            },
        ));
        let gesendet = gesendet.lock().unwrap().clone();
        (ergebnis, gesendet)
    }

    #[test]
    fn nach_flugende_gehen_alle_offenen_positionen_raus() {
        // SIA 436 (12.10.2026): Bis 2.0.11 leerte `flight_end` die Outbox —
        // die letzten Sekunden fehlten in der phpVMS-Spur.
        let punkte: Vec<_> = (0..120).map(|i| punkt(i as f64)).collect();
        let ids: Vec<String> = punkte.iter().map(|p| p.id.clone()).collect();
        let ((nachgesendet, offen), gesendet) =
            nachsenden_mit(punkte, Duration::from_secs(5), vec![]);
        assert_eq!((nachgesendet, offen), (120, 0));
        assert!(gesendet.iter().all(|b| b.len() <= 50), "hoechstens 50 je Anfrage");
        assert_eq!(gesendet.concat(), ids, "alle, in Reihenfolge, keine doppelt");
    }

    #[test]
    fn voruebergehender_fehler_wird_wiederholt() {
        let p = punkt(1.0);
        let id = p.id.clone();
        let ((nachgesendet, offen), gesendet) = nachsenden_mit(
            vec![p],
            Duration::from_secs(5),
            vec![Err(ApiError::Network("weg".into())), Ok(())],
        );
        assert_eq!((nachgesendet, offen), (1, 0));
        assert_eq!(gesendet, vec![vec![id.clone()], vec![id]], "dieselbe id erneut");
    }

    #[test]
    fn endgueltige_ablehnung_bricht_ab() {
        let ((nachgesendet, offen), gesendet) = nachsenden_mit(
            vec![punkt(1.0), punkt(2.0)],
            Duration::from_secs(5),
            vec![Err(ApiError::Server {
                status: 400,
                body: "{\"type\":\"pirep-cancelled\"}".into(),
            })],
        );
        assert_eq!((nachgesendet, offen), (0, 2));
        assert_eq!(gesendet.len(), 1, "kein zweiter Versuch nach endgueltiger Ablehnung");
    }

    #[test]
    fn nachsenden_endet_spaetestens_mit_der_frist() {
        let antworten = (0..10_000).map(|_| Err(ApiError::Network("weg".into()))).collect();
        let t0 = std::time::Instant::now();
        let ((nachgesendet, offen), _) =
            nachsenden_mit(vec![punkt(1.0)], Duration::from_millis(100), antworten);
        assert!(t0.elapsed() < Duration::from_secs(2), "toter Server haelt nicht auf");
        assert_eq!((nachgesendet, offen), (0, 1));
    }

    #[test]
    fn startsperre_liefert_die_aktuelle_generation_genau_einmal() {
        let flight = flug();
        assert_eq!(startsperre_nehmen(&flight, &flight.streamer_spawned), Some(0));
        assert_eq!(startsperre_nehmen(&flight, &flight.streamer_spawned), None, "laeuft schon");
        // Fortsetzen ueber den echten Weg: neue Generation, Sperren frei.
        fortsetzen_vorbereiten(&flight);
        assert_eq!(startsperre_nehmen(&flight, &flight.streamer_spawned), Some(1));
    }

    #[test]
    fn stopp_entscheidung_unterscheidet_ende_fortsetzen_und_abbruch() {
        // Normales Flugende (wie `flight_end`): Flag vor stop.
        let flight = flug();
        let mut outbox: std::collections::VecDeque<_> = [punkt(1.0)].into();
        flight.positionen_nachsenden.store(true, Ordering::SeqCst);
        flight.stop.store(true, Ordering::SeqCst);
        assert!(matches!(
            stopp_entscheidung(&flight, &mut outbox, 0),
            StoppEnde::Abgeben(v) if v.len() == 1
        ));

        // Einreichen gescheitert, Flug fortgesetzt: Der alte Worker (Generation 0)
        // ist abgeloest und darf die Outbox des neuen nicht anfassen.
        fortsetzen_vorbereiten(&flight);
        let mut outbox: std::collections::VecDeque<_> = [punkt(2.0)].into();
        flight.stop.store(true, Ordering::SeqCst);
        assert!(matches!(
            stopp_entscheidung(&flight, &mut outbox, 0),
            StoppEnde::Abgeloest
        ));
        assert_eq!(outbox.len(), 1, "nichts entnommen");

        // Abbruch des fortgesetzten Flugs: Generation 1, Flag aus — verwerfen.
        assert!(matches!(
            stopp_entscheidung(&flight, &mut outbox, 1),
            StoppEnde::Verwerfen
        ));
    }

    /// Bis 2.0.11 leerten `flight_end` und `flight_end_manual` die Outbox vor
    /// dem Einreichen — die letzten Sekunden fehlten in der phpVMS-Spur. Ein
    /// Ablauftest braeuchte die laufende App; der Waechter faengt das
    /// Wiedereinfuehren.
    #[test]
    fn flugende_leert_die_outbox_nicht_mehr() {
        let quelle: String = include_str!("../lib.rs")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        // Suchtexte zur Laufzeit zusammengesetzt: Als Literal stuenden sie in
        // diesem Test selbst, und der Waechter faende sich selbst.
        let flag_setzen = ["positionen_nachsenden", ".store(true"].concat();
        // Ohne Leerraum — auch der im Text "position_outbox lock" faellt weg.
        let leeren = [".position_outbox", ".lock()", ".expect(\"position_outboxlock\")", ".clear()"]
            .concat();
        for name in ["flight_end", "flight_end_manual"] {
            // Mit den Parametern, damit nur die Definition trifft — andere
            // Tests fuehren den Funktionskopf als Text.
            let kopf = ["async", "fn", name, "(app:AppHandle,state:tauri::State<'_,AppState>"].concat();
            let start = quelle
                .find(&kopf)
                .unwrap_or_else(|| panic!("{name} nicht gefunden — Waechter anpassen"));
            let rumpf = &quelle[start..];
            let ende = rumpf[1..].find("asyncfn").map_or(rumpf.len(), |i| i + 1);
            let rumpf = &rumpf[..ende];
            assert!(rumpf.contains(&flag_setzen), "{name}: setzt das Nachsende-Flag nicht");
            assert!(!rumpf.contains(&leeren), "{name}: leert die Outbox wieder");
        }
    }

    #[test]
    fn nur_das_normale_flugende_gibt_die_outbox_ab() {
        let flight = flug();
        let mut outbox: std::collections::VecDeque<_> = [punkt(1.0), punkt(2.0)].into();
        // Laufender Flug: nichts abgeben.
        assert!(outbox_zum_nachsenden(&flight, &mut outbox).is_none());
        // Abbruch/Verwerfen: Stopp ohne Flag — ebenfalls nichts.
        flight.stop.store(true, Ordering::SeqCst);
        assert!(outbox_zum_nachsenden(&flight, &mut outbox).is_none());
        assert_eq!(outbox.len(), 2);
        // Normales Flugende: alles heraus, die Outbox ist danach leer.
        flight.positionen_nachsenden.store(true, Ordering::SeqCst);
        assert_eq!(outbox_zum_nachsenden(&flight, &mut outbox).map(|v| v.len()), Some(2));
        assert!(outbox.is_empty());
    }

    #[test]
    fn requeue_erhaelt_ids_und_reihenfolge() {
        let flight = flug();
        let batch = vec![punkt(1.0), punkt(2.0), punkt(3.0)];
        let ids: Vec<String> = batch.iter().map(|p| p.id.clone()).collect();

        requeue_batch(&flight, batch);

        let outbox = flight.position_outbox.lock().unwrap();
        let danach: Vec<String> = outbox.iter().map(|p| p.id.clone()).collect();
        assert_eq!(
            danach, ids,
            "der Requeue muss die IDs unveraendert und in Originalreihenfolge zurueckgeben"
        );
    }

    #[test]
    fn requeue_stellt_den_batch_vor_neu_eingetroffene_punkte() {
        let flight = flug();
        let spaeter = punkt(9.0);
        let spaeter_id = spaeter.id.clone();
        flight.position_outbox.lock().unwrap().push_back(spaeter);

        let batch = vec![punkt(1.0), punkt(2.0)];
        let batch_ids: Vec<String> = batch.iter().map(|p| p.id.clone()).collect();
        requeue_batch(&flight, batch);

        let outbox = flight.position_outbox.lock().unwrap();
        let danach: Vec<String> = outbox.iter().map(|p| p.id.clone()).collect();
        assert_eq!(danach, [batch_ids, vec![spaeter_id]].concat());
    }

    #[test]
    fn cruise_laesst_nur_einen_punkt_je_intervall_durch() {
        let p = FlightPhase::Cruise;
        assert!(
            phpvms_punkt_faellig(p, Some(Duration::from_secs(3)), Some(p)) == false,
            "3 s nach dem letzten Punkt darf im Cruise keiner rausgehen"
        );
        assert!(phpvms_punkt_faellig(
            p,
            Some(phpvms_punkt_intervall(p)),
            Some(p)
        ));
    }

    #[test]
    fn phasenwechsel_geht_immer_raus() {
        // Sonst faellt der Touchdown in ein laufendes Intervall und fehlt
        // auf der Karte.
        assert!(phpvms_punkt_faellig(
            FlightPhase::Landing,
            Some(Duration::from_millis(200)),
            Some(FlightPhase::Final),
        ));
    }

    #[test]
    fn erster_punkt_geht_immer_raus() {
        assert!(phpvms_punkt_faellig(FlightPhase::Boarding, None, None));
    }

    #[test]
    fn anflug_bleibt_feiner_als_reiseflug() {
        assert!(
            phpvms_punkt_intervall(FlightPhase::Final)
                < phpvms_punkt_intervall(FlightPhase::Cruise),
            "der Anflug muss dichter aufgeloest bleiben als der Reiseflug"
        );
    }

    /// Der Grund fuer die eigene Tabelle: am Boden muss der Weg erkennbar
    /// bleiben. Die POST-Kadenz gibt `TakeoffRoll` 10 s — das waeren im
    /// Startlauf ~770 m zwischen zwei Punkten.
    #[test]
    fn am_boden_bleibt_es_fein() {
        for phase in [
            FlightPhase::TaxiOut,
            FlightPhase::TakeoffRoll,
            FlightPhase::TaxiIn,
            FlightPhase::Pushback,
        ] {
            assert!(
                phpvms_punkt_intervall(phase) <= Duration::from_secs(5),
                "{phase:?} muss am Boden hoechstens 5 s Abstand haben"
            );
        }
    }

    #[test]
    fn nur_der_reiseflug_geht_auf_60_sekunden() {
        assert_eq!(
            phpvms_punkt_intervall(FlightPhase::Cruise),
            Duration::from_secs(60)
        );
        for phase in [
            FlightPhase::Climb,
            FlightPhase::Descent,
            FlightPhase::Approach,
            FlightPhase::Final,
            FlightPhase::Landing,
            FlightPhase::TaxiIn,
        ] {
            assert!(
                phpvms_punkt_intervall(phase) < Duration::from_secs(60),
                "{phase:?} darf nicht auf den Reiseflug-Wert fallen"
            );
        }
    }

    #[test]
    fn id_ueberlebt_den_persistenz_umweg() {
        // Der Worker schreibt die Outbox bei Backlog/Stop nach
        // `position_queue.json` und liest sie beim naechsten Start zurueck.
        // Genau dort darf die id nicht verloren gehen — sonst waere ein
        // Flug nach App-Neustart wieder unbeschuetzt.
        let original = punkt(51.5);
        let als_json = serde_json::to_value(&original).expect("serialisieren");
        let zurueck: api_client::PositionEntry =
            serde_json::from_value(als_json).expect("deserialisieren");
        assert_eq!(zurueck.id, original.id);
    }
