//! Simulator automatisch waehlen (v1.9.14, Wunsch Thomas 30.09.2026).
//!
//! # Was hier entschieden wird
//!
//! Steht die Auswahl auf „Automatisch“, schaut ein Waechter alle paar
//! Sekunden nach, welcher Simulator laeuft, und stellt den Adapter darauf
//! um. Die Logik hier ist rein (keine Prozessliste, kein Adapter), damit
//! sie ohne Simulator testbar ist; den Waechter selbst gibt es in `lib.rs`.
//!
//! # Zwei Stufen
//!
//! 1. **Familie** (MSFS oder X-Plane) aus der Prozessliste — nur solange
//!    der aktuelle Adapter NICHT verbunden ist.
//! 2. **Fassung** (2020/2024, 11/12) aus dem, was der Simulator beim
//!    Verbinden selbst meldet: MSFS nennt sich „KittyHawk“ (2020) bzw.
//!    „SunRise“ (2024, in 124 Pilotenlogs belegt), X-Plane gibt ueber das
//!    Plugin seine Versionsnummer.
//!
//! # Was hier NICHT entschieden wird
//!
//! Waehrend eines Flugs schaltet der Waechter nie um — das prueft der
//! Aufrufer. Laufen beide Simulatoren, bleibt es beim aktuellen. Laeuft
//! keiner, ebenfalls (kein Wechsel auf „Aus“).

use sim_core::process_probe::LaufendeSimulatoren;
use sim_core::SimKind;

/// Fassung aus der SimConnect-Kennung („SunRise 12.2“, „KittyHawk 11.0“).
/// Aufgerufen nur im Windows-Zweig (MSFS gibt es nur dort).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn fassung_aus_msfs_kennung(kennung: &str) -> Option<SimKind> {
    let k = kennung.trim().to_ascii_lowercase();
    if k.starts_with("sunrise") {
        Some(SimKind::Msfs2024)
    } else if k.starts_with("kittyhawk") {
        Some(SimKind::Msfs2020)
    } else {
        None
    }
}

/// Fassung aus der X-Plane-Version des Plugins (`XPLMGetVersions`, z. B.
/// 12100 fuer 12.1.0, 11550 fuer 11.55). Kuenftige Hauptversionen gelten
/// als 12 — der Adapter kennt nur 11 und 12.
pub(crate) fn fassung_aus_xplane_version(version: u32) -> Option<SimKind> {
    let haupt = if version >= 100_000 {
        version / 10_000
    } else {
        version / 1_000
    };
    match haupt {
        11 => Some(SimKind::XPlane11),
        h if h >= 12 => Some(SimKind::XPlane12),
        _ => None,
    }
}

fn gleiche_familie(a: SimKind, b: SimKind) -> bool {
    (a.is_msfs() && b.is_msfs()) || (a.is_xplane() && b.is_xplane())
}

