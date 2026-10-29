//! v3 (Etappe 1b): Fachliche Grenzwerte aus dem Laufzeit-Paket (Pflichtenheft
//! P-3.55, P-3.65).
//!
//! Die Zahlen stehen nur in `bundle/grenzwerte/<bereich>.json`, nie im
//! Rust-Code. Je Bereich eine Struktur ohne Vorgabewerte und mit
//! `deny_unknown_fields`: Ein falscher Feldname ist ein Compile-Fehler, ein
//! fehlendes oder fremdes Feld im Paket ein Lesefehler — dann gilt der ganze
//! eingebaute Stand (fail-closed, P-3.21). Neue Felder brauchen eine neue
//! Faehigkeit `grenzwerte-n` (P-2.15).

use std::sync::OnceLock;

use serde::Deserialize;

/// Ein Wert mit Einheit und Herkunft im Code von v2.1.24 (P-3.55). Gelesen
/// wird nur `wert`; `einheit` und `quelle` sind Pflicht, damit jede Zahl
/// nachvollziehbar bleibt.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Grenzwert<T> {
    pub wert: T,
    #[allow(dead_code)]
    pub einheit: String,
    #[allow(dead_code)]
    pub quelle: String,
}

/// Stand- und Rollweg-Erkennung (`stands.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Staende {
    pub stand_capture_radius_m: Grenzwert<f64>,
    pub stand_name_radius_m: Grenzwert<f64>,
    pub stand_name_vorsprung_m: Grenzwert<f64>,
    pub rollweg_mittellinie_m: Grenzwert<f64>,
}

/// `phase_v2.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Phasen {
    pub window_secs: Grenzwert<f64>,
    pub min_span_secs: Grenzwert<f64>,
    pub max_samples: Grenzwert<usize>,
    pub min_sample_spacing_secs: Grenzwert<f64>,
    pub reset_gap_secs: Grenzwert<f64>,
    pub rate_climb_fpm: Grenzwert<f64>,
    pub median_vs_climb_fpm: Grenzwert<f64>,
    pub rate_level_fpm: Grenzwert<f64>,
    pub median_vs_level_fpm: Grenzwert<f64>,
    pub cruise_ref_band_ft: Grenzwert<f64>,
    pub level_to_cruise_fallback_secs: Grenzwert<i64>,
    pub level_below_ref_to_cruise_fallback_secs: Grenzwert<i64>,
    pub obsref_band_ft: Grenzwert<f64>,
    pub ga_confirm_ft: Grenzwert<f64>,
    pub climbing_to_climb_fallback_secs: Grenzwert<i64>,
    pub cruise_min_agl_ft: Grenzwert<f64>,
    pub approach_agl_ft: Grenzwert<f64>,
    pub final_agl_ft: Grenzwert<f64>,
    pub cruise_fastpath_agl_ft: Grenzwert<f64>,
    pub cruise_fastpath_vs_fpm: Grenzwert<f32>,
    pub umkehr_min_agl_ft: Grenzwert<f64>,
    pub umkehr_halte_secs: Grenzwert<i64>,
    pub reiseflug_stufe_ft: Grenzwert<f64>,
    pub sinkflug_level_zu_cruise_secs: Grenzwert<i64>,
    pub climb_schnellsink_agl_ft: Grenzwert<f64>,
    pub climb_schnellsink_vs_fpm: Grenzwert<f32>,
    pub climb_schnellsink_secs: Grenzwert<i64>,
    pub climb_schnellsink_fruehestens_secs: Grenzwert<i64>,
}

/// `touchdown_v2.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Aufsetzen {
    pub bounce_forensic_min_agl_ft: Grenzwert<f32>,
    pub bounce_scored_min_agl_ft: Grenzwert<f32>,
    pub water_touchdown_min_descent_fpm: Grenzwert<f32>,
    pub tiefflug_spielraum_ft: Grenzwert<f32>,
    pub bodenhoehe_max_ft: Grenzwert<f32>,
    pub bodenhoehe_min_proben: Grenzwert<u32>,
    pub bodenhoehe_max_gs_kt: Grenzwert<f32>,
    pub bodenhoehe_untergrenze_ft: Grenzwert<f32>,
    pub bodenhoehe_klasse_ft: Grenzwert<f32>,
    pub gear_force_epsilon_n: Grenzwert<f32>,
    pub abs_floor: Grenzwert<f32>,
    pub mass_ratio: Grenzwert<f32>,
    pub max_bewertungs_luecke_ms: Grenzwert<i64>,
    pub min_bewertungs_proben: Grenzwert<usize>,
    pub bewertungs_fenster_vor_ms: Grenzwert<i64>,
    pub bewertungs_fenster_nach_ms: Grenzwert<i64>,
    pub min_gear_force_samples_in_window: Grenzwert<usize>,
    pub min_ground_truth_samples: Grenzwert<usize>,
    pub min_settle_g_force_peak: Grenzwert<f32>,
    pub max_coverage_gap_ms: Grenzwert<i64>,
    pub vs_floor_fpm: Grenzwert<f32>,
}

