//! „Flugzeug vermessen" (28.09.2026) — alle Werte des geladenen Flugzeugs
//! live mitlesen.
//!
//! Über die Web-API v2 (X-Plane 12.1.4+) werden alle Zahlen-Datarefs per
//! WebSocket abonniert; X-Plane schickt erst alle Werte und dann laufend die
//! geänderten. Ein Schnappschuss ist damit nur eine Kopie — bei ~8000
//! Werten Sekundenbruchteile statt 40 s mit Einzelabfragen (gemessen an der
//! X-Plane-12-Demo, 27.09.2026).
//!
//! ⚠ Abo-Nachrichten höchstens [`PAKET`] Datarefs: auf eine Nachricht mit
//! allen ~7800 IDs (≈170 KB) antwortete X-Plane 12.4.3 gar nicht.
//!
//! Nur lesen — es wird nichts in X-Plane geschrieben.
//!
//! **Plugin (Protokoll 2, AP7):** Besteht eine Sitzung mit dem
//! AeroACARS-Plugin, holt die Messung die Namen per `LISTE` und abonniert
//! sie ueber das Plugin (Abos 3–16, je ≤ 8192 Namen, 5 Hz) — ohne Web-API,
//! ohne deren Paketverluste, und auch unter X-Plane 11 ohne Web-API. Die
//! Web-API bleibt der Rueckfall (kein Plugin, `liste_nicht_verfuegbar`,
//! Zeitueberschreitung). Das Berichtsschema bleibt gleich.

use std::collections::{BTreeMap, HashMap};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;
use tungstenite::Message;

use crate::plugin2::{name_gueltig, NameStatus, Typ, Wert as P2Wert, MAX_NAMEN};
use crate::plugin2_ziel::{PluginZugang, ABO_MESSUNG_AB};
use crate::web_api::{AircraftInfo, DrefIdCache, WebApiClient};

/// Datarefs je Abo-Nachricht.
pub const PAKET: usize = 500;
/// Array-Datarefs werden bis zu dieser Länge elementweise geführt.
pub const MAX_ARRAY: usize = 48;
const HOST: &str = "127.0.0.1:8086";

#[derive(Debug, Deserialize)]
struct Liste {
    data: Vec<Eintrag>,
}

#[derive(Debug, Deserialize, Clone)]
struct Eintrag {
    id: i64,
    name: String,
    value_type: String,
}

fn ist_zahl(typ: &str) -> bool {
    matches!(
        typ,
        "int" | "float" | "double" | "int_array" | "float_array"
    )
}

/// Aktueller Wert eines Datarefs.
#[derive(Debug, Clone, PartialEq)]
enum Wert {
    Zahl(f64),
    Liste(Vec<f64>),
}

fn wert_aus_json(v: &serde_json::Value) -> Option<Wert> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().filter(|x| x.is_finite()).map(Wert::Zahl),
        serde_json::Value::Array(a) => Some(Wert::Liste(
            a.iter()
                .take(MAX_ARRAY)
                .map(|x| x.as_f64().unwrap_or(f64::NAN))
                .collect(),
        )),
        _ => None,
    }
}

/// Eine `dataref_update_values`-Nachricht in den Spiegel übernehmen.
/// Liefert die Zahl der übernommenen Werte.
fn nachricht_uebernehmen(text: &str, werte: &mut HashMap<i64, Wert>) -> usize {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return 0;
    };
    if v.get("type").and_then(|t| t.as_str()) != Some("dataref_update_values") {
        return 0;
    }
    let Some(daten) = v.get("data").and_then(|d| d.as_object()) else {
        return 0;
    };
    let mut n = 0;
    for (id, w) in daten {
        if let (Ok(id), Some(w)) = (id.parse::<i64>(), wert_aus_json(w)) {
            werte.insert(id, w);
            n += 1;
        }
    }
    n
}

/// Werte in einen flachen Schnappschuss (Name → Zahl), Arrays als `name[i]`.
fn flach(werte: &HashMap<i64, Wert>, namen: &HashMap<i64, String>) -> HashMap<String, f64> {
    let mut aus = HashMap::with_capacity(werte.len());
    for (id, w) in werte {
        let Some(name) = namen.get(id) else { continue };
        match w {
            Wert::Zahl(x) => {
                aus.insert(name.clone(), *x);
            }
            Wert::Liste(l) => {
                for (i, x) in l.iter().enumerate() {
                    if x.is_finite() {
                        aus.insert(format!("{name}[{i}]"), *x);
                    }
                }
            }
        }
    }
    aus
}

/// Die Abo-Nachrichten, in Paketen zu [`PAKET`].
#[cfg(test)]
fn abo_nachrichten(ids: &[i64]) -> Vec<String> {
    Abos::neu(ids).1
}

fn abo_nachricht(req_id: u64, ids: &[i64]) -> String {
    serde_json::json!({
        "req_id": req_id,
        "type": "dataref_subscribe_values",
        "params": { "datarefs": ids.iter().map(|id| serde_json::json!({ "id": id })).collect::<Vec<_>>() },
    })
    .to_string()
}

