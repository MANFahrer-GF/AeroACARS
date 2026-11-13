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
//! Datei; 400/403/413 legen sie nach `abgelehnt/` (Diagnose, kein
//! endloses Wiederholen; nach 30 Tagen aufgeraeumt); 409 (live hat fuer
//! `(pirep_id, seq)` schon eine andere Fassung) nach `konflikt/` — nie
//! geloescht, als Konflikt angezeigt (Codex 30.10.).
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

/// Kontoordner bis `8082a527`: Kurzhash des Pilot-Tokens. Nur, um dessen
/// Eintraege zu uebernehmen (`upload_warteschlange::konto_uebernehmen`).
fn alter_konto(token: &str) -> String {
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
    /// 409: andere Fassung bei live — aufbewahren und anzeigen, nicht senden.
    Konflikt,
    Behalten,
}

/// Antwort von live → Folge (reine Funktion, getestet).
pub(crate) fn folge(status: Option<u16>) -> Folge {
    match status {
        Some(200..=299) => Folge::Loeschen,
        Some(400 | 403 | 413) => Folge::Ablehnen,
        Some(409) => Folge::Konflikt,
        _ => Folge::Behalten,
    }
}

/// Legt ein abgelehntes Dokument nach `<unter>/` (`abgelehnt` oder
/// `konflikt`). Bei Fehler bleibt es, wo es ist.
fn ablegen(ordner: &Path, unter: &str, pfad: &Path) -> std::io::Result<()> {
    let ziel = ordner.join(unter);
    std::fs::create_dir_all(&ziel)?;
    std::fs::rename(pfad, ziel.join(pfad.file_name().unwrap_or_default()))
}

/// Altes Abgelehntes aufraeumen. Offene Eintraege und Konflikte bleiben,
/// egal wie alt (P-7.21: gibt nie auf).
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
    uw::konto_uebernehmen(&uw::eingabe_ordner(&app_daten, &alter_konto(&token)), &ordner);
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
            f @ (Folge::Ablehnen | Folge::Konflikt) => {
                // Nur wenn das Verschieben gelingt, ist der Eintrag erledigt —
                // sonst bleibt er liegen (Codex 30.10.: kein Datenverlust).
                let unter = if f == Folge::Konflikt { "konflikt" } else { "abgelehnt" };
                match ablegen(&ordner, unter, &pfad) {
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

/// Rolle des Gastgebers im Shared Cockpit.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum ScRolle {
    PF,
    PM,
}

/// Shared Cockpit (B4, Faehigkeit `sc-landung-1`): die Landung des
/// Gastgebers, wie live sie fuer den Partner fuehrt
/// (`GET /api/sharedcockpit/landung/:pirep_id`). Der Client rechnet nichts
/// nach, Client und Webapp zeigen dieselbe Landung aus einer Quelle. Die
/// eigene Messung des Partners bleibt unberuehrt im Flugprotokoll.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub(crate) enum ScLandung {
    Gelandet {
        touchdown_id: i64,
        fuehrender_pirep_id: String,
        gastgeber_name: Option<String>,
        rolle_gastgeber: Option<ScRolle>,
        /// `null`/fehlend, wo die Webapp keine Rate zeigt (alte 2.x-Landung,
        /// „nicht bewertbar“ ohne Simulator-Messung): Anzeige `--`, nie 0.
        landing_rate_fpm: Option<f64>,
        landing_score: Option<f64>,
        g_force: Option<f64>,
        bounce_count: Option<u32>,
        score_text: Option<String>,
    },
    /// Crew bekannt, der Gastgeber ist noch nicht gelandet.
    Ausstehend {
        fuehrender_pirep_id: Option<String>,
        gastgeber_name: Option<String>,
        rolle_gastgeber: Option<ScRolle>,
    },
    /// Kein Partner einer Crew (auch der Gastgeber selbst): Anzeige wie bisher.
    KeineCrew,
}

/// Antwort von live auswerten. 404 heisst „keine Crew“ (auch ein live ohne
/// diese Route: dann bleibt ebenfalls alles wie bisher). Alles andere, was
/// nicht dem Vertrag entspricht, ist `None` = Fehler.
pub(crate) fn sc_landung_aus(status: u16, body: &str) -> Option<ScLandung> {
    match status {
        200 => serde_json::from_str::<ScLandung>(body).ok().filter(|l| *l != ScLandung::KeineCrew),
        404 => Some(ScLandung::KeineCrew),
        _ => None,
    }
}

async fn sc_landung_holen(basis: &str, token: &str, pirep_id: &str) -> Option<ScLandung> {
    let c = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let r = c.get(format!("{basis}/api/sharedcockpit/landung/{pirep_id}")).bearer_auth(token).send().await.ok()?;
    let status = r.status().as_u16();
    let body = r.text().await.ok()?;
    sc_landung_aus(status, &body)
}

/// Shared Cockpit: Landung des Gastgebers fuer den eigenen Flug `pirep_id`
/// (Pilot-Zugang, Token bleibt in Rust). Fehler (kein Token, Netz, 5xx,
/// unbekannte Antwort) als `sc_landung_fehler`: die Oberflaeche bleibt dann,
/// wie sie ist.
///
/// Die Antwort landet zusaetzlich im Rust-Merker (Sim-Anzeigen,
/// Kennzeichen am Landungsdatensatz), damit beide dieselbe Antwort sehen;
/// zurueck kommt der massgebliche Stand aus dem Merker (`sc_ersetzt`).
#[tauri::command]
pub(crate) async fn sc_landung_gastgeber(app: tauri::AppHandle, pirep_id: String) -> Result<ScLandung, crate::UiError> {
    if !pirep_kennung_gueltig(&pirep_id) {
        return Err(crate::UiError::new("pirep_id_ungueltig", "PIREP-Kennung ungueltig"));
    }
    let fehler = || crate::UiError::new("sc_landung_fehler", "Shared-Cockpit-Landung nicht abrufbar");
    let token = crate::live_zugang::pilot_token().ok_or_else(fehler)?;
    let l = sc_landung_holen(crate::live_zugang::live_basis(), &token, &pirep_id).await.ok_or_else(fehler)?;
    let nach_td = sc_nach_td_fuer(&crate::remote::current_flight_status_value(&app), &pirep_id);
    Ok(sc_merker_setzen(&app, &pirep_id, l, nach_td))
}

