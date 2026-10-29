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

/// Cockpit-Ereignisse im Aktivitaetslog, Klappen-Log, Auto-Start (`lib.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cockpit {
    pub ap_debounce_secs: Grenzwert<i64>,
    pub premium_fma_debounce_secs: Grenzwert<i64>,
    pub master_annunciator_debounce_secs: Grenzwert<i64>,
    pub aircraft_phase_debounce_secs: Grenzwert<i64>,
    pub stab_trim_debounce_secs: Grenzwert<i64>,
    pub thrust_gate_debounce_secs: Grenzwert<i64>,
    pub approach_speeds_debounce_secs: Grenzwert<i64>,
    pub approach_speeds_bucket_kt: Grenzwert<f64>,
    pub premium_log_warmup_secs: Grenzwert<i64>,
    pub premium_log_stream_gap_secs: Grenzwert<i64>,
    pub reverser_spoiler_min_gs_kt: Grenzwert<f32>,
    pub below_gs_rearm_secs: Grenzwert<i64>,
    pub stall_warning_mqtt_debounce_secs: Grenzwert<i64>,
    pub activity_dedupe_window_secs: Grenzwert<i64>,
    pub flicker_window_secs: Grenzwert<i64>,
    pub flicker_min_changes: Grenzwert<u8>,
    pub flap_log_stable_secs: Grenzwert<i64>,
    pub auto_start_interval_secs: Grenzwert<u64>,
    pub auto_start_fail_cooldown_secs: Grenzwert<i64>,
    pub auto_start_proximity_m: Grenzwert<f64>,
    pub auto_start_sim_ruhe_secs: Grenzwert<i64>,
    pub auto_start_sprung_m: Grenzwert<f64>,
}

/// Phasenautomat im Kern (`lib.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhasenKern {
    pub distance_epsilon_m: Grenzwert<f64>,
    pub min_boarding_movement_m: Grenzwert<f64>,
    pub reload_gap_factor: Grenzwert<f64>,
    pub pushback_haengt_kt: Grenzwert<f32>,
    pub pushback_haengt_s: Grenzwert<i64>,
    pub taxi_direction_fps: Grenzwert<f32>,
    pub arrived_fallback_dwell_secs: Grenzwert<i64>,
    pub rollweg_halt_bestaetigt_secs: Grenzwert<i64>,
    pub arrived_standstill_dwell_secs: Grenzwert<i64>,
    pub final_landing_verbuchen_nach_secs: Grenzwert<i64>,
    pub touch_and_go_agl_threshold_ft: Grenzwert<f32>,
    pub touch_and_go_dwell_secs: Grenzwert<i64>,
    pub go_around_agl_recovery_ft: Grenzwert<f32>,
    pub holding_kurs_fenster_secs: Grenzwert<i64>,
    pub holding_eintritt_drehung_deg: Grenzwert<f32>,
    pub holding_austritt_fenster_secs: Grenzwert<i64>,
    pub holding_austritt_drehung_deg: Grenzwert<f32>,
    pub holding_min_agl_ft: Grenzwert<f64>,
    pub holding_level_vs_fpm: Grenzwert<f32>,
    pub go_around_dwell_secs: Grenzwert<i64>,
    pub enroute_reconcile_dwell_secs: Grenzwert<i64>,
    pub enroute_reconcile_forward_dwell_secs: Grenzwert<i64>,
    pub go_around_min_vs_fpm: Grenzwert<f32>,
    pub go_around_schwelle_nm: Grenzwert<f64>,
    pub go_around_schwelle_kurs_deg: Grenzwert<f32>,
    pub was_airborne_agl_ft: Grenzwert<f32>,
    pub was_airborne_agl_max_ft: Grenzwert<f32>,
    pub was_airborne_dwell_ticks: Grenzwert<u8>,
    pub takeoff_roll_unambiguous_kt: Grenzwert<f64>,
    pub takeoff_roll_min_kt: Grenzwert<f64>,
    pub takeoff_roll_accel_kt_per_s: Grenzwert<f64>,
    pub takeoff_roll_accel_window_secs: Grenzwert<i64>,
    pub takeoff_roll_accel_windows_needed: Grenzwert<u8>,
    pub takeoff_roll_accel_strong_kt_per_s: Grenzwert<f64>,
    pub takeoff_roll_n1_pct: Grenzwert<f64>,
    pub min_fahrt_kt: Grenzwert<f32>,
    pub takeoff_roll_abort_kt: Grenzwert<f32>,
    pub takeoff_roll_abort_dwell_secs: Grenzwert<i64>,
    pub takeoff_roll_confirm_kt: Grenzwert<f32>,
    pub takeoff_roll_stall_dwell_secs: Grenzwert<i64>,
    pub pushback_dwell_secs: Grenzwert<i64>,
}

