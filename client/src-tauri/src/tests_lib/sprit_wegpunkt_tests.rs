    use super::*;

    const BREITE: f64 = 48.0;

    fn lon_nm(nm: f64) -> f64 {
        10.0 + nm / (60.0 * BREITE.to_radians().cos())
    }

    /// Ein Fix auf der Route: Strecke ab Start, Plan-Verbrauch bis hier,
    /// Plan an Bord und Minimum laut OFP.
    fn fx(ident: &str, lat: f64, nm: f64, seg: f32, verbrauch: f32) -> api_client::RouteFix {
        api_client::RouteFix {
            ident: ident.into(),
            lat,
            lon: lon_nm(nm),
            kind: "wpt".into(),
            sprit_bis_hier_kg: Some(verbrauch),
            segment_nm: Some(seg),
            sprit_an_bord_kg: Some(8_000.0 - verbrauch),
            sprit_min_an_bord_kg: Some(8_000.0 - verbrauch - 900.0),
            ..Default::default()
        }
    }

    /// Eine Route nach Osten; KILO liegt 30 NM nördlich der Linie —
    /// geradeaus geflogen ist das ein Direct an KILO vorbei.
    fn stats_mit_route() -> FlightStats {
        let mut st = FlightStats::default();
        st.takeoff_fuel_kg = Some(8_000.0);
        st.takeoff_at = Some(Utc::now());
        st.planned_contingency_kg = Some(150.0);
        st.planned_waypoints = vec![
            // Als FLUGHAFEN gekennzeichnet — dann ist die erste Zeile
            // wirklich der Abflug und bekommt keine eigene davor.
            api_client::RouteFix {
                kind: "apt".into(),
                ..fx("DEP", BREITE, 0.0, 0.0, 0.0)
            },
            fx("ALFA", BREITE, 40.0, 40.0, 400.0),
            fx("BRAVO", BREITE, 80.0, 40.0, 700.0),
            fx("KILO", BREITE + 0.5, 120.0, 50.0, 1_050.0),
            fx("DELTA", BREITE, 160.0, 50.0, 1_400.0),
            fx("ARR", BREITE, 200.0, 40.0, 1_700.0),
        ];
        st
    }

    /// Geradeaus nach Osten fliegen, 5 % über Plan-Verbrauch.
    fn fliegen(st: &mut FlightStats, von: f64, bis: f64) {
        let mut nm = von;
        while nm <= bis {
            st.distance_nm = nm;
            let verbrauch = (nm / 200.0 * 1_700.0 * 1.05) as f32;
            sprit_wegpunkt_tick(
                st,
                BREITE,
                lon_nm(nm),
                8_000.0 - verbrauch,
                false,
                false,
                false,
                false,
                nm as i64,
            );
            nm += 2.0;
        }
    }

    #[test]
    fn ueberflug_wird_gemessen_und_ein_direct_als_uebersprungen_gefuehrt() {
        let mut st = stats_mit_route();
        fliegen(&mut st, 1.0, 195.0);
        let m: Vec<(&str, bool)> = st
            .sprit_wp_messungen
            .iter()
            .map(|m| (m.ident.as_str(), m.uebersprungen))
            .collect();
        assert_eq!(
            m,
            vec![
                ("ALFA", false),
                ("BRAVO", false),
                ("KILO", true),
                ("DELTA", false)
            ],
            "Überflüge falsch erkannt"
        );
        // Gemessen heisst: Tankstand beim Überflug, nicht irgendwann.
        let alfa = st.sprit_wp_messungen[0].ist_kg.unwrap();
        assert!(
            (alfa - (8_000.0 - 40.0 / 200.0 * 1_700.0 * 1.05)).abs() < 25.0,
            "{alfa}"
        );
        assert!(
            st.sprit_wp_messungen[2].ist_kg.is_none(),
            "übersprungen ohne Messwert"
        );

        // Die Tabelle: KILO gerechnet, aber als übersprungen gekennzeichnet.
        use landing_scoring::sprit::{Ampel, WegpunktZustand};
        let z = sprit_wegpunkt_zeilen(&st, Some("EDDL"), false);
        assert_eq!(z.len(), 6);
        assert_eq!(
            z[0].zustand,
            WegpunktZustand::Gemessen,
            "Abflug trägt den Abhebe-Tankstand"
        );
        assert_eq!(z[3].zustand, WegpunktZustand::Uebersprungen);
        assert!(z[3].ist_an_bord_kg.is_some());
        assert_eq!(
            z[5].zustand,
            WegpunktZustand::Offen,
            "vor der Landung ist das Ziel offen"
        );
        // 5 % Mehrverbrauch auf 1.700 kg Trip = 85 kg, Contingency 150: grün.
        assert_eq!(z[4].ampel, Some(Ampel::Gruen));
    }

    #[test]
    fn ohne_flughafen_im_navlog_bekommt_der_abflug_eine_eigene_zeile() {
        // A380 nach EDDM (20.09.2026): SimBrief beginnt das Navlog am
        // ersten Streckenpunkt („BETTE", laut FMS um 11:56 überflogen),
        // nicht am Flughafen. Bis dahin trug diese Zeile trotzdem die
        // Abhebezeit und den Abhebe-Tankstand — verglichen wurde der
        // Tankstand an EINEM Ort mit dem Plan an einem ANDEREN.
        let mut st = stats_mit_route();
        // Navlog ohne Flughafen: der erste Fix ist ein Streckenpunkt.
        st.planned_waypoints.remove(0);
        st.planned_block_fuel_kg = Some(8_300.0);
        st.planned_taxi_kg = Some(300.0);
        st.takeoff_fuel_kg = Some(8_100.0);

        use landing_scoring::sprit::WegpunktZustand;
        let z = sprit_wegpunkt_zeilen(&st, Some("EDDL"), false);
        assert_eq!(z[0].ident, "EDDL", "keine eigene Abflugzeile");
        assert_eq!(
            z[0].plan_an_bord_kg,
            Some(8_000.0),
            "Plan beim Abheben ist Block minus Taxi"
        );
        assert_eq!(z[0].ist_an_bord_kg, Some(8_100.0));
        assert_eq!(
            z[1].ident, "ALFA",
            "der Streckenpunkt bleibt ein Streckenpunkt"
        );
        assert_eq!(
            z[1].zustand,
            WegpunktZustand::Offen,
            "ALFA trägt nicht mehr den Abhebe-Tankstand"
        );
        // Und die Rechnung stimmt: 100 kg mehr getankt als geplant,
        // nicht „100 kg plus der Verbrauch bis ALFA".
        assert_eq!(
            z[0].ist_an_bord_kg
                .zip(z[0].plan_an_bord_kg)
                .map(|(i, p)| i - p),
            Some(100.0)
        );
    }

    #[test]
    fn die_entfernung_trifft_bei_doppelter_kennung_den_richtigen_fix() {
        // Abnahme 20.09.2026, dritte Runde: Die Suche ueber die Kennung
        // verglich einen ZEILEN-Index mit WEGPUNKT-Indizes. Bei zwei
        // gleichen Kennungen direkt hintereinander (Warteschleife) waehlte
        // sie damit den falschen — dieselbe Klasse wie der Fehler, den sie
        // beheben sollte.
        use landing_scoring::sprit::{Wegpunkt, WegpunktZustand};
        // Wegpunkte: ALFA (40 NM), ALFA (60 NM), BRAVO (80 NM).
        let fixes = vec![
            fx("ALFA", BREITE, 40.0, 40.0, 400.0),
            fx("ALFA", BREITE, 60.0, 20.0, 550.0),
            fx("BRAVO", BREITE, 80.0, 20.0, 700.0),
        ];
        // Zeilen mit Abflugzeile vorn: [EDDL, ALFA, ALFA, BRAVO].
        let zeile = |ident: &str| Wegpunkt {
            ident: ident.into(),
            zustand: WegpunktZustand::Offen,
            ..Default::default()
        };
        let zeilen = vec![zeile("EDDL"), zeile("ALFA"), zeile("ALFA"), zeile("BRAVO")];
        // Standort am Abflug. Zeile 1 ist das ERSTE ALFA (40 NM), Zeile 2
        // das zweite (60 NM) — die Entfernungen muessen das abbilden.
        let erste = entfernung_zur_zeile(&zeilen, &fixes, 1, BREITE, lon_nm(0.0))
            .expect("keine Entfernung");
        let zweite = entfernung_zur_zeile(&zeilen, &fixes, 2, BREITE, lon_nm(0.0))
            .expect("keine Entfernung");
        assert!(
            (erste - 40.0).abs() < 1.0,
            "Zeile 1 zeigt nicht auf das erste ALFA ({erste:.1} NM)"
        );
        assert!(
            (zweite - 60.0).abs() < 1.0,
            "Zeile 2 zeigt nicht auf das zweite ALFA ({zweite:.1} NM)"
        );
    }

    #[test]
    fn die_abflugzeile_verschiebt_die_zuordnung_nicht_bei_doppelter_kennung() {
        // Abnahme 20.09.2026, zweite Runde: Der erste Fassung dieses Tests
        // konnte den Fehler nicht rot machen — bei lauter eindeutigen
        // Kennungen rettet der Rueckfall ueber die Kennung die Zuordnung
        // auch ohne Versatz. Erst wenn eine Kennung ZWEIMAL vorkommt,
        // entscheidet die Index-Naehe, und dann trifft ein um eins
        // verschobener Bezug die falsche Zeile.
        let mut st = stats_mit_route();
        st.planned_waypoints.remove(0); // Navlog ohne Flughafen
                                        // ALFA kommt ZWEIMAL DIREKT HINTEREINANDER vor — so sieht eine
                                        // Warteschleife im Navlog aus. Nur in dieser Lage schlaegt der
                                        // fehlende Versatz durch: Der schnelle Weg ueber den Index trifft
                                        // dann eine Zeile mit der RICHTIGEN Kennung, aber der falschen.
        st.planned_waypoints
            .insert(1, fx("ALFA", BREITE, 60.0, 20.0, 550.0));
        st.planned_block_fuel_kg = Some(8_300.0);
        st.planned_taxi_kg = Some(300.0);

        // Eine Messung am ZWEITEN ALFA (Index 1 in den Wegpunkten).
        st.sprit_wp_messungen = vec![SpritWpMessung {
            idx: 1,
            ident: "ALFA".to_string(),
            zeit_ms: 1,
            ist_kg: Some(7_000.0),
            uebersprungen: false,
        }];

        use landing_scoring::sprit::WegpunktZustand;
        let z = sprit_wegpunkt_zeilen(&st, Some("EDDL"), false);
        // Mit Abflugzeile liegt das zweite ALFA in Zeile 2, das erste in 1.
        assert_eq!(
            z[2].ident, "ALFA",
            "Testaufbau: Zeile 2 ist das zweite ALFA"
        );
        assert_eq!(
            z[2].zustand,
            WegpunktZustand::Gemessen,
            "die Messung landete nicht am zweiten ALFA"
        );
        assert_eq!(z[2].ist_an_bord_kg, Some(7_000.0));
        assert_eq!(
            z[1].zustand,
            WegpunktZustand::Offen,
            "die Messung landete faelschlich am ERSTEN ALFA"
        );
    }

    /// Jede Messung findet ihre Zeile, und die Abflugzeile bleibt stehen.
    ///
    /// ⚠ Dieser Test kann den Versatz-Fehler NICHT rot machen: Bei lauter
    /// eindeutigen Kennungen rettet der Rueckfall ueber die Kennung die
    /// Zuordnung auch ohne ihn. Dafuer ist
    /// `die_abflugzeile_verschiebt_die_zuordnung_nicht_bei_doppelter_kennung`
    /// zustaendig (Abnahme 20.09.2026, dritte Runde). Hier geht es nur
    /// darum, dass ueberhaupt jede Messung ankommt.
    #[test]
    fn jede_messung_findet_ihre_zeile() {
        let mut st = stats_mit_route();
        st.planned_waypoints.remove(0); // Navlog ohne Flughafen
        st.planned_block_fuel_kg = Some(8_300.0);
        st.planned_taxi_kg = Some(300.0);
        fliegen(&mut st, 1.0, 95.0);

        let gemessen: Vec<&str> = st
            .sprit_wp_messungen
            .iter()
            .map(|m| m.ident.as_str())
            .collect();
        assert!(!gemessen.is_empty(), "Gegenprobe: es wurde etwas gemessen");

        use landing_scoring::sprit::WegpunktZustand;
        let z = sprit_wegpunkt_zeilen(&st, Some("EDDL"), false);
        for m in &st.sprit_wp_messungen {
            let zeile = z
                .iter()
                .find(|zz| zz.ident == m.ident)
                .expect("Messung ohne Zeile");
            assert_ne!(
                zeile.zustand,
                WegpunktZustand::Offen,
                "die Messung von {} landete nicht in ihrer Zeile",
                m.ident
            );
        }
        // Und die Abflugzeile bleibt der Abflug, nicht eine Messung.
        assert_eq!(z[0].ident, "EDDL");
    }

    #[test]
    fn ohne_plan_zahlen_wird_keine_abflugzeile_erfunden() {
        // Fehlt Block oder Taxi, gibt es keine Zeile — ein geratener
        // Planwert waere schlimmer als eine fehlende Zeile.
        let mut st = stats_mit_route();
        st.planned_waypoints.remove(0);
        st.planned_block_fuel_kg = None;
        st.planned_taxi_kg = None;

        let z = sprit_wegpunkt_zeilen(&st, Some("EDDL"), false);
        assert_eq!(z[0].ident, "ALFA", "es wurde eine Zeile erfunden");
    }

    #[test]
    fn eine_warteschleife_fuehrt_nicht_zurueck() {
        let mut st = stats_mit_route();
        fliegen(&mut st, 1.0, 95.0);
        let vorher = st.sprit_wp_messungen.len();
        // Zurück über ALFA (Holding) — nichts wird doppelt erfasst.
        let mut nm = 95.0;
        while nm > 30.0 {
            st.distance_nm += 2.0;
            sprit_wegpunkt_tick(
                &mut st,
                BREITE,
                lon_nm(nm),
                7_000.0,
                false,
                false,
                false,
                false,
                0,
            );
            nm -= 2.0;
        }
        assert_eq!(st.sprit_wp_messungen.len(), vorher);
    }

    #[test]
    fn am_boden_und_im_slew_wird_nichts_erfasst() {
        let mut st = stats_mit_route();
        sprit_wegpunkt_tick(
            &mut st,
            BREITE,
            lon_nm(1.0),
            8_000.0,
            false,
            false,
            false,
            false,
            0,
        );
        sprit_wegpunkt_tick(
            &mut st,
            BREITE,
            lon_nm(41.0),
            7_500.0,
            false,
            true,
            false,
            false,
            0,
        );
        sprit_wegpunkt_tick(
            &mut st,
            BREITE,
            lon_nm(81.0),
            7_200.0,
            false,
            false,
            false,
            true,
            0,
        );
        assert!(st.sprit_wp_messungen.is_empty());
    }

    #[test]
    fn einstieg_in_der_luft_misst_den_abflug_nicht() {
        let mut st = stats_mit_route();
        st.sprit_einstieg_in_der_luft = true;
        st.takeoff_fuel_kg = Some(7_300.0);
        // Bis hinter DELTA (160 NM) — sonst ist die geprüfte Zeile noch
        // gar nicht überflogen, und der Test misst nur sich selbst.
        fliegen(&mut st, 70.0, 165.0);
        use landing_scoring::sprit::WegpunktZustand;
        let z = sprit_wegpunkt_zeilen(&st, Some("EDDL"), false);
        assert_eq!(
            z[0].zustand,
            WegpunktZustand::Offen,
            "Einstiegs-Tankstand als Abflug gemessen"
        );
        assert!(z[0].ist_an_bord_kg.is_none());
        // Bezug ist BRAVO, der erste Überflug nach dem Einstieg. Seit dem
        // Wegfall der Hochskalierung (20.09.2026) braucht die Rechnung
        // keinen Anlauf mehr: Tankstand dort minus dem geplanten Rest ist
        // ab der ersten Messung eine gültige Aussage. Vorher blieb die
        // Zeile farblos, weil ein Verhältnis aus einem winzigen Planstück
        // Rauschen gewesen wäre.
        assert!(
            z[2].ampel.is_some(),
            "auch der erste Überflug bekommt seine Hochrechnung",
        );
        assert_eq!(
            z[4].zustand,
            WegpunktZustand::Gemessen,
            "DELTA wurde überflogen"
        );
        assert!(
            z[4].ampel.is_some(),
            "nach genug Strecke gehört die Ampel hin"
        );
    }

    #[test]
    fn eine_neue_route_setzt_den_abschnitt_neu_an() {
        // Route-Sync im Reiseflug: Die neue Route hat vorn zwei Fixe mehr.
        // Mit dem alten Index galten ALFA..BRAVO der neuen Route als
        // übersprungen, obwohl das Flugzeug dort nie war.
        let mut st = stats_mit_route();
        fliegen(&mut st, 1.0, 50.0);
        assert_eq!(st.sprit_wp_segment, Some(1));
        let vorher = st.sprit_wp_messungen.len();
        let mut neu = vec![
            fx("DEP", BREITE, 0.0, 0.0, 0.0),
            fx("XRAY", BREITE, 10.0, 10.0, 100.0),
            fx("YANK", BREITE, 20.0, 10.0, 200.0),
        ];
        neu.extend(st.planned_waypoints[1..].iter().cloned());
        st.planned_waypoints = neu;
        sprit_plan_neu_lesen(&mut st);
        assert_eq!(
            st.sprit_wp_segment, None,
            "Index der alten Route bleibt stehen"
        );
        fliegen(&mut st, 52.0, 90.0);
        let dazu: Vec<(&str, bool)> = st.sprit_wp_messungen[vorher..]
            .iter()
            .map(|m| (m.ident.as_str(), m.uebersprungen))
            .collect();
        assert_eq!(
            dazu,
            vec![("BRAVO", false)],
            "nach dem Routenwechsel falsch erfasst"
        );
    }

    #[test]
    fn nach_der_landung_friert_die_tabelle_mit_der_auswertung_ein() {
        let mut st = stats_mit_route();
        fliegen(&mut st, 1.0, 199.0);
        st.landing_fuel_kg = Some(6_200.0);
        let a = sprit_auswertung_einmal(&mut st).expect("Auswertung");
        assert_eq!(a.wegpunkte.len(), 6, "die Tabelle reist mit der Auswertung");
        assert_eq!(a.wegpunkte[5].ist_an_bord_kg, Some(6_200.0));
    }

    #[test]
    fn der_takt_ruft_die_erfassung_auf() {
        // Verdrahtung, nicht die Funktion: Ohne den Aufruf im Takt bleibt
        // die Tabelle leer, und alle Tests oben wären grün.
        let quelle = include_str!("../lib.rs");
        let takt = quelle
            .find("    sprit_tick(\n        &mut stats,")
            .expect("Aufruf von sprit_tick im Takt nicht gefunden");
        let rest = &quelle[takt..];
        let wp = rest.find("sprit_wegpunkt_tick(\n        &mut stats,");
        assert!(
            wp.is_some_and(|i| i < 1_500),
            "sprit_wegpunkt_tick wird im Takt nicht direkt nach sprit_tick gerufen"
        );
    }
