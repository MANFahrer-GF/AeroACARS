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

use crate::provision::DEFAULT_PROVISION_URL;

fn default_basis_url() -> String {
    DEFAULT_PROVISION_URL.replace("/api/provision", "/api/flight-logs/flugakte")
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

/// Holt die Flugakte als JSON-Text. `Ok(None)`: Der Server hat zu diesem
/// PIREP kein Flugprotokoll (nicht hochgeladen, fremder Flug, vor Mai 2026).
pub async fn flugakte_abrufen(
    pirep_id: &str,
    username: &str,
    password: &str,
    endpoint: Option<&str>,
) -> Result<Option<String>> {
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
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("flugakte abgelehnt: HTTP {} — {}", status.as_u16(), body);
    }
    Ok(Some(res.text().await.context("flugakte: Antwort unlesbar")?))
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

    #[test]
    fn endpunkt_haengt_am_selben_server_wie_provision() {
        assert!(default_basis_url().ends_with("/api/flight-logs/flugakte"));
        assert!(!default_basis_url().contains("/api/provision"));
    }
}
