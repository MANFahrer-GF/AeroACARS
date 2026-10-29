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

/// `arrival.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ankunft {
    pub on_field_radius_nm: Grenzwert<f64>,
    pub on_field_fallback_radius_nm: Grenzwert<f64>,
    pub nearest_search_radius_nm: Grenzwert<f64>,
}

/// `spaetes_final.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpaetesFinal {
    pub spur_toleranz_deg: Grenzwert<f64>,
    pub quer_grund_m: Grenzwert<f64>,
    pub quer_anteil: Grenzwert<f64>,
    pub luecke_s: Grenzwert<f64>,
    pub max_probenabstand_s: Grenzwert<f64>,
    pub spur_zeitraum_s: Grenzwert<f64>,
    pub spur_min_weg_m: Grenzwert<f64>,
    pub spaet_unter_ft: Grenzwert<f32>,
    pub fehl_min_s: Grenzwert<f64>,
    pub kurve_unter_ft: Grenzwert<f32>,
    pub final_spaetestens_ft: Grenzwert<f32>,
    pub fluegel_waagerecht_deg: Grenzwert<f32>,
}

/// `ausfahrten.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ausfahrten {
    pub verlauf_min_abstand_m: Grenzwert<f64>,
    pub verlauf_vor_m: Grenzwert<f64>,
    pub verlauf_nach_m: Grenzwert<f64>,
    pub verlauf_quer_ueber_kante_m: Grenzwert<f64>,
    pub kanten_toleranz_m: Grenzwert<f64>,
    pub draussen_min_m: Grenzwert<f64>,
    pub dieselbe_stelle_m: Grenzwert<f64>,
    pub dieselbe_abzweigung_m: Grenzwert<f64>,
}

/// `replay_erkennung.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Replay {
    pub min_paardauer_s: Grenzwert<f64>,
    pub max_paardauer_s: Grenzwert<f64>,
    pub min_paare: Grenzwert<usize>,
    pub min_errechnete_gs_kt: Grenzwert<f64>,
    pub max_gemeldete_gs_kt: Grenzwert<f64>,
    pub max_gemeldete_ias_kt: Grenzwert<f64>,
    pub min_verhaeltnis: Grenzwert<f64>,
    pub min_hoehenrate_fps: Grenzwert<f64>,
    pub min_vs_fehler_fps: Grenzwert<f64>,
}

/// `runway_assessment.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bahnbewertung {
    pub tdz_min_runway_m: Grenzwert<f64>,
    pub tdz_max_length_m: Grenzwert<f64>,
    pub aim_short_m: Grenzwert<f64>,
    pub aim_long_m: Grenzwert<f64>,
    pub wch_min_ft: Grenzwert<f64>,
    pub wch_max_ft: Grenzwert<f64>,
    pub wch_toleranz_ft: Grenzwert<f64>,
}

/// `anflug_forensik.rs`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Anflug {
    pub grad_je_dot_bei_3_grad: Grenzwert<f64>,
    pub hoehe_oben_ft: Grenzwert<f64>,
    pub hoehe_mitte_ft: Grenzwert<f64>,
    pub hoehe_unten_ft: Grenzwert<f64>,
    pub anflug_fenster_s: Grenzwert<i64>,
    pub gleitweg_sektor_halbwinkel_deg: Grenzwert<f64>,
    pub kurs_toleranz_deg: Grenzwert<f64>,
    pub min_proben_je_tor: Grenzwert<usize>,
    pub tch_annahme_ft: Grenzwert<f64>,
    pub sim_boden_toleranz_ft: Grenzwert<f64>,
    pub sim_boden_laengs_m: Grenzwert<f64>,
    pub sim_boden_quer_min_m: Grenzwert<f64>,
    pub sim_boden_proben: Grenzwert<usize>,
    pub sim_boden_min_proben: Grenzwert<usize>,
    pub pfad_totband_dots: Grenzwert<f64>,
    pub schub_totband_pct: Grenzwert<f64>,
    pub rate_dt_min_s: Grenzwert<f64>,
    pub rate_dt_max_s: Grenzwert<f64>,
    pub min_raten: Grenzwert<usize>,
    pub min_dauer_schub_s: Grenzwert<f64>,
    pub scherung_min_kt: Grenzwert<f64>,
    pub scherung_dt_min_s: Grenzwert<f64>,
    pub scherung_dt_max_s: Grenzwert<f64>,
    pub scherung_hoehe_unten_ft: Grenzwert<f64>,
    pub scherung_hoehe_oben_ft: Grenzwert<f64>,
    pub scherung_entprellen_s: Grenzwert<f64>,
    pub scherungen_max: Grenzwert<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct Grenzwerte {
    pub staende: Staende,
    pub phasen: Phasen,
    pub aufsetzen: Aufsetzen,
    pub ankunft: Ankunft,
    pub spaetes_final: SpaetesFinal,
    pub ausfahrten: Ausfahrten,
    pub replay: Replay,
    pub bahnbewertung: Bahnbewertung,
    pub anflug: Anflug,
}

