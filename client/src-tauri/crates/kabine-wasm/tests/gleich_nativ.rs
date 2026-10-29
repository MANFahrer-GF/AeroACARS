//! v3 Etappe 4 (P-3.58): Das Kabinenmodul `bundle/module/kabine.wasm`
//! plant wie der Planer nativ — Takt fuer Takt ueber die sechs echten Fluege
//! aus `cabin-announcements/tests/fluege_2028.rs`, mit allen Abfragen, die
//! der Client je Takt stellt.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use bewertung_host::{Host, Instanz};
use cabin_announcements::flugart::Flugart;
use cabin_announcements::modul::Befehl;
use cabin_announcements::slot::{Rolle, Slot};
use cabin_announcements::{Entfall, Faellig, Flugdaten, Planer, Tick, Warteschlange};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sim_core::FlightPhase;

const MODUL: &[u8] = include_bytes!("../../../../../bundle/module/kabine.wasm");
const TABELLE: &str = cabin_announcements::EINGEBAUT;
const VIEL: u64 = 1_000_000_000;

type Takt = (i64, FlightPhase, f64, f64, f64, u8, Option<u8>, u8, u8, f64, u8, u8, f64, Option<u8>);

#[derive(Deserialize)]
struct Daten {
    landung: i64,
    ticks: Vec<Takt>,
}

/// Die sechs Fluege wie in `rundlauf_aendert_nichts`.
fn fluege() -> [(&'static str, &'static str, u32, u32, Option<bool>); 6] {
    [
        ("ltu181", include_str!("../../cabin-announcements/tests/daten/ltu181.json"), 165, 36_000, None),
        ("dlh16", include_str!("../../cabin-announcements/tests/daten/dlh16.json"), 100, 37_000, None),
        ("dlh418", include_str!("../../cabin-announcements/tests/daten/dlh418.json"), 500, 37_000, None),
        ("qaf446", include_str!("../../cabin-announcements/tests/daten/qaf446.json"), 600, 38_000, None),
        ("afr218", include_str!("../../cabin-announcements/tests/daten/afr218.json"), 600, 37_000, Some(true)),
        ("dlh454", include_str!("../../cabin-announcements/tests/daten/dlh454.json"), 600, 38_000, Some(true)),
    ]
}

/// Das Modul ueber den Befehlskanal, mit Messung je Befehlsart.
struct Modul {
    instanz: Instanz,
    /// Befehlsart -> (Fuel, Zeit) je Aufruf.
    messung: HashMap<String, Vec<(u64, Duration)>>,
}

impl Modul {
    fn neu(wasm: &[u8]) -> Self {
        let host = Host::neu_mit(wasm, &["memory", "alloc", "kabine"]).unwrap();
        Self { instanz: host.instanz("kabine", VIEL).unwrap(), messung: HashMap::new() }
    }

    fn rufen<T: DeserializeOwned>(&mut self, b: Befehl) -> T {
        let json = serde_json::to_string(&b).unwrap();
        let art = serde_json::from_str::<serde_json::Value>(&json).unwrap()["befehl"].as_str().unwrap().to_string();
        let start = Instant::now();
        let (aus, fuel) = self.instanz.aufrufen(&json, VIEL).unwrap();
        self.messung.entry(art).or_default().push((fuel, start.elapsed()));
        match serde_json::from_str::<Result<T, String>>(&aus).unwrap() {
            Ok(t) => t,
            Err(e) => panic!("Modul: {e} bei {json}"),
        }
    }
}

fn dauer(s: Slot) -> i64 {
    match s.rolle() {
        Rolle::Chime => 3,
        Rolle::Kommando => 4,
        _ if s == Slot::Sicherheit => 190,
        _ => 60,
    }
}

