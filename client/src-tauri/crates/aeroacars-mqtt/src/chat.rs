//! Pilotenchat — die beiden Abrufe, die nicht über MQTT laufen.
//!
//! Der laufende Betrieb geht über den Broker: senden auf dem eigenen Thema,
//! empfangen auf `chat_in`. Zwei Dinge braucht man aber schon **vor** der
//! ersten Nachricht, und dafür ist HTTP das richtige Werkzeug:
//!
//!   * **Verlauf** — was lief, bevor ich mich verbunden habe. Zwölf Stunden
//!     Gedächtnis, nicht mehr; der Server räumt Älteres weg.
//!   * **Teilnehmer** — wer ist überhaupt gerade in der Luft, mit Name,
//!     Rufzeichen und Strecke. Rufzeichen allein nutzt im Flug nichts.
//!
//! Auth wie bei den Navdaten: `Authorization: Bearer <Pilot-Token>` aus der
//! Bereitstellung. Same-Host, deshalb dieselbe Basis-URL.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::navdata::DEFAULT_NAVDATA_BASE;

/// Eine Zeile aus dem Verlauf. Deckungsgleich mit dem, was der Server auf
/// `chat_in` zustellt — dieselbe Form, damit die Oberfläche nicht zwei
/// Varianten kennen muss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatVerlaufNachricht {
    pub id: i64,
    pub va_prefix: String,
    pub von_pilot_id: String,
    #[serde(default)]
    pub an_pilot_id: Option<String>,
    pub ts: i64,
    pub text: String,
    #[serde(default)]
    pub callsign: Option<String>,
    #[serde(default)]
    pub anzeigename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatTeilnehmer {
    pub pilot_id: String,
    #[serde(default)]
    pub callsign: Option<String>,
    #[serde(default)]
    pub dep: Option<String>,
    #[serde(default)]
    pub arr: Option<String>,
    #[serde(default)]
    pub anzeigename: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VerlaufAntwort {
    nachrichten: Vec<ChatVerlaufNachricht>,
    #[serde(default)]
    fenster_stunden: f64,
}

#[derive(Debug, Deserialize)]
struct TeilnehmerAntwort {
    teilnehmer: Vec<ChatTeilnehmer>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChatFehler {
    #[error("Netzwerk: {0}")]
    Netz(String),
    #[error("nicht angemeldet")]
    NichtAngemeldet,
    #[error("Server {status}: {body}")]
    Server { status: u16, body: String },
}

impl From<reqwest::Error> for ChatFehler {
    fn from(e: reqwest::Error) -> Self {
        ChatFehler::Netz(e.to_string())
    }
}

fn client() -> Result<reqwest::Client, ChatFehler> {
    reqwest::Client::builder()
        // Kurz: das hier blockiert den Einstieg in den Chat. Lieber leer
        // starten und über MQTT nachfüllen, als den Piloten warten lassen.
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| ChatFehler::Netz(e.to_string()))
}

async fn hole<T: for<'de> Deserialize<'de>>(
    pfad: &str,
    base: Option<&str>,
    token: Option<&str>,
) -> Result<T, ChatFehler> {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    let url = format!("{}{}", base.trim_end_matches('/'), pfad);
    let mut req = client()?.get(&url);
    if let Some(t) = token {
        req = req.header(reqwest::header::AUTHORIZATION, format!("Bearer {t}"));
    }
    let antwort = req.send().await?;
    let status = antwort.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(ChatFehler::NichtAngemeldet);
    }
    if !status.is_success() {
        let body = antwort.text().await.unwrap_or_default();
        return Err(ChatFehler::Server {
            status: status.as_u16(),
            body: body.chars().take(300).collect(),
        });
    }
    antwort.json::<T>().await.map_err(ChatFehler::from)
}

/// Die letzten Zurufe, die diesen Piloten etwas angehen. Fremde
/// Direktnachrichten filtert der Server bereits weg — die kommen hier gar
/// nicht erst an.
pub async fn verlauf(
    base: Option<&str>,
    token: Option<&str>,
) -> Result<(Vec<ChatVerlaufNachricht>, f64), ChatFehler> {
    let a: VerlaufAntwort = hole("/api/chat/verlauf", base, token).await?;
    Ok((a.nachrichten, a.fenster_stunden))
}

/// Wer gerade fliegt.
pub async fn teilnehmer(
    base: Option<&str>,
    token: Option<&str>,
) -> Result<Vec<ChatTeilnehmer>, ChatFehler> {
    let a: TeilnehmerAntwort = hole("/api/chat/teilnehmer", base, token).await?;
    Ok(a.teilnehmer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verlaufszeile_liest_sich_aus_dem_serverformat() {
        // Genau die Form, die der Recorder auf chat_in zustellt und im
        // Verlauf zurueckgibt. Faellt der Server-Typ auseinander, faellt
        // dieser Test — deshalb steht hier echtes JSON, kein Nachbau.
        let roh = r#"{
            "id": 42, "va_prefix": "gsg", "von_pilot_id": "2",
            "an_pilot_id": null, "ts": 1786500000000,
            "text": "Wer ist noch nach Catania unterwegs?",
            "callsign": "EZY 5077", "anzeigename": "Michel D"
        }"#;
        let n: ChatVerlaufNachricht = serde_json::from_str(roh).expect("lesbar");
        assert_eq!(n.id, 42);
        assert_eq!(n.callsign.as_deref(), Some("EZY 5077"));
        assert!(n.an_pilot_id.is_none(), "an alle");
    }

    #[test]
    fn direktnachricht_traegt_den_empfaenger() {
        let roh = r#"{
            "id": 43, "va_prefix": "gsg", "von_pilot_id": "2",
            "an_pilot_id": "1", "ts": 1786500001000,
            "text": "Nur fuer dich", "callsign": "EZY 5077", "anzeigename": "Michel D"
        }"#;
        let n: ChatVerlaufNachricht = serde_json::from_str(roh).expect("lesbar");
        assert_eq!(n.an_pilot_id.as_deref(), Some("1"));
    }

    #[test]
    fn fehlende_felder_kippen_nichts() {
        // Ein aelterer Server koennte callsign/anzeigename nicht mitliefern.
        // Dann soll der Zuruf trotzdem ankommen, statt die ganze Liste zu
        // verlieren.
        let roh = r#"{"id":1,"va_prefix":"gsg","von_pilot_id":"2","ts":1,"text":"hallo"}"#;
        let n: ChatVerlaufNachricht = serde_json::from_str(roh).expect("lesbar");
        assert!(n.callsign.is_none());
        assert_eq!(n.text, "hallo");
    }

    #[test]
    fn teilnehmer_traegt_strecke_und_name() {
        let roh = r#"{"teilnehmer":[
            {"pilot_id":"1","callsign":"ITY 4532","dep":"EDDB","arr":"LICC","anzeigename":"Thomas K"}
        ]}"#;
        let a: TeilnehmerAntwort = serde_json::from_str(roh).expect("lesbar");
        assert_eq!(a.teilnehmer.len(), 1);
        assert_eq!(a.teilnehmer[0].arr.as_deref(), Some("LICC"));
        assert_eq!(a.teilnehmer[0].anzeigename.as_deref(), Some("Thomas K"));
    }
}
