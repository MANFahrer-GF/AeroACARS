//! Touchdown-quality assessment against authoritative runway data.
//!
//! Pure functions — no I/O, no globals. Inputs are the matched runway
//! plus a single touchdown sample; outputs are classification enums and
//! signed delta-meters/feet. The streamer-tick wires these into the
//! `LandingRecord` and the LandingPanel renders them as compliance pills.
//!
//! Spec: `docs/spec/v0.8.0-vps-navdata-runway-awareness.md`
//!  - F3 Touchdown-Zone (TDZ): ICAO Annex 14 / FAA 150/5340-1.
//!    TDZ length = min(900 m, runway_length / 3). Below 1200 m runway
//!    length the marking is not defined and we skip the feature.
//!  - F4 Aim-Point: FAA AIM 8-9-1. 400 m past threshold for runways
//!    ≥ 2400 m (= 7874 ft), 300 m for shorter runways.
//!  - F5 TCH-Compliance: comparison against `nav_runway.tch_ft`.
//!  - F6 Displaced-Threshold-Warning: touchdown in the painted-arrow
//!    pre-threshold zone is illegal in real ops.
//!  - F7 Wind-vs-Runway: classic vector decomposition against the
//!    runway's true course, NOT the magnetic course (the wind we get
//!    from the sim is also referenced to true north).
//!
//! Distances passed to `classify_tdz`/`classify_aim` must be **meters
//! from the landing threshold along the centerline**, positive = past
//! threshold. `runway::lookup_runway`/`lookup_runway_in_nav` measure
//! from the *physical runway-pavement start* instead (see their doc
//! comments) — callers MUST subtract `displaced_threshold_m` before
//! calling those two functions whenever the runway has one (see
//! `assess_touchdown` in `lib.rs`, the single place this correction is
//! applied). `classify_displaced` is the one function in this module
//! that wants the *uncorrected* pavement-start-relative distance —
//! see its own doc comment.
//!
//! Slice B wired these into the streamer-tick + `record_landing_for_
//! filed_flight` + the live MQTT TouchdownPayload — no module-level
//! `dead_code`-Allow needed anymore. `classify_wind` stays unused for
//! now because the F7 wind-vs-runway feature falls out for free from
//! the existing body-frame-wind path; kept here so a future caller
//! against a METAR-derived wind has a ready-made helper.

/// FAA AIM aim-point switchover: at or above this length, the standard
/// aim-point shifts from 300 m to 400 m past the threshold.
const LONG_RUNWAY_FT: f64 = 7874.0; // = 2400 m

/// ICAO Annex 14: TDZ markings are only painted on runways ≥ 1200 m.
/// Below that there's no defined "touchdown zone" and we surface "n/a"
/// in the UI instead of inventing a number.
const TDZ_MIN_RUNWAY_M: f64 = 1200.0;

/// ICAO Annex 14: TDZ ends at min(900 m, length/3).
const TDZ_MAX_LENGTH_M: f64 = 900.0;

/// Aim-Point distances (FAA AIM 8-9-1).
const AIM_SHORT_M: f64 = 300.0;
const AIM_LONG_M: f64 = 400.0;

const FT_PER_M: f64 = 3.280_839_895;

// ─── F3: TDZ ─────────────────────────────────────────────────────────

/// Whether the touchdown landed inside the marked TDZ and which third
/// of the runway it sits in. `None` when the runway is too short to
/// have a defined TDZ.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TdzResult {
    /// True when 0 < td_distance_m ≤ tdz_length_m. Negative
    /// `td_distance_m` (undershoot) is reported as `false` — the pilot
    /// is on the approach side of the threshold, not in the TDZ.
    pub in_tdz: bool,
    /// 1-indexed third of the runway the touchdown lies in. 1 = first
    /// third (the good zone), 2 = middle, 3 = last. Undershoots = 1
    /// for simplicity (they're below the threshold but conceptually in
    /// the "near the start" bucket).
    pub third: u8,
    /// Length of the TDZ marker in meters (≤ 900 m, ≤ length/3).
    pub tdz_length_m: f64,
}

