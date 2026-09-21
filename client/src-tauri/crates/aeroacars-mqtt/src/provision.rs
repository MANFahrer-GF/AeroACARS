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

/// Der Server hat die Anmeldung mit einem HTTP-Status abgelehnt.
/// Netzfehler kommen NICHT als dieser Typ — die sind immer voruebergehend.
#[derive(Debug, Clone, thiserror::Error)]
#[error("provision rejected: {text} ({status})")]
pub struct ProvisionAbgelehnt {
    pub status: u16,
    /// `Retry-After` in Sekunden, wenn der Server einen genannt hat (429).
    pub retry_after_s: Option<u64>,
    pub text: String,
}

/// Erster Abstand fuer einen erneuten Versuch; verdoppelt sich je Versuch.
const WIEDERHOLUNG_START: Duration = Duration::from_secs(30);
/// Laengster Abstand zwischen zwei Versuchen.
const WIEDERHOLUNG_MAX: Duration = Duration::from_secs(15 * 60);

/// Wann nach einem gescheiterten Provision-Aufruf erneut gefragt wird —
/// `None` heisst: gar nicht, ein neuer Versuch aendert nichts.
///
/// Befund 21.09.2026: Ein einziger gescheiterter Aufruf (bei Sven M ein
/// 429 aus der Anmelde-Bremse des Servers) schaltete das Live-Tracking
/// fuer die ganze Sitzung ab. Er flog LHBP→EDDS ohne Live-Karte, und weil
/// der Recorder keine Sitzung kannte, lehnte er danach auch Flug- und
/// Diagnose-Log ab. Voruebergehende Fehler (Netz, 429, 5xx, 408) werden
/// deshalb wiederholt; ein ungueltiger Schluessel (401/403) oder eine
/// kaputte Anfrage (400/404/422) nicht — die wuerden nur die Bremse
/// weiter fuellen.
pub fn naechster_versuch(fehler: &anyhow::Error, versuch: u32) -> Option<Duration> {
    let rueckzug = WIEDERHOLUNG_START
        .checked_mul(1u32 << versuch.min(8))
        .unwrap_or(WIEDERHOLUNG_MAX)
        .min(WIEDERHOLUNG_MAX);
    let Some(abgelehnt) = fehler.downcast_ref::<ProvisionAbgelehnt>() else {
        // Netz, Zeitueberschreitung, unlesbare Antwort — voruebergehend.
        return Some(rueckzug);
    };
    match abgelehnt.status {
        429 => Some(
            abgelehnt
                .retry_after_s
                .map(|s| Duration::from_secs(s.saturating_add(5)))
                .unwrap_or(rueckzug)
                .clamp(WIEDERHOLUNG_START, Duration::from_secs(60 * 60)),
        ),
        408 | 500..=599 => Some(rueckzug),
        _ => None,
    }
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
        let retry_after_s = res
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.trim().parse::<u64>().ok());
        let err = res
            .json::<ErrorBody>()
            .await
            .ok()
            .map(|b| b.error)
            .unwrap_or_else(|| status.to_string());
        return Err(ProvisionAbgelehnt {
            status: status.as_u16(),
            retry_after_s,
            text: err,
        }
        .into());
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

#[cfg(test)]
mod naechster_versuch_tests {
    use super::*;

    fn abgelehnt(status: u16, retry_after_s: Option<u64>) -> anyhow::Error {
        ProvisionAbgelehnt {
            status,
            retry_after_s,
            text: "x".into(),
        }
        .into()
    }

    /// Svens Fall: 429 mit „retry in 48 minutes" — der Client wartet so
    /// lange wie genannt, statt aufzugeben.
    #[test]
    fn bremse_429_wird_nach_der_genannten_zeit_wiederholt() {
        let d = naechster_versuch(&abgelehnt(429, Some(48 * 60)), 0).unwrap();
        assert_eq!(d, Duration::from_secs(48 * 60 + 5));
        // Ohne Angabe: normaler Rueckzug.
        assert_eq!(
            naechster_versuch(&abgelehnt(429, None), 0),
            Some(WIEDERHOLUNG_START)
        );
    }

    #[test]
    fn netz_und_serverfehler_werden_mit_wachsendem_abstand_wiederholt() {
        let netz = anyhow::anyhow!("provision request failed");
        assert_eq!(naechster_versuch(&netz, 0), Some(Duration::from_secs(30)));
        assert_eq!(naechster_versuch(&netz, 1), Some(Duration::from_secs(60)));
        assert_eq!(naechster_versuch(&netz, 3), Some(Duration::from_secs(240)));
        assert_eq!(naechster_versuch(&netz, 30), Some(WIEDERHOLUNG_MAX));
        assert_eq!(
            naechster_versuch(&abgelehnt(503, None), 2),
            Some(Duration::from_secs(120))
        );
    }

    /// Gegenprobe: ein ungueltiger Schluessel wird NICHT wiederholt — jeder
    /// Versuch fuellte nur die Anmelde-Bremse (10 pro Stunde je IP).
    #[test]
    fn harte_ablehnungen_werden_nicht_wiederholt() {
        for status in [400, 401, 403, 404, 422] {
            assert_eq!(
                naechster_versuch(&abgelehnt(status, None), 0),
                None,
                "{status}"
            );
        }
    }

    /// Der Typ ueberlebt den Weg durch `anyhow` (sonst griffe die
    /// Unterscheidung nie und alles saehe wie ein Netzfehler aus).
    #[test]
    fn der_fehlertyp_ist_durch_anyhow_erkennbar() {
        let e = abgelehnt(401, None);
        assert!(e.downcast_ref::<ProvisionAbgelehnt>().is_some());
        assert_eq!(e.to_string(), "provision rejected: x (401)");
    }
}
