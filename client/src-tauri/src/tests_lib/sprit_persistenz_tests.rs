    use super::*;

    /// Jedes in v1.7.36 neue Feld ueberlebt den Neustart.
    ///
    /// Fehlt eines in `PersistedFlightStats`, verliert ein laufender Flug es
    /// beim Neustart STILL: kein Fehler, nur eine fehlende Zahl im PIREP.
    /// Jede Zuweisung wird hier einzeln geprueft — ein Vergleich der ganzen
    /// Struktur wuerde nicht sagen, WELCHES Feld fehlt.
    #[test]
    fn sprit_neue_felder_ueberleben_den_neustart() {
        let mut st = FlightStats::default();
        st.landing_fuel_kg = Some(16_770.0);
        st.landing_fuel_puffer = true;
        st.engine_start_fuel_kg = Some(41_644.0);
        st.engine_off_fuel_kg = Some(16_121.0);
        st.sprit_plan_bis_vergleichspunkt_kg = Some(19_420.0);
        st.sprit_plan_rest_ab_vergleichspunkt_nm = Some(133.5);
        let json = serde_json::to_string(&PersistedFlightStats::snapshot_from(&st)).unwrap();
        let zurueck: PersistedFlightStats = serde_json::from_str(&json).unwrap();
        let mut n = FlightStats::default();
        zurueck.apply_to(&mut n);
        assert!(n.landing_fuel_puffer, "landing_fuel_puffer ging verloren");
        assert_eq!(
            n.engine_start_fuel_kg,
            Some(41_644.0),
            "engine_start_fuel_kg"
        );
        assert_eq!(n.engine_off_fuel_kg, Some(16_121.0), "engine_off_fuel_kg");
        assert_eq!(
            n.sprit_plan_bis_vergleichspunkt_kg,
            Some(19_420.0),
            "plan_bis am Ort"
        );
        assert_eq!(
            n.sprit_plan_rest_ab_vergleichspunkt_nm,
            Some(133.5),
            "plan_rest am Ort"
        );
    }

    /// Durchstart und neues Abheben setzen die Herkunft des Landesprits
    /// zurueck — sonst behielte die echte Landung den Puffer-Vorrang der
    /// abgebrochenen, und ein Stempel ohne Puffer verdraengte nichts mehr.
    #[test]
    fn sprit_herkunft_des_landesprits_wird_zurueckgesetzt() {
        let mut st = FlightStats::default();
        st.landing_fuel_kg = Some(20_000.0);
        st.landing_fuel_puffer = true;
        sprit_durchstart_zuruecksetzen(&mut st);
        assert!(
            !st.landing_fuel_puffer,
            "Durchstart liess die Herkunft stehen"
        );
        assert!(
            st.landing_fuel_kg.is_none(),
            "Gegenprobe: der Wert selbst ist auch weg"
        );
    }

    /// Die eingefrorene Auswertung ueberlebt einen Neustart unveraendert.
    ///
    /// Ohne Persistenz wuerde `sprit_auswertung_einmal` nach dem Resume neu
    /// rechnen. Weil `distance_nm` beim Rollen zum Gate weiterlaeuft, stuende
    /// dann eine andere Anflugstrecke im PIREP als die, die der Pilot im
    /// Landungs-Tab schon gesehen hat.
    #[test]
    fn sprit_auswertung_ueberlebt_den_neustart() {
        let mut st = FlightStats::default();
        st.takeoff_fuel_kg = Some(40_919.0);
        st.landing_fuel_kg = Some(16_770.0);
        st.planned_reserve_kg = Some(4_916.0);
        st.sprit_vergleichspunkt_kg = Some(22_231.0);
        st.sprit_vergleichspunkt_odo_nm = Some(600.0);
        st.sprit_plan_bis_tod_kg = Some(19_353.0);
        st.planned_burn_kg = Some(21_218.0);
        st.distance_nm = 782.0;
        let eingefroren = sprit_auswertung_einmal(&mut st).expect("auswertung");
        assert_eq!(eingefroren.strecke_anflug_nm, Some(182.0));

        let json = serde_json::to_string(&PersistedFlightStats::snapshot_from(&st)).unwrap();
        let zurueck: PersistedFlightStats = serde_json::from_str(&json).unwrap();
        let mut nach_neustart = FlightStats::default();
        zurueck.apply_to(&mut nach_neustart);

        // Nach dem Neustart rollt das Flugzeug weiter zum Gate.
        nach_neustart.distance_nm = 795.0;
        let wieder = sprit_auswertung_einmal(&mut nach_neustart).expect("auswertung");
        assert_eq!(
            wieder, eingefroren,
            "die Auswertung wurde nach dem Neustart neu gerechnet"
        );
        assert_eq!(wieder.strecke_anflug_nm, Some(182.0));
    }

    /// Eine Sicherungsdatei vor v1.7.35 kennt das Feld nicht — sie muss
    /// weiterhin laden, und die Auswertung entsteht dann neu.
    #[test]
    fn sprit_altbestand_ohne_auswertung_laedt_weiter() {
        let st = FlightStats::default();
        let mut value = serde_json::to_value(PersistedFlightStats::snapshot_from(&st)).unwrap();
        value.as_object_mut().unwrap().remove("sprit_auswertung");
        let zurueck: PersistedFlightStats = serde_json::from_value(value).expect("altbestand");
        let mut back = FlightStats::default();
        zurueck.apply_to(&mut back);
        assert!(back.sprit_auswertung.is_none());
    }
