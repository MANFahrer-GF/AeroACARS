//! Anflug-Urteil: STABLE / PARTIAL / UNSTABLE — eine Quelle.
//!
//! # Warum das hier steht
//!
//! Das Urteil gab es bisher zweimal. Die Karte „Anflug-Stabilität" im
//! Landungs-Tab rechnete es aus sieben Kacheln (`ApproachStabilityCard.tsx`),
//! das Backend meldete daneben ein eigenes `stable_at_gate` mit lockereren
//! Grenzen — und der Stabilitäts-Teilscore kannte keines von beiden, er
//! rechnete nur aus der Streuung (σ).
//!
//! Folge (DLH2248, 01.10.2026): Backend `stable_at_gate = true`, Karte
//! **PARTIAL**, Stabilität **100 Punkte „sehr stabil"**. Im Bestand
//! (1324 Landungen) trugen 259 volle Stabilitätspunkte, obwohl die Karte
//! nicht STABLE zeigte, 70 davon sogar UNSTABLE.
//!
//! # Score-Version 19 (05.10.2026): Gleitpfad statt Sinkraten-Abweichung
//!
//! Bis dahin zaehlten zwei Sinkraten-Abweichungen gegen eine ideale 3°-
//! Sinkrate mit — eine davon als SPITZE eines einzigen Messpunkts unter
//! 500 ft. Schon die typische Landung lag im „mittel"-Band (Median 125 bzw.
//! 250 fpm); nur 25 % aller Anfluege galten als stabil. GSG1709 flog den
//! Gleitpfad auf 0,24 Dots genau und war „teilweise stabil", weil EIN
//! Messpunkt 212 statt 200 fpm abwich. Thomas: „ungerecht, nicht erklaert".
//!
//! Jetzt prueft das Gate, was die Flight Safety Foundation unter einem
//! stabilisierten Anflug versteht — ab 1000 ft: auf dem Gleitpfad, ruhige
//! Fahrt und Querneigung, kein anhaltend zu starkes Sinken, Landekonfi-
//! guration. Jede Pruefung beruht auf einem Durchschnitt oder einer Dauer,
//! nie auf einem einzelnen Ausreisser. [`anflug_pruefung`] liefert die
//! Liste mit Messwert und Grenze, damit jede Oberflaeche den Grund nennt.
//!
//! Die Grenzen unten sind 1:1 die der Karte und des Hilfe-Fensters; wer hier
//! eine Grenze aendert, muss `ApproachStabilityCard.tsx` mitziehen.

use serde::{Deserialize, Serialize};

/// Die Messwerte des Anflug-Gates. `None` = nicht gemessen.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AnflugWerte {
    pub vs_jerk_fpm: Option<f32>,
    pub bank_stddev_deg: Option<f32>,
    pub ias_stddev_kt: Option<f32>,
    pub excessive_sink: Option<bool>,
    pub stable_config: Option<bool>,
    /// Durchschnittliche Abweichung vom Gleitpfad in Dots, 1000–200 ft
    /// (`anflug_gleitpfad.gesamt.mittel_abs_dots`). `None` ohne Gleitpfad-
    /// bezug (Sichtanflug ohne Navdaten) — die Pruefung entfaellt dann.
    pub gleitpfad_dots: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnflugUrteil {
    Stable,
    Partial,
    Unstable,
}

/// Stufe einer Pruefung.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stufe {
    Gut,
    Mittel,
    Schlecht,
}

/// Eine Pruefung des Gates mit Messwert und Grenze — fuer die Anzeige
/// „Gleitpfad 1,7 Dots (gut unter 1)". Schluessel sprachneutral, Texte in
/// den Oberflaechen (`landing.gate.<key>`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pruefpunkt {
    /// `gleitpfad`, `fahrt`, `querneigung`, `ruck`, `sinken`, `konfiguration`
    pub key: String,
    pub stufe: Stufe,
    /// Messwert (Zahl) — bei Ja/Nein-Pruefungen `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wert: Option<f32>,
    /// Grenze fuer „gut" (Messwert darunter) — bei Ja/Nein `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gut_unter: Option<f32>,
}

