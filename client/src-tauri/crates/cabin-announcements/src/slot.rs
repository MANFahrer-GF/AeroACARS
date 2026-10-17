//! Der Ansage-Katalog aus dem Leitfaden (LEITFADEN_Ansage-Drehbuch.md,
//! Abschnitte 2 und 3). Jeder Slot weiß, wer spricht, ob sein Text fest
//! ist oder aus dem KI-Drehbuch kommt, und wie lang er höchstens sein darf.

use serde::{Deserialize, Serialize};

/// Wer spricht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rolle {
    Kapitaen,
    Kabine,
    /// Kurze Crew-Kommandos („Cabin crew, seats for take-off“) — fester
    /// Wortlaut, immer Englisch.
    Kommando,
    /// Signalton, kein Text.
    Chime,
}

/// Eine Ansage im Ablauf. Reihenfolge der Varianten = Reihenfolge im Flug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    // ---- Normalflug (Leitfaden Abschnitt 2) ----
    Boarding,
    BegruessungTuerenZu,
    KommandoTuerenScharf,
    Sicherheit,
    KapitaenBegruessung,
    KommandoStart,
    Chime10000Steigflug,
    AnschnallzeichenAus,
    Reiseflug25,
    Reiseflug50,
    Reiseflug80,
    KapitaenSinkflug,
    KabineSinkflug,
    Chime10000Sinkflug,
    KommandoLandungVorbereiten,
    KommandoPlaetzeLandung,
    NachDerLandung,
    KommandoTuerenEntschaerfen,
    Aussteigen,
    // ---- Besondere Lagen (Leitfaden Abschnitt 3) ----
    VerspaetungGate,
    Turbulenz,
    TurbulenzVorbei,
    Warteschleife,
    Durchstarten,
    Umleitung,
}

impl Slot {
    pub const ALLE: [Slot; 25] = [
        Slot::Boarding,
        Slot::BegruessungTuerenZu,
        Slot::KommandoTuerenScharf,
        Slot::Sicherheit,
        Slot::KapitaenBegruessung,
        Slot::KommandoStart,
        Slot::Chime10000Steigflug,
        Slot::AnschnallzeichenAus,
        Slot::Reiseflug25,
        Slot::Reiseflug50,
        Slot::Reiseflug80,
        Slot::KapitaenSinkflug,
        Slot::KabineSinkflug,
        Slot::Chime10000Sinkflug,
        Slot::KommandoLandungVorbereiten,
        Slot::KommandoPlaetzeLandung,
        Slot::NachDerLandung,
        Slot::KommandoTuerenEntschaerfen,
        Slot::Aussteigen,
        Slot::VerspaetungGate,
        Slot::Turbulenz,
        Slot::TurbulenzVorbei,
        Slot::Warteschleife,
        Slot::Durchstarten,
        Slot::Umleitung,
    ];

    pub fn rolle(self) -> Rolle {
        use Slot::*;
        match self {
            KommandoTuerenScharf
            | KommandoStart
            | KommandoLandungVorbereiten
            | KommandoPlaetzeLandung
            | KommandoTuerenEntschaerfen => Rolle::Kommando,
            Chime10000Steigflug | Chime10000Sinkflug => Rolle::Chime,
            KapitaenBegruessung | Reiseflug50 | KapitaenSinkflug
            | VerspaetungGate | Warteschleife | Durchstarten | Umleitung => Rolle::Kapitaen,
            Boarding | BegruessungTuerenZu | Sicherheit | AnschnallzeichenAus | Reiseflug25
            | Reiseflug80 | KabineSinkflug | NachDerLandung | Aussteigen | Turbulenz
            | TurbulenzVorbei => Rolle::Kabine,
        }
    }

    /// Fester Wortlaut (einmal je Airline/Muster/Sprache erzeugt und dann
    /// eingefroren) statt pro Flug aus dem KI-Drehbuch.
    pub fn text_ist_fest(self) -> bool {
        matches!(self.rolle(), Rolle::Kommando | Rolle::Chime) || self == Slot::Sicherheit
    }

    /// Kommt höchstens einmal pro Flug. Turbulenz-Ansagen dürfen sich
    /// wiederholen (mit Mindestabstand im Planer).
    pub fn einmalig(self) -> bool {
        !matches!(self, Slot::Turbulenz | Slot::TurbulenzVorbei | Slot::Warteschleife)
    }

