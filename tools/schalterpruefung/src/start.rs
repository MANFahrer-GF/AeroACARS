//! Programmstart (nur Windows): Verbindung, Prüfungen mit klaren deutschen
//! Meldungen, Skriptwahl, Strg+C-Behandlung.

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::ablauf::{self, gross, hinweis, zeile_lesen, Ablauf, ABBRUCH, ABGEBROCHEN};
use crate::bericht::{self, Lauf, LaufInfo};
use crate::mobiflight;
use crate::sim::{self, Sim, BEENDET};
use crate::skripte::{self, Skript};

static LAUF: OnceLock<Arc<Mutex<Lauf>>> = OnceLock::new();
static SIM: OnceLock<Arc<Mutex<Sim>>> = OnceLock::new();

fn sperren<T>(m: &Mutex<T>, max: Duration) -> Option<std::sync::MutexGuard<'_, T>> {
    let ende = Instant::now() + max;
    loop {
        match m.try_lock() {
            Ok(g) => return Some(g),
            Err(std::sync::TryLockError::Poisoned(p)) => return Some(p.into_inner()),
            Err(std::sync::TryLockError::WouldBlock) => {
                if Instant::now() >= ende {
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
}

/// Strg+C oder Fenster schließen: Bericht schreiben, eigene MobiFlight-
/// Clients leeren, beenden. Windows lässt beim Schließen ~5 s Zeit.
fn abbruch_handler() {
    ABBRUCH.store(true, Ordering::SeqCst);
    println!();
    println!("  Abbruch — schreibe, was bisher gemessen wurde …");
    if let Some(lauf) = LAUF.get() {
        if let Some(mut l) = sperren(lauf, Duration::from_secs(3)) {
            match l.abschliessen(ABGEBROCHEN) {
                Ok((j, t)) => {
                    println!("  Bericht: {}", t.display());
                    println!("           {}", j.display());
                }
                Err(e) => println!("  Bericht konnte nicht geschrieben werden: {e}"),
            }
        }
    }
    if let Some(sim) = SIM.get() {
        if let Some(mut s) = sperren(sim, Duration::from_secs(1)) {
            s.aufraeumen();
        }
    }
    std::process::exit(130);
}

fn fehler(zeilen: &[&str]) {
    let mut v = vec!["!!! PROBLEM !!!", ""];
    v.extend_from_slice(zeilen);
    gross(&v);
}

fn schliessen_warten() {
    println!();
    let _ = zeile_lesen("Enter drücken zum Schließen … ");
}

fn berichtsordner() -> PathBuf {
    let kandidaten = [
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("aeroacars-messung"))),
        std::env::current_dir()
            .ok()
            .map(|d| d.join("aeroacars-messung")),
        Some(std::env::temp_dir().join("aeroacars-messung")),
    ];
    for k in kandidaten.into_iter().flatten() {
        if std::fs::create_dir_all(&k).is_ok() {
            // Schreibprobe — ein Netzlaufwerk kann Anlegen erlauben, Schreiben nicht.
            let probe = k.join(".schreibprobe");
            if std::fs::write(&probe, b"ok").is_ok() {
                let _ = std::fs::remove_file(probe);
                return k;
            }
        }
    }
    PathBuf::from(".")
}

fn skript_waehlen(titel: &str, icao: Option<&str>) -> Option<Skript> {
    if let Some(s) = skripte::fuer_titel(titel, icao) {
        gross(&[&format!("Erkanntes Prüfskript: {}", s.name)]);
        let e = zeile_lesen("Enter = passt   M = anderes Skript wählen: ")?;
        if !e.eq_ignore_ascii_case("M") {
            return Some(s);
        }
    } else {
        gross(&[
            "Für dieses Flugzeug gibt es kein eigenes Prüfskript.",
            "Bitte wählen:",
        ]);
    }
    let alle = skripte::alle();
    loop {
        for (i, s) in alle.iter().enumerate() {
            hinweis(&format!("  {} = {}", i + 1, s.name));
        }
        let e = zeile_lesen(&format!("Nummer + Enter (nur Enter = {}): ", alle.len()))?;
        if e.is_empty() {
            return alle.last().cloned();
        }
        match e.parse::<usize>() {
            Ok(n) if (1..=alle.len()).contains(&n) => return Some(alle[n - 1].clone()),
            _ => hinweis("Bitte eine der Nummern eingeben."),
        }
    }
}

pub fn run() -> i32 {
    gross(&[
        "AeroACARS-Schalterprüfung",
        "Findet heraus, welche Simulator-Variable zu welchem Cockpitschalter gehört.",
        "Das Programm LIEST nur — es verstellt nichts im Simulator.",
        "Beenden jederzeit mit Strg+C (das bisher Gemessene wird gespeichert).",
    ]);

    if let Err(e) = ctrlc::set_handler(abbruch_handler) {
        hinweis(&format!(
            "(Hinweis: Strg+C-Behandlung nicht verfügbar: {e})"
        ));
    }

    if let Err(e) = sim::dll_bereitstellen() {
        fehler(&["SimConnect konnte nicht vorbereitet werden.", &e]);
        schliessen_warten();
        return 2;
    }

    // 1. MSFS
    hinweis("Verbinde mit MSFS …");
    let mut sim = loop {
        match Sim::verbinden() {
            Ok(s) => break s,
            Err(e) => {
                fehler(&[
                    "MSFS läuft nicht (oder ist noch nicht fertig gestartet).",
                    "Bitte MSFS 2020/2024 starten und einen Flug laden.",
                    &format!("(technisch: {e})"),
                ]);
                if zeile_lesen("Enter = nochmal versuchen: ").is_none() {
                    return 1;
                }
            }
        }
    };
    hinweis(&format!("Verbunden mit: {}", sim.simulator));

    // 2. Flugzeug
    let (titel, atc_model) = loop {
        match sim.flugzeug() {
            Ok(Some(x)) => break x,
            Ok(None) => {
                fehler(&[
                    "Kein Flugzeug geladen.",
                    "Bitte einen Flug starten und warten, bis du im Cockpit sitzt.",
                ]);
                if zeile_lesen("Enter = nochmal prüfen: ").is_none() {
                    return 1;
                }
            }
            Err(e) => {
                fehler(&[&e]);
                schliessen_warten();
                return 1;
            }
        }
    };
    let icao = mobiflight::icao_aus_atc_model(&atc_model);
    gross(&[
        &format!("Flugzeug (TITLE): {titel}"),
        &format!("ATC MODEL:        {atc_model}"),
        &format!(
            "ICAO:             {}",
            icao.clone().unwrap_or_else(|| "?".into())
        ),
    ]);

    // 3. MobiFlight
    hinweis("Prüfe MobiFlight-Modul (MF.Ping) …");
    loop {
        match sim.mobiflight_pruefen() {
            Ok(true) => break,
            Ok(false) => {
                fehler(&[
                    "Das MobiFlight-Modul antwortet nicht (kein MF.Pong in 5 Sekunden).",
                    "Bitte prüfen:",
                    " - Ordner „mobiflight-event-module“ liegt im Community-Ordner",
                    " - MSFS wurde danach neu gestartet",
                    " - ein Flug ist geladen (nicht das Hauptmenü)",
                ]);
                if zeile_lesen("Enter = nochmal versuchen: ").is_none() {
                    return 1;
                }
            }
            Err(e) => {
                fehler(&[&e]);
                schliessen_warten();
                return 1;
            }
        }
    }
    let mf_version = sim.mf_version.clone().unwrap_or_else(|| "unbekannt".into());
    hinweis(&format!(
        "MobiFlight-Modul antwortet (Version {mf_version})."
    ));

    // 4. LVar-Liste
    let lvars = loop {
        hinweis("Hole die LVar-Liste …");
        match sim.lvars_holen() {
            Ok(l) if l.len() >= 10 => break l,
            Ok(l) => {
                fehler(&[
                    &format!("Nur {} LVars gefunden.", l.len()),
                    "Das Flugzeug ist vielleicht noch nicht fertig geladen.",
                ]);
                let e = match zeile_lesen("Enter = nochmal holen   W = trotzdem weiter: ") {
                    Some(e) => e,
                    None => return 1,
                };
                if e.eq_ignore_ascii_case("W") {
                    break l;
                }
            }
            Err(e) => {
                fehler(&[&e]);
                schliessen_warten();
                return 1;
            }
        }
    };
    let gekappt = lvars.len() >= mobiflight::LISTE_MAX;
    hinweis(&format!("{} LVars gefunden.", lvars.len()));
    if gekappt {
        hinweis("(Das MobiFlight-Modul listet höchstens 1000 — es kann weitere geben.)");
    }

    // 5. Abonnieren
    hinweis("Melde alle LVars bei MobiFlight an …");
    if let Err(e) = sim.abonnieren(&lvars, |f, g| hinweis(&format!("   {f} von {g}"))) {
        fehler(&["Anmelden der LVars ist gescheitert.", &e]);
        schliessen_warten();
        return 1;
    }

    // 6. Skript
    let Some(skript) = skript_waehlen(&titel, icao.as_deref()) else {
        return 1;
    };

    let ordner = berichtsordner();
    let info = LaufInfo {
        werkzeug_version: env!("CARGO_PKG_VERSION").to_string(),
        simulator: sim.simulator.clone(),
        titel: titel.clone(),
        atc_model,
        icao,
        mobiflight_version: mf_version,
        lvars_gesamt: lvars.len(),
        lvar_liste_moeglicherweise_gekappt: gekappt,
        lvars_uebersprungen: sim.uebersprungen.clone(),
        simvars_abgelehnt: sim.abgelehnt.clone(),
        skript: skript.name.to_string(),
        beginn: bericht::jetzt(),
    };
    let lauf = Arc::new(Mutex::new(Lauf::neu(info, sim.variablen(), ordner.clone())));
    {
        let mut l = lauf.lock().unwrap();
        l.start_ereignis();
        hinweis(&format!("Berichte landen in: {}", ordner.display()));
        hinweis(&format!("Live-Protokoll:     {}", l.live_pfad().display()));
        if let Some(e) = &l.live_fehler {
            hinweis(&format!("(Live-Protokoll nicht schreibbar: {e})"));
        }
    }
    let sim = Arc::new(Mutex::new(sim));
    let _ = LAUF.set(lauf.clone());
    let _ = SIM.set(sim.clone());

    let sim_m = sim.clone();
    let mut quelle = move || sim_m.lock().unwrap_or_else(|e| e.into_inner()).messen();
    let ergebnis = {
        let mut ab = Ablauf {
            lauf: lauf.clone(),
            messen: &mut quelle,
        };
        ab.ruhemessung().and_then(|_| ab.skript(&skript))
    };

    let grund = match &ergebnis {
        Ok(()) => "fertig".to_string(),
        Err(e) => e.clone(),
    };
    if ABBRUCH.load(Ordering::SeqCst) {
        // Der Handler schreibt und beendet; hier nur nicht dazwischenfunken.
        std::thread::sleep(Duration::from_secs(10));
    }
    let pfade = lauf
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .abschliessen(&grund);
    sim.lock().unwrap_or_else(|e| e.into_inner()).aufraeumen();

    match &ergebnis {
        Ok(()) => gross(&["Fertig!"]),
        Err(e) if e == BEENDET => {
            fehler(&["MSFS wurde beendet — das bisher Gemessene ist gespeichert."])
        }
        Err(e) if e == ablauf::ABGEBROCHEN => {}
        Err(e) => fehler(&["Der Lauf wurde unterbrochen:", e]),
    }
    match pfade {
        Ok((j, t)) => {
            hinweis(&format!("Bericht (lesbar): {}", t.display()));
            hinweis(&format!("Bericht (JSON):   {}", j.display()));
        }
        Err(e) => hinweis(&format!("Bericht konnte nicht geschrieben werden: {e}")),
    }
    schliessen_warten();
    if ergebnis.is_ok() {
        0
    } else {
        1
    }
}
