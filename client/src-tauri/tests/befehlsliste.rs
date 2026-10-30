//! Jeder registrierte Tauri-Befehl hat genau eine Rechte-Stufe in
//! `befehle.json` (Pflichtenheft P-8.10/P-8.11, Anhang G Kap. 8).
//!
//! Etappe 5 gibt der nachgeladenen Oberflaeche nur die Stufen „lesen“ und
//! „fachlich“; „system“ bleibt der Huelle. Ein neuer Befehl ohne Eintrag
//! wuerde sonst still in keiner Stufe landen — dieser Test haelt ihn auf.

use std::collections::{BTreeMap, BTreeSet};

fn registriert() -> BTreeSet<String> {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).unwrap();
    let start = src.find("generate_handler![").expect("generate_handler! fehlt") + "generate_handler![".len();
    let ende = start + src[start..].find(']').unwrap();
    src[start..ende]
        .lines()
        .map(|z| z.split("//").next().unwrap())
        .flat_map(|z| z.split(','))
        .map(|s| s.trim().rsplit("::").next().unwrap().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn liste() -> BTreeMap<String, String> {
    let roh = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/befehle.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&roh).unwrap();
    assert_eq!(v["fassung"], 1);
    serde_json::from_value(v["befehle"].clone()).unwrap()
}

#[test]
fn jeder_befehl_hat_genau_eine_stufe() {
    let reg = registriert();
    let liste = liste();
    assert!(reg.len() > 150, "Registrierung nicht gelesen: {}", reg.len());
    let ohne: Vec<_> = reg.iter().filter(|b| !liste.contains_key(*b)).collect();
    let tot: Vec<_> = liste.keys().filter(|b| !reg.contains(*b)).collect();
    assert!(ohne.is_empty(), "registriert, aber ohne Stufe in befehle.json: {ohne:?}");
    assert!(tot.is_empty(), "in befehle.json, aber nicht registriert: {tot:?}");
    for (b, s) in &liste {
        assert!(["lesen", "fachlich", "system"].contains(&s.as_str()), "{b}: unbekannte Stufe {s}");
    }
}

/// P-8.10: Loeschen, nach aussen senden, installieren, Netzdienste oeffnen,
/// Zugangscodes und Dateipfade aus der Oberflaeche bleiben der Huelle
/// vorbehalten.
#[test]
fn systembefehle_bleiben_in_der_huelle() {
    let liste = liste();
    for b in [
        "hoppie_set_logon_code", "xplane_install_plugin", "flight_logs_delete_all", "flight_logs_purge_older_than",
        "remote_server_start", "remote_server_set_port", "landing_delete", "ascan_submit", "vermessung_senden",
        "kabinen_musik_hinzufuegen", "kabinen_musik_entfernen", "activity_log_clear", "flight_forget_remote",
        // Etappe 5: Wer Auftraege beantworten duerfte, koennte sich selbst
        // bestaetigen — nur die Huelle.
        "shell_auftraege", "shell_auftrag_beantworten", "update_installieren", "flight_logs_auto_purge_set",
    ] {
        assert_eq!(liste.get(b).map(String::as_str), Some("system"), "{b}");
    }
}

/// Abweichung von P-8.10 (30.10.2026, Thomas' erster Start von Beta 1):
/// Eine zweite Rueckfrage der Huelle schuetzt nur, wo der Pilot den Schaden
/// nicht sofort sieht oder nicht aus demselben Bildschirm zuruecknehmen kann.
/// Fenster oeffnen, lesende Vorbereitung, lokale Schalter mit sichtbarem
/// Rueckweg und Aktionen, deren eigene Oberflaeche schon die Bestaetigung ist
/// (Anmeldeformular, Speichern-Dialog des Systems, Klick auf die
/// Einwilligung), laufen fachlich. Das Passwort tippt der Pilot ohnehin in
/// der Oberflaeche — die Huelle kann es nicht schuetzen.
///
/// Bewusst geprueft (unabhaengiger Pruefer 30.10.):
/// - `auto_start_set_enabled` ist der automatische Flugstart, nicht der
///   Autostart des Systems. `flight_start` ist schon fachlich — der Schalter
///   erlaubt einer Oberflaeche nichts, was sie nicht direkt koennte.
/// - `error_reporting_set_consent`: Der Klick im Banner/Schalter ist die
///   Einwilligung. Eine gefaelschte Einwilligung setzt einen gestohlenen
///   Paketschluessel voraus (P-8.12) und schickt nur anonyme Fehlerberichte
///   an den eigenen GlitchTip.
/// - LAN-Bruecke: Ein per PIN gekoppeltes Geraet ruft die 17 direkt auf, wie
///   in 2.x („alles 1 zu 1“, Thomas). Die Kopplung ist die Vertrauensgrenze.
/// - Pfade aus der Oberflaeche (`kabinen_musik_pruefen`,
///   `xplane_check_install_path`, `ascan_list_aircraft`, `ascan_collect`)
///   geben keinen Dateiinhalt zurueck, nur Existenz/Groesse/Titel.
#[test]
fn umgestufte_befehle_laufen_fachlich() {
    let liste = liste();
    for b in [
        "phpvms_login", "phpvms_logout", "error_reporting_set_consent", "telemetrie_fenster_oeffnen",
        "vdgs_fenster_oeffnen", "kabinen_musik_ordner_oeffnen", "ascan_list_aircraft", "ascan_collect",
        "xplane_check_install_path", "kabinen_musik_pruefen", "auto_start_set_enabled", "set_minimize_to_tray",
        "xplane_band_set_enabled", "panel_server_set_enabled", "telemetrie_csv_speichern", "landing_backup_now",
        "remote_server_stop",
    ] {
        assert_eq!(liste.get(b).map(String::as_str), Some("fachlich"), "{b}");
    }
}