/// Offene Abo-Anfragen. X-Plane lehnt eine Anfrage GANZ ab, wenn auch nur
/// ein Dataref darin nicht abonnierbar ist („A failed subscription event
/// fails all datarefs sent", developer.x-plane.com, Web-API). Vorher fehlten
/// dann stillschweigend alle 500 Werte des Pakets — beim ToLiss A320neo
/// (Messung 28.09.2026) u. a. `AirbusFBW/AP1Engage`, obwohl er im Flug
/// nachweislich mitläuft. Jetzt wird ein abgelehntes Paket halbiert und neu
/// angemeldet, bis nur die einzelnen störrischen Datarefs übrig sind.
struct Abos {
    offen: HashMap<u64, Vec<i64>>,
    naechste: u64,
    abgelehnt: Vec<i64>,
}

impl Abos {
    fn neu(ids: &[i64]) -> (Abos, Vec<String>) {
        let mut a = Abos {
            offen: HashMap::new(),
            naechste: 1,
            abgelehnt: Vec::new(),
        };
        let n = ids.chunks(PAKET).map(|c| a.anmelden(c.to_vec())).collect();
        (a, n)
    }

    fn anmelden(&mut self, ids: Vec<i64>) -> String {
        let id = self.naechste;
        self.naechste += 1;
        let n = abo_nachricht(id, &ids);
        self.offen.insert(id, ids);
        n
    }

    /// Antwort von X-Plane verarbeiten; liefert die neu zu sendenden
    /// Abo-Nachrichten (die Hälften eines abgelehnten Pakets).
    fn antwort(&mut self, text: &str) -> Vec<String> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
            return Vec::new();
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("result") {
            return Vec::new();
        }
        let Some(ids) = v
            .get("req_id")
            .and_then(|r| r.as_u64())
            .and_then(|r| self.offen.remove(&r))
        else {
            return Vec::new();
        };
        if v.get("success").and_then(|s| s.as_bool()) != Some(false) {
            return Vec::new();
        }
        if ids.len() == 1 {
            self.abgelehnt.extend(ids);
            return Vec::new();
        }
        let (a, b) = ids.split_at(ids.len() / 2);
        vec![self.anmelden(a.to_vec()), self.anmelden(b.to_vec())]
    }
}

/// Laufende Messung: alle Werte des Flugzeugs, über das Plugin oder die
/// Web-API.
pub struct Spiegel {
    art: Art,
    pub flugzeug: AircraftInfo,
    pub abonniert: usize,
}

enum Art {
    WebApi(WebSpiegel),
    Plugin(PluginSpiegel),
}

struct WebSpiegel {
    werte: Arc<Mutex<HashMap<i64, Wert>>>,
    namen: HashMap<i64, String>,
    stop: Arc<AtomicBool>,
    /// Fällt auf `false`, sobald der Lesefaden endet (X-Plane beendet,
    /// Verbindung abgerissen) — danach wäre jeder Stand nur der alte Cache.
    lebt: Arc<AtomicBool>,
    faden: Option<JoinHandle<()>>,
    /// Datarefs, die X-Plane einzeln nicht abonnieren ließ (siehe [`Abos`]).
    abgelehnt: Arc<Mutex<Vec<i64>>>,
    abonniert: usize,
}

struct PluginSpiegel {
    zugang: PluginZugang,
    messung: Arc<P2Messung>,
}

impl Drop for PluginSpiegel {
    fn drop(&mut self) {
        // Abos 3.. abbestellen.
        self.zugang.messung_beenden(&self.messung);
    }
}

/// Wie vollständig die Anmeldung war — geht mit dem Bericht zum Server,
/// damit Lücken der Messung sichtbar sind statt still.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AboStand {
    pub angemeldet: usize,
    pub angekommen: usize,
    pub abgelehnt: usize,
    pub abgelehnt_namen: Vec<String>,
    /// Woher die Werte kamen: „plugin" (Protokoll 2) oder „web_api". Ältere
    /// Server verwerfen das Feld (zod-Objekt ohne `strict`), die lokale
    /// Kopie des Berichts behält es.
    pub quelle: &'static str,
}

impl Spiegel {
    /// Über das Plugin messen, wenn eine Sitzung besteht, sonst (oder wenn
    /// das Plugin die Liste nicht liefern kann) über die Web-API.
    ///
    /// QS AP7 H1: Bleibt ein Mess-Abo beim Plugin ohne Status, fällt DIESER
    /// Lauf auf die Web-API zurück — wie ohne Plugin, statt still leer zu
    /// bleiben. Der Bericht nennt die Quelle (`abo.quelle`).
    pub fn starten_mit(zugang: Option<PluginZugang>) -> Result<Spiegel, String> {
        let mut host = HOST.to_string();
        if let Some(z) = zugang {
            host = z.web_api_host();
            match plugin_starten(z) {
                Ok(s) => return Ok(s),
                Err(e) => tracing::info!(
                    grund = %e,
                    "X-Plane-Vermessung: Plugin kann nicht messen — Web-API"
                ),
            }
        }
        Self::starten_bei(&host)
    }

