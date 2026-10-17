//! Welche Flüge überhaupt Ansagen bekommen (Leitfaden Abschnitt 2a).

use serde::{Deserialize, Serialize};

use crate::slot::Slot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flugart {
    /// Linienflug mit Passagieren: voller Ablauf.
    Linie,
    /// Bizjet/Kleinflugzeug: nur der Kapitän, kurz.
    Bizjet,
    /// Fracht, Überführung, ohne Passagiere: keine Ansagen.
    Aus,
}

/// Muster ohne Kabinenbesatzung (ICAO-Typ): Business-Jets, Kleinflugzeuge,
/// Turboprops ohne Flugbegleiter, Hubschrauber, Militär. Hier spricht nur der
/// Kapitän. Entscheidend ist der Typ, nicht die Sitzzahl (Thomas 17.10.2026:
/// der A380-Versuchstraeger F-WWDD hat in der Unterflotte nur 10 Plaetze).
/// Unbekannte Typen gelten als Linie.
const OHNE_KABINENCREW: &[&str] = &[
    // Business-Jets: wie `bordbuch::BUSINESS_JETS` (ICAO Doc 8643).
    "ASTR", "BE40", "C25A", "C25B", "C25C", "C25M", "C500", "C501", "C510", "C525", "C526", "C550",
    "C551", "C560", "C56X", "C650", "C680", "C68A", "C700", "C750", "CL30", "CL35", "CL60", "E35L",
    "E50P", "E545", "E550", "E55P", "EA50", "F2TH", "F900", "FA10", "FA20", "FA50", "FA7X", "FA8X",
    "G150", "G280", "GA5C", "GA6C", "GALX", "GL5T", "GL7T", "GLEX", "GLF2", "GLF3", "GLF4", "GLF5",
    "GLF6", "H25A", "H25B", "H25C", "HDJT", "LJ23", "LJ24", "LJ25", "LJ31", "LJ35", "LJ40", "LJ45",
    "LJ55", "LJ60", "LJ70", "LJ75", "PC24", "PRM1", "SF50", "WW24",
    // Kleinflugzeuge, leichte Turboprops, Hubschrauber: wie `bordbuch::GA_TYPEN`.
    "AC11", "AN2", "AS50", "ASK2", "B06", "B407", "BE20", "BE33", "BE35", "BE36", "BE55", "BE58",
    "BE9L", "BE9T", "BL8", "C140", "C150", "C152", "C170", "C172", "C177", "C182", "C206", "C207",
    "C208", "C210", "C310", "C337", "C340", "C414", "C421", "CRUZ", "DA20", "DA40", "DA42", "DA50",
    "DA62", "DHC2", "DHC3", "DHC6", "DR40", "EC35", "EC45", "EV97", "GLID", "H125", "H135", "H145",
    "J3", "KODI", "M20P", "M20T", "P28A", "P28B", "P28R", "P32R", "P46T", "PA18", "PA24", "PA28",
    "PA31", "PA32", "PA34", "PA44", "PA46", "PC12", "PC6T", "PC7", "R22", "R44", "R66", "RV10",
    "RV7", "RV8", "S22T", "S76", "SR20", "SR22", "TBM7", "TBM8", "TBM9", "TOBA", "ULAC",
    // Dazu: Turboprops ohne Flugbegleiter und Muster der GSG-Flotte, die
    // dort fehlen (Stand 17.10.2026), sowie Militaer.
    "A109", "A139", "AEST", "B190", "B350", "BE10", "BE24", "BE30", "BE60", "BE76", "C162", "C185",
    "C404", "C441", "E13L", "EUFI", "GA8", "MU2", "P180", "P68", "SR2T", "YK18",
];

/// phpVMS-Flugarten ohne Passagiere: F Scheduled Cargo, A Additional Cargo,
/// H Charter Cargo/Mail, M Mail Service, P Positioning.
const OHNE_PASSAGIERE: [&str; 5] = ["F", "A", "H", "M", "P"];

