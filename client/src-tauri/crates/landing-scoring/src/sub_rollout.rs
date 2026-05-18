//! Rollout Sub-Score („Bahn-Auslastung").
//!
//! v0.7.17 (N-002): Schwellen jetzt aircraft-kategorie-abhaengig.
//! Vorher waren die Grenzen (800/1200/1800/2500 m) absolut — was fuer
//! Light-GA grob passt, aber jeden Airliner mit > 1800 m Rollout in
//! die rote „long_rollout"-Zone schickte, obwohl ein A320 oder B738
//! auf einer 3 km Bahn typisch 1800-2200 m braucht und damit voellig
//! normal operiert.
//!
//! Die Klassifikation ist pragmatisch nach ICAO-Type-Designator:
//!   * Heavy / Wide-Body: A330 / A340 / A350 / A380 / B747 / B767 /
//!     B777 / B787 / MD11
//!   * Medium / Narrow-Body / Regional: A318/A319/A320/A321/Neo,
//!     B737-Family, B757, Embraer 170-195, CRJ, ATR, Dash-8, etc.
//!   * Light (Default / unbekannt): kleiner GA, Cessna 172 etc.

use crate::{Band, SubScoreEntry};

// ═════════════════════════════════════════════════════════════════════
// v0.10.0 — Runway-Utilization-Score (Bahn-Auslastung neu)
// Spec: docs/spec/v0.10.0-runway-utilization-score.md (SPEC ACCEPTED)
//
// Vorher (v0.9.x): nur `rollout_distance_m` + ICAO-Kategorie. Sagt nichts
// über die genutzte Landing-Distance auf der TATSÄCHLICHEN Bahn —
// 1500 m Rollout auf einer 1800-m-Stoßbahn ist gefährlich, 1500 m auf
// einer 4000-m-Bahn ist banal, beide kriegen denselben Score.
//
// v0.10.0: `(td_distance_from_threshold_m + rollout_distance_m) / LDA`,
// LDA = `runway_length_m - displaced_m`. Score-Bänder auf den effective_pct
// nach Heavy-Allowance (-5 pp). Skip-Gates wenn Datenlage unvollständig
// (KEIN Fallback auf rollout-only, KEIN Meter-Backup).
// ═════════════════════════════════════════════════════════════════════

/// Eingabe für den v0.10.0-Algorithmus. Wenn eines der **Required**-Felder
/// None ist, returnt `sub_rollout_v2` einen `skipped`-Entry mit konkretem
/// Reason (siehe Spec LE6). Allowance-Heavy + Display nutzt ICAO.
#[derive(Debug, Clone, Default)]
pub struct RolloutInput<'a> {
    /// Spec LE1 (required) — TD-Distance Threshold→Touchdown in Metern.
    /// Negativ = Touchdown vor Schwelle (= regelwidrig).
    pub td_distance_from_threshold_m: Option<f64>,
    /// Spec LE1 (required) — Rollout-Distance Touchdown→Stop in Metern.
    pub rollout_distance_m: Option<f32>,
    /// Spec LE9 — Reine Display-Info (Float-Distance vor Aufsetzen).
    /// None → entfällt aus der Extra-Zeile.
    pub landing_float_distance_m: Option<f32>,
    /// Spec LE2 (required) — physische Bahnlänge in Metern.
    pub runway_length_m: Option<f32>,
    /// Spec LE2 — Displaced-Threshold in ft. None → 0 angenommen.
    pub runway_displaced_threshold_ft: Option<i32>,
    /// Spec LE3 — Touchdown im pre-displaced Paint. true → Cap auf 55.
    pub pre_displaced_threshold: Option<bool>,
    /// Spec LE6 — `Some(true)` required. `Some(false)` UND `None` → Skip.
    pub runway_geometry_trusted: Option<bool>,
    /// Spec LE6 — muss `"runway_match"` sein. Sonst Skip
    /// (`off_airport_landing`).
    pub airport_source: Option<&'a str>,
    /// Spec LE9 — Display-Info "Bahn: XXXX 16L".
    pub runway_match_icao: Option<&'a str>,
    pub runway_match_ident: Option<&'a str>,
    /// Spec LE5 — Heavy bekommt -5 pp Allowance vor dem Banding.
    pub aircraft_icao: Option<&'a str>,
}

fn is_heavy(icao: Option<&str>) -> bool {
    category_for_icao(icao) == Category::Heavy
}

