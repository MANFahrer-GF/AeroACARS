//! AeroACARS landing-score Single-Source-of-Truth Crate.
//!
//! v0.7.1 Phase 0: 1:1-Port von `client/src/lib/landingScoring.ts`.
//! Per Spec docs/spec/v0.7.1-landing-ux-fairness.md §3.1: alle
//! Konsumenten (Backend lib.rs, Tauri-Frontend, aeroacars-live
//! webapp + monitor) nutzen identische Sub-Scores aus dieser
//! Crate. UI rendert ausschliesslich die hier produzierten
//! `SubScoreEntry`-Werte aus dem PIREP-Payload — KEIN Recompute.
//!
//! Phase 0 portiert die Legacy-Algorithmen 1:1 (Goldenset blockiert
//! Drift > 0.5 Punkte gegen TS). Phase 2 fuehrt die Asymmetrie und
//! Skip-Logik ein (F2/F3). Phase 3 baut sub_stability auf 4-Faktor-
//! Voting um (F7-B).

use serde::{Deserialize, Serialize};

pub mod gate;
pub mod sub_bounces;
pub mod sub_fuel;
pub mod sub_g_force;
pub mod sub_landing_rate;
pub mod sub_loadsheet;
pub mod sub_rollout;
pub mod sub_stability;

/// Score-Band — 1:1 aus TS `Band`. NICHT umbenennen, bestehende UI
/// erwartet exakt diese Werte (siehe Spec §5.4 K1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Band {
    Good,
    Ok,
    Bad,
    Skipped,
}

impl Band {
    /// Wire-Format als String fuer JSON/Serde — matched die TS-
    /// Enum-Werte 1:1 ("good" | "ok" | "bad" | "skipped").
    pub fn as_str(self) -> &'static str {
        match self {
            Band::Good => "good",
            Band::Ok => "ok",
            Band::Bad => "bad",
            Band::Skipped => "skipped",
        }
    }
}

/// Konvertiert Score-Punkte → Band. Identisch zu TS `band(points)`
/// in landingScoring.ts:138-142.
pub fn band_from_points(points: u8) -> Band {
    if points >= 75 {
        Band::Good
    } else if points >= 45 {
        Band::Ok
    } else {
        Band::Bad
    }
}

/// Sub-Score Wire-Format. Spec §5.4 (P1.5-A): voll ausgebaut auf
/// alle Render-Felder, damit Web/Monitor ohne Recompute bit-identisch
/// rendern koennen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubScoreEntry {
    /// Stabile Schluessel — siehe TS-Type SubScore["key"] +
    /// neue v0.7.1-Schluessel "loadsheet" (F1) + "flare" (F6).
    pub key: String,
    /// 0-100, gerundet
    pub score: u8,
    /// Alias fuer score — bestehende UI nutzt .points
    pub points: u8,
    /// "good" | "ok" | "bad" | "skipped" (siehe Band)
    pub band: String,
    /// i18n-Key z.B. "landing.sub.fuel"
    pub label_key: String,
    /// formatiert: "-191 fpm", "1.32 g", "+5.2 %"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// i18n-Key z.B. "landing.rat.smooth_touchdown"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale_key: Option<String>,
    /// i18n-Key z.B. "landing.tip.firm_touchdown"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tip_key: Option<String>,
    /// true wenn Sub-Score nicht bewertet wurde (z.B. VFR ohne ZFW)
    pub skipped: bool,
    /// Skip-Reason ("no_planned_burn", "no_actual_burn", ...)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Optionale Warnung (z.B. "planned_burn_may_be_off" bei
    /// implausibel-hohem Minderverbrauch — Phase 2/F3)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// v0.10.0 (#runway-utilization-score): Zusatz-Display-Zeilen für die
    /// UI-Card (z.B. „davon ~520 m Float vor Aufsetzen", „Bahn: YMML 16,
    /// LDA 3657 m"). Renderer alter Versionen ignorieren das Feld
    /// schweigend (forward-compat). Renderer v0.10+ die ein Payload ohne
    /// `extra` bekommen, behandeln es als leere Liste (`#[serde(default)]`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub extra: Vec<String>,
}

