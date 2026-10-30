//! v3 Etappe 3 (P-4.10, P-6.01, P-7.02, P-7.21): Eingabe-Dokument je
//! Aufsetzer an live (`POST /api/landing-input`).
//!
//! Das Dokument wird beim Abgeben gebaut und ZUERST auf die Platte gelegt
//! (Warteschlange `pending_uploads/eingabe/<konto>/`, P-7.21; `<konto>` =
//! Kurzhash der phpVMS-Pilot-ID — stabil, auch wenn das Token erneuert wird;
//! nach einem Kontowechsel warten fremde Dokumente, statt mit dem falschen
//! Token abgelehnt zu werden, Codex 30.10.), dann
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
/// Hoechste `seq`, die der Server annimmt (`landingInput.ts`: `seq > SEQ_MAX`
/// → 400 `seq_ungueltig`; Grenze dort auf 200 angehoben, 30.10.2026 — vorher
/// 50, das reicht fuer lange Platzrunden-Trainings nicht).
pub(crate) const SEQ_MAX: u32 = 200;

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
    let input_sha256 = input_sha256(eingang, herkunft);
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

/// `meta.input_sha256`: SHA-256 ueber `{"eingang": {"landung": …},
/// "herkunft": …}`. Eine Stelle fuer Dokument und MQTT-Nutzlasten (P-5.10).
pub(crate) fn input_sha256(eingang: &LandingScoringInput, herkunft: &serde_json::Value) -> String {
    let gehasht = serde_json::json!({ "eingang": { "landung": eingang }, "herkunft": herkunft });
    hex(&Sha256::digest(gehasht.to_string().as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn sicher(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

/// Aufbewahrung des Abgelehnten (offene Eintraege bleiben, P-7.21).
const AUFBEWAHREN: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

/// Kontoordner: Kurzhash der phpVMS-Pilot-ID (keine Kennung im Klartext,
/// stabil ueber Token-Erneuerungen — Codex 30.10.).
pub(crate) fn konto(pilot_id: &str) -> String {
    hex(&Sha256::digest(format!("pilot:{pilot_id}").as_bytes()))[..16].to_string()
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

/// Legt ein abgelehntes Dokument nach `abgelehnt/`. Bei Fehler bleibt es, wo
/// es ist.
fn nach_abgelehnt(ordner: &Path, pfad: &Path) -> std::io::Result<()> {
    let abgelehnt = ordner.join("abgelehnt");
    std::fs::create_dir_all(&abgelehnt)?;
    std::fs::rename(pfad, abgelehnt.join(pfad.file_name().unwrap_or_default()))
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

/// Sendet alle faelligen Dokumente des Kontos `konto` (des angemeldeten
/// Piloten; je Anfrage hoechstens `zeit`). Ohne Konto oder Pilot-Token
/// passiert nichts (die Dateien bleiben liegen). `trotz_abstand`: auch
/// Eintraege, deren naechster Termin noch nicht erreicht ist (der eine
/// Versuch unmittelbar vor dem PIREP, Codex 30.10.). Immer nur ein Durchlauf
/// gleichzeitig (Codex 29.10.).
pub(crate) async fn nachsenden(app_daten: PathBuf, konto: Option<String>, zeit: std::time::Duration, trotz_abstand: bool) {
    use crate::upload_warteschlange as uw;
    static NUR_EINER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _einer = NUR_EINER.lock().await;
    let (Some(konto), Some(token)) = (konto, crate::live_zugang::pilot_token()) else {
        return;
    };
    let ordner = uw::eingabe_ordner(&app_daten, &konto);
    aufraeumen(&ordner);
    uw::verwaiste_merker_raeumen(&ordner);
    let url = format!("{}/api/landing-input", crate::live_zugang::live_basis());
    let Ok(client) = reqwest::Client::builder().timeout(zeit).build() else {
        return;
    };
    for pfad in uw::eingaben(&app_daten, &konto) {
        if !trotz_abstand && !uw::faellig(&pfad, chrono::Utc::now().timestamp_millis()) {
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
                // Nur wenn das Verschieben gelingt, ist der Eintrag erledigt —
                // sonst bleibt er liegen (Codex 30.10.: kein Datenverlust).
                match nach_abgelehnt(&ordner, &pfad) {
                    Ok(()) => {
                        uw::erledigt(&pfad);
                        tracing::warn!(datei = %pfad.display(), ?status, "Eingabe-Dokument von live abgelehnt");
                    }
                    Err(e) => {
                        uw::fehlschlag(&pfad, chrono::Utc::now().timestamp_millis(), &format!("abgelehnt, nicht verschiebbar: {e}"));
                        tracing::warn!(datei = %pfad.display(), error = %e, "Abgelehntes Eingabe-Dokument nicht verschiebbar — bleibt liegen");
                    }
                }
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

/// P-4.44 / P-9.01: Note, die live nach einem Modulfehler im Client aus der
/// gespeicherten Eingabe nachgerechnet hat (`landing_inputs.output_json`,
/// `bewertung_status = nachbewertet`). Nur Anzeige: nichts wird gespeichert
/// oder neu gerechnet.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub(crate) struct Nachbewertung {
    pub seq: u64,
    pub gesamt: Option<f64>,
    pub gesamt_buchstabe: String,
    pub gesamt_wort: String,
}

const WOERTER: [&str; 5] = ["smooth", "acceptable", "firm", "hard", "severe"];
const BUCHSTABEN: [&str; 7] = ["A+", "A", "B+", "B", "C", "D", "F"];

/// Aus der Antwort von `GET /api/landing-input/:pirep_id` die nachbewertete
/// LANDUNG (nicht Touch-and-Go) mit der hoechsten `seq`; Wort und Buchstabe
/// muessen gueltig sein (wie `modulAus` der Webapp).
pub(crate) fn nachbewertung_aus(antwort: &serde_json::Value) -> Option<Nachbewertung> {
    antwort
        .get("landungen")?
        .as_array()?
        .iter()
        .filter(|l| l.get("nachbewertet").and_then(|n| n.as_bool()) == Some(true))
        .filter(|l| l.get("art").and_then(|a| a.as_str()) == Some("landung"))
        .filter_map(|l| {
            let e = l.get("ergebnis")?;
            let wort = e.get("gesamt_wort")?.as_str().filter(|w| WOERTER.contains(w))?;
            let buchstabe = e.get("gesamt_buchstabe")?.as_str().filter(|b| BUCHSTABEN.contains(b))?;
            Some(Nachbewertung {
                seq: l.get("seq")?.as_u64()?,
                gesamt: e.get("gesamt").and_then(|g| g.as_f64()).filter(|g| g.is_finite()),
                gesamt_buchstabe: buchstabe.to_string(),
                gesamt_wort: wort.to_string(),
            })
        })
        .max_by_key(|n| n.seq)
}

fn pirep_kennung_gueltig(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Die nachbewertete Note eines eigenen Flugs von live (Pilot-Zugang, Token
/// bleibt in Rust). `None`, wenn live keine hat, nicht erreichbar ist oder
/// kein Token vorliegt — die Anzeige bleibt dann, wie sie ist. Stufe
/// „fachlich“ (wie `live_inhalt`): Netz nur zu live, Kennung geprueft.
#[tauri::command]
pub(crate) async fn landung_nachbewertung(pirep_id: String) -> Result<Option<Nachbewertung>, crate::UiError> {
    if !pirep_kennung_gueltig(&pirep_id) {
        return Err(crate::UiError::new("pirep_id_ungueltig", "PIREP-Kennung ungueltig"));
    }
    let Some(token) = crate::live_zugang::pilot_token() else {
        return Ok(None);
    };
    let url = format!("{}/api/landing-input/{pirep_id}", crate::live_zugang::live_basis());
    let antwort = async {
        let c = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()?;
        let r = c.get(&url).bearer_auth(&token).send().await.ok()?;
        if !r.status().is_success() {
            tracing::debug!(status = r.status().as_u16(), "landing-input: keine Nachbewertung abrufbar");
            return None;
        }
        r.json::<serde_json::Value>().await.ok()
    }
    .await;
    Ok(antwort.as_ref().and_then(nachbewertung_aus))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nachbewertung_nur_landung_mit_gueltiger_ausgabe() {
        let antwort = serde_json::json!({ "pirep_id": "P1", "landungen": [
            { "seq": 1, "art": "touch_and_go", "nachbewertet": true,
              "ergebnis": { "gesamt": 70, "gesamt_wort": "firm", "gesamt_buchstabe": "C" } },
            { "seq": 2, "art": "landung", "nachbewertet": true,
              "ergebnis": { "gesamt": 88.0, "gesamt_wort": "acceptable", "gesamt_buchstabe": "B+" } },
            { "seq": 3, "art": "landung", "nachbewertet": false,
              "ergebnis": { "gesamt": 99, "gesamt_wort": "smooth", "gesamt_buchstabe": "A+" } }
        ]});
        assert_eq!(
            nachbewertung_aus(&antwort),
            Some(Nachbewertung { seq: 2, gesamt: Some(88.0), gesamt_buchstabe: "B+".into(), gesamt_wort: "acceptable".into() })
        );
        // ungueltiges Wort/Buchstabe zaehlt nicht (wie modulAus der Webapp)
        let kaputt = serde_json::json!({ "landungen": [
            { "seq": 2, "art": "landung", "nachbewertet": true, "ergebnis": { "gesamt_wort": "gut", "gesamt_buchstabe": "B+" } },
            { "seq": 3, "art": "landung", "nachbewertet": true, "ergebnis": { "gesamt_wort": "firm", "gesamt_buchstabe": "Z" } }
        ]});
        assert_eq!(nachbewertung_aus(&kaputt), None);
        assert_eq!(nachbewertung_aus(&serde_json::json!({ "landungen": [] })), None);
        // Nur das Touch-and-Go ist nachbewertet: das ist nicht die Note der Landung.
        let nur_tg = serde_json::json!({ "landungen": [
            { "seq": 1, "art": "touch_and_go", "nachbewertet": true,
              "ergebnis": { "gesamt": 70, "gesamt_wort": "firm", "gesamt_buchstabe": "C" } },
            { "seq": 2, "art": "landung", "nachbewertet": false,
              "ergebnis": { "gesamt": 90, "gesamt_wort": "smooth", "gesamt_buchstabe": "A" } }
        ]});
        assert_eq!(nachbewertung_aus(&nur_tg), None);
    }

    #[test]
    fn pirep_kennung_wird_geprueft() {
        assert!(pirep_kennung_gueltig("Ab12_x-9"));
        assert!(!pirep_kennung_gueltig(""));
        assert!(!pirep_kennung_gueltig("../admin"));
        assert!(!pirep_kennung_gueltig("a?b"));
        assert!(!pirep_kennung_gueltig(&"a".repeat(65)));
    }

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

    /// P-5.01: Die drei neuen Ereignisse werden an ihren Stellen geschrieben —
    /// `PaketStand` nach JEDEM `FlightStarted`, `AnflugPuffer` vor jedem
    /// `TouchdownComplete`, `BewertungEingabe` je eingereihtem Dokument.
    /// (Textwaechter ueber den Produktionsquelltext; die Formate prueft
    /// `recorder::v3_ereignis_tests`.)
    #[test]
    fn neue_ereignisse_stehen_an_ihren_stellen() {
        let src = crate::quelle_gesamt::PRODUKTION;
        let starts = src.matches("&FlightLogEvent::FlightStarted {").count();
        assert!(starts >= 3, "Flugstart-Stellen nicht gefunden");
        let mut rest = src;
        while let Some(i) = rest.find("&FlightLogEvent::FlightStarted {") {
            rest = &rest[i + 1..];
            let ende = rest.find("\n}\n").expect("Funktionsende");
            assert!(rest[..ende].contains("paket_stand_vermerken(&app, &flight.pirep_id);"), "Flugstart ohne PaketStand");
        }
        let td = src.find("&FlightLogEvent::TouchdownComplete {").expect("TouchdownComplete");
        let davor = &src[td.saturating_sub(600)..td];
        assert!(davor.contains("&FlightLogEvent::AnflugPuffer {"), "AnflugPuffer fehlt vor TouchdownComplete");
        let ein = src.find("landungs_eingabe::einreihen(&daten, &konto, &flight.pirep_id, seq, &doc)").expect("einreihen");
        assert!(src[ein..ein + 900].contains("&FlightLogEvent::BewertungEingabe {"), "BewertungEingabe fehlt beim Einreihen");
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

    /// Codex 30.10.: Laesst sich ein abgelehntes Dokument nicht verschieben
    /// (hier: `abgelehnt` ist eine Datei), bleibt es liegen. Gegenprobe: mit
    /// Ordner wird es verschoben.
    #[test]
    fn abgelehntes_geht_nie_verloren() {
        let ordner = std::env::temp_dir().join(format!("aeroacars-li-abg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&ordner);
        std::fs::create_dir_all(&ordner).unwrap();
        let doc = ordner.join("P1_1.json.gz");
        std::fs::write(&doc, b"x").unwrap();
        std::fs::write(ordner.join("abgelehnt"), b"keine Mappe").unwrap();
        assert!(nach_abgelehnt(&ordner, &doc).is_err());
        assert!(doc.exists(), "bleibt liegen");
        std::fs::remove_file(ordner.join("abgelehnt")).unwrap();
        nach_abgelehnt(&ordner, &doc).unwrap();
        assert!(!doc.exists() && ordner.join("abgelehnt").join("P1_1.json.gz").exists());
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
        assert_ne!(konto("7"), konto("8"));
        assert_eq!(konto("7").len(), 16);
        let mut text = String::new();
        std::io::Read::read_to_string(&mut flate2::read::GzDecoder::new(std::fs::File::open(&p).unwrap()), &mut text).unwrap();
        assert_eq!(text, d.to_string());
    }
}
