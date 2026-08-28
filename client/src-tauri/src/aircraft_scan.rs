//! Aircraft-Scan — "Geladenes Flugzeug analysieren"-Feature.
//!
//! Der Pilot schickt einen GEFILTERTEN Auszug eines MSFS-Addon-Pakets
//! (nur cfg/json/xml/js + panel-WASM — nie Texturen/Modelle/Sounds) an
//! live.kant.ovh (`POST /api/ascan/submissions?source=client`). Der
//! Recorder analysiert LVar-Kandidaten/Engines/Titles fuer AeroACARS-
//! Profile; der Pilot sieht seine Einreichung (exakte Dateiliste, DSGVO)
//! unter https://live.kant.ovh/aircraft/.
//!
//! Transparenz-Regel: `ascan_collect` liefert die exakte Dateiliste an die
//! UI, die sie dem Piloten VOR dem Senden anzeigt. `ascan_submit` sendet
//! GENAU dieselbe Auswahl (gleiche Whitelist, gleicher Walk, gleiche Kappen
//! — deterministisch), es gibt keinen zweiten, verdeckten Sammel-Pfad.
//!
//! Sicherheits-Seam: Die Webview waehlt Pakete nur ueber einen INDEX in die
//! serverseitig (Rust) gehaltene Paketliste — es wandern keine freien Pfade
//! vom Frontend in Datei-Operationen. Der optionale `manual_dir` (Pilot
//! tippt seinen Community-Pfad, z.B. Addon-Linker-Setups) wird nur als
//! Wurzel fuer die manifest.json-Suche verwendet.

use serde::Serialize;
use std::io::{Cursor, Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Whitelist-Kappen — MUESSEN mit aircraft-webapp/src/scanner.ts und
/// recorder/src/aircraftAnalyzer.ts (isAllowedUploadPath) uebereinstimmen.
const MAX_TEXT_FILE: u64 = 5 * 1024 * 1024;
const MAX_WASM_FILE: u64 = 110 * 1024 * 1024;
const MAX_TOTAL: u64 = 400 * 1024 * 1024;
const MAX_FILES: usize = 3000;

const DEFAULT_ENDPOINT: &str = "https://live.kant.ovh/api/ascan/submissions";

/// Vom letzten `ascan_list_aircraft` gefundene Paket-Wurzeln. Die UI
/// referenziert Pakete ausschliesslich per Index in diese Liste.
#[derive(Default)]
pub struct AircraftScanState {
    packages: Mutex<Vec<ScanPackage>>,
}

#[derive(Clone)]
struct ScanPackage {
    dir: PathBuf,
    folder: String,
}

#[derive(Serialize, Clone)]
pub struct FoundAircraft {
    pub index: usize,
    pub folder: String,
    pub title: String,
    pub creator: Option<String>,
    /// Woher das Paket stammt (Community-Ordner-Pfad, gekuerzt fuer die UI)
    pub source_dir: String,
}

#[derive(Serialize)]
pub struct CollectedFile {
    pub path: String,
    pub size: u64,
}

#[derive(Serialize)]
pub struct CollectResult {
    pub files: Vec<CollectedFile>,
    pub total_bytes: u64,
    pub skipped_large: Vec<String>,
}

#[derive(Serialize)]
pub struct SubmitResult {
    pub ok: bool,
    pub id: Option<String>,
    pub status: Option<String>,
    pub zip_bytes: u64,
    /// Kompakter Report-Auszug fuer die UI (Rest sieht der Pilot im Web)
    pub icao: Option<String>,
    pub lvar_count: Option<i64>,
    pub external_process_suspected: Option<bool>,
    pub warnings: Vec<String>,
}

// ─── Community-Ordner finden ────────────────────────────────────────────

/// UserCfg.opt-Kandidaten fuer MSFS 2020/2024, Store + Steam.
fn usercfg_candidates() -> Vec<(PathBuf, &'static str)> {
    let mut out = Vec::new();
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        out.push((
            Path::new(&local)
                .join("Packages/Microsoft.FlightSimulator_8wekyb3d8bbwe/LocalCache/UserCfg.opt"),
            "MSFS 2020 (Microsoft Store)",
        ));
        out.push((
            Path::new(&local)
                .join("Packages/Microsoft.Limitless_8wekyb3d8bbwe/LocalCache/UserCfg.opt"),
            "MSFS 2024 (Microsoft Store)",
        ));
    }
    if let Ok(roaming) = std::env::var("APPDATA") {
        out.push((
            Path::new(&roaming).join("Microsoft Flight Simulator/UserCfg.opt"),
            "MSFS 2020 (Steam)",
        ));
        out.push((
            Path::new(&roaming).join("Microsoft Flight Simulator 2024/UserCfg.opt"),
            "MSFS 2024 (Steam)",
        ));
    }
    out
}

/// `InstalledPackagesPath "D:\MSFS"` aus einer UserCfg.opt ziehen.
fn parse_installed_packages_path(text: &str) -> Option<PathBuf> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("InstalledPackagesPath") {
            let rest = rest.trim();
            let unquoted = rest.trim_matches('"');
            if !unquoted.is_empty() {
                return Some(PathBuf::from(unquoted));
            }
        }
    }
    None
}

