//! Echte Flüge als Prüfstein für v2.0.28: jede Rückmeldung an dem Flug, an
//! dem sie auffiel. Planer und Warteschlange laufen über jeden Takt des
//! Flight-Logs (5-s-Raster, im Reiseflug 60 s) mit echten Sprechdauern.

use std::collections::BTreeMap;

use cabin_announcements::flugart::Flugart;
use cabin_announcements::slot::{Rolle, Slot};
use cabin_announcements::{Flugdaten, Planer, Tick, Warteschlange};
use serde::Deserialize;
use sim_core::FlightPhase;

type Takt = (i64, FlightPhase, f64, f64, f64, u8, Option<u8>, u8, u8, f64, u8, u8, f64, Option<u8>);

#[derive(Deserialize)]
struct Daten {
    landung: i64,
    abheben: i64,
    ticks: Vec<Takt>,
}

fn uhr(t: i64) -> String {
    let s = t.rem_euclid(86_400);
    format!("{:02}:{:02}:{:02}", s / 3600, s % 3600 / 60, s % 60)
}

/// Echte Längen, zwei Sprachen: Sicherheit gut 1,5 min je Sprache, Kommando
/// ein Satz, sonst eine halbe Minute je Sprache.
fn dauer(s: Slot) -> i64 {
    match s.rolle() {
        Rolle::Chime => 3,
        Rolle::Kommando => 4,
        _ if s == Slot::Sicherheit => 190,
        _ => 60,
    }
}

/// Wann jede Ansage gespielt wurde, und wann sie gemeldet war.
struct Ablauf {
    gespielt: BTreeMap<Slot, i64>,
    gemeldet: BTreeMap<Slot, i64>,
}

fn fliegen(datei: &str, flugzeit_min: u32, reise_ft: u32) -> (Daten, Ablauf) {
    fliegen_mit(datei, flugzeit_min, reise_ft, None)
}

fn fliegen_mit(datei: &str, flugzeit_min: u32, reise_ft: u32, zweiter_service: Option<bool>) -> (Daten, Ablauf) {
    fliegen_rundlauf(datei, flugzeit_min, reise_ft, zweiter_service, false)
}

/// `rundlauf`: nach jedem Takt den Planer sichern und wiederherstellen
/// (v3 Etappe 4: Planer mit Zustand von aussen betreiben, P-3.58).
fn fliegen_rundlauf(datei: &str, flugzeit_min: u32, reise_ft: u32, zweiter_service: Option<bool>, rundlauf: bool) -> (Daten, Ablauf) {
    let d: Daten = serde_json::from_str(datei).expect("Testdaten");
    // Wie der Host: Planer mit `fortsetzen` aus der ersten Phase.
    let erste = d.ticks.first().map(|x| x.1).expect("Takte");
    let mut p = Planer::fortsetzen(Flugdaten {
        flugart: Some(Flugart::Linie),
        geplante_flugzeit_min: Some(flugzeit_min),
        tod_nach_abheben_s: None,
        ziel_plan_icao: Some("ZIEL".into()),
        reiseflughoehe_ft: Some(reise_ft),
        abheben_epoch: None,
    }, erste);
    p.reiseflug80_art(zweiter_service);
    let mut w = Warteschlange::default();
    let mut gespielt = BTreeMap::new();
    let mut gemeldet = BTreeMap::new();
    let mut ende: Option<(i64, Slot)> = None;
    for &(t, phase, alt, agl, vs, boden, zeichen, beacon, licht, fahrwerk, pushback, triebwerke, zfw, _gruen) in &d.ticks {
        if let Some((e, s)) = ende {
            if t >= e {
                if s.loest_pause_aus() {
                    w.ansage_zu_ende_nach(e, s);
                } else {
                    w.frei();
                }
                if s == Slot::KapitaenSinkflug {
                    p.kapitaen_sinkflug_ende(true, e);
                }
                if s == Slot::Reiseflug25 {
                    p.service_ende(true);
                }
                ende = None;
            }
        }
        let tk = Tick {
            jetzt: t,
            phase,
            hoehe_ft: alt,
            vs_fpm: vs,
            am_boden: boden == 1,
            anschnallzeichen: zeichen,
            pushback_aktiv: pushback == 1,
            rest_min: (t < d.landung).then(|| (d.landung - t) as f64 / 60.0),
            beacon: Some(beacon == 1),
            landelicht: Some(licht == 1),
            fahrwerk_unten: Some(fahrwerk >= 0.9),
            hoehe_agl_ft: Some(agl),
            triebwerke_an: Some(triebwerke > 0),
            zfw_kg: (zfw > 0.0).then_some(zfw),
            ..Default::default()
        };
        let neu = p.tick(&tk);
        for f in &neu {
            gemeldet.entry(f.slot).or_insert(t);
        }
        w.aufnehmen(neu);
        // Host-Regel: Kapitän nach dem Start erst über 10.000 ft.
        let tief = boden == 0 && alt < 10_000.0 && cabin_announcements::planer::rang(phase) < cabin_announcements::planer::rang(FlightPhase::Cruise);
        if let Some(s) = w.naechstes_bereit(t, phase, p.ruhephase(&tk), |s| !(tief && s == Slot::KapitaenBegruessung)) {
            let bis = t + dauer(s);
            if s.loest_pause_aus() {
                w.ansage_spielt_bis(bis);
            } else {
                w.spielt_bis(bis);
            }
            ende = Some((bis, s));
            gespielt.entry(s).or_insert(t);
        }
        let _ = w.verfallene_abholen();
        let _ = p.entfallene_abholen();
        if rundlauf {
            let wieder: Planer = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
            assert!(wieder == p, "Rundlauf veraendert den Planer bei t={t}");
            p = wieder;
        }
    }
    let mut zeilen: Vec<_> = gespielt.iter().collect();
    zeilen.sort_by_key(|(_, t)| **t);
    for (s, t) in zeilen {
        println!("{} {s:?} (gemeldet {})", uhr(*t), uhr(gemeldet[s]));
    }
    (d, Ablauf { gespielt, gemeldet })
}

