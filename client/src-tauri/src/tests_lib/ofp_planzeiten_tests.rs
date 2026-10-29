    use super::*;

    /// Die Erstellzeit des OFP kommt mit — sonst fehlt „Plan aus dem OFP
    /// vom …", und die Pruefung, ob der Plan zum Flug passt, hat im
    /// Bericht keinen Hinweis, woher der Plan stammt (14.10.2026).
    #[test]
    fn die_erstellzeit_des_ofp_kommt_mit() {
        let ofp = api_client::SimBriefOfp {
            ofp_generated_at: " 1791900000 ".into(),
            sched_out_epoch: Some(1_791_902_400),
            ..Default::default()
        };
        let z = ofp_planzeiten(&ofp);
        assert_eq!(z.ofp_erstellt, Some(1_791_900_000));
        assert_eq!(z.plan_off_block, Some(1_791_902_400));
        assert_eq!(
            ofp_planzeiten(&api_client::SimBriefOfp::default()).ofp_erstellt,
            None
        );
    }

    /// Wann der wiederhergestellte Flug den OFP nachlaedt (2.0.16).
    ///
    /// AIB 431 (13.10.2026): mit 2.0.14 gestartet, mitten im Flug auf
    /// 2.0.15 aktualisiert — der fortgesetzte Flug hatte keine Planzeiten,
    /// das Cockpit zeigte nichts. Gebaut wie der echte Weg: ueber den
    /// Speicherstand (`PersistedFlightStats`), Direct-Start ohne
    /// Bid-Kennung (`simbrief_ofp_id` leer).
    #[test]
    fn ein_fortgesetzter_flug_ohne_planzeiten_laedt_den_ofp_nach() {
        let fortgesetzt = |vorher: FlightStats| {
            let gespeichert = PersistedFlightStats::snapshot_from(&vorher);
            let mut st = FlightStats::new();
            gespeichert.apply_to(&mut st);
            st
        };
        let mut vor_dem_update = FlightStats::new();
        vor_dem_update.flight_plan_source = Some("simbrief");
        vor_dem_update.simbrief_ofp_generated_at = Some("1791906600".into());
        let mut st = fortgesetzt(vor_dem_update);
        assert!(
            ofp_nachladen_noetig(&st),
            "SimBrief-Flug ohne Planzeiten nach dem Fortsetzen"
        );

        // Nachgeladen — auch ein OFP ganz ohne Planzeiten gilt dann als
        // erledigt, sonst luede jeder Neustart erneut (Codex P2).
        let ohne_zeiten = api_client::SimBriefOfp {
            ofp_generated_at: "1791906600".into(),
            ..Default::default()
        };
        apply_route_only_to_stats(&mut st, &ohne_zeiten);
        let st = fortgesetzt(st);
        assert!(
            !ofp_nachladen_noetig(&st),
            "schon nachgeladen — nicht bei jedem Neustart erneut"
        );

        // Auch wenn der nachgeladene OFP nicht einmal eine Erstellzeit
        // traegt (Codex P2): die vom Flugstart gilt als Merker.
        let mut ganz_leer = FlightStats::new();
        ganz_leer.simbrief_ofp_generated_at = Some("1791906600".into());
        apply_route_only_to_stats(&mut ganz_leer, &api_client::SimBriefOfp::default());
        assert!(!ofp_nachladen_noetig(&fortgesetzt(ganz_leer)));

        // Ohne OFP (manueller Plan) gibt es nichts zu holen.
        let mut manuell = FlightStats::new();
        manuell.flight_plan_source = Some("manual");
        assert!(!ofp_nachladen_noetig(&fortgesetzt(manuell)));

        // Der bisherige Fall bleibt: Wegpunkte ohne Sprit-Planwerte.
        let mut alt = FlightStats::new();
        alt.planned_waypoints = vec![
            api_client::RouteFix::default(),
            api_client::RouteFix::default(),
        ];
        assert!(ofp_nachladen_noetig(&alt), "Sprit-Planwerte fehlen");
    }

    /// Das Nachladen traegt die Planzeiten nach — ersetzt einen
    /// vorhandenen Plan aber nicht.
    #[test]
    fn das_routen_nachladen_traegt_planzeiten_nach_ersetzt_sie_aber_nicht() {
        let ofp = api_client::SimBriefOfp {
            sched_out_epoch: Some(1_791_909_000),
            sched_in_epoch: Some(1_791_920_000),
            ofp_generated_at: "1791906600".into(),
            ..Default::default()
        };
        let mut st = FlightStats::new();
        apply_route_only_to_stats(&mut st, &ofp);
        assert_eq!(st.ofp_planzeiten.plan_off_block, Some(1_791_909_000));
        assert_eq!(st.ofp_planzeiten.ofp_erstellt, Some(1_791_906_600));

        let mut mit_plan = FlightStats::new();
        mit_plan.ofp_planzeiten.plan_off_block = Some(1);
        apply_route_only_to_stats(&mut mit_plan, &ofp);
        assert_eq!(mit_plan.ofp_planzeiten.plan_off_block, Some(1));
    }