/// In welchem Drittel der Bahn das Aufsetzen liegt — 1, 2 oder 3.
///
/// # Warum das nicht in `classify_tdz` bleibt
///
/// Die Aufsetzzonen-MARKIERUNG gibt es laut ICAO Annex 14 erst ab
/// 1200 m; darunter meldet `classify_tdz` deshalb `None`. Das Drittel
/// ist aber keine Markierung, sondern eine Division — es gilt auf einem
/// 700-m-Buschplatz genauso wie auf 4000 m.
///
/// Bis v1.7.15 stand diese Rechnung zweimal im Haus, einmal hier und
/// einmal roh in `lib.rs`. Beim Zusammenlegen fiel das alte Feld
/// `landing_touchdown_zone` auf kurzen Plaetzen lautlos weg, weil es an
/// die 1200-m-Grenze der Markierung gekoppelt wurde (externe QS,
/// 02.09.2026). Jetzt gibt es EINE Rechnung ohne die Grenze; die
/// Markierung nutzt sie mit.
///
/// Undershoots (negative Distanz) landen in 1 — sie liegen vor der
/// Schwelle, also am Anfang.
pub fn drittel(td_distance_m: f64, runway_length_m: f64) -> u8 {
    if td_distance_m <= runway_length_m / 3.0 {
        1
    } else if td_distance_m <= (2.0 * runway_length_m) / 3.0 {
        2
    } else {
        3
    }
}

/// Classify a touchdown distance against the runway's TDZ markings.
///
/// `td_distance_m` is signed along-track from the landing threshold —
/// same convention as `runway::lookup_runway`. Returns `None` when the
/// runway is too short for ICAO TDZ markings to apply.
pub fn classify_tdz(td_distance_m: f64, runway_length_m: f64) -> Option<TdzResult> {
    if runway_length_m < TDZ_MIN_RUNWAY_M {
        return None;
    }
    let tdz_length_m = (runway_length_m / 3.0).min(TDZ_MAX_LENGTH_M);
    let in_tdz = td_distance_m > 0.0 && td_distance_m <= tdz_length_m;
    let third = drittel(td_distance_m, runway_length_m);
    Some(TdzResult {
        in_tdz,
        third,
        tdz_length_m,
    })
}

// ─── F4: Aim-Point ───────────────────────────────────────────────────

/// Aim-point classification buckets. Stable strings — the i18n keys
/// and the wire-payload field both consume these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AimClass {
    /// |delta| < 60 m — on or very close to the aim point.
    Perfect,
    /// delta in [-150, -60] m — touched down a bit early.
    ShortOfAim,
    /// delta in [60, 200] m — touched a bit past, still acceptable.
    PastAim,
    /// delta in [200, 500] m — long landing, rollout-distance concern.
    LongLanding,
    /// |delta| > 500 m (past) or delta < -150 m (short) — severe.
    Severe,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AimResult {
    /// Expected aim-point distance from threshold in meters
    /// (300 m short / 400 m long, per FAA AIM 8-9-1).
    pub aim_point_m: f64,
    /// Signed delta = `td_distance_m - aim_point_m`. Positive = past
    /// aim point, negative = short of aim point.
    pub delta_m: f64,
    pub class: AimClass,
}

/// Classify the touchdown distance against the standard aim point.
pub fn classify_aim(td_distance_m: f64, runway_length_m: f64) -> AimResult {
    let aim_point_m = if runway_length_m * FT_PER_M >= LONG_RUNWAY_FT {
        AIM_LONG_M
    } else {
        AIM_SHORT_M
    };
    let delta_m = td_distance_m - aim_point_m;
    let class = if delta_m.abs() < 60.0 {
        AimClass::Perfect
    } else if delta_m >= 60.0 && delta_m < 200.0 {
        AimClass::PastAim
    } else if delta_m <= -60.0 && delta_m >= -150.0 {
        AimClass::ShortOfAim
    } else if delta_m >= 200.0 && delta_m <= 500.0 {
        AimClass::LongLanding
    } else {
        AimClass::Severe
    };
    AimResult {
        aim_point_m,
        delta_m,
        class,
    }
}

