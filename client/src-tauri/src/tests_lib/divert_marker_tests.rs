    use super::*;
    use crate::arrival::ArrivalSite;

    /// Hints kommen ausschließlich aus einer `ArrivalSite` — dieselbe Regel,
    /// die auch der Produktionscode hat. Ein Hint auf den Planflughafen ist
    /// deshalb nicht "ein Test, den wir vergessen haben", sondern nicht
    /// konstruierbar (siehe `arrival::DivertHint`).
    fn hint(actual: Option<&str>) -> DivertHint {
        let site = match actual {
            Some(icao) => ArrivalSite::AtOtherAirport {
                icao: icao.to_string(),
                distance_from_planned_nm: 120.0,
            },
            None => ArrivalSite::OffAirport {
                distance_from_planned_nm: 120.0,
            },
        };
        DivertHint::from_site(&site, "EDDF", None).expect("non-planned site yields a hint")
    }

    #[test]
    fn normal_arrival_no_hint_yields_no_markers() {
        let m = divert_payload_markers("EDDF", "EDDF", None);
        assert_eq!(m.divert, None);
        assert_eq!(m.diverted_to, None);
        assert_eq!(m.divert_suspected, None);
    }

    /// Die Sprit-Achse muss dieselbe Regel benutzen wie der Marker.
    ///
    /// ⚠ v1.7.12 hat die Achse zuerst an `divert_hint` gehaengt — an den
    /// VERDACHT. Das geht in beide Richtungen schief, und beide Faelle
    /// stehen als Regressionstest schon eine Zeile weiter oben:
    ///
    ///   * `confirmed_divert_is_reported_as_fact` — ein echter,
    ///     eingereichter Divert OHNE Hinweis. Er waere bewertet worden,
    ///     gegen einen Plan, der nie geflogen wurde.
    ///   * `suspicion_the_pilot_declined_is_never_reported_as_a_divert` —
    ///     ein Verdacht, den der Pilot abgelehnt hat. Er waere
    ///     uebersprungen worden, obwohl genau die geplante Strecke
    ///     geflogen wurde.
    #[test]
    fn die_spritachse_folgt_dem_eingereichten_ziel_nicht_dem_verdacht() {
        // Echter Divert, kein Hinweis — muss uebersprungen werden.
        assert_eq!(
            Ausweichziel::ist_ausweichflug(Some(Ausweichziel {
                eingereicht: "EDDV",
                geplant: "EDDF"
            })),
            Some(true),
            "ein eingereichter Divert ohne Hinweis wurde nicht erkannt"
        );
        // Verdacht abgelehnt, planmaessig angekommen — muss bewertet werden.
        assert_eq!(
            Ausweichziel::ist_ausweichflug(Some(Ausweichziel {
                eingereicht: "EDDF",
                geplant: "EDDF"
            })),
            Some(false),
            "ein abgelehnter Verdacht darf die Achse nicht aussetzen"
        );
        // Schreibweise und Leerraum duerfen nichts entscheiden.
        assert_eq!(
            Ausweichziel::ist_ausweichflug(Some(Ausweichziel {
                eingereicht: " eddf ",
                geplant: "EDDF"
            })),
            Some(false)
        );
        // Unbekannt ist NICHT dasselbe wie "kein Divert".
        assert_eq!(Ausweichziel::ist_ausweichflug(None), None);
    }

    #[test]
    fn confirmed_divert_is_reported_as_fact() {
        let m = divert_payload_markers("EDDV", "EDDF", None);
        assert_eq!(m.divert, Some(true));
        assert_eq!(m.diverted_to.as_deref(), Some("EDDV"));
        assert_eq!(m.divert_suspected, None);
    }

    #[test]
    fn confirmed_divert_wins_over_stale_hint() {
        // Der bestätigte Divert zählt — diverted_to spiegelt das echte Ziel,
        // nicht ein abweichendes Hint-ICAO.
        let h = hint(Some("EDDH"));
        let m = divert_payload_markers("EDDV", "EDDF", Some(&h));
        assert_eq!(m.divert, Some(true));
        assert_eq!(m.diverted_to.as_deref(), Some("EDDV"));
    }

    #[test]
    fn suspicion_the_pilot_declined_is_never_reported_as_a_divert() {
        // Die FSM hat ein Ausweichfeld erkannt, der Pilot hat trotzdem als
        // Planflug eingereicht. phpVMS bekommt eine normale Ankunft — dann
        // darf der Recorder daraus keinen Divert machen.
        let h = hint(Some("LOWW"));
        let m = divert_payload_markers("EDDF", "EDDF", Some(&h));
        assert_eq!(
            m.divert, None,
            "a suspicion must never be reported as a filed divert"
        );
        assert_eq!(m.diverted_to, None);
        // Der Verdacht geht als Diagnose mit — er verschwindet nicht.
        assert_eq!(m.divert_suspected, Some(true));
        assert_eq!(m.divert_suspected_icao.as_deref(), Some("LOWW"));
    }

    #[test]
    fn suspicion_without_a_named_field_still_travels_as_a_suspicion() {
        let h = hint(None);
        let m = divert_payload_markers("EDDF", "EDDF", Some(&h));
        assert_eq!(m.divert, None);
        assert_eq!(m.divert_suspected, Some(true));
        assert_eq!(m.divert_suspected_icao, None);
    }
