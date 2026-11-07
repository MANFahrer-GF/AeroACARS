//! Bilder-Tab (N-13, Faehigkeit `bilder-1`, P-11.30 bis P-11.34): die zehn
//! Befehle, das Ereignis `bild_quelle_neu` und das Tastenkuerzel.
//!
//! Grundsatz (P-11.33): Dieser Teil liefert nur Plattform-Bausteine. Aktiver
//! Flug, Zuordnung, Metadaten, Verkleinern, EXIF und Filter sind Sache des
//! Laufzeit-Pakets. Die Logik steht in `crates/bilder-kern` (ohne Tauri
//! testbar); die Befehle hier sind duenne Adapter.
//!
//! Byte-Vertrag der Befehle:
//! * `bild_lesen` gibt die Bytes **roh** zurueck (`ArrayBuffer`, kein JSON-Feld).
//! * Bytes an den Kern (`bild_schreiben`, `bild_live`, `bild_export`) gehen als
//!   **Base64** (Standard-Alphabet) in den JSON-Argumenten; so funktionieren sie
//!   auch ueber `shell_request` (Hülle) unveraendert.
//!
//! Argumente (Tauri-Standard: oberste Ebene camelCase, Felder verschachtelter
//! Objekte wie im Vertrag snake_case):
//! * `bilder_liste({mitAltbestand?})` -> `{eintraege, belegt_bytes, quellen, tastenkuerzel, aufnahme_verfuegbar, altbestand?}`
//! * `bild_lesen({pfad})` -> ArrayBuffer (`pfad` relativ aus der Liste)
//! * `bild_quellen_finden()` -> `[{pfad, art, simulator, existiert, vorschlag_modus}]`
//! * `bild_uebernehmen({pfad, modus, gruppe, id?})` -> Eingangskopie `<gruppe>/<id>.quelle.png|jpg`
//! * `bild_schreiben({gruppe, id, jpegBase64?, vorschauBase64?, sidecar?, quelleEntfernen?})` -> Eintrag
//! * `bild_aufnehmen({gruppe?})` -> `{id, gruppe, quelle_pfad, breite, hoehe, ...}`
//! * `bild_live({art: "senden"|"status", sendungId?, text?, geoStufe?, bilder?: [{bild_id, jpeg_base64, meta}]})`
//! * `bild_loeschen({gruppe, id, bezeichnung?})`, `bild_quellen_setzen({quellen: [{pfad, aktiv, original_entfernen, art}], tastenkuerzel?})`,
//!   `bild_export({zipBase64, dateiname})`
//! * Ereignisse: `bild_quelle_neu` (Fund), `bild_aufgenommen` (`{ok, aufnahme|code, meldung}`, nach dem Tastenkuerzel)
//!
//! Stufen (befehle.json): lesen `bilder_liste`, `bild_lesen`,
//! `bild_quellen_finden`; fachlich `bild_uebernehmen`, `bild_schreiben`,
//! `bild_aufnehmen`, `bild_live`; system `bild_loeschen`, `bild_quellen_setzen`,
//! `bild_export`. Die LAN-Bruecke reicht keinen davon durch.
//!
//! Auslegung: Weil es genau zehn Befehle sind, tragen zwei Antworten mehr als
//! der Name sagt: `bilder_liste` liefert auch die bestaetigten Quellen mit
//! Beobachtungszustand, den Altbestand (auf Wunsch) und das Tastenkuerzel;
//! `bild_quellen_setzen` setzt neben den Ordnern auch das Tastenkuerzel (beides
//! ist eine Systemeinstellung und laeuft ueber denselben Hüllen-Dialog).

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

