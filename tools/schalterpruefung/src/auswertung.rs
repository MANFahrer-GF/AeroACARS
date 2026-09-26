//! Diff-Auswertung: Welche Variable folgt dem Schalter?
//!
//! Plattformunabhängig (auf dem Mac getestet). Eine Messung ist ein Vektor
//! über ALLE beobachteten Variablen (LVars + Standard-SimVars) in fester
//! Reihenfolge; `None` = in dieser Messung nicht geliefert.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::skripte::Schalter;

pub type Werte = Vec<Option<f64>>;

/// Zwei Messwerte gelten als gleich, wenn beide vorhanden sind und sich
/// höchstens im Rundungsrauschen eines float32 unterscheiden. Die LVars kommen
/// aus MobiFlight als float32 — mehr Genauigkeit gibt es nicht.
pub fn gleich(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => {
            if x == y {
                return true;
            }
            let skala = x.abs().max(y.abs()).max(1.0);
            (x - y).abs() <= 1e-6 * skala
        }
        _ => false,
    }
}

/// Variablen, die sich innerhalb einer Reihe von Messungen OHNE
/// Schalterbewegung ändern (Ruhemessung) — oder in einer davon fehlen.
/// Sie sind für den ganzen Lauf „unruhig" und werden ausgeschlossen.
pub fn unruhige(messungen: &[&Werte]) -> BTreeSet<usize> {
    let mut aus = BTreeSet::new();
    let Some(erste) = messungen.first() else {
        return aus;
    };
    for i in 0..erste.len() {
        let bezug = erste[i];
        if bezug.is_none() {
            aus.insert(i);
            continue;
        }
        if messungen
            .iter()
            .skip(1)
            .any(|m| !gleich(bezug, m.get(i).copied().flatten()))
        {
            aus.insert(i);
        }
    }
    aus
}

