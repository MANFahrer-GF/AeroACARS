//! Aufspueren von Bildquellen (`bild_quellen_finden`, P-11.30). Hier steht der
//! reine Teil (Dateisystem und Text); die Windows-Abfragen (Known Folder,
//! Registry) liefern ihre Rohwerte aus [`crate::plattform`] zu.
//!
//! Gemessen ist nur, was Thomas am 07.11.2026 auf seinem Windows-PC sah
//! (Game Bar: Known Folder „Captures“; NVIDIA: `NVSPCAPS\DefaultPathW`, ein
//! Unterordner je Spiel). Steam-Ordner und der Steam-Screenshot-Pfad sind
//! nicht gemessen; der Pfadschluessel wird deshalb ueber einen Namensvergleich
//! gelesen („screenshot“ und „path“ im Schluessel).

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// Ein Vorschlag fuer eine Bildquelle. Der Pilot bestaetigt, das Paket setzt
/// ueber `bild_quellen_setzen`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Fundort {
    pub pfad: String,
    /// `xplane_standard`, `captures`, `bilder`, `steam`, `steam_extern`, `nvidia`
    pub art: String,
    /// `msfs`, `xplane` oder leer, wenn der Ordner dem Simulator nicht zuzuordnen ist.
    pub simulator: Option<String>,
    pub existiert: bool,
    /// X-Plane-Standard wird verschoben; Ordner mit privaten Bildern nur kopiert (P-11.30).
    pub vorschlag_modus: String,
}

pub fn fundort(pfad: &Path, art: &str, simulator: Option<&str>, modus: &str) -> Fundort {
    Fundort {
        existiert: pfad.is_dir(),
        pfad: pfad.to_string_lossy().into_owned(),
        art: art.to_string(),
        simulator: simulator.map(str::to_string),
        vorschlag_modus: modus.to_string(),
    }
}

/// `<X-Plane>/Output/screenshots` (gemessen an X-Plane 12; der Ordner entsteht beim ersten Bild).
pub fn xplane_screenshots(install: &Path) -> PathBuf {
    install.join("Output").join("screenshots")
}

pub fn xplane_fundorte(installationen: &[PathBuf]) -> Vec<Fundort> {
    let mut v: Vec<Fundort> = Vec::new();
    for i in installationen {
        let p = xplane_screenshots(i);
        if !v.iter().any(|f| f.pfad == p.to_string_lossy()) {
            v.push(fundort(&p, "xplane_standard", Some("xplane"), "verschieben"));
        }
    }
    v
}

/// Steam-App-IDs der Simulatoren (gegen store.steampowered.com am 07.11.2026 geprueft).
pub const STEAM_APPS: [(&str, &str); 4] = [
    ("1250410", "msfs"),    // Microsoft Flight Simulator (2020)
    ("2537590", "msfs"),    // Microsoft Flight Simulator 2024
    ("2014780", "xplane"),  // X-Plane 12
    ("269950", "xplane"),   // X-Plane 11
];

/// `<Steam>/userdata/<konto>/760/remote/<appid>/screenshots` fuer die Simulatoren, soweit vorhanden.
pub fn steam_sim_ordner(steam: &Path) -> Vec<Fundort> {
    let mut v = Vec::new();
    let Ok(rd) = fs::read_dir(steam.join("userdata")) else { return v };
    let mut konten: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    konten.sort();
    for konto in konten {
        for (app, sim) in STEAM_APPS {
            let p = konto.join("760").join("remote").join(app).join("screenshots");
            if p.is_dir() {
                v.push(fundort(&p, "steam", Some(sim), "kopieren"));
            }
        }
    }
    v
}

/// Zeilen `"Schluessel"  "Wert"` aus einer Steam-VDF-Datei: Werte, deren
/// Schluessel „screenshot“ und „path“ enthaelt (Gross-/Kleinschreibung egal).
pub fn steam_pfade_aus_vdf(text: &str) -> Vec<String> {
    let mut aus: Vec<String> = Vec::new();
    for zeile in text.lines() {
        let mut tokens: Vec<String> = Vec::new();
        let mut cur: Option<String> = None;
        let mut escape = false;
        for c in zeile.chars() {
            match (&mut cur, c) {
                (None, '"') => cur = Some(String::new()),
                (Some(_), _) if escape => {
                    let s = cur.as_mut().unwrap();
                    s.push(match c {
                        'n' => '\n',
                        't' => '\t',
                        o => o,
                    });
                    escape = false;
                }
                (Some(_), '\\') => escape = true,
                (Some(_), '"') => tokens.push(cur.take().unwrap()),
                (Some(s), o) => s.push(o),
                (None, _) => {}
            }
        }
        if let [schluessel, wert] = tokens.as_slice() {
            let k = schluessel.to_ascii_lowercase();
            if k.contains("screenshot") && k.contains("path") && !wert.trim().is_empty() && !aus.contains(wert) {
                aus.push(wert.clone());
            }
        }
    }
    aus
}

