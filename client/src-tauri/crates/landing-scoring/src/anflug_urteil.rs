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
//! Die Grenzen unten sind 1:1 die der Karte und des Hilfe-Fensters. Die
//! Karte rechnet sie noch selbst; wer hier eine Grenze ändert, muss
//! `ApproachStabilityCard.tsx` mitziehen (Kommentar dort verweist hierher).

use serde::{Deserialize, Serialize};

/// Die sieben Messwerte des Anflug-Gates. `None` = nicht gemessen.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AnflugWerte {
    pub vs_jerk_fpm: Option<f32>,
    pub bank_stddev_deg: Option<f32>,
    pub ias_stddev_kt: Option<f32>,
    pub excessive_sink: Option<bool>,
    pub stable_config: Option<bool>,
    pub vs_deviation_fpm: Option<f32>,
    pub max_vs_deviation_below_500_fpm: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnflugUrteil {
    Stable,
    Partial,
    Unstable,
}

#[derive(Clone, Copy, PartialEq)]
enum Stufe {
    Gut,
    Mittel,
    Schlecht,
    Fehlt,
}

fn bereich(v: Option<f32>, gut_unter: f32, mittel_unter: f32) -> Stufe {
    match v {
        Some(x) if x.is_finite() => {
            if x < gut_unter {
                Stufe::Gut
            } else if x < mittel_unter {
                Stufe::Mittel
            } else {
                Stufe::Schlecht
            }
        }
        _ => Stufe::Fehlt,
    }
}

fn wahrheit(v: Option<bool>, gut_wert: bool) -> Stufe {
    match v {
        None => Stufe::Fehlt,
        Some(x) if x == gut_wert => Stufe::Gut,
        Some(_) => Stufe::Schlecht,
    }
}

/// `None`, wenn kein einziger Wert gemessen wurde (Altbestand) — dann
/// gibt es kein Urteil und folglich auch keinen Deckel.
pub fn anflug_urteil(w: &AnflugWerte) -> Option<AnflugUrteil> {
    let stufen = [
        bereich(w.vs_jerk_fpm, 100.0, 200.0),
        bereich(w.bank_stddev_deg, 3.0, 6.0),
        bereich(w.ias_stddev_kt, 5.0, 8.0),
        wahrheit(w.excessive_sink, false),
        wahrheit(w.stable_config, true),
        bereich(w.vs_deviation_fpm, 100.0, 200.0),
        bereich(w.max_vs_deviation_below_500_fpm, 200.0, 400.0),
    ];
    if stufen.iter().all(|s| *s == Stufe::Fehlt) {
        return None;
    }
    let schlecht = stufen.iter().filter(|s| **s == Stufe::Schlecht).count();
    let mittel = stufen.iter().filter(|s| **s == Stufe::Mittel).count();
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
            vs_deviation_fpm: Some(50.0),
            max_vs_deviation_below_500_fpm: Some(100.0),
        }
    }

    #[test]
    fn alles_gruen_ist_stable() {
        assert_eq!(anflug_urteil(&stabil()), Some(AnflugUrteil::Stable));
    }

    /// DLH2248: V/S-Abweichung 127 (mittel) + Max-Abweichung 476 (schlecht).
    #[test]
    fn dlh2248_ist_partial() {
        let w = AnflugWerte {
            vs_jerk_fpm: Some(48.9),
            bank_stddev_deg: Some(0.75),
            ias_stddev_kt: Some(3.49),
            excessive_sink: Some(false),
            stable_config: Some(true),
            vs_deviation_fpm: Some(127.4),
            max_vs_deviation_below_500_fpm: Some(476.0),
        };
        assert_eq!(anflug_urteil(&w), Some(AnflugUrteil::Partial));
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

    /// Fehlende Werte zaehlen weder als gut noch als schlecht — wie die Karte.
    #[test]
    fn fehlende_werte_machen_nichts_schlechter() {
        let mut w = stabil();
        w.vs_deviation_fpm = None;
        w.max_vs_deviation_below_500_fpm = None;
        assert_eq!(anflug_urteil(&w), Some(AnflugUrteil::Stable));
    }
}