// ── Shared Cockpit: Merker in Rust ──────────────────────────────────────
//
// Quelle fuer MSFS-Panel, Flow-Widget, Browser-HUD, Stream Deck (alle ueber
// `/panel/*`) und das X-Plane-Band, dazu fuer das Kennzeichen am
// Landungsdatensatz. Warum Rust selbst fragt (statt nur die Antwort der
// Oberflaeche zu uebernehmen): Die Oberflaeche fragt nur, solange die
// Landeanzeige offen ist; die Anzeigen im Simulator muessen auch stimmen,
// wenn der Pilot einen anderen Reiter offen hat. Beide Wege schreiben in
// denselben Merker, es bleibt eine Antwort von live (nichts wird gerechnet).
// HTTP laeuft nie im Anzeigetakt: Die Anzeigen lesen nur den Merker und
// stossen hoechstens einen Hintergrundabruf an.

/// Abstand der Nachfragen nach dem Aufsetzen, solange „ausstehend“.
const SC_ABSTAND: std::time::Duration = std::time::Duration::from_secs(10);
/// Hoechstens so viele Abrufe nach dem Aufsetzen (15 min), danach bleibt
/// es bei „Landung des Gastgebers folgt“, ohne die Anzeigen anzuhalten.
const SC_HOECHSTENS_NACH_TD: u32 = 90;
/// Abstand der Nachfragen, solange „gelandet“ noch ohne Note ist (die Note
/// setzt live erst mit dem PIREP des Gastgebers).
const SC_ABSTAND_NOTE: std::time::Duration = std::time::Duration::from_secs(30);
/// Hoechstens so viele Abrufe ohne Note (60 min), danach bleibt die Landung
/// ohne Note stehen.
const SC_HOECHSTENS_NOTE: u32 = 120;
/// Fehlversuche ohne jede Antwort, danach wie bisher (je vor und nach dem Aufsetzen).
const SC_FEHLVERSUCHE: u32 = 3;
/// Mehr Fluege behaelt der Merker nicht (aeltere fallen heraus).
const SC_MERKER_MAX: usize = 64;

#[derive(Debug, Clone, Default)]
pub(crate) struct ScEintrag {
    pub(crate) stand: Option<ScLandung>,
    /// Kam `stand` nach dem Aufsetzen (oder von der Oberflaeche)?
    pub(crate) stand_nach_td: bool,
    pub(crate) zuletzt: Option<std::time::Instant>,
    pub(crate) versuche_nach_td: u32,
    pub(crate) fehler: u32,
    pub(crate) laeuft: bool,
    pub(crate) td_gesehen: bool,
    /// Abrufe nach dem Aufsetzen, die „gelandet“ noch ohne Note brachten.
    pub(crate) versuche_note: u32,
    /// Laeuft der Nachlauf (fragt selbst weiter, auch ohne Anzeige)?
    nachlauf: bool,
    /// Zuletzt in den Landungsdatensatz geschriebenes Kennzeichen.
    im_datensatz: Option<serde_json::Value>,
}

/// Was die Anzeigen im Simulator nach dem Aufsetzen zeigen.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScAnzeige {
    /// Kein Partner, unbekannt oder Fehler: wie bisher.
    WieBisher,
    /// Erste Antwort steht nach dem Aufsetzen noch aus: anhalten, aber
    /// keine eigene Note zeigen.
    Warten,
    /// Partner, Gastgeber noch nicht gelandet. `halten`: Anzeigen bleiben in
    /// „Auswertung“, bis die Landung kommt (bis zur Hoechstzahl).
    Folgt { name: Option<String>, halten: bool },
    /// Partner, Gastgeber gelandet, live fuehrt aber noch keine Note (die
    /// kommt erst mit seinem PIREP). Anzeigen halten wie bei `Folgt`, bis die
    /// Note da ist (bis zur Hoechstzahl, danach `Gelandet`).
    NoteFolgt(ScLandung),
    /// Partner, Landung des Gastgebers von live.
    Gelandet(ScLandung),
}

/// Ist jetzt ein Abruf faellig? (Reine Entscheidung, getestet.)
pub(crate) fn sc_faellig(e: &ScEintrag, nach_td: bool, jetzt: std::time::Instant) -> bool {
    if e.laeuft {
        return false;
    }
    let alt = e.zuletzt.is_none_or(|z| jetzt.duration_since(z) >= SC_ABSTAND);
    match &e.stand {
        // Ohne Note langsamer weiter, bis die Note da ist (begrenzt).
        Some(ScLandung::Gelandet { .. }) => {
            nach_td && sc_wartet_auf_note(e) && e.zuletzt.is_none_or(|z| jetzt.duration_since(z) >= SC_ABSTAND_NOTE)
        }
        // Vor dem Aufsetzen kann live die Crew noch nicht kennen: einmal neu fragen.
        Some(ScLandung::KeineCrew) => nach_td && !e.stand_nach_td && alt,
        Some(ScLandung::Ausstehend { .. }) => nach_td && alt && e.versuche_nach_td < SC_HOECHSTENS_NACH_TD,
        None => e.fehler < SC_FEHLVERSUCHE && alt,
    }
}

/// Ersetzt `neu` den bisherigen Stand? (Rein, getestet.)
/// - „ausstehend“/„keine Crew“ ersetzt nie ein „gelandet“;
/// - dieselbe Landung (`touchdown_id`) mit Note wird nie durch eine Antwort
///   ohne Note ersetzt (verspaetete Antwort);
/// - sonst gilt die neuere Antwort: Note und angeglichene Sinkrate kommen
///   mit dem PIREP des Gastgebers, nach einem Touch-and-Go fuehrt live eine
///   andere `touchdown_id`. Eine ueberholte Antwort ohne Note heilt sich
///   selbst, weil ohne Note weiter gefragt wird.
/// Dieselbe Regel steht in `client/src/lib/scLandung.ts` (`scUebernehmen`).
pub(crate) fn sc_ersetzt(alt: Option<&ScLandung>, neu: &ScLandung) -> bool {
    match (alt, neu) {
        (Some(ScLandung::Gelandet { .. }), ScLandung::Ausstehend { .. } | ScLandung::KeineCrew) => false,
        (
            Some(ScLandung::Gelandet { touchdown_id: a, landing_score: Some(_), .. }),
            ScLandung::Gelandet { touchdown_id: b, landing_score: None, .. },
        ) => a != b,
        _ => true,
    }
}

