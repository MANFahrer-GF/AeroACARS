//! Kabinen-Reaktionen nach der Landung (Thomas 23.10.2026, Konzept
//! `handoff/special-sounds/konzept-2026-10-23.md`): Die Kabine klatscht,
//! murmelt oder raunt, leise durch die Cockpittür. Pure Logik; die
//! Wiedergabe macht der Host.
//!
//! - Grundlage ist die Note, die der Pilot sieht (`score_grade` am
//!   Aufsetzen, +9 s) — nicht die Touchdown-Klasse. Weich unter 90 fpm gibt
//!   in der Hausbewertung Abzug und ist kein Grund für Applaus.
//! - Echter Hopser (>= 5 ft, nicht die leichten) → immer Raunen, geht vor.
//! - A+ → Applaus (Ferienflieger meistens, sonst etwa jedes zweite Mal),
//!   sonst zufriedenes Murmeln; A → manchmal Murmeln; B+/B/C → still;
//!   D/F → immer Raunen.
//! - Ferienflieger: Urlaubsziel ODER Ferienairline (Listen abgenommen
//!   23.10.).
//! - Einmal je Flug, nie während einer Ansage oder Musik; eine beginnende
//!   Ansage oder Musik blendet die Reaktion aus.

/// Was die Kabine tut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    ApplausFerien,
    ApplausBegeistert,
    ApplausHoeflich,
    Murmeln,
    RaunenHart,
    RaunenHopser,
}

impl Art {
    /// Kennung fürs Flugprotokoll und Präfix der Tondateien.
    pub fn name(self) -> &'static str {
        match self {
            Art::ApplausFerien => "applaus-ferienflieger",
            Art::ApplausBegeistert => "applaus-begeistert",
            Art::ApplausHoeflich => "applaus-hoeflich",
            Art::Murmeln => "murmeln-zufrieden",
            Art::RaunenHart => "raunen-hart",
            Art::RaunenHopser => "raunen-hopser",
        }
    }

    /// Ein Schreck schleicht sich nicht an: Raunen setzt fast sofort ein,
    /// Applaus kommt nach einer kurzen Pause.
    pub fn ist_raunen(self) -> bool {
        matches!(self, Art::RaunenHart | Art::RaunenHopser)
    }
}

/// A+ auf einem Ferienflieger: so oft Applaus.
pub const APPLAUS_FERIEN: f64 = 0.85;
/// A+ sonst: etwa jedes zweite Mal.
pub const APPLAUS_SONST: f64 = 0.50;
/// A: so oft zufriedenes Murmeln.
pub const MURMELN_BEI_A: f64 = 0.30;
/// Pause zwischen fester Note und Applaus/Murmeln (s), zufällig dazwischen —
/// es soll überraschen, nicht wie ein Taster wirken.
pub const PAUSE_MIN_S: f64 = 1.0;
pub const PAUSE_MAX_S: f64 = 4.0;
/// Raunen kommt sofort nach der Note.
pub const PAUSE_RAUNEN_S: f64 = 0.0;
/// Nach dem Aufsetzen so lange darf die Reaktion noch beginnen; später
/// (Neustart, lange Ansage davor) entfällt sie — sie gehört zum Aufsetzen.
/// Die Note steht nach ~9 s fest, „nach der Landung“ kam gemessen nach
/// 35–52 s.
pub const SPAETESTENS_NACH_AUFSETZEN_S: i64 = 45;

/// Was der Host über die Landung weiß.
#[derive(Debug, Clone, Copy)]
pub struct Landung<'a> {
    /// `score_grade` am Aufsetzen („A+“, „A“, „B+“ …).
    pub note: &'a str,
    /// Mindestens ein echter Hopser (>= 5 ft); leichte zählen nicht.
    pub echter_hopser: bool,
    pub ferienflieger: bool,
}

/// Entscheidet einmal je Landung. `wurf` liegt in `[0, 1)`.
pub fn waehlen(l: Landung<'_>, wurf: f64) -> Option<Art> {
    if l.echter_hopser {
        return Some(Art::RaunenHopser);
    }
    match l.note {
        "A+" => {
            let schwelle = if l.ferienflieger { APPLAUS_FERIEN } else { APPLAUS_SONST };
            if wurf >= schwelle {
                return Some(Art::Murmeln);
            }
            // Zweite Hälfte des Wurfs: welche Sorte Applaus.
            let sorte = wurf / schwelle < 0.5;
            Some(match (l.ferienflieger, sorte) {
                (true, true) => Art::ApplausFerien,
                (true, false) | (false, true) => Art::ApplausBegeistert,
                (false, false) => Art::ApplausHoeflich,
            })
        }
        "A" => (wurf < MURMELN_BEI_A).then_some(Art::Murmeln),
        "D" | "F" => Some(Art::RaunenHart),
        _ => None,
    }
}

