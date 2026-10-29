    use super::*;

    /// Kette Plugin-Paket (JSON mit `null`) → Uebernahme → VALIDATED-Werte.
    fn aufsetzen(json: &str) -> sim_xplane::PremiumTouchdown {
        serde_json::from_str(json).expect("Aufsetzpaket")
    }

    fn kandidat() -> FlightStats {
        FlightStats::new()
    }

    /// Cloud-QS N1: erste Meldung ohne Sinkrate (`null`), aber mit g. Das
    /// Hopser-Echo im Fenster haengt sich NICHT an und ueberschreibt das g
    /// der ersten Meldung nicht; die Sinkrate kommt vom Sampler.
    #[test]
    fn hopser_echo_ueberschreibt_die_erste_meldung_nicht() {
        let mut st = kandidat();
        let t0 = Utc::now();
        let erste = aufsetzen(r#"{"captured_vs_fpm":null,"captured_g_normal":1.42}"#);
        assert_eq!(
            premium_td_uebernehmen(
                &mut st,
                erste.captured_vs_fpm,
                erste.captured_g_normal,
                true,
                t0
            ),
            PremiumTd::NeuerKandidat
        );
        assert_eq!(st.pending_td_premium_vs, None);
        let echo = aufsetzen(r#"{"captured_vs_fpm":-95.0,"captured_g_normal":1.05}"#);
        assert_eq!(
            premium_td_uebernehmen(
                &mut st,
                echo.captured_vs_fpm,
                echo.captured_g_normal,
                true,
                t0 + chrono::Duration::milliseconds(400)
            ),
            PremiumTd::Wiederholt
        );
        let (vs, g) = td_werte_mit_premium(&st, -310.0, 1.2);
        assert_eq!(vs, -310.0, "Sinkrate vom Sampler, nicht vom Echo");
        assert_eq!(g, 1.42, "g der ersten Meldung bleibt");
    }

    /// RREF-Edge zuerst, Plugin einen Tick spaeter: angehaengt; gemessene
    /// Werte gewinnen, `null` faellt auf den Sampler zurueck — nie 0.
    #[test]
    fn plugin_nach_rref_edge_und_null_kette() {
        let mut st = kandidat();
        let t0 = Utc::now();
        st.pending_td_at = Some(t0);
        let td = aufsetzen(r#"{"captured_vs_fpm":-212.5,"captured_g_normal":null}"#);
        assert_eq!(
            premium_td_uebernehmen(
                &mut st,
                td.captured_vs_fpm,
                td.captured_g_normal,
                true,
                t0 + chrono::Duration::milliseconds(20)
            ),
            PremiumTd::Angehaengt { pending_age_ms: 20 }
        );
        assert_eq!(st.pending_td_at, Some(t0));
        assert_eq!(td_werte_mit_premium(&st, -260.0, 1.31), (-212.5, 1.31));
        // Beide `null`: der Sampler traegt alles.
        let mut st = kandidat();
        let td = aufsetzen(r#"{"captured_vs_fpm":null,"captured_g_normal":null}"#);
        premium_td_uebernehmen(&mut st, td.captured_vs_fpm, td.captured_g_normal, true, t0);
        assert_eq!(td_werte_mit_premium(&st, -260.0, 1.31), (-260.0, 1.31));
    }

    #[test]
    fn ausserhalb_der_phase_verworfen() {
        let mut st = kandidat();
        let t0 = Utc::now();
        assert_eq!(
            premium_td_uebernehmen(&mut st, -200.0, 1.2, false, t0),
            PremiumTd::Verworfen
        );
        assert!(st.pending_td_at.is_none() && !st.pending_td_premium_gesehen);
        st.sampler_touchdown_at = Some(t0);
        assert_eq!(
            premium_td_uebernehmen(&mut st, -200.0, 1.2, true, t0),
            PremiumTd::Verworfen
        );
    }
