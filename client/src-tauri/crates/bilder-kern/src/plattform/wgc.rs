//! Fensteraufnahme mit Windows.Graphics.Capture (WGC). Nimmt das Fenster selbst
//! auf, auch wenn es im Vollbild vorn steht oder AeroACARS im Hintergrund liegt;
//! es wird nichts vom Bildschirm kopiert, was andere Fenster darueber zeigen
//! (anders als `BitBlt`). Auf dem Mac nur mit `cargo check --target
//! x86_64-pc-windows-msvc` uebersetzt, nicht ausgefuehrt.

use std::time::{Duration, Instant};

use windows::core::{factory, Interface};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

use super::Aufnahme;

/// So lange wird auf den ersten Rahmen gewartet.
const ERSTER_RAHMEN: Duration = Duration::from_secs(2);

/// Gibt `RoInitialize` am Ende des Fadens wieder frei (nur, wenn es gelang).
struct Ro(bool);

impl Ro {
    fn neu() -> Self {
        // Fehlschlag (z. B. der Faden ist schon anders initialisiert) ist kein Abbruch:
        // dann gilt die vorhandene Initialisierung, und wir geben nichts frei.
        // SAFETY: gehoert zu diesem Faden; `Drop` ruft das Gegenstueck nur nach Erfolg.
        Self(unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.is_ok())
    }
}

impl Drop for Ro {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: Gegenstueck zu dem erfolgreichen `RoInitialize` aus `neu`.
            unsafe { RoUninitialize() };
        }
    }
}

/// Schliesst Sitzung und Rahmenpool in jedem Fall (auch bei einem Fehler dazwischen).
struct Offen {
    sitzung: Option<GraphicsCaptureSession>,
    pool: Option<Direct3D11CaptureFramePool>,
}

impl Drop for Offen {
    fn drop(&mut self) {
        if let Some(s) = self.sitzung.take() {
            let _ = s.Close();
        }
        if let Some(p) = self.pool.take() {
            let _ = p.Close();
        }
    }
}

fn fehler(was: &str, e: windows::core::Error) -> String {
    format!("{was}: {e}")
}

fn geraet_anlegen() -> Result<(ID3D11Device, ID3D11DeviceContext), String> {
    let mut geraet: Option<ID3D11Device> = None;
    let mut kontext: Option<ID3D11DeviceContext> = None;
    // SAFETY: Ausgabezeiger zeigen auf lokale Optionen, die danach geprueft werden.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut geraet),
            None,
            Some(&mut kontext),
        )
    }
    .map_err(|e| fehler("D3D11-Geraet", e))?;
    match (geraet, kontext) {
        (Some(g), Some(k)) => Ok((g, k)),
        _ => Err("D3D11-Geraet leer".into()),
    }
}

