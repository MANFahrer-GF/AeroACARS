    //! En-Route-Versöhner (Korpus-Audit 2026-07, 632 Flüge) — siehe Doc an
    //! `ENROUTE_RECONCILE_DWELL_SECS`. Diese Tests seeden
    //! `enroute_divergence_since` direkt (gleiches Muster wie
    //! `arrived_fallback_dwell_tests`: `step_flight` liest `Utc::now()`
    //! ohne injizierbare Uhr — seeden statt schlafen). Die v2-Evidenz wird
    //! ECHT aufgebaut (Samples durch die Schatten-Engine), damit die Tests
    //! nicht an Resolver-Interna vorbei konstruieren.

    use super::*;

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
            aircraft_icao: "BE24".into(),
            aircraft_name: String::new(),
            flight_number: "22".into(),
            dpt_airport: "EDLN".into(),
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

    /// Baut ECHTE v2-Fenster-Evidenz auf: `n` Samples im 3-s-Raster durch
    /// die Schatten-Engine, `old_phase` konstant gehalten (wie eine
    /// festgefahrene v1). Endet zeitlich bei ~`Utc::now()`, damit der
    /// nachfolgende `step_flight`-Tick keinen Segmenter-Time-Skip sieht.
    /// Gibt die zuletzt aufgelöste v2-Phase zurück; der Test stempelt sie
    /// — wie der Streamer — nach `stats.shadow_phase`.
    #[allow(clippy::too_many_arguments)]
    fn feed_v2_evidence(
        stats: &mut FlightStats,
        old_phase: FlightPhase,
        n: usize,
        alt0: f64,
        alt_step: f64,
        agl0: f64,
        agl_step: f64,
        vs: f32,
        on_ground: bool,
    ) -> FlightPhase {
        feed_v2_evidence_ending(
            stats, old_phase, n, alt0, alt_step, agl0, agl_step, vs, on_ground, 0,
        )
    }

    /// Wie [`feed_v2_evidence`], aber das Sample-Fenster endet
    /// `end_secs_ago` Sekunden VOR jetzt — für Tests, die zwei Evidenz-
    /// Regime hintereinander füttern (z. B. Sinkflug, dann Level-Off):
    /// zwei Feeds, die beide bei `now` endeten, überlappten sich zeitlich,
    /// und der Segmenter verwürfe die rückdatierten Samples still (Min-
    /// Abstands-Guard gegen rückwärts laufende Zeit).
    #[allow(clippy::too_many_arguments)]
    fn feed_v2_evidence_ending(
        stats: &mut FlightStats,
        old_phase: FlightPhase,
        n: usize,
        alt0: f64,
        alt_step: f64,
        agl0: f64,
        agl_step: f64,
        vs: f32,
        on_ground: bool,
        end_secs_ago: i64,
    ) -> FlightPhase {
        let start = Utc::now() - chrono::Duration::seconds(3 * n as i64 + end_secs_ago);
        let mut phase = old_phase;
        for i in 0..n {
            let t = start + chrono::Duration::seconds(3 * i as i64);
            let (p, _) = stats.shadow_engine.step(
                t,
                alt0 + alt_step * i as f64,
                (agl0 + agl_step * i as f64).max(0.0),
                vs,
                on_ground,
                old_phase,
                None,
            );
            phase = p;
        }
        stats.shadow_phase = Some(phase);
        phase
    }

    /// GSG22-Form: v1 hängt auf `Climb`, während der Flieger real längst
    /// sinkt. Snapshot so gewählt, dass JEDER reguläre Climb-Arm-Übergang
    /// still bleibt: V/S −300 (über der −500-standard_tod-Schwelle),
    /// `climb_peak_msl` == aktuelle MSL (lost_from_peak = 0 → low_alt/
    /// near_ground unerreichbar — exakt die Niedrig-Peak-Blockade aus dem
    /// Audit), AGL < 5000 (Cruise-Gate zu).
    fn stuck_climb_flight(divergence_seeded_secs_ago: Option<i64>) -> (ActiveFlight, SimSnapshot) {
        let flight = fixture();
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 900.0;
        snap.altitude_agl_ft = 800.0;
        snap.vertical_speed_fpm = -300.0;
        snap.groundspeed_kt = 95.0;
        snap.engines_running = 1;
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Climb;
            stats.climb_peak_msl = Some(snap.altitude_msl_ft as f32);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
            // Sinkende Fenster-Evidenz: −600 fpm über ~2 min → Segment
            // Descending, v2-Phase Descent (von Descent aus gefüttert,
            // Baseline-Sync beim ersten Sample). AGL bleibt über dem
            // 5000-ft-Approach-Gate, damit v2 nicht schon zu Approach
            // weiterbefördert — die Tests wollen das Paar (Climb, Descent).
            // Seit 17.10.2026 übernimmt v1 Reiseband-Phasen von v2
            // direkt; der Versöhner zieht nur noch aus dem Reiseband in
            // den Anflug. Die Evidenz liegt deshalb unter dem 5000-ft-
            // Approach-Gate: v2 steht schon im Anflug, v1 hängt im Climb.
            let v2 = feed_v2_evidence(
                &mut stats,
                FlightPhase::Descent,
                40,
                4500.0,
                -30.0,
                4000.0,
                -30.0,
                -600.0,
                false,
            );
            assert_eq!(
                v2,
                FlightPhase::Approach,
                "test setup: v2 must resolve the sustained low descent as Approach"
            );
            if let Some(secs) = divergence_seeded_secs_ago {
                stats.enroute_divergence_since = Some((
                    FlightPhase::Climb,
                    Utc::now() - chrono::Duration::seconds(secs),
                ));
            }
        }
        (flight, snap)
    }

    // ---- Eignungs-Prüfung (rein) ----

    #[test]
    fn eligible_pairs_within_the_air_band() {
        for (v1, v2) in [
            (FlightPhase::Climb, FlightPhase::Approach),
            (FlightPhase::Climb, FlightPhase::Landing),
            (FlightPhase::Cruise, FlightPhase::Final),
            (FlightPhase::Descent, FlightPhase::Approach),
            (FlightPhase::Approach, FlightPhase::Climb),
            (FlightPhase::Approach, FlightPhase::Final),
            (FlightPhase::Final, FlightPhase::Landing),
        ] {
            assert!(enroute_reconcile_eligible(v1, v2), "{v1:?} → {v2:?}");
        }
        // Innerhalb des Reisebands übernimmt v1 v2s Phase direkt
        // (`reiseband_phase`) — dort gibt es nichts zu versöhnen.
        for (v1, v2) in [
            (FlightPhase::Climb, FlightPhase::Descent),
            (FlightPhase::Cruise, FlightPhase::Descent),
            (FlightPhase::Descent, FlightPhase::Cruise),
            (FlightPhase::Cruise, FlightPhase::Climb),
        ] {
            assert!(!enroute_reconcile_eligible(v1, v2), "{v1:?} → {v2:?}");
        }
    }

    #[test]
    fn holding_is_never_reconciled() {
        // v2 modelliert Holding bewusst nicht — dort DARF v1 abweichen.
        for v2 in [
            FlightPhase::Climb,
            FlightPhase::Cruise,
            FlightPhase::Descent,
            FlightPhase::Landing,
        ] {
            assert!(!enroute_reconcile_eligible(FlightPhase::Holding, v2));
        }
        // ... und Holding ist auch als ZIEL tabu (v2 erzeugt es nie —
        // defensive Absicherung, falls sich das je ändert).
        assert!(!enroute_reconcile_eligible(
            FlightPhase::Cruise,
            FlightPhase::Holding
        ));
    }

    #[test]
    fn ground_and_terminal_v1_phases_are_authoritative() {
        // Außerhalb des Luft-Bands ist v1 die Sync-QUELLE für v2 (1:1-
        // Mirror) — der Versöhner darf dort nie eingreifen. `Landing` als
        // v1 gehört dazu: eine v1, die Landing selbst erreicht hat, ist
        // nicht festgefahren.
        for v1 in [
            FlightPhase::Preflight,
            FlightPhase::Boarding,
            FlightPhase::Pushback,
            FlightPhase::TaxiOut,
            FlightPhase::TakeoffRoll,
            FlightPhase::Takeoff,
            FlightPhase::Landing,
            FlightPhase::TaxiIn,
            FlightPhase::BlocksOn,
            FlightPhase::Arrived,
        ] {
            assert!(
                !enroute_reconcile_eligible(v1, FlightPhase::Descent),
                "{v1:?}"
            );
        }
    }

    #[test]
    fn v2_targets_outside_the_air_band_are_taboo() {
        for v2 in [
            FlightPhase::Preflight,
            FlightPhase::Boarding,
            FlightPhase::TaxiIn,
            FlightPhase::BlocksOn,
            FlightPhase::Arrived,
        ] {
            assert!(
                !enroute_reconcile_eligible(FlightPhase::Climb, v2),
                "{v2:?}"
            );
        }
    }

    #[test]
    fn agreement_is_not_divergence() {
        for p in [
            FlightPhase::Climb,
            FlightPhase::Cruise,
            FlightPhase::Descent,
            FlightPhase::Approach,
            FlightPhase::Final,
        ] {
            assert!(!enroute_reconcile_eligible(p, p));
        }
    }

    #[test]
    fn contradiction_gate_matrix() {
        use phase_v2::Segment;
        // Climb: nur ein sinkendes oder bodenständiges Fenster widerlegt
        // einen Steigflug — nie Level (TOC-Wartezeit, Restrictions) oder
        // Climbing (er steigt ja gerade, per Definition kein Widerspruch).
        assert!(segment_contradicts_v1(
            FlightPhase::Climb,
            Segment::Descending
        ));
        assert!(segment_contradicts_v1(FlightPhase::Climb, Segment::Ground));
        assert!(!segment_contradicts_v1(
            FlightPhase::Climb,
            Segment::Climbing
        ));
        assert!(!segment_contradicts_v1(FlightPhase::Climb, Segment::Level));
        assert!(!segment_contradicts_v1(
            FlightPhase::Climb,
            Segment::Insufficient
        ));

        // Cruise: BEWUSST nur Ground (Absurditäts-Guard). NICHT Descending
        // — v1s eigener single-tick −500-fpm-Trigger deckt jeden echten
        // TOD ab (0/632 Flüge im Korpus je in Cruise festgefahren); ein
        // Zug bei Descending erzeugte in QS-Runde 1 den bewiesenen Pull/
        // Recovery-Zyklus bei ATC-Step-Downs. Diese Zeile hält die
        // Absicht fest — NICHT versehentlich auf Descending erweitern.
        assert!(segment_contradicts_v1(FlightPhase::Cruise, Segment::Ground));
        assert!(!segment_contradicts_v1(
            FlightPhase::Cruise,
            Segment::Descending
        ));
        assert!(!segment_contradicts_v1(
            FlightPhase::Cruise,
            Segment::Climbing
        ));
        assert!(!segment_contradicts_v1(FlightPhase::Cruise, Segment::Level));

        // Descent/Approach/Final: nur ein steigendes oder bodenständiges
        // Fenster widerlegt sie (GSG-105-Klasse + verpasster Go-Around /
        // verpasste Touchdown-Kante) — nie Level (Zwischen-Level-Off,
        // v1s B-003-Recovery übernimmt eigenständig bei |V/S|<200) oder
        // Descending (konsistent mit der eigenen Phase).
        for p in [
            FlightPhase::Descent,
            FlightPhase::Approach,
            FlightPhase::Final,
        ] {
            assert!(segment_contradicts_v1(p, Segment::Climbing), "{p:?}");
            assert!(segment_contradicts_v1(p, Segment::Ground), "{p:?}");
            assert!(!segment_contradicts_v1(p, Segment::Descending), "{p:?}");
            assert!(!segment_contradicts_v1(p, Segment::Level), "{p:?}");
        }

        // Außerhalb des Luft-Bands (z. B. Holding) widerspricht nichts —
        // der Versöhner ist dort ohnehin nicht eligible.
        assert!(!segment_contradicts_v1(
            FlightPhase::Holding,
            Segment::Descending
        ));
    }

    #[test]
    fn cruise_follows_v2_into_the_descent() {
        // Absurditäts-Guard, integrationsscharf: selbst eine lange (weit
        // über jedem Dwell liegende), durchgehend bestätigte Descending-
        // Evidenz darf ein Cruise NIE über den Versöhner nach Descent
        // ziehen — das ist v1s eigenem single-tick-Trigger vorbehalten
        // (Absicht siehe contradiction_gate_matrix). Snapshot selbst hält
        // v1s Trigger bewusst still (vs>−500, AGL fern von Boden, kein
        // 5000-ft-Verlust seit Peak).
        let flight = fixture();
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 33000.0;
        snap.altitude_agl_ft = 32000.0;
        snap.vertical_speed_fpm = -480.0; // knapp unter v1s -500-Schwelle
        snap.groundspeed_kt = 400.0;
        snap.engines_running = 2;
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Cruise;
            stats.cruise_peak_msl = Some(35000.0); // lost_alt = 2000 < 5000
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
            let v2 = feed_v2_evidence(
                &mut stats,
                FlightPhase::Descent,
                40,
                35000.0,
                -20.0,
                34000.0,
                -20.0,
                -480.0,
                false,
            );
            assert_eq!(
                v2,
                FlightPhase::Descent,
                "test setup: v2 resolves sustained descent"
            );
            assert_eq!(
                stats.shadow_engine.current_segment(),
                phase_v2::Segment::Descending
            );
            stats.enroute_divergence_since = Some((
                FlightPhase::Cruise,
                Utc::now() - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 500),
            ));
        }
        // Seit 17.10.2026: kein Zug durch den Versöhner, sondern die
        // direkte Übernahme — v2s Fenster beweist den Sinkflug, v1 folgt
        // im nächsten Takt (vorher wartete v1 auf −500 fpm und 5000 ft
        // Verlust und begann den Sinkflug im Median 2 min zu spät).
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Descent));
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Descent);
    }

    #[test]
    fn pull_direction_follows_the_flight_arc() {
        // Vorwärts (kurzer Dwell): v2 ist im Bogen weiter als v1.
        assert!(enroute_reconcile_is_forward(
            FlightPhase::Climb,
            FlightPhase::Descent
        ));
        assert!(enroute_reconcile_is_forward(
            FlightPhase::Climb,
            FlightPhase::Landing
        ));
        assert!(enroute_reconcile_is_forward(
            FlightPhase::Final,
            FlightPhase::Landing
        ));
        assert!(enroute_reconcile_is_forward(
            FlightPhase::Cruise,
            FlightPhase::Approach
        ));
        // Rückwärts (langer Dwell): v2 hängt hinter v1.
        assert!(!enroute_reconcile_is_forward(
            FlightPhase::Descent,
            FlightPhase::Cruise
        ));
        assert!(!enroute_reconcile_is_forward(
            FlightPhase::Final,
            FlightPhase::Approach
        ));
        assert!(!enroute_reconcile_is_forward(
            FlightPhase::Cruise,
            FlightPhase::Climb
        ));
        // Gleichstand ist keine Richtung.
        assert!(!enroute_reconcile_is_forward(
            FlightPhase::Cruise,
            FlightPhase::Cruise
        ));
    }

    // ---- Integration über step_flight ----

    #[test]
    fn stuck_climb_with_v2_in_the_approach_reconciles_after_dwell() {
        // Climb→Descent ist VORWÄRTS im Bogen → der kurze Dwell gilt.
        let (flight, snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_forward_dwell_secs.wert + 1));
        {
            // QS-Runde 1: Timer-Hygiene-Assertions nicht-vakuös machen —
            // die Timer ECHT armieren, damit ihr Clear beweisbar am
            // Feuer-Pfad hängt (check_descent_transition nullt
            // descent_pending bei diesem Snapshot ohnehin jeden Tick,
            // die anderen beiden räumt NUR der Versöhner).
            let mut stats = flight.stats.lock().unwrap();
            stats.go_around_climb_pending_since = Some(Utc::now() - chrono::Duration::seconds(3));
            stats.touch_and_go_pending_since = Some(Utc::now() - chrono::Duration::seconds(3));
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Approach));
        let stats = flight.stats.lock().unwrap();
        assert_eq!(stats.phase, FlightPhase::Approach);
        assert!(
            stats.enroute_divergence_since.is_none(),
            "fired — clock cleared"
        );
        assert!(
            stats.go_around_climb_pending_since.is_none(),
            "a stale armed GA dwell must not survive the pull (false GA on re-entry)"
        );
        assert!(
            stats.touch_and_go_pending_since.is_none(),
            "a stale armed T&G dwell must not survive the pull"
        );
    }

    #[test]
    fn does_not_fire_before_dwell_elapsed() {
        let (flight, snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_forward_dwell_secs.wert - 10));
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None);
        let stats = flight.stats.lock().unwrap();
        assert_eq!(stats.phase, FlightPhase::Climb);
        assert!(
            stats.enroute_divergence_since.is_some(),
            "clock keeps running, just hasn't reached the dwell yet"
        );
    }

    #[test]
    fn backward_pull_needs_the_long_dwell() {
        // GSG-105-Klasse: v1 hängt in Descent (es gibt keinen Descent→
        // Climb-Weg zurück), der Flieger steigt aber real und nachhaltig —
        // das Kinematik-Gate ist erfüllt (Climbing ⊥ Descent). v2 sagt
        // Cruise — RÜCKWÄRTS im Bogen. Der kurze Vorwärts-Dwell darf hier
        // NICHT reichen; erst der lange Dwell feuert.
        fn descent_stuck_flight(seed_secs: i64) -> (ActiveFlight, SimSnapshot) {
            let flight = fixture();
            let mut snap = SimSnapshot::default();
            snap.on_ground = false;
            snap.altitude_msl_ft = 9000.0;
            snap.altitude_agl_ft = 8000.0;
            // Seit 17.10.2026 übernimmt v1 im Reiseband v2 direkt; das
            // Rückwärts-Paar lebt deshalb nur noch im Anflug: v1 hängt im
            // Approach (Steigen aus dem Anflug OHNE Durchstarter — der
            // Detektor greift oberhalb des Stabilitäts-Gates nicht), der
            // Flieger steigt real (+800 fpm).
            snap.vertical_speed_fpm = 800.0;
            snap.groundspeed_kt = 250.0;
            snap.engines_running = 2;
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.phase = FlightPhase::Approach;
                stats.was_airborne = true;
                stats.was_on_ground = Some(false);
                // Steigende Fenster-Evidenz (+800 fpm) → Segment Climbing;
                // v2 promotet aus Cruise heraus auf Climb → Ziel = Climb,
                // Rang 0 < 2 = rückwärts.
                // 3200 ft Gewinn: mehr als `REISEFLUG_STUFE_FT` über der
                // Reiseflughöhe, sonst bliebe es eine Stufe im Cruise.
                let v2 = feed_v2_evidence(
                    &mut stats,
                    FlightPhase::Cruise,
                    40,
                    7000.0,
                    80.0,
                    6000.0,
                    80.0,
                    1600.0,
                    false,
                );
                assert_eq!(
                    v2,
                    FlightPhase::Climb,
                    "test setup: v2 resolves the sustained climb"
                );
                assert_eq!(
                    stats.shadow_engine.current_segment(),
                    phase_v2::Segment::Climbing,
                    "test setup: sustained climb evidence"
                );
                stats.enroute_divergence_since = Some((
                    FlightPhase::Approach,
                    Utc::now() - chrono::Duration::seconds(seed_secs),
                ));
            }
            (flight, snap)
        }

        // Vorwärts-Dwell überschritten, Rückwärts-Dwell nicht → kein Zug.
        let (flight, snap) = descent_stuck_flight(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_forward_dwell_secs.wert + 1);
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result, None,
            "backward pull must not borrow the short forward dwell"
        );
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Approach);

        // Rückwärts-Dwell voll → jetzt zieht v2 die festgefahrene v1.
        let (flight, snap) = descent_stuck_flight(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 1);
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Climb));
    }

    // ---- QS-Runde 1: Anti-Oszillations-Gate (segment_contradicts_v1) ----

    /// 17.10.2026 („Stück 2"): Bestätigt v2 nur den Reiseflug, muss die
    /// Warteschleifen-Erkennung des Cruise-Arms weiterlaufen. Im ersten
    /// Korpus-Lauf fiel sie still weg (9 → 1 Holding-Eintritte), weil die
    /// Übernahme auch die eigene Phase „übernahm".
    /// 24.10.2026, AUA 561 EDDN–LOWI (Pilot 15): 6.773 ft MSL über den
    /// Lechtaler Alpen, 45 km vor Innsbruck, Gelände knapp darunter. Vorher
    /// wurde daraus „Final“ (nur AGL < 700) und beim Wiedersteigen sagte die
    /// Kabine ein Durchstarten an. Jetzt bleibt es Approach; an der Bahn und
    /// ohne bekannte Platzhöhe ist es weiter Final.
    #[test]
    fn final_nur_in_platznaehe_nicht_ueber_den_bergen() {
        let lauf = |lat: f64, lon: f64, msl: f64, elev: Option<f32>| {
            let flight = fixture();
            let t0 = Utc::now() - chrono::Duration::seconds(100);
            {
                let mut stats = flight.stats.lock().unwrap();
                stats.phase = FlightPhase::Approach;
                stats.shadow_phase = Some(FlightPhase::Approach);
                stats.was_airborne = true;
                stats.was_on_ground = Some(false);
                stats.arr_airport_elevation_ft = elev;
            }
            let snap = SimSnapshot {
                lat,
                lon,
                altitude_msl_ft: msl,
                altitude_agl_ft: 600.0,
                vertical_speed_fpm: -500.0,
                groundspeed_kt: 250.0,
                indicated_airspeed_kt: 220.0,
                heading_deg_true: 254.0,
                engines_running: 2,
                ..SimSnapshot::default()
            };
            let mut phase = FlightPhase::Approach;
            for i in 0..4 {
                if let Some(p) = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i)) {
                    phase = p;
                }
            }
            phase
        };
        assert_eq!(lauf(47.3119, 10.7761, 6_773.0, Some(1_907.0)), FlightPhase::Approach, "über den Bergen kein Final");
        assert_eq!(lauf(47.2602, 11.3800, 2_507.0, Some(1_907.0)), FlightPhase::Final, "vor der Bahn in Innsbruck Final");
        assert_eq!(lauf(47.3119, 10.7761, 6_773.0, None), FlightPhase::Final, "ohne Platzhöhe wie bisher");
    }

    #[test]
    fn holding_entry_from_cruise_still_works() {
        let flight = fixture();
        let t0 = Utc::now() - chrono::Duration::seconds(200);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Cruise;
            stats.shadow_phase = Some(FlightPhase::Cruise);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
        }
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 12000.0;
        snap.altitude_agl_ft = 11000.0;
        snap.vertical_speed_fpm = 0.0;
        snap.groundspeed_kt = 220.0;
        snap.engines_running = 2;
        snap.bank_deg = 25.0;
        let mut phase = FlightPhase::Cruise;
        for i in 0..40 {
            // Standardkurve 3°/s → 15° je 5-s-Takt.
            snap.heading_deg_true = (15.0 * i as f32) % 360.0;
            if let Some(p) = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i)) {
                phase = p;
            }
            flight.stats.lock().unwrap().shadow_phase = Some(FlightPhase::Cruise);
        }
        assert_eq!(phase, FlightPhase::Holding, "circling in cruise must enter Holding");
    }

    /// Warteschleife während des Sinkflugs (Level-off, anhaltende
    /// Querneigung) — seit 17.10.2026 im Descent-Arm erkannt.
    #[test]
    fn holding_entry_from_descent_level_off() {
        let flight = fixture();
        let t0 = Utc::now() - chrono::Duration::seconds(200);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Descent;
            stats.shadow_phase = Some(FlightPhase::Descent);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
        }
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 15000.0;
        snap.altitude_agl_ft = 14000.0;
        snap.vertical_speed_fpm = 0.0;
        snap.groundspeed_kt = 230.0;
        snap.engines_running = 2;
        snap.bank_deg = 25.0;
        let mut phase = FlightPhase::Descent;
        for i in 0..40 {
            snap.heading_deg_true = (360.0 - 15.0 * i as f32).rem_euclid(360.0);
            if let Some(p) = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i)) {
                phase = p;
            }
            flight.stats.lock().unwrap().shadow_phase = Some(FlightPhase::Descent);
        }
        assert_eq!(phase, FlightPhase::Holding, "circling on a descent level-off must enter Holding");
        assert_eq!(
            flight.stats.lock().unwrap().previous_phase_before_holding,
            Some(FlightPhase::Descent)
        );
    }

    /// Codex-QS 17.10.2026: v2 springt bei anhaltendem tiefem Sinkflug
    /// direkt vom Reiseflug in den Anflug. v1 im Cruise muss folgen (über
    /// Descent und seine eigene Anflug-Kante), auch bei einem flachen
    /// Anflug, bei dem der Cruise-Schnellweg (< −500 fpm) nie greift.
    #[test]
    fn shallow_approach_from_cruise_follows_v2_into_the_approach() {
        let flight = fixture();
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Cruise;
            stats.shadow_phase = Some(FlightPhase::Approach);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
        }
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 4800.0;
        snap.altitude_agl_ft = 4500.0;
        snap.vertical_speed_fpm = -400.0;
        snap.groundspeed_kt = 160.0;
        snap.engines_running = 2;
        assert_eq!(step_flight(&flight, &snap), Some(FlightPhase::Descent));
        assert_eq!(step_flight(&flight, &snap), Some(FlightPhase::Approach));
    }

    /// Codex-QS 17.10.2026: Ende einer Warteschleife aus dem Sinkflug,
    /// während v2 inzwischen Cruise sagt → direkt Cruise, kein Takt Descent.
    #[test]
    fn holding_exit_goes_straight_to_the_current_v2_phase() {
        let flight = fixture();
        let t0 = Utc::now() - chrono::Duration::seconds(100);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Holding;
            stats.previous_phase_before_holding = Some(FlightPhase::Descent);
            stats.shadow_phase = Some(FlightPhase::Cruise);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
        }
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 15000.0;
        snap.altitude_agl_ft = 14000.0;
        snap.vertical_speed_fpm = 0.0;
        snap.groundspeed_kt = 230.0;
        snap.engines_running = 2;
        snap.bank_deg = 0.0;
        snap.heading_deg_true = 90.0;
        let mut wechsel = Vec::new();
        for i in 0..45 {
            if let Some(p) = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i)) {
                wechsel.push(p);
            }
        }
        assert_eq!(wechsel, vec![FlightPhase::Cruise], "one clean Holding→Cruise, no Descent tick");
    }

    /// Fliegt eine Rennbahn-Warteschleife (Schenkel 60 s, Kurven 180° in
    /// 60 s) über 12 min ab und beginnt in der ersten Kurve, ergibt sich EINE
    /// Warteschleife bis zum Geradeausflug danach — nicht ein Bruchstück je
    /// Schenkel (P16 FL205 vor dem 17.10.2026: drei 45-s-Stücke).
    fn racetrack_flight() -> (ActiveFlight, SimSnapshot, DateTime<Utc>) {
        let flight = fixture();
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Cruise;
            stats.shadow_phase = Some(FlightPhase::Cruise);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
        }
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 20500.0;
        snap.altitude_agl_ft = 19500.0;
        snap.groundspeed_kt = 240.0;
        snap.engines_running = 2;
        (flight, snap, Utc::now() - chrono::Duration::seconds(1500))
    }

    #[test]
    fn racetrack_hold_is_one_holding_not_fragments() {
        let (flight, mut snap, t0) = racetrack_flight();
        let mut wechsel = Vec::new();
        let mut kurs = 90.0_f32;
        for i in 0..(12 * 60 / 5) {
            // Zyklus 4 min: 60 s Kurve rechts, 60 s gerade, 60 s Kurve, 60 s gerade.
            let im_zyklus = (i * 5) % 240;
            let kurve = im_zyklus < 60 || (120..180).contains(&im_zyklus);
            if kurve {
                kurs = (kurs + 15.0) % 360.0;
            }
            snap.bank_deg = if kurve { 25.0 } else { 0.0 };
            snap.heading_deg_true = kurs;
            if let Some(p) = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i)) {
                wechsel.push(p);
            }
            flight.stats.lock().unwrap().shadow_phase = Some(FlightPhase::Cruise);
        }
        assert_eq!(wechsel, vec![FlightPhase::Holding], "one hold for the whole racetrack");
        // Danach 4 min geradeaus → Ende der Warteschleife, genau einmal.
        for i in (12 * 60 / 5)..(16 * 60 / 5) {
            snap.bank_deg = 0.0;
            if let Some(p) = step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i)) {
                wechsel.push(p);
            }
            flight.stats.lock().unwrap().shadow_phase = Some(FlightPhase::Cruise);
        }
        assert_eq!(wechsel, vec![FlightPhase::Holding, FlightPhase::Cruise]);
    }

    /// Eine lange Kurve (200° in 3 min, 25° Querneigung) ist keine
    /// Warteschleife (FL388 vor dem 17.10.2026: zwei Schein-Warteschleifen).
    #[test]
    fn long_course_change_is_no_holding() {
        let (flight, mut snap, t0) = racetrack_flight();
        snap.bank_deg = 25.0;
        let mut kurs = 0.0_f32;
        for i in 0..72 {
            if i < 36 {
                kurs = (kurs + 200.0 / 36.0) % 360.0;
            } else {
                snap.bank_deg = 0.0;
            }
            snap.heading_deg_true = kurs;
            assert_eq!(step_flight_at(&flight, &snap, t0 + chrono::Duration::seconds(5 * i)), None);
            flight.stats.lock().unwrap().shadow_phase = Some(FlightPhase::Cruise);
        }
    }

    #[test]
    fn atc_step_down_level_off_stays_descent() {
        // Kern von „Stück 2" (17.10.2026): Bis hier sprang v1 bei einem
        // Level-off im Sinkflug (|V/S| < 200, AGL > 5000) sofort auf Cruise
        // und beim nächsten Sinken zurück — in 40 % der Flüge. Jetzt
        // bleibt es, wie v2 (Descent + Segment Level), beim Sinkflug.
        let flight = fixture();
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 7500.0;
        snap.altitude_agl_ft = 6500.0;
        snap.vertical_speed_fpm = -50.0;
        snap.groundspeed_kt = 280.0;
        snap.engines_running = 2;
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Descent;
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
            // Erst Sinkflug (FL100 → 7500 ft), dann Level-off auf 7500 ft.
            feed_v2_evidence_ending(
                &mut stats,
                FlightPhase::Descent,
                30,
                10000.0,
                -85.0,
                9000.0,
                -85.0,
                -1500.0,
                false,
                90,
            );
            let v2 = feed_v2_evidence(
                &mut stats,
                FlightPhase::Descent,
                30,
                7500.0,
                0.0,
                6500.0,
                0.0,
                0.0,
                false,
            );
            assert_eq!(v2, FlightPhase::Descent, "test setup: v2 holds Descent on level-off");
            assert_eq!(
                stats.shadow_engine.current_segment(),
                phase_v2::Segment::Level,
                "test setup: level evidence"
            );
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None, "a level-off in the descent must stay Descent");
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Descent);
    }

    #[test]
    fn toc_level_off_follows_v2_until_it_confirms_cruise() {
        // Seit 17.10.2026 gibt es im Reiseband nur noch eine Phase: v1
        // folgt v2, auch zurück zum Climb, bis v2 den Reiseflug bestätigt
        // (ohne Referenz nach 240 s Level). Im Betrieb entsteht das Paar
        // (Cruise, Climb) nur nach einem Neustart oder einer Warteschleife.
        let flight = fixture();
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 36000.0;
        snap.altitude_agl_ft = 35000.0;
        snap.vertical_speed_fpm = -20.0;
        snap.groundspeed_kt = 480.0;
        snap.engines_running = 2;
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Cruise;
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
            let v2 = feed_v2_evidence(
                &mut stats,
                FlightPhase::Climb,
                40,
                36000.0,
                0.0,
                35000.0,
                0.0,
                0.0,
                false,
            );
            assert_eq!(v2, FlightPhase::Climb, "test setup: v2 still on Climb before 240s");
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Climb));
    }

    #[test]
    fn ga_climbout_is_never_pulled_back_into_the_approach() {
        // QS-Befund F3: v1 klassifiziert einen Go-Around korrekt (→Climb),
        // v2 hält Final bis +700 ft Bestätigung — bei langsamen Steigern
        // u. U. minutenlang. (Climb, Final) wäre „vorwärts" (45 s) — aber
        // Climbing widerspricht Climb NICHT → kein Rückzug in den Anflug,
        // keine GA-Doppelzählung.
        let flight = fixture();
        let mut snap = SimSnapshot::default();
        snap.on_ground = false;
        snap.altitude_msl_ft = 1400.0;
        snap.altitude_agl_ft = 900.0;
        snap.vertical_speed_fpm = 450.0;
        snap.groundspeed_kt = 120.0;
        snap.engines_running = 1;
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Climb;
            stats.climb_peak_msl = Some(snap.altitude_msl_ft as f32);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
            // Steigende Evidenz (+400 fpm, +500 ft gesamt — unter der
            // 700-ft-GA-Bestätigung) mit old=Final → v2 hält Final.
            let v2 = feed_v2_evidence(
                &mut stats,
                FlightPhase::Final,
                25,
                900.0,
                20.0,
                400.0,
                20.0,
                400.0,
                false,
            );
            assert_eq!(
                v2,
                FlightPhase::Final,
                "test setup: v2 still holds Final during climb-out"
            );
            assert_eq!(
                stats.shadow_engine.current_segment(),
                phase_v2::Segment::Climbing,
                "test setup: climbing evidence"
            );
            stats.enroute_divergence_since = Some((
                FlightPhase::Climb,
                Utc::now() - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 100),
            ));
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result, None,
            "a genuine climb-out must never be pulled back to Final"
        );
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Climb);
    }

    #[test]
    fn paused_tick_resets_the_clock() {
        // QS-Befund (Pause-Loch): die Uhr ist Wanduhr; eine Sim-Pause
        // unter der Reload-Gap-Schwelle (Cruise: bis 150 s!) liefe sonst
        // voll in den Dwell. Pause unterbricht die Beobachtung → Reset.
        let (flight, mut snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_forward_dwell_secs.wert + 1));
        snap.paused = true;
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None);
        assert!(
            flight
                .stats
                .lock()
                .unwrap()
                .enroute_divergence_since
                .is_none(),
            "a paused tick must discard the divergence clock"
        );
    }

    #[test]
    fn fires_exactly_at_the_dwell_boundary() {
        // >= — die Grenze selbst muss feuern (QS-Runde 1: Mutation >= → >
        // überlebte die Suite; dieser Test tötet sie).
        let (flight, snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_forward_dwell_secs.wert));
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Approach));
    }

    #[test]
    fn first_divergent_tick_only_arms_the_clock() {
        let (flight, snap) = stuck_climb_flight(None);
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None);
        let stats = flight.stats.lock().unwrap();
        let (p1, since) = stats
            .enroute_divergence_since
            .expect("first divergent tick must arm the clock");
        assert_eq!(p1, FlightPhase::Climb);
        assert!((Utc::now() - since).num_seconds() < 5, "armed at ~now");
    }

    #[test]
    fn insufficient_evidence_resets_the_clock() {
        // Frisch resettete Schatten-Engine (Reload/Time-Skip): v2s Phase
        // steht zwar noch auf Descent, aber ohne aktuelle Fenster-Evidenz
        // (`Insufficient`) darf der Versöhner weder feuern noch die Uhr
        // weiterlaufen lassen.
        let (flight, snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 100));
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.shadow_engine = phase_v2::ShadowPhaseEngine::default();
            stats.shadow_phase = Some(FlightPhase::Approach);
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None);
        let stats = flight.stats.lock().unwrap();
        assert_eq!(stats.phase, FlightPhase::Climb);
        assert!(
            stats.enroute_divergence_since.is_none(),
            "no evidence — clock must reset, not idle along"
        );
    }

    #[test]
    fn v2_progress_during_the_dwell_does_not_restart_the_clock() {
        // Replay-Befund BE24: eine paar-gekoppelte Uhr startete bei jedem
        // legitimen v2-Evidenz-Fortschritt (Descent→Approach→Final auf
        // kurzen, flachen Anflügen) neu und feuerte erst NACH dem
        // Touchdown. Die Uhr hängt deshalb an v1s STILLSTAND: hier wurde
        // sie (irgendwann, mit damals anderem v2-Wert) für das
        // festgefahrene Climb gestartet — v2 zeigt inzwischen Approach.
        // Der abgelaufene Dwell muss auf v2s AKTUELLE Phase ziehen.
        let (flight, snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 1));
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result,
            Some(FlightPhase::Approach),
            "v1's standstill is what counts — v2 wandering must not reset the dwell"
        );
    }

    #[test]
    fn stale_clock_from_another_v1_phase_re_arms() {
        // Die gespeicherte Uhr gehört zu einer ANDEREN v1-Phase (Cruise) —
        // z. B. weil v1 über einen Pfad gewechselt hat, der den normalen
        // Commit umgeht (v0.13.17-Airborne-Rescue early return, Resume-
        // Restore). Sie darf nicht für das jetzige Climb weiterzählen.
        let (flight, snap) = stuck_climb_flight(None);
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.enroute_divergence_since = Some((
                FlightPhase::Cruise,
                Utc::now() - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 500),
            ));
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result, None,
            "a clock armed for another v1 phase must not fire"
        );
        let stats = flight.stats.lock().unwrap();
        let (p1, since) = stats.enroute_divergence_since.expect("re-armed");
        assert_eq!(p1, FlightPhase::Climb);
        assert!((Utc::now() - since).num_seconds() < 5, "fresh clock");
    }

    #[test]
    fn fresh_v1_transition_wins_and_resets_the_clock() {
        // v1 lebt: ihr Climb→Cruise-Übergang (seit 17.10.2026 die Übernahme
        // von v2s Reiseband-Phase) feuert diesen Tick. Der Versöhner —
        // obwohl sein Dwell längst um wäre — darf den Normalpfad weder
        // übersteuern noch seine Uhr behalten.
        let (flight, mut snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 100));
        snap.vertical_speed_fpm = -50.0;
        snap.altitude_agl_ft = 6000.0;
        snap.altitude_msl_ft = 7000.0;
        flight.stats.lock().unwrap().shadow_phase = Some(FlightPhase::Cruise);
        let result = step_flight(&flight, &snap);
        assert_eq!(
            result,
            Some(FlightPhase::Cruise),
            "the normal path always wins"
        );
        let stats = flight.stats.lock().unwrap();
        assert!(
            stats.enroute_divergence_since.is_none(),
            "a live v1 transition restarts the divergence clock"
        );
    }

    #[test]
    fn reload_glitch_tick_neither_fires_nor_keeps_the_clock() {
        let (flight, snap) = stuck_climb_flight(Some(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 100));
        {
            // 60 s Lücke seit dem letzten Tick — weit über dem Reload-
            // Verdacht (2.5 × 10 s Climb-Intervall).
            let mut stats = flight.stats.lock().unwrap();
            stats.last_step_at = Some(Utc::now() - chrono::Duration::seconds(60));
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None);
        let stats = flight.stats.lock().unwrap();
        assert_eq!(stats.phase, FlightPhase::Climb);
        assert!(
            stats.enroute_divergence_since.is_none(),
            "glitch tick restarts the clock"
        );
    }

    #[test]
    fn stuck_climb_on_the_ground_is_pulled_to_v2_landing() {
        // GSG22-Endzustand: v1 hing noch auf Climb, als der Flieger längst
        // ausrollte; v2 hatte sich per Ground-Evidenz auf Landing
        // selbstbefördert (v0.19.1). Der Versöhner zieht v1 nach Landing —
        // ab dort läuft v1s EIGENE Landing→TaxiIn→BlocksOn→Arrived-Kette
        // normal weiter, statt dass erst der Not-Arrived-Fallback greift.
        let flight = fixture();
        let mut snap = SimSnapshot::default();
        snap.on_ground = true;
        snap.altitude_msl_ft = 120.0;
        snap.altitude_agl_ft = 0.0;
        snap.vertical_speed_fpm = 0.0;
        snap.groundspeed_kt = 25.0; // Rollout — Arrived-Fallback bleibt still
        snap.engines_running = 1;
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Climb;
            stats.climb_peak_msl = Some(snap.altitude_msl_ft as f32);
            stats.was_airborne = true;
            stats.was_on_ground = Some(true);
            // Für den landing_at-Rescue (QS-Runde 1): der Flug ist real
            // gestartet, der 50-Hz-Sampler hat den Touchdown gelatcht.
            stats.takeoff_at = Some(Utc::now() - chrono::Duration::minutes(10));
            stats.sampler_touchdown_at = Some(Utc::now() - chrono::Duration::seconds(30));
            stats.sampler_touchdown_vs_fpm = Some(-320.0);
            let v2 = feed_v2_evidence(
                &mut stats,
                FlightPhase::Landing,
                40,
                120.0,
                0.0,
                0.0,
                0.0,
                0.0,
                true,
            );
            assert_eq!(
                v2,
                FlightPhase::Landing,
                "test setup: old=Landing is mirrored 1:1 while ground evidence accumulates"
            );
            stats.enroute_divergence_since = Some((
                FlightPhase::Climb,
                Utc::now() - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 1),
            ));
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, Some(FlightPhase::Landing));
        let stats = flight.stats.lock().unwrap();
        assert_eq!(stats.phase, FlightPhase::Landing);
        // QS-Runde 1 (HIGH): der Zug auf Landing überspringt die Final→
        // Landing-Kante — landing_at MUSS aus dem Sampler gerettet werden,
        // sonst: aufgeblähte flight_time, kein LandingRecord, toter
        // Landing-Arm-Analyzer, is_finalizable=false.
        assert_eq!(stats.landing_at, stats.sampler_touchdown_at);
        assert_eq!(stats.landing_rate_fpm, Some(-320.0));
    }

    #[test]
    fn landing_pull_requires_raw_on_ground_this_tick() {
        // QS-Runde 1 (HIGH): v2=Landing stammt aus fensterträger Ground-
        // Mehrheit — hebt der Flieger im Versatz-Tick wieder ab (Balked
        // Landing), würde ein Luft-Zug auf Landing beide Engines unlösbar
        // festnageln. Rohes on_ground=false ⇒ kein Landing-Zug.
        let flight = fixture();
        let mut snap = SimSnapshot::default();
        snap.on_ground = false; // wieder in der Luft
        snap.altitude_msl_ft = 200.0;
        snap.altitude_agl_ft = 80.0;
        snap.vertical_speed_fpm = 900.0;
        snap.groundspeed_kt = 130.0;
        snap.engines_running = 1;
        {
            let mut stats = flight.stats.lock().unwrap();
            stats.phase = FlightPhase::Climb;
            stats.climb_peak_msl = Some(snap.altitude_msl_ft as f32);
            stats.was_airborne = true;
            stats.was_on_ground = Some(false);
            let v2 = feed_v2_evidence(
                &mut stats,
                FlightPhase::Landing,
                40,
                120.0,
                0.0,
                0.0,
                0.0,
                0.0,
                true,
            );
            assert_eq!(
                v2,
                FlightPhase::Landing,
                "test setup: v2 still says Landing"
            );
            stats.enroute_divergence_since = Some((
                FlightPhase::Climb,
                Utc::now() - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().phasen_kern.enroute_reconcile_dwell_secs.wert + 100),
            ));
        }
        let result = step_flight(&flight, &snap);
        assert_eq!(result, None, "no Landing pull while airborne");
        assert_eq!(flight.stats.lock().unwrap().phase, FlightPhase::Climb);
    }
