//! Bilder-Tab (N-13, Faehigkeit `bilder-1`, P-11.30 bis P-11.34): die dreizehn
//! Befehle, der dauerhafte Eingang `.eingang/`, das Ereignis `bild_quelle_neu`
//! und das Tastenkuerzel. **Diese Kopf-Doku ist der Vertrag fuer das Paket.**
//!
//! Grundsatz (P-11.33): Dieser Teil liefert nur Plattform-Bausteine. Zuordnung,
//! Metadaten, Verkleinern, EXIF und Filter sind Sache des Laufzeit-Pakets. Die
//! Logik steht in `crates/bilder-kern` (ohne Tauri testbar); die Befehle hier
//! sind duenne Adapter. Was der Kern nicht dem Paket ueberlassen darf, erzwingt
//! er selbst: Pfade nur in bestaetigten Quellen und ohne Linkfolgen, Funde nur
//! aus einem aktiven AeroACARS-Flug, Loeschen und Senden nur nach Bestaetigung
//! der Huelle.
//!
//! # Eingang (dauerhaft)
//! Jeder Fund im aktiven Flug (neue Datei in einer Quelle, Aufnahme per
//! Tastenkuerzel oder `bild_aufnehmen`) wird **zuerst** als
//! `<bilder>/.eingang/<fund_id>.json` abgelegt, mit dem Stand zur Entdeckung.
//! Ausserhalb eines Flugs entsteht kein Fund (die Datei bleibt unangetastet).
//! `bilder_liste` liefert die offenen Funde als `funde` (aelteste zuerst). Die
//! Ereignisse `bild_quelle_neu` und `bild_aufgenommen` sind nur Wecksignale:
//! ohne sie, auch bei geschlossenem Tab oder neu gestartetem Client, ist der
//! Eingang vollstaendig. Das Paket arbeitet ihn strikt nacheinander ab.
//! Eintrag (flach): `fund_id`, `art` (`datei`|`aufnahme`), `entdeckt_ms`,
//! `pfad` (Quelldatei, nur `datei`), `quelle` (Quellordner), `quelle_art`
//! (`standard`|`aufgespuert`|`hand`|frei), `quelle_pfad` (Eingangskopie
//! `<gruppe>/<id>.quelle.png`, nur `aufnahme`), `dateizeit_ms`, `erstellt_ms`,
//! `groesse`, `format`, `uebernehmbar`, `bild_breite`, `bild_hoehe` (Pixel),
//! `sha256` (der Originaldatei), `sim_vordergrund` und der *Stand* zur
//! Entdeckung (Felder von `bild_meta`, P-11.32) flach daneben.
//!
//! *Stand* (gleiche Namen im Eingang und in `bild_kontext`): `pirep_id`, `phase`,
//! `rufzeichen`, `abflug`, `ziel`, `zeit_utc`, `breite`, `laenge`, `hoehe_ft`,
//! `gs_kt`, `kurs`, `flugzeug_titel`, `flugzeug_icao`, `simulator`,
//! `naechster_flugplatz` (`{icao, abstand_m}`). Fehlende Werte fehlen (`null`),
//! nichts wird geraten. `breite`/`laenge` sind die Position, nicht die Bildmasse.
//!
//! # Befehle
//! Byte-Vertrag: `bild_lesen` gibt Bytes **roh** zurueck (`ArrayBuffer`);
//! `bild_schreiben` und `bild_export` nehmen Base64 (Standard-Alphabet);
//! `bild_senden` nimmt **keine Bytes mehr** (der Kern liest die Dateien selbst).
//! Argumente: oberste Ebene camelCase, Felder verschachtelter Objekte snake_case.
//!
//! lesen
//! * `bilder_liste({mitAltbestand?})` -> `{eintraege, belegt_bytes, funde, quellen, tastenkuerzel, aufnahme_verfuegbar, altbestand?}`;
//!   `quellen[]` tragen `art` (`standard`, `aufgespuert`, `hand`, sonst frei) und `wird_geprueft`
//! * `bild_lesen({pfad})` -> ArrayBuffer (`pfad` relativ aus der Liste)
//! * `bild_quellen_finden()` -> `[{pfad, art, simulator, existiert, vorschlag_modus}]`
//! * `bild_status({sendungId?})` -> Stand einer Sendung und Kontingent (`GET /api/bilder/status`)
//! * `bild_kontext({zeiten?: [iso], pirepId?, mitSpur?})` -> `{ergebnisse: [{zeit, quelle, abstand_s?, ...Stand}], spur?, pirep_id?}`;
//!   je Zeit der zeitlich naechste Stand aus `flight_logs/<pirep>.jsonl` (`quelle: "protokoll"`, hoechstens
//!   120 s Abstand), sonst die PIREP-Block-Zeiten aus phpVMS (`"logbuch"`: `pirep_id`, `rufzeichen`, `abflug`,
//!   `ziel`, `flugzeug_icao`, sonst nichts), sonst `"keine"`; `naechster_flugplatz` aus dem OurAirports-Stand der
//!   aktiven Generation. `mitSpur`: ausgeduennte Spur des Flugs `[{zeit_utc, breite, laenge, hoehe_ft}]`
//!   (hoechstens 600 Punkte; im aktiven Flug stattdessen `flight_get_track`)
//!
//! fachlich
//! * `bild_uebernehmen({fundId, modus, gruppe?, id?})` -> `Uebernahme`: `modus` `kopieren`|`verschieben`|`verwerfen`.
//!   Prueft, dass der Fund im aktiven Flug entstand (er steht im Eingang); die Uebernahme selbst darf nach
//!   Flugende laufen. Ergebnis: Eingangskopie `<gruppe>/<id>.quelle.png|jpg` (`gruppe` Pflicht), der Eintrag
//!   wird entfernt. `verwerfen` entfernt nur den Eintrag und fasst die Quelldatei nie an (eine eigene
//!   Aufnahme-Kopie wird mit entfernt). Es gibt kein `bild_verwerfen`.
//! * `bild_schreiben({gruppe, id, jpegBase64?, vorschauBase64?, sidecar?, quelleEntfernen?, sha256?})` -> Eintrag;
//!   `sha256` (kanonischer Hash der EXIF-freien Bytes, 64 Hex) wird im Sidecar festgehalten; ein im Bilderordner
//!   schon vorhandener Hash anderer Bild-id wird atomar mit Code `duplikat` abgelehnt (Meldung nennt `gruppe/id`)
//! * `bild_aufnehmen({gruppe?})` -> Fund (wie ein Eintrag); nur im aktiven Flug (`kein_flug`)
//!
//! system (Huellen-Dialog)
//! * `bild_senden({sendungId, text?, geoStufe?, bilder: [{gruppe, bild_id, meta}]})` -> Antwort des Servers (`POST /api/bilder`).
//!   Der Kern liest `<gruppe>/<bild_id>.jpg`, entfernt **alle APP1-Segmente** (EXIF, XMP) mit einem begrenzten
//!   JPEG-Segment-Parser und baut die Anfrage. Gleiche Datei = gleiche Upload-Bytes (kein `konflikt` bei
//!   Wiederholung). Die Huelle zeigt nur die Bildanzahl.
//! * `bild_hinzufuegen({gruppe, pfade?})` -> `{abgebrochen, ergebnisse: [{pfad, ok, uebernahme | code, meldung}]}`;
//!   ohne `pfade` oeffnet der Kern die Dateiauswahl; JPG/PNG, immer kopiert, auch ohne Flug
//! * `bild_loeschen({gruppe, id, bezeichnung?})`, `bild_export({zipBase64, dateiname})`
//! * `bild_quellen_setzen({quellen?, tastenkuerzel?, ordnerWaehlen?})`: `ordnerWaehlen: true` oeffnet nach der
//!   Bestaetigung den Ordnerdialog des Systems und haengt den Ordner als Quelle `hand` an (nur kopieren);
//!   Abbrechen aendert nichts
//!
//! Ereignisse (nur Wecksignale): `bild_quelle_neu` (der Eingangseintrag), `bild_aufgenommen`
//! (`{ok, aufnahme}` oder `{ok:false, code, meldung}`, `code` z. B. `kein_flug`, `verdeckt`).
//!
//! Fehlercodes des Kerns: `bild_name`, `bild_pfad`, `bild_nicht_gefunden`, `bild_format`, `bild_unfertig`,
//! `bild_quelle`, `bild_verboten`, `bild_zu_gross`, `bild_eingabe`, `bild_nicht_verfuegbar`, `bild_io`,
//! `kein_flug`, `verdeckt`, `duplikat`; beim Senden `live_nicht_angemeldet`, `live_nicht_erreichbar`,
//! `live_antwort_fehlt` (Zustellung unklar: erst `bild_status` mit derselben `sendungId`), `live_kontingent`
//! (`details.retry_after`), `live_fehler` (`details.status`).
//!
//! Stufen (befehle.json): lesen `bilder_liste`, `bild_lesen`, `bild_quellen_finden`, `bild_status`,
//! `bild_kontext`; fachlich `bild_uebernehmen`, `bild_schreiben`, `bild_aufnehmen`; system `bild_senden`,
//! `bild_hinzufuegen`, `bild_loeschen`, `bild_quellen_setzen`, `bild_export`. Die LAN-Bruecke reicht keinen
//! davon durch.
//!
//! Auslegung: Weil es genau dreizehn Befehle sind, tragen zwei Antworten mehr als der Name sagt:
//! `bilder_liste` liefert auch Funde, Quellen mit Beobachtungszustand, den Altbestand (auf Wunsch) und das
//! Tastenkuerzel; `bild_quellen_setzen` setzt neben den Ordnern auch das Tastenkuerzel.
//!
//! Beobachtung: jede Quelle hat ihren eigenen Stand (siehe `bilder_kern::beobachter`); die Grundlage einer
//! neuen Quelle laeuft im Hintergrund, `bild_quellen_setzen` wartet nie auf einen haengenden Ordner.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use bilder_kern::beobachter::Beobachter;
use bilder_kern::live::{self, LiveBild};
use bilder_kern::namen::QuellFormat;
use bilder_kern::plattform;
use bilder_kern::quellen::{self, Einstellungen, Modus, Quelle};
use bilder_kern::speicher::{self, SchreibAnfrage};
use bilder_kern::suche::{self, Fundort};
use bilder_kern::{tastenkuerzel, Fehler};
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::UiError;