fn bereich(key: &str, v: Option<f32>, gut_unter: f32, mittel_unter: f32) -> Option<Pruefpunkt> {
    let x = v.filter(|x| x.is_finite())?;
    let stufe = if x < gut_unter {
        Stufe::Gut
    } else if x < mittel_unter {
        Stufe::Mittel
    } else {
        Stufe::Schlecht
    };
    Some(Pruefpunkt {
        key: key.to_string(),
        stufe,
        wert: Some(x),
        gut_unter: Some(gut_unter),
    })
}

fn wahrheit(key: &str, v: Option<bool>, gut_wert: bool) -> Option<Pruefpunkt> {
    let x = v?;
    Some(Pruefpunkt {
        key: key.to_string(),
        stufe: if x == gut_wert {
            Stufe::Gut
        } else {
            Stufe::Schlecht
        },
        wert: None,
        gut_unter: None,
    })
}

/// Alle gemessenen Pruefungen, in fester Reihenfolge (Gleitpfad zuerst —
/// er ist das, was „stabil" im Kern meint). Nicht gemessene fehlen.
pub fn anflug_pruefung(w: &AnflugWerte) -> Vec<Pruefpunkt> {
    [
        bereich("gleitpfad", w.gleitpfad_dots, 1.0, 2.0),
        bereich("fahrt", w.ias_stddev_kt, 5.0, 8.0),
        bereich("querneigung", w.bank_stddev_deg, 3.0, 6.0),
        bereich("ruck", w.vs_jerk_fpm, 100.0, 200.0),
        wahrheit("sinken", w.excessive_sink, false),
        wahrheit("konfiguration", w.stable_config, true),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// `None`, wenn kein einziger Wert gemessen wurde (Altbestand) — dann
/// gibt es kein Urteil und folglich auch keinen Deckel.
pub fn anflug_urteil(w: &AnflugWerte) -> Option<AnflugUrteil> {
    urteil_aus(&anflug_pruefung(w))
}

/// Urteil aus einer Pruefliste: alles gut → STABLE; zwei schlecht oder
/// drei ausserhalb von „gut" → UNSTABLE; sonst PARTIAL.
pub fn urteil_aus(pruefung: &[Pruefpunkt]) -> Option<AnflugUrteil> {
    if pruefung.is_empty() {
        return None;
    }
    let schlecht = pruefung
        .iter()
        .filter(|p| p.stufe == Stufe::Schlecht)
        .count();
    let mittel = pruefung.iter().filter(|p| p.stufe == Stufe::Mittel).count();
    Some(if schlecht == 0 && mittel == 0 {
        AnflugUrteil::Stable
    } else if schlecht >= 2 || schlecht + mittel >= 3 {
        AnflugUrteil::Unstable
    } else {
        AnflugUrteil::Partial
    })
}

/// Höchstpunkte der Stabilitätsachse je Urteil. `None` = kein Deckel.
pub fn punkte_deckel(u: Option<AnflugUrteil>) -> Option<u8> {
    match u {
        Some(AnflugUrteil::Partial) => Some(80),
        Some(AnflugUrteil::Unstable) => Some(45),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stabil() -> AnflugWerte {
        AnflugWerte {
            vs_jerk_fpm: Some(40.0),
            bank_stddev_deg: Some(1.0),
            ias_stddev_kt: Some(2.0),
            excessive_sink: Some(false),
            stable_config: Some(true),
            gleitpfad_dots: Some(0.3),
        }
    }

    #[test]
    fn alles_gruen_ist_stable() {
        assert_eq!(anflug_urteil(&stabil()), Some(AnflugUrteil::Stable));
    }

    /// GSG1709 (05.10.2026): Gleitpfad 0,24 Dots, alles ruhig. Unter Score-
    /// Version 18 „teilweise stabil" wegen EINES Messpunkts (212 fpm statt
    /// 200 Sinkraten-Abweichung) — jetzt stabil.
    #[test]
    fn gsg1709_ist_stable() {
        let w = AnflugWerte {
            vs_jerk_fpm: Some(23.6),
            bank_stddev_deg: Some(0.87),
            ias_stddev_kt: Some(3.39),
            excessive_sink: Some(false),
            stable_config: Some(true),
            gleitpfad_dots: Some(0.24),
        };
        assert_eq!(anflug_urteil(&w), Some(AnflugUrteil::Stable));
    }

    /// QAF434 (05.10.2026): im Schnitt 1,69 Dots neben dem Gleitpfad, sonst
    /// ruhig — teilweise stabil, und die Pruefliste nennt den Grund.
    #[test]
    fn qaf434_ist_partial_wegen_gleitpfad() {
        let w = AnflugWerte {
            vs_jerk_fpm: Some(21.4),
            bank_stddev_deg: Some(1.51),
            ias_stddev_kt: Some(0.97),
            excessive_sink: Some(false),
            stable_config: Some(true),
            gleitpfad_dots: Some(1.69),
        };
        assert_eq!(anflug_urteil(&w), Some(AnflugUrteil::Partial));
        let auffaellig: Vec<_> = anflug_pruefung(&w)
            .into_iter()
            .filter(|p| p.stufe != Stufe::Gut)
            .collect();
        assert_eq!(auffaellig.len(), 1);
        assert_eq!(auffaellig[0].key, "gleitpfad");
        assert_eq!(auffaellig[0].stufe, Stufe::Mittel);
        assert_eq!(auffaellig[0].gut_unter, Some(1.0));
    }

    /// Ab 2 Dots ist der Gleitpfad „schlecht" (GSG410: bis 3 Dots).
    #[test]
    fn gleitpfad_grenzen() {
        let stufe = |d: f32| {
            let mut w = stabil();
            w.gleitpfad_dots = Some(d);
            anflug_pruefung(&w)[0].stufe
        };
        assert_eq!(stufe(0.99), Stufe::Gut);
        assert_eq!(stufe(1.0), Stufe::Mittel);
        assert_eq!(stufe(1.99), Stufe::Mittel);
        assert_eq!(stufe(2.0), Stufe::Schlecht);
    }

    #[test]
    fn zwei_schlechte_oder_drei_auffaellige_sind_unstable() {
        let mut w = stabil();
        w.excessive_sink = Some(true);
        w.stable_config = Some(false);
        assert_eq!(anflug_urteil(&w), Some(AnflugUrteil::Unstable));
        let mut w = stabil();
        w.vs_jerk_fpm = Some(150.0);
        w.bank_stddev_deg = Some(4.0);
        w.ias_stddev_kt = Some(6.0);
        assert_eq!(anflug_urteil(&w), Some(AnflugUrteil::Unstable));
    }

    #[test]
    fn nichts_gemessen_gibt_kein_urteil_und_keinen_deckel() {
        let u = anflug_urteil(&AnflugWerte::default());
        assert_eq!(u, None);
        assert_eq!(punkte_deckel(u), None);
    }

    #[test]
    fn deckel_je_urteil() {
        assert_eq!(punkte_deckel(Some(AnflugUrteil::Stable)), None);
        assert_eq!(punkte_deckel(Some(AnflugUrteil::Partial)), Some(80));
        assert_eq!(punkte_deckel(Some(AnflugUrteil::Unstable)), Some(45));
    }

    /// Ohne Gleitpfadbezug (Sichtanflug ohne Navdaten) entfaellt die
    /// Pruefung — sie zaehlt weder als gut noch als schlecht.
    #[test]
    fn fehlender_gleitpfad_macht_nichts_schlechter() {
        let mut w = stabil();
        w.gleitpfad_dots = None;
        assert_eq!(anflug_urteil(&w), Some(AnflugUrteil::Stable));
        assert!(anflug_pruefung(&w).iter().all(|p| p.key != "gleitpfad"));
    }
}