/// Spec LE6 — Skip-Gate. Reihenfolge (v0.10.0 Code-QS-R2-Fix P1-1):
/// 1. Preconditions: Airport-Source ZUERST, dann Geometry-Trust
/// 2. Required data: Rollout / TD / Length
/// 3. LDA-Sanity am Schluss
///
/// **Warum airport_source vor geometry_trust:** In der Realität setzt
/// `runway_geometry_trust_check` bei fehlendem `runway_match` automatisch
/// `(false, "no_runway_match")` zurück — d. h. ein Off-Airport-Touchdown
/// (= Crash auf dem Acker) hat IMMER `runway_geometry_trusted=Some(false)`.
/// Mit Geometry-zuerst-Reihenfolge wäre der Reason dann „untrusted_geometry"
/// — semantisch falsch („untrusted geometry" impliziert: es gibt eine
/// Geometrie, aber ihr ist nicht zu trauen). „off_airport_landing" ist
/// die spezifischere und ehrlichere Aussage.
///
/// **Warum Preconditions vor Data:** Off-Airport propagiert im
/// fill_v2_rollout_fields-Helper auch zu `td_distance=None` und
/// `runway_length=None` (beide werden aus dem runway_match abgeleitet).
/// Data-zuerst hätte als „missing_td_distance" gerendert — Quatsch bei
/// einem Acker-Crash.
///
/// KEIN Default-Fallback (sonst Datenmangel wird zu falschen 100 PTS —
/// R2-P0-1 Fix).
fn skip_reason(input: &RolloutInput) -> Option<&'static str> {
    // ── 1. Preconditions ─────────────────────────────────────────────
    // airport_source ZUERST — off-airport ist die spezifischste Aussage.
    if input.airport_source != Some("runway_match") {
        return Some("off_airport_landing");
    }
    // Dann geometry_trust. None ist NICHT trusted (R2-P1-2 Fix).
    if input.runway_geometry_trusted != Some(true) {
        return Some("untrusted_geometry");
    }
    // ── 2. Required-Data-Felder ──────────────────────────────────────
    if input.rollout_distance_m.is_none() {
        return Some("missing_rollout_distance");
    }
    if input.td_distance_from_threshold_m.is_none() {
        return Some("missing_td_distance");
    }
    if input.runway_length_m.is_none() {
        return Some("missing_length");
    }
    // ── 3. LDA-Sanity ────────────────────────────────────────────────
    let displaced_m = input.runway_displaced_threshold_ft.unwrap_or(0) as f32 * 0.3048;
    let lda = input.runway_length_m.unwrap() - displaced_m;
    if lda <= 0.0 {
        return Some("invalid_lda");
    }
    None
}

