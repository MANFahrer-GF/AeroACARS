    use super::*;
    use serde_json::json;

    fn ms713_live_stats() -> FlightStats {
        // Echte Werte aus der VPS-DB fuer MS713 touchdown id 134:
        //   vs_at_edge_fpm: -194.057
        //   vs_smoothed_500ms_fpm: -204.7
        //   landing_rate_fpm: -204 (streamer-tick snapshot)
        //   landing_peak_vs_fpm: -204 (touchdown-window peak)
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-204.69882);
        stats.landing_peak_vs_fpm = Some(-204.69882);
        stats.landing_analysis = Some(json!({
            "vs_at_edge_fpm": -194.057,
            "vs_smoothed_500ms_fpm": -204.7,
        }));
        stats
    }

    #[test]
    fn ms713_canonical_returns_edge_value_not_streamer_tick() {
        // Der Bug-Anchor: vor v0.7.21 lieferten manche Stellen -204
        // (Streamer-Tick), andere -194 (Edge). canonical MUSS jetzt
        // ueberall -194 sein.
        let stats = ms713_live_stats();
        let v = stats.canonical_landing_rate_fpm().expect("some");
        assert!(
            (v - (-194.057)).abs() < 0.001,
            "expected Engineering-Edge -194.057, got {v}",
        );
    }

    #[test]
    fn ms713_round_to_int_matches_pilot_client_display() {
        // Das ist der Wert der bei phpVMS als "Landungsrate" landen
        // soll, und derselbe den der Pilot-Client Tab "Landung" zeigt.
        let stats = ms713_live_stats();
        let rounded = stats.canonical_landing_rate_fpm().map(|v| v.round() as i32);
        assert_eq!(rounded, Some(-194));
    }

    /// PIA3452 (FenixA320, MSFS, 13.07.2026): der Pilot sah DREI Zahlen fuer
    /// EINEN Touchdown — ACARS-Log -233 fpm (touchdown_v2 `vs_at_impact_frame`
    /// = Fenster-MINIMUM), Landing-Karte/Score/PIREP -206 fpm (`vs_at_edge_fpm`
    /// = Interpolation am on_ground-Frame), und die Einstufung klassifizierte
    /// auf -233 waehrend die Punkte aus -206 kamen. Seit v0.20.0 finalisiert
    /// der Buffer-Dump auf den Edge-Wert, und Log + Klasse lesen die Kanonik.
    fn pia3452_live_stats() -> FlightStats {
        let mut stats = FlightStats::default();
        // Was der v2-Cascade-Finalize frueher in BEIDE Rohfelder schrieb.
        stats.landing_rate_fpm = Some(-233.0);
        stats.landing_peak_vs_fpm = Some(-233.0);
        stats.landing_peak_g_force = Some(1.13);
        stats.landing_analysis = Some(json!({
            "vs_at_edge_fpm": -206.0,
            "vs_at_impact_frame_fpm": -233.0,
        }));
        stats
    }

    #[test]
    fn pia3452_log_score_and_pirep_all_read_the_same_vs() {
        // Die Log-Zeile ("Touchdown: V/S … fpm"), die Score-Basis und der
        // PIREP-Wert gehen seit v0.20.0 alle durch dieselbe Kanonik — es darf
        // KEINE Konstellation geben in der sie auseinanderlaufen.
        let stats = pia3452_live_stats();
        let canonical = stats.canonical_landing_rate_fpm().expect("some");
        assert!(
            (canonical - (-206.0)).abs() < 0.001,
            "Kanonik muss den 50-Hz-Edge liefern, nicht v2s Fenster-Minimum: got {canonical}",
        );
        // Score-Basis (Sinkrate-Punkte) == Log-Wert == PIREP-Wert.
        assert_eq!(score_basis_vs_fpm(&stats), Some(canonical));
        assert_eq!(canonical.round() as i32, -206);
    }

    /// QS-Fix (fallback_zero-Score-Bug, Joel EWG3552 2026-07-22): wenn KEINE
    /// echte Quelle je einen Touchdown-VS lieferte, schreibt der Streamer
    /// "fallback_zero" als landing_source. In der Praxis landet dabei IMMER
    /// exakt 0.0 fpm im Feld (derselbe `.unwrap_or(0.0)`, der auch die
    /// Source-Wahl trifft) — und 0.0 faellt schon durch die bestehende
    /// `landing_rate_is_plausible`-Pruefung (verlangt < 0.0), unabhaengig
    /// von diesem Fix. Dieser Test verriegelt trotzdem den INTENDIERTEN
    /// Vertrag explizit ("fallback_zero heisst: Zahl ignorieren, egal wie
    /// plausibel sie aussieht") mit einem Wert, der die alte Pruefung
    /// alleine bestehen WUERDE — falls sich der Fallback-Default je aendert
    /// (nicht mehr exakt 0.0), bleibt die Sperre trotzdem wirksam.
    #[test]
    fn fallback_zero_source_is_never_used_even_if_numerically_plausible() {
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-50.0);
        stats.landing_peak_vs_fpm = Some(-50.0);
        stats.landing_source = Some("fallback_zero".to_string());
        assert_eq!(stats.canonical_landing_rate_fpm(), None);
    }

    #[test]
    fn other_fallback_source_is_also_rejected() {
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-50.0);
        stats.landing_source = Some("other_fallback".to_string());
        assert_eq!(stats.canonical_landing_rate_fpm(), None);
    }

    /// Ein GENUINE Greaser-Touchdown (echte Quelle, kein Fallback) darf
    /// natuerlich weiterhin durchgehen — der Fix darf nur fallback_zero/
    /// other_fallback aussparen, sonst wuerde er auch echte, sehr sanfte
    /// Landungen faelschlich als "nicht gemessen" markieren. (Hinweis:
    /// exakt 0.0 fpm gilt schon vor diesem Fix nie als plausibel —
    /// `landing_rate_is_plausible` verlangt < 0.0 — daher hier -5.0.)
    #[test]
    fn genuine_soft_landing_still_counts() {
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-5.0);
        stats.landing_peak_vs_fpm = Some(-5.0);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-5.0));
    }

    /// `edge` kommt aus der unabhaengigen touchdown_v2-Pipeline — ein
    /// fallback_zero auf dem Legacy-Pfad darf einen ECHTEN Edge-Wert nicht
    /// mit-unterdruecken.
    #[test]
    fn fallback_zero_does_not_suppress_independent_edge_value() {
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(0.0);
        stats.landing_peak_vs_fpm = Some(0.0);
        stats.landing_source = Some("fallback_zero".to_string());
        stats.landing_analysis = Some(json!({ "vs_at_edge_fpm": -180.0 }));
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-180.0));
    }

    #[test]
    fn schlanke_liste_laesst_genau_die_schweren_felder_weg() {
        // Feldbefund 28.10.2026: landing_list ist schlank. Echte, vom
        // LandingStore geschriebene Landung (große Listen auf drei Einträge
        // gekürzt; touchdown_profile/approach_samples speichert der Client
        // nicht, dort je ein vollständiger Punkt ergänzt): genau
        // diese zehn Felder werden leer, JEDES andere bleibt unverändert —
        // sonst zeigt die Übersicht still einen falschen Wert (Codex-QS).
        const SCHWER: [&str; 10] = [
            "lateral_samples", "touchdown_profile", "approach_samples", "runway_exits", "sub_scores",
            "sprit", "runway_match", "sampler_diagnose", "gate_window", "anflug_gleitpfad",
        ];
        let r: LandingRecord =
            serde_json::from_str(include_str!("../../tests/fixtures/landung_voll_schlank_test.json")).expect("Landung");
        let vorher = serde_json::to_value(&r).unwrap();
        let nachher = serde_json::to_value(ohne_schwere_felder(r)).unwrap();
        let (vorher, nachher) = (vorher.as_object().unwrap(), nachher.as_object().unwrap());
        // Leere Felder lässt die Serialisierung teils ganz weg
        // (`skip_serializing_if`) — bei den schweren zählt das als geleert.
        for feld in nachher.keys() {
            assert!(vorher.contains_key(feld), "{feld} ist neu dazugekommen");
        }
        for (feld, wert) in vorher {
            let neu = nachher.get(feld);
            if SCHWER.contains(&feld.as_str()) {
                assert!(
                    !(wert.is_null() || wert.as_array().is_some_and(|a| a.is_empty())),
                    "{feld}: Vorlage leer, Test sagt nichts"
                );
                assert!(
                    neu.is_none_or(|v| v.is_null() || v.as_array().is_some_and(|a| a.is_empty())),
                    "{feld} nicht geleert"
                );
            } else {
                assert_eq!(neu, Some(wert), "{feld} darf sich nicht ändern");
            }
        }
    }

    /// Minimal-Datensatz fuer die Normalisierungs-Tests. Wie im
    /// storage-Crate ueber JSON gebaut — alles andere hat `serde(default)`,
    /// das erspart ~90 Felder von Hand.
    fn landing_record_fixture() -> LandingRecord {
        serde_json::from_value(json!({
            "pirep_id": "test-pirep",
            "touchdown_at": "2026-06-07T10:00:00Z",
            "recorded_at": "2026-06-07T10:05:00Z",
            "flight_number": "1224",
            "airline_icao": "GSG",
            "dpt_airport": "EYVI",
            "arr_airport": "ENTC",
            "score_numeric": 88,
            "score_label": "acceptable",
            "grade_letter": "A",
            "landing_rate_fpm": -180.0,
            "bounce_count": 0,
            "touchdown_profile": [],
            "approach_samples": [],
            "ux_version": 1,
            "forensics_version": 1,
            "sub_scores": [],
            "accident": false,
            "accident_reasons": [],
        }))
        .expect("test record")
    }

    /// Score-Version 21: RYR73 wurde mit Version 18 und Deckel 45 bewertet
    /// (Teilnoten wie im PIREP). Beim Lesen gilt der Abzug: 80 − 15 = 65.
    /// Version 17 und 21 bleiben unberuehrt, ebenso Landungen ohne Note.
    #[test]
    fn normalize_rechnet_deckel_fluege_mit_abzug_neu() {
        let teile = json!([
            {"key":"landing_rate","label_key":"l","points":80,"score":80,"value":"-268 fpm","rationale_key":"landing.rat.above_target","band":"good","skipped":false},
            {"key":"g_force","label_key":"l","points":80,"score":80,"value":"1.21 G","messwert":1.21,"rationale_key":"landing.rat.comfortable_g","band":"good","skipped":false},
            {"key":"bounces","label_key":"l","points":100,"score":100,"value":"0","rationale_key":"landing.rat.clean_set","band":"good","skipped":false},
            {"key":"stability","label_key":"l","points":45,"score":45,"value":"σ","warning":"anflug_unstable","rationale_key":"landing.rat.unstable_approach","band":"ok","skipped":false},
            {"key":"rollout","label_key":"l","points":100,"score":100,"value":"-","rationale_key":"landing.rat.centered","band":"good","skipped":false},
            {"key":"alignment","label_key":"l","points":85,"score":85,"value":"-","rationale_key":"landing.rat.off_centerline","band":"good","skipped":false},
            {"key":"touchdown_point","label_key":"l","points":85,"score":85,"value":"-","rationale_key":"landing.rat.in_tdz","band":"good","skipped":false}
        ]);
        let mit = |version: Option<u8>, note: Option<i32>| {
            let mut r = landing_record_fixture();
            r.sub_scores = serde_json::from_value(teile.clone()).expect("teilnoten");
            r.score_algorithm_version = version;
            r.score_numeric = note;
            r.score_deckel = Some("anflug_unstable_gesamt".into());
            r
        };
        let n = normalize_derived_scores(mit(Some(18), Some(45)));
        assert_eq!(n.score_numeric, Some(65));
        assert_eq!(n.score_deckel.as_deref(), Some("anflug_unstable_abzug"));
        assert_eq!(n.grade_letter.as_deref(), Some("C"));
        assert_eq!(n.score_algorithm_version, Some(18), "Version bleibt");

        for (v, note) in [(Some(17), Some(45)), (Some(21), Some(45)), (Some(18), None)] {
            let n = normalize_derived_scores(mit(v, note));
            assert_eq!(n.score_numeric, note, "{v:?}");
            assert_eq!(
                n.score_deckel.as_deref(),
                Some("anflug_unstable_gesamt"),
                "{v:?}"
            );
        }
    }

    /// QS 2026-08-04: `score_label`/`grade_letter` sind abgeleitete Werte,
    /// werden aber beim Einreichen eingefroren. Aendert sich die Regel
    /// spaeter (v0.20.0: Label-Schwellen auf die Noten-Schwellen
    /// ausgerichtet), tragen Alt-Datensaetze fuer immer das Ergebnis der
    /// alten Regel — real gemessen: 88 Punkte / Note A, gespeichert als
    /// "acceptable", waehrend die aktuelle Regel "smooth" sagt. Die
    /// Uebersicht rechnete frisch, der Bericht las das eingefrorene Feld:
    /// zwei verschiedene Woerter fuer dieselbe Landung. `landing_list`
    /// normalisiert deshalb beim Lesen — dieser Test nagelt das fest.
    #[test]
    fn normalize_derived_scores_recomputes_a_stale_stored_label() {
        let mut r = landing_record_fixture();
        r.score_numeric = Some(88);
        r.score_label = Some("acceptable".to_string()); // Ergebnis der ALTEN Regel
        r.grade_letter = Some("A".to_string());
        let n = normalize_derived_scores(r);
        assert_eq!(
            n.score_label.as_deref(),
            Some("smooth"),
            "Label muss der AKTUELLEN Regel folgen"
        );
        assert_eq!(n.grade_letter.as_deref(), Some("A"));
        assert_eq!(
            n.score_numeric,
            Some(88),
            "die Punktzahl selbst bleibt unangetastet"
        );
    }

    /// 2.0.6: Alt-Landungen mit Pauschal-Vref (`icao_default`) rechnen beim
    /// Lesen aus dem Gewicht neu (AIB 427: −20 kt pauschal → rund −5 kt,
    /// ohne Urteil); ohne Bezugswert faellt die Abweichung weg. Messungen
    /// bleiben, wie sie sind.
    #[test]
    fn vref_pauschal_wird_beim_lesen_neu_gerechnet() {
        let mut r = landing_record_fixture();
        r.aircraft_icao = Some("A388".to_string());
        r.landing_weight_kg = Some(348_525.0);
        r.landing_speed_kt = Some(125.0);
        r.landing_vref_source = Some("icao_default".to_string());
        r.landing_vref_deviation_kt = Some(-20.0);
        let n = normalize_derived_scores(r.clone());
        assert_eq!(n.landing_vref_source.as_deref(), Some("faa_ungeprueft"));
        assert_eq!(n.landing_vref_toleranz_kt, None);
        let dev = n.landing_vref_deviation_kt.unwrap();
        assert!((dev + 4.8).abs() < 0.3, "{dev}");

        r.aircraft_icao = Some("B738".to_string()); // FAA-Eintrag durchgefallen
        r.aircraft_title = None;
        let n = normalize_derived_scores(r.clone());
        assert_eq!(n.landing_vref_source.as_deref(), Some("unknown"));
        assert_eq!(n.landing_vref_deviation_kt, None);

        r.landing_vref_source = Some("pmdg".to_string());
        r.landing_vref_deviation_kt = Some(-3.0);
        let n = normalize_derived_scores(r);
        assert_eq!(n.landing_vref_deviation_kt, Some(-3.0));
    }

    /// Airbus-VLS zaehlt nur in Landestellung als Vref; eine gelesene Vref
    /// (iniBuilds, FSS, Zibo) immer. Quelle `fbw` nur fuer das FBW-Profil.
    #[test]
    fn vref_aus_dem_flugzeug_nimmt_vls_nur_in_landestellung() {
        let mut snap = SimSnapshot {
            vls_kt: Some(131.0),
            flap_handle_index: Some(4),
            flap_num_positions: Some(4),
            aircraft_profile: sim_core::AircraftProfile::FbwA32nx,
            ..SimSnapshot::default()
        };
        assert_eq!(vref_aus_dem_flugzeug(&snap), Some((131.0, "fbw")));
        snap.flap_handle_index = Some(2);
        assert_eq!(vref_aus_dem_flugzeug(&snap), None);
        snap.vref_kt = Some(133.0);
        snap.aircraft_profile = sim_core::AircraftProfile::IniA380;
        assert_eq!(vref_aus_dem_flugzeug(&snap), Some((133.0, "fmc")));
        // PMDG-FMC geht vor und heisst so.
        snap.pmdg = Some(sim_core::PmdgState { fmc_vref_kt: Some(141), ..Default::default() });
        assert_eq!(vref_aus_dem_flugzeug(&snap), Some((141.0, "pmdg")));
    }

    /// Gegenprobe: ein bereits korrekter Datensatz darf sich nicht aendern,
    /// und die Normalisierung muss ueber die ganze Leiter stimmen.
    #[test]
    fn normalize_derived_scores_is_stable_and_covers_every_band() {
        for (score, label, grade) in [
            (100, "smooth", "A+"),
            (95, "smooth", "A+"),
            (88, "smooth", "A"),
            (87, "acceptable", "B+"),
            (75, "acceptable", "B"),
            (74, "firm", "C"),
            (50, "firm", "D"),
            (49, "hard", "F"),
            (15, "hard", "F"),
            (14, "severe", "F"),
            (0, "severe", "F"),
        ] {
            let mut r = landing_record_fixture();
            r.score_numeric = Some(score);
            let n = normalize_derived_scores(r);
            assert_eq!(
                n.score_label.as_deref(),
                Some(label),
                "Label bei {score} Punkten"
            );
            assert_eq!(
                n.grade_letter.as_deref(),
                Some(grade),
                "Note bei {score} Punkten"
            );
            // idempotent: nochmal drueber aendert nichts mehr
            let again = normalize_derived_scores(n);
            assert_eq!(again.score_label.as_deref(), Some(label));
            assert_eq!(again.grade_letter.as_deref(), Some(grade));
        }
    }

    /// Ausreisser-Punktzahlen duerfen die Ableitung nicht sprengen.
    #[test]
    fn normalize_derived_scores_clamps_out_of_range_scores() {
        let mut low = landing_record_fixture();
        low.score_numeric = Some(-5);
        assert_eq!(
            normalize_derived_scores(low).score_label.as_deref(),
            Some("severe")
        );
        let mut high = landing_record_fixture();
        high.score_numeric = Some(250);
        assert_eq!(
            normalize_derived_scores(high).score_label.as_deref(),
            Some("smooth")
        );
    }

    #[test]
    fn canonical_peak_g_force_rejects_fallback_zero() {
        let mut stats = FlightStats::default();
        stats.landing_peak_g_force = Some(0.15);
        stats.landing_source = Some("fallback_zero".to_string());
        assert_eq!(stats.canonical_peak_g_force(), None);
    }

    // ─── Landung erkannt, aber nicht gemessen ────────────────────────────
    //
    // CFG 2090 (EDDF→KPDX, 12.09.2026): 0,92 s ohne Messung genau im
    // Aufsetzmoment, daraus 97 Punkte und Note A+. Seitdem gilt: Reicht die
    // Aufzeichnung nicht, gibt es keine Zahl — und zwar an JEDER Stelle,
    // die eine ausgeben könnte. Ein Riegel in der Kaskade allein genügte
    // nicht; Codex fand am selben Tag fünf weitere Wege ins Ergebnis.

    fn fehlende_abdeckung() -> touchdown_v2::FehlendeAbdeckung {
        touchdown_v2::FehlendeAbdeckung {
            groesste_luecke_ms: 920,
            proben: 5,
        }
    }

    #[test]
    fn ohne_ausreichende_aufzeichnung_gibt_es_keine_landerate() {
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-10.4);
        stats.landing_peak_vs_fpm = Some(-10.4);
        stats.landing_source = Some("vs_at_impact_frame".to_string());
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-10.4));

        stats.landung_abdeckung_fehlt = Some(fehlende_abdeckung());
        assert_eq!(
            stats.canonical_landing_rate_fpm(),
            None,
            "die Kanonik ist die Stelle, die Anzeige, PIREP und MQTT fragen"
        );
        assert_eq!(stats.canonical_peak_g_force(), None);
    }

    #[test]
    fn der_vom_simulator_gemessene_wert_bleibt_gueltig() {
        // MSFS meldet die Aufsetzrate selbst (`PLANE TOUCHDOWN NORMAL
        // VELOCITY`) — unabhängig von unserer Abtastung. Im Bestand haben
        // 29 von 1170 Landungen gar kein Aufsetzfenster und trotzdem einen
        // belastbaren Wert, genau über diese Quelle. Eine pauschale Sperre
        // hätte sie zu Unrecht entwertet.
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-168.0);
        stats.landing_peak_vs_fpm = Some(-168.0);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        stats.landung_abdeckung_fehlt = Some(fehlende_abdeckung());
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-168.0));
    }

    /// AZU 8710 (11.10.2026), nachgestellt in der Reihenfolge des Flugs:
    /// Streamer-Kaskade (−110), dann das dünne Fenster, dann der Edge-Wert
    /// −14 aus genau diesem Fenster. Gelten muss die Plugin-Messung −140.
    #[test]
    fn duennes_fenster_auf_x_plane_nimmt_die_plugin_sinkrate() {
        let mut stats = FlightStats::default();
        stats.sampler_touchdown_vs_fpm = Some(-140.48);
        stats.sampler_touchdown_vs_vom_plugin = true;
        stats.landing_peak_g_force = Some(1.14);
        finalize_landing_rate(&mut stats, -110.5, Some("Medium"), Some("last_low_agl_vs"));

        stats.landung_abdeckung_fehlt = Some(touchdown_v2::FehlendeAbdeckung {
            groesste_luecke_ms: 310,
            proben: 3,
        });
        assert!(plugin_sinkrate_uebernehmen(&mut stats));
        finalize_landing_rate(&mut stats, -14.35, Some("High"), Some("vs_at_edge_50hz"));

        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-140.48));
        assert_eq!(stats.landing_source.as_deref(), Some(PLUGIN_SINKRATE_QUELLE));
        assert_eq!(
            stats.canonical_peak_g_force(),
            None,
            "G stammt aus unserem Fenster und bleibt gesperrt"
        );
    }

    /// Gegenprobe: Ohne Plugin-Meldung ist `sampler_touchdown_vs_fpm` unser
    /// eigener Fensterwert — der darf die Sperre nicht umgehen.
    #[test]
    fn ohne_plugin_meldung_bleibt_das_duenne_fenster_gesperrt() {
        let mut stats = FlightStats::default();
        stats.sampler_touchdown_vs_fpm = Some(-14.35);
        stats.sampler_touchdown_vs_vom_plugin = false;
        stats.landung_abdeckung_fehlt = Some(fehlende_abdeckung());
        assert!(!plugin_sinkrate_uebernehmen(&mut stats));
        assert_eq!(stats.canonical_landing_rate_fpm(), None);
    }

    #[test]
    fn die_zahl_wird_erst_gar_nicht_festgehalten() {
        // Nicht nur verschweigen, sondern nicht speichern: Sonst fände die
        // nächste Auswertung den Wert im Protokoll und hielte ihn für gültig.
        let mut stats = FlightStats::default();
        stats.landung_abdeckung_fehlt = Some(fehlende_abdeckung());
        finalize_landing_rate(&mut stats, -10.4, Some("High"), Some("vs_at_impact_frame"));
        assert_eq!(stats.landing_rate_fpm, None);
        assert_eq!(stats.landing_peak_vs_fpm, None);
        assert_eq!(
            stats.landing_source.as_deref(),
            Some("aufzeichnung_unzureichend"),
            "die Begründung bleibt, damit die Anzeige sie nennen kann"
        );
    }

    #[test]
    fn ohne_messung_gibt_es_auch_keine_gesamtnote() {
        // Die Bewertung kennt "lieber gar keine Note als eine geschenkte"
        // bereits: Ohne Sinkrate wird der Teilwert übersprungen, und ohne
        // ihn entsteht kein Gesamtscore. Diese Kette muss halten.
        let ohne = landing_scoring::compute_sub_scores(&landing_scoring::LandingScoringInput {
            vs_fpm: None,
            ..Default::default()
        });
        assert!(
            ohne.iter().any(|s| s.key == "landing_rate" && s.skipped),
            "die Landerate muss als übersprungen erscheinen"
        );
        assert_eq!(landing_scoring::aggregate_master_score(&ohne), None);
    }

    // ─── Selbstmessung des Samplers ──────────────────────────────────────

    #[test]
    fn die_diagnose_rechnet_auch_ohne_laeufe_ohne_absturz() {
        // Ein Flug, der vor der ersten Landung endet: keine Läufe, keine
        // Division durch Null, keine erfundene Rate.
        let leer = SamplerDiagnose::default();
        assert_eq!(leer.takt_mittel_ms(), 0.0);
        assert_eq!(leer.proben_je_sekunde(), 0.0);

        // Ein einzelner Lauf hat noch keinen Abstand zum Vorgänger.
        let einer = SamplerDiagnose {
            laeufe: 1,
            proben: 1,
            ..Default::default()
        };
        assert_eq!(einer.takt_mittel_ms(), 0.0);
        assert_eq!(einer.proben_je_sekunde(), 0.0);
    }

    #[test]
    fn die_diagnose_gibt_den_echten_takt_wieder() {
        // Soll: 50 Läufe je Sekunde. Hier 100 Läufe mit je 20 ms Abstand
        // (99 Abstände) und lückenloser Probenausbeute.
        let gut = SamplerDiagnose {
            laeufe: 100,
            proben: 100,
            takt_summe_ms: 20.0 * 99.0,
            ..Default::default()
        };
        assert!((gut.takt_mittel_ms() - 20.0).abs() < 0.001);
        assert!((gut.proben_je_sekunde() - 50.0).abs() < 0.1);

        // Der Fall CFG 2090: der Takt bricht auf ein Fünfundzwanzigstel ein.
        let schlecht = SamplerDiagnose {
            laeufe: 100,
            proben: 100,
            takt_summe_ms: 500.0 * 99.0,
            ..Default::default()
        };
        assert!(
            (schlecht.proben_je_sekunde() - 2.0).abs() < 0.1,
            "zwei statt fünfzig Messungen je Sekunde: {}",
            schlecht.proben_je_sekunde()
        );
    }

    #[test]
    fn ausgefallene_proben_senken_die_gemeldete_rate() {
        // Der Takt stimmt, aber nur jede zweite Runde liefert eine Probe —
        // die gemeldete Rate muss das zeigen, sonst sieht die Diagnose
        // gesund aus, während im Fenster die Hälfte fehlt.
        let halb = SamplerDiagnose {
            laeufe: 100,
            proben: 50,
            ohne_snapshot: 50,
            takt_summe_ms: 20.0 * 99.0,
            ..Default::default()
        };
        assert!((halb.proben_je_sekunde() - 25.0).abs() < 0.2);
        let json = halb.als_json();
        assert_eq!(json["ohne_snapshot"], 50);
        assert_eq!(json["proben"], 50);
    }

    #[test]
    fn die_sperre_ueberlebt_einen_neustart() {
        // QS-Befund 12.09.2026 zum eigenen Umbau: Startet der Client
        // zwischen Aufsetzen und Einreichen neu, wurde der Zustand vorher
        // nicht mitgespeichert — die Sperre fiel weg, und über den in
        // `landing_analysis` gespeicherten Edge-Wert entstand wieder eine
        // Note für eine Landung, die niemand gemessen hat.
        let mut stats = FlightStats::default();
        stats.landung_abdeckung_fehlt = Some(touchdown_v2::FehlendeAbdeckung {
            groesste_luecke_ms: 920,
            proben: 5,
        });
        stats.sampler_diagnose = Some(serde_json::json!({ "proben_je_sekunde": 2.0 }));
        stats.landing_analysis = Some(serde_json::json!({ "vs_at_edge_fpm": -10.4 }));

        let gespeichert = PersistedFlightStats::snapshot_from(&stats);
        let json = serde_json::to_string(&gespeichert).expect("speicherbar");
        let geladen: PersistedFlightStats = serde_json::from_str(&json).expect("lesbar");
        let mut nach_neustart = FlightStats::default();
        geladen.apply_to(&mut nach_neustart);
        // Der Edge-Wert überlebt den Neustart in der Analyse — genau deshalb
        // muss der Zustand mit ihm zusammen zurückkommen.
        nach_neustart.landing_analysis = stats.landing_analysis.clone();

        assert!(
            nach_neustart.landung_abdeckung_fehlt.is_some(),
            "ohne den Zustand käme nach dem Neustart wieder eine Note heraus"
        );
        assert_eq!(nach_neustart.canonical_landing_rate_fpm(), None);
    }

    // ─── Codex, zweite Abnahme 13.09.2026 ───────────────────────────────

    #[test]
    fn eine_spaete_fensterverfeinerung_entwertet_die_simulatorrate_nicht() {
        // Ablauf aus der Abnahme: MSFS meldet −168 fpm, die Sperre ist
        // gesetzt, danach liefert der Sampler −10,4 aus dem dünnen Fenster.
        // Die neue Zahl muss verworfen werden — die alte samt Kennung bleiben.
        let mut stats = FlightStats::default();
        stats.landung_abdeckung_fehlt = Some(fehlende_abdeckung());
        finalize_landing_rate(
            &mut stats,
            -168.0,
            Some("High"),
            Some("msfs_simvar_latched"),
        );
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-168.0));

        finalize_landing_rate(&mut stats, -10.4, Some("High"), Some("vs_at_edge_50hz"));
        assert_eq!(
            stats.landing_source.as_deref(),
            Some("msfs_simvar_latched"),
            "die Kennung der gültigen Rate darf nicht überschrieben werden"
        );
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-168.0));
    }

    #[test]
    fn ohne_fenster_gelten_g_und_hopser_auch_bei_simulatorrate_nicht() {
        // Befund 1 der zweiten Abnahme: Bei `msfs_simvar_latched` wurde die
        // Fenster-Markierung gar nicht gesetzt, und G-Spitze und Hopser aus
        // dem FSM-Puffer liefen in die Bewertung. Die Sinkrate gilt, die
        // Werte aus unserem Fenster nicht.
        let mut stats = FlightStats::default();
        stats.landing_at = Some(t_fenster(0));
        stats.landing_peak_vs_fpm = Some(-168.0);
        stats.landing_rate_fpm = Some(-168.0);
        stats.landing_peak_g_force = Some(1.9);
        stats.bounce_count = 2;
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        finalize_landing_score_if_due(&mut stats, "TEST123", t_fenster(9), 0.0, true);

        assert!(
            stats.landung_abdeckung_fehlt.is_some(),
            "die Tatsache wird festgehalten"
        );
        assert_eq!(
            stats.canonical_landing_rate_fpm(),
            Some(-168.0),
            "die Sinkrate gilt"
        );
        assert_eq!(stats.canonical_peak_g_force(), None, "die G-Spitze nicht");
        assert!(score_g_for_stats(&stats).is_none());
        assert!(
            landung_nicht_bewertbar_json(&stats).is_none(),
            "mit gültiger Sinkrate ist die Landung bewertbar — kein Banner"
        );
        // Codex, dritte Abnahme: Die diskrete Klasse bekam die ungemessenen
        // Hopser trotzdem und wurde dadurch "Acceptable" statt "Smooth".
        assert_eq!(hopser_aus_dem_fenster(&stats), None);
        assert_eq!(
            stats.landing_score,
            Some(LandingScore::classify(-168.0, 0.0, 0)),
            "die Klasse darf nicht an ungemessenen Hopsern hängen"
        );
    }

    #[test]
    fn das_protokoll_erfindet_keine_null_fuer_ungemessenes_g() {
        // Codex, dritte Abnahme: Aus gesperrtem G wurde im Aktivitäts- und
        // ACARS-Protokoll "G 0.00" — eine Nullmessung, wo gar nicht gemessen
        // wurde.
        assert_eq!(g_protokolltext(None, false), "G nicht gemessen");
        assert_eq!(g_protokolltext(Some(1.23), true), "G 1.23");
        // Mit gemessenem Fenster, aber ohne Wert bleibt das bisherige Verhalten.
        assert_eq!(g_protokolltext(None, true), "G 0.00");
    }

    fn t_fenster(sek: i64) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.timestamp_opt(1_700_000_000 + sek, 0).unwrap()
    }

    #[test]
    fn canonical_peak_g_force_passes_through_genuine_source() {
        let mut stats = FlightStats::default();
        stats.landing_peak_g_force = Some(1.24);
        stats.landing_source = Some("msfs_simvar_latched".to_string());
        assert_eq!(stats.canonical_peak_g_force(), Some(1.24));
    }

    #[test]
    fn score_g_for_stats_is_none_for_fallback_zero_without_forensics() {
        let mut stats = FlightStats::default();
        stats.landing_peak_g_force = Some(0.15);
        stats.landing_source = Some("fallback_zero".to_string());
        assert!(score_g_for_stats(&stats).is_none());
    }

    /// QS-Fix (/code-review high, gefunden am Release-Review): OHNE den
    /// T&G/Go-Around-Reset (`landing_analysis`/`landing_source` = None,
    /// lib.rs ~25362) wuerde ein zweiter, fallback_zero-getaggter Touchdown
    /// nach einem Touch-and-Go trotzdem die ALTE Forensik des T&G als
    /// "unabhaengige Pipeline" durchlassen — `edge`/`scored_g` sind ja nur
    /// dann "von selbst leer", wenn wirklich genullt wurde. Dieser Test
    /// simuliert exakt die vom Reviewer gefundene Sequenz: T&G-Forensik
    /// erfassen → Reset (wie der Fix ihn jetzt macht) → zweite Landung
    /// faellt auf fallback_zero zurueck → NICHTS von der T&G-Landung darf
    /// mehr durchscheinen.
    #[test]
    fn stale_touch_and_go_forensics_do_not_leak_into_a_later_fallback_zero_landing() {
        let mut stats = FlightStats::default();
        // 1) Touch-and-Go: echte Forensik erfasst.
        stats.landing_analysis = Some(json!({
            "vs_at_edge_fpm": -300.0,
            "scored_g": 1.1,
        }));
        stats.landing_source = Some("vs_at_edge_50hz".to_string());
        stats.landing_rate_fpm = Some(-300.0);
        stats.landing_peak_vs_fpm = Some(-300.0);
        stats.landing_peak_g_force = Some(1.1);

        // 2) Der Fix: Reset-Block (lib.rs ~25362-25382) nullt jetzt ALLES,
        // nicht nur landing_peak_vs_fpm/landing_peak_g_force.
        stats.landing_at = None;
        stats.landing_peak_vs_fpm = None;
        stats.landing_peak_g_force = None;
        stats.landing_score = None;
        stats.bounce_count = 0;
        stats.landing_rate_fpm = None;
        stats.landing_analysis = None;
        stats.landing_source = None;

        // 3) Zweite (finale) Landung: kein echter Touchdown erfasst.
        stats.landing_rate_fpm = Some(0.0);
        stats.landing_peak_vs_fpm = Some(0.0);
        stats.landing_peak_g_force = Some(0.15);
        stats.landing_source = Some("fallback_zero".to_string());

        assert_eq!(
            stats.canonical_landing_rate_fpm(),
            None,
            "darf NICHT die -300 fpm der T&G durchlassen"
        );
        assert!(
            score_g_for_stats(&stats).is_none(),
            "darf NICHT die 1.1 G der T&G durchlassen"
        );
    }

    #[test]
    fn glitch_samples_kommen_nicht_in_den_anflug_puffer() {
        // VPS-Korpus, Flug 127 (MSFS): Sinkraten-Streuung 12.810 fpm aus 7
        // Samples. Kein Flugzustand, sondern ein Sim-Artefakt (Pause/Slew/
        // Ladebild). Ein einziges solches Sample vergiftet Streuung, Jerk,
        // Gleitpfad-Abweichung und Stabilitaets-Gate zugleich — deshalb wird
        // an der QUELLE gefiltert, nicht in den einzelnen Formeln.
        let mut snap = SimSnapshot::default();
        snap.altitude_agl_ft = 800.0;
        snap.groundspeed_kt = 140.0;
        snap.indicated_airspeed_kt = 145.0;
        snap.bank_deg = 2.0;

        snap.vertical_speed_fpm = -750.0;
        assert!(
            approach_sample_is_plausible(&snap),
            "normaler Anflug muss rein"
        );

        snap.vertical_speed_fpm = -30_000.0;
        assert!(
            !approach_sample_is_plausible(&snap),
            "Slew/Teleport muss raus"
        );

        snap.vertical_speed_fpm = f32::NAN;
        assert!(!approach_sample_is_plausible(&snap), "NaN muss raus");

        snap.vertical_speed_fpm = f32::INFINITY;
        assert!(!approach_sample_is_plausible(&snap), "Inf muss raus");

        // Ein steiler, aber echter Sinkflug bleibt drin — wir filtern
        // Artefakte, nicht schlechtes Fliegen. (Der LFMN-Flug mit -2000 fpm
        // bis 750 ft SOLL seine schlechte Stabilitaets-Note bekommen.)
        snap.vertical_speed_fpm = -2036.0;
        assert!(
            approach_sample_is_plausible(&snap),
            "ein genuin steiler Anflug darf NICHT weggefiltert werden",
        );

        snap.vertical_speed_fpm = -750.0;
        snap.bank_deg = 179.0;
        assert!(
            !approach_sample_is_plausible(&snap),
            "Rueckenlage im Anflug = Artefakt"
        );
    }

    #[test]
    fn stabilitaets_streuung_braucht_eine_echte_stichprobe() {
        // Im Korpus lieferten die duennen Fenster den Unsinn: bei 1-9 Samples
        // war jede zweite V/S-Streuung ueber 1000 fpm. Unter der Mindest-
        // Stichprobe gibt es KEINEN Wert — der Sub-Score wird uebersprungen,
        // statt eine erfundene Zahl als Note auszuliefern.
        let mk = |n: usize| {
            let mut buf = std::collections::VecDeque::new();
            let t0 = Utc::now();
            for i in 0..n {
                buf.push_back(ApproachBufferSample {
                    at: t0 + chrono::Duration::seconds(i as i64),
                    agl_ft: 900.0,
                    msl_ft: 900.0,
                    gs_kt: 140.0,
                    ias_kt: 145.0,
                    vs_fpm: -700.0 + (i % 3) as f32 * 20.0,
                    bank_deg: 1.0,
                    heading_true_deg: 90.0,
                    gear_position: 1.0,
                    flaps_position: 1.0,
                    selected_runway: None,
                    stall_warning: false,
                    lat: None,
                    lon: None,
                    pitch_deg: None,
                    n1_mittel_pct: None,
                    headwind_kt: None,
                    crosswind_kt: None,
                });
            }
            compute_approach_stddev(&buf, None)
        };
        let (vs_thin, bank_thin) = mk(STABILITY_MIN_SAMPLES - 1);
        assert_eq!(
            vs_thin, None,
            "zu duenne Stichprobe darf keinen Wert liefern"
        );
        assert_eq!(bank_thin, None);

        let (vs_ok, bank_ok) = mk(STABILITY_MIN_SAMPLES + 5);
        assert!(
            vs_ok.is_some(),
            "ausreichende Stichprobe muss bewertet werden"
        );
        assert!(bank_ok.is_some());
    }

    #[test]
    fn ellx804_positiver_edge_wert_wird_nicht_als_landerate_ausgeliefert() {
        // Flug 804 (ELLX, 14.07.2026, Client v0.20.0): der Edge-Wert war
        // +24,47 fpm — eine POSITIVE Sinkrate beim Aufsetzen, physikalisch
        // Unsinn. Er wurde nur deshalb nicht ausgeliefert, weil die Kanonik
        // zufaellig auf "< 0" prueft. Ein negativ-absurder Glitch waere
        // ungebremst durchgegangen.
        let mut stats = FlightStats::default();
        stats.landing_peak_vs_fpm = Some(-38.0);
        stats.landing_analysis = Some(json!({ "vs_at_edge_fpm": 24.474783 }));
        assert_eq!(
            stats.canonical_landing_rate_fpm(),
            Some(-38.0),
            "positiver Edge-Wert darf nicht als Landerate durchgehen",
        );
    }

    #[test]
    fn absurde_landerate_wird_verworfen_statt_ausgeliefert() {
        // Der Fall, den nichts aufgehalten haette: ein Sim-Glitch mit stark
        // NEGATIVER Sinkrate. Der Edge-Wert ist eine ungeguardete Interpolation;
        // touchdown_v2 lehnt so etwas seit jeher ab (finalize_vs), die Kanonik
        // tat es nicht.
        let mut stats = FlightStats::default();
        stats.landing_analysis = Some(json!({ "vs_at_edge_fpm": -30_000.0 }));
        stats.landing_peak_vs_fpm = Some(-247.0);
        assert_eq!(
            stats.canonical_landing_rate_fpm(),
            Some(-247.0),
            "absurder Edge-Wert muss auf den naechsten plausiblen Kandidaten fallen",
        );

        // Auch der FALLBACK muss geprueft werden — sonst wandert der Glitch
        // eben ueber ihn in den PIREP.
        let mut all_broken = FlightStats::default();
        all_broken.landing_analysis = Some(json!({ "vs_at_edge_fpm": -30_000.0 }));
        all_broken.landing_peak_vs_fpm = Some(-42_000.0);
        all_broken.landing_rate_fpm = Some(f32::NAN);
        assert_eq!(
            all_broken.canonical_landing_rate_fpm(),
            None,
            "wenn KEIN Kandidat plausibel ist, gibt es keine Landerate — \
             das ist ehrlicher als eine erfundene Zahl",
        );

        // Eine harte, aber echte Landung bleibt eine Landung.
        let mut hard = FlightStats::default();
        hard.landing_analysis = Some(json!({ "vs_at_edge_fpm": -890.0 }));
        assert_eq!(hard.canonical_landing_rate_fpm(), Some(-890.0));
    }

    #[test]
    fn ohne_landerate_wird_die_achse_sichtbar_uebersprungen_nicht_weggelassen() {
        // Der gefaehrlichste Befund aus dem Review dieses Fixes: seit die
        // Kanonik unplausible Werte verwirft, kann sie None liefern — und dann
        // fehlte der Sinkraten-Sub-Score KOMPLETT. Das Aggregat waere aus den
        // uebrigen Achsen gebildet worden, und ein Flug, dessen Landerate gar
        // nicht messbar war, haette einen gut aussehenden Score bekommen. Die
        // wichtigste Achse still wegzulassen ist schlimmer als die falsche Zahl,
        // die wir gerade abgeschafft haben.
        let input = landing_scoring::LandingScoringInput {
            vs_fpm: None,
            peak_g_load: Some(1.2),
            scored_g_load: Some(1.18),
            bounce_count: Some(0),
            ..Default::default()
        };
        let subs = landing_scoring::compute_sub_scores(&input);
        let rate = subs
            .iter()
            .find(|s| s.key == "landing_rate")
            .expect("die Sinkraten-Achse MUSS auftauchen — sichtbar als skipped");
        assert!(rate.skipped, "sie muss als 'nicht bewertet' markiert sein");
        assert_eq!(rate.reason.as_deref(), Some("landing_rate_unmeasurable"));
    }

    #[test]
    fn gefilterte_stabilitaetswerte_ueberleben_einen_neustart() {
        // Code-Review-Befund (betraf auch das schon veroeffentlichte v0.20.0):
        // `PersistedFlightStats` sicherte nur die Legacy-Werte. Stuerzte die App
        // nach dem Touchdown und vor dem Filing ab, waren die v2-Werte nach dem
        // Restore weg — die Kanonik fiel auf das alte Fenster zurueck und der
        // PIREP wurde anders bewertet als der Landungs-Tab es zeigte. Derselbe
        // Flug, zwei Noten, je nachdem ob ein Neustart dazwischen lag.
        let mut stats = FlightStats::default();
        stats.approach_vs_stddev_fpm = Some(641.0);
        stats.approach_bank_stddev_deg = Some(7.4);
        stats.approach_vs_stddev_filtered_fpm = Some(180.0);
        stats.approach_bank_stddev_filtered_deg = Some(1.0);

        let persisted = PersistedFlightStats::snapshot_from(&stats);
        let json = serde_json::to_string(&persisted).expect("serialisierbar");
        let back: PersistedFlightStats = serde_json::from_str(&json).expect("deserialisierbar");

        let mut restored = FlightStats::default();
        back.apply_to(&mut restored);

        assert_eq!(
            restored.canonical_vs_stddev_fpm(),
            Some(180.0),
            "nach dem Neustart muss die Bewertung aus DEMSELBEN Fenster kommen",
        );
        assert_eq!(restored.canonical_bank_stddev_deg(), Some(1.0));
    }

    #[test]
    fn vs_und_bank_streuung_kommen_aus_demselben_fenster() {
        // Der Stabilitaets-Sub-Score bewertet beide Achsen. Bis v0.20.0 kam die
        // Bank aus dem v2-Fenster (gefiltert, <= 1000 ft HAT) und die V/S aus dem
        // Legacy-Fenster (ungefiltert, <= 1500 ft AGL) — eine Note aus zwei
        // Wirklichkeiten. Im Korpus hatten 18 Landungen NULL Samples im v2-Fenster
        // und trotzdem eine Legacy-V/S-Streuung.
        let mut stats = FlightStats::default();
        stats.approach_vs_stddev_fpm = Some(641.0); // legacy
        stats.approach_bank_stddev_deg = Some(7.4);
        stats.approach_vs_stddev_filtered_fpm = Some(180.0); // v2, dasselbe Fenster
        stats.approach_bank_stddev_filtered_deg = Some(1.0);

        assert_eq!(stats.canonical_vs_stddev_fpm(), Some(180.0));
        assert_eq!(stats.canonical_bank_stddev_deg(), Some(1.0));

        // Alt-Datensatz ohne v2-Felder: BEIDE fallen gemeinsam auf Legacy
        // zurueck — niemals die eine Achse v2 und die andere legacy.
        let mut old = FlightStats::default();
        old.approach_vs_stddev_fpm = Some(641.0);
        old.approach_bank_stddev_deg = Some(7.4);
        assert_eq!(old.canonical_vs_stddev_fpm(), Some(641.0));
        assert_eq!(old.canonical_bank_stddev_deg(), Some(7.4));
    }

    #[test]
    fn bank_stddev_kanonik_bevorzugt_den_gefilterten_v2_wert() {
        // Dieselbe Krankheit wie bei der Sinkrate, nur mit der Bank-Streuung:
        // legacy (ungefiltert, bis 1500 ft AGL) zaehlt absichtliche Anflug-
        // kurven als Instabilitaet mit und ueberschaetzt sie. Das Stabilitaets-
        // Gate rechnete laengst aus v2, Sub-Score und Client-Kachel aber aus
        // legacy — Banner "instabil" neben harmloser Kachel.
        let mut stats = FlightStats::default();
        stats.approach_bank_stddev_deg = Some(7.4);
        stats.approach_bank_stddev_filtered_deg = Some(2.1);
        assert_eq!(
            stats.canonical_bank_stddev_deg(),
            Some(2.1),
            "v2 (gefiltert) muss gewinnen, nicht der ungefilterte Legacy-Wert",
        );

        // Alt-Datensatz ohne v2-Feld: legacy bleibt Fallback, sonst verlieren
        // bestehende Landungen ihre Stabilitaets-Anzeige.
        let mut old = FlightStats::default();
        old.approach_bank_stddev_deg = Some(7.4);
        assert_eq!(old.canonical_bank_stddev_deg(), Some(7.4));

        // Gar keine Daten → None, kein 0.0-Default (0.0 hiesse "perfekt
        // stabil" und wuerde einen VFR-Anflug ohne Buffer voll punkten).
        assert_eq!(FlightStats::default().canonical_bank_stddev_deg(), None);
    }

    #[test]
    fn pia3452_class_is_derived_from_the_same_vs_as_the_score() {
        // Vor dem Fix: classify() lief auf `landing_peak_vs_fpm` (-233),
        // die Punkte auf dem Edge (-206) → "SMOOTH · 92/100" aus zwei
        // verschiedenen Metriken. Klasse und Punkte muessen auf DERSELBEN
        // Zahl beruhen.
        let stats = pia3452_live_stats();
        let canonical = stats.canonical_landing_rate_fpm().expect("some");
        let scored_g = score_g_for_stats(&stats).expect("some").scored_g;
        assert_eq!(
            LandingScore::classify(canonical, scored_g, stats.bounce_count),
            LandingScore::classify(
                score_basis_vs_fpm(&stats).expect("some"),
                scored_g,
                stats.bounce_count
            ),
        );
    }

    /// Strukturelle Schranke gegen den PIA3452-Rueckfall.
    ///
    /// Die Regel "Anzeige-/Filing-Pfade lesen NIE `landing_rate_fpm` /
    /// `landing_peak_vs_fpm` direkt, sondern immer `canonical_landing_rate_
    /// fpm()`" stand seit v0.7.17 als KOMMENTAR im Code — und wurde vom
    /// touchdown-v2-Pfad trotzdem unterlaufen, weil ein Kommentar nichts
    /// erzwingt. Dieser Test macht die Regel pruefbar: er liest den eigenen
    /// Quelltext und faellt um, sobald eine der Funktionen, die dem Piloten
    /// eine V/S-Zahl ZEIGT oder sie an phpVMS SCHICKT, wieder direkt auf ein
    /// Rohfeld zugreift.
    ///
    /// Die Rohfelder bleiben absichtlich schreibbar (Streamer-Refinement,
    /// FSM-Fallback, Snapshot-Restore, LandingRecord-Forensik) — verboten ist
    /// nur das LESEN in den unten gelisteten Ausgabe-Funktionen.
    #[test]
    fn display_and_filing_paths_never_read_raw_vs_fields() {
        const SRC: &str = include_str!("../lib.rs");

        /// Body einer Funktion, ohne Kommentarzeilen. Kommentare DUERFEN die
        /// Rohfelder benennen — sie erklaeren ja gerade, warum man sie nicht
        /// liest.
        fn body_of(src: &str, needle: &str) -> String {
            let start = src.find(needle).unwrap_or_else(|| {
                panic!("Ausgabe-Funktion nicht mehr gefunden: {needle} — Test anpassen, nicht loeschen")
            });
            let rest = &src[start..];
            let end = rest[1..].find("\nfn ").map(|i| i + 1).unwrap_or(rest.len());
            rest[..end]
                .lines()
                .filter(|l| !l.trim_start().starts_with("//"))
                .collect::<Vec<_>>()
                .join("\n")
        }

        // ── Sinkrate ────────────────────────────────────────────────────────
        // Funktionen, die eine V/S-Zahl nach aussen geben (Pilot-UI, ACARS-Log,
        // Flight-Log-Forensik). Neue Ausgabe-Pfade hier eintragen.
        for needle in ["fn announce_landing_score(", "fn emit_landing_finalized("] {
            let body = body_of(SRC, needle);
            // Punkt-Praefix, sonst matcht `.canonical_landing_rate_fpm()` auf
            // sein eigenes Suffix `landing_rate_fpm`.
            for raw in [".landing_peak_vs_fpm", ".landing_rate_fpm"] {
                assert!(
                    !body.contains(raw),
                    "{needle} liest das Rohfeld `{raw}` direkt. Das ist der \
                     PIA3452-Bug (Log -233 vs Karte/PIREP -206): Rohfelder \
                     koennen touchdown-v2s Fenster-Minimum tragen, waehrend \
                     Score und PIREP ueber die Kanonik den 50-Hz-Edge zeigen. \
                     Nutze `stats.canonical_landing_rate_fpm()`.",
                );
            }
            assert!(
                body.contains("canonical_landing_rate_fpm()"),
                "{needle} gibt eine V/S-Zahl aus, holt sie aber nicht ueber \
                 `canonical_landing_rate_fpm()` — genau so ist der PIA3452-Split \
                 entstanden.",
            );
        }

        // ── Bank-Streuung ───────────────────────────────────────────────────
        // Dieselbe Krankheit, andere Kennzahl: legacy (ungefiltert, bis 1500 ft
        // AGL) vs. v2 (vektor-gefiltert, bis 1000 ft HAT). Das Stabilitaets-Gate
        // rechnete aus v2, die PIREP-Felder zeigten legacy — Banner "instabil"
        // neben einer harmlos aussehenden Zahl.
        // v0.20.1: BEIDE Achsen. Die Schranke deckte nur die Bank ab — deshalb
        // konnte die V/S-Streuung munter aus einem anderen Fenster kommen, ohne
        // dass ein Test umfiel. Eine Schranke, die nur die halbe Formel schuetzt,
        // schuetzt die Formel nicht.
        for needle in [
            "fn build_pirep_fields(",
            "fn build_pirep_notes(",
            "fn build_pirep_payload(",
            "fn compute_aggregate_master_score(",
        ] {
            let body = body_of(SRC, needle);
            for (raw, canonical) in [
                (
                    "stats.approach_bank_stddev_deg",
                    "canonical_bank_stddev_deg()",
                ),
                ("stats.approach_vs_stddev_fpm", "canonical_vs_stddev_fpm()"),
            ] {
                assert!(
                    !body.contains(raw),
                    "{needle} liest das Legacy-Rohfeld `{raw}` direkt. Bank- und \
                     V/S-Streuung fliessen in DENSELBEN Stabilitaets-Sub-Score — \
                     sie muessen aus demselben Fenster kommen, sonst ist die Note \
                     aus zwei Wirklichkeiten gemischt. Nutze `stats.{canonical}`.",
                );
            }
        }

        // ── Bewertung (Punkte, Klasse, Note) ────────────────────────────────
        // Die Touchdown-Klasse (`stats.landing_score`, nur 100/80/60/30/0) ist
        // NICHT die Bewertung — sie ist ein Teil davon. Wer sie direkt ausgibt,
        // schreibt "A+ (SMOOTH, 100/100)" neben ein Feld, in dem "A (smooth) —
        // 92/100" steht. Genau das stand auf Thomas' phpVMS-Seite.
        for needle in ["fn build_pirep_notes(", "fn build_pirep_fields("] {
            let body = body_of(SRC, needle);
            assert!(
                body.contains("canonical_landing_verdict(")
                    || body.contains("compute_aggregate_master_score("),
                "{needle} gibt eine Note oder Klasse aus, ohne sie ueber die \
                 EINE Bewertung zu holen (`canonical_landing_verdict()`). \
                 Die Touchdown-Klasse allein ist nicht das Urteil.",
            );
        }
    }

    #[test]
    fn shim_score_basis_vs_fpm_delegates_to_canonical() {
        // Der alte freistehende `score_basis_vs_fpm()` ist ab v0.7.21
        // nur noch ein Shim auf die Methode. Beide muessen IMMER
        // dasselbe liefern.
        let stats = ms713_live_stats();
        assert_eq!(
            score_basis_vs_fpm(&stats),
            stats.canonical_landing_rate_fpm()
        );
    }

    #[test]
    fn no_edge_falls_back_to_peak_then_rate() {
        // Ohne 50-Hz-Buffer-Dump (= GSG219-Fall): Cascade soll auf
        // landing_peak_vs_fpm fallen.
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-300.0);
        stats.landing_peak_vs_fpm = Some(-450.0);
        stats.landing_analysis = None;
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-450.0));

        // Ohne Peak: auf landing_rate_fpm.
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-300.0);
        stats.landing_peak_vs_fpm = None;
        stats.landing_analysis = None;
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-300.0));
    }

    #[test]
    fn positive_edge_value_is_ignored_falls_to_cascade() {
        // Defensiver Guard: edge < 0.0 ist Pflicht (negative = Sinken).
        // Ein positiver Edge waere ein Sim-Bug → wir nehmen den nicht.
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-200.0);
        stats.landing_peak_vs_fpm = Some(-180.0);
        stats.landing_analysis = Some(json!({
            "vs_at_edge_fpm": 42.0, // bogus positive
        }));
        // Edge wird verworfen → Peak gewinnt.
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-180.0));
    }

    #[test]
    fn none_when_nothing_set() {
        let stats = FlightStats::default();
        assert_eq!(stats.canonical_landing_rate_fpm(), None);
    }

    #[test]
    fn b005_anchor_smooth_with_high_g_spike() {
        // v0.7.17 B-009-Anchor: -116 fpm aber 2.30 G — vor dem Fix
        // wurde der spurious Edge nicht erkannt. canonical MUSS hier
        // den Edge liefern, NICHT den Peak.
        let mut stats = FlightStats::default();
        stats.landing_rate_fpm = Some(-116.0);
        stats.landing_peak_vs_fpm = Some(-200.0); // sim strut spike
        stats.landing_analysis = Some(json!({
            "vs_at_edge_fpm": -116.0,
        }));
        assert_eq!(stats.canonical_landing_rate_fpm(), Some(-116.0));
    }
