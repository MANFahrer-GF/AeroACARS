//! Bilder-Tab (N-13, Faehigkeit `bilder-1`, P-11.30 bis P-11.34): die fuenfzehn
//! Befehle, der dauerhafte Eingang `.eingang/`, das Ereignis `bild_quelle_neu`
//! und das Tastenkuerzel. **Diese Kopf-Doku ist der Vertrag fuer das Paket.**
//!
//! Grundsatz (P-11.33): Dieser Teil liefert nur Plattform-Bausteine. Zuordnung,
//! Metadaten, Verkleinern, EXIF und Filter sind Sache des Laufzeit-Pakets. Die
//! Logik steht in `crates/bilder-kern` (ohne Tauri testbar); die Befehle hier
//! sind duenne Adapter. Was der Kern nicht dem Paket ueberlassen darf, erzwingt
//! er selbst: Pfade nur in bestaetigten Quellen und ohne Linkfolgen, Funde nur
//! aus einem aktiven AeroACARS-Flug, Loeschen, Senden und Hinzufuegen nur nach
//! Bestaetigung der Huelle.
//!
//! # Eingang (dauerhaft)
//! Jeder Fund im aktiven Flug (neue Datei in einer Quelle, Aufnahme per
//! Tastenkuerzel oder `bild_aufnehmen`) wird **zuerst** als
//! `<bilder>/.eingang/<fund_id>.json` abgelegt, mit dem Stand zur Entdeckung.
//! Ausserhalb eines Flugs entsteht kein Fund (die Datei bleibt unangetastet).
//! `bilder_liste` liefert die offenen Funde als `funde` (aelteste zuerst). Die
//! Ereignisse `bild_quelle_neu` (der Eintrag) und `bild_aufgenommen` sind nur
//! Wecksignale: ohne sie, auch bei geschlossenem Tab oder nach einem Neustart,
//! ist der Eingang vollstaendig. Das Paket arbeitet ihn strikt nacheinander ab. Gelingt das Schreiben des Eintrags
//! nicht (z. B. Datentraeger voll), gilt die Datei nicht als gesehen: sie wird nach 10 s erneut gelesen und gemeldet.
//!
//! **Fund** (genau diese Felder, nichts doppelt flach): `fund_id`, `herkunft`
//! (`datei` | `aufnahme`), `entdeckt_ms`, `pfad` (Quelldatei, nur `datei`),
//! `dateizeit_ms`, `erstellt_ms`, `groesse`, `format`, `px_breite`, `px_hoehe`,
//! `uebernehmbar`, `quelle` (Quellordner), `art` (`standard` | `aufgespuert` |
//! `hand`, die Art der Quelle), `quelle_pfad` (Eingangskopie
//! `<gruppe>/<id>.quelle.png`, nur `aufnahme`), `sha256` (der Originaldatei),
//! `sim_vordergrund`, `sim_fenster {breite, hoehe}` (Simulatorfenster, nur wo
//! ermittelbar; sonst greift die Pixelmass-Regel nicht) und
//! `meta_basis {zeit_utc, breite, laenge, hoehe_ft, gs_kt, kurs, flugzeug_titel,
//! flugzeug_icao, phase, pirep_id, rufzeichen, abflug, ziel, simulator,
//! naechster_flugplatz}` (verschachtelt). `breite`/`laenge` sind die Position,
//! die Bildmasse heissen `px_breite`/`px_hoehe`. Fehlende Werte sind `null`,
//! nichts wird geraten. `naechster_flugplatz` ist `{icao, abstand_m}`.
//!
//! # Befehle
//! Byte-Vertrag: `bild_lesen` gibt Bytes **roh** zurueck (`ArrayBuffer`);
//! `bild_schreiben` und `bild_export` nehmen Base64 (Standard-Alphabet);
//! `bild_senden` nimmt **keine Bytes** (der Kern liest die Dateien selbst).
//! Argumente: oberste Ebene camelCase, Felder verschachtelter Objekte snake_case.
//!
//! lesen
//! * `bilder_liste({mitAltbestand?})` -> `{eintraege, belegt_bytes, funde, quellen, tastenkuerzel, aufnahme_verfuegbar, simulator_taste, altbestand?}`;
//!   `simulator_taste` ist `{simulator: "xplane", text} | null`: die wirkliche Screenshot-Taste von X-Plane
//!   (`text` z. B. `Shift+Space`, mehrere Belegungen mit ` / `), gelesen aus
//!   `Output/preferences/control profiles/User Profile - Keys.prf` (Vorrang) bzw. `X-Plane Keys.prf` der ersten
//!   erkannten Installation mit lesbarer Belegung; `null` bei fehlender Datei, fremdem Format oder ohne X-Plane,
//!   nie ein Fehler, nie geraten. Der Tab zeigt sie statt einer festen Taste.
//!   `quellen[]` tragen `art` (`standard`, `aufgespuert`, `hand`, sonst frei), `wird_geprueft` und `fehler`
//!   (`nicht_vorhanden`, `zugriff_verweigert`, `quelle_geaendert` oder `null`)
//! * `bild_lesen({pfad})` -> ArrayBuffer (`pfad` relativ aus der Liste)
//! * `bild_quellen_finden()` -> `[{pfad, art, simulator, existiert, vorschlag_modus}]`
//! * `bild_status({sendungId?})` -> Stand einer Sendung und Kontingent (`GET /api/bilder/status`)
//! * `bild_kontext({zeiten?: [iso], pirepId?, mitSpur?, fluege?})` -> `{staende: [{zeit, quelle, abstand_s?, ...}], spur: [[lat, lon], ...], fluege?: [{pirep_id, rufzeichen, abflug, ziel, flugzeug_icao, start_ms, ende_ms}]}`.
//!   Je Zeit der zeitlich naechste Stand aus `flight_logs/<pirep>.jsonl` (`quelle: "protokoll"`, hoechstens 120 s
//!   Abstand) mit den Feldern von `meta_basis` (flach) samt `naechster_flugplatz` aus dem OurAirports-Stand der
//!   aktiven Generation; sonst die Block-Zeiten des PIREP aus phpVMS (`"logbuch"`: nur `pirep_id`, `flugnummer`,
//!   `abflug`, `ziel`, `flugzeug_icao`, seitenweise zurueckgeblaettert, nur mit Block-off und Block-on); sonst
//!   `"keine"`. `spur` (nur mit `mitSpur`, sonst leer): ausgeduennt auf hoechstens 600 Punkte, von `pirepId` oder
//!   dem ersten Treffer; im aktiven Flug dessen Aufzeichnung
//!
//! fachlich
//! * `bild_uebernehmen({fundId, modus, gruppe?, id?})`: `modus` `kopieren` | `verschieben` | `verwerfen`.
//!   Der Fund steht im Eingang, weil er im aktiven Flug entstand; die Uebernahme selbst darf nach Flugende laufen.
//!   Ergebnis: Eingangskopie `<gruppe>/<id>.quelle.png|jpg` (`gruppe` Pflicht), der Eintrag wird entfernt.
//!   `verschieben` nur aus Quellen, die der Pilot dafuer freigegeben hat. `verwerfen` -> `{fund_id, verworfen: true}`:
//!   entfernt nur den Eintrag und fasst die Quelldatei nie an (eine eigene Aufnahme-Kopie wird mit entfernt).
//!   Es gibt kein `bild_verwerfen`.
//! * `bild_schreiben({gruppe, id, jpegBase64?, vorschauBase64?, sidecar?, quelleEntfernen?, sha256?})` -> Eintrag;
//!   `sha256` (kanonischer Hash der EXIF-freien Bytes, 64 Hex) wird im Sidecar als `sha256` festgehalten; ein im
//!   Bilderordner schon vorhandener Hash einer anderen Bild-id wird atomar mit Code `duplikat` abgelehnt (Meldung
//!   `gruppe/id` des vorhandenen Bildes) und die Eingangskopie dieser id selbst weggeraeumt (keine Waise)
//! * `bild_aufnehmen({gruppe?})` -> `{fund, methode}`; nur im aktiven Flug (`kein_flug`). Die Aufnahme laeuft in einem
//!   eigenen Faden mit Zeitgrenze (8 s): antwortet der Simulator nicht, kommt `bild_io` und eine neue Aufnahme ist
//!   sofort wieder moeglich; der haengende Faden bleibt liegen (hoechstens einer, solange er haengt kommt `bild_io`
//!   "haengt noch"), ein zu spaetes Ergebnis wird verworfen
//! * `bild_ordner_zeigen({gruppe, id})`: markiert das JPEG im Dateimanager (Finder, Explorer); nur ein vorhandenes
//!   JPEG im verwalteten Ordner, kein Pfad-Ausbruch
//!
//! system (Huellen-Dialog)
//! * `bild_senden({sendungId, text?, geoStufe?, bilder: [{gruppe, bild_id, meta}]})` -> Antwort des Servers (`POST /api/bilder`).
//!   Der Kern liest `<gruppe>/<bild_id>.jpg`, entfernt **alle APP1-Segmente** (EXIF, XMP) mit einem begrenzten
//!   JPEG-Segment-Parser und baut die Anfrage. Gleiche Datei = gleiche Upload-Bytes (kein `konflikt` bei
//!   Wiederholung). Die Huelle zeigt nur die Bildanzahl.
//! * `bild_hinzufuegen({gruppe, pfade?})` -> `{abgebrochen, ergebnisse: [{pfad, ok, uebernahme | code, meldung}]}`;
//!   `gruppe` ist Pflicht (der Tab gibt `ohne-flug`); ohne `pfade` (Ziehen) oeffnet der Kern die Dateiauswahl;
//!   JPG/PNG, immer kopiert, auch ohne Flug. Ergebnis wie `bild_uebernehmen`: Eingangskopie in `gruppe`
//! * `bild_loeschen({bilder: [{gruppe, id}], bezeichnung?})`, `bild_export({zipBase64, dateiname})`
//! * `bild_quellen_setzen({quellen?, tastenkuerzel?, yokeTaste?, ordnerWaehlen?})`: Mit der Bestaetigung wird der
//!   aufgeloeste Pfad jeder Quelle gespeichert (`kanonisch`, kommt nie vom Aufrufer; alte Einstellungen werden beim
//!   Start gebunden). Ist der Pfad spaeter ein anderer Ordner (durch Link/Junction ersetzt), wird die Quelle nicht
//!   beobachtet (Status `fehler: "quelle_geaendert"`, `beobachtet: false`) und nichts daraus uebernommen oder
//!   verschoben (`bild_quelle`); Abhilfe: Quelle entfernen und neu hinzufuegen.
//!   `ordnerWaehlen: true` oeffnet nach der
//!   Bestaetigung den Ordnerdialog des Systems und haengt den Ordner als Quelle `hand` an (nur kopieren);
//!   Abbrechen aendert nichts. `yokeTaste` ist `{geraet_name, hersteller_id, produkt_id, knopf}` (Knopf 1 bis 32)
//!   oder `null`: fehlt der Schluessel, bleibt der Knopf; `null` loescht ihn. Der Befehl liest dazu den Rohauftrag,
//!   weil Tauri fehlend und `null` bei `Option` nicht trennt.
//! * `bild_taste_lernen()` -> `{geraet_name, hersteller_id, produkt_id, knopf} | null`: wartet hoechstens 10 s auf
//!   einen neu gedrueckten Knopf an einem Yoke oder Joystick (nur Windows, `winmm`) und speichert nichts; das Ergebnis
//!   geht danach als `yokeTaste` an `bild_quellen_setzen`. Waehrenddessen loest der belegte Knopf keine Aufnahme aus. Auf anderen Systemen Fehlercode `bild_nicht_verfuegbar`.
//!   Die Schnittstelle `winmm` sieht je Geraet hoechstens 32 Knoepfe (keine Hat-Schalter, keine Achsen). Der Windows-Teil
//!   ist unter macOS nur uebersetzt (`cargo check --target x86_64-pc-windows-msvc`), nicht ausgefuehrt.
//!
//! Yoke-Knopf als Ausloeser: `bilder_liste.tastenkuerzel` traegt `yoke` (der gemerkte Knopf oder `null`) und
//! `yoke_verfuegbar` (nur Windows). Ein eigener Faden fragt mit 20 Hz ab, solange ein Knopf belegt ist; das Geraet
//! wird ueber `hersteller_id`, `produkt_id` und `geraet_name` gefunden (nicht ueber die Joystick-Nummer), ein
//! abgestecktes Geraet blockiert nichts und wird wiedergefunden. Die steigende Flanke loest denselben Weg aus wie
//! das Tastenkuerzel (Ereignis `bild_aufgenommen`, ohne Flug `kein_flug`); nach einer Ausloesung mindestens 1 s Sperre.
//!
//! Ereignisse (nur Wecksignale): `bild_quelle_neu` (der Eingangseintrag), `bild_aufgenommen`
//! (`{ok, aufnahme}` oder `{ok:false, code, meldung}`, `code` z. B. `kein_flug`, `verdeckt`).
//!
//! Fensteraufnahme (Windows): zuerst Windows.Graphics.Capture (`aufnahme.methode: "wgc"`). Das nimmt das
//! Simulatorfenster selbst auf, auch im Vollbild und wenn AeroACARS im Hintergrund liegt, und kopiert nichts
//! von anderen Fenstern; das Bildmass kommt aus dem Rahmen. Nur wenn WGC nicht verfuegbar ist, scheitert oder
//! ganz schwarz liefert, folgt der Rueckfall `printwindow`, dann `bitblt`. Der Code `verdeckt` gilt allein fuer
//! diese Bildschirmkopie (liegt ein fremdes Fenster darueber), nie fuer `wgc`. Die Aufnahme bekommt hoechstens 8 s.
//! Der Windows-Teil ist unter macOS nur uebersetzt (`cargo check --target x86_64-pc-windows-msvc`), nicht ausgefuehrt.
//!
//! Ton: Jede Aufnahme (Tastenkuerzel, Yoke-Knopf, `bild_aufnehmen`) meldet sich hoerbar, auch wenn AeroACARS
//! im Hintergrund liegt: kurzes doppeltes Klicken bei Erfolg, tiefer Ton bei Fehlschlag (auch `kein_flug`). Die
//! Nutzlast des Ereignisses bleibt unveraendert; ein Fehlschlag steht zusaetzlich als Warnung im Protokoll.
//!
//! Fehlercodes des Kerns: `bild_name`, `bild_pfad`, `bild_nicht_gefunden`, `bild_format`, `bild_unfertig`,
//! `bild_quelle`, `bild_verboten`, `bild_zu_gross`, `bild_eingabe`, `bild_nicht_verfuegbar`, `bild_io`,
//! `bild_veraltet` (nur `bild_uebernehmen` eines Funds `datei`: Groesse oder SHA-256 der Datei passen nicht mehr zum
//! Fund, weil sie unter demselben Namen ueberschrieben wurde; nichts wurde kopiert oder entfernt, der Eintrag bleibt,
//! `verwerfen` geht, die Datei wird danach als neuer Fund gemeldet), `bild_beschaeftigt`,
//! `kein_flug`, `verdeckt`, `duplikat`; beim Senden `live_nicht_angemeldet`, `live_nicht_erreichbar`,
//! `live_antwort_fehlt` (Zustellung unklar: erst `bild_status` mit derselben `sendungId`), `live_kontingent`
//! (`details.retry_after`), `live_fehler` (`details.status`).
//!
//! Stufen (befehle.json): lesen `bilder_liste`, `bild_lesen`, `bild_quellen_finden`, `bild_status`,
//! `bild_kontext`; fachlich `bild_uebernehmen`, `bild_schreiben`, `bild_aufnehmen`, `bild_ordner_zeigen`,
//! `bild_taste_lernen`;
//! system `bild_senden` (aus dem Hauptfenster ohne Huelle-Fenster: der Senden-Dialog ist die Bestaetigung,
//! Thomas 07.11.), `bild_hinzufuegen`, `bild_loeschen`, `bild_quellen_setzen`, `bild_export`.
//! LAN-Bruecke (Thomas 07.11.): `bilder_liste`, `bild_status`, `bild_kontext`, `bild_aufnehmen` und `bild_schreiben`
//! (dort nur der Sidecar) haben Arme, `bild_lesen` die Route `POST /api/bild` (Token-Kopf, Antwort rohe Bytes),
//! `bild_senden` laeuft ueber `shell_request` ohne Huelle-Fenster, die anderen Systembefehle mit. Den Eingang arbeitet
//! nur der Sim-PC ab; Quellen, Tasten, Hinzufuegen, Export und Dateimanager gibt es am Tablet nicht.
//!
//! Auslegung: Weil es genau fuenfzehn Befehle sind, tragen zwei Antworten mehr als der Name sagt:
//! `bilder_liste` liefert auch Funde, Quellen mit Beobachtungszustand, den Altbestand (auf Wunsch) und das
//! Tastenkuerzel; `bild_quellen_setzen` setzt neben den Ordnern auch das Tastenkuerzel.
//!
//! Beobachtung: jede Quelle hat ihren eigenen Stand und im Takt ihren eigenen Abfrage-Faden (siehe
//! `bilder_kern::beobachter`): eine haengende Netzquelle haelt die lokalen nicht auf. Die Grundlage einer
//! neuen Quelle laeuft im Hintergrund, `bild_quellen_setzen` wartet nie auf einen haengenden Ordner.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

