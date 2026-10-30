//! v3 Etappe 3 (P-4.10, P-6.01, P-7.02, P-7.21): Eingabe-Dokument je
//! Aufsetzer an live (`POST /api/landing-input`).
//!
//! Das Dokument wird beim Abgeben gebaut und ZUERST auf die Platte gelegt
//! (`landing_inputs/<konto>/`, `<konto>` = Kurzhash des Pilot-Tokens: nach
//! einem Kontowechsel bleiben fremde Dokumente liegen statt mit dem falschen
//! Token abgelehnt zu werden), dann gesendet. Ohne Aufsetzfenster gibt es kein
//! Dokument (der Server verlangt mindestens eine Probe). Immer nur EIN
//! Sendevorgang; geloescht wird nur, wenn die Datei noch genau das Gesendete
//! enthaelt. Dateien aelter als 30 Tage werden aufgeraeumt. Scheitert das Senden (Netz, live
//! nicht erreichbar, 5xx), bleibt die Datei liegen und wird beim naechsten
//! Abgeben und beim Programmstart erneut versucht. Endgueltige Antworten:
//! 200 (auch „wiederholt“) loescht die Datei; 400/403/409/413 legen sie nach
//! `landing_inputs/abgelehnt/` (Diagnose, kein endloses Wiederholen).
//!
//! Vertrag (Server `recorder/src/landingInput.ts`): gzip-JSON
//! `{eingabe: {meta, eingang, herkunft}, fenster: {proben}, ergebnis}`.
//! `herkunft` (P-9.44, Kap. 5.2) erklaert `eingang` und geht nie in die
//! Rechnung ein: Modul und Server rechnen nur aus `eingang`.
//! `meta.input_sha256` = SHA-256 ueber `{"eingang": …, "herkunft": …}` —
//! die Herkunft ist Teil der Eingabe, `meta` (mit dem Hash selbst) nicht.

use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use landing_scoring::LandingScoringInput;
use recorder::TouchdownWindowSample;
use sha2::{Digest, Sha256};

use crate::bewertung::Status;

const ORDNER: &str = "landing_inputs";
const SCHEMA: u32 = 1;

/// Art des Aufsetzers (P-6.01).
#[derive(Debug, Clone, Copy)]
pub(crate) enum Art {
    Landung,
    /// P-4.13: Aufsetzen mit Wegsteigen (eingefroren vor dem Zuruecksetzen).
    TouchAndGo,
}

impl Art {
    fn text(self) -> &'static str {
        match self {
            Art::Landung => "landung",
            Art::TouchAndGo => "touch_and_go",
        }
    }
}

/// Das Dokument fuer `POST /api/landing-input`.
pub(crate) fn dokument(
    pirep_id: &str,
    seq: u32,
    art: Art,
    runtime_bundle_id: Option<String>,
    eingang: &LandingScoringInput,
    herkunft: &serde_json::Value,
    fenster: &[TouchdownWindowSample],
    ergebnis: &Option<(landing_scoring::wasm::Ausgabe, Status)>,
) -> serde_json::Value {
    let eingang_json = serde_json::json!({ "landung": eingang });
    let gehasht = serde_json::json!({ "eingang": eingang_json, "herkunft": herkunft });
    let input_sha256 = hex(&Sha256::digest(gehasht.to_string().as_bytes()));
    let ergebnis = match ergebnis {
        Some((a, _)) => {
            let mut v = serde_json::to_value(a).unwrap_or(serde_json::Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("bewertung_status".into(), crate::bewertung::status_text(ergebnis).into());
            }
            v
        }
        None => serde_json::json!({ "bewertung_status": "modulfehler" }),
    };
    serde_json::json!({
        "eingabe": {
            "meta": {
                "pirep_id": pirep_id,
                "seq": seq,
                "art": art.text(),
                "input_schema": SCHEMA,
                "runtime_bundle_id": runtime_bundle_id,
                "input_sha256": input_sha256,
                "client_version": env!("CARGO_PKG_VERSION"),
            },
            "eingang": eingang_json,
            "herkunft": herkunft,
        },
        "fenster": { "proben": fenster },
        "ergebnis": ergebnis,
    })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sicher(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

/// Aufbewahrung von Warteschlange und Abgelehntem.
const AUFBEWAHREN: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Kontoordner: Kurzhash des Pilot-Tokens (keine Kennung im Klartext).
pub(crate) fn konto(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))[..16].to_string()
}

