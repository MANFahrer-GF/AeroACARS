//! „Flugzeug vermessen" (28.09.2026) — alle Werte des geladenen Flugzeugs
//! live mitlesen.
//!
//! Über die Web-API v2 (X-Plane 12.1.4+) werden alle Zahlen-Datarefs per
//! WebSocket abonniert; X-Plane schickt erst alle Werte und dann laufend die
//! geänderten. Ein Schnappschuss ist damit nur eine Kopie — bei ~8000
//! Werten Sekundenbruchteile statt 40 s mit Einzelabfragen (gemessen an der
//! X-Plane-12-Demo, 27.09.2026).
//!
//! ⚠ Abo-Nachrichten höchstens [`PAKET`] Datarefs: auf eine Nachricht mit
//! allen ~7800 IDs (≈170 KB) antwortete X-Plane 12.4.3 gar nicht.
//!
//! Nur lesen — es wird nichts in X-Plane geschrieben.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;
use tungstenite::Message;

use crate::web_api::{AircraftInfo, DrefIdCache, WebApiClient};

/// Datarefs je Abo-Nachricht.
pub const PAKET: usize = 500;
/// Array-Datarefs werden bis zu dieser Länge elementweise geführt.
pub const MAX_ARRAY: usize = 48;
const HOST: &str = "127.0.0.1:8086";

#[derive(Debug, Deserialize)]
struct Liste {
    data: Vec<Eintrag>,
}

#[derive(Debug, Deserialize, Clone)]
struct Eintrag {
    id: i64,
    name: String,
    value_type: String,
}

fn ist_zahl(typ: &str) -> bool {
    matches!(
        typ,
        "int" | "float" | "double" | "int_array" | "float_array"
    )
}

/// Aktueller Wert eines Datarefs.
#[derive(Debug, Clone, PartialEq)]
enum Wert {
    Zahl(f64),
    Liste(Vec<f64>),
}

fn wert_aus_json(v: &serde_json::Value) -> Option<Wert> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(Wert::Zahl),
        serde_json::Value::Array(a) => Some(Wert::Liste(
            a.iter()
                .take(MAX_ARRAY)
                .map(|x| x.as_f64().unwrap_or(f64::NAN))
                .collect(),
        )),
        _ => None,
    }
}

/// Eine `dataref_update_values`-Nachricht in den Spiegel übernehmen.
/// Liefert die Zahl der übernommenen Werte.
fn nachricht_uebernehmen(text: &str, werte: &mut HashMap<i64, Wert>) -> usize {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return 0;
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("dataref_update_values") {
        return 0;
    }
    let Some(daten) = v.get("data").and_then(|d| d.as_object()) else {
        return 0;
    };
    let mut n = 0;
    for (id, w) in daten {
        if let (Ok(id), Some(w)) = (id.parse::<i64>(), wert_aus_json(w)) {
            werte.insert(id, w);
            n += 1;
        }
    }
    n
}

/// Werte in einen flachen Schnappschuss (Name → Zahl), Arrays als `name[i]`.
fn flach(werte: &HashMap<i64, Wert>, namen: &HashMap<i64, String>) -> HashMap<String, f64> {
    let mut aus = HashMap::with_capacity(werte.len());
    for (id, w) in werte {
        let Some(name) = namen.get(id) else { continue };
        match w {
            Wert::Zahl(x) => {
                aus.insert(name.clone(), *x);
            }
            Wert::Liste(l) => {
                for (i, x) in l.iter().enumerate() {
                    if x.is_finite() {
                        aus.insert(format!("{name}[{i}]"), *x);
                    }
                }
            }
        }
    }
    aus
}

/// Die Abo-Nachrichten, in Paketen zu [`PAKET`].
fn abo_nachrichten(ids: &[i64]) -> Vec<String> {
    ids.chunks(PAKET)
        .enumerate()
        .map(|(i, c)| {
            serde_json::json!({
                "req_id": i + 1,
                "type": "dataref_subscribe_values",
                "params": { "datarefs": c.iter().map(|id| serde_json::json!({ "id": id })).collect::<Vec<_>>() },
            })
            .to_string()
        })
        .collect()
}

pub struct Spiegel {
    werte: Arc<Mutex<HashMap<i64, Wert>>>,
    namen: HashMap<i64, String>,
    stop: Arc<AtomicBool>,
    faden: Option<JoinHandle<()>>,
    pub flugzeug: AircraftInfo,
    pub abonniert: usize,
}

