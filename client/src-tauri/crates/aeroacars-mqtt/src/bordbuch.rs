//! Bordbuch-Sicherung auf dem Live-Server (26.09.2026).
//!
//! Einträge je Flug und die Einstellungen des Piloten liegen im Client
//! lokal und zusätzlich auf dem Server — nach einer Neuinstallation holt
//! AeroACARS beides zurück, und der Admin sieht auf live.kant.ovh pro Flug
//! dasselbe Bordbuch wie der Pilot.
//!
//! Wie bei der Landungs-Sicherung ([`crate::backup`]) nimmt der Server die
//! Piloten-Kennung ausschliesslich aus dem geprüften Token. Dieses Modul
//! kennt die Bordbuch-Typen nicht (die leben in der App) und reicht JSON
//! durch.

use serde::Deserialize;

use crate::navdata::{build_client, NavdataError, DEFAULT_NAVDATA_BASE};

fn url(base: Option<&str>, pfad: &str) -> String {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    format!("{}/api/bordbuch{pfad}", base.trim_end_matches('/'))
}

async fn pruefen(response: reqwest::Response) -> Result<reqwest::Response, NavdataError> {
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(NavdataError::Unauthorized);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(NavdataError::Server {
            status: status.as_u16(),
            body: body.chars().take(300).collect(),
        });
    }
    Ok(response)
}

/// Einen Eintrag sichern (anlegen oder ersetzen). Der Server behält die
/// Fassung mit dem neueren `updated_at`.
pub async fn eintrag_sichern(
    base: Option<&str>,
    token: &str,
    eintrag: &serde_json::Value,
) -> Result<(), NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = client
        .put(url(base, "/eintrag"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "eintrag": eintrag }))
        .send()
        .await?;
    pruefen(r).await.map(|_| ())
}

#[derive(Debug, Clone, Deserialize)]
struct EintraegeAntwort {
    eintraege: Vec<serde_json::Value>,
}

/// Alle eigenen Einträge vom Server.
pub async fn eintraege_holen(
    base: Option<&str>,
    token: &str,
) -> Result<Vec<serde_json::Value>, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = client
        .get(url(base, "/eintraege"))
        .bearer_auth(token)
        .send()
        .await?;
    let r = pruefen(r).await?;
    r.json::<EintraegeAntwort>()
        .await
        .map(|a| a.eintraege)
        .map_err(|e| NavdataError::BadResponse(e.to_string()))
}

#[derive(Debug, Clone, Deserialize)]
struct EinstellungenAntwort {
    einstellungen: Option<serde_json::Value>,
}

/// Die eigenen Einstellungen vom Server. `Ok(None)`: es gibt noch keine.
pub async fn einstellungen_holen(
    base: Option<&str>,
    token: &str,
) -> Result<Option<serde_json::Value>, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = client
        .get(url(base, "/einstellungen"))
        .bearer_auth(token)
        .send()
        .await?;
    if r.status().as_u16() == 404 {
        return Ok(None);
    }
    let r = pruefen(r).await?;
    r.json::<EinstellungenAntwort>()
        .await
        .map(|a| a.einstellungen)
        .map_err(|e| NavdataError::BadResponse(e.to_string()))
}

pub async fn einstellungen_sichern(
    base: Option<&str>,
    token: &str,
    einstellungen: &serde_json::Value,
) -> Result<(), NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = client
        .put(url(base, "/einstellungen"))
        .bearer_auth(token)
        .json(&serde_json::json!({ "einstellungen": einstellungen }))
        .send()
        .await?;
    pruefen(r).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(
            url(Some("https://live.example/"), "/eintrag"),
            "https://live.example/api/bordbuch/eintrag"
        );
        assert!(url(None, "/eintraege").ends_with("/api/bordbuch/eintraege"));
    }
}
