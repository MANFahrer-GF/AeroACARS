//! Korpus-Vergleich der Aufsetz-Messregeln (DLH9901, 11.11.2026).
//!
//! Rechnet jeden Aufsetzer eines Auszugs (`{log, pilot, seq, sim, alt,
//! proben, kante_ms}` je Zeile, `proben` mit `t_ms`) mit jeder Regel-Variante und
//! schreibt je Aufsetzer eine Zeile. Nicht im normalen Lauf:
//!   MESS_KORPUS_EIN=<anfragen.jsonl.gz> MESS_KORPUS_AUS=<aus.jsonl> \
//!     cargo test -p landing-scoring --release --test korpus_messung -- --ignored

use std::io::{BufRead, Write};

use landing_scoring::aufsetz_messung::{messen_mit, Anfrage, Parameter, Regeln, Rueckblick, REGELN, REGELN_FASSUNG_1};

fn parameter() -> Parameter {
    let landung: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../bundle/grenzwerte/landung.json")).unwrap();
    let aufsetzen: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../bundle/grenzwerte/aufsetzen.json")).unwrap();
    let mut v = serde_json::json!({ "fenster_ms": aufsetzen["luecken_fenster_vor_ms"]["wert"] });
    for k in [
        "agl_min_samples", "agl_min_verschiedene", "agl_max_stillstand_ms", "agl_min_spanne_s",
        "agl_max_plausibel_fpm", "probe_toleranz_fpm", "td_agl_max_at_window_start_ft",
        "msfs_flare_agl_fit_half_window_ms", "msfs_flare_agl_fit_min_samples", "msfs_flare_min_distinct_agl",
        "msfs_flare_max_median_dt_ms", "msfs_flare_min_end_fit_distinct_agl", "msfs_flare_min_end_agl_span_ft",
        "msfs_flare_balloon_tol_ft", "msfs_flare_pitch_nosedown_veto_deg", "msfs_flare_detect_floor_fpm",
    ] {
        v[k] = landung[k]["wert"].clone();
    }
    serde_json::from_value(v).unwrap()
}

#[test]
#[ignore = "Korpus-Lauf, nur auf Anforderung (MESS_KORPUS_EIN/MESS_KORPUS_AUS)"]
fn korpus_regeln_vergleichen() {
    let ein = std::env::var("MESS_KORPUS_EIN").expect("MESS_KORPUS_EIN");
    let aus = std::env::var("MESS_KORPUS_AUS").expect("MESS_KORPUS_AUS");
    let leser = std::io::BufReader::new(flate2::read::GzDecoder::new(std::fs::File::open(ein).unwrap()));
    let mut ziel = std::io::BufWriter::new(std::fs::File::create(aus).unwrap());
    let varianten = [
        ("K", REGELN_FASSUNG_1),
        ("B", Regeln { fassung: 2, rueckblick: Rueckblick::Bild, abfang_stillstand: false }),
        ("BF", REGELN),
    ];
    let parameter = parameter();
    for zeile in leser.lines() {
        let o: serde_json::Value = serde_json::from_str(&zeile.unwrap()).unwrap();
        let a = Anfrage {
            simulator: o["sim"].as_str().unwrap_or("Other").into(),
            proben: serde_json::from_value(o["proben"].clone()).unwrap(),
            parameter: parameter.clone(),
            kante_ms: o["kante_ms"].as_i64().unwrap_or(0),
        };
        let mut r = serde_json::json!({ "log": o["log"], "pilot": o["pilot"], "seq": o["seq"], "sim": o["sim"], "alt": o["alt"] });
        for (name, regeln) in varianten {
            r[name] = serde_json::to_value(messen_mit(&a, regeln)).unwrap();
        }
        writeln!(ziel, "{r}").unwrap();
    }
}