fn sim_aus_name(name: &str) -> Option<&'static str> {
    let n = name.to_ascii_lowercase();
    if n.contains("flight simulator") {
        Some("msfs")
    } else if n.contains("x-plane") {
        Some("xplane")
    } else {
        None
    }
}

/// NVIDIA legt je Spiel einen Unterordner im Stammordner an; nur die der
/// Simulatoren werden vorgeschlagen, nicht der ganze Stamm mit anderen Spielen.
pub fn nvidia_fundorte(stamm: &Path) -> Vec<Fundort> {
    let Ok(rd) = fs::read_dir(stamm) else { return Vec::new() };
    let mut v: Vec<Fundort> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            Some(fundort(&e.path(), "nvidia", Some(sim_aus_name(&name)?), "kopieren"))
        })
        .collect();
    v.sort_by(|a, b| a.pfad.cmp(&b.pfad));
    v
}

/// Registrywert: `REG_SZ` (UTF-16, NUL-terminiert) oder `REG_BINARY` mit demselben Inhalt.
pub fn utf16_bytes_zu_text(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 2 {
        return None;
    }
    let u: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
    let ende = u.iter().position(|&c| c == 0).unwrap_or(u.len());
    let s = String::from_utf16(&u[..ende]).ok()?;
    let s = s.trim().to_string();
    (!s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xplane_ordner_auch_wenn_er_noch_nicht_existiert() {
        let t = tempfile::tempdir().unwrap();
        let v = xplane_fundorte(&[t.path().to_path_buf(), t.path().to_path_buf()]);
        assert_eq!(v.len(), 1, "doppelte Installation nur einmal");
        assert!(v[0].pfad.ends_with("screenshots") && !v[0].existiert);
        assert_eq!((v[0].art.as_str(), v[0].vorschlag_modus.as_str(), v[0].simulator.as_deref()), ("xplane_standard", "verschieben", Some("xplane")));
        fs::create_dir_all(xplane_screenshots(t.path())).unwrap();
        assert!(xplane_fundorte(&[t.path().to_path_buf()])[0].existiert);
        assert!(xplane_fundorte(&[]).is_empty());
    }

    #[test]
    fn steam_ordner_nur_der_simulatoren() {
        let t = tempfile::tempdir().unwrap();
        for app in ["2537590", "2014780", "570"] {
            fs::create_dir_all(t.path().join("userdata/123/760/remote").join(app).join("screenshots")).unwrap();
        }
        let v = steam_sim_ordner(t.path());
        let sims: Vec<_> = v.iter().map(|f| f.simulator.as_deref().unwrap()).collect();
        assert_eq!(sims, ["msfs", "xplane"], "Dota (570) gehoert nicht dazu");
        assert!(v.iter().all(|f| f.existiert && f.vorschlag_modus == "kopieren"));
        assert!(steam_sim_ordner(&t.path().join("nix")).is_empty());
    }

    #[test]
    fn steam_vdf_pfad_schluessel() {
        let vdf = "\"UserLocalConfigStore\"\n{\n\t\"system\"\n\t{\n\t\t\"InGameOverlayScreenshotSaveUncompressedPath\"\t\t\"D:\\\\Shots\\\\Steam\"\n\t\t\"EnableGameOverlay\"\t\t\"1\"\n\t\t\"ScreenshotPath\"\t\t\"E:\\\\Bilder\"\n\t\t\"ScreenshotPathLeer\"\t\t\" \"\n\t}\n}\n";
        assert_eq!(steam_pfade_aus_vdf(vdf), ["D:\\Shots\\Steam", "E:\\Bilder"]);
        assert!(steam_pfade_aus_vdf("").is_empty());
    }

    #[test]
    fn nvidia_nur_simulator_unterordner() {
        let t = tempfile::tempdir().unwrap();
        for n in ["Microsoft Flight Simulator 2024", "X-Plane 12", "Cyberpunk 2077", "Desktop"] {
            fs::create_dir_all(t.path().join(n)).unwrap();
        }
        fs::write(t.path().join("Microsoft Flight Simulator.mp4"), b"x").unwrap();
        let v = nvidia_fundorte(t.path());
        assert_eq!(v.len(), 2);
        assert!(v.iter().all(|f| f.art == "nvidia" && f.vorschlag_modus == "kopieren"));
        assert_eq!(v[0].simulator.as_deref(), Some("msfs"));
        assert_eq!(v[1].simulator.as_deref(), Some("xplane"));
    }

    #[test]
    fn registry_binaerwert_utf16() {
        let mut b: Vec<u8> = "D:\\Videos\\NVIDIA".encode_utf16().flat_map(u16::to_le_bytes).collect();
        b.extend_from_slice(&[0, 0, 0, 0]);
        assert_eq!(utf16_bytes_zu_text(&b).as_deref(), Some("D:\\Videos\\NVIDIA"));
        assert_eq!(utf16_bytes_zu_text(&[0, 0]), None);
        assert_eq!(utf16_bytes_zu_text(&[]), None);
    }
}
