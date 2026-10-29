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
/// — plus Etappe 1b (Flotte): EINE neue Zeile im `generate_handler!`,
/// `laufzeit_paket::paket_datei` (lesender Befehl fuer die Oberflaeche, nur
/// die feste Liste `UI_DATEIEN`, Test `paket_datei_gibt_nur_freigegebene_dateien`).
/// — plus Etappe 1b (Klappenraster): `raster_flap_label` liest die Raster aus
/// `tabellen/klappen.json` statt aus vier lokalen `const`; `ist_airbus_mit_1f`
/// entfaellt (Liste dort als `nur_muster`). Abdruck `hoehengruppen_und_klappen_wie_2_1_24`,
/// Verhalten `raster_flap_label_tests` unveraendert gruen.
/// — plus Etappe 1b (Endpunkte, P-3.60): die VDGS-Adresse `https://vats.im/vdgs`
/// kommt aus `paket_endpunkte::endpunkte().vatsim_vdgs` (gleicher Wert, Test
/// `eingebaute_adressen_wie_2_1_24`; Host nur aus der festen Liste).
/// — plus P-14.08 (eigene App neben 2.x): `mod datenuebernahme`,
/// `BUNDLE_IDENTIFIER` = `com.aeroacars.app3` (Test
/// `log_dir_identifier_matches_tauri_conf`), Webview-Diagnose ueber die
/// Konstante, und in `run` die einmalige Kopie der 2.x-Daten (Tests
/// `datenuebernahme::tests`).
/// — plus Etappe 3 (P-4.31): `mod bewertung`; die vier Rechenstellen
/// (Touchdown-Payload, `compute_aggregate_master_score`, Landungsdatensatz,
/// Schattenvergleich) nehmen Teilnoten, Gesamtnote und Deckel aus
/// `bewertung::bewerten` (WASM-Modul) statt `landing_scoring` nativ; Test
/// `bewertung::tests::modul_rechnet_wie_das_crate`, `bewertung-host`
/// (60 Faelle Byte fuer Byte). Nativ bleibt nur `normalize_derived_scores`
/// fuer 2.x-Datensaetze der Fassung 18–20.
/// — plus P-4.50/P-4.33: `SCORE_ALGORITHMUS_VERSION` = 27 (Test
/// `bewertung::tests::fassung_gleich_modul`) und `bewertung_status` an
/// PIREP, Landungsdatensatz (aus dem Modulergebnis) und Touchdown-Nutzlast
/// (`None`, sie traegt die Klassen-Note).
/// — plus P-6.03/P-7.02: `runtime_bundle_id` (Flug-Pin) in der
/// PIREP-Nutzlast; `mod landungs_eingabe`; Feld `landung_fenster` (Kopie
/// des Aufsetzfensters beim Dump); `landungs_bewertungseingabe` als EINE
/// Stelle fuer Datensatz und Eingabe-Dokument (vorher inline in
/// `build_landing_record`, Inhalt gleich); `record_landing_for_filed_flight`
/// legt das Dokument ab und sendet es; Start sendet Liegengebliebenes
/// (Tests `landungs_eingabe::tests`).
#[test]
fn lib_rs_ist_der_gepruefte_stand() {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(PRODUKTION.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hash, HASH_VOR_DER_ZERLEGUNG, "lib.rs weicht vom zuletzt geprueften Stand ab — Hash und Liste nachziehen");
}

const HASH_VOR_DER_ZERLEGUNG: &str = "3bd150dfe1638782d84391902d5666e02871a8da4a115c915b89ed9fa8f4570e";
