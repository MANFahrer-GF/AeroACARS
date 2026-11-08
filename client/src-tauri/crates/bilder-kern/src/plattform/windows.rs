//! Windows-Teile. Auf dem Mac nur mit `cargo check --target x86_64-pc-windows-msvc`
//! uebersetzt, nicht gelaufen (siehe Bericht).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Dwm::DwmGetWindowAttribute;
use windows_sys::Win32::Media::Multimedia::{
    joyGetDevCapsW, joyGetNumDevs, joyGetPosEx, JOYCAPSW, JOYERR_NOERROR, JOYINFOEX, JOY_RETURNBUTTONS,
};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows_sys::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_MEMORY, SND_NODEFAULT};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_SZ};
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetForegroundWindow, GetLayeredWindowAttributes, GetMessageW, GetWindow, GetWindowLongW, GetWindowRect,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, PeekMessageW, PostThreadMessageW, GWL_EXSTYLE, GW_HWNDPREV, GW_OWNER, LWA_ALPHA,
    MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY, WS_EX_LAYERED,
};

use super::Aufnahme;
use crate::suche::{self, Fundort};
use crate::tastenkuerzel::{Befehl, Belegung, Taste};
use crate::verdeckung::{verdeckt, Rechteck};
use crate::yoke::{self, Flanke, Geraet, SuchStand, YokeTaste};
use crate::Fehler;

pub const AUFNAHME_VERFUEGBAR: bool = true;

/// Prozessnamen der Simulatoren (klein geschrieben). Dieselben Namen wie
/// `sim-core::process_probe` (dort nicht oeffentlich, auch dort ungemessen).
const SIM_PROZESSE: [&str; 3] = ["flightsimulator.exe", "flightsimulator2024.exe", "x-plane.exe"];

/// `PW_RENDERFULLCONTENT` aus winuser.h (windows-sys kennt die Konstante nicht).
const PW_RENDERFULLCONTENT: u32 = 0x2;

/// `FOLDERID_Captures`: Ablage der Game Bar (`Videos\Captures`), laut Messung am
/// 07.11.2026 per API zu erfragen, weil der Registrywert leer ist.
const FOLDERID_CAPTURES: GUID = GUID { data1: 0xEDC0FE71, data2: 0x98D8, data3: 0x4F4A, data4: [0xB9, 0x20, 0xC8, 0xDC, 0x13, 0x3C, 0xB1, 0x65] };

fn wide_zu_string(p: *const u16) -> String {
    let mut n = 0usize;
    // SAFETY: Windows liefert NUL-terminierte Zeichenketten; wir lesen bis zur NUL.
    unsafe {
        while *p.add(n) != 0 {
            n += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, n))
    }
}

fn bekannter_ordner(id: &GUID) -> Option<PathBuf> {
    let mut pfad: *mut u16 = null_mut();
    // SAFETY: gueltige GUID, Zeiger auf Ausgabe; Speicher gibt CoTaskMemFree frei.
    unsafe {
        let hr = SHGetKnownFolderPath(id, 0, null_mut(), &mut pfad);
        if hr < 0 || pfad.is_null() {
            if !pfad.is_null() {
                CoTaskMemFree(pfad as *const c_void);
            }
            return None;
        }
        let s = wide_zu_string(pfad);
        CoTaskMemFree(pfad as *const c_void);
        (!s.is_empty()).then(|| PathBuf::from(s))
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Liest einen Wert aus `HKCU` (REG_SZ oder REG_BINARY mit UTF-16-Inhalt).
fn registry_text(unterschluessel: &str, wert: &str) -> Option<String> {
    let k = wide(unterschluessel);
    let w = wide(wert);
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_BINARY;
    let mut groesse: u32 = 0;
    // SAFETY: erst Groesse erfragen, dann mit passendem Puffer lesen.
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), w.as_ptr(), flags, null_mut(), null_mut(), &mut groesse) != 0 || groesse == 0 {
            return None;
        }
        let mut puffer = vec![0u8; groesse as usize];
        if RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), w.as_ptr(), flags, null_mut(), puffer.as_mut_ptr() as *mut c_void, &mut groesse) != 0 {
            return None;
        }
        puffer.truncate(groesse as usize);
        suche::utf16_bytes_zu_text(&puffer)
    }
}