impl SubScoreEntry {
    /// Hilfs-Konstruktor fuer skipped Sub-Scores.
    pub fn skipped(key: &str, label_key: &str, reason: &str) -> Self {
        Self {
            key: key.to_string(),
            score: 0,
            points: 0,
            band: Band::Skipped.as_str().to_string(),
            label_key: label_key.to_string(),
            value: None,
            rationale_key: None,
            tip_key: None,
            skipped: true,
            reason: Some(reason.to_string()),
            warning: None,
            extra: Vec::new(),
        }
    }

    /// Hilfs-Konstruktor fuer bewertete Sub-Scores. Setzt
    /// `points = score`, `band` aus `band_from_points`, `tip_key
    /// = label_key + "tip." + rationale` Konvention.
    pub fn scored(
        key: &str,
        label_key: &str,
        score: u8,
        value: String,
        rationale: &str,
        band: Band,
    ) -> Self {
        Self {
            key: key.to_string(),
            score,
            points: score,
            band: band.as_str().to_string(),
            label_key: label_key.to_string(),
            value: Some(value),
            rationale_key: Some(format!("landing.rat.{}", rationale)),
            tip_key: Some(format!("landing.tip.{}", rationale)),
            skipped: false,
            reason: None,
            warning: None,
            extra: Vec::new(),
        }
    }

    /// v0.10.0 Builder: hängt Display-Extra-Zeilen an (LE12). Wird vom
    /// `sub_rollout_v2` genutzt um Float-/Bahn-Info unter die Rationale
    /// zu setzen. No-op wenn `extra` leer ist.
    pub fn with_extra(mut self, extra: Vec<String>) -> Self {
        self.extra = extra;
        self
    }

    /// v0.10.0 Builder: setzt das Warning-Feld (existiert bereits im
    /// Wire-Format, kriegt in v0.10.0 nur einen neuen Wert
    /// `pre_displaced_threshold`). `None` löscht eine evtl. vorher
    /// gesetzte Warning.
    pub fn with_warning(mut self, warning: Option<String>) -> Self {
        self.warning = warning;
        self
    }
}

/// Eingabe-Struktur fuer `compute_sub_scores`. Spiegel der TS-Signatur
/// von `computeSubScores(p)` in landingScoring.ts:218-238 plus die
/// in Phase 2+ benoetigten Felder fuer F1 (loadsheet) und F6 (flare).
///
/// Felder mit `Option<>` werden als "nicht vorhanden" interpretiert
/// und produzieren entweder den Default-Pfad (z.B. `bounce_count
/// .unwrap_or(0)`) oder einen skipped Sub-Score (Phase 2: fuel/
/// loadsheet).
#[derive(Debug, Clone, Default)]
pub struct LandingScoringInput {
    pub vs_fpm: Option<f32>,
    /// Roher 50-Hz-Einzelframe-G-Peak. **Forensik / Backward-Fallback** —
    /// `sub_g_force` scort dies NICHT mehr direkt, sondern `scored_g_load`
    /// falls vorhanden (v0.12.3 LE7/LE8).
    pub peak_g_load: Option<f32>,
    /// v0.12.3 (LE8): EMA-geglätteter Fenster-Peak (FOQA-Methode). Wenn
    /// gesetzt, scort `sub_g_force` diesen Wert; sonst Fallback auf
    /// `peak_g_load`. `None` bei pre-v0.12.3-Callern → identisches
    /// Alt-Verhalten (Roh-Peak).
    pub scored_g_load: Option<f32>,
    pub bounce_count: Option<u32>,
    pub approach_vs_stddev_fpm: Option<f32>,
    pub approach_bank_stddev_deg: Option<f32>,
    pub rollout_distance_m: Option<f32>,
    pub fuel_efficiency_pct: Option<f32>,
    // Phase 2 (F1 + F2 + F3): VFR/ZFW + Fuel-Asymmetrie
    pub planned_zfw_kg: Option<f32>,
    pub planned_tow_kg: Option<f32>,
    pub planned_burn_kg: Option<f32>,
    pub actual_trip_burn_kg: Option<f32>,
    // Phase 3 hook (Flare-Sub-Score kommt in Phase 3/F6).
    pub flare_quality_score: Option<u8>,
    /// v0.7.17 (N-002): ICAO type designator des geflogenen
    /// Aircraft (z.B. "A320", "B738", "C172"). Wird vom `sub_rollout`-
    /// Score genutzt um die Bahn-Auslastung-Schwellen aircraft-
    /// kategorie-abhaengig zu staffeln — vorher feuerte jeder Airliner
    /// einen „long_rollout"-Score (25 Pkt) selbst bei voellig normalen
    /// 2 km auf einer 3 km Bahn. None heisst „unbekannt" → konservative
    /// (medium-light) Schwellen werden genutzt.
    pub aircraft_icao: Option<String>,
    // ─── v0.10.0 Runway-Utilization-Score (Spec docs/spec/v0.10.0-
    // runway-utilization-score.md) ──────────────────────────────────
    //
    // Wenn ALLE folgenden Felder Some sind UND alle Skip-Gates pass
    // (siehe sub_rollout::sub_rollout_v2), wird der NEUE Algorithmus
    // (LDA-basiert, td+rollout/lda) verwendet und im PIREP/Touchdown
    // mit `score_algorithm_version` markiert (v0.16.21: Some(4) nach
    // MSFS-Touchdown-V/S-g-Delag; v0.12.0–v0.16.20: Some(3) Float-
    // Toleranz-Refinement; vor v0.12.0: Some(2)). Wenn
    // eines fehlt → Skip mit konkretem Reason (KEIN Fallback auf v1).
    //
    // Backward-Compat: Wenn aufrufender Code die Felder None lässt,
    // ruft `compute_sub_scores` weiterhin den alten `sub_rollout` als
    // Fallback (= identisch zu v0.9.x-Verhalten).
    pub td_distance_from_threshold_m: Option<f64>,
    pub landing_float_distance_m: Option<f32>,
    pub runway_length_m: Option<f32>,
    pub runway_displaced_threshold_ft: Option<i32>,
    pub pre_displaced_threshold: Option<bool>,
    pub runway_geometry_trusted: Option<bool>,
    pub airport_source: Option<String>,
    pub runway_match_icao: Option<String>,
    pub runway_match_ident: Option<String>,
}