/// Alle auffindbaren Community-Ordner (dedupliziert).
fn community_dirs() -> Vec<(PathBuf, &'static str)> {
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    for (cfg_path, label) in usercfg_candidates() {
        let Ok(text) = std::fs::read_to_string(&cfg_path) else {
            continue;
        };
        let Some(base) = parse_installed_packages_path(&text) else {
            continue;
        };
        let community = base.join("Community");
        if community.is_dir() && !out.iter().any(|(p, _)| p == &community) {
            out.push((community, label));
        }
    }
    out
}

/// manifest.json eines Top-Level-Paket-Ordners lesen; nur AIRCRAFT zaehlt.
fn read_aircraft_manifest(pkg_dir: &Path) -> Option<(String, Option<String>)> {
    let manifest_path = pkg_dir.join("manifest.json");
    let meta = std::fs::metadata(&manifest_path).ok()?;
    if meta.len() > 1024 * 1024 {
        return None;
    }
    let text = std::fs::read_to_string(&manifest_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let ct = json.get("content_type")?.as_str()?;
    if !ct.eq_ignore_ascii_case("AIRCRAFT") {
        return None;
    }
    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| pkg_dir.file_name().and_then(|n| n.to_str()).unwrap_or("?"))
        .to_string();
    let creator = json
        .get("creator")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Some((title, creator))
}

/// X-Plane: ein Ordner mit einer `.acf`-Datei ist ein Flugzeug. Titel/Studio
/// aus der text-basierten .acf (`P acf/_name` / `_studio`), sonst Ordnername.
/// Nur der Kopf der .acf wird gelesen (sie kann mehrere MB sein; die
/// _name/_studio-Header stehen weit oben).
fn read_xplane_acf(pkg_dir: &Path) -> Option<(String, Option<String>)> {
    let mut acf_path: Option<PathBuf> = None;
    for entry in std::fs::read_dir(pkg_dir).ok()?.flatten() {
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .map(|x| x.eq_ignore_ascii_case("acf"))
                .unwrap_or(false)
        {
            acf_path = Some(p);
            break;
        }
    }
    let acf_path = acf_path?;
    let head = read_file_head(&acf_path, 64 * 1024).unwrap_or_default();
    let folder_name = pkg_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let name = acf_prop(&head, "acf/_name").unwrap_or(folder_name);
    let studio = acf_prop(&head, "acf/_studio");
    Some((name, studio))
}

/// Liest eine .acf-Property-Zeile `P <key> <wert>`.
fn acf_prop(text: &str, key: &str) -> Option<String> {
    let prefix = format!("P {key} ");
    for line in text.lines() {
        if let Some(v) = line.strip_prefix(&prefix) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

/// Die ersten `max` Bytes einer Datei als (lossy) String.
fn read_file_head(path: &Path, max: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; max];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Ein Ordner als Flugzeug-Paket erkennen — MSFS (manifest.json) ODER
/// X-Plane (.acf). Damit funktioniert das Direkt-Wählen EINES Flugzeug-
/// Ordners für beide Sims, egal wo er liegt (via manual_dir).
fn detect_aircraft(pkg_dir: &Path) -> Option<(String, Option<String>)> {
    read_aircraft_manifest(pkg_dir).or_else(|| read_xplane_acf(pkg_dir))
}

/// X-Plane schreibt seine Installationspfade in `x-plane_install_{12,11}.txt`
/// (eine Zeile pro Install). Wir hängen `/Aircraft` an und nehmen die als
/// Scan-Wurzeln. Best-effort für Win/Mac/Linux; findet nichts → der Pilot
/// gibt seinen Aircraft-Ordner manuell an (manual_dir).
fn xplane_aircraft_dirs() -> Vec<(PathBuf, &'static str)> {
    let mut install_files: Vec<PathBuf> = Vec::new();
    for ver in ["12", "11"] {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            install_files.push(PathBuf::from(&local).join(format!("x-plane_install_{ver}.txt")));
        }
        if let Ok(home) = std::env::var("HOME") {
            install_files.push(
                PathBuf::from(&home).join(format!("Library/Preferences/x-plane_install_{ver}.txt")),
            );
            install_files
                .push(PathBuf::from(&home).join(format!(".x-plane/x-plane_install_{ver}.txt")));
        }
    }
    let mut out: Vec<(PathBuf, &'static str)> = Vec::new();
    for f in install_files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let ac = PathBuf::from(line).join("Aircraft");
            if ac.is_dir() && !out.iter().any(|(p, _)| p == &ac) {
                out.push((ac, "X-Plane (Aircraft-Ordner)"));
            }
        }
    }
    out
}

// ─── Whitelist (Spiegel von scanner.ts / aircraftAnalyzer.ts) ───────────

fn is_text_ext(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        // MSFS-Text + X-Plane-Text (.acf Aircraft-Datei, .lua SASL/XLua).
        ".cfg", ".json", ".xml", ".js", ".mjs", ".html", ".htm", ".txt", ".ini", ".yaml", ".yml",
        ".lua", ".acf",
    ]
    .iter()
    .any(|e| lower.ends_with(e))
}

