    //! DLH 41 UUDD–EDDK (10.11.2026, PIREP r3Y9WGojGbk7DgX7): In 9 700 ft,
    //! 55 NM vor dem Ziel, sprang der Tank in einem Takt von 1 384 auf
    //! 22 146 kg. Der Client rechnete „Startsprit − Landesprit" und meldete
    //! 2 139 kg Verbrauch fuer einen Flug, der rund 22 300 kg gekostet hat.
    //! Geprueft an `step_flight_at` mit den Messwerten aus dem Flugprotokoll.
    use super::*;

    fn zeit(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// Der Flug kurz vor dem Sprung: Startsprit und Blocksprit wie gemessen.
    fn dlh41() -> ActiveFlight {
        let flight = crate::enroute_reconcile_replay_tests::replay_fixture();
        {
            let mut st = flight.stats.lock().unwrap();
            st.phase = FlightPhase::Descent;
            st.block_off_at = Some(zeit("2026-11-10T15:40:00Z"));
            st.takeoff_at = Some(zeit("2026-11-10T15:58:00Z"));
            st.block_fuel_kg = Some(22_145.973);
            st.takeoff_fuel_kg = Some(21_637.6);
            st.was_airborne = true;
            st.last_fuel_kg = Some(1_394.8253);
            st.last_pos_at = Some(zeit("2026-11-10T19:20:00.475Z"));
            st.nachtanken_luft.stand_kg = Some(1_394.8253);
            st.nachtanken_luft.stand_am = Some(zeit("2026-11-10T19:20:00.475Z"));
        }
        flight
    }

    fn in_der_luft(fuel_kg: f32) -> SimSnapshot {
        SimSnapshot {
            on_ground: false,
            altitude_agl_ft: 9_500.0,
            altitude_msl_ft: 9_700.0,
            vertical_speed_fpm: -800.0,
            groundspeed_kt: 280.0,
            fuel_total_kg: fuel_kg,
            ..SimSnapshot::default()
        }
    }

    /// Takte im Drei-Sekunden-Abstand ab 19:20:03 Z.
    fn takte(flight: &ActiveFlight, werte: &[f32]) -> DateTime<Utc> {
        let mut t = zeit("2026-11-10T19:20:00Z");
        for kg in werte {
            t += chrono::Duration::seconds(3);
            step_flight_at(flight, &in_der_luft(*kg), t);
        }
        t
    }

    /// Die Takte aus dem Flugprotokoll rund um 19:20:06 Z, danach weiter mit
    /// dem gemessenen Verbrauch von rund 6,7 kg je Takt.
    const DLH41_SPRUNG: [f32; 7] = [
        1_390.4783, 1_383.9695, 22_145.973, 22_139.467, 22_132.68, 22_125.482, 22_118.7,
    ];

    /// Ein nackter Zaehler fuer die Regeln ohne den ganzen Takt.
    fn zaehler(stand_kg: f32) -> (NachtankenLuft, DateTime<Utc>) {
        let t = zeit("2026-11-10T19:00:00Z");
        let n = NachtankenLuft {
            stand_kg: Some(stand_kg),
            stand_am: Some(t),
            ..NachtankenLuft::default()
        };
        (n, t)
    }

    /// Werte im Drei-Sekunden-Abstand in den Zaehler geben.
    fn fuettern(n: &mut NachtankenLuft, t: &mut DateTime<Utc>, werte: &[f32]) {
        for kg in werte {
            *t += chrono::Duration::seconds(3);
            n.takt(*kg, 1.0, *t);
        }
    }

    #[test]
    fn dlh41_sprung_wird_als_nachtanken_gezaehlt() {
        let flight = dlh41();
        // Im Takt des Sprungs und kurz danach zaehlt noch nichts: Der
        // Anstieg muss erst stehen.
        takte(&flight, &DLH41_SPRUNG[..4]);
        assert_eq!(nachgetankt_luft(&flight.stats.lock().unwrap()), 0.0);
        let flight = dlh41();
        takte(&flight, &DLH41_SPRUNG);
        let mut st = flight.stats.lock().unwrap();
        assert!(
            (nachgetankt_luft(&st) - 20_762.0).abs() < 0.1,
            "nachgetankt: {}",
            nachgetankt_luft(&st)
        );
        assert_eq!(st.nachtanken_luft.zuerst_am, Some(zeit("2026-11-10T19:20:09Z")));
        // Der Resttank davor ist der letzte Wert VOR dem Sprung.
        assert_eq!(st.nachtanken_luft.tank_davor_kg, Some(1_383.9695));
        // Der Blocksprit der Flugdaten bleibt der Wert vom Anlassen.
        assert_eq!(st.block_fuel_kg, Some(22_145.973));
        assert!(st.nachtanken_luft.geaendert, "der Streamer speichert sofort");

        // Landung und Abstellen wie gemessen.
        st.landing_fuel_kg = Some(20_083.8);
        st.last_fuel_kg = Some(20_007.0);
        let verbrauch = actual_burn_for_record(&st).expect("Verbrauch");
        assert!((verbrauch - 22_315.8).abs() < 1.0, "Verbrauch: {verbrauch}");
        // +41 % gegen den Plan statt „sehr effizient".
        st.planned_burn_kg = Some(15_827.0);
        let pct = trip_burn_efficiency_pct(
            startsprit_mit_nachtanken(&st),
            st.landing_fuel_kg,
            st.planned_burn_kg,
        )
        .unwrap();
        assert!((pct - 41.0).abs() < 0.5, "Abweichung: {pct}");

        // Nach aussen: Block und Verbrauch beide mit der nachgetankten Menge
        // — `block − fuel_used` bleibt der echte Resttank.
        let block = blocksprit_mit_nachtanken(&st).unwrap();
        let used = verbrauch_ab_block_kg(&st).unwrap();
        assert!((block - 42_908.0).abs() < 0.5, "Block: {block}");
        assert!((used - 22_901.0).abs() < 0.5, "Verbrauch ab Block: {used}");
        assert!((block - used - 20_007.0).abs() < 0.01);
        // Die Detail-Auswertung (Phasen, Reserve) ist nach einem Sprung nicht
        // ehrlich zuzuordnen.
        assert!(!sprit_tank_plausibel(&st));
    }

    /// Paritaet: PIREP-Felder, Notiz und Lebenszeichen an phpVMS nennen
    /// denselben Block und denselben Verbrauch (Codex-Fund 10.11.2026: MQTT
    /// und Felder trugen den alten Block neben dem neuen Verbrauch).
    #[test]
    fn dlh41_block_und_verbrauch_ueberall_gleich() {
        let flight = dlh41();
        takte(&flight, &DLH41_SPRUNG);
        let mut st = flight.stats.lock().unwrap();
        st.landing_fuel_kg = Some(20_083.8);
        st.last_fuel_kg = Some(20_007.0);
        let felder = build_pirep_fields_mit(&flight, &st, "EDDK", None);
        assert_eq!(felder.get("Block Fuel").map(String::as_str), Some("42908 kg"));
        assert_eq!(felder.get("Fuel Used").map(String::as_str), Some("22901 kg"));
        assert_eq!(felder.get("Refuelled In Flight").map(String::as_str), Some("20762 kg"));
        assert_eq!(felder.get("Fuel Before Refuelling").map(String::as_str), Some("1384 kg"));
        let satz = blocksprit_mit_nachtanken(&st).unwrap();
        assert!((satz - 42_908.0).abs() < 0.5);
    }

    /// Gegenprobe: derselbe Flug ohne Sprung zaehlt nichts, und alle Werte
    /// bleiben die alten.
    #[test]
    fn ohne_sprung_kein_nachtanken() {
        let flight = dlh41();
        takte(&flight, &[1_390.4783, 1_383.9695, 1_377.4, 1_370.9, 1_364.3, 1_357.8, 1_351.2]);
        let st = flight.stats.lock().unwrap();
        assert_eq!(nachgetankt_luft(&st), 0.0);
        assert_eq!(st.nachtanken_luft.saldo_kg, 0.0);
        assert_eq!(st.nachtanken_luft.zuerst_am, None);
        assert!(!st.nachtanken_luft.geaendert);
        assert_eq!(startsprit_mit_nachtanken(&st), st.takeoff_fuel_kg);
        assert_eq!(blocksprit_mit_nachtanken(&st), st.block_fuel_kg);
    }

    /// Wer mit wenig startet, nachtankt und mit mehr landet, als er beim
    /// Start hatte, bekommt trotzdem einen Verbrauch.
    #[test]
    fn landesprit_ueber_startsprit_rechnet_mit_der_summe() {
        let mut st = FlightStats::default();
        st.takeoff_fuel_kg = Some(5_000.0);
        st.block_fuel_kg = Some(5_200.0);
        st.nachtanken_luft.saldo_kg = 18_000.0;
        st.landing_fuel_kg = Some(20_000.0);
        st.last_fuel_kg = Some(19_900.0);
        assert_eq!(actual_burn_for_record(&st), Some(3_000.0));
        assert_eq!(verbrauch_ab_block_kg(&st), Some(3_300.0));
        // Ohne Nachtanken bleibt der alte Schutz: kein erfundener Verbrauch.
        st.nachtanken_luft.saldo_kg = 0.0;
        assert_eq!(actual_burn_for_record(&st), None);
        assert_eq!(verbrauch_ab_block_kg(&st), None);
        // Ein negativer Saldo (Sprit verschwunden) zaehlt nie als Nachtanken.
        st.nachtanken_luft.saldo_kg = -4_000.0;
        assert_eq!(nachgetankt_luft(&st), 0.0);
        assert_eq!(blocksprit_mit_nachtanken(&st), Some(5_200.0));
    }

    /// Langsames Tanken in kleinen Schritten zaehlt (Codex-Fund: je Takt
    /// unter 50 kg wurde es vollstaendig uebersehen).
    #[test]
    fn langsames_nachtanken_zaehlt() {
        let (mut n, mut t) = zaehler(5_000.0);
        let werte: Vec<f32> = (1..=100).map(|i| 5_000.0 + 40.0 * i as f32).collect();
        fuettern(&mut n, &mut t, &werte);
        // Danach normaler Verbrauch, bis auch der letzte Anstieg steht.
        fuettern(&mut n, &mut t, &[8_994.0, 8_988.0, 8_982.0, 8_976.0, 8_970.0]);
        assert!(n.menge_kg() > 3_900.0 && n.menge_kg() <= 4_000.0, "{}", n.menge_kg());
        assert_eq!(n.offen_kg, 0.0);
        assert_eq!(n.tank_davor_kg, Some(5_000.0));
    }

    /// Ein Sensor-Aussetzer ist kein Tanken — nicht bei null, nicht bei einem
    /// falschen Wert, nicht bei unvollstaendiger Rueckkehr und egal wie lange.
    #[test]
    fn sensor_aussetzer_zaehlt_nicht() {
        // Tank liest zwei Minuten lang 0, dann wieder den echten Wert.
        let (mut n, mut t) = zaehler(12_000.0);
        fuettern(&mut n, &mut t, &[11_994.0]);
        fuettern(&mut n, &mut t, &[0.0; 40]);
        fuettern(&mut n, &mut t, &[11_700.0, 11_694.0, 11_688.0, 11_682.0, 11_676.0]);
        assert_eq!(n.menge_kg(), 0.0);
        assert_eq!(n.saldo_kg, 0.0, "Werte ≤ 0 hinterlassen nichts");

        // Codex-Fund: 10 000 → 1 000 → 8 500 zaehlte 7 500 kg.
        let (mut n, mut t) = zaehler(10_000.0);
        fuettern(&mut n, &mut t, &[1_000.0, 995.0, 8_500.0, 8_494.0, 8_488.0, 8_482.0, 8_476.0]);
        assert_eq!(n.menge_kg(), 0.0);
        assert!(n.saldo_kg < -1_000.0, "der Rest ist verschwundener Sprit: {}", n.saldo_kg);
        assert_eq!(n.zuerst_am, None);

        // Voller Ruecksprung: Der Saldo steht wieder bei null, ein spaeteres
        // echtes Nachtanken zaehlt ganz.
        let (mut n, mut t) = zaehler(10_000.0);
        fuettern(&mut n, &mut t, &[3_000.0, 2_995.0, 9_990.0, 9_984.0, 9_978.0, 9_972.0, 9_966.0]);
        assert!(n.saldo_kg.abs() < 6.0, "{}", n.saldo_kg);
        fuettern(&mut n, &mut t, &[15_000.0, 14_994.0, 14_988.0, 14_982.0, 14_976.0]);
        assert!((n.menge_kg() - 5_034.0).abs() < 6.0, "{}", n.menge_kg());
    }

    /// Ein Ausschlag nach oben, der gleich wieder zurueckfaellt, ist kein
    /// Tanken (Codex-Fund: 10 000 → 10 060 → 10 000 blieb gezaehlt).
    #[test]
    fn ausschlag_nach_oben_zaehlt_nicht() {
        let (mut n, mut t) = zaehler(10_000.0);
        fuettern(&mut n, &mut t, &[10_060.0, 10_000.0, 9_994.0, 9_988.0, 9_982.0, 9_976.0]);
        assert_eq!(n.menge_kg(), 0.0);
        assert_eq!(n.offen_kg, 0.0);
        // Grosser Ausschlag: +8 000 und im naechsten Takt wieder weg.
        let (mut n, mut t) = zaehler(10_000.0);
        fuettern(&mut n, &mut t, &[18_000.0, 10_000.0, 9_994.0, 9_988.0, 9_982.0, 9_976.0]);
        assert_eq!(n.menge_kg(), 0.0);
        assert_eq!(n.saldo_kg, 0.0, "hebt sich restlos auf");
        // Teilweise zurueck: Es zaehlt, was geblieben ist.
        let (mut n, mut t) = zaehler(10_000.0);
        fuettern(&mut n, &mut t, &[18_000.0, 12_000.0, 11_994.0, 11_988.0, 11_982.0, 11_976.0]);
        assert_eq!(n.menge_kg(), 2_000.0);
        // Rauschen der Anzeige um wenige Kilo.
        let (mut n, mut t) = zaehler(5_000.0);
        fuettern(&mut n, &mut t, &[5_030.0, 4_990.0, 5_020.0, 4_985.0, 5_015.0, 4_980.0, 4_975.0, 4_970.0]);
        assert_eq!(n.menge_kg(), 0.0);
        assert_eq!(n.saldo_kg, 0.0);
    }

    /// Normaler Verbrauch hinterlaesst nichts im Saldo — auch nicht ueber
    /// eine lange Luecke und nicht bei einer Anzeige, die in Treppen faellt.
    #[test]
    fn verbrauch_ist_kein_einbruch() {
        // Zwei Minuten ohne Takt, 600 kg weniger.
        let (mut n, t) = zaehler(10_000.0);
        n.takt(9_400.0, 1.0, t + chrono::Duration::seconds(120));
        assert_eq!(n.saldo_kg, 0.0);
        // Dieselbe Abnahme in drei Sekunden verbrennt kein Triebwerk.
        let (mut n, t) = zaehler(10_000.0);
        n.takt(9_400.0, 1.0, t + chrono::Duration::seconds(3));
        assert_eq!(n.saldo_kg, -600.0);
        // Treppe: 30 s lang derselbe Wert, dann 100 kg weniger in einem Takt.
        let (mut n, mut t) = zaehler(10_000.0);
        for stufe in 1..=20 {
            let kg = 10_000.0 - 100.0 * stufe as f32;
            fuettern(&mut n, &mut t, &[kg; 10]);
        }
        assert_eq!(n.saldo_kg, 0.0);
        // Erster Takt ohne Vergleichsstand: keine Aussage.
        let mut n = NachtankenLuft::default();
        assert_eq!(n.takt(5_000.0, 1.0, zeit("2026-11-10T19:00:00Z")), 0.0);
        assert_eq!(n.stand_kg, Some(5_000.0));
    }

    /// Tanken in der Pause (Sim-Menue) ist Tanken: Der Sprit ist an Bord.
    #[test]
    fn nachtanken_in_der_pause_zaehlt() {
        let flight = dlh41();
        let mut t = zeit("2026-11-10T19:20:00Z");
        for kg in [9_000.0, 9_000.0, 9_000.0, 9_000.0, 9_000.0] {
            t += chrono::Duration::seconds(3);
            let mut pausiert = in_der_luft(kg);
            pausiert.paused = true;
            step_flight_at(&flight, &pausiert, t);
        }
        step_flight_at(&flight, &in_der_luft(8_998.0), t + chrono::Duration::seconds(3));
        let st = flight.stats.lock().unwrap();
        assert!((nachgetankt_luft(&st) - 7_605.1747).abs() < 0.1, "{}", nachgetankt_luft(&st));
    }

    /// Fortsetzen nach einem Sim-Absturz in der Luft: Menge und Hinweisstand
    /// ueberleben den Neustart, und der erste Takt vergleicht mit dem
    /// gespeicherten Stand.
    #[test]
    fn fortsetzen_behaelt_und_erkennt() {
        let flight = dlh41();
        takte(&flight, &DLH41_SPRUNG);
        let gespeichert = {
            let mut st = flight.stats.lock().unwrap();
            assert!(st.nachtanken_luft.hinweis_faellig().is_some());
            serde_json::to_string(&PersistedFlightStats::snapshot_from(&st)).unwrap()
        };
        let geladen: PersistedFlightStats = serde_json::from_str(&gespeichert).unwrap();
        let mut st = FlightStats::default();
        geladen.apply_to(&mut st);
        assert!((nachgetankt_luft(&st) - 20_762.0).abs() < 0.1);
        assert_eq!(st.nachtanken_luft.tank_davor_kg, Some(1_383.9695));
        assert_eq!(st.nachtanken_luft.stand_kg, Some(22_118.7));
        assert_eq!(st.nachtanken_luft.hinweis_faellig(), None, "kein zweiter Hinweis nach dem Neustart");
        // Altbestand ohne das Feld bleibt lesbar.
        let mut alt: serde_json::Value = serde_json::from_str(&gespeichert).unwrap();
        assert!(alt.as_object_mut().unwrap().remove("nachtanken_luft").is_some());
        let alt: PersistedFlightStats = serde_json::from_value(alt).unwrap();
        let mut st = FlightStats::default();
        alt.apply_to(&mut st);
        assert_eq!(st.nachtanken_luft, NachtankenLuft::default());

        // Neu geladener Sim mit 5 000 kg mehr als beim Absturz.
        let flight = dlh41();
        let mut t = zeit("2026-11-10T19:45:00Z");
        for kg in [6_394.8253, 6_388.0, 6_382.0, 6_376.0, 6_370.0] {
            step_flight_at(&flight, &in_der_luft(kg), t);
            t += chrono::Duration::seconds(3);
        }
        let n = nachgetankt_luft(&flight.stats.lock().unwrap());
        assert!((n - 5_000.0).abs() < 0.01, "{n}");
        // Stellt der Pilot den Tank gleich wieder auf den alten Wert, bleibt nichts.
        let flight = dlh41();
        let mut t = zeit("2026-11-10T19:45:00Z");
        for kg in [6_394.8253, 1_394.0, 1_388.0, 1_382.0, 1_376.0, 1_370.0] {
            step_flight_at(&flight, &in_der_luft(kg), t);
            t += chrono::Duration::seconds(3);
        }
        assert_eq!(nachgetankt_luft(&flight.stats.lock().unwrap()), 0.0);
    }

    /// Fall Joel (24.09.2026) bleibt: Tanken am Boden nach der Landung ist
    /// kein Nachtanken in der Luft und aendert den Verbrauch nicht.
    #[test]
    fn tanken_am_boden_nach_der_landung_bleibt_aussen_vor() {
        let flight = dlh41();
        {
            let mut st = flight.stats.lock().unwrap();
            st.phase = FlightPhase::TaxiIn;
            st.landing_fuel_kg = Some(1_300.0);
            st.landing_at = Some(zeit("2026-11-10T19:30:00Z"));
            st.last_fuel_kg = Some(1_250.0);
        }
        let mut t = zeit("2026-11-10T19:50:00Z");
        for _ in 0..8 {
            let mut am_boden = in_der_luft(9_546.0);
            am_boden.on_ground = true;
            am_boden.altitude_agl_ft = 0.0;
            am_boden.groundspeed_kt = 0.0;
            step_flight_at(&flight, &am_boden, t);
            t += chrono::Duration::seconds(3);
        }
        let st = flight.stats.lock().unwrap();
        assert_eq!(nachgetankt_luft(&st), 0.0);
        assert_eq!(st.nachtanken_luft.offen_kg, 0.0);
        assert_eq!(st.landing_fuel_kg, Some(1_300.0));
        assert_eq!(actual_burn_for_record(&st), Some(21_637.6 - 1_300.0));
    }

    /// Wann gezaehlt wird — und dass ein Durchstart nichts verliert
    /// (Codex-Fund: Im ersten Lufttakt nach dem Aufsetzen stand der
    /// Landesprit noch, der Anstieg ging unter).
    #[test]
    fn nur_in_der_luft_und_durchstart_verliert_nichts() {
        let mut st = FlightStats::default();
        assert!(!nachtanken_luft_moeglich(&st, false), "vor dem Abheben nie");
        st.takeoff_at = Some(Utc::now());
        assert!(nachtanken_luft_moeglich(&st, false));
        assert!(!nachtanken_luft_moeglich(&st, true), "am Boden nie");
        st.replay_verdacht = true;
        assert!(!nachtanken_luft_moeglich(&st, false), "Replay zaehlt nicht");
        st.replay_verdacht = false;

        let mut t = zeit("2026-11-10T19:00:00Z");
        let mut takt = |st: &mut FlightStats, kg: f32, am_boden: bool| {
            t += chrono::Duration::seconds(3);
            nachtanken_luft_takt(st, kg, am_boden, 1.0, t);
        };
        takt(&mut st, 2_010.0, false);
        // Aufsetzen: Landesprit steht, es wird nicht gezaehlt.
        st.landing_fuel_kg = Some(2_000.0);
        takt(&mut st, 2_000.0, true);
        // Wieder in der Luft, Tank gestiegen, Landesprit steht noch.
        takt(&mut st, 6_000.0, false);
        assert_eq!(st.nachtanken_luft.stand_kg, Some(2_000.0), "der Vergleichsstand bleibt in der Luft stehen");
        // Der Durchstart raeumt den Landesprit ab — jetzt faellt der Anstieg auf.
        st.landing_fuel_kg = None;
        for kg in [5_990.0, 5_980.0, 5_970.0, 5_960.0, 5_950.0] {
            takt(&mut st, kg, false);
        }
        assert_eq!(nachgetankt_luft(&st), 3_990.0);
    }

    /// Advisor-Fund (blockierend): Am Boden getankter Sprit ist nach dem
    /// naechsten Start kein Nachtanken in der Luft. GAF 278 („Vom Boden
    /// weiterfliegen"): im Sinkflug mit 5 000 kg abgestuerzt, am Boden mit
    /// 11 700 kg neu geladen, von dort gestartet.
    #[test]
    fn am_boden_getankt_ist_nach_dem_start_kein_nachtanken() {
        use super::weiterflug_vom_boden_tests as w;
        let flight = w::sinkflug();
        let mut t = zeit("2026-10-24T14:00:00Z");
        let mut schritt = |snap: &SimSnapshot| {
            t += chrono::Duration::seconds(3);
            step_flight_at(&flight, snap, t);
            t
        };
        let mut im_sinkflug = in_der_luft(5_000.0);
        schritt(&im_sinkflug);
        im_sinkflug.fuel_total_kg = 4_995.0;
        let jetzt = schritt(&im_sinkflug);
        {
            let mut st = flight.stats.lock().unwrap();
            assert_eq!(st.nachtanken_luft.stand_kg, Some(4_995.0));
            bodenneustart_vorbereiten(&mut st, jetzt);
        }
        let mut am_boden = w::kalt_am_boden();
        am_boden.fuel_total_kg = 11_700.0;
        schritt(&am_boden);
        schritt(&am_boden);
        let mut ab = w::abheben();
        for _ in 0..8 {
            schritt(&ab);
            ab.fuel_total_kg -= 5.0;
            ab.altitude_agl_ft += 100.0;
        }
        let st = flight.stats.lock().unwrap();
        assert_eq!(nachgetankt_luft(&st), 0.0);
        assert_eq!(st.nachtanken_luft.offen_kg, 0.0);
        let mut n = st.nachtanken_luft;
        assert_eq!(n.hinweis_faellig(), None);
    }

    /// Zwischenlandung: aufgesetzt, am Boden getankt, wieder abgehoben. Der
    /// Stand folgt am Boden dem Tank; nach dem Start zaehlt nichts.
    #[test]
    fn tanken_am_boden_und_wiederabheben_zaehlt_nicht() {
        let mut st = FlightStats::default();
        st.takeoff_at = Some(zeit("2026-11-10T18:00:00Z"));
        let mut t = zeit("2026-11-10T19:00:00Z");
        let mut takt = |st: &mut FlightStats, kg: f32, am_boden: bool| {
            t += chrono::Duration::seconds(3);
            nachtanken_luft_takt(st, kg, am_boden, 1.0, t);
        };
        takt(&mut st, 2_010.0, false);
        st.landing_fuel_kg = Some(2_000.0);
        takt(&mut st, 2_000.0, true);
        takt(&mut st, 9_000.0, true);
        takt(&mut st, 9_000.0, true);
        // Abgehoben, der Landesprit steht noch, dann raeumt der Start ihn ab.
        takt(&mut st, 8_990.0, false);
        st.landing_fuel_kg = None;
        for kg in [8_980.0, 8_970.0, 8_960.0, 8_950.0, 8_940.0] {
            takt(&mut st, kg, false);
        }
        assert_eq!(nachgetankt_luft(&st), 0.0);
        assert_eq!(st.nachtanken_luft.saldo_kg, 0.0);
    }

    /// Feldmessung 10.11.2026 (gsg/4 lJPy…): Sim am Gate neu geladen, der
    /// erste Takt meldet „nicht am Boden" bei 11 ft und 0 kt, Tank 5 822 →
    /// 11 800 kg. Das ist Tanken am Boden.
    #[test]
    fn neu_geladen_am_gate_mit_schwebendem_takt_zaehlt_nicht() {
        let flight = dlh41();
        {
            let mut st = flight.stats.lock().unwrap();
            st.nachtanken_luft.stand_kg = Some(5_822.0);
            st.last_fuel_kg = Some(5_822.0);
        }
        let mut t = zeit("2026-11-10T19:36:00Z");
        let mut schwebt = SimSnapshot {
            on_ground: false,
            altitude_agl_ft: 11.0,
            groundspeed_kt: 0.0,
            fuel_total_kg: 11_800.0,
            ..SimSnapshot::default()
        };
        assert!(nachtanken_am_boden(&schwebt));
        step_flight_at(&flight, &schwebt, t);
        schwebt.on_ground = true;
        for _ in 0..8 {
            t += chrono::Duration::seconds(3);
            step_flight_at(&flight, &schwebt, t);
        }
        let st = flight.stats.lock().unwrap();
        assert_eq!(nachgetankt_luft(&st), 0.0);
        assert_eq!(st.nachtanken_luft.offen_kg, 0.0);
        assert_eq!(st.nachtanken_luft.stand_kg, Some(11_800.0));
        // Gegenprobe: In der Luft ist dieselbe Lage keine Bodenlage.
        assert!(!nachtanken_am_boden(&in_der_luft(11_800.0)));
        let tief_und_schnell = SimSnapshot { altitude_agl_ft: 20.0, groundspeed_kt: 140.0, ..in_der_luft(1.0) };
        assert!(!nachtanken_am_boden(&tief_und_schnell));
    }

    /// Feldmessung 10.11.2026 (gsg/30 E4x7…): Im Startlauf fiel die Anzeige
    /// um 144 kg und kam eine Sekunde spaeter zurueck. Kein Tanken.
    #[test]
    fn abfall_der_gleich_zurueckkommt_ist_kein_tanken() {
        let (mut n, t) = zaehler(47_805.0);
        let s = |sek: i64| t + chrono::Duration::seconds(sek);
        n.takt(47_662.0, 1.0, s(23));
        n.takt(47_808.0, 1.0, s(24));
        let mut kg = 47_808.0;
        for i in 0..10 {
            kg -= 27.0;
            n.takt(kg, 1.0, s(27 + 3 * i));
        }
        assert_eq!(n.menge_kg(), 0.0);
        assert_eq!(n.saldo_kg, 0.0);
        assert_eq!(n.offen_kg, 0.0);
        // Gegenprobe: Ein deutlich groesserer Anstieg nach demselben Abfall
        // ist Tanken und zaehlt ganz …
        let (mut n, t) = zaehler(47_805.0);
        let s = |sek: i64| t + chrono::Duration::seconds(sek);
        n.takt(47_662.0, 1.0, s(23));
        n.takt(52_662.0, 1.0, s(24));
        n.takt(52_660.0, 1.0, s(36));
        assert_eq!(n.menge_kg(), 5_000.0);
        // … und dieselbe Rueckkehr nach mehr als 30 s auch.
        let (mut n, t) = zaehler(47_805.0);
        let s = |sek: i64| t + chrono::Duration::seconds(sek);
        n.takt(47_662.0, 1.0, s(23));
        n.takt(47_808.0, 1.0, s(60));
        n.takt(47_806.0, 1.0, s(72));
        assert_eq!(n.menge_kg(), 146.0);
    }

    /// Advisor-Fund: Im Zeitraffer ist normaler Verbrauch kein Einbruch.
    /// 16-fach, 2,5 kg/s Sim-Verbrauch = 120 kg je Drei-Sekunden-Takt; ein
    /// spaeteres Nachtanken zaehlt danach ganz.
    #[test]
    fn zeitraffer_verbrauch_ist_kein_einbruch() {
        let (mut n, mut t) = zaehler(30_000.0);
        let mut kg = 30_000.0;
        for _ in 0..50 {
            kg -= 120.0;
            t += chrono::Duration::seconds(3);
            n.takt(kg, 16.0, t);
        }
        assert_eq!(n.saldo_kg, 0.0, "Zeitraffer-Verbrauch darf nicht als Einbruch gebucht werden");
        // Gegenprobe: derselbe Verlauf in Echtzeit waere ein Einbruch.
        let (mut e, mut te) = zaehler(30_000.0);
        te += chrono::Duration::seconds(3);
        e.takt(29_880.0, 1.0, te);
        assert_eq!(e.saldo_kg, -120.0);
        // Zurueck auf 1-fach und 5 000 kg nachgetankt: zaehlt ganz.
        let davor = kg;
        for i in 0..6 {
            t += chrono::Duration::seconds(3);
            n.takt(davor + 5_000.0 - 7.0 * i as f32, 1.0, t);
        }
        assert_eq!(n.menge_kg(), 5_000.0);
        // Der Takt, in dem die Rate von 16 auf 1 faellt, gilt noch als 16-fach.
        let (mut w, mut tw) = zaehler(30_000.0);
        tw += chrono::Duration::seconds(3);
        w.takt(29_990.0, 16.0, tw);
        tw += chrono::Duration::seconds(3);
        w.takt(29_870.0, 1.0, tw);
        assert_eq!(w.saldo_kg, 0.0);
    }

    /// Ein Anstieg kurz vor dem Aufsetzen wird am Boden noch bestaetigt.
    #[test]
    fn anstieg_kurz_vor_dem_aufsetzen_geht_nicht_verloren() {
        let mut st = FlightStats::default();
        st.takeoff_at = Some(zeit("2026-11-10T18:00:00Z"));
        let mut t = zeit("2026-11-10T19:00:00Z");
        nachtanken_luft_takt(&mut st, 2_000.0, false, 1.0, t);
        t += chrono::Duration::seconds(3);
        nachtanken_luft_takt(&mut st, 7_000.0, false, 1.0, t);
        st.landing_fuel_kg = Some(6_995.0);
        for kg in [6_990.0, 6_985.0, 6_980.0, 6_975.0] {
            t += chrono::Duration::seconds(3);
            nachtanken_luft_takt(&mut st, kg, true, 1.0, t);
        }
        assert_eq!(nachgetankt_luft(&st), 5_000.0);
        // Am Boden kommt danach nichts mehr dazu.
        t += chrono::Duration::seconds(60);
        nachtanken_luft_takt(&mut st, 20_000.0, true, 1.0, t);
        t += chrono::Duration::seconds(60);
        nachtanken_luft_takt(&mut st, 20_000.0, true, 1.0, t);
        assert_eq!(nachgetankt_luft(&st), 5_000.0);
    }

    /// Der laufende Verbrauchswert (`fuel_used_kg` im Protokoll) fiel bei
    /// DLH 41 von 20 762 auf 0. Er laeuft jetzt stetig weiter — Vorschau des
    /// Streamers und Takt gehen denselben Weg.
    #[test]
    fn dlh41_fuel_used_bleibt_stetig() {
        use super::fuel_used_effective_kg as fu;
        let flight = dlh41();
        let mut initial = Some(22_145.973_f32);
        let mut t = zeit("2026-11-10T19:20:00Z");
        let mut vor = Some(1_394.8253_f32);
        let mut verlauf = Vec::new();
        for kg in DLH41_SPRUNG {
            t += chrono::Duration::seconds(3);
            let vorschau = {
                let st = flight.stats.lock().unwrap();
                nachtanken_luft_vorschau(&st, kg, false, 1.0, t)
            };
            verlauf.push(fu(&mut initial, 0.0, kg, false, vor, vorschau));
            step_flight_at(&flight, &in_der_luft(kg), t);
            vor = Some(kg);
        }
        // 20 755,5 → 20 762,0 → [Sprung] 20 762,0 → 20 768,5 → …
        for paar in verlauf.windows(2) {
            let schritt = paar[1] - paar[0];
            assert!((0.0..10.0).contains(&schritt), "Verlauf nicht stetig: {verlauf:?}");
        }
        assert!((verlauf[2] - 20_762.0).abs() < 0.1, "{verlauf:?}");
        // Am Ende derselbe Wert wie der Verbrauch ab Block im PIREP.
        let st = flight.stats.lock().unwrap();
        let block = blocksprit_mit_nachtanken(&st).unwrap();
        assert!((verlauf[6] - (block - 22_118.7)).abs() < 0.1, "{verlauf:?} / Block {block}");
        drop(st);

        // Nachtanken UEBER den Blockwert hinaus setzt in der Luft nicht mehr
        // auf null zurueck.
        let mut initial = Some(8_000.0_f32);
        assert!((fu(&mut initial, 0.0, 12_000.0, false, Some(2_000.0), 10_000.0) - 6_000.0).abs() < 0.1);
        // Ein zurueckgenommener Ausschlag nimmt auch die Basis zurueck.
        assert!((fu(&mut initial, 0.0, 2_000.0, false, Some(12_000.0), -10_000.0) - 6_000.0).abs() < 0.1);
        assert_eq!(initial, Some(8_000.0));
        // Am Gate bleibt es beim alten Verhalten: Tanken setzt die Basis neu.
        let mut initial = Some(8_000.0_f32);
        assert_eq!(fu(&mut initial, 0.0, 12_000.0, true, Some(8_000.0), 0.0), 0.0);
        assert_eq!(initial, Some(12_000.0));
    }

    /// Der Hinweis kommt einmal je Tankvorgang, erst wenn der Anstieg steht,
    /// mit der ganzen Menge und dem Tankstand vor DIESEM Vorgang.
    #[test]
    fn hinweis_einmal_und_erst_wenn_der_anstieg_steht() {
        let (mut n, mut t) = zaehler(1_390.0);
        let mut takt = |n: &mut NachtankenLuft, kg: f32| {
            t += chrono::Duration::seconds(3);
            n.takt(kg, 1.0, t);
            n.hinweis_faellig()
        };
        assert_eq!(takt(&mut n, 8_000.0), None, "steigt noch");
        assert_eq!(takt(&mut n, 15_000.0), None, "steigt noch");
        assert_eq!(takt(&mut n, 14_994.0), None, "steht noch nicht lange genug");
        assert_eq!(takt(&mut n, 14_988.0), None);
        let h = takt(&mut n, 14_982.0).expect("Anstieg steht");
        assert_eq!(h.menge_kg, 13_610.0);
        assert_eq!(h.tank_davor_kg, Some(1_390.0));
        assert!(n.geaendert, "nach dem Hinweis wird sofort gespeichert");
        assert_eq!(takt(&mut n, 14_976.0), None, "nur einmal");
        // Zweiter Tankvorgang: eigener Hinweis, eigener Tankstand davor.
        assert_eq!(takt(&mut n, 20_000.0), None);
        assert_eq!(takt(&mut n, 19_994.0), None);
        assert_eq!(takt(&mut n, 19_988.0), None);
        assert_eq!(takt(&mut n, 19_982.0), None);
        let h = takt(&mut n, 19_976.0).expect("zweiter Hinweis");
        assert_eq!(h.menge_kg, 5_024.0);
        assert_eq!(h.tank_davor_kg, Some(14_976.0));
        assert_eq!(n.tank_davor_kg, Some(1_390.0), "fuer den PIREP bleibt der niedrigste");
        let text = nachtanken_luft_hinweis_text(&h);
        assert!(text.0.contains("5024 kg"), "{}", text.0);
        assert!(text.1.contains("Tank davor 14976 kg"), "{}", text.1);
        assert!(text.1.contains("insgesamt 18634 kg"), "{}", text.1);
    }