fn lies_vdf(p: &Path) -> Vec<String> {
    std::fs::read_to_string(p).map(|t| suche::steam_pfade_aus_vdf(&t)).unwrap_or_default()
}

/// Bildordner, die der Rechner wirklich eingestellt hat (P-11.30 „Aufspueren unter Windows“):
/// Game Bar (Known Folder „Captures“), Bilder-Ordner samt OneDrive-Umleitung,
/// Steam (Ordner je Simulator-App-ID und der eingestellte Screenshot-Pfad),
/// NVIDIA (Stammordner aus der Registry, nur die Simulator-Unterordner).
pub fn windows_fundorte() -> Vec<Fundort> {
    let mut v: Vec<Fundort> = Vec::new();
    if let Some(p) = bekannter_ordner(&FOLDERID_CAPTURES) {
        v.push(suche::fundort(&p, "captures", None, "kopieren"));
    }
    if let Some(p) = bekannter_ordner(&FOLDERID_Pictures) {
        v.push(suche::fundort(&p, "bilder", None, "kopieren"));
    }
    if let Some(steam) = registry_text("Software\\Valve\\Steam", "SteamPath").map(|s| PathBuf::from(s.replace('/', "\\"))) {
        v.extend(suche::steam_sim_ordner(&steam));
        let mut dateien = vec![steam.join("config").join("config.vdf")];
        if let Ok(rd) = std::fs::read_dir(steam.join("userdata")) {
            dateien.extend(rd.flatten().map(|e| e.path().join("config").join("localconfig.vdf")));
        }
        for d in dateien {
            for p in lies_vdf(&d) {
                if !v.iter().any(|f| f.pfad == p) {
                    v.push(suche::fundort(Path::new(&p), "steam_extern", None, "kopieren"));
                }
            }
        }
    }
    if let Some(stamm) = registry_text("Software\\NVIDIA Corporation\\Global\\ShadowPlay\\NVSPCAPS", "DefaultPathW") {
        v.extend(suche::nvidia_fundorte(Path::new(&stamm)));
    }
    v
}

fn prozessname(pid: u32) -> Option<String> {
    // SAFETY: Handle wird geschlossen; Puffer hat die angegebene Laenge.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let mut puffer = [0u16; 1024];
        let mut len = puffer.len() as u32;
        let ok = QueryFullProcessImageNameW(h, 0, puffer.as_mut_ptr(), &mut len);
        CloseHandle(h);
        if ok == 0 {
            return None;
        }
        let voll = String::from_utf16_lossy(&puffer[..len as usize]);
        Some(voll.rsplit(['\\', '/']).next().unwrap_or(&voll).to_ascii_lowercase())
    }
}

fn ist_sim(hwnd: HWND) -> bool {
    let mut pid = 0u32;
    // SAFETY: hwnd darf ungueltig geworden sein; die Abfrage schlaegt dann fehl.
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    pid != 0 && prozessname(pid).is_some_and(|n| SIM_PROZESSE.contains(&n.as_str()))
}

/// Steht ein Simulatorfenster im Vordergrund?
pub fn sim_vordergrund() -> Option<bool> {
    // SAFETY: reine Abfrage.
    let h = unsafe { GetForegroundWindow() };
    if h.is_null() {
        return Some(false);
    }
    Some(ist_sim(h))
}

unsafe extern "system" fn sammeln(hwnd: HWND, lparam: LPARAM) -> i32 {
    // SAFETY: lparam ist der Zeiger auf den Vec aus `sim_fenster`.
    let liste = unsafe { &mut *(lparam as *mut Vec<(HWND, RECT)>) };
    // SAFETY: Fensterabfragen auf ein von EnumWindows geliefertes Handle.
    unsafe {
        if IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
            return 1;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(hwnd, &mut r) == 0 || r.right - r.left < 200 || r.bottom - r.top < 200 {
            return 1;
        }
        if ist_sim(hwnd) {
            liste.push((hwnd, r));
        }
    }
    1
}

