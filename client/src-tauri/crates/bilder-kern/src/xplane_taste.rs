//! Die wirkliche Screenshot-Taste von X-Plane (P-11.30), nur lesend.
//!
//! X-Plane speichert die Belegung als Zeile `<Taste> <Modifikatoren> <Befehl>`,
//! z. B. `Space SHIFT sim/operation/screenshot`; Modifikatoren stehen mit `+`
//! verbunden (`SHIFT+CTRL`) oder als `<NONE>`. Gemessen 07.11.2026 an einer
//! echten Installation (X-Plane 12): dieselbe Zeile in
//! `Output/preferences/control profiles/User Profile - Keys.prf` (aktives
//! Steuerprofil) und in `Output/preferences/X-Plane Keys.prf` (allgemeine Datei).
//!
//! Das Profil gilt zuerst; die allgemeine Datei ist nur Rueckfall, wenn das
//! Profil fehlt oder keine lesbare Belegung des Befehls traegt. Fehlende Datei,
//! fremdes Format, unlesbare Zeile oder kein Treffer ergeben `None`, nie einen
//! Fehler und nie eine geratene Taste.

use std::path::Path;

const BEFEHL: &str = "sim/operation/screenshot";

/// Anzeigetext der Belegung(en), z. B. `Shift+Space`; mehrere Belegungen
/// stehen mit ` / ` getrennt. `None`, wenn nichts sicher lesbar ist.
pub fn screenshot_taste(install: &Path) -> Option<String> {
    let prefs = install.join("Output").join("preferences");
    [prefs.join("control profiles").join("User Profile - Keys.prf"), prefs.join("X-Plane Keys.prf")]
        .iter()
        .find_map(|datei| aus_datei(datei))
}

fn aus_datei(datei: &Path) -> Option<String> {
    let inhalt = std::fs::read_to_string(datei).ok()?;
    aus_text(&inhalt)
}

fn aus_text(inhalt: &str) -> Option<String> {
    let mut zeilen = inhalt.lines().map(str::trim);
    // Kopf: `I` oder `A`, dann `<Zahl> Version`.
    if !matches!(zeilen.next()?, "I" | "A") {
        return None;
    }
    let mut version = zeilen.next()?.split_whitespace();
    if version.next()?.parse::<u32>().is_err() || version.next()? != "Version" || version.next().is_some() {
        return None;
    }
    let mut treffer: Vec<String> = Vec::new();
    for z in zeilen {
        let teile: Vec<&str> = z.split_whitespace().collect();
        if teile.last() != Some(&BEFEHL) {
            continue;
        }
        if let Some(t) = belegung(&teile) {
            if !treffer.contains(&t) {
                treffer.push(t);
            }
        }
    }
    if treffer.is_empty() {
        None
    } else {
        Some(treffer.join(" / "))
    }
}

