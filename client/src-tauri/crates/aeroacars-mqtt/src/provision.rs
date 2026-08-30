//! Auto-Provisioning gegen live.kant.ovh — gibt phpVMS-API-Key durch,
//! kriegt MQTT-Credentials zurück. Idempotent serverseitig (DB-cached
//! per phpVMS-Pilot-ID). Re-Install des Clients = identische
//! Credentials, kein Race.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Default-URL — kann via Override-Parameter überschrieben werden für
/// Test-VPS / Dev-Setups.
pub const DEFAULT_PROVISION_URL: &str = "https://live.kant.ovh/api/provision";

#[derive(Serialize)]
struct ProvisionRequest<'a> {
    api_key: &'a str,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ProvisionResponse {
    pub broker_url: String,
    pub username: String,
    pub password: String,
    pub va_prefix: String,
    pub pilot_id: String,
    pub display_name: Option<String>,
    pub topic_root: String,
    pub newly_created: bool,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: String,
}

/// Ruft den Provision-Endpoint mit dem phpVMS-API-Key auf.
/// Kehrt mit MQTT-Credentials zurück oder Fehler (z.B. 401 wenn Key invalid).
pub async fn provision(api_key: &str, endpoint: Option<&str>) -> Result<ProvisionResponse> {
    let url = endpoint.unwrap_or(DEFAULT_PROVISION_URL);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let res = client
        .post(url)
        .json(&ProvisionRequest { api_key })
        .send()
        .await
        .context("provision request failed")?;

    let status = res.status();
    if !status.is_success() {
        let err = res
            .json::<ErrorBody>()
            .await
            .ok()
            .map(|b| b.error)
            .unwrap_or_else(|| status.to_string());
        anyhow::bail!("provision rejected: {} ({})", err, status);
    }

    let body: ProvisionResponse = res.json().await.context("parsing provision response")?;
    Ok(body)
}

/// Konvertiert ProvisionResponse in MqttConfig (siehe lib.rs).
impl From<ProvisionResponse> for crate::MqttConfig {
    fn from(p: ProvisionResponse) -> Self {
        crate::MqttConfig {
            broker_url: p.broker_url,
            username: p.username,
            password: p.password,
            va_prefix: p.va_prefix,
            pilot_id: p.pilot_id,
        }
    }
}