/// Takt der Ordnerbeobachtung.
const TAKT: Duration = Duration::from_millis(1500);
/// Gruppe der Eingangskopien, wenn der Aufrufer keine nennt (Tastenkuerzel).
const EINGANG: &str = "eingang";

pub(crate) const EREIGNIS_QUELLE_NEU: &str = "bild_quelle_neu";
pub(crate) const EREIGNIS_AUFGENOMMEN: &str = "bild_aufgenommen";

fn fehler(e: Fehler) -> UiError {
    UiError::new(e.code(), e.meldung().to_string())
}

fn entsperren<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn wurzel(app: &AppHandle) -> Result<PathBuf, UiError> {
    app.path()
        .app_data_dir()
        .map(|d| d.join("bilder"))
        .map_err(|e| UiError::new("bild_io", format!("Datenordner nicht ermittelbar: {e}")))
}

fn einstellungen_datei(app: &AppHandle) -> Result<PathBuf, UiError> {
    app.path()
        .app_config_dir()
        .map(|d| d.join("bilder-einstellungen.json"))
        .map_err(|e| UiError::new("bild_io", format!("Konfigurationsordner nicht ermittelbar: {e}")))
}

fn beobachter() -> &'static Mutex<Beobachter> {
    static B: OnceLock<Mutex<Beobachter>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(Beobachter::neu()))
}

/// Abbild fuer die Anzeige: `bilder_liste` wartet so nie auf einen laufenden
/// Ordnerdurchlauf (die erste Grundlage eines grossen Bilder-Ordners kann dauern).
#[derive(Default)]
struct Anzeige {
    status: Vec<bilder_kern::beobachter::QuellenStatus>,
    alt: Vec<bilder_kern::beobachter::AltEintrag>,
}