/// Das groesste sichtbare Hauptfenster eines Simulators.
fn sim_fenster() -> Option<(HWND, RECT)> {
    let mut liste: Vec<(HWND, RECT)> = Vec::new();
    // SAFETY: Rueckruf nutzt den Zeiger nur waehrend des Aufrufs.
    unsafe { EnumWindows(Some(sammeln), &mut liste as *mut _ as LPARAM) };
    liste.into_iter().max_by_key(|(_, r)| i64::from(r.right - r.left) * i64::from(r.bottom - r.top))
}

/// Breite und Hoehe (Pixel) des Simulatorfensters, falls eines offen ist.
pub fn sim_fenstermasse() -> Option<(u32, u32)> {
    let (_, r) = sim_fenster()?;
    Some(((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32))
}

pub(super) fn ganz_schwarz(bgra: &[u8]) -> bool {
    bgra.chunks_exact(4).step_by(997).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0)
}

/// DWM-Attribut „weggeblendet“ (`DWMWA_CLOAKED`): Fenster auf einem anderen
/// virtuellen Desktop oder eine ruhende Store-App; in der Z-Reihenfolge, aber unsichtbar.
const DWMWA_CLOAKED: u32 = 14;

/// Zaehlt ein Fenster ueber dem Simulator als „verdeckend“? Nein, wenn es
/// unsichtbar, minimiert, weggeblendet, vollstaendig durchsichtig oder leer ist
/// oder zum Simulator selbst gehoert (dessen eigene Menues und Overlays).
fn verdeckt_moeglich(w: HWND, sim_pid: u32) -> Option<Rechteck> {
    // SAFETY: reine Abfragen auf ein von GetWindow geliefertes Handle.
    unsafe {
        if IsWindowVisible(w) == 0 || IsIconic(w) != 0 {
            return None;
        }
        let mut weg: u32 = 0;
        if DwmGetWindowAttribute(w, DWMWA_CLOAKED, &mut weg as *mut u32 as *mut c_void, std::mem::size_of::<u32>() as u32) == 0 && weg != 0 {
            return None;
        }
        if GetWindowLongW(w, GWL_EXSTYLE) as u32 & WS_EX_LAYERED != 0 {
            let (mut farbe, mut alpha, mut flags) = (0u32, 0u8, 0u32);
            if GetLayeredWindowAttributes(w, &mut farbe, &mut alpha, &mut flags) != 0 && flags & LWA_ALPHA != 0 && alpha == 0 {
                return None;
            }
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(w, &mut pid);
        if pid != 0 && pid == sim_pid {
            return None;
        }
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(w, &mut r) == 0 {
            return None;
        }
        let rr = Rechteck::neu(r.left, r.top, r.right, r.bottom);
        (!rr.leer()).then_some(rr)
    }
}

/// Alle Fenster, die in der Z-Reihenfolge ueber `hwnd` liegen und etwas verdecken koennten.
fn fenster_darueber(hwnd: HWND) -> Vec<Rechteck> {
    let mut sim_pid = 0u32;
    // SAFETY: Abfrage auf das Fenster des Simulators; schlaegt bei ungueltigem Handle fehl.
    unsafe { GetWindowThreadProcessId(hwnd, &mut sim_pid) };
    let mut v = Vec::new();
    // SAFETY: GW_HWNDPREV laeuft die Kette von hwnd nach oben (zum obersten Fenster).
    let mut w = unsafe { GetWindow(hwnd, GW_HWNDPREV) };
    let mut schritte = 0;
    while !w.is_null() && schritte < 10_000 {
        if let Some(r) = verdeckt_moeglich(w, sim_pid) {
            v.push(r);
        }
        // SAFETY: wie oben.
        w = unsafe { GetWindow(w, GW_HWNDPREV) };
        schritte += 1;
    }
    v
}

/// Die GDI-Objekte einer Aufnahme; `Drop` gibt in jedem Fall alles frei, und
/// zwar in der Reihenfolge, die GDI verlangt (Bitmap erst aus dem DC loesen).
struct Gdi {
    schirm: HDC,
    mem: HDC,
    bmp: HBITMAP,
    /// Das Objekt, das vor dem Auswaehlen unseres Bitmaps im DC war.
    alt: HGDIOBJ,
    gewaehlt: bool,
}

impl Gdi {
    fn neu(b: i32, h: i32) -> Result<Self, Fehler> {
        // SAFETY: jedes angelegte Objekt gehoert sofort `Gdi` bzw. wird hier freigegeben.
        unsafe {
            let schirm = GetDC(null_mut());
            if schirm.is_null() {
                return Err(Fehler::Io("Bildschirmkontext nicht verfuegbar".into()));
            }
            let mem = CreateCompatibleDC(schirm);
            let bmp = CreateCompatibleBitmap(schirm, b, h);
            if mem.is_null() || bmp.is_null() {
                if !bmp.is_null() {
                    DeleteObject(bmp);
                }
                if !mem.is_null() {
                    DeleteDC(mem);
                }
                ReleaseDC(null_mut(), schirm);
                return Err(Fehler::Io("Aufnahmepuffer nicht anlegbar".into()));
            }
            Ok(Self { schirm, mem, bmp, alt: null_mut(), gewaehlt: false })
        }
    }

    /// Bitmap in den Speicher-DC waehlen (zum Zeichnen: PrintWindow, BitBlt).
    fn waehlen(&mut self) {
        if !self.gewaehlt {
            // SAFETY: gueltige Handles aus `neu`.
            self.alt = unsafe { SelectObject(self.mem, self.bmp) };
            self.gewaehlt = true;
        }
    }

    /// Bitmap wieder aus dem DC loesen: Voraussetzung fuer `GetDIBits` und `DeleteObject`.
    fn loesen(&mut self) {
        if self.gewaehlt {
            // SAFETY: das gemerkte alte Objekt gehoert in diesen DC.
            unsafe { SelectObject(self.mem, self.alt) };
            self.gewaehlt = false;
        }
    }

    /// Pixel auslesen. Das Bitmap darf nicht in einem DC gewaehlt sein.
    fn auslesen(&mut self, b: i32, h: i32, bgra: &mut [u8]) -> bool {
        self.loesen();
        let mut kopf: BITMAPINFO = unsafe { std::mem::zeroed() };
        kopf.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: b,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        // SAFETY: `bgra` ist b*h*4 Bytes gross, das Bitmap ist nicht gewaehlt.
        unsafe { GetDIBits(self.mem, self.bmp, 0, h as u32, bgra.as_mut_ptr() as *mut c_void, &mut kopf, DIB_RGB_COLORS) == h }
    }
}

impl Drop for Gdi {
    fn drop(&mut self) {
        self.loesen();
        // SAFETY: Handles aus `neu`, Bitmap ist nicht mehr gewaehlt.
        unsafe {
            DeleteObject(self.bmp);
            DeleteDC(self.mem);
            ReleaseDC(null_mut(), self.schirm);
        }
    }
}

/// Nimmt das Simulatorfenster gezielt auf (nicht das Vordergrundfenster):
/// zuerst mit Windows.Graphics.Capture (`wgc`, nimmt das Fenster selbst auf, auch
/// im Vollbild und bei verdeckendem Fenster, ohne andere Fenster mitzunehmen).
/// Ist das nicht verfuegbar, scheitert es oder ist das Ergebnis ganz schwarz, folgt
/// der Rueckfall: `PrintWindow` mit `PW_RENDERFULLCONTENT`; liefert das nur
/// Schwarz (manche DirectX-Fenster), eine Bildschirmkopie des Fensterbereichs —
/// nur, wenn **kein sichtbares Fenster in der Z-Reihenfolge ueber dem Simulator
/// dessen Rechteck schneidet** (Fehler `verdeckt`), denn die Kopie nimmt alles
/// mit, was dort auf dem Bildschirm liegt. `verdeckt` gilt daher nur fuer diese
/// Bildschirmkopie, nie fuer `wgc`.
pub fn fenster_aufnehmen() -> Result<Aufnahme, Fehler> {
    static FADEN: AtomicBool = AtomicBool::new(false);
    super::mit_frist(super::AUFNAHME_FRIST, &FADEN, fenster_aufnehmen_roh)
}

fn fenster_aufnehmen_roh() -> Result<Aufnahme, Fehler> {
    let (hwnd, r) = sim_fenster().ok_or_else(|| Fehler::NichtGefunden("Kein Simulatorfenster gefunden (FlightSimulator, X-Plane)".into()))?;
    let wgc_grund = match super::wgc::aufnehmen(hwnd) {
        Ok(a) => return Ok(a),
        Err(grund) => grund,
    };
    // Rueckfall; scheitert auch er mit einem E/A-Fehler, steht der WGC-Grund dabei.
    match fenster_aufnehmen_gdi(hwnd, r) {
        Ok(a) => Ok(Aufnahme { rueckfall_grund: Some(wgc_grund), ..a }),
        Err(Fehler::Io(m)) => Err(Fehler::Io(format!("{m} (Windows.Graphics.Capture: {wgc_grund})"))),
        Err(Fehler::Verdeckt(m)) => Err(Fehler::Verdeckt(format!("{m} (Windows.Graphics.Capture: {wgc_grund})"))),
        Err(andere) => Err(andere),
    }
}

/// Rueckfall: `PrintWindow`, dann `BitBlt` (siehe `fenster_aufnehmen`).
fn fenster_aufnehmen_gdi(hwnd: HWND, r: RECT) -> Result<Aufnahme, Fehler> {
    let (b, h) = (r.right - r.left, r.bottom - r.top);
    let mut gdi = Gdi::neu(b, h)?;
    let mut bgra = vec![0u8; (b as usize) * (h as usize) * 4];
    let mut methode = "printwindow";
    gdi.waehlen();
    // SAFETY: gueltiges Fenster- und DC-Handle; das Bitmap ist gewaehlt.
    let gezeichnet = unsafe { PrintWindow(hwnd, gdi.mem, PW_RENDERFULLCONTENT) } != 0;
    // GetDIBits verlangt ein Bitmap, das in keinem DC gewaehlt ist: `auslesen` loest es zuerst.
    let mut ok = gezeichnet && gdi.auslesen(b, h, &mut bgra) && !ganz_schwarz(&bgra);
    if !ok {
        let sim = Rechteck::neu(r.left, r.top, r.right, r.bottom);
        if verdeckt(&sim, &fenster_darueber(hwnd)) {
            return Err(Fehler::Verdeckt("Der Simulator liefert nur Schwarz und ein anderes Fenster liegt darueber; bitte den Simulator nach vorn holen".into()));
        }
        methode = "bitblt";
        gdi.waehlen();
        // SAFETY: Quell-DC ist der Bildschirm, Ziel der Speicher-DC mit gewaehltem Bitmap.
        let kopiert = unsafe { BitBlt(gdi.mem, 0, 0, b, h, gdi.schirm, r.left, r.top, SRCCOPY | CAPTUREBLT) } != 0;
        let fehler_nr = if kopiert { 0 } else { unsafe { GetLastError() } };
        ok = kopiert && gdi.auslesen(b, h, &mut bgra);
        if !ok {
            return Err(Fehler::Io(format!("Bildschirmkopie fehlgeschlagen (Fehler {fehler_nr})")));
        }
    }
    if !ok {
        return Err(Fehler::Io("Aufnahme fehlgeschlagen".into()));
    }
    if ganz_schwarz(&bgra) {
        return Err(Fehler::Io("Die Aufnahme ist schwarz (Vollbild-Ausgabe des Simulators nicht abgreifbar)".into()));
    }
    Ok(Aufnahme { breite: b as u32, hoehe: h as u32, bgra, methode, rueckfall_grund: None })
}

/// Rueckmeldeton der Aufnahme: kurzes Klicken bei Erfolg, tiefer Ton bei Fehlschlag.
/// Asynchron aus dem Speicher (`SND_MEMORY|SND_ASYNC`); die Bytes liegen in einem
/// `OnceLock` und bleiben damit fuer die ganze Laufzeit gueltig, wie `ASYNC` es verlangt.
/// Hoerbar auch, wenn AeroACARS im Hintergrund ist (der Simulator vorn).
pub fn ton_abspielen(ok: bool) {
    static ERFOLG: OnceLock<Vec<u8>> = OnceLock::new();
    static FEHLSCHLAG: OnceLock<Vec<u8>> = OnceLock::new();
    let wav = if ok { ERFOLG.get_or_init(crate::ton::erfolg_wav) } else { FEHLSCHLAG.get_or_init(crate::ton::fehlschlag_wav) };
    // SAFETY: `wav` ist ein 'static-Puffer mit gueltigem WAV; bei SND_MEMORY ist der Zeiger der Puffer selbst.
    unsafe { PlaySoundW(wav.as_ptr() as *const u16, null_mut(), SND_MEMORY | SND_ASYNC | SND_NODEFAULT) };
}

/// Offener Auftrag an den Hotkey-Faden samt Antwortkanal.
type Auftrag = Mutex<Option<(Befehl, mpsc::Sender<Result<(), String>>)>>;

/// Eigener Faden mit Nachrichtenschleife fuer `RegisterHotKey`.
pub struct Hotkey {
    faden: u32,
    auftrag: Arc<Auftrag>,
}

const MOD_NOREPEAT: u32 = 0x4000;

pub fn hotkey_starten(bei_taste: impl Fn() + Send + 'static) -> Result<Hotkey, Fehler> {
    let auftrag: Arc<Auftrag> = Arc::new(Mutex::new(None));
    let a2 = Arc::clone(&auftrag);
    let (bereit_tx, bereit_rx) = mpsc::channel::<u32>();
    std::thread::Builder::new()
        .name("bild-hotkey".into())
        .spawn(move || {
            // SAFETY: Nachrichtenschleife im eigenen Faden; Hotkey gehoert diesem Faden.
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                PeekMessageW(&mut msg, null_mut(), WM_APP, WM_APP, PM_NOREMOVE); // Nachrichtenwarteschlange anlegen
                let _ = bereit_tx.send(GetCurrentThreadId());
                let mut belegung = Belegung::default();
                loop {
                    let r = GetMessageW(&mut msg, null_mut(), 0, 0);
                    if r <= 0 {
                        break;
                    }
                    if msg.message == WM_HOTKEY {
                        bei_taste();
                    } else if msg.message == WM_APP {
                        let Some((befehl, antwort)) = a2.lock().ok().and_then(|mut g| g.take()) else { continue };
                        let erg = belegung
                            .ausfuehren(
                                befehl,
                                |id, mods, vk| if RegisterHotKey(null_mut(), id, mods | MOD_NOREPEAT, vk) != 0 { Ok(()) } else { Err(GetLastError()) },
                                |id| {
                                    UnregisterHotKey(null_mut(), id);
                                },
                            )
                            .map_err(|code| format!("Tastenkuerzel ist schon belegt (Windows-Fehler {code})"));
                        let _ = antwort.send(erg);
                    }
                }
            }
        })
        .map_err(|e| Fehler::Io(e.to_string()))?;
    let faden = bereit_rx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| Fehler::Io("Hotkey-Faden startet nicht".into()))?;
    Ok(Hotkey { faden, auftrag })
}

