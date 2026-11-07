//! Ersatz fuer Systeme ohne Fensteraufnahme (macOS, Linux).

use crate::suche::Fundort;
use crate::tastenkuerzel::Taste;
use crate::{plattform::Aufnahme, Fehler};

pub const AUFNAHME_VERFUEGBAR: bool = false;

/// Bildordner, die nur Windows kennt (Known Folder, Steam, NVIDIA): hier keine.
pub fn windows_fundorte() -> Vec<Fundort> {
    Vec::new()
}

/// Ob ein Simulatorfenster im Vordergrund steht: auf diesem System nicht ermittelbar.
pub fn sim_vordergrund() -> Option<bool> {
    None
}

/// Masse des Simulatorfensters: hier nicht ermittelbar.
pub fn sim_fenstermasse() -> Option<(u32, u32)> {
    None
}

pub fn fenster_aufnehmen() -> Result<Aufnahme, Fehler> {
    Err(Fehler::NichtVerfuegbar("Die Fensteraufnahme gibt es nur unter Windows".into()))
}

pub struct Hotkey;

pub fn hotkey_starten(_bei_taste: impl Fn() + Send + 'static) -> Result<Hotkey, Fehler> {
    Err(Fehler::NichtVerfuegbar("Das Tastenkuerzel gibt es nur unter Windows".into()))
}

impl Hotkey {
    pub fn setzen(&self, _taste: Option<&Taste>) -> Result<(), Fehler> {
        Err(Fehler::NichtVerfuegbar("Das Tastenkuerzel gibt es nur unter Windows".into()))
    }
}
