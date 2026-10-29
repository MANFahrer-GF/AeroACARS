//! Der gesamte Produktions-Quelltext fuer die Text-Waechter in `tests_lib/`.
//!
//! v3 (Etappe 0): `lib.rs` wird in Dateien zerlegt. Ein Waechter, der nur
//! `lib.rs` liest, verloere bei jedem Umzug still seine Fundstellen. Er liest
//! deshalb `PRODUKTION`: den von `build.rs` zusammengesetzten Text — `lib.rs`
//! mit jeder ausgelagerten Datei an ihrer alten Stelle. Die Liste der
//! ausgelagerten Teile steht nur in `build.rs`.

pub(crate) const PRODUKTION: &str = include_str!(concat!(env!("OUT_DIR"), "/produktion.txt"));

/// Beweis „`lib.rs` ist der zuletzt geprüfte Stand“: Der zusammengesetzte
/// Text hat genau diesen Hash. Bis Etappe 0 Schritt 2a bedeutete das „nur
/// verschoben“; seit Etappe 1a/1b steht jede bewusste Aenderung unten in der
/// Liste. Wer Programmtext in `lib.rs` oder einer ausgelagerten Datei aendert,
/// setzt den Hash neu, ergaenzt die Liste und begruendet es im Commit.
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
/// Werte gleich, Test `grenzwerte_aufsetzen_wie_2_1_24`) — plus Bereich
/// Ankunft: `arrival::ON_FIELD_*`/`NEAREST_SEARCH_RADIUS_NM` -> `grenzwerte().ankunft.*`
/// (Werte gleich, Test `grenzwerte_ankunft_wie_2_1_24`) — plus die fachlichen
/// Konstanten von `lib.rs` selbst (Etappe 1b, Bereiche phasen_kern, landung,
/// anflug_kern, ausrollen, sprit, abgabe, cockpit; je Bereich ein Test
/// `grenzwerte_<bereich>_wie_2_1_24`): `const`-Zeilen durch Verweise ersetzt,
/// Verwendungen auf `grenzwerte().<bereich>.*` umgestellt, zwei abgeleitete
/// Konstanten (ROLLOUT_TICK_MIN_GS_KT, MSFS_FLARE_AGL_MAX_FT) als Funktion.
#[test]
fn lib_rs_ist_der_gepruefte_stand() {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(PRODUKTION.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hash, HASH_VOR_DER_ZERLEGUNG, "lib.rs weicht vom zuletzt geprueften Stand ab — Hash und Liste nachziehen");
}

const HASH_VOR_DER_ZERLEGUNG: &str = "f759164cceff378ffa2c1e6e3afae17ddbebc398599de6d861abd6353028c901";