impl Hotkey {
    /// Meldet das Tastenkuerzel an (ersetzt das bisherige); `None` gibt es frei.
    pub fn setzen(&self, taste: Option<&Taste>) -> Result<(), Fehler> {
        self.auftragen(Befehl::Setzen(taste.map(|t| (t.mods, t.vk))))
    }

    /// Meldet die neue Kombi zusaetzlich an; die alte gilt bis [`Hotkey::bestaetigen`].
    pub fn vorbereiten(&self, taste: &Taste) -> Result<(), Fehler> {
        let erg = self.auftragen(Befehl::Vorbereiten((taste.mods, taste.vk)));
        if matches!(erg, Err(Fehler::Io(_))) {
            // Keine Antwort: der Versuch darf nicht spaeter unbemerkt wirksam werden.
            let _ = self.absenden(Befehl::Verwerfen);
        }
        erg
    }

    pub fn bestaetigen(&self) -> Result<(), Fehler> {
        self.auftragen(Befehl::Bestaetigen)
    }

    pub fn verwerfen(&self) -> Result<(), Fehler> {
        self.auftragen(Befehl::Verwerfen)
    }

    fn auftragen(&self, befehl: Befehl) -> Result<(), Fehler> {
        self.absenden(befehl)?
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| Fehler::Io("Hotkey-Faden antwortet nicht".into()))?
            .map_err(Fehler::Eingabe)
    }

    fn absenden(&self, befehl: Befehl) -> Result<mpsc::Receiver<Result<(), String>>, Fehler> {
        let (tx, rx) = mpsc::channel();
        *self.auftrag.lock().map_err(|_| Fehler::Io("Hotkey gesperrt".into()))? = Some((befehl, tx));
        // SAFETY: Faden-ID stammt aus dem Hotkey-Faden.
        if unsafe { PostThreadMessageW(self.faden, WM_APP, 0, 0) } == 0 {
            return Err(Fehler::Io("Hotkey-Faden nicht erreichbar".into()));
        }
        Ok(rx)
    }
}

