    use super::*;
    use flate2::read::GzDecoder;
    use std::io::{BufRead, BufReader};

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

    fn t0() -> DateTime<Utc> {
        Utc::now() - chrono::Duration::seconds(3_600)
    }

    /// Bahn nach Norden; `meter` ab dem Aufsetzpunkt.
    fn snap(meter: f64, agl_ft: f64, vs_fpm: f32, gs_kt: f32, am_boden: bool, triebwerke: u8) -> SimSnapshot {
        let mut s = SimSnapshot::default();
        s.lat = 28.445 + meter / 111_320.0;
        s.lon = -13.8637;
        s.altitude_agl_ft = agl_ft;
        s.altitude_msl_ft = 30.0 + agl_ft;
        s.vertical_speed_fpm = vs_fpm;
        s.groundspeed_kt = gs_kt;
        s.indicated_airspeed_kt = gs_kt;
        s.true_airspeed_kt = gs_kt;
        s.heading_deg_true = 0.0;
        s.heading_deg_magnetic = 0.0;
        s.on_ground = am_boden;
        s.engines_running = triebwerke;
        s.g_force = 1.0;
        s.gear_position = 1.0;
        s.simulation_rate = 1.0;
        s
    }

    /// Zustand unmittelbar nach dem Aufsetzen, wie ihn der `Final`-Arm
    /// hinterlässt.
    fn gerade_aufgesetzt(flight: &ActiveFlight, td: DateTime<Utc>) {
        let mut st = flight.stats.lock().unwrap();
        st.phase = FlightPhase::Landing;
        st.shadow_phase = Some(FlightPhase::Landing);
        st.was_airborne = true;
        st.was_on_ground = Some(true);
        st.landing_at = Some(td);
        st.landing_peak_vs_fpm = Some(-667.0);
        st.landing_rate_fpm = Some(-667.0);
        st.landing_peak_g_force = Some(1.71);
        st.landing_lat = Some(28.445);
        st.landing_lon = Some(-13.8637);
        st.landing_score = Some(LandingScore::Hard);
        st.landing_score_announced = true;
    }

    struct Verlauf {
        /// Zeitpunkt des ersten Messwerts über 100 ft.
        ueber_100ft: Option<DateTime<Utc>>,
        /// Zeitpunkt, zu dem die Phase `Landing` verlassen hat.
        landing_verlassen: Option<(DateTime<Utc>, FlightPhase)>,
    }

    /// Bodenlauf von `bodenlauf_s` Sekunden, dann Wegsteigen mit 600 fpm.
    fn durchstart_nach_bodenlauf(flight: &ActiveFlight, bodenlauf_s: i64, triebwerke: u8) -> Verlauf {
        let td = t0();
        gerade_aufgesetzt(flight, td);
        let mut v = Verlauf { ueber_100ft: None, landing_verlassen: None };
        let mut meter = 0.0;
        let mut beobachte = |now: DateTime<Utc>, s: &SimSnapshot, v: &mut Verlauf| {
            phasen_takt(flight, s, now);
            if v.ueber_100ft.is_none() && !s.on_ground && s.altitude_agl_ft > 100.0 {
                v.ueber_100ft = Some(now);
            }
            let phase = flight.stats.lock().unwrap().phase;
            if v.landing_verlassen.is_none() && phase != FlightPhase::Landing {
                v.landing_verlassen = Some((now, phase));
            }
        };
        for i in 0..=bodenlauf_s {
            let gs = 133.0 - 0.3 * i as f32;
            meter += gs as f64 * 0.514;
            let s = snap(meter, 9.0, 0.0, gs, true, triebwerke);
            beobachte(td + chrono::Duration::seconds(i), &s, &mut v);
        }
        for k in 1..=90_i64 {
            meter += 140.0 * 0.514;
            let agl = (14.0 + 10.0 * k as f64).min(800.0);
            let s = snap(meter, agl, 600.0, 140.0, false, triebwerke);
            beobachte(td + chrono::Duration::seconds(bodenlauf_s + k), &s, &mut v);
        }
        v
    }

    /// Bodenlauf 10 s (Touch-and-Go wie W4B0, 13.10.) bis 120 s: Die Landung
    /// wird abgeschlossen, sobald das Flugzeug über 100 ft steigt — nicht
    /// nur innerhalb von 30 s ab dem Aufsetzen.
    #[test]
    fn durchstart_nach_jedem_bodenlauf_verlaesst_landing() {
        for bodenlauf in [10_i64, 29, 31, 36, 60, 120] {
            let flight = fixture();
            let v = durchstart_nach_bodenlauf(&flight, bodenlauf, 2);
            let ueber = v.ueber_100ft.expect("Testverlauf steigt über 100 ft");
            let (wann, phase) = v
                .landing_verlassen
                .unwrap_or_else(|| panic!("Bodenlauf {bodenlauf} s: Phase blieb in der Luft auf Landing"));
            assert_eq!(phase, FlightPhase::Climb, "Bodenlauf {bodenlauf} s");
            assert!(
                (wann - ueber).num_seconds() <= 3,
                "Bodenlauf {bodenlauf} s: Landing erst {} s nach 100 ft verlassen",
                (wann - ueber).num_seconds()
            );
            let st = flight.stats.lock().unwrap();
            assert_eq!(st.go_around_count, 1, "Bodenlauf {bodenlauf} s");
            assert_eq!(
                st.touchdown_events.len(),
                1,
                "Bodenlauf {bodenlauf} s: genau ein Eintrag für den einen Aufsetzer: {:?}",
                st.touchdown_events
            );
            let ev = &st.touchdown_events[0];
            assert_eq!(ev.kind, TouchdownKind::TouchAndGo, "Bodenlauf {bodenlauf} s");
            assert!((ev.peak_vs_fpm + 667.0).abs() < 0.5, "Bodenlauf {bodenlauf} s: {}", ev.peak_vs_fpm);
            assert!((ev.peak_g - 1.71).abs() < 0.01, "Bodenlauf {bodenlauf} s: {}", ev.peak_g);
            // Die nächste Landung beginnt frisch.
            assert_eq!(st.landing_at, None, "Bodenlauf {bodenlauf} s");
            assert!(st.landing_score.is_none(), "Bodenlauf {bodenlauf} s");
            assert!(!st.landing_score_announced, "Bodenlauf {bodenlauf} s");
            assert_eq!(st.landing_peak_vs_fpm, None, "Bodenlauf {bodenlauf} s");
            assert_eq!(st.rollout_distance_m, None, "Bodenlauf {bodenlauf} s: Rollstrecke des Steigflugs");
            assert!(st.bahn_spur.is_empty(), "Bodenlauf {bodenlauf} s: Spur des Steigflugs");
            // v2 folgt dem Wechsel.
            assert_eq!(st.shadow_phase, Some(FlightPhase::Climb), "Bodenlauf {bodenlauf} s");
        }
    }

    /// Letzte Absicherung: Greift ein Riegel nicht (hier: Triebwerke melden
    /// 0, wie bei manchen Add-ons), darf `Landing` trotzdem nicht in der Luft
    /// hängen bleiben.
    #[test]
    fn landing_haengt_nie_in_der_luft() {
        let flight = fixture();
        let v = durchstart_nach_bodenlauf(&flight, 26, 0);
        let (_, phase) = v.landing_verlassen.expect("Landing in 800 ft verlassen");
        assert_eq!(phase, FlightPhase::Climb);
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.touchdown_events.len(), 1, "{:?}", st.touchdown_events);
        assert_eq!(st.touchdown_events[0].kind, TouchdownKind::TouchAndGo);
    }

    /// Busch-Hüpfer landen noch in `Takeoff` (der Sampler erfasst sie, die
    /// Phasenlogik nicht). Steigt das Flugzeug wieder weg, wird die Landung
    /// ebenso abgeschlossen und verbucht — an derselben Stelle.
    #[test]
    fn wegsteigen_schliesst_auch_eine_landung_in_takeoff_ab() {
        let flight = fixture();
        let td = t0();
        {
            let mut st = flight.stats.lock().unwrap();
            st.phase = FlightPhase::Takeoff;
            st.shadow_phase = Some(FlightPhase::Takeoff);
            st.was_airborne = true;
            st.was_on_ground = Some(true);
            st.sampler_touchdown_at = Some(td);
            st.sampler_touchdown_vs_fpm = Some(-310.0);
            st.sampler_touchdown_g_force = Some(1.25);
            st.touchdown_window_dumped_at = Some(td + chrono::Duration::seconds(10));
            st.touchdown_window_score_data_ready_at = Some(td + chrono::Duration::seconds(10));
            st.landing_peak_vs_fpm = Some(-310.0);
            st.landing_rate_fpm = Some(-310.0);
            st.landing_peak_g_force = Some(1.25);
        }
        for k in 1..=20_i64 {
            let s = snap(100.0 * k as f64, 20.0 * k as f64, 900.0, 70.0, false, 1);
            phasen_takt(&flight, &s, td + chrono::Duration::seconds(30 + k));
        }
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.touchdown_events.len(), 1, "{:?}", st.touchdown_events);
        assert_eq!(st.touchdown_events[0].kind, TouchdownKind::TouchAndGo);
        assert!((st.touchdown_events[0].peak_vs_fpm + 310.0).abs() < 0.5);
        assert_eq!(st.sampler_touchdown_at, None, "Sampler für den nächsten Aufsetzer bereit");
        assert_eq!(st.touchdown_window_dumped_at, None);
    }

    /// „Aufsetzfenster nicht gemessen“ gehoert zu EINEM Aufsetzer. War der
    /// erste nicht messbar, muss die finale Landung trotzdem bewertbar sein.
    #[test]
    fn ungemessener_erster_aufsetzer_sperrt_die_finale_landung_nicht() {
        let flight = fixture();
        let td = t0();
        gerade_aufgesetzt(&flight, td);
        let mut st = flight.stats.lock().unwrap();
        st.landung_abdeckung_fehlt = Some(touchdown_v2::FehlendeAbdeckung {
            groesste_luecke_ms: 4_000,
            proben: 0,
        });
        let s = snap(2_000.0, 400.0, 900.0, 140.0, false, 2);
        landung_beim_wegsteigen_abschliessen(&mut st, &s, td + chrono::Duration::seconds(40));
        assert!(st.landung_abdeckung_fehlt.is_none());
        assert_eq!(st.touchdown_events.len(), 1);
        assert!(st.landung_abgebrochen_phase_offen);
    }

    /// Die festgehaltene Touchdown-Bewertung (Meldung + Payload) gehoert zum
    /// Aufsetzer: Ein Touch-and-Go raeumt sie ab, sonst truege die naechste
    /// Landung die Note der vorigen.
    #[test]
    fn touch_and_go_raeumt_die_festgehaltene_bewertung_ab() {
        let flight = fixture();
        let td = t0();
        gerade_aufgesetzt(&flight, td);
        let mut st = flight.stats.lock().unwrap();
        st.landung_bewertung = Some(LandungsBewertung { verdict: None, status: "endgueltig", fassung: None });
        let s = snap(2_000.0, 400.0, 900.0, 140.0, false, 2);
        landung_beim_wegsteigen_abschliessen(&mut st, &s, td + chrono::Duration::seconds(40));
        assert_eq!(st.touchdown_events.len(), 1);
        assert_eq!(st.landung_bewertung, None);
    }

    /// Eine abgeschlossene Landung bleibt abgeschlossen: Nach dem Abrollen
    /// (TaxiIn/BlocksOn) setzt ein Slew oder Neuladen das Flugzeug in die
    /// Hoehe — das ist kein Touch-and-Go. Der Sampler ruft den Abschluss
    /// ohne eigene Phasenpruefung; die Sperre muss in der Funktion selbst
    /// sitzen.
    #[test]
    fn abgerollte_landung_wird_nicht_nachtraeglich_zum_touch_and_go() {
        for phase in [FlightPhase::TaxiIn, FlightPhase::BlocksOn, FlightPhase::Arrived] {
            let flight = fixture();
            let td = t0();
            gerade_aufgesetzt(&flight, td);
            let mut st = flight.stats.lock().unwrap();
            st.phase = phase;
            st.touchdown_events.push(TouchdownEvent {
                timestamp: td,
                kind: TouchdownKind::FinalLanding,
                peak_vs_fpm: -55.0,
                peak_g: 1.1,
                lat: 28.445,
                lon: -13.8637,
                sub_bounces: 0,
            });
            let s = snap(2_000.0, 3_000.0, 0.0, 250.0, false, 2);
            landung_beim_wegsteigen_abschliessen(&mut st, &s, td + chrono::Duration::seconds(600));
            assert_eq!(st.touchdown_events.len(), 1, "{phase:?}");
            assert_eq!(st.touchdown_events[0].kind, TouchdownKind::FinalLanding, "{phase:?}");
            assert_eq!(st.go_around_count, 0, "{phase:?}");
            assert_eq!(st.landing_at, Some(td), "{phase:?}");
            assert!(!st.landung_abgebrochen_phase_offen, "{phase:?}");
        }
    }

    /// Korpus-Fund J4dxXAPBz9BvZd0P (Pilot 15, 30-Tage-Replay 20.10.2026):
    /// Nach der Landung riss die Aufzeichnung sechs Minuten ab, danach
    /// meldete MSFS beim Neuladen eine Fantasie-Position (lat 0, lon 90,
    /// 211 ft ueber Grund, Fahrt 0, Triebwerke aus). Das ist kein Wegsteigen —
    /// die Landung bleibt die Landung.
    #[test]
    fn teleport_ohne_fahrt_ist_kein_wegsteigen() {
        let flight = fixture();
        let td = t0();
        gerade_aufgesetzt(&flight, td);
        for k in 0..90_i64 {
            let mut s = snap(0.0, 211.0, 0.0, 0.0, false, 0);
            s.lat = -0.0;
            s.lon = 90.0003;
            s.altitude_msl_ft = 228.0;
            phasen_takt(&flight, &s, td + chrono::Duration::seconds(372 + k));
        }
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.phase, FlightPhase::Landing);
        assert!(st.touchdown_events.iter().all(|e| e.kind != TouchdownKind::TouchAndGo), "{:?}", st.touchdown_events);
        assert_eq!(st.go_around_count, 0);
        assert_eq!(st.landing_at, Some(td));
    }

    /// Der Sampler misst mit 50 Hz. Ein einzelner AGL-Ausreisser im Ausrollen
    /// darf die Landung nicht abschliessen — erst eine Sekunde am Stueck.
    #[test]
    fn einzelner_ausreisser_schliesst_keine_landung_ab() {
        let t = t0();
        let ms = |n: i64| t + chrono::Duration::milliseconds(n);
        let mut seit = None;
        assert!(!steigflug_gehalten(&mut seit, true, ms(0)), "erster Messwert");
        assert!(!steigflug_gehalten(&mut seit, false, ms(20)), "Ausreisser vorbei");
        assert_eq!(seit, None, "Uhr beginnt neu");
        for n in 0..50 {
            assert!(!steigflug_gehalten(&mut seit, true, ms(1_000 + 20 * n)), "nach {} ms", 20 * n);
        }
        assert!(steigflug_gehalten(&mut seit, true, ms(2_000)), "eine Sekunde am Stueck");
    }

    fn ev(sek: i64, art: TouchdownKind, vs: f32, g: f32) -> TouchdownEvent {
        TouchdownEvent {
            timestamp: t0() + chrono::Duration::seconds(sek),
            kind: art,
            peak_vs_fpm: vs,
            peak_g: g,
            lat: 28.445,
            lon: -13.8637,
            sub_bounces: 0,
        }
    }

    /// Ein Flug, eine Landung: nichts Zusaetzliches im PIREP (byte-gleich zu
    /// vorher). Touch-and-Go + Landung: beide Eintraege, der haerteste ist
    /// der Touch-and-Go — und er steht als Zahl vorn im phpVMS-Feld, das die
    /// GSG-Wartung liest.
    #[test]
    fn haertester_aufsetzer_kommt_in_den_pirep() {
        let flight = fixture();
        let mut st = flight.stats.lock().unwrap();
        st.touchdown_events.push(ev(0, TouchdownKind::FinalLanding, -180.0, 1.2));
        let (liste, h) = aufsetzer_fuer_pirep(&st);
        assert!(liste.is_empty());
        assert!(h.is_none());
        assert!(!build_pirep_fields(&flight, &st, "YYYY").contains_key("Hardest Touchdown"));

        st.touchdown_events.clear();
        st.touchdown_events.push(ev(0, TouchdownKind::TouchAndGo, -667.0, 1.71));
        st.touchdown_events.push(ev(960, TouchdownKind::FinalLanding, -55.0, 1.10));
        let (liste, h) = aufsetzer_fuer_pirep(&st);
        let arten: Vec<_> = liste.iter().map(|e| e.art).collect();
        assert_eq!(arten, vec!["touch_and_go", "final_landing"]);
        let h = h.expect("haertester");
        assert!((h.peak_vs_fpm + 667.0).abs() < 0.5);
        assert!((h.peak_g - 1.71).abs() < 0.01);
        let feld = build_pirep_fields(&flight, &st, "YYYY")
            .remove("Hardest Touchdown")
            .expect("Feld fuer die Wartung");
        // Reine Zahl wie "Landing Rate" — die Wartung prueft is_numeric().
        assert_eq!(feld, "-667");
    }

    /// Ein ungemessener Aufsetzer (Sinkrate 0) ist nie der haerteste.
    #[test]
    fn ungemessener_aufsetzer_ist_nicht_der_haerteste() {
        let flight = fixture();
        let mut st = flight.stats.lock().unwrap();
        st.touchdown_events.push(ev(0, TouchdownKind::TouchAndGo, 0.0, 0.0));
        st.touchdown_events.push(ev(600, TouchdownKind::FinalLanding, -240.0, 1.3));
        let (_, h) = aufsetzer_fuer_pirep(&st);
        assert!((h.expect("haertester").peak_vs_fpm + 240.0).abs() < 0.5);
    }

    /// Vergleich PIREP gegen Live-Meldung: gleich → nichts; andere Sinkrate
    /// oder anderer Aufsetzer → Widerspruch. Ohne Meldung kein Vergleich.
    #[test]
    fn widerspruch_zur_live_meldung_wird_erkannt() {
        let flight = fixture();
        let td = t0();
        gerade_aufgesetzt(&flight, td);
        let mut st = flight.stats.lock().unwrap();
        assert_eq!(landung_widerspricht_meldung(&st), None, "nichts gemeldet");
        st.gemeldete_landung = Some((td.timestamp_millis(), -667));
        assert_eq!(landung_widerspricht_meldung(&st), None, "gleich");
        st.gemeldete_landung = Some((td.timestamp_millis() + 1_500, -667));
        assert_eq!(landung_widerspricht_meldung(&st), None, "±2 s wie der Recorder");
        st.gemeldete_landung = Some((td.timestamp_millis(), -55));
        assert!(landung_widerspricht_meldung(&st).is_some(), "andere Sinkrate");
        st.gemeldete_landung = Some((td.timestamp_millis() - 960_000, -667));
        assert!(landung_widerspricht_meldung(&st).is_some(), "anderer Aufsetzer");
    }

    /// Die Live-Meldung eines abgebrochenen Aufsetzers ist nicht die
    /// Landung: nicht nachschicken, nicht vergleichen.
    #[test]
    fn abschluss_vergisst_die_live_meldung_des_abgebrochenen_aufsetzers() {
        let flight = fixture();
        let td = t0();
        gerade_aufgesetzt(&flight, td);
        let mut st = flight.stats.lock().unwrap();
        st.gemeldete_landung = Some((td.timestamp_millis(), -667));
        st.landung_live_bestaetigt = false;
        let s = snap(2_000.0, 400.0, 900.0, 140.0, false, 2);
        landung_beim_wegsteigen_abschliessen(&mut st, &s, td + chrono::Duration::seconds(40));
        assert!(st.landung_payload.is_none());
        assert_eq!(st.gemeldete_landung, None);
        assert_eq!(landung_widerspricht_meldung(&st), None);
    }

    /// Wind, Eigengeschwindigkeit und Sprit gehoeren zum abgebrochenen
    /// Aufsetzer — die naechste Landung rechnet mit ihren eigenen.
    #[test]
    fn abschluss_raeumt_wind_und_sprit_ab() {
        let flight = fixture();
        let td = t0();
        gerade_aufgesetzt(&flight, td);
        let mut st = flight.stats.lock().unwrap();
        st.landing_wind_direction_deg = Some(70.0);
        st.landing_wind_speed_kt = Some(7.0);
        st.landing_true_airspeed_kt = Some(135.0);
        st.landing_fuel_kg = Some(3_340.0);
        st.landing_fuel_puffer = true;
        let s = snap(2_000.0, 400.0, 900.0, 140.0, false, 2);
        landung_beim_wegsteigen_abschliessen(&mut st, &s, td + chrono::Duration::seconds(40));
        assert_eq!(st.landing_wind_direction_deg, None);
        assert_eq!(st.landing_wind_speed_kt, None);
        assert_eq!(st.landing_true_airspeed_kt, None);
        assert_eq!(st.landing_fuel_kg, None);
        assert!(!st.landing_fuel_puffer);
    }

    /// Kinematik aus dem echten Flug (EWG 1599, 20.10.2026, 15:34–15:57Z),
    /// ohne Kennungen: Aufsetzer −667 fpm, 26 s Bodenlauf, Durchstart,
    /// Platzrunde, Landung −55 fpm.
    fn ewg1599_positionen() -> Vec<(DateTime<Utc>, SimSnapshot)> {
        let gz: &[u8] = include_bytes!("../../tests/fixtures/td_balked_after_ground_roll.jsonl.gz");
        let basis = serde_json::to_value(SimSnapshot::default()).expect("default");
        BufReader::new(GzDecoder::new(gz))
            .lines()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(&l.expect("Zeile")).expect("JSON");
                let mut s = basis.clone();
                for (k, x) in v.as_object().expect("Objekt") {
                    if k != "timestamp" {
                        s[k] = x.clone();
                    }
                }
                let at = DateTime::parse_from_rfc3339(v["timestamp"].as_str().expect("Zeit"))
                    .expect("rfc3339")
                    .with_timezone(&Utc);
                (at, serde_json::from_value(s).expect("SimSnapshot"))
            })
            .collect()
    }

    #[test]
    fn ewg1599_zwei_aufsetzer_zwei_episoden() {
        let flight = fixture();
        let pos = ewg1599_positionen();
        {
            let mut st = flight.stats.lock().unwrap();
            st.phase = FlightPhase::Descent;
            st.shadow_phase = Some(FlightPhase::Descent);
            st.was_airborne = true;
            st.was_on_ground = Some(false);
        }
        let mut wechsel = Vec::new();
        let mut luft_in_landing_s = 0_i64;
        let mut laengste_luft_in_landing_s = 0_i64;
        let mut vorher: Option<DateTime<Utc>> = None;
        for (at, s) in &pos {
            let takt = phasen_takt(&flight, s, *at);
            let phase = flight.stats.lock().unwrap().phase;
            if let Some(p) = takt.wechsel {
                wechsel.push((*at, p));
            }
            if phase == FlightPhase::Landing && !s.on_ground && s.altitude_agl_ft > 300.0 {
                luft_in_landing_s += vorher.map(|v| (*at - v).num_seconds()).unwrap_or(0);
                laengste_luft_in_landing_s = laengste_luft_in_landing_s.max(luft_in_landing_s);
            } else {
                luft_in_landing_s = 0;
            }
            vorher = Some(*at);
        }
        let zeit = |hms: &str| {
            DateTime::parse_from_rfc3339(&format!("2026-10-20T{hms}Z")).unwrap().with_timezone(&Utc)
        };
        let abheben = wechsel
            .iter()
            .find(|(at, p)| *p == FlightPhase::Climb && *at > zeit("15:39:30"))
            .unwrap_or_else(|| panic!("kein Wechsel Landing→Climb nach dem ersten Aufsetzer: {wechsel:?}"));
        assert!(
            abheben.0 >= zeit("15:40:03") && abheben.0 <= zeit("15:40:07"),
            "Landing→Climb um {} statt kurz nach 100 ft (15:40:03)",
            abheben.0
        );
        assert!(laengste_luft_in_landing_s < 10, "Landing in der Luft: {laengste_luft_in_landing_s} s");
        assert!(
            wechsel.iter().any(|(at, p)| *p == FlightPhase::Landing && *at > zeit("15:55:00")),
            "zweite Landung erkannt: {wechsel:?}"
        );
        assert!(
            wechsel.iter().any(|(at, p)| *p == FlightPhase::TaxiIn && *at > zeit("15:55:00")),
            "Rollen nach der zweiten Landung: {wechsel:?}"
        );
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.go_around_count, 1);
        let arten: Vec<_> = st.touchdown_events.iter().map(|e| e.kind).collect();
        assert_eq!(arten, vec![TouchdownKind::TouchAndGo, TouchdownKind::FinalLanding], "{:?}", st.touchdown_events);
        // Die Sinkraten selbst misst der Aufsetz-Sampler, der im Replay nicht
        // laeuft — beide Aufsetzer gelten hier als nicht gemessen. Die Werte
        // je Eintrag pruefen die Faelle mit gesetzten Messwerten oben.
        assert!(st.touchdown_events[0].timestamp < zeit("15:39:30"));
        assert!(st.touchdown_events[1].timestamp > zeit("15:55:00"));
    }
    /// P1/B3 (Bahnansicht-Befund, 22.10.2026): #1905 (EWG 1599 GCFV 01,
    /// Client 2.1.4) trug in `lateral_samples` zuerst 68 Proben bei
    /// 2560–3656 m und `mess_ende_laengs_m = 3418` — beides aus dem
    /// Touch-and-go davor (#1896). Am Code bestaetigt: 2.1.4 schloss die
    /// Landung beim Wegsteigen nicht ab (`landung_beim_wegsteigen_abschliessen`
    /// fehlt dort); seit 2.1.5 (be1b3f31) laeuft dabei
    /// `clear_approach_stability_and_rollout` → `spur_verwerfen`.
    ///
    /// Dieser Test haelt das an den ECHTEN Proben aus `fixtures/1905.json`
    /// fest und prueft, was der bestehende Kettentest offen liess: Die Spur
    /// ist nicht nur leer, sie ist als frueherer Durchgang gesichert
    /// (Gegenprobe gegen „nie gesetzt“), und das Messfenster-Ende der
    /// ersten Landung ist weg.
    #[test]
    fn touch_and_go_gibt_spur_und_messende_nicht_an_die_naechste_landung() {
        let fall: serde_json::Value = serde_json::from_str(include_str!(
            "../../../src/dev/bahnansicht/fixtures/1905.json"
        ))
        .unwrap();
        let tg_spur: Vec<(f32, f32)> = fall["lateral_samples"]
            .as_array()
            .unwrap()
            .iter()
            .take(68)
            .map(|p| (p["laengs_m"].as_f64().unwrap() as f32, p["quer_m"].as_f64().unwrap() as f32))
            .collect();
        assert_eq!(tg_spur.first(), Some(&(2559.7, -2.8)), "Vorrichtung: Proben aus 1905");
        let mess_ende = fall["mess_ende_laengs_m"].as_f64().unwrap();
        assert!((mess_ende - 3417.97).abs() < 0.01);

        let flight = fixture();
        {
            let mut st = flight.stats.lock().unwrap();
            st.runway_match = Some(runway::RunwayMatch {
                airport_ident: "GCFV".into(),
                runway_ident: "01".into(),
                heading_true_deg: 9.0,
                length_ft: 11155.0,
                width_ft: 251.0,
                surface: "ASP".into(),
                threshold_lat: 28.43,
                threshold_lon: -13.86,
                end_lat: 28.46,
                end_lon: -13.86,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 0.0,
                side: "right".into(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            });
            st.bahn_spur = tg_spur.clone();
            st.bahn_fenster_zu_laengs_m = Some(mess_ende);
            st.bahn_raeum_laengs_m = Some(3500.0);
        }
        durchstart_nach_bodenlauf(&flight, 10, 2);
        let st = flight.stats.lock().unwrap();
        assert_eq!(st.touchdown_events.len(), 1);
        assert!(st.bahn_spur.is_empty(), "Spur des Touch-and-go blieb fuer die naechste Landung");
        assert_eq!(st.bahn_fenster_zu_laengs_m, None, "Messfenster-Ende des Touch-and-go blieb stehen");
        assert_eq!(st.bahn_raeum_laengs_m, None);
        let (bahn, vorige) = st.bahn_vorige_spuren.last().expect("Touch-and-go als frueherer Durchgang");
        assert_eq!(bahn, "GCFV 01");
        assert_eq!(vorige.first(), tg_spur.first(), "gesichert wurde nicht die Spur des Touch-and-go");
        assert!(vorige.len() >= tg_spur.len());
    }