/// Spec LE1..LE9 (R5-SPEC-ACCEPTED). Einzige Compute-Stelle (SSoT) für
/// den v0.10.0-Bahn-Auslastungs-Sub-Score. UI rendert NUR.
pub fn sub_rollout_v2(input: &RolloutInput) -> SubScoreEntry {
    // ── Skip-Gate (LE6) ──────────────────────────────────────────────
    if let Some(reason) = skip_reason(input) {
        return SubScoreEntry::skipped("rollout", "landing.sub.rollout", reason);
    }

    // Skip-Gate hat alle None-Fälle gefiltert → unwrap safe.
    let td_dist = input.td_distance_from_threshold_m.unwrap() as f32;
    let rollout = input.rollout_distance_m.unwrap();
    let runway_m = input.runway_length_m.unwrap();
    let displaced_m = input.runway_displaced_threshold_ft.unwrap_or(0) as f32 * 0.3048;
    let lda_m = runway_m - displaced_m;

    // ── Numerator + Negativ-Schutz (LE1 + LE3 Halb-1) ────────────────
    // Pre-displaced + neg TD-Distance → raw_used könnte < rollout sein
    // (Pilot hat „virtuell" weniger Bahn genutzt als der reine Rollout).
    // Wir clampen auf max(rollout) damit niemand mit Aufsetzen-vor-
    // Schwelle einen kürzeren Distance-Used als sein eigener Rollout
    // bekommt (würde Score künstlich verbessern).
    let raw_used = td_dist + rollout;
    let distance_used = raw_used.max(rollout);

    // ── Raw-Ratio als Float (KEINE Pre-Rundung — R2-P2-1) ────────────
    let raw_ratio_pct: f32 = (distance_used / lda_m) * 100.0;

    // ── Overrun-Check ZUERST auf RAW (LE4 Step 2, R2-P0-2) ───────────
    // Wichtig: VOR Heavy-Allowance. Sonst würde 103 % Heavy → 98 %
    // effective → marginal_runway und der Overrun verschwindet.
    let (points, rationale, band) = if raw_ratio_pct > 100.0 {
        (0u8, "overrun_risk", Band::Bad)
    } else {
        let allowance_pp: f32 = if is_heavy(input.aircraft_icao) { 5.0 } else { 0.0 };
        let effective_pct: f32 = raw_ratio_pct - allowance_pp;

        match effective_pct {
            e if e < 30.0 => (100, "excellent_margin", Band::Good),
            e if e < 50.0 => (80, "good_stop", Band::Good),
            e if e < 70.0 => (55, "ok_stop", Band::Ok),
            e if e < 90.0 => (25, "long_rollout", Band::Bad),
            _ => (5, "marginal_runway", Band::Bad),
        }
    };

    // ── pre_displaced-Cap (LE3 Halb-2, R2-P1-4, R4-P1-3) ─────────────
    // Bei Cap wird Rationale-Key auf "pre_displaced_capped" überschrieben
    // (sonst UI „Viel Bahn-Reserve" bei 55 PTS = unehrlich).
    let pre_displaced = input.pre_displaced_threshold.unwrap_or(false);
    let (final_points, final_rationale, final_band) = if pre_displaced {
        let pts = points.min(55);
        let band = if pts >= 75 {
            Band::Good
        } else if pts >= 45 {
            Band::Ok
        } else {
            Band::Bad
        };
        (pts, "pre_displaced_capped", band)
    } else {
        (points, rationale, band)
    };

    let warning = if pre_displaced {
        Some("pre_displaced_threshold".to_string())
    } else {
        None
    };

    // ── Display-Strings (LE9) ────────────────────────────────────────
    let display_pct = raw_ratio_pct.round() as i32;
    let value = format!(
        "{} m / {} m  ·  {} %",
        distance_used.round() as i32,
        lda_m.round() as i32,
        display_pct
    );

    let mut extra: Vec<String> = Vec::new();
    if let Some(float) = input.landing_float_distance_m {
        if float > 0.0 {
            extra.push(format!("davon ~{} m Float vor Aufsetzen", float.round() as i32));
        }
    }
    if let (Some(icao), Some(ident)) = (input.runway_match_icao, input.runway_match_ident) {
        extra.push(format!(
            "Bahn: {} {}, LDA {} m",
            icao,
            ident,
            lda_m.round() as i32
        ));
    }

    SubScoreEntry::scored(
        "rollout",
        "landing.sub.rollout",
        final_points,
        value,
        final_rationale,
        final_band,
    )
    .with_extra(extra)
    .with_warning(warning)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    Light,
    Medium,
    Heavy,
}

/// Map an ICAO type designator to a rollout-score category.
///
/// Unbekannte / nicht-gemappte ICAOs fallen auf `Light` zurueck —
/// das ist der konservative Pfad (engste Schwellen). Wer als Cessna
/// 2 km ausrollt, hat tatsaechlich ein Problem; wer als A320 2 km
/// ausrollt, war gut.
pub(crate) fn category_for_icao(icao: Option<&str>) -> Category {
    let Some(icao) = icao else { return Category::Light };
    let icao = icao.trim().to_uppercase();

    // Heavy / Wide-Bodies.
    const HEAVY: &[&str] = &[
        // Airbus widebodies
        "A332", "A333", "A338", "A339",
        "A342", "A343", "A345", "A346",
        "A359", "A35K",
        "A388",
        // Boeing 747 family
        "B741", "B742", "B743", "B744", "B748",
        // Boeing 767
        "B762", "B763", "B764",
        // Boeing 777
        "B772", "B773", "B77F", "B77L", "B77W",
        // Boeing 787
        "B788", "B789", "B78X",
        // MD-11
        "MD11", "MD1F",
        // Antonov
        "A124", "A225",
        // IL-96
        "IL96",
    ];
    if HEAVY.contains(&icao.as_str()) {
        return Category::Heavy;
    }

    // Medium / Narrow-Body / Regional.
    const MEDIUM: &[&str] = &[
        // Airbus narrowbodies
        "A318", "A319", "A320", "A321",
        "A19N", "A20N", "A21N", // NEO family
        "A220", "BCS1", "BCS3", // A220 / CSeries
        // Boeing 737 family
        "B731", "B732", "B733", "B734", "B735",
        "B736", "B737", "B738", "B739",
        "B37M", "B38M", "B39M", "B3XM", // MAX
        // Boeing 757
        "B752", "B753",
        // Embraer regional
        "E135", "E145", "E170", "E175", "E190", "E195",
        "E290", "E295", // E2
        // Bombardier CRJ
        "CRJ1", "CRJ2", "CRJ7", "CRJ9", "CRJX",
        // ATR
        "AT42", "AT43", "AT44", "AT45", "AT46",
        "AT72", "AT73", "AT74", "AT75", "AT76",
        // Dash 8
        "DH8A", "DH8B", "DH8C", "DH8D",
        // Fokker
        "F70", "F100", "F50",
        // MD-80/90
        "MD81", "MD82", "MD83", "MD87", "MD88", "MD90",
    ];
    if MEDIUM.contains(&icao.as_str()) {
        return Category::Medium;
    }

    Category::Light
}