    /// Woher die Werte kommen („plugin" / „web_api"), fürs Protokoll.
    pub fn quelle(&self) -> &'static str {
        match self.art {
            Art::WebApi(_) => "web_api",
            Art::Plugin(_) => "plugin",
        }
    }

    /// Liste holen, verbinden, alles abonnieren, Lesefaden starten (Web-API).
    pub fn starten() -> Result<Spiegel, String> {
        Self::starten_bei(HOST)
    }

    /// Wie [`Self::starten`], mit anderer Web-API-Adresse (`host:port`).
    pub fn starten_bei(host: &str) -> Result<Spiegel, String> {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(3))
            .timeout_read(Duration::from_secs(30))
            .build();
        let liste: Liste = agent
            .get(&format!("http://{host}/api/v2/datarefs"))
            .call()
            .map_err(|e| format!("X-Plane-Web-API nicht erreichbar: {e}"))?
            .into_json()
            .map_err(|e| format!("Dataref-Liste nicht lesbar: {e}"))?;
        let zahlen: Vec<Eintrag> = liste
            .data
            .into_iter()
            .filter(|e| ist_zahl(&e.value_type))
            .collect();
        if zahlen.is_empty() {
            return Err("X-Plane meldet keine Werte — ist ein Flugzeug geladen?".into());
        }
        let namen: HashMap<i64, String> = zahlen.iter().map(|e| (e.id, e.name.clone())).collect();
        let ids: Vec<i64> = zahlen.iter().map(|e| e.id).collect();

        let flugzeug = WebApiClient::mit_basis(&format!("http://{host}"))
            .fetch_aircraft_info(&mut DrefIdCache::default())
            .unwrap_or_default();

        let strom = TcpStream::connect(host).map_err(|e| format!("WebSocket: {e}"))?;
        strom
            .set_read_timeout(Some(Duration::from_millis(400)))
            .map_err(|e| e.to_string())?;
        let (mut ws, _) = tungstenite::client(format!("ws://{host}/api/v2"), strom)
            .map_err(|e| format!("WebSocket-Handshake: {e}"))?;
        let (mut abos, erste) = Abos::neu(&ids);
        for n in erste {
            ws.send(Message::text(n))
                .map_err(|e| format!("Abo senden: {e}"))?;
        }

        let werte = Arc::new(Mutex::new(HashMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let lebt = Arc::new(AtomicBool::new(true));
        let abgelehnt = Arc::new(Mutex::new(Vec::new()));
        let (w2, s2, l2) = (Arc::clone(&werte), Arc::clone(&stop), Arc::clone(&lebt));
        let a2 = Arc::clone(&abgelehnt);
        let faden = std::thread::Builder::new()
            .name("xplane-vermessung".into())
            .spawn(move || {
                while !s2.load(Ordering::SeqCst) {
                    match ws.read() {
                        Ok(Message::Text(t)) => {
                            if nachricht_uebernehmen(t.as_str(), &mut w2.lock()) == 0 {
                                for n in abos.antwort(t.as_str()) {
                                    if let Err(e) = ws.send(Message::text(n)) {
                                        tracing::info!(error = %e, "X-Plane-Vermessung: Abo nachsenden");
                                    }
                                }
                                if !abos.abgelehnt.is_empty() {
                                    tracing::info!(
                                        abgelehnt = abos.abgelehnt.len(),
                                        "X-Plane-Vermessung: einzelne Datarefs nicht abonnierbar"
                                    );
                                    a2.lock().append(&mut abos.abgelehnt);
                                }
                            }
                        }
                        Ok(Message::Close(_)) => break,
                        Ok(_) => {}
                        Err(tungstenite::Error::Io(e))
                            if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                        Err(e) => {
                            tracing::info!(error = %e, "X-Plane-Vermessung: Verbindung beendet");
                            break;
                        }
                    }
                }
                l2.store(false, Ordering::SeqCst);
                let _ = ws.close(None);
            })
            .map_err(|e| e.to_string())?;

        Ok(Spiegel {
            art: Art::WebApi(WebSpiegel {
                werte,
                namen,
                stop,
                lebt,
                faden: Some(faden),
                abgelehnt,
                abonniert: ids.len(),
            }),
            flugzeug,
            abonniert: ids.len(),
        })
    }

    /// Stand der Anmeldung für den Bericht.
    pub fn abo_stand(&self) -> AboStand {
        match &self.art {
            Art::WebApi(w) => {
                let abgelehnt = w.abgelehnt.lock();
                AboStand {
                    angemeldet: w.abonniert,
                    angekommen: w.werte.lock().len(),
                    abgelehnt: abgelehnt.len(),
                    abgelehnt_namen: abgelehnt
                        .iter()
                        .filter_map(|id| w.namen.get(id).cloned())
                        .take(50)
                        .collect(),
                    quelle: "web_api",
                }
            }
            Art::Plugin(p) => p.messung.abo_stand(),
        }
    }

    /// Wie viele Datarefs schon einen Wert geliefert haben.
    pub fn verbunden(&self) -> usize {
        match &self.art {
            Art::WebApi(w) => w.werte.lock().len(),
            Art::Plugin(p) => p.messung.verbunden(),
        }
    }

    /// Steht die Verbindung zu X-Plane noch?
    pub fn lebt(&self) -> bool {
        match &self.art {
            Art::WebApi(w) => w.lebt.load(Ordering::SeqCst),
            Art::Plugin(p) => p.zugang.messung_aktiv(&p.messung),
        }
    }

    /// Alle aktuellen Werte, Arrays elementweise.
    pub fn schnappschuss(&self) -> HashMap<String, f64> {
        match &self.art {
            Art::WebApi(w) => flach(&w.werte.lock(), &w.namen),
            Art::Plugin(p) => p.messung.schnappschuss(),
        }
    }
}