#[derive(Debug, Clone)]
pub(crate) struct Grenzwerte {
    pub staende: Staende,
    pub phasen: Phasen,
    pub aufsetzen: Aufsetzen,
}

/// Bereich -> Datei im Paket.
pub(crate) const DATEIEN: &[&str] = &["grenzwerte/staende.json", "grenzwerte/phasen.json", "grenzwerte/aufsetzen.json"];

static GRENZWERTE: OnceLock<Grenzwerte> = OnceLock::new();

/// Die geltenden Grenzwerte. Ohne Laufzeit-Paket (z. B. in Tests) die
/// eingebauten.
pub(crate) fn grenzwerte() -> &'static Grenzwerte {
    GRENZWERTE.get_or_init(|| {
        lesen(|p| crate::laufzeit_paket::eingebaute_datei(p).map(<[u8]>::to_vec))
            .expect("eingebaute Grenzwerte lesbar (Test grenzwerte_eingebaut_lesbar)")
    })
}

/// Setzt die Grenzwerte aus dem Laufzeit-Paket. `false`, wenn schon welche
/// gelesen wurden.
pub(crate) fn setzen(g: Grenzwerte) -> bool {
    GRENZWERTE.set(g).is_ok()
}

/// Liest alle Bereiche ueber `datei(pfad)`.
pub(crate) fn lesen(datei: impl Fn(&str) -> Option<Vec<u8>>) -> Result<Grenzwerte, String> {
    fn bereich<T: serde::de::DeserializeOwned>(
        datei: &impl Fn(&str) -> Option<Vec<u8>>,
        pfad: &str,
    ) -> Result<T, String> {
        let bytes = datei(pfad).ok_or_else(|| format!("{pfad} fehlt"))?;
        serde_json::from_slice(&bytes).map_err(|e| format!("{pfad}: {e}"))
    }
    Ok(Grenzwerte {
        staende: bereich(&datei, "grenzwerte/staende.json")?,
        phasen: bereich(&datei, "grenzwerte/phasen.json")?,
        aufsetzen: bereich(&datei, "grenzwerte/aufsetzen.json")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grenzwerte_eingebaut_lesbar() {
        lesen(|p| crate::laufzeit_paket::eingebaute_datei(p).map(<[u8]>::to_vec)).unwrap();
    }

    /// Die Tabelle traegt dieselben Werte wie die Konstanten in v2.1.24
    /// (Fundstellen in `bundle/grenzwerte/staende.json`). Eine bewusste
    /// Aenderung passt diesen Test im selben Commit an.
    #[test]
    fn grenzwerte_staende_wie_2_1_24() {
        let s = &grenzwerte().staende;
        assert_eq!(s.stand_capture_radius_m.wert, 60.0); // stands.rs:51
        assert_eq!(s.stand_name_radius_m.wert, 60.0); // stands.rs:64
        assert_eq!(s.stand_name_vorsprung_m.wert, 10.0); // stands.rs:69
        assert_eq!(s.rollweg_mittellinie_m.wert, 3.0); // stands.rs:326
    }

    #[test]
    fn grenzwerte_phasen_wie_2_1_24() {
        let g = &grenzwerte().phasen;
        assert_eq!(g.window_secs.wert, 60.0); // phase_v2.rs:66
        assert_eq!(g.min_span_secs.wert, 45.0); // phase_v2.rs:70
        assert_eq!(g.max_samples.wert, 64); // phase_v2.rs:76
        assert_eq!(g.min_sample_spacing_secs.wert, 1.5); // phase_v2.rs:81
        assert_eq!(g.reset_gap_secs.wert, 300.0); // phase_v2.rs:86
        assert_eq!(g.rate_climb_fpm.wert, 300.0); // phase_v2.rs:90
        assert_eq!(g.median_vs_climb_fpm.wert, 200.0); // phase_v2.rs:92
        assert_eq!(g.rate_level_fpm.wert, 150.0); // phase_v2.rs:94
        assert_eq!(g.median_vs_level_fpm.wert, 200.0); // phase_v2.rs:96
        assert_eq!(g.cruise_ref_band_ft.wert, 1000.0); // phase_v2.rs:102
        assert_eq!(g.level_to_cruise_fallback_secs.wert, 240); // phase_v2.rs:108
        assert_eq!(g.level_below_ref_to_cruise_fallback_secs.wert, 600); // phase_v2.rs:121
        assert_eq!(g.obsref_band_ft.wert, 1000.0); // phase_v2.rs:137
        assert_eq!(g.ga_confirm_ft.wert, 700.0); // phase_v2.rs:150
        assert_eq!(g.climbing_to_climb_fallback_secs.wert, 60); // phase_v2.rs:155
        assert_eq!(g.cruise_min_agl_ft.wert, 5000.0); // phase_v2.rs:158
        assert_eq!(g.approach_agl_ft.wert, 5000.0); // phase_v2.rs:161
        assert_eq!(g.final_agl_ft.wert, 700.0); // phase_v2.rs:163
        assert_eq!(g.cruise_fastpath_agl_ft.wert, 3000.0); // phase_v2.rs:166
        assert_eq!(g.cruise_fastpath_vs_fpm.wert, -500.0); // phase_v2.rs:168
        assert_eq!(g.umkehr_min_agl_ft.wert, 3000.0); // phase_v2.rs:175
        assert_eq!(g.umkehr_halte_secs.wert, 120); // phase_v2.rs:179
        assert_eq!(g.reiseflug_stufe_ft.wert, 2000.0); // phase_v2.rs:183
        assert_eq!(g.sinkflug_level_zu_cruise_secs.wert, 600); // phase_v2.rs:186
        assert_eq!(g.climb_schnellsink_agl_ft.wert, 3000.0); // phase_v2.rs:190
        assert_eq!(g.climb_schnellsink_vs_fpm.wert, -500.0); // phase_v2.rs:191
        assert_eq!(g.climb_schnellsink_secs.wert, 10); // phase_v2.rs:192
        assert_eq!(g.climb_schnellsink_fruehestens_secs.wert, 120); // phase_v2.rs:197
    }

    #[test]
    fn grenzwerte_aufsetzen_wie_2_1_24() {
        let g = &grenzwerte().aufsetzen;
        assert_eq!(g.bounce_forensic_min_agl_ft.wert, 5.0); // touchdown_v2.rs:45
        assert_eq!(g.bounce_scored_min_agl_ft.wert, 5.0); // touchdown_v2.rs:62
        assert_eq!(g.water_touchdown_min_descent_fpm.wert, -50.0); // touchdown_v2.rs:73
        assert_eq!(g.tiefflug_spielraum_ft.wert, 5.0); // touchdown_v2.rs:99
        assert_eq!(g.bodenhoehe_max_ft.wert, 22.0); // touchdown_v2.rs:110
        assert_eq!(g.bodenhoehe_min_proben.wert, 250); // touchdown_v2.rs:113
        assert_eq!(g.bodenhoehe_max_gs_kt.wert, 40.0); // touchdown_v2.rs:117
        assert_eq!(g.bodenhoehe_untergrenze_ft.wert, -5.0); // touchdown_v2.rs:119
        assert_eq!(g.bodenhoehe_klasse_ft.wert, 0.25); // touchdown_v2.rs:120
        assert_eq!(g.gear_force_epsilon_n.wert, 1.0); // touchdown_v2.rs:255
        assert_eq!(g.abs_floor.wert, 1000.0); // touchdown_v2.rs:376
        assert_eq!(g.mass_ratio.wert, 0.03); // touchdown_v2.rs:377
        assert_eq!(g.max_bewertungs_luecke_ms.wert, 200); // touchdown_v2.rs:463
        assert_eq!(g.min_bewertungs_proben.wert, 12); // touchdown_v2.rs:472
        assert_eq!(g.bewertungs_fenster_vor_ms.wert, 1000); // touchdown_v2.rs:482
        assert_eq!(g.bewertungs_fenster_nach_ms.wert, 100); // touchdown_v2.rs:483
        assert_eq!(g.min_gear_force_samples_in_window.wert, 2); // touchdown_v2.rs:704
        assert_eq!(g.min_ground_truth_samples.wert, 2); // touchdown_v2.rs:787
        assert_eq!(g.min_settle_g_force_peak.wert, 1.02); // touchdown_v2.rs:820
        assert_eq!(g.max_coverage_gap_ms.wert, 200); // touchdown_v2.rs:847
        assert_eq!(g.vs_floor_fpm.wert, -3000.0); // touchdown_v2.rs:1154
    }

    #[test]
    fn fremdes_oder_fehlendes_feld_wird_abgelehnt() {
        let echt = crate::laufzeit_paket::eingebaute_datei("grenzwerte/staende.json").unwrap();
        let mut v: serde_json::Value = serde_json::from_slice(echt).unwrap();
        v["tippfehler_radius_m"] = serde_json::json!({"wert": 1.0, "einheit": "m", "quelle": "x"});
        let fremd = serde_json::to_vec(&v).unwrap();
        assert!(lesen(|_| Some(fremd.clone())).is_err());
        let mut v: serde_json::Value = serde_json::from_slice(echt).unwrap();
        v.as_object_mut().unwrap().remove("stand_name_radius_m");
        let fehlt = serde_json::to_vec(&v).unwrap();
        assert!(lesen(|_| Some(fehlt.clone())).is_err());
    }
}
