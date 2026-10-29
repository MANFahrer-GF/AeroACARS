    use super::*;

    /// „bis Sinkflug" bleibt immer in Prozent — auch über 60 %. Die
    /// Kilogramm-Regel gilt NUR im Anflug, sonst zeigte die Webseite der GA-
    /// Flotte Kilogramm und die drei anderen Oberflächen Prozent (QS-Abnahme).
    #[test]
    fn pirep_felder_sprit_hauptzahl_nur_im_anflug_in_kg() {
        let a = landing_scoring::sprit::auswerten(&landing_scoring::sprit::SpritEingang {
            planned_burn_kg: Some(400.0),
            plan_bis_vergleichspunkt_kg: Some(130.0),
            takeoff_fuel_kg: Some(900.0),
            sprit_bei_vergleichspunkt_kg: Some(680.0), // 220 gegen Plan 130 = +69,2 %
            landing_fuel_kg: Some(400.0),
            tank_plausibel: true,
            ..Default::default()
        });
        let p = a.bis_sinkflug.as_ref().expect("Phase");
        assert!(
            p.abweichung_pct > 60.0,
            "Vorbedingung: {}",
            p.abweichung_pct
        );
        let mut f = HashMap::new();
        sprit_pirep_felder(Some(&a), &mut f);
        assert!(
            f["Sprit bis Sinkflug"].starts_with("+69,2 %"),
            "bis Sinkflug muss in Prozent bleiben: {}",
            f["Sprit bis Sinkflug"]
        );
    }

    /// v1.7.38 (Flug #1417): Die PIREP-Felder sagen, was wirklich war —
    /// angebrochen, unberuehrt, keine geplant, kein Extra, ganze Minuten.
    #[test]
    fn pirep_felder_sprit_contingency_extra_minuten() {
        let basis = || landing_scoring::sprit::SpritEingang {
            planned_burn_kg: Some(2_115.0),
            planned_taxi_kg: Some(227.0),
            planned_contingency_kg: Some(238.0),
            planned_alternate_kg: Some(1_927.0),
            planned_reserve_kg: Some(1_130.0),
            planned_extra_kg: Some(0.0),
            planned_block_fuel_kg: Some(5_637.0),
            engine_start_fuel_kg: Some(5_628.0),
            takeoff_fuel_kg: Some(5_444.0),
            landing_fuel_kg: Some(3_209.0),
            zeit_unter_schwelle_s: Some(858.0),
            schwelle_ft: Some(8_362.0),
            tank_plausibel: true,
            ..Default::default()
        };
        let felder = |e: landing_scoring::sprit::SpritEingang| {
            let a = landing_scoring::sprit::auswerten(&e);
            let mut f = HashMap::new();
            sprit_pirep_felder(Some(&a), &mut f);
            f
        };
        let f = felder(basis());
        assert_eq!(f["Contingency"], "angebrochen — 86 von 238 kg genutzt");
        assert_eq!(f["Extra Fuel"], "kein Extra getankt");
        assert_eq!(f["Zeit unter Schwelle"], "14 min unter 8\u{202f}362 ft");
        let mut e = basis();
        e.landing_fuel_kg = Some(3_400.0);
        assert_eq!(felder(e)["Contingency"], "unberührt");
        let mut e = basis();
        e.planned_contingency_kg = Some(0.0);
        e.planned_block_fuel_kg = Some(5_399.0);
        assert_eq!(felder(e)["Contingency"], "keine geplant");
        // Gemessene Kurzzeit: „< 1 min", wie in der Anzeige.
        let mut e = basis();
        e.zeit_unter_schwelle_s = Some(18.0);
        assert_eq!(
            felder(e)["Zeit unter Schwelle"],
            "< 1 min unter 8\u{202f}362 ft"
        );
        // Tank unplausibel: keine Aussage — kein Feld.
        let mut e = basis();
        e.tank_plausibel = false;
        assert!(!felder(e).contains_key("Contingency"));
    }

    #[test]
    fn pirep_felder_sprit_schreibt_die_fertigen_werte() {
        let a = landing_scoring::sprit::auswerten(&landing_scoring::sprit::SpritEingang {
            planned_burn_kg: Some(21_218.0),
            plan_bis_vergleichspunkt_kg: Some(19_353.0),
            takeoff_fuel_kg: Some(40_919.0),
            sprit_bei_vergleichspunkt_kg: Some(22_231.0),
            landing_fuel_kg: Some(16_770.0),
            zeit_unter_schwelle_s: Some(948.0),
            schwelle_ft: Some(9_487.0),
            planned_contingency_kg: Some(1_061.0),
            planned_alternate_kg: Some(8_273.0),
            planned_reserve_kg: Some(4_916.0),
            planned_extra_kg: Some(5_178.0),
            planned_block_fuel_kg: Some(41_644.0),
            // Wie im echten Flug: Plan-Taxi und Tank beim Anlassen. Ohne sie
            // stuenden die 998 kg Taxi als „Zusatzsprit" in der Leiter.
            planned_taxi_kg: Some(998.0),
            engine_start_fuel_kg: Some(41_644.0),
            tank_plausibel: true,
            ..Default::default()
        });
        let mut f = HashMap::new();
        sprit_pirep_felder(Some(&a), &mut f);
        // Komma, schmales Leerzeichen, echtes Minuszeichen — Zeichen fuer
        // Zeichen wie Client, Bericht und Live-Uebersicht.
        assert_eq!(
            f["Sprit bis Sinkflug"],
            "\u{2212}3,4 % (18\u{202f}688 kg / Plan 19\u{202f}353 kg)"
        );
        // Ueber 60 %: Kilogramm, wie in allen anderen Oberflaechen.
        assert_eq!(
            f["Sprit Anflug"],
            "+3\u{202f}596 kg (5\u{202f}461 kg / Plan 1\u{202f}865 kg)"
        );
        assert_eq!(f["Zeit unter Schwelle"], "16 min unter 9\u{202f}487 ft");
        // DLH 370: Contingency ganz verbraucht (1 061 kg).
        assert_eq!(
            f["Contingency"],
            "aufgebraucht — alle 1\u{202f}061 kg genutzt"
        );
        assert_eq!(
            f["Final Reserve"],
            "intakt — 16\u{202f}770 kg gelandet = 4\u{202f}916 kg Final Reserve + 11\u{202f}854 kg darüber"
        );
        assert_eq!(
            f["Extra Fuel"],
            "5\u{202f}178 kg getankt · 1\u{202f}597 kg genutzt · 3\u{202f}581 kg ungenutzt"
        );
        let mut leer = HashMap::new();
        sprit_pirep_felder(None, &mut leer);
        assert!(leer.is_empty());
    }