// ─── Messung über das Plugin (Protokoll 2) ────────────────────────────────

/// Höchstens so viele Namen (Abos 3–16 zu je 8192).
const MAX_MESSNAMEN: usize = (crate::plugin2::MAX_ABO_ID - ABO_MESSUNG_AB + 1) as usize * MAX_NAMEN;
/// So lange auf die vollständige `LISTE` warten (je Versuch).
const LISTE_WARTEN: Duration = Duration::from_secs(8);

fn plugin_starten(zugang: PluginZugang) -> Result<Spiegel, String> {
    let mut namen = None;
    // Zwei Versuche: bei vielen Teilen kann auf dem Loopback einer
    // verloren gehen.
    for _ in 0..2 {
        let m = zugang.messung_starten(|id| Arc::new(P2Messung::neu(id)));
        let ende = std::time::Instant::now() + LISTE_WARTEN;
        let ergebnis = loop {
            std::thread::sleep(Duration::from_millis(50));
            if !zugang.messung_aktiv(&m) {
                break Some(Err("Plugin-Sitzung beendet".to_string()));
            }
            if let Some(r) = m.liste_ergebnis() {
                break Some(r);
            }
            if std::time::Instant::now() >= ende {
                break None;
            }
        };
        match ergebnis {
            Some(Ok(n)) => {
                namen = Some((m, n));
                break;
            }
            Some(Err(e)) => {
                zugang.messung_beenden(&m);
                return Err(e);
            }
            None => {
                tracing::info!(
                    "X-Plane-Vermessung: Namensliste des Plugins unvollständig — neuer Versuch"
                );
                zugang.messung_beenden(&m);
            }
        }
    }
    let Some((m, n)) = namen else {
        return Err("Namensliste des Plugins kam nicht vollständig an".into());
    };
    let anzahl = m.abonnieren(n);
    if anzahl == 0 {
        zugang.messung_beenden(&m);
        return Err("Das Plugin meldet keine Werte — ist ein Flugzeug geladen?".into());
    }
    zugang.wunsch_geaendert();
    // Auf den Status aller Mess-Abos warten. Die Sitzung wartet je Abo mit
    // der Namenszahl wachsend und wiederholt einmal; kommt dann nichts, meldet
    // sie `AboOhneAntwort` — dieser Lauf nimmt dann die Web-API.
    let groesstes = m.teile().iter().map(|t| t.len()).max().unwrap_or(0);
    let schub = Duration::from_millis(anzahl as u64 / 8 + 20 * m.teile().len() as u64);
    let ende = std::time::Instant::now()
        + crate::plugin2::bestaetigung_fuer(groesstes) * 2
        + schub
        + Duration::from_secs(3);
    loop {
        std::thread::sleep(Duration::from_millis(50));
        if !zugang.messung_aktiv(&m) {
            zugang.messung_beenden(&m);
            return Err("Plugin-Sitzung beendet".into());
        }
        match m.bereit() {
            Some(Ok(())) => break,
            Some(Err(e)) => {
                zugang.messung_beenden(&m);
                return Err(e);
            }
            None if std::time::Instant::now() >= ende => {
                zugang.messung_beenden(&m);
                return Err("Plugin bestätigt die Mess-Abos nicht".into());
            }
            None => {}
        }
    }
    // Flugzeug: Meldung des Plugins; Autor (nur Web-API) dazu, wenn die
    // Web-API dasselbe Flugzeug meint.
    let mut flugzeug = zugang.flugzeug().unwrap_or_default();
    if let Ok(web) = WebApiClient::mit_basis(&format!("http://{}", zugang.web_api_host()))
        .fetch_aircraft_info(&mut DrefIdCache::default())
    {
        if flugzeug.relative_path.is_none() || flugzeug.relative_path == web.relative_path {
            flugzeug.author = web.author;
            flugzeug.studio = web.studio;
            flugzeug.tailnum = web.tailnum;
            if flugzeug.descrip.is_none() {
                flugzeug.descrip = web.descrip;
            }
            if flugzeug.icao.is_none() {
                flugzeug.icao = web.icao;
            }
            if flugzeug.relative_path.is_none() {
                flugzeug.relative_path = web.relative_path;
            }
        }
    }
    tracing::info!(
        namen = anzahl,
        "X-Plane-Vermessung über das Plugin (Protokoll 2)"
    );
    Ok(Spiegel {
        art: Art::Plugin(PluginSpiegel { zugang, messung: m }),
        flugzeug,
        abonniert: anzahl,
    })
}

#[derive(Default)]
struct ListeStand {
    teile: Option<u32>,
    stuecke: BTreeMap<u32, Vec<String>>,
    fehler: Option<String>,
}

#[derive(Default)]
struct MessDaten {
    namen: Vec<String>,
    status: Vec<Option<NameStatus>>,
    werte: Vec<Option<P2Wert>>,
}