/// Berechnet alle Sub-Scores.
///
/// v0.7.1 Phase 2: nutzt `sub_fuel_v0_7_1` (F2 Hard-Gate + F3
/// Asymmetrie) und `sub_loadsheet` (F1 VFR-Skip). Stability bleibt
/// im 2-Faktor-Modus bis Phase 3 F7-B aktiviert (siehe Spec §5.5
/// Backward-Compat-Test 7.2.1).
/// v0.10.0 — Sentinel: ist mindestens ein v2-Feld gesetzt? Wenn ja
/// laufen wir den neuen `sub_rollout_v2`-Pfad (auch wenn der dann
/// skipped). Wenn nein → alter `sub_rollout` (Backward-Compat für
/// pre-v0.10-Caller wie Bin-Tools oder Tests).
///
/// Wir prüfen die DENOMINATOR-Bezogenen Felder (Bahn-Länge / Geometry-
/// Trust / Airport-Source) — die sind erst ab v0.8.x überhaupt da. Wenn
/// hier nichts steht, ist der Caller definitiv pre-v0.10.
fn scoring_input_has_v2_fields(input: &LandingScoringInput) -> bool {
    input.runway_length_m.is_some()
        || input.runway_geometry_trusted.is_some()
        || input.airport_source.is_some()
        || input.td_distance_from_threshold_m.is_some()
}