fn anzeige() -> &'static Mutex<Anzeige> {
    static A: OnceLock<Mutex<Anzeige>> = OnceLock::new();
    A.get_or_init(|| Mutex::new(Anzeige::default()))
}

/// Anzeige-Abbild aus dem Beobachter erneuern (nur Anzeige-Sperren, nie Ordnerzugriffe).
fn anzeige_erneuern() {
    let (status, alt) = {
        let b = entsperren(beobachter());
        (b.status(), b.altbestand())
    };
    *entsperren(anzeige()) = Anzeige { status, alt };
}

/// Beobachtung auf die bestaetigte Liste setzen. Kehrt sofort zurueck: die
/// Grundlage (Altbestand) neuer Quellen erhebt je Quelle ein eigener Faden, ein
/// haengendes Netzlaufwerk haelt also weder das Speichern noch die anderen
/// Quellen auf. Bis dahin steht die Quelle auf `wird_geprueft`.
fn beobachtung_aufsetzen(quellen: &[Quelle]) {
    let vordergrund = plattform::sim_vordergrund();
    let neue = entsperren(beobachter()).einsetzen(quellen);
    anzeige_erneuern();
    for slot in neue {
        let _ = std::thread::Builder::new().name("bilder-grundlage".into()).spawn(move || {
            slot.grundlage(vordergrund);
            anzeige_erneuern();
        });
    }
}

/// Laeuft ein AeroACARS-Flug? Nur dann nimmt der Kern auf und uebernimmt.
fn flug_aktiv(app: &AppHandle) -> bool {
    app.state::<crate::AppState>().active_flight.lock().unwrap_or_else(|e| e.into_inner()).is_some()
}

fn kein_flug() -> UiError {
    fehler(Fehler::KeinFlug("Ohne AeroACARS-Flug wird nichts aufgenommen oder uebernommen".into()))
}

fn hotkey() -> &'static Mutex<Option<plattform::Hotkey>> {
    static H: OnceLock<Mutex<Option<plattform::Hotkey>>> = OnceLock::new();
    H.get_or_init(|| Mutex::new(None))
}

/// Letzter Fehler beim Anmelden des Tastenkuerzels (z. B. „schon belegt“).
fn hotkey_fehler() -> &'static Mutex<Option<String>> {
    static F: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    F.get_or_init(|| Mutex::new(None))
}

fn b64(s: &str, was: &str) -> Result<Vec<u8>, UiError> {
    base64::engine::general_purpose::STANDARD
        .decode(s.trim())
        .map_err(|e| UiError::new("bild_eingabe", format!("{was}: kein gueltiges Base64 ({e})")))
}

fn jetzt_ms() -> i64 {
    speicher::ms_seit_1970(std::time::SystemTime::now()).unwrap_or(0)
}

// ---------------------------------------------------------------- lesen

fn tastenkuerzel_stand(e: &Einstellungen) -> Value {
    json!({
        "text": e.tastenkuerzel,
        "verfuegbar": plattform::AUFNAHME_VERFUEGBAR,
        "aktiv": entsperren(hotkey()).is_some() && entsperren(hotkey_fehler()).is_none(),
        "fehler": entsperren(hotkey_fehler()).clone(),
    })
}

/// Alle Eintraege des verwalteten Ordners ohne Filter, dazu der Stand der
/// Bildquellen und des Tastenkuerzels. `mit_altbestand` liefert zusaetzlich die
/// beim Aufsetzen schon vorhandenen Bilder der aktiven Quellen (neueste zuerst,
/// hoechstens 5000).
#[tauri::command(async)]
pub(crate) fn bilder_liste(app: AppHandle, mit_altbestand: Option<bool>) -> Result<Value, UiError> {
    let wurzel = wurzel(&app)?;
    let liste = speicher::liste(&wurzel);
    let einst = quellen::einstellungen_lesen(&einstellungen_datei(&app)?);
    let a = entsperren(anzeige());
    let mut v = json!({
        "eintraege": liste.eintraege,
        "belegt_bytes": liste.belegt_bytes,
        "quellen": a.status,
        "tastenkuerzel": tastenkuerzel_stand(&einst),
        "aufnahme_verfuegbar": plattform::AUFNAHME_VERFUEGBAR,
    });
    if mit_altbestand == Some(true) {
        v["altbestand"] = json!(a.alt);
    }
    Ok(v)
}

/// Bytes eines Bildes, seiner Vorschau oder der Eingangskopie; `pfad` ist der
/// relative Pfad aus `bilder_liste`. Pfad-Ausbruch ist ausgeschlossen.
#[tauri::command(async)]
pub(crate) fn bild_lesen(app: AppHandle, pfad: String) -> Result<tauri::ipc::Response, UiError> {
    let bytes = speicher::lesen(&wurzel(&app)?, &pfad).map_err(fehler)?;
    Ok(tauri::ipc::Response::new(bytes))
}

/// Windows: Game Bar, Bilder-Ordner (mit OneDrive), Steam, NVIDIA. X-Plane:
/// `Output/screenshots` jeder erkannten Installation (Mac und Windows). Ohne
/// Treffer eine leere Liste, nie ein Fehler.
#[tauri::command(async)]
pub(crate) fn bild_quellen_finden() -> Vec<Fundort> {
    let mut installs: Vec<PathBuf> = sim_xplane::szenerie::installationen();
    if let Some(p) = crate::xplane_plugin_install::detect_install_path() {
        if !installs.contains(&p) {
            installs.push(p);
        }
    }
    let mut v = suche::xplane_fundorte(&installs);
    for f in plattform::windows_fundorte() {
        if !v.iter().any(|g| g.pfad == f.pfad) {
            v.push(f);
        }
    }
    v
}

// ------------------------------------------------------------- fachlich