fn is_wasm_ext(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    // MSFS-WASM + X-Plane-.xpl-Plugin-Binaries (Dataref-Strings wie WASM).
    lower.ends_with(".wasm") || lower.ends_with(".xpl")
}

/// Verzeichnisse, in die wir nicht absteigen (die Gigabyte-Fresser).
/// `model*` wird betreten, aber nur .xml daraus mitgenommen (Behaviors).
/// MSFS: texture/sound/model…; X-Plane: objects (.obj-3D), liveries,
/// cockpit(_3d)-Texturen.
fn is_excluded_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.split('.').next().unwrap_or(&lower);
    matches!(
        base,
        "texture"
            | "textures"
            | "sound"
            | "soundai"
            | "effects"
            | "autogen"
            | "scenery"
            | "cgl"
            | "font"
            | "fonts"
            | "objects"
            | "liveries"
            | "cockpit"
            | "cockpit_3d"
    )
}

fn is_model_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let base = lower.split('.').next().unwrap_or(&lower);
    matches!(base, "model" | "models")
}

enum Verdict {
    Yes,
    No,
    TooLarge,
}

fn whitelist_verdict(rel_path: &str, size: u64, in_model_dir: bool) -> Verdict {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    if in_model_dir && !name.to_ascii_lowercase().ends_with(".xml") {
        return Verdict::No;
    }
    if is_wasm_ext(name) {
        return if size <= MAX_WASM_FILE {
            Verdict::Yes
        } else {
            Verdict::TooLarge
        };
    }
    if is_text_ext(name) {
        return if size <= MAX_TEXT_FILE {
            Verdict::Yes
        } else {
            Verdict::TooLarge
        };
    }
    Verdict::No
}

/// Deterministischer Walk: sortierte Verzeichniseintraege, Whitelist,
/// Kappen. Wird von collect UND submit identisch benutzt (Transparenz).
fn collect_files(pkg_dir: &Path) -> Result<CollectResult, String> {
    let mut files: Vec<CollectedFile> = Vec::new();
    let mut skipped_large: Vec<String> = Vec::new();
    let mut total: u64 = 0;

    fn walk(
        dir: &Path,
        prefix: &str,
        in_model: bool,
        files: &mut Vec<CollectedFile>,
        skipped: &mut Vec<String>,
        total: &mut u64,
    ) -> Result<(), String> {
        if files.len() >= MAX_FILES || *total >= MAX_TOTAL {
            return Ok(());
        }
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("Ordner nicht lesbar: {} ({e})", dir.display()))?
            .filter_map(|e| e.ok())
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if files.len() >= MAX_FILES || *total >= MAX_TOTAL {
                return Ok(());
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if is_excluded_dir(&name) {
                    continue;
                }
                let child_in_model = in_model || is_model_dir(&name);
                walk(&entry.path(), &rel, child_in_model, files, skipped, total)?;
            } else if ft.is_file() {
                let Ok(meta) = entry.metadata() else { continue };
                match whitelist_verdict(&rel, meta.len(), in_model) {
                    Verdict::Yes => {
                        *total += meta.len();
                        files.push(CollectedFile {
                            path: rel,
                            size: meta.len(),
                        });
                    }
                    Verdict::TooLarge => skipped.push(rel),
                    Verdict::No => {}
                }
            }
        }
        Ok(())
    }

    walk(
        pkg_dir,
        "",
        false,
        &mut files,
        &mut skipped_large,
        &mut total,
    )?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(CollectResult {
        files,
        total_bytes: total,
        skipped_large,
    })
}

/// ZIP (deflate) aus der Collect-Auswahl bauen.
///
/// WICHTIG: KEIN `.large_file(true)` — das erzwingt ZIP64-Erweiterungsfelder
/// selbst fuer winzige Dateien, und der Server (`fflate`/Node) liest deren
/// klassisches 32-Bit-Groessenfeld dann als ZIP64-Sentinel `0xFFFFFFFF`
/// (~4 GB) statt die echte Groesse aus dem Extra-Feld zu holen — JEDE
/// Datei wird faelschlich als "zu gross" abgelehnt (Live-Befund Thomas K.,
/// 05.07.2026: reproduziert mit einer 6-KB-Testdatei → Server sah 4 294
/// 967 295 Bytes). Unsere Groessenkappen (MAX_TEXT_FILE 5 MB, MAX_WASM_FILE
/// 110 MB, MAX_TOTAL 400 MB) liegen alle weit unter der klassischen
/// ZIP32-Grenze (~4 GB) — ZIP64 wird hier nie gebraucht.
fn build_zip(pkg_dir: &Path, folder: &str, collected: &CollectResult) -> Result<Vec<u8>, String> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for f in &collected.files {
        let abs = pkg_dir.join(&f.path);
        let mut buf = Vec::with_capacity(f.size.min(16 * 1024 * 1024) as usize);
        std::fs::File::open(&abs)
            .and_then(|mut fh| fh.read_to_end(&mut buf))
            .map_err(|e| format!("Datei nicht lesbar: {} ({e})", f.path))?;
        // Server erwartet Pfade mit Paket-Ordner als Wurzel (wie Web-Upload)
        writer
            .start_file(format!("{folder}/{}", f.path), options)
            .and_then(|()| writer.write_all(&buf).map_err(zip::result::ZipError::Io))
            .map_err(|e| format!("ZIP-Fehler bei {}: {e}", f.path))?;
    }
    let cursor = writer
        .finish()
        .map_err(|e| format!("ZIP-Finish-Fehler: {e}"))?;
    Ok(cursor.into_inner())
}