// ------------------------------------------------------------------ Yoke

pub const YOKE_VERFUEGBAR: bool = true;

/// Hoechstzahl der Joystick-Nummern, die `winmm` kennt.
const MAX_JOYSTICKS: u32 = 16;

/// Knopfmaske eines Geraets (Bit 0 = Knopf 1); `None`, wenn es nicht lesbar ist
/// (abgesteckt). `winmm` liefert hoechstens 32 Knoepfe.
fn joy_lesen(nummer: u32) -> Option<u32> {
    // SAFETY: Ausgabestruktur ist null-initialisiert und `dwSize` gesetzt; nur lesen.
    unsafe {
        let mut info: JOYINFOEX = std::mem::zeroed();
        info.dwSize = std::mem::size_of::<JOYINFOEX>() as u32;
        info.dwFlags = JOY_RETURNBUTTONS as u32;
        if joyGetPosEx(nummer, &mut info) != JOYERR_NOERROR {
            return None;
        }
        Some(info.dwButtons)
    }
}

/// Alle jetzt angeschlossenen und lesbaren Geraete.
fn joy_geraete() -> Vec<Geraet> {
    let mut v = Vec::new();
    // SAFETY: `joyGetDevCapsW` schreibt in eine null-initialisierte Struktur der angegebenen Groesse.
    let n = unsafe { joyGetNumDevs() }.min(MAX_JOYSTICKS);
    for i in 0..n {
        if joy_lesen(i).is_none() {
            continue;
        }
        // SAFETY: wie oben; gepackte Felder werden nur kopiert, nie referenziert.
        let (mid, pid, roh) = unsafe {
            let mut caps: JOYCAPSW = std::mem::zeroed();
            if joyGetDevCapsW(i as usize, &mut caps, std::mem::size_of::<JOYCAPSW>() as u32) != JOYERR_NOERROR {
                continue;
            }
            (caps.wMid, caps.wPid, caps.szPname)
        };
        let laenge = roh.iter().position(|&c| c == 0).unwrap_or(roh.len());
        let oem = registry_text(&yoke::oem_schluessel(mid, pid), "OEMName");
        let name = yoke::name_waehlen(oem.as_deref(), &String::from_utf16_lossy(&roh[..laenge]));
        v.push(Geraet { nummer: i, name, hersteller_id: mid, produkt_id: pid });
    }
    v
}

