//! Planer und Warteschlange an echten Flugspuren (Sprit-Korpus, nicht im
//! Repo). Die Phasen werden grob aus der Spur abgeleitet — das ist NICHT
//! die Phasen-Engine von AeroACARS, sondern eine Näherung, um die
//! Zeitregeln (Reiseflug-Prozente, Sinkflug, Ruhephase, Verfall) an echten
//! Flugdauern zu sehen.
//!
//!     KABINEN_KORPUS=~/Claude/aeroacars-korpus/sprit-2026-09-18 \
//!     cargo test -p cabin-announcements --test korpus -- --ignored --nocapture

use std::collections::BTreeMap;

use cabin_announcements::flugart::Flugart;
use cabin_announcements::slot::Slot;
use cabin_announcements::{Flugdaten, Planer, Tick, Warteschlange};
use sim_core::FlightPhase;

#[derive(Clone, Copy)]
struct Probe {
    t: i64,
    alt: f64,
    gs: f64,
    agl: f64,
}

fn phasen(spur: &[Probe]) -> Vec<FlightPhase> {
    use FlightPhase::*;
    let max_alt = spur.iter().map(|p| p.alt).fold(0.0, f64::max);
    let mut aus = Vec::with_capacity(spur.len());
    let mut ph = Boarding;
    let mut war_oben = false;
    let mut stand_seit: Option<i64> = None;
    for (i, p) in spur.iter().enumerate() {
        let vs = if i > 0 {
            let q = spur[i - 1];
            let dt = (p.t - q.t).max(1) as f64;
            (p.alt - q.alt) / dt * 60.0
        } else {
            0.0
        };
        let boden = p.agl < 30.0 && p.gs < 200.0;
        ph = match ph {
            Boarding if p.gs > 1.0 => Pushback,
            Pushback if p.gs > 6.0 => TaxiOut,
            TaxiOut if boden && p.gs > 40.0 => TakeoffRoll,
            TakeoffRoll if !boden => Takeoff,
            Takeoff if p.agl > 1500.0 => Climb,
            Climb if p.alt > max_alt - 1500.0 && vs.abs() < 500.0 => {
                war_oben = true;
                Cruise
            }
            Cruise if p.alt < max_alt - 3000.0 && vs < -300.0 => Descent,
            Descent if p.agl < 4000.0 => Approach,
            Approach if p.agl < 1000.0 => Final,
            Final if boden => Landing,
            Landing if p.gs < 40.0 => TaxiIn,
            TaxiIn => {
                if p.gs < 1.0 {
                    let s = *stand_seit.get_or_insert(p.t);
                    if p.t - s > 30 { BlocksOn } else { TaxiIn }
                } else {
                    stand_seit = None;
                    TaxiIn
                }
            }
            BlocksOn if stand_seit.is_some_and(|s| p.t - s > 120) => Arrived,
            andere => andere,
        };
        let _ = war_oben;
        aus.push(ph);
    }
    aus
}

#[test]
#[ignore = "braucht den Sprit-Korpus (Flugspuren, nicht im Repo)"]
fn planer_an_echten_fluegen() {
    let Ok(dir) = std::env::var("KABINEN_KORPUS") else {
        panic!("KABINEN_KORPUS nicht gesetzt");
    };
    let text = std::fs::read_to_string(format!("{dir}/tracks.tsv")).expect("tracks.tsv");
    let mut fluege: BTreeMap<String, Vec<Probe>> = BTreeMap::new();
    for z in text.lines() {
        let f: Vec<&str> = z.split('\t').collect();
        if f.len() < 8 {
            continue;
        }
        let (Ok(t), Ok(alt), Ok(gs), Ok(agl)) = (f[1].parse(), f[4].parse(), f[6].parse(), f[7].parse()) else {
            continue;
        };
        fluege.entry(f[0].to_string()).or_default().push(Probe { t, alt, gs, agl });
    }

    let mut auffaellig = 0;
    for (pid, mut spur) in fluege {
        spur.sort_by_key(|p| p.t);
        let ph = phasen(&spur);
        let ab = spur.iter().zip(&ph).find(|(_, p)| **p == FlightPhase::Takeoff).map(|(s, _)| s.t);
        let land = spur.iter().zip(&ph).find(|(_, p)| **p == FlightPhase::Landing).map(|(s, _)| s.t);
        let (Some(ab), Some(land)) = (ab, land) else {
            println!("{pid}: Phasen nicht ableitbar, übersprungen");
            continue;
        };
        let flugzeit = ((land - ab) / 60) as u32;
        let max_alt = spur.iter().map(|p| p.alt).fold(0.0, f64::max);
        let mut planer = Planer::neu(Flugdaten {
            flugart: Some(Flugart::Linie),
            geplante_flugzeit_min: Some(flugzeit),
            tod_nach_abheben_s: None,
            ziel_plan_icao: Some("ZIEL".into()),
            reiseflughoehe_ft: Some(max_alt as u32),
            abheben_epoch: None,
        });
        let mut w = Warteschlange::default();
        let mut zeitachse = Vec::new();
        let mut verfallen = Vec::new();
        for (p, phase) in spur.iter().zip(&ph) {
            let zeichen = if p.alt > 10_000.0 && matches!(phase, FlightPhase::Cruise) { 0 } else { 2 };
            let tick = Tick {
                jetzt: p.t,
                phase: *phase,
                hoehe_ft: p.alt,
                vs_fpm: 0.0,
                am_boden: p.agl < 30.0,
                anschnallzeichen: Some(zeichen),
                pushback_aktiv: matches!(phase, FlightPhase::Pushback),
                rest_min: (p.t < land).then(|| (land - p.t) as f64 / 60.0),
                ziel_icao: None,
                abweichung_off_block_min: None,
                durchgestartet: false,
                hoehe_agl_ft: Some(p.agl),
                ..Default::default()
            };
            w.aufnehmen(planer.tick(&tick));
            if let Some(s) = w.naechstes(p.t, *phase, planer.ruhephase(&tick)) {
                // Sprechdauer: 70 % der Höchstlänge, mindestens drei Sekunden.
                let dauer = ((s.max_zeichen() as f64 / 14.0) * 0.7).max(3.0) as i64;
                w.spielt_bis(p.t + dauer);
                zeitachse.push(format!("{:>+5.0} min {:?} {:?}", (p.t - ab) as f64 / 60.0, phase, s));
            }
            verfallen.extend(w.verfallene_abholen());
        }
        println!("\n=== {pid}: {flugzeit} min Flug, Reiseflug {max_alt:.0} ft ===");
        for z in &zeitachse {
            println!("  {z}");
        }
        if !verfallen.is_empty() {
            println!("  verfallen: {verfallen:?}");
        }
        // Grobe Plausibilität: Begrüßung und Landung müssen vorkommen.
        let alles = zeitachse.join(" ");
        for muss in ["KapitaenBegruessung", "NachDerLandung", "KapitaenSinkflug"] {
            if !alles.contains(muss) {
                println!("  !! fehlt: {muss}");
                auffaellig += 1;
            }
        }
        let _ = Slot::ALLE;
    }
    println!("\nauffällig: {auffaellig}");
}
