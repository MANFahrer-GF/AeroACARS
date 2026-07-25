//! Sicherung der Landungshistorie auf dem Live-Server.
//!
//! Die Landungen liegen im Client nur lokal in `landings.json`. Wer seinen
//! Rechner neu aufsetzt oder die Datei verliert, faengt sonst bei null an.
//!
//! ## Was gesichert wird — und was nicht
//!
//! NUR die Kennzahlen. Die Messkurven (`touchdown_profile`,
//! `approach_samples`) bleiben aussen vor, und zwar aus zwei Gruenden:
//!
//! 1. Sie sind 95 % der Datenmenge. Gemessen an einem echten Bestand
//!    (24 Landungen, 2,4 MB): Profil 83 %, Anflugproben 12 %, alles andere
//!    3,3 % — also 3,6 KB je Landung statt 134 KB. Bei 500 Landungen sind
//!    das 1,8 MB statt 67 MB.
//! 2. Sie liegen bereits auf dem Server. Die Flug-Aufzeichnungen werden nach
//!    jedem Flug hochgeladen und sind nach PIREP-Kennung benannt; zusaetzlich
//!    stehen die Touchdown-Fenster in der Recorder-Datenbank. Ein zweites Mal
//!    zu sichern, was schon gesichert ist, kostet nur Bandbreite.
//!
//! Nach einer Wiederherstellung fehlen deshalb die beiden Detaildiagramme
//! alter Landungen — die Ansicht laesst sie sauber weg (`length >= 5`), es
//! bleibt kein leerer Rahmen stehen. Liste, Werte und Statistik sind
//! vollstaendig.
//!
//! ## Schutz
//!
//! Kein Pilot sieht die Landungen eines anderen; Administratoren duerfen
//! (Entscheidung 25.07.2026, siehe docs/spec/landing-backup.md). Deshalb
//! keine Verschluesselung, sondern Zugriffskontrolle: Der Server nimmt die
//! Piloten-Kennung ausschliesslich aus dem geprueften Token, nie aus Pfad
//! oder Rumpf.

use serde::{Deserialize, Serialize};

use crate::navdata::{build_client, NavdataError, DEFAULT_NAVDATA_BASE};

/// Antwort auf einen erfolgreichen Upload.
#[derive(Debug, Clone, Deserialize)]
pub struct BackupPutResult {
    pub ok: bool,
    pub count: usize,
    pub bytes: usize,
}

/// Serverstand, wie er zurueckkommt.
#[derive(Debug, Clone, Deserialize)]
pub struct BackupPayload {
    pub saved_at: String,
    pub count: usize,
    pub landings: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize)]
struct BackupBody<'a> {
    landings: &'a [serde_json::Value],
}

fn url(base: Option<&str>, suffix: &str) -> String {
    let base = base.unwrap_or(DEFAULT_NAVDATA_BASE);
    format!("{}/api/backup/landings{suffix}", base.trim_end_matches('/'))
}

/// Landungen hochladen. Ersetzt den Serverstand; der vorherige wandert dort
/// in eine Historie der letzten fuenf Staende.
pub async fn put_landings(
    base: Option<&str>,
    auth_token: &str,
    landings: &[serde_json::Value],
) -> Result<BackupPutResult, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let response = client
        .put(url(base, ""))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {auth_token}"))
        .json(&BackupBody { landings })
        .send()
        .await?;

    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(NavdataError::Unauthorized);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(NavdataError::Network(format!("{status}: {body}")));
    }
    response
        .json::<BackupPutResult>()
        .await
        .map_err(|e| NavdataError::Network(e.to_string()))
}

/// Den eigenen letzten Stand holen. `Ok(None)` heisst: Es gibt noch keinen —
/// bewusst kein Fehler, das ist der Normalfall beim ersten Start.
pub async fn get_landings(
    base: Option<&str>,
    auth_token: &str,
) -> Result<Option<BackupPayload>, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let response = client
        .get(url(base, ""))
        .header(reqwest::header::AUTHORIZATION, format!("Bearer {auth_token}"))
        .send()
        .await?;

    let status = response.status();
    if status.as_u16() == 404 {
        return Ok(None);
    }
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(NavdataError::Unauthorized);
    }
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(NavdataError::Network(format!("{status}: {body}")));
    }
    response
        .json::<BackupPayload>()
        .await
        .map(Some)
        .map_err(|e| NavdataError::Network(e.to_string()))
}

