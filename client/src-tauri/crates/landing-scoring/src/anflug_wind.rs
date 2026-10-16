//! Wind im Anflug — Kontext zur Anflug-Bewertung, keine Note.
//!
//! Score-Version 24 (Thomas 16.10.2026, JBU1500 MDPC→MUVR): Pilot Joel
//! bemaengelte, dass die Bewertung den Wind nicht kenne. Die Pruefpunkte
//! des Stable Gate bleiben dieselben; die Sinkgrenze waechst aber mit der
//! Grundgeschwindigkeit (`sinkgrenze_fpm` in lib.rs). Dieses Modul haelt
//! fest, was die Oberflaechen dazu zeigen: Wind im Gate, Soll-Sinkrate bei
//! der tatsaechlichen Grundgeschwindigkeit, angewandte Grenze und den
//! laengsten Lauf unter ihr. Alles wird beim Aufsetzen EINMAL gerechnet und
//! eingefroren — Client und Webapp rechnen nichts nach.
//!
//! Fenster: dieselben Messpunkte wie das Gate (0 < Hoehe ueber Schwelle
//! ≤ 1000 ft, ohne die letzten 3 s vor dem Aufsetzen).
//!
//! Wind aus den koerperfesten Komponenten (`AIRCRAFT WIND X/Z` bzw. die
//! X-Plane-Entsprechung). Vorzeichen wie im Landeprotokoll: Gegenwind
//! positiv, Seitenwind positiv = von rechts.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
pub struct SinkLaufInfo {
    /// Dauer des Laufs in Sekunden.
    pub dauer_s: f32,
    /// Hoehe ueber der Schwelle am Beginn bzw. Ende (ft).
    pub von_ft: f32,
    pub bis_ft: f32,
    /// Staerkste Sinkrate im Lauf (fpm, negativ).
    pub spitze_fpm: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct AnflugWind {
    /// Messpunkte mit lesbarem Wind. 0 → alle Windfelder `None`.
    pub wind_proben: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gegenwind_mittel_kt: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gegenwind_min_kt: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gegenwind_max_kt: Option<f32>,
    /// Mittlerer Seitenwind mit Vorzeichen (positiv = von rechts).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seitenwind_mittel_kt: Option<f32>,
    /// Staerkster Seitenwind (Betrag).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seitenwind_max_kt: Option<f32>,
    /// Schwankung des Gegenwinds (Standardabweichung, kt) — Boeigkeit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schwankung_kt: Option<f32>,
    /// Mittlere Grundgeschwindigkeit im Gate (kt).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gs_mittel_kt: Option<f32>,
    /// Soll-Sinkrate des Gleitpfads bei dieser Grundgeschwindigkeit (fpm,
    /// negativ) — dieselbe Rechnung wie `target_vs` im Gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soll_sinkrate_fpm: Option<f32>,
    /// Angewandte Sinkgrenze, gemittelt ueber das Gate (fpm, negativ).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sinkgrenze_fpm: Option<f32>,
    /// Laengster Lauf unter der Sinkgrenze — auch unter 10 s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sinklauf: Option<SinkLaufInfo>,
}

/// Windteil aus den Messpunkten: (Gegenwind, Seitenwind) je Probe, `None`
/// = nicht gemessen. Proben mit fehlender oder nicht endlicher Komponente
/// zaehlen nicht mit — lieber kein Wert als eine erfundene Null.
pub fn wind_zusammenfassen(proben: &[(Option<f32>, Option<f32>)]) -> AnflugWind {
    let gueltig: Vec<(f64, f64)> = proben
        .iter()
        .filter_map(|&(h, x)| match (h, x) {
            (Some(h), Some(x)) if h.is_finite() && x.is_finite() => {
                Some((f64::from(h), f64::from(x)))
            }
            _ => None,
        })
        .collect();
    let mut w = AnflugWind {
        wind_proben: gueltig.len() as u32,
        ..Default::default()
    };
    if gueltig.is_empty() {
        return w;
    }
    let n = gueltig.len() as f64;
    let h_mittel = gueltig.iter().map(|p| p.0).sum::<f64>() / n;
    let x_mittel = gueltig.iter().map(|p| p.1).sum::<f64>() / n;
    let h_var = gueltig
        .iter()
        .map(|p| (p.0 - h_mittel).powi(2))
        .sum::<f64>()
        / n;
    w.gegenwind_mittel_kt = Some(h_mittel as f32);
    w.gegenwind_min_kt = gueltig
        .iter()
        .map(|p| p.0)
        .reduce(f64::min)
        .map(|v| v as f32);
    w.gegenwind_max_kt = gueltig
        .iter()
        .map(|p| p.0)
        .reduce(f64::max)
        .map(|v| v as f32);
    w.seitenwind_mittel_kt = Some(x_mittel as f32);
    w.seitenwind_max_kt = gueltig
        .iter()
        .map(|p| p.1.abs())
        .reduce(f64::max)
        .map(|v| v as f32);
    // Streuung erst ab drei Proben — aus einer einzelnen kaeme „± 0 kt",
    // als waere es windstill (wie `stability_stddev`).
    if gueltig.len() >= 3 {
        w.schwankung_kt = Some(h_var.sqrt() as f32);
    }
    w
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wind_aus_proben_mit_luecken() {
        let w = wind_zusammenfassen(&[
            (Some(10.0), Some(-4.0)),
            (None, Some(3.0)),
            (Some(20.0), Some(-8.0)),
            (Some(f32::NAN), Some(1.0)),
            (Some(15.0), Some(-6.0)),
        ]);
        assert_eq!(w.wind_proben, 3);
        assert_eq!(w.gegenwind_mittel_kt, Some(15.0));
        assert_eq!(w.gegenwind_min_kt, Some(10.0));
        assert_eq!(w.gegenwind_max_kt, Some(20.0));
        assert_eq!(w.seitenwind_mittel_kt, Some(-6.0));
        assert_eq!(w.seitenwind_max_kt, Some(8.0));
        assert!((w.schwankung_kt.unwrap() - 4.082).abs() < 0.001);
    }

    #[test]
    fn schwankung_erst_ab_drei_proben() {
        let w = wind_zusammenfassen(&[(Some(12.0), Some(1.0)), (Some(14.0), Some(1.0))]);
        assert_eq!(w.gegenwind_mittel_kt, Some(13.0));
        assert_eq!(w.schwankung_kt, None, "zwei Proben sind keine Boeigkeit");
    }

    #[test]
    fn ohne_windwerte_bleibt_alles_leer_statt_null() {
        let w = wind_zusammenfassen(&[(None, None), (Some(5.0), None)]);
        assert_eq!(w.wind_proben, 0);
        assert_eq!(w.gegenwind_mittel_kt, None);
        assert_eq!(w.seitenwind_max_kt, None);
        let json = serde_json::to_value(&w).unwrap();
        assert!(json.get("gegenwind_mittel_kt").is_none());
    }
}
