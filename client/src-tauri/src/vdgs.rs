//! VDGS-Band — die eigene Abflugfolge (TOBT/TSAT/CTOT) im Cockpit.
//!
//! Quelle ist `api.viffsys.com`, das Rechenwerk hinter dem A-CDM-Werkzeug
//! von VATSIM Spain (`vats.im/vdgs`). Der Abruf je Rufzeichen ist offen
//! (keine Anmeldung, kein Schluessel) und liefert weltweit — nachgemessen
//! am 20.09.2026 mit EDDF, LEBL, LIRF.
//!
//! **Nur lesend.** Gesetzt wird die TOBT weiterhin auf deren Seite (s.
//! `VatsimCdmView.tsx`): der Schreibweg `POST /ifps/dpi` verlangt einen
//! `x-api-key`, den VATSIM Spain einzeln vergibt, und die Bindung an die
//! echte Pilotenkennung muesste der Aufrufer selbst sicherstellen. Ohne
//! beides wird hier nichts geschrieben.
//!
//! Etikette gegenueber einem fremden Dienst, den wir nicht bezahlen:
//!   * Abruf nur, wenn ueberhaupt ein Flug laeuft (kein Leerlauf-Polling),
//!   * je Rufzeichen statt je Flughafen (LEBL: 2 kB statt 59 kB),
//!   * serverseitiger Zwischenspeicher mit Mindestabstand, damit eine
//!     hektische Oberflaeche den Dienst nicht haeufiger trifft als noetig.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

/// Mindestabstand zweier echter Abrufe. Die CDM-Rechnung drueben laeuft
/// alle zwei Minuten (IFPS) bzw. jede Minute (ETFMS) — oefter als einmal
/// pro Minute zu fragen brachte nie etwas Neues.
const MIN_ABSTAND: Duration = Duration::from_secs(60);

/// Wie lange ein gemerkter Stand einen Ausfall ueberbruecken darf.
/// Waehrenddessen bleibt das Band stehen, statt bei jedem Netzhaenger zu
/// verschwinden und wiederzukommen — ein flackerndes Geraet im Cockpit
/// waere schlimmer als eine zwei Minuten alte TSAT. Danach ist der Wert
/// zu alt, um ihn noch zu zeigen.
const HOECHSTALTER_BEI_AUSFALL: Duration = Duration::from_secs(600);

/// Was das Band zeigt. Zeiten als `HH:MM` oder leer — die Umrechnung aus
/// den beiden Formaten der Gegenseite (HHMM und HHMMSS) passiert hier,
/// damit die Oberflaeche nur noch anzeigt.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VdgsStand {
    pub callsign: String,
    pub departure: String,
    /// Aus dem VATSIM-Flugplan.
    pub eobt: String,
    /// Vom Piloten gesetzt bzw. bestaetigt.
    pub tobt: String,
    /// Vom CDM zugeteilt. Leer an Plaetzen ohne CDM-Sequenzierung.
    pub tsat: String,
    /// Aus der Flussregelung. Leer, wenn keine Regulierung greift.
    pub ctot: String,
    /// Rollzeit in Minuten.
    pub taxi_min: Option<u32>,
    /// `COMPLY`, `FLS-NRA`, … — kann leer sein, solange nichts entschieden ist.
    pub cdm_sts: String,
    /// Die schwerste greifende Regulierung, falls eine vorliegt.
    pub regulierung: String,
    /// Bahn und Abflugstrecke, wie die Gegenseite sie fuehrt ("24L/OLOXO3Q").
    pub rwy_sid: String,
}

