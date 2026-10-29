//! v3 Etappe 3 (P-4.30): das Crate als Bewertungsmodul — JSON hinein, JSON
//! heraus. [`bewerten_json`] ist plattformunabhaengig (nativ getestet); die
//! Exporte `alloc`/`bewerte` stehen im Huell-Crate `bewertung-wasm` (cdylib).
//!
//! Die Fassung ([`FASSUNG`]) steht im Modul und kommt mit der Ausgabe
//! (P-4.50): Fortfuehrung von `SCORE_ALGORITHMUS_VERSION = 26`, erste
//! 3.0-Fassung = 27.

use serde::{Deserialize, Serialize};

use crate::{
    aggregate_master_score, anflug_abzug, compute_sub_scores, landungs_abzug, master_deckel_wirksam,
    LandingScoringInput, SubScoreEntry,
};

/// Fassung der Bewertungsregeln dieses Moduls.
pub const FASSUNG: u32 = 27;

/// Eingabe-Dokument (P-4.04): heute die verdichtete `LandingScoringInput`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Eingabe {
    pub landung: LandingScoringInput,
}

/// Ausgabe (P-4.20). Wort, Buchstabe, Anflug-Urteil und die Farbgrenzen
/// kommen nur von hier (P-4.21) — Client und Webapp rechnen sie nicht nach.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ausgabe {
    pub fassung: u32,
    pub teilnoten: Vec<SubScoreEntry>,
    pub gesamt: Option<u8>,
    pub deckel: Option<String>,
    pub landungs_abzug: Option<(String, u8)>,
    pub anflug_abzug: Option<(String, u8)>,
    /// `smooth`/`acceptable`/`firm`/`hard`/`severe` zur Gesamtnote.
    pub gesamt_wort: Option<String>,
    /// `A+` … `F` zur Gesamtnote.
    pub gesamt_buchstabe: Option<String>,
    /// `stable`/`partial`/`unstable`; `None` ohne Gate-Pruefung.
    pub anflug_urteil: Option<String>,
    /// Farbgrenzen der Forensik-Kacheln (Sinkrate fpm, G).
    pub grenzen: Grenzen,
}

/// Farbgrenzen der Anzeige — dieselben Werte wie die Teilnoten.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Grenzen {
    pub vs_fpm: [f32; 4],
    pub g: [f32; 4],
}

/// Wort zur Gesamtnote (Leiter 88/75/50/15).
pub fn gesamt_wort(gesamt: u8) -> &'static str {
    if gesamt >= 88 {
        "smooth"
    } else if gesamt >= 75 {
        "acceptable"
    } else if gesamt >= 50 {
        "firm"
    } else if gesamt >= 15 {
        "hard"
    } else {
        "severe"
    }
}

/// Buchstabe zur Gesamtnote (95/88/82/75/65/50).
pub fn gesamt_buchstabe(gesamt: i32) -> &'static str {
    match gesamt {
        95..=100 => "A+",
        88..=94 => "A",
        82..=87 => "B+",
        75..=81 => "B",
        65..=74 => "C",
        50..=64 => "D",
        _ => "F",
    }
}

/// Anflug-Urteil aus der Stabilitaetsachse: Marke (`warning`, sonst
/// `reason`) `anflug_unstable`/`anflug_partial`, sonst stabil; ohne
/// Gate-Pruefung kein Urteil (wie bisher `stableGate.ts` gateUrteil).
fn anflug_urteil(teilnoten: &[SubScoreEntry]) -> Option<String> {
    let s = teilnoten.iter().find(|t| t.key == "stability")?;
    if s.gate.is_empty() {
        return None;
    }
    Some(match s.warning.as_deref().or(s.reason.as_deref()) {
        Some("anflug_unstable") => "unstable",
        Some("anflug_partial") => "partial",
        _ => "stable",
    }
    .to_string())
}

