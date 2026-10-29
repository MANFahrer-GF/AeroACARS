//! Jeder registrierte Tauri-Befehl hat genau eine Rechte-Stufe in
//! `befehle.json` (Pflichtenheft P-8.10/P-8.11, Anhang G Kap. 8).
//!
//! Etappe 5 gibt der nachgeladenen Oberflaeche nur die Stufen „lesen“ und
//! „fachlich“; „system“ bleibt der Huelle. Ein neuer Befehl ohne Eintrag
//! wuerde sonst still in keiner Stufe landen — dieser Test haelt ihn auf.

use std::collections::{BTreeMap, BTreeSet};

fn registriert() -> BTreeSet<String> {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let start = src.find("generate_handler![").expect("generate_handler! fehlt") + "generate_handler![".len();
    let ende = start + src[start..].find(']').unwrap();
    src[start..ende]
        .lines()
        .map(|z| z.split("//").next().unwrap())
        .flat_map(|z| z.split(','))
        .map(|s| s.trim().rsplit("::").next().unwrap().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn liste() -> BTreeMap<String, String> {
    let roh = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/befehle.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&roh).unwrap();
    assert_eq!(v["fassung"], 1);
    serde_json::from_value(v["befehle"].clone()).unwrap()
}

#[test]
fn jeder_befehl_hat_genau_eine_stufe() {
    let reg = registriert();
    let liste = liste();
    assert!(reg.len() > 150, "Registrierung nicht gelesen: {}", reg.len());
    let ohne: Vec<_> = reg.iter().filter(|b| !liste.contains_key(*b)).collect();
    let tot: Vec<_> = liste.keys().filter(|b| !reg.contains(*b)).collect();
    assert!(ohne.is_empty(), "registriert, aber ohne Stufe in befehle.json: {ohne:?}");
    assert!(tot.is_empty(), "in befehle.json, aber nicht registriert: {tot:?}");
    for (b, s) in &liste {
        assert!(["lesen", "fachlich", "system"].contains(&s.as_str()), "{b}: unbekannte Stufe {s}");
    }
}

/// P-8.10: Pfade, Geheimnisse, Einwilligung, Autostart, Loeschen, neue
/// Fenster und Netzdienste nach aussen bleiben der Huelle vorbehalten.
#[test]
fn systembefehle_bleiben_in_der_huelle() {
    let liste = liste();
    for b in [
        "phpvms_login", "hoppie_set_logon_code", "xplane_install_plugin", "auto_start_set_enabled",
        "flight_logs_delete_all", "remote_server_start", "telemetrie_csv_speichern",
        "error_reporting_set_consent", "landing_delete", "kabinen_musik_pruefen", "ascan_list_aircraft",
        "vdgs_fenster_oeffnen", "telemetrie_fenster_oeffnen",
    ] {
        assert_eq!(liste.get(b).map(String::as_str), Some("system"), "{b}");
    }
}