/// Nimmt eine Datei aus einer bestaetigten, aktiven Quelle in den Bilderordner
/// (`<gruppe>/<id>.quelle.png|jpg`). `kopieren` laesst das Original,
/// `verschieben` entfernt es und geht nur bei Quellen, die der Pilot dafuer
/// freigegeben hat. Danach liest das Paket die Eingangskopie mit `bild_lesen`
/// und legt das JPEG mit `bild_schreiben` ab.
#[tauri::command(async)]
pub(crate) fn bild_uebernehmen(
    app: AppHandle,
    pfad: String,
    modus: Modus,
    gruppe: String,
    id: Option<String>,
) -> Result<quellen::Uebernahme, UiError> {
    let einst = quellen::einstellungen_lesen(&einstellungen_datei(&app)?);
    let lage = quellen::Lage { flug_aktiv: flug_aktiv(&app), stabil_pause: Duration::from_millis(400) };
    quellen::uebernehmen(&wurzel(&app)?, &einst.quellen, &pfad, modus, &gruppe, id.as_deref(), lage).map_err(fehler)
}

/// Schreibt JPEG, Vorschau und Sidecar eines Bildes atomar in den verwalteten
/// Ordner. Ein neues Bild braucht JPEG und Sidecar; danach geht jeder Teil
/// einzeln (z. B. nur das Sidecar fuer Text oder Favorit).
#[tauri::command(async)]
pub(crate) fn bild_schreiben(
    app: AppHandle,
    gruppe: String,
    id: String,
    jpeg_base64: Option<String>,
    vorschau_base64: Option<String>,
    sidecar: Option<Value>,
    quelle_entfernen: Option<bool>,
) -> Result<speicher::Eintrag, UiError> {
    let anfrage = SchreibAnfrage {
        gruppe,
        id,
        jpeg: jpeg_base64.as_deref().map(|s| b64(s, "jpeg_base64")).transpose()?,
        vorschau: vorschau_base64.as_deref().map(|s| b64(s, "vorschau_base64")).transpose()?,
        sidecar,
        quelle_entfernen: quelle_entfernen.unwrap_or(false),
    };
    speicher::schreiben(&wurzel(&app)?, anfrage).map_err(fehler)
}

static AUFNAHME_LAEUFT: AtomicBool = AtomicBool::new(false);

/// Nimmt das Simulatorfenster auf (Windows) und legt es als Eingangskopie
/// (PNG) in `gruppe` ab. Auf anderen Systemen: Fehler `bild_nicht_verfuegbar`.
/// Ohne aktiven Flug: `kein_flug`, auch vom Knopf.
fn aufnehmen_intern(app: &AppHandle, gruppe: &str) -> Result<Value, UiError> {
    if !flug_aktiv(app) {
        return Err(kein_flug());
    }
    if AUFNAHME_LAEUFT.swap(true, Ordering::SeqCst) {
        return Err(UiError::new("bild_beschaeftigt", "Eine Aufnahme laeuft schon"));
    }
    let r = (|| -> Result<Value, Fehler> {
        let a = plattform::fenster_aufnehmen()?;
        let zeit = jetzt_ms();
        let png = bilder_kern::png::aus_bgra(a.breite, a.hoehe, &a.bgra)?;
        let id = uuid::Uuid::new_v4().to_string();
        let e = speicher::quelle_schreiben(&wurzel(app).map_err(|u| Fehler::Io(u.message))?, gruppe, &id, QuellFormat::Png, &png)?;
        Ok(json!({
            "id": id,
            "gruppe": gruppe,
            "quelle_pfad": e.quelle_pfad,
            "format": "png",
            "breite": a.breite,
            "hoehe": a.hoehe,
            "groesse": e.quelle_groesse,
            "dateizeit_ms": zeit,
            "methode": a.methode,
        }))
    })();
    AUFNAHME_LAEUFT.store(false, Ordering::SeqCst);
    r.map_err(fehler)
}

#[tauri::command]
pub(crate) async fn bild_aufnehmen(app: AppHandle, gruppe: Option<String>) -> Result<Value, UiError> {
    let gruppe = gruppe.unwrap_or_else(|| EINGANG.to_string());
    tauri::async_runtime::spawn_blocking(move || aufnehmen_intern(&app, &gruppe))
        .await
        .map_err(|e| UiError::new("bild_io", e.to_string()))?
}

/// Nutzlast des Ereignisses `bild_aufgenommen`. Ohne aktiven Flug wird die
/// Aufnahme gar nicht erst versucht (`aufnehmen` laeuft nicht) und `kein_flug`
/// gemeldet.
fn aufgenommen_nutzlast(flug_aktiv: bool, aufnehmen: impl FnOnce() -> Result<Value, UiError>) -> Value {
    let ergebnis = if flug_aktiv { aufnehmen() } else { Err(kein_flug()) };
    match ergebnis {
        Ok(v) => json!({"ok": true, "aufnahme": v}),
        Err(e) => json!({"ok": false, "code": e.code, "meldung": e.message}),
    }
}

/// Ein Bild der Sendung: die zu sendenden JPEG-Bytes (nach Verkleinern, ohne
/// EXIF; das macht das Paket) und das Metadaten-Dokument.
#[derive(Debug, Deserialize)]
pub(crate) struct LiveBildEin {
    bild_id: String,
    jpeg_base64: String,
    meta: Value,
}

/// Sendet Bilder an `POST /api/bilder` (P-7.25). Stufe **system**: ein
/// gesendetes Bild laesst sich nicht zurueckholen, darum bestaetigt die Huelle
/// (mit der Zahl der Bilder). Das Pilot-Token haengt der Kern an; es erreicht
/// die Oberflaeche nie. Die Antwort des Servers geht unveraendert zurueck.
///
/// Fehlercodes: `live_nicht_angemeldet` (kein Token, 401/403),
/// `live_nicht_erreichbar` (Verbindung kam nicht zustande: Senden wartet),
/// `live_antwort_fehlt` (Verbindung stand, Antwort blieb aus: Zustellung
/// unklar, erst `bild_status` mit derselben `sendung_id` fragen),
/// `live_kontingent` (429, `details.retry_after`), `live_fehler` (alles andere,
/// `details.status`).
#[tauri::command]
pub(crate) async fn bild_senden(
    sendung_id: String,
    text: Option<String>,
    geo_stufe: Option<String>,
    bilder: Vec<LiveBildEin>,
) -> Result<Value, UiError> {
    let anfrage = senden_anfrage(&sendung_id, text.as_deref(), geo_stufe.as_deref(), bilder)?;
    live_mit_token(anfrage).await
}

