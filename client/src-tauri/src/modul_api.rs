//! Allgemeine Modul-Schnittstelle (Faehigkeit `modul-api-1`).
//!
//! Nur der Kern kennt den phpVMS-Schluessel des Piloten. Damit ein Fenster aus
//! dem Laufzeit-Paket ein phpVMS-Modul ansprechen kann, ohne dass jedes neue
//! Feld einen Client braucht, reicht dieser Befehl Methode, Pfad, Abfrage und
//! JSON durch. Der Host bleibt fest `ALLOWED_PHPVMS_HOST`, der Pfad muss auf
//! der Paketliste stehen (`tabellen/modul-api.json`) und die feste Pruefung in
//! `paket-endpunkte` bestehen, die phpVMS-Kernpfade und jede
//! Drehbuch-Bestellung ausschliesst.
//!
//! HTTP-Fehler sind kein `UiError`: Das Fenster unterscheidet 403, 404, 409,
//! 423, 429 und 503 selbst und bekommt darum `{status, daten}` roh zurueck.
//! `UiError` heisst: Die Anfrage ist gar nicht rausgegangen oder hat keine
//! Antwort gebracht.

use crate::UiError;
use serde::Serialize;
use std::collections::BTreeMap;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// Die Story-Pruefung im Modul darf bis 20 s dauern (`StoryPruefung::TIMEOUT_S`).
const TIMEOUT_S: u64 = 45;
const KOERPER_MAX_BYTES: usize = 256 * 1024;
const ABFRAGE_MAX_ANZAHL: usize = 20;
const ABFRAGE_NAME_MAX: usize = 64;
const ABFRAGE_WERT_MAX: usize = 2000;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct ModulAntwort {
    pub status: u16,
    /// JSON der Antwort; `null`, wenn der Server kein JSON geschickt hat.
    pub daten: serde_json::Value,
}

/// Nur diese Methoden, in Grossschreibung: eine feste Liste statt einer
/// beliebigen Zeichenkette aus der Oberflaeche.
fn methode_lesen(methode: &str) -> Result<reqwest::Method, UiError> {
    match methode {
        "GET" => Ok(reqwest::Method::GET),
        "POST" => Ok(reqwest::Method::POST),
        "PUT" => Ok(reqwest::Method::PUT),
        "PATCH" => Ok(reqwest::Method::PATCH),
        "DELETE" => Ok(reqwest::Method::DELETE),
        _ => Err(UiError::new("modul_api_methode", "method not allowed")),
    }
}

/// Pfad gegen die feste Pruefung und die Paketliste des geltenden Stands.
fn pfad_pruefen(pfad: &str, liste: &paket_endpunkte::ModulApi) -> Result<(), UiError> {
    paket_endpunkte::modul_api_pfad_pruefen(pfad).map_err(|x| UiError::new("modul_api_pfad", x))?;
    if !liste.erlaubt(pfad) {
        return Err(UiError::new("modul_api_pfad", "path not in package list"));
    }
    Ok(())
}

fn abfrage_pruefen(abfrage: &BTreeMap<String, String>) -> Result<(), UiError> {
    let ok = abfrage.len() <= ABFRAGE_MAX_ANZAHL
        && abfrage.iter().all(|(n, w)| {
            !n.is_empty() && n.len() <= ABFRAGE_NAME_MAX && w.chars().count() <= ABFRAGE_WERT_MAX && !n.chars().chain(w.chars()).any(char::is_control)
        });
    ok.then_some(()).ok_or_else(|| UiError::new("modul_api_abfrage", "query invalid"))
}

fn koerper_pruefen(koerper: &serde_json::Value) -> Result<(), UiError> {
    let laenge = serde_json::to_vec(koerper).map_err(|_| UiError::new("modul_api_koerper", "body invalid"))?.len();
    if laenge > KOERPER_MAX_BYTES {
        return Err(UiError::new("modul_api_koerper", "body too large"));
    }
    Ok(())
}

fn antwort_lesen(status: u16, text: &str) -> ModulAntwort {
    ModulAntwort { status, daten: serde_json::from_str(text).unwrap_or(serde_json::Value::Null) }
}

fn angemeldet(app: &AppHandle) -> Option<i64> {
    *app.state::<crate::AppState>().authenticated_pilot_id.lock().expect("authenticated_pilot_id lock")
}

