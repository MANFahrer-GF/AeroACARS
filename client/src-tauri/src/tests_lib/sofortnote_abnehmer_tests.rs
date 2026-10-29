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
        let st = f.stats.lock().unwrap();
        let (_, felder, notiz) = pirep_bewertete_teile(f, &st, "YYYY");
        (felder.get("Landing Score").cloned(), notiz)
    }

    /// Codex-Befund zu P-4.40: Score, Feld und Notiz eines PIREP aus EINER
    /// Bewertung. Vorher drei Rechnungen unter derselben Sperre.
    #[test]
    fn pirep_aufbau_eine_bewertung() {
        let f = fixture();
        mit_landung(&f);
        let st = f.stats.lock().unwrap();
        aufrufe_zuruecksetzen();
        let (score, felder, notiz) = pirep_bewertete_teile(&f, &st, "YYYY");
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
