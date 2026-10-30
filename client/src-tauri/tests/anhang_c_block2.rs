//! Pflichtenheft P-16.01, Anhang C Block 2 (Bahn): Regeln aus dem
//! oeffentlichen Modul `szenerie_bahn`, je Regel ein Test, benannt nach der
//! Regelnummer.

use aeroacars_app_lib::szenerie_bahn::{test_navairport, uebernimm_szenerie, Quelle, UebernahmeBericht};
use sim_core::szenerie::{KursQuelle, SzenerieBahn, SzenerieFlughafen};

fn edhe() -> aeroacars_mqtt::navdata::NavAirport {
    let zeile: Vec<String> = [
        "EDHE", "09", "89.9957", "87.0", "131", "3609", "53.6459", "9.6942", "53.6459", "9.7142",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    test_navairport("EDHE", &[zeile])
}

fn szenerie(schwelle: (f64, f64), bestaetigt: Option<f64>) -> SzenerieFlughafen {
    SzenerieFlughafen {
        icao: "EDHE".into(),
        quelle: "Test".into(),
        bahnen: vec![SzenerieBahn {
            bezeichner: "09".into(),
            kurs_grad: 93.72,
            breite_m: 55.0,
            laenge_m: 1100.0,
            versetzte_schwelle_m: 0.0,
            schwelle,
            gegenende: (schwelle.0, schwelle.1 + 0.02),
            belag_code: 1,
            kurs_bestaetigt_grad: bestaetigt,
            kurs_quelle: KursQuelle::Unbestaetigt,
        }],
        ..Default::default()
    }
}

#[test]
fn x2_019_bestaetigter_kurs_aendert_weder_kurs_noch_uebernahme() {
    let nav = edhe();
    for quelle in [Quelle::XPlaneDatei, Quelle::MsfsFacility] {
        let ohne = uebernimm_szenerie(&nav, &szenerie((53.6459, 9.6942), None), quelle);
        // Ein „bestaetigter" Kurs weit neben dem gemeldeten: wuerde er gelesen,
        // kaeme ein anderer Kurs oder ein anderer Bericht heraus.
        let mit = uebernimm_szenerie(&nav, &szenerie((53.6459, 9.6942), Some(120.0)), quelle);
        assert_eq!(ohne, mit, "{quelle:?}");
    }
    // Gegenprobe Messgroesse: der gemeldete Kurs wirkt (X-Plane uebernimmt ihn).
    let (aus, _) = uebernimm_szenerie(&nav, &szenerie((53.6459, 9.6942), None), Quelle::XPlaneDatei);
    assert!((aus.runways[0].true_course - 93.72).abs() < 0.001);
}

#[test]
fn s6_038_uebernahmebericht_ueberlebt_den_neustart_verworfen_getrennt() {
    let nav = edhe();
    // Gleicher Bezeichner, Lage 10 km daneben: verworfen, nicht „ohne Treffer".
    let (_, b) = uebernimm_szenerie(&nav, &szenerie((53.7359, 9.6942), None), Quelle::XPlaneDatei);
    assert_eq!(b.verworfen, vec!["09"]);
    assert!(b.ohne_treffer.is_empty() && b.uebernommen.is_empty());

    let voll = UebernahmeBericht {
        uebernommen: vec!["09".into()],
        ohne_treffer: vec!["27".into()],
        verworfen: vec!["15".into()],
        groesste_kursabweichung_grad: 3.72,
        groesste_breitenabweichung_m: 15.1,
        groesste_schwellenabweichung_m: 573.0,
    };
    let json = serde_json::to_string(&voll).unwrap();
    let zurueck: UebernahmeBericht = serde_json::from_str(&json).unwrap();
    assert_eq!(zurueck, voll, "kein Feld darf beim Speichern verloren gehen");
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["groesste_schwellenabweichung_m"], 573.0);
}

// ---------------- Bordbuch (oeffentliches Modul `bordbuch`) ----------------

mod bordbuch_anflug {
    use aeroacars_app_lib::bordbuch::{tick, Einstellungen, Klasse, Kontext, Regel, Status, Zustand};
    use chrono::{DateTime, TimeZone, Utc};
    use sim_core::{AircraftProfile, FlightPhase, SimSnapshot};

    fn t(sek: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap() + chrono::Duration::seconds(sek)
    }

    fn status(z: &Zustand, r: Regel) -> Status {
        z.punkte.iter().find(|p| p.regel == r).expect("Punkt fehlt").status
    }