/// Was der Client je Takt abfragt (kabinenansagen.rs: kommt_noch, erledigt,
/// zeitpunkt, Prognose, Restflug, Kurzstrecke, Flugzeit bekannt).
#[derive(Debug, PartialEq)]
struct Abfragen {
    kommt_noch: Vec<bool>,
    erledigt: Vec<bool>,
    zeitpunkt: Vec<Option<i64>>,
    prognose: Option<i64>,
    rest: Option<f64>,
    kurz: bool,
    bekannt: bool,
    tueren_zu: bool,
}

fn abfragen_nativ(p: &Planer, t: &Tick, landung: Option<i64>) -> Abfragen {
    Abfragen {
        kommt_noch: Slot::ALLE.iter().map(|s| p.kommt_noch(*s, t.phase)).collect(),
        erledigt: Slot::ALLE.iter().map(|s| p.erledigt(*s)).collect(),
        zeitpunkt: Slot::ALLE.iter().map(|s| p.zeitpunkt(*s, landung)).collect(),
        prognose: p.kapitaen_sinkflug_prognose(landung),
        rest: p.restflug_min(t.jetzt, t.rest_min),
        kurz: p.ist_kurzstrecke(),
        bekannt: p.flugzeit_bekannt(),
        tueren_zu: p.tueren_zu(),
    }
}

fn abfragen_modul(m: &mut Modul, t: &Tick, landung: Option<i64>) -> Abfragen {
    Abfragen {
        kommt_noch: Slot::ALLE.iter().map(|s| m.rufen(Befehl::KommtNoch { slot: *s, phase: t.phase })).collect(),
        erledigt: Slot::ALLE.iter().map(|s| m.rufen(Befehl::Erledigt { slot: *s })).collect(),
        zeitpunkt: Slot::ALLE.iter().map(|s| m.rufen(Befehl::Zeitpunkt { slot: *s, landung_erwartet: landung })).collect(),
        prognose: m.rufen(Befehl::KapitaenSinkflugPrognose { landung_erwartet: landung }),
        rest: m.rufen(Befehl::RestflugMin { jetzt: t.jetzt, rest_min: t.rest_min }),
        kurz: m.rufen(Befehl::IstKurzstrecke),
        bekannt: m.rufen(Befehl::FlugzeitBekannt),
        tueren_zu: m.rufen(Befehl::TuerenZu),
    }
}