use base64::Engine as _;
use bilder_kern::beobachter::Beobachter;
use bilder_kern::eingang::{self, EingangsFund, SimFenster};
use bilder_kern::kontext::{self, PirepKurz};
use bilder_kern::live::{self, BildKennung, LiveBild};
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

fn yoke() -> &'static Mutex<Option<plattform::Yoke>> {
    static Y: OnceLock<Mutex<Option<plattform::Yoke>>> = OnceLock::new();
    Y.get_or_init(|| Mutex::new(None))
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
        "yoke": e.yoke,
        "yoke_verfuegbar": plattform::YOKE_VERFUEGBAR,
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
        "funde": eingang::liste(&wurzel),
        "quellen": a.status,
        "tastenkuerzel": tastenkuerzel_stand(&einst),
        "aufnahme_verfuegbar": plattform::AUFNAHME_VERFUEGBAR,
        "simulator_taste": simulator_taste(),
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
    Ok(tauri::ipc::Response::new(bild_bytes(&app, &pfad)?))
}

/// Fertige Bilder und Vorschauen; die Eingangskopien bleiben am Sim-PC.
pub(crate) fn fuers_tablet(pfad: &str) -> bool {
    pfad.ends_with(".jpg") && !pfad.ends_with(".quelle.jpg")
}

