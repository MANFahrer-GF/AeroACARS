//! Ist jede öffentliche Funktion der Landebewertung auch angeschlossen?
//!
//! # Warum der Compiler das nicht meldet
//!
//! `cargo build` warnt bei `never used` — aber nur für **private** Items.
//! Ein `pub fn` in einem Crate könnte von aussen benutzt werden, also
//! schweigt der Compiler. Genau dort ist die Lücke:
//!
//! | Funktion | Zustand | gefunden |
//! |---|---|---|
//! | `ausfahrten_fuer_bahn` | 7 eigene Tests, nie aufgerufen | QS-Runde 1 |
//! | `aussenkante_halb_aus_spur` | Bewertung rechnete ohne sie | QS-Runde 15 |
//! | `aussenkante_halb_m` | von der neuen Fassung abgelöst | QS-Runde 17 |
//!
//! Dreimal dieselbe Klasse in einer QS. Sie ist besonders tückisch, weil
//! die Funktion **getestet** ist: Die eigenen Tests sind grün, die Doku
//! liest sich sauber, und niemand ruft sie an.
//!
//! Diese Prüfung liest den Quelltext. Das ist grob — sie erkennt keine
//! Aufrufe über Makros oder Traits —, aber sie hätte alle drei Fälle
//! gefunden, und das ist der Massstab.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Dateien, deren öffentliche Schnittstelle angeschlossen sein muss.
///
/// Bewusst eine Liste und kein Verzeichnisbaum: Diese Module gehören zur
/// Landebewertung, und für sie gilt „gebaut heisst benutzt". Ein Crate
/// mit echter Aussenschnittstelle (`aeroacars-mqtt`, `storage`) hat
/// zurecht `pub`-Elemente, die intern niemand ruft.
const MODULE: &[&str] = &[
    "crates/landing-scoring/src/spurweite.rs",
    "crates/landing-scoring/src/belag.rs",
    "crates/landing-scoring/src/sub_bahndisziplin.rs",
    "crates/landing-scoring/src/sub_touchdown_point.rs",
    "src/ausfahrten.rs",
    "src/fahrwerk.rs",
];

/// Wo nach Aufrufen gesucht wird.
const SUCHPFADE: &[&str] = &["src", "crates"];

/// Die Datei ohne ihre Testmodule.
///
/// # Warum das Klammern zählt und nicht am ersten `#[cfg(test)]` abschneidet
///
/// Die erste Fassung tat genau das — mit der Begründung, Testmodule stünden
/// am Ende. In `src/lib.rs` stimmt das nicht: Dort liegen über vierzig
/// Testmodule zwischen dem produktiven Code, das erste lange vor
/// `bahn_felder`. Abgeschnitten wurde damit die halbe Datei, und die Prüfung
/// meldete Funktionen als tot, die zwei Zeilen weiter gerufen werden.
///
/// Eine Prüfung, die falschen Alarm schlägt, wird abgeschaltet — das ist
/// derselbe Schaden wie eine, die nie anschlägt.
fn produktiv(text: &str) -> String {
    let mut raus = String::with_capacity(text.len());
    let mut zeilen = text.lines().peekable();
    while let Some(z) = zeilen.next() {
        if z.trim_start().starts_with("#[cfg(test)]") {
            // Bis zur schliessenden Klammer des Testmoduls überspringen.
            let mut tiefe = 0i32;
            let mut begonnen = false;
            for t in zeilen.by_ref() {
                tiefe += t.matches('{').count() as i32;
                tiefe -= t.matches('}').count() as i32;
                if t.contains('{') {
                    begonnen = true;
                }
                if begonnen && tiefe <= 0 {
                    break;
                }
            }
            continue;
        }
        raus.push_str(z);
        raus.push('\n');
    }
    raus
}

fn wurzel() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Alle `.rs`-Dateien unter einem Pfad.
fn rust_dateien(pfad: &Path, raus: &mut Vec<PathBuf>) {
    let Ok(eintraege) = fs::read_dir(pfad) else {
        return;
    };
    for e in eintraege.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_dateien(&p, raus);
        } else if p.extension().is_some_and(|x| x == "rs") {
            raus.push(p);
        }
    }
}

/// Die Namen der `pub fn` einer Datei.
fn oeffentliche_funktionen(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|z| z.strip_prefix("pub fn "))
        .filter_map(|rest| {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

#[test]
fn jede_oeffentliche_funktion_wird_auch_gerufen() {
    let w = wurzel();
    let mut dateien = Vec::new();
    for p in SUCHPFADE {
        rust_dateien(&w.join(p), &mut dateien);
    }
    // Der gesamte Quelltext, je Datei — damit die eigene Datei beim
    // Suchen ausgenommen werden kann.
    let inhalte: Vec<(PathBuf, String)> = dateien
        .iter()
        .filter_map(|p| fs::read_to_string(p).ok().map(|t| (p.clone(), t)))
        .collect();

    let mut tot: BTreeSet<String> = BTreeSet::new();

    for modul in MODULE {
        let pfad = w.join(modul);
        let text =
            fs::read_to_string(&pfad).unwrap_or_else(|e| panic!("{modul} nicht lesbar: {e}"));

        for name in oeffentliche_funktionen(&text) {
            // Aufrufe in ANDEREN Dateien. Die eigene zählt nicht: Dort
            // stehen die Definition und die eigenen Tests, und beides
            // beweist nichts über den Anschluss.
            //
            // Ausnahme: eine Funktion, die von einer anderen `pub fn`
            // derselben Datei gerufen wird, ist angeschlossen, sobald
            // JENE es ist — das deckt `spurweite_aus_acf` ab, das nur
            // `spurweite_aus_paket` benutzt.
            let anderswo = inhalte.iter().any(|(p, t)| {
                // NUR der produktive Teil. Ein Aufruf im Testmodul einer
                // anderen Datei beweist nichts über den Anschluss — er ist
                // dieselbe Art Beleg wie die eigenen Tests der Funktion.
                //
                // Die erste Fassung dieser Prüfung liess ihn zu, und die
                // Gegenprobe blieb grün: Der Verdrahtungstest aus einer
                // früheren QS-Runde rief die Funktion, also galt sie als
                // angeschlossen — obwohl die Bewertung sie nicht mehr
                // benutzte. Eine Prüfung, die dabei grün bleibt, prüft
                // nichts.
                p != &pfad && produktiv(t).contains(&format!("{name}("))
            });
            let intern_ausserhalb_tests = produktiv(&text).matches(&format!("{name}(")).count() > 1;
            if !anderswo && !intern_ausserhalb_tests {
                tot.insert(format!("{name}  ({modul})"));
            }
        }
    }

    assert!(
        tot.is_empty(),
        "Diese Funktionen sind gebaut, getestet — und werden nirgends \
         gerufen.\nDer Compiler meldet das bei `pub fn` nicht.\n  {}",
        tot.into_iter().collect::<Vec<_>>().join("\n  ")
    );
}