/// Eine Messung über das Plugin: `LISTE` sammeln, dann Status und Werte je
/// Name. Der Adapter reicht die Ereignisse der Abos 3–16 hierher.
pub(crate) struct P2Messung {
    pub(crate) liste_id: u32,
    liste: Mutex<ListeStand>,
    /// Namen je Abo (Abo `ABO_MESSUNG_AB + k` = `teile[k]`).
    teile: Mutex<Vec<Arc<Vec<String>>>>,
    daten: Mutex<MessDaten>,
    /// Abos (Index in `teile`), zu denen ein Status kam.
    bestaetigt: Mutex<std::collections::HashSet<usize>>,
    /// Ein Mess-Abo blieb ohne Status oder wurde abgelehnt.
    gescheitert: Mutex<Option<String>>,
}

impl P2Messung {
    pub(crate) fn neu(liste_id: u32) -> Self {
        Self {
            liste_id,
            liste: Mutex::new(ListeStand::default()),
            teile: Mutex::new(Vec::new()),
            daten: Mutex::new(MessDaten::default()),
            bestaetigt: Mutex::new(std::collections::HashSet::new()),
            gescheitert: Mutex::new(None),
        }
    }

    /// Ein Mess-Abo bleibt ohne Status (bzw. das Plugin lehnt es ab).
    pub(crate) fn gescheitert_setzen(&self, grund: String) {
        let mut g = self.gescheitert.lock();
        if g.is_none() {
            *g = Some(grund);
        }
    }

    /// `Some(Ok)` wenn zu jedem Mess-Abo ein Status kam, `Some(Err)` wenn
    /// eines scheiterte, sonst `None` (noch warten).
    fn bereit(&self) -> Option<Result<(), String>> {
        if let Some(g) = self.gescheitert.lock().clone() {
            return Some(Err(format!("Plugin: {g}")));
        }
        let n = self.teile.lock().len();
        (self.bestaetigt.lock().len() >= n).then_some(Ok(()))
    }

    pub(crate) fn liste_teil(&self, id: u32, teil: u32, teile: u32, namen: Vec<String>) {
        if id != self.liste_id || teil == 0 || teil > teile {
            return;
        }
        let mut l = self.liste.lock();
        l.teile = Some(teile.max(1));
        l.stuecke.insert(teil, namen);
    }

    pub(crate) fn liste_fehler(&self, grund: String) {
        self.liste.lock().fehler = Some(grund);
    }

    /// `None`, solange Teile fehlen.
    fn liste_ergebnis(&self) -> Option<Result<Vec<String>, String>> {
        let l = self.liste.lock();
        if let Some(f) = &l.fehler {
            return Some(Err(format!("Plugin: {f}")));
        }
        let t = l.teile?;
        if !(1..=t).all(|i| l.stuecke.contains_key(&i)) {
            return None;
        }
        Some(Ok(l.stuecke.values().flatten().cloned().collect()))
    }

    /// Namen festlegen (gültige, ohne Doppelte, höchstens [`MAX_MESSNAMEN`])
    /// und in Abos zu je [`MAX_NAMEN`] teilen. Liefert die Anzahl.
    fn abonnieren(&self, namen: Vec<String>) -> usize {
        let mut gesehen = std::collections::HashSet::new();
        let mut n: Vec<String> = namen
            .into_iter()
            .filter(|x| name_gueltig(x) && gesehen.insert(x.clone()))
            .collect();
        if n.len() > MAX_MESSNAMEN {
            tracing::warn!(
                namen = n.len(),
                "X-Plane-Vermessung: mehr Namen als Abos — gekürzt"
            );
            n.truncate(MAX_MESSNAMEN);
        }
        *self.teile.lock() = n.chunks(MAX_NAMEN).map(|c| Arc::new(c.to_vec())).collect();
        let mut d = self.daten.lock();
        d.status = vec![None; n.len()];
        d.werte = vec![None; n.len()];
        d.namen = n;
        d.namen.len()
    }

    pub(crate) fn teile(&self) -> Vec<Arc<Vec<String>>> {
        self.teile.lock().clone()
    }

    /// Anfang des Abos in der Gesamtliste — nur, wenn `namen` noch genau
    /// die Liste dieses Abos ist.
    fn anfang(&self, abo: u8, namen: &Arc<Vec<String>>) -> Option<usize> {
        let k = usize::from(abo.checked_sub(ABO_MESSUNG_AB)?);
        let t = self.teile.lock();
        let teil = t.get(k)?;
        (Arc::ptr_eq(teil, namen) || **teil == **namen).then_some(k * MAX_NAMEN)
    }

    pub(crate) fn status(&self, abo: u8, namen: &Arc<Vec<String>>, st: Vec<(usize, NameStatus)>) {
        let Some(a) = self.anfang(abo, namen) else {
            return;
        };
        self.bestaetigt.lock().insert(a / MAX_NAMEN);
        let mut d = self.daten.lock();
        for (li, s) in st {
            if let Some(slot) = d.status.get_mut(a + li) {
                *slot = Some(s);
            }
            if !s.da() {
                if let Some(w) = d.werte.get_mut(a + li) {
                    *w = None;
                }
            }
        }
    }

