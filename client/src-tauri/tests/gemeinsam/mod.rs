//! Gemeinsame Helfer der Quelltext-Waechter.
//!
//! v3 (Etappe 0): `lib.rs` wird zerlegt. Die Waechter pruefen Verdrahtung
//! im Text und lesen deshalb den von `build.rs` zusammengesetzten Quelltext
//! (`OUT_DIR/produktion.txt`): `lib.rs` mit jeder ausgelagerten Datei an
//! ihrer alten Stelle — derselbe Text wie vor der Zerlegung. Dieselbe Datei
//! lesen die Waechter in `src/tests_lib/` (`quelle_gesamt::PRODUKTION`);
//! die Liste der ausgelagerten Teile steht nur in `build.rs`.

/// `src/lib.rs` mit allen ausgelagerten Teilen an ihrer alten Stelle.
#[allow(dead_code)]
pub fn lib_quelle() -> String {
    include_str!(concat!(env!("OUT_DIR"), "/produktion.txt")).to_string()
}
