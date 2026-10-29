    use super::*;

    // ─── Runway-Geometry-Trust ──────────────────────────────────────────

    #[test]
    fn runway_trust_passes_for_clean_match() {
        // ENSB Match wie in SAS9987 v0.7.5: matched_icao=arr, kleine
        // Centerline-Distanz, Float-Distanz im normalen Bereich.
        let (trusted, reason) = runway_geometry_trust_check(
            Some("ENSB"),
            "ENSB",
            None,
            Some(2.0),   // 2 m centerline offset
            Some(320.0), // 320 m past threshold
        );
        assert!(trusted);
        assert!(reason.is_none());
    }

    #[test]
    fn runway_trust_blocks_on_icao_mismatch() {
        // GSG303-Klasse: arr=OR66 aber Match war K5S9 (3.5 km daneben)
        let (trusted, reason) =
            runway_geometry_trust_check(Some("K5S9"), "OR66", None, Some(3464.0), Some(-613.0));
        assert!(!trusted);
        // ICAO-Check kommt zuerst
        assert_eq!(reason, Some("icao_mismatch"));
    }

    #[test]
    fn runway_trust_accepts_divert_icao() {
        // Divert-Szenario: arr war LFPG, divert nach EDDM, Match=EDDM.
        // Match darf entweder arr ODER divert_to sein.
        let (trusted, reason) = runway_geometry_trust_check(
            Some("EDDM"),
            "LFPG",
            Some("EDDM"),
            Some(15.0),
            Some(450.0),
        );
        assert!(trusted);
        assert!(reason.is_none());
    }

    #[test]
    fn runway_trust_accepts_divert_via_effective_arr() {
        // v0.15.x (Live-Report Thomas, GLG2): build_landing_record reicht bei
        // einem Divert jetzt den EFFEKTIVEN Landeflughafen als `arr_airport`
        // durch (statt des geplanten). Geplant MZBE, gelandet + gematcht MGGT
        // → trusted, Rollout/Runway werden gewertet.
        let (trusted, reason) = runway_geometry_trust_check(
            Some("MGGT"),
            "MGGT", // effektiver (tatsächlicher) Landeflughafen
            None,
            Some(-10.0),
            Some(450.0),
        );
        assert!(trusted);
        assert!(reason.is_none());

        // Gegenprobe — der ALTE Bug: geplantes Ziel (MZBE) durchgereicht,
        // kein divert_to → icao_mismatch → Rollout fälschlich „untrusted".
        let (trusted_old, reason_old) = runway_geometry_trust_check(
            Some("MGGT"),
            "MZBE", // geplant (alt, falsch)
            None,
            Some(-10.0),
            Some(450.0),
        );
        assert!(!trusted_old);
        assert_eq!(reason_old, Some("icao_mismatch"));
    }

    #[test]
    fn g_referenzkette_gleicht_xplane_an_msfs_an() {
        // Kernfall der Fairness-Normierung: derselbe Landestoss, in X-Plane
        // ungefiltert gemessen, muss nach der Umrechnung im selben
        // Punkteband landen wie der gedaempfte MSFS-Wert.
        // Feldzahlen A21N bei ~220 fpm: MSFS 1,14 g, X-Plane 1,37 g.
        let xp = g_auf_referenzkette(1.37, Some("xplane"));
        assert!(
            (xp - 1.16).abs() < 0.02,
            "X-Plane 1,37 g muss auf ~1,16 g der MSFS-Kette abbilden, war {xp}"
        );
        // Beide landen damit im 100-Punkte-Band (Schwelle 1,20).
        assert!(xp <= 1.20);
    }

    #[test]
    fn g_referenzkette_laesst_msfs_und_unbekannte_unveraendert() {
        for sim in [Some("msfs"), Some("other"), None] {
            assert_eq!(g_auf_referenzkette(1.42, sim), 1.42, "sim={sim:?}");
        }
    }

    #[test]
    fn g_referenzkette_haelt_den_ruhezustand_fest() {
        // 1 g ist in beiden Sims derselbe Ruhezustand — die Normierung darf
        // ihn nicht verschieben, sonst wanderte jede sanfte Landung.
        assert!((g_auf_referenzkette(1.0, Some("xplane")) - 1.0).abs() < 1e-6);
        // Und sie muss monoton bleiben: haerter bleibt haerter.
        let a = g_auf_referenzkette(1.5, Some("xplane"));
        let b = g_auf_referenzkette(2.5, Some("xplane"));
        assert!(b > a);
    }

    #[test]
    fn runway_trust_blocks_on_centerline_offset_too_large() {
        // ICAO matched aber Centerline-Offset > 200 m → vermutlich
        // falsche Runway-Auswahl auf demselben Flughafen (z.B. parallel
        // runway 26L statt 26R, 200m Achsen-Abstand).
        let (trusted, reason) = runway_geometry_trust_check(
            Some("EDDM"),
            "EDDM",
            None,
            Some(250.0), // > 200 m
            Some(400.0),
        );
        assert!(!trusted);
        assert_eq!(reason, Some("centerline_offset_too_large"));
    }

    #[test]
    fn runway_trust_at_centerline_threshold_boundary() {
        // 200.0 m exakt: <= MAX → trusted
        let (trusted, _) =
            runway_geometry_trust_check(Some("EDDM"), "EDDM", None, Some(200.0), Some(400.0));
        assert!(trusted, "200 m exakt darf noch trusted sein");

        // 200.01 m: > MAX → not trusted
        let (trusted, reason) =
            runway_geometry_trust_check(Some("EDDM"), "EDDM", None, Some(200.01), Some(400.0));
        assert!(!trusted);
        assert_eq!(reason, Some("centerline_offset_too_large"));
    }

    #[test]
    fn runway_trust_blocks_on_negative_float_distance() {
        // GSG303: float_distance = -613 m (Touchdown weit vor Threshold).
        let (trusted, reason) =
            runway_geometry_trust_check(Some("OR66"), "OR66", None, Some(50.0), Some(-613.0));
        assert!(!trusted);
        assert_eq!(reason, Some("negative_float_distance"));
    }

    #[test]
    fn runway_trust_at_float_threshold_boundary() {
        // -100.0 m exakt: >= MIN → trusted (legitim wenn Runway-DB
        // TDZ-Marker statt Threshold nutzt).
        let (trusted, _) =
            runway_geometry_trust_check(Some("EDDM"), "EDDM", None, Some(50.0), Some(-100.0));
        assert!(trusted, "-100 m exakt darf noch trusted sein");

        // -100.01 m: < MIN → not trusted
        let (trusted, reason) =
            runway_geometry_trust_check(Some("EDDM"), "EDDM", None, Some(50.0), Some(-100.01));
        assert!(!trusted);
        assert_eq!(reason, Some("negative_float_distance"));
    }

    #[test]
    fn runway_trust_no_match_returns_no_runway_match() {
        // v1.1: "kein Match" und "falscher Match" sind verschieden.
        // Bei Privatplaetzen / Bush-Strips ohne ourairports-DB-Eintrag
        // ist None das normale Verhalten. UI darf das NICHT als
        // alarmierende "Falscher Flughafen erkannt"-Warnung zeigen.
        let (trusted, reason) =
            runway_geometry_trust_check(None, "EDDM", None, Some(50.0), Some(400.0));
        assert!(!trusted);
        assert_eq!(reason, Some("no_runway_match"));
    }

    #[test]
    fn runway_trust_icao_compare_is_case_insensitive() {
        // v0.7.6 P3-Refinement: ICAOs koennten aus externen Quellen
        // mit Mixed-Case kommen (phpVMS-Imports, OFP-Snapshots).
        // Match "EDDM" gegen arr "eddm" muss trusted sein.
        let (trusted, reason) =
            runway_geometry_trust_check(Some("EDDM"), "eddm", None, Some(50.0), Some(400.0));
        assert!(trusted, "EDDM/eddm muss case-insensitive matchen");
        assert!(reason.is_none());

        // Auch fuer divert
        let (trusted, _) = runway_geometry_trust_check(
            Some("eddm"),
            "LFPG",
            Some("EdDm"),
            Some(15.0),
            Some(400.0),
        );
        assert!(trusted, "divert ICAO muss case-insensitive matchen");

        // Sanity: echte unterschiedliche Codes bleiben mismatch
        let (trusted, reason) =
            runway_geometry_trust_check(Some("EDDF"), "EDDM", None, Some(50.0), Some(400.0));
        assert!(!trusted);
        assert_eq!(reason, Some("icao_mismatch"));
    }

    #[test]
    fn runway_trust_empty_match_string_returns_no_runway_match() {
        // Defensive Variante: Leerer String wird wie None behandelt
        // (verteidigt gegen Adapter die "" statt None liefern).
        let (trusted, reason) =
            runway_geometry_trust_check(Some(""), "EDDM", None, Some(50.0), Some(400.0));
        assert!(!trusted);
        assert_eq!(reason, Some("no_runway_match"));
    }

    #[test]
    fn runway_trust_handles_missing_offsets_gracefully() {
        // Diagnostische Felder fehlen aber ICAO matcht → trusted
        // (wir bestrafen nicht das Fehlen einer optionalen Diagnostik).
        let (trusted, reason) = runway_geometry_trust_check(Some("EDDM"), "EDDM", None, None, None);
        assert!(trusted);
        assert!(reason.is_none());
    }

    #[test]
    fn runway_trust_thresholds_are_pinned_to_spec() {
        // Spec docs/spec/v0.7.6-landing-payload-consistency.md §3 P1-3
        // pinned die Werte explizit. Aenderung der Konstanten ist eine
        // Spec-Aenderung — dieser Test schlaegt fehl wenn jemand die
        // Werte still-und-leise verschiebt.
        assert_eq!(crate::grenzwerte::grenzwerte().ausrollen.runway_trust_max_centerline_offset_m.wert, 200.0);
        assert_eq!(crate::grenzwerte::grenzwerte().ausrollen.runway_trust_min_float_distance_m.wert, -100.0);
    }

    // ─── Bounce-Threshold-Konstanten (touchdown_v2) ─────────────────────

    #[test]
    fn bounce_thresholds_are_pinned_to_spec() {
        // Spec §3 P1-2: 5 ft forensic; scored seit Lernpaket AP3
        // (29.09.2026) ebenfalls 5 ft statt 15 ft — Begruendung und
        // Messung am Live-Bestand siehe `BOUNCE_SCORED_MIN_AGL_FT`.
        // Aenderung der Werte ist eine Spec-Aenderung — dieser Test schlaegt
        // fehl wenn jemand die Werte still-und-leise verschiebt.
        assert_eq!(crate::grenzwerte::grenzwerte().aufsetzen.bounce_forensic_min_agl_ft.wert, 5.0);
        assert_eq!(crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert, 5.0);
    }

    #[test]
    fn bounce_scored_never_below_forensic() {
        // Sanity: was benotet wird, muss auch forensisch sichtbar sein.
        // Seit Lernpaket AP3 sind beide Schwellen gleich (5 ft) — jeder
        // sichtbare Hopser ab 5 ft ist ein „high bounce" und zaehlt.
        assert!(crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert >= crate::grenzwerte::grenzwerte().aufsetzen.bounce_forensic_min_agl_ft.wert);
    }

    #[test]
    fn sas9987_bounce_class_counts_since_lernpaket_ap3() {
        // SAS9987 v0.7.5 Real-Beleg: bounce_max_agl_ft = 13.57. Galt bis
        // Lernpaket AP3 als score-freier „Federwerk-Hopser" (15-ft-Schwelle);
        // 13,6 ft sind aber ein echtes Wiederabheben und zaehlen jetzt.
        // Das Bodenflag-Flackern (Live-Bestand: bis 4,8 ft) bleibt draussen.
        let bounce_height = 13.57_f32;
        assert!(bounce_height >= crate::grenzwerte::grenzwerte().aufsetzen.bounce_forensic_min_agl_ft.wert);
        assert!(bounce_height >= crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert);
        let flackern_max = 4.8_f32; // td 1474, 3,1 s „in der Luft"
        assert!(flackern_max < crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert);
    }

    #[test]
    fn streamer_bounce_threshold_matches_scored_v2_threshold() {
        // v0.8.3 (#3): Der live-Streamer-Tally (BOUNCE_AGL_THRESHOLD_FT)
        // muss gegen dieselbe Schwelle messen wie der touchdown_v2
        // Forensics-SCORED-Pfad — sonst weichen scored_bounce_count und
        // stats.bounce_count (Fallback bei unvollstaendigem Sampler-
        // Buffer) auseinander, und Piloten sehen je nach Pfad andere
        // Bounce-Counts.
        //
        // Wenn dieser Test fehlschlaegt: BOUNCE_AGL_THRESHOLD_FT (lib.rs)
        // und BOUNCE_SCORED_MIN_AGL_FT (touchdown_v2.rs) gemeinsam
        // anpassen — KEINE der beiden Konstanten in Isolation aendern.
        assert_eq!(
            crate::grenzwerte::grenzwerte().landung.bounce_agl_threshold_ft.wert as f32,
            crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert,
            "Streamer-Threshold ({} ft) muss gleich v2-SCORED-Threshold ({} ft) sein",
            crate::grenzwerte::grenzwerte().landung.bounce_agl_threshold_ft.wert,
            crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert,
        );
    }

    #[test]
    fn streamer_bounce_return_strictly_below_threshold() {
        // Sanity: nach dem Hopser muss der Flieger wieder TIEFER als
        // RETURN sinken (5 ft). Wenn RETURN >= THRESHOLD waere, koennte
        // der Detector nie firen.
        assert!(crate::grenzwerte::grenzwerte().landung.bounce_agl_return_ft.wert < crate::grenzwerte::grenzwerte().landung.bounce_agl_threshold_ft.wert);
    }