// ─── F5: TCH-Compliance ──────────────────────────────────────────────
// Wired in step_flight at the touchdown-edge: the streamer-tick scans
// `stats.snapshot_buffer` for the earliest sample with positive along-
// track distance from the landing threshold and stores its AGL in
// `stats.runway_tch_actual_ft`. `classify_tch` then turns the
// (actual, expected, Muster)-triple into a bucket.
//
// v1.8.1: Bewertet wird die Hoehe der RAEDER ueber der Schwelle, nicht
// mehr die feste Fuss-Abweichung vom Gleitpfad. Grundlage ist FAA Order
// 8260.58D, Abschnitt 1-3 mit Tabelle 1-3-1: Die TCH ist so gewaehlt, dass
// die Raeder (Wheel Crossing Height, WCH) mindestens 20 ft und hoechstens
// 50 ft ueber der Schwelle bleiben; der Abstand Gleitpfad→Rad haengt von
// der Hoehengruppe des Musters ab (10/15/20/25 ft). Vorher galt fuer jedes
// Flugzeug dasselbe feste Band (±5/−15/+20 ft) — eine 737 mit 20 ft Raedern
// ueber der Schwelle (genau das FAA-Minimum) stand rot da (Befund Thomas,
// 25.09.2026).
//
// Die gemessene Hoehe ist die des Simulator-Bezugspunkts am Flugzeug, nicht
// die der Gleitpfad-Antenne. Der Unterschied betraegt je nach Muster einige
// Fuss; die Raederhoehe ist deshalb eine Naeherung („ca.").

/// Hoehengruppe nach FAA Order 8260.58D, Tabelle 1-3-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Hoehengruppe {
    /// GA, Business-Jets, kleine Zubringer: Gleitpfad→Rad bis 10 ft.
    G1,
    /// B737, DC-9, F-28 (und vergleichbar A320-Familie, Regionaljets,
    /// Turboprops der Zubringerklasse): 15 ft.
    G2,
    /// B757/727/707: 20 ft.
    G3,
    /// B747/767/777, DC-10, A300 (und vergleichbar alle Grossraumjets): 25 ft.
    G4,
}

impl Hoehengruppe {
    /// Naeherungsweiser Abstand Gleitpfad→Rad (FAA, Tabelle 1-3-1).
    pub fn gleitpfad_zu_rad_ft(self) -> f64 {
        match self {
            Hoehengruppe::G1 => 10.0,
            Hoehengruppe::G2 => 15.0,
            Hoehengruppe::G3 => 20.0,
            Hoehengruppe::G4 => 25.0,
        }
    }
    pub fn nummer(self) -> u8 {
        match self {
            Hoehengruppe::G1 => 1,
            Hoehengruppe::G2 => 2,
            Hoehengruppe::G3 => 3,
            Hoehengruppe::G4 => 4,
        }
    }
}

/// Muster (ICAO-Kennung) → Hoehengruppe.
///
/// Die FAA-Tabelle nennt nur Beispiele. Die uebrigen Muster sind nach
/// Groesse und Fahrwerkshoehe zugeordnet — das ist unsere Einordnung, nicht
/// die der FAA. Unbekannte Kennungen gelten als Gruppe 1 (GA/Business-Jet);
/// die verbreiteten Verkehrs- und Transportflugzeuge sind hier aufgefuehrt,
/// seltene Muster koennen fehlen und landen dann in Gruppe 1 (Raederhoehe
/// eher zu hoch eingeschaetzt). Ohne Kennung Gruppe 3 als Mitte.
pub fn hoehengruppe(icao: Option<&str>) -> Hoehengruppe {
    let Some(roh) = icao else {
        return Hoehengruppe::G3;
    };
    let k = roh.trim().to_ascii_uppercase();
    if k.is_empty() {
        return Hoehengruppe::G3;
    }
    // Praefixe (Familien) und exakte Kennungen getrennt: kurze Praefixe wie
    // "C17" oder "C5" traefen sonst die Cessna 172 bzw. Citation 550.
    const G4: &[&str] = &[
        "B74", "B76", "B77", "B78", "A30", "A33", "A34", "A35", "A38", "MD11", "DC10", "L101",
        "IL96", "IL86", "A124", "A225", "KC10",
    ];
    // BLCF = 747 Dreamlifter, A3ST = Beluga, CONC = Concorde.
    const G4_EXAKT: &[&str] = &["A310", "C5", "C5M", "C17", "B52", "BLCF", "A3ST", "CONC"];
    const G3: &[&str] = &["B75", "B72", "B70", "IL76", "IL62", "K35R", "DC8"];
    const G3_EXAKT: &[&str] = &["C135", "E3TF", "E3CF", "T154", "T204", "A400"];
    const G2: &[&str] = &[
        "B73", "B37M", "B38M", "B39M", "B3XM", "A31", "A32", "A19N", "A20N", "A21N", "BCS1",
        "BCS3", "MD8", "MD9", "DC9", "B712", "F28", "F70", "F100", "E17", "E19", "E29", "E75",
        "CRJ", "AT4", "AT7", "DH8", "B46", "RJ70", "RJ85", "RJ1H", "SU95", "E145", "E135", "E140",
        "SF34", "SB20", "JS41", "D328", "AN24", "AN26", "BA11", "F27", "F50", "DHC7", "C919",
        "AJ27",
    ];
    // C30J = C-130J, P8 = Poseidon (737-Basis).
    const G2_EXAKT: &[&str] = &["C130", "C30J", "Y12", "P8"];
    let passt = |praefix: &[&str], exakt: &[&str]| {
        exakt.contains(&k.as_str()) || praefix.iter().any(|p| k.starts_with(p))
    };
    // G4 zuerst: A310 (Grossraum) steht exakt dort, "A31" (A318/A319) erst
    // in G2.
    if passt(G4, G4_EXAKT) {
        Hoehengruppe::G4
    } else if passt(G3, G3_EXAKT) {
        Hoehengruppe::G3
    } else if passt(G2, G2_EXAKT) {
        Hoehengruppe::G2
    } else {
        Hoehengruppe::G1
    }
}