/// Landeerkennung und Aufsetzfenster im Kern (`lib.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Landung {
    pub aufsetzen_vor_luecke_max_s: Grenzwert<i64>,
    pub aufsetzen_nach_letztem_punkt_max_s: Grenzwert<i64>,
    pub aufsetzen_vor_luecke_max_nm: Grenzwert<f64>,
    pub aufsetz_pruefung_abwarten_s: Grenzwert<i64>,
    pub touchdown_buffer_secs: Grenzwert<i64>,
    pub touchdown_post_window_ms: Grenzwert<i64>,
    pub td_agl_max_at_touchdown_ft: Grenzwert<f32>,
    pub td_agl_max_at_window_start_ft: Grenzwert<f32>,
    pub lua_style_sample_count: Grenzwert<usize>,
    pub lua_style_min_samples: Grenzwert<usize>,
    pub touchdown_window_secs: Grenzwert<i64>,
    pub touchdown_vs_window_ms: Grenzwert<i64>,
    pub touchdown_g_window_ms: Grenzwert<i64>,
    pub bounce_window_secs: Grenzwert<i64>,
    pub bounce_min_dauer_ms: Grenzwert<i64>,
    pub bounce_light_min_dauer_ms: Grenzwert<i64>,
    pub bounce_light_min_agl_ft: Grenzwert<f32>,
    pub bounce_agl_threshold_ft: Grenzwert<f64>,
    pub bounce_agl_return_ft: Grenzwert<f64>,
    pub post_td_profile_ms: Grenzwert<i32>,
    pub g_kanal_steigung_msfs: Grenzwert<f32>,
    pub g_kanal_steigung_xplane: Grenzwert<f32>,
    pub msfs_flare_agl_fit_half_window_ms: Grenzwert<i64>,
    pub msfs_flare_agl_fit_min_samples: Grenzwert<usize>,
    pub msfs_flare_min_distinct_agl: Grenzwert<usize>,
    pub msfs_flare_max_median_dt_ms: Grenzwert<i64>,
    pub msfs_flare_min_end_fit_distinct_agl: Grenzwert<usize>,
    pub msfs_flare_min_end_agl_span_ft: Grenzwert<f32>,
    pub msfs_flare_balloon_tol_ft: Grenzwert<f32>,
    pub msfs_flare_pitch_nosedown_veto_deg: Grenzwert<f32>,
    pub msfs_flare_detect_floor_fpm: Grenzwert<f64>,
    pub agl_min_samples: Grenzwert<usize>,
    pub agl_min_verschiedene: Grenzwert<usize>,
    pub agl_max_stillstand_ms: Grenzwert<i64>,
    pub agl_min_spanne_s: Grenzwert<f64>,
    pub agl_max_plausibel_fpm: Grenzwert<f32>,
    pub probe_toleranz_fpm: Grenzwert<f32>,
    pub sampler_gear_touchdown_threshold_n: Grenzwert<f32>,
    pub sampler_water_contact_agl_ft: Grenzwert<f64>,
    pub aufsetzprobe_toleranz_ms: Grenzwert<i64>,
    pub abfang_fenster_s: Grenzwert<i64>,
}

/// Anflug-Plausibilitaet und -Stabilitaet im Kern (`lib.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnflugKern {
    pub approach_vs_plausibility_cap_fpm: Grenzwert<f32>,
    pub approach_bank_plausibility_cap_deg: Grenzwert<f32>,
    pub final_platz_nm: Grenzwert<f64>,
    pub hat_agl_plausibel_ft: Grenzwert<f32>,
    pub hat_agl_vertraut_ft: Grenzwert<f32>,
    pub approach_stability_agl_cap_ft: Grenzwert<f32>,
    pub approach_flare_cutoff_ms: Grenzwert<i64>,
    pub stability_min_samples: Grenzwert<usize>,
    pub sampler_stability_window_secs: Grenzwert<i64>,
    pub max_vs_dev_floor_ft: Grenzwert<f32>,
    pub flaps_unreadable_max_ias_kt: Grenzwert<f32>,
    pub flaps_implausible_below: Grenzwert<f32>,
    pub flaps_landing_min: Grenzwert<f32>,
    pub stabilisiert_ueber_vref_kt: Grenzwert<f32>,
    pub sink_toleranz_ms: Grenzwert<i64>,
    pub da_sink_toleranz_ms: Grenzwert<i64>,
    pub da_vref_toleranz_kt: Grenzwert<f32>,
    pub sink_gs_bezug_kt: Grenzwert<f64>,
    pub sink_max_luecke_ms: Grenzwert<i64>,
    pub min_bezugsgeschwindigkeit_kt: Grenzwert<f32>,
}

