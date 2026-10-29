//! Flugakte (Daten der Flugkarte) eines eigenen PIREPs vom Live-Server holen.
//!
//! Der Server rechnet die Akte aus dem hochgeladenen Flugprotokoll — dieselbe
//! Rechnung, die der Admin in der Webapp sieht (Thomas 28.10.2026: „eine
//! Sicht“, „Server mit Prüfung“). Der Client rechnet nichts selbst.
//!
//! Anmeldung wie beim Flugprotokoll-Upload: HTTP Basic mit dem
//! MQTT-Zugangspaar. Der Server liefert nur eigene Flüge; fremde und
//! unbekannte kommen als 404 — dann gibt es (noch) keine Akte.
//!
//! Endpunkt aus der Provision-URL abgeleitet (`/api/provision` →
//! `/api/flight-logs/flugakte/<pirep>`), damit Test-Server mitziehen.

use anyhow::{Context, Result};
use base64::Engine;
use std::time::Duration;

use crate::provision::default_provision_url;

fn default_basis_url() -> String {
    default_provision_url().replace("/api/provision", "/api/flight-logs/flugakte")
}

/// Nur Kennungen, die der Server annimmt ([A-Za-z0-9_-]{1,128}) — alles
/// andere landete sonst ungeprüft im Pfad der Anfrage.
pub fn gueltige_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Ergebnis eines Abrufs.
#[derive(Debug, PartialEq)]
pub enum Abruf {
    /// Die Akte als JSON-Text.
    Akte(String),
    /// Kein Flugprotokoll auf dem Server (nicht hochgeladen, fremder Flug, vor Mai 2026).
    Fehlt,
    /// Der Server lehnt den Zugang ab (401/403, z. B. abgelaufenes Zugangspaar) —
    /// der Pilot soll „neu anmelden“ lesen, nicht einen rohen HTTP-Fehler (Codex 28.10.).
    KeinZugang,
}

/// Holt die Flugakte eines eigenen PIREPs.
pub async fn flugakte_abrufen(
    pirep_id: &str,
    username: &str,
    password: &str,
    endpoint: Option<&str>,
) -> Result<Abruf> {
    if !gueltige_id(pirep_id) {
        anyhow::bail!("flugakte: ungültige PIREP-Kennung");
    }
    let basis = endpoint.map(String::from).unwrap_or_else(default_basis_url);
    let url = format!("{}/{}", basis.trim_end_matches('/'), pirep_id);
    let auth_b64 = base64::engine::general_purpose::STANDARD
        .encode(format!("{username}:{password}").as_bytes());
    // Lange Flüge rechnet der Server beim ersten Abruf bis ~2 s, die Akte
    // ist dann einige MB groß — großzügige Frist.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let res = client
        .get(&url)
        .header("Authorization", format!("Basic {auth_b64}"))
        .send()
        .await
        .context("flugakte GET fehlgeschlagen")?;
    let status = res.status();
    match status.as_u16() {
        404 => return Ok(Abruf::Fehlt),
        401 | 403 => return Ok(Abruf::KeinZugang),
        _ => {}
    }
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("flugakte abgelehnt: HTTP {} — {}", status.as_u16(), body);
    }
    Ok(Abruf::Akte(res.text().await.context("flugakte: Antwort unlesbar")?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kennung_wird_geprueft_bevor_sie_in_den_pfad_kommt() {
        assert!(gueltige_id("278xq2bgoYV5qZjL"));
        assert!(gueltige_id("ok_-1"));
        assert!(!gueltige_id(""));
        assert!(!gueltige_id("../7/FREMDER"));
        assert!(!gueltige_id("a/b"));
        assert!(!gueltige_id(&"x".repeat(129)));
    }

    /// Kleiner Server: antwortet einmal mit `status` und `body`, gibt die Anfrage zurück.
    async fn server(status: &'static str, body: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/api/flight-logs/flugakte", l.local_addr().unwrap());
        let h = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = vec![0u8; 4096];
            let n = s.read(&mut buf).await.unwrap();
            let antwort = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            s.write_all(antwort.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&buf[..n]).to_string()
        });
        (url, h)
    }

    #[tokio::test]
    async fn status_wird_richtig_zugeordnet_und_die_anmeldung_mitgeschickt() {
        let (url, h) = server("200 OK", r#"{"version":3}"#).await;
        assert_eq!(flugakte_abrufen("ABC_1", "u", "p", Some(&url)).await.unwrap(), Abruf::Akte(r#"{"version":3}"#.into()));
        let anfrage = h.await.unwrap();
        assert!(anfrage.starts_with("GET /api/flight-logs/flugakte/ABC_1 "), "{anfrage}");
        assert!(anfrage.contains("authorization: Basic dTpw") || anfrage.contains("Authorization: Basic dTpw"), "{anfrage}");

        let (url, _) = server("404 Not Found", "").await;
        assert_eq!(flugakte_abrufen("ABC_1", "u", "p", Some(&url)).await.unwrap(), Abruf::Fehlt);
        // Codex 28.10.: abgelaufenes Zugangspaar soll „neu anmelden“ heißen, nicht „HTTP 401“
        for st in ["401 Unauthorized", "403 Forbidden"] {
            let (url, _) = server(st, "nein").await;
            assert_eq!(flugakte_abrufen("ABC_1", "u", "p", Some(&url)).await.unwrap(), Abruf::KeinZugang, "{st}");
        }
        let (url, _) = server("500 Internal Server Error", "kaputt").await;
        let e = flugakte_abrufen("ABC_1", "u", "p", Some(&url)).await.unwrap_err().to_string();
        assert!(e.contains("HTTP 500"), "{e}");
    }

    #[test]
    fn endpunkt_haengt_am_selben_server_wie_provision() {
        assert!(default_basis_url().ends_with("/api/flight-logs/flugakte"));
        assert!(!default_basis_url().contains("/api/provision"));
    }
}