    pub(crate) fn werte(&self, abo: u8, namen: &Arc<Vec<String>>, v: Vec<(usize, P2Wert)>) {
        let Some(a) = self.anfang(abo, namen) else {
            return;
        };
        let mut d = self.daten.lock();
        for (li, w) in v {
            if let Some(slot) = d.werte.get_mut(a + li) {
                *slot = Some(w);
            }
        }
    }

    fn ist_text(s: Option<NameStatus>) -> bool {
        matches!(
            s,
            Some(NameStatus::Da {
                typ: Typ::Bytes | Typ::Unbekannt,
                ..
            })
        )
    }

    /// Alle Zahlen, Arrays als `name[i]` (bis [`MAX_ARRAY`]) — wie der
    /// Web-API-Spiegel.
    fn schnappschuss(&self) -> HashMap<String, f64> {
        let d = self.daten.lock();
        let mut aus = HashMap::with_capacity(d.namen.len());
        for (i, w) in d.werte.iter().enumerate() {
            if Self::ist_text(d.status[i]) {
                continue;
            }
            match w {
                Some(P2Wert::Zahl(x)) if x.is_finite() => {
                    aus.insert(d.namen[i].clone(), *x);
                }
                Some(P2Wert::Liste(l)) => {
                    for (j, x) in l.iter().take(MAX_ARRAY).enumerate() {
                        if x.is_finite() {
                            aus.insert(format!("{}[{j}]", d.namen[i]), *x);
                        }
                    }
                }
                _ => {}
            }
        }
        aus
    }

    fn zahl_wert(w: &Option<P2Wert>) -> bool {
        matches!(w, Some(P2Wert::Zahl(_) | P2Wert::Liste(_)))
    }

    fn verbunden(&self) -> usize {
        let d = self.daten.lock();
        d.werte
            .iter()
            .zip(&d.status)
            .filter(|(w, s)| Self::zahl_wert(w) && !Self::ist_text(**s))
            .count()
    }

    /// Wie bei der Web-API: angemeldet = Zahlen-Namen (Text-Datarefs zählen
    /// nicht, die Web-API meldet sie gar nicht erst an), abgelehnt = vom
    /// Plugin als „fehlt" gemeldet.
    fn abo_stand(&self) -> AboStand {
        let d = self.daten.lock();
        let fehlt: Vec<&String> = d
            .status
            .iter()
            .enumerate()
            .filter(|(_, s)| **s == Some(NameStatus::Fehlt))
            .map(|(i, _)| &d.namen[i])
            .collect();
        AboStand {
            angemeldet: d.status.iter().filter(|s| !Self::ist_text(**s)).count(),
            angekommen: d
                .werte
                .iter()
                .zip(&d.status)
                .filter(|(w, s)| Self::zahl_wert(w) && !Self::ist_text(**s))
                .count(),
            abgelehnt: fehlt.len(),
            abgelehnt_namen: fehlt.into_iter().take(50).cloned().collect(),
            quelle: "plugin",
        }
    }
}

impl Drop for WebSpiegel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(f) = self.faden.take() {
            let _ = f.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abo_in_paketen_zu_500() {
        let ids: Vec<i64> = (1..=1203).collect();
        let n = abo_nachrichten(&ids);
        assert_eq!(n.len(), 3);
        let erste: serde_json::Value = serde_json::from_str(&n[0]).unwrap();
        assert_eq!(erste["type"], "dataref_subscribe_values");
        assert_eq!(erste["params"]["datarefs"].as_array().unwrap().len(), 500);
        let letzte: serde_json::Value = serde_json::from_str(&n[2]).unwrap();
        assert_eq!(letzte["params"]["datarefs"].as_array().unwrap().len(), 203);
    }