/// Legt das Dokument gzip-gepackt in die Warteschlange des Kontos.
pub(crate) fn einreihen(app_daten: &Path, konto: &str, pirep_id: &str, seq: u32, doc: &serde_json::Value) -> std::io::Result<PathBuf> {
    let ordner = app_daten.join(ORDNER).join(sicher(konto));
    std::fs::create_dir_all(&ordner)?;
    let ziel = ordner.join(format!("{}_{seq}.json.gz", sicher(pirep_id)));
    let tmp = ziel.with_extension("tmp");
    let mut gz = GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(doc.to_string().as_bytes())?;
    std::fs::write(&tmp, gz.finish()?)?;
    std::fs::rename(&tmp, &ziel)?;
    Ok(ziel)
}

/// Was mit einer Datei nach einer Antwort geschieht.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Folge {
    Loeschen,
    Ablehnen,
    Behalten,
}

/// Antwort von live → Folge (reine Funktion, getestet).
pub(crate) fn folge(status: Option<u16>) -> Folge {
    match status {
        Some(200..=299) => Folge::Loeschen,
        Some(400 | 403 | 409 | 413) => Folge::Ablehnen,
        _ => Folge::Behalten,
    }
}

/// Alte Dateien (Warteschlange und Abgelehntes) aufraeumen.
fn aufraeumen(ordner: &Path) {
    for unter in [ordner.to_path_buf(), ordner.join("abgelehnt")] {
        let Ok(es) = std::fs::read_dir(&unter) else { continue };
        for e in es.flatten() {
            let alt = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
            if e.path().is_file() && alt.is_some_and(|a| a > AUFBEWAHREN) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

/// Sendet alle wartenden Dokumente des angemeldeten Kontos. Ohne
/// Pilot-Token passiert nichts (die Dateien bleiben liegen). Immer nur ein
/// Durchlauf gleichzeitig (Codex 29.10.).
pub(crate) async fn nachsenden(app_daten: PathBuf) {
    static NUR_EINER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _einer = NUR_EINER.lock().await;
    let Some(token) = crate::live_zugang::pilot_token() else {
        return;
    };
    let ordner = app_daten.join(ORDNER).join(konto(&token));
    aufraeumen(&ordner);
    let Ok(eintraege) = std::fs::read_dir(&ordner) else {
        return;
    };
    let url = format!("{}/api/landing-input", crate::live_zugang::live_basis());
    let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_secs(30)).build() else {
        return;
    };
    for e in eintraege.flatten() {
        let pfad = e.path();
        if pfad.extension().and_then(|x| x.to_str()) != Some("gz") {
            continue;
        }
        let Ok(koerper) = std::fs::read(&pfad) else { continue };
        let antwort = client
            .post(&url)
            .bearer_auth(&token)
            // Server: Parser fuer application/gzip, Koerper roh gepackt.
            .header("content-type", "application/gzip")
            .body(koerper.clone())
            .send()
            .await;
        let status = antwort.as_ref().ok().map(|r| r.status().as_u16());
        // Nur loeschen/verschieben, was noch genau das Gesendete ist.
        if std::fs::read(&pfad).ok().as_deref() != Some(koerper.as_slice()) {
            continue;
        }
        match folge(status) {
            Folge::Loeschen => {
                let _ = std::fs::remove_file(&pfad);
                tracing::info!(datei = %pfad.display(), "Eingabe-Dokument an live uebergeben");
            }
            Folge::Ablehnen => {
                let abgelehnt = ordner.join("abgelehnt");
                let _ = std::fs::create_dir_all(&abgelehnt);
                let _ = std::fs::rename(&pfad, abgelehnt.join(e.file_name()));
                tracing::warn!(datei = %pfad.display(), ?status, "Eingabe-Dokument von live abgelehnt");
            }
            Folge::Behalten => {
                tracing::info!(datei = %pfad.display(), ?status, "Eingabe-Dokument: live nicht erreichbar, spaeter erneut");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dokument_nach_vertrag() {
        let e = LandingScoringInput { vs_fpm: Some(-200.0), ..Default::default() };
        let ergebnis = crate::bewertung::bewerten(&e);
        let h = serde_json::json!({ "bahn": { "quelle": { "art": "ourairports" } } });
        let d = dokument("PIREP1", 1, Art::Landung, Some("3-abcdef123456".into()), &e, &h, &[], &ergebnis);
        assert_eq!(d["eingabe"]["meta"]["seq"], 1);
        assert_eq!(d["eingabe"]["meta"]["art"], "landung");
        assert_eq!(d["eingabe"]["meta"]["input_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(d["eingabe"]["eingang"]["landung"]["vs_fpm"], -200.0);
        assert_eq!(d["ergebnis"]["fassung"], 27);
        assert_eq!(d["ergebnis"]["bewertung_status"], "endgueltig");
        assert!(d["fenster"]["proben"].as_array().unwrap().is_empty());
        assert_eq!(d["eingabe"]["herkunft"], h);
        let ohne = dokument("PIREP1", 1, Art::Landung, None, &e, &h, &[], &None);
        assert_eq!(ohne["ergebnis"]["bewertung_status"], "modulfehler");
    }

    /// P-9.44: Eine andere OurAirports-Fassung (nur `herkunft` anders) aendert
    /// weder die Modul-Eingabe noch das Ergebnis — aber den Hash, denn die
    /// Herkunft ist Teil der Eingabe. Gegenprobe: eine andere Sinkrate in
    /// `eingang` aendert das Ergebnis.
    #[test]
    fn herkunft_dokumentiert_nur() {
        let herkunft = |commit: &str| serde_json::json!({ "bahn": { "quelle": { "art": "ourairports", "ourairports_commit": commit } } });
        let doc = |e: &LandingScoringInput, h: &serde_json::Value| {
            dokument("P1", 1, Art::Landung, None, e, h, &[], &crate::bewertung::bewerten(e))
        };
        let e = LandingScoringInput { vs_fpm: Some(-180.0), ..Default::default() };
        let a = doc(&e, &herkunft("aaaa111"));
        let b = doc(&e, &herkunft("bbbb222"));
        assert_ne!(a["eingabe"]["herkunft"], b["eingabe"]["herkunft"]);
        assert_eq!(a["eingabe"]["eingang"], b["eingabe"]["eingang"], "Modul-Eingabe unabhaengig von der Herkunft");
        assert_eq!(a["ergebnis"], b["ergebnis"], "Note unabhaengig von der Herkunft");
        assert_ne!(a["eingabe"]["meta"]["input_sha256"], b["eingabe"]["meta"]["input_sha256"], "Hash deckt die Herkunft");
        // Hash = SHA-256 ueber genau {eingang, herkunft} des Dokuments.
        let nach = serde_json::json!({ "eingang": a["eingabe"]["eingang"], "herkunft": a["eingabe"]["herkunft"] });
        assert_eq!(a["eingabe"]["meta"]["input_sha256"], hex(&Sha256::digest(nach.to_string().as_bytes())));
        // Gegenprobe: dieselbe Herkunft, andere Sinkrate → anderes Ergebnis.
        let hart = LandingScoringInput { vs_fpm: Some(-900.0), ..Default::default() };
        let c = doc(&hart, &herkunft("aaaa111"));
        assert_ne!(a["ergebnis"], c["ergebnis"], "Gegenprobe: eingang wirkt");
    }

    #[test]
    fn folge_der_antworten() {
        assert_eq!(folge(Some(200)), Folge::Loeschen);
        for s in [400, 403, 409, 413] {
            assert_eq!(folge(Some(s)), Folge::Ablehnen, "{s}");
        }
        for s in [Some(401), Some(500), Some(503), None] {
            assert_eq!(folge(s), Folge::Behalten, "{s:?}");
        }
    }

    #[test]
    fn einreihen_legt_gzip_datei_ab() {
        let ordner = std::env::temp_dir().join(format!("aeroacars-li-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ordner);
        let d = serde_json::json!({"a": 1});
        let p = einreihen(&ordner, "k1", "AB/../C", 2, &d).unwrap();
        assert_eq!(p.file_name().unwrap(), "ABC_2.json.gz");
        assert!(p.starts_with(ordner.join(ORDNER).join("k1")), "je Konto ein Ordner");
        assert_ne!(konto("token-a"), konto("token-b"));
        assert_eq!(konto("token-a").len(), 16);
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(std::fs::File::open(&p).unwrap()), &mut text).unwrap();
        assert_eq!(text, d.to_string());
    }
}