/// Stand einer Sendung und Kontingent (`GET /api/bilder/status`); ohne
/// `sendung_id` nur das Kontingent. Lesend, braucht keine Bestaetigung.
#[tauri::command]
pub(crate) async fn bild_status(sendung_id: Option<String>) -> Result<Value, UiError> {
    live_mit_token(LiveAnfrage::Status(sendung_id)).await
}

fn senden_anfrage(sendung_id: &str, text: Option<&str>, geo_stufe: Option<&str>, bilder: Vec<LiveBildEin>) -> Result<LiveAnfrage, UiError> {
    let bilder = bilder
        .into_iter()
        .map(|b| Ok(LiveBild { jpeg: b64(&b.jpeg_base64, "jpeg_base64")?, bild_id: b.bild_id, meta: b.meta }))
        .collect::<Result<Vec<_>, UiError>>()?;
    let teile = live::senden_teile(sendung_id, text, geo_stufe.unwrap_or("verdeckt"), &bilder).map_err(fehler)?;
    Ok(LiveAnfrage::Senden(teile))
}

async fn live_mit_token(anfrage: LiveAnfrage) -> Result<Value, UiError> {
    let token = crate::live_zugang::pilot_token().ok_or_else(|| UiError::new("live_nicht_angemeldet", "Noch nicht bei live angemeldet"))?;
    live_anfrage(crate::live_zugang::live_basis(), &token, anfrage).await
}

pub(crate) enum LiveAnfrage {
    Senden(Vec<live::Teil>),
    Status(Option<String>),
}

/// Der eigentliche Aufruf; Adresse und Token kommen von aussen (Test gegen einen lokalen Server).
pub(crate) async fn live_anfrage(basis: &str, token: &str, anfrage: LiveAnfrage) -> Result<Value, UiError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(120))
        .user_agent(concat!("AeroACARS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| UiError::new("live_fehler", e.to_string()))?;
    let req = match anfrage {
        LiveAnfrage::Senden(teile) => {
            let grenze = live::grenze_waehlen(&teile);
            client
                .post(live::senden_url(basis))
                .header(reqwest::header::CONTENT_TYPE, format!("multipart/form-data; boundary={grenze}"))
                .body(live::multipart_bauen(&grenze, &teile))
        }
        LiveAnfrage::Status(sendung) => client.get(live::status_url(basis, sendung.as_deref()).map_err(fehler)?),
    };
    let antwort = req.bearer_auth(token).header(reqwest::header::ACCEPT, "application/json").send().await.map_err(|e| {
        if e.is_connect() {
            UiError::new("live_nicht_erreichbar", e.to_string())
        } else {
            UiError::new("live_antwort_fehlt", e.to_string())
        }
    })?;
    let status = antwort.status().as_u16();
    match status {
        200..=299 => antwort.json::<Value>().await.map_err(|e| UiError::new("live_antwort_fehlt", e.to_string())),
        401 | 403 => Err(UiError::new("live_nicht_angemeldet", format!("HTTP {status}"))),
        _ => {
            let retry = antwort.headers().get(reqwest::header::RETRY_AFTER).and_then(|v| v.to_str().ok()).map(str::to_string);
            let koerper = antwort.text().await.unwrap_or_default();
            let koerper: String = koerper.chars().take(300).collect();
            let code = if status == 429 { "live_kontingent" } else { "live_fehler" };
            Err(UiError::new(code, format!("HTTP {status}")).with_details(json!({"status": status, "retry_after": retry, "antwort": koerper})))
        }
    }
}

// --------------------------------------------------------------- system

/// Papierkorb des Systems. macOS: `NSFileManager` statt der Vorgabe „Finder“
/// (die per AppleScript laeuft und beim ersten Mal die Rueckfrage „AeroACARS
/// moechte den Finder steuern“ ausloest, mitten aus der Huelle heraus); dafuer
/// fehlt dort mancherorts „Zuruecklegen“ im Finder, die Datei liegt aber im Papierkorb.
fn papierkorb(p: &std::path::Path) -> Result<(), trash::Error> {
    #[allow(unused_mut)]
    let mut ctx = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        ctx.set_delete_method(DeleteMethod::NsFileManager);
    }
    ctx.delete(p)
}

/// Entfernt ein Bild mit allen Teilen: JPEG (und eine uebrige Eingangskopie) in
/// den Papierkorb, Vorschau und Sidecar endgueltig. Nie nur ein Teil. Der
/// Hüllen-Dialog zeigt `bezeichnung`, falls das Paket sie mitgibt.
#[tauri::command(async)]
pub(crate) fn bild_loeschen(app: AppHandle, gruppe: String, id: String) -> Result<Vec<String>, UiError> {
    let weg = |p: &std::path::Path| papierkorb(p).map_err(|e| std::io::Error::other(e.to_string()));
    speicher::loeschen(&wurzel(&app)?, &gruppe, &id, &weg).map_err(fehler)
}

/// Speichert die bestaetigte Liste der Bildquellen (und, wenn angegeben, das
/// Tastenkuerzel) und setzt Beobachtung und Tastenkuerzel neu auf. Gibt den
/// Stand wie `bilder_liste` zurueck (`quellen`, `tastenkuerzel`).
///
/// `quellen` ersetzt die Liste; fehlt es, bleibt sie. Mit `ordner_waehlen: true`
/// oeffnet der Kern nach der Bestaetigung der Huelle den Ordnerdialog des
/// Systems und haengt den gewaehlten Ordner als Quelle der Art `hand` an
/// (nur kopieren, je Quelle spaeter einschaltbar). Abbrechen aendert nichts.
/// Der Altbestand einer neuen Quelle wird nie gemeldet; ein fehlender oder
/// haengender Ordner (Netzlaufwerk) blockiert nichts.
#[tauri::command]
pub(crate) async fn bild_quellen_setzen(
    app: AppHandle,
    quellen: Option<Vec<Quelle>>,
    tastenkuerzel: Option<String>,
    ordner_waehlen: Option<bool>,
) -> Result<Value, UiError> {
    let ordner = if ordner_waehlen == Some(true) {
        match ordner_dialog(&app).await? {
            Some(p) => Some(p),
            None => return Ok(aktueller_stand(&einstellungen_lesen_app(&app)?)), // abgebrochen: nichts aendert sich
        }
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || quellen_setzen_intern(&app, quellen, tastenkuerzel, ordner))
        .await
        .map_err(|e| UiError::new("bild_io", e.to_string()))?
}