/// Worauf soll umgestellt werden? `None` = so lassen.
///
/// * `verbunden` — der Adapter des aktuellen Simulators hat Verbindung.
/// * `laufend` — Prozessliste (nur gebraucht, wenn nicht verbunden).
/// * `gemeldet` — die Fassung, die der verbundene Simulator selbst meldet.
pub(crate) fn naechste_wahl(
    aktuell: SimKind,
    verbunden: bool,
    laufend: Option<LaufendeSimulatoren>,
    gemeldet: Option<SimKind>,
) -> Option<SimKind> {
    if verbunden {
        // Nur die Fassung innerhalb derselben Familie berichtigen.
        return gemeldet.filter(|g| *g != aktuell && gleiche_familie(*g, aktuell));
    }
    let l = laufend?;
    let msfs = l.msfs && cfg!(target_os = "windows");
    let ziel = match (msfs, l.xplane) {
        (true, false) => {
            if aktuell.is_msfs() {
                return None; // richtige Familie, verbindet noch
            }
            // Ohne Kennung: der eindeutige 2024er-Name, sonst 2024 als die
            // Fassung fast aller Piloten. Die Kennung berichtigt danach.
            SimKind::Msfs2024
        }
        (false, true) => {
            if aktuell.is_xplane() {
                return None;
            }
            SimKind::XPlane12
        }
        // Beide oder keiner: nichts entscheiden.
        _ => return None,
    };
    (ziel != aktuell).then_some(ziel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use SimKind::*;

    fn l(msfs: bool, xplane: bool) -> Option<LaufendeSimulatoren> {
        Some(LaufendeSimulatoren {
            msfs,
            msfs2024_name: false,
            xplane,
        })
    }

    #[test]
    fn msfs_kennung() {
        assert_eq!(fassung_aus_msfs_kennung("SunRise 12.2"), Some(Msfs2024));
        assert_eq!(fassung_aus_msfs_kennung("KittyHawk 11.0"), Some(Msfs2020));
        assert_eq!(fassung_aus_msfs_kennung("Prepar3D 5.4"), None);
        assert_eq!(fassung_aus_msfs_kennung(""), None);
    }

    #[test]
    fn xplane_version() {
        assert_eq!(fassung_aus_xplane_version(12100), Some(XPlane12));
        assert_eq!(fassung_aus_xplane_version(11550), Some(XPlane11));
        assert_eq!(fassung_aus_xplane_version(120_400), Some(XPlane12));
        assert_eq!(fassung_aus_xplane_version(13000), Some(XPlane12));
        assert_eq!(fassung_aus_xplane_version(10510), None);
        assert_eq!(fassung_aus_xplane_version(0), None);
    }

    #[test]
    fn verbunden_berichtigt_nur_die_fassung() {
        assert_eq!(
            naechste_wahl(Msfs2024, true, None, Some(Msfs2020)),
            Some(Msfs2020)
        );
        assert_eq!(
            naechste_wahl(XPlane12, true, None, Some(XPlane11)),
            Some(XPlane11)
        );
        // Gleiche Fassung oder nichts gemeldet: bleiben.
        assert_eq!(naechste_wahl(Msfs2024, true, None, Some(Msfs2024)), None);
        assert_eq!(naechste_wahl(Msfs2024, true, None, None), None);
        // Nie die Familie wechseln, solange verbunden — auch wenn die
        // Prozessliste etwas anderes sagt.
        assert_eq!(
            naechste_wahl(Msfs2024, true, l(false, true), Some(XPlane12)),
            None
        );
    }

    #[test]
    fn nicht_verbunden_folgt_dem_laufenden_simulator() {
        assert_eq!(
            naechste_wahl(Msfs2024, false, l(false, true), None),
            Some(XPlane12)
        );
        assert_eq!(
            naechste_wahl(Off, false, l(false, true), None),
            Some(XPlane12)
        );
        // Richtige Familie laeuft schon, verbindet nur noch: bleiben.
        assert_eq!(naechste_wahl(XPlane11, false, l(false, true), None), None);
        // Beide oder keiner: nichts entscheiden. (Ausserhalb von Windows
        // zaehlt ein MSFS-Prozess nicht — dort ist „beide“ nur X-Plane.)
        let beide = naechste_wahl(Msfs2024, false, l(true, true), None);
        if cfg!(target_os = "windows") {
            assert_eq!(beide, None);
        } else {
            assert_eq!(beide, Some(XPlane12));
        }
        assert_eq!(naechste_wahl(XPlane12, false, l(false, false), None), None);
        assert_eq!(naechste_wahl(XPlane12, false, None, None), None);
    }

    #[test]
    fn msfs_nur_unter_windows() {
        let erwartet = if cfg!(target_os = "windows") {
            Some(Msfs2024)
        } else {
            None
        };
        assert_eq!(
            naechste_wahl(XPlane12, false, l(true, false), None),
            erwartet
        );
        if cfg!(target_os = "windows") {
            assert_eq!(naechste_wahl(Msfs2020, false, l(true, false), None), None);
        }
    }
}
