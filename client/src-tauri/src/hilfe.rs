//! Hilfe anfordern aus dem Client (Pflichtenheft P-9.20 bis P-9.22, N-06).
//!
//! Zwei Schritte, in dieser Reihenfolge:
//!
//! 1. Die **Diagnose** (Tagesdateien des Client-Logs von heute und gestern,
//!    dazu Version, Sim, Flugzeugprofil, Paket-Generation, Name der letzten
//!    Flugdatei) geht an live: `POST /api/diagnose/ticket`, Anmeldung wie
//!    beim Flugprotokoll (Basic mit den MQTT-Zugangsdaten), Kopf
//!    `x-diagnose-meta`. live antwortet mit der Kennung `D-<12 hex>`.
//! 2. Das **Ticket** geht an das HelpDesk-Modul auf german-sky-group.eu:
//!    `POST /api/helpdesk/tickets` mit dem phpVMS-Schluessel des Piloten,
//!    Felder `subject`, `body`, `diagnose`. Grosse Dateien landen so nie in
//!    phpVMS; das Ticket traegt nur die Kennung. Den **Pfad** darf das
//!    Laufzeit-Paket aendern (`endpunkte.json` → `helpdesk_pfad`, geprueft in
//!    `paket-endpunkte`), der Host bleibt fest `ALLOWED_PHPVMS_HOST`.
//!
//! **Zusatzfelder** (Faehigkeit `hilfe-formular-1`): Welche weiteren
//! Textfelder das Formular hat (Flugnummer, Simulator …), beschreibt das
//! Paket in `tabellen/hilfe-formular.json`; die Seite zeigt sie ohne
//! Codeaenderung, der Kern nimmt nur dort genannte Namen an, prueft sie
//! ([`zusatz_pruefen`]) und reicht sie als `custom_fields[<name>]` ans
//! Ticket — dasselbe Feld, unter dem das HelpDesk-Formular seine
//! Admin-Zusatzfelder schickt. Ein neues Feld braucht so Paket (und im
//! HelpDesk hoechstens ein Admin-Zusatzfeld), aber kein Client-Release.
//!
//! **Bilder** (Screenshots, Beta-1-Rueckmeldung „man kann kein Bild
//! hochladen“) gehen NICHT in die Diagnose, sondern als Anhang direkt ans
//! Ticket (multipart `attachments[]`) — dort liest der Betreiber, das Modul
//! hat Tabelle, Rechte und Anzeige dafuer. Die Oberflaeche verkleinert sie
//! vorher (laengste Kante, JPEG/WebP), hier wird nur noch geprueft: hoechstens
//! [`BILD_MAX_ANZAHL`], je hoechstens [`BILD_MAX_BYTES`], Typ an den ersten
//! Bytes (PNG/JPEG/WebP), nicht an einer Angabe der Oberflaeche. Dateiname
//! und MIME-Typ setzt Rust selbst (`bild-1.png` …). Scheitert das Ticket,
//! sind die Bilder nicht angekommen — der Dialog sagt das.
//!
//! Scheitert Schritt 2 (HelpDesk noch nicht veroeffentlicht → 404, Kategorie
//! fehlt → 503 …), bleibt die Diagnose trotzdem auf live — der Pilot bekommt
//! die Kennung zum Weitergeben. Scheitert Schritt 1, geht das Ticket ohne
//! Diagnose raus.
//!
//! Warum hier und nicht in `aeroacars-mqtt::log_upload` / `api-client`: Die
//! Crates sind in diesem Paket fuer einen parallel laufenden Testauftrag
//! gesperrt; `DiagnosePaket` und der phpVMS-Schluessel sind dort privat, und
//! das HelpDesk antwortet ohne `{ data }`-Huelle. Dieselben Sicherungen wie
//! dort gelten hier: kein Upload bei gesetztem `RUST_LOG` (rumqttc schreibt
//! dann Zugangsdaten mit), Kontopruefung im Moment des Lesens der
//! Zugangsdaten, keine Weiterleitung mit dem Schluessel zu einem anderen Host.

use crate::UiError;
use base64::Engine as _;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Hoechstens so viel rohes Log geht mit; ist mehr da, faellt der ANFANG weg
/// (wie `log_upload::DIAGNOSE_MAX_ROH`: der Fehler steht am Ende).
const DIAGNOSE_MAX_ROH: usize = 8 * 1024 * 1024;
/// Grenzen des Servers (ticketDiagnose.ts `metaLesen`) bzw. des HelpDesk
/// (`subject` max 255, `body` max 20000). Betreff 200, weil er auch in die
/// Diagnose geht.
const BETREFF_MAX: usize = 200;
const TEXT_MAX: usize = 20_000;
const META_FELD_MAX: usize = 120;
/// Bilder je Anfrage und Bytes je Bild — dieselben Grenzen wie im
/// HelpDesk-Controller (`IMAGE_MAX_COUNT`, `IMAGE_MAX_KB` = 5120).
pub(crate) const BILD_MAX_ANZAHL: usize = 5;
pub(crate) const BILD_MAX_BYTES: usize = 5 * 1024 * 1024;
/// Zusatzfelder: hoechstens so viele, Wert je hoechstens so viele Zeichen.
pub(crate) const ZUSATZ_MAX_ANZAHL: usize = 12;
pub(crate) const ZUSATZ_WERT_MAX: usize = 2000;
/// Namen, die ein Zusatzfeld nie tragen darf: die festen Felder des Tickets,
/// die Spalten, die das HelpDesk selbst fuellt, und die Argumente von
/// `hilfe_anfordern`.
const ZUSATZ_GESPERRT: &[&str] = &[
    "subject", "body", "diagnose", "attachments", "custom_fields", "zusatz", "pirep_id", "flight_id", "aircraft_id",
    "category_id", "source", "status", "priority", "betreff", "text", "bilder",
];
/// Teilwoerter, die nach einem Geheimnis klingen: ein solches Feld wird nie
/// angenommen (auch `monkey` faellt damit weg — bewusst grob).
const ZUSATZ_GEHEIMNIS: &[&str] = &["key", "token", "password", "passwort", "secret", "geheim"];

/// Ein Bild aus dem Dialog: nur die Bytes (Base64). Name und Typ bestimmt Rust.
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct HilfeBild {
    pub daten: String,
}

/// Geprueftes Bild, bereit fuer den Versand.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Bild {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub endung: &'static str,
}