/// Ausrollen, Bahnspur, Bahnzuordnung im Kern (`lib.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ausrollen {
    pub rollout_tick_ms: Grenzwert<u64>,
    pub rollout_exit_gs_kt: Grenzwert<f32>,
    pub rollout_stop_gs_kt: Grenzwert<f32>,
    pub rollout_heading_deviation_deg: Grenzwert<f32>,
    pub touchdown_vs_severe_fpm: Grenzwert<f32>,
    pub touchdown_vs_hard_fpm: Grenzwert<f32>,
    pub touchdown_vs_firm_fpm: Grenzwert<f32>,
    pub touchdown_vs_smooth_fpm: Grenzwert<f32>,
    pub touchdown_g_severe: Grenzwert<f32>,
    pub touchdown_g_hard: Grenzwert<f32>,
    pub touchdown_g_firm: Grenzwert<f32>,
    pub touchdown_g_smooth: Grenzwert<f32>,
    pub runway_trust_max_centerline_offset_m: Grenzwert<f32>,
    pub runway_trust_min_float_distance_m: Grenzwert<f32>,
    pub td_nearest_radius_nm: Grenzwert<f64>,
    pub bestaetigungs_toleranz_m: Grenzwert<f64>,
    pub verlauf_behalten_m: Grenzwert<f64>,
    pub correlate_airport_ambiguity_nm: Grenzwert<f64>,
    pub correlate_airport_search_radius_nm: Grenzwert<f64>,
    pub achse_toleranz_m: Grenzwert<f64>,
    pub schwellenspur_max_agl_ft: Grenzwert<f64>,
    pub schwellenspur_takt_ms: Grenzwert<i64>,
    pub schwellenspur_secs: Grenzwert<i64>,
    pub tch_max_klammer_m: Grenzwert<f64>,
    pub bahn_kurs_ausfahrt_grad: Grenzwert<f32>,
    pub bahn_ausfahrt_max_gs_kt: Grenzwert<f32>,
    pub bahn_ausfahrt_beginn_grad: Grenzwert<f32>,
    pub bahn_mess_min_gs_kt: Grenzwert<f32>,
    pub bahn_mess_min_gs_kt_boden: Grenzwert<f32>,
    pub bahn_mess_schwelle_faktor: Grenzwert<f32>,
    pub bahn_spur_min_abstand_m: Grenzwert<f64>,
    pub bahn_spur_quer_gewicht: Grenzwert<f64>,
    pub bahn_spur_max_punkte: Grenzwert<usize>,
    pub bahn_richtung_fenster_m: Grenzwert<f32>,
    pub bahn_richtung_min_versatz_m: Grenzwert<f32>,
    pub bahn_spur_rand_m: Grenzwert<f64>,
    pub bahn_spur_stop_gs_kt: Grenzwert<f32>,
}

/// Sprit-Auswertung und Sprit-Signale (`lib.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Sprit {
    pub sprit_schwelle_ueber_platz_ft: Grenzwert<f64>,
    pub sprit_schwelle_abstand_zur_reisehoehe_ft: Grenzwert<f64>,
    pub sprit_tick_max_s: Grenzwert<f64>,
    pub sprit_projektion_max_abstand_nm: Grenzwert<f64>,
    pub sprit_projektion_gleichstand_nm: Grenzwert<f64>,
    pub sprit_tank_min_anteil: Grenzwert<f32>,
    pub sprit_sinkflug_unter_reisehoehe_ft: Grenzwert<f64>,
    pub sprit_triebwerke_aus_s: Grenzwert<f64>,
    pub sprit_wp_nah_nm: Grenzwert<f64>,
    pub ff_derive_window_s: Grenzwert<f32>,
    pub ff_max_kg_per_h: Grenzwert<f32>,
    pub fuel_used_simvar_alive_kg: Grenzwert<f32>,
    pub fuel_used_min_delta_kg: Grenzwert<f32>,
    pub fuel_used_refuel_relatch_kg: Grenzwert<f32>,
    pub fuel_used_defuel_relatch_kg: Grenzwert<f32>,
    pub toleranz_kg: Grenzwert<f32>,
    pub rueckfall_kg_pro_h: Grenzwert<f32>,
    pub snapshot_hoehe_min_ft: Grenzwert<f64>,
    pub snapshot_hoehe_max_ft: Grenzwert<f64>,
    pub defuel_threshold_kg: Grenzwert<f32>,
}

/// Wiederaufnahme, Integritaet, Abgabe, Divert-Vorabruf (`lib.rs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Abgabe {
    pub resume_stillstand_max_hours: Grenzwert<i64>,
    pub divert_prefetch_max_agl_ft: Grenzwert<f64>,
    pub divert_prefetch_max_dist_nm: Grenzwert<f64>,
    pub divert_prefetch_min_sink_fpm: Grenzwert<f64>,
    pub divert_prefetch_stable_secs: Grenzwert<i64>,
    pub divert_prefetch_planned_diverge_nm: Grenzwert<f64>,
    pub resume_drift_toast_nm: Grenzwert<f64>,
    pub resume_drift_warn_nm: Grenzwert<f64>,
    pub resume_drift_extreme_nm: Grenzwert<f64>,
    pub resume_fuel_jump_impossible_kg: Grenzwert<f64>,
    pub resume_zielsprung_vorher_nm: Grenzwert<f64>,
    pub resume_zielsprung_nachher_nm: Grenzwert<f64>,
    pub resume_max_plausibel_kt: Grenzwert<f64>,
    pub abgabe_unterbrechung_min_secs: Grenzwert<i64>,
    pub resume_drift_clean_nm: Grenzwert<f64>,
    pub max_start_distance_nm: Grenzwert<f64>,
    pub max_file_distance_nm: Grenzwert<f64>,
    pub pilot_confirmed_arrival_max_nm: Grenzwert<f64>,
    pub standing_on_other_field_nm: Grenzwert<f64>,
    pub min_flight_time_min: Grenzwert<i32>,
    pub min_distance_nm: Grenzwert<f64>,
    pub min_position_count: Grenzwert<u32>,
    pub reset_null_island_deg: Grenzwert<f64>,
    pub reset_max_alt_jump_ft: Grenzwert<f64>,
    pub simrate_strecke_max: Grenzwert<f64>,
    pub zeitraffer_gs_spielraum: Grenzwert<f64>,
    pub sim_referenz_fenster_s: Grenzwert<i64>,
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
    pub cockpit: Cockpit,
    pub phasen_kern: PhasenKern,
    pub landung: Landung,
    pub anflug_kern: AnflugKern,
    pub ausrollen: Ausrollen,
    pub sprit: Sprit,
    pub abgabe: Abgabe,
}