/// Pause vor dem Einsatz (s). `wurf` liegt in `[0, 1)`.
pub fn pause_s(art: Art, wurf: f64) -> f64 {
    if art.ist_raunen() {
        PAUSE_RAUNEN_S
    } else {
        PAUSE_MIN_S + (PAUSE_MAX_S - PAUSE_MIN_S) * wurf.clamp(0.0, 1.0)
    }
}

/// Welche Aufnahme aus `anzahl`: zufällig, aber nie dieselbe wie zuletzt.
pub fn aufnahme(anzahl: usize, zuletzt: Option<usize>, wurf: f64) -> Option<usize> {
    match anzahl {
        0 => None,
        1 => Some(0),
        n => {
            let frei: Vec<usize> = (0..n).filter(|&i| Some(i) != zuletzt).collect();
            let i = ((wurf.clamp(0.0, 0.999_999) * frei.len() as f64) as usize).min(frei.len() - 1);
            Some(frei[i])
        }
    }
}

/// Ferienairlines (ICAO, aus der GSG-Datenbank; abgenommen 23.10.2026).
pub const FERIENAIRLINES: [&str; 21] = [
    "CFG", "OCN", "MBU", "TUI", "TOM", "TFL", "JAF", "SXS", "CAI", "CXI", "EDW", "EXS", "FHY", "ENT", "NOS", "VKG", "EVE",
    "WFL", "BGH", "TVS", "LTU",
];

/// Urlaubsziele (ICAO; abgenommen 23.10.2026). Großstädte bewusst nicht.
pub const URLAUBSZIELE: [&str; 85] = [
    // Balearen, Spanien
    "LEPA", "LEIB", "LEMH", "LEMG", "LEAL", "LERS", "LEGE", "LEAM",
    // Kanaren
    "GCLP", "GCTS", "GCRR", "GCFV", "GCLA", "GCHI",
    // Portugal
    "LPFR", "LPMA", "LPPS",
    // Griechenland
    "LGIR", "LGSA", "LGRP", "LGKO", "LGKR", "LGZA", "LGSR", "LGMK", "LGKF", "LGSM", "LGPZ", "LGKL",
    // Türkei
    "LTAI", "LTBS", "LTFE", "LTBJ",
    // Kroatien
    "LDSP", "LDDU", "LDPL", "LDZD",
    // Italien
    "LICC", "LICJ", "LIEO", "LIEE", "LIEA",
    // Zypern, Malta
    "LCLK", "LCPH", "LMML",
    // Bulgarien
    "LBBG", "LBWN",
    // Ägypten
    "HEGN", "HESH", "HEMA",
    // Marokko, Tunesien
    "GMMX", "GMAD", "DTTJ", "DTNH", "DTMB",
    // Kapverden
    "GVAC", "GVBA",
    // Karibik, Mexiko
    "MDPC", "MDPP", "MMUN", "MUVR", "TNCA", "TNCC", "MKJS", "TBPB", "TFFR", "TFFF",
    // Indischer Ozean
    "VRMM", "FIMP", "FSIA", "FMEE",
    // Asien
    "WADD", "VTSP", "VTSM", "VTBU",
    // USA
    "KMCO", "KLAS", "PHOG", "PHNL", "PHKO",
    // Nordsee-Inseln
    "EDXW", "EDWY",
    // Afrika Süd/Ost
    "FAKN", "HKMO", "HTZA",
];