/// Was mit der Diagnose mitgeht — der Pilot sieht es vor dem Senden (P-9.22).
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub(crate) struct HilfeMeta {
    pub client_version: String,
    pub sim: String,
    pub sim_version: Option<String>,
    pub flugzeug_profil: Option<String>,
    pub addon: Option<String>,
    pub paket_generation: String,
    pub letzte_flugdatei: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct LogDatei {
    pub name: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct HilfeVorschau {
    pub meta: HilfeMeta,
    pub dateien: Vec<LogDatei>,
    /// `RUST_LOG` gesetzt: Das Log kann Zugangsdaten enthalten, es geht
    /// dann NICHT mit (das Ticket schon).
    pub log_gesperrt: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct Ticket {
    pub id: Option<serde_json::Value>,
    pub ticket_number: Option<String>,
    pub url: Option<String>,
    /// Zahl der am Ticket gespeicherten Bilder (`anhaenge` des HelpDesk);
    /// `None`, wenn das Modul das Feld noch nicht kennt.
    pub anhaenge: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub(crate) struct HilfeErgebnis {
    /// Kennung der Diagnose auf live (`D-…`), auch wenn das Ticket scheitert.
    pub kennung: Option<String>,
    pub link: Option<String>,
    /// Warum die Diagnose nicht mitging (Schluessel unter `hilfe.fehler.*`).
    pub diagnose_fehler: Option<String>,
    pub ticket: Option<Ticket>,
    /// Warum das Ticket nicht angelegt wurde (Schluessel unter `hilfe.fehler.*`).
    pub ticket_fehler: Option<String>,
    /// Wie viele Bilder mitgeschickt wurden (der Dialog vergleicht mit
    /// `ticket.anhaenge`).
    pub bilder_gesendet: usize,
}

/// Betreff und Text pruefen (der Kern prueft jeden Aufruf, Stufe „fachlich“).
pub(crate) fn eingaben_pruefen(betreff: &str, text: &str) -> Result<(String, String), UiError> {
    let b = betreff.trim();
    let t = text.trim();
    if b.is_empty() || b.chars().count() > BETREFF_MAX || b.chars().any(char::is_control) {
        return Err(UiError::new("hilfe_betreff", "Betreff fehlt oder ist zu lang"));
    }
    if t.is_empty() || t.chars().count() > TEXT_MAX {
        return Err(UiError::new("hilfe_text", "Text fehlt oder ist zu lang"));
    }
    Ok((b.to_string(), t.to_string()))
}

/// Name eines Zusatzfelds: `[a-z][a-z0-9_]{0,31}`, nicht gesperrt, kein
/// Geheimnis-Muster.
pub(crate) fn zusatz_name_ok(name: &str) -> bool {
    let b = name.as_bytes();
    !b.is_empty()
        && b.len() <= 32
        && b[0].is_ascii_lowercase()
        && b.iter().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
        && !ZUSATZ_GESPERRT.contains(&name)
        && !ZUSATZ_GEHEIMNIS.iter().any(|g| name.contains(g))
}

/// Zusatzfelder aus der Oberflaeche pruefen. Nur Namen, die die
/// Formularbeschreibung des geltenden Stands nennt (`erlaubt`), nur Text
/// (getrimmt, hoechstens [`ZUSATZ_WERT_MAX`] Zeichen, keine Steuerzeichen
/// ausser Zeilenumbruch und Tab). Leere Werte fallen weg. Ein schlechtes Feld
/// verwirft alle (kein halbes Senden). Pflichtfelder prueft die Oberflaeche,
/// nicht der Kern: sonst koennte eine aeltere Oberflaeche (oder die
/// LAN-Bruecke) ohne das Feld gar keine Hilfe mehr anfordern.
pub(crate) fn zusatz_pruefen(zusatz: &BTreeMap<String, String>, erlaubt: &[String]) -> Result<BTreeMap<String, String>, UiError> {
    if zusatz.len() > ZUSATZ_MAX_ANZAHL {
        return Err(UiError::new("hilfe_zusatz", "too many extra fields"));
    }
    let mut aus = BTreeMap::new();
    for (name, wert) in zusatz {
        if !zusatz_name_ok(name) || !erlaubt.iter().any(|e| e == name) {
            return Err(UiError::new("hilfe_zusatz", format!("extra field {name:?} not allowed")));
        }
        let w = wert.trim();
        if w.chars().count() > ZUSATZ_WERT_MAX || w.chars().any(|c| c.is_control() && c != '\n' && c != '\t') {
            return Err(UiError::new("hilfe_zusatz", format!("extra field {name:?}: value invalid")));
        }
        if !w.is_empty() {
            aus.insert(name.clone(), w.to_string());
        }
    }
    Ok(aus)
}

/// Was der Kern aus `tabellen/hilfe-formular.json` braucht: die Namen der
/// Zusatzfelder. Bezeichnungen, Typen, Pflicht und Optionen liest nur die
/// Oberflaeche — die Datei darf dafuer weitere Schluessel tragen.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct HilfeFormular {
    pub namen: Vec<String>,
}

#[derive(Deserialize)]
struct FormularDatei {
    format: u32,
    #[serde(default)]
    felder: Vec<FormularFeld>,
}

#[derive(Deserialize)]
struct FormularFeld {
    name: String,
    typ: String,
    #[serde(default)]
    optionen: Vec<FormularOption>,
}

#[derive(Deserialize)]
struct FormularOption {
    wert: String,
}

/// Liest `tabellen/hilfe-formular.json` (fail-closed, im Selbsttest des
/// Pakets): Format 1, hoechstens [`ZUSATZ_MAX_ANZAHL`] Felder, jeder Name
/// besteht dieselbe Pruefung wie beim Senden ([`zusatz_name_ok`]) und kommt
/// nur einmal vor, Typ `[a-z]{1,20}`, `auswahl` mit mindestens einer Option,
/// jeder Optionswert ein gueltiger Zusatzwert und nur einmal. Sonst zeigte die Seite ein
/// Feld, dessen Wert der Kern beim Senden ablehnt.
pub(crate) fn hilfe_formular_lesen(json: &str) -> Result<HilfeFormular, String> {
    let d: FormularDatei = serde_json::from_str(json).map_err(|e| format!("hilfe-formular.json: {e}"))?;
    if d.format != 1 {
        return Err(format!("hilfe-formular.json: Format {} unbekannt", d.format));
    }
    if d.felder.len() > ZUSATZ_MAX_ANZAHL {
        return Err("hilfe-formular.json: zu viele Felder".into());
    }
    let mut namen: Vec<String> = Vec::new();
    for f in &d.felder {
        if !zusatz_name_ok(&f.name) || namen.contains(&f.name) {
            return Err(format!("hilfe-formular.json: Name {:?} ungueltig oder doppelt", f.name));
        }
        if f.typ.is_empty() || f.typ.len() > 20 || !f.typ.bytes().all(|b| b.is_ascii_lowercase()) {
            return Err(format!("hilfe-formular.json: {}: Typ ungueltig", f.name));
        }
        if f.typ == "auswahl" && f.optionen.is_empty() {
            return Err(format!("hilfe-formular.json: {}: Auswahl ohne Optionen", f.name));
        }
        for (i, o) in f.optionen.iter().enumerate() {
            let w = o.wert.trim();
            if f.optionen[..i].iter().any(|x| x.wert == o.wert) {
                return Err(format!("hilfe-formular.json: {}: Optionswert doppelt", f.name));
            }
            if w.is_empty() || w != o.wert || w.chars().count() > ZUSATZ_WERT_MAX || w.chars().any(char::is_control) {
                return Err(format!("hilfe-formular.json: {}: Optionswert ungueltig", f.name));
            }
        }
        namen.push(f.name.clone());
    }
    Ok(HilfeFormular { namen })
}

/// Name des Zusatzfelds im Ticket (multipart) — wie die Admin-Zusatzfelder
/// im HelpDesk-Formular (`CustomFieldService::inputNameFor`).
pub(crate) fn zusatz_feldname(name: &str) -> String {
    format!("custom_fields[{name}]")
}

/// Adresse der Ticket-Schnittstelle: Host fest, Pfad aus dem Paket (in
/// `paket-endpunkte` geprueft).
pub(crate) fn ticket_url(pfad: &str) -> String {
    format!("https://{}{}", crate::ALLOWED_PHPVMS_HOST, pfad)
}

/// Textfelder des multipart-Koerpers: feste Felder, dann die Zusatzfelder
/// als `custom_fields[<name>]`.
pub(crate) fn multipart_felder(betreff: &str, text: &str, kennung: Option<&str>, zusatz: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut felder = vec![("subject".to_string(), betreff.to_string()), ("body".to_string(), text.to_string())];
    if let Some(k) = kennung {
        felder.push(("diagnose".into(), k.to_string()));
    }
    felder.extend(zusatz.iter().map(|(n, w)| (zusatz_feldname(n), w.clone())));
    felder
}

/// JSON-Koerper des Tickets ohne Bilder. Ohne Zusatzfelder genau wie vor
/// `hilfe-formular-1` (kein `custom_fields`-Schluessel).
pub(crate) fn json_koerper(betreff: &str, text: &str, kennung: Option<&str>, zusatz: &BTreeMap<String, String>) -> serde_json::Value {
    let mut k = serde_json::json!({ "subject": betreff, "body": text, "diagnose": kennung });
    if !zusatz.is_empty() {
        k["custom_fields"] = serde_json::json!(zusatz);
    }
    k
}

/// Typ an den ersten Bytes: PNG, JPEG oder WebP — sonst `None`.
pub(crate) fn bild_typ(b: &[u8]) -> Option<(&'static str, &'static str)> {
    if b.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some(("image/png", "png"))
    } else if b.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(("image/jpeg", "jpg"))
    } else if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        Some(("image/webp", "webp"))
    } else {
        None
    }
}