/// Rohantwort. Nur die Felder, die das Band braucht; alles andere bleibt
/// bewusst ungelesen, damit ein Formatwechsel drueben nicht den Abruf
/// killt.
#[derive(Debug, Clone, Default, Deserialize)]
struct ApiFlug {
    #[serde(default)]
    callsign: String,
    #[serde(default)]
    departure: String,
    #[serde(default)]
    eobt: String,
    #[serde(default)]
    tobt: String,
    #[serde(default)]
    ctot: String,
    #[serde(default)]
    taxi: Option<u32>,
    #[serde(default, rename = "cdmSts")]
    cdm_sts: String,
    #[serde(default, rename = "cdmData")]
    cdm_data: ApiCdm,
    #[serde(default, rename = "atfcmData")]
    atfcm_data: ApiAtfcm,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ApiCdm {
    #[serde(default)]
    tobt: String,
    #[serde(default)]
    tsat: String,
    #[serde(default)]
    ctot: String,
    /// "24L/OLOXO3Q" — Bahn und Abflugstrecke.
    #[serde(default, rename = "depInfo")]
    dep_info: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ApiAtfcm {
    #[serde(default, rename = "mostPenalisingRegulation")]
    most_penalising_regulation: String,
}

/// `1546` → `15:46`, `154600` → `15:46`, alles andere → leer.
///
/// Die Gegenseite fuehrt beide Formate NEBENEINANDER: die Flugplanzeiten
/// vierstellig, die CDM-Zeiten sechsstellig mit Sekunden (nachgemessen an
/// LEBL/VLG10WL, 20.09.2026). Wer das verwechselt, zeigt "15:46:00" als
/// Uhrzeit an oder gar nichts.
fn zeit(roh: &str) -> String {
    let r = roh.trim();
    if !(r.len() == 4 || r.len() == 6) || !r.bytes().all(|b| b.is_ascii_digit()) {
        return String::new();
    }
    let (h, m) = (&r[0..2], &r[2..4]);
    // Unsinnige Werte lieber verschweigen als falsch anzeigen.
    if h.parse::<u32>().unwrap_or(99) > 23 || m.parse::<u32>().unwrap_or(99) > 59 {
        return String::new();
    }
    format!("{h}:{m}")
}

/// Die Rohantwort auf das reduzieren, was das Band zeigt.
///
/// Reihenfolge bei TOBT/CTOT: der CDM-Wert gewinnt, wenn es einen gibt —
/// er ist der juengere. Sonst der Flugplanwert.
fn aufbereiten(f: &ApiFlug) -> VdgsStand {
    let tobt = {
        let cdm = zeit(&f.cdm_data.tobt);
        if cdm.is_empty() { zeit(&f.tobt) } else { cdm }
    };
    let ctot = {
        let cdm = zeit(&f.cdm_data.ctot);
        if cdm.is_empty() { zeit(&f.ctot) } else { cdm }
    };
    VdgsStand {
        callsign: f.callsign.trim().to_uppercase(),
        departure: f.departure.trim().to_uppercase(),
        eobt: zeit(&f.eobt),
        tobt,
        tsat: zeit(&f.cdm_data.tsat),
        ctot,
        taxi_min: f.taxi.filter(|t| *t > 0),
        cdm_sts: f.cdm_sts.trim().to_uppercase(),
        regulierung: f.atfcm_data.most_penalising_regulation.trim().to_string(),
        rwy_sid: f.cdm_data.dep_info.trim().to_uppercase(),
    }
}

/// Zwischenspeicher: letzter Abruf je Rufzeichen. `None` als Wert heisst
/// „gefragt, aber dieser Flug ist dort nicht gefuehrt" — auch das wird
/// gemerkt, sonst fragt die Oberflaeche fuer jeden Flug ausserhalb des
/// CDM-Systems im Minutentakt vergeblich nach.
type Speicher = Mutex<Option<(String, Instant, Option<VdgsStand>)>>;

fn speicher() -> &'static Speicher {
    static S: OnceLock<Speicher> = OnceLock::new();
    S.get_or_init(|| Mutex::new(None))
}

fn aus_speicher(callsign: &str, hoechstalter: Duration) -> Option<Option<VdgsStand>> {
    let g = speicher().lock().ok()?;
    let (gemerkt, wann, wert) = g.as_ref()?;
    if gemerkt == callsign && wann.elapsed() < hoechstalter {
        Some(wert.clone())
    } else {
        None
    }
}

fn in_speicher(callsign: &str, wert: Option<VdgsStand>) {
    if let Ok(mut g) = speicher().lock() {
        *g = Some((callsign.to_string(), Instant::now(), wert));
    }
}

/// Nur das, was ein Rufzeichen sein darf — der Rest fliegt raus, damit
/// aus einem krummen Flugplanfeld keine zusaetzlichen URL-Parameter
/// werden koennen.
fn sauberes_rufzeichen(roh: &str) -> String {
    roh.trim()
        .to_uppercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(16)
        .collect()
}

/// Einen Abruf gegen die Gegenseite. `Ok(None)` = Flug dort nicht gefuehrt.
async fn abrufen(callsign: &str) -> Result<Option<VdgsStand>, String> {
    let url = format!(
        "https://api.viffsys.com/ifps/callsign?callsign={callsign}&profile=false"
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let antwort = client.get(&url).send().await.map_err(|e| e.to_string())?;
    let status = antwort.status();
    // 404 heisst dort schlicht „kein Eintrag" — kein Fehler, den der Pilot
    // sehen muesste.
    if status == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(format!("HTTP {}", status.as_u16()));
    }
    let rumpf = antwort.bytes().await.map_err(|e| e.to_string())?;
    aus_rumpf(&rumpf)
}

/// Antwortrumpf → Stand. Eigene Funktion, weil hier der Fall sitzt, den
/// ein unbekanntes Rufzeichen ausloest: die Gegenseite antwortet dann
/// mit **HTTP 200 und leerem Rumpf** (nachgemessen 20.09.2026), nicht
/// mit 404. Wer nur auf den Statuscode schaut, zeigt im Cockpit einen
/// Fehler, wo schlicht kein Eintrag ist.
fn aus_rumpf(rumpf: &[u8]) -> Result<Option<VdgsStand>, String> {
    if rumpf.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(None);
    }
    // Die Gegenseite antwortet auf diesen Weg mit EINEM Objekt; auf dem
    // Flughafenweg mit einer Liste. Beides annehmen kostet nichts und
    // faengt den Tag ab, an dem sie es angleichen.
    let flug: Option<ApiFlug> = match serde_json::from_slice::<ApiFlug>(rumpf) {
        Ok(f) => Some(f),
        Err(_) => serde_json::from_slice::<Vec<ApiFlug>>(rumpf)
            .map_err(|e| e.to_string())?
            .into_iter()
            .next(),
    };
    Ok(flug
        .filter(|f| !f.callsign.trim().is_empty())
        .map(|f| aufbereiten(&f)))
}