/// Wartet hoechstens `max` auf einen neu gedrueckten Knopf an irgendeinem Geraet
/// (Knoepfe, die beim Start schon gehalten werden, zaehlen erst nach dem Loslassen).
/// `Ok(None)`: nichts gedrueckt oder kein Geraet angeschlossen.
pub fn yoke_lernen(max: std::time::Duration) -> Result<Option<YokeTaste>, Fehler> {
    let geraete = joy_geraete();
    let mut vorher: Vec<u32> = geraete.iter().map(|g| joy_lesen(g.nummer).unwrap_or(u32::MAX)).collect();
    let ende = std::time::Instant::now() + max;
    while !geraete.is_empty() && std::time::Instant::now() < ende {
        std::thread::sleep(std::time::Duration::from_millis(yoke::TAKT_MS));
        for (i, g) in geraete.iter().enumerate() {
            let Some(maske) = joy_lesen(g.nummer) else { continue };
            if let Some(knopf) = yoke::neuer_knopf(vorher[i], maske) {
                return Ok(Some(YokeTaste { geraet_name: g.name.clone(), hersteller_id: g.hersteller_id, produkt_id: g.produkt_id, knopf }));
            }
            vorher[i] = maske;
        }
    }
    Ok(None)
}

type YokeZustand = (Mutex<Option<YokeTaste>>, Condvar);

