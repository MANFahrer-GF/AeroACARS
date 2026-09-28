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

/// Ein schon vermessenes Flugzeug (ohne Pilotenbezug).
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct Vermessen {
    pub sim: Option<String>,
    /// „boden" oder „luft" — ohne das Feld zeigte die Liste jede Luft-Messung
    /// als Boden (QS 28.09.2026). Ältere Server liefern es nicht.
    #[serde(default)]
    pub teil: Option<String>,
    /// MSFS: L:-Namen, die die Scans für dieses Flugzeug liefern (None =
    /// X-Plane oder älterer Server).
    #[serde(default)]
    pub scan_namen: Option<u32>,
    pub icao: Option<String>,
    pub titel: Option<String>,
    /// Jüngste Messung, ms seit 1970.
    pub zuletzt: i64,
    pub anzahl: u32,
}

/// Ein Aircraft-Scan der VA (Übersichtstabelle: auch gescannte, aber noch
/// nicht vermessene Flugzeuge).
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct ScanEintrag {
    pub sim: Option<String>,
    pub icao: Option<String>,
    pub paket: Option<String>,
    #[serde(default)]
    pub titel_liste: Vec<String>,
    pub scan_namen: Option<u32>,
    /// „fertig" | „in_arbeit" | None — Profil-Stand, wie der Admin ihn setzt.
    pub profil: Option<String>,
    pub zuletzt: i64,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct VermessenListe {
    pub flugzeuge: Vec<Vermessen>,
    /// Ältere Server liefern keine Scans.
    #[serde(default)]
    pub scans: Vec<ScanEintrag>,
}

/// Welche Flugzeuge die VA schon vermessen und gescannt hat.
pub async fn vermessen(base: Option<&str>, token: &str) -> Result<VermessenListe, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = client
        .get(url(base, "/vermessen"))
        .bearer_auth(token)
        .send()
        .await?;
    let r = pruefen(r).await?;
    r.json::<VermessenListe>()
        .await
        .map_err(|e| NavdataError::BadResponse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Die Liste muss `teil` bis zur Oberfläche durchreichen.
    #[test]
    fn vermessen_behaelt_den_teil() {
        let l: VermessenListe = serde_json::from_str(
            r#"{"flugzeuge":[{"sim":"msfs","teil":"luft","icao":"A388","titel":"A380-800 RR Basic","zuletzt":1,"anzahl":1},
                             {"sim":"xplane","icao":"B77W","titel":"B777","zuletzt":2,"anzahl":3}]}"#,
        )
        .unwrap();
        assert_eq!(l.flugzeuge[0].teil.as_deref(), Some("luft"));
        assert_eq!(l.flugzeuge[1].teil, None, "Altbestand ohne Teil");
        let aus = serde_json::to_value(&l.flugzeuge[0]).unwrap();
        assert_eq!(aus["teil"], "luft", "auch zur Oberfläche serialisiert");
        assert!(l.scans.is_empty(), "älterer Server ohne scans");
        let mit: VermessenListe = serde_json::from_str(
            r#"{"flugzeuge":[],"scans":[{"sim":"msfs","icao":"BCS3","paket":"Synaptic A220","titel_liste":["A220-300"],"scan_namen":1200,"profil":"in_arbeit","zuletzt":5,"quelle":"client"}]}"#,
        )
        .unwrap();
        assert_eq!(mit.scans[0].titel_liste, ["A220-300"]);
        assert_eq!(mit.scans[0].profil.as_deref(), Some("in_arbeit"));
    }
}
