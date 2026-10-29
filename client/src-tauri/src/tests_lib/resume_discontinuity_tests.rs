    use super::*;

    fn paused_snapshot(
        lat: f64,
        lon: f64,
        altitude_ft: f64,
        fuel_total_kg: f32,
        on_ground: bool,
    ) -> PausedSnapshot {
        PausedSnapshot {
            lat,
            lon,
            heading_deg: 0.0,
            altitude_ft,
            fuel_total_kg,
            zfw_kg: None,
            on_ground,
        }
    }

    fn sim_snapshot(
        lat: f64,
        lon: f64,
        altitude_msl_ft: f64,
        fuel_total_kg: f32,
        on_ground: bool,
    ) -> SimSnapshot {
        SimSnapshot {
            lat,
            lon,
            altitude_msl_ft,
            fuel_total_kg,
            on_ground,
            ..SimSnapshot::default()
        }
    }

    /// Incident-Replay (Joel EWG3552, 2026-07-22): letzter bekannter Stand
    /// vor der Lücke war AIRBOREN (Endanflug, 233 ft AGL, im Sinkflug);
    /// danach stand das Flugzeug plötzlich GEPARKT mit 4117→11800 kg mehr
    /// Fuel im Tank. Diese airborne→ground-Transition MIT Fuel-Anstieg ist
    /// genau der Fall, den `is_impossible_discontinuity` fangen soll — der
    /// QS-Fix (both_grounded-Ausnahme) darf ihn NICHT versehentlich
    /// mit-exemptieren, weil `cur` allein schon on_ground=true ist.
    #[test]
    fn incident_fuel_jump_is_impossible() {
        let prev = paused_snapshot(49.4955, 11.0752, 1200.0, 4117.4, false);
        let cur = sim_snapshot(49.4955, 11.0752, 1027.0, 11800.7, true);
        let d = compute_resume_discontinuity(&prev, &cur, None, None);
        assert!(
            d.fuel_delta_kg > 0.0,
            "fuel delta must be signed positive on increase"
        );
        assert!(
            !d.both_grounded,
            "prev was airborne, so both_grounded must be false"
        );
        assert!(is_impossible_discontinuity(&d));
    }

    /// QS-Fix (Finding 2): ein Fuel-ANSTIEG ist harmlos, wenn das Flugzeug
    /// DURCHGEHEND am Boden stand — das ist ein ganz normales Nachtanken
    /// während einer Pause (z. B. App-Neustart am Gate), keine unmögliche
    /// Simulation. Vorher hätte das fälschlich eskaliert.
    #[test]
    fn ground_refuel_between_pause_and_resume_is_not_impossible() {
        let prev = paused_snapshot(50.0331, 8.5622, 360.0, 3000.0, true);
        let cur = sim_snapshot(50.0331, 8.5622, 360.0, 12000.0, true);
        let d = compute_resume_discontinuity(&prev, &cur, None, None);
        assert!(d.fuel_delta_kg > RESUME_FUEL_JUMP_IMPOSSIBLE_KG);
        assert!(d.both_grounded);
        assert!(!is_impossible_discontinuity(&d));
    }

    /// Normaler Verbrauch während einer Pause (z. B. APU/Triebwerke liefen
    /// weiter) — eine Abnahme, egal wie groß, ist niemals der unmögliche Fall.
    #[test]
    fn normal_fuel_burn_is_not_impossible() {
        let prev = paused_snapshot(50.0, 8.0, 35000.0, 5500.0, false);
        let cur = sim_snapshot(50.01, 8.01, 34950.0, 4300.0, false);
        let d = compute_resume_discontinuity(&prev, &cur, None, None);
        assert!(d.fuel_delta_kg < 0.0);
        assert!(!is_impossible_discontinuity(&d));
    }

    /// Eine sehr große Reposition (> RESUME_DRIFT_EXTREME_NM) ist auch ohne
    /// jeden Fuel-Sprung unmöglich — der Drift-Pfad muss unabhängig feuern
    /// (und darf NICHT durch die both_grounded-Fuel-Ausnahme mit-exemptiert
    /// werden — diese greift ausschließlich beim Fuel-Zweig).
    #[test]
    fn extreme_drift_alone_is_impossible() {
        let prev = paused_snapshot(50.0, 8.0, 35000.0, 5000.0, false);
        // 1° Breite ≈ 60 nm unabhängig von der geografischen Breite (anders
        // als Längengrad, der mit cos(lat) schrumpft) → 5° ≈ 300 nm nördlich,
        // gleicher Fuel-Stand.
        let cur = sim_snapshot(55.0, 8.0, 35000.0, 5000.0, false);
        let d = compute_resume_discontinuity(&prev, &cur, None, None);
        assert!(d.drift_nm > RESUME_DRIFT_EXTREME_NM);
        assert!(is_impossible_discontinuity(&d));
    }

    /// Extreme Drift bleibt unmöglich, auch wenn beide Seiten (zufällig)
    /// on_ground sind — die both_grounded-Ausnahme gilt NUR für den Fuel-
    /// Zweig, ein 300-nm-Sprung am Boden ist genauso unmöglich wie in der Luft.
    #[test]
    fn extreme_drift_stays_impossible_even_when_both_grounded() {
        let prev = paused_snapshot(50.0, 8.0, 400.0, 5000.0, true);
        let cur = sim_snapshot(55.0, 8.0, 400.0, 5000.0, true);
        let d = compute_resume_discontinuity(&prev, &cur, None, None);
        assert!(d.both_grounded);
        assert!(is_impossible_discontinuity(&d));
    }

    /// Ein kleiner, plausibler Sprung (ein paar NM Drift, moderater Verbrauch)
    /// darf NICHT als unmöglich gelten — sonst würde jeder normale Resume
    /// fälschlich eskalieren.
    #[test]
    fn small_plausible_resume_is_not_impossible() {
        let prev = paused_snapshot(50.0, 8.0, 5000.0, 2000.0, false);
        let cur = sim_snapshot(50.02, 8.02, 4950.0, 1950.0, false);
        let d = compute_resume_discontinuity(&prev, &cur, None, None);
        assert!(!is_impossible_discontinuity(&d));
    }
    /// Regel Thomas, 23.09.2026: Nach einem Sim-Absturz darf sich der Pilot
    /// in der Gegend der Unterbrechung neu hinstellen. Was NICHT geht: sich
    /// ans Ziel stellen und den Flug genehmigen lassen.
    /// Eine Unterbrechung von 15 Minuten: lang genug, dass die 46 nm bis
    /// Kairo fliegbar gewesen waeren. Der Sprung muss sich also am Boden
    /// entscheiden, nicht an der Uhr.
    const LUECKE: Option<i64> = Some(900);

    #[test]
    fn sprung_ans_ziel_faellt_auf_reposition_unterwegs_nicht() {
        // EDDN → HECA. Unterbrechung 49 nm vor Kairo, danach am Platz.
        let ziel = Some((30.1219, 31.4056));
        let vorher = paused_snapshot(30.9219, 31.2357, 24523.0, 3876.0, false);
        let am_ziel = sim_snapshot(30.1250, 31.4100, 358.0, 3876.0, true);
        let d = compute_resume_discontinuity(&vorher, &am_ziel, ziel, LUECKE);
        assert!(
            ist_sprung_ans_ziel(&d),
            "vorher {:?} nm, danach {:?} nm",
            d.ziel_vorher_nm,
            d.ziel_nachher_nm
        );
        assert!(is_impossible_discontinuity(&d));

        // Erlaubt: in der Gegend der Unterbrechung neu hingestellt, mit
        // gleichem Sprit — 3 nm neben der letzten Position.
        let daneben = sim_snapshot(30.9500, 31.2600, 24000.0, 3876.0, false);
        let d2 = compute_resume_discontinuity(&vorher, &daneben, ziel, LUECKE);
        assert!(!ist_sprung_ans_ziel(&d2));
        assert!(!is_impossible_discontinuity(&d2));

        // Ebenfalls erlaubt: Abbruch KURZ vor dem Platz (innerhalb der
        // ersten Schwelle) und Neuladen am Platz — dort ist das der
        // normale Weg, nicht der Sprung ans Ziel.
        let kurz_davor = paused_snapshot(30.20, 31.42, 2000.0, 3000.0, false);
        let d3 = compute_resume_discontinuity(&kurz_davor, &am_ziel, ziel, LUECKE);
        assert!(!ist_sprung_ans_ziel(&d3));

        // Ohne bekannte Zielkoordinaten bleibt die Regel stumm.
        let d4 = compute_resume_discontinuity(&vorher, &am_ziel, None, LUECKE);
        assert!(!ist_sprung_ans_ziel(&d4));

        // Erlaubt: SimConnect reisst unterwegs ab, der Simulator laeuft
        // weiter, und die Verbindung kommt erst im Endanflug zurueck —
        // in der Luft, 3 nm vor der Bahn. In 15 Minuten sind 46 nm zu
        // schaffen, also hat der Pilot sie geflogen.
        let im_anflug = sim_snapshot(30.1500, 31.4200, 2000.0, 3000.0, false);
        let d5 = compute_resume_discontinuity(&vorher, &im_anflug, ziel, LUECKE);
        assert!(!ist_sprung_ans_ziel(&d5), "{:?}", d5.ziel_nachher_nm);
        assert!(!is_impossible_discontinuity(&d5));

        // NICHT erlaubt: dieselbe Lage, aber die Verbindung war nur eine
        // Minute weg. 46 nm in 60 s waeren 2760 kt — da hat sich jemand
        // in den Endanflug gestellt (Cloud-QS 23.09.2026, Befund 1).
        let d6 = compute_resume_discontinuity(&vorher, &im_anflug, ziel, Some(60));
        assert!(ist_sprung_ans_ziel(&d6));
        assert!(is_impossible_discontinuity(&d6));
        assert_eq!(sprung_grund(&d6), "ans Ziel gesprungen");

        // Ohne bekannte Luecke bleibt es beim Zweifel fuer den Piloten.
        let d7 = compute_resume_discontinuity(&vorher, &im_anflug, ziel, None);
        assert!(!ist_sprung_ans_ziel(&d7));

        // Erlaubt und haeufig: SimConnect reisst 18 nm vor dem Platz ab, der
        // Pilot fliegt selbst weiter, landet und rollt ans Gate. Danach steht
        // er AM BODEN am Ziel — wie ein Hingestellter. Ihn unterscheidet der
        // Sprit: 900 kg weniger (Cloud-QS 23.09.2026, zweite Runde).
        let kurz_vorher = paused_snapshot(30.42, 31.43, 6000.0, 3876.0, false);
        let gelandet = sim_snapshot(30.1250, 31.4100, 358.0, 2976.0, true);
        let d8 = compute_resume_discontinuity(&kurz_vorher, &gelandet, ziel, Some(600));
        assert!(
            !ist_sprung_ans_ziel(&d8),
            "ehrliche Landung nach Abriss: {:?} nm, Sprit {:+.0}",
            d8.ziel_vorher_nm,
            d8.fuel_delta_kg
        );
        assert!(!is_impossible_discontinuity(&d8));

        // Gegenprobe zur Gegenprobe: dieselbe Lage, aber der Tank steht noch
        // genau so voll wie vorher. Dann sind die Meilen nicht geflogen.
        let gelandet_ohne_verbrauch = sim_snapshot(30.1250, 31.4100, 358.0, 3876.0, true);
        let d9 =
            compute_resume_discontinuity(&kurz_vorher, &gelandet_ohne_verbrauch, ziel, Some(600));
        assert!(ist_sprung_ans_ziel(&d9));
    }
    /// Die Hoehe gehoert zum Filter vor allem anderen: 20.926.040 ft
    /// lieferte MSFS beim Laden (OCN 712), daraus wurde ein vorgetaeuschter
    /// Durchstart. Echte Extreme muessen durch.
    #[test]
    fn unmoegliche_hoehe_ist_kein_brauchbarer_messwert() {
        let mut snap = sim_snapshot(49.5, 11.08, 1_046.0, 3_000.0, true);
        assert!(snapshot_position_is_usable(&snap));
        snap.altitude_msl_ft = 20_926_040.0;
        assert!(!snapshot_position_is_usable(&snap));
        snap.altitude_msl_ft = f64::NAN;
        assert!(!snapshot_position_is_usable(&snap));
        // Totes Meer und Reiseflughoehe bleiben brauchbar.
        snap.altitude_msl_ft = -1_400.0;
        assert!(snapshot_position_is_usable(&snap));
        snap.altitude_msl_ft = 59_000.0;
        assert!(snapshot_position_is_usable(&snap));
    }

    /// GAF 278 (24.10.2026): Die Menue-Scheinposition (LAT 0 / LON 90, 228 ft)
    /// stand minutenlang ruhig — das Gate schaltete darauf scharf, und der
    /// erste echte Wert (Zagreb am Boden) lief ungeprueft in die Phasen.
    #[test]
    fn resume_gate_schaltet_nie_auf_der_menue_position_scharf() {
        let t0 = Utc::now();
        let mut ruhe = SimRuhe::default();
        let menue = sim_snapshot(-0.0, 90.0003, 228.0, 0.0, true);
        for s in (0..=120).step_by(2) {
            let mut snap = menue.clone();
            snap.timestamp = t0 + chrono::Duration::seconds(s);
            assert!(
                resume_gate_takt(&mut ruhe, Some(snap), t0 + chrono::Duration::seconds(s))
                    .is_err(),
                "nach {s} s auf der Menue-Position scharf"
            );
        }
    }

    /// Die Folge von OCN 712 (24.09.2026) durch die echte Gate-Entscheidung:
    /// Menue (anderes Flugzeug, 0 kg, anderer Ort, pausiert), dann ein Wert
    /// mit 20.926.040 ft, dann erst der geladene Flug. Scharf wird es erst,
    /// wenn der geladene Flug 20 s ruhig war.
    #[test]
    fn resume_gate_folgt_der_ocn712_folge() {
        let t0 = Utc::now();
        let jetzt = |s: i64| t0 + chrono::Duration::seconds(s);
        let frisch = |mut snap: SimSnapshot, t: DateTime<Utc>| {
            snap.timestamp = t;
            snap
        };
        let mut ruhe = SimRuhe::default();

        let mut menue = sim_snapshot(47.4, -122.3, 400.0, 0.0, true);
        menue.paused = true;
        menue.aircraft_title = Some("FenixA320 CFM SL".into());
        assert!(resume_gate_takt(&mut ruhe, Some(frisch(menue, jetzt(0))), jetzt(0)).is_err());

        let mut muell = sim_snapshot(49.4952, 11.0779, 20_926_040.0, 9_546.0, false);
        muell.aircraft_title = Some("FenixA320 CFM SL".into());
        assert!(resume_gate_takt(&mut ruhe, Some(frisch(muell, jetzt(4))), jetzt(4)).is_err());

        let mut eddn = sim_snapshot(49.4952, 11.0779, 1_028.0, 9_544.0, true);
        eddn.aircraft_title = Some("FenixA321 CFM WF SC".into());
        // Ruhig, aber noch nicht lange genug.
        for s in (10..=28).step_by(2) {
            assert!(
                resume_gate_takt(&mut ruhe, Some(frisch(eddn.clone(), jetzt(s))), jetzt(s))
                    .is_err(),
                "nach {s} s schon scharf"
            );
        }
        // Nach 20 s Ruhe ist er bereit — mit dem GELADENEN Flug als Ausgangslage.
        let bereit = resume_gate_takt(&mut ruhe, Some(frisch(eddn.clone(), jetzt(32))), jetzt(32))
            .expect("nach 20 s Ruhe bereit");
        assert!((bereit.altitude_msl_ft - 1_028.0).abs() < 1.0);

        // Ein Muellwert, der sonst ruhig aussaehe (gleicher Ort, gleiches
        // Flugzeug, nur die Hoehe unmoeglich), darf weder Ruhe zaehlen noch
        // Ausgangslage werden — dafuer steht der Filter im Gate.
        let mut ruhe = SimRuhe::default();
        for s in (0..=18).step_by(2) {
            let _ = resume_gate_takt(&mut ruhe, Some(frisch(eddn.clone(), jetzt(s))), jetzt(s));
        }
        let mut hoehe_muell = eddn.clone();
        hoehe_muell.altitude_msl_ft = 20_926_040.0;
        assert!(
            resume_gate_takt(&mut ruhe, Some(frisch(hoehe_muell, jetzt(20))), jetzt(20)).is_err(),
            "unmoegliche Hoehe als Ausgangslage"
        );
        assert!(
            resume_gate_takt(&mut ruhe, Some(frisch(eddn.clone(), jetzt(22))), jetzt(22)).is_err(),
            "die Ruhe muss nach dem Muellwert von vorn beginnen"
        );

        // Ein eingefrorener (veralteter) Wert zaehlt nicht als Ruhe.
        let mut ruhe = SimRuhe::default();
        let alt = frisch(eddn.clone(), jetzt(0));
        for s in (0..=40).step_by(2) {
            assert!(resume_gate_takt(&mut ruhe, Some(alt.clone()), jetzt(s + 10)).is_err());
        }

        // Ein Menuewert, der NICHT als Pause gemeldet ist, am selben Ort,
        // aber mit anderem Flugzeug: Die Titelpruefung muss die Ruhe
        // zuruecksetzen (Claude-QS 24.09.2026).
        let mut ruhe = SimRuhe::default();
        for s in (0..=18).step_by(2) {
            let _ = resume_gate_takt(&mut ruhe, Some(frisch(eddn.clone(), jetzt(s))), jetzt(s));
        }
        let mut anderes = eddn.clone();
        anderes.aircraft_title = Some("FenixA320 CFM SL".into());
        assert!(
            resume_gate_takt(&mut ruhe, Some(frisch(anderes, jetzt(20))), jetzt(20)).is_err(),
            "Flugzeugwechsel als ruhig gewertet"
        );
    }

    /// Nach einem Neustart im Reiseflug unter Zeitbeschleunigung muss das
    /// Gate scharf werden — ohne die Sim-Rate hielt es jeden Takt fuer einen
    /// Sprung (Cloud-QS 24.09.2026).
    #[test]
    fn resume_gate_schaltet_auch_unter_zeitbeschleunigung_scharf() {
        let t0 = Utc::now();
        let mut ruhe = SimRuhe::default();
        let mut snap = sim_snapshot(50.0, 8.0, 36_000.0, 12_000.0, false);
        snap.groundspeed_kt = 480.0;
        snap.simulation_rate = 4.0;
        let mut ergebnis = Err(String::new());
        // Genau bis t = 20 s: Nur MIT Sim-Rate ist schon der erste Takt ruhig.
        // Ohne sie rettet erst die Gleichmaessigkeits-Regel ab dem zweiten, und
        // die 20 s sind noch nicht voll (Claude-QS 24.09.2026).
        for i in 0..=10 {
            let t = t0 + chrono::Duration::seconds(i * 2);
            let mut s = snap.clone();
            // 4x: rund 1975 m je 2 s Echtzeit.
            s.lat += (i as f64) * 1_975.0 / 111_320.0;
            s.timestamp = t;
            ergebnis = resume_gate_takt(&mut ruhe, Some(s), t);
        }
        assert!(ergebnis.is_ok(), "unter 4x nie scharf: {ergebnis:?}");
    }

    /// Ohne passende Sim-Rate im Snapshot: X-Plane vor v1.9.12 meldete fest
    /// 1.0; seither `sim_speed` × `ground_speed_flt` (Sollwerte — ein von der
    /// Bildrate gebremster Zeitraffer bleibt darin unsichtbar), ohne die
    /// Datarefs wieder 1.0. Unter Zeitbeschleunigung bewegt sich das Flugzeug
    /// trotzdem gleichmaessig schnell — das muss als ruhig gelten. Ein Teleport danach bleibt ein Sprung
    /// (Cloud-QS 24.09.2026, dritte Runde).
    #[test]
    fn resume_gate_ohne_gemeldete_sim_rate() {
        let t0 = Utc::now();
        let mut ruhe = SimRuhe::default();
        let mut snap = sim_snapshot(50.0, 8.0, 36_000.0, 12_000.0, false);
        snap.groundspeed_kt = 480.0;
        snap.simulation_rate = 1.0; // X-Plane
        let mut ergebnis = Err(String::new());
        let mut letzte = snap.clone();
        for i in 0..=14 {
            let t = t0 + chrono::Duration::seconds(i * 2);
            let mut s = snap.clone();
            s.lat += (i as f64) * 1_975.0 / 111_320.0;
            s.timestamp = t;
            letzte = s.clone();
            ergebnis = resume_gate_takt(&mut ruhe, Some(s), t);
        }
        assert!(
            ergebnis.is_ok(),
            "gleichmaessiger Flug unter 4x nie scharf: {ergebnis:?}"
        );

        // Ein Teleport danach ist trotzdem ein Sprung.
        let mut ruhe2 = SimRuhe::default();
        for i in 0..=3 {
            let t = t0 + chrono::Duration::seconds(i * 2);
            let mut s = snap.clone();
            s.lat += (i as f64) * 1_975.0 / 111_320.0;
            s.timestamp = t;
            let _ = ruhe2.pruefen(&s, t);
        }
        let mut weit = letzte;
        weit.lat += 3.0; // ~330 km
        let t = t0 + chrono::Duration::seconds(8);
        weit.timestamp = t;
        let grund = ruhe2.pruefen(&weit, t);
        assert!(
            grund.as_deref().is_some_and(|g| g.contains("springt")),
            "{grund:?}"
        );

        // Zwei gleich grosse Teleports hintereinander (Menue-Weltkarte): Der
        // zweite darf nicht als „gleichmaessig" durchgehen — dafuer ist der
        // Deckel auf das Fliegbare da.
        let mut noch_weiter = weit.clone();
        noch_weiter.lat += 3.0;
        let t = t0 + chrono::Duration::seconds(10);
        noch_weiter.timestamp = t;
        let grund = ruhe2.pruefen(&noch_weiter, t);
        assert!(
            grund.as_deref().is_some_and(|g| g.contains("springt")),
            "zweiter Teleport als gleichmaessig durchgewunken: {grund:?}"
        );

        // Nach einer langen Unterbrechung (60 s ohne Takt, z. B. App Nap)
        // gilt der vorige Abstand nicht als Vergleich — sonst wuechse der
        // Deckel mit der Zeit, und ein zweiter Teleport gleicher Groesse
        // ginge als gleichmaessig durch (Cloud-QS 24.09.2026).
        let mut ruhe3 = SimRuhe::default();
        let mut a = snap.clone();
        a.timestamp = t0;
        let _ = ruhe3.pruefen(&a, t0);
        let mut b = a.clone();
        b.lat += 3.0;
        let tb = t0 + chrono::Duration::seconds(2);
        let _ = ruhe3.pruefen(&b, tb);
        let mut c = b.clone();
        c.lat += 3.0;
        let tc = tb + chrono::Duration::seconds(60);
        let grund = ruhe3.pruefen(&c, tc);
        assert!(
            grund.as_deref().is_some_and(|g| g.contains("springt")),
            "Teleport nach langer Pause als gleichmaessig durchgewunken: {grund:?}"
        );
    }

    /// Ein zweites Bestaetigen nach dem Scharfschalten startet kein neues
    /// Gate; ein nach Sim-Absturz pausierter Flug laesst sich aber weiter
    /// freigeben (Codex-QS 25.09.2026).
    #[test]
    fn fortsetzen_nach_dem_gate_ist_ein_noop() {
        let flight = crate::enroute_reconcile_replay_tests::replay_fixture();
        // Frisch wiederhergestellt: Streamer laeuft noch nicht.
        flight.streamer_spawned.store(false, Ordering::SeqCst);
        flight.was_just_resumed.store(true, Ordering::SeqCst);
        assert!(!fortsetzen_laeuft_schon(&flight));
        // Nach der Positionspruefung, noch vor dem Gate.
        flight.was_just_resumed.store(false, Ordering::SeqCst);
        assert!(!fortsetzen_laeuft_schon(&flight));
        // Gate hat scharf geschaltet.
        flight.streamer_spawned.store(true, Ordering::SeqCst);
        assert!(fortsetzen_laeuft_schon(&flight));
        // Sim-Absturz mitten im Flug: Streamer pausiert, Pilot muss freigeben.
        flight.was_just_resumed.store(true, Ordering::SeqCst);
        assert!(!fortsetzen_laeuft_schon(&flight));

        // Und der Befehl fragt das, bevor er ein Gate startet.
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find(concat!("\nasync fn flight_resume_", "confirm("))
            .expect("flight_resume_confirm nicht gefunden");
        let koerper = &SRC[start..start + SRC[start..].find("\n}\n").unwrap()];
        let frage = koerper
            .find(concat!("if fortsetzen_laeuft_", "schon(&flight)"))
            .expect("Befehl fragt nicht mehr, ob der Flug schon laeuft");
        let gate = koerper
            .find(concat!("spawn_resume_", "sim_gate("))
            .expect("Gate-Start fehlt");
        assert!(frage < gate);
    }

    /// Jeder Ausgang des Resume-Gates raeumt den Wartegrund weg — sonst
    /// bleibt im Cockpit ein eingefrorener Hinweis stehen
    /// (Cloud-QS 24.09.2026, dritte Runde).
    #[test]
    fn resume_gate_raeumt_den_wartegrund_bei_jedem_ausgang() {
        // Verhalten: Der Guard raeumt beim Verlassen, egal auf welchem Weg.
        let flight = Arc::new(crate::enroute_reconcile_replay_tests::replay_fixture());
        {
            let mut st = flight.stats.lock().unwrap();
            st.resume_wartet = Some(("Laedt".into(), 12));
            st.resume_gate_laeuft = true;
        }
        {
            let _g = WartegrundRaeumen(Arc::clone(&flight));
        }
        assert!(flight.stats.lock().unwrap().resume_wartet.is_none());
        // Und das Gate gibt sich frei — sonst waere nach einem Ende nie
        // wieder ein Fortsetzen moeglich.
        assert!(!flight.stats.lock().unwrap().resume_gate_laeuft);

        // Und das Gate setzt den Guard, bevor es irgendetwas anderes tut.
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find(concat!("\nfn spawn_resume_", "sim_gate("))
            .expect("Resume-Gate nicht gefunden — Test anpassen, nicht loeschen");
        let koerper = &SRC[start..start + SRC[start..].find("\n}\n").unwrap()];
        // Benannt gebunden — `let _ = …` droppte den Guard SOFORT, und der
        // Wartegrund bliebe fuer immer stehen (Cloud-QS 24.09.2026).
        let guard = koerper
            .find(concat!("let _wartegrund_raeumen = ", "WartegrundRaeumen("))
            .expect("Gate bindet den Raeum-Guard nicht mehr benannt");
        let schleife = koerper.find("loop {").expect("Gate-Schleife fehlt");
        assert!(guard < schleife, "Guard muss vor der Schleife stehen");

        // Waehrend des Wartens geht ein Heartbeat an phpVMS — sonst loescht
        // sein Cron den PIREP nach ~2 h (Claude-QS 24.09.2026).
        let warte_zweig = &koerper[koerper.find("Err(grund) =>").expect("Warte-Zweig fehlt")
            ..koerper.find("Ok(snap) =>").expect("Scharf-Zweig fehlt")];
        assert!(
            warte_zweig.contains(concat!(".update_", "pirep(")),
            "Gate schickt beim Warten keinen Heartbeat mehr"
        );
        // In einem eigenen Task — sonst bremst ein haengendes phpVMS das Gate.
        let hb = warte_zweig.find(concat!(".update_", "pirep(")).unwrap();
        let task = warte_zweig
            .find("async_runtime::spawn(")
            .expect("Heartbeat nicht im eigenen Task");
        assert!(
            task < hb,
            "Heartbeat laeuft wieder direkt in der Gate-Schleife"
        );
        // Ein geloeschter PIREP beendet den Flug, wie im Streamer.
        assert!(
            warte_zweig.contains("ApiError::NotFound")
                && warte_zweig.contains("handle_remote_cancellation("),
            "404 im Gate-Heartbeat wird nicht behandelt"
        );
        // Ohne Leerraum verglichen — rustfmt bricht die Bedingung um.
        let ohne_leerraum: String = warte_zweig.split_whitespace().collect();
        assert!(
            ohne_leerraum.contains(concat!(
                "Err(ApiError::NotFound)if!flight_hb.stop",
                ".load("
            )),
            "ein verspaeteter 404 trifft auch einen schon beendeten Flug"
        );
        // Die Uhr laeuft ab dem Versuch, nicht erst ab dem Erfolg.
        let uhr = warte_zweig
            .find("letzter_heartbeat = std::time::Instant::now();")
            .expect("Heartbeat-Uhr fehlt");
        assert!(uhr < task, "Uhr wird erst nach dem Erfolg gesetzt");
    }

    /// Ein Durchstart nach dem Aufsetzen ist nur einer, wenn das Flugzeug
    /// wirklich steigt. Messmuell darf ihn nicht vortaeuschen — sonst ist
    /// der Landesprit weg (Codex-QS 24.09.2026).
    #[test]
    fn steigflug_nach_aufsetzen_erkennt_messmuell() {
        let mut echt = sim_snapshot(49.5, 11.08, 1_300.0, 2_900.0, false);
        echt.altitude_agl_ft = 300.0;
        assert!(echter_steigflug_nach_aufsetzen(&echt));
        // GSX-Pushback-Fehler: am Boden, AGL 53.819 ft.
        let mut gsx = sim_snapshot(49.5, 11.08, 1_046.0, 2_900.0, true);
        gsx.altitude_agl_ft = 53_819.0;
        assert!(!echter_steigflug_nach_aufsetzen(&gsx));
        // Derselbe Fehler „in der Luft": AGL passt nicht zur Hoehe.
        gsx.on_ground = false;
        assert!(!echter_steigflug_nach_aufsetzen(&gsx));
        // Am Boden mit plausiblem, aber falschem AGL (Szenerie-Versatz): kein
        // Steigflug, auch wenn AGL > 100 ft und zur Hoehe passt.
        let mut boden = sim_snapshot(49.5, 11.08, 1_200.0, 2_900.0, true);
        boden.altitude_agl_ft = 150.0;
        assert!(!echter_steigflug_nach_aufsetzen(&boden));
        // Hochgebirge bleibt erkannt (Daocheng, Gelaende 14.472 ft).
        let mut daocheng = sim_snapshot(29.3, 100.05, 15_000.0, 2_900.0, false);
        daocheng.altitude_agl_ft = 528.0;
        assert!(echter_steigflug_nach_aufsetzen(&daocheng));
        // Die Gelaendespanne als Paar: 30.000 ft gilt, 30.001 nicht.
        let mut grenze = sim_snapshot(29.3, 100.05, 30_300.0, 2_900.0, false);
        grenze.altitude_agl_ft = 300.0;
        assert!(echter_steigflug_nach_aufsetzen(&grenze));
        grenze.altitude_msl_ft = 30_301.0;
        assert!(!echter_steigflug_nach_aufsetzen(&grenze));
        // Gueltige MSL-Hoehe, aber das Gelaende laege auf 99.790 ft.
        let mut hoch = sim_snapshot(49.5, 11.08, 100_000.0, 2_900.0, false);
        hoch.altitude_agl_ft = 210.0;
        assert!(!echter_steigflug_nach_aufsetzen(&hoch));
        // OCN 712: AGL 210 ft bei 20.926.040 ft MSL — gar keine Messung.
        let mut menue = sim_snapshot(49.5, 11.08, 20_926_040.0, 9_546.0, false);
        menue.altitude_agl_ft = 210.0;
        assert!(!echter_steigflug_nach_aufsetzen(&menue));
    }

    /// Die Wartezeit im Gate zaehlt fuer die Abgabe-Sperre ganz, fuer die
    /// Sprung-Pruefung nur als ruhige Endphase (Claude-QS 24.09.2026).
    #[test]
    fn wartezeit_zaehlt_fuer_sperre_ganz_fuer_sprungpruefung_kaum() {
        let t0 = Utc::now();
        let mut st = FlightStats::new();
        st.resume_luecke_secs = Some(30);
        luecke_aufschlagen(&mut st, Some(t0), t0 + chrono::Duration::seconds(380));
        assert_eq!(
            st.resume_luecke_secs,
            Some(30 + AUTO_START_SIM_RUHE_SECS + 10)
        );
        assert_eq!(st.resume_luecke_max_secs, Some(410));
        // 37 nm in der so gerechneten Luecke sind nicht fliegbar.
        let d = ResumeDiscontinuity {
            drift_nm: 37.0,
            altitude_delta_ft: 0.0,
            fuel_delta_kg: -50.0,
            both_grounded: false,
            ziel_vorher_nm: Some(40.0),
            ziel_nachher_nm: Some(3.0),
            danach_am_boden: false,
            luecke_secs: st.resume_luecke_secs,
            nur_bestaetigt: false,
        };
        assert!(
            ist_sprung_ans_ziel(&d),
            "37 nm in {:?} s als fliegbar gewertet",
            d.luecke_secs
        );
        // Eine unbekannte Luecke bleibt fuer die Sprung-Pruefung unbekannt —
        // fuer die Abgabe-Sperre zaehlt die Wartezeit trotzdem mit.
        let mut alt = FlightStats::new();
        alt.resume_luecke_max_secs = Some(60);
        luecke_aufschlagen(&mut alt, Some(t0), t0 + chrono::Duration::seconds(90));
        assert_eq!(alt.resume_luecke_secs, None);
        assert_eq!(alt.resume_luecke_max_secs, Some(150));
    }

    /// Am Boden gilt die feste Schwelle — egal, welche Geschwindigkeit ein
    /// Ladewert meldet (Codex-QS 24.09.2026).
    #[test]
    fn ruhe_regel_am_boden_bleibt_streng() {
        let t0 = Utc::now();
        let mut ruhe = SimRuhe::default();
        let mut a = sim_snapshot(50.0, 8.0, 300.0, 5_000.0, true);
        a.groundspeed_kt = 700.0;
        a.simulation_rate = 16.0;
        let _ = ruhe.pruefen(&a, t0);
        for i in 1..=5 {
            let mut b = a.clone();
            b.lat += (i as f64) * 20_000.0 / 111_320.0;
            let grund = ruhe.pruefen(&b, t0 + chrono::Duration::seconds(3 * i));
            assert!(
                grund.as_deref().is_some_and(|g| g.contains("springt")),
                "20-km-Sprung am Boden als ruhig gewertet (Takt {i}): {grund:?}"
            );
        }
        // Flackern zwischen zwei Punkten 5 km auseinander: jeder Wechsel ein Sprung.
        let mut ruhe = SimRuhe::default();
        let p = sim_snapshot(50.0, 8.0, 300.0, 5_000.0, true);
        let mut q = p.clone();
        q.lat += 5_000.0 / 111_320.0;
        let _ = ruhe.pruefen(&p, t0);
        for i in 1..=6 {
            let x = if i % 2 == 1 { &q } else { &p };
            let grund = ruhe.pruefen(x, t0 + chrono::Duration::seconds(3 * i));
            assert!(
                grund.as_deref().is_some_and(|g| g.contains("springt")),
                "Takt {i}: {grund:?}"
            );
        }
    }

    /// Hoch fliegende Add-ons (Concorde, Darkstar) sind kein Messmuell.
    #[test]
    fn hohe_flughoehe_ist_brauchbar() {
        let mut snap = sim_snapshot(50.0, 8.0, 100_000.0, 5_000.0, false);
        assert!(snapshot_position_is_usable(&snap));
        // Die Grenze selbst als Paar — eine versehentlich tiefere fiele auf.
        snap.altitude_msl_ft = SNAPSHOT_HOEHE_MAX_FT;
        assert!(snapshot_position_is_usable(&snap));
        assert_eq!(SNAPSHOT_HOEHE_MAX_FT, 150_000.0);
        snap.altitude_msl_ft = SNAPSHOT_HOEHE_MAX_FT + 1.0;
        assert!(!snapshot_position_is_usable(&snap));
    }

    /// Ohne bekannten Aufsetzzeitpunkt gibt es keine Zeitgrenze — im Zweifel
    /// gilt der echte Wert. Mit dem Zeitpunkt des Aufsetz-Samplers greift sie.
    #[test]
    fn verbrauchsgrenze_braucht_eine_zeitbasis() {
        let t0 = Utc::now();
        let t = |s: i64| t0 + chrono::Duration::seconds(s);
        let ldg = Some(16_770.0_f32);
        let f = tankstand_nach_landung_begrenzen;
        // Unbekannte Zeit: echter Rollverbrauch von 649 kg gilt.
        assert_eq!(f(16_121.0, ldg, None, None, 2_000.0, t(30)), 16_121.0);
        // Unbekannte Zeit, aber Neubeladung und fast leerer Tank: weiter nie.
        assert_eq!(
            f(30_000.0, ldg, Some(16_121.0), None, 2_000.0, t(30)),
            16_121.0
        );
        assert_eq!(f(8.0, ldg, Some(16_121.0), None, 2_000.0, t(30)), 16_121.0);
        // Mit Zeitbasis (z. B. vom Sampler): 649 kg in 3 s sind unmoeglich.
        assert_eq!(
            f(16_121.0, ldg, Some(16_770.0), Some(t(0)), 2_000.0, t(3)),
            16_770.0
        );
    }

    /// Nach der Landung: erst ein Menue-Wert mit 0 kg, dann die Neubeladung.
    /// Der Endsprit bleibt beim echten Wert — er klemmt weder auf 0 noch
    /// springt er auf die neue Beladung (Cloud-QS 24.09.2026).
    #[test]
    fn endsprit_uebersteht_menue_und_neubeladung() {
        let t0 = Utc::now();
        let t = |s: i64| t0 + chrono::Duration::seconds(s);
        let ldg = Some(2_966.0_f32);
        // A321 wie OCN 712: 8951 kg Block, 2966 kg Landesprit, 2 h 11 min.
        let rate = bodenverbrauch_kg_pro_h(Some(8_951.0), ldg, Some(t(-7_860)), Some(t(0)));
        let f = tankstand_nach_landung_begrenzen;
        // Menue-Wert (8 kg wie bei OCN 712): verworfen, auch nach Stunden.
        assert_eq!(f(8.0, ldg, Some(2_940.0), Some(t(0)), rate, t(3)), 2_940.0);
        assert_eq!(
            f(8.0, ldg, Some(2_940.0), Some(t(0)), rate, t(36_000)),
            2_940.0
        );
        // Neubeladung: nie uebernommen.
        assert_eq!(
            f(9_546.0, ldg, Some(2_940.0), Some(t(0)), rate, t(6)),
            2_940.0
        );
        // Normales Rollen: wenige kg weniger werden uebernommen.
        assert_eq!(
            f(2_931.0, ldg, Some(2_940.0), Some(t(0)), rate, t(30)),
            2_931.0
        );
        // Vor der Landung gilt alles wie gemessen.
        assert_eq!(f(9_546.0, None, Some(2_940.0), None, rate, t(3)), 9_546.0);
    }

    /// Nach einer Luecke nach der Landung (APU lief weiter) stehen 400 kg
    /// weniger im Tank. Das ist echt und muss irgendwann gelten — der
    /// Endsprit darf nicht fuer immer auf dem alten Wert haengen
    /// (Cloud-QS 24.09.2026, dritte Runde).
    #[test]
    fn echter_abfall_passt_zur_verstrichenen_zeit() {
        let t0 = Utc::now();
        let t = |s: i64| t0 + chrono::Duration::seconds(s);
        let ldg = Some(2_966.0_f32);
        // A321 wie OCN 712: 8951 kg Block, 2966 kg Landesprit, 2 h 11 min.
        let rate = bodenverbrauch_kg_pro_h(Some(8_951.0), ldg, Some(t(-7_860)), Some(t(0)));
        let f = tankstand_nach_landung_begrenzen;
        // Rund 960 kg/h am Boden erlaubt. 250 kg nach 20 Minuten Rollen: echt.
        assert_eq!(
            f(2_716.0, ldg, Some(2_940.0), Some(t(0)), rate, t(1_200)),
            2_716.0
        );
        // Dieselben 250 kg drei Sekunden nach dem Aufsetzen: unmoeglich.
        assert_eq!(
            f(2_716.0, ldg, Some(2_940.0), Some(t(0)), rate, t(3)),
            2_940.0
        );
    }

    /// Eine Cessna mit 60 kg Landesprit. Mit festen 200 kg Reserve ging hier
    /// JEDER Abfall durch — die Grenze muss mit dem Flugzeug schrumpfen
    /// (Cloud-QS 24.09.2026, fuenfte Runde).
    #[test]
    fn verbrauchsgrenze_passt_zum_kleinen_flugzeug() {
        let t0 = Utc::now();
        let t = |s: i64| t0 + chrono::Duration::seconds(s);
        let ldg = Some(60.0_f32);
        // 1 h Flug, 30 kg verbraucht.
        let rate = bodenverbrauch_kg_pro_h(Some(90.0), ldg, Some(t(-3_600)), Some(t(0)));
        assert!(
            rate < 15.0,
            "Cessna-Bodenverbrauch unplausibel hoch: {rate}"
        );
        let f = tankstand_nach_landung_begrenzen;
        // Ein Menue-Wert von 30 kg zehn Minuten nach der Landung: Muell.
        assert_eq!(f(30.0, ldg, Some(59.0), Some(t(0)), rate, t(600)), 59.0);
        // 3 kg weniger nach zehn Minuten Rollen: echt.
        assert_eq!(f(57.0, ldg, Some(59.0), Some(t(0)), rate, t(600)), 57.0);
    }

    /// Viele kleine Schritte duerfen sich nicht zu einem unmoeglichen Abfall
    /// summieren — die Grenze gilt fuer die SUMME seit der Landung, nicht je
    /// Messung (Cloud-QS 24.09.2026, fuenfte Runde).
    #[test]
    fn verbrauchsgrenze_gilt_fuer_die_summe() {
        let t0 = Utc::now();
        let t = |s: i64| t0 + chrono::Duration::seconds(s);
        let ldg = Some(2_966.0_f32);
        // A321 wie OCN 712: 8951 kg Block, 2966 kg Landesprit, 2 h 11 min.
        let rate = bodenverbrauch_kg_pro_h(Some(8_951.0), ldg, Some(t(-7_860)), Some(t(0)));
        let f = tankstand_nach_landung_begrenzen;
        let mut stand = 2_966.0;
        for i in 1..=20 {
            stand = f(
                2_966.0 - 100.0 * i as f32,
                ldg,
                Some(stand),
                Some(t(0)),
                rate,
                t(3 * i as i64),
            );
        }
        assert!(
            stand > 2_966.0 - 200.0,
            "20 Schritte a 100 kg in einer Minute durchgewunken: {stand}"
        );
    }

    /// Der Fall, an dem die erste Fassung scheiterte: ein Muellwert, dann
    /// eine Pause OHNE Messung, dann derselbe Muellwert. Die Pause darf ihn
    /// nicht glaubwuerdig machen — fuenf Minuten reichen nicht fuer 2932 kg
    /// (Cloud-QS 24.09.2026, vierte Runde).
    #[test]
    fn muell_wird_durch_eine_pause_nicht_glaubwuerdig() {
        let t0 = Utc::now();
        let t = |s: i64| t0 + chrono::Duration::seconds(s);
        let ldg = Some(2_966.0_f32);
        // A321 wie OCN 712: 8951 kg Block, 2966 kg Landesprit, 2 h 11 min.
        let rate = bodenverbrauch_kg_pro_h(Some(8_951.0), ldg, Some(t(-7_860)), Some(t(0)));
        let f = tankstand_nach_landung_begrenzen;
        // Ein Menue-Wert von 500 kg (nicht „fast leer"), vor und nach fuenf
        // Minuten Pause ohne Messung: 2466 kg in fuenf Minuten sind unmoeglich.
        assert_eq!(
            f(500.0, ldg, Some(2_940.0), Some(t(0)), rate, t(3)),
            2_940.0
        );
        assert_eq!(
            f(500.0, ldg, Some(2_940.0), Some(t(0)), rate, t(303)),
            2_940.0
        );
    }

    /// Die Ruhe-Regel des Resume-Gates: Ein Teleport ist Unruhe, ein
    /// schnelles Flugzeug in der Luft nicht (sonst schaltete das Gate nach
    /// einem App-Neustart im Reiseflug nie scharf).
    #[test]
    fn ruhe_regel_kennt_die_geschwindigkeit() {
        let t0 = Utc::now();
        let mut ruhe = SimRuhe::default();
        // Concorde: 1150 kt, 2 s → rund 1180 m, mehr als die Bodenschwelle.
        let mut a = sim_snapshot(50.0, 8.0, 55_000.0, 20_000.0, false);
        a.groundspeed_kt = 1_150.0;
        let mut b = a.clone();
        b.lat += 1_180.0 / 111_320.0;
        let _ = ruhe.pruefen(&a, t0);
        let grund = ruhe.pruefen(&b, t0 + chrono::Duration::seconds(2));
        assert!(
            grund.as_deref().is_none_or(|g| !g.contains("springt")),
            "ehrlicher Flug ist kein Sprung: {grund:?}"
        );
        // Teleport wie beim Laden (Menue-Position 4973 nm entfernt).
        let mut c = b.clone();
        c.lat += 60.0;
        let grund = ruhe.pruefen(&c, t0 + chrono::Duration::seconds(4));
        assert!(
            grund.as_deref().is_some_and(|g| g.contains("springt")),
            "{grund:?}"
        );
        // Ein Ladewert mit unsinniger Geschwindigkeit darf den Teleport
        // nicht durchwinken (GS ist gedeckelt).
        let mut ruhe = SimRuhe::default();
        let mut d = a.clone();
        d.groundspeed_kt = 1_000_000.0;
        let _ = ruhe.pruefen(&d, t0);
        let mut e = d.clone();
        // ~556 km: ohne Deckel in 2 s „erlaubt", mit Deckel ein Sprung.
        e.lat += 5.0;
        let grund = ruhe.pruefen(&e, t0 + chrono::Duration::seconds(2));
        assert!(
            grund.as_deref().is_some_and(|g| g.contains("springt")),
            "{grund:?}"
        );
        // Am Boden gilt weiter die feste Schwelle.
        let mut ruhe = SimRuhe::default();
        let boden = sim_snapshot(50.0, 8.0, 300.0, 5_000.0, true);
        let mut weg = boden.clone();
        weg.lat += 1_500.0 / 111_320.0;
        let _ = ruhe.pruefen(&boden, t0);
        let grund = ruhe.pruefen(&weg, t0 + chrono::Duration::seconds(2));
        assert!(
            grund.as_deref().is_some_and(|g| g.contains("springt")),
            "{grund:?}"
        );
    }

    /// Der Riegel auf die Begruendung: eine Zeile, hoechstens 500 Zeichen.
    /// Ueber die LAN-Bruecke kommt ungepruefter Text an — mit Umbruechen
    /// liesse sich sonst eine zweite „Begründung:"- oder „DIVERT:"-Zeile
    /// faelschen. Dieser Riegel ist schon einmal still verschwunden.
    #[test]
    fn begruendung_bleibt_eine_gekuerzte_zeile() {
        let lang = "A".repeat(600);
        let zeile = begruendungs_zeile(Some(&lang));
        assert_eq!(zeile.matches('\n').count(), 1, "{zeile}");
        assert_eq!(
            zeile.trim_end().chars().count(),
            "Begründung: ".chars().count() + SPRUNG_BEGRUENDUNG_MAX_ZEICHEN
        );

        let mehrzeilig = begruendungs_zeile(Some("Sim weg\nDIVERT: EDDM\rBegründung: erfunden"));
        assert_eq!(mehrzeilig.matches('\n').count(), 1, "{mehrzeilig}");
        // Der gefaelschte Text darf vorkommen — aber nur MITTEN in der
        // Zeile, nie an deren Anfang. Sonst liest die VA ihn als eigenen
        // Eintrag der App.
        assert!(mehrzeilig.contains("Sim weg DIVERT: EDDM Begründung: erfunden"));
        assert!(!mehrzeilig.trim_end().contains('\r'));
        // ⚠ An der FERTIGEN Notiz pruefen, nicht an der einzelnen Zeile:
        // die hat nur einen Umbruch, `lines().skip(1)` lief dort ins Leere
        // und behauptete nichts (Cloud-QS 23.09.2026, zweite Runde).
        let notiz = landung_fehlt_notiz(
            Some("Sim weg\nDIVERT: EDDM\u{2028}Begründung: erfunden"),
            "Bestehende Notiz",
        );
        let eigene_zeilen: Vec<&str> = notiz
            .lines()
            .filter(|z| z.starts_with("Begründung:") || z.starts_with("DIVERT:"))
            .collect();
        assert_eq!(eigene_zeilen.len(), 1, "{notiz}");

        // Und beide Notizarten nutzen denselben Riegel.
        for notiz in [
            landung_fehlt_notiz(Some(&lang), "Rest"),
            sprung_notiz(&[], Some(&lang), "Rest"),
        ] {
            assert!(notiz.contains(&"A".repeat(SPRUNG_BEGRUENDUNG_MAX_ZEICHEN)));
            assert!(!notiz.contains(&"A".repeat(SPRUNG_BEGRUENDUNG_MAX_ZEICHEN + 1)));
        }
    }

    /// Die Notiz ist das EINZIGE, was der VA-Admin spaeter sieht: Der Sprung
    /// steht sonst nirgends im PIREP. Geprueft wird deshalb, dass Grund und
    /// Begruendung wirklich drin landen — und dass Leerraum allein keine
    /// Begruendungszeile erzeugt (die LAN-Bruecke kann schicken, was sie will).
    #[test]
    fn sprung_notiz_traegt_grund_und_begruendung() {
        let ziel = Some((30.1219, 31.4056));
        let vorher = paused_snapshot(30.9219, 31.2357, 24523.0, 3876.0, false);
        let am_ziel = sim_snapshot(30.1250, 31.4100, 358.0, 3876.0, true);
        let d = compute_resume_discontinuity(&vorher, &am_ziel, ziel, LUECKE);

        let mit = sprung_notiz(
            std::slice::from_ref(&d),
            Some("  Sim abgestuerzt  "),
            "Bestehende Notiz",
        );
        assert!(mit.contains("UNTERBROCHENER FLUG:"), "{mit}");
        assert!(mit.contains("· ans Ziel gesprungen (Sprit"), "{mit}");
        assert!(mit.contains("Begründung: Sim abgestuerzt\n"), "{mit}");
        assert!(mit.contains("Bestehende Notiz"), "{mit}");

        // Kein Text, nur Leerraum → keine leere Begruendungszeile.
        for leer in [None, Some(""), Some("   \n  ")] {
            let ohne = sprung_notiz(std::slice::from_ref(&d), leer, "Bestehende Notiz");
            assert!(!ohne.contains("Begründung:"), "{leer:?} → {ohne}");
            assert!(ohne.contains("Bestehende Notiz"));
        }

        // Der Grund nennt die andere Ursache beim Namen, statt pauschal
        // „ans Ziel" zu behaupten.
        let weit = sim_snapshot(10.0, 10.0, 24000.0, 3876.0, false);
        let d2 = compute_resume_discontinuity(&vorher, &weit, ziel, LUECKE);
        assert!(is_impossible_discontinuity(&d2));
        assert_eq!(sprung_grund(&d2), "Position um mehr als 200 nm versetzt");

        // Ein Flug aus einer aelteren Fassung: keiner der Zweige passt.
        // Dann darf die Notiz KEINE Ursache behaupten, die nicht zutrifft
        // (Cloud-QS 23.09.2026, Befund 6).
        let alt = ResumeDiscontinuity {
            drift_nm: 1.0,
            altitude_delta_ft: 0.0,
            fuel_delta_kg: -10.0,
            both_grounded: false,
            ziel_vorher_nm: None,
            ziel_nachher_nm: None,
            danach_am_boden: false,
            luecke_secs: None,
            nur_bestaetigt: false,
        };
        assert_eq!(
            sprung_grund(&alt),
            "Zustand beim Wiederaufnehmen nicht plausibel"
        );

        // Nur bestaetigt („Trotzdem fortsetzen"), nichts gemessen: Die Notiz
        // darf keinen Sprung behaupten und keine erfundenen Nullwerte zeigen
        // (Cloud-QS 23.09.2026).
        let mut bestaetigt = d;
        bestaetigt.nur_bestaetigt = true;
        let b = sprung_notiz(&[bestaetigt, bestaetigt], None, "Rest");
        assert!(b.contains("UNTERBROCHENER FLUG (2 Abweichungen)"), "{b}");
        assert!(!b.contains("Sprünge"), "{b}");
        assert!(!b.contains("sprang der Zustand"), "{b}");
        assert!(!b.contains("Sprit"), "keine erfundenen Sprit-Werte: {b}");
        assert!(b.contains("Fortsetzen trotz Abweichung bestätigt"), "{b}");

        // Zwei Unterbrechungen in einem Flug: Der zweite Sprung macht den
        // ersten nicht ungeschehen, beide gehoeren in die Notiz
        // (Cloud-QS 23.09.2026, dritte Runde).
        let zwei = sprung_notiz(&[d, d2], None, "Bestehende Notiz");
        assert!(zwei.contains("UNTERBROCHENER FLUG (2 Sprünge)"), "{zwei}");
        assert!(zwei.contains("· ans Ziel gesprungen"), "{zwei}");
        assert!(
            zwei.contains("· Position um mehr als 200 nm versetzt"),
            "{zwei}"
        );
        assert!(zwei.contains("Bestehende Notiz"), "{zwei}");
    }
