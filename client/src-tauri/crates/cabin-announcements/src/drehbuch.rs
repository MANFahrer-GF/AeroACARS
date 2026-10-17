//! Das Drehbuch: die KI-Texte eines Flugs, je Slot und Sprache. Kommt als
//! JSON vom GSG-Endpunkt. Hier wird es geprüft, bevor auch nur ein Zeichen
//! an die Sprachausgabe geht (Leitfaden Abschnitte 6 und 8).

use serde::{Deserialize, Serialize};

use crate::slot::Slot;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Eintrag {
    pub slot: Slot,
    /// BCP-47, z. B. "de-DE", "en-GB".
    pub sprache: String,
    pub text: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drehbuch {
    pub eintraege: Vec<Eintrag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Befund {
    Fehlt { slot: Slot, sprache: String },
    Leer { slot: Slot, sprache: String },
    ZuLang { slot: Slot, sprache: String, zeichen: usize, max: usize },
    /// Fester Slot (Kommando/Sicherheit) gehört nicht ins KI-Drehbuch.
    FesterSlot { slot: Slot },
    /// Ziffern werden von der Sprachausgabe falsch gelesen — die KI soll
    /// Zahlen und Uhrzeiten in Worten schreiben.
    Ziffer { slot: Slot, sprache: String },
    /// ICAO-Code im Text („Willkommen in EDDF“) — nie vorlesen.
    IcaoCode { slot: Slot, sprache: String, code: String },
    GesamtZuLang { zeichen: usize, max: usize },
}

impl Drehbuch {
    pub fn text(&self, slot: Slot, sprache: &str) -> Option<&str> {
        self.eintraege
            .iter()
            .find(|e| e.slot == slot && e.sprache.eq_ignore_ascii_case(sprache))
            .map(|e| e.text.as_str())
    }

    pub fn zeichen_gesamt(&self) -> usize {
        self.eintraege.iter().map(|e| e.text.chars().count()).sum()
    }

    /// Prüft das Drehbuch. `slots` = die Slots, die dieser Flug braucht,
    /// `sprachen` = die gewünschten Sprachen, `icao_codes` = Codes, die
    /// nicht im Text stehen dürfen (Abflug, Ziel, Ausweich, Airline).
    pub fn pruefen(
        &self,
        slots: &[Slot],
        sprachen: &[&str],
        icao_codes: &[&str],
        max_gesamt: usize,
    ) -> Vec<Befund> {
        let mut befunde = Vec::new();
        for e in &self.eintraege {
            if !e.slot.kommt_aus_drehbuch() {
                befunde.push(Befund::FesterSlot { slot: e.slot });
                continue;
            }
            let n = e.text.chars().count();
            let max = e.slot.max_zeichen();
            if e.text.trim().is_empty() {
                befunde.push(Befund::Leer { slot: e.slot, sprache: e.sprache.clone() });
            } else if n > max {
                befunde.push(Befund::ZuLang { slot: e.slot, sprache: e.sprache.clone(), zeichen: n, max });
            }
            if e.text.chars().any(|c| c.is_ascii_digit()) {
                befunde.push(Befund::Ziffer { slot: e.slot, sprache: e.sprache.clone() });
            }
            for code in icao_codes.iter().filter(|c| c.len() >= 3) {
                if enthaelt_wort(&e.text, code) {
                    befunde.push(Befund::IcaoCode {
                        slot: e.slot,
                        sprache: e.sprache.clone(),
                        code: (*code).to_string(),
                    });
                }
            }
        }
        for &slot in slots.iter().filter(|s| s.kommt_aus_drehbuch()) {
            for &sprache in sprachen {
                if self.text(slot, sprache).is_none() {
                    befunde.push(Befund::Fehlt { slot, sprache: sprache.to_string() });
                }
            }
        }
        let gesamt = self.zeichen_gesamt();
        if gesamt > max_gesamt {
            befunde.push(Befund::GesamtZuLang { zeichen: gesamt, max: max_gesamt });
        }
        befunde
    }
}

/// Ganzes Wort, Groß/klein egal („EDDF“ ja, „EDDFX“ nein).
fn enthaelt_wort(text: &str, wort: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case(wort))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(slot: Slot, sprache: &str, text: &str) -> Eintrag {
        Eintrag { slot, sprache: sprache.into(), text: text.into() }
    }

    #[test]
    fn sauberes_drehbuch_ohne_befund() {
        let d = Drehbuch {
            eintraege: vec![
                e(Slot::KapitaenBegruessung, "de-DE", "Guten Abend, liebe Gäste, hier spricht Ihr Kapitän."),
                e(Slot::KapitaenBegruessung, "en-GB", "Good evening, this is your captain speaking."),
            ],
        };
        assert!(d.pruefen(&[Slot::KapitaenBegruessung], &["de-DE", "en-GB"], &["EDDF"], 10_000).is_empty());
    }

    #[test]
    fn findet_fehlende_sprache_ziffern_icao_und_feste_slots() {
        let d = Drehbuch {
            eintraege: vec![
                e(Slot::KapitaenBegruessung, "de-DE", "Wir landen um 20:35 in EDDF."),
                e(Slot::KommandoStart, "en-GB", "Cabin crew, seats for take-off."),
            ],
        };
        let b = d.pruefen(&[Slot::KapitaenBegruessung, Slot::KommandoStart], &["de-DE", "en-GB"], &["EDDF", "LFBO"], 10_000);
        assert!(b.contains(&Befund::Ziffer { slot: Slot::KapitaenBegruessung, sprache: "de-DE".into() }));
        assert!(b.contains(&Befund::IcaoCode { slot: Slot::KapitaenBegruessung, sprache: "de-DE".into(), code: "EDDF".into() }));
        assert!(b.contains(&Befund::FesterSlot { slot: Slot::KommandoStart }));
        assert!(b.contains(&Befund::Fehlt { slot: Slot::KapitaenBegruessung, sprache: "en-GB".into() }));
        // Kommando wird nicht als „fehlt“ gemeldet — es kommt nicht aus dem Drehbuch.
        assert!(!b.iter().any(|x| matches!(x, Befund::Fehlt { slot: Slot::KommandoStart, .. })));
    }

    #[test]
    fn laengen_je_slot_und_gesamt() {
        let lang = "a".repeat(Slot::Boarding.max_zeichen() + 1);
        let d = Drehbuch { eintraege: vec![e(Slot::Boarding, "en-GB", &lang)] };
        let b = d.pruefen(&[], &[], &[], 100);
        assert!(b.iter().any(|x| matches!(x, Befund::ZuLang { slot: Slot::Boarding, .. })));
        assert!(b.iter().any(|x| matches!(x, Befund::GesamtZuLang { .. })));
    }

    #[test]
    fn icao_nur_als_ganzes_wort() {
        assert!(enthaelt_wort("Willkommen in eddf!", "EDDF"));
        assert!(!enthaelt_wort("EDDFX", "EDDF"));
    }

    #[test]
    fn json_rundreise() {
        let d = Drehbuch { eintraege: vec![e(Slot::Reiseflug50, "de-DE", "Halbzeit.")] };
        let j = serde_json::to_string(&d).unwrap();
        assert!(j.contains("\"reiseflug50\""));
        assert_eq!(serde_json::from_str::<Drehbuch>(&j).unwrap(), d);
    }
}