/// Bereich -> Datei im Paket (Pruefliste fuer Tests).
#[cfg(test)]
pub(crate) const DATEIEN: &[&str] = &["grenzwerte/staende.json", "grenzwerte/phasen.json", "grenzwerte/aufsetzen.json", "grenzwerte/ankunft.json", "grenzwerte/spaetes_final.json", "grenzwerte/ausfahrten.json", "grenzwerte/replay.json", "grenzwerte/bahnbewertung.json", "grenzwerte/anflug.json", "grenzwerte/cockpit.json", "grenzwerte/phasen_kern.json", "grenzwerte/landung.json", "grenzwerte/anflug_kern.json", "grenzwerte/ausrollen.json", "grenzwerte/sprit.json", "grenzwerte/abgabe.json"];

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
        cockpit: bereich(&datei, "grenzwerte/cockpit.json")?,
        phasen_kern: bereich(&datei, "grenzwerte/phasen_kern.json")?,
        landung: bereich(&datei, "grenzwerte/landung.json")?,
        anflug_kern: bereich(&datei, "grenzwerte/anflug_kern.json")?,
        ausrollen: bereich(&datei, "grenzwerte/ausrollen.json")?,
        sprit: bereich(&datei, "grenzwerte/sprit.json")?,
        abgabe: bereich(&datei, "grenzwerte/abgabe.json")?,
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
    fn grenzwerte_cockpit_wie_2_1_24() {
        let g = &grenzwerte().cockpit;
        assert_eq!(g.ap_debounce_secs.wert, 2); // lib.rs:11764
        assert_eq!(g.premium_fma_debounce_secs.wert, 3); // lib.rs:11769
        assert_eq!(g.master_annunciator_debounce_secs.wert, 2); // lib.rs:11772
        assert_eq!(g.aircraft_phase_debounce_secs.wert, 5); // lib.rs:11774
        assert_eq!(g.stab_trim_debounce_secs.wert, 5); // lib.rs:11776
        assert_eq!(g.thrust_gate_debounce_secs.wert, 3); // lib.rs:11779
        assert_eq!(g.approach_speeds_debounce_secs.wert, 10); // lib.rs:11783
        assert_eq!(g.approach_speeds_bucket_kt.wert, 5.0); // lib.rs:11785
        assert_eq!(g.premium_log_warmup_secs.wert, 10); // lib.rs:11789
        assert_eq!(g.premium_log_stream_gap_secs.wert, 15); // lib.rs:11793
        assert_eq!(g.reverser_spoiler_min_gs_kt.wert, 40.0); // lib.rs:11798
        assert_eq!(g.below_gs_rearm_secs.wert, 30); // lib.rs:11802
        assert_eq!(g.stall_warning_mqtt_debounce_secs.wert, 3); // lib.rs:11811
        assert_eq!(g.activity_dedupe_window_secs.wert, 5); // lib.rs:11915
        assert_eq!(g.flicker_window_secs.wert, 30); // lib.rs:12092
        assert_eq!(g.flicker_min_changes.wert, 4); // lib.rs:12096
        assert_eq!(g.flap_log_stable_secs.wert, 4); // lib.rs:49810
        assert_eq!(g.auto_start_interval_secs.wert, 3); // lib.rs:52604
        assert_eq!(g.auto_start_fail_cooldown_secs.wert, 120); // lib.rs:52608
        assert_eq!(g.auto_start_proximity_m.wert, 5_000.0); // lib.rs:52613
        assert_eq!(g.auto_start_sim_ruhe_secs.wert, 20); // lib.rs:52741
        assert_eq!(g.auto_start_sprung_m.wert, 1_000.0); // lib.rs:52767
    }

    #[test]
    fn grenzwerte_phasen_kern_wie_2_1_24() {
        let g = &grenzwerte().phasen_kern;
        assert_eq!(g.distance_epsilon_m.wert, 5.0); // lib.rs:458
        assert_eq!(g.min_boarding_movement_m.wert, 5.0); // lib.rs:466
        assert_eq!(g.reload_gap_factor.wert, 2.5); // lib.rs:473
        assert_eq!(g.pushback_haengt_kt.wert, 8.0); // lib.rs:496
        assert_eq!(g.pushback_haengt_s.wert, 15); // lib.rs:499
        assert_eq!(g.taxi_direction_fps.wert, 1.0); // lib.rs:552
        assert_eq!(g.arrived_fallback_dwell_secs.wert, 9); // lib.rs:987
        assert_eq!(g.rollweg_halt_bestaetigt_secs.wert, 120); // lib.rs:991
        assert_eq!(g.arrived_standstill_dwell_secs.wert, 300); // lib.rs:1026
        assert_eq!(g.final_landing_verbuchen_nach_secs.wert, 30); // lib.rs:1054
        assert_eq!(g.touch_and_go_agl_threshold_ft.wert, 100.0); // lib.rs:1057
        assert_eq!(g.touch_and_go_dwell_secs.wert, 1); // lib.rs:1060
        assert_eq!(g.go_around_agl_recovery_ft.wert, 150.0); // lib.rs:1068
        assert_eq!(g.holding_kurs_fenster_secs.wert, 360); // lib.rs:1097
        assert_eq!(g.holding_eintritt_drehung_deg.wert, 300.0); // lib.rs:1098
        assert_eq!(g.holding_austritt_fenster_secs.wert, 180); // lib.rs:1100
        assert_eq!(g.holding_austritt_drehung_deg.wert, 45.0); // lib.rs:1101
        assert_eq!(g.holding_min_agl_ft.wert, 1500.0); // lib.rs:1104
        assert_eq!(g.holding_level_vs_fpm.wert, 300.0); // lib.rs:1105
        assert_eq!(g.go_around_dwell_secs.wert, 8); // lib.rs:1107
        assert_eq!(g.enroute_reconcile_dwell_secs.wert, 90); // lib.rs:1164
        assert_eq!(g.enroute_reconcile_forward_dwell_secs.wert, 45); // lib.rs:1167
        assert_eq!(g.go_around_min_vs_fpm.wert, 300.0); // lib.rs:1174
        assert_eq!(g.go_around_schwelle_nm.wert, 5.0); // lib.rs:1180
        assert_eq!(g.go_around_schwelle_kurs_deg.wert, 45.0); // lib.rs:1182
        assert_eq!(g.was_airborne_agl_ft.wert, 50.0); // lib.rs:1188
        assert_eq!(g.was_airborne_agl_max_ft.wert, 30000.0); // lib.rs:1200
        assert_eq!(g.was_airborne_dwell_ticks.wert, 2); // lib.rs:1205
        assert_eq!(g.takeoff_roll_unambiguous_kt.wert, 60.0); // lib.rs:9365
        assert_eq!(g.takeoff_roll_min_kt.wert, 30.0); // lib.rs:9369
        assert_eq!(g.takeoff_roll_accel_kt_per_s.wert, 1.2); // lib.rs:9375
        assert_eq!(g.takeoff_roll_accel_window_secs.wert, 3); // lib.rs:9376
        assert_eq!(g.takeoff_roll_accel_windows_needed.wert, 2); // lib.rs:9380
        assert_eq!(g.takeoff_roll_accel_strong_kt_per_s.wert, 2.5); // lib.rs:9385
        assert_eq!(g.takeoff_roll_n1_pct.wert, 80.0); // lib.rs:9395
        assert_eq!(g.min_fahrt_kt.wert, 30.0); // lib.rs:43856
        assert_eq!(g.takeoff_roll_abort_kt.wert, 20.0); // lib.rs:44033
        assert_eq!(g.takeoff_roll_abort_dwell_secs.wert, 3); // lib.rs:44036
        assert_eq!(g.takeoff_roll_confirm_kt.wert, 45.0); // lib.rs:44044
        assert_eq!(g.takeoff_roll_stall_dwell_secs.wert, 12); // lib.rs:44051
        assert_eq!(g.pushback_dwell_secs.wert, 10); // lib.rs:45413
    }

    #[test]
    fn grenzwerte_landung_wie_2_1_24() {
        let g = &grenzwerte().landung;
        assert_eq!(g.aufsetzen_vor_luecke_max_s.wert, 30); // lib.rs:629
        assert_eq!(g.aufsetzen_nach_letztem_punkt_max_s.wert, 3); // lib.rs:633
        assert_eq!(g.aufsetzen_vor_luecke_max_nm.wert, 1.0); // lib.rs:636
        assert_eq!(g.aufsetz_pruefung_abwarten_s.wert, 5); // lib.rs:666
        assert_eq!(g.touchdown_buffer_secs.wert, 5); // lib.rs:5539
        assert_eq!(g.touchdown_post_window_ms.wert, 10_000); // lib.rs:5545
        assert_eq!(g.td_agl_max_at_touchdown_ft.wert, 5.0); // lib.rs:9736
        assert_eq!(g.td_agl_max_at_window_start_ft.wert, 250.0); // lib.rs:9737
        assert_eq!(g.lua_style_sample_count.wert, 30); // lib.rs:9951
        assert_eq!(g.lua_style_min_samples.wert, 5); // lib.rs:9952
        assert_eq!(g.touchdown_window_secs.wert, 5); // lib.rs:11483
        assert_eq!(g.touchdown_vs_window_ms.wert, 500); // lib.rs:11492
        assert_eq!(g.touchdown_g_window_ms.wert, 800); // lib.rs:11502
        assert_eq!(g.bounce_window_secs.wert, 8); // lib.rs:11507
        assert_eq!(g.bounce_min_dauer_ms.wert, 300); // lib.rs:11520
        assert_eq!(g.bounce_light_min_dauer_ms.wert, 1000); // lib.rs:11527
        assert_eq!(g.bounce_light_min_agl_ft.wert, 2.0); // lib.rs:11532
        assert_eq!(g.bounce_agl_threshold_ft.wert, 5.0); // lib.rs:11565
        assert_eq!(g.bounce_agl_return_ft.wert, 2.0); // lib.rs:11570
        assert_eq!(g.post_td_profile_ms.wert, 10_000); // lib.rs:24392
        assert_eq!(g.g_kanal_steigung_msfs.wert, 0.000_844); // lib.rs:32135
        assert_eq!(g.g_kanal_steigung_xplane.wert, 0.001_942); // lib.rs:32136
        assert_eq!(g.msfs_flare_agl_fit_half_window_ms.wert, 300); // lib.rs:32295
        assert_eq!(g.msfs_flare_agl_fit_min_samples.wert, 4); // lib.rs:32299
        assert_eq!(g.msfs_flare_min_distinct_agl.wert, 8); // lib.rs:32331
        assert_eq!(g.msfs_flare_max_median_dt_ms.wert, 60); // lib.rs:32338
        assert_eq!(g.msfs_flare_min_end_fit_distinct_agl.wert, 4); // lib.rs:32350
        assert_eq!(g.msfs_flare_min_end_agl_span_ft.wert, 0.3); // lib.rs:32364
        assert_eq!(g.msfs_flare_balloon_tol_ft.wert, 1.5); // lib.rs:32371
        assert_eq!(g.msfs_flare_pitch_nosedown_veto_deg.wert, -1.0); // lib.rs:32382
        assert_eq!(g.msfs_flare_detect_floor_fpm.wert, 50.0); // lib.rs:32388
        assert_eq!(g.agl_min_samples.wert, 4); // lib.rs:32840
        assert_eq!(g.agl_min_verschiedene.wert, 3); // lib.rs:32843
        assert_eq!(g.agl_max_stillstand_ms.wert, 120); // lib.rs:32874
        assert_eq!(g.agl_min_spanne_s.wert, 0.15); // lib.rs:32879
        assert_eq!(g.agl_max_plausibel_fpm.wert, -3000.0); // lib.rs:32883
        assert_eq!(g.probe_toleranz_fpm.wert, 20.0); // lib.rs:33150
        assert_eq!(g.sampler_gear_touchdown_threshold_n.wert, 1.0); // lib.rs:40160
        assert_eq!(g.sampler_water_contact_agl_ft.wert, 3.0); // lib.rs:40164
        assert_eq!(g.aufsetzprobe_toleranz_ms.wert, 2_000); // lib.rs:40209
        assert_eq!(g.abfang_fenster_s.wert, 180); // lib.rs:43237
    }

    #[test]
    fn grenzwerte_anflug_kern_wie_2_1_24() {
        let g = &grenzwerte().anflug_kern;
        assert_eq!(g.approach_vs_plausibility_cap_fpm.wert, 10_000.0); // lib.rs:8812
        assert_eq!(g.approach_bank_plausibility_cap_deg.wert, 90.0); // lib.rs:8815
        assert_eq!(g.final_platz_nm.wert, 5.0); // lib.rs:9096
        assert_eq!(g.hat_agl_plausibel_ft.wert, 1200.0); // lib.rs:9118
        assert_eq!(g.hat_agl_vertraut_ft.wert, 600.0); // lib.rs:9122
        assert_eq!(g.approach_stability_agl_cap_ft.wert, 1500.0); // lib.rs:10014
        assert_eq!(g.approach_flare_cutoff_ms.wert, 3000); // lib.rs:10015
        assert_eq!(g.stability_min_samples.wert, 10); // lib.rs:10026
        assert_eq!(g.sampler_stability_window_secs.wert, 180); // lib.rs:10038
        assert_eq!(g.max_vs_dev_floor_ft.wert, 50.0); // lib.rs:10062
        assert_eq!(g.flaps_unreadable_max_ias_kt.wert, 160.0); // lib.rs:10071
        assert_eq!(g.flaps_implausible_below.wert, 0.25); // lib.rs:10077
        assert_eq!(g.flaps_landing_min.wert, 0.70); // lib.rs:10081
        assert_eq!(g.stabilisiert_ueber_vref_kt.wert, 20.0); // lib.rs:10090
        assert_eq!(g.sink_toleranz_ms.wert, 10_000); // lib.rs:10725
        assert_eq!(g.da_sink_toleranz_ms.wert, 5_000); // lib.rs:10731
        assert_eq!(g.da_vref_toleranz_kt.wert, 15.0); // lib.rs:10735
        assert_eq!(g.sink_gs_bezug_kt.wert, 145.0); // lib.rs:10748
        assert_eq!(g.sink_max_luecke_ms.wert, 4000); // lib.rs:10811
        assert_eq!(g.min_bezugsgeschwindigkeit_kt.wert, 25.0); // lib.rs:24003
    }

    #[test]
    fn grenzwerte_ausrollen_wie_2_1_24() {
        let g = &grenzwerte().ausrollen;
        assert_eq!(g.rollout_tick_ms.wert, 200); // lib.rs:251
        assert_eq!(g.rollout_exit_gs_kt.wert, 40.0); // lib.rs:11700
        assert_eq!(g.rollout_stop_gs_kt.wert, 5.0); // lib.rs:11706
        assert_eq!(g.rollout_heading_deviation_deg.wert, 30.0); // lib.rs:11713
        assert_eq!(g.touchdown_vs_severe_fpm.wert, 1000.0); // lib.rs:11750
        assert_eq!(g.touchdown_vs_hard_fpm.wert, 600.0); // lib.rs:11751
        assert_eq!(g.touchdown_vs_firm_fpm.wert, 400.0); // lib.rs:11752
        assert_eq!(g.touchdown_vs_smooth_fpm.wert, 200.0); // lib.rs:11753
        assert_eq!(g.touchdown_g_severe.wert, 2.10); // lib.rs:11755
        assert_eq!(g.touchdown_g_hard.wert, 1.70); // lib.rs:11756
        assert_eq!(g.touchdown_g_firm.wert, 1.40); // lib.rs:11757
        assert_eq!(g.touchdown_g_smooth.wert, 1.20); // lib.rs:11758
        assert_eq!(g.runway_trust_max_centerline_offset_m.wert, 200.0); // lib.rs:20793
        assert_eq!(g.runway_trust_min_float_distance_m.wert, -100.0); // lib.rs:20799
        assert_eq!(g.td_nearest_radius_nm.wert, 25.0); // lib.rs:21558
        assert_eq!(g.bestaetigungs_toleranz_m.wert, 20.0); // lib.rs:24532
        assert_eq!(g.verlauf_behalten_m.wert, 200.0); // lib.rs:25517
        assert_eq!(g.correlate_airport_ambiguity_nm.wert, 1.5); // lib.rs:40515
        assert_eq!(g.correlate_airport_search_radius_nm.wert, 6.0); // lib.rs:40520
        assert_eq!(g.achse_toleranz_m.wert, 5.0); // lib.rs:41082
        assert_eq!(g.schwellenspur_max_agl_ft.wert, 600.0); // lib.rs:41543
        assert_eq!(g.schwellenspur_takt_ms.wert, 400); // lib.rs:41546
        assert_eq!(g.schwellenspur_secs.wert, 60); // lib.rs:41549
        assert_eq!(g.tch_max_klammer_m.wert, 150.0); // lib.rs:41576
        assert_eq!(g.bahn_kurs_ausfahrt_grad.wert, 10.0); // lib.rs:42117
        assert_eq!(g.bahn_ausfahrt_max_gs_kt.wert, 90.0); // lib.rs:42127
        assert_eq!(g.bahn_ausfahrt_beginn_grad.wert, 3.0); // lib.rs:42142
        assert_eq!(g.bahn_mess_min_gs_kt.wert, 60.0); // lib.rs:42204
        assert_eq!(g.bahn_mess_min_gs_kt_boden.wert, 25.0); // lib.rs:42208
        assert_eq!(g.bahn_mess_schwelle_faktor.wert, 0.6); // lib.rs:42212
        assert_eq!(g.bahn_spur_min_abstand_m.wert, 10.0); // lib.rs:42250
        assert_eq!(g.bahn_spur_quer_gewicht.wert, 4.0); // lib.rs:42262
        assert_eq!(g.bahn_spur_max_punkte.wert, 400); // lib.rs:42269
        assert_eq!(g.bahn_richtung_fenster_m.wert, 100.0); // lib.rs:42272
        assert_eq!(g.bahn_richtung_min_versatz_m.wert, 2.0); // lib.rs:42278
        assert_eq!(g.bahn_spur_rand_m.wert, 80.0); // lib.rs:42348
        assert_eq!(g.bahn_spur_stop_gs_kt.wert, 5.0); // lib.rs:42354
    }

    #[test]
    fn grenzwerte_sprit_wie_2_1_24() {
        let g = &grenzwerte().sprit;
        assert_eq!(g.sprit_schwelle_ueber_platz_ft.wert, 8_000.0); // lib.rs:22490
        assert_eq!(g.sprit_schwelle_abstand_zur_reisehoehe_ft.wert, 1_000.0); // lib.rs:22491
        assert_eq!(g.sprit_tick_max_s.wert, 60.0); // lib.rs:22494
        assert_eq!(g.sprit_projektion_max_abstand_nm.wert, 25.0); // lib.rs:22501
        assert_eq!(g.sprit_projektion_gleichstand_nm.wert, 1.0); // lib.rs:22506
        assert_eq!(g.sprit_tank_min_anteil.wert, 0.01); // lib.rs:22512
        assert_eq!(g.sprit_sinkflug_unter_reisehoehe_ft.wert, 1_500.0); // lib.rs:22525
        assert_eq!(g.sprit_triebwerke_aus_s.wert, 10.0); // lib.rs:23014
        assert_eq!(g.sprit_wp_nah_nm.wert, 5.0); // lib.rs:23035
        assert_eq!(g.ff_derive_window_s.wert, 60.0); // lib.rs:35790
        assert_eq!(g.ff_max_kg_per_h.wert, 30_000.0); // lib.rs:35792
        assert_eq!(g.fuel_used_simvar_alive_kg.wert, 0.5); // lib.rs:35810
        assert_eq!(g.fuel_used_min_delta_kg.wert, 5.0); // lib.rs:35813
        assert_eq!(g.fuel_used_refuel_relatch_kg.wert, 50.0); // lib.rs:35817
        assert_eq!(g.fuel_used_defuel_relatch_kg.wert, 200.0); // lib.rs:35824
        assert_eq!(g.toleranz_kg.wert, 50.0); // lib.rs:44240
        assert_eq!(g.rueckfall_kg_pro_h.wert, 2_000.0); // lib.rs:44295
        assert_eq!(g.snapshot_hoehe_min_ft.wert, -2_000.0); // lib.rs:44309
        assert_eq!(g.snapshot_hoehe_max_ft.wert, 150_000.0); // lib.rs:44313
        assert_eq!(g.defuel_threshold_kg.wert, 200.0); // lib.rs:44939
    }

    #[test]
    fn grenzwerte_abgabe_wie_2_1_24() {
        let g = &grenzwerte().abgabe;
        assert_eq!(g.resume_stillstand_max_hours.wert, 24); // lib.rs:193
        assert_eq!(g.divert_prefetch_max_agl_ft.wert, 5000.0); // lib.rs:1227
        assert_eq!(g.divert_prefetch_max_dist_nm.wert, 10.0); // lib.rs:1232
        assert_eq!(g.divert_prefetch_min_sink_fpm.wert, -200.0); // lib.rs:1235
        assert_eq!(g.divert_prefetch_stable_secs.wert, 10); // lib.rs:1239
        assert_eq!(g.divert_prefetch_planned_diverge_nm.wert, 5.0); // lib.rs:1244
        assert_eq!(g.resume_drift_toast_nm.wert, 1.0); // lib.rs:1291
        assert_eq!(g.resume_drift_warn_nm.wert, 50.0); // lib.rs:1292
        assert_eq!(g.resume_drift_extreme_nm.wert, 200.0); // lib.rs:1293
        assert_eq!(g.resume_fuel_jump_impossible_kg.wert, 200.0); // lib.rs:1302
        assert_eq!(g.resume_zielsprung_vorher_nm.wert, 15.0); // lib.rs:1316
        assert_eq!(g.resume_zielsprung_nachher_nm.wert, 5.0); // lib.rs:1317
        assert_eq!(g.resume_max_plausibel_kt.wert, 700.0); // lib.rs:1321
        assert_eq!(g.abgabe_unterbrechung_min_secs.wert, 120); // lib.rs:1475
        assert_eq!(g.resume_drift_clean_nm.wert, 5.0); // lib.rs:1896
        assert_eq!(g.max_start_distance_nm.wert, 5.0); // lib.rs:1901
        assert_eq!(g.max_file_distance_nm.wert, 5.0); // lib.rs:1908
        assert_eq!(g.pilot_confirmed_arrival_max_nm.wert, 15.0); // lib.rs:1925
        assert_eq!(g.standing_on_other_field_nm.wert, 1.0); // lib.rs:1939
        assert_eq!(g.min_flight_time_min.wert, 1); // lib.rs:19708
        assert_eq!(g.min_distance_nm.wert, 1.0); // lib.rs:19709
        assert_eq!(g.min_position_count.wert, 5); // lib.rs:19710
        assert_eq!(g.reset_null_island_deg.wert, 0.1); // lib.rs:33959
        assert_eq!(g.reset_max_alt_jump_ft.wert, 10_000.0); // lib.rs:33960
        assert_eq!(g.simrate_strecke_max.wert, 16.0); // lib.rs:44397
        assert_eq!(g.zeitraffer_gs_spielraum.wert, 1.5); // lib.rs:44403
        assert_eq!(g.sim_referenz_fenster_s.wert, 4); // lib.rs:45944
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