    /// Nachrichten so, wie X-Plane 12.4.3 sie am 27.09.2026 schickte.
    #[test]
    fn updates_werden_gespiegelt() {
        let mut w = HashMap::new();
        assert_eq!(
            nachricht_uebernehmen(r#"{"req_id":1,"success":true,"type":"result"}"#, &mut w),
            0
        );
        let n = nachricht_uebernehmen(
            r#"{"data":{"500556762880":0,"12":[1.0,0.5,2],"13":"QTMzMw=="},"type":"dataref_update_values"}"#,
            &mut w,
        );
        assert_eq!(n, 2, "Text-Werte (base64) werden nicht gespiegelt");
        nachricht_uebernehmen(
            r#"{"data":{"500556762880":1},"type":"dataref_update_values"}"#,
            &mut w,
        );
        let namen: HashMap<i64, String> = [
            (
                500556762880,
                "sim/cockpit2/switches/strobe_lights_on".to_string(),
            ),
            (12, "laminar/a333/arr".to_string()),
        ]
        .into_iter()
        .collect();
        let s = flach(&w, &namen);
        assert_eq!(s.get("sim/cockpit2/switches/strobe_lights_on"), Some(&1.0));
        assert_eq!(s.get("laminar/a333/arr[1]"), Some(&0.5));
        assert_eq!(s.get("laminar/a333/arr[2]"), Some(&2.0));
        assert_eq!(s.len(), 4);
    }

    /// Ein abgelehntes Paket wird halbiert, bis nur der störrische Dataref
    /// übrig bleibt — alle anderen werden angemeldet (ToLiss 28.09.2026).
    #[test]
    fn abgelehntes_paket_wird_bis_zum_stoerer_halbiert() {
        let ids: Vec<i64> = (1..=8).collect();
        let (mut a, erste) = Abos::neu(&ids);
        assert_eq!(erste.len(), 1);
        let stoerer = 6;
        // X-Plane nachspielen: jede Anfrage mit dem Störer scheitert ganz.
        let mut warteschlange = erste;
        let mut angemeldet: Vec<i64> = Vec::new();
        let mut runden = 0;
        while let Some(n) = warteschlange.pop() {
            runden += 1;
            assert!(runden < 50, "halbiert nicht");
            let v: serde_json::Value = serde_json::from_str(&n).unwrap();
            let req = v["req_id"].as_u64().unwrap();
            let drin: Vec<i64> = v["params"]["datarefs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|d| d["id"].as_i64().unwrap())
                .collect();
            let ok = !drin.contains(&stoerer);
            if ok {
                angemeldet.extend(&drin);
            }
            warteschlange.extend(a.antwort(&format!(
                r#"{{"req_id":{req},"success":{ok},"type":"result"}}"#
            )));
        }
        angemeldet.sort_unstable();
        assert_eq!(angemeldet, vec![1, 2, 3, 4, 5, 7, 8]);
        assert_eq!(a.abgelehnt, vec![stoerer]);
        assert!(a.offen.is_empty());
    }

    #[test]
    fn erfolg_und_fremde_antworten_senden_nichts_nach() {
        let (mut a, _) = Abos::neu(&[1, 2, 3]);
        assert!(a
            .antwort(r#"{"req_id":99,"success":false,"type":"result"}"#)
            .is_empty());
        assert!(a
            .antwort(r#"{"data":{"1":0},"type":"dataref_update_values"}"#)
            .is_empty());
        assert!(a
            .antwort(r#"{"req_id":1,"success":true,"type":"result"}"#)
            .is_empty());
        assert!(a.offen.is_empty());
    }

    /// Gegen ein laufendes X-Plane (Web-API auf 8086): meldet alles an wie
    /// die Messung und zählt, was abgelehnt wird und was ankommt. Nur von
    /// Hand: `cargo test -p sim-xplane live_abo_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_abo_probe() {
        let liste: Liste = ureq::get(&format!("http://{HOST}/api/v2/datarefs"))
            .call()
            .unwrap()
            .into_json()
            .unwrap();
        let zahlen: Vec<Eintrag> = liste
            .data
            .into_iter()
            .filter(|e| ist_zahl(&e.value_type))
            .collect();
        let namen: HashMap<i64, String> = zahlen.iter().map(|e| (e.id, e.name.clone())).collect();
        let ids: Vec<i64> = zahlen.iter().map(|e| e.id).collect();
        let strom = TcpStream::connect(HOST).unwrap();
        strom
            .set_read_timeout(Some(Duration::from_millis(400)))
            .unwrap();
        let (mut ws, _) = tungstenite::client(format!("ws://{HOST}/api/v2"), strom).unwrap();
        let (mut abos, erste) = Abos::neu(&ids);
        let erste_pakete = erste.len() as u64;
        for n in erste {
            ws.send(Message::text(n)).unwrap();
        }
        let mut werte = HashMap::new();
        let mut erste_abgelehnt = 0usize;
        let mut pakete_abgelehnt = 0usize;
        let ende = std::time::Instant::now() + Duration::from_secs(20);
        while std::time::Instant::now() < ende {
            match ws.read() {
                Ok(Message::Text(t)) => {
                    if nachricht_uebernehmen(t.as_str(), &mut werte) == 0 {
                        let v: serde_json::Value = serde_json::from_str(t.as_str()).unwrap();
                        if v["type"] == "result" && v["success"] == false {
                            pakete_abgelehnt += 1;
                            let r = v["req_id"].as_u64().unwrap_or(0);
                            if r <= erste_pakete {
                                erste_abgelehnt += abos.offen.get(&r).map_or(0, |x| x.len());
                            }
                            println!("abgelehnt req {r}: {}", t.as_str());
                        }
                        for n in abos.antwort(t.as_str()) {
                            ws.send(Message::text(n)).unwrap();
                        }
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => panic!("{e}"),
            }
        }
        println!(
            "angemeldet {} | angekommen {} | bisher (ohne Nachsenden) verloren {} | Pakete abgelehnt {} | einzeln nicht abonnierbar {} | noch offen {}",
            ids.len(),
            werte.len(),
            erste_abgelehnt,
            pakete_abgelehnt,
            abos.abgelehnt.len(),
            abos.offen.len()
        );
        for id in ids.iter().filter(|i| !werte.contains_key(i)).take(30) {
            println!(
                "  nie angekommen: {} ({})",
                namen.get(id).map_or("?", |s| s.as_str()),
                zahlen
                    .iter()
                    .find(|e| e.id == *id)
                    .map_or("?", |e| e.value_type.as_str())
            );
        }
        for id in abos.abgelehnt.iter().take(30) {
            println!(
                "  nicht abonnierbar: {}",
                namen.get(id).map_or("?", |s| s.as_str())
            );
        }
    }

    /// Plugin-Messung: Liste in beliebiger Teilreihenfolge, erst vollständig
    /// ein Ergebnis; fremde Anfrage-IDs zählen nicht.
    #[test]
    fn plugin_liste_wird_zusammengesetzt() {
        let m = P2Messung::neu(7);
        m.liste_teil(7, 2, 2, vec!["sim/c".into()]);
        assert!(m.liste_ergebnis().is_none());
        m.liste_teil(8, 1, 2, vec!["fremd/a".into()]);
        assert!(m.liste_ergebnis().is_none());
        m.liste_teil(7, 1, 2, vec!["sim/a".into(), "sim/b".into()]);
        assert_eq!(
            m.liste_ergebnis(),
            Some(Ok(vec!["sim/a".into(), "sim/b".into(), "sim/c".into()]))
        );
        let f = P2Messung::neu(1);
        f.liste_fehler("liste_nicht_verfuegbar".into());
        assert!(matches!(f.liste_ergebnis(), Some(Err(_))));
    }

    /// Mehr als 8192 Namen → mehrere Abos; Status und Werte landen am
    /// richtigen Namen, „fehlt" zählt als abgelehnt, Text-Datarefs zählen
    /// nicht, Arrays elementweise.
    #[test]
    fn plugin_messung_ordnet_werte_zu() {
        let m = P2Messung::neu(1);
        let mut namen: Vec<String> = (0..9000).map(|i| format!("sim/wert/{i}")).collect();
        namen.push("sim/wert/0".into()); // doppelt
        namen.push("kaputt name".into()); // ungültig
        assert_eq!(m.abonnieren(namen), 9000);
        let teile = m.teile();
        assert_eq!(teile.len(), 2);
        assert_eq!(teile[0].len(), MAX_NAMEN);
        // Abo 4 = zweiter Teil; lokaler Index 5 = Name 8197.
        m.status(
            ABO_MESSUNG_AB + 1,
            &teile[1],
            vec![
                (
                    5,
                    NameStatus::Da {
                        typ: Typ::Float,
                        laenge: 1,
                    },
                ),
                (6, NameStatus::Fehlt),
                (
                    7,
                    NameStatus::Da {
                        typ: Typ::Bytes,
                        laenge: 40,
                    },
                ),
                (
                    8,
                    NameStatus::Da {
                        typ: Typ::FloatArray,
                        laenge: 3,
                    },
                ),
            ],
        );
        m.werte(
            ABO_MESSUNG_AB + 1,
            &teile[1],
            vec![
                (5, P2Wert::Zahl(1.25)),
                (7, P2Wert::Text("A20N".into())),
                (8, P2Wert::Liste(vec![0.0, f64::NAN, 2.0])),
            ],
        );
        // Werte zu einer veralteten Liste werden verworfen.
        m.werte(
            ABO_MESSUNG_AB + 1,
            &Arc::new(vec!["sim/anders".into()]),
            vec![(0, P2Wert::Zahl(9.0))],
        );
        let s = m.schnappschuss();
        assert_eq!(s.get("sim/wert/8197"), Some(&1.25));
        assert_eq!(s.get("sim/wert/8200[0]"), Some(&0.0));
        assert_eq!(s.get("sim/wert/8200[2]"), Some(&2.0));
        assert_eq!(s.len(), 3);
        let a = m.abo_stand();
        assert_eq!(a.angemeldet, 8999, "Text-Dataref zählt nicht");
        assert_eq!(a.angekommen, 2);
        assert_eq!(a.abgelehnt, 1);
        assert_eq!(a.abgelehnt_namen, vec!["sim/wert/8198".to_string()]);
        assert_eq!(m.verbunden(), 2);
    }

    /// Bereit erst mit Status zu JEDEM Mess-Abo; gescheitert schlägt alles.
    #[test]
    fn plugin_messung_bereit_oder_gescheitert() {
        let m = P2Messung::neu(1);
        m.abonnieren((0..9000).map(|i| format!("sim/wert/{i}")).collect());
        let teile = m.teile();
        assert_eq!(m.bereit(), None);
        m.status(ABO_MESSUNG_AB, &teile[0], vec![(0, NameStatus::Fehlt)]);
        assert_eq!(m.bereit(), None);
        m.status(ABO_MESSUNG_AB + 1, &teile[1], vec![(0, NameStatus::Fehlt)]);
        assert_eq!(m.bereit(), Some(Ok(())));
        m.gescheitert_setzen("Abo 4 ohne Status".into());
        m.gescheitert_setzen("zweiter Grund".into());
        assert_eq!(m.bereit(), Some(Err("Plugin: Abo 4 ohne Status".into())));
    }

    #[test]
    fn nur_zahlen_typen() {
        assert!(ist_zahl("int") && ist_zahl("float_array") && ist_zahl("double"));
        assert!(!ist_zahl("data"));
    }

    #[test]
    fn lange_arrays_werden_gekappt() {
        let mut w = HashMap::new();
        let arr: Vec<String> = (0..100).map(|i| i.to_string()).collect();
        nachricht_uebernehmen(
            &format!(
                r#"{{"data":{{"7":[{}]}},"type":"dataref_update_values"}}"#,
                arr.join(",")
            ),
            &mut w,
        );
        let namen: HashMap<i64, String> = [(7, "x".to_string())].into_iter().collect();
        assert_eq!(flach(&w, &namen).len(), MAX_ARRAY);
    }
}
