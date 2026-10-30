//! v3 Etappe 3 (P-4.10, P-6.01, P-7.02, P-7.21): Eingabe-Dokument je
//! Aufsetzer an live (`POST /api/landing-input`).
//!
//! Das Dokument wird beim Abgeben gebaut und ZUERST auf die Platte gelegt
//! (Warteschlange `pending_uploads/eingabe/<konto>/`, P-7.21; `<konto>` =
//! Kurzhash des Pilot-Tokens: nach einem Kontowechsel warten fremde
//! Dokumente, statt mit dem falschen Token abgelehnt zu werden), dann
//! gesendet. Ohne Aufsetzfenster gibt es kein Dokument (der Server verlangt
//! mindestens eine Probe). Immer nur EIN Sendevorgang; geloescht wird nur,
//! wenn die Datei noch genau das Gesendete enthaelt. Scheitert das Senden
//! (Netz, live nicht erreichbar, 5xx), bleibt die Datei liegen; der Arbeiter
//! (`upload_warteschlange`) versucht sie mit wachsendem Abstand erneut und
//! gibt nie auf. Endgueltige Antworten: 200 (auch „wiederholt“) loescht die
//! Datei; 400/403/409/413 legen sie nach `abgelehnt/` (Diagnose, kein
//! endloses Wiederholen; nach 30 Tagen aufgeraeumt).
//!
//! Vertrag (Server `recorder/src/landingInput.ts`): gzip-JSON
//! `{eingabe: {meta, eingang, herkunft, roh}, fenster: {proben}, ergebnis}`
//! (Kap. 5.2). `meta` nennt neben den Kennungen Sim, Simfassung, Profil,
//! `aircraft_icao` und Kategorie. `herkunft` (P-9.44) erklaert `eingang`;
//! `roh` (Anflugpuffer 120 + 400, Rollspur, METAR) dient Kontrolle und
//! Forensik (F-027). Das Fenster (17 Kanaele) steht als `fenster` daneben,
//! weil der Server es in eigener Spalte (`window_gz`) ablegt. Nichts davon
//! geht in die Rechnung ein: Modul und Server rechnen nur aus `eingang`.
//! `meta.input_sha256` = SHA-256 ueber `{"eingang": …, "herkunft": …}` —
//! die Herkunft ist Teil der Eingabe, `meta` (mit dem Hash selbst) und
//! `roh` (Forensik) nicht.

use std::io::Write;
use std::path::{Path, PathBuf};

use flate2::write::GzEncoder;
use landing_scoring::LandingScoringInput;
use recorder::TouchdownWindowSample;
use sha2::{Digest, Sha256};

use crate::bewertung::Status;

const SCHEMA: u32 = 1;
/// Hoechste `seq`, die der Server annimmt (`landingInput.ts`, `seq > 50`
/// → 400 `seq_ungueltig`).
pub(crate) const SEQ_MAX: u32 = 50;

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

/// Was je Aufsetzer neben Eingabe und Fenster mitgeht (Kap. 5.2), im selben
/// Augenblick gebaut (beim Touch-and-Go eingefroren, P-4.13). Reine
/// Dokumentation, nie in der Rechnung.
#[derive(Debug, Clone, Default)]
pub(crate) struct Begleitung {
    /// Zusatz zu `meta`: `sim`, `sim_fassung`, `profil`, `profil_variante`,
    /// `kategorie`, `aircraft_icao`.
    pub meta: serde_json::Value,
    pub herkunft: serde_json::Value,
    pub roh: serde_json::Value,
}

