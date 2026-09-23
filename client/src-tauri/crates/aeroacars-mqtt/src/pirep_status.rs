//! Prüfstatus eingereichter PIREPs beim Live-Server abfragen.
//!
//! Anlass: DLH 880 (Sven M, 15.09.2026) hing im Integritäts-Gate, weil der
//! Server beim Einreichen keine Landung kannte. Der Pilot sah davon im
//! Client nichts — weder das Festhalten noch die spätere Freigabe. Dieser
//! Abruf liefert den Stand, den der Server (und über ihn das phpVMS-Gate)
//! zu jedem eigenen PIREP hat; der Landungs-Tab zeigt ihn an.
//!
//! Anmeldung wie beim Flugprotokoll-Upload: HTTP Basic mit dem
//! MQTT-Zugangspaar. Der Server liefert nur eigene PIREPs mit Inhalt,
//! fremde und unbekannte kommen als `known: false`.
//!
//! Endpunkt aus der Provision-URL abgeleitet (`/api/provision` →
//! `/api/flight-logs/pirep-status`), damit Test-Server mitziehen.

use anyhow::{Context, Result};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::provision::DEFAULT_PROVISION_URL;

/// Höchstzahl IDs je Anfrage — muss zum Server passen (`PIREP_STATUS_MAX`).
pub const MAX_IDS_JE_ANFRAGE: usize = 50;

fn default_status_url() -> String {
    DEFAULT_PROVISION_URL.replace("/api/provision", "/api/flight-logs/pirep-status")
}

/// Stand eines PIREPs aus Sicht des Live-Servers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PirepPruefstatus {
    pub pirep_id: String,
    /// `false`: Der Server kennt diesen PIREP nicht (oder er gehört nicht
    /// zum angemeldeten Piloten). Dann ist nichts anzuzeigen.
    pub known: bool,
    /// Liegt das Flugprotokoll auf dem Server? `None` = Server kennt das
    /// Feld noch nicht (aelterer Recorder) — dann wird NICHTS nachgereicht.
    #[serde(default)]
    pub flug_log_vorhanden: Option<bool>,
    /// Liegt das Diagnose-Log auf dem Server? `None` = aelterer Recorder,
    /// dann wird nichts nachgereicht. Abgebrochene Fluege laden beim
    /// Abbruch selbst hoch; das hier faengt die Faelle, in denen das
    /// scheiterte (23.09.2026).
    #[serde(default)]
    pub diagnose_log_vorhanden: Option<bool>,
    #[serde(default)]
    pub score_trust_level: Option<String>,
    #[serde(default)]
    pub requires_review: bool,
    #[serde(default)]
    pub review_state: Option<String>,
    #[serde(default)]
    pub review_decision: Option<String>,
    #[serde(default)]
    pub reason_codes: Vec<String>,
    #[serde(default)]
    pub reviewed_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct Antwort {
    pireps: Vec<PirepPruefstatus>,
}

/// Nur IDs, die der Server annimmt ([A-Za-z0-9_-]{1,128}); Dubletten raus,
/// Reihenfolge bleibt.
pub fn gueltige_ids(ids: &[String]) -> Vec<String> {
    let mut gesehen = std::collections::HashSet::new();
    ids.iter()
        .filter(|id| {
            !id.is_empty()
                && id.len() <= 128
                && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        })
        .filter(|id| gesehen.insert((*id).clone()))
        .cloned()
        .collect()
}

/// Fragt den Prüfstatus in Paketen von höchstens [`MAX_IDS_JE_ANFRAGE`] ab.
pub async fn pruefstatus_abrufen(
    ids: &[String],
    username: &str,
    password: &str,
    endpoint: Option<&str>,
) -> Result<Vec<PirepPruefstatus>> {
    let ids = gueltige_ids(ids);
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let url = endpoint.map(String::from).unwrap_or_else(default_status_url);
    let auth_b64 =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}").as_bytes());
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let mut alle = Vec::with_capacity(ids.len());
    for paket in ids.chunks(MAX_IDS_JE_ANFRAGE) {
        let res = client
            .get(&url)
            .query(&[("pirep_ids", paket.join(","))])
            .header("Authorization", format!("Basic {auth_b64}"))
            .send()
            .await
            .context("pirep-status GET fehlgeschlagen")?;
        let status = res.status();
        if !status.is_success() {
            let body = res.text().await.unwrap_or_default();
            anyhow::bail!("pirep-status abgelehnt: HTTP {} — {}", status.as_u16(), body);
        }
        let antwort: Antwort = res.json().await.context("pirep-status: Antwort unlesbar")?;
        alle.extend(antwort.pireps);
    }
    Ok(alle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ungueltige_und_doppelte_ids_fallen_raus() {
        let ids = vec![
            "278xq2bgoYV5qZjL".to_string(),
            "a/b".to_string(),
            "".to_string(),
            "278xq2bgoYV5qZjL".to_string(),
            "x".repeat(129),
            "ok_-1".to_string(),
        ];
        assert_eq!(gueltige_ids(&ids), vec!["278xq2bgoYV5qZjL".to_string(), "ok_-1".to_string()]);
    }

    #[test]
    fn antwort_des_servers_ist_lesbar_auch_ohne_optionale_felder() {
        let json = r#"{"pireps":[
            {"pirep_id":"A","known":true,"score_trust_level":"review","requires_review":true,
             "review_state":"open","review_decision":null,"reason_codes":["no_touchdown_recorded"],"reviewed_at":null},
            {"pirep_id":"B","known":false}
        ]}"#;
        let a: Antwort = serde_json::from_str(json).unwrap();
        assert_eq!(a.pireps.len(), 2);
        assert!(a.pireps[0].requires_review);
        assert_eq!(a.pireps[0].reason_codes, vec!["no_touchdown_recorded"]);
        assert!(!a.pireps[1].known);
        assert!(a.pireps[1].reason_codes.is_empty());
    }

    #[test]
    fn endpunkt_haengt_am_selben_server_wie_provision() {
        assert!(default_status_url().ends_with("/api/flight-logs/pirep-status"));
        assert!(!default_status_url().contains("/api/provision"));
    }
}
