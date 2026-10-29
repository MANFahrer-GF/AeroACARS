    use super::*;

    // ─── flight_time_with_pause_secs: F3 Saturating-Arithmetik ───

    #[test]
    fn flight_time_no_pause_returns_raw() {
        // 60 min Roh-Flugzeit, 0 Pause → 60 min unveraendert.
        assert_eq!(flight_time_with_pause_secs(3600, 0), 3600);
    }

    #[test]
    fn flight_time_pause_subtracted() {
        // 60 min Roh, 10 min Pause → 50 min.
        assert_eq!(flight_time_with_pause_secs(3600, 600), 3000);
    }

    #[test]
    fn flight_time_pause_equals_raw_returns_zero() {
        // Pause genauso lang wie Roh → 0, kein Unterlauf.
        assert_eq!(flight_time_with_pause_secs(600, 600), 0);
    }

    #[test]
    fn flight_time_pause_larger_than_raw_returns_zero() {
        // Pause-Akkumulator groesser als Roh-Wert (defensiv gegen
        // Clock-Skew nach App-Restart): saturating auf 0.
        assert_eq!(flight_time_with_pause_secs(600, 99999), 0);
    }

    #[test]
    fn flight_time_negative_pause_is_clamped_to_zero() {
        // Negative Pause (sollte nie vorkommen, defensive
        // Programmierung): clamp auf 0 → raw bleibt.
        assert_eq!(flight_time_with_pause_secs(3600, -100), 3600);
    }

    #[test]
    fn flight_time_pause_exceeding_i32_max_is_clamped() {
        // i64-Pause > i32::MAX wird auf i32::MAX gecapped, dann
        // saturating_sub → 0 (defensive).
        let huge_pause: i64 = i32::MAX as i64 + 1000;
        assert_eq!(flight_time_with_pause_secs(3600, huge_pause), 0);
    }

    #[test]
    fn flight_time_zero_raw_returns_zero() {
        // Pre-Takeoff (raw=0) ohne Pause → 0; mit Pause → 0.
        assert_eq!(flight_time_with_pause_secs(0, 0), 0);
        assert_eq!(flight_time_with_pause_secs(0, 600), 0);
    }

    // ─── pause_secs_within: nur Pausen innerhalb des Fensters zaehlen ───
    // (ITY 1358, 07.09.2026 — siehe Doc-Kommentar an der Funktion)

    fn segment(started_at: DateTime<Utc>, ended_at: DateTime<Utc>) -> PauseSegment {
        PauseSegment {
            started_at,
            ended_at,
            duration_secs: (ended_at - started_at).num_seconds(),
            reason: PauseReason::SimPause,
            drift_nm: None,
            altitude_delta_ft: None,
            fuel_delta_kg: None,
        }
    }

    #[test]
    fn ity1358_boarding_pause_entirely_before_takeoff_does_not_count() {
        // Der reale Vorfall: ein 36-Minuten-Verbindungsabbruch waehrend
        // des Boardings, komplett vor dem Start. Vorher wurde die
        // gesamte Session-Pausensumme abgezogen (44 min echte Luftzeit
        // minus 36 min Boarding-Pause ergab faelschlich ~8 min) — jetzt
        // zaehlt nur, was tatsaechlich im Start→Landung-Fenster lag: 0.
        let takeoff = Utc::now();
        let landing = takeoff + chrono::Duration::minutes(44);
        let boarding_pause = segment(
            takeoff - chrono::Duration::minutes(50),
            takeoff - chrono::Duration::minutes(14),
        );
        let segments = vec![boarding_pause];
        assert_eq!(
            pause_secs_within(&segments, 36 * 60, takeoff, landing),
            0,
            "eine Pause komplett vor dem Start darf die Flugzeit nicht kuerzen"
        );
    }

    #[test]
    fn a_pause_fully_inside_the_window_counts_in_full() {
        // Ein echter Sim-Crash waehrend des Reisefluges — unveraendertes
        // Verhalten gegenueber vorher.
        let takeoff = Utc::now();
        let landing = takeoff + chrono::Duration::hours(2);
        let inflight_pause = segment(
            takeoff + chrono::Duration::minutes(30),
            takeoff + chrono::Duration::minutes(53),
        );
        let segments = vec![inflight_pause];
        assert_eq!(
            pause_secs_within(&segments, 23 * 60, takeoff, landing),
            23 * 60
        );
    }

    #[test]
    fn a_pause_straddling_takeoff_counts_only_its_overlap() {
        // Verbindung bricht kurz vor dem Start ab, kommt kurz danach
        // zurueck — nur der Teil NACH dem Start zaehlt als verlorene
        // Flugzeit.
        let takeoff = Utc::now();
        let landing = takeoff + chrono::Duration::hours(1);
        let straddling = segment(
            takeoff - chrono::Duration::minutes(5),
            takeoff + chrono::Duration::minutes(10),
        );
        let segments = vec![straddling];
        assert_eq!(
            pause_secs_within(&segments, 15 * 60, takeoff, landing),
            10 * 60
        );
    }

    #[test]
    fn a_pause_entirely_after_landing_does_not_count() {
        let takeoff = Utc::now();
        let landing = takeoff + chrono::Duration::hours(1);
        let post_landing = segment(
            landing + chrono::Duration::minutes(5),
            landing + chrono::Duration::minutes(20),
        );
        let segments = vec![post_landing];
        assert_eq!(pause_secs_within(&segments, 15 * 60, takeoff, landing), 0);
    }

    #[test]
    fn multiple_segments_each_contribute_only_their_own_overlap() {
        let takeoff = Utc::now();
        let landing = takeoff + chrono::Duration::hours(3);
        let segments = vec![
            // Vor dem Start — zaehlt nicht.
            segment(
                takeoff - chrono::Duration::minutes(40),
                takeoff - chrono::Duration::minutes(5),
            ),
            // Mitten im Flug — zaehlt voll.
            segment(
                takeoff + chrono::Duration::minutes(60),
                takeoff + chrono::Duration::minutes(75),
            ),
            // Nach der Landung — zaehlt nicht.
            segment(
                landing + chrono::Duration::minutes(2),
                landing + chrono::Duration::minutes(6),
            ),
        ];
        assert_eq!(
            pause_secs_within(&segments, 40 * 60, takeoff, landing),
            15 * 60
        );
    }

    #[test]
    fn empty_segments_falls_back_to_the_old_total_based_behaviour() {
        // Vor v1.7.20 wiederaufgenommene Session (oder aus einem anderen
        // Grund ohne Segment-Detail): ohne Segmente laesst sich nicht
        // sagen, auf welcher Seite die Pause lag — die alte, konservative
        // Voll-Summe bleibt erhalten statt stillschweigend auf 0 zu fallen.
        let takeoff = Utc::now();
        let landing = takeoff + chrono::Duration::hours(1);
        assert_eq!(pause_secs_within(&[], 900, takeoff, landing), 900);
    }

    #[test]
    fn empty_segments_with_negative_total_clamps_to_zero() {
        let takeoff = Utc::now();
        let landing = takeoff + chrono::Duration::hours(1);
        assert_eq!(pause_secs_within(&[], -100, takeoff, landing), 0);
    }

    // ─── Drift-Schwellen-Konstanten: F2 UI-Stufen-Mapping ───

    #[test]
    fn drift_threshold_constants_are_monotonic() {
        // Schwellen muessen in aufsteigender Reihenfolge stehen,
        // sonst greifen die match-Arms in apply_pause_resume nicht
        // in der erwarteten Reihenfolge.
        assert!(RESUME_DRIFT_TOAST_NM < RESUME_DRIFT_WARN_NM);
        assert!(RESUME_DRIFT_WARN_NM < RESUME_DRIFT_EXTREME_NM);
    }

    #[test]
    fn drift_threshold_values_match_spec_v0_4() {
        // Spec sim-disconnect-auto-resume v0.4 § "Drift-Level fuer MVP":
        //   < 1 NM   Info ohne Suffix
        //   1-50 NM  Info "repositioniert X nm"
        //   50-200   Warn
        //   > 200    Warn "Sehr grosse Reposition"
        assert_eq!(RESUME_DRIFT_TOAST_NM, 1.0);
        assert_eq!(RESUME_DRIFT_WARN_NM, 50.0);
        assert_eq!(RESUME_DRIFT_EXTREME_NM, 200.0);
    }

    // ─── PauseSegment: serde-Roundtrip damit Persistence haelt ───

    #[test]
    fn pause_segment_serde_roundtrip() {
        let original = PauseSegment {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            duration_secs: 60,
            reason: PauseReason::SimDisconnect,
            drift_nm: Some(2.5),
            altitude_delta_ft: Some(50.0),
            fuel_delta_kg: Some(10.0),
        };
        let json = serde_json::to_string(&original).expect("serialize");
        let decoded: PauseSegment = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.duration_secs, 60);
        assert_eq!(decoded.reason, PauseReason::SimDisconnect);
        assert_eq!(decoded.drift_nm, Some(2.5));
    }

    #[test]
    fn pause_segment_handles_missing_optional_fields() {
        // Vorgaenger-PauseSegment-Records ohne drift/alt/fuel-Felder
        // muessen weiter ladbar sein (Spec F2: serde(default)).
        let minimal_json = r#"{
            "started_at": "2026-05-11T11:00:00Z",
            "ended_at": "2026-05-11T11:01:00Z",
            "duration_secs": 60,
            "reason": "sim_disconnect"
        }"#;
        let decoded: PauseSegment = serde_json::from_str(minimal_json).expect("deserialize");
        assert_eq!(decoded.duration_secs, 60);
        assert_eq!(decoded.drift_nm, None);
        assert_eq!(decoded.altitude_delta_ft, None);
        assert_eq!(decoded.fuel_delta_kg, None);
    }

    // ─── PersistedFlightStats: serde-default Backward-Compat ───

    #[test]
    fn persisted_flight_stats_loads_without_pause_fields() {
        // Pre-Spec active_flight.json (v0.7.14) hat weder
        // `pause_total_duration_secs` noch `pause_segments`. Beide
        // muessen mit serde(default) sauber default-en.
        let pre_spec_json = r#"{"distance_nm": 12.5}"#;
        let decoded: PersistedFlightStats =
            serde_json::from_str(pre_spec_json).expect("legacy json must load");
        assert_eq!(decoded.pause_total_duration_secs, 0);
        assert!(decoded.pause_segments.is_empty());
        assert_eq!(decoded.distance_nm, 12.5);
    }

    #[test]
    fn persisted_flight_stats_roundtrips_with_pause_fields() {
        let mut stats = FlightStats::default();
        stats.pause_total_duration_secs = 720;
        stats.pause_segments.push(PauseSegment {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            duration_secs: 720,
            reason: PauseReason::ManualResume,
            drift_nm: None,
            altitude_delta_ft: None,
            fuel_delta_kg: None,
        });
        let snapshot = PersistedFlightStats::snapshot_from(&stats);
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let decoded: PersistedFlightStats = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.pause_total_duration_secs, 720);
        assert_eq!(decoded.pause_segments.len(), 1);
        assert_eq!(decoded.pause_segments[0].reason, PauseReason::ManualResume);
    }

    /// ⚠ Der Neustart darf die Kette nicht unterbrechen.
    ///
    /// Zwei Felder haengen daran, und beide fehlten zuerst:
    ///
    /// * `bahn_revision` — geht auf die Leitung und entscheidet beim
    ///   Recorder, welcher Nachtrag gewinnt. Ohne Persistenz faengt sie
    ///   nach dem Neustart wieder bei null an: Der Recorder hat Revision
    ///   7 gespeichert, bekommt die tatsaechlich neuere Zuordnung als 1
    ///   und weist sie ab. Die Korrektur ist dann fuer immer verloren,
    ///   und nirgends wird etwas rot.
    /// * `aufsetz_simulator` — ohne ihn kehrt
    ///   `bahn_am_aufsetzpunkt_nachholen` nach dem Resume sofort zurueck,
    ///   und die spaet eingetroffene Szenerie erreicht den Flug nie.
    #[test]
    fn revision_und_aufsetz_simulator_ueberleben_den_neustart() {
        let mut stats = FlightStats::default();
        stats.bahn_revision = 7;
        stats.aufsetz_simulator = Some(sim_core::Simulator::Msfs2024);

        let json =
            serde_json::to_string(&PersistedFlightStats::snapshot_from(&stats)).expect("serialize");
        let decoded: PersistedFlightStats = serde_json::from_str(&json).expect("deserialize");
        let mut nachher = FlightStats::default();
        decoded.apply_to(&mut nachher);

        assert_eq!(
            nachher.bahn_revision, 7,
            "die Revision faellt beim Neustart auf null zurueck — der \
             Recorder weist die naechste Korrektur dann als veraltet ab"
        );
        assert_eq!(
            nachher.aufsetz_simulator,
            Some(sim_core::Simulator::Msfs2024),
            "der Aufsetz-Simulator ueberlebt den Neustart nicht — das \
             Nachholen faellt danach vollstaendig aus"
        );
    }

    /// Und ein Stand von VOR dieser Version laedt weiterhin.
    #[test]
    fn ein_alter_stand_ohne_die_beiden_felder_laedt_weiter() {
        let alt = r#"{"distance_nm": 3.0}"#;
        let decoded: PersistedFlightStats = serde_json::from_str(alt).expect("legacy json");
        assert_eq!(decoded.bahn_revision, 0);
        assert!(decoded.aufsetz_simulator.is_none());
    }

    /// ⚠ Eine v1.7.14-Flugdatei behaelt ihren Simulator.
    ///
    /// Sie kennt `aufsetz_simulator` nicht, fuehrt aber
    /// `landing_simulator`. Ohne den Rueckfall faellt das Nachholen bei
    /// genau den Fluegen aus, die auf v1.7.15 fortgesetzt werden — also
    /// bei den ersten, die die neue Mechanik ueberhaupt sehen.
    #[test]
    fn eine_alte_flugdatei_leitet_den_aufsetz_simulator_ab() {
        for (gespeichert, erwartet) in [
            ("msfs", Some(sim_core::Simulator::Msfs2024)),
            ("xplane", Some(sim_core::Simulator::XPlane12)),
            ("other", None),
        ] {
            let alt = format!(r#"{{"landing_simulator": "{gespeichert}"}}"#);
            let decoded: PersistedFlightStats = serde_json::from_str(&alt).expect("legacy json");
            let mut stats = FlightStats::default();
            decoded.apply_to(&mut stats);
            assert_eq!(
                stats.aufsetz_simulator, erwartet,
                "`{gespeichert}` wird nicht abgeleitet — der fortgesetzte \
                 Flug kann dann nicht mehr nachholen"
            );
        }
    }

    /// ⚠ Ausrollen, Spur und Fahne ueberleben den Neustart (N6/N7).
    ///
    /// Ohne sie baute der Einreich-Nachtrag nach einem Neustart mit
    /// leerer Spur `rollout_final: false` und drehte eine endgueltige
    /// Zeile zurueck; die Fahne einer offenen Korrektur war weg.
    #[test]
    fn ausrollen_spur_und_fahne_ueberleben_den_neustart() {
        let mut stats = FlightStats::default();
        stats.rollout_finalized = true;
        stats.bahn_spur = vec![(0.0, 1.0), (500.0, -2.0)];
        stats.bahn_spur_laeuft = false;
        let bis = chrono::DateTime::<Utc>::from_timestamp(1_755_000_000, 0).expect("Zeit");
        stats.bahn_spur_bis = Some(bis);
        stats.bahn_nachtrag_offen = true;
        let json =
            serde_json::to_string(&PersistedFlightStats::snapshot_from(&stats)).expect("serialize");
        let decoded: PersistedFlightStats = serde_json::from_str(&json).expect("deserialize");
        let mut nachher = FlightStats::default();
        decoded.apply_to(&mut nachher);
        assert!(nachher.rollout_finalized, "Ausrollen vergessen");
        assert_eq!(
            nachher.bahn_spur,
            vec![(0.0, 1.0), (500.0, -2.0)],
            "Spur vergessen"
        );
        assert!(!nachher.bahn_spur_laeuft);
        assert_eq!(nachher.bahn_spur_bis, Some(bis));
        assert!(nachher.bahn_nachtrag_offen, "Fahne vergessen");
    }

    /// ⚠ Der Szenerie-Status kippt nach dem Neustart nicht auf eine vagere
    /// Aussage (Runde 5, N26).
    #[test]
    fn der_szenerie_status_ueberlebt_den_verlust_der_auskunft() {
        let mut stats = FlightStats::default();
        stats.szenerie_uebernahme = Some(szenerie_bahn::UebernahmeBericht {
            ohne_treffer: vec!["26R".to_string()],
            ..Default::default()
        });
        let bahn = sim_core::szenerie::SzenerieBahn {
            bezeichner: "26R".to_string(),
            kurs_grad: 265.7,
            breite_m: 45.0,
            laenge_m: 3600.0,
            versetzte_schwelle_m: 0.0,
            schwelle: (51.4336, 12.2674),
            gegenende: (51.4312, 12.2158),
            belag_code: 1,
            kurs_bestaetigt_grad: None,
            kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
        };
        stats.szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
            icao: "EDDP".to_string(),
            bahnen: vec![bahn.clone(), bahn],
            ..Default::default()
        });
        let genau = szenerie_status(&stats);
        assert!(genau.starts_with("kein_treffer(bahnen=2"), "{genau}");
        stats.szenerie_status_fest = Some(genau.clone());
        // Neustart: die Auskunft ist weg.
        stats.szenerie_auskunft = None;
        assert_eq!(
            szenerie_status(&stats),
            genau,
            "ohne Auskunft kippt der Status auf eine vagere Aussage und \
             ueberschreibt die genauere beim Recorder"
        );
    }

    /// Ein Ueberflugplatz darf die Auskunft des Ziels nicht wegraeumen.
    ///
    /// DLH 373, KJFK→EDDM, 20.09.2026: Um 15:38 lag EDDM mit 682
    /// Rollwegen vor. Im Sinkflug korrelierte die Bahn kurz mit EDML
    /// (Landshut, liegt auf der Anflugstrecke), das Ernteziel wechselte
    /// dorthin, und `szenerie_auskunft` wurde verworfen — richtig, denn
    /// fremde Rollwege duerfen nicht in die Ausfahrten geraten. EDML
    /// lieferte 0 Rollwege. Beim Aufsetzen in Muenchen war damit KEINE
    /// Bodenkarte da: keine Ausfahrten, keine Bemassung in der
    /// Queransicht. Die richtige Auskunft traf vier Sekunden nach dem
    /// Aufsetzen wieder ein.
    #[test]
    fn ein_ueberflugplatz_raeumt_die_auskunft_des_ziels_nicht_weg() {
        let mut stats = FlightStats::default();
        let auskunft = |icao: &str, rollwege: usize| sim_core::szenerie::SzenerieFlughafen {
            icao: icao.to_string(),
            rollwege: (0..rollwege)
                .map(|i| sim_core::szenerie::SzenerieRollweg {
                    name: format!("A{i}"),
                    punkte: vec![(48.35, 11.78), (48.36, 11.79)],
                })
                .collect(),
            ..Default::default()
        };
        let liefere = |a: sim_core::szenerie::SzenerieFlughafen, stand: u32| {
            sim_core::szenerie::Schnappschuss {
                generation: 0,
                auskunft: Some((Arc::new(a), stand)),
                diagnose: "geliefert".to_string(),
                kennung: None,
            }
        };

        // 15:38 — Muenchen liefert, Ernteziel ist EDDM.
        schnappschuss_uebernehmen(
            &mut stats,
            Some("EDDM"),
            "EDDM",
            liefere(auskunft("EDDM", 682), 1),
        );
        assert_eq!(
            stats.szenerie_auskunft.as_ref().map(|a| a.rollwege.len()),
            Some(682),
        );
        assert_eq!(
            stats
                .ziel_szenerie_auskunft
                .as_ref()
                .map(|a| a.rollwege.len()),
            Some(682),
            "die Auskunft des geplanten Ziels wurde nicht mitgeschrieben",
        );

        // 20:55 — im Sinkflug korreliert die Bahn mit Landshut.
        schnappschuss_uebernehmen(
            &mut stats,
            Some("EDML"),
            "EDDM",
            liefere(auskunft("EDML", 0), 2),
        );
        assert_eq!(
            stats.szenerie_auskunft.as_ref().map(|a| a.icao.as_str()),
            Some("EDML"),
            "Vorrichtung: das Ernteziel ist gewechselt",
        );
        // DAS ist der Punkt: Muenchen liegt weiterhin bereit.
        assert_eq!(
            stats
                .ziel_szenerie_auskunft
                .as_ref()
                .map(|a| a.rollwege.len()),
            Some(682),
            "der Ueberflugplatz hat die Auskunft des Ziels weggeraeumt",
        );
    }

    /// ⚠ Der feste Status faellt mit der Auskunft, die er beschreibt
    /// (Runde 6, Befund 5) — aber NICHT nach einem Neustart.
    #[test]
    fn der_feste_status_faellt_mit_der_auskunft_nicht_mit_dem_neustart() {
        let mut stats = FlightStats::default();
        stats.szenerie_auskunft = arc_some(sim_core::szenerie::SzenerieFlughafen {
            icao: "EDDP".to_string(),
            ..Default::default()
        });
        stats.szenerie_status_fest =
            Some("kein_treffer(bahnen=2, ohne_treffer=1, verworfen=1)".to_string());
        let leer = || sim_core::szenerie::Schnappschuss {
            generation: 0,
            auskunft: None,
            diagnose: "keine_antwort".to_string(),
            kennung: None,
        };
        // Ausweichen: das Ernteziel ist jetzt EDDC, die Auskunft ist fremd.
        schnappschuss_uebernehmen(&mut stats, Some("EDDC"), "EDDC", leer());
        assert!(
            stats.szenerie_auskunft.is_none(),
            "Vorrichtung: Auskunft verworfen"
        );
        assert!(
            stats.szenerie_status_fest.is_none(),
            "der Status des vorigen Platzes klebt am neuen — auf der Leitung \
             stuende eine erfundene Auskunft"
        );
        // Neustart: keine Auskunft, aber ein fester Status — der bleibt.
        //
        // ⚠ Runde 7 (N1): Mit `generation: 0` lief hier KEIN Zweig —
        // `flugkopie_entwerten(0, 0)` ist false, `fremd` bei `(None, _)`
        // auch. Der Riegel `if szenerie_auskunft.is_some()` war damit
        // ungedeckt. Nach einem echten Neustart steht die Generation im
        // Buch ueber 0, die persistierte bei 0 — genau das hier.
        stats.szenerie_status_fest = Some("uebernommen".to_string());
        assert_eq!(
            stats.szenerie_auskunft_generation, 0,
            "Vorrichtung: frisch geladen"
        );
        let mut neustart = leer();
        neustart.generation = 1;
        schnappschuss_uebernehmen(&mut stats, Some("EDDC"), "EDDC", neustart);
        assert_eq!(
            stats.szenerie_auskunft_generation, 1,
            "der Generationszweig lief nicht"
        );
        assert_eq!(
            stats.szenerie_status_fest.as_deref(),
            Some("uebernommen"),
            "der Generationswechsel nach dem Neustart wischt den festen Status weg — \
             dann ist N26 wieder offen"
        );
    }

    /// ⚠ Auch die MARKEN ueberleben den Neustart — nicht nur die Spur.
    ///
    /// Nach dem Neustart sendete der Streamer den Nachtrag erneut, mit
    /// Spur, aber ohne Marken: Die gingen als `null` hinaus und loeschten
    /// beim Recorder gute Werte (externe QS, Runde 3, P1). Und die Sperre
    /// wird aus dem Flug geseedet, damit er ueberhaupt nicht blind sendet.
    #[test]
    fn die_marken_und_der_letzte_nachtrag_ueberleben_den_neustart() {
        let mut stats = FlightStats::default();
        stats.bahn_max_querversatz_m = Some(4.5);
        stats.bahn_overrun_m = Some(12.0);
        stats.bahn_proben = 77;
        stats.bahn_fenster_zu = true;
        stats.bahn_fenster_zu_laengs_m = Some(1400.0);
        stats.bahn_raeum_laengs_m = Some(1500.0);
        stats.bahn_raeum_gs_kt = Some(38.0);
        stats.bahn_raeum_seite = Some("right".to_string());
        stats.bahn_kante_laengs_m = Some(1550.0);
        stats.bahn_kante_gs_kt = Some(30.0);
        stats.bahn_raeum_kurs_diff = Some(12.5);
        stats.rollout_finalize_reason = Some("exit_speed".to_string());
        stats.landing_touchdown_zone = Some(2);
        stats.landing_float_distance_m = Some(310.0);
        stats.letzter_nachtrag = Some((1_755_000_000_000, 3));
        stats.szenerie_uebernahme = Some(szenerie_bahn::UebernahmeBericht {
            uebernommen: vec!["26R".to_string()],
            groesste_schwellenabweichung_m: 350.5,
            ..Default::default()
        });
        stats.sim_kennung = Some("MSFS 2024".to_string());
        stats.runway_tch_actual_ft = Some(48.2);
        stats.fahrwerk_spurweite_m = Some(7.59);
        stats.bahn_spur_bezug_veraltet = true;
        stats.szenerie_status_fest =
            Some("kein_treffer(bahnen=40, ohne_treffer=1, verworfen=39)".to_string());
        stats.aufgeloestes_muster = Some("A20N".to_string());
        let json =
            serde_json::to_string(&PersistedFlightStats::snapshot_from(&stats)).expect("serialize");
        let decoded: PersistedFlightStats = serde_json::from_str(&json).expect("deserialize");
        let mut n = FlightStats::default();
        decoded.apply_to(&mut n);
        assert_eq!(n.bahn_max_querversatz_m, Some(4.5));
        assert_eq!(n.bahn_overrun_m, Some(12.0));
        assert_eq!(n.bahn_proben, 77);
        assert!(n.bahn_fenster_zu);
        assert_eq!(n.bahn_fenster_zu_laengs_m, Some(1400.0));
        assert_eq!(n.bahn_raeum_laengs_m, Some(1500.0));
        assert_eq!(n.bahn_raeum_gs_kt, Some(38.0));
        assert_eq!(n.bahn_raeum_seite.as_deref(), Some("right"));
        assert_eq!(n.bahn_kante_laengs_m, Some(1550.0));
        assert_eq!(n.bahn_kante_gs_kt, Some(30.0));
        assert_eq!(n.bahn_raeum_kurs_diff, Some(12.5));
        assert_eq!(n.rollout_finalize_reason.as_deref(), Some("exit_speed"));
        assert_eq!(
            n.landing_touchdown_zone,
            Some(2),
            "das Drittel vergessen — die Spalte wird geleert"
        );
        assert_eq!(n.landing_float_distance_m, Some(310.0));
        assert_eq!(
            n.letzter_nachtrag,
            Some((1_755_000_000_000, 3)),
            "die Sperre startet bei None — blindes Senden"
        );
        assert_eq!(
            n.szenerie_uebernahme.as_ref().map(|b| b.uebernommen.len()),
            Some(1),
            "der Szenerie-Bericht vergessen — die Herkunft kippt auf „navdaten\""
        );
        assert_eq!(n.sim_kennung.as_deref(), Some("MSFS 2024"));
        assert_eq!(n.runway_tch_actual_ft, Some(48.2));
        assert_eq!(n.fahrwerk_spurweite_m, Some(7.59));
        assert!(
            n.bahn_spur_bezug_veraltet,
            "veraltete Spur wird nach Neustart wieder gesendet"
        );
        assert_eq!(
            n.szenerie_status_fest.as_deref(),
            Some("kein_treffer(bahnen=40, ohne_treffer=1, verworfen=39)")
        );
        assert_eq!(
            n.aufgeloestes_muster.as_deref(),
            Some("A20N"),
            "wingspan faellt aufs gebuchte Muster"
        );
    }

    /// Und der gespeicherte Wert schlaegt die Ableitung.
    #[test]
    fn der_gespeicherte_aufsetz_simulator_schlaegt_die_ableitung() {
        let mut stats = FlightStats::default();
        stats.landing_simulator = Some("xplane");
        stats.aufsetz_simulator = Some(sim_core::Simulator::Msfs2020);
        let json =
            serde_json::to_string(&PersistedFlightStats::snapshot_from(&stats)).expect("ser");
        let decoded: PersistedFlightStats = serde_json::from_str(&json).expect("de");
        let mut nachher = FlightStats::default();
        decoded.apply_to(&mut nachher);
        assert_eq!(
            nachher.aufsetz_simulator,
            Some(sim_core::Simulator::Msfs2020)
        );
    }

    // ─── v0.7.15 F5/F6: SimPause-Reason serde + State-Persistenz ───

    #[test]
    fn sim_pause_reason_serde_roundtrip() {
        // Spec v0.7.15 F5: PauseReason::SimPause muss in PauseSegments
        // serialisierbar sein (sonst gehen Esc-Pause-Audit-Daten
        // verloren beim active_flight.json-Roundtrip).
        let segment = PauseSegment {
            started_at: Utc::now(),
            ended_at: Utc::now(),
            duration_secs: 120,
            reason: PauseReason::SimPause,
            drift_nm: Some(0.0),
            altitude_delta_ft: Some(0.0),
            fuel_delta_kg: Some(0.0),
        };
        let json = serde_json::to_string(&segment).expect("serialize");
        // Snake-case-Konvention im JSON.
        assert!(
            json.contains("\"sim_pause\""),
            "expected snake_case reason in JSON, got: {}",
            json
        );
        let decoded: PauseSegment = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.reason, PauseReason::SimPause);
    }

    #[test]
    fn current_pause_reason_persists_across_resume() {
        // Spec v0.7.15 F5/F6: current_pause_reason muss im
        // PersistedFlightStats-Roundtrip erhalten bleiben damit ein
        // App-Restart waehrend laufender Sim-Pause die Reason behaelt
        // (= apply_pause_resume nimmt sie wieder auf).
        let mut stats = FlightStats::default();
        stats.current_pause_reason = Some(PauseReason::SimPause);
        let snapshot = PersistedFlightStats::snapshot_from(&stats);
        let json = serde_json::to_string(&snapshot).expect("serialize");
        let decoded: PersistedFlightStats = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(decoded.current_pause_reason, Some(PauseReason::SimPause));
    }

    #[test]
    fn current_pause_reason_defaults_to_none_in_legacy_json() {
        // Pre-v0.7.15 active_flight.json hat das Feld nicht — serde
        // muss es defaulten auf None damit alte Files weiter laden.
        let legacy_json = r#"{"distance_nm": 0.0}"#;
        let decoded: PersistedFlightStats =
            serde_json::from_str(legacy_json).expect("legacy json must load");
        assert_eq!(decoded.current_pause_reason, None);
    }

    // ---- v0.12.1 Stream A — Boarding→Pushback de-glitch (LE1 + LE2) ----

    #[test]
    fn boarding_glitch_sample_is_not_real_movement() {
        // BTX8815: a post-reload tick spiked gs to 2.6 kt but the aircraft
        // had not moved (bit-identical lat/lon → 0 m displacement). That
        // must NOT count as real movement → no Boarding→Pushback.
        assert!(!boarding_real_movement(true, true, true, 0.0));
    }

    #[test]
    fn boarding_single_gs_spike_is_not_real_movement() {
        // gs over threshold this tick but not the previous one — not
        // sustained (no de-bounce) → no transition, even with displacement.
        assert!(!boarding_real_movement(true, true, false, 12.0));
    }

    #[test]
    fn boarding_sustained_movement_with_displacement_triggers() {
        // gs over threshold across two ticks + real displacement
        // > MIN_BOARDING_MOVEMENT_M → genuine movement.
        assert!(boarding_real_movement(true, true, true, 6.0));
    }

    #[test]
    fn boarding_movement_below_threshold_does_not_trigger() {
        // 4 m sits inside the GPS-jitter band (< MIN_BOARDING_MOVEMENT_M).
        assert!(!boarding_real_movement(true, true, true, 4.0));
    }

    #[test]
    fn boarding_airborne_sample_is_not_surface_movement() {
        // not on the surface → the Boarding pushback trigger never fires.
        assert!(!boarding_real_movement(false, true, true, 50.0));
    }

    #[test]
    fn reload_gap_detects_btx8815_eleven_second_gap() {
        // BTX8815 EDVE: an 11 s gap in Boarding (position_interval = 4 s).
        // 11 > 2.5 × 4 = 10 → reload-suspect, the glitch tick is ignored.
        assert!(is_reload_gap(11.0, 4.0));
    }

    #[test]
    fn reload_gap_ignores_normal_tick_cadence() {
        // A normal ~4-5 s Boarding tick is not a reload gap. The threshold
        // is exclusive: exactly 2.5× the interval does not count.
        assert!(!is_reload_gap(5.0, 4.0));
        assert!(!is_reload_gap(10.0, 4.0));
    }

    #[test]
    fn pushback_reload_gap_does_not_taxiout() {
        // QS-P2: a reload-suspect tick must not drive Pushback→TaxiOut
        // either — even with engines on and groundspeed over threshold.
        // (v0.15.13: vbz=forward mitgegeben — der reload-/gs-Gate macht die
        // ersten/letzten Fälle ohnehin false.)
        assert!(!powered_taxi_move(true, true, true, 12.0, Some(20.0)));
        // Without the reload gap, the same inputs DO mean powered taxi.
        assert!(powered_taxi_move(false, true, true, 12.0, Some(20.0)));
        // Below the 3 kt threshold it is never powered taxi.
        assert!(!powered_taxi_move(false, true, true, 2.0, Some(20.0)));
    }

    // ---- v0.12.1 Stream E — resume position check (LE14) ----

    /// Referenz ohne Zeit/Tempo (Altstand) — Weite faellt auf die alte
    /// 50-nm-Grenze zurueck.
    fn alte_referenz(phase: FlightPhase) -> LageReferenz {
        LageReferenz {
            phase,
            lat: 52.0,
            lon: 10.0,
            alt_ft: None,
            fuel_kg: None,
            on_ground: false,
            gs_kt: None,
            at: None,
            aufgesetzt: None,
        }
    }

    fn sim_bei(lat: f64, lon: f64, alt_ft: f64, on_ground: bool) -> SimSnapshot {
        SimSnapshot {
            lat,
            lon,
            altitude_msl_ft: alt_ft,
            altitude_agl_ft: if on_ground { 0.0 } else { alt_ft },
            on_ground,
            ..SimSnapshot::default()
        }
    }

    #[test]
    fn resume_suspect_when_airborne_phase_but_on_ground() {
        // Sim crash in the cruise → reload spawns the aircraft on a runway.
        assert_eq!(
            lage_abweichung(
                &alte_referenz(FlightPhase::Cruise),
                &sim_bei(52.0, 10.0, 300.0, true),
                Utc::now(),
                true
            ),
            Some(LageAbweichung::AmBoden)
        );
    }

    #[test]
    fn resume_suspect_on_large_drift() {
        // Persisted near (52,10), sim now ~60 NM away (1° latitude).
        assert!(matches!(
            lage_abweichung(
                &alte_referenz(FlightPhase::Cruise),
                &sim_bei(53.0, 10.0, 35_000.0, false),
                Utc::now(),
                true
            ),
            Some(LageAbweichung::ZuWeitWeg { .. })
        ));
    }

    #[test]
    fn resume_not_suspect_on_altitude_delta_while_airborne() {
        // Airborne → still airborne, position matches. A pure altitude
        // delta is NOT flagged (the altitude condition was dropped, QS-R5).
        assert_eq!(
            lage_abweichung(
                &alte_referenz(FlightPhase::Descent),
                &sim_bei(52.0, 10.0, 12_000.0, false),
                Utc::now(),
                true
            ),
            None
        );
    }

    #[test]
    fn resume_not_suspect_when_position_matches() {
        // App restart, sim kept running — ground phase, on ground, no drift.
        assert_eq!(
            lage_abweichung(
                &alte_referenz(FlightPhase::Boarding),
                &sim_bei(52.0, 10.0, 300.0, true),
                Utc::now(),
                true
            ),
            None
        );
    }

    // ---- v0.13.9 Stream F (LE27) — Mid-Session Sim-Crash-Recovery ----

    #[test]
    fn mid_session_recovery_michel_reload_1_cruise_to_edtd_ground() {
        // Reale Werte ITY 608 / Michel 2026-05-25 12:32:15 UTC:
        // FL320 (31993 ft) airborne im Cruise → 2117 ft on_ground bei
        // EDTD Donaueschingen. Alt-Drop: 29876 ft.
        assert!(is_mid_session_sim_crash_recovery(
            FlightPhase::Cruise,
            false, // prev airborne
            31993.0,
            true, // curr grounded
            2117.0,
        ));
    }

    #[test]
    fn mid_session_recovery_michel_reload_2_caen() {
        // Reale Werte 13:31:06 UTC: FL340 → -6 ft on_ground bei LFRK Caen.
        assert!(is_mid_session_sim_crash_recovery(
            FlightPhase::Cruise,
            false,
            33999.0,
            true,
            -6.0,
        ));
    }

    #[test]
    fn mid_session_recovery_michel_reload_3_newfoundland() {
        // Reale Werte 18:04:21 UTC: FL360 → 152 ft on_ground bei CYYT.
        assert!(is_mid_session_sim_crash_recovery(
            FlightPhase::Cruise,
            false,
            36001.0,
            true,
            152.0,
        ));
    }

    #[test]
    fn mid_session_recovery_not_triggered_during_normal_descent() {
        // Realer Descent kann max ~4000 fpm = ~200 ft pro 3-s-Tick. Selbst
        // ein extrem langer 30-s-Cruise-Tick kann maximal 2000 ft Drop
        // erzeugen. 1500 ft Drop ist also normaler Sink-Rate.
        assert!(!is_mid_session_sim_crash_recovery(
            FlightPhase::Descent,
            false,
            10000.0,
            false,
            8500.0,
        ));
    }

    #[test]
    fn mid_session_recovery_not_triggered_when_phase_not_airborne() {
        // Wenn die FSM bereits auf BlocksOn oder TaxiIn ist, ist on_ground
        // erwartet, nicht alarmierend. Der Trigger soll NUR bei mid-flight
        // Sim-Crash anschlagen.
        assert!(!is_mid_session_sim_crash_recovery(
            FlightPhase::BlocksOn,
            false,
            31993.0,
            true,
            2117.0,
        ));
    }

    #[test]
    fn mid_session_recovery_not_triggered_on_normal_landing() {
        // Normaler Touchdown: airborne → grounded, aber Altitude-Drop ist
        // beim Touchdown nur wenige Hundert Fuss (AGL). 50 ft Drop in
        // einem Tick = normaler Flare, kein Crash.
        assert!(!is_mid_session_sim_crash_recovery(
            FlightPhase::Final,
            false,
            150.0,
            true,
            100.0,
        ));
    }

    #[test]
    fn mid_session_recovery_not_triggered_when_already_grounded() {
        // Wenn prev bereits on_ground=true war, kann das kein Sim-Reload-
        // Recovery sein — wir waren bereits am Boden.
        assert!(!is_mid_session_sim_crash_recovery(
            FlightPhase::Cruise, // hypothetisch
            true,                // prev grounded
            2000.0,
            true,
            2000.0,
        ));
    }

    #[test]
    fn mid_session_recovery_not_triggered_when_still_airborne_after() {
        // Wenn curr noch airborne ist, ist's kein Reload (Recovery setzt
        // den Aircraft am Boden ab).
        assert!(!is_mid_session_sim_crash_recovery(
            FlightPhase::Cruise,
            false,
            36000.0,
            false, // still airborne
            10000.0,
        ));
    }

    // ---- v0.12.1 Stream C — approach-stability fixes (LE11 + LE12) ----

    /// Score-Version 23: beim spaeten Final zaehlt die Querneigung erst ab
    /// dem Ausrichten — und ein nicht rechtzeitiges Final ist nicht stabil.
    #[test]
    fn spaetes_final_misst_querneigung_ab_dem_ausrichten() {
        // 20 Proben Kurve (±25°) von 900 auf 520 ft, dann 20 ruhige Proben.
        let buf: std::collections::VecDeque<ApproachBufferSample> = (0..40)
            .map(|i| {
                let agl = 900.0 - i as f32 * 20.0;
                let mut a = approach_sample(agl, 120.0, 120.0, -600.0, 1.0, 1.0);
                a.bank_deg = if i >= 20 {
                    0.5
                } else if i % 2 == 0 {
                    25.0
                } else {
                    -25.0
                };
                a
            })
            .collect();
        let ohne =
            compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert!(ohne.bank_stddev_filtered_deg.unwrap() > 5.0, "Gegenprobe: Kurve zaehlt bisher");
        assert_eq!(ohne.stable_at_gate, Some(false));

        let sf = spaetes_final::SpaetesFinal {
            ab: buf[20].at,
            ab_hoehe_ft: 500.0,
            kurve_deg: 180.0,
            querneigung_max_unter_300_deg: 0.5,
            rechtzeitig: true,
        };
        let mit = compute_approach_stability_v2_mit_final(
            &buf,
            None,
            None,
            None,
            None,
            Default::default(),
            Some(&sf),
        );
        assert!(
            mit.bank_stddev_filtered_deg.unwrap() < 1.0,
            "ab dem Final ruhig: {:?}",
            mit.bank_stddev_filtered_deg
        );
        assert_eq!(mit.stable_at_gate, Some(true));

        let zu_spaet = spaetes_final::SpaetesFinal {
            rechtzeitig: false,
            ..sf
        };
        let r = compute_approach_stability_v2_mit_final(
            &buf,
            None,
            None,
            None,
            None,
            Default::default(),
            Some(&zu_spaet),
        );
        assert_eq!(r.stable_at_gate, Some(false), "Final nicht rechtzeitig");
    }

    fn approach_sample(
        agl: f32,
        gs: f32,
        ias: f32,
        vs: f32,
        gear: f32,
        flaps: f32,
    ) -> ApproachBufferSample {
        // v1.9.16: zwei Sekunden je Aufruf — die Sinkgrenze zaehlt erst nach
        // 10 s am Stueck, ein Helfer mit gleichem Zeitstempel koennte das
        // nie pruefen.
        thread_local! {
            static NAECHSTE_SEKUNDE: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
        }
        let sek = NAECHSTE_SEKUNDE.with(|c| {
            let v = c.get();
            c.set(v + 1);
            v
        });
        ApproachBufferSample {
            at: DateTime::<Utc>::from_timestamp(1_790_000_000 + sek * 2, 0).unwrap(),
            agl_ft: agl,
            msl_ft: agl,
            gs_kt: gs,
            ias_kt: ias,
            vs_fpm: vs,
            bank_deg: 0.0,
            heading_true_deg: 0.0,
            gear_position: gear,
            flaps_position: flaps,
            selected_runway: None,
            stall_warning: false,
            lat: None,
            lon: None,
            pitch_deg: None,
            n1_mittel_pct: None,
            headwind_kt: None,
            crosswind_kt: None,
        }
    }

    /// Die Muster, die die GSG-Flotte laut den 827 aufgezeichneten
    /// Fluegen vom 16.08.2026 tatsaechlich geflogen ist — genau so
    /// geschrieben, wie der Simulator sie meldet.
    ///
    /// Der Test prueft die Tabelle gegen die Wirklichkeit statt gegen eine
    /// Wunschliste. Genau daran ist die Positivliste dreimal gescheitert:
    /// sie wurde nachgetragen, wenn jemand ein Muster vermisste, nie
    /// abgeglichen mit dem, was wirklich fliegt.
    const GEFLOGENE_FLOTTE: &[(&str, u32)] = &[
        ("A320", 136),
        ("A321", 71),
        ("B738", 59),
        ("A21N", 54),
        ("A20N", 52),
        ("PHENOM 300E", 51),
        ("A319", 32),
        ("E55P", 23),
        ("HA420", 20),
        ("BE58", 20),
        ("B77L", 18),
        ("DH8D", 18),
        ("DA40", 17),
        ("C172", 17),
        ("BE36", 15),
        ("A350-900", 14),
        ("AC11", 12),
        ("CL60", 12),
        ("A346", 10),
        ("C208", 9),
        ("C182Q", 8),
        ("AEST", 7),
        ("FALCON 50", 6),
        ("PA24", 5),
        ("C680+", 5),
        ("B77W", 5),
        ("C750", 4),
        ("A300", 4),
        ("B752", 4),
        ("BE24", 4),
        ("A330", 3),
        ("A380", 3),
        ("A400M", 3),
        ("B763", 3),
        ("PA34", 3),
        ("A339", 3),
        ("A359", 3),
        ("MD11F", 2),
        ("A350-900 ULR", 2),
        ("B789", 2),
        ("A340-300", 2),
        ("A350-1000", 2),
        ("F28", 2),
        ("FA50", 2),
        ("B736", 1),
        ("414AW", 1),
        ("GA-8", 1),
        ("B772", 1),
        ("C185", 1),
        ("A343", 1),
        ("E170", 1),
        ("E190F", 1),
        ("MU2", 1),
        ("MD88", 1),
        ("RJ85", 1),
        ("TYPHOON", 1),
        ("B762", 1),
        ("E195", 1),
        ("B748", 1),
        ("B742", 1),
        ("C152", 1),
        ("MD11", 1),
        ("A109", 1),
        ("P28R", 1),
    ];

    /// Die gebuchte Flotte: `phpvmsaircraft.icao` ueber alle 3794 PIREPs,
    /// gemessen am 16.08.2026. Zweite Quelle neben `GEFLOGENE_FLOTTE`, weil
    /// die Bewertung beide Wege kennt — der Simulator meldet ein Muster, die
    /// Buchung ein anderes, und beide landen in derselben Tabelle.
    const GEBUCHTE_FLOTTE: &[(&str, u32)] = &[
        ("A320", 929),
        ("A321", 397),
        ("B738", 270),
        ("A319", 265),
        ("A20N", 202),
        ("A359", 129),
        ("A21N", 126),
        ("B77L", 117),
        ("A339", 109),
        ("E55P", 106),
        ("MD11", 81),
        ("A346", 77),
        ("CL60", 53),
        ("B772", 53),
        ("A388", 52),
        ("A333", 46),
        ("DH8D", 45),
        ("B763", 40),
        ("B77W", 35),
        ("B38M", 35),
        ("BCS3", 35),
        ("A35K", 31),
        ("A343", 30),
        ("BE58", 29),
        ("C750", 26),
        ("B742", 25),
        ("BE36", 24),
        ("B752", 24),
        ("C208", 24),
        ("DA40", 23),
        ("HDJT", 20),
        ("P28R", 19),
        ("B789", 18),
        ("AEST", 18),
        ("C172", 17),
        ("B748", 15),
        ("A332", 15),
        ("E190", 14),
        ("B737", 14),
        ("B744", 14),
        ("B736", 13),
        ("C680", 13),
        ("AC11", 12),
        ("A139", 11),
        ("FA50", 10),
        ("SF50", 9),
        ("C182", 8),
        ("PA24", 7),
        ("A400", 7),
        ("A109", 7),
        ("B350", 6),
        ("GA8", 6),
        ("A306", 6),
        ("C152", 6),
        ("P180", 5),
        ("L101", 5),
        ("PA34", 4),
        ("BE24", 4),
        ("E195", 4),
        ("C525", 3),
        ("MD88", 3),
        ("B76F", 3),
        ("RJ85", 3),
        ("TBM9", 3),
        ("AT76", 3),
        ("B762", 3),
        ("B78X", 2),
        ("B463", 2),
        ("CONC", 2),
        ("H145", 2),
        ("B753", 2),
        ("MU2", 2),
        ("F28", 2),
        ("C25C", 2),
        ("C185", 2),
        ("C414", 2),
        ("E135", 1),
        ("GLF5", 1),
        ("B739", 1),
        ("BE35", 1),
        ("DA42", 1),
        ("EUFI", 1),
        ("GLF6", 1),
        ("E170", 1),
    ];

    #[test]
    fn grenzen_tabelle_deckt_die_geflogene_flotte() {
        let luecken: Vec<(&str, u32)> = GEFLOGENE_FLOTTE
            .iter()
            .chain(GEBUCHTE_FLOTTE.iter())
            .copied()
            .filter(|(muster, _)| aircraft_limits_for(muster).is_fallback)
            .collect();
        let betroffene_fluege: u32 = luecken.iter().map(|(_, n)| n).sum();
        assert!(
            luecken.is_empty(),
            "{} Muster ohne Eintrag in der Grenzen-Tabelle, zusammen {} Fluege: {:?}\n\
             Entweder einen Eintrag ergaenzen oder in `muster_kandidaten` uebersetzen. \
             Diese Liste kommt aus echten Aufzeichnungen — sie zu kuerzen, statt die \
             Luecke zu schliessen, waere genau der Fehler, den der Test verhindern soll.",
            luecken.len(),
            betroffene_fluege,
            luecken,
        );
    }

    #[test]
    #[ignore = "Hilfsmittel: gibt die aufgeloesten Grenzen der Messflotte aus"]
    fn dump_flottengrenzen() {
        for (muster, _) in GEFLOGENE_FLOTTE.iter().chain(GEBUCHTE_FLOTTE.iter()) {
            let l = aircraft_limits_for(muster);
            println!(
                "DUMP\t{muster}\t{}\t{}",
                l.typical_vref_kt
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "-".into()),
                l.is_fallback
            );
        }
    }

    #[test]
    fn muster_kandidaten_uebersetzen_die_sim_schreibweisen() {
        // Airbus-Langform und Familie ohne Variante.
        assert_eq!(aircraft_limits_for("A350-900").typical_vref_kt, Some(140.0));
        assert_eq!(
            aircraft_limits_for("A350-1000").typical_vref_kt,
            Some(140.0)
        );
        assert_eq!(aircraft_limits_for("A340-300").typical_vref_kt, Some(138.0));
        assert!(!aircraft_limits_for("A380").is_fallback);
        // Frachter-Suffix, Bindestrich, Varianten-Anhang, fuehrende Zahl.
        assert!(!aircraft_limits_for("MD11F").is_fallback);
        assert!(!aircraft_limits_for("GA-8").is_fallback);
        assert!(!aircraft_limits_for("C680+").is_fallback);
        assert!(!aircraft_limits_for("C182Q").is_fallback);
        assert!(!aircraft_limits_for("414AW").is_fallback);
        // Marketingname.
        assert_eq!(
            aircraft_limits_for("PHENOM 300E").typical_vref_kt,
            Some(110.0)
        );

        // Und die Gegenprobe: ein echter Designator darf NICHT durch die
        // Varianten-Regel zerlegt werden, und Unbekanntes bleibt unbekannt.
        assert_eq!(aircraft_limits_for("B77W").typical_vref_kt, Some(150.0));
        assert!(aircraft_limits_for("QQQQ").is_fallback);
        assert!(aircraft_limits_for("").is_fallback);
        // Der Simulator meldet beliebigen Text — Mehrbyte-Zeichen duerfen
        // die Zerlegung nicht in eine Panik reissen.
        for muell in ["ÜBUNG", "Ä", "飛行機", "A—350", "🛫🛬"] {
            assert!(aircraft_limits_for(muell).is_fallback, "{muell}");
        }
    }

    /// DLH 780 (26.09.2026): Divert EDDF→EDDM statt WSSS. Mit der Höhe des
    /// geplanten Ziels (22 ft) kam der Endanflug auf München (1 487 ft) nie
    /// ins 1000-ft-Fenster — alle Anflugwerte blieben leer.
    #[test]
    fn divert_anflug_nutzt_schwellenhoehe_der_tatsaechlichen_bahn() {
        let mut stats = FlightStats::default();
        stats.arr_airport_elevation_ft = Some(22.0); // WSSS, geplant
        let bahn = |icao: &str| runway::RunwayMatch {
            airport_ident: icao.to_string(),
            runway_ident: "26L".to_string(),
            heading_true_deg: 262.0,
            length_ft: 13123.0,
            width_ft: 197.0,
            surface: "ASP".to_string(),
            threshold_lat: 48.34,
            threshold_lon: 11.82,
            end_lat: 48.33,
            end_lon: 11.76,
            centerline_distance_m: 0.0,
            centerline_distance_abs_ft: 0.0,
            touchdown_distance_from_threshold_ft: 1200.0,
            side: "left".to_string(),
            displaced_threshold_ft: 0,
            geometry_implied_displaced_threshold_ft: 0,
        };
        let nav = |elev: Option<i32>| aeroacars_mqtt::navdata::NavRunway {
            designator: "26L".to_string(),
            magnetic_course: 260.0,
            true_course: 262.0,
            length_ft: 13123,
            width_ft: Some(197),
            surface: None,
            threshold: aeroacars_mqtt::navdata::NavPoint {
                lat: 48.34,
                lon: 11.82,
                elev_ft: elev,
            },
            far_end: aeroacars_mqtt::navdata::NavPoint {
                lat: 48.33,
                lon: 11.76,
                elev_ft: elev,
            },
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: 3.0,
            tch_ft: 50,
        };
        // Ohne Zuordnung und am geplanten Ziel: unverändert die Zielhöhe.
        assert_eq!(anflug_bezugshoehe_ft(&stats, "WSSS", None), Some(22.0));
        stats.runway_correlation_icao = Some("WSSS".to_string());
        stats.runway_match = Some(bahn("WSSS"));
        stats.runway_nav_geometry = Some(nav(Some(30)));
        assert_eq!(
            anflug_bezugshoehe_ft(&stats, "wsss ", Some(22)),
            Some(22.0),
            "normaler Flug unverändert"
        );
        // QS: OurAirports-Rückfall liefert einen Nachbar-Ident, die
        // Zuordnung sagt aber „am Ziel" → kein Divert, Zielhöhe bleibt.
        stats.runway_match = Some(bahn("WSSL"));
        stats.runway_nav_geometry = None;
        assert_eq!(anflug_bezugshoehe_ft(&stats, "WSSS", None), Some(22.0));
        // Divert: Schwellenhöhe der tatsächlichen Bahn.
        stats.runway_correlation_icao = Some("EDDM".to_string());
        stats.runway_match = Some(bahn("EDDM"));
        stats.runway_nav_geometry = Some(nav(Some(1487)));
        assert_eq!(
            anflug_bezugshoehe_ft(&stats, "WSSS", Some(1500)),
            Some(1487.0)
        );
        // Divert, Schwelle ohne Höhe: Platzhöhe aus den Navdaten.
        stats.runway_nav_geometry = Some(nav(None));
        assert_eq!(
            anflug_bezugshoehe_ft(&stats, "WSSS", Some(1487)),
            Some(1487.0)
        );
        // Divert ohne jede Höhe: über Grund statt falscher Platzhöhe.
        assert_eq!(anflug_bezugshoehe_ft(&stats, "WSSS", None), None);

        // Durch die Rechnung: Endanflug 900/600/300 ft über München.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [900.0f32, 600.0, 300.0]
            .iter()
            .map(|agl| {
                let mut a = approach_sample(*agl, 140.0, 142.0, -700.0, 1.0, 0.6);
                a.msl_ft = agl + 1487.0;
                a
            })
            .collect();
        let alt =
            compute_approach_stability_v2(&buf, Some(22.0), None, None, None, Default::default());
        let neu =
            compute_approach_stability_v2(&buf, Some(1487.0), None, None, None, Default::default());
        assert_eq!(
            alt.window_sample_count, 0,
            "Gegenprobe: mit WSSS-Höhe leer (der Fehler)"
        );
        assert_eq!(
            neu.window_sample_count, 3,
            "mit EDDM-Schwelle alle drei Proben im Fenster"
        );
        assert!(neu.vs_jerk_fpm.is_some() || neu.stable_at_gate.is_some());
    }

    #[test]
    fn landing_config_softfails_when_flaps_unreadable() {
        // GSG225 / Hot-Start CL650: flaps dataref reads 0 the whole
        // approach, gear is down, speed is in the landing range. The
        // LANDING-CONFIG check must report not-assessable (None), not a
        // false "INCOMPLETE".
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 130.0, 132.0, -650.0, 1.0, 0.0),
            approach_sample(600.0, 128.0, 130.0, -620.0, 1.0, 0.0),
            approach_sample(300.0, 125.0, 128.0, -600.0, 1.0, 0.0),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.stable_config, None, "flaps unreadable → not assessable");
    }

    #[test]
    fn landing_config_still_fails_genuine_no_flaps() {
        // Flaps 0 but the approach speed is far too high for a flapped
        // landing — this is a genuine no-flaps approach, keep the fail.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 200.0, 200.0, -650.0, 1.0, 0.0),
            approach_sample(600.0, 195.0, 195.0, -620.0, 1.0, 0.0),
            approach_sample(300.0, 190.0, 190.0, -600.0, 1.0, 0.0),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.stable_config, Some(false), "genuine no-flaps → fail");
    }

    #[test]
    fn landekonfiguration_nicht_bewertbar_wenn_klappen_sich_nie_ruehren() {
        // Korpus 16.08.2026: 15 Fluege mit einem Klappenwert, der sich im
        // GANZEN Flug nicht bewegt — B77W, A339, MD11, A346, A20N, DH8D.
        // Der aeltere Riegel greift nur unter 160 kt; schwere Muster sind am
        // Tor legitim schneller und bekamen deshalb ein falsches INCOMPLETE.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 168.0, 170.0, -700.0, 1.0, 0.0),
            approach_sample(600.0, 165.0, 167.0, -680.0, 1.0, 0.0),
            approach_sample(300.0, 160.0, 162.0, -650.0, 1.0, 0.0),
        ]
        .into_iter()
        .collect();
        let konfig = KonfigKontext {
            typical_vref_kt: Some(150.0),
            flaps_bewegt: Some(false),
            flaps_max_im_flug: Some(0.0),
        };
        let out = compute_approach_stability_v2(&buf, None, None, None, None, konfig);
        assert_eq!(
            out.stable_config, None,
            "toter Klappenkanal → nicht bewertbar, egal wie schnell"
        );
    }

    /// RYR73 (06.10.2026, B738, EGSS 22): 178 kt mit Klappen 15 bei 1000 ft,
    /// Klappen 30 ab ~830 ft, 145 kt ab ~490 ft. Bei 1000 ft nicht stabil,
    /// ab 500 ft schon — Score-Version 20 macht daraus PARTIAL statt UNSTABLE.
    #[test]
    fn ryr73_ab_500_ft_stabil() {
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            (1000.0, 178.0, 0.625),
            (870.0, 170.0, 0.75),
            (830.0, 166.0, 0.875),
            (700.0, 157.0, 0.875),
            (600.0, 150.0, 0.875),
            (490.0, 145.0, 0.875),
            (400.0, 146.0, 0.875),
            (300.0, 144.0, 0.875),
            (200.0, 147.0, 0.875),
            (100.0, 144.0, 0.875),
        ]
        .into_iter()
        .map(|(h, ias, flaps)| approach_sample(h, ias, ias, -750.0, 1.0, flaps))
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.stable_config, Some(false), "Klappen 15 bei 1000 ft");
        assert!(out.ias_stddev_kt.unwrap() >= 8.0, "{:?}", out.ias_stddev_kt);
        assert_eq!(out.stable_config_500, Some(true), "Klappen 30 bei 490 ft");
        assert!(out.ias_stddev_unter_500_kt.unwrap() < 5.0);
        let werte = landing_scoring::anflug_urteil::AnflugWerte {
            vs_jerk_fpm: Some(49.0),
            bank_stddev_deg: Some(0.79),
            ias_stddev_kt: out.ias_stddev_kt,
            excessive_sink: out.excessive_sink,
            stable_config: out.stable_config,
            gleitpfad_dots: Some(0.34),
            ias_stddev_unter_500_kt: out.ias_stddev_unter_500_kt,
            stable_config_500: out.stable_config_500,
            spaetes_final_rechtzeitig: None,
        };
        assert_eq!(
            landing_scoring::anflug_urteil::anflug_urteil(&werte),
            Some(landing_scoring::anflug_urteil::AnflugUrteil::Partial)
        );
    }

    /// Durchstarten (Codex-QS): Die Punkte des ersten Anflugs unter 500 ft
    /// (schnell, Klappen 15) duerfen die 500-ft-Messung des letzten Anflugs
    /// nicht verderben.
    #[test]
    fn spaet_stabil_misst_nur_den_letzten_anflug() {
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            (450.0, 175.0, 0.625),
            (350.0, 172.0, 0.625),
            (700.0, 180.0, 0.625),
            (1000.0, 178.0, 0.625),
            (700.0, 160.0, 0.875),
            (490.0, 145.0, 0.875),
            (300.0, 144.0, 0.875),
            (150.0, 146.0, 0.875),
        ]
        .into_iter()
        .map(|(h, ias, flaps)| approach_sample(h, ias, ias, -700.0, 1.0, flaps))
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.stable_config_500, Some(true));
        assert!(
            out.ias_stddev_unter_500_kt.unwrap() < 5.0,
            "{:?}",
            out.ias_stddev_unter_500_kt
        );
    }

    /// Fahrwerk erst unter 500 ft unten → auch am 500-ft-Tor nicht stabil.
    #[test]
    fn fahrwerk_oben_bei_500_ft_ist_nicht_spaet_stabil() {
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            (1000.0, 150.0, 0.0),
            (480.0, 145.0, 0.875),
            (300.0, 145.0, 0.875),
            (150.0, 145.0, 0.875),
        ]
        .into_iter()
        .map(|(h, ias, flaps)| {
            approach_sample(
                h,
                ias,
                ias,
                -700.0,
                if h < 400.0 { 1.0 } else { 0.0 },
                flaps,
            )
        })
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.stable_config, Some(false));
        assert_eq!(out.stable_config_500, Some(false));
    }

    #[test]
    fn landekonfiguration_akzeptiert_fremdes_rastermass() {
        // Fokker 28 im Sim: das Raster endet bei 0,667, nicht bei 1,0. Der
        // Hebel steht am Tor auf dem Maximum dieses Fluges und die
        // Geschwindigkeit liegt im stabilisierten Fenster (Vref 122 + 10).
        // Die absolute 0,70-Schwelle misst hier das Raster, nicht den Piloten.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 130.0, 132.0, -650.0, 1.0, 0.667),
            approach_sample(600.0, 128.0, 130.0, -620.0, 1.0, 0.667),
            approach_sample(300.0, 125.0, 128.0, -600.0, 1.0, 0.667),
        ]
        .into_iter()
        .collect();
        let konfig = KonfigKontext {
            typical_vref_kt: Some(122.0),
            flaps_bewegt: Some(true),
            flaps_max_im_flug: Some(0.667),
        };
        let out = compute_approach_stability_v2(&buf, None, None, None, None, konfig);
        assert_eq!(
            out.stable_config,
            Some(true),
            "fremdes Raster → Landekonfiguration"
        );
    }

    #[test]
    fn landekonfiguration_faellt_bei_echter_unterklappung_durch() {
        // Phenom 300 im Korpus: Klappen 0,5 am Tor — auch das Maximum dieses
        // Fluges — aber 169 kt bei Vref 110, also Vref+59. Mit Anflugklappen
        // kommt man nicht auf Landegeschwindigkeit; die Geschwindigkeit ist
        // hier der Riegel, der die Rasterkorrektur oben nicht zum Freifahrt-
        // schein werden laesst.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 168.0, 169.0, -700.0, 1.0, 0.5),
            approach_sample(600.0, 165.0, 166.0, -680.0, 1.0, 0.5),
            approach_sample(300.0, 160.0, 162.0, -650.0, 1.0, 0.5),
        ]
        .into_iter()
        .collect();
        let konfig = KonfigKontext {
            typical_vref_kt: Some(110.0),
            flaps_bewegt: Some(true),
            flaps_max_im_flug: Some(0.5),
        };
        let out = compute_approach_stability_v2(&buf, None, None, None, None, konfig);
        assert_eq!(
            out.stable_config,
            Some(false),
            "Vref+59 mit halben Klappen → durchgefallen"
        );
    }

    #[test]
    fn landekonfiguration_bleibt_streng_ohne_vref() {
        // Dasselbe Bild wie beim fremden Raster, nur ohne Vref in der
        // Tabelle. Ohne Geschwindigkeits-Anker laesst sich ein fremdes
        // Raster nicht von einer Unter-Klappung unterscheiden — dann bleibt
        // es beim bisherigen, strengen Verhalten. Der Health-Report meldet
        // das Muster separat, damit die Tabelle waechst.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 105.0, 108.0, -600.0, 1.0, 0.5),
            approach_sample(600.0, 103.0, 106.0, -580.0, 1.0, 0.5),
            approach_sample(300.0, 100.0, 104.0, -560.0, 1.0, 0.5),
        ]
        .into_iter()
        .collect();
        let konfig = KonfigKontext {
            typical_vref_kt: None,
            flaps_bewegt: Some(true),
            flaps_max_im_flug: Some(0.5),
        };
        let out = compute_approach_stability_v2(&buf, None, None, None, None, konfig);
        assert_eq!(
            out.stable_config,
            Some(false),
            "kein Vref → Schwelle bleibt absolut"
        );
    }

    #[test]
    fn klappen_stillstand_zaehlt_nur_bei_lueckenloser_beobachtung() {
        // Normaler Flug: die Spanne laeuft ab dem Gate, also vor dem
        // Abheben. Ruehrt sich der Wert danach nie, ist der Kanal tot.
        let mut vom_gate = FlightStats::default();
        merke_klappen_spanne(&mut vom_gate, 0.0);
        merke_klappen_spanne(&mut vom_gate, 0.0);
        assert_eq!(
            konfig_kontext(&vom_gate, &aircraft_limits_for("A320")).flaps_bewegt,
            Some(false),
            "von Anfang an beobachtet → Stillstand heisst toter Kanal"
        );

        // Resume: die App startet mitten im Anflug neu, `takeoff_at` kommt
        // aus dem gespeicherten Zustand. Die Klappen stehen schon auf FULL
        // und bewegen sich bis zum Aufsetzen nicht mehr — das darf NICHT als
        // toter Kanal gelten, sonst verliert eine korrekt geflogene
        // Landekonfiguration ihr Urteil.
        let mut nach_resume = FlightStats {
            takeoff_at: Some(Utc::now()),
            ..Default::default()
        };
        merke_klappen_spanne(&mut nach_resume, 1.0);
        merke_klappen_spanne(&mut nach_resume, 1.0);
        assert_eq!(
            konfig_kontext(&nach_resume, &aircraft_limits_for("A320")).flaps_bewegt,
            None,
            "erst ab Anflug beobachtet → Frage bleibt offen"
        );

        // Und die Landekonfiguration selbst: offene Frage → alte, strenge
        // Bewertung, kein stilles „nicht bewertbar".
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 130.0, 132.0, -650.0, 1.0, 1.0),
            approach_sample(600.0, 128.0, 130.0, -620.0, 1.0, 1.0),
            approach_sample(300.0, 125.0, 128.0, -600.0, 1.0, 1.0),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(
            &buf,
            None,
            None,
            None,
            None,
            konfig_kontext(&nach_resume, &aircraft_limits_for("A320")),
        );
        assert_eq!(
            out.stable_config,
            Some(true),
            "volle Klappen bleiben bestanden"
        );
    }

    #[test]
    fn klappen_spanne_ignoriert_kaputte_messwerte() {
        let mut stats = FlightStats::default();
        merke_klappen_spanne(&mut stats, f32::NAN);
        assert_eq!(
            stats.flaps_max_gesehen, None,
            "NaN darf die Spanne nicht oeffnen"
        );
        merke_klappen_spanne(&mut stats, 0.0);
        merke_klappen_spanne(&mut stats, 1.5);
        merke_klappen_spanne(&mut stats, -0.3);
        assert_eq!(stats.flaps_min_gesehen, Some(0.0));
        assert_eq!(stats.flaps_max_gesehen, Some(1.0), "auf 0..1 geklemmt");
    }

    #[test]
    fn landing_config_scale_artifact_is_not_assessable() {
        // Befund ITY 4TK (Synaptic A220, 12.08.2026): Klappenhebel doppelt
        // normalisiert → flaps_position 0.2 statt 1.0 den ganzen Anflug.
        // Fahrwerk unten, 130 kt — das ist ein Messfehler, kein Pilot, der
        // mit Klappen 20 % landet. Muss None (nicht bewertbar) ergeben,
        // nie wieder ein falsches „INCOMPLETE".
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 130.0, 132.0, -650.0, 1.0, 0.2),
            approach_sample(600.0, 128.0, 130.0, -620.0, 1.0, 0.2),
            approach_sample(300.0, 125.0, 128.0, -600.0, 1.0, 0.2),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.stable_config, None, "Skalen-Artefakt → nicht bewertbar");
    }

    /// v0.19.x FIX: fewer than 3 gate samples (a telemetry stall right at
    /// the gate) must leave `excessive_sink` at `None` ("not measured"),
    /// not the struct's old bool-default of `false` ("measured, clean").
    /// Every OTHER field here already correctly defaults to `None`/absent
    /// on this early-return path — this closes the one field that didn't,
    /// which let the frontend's stable-approach pill show a confident
    /// green "STABLE" built from zero real measurements.
    #[test]
    fn excessive_sink_is_none_not_false_when_gate_has_too_few_samples() {
        let buf: std::collections::VecDeque<ApproachBufferSample> =
            [approach_sample(900.0, 130.0, 132.0, -650.0, 1.0, 1.0)]
                .into_iter()
                .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.window_sample_count, 1, "only 1 sample in the gate");
        assert_eq!(
            out.excessive_sink, None,
            "1 sample is not enough evidence to confirm EITHER way — must be None, not Some(false)"
        );
        // Every other measured field must also report "not measured", the
        // established contract this field was the one exception to.
        assert_eq!(out.stable_config, None);
        assert_eq!(out.stable_at_gate, None);
        assert_eq!(out.vs_deviation_fpm, None);
    }

    #[test]
    fn excessive_sink_is_some_once_the_gate_has_enough_samples() {
        // Regression guard: the fix must not turn a genuine 3-sample
        // measurement into "not measured".
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 130.0, 132.0, -650.0, 1.0, 1.0),
            approach_sample(600.0, 128.0, 130.0, -620.0, 1.0, 1.0),
            approach_sample(300.0, 125.0, 128.0, -600.0, 1.0, 1.0),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(
            out.excessive_sink,
            Some(false),
            "3 calm samples is a real measurement confirming no excess sink"
        );
    }

    /// v1.9.16: Ein Ausreisser unter −1000 fpm ist kein „excessive sink" —
    /// erst 10 s am Stueck.
    #[test]
    fn sinkgrenze_toleriert_kurzes_ueberschreiten() {
        let reihe = |vs: &[f32]| -> std::collections::VecDeque<ApproachBufferSample> {
            vs.iter()
                .enumerate()
                .map(|(i, v)| approach_sample(900.0 - 50.0 * i as f32, 130.0, 132.0, *v, 1.0, 1.0))
                .collect()
        };
        // 3 Samples (4 s) unter der Grenze: toleriert.
        let kurz = reihe(&[-650.0, -1100.0, -1050.0, -1080.0, -700.0, -650.0]);
        let out = compute_approach_stability_v2(&kurz, None, None, None, None, Default::default());
        assert_eq!(out.excessive_sink, Some(false), "4 s unter der Grenze");

        // 6 Samples (10 s) am Stueck: excessive.
        let lang = reihe(&[
            -650.0, -1100.0, -1150.0, -1200.0, -1100.0, -1120.0, -1100.0, -700.0,
        ]);
        let out = compute_approach_stability_v2(&lang, None, None, None, None, Default::default());
        assert_eq!(
            out.excessive_sink,
            Some(true),
            "10 s am Stueck unter der Grenze"
        );
    }

    /// Sim-Pause: zwei Samples unter der Grenze mit grosser Luecke
    /// dazwischen sind keine 10 s Sinkflug.
    #[test]
    fn sinkgrenze_ueberbrueckt_keine_buffer_luecke() {
        let base = DateTime::<Utc>::from_timestamp(1_790_100_000, 0).unwrap();
        let mk = |sek: i64, vs: f32| {
            let mut s = approach_sample(800.0, 130.0, 132.0, vs, 1.0, 1.0);
            s.at = base + chrono::Duration::seconds(sek);
            s
        };
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            mk(0, -1100.0),
            mk(1, -1100.0),
            mk(15, -1100.0),
            mk(16, -1100.0),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.excessive_sink, Some(false), "Luecke von 14 s");
        // Gegenprobe: lueckenlos 11 s unter der Grenze zaehlt.
        let dicht: std::collections::VecDeque<ApproachBufferSample> =
            (0..12).map(|i| mk(i, -1100.0)).collect();
        let out = compute_approach_stability_v2(&dicht, None, None, None, None, Default::default());
        assert_eq!(out.excessive_sink, Some(true));
    }

    #[test]
    fn sinkgrenze_zaehlt_nur_am_stueck() {
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 130.0, 132.0, -1100.0, 1.0, 1.0),
            approach_sample(800.0, 130.0, 132.0, -1100.0, 1.0, 1.0),
            approach_sample(700.0, 130.0, 132.0, -700.0, 1.0, 1.0),
            approach_sample(600.0, 130.0, 132.0, -1100.0, 1.0, 1.0),
            approach_sample(500.0, 130.0, 132.0, -1100.0, 1.0, 1.0),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.excessive_sink, Some(false));
    }

    /// Score-Version 24: Die Sinkgrenze waechst ab 145 kt ueber Grund mit —
    /// nur lockerer, nie strenger.
    #[test]
    fn sinkgrenze_waechst_mit_der_grundgeschwindigkeit() {
        let reihe = |gs: f32, vs: f32| -> std::collections::VecDeque<ApproachBufferSample> {
            (0..8)
                .map(|i| approach_sample(900.0 - 50.0 * i as f32, gs, 140.0, vs, 1.0, 1.0))
                .collect()
        };
        // 180 kt ueber Grund → Grenze −1241 fpm: 14 s bei −1100 sind erlaubt
        // (vor Version 24: excessive).
        let schnell = reihe(180.0, -1100.0);
        let out =
            compute_approach_stability_v2(&schnell, None, None, None, None, Default::default());
        assert_eq!(
            out.excessive_sink,
            Some(false),
            "Rueckenwind: −1100 bei 180 kt GS"
        );
        let w = out.wind.expect("Gate gemessen");
        assert!(
            (w.sinkgrenze_fpm.unwrap() + 1241.4).abs() < 1.0,
            "{:?}",
            w.sinkgrenze_fpm
        );
        assert!(w.sinklauf.is_none(), "kein Punkt unter der Grenze");
        // Dasselbe unter der Grenze bleibt excessive.
        let out = compute_approach_stability_v2(
            &reihe(180.0, -1300.0),
            None,
            None,
            None,
            None,
            Default::default(),
        );
        assert_eq!(out.excessive_sink, Some(true));
        // 120 kt ueber Grund: die Grenze bleibt −1000, nicht −828.
        let out = compute_approach_stability_v2(
            &reihe(120.0, -900.0),
            None,
            None,
            None,
            None,
            Default::default(),
        );
        assert_eq!(out.excessive_sink, Some(false), "nie strenger als −1000");
        let out = compute_approach_stability_v2(
            &reihe(120.0, -1100.0),
            None,
            None,
            None,
            None,
            Default::default(),
        );
        assert_eq!(
            out.excessive_sink,
            Some(true),
            "−1100 bei 120 kt bleibt zu stark"
        );
        // Steiler Gleitpfad: Faktor und Grundgeschwindigkeit multiplizieren sich.
        assert!((sinkgrenze_fpm(180.0, 2.0) + 2482.8).abs() < 1.0);
        assert_eq!(
            sinkgrenze_fpm(f32::NAN, 1.0),
            -1000.0,
            "unlesbare GS → feste Grenze"
        );
    }

    /// Score-Version 24: Wind, Soll-Sinkrate und laengster Sinklauf aus dem
    /// Gate-Fenster; Messpunkte ohne Wind fehlen, statt als 0 zu zaehlen.
    #[test]
    fn wind_im_anflug_und_sinklauf_aus_dem_gate() {
        let mut buf: std::collections::VecDeque<ApproachBufferSample> = [
            (900.0, -700.0, Some(10.0), Some(-4.0)),
            (850.0, -1200.0, Some(14.0), Some(-6.0)),
            (800.0, -1300.0, None, None),
            (750.0, -1100.0, Some(18.0), Some(-8.0)),
            (700.0, -700.0, Some(14.0), Some(2.0)),
        ]
        .into_iter()
        .map(|(h, vs, hw, xw)| {
            let mut s = approach_sample(h, 140.0, 145.0, vs, 1.0, 1.0);
            s.headwind_kt = hw;
            s.crosswind_kt = xw;
            s
        })
        .collect();
        let w = compute_approach_stability_v2(&buf, None, None, None, None, Default::default())
            .wind
            .expect("Gate gemessen");
        assert_eq!(w.wind_proben, 4);
        assert_eq!(w.gegenwind_mittel_kt, Some(14.0));
        assert_eq!(w.seitenwind_max_kt, Some(8.0));
        assert_eq!(w.seitenwind_mittel_kt, Some(-4.0), "meist von links");
        assert_eq!(w.gs_mittel_kt, Some(140.0));
        assert!((w.soll_sinkrate_fpm.unwrap() + 743.4).abs() < 0.5);
        assert_eq!(w.sinkgrenze_fpm, Some(-1000.0));
        let lauf = w.sinklauf.expect("drei Punkte unter −1000");
        assert_eq!(lauf.dauer_s, 4.0, "Helfer: 2 s je Punkt");
        assert_eq!((lauf.von_ft, lauf.bis_ft), (850.0, 750.0));
        assert_eq!(lauf.spitze_fpm, -1300.0);
        // Ganz ohne Windwerte: kein Wind, aber Soll und Grenze bleiben.
        for s in buf.iter_mut() {
            s.headwind_kt = None;
            s.crosswind_kt = None;
        }
        let w = compute_approach_stability_v2(&buf, None, None, None, None, Default::default())
            .wind
            .unwrap();
        assert_eq!(w.wind_proben, 0);
        assert_eq!(w.gegenwind_mittel_kt, None);
        assert!(w.soll_sinkrate_fpm.is_some());
    }

    #[test]
    fn landing_config_ok_with_flaps_extended() {
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 130.0, 132.0, -650.0, 1.0, 0.85),
            approach_sample(600.0, 128.0, 130.0, -620.0, 1.0, 0.85),
            approach_sample(300.0, 125.0, 128.0, -600.0, 1.0, 0.85),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(out.stable_config, Some(true));
    }

    #[test]
    fn max_vs_dev_excludes_flare_balloon_below_50ft() {
        // GSG225: a +200 fpm flare balloon at 9 ft AGL must NOT show up
        // as an approach-stability deviation; a real -300 fpm excursion
        // at 200 ft must. Without the height floor the 9 ft sample
        // produced a spurious ~840 fpm "deviation".
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(800.0, 120.0, 130.0, -640.0, 1.0, 0.85),
            approach_sample(200.0, 120.0, 128.0, -300.0, 1.0, 0.85),
            approach_sample(9.0, 120.0, 120.0, 200.0, 1.0, 0.85),
        ]
        .into_iter()
        .collect();
        let out = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        let max_dev = out
            .max_vs_deviation_below_500_fpm
            .expect("a sub-500 ft sample exists");
        // target at 120 kt ≈ -637 fpm; 200 ft sample dev ≈ 337,
        // 9 ft balloon dev ≈ 837 — the balloon must be excluded.
        assert!(max_dev < 500.0, "flare balloon excluded, got {max_dev}");
        assert!(max_dev > 200.0, "real 200 ft excursion kept, got {max_dev}");
    }

    #[test]
    fn steep_glideslope_v01517_not_flagged_excessive() {
        // EGLC 5,5° (Live-Report Michael, London City): ein KORREKT geflogener
        // Steilanflug bei ~120 kt hat ~−1170 fpm. Gegen den fixen 3°-Standard
        // wäre das fälschlich „excessive" + große Abweichung; mit dem echten
        // Gleitwinkel ist es sauber. Der Score ist nicht betroffen (sub_stability
        // nutzt σ, nicht diese Felder) — reine Anzeige/„stable"-Korrektur.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(900.0, 120.0, 132.0, -1170.0, 1.0, 0.85),
            approach_sample(800.0, 120.0, 132.0, -1165.0, 1.0, 0.85),
            approach_sample(700.0, 119.0, 131.0, -1160.0, 1.0, 0.85),
            approach_sample(650.0, 119.0, 131.0, -1155.0, 1.0, 0.85),
            approach_sample(600.0, 118.0, 130.0, -1150.0, 1.0, 0.85),
            approach_sample(300.0, 115.0, 128.0, -1120.0, 1.0, 0.85),
        ]
        .into_iter()
        .collect();

        // Ohne Gleitwinkel (3°-Fallback): fälschlich „excessive".
        let out_3deg =
            compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(
            out_3deg.excessive_sink,
            Some(true),
            "gegen 3° gilt der Steilanflug als excessive"
        );

        // Mit echtem 5,5°-Gleitwinkel: NICHT excessive + kleine V/S-Abweichung.
        let out_55 =
            compute_approach_stability_v2(&buf, None, None, Some(5.5), None, Default::default());
        assert_eq!(
            out_55.excessive_sink,
            Some(false),
            "5,5°-Steilanflug ist nicht excessive"
        );
        let dev = out_55.vs_deviation_fpm.expect("deviation vorhanden");
        assert!(dev < 200.0, "Abweichung vs 5,5°-Profil klein, war {dev}");

        // AUDIT-Invariante 1: None == exakt 3,0° (bit-identisch, keine Regression).
        let out_explicit_3 =
            compute_approach_stability_v2(&buf, None, None, Some(3.0), None, Default::default());
        assert_eq!(
            out_3deg.excessive_sink, out_explicit_3.excessive_sink,
            "None und 3,0° müssen identisch sein"
        );
        assert_eq!(out_3deg.vs_deviation_fpm, out_explicit_3.vs_deviation_fpm);

        // AUDIT-Invariante 2: unplausibler Gleitwinkel (außerhalb 2–7,5°) →
        // Fallback 3° (= None-Verhalten), kein Müll-Scaling.
        let out_garbage =
            compute_approach_stability_v2(&buf, None, None, Some(15.0), None, Default::default());
        assert_eq!(
            out_3deg.excessive_sink, out_garbage.excessive_sink,
            "Müll-Gleitwinkel → 3°-Fallback"
        );
    }

    #[test]
    fn steep_glideslope_v01518_da_gate_scaled() {
        // Regression für den DA-Gate-Bug, den der v0.15.17-London-Fix offen
        // ließ: das 200-ft-DA-Gate (stable_at_da) prüfte excessive-sink fix
        // gegen −1000 fpm. Auf einem korrekt geflogenen Steilanflug liegt die
        // Soll-Sinkrate dort schon über −1000 fpm → stable_at_da war IMMER
        // false. Samples ≤200 ft AGL, ruhig (kleiner Jerk), nur die Sinkrate
        // ist steilanflug-typisch hoch.
        let buf: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(180.0, 120.0, 132.0, -1150.0, 1.0, 0.85),
            approach_sample(170.0, 120.0, 132.0, -1148.0, 1.0, 0.85),
            approach_sample(160.0, 120.0, 132.0, -1146.0, 1.0, 0.85),
            approach_sample(150.0, 120.0, 132.0, -1145.0, 1.0, 0.85),
            approach_sample(120.0, 119.0, 131.0, -1140.0, 1.0, 0.85),
            approach_sample(60.0, 118.0, 130.0, -1120.0, 1.0, 0.85),
        ]
        .into_iter()
        .collect();

        // 3°-Fallback (None): DA-Gate gegen fix −1000 → fälschlich instabil.
        let out_3 = compute_approach_stability_v2(&buf, None, None, None, None, Default::default());
        assert_eq!(
            out_3.stable_at_da,
            Some(false),
            "gegen 3° ist der Steilanflug am DA-Gate fälschlich instabil"
        );

        // 5,5°-Gleitwinkel: Schwelle ~−1834 fpm → die −1120…−1150 sind sauber.
        let out_55 =
            compute_approach_stability_v2(&buf, None, None, Some(5.5), None, Default::default());
        assert_eq!(
            out_55.stable_at_da,
            Some(true),
            "mit echtem 5,5°-Gleitwinkel ist der DA-Gate-Anflug stabil"
        );

        // AUDIT-Invariante: bei exakt 3,0° identisch zum None-Fallback.
        let out_3_explicit =
            compute_approach_stability_v2(&buf, None, None, Some(3.0), None, Default::default());
        assert_eq!(out_3.stable_at_da, out_3_explicit.stable_at_da);
    }

    /// 2.1.6 — SWR 255 LFSB–EBBR (X-Plane A20N, 20.10.2026): unter 200 ft
    /// 116 → 104 kt bei Vref 135 und −970…−1110 fpm, kein Abfangen, −1180 fpm
    /// am Boden — und `stable_at_da` stand auf true, weil das Tor nur die
    /// Streuung pruefte (ruhig, aber falsch) und der Sinklauf 10 s brauchte.
    /// Die Samples sind das Flugprotokoll (2 s Raster, gleiche Werte).
    #[test]
    fn da_gate_swr255_zu_langsam_und_zu_steil_ist_nicht_stabil() {
        let swr255 = || -> std::collections::VecDeque<ApproachBufferSample> {
            [
                approach_sample(190.0, 119.0, 116.0, -1093.0, 1.0, 1.0),
                approach_sample(176.0, 118.0, 115.0, -1082.0, 1.0, 1.0),
                approach_sample(163.0, 118.0, 114.0, -1062.0, 1.0, 1.0),
                approach_sample(151.0, 117.0, 113.0, -1030.0, 1.0, 1.0),
                approach_sample(139.0, 117.0, 112.0, -1003.0, 1.0, 1.0),
                approach_sample(128.0, 116.0, 111.0, -993.0, 1.0, 1.0),
                approach_sample(115.0, 115.0, 110.0, -985.0, 1.0, 1.0),
                approach_sample(103.0, 115.0, 109.0, -974.0, 1.0, 1.0),
                approach_sample(90.0, 114.0, 108.0, -969.0, 1.0, 1.0),
                approach_sample(75.0, 113.0, 107.0, -996.0, 1.0, 1.0),
                approach_sample(57.0, 113.0, 106.0, -1054.0, 1.0, 1.0),
                approach_sample(39.0, 112.0, 105.0, -1092.0, 1.0, 1.0),
                approach_sample(19.0, 112.0, 104.0, -1108.0, 1.0, 1.0),
            ]
            .into_iter()
            .collect()
        };
        let a20n = KonfigKontext { typical_vref_kt: Some(137.0), ..Default::default() };
        let out = compute_approach_stability_v2(&swr255(), None, None, None, None, a20n);
        assert_eq!(out.stable_at_da, Some(false), "32 kt unter Vref und 8 s unter der Sinkgrenze: nicht stabil");
        // Ohne bekannte Vref traegt der kurze Sinklauf allein.
        let out_ohne_vref = compute_approach_stability_v2(&swr255(), None, None, None, None, Default::default());
        assert_eq!(out_ohne_vref.stable_at_da, Some(false), "der 8-s-Sinklauf unter 200 ft genuegt");

        // Gegenprobe: dieselbe Ruhe mit 3°-Sinkrate und Fahrt um Vref ist stabil.
        let ruhig: std::collections::VecDeque<ApproachBufferSample> = (0..13)
            .map(|i| approach_sample(190.0 - 14.0 * i as f32, 135.0, 137.0 - 0.3 * i as f32, -720.0 + 5.0 * (i % 3) as f32, 1.0, 1.0))
            .collect();
        let a20n = KonfigKontext { typical_vref_kt: Some(137.0), ..Default::default() };
        assert_eq!(compute_approach_stability_v2(&ruhig, None, None, None, None, a20n).stable_at_da, Some(true));
        // Nur 3 s unter der Grenze (ein Rempler) bleibt stabil; 12 kt unter Vref auch.
        let rempler: std::collections::VecDeque<ApproachBufferSample> = (0..13)
            .map(|i| approach_sample(190.0 - 14.0 * i as f32, 130.0, 126.0, if i == 6 { -1050.0 } else { -720.0 }, 1.0, 1.0))
            .collect();
        let a20n = KonfigKontext { typical_vref_kt: Some(137.0), ..Default::default() };
        assert_eq!(compute_approach_stability_v2(&rempler, None, None, None, None, a20n).stable_at_da, Some(true));

        // Codex-Review: Das Abfangen (letzte 3 s vor dem Aufsetzen) zaehlt am
        // 200-ft-Tor nicht mit — sonst laese die Vref-Pruefung die planmaessig
        // fallende Fahrt als „zu langsam“. Mit Aufsetz-Zeitpunkt faellt das
        // letzte Sample (2 s davor) weg; ohne kippt es das Tor.
        let abfangen: std::collections::VecDeque<ApproachBufferSample> = [
            approach_sample(150.0, 135.0, 136.0, -720.0, 1.0, 1.0),
            approach_sample(110.0, 135.0, 136.0, -715.0, 1.0, 1.0),
            approach_sample(70.0, 135.0, 136.0, -720.0, 1.0, 1.0),
            approach_sample(20.0, 100.0, 60.0, -200.0, 1.0, 1.0),
        ]
        .into_iter()
        .collect();
        let td = abfangen.back().unwrap().at + chrono::Duration::seconds(2);
        let a20n = KonfigKontext { typical_vref_kt: Some(137.0), ..Default::default() };
        assert_eq!(compute_approach_stability_v2(&abfangen, None, Some(td), None, None, a20n).stable_at_da, Some(true), "Abfangen ausgeschnitten");
        assert_eq!(compute_approach_stability_v2(&abfangen, None, None, None, None, a20n).stable_at_da, Some(false), "ohne Schnitt kippt das Sample");
    }

    #[test]
    fn takeoff_climb_gate_is_low_for_rotorcraft() {
        use crate::aircraft_category::AircraftCategory;
        // Fixed-wing keeps the 500 ft gate; a rotorcraft uses a low gate so a
        // low-level heli leaves Takeoff into a capturable flight phase.
        assert_eq!(takeoff_climb_gate_ft(AircraftCategory::FixedWing), 500.0);
        assert_eq!(takeoff_climb_gate_ft(AircraftCategory::Helicopter), 60.0);
        // Seaplane / amphibian are not rotorcraft → fixed-wing gate unchanged.
        assert_eq!(takeoff_climb_gate_ft(AircraftCategory::Seaplane), 500.0);
        assert_eq!(takeoff_climb_gate_ft(AircraftCategory::Amphibian), 500.0);
    }

    #[test]
    fn touchdown_capturable_in_takeoff_only_when_airborne() {
        // Flight phases are always capturable (airborne flag irrelevant).
        assert!(is_touchdown_capturable_phase(FlightPhase::Climb, false));
        assert!(is_touchdown_capturable_phase(FlightPhase::Final, false));
        assert!(is_touchdown_capturable_phase(FlightPhase::Landing, false));
        // Takeoff: capturable ONLY once the aircraft has genuinely flown — the
        // short/low bush hop that lands before reaching Climb (GSG309). Before
        // airborne (initial takeoff roll) it is NOT capturable.
        assert!(is_touchdown_capturable_phase(FlightPhase::Takeoff, true));
        assert!(!is_touchdown_capturable_phase(FlightPhase::Takeoff, false));
        // Ground / taxi phases are NEVER capturable (taxi gear-bump phantom
        // protection) — even with was_airborne set (post-landing rollout/taxi).
        assert!(!is_touchdown_capturable_phase(FlightPhase::TaxiOut, true));
        assert!(!is_touchdown_capturable_phase(FlightPhase::TaxiIn, true));
        assert!(!is_touchdown_capturable_phase(FlightPhase::Boarding, true));
        assert!(!is_touchdown_capturable_phase(FlightPhase::Arrived, true));
        assert!(!is_touchdown_capturable_phase(
            FlightPhase::TakeoffRoll,
            true
        ));
    }

    #[test]
    fn sampler_in_air_now_uses_water_surface_for_seaplane() {
        use crate::aircraft_category::AircraftCategory;
        // Seaplane on water (on_ground=false, no gear force): airborne above the
        // water-contact AGL, on the surface below it → the edge that captures a
        // water touchdown the wheeled signal would miss.
        assert!(sampler_in_air_now(
            AircraftCategory::Seaplane,
            false,
            20.0,
            None
        ));
        assert!(!sampler_in_air_now(
            AircraftCategory::Seaplane,
            false,
            1.0,
            None
        ));
        assert!(sampler_in_air_now(
            AircraftCategory::Amphibian,
            false,
            10.0,
            None
        ));
        // A seaplane ON A RUNWAY (on_ground=true) uses the wheeled signal, not
        // the water surface → on the ground = not in air.
        assert!(!sampler_in_air_now(
            AircraftCategory::Seaplane,
            true,
            1.0,
            None
        ));
        // Fixed-wing / helicopter are NOT water-capable → byte-identical to the
        // wheeled signal: low AGL over water does NOT make them "on the surface".
        assert!(sampler_in_air_now(
            AircraftCategory::FixedWing,
            false,
            1.0,
            None
        ));
        assert!(sampler_in_air_now(
            AircraftCategory::Helicopter,
            false,
            1.0,
            None
        ));
        // Wheeled gear-force branch unchanged: spike → on ground; unloaded → air.
        assert!(!sampler_in_air_now(
            AircraftCategory::FixedWing,
            false,
            50.0,
            Some(5000.0)
        ));
        assert!(sampler_in_air_now(
            AircraftCategory::FixedWing,
            false,
            50.0,
            Some(0.5)
        ));
    }

    #[test]
    fn runway_glideslope_lookup_v01519() {
        // v0.15.19: Live-Lookup des Approach-Gleitwinkels für das Stable-
        // Approach-Banner — Designator-Match (exakt + numerischer Fallback für
        // Parallelbahnen) + Plausibilitäts-Clamp (2–7,5°).
        let runways: Vec<aeroacars_mqtt::navdata::NavRunway> = serde_json::from_str(
            r#"[
              {"designator":"09","magnetic_course":90.0,"true_course":90.0,"length_ft":4900,
               "threshold":{"lat":51.50,"lon":0.05},"end":{"lat":51.50,"lon":0.10},"glideslope_angle":3.0},
              {"designator":"27","magnetic_course":270.0,"true_course":270.0,"length_ft":4900,
               "threshold":{"lat":51.50,"lon":0.10},"end":{"lat":51.50,"lon":0.05},"glideslope_angle":5.5},
              {"designator":"13L","magnetic_course":130.0,"true_course":130.0,"length_ft":9000,
               "threshold":{"lat":40.0,"lon":-74.0},"end":{"lat":40.1,"lon":-74.1},"glideslope_angle":1.0}
            ]"#,
        )
        .expect("parse runways");

        // Exakter Designator-Match → echter Steilanflug-Winkel (EGLC-artig)
        assert_eq!(runway_glideslope_for(&runways, "27"), Some(5.5));
        // 3°-Bahn → Some(3.0) (im Band; Banner skaliert dann ×1,0 = unverändert)
        assert_eq!(runway_glideslope_for(&runways, "09"), Some(3.0));
        // L/R-Suffix → numerischer Fallback (Parallelbahnen teilen den Gleitweg)
        assert_eq!(runway_glideslope_for(&runways, "27R"), Some(5.5));
        // "RW"-Präfix wird gestrippt (manche Sims liefern "RW27")
        assert_eq!(runway_glideslope_for(&runways, "RW27"), Some(5.5));
        // Unplausibel (<2°) → None → Banner bleibt beim 3°-Default
        assert_eq!(runway_glideslope_for(&runways, "13L"), None);
        // Unbekannte Bahn / leer → None
        assert_eq!(runway_glideslope_for(&runways, "18"), None);
        assert_eq!(runway_glideslope_for(&runways, ""), None);
    }

    #[test]
    fn glideslope_lookup_no_longer_leaks_phantom_runway_after_fetch_time_dedupe() {
        // Code-Review-Nachtrag zum EFTP-Phantom-Bahn-Fix (siehe runway.rs
        // `dedupe_near_duplicate_nav_runways`): `runway_glideslope_for` liest
        // dieselbe `flight.navdata`-Bahnliste wie der Touchdown-Matcher, hat
        // aber NIE selbst dedupet. Der ursprüngliche Fix lief nur in
        // `lookup_runway_in_nav` und hätte diesen Konsumenten (live im
        // Anflug-Banner, siehe `resolve_approach_glideslope_deg`) weiterhin
        // dem Phantom "24C" ausgesetzt gelassen. Nachhaltig gelöst durch
        // Dedupe EINMAL an der Cache-Grenze (`spawn_navdata_fetch`), nicht
        // durch einen zweiten Dedupe-Aufruf hier — dieser Test beweist, dass
        // genau diese eine Reinigung ausreicht.
        let raw: Vec<aeroacars_mqtt::navdata::NavRunway> = serde_json::from_str(
            r#"[
              {"designator":"24","magnetic_course":244.4,"true_course":244.445012365508,"length_ft":8858,
               "threshold":{"lat":61.419369,"lon":23.627208},"end":{"lat":61.408922,"lon":23.581586},"glideslope_angle":3.0},
              {"designator":"24C","magnetic_course":244.4,"true_course":244.439196313664,"length_ft":7871,
               "threshold":{"lat":61.418208,"lon":23.622125},"end":{"lat":61.410561,"lon":23.588731},"glideslope_angle":4.2}
            ]"#,
        )
        .expect("parse runways");

        // Sanity: undedupliziert enthält die Liste den Phantom-Gleitwinkel.
        assert_eq!(
            runway_glideslope_for(&raw, "24C"),
            Some(4.2),
            "sanity: undeduped list still contains the phantom's glideslope"
        );

        // Dieselbe Reinigung, die `spawn_navdata_fetch` vor dem Cache-Insert
        // anwendet — nicht ein zweiter, glideslope-spezifischer Dedupe-Pfad.
        let cleaned = runway::dedupe_near_duplicate_nav_runways("EFTP", raw);
        assert_eq!(
            runway_glideslope_for(&cleaned, "24C"),
            Some(3.0),
            "'24C' is gone from the cleaned list, so the numeric-prefix fallback \
             now resolves to the real runway 24's glideslope instead of the phantom's"
        );
        assert_eq!(
            runway_glideslope_for(&cleaned, "24"),
            Some(3.0),
            "real runway 24 must survive with its own (correct) glideslope"
        );
    }

    #[test]
    fn is_flight_phase_v01520() {
        use FlightPhase::*;
        // FLUG-Phasen → Touchdown wird an allen drei Erfassungsstellen
        // akzeptiert (auch Climb/Cruise/Descent/Holding für die fehl-phasierte
        // FSM nach einem Go-Around — OWx9).
        for p in [Climb, Cruise, Descent, Holding, Approach, Final, Landing] {
            assert!(is_flight_phase(p), "{p:?} sollte Flug-Phase sein");
        }
        // BODEN-/Departure-Phasen → AUSGESCHLOSSEN. Das IST der Phantom-Schutz:
        // ein Gear-Force-Wackler beim Taxi/am Boden darf NIE als Touchdown
        // durchgehen. Diese Invariante sichert v0.5.45 ab.
        for p in [
            Boarding,
            Pushback,
            TaxiOut,
            TakeoffRoll,
            Takeoff,
            TaxiIn,
            BlocksOn,
            Arrived,
        ] {
            assert!(
                !is_flight_phase(p),
                "{p:?} darf KEINE Flug-Phase sein (Phantom-Schutz)"
            );
        }
    }

    // ---- v0.12.1 Stream B — pilot-status gate (LE6) ----

    fn profile_with_state(state: Option<i32>) -> api_client::Profile {
        api_client::Profile {
            id: 1,
            pilot_id: 1,
            ident: None,
            name: "Test Pilot".to_string(),
            email: None,
            airline_id: None,
            curr_airport: None,
            home_airport: None,
            airline: None,
            rank: None,
            callsign: None,
            state,
            simbrief_username: None,
        }
    }

    #[test]
    fn pilot_state_active_is_allowed() {
        assert_eq!(pilot_state_block_reason(&profile_with_state(Some(1))), None);
    }

    #[test]
    fn pilot_state_pending_blocks() {
        assert_eq!(
            pilot_state_block_reason(&profile_with_state(Some(0))),
            Some("pilot_pending"),
        );
    }

    #[test]
    fn pilot_state_rejected_blocks() {
        assert_eq!(
            pilot_state_block_reason(&profile_with_state(Some(2))),
            Some("pilot_rejected"),
        );
    }

    #[test]
    fn pilot_state_on_leave_blocks() {
        assert_eq!(
            pilot_state_block_reason(&profile_with_state(Some(3))),
            Some("pilot_on_leave"),
        );
    }

    #[test]
    fn pilot_state_suspended_blocks() {
        assert_eq!(
            pilot_state_block_reason(&profile_with_state(Some(4))),
            Some("pilot_suspended"),
        );
    }

    #[test]
    fn pilot_state_deleted_blocks() {
        // phpVMS UserState::DELETED = 5 — verified against the official
        // enum. A deleted account is a hard block (uses the suspended
        // message: "account blocked, contact the VA").
        assert_eq!(
            pilot_state_block_reason(&profile_with_state(Some(5))),
            Some("pilot_suspended"),
        );
    }

    #[test]
    fn pilot_state_missing_fails_closed() {
        // No `state` field on the profile → fail-closed, not allowed.
        assert_eq!(
            pilot_state_block_reason(&profile_with_state(None)),
            Some("pilot_state_unknown"),
        );
    }

    #[test]
    fn pilot_state_unexpected_value_fails_closed() {
        assert_eq!(
            pilot_state_block_reason(&profile_with_state(Some(99))),
            Some("pilot_state_unknown"),
        );
    }