/// Eigener Faden, der nur abfragt (20 Hz), solange ein Knopf belegt ist.
pub struct Yoke {
    zustand: Arc<YokeZustand>,
}

pub fn yoke_starten(bei_taste: impl Fn() + Send + 'static) -> Result<Yoke, Fehler> {
    let zustand: Arc<YokeZustand> = Arc::new((Mutex::new(None), Condvar::new()));
    let z = Arc::clone(&zustand);
    std::thread::Builder::new()
        .name("bild-yoke".into())
        .spawn(move || {
            let start = std::time::Instant::now();
            let mut gemerkt: Option<YokeTaste> = None;
            let mut flanke = Flanke::default();
            let mut nummer: Option<u32> = None;
            let mut zuletzt_gesucht: Option<u64> = None;
            let mut suche = SuchStand::default();
            loop {
                let taste = {
                    let (m, cv) = &*z;
                    let mut g = m.lock().unwrap_or_else(|e| e.into_inner());
                    while g.is_none() {
                        g = cv.wait(g).unwrap_or_else(|e| e.into_inner());
                    }
                    g.clone()
                };
                let Some(taste) = taste else { continue };
                let jetzt = start.elapsed().as_millis() as u64;
                if gemerkt.as_ref() != Some(&taste) {
                    gemerkt = Some(taste.clone());
                    flanke = Flanke::default();
                    nummer = None;
                    zuletzt_gesucht = None;
                    suche = SuchStand::default();
                }
                // Fehlt das Geraet, einmal je Sekunde ueber Hersteller, Produkt und Name
                // suchen (auch unter neuer Nummer); `winmm` fragt dabei alle Nummern ab, also
                // nicht, solange das Geraet lesbar ist.
                if nummer.is_none() && zuletzt_gesucht.is_none_or(|t| jetzt.saturating_sub(t) >= 1000) {
                    zuletzt_gesucht = Some(jetzt);
                    let geraete = joy_geraete();
                    nummer = yoke::geraet_finden(&geraete, &taste);
                    if suche.geaendert(nummer) {
                        tracing::info!("{}", yoke::suchbericht(&taste, nummer, &geraete));
                    }
                }
                let gedrueckt = match nummer {
                    Some(n) => match joy_lesen(n) {
                        Some(maske) => Some(yoke::knopf_gedrueckt(maske, taste.knopf)),
                        None => {
                            nummer = None; // abgesteckt: nichts blockieren, bei der naechsten Suche wiederfinden
                            None
                        }
                    },
                    None => None,
                };
                if flanke.pruefen(jetzt, gedrueckt) {
                    bei_taste();
                }
                std::thread::sleep(std::time::Duration::from_millis(yoke::TAKT_MS));
            }
        })
        .map_err(|e| Fehler::Io(e.to_string()))?;
    Ok(Yoke { zustand })
}

impl Yoke {
    /// Merkt den Knopf (ersetzt den bisherigen); `None` schaltet die Abfrage ab.
    pub fn setzen(&self, taste: Option<YokeTaste>) {
        let (m, cv) = &*self.zustand;
        *m.lock().unwrap_or_else(|e| e.into_inner()) = taste;
        cv.notify_all();
    }
}