// ─── Tauri-Commands ─────────────────────────────────────────────────────

/// Such-Wurzeln fuer `ascan_list_aircraft` bestimmen. Ein manuell
/// angegebener/gewaehlter Pfad ERSETZT die Auto-Erkennung, statt sie nur zu
/// ergaenzen — vorher wurde er einfach an die volle MSFS-Community- +
/// X-Plane-Aircraft-Suche ANGEHAENGT, sodass ein Klick auf "Flugzeuge
/// suchen" nach dem Waehlen eines einzelnen Ordners trotzdem die GESAMTE
/// Bibliothek erneut mitscannte, vermischt im selben Ergebnis (Live-Befund
/// Thomas K., 05.07.2026: "ich waehle einen Ordner, aber der scannt
/// trotzdem den ganzen Aircraft-Ordner — ich werde wahnsinnig"). Wer bewusst
/// einen Ordner waehlt, will GENAU DEN gescannt haben, nicht die ganze
/// Bibliothek obendrauf.
fn select_roots(manual_dir: Option<&str>) -> Result<Vec<(PathBuf, String)>, String> {
    if let Some(m) = manual_dir.map(str::trim).filter(|s| !s.is_empty()) {
        let p = PathBuf::from(m);
        if !p.is_dir() {
            return Err(format!("Ordner nicht gefunden: {m}"));
        }
        return Ok(vec![(p, "manuell gewaehlt".to_string())]);
    }
    Ok(community_dirs()
        .into_iter()
        .chain(xplane_aircraft_dirs())
        .map(|(p, l)| (p, l.to_string()))
        .collect())
}

/// Flugzeug-Pakete finden: MSFS-Community-Ordner (aus UserCfg.opt) +
/// X-Plane-Aircraft-Ordner (aus x-plane_install_*.txt) automatisch, ODER —
/// falls angegeben — ausschliesslich der manuelle Pfad (siehe
/// [`select_roots`]). Pro Paket wird nur die manifest.json (MSFS) bzw. der
/// .acf-Kopf (X-Plane) gelesen.
#[tauri::command]
pub async fn ascan_list_aircraft(
    state: tauri::State<'_, AircraftScanState>,
    manual_dir: Option<String>,
) -> Result<Vec<FoundAircraft>, String> {
    let roots = select_roots(manual_dir.as_deref())?;

    let (packages, found) = tauri::async_runtime::spawn_blocking(move || scan_roots(&roots))
        .await
        .map_err(|e| format!("Scan-Task abgebrochen: {e}"))?;

    *state
        .packages
        .lock()
        .map_err(|_| "state poisoned".to_string())? = packages;
    Ok(found)
}

/// Sortierte, direkte Unterordner von `dir` (best-effort — Lesefehler ergeben
/// eine leere Liste statt eines Absturzes).
fn sorted_subdirs(dir: &Path) -> Vec<std::fs::DirEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<_> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    dirs.sort_by_key(|e| e.file_name());
    dirs
}

/// Kern der Flugzeug-Suche — bewusst reine Funktion (kein Tauri-State/async),
/// damit sie mit echten Tempdir-Fixtures testbar ist statt nur "sieht richtig
/// aus". Prüft für jede Wurzel: die Wurzel selbst als Paket (manual_dir zeigt
/// direkt auf EIN Flugzeug), jeden direkten Unterordner als Paket (MSFS-
/// Community: flach), und — wenn ein Unterordner selbst KEIN Paket ist — eine
/// Ebene tiefer (X-Plane gruppiert Flugzeuge in Kategorie-Ordnern, z. B.
/// "Aircraft/Laminar Research/Cessna 172SP/*.acf",
/// "Aircraft/Extra Aircraft/Zibo B738/*.acf" — der Kategorie-Ordner selbst hat
/// keine .acf, das eigentliche Flugzeug sonst nie gefunden; Live-Befund
/// Thomas K., 05.07.2026: "mein X-Plane-Ordner wird nicht gefunden"). MSFS-
/// Community-Ordner sind flach — für sie ist der zweite Blick ein billiges,
/// folgenloses Read-Dir auf Ordner, die ohnehin schon kein Paket waren.
/// Der Paketordner zu einem Flugzeug-Titel, wie der Simulator ihn meldet.
///
/// # Wofür
///
/// Schritt 11 der v1.7.0-Bauliste liest die Spurweite aus der
/// Flugzeugdatei. Dazu muss der Ordner des **geflogenen** Flugzeugs bekannt
/// sein — der Simulator liefert aber nur den Titel (`TITLE` bei MSFS,
/// `acf/_name` bei X-Plane).
///
/// # Warum der Vergleich unscharf ist
///
/// Titel sind uneinheitlich geschrieben: „FenixA321 CFM SL SC" gegen
/// „Fenix A321 CFM", Bindestriche statt Leerzeichen, angehängte
/// Bemalungsnamen. Verglichen wird deshalb über eine normalisierte Form —
/// Grossbuchstaben, nur Buchstaben und Ziffern. Bleibt es mehrdeutig
/// (mehrere Pakete passen), gibt die Funktion **nichts** zurück: Ein
/// geratener Ordner liefert die Spurweite eines anderen Flugzeugs, und die
/// entscheidet mit darüber, ob ein Rad neben der Bahn war.
///
/// Der Scan geht über dieselben Wurzeln wie die Flugzeugliste und ist
/// entsprechend teuer — er gehört an den Flugbeginn, nicht in einen Tick.
pub fn paket_zu_titel(titel: &str) -> Option<PathBuf> {
    let gesucht = normalisiere_titel(titel);
    if gesucht.len() < 4 {
        return None;
    }
    let roots = select_roots(None).ok()?;
    let (_, gefunden) = scan_roots(&roots);

    // Erst exakt, dann als Präfix. Ein Präfix-Treffer deckt den häufigen
    // Fall ab, dass der Simulator den Titel um eine Bemalung erweitert.
    let mut treffer: Vec<&FoundAircraft> = gefunden
        .iter()
        .filter(|a| normalisiere_titel(&a.title) == gesucht)
        .collect();
    if treffer.is_empty() {
        treffer = gefunden
            .iter()
            .filter(|a| {
                let n = normalisiere_titel(&a.title);
                n.len() >= 6 && (gesucht.starts_with(&n) || n.starts_with(&gesucht))
            })
            .collect();
    }
    if treffer.len() != 1 {
        return None;
    }
    // `source_dir` ist für die Anzeige gekürzt; den echten Pfad hat nur
    // `scan_roots` in seinem Paket-Vektor. Erneut suchen ist billiger als
    // die Struktur umzubauen — der teure Teil war das Verzeichnis-Lesen.
    let index = treffer[0].index;
    let (pakete, _) = scan_roots(&roots);
    pakete.get(index).map(|p| p.dir.clone())
}

