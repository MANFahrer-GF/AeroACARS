//! `paket-bauen <quelle> <ziel> --generation N [--erstellt RFC3339] [--repo-commit SHA]
//!   [--ourairports-commit SHA] [--client-min V] [--client-max V] [--requires a,b]
//!   [--score-version N] [--input-schema N] [--gueltig-tage N]`
//!
//! Schreibt `<ziel>/manifest.json` und `<ziel>/blobs/`. Signieren macht die CI.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use runtime_bundle::bauen::{bauen, Optionen};
use runtime_bundle::manifest::Quelle;

fn main() {
    if let Err(e) = lauf() {
        eprintln!("paket-bauen: {e}");
        std::process::exit(1);
    }
}

fn lauf() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let quelle = PathBuf::from(args.next().ok_or("Quellordner fehlt")?);
    let ziel = PathBuf::from(args.next().ok_or("Zielordner fehlt")?);
    let mut o = Optionen {
        generation: 0,
        erstellt: Utc::now(),
        gueltig_tage: 180,
        client_min: "3.0.0-0".into(),
        client_max: "3.99.99".into(),
        requires: Vec::new(),
        score_version: 27,
        input_schema: 1,
        quelle: Quelle::default(),
    };
    while let Some(schalter) = args.next() {
        let wert = args.next().ok_or_else(|| format!("{schalter}: Wert fehlt"))?;
        match schalter.as_str() {
            "--generation" => o.generation = wert.parse().map_err(|_| "Generation ungueltig")?,
            "--erstellt" => {
                o.erstellt = DateTime::parse_from_rfc3339(&wert)
                    .map_err(|e| e.to_string())?
                    .with_timezone(&Utc)
            }
            "--gueltig-tage" => o.gueltig_tage = wert.parse().map_err(|_| "Tage ungueltig")?,
            "--client-min" => o.client_min = wert,
            "--client-max" => o.client_max = wert,
            "--requires" => o.requires = wert.split(',').filter(|s| !s.is_empty()).map(Into::into).collect(),
            "--score-version" => o.score_version = wert.parse().map_err(|_| "score-version ungueltig")?,
            "--input-schema" => o.input_schema = wert.parse().map_err(|_| "input-schema ungueltig")?,
            "--repo-commit" => o.quelle.repo_commit = Some(wert),
            "--ourairports-commit" => o.quelle.ourairports_commit = Some(wert),
            _ => return Err(format!("unbekannt: {schalter}")),
        }
    }
    if o.generation == 0 {
        return Err("--generation fehlt".into());
    }
    let m = bauen(&quelle, &ziel, &o).map_err(|e| e.to_string())?;
    println!("{} Bytes Manifest, Generation {}", m.len(), o.generation);
    Ok(())
}