pub fn compute_sub_scores(input: &LandingScoringInput) -> Vec<SubScoreEntry> {
    let mut out = Vec::with_capacity(8);

    if let Some(vs) = input.vs_fpm {
        out.push(sub_landing_rate::sub_landing_rate(vs));
    }
    // v0.12.3 (LE8): score the EMA-smoothed `scored_g_load` when present;
    // fall back to the raw `peak_g_load` for pre-v0.12.3 callers.
    if let Some(g) = input.scored_g_load.or(input.peak_g_load) {
        out.push(sub_g_force::sub_g_force(g));
    }
    out.push(sub_bounces::sub_bounces(input.bounce_count.unwrap_or(0)));

    if let Some(stab) = sub_stability::sub_stability_legacy(
        input.approach_vs_stddev_fpm,
        input.approach_bank_stddev_deg,
    ) {
        out.push(stab);
    }
    // v0.10.0 (#runway-utilization-score): Wenn die v2-Datenlage da ist,
    // wird der neue LDA-basierte Sub-Score gerechnet (auch bei
    // Skip-Outcomes wie `missing_td_distance`). Der Caller markiert die
    // Records dann mit `score_algorithm_version` (v0.16.21: Some(4) nach
    // MSFS-Touchdown-V/S-g-Delag; v0.12.0–v0.16.20: Some(3) Float-Toleranz-
    // Refinement; vor v0.12.0: Some(2)).
    //
    // Wenn keines der v2-Felder gesetzt ist (= Caller ist nicht-migriert
    // oder Test-Fixture ohne Touchdown-Forensik), fallen wir auf den
    // alten meter-only `sub_rollout` zurück damit pre-v0.10 Code-Pfade
    // (Bin-Tools, alte Tests) unverändert weiterlaufen.
    if scoring_input_has_v2_fields(input) {
        out.push(sub_rollout::sub_rollout_v2(&sub_rollout::RolloutInput {
            td_distance_from_threshold_m: input.td_distance_from_threshold_m,
            rollout_distance_m: input.rollout_distance_m,
            landing_float_distance_m: input.landing_float_distance_m,
            runway_length_m: input.runway_length_m,
            runway_displaced_threshold_ft: input.runway_displaced_threshold_ft,
            pre_displaced_threshold: input.pre_displaced_threshold,
            runway_geometry_trusted: input.runway_geometry_trusted,
            airport_source: input.airport_source.as_deref(),
            runway_match_icao: input.runway_match_icao.as_deref(),
            runway_match_ident: input.runway_match_ident.as_deref(),
            aircraft_icao: input.aircraft_icao.as_deref(),
        }));
    } else if let Some(ro) =
        sub_rollout::sub_rollout(input.rollout_distance_m, input.aircraft_icao.as_deref())
    {
        out.push(ro);
    }

    // v0.7.1 Phase 2 F2 + F3: ersetzt sub_fuel_legacy durch
    // sub_fuel_v0_7_1 mit Hard-Gate + Asymmetrie. Wenn weder
    // planned_burn noch actual_trip_burn vorhanden → skipped (NICHT
    // in den Master-Score eingerechnet).
    out.push(sub_fuel::sub_fuel_v0_7_1(
        input.planned_burn_kg,
        input.actual_trip_burn_kg,
    ));

    // v0.7.1 Phase 2 F1: NEU sub_loadsheet. VFR/Manual-Mode ohne
    // Dispatch-Daten → skipped (planned_zfw/tow None). Sonst Score 100
    // als Phase-2-Placeholder; Phase 3 wird actuelle Mass-Schwellen.
    out.push(sub_loadsheet::sub_loadsheet(
        input.planned_zfw_kg,
        input.planned_tow_kg,
    ));

    out
}

/// Master-Score-Aggregation — gewichteter Mittelwert. Spec §5.5
/// (P1.4-B): bestehende Gewichte beibehalten, skipped Sub-Scores
/// aus Summe UND Gewichtssumme entfernen.
///
/// 1:1-Spiegel von TS `aggregateSubScores` in landingScoring.ts:292-309
/// plus Phase-1-Erweiterung um neue Sub-Score-Schluessel "loadsheet"
/// (Gewicht 1) und "flare" (Gewicht 1) — siehe Spec §5.5 Tabelle.
pub fn aggregate_master_score(subs: &[SubScoreEntry]) -> Option<u8> {
    if subs.is_empty() {
        return None;
    }
    let mut sum: f32 = 0.0;
    let mut wsum: f32 = 0.0;
    for s in subs {
        if s.skipped {
            continue; // Skip aus Summe UND Gewichtssumme
        }
        let w = match s.key.as_str() {
            "landing_rate" => 3.0,
            "g_force" => 3.0,
            "bounces" => 2.0,
            "stability" => 2.0,
            "rollout" => 1.0,
            "fuel" => 1.0,
            "loadsheet" => 1.0, // NEU v0.7.1
            "flare" => 1.0,     // NEU v0.7.1
            _ => 1.0,           // unbekannt → default 1
        };
        sum += s.score as f32 * w;
        wsum += w;
    }
    if wsum > 0.0 {
        Some((sum / wsum).round() as u8)
    } else {
        None
    }
}

/// Landing-Kategorie-Klassifikation — Spiegel von TS `classifyLanding`
/// in landingScoring.ts:280-290. Wird vom Backend lib.rs `LandingScore::
/// classify` parallel verwendet (siehe lib.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LandingCategory {
    Smooth,
    Acceptable,
    Firm,
    Hard,
    Severe,
}

impl LandingCategory {
    pub fn numeric(self) -> u8 {
        match self {
            LandingCategory::Smooth => 100,
            LandingCategory::Acceptable => 80,
            LandingCategory::Firm => 60,
            LandingCategory::Hard => 30,
            LandingCategory::Severe => 0,
        }
    }