/// Der Hash haengt am Bild und an der Duplikatpruefung: Er bleibt, was immer das Tablet schickt.
fn sha_behalten(neu: &mut Value, alt: Option<&Value>) {
    if let Some(o) = neu.as_object_mut() {
        match alt.and_then(|a| a.get("sha256")) {
            Some(sha) => o.insert("sha256".into(), sha.clone()),
            None => o.remove("sha256"),
        };
    }
}

pub(crate) fn sidecar_vom_tablet(app: AppHandle, gruppe: String, id: String, mut sidecar: Value) -> Result<speicher::Eintrag, UiError> {
    let alt = speicher::eintrag(&wurzel(&app)?, &gruppe, &id).map_err(fehler)?;
    let exif_veraltet = position_geaendert(alt.sidecar.as_ref(), &sidecar);
    sha_behalten(&mut sidecar, alt.sidecar.as_ref());
    // Das Tablet legt keine Bytes ab: Nach einem Flugwechsel faellt die alte Position aus dem JPEG, statt falsch stehen zu bleiben.
    let jpeg = match alt.pfad.as_deref().filter(|_| exif_veraltet) {
        Some(p) => Some(bilder_kern::jpeg::app1_entfernen(&std::fs::read(p).map_err(|e| UiError::new("bild_io", e.to_string()))?).map_err(fehler)?),
        None => None,
    };
    speicher::schreiben(&wurzel(&app)?, SchreibAnfrage { gruppe, id, jpeg, vorschau: None, sidecar: Some(sidecar), quelle_entfernen: false, sha256: None }).map_err(fehler)
}

fn position_geaendert(alt: Option<&Value>, neu: &Value) -> bool {
    let ort = |s: &Value| (s["meta"]["breite"].clone(), s["meta"]["laenge"].clone());
    alt.is_some_and(|a| ort(a) != ort(neu))
}

pub(crate) fn bild_bytes(app: &AppHandle, pfad: &str) -> Result<Vec<u8>, UiError> {
    speicher::lesen(&wurzel(app)?, pfad).map_err(fehler)
}

/// Windows: Game Bar, Bilder-Ordner (mit OneDrive), Steam, NVIDIA. X-Plane:
/// `Output/screenshots` jeder erkannten Installation (Mac und Windows). Ohne
/// Treffer eine leere Liste, nie ein Fehler.
fn xplane_installationen() -> Vec<PathBuf> {
    let mut installs: Vec<PathBuf> = sim_xplane::szenerie::installationen();
    if let Some(p) = crate::xplane_plugin_install::detect_install_path() {
        if !installs.contains(&p) {
            installs.push(p);
        }
    }
    installs
}

/// Die wirkliche Screenshot-Taste der ersten erkannten X-Plane-Installation, die
/// eine lesbare Belegung traegt (`xplane_taste`); sonst `null`, nie ein Fehler.
fn simulator_taste() -> Value {
    xplane_installationen()
        .iter()
        .find_map(|i| bilder_kern::xplane_taste::screenshot_taste(i))
        .map_or(Value::Null, |text| json!({ "simulator": "xplane", "text": text }))
}

#[tauri::command(async)]
pub(crate) fn bild_quellen_finden() -> Vec<Fundort> {
    let installs = xplane_installationen();
    let mut v = suche::xplane_fundorte(&installs);
    for f in plattform::windows_fundorte() {
        if !v.iter().any(|g| g.pfad == f.pfad) {
            v.push(f);
        }
    }
    v
}

// ------------------------------------------------------------- fachlich

/// `bild_taste_lernen` laeuft: der belegte Knopf loest solange nichts aus.
static YOKE_LERNT: AtomicBool = AtomicBool::new(false);