/// Das Dokument fuer `POST /api/landing-input`.
pub(crate) fn dokument(
    pirep_id: &str,
    seq: u32,
    art: Art,
    runtime_bundle_id: Option<String>,
    eingang: &LandingScoringInput,
    begleitung: &Begleitung,
    fenster: &[TouchdownWindowSample],
    ergebnis: &Option<(landing_scoring::wasm::Ausgabe, Status)>,
) -> serde_json::Value {
    let herkunft = &begleitung.herkunft;
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
    let mut meta = serde_json::json!({
        "pirep_id": pirep_id,
        "seq": seq,
        "art": art.text(),
        "input_schema": SCHEMA,
        "runtime_bundle_id": runtime_bundle_id,
        "input_sha256": input_sha256,
        "client_version": env!("CARGO_PKG_VERSION"),
    });
    // Die Kennungen oben gewinnen: der Zusatz traegt nur Sim, Profil & Co.
    if let (Some(m), Some(zusatz)) = (meta.as_object_mut(), begleitung.meta.as_object()) {
        for (k, v) in zusatz {
            m.entry(k.clone()).or_insert_with(|| v.clone());
        }
    }
    serde_json::json!({
        "eingabe": {
            "meta": meta,
            "eingang": eingang_json,
            "herkunft": herkunft,
            "roh": begleitung.roh,
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

/// Aufbewahrung des Abgelehnten (offene Eintraege bleiben, P-7.21).
const AUFBEWAHREN: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Kontoordner: Kurzhash des Pilot-Tokens (keine Kennung im Klartext).
pub(crate) fn konto(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))[..16].to_string()
}

/// Legt das Dokument gzip-gepackt in die Warteschlange des Kontos.
pub(crate) fn einreihen(app_daten: &Path, konto: &str, pirep_id: &str, seq: u32, doc: &serde_json::Value) -> std::io::Result<PathBuf> {
    let ordner = crate::upload_warteschlange::eingabe_ordner(app_daten, &sicher(konto));
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

/// Altes Abgelehntes aufraeumen. Offene Eintraege bleiben, egal wie alt
/// (P-7.21: gibt nie auf).
fn aufraeumen(ordner: &Path) {
    let Ok(es) = std::fs::read_dir(ordner.join("abgelehnt")) else { return };
    for e in es.flatten() {
        let alt = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
        if e.path().is_file() && alt.is_some_and(|a| a > AUFBEWAHREN) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Sendet alle faelligen Dokumente des angemeldeten Kontos (je Anfrage
/// hoechstens `zeit`). Ohne Pilot-Token passiert nichts (die Dateien bleiben
/// liegen). Immer nur ein Durchlauf gleichzeitig (Codex 29.10.).
pub(crate) async fn nachsenden(app_daten: PathBuf, zeit: std::time::Duration) {
    use crate::upload_warteschlange as uw;
    static NUR_EINER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _einer = NUR_EINER.lock().await;
    let Some(token) = crate::live_zugang::pilot_token() else {
        return;
    };
    let ordner = uw::eingabe_ordner(&app_daten, &konto(&token));
    aufraeumen(&ordner);
    uw::verwaiste_merker_raeumen(&ordner);
    let url = format!("{}/api/landing-input", crate::live_zugang::live_basis());
    let Ok(client) = reqwest::Client::builder().timeout(zeit).build() else {
        return;
    };
    for pfad in uw::eingaben(&app_daten, &konto(&token)) {
        if !uw::faellig(&pfad, chrono::Utc::now().timestamp_millis()) {
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
                uw::erledigt(&pfad);
                tracing::info!(datei = %pfad.display(), "Eingabe-Dokument an live uebergeben");
            }
            Folge::Ablehnen => {
                let abgelehnt = ordner.join("abgelehnt");
                let _ = std::fs::create_dir_all(&abgelehnt);
                if let Some(name) = pfad.file_name() {
                    let _ = std::fs::rename(&pfad, abgelehnt.join(name));
                }
                uw::erledigt(&pfad);
                tracing::warn!(datei = %pfad.display(), ?status, "Eingabe-Dokument von live abgelehnt");
            }
            Folge::Behalten => {
                let grund = match (&antwort, status) {
                    (_, Some(s)) => format!("HTTP {s}"),
                    (Err(e), None) => e.to_string(),
                    _ => "unbekannt".to_string(),
                };
                uw::fehlschlag(&pfad, chrono::Utc::now().timestamp_millis(), &grund);
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
        let b = Begleitung {
            meta: serde_json::json!({ "sim": "XPlane12", "kategorie": "FixedWing", "aircraft_icao": "A20N", "seq": 99 }),
            herkunft: h.clone(),
            roh: serde_json::json!({ "metar": "EDDF 301050Z 25008KT CAVOK 12/04 Q1021" }),
        };
        let d = dokument("PIREP1", 1, Art::Landung, Some("3-abcdef123456".into()), &e, &b, &[], &ergebnis);
        assert_eq!(d["eingabe"]["meta"]["seq"], 1);
        assert_eq!(d["eingabe"]["meta"]["art"], "landung");
        assert_eq!(d["eingabe"]["meta"]["input_sha256"].as_str().unwrap().len(), 64);
        assert_eq!(d["eingabe"]["eingang"]["landung"]["vs_fpm"], -200.0);
        assert_eq!(d["ergebnis"]["fassung"], 27);
        assert_eq!(d["ergebnis"]["bewertung_status"], "endgueltig");
        assert!(d["fenster"]["proben"].as_array().unwrap().is_empty());
        assert_eq!(d["eingabe"]["herkunft"], h);
        // Kap. 5.2: meta traegt den Zusatz, die Kennungen bleiben; roh steht da.
        assert_eq!(d["eingabe"]["meta"]["sim"], "XPlane12");
        assert_eq!(d["eingabe"]["meta"]["aircraft_icao"], "A20N");
        assert_eq!(d["eingabe"]["meta"]["kategorie"], "FixedWing");
        assert_eq!(d["eingabe"]["meta"]["seq"], 1, "Zusatz ueberschreibt keine Kennung");
        assert_eq!(d["eingabe"]["roh"], b.roh);
        let ohne = dokument("PIREP1", 1, Art::Landung, None, &e, &b, &[], &None);
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
            let b = Begleitung { herkunft: h.clone(), ..Default::default() };
            dokument("P1", 1, Art::Landung, None, e, &b, &[], &crate::bewertung::bewerten(e))
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

    /// Kap. 5.2: `meta`-Zusatz und `roh` sind Dokumentation und Forensik —
    /// zwei Dokumente, die sich nur darin unterscheiden, haben dieselbe
    /// Modul-Eingabe, dasselbe Ergebnis und denselben `input_sha256`.
    /// Gegenprobe: eine andere Sinkrate in `eingang` aendert Ergebnis und Hash.
    #[test]
    fn meta_und_roh_dokumentieren_nur() {
        let doc = |e: &LandingScoringInput, b: &Begleitung| {
            dokument("P1", 1, Art::Landung, None, e, b, &[], &crate::bewertung::bewerten(e))
        };
        let h = serde_json::json!({ "sinkrate": { "landing_source": "vs_at_impact" } });
        let b1 = Begleitung {
            meta: serde_json::json!({ "sim": "Msfs2024", "profil": "fenix_a320", "kategorie": "FixedWing", "aircraft_icao": "A320" }),
            herkunft: h.clone(),
            roh: serde_json::json!({ "anflugpuffer": { "note": [{ "vs_fpm": -700.0 }] }, "metar": "EDDF 301050Z 25008KT CAVOK" }),
        };
        let b2 = Begleitung {
            meta: serde_json::json!({ "sim": "XPlane12", "profil": "default", "kategorie": "Helicopter", "aircraft_icao": "H145" }),
            herkunft: h.clone(),
            roh: serde_json::json!({ "anflugpuffer": { "note": [{ "vs_fpm": -2400.0 }] }, "metar": "EDDF 301050Z 27035G50KT 0200 FG" }),
        };
        let e = LandingScoringInput { vs_fpm: Some(-180.0), ..Default::default() };
        let a = doc(&e, &b1);
        let b = doc(&e, &b2);
        assert_ne!(a["eingabe"]["meta"], b["eingabe"]["meta"]);
        assert_ne!(a["eingabe"]["roh"], b["eingabe"]["roh"]);
        assert_eq!(a["eingabe"]["eingang"], b["eingabe"]["eingang"], "Modul-Eingabe unabhaengig von meta/roh");
        assert_eq!(a["ergebnis"], b["ergebnis"], "Note unabhaengig von meta/roh");
        assert_eq!(a["eingabe"]["meta"]["input_sha256"], b["eingabe"]["meta"]["input_sha256"], "Hash ohne meta/roh");
        // Gegenprobe: andere Sinkrate → anderes Ergebnis, anderer Hash.
        let hart = LandingScoringInput { vs_fpm: Some(-900.0), ..Default::default() };
        let c = doc(&hart, &b1);
        assert_ne!(a["ergebnis"], c["ergebnis"], "Gegenprobe: eingang wirkt");
        assert_ne!(a["eingabe"]["meta"]["input_sha256"], c["eingabe"]["meta"]["input_sha256"]);
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

    /// P-7.21: offene Eintraege werden nie wegen ihres Alters geloescht,
    /// altes Abgelehntes schon (Gegenprobe).
    #[test]
    fn aufraeumen_laesst_offene_eintraege_stehen() {
        let ordner = std::env::temp_dir().join(format!("aeroacars-li-alt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ordner);
        std::fs::create_dir_all(ordner.join("abgelehnt")).unwrap();
        let alt = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600);
        for p in [ordner.join("P1_1.json.gz"), ordner.join("abgelehnt").join("P0_1.json.gz")] {
            std::fs::File::create(&p).unwrap().set_modified(alt).unwrap();
        }
        aufraeumen(&ordner);
        assert!(ordner.join("P1_1.json.gz").exists(), "offener Eintrag bleibt");
        assert!(!ordner.join("abgelehnt").join("P0_1.json.gz").exists(), "Gegenprobe: altes Abgelehntes geht");
    }

    #[test]
    fn einreihen_legt_gzip_datei_ab() {
        let ordner = std::env::temp_dir().join(format!("aeroacars-li-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ordner);
        let d = serde_json::json!({"a": 1});
        let p = einreihen(&ordner, "k1", "AB/../C", 2, &d).unwrap();
        assert_eq!(p.file_name().unwrap(), "ABC_2.json.gz");
        assert!(p.starts_with(ordner.join("pending_uploads").join("eingabe").join("k1")), "je Konto ein Ordner");
        assert_ne!(konto("token-a"), konto("token-b"));
        assert_eq!(konto("token-a").len(), 16);
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(std::fs::File::open(&p).unwrap()), &mut text).unwrap();
        assert_eq!(text, d.to_string());
    }
}
