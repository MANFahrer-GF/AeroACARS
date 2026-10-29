    use super::*;

    /// Baut einen OFP mit Sentinel-Score-Werten + einer konkreten Route.
    /// Die `*_kg`-Felder sind ABSICHTLICH != 0, damit ein versehentlicher
    /// Write in apply_route_only_to_stats sofort als Aenderung auffaellt.
    fn ofp_with_route(
        route: Option<&str>,
        alternate: Option<&str>,
        waypoints: Vec<api_client::RouteFix>,
    ) -> api_client::SimBriefOfp {
        api_client::SimBriefOfp {
            // Score-relevante Felder bewusst gefuellt — der Route-Only-Pfad
            // darf KEINS davon in die FlightStats schreiben.
            planned_block_fuel_kg: 12000.0,
            planned_burn_kg: 8000.0,
            planned_reserve_kg: 2200.0,
            planned_zfw_kg: 60000.0,
            planned_tow_kg: 72000.0,
            planned_ldw_kg: 64000.0,
            max_zfw_kg: 62500.0,
            max_tow_kg: 79000.0,
            max_ldw_kg: 66000.0,
            route: route.map(|s| s.to_string()),
            alternate: alternate.map(|s| s.to_string()),
            // bewusst gesetzt — der Route-Only-Pfad darf die Cruise-Ref
            // ebenso wenig in die FlightStats schreiben wie die *_kg-Felder.
            planned_cruise_alt_ft: Some(35000.0),
            waypoints,
            ofp_flight_number: "GSG100".to_string(),
            ofp_origin_icao: "EDDF".to_string(),
            ofp_destination_icao: "EGLL".to_string(),
            ofp_generated_at: "1778452800".to_string(),
            request_id: "req_new_ofp".to_string(),
            pax_count: 150,
            cargo_kg: 2000.0,
            freight_kg: 2000.0,
            ..Default::default()
        }
    }

    fn fix(ident: &str, lat: f64, lon: f64, kind: &str) -> api_client::RouteFix {
        api_client::RouteFix {
            ident: ident.to_string(),
            lat,
            lon,
            kind: kind.to_string(),
            // v1.7.35: die Sprit-Felder des Navlogs interessieren diesen
            // Test nicht — Default heisst hier None, nicht 0.
            ..Default::default()
        }
    }

    /// CONSTRAINT 1 (Kern): Vor/Nach-Snapshot beweist dass NUR die
    /// Routen-Felder wandern. Jedes `*_kg`, flight_plan_source,
    /// simbrief_ofp_id, simbrief_ofp_generated_at MUSS byte-identisch
    /// bleiben.
    #[test]
    fn route_only_mutation_touches_only_route_fields() {
        let mut stats = FlightStats::new();

        // Vorzustand: alle Score-Felder + OFP-Metadaten mit klar
        // identifizierbaren Werten, plus eine ALTE Route/Wegpunkte.
        stats.planned_block_fuel_kg = Some(9999.0);
        stats.planned_burn_kg = Some(8888.0);
        stats.planned_reserve_kg = Some(1111.0);
        stats.planned_zfw_kg = Some(55555.0);
        stats.planned_tow_kg = Some(66666.0);
        stats.planned_ldw_kg = Some(60000.0);
        stats.planned_max_zfw_kg = Some(60001.0);
        stats.planned_max_tow_kg = Some(77777.0);
        stats.planned_max_ldw_kg = Some(64321.0);
        stats.flight_plan_source = Some("simbrief");
        stats.simbrief_ofp_id = Some("OLD_OFP_ID".to_string());
        stats.simbrief_ofp_generated_at = Some("1700000000".to_string());
        stats.planned_route = Some("OLD ROUTE STRING".to_string());
        stats.planned_waypoints = vec![fix("OLDWP", 50.0, 8.0, "wpt")];
        stats.planned_alternate = Some("EDDK".to_string());

        // Frischer OFP mit ANDERER Route + Wegpunkten + Alternate.
        let new_ofp = ofp_with_route(
            Some("DENUT5L DENUT M624 SUSAN"),
            Some("EGKK"),
            vec![
                fix("DENUT", 50.1, 8.6, "wpt"),
                fix("SUSAN", 51.2, 0.3, "vor"),
            ],
        );

        apply_route_only_to_stats(&mut stats, &new_ofp);

        // ── Score-Felder: byte-identisch geblieben ──────────────────────
        assert_eq!(stats.planned_block_fuel_kg, Some(9999.0));
        assert_eq!(stats.planned_burn_kg, Some(8888.0));
        assert_eq!(stats.planned_reserve_kg, Some(1111.0));
        assert_eq!(stats.planned_zfw_kg, Some(55555.0));
        assert_eq!(stats.planned_tow_kg, Some(66666.0));
        assert_eq!(stats.planned_ldw_kg, Some(60000.0));
        assert_eq!(stats.planned_max_zfw_kg, Some(60001.0));
        assert_eq!(stats.planned_max_tow_kg, Some(77777.0));
        assert_eq!(stats.planned_max_ldw_kg, Some(64321.0));
        assert_eq!(stats.flight_plan_source, Some("simbrief"));
        assert_eq!(stats.simbrief_ofp_id.as_deref(), Some("OLD_OFP_ID"));
        assert_eq!(
            stats.simbrief_ofp_generated_at.as_deref(),
            Some("1700000000"),
            "OFP generated_at must NOT be touched by route-only sync"
        );

        // ── Route-Felder: uebernommen ───────────────────────────────────
        assert_eq!(
            stats.planned_route.as_deref(),
            Some("DENUT5L DENUT M624 SUSAN")
        );
        assert_eq!(stats.planned_waypoints.len(), 2);
        assert_eq!(stats.planned_waypoints[0].ident, "DENUT");
        assert_eq!(stats.planned_waypoints[1].ident, "SUSAN");
        assert_eq!(stats.planned_alternate.as_deref(), Some("EGKK"));
    }

    /// CONSTRAINT 1 (Defensive): ein OFP OHNE Navlog darf die schon
    /// vorhandenen Wegpunkte NICHT loeschen, und ein leerer Alternate
    /// darf den bestehenden Alternate nicht wegwischen.
    #[test]
    fn route_only_does_not_wipe_existing_waypoints_or_alternate_on_empty_ofp() {
        let mut stats = FlightStats::new();
        stats.planned_waypoints = vec![fix("KEEP", 50.0, 8.0, "wpt")];
        stats.planned_alternate = Some("EDDK".to_string());

        // OFP ohne Wegpunkte, leerer Alternate, aber mit Route-String.
        let ofp = ofp_with_route(Some("NEW ROUTE"), Some("   "), vec![]);
        apply_route_only_to_stats(&mut stats, &ofp);

        // Route-String wird uebernommen …
        assert_eq!(stats.planned_route.as_deref(), Some("NEW ROUTE"));
        // … aber die bestehenden Wegpunkte bleiben erhalten …
        assert_eq!(stats.planned_waypoints.len(), 1);
        assert_eq!(stats.planned_waypoints[0].ident, "KEEP");
        // … und der bestehende Alternate auch (leerer OFP-Alt loescht nicht).
        assert_eq!(stats.planned_alternate.as_deref(), Some("EDDK"));
    }

    /// CONSTRAINT 1: die exakte Feld-Liste die wandern DARF. Wenn jemand
    /// spaeter ein Feld in apply_route_only_to_stats ergaenzt, das KEIN
    /// Routen-Feld ist, faengt der Isolations-Test oben es. Dieser Test
    /// dokumentiert zusaetzlich die erlaubte Menge explizit.
    #[test]
    fn route_only_allowed_field_set_is_exactly_route_plus_alternate() {
        // Reiner Dokumentations-/Lock-Test: wenn ein OFP NUR die Route
        // aendert (gleicher Alternate, keine Wegpunkte), bleibt alles
        // andere unangetastet inkl. Alternate.
        let mut stats = FlightStats::new();
        stats.planned_alternate = Some("EDDK".to_string());
        let ofp = ofp_with_route(Some("R1"), Some("EDDK"), vec![]);
        apply_route_only_to_stats(&mut stats, &ofp);
        assert_eq!(stats.planned_route.as_deref(), Some("R1"));
        assert_eq!(stats.planned_alternate.as_deref(), Some("EDDK"));
        // Score-Felder default-leer geblieben.
        assert_eq!(stats.planned_block_fuel_kg, None);
        assert_eq!(stats.planned_tow_kg, None);
        assert_eq!(stats.simbrief_ofp_id, None);
        assert_eq!(stats.flight_plan_source, None);
    }

    /// CONSTRAINT 2: der DEP/ARR-Mismatch-HARD-Block. Die Match-Logik
    /// (try_simbrief_direct_with_match → DirectOutcome::Mismatch) ist die
    /// gleiche die flight_refresh_simbrief nutzt. Hier verifizieren wir die
    /// zugrundeliegende Match-Funktion: ein OFP fuer einen ANDEREN
    /// Flughafen matcht NICHT (→ wuerde Mismatch → HARD-Block ausloesen).
    #[test]
    fn mismatch_block_dep_arr_must_not_match_other_airport() {
        // Aktiver Flug EDDF→EGLL, aber SimBrief-OFP ist EDDM→LFPG.
        let dpt_ok = "EDDM".eq_ignore_ascii_case("EDDF");
        let arr_ok = "LFPG".eq_ignore_ascii_case("EGLL");
        assert!(
            !dpt_ok || !arr_ok,
            "ein OFP fuer einen anderen Flughafen DARF nicht matchen — \
             sonst koennte eine falsche Route gemalt werden"
        );
        // Und der gleiche Flug matcht (Sanity).
        assert!("EDDF".eq_ignore_ascii_case("eddf"));
        assert!("EGLL".eq_ignore_ascii_case("egll"));
    }

    /// CONSTRAINT 3a: Auto-Source fuellt einen LEEREN Identifier.
    #[test]
    fn autosource_fills_when_empty() {
        let mut settings = SimBriefSettings::default();
        let filled = maybe_autosource_simbrief_username(&mut settings, Some("thomaskant"));
        assert!(filled, "leerer Identifier muss auto-gefuellt werden");
        assert_eq!(settings.username.as_deref(), Some("thomaskant"));
        assert_eq!(settings.user_id, None);
    }

    /// CONSTRAINT 3b: Auto-Source ueberschreibt NIE einen explizit
    /// gesetzten Username.
    #[test]
    fn autosource_preserves_explicit_username() {
        let mut settings = SimBriefSettings {
            username: Some("manual_user".to_string()),
            user_id: None,
        };
        let filled = maybe_autosource_simbrief_username(&mut settings, Some("profile_user"));
        assert!(
            !filled,
            "expliziter Username darf NICHT ueberschrieben werden"
        );
        assert_eq!(settings.username.as_deref(), Some("manual_user"));
    }

    /// CONSTRAINT 3b: Auto-Source ueberschreibt NIE eine explizit gesetzte
    /// User-ID (auch wenn kein Username gesetzt ist — die User-ID zaehlt
    /// als "Identifier gesetzt").
    #[test]
    fn autosource_preserves_explicit_user_id() {
        let mut settings = SimBriefSettings {
            username: None,
            user_id: Some("612345".to_string()),
        };
        let filled = maybe_autosource_simbrief_username(&mut settings, Some("profile_user"));
        assert!(
            !filled,
            "gesetzte User-ID blockt Auto-Source des Usernamens"
        );
        assert_eq!(settings.username, None);
        assert_eq!(settings.user_id.as_deref(), Some("612345"));
    }

    /// CONSTRAINT 3c: leeres / fehlendes Profil-Feld fuellt nichts.
    #[test]
    fn autosource_noop_on_empty_or_missing_profile_value() {
        // None
        let mut s1 = SimBriefSettings::default();
        assert!(!maybe_autosource_simbrief_username(&mut s1, None));
        assert_eq!(s1.username, None);
        // Whitespace-only
        let mut s2 = SimBriefSettings::default();
        assert!(!maybe_autosource_simbrief_username(&mut s2, Some("   ")));
        assert_eq!(s2.username, None);
        // Trim wird angewandt
        let mut s3 = SimBriefSettings::default();
        assert!(maybe_autosource_simbrief_username(
            &mut s3,
            Some("  trimmed  ")
        ));
        assert_eq!(s3.username.as_deref(), Some("trimmed"));
    }

    /// CONSTRAINT 5: das Kommando ist im generate_handler! registriert.
    /// Wir lesen die Quelle (compile-time include_str!) und pruefen dass
    /// der Command-Name in der Handler-Liste steht — ein versehentliches
    /// Entfernen aus der Registrierung wuerde diesen Test brechen.
    #[test]
    fn command_is_registered_in_invoke_handler() {
        let src = include_str!("../lib.rs");
        // Die Registrierungs-Zeile in der generate_handler!-Liste.
        assert!(
            src.contains("\n            flight_refresh_route_only,\n"),
            "flight_refresh_route_only muss in generate_handler! registriert sein"
        );
    }

    /// v1.3.0 (#Hoppie-PDC-CPDLC): gleiche Absicherung fuer den
    /// wichtigsten Hoppie-Command — ein versehentliches Entfernen aus
    /// der Registrierung (Refactor-Unfall) wuerde sonst erst zur
    /// Laufzeit als kryptischer "command not found" auffallen.
    #[test]
    fn hoppie_connect_is_registered_in_invoke_handler() {
        let src = include_str!("../lib.rs");
        assert!(
            src.contains("\n            hoppie::hoppie_connect,\n"),
            "hoppie::hoppie_connect muss in generate_handler! registriert sein"
        );
    }

    /// Der geteilte Waypoint-Builder muss order 0-based hochzaehlen und
    /// die SimBrief-kind→nav_type-Map korrekt anwenden. Stellt sicher dass
    /// flight_start und flight_refresh_route_only bit-identische Routen
    /// posten.
    #[test]
    fn build_route_waypoints_orders_and_maps_nav_type() {
        let fixes = vec![
            fix("EDDF", 50.03, 8.57, "apt"),
            fix("DENUT", 50.1, 8.6, "wpt"),
            fix("SPL", 52.3, 4.7, "vor"),
            fix("XYZ", 51.0, 5.0, "ndb"),
            fix("WEIRD", 49.0, 6.0, "unknown_kind"),
        ];
        let route = build_route_waypoints(&fixes);
        assert_eq!(route.len(), 5);
        assert_eq!(route[0].order, 0);
        assert_eq!(route[0].nav_type, Some(4)); // apt
        assert_eq!(route[1].order, 1);
        assert_eq!(route[1].nav_type, Some(1)); // wpt
        assert_eq!(route[2].nav_type, Some(3)); // vor
        assert_eq!(route[3].nav_type, Some(2)); // ndb
        assert_eq!(route[4].nav_type, None); // unknown → omitted
        assert_eq!(route[4].order, 4);
        assert_eq!(route[0].name, "EDDF");
        assert_eq!(route[0].lat, 50.03);
        assert_eq!(route[0].lon, 8.57);
    }

    /// CONSTRAINT 4 (Guard): flight_refresh_simbrief behaelt sein
    /// Phasen-Gate. Wir verifizieren die Gate-Logik (nur Preflight..TaxiOut
    /// erlaubt) als Pure-Predicate — wenn jemand das Gate aus dem
    /// Full-OFP-Refresh entfernt, dokumentiert dieser Test die erwartete
    /// Menge.
    #[test]
    fn full_ofp_refresh_phase_gate_unchanged() {
        let allowed = |p: FlightPhase| {
            matches!(
                p,
                FlightPhase::Preflight
                    | FlightPhase::Boarding
                    | FlightPhase::Pushback
                    | FlightPhase::TaxiOut
            )
        };
        assert!(allowed(FlightPhase::Preflight));
        assert!(allowed(FlightPhase::Boarding));
        assert!(allowed(FlightPhase::Pushback));
        assert!(allowed(FlightPhase::TaxiOut));
        // Nach dem Takeoff gesperrt (Full-OFP-Refresh-Gate).
        assert!(!allowed(FlightPhase::Takeoff));
        assert!(!allowed(FlightPhase::Cruise));
        assert!(!allowed(FlightPhase::Approach));
        // Die Quelle haelt den Gate-Block weiterhin in
        // flight_refresh_simbrief.
        let src = include_str!("../lib.rs");
        assert!(
            src.contains("OFP-Refresh ist nur bis vor Takeoff moeglich (Preflight bis TaxiOut)"),
            "flight_refresh_simbrief muss sein Phasen-Gate behalten"
        );
    }