/// Zwei Messungen einer Stellung (Abstand 0,5 s).
#[derive(Debug, Clone)]
pub struct StellungMessung {
    pub stellung: String,
    pub a: Werte,
    pub b: Werte,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Kandidat {
    pub variable: String,
    /// Wert je Stellung, in Prüfreihenfolge.
    pub werte: Vec<(String, f64)>,
    pub schluesselwort: bool,
    /// Jede Stellung hat einen eigenen Wert.
    pub trennt_alle_stellungen: bool,
    /// Alle Werte ganzzahlig (Rasten 0/1/2 …).
    pub ganzzahlig: bool,
    pub verschiedene_werte: usize,
}

impl Kandidat {
    /// „L:XYZ → OFF=0, AUTO=1, ON=2"
    pub fn zeile(&self) -> String {
        let teile: Vec<String> = self
            .werte
            .iter()
            .map(|(s, w)| format!("{s}={}", wert_text(*w)))
            .collect();
        format!("{} → {}", self.variable, teile.join(", "))
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
pub struct Ergebnis {
    /// Sortiert, beste zuerst.
    pub kandidaten: Vec<Kandidat>,
    /// Variablen, die sich innerhalb EINER Stellung bewegt haben (zwischen
    /// den zwei Messungen) und deshalb für diesen Schalter ausgeschlossen sind.
    pub unruhig_bei_diesem_schalter: Vec<String>,
}

pub fn wert_text(w: f64) -> String {
    if w.fract() == 0.0 && w.abs() < 1e15 {
        format!("{}", w as i64)
    } else {
        let s = format!("{w:.4}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn hat_schluesselwort(name: &str, kw: &[String]) -> bool {
    let n = name.to_ascii_uppercase();
    kw.iter().any(|k| n.contains(&k.to_ascii_uppercase()))
}

/// Kern der Auswertung.
///
/// Kandidat = Variable, die (1) nicht global unruhig ist, (2) in JEDER
/// Stellung bei beiden Messungen denselben Wert hat und (3) sich zwischen den
/// Stellungen unterscheidet.
///
/// Rangfolge (beste zuerst):
/// 1. Name enthält ein Schlüsselwort des Schalters,
/// 2. jede Stellung hat einen eigenen Wert (sonst beschreibt die Variable nur
///    einen Teil des Schalters, z. B. „Strobe leuchtet" statt der Raste),
/// 3. ganzzahlige Werte (Rasten) vor Kommazahlen (Animationen),
/// 4. weniger verschiedene Werte,
/// 5. Name (stabile Reihenfolge).
pub fn auswerten(
    schalter: &Schalter,
    variablen: &[String],
    messungen: &[StellungMessung],
    global_unruhig: &BTreeSet<usize>,
) -> Ergebnis {
    let mut kandidaten = Vec::new();
    let mut unruhig_hier = Vec::new();
    if messungen.len() < 2 {
        return Ergebnis::default();
    }
    'var: for (i, name) in variablen.iter().enumerate() {
        if global_unruhig.contains(&i) {
            continue;
        }
        let mut werte = Vec::with_capacity(messungen.len());
        for m in messungen {
            let a = m.a.get(i).copied().flatten();
            let b = m.b.get(i).copied().flatten();
            if !gleich(a, b) {
                // Fehlt der Wert in beiden Messungen, ist das keine Unruhe,
                // sondern schlicht keine Messung.
                if a.is_some() || b.is_some() {
                    unruhig_hier.push(name.clone());
                }
                continue 'var;
            }
            werte.push((m.stellung.clone(), a.expect("gleich() verlangt Some")));
        }
        let erster = werte[0].1;
        if werte.iter().all(|(_, w)| gleich(Some(*w), Some(erster))) {
            continue;
        }
        let mut verschieden: Vec<f64> = Vec::new();
        for (_, w) in &werte {
            if !verschieden.iter().any(|v| gleich(Some(*v), Some(*w))) {
                verschieden.push(*w);
            }
        }
        kandidaten.push(Kandidat {
            variable: name.clone(),
            schluesselwort: hat_schluesselwort(name, &schalter.schluesselwoerter),
            trennt_alle_stellungen: verschieden.len() == werte.len(),
            ganzzahlig: werte.iter().all(|(_, w)| w.fract() == 0.0),
            verschiedene_werte: verschieden.len(),
            werte,
        });
    }
    kandidaten.sort_by(|x, y| {
        y.schluesselwort
            .cmp(&x.schluesselwort)
            .then(y.trennt_alle_stellungen.cmp(&x.trennt_alle_stellungen))
            .then(y.ganzzahlig.cmp(&x.ganzzahlig))
            .then(x.verschiedene_werte.cmp(&y.verschiedene_werte))
            .then(x.variable.cmp(&y.variable))
    });
    Ergebnis {
        kandidaten,
        unruhig_bei_diesem_schalter: unruhig_hier,
    }
}

/// Indizes der Variablen, die sich irgendwann im Lauf geändert haben
/// (gegenüber der ersten Messung) — nur diese kommen als Rohmessung in den
/// Bericht, nicht alle LVars.
pub fn geaenderte(messungen: &[&Werte]) -> Vec<usize> {
    let Some(erste) = messungen.first() else {
        return Vec::new();
    };
    (0..erste.len())
        .filter(|&i| {
            messungen.iter().any(|m| {
                !gleich(erste[i], m.get(i).copied().flatten())
                    && m.get(i).copied().flatten().is_some()
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skripte;

    fn namen() -> Vec<String> {
        [
            "L:INI_STROBE_SWITCH",   // 0: echter Schalter 0/1/2
            "L:INI_STROBE_LIGHT_ON", // 1: leuchtet ja/nein 0/1/1
            "L:BLINK_ANNUNC",        // 2: blinkt immer (Ruhemessung fängt es)
            "L:KNOB_ANIM_POS",       // 3: Animation 0/0.5/1 ohne Schlüsselwort
            "L:JITTER_IN_STELLUNG",  // 4: ändert sich NUR zwischen zwei Messungen einer Stellung
            "A:LIGHT STROBE",        // 5: Standard-SimVar 0/1/1
            "L:KONSTANT",            // 6: nie verändert
            "L:FEHLT",               // 7: nie geliefert
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    fn m(v: [Option<f64>; 8]) -> Werte {
        v.to_vec()
    }

    #[test]
    fn ruhemessung_findet_blinker_und_fehlende() {
        let r1 = m([
            Some(0.),
            Some(0.),
            Some(0.),
            Some(0.),
            Some(3.),
            Some(0.),
            Some(7.),
            None,
        ]);
        let r2 = m([
            Some(0.),
            Some(0.),
            Some(1.),
            Some(0.),
            Some(3.),
            Some(0.),
            Some(7.),
            None,
        ]);
        let u = unruhige(&[&r1, &r2, &r1]);
        assert_eq!(u.into_iter().collect::<Vec<_>>(), vec![2, 7]);
    }

    #[test]
    fn strobe_schalter_wird_richtig_erkannt_und_rauschen_ausgeschlossen() {
        let schalter = skripte::a380().schalter[0].clone();
        assert_eq!(schalter.stellungen, vec!["OFF", "AUTO", "ON"]);
        let global = [2usize, 7].into_iter().collect();
        let st = |name: &str, a: Werte, b: Werte| StellungMessung {
            stellung: name.into(),
            a,
            b,
        };
        let messungen = vec![
            st(
                "OFF",
                m([
                    Some(0.),
                    Some(0.),
                    Some(1.),
                    Some(0.),
                    Some(10.),
                    Some(0.),
                    Some(7.),
                    None,
                ]),
                m([
                    Some(0.),
                    Some(0.),
                    Some(0.),
                    Some(0.),
                    Some(11.),
                    Some(0.),
                    Some(7.),
                    None,
                ]),
            ),
            st(
                "AUTO",
                m([
                    Some(1.),
                    Some(1.),
                    Some(0.),
                    Some(0.5),
                    Some(12.),
                    Some(1.),
                    Some(7.),
                    None,
                ]),
                m([
                    Some(1.),
                    Some(1.),
                    Some(1.),
                    Some(0.5),
                    Some(13.),
                    Some(1.),
                    Some(7.),
                    None,
                ]),
            ),
            st(
                "ON",
                m([
                    Some(2.),
                    Some(1.),
                    Some(1.),
                    Some(1.),
                    Some(14.),
                    Some(1.),
                    Some(7.),
                    None,
                ]),
                m([
                    Some(2.),
                    Some(1.),
                    Some(0.),
                    Some(1.),
                    Some(15.),
                    Some(1.),
                    Some(7.),
                    None,
                ]),
            ),
        ];
        let e = auswerten(&schalter, &namen(), &messungen, &global);
        let reihenfolge: Vec<&str> = e.kandidaten.iter().map(|k| k.variable.as_str()).collect();
        // Schlüsselwort + trennt alle + ganzzahlig zuerst; dann die übrigen
        // Schlüsselwort-Treffer; die Animation ohne Schlüsselwort zuletzt.
        assert_eq!(
            reihenfolge,
            vec![
                "L:INI_STROBE_SWITCH",
                "A:LIGHT STROBE",
                "L:INI_STROBE_LIGHT_ON",
                "L:KNOB_ANIM_POS"
            ]
        );
        // Die in der Stellung zappelnde Variable ist ausgeschlossen und
        // benannt; der globale Blinker taucht gar nicht erst auf.
        assert!(!reihenfolge.contains(&"L:JITTER_IN_STELLUNG"));
        assert!(!reihenfolge.contains(&"L:BLINK_ANNUNC"));
        assert_eq!(e.unruhig_bei_diesem_schalter, vec!["L:JITTER_IN_STELLUNG"]);
        assert_eq!(
            e.kandidaten[0].zeile(),
            "L:INI_STROBE_SWITCH → OFF=0, AUTO=1, ON=2"
        );
        assert_eq!(
            e.kandidaten[3].zeile(),
            "L:KNOB_ANIM_POS → OFF=0, AUTO=0.5, ON=1"
        );
    }

    #[test]
    fn ganzzahlige_rasten_vor_kommazahlen_bei_gleichem_rang() {
        let schalter = skripte::a330().schalter[7].clone(); // Beacon aus/an
        let namen: Vec<String> = vec!["L:BCN_ANIM".into(), "L:BCN_SW".into()];
        let messungen = vec![
            StellungMessung {
                stellung: "aus".into(),
                a: vec![Some(0.1), Some(0.)],
                b: vec![Some(0.1), Some(0.)],
            },
            StellungMessung {
                stellung: "an".into(),
                a: vec![Some(0.9), Some(1.)],
                b: vec![Some(0.9), Some(1.)],
            },
        ];
        let e = auswerten(&schalter, &namen, &messungen, &BTreeSet::new());
        assert_eq!(e.kandidaten[0].variable, "L:BCN_SW");
        assert_eq!(e.kandidaten[1].variable, "L:BCN_ANIM");
    }

    #[test]
    fn keine_unterschiede_keine_kandidaten() {
        let schalter = skripte::a220().schalter[1].clone();
        let namen: Vec<String> = vec!["L:X".into()];
        let st = StellungMessung {
            stellung: "eingefahren".into(),
            a: vec![Some(1.)],
            b: vec![Some(1.)],
        };
        let e = auswerten(&schalter, &namen, &[st.clone(), st], &BTreeSet::new());
        assert!(e.kandidaten.is_empty());
    }

    #[test]
    fn geaenderte_listet_nur_bewegte_variablen() {
        let a = vec![Some(1.), Some(2.), None];
        let b = vec![Some(1.), Some(3.), None];
        assert_eq!(geaenderte(&[&a, &b]), vec![1]);
    }

    #[test]
    fn werte_text() {
        assert_eq!(wert_text(2.0), "2");
        assert_eq!(wert_text(-1.0), "-1");
        assert_eq!(wert_text(0.25), "0.25");
        assert_eq!(wert_text(1.0 / 3.0), "0.3333");
    }
}
