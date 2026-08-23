//! Bahnbelag — befestigt, unbefestigt oder Wasser.
//!
//! # Warum das für die Bewertung zählt
//!
//! Auf einer befestigten Bahn ist die Kante eindeutig: Asphalt hört auf, Gras
//! fängt an. Auf einer Gras- oder Naturpiste ist der Übergang zum Randstreifen
//! fliessend, oft ohne erkennbare Grenze — und die Bahnbreite in den Daten ist
//! dort eine Näherung.
//!
//! Ein Rad zu bewerten, das „neben der Bahn" war, hiesse dort eine Genauigkeit
//! zu behaupten, die es nicht gibt. Deshalb setzt die seitliche Bewertung auf
//! unbefestigten Bahnen aus — sichtbar, mit Begründung. Bewertet werden dort nur
//! Aufsetzpunkt und Bahnende.
//!
//! # Warum eine Normalisierung nötig ist
//!
//! Die Belagsangaben stammen aus OurAirports und sind **uneinheitlich
//! geschrieben**. Gemessen über alle 85.058 Bahnen der eingebetteten Tabelle
//! (23.08.2026):
//!
//! ```text
//! ASP 11371   TURF 7489   CON 3652   CONC 3100   GRS 2243
//! ASPH 1678   GRE 1537    Turf 1312  GVL 1067    WATER 662   Earth 649
//! ```
//!
//! `ASP` und `ASPH` meinen dasselbe, `CON` und `CONC` ebenso, und Gras erscheint
//! als `TURF`, `Turf`, `TURF-G`, `GRS` und `GRE`. Ohne Normalisierung rutscht
//! rund die Hälfte durch — und Gras- und Naturbahnen sind in den Daten etwa so
//! häufig wie Asphaltbahnen.
//!
//! ⚠ **Quelle ist `runways.surface` (OurAirports), nicht
//! `nav_runways.surface_code`** — letzteres ist in allen 85.058 Zeilen leer.

/// Belagsart einer Bahn, so weit sie für die Bewertung zählt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Belag {
    /// Asphalt, Beton, Bitumen — die Kante ist eindeutig.
    Befestigt,
    /// Gras, Erde, Kies, Sand — der Rand ist fliessend.
    Unbefestigt,
    /// Wasserlandeplatz.
    Wasser,
    /// Nicht zuzuordnen. Wird wie `Unbefestigt` behandelt, aber mit eigenem
    /// Grund, damit man im Bericht die beiden Fälle auseinanderhält.
    Unbekannt,
}

impl Belag {
    /// Darf auf diesem Belag seitlich bewertet werden?
    ///
    /// Nur auf befestigten Bahnen. Bei allem anderen — auch bei `Unbekannt` —
    /// entfällt die seitliche Bewertung. Im Zweifel wird nicht bewertet.
    pub fn seitlich_bewertbar(self) -> bool {
        matches!(self, Belag::Befestigt)
    }

    /// Grund-Schlüssel für den Skip, wenn nicht bewertbar.
    pub fn skip_grund(self) -> &'static str {
        match self {
            Belag::Befestigt => "",
            Belag::Unbefestigt => "unpaved_runway",
            Belag::Wasser => "water_runway",
            Belag::Unbekannt => "surface_unknown",
        }
    }
}

