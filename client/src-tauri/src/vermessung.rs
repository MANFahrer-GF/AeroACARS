//! „Flugzeug vermessen" (28.09.2026) — geführte Schaltermessung im Client.
//!
//! Der Aircraft-Scan auf live.kant.ovh sagt, welche Variablen ein Add-on
//! HAT. Welche davon mit welchem Cockpitschalter mitgeht und mit welchen
//! Werten, sieht man nur im laufenden Simulator. Dafür schaltet der Pilot
//! Schritt für Schritt, der Client schaut zu:
//!
//! 1. `vermessung_starten` — Quelle öffnen (X-Plane: alle Datarefs per Web-API
//!    v2; MSFS: B:-Input-Events + A:-Schalter-SimVars + L:-Namen aus dem Scan).
//! 2. `vermessung_ruhe` — ein paar Sekunden nichts anfassen; was sich dabei
//!    ändert, ist Rauschen und wird ignoriert.
//! 3. je Schalter: `vermessung_stellung` pro Stellung, dann
//!    `vermessung_schritt_abschliessen`.
//! 4. `vermessung_senden` → live.kant.ovh (Einsendestrecke des Scans).
//!
//! Die Auswertung (Rauschen, Kandidaten, Bericht) ist rein und getestet; die
//! Befehle verdrahten nur. Nichts wird in den Simulator geschrieben.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// Kandidaten je Schritt im Bericht.
pub const MAX_KANDIDATEN: usize = 80;
/// Wartezeit nach dem Umschalten, bevor gemessen wird (Animation/Logik des
/// Add-ons, MSFS liefert die Blöcke einmal pro Sekunde).
const NACHLAUF: Duration = Duration::from_millis(1600);

// ─── reine Auswertung ─────────────────────────────────────────────────────

type Stand = HashMap<String, f64>;

fn gleich(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() < 1e-4,
        (None, None) => true,
        _ => false,
    }
}

/// Werte, die sich zwischen Ruhe-Schnappschüssen ändern (Uhr, Animationen …).
pub fn unruhig(staende: &[Stand]) -> HashSet<String> {
    let mut aus = HashSet::new();
    for paar in staende.windows(2) {
        let (a, b) = (&paar[0], &paar[1]);
        for k in a.keys().chain(b.keys()) {
            if !gleich(a.get(k).copied(), b.get(k).copied()) {
                aus.insert(k.clone());
            }
        }
    }
    aus
}