/// „gelandet“, aber live fuehrt noch keine Note?
fn sc_ohne_note(l: &ScLandung) -> bool {
    matches!(l, ScLandung::Gelandet { landing_score: None, .. })
}

/// Wird fuer diesen Eintrag noch auf die Note gewartet (gefragt und gehalten)?
fn sc_wartet_auf_note(e: &ScEintrag) -> bool {
    e.stand.as_ref().is_some_and(sc_ohne_note) && e.versuche_note < SC_HOECHSTENS_NOTE
}

/// Antwort in den Eintrag uebernehmen (rein, getestet).
pub(crate) fn sc_uebernehmen(e: &mut ScEintrag, l: ScLandung, nach_td: bool) {
    if sc_ersetzt(e.stand.as_ref(), &l) {
        e.stand = Some(l);
        e.stand_nach_td = nach_td;
    }
    e.fehler = 0;
}

/// Ist der Flug `pirep_id` laut Flugstatus schon aufgesetzt? (Rein, getestet.)
/// Laeuft gerade ein anderer Flug oder keiner, ist dieser vorbei.
pub(crate) fn sc_nach_td_fuer(status: &serde_json::Value, pirep_id: &str) -> bool {
    if status.get("pirep_id").and_then(serde_json::Value::as_str) != Some(pirep_id) {
        return true;
    }
    crate::panel_server::sc_nach_aufsetzen(status)
}

/// Kennzeichen aus dem Merker (rein, getestet).
pub(crate) fn sc_kennzeichen_aus(
    m: &std::collections::HashMap<String, ScEintrag>,
    pirep_id: &str,
) -> Option<serde_json::Value> {
    sc_als_kennzeichen(m.get(pirep_id)?.stand.as_ref()?)
}

/// Anzeige aus dem Merker (reine Abbildung, getestet). Vor dem Aufsetzen
/// immer wie bisher.
pub(crate) fn sc_anzeige_aus(e: Option<&ScEintrag>, nach_td: bool) -> ScAnzeige {
    if !nach_td {
        return ScAnzeige::WieBisher;
    }
    let Some(e) = e else { return ScAnzeige::WieBisher };
    match &e.stand {
        Some(l @ ScLandung::Gelandet { .. }) if sc_wartet_auf_note(e) => ScAnzeige::NoteFolgt(l.clone()),
        Some(l @ ScLandung::Gelandet { .. }) => ScAnzeige::Gelandet(l.clone()),
        Some(ScLandung::Ausstehend { gastgeber_name, .. }) => ScAnzeige::Folgt {
            name: gastgeber_name.clone(),
            halten: e.versuche_nach_td < SC_HOECHSTENS_NACH_TD,
        },
        Some(ScLandung::KeineCrew) => ScAnzeige::WieBisher,
        // Nur die allererste Anfrage des Flugs haelt an (noch nie beantwortet
        // oder gescheitert); Fehler heissen „wie bisher“, auch wenn der
        // Abruf beim Aufsetzen neue Fehlversuche bekommt.
        None if e.laeuft && e.zuletzt.is_none() => ScAnzeige::Warten,
        None => ScAnzeige::WieBisher,
    }
}

fn sc_merker() -> std::sync::MutexGuard<'static, std::collections::HashMap<String, ScEintrag>> {
    static M: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, ScEintrag>>> =
        std::sync::LazyLock::new(Default::default);
    M.lock().unwrap_or_else(|e| e.into_inner())
}

/// Ordnet das Schreiben des Kennzeichens und das Speichern des Datensatzes
/// (`record_landing_for_filed_flight`), damit keins das andere ueberschreibt.
pub(crate) fn sc_speicher_sperre() -> std::sync::MutexGuard<'static, ()> {
    static S: std::sync::Mutex<()> = std::sync::Mutex::new(());
    S.lock().unwrap_or_else(|e| e.into_inner())
}

/// Anzeige fuer den laufenden Flug; stoesst bei Bedarf den Abruf an.
pub(crate) fn sc_anzeige(app: &tauri::AppHandle, pirep_id: &str, nach_td: bool) -> ScAnzeige {
    if pirep_id.is_empty() {
        return ScAnzeige::WieBisher;
    }
    sc_anstossen(app, pirep_id, nach_td);
    sc_anzeige_aus(sc_merker().get(pirep_id), nach_td)
}

/// Kennzeichen fuer den Landungsdatensatz (nur Partner).
pub(crate) fn sc_fuer_datensatz(pirep_id: &str) -> Option<serde_json::Value> {
    sc_kennzeichen_aus(&sc_merker(), pirep_id)
}

fn sc_als_kennzeichen(l: &ScLandung) -> Option<serde_json::Value> {
    match l {
        ScLandung::KeineCrew => None,
        _ => serde_json::to_value(l).ok(),
    }
}

/// Startet einen Hintergrundabruf, wenn er faellig ist.
pub(crate) fn sc_anstossen(app: &tauri::AppHandle, pirep_id: &str, nach_td: bool) {
    if !pirep_kennung_gueltig(pirep_id) {
        return;
    }
    {
        let mut m = sc_merker();
        if !m.contains_key(pirep_id) && m.len() >= SC_MERKER_MAX {
            m.clear();
        }
        let e = m.entry(pirep_id.to_string()).or_default();
        if nach_td && !e.td_gesehen {
            // Nach dem Aufsetzen neue Fehlversuche.
            e.td_gesehen = true;
            e.fehler = 0;
        }
        if !sc_faellig(e, nach_td, std::time::Instant::now()) {
            return;
        }
        e.laeuft = true;
    }
    let app = app.clone();
    let pirep = pirep_id.to_string();
    tauri::async_runtime::spawn(async move {
        let r = match crate::live_zugang::pilot_token() {
            Some(token) => sc_landung_holen(crate::live_zugang::live_basis(), &token, &pirep).await,
            None => None,
        };
        let ohne_zugang = crate::live_zugang::pilot_token().is_none();
        sc_eintragen(&mut sc_merker(), &pirep, r.clone(), nach_td, ohne_zugang);
        if let Some(l) = r {
            sc_datensatz_nachtragen(&app, &pirep, &l);
        }
        sc_nachlauf(&app, &pirep);
    });
}