/// Die Capabilities der App-Befehle sind aus `befehle.json` erzeugt
/// (`scripts/rechte-erzeugen.mjs`). Weicht eine Datei von der Liste ab —
/// Stufe geaendert und nicht neu erzeugt, Recht von Hand ergaenzt —, wird
/// das hier rot statt still ein Systembefehl in der Oberflaeche zu landen.
#[test]
fn rechte_dateien_passen_zur_liste() {
    let liste = liste();
    let recht = |b: &str| format!("allow-{}", b.replace('_', "-"));
    let datei = |name: &str| -> serde_json::Value {
        let pfad = format!("{}/capabilities/{name}.json", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(&pfad).unwrap()).unwrap()
    };
    let rechte = |v: &serde_json::Value| -> BTreeSet<String> {
        v["permissions"].as_array().unwrap().iter().map(|p| p.as_str().unwrap().to_string()).collect()
    };
    let ui: BTreeSet<String> = liste.iter().filter(|(_, s)| *s != "system").map(|(b, _)| recht(b)).collect();
    let alle: BTreeSet<String> = liste.keys().map(|b| recht(b)).collect();
    assert_eq!(rechte(&datei("ui")), ui, "capabilities/ui.json veraltet: node scripts/rechte-erzeugen.mjs");
    assert_eq!(rechte(&datei("huelle")), alle, "capabilities/huelle.json veraltet: node scripts/rechte-erzeugen.mjs");
    // App-Befehle nur ueber diese beiden Dateien; default.json traegt nur Plugin-Rechte.
    for p in rechte(&datei("default")) {
        assert!(p.contains(':'), "default.json: App-Recht {p} gehoert in ui.json/huelle.json");
    }
}

/// P-8.02: Ein Fenster bekommt entweder die Rechte der Oberflaeche oder die
/// der Huelle, nie beide (Tauri vereinigt Capabilities je Fenster). Die
/// Oberflaechen-Fenster duerfen keinen Systembefehl.
#[test]
fn fenster_haben_genau_eine_rechte_datei() {
    let fenster = |name: &str| -> BTreeSet<String> {
        let pfad = format!("{}/capabilities/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&pfad).unwrap()).unwrap();
        v["windows"].as_array().unwrap().iter().map(|w| w.as_str().unwrap().to_string()).collect()
    };
    let ui = fenster("ui");
    let huelle = fenster("huelle");
    assert!(ui.contains("main") && ui.contains("telemetrie"), "Haupt- und Telemetriefenster laden die Oberflaeche: {ui:?}");
    assert_eq!(huelle, BTreeSet::from(["huelle".to_string()]), "nur das Fenster huelle darf Systembefehle");
    assert!(ui.is_disjoint(&huelle), "Fenster in ui.json UND huelle.json: {:?}", ui.intersection(&huelle).collect::<Vec<_>>());
}

/// `ui-befehle-4` (30.10.2026, Thomas: Paketstand sichtbar, ohne zweimal
/// starten): `paket_info` liest nur. „Auf neues Paket pruefen“ fragt nur live
/// (Host fest, Token im Kern) und „Jetzt neu starten“ startet nur ohne
/// offenen Flug und nur mit wartendem neuem Stand (Kern prueft
/// `neustart_erlaubt`) — der Knopf ist die Bestaetigung, wie bei den 17
/// umgestuften Befehlen; keine zweite Rueckfrage der Huelle.
#[test]
fn paket_befehle_der_ui_befehle_4() {
    let liste = liste();
    assert_eq!(liste.get("paket_info").map(String::as_str), Some("lesen"));
    assert_eq!(liste.get("paket_datei").map(String::as_str), Some("lesen"));
    assert_eq!(liste.get("paket_jetzt_pruefen").map(String::as_str), Some("fachlich"));
    assert_eq!(liste.get("paket_neu_starten").map(String::as_str), Some("fachlich"));
}

/// „Meine Anfragen“ (30.10.2026, Thomas: alte Tickets verschwanden, ohne
/// Betreff und Status nicht zuzuordnen): `hilfe_tickets` liest nur die
/// eigenen Tickets aus dem HelpDesk, Host fest, Schluessel im Kern — Stufe
/// fachlich wie `hilfe_anfordern`, keine Rueckfrage der Huelle.
#[test]
fn hilfe_tickets_ist_fachlich() {
    let liste = liste();
    assert_eq!(liste.get("hilfe_tickets").map(String::as_str), Some("fachlich"));
}