/// Wie viele Werte zwischen zwei Ständen mitgegangen sind (ohne Rauschen) —
/// die Rückmeldung „es hat sich etwas bewegt" für den Piloten.
pub fn mitgegangen(vorher: &Stand, jetzt: &Stand, rauschen: &HashSet<String>) -> usize {
    jetzt
        .keys()
        .chain(vorher.keys())
        .collect::<HashSet<_>>()
        .into_iter()
        .filter(|k| !rauschen.contains(*k))
        .filter(|k| !gleich(vorher.get(*k).copied(), jetzt.get(*k).copied()))
        .count()
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Kandidat {
    pub variable: String,
    pub werte: Vec<Option<f64>>,
}

/// Ist das ein Standardwert des Simulators (nachrangig im Bericht)?
fn standard(name: &str) -> bool {
    name.starts_with("sim/") || name.starts_with("A:")
}

/// Kandidaten eines Schritts: Werte, die sich über die Stellungen ändern und
/// nicht zum Rauschen gehören. Add-on-Werte zuerst, dann nach Zahl
/// verschiedener Werte, dann nach Name.
pub fn kandidaten(staende: &[Stand], rauschen: &HashSet<String>) -> Vec<Kandidat> {
    if staende.len() < 2 {
        return Vec::new();
    }
    let alle: HashSet<&String> = staende.iter().flat_map(|s| s.keys()).collect();
    let mut aus: Vec<Kandidat> = alle
        .into_iter()
        .filter(|k| !rauschen.contains(*k))
        .filter_map(|k| {
            let werte: Vec<Option<f64>> = staende.iter().map(|s| s.get(k).copied()).collect();
            let wechselt = werte.windows(2).any(|p| !gleich(p[0], p[1]));
            wechselt.then(|| Kandidat {
                variable: k.clone(),
                werte,
            })
        })
        .collect();
    let verschieden = |k: &Kandidat| {
        let mut v: Vec<i64> = k
            .werte
            .iter()
            .map(|w| w.map(|x| (x * 1e4).round() as i64).unwrap_or(i64::MIN))
            .collect();
        v.sort_unstable();
        v.dedup();
        v.len()
    };
    aus.sort_by(|a, b| {
        standard(&a.variable)
            .cmp(&standard(&b.variable))
            .then(verschieden(b).cmp(&verschieden(a)))
            .then(a.variable.cmp(&b.variable))
    });
    aus.truncate(MAX_KANDIDATEN);
    aus
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct Flugzeug {
    pub titel: Option<String>,
    pub icao: Option<String>,
    pub autor: Option<String>,
    pub pfad: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Schritt {
    pub schalter: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub uebersprungen: bool,
    pub stellungen: Vec<String>,
    pub kandidaten: Vec<Kandidat>,
}

// ─── Sitzung ──────────────────────────────────────────────────────────────

enum Quelle {
    XPlane(sim_xplane::vermessung::Spiegel),
    #[cfg(target_os = "windows")]
    Msfs,
}

struct Sitzung {
    /// Laufnummer (siehe [`LAUF`]).
    nr: u64,
    /// Start-Kennung der Oberfläche (siehe [`vermessung_beenden`]).
    start: Option<u64>,
    /// Kennung für den Server — erneutes Senden derselben Messung legt dort
    /// keine zweite Einreichung an.
    messung_id: String,
    quelle: Quelle,
    sim: &'static str,
    flugzeug: Flugzeug,
    rauschen: HashSet<String>,
    anzahl_werte: usize,
    /// Laufender Schritt: Stellungsnamen und Stände.
    stellungen: Vec<String>,
    staende: Vec<Stand>,
    schritte: Vec<Schritt>,
}

static SITZUNG: Mutex<Option<Sitzung>> = Mutex::new(None);
/// Zählt bei jedem Start und jedem Beenden hoch. Ein Befehl merkt sich die
/// Nummer, mit der er begann; wurde inzwischen abgebrochen oder neu
/// gestartet, fasst er die Sitzung nicht mehr an (QS Codex, 28.09.2026:
/// eine alte Ruhemessung schrieb sonst ihr Rauschen in die neue Sitzung,
/// ein abgebrochener Start hinterließ eine aktive Messung).
static LAUF: AtomicU64 = AtomicU64::new(0);

/// Starts laufen nacheinander (PC und LAN-Brücke gleichzeitig): die
/// MSFS-Messquelle gibt es nur einmal, ein abgebrochener Start dürfte sie
/// sonst einem neueren unter den Füßen wegräumen.
static START: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Abgesagte Starts (Kennung der Oberfläche): wer die Seite mitten im
/// Verbinden verlässt, kennt die Sitzungsnummer noch nicht — er sagt seinen
/// Start ab, ohne einen anderen, gleichzeitig laufenden zu treffen.
static ABGESAGT: Mutex<Vec<u64>> = Mutex::new(Vec::new());

fn abgesagt(start: Option<u64>) -> bool {
    start.is_some_and(|t| ABGESAGT.lock().map(|g| g.contains(&t)).unwrap_or(false))
}

const ABGEBROCHEN: &str = "Die Messung wurde abgebrochen.";

// `app` braucht nur der MSFS-Zweig (Windows).
#[cfg_attr(not(target_os = "windows"), allow(unused_variables))]
fn stand_lesen(app: &AppHandle, q: &Quelle) -> Result<Stand, String> {
    match q {
        Quelle::XPlane(s) => {
            if !s.lebt() {
                return Err(
                    "Die Verbindung zu X-Plane ist abgerissen — bitte die Messung \
                            abbrechen und neu starten."
                        .into(),
                );
            }
            Ok(s.schnappschuss())
        }
        #[cfg(target_os = "windows")]
        Quelle::Msfs => {
            let st = app.state::<crate::AppState>();
            let a = st.msfs.lock().expect("msfs lock");
            Ok(a.vermessung_werte().into_iter().collect())
        }
    }
}

fn quelle_beenden(app: &AppHandle, q: Quelle) {
    match q {
        Quelle::XPlane(s) => drop(s),
        #[cfg(target_os = "windows")]
        Quelle::Msfs => {
            let st = app.state::<crate::AppState>();
            st.msfs.lock().expect("msfs lock").vermessung_stoppen();
        }
    }
    let _ = app;
}

/// Nummer der laufenden Sitzung. Nennt der Aufrufer eine Sitzung (die
/// Oberfläche tut das immer, auch über die LAN-Brücke), muss es diese sein —
/// ein verspäteter Befehl aus einem alten Lauf fasst die neue nicht an.
fn aktuelle_nr(sitzung: Option<u64>) -> Result<u64, String> {
    let g = SITZUNG.lock().map_err(|_| "Sperre")?;
    let nr = g.as_ref().ok_or("Keine Messung aktiv")?.nr;
    match sitzung {
        Some(x) if x != nr => Err(ABGEBROCHEN.into()),
        _ => Ok(nr),
    }
}

/// Sitzung `nr` schließen, falls sie noch die aktuelle ist (ohne die
/// Laufnummer zu erhöhen).
fn schliessen_wenn(app: &AppHandle, nr: u64) {
    let alt = SITZUNG.lock().ok().and_then(|mut g| {
        if g.as_ref().is_some_and(|s| s.nr == nr) {
            g.take()
        } else {
            None
        }
    });
    if let Some(alt) = alt {
        quelle_beenden(app, alt.quelle);
    }
}

/// Die Sitzung `nr` — oder Fehler, wenn sie inzwischen beendet/ersetzt ist.
fn mit_sitzung<T>(nr: u64, f: impl FnOnce(&mut Sitzung) -> Result<T, String>) -> Result<T, String> {
    let mut g = SITZUNG.lock().map_err(|_| "Sperre")?;
    match g.as_mut() {
        Some(s) if s.nr == nr => f(s),
        _ => Err(ABGEBROCHEN.into()),
    }
}

fn stand_jetzt(app: &AppHandle, nr: u64) -> Result<Stand, String> {
    mit_sitzung(nr, |s| stand_lesen(app, &s.quelle))
}

// ─── Befehle ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StartAntwort {
    sim: &'static str,
    flugzeug: Flugzeug,
    anzahl_werte: usize,
    /// MSFS: wie viele L:-Namen aus dem Aircraft-Scan kamen (0 = kein Scan).
    l_namen: usize,
    /// Sitzungsnummer — die Oberfläche gibt sie bei jedem Befehl mit.
    sitzung: u64,
}

/// Messung starten. Nur am Boden und mit verbundenem Simulator.
#[tauri::command]
pub async fn vermessung_starten(
    app: AppHandle,
    start: Option<u64>,
) -> Result<StartAntwort, String> {
    let snap = crate::current_snapshot(&app).ok_or(
        "Kein Simulator verbunden — bitte erst den Simulator starten und ein Flugzeug laden.",
    )?;
    if !snap.on_ground {
        return Err("Bitte nur am Boden messen (Parkposition, Parkbremse gesetzt).".into());
    }
    // Laufnummer VOR dem Warten auf einen anderen Start ziehen: wird dieser
    // Aufruf abgebrochen, während er wartet, merkt er es danach.
    let nr = LAUF.fetch_add(1, Ordering::SeqCst) + 1;
    let _start = START.lock().await;
    if LAUF.load(Ordering::SeqCst) != nr || abgesagt(start) {
        return Err(ABGEBROCHEN.into());
    }
    // Eine alte Sitzung sauber beenden.
    let alt = SITZUNG.lock().map_err(|_| "Sperre")?.take();
    if let Some(alt) = alt {
        quelle_beenden(&app, alt.quelle);
    }
    let kind = crate::read_sim_config(&app).kind;
    let (quelle, sim, flugzeug, l_namen) = if kind.is_xplane() {
        let spiegel =
            tauri::async_runtime::spawn_blocking(sim_xplane::vermessung::Spiegel::starten)
                .await
                .map_err(|e| e.to_string())??;
        let f = Flugzeug {
            titel: spiegel.flugzeug.descrip.clone(),
            icao: spiegel.flugzeug.icao.clone(),
            autor: spiegel.flugzeug.author.clone(),
            pfad: spiegel.flugzeug.relative_path.clone(),
        };
        (Quelle::XPlane(spiegel), "xplane", f, 0)
    } else {
        msfs_starten(&app, &snap).await?
    };
    {
        let mut g = SITZUNG.lock().map_err(|_| "Sperre")?;
        // Während des Verbindens abgebrochen (Seite verlassen) oder neu
        // gestartet: diese Quelle gleich wieder schließen.
        if LAUF.load(Ordering::SeqCst) != nr || abgesagt(start) {
            drop(g);
            quelle_beenden(&app, quelle);
            return Err(ABGEBROCHEN.into());
        }
        *g = Some(Sitzung {
            nr,
            start,
            messung_id: uuid::Uuid::new_v4().simple().to_string(),
            quelle,
            sim,
            flugzeug: flugzeug.clone(),
            rauschen: HashSet::new(),
            anzahl_werte: 0,
            stellungen: Vec::new(),
            staende: Vec::new(),
            schritte: Vec::new(),
        });
    }
    // Warten, bis die Werte da sind (höchstens ~12 s).
    let mut anzahl = 0;
    for _ in 0..24 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if abgesagt(start) {
            schliessen_wenn(&app, nr);
            return Err(ABGEBROCHEN.into());
        }
        let n = match stand_jetzt(&app, nr) {
            Ok(st) => st.len(),
            Err(e) => {
                // z. B. X-Plane-Verbindung abgerissen: nichts halb offen lassen.
                schliessen_wenn(&app, nr);
                return Err(e);
            }
        };
        if n > 0 && n == anzahl {
            break; // stabil
        }
        anzahl = n;
    }
    if anzahl == 0 {
        schliessen_wenn(&app, nr);
        return Err(
            "Vom Simulator kommen keine Werte an. Läuft er, und ist das Flugzeug geladen?".into(),
        );
    }
    mit_sitzung(nr, |s| {
        s.anzahl_werte = anzahl;
        Ok(())
    })?;
    Ok(StartAntwort {
        sim,
        flugzeug,
        anzahl_werte: anzahl,
        l_namen,
        sitzung: nr,
    })
}

#[cfg(target_os = "windows")]
async fn msfs_starten(
    app: &AppHandle,
    snap: &sim_core::SimSnapshot,
) -> Result<(Quelle, &'static str, Flugzeug, usize), String> {
    let titel = snap.aircraft_title.clone().unwrap_or_default();
    let icao = snap.aircraft_icao.clone().unwrap_or_default();
    // L:-Namen aus dem Aircraft-Scan — SimConnect kann L:-Variablen nicht
    // aufzählen. Ohne Scan geht es mit B:-Events und Standardwerten weiter.
    let namen = match crate::bordbuch_token(app) {
        Some(t) => aeroacars_mqtt::messung::lvar_namen(None, &t, &icao, &titel)
            .await
            .unwrap_or_default(),
        None => Vec::new(),
    };
    let n = namen.len();
    {
        let st = app.state::<crate::AppState>();
        st.msfs.lock().expect("msfs lock").vermessung_starten(namen);
    }
    let f = Flugzeug {
        titel: (!titel.is_empty()).then_some(titel),
        icao: (!icao.is_empty()).then_some(icao),
        autor: None,
        pfad: None,
    };
    Ok((Quelle::Msfs, "msfs", f, n))
}

#[cfg(not(target_os = "windows"))]
async fn msfs_starten(
    _app: &AppHandle,
    _snap: &sim_core::SimSnapshot,
) -> Result<(Quelle, &'static str, Flugzeug, usize), String> {
    Err("MSFS gibt es nur unter Windows.".into())
}

#[derive(Serialize)]
pub struct RuheAntwort {
    rauschen: usize,
}

/// Ruhemessung: sechs Stände über ~8 s; was sich ändert, ist Rauschen.
#[tauri::command]
pub async fn vermessung_ruhe(app: AppHandle, sitzung: Option<u64>) -> Result<RuheAntwort, String> {
    let nr = aktuelle_nr(sitzung)?;
    let mut staende = vec![stand_jetzt(&app, nr)?];
    for _ in 0..5 {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        staende.push(stand_jetzt(&app, nr)?);
    }
    let r = unruhig(&staende);
    let n = r.len();
    mit_sitzung(nr, |s| {
        s.rauschen = r;
        Ok(RuheAntwort { rauschen: n })
    })
}

#[derive(Serialize)]
pub struct StellungAntwort {
    /// Werte, die seit der vorigen Stellung mitgegangen sind.
    mitgegangen: usize,
    /// Erste Stellung eines Schritts: nur der Ausgangsstand.
    erste: bool,
}

/// Eine Stellung festhalten (nach kurzem Nachlauf).
#[tauri::command]
pub async fn vermessung_stellung(
    app: AppHandle,
    stellung: String,
    sitzung: Option<u64>,
) -> Result<StellungAntwort, String> {
    let nr = aktuelle_nr(sitzung)?;
    tokio::time::sleep(NACHLAUF).await;
    let stand = stand_jetzt(&app, nr)?;
    mit_sitzung(nr, |s| {
        let antwort = match s.staende.last() {
            Some(vorher) => StellungAntwort {
                mitgegangen: mitgegangen(vorher, &stand, &s.rauschen),
                erste: false,
            },
            None => StellungAntwort {
                mitgegangen: 0,
                erste: true,
            },
        };
        s.stellungen.push(stellung.chars().take(60).collect());
        s.staende.push(stand);
        Ok(antwort)
    })
}

#[derive(Serialize)]
pub struct SchrittAntwort {
    kandidaten: usize,
    beispiele: Vec<Kandidat>,
}

/// Schalter fertig (oder übersprungen): Kandidaten bilden, neu beginnen.
#[tauri::command]
pub fn vermessung_schritt_abschliessen(
    schalter: String,
    uebersprungen: bool,
    sitzung: Option<u64>,
) -> Result<SchrittAntwort, String> {
    let nr = aktuelle_nr(sitzung)?;
    mit_sitzung(nr, |s| {
        // Doppelklick: der zweite Abschluss fände leere Stände vor und würde
        // das gerade gebildete Ergebnis durch null Kandidaten ersetzen.
        if !uebersprungen && s.staende.is_empty() {
            if let Some(x) = s.schritte.iter().find(|x| x.schalter == schalter) {
                return Ok(SchrittAntwort {
                    kandidaten: x.kandidaten.len(),
                    beispiele: x.kandidaten.iter().take(3).cloned().collect(),
                });
            }
        }
        let staende = std::mem::take(&mut s.staende);
        let stellungen = std::mem::take(&mut s.stellungen);
        let k = if uebersprungen {
            Vec::new()
        } else {
            kandidaten(&staende, &s.rauschen)
        };
        let beispiele = k.iter().take(3).cloned().collect();
        let n = k.len();
        s.schritte.retain(|x| x.schalter != schalter);
        s.schritte.push(Schritt {
            schalter: schalter.chars().take(40).collect(),
            uebersprungen,
            stellungen: if uebersprungen {
                Vec::new()
            } else {
                stellungen
            },
            kandidaten: k,
        });
        Ok(SchrittAntwort {
            kandidaten: n,
            beispiele,
        })
    })
}

/// Stellungen des laufenden Schritts verwerfen (Pilot will neu beginnen).
#[tauri::command]
pub fn vermessung_schritt_neu(sitzung: Option<u64>) -> Result<(), String> {
    let nr = aktuelle_nr(sitzung)?;
    mit_sitzung(nr, |s| {
        s.staende.clear();
        s.stellungen.clear();
        Ok(())
    })
}

/// Der Bericht, wie er gesendet wird.
fn bericht(s: &Sitzung) -> serde_json::Value {
    serde_json::json!({
        "werkzeug": format!("AeroACARS {}", env!("CARGO_PKG_VERSION")),
        "messung_id": s.messung_id,
        "client_version": env!("CARGO_PKG_VERSION"),
        "sim": s.sim,
        "zeit_utc": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "flugzeug": s.flugzeug,
        "anzahl_werte": s.anzahl_werte,
        "unruhig_anzahl": s.rauschen.len(),
        "schritte": s.schritte,
    })
}

#[derive(Serialize)]
pub struct SendenAntwort {
    id: String,
}

/// Ergebnis an live.kant.ovh senden. Eine Kopie bleibt lokal liegen.
#[tauri::command]
pub async fn vermessung_senden(
    app: AppHandle,
    sitzung: Option<u64>,
) -> Result<SendenAntwort, String> {
    let nr = aktuelle_nr(sitzung)?;
    let json = mit_sitzung(nr, |s| {
        if s.schritte.iter().all(|x| x.uebersprungen) {
            return Err("Noch kein Schalter gemessen.".into());
        }
        Ok(bericht(s))
    })?;
    if let Ok(dir) = app.path().app_data_dir() {
        let ziel = dir.join("vermessungen");
        let _ = std::fs::create_dir_all(&ziel);
        let name = format!(
            "messung-{}.json",
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );
        let _ = std::fs::write(
            ziel.join(name),
            serde_json::to_vec_pretty(&json).unwrap_or_default(),
        );
    }
    let token = crate::bordbuch_token(&app)
        .ok_or("Nicht angemeldet — bitte in AeroACARS anmelden und erneut senden.")?;
    let id = aeroacars_mqtt::messung::senden(None, &token, &json)
        .await
        .map_err(|e| format!("Senden fehlgeschlagen: {e}"))?;
    Ok(SendenAntwort { id })
}

/// Schon vermessene Flugzeuge der VA (Startseite: „Schon vermessen").
/// Ohne Anmeldung eine leere Liste — die Anzeige ist nur ein Hinweis.
#[tauri::command]
pub async fn vermessung_liste(
    app: AppHandle,
) -> Result<Vec<aeroacars_mqtt::messung::Vermessen>, String> {
    let Some(token) = crate::bordbuch_token(&app) else {
        return Ok(Vec::new());
    };
    aeroacars_mqtt::messung::vermessen(None, &token)
        .await
        .map_err(|e| e.to_string())
}

/// Wie viele L:-Namen die Aircraft-Scans für dieses Flugzeug liefern
/// (Startseite: „erst einen Scan machen?"). Nur MSFS braucht das.
#[tauri::command]
pub async fn vermessung_scan_namen(
    app: AppHandle,
    icao: String,
    titel: String,
) -> Result<usize, String> {
    let token = crate::bordbuch_token(&app).ok_or("Nicht angemeldet")?;
    aeroacars_mqtt::messung::lvar_namen(None, &token, &icao, &titel)
        .await
        .map(|n| n.len())
        .map_err(|e| e.to_string())
}

/// Messung beenden (auch Abbrechen): Quelle schließen, alles verwerfen.
/// Mit `sitzung` nur diese — ein verspätetes Beenden aus einem alten Lauf
/// lässt eine neuere Messung stehen. Ohne (Seite verlassen mitten im
/// Verbinden) alles, auch einen noch laufenden Start.
#[tauri::command]
pub fn vermessung_beenden(app: AppHandle, sitzung: Option<u64>, start: Option<u64>) {
    if let Some(t) = start {
        // Genau diesen Start absagen (auch wenn er noch läuft) und seine
        // Sitzung schließen, falls sie schon steht.
        if let Ok(mut g) = ABGESAGT.lock() {
            if g.len() >= 64 {
                g.clear();
            }
            g.push(t);
        }
        let alt = SITZUNG.lock().ok().and_then(|mut g| {
            if g.as_ref().is_some_and(|s| s.start == Some(t)) {
                g.take()
            } else {
                None
            }
        });
        if let Some(alt) = alt {
            quelle_beenden(&app, alt.quelle);
        }
        return;
    }
    if let Some(nr) = sitzung {
        if LAUF.load(Ordering::SeqCst) == nr {
            LAUF.fetch_add(1, Ordering::SeqCst);
        }
        schliessen_wenn(&app, nr);
        return;
    }
    LAUF.fetch_add(1, Ordering::SeqCst);
    let alt = SITZUNG.lock().ok().and_then(|mut g| g.take());
    if let Some(alt) = alt {
        quelle_beenden(&app, alt.quelle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(p: &[(&str, f64)]) -> Stand {
        p.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn rauschen_aus_der_ruhemessung() {
        let r = unruhig(&[
            st(&[("sim/time/zulu", 1.0), ("laminar/strobe", 0.0)]),
            st(&[("sim/time/zulu", 2.0), ("laminar/strobe", 0.0)]),
            st(&[
                ("sim/time/zulu", 3.0),
                ("laminar/strobe", 0.0),
                ("neu", 1.0),
            ]),
        ]);
        assert!(r.contains("sim/time/zulu"));
        assert!(r.contains("neu"), "auftauchende Werte zählen als unruhig");
        assert!(!r.contains("laminar/strobe"));
    }

    /// Wie am 27.09.2026 im Laminar-A330 gemessen: Strobe-Schalter 0/1/2,
    /// Standardwert blitzt erst bei ON.
    #[test]
    fn kandidaten_addon_vor_standard() {
        let rauschen: HashSet<String> = ["sim/time/zulu".to_string()].into_iter().collect();
        let staende = vec![
            st(&[
                ("laminar/a333/switches/strobe_pos", 0.0),
                ("sim/cockpit2/switches/strobe_lights_on", 0.0),
                ("sim/time/zulu", 1.0),
                ("x/fest", 5.0),
            ]),
            st(&[
                ("laminar/a333/switches/strobe_pos", 1.0),
                ("sim/cockpit2/switches/strobe_lights_on", 0.0),
                ("sim/time/zulu", 2.0),
                ("x/fest", 5.0),
            ]),
            st(&[
                ("laminar/a333/switches/strobe_pos", 2.0),
                ("sim/cockpit2/switches/strobe_lights_on", 1.0),
                ("sim/time/zulu", 3.0),
                ("x/fest", 5.0),
            ]),
        ];
        let k = kandidaten(&staende, &rauschen);
        let namen: Vec<&str> = k.iter().map(|k| k.variable.as_str()).collect();
        assert_eq!(
            namen,
            [
                "laminar/a333/switches/strobe_pos",
                "sim/cockpit2/switches/strobe_lights_on"
            ]
        );
        assert_eq!(k[0].werte, vec![Some(0.0), Some(1.0), Some(2.0)]);
        assert_eq!(mitgegangen(&staende[1], &staende[2], &rauschen), 2);
        assert_eq!(mitgegangen(&staende[0], &staende[0], &rauschen), 0);
    }

    #[test]
    fn mehr_verschiedene_werte_zuerst_und_kappung() {
        let mut a = Stand::new();
        let mut b = Stand::new();
        let mut c = Stand::new();
        for i in 0..200 {
            let n = format!("L:X{i:03}");
            a.insert(n.clone(), 0.0);
            b.insert(n.clone(), 1.0);
            c.insert(n, 1.0);
        }
        a.insert("B:KNOPF".into(), 0.0);
        b.insert("B:KNOPF".into(), 1.0);
        c.insert("B:KNOPF".into(), 2.0);
        let k = kandidaten(&[a, b, c], &HashSet::new());
        assert_eq!(k.len(), MAX_KANDIDATEN);
        assert_eq!(
            k[0].variable, "B:KNOPF",
            "drei verschiedene Werte vor zweien"
        );
    }

    #[test]
    fn eine_stellung_ergibt_keine_kandidaten() {
        assert!(kandidaten(&[st(&[("a", 1.0)])], &HashSet::new()).is_empty());
    }
}