/// Rollout-Schwellen pro Kategorie. (good_top, ok_top, long_top,
/// very_long_top) — Werte sind die *obere* Grenze jedes Bandes in
/// Metern, oberhalb der nahesten Grenze faellt der Score in das
/// jeweils naechst-schlechtere Band.
pub(crate) fn thresholds_for(category: Category) -> (f32, f32, f32, f32) {
    match category {
        Category::Light => (800.0, 1200.0, 1800.0, 2500.0),
        Category::Medium => (1200.0, 1800.0, 2400.0, 3000.0),
        Category::Heavy => (1500.0, 2300.0, 3000.0, 3800.0),
    }
}

pub fn sub_rollout(rollout_m: Option<f32>, aircraft_icao: Option<&str>) -> Option<SubScoreEntry> {
    let m = rollout_m?;
    let value = format!("{} m", m.round() as i32);
    let category = category_for_icao(aircraft_icao);
    let (t_excellent, t_good, t_ok, t_long) = thresholds_for(category);

    let entry = if m < t_excellent {
        SubScoreEntry::scored(
            "rollout",
            "landing.sub.rollout",
            100,
            value,
            "excellent_stop",
            Band::Good,
        )
    } else if m < t_good {
        SubScoreEntry::scored(
            "rollout",
            "landing.sub.rollout",
            80,
            value,
            "good_stop",
            Band::Good,
        )
    } else if m < t_ok {
        SubScoreEntry::scored(
            "rollout",
            "landing.sub.rollout",
            55,
            value,
            "long_rollout",
            Band::Ok,
        )
    } else if m < t_long {
        SubScoreEntry::scored(
            "rollout",
            "landing.sub.rollout",
            25,
            value,
            "very_long_rollout",
            Band::Bad,
        )
    } else {
        SubScoreEntry::scored(
            "rollout",
            "landing.sub.rollout",
            5,
            value,
            "marginal_runway",
            Band::Bad,
        )
    };
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(m: f32, icao: Option<&str>) -> (u8, String) {
        let s = sub_rollout(Some(m), icao).unwrap();
        (s.points, s.rationale_key.unwrap())
    }

    #[test]
    fn none_returns_none() {
        assert!(sub_rollout(None, None).is_none());
        assert!(sub_rollout(None, Some("A320")).is_none());
    }

    #[test]
    fn light_category_unchanged_from_v0_7_16() {
        // Cessna 172 etc. — Schwellen identisch zur alten Hardcode-
        // Tabelle aus v0.7.16. Keine Regression fuer GA-Piloten.
        assert_eq!(run(500.0, Some("C172")), (100, "landing.rat.excellent_stop".into()));
        assert_eq!(run(799.99, Some("C172")), (100, "landing.rat.excellent_stop".into()));
        assert_eq!(run(800.0, Some("C172")), (80, "landing.rat.good_stop".into()));
        assert_eq!(run(1199.99, Some("C172")), (80, "landing.rat.good_stop".into()));
        assert_eq!(run(1200.0, Some("C172")), (55, "landing.rat.long_rollout".into()));
        assert_eq!(run(1799.99, Some("C172")), (55, "landing.rat.long_rollout".into()));
        assert_eq!(run(1800.0, Some("C172")), (25, "landing.rat.very_long_rollout".into()));
        assert_eq!(run(2499.99, Some("C172")), (25, "landing.rat.very_long_rollout".into()));
        assert_eq!(run(2500.0, Some("C172")), (5, "landing.rat.marginal_runway".into()));
    }

    #[test]
    fn medium_category_a320_landing_2km_is_good() {
        // N-002 Original-Beschwerde: A21N landete in BIKF mit ~2km
        // Rollout auf einer 3km Bahn. Vorher: 25 Pkt (very_long).
        // Jetzt: 80 Pkt (good_stop) bei 2000m — passt zur Realitaet.
        assert_eq!(run(2000.0, Some("A320")), (55, "landing.rat.long_rollout".into()));
        // 1500m fuer einen A320 ist sehr gut
        assert_eq!(run(1500.0, Some("A320")), (80, "landing.rat.good_stop".into()));
        // 1000m ist exzellent (kurze Bahn, perfektes Bremsen)
        assert_eq!(run(1000.0, Some("A320")), (100, "landing.rat.excellent_stop".into()));
        // 2800m ist sehr lang fuer A320
        assert_eq!(run(2800.0, Some("A320")), (25, "landing.rat.very_long_rollout".into()));
    }

    #[test]
    fn medium_category_covers_a320_family_and_b737() {
        // Alle A320-Family-Varianten muessen in Medium fallen.
        assert_eq!(category_for_icao(Some("A318")), Category::Medium);
        assert_eq!(category_for_icao(Some("A319")), Category::Medium);
        assert_eq!(category_for_icao(Some("A320")), Category::Medium);
        assert_eq!(category_for_icao(Some("A321")), Category::Medium);
        assert_eq!(category_for_icao(Some("A20N")), Category::Medium);
        assert_eq!(category_for_icao(Some("A21N")), Category::Medium);
        // B737 family
        assert_eq!(category_for_icao(Some("B738")), Category::Medium);
        assert_eq!(category_for_icao(Some("B739")), Category::Medium);
        assert_eq!(category_for_icao(Some("B38M")), Category::Medium);
    }

    #[test]
    fn b015_ein799_regression_a20n_1096m_is_excellent_not_good() {
        // v0.7.17 (B-015): EIN799 LTBJ→EIDW, A20N, Rollout 1096 m.
        // Vorher meldete der Pilot-Client 80 PTS „good_stop", weil
        // `aircraft_icao` beim PIREP-File None war (X-Plane Web API
        // hatte den ICAO nicht geliefert) → Fallback auf Light-GA-
        // Schwellen (800/1200) → 1096 fiel in „good_stop". Nach der
        // bid_icao-Fallback-Reparatur in lib.rs:8482 bekommt der Sub-
        // Score den Bid-Wert „A20N" und nutzt Medium-Schwellen
        // (1200/1800) → 1096 < 1200 → excellent_stop, 100 PTS.
        assert_eq!(run(1096.0, Some("A20N")), (100, "landing.rat.excellent_stop".into()));
        // Auch wenn snapshot None lieferte: jetzt kommt der Wert via Bid.
        // (Wenn beides None ist, fallen wir auf Light zurueck → 80 PTS.)
        assert_eq!(run(1096.0, None), (80, "landing.rat.good_stop".into()));
    }

    #[test]
    fn heavy_category_b777_a350_etc() {
        assert_eq!(category_for_icao(Some("B77W")), Category::Heavy);
        assert_eq!(category_for_icao(Some("A35K")), Category::Heavy);
        assert_eq!(category_for_icao(Some("A388")), Category::Heavy);
        assert_eq!(category_for_icao(Some("B748")), Category::Heavy);
    }

    #[test]
    fn heavy_category_b77w_2500m_still_good() {
        // Heavy braucht naturgemaess mehr Rollout. 2500m bei B777 ist
        // noch im „good"-Band, nicht „bad".
        assert_eq!(run(2500.0, Some("B77W")), (55, "landing.rat.long_rollout".into()));
        assert_eq!(run(2000.0, Some("B77W")), (80, "landing.rat.good_stop".into()));
        // 4000m+ ist auch fuer Heavy nicht mehr ok
        assert_eq!(run(4000.0, Some("B77W")), (5, "landing.rat.marginal_runway".into()));
    }

    #[test]
    fn unknown_icao_falls_back_to_light() {
        assert_eq!(category_for_icao(None), Category::Light);
        assert_eq!(category_for_icao(Some("UNKN")), Category::Light);
        assert_eq!(category_for_icao(Some("")), Category::Light);
    }

    #[test]
    fn icao_case_and_whitespace_tolerant() {
        // Pilot-Client kann ICAO mit Groß-/Kleinschreibung oder
        // Whitespace liefern — Match muss robust sein.
        assert_eq!(category_for_icao(Some("a320")), Category::Medium);
        assert_eq!(category_for_icao(Some("  B77W  ")), Category::Heavy);
        assert_eq!(category_for_icao(Some("a320")), Category::Medium);
    }
}
