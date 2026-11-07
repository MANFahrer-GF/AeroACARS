//! Plattformteile. Unter Windows echt (Known Folder, Registry, Vordergrundfenster,
//! Fensteraufnahme, Hotkey); sonst Ersatz, der „nicht verfuegbar“ meldet oder
//! nichts findet — nie einen Fehler beim Aufspueren (P-11.30).

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod ersatz;
#[cfg(not(windows))]
pub use ersatz::*;

/// Ergebnis der Fensteraufnahme: BGRA, oben links beginnend.
#[derive(Debug, Clone)]
pub struct Aufnahme {
    pub breite: u32,
    pub hoehe: u32,
    pub bgra: Vec<u8>,
    /// Womit aufgenommen wurde: `printwindow` oder `bitblt`.
    pub methode: &'static str,
}