/// Solange „gelandet“ ohne Note: selbst weiterfragen, auch wenn keine Anzeige
/// mehr fragt (nach der Abgabe des Partners ist der Flugstatus leer, die Note
/// kommt erst mit dem PIREP des Gastgebers). Hoechstens ein Nachlauf je Flug,
/// begrenzt durch `SC_HOECHSTENS_NOTE`; der Datensatz bekommt die Note ueber
/// `sc_datensatz_nachtragen`.
fn sc_nachlauf(app: &tauri::AppHandle, pirep_id: &str) {
    {
        let mut m = sc_merker();
        let Some(e) = m.get_mut(pirep_id) else { return };
        if e.nachlauf || !sc_wartet_auf_note(e) {
            return;
        }
        e.nachlauf = true;
    }
    let app = app.clone();
    let pirep = pirep_id.to_string();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(SC_ABSTAND_NOTE + std::time::Duration::from_secs(1)).await;
            if !sc_merker().get(&pirep).is_some_and(sc_wartet_auf_note) {
                break;
            }
            sc_anstossen(&app, &pirep, true);
        }
        if let Some(e) = sc_merker().get_mut(&pirep) {
            e.nachlauf = false;
        }
    });
}

/// Ergebnis eines eigenen Abrufs eintragen (rein, getestet).
pub(crate) fn sc_eintragen(
    m: &mut std::collections::HashMap<String, ScEintrag>,
    pirep_id: &str,
    r: Option<ScLandung>,
    nach_td: bool,
    ohne_zugang: bool,
) {
    let e = m.entry(pirep_id.to_string()).or_default();
    e.laeuft = false;
    e.zuletzt = Some(std::time::Instant::now());
    if nach_td {
        e.versuche_nach_td += 1;
    }
    match r {
        Some(l) => {
            sc_uebernehmen(e, l, nach_td);
            if nach_td && e.stand.as_ref().is_some_and(sc_ohne_note) {
                e.versuche_note += 1;
            }
        }
        // Ohne Pilot-Zugang gibt es nichts zu fragen: gleich wie bisher.
        None if ohne_zugang => e.fehler = SC_FEHLVERSUCHE,
        None => e.fehler += 1,
    }
}

/// Antwort der Oberflaeche uebernehmen; `nach_td` aus dem echten Flugstatus
/// (`sc_nach_td_fuer`). Gibt den massgeblichen Stand zurueck (nach
/// `sc_ersetzt`), damit Oberflaeche und Sim-Anzeigen dieselbe Antwort sehen.
fn sc_merker_setzen(app: &tauri::AppHandle, pirep_id: &str, l: ScLandung, nach_td: bool) -> ScLandung {
    let stand = {
        let mut m = sc_merker();
        if !m.contains_key(pirep_id) && m.len() >= SC_MERKER_MAX {
            m.clear();
        }
        let e = m.entry(pirep_id.to_string()).or_default();
        sc_uebernehmen(e, l.clone(), nach_td);
        e.stand.clone().unwrap_or(l)
    };
    sc_datensatz_nachtragen(app, pirep_id, &stand);
    sc_nachlauf(app, pirep_id);
    stand
}

/// Kennzeichen am gespeicherten Datensatz nachtragen, nur dieses Feld. Fehlt
/// der Datensatz (Flug noch nicht abgegeben), setzt ihn das Speichern.
fn sc_datensatz_nachtragen(app: &tauri::AppHandle, pirep_id: &str, l: &ScLandung) {
    let _s = sc_speicher_sperre();
    // Massgeblich ist der Merker (ein „gelandet“ bleibt, auch wenn diese
    // Antwort aelter ist).
    let Some(neu) = sc_fuer_datensatz(pirep_id).or_else(|| sc_als_kennzeichen(l)) else { return };
    {
        let m = sc_merker();
        if m.get(pirep_id).and_then(|e| e.im_datensatz.as_ref()) == Some(&neu) {
            return;
        }
    }
    if let Some(store) = crate::open_landing_store(app) {
        if let Ok(Some(rec)) = store.get(pirep_id) {
            if let Some(rec) = mit_kennzeichen(rec, &neu) {
                if let Err(e) = store.upsert(rec) {
                    tracing::warn!(error = ?e, "Shared-Cockpit-Kennzeichen nicht gespeichert");
                    return;
                }
            }
        }
    }
    if let Some(e) = sc_merker().get_mut(pirep_id) {
        e.im_datensatz = Some(neu);
    }
}