/// Stand der eigenen Abflugfolge, oder `None`, wenn es nichts zu zeigen
/// gibt: kein laufender Flug, kein Eintrag drueben, oder der Dienst
/// antwortet nicht. Ein Ausfall ist bewusst kein Fehler — das Band
/// verschwindet dann einfach, statt im Cockpit eine Fehlermeldung zu
/// hinterlassen.
#[tauri::command]
pub async fn vdgs_stand(app: AppHandle) -> Option<VdgsStand> {
    // Erst das Rufzeichen (kurz, synchron, keine Sperre ueber ein await),
    // dann der Netzabruf.
    let callsign = crate::hoppie::hoppie_get_flight_context(app)
        .callsign
        .map(|c| c.trim().to_uppercase())
        .filter(|c| !c.is_empty())?;

    let callsign = sauberes_rufzeichen(&callsign);
    if callsign.is_empty() {
        return None;
    }

    if let Some(gemerkt) = aus_speicher(&callsign, MIN_ABSTAND) {
        return gemerkt;
    }

    match abrufen(&callsign).await {
        Ok(stand) => {
            in_speicher(&callsign, stand.clone());
            stand
        }
        Err(e) => {
            tracing::debug!(target: "vdgs", "VDGS-Abruf fehlgeschlagen: {e}");
            // Fehler NICHT merken: beim naechsten Takt darf es wieder
            // versucht werden. Solange der letzte gute Stand nicht zu alt
            // ist, bleibt er stehen — sonst flackert das Band bei jedem
            // Netzhaenger weg und wieder hin.
            aus_speicher(&callsign, HOECHSTALTER_BEI_AUSFALL).flatten()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leer() -> VdgsStand {
        VdgsStand {
            callsign: String::new(),
            departure: String::new(),
            eobt: String::new(),
            tobt: String::new(),
            tsat: String::new(),
            ctot: String::new(),
            taxi_min: None,
            cdm_sts: String::new(),
            regulierung: String::new(),
            rwy_sid: String::new(),
        }
    }

    #[test]
    fn vierstellige_und_sechsstellige_zeiten_werden_beide_gelesen() {
        assert_eq!(zeit("1546"), "15:46");
        assert_eq!(zeit("154600"), "15:46");
        assert_eq!(zeit(" 0905 "), "09:05");
    }

    #[test]
    fn unsinnige_zeiten_bleiben_leer() {
        assert_eq!(zeit(""), "");
        assert_eq!(zeit("--"), "");
        assert_eq!(zeit("2461"), "");
        assert_eq!(zeit("9999"), "");
        assert_eq!(zeit("15460"), "");
        assert_eq!(zeit("15:46"), "");
    }

    /// Gegenprobe zur Vorrangregel: gaebe es sie nicht, zeigte das Band
    /// die aeltere Flugplan-TOBT statt der frisch gesetzten.
    #[test]
    fn cdm_wert_geht_vor_flugplanwert() {
        let f = ApiFlug {
            tobt: "1530".into(),
            cdm_data: ApiCdm {
                tobt: "154400".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(aufbereiten(&f).tobt, "15:44");

        let ohne_cdm = ApiFlug {
            tobt: "1530".into(),
            ..Default::default()
        };
        assert_eq!(aufbereiten(&ohne_cdm).tobt, "15:30");
    }

    /// Echte Antwort (LEBL/VLG10WL, 20.09.2026), auf die gelesenen Felder
    /// gekuerzt — faengt einen Feldumbenennung drueben.
    #[test]
    fn echte_antwort_wird_vollstaendig_gelesen() {
        let roh = r#"{
            "callsign":"VLG10WL","departure":"LEBL","eobt":"1550","tobt":"1546",
            "ctot":"","taxi":15,"cdmSts":"COMPLY",
            "atfcmData":{"mostPenalisingRegulation":"LECBCTA"},
            "cdmData":{"tobt":"154400","tsat":"154600","ttot":"160100",
                       "ctot":"","depInfo":"24L/OLOXO3Q"}
        }"#;
        let f: ApiFlug = serde_json::from_str(roh).expect("Antwort lesbar");
        let s = aufbereiten(&f);
        assert_eq!(s.callsign, "VLG10WL");
        assert_eq!(s.departure, "LEBL");
        assert_eq!(s.eobt, "15:50");
        assert_eq!(s.tobt, "15:44");
        assert_eq!(s.tsat, "15:46");
        assert_eq!(s.ctot, "");
        assert_eq!(s.taxi_min, Some(15));
        assert_eq!(s.cdm_sts, "COMPLY");
        assert_eq!(s.regulierung, "LECBCTA");
        assert_eq!(s.rwy_sid, "24L/OLOXO3Q");
    }

    /// Ein Platz ohne CDM-Sequenzierung (EDDF, 20.09.2026): `cdmData` ist
    /// leer, die Flugplanzeiten stehen trotzdem. Das Band muss das zeigen
    /// koennen, statt auf eine TSAT zu warten, die dort nie kommt.
    #[test]
    fn platz_ohne_cdm_liefert_trotzdem_zeiten() {
        let roh = r#"{"callsign":"DLH355","departure":"EDDF","eobt":"1844",
            "tobt":"1753","ctot":"","taxi":9,"cdmSts":"COMPLY",
            "cdmData":{"tobt":"","tsat":"","ctot":"","depInfo":""}}"#;
        let f: ApiFlug = serde_json::from_str(roh).expect("Antwort lesbar");
        let s = aufbereiten(&f);
        assert_eq!(s.tobt, "17:53");
        assert_eq!(s.tsat, "");
        assert_eq!(s.taxi_min, Some(9));
    }

    /// Unbekannte oder neue Felder duerfen den Abruf nicht killen.
    #[test]
    fn unbekannte_felder_stoeren_nicht() {
        let roh = r#"{"callsign":"GSG1","voellig":"neu","cdmData":{"tsat":"120000","neu":1}}"#;
        let f: ApiFlug = serde_json::from_str(roh).expect("Antwort lesbar");
        assert_eq!(aufbereiten(&f).tsat, "12:00");
    }

    /// Der Zwischenspeicher hat zwei Fristen: die kurze fuer den
    /// Normalbetrieb, die lange als Ueberbrueckung bei Ausfall.
    #[test]
    fn gemerkter_stand_verfaellt_nach_frist() {
        let stand = VdgsStand {
            callsign: "GSG9".into(),
            ..leer()
        };
        in_speicher("GSG9", Some(stand.clone()));

        assert_eq!(
            aus_speicher("GSG9", MIN_ABSTAND),
            Some(Some(stand.clone())),
            "frisch gemerkt muss innerhalb der kurzen Frist kommen"
        );
        // Gegenprobe: mit Frist Null darf derselbe Eintrag NICHT kommen.
        assert_eq!(aus_speicher("GSG9", Duration::ZERO), None);
        // Und ein anderes Rufzeichen bekommt nie den fremden Stand.
        assert_eq!(aus_speicher("GSG8", MIN_ABSTAND), None);
    }

    /// Unbekanntes Rufzeichen: HTTP 200, leerer Rumpf. Muss „kein
    /// Eintrag" heissen, nicht „Fehler". Am 20.09.2026 am echten Dienst
    /// nachgemessen (ZZZ9999 -> 200, 0 Bytes).
    #[test]
    fn leerer_rumpf_ist_kein_fehler() {
        assert_eq!(aus_rumpf(b""), Ok(None));
        assert_eq!(aus_rumpf(b"   \n"), Ok(None));
    }

    #[test]
    fn rumpf_ohne_rufzeichen_zaehlt_nicht_als_eintrag() {
        assert_eq!(aus_rumpf(br#"{"callsign":""}"#), Ok(None));
    }

    #[test]
    fn liste_wird_ebenso_gelesen_wie_ein_einzelnes_objekt() {
        let s = aus_rumpf(br#"[{"callsign":"GSG1","cdmData":{"tsat":"120000"}}]"#)
            .expect("lesbar")
            .expect("ein Eintrag");
        assert_eq!(s.tsat, "12:00");
    }

    #[test]
    fn kaputter_rumpf_meldet_fehler() {
        assert!(aus_rumpf(b"<html>nope</html>").is_err());
    }

    #[test]
    fn rufzeichen_wird_auf_erlaubte_zeichen_gestutzt() {
        assert_eq!(sauberes_rufzeichen(" dlh4tk "), "DLH4TK");
        assert_eq!(sauberes_rufzeichen("GSG1&profile=true"), "GSG1PROFILETRUE");
        assert_eq!(sauberes_rufzeichen("N-123"), "N-123");
        assert_eq!(sauberes_rufzeichen("  "), "");
    }

    #[test]
    fn taxi_null_zaehlt_als_unbekannt() {
        let f = ApiFlug {
            taxi: Some(0),
            ..Default::default()
        };
        assert_eq!(aufbereiten(&f).taxi_min, None);
    }
}
