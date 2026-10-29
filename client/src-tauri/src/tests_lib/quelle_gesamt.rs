//! Der gesamte Produktions-Quelltext fuer die Text-Waechter in `tests_lib/`.
//!
//! v3 (Etappe 0): `lib.rs` wird in Dateien zerlegt. Ein Waechter, der nur
//! `lib.rs` liest, verloere bei jedem Umzug still seine Fundstellen. Er liest
//! deshalb `PRODUKTION`: den von `build.rs` zusammengesetzten Text — `lib.rs`
//! mit jeder ausgelagerten Datei an ihrer alten Stelle. Die Liste der
//! ausgelagerten Teile steht nur in `build.rs`.

pub(crate) const PRODUKTION: &str = include_str!(concat!(env!("OUT_DIR"), "/produktion.txt"));

/// Beweis „nur verschoben“: Der zusammengesetzte Text ist byte-gleich mit dem
/// Stand vor der Zerlegung. Aendert ein Commit absichtlich Programmtext in
/// `lib.rs` oder einer ausgelagerten Datei, setzt er den Hash hier neu und
/// begruendet es in der Commit-Nachricht.
///
/// Stand: `lib.rs` bei `cdd6028c` (nach dem Auslagern der Testmodule) plus
/// die vier Zeilen der Erklaerung `mod quelle_gesamt` (`341303c6`).
#[test]
fn zerlegung_ist_nur_verschoben() {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(PRODUKTION.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hash, HASH_VOR_DER_ZERLEGUNG, "lib.rs-Zerlegung hat Programmtext veraendert");
}

const HASH_VOR_DER_ZERLEGUNG: &str = "b016bd046c0bc7399d72be9ec3d6526029e654d007ce9a769e679a0fb26f8e11";