/// Ruft einen freigegebenen Modul-Pfad auf phpVMS auf. Stufe „fachlich“.
#[tauri::command]
pub(crate) async fn modul_api(
    app: AppHandle,
    methode: String,
    pfad: String,
    abfrage: Option<BTreeMap<String, String>>,
    koerper: Option<serde_json::Value>,
) -> Result<ModulAntwort, UiError> {
    let methode = methode_lesen(&methode)?;
    pfad_pruefen(&pfad, &crate::laufzeit_paket::stand().modul_api)?;
    let abfrage = abfrage.unwrap_or_default();
    abfrage_pruefen(&abfrage)?;
    if let Some(k) = &koerper {
        koerper_pruefen(k)?;
    }
    // An den Piloten binden: Wechselt die Anmeldung waehrend des Abrufs, wird
    // die Antwort verworfen.
    let Some(pilot) = angemeldet(&app) else {
        return Err(UiError::new("modul_api_anmeldung", "no active session"));
    };
    let Some(schluessel) = crate::secrets_load_phpvms_key() else {
        return Err(UiError::new("modul_api_anmeldung", "no active session"));
    };
    let url = format!("https://{}{}", crate::ALLOWED_PHPVMS_HOST, pfad);
    let antwort = async {
        let c = reqwest::Client::builder()
            .timeout(Duration::from_secs(TIMEOUT_S))
            // Der Schluessel darf nie einer Weiterleitung folgen.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
            .build()
            .ok()?;
        let mut anfrage = c.request(methode, &url).header("X-API-Key", schluessel).header("Accept", "application/json");
        if !abfrage.is_empty() {
            anfrage = anfrage.query(&abfrage);
        }
        if let Some(k) = &koerper {
            anfrage = anfrage.json(k);
        }
        let r = anfrage.send().await.ok()?;
        let s = r.status().as_u16();
        Some((s, r.text().await.unwrap_or_default()))
    }
    .await;
    if angemeldet(&app) != Some(pilot) {
        return Err(UiError::new("modul_api_anmeldung", "session changed"));
    }
    match antwort {
        Some((s, text)) => Ok(antwort_lesen(s, &text)),
        None => Err(UiError::new("modul_api_netz", "not reachable")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn liste() -> paket_endpunkte::ModulApi {
        paket_endpunkte::modul_api_lesen(r#"{"format":1,"pfade":["/api/kabinenansagen/briefing"]}"#).unwrap()
    }

    #[test]
    fn nur_die_fuenf_methoden() {
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            assert_eq!(methode_lesen(m).unwrap().as_str(), m);
        }
        for boese in ["get", "HEAD", "OPTIONS", "TRACE", "CONNECT", "", "POST "] {
            assert_eq!(methode_lesen(boese).unwrap_err().code, "modul_api_methode", "{boese}");
        }
    }

    #[test]
    fn pfad_muss_auf_der_liste_stehen_und_die_sperre_bestehen() {
        let l = liste();
        assert!(pfad_pruefen("/api/kabinenansagen/briefing", &l).is_ok());
        assert!(pfad_pruefen("/api/kabinenansagen/briefing/pruefen", &l).is_ok());
        for boese in [
            "/api/helpdesk/tickets",
            "/api/kabinenansagen/briefings",
            "/api/kabinenansagen/briefing/drehbuch",
            "/api/kabinenansagen/briefing/../../user",
            "/api/kabinenansagen/briefing/",
            "/api/user",
        ] {
            assert_eq!(pfad_pruefen(boese, &l).unwrap_err().code, "modul_api_pfad", "{boese}");
        }
    }

    #[test]
    fn abfrage_gedeckelt() {
        let einer = |n: &str, w: &str| BTreeMap::from([(n.to_string(), w.to_string())]);
        assert!(abfrage_pruefen(&BTreeMap::new()).is_ok());
        assert!(abfrage_pruefen(&einer("flight_id", "abc&d=e")).is_ok(), "kodieren macht reqwest");
        assert!(abfrage_pruefen(&einer(&"n".repeat(ABFRAGE_NAME_MAX), &"w".repeat(ABFRAGE_WERT_MAX))).is_ok(), "genau an der Grenze");
        assert!(abfrage_pruefen(&einer("", "x")).is_err());
        assert!(abfrage_pruefen(&einer(&"n".repeat(ABFRAGE_NAME_MAX + 1), "x")).is_err());
        assert!(abfrage_pruefen(&einer("a", &"w".repeat(ABFRAGE_WERT_MAX + 1))).is_err());
        assert!(abfrage_pruefen(&einer("a", "x\ny")).is_err());
        let viele = |n: usize| (0..n).map(|i| (format!("f{i}"), "x".to_string())).collect::<BTreeMap<_, _>>();
        assert!(abfrage_pruefen(&viele(ABFRAGE_MAX_ANZAHL)).is_ok());
        assert!(abfrage_pruefen(&viele(ABFRAGE_MAX_ANZAHL + 1)).is_err());
    }

    #[test]
    fn koerper_gedeckelt() {
        // Text in Anfuehrungszeichen: zwei Bytes kommen zur Laenge dazu.
        assert!(koerper_pruefen(&serde_json::json!("a".repeat(KOERPER_MAX_BYTES - 2))).is_ok());
        assert_eq!(koerper_pruefen(&serde_json::json!("a".repeat(KOERPER_MAX_BYTES - 1))).unwrap_err().code, "modul_api_koerper");
    }

    #[test]
    fn antwort_bleibt_roh() {
        assert_eq!(antwort_lesen(423, r#"{"fehler":"gesperrt"}"#), ModulAntwort { status: 423, daten: serde_json::json!({"fehler":"gesperrt"}) });
        assert_eq!(antwort_lesen(302, "<html>"), ModulAntwort { status: 302, daten: serde_json::Value::Null });
        assert_eq!(antwort_lesen(204, ""), ModulAntwort { status: 204, daten: serde_json::Value::Null });
    }
}
