    use super::*;

    fn vorlage() -> PrefileBody {
        PrefileBody {
            airline_id: 52,
            aircraft_id: "360".to_string(),
            flight_id: None,
            flight_number: "58".to_string(),
            dpt_airport_id: "SBBR".to_string(),
            arr_airport_id: "LPPT".to_string(),
            ..Default::default()
        }
    }

    fn offener_bericht() -> api_client::PirepSummary {
        api_client::PirepSummary {
            id: "ALT-PIREP".to_string(),
            airline_id: Some(52),
            flight_number: Some("58".to_string()),
            aircraft_id: Some(360),
            state: Some(0),
            dpt_airport_id: Some("SBBR".to_string()),
            arr_airport_id: Some("LPPT".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn belegtes_flugzeug_nennt_den_richtigen_grund() {
        // Anderer Pilot fliegt (eigene Liste bekannt, kein eigener PIREP).
        assert_eq!(
            flugzeug_belegt_code(Some(2), true, false),
            "aircraft_in_use"
        );
        assert_eq!(
            flugzeug_belegt_code(Some(1), true, false),
            "aircraft_in_use"
        );
        // Eigener hängengebliebener Flugbericht belegt die Maschine.
        assert_eq!(
            flugzeug_belegt_code(Some(2), true, true),
            "aircraft_in_use_own"
        );
        // Eigene Liste unbekannt → keine Behauptung über den Verursacher.
        assert_eq!(
            flugzeug_belegt_code(Some(2), false, false),
            "aircraft_not_available"
        );
        // Geparkt oder unbekannt → andere Ursache (Standort, Rang …).
        assert_eq!(
            flugzeug_belegt_code(Some(0), true, false),
            "aircraft_not_available"
        );
        assert_eq!(
            flugzeug_belegt_code(None, true, true),
            "aircraft_not_available"
        );
    }

    #[test]
    fn wiederaufnahme_desselben_flugs_wird_uebernommen() {
        // Der Fall, für den die Übernahme gebaut wurde: Absturz mitten im
        // Flug, alles identisch. Muss weiterhin greifen.
        assert!(pirep_ist_uebernehmbar(&offener_bericht(), &vorlage(), 52));
    }

    #[test]
    fn gleiche_nummer_andere_strecke_wird_nicht_uebernommen() {
        // DER BEFUND. Gleiche Flugnummer, aber der Pilot fliegt jetzt
        // zurück. Vorher erbte dieser Flug die Kennung des alten, und der
        // alte Eintrag behielt Strecke und Flugzeug.
        let mut body = vorlage();
        body.dpt_airport_id = "LPPT".to_string();
        body.arr_airport_id = "SBBR".to_string();
        assert!(!pirep_ist_uebernehmbar(&offener_bericht(), &body, 52));
    }

    #[test]
    fn gleiche_nummer_anderes_ziel_wird_nicht_uebernommen() {
        let mut body = vorlage();
        body.arr_airport_id = "LPPR".to_string();
        assert!(!pirep_ist_uebernehmbar(&offener_bericht(), &body, 52));
    }

    #[test]
    fn anderes_flugzeug_wird_nicht_uebernommen() {
        let mut body = vorlage();
        body.aircraft_id = "999".to_string();
        assert!(!pirep_ist_uebernehmbar(&offener_bericht(), &body, 52));
    }

    #[test]
    fn fehlende_felder_blockieren_die_wiederaufnahme_nicht() {
        // phpVMS liefert Strecke und Flugzeug je nach Version/Konfiguration
        // nicht mit. Dann darf der Vergleich nicht ausschliessen — sonst
        // wäre die Wiederaufnahme auf solchen Installationen ganz tot.
        let mut alt = offener_bericht();
        alt.dpt_airport_id = None;
        alt.arr_airport_id = None;
        alt.aircraft_id = None;
        assert!(pirep_ist_uebernehmbar(&alt, &vorlage(), 52));
    }

    #[test]
    fn grossschreibung_der_icao_spielt_keine_rolle() {
        let mut alt = offener_bericht();
        alt.dpt_airport_id = Some("sbbr".to_string());
        assert!(pirep_ist_uebernehmbar(&alt, &vorlage(), 52));
    }

    #[test]
    fn abgeschlossener_bericht_wird_nie_uebernommen() {
        let mut alt = offener_bericht();
        alt.state = Some(2); // ACCEPTED
        assert!(!pirep_ist_uebernehmbar(&alt, &vorlage(), 52));
    }

    #[test]
    fn andere_flugnummer_wird_nicht_uebernommen() {
        let mut alt = offener_bericht();
        alt.flight_number = Some("59".to_string());
        assert!(!pirep_ist_uebernehmbar(&alt, &vorlage(), 52));
    }
