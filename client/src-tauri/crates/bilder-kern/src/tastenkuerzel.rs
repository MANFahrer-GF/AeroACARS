//! Tastenkuerzel der Fensteraufnahme (`bild_aufnehmen`, Vorgabe Strg+Shift+S).
//!
//! Text wie „Ctrl+Shift+S“; „Strg“, „Umschalt“ und „Win“ werden verstanden.
//! Mindestens ein Modifikator ist Pflicht, sonst waere eine einzelne
//! Buchstabentaste im ganzen System belegt.

use crate::Fehler;

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taste {
    /// Bits wie bei `RegisterHotKey` (MOD_ALT ...).
    pub mods: u32,
    /// Windows-Tastencode (`A`-`Z` = 0x41.., `0`-`9` = 0x30.., `F1` = 0x70..).
    pub vk: u32,
    /// Einheitliche Schreibweise, z. B. `Ctrl+Shift+S`.
    pub text: String,
}

pub fn lesen(roh: &str) -> Result<Taste, Fehler> {
    let fehler = || Fehler::Eingabe(format!("Tastenkuerzel {roh:?} nicht lesbar (Beispiel: Ctrl+Shift+S)"));
    let mut mods = 0u32;
    let mut taste: Option<(u32, String)> = None;
    for teil in roh.split('+').map(str::trim) {
        if teil.is_empty() {
            return Err(fehler());
        }
        let klein = teil.to_ascii_lowercase();
        match klein.as_str() {
            "ctrl" | "strg" | "control" => mods |= MOD_CONTROL,
            "shift" | "umschalt" => mods |= MOD_SHIFT,
            "alt" => mods |= MOD_ALT,
            "win" | "super" | "meta" | "windows" => mods |= MOD_WIN,
            _ => {
                if taste.is_some() {
                    return Err(fehler());
                }
                let b = klein.as_bytes();
                let t = if b.len() == 1 && b[0].is_ascii_lowercase() {
                    (u32::from(b[0].to_ascii_uppercase()), teil.to_ascii_uppercase())
                } else if b.len() == 1 && b[0].is_ascii_digit() {
                    (u32::from(b[0]), teil.to_string())
                } else if let Some(n) = klein.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()).filter(|n| (1..=24).contains(n)) {
                    (0x70 + n - 1, format!("F{n}"))
                } else {
                    return Err(fehler());
                };
                taste = Some(t);
            }
        }
    }
    let (vk, name) = taste.ok_or_else(fehler)?;
    if mods == 0 {
        return Err(Fehler::Eingabe("Ein Tastenkuerzel braucht mindestens Strg, Alt, Umschalt oder Win".into()));
    }
    let mut text = String::new();
    for (bit, n) in [(MOD_CONTROL, "Ctrl"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")] {
        if mods & bit != 0 {
            text.push_str(n);
            text.push('+');
        }
    }
    text.push_str(&name);
    Ok(Taste { mods, vk, text })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorgabe_und_schreibweisen() {
        let t = lesen("Ctrl+Shift+S").unwrap();
        assert_eq!((t.mods, t.vk, t.text.as_str()), (MOD_CONTROL | MOD_SHIFT, 0x53, "Ctrl+Shift+S"));
        assert_eq!(lesen(" strg + umschalt + s ").unwrap(), t);
        assert_eq!(lesen("Shift+Ctrl+S").unwrap().text, "Ctrl+Shift+S");
        let f = lesen("Alt+F9").unwrap();
        assert_eq!((f.mods, f.vk, f.text.as_str()), (MOD_ALT, 0x78, "Alt+F9"));
        assert_eq!(lesen("Win+1").unwrap().vk, 0x31);
    }

    #[test]
    fn ohne_modifikator_oder_unlesbar_scheitert() {
        for schlecht in ["S", "F9", "", "Ctrl", "Ctrl+", "Ctrl+Shift", "Ctrl+S+D", "Ctrl+Ä", "Ctrl+F25", "Ctrl+F0", "Ctrl++S", "Ctrl+Space"] {
            assert!(lesen(schlecht).is_err(), "{schlecht:?}");
        }
    }
}
