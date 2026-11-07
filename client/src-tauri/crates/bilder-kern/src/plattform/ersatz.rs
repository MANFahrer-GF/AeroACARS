//! Ersatz fuer Systeme ohne Fensteraufnahme (macOS, Linux).

use crate::suche::Fundort;
use crate::tastenkuerzel::Taste;
use crate::yoke::YokeTaste;
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

/// Rueckmeldeton der Aufnahme: hier ohne Wirkung.
pub fn ton_abspielen(_ok: bool) {}

pub struct Hotkey;

pub fn hotkey_starten(_bei_taste: impl Fn() + Send + 'static) -> Result<Hotkey, Fehler> {
    Err(Fehler::NichtVerfuegbar("Das Tastenkuerzel gibt es nur unter Windows".into()))
}

impl Hotkey {
    pub fn setzen(&self, _taste: Option<&Taste>) -> Result<(), Fehler> {
        Err(Fehler::NichtVerfuegbar("Das Tastenkuerzel gibt es nur unter Windows".into()))
    }
}

pub const YOKE_VERFUEGBAR: bool = false;

pub fn yoke_lernen(_max: std::time::Duration) -> Result<Option<YokeTaste>, Fehler> {
    Err(Fehler::NichtVerfuegbar("Der Yoke-Knopf als Ausloeser gehoert nur zu Windows".into()))
}

pub struct Yoke;

pub fn yoke_starten(_bei_taste: impl Fn() + Send + 'static) -> Result<Yoke, Fehler> {
    Err(Fehler::NichtVerfuegbar("Der Yoke-Knopf als Ausloeser gehoert nur zu Windows".into()))
}

impl Yoke {
    pub fn setzen(&self, _taste: Option<YokeTaste>) {}
}