/// Wartet hoechstens 10 s auf einen neu gedrueckten Knopf an einem Yoke oder
/// Joystick (Windows) und liefert `{geraet_name, hersteller_id, produkt_id, knopf}`
/// oder `null`, wenn nichts gedrueckt wurde. Speichert nichts: der Tab uebergibt
/// das Ergebnis danach als `yokeTaste` an `bild_quellen_setzen`. Ausserhalb von
/// Windows `bild_nicht_verfuegbar`.
#[tauri::command]
pub(crate) async fn bild_taste_lernen() -> Result<Value, UiError> {
    YOKE_LERNT.store(true, Ordering::SeqCst);
    let ergebnis = tauri::async_runtime::spawn_blocking(|| plattform::yoke_lernen(Duration::from_secs(10))).await;
    YOKE_LERNT.store(false, Ordering::SeqCst);
    let t = ergebnis.map_err(|e| UiError::new("bild_io", e.to_string()))?.map_err(fehler)?;
    Ok(json!(t))
}

/// Arbeitet einen Fund des Eingangs ab: `kopieren`/`verschieben` nimmt die Datei
/// aus ihrer bestaetigten Quelle (`verschieben` nur, wenn der Pilot die Quelle
/// dafuer freigegeben hat) als Eingangskopie `<gruppe>/<id>.quelle.png|jpg` in
/// den Bilderordner, `verwerfen` entfernt nur den Eintrag und fasst die
/// Quelldatei nie an. Der Fund steht im Eingang, weil er in einem aktiven Flug
/// entstand; die Uebernahme darf danach laufen. Danach liest das Paket die
/// Eingangskopie mit `bild_lesen` und legt das JPEG mit `bild_schreiben` ab.
#[tauri::command(async)]
pub(crate) fn bild_uebernehmen(
    app: AppHandle,
    fund_id: String,
    modus: Modus,
    gruppe: Option<String>,
    id: Option<String>,
) -> Result<quellen::FundErgebnis, UiError> {
    let einst = quellen::einstellungen_lesen(&einstellungen_datei(&app)?);
    quellen::uebernehmen_fund(&wurzel(&app)?, &einst.quellen, &fund_id, modus, gruppe.as_deref(), id.as_deref(), Duration::from_millis(400)).map_err(fehler)
}

/// Schreibt JPEG, Vorschau und Sidecar eines Bildes atomar in den verwalteten
/// Ordner. Ein neues Bild braucht JPEG und Sidecar; danach geht jeder Teil
/// einzeln (z. B. nur das Sidecar fuer Text oder Favorit).
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)] // die Argumente sind der Vertrag des Befehls
pub(crate) fn bild_schreiben(
    app: AppHandle,
    gruppe: String,
    id: String,
    jpeg_base64: Option<String>,
    vorschau_base64: Option<String>,
    sidecar: Option<Value>,
    quelle_entfernen: Option<bool>,
    sha256: Option<String>,
) -> Result<speicher::Eintrag, UiError> {
    let anfrage = SchreibAnfrage {
        gruppe,
        id,
        jpeg: jpeg_base64.as_deref().map(|s| b64(s, "jpeg_base64")).transpose()?,
        vorschau: vorschau_base64.as_deref().map(|s| b64(s, "vorschau_base64")).transpose()?,
        sidecar,
        quelle_entfernen: quelle_entfernen.unwrap_or(false),
        sha256,
    };
    speicher::schreiben(&wurzel(&app)?, anfrage).map_err(fehler)
}

/// Der Stand eines aktiven Flugs jetzt (`meta_basis`): Flug, Phase, Position, Hoehe,
/// Geschwindigkeit, Kurs, Flugzeug, Simulator und der naechste Flugplatz aus dem
/// OurAirports-Stand der aktiven Generation. `None`, wenn kein Flug aktiv ist.
/// Was der Simulator gerade nicht liefert, fehlt (`null`), nichts wird geraten.
fn meta_basis_jetzt(app: &AppHandle) -> Option<Value> {
    let flug = app.state::<crate::AppState>().active_flight.lock().unwrap_or_else(|e| e.into_inner()).clone()?;
    let phase = flug.stats.lock().ok().and_then(|s| serde_json::to_value(s.phase).ok()).unwrap_or(Value::Null);
    let snap = crate::current_snapshot(app);
    let rufzeichen = flug.bid_callsign.clone().filter(|c| !c.trim().is_empty()).or_else(|| {
        let r = format!("{}{}", flug.airline_icao.trim(), flug.flight_number.trim());
        (!flug.flight_number.trim().is_empty()).then_some(r)
    });
    let text = |s: &Option<String>| s.clone().filter(|x| !x.trim().is_empty()).map_or(Value::Null, Value::String);
    let (breite, laenge, hoehe, gs, kurs, titel, icao, sim) = match &snap {
        Some(n) => (
            json!(n.lat),
            json!(n.lon),
            json!(n.altitude_msl_ft),
            json!(n.groundspeed_kt),
            json!(n.heading_deg_true),
            text(&n.aircraft_title),
            text(&n.aircraft_icao),
            serde_json::to_value(n.simulator).unwrap_or(Value::Null),
        ),
        None => (Value::Null, Value::Null, Value::Null, Value::Null, Value::Null, Value::Null, Value::Null, Value::Null),
    };
    let icao = if icao.is_null() && !flug.aircraft_icao.trim().is_empty() { json!(flug.aircraft_icao) } else { icao };
    let naechster = snap.as_ref().and_then(|n| naechster_flugplatz(n.lat, n.lon));
    Some(json!({
        "zeit_utc": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        "breite": breite, "laenge": laenge, "hoehe_ft": hoehe, "gs_kt": gs, "kurs": kurs,
        "flugzeug_titel": titel, "flugzeug_icao": icao,
        "phase": phase,
        "pirep_id": flug.pirep_id,
        "rufzeichen": rufzeichen,
        "abflug": flug.dpt_airport,
        "ziel": flug.arr_airport,
        "simulator": sim,
        "naechster_flugplatz": naechster,
    }))
}

/// Naechster Flugplatz (hoechstens 100 km) aus OurAirports: `{icao, abstand_m}`.
fn naechster_flugplatz(lat: f64, lon: f64) -> Option<Value> {
    let a = crate::runway::find_nearest_airports(lat, lon, 100_000.0, 1).into_iter().next()?;
    Some(json!({"icao": a.icao, "abstand_m": a.distance_m.round()}))
}

/// Ausgang der Ablage eines Funds aus der Ordnerbeobachtung.
enum Ablage {
    Abgelegt(Box<EingangsFund>),
    /// Kein aktiver Flug: es entsteht kein Fund, die Datei bleibt unangetastet (und gilt als gesehen).
    KeinFlug,
    /// Das Schreiben des Eintrags misslang (z. B. Datentraeger voll): die Datei wird erneut gemeldet.
    Misslungen,
}

/// Ein neuer Fund aus der Ordnerbeobachtung: im aktiven Flug dauerhaft in den
/// Eingang, sonst gar nicht (die Datei bleibt unangetastet).
fn fund_in_eingang(app: &AppHandle, f: &bilder_kern::beobachter::Fund) -> Ablage {
    let Some(meta) = meta_basis_jetzt(app) else { return Ablage::KeinFlug };
    let fenster = plattform::sim_fenstermasse().map(|(breite, hoehe)| SimFenster { breite, hoehe });
    let e = EingangsFund::aus_fund(f, uuid::Uuid::new_v4().to_string(), jetzt_ms(), fenster, meta);
    match wurzel(app).map_err(|u| Fehler::Io(u.message)).and_then(|w| eingang::ablegen(&w, &e)) {
        Ok(()) => Ablage::Abgelegt(Box::new(e)),
        Err(err) => {
            tracing::warn!(fehler = %err, pfad = %f.pfad, "Bilder: Fund nicht im Eingang abgelegt, wird erneut gemeldet");
            Ablage::Misslungen
        }
    }
}

