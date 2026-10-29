    use super::*;
    use crate::msfs_agl_flare_tests::tw;

    /// Der echte Verlauf aus dem Flugprotokoll von THY 42 (A350-900,
    /// FAOR→FALE, Landung FALE 24 am 30.08.2026, 08:04:02Z).
    ///
    /// ⚠ Die Werte sind ABGELESEN, nicht erfunden: Bodenhoehe 17,9 ft,
    /// zwei Kontaktwechsel von 160 ms und 40 ms, Hoehenaenderung dabei
    /// 0,01 ft. Aufgesetzt mit 171 fpm und 1,16 G.
    fn thy42_verlauf(base: DateTime<Utc>) -> Vec<TouchdownWindowSample> {
        vec![
            // Anflug, noch in der Luft
            tw(base, 0, 43.38, 4.0, false, -566.0),
            tw(base, 2000, 25.0, 5.0, false, -400.0),
            // Aufsetzen — ab hier ist 17,9 ft der BODEN
            tw(base, 3900, 17.91, 5.5, true, -278.9),
            tw(base, 4500, 17.88, 5.0, true, -120.0),
            // "Sprung" 1: 160 ms, Hoehe unveraendert
            tw(base, 5100, 17.83, 4.8, false, -85.7),
            tw(base, 5260, 17.84, 4.7, true, -79.2),
            // "Sprung" 2: 40 ms
            tw(base, 5320, 17.83, 4.6, false, -77.1),
            tw(base, 5360, 17.80, 4.5, true, -75.4),
            tw(base, 6000, 17.79, 4.0, true, -40.0),
            tw(base, 8000, 17.78, 2.0, true, 0.0),
        ]
    }

    fn zaehle(samples: &[TouchdownWindowSample], base: DateTime<Utc>) -> (u64, u64) {
        let ana = compute_landing_analysis(
            samples,
            base + chrono::Duration::milliseconds(3900),
            Simulator::Msfs2024,
            None,
        );
        (
            ana["forensic_bounce_count"].as_u64().unwrap_or(999),
            ana["scored_bounce_count"].as_u64().unwrap_or(999),
        )
    }

    #[test]
    fn ein_flatternder_bodenkontakt_ist_kein_sprung() {
        // ⚠ Vorher: 2 gewertete Spruenge → 40 statt 100 Punkte auf der
        // Achse. Ursache: Die Sprunghoehe wurde ABSOLUT genommen, und
        // eine A350 meldet am Fahrwerk stehend 17,8 ft — ueber der
        // gewerteten Schwelle von 15 ft.
        let base = Utc::now();
        let (forensisch, gewertet) = zaehle(&thy42_verlauf(base), base);
        assert_eq!(gewertet, 0, "ein 40-ms-Flattern wurde als Sprung gewertet");
        assert_eq!(forensisch, 0, "auch forensisch ist das kein Sprung");
    }

    #[test]
    fn ein_echter_sprung_wird_weiterhin_gezaehlt() {
        // Gegenprobe: DASSELBE Muster mit derselben Bodenhoehe, aber das
        // Flugzeug steigt wirklich — 20 ft ueber den Boden, ueber eine
        // Sekunde lang. Das MUSS zaehlen, sonst hat der Riegel die
        // Erkennung abgeschaltet statt sie zu schaerfen.
        let base = Utc::now();
        let mut s = thy42_verlauf(base);
        s[4] = tw(base, 5100, 17.83 + 8.0, 4.8, false, 200.0);
        s[5] = tw(base, 5600, 17.83 + 20.0, 4.7, false, 100.0);
        s[6] = tw(base, 6100, 17.83 + 9.0, 4.6, false, -300.0);
        s[7] = tw(base, 6600, 17.90, 4.5, true, -400.0);
        let (forensisch, gewertet) = zaehle(&s, base);
        assert_eq!(gewertet, 1, "ein echter 20-ft-Sprung wurde verschluckt");
        assert_eq!(forensisch, 1);
    }

    #[test]
    fn nur_die_bezugshoehe_rettet_hier() {
        // ⚠ ISOLIERT den einen Riegel. Ein langes Flattern (800 ms, ueber
        // der Mindestdauer) bei unveraenderter Hoehe: Hier haelt NUR die
        // Umrechnung auf die Bodenhoehe.
        //
        // Ohne diesen Fall deckten sich die beiden Riegel gegenseitig —
        // die Gegenprobe blieb gruen, obwohl ich einen entfernt hatte.
        let base = Utc::now();
        let s = vec![
            tw(base, 0, 40.0, 4.0, false, -500.0),
            tw(base, 1000, 17.90, 5.0, true, -200.0),
            tw(base, 2000, 17.88, 4.8, false, -50.0),
            tw(base, 2400, 17.87, 4.7, false, -40.0),
            tw(base, 2800, 17.86, 4.6, false, -30.0),
            tw(base, 3000, 17.85, 4.5, true, -20.0),
        ];
        let ana = compute_landing_analysis(
            &s,
            base + chrono::Duration::milliseconds(1000),
            Simulator::Msfs2024,
            None,
        );
        assert_eq!(
            ana["scored_bounce_count"].as_u64().unwrap_or(999),
            0,
            "ein 1000-ms-Flattern auf Bodenhoehe wurde als Sprung gewertet"
        );
    }

    #[test]
    fn nur_die_mindestdauer_rettet_hier() {
        // ⚠ ISOLIERT den anderen Riegel. Ein kurzer Ausschlag, der die
        // Hoehenschwelle REISST (20 ft ueber Boden), aber nur 80 ms
        // dauert — physikalisch unmoeglich, also ein Datenfehler.
        // Hier haelt NUR die Mindestdauer.
        let base = Utc::now();
        let s = vec![
            tw(base, 0, 40.0, 4.0, false, -500.0),
            tw(base, 1000, 17.90, 5.0, true, -200.0),
            tw(base, 2000, 17.90 + 20.0, 4.8, false, 0.0),
            tw(base, 2080, 17.90, 4.7, true, 0.0),
        ];
        let ana = compute_landing_analysis(
            &s,
            base + chrono::Duration::milliseconds(1000),
            Simulator::Msfs2024,
            None,
        );
        assert_eq!(
            ana["scored_bounce_count"].as_u64().unwrap_or(999),
            0,
            "ein 80-ms-Ausschlag auf 20 ft wurde als Sprung gewertet — \
             ein Flugzeug steigt nicht in 80 ms sechs Meter"
        );
    }

    #[test]
    fn die_bodenhoehe_wird_vom_muster_genommen_nicht_geraten() {
        // Bei einem Muster mit niedrigem Modellursprung (Boden ~1 ft)
        // muss derselbe echte Sprung ebenso zaehlen — die Erkennung darf
        // nicht an einer festen Bodenhoehe haengen.
        let base = Utc::now();
        let s = vec![
            tw(base, 0, 30.0, 4.0, false, -400.0),
            tw(base, 1000, 1.2, 5.0, true, -200.0),
            tw(base, 1500, 19.0, 5.0, false, 300.0),
            tw(base, 2200, 21.0, 5.0, false, 0.0),
            tw(base, 3000, 1.3, 4.0, true, -300.0),
        ];
        let ana = compute_landing_analysis(
            &s,
            base + chrono::Duration::milliseconds(1000),
            Simulator::Msfs2024,
            None,
        );
        assert_eq!(
            ana["scored_bounce_count"].as_u64().unwrap_or(999),
            1,
            "ein 20-ft-Sprung bei niedrigem Modellursprung wurde verschluckt"
        );
    }
    // ─── P1/8b leichter Hopser (Thomas 22.10.2026) ─────────────────────

    /// (normal, leicht) aus dem 50-Hz-Pfad.
    fn zaehle_beide(samples: &[TouchdownWindowSample], base: DateTime<Utc>, edge_ms: i64) -> (u64, u64) {
        let ana = compute_landing_analysis(
            samples,
            base + chrono::Duration::milliseconds(edge_ms),
            Simulator::Msfs2024,
            None,
        );
        (
            ana["scored_bounce_count"].as_u64().unwrap_or(999),
            ana["scored_light_bounce_count"].as_u64().unwrap_or(999),
        )
    }

    /// LNK 861 FASZ (04.10.2026, yp5VXAZd0JJz3KYR), Werte aus der Messung:
    /// Bodenhoehe 9,88 ft, Luftphase ab 0,59 s fuer 2735 ms, Gipfel 4,90 ft
    /// ueber Boden. Heute 0 Hopser — mit der Regel genau 1 leichter.
    /// Auf dem alten Stand fehlte `scored_light_bounce_count` (→ 999, rot).
    #[test]
    fn lnk861_ist_ein_leichter_hopser() {
        let base = Utc::now();
        let b = 9.88_f32;
        let s = vec![
            tw(base, 0, b + 30.0, 4.0, false, -736.0),
            tw(base, 1000, b, 5.0, true, -736.0),
            tw(base, 1590, b + 0.4, 5.0, false, 85.0),
            tw(base, 2500, b + 4.90, 4.5, false, 0.0),
            tw(base, 3800, b + 1.0, 4.0, false, -300.0),
            tw(base, 4325, b + 0.1, 4.0, true, -378.0),
            tw(base, 9000, b - 1.0, 1.0, true, 0.0),
        ];
        assert_eq!(zaehle_beide(&s, base, 1000), (0, 1));
    }

    /// THY42: zwei Flatterphasen von 160 und 40 ms bei 0 ft — weder normal
    /// noch leicht.
    #[test]
    fn thy42_bleibt_ohne_leichten_hopser() {
        let base = Utc::now();
        assert_eq!(zaehle_beide(&thy42_verlauf(base), base, 3900), (0, 0));
    }

    /// AFR1010 (pr2LDBwPGQ0LK2LQ, „td 1492“): 7088 ms ohne Bodenkontakt bei
    /// 0,05 ft — Flattern, kein Sprung. Die Dauer allein darf nicht zaehlen.
    #[test]
    fn afr1010_langes_flattern_ist_kein_leichter_hopser() {
        let base = Utc::now();
        let b = 11.0_f32;
        let s = vec![
            tw(base, 0, b + 25.0, 4.0, false, -300.0),
            tw(base, 1000, b, 5.0, true, -250.0),
            tw(base, 1530, b + 0.05, 5.0, false, -20.0),
            tw(base, 5000, b + 0.03, 4.0, false, -10.0),
            tw(base, 8618, b + 0.02, 3.0, true, 0.0),
        ];
        assert_eq!(zaehle_beide(&s, base, 1000), (0, 0));
    }

    /// Touch-and-go / Durchstarten: eine lange, hohe Luftphase OHNE zweiten
    /// Kontakt zaehlt nicht. Gegenprobe: dieselbe Phase mit Kontakt zaehlt.
    #[test]
    fn durchstart_ohne_zweiten_kontakt_ist_kein_leichter_hopser() {
        let base = Utc::now();
        let b = 10.0_f32;
        let mut s = vec![
            tw(base, 0, b + 30.0, 4.0, false, -400.0),
            tw(base, 1000, b, 5.0, true, -300.0),
            tw(base, 1600, b + 1.0, 6.0, false, 200.0),
            tw(base, 2800, b + 3.5, 7.0, false, 400.0),
        ];
        assert_eq!(zaehle_beide(&s, base, 1000), (0, 0), "Durchstart gezaehlt");
        s.push(tw(base, 3000, b + 0.2, 5.0, true, -200.0));
        assert_eq!(zaehle_beide(&s, base, 1000), (0, 1), "Gegenprobe mit Kontakt");
    }

    /// Ein normaler Hopser (>= 5 ft) wird nie zusaetzlich leicht gezaehlt.
    #[test]
    fn normaler_hopser_ist_nicht_auch_leicht() {
        let base = Utc::now();
        let b = 10.0_f32;
        let s = vec![
            tw(base, 0, b + 30.0, 4.0, false, -400.0),
            tw(base, 1000, b, 5.0, true, -600.0),
            tw(base, 1500, b + 3.0, 5.0, false, 200.0),
            tw(base, 2300, b + 6.0, 5.0, false, 0.0),
            tw(base, 3200, b + 0.1, 4.0, true, -300.0),
        ];
        assert_eq!(zaehle_beide(&s, base, 1000), (1, 0));
    }

    /// Nachrechnung am Bestand: alle 1738 Landungen der Messung vom
    /// 22.10.2026 (`handoff/2026-10-22-hopser-messung.md`). Die Fixture
    /// traegt je Landung die echten Luftphasen >= 300 ms (Beginn, Dauer,
    /// Hoehe ueber Boden, mit/ohne Kontakt) aus den 30/50-Hz-Fenstern; der
    /// Test baut daraus Probenfolgen und schickt sie durch die ECHTE
    /// `compute_landing_analysis`. Erwartet: heute 18 Landungen mit
    /// Hopser, mit leichtem Hopser 62 (+44). Einschraenkung: die Proben
    /// zwischen den Phasen sind nachgebaut (Bodenkontakt), nicht roh.
    #[test]
    fn bestand_nachgerechnet_62_landungen_mit_hopser() {
        let f: serde_json::Value =
            serde_json::from_str(include_str!("../../tests/fixtures/hopser_bestand.json")).unwrap();
        let base = Utc::now();
        let b = 10.0_f32;
        let (mut mit_normal, mut mit_irgendeinem) = (0, 0);
        for landung in f["phasen"].as_array().unwrap() {
            let mut s = vec![
                tw(base, 0, b + 30.0, 4.0, false, -400.0),
                tw(base, 1000, b, 5.0, true, -300.0),
            ];
            for p in landung[1].as_array().unwrap() {
                let ofs = p[0].as_i64().unwrap();
                let dauer = p[1].as_i64().unwrap();
                let max = p[2].as_f64().unwrap() as f32;
                let kontakt = p[3].as_bool().unwrap();
                let t0 = 1000 + ofs.max(1);
                if s.last().unwrap().at < base + chrono::Duration::milliseconds(t0 - 5) {
                    s.push(tw(base, t0 - 5, b, 5.0, true, -50.0));
                }
                s.push(tw(base, t0, b + max * 0.3, 5.0, false, 50.0));
                s.push(tw(base, t0 + dauer / 2, b + max, 5.0, false, 0.0));
                s.push(tw(base, t0 + dauer - 1, b + max * 0.2, 5.0, false, -100.0));
                if kontakt {
                    s.push(tw(base, t0 + dauer, b, 5.0, true, -100.0));
                } else {
                    break;
                }
            }
            let (normal, leicht) = zaehle_beide(&s, base, 1000);
            mit_normal += (normal > 0) as u32;
            mit_irgendeinem += (normal > 0 || leicht > 0) as u32;
        }
        assert_eq!(f["landungen_gesamt"].as_u64(), Some(1738));
        assert_eq!(mit_normal, 18, "heutige Regel am Bestand");
        assert_eq!(mit_irgendeinem, 62, "mit leichtem Hopser am Bestand");
    }

    /// Der 1-Hz-Streamer (Vorwert) nutzt dieselben Grenzen.
    #[test]
    fn streamer_regel_leichter_hopser() {
        assert!(leichter_hopser_im_streamer(2735, 4.9));
        assert!(!leichter_hopser_im_streamer(999, 4.9), "unter 1 s");
        assert!(!leichter_hopser_im_streamer(7088, 0.05), "Flattern");
        assert!(!leichter_hopser_im_streamer(2000, 6.0), "normaler Hopser");
        assert!(leichter_hopser_im_streamer(1000, 2.0), "Grenze eingeschlossen");
        // Codex-QS 22.10.2026: genau 5 ft ist wie in der 50-Hz-Auswertung kein leichter Hopser.
        assert!(!leichter_hopser_im_streamer(2000, 5.0), "5 ft gehört nicht mehr zum leichten Hopser");
        assert_eq!(crate::grenzwerte::grenzwerte().landung.bounce_light_min_agl_ft.wert, 2.0);
        assert_eq!(crate::grenzwerte::grenzwerte().landung.bounce_light_min_dauer_ms.wert, 1000);
        assert_eq!(crate::grenzwerte::grenzwerte().landung.bounce_agl_threshold_ft.wert as f32, crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert);
    }
