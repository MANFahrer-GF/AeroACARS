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

// Muster ohne Kabinenbesatzung: seit v3 bundle/tabellen/kabine.json -> listen.ohne_kabinencrew

// Flugarten ohne Passagiere: seit v3 bundle/tabellen/kabine.json -> listen.ohne_passagiere

/// Ordnet den Flug ein. `pax` = gebuchte Passagiere (OFP oder Fares),
/// `muster` = ICAO-Typ des gebuchten Flugzeugs.
pub fn einordnen(flight_type: Option<&str>, pax: Option<u32>, muster: &str) -> Flugart {
    if let Some(ft) = flight_type {
        let ft = ft.trim().to_ascii_uppercase();
        if crate::tabelle().listen.ohne_passagiere.iter().any(|x| *x == ft) {
            return Flugart::Aus;
        }
    }
    // Ohne Passagierzahl (weder OFP noch Passagier-Fares) keine Ansagen —
    // Entscheidung Thomas, 17.10.2026. Das trifft z. B. den manuellen VFR-Flug.
    if matches!(pax, None | Some(0)) {
        return Flugart::Aus;
    }
    let muster = muster.trim().to_ascii_uppercase();
    if crate::tabelle().listen.ohne_kabinencrew.contains(&muster) {
        Flugart::Bizjet
    } else {
        Flugart::Linie
    }
}

/// Steht die Einordnung fest? Ohne Passagierzahl ist ein Flug erst dann
/// sicher „aus“, wenn seine Flugart keine Passagiere kennt (Fracht,
/// Überführung); sonst holt der Host die Zahl vielleicht noch nach.
pub fn eingeordnet(flight_type: Option<&str>, pax: Option<u32>) -> bool {
    pax.is_some() || flight_type.is_some_and(|ft| crate::tabelle().listen.ohne_passagiere.contains(&ft.trim().to_ascii_uppercase()))
}

impl Flugart {
    /// Darf dieser Slot bei dieser Flugart überhaupt kommen?
    pub fn erlaubt(self, slot: Slot) -> bool {
        match self {
            Flugart::Aus => false,
            Flugart::Linie => true,
            // Gong (Anschnallzeichen) auch ohne Kabinencrew (Thomas
            // 18.10.2026: „Gong immer“). Sicherheit nur als Kurzfassung des
            // Kapitäns: Der Host spielt sie nur, wenn der Server sie so
            // kennzeichnet (`rolle: kapitaen`), sonst entfällt sie.
            Flugart::Bizjet => matches!(
                slot,
                Slot::KapitaenBegruessung
                    | Slot::Sicherheit
                    | Slot::Chime10000Steigflug
                    | Slot::Chime10000Sinkflug
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
    fn eingeordnet_erst_mit_passagierzahl_oder_frachtart() {
        assert!(eingeordnet(Some("F"), None), "Fracht steht fest");
        assert!(!eingeordnet(Some("J"), None), "Linie ohne Zahl: noch offen");
        assert!(!eingeordnet(None, None));
        assert!(eingeordnet(None, Some(0)));
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
        // Gong und Kapitäns-Kurzfassung der Sicherheit ja (beta.6) …
        assert!(Flugart::Bizjet.erlaubt(Slot::Chime10000Steigflug));
        assert!(Flugart::Bizjet.erlaubt(Slot::Chime10000Sinkflug));
        assert!(Flugart::Bizjet.erlaubt(Slot::Sicherheit));
        // … Kabinenansagen und Kommandos nein.
        assert!(!Flugart::Bizjet.erlaubt(Slot::Boarding));
        assert!(!Flugart::Bizjet.erlaubt(Slot::KabineSinkflug));
        assert!(!Flugart::Bizjet.erlaubt(Slot::KommandoStart));
        assert!(!Flugart::Bizjet.erlaubt(Slot::NachDerLandung));
        assert!(!Flugart::Aus.erlaubt(Slot::KapitaenBegruessung));
    }
}