fn einstellungen_lesen_app(app: &AppHandle) -> Result<Einstellungen, UiError> {
    Ok(quellen::einstellungen_lesen(&einstellungen_datei(app)?))
}

fn aktueller_stand(e: &Einstellungen) -> Value {
    json!({"quellen": entsperren(anzeige()).status, "tastenkuerzel": tastenkuerzel_stand(e), "abgebrochen": true})
}

fn quellen_setzen_intern(app: &AppHandle, quellen: Option<Vec<Quelle>>, tastenkuerzel: Option<String>, ordner: Option<PathBuf>) -> Result<Value, UiError> {
    let datei = einstellungen_datei(app)?;
    let bisher = quellen::einstellungen_lesen(&datei);
    let neu = quellen::einstellungen_neu(&bisher, quellen, tastenkuerzel.as_deref(), ordner.as_deref(), &wurzel(app)?, app.path().home_dir().ok().as_deref())
        .map_err(fehler)?;
    quellen::einstellungen_schreiben(&datei, &neu).map_err(fehler)?;
    beobachtung_aufsetzen(&neu.quellen);
    hotkey_anwenden(&neu.tastenkuerzel);
    Ok(json!({"quellen": entsperren(anzeige()).status, "tastenkuerzel": tastenkuerzel_stand(&neu)}))
}

/// Ordnerdialog des Systems; `None`, wenn der Pilot abbricht.
async fn ordner_dialog(app: &AppHandle) -> Result<Option<PathBuf>, UiError> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |p| {
        let _ = tx.send(p);
    });
    match rx.await.ok().flatten() {
        Some(p) => Ok(Some(p.into_path().map_err(|e| UiError::new("bild_pfad", format!("Ordner ungueltig: {e}")))?)),
        None => Ok(None),
    }
}

/// Fuegt vom Piloten gewaehlte oder hineingezogene Dateien (JPG/PNG) als
/// Eingangskopien in `gruppe` ein: immer **kopiert**, nie verschoben, auch
/// ausserhalb der Quellen und ausserhalb eines Flugs. Ohne `pfade` (Ziehen)
/// oeffnet der Kern die Dateiauswahl des Systems. Das Paket liest danach die
/// Eingangskopien mit `bild_lesen` und schreibt das JPEG mit `bild_schreiben`.
#[tauri::command]
pub(crate) async fn bild_hinzufuegen(app: AppHandle, gruppe: String, pfade: Option<Vec<String>>) -> Result<Value, UiError> {
    let pfade = match pfade.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => match dateien_dialog(&app).await? {
            Some(p) if !p.is_empty() => p,
            _ => return Ok(json!({"abgebrochen": true, "ergebnisse": []})),
        },
    };
    let wurzel = wurzel(&app)?;
    tauri::async_runtime::spawn_blocking(move || hinzufuegen_ergebnis(&wurzel, &pfade, &gruppe))
        .await
        .map_err(|e| UiError::new("bild_io", e.to_string()))?
}

fn hinzufuegen_ergebnis(wurzel: &std::path::Path, pfade: &[String], gruppe: &str) -> Result<Value, UiError> {
    let r = quellen::hinzufuegen_mehrere(wurzel, pfade, gruppe, Duration::from_millis(400)).map_err(fehler)?;
    let ergebnisse: Vec<Value> = r
        .into_iter()
        .map(|(pfad, e)| match e {
            Ok(u) => json!({"pfad": pfad, "ok": true, "uebernahme": u}),
            Err(f) => json!({"pfad": pfad, "ok": false, "code": f.code(), "meldung": f.meldung()}),
        })
        .collect();
    Ok(json!({"abgebrochen": false, "ergebnisse": ergebnisse}))
}

/// Dateiauswahl des Systems (JPG, PNG); `None`, wenn der Pilot abbricht.
async fn dateien_dialog(app: &AppHandle) -> Result<Option<Vec<String>>, UiError> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().add_filter("JPG, PNG", &["jpg", "jpeg", "png"]).pick_files(move |p| {
        let _ = tx.send(p);
    });
    let Some(liste) = rx.await.ok().flatten() else { return Ok(None) };
    let mut v = Vec::new();
    for f in liste {
        let p = f.into_path().map_err(|e| UiError::new("bild_pfad", format!("Datei ungueltig: {e}")))?;
        v.push(p.to_string_lossy().into_owned());
    }
    Ok(Some(v))
}

/// Speichert fertige ZIP-Bytes an einem Ort, den der Pilot im Speichern-Dialog
/// des Systems waehlt. `None`, wenn er abbricht; sonst der gewaehlte Pfad.
#[tauri::command]
pub(crate) async fn bild_export(app: AppHandle, zip_base64: String, dateiname: String) -> Result<Option<String>, UiError> {
    use tauri_plugin_dialog::DialogExt;
    let bytes = b64(&zip_base64, "zip_base64")?;
    let vorschlag: String = dateiname.chars().filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ')).collect();
    let vorschlag = if vorschlag.trim().is_empty() { "AeroACARS-Bilder.zip".to_string() } else { vorschlag };
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().add_filter("ZIP", &["zip"]).set_file_name(vorschlag).save_file(move |p| {
        let _ = tx.send(p);
    });
    let Some(ziel) = rx.await.ok().flatten() else { return Ok(None) };
    let pfad = ziel.into_path().map_err(|e| UiError::new("bild_pfad", format!("Speicherort ungueltig: {e}")))?;
    let pfad = tauri::async_runtime::spawn_blocking(move || speicher::export_schreiben(&pfad, &bytes))
        .await
        .map_err(|e| UiError::new("bild_io", e.to_string()))?
        .map_err(fehler)?;
    Ok(Some(pfad.to_string_lossy().into_owned()))
}

