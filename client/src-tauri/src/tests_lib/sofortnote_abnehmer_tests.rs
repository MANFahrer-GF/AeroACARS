    //! P-4.40 (Kap. 4.5): die fuenf Abnehmer der Sofortnote in drei Faellen.
    //! Jede Zelle der Tabelle ist ein Test; der Name sagt `abnehmer_fall`.
    //! Die Kabinen-Zellen stehen in `kabinenansagen::reaktion_tests`, weil dort
    //! die Reaktion gebaut wird.
    use super::*;
    use crate::bewertung::{aufrufe, aufrufe_zuruecksetzen, erzwingen, Erzwungen};

    fn fixture() -> ActiveFlight {
        ActiveFlight {
            pirep_id: "test-pirep".into(),
            bid_id: 0,
            flight_id: String::new(),
            bid_callsign: None,
            pilot_callsign: None,
            started_at: Utc::now(),
            airline_icao: String::new(),
            planned_registration: String::new(),
            aircraft_icao: "A321".into(),
            aircraft_name: String::new(),
            flight_number: "1".into(),
            dpt_airport: "XXXX".into(),
            arr_airport: "YYYY".into(),
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

    /// Eine gemessene Landung mit sanfter Sinkrate: das Modul liefert eine Note.
    fn mit_landung(flight: &ActiveFlight) {
        let mut st = flight.stats.lock().unwrap();
        st.landing_score = Some(LandingScore::Smooth);
        st.landing_at = Some(Utc::now());
        st.landing_rate_fpm = Some(-190.0);
        st.landing_peak_vs_fpm = Some(-190.0);
        st.landing_peak_g_force = Some(1.2);
        st.landing_g_force = Some(1.2);
    }

    fn bewertung(flight: &ActiveFlight) -> Option<LandungsBewertung> {
        let st = flight.stats.lock().unwrap();
        canonical_landing_bewertung(flight, &st, "YYYY")
    }

    fn meldung(f: &ActiveFlight) -> (String, String) {
        let b = bewertung(f);
        touchdown_meldungen(-190.0, "G 1.20", "", b.as_ref())
    }

    // ── Landungsmeldung im Client ───────────────────────────────────────

    #[test]
    fn landungsmeldung_modul_rechnet() {
        let f = fixture();
        mit_landung(&f);
        let (log, acars) = meldung(&f);
        assert_eq!(log, "Touchdown: V/S -190 fpm, G 1.20");
        assert_eq!(acars, "Touchdown — V/S -190 fpm, G 1.20");
    }

    #[test]
    fn landungsmeldung_paket_modul_faellt_aus() {
        let f = fixture();
        mit_landung(&f);
        let _w = erzwingen(Erzwungen::PaketKaputt);
        let (log, acars) = meldung(&f);
        for m in [&log, &acars] {
            assert!(m.contains("(vorläufig)"), "{m}");
            assert!(m.contains("Note "), "{m}");
            assert!(!m.contains("nachgeholt"), "{m}");
        }
    }

    #[test]
    fn landungsmeldung_beide_fallen_aus() {
        let f = fixture();
        mit_landung(&f);
        let _w = erzwingen(Erzwungen::BeideKaputt);
        let (log, acars) = meldung(&f);
        for m in [&log, &acars] {
            assert!(m.ends_with("Bewertung nicht möglich, wird auf dem Server nachgeholt"), "{m}");
            assert!(!m.contains("Note "), "{m}");
        }
    }

    // ── MQTT-Touchdown → live → Discord ─────────────────────────────────

    /// Der Leseweg des Touchdown-Payloads: festgehalten von der Meldung
    /// (`announce_landing_score`), gelesen vom Payload.
    fn payload_felder(f: &ActiveFlight) -> (Option<String>, Option<String>, Option<String>) {
        let mut st = f.stats.lock().unwrap();
        touchdown_bewertung_festhalten(f, &mut st);
        touchdown_payload_note(&st)
    }

    /// Codex-Befund zu P-4.40: Meldung und Payload lesen EINE Bewertung. Vorher
    /// rechnete der Payload neu — bei einem Modulausfall zwischen beiden
    /// Rechnungen trug die Meldung „vorlaeufig“, der Payload „endgueltig“.
    #[test]
    fn touchdown_meldung_und_payload_eine_bewertung() {
        let f = fixture();
        mit_landung(&f);
        aufrufe_zuruecksetzen();
        let mut st = f.stats.lock().unwrap();
        let gemeldet = {
            let _w = erzwingen(Erzwungen::PaketKaputt);
            touchdown_bewertung_festhalten(&f, &mut st)
        };
        // Das Paketmodul ist wieder da, wenn der Payload gebaut wird.
        let (_, _, status) = touchdown_payload_note(&st);
        assert_eq!(gemeldet.map(|b| b.status), Some("vorlaeufig"));
        assert_eq!(status.as_deref(), Some("vorlaeufig"), "Payload = Meldung");
        assert_eq!(aufrufe(), 1, "eine Bewertung je Landung");
    }

    #[test]
    fn mqtt_touchdown_modul_rechnet() {
        let f = fixture();
        mit_landung(&f);
        let (label, grade, status) = payload_felder(&f);
        assert!(label.is_some() && grade.is_some());
        assert_eq!(status.as_deref(), Some("endgueltig"));
    }

    #[test]
    fn mqtt_touchdown_paket_modul_faellt_aus() {
        let f = fixture();
        mit_landung(&f);
        let (label, grade, status) = {
            let _w = erzwingen(Erzwungen::PaketKaputt);
            payload_felder(&f)
        };
        assert!(label.is_some() && grade.is_some(), "die Note reist mit");
        assert_eq!(status.as_deref(), Some("vorlaeufig"), "und der Vermerk");
    }

    #[test]
    fn mqtt_touchdown_beide_fallen_aus() {
        let f = fixture();
        mit_landung(&f);
        let (label, grade, status) = {
            let _w = erzwingen(Erzwungen::BeideKaputt);
            payload_felder(&f)
        };
        assert_eq!((label, grade), (None, None), "ohne Note");
        assert_eq!(status.as_deref(), Some("modulfehler"));
    }

    // ── phpVMS-Feld „Landing Score“ + Notiz ─────────────────────────────

    /// Derselbe Aufbau wie beim Einreichen (`pirep_bewertete_teile`).
    fn feld_und_notiz(f: &ActiveFlight) -> (Option<String>, String) {
        let mut st = f.stats.lock().unwrap();
        let (_, felder, notiz) = pirep_bewertete_teile(f, &mut st, "YYYY");
        (felder.get("Landing Score").cloned(), notiz)
    }

    /// Codex-Befund zu P-4.40: Score, Feld und Notiz eines PIREP aus EINER
    /// Bewertung. Vorher drei Rechnungen unter derselben Sperre.
    #[test]
    fn pirep_aufbau_eine_bewertung() {
        let f = fixture();
        mit_landung(&f);
        let mut st = f.stats.lock().unwrap();
        aufrufe_zuruecksetzen();
        let (score, felder, notiz) = pirep_bewertete_teile(&f, &mut st, "YYYY");
        assert_eq!(aufrufe(), 1, "eine Bewertung je PIREP-Aufbau");
        let score = score.expect("Note");
        assert!(felder["Landing Score"].contains(&format!("{score}/100")), "{felder:?}");
        assert!(notiz.contains(&format!("{score}/100")), "{notiz}");
    }

    #[test]
    fn phpvms_modul_rechnet() {
        let f = fixture();
        mit_landung(&f);
        let (feld, notiz) = feld_und_notiz(&f);
        assert!(feld.expect("Feld gefuellt").contains("/100"));
        assert!(notiz.contains("Grade "), "{notiz}");
        assert!(!notiz.contains("vorläufig") && !notiz.contains("nicht bewertet"), "{notiz}");
    }

    #[test]
    fn phpvms_paket_modul_faellt_aus() {
        let f = fixture();
        mit_landung(&f);
        let _w = erzwingen(Erzwungen::PaketKaputt);
        let (feld, notiz) = feld_und_notiz(&f);
        assert!(feld.expect("Note bleibt im Feld").contains("/100"));
        assert!(notiz.contains("Grade "), "{notiz}");
        assert!(notiz.contains(&format!("Bewertung     vorläufig, Fassung {}", crate::SCORE_ALGORITHMUS_VERSION)), "{notiz}");
    }

    #[test]
    fn phpvms_beide_fallen_aus() {
        let f = fixture();
        mit_landung(&f);
        let _w = erzwingen(Erzwungen::BeideKaputt);
        let (feld, notiz) = feld_und_notiz(&f);
        assert_eq!(feld, None, "Feld leer");
        assert!(!notiz.contains("Grade "), "{notiz}");
        assert!(notiz.contains("Bewertung     nicht bewertet (Modulfehler)"), "{notiz}");
    }

    /// Codex-Befund zu P-4.40, zweite Runde: je Einreichung EINE
    /// Modulrechnung fuer alle Abnehmer — phpVMS (Feld, Notiz), MQTT-PIREP,
    /// Landungsdatensatz und Eingabe-Dokument. Datensatz, PIREP und Dokument
    /// entstehen erst nach dem Senden; faellt das Paketmodul nur beim Aufbau
    /// aus, muessen sie trotzdem dasselbe „vorlaeufig“ tragen wie phpVMS. Das
    /// Dokument traegt genau die Eingabe, zu der sein Ergebnis gehoert.
    #[test]
    fn einreichung_eine_rechnung_fuer_alle_abnehmer() {
        let f = fixture();
        mit_landung(&f);
        f.stats.lock().unwrap().landung_fenster =
            vec![TouchdownWindowSample::from(TelemetrySample::default())];
        aufrufe_zuruecksetzen();
        let (_, felder, notiz) = {
            let _w = erzwingen(Erzwungen::PaketKaputt);
            let mut st = f.stats.lock().unwrap();
            pirep_bewertete_teile(&f, &mut st, "YYYY")
        };
        // Ab hier rechnet das Paketmodul wieder.
        let payload = build_pirep_payload(&f, &api_client::FileBody::default(), "YYYY", "YYYY");
        let st = f.stats.lock().unwrap();
        let rechnung = einreichungs_rechnung(&f, &st, "YYYY");
        let record = build_landing_record(&f, &st, None, Some("A321"), None, "YYYY", Some(&rechnung), |_| None)
            .expect("Datensatz");
        let teile = eingabe_teile(&f, &st, true, rechnung.clone());
        let (art, eingabe, fenster, begleitung, ergebnis) = teile.last().expect("Dokument der Landung");
        let ergebnis = ergebnis.clone().expect("Ergebnis der Einreichung");
        let doc = crate::landungs_eingabe::dokument("test-pirep", 1, *art, None, eingabe, begleitung, fenster, &ergebnis);

        assert_eq!(aufrufe(), 1, "eine Modulrechnung je Einreichung");
        assert!(felder.contains_key("Landing Score"), "{felder:?}");
        assert!(notiz.contains("Bewertung     vorläufig"), "{notiz}");
        assert_eq!(payload.bewertung_status.as_deref(), Some("vorlaeufig"), "MQTT-PIREP");
        assert_eq!(record.bewertung_status.as_deref(), Some("vorlaeufig"), "Datensatz");
        assert_eq!(doc["ergebnis"]["bewertung_status"], "vorlaeufig", "Dokument");
        // Eingabe und Ergebnis des Dokuments aus derselben Rechnung.
        assert_eq!(eingabe, &rechnung.eingabe);
        assert_eq!(ergebnis, rechnung.ergebnis);
        assert_eq!(doc["eingabe"]["eingang"]["landung"], serde_json::to_value(&rechnung.eingabe).unwrap());
        // P-9.44: die Herkunft derselben Landung, am eingereichten Flughafen.
        assert_eq!(doc["eingabe"]["herkunft"]["ausweichziel"]["eingereicht"], "YYYY");
        assert_eq!(doc["eingabe"]["herkunft"], landungs_herkunft(&f, &st, &rechnung.eingabe, "YYYY"));
        // Kap. 5.2: meta nennt das Muster der Rechnung; roh steht im Dokument.
        assert_eq!(doc["eingabe"]["meta"]["aircraft_icao"], serde_json::json!(muster_fuer_landung(&st, &f.aircraft_icao)));
        assert!(doc["eingabe"]["roh"]["anflugpuffer"]["forensik"].is_array());
    }

    /// P-5.10: `seq` und `input_sha256` des MQTT-PIREP sind genau die des
    /// Eingabe-Dokuments der Landung (live gleicht damit ab). Gegenprobe:
    /// ohne Aufsetzfenster gibt es kein Dokument, also auch keine Kennung.
    #[test]
    fn pirep_nutzlast_nennt_das_eingabe_dokument() {
        let f = fixture();
        mit_landung(&f);
        f.stats.lock().unwrap().landung_fenster =
            vec![TouchdownWindowSample::from(TelemetrySample::default())];
        {
            let mut st = f.stats.lock().unwrap();
            pirep_bewertete_teile(&f, &mut st, "YYYY");
        }
        let payload = build_pirep_payload(&f, &api_client::FileBody::default(), "YYYY", "YYYY");
        let st = f.stats.lock().unwrap();
        let rechnung = einreichungs_rechnung(&f, &st, "YYYY");
        let teile = eingabe_teile(&f, &st, true, rechnung);
        let seq = teile.len() as u32;
        let (art, eingabe, fenster, begleitung, ergebnis) = teile.last().expect("Dokument der Landung");
        let doc = crate::landungs_eingabe::dokument("test-pirep", seq, *art, None, eingabe, begleitung, fenster, &ergebnis.clone().unwrap());
        assert_eq!(payload.seq, Some(seq));
        assert_eq!(payload.input_sha256.as_deref(), doc["eingabe"]["meta"]["input_sha256"].as_str());
        let leitung = serde_json::to_value(&payload).unwrap();
        assert_eq!(leitung["seq"], seq);
        assert_eq!(leitung["input_sha256"], doc["eingabe"]["meta"]["input_sha256"]);
        assert!(payload.runtime_bundle_id.is_some());
        drop(st);
        f.stats.lock().unwrap().landung_fenster.clear();
        let ohne = build_pirep_payload(&f, &api_client::FileBody::default(), "YYYY", "YYYY");
        let leitung = serde_json::to_value(&ohne).unwrap();
        assert!(leitung.get("seq").is_none() && leitung.get("input_sha256").is_none(), "ohne Wert nicht auf der Leitung");
        assert_eq!((ohne.seq, ohne.input_sha256), (None, None));
    }

    /// P-5.10: Der Touchdown traegt die Nummer des Aufsetzers (nach einem
    /// eingefrorenen Touch-and-Go die 2) und den Hash der Eingabe der
    /// Sofortnote — derselbe Hash, den ein Dokument aus dieser Eingabe haette.
    #[test]
    fn touchdown_nennt_seq_und_hash_der_sofortnote() {
        let f = fixture();
        mit_landung(&f);
        let mut st = f.stats.lock().unwrap();
        assert_eq!(touchdown_seq(&st), 1);
        let arr = aufsetz_flughafen(&f, &st);
        let eingabe = landungs_bewertungseingabe(&f, &st, muster_fuer_landung(&st, &f.aircraft_icao), &arr);
        let begleitung = landungs_begleitung(&f, &st, &eingabe, muster_fuer_landung(&st, &f.aircraft_icao), &arr);
        let doc = crate::landungs_eingabe::dokument("test-pirep", 1, crate::landungs_eingabe::Art::Landung, None, &eingabe, &begleitung, &[], &None);
        assert_eq!(Some(touchdown_input_sha256(&f, &st).as_str()), doc["eingabe"]["meta"]["input_sha256"].as_str());
        st.aufsetzer_eingefroren.push((eingabe, Vec::new(), begleitung));
        assert_eq!(touchdown_seq(&st), 2);
        // Gegenprobe: eine andere Sinkrate ergibt einen anderen Hash.
        let vorher = touchdown_input_sha256(&f, &st);
        st.landing_rate_fpm = Some(-650.0);
        st.landing_peak_vs_fpm = Some(-650.0);
        assert_ne!(touchdown_input_sha256(&f, &st), vorher);
    }

    /// P-4.10: phpVMS-Feld, Notiz und PIREP uebernehmen die zuletzt
    /// gespeicherte Ausgabe OHNE Neurechnung — erst die Sofortnote, nach dem
    /// Ausrollen die endgueltige. Belegt ueber den Aufrufzaehler (0) und den
    /// Status: die Sofortnote wurde bei ausgefallenem Paketmodul gerechnet
    /// („vorlaeufig“), beim Einreichen rechnet es wieder — uebernommen wird
    /// trotzdem das Gespeicherte. Gegenproben: nach einem Bahnwechsel und mit
    /// anderem Ankunftsflughafen rechnet die Einreichung selbst (1 Aufruf).
    #[test]
    fn pirep_uebernimmt_gespeicherte_ausgabe() {
        let f = fixture();
        mit_landung(&f);
        let teile = |arr: &str| {
            let mut st = f.stats.lock().unwrap();
            pirep_bewertete_teile(&f, &mut st, arr)
        };
        {
            let _w = erzwingen(Erzwungen::PaketKaputt);
            let mut st = f.stats.lock().unwrap();
            touchdown_bewertung_festhalten(&f, &mut st);
        }
        aufrufe_zuruecksetzen();
        let (_, _, notiz) = teile("YYYY");
        assert_eq!(aufrufe(), 0, "kein bewerten beim Einreichen");
        assert!(notiz.contains("Bewertung     vorläufig"), "die gespeicherte Sofortnote: {notiz}");

        // Nach dem Ausrollen: die endgueltige Rechnung (Paketmodul rechnet).
        {
            let mut st = f.stats.lock().unwrap();
            st.rollout_finalized = true;
            st.bahn_spur_laeuft = false;
        }
        endgueltige_rechnung_festhalten(&f);
        aufrufe_zuruecksetzen();
        let (score, _, notiz) = teile("YYYY");
        assert_eq!(aufrufe(), 0, "kein bewerten beim Einreichen");
        assert!(!notiz.contains("vorläufig"), "die endgueltige geht vor: {notiz}");
        let st = f.stats.lock().unwrap();
        let gespeichert = st.landung_endgueltig.as_ref().unwrap().rechnung.clone();
        assert_eq!(score, gespeichert.ergebnis.as_ref().and_then(|(a, _)| a.gesamt).map(i32::from));
        assert_eq!(st.pirep_rechnung.as_ref().unwrap().eingabe, gespeichert.eingabe, "PIREP traegt die gespeicherte Eingabe");
        drop(st);

        // Gegenprobe 1: anderer eingereichter Ankunftsflughafen → eigene Rechnung.
        aufrufe_zuruecksetzen();
        teile("ZZZZ");
        assert_eq!(aufrufe(), 1);
        // Gegenprobe 2: Bahnzuordnung seither geaendert → eigene Rechnung.
        f.stats.lock().unwrap().bahn_revision += 1;
        aufrufe_zuruecksetzen();
        teile("YYYY");
        assert_eq!(aufrufe(), 1);
    }

    /// P-4.10: Eine gespeicherte Rechnung gilt nur fuer ihren Aufsetzer — die
    /// naechste Landung bekommt nie die Ausgabe des vorigen (zusaetzlich zum
    /// Abraeumen beim Touch-and-Go).
    #[test]
    fn neuer_aufsetzer_nimmt_keine_alte_rechnung() {
        let f = fixture();
        mit_landung(&f);
        let mut st = f.stats.lock().unwrap();
        touchdown_bewertung_festhalten(&f, &mut st);
        assert!(st.landung_sofort.is_some());
        st.landung_endgueltig = st.landung_sofort.clone();
        // Neuer Aufsetzer: anderer Zeitpunkt → keine gespeicherte gilt.
        st.landing_at = Some(Utc::now() + chrono::Duration::seconds(90));
        assert!(gespeicherte_rechnung(&st, "YYYY", true).is_none(), "Schluessel haengt am Aufsetzer");
    }

    /// P-3.31: Fehlt der Stand des Flugs, traegt die Note „vorläufig,
    /// Fassung n“ (Meldung, Notiz). Solange er fehlt, uebernimmt die Abgabe
    /// die gespeicherte Note (kein Aufruf); ist er nachgeholt, rechnet sie vor
    /// der Abgabe neu — mit seinem Modul, `endgueltig`.
    #[test]
    fn ohne_pinstand_vorlaeufig_und_neu_vor_der_abgabe() {
        use crate::bewertung::pin_erzwingen;
        use crate::laufzeit_paket::PinStand;
        let f = fixture();
        mit_landung(&f);
        let teile = || {
            let mut st = f.stats.lock().unwrap();
            pirep_bewertete_teile(&f, &mut st, "YYYY")
        };
        let fassung = crate::SCORE_ALGORITHMUS_VERSION;
        {
            let _p = pin_erzwingen(PinStand::Fehlt);
            let b = touchdown_bewertung_festhalten(&f, &mut f.stats.lock().unwrap()).expect("Bewertung");
            assert_eq!(bewertung_vermerk(&b), Some(format!("vorläufig, Fassung {fassung}")));
            aufrufe_zuruecksetzen();
            let (score, _, notiz) = teile();
            assert_eq!(aufrufe(), 0, "Stand fehlt weiter: gespeicherte Note");
            assert!(score.is_some(), "die Note bleibt im Feld");
            assert!(notiz.contains(&format!("Bewertung     vorläufig, Fassung {fassung}")), "{notiz}");
        }
        let _p = pin_erzwingen(PinStand::Nachgeholt(crate::laufzeit_paket::eingebautes_bewertungsmodul().unwrap()));
        aufrufe_zuruecksetzen();
        let (_, _, notiz) = teile();
        assert_eq!(aufrufe(), 1, "Stand nachgeholt: vor der Abgabe neu gerechnet");
        assert!(!notiz.contains("vorläufig"), "{notiz}");
    }

    // ── Abgabe-Sperre: haengt nicht vom Modul ab ────────────────────────

    /// Mit gemessener Landung keine Rueckfrage, ohne sie nach langer
    /// Unterbrechung `LandungFehlt` — in jedem Fall dasselbe.
    ///
    /// Codex-Befund zu P-4.40: Gleichheit allein war eine Tautologie — der
    /// Weg rief die Bewertung nie, also kam immer dasselbe heraus. Der Zaehler
    /// belegt, dass die Sperre die Bewertung gar nicht beruehrt: Haengt sie je
    /// davon ab, wird er ungleich null und der Test rot.
    fn sperre_unveraendert() -> Vec<&'static str> {
        aufrufe_zuruecksetzen();
        let f = fixture();
        mit_landung(&f);
        let pflicht = {
            let mut st = f.stats.lock().unwrap();
            st.resume_luecke_secs = Some(i64::from(u32::MAX));
            assert!(landung_wurde_gemessen(&st));
            assert_eq!(abgabe_sperre(&st), None);
            validate_for_filing(&f, &st, 120)
        };
        let g = fixture();
        {
            let mut st = g.stats.lock().unwrap();
            st.resume_luecke_secs = Some(i64::from(u32::MAX));
            assert!(!landung_wurde_gemessen(&st));
            assert_eq!(abgabe_sperre(&st), Some(AbgabeSperre::LandungFehlt));
        }
        assert_eq!(aufrufe(), 0, "die Abgabe-Sperre darf die Bewertung nicht rufen");
        pflicht
    }

    #[test]
    fn abgabe_sperre_modul_rechnet() {
        let _ = sperre_unveraendert();
    }

    #[test]
    fn abgabe_sperre_paket_modul_faellt_aus() {
        let normal = sperre_unveraendert();
        let _w = erzwingen(Erzwungen::PaketKaputt);
        assert_eq!(sperre_unveraendert(), normal);
    }

    #[test]
    fn abgabe_sperre_beide_fallen_aus() {
        let normal = sperre_unveraendert();
        let _w = erzwingen(Erzwungen::BeideKaputt);
        assert_eq!(sperre_unveraendert(), normal);
    }