/// Bilder aus dem Dialog pruefen (Anzahl, Groesse, Typ an den Bytes).
pub(crate) fn bilder_pruefen(bilder: &[HilfeBild]) -> Result<Vec<Bild>, UiError> {
    if bilder.len() > BILD_MAX_ANZAHL {
        return Err(UiError::new("hilfe_bild_anzahl", "too many images"));
    }
    // Base64 ist 4/3 so lang: alles darueber kann nur zu gross sein, ohne zu dekodieren.
    let b64_max = BILD_MAX_BYTES.div_ceil(3) * 4;
    bilder
        .iter()
        .map(|b| {
            if b.daten.len() > b64_max {
                return Err(UiError::new("hilfe_bild_gross", "image too large"));
            }
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b.daten.as_bytes())
                .map_err(|_| UiError::new("hilfe_bild_typ", "image not readable"))?;
            if bytes.len() > BILD_MAX_BYTES {
                return Err(UiError::new("hilfe_bild_gross", "image too large"));
            }
            let (mime, endung) = bild_typ(&bytes).ok_or_else(|| UiError::new("hilfe_bild_typ", "not a PNG/JPEG/WebP image"))?;
            Ok(Bild { bytes, mime, endung })
        })
        .collect()
}

/// multipart/form-data fuer das Ticket mit Bildern: Textfelder, dann
/// `attachments[]` als `bild-<n>.<endung>` mit dem an den Bytes erkannten Typ.
/// Von Hand gebaut, damit reqwest kein weiteres Feature (und Cargo.lock keine
/// neue Abhaengigkeit) braucht.
pub(crate) fn multipart_koerper(grenze: &str, felder: &[(&str, &str)], bilder: &[Bild]) -> Vec<u8> {
    let mut k = Vec::new();
    for (name, wert) in felder {
        k.extend_from_slice(format!("--{grenze}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
        k.extend_from_slice(wert.as_bytes());
        k.extend_from_slice(b"\r\n");
    }
    for (i, b) in bilder.iter().enumerate() {
        k.extend_from_slice(
            format!(
                "--{grenze}\r\nContent-Disposition: form-data; name=\"attachments[]\"; filename=\"bild-{}.{}\"\r\nContent-Type: {}\r\n\r\n",
                i + 1,
                b.endung,
                b.mime
            )
            .as_bytes(),
        );
        k.extend_from_slice(&b.bytes);
        k.extend_from_slice(b"\r\n");
    }
    k.extend_from_slice(format!("--{grenze}--\r\n").as_bytes());
    k
}

fn kuerzen(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Kopf `x-diagnose-meta`: base64 von JSON, nur die Felder, die live kennt
/// (`META_FELDER` in ticketDiagnose.ts), jeweils gekuerzt.
pub(crate) fn meta_kopf(meta: &HilfeMeta, betreff: &str) -> String {
    let mut m = serde_json::Map::new();
    let mut setze = |k: &str, v: Option<&str>| {
        if let Some(v) = v.filter(|v| !v.is_empty()) {
            m.insert(k.into(), serde_json::Value::String(kuerzen(v, if k == "betreff" { BETREFF_MAX } else { META_FELD_MAX })));
        }
    };
    setze("client_version", Some(&meta.client_version));
    setze("sim", Some(&meta.sim));
    setze("sim_version", meta.sim_version.as_deref());
    setze("flugzeug_profil", meta.flugzeug_profil.as_deref());
    setze("addon", meta.addon.as_deref());
    setze("paket_generation", Some(&meta.paket_generation));
    setze("letzte_flugdatei", meta.letzte_flugdatei.as_deref());
    setze("betreff", Some(betreff));
    base64::engine::general_purpose::STANDARD.encode(serde_json::Value::Object(m).to_string())
}

/// Liest die Logdateien in Reihenfolge, kuerzt vorn auf `max` und packt.
pub(crate) fn paket_bauen(dateien: &[PathBuf], max: usize) -> Result<Vec<u8>, String> {
    let mut roh: Vec<u8> = Vec::new();
    for pfad in dateien {
        let Ok(teil) = std::fs::read(pfad) else { continue };
        // Nur der Dateiname: der volle Pfad naennte Benutzername und Ordner,
        // die die Vorschau nicht zeigt (Codex 30.10.).
        let name = pfad.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        roh.extend_from_slice(format!("\n===== {name} =====\n").as_bytes());
        roh.extend_from_slice(&teil);
    }
    if roh.is_empty() {
        return Err("keine_datei".into());
    }
    if roh.len() > max {
        roh = roh.split_off(roh.len() - max);
    }
    let mut gz = GzEncoder::new(Vec::with_capacity(roh.len() / 8), flate2::Compression::default());
    gz.write_all(&roh).map_err(|e| e.to_string())?;
    gz.finish().map_err(|e| e.to_string())
}

/// Antwort von live auf den Diagnose-Upload → Kennung und Link oder Fehlerschluessel.
pub(crate) fn diagnose_antwort(status: u16, body: &str) -> Result<(String, Option<String>), &'static str> {
    match status {
        200 | 201 => {
            let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "diagnose_antwort")?;
            let kennung = v.get("kennung").and_then(|k| k.as_str()).unwrap_or_default();
            let gueltig = kennung.len() == 14
                && kennung.starts_with("D-")
                && kennung[2..].bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
            if !gueltig {
                return Err("diagnose_antwort");
            }
            let link = v.get("link").and_then(|l| l.as_str()).map(str::to_string);
            Ok((kennung.to_string(), link))
        }
        401 | 403 => Err("diagnose_anmeldung"),
        413 => Err("diagnose_zu_gross"),
        429 => Err("diagnose_zu_viele"),
        _ => Err("diagnose_nicht_erreichbar"),
    }
}

/// Antwort des HelpDesk → Ticket oder Fehlerschluessel. 404 = Route fehlt
/// (Modul noch nicht veroeffentlicht); der Koerper kann dann HTML sein.
pub(crate) fn ticket_antwort(status: u16, body: &str) -> Result<Ticket, &'static str> {
    let json = || serde_json::from_str::<serde_json::Value>(body).ok();
    match status {
        200 | 201 => {
            let v = json().ok_or("ticket_antwort")?;
            Ok(Ticket {
                id: v.get("id").cloned(),
                ticket_number: v.get("ticket_number").and_then(|x| x.as_str()).map(str::to_string),
                url: v
                    .get("url")
                    .and_then(|x| x.as_str())
                    .filter(|u| u.starts_with(&format!("https://{}/", crate::ALLOWED_PHPVMS_HOST)))
                    .map(str::to_string),
                anhaenge: v.get("anhaenge").and_then(|x| x.as_u64()),
            })
        }
        401 => Err("ticket_anmeldung"),
        403 => Err("ticket_kein_recht"),
        404 => Err("ticket_helpdesk_fehlt"),
        // Ein abgelehntes Bild nennt der Controller unter `felder.attachments.<n>`.
        422 if json()
            .and_then(|v| v.get("felder").and_then(|f| f.as_object()).map(|f| f.keys().any(|k| k.starts_with("attachments"))))
            == Some(true) =>
        {
            Err("ticket_bild_ungueltig")
        }
        // Ein abgelehntes Zusatzfeld nennt der Controller unter `felder.custom_fields…`.
        422 if json()
            .and_then(|v| v.get("felder").and_then(|f| f.as_object()).map(|f| f.keys().any(|k| k.starts_with("custom_fields"))))
            == Some(true) =>
        {
            Err("ticket_zusatz_ungueltig")
        }
        422 => Err("ticket_ungueltig"),
        // Webserver/PHP-Grenze fuer den Koerper (nur mit Bildern denkbar).
        413 => Err("ticket_zu_gross"),
        503 if json().and_then(|v| v.get("error").and_then(|e| e.as_str()).map(|e| e == "kategorie_fehlt")) == Some(true) => {
            Err("ticket_kategorie_fehlt")
        }
        _ => Err("ticket_nicht_erreichbar"),
    }
}

