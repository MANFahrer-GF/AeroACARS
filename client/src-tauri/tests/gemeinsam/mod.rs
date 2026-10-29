//! Gemeinsame Helfer der Quelltext-Waechter.
//!
//! v3 (Etappe 0): `lib.rs` wird zerlegt. Programmmodule wandern aus
//! `mod x { … }` in eigene Dateien `src/x.rs`, in `lib.rs` bleibt nur
//! `mod x;`. Die Waechter pruefen aber Verdrahtung ueber Modulgrenzen
//! hinweg im Text. `lib_quelle()` setzt deshalb jedes ausgelagerte Modul
//! wieder an seine alte Stelle — Zeichen fuer Zeichen derselbe Text wie
//! vor dem Umzug. Wer ein Modul auslagert, traegt es in `AUSGELAGERT` ein.

use std::fs;

/// Aus `lib.rs` ausgelagerte Programmmodule (Name = Datei `src/<name>.rs`).
pub const AUSGELAGERT: &[&str] = &["pirep_queue", "nachtrag_queue"];

/// `src/lib.rs` mit allen ausgelagerten Modulen wieder eingesetzt.
#[allow(dead_code)]
pub fn lib_quelle() -> String {
    let mut quelle = fs::read_to_string("src/lib.rs").expect("lib.rs");
    for name in AUSGELAGERT {
        let erklaerung = format!("\nmod {name};\n");
        let koerper = fs::read_to_string(format!("src/{name}.rs"))
            .unwrap_or_else(|_| panic!("src/{name}.rs fehlt"));
        assert_eq!(
            quelle.matches(&erklaerung).count(),
            1,
            "`mod {name};` steht nicht genau einmal in lib.rs"
        );
        let eingesetzt = format!("\nmod {name} {{\n{}}}\n", koerper);
        quelle = quelle.replacen(&erklaerung, &eingesetzt, 1);
    }
    quelle
}