fn um(s: &str) -> i64 {
    let teile: Vec<i64> = s.split(':').map(|x| x.parse().unwrap()).collect();
    teile[0] * 3600 + teile[1] * 60 + teile[2]
}

fn tageszeit(t: i64) -> i64 {
    t.rem_euclid(86_400)
}

/// Michael LTU181: Anschnallzeichen 15:49:56 unter FL100 aus, Ansage kam
/// erst 15:51:14 nach dem Gong. Jetzt direkt nach dem Schalter.
#[test]
fn ltu181_anschnallzeichen_aus_unter_fl100() {
    let (_, a) = fliegen(include_str!("daten/ltu181.json"), 32, 17_000);
    let aus = tageszeit(a.gespielt[&Slot::AnschnallzeichenAus]);
    assert!(aus < um("15:51:10"), "vor FL100 (15:51:10), war {}", uhr(aus));
    assert!(tageszeit(a.gemeldet[&Slot::AnschnallzeichenAus]) >= um("15:49:55"), "nicht vor dem Schalter");
}

/// Peter DLH16: Anschnallzeichen 14:29:57 an, Passagiere stiegen 14:41–14:50
/// ein. Boarding folgt dem Einsteigen, „Boarding completed“ kurz vor „Türen
/// zu“ beim Beacon 14:52:23 (2.1.1 E), „Plätze für den Start“ am Landelicht
/// 15:11:18.
#[test]
fn dlh16_boarding_folgt_dem_einsteigen() {
    let (_, a) = fliegen(include_str!("daten/dlh16.json"), 40, 29_000);
    let boarding = tageszeit(a.gespielt[&Slot::Boarding]);
    assert!((um("14:41:30")..um("14:43:30")).contains(&boarding), "Boarding beim Einsteigen, war {}", uhr(boarding));
    let fertig = tageszeit(*a.gespielt.get(&Slot::BoardingAbgeschlossen).expect("Boarding completed kam"));
    assert!((um("14:52:23")..um("14:52:40")).contains(&fertig), "beim Beacon: {}", uhr(fertig));
    assert!(tageszeit(a.gespielt[&Slot::KommandoTuerenScharf]) > fertig);
    assert!(fertig < tageszeit(a.gespielt[&Slot::BegruessungTuerenZu]));
    let start = tageszeit(a.gespielt[&Slot::KommandoStart]);
    assert!((um("15:11:15")..um("15:11:40")).contains(&start), "Plätze für den Start am Landelicht, war {}", uhr(start));
}

/// A346 DLH418: Halt mit vier laufenden Triebwerken auf dem Rollweg
/// (BlocksOn 20:56:45), echte Parkposition 20:59:39, Triebwerke aus ~21:00.
#[test]
fn dlh418_tueren_erst_bei_stehenden_triebwerken() {
    let (_, a) = fliegen(include_str!("daten/dlh418.json"), 495, 38_000);
    let tueren = tageszeit(a.gespielt[&Slot::KommandoTuerenEntschaerfen]);
    assert!(tueren >= um("20:59:39"), "nicht beim Halt auf dem Rollweg, war {}", uhr(tueren));
}

/// QAF446: Sicherheit bis ~12:23, Startlauf 12:24:14. Mit 30 s Pause kam
/// der Kapitän 2.0.28 noch am Boden. Seit 2.1.1 (A) steht die Zuladung hier
/// schon beim Flugstart: Die Begrüßung kommt erst beim Beacon 12:18:32, davor
/// nichts; danach Begrüßung, „Boarding completed“, „Türen zu“ und Sicherheit
/// noch am Boden, der Kapitän (wie vorgesehen) nach dem Start über FL100.
#[test]
fn qaf446_vorab_geladen_alles_noch_vor_dem_start() {
    let (d, a) = fliegen(include_str!("daten/qaf446.json"), 410, 38_000);
    let beacon = d.ticks.iter().find(|x| x.7 == 1).expect("Beacon").0;
    assert_eq!(a.gespielt[&Slot::Boarding], beacon, "Begrüßung beim Beacon");
    for s in [Slot::BoardingAbgeschlossen, Slot::KommandoTuerenScharf, Slot::BegruessungTuerenZu, Slot::Sicherheit] {
        assert!(a.gespielt[&s] > beacon && a.gespielt[&s] < d.abheben, "{s:?} am Boden");
    }
    assert!(a.gespielt[&Slot::BegruessungTuerenZu] - (a.gespielt[&Slot::Boarding] + 60) <= 15, "10 s nach der Begrüßung, nicht 90 s");
    let kapitaen = a.gespielt[&Slot::KapitaenBegruessung];
    assert!(kapitaen > d.abheben, "Kapitän nach dem Start, war {}", uhr(kapitaen));
}

