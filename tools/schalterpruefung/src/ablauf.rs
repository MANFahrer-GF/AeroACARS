//! Geführter Ablauf: Ruhemessung, Skript Schalter für Schalter, freier
//! Modus. Plattformunabhängig — die Messquelle kommt als Funktion herein.

use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::auswertung::{self, StellungMessung, Werte};
use crate::bericht::{self, FreiEreignis, Lauf, SchalterBericht, StellungNachweis};
use crate::skripte::{Schalter, Skript};

/// Gesetzt vom Strg+C-/Fenster-schließen-Handler.
pub static ABBRUCH: AtomicBool = AtomicBool::new(false);
pub const ABGEBROCHEN: &str = "Abgebrochen (Strg+C oder Fenster geschlossen)";

pub const WARTEN_NACH_ENTER: Duration = Duration::from_millis(1500);
pub const ABSTAND_MESSUNGEN: Duration = Duration::from_millis(500);
pub const RUHE_MESSUNGEN: usize = 11; // 5 s bei 0,5 s Abstand

const BREITE: usize = 72;

pub fn linie(z: char) -> String {
    std::iter::repeat_n(z, BREITE).collect()
}

/// Große, klare Anweisung.
pub fn gross(zeilen: &[&str]) {
    println!();
    println!("{}", linie('═'));
    for z in zeilen {
        println!("  {z}");
    }
    println!("{}", linie('═'));
}

pub fn hinweis(t: &str) {
    println!("  {t}");
}

/// Eine Zeile von der Tastatur. `None` bei Abbruch/Eingabeende.
pub fn zeile_lesen(prompt: &str) -> Option<String> {
    print!("  {prompt}");
    let _ = std::io::stdout().flush();
    let mut s = String::new();
    match std::io::stdin().lock().read_line(&mut s) {
        Ok(0) | Err(_) => None,
        Ok(_) => {
            if ABBRUCH.load(Ordering::SeqCst) {
                None
            } else {
                Some(s.trim().to_string())
            }
        }
    }
}

pub fn warten(d: Duration) {
    std::thread::sleep(d);
}

pub type Messen<'a> = dyn FnMut() -> Result<Werte, String> + 'a;

pub struct Ablauf<'a> {
    pub lauf: Arc<Mutex<Lauf>>,
    pub messen: &'a mut Messen<'a>,
}

enum Weiter {
    Naechster,
    Nochmal,
}

fn l(lauf: &Arc<Mutex<Lauf>>) -> std::sync::MutexGuard<'_, Lauf> {
    // Ein vergifteter Lock (Panik woanders) darf das Schreiben des Berichts
    // nicht verhindern.
    lauf.lock().unwrap_or_else(|e| e.into_inner())
}

impl<'a> Ablauf<'a> {
    fn eine_messung(&mut self, anlass: &str) -> Result<(usize, Vec<usize>), String> {
        if ABBRUCH.load(Ordering::SeqCst) {
            return Err(ABGEBROCHEN.into());
        }
        let w = (self.messen)()?;
        Ok(l(&self.lauf).messpunkt(anlass, w))
    }

    /// 1,5 s warten, dann zwei Messungen im Abstand von 0,5 s.
    fn mess_paar(&mut self, anlass: &str) -> Result<(usize, usize, Vec<usize>), String> {
        hinweis("… messe, bitte jetzt nichts bewegen …");
        warten(WARTEN_NACH_ENTER);
        let (a, mut neu) = self.eine_messung(&format!("{anlass} (1)"))?;
        warten(ABSTAND_MESSUNGEN);
        let (b, neu_b) = self.eine_messung(&format!("{anlass} (2)"))?;
        neu.extend(neu_b);
        Ok((a, b, neu))
    }

    pub fn ruhemessung(&mut self) -> Result<(), String> {
        gross(&[
            "RUHEMESSUNG — bitte 5 Sekunden NICHTS anfassen.",
            "Das Programm merkt sich dabei, welche Werte von allein zappeln.",
        ]);
        let start = l(&self.lauf).messpunkte.len();
        for i in 0..RUHE_MESSUNGEN {
            self.eine_messung(&format!("Ruhe {}", i + 1))?;
            print!(".");
            let _ = std::io::stdout().flush();
            if i + 1 < RUHE_MESSUNGEN {
                warten(ABSTAND_MESSUNGEN);
            }
        }
        println!();
        let mut lauf = l(&self.lauf);
        let ende = lauf.messpunkte.len();
        lauf.ruhe_abschliessen(start, ende);
        hinweis(&format!(
            "Fertig. {} Werte zappeln von allein und werden ignoriert.",
            lauf.global_unruhig.len()
        ));
        Ok(())
    }