impl Spiegel {
    /// Liste holen, verbinden, alles abonnieren, Lesefaden starten.
    pub fn starten() -> Result<Spiegel, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(3))
            .timeout_read(Duration::from_secs(30))
            .build();
        let liste: Liste = agent
            .get(&format!("http://{HOST}/api/v2/datarefs"))
            .call()
            .map_err(|e| format!("X-Plane-Web-API nicht erreichbar: {e}"))?
            .into_json()
            .map_err(|e| format!("Dataref-Liste nicht lesbar: {e}"))?;
        let zahlen: Vec<Eintrag> = liste
            .data
            .into_iter()
            .filter(|e| ist_zahl(&e.value_type))
            .collect();
        if zahlen.is_empty() {
            return Err("X-Plane meldet keine Werte — ist ein Flugzeug geladen?".into());
        }
        let namen: HashMap<i64, String> = zahlen.iter().map(|e| (e.id, e.name.clone())).collect();
        let ids: Vec<i64> = zahlen.iter().map(|e| e.id).collect();

        let flugzeug = WebApiClient::new()
            .fetch_aircraft_info(&mut DrefIdCache::default())
            .unwrap_or_default();

        let strom = TcpStream::connect(HOST).map_err(|e| format!("WebSocket: {e}"))?;
        strom
            .set_read_timeout(Some(Duration::from_millis(400)))
            .map_err(|e| e.to_string())?;
        let (mut ws, _) = tungstenite::client(format!("ws://{HOST}/api/v2"), strom)
            .map_err(|e| format!("WebSocket-Handshake: {e}"))?;
        for n in abo_nachrichten(&ids) {
            ws.send(Message::text(n))
                .map_err(|e| format!("Abo senden: {e}"))?;
        }

        let werte = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (w2, s2) = (Arc::clone(&werte), Arc::clone(&stop));
        let faden = std::thread::Builder::new()
            .name("xplane-vermessung".into())
            .spawn(move || {
                while !s2.load(Ordering::SeqCst) {
                    match ws.read() {
                        Ok(Message::Text(t)) => {
                            nachricht_uebernehmen(t.as_str(), &mut w2.lock());
                        }
                        Ok(Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(e))
                            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                        Err(e) => {
                            tracing::info!(error = %e, "X-Plane-Vermessung: Verbindung beendet");
                            break;
                        }
                    }
                }
                let _ = ws.close(None);
            })
            .map_err(|e| e.to_string())?;

        Ok(Spiegel {
            werte,
            namen,
            stop,
            faden: Some(faden),
            flugzeug,
            abonniert: ids.len(),
        })
    }

    /// Wie viele Datarefs schon einen Wert geliefert haben.
    pub fn verbunden(&self) -> usize {
        self.werte.lock().len()
    }

    /// Alle aktuellen Werte, Arrays elementweise.
    pub fn schnappschuss(&self) -> HashMap<String, f64> {
        flach(&self.werte.lock(), &self.namen)
    }
}

impl Drop for Spiegel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(f) = self.faden.take() {
            let _ = f.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abo_in_paketen_zu_500() {
        let ids: Vec<i64> = (1..=1203).collect();
        let n = abo_nachrichten(&ids);
        assert_eq!(n.len(), 3);
        let erste: serde_json::Value = serde_json::from_str(&n[0]).unwrap();
        assert_eq!(erste["type"], "dataref_subscribe_values");
        assert_eq!(erste["params"]["datarefs"].as_array().unwrap().len(), 500);
        let letzte: serde_json::Value = serde_json::from_str(&n[2]).unwrap();
        assert_eq!(letzte["params"]["datarefs"].as_array().unwrap().len(), 203);
    }

    /// Nachrichten so, wie X-Plane 12.4.3 sie am 27.09.2026 schickte.
    #[test]
    fn updates_werden_gespiegelt() {
        let mut w = HashMap::new();
        assert_eq!(
            nachricht_uebernehmen(r#"{"req_id":1,"success":true,"type":"result"}"#, &mut w),
            0
        );
        let n = nachricht_uebernehmen(
            r#"{"data":{"500556762880":0,"12":[1.0,0.5,2],"13":"QTMzMw=="},"type":"dataref_update_values"}"#,
            &mut w,
        );
        assert_eq!(n, 2, "Text-Werte (base64) werden nicht gespiegelt");
        nachricht_uebernehmen(
            r#"{"data":{"500556762880":1},"type":"dataref_update_values"}"#,
            &mut w,
        );
        let namen: HashMap<i64, String> = [
            (
                500556762880,
                "sim/cockpit2/switches/strobe_lights_on".to_string(),
            ),
            (12, "laminar/a333/arr".to_string()),
        ]
        .into_iter()
        .collect();
        let s = flach(&w, &namen);
        assert_eq!(s.get("sim/cockpit2/switches/strobe_lights_on"), Some(&1.0));
        assert_eq!(s.get("laminar/a333/arr[1]"), Some(&0.5));
        assert_eq!(s.get("laminar/a333/arr[2]"), Some(&2.0));
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn nur_zahlen_typen() {
        assert!(ist_zahl("int") && ist_zahl("float_array") && ist_zahl("double"));
        assert!(!ist_zahl("data"));
    }

    #[test]
    fn lange_arrays_werden_gekappt() {
        let mut w = HashMap::new();
        let arr: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        nachricht_uebernehmen(
            &format!(
                r#"{{"data":{{"7":[{}]}},"type":"dataref_update_values"}}"#,
                arr.join(",")
            ),
            &mut w,
        );
        let namen: HashMap<i64, String> = [(7, "x".to_string())].into_iter().collect();
        assert_eq!(flach(&w, &namen).len(), MAX_ARRAY);
    }
}