/// Ordnet eine OurAirports-Belagsangabe einer Belagsart zu.
///
/// Die Zuordnung ist **präfixbasiert nach Normalisierung**: Gross-/Kleinschreibung
/// egal, Trennzeichen und Zusätze wie `-G` (graded) oder `-E` fallen weg. Das
/// deckt die uneinheitlichen Schreibweisen ab, ohne für jede Variante eine
/// eigene Zeile zu brauchen.
pub fn belag_aus_angabe(angabe: Option<&str>) -> Belag {
    let Some(roh) = angabe else {
        return Belag::Unbekannt;
    };
    // Normalisieren: Grossbuchstaben, alles ausser A-Z entfernt. Aus
    // "TURF-G" wird "TURFG", aus "Asph." wird "ASPH".
    let n: String = roh
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if n.is_empty() {
        return Belag::Unbekannt;
    }

    // Wasser zuerst — sonst verschluckt kein Präfix es, aber die Reihenfolge
    // macht die Absicht lesbar.
    if n.starts_with("WATER") || n.starts_with("WAT") {
        return Belag::Wasser;
    }
    // Befestigt: Asphalt, Beton, Bitumen, Pflaster, Makadam.
    for p in [
        "ASP", "ASPH", "BIT", "CON", "CONC", "PEM", "PAV", "TAR", "MAC", "COP", "COM",
    ] {
        if n.starts_with(p) {
            return Belag::Befestigt;
        }
    }
    // Unbefestigt: Gras, Erde, Kies, Sand, Schnee, Eis, Koralle.
    for p in [
        "TURF", "GRS", "GRE", "GRASS", "GRAV", "GVL", "DIRT", "EARTH", "SAND", "SNOW", "ICE",
        "CLAY", "CORAL", "SOIL", "GRD", "MATS", "PSP",
    ] {
        if n.starts_with(p) {
            return Belag::Unbefestigt;
        }
    }
    Belag::Unbekannt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deckt_die_haeufigsten_schreibweisen_ab() {
        // Die Verteilung ueber alle 85.058 Bahnen der eingebetteten Tabelle.
        // Jede Zeile hier steht fuer mindestens 649 echte Bahnen.
        for (angabe, erwartet) in [
            ("ASP", Belag::Befestigt),      // 11371
            ("TURF", Belag::Unbefestigt),   //  7489
            ("CON", Belag::Befestigt),      //  3652
            ("CONC", Belag::Befestigt),     //  3100
            ("GRS", Belag::Unbefestigt),    //  2243
            ("ASPH", Belag::Befestigt),     //  1678
            ("GRE", Belag::Unbefestigt),    //  1537
            ("Turf", Belag::Unbefestigt),   //  1312 — Kleinschreibung!
            ("GVL", Belag::Unbefestigt),    //  1067
            ("WATER", Belag::Wasser),       //   662
            ("Earth", Belag::Unbefestigt),  //   649
        ] {
            assert_eq!(
                belag_aus_angabe(Some(angabe)),
                erwartet,
                "{angabe} falsch zugeordnet"
            );
        }
    }

    #[test]
    fn trennzeichen_und_zusaetze_stoeren_nicht() {
        // "TURF-G" (graded) ist dieselbe Bahn wie "TURF".
        for angabe in ["TURF-G", "turf-g", "TURF G", "Turf/Gravel"] {
            assert_eq!(
                belag_aus_angabe(Some(angabe)),
                Belag::Unbefestigt,
                "{angabe}"
            );
        }
        for angabe in ["ASP-CON", "Asph.", "asphalt", "CONC/ASPH"] {
            assert_eq!(belag_aus_angabe(Some(angabe)), Belag::Befestigt, "{angabe}");
        }
    }

    #[test]
    fn seitlich_bewertet_wird_nur_auf_befestigtem() {
        assert!(Belag::Befestigt.seitlich_bewertbar());
        assert!(!Belag::Unbefestigt.seitlich_bewertbar());
        assert!(!Belag::Wasser.seitlich_bewertbar());
        // Im Zweifel NICHT bewerten — ein geratener Belag waere eine
        // Behauptung ueber die Kante, die es so nicht gibt.
        assert!(!Belag::Unbekannt.seitlich_bewertbar());
    }

    #[test]
    fn skip_gruende_sind_unterscheidbar() {
        // "Graspiste" und "Belag unbekannt" sind zwei verschiedene Aussagen.
        assert_eq!(Belag::Unbefestigt.skip_grund(), "unpaved_runway");
        assert_eq!(Belag::Wasser.skip_grund(), "water_runway");
        assert_eq!(Belag::Unbekannt.skip_grund(), "surface_unknown");
    }

    #[test]
    fn leeres_und_unsinniges_ist_unbekannt() {
        for angabe in [None, Some(""), Some("   "), Some("???"), Some("XYZ")] {
            assert_eq!(belag_aus_angabe(angabe), Belag::Unbekannt, "{angabe:?}");
        }
    }
}