/// Beobachtung auf die bestaetigte Liste setzen und das Anzeige-Abbild erneuern.
fn beobachtung_aufsetzen(quellen: &[Quelle]) {
    let vordergrund = plattform::sim_vordergrund();
    let (status, alt) = {
        let mut b = entsperren(beobachter());
        b.aufsetzen(quellen, vordergrund);
        (b.status(), b.altbestand())
    };
    *entsperren(anzeige()) = Anzeige { status, alt };
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
    quellen::uebernehmen(&wurzel(&app)?, &einst.quellen, &pfad, modus, &gruppe, id.as_deref(), Duration::from_millis(400)).map_err(fehler)
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
fn aufnehmen_intern(app: &AppHandle, gruppe: &str) -> Result<Value, UiError> {
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

/// Ein Bild der Sendung: die zu sendenden JPEG-Bytes (nach Verkleinern, ohne
/// EXIF; das macht das Paket) und das Metadaten-Dokument.
#[derive(Debug, Deserialize)]
pub(crate) struct LiveBildEin {
    bild_id: String,
    jpeg_base64: String,
    meta: Value,
}

/// Eine Anfrage an `POST /api/bilder` (`art: "senden"`) oder
/// `GET /api/bilder/status` (`art: "status"`, `sendung_id` optional). Das
/// Pilot-Token haengt der Kern an; es erreicht die Oberflaeche nie. Die
/// Antwort des Servers geht unveraendert zurueck.
///
/// Fehlercodes: `live_nicht_angemeldet` (kein Token, 401/403),
/// `live_nicht_erreichbar` (Verbindung kam nicht zustande: Senden wartet),
/// `live_antwort_fehlt` (Verbindung stand, Antwort blieb aus: Zustellung
/// unklar, erst `status` mit derselben `sendung_id` fragen),
/// `live_kontingent` (429, `details.retry_after`), `live_fehler` (alles andere,
/// `details.status`).
#[tauri::command]
pub(crate) async fn bild_live(
    art: String,
    sendung_id: Option<String>,
    text: Option<String>,
    geo_stufe: Option<String>,
    bilder: Option<Vec<LiveBildEin>>,
) -> Result<Value, UiError> {
    let anfrage = match art.as_str() {
        "senden" => {
            let bilder = bilder
                .unwrap_or_default()
                .into_iter()
                .map(|b| Ok(LiveBild { jpeg: b64(&b.jpeg_base64, "jpeg_base64")?, bild_id: b.bild_id, meta: b.meta }))
                .collect::<Result<Vec<_>, UiError>>()?;
            let sendung = sendung_id.ok_or_else(|| UiError::new("bild_eingabe", "sendung_id fehlt"))?;
            let teile = live::senden_teile(&sendung, text.as_deref(), geo_stufe.as_deref().unwrap_or("verdeckt"), &bilder).map_err(fehler)?;
            LiveAnfrage::Senden(teile)
        }
        "status" => LiveAnfrage::Status(sendung_id),
        _ => return Err(UiError::new("bild_eingabe", "art muss „senden“ oder „status“ sein")),
    };
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

/// Entfernt ein Bild mit allen Teilen: JPEG (und eine uebrige Eingangskopie) in
/// den Papierkorb, Vorschau und Sidecar endgueltig. Nie nur ein Teil. Der
/// Hüllen-Dialog zeigt `bezeichnung`, falls das Paket sie mitgibt.
#[tauri::command(async)]
pub(crate) fn bild_loeschen(app: AppHandle, gruppe: String, id: String) -> Result<Vec<String>, UiError> {
    let weg = |p: &std::path::Path| trash::delete(p).map_err(|e| std::io::Error::other(e.to_string()));
    speicher::loeschen(&wurzel(&app)?, &gruppe, &id, &weg).map_err(fehler)
}

/// Speichert die bestaetigte Liste der Bildquellen (und, wenn angegeben, das
/// Tastenkuerzel) und setzt Beobachtung und Tastenkuerzel neu auf. Gibt den
/// Stand wie `bilder_liste` zurueck (`quellen`, `tastenkuerzel`).
#[tauri::command(async)]
pub(crate) fn bild_quellen_setzen(app: AppHandle, quellen: Vec<Quelle>, tastenkuerzel: Option<String>) -> Result<Value, UiError> {
    let datei = einstellungen_datei(&app)?;
    let bisher = quellen::einstellungen_lesen(&datei);
    let neu = Einstellungen {
        quellen,
        tastenkuerzel: match tastenkuerzel {
            Some(t) => tastenkuerzel::lesen(&t).map_err(fehler)?.text,
            None => bisher.tastenkuerzel,
        },
    };
    quellen::einstellungen_pruefen(&neu, &wurzel(&app)?, app.path().home_dir().ok().as_deref()).map_err(fehler)?;
    quellen::einstellungen_schreiben(&datei, &neu).map_err(fehler)?;
    beobachtung_aufsetzen(&neu.quellen);
    hotkey_anwenden(&neu.tastenkuerzel);
    Ok(json!({"quellen": entsperren(anzeige()).status, "tastenkuerzel": tastenkuerzel_stand(&neu)}))
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
            let (funde, status) = {
                let mut b = entsperren(beobachter());
                (b.takt(start.elapsed().as_millis() as u64, vordergrund), b.status())
            };
            entsperren(anzeige()).status = status;
            for f in funde {
                if let Err(e) = app.emit(EREIGNIS_QUELLE_NEU, &f) {
                    tracing::warn!(fehler = %e, "bild_quelle_neu nicht gesendet");
                }
            }
        }
    });
}

/// Tastenkuerzel gedrueckt: sofort aufnehmen (der Moment zaehlt), Ergebnis als
/// Ereignis `bild_aufgenommen`; das Paket ordnet zu und schreibt das JPEG.
fn taste_gedrueckt(app: &AppHandle) {
    let app = app.clone();
    let _ = std::thread::Builder::new().name("bild-aufnahme".into()).spawn(move || {
        let nutzlast = match aufnehmen_intern(&app, EINGANG) {
            Ok(v) => json!({"ok": true, "aufnahme": v}),
            Err(e) => json!({"ok": false, "code": e.code, "meldung": e.message}),
        };
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
}