/// Titel auf eine vergleichbare Form bringen: Grossbuchstaben, nur
/// Buchstaben und Ziffern.
fn normalisiere_titel(t: &str) -> String {
    t.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

fn scan_roots(roots: &[(PathBuf, String)]) -> (Vec<ScanPackage>, Vec<FoundAircraft>) {
    let mut packages: Vec<ScanPackage> = Vec::new();
    let mut found: Vec<FoundAircraft> = Vec::new();
    for (root, label) in roots {
        for d in sorted_subdirs(root) {
            let pkg_dir = d.path();
            if let Some((title, creator)) = detect_aircraft(&pkg_dir) {
                let folder = d.file_name().to_string_lossy().to_string();
                found.push(FoundAircraft {
                    index: packages.len(),
                    folder: folder.clone(),
                    title,
                    creator,
                    source_dir: format!("{} — {}", label, root.display()),
                });
                packages.push(ScanPackage {
                    dir: pkg_dir,
                    folder,
                });
                continue;
            }
            for sd in sorted_subdirs(&pkg_dir) {
                let sub_pkg_dir = sd.path();
                if let Some((title, creator)) = detect_aircraft(&sub_pkg_dir) {
                    let folder = sd.file_name().to_string_lossy().to_string();
                    found.push(FoundAircraft {
                        index: packages.len(),
                        folder: folder.clone(),
                        title,
                        creator,
                        source_dir: format!(
                            "{} — {}/{}",
                            label,
                            root.display(),
                            d.file_name().to_string_lossy()
                        ),
                    });
                    packages.push(ScanPackage {
                        dir: sub_pkg_dir,
                        folder,
                    });
                }
            }
        }
        // Root selbst als Paket (manual_dir zeigt direkt auf EIN Flugzeug,
        // egal wo — MSFS-manifest.json ODER X-Plane-.acf).
        if let Some((title, creator)) = detect_aircraft(root) {
            let folder = root
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "package".to_string());
            found.push(FoundAircraft {
                index: packages.len(),
                folder: folder.clone(),
                title,
                creator,
                source_dir: format!("{} — {}", label, root.display()),
            });
            packages.push(ScanPackage {
                dir: root.clone(),
                folder,
            });
        }
    }
    found.sort_by_key(|a| a.title.to_lowercase());
    // Indizes nach Sortierung NICHT umschreiben — index zeigt in die
    // packages-Liste, nicht in die Anzeige-Reihenfolge.
    (packages, found)
}

/// Exakte Dateiliste des gewaehlten Pakets — wird dem Piloten VOR dem
/// Senden angezeigt (DSGVO-Transparenz).
#[tauri::command]
pub async fn ascan_collect(
    state: tauri::State<'_, AircraftScanState>,
    index: usize,
) -> Result<CollectResult, String> {
    let pkg = {
        let guard = state
            .packages
            .lock()
            .map_err(|_| "state poisoned".to_string())?;
        guard
            .get(index)
            .cloned()
            .ok_or("Unbekanntes Paket — bitte neu suchen")?
    };
    tauri::async_runtime::spawn_blocking(move || collect_files(&pkg.dir))
        .await
        .map_err(|e| format!("Collect-Task abgebrochen: {e}"))?
}