    /// Das ganze Skript. Am Ende automatisch Angebot freier Modus.
    pub fn skript(&mut self, skript: &Skript) -> Result<(), String> {
        let n = skript.schalter.len();
        for (i, s) in skript.schalter.iter().enumerate() {
            let mut durchgang = 1;
            loop {
                let bericht = self.schalter_messen(s, i + 1, n, durchgang)?;
                self.ergebnis_zeigen(&bericht);
                l(&self.lauf).schalter_ausgewertet(bericht);
                let _ = l(&self.lauf).zwischenstand();
                match self.nach_schalter_menue()? {
                    Weiter::Naechster => break,
                    Weiter::Nochmal => durchgang += 1,
                }
            }
        }
        gross(&["Das Skript ist fertig — danke!"]);
        loop {
            let e = zeile_lesen(
                "F = freier Modus (weitere Schalter prüfen), Enter = beenden und Bericht schreiben: ",
            )
            .ok_or(ABGEBROCHEN)?;
            match e.to_ascii_uppercase().as_str() {
                "" => return Ok(()),
                "F" => self.frei_modus()?,
                _ => hinweis("Bitte nur F oder Enter."),
            }
        }
    }

    fn nach_schalter_menue(&mut self) -> Result<Weiter, String> {
        loop {
            let e =
                zeile_lesen("Enter = weiter   W = diesen Schalter nochmal   F = freier Modus: ")
                    .ok_or(ABGEBROCHEN)?;
            match e.to_ascii_uppercase().as_str() {
                "" => return Ok(Weiter::Naechster),
                "W" => return Ok(Weiter::Nochmal),
                "F" => {
                    self.frei_modus()?;
                    hinweis("Zurück im Skript.");
                }
                _ => hinweis("Bitte nur Enter, W oder F."),
            }
        }
    }

    fn stellung_aufnehmen(
        &mut self,
        s: &Schalter,
        stellung: String,
    ) -> Result<(StellungNachweis, StellungMessung), String> {
        let anlass = format!("{} = {}", s.name, stellung);
        let (a, b, neu) = self.mess_paar(&anlass)?;
        let (wa, wb) = {
            let mut lauf = l(&self.lauf);
            lauf.stellung_gemessen(&s.name, &stellung, a, b, &neu);
            (
                lauf.messpunkte[a - 1].werte.clone(),
                lauf.messpunkte[b - 1].werte.clone(),
            )
        };
        let zappler = wa
            .iter()
            .zip(&wb)
            .filter(|(x, y)| x.is_some() && !auswertung::gleich(**x, **y))
            .count();
        hinweis(&format!(
            "OK, „{stellung}“ gemessen (Messpunkte #{a}/#{b}, {zappler} Werte zappelten)."
        ));
        Ok((
            StellungNachweis {
                stellung: stellung.clone(),
                messpunkt_a: a,
                messpunkt_b: b,
            },
            StellungMessung {
                stellung,
                a: wa,
                b: wb,
            },
        ))
    }

    fn schalter_messen(
        &mut self,
        s: &Schalter,
        nr: usize,
        gesamt: usize,
        durchgang: usize,
    ) -> Result<SchalterBericht, String> {
        let mut messungen: Vec<StellungMessung> = Vec::new();
        let mut nachweise: Vec<StellungNachweis> = Vec::new();
        let mut uebersprungen = false;
        let kopf = format!("SCHALTER {nr} von {gesamt}: {}", s.name);

        if s.ist_frei() {
            gross(&[
                &kopf,
                "Stell den Schalter nacheinander in jede Stellung, die er hat.",
                "Nach jeder Stellung: kurze Bezeichnung tippen (z. B. AUS) und Enter —",
                "oder nur Enter, dann nummeriert das Programm selbst.",
                "X + Enter = dieser Schalter ist fertig (oder fehlt: dann wird er übersprungen).",
            ]);
            loop {
                let k = messungen.len() + 1;
                println!();
                let e = zeile_lesen(&format!(
                    ">>> {} in Stellung {k} bringen, Bezeichnung + Enter (X = fertig): ",
                    s.name
                ))
                .ok_or(ABGEBROCHEN)?;
                if e.eq_ignore_ascii_case("X") {
                    if messungen.len() < 2 {
                        uebersprungen = true;
                    }
                    break;
                }
                let name = if e.is_empty() {
                    format!("Stellung {k}")
                } else {
                    e
                };
                let (n, m) = self.stellung_aufnehmen(s, name)?;
                nachweise.push(n);
                messungen.push(m);
            }
        } else {
            for (j, st) in s.stellungen.iter().enumerate() {
                gross(&[
                    &format!("{kopf}   —   Stellung {} von {}", j + 1, s.stellungen.len()),
                    "",
                    &format!(">>> Stell jetzt {} auf {} und drück Enter.", s.name, st),
                    "",
                    "(S + Enter = diesen Schalter überspringen)",
                ]);
                let e = zeile_lesen("Enter: ").ok_or(ABGEBROCHEN)?;
                if e.eq_ignore_ascii_case("S") {
                    uebersprungen = true;
                    break;
                }
                let (n, m) = self.stellung_aufnehmen(s, st.clone())?;
                nachweise.push(n);
                messungen.push(m);
            }
        }

        let ergebnis = if uebersprungen {
            Default::default()
        } else {
            let lauf = l(&self.lauf);
            auswertung::auswerten(s, &lauf.variablen, &messungen, &lauf.global_unruhig)
        };
        Ok(SchalterBericht {
            schalter: s.name.clone(),
            durchgang,
            ersetzt: false,
            uebersprungen,
            stellungen: nachweise,
            ergebnis,
        })
    }