/// AFR218: Reiseflughöhe nach ~25 min, erster Service bisher bei +121 min.
#[test]
fn afr218_service_kurz_nach_reiseflughoehe() {
    let (d, a) = fliegen(include_str!("daten/afr218.json"), 510, 38_000);
    let reiseflug = d.ticks.iter().find(|x| x.1 == FlightPhase::Cruise).expect("Reiseflug").0;
    let service = a.gespielt[&Slot::Reiseflug25];
    assert!(service - reiseflug <= 12 * 60, "Service {} min nach Reiseflugbeginn", (service - reiseflug) / 60);
    assert!(service - d.abheben < 60 * 60, "statt +121 min jetzt +{} min", (service - d.abheben) / 60);
}

/// Thomas 19.10.: Der zweite Service (AFR: `zweiter_service`) beginnt rund
/// 90 min vor dem Sinkflug — bisher 12 min davor wie das Ende des Bordverkaufs.
#[test]
fn afr218_zweiter_service_neunzig_minuten_vor_dem_sinkflug() {
    let (d, a) = fliegen_mit(include_str!("daten/afr218.json"), 510, 38_000, Some(true));
    let sinkflug = d.ticks.iter().find(|x| x.1 == FlightPhase::Descent).expect("Sinkflug").0;
    let zweiter = a.gespielt[&Slot::Reiseflug80];
    let vorlauf_min = (sinkflug - zweiter) / 60;
    assert!((60..=120).contains(&vorlauf_min), "zweiter Service {vorlauf_min} min vor dem Sinkflug");
    assert!(a.gespielt[&Slot::Reiseflug50] < zweiter, "nach der Halbzeit");
}

/// Peter DLH454 (19.10., noch 2.0.27): Reiseflug 80 fiel weg, weil er vor
/// TOD − 12 in den Sinkflug ging. Mit dem zweiten Service bei TOD − 90 kommt
/// er weit vorher. „Seats for take-off“ am Landelicht: 2 min vor dem Startlauf.
#[test]
fn dlh454_zweiter_service_trotz_fruehem_sinkflug() {
    let (d, a) = fliegen_mit(include_str!("daten/dlh454.json"), 401, 39_000, Some(true));
    let sinkflug = d.ticks.iter().find(|x| x.1 == FlightPhase::Descent).expect("Sinkflug").0;
    let zweiter = *a.gespielt.get(&Slot::Reiseflug80).expect("zweiter Service kam");
    assert!(sinkflug - zweiter >= 60 * 60, "zweiter Service {} min vor dem Sinkflug", (sinkflug - zweiter) / 60);
    let start = a.gespielt[&Slot::KommandoStart];
    assert!((90..=180).contains(&(d.abheben - start)), "Seats for take-off {} s vor dem Startlauf", d.abheben - start);
    // 2.1.0: Ende des Service kommt vor der Sinkflug-Ansage des Kapitäns.
    let ende = *a.gespielt.get(&Slot::EndeService).expect("Ende des Service kam");
    assert!(ende < a.gespielt[&Slot::KapitaenSinkflug], "vor dem Kapitän");
    assert!(ende > zweiter, "nach dem zweiten Service");
}

/// v3 Etappe 4 (P-3.58): Den Planer nach jedem Takt sichern und
/// wiederherstellen aendert keinen Flug — gleiche Ansagen zur gleichen Zeit.
#[test]
fn rundlauf_aendert_nichts() {
    for (datei, min, ft, zweiter) in [
        (include_str!("daten/ltu181.json"), 165, 36_000, None),
        (include_str!("daten/dlh16.json"), 100, 37_000, None),
        (include_str!("daten/dlh418.json"), 500, 37_000, None),
        (include_str!("daten/qaf446.json"), 600, 38_000, None),
        (include_str!("daten/afr218.json"), 600, 37_000, Some(true)),
        (include_str!("daten/dlh454.json"), 600, 38_000, Some(true)),
    ] {
        let (_, normal) = fliegen_rundlauf(datei, min, ft, zweiter, false);
        let (_, rund) = fliegen_rundlauf(datei, min, ft, zweiter, true);
        assert_eq!(normal.gespielt, rund.gespielt);
        assert_eq!(normal.gemeldet, rund.gemeldet);
        assert!(!normal.gespielt.is_empty());
    }
}
