// Korpus-Lauf der Aufsetz-Messung (DLH9901, 11.11.2026).
//
// Rechnet jedes Aufsetzfenster eines Korpus-Auszugs mit GENAU dem Code
// dieses Stands nach und schreibt die Messfelder je Aufsetzer als JSON-Zeile.
// So lassen sich zwei Staende vergleichen (Umzug ins Modul: muss identisch
// sein; inhaltlicher Fix: nur die gewollten Faelle aendern sich) und die
// Datenkorrektur rechnet mit demselben Code wie der Client.
//
// Aufruf (nicht im normalen Lauf):
//   KORPUS_EIN=<auszug.jsonl.gz> KORPUS_AUS=<ergebnis.jsonl> \
//     cargo test --lib korpus_messung -- --ignored --nocapture
//
// Auszug: je Zeile `{log, pilot, seq, sim, edge_at, alt, p}` mit `p` = die
// Proben des `touchdown_window` (alle Kanaele), `alt` = die damals
// gespeicherte Analyse (nur zum Vergleich).

use super::*;
use std::io::{BufRead, Write};

const FELDER: &[&str] = &[
    "vs_at_edge_fpm",
    "vs_at_edge_quelle",
    "vs_geometrie_fpm",
    "vs_geometrie_ungefiltert_fpm",
    "vs_simvar_edge_fpm",
    "vs_gelaende_fpm",
    "vs_eigensinken_fpm",
    "peak_vs_pre_flare_fpm",
    "vs_at_flare_end_fpm",
    "flare_reduction_fpm",
    "flare_dvs_dt_fpm_per_sec",
    "flare_quality_score",
    "flare_detected",
    "flare_vs_source",
    "peak_vs_pre_flare_fpm_raw",
    "vs_at_flare_end_fpm_raw",
    "peak_g_post_500ms",
    "scored_g",
    "messung_quelle",
    "messung_fassung",
];

#[test]
#[ignore = "Korpus-Lauf, nur auf Anforderung (KORPUS_EIN/KORPUS_AUS)"]
fn korpus_messung() {
    let ein = std::env::var("KORPUS_EIN").expect("KORPUS_EIN");
    let aus = std::env::var("KORPUS_AUS").expect("KORPUS_AUS");
    let datei = std::fs::File::open(&ein).expect("Auszug");
    let leser = std::io::BufReader::new(flate2::read::GzDecoder::new(datei));
    let mut ziel = std::io::BufWriter::new(std::fs::File::create(&aus).expect("Ziel"));
    let mut n = 0usize;
    for zeile in leser.lines() {
        let zeile = zeile.expect("Zeile");
        let o: serde_json::Value = serde_json::from_str(&zeile).expect("JSON");
        let proben: Vec<recorder::TouchdownWindowSample> =
            serde_json::from_value(o["p"].clone()).expect("Proben");
        let edge_at = DateTime::parse_from_rfc3339(o["edge_at"].as_str().expect("edge_at"))
            .expect("edge_at")
            .with_timezone(&Utc);
        let sim: Simulator = serde_json::from_value(o["sim"].clone()).unwrap_or(Simulator::Other);
        let referenz = o["alt"]["vs_sim_referenz_fpm"].as_f64().map(|v| v as f32);
        let a = compute_landing_analysis(&proben, edge_at, sim, referenz);
        let mut neu = serde_json::Map::new();
        for f in FELDER {
            neu.insert((*f).into(), a.get(*f).cloned().unwrap_or(serde_json::Value::Null));
        }
        let rec = serde_json::json!({
            "log": o["log"], "pilot": o["pilot"], "seq": o["seq"], "sim": o["sim"],
            "neu": neu, "alt": o["alt"],
        });
        writeln!(ziel, "{rec}").expect("schreiben");
        n += 1;
    }
    eprintln!("korpus_messung: {n} Aufsetzer nach {aus}");
}