/// Bewertet ein Eingabe-Dokument. Fehler = ungueltige Eingabe.
pub fn bewerten(e: &Eingabe) -> Ausgabe {
    let teilnoten = compute_sub_scores(&e.landung);
    let gesamt = aggregate_master_score(&teilnoten);
    Ausgabe {
        fassung: FASSUNG,
        gesamt,
        deckel: master_deckel_wirksam(&teilnoten),
        landungs_abzug: landungs_abzug(&teilnoten).map(|(k, p)| (k.to_string(), p)),
        anflug_abzug: anflug_abzug(&teilnoten).map(|(k, p)| (k.to_string(), p)),
        gesamt_wort: gesamt.map(|g| gesamt_wort(g).to_string()),
        gesamt_buchstabe: gesamt.map(|g| gesamt_buchstabe(i32::from(g)).to_string()),
        anflug_urteil: anflug_urteil(&teilnoten),
        grenzen: Grenzen {
            vs_fpm: [
                crate::sub_landing_rate::T_VS_SMOOTH_FPM,
                crate::sub_landing_rate::T_VS_FIRM_FPM,
                crate::sub_landing_rate::T_VS_HARD_FPM,
                crate::sub_landing_rate::T_VS_SEVERE_FPM,
            ],
            g: [
                crate::sub_g_force::T_G_SMOOTH,
                crate::sub_g_force::T_G_FIRM,
                crate::sub_g_force::T_G_HARD,
                crate::sub_g_force::T_G_SEVERE,
            ],
        },
        teilnoten,
    }
}

/// JSON-Schnittstelle des Moduls: Eingabe-Dokument hinein, Ausgabe oder
/// `{"fehler": …}` heraus.
pub fn bewerten_json(eingabe: &str) -> String {
    match serde_json::from_str::<Eingabe>(eingabe) {
        Ok(e) => serde_json::to_string(&bewerten(&e)).unwrap_or_else(|x| fehler(&x.to_string())),
        Err(x) => fehler(&format!("Eingabe ungueltig: {x}")),
    }
}

fn fehler(text: &str) -> String {
    serde_json::json!({ "fehler": text }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> LandingScoringInput {
        LandingScoringInput { vs_fpm: Some(-180.0), peak_g_load: Some(1.21), bounce_count: Some(0), ..Default::default() }
    }

    #[test]
    fn json_ergibt_dasselbe_wie_nativ() {
        let e = Eingabe { landung: probe() };
        let nativ = bewerten(&e);
        let ueber_json: Ausgabe = serde_json::from_str(&bewerten_json(&serde_json::to_string(&e).unwrap())).unwrap();
        assert_eq!(ueber_json, nativ);
        assert_eq!(nativ.fassung, 27);
        assert_eq!(nativ.gesamt, aggregate_master_score(&compute_sub_scores(&probe())));
    }

    /// Leitern wie bisher im Client (`aggregate_score_label`, `letter_grade`)
    /// und in der Webapp (`noteWieDerClient.ts`).
    #[test]
    fn wort_und_buchstabe_an_den_grenzen() {
        for (n, w) in [(100, "smooth"), (88, "smooth"), (87, "acceptable"), (75, "acceptable"), (74, "firm"), (50, "firm"), (49, "hard"), (15, "hard"), (14, "severe"), (0, "severe")] {
            assert_eq!(gesamt_wort(n), w, "{n}");
        }
        for (n, b) in [(100, "A+"), (95, "A+"), (94, "A"), (88, "A"), (87, "B+"), (82, "B+"), (81, "B"), (75, "B"), (74, "C"), (65, "C"), (64, "D"), (50, "D"), (49, "F"), (0, "F")] {
            assert_eq!(gesamt_buchstabe(n), b, "{n}");
        }
        let a = bewerten(&Eingabe { landung: probe() });
        let g = a.gesamt.unwrap();
        assert_eq!(a.gesamt_wort.as_deref(), Some(gesamt_wort(g)));
        assert_eq!(a.gesamt_buchstabe.as_deref(), Some(gesamt_buchstabe(i32::from(g))));
        assert_eq!(a.grenzen.vs_fpm, [200.0, 400.0, 600.0, 1000.0]);
        assert_eq!(a.grenzen.g, [1.2, 1.4, 1.7, 2.1]);
        assert_eq!(a.anflug_urteil, None, "ohne Gate-Pruefung kein Urteil");
    }

    #[test]
    fn ungueltige_eingabe_gibt_fehler() {
        let v: serde_json::Value = serde_json::from_str(&bewerten_json("{\"x\":1}")).unwrap();
        assert!(v["fehler"].as_str().unwrap().contains("ungueltig"));
    }
}
