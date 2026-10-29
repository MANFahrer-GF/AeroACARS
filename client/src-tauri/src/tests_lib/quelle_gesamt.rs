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
/// die vier Zeilen der Erklaerung `mod quelle_gesamt` (`341303c6`) plus die
/// Anbindung des Laufzeit-Pakets (Etappe 1a: `mod laufzeit_paket`, Feld
/// `runtime_bundle_id` in `PersistedFlight`, Eintrag
/// `laufzeit_paket::flug_pin(&flight.pirep_id)` in `save_active_flight`,
/// eine Zeile am Anfang von `.setup` mit `starten` und
/// `aktualisieren_im_hintergrund`) — nur Zusaetze, keine Zeile entfernt —
/// plus Etappe 1b (Grenzwerte): `mod grenzwerte` und EINE geaenderte Zeile,
/// `stands::ROLLWEG_MITTELLINIE_M` -> `grenzwerte().staende.rollweg_mittellinie_m.wert`
/// (Wert gleich, Test `grenzwerte_staende_wie_2_1_24`) — plus Bereich
/// Aufsetzen: vier Stellen `touchdown_v2::<KONST>` -> `grenzwerte().aufsetzen.*`
/// (VS_FLOOR_FPM, BOUNCE_FORENSIC/SCORED_MIN_AGL_FT, BEWERTUNGS_FENSTER_VOR/NACH_MS;
/// Werte gleich, Test `grenzwerte_aufsetzen_wie_2_1_24`).
#[test]
fn zerlegung_ist_nur_verschoben() {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(PRODUKTION.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hash, HASH_VOR_DER_ZERLEGUNG, "lib.rs-Zerlegung hat Programmtext veraendert");
}

const HASH_VOR_DER_ZERLEGUNG: &str = "f46bee92e41f3ccbea726c3fbbf9440a2dba422b69777ad8d896f23e38346b05";