/// Legt die Funde eines Takts ab und meldet die abgelegten. Ein Fund, dessen
/// Eintrag nicht geschrieben werden konnte, wird zur Beobachtung zurueckgestellt
/// und spaeter erneut gemeldet, statt als bekannt zu gelten.
fn funde_abarbeiten(
    slots: &[std::sync::Arc<bilder_kern::beobachter::Slot>],
    funde: Vec<bilder_kern::beobachter::Fund>,
    jetzt_ms: u64,
    mut ablegen: impl FnMut(&bilder_kern::beobachter::Fund) -> Ablage,
    mut melden: impl FnMut(&EingangsFund),
) {
    for f in funde {
        match ablegen(&f) {
            Ablage::Abgelegt(e) => melden(&e),
            Ablage::KeinFlug => {}
            Ablage::Misslungen => bilder_kern::beobachter::zurueckstellen(slots, &f, jetzt_ms),
        }
    }
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
        tracing::info!(methode = a.methode, rueckfall = ?a.rueckfall_grund, "Bildaufnahme");
        let png = bilder_kern::png::aus_bgra(a.breite, a.hoehe, &a.bgra)?;
        let id = uuid::Uuid::new_v4().to_string();
        let w = wurzel(app).map_err(|u| Fehler::Io(u.message))?;
        let e = speicher::quelle_schreiben(&w, gruppe, &id, QuellFormat::Png, &png)?;
        // Dauerhaft im Eingang ablegen (mit dem Stand zur Aufnahme), erst dann melden.
        let Some(meta) = meta_basis_jetzt(app) else {
            let _ = std::fs::remove_file(w.join(e.quelle_pfad.clone().unwrap_or_default()));
            return Err(Fehler::KeinFlug("Der Flug ist inzwischen beendet".into()));
        };
        let quelle_pfad = e.quelle_pfad.clone().unwrap_or_default();
        let fund = EingangsFund::aufnahme(id, jetzt_ms(), quelle_pfad, e.quelle_groesse, a.breite, a.hoehe, Some(SimFenster { breite: a.breite, hoehe: a.hoehe }), meta);
        if let Err(f) = eingang::ablegen(&w, &fund) {
            let _ = std::fs::remove_file(w.join(fund.quelle_pfad.clone().unwrap_or_default()));
            return Err(f);
        }
        Ok(json!({"fund": fund, "methode": a.methode}))
    })();
    AUFNAHME_LAEUFT.store(false, Ordering::SeqCst);
    r.map_err(fehler)
}

#[tauri::command]
pub(crate) async fn bild_aufnehmen(app: AppHandle, gruppe: Option<String>) -> Result<Value, UiError> {
    let gruppe = gruppe.unwrap_or_else(|| EINGANG.to_string());
    let r = tauri::async_runtime::spawn_blocking(move || aufnehmen_intern(&app, &gruppe))
        .await
        .map_err(|e| UiError::new("bild_io", e.to_string()))?;
    rueckmeldung(&r);
    r
}

/// Hoerbare Rueckmeldung zu einer Aufnahme (Klicken bei Erfolg, tiefer Ton sonst, auch
/// `kein_flug`); bei Fehlschlag zusaetzlich ins Protokoll, bisher blieb er still.
fn rueckmeldung(r: &Result<Value, UiError>) {
    if let Err(e) = r {
        tracing::warn!(code = %e.code, meldung = %e.message, "Bilder: Aufnahme fehlgeschlagen");
    }
    plattform::ton_abspielen(r.is_ok());
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

/// Wie [`rueckmeldung`], aber nach der Nutzlast des Ereignisses `bild_aufgenommen`
/// (die dabei unveraendert bleibt): `ok` entscheidet ueber den Ton, `code` und
/// `meldung` gehen bei Fehlschlag ins Protokoll.
fn nutzlast_rueckmeldung(nutzlast: &Value) {
    let ok = nutzlast["ok"].as_bool().unwrap_or(false);
    if !ok {
        tracing::warn!(
            code = nutzlast["code"].as_str().unwrap_or(""),
            meldung = nutzlast["meldung"].as_str().unwrap_or(""),
            "Bilder: Aufnahme per Taste fehlgeschlagen"
        );
    }
    plattform::ton_abspielen(ok);
}

/// Werte von `bild_senden`, wenn der Befehl ueber `shell_request` kommt (ohne zweite Rueckfrage).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendenWerte {
    sendung_id: String,
    text: Option<String>,
    geo_stufe: Option<String>,
    bilder: Vec<BildKennungEin>,
}

pub(crate) async fn bild_senden_aus_werten(app: AppHandle, werte: Value) -> Result<Value, UiError> {
    let w: SendenWerte = serde_json::from_value(werte).map_err(|e| UiError::new("bild_eingabe", e.to_string()))?;
    bild_senden(app, w.sendung_id, w.text, w.geo_stufe, w.bilder).await
}

/// Ein Bild der Sendung als Kennung: der Kern liest `<gruppe>/<bild_id>.jpg`
/// selbst und entfernt EXIF/XMP; es gehen keine Bytes durch die Oberflaeche.
#[derive(Debug, Deserialize)]
pub(crate) struct BildKennungEin {
    gruppe: String,
    bild_id: String,
    meta: Value,
}

/// Sendet Bilder an `POST /api/bilder` (P-7.25). Stufe **system**, aber ohne
/// Huelle-Fenster (Thomas 07.11.): Bestaetigung ist der Senden-Dialog, am Sim-PC
/// wie am Tablet; `shell_request` und die Bruecke machen die Ausnahme nur hierfuer. Der Kern liest die verwalteten JPEG-Dateien,
/// entfernt alle APP1-Segmente (gleiche Datei = gleiche Upload-Bytes, kein
/// `konflikt` bei Wiederholung) und haengt das Pilot-Token an; es erreicht die
/// Oberflaeche nie. Die Antwort des Servers geht unveraendert zurueck.
///
/// Fehlercodes: `live_nicht_angemeldet` (kein Token, 401/403),
/// `live_nicht_erreichbar` (Verbindung kam nicht zustande: Senden wartet),
/// `live_antwort_fehlt` (Verbindung stand, Antwort blieb aus: Zustellung
/// unklar, erst `bild_status` mit derselben `sendung_id` fragen),
/// `live_kontingent` (429, `details.retry_after`), `live_fehler` (alles andere,
/// `details.status`).
#[tauri::command]
pub(crate) async fn bild_senden(
    app: AppHandle,
    sendung_id: String,
    text: Option<String>,
    geo_stufe: Option<String>,
    bilder: Vec<BildKennungEin>,
) -> Result<Value, UiError> {
    let w = wurzel(&app)?;
    let kennungen: Vec<BildKennung> = bilder.into_iter().map(|b| BildKennung { gruppe: b.gruppe, bild_id: b.bild_id, meta: b.meta }).collect();
    let bilder = tauri::async_runtime::spawn_blocking(move || live::bilder_aus_dateien(&w, &kennungen))
        .await
        .map_err(|e| UiError::new("bild_io", e.to_string()))?
        .map_err(fehler)?;
    let anfrage = senden_anfrage(&sendung_id, text.as_deref(), geo_stufe.as_deref(), &bilder)?;
    live_mit_token(anfrage).await
}