    fn order(self) -> u8 {
        match self {
            LandingCategory::Smooth => 0,
            LandingCategory::Acceptable => 1,
            LandingCategory::Firm => 2,
            LandingCategory::Hard => 3,
            LandingCategory::Severe => 4,
        }
    }

    fn worse_of(a: LandingCategory, b: LandingCategory) -> LandingCategory {
        if a.order() >= b.order() {
            a
        } else {
            b
        }
    }

    fn bump_up(self) -> LandingCategory {
        match self {
            LandingCategory::Smooth => LandingCategory::Acceptable,
            LandingCategory::Acceptable => LandingCategory::Firm,
            LandingCategory::Firm => LandingCategory::Hard,
            LandingCategory::Hard | LandingCategory::Severe => LandingCategory::Severe,
        }
    }
}

fn classify_by_vs(peak_vs_fpm: f32) -> LandingCategory {
    let vs = peak_vs_fpm.abs();
    if vs >= sub_landing_rate::T_VS_SEVERE_FPM {
        LandingCategory::Severe
    } else if vs >= sub_landing_rate::T_VS_HARD_FPM {
        LandingCategory::Hard
    } else if vs >= sub_landing_rate::T_VS_FIRM_FPM {
        LandingCategory::Firm
    } else if vs >= sub_landing_rate::T_VS_SMOOTH_FPM {
        LandingCategory::Acceptable
    } else {
        LandingCategory::Smooth
    }
}

fn classify_by_g(peak_g: f32) -> LandingCategory {
    if peak_g >= sub_g_force::T_G_SEVERE {
        LandingCategory::Severe
    } else if peak_g >= sub_g_force::T_G_HARD {
        LandingCategory::Hard
    } else if peak_g >= sub_g_force::T_G_FIRM {
        LandingCategory::Firm
    } else if peak_g >= sub_g_force::T_G_SMOOTH {
        LandingCategory::Acceptable
    } else {
        LandingCategory::Smooth
    }
}

pub fn classify_landing(
    peak_vs_fpm: f32,
    peak_g: Option<f32>,
    bounces: u32,
) -> LandingCategory {
    let by_vs = classify_by_vs(peak_vs_fpm);
    let by_g = peak_g.map(classify_by_g).unwrap_or(LandingCategory::Smooth);
    // v0.7.17 (B-009): V/S-led classification.
    //
    // Vorher: `worse_of(by_vs, by_g)` — fuehrte zu falschen „Severe"-
    // Klassifikationen bei butterweichen Touchdowns, sobald die Sim-
    // Strut-Compression einen einzelnen G-Spike >2.10 produzierte.
    // Echtes Pilot-Performance-Signal ist die vertikale Sinkrate (V/S)
    // — das was der Pilot beim Aufsetzen fuehlt und was Wartungs-
    // bestimmungen tatsaechlich definieren (-600 fpm = "Hard").
    //
    // Neue Logik: V/S fuehrt. G darf die Klassifikation maximal um
    // EINE Stufe verschlechtern (= Indikator fuer ungewohnt harte
    // Strut-Compression), aber nie alleine zu Severe fuehren bei
    // smoother V/S. Bei echten Hard-Impacts (V/S >= -600 fpm)
    // dominiert das schlechtere Signal wie bisher.
    let mut cat = match (by_vs, by_g) {
        // V/S = Smooth: G kann maximal um eine Stufe runter
        (LandingCategory::Smooth, LandingCategory::Hard)
        | (LandingCategory::Smooth, LandingCategory::Severe) => LandingCategory::Acceptable,
        (LandingCategory::Smooth, _) => LandingCategory::Smooth,

        // V/S = Acceptable: G kann maximal um eine Stufe runter
        (LandingCategory::Acceptable, LandingCategory::Hard)
        | (LandingCategory::Acceptable, LandingCategory::Severe) => LandingCategory::Firm,
        (LandingCategory::Acceptable, _) => LandingCategory::Acceptable,

        // V/S = Firm: G kann maximal um eine Stufe runter
        (LandingCategory::Firm, LandingCategory::Severe) => LandingCategory::Hard,
        (LandingCategory::Firm, _) => LandingCategory::Firm,

        // V/S = Hard/Severe: echter Impact, worse_of greift wie vorher
        (LandingCategory::Hard, _) | (LandingCategory::Severe, _) => {
            LandingCategory::worse_of(by_vs, by_g)
        }
    };
    if bounces > 0 && cat != LandingCategory::Severe {
        cat = cat.bump_up();
    }
    cat
}