// ------------------------------------------------- Beobachtung, Hotkey

fn hotkey_anwenden(text: &str) {
    if !plattform::AUFNAHME_VERFUEGBAR {
        return;
    }
    let ergebnis = match (tastenkuerzel::lesen(text), entsperren(hotkey()).as_ref()) {
        (Ok(t), Some(h)) => h.setzen(Some(&t)),
        (Err(e), _) => Err(e),
        (_, None) => Err(Fehler::Io("Tastenkuerzel-Dienst laeuft nicht".into())),
    };
    *entsperren(hotkey_fehler()) = ergebnis.err().map(|e| {
        tracing::warn!(fehler = %e, "Tastenkuerzel der Bildaufnahme nicht angemeldet");
        e.meldung().to_string()
    });
}

/// Beim Start: Einstellungen laden, Beobachtung aufsetzen, Tastenkuerzel
/// anmelden und den Takt-Faden starten. Blockiert den Start nicht.
pub(crate) fn starten(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("bilder-start".into()).spawn(move || {
        // Liegengebliebenes Loeschen (Absturz mittendrin) abschliessen oder zurueckrollen.
        if let Ok(w) = wurzel(&app) {
            for id in speicher::loeschreste_abschliessen(&w) {
                tracing::info!(id = %id, "Bilder: liegengebliebenes Loeschen abgeschlossen");
            }
        }
        let einst = einstellungen_datei(&app).map(|d| quellen::einstellungen_lesen(&d)).unwrap_or_default();
        beobachtung_aufsetzen(&einst.quellen);
        if plattform::AUFNAHME_VERFUEGBAR {
            let app_taste = app.clone();
            match plattform::hotkey_starten(move || taste_gedrueckt(&app_taste)) {
                Ok(h) => {
                    *entsperren(hotkey()) = Some(h);
                    hotkey_anwenden(&einst.tastenkuerzel);
                }
                Err(e) => *entsperren(hotkey_fehler()) = Some(e.meldung().to_string()),
            }
        }
        let start = std::time::Instant::now();
        loop {
            std::thread::sleep(TAKT);
            let vordergrund = plattform::sim_vordergrund();
            // Sperre nur zum Holen der Quellen: die Ordnerabfragen laufen ohne sie,
            // ein haengender Ordner haelt hoechstens diesen Faden auf.
            let (slots, vg) = entsperren(beobachter()).takt_vorbereiten(vordergrund);
            let funde = bilder_kern::beobachter::takt_ausfuehren(&slots, start.elapsed().as_millis() as u64, vg);
            let status = entsperren(beobachter()).status();
            entsperren(anzeige()).status = status;
            for f in funde {
                if let Err(e) = app.emit(EREIGNIS_QUELLE_NEU, &f) {
                    tracing::warn!(fehler = %e, "bild_quelle_neu nicht gesendet");
                }
            }
        }
    });
}