/// Urlaubsziel ODER Ferienairline (Entscheidung 3).
pub fn ferienflieger(airline_icao: &str, ziel_icao: &str) -> bool {
    let a = airline_icao.trim().to_ascii_uppercase();
    let z = ziel_icao.trim().to_ascii_uppercase();
    FERIENAIRLINES.contains(&a.as_str()) || URLAUBSZIELE.contains(&z.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(note: &str, hopser: bool, ferien: bool) -> Landung<'_> {
        Landung { note, echter_hopser: hopser, ferienflieger: ferien }
    }

    #[test]
    fn echter_hopser_geht_jeder_note_vor() {
        for note in ["A+", "A", "B+", "B", "C", "D", "F"] {
            for w in [0.0, 0.5, 0.99] {
                assert_eq!(waehlen(l(note, true, true), w), Some(Art::RaunenHopser), "{note} {w}");
            }
        }
    }

    #[test]
    fn mittlere_noten_bleiben_still() {
        for note in ["B+", "B", "C"] {
            for w in [0.0, 0.3, 0.99] {
                assert_eq!(waehlen(l(note, false, true), w), None, "{note}");
            }
        }
    }

    #[test]
    fn schlechte_noten_raunen_immer() {
        for note in ["D", "F"] {
            for w in [0.0, 0.99] {
                assert_eq!(waehlen(l(note, false, false), w), Some(Art::RaunenHart));
            }
        }
    }

    #[test]
    fn a_murmelt_manchmal() {
        assert_eq!(waehlen(l("A", false, false), 0.0), Some(Art::Murmeln));
        assert_eq!(waehlen(l("A", false, false), MURMELN_BEI_A - 0.001), Some(Art::Murmeln));
        assert_eq!(waehlen(l("A", false, false), MURMELN_BEI_A), None);
        assert_eq!(waehlen(l("A", false, true), 0.9), None);
    }

    #[test]
    fn a_plus_klatscht_auf_dem_ferienflieger_oefter() {
        // Gleicher Wurf: Ferienflieger klatscht, Linie murmelt.
        let w = 0.7;
        assert!(matches!(waehlen(l("A+", false, true), w), Some(Art::ApplausFerien | Art::ApplausBegeistert)));
        assert_eq!(waehlen(l("A+", false, false), w), Some(Art::Murmeln));
        // Über dem Ferien-Anteil murmelt auch der Ferienflieger.
        assert_eq!(waehlen(l("A+", false, true), APPLAUS_FERIEN), Some(Art::Murmeln));
    }

    #[test]
    fn applaus_sorte_passt_zur_strecke() {
        let ferien: Vec<_> = (0..100).filter_map(|i| waehlen(l("A+", false, true), i as f64 / 100.0)).collect();
        let linie: Vec<_> = (0..100).filter_map(|i| waehlen(l("A+", false, false), i as f64 / 100.0)).collect();
        assert!(ferien.contains(&Art::ApplausFerien) && ferien.contains(&Art::ApplausBegeistert));
        assert!(!ferien.contains(&Art::ApplausHoeflich));
        assert!(linie.contains(&Art::ApplausHoeflich) && linie.contains(&Art::ApplausBegeistert));
        assert!(!linie.contains(&Art::ApplausFerien));
        // Anteile wie entschieden.
        let applaus = |v: &[Art]| v.iter().filter(|a| !matches!(a, Art::Murmeln)).count();
        assert_eq!(applaus(&ferien), 85);
        assert_eq!(applaus(&linie), 50);
    }

    #[test]
    fn unbekannte_note_bleibt_still() {
        assert_eq!(waehlen(l("", false, true), 0.0), None);
        assert_eq!(waehlen(l("a+", false, true), 0.0), None);
    }

    #[test]
    fn raunen_sofort_applaus_mit_pause() {
        assert_eq!(pause_s(Art::RaunenHart, 0.9), 0.0);
        assert_eq!(pause_s(Art::RaunenHopser, 0.1), 0.0);
        assert_eq!(pause_s(Art::ApplausFerien, 0.0), PAUSE_MIN_S);
        assert!((pause_s(Art::Murmeln, 1.0) - PAUSE_MAX_S).abs() < 1e-9);
    }

    #[test]
    fn aufnahme_nie_zweimal_hintereinander() {
        assert_eq!(aufnahme(0, None, 0.5), None);
        assert_eq!(aufnahme(1, Some(0), 0.5), Some(0));
        for w in [0.0, 0.25, 0.5, 0.75, 0.999, 1.0] {
            let a = aufnahme(3, Some(1), w).unwrap();
            assert!(a != 1 && a < 3, "{w} → {a}");
        }
        assert_eq!(aufnahme(3, None, 0.0), Some(0));
        assert_eq!(aufnahme(3, None, 0.99), Some(2));
    }

    #[test]
    fn ferienflieger_ziel_oder_airline() {
        assert!(ferienflieger("DLH", "LEPA"));
        assert!(ferienflieger("CFG", "EDDF"));
        assert!(ferienflieger(" cfg ", "eddf"));
        assert!(!ferienflieger("DLH", "EDDH"));
        assert!(!ferienflieger("EWG", "LPPT"));
    }

    #[test]
    fn listen_ohne_doppelte_und_wohlgeformt() {
        let mut z = URLAUBSZIELE.to_vec();
        z.sort_unstable();
        z.dedup();
        assert_eq!(z.len(), URLAUBSZIELE.len());
        assert!(URLAUBSZIELE.iter().all(|c| c.len() == 4 && c.chars().all(|x| x.is_ascii_uppercase())));
        let mut a = FERIENAIRLINES.to_vec();
        a.sort_unstable();
        a.dedup();
        assert_eq!(a.len(), FERIENAIRLINES.len());
        assert!(FERIENAIRLINES.iter().all(|c| c.len() == 3));
    }
}