    /// Höchstlänge des Textes je Sprache in Zeichen. Abgeleitet aus der
    /// Sprechdauer im Leitfaden mit ~14 Zeichen/Sekunde.
    pub fn max_zeichen(self) -> usize {
        use Slot::*;
        let sekunden = match self {
            Boarding => 30,
            BegruessungTuerenZu => 45,
            Sicherheit => 180,
            KapitaenBegruessung => 60,
            AnschnallzeichenAus => 40,
            Reiseflug25 | Reiseflug80 => 30,
            Reiseflug50 => 40,
            KapitaenSinkflug => 40,
            KabineSinkflug => 45,
            NachDerLandung => 60,
            Aussteigen => 30,
            VerspaetungGate => 45,
            Turbulenz | TurbulenzVorbei => 30,
            Warteschleife | Durchstarten | Umleitung => 45,
            KommandoTuerenScharf | KommandoStart | KommandoLandungVorbereiten
            | KommandoPlaetzeLandung | KommandoTuerenEntschaerfen => 5,
            Chime10000Steigflug | Chime10000Sinkflug => 0,
        };
        sekunden * 14
    }

    /// Wird von KI-Ansagen gesprochen (also nicht Kommando, Chime oder
    /// eingefrorene Sicherheit) — diese Slots muss das Drehbuch liefern.
    pub fn kommt_aus_drehbuch(self) -> bool {
        !self.text_ist_fest()
    }

    /// Darf trotz Ruhephase (Sterile Cockpit) gespielt werden.
    pub fn in_ruhephase_erlaubt(self) -> bool {
        matches!(self.rolle(), Rolle::Kommando | Rolle::Chime)
    }

    /// Vorrang in der Warteschlange: kleiner = früher.
    pub fn vorrang(self) -> u8 {
        match self.rolle() {
            Rolle::Chime => 0,
            Rolle::Kommando => 1,
            Rolle::Kapitaen => 2,
            Rolle::Kabine => 3,
        }
    }

    /// Fester Wortlaut der Crew-Kommandos (Englisch, europäische Fassung).
    /// `None` für alle anderen Slots.
    pub fn kommando_text(self) -> Option<&'static str> {
        match self {
            Slot::KommandoTuerenScharf => Some("Cabin crew, doors to automatic and cross-check."),
            Slot::KommandoStart => Some("Cabin crew, please take your seats for take-off."),
            Slot::KommandoLandungVorbereiten => Some("Cabin crew, prepare for landing."),
            Slot::KommandoPlaetzeLandung => Some("Cabin crew, please take your seats for landing."),
            Slot::KommandoTuerenEntschaerfen => Some("Cabin crew, doors to manual and cross-check."),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alle_enthaelt_jeden_slot_genau_einmal() {
        let mut v = Slot::ALLE.to_vec();
        v.sort();
        v.dedup();
        assert_eq!(v.len(), Slot::ALLE.len());
    }

    #[test]
    fn jedes_kommando_hat_festen_text_und_keine_ki_ansage_hat_einen() {
        for s in Slot::ALLE {
            assert_eq!(
                s.kommando_text().is_some(),
                s.rolle() == Rolle::Kommando,
                "{s:?}"
            );
        }
    }

    #[test]
    fn kommandos_passen_in_ihre_laenge() {
        for s in Slot::ALLE {
            if let Some(t) = s.kommando_text() {
                assert!(t.len() <= s.max_zeichen(), "{s:?} zu lang");
            }
        }
    }

    #[test]
    fn sicherheit_ist_fest_aber_kein_kommando() {
        assert!(Slot::Sicherheit.text_ist_fest());
        assert_eq!(Slot::Sicherheit.rolle(), Rolle::Kabine);
        assert!(!Slot::Sicherheit.kommt_aus_drehbuch());
        assert!(Slot::KapitaenBegruessung.kommt_aus_drehbuch());
    }

    #[test]
    fn ruhephase_laesst_nur_kommandos_und_chimes_durch() {
        assert!(Slot::KommandoStart.in_ruhephase_erlaubt());
        assert!(Slot::Chime10000Sinkflug.in_ruhephase_erlaubt());
        assert!(!Slot::KapitaenSinkflug.in_ruhephase_erlaubt());
        assert!(!Slot::Sicherheit.in_ruhephase_erlaubt());
    }
}