/// Nimmt das Fenster `hwnd` mit WGC auf. `Err(grund)` heisst: nicht verfuegbar
/// oder gescheitert (auch: ganz schwarz) — der Aufrufer faellt auf `PrintWindow`/`BitBlt` zurueck.
pub fn aufnehmen(hwnd: *mut std::ffi::c_void) -> Result<Aufnahme, String> {
    let _ro = Ro::neu();
    if !GraphicsCaptureSession::IsSupported().map_err(|e| fehler("IsSupported", e))? {
        return Err("Windows.Graphics.Capture wird hier nicht unterstuetzt".into());
    }
    let (geraet, kontext) = geraet_anlegen()?;
    let dxgi: IDXGIDevice = geraet.cast().map_err(|e| fehler("IDXGIDevice", e))?;
    // SAFETY: gueltiges DXGI-Geraet; das Ergebnis ist das WinRT-Gegenstueck.
    let winrt_geraet: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .and_then(|i| i.cast())
        .map_err(|e| fehler("IDirect3DDevice", e))?;

    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(|e| fehler("Interop", e))?;
    // SAFETY: `hwnd` stammt aus EnumWindows; ein inzwischen ungueltiges Handle liefert einen Fehler.
    let item: GraphicsCaptureItem = unsafe { interop.CreateForWindow(HWND(hwnd)) }.map_err(|e| fehler("CreateForWindow", e))?;
    let groesse = item.Size().map_err(|e| fehler("Item.Size", e))?;
    if groesse.Width <= 0 || groesse.Height <= 0 {
        return Err("Fenster hat keine Groesse".into());
    }

    let mut offen = Offen { sitzung: None, pool: None };
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&winrt_geraet, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, groesse)
        .map_err(|e| fehler("FramePool", e))?;
    offen.pool = Some(pool.clone());
    let sitzung = pool.CreateCaptureSession(&item).map_err(|e| fehler("CreateCaptureSession", e))?;
    offen.sitzung = Some(sitzung.clone());
    let _ = sitzung.SetIsCursorCaptureEnabled(false);
    let _ = sitzung.SetIsBorderRequired(false); // gibt es erst ab Windows 11; ein Fehler ist egal
    sitzung.StartCapture().map_err(|e| fehler("StartCapture", e))?;

    let ende = Instant::now() + ERSTER_RAHMEN;
    let rahmen = loop {
        match pool.TryGetNextFrame() {
            Ok(r) => break r,
            Err(_) if Instant::now() < ende => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return Err(format!("kein Rahmen innerhalb von {} s", ERSTER_RAHMEN.as_secs())),
        }
    };
    let inhalt = rahmen.ContentSize().map_err(|e| fehler("ContentSize", e))?;
    let flaeche = rahmen.Surface().map_err(|e| fehler("Surface", e))?;
    let zugriff: IDirect3DDxgiInterfaceAccess = flaeche.cast().map_err(|e| fehler("DxgiInterfaceAccess", e))?;
    // SAFETY: die Flaeche des Rahmens ist eine D3D11-Textur.
    let quelle: ID3D11Texture2D = unsafe { zugriff.GetInterface() }.map_err(|e| fehler("Textur", e))?;

    let mut beschr = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: Ausgabestruktur ist lokal.
    unsafe { quelle.GetDesc(&mut beschr) };
    beschr.Usage = D3D11_USAGE_STAGING;
    beschr.BindFlags = 0;
    beschr.MiscFlags = 0;
    beschr.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging: Option<ID3D11Texture2D> = None;
    // SAFETY: Beschreibung und Ausgabezeiger sind lokal.
    unsafe { geraet.CreateTexture2D(&beschr, None, Some(&mut staging)) }.map_err(|e| fehler("Staging-Textur", e))?;
    let staging = staging.ok_or("Staging-Textur leer")?;
    // SAFETY: beide Texturen gehoeren demselben Geraet und haben gleiche Masse und Format.
    unsafe { kontext.CopyResource(&staging, &quelle) };

    // Bildmass aus dem Rahmen (Inhalt), nie breiter als die Textur.
    let b = (inhalt.Width.max(0) as u32).min(beschr.Width);
    let h = (inhalt.Height.max(0) as u32).min(beschr.Height);
    if b == 0 || h == 0 {
        return Err("Rahmen ohne Inhalt".into());
    }
    let mut abbild = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: Staging-Textur mit CPU-Lesezugriff; Unmap folgt unten in jedem Fall.
    unsafe { kontext.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut abbild)) }.map_err(|e| fehler("Map", e))?;
    let mut bgra = vec![0u8; (b as usize) * (h as usize) * 4];
    let zeile = (b as usize) * 4;
    let pitch = abbild.RowPitch as usize;
    if abbild.pData.is_null() || pitch < zeile {
        // SAFETY: gerade erfolgreich gemappt.
        unsafe { kontext.Unmap(&staging, 0) };
        return Err("Map lieferte keine brauchbaren Zeilen".into());
    }
    for y in 0..h as usize {
        // SAFETY: Zeile y liegt innerhalb der gemappten Flaeche (h <= Texturhoehe, Zeilenabstand RowPitch >= zeile).
        let quellzeile = unsafe { std::slice::from_raw_parts((abbild.pData as *const u8).add(y * pitch), zeile) };
        bgra[y * zeile..(y + 1) * zeile].copy_from_slice(quellzeile);
    }
    // SAFETY: Gegenstueck zu Map.
    unsafe { kontext.Unmap(&staging, 0) };
    drop(rahmen);
    drop(offen);

    if super::windows::ganz_schwarz(&bgra) {
        return Err("ganz_schwarz".into());
    }
    Ok(Aufnahme { breite: b, hoehe: h, bgra, methode: "wgc" })
}