/// Bereich -> Datei im Paket.
pub(crate) const DATEIEN: &[&str] = &["grenzwerte/staende.json", "grenzwerte/phasen.json", "grenzwerte/aufsetzen.json", "grenzwerte/ankunft.json", "grenzwerte/spaetes_final.json", "grenzwerte/ausfahrten.json", "grenzwerte/replay.json", "grenzwerte/bahnbewertung.json", "grenzwerte/anflug.json"];

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
        ankunft: bereich(&datei, "grenzwerte/ankunft.json")?,
        spaetes_final: bereich(&datei, "grenzwerte/spaetes_final.json")?,
        ausfahrten: bereich(&datei, "grenzwerte/ausfahrten.json")?,
        replay: bereich(&datei, "grenzwerte/replay.json")?,
        bahnbewertung: bereich(&datei, "grenzwerte/bahnbewertung.json")?,
        anflug: bereich(&datei, "grenzwerte/anflug.json")?,
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
    fn grenzwerte_ankunft_wie_2_1_24() {
        let g = &grenzwerte().ankunft;
        assert_eq!(g.on_field_radius_nm.wert, 2.0); // arrival.rs:61
        assert_eq!(g.on_field_fallback_radius_nm.wert, 3.0); // arrival.rs:72
        assert_eq!(g.nearest_search_radius_nm.wert, 50.0); // arrival.rs:77
    }

    #[test]
    fn grenzwerte_spaetes_final_wie_2_1_24() {
        let g = &grenzwerte().spaetes_final;
        assert_eq!(g.spur_toleranz_deg.wert, 20.0); // spaetes_final.rs:39
        assert_eq!(g.quer_grund_m.wert, 300.0); // spaetes_final.rs:41
        assert_eq!(g.quer_anteil.wert, 0.05); // spaetes_final.rs:43
        assert_eq!(g.luecke_s.wert, 3.0); // spaetes_final.rs:45
        assert_eq!(g.max_probenabstand_s.wert, 5.0); // spaetes_final.rs:49
        assert_eq!(g.spur_zeitraum_s.wert, 4.0); // spaetes_final.rs:51
        assert_eq!(g.spur_min_weg_m.wert, 30.0); // spaetes_final.rs:53
        assert_eq!(g.spaet_unter_ft.wert, 1000.0); // spaetes_final.rs:55
        assert_eq!(g.fehl_min_s.wert, 20.0); // spaetes_final.rs:61
        assert_eq!(g.kurve_unter_ft.wert, 2000.0); // spaetes_final.rs:63
        assert_eq!(g.final_spaetestens_ft.wert, 300.0); // spaetes_final.rs:65
        assert_eq!(g.fluegel_waagerecht_deg.wert, 10.0); // spaetes_final.rs:67
    }

    #[test]
    fn grenzwerte_ausfahrten_wie_2_1_24() {
        let g = &grenzwerte().ausfahrten;
        assert_eq!(g.verlauf_min_abstand_m.wert, 8.0); // ausfahrten.rs:85
        assert_eq!(g.verlauf_vor_m.wert, 50.0); // ausfahrten.rs:93
        assert_eq!(g.verlauf_nach_m.wert, 400.0); // ausfahrten.rs:94
        assert_eq!(g.verlauf_quer_ueber_kante_m.wert, 150.0); // ausfahrten.rs:107
        assert_eq!(g.kanten_toleranz_m.wert, 25.0); // ausfahrten.rs:117
        assert_eq!(g.draussen_min_m.wert, 2.0); // ausfahrten.rs:123
        assert_eq!(g.dieselbe_stelle_m.wert, 150.0); // ausfahrten.rs:135
        assert_eq!(g.dieselbe_abzweigung_m.wert, 0.5); // ausfahrten.rs:218
    }

    #[test]
    fn grenzwerte_replay_wie_2_1_24() {
        let g = &grenzwerte().replay;
        assert_eq!(g.min_paardauer_s.wert, 0.5); // replay_erkennung.rs:64
        assert_eq!(g.max_paardauer_s.wert, 4.0); // replay_erkennung.rs:84
        assert_eq!(g.min_paare.wert, 8); // replay_erkennung.rs:86
        assert_eq!(g.min_errechnete_gs_kt.wert, 30.0); // replay_erkennung.rs:89
        assert_eq!(g.max_gemeldete_gs_kt.wert, 5.0); // replay_erkennung.rs:91
        assert_eq!(g.max_gemeldete_ias_kt.wert, 30.0); // replay_erkennung.rs:93
        assert_eq!(g.min_verhaeltnis.wert, 4.0); // replay_erkennung.rs:96
        assert_eq!(g.min_hoehenrate_fps.wert, 3.0); // replay_erkennung.rs:98
        assert_eq!(g.min_vs_fehler_fps.wert, 5.0); // replay_erkennung.rs:100
    }

    #[test]
    fn grenzwerte_bahnbewertung_wie_2_1_24() {
        let g = &grenzwerte().bahnbewertung;
        assert_eq!(g.tdz_min_runway_m.wert, 1200.0); // runway_assessment.rs:46
        assert_eq!(g.tdz_max_length_m.wert, 900.0); // runway_assessment.rs:49
        assert_eq!(g.aim_short_m.wert, 300.0); // runway_assessment.rs:52
        assert_eq!(g.aim_long_m.wert, 400.0); // runway_assessment.rs:53
        assert_eq!(g.wch_min_ft.wert, 20.0); // runway_assessment.rs:294
        assert_eq!(g.wch_max_ft.wert, 50.0); // runway_assessment.rs:296
        assert_eq!(g.wch_toleranz_ft.wert, 10.0); // runway_assessment.rs:298
    }

    #[test]
    fn grenzwerte_anflug_wie_2_1_24() {
        let g = &grenzwerte().anflug;
        assert_eq!(g.grad_je_dot_bei_3_grad.wert, 0.35); // anflug_forensik.rs:48
        assert_eq!(g.hoehe_oben_ft.wert, 1000.0); // anflug_forensik.rs:55
        assert_eq!(g.hoehe_mitte_ft.wert, 500.0); // anflug_forensik.rs:57
        assert_eq!(g.hoehe_unten_ft.wert, 200.0); // anflug_forensik.rs:61
        assert_eq!(g.anflug_fenster_s.wert, 300); // anflug_forensik.rs:67
        assert_eq!(g.gleitweg_sektor_halbwinkel_deg.wert, 10.0); // anflug_forensik.rs:74
        assert_eq!(g.kurs_toleranz_deg.wert, 90.0); // anflug_forensik.rs:78
        assert_eq!(g.min_proben_je_tor.wert, 5); // anflug_forensik.rs:82
        assert_eq!(g.tch_annahme_ft.wert, 50.0); // anflug_forensik.rs:88
        assert_eq!(g.sim_boden_toleranz_ft.wert, 20.0); // anflug_forensik.rs:94
        assert_eq!(g.sim_boden_laengs_m.wert, 600.0); // anflug_forensik.rs:103
        assert_eq!(g.sim_boden_quer_min_m.wert, 30.0); // anflug_forensik.rs:106
        assert_eq!(g.sim_boden_proben.wert, 3); // anflug_forensik.rs:109
        assert_eq!(g.sim_boden_min_proben.wert, 2); // anflug_forensik.rs:111
        assert_eq!(g.pfad_totband_dots.wert, 0.1); // anflug_forensik.rs:118
        assert_eq!(g.schub_totband_pct.wert, 2.0); // anflug_forensik.rs:125
        assert_eq!(g.rate_dt_min_s.wert, 0.2); // anflug_forensik.rs:130
        assert_eq!(g.rate_dt_max_s.wert, 5.0); // anflug_forensik.rs:131
        assert_eq!(g.min_raten.wert, 4); // anflug_forensik.rs:134
        assert_eq!(g.min_dauer_schub_s.wert, 10.0); // anflug_forensik.rs:137
        assert_eq!(g.scherung_min_kt.wert, 15.0); // anflug_forensik.rs:150
        assert_eq!(g.scherung_dt_min_s.wert, 4.0); // anflug_forensik.rs:151
        assert_eq!(g.scherung_dt_max_s.wert, 5.5); // anflug_forensik.rs:152
        assert_eq!(g.scherung_hoehe_unten_ft.wert, 30.0); // anflug_forensik.rs:153
        assert_eq!(g.scherung_hoehe_oben_ft.wert, 1500.0); // anflug_forensik.rs:154
        assert_eq!(g.scherung_entprellen_s.wert, 60.0); // anflug_forensik.rs:157
        assert_eq!(g.scherungen_max.wert, 2); // anflug_forensik.rs:161
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
