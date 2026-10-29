    use super::*;

    fn ort(plan_bis_s: Option<f64>) -> SpritPlanOrt {
        SpritPlanOrt { plan_bis_kg: 0.0, plan_bis_s, plan_rest_nm: 0.0, abstand_nm: 0.0, segment: 0 }
    }

    /// Wie `landung_erwartet`, nur bis zum TOD-Fix; dahinter „jetzt“, ohne
    /// TOD-Fix oder ohne Planzeit am Ort kein Wert.
    #[test]
    fn tod_aus_der_position_auf_der_route() {
        let fixes = vec![
            api_client::RouteFix { ident: "TOC".into(), zeit_bis_hier_s: Some(1_200.0), ..Default::default() },
            api_client::RouteFix { ident: "TOD".into(), zeit_bis_hier_s: Some(4_500.0), ..Default::default() },
            api_client::RouteFix { ident: "EDDH".into(), zeit_bis_hier_s: Some(5_700.0), ..Default::default() },
        ];
        let jetzt_ms = 1_000_000_000_i64;
        assert_eq!(tod_erwartet(&fixes, &ort(Some(3_000.0)), jetzt_ms), Some(1_000_000 + 1_500));
        assert_eq!(landung_erwartet(&fixes, &ort(Some(3_000.0)), jetzt_ms), Some(1_000_000 + 2_700));
        assert_eq!(tod_erwartet(&fixes, &ort(Some(5_000.0)), jetzt_ms), Some(1_000_000), "hinter dem TOD: jetzt");
        assert_eq!(tod_erwartet(&fixes, &ort(None), jetzt_ms), None);
        assert_eq!(tod_erwartet(&fixes[2..], &ort(Some(3_000.0)), jetzt_ms), None, "ohne TOD-Fix");
    }
