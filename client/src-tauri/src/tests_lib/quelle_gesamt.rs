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
/// — plus Codex 29.10. zum Upload: Dokument nur mit Aufsetzfenster,
/// Bewertung dafuer ausserhalb der Sperre, Warteschlange je Konto.
/// — plus P-4.13: Feld `aufsetzer_eingefroren`, `aufsetzer_einfrieren` vor
/// beiden Aufrufen von `landung_beim_wegsteigen_abschliessen`; beim Abgeben
/// je Aufsetzer ein Dokument (seq 1 …, Touch-and-Go vor der Landung); Tests
/// `aufsetzer_einfrieren_tests`.
/// — plus P-4.21: `letter_grade`/`aggregate_score_label` rufen
/// `landing_scoring::wasm::gesamt_buchstabe`/`gesamt_wort` (eine Quelle,
/// Test `wort_und_buchstabe_an_den_grenzen`); die PIREP-Nutzlast nimmt Wort
/// und Buchstabe aus der Modul-Ausgabe.
/// — plus P-4.40 (Kap. 4.5): `canonical_landing_bewertung` traegt Urteil,
/// `bewertung_status` und Fassung zu allen Abnehmern (Touchdown-Payload mit
/// `touchdown_note_felder`, Landungsmeldung `touchdown_meldungen`, Notiz
/// `bewertung_vermerk`); die Kabine schweigt bei `modulfehler`; Tests
/// `sofortnote_abnehmer_tests` (Abgabe-Sperre unveraendert).
/// — plus Codex 29.10. zu P-4.40: je PIREP-Aufbau EINE Bewertung
/// (`pirep_bewertete_teile` fuer Score, `build_pirep_fields_mit`,
/// `build_pirep_notes_mit`; die Schatten-Rechnung der Notiz protokolliert
/// sie nur noch; `canonical_landing_verdict` entfaellt,
/// `compute_aggregate_master_score` nur noch in Tests); je Landung EINE
/// Touchdown-Bewertung am Aufsetz-Flughafen (`touchdown_bewertung_festhalten`
/// → Feld `landung_bewertung`, gelesen von Meldung und
/// `touchdown_payload_note`, abgeraeumt mit Touch-and-Go und verworfenem
/// Score); Tests mit Aufrufzaehler in `sofortnote_abnehmer_tests`.
/// — plus Codex 29.10., zweite Runde: je Einreichung EINE Modulrechnung
/// (`PirepRechnung` = Eingabe + Ergebnis, gebildet in `pirep_bewertete_teile`,
/// festgehalten in `pirep_rechnung`); `build_pirep_payload`,
/// `build_landing_record` (neuer Parameter `rechnung`) und die
/// Eingabe-Dokumente (`eingabe_teile`) lesen sie ueber
/// `einreichungs_rechnung`; `landungs_bewertung_aus` leitet das Urteil ab.
/// — plus Etappe 4 (P-3.58): `mod kabinenplaner`; `kabinen_planer` hat den
/// Typ `kabinenplaner::Planer` (Fassade ueber `module/kabine.wasm`, gleiche
/// Methoden), `kabinen_schritt` legt ihn damit an und baut einen
/// unbrauchbar gewordenen neu auf; in `kabinenansagen` nur der Typ und die
/// Notiz `planer` (welcher Weg plant). Tests `kabinenplaner::tests`,
/// `kabinenablauf_aendert_sich_mit_dem_paket`, `kabine-wasm/tests/gleich_nativ.rs`.
/// — plus Etappe 5 (P-8.02/P-8.03): `mod ui_lader`; am Builder das Protokoll
/// `aeroui` (`ui_lader::anfrage`); in `.setup` nach dem Laufzeit-Paket der
/// CI-Selbsttest (`ui_lader::selbsttest_starten`, nur mit
/// `AEROACARS_UI_SELBSTTEST`). Tests `ui_lader::tests`, CI-Job `ui-selbsttest`.
/// — plus Etappe 5 Schritt 3 (P-8.01/P-8.10): `mod huelle`; sechs Befehle
/// `huelle::*` im `generate_handler!` (shell_request, shell_auftraege,
/// shell_auftrag_beantworten, huelle_einstellungen,
/// flight_logs_auto_purge_set, update_installieren); `set_minimize_to_tray`
/// und `error_reporting_set_consent` nehmen `app` und speichern den Wert in
/// der Huelle; im `setup` `huelle::beim_start_anwenden`; Telemetriefenster
/// ueber `aeroui://` (`ui_lader::adresse`). Tests `huelle::tests`,
/// `tests/befehlsliste.rs` (fenster_haben_genau_eine_rechte_datei).
/// — plus P-9.44 (Kap. 5.2): `landungs_herkunft` baut den `herkunft`-Block
/// des Eingabe-Dokuments (reine Dokumentation, nicht in der Rechnung);
/// `aufsetzer_eingefroren` und `EingabeTeil` tragen ihn, eingefroren mit der
/// Eingabe; `landungs_eingabe::dokument` bekommt ihn;
/// `landung_beim_wegsteigen_abschliessen` traegt die Touch-and-Go-Entscheidung
/// in die Herkunft desselben Aufsetzers nach (Codex 30.10.). Tests
/// `landungs_eingabe::tests::herkunft_dokumentiert_nur`,
/// `aufsetzer_einfrieren_tests`.
/// — plus P-4.13 (Fehler aus 900ae7f7): `offene_landung_beim_wegsteigen`
/// ist die eine Bedingung fuer Abschliessen und Einfrieren;
/// `aufsetzer_einfrieren` friert nur ein, wenn der Abschluss stattfindet
/// (vorher: Phasenpruefung danach → `touch_and_go` ohne Entscheidung, die
/// Landung verlor ihr Fenster). Test
/// `aufsetzer_einfrieren_tests::ohne_abschluss_wird_nichts_eingefroren`.
/// — plus Kap. 5.2 vollstaendig: `landungs_begleitung` baut `meta`-Zusatz
/// (Stempel `landing_sim_meta` aus `stamp_touchdown_metadata`: Sim,
/// Simfassung, Profil, Kategorie; dazu `aircraft_icao` = Muster der
/// Rechnung), `herkunft` und `roh` (beide Anflugpuffer, Rollspur, METAR;
/// `ApproachBufferSample` ist dafuer `Serialize`); `aufsetzer_eingefroren`
/// und `EingabeTeil` tragen `landungs_eingabe::Begleitung`; beim Aufsetzen
/// haelt `landing_vref_gemessen` die verbrauchte Vref fuer
/// `herkunft.vref.gemessen_kt` fest. Nichts davon in der Rechnung. Tests
/// `landungs_eingabe::tests::meta_und_roh_dokumentieren_nur`,
/// `aufsetzer_einfrieren_tests::meta_und_roh_werden_mit_eingefroren`,
/// `eingabe_dokument_bleibt_deutlich_unter_der_servergrenze`,
/// `abtaster_stempel_tests::gemessene_vref_steht_in_der_herkunft`,
/// `stempel_haelt_sim_profil_und_kategorie_fest`.
/// — plus Codex 30.10.: `landing_vref_gemessen` wird beim Touch-and-Go
/// geleert; `aufsetzer_einfrieren` friert hoechstens `SEQ_MAX - 1`
/// Aufsetzer ein (Server-Grenze, letzte Nummer fuer die Landung). Tests
/// `touch_and_go_ueber_der_dokumentgrenze`,
/// `touch_and_go_entscheidung_steht_in_der_herkunft`.
/// — plus P-7.21/P-7.22/P-7.23 (Upload-Warteschlange): `mod upload_warteschlange`;
/// Eingabe-Dokumente unter `pending_uploads/eingabe/<konto>/`; der Start
/// laeuft `spawn_upload_arbeiter` (Takt 60 s, Anstoss nach jedem Einreihen,
/// `uploads_durchlauf` = Eingaben, dann `flugprotokolle_senden`) statt eines
/// einmaligen `nachsenden`; `spawn_flight_log_upload` reiht das Protokoll
/// ein (`protokoll_einreihen`, Besitzer = Einreicher) und laedt nur noch die
/// Diagnose selbst; `eingabe_vor_dem_bericht` (15 s) vor jedem
/// `finalize_filed_pirep`; Abgleich beim Start 30 Tage, ohne Stueckgrenze
/// (`abfrage_bloecke`), reiht ein statt hochzuladen; `UPLOAD_SPERRE` (ein
/// Upload gleichzeitig); Loeschautomatik laesst offene Protokolle stehen.
/// Tests `upload_warteschlange::tests`, `landungs_eingabe::tests::aufraeumen_laesst_offene_eintraege_stehen`,
/// `nachreichen_tests::abgleich_ohne_stueckgrenze_ueber_dreissig_tage`,
/// `recorder::purge_tests`.
/// — plus P-7.24: Lese-Befehl `uploads_ausstehend` (eine Zeile im
/// `generate_handler!`, Stufe `lesen` in befehle.json, LAN-Bruecke); liest
/// `upload_warteschlange::ausstehend` fuer das angemeldete Konto. Tests
/// `upload_warteschlange::tests::einreihen_und_anzeige_je_konto`,
/// `tests/befehlsliste.rs`, vitest `UploadAusstehend.test.tsx`.
/// — plus P-5.01: Ereignisse `PaketStand` (`paket_stand_vermerken` nach
/// jedem `FlightStarted`), `AnflugPuffer` (vor `TouchdownComplete`, Inhalt
/// aus `anflugpuffer_json`, das auch `roh.anflugpuffer` baut) und
/// `BewertungEingabe` (je eingereihtem Dokument: `input_sha256`, Groesse).
/// Tests `recorder::v3_ereignis_tests`,
/// `landungs_eingabe::tests::neue_ereignisse_stehen_an_ihren_stellen`.
/// — plus P-5.10: `seq`/`input_sha256` in Touchdown- und PIREP-Nutzlast,
/// `runtime_bundle_id` im Touchdown (`touchdown_seq`,
/// `touchdown_input_sha256` = Eingabe der Sofortnote am Aufsetz-Flughafen;
/// `pirep_eingabe_kennung` = Dokument der Landung aus derselben
/// `einreichungs_rechnung`, die `build_pirep_payload` jetzt einmal bildet);
/// `landungs_eingabe::input_sha256` als eine Stelle fuer den Hash. Tests
/// `sofortnote_abnehmer_tests::pirep_nutzlast_nennt_das_eingabe_dokument`,
/// `touchdown_nennt_seq_und_hash_der_sofortnote`, `aeroacars-mqtt::p5_10_felder_rundlauf`.
/// — plus P-4.10: Felder `landung_sofort`/`landung_endgueltig`
/// (`GespeicherteRechnung`, Schluessel Aufsetzzeit + `bahn_revision`);
/// `touchdown_bewertung_festhalten` speichert die Sofortnote (dieselbe
/// Rechnung wie vorher `canonical_landing_bewertung`, jetzt `cfg(test)`, ebenso
/// `compute_bewertung`); `endgueltige_rechnung_festhalten` im Streamer nach dem
/// Ausrollen; `pirep_bewertete_teile` nimmt `gespeicherte_rechnung` und rechnet
/// nur ohne passende selbst; beide Felder fallen mit Touch-and-Go und
/// verworfener Note. Tests `sofortnote_abnehmer_tests::pirep_uebernimmt_gespeicherte_ausgabe`,
/// `neuer_aufsetzer_nimmt_keine_alte_rechnung`.
/// — plus P-3.31: die Rechenstellen eines Flugs (`pirep_rechnung_bilden`,
/// `endgueltige_rechnung_festhalten`, Landungsdatensatz ohne Rechnung,
/// Touch-and-Go-Dokumente) rufen `bewertung::bewerten_fuer(pirep_id, …)`;
/// `GespeicherteRechnung::pin_fehlte`, `gespeicherte_rechnung` verwirft sie,
/// sobald der Stand nachgeholt ist. Tests
/// `bewertung::tests::ohne_pinstand_vorlaeufig_nachgeholt_endgueltig`,
/// `sofortnote_abnehmer_tests::ohne_pinstand_vorlaeufig_und_neu_vor_der_abgabe`.
/// — plus P-3.04: `aktualisieren_im_hintergrund` bekommt den Melder
/// `paket_pruefung_hinweis` (Aktivitaetsprotokoll nach ueber 30 Tagen ohne
/// erfolgreiche Pruefung). Test `laufzeit_paket::tests::hinweis_nach_dreissig_tagen_ohne_pruefung`.
/// — plus Codex 30.10. zur Warteschlange: Konto der Eingaben = phpVMS-Pilot-ID
/// (`upload_konto`, stabil ueber Token-Erneuerungen); Vorversuch vor dem PIREP
/// ignoriert den Abstand; fehlendes Protokoll beendet den Eintrag nur bei
/// `NotFound`; `landungsdatensatz_moeglich` gemeinsam fuer
/// `build_landing_record` und `pirep_eingabe_kennung`; `fehlende_einreihen`.
/// Tests `nachreichen_tests::fehlende_werden_alle_eingereiht`,
/// `landungs_eingabe::tests::abgelehntes_geht_nie_verloren`,
/// `sofortnote_abnehmer_tests::pirep_nutzlast_nennt_das_eingabe_dokument`.
/// — plus P-11.21: `BUNDLE_IDENTIFIER` = `env!("AEROACARS_KENNUNG")` aus
/// build.rs (`tauri.conf.json`, ueberlagert von `TAURI_CONFIG`), damit ein
/// Dev-Build (`com.aeroacars.app3.dev`) auch Log-Ordner und Datenuebernahme
/// getrennt hat. Test `logging_tests::log_dir_identifier_matches_tauri_conf`
/// (Gegenprobe mit `TAURI_CONFIG` der Dev-Kennung -> rot).
/// — plus P-4.21: `build_landing_record` setzt `modul_grenzen` (aus der
/// Modulausgabe `grenzen`) und `modul_sollband` (`bewertung::sollband` ueber
/// dieselben `approach_samples` und denselben Gleitwinkel wie die Grafik);
/// `ohne_schwere_felder` leert `modul_sollband`. Tests
/// `sofortnote_abnehmer_tests::datensatz_traegt_grenzen_und_sollband_aus_dem_modul`,
/// `bewertung::tests::sollband_kommt_aus_dem_modul`. Codex 30.10.:
/// `bewertung::sollband` bekommt `&flight.pirep_id` (dasselbe Modul wie die
/// Note, auch beim nachgeholten Stand).
/// — plus P-9.22/N-06 (Hilfe anfordern): `mod hilfe` und zwei Zeilen im
/// `generate_handler!` (`hilfe::hilfe_vorschau` Stufe `lesen`,
/// `hilfe::hilfe_anfordern` Stufe `fachlich`); sonst nichts an `lib.rs`.
/// Tests `hilfe::tests`, `tests/befehlsliste.rs`, vitest `HilfeDialog.test.tsx`.
/// — plus P-4.44/P-9.01: EINE Zeile im `generate_handler!`,
/// `landungs_eingabe::landung_nachbewertung` (Stufe `fachlich`, nur lesen von
/// live mit dem Pilot-Token). Tests `landungs_eingabe::tests::nachbewertung_*`,
/// vitest `bewertungsFassung.test.tsx`.
#[test]
fn lib_rs_ist_der_gepruefte_stand() {
    use sha2::{Digest, Sha256};
    let hash: String = Sha256::digest(PRODUKTION.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(hash, HASH_VOR_DER_ZERLEGUNG, "lib.rs weicht vom zuletzt geprueften Stand ab — Hash und Liste nachziehen");
}

const HASH_VOR_DER_ZERLEGUNG: &str = "117c93d848821fcced52b6f9f555aeb357915e53452c977e51a33fda1b8d27fe";
