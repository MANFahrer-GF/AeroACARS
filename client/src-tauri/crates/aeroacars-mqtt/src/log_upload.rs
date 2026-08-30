//! Forensik: gzip + POST des per-Flug JSONL-Logfiles an aeroacars-live
//! nach erfolgreichem PIREP-File. Der VA-Owner kriegt damit ohne den
//! Piloten kontaktieren zu muessen den vollstaendigen Telemetrie-Stream
//! (alle 80 SimSnapshot-Felder + PhaseChanged + Activity-Log + LandingScored).
//!
//! Auth: HTTP Basic gegen die provisioned_pilots-Tabelle des Recorders —
//! gleiche Username/Password-Combo die auch Mosquitto fuer MQTT verwendet.
//! Wir verwenden die Cred-Pair, die per `provision()` schon im OS-Keyring
//! gecached ist (siehe lib.rs MQTT_KEYRING_USERNAME/PASSWORD).
//!
//! Fehlertoleranz: fire-and-forget aus Anrufer-Sicht. Wir loggen alles
//! via tracing, aber blocken keine User-facing-Operation.
//!
//! Endpoint-URL: aus der provision-URL abgeleitet (Replace `/api/provision`
//! → `/api/flight-logs/upload`) damit Test-VPS / Dev-Setups automatisch
//! mitziehen ohne separate Konfiguration.

use anyhow::{Context, Result};
use base64::Engine;
use flate2::{write::GzEncoder, Compression};
use std::io::Write as _;
use std::path::Path;
use std::time::Duration;

use crate::provision::DEFAULT_PROVISION_URL;

/// Default-Endpoint — abgeleitet aus DEFAULT_PROVISION_URL damit beide
/// gegen denselben aeroacars-live Host gehen.
fn default_upload_url() -> String {
    DEFAULT_PROVISION_URL.replace("/api/provision", "/api/flight-logs/upload")
}

/// Lade JSONL-Datei, komprimiere mit gzip, POSTe an aeroacars-live.
///
/// Args:
/// - `log_path`  — absoluter Pfad zur `<pirep_id>.jsonl`-Datei lokal
/// - `pirep_id`  — gehoert zu der Session die der Server mit dem Log verknuepft
/// - `username`  — Pilot-Username aus der MQTT-Provision (= "pilot_<id>")
/// - `password`  — gleiches Passwort wie fuer Mosquitto
/// - `endpoint`  — None = Default; Some(...) fuer Test-Setups
///
/// Returns Ok wenn HTTP 200; Err sonst (Caller entscheidet ob Retry-Queue
/// genutzt wird).
pub async fn upload_flight_log(
    log_path: &Path,
    pirep_id: &str,
    username: &str,
    password: &str,
    endpoint: Option<&str>,
) -> Result<UploadStats> {
    let raw = tokio::fs::read(log_path)
        .await
        .with_context(|| format!("read log file {log_path:?}"))?;
    if raw.is_empty() {
        anyhow::bail!("log file is empty — nothing to upload");
    }
    let raw_size = raw.len();

    // Compression im Blocking-Pool damit der Tokio-Reactor nicht blockiert.
    // 1-2 MB JSONL sind nach gzip ~200-400 KB; CPU-Cost <100 ms.
    let compressed = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::with_capacity(raw.len() / 4), Compression::default());
        encoder.write_all(&raw)?;
        Ok(encoder.finish()?)
    })
    .await
    .context("gzip task panic")??;

    let compressed_size = compressed.len();
    tracing::info!(
        pirep_id = %pirep_id,
        raw_kb = raw_size / 1024,
        gzip_kb = compressed_size / 1024,
        ratio = format!("{:.0}%", (compressed_size as f64 / raw_size as f64) * 100.0),
        "uploading flight log",
    );

    let url = endpoint
        .map(String::from)
        .unwrap_or_else(default_upload_url);
    let auth_token = format!("{username}:{password}");
    let auth_b64 = base64::engine::general_purpose::STANDARD.encode(auth_token.as_bytes());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()?;

    let res = client
        .post(&url)
        .header("Authorization", format!("Basic {auth_b64}"))
        .header("X-Pirep-Id", pirep_id)
        .header("Content-Type", "application/gzip")
        .body(compressed)
        .send()
        .await
        .context("upload POST failed")?;

    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("upload rejected: HTTP {} — {}", status.as_u16(), body);
    }

    Ok(UploadStats {
        raw_size,
        compressed_size,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct UploadStats {
    pub raw_size: usize,
    pub compressed_size: usize,
}
