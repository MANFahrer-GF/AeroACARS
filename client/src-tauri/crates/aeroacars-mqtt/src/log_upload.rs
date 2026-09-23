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

/// Hoechstens so viel rohes Diagnose-Log wird geschickt. Ist mehr da, faellt
/// der ANFANG weg — das Ende ist das Interessante.
const DIAGNOSE_MAX_ROH: usize = 8 * 1024 * 1024;

fn default_diagnose_url() -> String {
    DEFAULT_PROVISION_URL.replace("/api/provision", "/api/flight-logs/diagnose")
}

/// Laedt das DIAGNOSE-Log des Clients hoch (die taegliche tracing-Datei).
///
/// # Warum
///
/// Am 20.09.2026 riss bei mehreren Piloten die Verbindung im Minutentakt.
/// Serverseitig sah das wie ein Netzproblem aus; die Zeile, die den Fehler
/// benannte, stand nur auf dem Rechner des Piloten. Die Suche dauerte zwei
/// Tage und endete erst, als jemand seine Logdatei schickte. Seitdem reist
/// sie mit dem Flugprotokoll mit.
///
/// Best effort: Schlaegt das fehl, ist das kein Grund, irgendetwas anderes
/// abzubrechen — der Flugbericht ist wichtiger als seine Diagnose.
pub async fn upload_diagnose_logs(
    log_paths: &[std::path::PathBuf],
    pirep_id: &str,
    username: &str,
    password: &str,
    endpoint: Option<&str>,
) -> Result<UploadStats> {
    upload_diagnose_logs_mit(log_paths, pirep_id, username, password, endpoint, false).await
}

/// Wie `upload_diagnose_logs`, aber mit der Wahl, welche Seite beim
/// Kuerzen ueberlebt.
///
/// Beim Einreichen ist der Fehler das Letzte, was passiert ist — da bleibt
/// das Ende. Beim NACHREICHEN laeuft der Upload kurz nach dem Programmstart:
/// Das Ende sind dann die frischen Startzeilen von heute, und der gesuchte
/// Abriss von gestern faellt als erstes weg (Cloud-QS 23.09.2026).
pub async fn upload_diagnose_logs_mit(
    log_paths: &[std::path::PathBuf],
    pirep_id: &str,
    username: &str,
    password: &str,
    endpoint: Option<&str>,
    anfang_behalten: bool,
) -> Result<UploadStats> {
    let paket = diagnose_paket_bauen(log_paths, anfang_behalten).await?;
    diagnose_paket_senden(&paket, pirep_id, username, password, endpoint).await
}

/// Ein fertig gepacktes Diagnose-Paket — einmal lesen, einmal zippen,
/// beliebig oft verschicken.
#[derive(Clone)]
pub struct DiagnosePaket {
    gz: Vec<u8>,
    raw_size: usize,
}

/// Liest die Tagesdateien, kuerzt auf `DIAGNOSE_MAX_ROH` und packt.
///
/// Getrennt vom Verschicken, weil das Nachreichen bis zu fuenf PIREPs
/// mit DENSELBEN ein bis zwei Tagesdateien bedient: vorher wurde dafuer
/// fuenfmal gelesen und fuenfmal gzippt (Cloud-QS 23.09.2026).
pub async fn diagnose_paket_bauen(
    log_paths: &[std::path::PathBuf],
    anfang_behalten: bool,
) -> Result<DiagnosePaket> {
    // Mehrere Tagesdateien in zeitlicher Reihenfolge, getrennt durch eine
    // Kopfzeile — ein Flug ueber Mitternacht braucht beide.
    let mut raw: Vec<u8> = Vec::new();
    for pfad in log_paths {
        let teil = match tokio::fs::read(pfad).await {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!(pfad = ?pfad, error = %e, "Diagnose-Teil nicht lesbar");
                continue;
            }
        };
        raw.extend_from_slice(format!("\n===== {} =====\n", pfad.display()).as_bytes());
        raw.extend_from_slice(&teil);
    }
    if raw.is_empty() {
        anyhow::bail!("diagnose log is empty");
    }
    if raw.len() > DIAGNOSE_MAX_ROH {
        if anfang_behalten {
            raw.truncate(DIAGNOSE_MAX_ROH);
        } else {
            // Vorn abschneiden: Der Fehler steht am Ende, nicht am Anfang.
            raw = raw.split_off(raw.len() - DIAGNOSE_MAX_ROH);
        }
    }
    let raw_size = raw.len();

    let compressed = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
        let mut encoder = GzEncoder::new(Vec::with_capacity(raw.len() / 8), Compression::default());
        encoder.write_all(&raw)?;
        Ok(encoder.finish()?)
    })
    .await
    .context("gzip task panic")??;
    Ok(DiagnosePaket {
        gz: compressed,
        raw_size,
    })
}

/// Verschickt ein gebautes Paket unter einer PIREP-Kennung.
pub async fn diagnose_paket_senden(
    paket: &DiagnosePaket,
    pirep_id: &str,
    username: &str,
    password: &str,
    endpoint: Option<&str>,
) -> Result<UploadStats> {
    let raw_size = paket.raw_size;
    let compressed = paket.gz.clone();
    let compressed_size = compressed.len();

    let url = endpoint
        .map(String::from)
        .unwrap_or_else(default_diagnose_url);
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
        .context("diagnose upload POST failed")?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!(
            "diagnose upload rejected: HTTP {} — {}",
            status.as_u16(),
            body
        );
    }
    tracing::info!(
        pirep_id = %pirep_id,
        roh_kb = raw_size / 1024,
        gzip_kb = compressed_size / 1024,
        "Diagnose-Log hochgeladen"
    );
    Ok(UploadStats {
        raw_size,
        compressed_size,
    })
}

/// Ein Wegpunkt einer fremden Route — so, wie der Server ihn liefert.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct FremderWegpunkt {
    #[serde(default)]
    pub name: Option<String>,
    pub lat: f64,
    pub lon: f64,
}

/// Die Route eines Kollegen für die Karte.
///
/// Thomas, 20.09.2026: „in der Map auf die anderen Flieger klicken und die
/// Route sehen." Der Server gibt nur Routen DERSELBEN VA heraus und
/// antwortet auf Unbekanntes mit einer leeren Liste — ein Flug ohne
/// SimBrief-Plan hat schlicht keine.
pub async fn fremde_route(
    pirep_id: &str,
    username: &str,
    password: &str,
    endpoint: Option<&str>,
) -> Result<Vec<FremderWegpunkt>> {
    #[derive(serde::Deserialize)]
    struct Antwort {
        #[serde(default)]
        waypoints: Vec<FremderWegpunkt>,
    }
    let basis = endpoint
        .map(String::from)
        .unwrap_or_else(|| DEFAULT_PROVISION_URL.replace("/api/provision", "/api/flight-route"));
    let url = format!("{}/{}", basis.trim_end_matches('/'), pirep_id);
    let auth_b64 =
        base64::engine::general_purpose::STANDARD.encode(format!("{username}:{password}"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let res = client
        .get(&url)
        .header("Authorization", format!("Basic {auth_b64}"))
        .send()
        .await
        .context("route GET failed")?;
    let status = res.status();
    if !status.is_success() {
        anyhow::bail!("route rejected: HTTP {}", status.as_u16());
    }
    let antwort: Antwort = res.json().await.context("route JSON")?;
    Ok(antwort.waypoints)
}

#[derive(Debug, Clone, Copy)]
pub struct UploadStats {
    pub raw_size: usize,
    pub compressed_size: usize,
}