/// Paket zippen und an live.kant.ovh senden. Auth = phpVMS-API-Key aus dem
/// Keyring (derselbe Trust-Anker wie Provisioning). Sendet exakt die
/// Auswahl aus `collect_files` (deterministisch identisch zu ascan_collect).
#[tauri::command]
pub async fn ascan_submit(
    state: tauri::State<'_, AircraftScanState>,
    index: usize,
    endpoint: Option<String>,
) -> Result<SubmitResult, String> {
    let pkg = {
        let guard = state
            .packages
            .lock()
            .map_err(|_| "state poisoned".to_string())?;
        guard
            .get(index)
            .cloned()
            .ok_or("Unbekanntes Paket — bitte neu suchen")?
    };
    let api_key = crate::secrets_load_phpvms_key()
        .ok_or("Kein phpVMS-API-Key hinterlegt — bitte zuerst anmelden")?;

    let folder = pkg.folder.clone();
    let zip_bytes = tauri::async_runtime::spawn_blocking(move || {
        let collected = collect_files(&pkg.dir)?;
        if collected.files.is_empty() {
            return Err("Keine analysierbaren Dateien im Paket gefunden".to_string());
        }
        build_zip(&pkg.dir, &pkg.folder, &collected)
    })
    .await
    .map_err(|e| format!("Zip-Task abgebrochen: {e}"))??;

    let url = endpoint.unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());
    let zip_len = zip_bytes.len() as u64;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| format!("HTTP-Client-Fehler: {e}"))?;
    let resp = client
        .post(&url)
        .query(&[("source", "client"), ("package", folder.as_str())])
        .header("X-API-Key", api_key)
        .header("Content-Type", "application/zip")
        .body(zip_bytes)
        .send()
        .await
        .map_err(|e| format!("Upload fehlgeschlagen: {e}"))?;

    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Server-Antwort nicht lesbar: {e}"))?;
    if !status.is_success() {
        let msg = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("unbekannter Fehler");
        return Err(format!("Server lehnte ab ({status}): {msg}"));
    }

    let report = body.get("report");
    Ok(SubmitResult {
        ok: body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
        id: body.get("id").and_then(|v| v.as_str()).map(String::from),
        status: body
            .get("status")
            .and_then(|v| v.as_str())
            .map(String::from),
        zip_bytes: zip_len,
        icao: report
            .and_then(|r| r.pointer("/aircraft/icao_type"))
            .and_then(|v| v.as_str())
            .map(String::from),
        lvar_count: report
            .and_then(|r| r.pointer("/lvars/explicit_total"))
            .and_then(|v| v.as_i64()),
        external_process_suspected: report
            .and_then(|r| r.get("external_process_suspected"))
            .and_then(|v| v.as_bool()),
        warnings: report
            .and_then(|r| r.get("warnings"))
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|w| w.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_installed_packages_path() {
        let cfg = "Version 1\nInstalledPackagesPath \"D:\\MSFS Packages\"\nOther 2\n";
        assert_eq!(
            parse_installed_packages_path(cfg),
            Some(PathBuf::from("D:\\MSFS Packages"))
        );
        assert_eq!(parse_installed_packages_path("nix"), None);
    }

    #[test]
    fn whitelist_mirrors_server_rules() {
        assert!(matches!(
            whitelist_verdict("aircraft.cfg", 100, false),
            Verdict::Yes
        ));
        assert!(matches!(
            whitelist_verdict("panel/sys.wasm", 100, false),
            Verdict::Yes
        ));
        assert!(matches!(
            whitelist_verdict("panel/sys.wasm", MAX_WASM_FILE + 1, false),
            Verdict::TooLarge
        ));
        assert!(matches!(
            whitelist_verdict("texture/a.dds", 100, false),
            Verdict::No
        ));
        // model-Ordner: nur XML
        assert!(matches!(
            whitelist_verdict("model/behaviors.xml", 100, true),
            Verdict::Yes
        ));
        assert!(matches!(
            whitelist_verdict("model/a320.bin.json", 100, true),
            Verdict::No
        ));
    }

    #[test]
    fn excluded_dirs_match_web_scanner() {
        for d in [
            "texture",
            "TEXTURE.FLEET",
            "sound",
            "SoundAI",
            "effects",
            "cgl",
        ] {
            assert!(is_excluded_dir(d), "{d} muss ausgeschlossen sein");
        }
        // X-Plane-Gigabyte-Ordner ebenfalls ausschliessen.
        for d in ["objects", "liveries", "cockpit", "cockpit_3d"] {
            assert!(is_excluded_dir(d), "{d} (X-Plane) muss ausgeschlossen sein");
        }
        assert!(!is_excluded_dir("panel"));
        assert!(!is_excluded_dir("plugins"));
        assert!(is_model_dir("model.LVFR"));
    }

    #[test]
    fn xplane_whitelist_and_acf_detection() {
        // Whitelist: X-Plane-Dateitypen akzeptiert.
        assert!(matches!(
            whitelist_verdict("777.acf", 100, false),
            Verdict::Yes
        ));
        assert!(matches!(
            whitelist_verdict("modules/auto_thr.lua", 100, false),
            Verdict::Yes
        ));
        assert!(matches!(
            whitelist_verdict("plugins/sys/win.xpl", 100, false),
            Verdict::Yes
        ));
        assert!(matches!(
            whitelist_verdict("objects/skin.obj", 100, false),
            Verdict::No
        ));

        // .acf-Property-Parser.
        let acf = "I\n800 version\nP acf/_ICAO B77W\nP acf/_name Boeing 777-300ER\nP acf/_studio Stratosphere\n";
        assert_eq!(acf_prop(acf, "acf/_ICAO").as_deref(), Some("B77W"));
        assert_eq!(
            acf_prop(acf, "acf/_name").as_deref(),
            Some("Boeing 777-300ER")
        );
        assert_eq!(acf_prop(acf, "acf/_missing"), None);

        // read_xplane_acf erkennt einen Ordner mit .acf, liefert Name+Studio.
        let tmp = std::env::temp_dir().join(format!("ascan-xp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("myplane.acf"), acf).unwrap();
        let (name, studio) = read_xplane_acf(&tmp).unwrap();
        assert_eq!(name, "Boeing 777-300ER");
        assert_eq!(studio.as_deref(), Some("Stratosphere"));
        // detect_aircraft findet es auch (X-Plane-Zweig).
        assert!(detect_aircraft(&tmp).is_some());
        // Ordner ohne .acf/manifest.json → kein Flugzeug.
        let empty = tmp.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(detect_aircraft(&empty).is_none());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_roots_finds_xplane_aircraft_nested_in_category_folder() {
        // Reproduziert die reale X-Plane-Struktur (Live-Befund Thomas K.,
        // 05.07.2026): Flugzeuge liegen NICHT flach unter Aircraft/, sondern
        // in Kategorie-Ordnern — "Aircraft/Laminar Research/Cessna 172SP/*.acf",
        // "Aircraft/Extra Aircraft/Zibo B738/*.acf". Vor dem Fix fand die
        // Ein-Ebenen-Suche in diesem Fixture GAR NICHTS.
        let tmp = std::env::temp_dir().join(format!("ascan-xp-nested-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let cessna = tmp.join("Laminar Research/Cessna 172SP");
        let zibo = tmp.join("Extra Aircraft/Zibo B738");
        std::fs::create_dir_all(&cessna).unwrap();
        std::fs::create_dir_all(&zibo).unwrap();
        std::fs::write(
            cessna.join("Cessna_172SP.acf"),
            "I\n1100 version\nP acf/_name Cessna 172SP\n",
        )
        .unwrap();
        std::fs::write(
            zibo.join("B738.acf"),
            "I\n1100 version\nP acf/_name Zibo 737-800X\n",
        )
        .unwrap();

        let roots = vec![(tmp.clone(), "X-Plane (Aircraft-Ordner)".to_string())];
        let (packages, found) = scan_roots(&roots);

        assert_eq!(
            found.len(),
            2,
            "beide verschachtelten Flugzeuge muessen gefunden werden"
        );
        let titles: Vec<&str> = found.iter().map(|a| a.title.as_str()).collect();
        assert!(titles.contains(&"Cessna 172SP"));
        assert!(titles.contains(&"Zibo 737-800X"));
        assert_eq!(packages.len(), 2);
        // Die gespeicherten Paket-Pfade zeigen auf den ECHTEN Flugzeug-Ordner
        // (zwei Ebenen tief), nicht auf den Kategorie-Ordner.
        assert!(packages.iter().any(|p| p.dir == cessna));
        assert!(packages.iter().any(|p| p.dir == zibo));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn scan_roots_stays_flat_for_msfs_community_folder() {
        // Gegenprobe: ein flacher MSFS-Community-Ordner (Addon direkt unter
        // der Wurzel) darf durch die neue zweite Ebene NICHT doppelt gezaehlt
        // werden — das Addon wird auf Ebene 1 gefunden, Ebene 2 wird fuer
        // dieses Verzeichnis gar nicht erst versucht.
        let tmp = std::env::temp_dir().join(format!("ascan-msfs-flat-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let addon = tmp.join("some-addon");
        std::fs::create_dir_all(addon.join("SimObjects/Airplanes/X")).unwrap();
        std::fs::write(
            addon.join("manifest.json"),
            "{\"content_type\":\"AIRCRAFT\",\"title\":\"Some Addon\"}",
        )
        .unwrap();

        let roots = vec![(tmp.clone(), "MSFS Community".to_string())];
        let (packages, found) = scan_roots(&roots);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "Some Addon");
        assert_eq!(packages.len(), 1);
        assert_eq!(packages[0].dir, addon);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn select_roots_manual_dir_is_exclusive_not_additive() {
        // Live-Befund Thomas K. (05.07.2026): "ich waehle einen Ordner,
        // aber der scannt trotzdem den ganzen Aircraft-Ordner". Vorher
        // wurde ein manueller Pfad an die volle Auto-Erkennung ANGEHAENGT
        // statt sie zu ersetzen — mit einem manuellen Pfad muss GENAU EINE
        // Wurzel herauskommen, keine zusaetzlichen Auto-Erkennungs-Roots
        // (auf dieser Linux-Testumgebung waeren die zwar ohnehin leer, das
        // ist aber Zufall der Umgebung, nicht der Beweis — deshalb pruefen
        // wir die Laenge explizit statt nur "enthaelt X").
        let tmp =
            std::env::temp_dir().join(format!("ascan-manual-exclusive-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let roots = select_roots(Some(tmp.to_str().unwrap())).unwrap();
        assert_eq!(
            roots.len(),
            1,
            "manueller Pfad muss die EINZIGE Wurzel sein, nicht eine zusaetzliche"
        );
        assert_eq!(roots[0].0, tmp);
        assert_eq!(roots[0].1, "manuell gewaehlt");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn select_roots_rejects_missing_manual_dir() {
        let missing = std::env::temp_dir().join("ascan-does-not-exist-xyz");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(select_roots(Some(missing.to_str().unwrap())).is_err());
    }

    #[test]
    fn collect_and_zip_synthetic_package() {
        let tmp = std::env::temp_dir().join(format!("ascan-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("SimObjects/Airplanes/X/panel")).unwrap();
        std::fs::create_dir_all(tmp.join("texture")).unwrap();
        std::fs::write(
            tmp.join("manifest.json"),
            "{\"content_type\":\"AIRCRAFT\",\"title\":\"T\"}",
        )
        .unwrap();
        std::fs::write(
            tmp.join("SimObjects/Airplanes/X/aircraft.cfg"),
            "[GENERAL]\n",
        )
        .unwrap();
        std::fs::write(
            tmp.join("SimObjects/Airplanes/X/panel/sys.wasm"),
            vec![0u8; 128],
        )
        .unwrap();
        std::fs::write(tmp.join("texture/big.dds"), vec![0u8; 4096]).unwrap();

        let collected = collect_files(&tmp).unwrap();
        let paths: Vec<&str> = collected.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"manifest.json"));
        assert!(paths.contains(&"SimObjects/Airplanes/X/aircraft.cfg"));
        assert!(paths.contains(&"SimObjects/Airplanes/X/panel/sys.wasm"));
        assert!(
            !paths.iter().any(|p| p.ends_with(".dds")),
            "dds darf nie mit"
        );

        let zip_bytes = build_zip(&tmp, "pkg", &collected).unwrap();
        assert!(zip_bytes.len() > 100);
        // ZIP wieder oeffnen und Pfad-Prefix pruefen
        let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"pkg/manifest.json".to_string()));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn build_zip_does_not_emit_zip64_sentinel_size() {
        // Live-Befund Thomas K. (05.07.2026, A330-Einreichung): der Server
        // (Node/`fflate`) lehnte JEDE Datei als "zu gross" ab. Ursache:
        // `.large_file(true)` erzwang ZIP64-Erweiterungsfelder auch fuer
        // winzige Dateien; `fflate` liest dann das klassische 32-Bit-
        // Groessenfeld im Local-File-Header NICHT als "siehe ZIP64-Extra-
        // Feld"-Sentinel, sondern uebernimmt es woertlich — 0xFFFFFFFF
        // (~4 GB) statt der echten Groesse. Ein Rust-Rueckwaerts-Test mit
        // derselben `zip`-Crate haette das NICHT gefangen (sie parst ZIP64
        // korrekt) — deshalb hier direkt die rohen Bytes des Local-File-
        // Headers pruefen, wie es ein simplerer Reader wie fflate tut.
        let tmp = std::env::temp_dir().join(format!("ascan-zip64-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let content = b"tiny acf content, nowhere near 4 GB".repeat(50);
        std::fs::write(tmp.join("plane.acf"), &content).unwrap();

        let collected = collect_files(&tmp).unwrap();
        let zip_bytes = build_zip(&tmp, "pkg", &collected).unwrap();

        // Klassischer ZIP-Local-File-Header: 4 Byte Signatur "PK\x03\x04",
        // dann je 2 Byte version/flags/method/modtime/moddate (=10 Byte),
        // dann 4 Byte CRC32, dann 4 Byte compressed size, dann 4 Byte
        // uncompressed size — bei Offset 22.
        assert_eq!(
            &zip_bytes[0..4],
            b"PK\x03\x04",
            "muss mit Local-File-Header-Signatur beginnen"
        );
        let uncompressed_size = u32::from_le_bytes(zip_bytes[22..26].try_into().unwrap());
        assert_ne!(
            uncompressed_size, 0xFFFF_FFFF,
            "Local-File-Header darf NICHT den ZIP64-Sentinel tragen — fflate (Server) liest ihn woertlich als ~4 GB"
        );
        assert_eq!(uncompressed_size as usize, content.len());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn manifest_filter_only_aircraft() {
        let tmp = std::env::temp_dir().join(format!("ascan-mani-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("manifest.json"),
            "{\"content_type\":\"SCENERY\",\"title\":\"S\"}",
        )
        .unwrap();
        assert!(read_aircraft_manifest(&tmp).is_none());
        std::fs::write(
            tmp.join("manifest.json"),
            "{\"content_type\":\"AIRCRAFT\",\"title\":\"Fenix A320\",\"creator\":\"Fenix\"}",
        )
        .unwrap();
        let (title, creator) = read_aircraft_manifest(&tmp).unwrap();
        assert_eq!(title, "Fenix A320");
        assert_eq!(creator.as_deref(), Some("Fenix"));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