fn belegung(teile: &[&str]) -> Option<String> {
    let [taste, mods, _befehl] = teile else { return None };
    let (mut strg, mut opt, mut umschalt) = (false, false, false);
    if *mods != "<NONE>" {
        for m in mods.split('+') {
            match m {
                "CTRL" => strg = true,
                "OPTION" => opt = true,
                "SHIFT" => umschalt = true,
                _ => return None,
            }
        }
    }
    let mut teile_aus: Vec<&str> = Vec::new();
    if strg {
        teile_aus.push("Ctrl");
    }
    if opt {
        teile_aus.push("Option");
    }
    if umschalt {
        teile_aus.push("Shift");
    }
    teile_aus.push(taste);
    Some(teile_aus.join("+"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const KOPF: &str = "I\n1005 Version\n";

    fn install(profil: Option<&str>, allgemein: Option<&str>) -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let prefs = t.path().join("Output/preferences");
        fs::create_dir_all(prefs.join("control profiles")).unwrap();
        if let Some(p) = profil {
            fs::write(prefs.join("control profiles/User Profile - Keys.prf"), p).unwrap();
        }
        if let Some(a) = allgemein {
            fs::write(prefs.join("X-Plane Keys.prf"), a).unwrap();
        }
        t
    }

    fn datei(zeilen: &str) -> String {
        format!("{KOPF}{zeilen}")
    }

    #[test]
    fn echte_zeile_aus_der_messung() {
        let d = datei("Q CTRL sim/operation/quit\nSpace SHIFT sim/operation/screenshot\nEscape <NONE> sim/operation/show_menu\n");
        let t = install(Some(&d), Some(&d));
        assert_eq!(screenshot_taste(t.path()).as_deref(), Some("Shift+Space"));
    }

    #[test]
    fn profil_gilt_vor_der_allgemeinen_datei() {
        let t = install(
            Some(&datei("F12 <NONE> sim/operation/screenshot\n")),
            Some(&datei("Space SHIFT sim/operation/screenshot\n")),
        );
        assert_eq!(screenshot_taste(t.path()).as_deref(), Some("F12"));
    }

    #[test]
    fn allgemeine_datei_nur_als_rueckfall() {
        let allg = datei("Space SHIFT sim/operation/screenshot\n");
        let t = install(None, Some(&allg));
        assert_eq!(screenshot_taste(t.path()).as_deref(), Some("Shift+Space"));
        // Profil ohne den Befehl: ebenfalls Rueckfall
        let t = install(Some(&datei("Q CTRL sim/operation/quit\n")), Some(&allg));
        assert_eq!(screenshot_taste(t.path()).as_deref(), Some("Shift+Space"));
    }

    #[test]
    fn mehrere_belegungen_alle_genannt_modifikatoren_geordnet() {
        let d = datei(
            "P SHIFT+OPTION+CTRL sim/operation/screenshot\nF12 <NONE> sim/operation/screenshot\nF12 <NONE> sim/operation/screenshot\n",
        );
        let t = install(Some(&d), None);
        assert_eq!(screenshot_taste(t.path()).as_deref(), Some("Ctrl+Option+Shift+P / F12"));
    }

    #[test]
    fn crlf_wird_gelesen() {
        let d = "I\r\n1005 Version\r\nSpace SHIFT sim/operation/screenshot\r\n";
        let t = install(Some(d), None);
        assert_eq!(screenshot_taste(t.path()).as_deref(), Some("Shift+Space"));
    }

    #[test]
    fn nie_raten() {
        // kein X-Plane / keine Datei
        let t = tempfile::tempdir().unwrap();
        assert_eq!(screenshot_taste(t.path()), None);
        assert_eq!(screenshot_taste(&t.path().join("nix")), None);
        // fremdes Format
        for roh in ["", "irgendwas\nSpace SHIFT sim/operation/screenshot\n", "I\nVersion 1005\nSpace SHIFT sim/operation/screenshot\n"] {
            assert_eq!(aus_text(roh), None, "{roh:?}");
        }
        // kein Treffer
        assert_eq!(aus_text(&datei("Q CTRL sim/operation/quit\n")), None);
        // unlesbare Zeilen: unbekannter Modifikator, falsche Feldzahl, Befehl nur als Teilstring
        for z in [
            "Space HYPER sim/operation/screenshot",
            "Space sim/operation/screenshot",
            "Space SHIFT extra sim/operation/screenshot",
            "Space SHIFT sim/operation/screenshot_x",
            "Space SHIFT+ sim/operation/screenshot",
        ] {
            assert_eq!(aus_text(&datei(&format!("{z}\n"))), None, "{z}");
        }
    }

    #[test]
    fn unlesbare_zeile_stoert_lesbare_nicht() {
        let d = datei("Space HYPER sim/operation/screenshot\nF12 <NONE> sim/operation/screenshot\n");
        assert_eq!(aus_text(&d).as_deref(), Some("F12"));
    }
}
