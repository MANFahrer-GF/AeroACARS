//! Selbsttest-Korpus (Pflichtenheft P-13.02, P-3.21): echte Landungen aus
//! den 2.x-Protokollen, anonymisiert (nur Eingabe-Dokument und erwartete
//! Ausgabe, keine Kennung, kein Name, kein Rufzeichen). Je Simulator
//! mindestens 20, darunter die Sonderfaelle aus Anhang C. Geschrieben vom
//! Gleichheitslauf (`tests/gleichheitslauf.rs`, `KORPUS_ZIEL`).
//!
//! Das eingecheckte Modul muss jede erwartete Ausgabe genau treffen — so wie
//! das Crate nativ.

use bewertung_host::Host;
use landing_scoring::wasm::{bewerten_json, Ausgabe, Eingabe};
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

/// Byte fuer Byte: die erwartete Ausgabe in der Form, die das Modul
/// schreibt (Feldreihenfolge der Struktur, kuerzeste `f32`-Form).
#[test]
fn modul_trifft_jede_erwartete_ausgabe() {
    let host = Host::neu(MODUL).unwrap();
    for (name, fall) in faelle() {
        let eingabe: Eingabe = serde_json::from_value(fall["eingabe"].clone()).unwrap();
        let ein = serde_json::to_string(&eingabe).unwrap();
        let erwartet: Ausgabe = serde_json::from_value(fall["ausgabe"].clone()).unwrap();
        let erwartet = serde_json::to_string(&erwartet).unwrap();
        assert_eq!(host.bewerten(&ein, VIEL).unwrap(), erwartet, "{name}: Modul");
        assert_eq!(bewerten_json(&ein), erwartet, "{name}: Crate nativ");
    }
}

/// Gegenprobe: Der Korpus traegt — eine andere Sinkrate aendert die Ausgabe.
#[test]
fn andere_eingabe_trifft_die_erwartung_nicht() {
    let host = Host::neu(MODUL).unwrap();
    let mut geaendert = 0;
    for (_, fall) in faelle() {
        let mut eingabe: Eingabe = serde_json::from_value(fall["eingabe"].clone()).unwrap();
        let Some(vs) = eingabe.landung.vs_fpm else { continue };
        eingabe.landung.vs_fpm = Some(vs * 3.0 - 400.0);
        let aus: Value = serde_json::from_str(&host.bewerten(&serde_json::to_string(&eingabe).unwrap(), VIEL).unwrap()).unwrap();
        assert_ne!(aus, fall["ausgabe"]);
        geaendert += 1;
    }
    assert!(geaendert >= 40, "{geaendert}");
}

/// Keine Personendaten: nur Eingabe und Ausgabe, kein Schluessel, der eine
/// Kennung, einen Namen oder ein Rufzeichen tragen koennte.
#[test]
fn korpus_ist_anonym() {
    fn schluessel(v: &Value, aus: &mut Vec<String>) {
        match v {
            Value::Object(o) => o.iter().for_each(|(k, x)| {
                aus.push(k.to_ascii_lowercase());
                schluessel(x, aus);
            }),
            Value::Array(a) => a.iter().for_each(|x| schluessel(x, aus)),
            _ => {}
        }
    }
    for (name, fall) in faelle() {
        let obersten: Vec<&String> = fall.as_object().unwrap().keys().collect();
        assert_eq!(obersten, ["ausgabe", "eingabe"], "{name}");
        serde_json::from_value::<Eingabe>(fall["eingabe"].clone()).unwrap();
        let mut k = Vec::new();
        schluessel(&fall, &mut k);
        for verboten in ["pirep", "pilot", "user", "callsign", "rufzeichen", "flight_number", "registration", "name"] {
            assert!(!k.iter().any(|x| x.contains(verboten)), "{name}: Schluessel mit {verboten}");
        }
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
