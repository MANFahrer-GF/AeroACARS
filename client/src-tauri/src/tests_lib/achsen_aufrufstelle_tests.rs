    use super::*;

    /// Eine Spur, die bis `knick_m` gerade läuft und danach scharf
    /// ausschwenkt — genau die Form einer Rollstrecke zur Ausfahrt.
    fn spur_mit_ausfahrt(knick_m: f32, bis_m: f32) -> Vec<(f32, f32)> {
        let mut p = Vec::new();
        let mut x = 800.0f32;
        while x <= bis_m {
            let q = if x <= knick_m {
                // 0,2 m je 100 m ≈ 0,11° — klar unbedenklich.
                (x - 800.0) * 0.002
            } else {
                (knick_m - 800.0) * 0.002 + (x - knick_m) * 0.35
            };
            p.push((x, q));
            x += 10.0;
        }
        p
    }

    fn stats_mit(fenster: Option<f64>, raeum: Option<f64>, kante: Option<f64>) -> FlightStats {
        let mut s = FlightStats::default();
        s.bahn_spur = spur_mit_ausfahrt(1500.0, 2400.0);
        s.bahn_fenster_zu_laengs_m = fenster;
        s.bahn_raeum_laengs_m = raeum;
        s.bahn_kante_laengs_m = kante;
        s
    }

    fn winkel_der_aufrufstelle(s: &FlightStats) -> Option<f64> {
        let mut input = landing_scoring::LandingScoringInput::default();
        fill_v2_rollout_fields(&mut input, s, "EDDM");
        input.bahn_achsen_abweichung_grad
    }

    /// ⚠ Diese Prüfung bewacht die AUFRUFSTELLE, nicht die Funktion.
    ///
    /// Der Fehler, der v1.7.0–v1.7.6 gekostet hat, sass genau hier: Die
    /// Rangfolge stand als Ausdruck in `fill_v2_rollout_fields`
    /// (`kante.or(raeum)`), und keine Prüfung konnte sie fassen. Die
    /// Funktion `achsen_fenster_bis_m` allein zu prüfen reicht nicht —
    /// wer ihr die drei Werte in der falschen Reihenfolge übergibt,
    /// bekommt denselben Fehler zurück, und alles bleibt grün.
    #[test]
    fn die_aufrufstelle_nimmt_das_messfenster_nicht_die_kante() {
        let s = stats_mit(Some(1500.0), Some(1900.0), Some(2400.0));
        let w = winkel_der_aufrufstelle(&s).expect("Winkel muss bestimmbar sein");
        assert!(
            w.abs() < 0.5,
            "Die Gerade lief über das Ausschwenken hinaus: {w:.2}°"
        );
    }

    #[test]
    fn ohne_messfenster_greift_das_ausschwenken() {
        let s = stats_mit(None, Some(1500.0), Some(2400.0));
        let w = winkel_der_aufrufstelle(&s).expect("Winkel muss bestimmbar sein");
        assert!(w.abs() < 0.5, "Ausschwenken nicht benutzt: {w:.2}°");
    }

    /// Die Gegenprobe im Test selbst: Mit NUR der Kante muss der Winkel
    /// gross werden. Sonst prüften die beiden obigen Fälle ins Leere —
    /// etwa weil die Spur gar nicht ausschwenkt.
    #[test]
    fn nur_die_kante_ergibt_den_falschen_grossen_winkel() {
        let s = stats_mit(None, None, Some(2400.0));
        let w = winkel_der_aufrufstelle(&s).expect("Winkel muss bestimmbar sein");
        assert!(
            w.abs() > 5.0,
            "Die Testspur schwenkt nicht aus — die anderen Fälle prüfen dann nichts: {w:.2}°"
        );
    }

    #[test]
    fn die_begleitwerte_kommen_an_der_aufrufstelle_mit() {
        // Ohne sie greift die Manöver-Ausnahme nie, und die Korrektur
        // wäre halb wirkungslos — genau die Sorte stiller Verlust, die
        // uns bei `mess_ende_laengs_m` schon einmal getroffen hat.
        let s = stats_mit(Some(1500.0), Some(1900.0), Some(2400.0));
        let mut input = landing_scoring::LandingScoringInput::default();
        fill_v2_rollout_fields(&mut input, &s, "EDDM");
        assert!(input.bahn_achsen_kreuzt_mitte.is_some());
        assert!(input.bahn_achsen_groesster_betrag_m.is_some());
    }