/// Stand einer Sendung und Kontingent (`GET /api/bilder/status`); ohne
/// `sendung_id` nur das Kontingent. Lesend, braucht keine Bestaetigung.
#[tauri::command]
pub(crate) async fn bild_status(sendung_id: Option<String>) -> Result<Value, UiError> {
    live_mit_token(LiveAnfrage::Status(sendung_id)).await
}

fn senden_anfrage(sendung_id: &str, text: Option<&str>, geo_stufe: Option<&str>, bilder: &[LiveBild]) -> Result<LiveAnfrage, UiError> {
    let teile = live::senden_teile(sendung_id, text, geo_stufe.unwrap_or("verdeckt"), bilder).map_err(fehler)?;
    Ok(LiveAnfrage::Senden(teile))
}

async fn live_mit_token(anfrage: LiveAnfrage) -> Result<Value, UiError> {
    let token = crate::live_zugang::pilot_token().ok_or_else(|| UiError::new("live_nicht_angemeldet", "Noch nicht bei live angemeldet"))?;
    live_anfrage(crate::live_zugang::live_basis(), &token, anfrage).await
}

/// Stand zu Zeiten: je Zeit der zeitlich naechste Stand aus `flight_logs/<pirep>.jsonl`
/// (`quelle: "protokoll"`, hoechstens 120 s Abstand), sonst die Block-Zeiten des PIREP bei
/// phpVMS (`"logbuch"`: `pirep_id`, `flugnummer`, `abflug`, `ziel`, `flugzeug_icao`, sonst
/// nichts, seitenweise zurueckgeblaettert), sonst `"keine"`. Dazu der naechste Flugplatz
/// (OurAirports der aktiven Generation) und mit `mit_spur` die ausgeduennte Spur
/// `[[breite, laenge], ...]` des Flugs (`pirep_id`, sonst der des ersten Treffers; im
/// aktiven Flug dessen Aufzeichnung). Mit `fluege` die Flugliste fuer die Zuordnung von
/// Hand aus Protokollen und Logbuch, der Bildzeit (erste Zeit) naechster Flug vorn. Lesend.
#[tauri::command]
pub(crate) async fn bild_kontext(
    app: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    zeiten: Option<Vec<String>>,
    pirep_id: Option<String>,
    mit_spur: Option<bool>,
    fluege: Option<bool>,
) -> Result<Value, UiError> {
    let mit_fluegen = fluege == Some(true);
    let zeiten = zeiten.unwrap_or_default();
    if zeiten.len() > 500 {
        return Err(UiError::new("bild_eingabe", "Hoechstens 500 Zeiten auf einmal"));
    }
    let ms: Vec<i64> = zeiten
        .iter()
        .map(|z| kontext::zeit_ms(z).ok_or_else(|| UiError::new("bild_eingabe", format!("Zeit unlesbar: {z}"))))
        .collect::<Result<_, _>>()?;
    if ms.is_empty() && mit_spur != Some(true) && !mit_fluegen {
        return Err(UiError::new("bild_eingabe", "zeiten, mitSpur oder fluege noetig"));
    }
    let client = state.client.lock().expect("client mutex").as_ref().cloned();
    let aktiv = state.active_flight.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let aktiv_pirep = aktiv.as_ref().map(|f| f.pirep_id.clone());
    let aktiv_spur: Option<Vec<[f64; 2]>> = if mit_spur == Some(true) && aktiv_pirep.is_some() && (pirep_id.is_none() || pirep_id == aktiv_pirep) {
        aktiv.as_ref().and_then(|f| f.stats.lock().ok().map(|s| s.track.clone()))
    } else {
        None
    };
    let dir = app.path().app_data_dir().map(|d| d.join("flight_logs")).map_err(|e| UiError::new("bild_io", e.to_string()))?;
    let (ms2, pirep2) = (ms.clone(), pirep_id.clone());
    let hat_aktive_spur = aktiv_spur.is_some();
    let (treffer, spur_datei, protokoll_fluege) = tauri::async_runtime::spawn_blocking(move || {
        crate::flugprotokoll_abwarten(); // alles Bisherige steht in der Datei
        let t = kontext::protokoll_staende(&dir, pirep2.as_deref(), &ms2).map_err(fehler)?;
        let spur = if mit_spur == Some(true) && !hat_aktive_spur {
            let id = pirep2.or_else(|| t.iter().flatten().find_map(|x| x.stand.get("pirep_id").and_then(Value::as_str).map(str::to_string)));
            id.and_then(|i| kontext::protokoll_spur(&dir, &i).ok())
        } else {
            None
        };
        let fluege = if mit_fluegen { kontext::protokoll_fluege(&dir) } else { Vec::new() };
        Ok::<_, UiError>((t, spur, fluege))
    })
    .await
    .map_err(|e| UiError::new("bild_io", e.to_string()))??;

    // Rueckfall Logbuch fuer alles, was das Protokoll nicht abdeckt.
    let offen: Vec<i64> = ms.iter().zip(&treffer).filter(|(_, t)| t.is_none()).map(|(z, _)| *z).collect();
    // Die Flugliste reicht einen Monat hinter die Bildzeit zurueck: Die Dateizeit eines Bildes kann spaeter liegen als der Flug.
    let bezug = ms.first().copied().unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let frueh = offen.iter().copied().chain(mit_fluegen.then_some(bezug - 30 * 86_400_000)).min();
    let pireps = match (&client, frueh) {
        (Some(c), Some(frueh)) => logbuch_pireps(c, frueh).await,
        _ => Vec::new(),
    };
    let staende: Vec<Value> = zeiten
        .iter()
        .zip(&ms)
        .zip(treffer)
        .map(|((iso, z), t)| {
            let (quelle, mut stand, abstand) = match t {
                Some(t) => ("protokoll", t.stand, Some(t.abstand_ms)),
                None => match kontext::logbuch_stand(&pireps, *z) {
                    Some(s) => ("logbuch", s, None),
                    None => ("keine", kontext::Stand::new(), None),
                },
            };
            stand.insert("zeit".into(), json!(iso));
            stand.insert("quelle".into(), json!(quelle));
            if let Some(a) = abstand {
                stand.insert("abstand_s".into(), json!((a as f64 / 1000.0).round()));
            }
            if let (Some(la), Some(lo)) = (stand.get("breite").and_then(Value::as_f64), stand.get("laenge").and_then(Value::as_f64)) {
                stand.insert("naechster_flugplatz".into(), naechster_flugplatz(la, lo).unwrap_or(Value::Null));
            }
            Value::Object(stand)
        })
        .collect();
    let spur = aktiv_spur.map(|s| kontext::ausduennen(s, kontext::MAX_SPUR)).or(spur_datei).unwrap_or_default();
    let mut antwort = json!({"staende": staende, "spur": spur});
    if mit_fluegen {
        let liste = kontext::fluege_ordnen(protokoll_fluege, &pireps, ms.first().copied());
        antwort["fluege"] = liste.iter().map(kontext::FlugEintrag::als_json).collect();
    }
    Ok(antwort)
}