#[cfg(test)]
mod tests {
    use super::*;

    /// v0.12.3 (LE8): `sub_g_force` scores `scored_g_load` when present,
    /// else falls back to the raw `peak_g_load`.
    #[test]
    fn sub_g_force_uses_scored_g() {
        let g_value = |input: &LandingScoringInput| -> String {
            compute_sub_scores(input)
                .into_iter()
                .find(|s| s.key == "g_force")
                .and_then(|s| s.value)
                .expect("g_force sub-score present")
        };
        // scored_g present → it is scored, not the raw peak.
        let with_scored = LandingScoringInput {
            peak_g_load: Some(1.95),
            scored_g_load: Some(1.78),
            ..Default::default()
        };
        assert_eq!(g_value(&with_scored), "1.78 G");
        // No scored_g (pre-v0.12.3 caller) → falls back to the raw peak.
        let legacy = LandingScoringInput {
            peak_g_load: Some(1.95),
            scored_g_load: None,
            ..Default::default()
        };
        assert_eq!(g_value(&legacy), "1.95 G");
    }

    #[test]
    fn band_thresholds_match_ts() {
        assert_eq!(band_from_points(100), Band::Good);
        assert_eq!(band_from_points(75), Band::Good);
        assert_eq!(band_from_points(74), Band::Ok);
        assert_eq!(band_from_points(45), Band::Ok);
        assert_eq!(band_from_points(44), Band::Bad);
        assert_eq!(band_from_points(0), Band::Bad);
    }

    #[test]
    fn aggregate_master_uses_weighted_mean() {
        // landing_rate=100×3 + g_force=80×3 + bounces=70×2 + stability=50×2
        //   = 300 + 240 + 140 + 100 = 780
        // wsum = 3+3+2+2 = 10
        // → 78
        let subs = vec![
            SubScoreEntry::scored(
                "landing_rate",
                "landing.sub.landing_rate",
                100,
                "-50 fpm".into(),
                "smooth_touchdown",
                Band::Good,
            ),
            SubScoreEntry::scored(
                "g_force",
                "landing.sub.g_force",
                80,
                "1.30 G".into(),
                "comfortable_g",
                Band::Good,
            ),
            SubScoreEntry::scored(
                "bounces",
                "landing.sub.bounces",
                70,
                "1".into(),
                "one_bounce",
                Band::Ok,
            ),
            SubScoreEntry::scored(
                "stability",
                "landing.sub.stability",
                50,
                "σ 250 fpm / 4.0°".into(),
                "average_stability",
                Band::Ok,
            ),
        ];
        assert_eq!(aggregate_master_score(&subs), Some(78));
    }

    #[test]
    fn aggregate_master_skips_skipped_subs() {
        // Skip senkt nicht den Master-Score
        let subs = vec![
            SubScoreEntry::scored(
                "landing_rate",
                "landing.sub.landing_rate",
                100,
                "-50 fpm".into(),
                "smooth_touchdown",
                Band::Good,
            ),
            SubScoreEntry::skipped("loadsheet", "landing.sub.loadsheet", "no_planned_zfw"),
        ];
        assert_eq!(aggregate_master_score(&subs), Some(100));
    }