fn letzte_flugdatei(app: &AppHandle) -> Option<String> {
    let ordner = app.path().app_data_dir().ok()?.join("flight_logs");
    std::fs::read_dir(ordner)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.file_name().to_string_lossy().into_owned())))
        .max()
        .map(|(_, n)| n)
}

fn meta_sammeln(app: &AppHandle) -> HilfeMeta {
    let snap = crate::current_snapshot(app);
    HilfeMeta {
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        sim: crate::kind_str(crate::read_sim_config(app).kind).to_string(),
        sim_version: snap.as_ref().and_then(|s| s.sim_version.clone()),
        flugzeug_profil: snap.as_ref().map(|s| s.aircraft_profile.label().to_string()),
        addon: snap.as_ref().and_then(|s| s.aircraft_title.clone()),
        paket_generation: crate::laufzeit_paket::stand().id.to_string(),
        letzte_flugdatei: letzte_flugdatei(app),
    }
}

fn log_gesperrt() -> bool {
    std::env::var_os("RUST_LOG").is_some()
}

/// Was mitgeht, ohne etwas zu senden (P-9.22). Stufe „lesen“.
#[tauri::command]
pub(crate) fn hilfe_vorschau(app: AppHandle) -> HilfeVorschau {
    let dateien = crate::diagnose_logs_fuer_upload()
        .into_iter()
        .map(|p| LogDatei {
            name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            bytes: std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0),
        })
        .collect();
    HilfeVorschau { meta: meta_sammeln(&app), dateien, log_gesperrt: log_gesperrt() }
}

fn angemeldet(app: &AppHandle) -> Option<i64> {
    *app.state::<crate::AppState>().authenticated_pilot_id.lock().expect("authenticated_pilot_id lock")
}