/// Fliegt einen Flug mit Planer nativ und Modul im Gleichschritt (eine
/// Warteschlange, vom nativen Planer gefuehrt) und vergleicht jeden Takt.
/// `jeder_n`: Abfragen nur in jedem n-ten Takt (Laufzeit der Debug-Tests;
/// die CI-Pruefung des frischen Moduls fragt jeden Takt, im Release-Profil).
fn gleichschritt(name: &str, datei: &str, flugzeit_min: u32, reise_ft: u32, zweiter: Option<bool>, m: &mut Modul, jeder_n: usize) -> usize {
    let d: Daten = serde_json::from_str(datei).unwrap();
    let flug = Flugdaten {
        flugart: Some(Flugart::Linie),
        geplante_flugzeit_min: Some(flugzeit_min),
        tod_nach_abheben_s: None,
        ziel_plan_icao: Some("ZIEL".into()),
        reiseflughoehe_ft: Some(reise_ft),
        abheben_epoch: None,
    };
    let erste = d.ticks[0].1;
    let mut p = Planer::fortsetzen(flug.clone(), erste);
    let () = m.rufen(Befehl::Fortsetzen { tabelle: TABELLE.into(), flug, phase: erste });
    p.reiseflug80_art(zweiter);
    let () = m.rufen(Befehl::Reiseflug80Art { zweiter_service: zweiter });
    let mut w = Warteschlange::default();
    let mut ende: Option<(i64, Slot)> = None;
    let mut gemeldet = 0;
    for (i, &(t, phase, alt, agl, vs, boden, zeichen, beacon, licht, fahrwerk, pushback, triebwerke, zfw, _)) in d.ticks.iter().enumerate() {
        if let Some((e, s)) = ende {
            if t >= e {
                if s.loest_pause_aus() {
                    w.ansage_zu_ende_nach(e, s);
                } else {
                    w.frei();
                }
                if s == Slot::KapitaenSinkflug {
                    p.kapitaen_sinkflug_ende(true, e);
                    let () = m.rufen(Befehl::KapitaenSinkflugEnde { gespielt: true, jetzt: e });
                }
                if s == Slot::Reiseflug25 {
                    p.service_ende(true);
                    let () = m.rufen(Befehl::ServiceEnde { gespielt: true });
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
        // Wie `kabinen_schritt` im Client: abwaehlen, Art, Takt, Ruhephase.
        p.abwaehlen([]);
        let () = m.rufen(Befehl::Abwaehlen { slots: Vec::new() });
        p.reiseflug80_art(zweiter);
        let () = m.rufen(Befehl::Reiseflug80Art { zweiter_service: zweiter });
        let neu = p.tick(&tk);
        let neu_m: Vec<Faellig> = m.rufen(Befehl::Tick { tick: tk.clone() });
        assert_eq!(neu, neu_m, "{name} t={t}: Tick");
        let ruhe = p.ruhephase(&tk);
        assert_eq!(ruhe, m.rufen::<bool>(Befehl::Ruhephase { tick: tk.clone() }), "{name} t={t}: Ruhephase");
        let entf = p.entfallene_abholen();
        assert_eq!(entf, m.rufen::<Vec<(Slot, Entfall)>>(Befehl::EntfalleneAbholen), "{name} t={t}: entfallen");
        if i % jeder_n == 0 {
            let landung = Some(d.landung);
            assert_eq!(abfragen_nativ(&p, &tk, landung), abfragen_modul(m, &tk, landung), "{name} t={t}: Abfragen");
        }
        gemeldet += neu.len();
        w.aufnehmen(neu);
        let tief = boden == 0 && alt < 10_000.0 && cabin_announcements::planer::rang(phase) < cabin_announcements::planer::rang(FlightPhase::Cruise);
        if let Some(s) = w.naechstes_bereit(t, phase, ruhe, |s| !(tief && s == Slot::KapitaenBegruessung)) {
            let bis = t + dauer(s);
            if s.loest_pause_aus() {
                w.ansage_spielt_bis(bis);
            } else {
                w.spielt_bis(bis);
            }
            ende = Some((bis, s));
        }
        let _ = w.verfallene_abholen();
    }
    // Am Ende: derselbe Zustand, Feld fuer Feld.
    let stand: String = m.rufen(Befehl::Sichern);
    assert!(serde_json::from_str::<Planer>(&stand).unwrap() == p, "{name}: Endstand");
    gemeldet
}

/// P-3.58: Modul und Planer nativ liefern je Takt dieselben Ausgaben.
#[test]
fn modul_plant_wie_nativ() {
    for (name, datei, min, ft, zweiter) in fluege() {
        let mut m = Modul::neu(MODUL);
        assert!(gleichschritt(name, datei, min, ft, zweiter, &mut m, 20) > 0, "{name}: nichts gemeldet");
    }
}

/// CI (Schritt „Kabinenmodul gleich dem Quellstand“): Ein frisch aus dem
/// Quellstand gebautes Modul (`KABINE_WASM_FRISCH`) plant alle Fluege wie
/// das eingecheckte und wie der Planer nativ (bytegleich sind Mac- und
/// Linux-Build nicht, siehe Bewertungsmodul).
#[test]
fn frisch_gebautes_modul_rechnet_wie_das_eingecheckte() {
    let Ok(pfad) = std::env::var("KABINE_WASM_FRISCH") else {
        return;
    };
    let frisch = std::fs::read(pfad).unwrap();
    for (name, datei, min, ft, zweiter) in fluege() {
        let (mut a, mut b) = (Modul::neu(&frisch), Modul::neu(MODUL));
        gleichschritt(name, datei, min, ft, zweiter, &mut a, 1);
        gleichschritt(name, datei, min, ft, zweiter, &mut b, 1);
        // Inhalt vergleichen: Die Reihenfolge in `gemeldet` (HashSet) ist frei.
        let stand = |m: &mut Modul| serde_json::from_str::<Planer>(&m.rufen::<String>(Befehl::Sichern)).unwrap();
        assert!(stand(&mut a) == stand(&mut b), "{name}: Endstand");
    }
}

/// Die Tabelle kommt aus dem Befehl, nicht aus der im Modul einkompilierten
/// Kopie: mit Kurzstrecke ab 200 min entfaellt die Halbzeit des 165-min-Flugs.
#[test]
fn tabelle_kommt_aus_dem_befehl() {
    let mut t: serde_json::Value = serde_json::from_str(TABELLE).unwrap();
    t["werte"]["kurzstrecke_min"]["wert"] = 200.into();
    let mut m = Modul::neu(MODUL);
    let flug = Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(165), ..Default::default() };
    let () = m.rufen(Befehl::Neu { tabelle: t.to_string(), flug: flug.clone() });
    assert!(m.rufen::<bool>(Befehl::IstKurzstrecke));
    let mut n = Modul::neu(MODUL);
    let () = n.rufen(Befehl::Neu { tabelle: TABELLE.into(), flug });
    assert!(!n.rufen::<bool>(Befehl::IstKurzstrecke));
}

#[test]
fn ohne_planer_und_mit_kaputter_tabelle_kommt_ein_fehler() {
    let mut m = Modul::neu(MODUL);
    let (aus, _) = m.instanz.aufrufen(r#"{"befehl":"tueren_zu"}"#, VIEL).unwrap();
    assert!(aus.contains("kein Planer"), "{aus}");
    let (aus, _) = m.instanz.aufrufen(r#"{"befehl":"neu","tabelle":"{}","flug":{}}"#, VIEL).unwrap();
    assert!(aus.contains("Err"), "{aus}");
}

/// Messung (P-4.32 sinngemaess): Fuel und Zeit je Befehlsart ueber alle
/// Fluege mit allen Abfragen in jedem Takt, Speicher am Anfang und Ende.
/// `cargo test -p kabine-wasm --release --test gleich_nativ messen -- --ignored --nocapture`
#[test]
#[ignore]
fn messen() {
    let mut alle: HashMap<String, Vec<(u64, Duration)>> = HashMap::new();
    let mut takte = 0;
    for (name, datei, min, ft, zweiter) in fluege() {
        let mut m = Modul::neu(MODUL);
        let vorher = m.instanz.speicher_bytes();
        gleichschritt(name, datei, min, ft, zweiter, &mut m, 1);
        takte += m.messung["tick"].len();
        println!("{name}: {} Takte, Speicher {} -> {} KB", m.messung["tick"].len(), vorher / 1024, m.instanz.speicher_bytes() / 1024);
        for (art, v) in m.messung {
            alle.entry(art).or_default().extend(v);
        }
    }
    let mut arten: Vec<_> = alle.into_iter().collect();
    arten.sort_by(|a, b| a.0.cmp(&b.0));
    let mut je_takt = 0u64;
    for (art, mut v) in arten {
        v.sort_by_key(|x| x.0);
        let fuel: Vec<u64> = v.iter().map(|x| x.0).collect();
        let mut zeit: Vec<Duration> = v.iter().map(|x| x.1).collect();
        zeit.sort();
        je_takt += fuel.iter().sum::<u64>();
        println!(
            "{art:28} n={:6} Fuel median {:7} max {:7} | Zeit median {:6.1} us max {:7.1} us",
            v.len(),
            fuel[fuel.len() / 2],
            fuel[fuel.len() - 1],
            zeit[zeit.len() / 2].as_secs_f64() * 1e6,
            zeit[zeit.len() - 1].as_secs_f64() * 1e6
        );
    }
    println!("Fuel je Takt (alle Abfragen): {}", je_takt / takte as u64);
}
