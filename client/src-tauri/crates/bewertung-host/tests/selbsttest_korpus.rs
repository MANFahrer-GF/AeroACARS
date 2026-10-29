//! Selbsttest-Korpus (Pflichtenheft P-13.02, P-3.21): echte Landungen aus
//! den 2.x-Protokollen, anonymisiert (nur Eingabe-Dokument und erwartete
//! Ausgabe, keine Kennung, kein Name, kein Rufzeichen). Je Simulator
//! mindestens 20, darunter die Sonderfaelle aus Anhang C. Geschrieben vom
//! Gleichheitslauf (`tests/gleichheitslauf.rs`, `KORPUS_ZIEL`).
//!
//! Das eingecheckte Modul muss jede erwartete Ausgabe genau treffen — so wie
//! das Crate nativ.

use bewertung_host::Host;
use landing_scoring::wasm::bewerten_json;
use serde_json::Value;

const MODUL: &[u8] = include_bytes!("../../../../../bundle/module/bewertung.wasm");
const VIEL: u64 = 1_000_000_000;

fn faelle() -> Vec<(String, Value)> {
    let ordner = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/korpus");
    let mut v: Vec<(String, Value)> = std::fs::read_dir(ordner)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            (name, serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap())
        })
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

#[test]
fn modul_trifft_jede_erwartete_ausgabe() {
    let host = Host::neu(MODUL).unwrap();
    for (name, fall) in faelle() {
        let ein = fall["eingabe"].to_string();
        let wasm: Value = serde_json::from_str(&host.bewerten(&ein, VIEL).unwrap()).unwrap();
        assert_eq!(wasm, fall["ausgabe"], "{name}: Modul");
        let nativ: Value = serde_json::from_str(&bewerten_json(&ein)).unwrap();
        assert_eq!(nativ, fall["ausgabe"], "{name}: Crate nativ");
    }
}

#[test]
fn je_simulator_mindestens_20_und_alle_sonderfaelle() {
    let namen: Vec<String> = faelle().into_iter().map(|(n, _)| n).collect();
    for sim in ["msfs_", "xplane_"] {
        let n = namen.iter().filter(|x| x.starts_with(sim)).count();
        assert!(n >= 20, "{sim}: nur {n} Faelle");
    }
    for fall in [
        "touch_and_go",
        "hopser",
        "durchstart",
        "umleitung",
        "sonderfall_abdeckung_fehlt",
        "sonderfall_landung_verloren",
        "sonderfall_bodenbezug_9ft",
        "sonderfall_g_ohne_ema",
    ] {
        assert!(namen.iter().any(|x| x.contains(fall)), "Sonderfall {fall} fehlt");
    }
}
