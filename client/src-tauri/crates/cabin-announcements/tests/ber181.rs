//! Echter Flug als Prüfstein: Michael K, BER181 EDDF–EDDL (A320), 18.10.2026.
//! Seine Rückmeldung („Boarding sofort“, „Türen-zu-Ansage erst beim Rollen“,
//! „Plätze für die Landung erst bei 700 ft“) lag an den Auslösern. Hier läuft
//! jeder Takt seines Fluges (5-s-Raster aus dem Flight-Log) durch den Planer,
//! und die Auslöser müssen zu den Schaltern seines Cockpits passen.

use std::collections::BTreeMap;

use cabin_announcements::flugart::Flugart;
use cabin_announcements::slot::Slot;
use cabin_announcements::{Entfall, Flugdaten, Planer, Tick};
use serde::Deserialize;
use sim_core::FlightPhase;

#[derive(Deserialize)]
struct Daten {
    ticks: Vec<(i64, FlightPhase, f64, f64, f64, u8, Option<u8>, u8, u8, f64, u8)>,
}

/// Aufsetzen laut Flight-Log (13:24:24 UTC).
const LANDUNG: i64 = 1_792_329_864;

fn uhr(t: i64) -> String {
    let s = t.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

#[test]
fn ausloeser_passen_zu_michaels_cockpit() {
    let d: Daten = serde_json::from_str(include_str!("daten/ber181.json")).expect("Testdaten");
    let mut p = Planer::neu(Flugdaten {
        flugart: Some(Flugart::Linie),
        geplante_flugzeit_min: Some(55),
        tod_nach_abheben_s: None,
        ziel_plan_icao: Some("EDDL".into()),
        reiseflughoehe_ft: Some(20_000),
        abheben_epoch: None,
    });
    let mut wann: BTreeMap<Slot, i64> = BTreeMap::new();
    let mut entfallen = Vec::new();
    for (t, phase, alt, agl, vs, boden, zeichen, beacon, licht, fahrwerk, pushback) in d.ticks {
        let tk = Tick {
            jetzt: t,
            phase,
            hoehe_ft: alt,
            vs_fpm: vs,
            am_boden: boden == 1,
            anschnallzeichen: zeichen,
            pushback_aktiv: pushback == 1,
            rest_min: (t < LANDUNG).then(|| (LANDUNG - t) as f64 / 60.0),
            beacon: Some(beacon == 1),
            landelicht: Some(licht == 1),
            fahrwerk_unten: Some(fahrwerk >= 0.9),
            hoehe_agl_ft: Some(agl),
            ..Default::default()
        };
        for f in p.tick(&tk) {
            wann.entry(f.slot).or_insert(t);
        }
        entfallen.extend(p.entfallene_abholen());
    }
    for (s, t) in &wann {
        println!("{} {s:?}", uhr(*t));
    }
    println!("entfallen: {entfallen:?}");
    let bei = |s: Slot| uhr(*wann.get(&s).unwrap_or_else(|| panic!("{s:?} kam nicht")));
    // Boarding (2.1.1 A): Das Anschnallzeichen (12:25:59) löst nicht mehr
    // aus; ohne ZFW-Signal im Log kommt die Begrüßung beim Beacon 12:35:13,
    // vor „Boarding completed“ und „Türen zu“.
    assert_eq!(bei(Slot::Boarding), bei(Slot::BegruessungTuerenZu));
    assert_eq!(bei(Slot::BoardingAbgeschlossen), bei(Slot::BegruessungTuerenZu));
    // Türen zu: Beacon an 12:35:13 (vorher erst beim Rollen 12:38:50).
    assert_eq!(&bei(Slot::BegruessungTuerenZu)[..5], "12:35");
    assert_eq!(bei(Slot::BegruessungTuerenZu), bei(Slot::Sicherheit));
    // Plätze für den Start: Landelicht beim Rollen 12:42:44 (vorher Startlauf 12:51:49).
    assert_eq!(&bei(Slot::KommandoStart)[..5], "12:42");
    // Plätze für die Landung: Fahrwerk raus 13:21:20 bei 2.179 ft (vorher Final 13:23:35).
    assert_eq!(&bei(Slot::KommandoPlaetzeLandung)[..5], "13:21");
    // Kurzstrecke: Das Anschnallzeichen erlosch erst 13:01:56, 22 min vor der
    // Landung — der Service entfällt sichtbar wegen zu wenig Zeit.
    assert!(entfallen.contains(&(Slot::Reiseflug25, Entfall::ZuWenigZeit)), "{entfallen:?}");
    // Anschnallzeichen 13:07:24 für den Sinkflug eingeschaltet: keine Turbulenz.
    assert!(!wann.contains_key(&Slot::Turbulenz), "Zeichen für den Sinkflug ist keine Turbulenz");
}