/// FAA: Raeder mindestens 20 ft ueber der Schwelle.
pub const WCH_MIN_FT: f64 = 20.0;
/// FAA: Raeder hoechstens 50 ft ueber der Schwelle.
pub const WCH_MAX_FT: f64 = 50.0;
/// Unser Toleranzstreifen (gelb) jenseits des FAA-Bands.
pub const WCH_TOLERANZ_FT: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TchClass {
    /// Raeder 20–50 ft ueber der Schwelle — im FAA-Band.
    OnProfile,
    /// Raeder 10–20 ft — unter dem FAA-Minimum, aber noch mit Abstand.
    SlightlyLow,
    /// Raeder 50–60 ft — ueber dem FAA-Maximum, laengeres Ausschweben.
    SlightlyHigh,
    /// Raeder ueber 60 ft — Gefahr einer langen Landung.
    High,
    /// Raeder unter 10 ft — kaum Abstand zur Schwelle.
    BelowProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TchResult {
    pub actual_ft: f64,
    pub expected_ft: f64,
    pub delta_ft: f64,
    /// Naeherungsweise Hoehe der Raeder ueber der Schwelle.
    pub rad_ft: f64,
    pub gruppe: Hoehengruppe,
    pub class: TchClass,
}

/// Classify the actual TCH measured at threshold-crossing. `delta_ft`
/// bleibt die Abweichung vom veroeffentlichten Gleitpfad (Anzeige); die
/// Einstufung richtet sich nach der Raederhoehe.
pub fn classify_tch(actual_ft: f64, expected_ft: f64, gruppe: Hoehengruppe) -> TchResult {
    let delta_ft = actual_ft - expected_ft;
    let rad_ft = actual_ft - gruppe.gleitpfad_zu_rad_ft();
    let class = if rad_ft < WCH_MIN_FT - WCH_TOLERANZ_FT {
        TchClass::BelowProfile
    } else if rad_ft < WCH_MIN_FT {
        TchClass::SlightlyLow
    } else if rad_ft <= WCH_MAX_FT {
        TchClass::OnProfile
    } else if rad_ft <= WCH_MAX_FT + WCH_TOLERANZ_FT {
        TchClass::SlightlyHigh
    } else {
        TchClass::High
    };
    TchResult {
        actual_ft,
        expected_ft,
        delta_ft,
        rad_ft,
        gruppe,
        class,
    }
}

// ─── F6: Displaced-Threshold-Warning ─────────────────────────────────

/// Did the pilot touch down inside the painted pre-threshold zone —
/// on the runway pavement, but before the legal landing threshold?
///
/// v0.19.x FIX: `td_distance_m` here is the RAW along-track distance
/// from the *physical runway-pavement start* — i.e. `NavRunway::
/// threshold` / `RunwayMatch::threshold_lat/lon`, exactly as computed
/// by `runway::lookup_runway_in_nav`. That point is **not** the
/// landing threshold whenever the runway has a displaced threshold:
/// proven by the OLBA fixture in `runway.rs`, where RWY 35's
/// `threshold` coordinate is bit-identical to RWY 17's `far_end` —
/// the physical pavement end shared by both landing directions. The
/// legal landing threshold sits `displaced_threshold_ft` further down
/// the runway *from that point*.
///
/// So the illegal DDS zone is `0 <= td_distance_m < displaced_threshold_m`
/// — on the pavement, before the legal threshold. `td_distance_m < 0`
/// means the touchdown is before the physical runway even starts —
/// a plain undershoot (off-airport), not a displaced-threshold
/// violation.
///
/// (An earlier version of this function had the sign/zone backwards —
/// it treated `td_distance_m < 0` as "before the landing threshold",
/// which meant a real DDS violation could never be detected and the
/// check instead fired on undershoots that never touched pavement at
/// all. `assess_touchdown` in `lib.rs` still needs to independently
/// correct `td_distance_m` for this same displacement before feeding
/// it to `classify_tdz`/`classify_aim`, which — per this module's own
/// doc comment — expect distance from the *landing* threshold.)
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DisplacedResult {
    /// True when the touchdown sits between the physical runway start
    /// and the landing threshold (= on the displaced-threshold paint).
    pub in_pre_threshold_zone: bool,
    pub displaced_threshold_m: f64,
}

pub fn classify_displaced(td_distance_m: f64, displaced_threshold_ft: f64) -> DisplacedResult {
    let displaced_threshold_m = displaced_threshold_ft / FT_PER_M;
    // On the pavement (>= physical runway start) but short of the
    // legal landing threshold.
    let in_pre_threshold_zone = displaced_threshold_m > 0.0
        && td_distance_m >= 0.0
        && td_distance_m < displaced_threshold_m;
    DisplacedResult {
        in_pre_threshold_zone,
        displaced_threshold_m,
    }
}

// ─── F7: Wind-vs-Runway (exact) ──────────────────────────────────────
// Production today gets headwind/crosswind from the body-frame
// SimVars (`AIRCRAFT WIND X/Z`), which is more accurate than METAR-
// derived math. This pure helper stays for any future caller that has
// only a METAR (wind_dir/wind_speed) — e.g. server-side replay against
// a METAR fixture without the body-frame vector.

/// Wind decomposition relative to the runway centerline (using the
/// runway's *true* course, not magnetic).
///
/// Sign conventions:
///   * `headwind_kt > 0` → wind blowing into the aircraft's face
///     (= classic „good" headwind on approach).
///   * `crosswind_kt > 0` → wind from the right (= aircraft would
///     drift left without correction).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WindResult {
    pub headwind_kt: f64,
    pub crosswind_kt: f64,
}

