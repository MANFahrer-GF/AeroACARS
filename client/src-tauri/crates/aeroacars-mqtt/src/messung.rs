//! „Flugzeug vermessen" (28.09.2026): Messergebnis an live.kant.ovh senden
//! und die L:-Namen aus den Aircraft-Scans eines Flugzeugs holen.
//!
//! Gleicher Zugang wie das Bordbuch ([`crate::bordbuch`]): Bearer-Token des
//! Clients, der Server nimmt die Piloten-Kennung ausschließlich daraus.

use serde::Deserialize;

use crate::navdata::{build_client, NavdataError, DEFAULT_NAVDATA_BASE};

fn url(base: Option<&str>, pfad: &str) -> String {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    format!("{}/api/ascan{pfad}", base.trim_end_matches('/'))
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

#[derive(Debug, Deserialize)]
struct Gesendet {
    id: String,
}

/// Messung einsenden. Liefert die ID der Einreichung.
pub async fn senden(
    base: Option<&str>,
    token: &str,
    messung: &serde_json::Value,
) -> Result<String, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = client
        .post(url(base, "/messungen"))
        .bearer_auth(token)
        .json(messung)
        .send()
        .await?;
    let r = pruefen(r).await?;
    r.json::<Gesendet>()
        .await
        .map(|g| g.id)
        .map_err(|e| NavdataError::BadResponse(e.to_string()))
}

#[derive(Debug, Deserialize)]
struct Namen {
    namen: Vec<String>,
}

/// L:-Namen aus den Aircraft-Scans, die zu diesem Flugzeug passen.
pub async fn lvar_namen(
    base: Option<&str>,
    token: &str,
    icao: &str,
    titel: &str,
) -> Result<Vec<String>, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = client
        .get(url(base, "/lvar-namen"))
        .query(&[("icao", icao), ("titel", titel)])
        .bearer_auth(token)
        .send()
        .await?;
    let r = pruefen(r).await?;
    r.json::<Namen>()
        .await
        .map(|n| n.namen)
        .map_err(|e| NavdataError::BadResponse(e.to_string()))
}
