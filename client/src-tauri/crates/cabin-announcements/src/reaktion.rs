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

// Wert seit v3: bundle/tabellen/kabine.json -> werte.applaus_ferien (v2.1.24: 0.85)
// Wert seit v3: bundle/tabellen/kabine.json -> werte.applaus_sonst (v2.1.24: 0.50)
// Wert seit v3: bundle/tabellen/kabine.json -> werte.murmeln_bei_a (v2.1.24: 0.30)
// Wert seit v3: bundle/tabellen/kabine.json -> werte.pause_min_s (v2.1.24: 1.0)
// Wert seit v3: bundle/tabellen/kabine.json -> werte.pause_max_s (v2.1.24: 4.0)
// Wert seit v3: bundle/tabellen/kabine.json -> werte.pause_raunen_s (v2.1.24: 0.0)
// Wert seit v3: bundle/tabellen/kabine.json -> werte.spaetestens_nach_aufsetzen_s (v2.1.24: 45)

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
            let schwelle = if l.ferienflieger { crate::tabelle().werte.applaus_ferien.wert } else { crate::tabelle().werte.applaus_sonst.wert };
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
        "A" => (wurf < crate::tabelle().werte.murmeln_bei_a.wert).then_some(Art::Murmeln),
        "D" | "F" => Some(Art::RaunenHart),
        _ => None,
    }
}

/// Pause vor dem Einsatz (s). `wurf` liegt in `[0, 1)`.
pub fn pause_s(art: Art, wurf: f64) -> f64 {
    if art.ist_raunen() {
        crate::tabelle().werte.pause_raunen_s.wert
    } else {
        crate::tabelle().werte.pause_min_s.wert + (crate::tabelle().werte.pause_max_s.wert - crate::tabelle().werte.pause_min_s.wert) * wurf.clamp(0.0, 1.0)
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

// Ferienairlines und Urlaubsziele: seit v3 bundle/tabellen/kabine.json ->
// listen.ferienairlines / listen.urlaubsziele

/// Urlaubsziel ODER Ferienairline (Entscheidung 3).
pub fn ferienflieger(airline_icao: &str, ziel_icao: &str) -> bool {
    let a = airline_icao.trim().to_ascii_uppercase();
    let z = ziel_icao.trim().to_ascii_uppercase();
    crate::tabelle().listen.ferienairlines.contains(&a) || crate::tabelle().listen.urlaubsziele.contains(&z)
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
        assert_eq!(waehlen(l("A", false, false), crate::tabelle().werte.murmeln_bei_a.wert - 0.001), Some(Art::Murmeln));
        assert_eq!(waehlen(l("A", false, false), crate::tabelle().werte.murmeln_bei_a.wert), None);
        assert_eq!(waehlen(l("A", false, true), 0.9), None);
    }

    #[test]
    fn a_plus_klatscht_auf_dem_ferienflieger_oefter() {
        // Gleicher Wurf: Ferienflieger klatscht, Linie murmelt.
        let w = 0.7;
        assert!(matches!(waehlen(l("A+", false, true), w), Some(Art::ApplausFerien | Art::ApplausBegeistert)));
        assert_eq!(waehlen(l("A+", false, false), w), Some(Art::Murmeln));
        // Über dem Ferien-Anteil murmelt auch der Ferienflieger.
        assert_eq!(waehlen(l("A+", false, true), crate::tabelle().werte.applaus_ferien.wert), Some(Art::Murmeln));
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
        assert_eq!(pause_s(Art::ApplausFerien, 0.0), crate::tabelle().werte.pause_min_s.wert);
        assert!((pause_s(Art::Murmeln, 1.0) - crate::tabelle().werte.pause_max_s.wert).abs() < 1e-9);
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
        let mut z = crate::tabelle().listen.urlaubsziele.to_vec();
        z.sort_unstable();
        z.dedup();
        assert_eq!(z.len(), crate::tabelle().listen.urlaubsziele.len());
        assert!(crate::tabelle().listen.urlaubsziele.iter().all(|c| c.len() == 4 && c.chars().all(|x| x.is_ascii_uppercase())));
        let mut a = crate::tabelle().listen.ferienairlines.to_vec();
        a.sort_unstable();
        a.dedup();
        assert_eq!(a.len(), crate::tabelle().listen.ferienairlines.len());
        assert!(crate::tabelle().listen.ferienairlines.iter().all(|c| c.len() == 3));
    }
}