/// Tastenkuerzel gedrueckt: ohne aktiven Flug nichts aufnehmen (Ereignis mit
/// `kein_flug`), sonst sofort aufnehmen (der Moment zaehlt). Ergebnis als
/// Ereignis `bild_aufgenommen`; das Paket ordnet zu und schreibt das JPEG.
fn taste_gedrueckt(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("bild-aufnahme".into()).spawn(move || {
        let nutzlast = aufgenommen_nutzlast(flug_aktiv(&app), || aufnehmen_intern(&app, EINGANG));
        let _ = app.emit(EREIGNIS_AUFGENOMMEN, nutzlast);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const S: &str = "11111111-2222-4333-8444-555555555555";
    const B1: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

    /// Ein Einweg-Server: liest eine ganze Anfrage, antwortet mit `antwort`.
    async fn server(antwort: &'static str) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let basis = format!("http://{}", l.local_addr().unwrap());
        let h = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut roh = Vec::new();
            let mut puffer = [0u8; 8192];
            loop {
                let n = s.read(&mut puffer).await.unwrap();
                roh.extend_from_slice(&puffer[..n]);
                let Some(k) = roh.windows(4).position(|w| w == b"\r\n\r\n") else { continue };
                let kopf = String::from_utf8_lossy(&roh[..k]).to_ascii_lowercase();
                let laenge = kopf.lines().find_map(|z| z.strip_prefix("content-length: ")).and_then(|v| v.trim().parse::<usize>().ok()).unwrap_or(0);
                if roh.len() >= k + 4 + laenge {
                    break;
                }
            }
            s.write_all(antwort.as_bytes()).await.unwrap();
            roh
        });
        (basis, h)
    }

    fn teile() -> Vec<live::Teil> {
        let b = LiveBild { bild_id: B1.into(), jpeg: vec![0xFF, 0xD8, 0xFF, 0xD9], meta: json!({"fassung": 1}) };
        live::senden_teile(S, Some("Hallo"), "verdeckt", &[b]).unwrap()
    }

    #[tokio::test]
    async fn senden_haengt_das_token_an_und_schickt_den_vertrag_unveraendert() {
        let antwort = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 54\r\nconnection: close\r\n\r\n{\"sendung_id\":\"x\",\"bilder\":[],\"kontingent\":{\"rest\":9}}";
        let (basis, h) = server(antwort).await;
        let erwartet = teile();
        let r = live_anfrage(&basis, "GEHEIM-TOKEN", LiveAnfrage::Senden(erwartet.clone())).await.unwrap();
        assert_eq!(r["kontingent"]["rest"], 9, "Antwort unveraendert durchgereicht");
        let roh = h.await.unwrap();
        let text = String::from_utf8_lossy(&roh).to_string();
        let kopf_ende = roh.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let kopf = text[..kopf_ende].to_ascii_lowercase();
        assert!(kopf.starts_with("post /api/bilder http/1.1"), "{kopf}");
        assert!(kopf.contains("authorization: bearer geheim-token"), "{kopf}");
        let grenze = kopf
            .lines()
            .find_map(|z| z.strip_prefix("content-type: multipart/form-data; boundary="))
            .expect("multipart mit Grenze")
            .to_string();
        // Grenze steht in Originalschreibweise im Koerper
        let orig_kopf = &text[..kopf_ende];
        let g = &orig_kopf[orig_kopf.to_ascii_lowercase().find(&grenze).unwrap()..][..grenze.len()];
        assert_eq!(&roh[kopf_ende + 4..], live::multipart_bauen(g, &erwartet).as_slice(), "Koerper = Vertrag, byte-genau");
    }

    #[tokio::test]
    async fn status_geht_als_get_mit_sendung_id() {
        let antwort = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}";
        let (basis, h) = server(antwort).await;
        live_anfrage(&basis, "T", LiveAnfrage::Status(Some(S.into()))).await.unwrap();
        let roh = String::from_utf8_lossy(&h.await.unwrap()).to_string();
        assert!(roh.starts_with(&format!("GET /api/bilder/status?sendung_id={S} HTTP/1.1")), "{roh}");
    }

    #[tokio::test]
    async fn fehlerarten_haben_eigene_codes() {
        let f = |antwort: &'static str| async move {
            let (basis, _h) = server(antwort).await;
            live_anfrage(&basis, "T", LiveAnfrage::Status(None)).await.unwrap_err()
        };
        assert_eq!(f("HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await.code, "live_nicht_angemeldet");
        let e = f("HTTP/1.1 429 Too Many\r\nretry-after: 17\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").await;
        assert_eq!(e.code, "live_kontingent");
        let d = serde_json::to_value(&e).unwrap();
        assert_eq!(d["details"]["retry_after"], "17");
        assert_eq!(f("HTTP/1.1 400 Bad\r\ncontent-length: 4\r\nconnection: close\r\n\r\nnein").await.code, "live_fehler");
        // niemand hoert zu: Verbindung kommt nicht zustande
        let tot = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let a = l.local_addr().unwrap();
            drop(l);
            format!("http://{a}")
        };
        assert_eq!(live_anfrage(&tot, "T", LiveAnfrage::Status(None)).await.unwrap_err().code, "live_nicht_erreichbar");
        // Verbindung steht, Antwort bricht ab: Zustellung unklar
        let (basis, _h) = server("").await;
        assert_eq!(live_anfrage(&basis, "T", LiveAnfrage::Status(None)).await.unwrap_err().code, "live_antwort_fehlt");
    }

    #[test]
    fn base64_eingabe_wird_geprueft() {
        assert_eq!(b64("AQID", "x").unwrap(), [1, 2, 3]);
        assert_eq!(b64("!!", "x").unwrap_err().code, "bild_eingabe");
    }

    #[test]
    fn kern_fehler_werden_zu_stabilen_codes() {
        assert_eq!(fehler(Fehler::Pfad("x".into())).code, "bild_pfad");
        assert_eq!(fehler(Fehler::Quelle("x".into())).code, "bild_quelle");
        assert_eq!(fehler(Fehler::NichtVerfuegbar("x".into())).code, "bild_nicht_verfuegbar");
    }

    #[test]
    fn ohne_aktiven_flug_wird_nicht_aufgenommen_und_kein_flug_gemeldet() {
        let aufgerufen = std::cell::Cell::new(false);
        let n = aufgenommen_nutzlast(false, || {
            aufgerufen.set(true);
            Ok(json!({"id": "x"}))
        });
        assert!(!aufgerufen.get(), "die Aufnahme darf ohne Flug nicht einmal beginnen");
        assert_eq!(n["ok"], false);
        assert_eq!(n["code"], "kein_flug");
        assert!(n["meldung"].as_str().is_some_and(|m| !m.is_empty()));
        // Gegenprobe: mit Flug laeuft sie und meldet ihr Ergebnis; ein Fehler behaelt seinen Code
        let n = aufgenommen_nutzlast(true, || Ok(json!({"id": "x"})));
        assert_eq!((n["ok"].as_bool(), n["aufnahme"]["id"].as_str()), (Some(true), Some("x")));
        let n = aufgenommen_nutzlast(true, || Err(UiError::new("verdeckt", "Fenster davor")));
        assert_eq!((n["ok"].as_bool(), n["code"].as_str()), (Some(false), Some("verdeckt")));
    }

    #[test]
    fn senden_anfrage_baut_den_vertrag_und_prueft_die_bytes() {
        let bild = |b64s: &str| LiveBildEin { bild_id: B1.into(), jpeg_base64: b64s.into(), meta: json!({"fassung": 1}) };
        // /9j/2Q== = FF D8 FF D9
        let a = senden_anfrage(S, Some("Hallo"), None, vec![bild("/9j/2Q==")]).unwrap();
        let LiveAnfrage::Senden(teile) = a else { panic!("Senden erwartet") };
        assert_eq!(teile, teile_fuer_test());
        assert_eq!(senden_anfrage(S, None, None, vec![bild("!!")]).err().map(|e| e.code), Some("bild_eingabe".to_string()));
    }

    fn teile_fuer_test() -> Vec<live::Teil> {
        teile()
    }

    #[test]
    fn hinzufuegen_ergebnis_meldet_jede_datei_mit_code() {
        let t = tempfile::tempdir().unwrap();
        let png = t.path().join("a.png");
        std::fs::write(&png, bilder_kern::png::aus_bgra(4, 4, &[7u8; 64]).unwrap()).unwrap();
        let txt = t.path().join("b.txt");
        std::fs::write(&txt, b"x").unwrap();
        let v = hinzufuegen_ergebnis(&t.path().join("bilder"), &[png.to_string_lossy().into_owned(), txt.to_string_lossy().into_owned()], "ohne-flug").unwrap();
        assert_eq!(v["abgebrochen"], false);
        assert_eq!(v["ergebnisse"][0]["ok"], true);
        assert!(v["ergebnisse"][0]["uebernahme"]["quelle_pfad"].as_str().unwrap().starts_with("ohne-flug/"));
        assert_eq!((v["ergebnisse"][1]["ok"].as_bool(), v["ergebnisse"][1]["code"].as_str()), (Some(false), Some("bild_format")));
        assert!(txt.exists() && png.exists(), "Originale bleiben");
    }
}
