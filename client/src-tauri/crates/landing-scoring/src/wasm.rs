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

/// Ausgabe (P-4.20), Stand Etappe 3 Schritt 1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ausgabe {
    pub fassung: u32,
    pub teilnoten: Vec<SubScoreEntry>,
    pub gesamt: Option<u8>,
    pub deckel: Option<String>,
    pub landungs_abzug: Option<(String, u8)>,
    pub anflug_abzug: Option<(String, u8)>,
}

/// Bewertet ein Eingabe-Dokument. Fehler = ungueltige Eingabe.
pub fn bewerten(e: &Eingabe) -> Ausgabe {
    let teilnoten = compute_sub_scores(&e.landung);
    Ausgabe {
        fassung: FASSUNG,
        gesamt: aggregate_master_score(&teilnoten),
        deckel: master_deckel_wirksam(&teilnoten),
        landungs_abzug: landungs_abzug(&teilnoten).map(|(k, p)| (k.to_string(), p)),
        anflug_abzug: anflug_abzug(&teilnoten).map(|(k, p)| (k.to_string(), p)),
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

    #[test]
    fn ungueltige_eingabe_gibt_fehler() {
        let v: serde_json::Value = serde_json::from_str(&bewerten_json("{\"x\":1}")).unwrap();
        assert!(v["fehler"].as_str().unwrap().contains("ungueltig"));
    }
}