/// Neuer Datensatz mit Kennzeichen, oder `None`, wenn es schon so drinsteht.
/// Messfelder bleiben unberuehrt.
pub(crate) fn mit_kennzeichen(mut rec: storage::LandingRecord, neu: &serde_json::Value) -> Option<storage::LandingRecord> {
    if rec.shared_cockpit.as_ref() == Some(neu) {
        return None;
    }
    rec.shared_cockpit = Some(neu.clone());
    Some(rec)
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
    fn sc_landung_alle_antworten_nach_vertrag() {
        let gelandet = r#"{"status":"gelandet","touchdown_id":4711,"fuehrender_pirep_id":"H1","gastgeber_name":"Anna B.",
            "rolle_gastgeber":"PF","landing_rate_fpm":-200.03,"landing_score":83,"g_force":1.14,"bounce_count":0,"score_text":"acceptable"}"#;
        assert_eq!(
            sc_landung_aus(200, gelandet),
            Some(ScLandung::Gelandet {
                touchdown_id: 4711,
                fuehrender_pirep_id: "H1".into(),
                gastgeber_name: Some("Anna B.".into()),
                rolle_gastgeber: Some(ScRolle::PF),
                landing_rate_fpm: Some(-200.03),
                landing_score: Some(83.0),
                g_force: Some(1.14),
                bounce_count: Some(0),
                score_text: Some("acceptable".into()),
            })
        );
        // Nullbare Felder duerfen null sein oder fehlen.
        let knapp = r#"{"status":"gelandet","touchdown_id":1,"fuehrender_pirep_id":"H1","gastgeber_name":null,"rolle_gastgeber":null,"landing_rate_fpm":-90,"landing_score":null}"#;
        assert!(matches!(sc_landung_aus(200, knapp), Some(ScLandung::Gelandet { gastgeber_name: None, g_force: None, .. })));
        // Rate `null` oder fehlend (Webapp zeigt keine): trotzdem Gastgeber-Landung.
        for ohne_rate in [
            r#"{"status":"gelandet","touchdown_id":2,"fuehrender_pirep_id":"H1","landing_rate_fpm":null,"landing_score":70}"#,
            r#"{"status":"gelandet","touchdown_id":2,"fuehrender_pirep_id":"H1","landing_score":70}"#,
        ] {
            assert!(
                matches!(sc_landung_aus(200, ohne_rate), Some(ScLandung::Gelandet { landing_rate_fpm: None, landing_score: Some(_), .. })),
                "{ohne_rate}"
            );
        }
        let aus = r#"{"status":"ausstehend","fuehrender_pirep_id":"H1","gastgeber_name":"Anna B.","rolle_gastgeber":"PM"}"#;
        assert_eq!(
            sc_landung_aus(200, aus),
            Some(ScLandung::Ausstehend {
                fuehrender_pirep_id: Some("H1".into()),
                gastgeber_name: Some("Anna B.".into()),
                rolle_gastgeber: Some(ScRolle::PM),
            })
        );
        assert_eq!(sc_landung_aus(200, r#"{"status":"ausstehend","fuehrender_pirep_id":null,"gastgeber_name":null,"rolle_gastgeber":null}"#),
            Some(ScLandung::Ausstehend { fuehrender_pirep_id: None, gastgeber_name: None, rolle_gastgeber: None }));
        assert_eq!(sc_landung_aus(404, r#"{"error":"keine_crew"}"#), Some(ScLandung::KeineCrew));
        // Fehler: 5xx, 401, kaputte oder vertragsfremde Antworten.
        assert_eq!(sc_landung_aus(500, gelandet), None);
        assert_eq!(sc_landung_aus(401, ""), None);
        assert_eq!(sc_landung_aus(200, "kein json"), None);
        assert_eq!(sc_landung_aus(200, r#"{"status":"keine_crew"}"#), None);
        // ohne `touchdown_id` bzw. `fuehrender_pirep_id` (Pflicht); die Rate darf fehlen (oben)
        assert_eq!(sc_landung_aus(200, r#"{"status":"gelandet","fuehrender_pirep_id":"H1","landing_rate_fpm":-90}"#), None);
        assert_eq!(sc_landung_aus(200, r#"{"status":"gelandet","touchdown_id":1,"landing_rate_fpm":-90}"#), None);
        assert_eq!(sc_landung_aus(200, r#"{"status":"ausstehend","rolle_gastgeber":"XX"}"#), None);
        // Serialisiert fuer die Oberflaeche mit demselben Feld `status`.
        assert_eq!(serde_json::to_value(ScLandung::KeineCrew).unwrap(), serde_json::json!({ "status": "keine_crew" }));
    }

    #[tokio::test]
    async fn sc_landung_fragt_live_mit_bearer_und_eigener_nummer() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let basis = format!("http://{}", l.local_addr().unwrap());
        let h = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut roh = Vec::new();
            let mut puffer = [0u8; 4096];
            while !roh.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = s.read(&mut puffer).await.unwrap();
                roh.extend_from_slice(&puffer[..n]);
            }
            let body = r#"{"status":"ausstehend","fuehrender_pirep_id":"H1","gastgeber_name":"Anna B.","rolle_gastgeber":"PF"}"#;
            let antwort = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            s.write_all(antwort.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&roh).to_ascii_lowercase()
        });
        let r = sc_landung_holen(&basis, "T0K", "P9902").await;
        assert!(matches!(r, Some(ScLandung::Ausstehend { .. })));
        let anfrage = h.await.unwrap();
        assert!(anfrage.starts_with("get /api/sharedcockpit/landung/p9902 "), "{anfrage}");
        assert!(anfrage.contains("authorization: bearer t0k"), "{anfrage}");
        // niemand hoert zu: Fehler, kein Absturz
        let tot = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let a = l.local_addr().unwrap();
            drop(l);
            format!("http://{a}")
        };
        assert_eq!(sc_landung_holen(&tot, "T", "P1").await, None);
    }

    fn gelandet(name: &str) -> ScLandung {
        ScLandung::Gelandet {
            touchdown_id: 9,
            fuehrender_pirep_id: "H1".into(),
            gastgeber_name: Some(name.into()),
            rolle_gastgeber: Some(ScRolle::PF),
            landing_rate_fpm: Some(-180.0),
            landing_score: Some(91.0),
            g_force: Some(1.12),
            bounce_count: Some(0),
            score_text: Some("smooth".into()),
        }
    }

    fn ausstehend() -> ScLandung {
        ScLandung::Ausstehend { fuehrender_pirep_id: Some("H1".into()), gastgeber_name: Some("Anna B.".into()), rolle_gastgeber: None }
    }

    /// Partnerflug: einmal im Flug fragen, nach dem Aufsetzen alle 10 s bis
    /// „gelandet“, dann Schluss. Die Anzeige folgt dem Merker.
    #[test]
    fn sc_merker_partner_ablauf() {
        use std::collections::HashMap;
        use std::time::{Duration, Instant};
        let mut m: HashMap<String, ScEintrag> = HashMap::new();
        let t0 = Instant::now();
        // leer: sofort faellig (auch vor dem Aufsetzen), vorher wie bisher
        assert!(sc_faellig(&ScEintrag::default(), false, t0));
        assert_eq!(sc_anzeige_aus(None, true), ScAnzeige::WieBisher);
        // erste Anfrage nach dem Aufsetzen laeuft: anhalten, keine eigene Note
        let laeuft = ScEintrag { laeuft: true, ..Default::default() };
        assert_eq!(sc_anzeige_aus(Some(&laeuft), true), ScAnzeige::Warten);
        assert!(!sc_faellig(&laeuft, true, t0), "nie zwei Abrufe gleichzeitig");
        assert_eq!(sc_anzeige_aus(Some(&laeuft), false), ScAnzeige::WieBisher, "vor dem Aufsetzen nichts anhalten");

        sc_eintragen(&mut m, "P1", Some(ausstehend()), false, false);
        let e = &m["P1"];
        assert!(!sc_faellig(e, false, t0 + Duration::from_secs(60)), "im Flug reicht eine Antwort");
        assert!(sc_faellig(e, true, t0 + Duration::from_secs(60)), "nach dem Aufsetzen sofort neu");
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::Folgt { name: Some("Anna B.".into()), halten: true });

        sc_eintragen(&mut m, "P1", Some(ausstehend()), true, false);
        let e = &m["P1"];
        let z = e.zuletzt.unwrap();
        assert!(!sc_faellig(e, true, z + Duration::from_secs(9)));
        assert!(sc_faellig(e, true, z + SC_ABSTAND));

        sc_eintragen(&mut m, "P1", Some(gelandet("Anna B.")), true, false);
        let e = &m["P1"];
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::Gelandet(gelandet("Anna B.")));
        assert!(!sc_faellig(e, true, z + Duration::from_secs(3600)), "gelandet beendet");
        // ein verspaetetes „ausstehend“ macht „gelandet“ nicht rueckgaengig
        sc_eintragen(&mut m, "P1", Some(ausstehend()), true, false);
        assert!(matches!(m["P1"].stand, Some(ScLandung::Gelandet { .. })));
    }

    /// Nach der Hoechstzahl bleibt „folgt“, die Anzeigen werden aber nicht
    /// mehr angehalten.
    #[test]
    fn sc_merker_gibt_nach_hoechstzahl_das_anhalten_auf() {
        let mut m = std::collections::HashMap::new();
        for _ in 0..SC_HOECHSTENS_NACH_TD {
            sc_eintragen(&mut m, "P1", Some(ausstehend()), true, false);
        }
        let e = &m["P1"];
        assert!(!sc_faellig(e, true, std::time::Instant::now() + SC_ABSTAND * 2));
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::Folgt { name: Some("Anna B.".into()), halten: false });
    }

    /// Gegenprobe Nicht-Crew und Fehler: Anzeige exakt wie bisher.
    #[test]
    fn sc_merker_keine_crew_und_fehler_wie_bisher() {
        use std::time::Instant;
        let mut m = std::collections::HashMap::new();
        sc_eintragen(&mut m, "K1", Some(ScLandung::KeineCrew), false, false);
        let e = &m["K1"];
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::WieBisher);
        let spaeter = e.zuletzt.unwrap() + SC_ABSTAND;
        assert!(sc_faellig(e, true, spaeter), "404 im Flug: nach dem Aufsetzen einmal neu fragen");
        sc_eintragen(&mut m, "K1", Some(ScLandung::KeineCrew), true, false);
        assert!(!sc_faellig(&m["K1"], true, Instant::now() + SC_ABSTAND * 10), "danach Schluss");
        assert_eq!(sc_anzeige_aus(Some(&m["K1"]), true), ScAnzeige::WieBisher);

        for _ in 0..SC_FEHLVERSUCHE {
            sc_eintragen(&mut m, "F1", None, true, false);
        }
        assert_eq!(sc_anzeige_aus(Some(&m["F1"]), true), ScAnzeige::WieBisher);
        assert!(!sc_faellig(&m["F1"], true, Instant::now() + SC_ABSTAND * 10));
        // ohne Pilot-Zugang: sofort aufgeben
        sc_eintragen(&mut m, "Z1", None, true, true);
        assert!(!sc_faellig(&m["Z1"], true, Instant::now() + SC_ABSTAND * 10));
    }

    fn landung(touchdown_id: i64, rate: f64, note: Option<f64>) -> ScLandung {
        ScLandung::Gelandet {
            touchdown_id,
            fuehrender_pirep_id: "H1".into(),
            gastgeber_name: Some("Anna B.".into()),
            rolle_gastgeber: Some(ScRolle::PF),
            landing_rate_fpm: Some(rate),
            landing_score: note,
            g_force: Some(1.3),
            bounce_count: Some(0),
            score_text: Some("firm".into()),
        }
    }

    /// Befund 1: live legt die Landung beim Aufsetzen ohne Note an und setzt
    /// Note und angeglichene Sinkrate erst mit dem PIREP des Gastgebers.
    /// „gelandet ohne Note“ fragt deshalb (langsamer) weiter, die Anzeigen
    /// halten, und die Antwort mit Note ersetzt die alte im Merker und im
    /// Kennzeichen. Eine verspaetete Antwort ohne Note nimmt sie nicht weg.
    #[test]
    fn sc_merker_gelandet_ohne_note_fragt_bis_zur_note() {
        use std::collections::HashMap;
        use std::time::Duration;
        let mut m: HashMap<String, ScEintrag> = HashMap::new();
        sc_eintragen(&mut m, "P1", Some(ausstehend()), true, false);
        sc_eintragen(&mut m, "P1", Some(landung(7, -310.0, None)), true, false);
        let e = &m["P1"];
        let z = e.zuletzt.unwrap();
        assert!(!sc_faellig(e, true, z + SC_ABSTAND), "langsamerer Takt als bei ausstehend");
        assert!(sc_faellig(e, true, z + SC_ABSTAND_NOTE), "ohne Note weiterfragen");
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::NoteFolgt(landung(7, -310.0, None)));
        assert_eq!(sc_kennzeichen_aus(&m, "P1").unwrap()["landing_score"], serde_json::Value::Null);

        sc_eintragen(&mut m, "P1", Some(landung(7, -295.0, Some(83.0))), true, false);
        let e = &m["P1"];
        assert_eq!(e.stand, Some(landung(7, -295.0, Some(83.0))), "Note und angeglichene Sinkrate");
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::Gelandet(landung(7, -295.0, Some(83.0))));
        assert!(!sc_faellig(e, true, z + Duration::from_secs(3600)), "mit Note Schluss");
        let k = sc_kennzeichen_aus(&m, "P1").unwrap();
        assert_eq!(k["landing_score"], 83.0, "der Datensatz bekommt die Note");
        assert_eq!(k["landing_rate_fpm"], -295.0);

        // verspaetete Antworten ohne Note bzw. „ausstehend“ nehmen nichts weg
        sc_eintragen(&mut m, "P1", Some(landung(7, -310.0, None)), true, false);
        sc_eintragen(&mut m, "P1", Some(ausstehend()), true, false);
        assert_eq!(m["P1"].stand, Some(landung(7, -295.0, Some(83.0))));
    }

    /// Befund 1, Touch-and-Go: der erste Aufsetzer kommt ohne Note, die
    /// eigentliche Landung hat eine neue `touchdown_id` und ersetzt ihn, auf
    /// beiden Wegen (eigener Abruf und Antwort der Oberflaeche).
    #[test]
    fn sc_merker_neue_touchdown_id_ersetzt_den_ersten_aufsetzer() {
        let mut m = std::collections::HashMap::new();
        sc_eintragen(&mut m, "P1", Some(landung(7, -150.0, None)), true, false);
        sc_eintragen(&mut m, "P1", Some(landung(8, -220.0, None)), true, false);
        assert_eq!(m["P1"].stand, Some(landung(8, -220.0, None)));
        sc_eintragen(&mut m, "P1", Some(landung(8, -215.0, Some(77.0))), true, false);
        assert_eq!(sc_kennzeichen_aus(&m, "P1").unwrap()["touchdown_id"], 8);

        let mut e = ScEintrag::default();
        sc_uebernehmen(&mut e, landung(7, -150.0, None), true);
        sc_uebernehmen(&mut e, landung(8, -215.0, Some(77.0)), true);
        assert_eq!(e.stand, Some(landung(8, -215.0, Some(77.0))));
        // gleiche Landung mit Note bleibt gegen eine aeltere ohne Note
        sc_uebernehmen(&mut e, landung(8, -220.0, None), true);
        assert_eq!(e.stand, Some(landung(8, -215.0, Some(77.0))));
    }

    /// Nach der Hoechstzahl ohne Note: Schluss mit Fragen und Halten, die
    /// Landung bleibt ohne Note stehen.
    #[test]
    fn sc_merker_ohne_note_gibt_nach_hoechstzahl_auf() {
        let mut m = std::collections::HashMap::new();
        for _ in 0..SC_HOECHSTENS_NOTE {
            sc_eintragen(&mut m, "P1", Some(landung(7, -310.0, None)), true, false);
        }
        let e = &m["P1"];
        assert!(!sc_faellig(e, true, std::time::Instant::now() + SC_ABSTAND_NOTE * 2));
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::Gelandet(landung(7, -310.0, None)));
    }

    /// Befund 2: Die Oberflaeche fragt schon beim Flugstart; ein fruehes 404
    /// gilt nicht als „nach dem Aufsetzen“, Rust fragt nach dem Aufsetzen neu.
    #[test]
    fn sc_merker_fruehes_404_der_oberflaeche_wird_nach_dem_aufsetzen_neu_gefragt() {
        let im_flug = serde_json::json!({ "pirep_id": "P1", "phase": "cruise", "landing_at": null });
        let gelandet = serde_json::json!({ "pirep_id": "P1", "phase": "landing", "landing_at": "2026-11-13T10:00:00Z" });
        assert!(!sc_nach_td_fuer(&im_flug, "P1"));
        assert!(sc_nach_td_fuer(&gelandet, "P1"));
        assert!(sc_nach_td_fuer(&serde_json::Value::Null, "P1"), "kein Flug mehr: vorbei");
        assert!(sc_nach_td_fuer(&serde_json::json!({ "pirep_id": "P2", "landing_at": null }), "P1"), "anderer Flug laeuft");

        let mut e = ScEintrag::default();
        sc_uebernehmen(&mut e, ScLandung::KeineCrew, sc_nach_td_fuer(&im_flug, "P1"));
        assert!(sc_faellig(&e, true, std::time::Instant::now()), "nach dem Aufsetzen neu fragen");
        // Gegenprobe: 404 nach dem Aufsetzen beendet
        let mut e = ScEintrag::default();
        sc_uebernehmen(&mut e, ScLandung::KeineCrew, sc_nach_td_fuer(&gelandet, "P1"));
        assert!(!sc_faellig(&e, true, std::time::Instant::now()));
    }

    /// Befund 3: Soloflug, live haengt. Die Anfrage im Flug ist schon
    /// gescheitert; der Abruf beim Aufsetzen haelt die Anzeigen nicht an.
    /// Gegenprobe: die allererste Anfrage ueberhaupt haelt an.
    #[test]
    fn sc_merker_warten_nur_vor_der_ersten_antwort() {
        let mut m = std::collections::HashMap::new();
        sc_eintragen(&mut m, "S1", None, false, false);
        let e = m.get_mut("S1").unwrap();
        // wie `sc_anstossen` beim Aufsetzen
        e.td_gesehen = true;
        e.fehler = 0;
        e.laeuft = true;
        assert_eq!(sc_anzeige_aus(Some(e), true), ScAnzeige::WieBisher);
        let erste = ScEintrag { laeuft: true, td_gesehen: true, ..Default::default() };
        assert_eq!(sc_anzeige_aus(Some(&erste), true), ScAnzeige::Warten);
    }

    /// Das Kennzeichen kommt dazu, die eigene Messung bleibt; gleiches
    /// Kennzeichen schreibt nicht erneut; KeineCrew gibt kein Kennzeichen.
    #[test]
    fn sc_kennzeichen_am_datensatz_ohne_messwerte_anzufassen() {
        let rec: storage::LandingRecord = serde_json::from_value(serde_json::json!({
            "pirep_id": "P1", "touchdown_at": "2026-11-13T10:00:00Z", "recorded_at": "2026-11-13T10:30:00Z",
            "flight_number": "1", "airline_icao": "GSG", "dpt_airport": "EDDF", "arr_airport": "EDDM",
            "score_numeric": 55, "score_label": "firm", "grade_letter": "D", "landing_rate_fpm": -480.0,
            "bounce_count": 1, "touchdown_profile": [], "approach_samples": [], "ux_version": 1,
            "forensics_version": 1, "sub_scores": [], "accident": false, "accident_reasons": []
        }))
        .unwrap();
        let neu = sc_als_kennzeichen(&gelandet("Anna B.")).unwrap();
        assert_eq!(neu["status"], "gelandet");
        let mit = mit_kennzeichen(rec, &neu).expect("neues Kennzeichen");
        assert_eq!(mit.shared_cockpit.as_ref(), Some(&neu));
        assert_eq!(mit.landing_rate_fpm, Some(-480.0));
        assert_eq!(mit.score_numeric, Some(55));
        assert_eq!(mit.bounce_count, 1);
        assert!(mit_kennzeichen(mit, &neu).is_none(), "schon drin: nicht neu schreiben");
        assert_eq!(sc_als_kennzeichen(&ScLandung::KeineCrew), None);
        assert_eq!(sc_als_kennzeichen(&ausstehend()).unwrap()["status"], "ausstehend");
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
        for s in [400, 403, 413] {
            assert_eq!(folge(Some(s)), Folge::Ablehnen, "{s}");
        }
        assert_eq!(folge(Some(409)), Folge::Konflikt);
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
        assert!(ablegen(&ordner, "abgelehnt", &doc).is_err());
        assert!(doc.exists(), "bleibt liegen");
        std::fs::remove_file(ordner.join("abgelehnt")).unwrap();
        ablegen(&ordner, "abgelehnt", &doc).unwrap();
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

    /// Codex 30.10.: 409 (andere Fassung bei live) liegt unter `konflikt/`,
    /// bleibt nach 31 Tagen stehen, wird als Konflikt angezeigt und nicht
    /// erneut gesendet. Gegenprobe: altes Abgelehntes (400) geht.
    #[test]
    fn konflikt_bleibt_sichtbar_und_wird_nie_geloescht() {
        use crate::upload_warteschlange as uw;
        let d = std::env::temp_dir().join(format!("aeroacars-li-konflikt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let ordner = uw::eingabe_ordner(&d, "k1");
        std::fs::create_dir_all(&ordner).unwrap();
        let (k, a) = (ordner.join("P1_2.json.gz"), ordner.join("P0_1.json.gz"));
        for p in [&k, &a] {
            std::fs::write(p, b"x").unwrap();
        }
        ablegen(&ordner, "konflikt", &k).unwrap();
        ablegen(&ordner, "abgelehnt", &a).unwrap();
        let alt = std::time::SystemTime::now() - std::time::Duration::from_secs(31 * 24 * 3600);
        for p in [ordner.join("konflikt").join("P1_2.json.gz"), ordner.join("abgelehnt").join("P0_1.json.gz")] {
            std::fs::File::options().write(true).open(&p).unwrap().set_modified(alt).unwrap();
        }
        aufraeumen(&ordner);
        assert!(ordner.join("konflikt").join("P1_2.json.gz").exists(), "Konflikt bleibt");
        assert!(!ordner.join("abgelehnt").join("P0_1.json.gz").exists(), "Gegenprobe: altes Abgelehntes geht");
        assert!(uw::eingaben(&d, "k1").is_empty(), "nicht erneut gesendet");
        assert!(uw::eingaben_offen(&d, Some("k1")).is_empty(), "haelt das Flugprotokoll nicht auf");
        let v = uw::ausstehend(&d, Some("k1"), None);
        assert_eq!(v.iter().map(|x| (x.pirep_id.as_str(), x.konflikt)).collect::<Vec<_>>(), [("P1", true)]);
        assert!(uw::ausstehend(&d, Some("k2"), None).is_empty(), "fremdes Konto sieht ihn nicht");
    }

    /// Codex 30.10.: Eintraege im alten Token-Hash-Ordner kommen in den
    /// Ordner der Pilot-ID — mit Merker, Abgelehntem und Konflikt; ein
    /// gleichnamiger anderer Eintrag im neuen Ordner bleibt, der alte auch.
    /// Gegenprobe: vor der Uebernahme sieht der neue Ordner nichts.
    #[test]
    fn alter_kontoordner_wird_uebernommen() {
        use crate::upload_warteschlange as uw;
        let d = std::env::temp_dir().join(format!("aeroacars-li-uebernahme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (alt, neu) = (uw::eingabe_ordner(&d, &alter_konto("tok")), uw::eingabe_ordner(&d, &konto("7")));
        for o in [alt.join("abgelehnt"), alt.join("konflikt"), neu.clone()] {
            std::fs::create_dir_all(o).unwrap();
        }
        std::fs::write(alt.join("A_1.json.gz"), b"a").unwrap();
        uw::fehlschlag(&alt.join("A_1.json.gz"), 0, "HTTP 503");
        uw::fehlschlag(&alt.join("A_1.json.gz"), 0, "HTTP 503");
        std::fs::write(alt.join("A_1.json.tmp"), b"halb").unwrap();
        std::fs::write(alt.join("abgelehnt").join("B_2.json.gz"), b"b").unwrap();
        std::fs::write(alt.join("konflikt").join("K_1.json.gz"), b"k").unwrap();
        std::fs::write(alt.join("C_3.json.gz"), b"c-alt").unwrap();
        std::fs::write(neu.join("C_3.json.gz"), b"c-neu").unwrap();
        std::fs::write(alt.join("D_4.json.gz"), b"d").unwrap();
        std::fs::write(neu.join("D_4.json.gz"), b"d").unwrap();
        let k7 = konto("7");
        assert_eq!(uw::eingaben_offen(&d, Some(&k7)), ["C".to_string(), "D".to_string()].into(), "Gegenprobe");
        uw::konto_uebernehmen(&alt, &neu);
        assert_eq!(uw::eingaben_offen(&d, Some(&k7)), ["A".to_string(), "C".to_string(), "D".to_string()].into());
        assert_eq!(uw::versuch(&neu.join("A_1.json.gz")).versuche, 2, "Merker kommt mit");
        assert!(neu.join("abgelehnt").join("B_2.json.gz").exists());
        assert!(neu.join("konflikt").join("K_1.json.gz").exists());
        assert_eq!(std::fs::read(neu.join("C_3.json.gz")).unwrap(), b"c-neu", "nichts ueberschrieben");
        assert_eq!(std::fs::read(alt.join("C_3.json.gz")).unwrap(), b"c-alt", "nichts verloren");
        assert!(!alt.join("D_4.json.gz").exists(), "gleicher Inhalt: keine Doppelten");
        assert!(!alt.join("A_1.json.gz.versuch").exists() && !alt.join("abgelehnt").exists());
        // Zweiter Lauf aendert nichts; gleicher Ordner ist kein Fall.
        uw::konto_uebernehmen(&alt, &neu);
        uw::konto_uebernehmen(&neu, &neu);
        assert_eq!(uw::eingaben(&d, &k7).len(), 3);
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