/// Die PIREPs des Piloten bei phpVMS, seitenweise zurueck, bis die fruehste gefragte
/// Zeit abgedeckt ist (hoechstens 15 Seiten). Fehler beenden das Blaettern still:
/// es bleibt bei `keine`, nichts wird geraten.
async fn logbuch_pireps(client: &api_client::Client, fruehestens_ms: i64) -> Vec<PirepKurz> {
    let mut alle = Vec::new();
    for seite in 1..=15u32 {
        let Ok(v) = client.get_user_pireps_seite(seite).await else { break };
        if v.is_empty() {
            break;
        }
        let aelteste = v.iter().filter_map(|p| p.created_at.as_deref().and_then(kontext::zeit_ms)).min();
        let letzte_seite = v.len() < 20;
        alle.extend(v.into_iter().map(|p| PirepKurz {
            pirep_id: p.id,
            block_off_ms: p.block_off_time.as_deref().and_then(kontext::zeit_ms),
            block_on_ms: p.block_on_time.as_deref().and_then(kontext::zeit_ms),
            flugnummer: p.flight_number,
            abflug: p.dpt_airport_id,
            ziel: p.arr_airport_id,
            flugzeug_icao: p.aircraft_icao,
        }));
        // 2 Tage Puffer: der PIREP wird vor dem Block-off angelegt
        if letzte_seite || aelteste.is_some_and(|a| a < fruehestens_ms - 2 * 86_400_000) {
            break;
        }
    }
    alle
}

/// Markiert das JPEG eines Bildes im Dateimanager des Systems (Finder „Im Finder
/// zeigen“, Explorer „/select“). Pfad-Ausbruch und Links sind ausgeschlossen wie
/// bei `bild_lesen`; es muss ein vorhandenes JPEG im verwalteten Ordner sein.
#[tauri::command(async)]
pub(crate) fn bild_ordner_zeigen(app: AppHandle, gruppe: String, id: String) -> Result<(), UiError> {
    let p = speicher::jpeg_pfad(&wurzel(&app)?, &gruppe, &id).map_err(fehler)?;
    tauri_plugin_opener::reveal_item_in_dir(&p).map_err(|e| UiError::new("bild_io", format!("Dateimanager nicht geoeffnet: {e}")))
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
pub(crate) fn bild_loeschen(app: AppHandle, bilder: Vec<BildZiel>) -> Result<Vec<String>, UiError> {
    let weg = |p: &std::path::Path| papierkorb(p).map_err(|e| std::io::Error::other(e.to_string()));
    let wurzel = wurzel(&app)?;
    let mut entfernt = Vec::new();
    for b in &bilder {
        entfernt.extend(speicher::loeschen(&wurzel, &b.gruppe, &b.id, &weg).map_err(fehler)?);
    }
    Ok(entfernt)
}

/// Ein Bild in `bild_loeschen`: alle gewaehlten Bilder gehen mit einer Rueckfrage in einem Aufruf.
#[derive(serde::Deserialize)]
pub(crate) struct BildZiel {
    gruppe: String,
    id: String,
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
    request: tauri::ipc::Request<'_>,
) -> Result<Value, UiError> {
    // `yokeTaste` fehlt (unveraendert) oder ist `null` (loeschen): Tauri unterscheidet beides bei `Option` nicht.
    let yoke_taste = match request.body() {
        tauri::ipc::InvokeBody::Json(v) => bilder_kern::yoke::aus_auftrag(v).map_err(fehler)?,
        _ => None,
    };
    let ordner = if ordner_waehlen == Some(true) {
        match ordner_dialog(&app).await? {
            Some(p) => Some(p),
            None => return Ok(aktueller_stand(&einstellungen_lesen_app(&app)?)), // abgebrochen: nichts aendert sich
        }
    } else {
        None
    };
    tauri::async_runtime::spawn_blocking(move || quellen_setzen_intern(&app, quellen, tastenkuerzel, yoke_taste, ordner))
        .await
        .map_err(|e| UiError::new("bild_io", e.to_string()))?
}

fn einstellungen_lesen_app(app: &AppHandle) -> Result<Einstellungen, UiError> {
    Ok(quellen::einstellungen_lesen(&einstellungen_datei(app)?))
}

fn aktueller_stand(e: &Einstellungen) -> Value {
    json!({"quellen": entsperren(anzeige()).status, "tastenkuerzel": tastenkuerzel_stand(e), "abgebrochen": true})
}

fn quellen_setzen_intern(
    app: &AppHandle,
    quellen: Option<Vec<Quelle>>,
    tastenkuerzel: Option<String>,
    yoke_taste: Option<Option<bilder_kern::yoke::YokeTaste>>,
    ordner: Option<PathBuf>,
) -> Result<Value, UiError> {
    let datei = einstellungen_datei(app)?;
    let bisher = quellen::einstellungen_lesen(&datei);
    let neu = quellen::einstellungen_neu(&bisher, quellen, tastenkuerzel.as_deref(), yoke_taste, ordner.as_deref(), &wurzel(app)?, app.path().home_dir().ok().as_deref())
        .map_err(fehler)?;
    quellen::einstellungen_schreiben(&datei, &neu).map_err(fehler)?;
    beobachtung_aufsetzen(&neu.quellen);
    hotkey_anwenden(&neu.tastenkuerzel);
    yoke_anwenden(neu.yoke.as_ref());
    Ok(json!({"quellen": entsperren(anzeige()).status, "tastenkuerzel": tastenkuerzel_stand(&neu)}))
}

/// Ordnerdialog des Systems; `None`, wenn der Pilot abbricht.
async fn ordner_dialog(app: &AppHandle) -> Result<Option<PathBuf>, UiError> {
    use tauri_plugin_dialog::DialogExt;
    let _verborgen = crate::huelle::fuer_systemdialog(&app);
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
    let _verborgen = crate::huelle::fuer_systemdialog(&app);
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
    let _verborgen = crate::huelle::fuer_systemdialog(&app);
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

fn yoke_anwenden(taste: Option<&bilder_kern::yoke::YokeTaste>) {
    if let Some(y) = entsperren(yoke()).as_ref() {
        y.setzen(taste.cloned());
    }
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
        let datei = einstellungen_datei(&app);
        let mut einst = datei.as_ref().map(|d| quellen::einstellungen_lesen(d)).unwrap_or_default();
        // Alte Einstellungen kennen die Identitaet der Quellen noch nicht: jetzt binden und merken.
        if quellen::identitaet_binden(&mut einst) {
            if let Ok(d) = &datei {
                if let Err(e) = quellen::einstellungen_schreiben(d, &einst) {
                    tracing::warn!(fehler = %e, "Bilder: Identitaet der Quellen nicht gespeichert");
                }
            }
        }
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
        if plattform::YOKE_VERFUEGBAR {
            let app_yoke = app.clone();
            // Beim Lernen ist ein Druck die Antwort, keine Aufnahme (auch beim schon belegten Knopf).
            let bei_knopf = move || {
                if !YOKE_LERNT.load(Ordering::SeqCst) {
                    taste_gedrueckt(&app_yoke);
                }
            };
            match plattform::yoke_starten(bei_knopf) {
                Ok(y) => {
                    *entsperren(yoke()) = Some(y);
                    yoke_anwenden(einst.yoke.as_ref());
                }
                Err(e) => tracing::warn!(fehler = %e, "Yoke-Knopf der Bildaufnahme nicht gestartet"),
            }
        }
        let start = std::time::Instant::now();
        loop {
            std::thread::sleep(TAKT);
            let vordergrund = plattform::sim_vordergrund();
            // Sperre nur zum Holen der Quellen: die Ordnerabfragen laufen ohne sie,
            // je Quelle in einem eigenen Faden; ein haengender Ordner haelt hoechstens 1 s auf.
            let (slots, vg) = entsperren(beobachter()).takt_vorbereiten(vordergrund);
            let jetzt = start.elapsed().as_millis() as u64;
            let funde = bilder_kern::beobachter::takt_ausfuehren(&slots, jetzt, vg);
            let status = entsperren(beobachter()).status();
            entsperren(anzeige()).status = status;
            // Zuerst dauerhaft im Eingang; das Ereignis ist nur ein Wecksignal.
            funde_abarbeiten(&slots, funde, jetzt, |f| fund_in_eingang(&app, f), |e| {
                ans_tablet(&app, EREIGNIS_QUELLE_NEU, e);
                if let Err(err) = app.emit(EREIGNIS_QUELLE_NEU, e) {
                    tracing::warn!(fehler = %err, "bild_quelle_neu nicht gesendet");
                }
            });
        }
    });
}

