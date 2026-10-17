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

/// Standard-Schwelle für „Bizjet“: bis einschließlich so vieler Sitze.
/// Annahme aus dem Leitfaden, in den Einstellungen änderbar.
pub const BIZJET_SITZE_STANDARD: u32 = 19;

/// phpVMS-Flugarten ohne Passagiere: F Scheduled Cargo, A Additional Cargo,
/// H Charter Cargo/Mail, M Mail Service, P Positioning.
const OHNE_PASSAGIERE: [&str; 5] = ["F", "A", "H", "M", "P"];

/// Ordnet den Flug ein. `pax` = gebuchte Passagiere (OFP oder Fares),
/// `sitze` = Sitzplätze des Musters (OFP `max_passengers`).
pub fn einordnen(
    flight_type: Option<&str>,
    pax: Option<u32>,
    sitze: Option<u32>,
    bizjet_bis_sitze: u32,
) -> Flugart {
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
    match sitze {
        Some(s) if s <= bizjet_bis_sitze => Flugart::Bizjet,
        _ => Flugart::Linie,
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
            assert_eq!(einordnen(Some(ft), Some(180), Some(180), 19), Flugart::Aus, "{ft}");
        }
    }

    #[test]
    fn ohne_passagiere_aus_auch_bei_linien_flugart() {
        assert_eq!(einordnen(Some("J"), Some(0), Some(180), 19), Flugart::Aus);
    }

    #[test]
    fn kleine_muster_sind_bizjet() {
        assert_eq!(einordnen(Some("J"), Some(6), Some(12), 19), Flugart::Bizjet);
        assert_eq!(einordnen(None, Some(19), Some(19), 19), Flugart::Bizjet);
        assert_eq!(einordnen(None, Some(20), Some(20), 19), Flugart::Linie);
    }

    #[test]
    fn ohne_passagierzahl_aus() {
        assert_eq!(einordnen(None, None, None, 19), Flugart::Aus);
        assert_eq!(einordnen(Some("J"), None, Some(180), 19), Flugart::Aus);
        // Gegenprobe: mit Passagieren, aber ohne Sitzangabe bleibt es Linie.
        assert_eq!(einordnen(Some("J"), Some(150), None, 19), Flugart::Linie);
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