    /// Abflug EDDF, gestartet, in der Luft auf `msl` ft — Landelicht aus.
    fn gestartet(z: &mut Zustand, k: &Kontext, msl: f64) -> SimSnapshot {
        let mut s = SimSnapshot::default();
        s.lat = 50.03;
        s.lon = 8.56;
        s.on_ground = true;
        s.engines_running = 2;
        s.aircraft_profile = AircraftProfile::FenixA320;
        s.aircraft_title = Some("Fenix A320 CFM".into());
        s.light_landing = Some(false);
        s.spoilers_armed = Some(false);
        s.autobrake = Some("OFF".into());
        s.seatbelts_sign = Some(0);
        s.timestamp = t(0);
        tick(z, &s, FlightPhase::TaxiOut, k);
        s.timestamp = t(5);
        tick(z, &s, FlightPhase::TakeoffRoll, k);
        s.on_ground = false;
        s.altitude_msl_ft = msl;
        s.altitude_agl_ft = msl - 100.0;
        s.timestamp = t(600);
        tick(z, &s, FlightPhase::Cruise, k);
        s
    }

    /// Takte im 5-s-Raster wie der Streamer (groessere Spruenge gelten als
    /// Pause und verschieben die Fristen).
    fn takte(z: &mut Zustand, s: &mut SimSnapshot, phase: FlightPhase, k: &Kontext, von: i64, bis: i64) {
        let mut sek = von;
        while sek <= bis {
            s.timestamp = t(sek);
            tick(z, s, phase, k);
            sek += 5;
        }
    }