    fn ergebnis_zeigen(&self, b: &SchalterBericht) {
        println!();
        println!("{}", linie('─'));
        if b.uebersprungen {
            hinweis(&format!("{}: übersprungen.", b.schalter));
        } else if b.ergebnis.kandidaten.is_empty() {
            hinweis(&format!(
                "{}: KEINE Variable gefunden, die diesem Schalter folgt.",
                b.schalter
            ));
            hinweis("Tipp: W drücken und den Schalter nochmal langsamer durchschalten.");
        } else {
            hinweis(&format!("Gefunden für {}:", b.schalter));
            for k in b.ergebnis.kandidaten.iter().take(5) {
                hinweis(&format!("   {}", k.zeile()));
            }
            if b.ergebnis.kandidaten.len() > 5 {
                hinweis(&format!(
                    "   (… {} weitere stehen im Bericht)",
                    b.ergebnis.kandidaten.len() - 5
                ));
            }
        }
        if !b.ergebnis.unruhig_bei_diesem_schalter.is_empty() {
            hinweis(&format!(
                "{} Werte zappelten während einer Stellung und wurden ignoriert.",
                b.ergebnis.unruhig_bei_diesem_schalter.len()
            ));
        }
        println!("{}", linie('─'));
    }

    /// Freier Modus: beliebige Schalter umlegen, sofort sehen, was sich tut.
    pub fn frei_modus(&mut self) -> Result<(), String> {
        gross(&[
            "FREIER MODUS",
            "Leg jetzt einen beliebigen Schalter um und drück Enter.",
            "Du kannst vorher eine Bezeichnung tippen (z. B. „Wing Lights an“).",
            "Z + Enter = zurück.",
        ]);
        loop {
            println!();
            let e = zeile_lesen(">>> Schalter umlegen, dann (Bezeichnung +) Enter, Z = zurück: ")
                .ok_or(ABGEBROCHEN)?;
            if e.eq_ignore_ascii_case("Z") {
                return Ok(());
            }
            let vorher = l(&self.lauf).messpunkte.len();
            if vorher == 0 {
                return Err("Keine vorherige Messung".into());
            }
            let nr = l(&self.lauf).frei.len() + 1;
            let bezeichnung = if e.is_empty() {
                format!("Frei {nr}")
            } else {
                e
            };
            let (a, b, neu) = self.mess_paar(&format!("Frei: {bezeichnung}"))?;
            let mut lauf = l(&self.lauf);
            let aend = lauf.frei_aenderungen(vorher, a, b);
            if aend.is_empty() {
                hinweis("Keine ruhige Variable hat sich seit der letzten Messung geändert.");
            } else {
                hinweis(&format!(
                    "{} Änderung(en) seit der letzten Messung:",
                    aend.len()
                ));
                for x in aend.iter().take(25) {
                    hinweis(&format!("   {}", bericht::aenderung_zeile(x)));
                }
                if aend.len() > 25 {
                    hinweis(&format!(
                        "   (… {} weitere stehen im Bericht)",
                        aend.len() - 25
                    ));
                }
            }
            lauf.frei_ereignis(
                FreiEreignis {
                    bezeichnung,
                    messpunkt: b,
                    aenderungen: aend,
                },
                &neu,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bericht::LaufInfo;

    #[test]
    fn mess_paar_und_ruhe_mit_kuenstlicher_quelle() {
        // Kurzer Test ohne Tastatur: nur die Messmechanik.
        let o = std::env::temp_dir().join(format!("sp-ablauf-{}", std::process::id()));
        std::fs::create_dir_all(&o).unwrap();
        let lauf = Arc::new(Mutex::new(Lauf::neu(
            LaufInfo::default(),
            vec!["L:A".into(), "L:ZAPPEL".into()],
            o.clone(),
        )));
        let mut z = 0.0;
        let mut quelle = || -> Result<Werte, String> {
            z += 1.0;
            Ok(vec![Some(0.0), Some(z)])
        };
        let mut ab = Ablauf {
            lauf: lauf.clone(),
            messen: &mut quelle,
        };
        // Direkt statt `ruhemessung()` (die wartet 5 s): zwei Messpunkte.
        ab.eine_messung("Ruhe 1").unwrap();
        ab.eine_messung("Ruhe 2").unwrap();
        l(&lauf).ruhe_abschliessen(0, 2);
        assert_eq!(
            l(&lauf).global_unruhig.iter().copied().collect::<Vec<_>>(),
            vec![1]
        );
        let _ = std::fs::remove_dir_all(o);
    }
}