/// Ordnet den Flug ein. `pax` = gebuchte Passagiere (OFP oder Fares),
/// `muster` = ICAO-Typ des gebuchten Flugzeugs.
pub fn einordnen(flight_type: Option<&str>, pax: Option<u32>, muster: &str) -> Flugart {
    if let Some(ft) = flight_type {
        let ft = ft.trim().to_ascii_uppercase();
        if OHNE_PASSAGIERE.contains(&ft.as_str()) {
            return Flugart::Aus;
        }
    }
    // Ohne Passagierzahl (weder OFP noch Passagier-Fares) keine Ansagen —
    // Entscheidung Thomas, 17.10.2026. Das trifft z. B. den manuellen VFR-Flug.
    if matches!(pax, None | Some(0)) {
        return Flugart::Aus;
    }
    let muster = muster.trim().to_ascii_uppercase();
    if OHNE_KABINENCREW.contains(&muster.as_str()) {
        Flugart::Bizjet
    } else {
        Flugart::Linie
    }
}

impl Flugart {
    /// Darf dieser Slot bei dieser Flugart überhaupt kommen?
    pub fn erlaubt(self, slot: Slot) -> bool {
        match self {
            Flugart::Aus => false,
            Flugart::Linie => true,
            Flugart::Bizjet => matches!(
                slot,
                Slot::KapitaenBegruessung
                    | Slot::KapitaenSinkflug
                    | Slot::VerspaetungGate
                    | Slot::Warteschleife
                    | Slot::Durchstarten
                    | Slot::Umleitung
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fracht_und_ueberfuehrung_sind_aus() {
        for ft in ["F", "a", " h ", "M", "P"] {
            assert_eq!(einordnen(Some(ft), Some(180), "A320"), Flugart::Aus, "{ft}");
        }
    }

    #[test]
    fn ohne_passagiere_aus_auch_bei_linien_flugart() {
        assert_eq!(einordnen(Some("J"), Some(0), "A320"), Flugart::Aus);
    }

    #[test]
    fn kleine_muster_sind_bizjet() {
        assert_eq!(einordnen(Some("J"), Some(6), "C25A"), Flugart::Bizjet);
        assert_eq!(einordnen(None, Some(4), " pc12 "), Flugart::Bizjet);
        assert_eq!(einordnen(None, Some(19), "B190"), Flugart::Bizjet);
        // Der Typ entscheidet, nicht die Platzzahl: A380-Versuchstraeger mit
        // 10 Plaetzen ist Linie, ein ATR mit 20 Pax auch.
        assert_eq!(einordnen(None, Some(10), "A388"), Flugart::Linie);
        assert_eq!(einordnen(None, Some(20), "AT76"), Flugart::Linie);
        // Unbekannter oder fehlender Typ: Linie.
        assert_eq!(einordnen(None, Some(150), ""), Flugart::Linie);
        assert_eq!(einordnen(None, Some(150), "ZZZZ"), Flugart::Linie);
    }

    #[test]
    fn ohne_passagierzahl_aus() {
        assert_eq!(einordnen(None, None, "A320"), Flugart::Aus);
        assert_eq!(einordnen(Some("J"), None, "A320"), Flugart::Aus);
        // Gegenprobe: mit Passagieren bleibt es Linie.
        assert_eq!(einordnen(Some("J"), Some(150), "A320"), Flugart::Linie);
    }

    #[test]
    fn bizjet_hat_keine_kabine_und_keine_kommandos() {
        assert!(Flugart::Bizjet.erlaubt(Slot::KapitaenBegruessung));
        assert!(!Flugart::Bizjet.erlaubt(Slot::Sicherheit));
        assert!(!Flugart::Bizjet.erlaubt(Slot::KommandoStart));
        assert!(!Flugart::Bizjet.erlaubt(Slot::NachDerLandung));
        assert!(!Flugart::Aus.erlaubt(Slot::KapitaenBegruessung));
    }
}