/// Dieselben Wecksignale fuer offene Tablets.
fn ans_tablet(app: &AppHandle, ereignis: &str, nutzlast: &impl serde::Serialize) {
    if let Ok(wert) = serde_json::to_value(nutzlast) {
        app.state::<crate::AppState>().remote_events.send(crate::remote::RemoteEvent::new(ereignis, wert));
    }
}

/// Tastenkuerzel oder Yoke-Knopf gedrueckt: ohne aktiven Flug nichts aufnehmen (Ereignis mit
/// `kein_flug`), sonst sofort aufnehmen (der Moment zaehlt). Ergebnis als
/// Ereignis `bild_aufgenommen`; das Paket ordnet zu und schreibt das JPEG.
fn taste_gedrueckt(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("bild-aufnahme".into()).spawn(move || {
        let nutzlast = aufgenommen_nutzlast(flug_aktiv(&app), || aufnehmen_intern(&app, EINGANG));
        nutzlast_rueckmeldung(&nutzlast);
        ans_tablet(&app, EREIGNIS_AUFGENOMMEN, &nutzlast);
        let _ = app.emit(EREIGNIS_AUFGENOMMEN, nutzlast);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn das_tablet_liest_nur_fertige_bilder() {
        assert!(fuers_tablet("P1/b1.jpg"));
        assert!(fuers_tablet("P1/b1.vorschau.jpg"));
        assert!(!fuers_tablet("eingang/b1.quelle.jpg"));
        assert!(!fuers_tablet("eingang/b1.quelle.png"));
        assert!(!fuers_tablet("P1/b1.json"));
    }

    #[test]
    fn nur_eine_andere_position_macht_das_exif_im_jpeg_alt() {
        let mit = |la: f64| json!({"text": "x", "meta": {"breite": la, "laenge": 8.0}});
        assert!(!position_geaendert(Some(&mit(50.0)), &json!({"text": "neu", "meta": {"breite": 50.0, "laenge": 8.0}})));
        assert!(position_geaendert(Some(&mit(50.0)), &mit(51.0)));
        assert!(position_geaendert(Some(&mit(50.0)), &json!({"meta": {"breite": null, "laenge": null}})), "Ohne Flug: Position weg");
        assert!(!position_geaendert(None, &mit(50.0)));
    }

    #[test]
    fn das_tablet_aendert_den_hash_nicht() {
        let mut neu = json!({"text": "x", "sha256": "fremd"});
        sha_behalten(&mut neu, Some(&json!({"sha256": "echt"})));
        assert_eq!(neu, json!({"text": "x", "sha256": "echt"}));
        let mut ohne = json!({"sha256": "fremd"});
        sha_behalten(&mut ohne, Some(&json!({})));
        assert_eq!(ohne, json!({}));
    }

    #[test]
    fn senden_werte_aus_der_oberflaeche() {
        let w: SendenWerte = serde_json::from_value(json!({"sendungId": S, "text": "Hallo", "geoStufe": "verdeckt", "bilder": [{"gruppe": "P1", "bild_id": B1, "meta": {"fassung": 1}}]})).unwrap();
        assert_eq!((w.sendung_id.as_str(), w.text.as_deref(), w.geo_stufe.as_deref(), w.bilder.len()), (S, Some("Hallo"), Some("verdeckt"), 1));
        assert!(serde_json::from_value::<SendenWerte>(json!({"text": "ohne Kennung"})).is_err());
    }
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

    /// Fund 5: Ablage im Eingang misslungen (Datentraeger voll) -> die Datei wird
    /// erneut gemeldet; ohne Flug dagegen entsteht kein Fund und sie bleibt still.
    #[test]
    fn misslungene_ablage_meldet_die_datei_erneut_ohne_flug_bleibt_sie_still() {
        let t = tempfile::tempdir().unwrap();
        let png = bilder_kern::png::aus_bgra(2, 2, &[0u8; 16]).unwrap();
        let quelle = Quelle { pfad: t.path().to_string_lossy().into_owned(), aktiv: true, original_entfernen: false, art: None, kanonisch: None };
        let mut b = Beobachter::neu();
        b.aufsetzen(&[quelle], None);
        let (slots, _) = b.takt_vorbereiten(None);
        std::fs::write(t.path().join("a.png"), &png).unwrap();
        let mut zeit = 0u64;
        let mut takt = |zeit: &mut u64| {
            let f = bilder_kern::beobachter::takt_ausfuehren(&slots, *zeit, None);
            *zeit += 1500;
            f
        };
        assert!(takt(&mut zeit).is_empty());
        let funde = takt(&mut zeit);
        assert_eq!(funde.len(), 1);
        let mut gemeldet = 0;
        funde_abarbeiten(&slots, funde, zeit, |_| Ablage::Misslungen, |_| gemeldet += 1);
        assert_eq!(gemeldet, 0, "ohne Eintrag kein Wecksignal");
        zeit += 10_000;
        let wieder = takt(&mut zeit);
        assert_eq!(wieder.len(), 1, "die Datei wird erneut gemeldet");
        // Jetzt klappt das Schreiben: abgelegt, gemeldet, danach still.
        let meta = json!({"pirep_id": "P1"});
        funde_abarbeiten(&slots, wieder, zeit, |f| Ablage::Abgelegt(Box::new(EingangsFund::aus_fund(f, "f1".into(), 1, None, meta.clone()))), |_| gemeldet += 1);
        assert_eq!(gemeldet, 1);
        assert!(takt(&mut zeit).is_empty() && takt(&mut zeit).is_empty());
        // Ohne Flug: kein Fund, nicht erneut melden (Datei bleibt unangetastet).
        std::fs::write(t.path().join("b.png"), &png).unwrap();
        assert!(takt(&mut zeit).is_empty());
        let b_fund = takt(&mut zeit);
        assert_eq!(b_fund.len(), 1);
        funde_abarbeiten(&slots, b_fund, zeit, |_| Ablage::KeinFlug, |_| gemeldet += 1);
        zeit += 20_000;
        assert!(takt(&mut zeit).is_empty());
        assert_eq!(gemeldet, 1);
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
    fn senden_anfrage_baut_den_vertrag_aus_den_gelesenen_bildern() {
        let bild = LiveBild { bild_id: B1.into(), jpeg: vec![0xFF, 0xD8, 0xFF, 0xD9], meta: json!({"fassung": 1}) };
        let a = senden_anfrage(S, Some("Hallo"), None, std::slice::from_ref(&bild)).unwrap();
        let LiveAnfrage::Senden(teile) = a else { panic!("Senden erwartet") };
        assert_eq!(teile, teile_fuer_test());
        assert_eq!(senden_anfrage("keine-uuid", None, None, &[bild]).err().map(|e| e.code), Some("bild_eingabe".to_string()));
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
