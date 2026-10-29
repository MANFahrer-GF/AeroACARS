//! Der gesamte Produktions-Quelltext fuer die Text-Waechter in `tests_lib/`.
//!
//! v3 (Etappe 0): `lib.rs` wird in Dateien zerlegt. Ein Waechter, der nur
//! `lib.rs` liest, verloere bei jedem Umzug still seine Fundstellen. Er liest
//! deshalb `PRODUKTION`: `lib.rs` plus jede ausgelagerte Programmdatei. Wer
//! ein Modul aus `lib.rs` auslagert, haengt seine Datei hier an (und in
//! `tests/gemeinsam/mod.rs`, `AUSGELAGERT`).

pub(crate) const PRODUKTION: &str = concat!(
    include_str!("../lib.rs"),
    "\n",
    include_str!("../pirep_queue.rs"),
    "\n",
    include_str!("../nachtrag_queue.rs"),
);