/// Diagnose hochladen, dann Ticket anlegen. Stufe „fachlich“: Netz nur zu
/// festen Hosts (live aus dem Paket, Host gebunden; phpVMS fest
/// `ALLOWED_PHPVMS_HOST`), mit dem Schluessel des angemeldeten Piloten;
/// Betreff und Text prueft der Kern.
#[tauri::command]
pub(crate) async fn hilfe_anfordern(
    app: AppHandle,
    betreff: String,
    text: String,
    bilder: Option<Vec<HilfeBild>>,
    zusatz: Option<BTreeMap<String, String>>,
) -> Result<HilfeErgebnis, UiError> {
    let (betreff, text) = eingaben_pruefen(&betreff, &text)?;
    // Bilder und Zusatzfelder vor jedem Netzweg pruefen: ein ungueltiges
    // sendet gar nichts.
    let bilder = bilder_pruefen(bilder.as_deref().unwrap_or_default())?;
    let zusatz = zusatz_pruefen(&zusatz.unwrap_or_default(), &crate::laufzeit_paket::stand().hilfe_formular.namen)?;
    let Some(pilot) = angemeldet(&app) else {
        return Err(UiError::new("not_logged_in", "no active session"));
    };
    let mut erg = HilfeErgebnis { bilder_gesendet: bilder.len(), ..Default::default() };

    // 1. Diagnose an live.
    if log_gesperrt() {
        erg.diagnose_fehler = Some("diagnose_rust_log".into());
    } else {
        let dateien = crate::diagnose_logs_fuer_upload();
        let gz = tauri::async_runtime::spawn_blocking(move || paket_bauen(&dateien, DIAGNOSE_MAX_ROH))
            .await
            .map_err(|e| UiError::new("hilfe_intern", e.to_string()))?;
        match gz {
            Err(_) => erg.diagnose_fehler = Some("diagnose_keine_datei".into()),
            Ok(gz) => {
                let kopf = meta_kopf(&meta_sammeln(&app), &betreff);
                // Zugangsdaten erst jetzt lesen — und nur, wenn noch derselbe Pilot angemeldet ist.
                let zugang = (angemeldet(&app) == Some(pilot))
                    .then(|| {
                        let u = secrets::load_api_key(crate::MQTT_KEYRING_USERNAME).ok().flatten()?;
                        let p = secrets::load_api_key(crate::MQTT_KEYRING_PASSWORD).ok().flatten()?;
                        Some((u, p))
                    })
                    .flatten();
                match zugang {
                    None => erg.diagnose_fehler = Some("diagnose_anmeldung".into()),
                    Some((u, p)) => {
                        let _sperre = crate::UPLOAD_SPERRE.lock().await;
                        let url = format!("{}/api/diagnose/ticket", crate::live_zugang::live_basis());
                        let antwort = async {
                            let c = reqwest::Client::builder()
                                .timeout(Duration::from_secs(60))
                                .redirect(reqwest::redirect::Policy::none())
                                .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
                                .build()
                                .ok()?;
                            let r = c
                                .post(&url)
                                .basic_auth(u, Some(p))
                                .header("content-type", "application/gzip")
                                .header("x-diagnose-meta", kopf)
                                .body(gz)
                                .send()
                                .await
                                .ok()?;
                            let s = r.status().as_u16();
                            Some((s, r.text().await.unwrap_or_default()))
                        }
                        .await;
                        match antwort.map(|(s, b)| diagnose_antwort(s, &b)) {
                            Some(Ok((kennung, link))) => {
                                tracing::info!(kennung = %kennung, "Hilfe: Diagnose auf live abgelegt");
                                erg.kennung = Some(kennung);
                                erg.link = link;
                            }
                            Some(Err(code)) => erg.diagnose_fehler = Some(code.into()),
                            None => erg.diagnose_fehler = Some("diagnose_nicht_erreichbar".into()),
                        }
                    }
                }
            }
        }
    }

    // 2. Ticket im HelpDesk.
    let schluessel = (angemeldet(&app) == Some(pilot)).then(crate::secrets_load_phpvms_key).flatten();
    let Some(schluessel) = schluessel else {
        erg.ticket_fehler = Some("ticket_anmeldung".into());
        return Ok(erg);
    };
    // Host fest, Pfad aus dem Paket (geprueft in `paket-endpunkte`).
    let url = ticket_url(paket_endpunkte::endpunkte().helpdesk_pfad());
    let koerper = json_koerper(&betreff, &text, erg.kennung.as_deref(), &zusatz);
    // Mit Bildern multipart (Feldnamen wie im HelpDesk-Formular), sonst JSON wie bisher.
    let mehrteilig = (!bilder.is_empty()).then(|| {
        let grenze = format!("aeroacars-{}", uuid::Uuid::new_v4().simple());
        let felder = multipart_felder(&betreff, &text, erg.kennung.as_deref(), &zusatz);
        let felder: Vec<(&str, &str)> = felder.iter().map(|(n, w)| (n.as_str(), w.as_str())).collect();
        (format!("multipart/form-data; boundary={grenze}"), multipart_koerper(&grenze, &felder, &bilder))
    });
    let antwort = async {
        let c = reqwest::Client::builder()
            // Bis 25 MB Bilder brauchen auf langsamer Leitung mehr als 30 s.
            .timeout(Duration::from_secs(if mehrteilig.is_some() { 180 } else { 30 }))
            // Der Schluessel darf nie einer Weiterleitung folgen.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        let anfrage = c.post(&url).header("X-API-Key", schluessel).header("Accept", "application/json");
        let anfrage = match mehrteilig {
            Some((art, k)) => anfrage.header("content-type", art).body(k),
            None => anfrage.json(&koerper),
        };
        let r = anfrage.send().await.ok()?;
        let s = r.status().as_u16();
        Some((s, r.text().await.unwrap_or_default()))
    }
    .await;
    match antwort.map(|(s, b)| ticket_antwort(s, &b)) {
        Some(Ok(t)) => {
            tracing::info!(ticket = ?t.ticket_number, "Hilfe: Ticket angelegt");
            erg.ticket = Some(t);
        }
        Some(Err(code)) => erg.ticket_fehler = Some(code.into()),
        None => erg.ticket_fehler = Some("ticket_nicht_erreichbar".into()),
    }
    Ok(erg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eingaben_werden_geprueft() {
        assert!(eingaben_pruefen("", "x").is_err());
        assert!(eingaben_pruefen("  ", "x").is_err());
        assert!(eingaben_pruefen("Betreff", "  ").is_err());
        assert!(eingaben_pruefen(&"b".repeat(201), "x").is_err());
        assert!(eingaben_pruefen("a\nb", "x").is_err());
        assert!(eingaben_pruefen("Betreff", &"t".repeat(20_001)).is_err());
        assert_eq!(eingaben_pruefen(" Betreff ", " Text\nzwei ").unwrap(), ("Betreff".into(), "Text\nzwei".into()));
        assert!(eingaben_pruefen(&"ä".repeat(200), &"ö".repeat(20_000)).is_ok());
    }

    #[test]
    fn meta_kopf_nur_bekannte_felder_gekuerzt() {
        let meta = HilfeMeta {
            client_version: "3.0.0".into(),
            sim: "msfs2024".into(),
            sim_version: Some("1.2".into()),
            flugzeug_profil: Some("x".repeat(500)),
            addon: None,
            paket_generation: "7-abc".into(),
            letzte_flugdatei: Some("GSG1.jsonl".into()),
        };
        let roh = base64::engine::general_purpose::STANDARD.decode(meta_kopf(&meta, &"b".repeat(300))).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&roh).unwrap();
        let o = v.as_object().unwrap();
        // genau die Namen aus ticketDiagnose.ts META_FELDER (ohne leere)
        let mut namen: Vec<&str> = o.keys().map(String::as_str).collect();
        namen.sort_unstable();
        assert_eq!(namen, ["betreff", "client_version", "flugzeug_profil", "letzte_flugdatei", "paket_generation", "sim", "sim_version"]);
        assert_eq!(o["flugzeug_profil"].as_str().unwrap().len(), 120);
        assert_eq!(o["betreff"].as_str().unwrap().len(), 200);
        assert_eq!(o["sim"], "msfs2024");
    }

    #[test]
    fn paket_behaelt_das_ende_und_ist_gzip() {
        let ordner = std::env::temp_dir().join(format!("hilfe-test-{}", std::process::id()));
        std::fs::create_dir_all(&ordner).unwrap();
        let a = ordner.join("a.log");
        let b = ordner.join("b.log");
        std::fs::write(&a, "ANFANG".repeat(100)).unwrap();
        std::fs::write(&b, "mitte ENDE").unwrap();
        let gz = paket_bauen(&[a.clone(), ordner.join("fehlt.log"), b.clone()], 64).unwrap();
        assert_eq!(&gz[..2], &[0x1f, 0x8b]);
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(&gz[..]), &mut text).unwrap();
        assert_eq!(text.len(), 64);
        assert!(text.ends_with("mitte ENDE"));
        // Kopfzeilen nennen nur den Dateinamen, nie den Ordner (Benutzername).
        let voll = String::from_utf8_lossy(&{
            let mut t = Vec::new();
            std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&paket_bauen(&[b.clone()], 1 << 20).unwrap()[..]), &mut t).unwrap();
            t
        })
        .into_owned();
        assert!(voll.contains("===== b.log ====="), "{voll}");
        assert!(!voll.contains(&*ordner.to_string_lossy()), "{voll}");
        assert_eq!(paket_bauen(&[ordner.join("fehlt.log")], 64), Err("keine_datei".to_string()));
        std::fs::remove_dir_all(&ordner).unwrap();
    }

    #[test]
    fn diagnose_antworten() {
        let ok = r#"{"ok":true,"kennung":"D-0123456789ab","link":"https://live.kant.ovh/admin/diagnose.html?k=D-0123456789ab"}"#;
        assert_eq!(diagnose_antwort(200, ok).unwrap().0, "D-0123456789ab");
        assert_eq!(diagnose_antwort(200, r#"{"kennung":"D-0123456789AB"}"#), Err("diagnose_antwort"));
        assert_eq!(diagnose_antwort(200, r#"{"kennung":"../etc"}"#), Err("diagnose_antwort"));
        assert_eq!(diagnose_antwort(429, r#"{"error":"zu_viele_diagnosen"}"#), Err("diagnose_zu_viele"));
        assert_eq!(diagnose_antwort(401, ""), Err("diagnose_anmeldung"));
        assert_eq!(diagnose_antwort(413, ""), Err("diagnose_zu_gross"));
        assert_eq!(diagnose_antwort(500, ""), Err("diagnose_nicht_erreichbar"));
    }

    #[test]
    fn ticket_antworten() {
        let ok = r#"{"id":12,"ticket_number":"HD-0012","url":"https://german-sky-group.eu/helpdesk/tickets/12"}"#;
        let t = ticket_antwort(201, ok).unwrap();
        assert_eq!(t.ticket_number.as_deref(), Some("HD-0012"));
        assert_eq!(t.url.as_deref(), Some("https://german-sky-group.eu/helpdesk/tickets/12"));
        // fremder Host im Link wird nicht weitergereicht
        let fremd = r#"{"id":1,"ticket_number":"HD-1","url":"https://boese.example/x"}"#;
        assert_eq!(ticket_antwort(201, fremd).unwrap().url, None);
        // Modul noch nicht veroeffentlicht: 404, Koerper HTML
        assert_eq!(ticket_antwort(404, "<html>Not Found</html>"), Err("ticket_helpdesk_fehlt"));
        assert_eq!(ticket_antwort(503, r#"{"error":"kategorie_fehlt"}"#), Err("ticket_kategorie_fehlt"));
        assert_eq!(ticket_antwort(503, "<html>Wartung</html>"), Err("ticket_nicht_erreichbar"));
        assert_eq!(ticket_antwort(403, r#"{"error":"kein_recht"}"#), Err("ticket_kein_recht"));
        assert_eq!(ticket_antwort(422, r#"{"error":"ungueltig"}"#), Err("ticket_ungueltig"));
        assert_eq!(ticket_antwort(401, ""), Err("ticket_anmeldung"));
        assert_eq!(ticket_antwort(201, "kein json"), Err("ticket_antwort"));
        // Bilder: Zahl der gespeicherten Anhaenge; ohne Feld (altes Modul) None.
        assert_eq!(ticket_antwort(201, r#"{"id":3,"ticket_number":"HD-3","anhaenge":2}"#).unwrap().anhaenge, Some(2));
        assert_eq!(ticket_antwort(201, ok).unwrap().anhaenge, None);
        // abgelehntes Bild: eigener Schluessel, damit der Dialog es benennt
        let bild_weg = r#"{"error":"ungueltig","felder":{"attachments.0":["bild_ungueltig"]}}"#;
        assert_eq!(ticket_antwort(422, bild_weg), Err("ticket_bild_ungueltig"));
        assert_eq!(ticket_antwort(422, r#"{"error":"ungueltig","felder":{"subject":["x"]}}"#), Err("ticket_ungueltig"));
        assert_eq!(ticket_antwort(413, "<html>Request Entity Too Large</html>"), Err("ticket_zu_gross"));
        // abgelehntes Zusatzfeld: eigener Schluessel
        let zusatz_weg = r#"{"error":"ungueltig","felder":{"custom_fields.flugnummer":["x"]}}"#;
        assert_eq!(ticket_antwort(422, zusatz_weg), Err("ticket_zusatz_ungueltig"));
    }

    fn karte(paare: &[(&str, &str)]) -> BTreeMap<String, String> {
        paare.iter().map(|(n, w)| (n.to_string(), w.to_string())).collect()
    }

    #[test]
    fn zusatzfelder_werden_geprueft() {
        let erlaubt: Vec<String> = ["flugnummer", "simulator", "a1_b"].map(String::from).to_vec();
        // gut: getrimmt, leere fallen weg, Zeilenumbruch und Tab erlaubt
        let ok = zusatz_pruefen(&karte(&[("flugnummer", " GSG1234 "), ("simulator", ""), ("a1_b", "x\n\ty")]), &erlaubt).unwrap();
        assert_eq!(ok, karte(&[("flugnummer", "GSG1234"), ("a1_b", "x\n\ty")]));
        assert_eq!(zusatz_pruefen(&BTreeMap::new(), &[]).unwrap(), BTreeMap::new());
        // nicht in der Beschreibung des Stands → abgelehnt (auch wenn der Name an sich gut ist)
        assert_eq!(zusatz_pruefen(&karte(&[("rueckruf", "x")]), &erlaubt).unwrap_err().code, "hilfe_zusatz");
        // Wertgrenze: genau 2000 Zeichen gehen (auch Umlaute), eines mehr nicht
        assert!(zusatz_pruefen(&karte(&[("flugnummer", &"ä".repeat(ZUSATZ_WERT_MAX))]), &erlaubt).is_ok());
        assert!(zusatz_pruefen(&karte(&[("flugnummer", &"ä".repeat(ZUSATZ_WERT_MAX + 1))]), &erlaubt).is_err());
        // Steuerzeichen ausser \n/\t
        for w in ["a\u{0}b", "a\rb", "a\u{1b}[31m"] {
            assert!(zusatz_pruefen(&karte(&[("flugnummer", w)]), &erlaubt).is_err(), "{w:?}");
        }
        // ein schlechtes unter guten verwirft alle
        assert!(zusatz_pruefen(&karte(&[("flugnummer", "x"), ("api_key", "y")]), &erlaubt).is_err());
        // Anzahl: 12 gehen, 13 nicht
        let viele: Vec<String> = (0..13).map(|i| format!("f{i}")).collect();
        let zwoelf: BTreeMap<String, String> = viele[..12].iter().map(|n| (n.clone(), "x".into())).collect();
        assert_eq!(zusatz_pruefen(&zwoelf, &viele).unwrap().len(), 12);
        let dreizehn: BTreeMap<String, String> = viele.iter().map(|n| (n.clone(), "x".into())).collect();
        assert!(zusatz_pruefen(&dreizehn, &viele).is_err());
    }

    #[test]
    fn zusatz_namen() {
        for gut in ["flugnummer", "a", "sim_version2", &"a".repeat(32)] {
            assert!(zusatz_name_ok(gut), "{gut}");
        }
        for boese in [
            "", "Flugnummer", "1abc", "_a", "a-b", "a b", "a.b", "a[b]", "flugnümmer", &"a".repeat(33),
            // feste Felder und Spalten
            "subject", "body", "diagnose", "attachments", "custom_fields", "pirep_id", "category_id", "betreff",
            // Geheimnis-Muster
            "api_key", "token", "pilot_password", "passwort", "client_secret", "geheimzahl", "monkey",
        ] {
            assert!(!zusatz_name_ok(boese), "{boese}");
        }
    }

    #[test]
    fn formularbeschreibung_wird_geprueft() {
        let gut = r#"{"format":1,"notiz":["x"],"felder":[
            {"name":"flugnummer","typ":"text","pflicht":true,"bezeichnung":{"de":"Flugnummer"},"neu_in_spaeter":1},
            {"name":"simulator","typ":"auswahl","optionen":[{"wert":"msfs2024","bezeichnung":{"de":"MSFS 2024"}}]}]}"#;
        assert_eq!(hilfe_formular_lesen(gut).unwrap().namen, ["flugnummer", "simulator"]);
        assert_eq!(hilfe_formular_lesen(r#"{"format":1,"felder":[]}"#).unwrap(), HilfeFormular::default());
        // Der eingebaute Stand ist lesbar.
        hilfe_formular_lesen(crate::laufzeit_paket::eingebaute_datei_text("tabellen/hilfe-formular.json")).unwrap();
        let feld = |f: &str| format!(r#"{{"format":1,"felder":[{f}]}}"#);
        for boese in [
            r#"{"format":2,"felder":[]}"#.to_string(),
            "kein json".to_string(),
            feld(r#"{"name":"api_key","typ":"text"}"#),
            feld(r#"{"name":"subject","typ":"text"}"#),
            feld(r#"{"name":"Flug","typ":"text"}"#),
            feld(r#"{"name":"a","typ":"text"},{"name":"a","typ":"text"}"#),
            feld(r#"{"name":"a","typ":"Text"}"#),
            feld(r#"{"name":"a","typ":""}"#),
            feld(r#"{"name":"a"}"#),
            feld(r#"{"name":"a","typ":"auswahl"}"#),
            feld(r#"{"name":"a","typ":"auswahl","optionen":[{"wert":""}]}"#),
            feld(r#"{"name":"a","typ":"auswahl","optionen":[{"wert":" x"}]}"#),
            feld(r#"{"name":"a","typ":"auswahl","optionen":[{"bezeichnung":{}}]}"#),
            feld(r#"{"name":"a","typ":"auswahl","optionen":[{"wert":"x"},{"wert":"x"}]}"#),
            feld(&(0..13).map(|i| format!(r#"{{"name":"f{i}","typ":"text"}}"#)).collect::<Vec<_>>().join(",")),
        ] {
            assert!(hilfe_formular_lesen(&boese).is_err(), "{boese}");
        }
        // Gegenprobe zur Grenze: zwoelf Felder gehen
        assert!(hilfe_formular_lesen(&feld(&(0..12).map(|i| format!(r#"{{"name":"f{i}","typ":"text"}}"#)).collect::<Vec<_>>().join(","))).is_ok());
    }

    #[test]
    fn json_koerper_ohne_zusatz_wie_bisher() {
        let alt = serde_json::json!({ "subject": "B", "body": "T", "diagnose": "D-0123456789ab" });
        assert_eq!(json_koerper("B", "T", Some("D-0123456789ab"), &BTreeMap::new()), alt);
        assert_eq!(json_koerper("B", "T", None, &BTreeMap::new()), serde_json::json!({ "subject": "B", "body": "T", "diagnose": null }));
        let mit = json_koerper("B", "T", None, &karte(&[("flugnummer", "GSG1"), ("simulator", "xp12")]));
        assert_eq!(mit["custom_fields"], serde_json::json!({ "flugnummer": "GSG1", "simulator": "xp12" }));
        assert_eq!(mit["subject"], "B");
    }

    #[test]
    fn ticket_url_host_fest_pfad_aus_dem_paket() {
        assert_eq!(ticket_url(paket_endpunkte::HELPDESK_PFAD_STANDARD), "https://german-sky-group.eu/api/helpdesk/tickets");
        assert_eq!(ticket_url("/api/helpdesk/v2/tickets"), "https://german-sky-group.eu/api/helpdesk/v2/tickets");
        // Der eingebaute Stand nennt den heutigen Pfad.
        assert_eq!(crate::laufzeit_paket::stand().endpunkte.helpdesk_pfad(), "/api/helpdesk/tickets");
    }

    #[test]
    fn multipart_enthaelt_zusatzfelder() {
        let zusatz = karte(&[("flugnummer", "GSG1234"), ("simulator", "Zeile 1\nZeile 2")]);
        let felder = multipart_felder("B", "T", Some("D-0123456789ab"), &zusatz);
        assert_eq!(
            felder.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["subject", "body", "diagnose", "custom_fields[flugnummer]", "custom_fields[simulator]"]
        );
        // ohne Kennung und Zusatz: genau die bisherigen Felder
        let alt = multipart_felder("B", "T", None, &BTreeMap::new());
        assert_eq!(alt, [("subject".to_string(), "B".to_string()), ("body".to_string(), "T".to_string())]);
        let felder: Vec<(&str, &str)> = felder.iter().map(|(n, w)| (n.as_str(), w.as_str())).collect();
        let k = multipart_koerper("G", &felder, &[Bild { bytes: PNG.to_vec(), mime: "image/png", endung: "png" }]);
        let text = String::from_utf8_lossy(&k);
        assert!(text.contains("--G\r\nContent-Disposition: form-data; name=\"custom_fields[flugnummer]\"\r\n\r\nGSG1234\r\n"), "{text}");
        assert!(text.contains("name=\"custom_fields[simulator]\"\r\n\r\nZeile 1\nZeile 2\r\n"), "{text}");
        // Gegenprobe: ohne Zusatz kein custom_fields-Teil
        let ohne = String::from_utf8_lossy(&multipart_koerper("G", &[("subject", "B")], &[])).into_owned();
        assert!(!ohne.contains("custom_fields"));
    }

    const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
    fn b64(b: &[u8]) -> HilfeBild {
        HilfeBild { daten: base64::engine::general_purpose::STANDARD.encode(b) }
    }

    #[test]
    fn bildtyp_an_den_bytes() {
        assert_eq!(bild_typ(PNG), Some(("image/png", "png")));
        assert_eq!(bild_typ(&[0xff, 0xd8, 0xff, 0xe0, 1]), Some(("image/jpeg", "jpg")));
        assert_eq!(bild_typ(b"RIFF\0\0\0\0WEBPVP8 "), Some(("image/webp", "webp")));
        // Gegenproben: anderer RIFF-Inhalt, Text, SVG, zu kurz
        assert_eq!(bild_typ(b"RIFF\0\0\0\0WAVEfmt "), None);
        assert_eq!(bild_typ(b"<?php echo 1; ?>"), None);
        assert_eq!(bild_typ(b"<svg xmlns='http://www.w3.org/2000/svg'/>"), None);
        assert_eq!(bild_typ(&[0x89, b'P']), None);
    }

    #[test]
    fn bilder_werden_geprueft() {
        // ohne Bilder: nichts
        assert_eq!(bilder_pruefen(&[]).unwrap(), vec![]);
        // fuenf gehen, sechs nicht
        assert_eq!(bilder_pruefen(&vec![b64(PNG); 5]).unwrap().len(), 5);
        assert_eq!(bilder_pruefen(&vec![b64(PNG); 6]).unwrap_err().code, "hilfe_bild_anzahl");
        // genau 5 MB gehen, ein Byte mehr nicht
        let mut gross = PNG.to_vec();
        gross.resize(BILD_MAX_BYTES, 0);
        assert_eq!(bilder_pruefen(&[b64(&gross)]).unwrap()[0].bytes.len(), BILD_MAX_BYTES);
        gross.push(0);
        assert_eq!(bilder_pruefen(&[b64(&gross)]).unwrap_err().code, "hilfe_bild_gross");
        // Typ an den Bytes, kein Base64 → abgelehnt
        assert_eq!(bilder_pruefen(&[b64(b"kein bild")]).unwrap_err().code, "hilfe_bild_typ");
        assert_eq!(bilder_pruefen(&[HilfeBild { daten: "%%%".into() }]).unwrap_err().code, "hilfe_bild_typ");
        // ein schlechtes unter guten verwirft alle (kein halbes Senden)
        assert!(bilder_pruefen(&[b64(PNG), b64(b"x")]).is_err());
        let ok = &bilder_pruefen(&[b64(PNG)]).unwrap()[0];
        assert_eq!((ok.mime, ok.endung, ok.bytes.as_slice()), ("image/png", "png", PNG));
    }

    /// Schreibt einen Koerper mit zwei Bildern (Bytes mit CRLF und `--` darin)
    /// nach `HILFE_MULTIPART_DATEI`, damit ein echter PHP-Parser ihn lesen
    /// kann (Gegenprobe zum handgebauten Format, 30.10.):
    /// `php -S` + `curl --data-binary`, erwartet `$_FILES['attachments']`
    /// und `$_POST['custom_fields']['flugnummer'] === 'GSG1234'`.
    #[test]
    #[ignore = "nur fuer die PHP-Gegenprobe, schreibt eine Datei"]
    fn multipart_fuer_php_gegenprobe() {
        let Ok(ziel) = std::env::var("HILFE_MULTIPART_DATEI") else { return };
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
        jpeg.extend_from_slice(b"\r\n--aeroacars-x\r\nContent-Disposition: kein Teil\r\n\r\n");
        jpeg.extend((0..=255u8).cycle().take(70_000));
        let bilder = vec![
            Bild { bytes: PNG.to_vec(), mime: "image/png", endung: "png" },
            Bild { bytes: jpeg, mime: "image/jpeg", endung: "jpg" },
        ];
        let grenze = format!("aeroacars-{}", uuid::Uuid::new_v4().simple());
        let k = multipart_koerper(
            &grenze,
            &[
                ("subject", "Betreff ä"),
                ("body", "Zeile 1\r\nZeile 2"),
                ("diagnose", "D-0123456789ab"),
                (&zusatz_feldname("flugnummer"), "GSG1234"),
            ],
            &bilder,
        );
        std::fs::write(&ziel, &k).unwrap();
        std::fs::write(format!("{ziel}.grenze"), &grenze).unwrap();
        for (i, b) in bilder.iter().enumerate() {
            std::fs::write(format!("{ziel}.bild{}", i + 1), &b.bytes).unwrap();
        }
    }

    #[test]
    fn multipart_wie_das_helpdesk_formular() {
        let bilder = vec![
            Bild { bytes: PNG.to_vec(), mime: "image/png", endung: "png" },
            Bild { bytes: vec![0xff, 0xd8, 0xff, 0x00, b'\r', b'\n'], mime: "image/jpeg", endung: "jpg" },
        ];
        let k = multipart_koerper("GRENZE", &[("subject", "Betreff ä"), ("diagnose", "D-0123456789ab")], &bilder);
        let text = String::from_utf8_lossy(&k);
        assert!(text.starts_with("--GRENZE\r\nContent-Disposition: form-data; name=\"subject\"\r\n\r\nBetreff ä\r\n"), "{text}");
        assert!(text.contains("name=\"diagnose\"\r\n\r\nD-0123456789ab\r\n"));
        assert!(text.contains("name=\"attachments[]\"; filename=\"bild-1.png\"\r\nContent-Type: image/png\r\n\r\n"));
        assert!(text.contains("name=\"attachments[]\"; filename=\"bild-2.jpg\"\r\nContent-Type: image/jpeg\r\n\r\n"));
        assert!(text.ends_with("\r\n--GRENZE--\r\n"));
        assert_eq!(text.matches("--GRENZE\r\n").count(), 4);
        // Bytes unveraendert zwischen Kopf und naechster Grenze
        let pos = k.windows(PNG.len()).position(|w| w == PNG).unwrap();
        assert_eq!(&k[pos + PNG.len()..pos + PNG.len() + 10], b"\r\n--GRENZE");
    }
}