    fn kontext(e: &Einstellungen) -> Kontext<'_> {
        Kontext { klasse: Klasse::Airliner, einstellungen: e, abflug: Some("EDDF"), ziel: Some("EDDM") }
    }

    #[test]
    fn s7_210_landelicht_unter_fl100_frist_60_s() {
        let e = Einstellungen::default();
        let k = kontext(&e);
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 12_000.0);
        // Reiseflug ueber FL100 gesehen; jetzt Sinkflug darunter.
        s.altitude_msl_ft = 9_500.0;
        s.altitude_agl_ft = 9_400.0;
        s.vertical_speed_fpm = -1_500.0;
        takte(&mut z, &mut s, FlightPhase::Descent, &k, 1_200, 1_255);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::Offen, "55 s: Frist laeuft noch");
        takte(&mut z, &mut s, FlightPhase::Descent, &k, 1_260, 1_265);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::DiesmalOhne, "nach 60 s entschieden");

        // Gegenprobe: innerhalb der Frist eingeschaltet → erledigt.
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 12_000.0);
        s.altitude_msl_ft = 9_500.0;
        s.altitude_agl_ft = 9_400.0;
        s.vertical_speed_fpm = -1_500.0;
        takte(&mut z, &mut s, FlightPhase::Descent, &k, 1_200, 1_250);
        s.light_landing = Some(true);
        takte(&mut z, &mut s, FlightPhase::Descent, &k, 1_255, 1_255);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::Erledigt);
    }

    #[test]
    fn s7_210_unter_fl100_ohne_sinken_oder_anflugphase_keine_frist() {
        let e = Einstellungen::default();
        let k = kontext(&e);
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 12_000.0);
        s.altitude_msl_ft = 9_500.0;
        s.altitude_agl_ft = 9_400.0;
        s.vertical_speed_fpm = -200.0; // nicht < −300, Reiseflugphase
        takte(&mut z, &mut s, FlightPhase::Cruise, &k, 1_200, 1_400);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::Offen);
    }

    #[test]
    fn s7_210_kurzer_flug_erst_unter_1000_ft_im_endanflug_frist_10_s() {
        let e = Einstellungen::default();
        let k = kontext(&e);
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 6_000.0);
        // Nie ueber FL100: im Sinkflug auf 3000 ft AGL laeuft noch nichts.
        s.altitude_msl_ft = 3_100.0;
        s.altitude_agl_ft = 3_000.0;
        s.vertical_speed_fpm = -1_000.0;
        takte(&mut z, &mut s, FlightPhase::Descent, &k, 900, 1_100);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::Offen);
        // Unter 1000 ft im Final: 10 s Frist.
        s.altitude_msl_ft = 1_000.0;
        s.altitude_agl_ft = 900.0;
        takte(&mut z, &mut s, FlightPhase::Final, &k, 1_200, 1_205);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::Offen, "5 s: Frist laeuft");
        takte(&mut z, &mut s, FlightPhase::Final, &k, 1_210, 1_215);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::DiesmalOhne, "nach 10 s entschieden");
    }

    #[test]
    fn s7_211_endanflug_spoiler_autobrake_gurte_einmal_gesehen_reicht() {
        let e = Einstellungen::default();
        let k = kontext(&e);
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 6_000.0);
        // Ueber 1000 ft AGL zaehlt es nicht.
        s.altitude_msl_ft = 1_600.0;
        s.altitude_agl_ft = 1_500.0;
        s.spoilers_armed = Some(true);
        s.autobrake = Some("LO".into());
        s.seatbelts_sign = Some(2);
        takte(&mut z, &mut s, FlightPhase::Approach, &k, 900, 905);
        for r in [Regel::SpoilerLandung, Regel::AutobrakeLandung, Regel::AnschnallLandung] {
            assert_eq!(status(&z, r), Status::Offen, "{r:?} ueber 1000 ft");
        }
        assert_eq!(z.nacht_landung, None);
        // Darunter einmal gesehen → erledigt, auch wenn danach wieder aus.
        s.altitude_msl_ft = 900.0;
        s.altitude_agl_ft = 800.0;
        takte(&mut z, &mut s, FlightPhase::Final, &k, 1_000, 1_000);
        s.spoilers_armed = Some(false);
        s.autobrake = Some("OFF".into());
        s.seatbelts_sign = Some(0);
        takte(&mut z, &mut s, FlightPhase::Final, &k, 1_005, 1_010);
        for r in [Regel::SpoilerLandung, Regel::AutobrakeLandung, Regel::AnschnallLandung] {
            assert_eq!(status(&z, r), Status::Erledigt, "{r:?}");
        }
        assert_eq!(z.nacht_landung, Some(false), "Mittag in Muenchen: Tag");
    }

    #[test]
    fn s7_210_fl100_vorbedingung_10500_ft_und_anflugphase_ohne_sinken() {
        let e = Einstellungen::default();
        let k = kontext(&e);
        // Nur bis 10.400 ft: kein FL100-Reiseflug, unter FL100 sinkend
        // startet KEINE 60-s-Frist (erst der Endanflug unter 1000 ft).
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 10_400.0);
        s.altitude_msl_ft = 9_500.0;
        s.altitude_agl_ft = 9_400.0;
        s.vertical_speed_fpm = -1_500.0;
        takte(&mut z, &mut s, FlightPhase::Descent, &k, 1_200, 1_400);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::Offen);
        // Ueber 10.500 ft gewesen, Anflugphase ohne Sinkrate startet die Frist.
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 10_600.0);
        s.altitude_msl_ft = 9_500.0;
        s.altitude_agl_ft = 9_400.0;
        s.vertical_speed_fpm = 0.0;
        takte(&mut z, &mut s, FlightPhase::Approach, &k, 1_200, 1_265);
        assert_eq!(status(&z, Regel::LandelichtAnflug), Status::DiesmalOhne);
    }

    #[test]
    fn s7_211_landephase_zaehlt_am_boden_nicht_nachtlandung_wird_festgehalten() {
        let e = Einstellungen::default();
        let k = kontext(&e);
        // Am Boden (Aufsetzen) gesehen zaehlt nicht als Endanflug.
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 6_000.0);
        s.on_ground = true;
        s.altitude_msl_ft = 1_600.0;
        s.altitude_agl_ft = 0.0;
        s.spoilers_armed = Some(true);
        takte(&mut z, &mut s, FlightPhase::Landing, &k, 1_000, 1_000);
        assert_eq!(status(&z, Regel::SpoilerLandung), Status::Offen);
        // In der Luft in Landing-Phase unter 1000 ft: zaehlt; nachts wird die
        // Nachtlandung festgehalten (EDDM, 23:00 UTC).
        let mut z = Zustand::default();
        let mut s = gestartet(&mut z, &k, 6_000.0);
        s.altitude_msl_ft = 900.0;
        s.altitude_agl_ft = 800.0;
        s.spoilers_armed = Some(true);
        s.lat = 48.35;
        s.lon = 11.78;
        takte(&mut z, &mut s, FlightPhase::Landing, &k, 39_600, 39_600);
        assert_eq!(status(&z, Regel::SpoilerLandung), Status::Erledigt);
        assert_eq!(z.nacht_landung, Some(true));
    }
}
