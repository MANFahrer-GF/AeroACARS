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

/// Anfrage für [`lvar_namen`] — getrennt, damit Tests die fertige URL
/// (Kodierung von Leerzeichen, Backslashes, Klammern) prüfen können.
///
/// `pfad` ist der aircraft.cfg-Pfad aus `AircraftLoaded` (z. B.
/// `SimObjects\Airplanes\iFly 737-MAX8-189Seats\aircraft.CFG`). Der Server
/// ordnet darüber den Scan eindeutig zu (SimObject-Ordner); ohne Pfad bleibt
/// es beim Abgleich über ICAO und Titel. Ein leerer Pfad wird nicht
/// gesendet, damit ältere Server nichts Neues sehen.
fn lvar_namen_anfrage(
    client: &reqwest::Client,
    base: Option<&str>,
    token: &str,
    icao: &str,
    titel: &str,
    pfad: Option<&str>,
) -> reqwest::RequestBuilder {
    let mut q = vec![("icao", icao), ("titel", titel)];
    if let Some(p) = pfad.map(str::trim).filter(|p| !p.is_empty()) {
        q.push(("pfad", p));
    }
    client
        .get(url(base, "/lvar-namen"))
        .query(&q)
        .bearer_auth(token)
}

/// L:-Namen aus den Aircraft-Scans, die zu diesem Flugzeug passen.
pub async fn lvar_namen(
    base: Option<&str>,
    token: &str,
    icao: &str,
    titel: &str,
    pfad: Option<&str>,
) -> Result<Vec<String>, NavdataError> {
    let client = build_client().map_err(|e| NavdataError::Network(e.to_string()))?;
    let r = lvar_namen_anfrage(&client, base, token, icao, titel, pfad)
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
    /// „geprueft" | „aus_scan" | „in_arbeit" | None (Admin-Status).
    #[serde(default)]
    pub profil: Option<String>,
    pub icao: Option<String>,
    pub titel: Option<String>,
    /// MSFS: SimObject-Ordner aus dem aircraft.cfg-Pfad der Messung (Server
    /// ab 29.09.2026) — eindeutiger Abgleich mit dem geladenen Flugzeug,
    /// über alle Lackierungen hinweg. Ältere Server/Messungen: None.
    #[serde(default)]
    pub ordner: Option<String>,
    /// Alle gemessenen Titel dieses Flugzeugs (Lackierungen); ältere Server
    /// liefern nur `titel`.
    #[serde(default)]
    pub titel_liste: Vec<String>,
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
    /// Herkunft: „client“/„web“ = Aircraft-Scan, „aao-profil“ /
    /// „hersteller-doku“ = von uns hinterlegte Namensquelle.
    #[serde(default)]
    pub quelle: Option<String>,
    /// MSFS: SimObject-Ordner aus der Dateiliste des Scans (Server ab
    /// 29.09.2026).
    #[serde(default)]
    pub ordner: Vec<String>,
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

    fn anfrage_url(pfad: Option<&str>) -> reqwest::Url {
        lvar_namen_anfrage(
            &reqwest::Client::new(),
            Some("https://live.example/"),
            "tok",
            "B38M",
            "ifly-aircraft-737max8-TUI DAMAH-189Seats",
            pfad,
        )
        .build()
        .unwrap()
        .url()
        .clone()
    }

    /// Der aircraft.cfg-Pfad geht vollständig und dekodierbar mit — auch
    /// Backslashes, Leerzeichen und Groß-/Kleinschreibung bleiben erhalten.
    #[test]
    fn lvar_namen_schickt_den_pfad_kodiert_mit() {
        let pfad = r"SimObjects\Airplanes\iFly 737-MAX8-189Seats\aircraft.CFG";
        let u = anfrage_url(Some(pfad));
        assert_eq!(u.path(), "/api/ascan/lvar-namen");
        let q: Vec<(String, String)> = u.query_pairs().into_owned().collect();
        assert_eq!(
            q,
            [
                ("icao".to_string(), "B38M".to_string()),
                (
                    "titel".to_string(),
                    "ifly-aircraft-737max8-TUI DAMAH-189Seats".to_string()
                ),
                ("pfad".to_string(), pfad.to_string()),
            ]
        );
        // Roh kodiert: Backslash als %5C, kein nackter Backslash in der URL.
        let roh = u.query().unwrap();
        assert!(roh.contains("pfad=SimObjects%5CAirplanes%5CiFly"), "{roh}");
        assert!(!roh.contains('\\'), "{roh}");
    }

    /// Ohne Pfad (älterer Stand, X-Plane, `AircraftLoaded` noch nicht da)
    /// fehlt der Parameter ganz — kein leeres `pfad=`.
    #[test]
    fn lvar_namen_ohne_pfad_kein_parameter() {
        for p in [None, Some(""), Some("   ")] {
            let u = anfrage_url(p);
            assert!(
                !u.query_pairs().any(|(k, _)| k == "pfad"),
                "{p:?} → {}",
                u.query().unwrap_or_default()
            );
        }
        // Umgebende Leerzeichen werden abgeschnitten.
        let u = anfrage_url(Some("  SimObjects/Airplanes/FNX_32X/aircraft.cfg \n"));
        let pfad = u
            .query_pairs()
            .find(|(k, _)| k == "pfad")
            .map(|(_, v)| v.into_owned());
        assert_eq!(
            pfad.as_deref(),
            Some("SimObjects/Airplanes/FNX_32X/aircraft.cfg")
        );
    }

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
        assert!(mit.scans[0].ordner.is_empty(), "älterer Server ohne ordner");
    }

    /// Ordner und Titel-Liste (Server ab 29.09.2026) kommen bis zur
    /// Oberfläche durch — ohne sie könnte sie nicht über den Ordner abgleichen.
    #[test]
    fn vermessen_reicht_ordner_durch() {
        let l: VermessenListe = serde_json::from_str(
            r#"{"flugzeuge":[{"sim":"msfs","teil":"boden","icao":"B38M","titel":"ifly RYR","ordner":"ifly 737-max8-189seats","titel_liste":["ifly RYR","ifly TUI"],"zuletzt":1,"anzahl":2}],
                "scans":[{"sim":"msfs","icao":"B38M","paket":"737MAX","titel_liste":[],"scan_namen":1873,"profil":null,"zuletzt":5,"ordner":["ifly 737-max8","ifly 737-max8-189seats"]}]}"#,
        )
        .unwrap();
        let aus = serde_json::to_value(&l).unwrap();
        assert_eq!(aus["flugzeuge"][0]["ordner"], "ifly 737-max8-189seats");
        assert_eq!(aus["flugzeuge"][0]["titel_liste"][1], "ifly TUI");
        assert_eq!(aus["scans"][0]["ordner"][1], "ifly 737-max8-189seats");
    }
}
