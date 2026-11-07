//! Windows-Teile. Auf dem Mac nur mit `cargo check --target x86_64-pc-windows-msvc`
//! uebersetzt, nicht gelaufen (siehe Bericht).

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use windows_sys::core::GUID;
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HWND, LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};
use windows_sys::Win32::Storage::Xps::PrintWindow;
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY, RRF_RT_REG_SZ};
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey};
use windows_sys::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetForegroundWindow, GetMessageW, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, PeekMessageW, PostThreadMessageW, WindowFromPoint, GA_ROOT, GW_OWNER, MSG, PM_NOREMOVE, WM_APP, WM_HOTKEY,
};

use super::Aufnahme;
use crate::suche::{self, Fundort};
use crate::tastenkuerzel::Taste;
use crate::Fehler;

pub const AUFNAHME_VERFUEGBAR: bool = true;

/// Prozessnamen der Simulatoren (klein geschrieben).
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

fn ganz_schwarz(bgra: &[u8]) -> bool {
    bgra.chunks_exact(4).step_by(997).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0)
}

/// Nimmt das Simulatorfenster gezielt auf (nicht das Vordergrundfenster):
/// zuerst `PrintWindow` mit `PW_RENDERFULLCONTENT`; liefert das nur Schwarz
/// (manche DirectX-Fenster), eine Bildschirmkopie des Fensterbereichs — nur,
/// wenn das Simulatorfenster dort oben liegt, sonst Fehler statt fremder Inhalt.
pub fn fenster_aufnehmen() -> Result<Aufnahme, Fehler> {
    let (hwnd, r) = sim_fenster().ok_or_else(|| Fehler::NichtGefunden("Kein Simulatorfenster gefunden (FlightSimulator, X-Plane)".into()))?;
    let (b, h) = ((r.right - r.left) as i32, (r.bottom - r.top) as i32);
    // SAFETY: GDI-Objekte werden in jedem Fall wieder freigegeben.
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
        let alt = SelectObject(mem, bmp);
        let mut kopf: BITMAPINFO = std::mem::zeroed();
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
        let mut bgra = vec![0u8; (b as usize) * (h as usize) * 4];
        let mut methode = "printwindow";
        let mut ok = PrintWindow(hwnd, mem, PW_RENDERFULLCONTENT) != 0
            && GetDIBits(mem, bmp, 0, h as u32, bgra.as_mut_ptr() as *mut c_void, &mut kopf, DIB_RGB_COLORS) != 0
            && !ganz_schwarz(&bgra);
        let mut grund = String::new();
        if !ok {
            let mitte = windows_sys::Win32::Foundation::POINT { x: r.left + b / 2, y: r.top + h / 2 };
            let oben = WindowFromPoint(mitte);
            if !oben.is_null() && GetAncestor(oben, GA_ROOT) == hwnd {
                methode = "bitblt";
                ok = BitBlt(mem, 0, 0, b, h, schirm, r.left, r.top, SRCCOPY | CAPTUREBLT) != 0
                    && GetDIBits(mem, bmp, 0, h as u32, bgra.as_mut_ptr() as *mut c_void, &mut kopf, DIB_RGB_COLORS) != 0;
                if !ok {
                    grund = format!("Bildschirmkopie fehlgeschlagen (Fehler {})", GetLastError());
                }
            } else {
                grund = "Der Simulator liefert nur Schwarz und ist verdeckt; bitte nach vorn holen".into();
            }
        }
        SelectObject(mem, alt);
        DeleteObject(bmp);
        DeleteDC(mem);
        ReleaseDC(null_mut(), schirm);
        if !ok {
            return Err(Fehler::Io(if grund.is_empty() { "Aufnahme fehlgeschlagen".into() } else { grund }));
        }
        if ganz_schwarz(&bgra) {
            return Err(Fehler::Io("Die Aufnahme ist schwarz (Vollbild-Ausgabe des Simulators nicht abgreifbar)".into()));
        }
        Ok(Aufnahme { breite: b as u32, hoehe: h as u32, bgra, methode })
    }
}

/// Eigener Faden mit Nachrichtenschleife fuer `RegisterHotKey`.
pub struct Hotkey {
    faden: u32,
    auftrag: Arc<Mutex<Option<(Option<(u32, u32)>, mpsc::Sender<Result<(), String>>)>>>,
}

const HOTKEY_ID: i32 = 0x4141;
const MOD_NOREPEAT: u32 = 0x4000;

pub fn hotkey_starten(bei_taste: impl Fn() + Send + 'static) -> Result<Hotkey, Fehler> {
    let auftrag: Arc<Mutex<Option<(Option<(u32, u32)>, mpsc::Sender<Result<(), String>>)>>> = Arc::new(Mutex::new(None));
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
                let mut belegt = false;
                loop {
                    let r = GetMessageW(&mut msg, null_mut(), 0, 0);
                    if r <= 0 {
                        break;
                    }
                    if msg.message == WM_HOTKEY {
                        bei_taste();
                    } else if msg.message == WM_APP {
                        let Some((neu, antwort)) = a2.lock().ok().and_then(|mut g| g.take()) else { continue };
                        if belegt {
                            UnregisterHotKey(null_mut(), HOTKEY_ID);
                            belegt = false;
                        }
                        let erg = match neu {
                            None => Ok(()),
                            Some((mods, vk)) => {
                                if RegisterHotKey(null_mut(), HOTKEY_ID, mods | MOD_NOREPEAT, vk) != 0 {
                                    belegt = true;
                                    Ok(())
                                } else {
                                    Err(format!("Tastenkuerzel ist schon belegt (Windows-Fehler {})", GetLastError()))
                                }
                            }
                        };
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
        let (tx, rx) = mpsc::channel();
        *self.auftrag.lock().map_err(|_| Fehler::Io("Hotkey gesperrt".into()))? = Some((taste.map(|t| (t.mods, t.vk)), tx));
        // SAFETY: Faden-ID stammt aus dem Hotkey-Faden.
        if unsafe { PostThreadMessageW(self.faden, WM_APP, 0, 0) } == 0 {
            return Err(Fehler::Io("Hotkey-Faden nicht erreichbar".into()));
        }
        rx.recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| Fehler::Io("Hotkey-Faden antwortet nicht".into()))?
            .map_err(Fehler::Eingabe)
    }
}