/// Die Kurven aus einem Landungsdatensatz entfernen.
///
/// Arbeitet auf dem JSON und nicht auf dem Typ, damit neue Felder im
/// `LandingRecord` automatisch mitgesichert werden — nur diese beiden
/// bleiben aussen vor. Andersherum (Felder einzeln aufzaehlen) haette jedes
/// neue Feld stillschweigend gefehlt.
pub fn strip_curves(mut record: serde_json::Value) -> serde_json::Value {
    if let Some(obj) = record.as_object_mut() {
        // Leere Listen statt Entfernen: Der Datensatz muss sich beim
        // Zurueckholen wieder als LandingRecord lesen lassen, und beide
        // Felder sind dort Pflicht.
        for key in ["touchdown_profile", "approach_samples"] {
            if obj.contains_key(key) {
                obj.insert(key.to_string(), serde_json::Value::Array(Vec::new()));
            }
        }
    }
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strip_curves_removes_the_bulk_but_keeps_the_facts() {
        let rec = json!({
            "pirep_id": "ABC",
            "score_numeric": 88,
            "landing_rate_fpm": -236.0,
            "touchdown_profile": [{"t_ms": 0}, {"t_ms": 20}, {"t_ms": 40}],
            "approach_samples": [{"t_ms": 0}, {"t_ms": 1000}],
            "sub_scores": [{"key": "landing_rate", "value": 70}],
        });
        let out = strip_curves(rec);
        assert_eq!(out["touchdown_profile"].as_array().unwrap().len(), 0);
        assert_eq!(out["approach_samples"].as_array().unwrap().len(), 0);
        // Alles, was die Landung ausmacht, bleibt.
        assert_eq!(out["pirep_id"], "ABC");
        assert_eq!(out["score_numeric"], 88);
        assert_eq!(out["landing_rate_fpm"], -236.0);
        assert_eq!(out["sub_scores"].as_array().unwrap().len(), 1);
    }

    /// Die beiden Felder muessen als LEERE LISTE bestehen bleiben, nicht
    /// verschwinden — sie sind im LandingRecord Pflichtfelder, ein fehlender
    /// Schluessel liesse sich nicht zurueckdeserialisieren.
    #[test]
    fn stripped_record_still_has_both_keys() {
        let out = strip_curves(json!({
            "pirep_id": "ABC",
            "touchdown_profile": [{"t_ms": 0}],
            "approach_samples": [{"t_ms": 0}],
        }));
        assert!(out.get("touchdown_profile").is_some());
        assert!(out.get("approach_samples").is_some());
    }

    /// Ein Datensatz ohne die Felder darf nicht kuenstlich welche bekommen.
    #[test]
    fn leaves_records_without_curves_alone() {
        let out = strip_curves(json!({ "pirep_id": "ABC" }));
        assert!(out.get("touchdown_profile").is_none());
    }

    /// Der eigentliche Zweck, an echten Groessenverhaeltnissen gemessen:
    /// 478 Profilpunkte + 120 Anflugproben je Landung.
    #[test]
    fn strips_the_overwhelming_majority_of_the_payload() {
        let profile: Vec<serde_json::Value> = (0..478)
            .map(|i| json!({"t_ms": i * 20, "vs_fpm": -240.0, "g_force": 1.1, "agl_ft": 50.0,
                            "on_ground": false, "heading_true_deg": 73.0, "groundspeed_kt": 140,
                            "indicated_airspeed_kt": 135, "pitch_deg": 2.0, "bank_deg": 0.5}))
            .collect();
        let approach: Vec<serde_json::Value> = (0..120)
            .map(|i| json!({"t_ms": i * 1000, "vs_fpm": -700.0, "agl_ft": 2000.0,
                            "ias_kt": 160, "bank_deg": 1.0, "on_glide": true}))
            .collect();
        let rec = json!({
            "pirep_id": "ABC", "score_numeric": 88,
            "touchdown_profile": profile, "approach_samples": approach,
        });
        let before = serde_json::to_string(&rec).unwrap().len();
        let after = serde_json::to_string(&strip_curves(rec)).unwrap().len();
        assert!(
            after * 20 < before,
            "erwartet >95 % Ersparnis, war {before} -> {after}"
        );
    }
}