/// Compute headwind/crosswind for a runway from a meteorological wind
/// vector. `wind_dir_true_deg` is the *from* direction (METAR
/// convention: 270° = wind blowing from the west toward the east).
/// `runway_true_course_deg` is the direction the aircraft is rolling
/// in (= threshold-to-end bearing).
#[allow(dead_code)]
pub fn classify_wind(
    wind_speed_kt: f64,
    wind_dir_true_deg: f64,
    runway_true_course_deg: f64,
) -> WindResult {
    let mut diff = (wind_dir_true_deg - runway_true_course_deg).to_radians();
    // Normalise to (-π, π] so the trig comes out signed.
    while diff > std::f64::consts::PI {
        diff -= 2.0 * std::f64::consts::PI;
    }
    while diff <= -std::f64::consts::PI {
        diff += 2.0 * std::f64::consts::PI;
    }
    // METAR wind is the from-direction. A wind FROM the runway heading
    // is straight in your face → max headwind. cos(0) = 1.
    let headwind_kt = wind_speed_kt * diff.cos();
    // Wind FROM the right of the landing direction → crosswind +.
    // RWY 360° + wind FROM 090° (east, = right) → diff = +90° (after
    // normalisation), sin(+90°) = +1 → crosswind = +10. That matches
    // the pilot convention "crosswind from the right".
    let crosswind_kt = wind_speed_kt * diff.sin();
    WindResult {
        headwind_kt,
        crosswind_kt,
    }
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// MS713 anchor: OLBA RWY 17, 3250 m runway. Touchdown 320 m past
    /// threshold (the example used throughout the spec).
    const MS713_RUNWAY_M: f64 = 3250.0;
    const MS713_TD_M: f64 = 320.0;

    #[test]
    fn tdz_ms713_anchor() {
        let r = classify_tdz(MS713_TD_M, MS713_RUNWAY_M).expect("MS713 RWY > 1200 m");
        assert!(r.in_tdz, "320 m past threshold on 3250 m RWY → in TDZ");
        assert_eq!(r.third, 1);
        // TDZ = min(900, 3250/3) = min(900, 1083) = 900.
        assert!((r.tdz_length_m - 900.0).abs() < 0.01);
    }

    #[test]
    fn tdz_long_landing_third_two() {
        // 1430 m past threshold on 3250 m → outside 900 m TDZ, second third.
        let r = classify_tdz(1430.0, 3250.0).unwrap();
        assert!(!r.in_tdz);
        assert_eq!(r.third, 2);
    }

    #[test]
    fn tdz_skipped_short_runway() {
        // 1000 m runway — too short for TDZ markings.
        assert!(classify_tdz(200.0, 1000.0).is_none());
    }

    #[test]
    fn tdz_undershoot_not_in_tdz() {
        let r = classify_tdz(-12.0, 3250.0).unwrap();
        assert!(!r.in_tdz, "undershoot is on the approach side, not TDZ");
        assert_eq!(r.third, 1);
    }

    #[test]
    fn tdz_length_caps_at_900m_for_short_runway() {
        // 2400 m → length/3 = 800, cap doesn't bind, expect 800.
        let r = classify_tdz(500.0, 2400.0).unwrap();
        assert!((r.tdz_length_m - 800.0).abs() < 0.01);
        // 5000 m → length/3 = 1666, cap binds at 900.
        let r = classify_tdz(500.0, 5000.0).unwrap();
        assert!((r.tdz_length_m - 900.0).abs() < 0.01);
    }

    #[test]
    fn aim_ms713_perfect_within_60m() {
        // 320 m TD vs 400 m aim (3250 m > 2400 m threshold) → delta -80 m.
        // Actually wait — 3250 m × 3.28 = 10663 ft > 7874 ft → long runway,
        // aim = 400 m. Delta = 320 - 400 = -80 → outside ±60 → ShortOfAim.
        let r = classify_aim(MS713_TD_M, MS713_RUNWAY_M);
        assert!((r.aim_point_m - 400.0).abs() < 0.01, "long RWY → 400 m aim");
        assert!((r.delta_m - (-80.0)).abs() < 0.01);
        assert_eq!(r.class, AimClass::ShortOfAim);
    }

    #[test]
    fn aim_short_runway_uses_300m() {
        // 1500 m × 3.28 = 4920 ft < 7874 ft → short, aim = 300 m.
        // TD 320 m → delta +20 m → Perfect.
        let r = classify_aim(320.0, 1500.0);
        assert!((r.aim_point_m - 300.0).abs() < 0.01);
        assert!((r.delta_m - 20.0).abs() < 0.01);
        assert_eq!(r.class, AimClass::Perfect);
    }

    #[test]
    fn aim_long_landing_warn() {
        // TD 800 m, RWY 3250 m → aim 400 m, delta +400 m → LongLanding.
        let r = classify_aim(800.0, 3250.0);
        assert_eq!(r.class, AimClass::LongLanding);
    }

    #[test]
    fn aim_severe_far_past() {
        let r = classify_aim(1200.0, 3250.0);
        assert_eq!(r.class, AimClass::Severe);
    }

    #[test]
    fn aim_severe_far_short() {
        // delta = -250 → Severe (below -150 m threshold).
        let r = classify_aim(150.0, 3250.0);
        assert_eq!(r.class, AimClass::Severe);
    }

    #[test]
    fn tch_befund_thomas_737_auf_minimum_ist_nicht_rot() {
        // 35 ft gemessen, TCH 54: vorher „unter Profil" (rot). Eine 737
        // hat die Raeder bei ca. 20 ft = FAA-Minimum → im Band.
        let r = classify_tch(35.0, 54.0, hoehengruppe(Some("B738")));
        assert_eq!(r.gruppe, Hoehengruppe::G2);
        assert!((r.rad_ft - 20.0).abs() < 0.01);
        assert!((r.delta_ft - (-19.0)).abs() < 0.01);
        assert_eq!(r.class, TchClass::OnProfile);
    }

    #[test]
    fn tch_dieselbe_hoehe_ist_bei_der_777_zu_tief() {
        // Raeder ca. 10 ft ueber der Schwelle — Grenze zu rot.
        let r = classify_tch(35.0, 54.0, hoehengruppe(Some("B77W")));
        assert_eq!(r.gruppe, Hoehengruppe::G4);
        assert_eq!(r.class, TchClass::SlightlyLow);
        let r = classify_tch(33.0, 54.0, Hoehengruppe::G4);
        assert_eq!(r.class, TchClass::BelowProfile);
    }

    #[test]
    fn tch_baender_nach_raederhoehe() {
        let g = Hoehengruppe::G3; // 20 ft Abstand
        assert_eq!(classify_tch(50.0, 50.0, g).class, TchClass::OnProfile); // Rad 30
        assert_eq!(classify_tch(40.0, 50.0, g).class, TchClass::OnProfile); // Rad 20
        assert_eq!(classify_tch(35.0, 50.0, g).class, TchClass::SlightlyLow); // Rad 15
        assert_eq!(classify_tch(29.0, 50.0, g).class, TchClass::BelowProfile); // Rad 9
        assert_eq!(classify_tch(70.0, 50.0, g).class, TchClass::OnProfile); // Rad 50
        assert_eq!(classify_tch(75.0, 50.0, g).class, TchClass::SlightlyHigh); // Rad 55
        assert_eq!(classify_tch(85.0, 50.0, g).class, TchClass::High); // Rad 65
    }

    #[test]
    fn hoehengruppen_der_haeufigen_muster() {
        for (icao, g) in [
            ("A320", Hoehengruppe::G2),
            ("A20N", Hoehengruppe::G2),
            ("A319", Hoehengruppe::G2),
            ("A21N", Hoehengruppe::G2),
            ("A310", Hoehengruppe::G4),
            ("A306", Hoehengruppe::G4),
            ("A333", Hoehengruppe::G4),
            ("A359", Hoehengruppe::G4),
            ("A388", Hoehengruppe::G4),
            ("B738", Hoehengruppe::G2),
            ("B38M", Hoehengruppe::G2),
            ("B752", Hoehengruppe::G3),
            ("B763", Hoehengruppe::G4),
            ("B789", Hoehengruppe::G4),
            ("B748", Hoehengruppe::G4),
            ("MD11", Hoehengruppe::G4),
            ("CRJ9", Hoehengruppe::G2),
            ("E195", Hoehengruppe::G2),
            ("AT76", Hoehengruppe::G2),
            ("DH8D", Hoehengruppe::G2),
            ("BCS3", Hoehengruppe::G2),
            ("C750", Hoehengruppe::G1),
            ("C172", Hoehengruppe::G1),
            ("C17", Hoehengruppe::G4),
            ("C550", Hoehengruppe::G1),
            ("C560", Hoehengruppe::G1),
            ("E35L", Hoehengruppe::G1),
            ("C130", Hoehengruppe::G2),
            ("C30J", Hoehengruppe::G2),
            ("JS41", Hoehengruppe::G2),
            ("BLCF", Hoehengruppe::G4),
            ("A3ST", Hoehengruppe::G4),
            ("T154", Hoehengruppe::G3),
            ("DC86", Hoehengruppe::G3),
            ("BA11", Hoehengruppe::G2),
            ("F50", Hoehengruppe::G2),
            ("P8", Hoehengruppe::G2),
            ("C919", Hoehengruppe::G2),
            ("A400", Hoehengruppe::G3),
            ("PC12", Hoehengruppe::G1),
            ("FA50", Hoehengruppe::G1),
            (" b77w ", Hoehengruppe::G4),
        ] {
            assert_eq!(hoehengruppe(Some(icao)), g, "{icao}");
        }
        assert_eq!(hoehengruppe(None), Hoehengruppe::G3);
        assert_eq!(hoehengruppe(Some("")), Hoehengruppe::G3);
    }

    #[test]
    fn displaced_olba_rwy35_anchor() {
        // OLBA RWY 35 has DDS = 2690 ft (819.91 m), measured from the
        // physical runway-pavement start. Pilot touched down 200 m
        // past the *landing* threshold, i.e. 819.91+200 = 1019.91 m
        // from the pavement start → well past the DDS zone.
        let r = classify_displaced(1019.91, 2690.0);
        assert!(!r.in_pre_threshold_zone);
        assert!((r.displaced_threshold_m - 819.91).abs() < 0.5);
    }

    #[test]
    fn displaced_touchdown_on_dds_paint_illegal() {
        // Pilot touched down 80 m BEFORE the landing threshold on a
        // runway with 819.91 m DDS, i.e. 819.91-80 = 739.91 m from
        // the pavement start — on the paved runway, short of the
        // legal threshold. Illegal real-world landing on the painted
        // displaced-threshold arrows.
        let r = classify_displaced(739.91, 2690.0);
        assert!(r.in_pre_threshold_zone);
    }

    #[test]
    fn displaced_boundary_at_pavement_start_is_in_zone() {
        // Touching down exactly at the physical runway start (the
        // worst legal-pavement case) must still be flagged — it's on
        // the DDS paint, as far before the threshold as it gets.
        let r = classify_displaced(0.0, 2690.0);
        assert!(r.in_pre_threshold_zone);
    }

    #[test]
    fn displaced_boundary_at_landing_threshold_is_not_in_zone() {
        // Touching down exactly on (or just past) the landing threshold
        // itself is the textbook case, not a violation. 820.5 m is
        // deliberately just past the true 819.9512... m boundary so this
        // isn't sensitive to float rounding of the boundary itself.
        let r = classify_displaced(820.5, 2690.0);
        assert!(!r.in_pre_threshold_zone);
    }

    #[test]
    fn displaced_no_dds_never_triggers() {
        // OLBA RWY 17 has DDS = 0 — undershoot is undershoot, no DDS warning.
        let r = classify_displaced(-50.0, 0.0);
        assert!(!r.in_pre_threshold_zone);
    }

    #[test]
    fn displaced_undershoot_before_pavement_is_off_field() {
        // Pilot touched down 50 m before the physical runway even
        // starts (td_distance_m < 0) — that's a plain undershoot off
        // the airport, not a displaced-threshold violation.
        let r = classify_displaced(-50.0, 2690.0);
        assert!(!r.in_pre_threshold_zone);
    }

    #[test]
    fn wind_straight_headwind() {
        // RWY 17 (true 176.94°), wind FROM 177° at 12 kt → ~12 kt
        // headwind, ~0 kt crosswind.
        let r = classify_wind(12.0, 177.0, 176.94);
        assert!((r.headwind_kt - 12.0).abs() < 0.01);
        assert!(r.crosswind_kt.abs() < 0.05);
    }

    #[test]
    fn wind_pure_tailwind() {
        // Wind from opposite direction (357° from RWY 177°) → -12 kt
        // headwind (tailwind), 0 kt crosswind.
        let r = classify_wind(12.0, 357.0, 177.0);
        assert!((r.headwind_kt - (-12.0)).abs() < 0.05);
        assert!(r.crosswind_kt.abs() < 0.05);
    }

    #[test]
    fn wind_pure_crosswind_from_right() {
        // RWY 360° (= north), wind from 90° (east) at 10 kt → wind comes
        // from the right of the landing direction → crosswind +10 kt
        // (right-cross), headwind 0.
        let r = classify_wind(10.0, 90.0, 360.0);
        assert!(r.headwind_kt.abs() < 0.05);
        assert!(
            (r.crosswind_kt - 10.0).abs() < 0.05,
            "got cx={}",
            r.crosswind_kt
        );
    }

    #[test]
    fn wind_pure_crosswind_from_left() {
        // RWY 360°, wind from 270° (west) → from the left → crosswind -10 kt.
        let r = classify_wind(10.0, 270.0, 360.0);
        assert!(r.headwind_kt.abs() < 0.05);
        assert!(
            (r.crosswind_kt - (-10.0)).abs() < 0.05,
            "got cx={}",
            r.crosswind_kt
        );
    }

    #[test]
    fn wind_45deg_split() {
        // RWY 360°, wind FROM 045° at 14.14 kt → headwind 10, crosswind +10.
        let r = classify_wind(14.142_135_6, 45.0, 360.0);
        assert!((r.headwind_kt - 10.0).abs() < 0.05);
        assert!((r.crosswind_kt - 10.0).abs() < 0.05);
    }
}