    #[test]
    fn classify_landing_bumps_for_bounces() {
        // smooth + 1 bounce → acceptable
        let cat = classify_landing(-100.0, Some(1.10), 1);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    #[test]
    fn classify_landing_severe_does_not_bump() {
        // severe + bounce bleibt severe
        let cat = classify_landing(-1100.0, None, 1);
        assert_eq!(cat, LandingCategory::Severe);
    }

    #[test]
    fn classify_landing_vs_led_smooth_vs_severe_g() {
        // v0.7.17 (B-009): V/S-led classification.
        // Vorher worse_of(Smooth, Severe) = Severe.
        // Jetzt V/S Smooth fuehrt, G darf maximal 1 Stufe runter → Acceptable.
        let cat = classify_landing(-50.0, Some(2.20), 0);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    // v0.7.17 (B-009) — V/S-led classification tests
    #[test]
    fn b009_butter_landing_with_sim_strut_spike_stays_smooth() {
        // Real-Tester-Case GSG 105: VS -116 fpm (Smooth) + G 2.30 (Severe)
        // wegen Sim-Strut-Compression. Vorher: worse_of -> Severe.
        // Jetzt: Smooth gefuehrt von V/S, G nur 1-Stufen-Downgrade -> Acceptable.
        let cat = classify_landing(-116.0, Some(2.30), 0);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    #[test]
    fn b009_smooth_vs_with_hard_g_only_one_step_down() {
        let cat = classify_landing(-150.0, Some(1.80), 0);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    #[test]
    fn b009_smooth_vs_with_smooth_g_stays_smooth() {
        let cat = classify_landing(-100.0, Some(1.10), 0);
        assert_eq!(cat, LandingCategory::Smooth);
    }

    #[test]
    fn b009_real_hard_impact_still_classified_correctly() {
        // V/S Hard (-700 fpm), G Severe (2.50): worse_of greift, bleibt Severe
        let cat = classify_landing(-700.0, Some(2.50), 0);
        assert_eq!(cat, LandingCategory::Severe);
    }

    #[test]
    fn b009_acceptable_vs_with_severe_g_one_step_down() {
        // V/S Acceptable (-300 fpm), G Severe (2.30) -> Firm
        let cat = classify_landing(-300.0, Some(2.30), 0);
        assert_eq!(cat, LandingCategory::Firm);
    }

    #[test]
    fn b009_firm_vs_with_severe_g_one_step_down() {
        // V/S Firm (-500 fpm), G Severe (2.30) -> Hard
        let cat = classify_landing(-500.0, Some(2.30), 0);
        assert_eq!(cat, LandingCategory::Hard);
    }

    #[test]
    fn b009_bounces_still_bump_up_one_stufe() {
        // Smooth + bounce -> Acceptable (bump_up greift weiterhin)
        let cat = classify_landing(-100.0, Some(1.10), 1);
        assert_eq!(cat, LandingCategory::Acceptable);
    }

    /// v0.16.22 FIX 5 — dormant-flare-weight guardrail. `aggregate_master_
    /// score` carries a live `"flare" => 1.0` weight, but `compute_sub_
    /// scores` must NEVER emit a `"flare"` SubScoreEntry — the flare metric
    /// is forensic/coaching-only and stays OUT of the master score (Landing-
    /// Score is private; no scored flare ranking — see ACARS-focus policy).
    /// This test fails loudly if a future edit accidentally wires flare into
    /// the scored set, so the dormant weight can never silently activate.
    /// Run over a FULLY-populated input so every sub-score branch fires.
    #[test]
    fn compute_sub_scores_never_emits_flare() {
        let rich = LandingScoringInput {
            vs_fpm: Some(-150.0),
            peak_g_load: Some(1.4),
            scored_g_load: Some(1.3),
            bounce_count: Some(1),
            approach_vs_stddev_fpm: Some(120.0),
            approach_bank_stddev_deg: Some(3.0),
            rollout_distance_m: Some(1500.0),
            fuel_efficiency_pct: Some(2.0),
            planned_zfw_kg: Some(60_000.0),
            planned_tow_kg: Some(72_000.0),
            planned_burn_kg: Some(5_000.0),
            actual_trip_burn_kg: Some(5_100.0),
            // Set the flare-quality input HIGH: if a `flare` sub-score were
            // ever (wrongly) wired on this field, it would emit here.
            flare_quality_score: Some(100),
            aircraft_icao: Some("A320".into()),
            td_distance_from_threshold_m: Some(400.0),
            landing_float_distance_m: Some(200.0),
            runway_length_m: Some(3000.0),
            runway_displaced_threshold_ft: Some(0),
            pre_displaced_threshold: Some(false),
            runway_geometry_trusted: Some(true),
            airport_source: Some("ourairports".into()),
            runway_match_icao: Some("EDDM".into()),
            runway_match_ident: Some("08R".into()),
        };
        let subs = compute_sub_scores(&rich);
        assert!(
            !subs.iter().any(|s| s.key == "flare"),
            "compute_sub_scores must NEVER emit a `flare` sub-score (flare is forensic-only); got keys {:?}",
            subs.iter().map(|s| s.key.as_str()).collect::<Vec<_>>()
        );
        // Also assert over a bare/default input (no fields) — the empty path
        // must not emit flare either.
        let bare = compute_sub_scores(&LandingScoringInput::default());
        assert!(!bare.iter().any(|s| s.key == "flare"));
    }
}
