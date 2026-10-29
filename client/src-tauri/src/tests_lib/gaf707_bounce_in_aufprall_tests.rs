    use super::*;

    #[test]
    fn tg_dann_aufprall_wird_als_confirmed_impact_klassifiziert() {
        let mut stats = FlightStats {
            landing_simulator: Some("Msfs2024"),
            runway_match: Some(runway::RunwayMatch {
                airport_ident: "LIMJ".to_string(),
                runway_ident: "28".to_string(),
                heading_true_deg: 279.0,
                length_ft: 9075.0,
                width_ft: 197.0,
                surface: "ASP".to_string(),
                threshold_lat: 44.409_959,
                threshold_lon: 8.837_243,
                end_lat: 44.411_919,
                end_lon: 8.808_367,
                centerline_distance_m: 0.0,
                centerline_distance_abs_ft: 0.0,
                touchdown_distance_from_threshold_ft: 600.0,
                side: "left".to_string(),
                displaced_threshold_ft: 0,
                geometry_implied_displaced_threshold_ft: 0,
            }),
            ..Default::default()
        };

        // TD #1 — der Touch-and-Go. -1499 fpm liegt knapp UNTER der
        // Extreme-Impact-Schwelle (>=1500 fpm) — bewusst so, real gemessen.
        // Darf keinen Unfall ausloesen. Ablauf wie im echten Sampler-Code:
        // erst der Merge aus `peak_g_post_500ms` (VOR dem Klassifikator-
        // Aufruf, Codex-Folgefund fuenfte Runde), dann `apply_accident_
        // heuristic` — NICHT `landing_peak_g_force` manuell setzen, sonst
        // beweist der Test nur die Klassifikator-Logik, nicht die
        // tatsaechliche Verdrahtung im Sampler.
        let tg_analysis = serde_json::json!({
            "vs_at_edge_fpm": -1499.0,
            "peak_g_post_500ms": 4.67,
        });
        peak_g_force_verschmelzen(
            &mut stats,
            tg_analysis
                .get("peak_g_post_500ms")
                .and_then(|v| v.as_f64())
                .map(|x| x as f32),
        );
        apply_accident_heuristic(&mut stats, &tg_analysis);
        assert!(
            !stats.accident_detected,
            "der Touch-and-Go allein (-1499 fpm/4,67 G) liegt unter der \
             Extreme-Impact-Schwelle und darf keinen Unfall auslösen"
        );

        // Zwischen den Touchdowns: der Sampler-Klettern-Reset setzt
        // `landing_peak_g_force` auf `None` zurueck (siehe
        // `multi_td_klettern_reset_setzt_auch_landing_peak_g_force_zurueck`
        // weiter unten) — das MUSS hier ebenfalls passieren, sonst
        // reproduziert der Test nicht den echten Ausgangszustand fuer TD#2.
        stats.landing_peak_g_force = None;

        // TD #2 — der echte, finale Aufprall. `peak_g_post_500ms` steht in
        // der frisch berechneten Analyse fuer DIESEN Touchdown zur
        // Verfuegung — der fruehe Merge (Codex-Folgefund) sorgt dafuer,
        // dass der Klassifikator ihn sieht, statt mit dem `None` vom Reset
        // zu laufen.
        let final_analysis = serde_json::json!({
            "vs_at_edge_fpm": -3007.0,
            "peak_g_post_500ms": 5.88,
        });
        peak_g_force_verschmelzen(
            &mut stats,
            final_analysis
                .get("peak_g_post_500ms")
                .and_then(|v| v.as_f64())
                .map(|x| x as f32),
        );
        apply_accident_heuristic(&mut stats, &final_analysis);

        assert!(
            stats.accident_detected,
            "-3007 fpm/5,88 G mit Bahn-Match muss als Unfall erkannt werden, \
             auch wenn ihm ein (harmloser) Touch-and-Go vorausging"
        );
        assert_eq!(
            stats.accident_kind.as_deref(),
            Some(accident::AccidentKind::Impact.as_wire_str()),
            "Extreme-Impact-Pfad (|V/S|>=1500 UND G>=3.0) muss unabhängig \
             vom Bahn-Match als Impact klassifiziert werden"
        );
    }

    /// Codex-Folgefund (adversarial, 05.09.2026, gegen den Fruehdump-Fix
    /// oben): `landing_peak_g_force` ist ein RUNNING-MAX ueber die gesamte
    /// Session (`if g > cur { s.landing_peak_g_force = Some(g) }`, im
    /// Sampler-Dump-Handler), kein bedingungsloses Ueberschreiben wie
    /// Lat/Lon. Der Test daneben (`tg_dann_...`) beweist NUR, dass
    /// `apply_accident_heuristic` bei korrekt zurueckgesetzten Werten
    /// richtig klassifiziert — er ersetzt `landing_peak_g_force` manuell vor
    /// dem zweiten Aufruf und uebt damit NICHT den echten Reset-Pfad im
    /// Sampler aus. Dieser Test haelt stattdessen fest, WAS bei fehlendem
    /// Reset schiefgehen wuerde: ein harter, aber V/S-technisch harmloser
    /// erster Touchdown (hohes G, V/S unter der Schwelle) darf einen
    /// zweiten, eigentlich unauffaelligen Touchdown (normales G, aber V/S
    /// ueber der Schwelle) NICHT faelschlich als Confirmed(Impact)
    /// erscheinen lassen, nur weil das hohe G von TD#1 noch in
    /// `stats.landing_peak_g_force` steht. Ob der Sampler das G tatsaechlich
    /// zurueksetzt, prueft der Wiring-Test direkt danach.
    #[test]
    fn hohes_g_vom_ersten_touchdown_darf_den_zweiten_nicht_verfaelschen() {
        let mut stats = FlightStats {
            landing_simulator: Some("Msfs2024"),
            ..Default::default()
        };

        // TD #1 — harter Bump, hohes G, aber V/S klar unter der
        // Extreme-Impact-Schwelle. Legitim unauffaellig. Merge ueber
        // `peak_g_force_verschmelzen` wie im echten Sampler-Code, nicht
        // manuell gesetzt.
        let td1_analysis = serde_json::json!({
            "vs_at_edge_fpm": -900.0,
            "peak_g_post_500ms": 3.4,
        });
        peak_g_force_verschmelzen(
            &mut stats,
            td1_analysis
                .get("peak_g_post_500ms")
                .and_then(|v| v.as_f64())
                .map(|x| x as f32),
        );
        apply_accident_heuristic(&mut stats, &td1_analysis);
        assert!(!stats.accident_detected);

        // Zwischen den Touchdowns: der Sampler-Klettern-Reset MUSS
        // `landing_peak_g_force` auf None setzen (das ist die Zeile, die
        // der Wiring-Test unten im Quelltext verlangt).
        stats.landing_peak_g_force = None;

        // TD #2 — normale Landung mit unauffaelligem EIGENEN G, aber einer
        // V/S ueber der Extreme-Impact-Schwelle. Ohne den Reset waere hier
        // G=3.4 von TD#1 stehengeblieben und haette faelschlich
        // Confirmed(Impact) ausgeloest.
        let td2_analysis = serde_json::json!({
            "vs_at_edge_fpm": -1600.0,
            "peak_g_post_500ms": 1.2,
        });
        peak_g_force_verschmelzen(
            &mut stats,
            td2_analysis
                .get("peak_g_post_500ms")
                .and_then(|v| v.as_f64())
                .map(|x| x as f32),
        );
        apply_accident_heuristic(&mut stats, &td2_analysis);

        assert!(
            !stats.accident_detected,
            "TD#2 mit eigenem, unauffaelligem G (1.2) darf nicht als Unfall \
             gelten, nur weil TD#1 zufaellig ein hohes G hatte"
        );
    }

    /// Fuer den Fund oben: Das Abschliessen der Landung beim Wegsteigen muss
    /// `landing_peak_g_force` zuruecksetzen. Seit 20.10.2026 (Stück 3) liegt
    /// das in einer reinen Funktion, die der Sampler-Klettern-Block und die
    /// Phasenlogik beide rufen — damit ist es ein Ablauf-Test statt eines
    /// Quelltext-Wächters (dass beide sie rufen, bewacht
    /// `v163_mutationsluecken::beide_episoden_enden_raeumen_die_landung_ab`).
    #[test]
    fn multi_td_klettern_reset_setzt_auch_landing_peak_g_force_zurueck() {
        let mut stats = FlightStats::default();
        let td = Utc::now();
        stats.phase = FlightPhase::Landing;
        stats.landing_at = Some(td);
        stats.sampler_touchdown_at = Some(td);
        stats.landing_peak_g_force = Some(2.7);
        stats.sampler_touchdown_g_force = Some(2.7);
        let mut snap = SimSnapshot::default();
        snap.lat = 50.0;
        snap.lon = 8.0;
        snap.altitude_msl_ft = 600.0;
        snap.altitude_agl_ft = 300.0;
        landung_beim_wegsteigen_abschliessen(&mut stats, &snap, td + chrono::Duration::seconds(20));
        assert_eq!(
            stats.landing_peak_g_force, None,
            "ein hohes G vom ersten Touchdown wuerde in die Unfall-Klassifikation \
             des zweiten ueberleben (Codex-Folgefund 05.09.2026)"
        );
        assert_eq!(stats.sampler_touchdown_g_force, None);
        assert!((stats.touchdown_events[0].peak_g - 2.7).abs() < 1e-6, "vorher gesichert");
    }

    /// Codex-Folgefund (adversarial, 05.09.2026, fuenfte Runde): der Merge
    /// von `peak_g_post_500ms` nach `landing_peak_g_force` lief bisher NUR
    /// nach dem `apply_accident_heuristic`-Aufruf (nach `drop(stats)` +
    /// Re-Lock, ~180 Zeilen weiter unten) — der Klassifikator sah damit fuer
    /// GENAU DEN Touchdown, den er bewerten soll, entweder `None` (nach dem
    /// Klettern-Reset) oder einen veralteten Wert. Die Tests oben rufen
    /// `peak_g_force_verschmelzen` selbst auf und wuerden diese Verdrahtung
    /// NICHT fangen, wenn sie in der Produktion fehlte — deshalb hier ein
    /// Quelltext-Waechter direkt auf die Aufrufreihenfolge im Sampler.
    #[test]
    fn peak_g_merge_laeuft_im_sampler_vor_dem_klassifikator_nicht_danach() {
        const SRC: &str = crate::quelle_gesamt::PRODUKTION;
        let start = SRC
            .find("stats.landing_analysis = Some(analysis.clone());")
            .expect("Ankerstelle im Sampler nicht mehr gefunden — Test anpassen, nicht loeschen");
        let rest = &SRC[start..];
        let merge_pos = rest.find("peak_g_force_verschmelzen(&mut stats,");
        let klassifikator_pos = rest.find("apply_accident_heuristic(&mut stats, &analysis);");
        match (merge_pos, klassifikator_pos) {
            (Some(m), Some(k)) => assert!(
                m < k,
                "peak_g_force_verschmelzen muss im Sampler VOR \
                 apply_accident_heuristic laufen — sonst klassifiziert der \
                 Aufruf mit dem G-Stand von VOR diesem Touchdown, nicht mit \
                 dem gerade gemessenen (Codex-Folgefund 05.09.2026, fuenfte \
                 Runde)"
            ),
            _ => panic!(
                "peak_g_force_verschmelzen- oder apply_accident_heuristic-Aufruf \
                 im Sampler nicht mehr gefunden — Test anpassen, nicht loeschen"
            ),
        }
    }
