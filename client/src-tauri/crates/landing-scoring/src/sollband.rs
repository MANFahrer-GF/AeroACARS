//! v3 (P-4.20, P-4.21): Soll-Sinkrate und Soll-Band des Anflugs — bisher
//! `webapp/src/lib/anflugSollband.ts` (und dessen Kopie im Client). Gleiche
//! Werte, gleiche Ausnahmen; Webapp und Client lesen es kuenftig aus dem
//! Modul (Export `sollband`, gemeinsamer Lader Etappe 5).

use serde::{Deserialize, Serialize};

/// fpm je kt Grundgeschwindigkeit auf einem 3-Grad-Pfad.
pub const FPM_JE_KT_3_GRAD: f64 = 5.31;
/// Halbe Breite des Soll-Bands.
pub const SOLLBAND_TOLERANZ_FPM: f64 = 100.0;
/// Darunter keine Soll-Sinkrate (Rollen, Schweben).
pub const MIN_GS_KT: f64 = 30.0;

/// Faktor des Gleitwinkels gegenueber 3 Grad (unplausible Winkel = 1).
pub fn gleitwinkel_faktor(winkel_grad: Option<f64>) -> f64 {
    match winkel_grad {
        Some(w) if w.is_finite() && (2.0..=7.5).contains(&w) => {
            (w * std::f64::consts::PI / 180.0).tan() / (3.0 * std::f64::consts::PI / 180.0).tan()
        }
        _ => 1.0,
    }
}

/// Soll-Sinkrate (negativ) bei `gs_kt`; `None` unter [`MIN_GS_KT`].
pub fn soll_sinkrate_fpm(gs_kt: Option<f64>, winkel_grad: Option<f64>) -> Option<f64> {
    let gs = gs_kt.filter(|g| g.is_finite() && *g >= MIN_GS_KT)?;
    Some(-gs * FPM_JE_KT_3_GRAD * gleitwinkel_faktor(winkel_grad))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Probe {
    #[serde(default)]
    pub gs_kt: Option<f64>,
    #[serde(default)]
    pub is_flare: Option<bool>,
    #[serde(default)]
    pub t_ms: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Punkt {
    pub index: usize,
    pub soll: f64,
    pub oben: f64,
    pub unten: f64,
}

/// Soll-Band je Probe — nur vor dem Aufsetzen (`t_ms` <= 0 oder fehlt),
/// ausserhalb des Flare und ab [`MIN_GS_KT`].
pub fn sollband(proben: &[Probe], winkel_grad: Option<f64>) -> Vec<Punkt> {
    let faktor = gleitwinkel_faktor(winkel_grad);
    proben
        .iter()
        .enumerate()
        .filter_map(|(index, p)| {
            let gs = p.gs_kt.filter(|g| g.is_finite() && *g >= MIN_GS_KT)?;
            if p.is_flare == Some(true) || p.t_ms.is_some_and(|t| t > 0.0) {
                return None;
            }
            let soll = -gs * FPM_JE_KT_3_GRAD * faktor;
            Some(Punkt { index, soll, oben: soll + SOLLBAND_TOLERANZ_FPM, unten: soll - SOLLBAND_TOLERANZ_FPM })
        })
        .collect()
}

/// Eingabe des Exports `sollband`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anfrage {
    pub proben: Vec<Probe>,
    #[serde(default)]
    pub winkel_grad: Option<f64>,
}

/// JSON-Schnittstelle des Exports `sollband`.
pub fn sollband_json(eingabe: &str) -> String {
    match serde_json::from_str::<Anfrage>(eingabe) {
        Ok(a) => serde_json::to_string(&sollband(&a.proben, a.winkel_grad)).unwrap_or_default(),
        Err(e) => serde_json::json!({ "fehler": format!("Eingabe ungueltig: {e}") }).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(gs: Option<f64>, flare: Option<bool>, t: Option<f64>) -> Probe {
        Probe { gs_kt: gs, is_flare: flare, t_ms: t }
    }

    /// Dieselben Faelle wie `anflugSollband.ts`.
    #[test]
    fn wie_die_webapp() {
        assert_eq!(gleitwinkel_faktor(None), 1.0);
        assert_eq!(gleitwinkel_faktor(Some(1.9)), 1.0);
        assert_eq!(gleitwinkel_faktor(Some(7.6)), 1.0);
        assert!((gleitwinkel_faktor(Some(3.0)) - 1.0).abs() < 1e-12);
        assert!((gleitwinkel_faktor(Some(5.5)) - 1.8373).abs() < 1e-3, "steiler Anflug ~1,84 (tan 5,5° / tan 3°)");
        assert_eq!(soll_sinkrate_fpm(Some(29.9), None), None);
        assert!((soll_sinkrate_fpm(Some(140.0), None).unwrap() + 743.4).abs() < 1e-9);
        let band = sollband(
            &[p(Some(140.0), None, Some(-5000.0)), p(Some(20.0), None, None), p(Some(138.0), Some(true), None), p(Some(135.0), None, Some(100.0)), p(None, None, None), p(Some(136.0), Some(false), None)],
            None,
        );
        assert_eq!(band.iter().map(|b| b.index).collect::<Vec<_>>(), [0, 5]);
        assert!((band[0].oben - (band[0].soll + 100.0)).abs() < 1e-12);
        assert!((band[0].unten - (band[0].soll - 100.0)).abs() < 1e-12);
    }

    #[test]
    fn json_schnittstelle() {
        let aus: Vec<Punkt> = serde_json::from_str(&sollband_json(r#"{"proben":[{"gs_kt":140}],"winkel_grad":3}"#)).unwrap();
        assert_eq!(aus.len(), 1);
        assert!(sollband_json(r#"{"x":1}"#).contains("fehler"));
    }
}
