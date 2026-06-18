//! Runway lookup — given a touchdown lat/lon (and the aircraft's true
//! heading at touchdown), figure out which runway the pilot landed on
//! and where on it. Used by the PIREP report to surface "you landed on
//! EDDP/26R, 1.4 m right of centerline, 1100 ft past the threshold".
//!
//! Why embedded CSV: the NSIS installer drops a single binary into
//! `%LOCALAPPDATA%`. We don't want to ship a sidecar data file or wire
//! up a `tauri::path` resolver for a 4 MB blob that never changes at
//! runtime — `include_str!` keeps everything self-contained at the
//! cost of ~4 MB of binary.
//!
//! Source: <https://ourairports.com/data/> — public domain.
//!
//! Coordinates are WGS84 decimal degrees. Distances in meters unless
//! the field name says `_ft`; bearings are degrees true (0..360).

use std::sync::OnceLock;

/// Embedded snapshot of the ourairports runways table. Refreshed manually
/// when the upstream CSV gets significant updates (new airports, closed
/// runways) — this isn't a hot data source, the world's runway layout
/// is essentially static on human timescales.
const RUNWAYS_CSV: &str = include_str!("../data/ourairports-runways.csv");

/// Mean Earth radius (meters) — same value used by the haversine formula
/// throughout aviation tooling.
const EARTH_RADIUS_M: f64 = 6_371_000.0;

/// Bounding-box prefilter half-width in degrees. ~0.05° lat ≈ 5.5 km,
/// which comfortably covers any landable runway plus rollout. Lon is
/// deliberately treated the same — we'd have to scale by cos(lat) to be
/// distance-true, but a slightly-wider window costs nothing here and
/// keeps polar edge cases simple.
const BBOX_HALF_DEG: f64 = 0.05;

/// Default search radius. Anything farther than 3 km from the touchdown
/// point is almost certainly a different airport — bail out rather than
/// confidently mis-attribute the landing.
const DEFAULT_MAX_DISTANCE_M: f64 = 3000.0;

/// "On the centerline" tolerance for the side classification. 2 m matches
/// what BeatMyLanding uses and roughly the precision of the SimConnect
/// position fix at low altitude.
const CENTERLINE_TOLERANCE_M: f64 = 2.0;

/// One row of the parsed CSV. We only keep the fields we use, all already
/// validated as non-empty during parse so downstream code can `.unwrap_or`
/// safely on the optionals (length/width).
#[derive(Debug, Clone)]
struct RunwayRow {
    airport_ident: String,
    length_ft: f32,
    width_ft: f32,
    surface: String,
    le_ident: String,
    le_lat: f64,
    le_lon: f64,
    le_heading_true: f32,
    he_ident: String,
    he_lat: f64,
    he_lon: f64,
    he_heading_true: f32,
}

/// Result of resolving a touchdown coordinate to a runway.
// PartialEq (v0.16.24): lets the on-plan-byte-identical test assert the
// actual-airport-keyed correlation produces an identical match to the old
// `arr_airport`-keyed path. All fields are f32/f64/String — structural eq.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RunwayMatch {
    /// Airport ICAO/ident from CSV (e.g. "EDDP", "GB-0002").
    pub airport_ident: String,
    /// Resolved runway name as the pilot would say it ("26R", "08L").
    pub runway_ident: String,
    /// True heading of the runway centerline, in the landing direction.
    pub heading_true_deg: f32,
    /// Total runway length in ft.
    pub length_ft: f32,
    /// Runway width in ft.
    pub width_ft: f32,
    /// Surface code from CSV (e.g. "ASPH", "CON", "GRVL").
    pub surface: String,
    /// Threshold (= landing-direction start) lat/lon.
    pub threshold_lat: f64,
    pub threshold_lon: f64,
    /// Far-end (departure-direction) lat/lon.
    pub end_lat: f64,
    pub end_lon: f64,
    /// Signed perpendicular distance from runway centerline.
    /// Positive = pilot was right of centerline, negative = left.
    pub centerline_distance_m: f64,
    /// |centerline_distance_m| converted to feet — easier for pilots.
    pub centerline_distance_abs_ft: f64,
    /// Signed great-circle along-track distance from the threshold,
    /// in feet. Positive = touchdown PAST the threshold (the normal
    /// case — pilot crossed the threshold on final and put it down
    /// somewhere down the runway). Negative = touchdown BEFORE the
    /// threshold (undershoot — pilot was still on the approach side
    /// when the on-ground edge fired). Zero = touchdown exactly on
    /// the threshold within float precision.
    ///
    /// v0.5.20: pre-v0.5.20 this field was the unsigned magnitude
    /// only, so undershoots showed up as small positive values
    /// indistinguishable from "landed right at the threshold". The
    /// sign is computed by checking the bearing from threshold to
    /// touchdown against the runway heading: within ±90° → positive,
    /// outside ±90° → negative.
    pub touchdown_distance_from_threshold_ft: f64,
    /// "LEFT", "RIGHT", or "CENTER" (within 2 m of centerline).
    pub side: String,
}

/// Heuristic: does this airport_ident look like an ICAO code?
/// ICAO codes are exactly 4 letters, no digits or dashes. National
/// fallback identifiers use formats like "DE-0901" / "US-1234".
/// Matters because OurAirports ships *both* for many real airports —
/// the German aviation authority assigns DE-#### IDs and OurAirports
/// dutifully imports them as separate rows alongside the ICAO ones.
/// Without the dedupe step below the lookup would happily return
/// "DE-0901" for an EDDM landing — same coordinates, just the wrong
/// label. Real bug observed 2026-05-02.
fn looks_like_icao(ident: &str) -> bool {
    ident.len() == 4 && ident.chars().all(|c| c.is_ascii_uppercase())
}

/// Parse the embedded CSV exactly once. The OnceLock means concurrent
/// callers from a thread pool don't race on parsing — first one through
/// the door does the work, everyone else waits on the lock and reads
/// the cached `Vec`.
fn runways() -> &'static Vec<RunwayRow> {
    static CELL: OnceLock<Vec<RunwayRow>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(RUNWAYS_CSV.as_bytes());
        let mut out = Vec::with_capacity(48_000);
        for record in rdr.records().flatten() {
            // Skip closed runways — pilots can't land on them and matching
            // a touchdown to one would just be confusing.
            if record.get(7).unwrap_or("0") == "1" {
                continue;
            }
            // Both ends must have coordinates. Heliports, water "runways",
            // and a handful of legacy entries have empty lat/lon — they're
            // useless to us.
            let le_lat = parse_f64(record.get(9));
            let le_lon = parse_f64(record.get(10));
            let he_lat = parse_f64(record.get(15));
            let he_lon = parse_f64(record.get(16));
            let (Some(le_lat), Some(le_lon), Some(he_lat), Some(he_lon)) =
                (le_lat, le_lon, he_lat, he_lon)
            else {
                continue;
            };

            let airport_ident = record.get(2).unwrap_or("").to_string();
            let length_ft = parse_f32(record.get(3)).unwrap_or(0.0);
            let width_ft = parse_f32(record.get(4)).unwrap_or(0.0);
            let surface = record.get(5).unwrap_or("").to_string();
            let le_ident = record.get(8).unwrap_or("").to_string();
            // The CSV occasionally omits headings — fall back to a computed
            // bearing from the threshold to the far end. That's what real
            // ATC charts use anyway.
            let le_heading = parse_f32(record.get(12))
                .unwrap_or_else(|| initial_bearing_deg(le_lat, le_lon, he_lat, he_lon) as f32);
            let he_ident = record.get(14).unwrap_or("").to_string();
            let he_heading = parse_f32(record.get(18))
                .unwrap_or_else(|| initial_bearing_deg(he_lat, he_lon, le_lat, le_lon) as f32);

            out.push(RunwayRow {
                airport_ident,
                length_ft,
                width_ft,
                surface,
                le_ident,
                le_lat,
                le_lon,
                le_heading_true: le_heading,
                he_ident,
                he_lat,
                he_lon,
                he_heading_true: he_heading,
            });
        }
        tracing::debug!(count = out.len(), "runway table parsed (raw)");

        // Dedupe pass: many airports appear *twice* in OurAirports — once
        // under the ICAO ident, once under a national fallback identifier
        // (EDDM ↔ DE-0901, KJFK ↔ US-..., RJTT ↔ JP-..., etc.). They
        // share the exact same threshold coordinates because they
        // *describe the same physical runway*. Keep the ICAO row when
        // that happens; otherwise the lookup picks whichever came first
        // in the CSV (= often the national one) and the PIREP shows
        // "DE-0901/08L" instead of "EDDM/08L".
        //
        // Dedup key: (le_lat × 1e5, le_lon × 1e5, runway_ident) rounded
        // to ~1 m precision. Any two rows sharing that key are the same
        // physical runway.
        let mut by_key: std::collections::HashMap<(i64, i64, String), usize> =
            std::collections::HashMap::with_capacity(out.len());
        let mut to_drop: Vec<bool> = vec![false; out.len()];
        for (idx, row) in out.iter().enumerate() {
            let key = (
                (row.le_lat * 1e5).round() as i64,
                (row.le_lon * 1e5).round() as i64,
                row.le_ident.clone(),
            );
            match by_key.get(&key).copied() {
                Some(existing_idx) => {
                    let existing = &out[existing_idx];
                    let existing_is_icao = looks_like_icao(&existing.airport_ident);
                    let new_is_icao = looks_like_icao(&row.airport_ident);
                    if new_is_icao && !existing_is_icao {
                        // Replace the national-id row with the ICAO row.
                        to_drop[existing_idx] = true;
                        by_key.insert(key, idx);
                    } else {
                        // Keep the existing row, drop this one.
                        to_drop[idx] = true;
                    }
                }
                None => {
                    by_key.insert(key, idx);
                }
            }
        }
        let final_out: Vec<RunwayRow> = out
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| if to_drop[i] { None } else { Some(r) })
            .collect();
        tracing::debug!(count = final_out.len(), "runway table after ICAO dedupe");
        final_out
    })
}

fn parse_f64(s: Option<&str>) -> Option<f64> {
    s.and_then(|v| if v.is_empty() { None } else { v.parse().ok() })
}

fn parse_f32(s: Option<&str>) -> Option<f32> {
    s.and_then(|v| if v.is_empty() { None } else { v.parse().ok() })
}

/// Resolve an airport ICAO/ident to an approximate position by
/// averaging all runway thresholds belonging to that airport. Used by
/// the auto-start watcher to check "is the aircraft parked at the
/// departure airport". The returned point is somewhere on the
/// airport — usually the geometric centre of the runway layout, give
/// or take a few hundred meters.
///
/// Returns `None` when the ident isn't in the OurAirports table
/// (uncommon strips, military closed fields, etc.).
pub fn airport_position(icao: &str) -> Option<(f64, f64)> {
    let table = runways();
    let needle = icao.trim().to_uppercase();
    let mut sum_lat = 0.0_f64;
    let mut sum_lon = 0.0_f64;
    let mut count = 0_u32;
    for row in table.iter() {
        if row.airport_ident.eq_ignore_ascii_case(&needle) {
            sum_lat += (row.le_lat + row.he_lat) / 2.0;
            sum_lon += (row.le_lon + row.he_lon) / 2.0;
            count += 1;
        }
    }
    if count == 0 {
        None
    } else {
        Some((sum_lat / count as f64, sum_lon / count as f64))
    }
}

/// Great-circle distance in meters between two WGS84 points.
/// Exposed so the auto-start watcher can compute "how close is the
/// aircraft to the departure airport" without re-implementing the
/// haversine formula.
pub fn distance_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    haversine_m(lat1, lon1, lat2, lon2)
}

/// One result row from `find_nearest_airports`. Distance in meters
/// from the query point. The `position` is the same average-of-runway-
/// thresholds point that `airport_position()` would return.
#[derive(Debug, Clone, serde::Serialize)]
pub struct NearestAirport {
    pub icao: String,
    pub lat: f64,
    pub lon: f64,
    pub distance_m: f64,
    /// Longest runway at this airport, in feet — useful for the UI to
    /// show "is this strip even big enough for what I'm flying" without
    /// pulling extra data.
    pub longest_runway_ft: f32,
}

/// Find airports within `max_radius_m` of the given point, sorted by
/// distance ascending, capped at `limit` results.
///
/// Used by the divert-detection logic: when a pilot lands somewhere
/// other than their planned `arr_airport`, we surface the nearest few
/// airports as the "you actually landed at X" candidate list. The
/// result is grouped per airport (multiple runways collapsed) and
/// each group keeps the single closest runway threshold as its
/// distance — so a long runway whose far end is closer than the near
/// end of a tiny grass strip wins on proximity even if the centroids
/// would tie.
///
/// Returns an empty vec when no airport is in range — caller decides
/// how to recover (we typically fall back to "manual override").
pub fn find_nearest_airports(
    lat: f64,
    lon: f64,
    max_radius_m: f64,
    limit: usize,
) -> Vec<NearestAirport> {
    use std::collections::HashMap;
    let table = runways();
    let mut by_apt: HashMap<&str, (f64, f64, f64, f32)> = HashMap::new();
    // Coarse bounding-box pre-filter so we don't haversine the entire
    // world catalog. 1 degree latitude ≈ 111 km, so for a 50 nmi
    // (~93 km) max-radius we look ~1 degree out generously.
    let bbox_deg = (max_radius_m / 100_000.0).max(0.5);
    for row in table.iter() {
        let approx_lat = (row.le_lat + row.he_lat) / 2.0;
        let approx_lon = (row.le_lon + row.he_lon) / 2.0;
        if (approx_lat - lat).abs() > bbox_deg
            || (approx_lon - lon).abs() > bbox_deg
        {
            continue;
        }
        // Use the closer of the two threshold positions for each
        // runway as that runway's distance to the query. The pilot
        // touched down somewhere on the field — the nearer threshold
        // is the better proxy than the centroid.
        let d_le = haversine_m(lat, lon, row.le_lat, row.le_lon);
        let d_he = haversine_m(lat, lon, row.he_lat, row.he_lon);
        let d = d_le.min(d_he);
        if d > max_radius_m {
            continue;
        }
        let entry = by_apt
            .entry(&row.airport_ident)
            .or_insert((approx_lat, approx_lon, d, 0.0));
        if d < entry.2 {
            entry.0 = approx_lat;
            entry.1 = approx_lon;
            entry.2 = d;
        }
        if row.length_ft > entry.3 {
            entry.3 = row.length_ft;
        }
    }
    let mut out: Vec<NearestAirport> = by_apt
        .into_iter()
        .map(|(icao, (la, lo, d, len))| NearestAirport {
            icao: icao.to_string(),
            lat: la,
            lon: lo,
            distance_m: d,
            longest_runway_ft: len,
        })
        .collect();
    out.sort_by(|a, b| {
        a.distance_m
            .partial_cmp(&b.distance_m)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

/// Look up the runway for a touchdown coordinate.
///
/// `aircraft_heading_true_deg` is used to disambiguate between the two
/// ends of a runway (08 vs 26 etc.) — the end whose published heading
/// is closest to the aircraft heading wins.
///
/// Returns `None` when no runway is within ~3 km of the point.
pub fn lookup_runway(
    lat: f64,
    lon: f64,
    aircraft_heading_true_deg: f32,
) -> Option<RunwayMatch> {
    let table = runways();

    // Bounding-box prefilter. With ~48k rows and a 0.1° square window
    // we drop the candidate set to a handful (typically <10) before
    // doing any trig.
    let lat_min = lat - BBOX_HALF_DEG;
    let lat_max = lat + BBOX_HALF_DEG;
    let lon_min = lon - BBOX_HALF_DEG;
    let lon_max = lon + BBOX_HALF_DEG;

    let mut best: Option<(RunwayMatch, f64)> = None;

    for row in table.iter() {
        // Either end inside the bbox is enough — a long runway can
        // straddle the box if the pilot landed near one end.
        let le_in = row.le_lat >= lat_min
            && row.le_lat <= lat_max
            && row.le_lon >= lon_min
            && row.le_lon <= lon_max;
        let he_in = row.he_lat >= lat_min
            && row.he_lat <= lat_max
            && row.he_lon >= lon_min
            && row.he_lon <= lon_max;
        if !le_in && !he_in {
            continue;
        }

        // Pick the threshold the pilot crossed: whichever end's published
        // heading is closer to the aircraft heading at touchdown.
        let le_diff = heading_diff(aircraft_heading_true_deg, row.le_heading_true);
        let he_diff = heading_diff(aircraft_heading_true_deg, row.he_heading_true);
        let (threshold_lat, threshold_lon, end_lat, end_lon, runway_ident, runway_heading) =
            if le_diff <= he_diff {
                (
                    row.le_lat,
                    row.le_lon,
                    row.he_lat,
                    row.he_lon,
                    row.le_ident.clone(),
                    row.le_heading_true,
                )
            } else {
                (
                    row.he_lat,
                    row.he_lon,
                    row.le_lat,
                    row.le_lon,
                    row.he_ident.clone(),
                    row.he_heading_true,
                )
            };

        // Cheap rejection: if the threshold itself is more than ~5 km
        // away the pilot definitely didn't land here, regardless of
        // bbox membership of the other end.
        let d_threshold = haversine_m(threshold_lat, threshold_lon, lat, lon);
        if d_threshold > DEFAULT_MAX_DISTANCE_M + (row.length_ft as f64 * 0.3048) {
            continue;
        }

        // Centerline math (great-circle cross-track / along-track).
        let theta_ab = initial_bearing_rad(threshold_lat, threshold_lon, end_lat, end_lon);
        let theta_ac = initial_bearing_rad(threshold_lat, threshold_lon, lat, lon);
        let d_ab = d_threshold; // m
        // Cross-track distance: signed by the sin() of the bearing
        // difference. Positive = right of track in the landing direction
        // (because we measure from threshold toward the far end).
        let xtd_m = (d_ab / EARTH_RADIUS_M).sin() * (theta_ac - theta_ab).sin();
        let xtd_m = xtd_m.asin() * EARTH_RADIUS_M;
        // Along-track distance magnitude: how far along the centerline
        // the touchdown projects from the threshold.
        let cos_arg = (d_ab / EARTH_RADIUS_M).cos() / (xtd_m / EARTH_RADIUS_M).cos();
        // Clamp to [-1,1] — small floating-point drift can push it out
        // of range when the pilot lands ~exactly on the threshold.
        let cos_arg = cos_arg.clamp(-1.0, 1.0);
        let along_m = cos_arg.acos() * EARTH_RADIUS_M;
        // v0.5.20: signed along-track. The acos() above always returns
        // a non-negative value, so undershoots before the threshold
        // and overshoots past the threshold both reported as positive
        // distances pre-v0.5.20 — the doc-string promised negative =
        // undershoot, but the math couldn't deliver that. Server-side
        // analysis (Volanta-style "where on the runway did the pilot
        // touch") needs the sign to distinguish "landed 4 m past
        // threshold" (= chevron landing, dramatic but legal) from
        // "touched 4 m short of threshold" (= undershoot, bad).
        //
        // Sign by bearing diff: if the bearing from threshold to
        // touchdown is within ±90° of the runway heading, the
        // touchdown is on the runway side of the threshold (positive,
        // overshoot); otherwise it's on the approach side (negative,
        // undershoot).
        let mut bearing_diff = theta_ac - theta_ab;
        // Normalise to (-π, π].
        while bearing_diff > std::f64::consts::PI {
            bearing_diff -= 2.0 * std::f64::consts::PI;
        }
        while bearing_diff <= -std::f64::consts::PI {
            bearing_diff += 2.0 * std::f64::consts::PI;
        }
        let along_signed_m = if bearing_diff.abs() > std::f64::consts::FRAC_PI_2 {
            -along_m
        } else {
            along_m
        };
        let along_ft = along_signed_m * 3.280_839_895;

        let centerline_distance_abs_ft = xtd_m.abs() * 3.280_839_895;

        let side = if xtd_m.abs() < CENTERLINE_TOLERANCE_M {
            "CENTER"
        } else if xtd_m > 0.0 {
            "RIGHT"
        } else {
            "LEFT"
        };

        let candidate = RunwayMatch {
            airport_ident: row.airport_ident.clone(),
            runway_ident,
            heading_true_deg: runway_heading,
            length_ft: row.length_ft,
            width_ft: row.width_ft,
            surface: row.surface.clone(),
            threshold_lat,
            threshold_lon,
            end_lat,
            end_lon,
            centerline_distance_m: xtd_m,
            centerline_distance_abs_ft,
            touchdown_distance_from_threshold_ft: along_ft,
            side: side.to_string(),
        };

        // Pick the runway with the smallest perpendicular distance to
        // the centerline. This is what disambiguates parallel runways
        // (26L vs 26R) — the threshold-distance heuristic alone can't
        // tell them apart because both thresholds are on the same end.
        let score = xtd_m.abs();
        match &best {
            Some((_, best_score)) if *best_score <= score => {}
            _ => best = Some((candidate, score)),
        }
    }

    best.and_then(|(m, _)| {
        // Final sanity check on the absolute distance — refuse to
        // return something obviously wrong.
        let d = haversine_m(m.threshold_lat, m.threshold_lon, lat, lon);
        if d > DEFAULT_MAX_DISTANCE_M + (m.length_ft as f64 * 0.3048) {
            None
        } else {
            Some(m)
        }
    })
}

/// Quelle aus der die `RunwayMatch` stammt. Wird im LandingRecord
/// persistiert und im Activity-Log surface'd damit der Pilot sieht ob
/// gerade Navigraph-Daten oder der OurAirports-Fallback aktiv war.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunwaySource {
    /// Match aus VPS-Navdata (Aerosoft DFD AIRAC 2604+).
    Navigraph,
    /// Match aus eingebauter OurAirports-CSV (Fallback bei VPS-Outage
    /// oder unbekanntem ICAO).
    OurAirportsFallback,
}

/// Wie `lookup_runway`, aber gegen die NavRunway-Liste eines per VPS
/// geladenen Airports. Mathematik ist identisch — die Quelle ist nur
/// genauer (Jeppesen-Threshold-Koordinaten statt Community-CSV).
///
/// Verhalten:
///   * Filtert NavRunways auf jene mit `heading_diff(aircraft, true_course)
///     < 90°` (= Landerichtung passt grob, blockt 17 vs 35).
///   * Rechnet pro verbleibendem Kandidat Cross-Track + Along-Track
///     gegen `threshold` → `end` und wählt am Ende die Bahn mit dem
///     kleinsten `|centerline_distance_m|`. **Wichtig für Parallelbahnen
///     (26L/26R, 09L/09C/09R)** — heading allein kann sie nicht
///     unterscheiden weil sie identische Magnetic-Courses haben, das
///     XTD-Minimum schon. Gleiches Tie-Break-Verfahren wie der
///     OurAirports-Pfad (siehe `lookup_runway`).
///   * Returnt `None` wenn keine Bahn innerhalb von `3 km + length`
///     der Schwelle liegt (= Pilot ist nicht auf diesem Airport
///     gelandet — Caller soll auf OurAirports zurückfallen).
pub fn lookup_runway_in_nav(
    lat: f64,
    lon: f64,
    aircraft_heading_true_deg: f32,
    airport: &aeroacars_mqtt::navdata::NavAirport,
) -> Option<RunwayMatch> {
    if airport.runways.is_empty() {
        return None;
    }

    let mut best: Option<(RunwayMatch, f64)> = None;

    for rwy in &airport.runways {
        // > 90° heading-diff → other landing direction (17 vs 35).
        // Skip so parallel-runway tie-break is purely XTD-driven.
        if heading_diff(aircraft_heading_true_deg, rwy.true_course as f32) > 90.0 {
            continue;
        }

        let threshold_lat = rwy.threshold.lat;
        let threshold_lon = rwy.threshold.lon;
        let end_lat = rwy.far_end.lat;
        let end_lon = rwy.far_end.lon;
        let runway_heading = rwy.true_course as f32;
        let length_ft = rwy.length_ft as f32;
        let width_ft = rwy.width_ft.unwrap_or(0) as f32;
        let surface = rwy.surface.clone().unwrap_or_default();

        let d_threshold = haversine_m(threshold_lat, threshold_lon, lat, lon);
        if d_threshold > DEFAULT_MAX_DISTANCE_M + (length_ft as f64 * 0.3048) {
            continue;
        }

        // Same cross-track / along-track math as the CSV path. Kept
        // verbatim so MS713-equivalent calls reproduce identical signs.
        let theta_ab = initial_bearing_rad(threshold_lat, threshold_lon, end_lat, end_lon);
        let theta_ac = initial_bearing_rad(threshold_lat, threshold_lon, lat, lon);
        let d_ab = d_threshold;
        let xtd_m = (d_ab / EARTH_RADIUS_M).sin() * (theta_ac - theta_ab).sin();
        let xtd_m = xtd_m.asin() * EARTH_RADIUS_M;
        let cos_arg = (d_ab / EARTH_RADIUS_M).cos() / (xtd_m / EARTH_RADIUS_M).cos();
        let cos_arg = cos_arg.clamp(-1.0, 1.0);
        let along_m = cos_arg.acos() * EARTH_RADIUS_M;
        let mut bearing_diff = theta_ac - theta_ab;
        while bearing_diff > std::f64::consts::PI {
            bearing_diff -= 2.0 * std::f64::consts::PI;
        }
        while bearing_diff <= -std::f64::consts::PI {
            bearing_diff += 2.0 * std::f64::consts::PI;
        }
        let along_signed_m = if bearing_diff.abs() > std::f64::consts::FRAC_PI_2 {
            -along_m
        } else {
            along_m
        };
        let along_ft = along_signed_m * 3.280_839_895;
        let centerline_distance_abs_ft = xtd_m.abs() * 3.280_839_895;
        let side = if xtd_m.abs() < CENTERLINE_TOLERANCE_M {
            "CENTER"
        } else if xtd_m > 0.0 {
            "RIGHT"
        } else {
            "LEFT"
        };

        let candidate = RunwayMatch {
            airport_ident: airport.icao.clone(),
            runway_ident: rwy.designator.clone(),
            heading_true_deg: runway_heading,
            length_ft,
            width_ft,
            surface,
            threshold_lat,
            threshold_lon,
            end_lat,
            end_lon,
            centerline_distance_m: xtd_m,
            centerline_distance_abs_ft,
            touchdown_distance_from_threshold_ft: along_ft,
            side: side.to_string(),
        };

        let score = xtd_m.abs();
        match &best {
            Some((_, best_score)) if *best_score <= score => {}
            _ => best = Some((candidate, score)),
        }
    }

    best.map(|(m, _)| m)
}

/// Try Navigraph first, fall back to OurAirports. Returns the match
/// plus the source that produced it — Callers feed both into the
/// LandingRecord so the audit-log shows where the numbers came from.
///
/// `airport_nav` is the NavAirport from VPS (None when the pilot
/// flight had a VPS-outage or an unknown ICAO). Pass `None` to skip
/// the Navigraph path entirely.
pub fn lookup_runway_with_fallback(
    lat: f64,
    lon: f64,
    aircraft_heading_true_deg: f32,
    airport_nav: Option<&aeroacars_mqtt::navdata::NavAirport>,
) -> Option<(RunwayMatch, RunwaySource)> {
    if let Some(apt) = airport_nav {
        if let Some(m) = lookup_runway_in_nav(lat, lon, aircraft_heading_true_deg, apt) {
            return Some((m, RunwaySource::Navigraph));
        }
    }
    lookup_runway(lat, lon, aircraft_heading_true_deg)
        .map(|m| (m, RunwaySource::OurAirportsFallback))
}

/// v0.8.0 — signed along-track Distanz vom Threshold-Punkt zum
/// Sample-Punkt entlang der Runway-Centerline, in Metern. Positiv =
/// Sample ist past-threshold (auf Runway-Seite), negativ = Sample
/// ist auf der Anflug-Seite (Pilot mid-final). Diese Funktion ist
/// die geometrische Kernoperation für TCH-actual-Measurement: man
/// scannt den snapshot_buffer und nimmt den ersten Sample wo das
/// Vorzeichen flippt (= echtes Threshold-Crossing).
///
/// Mathematik ist identisch zur Inline-Implementierung in
/// `lookup_runway` / `lookup_runway_in_nav` — extrahiert, damit
/// step_flight pro Sample iterieren kann ohne den ganzen Match-Pfad
/// durchzulaufen.
pub fn along_track_m_signed(
    threshold_lat: f64,
    threshold_lon: f64,
    end_lat: f64,
    end_lon: f64,
    sample_lat: f64,
    sample_lon: f64,
) -> f64 {
    let d_threshold = haversine_m(threshold_lat, threshold_lon, sample_lat, sample_lon);
    let theta_ab = initial_bearing_rad(threshold_lat, threshold_lon, end_lat, end_lon);
    let theta_ac = initial_bearing_rad(threshold_lat, threshold_lon, sample_lat, sample_lon);
    let xtd = (d_threshold / EARTH_RADIUS_M).sin() * (theta_ac - theta_ab).sin();
    let xtd = xtd.asin() * EARTH_RADIUS_M;
    let cos_arg = ((d_threshold / EARTH_RADIUS_M).cos() / (xtd / EARTH_RADIUS_M).cos())
        .clamp(-1.0, 1.0);
    let along_m = cos_arg.acos() * EARTH_RADIUS_M;
    let mut bearing_diff = theta_ac - theta_ab;
    while bearing_diff > std::f64::consts::PI {
        bearing_diff -= 2.0 * std::f64::consts::PI;
    }
    while bearing_diff <= -std::f64::consts::PI {
        bearing_diff += 2.0 * std::f64::consts::PI;
    }
    if bearing_diff.abs() > std::f64::consts::FRAC_PI_2 {
        -along_m
    } else {
        along_m
    }
}

/// Great-circle distance in meters.
fn haversine_m(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let dphi = (lat2 - lat1).to_radians();
    let dlam = (lon2 - lon1).to_radians();
    let a = (dphi / 2.0).sin().powi(2) + phi1.cos() * phi2.cos() * (dlam / 2.0).sin().powi(2);
    2.0 * EARTH_RADIUS_M * a.sqrt().asin()
}

/// Initial bearing (forward azimuth) from point 1 → point 2, in radians,
/// normalized to [0, 2π).
fn initial_bearing_rad(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let phi1 = lat1.to_radians();
    let phi2 = lat2.to_radians();
    let dlam = (lon2 - lon1).to_radians();
    let y = dlam.sin() * phi2.cos();
    let x = phi1.cos() * phi2.sin() - phi1.sin() * phi2.cos() * dlam.cos();
    let mut b = y.atan2(x);
    if b < 0.0 {
        b += std::f64::consts::TAU;
    }
    b
}

fn initial_bearing_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    initial_bearing_rad(lat1, lon1, lat2, lon2).to_degrees()
}

/// Smallest unsigned angular difference between two bearings in degrees.
/// Result is in [0, 180].
fn heading_diff(a: f32, b: f32) -> f32 {
    let mut d = (a - b).abs() % 360.0;
    if d > 180.0 {
        d = 360.0 - d;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    // EDDP/26R from the bundled CSV:
    //   le="08L" 51.43119812011719, 12.215800285339355, hdg 85.7
    //   he="26R" 51.43360137939453, 12.267399787902832, hdg 265.7
    //   length 11811 ft
    const EDDP_26R_THR_LAT: f64 = 51.433_601_379_392_15;
    const EDDP_26R_THR_LON: f64 = 12.267_399_787_902_832;
    const EDDP_26R_HEADING: f32 = 265.7;

    #[test]
    fn touchdown_at_eddp_26r_threshold() {
        let m = lookup_runway(EDDP_26R_THR_LAT, EDDP_26R_THR_LON, EDDP_26R_HEADING)
            .expect("should resolve to EDDP/26R");
        assert_eq!(m.airport_ident, "EDDP");
        assert_eq!(m.runway_ident, "26R");
        // Centerline ≈ 0 (we sit exactly on the threshold which is on the
        // centerline by definition).
        assert!(
            m.centerline_distance_m.abs() < 1.0,
            "centerline_distance_m = {} (expected ≈0)",
            m.centerline_distance_m
        );
        // Along-track ≈ 0 ft.
        assert!(
            m.touchdown_distance_from_threshold_ft.abs() < 5.0,
            "along-track = {} ft (expected ≈0)",
            m.touchdown_distance_from_threshold_ft
        );
        assert_eq!(m.side, "CENTER");
    }

    #[test]
    fn touchdown_offset_right_and_down_runway() {
        // Construct a synthetic touchdown 1000 m down the runway and 10 m
        // right of centerline. We project from the threshold along the
        // landing bearing for the along-track component, then 90° to the
        // right (bearing + 90°) for the cross-track offset.
        let landing_bearing = (EDDP_26R_HEADING as f64).to_radians();
        let right_bearing = landing_bearing + std::f64::consts::FRAC_PI_2;

        let (lat1, lon1) =
            destination(EDDP_26R_THR_LAT, EDDP_26R_THR_LON, landing_bearing, 1000.0);
        let (lat2, lon2) = destination(lat1, lon1, right_bearing, 10.0);

        let m = lookup_runway(lat2, lon2, EDDP_26R_HEADING)
            .expect("should resolve to EDDP/26R");
        assert_eq!(m.airport_ident, "EDDP");
        assert_eq!(m.runway_ident, "26R");
        assert_eq!(m.side, "RIGHT");
        // 10 m right (positive). Tolerance is ±1.5 m to absorb the
        // spherical drift introduced by chaining two destination()
        // calls (the second leg's "perpendicular" direction is taken
        // at the displaced point, not the threshold, so the resulting
        // cross-track from the original great circle ends up ~0.8 m
        // shy of the leg length over 1 km of travel).
        assert!(
            (m.centerline_distance_m - 10.0).abs() < 1.5,
            "centerline_distance_m = {} (expected ≈10)",
            m.centerline_distance_m
        );
        // 1000 m → ~3280.84 ft. Allow ±5 ft.
        assert!(
            (m.touchdown_distance_from_threshold_ft - 3280.84).abs() < 5.0,
            "along-track = {} ft (expected ≈3280.84)",
            m.touchdown_distance_from_threshold_ft
        );
    }

    /// Forward-geodesic helper for the synthetic test — given a starting
    /// point, a true bearing in radians, and a distance in meters, return
    /// the destination on the sphere. Inverse of `initial_bearing_rad`.
    fn destination(lat: f64, lon: f64, bearing_rad: f64, dist_m: f64) -> (f64, f64) {
        let phi1 = lat.to_radians();
        let lam1 = lon.to_radians();
        let delta = dist_m / EARTH_RADIUS_M;
        let phi2 =
            (phi1.sin() * delta.cos() + phi1.cos() * delta.sin() * bearing_rad.cos()).asin();
        let lam2 = lam1
            + (bearing_rad.sin() * delta.sin() * phi1.cos())
                .atan2(delta.cos() - phi1.sin() * phi2.sin());
        (phi2.to_degrees(), lam2.to_degrees())
    }

    #[test]
    fn undershoot_before_threshold_is_negative() {
        // v0.5.20: synthetic touchdown 200 m short of the threshold
        // along the runway axis. Pre-v0.5.20 this returned +200 m
        // (indistinguishable from a 200 m overshoot); v0.5.20 returns
        // a signed value (~-656 ft).
        //
        // Constructed by walking 200 m in the OPPOSITE direction of
        // the runway heading (= bearing + 180°) from the threshold.
        let landing_bearing = (EDDP_26R_HEADING as f64).to_radians();
        let approach_bearing = landing_bearing + std::f64::consts::PI;
        let (lat, lon) = destination(
            EDDP_26R_THR_LAT,
            EDDP_26R_THR_LON,
            approach_bearing,
            200.0,
        );
        let m = lookup_runway(lat, lon, EDDP_26R_HEADING)
            .expect("should still resolve to EDDP/26R (pilot mid-final, 200 m short)");
        // 200 m → ~656.17 ft. Negative because pilot is on the
        // approach side of the threshold.
        assert!(
            (m.touchdown_distance_from_threshold_ft + 656.17).abs() < 5.0,
            "along-track = {} ft (expected ≈-656.17)",
            m.touchdown_distance_from_threshold_ft
        );
    }

    #[test]
    fn heading_diff_wraps_correctly() {
        assert!((heading_diff(10.0, 350.0) - 20.0).abs() < 0.001);
        assert!((heading_diff(350.0, 10.0) - 20.0).abs() < 0.001);
        assert!((heading_diff(85.7, 265.7) - 180.0).abs() < 0.001);
        assert!((heading_diff(266.0, 265.7) - 0.3).abs() < 0.001);
    }

    // ─── v0.8.0: Navigraph-aware lookup tests ────────────────────────

    use aeroacars_mqtt::navdata::{NavAirport, NavIls, NavPoint, NavRunway};

    /// MS713-Anchor: OLBA RWY 17 mit echten Aerosoft-DFD-2604-Threshold-
    /// Koordinaten. Wir bauen den NavAirport synthetisch nach (die Werte
    /// kommen 1:1 aus `E:\NAV_DATA\Airports.txt` R-Record).
    fn olba_nav_fixture() -> NavAirport {
        NavAirport {
            cycle: "2604".to_string(),
            valid_to: "2026-05-14".to_string(),
            icao: "OLBA".to_string(),
            name: "Rafic Hariri Intl".to_string(),
            latitude: 33.819_050,
            longitude: 35.490_031,
            elevation_ft: Some(85),
            runways: vec![
                NavRunway {
                    designator: "17".to_string(),
                    magnetic_course: 172.0,
                    // Computed bearing 33.838364,35.486978 → 33.809288,35.488861.
                    true_course: 176.94,
                    length_ft: 10663,
                    width_ft: Some(148),
                    surface: Some("ASP".to_string()),
                    threshold: NavPoint {
                        lat: 33.838_364,
                        lon: 35.486_978,
                        elev_ft: Some(85),
                    },
                    far_end: NavPoint {
                        lat: 33.809_288,
                        lon: 35.488_861,
                        elev_ft: Some(36),
                    },
                    displaced_threshold_ft: 0,
                    ils: Some(NavIls {
                        freq_mhz: 109.5,
                        course: 172.0,
                        category: 1,
                    }),
                    glideslope_angle: 3.0,
                    tch_ft: 49,
                },
                NavRunway {
                    designator: "35".to_string(),
                    magnetic_course: 352.0,
                    true_course: 356.94,
                    length_ft: 10663,
                    width_ft: Some(148),
                    surface: Some("ASP".to_string()),
                    threshold: NavPoint {
                        lat: 33.809_288,
                        lon: 35.488_861,
                        elev_ft: Some(36),
                    },
                    far_end: NavPoint {
                        lat: 33.838_364,
                        lon: 35.486_978,
                        elev_ft: Some(85),
                    },
                    displaced_threshold_ft: 2690,
                    ils: None,
                    glideslope_angle: 3.0,
                    tch_ft: 50,
                },
            ],
        }
    }

    #[test]
    fn nav_lookup_picks_landing_runway_by_heading() {
        let apt = olba_nav_fixture();
        // Aircraft heading 175° → RWY 17 (true_course 176.94).
        let m = lookup_runway_in_nav(33.838_364, 35.486_978, 175.0, &apt)
            .expect("touchdown at threshold should resolve");
        assert_eq!(m.airport_ident, "OLBA");
        assert_eq!(m.runway_ident, "17");
        assert!(m.centerline_distance_m.abs() < 1.0);
        assert!(m.touchdown_distance_from_threshold_ft.abs() < 5.0);

        // Aircraft heading 355° → RWY 35 (true_course 356.94).
        let m = lookup_runway_in_nav(33.809_288, 35.488_861, 355.0, &apt)
            .expect("RWY 35 threshold");
        assert_eq!(m.runway_ident, "35");
    }

    /// MS713 cross-track sanity: pilot touched down somewhere LEFT of
    /// the OLBA RWY 17 centerline. Against the corrected Navigraph
    /// threshold we get a LEFT/negative xtd value. The pre-v0.8.0 code
    /// against OurAirports' wrong threshold gave a positive (RIGHT)
    /// xtd — that was the bug.
    ///
    /// We construct a synthetic touchdown ~250 m down the RWY and ~6.6 m
    /// LEFT of the centerline (= the actual recorded MS713 position).
    #[test]
    fn nav_lookup_ms713_anchor_left_of_centerline() {
        let apt = olba_nav_fixture();
        let landing_bearing_rad = 176.94_f64.to_radians();
        let left_bearing = landing_bearing_rad - std::f64::consts::FRAC_PI_2;
        // 250 m along + 6.6 m left.
        let (lat1, lon1) = destination(33.838_364, 35.486_978, landing_bearing_rad, 250.0);
        let (lat2, lon2) = destination(lat1, lon1, left_bearing, 6.6);
        let m = lookup_runway_in_nav(lat2, lon2, 177.0, &apt).expect("MS713 should resolve");
        assert_eq!(m.runway_ident, "17");
        assert_eq!(m.side, "LEFT", "MS713 was left of centerline");
        // Negative cross-track = LEFT, ~ -6.6 m with a small spherical
        // drift tolerance.
        assert!(
            (m.centerline_distance_m + 6.6).abs() < 1.5,
            "xtd = {} m (expected ≈ -6.6)",
            m.centerline_distance_m
        );
    }

    #[test]
    fn nav_lookup_rejects_distant_touchdown() {
        let apt = olba_nav_fixture();
        // Far-away touchdown (Cyprus) — should NOT match OLBA.
        let m = lookup_runway_in_nav(35.0, 33.0, 180.0, &apt);
        assert!(m.is_none());
    }

    #[test]
    fn fallback_uses_navigraph_when_available() {
        let apt = olba_nav_fixture();
        let (m, src) =
            lookup_runway_with_fallback(33.838_364, 35.486_978, 175.0, Some(&apt))
                .expect("should match Navigraph runway");
        assert_eq!(src, RunwaySource::Navigraph);
        assert_eq!(m.runway_ident, "17");
    }

    #[test]
    fn fallback_uses_ourairports_when_nav_none() {
        // No NavAirport provided → falls back to bundled CSV. EDDP/26R
        // is in OurAirports, so we get a match flagged as fallback.
        let (m, src) = lookup_runway_with_fallback(
            EDDP_26R_THR_LAT,
            EDDP_26R_THR_LON,
            EDDP_26R_HEADING,
            None,
        )
        .expect("OurAirports has EDDP");
        assert_eq!(src, RunwaySource::OurAirportsFallback);
        assert_eq!(m.airport_ident, "EDDP");
        assert_eq!(m.runway_ident, "26R");
    }

    /// QS-Finding 2026-05-13: Parallelbahnen müssen via Cross-Track
    /// disambiguiert werden, nicht via Heading. Wir simulieren EDDF
    /// mit zwei parallelen Bahnen (07L und 07R, ~520 m seitlich
    /// versetzt). Die alte `min_by(heading_diff)`-Logik konnte hier die
    /// falsche Bahn picken weil beide gleichen `true_course` haben.
    fn parallel_runway_fixture() -> NavAirport {
        // 07L Threshold bei (50.05, 8.55), 07R 520 m süd-davon
        // (= ~0.00467° Lat). Beide laufen Richtung 070° (~3000 m lang).
        let landing_bearing = 70.0_f64.to_radians();
        let length_m = 3000.0_f64;
        let thr_07l_lat = 50.05_f64;
        let thr_07l_lon = 8.55_f64;
        let (end_07l_lat, end_07l_lon) =
            destination(thr_07l_lat, thr_07l_lon, landing_bearing, length_m);
        // Parallel 520 m nach Süden (= rechts der 07L Landerichtung,
        // bearing + 90° = 160°).
        let perp_right = landing_bearing + std::f64::consts::FRAC_PI_2;
        let (thr_07r_lat, thr_07r_lon) =
            destination(thr_07l_lat, thr_07l_lon, perp_right, 520.0);
        let (end_07r_lat, end_07r_lon) =
            destination(thr_07r_lat, thr_07r_lon, landing_bearing, length_m);

        let make_rwy = |des: &str, t_lat, t_lon, e_lat, e_lon| NavRunway {
            designator: des.to_string(),
            magnetic_course: 70.0,
            true_course: 70.0,
            length_ft: 9842, // ~3000 m
            width_ft: Some(148),
            surface: Some("ASP".to_string()),
            threshold: NavPoint {
                lat: t_lat,
                lon: t_lon,
                elev_ft: Some(364),
            },
            far_end: NavPoint {
                lat: e_lat,
                lon: e_lon,
                elev_ft: Some(364),
            },
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: 3.0,
            tch_ft: 50,
        };

        NavAirport {
            cycle: "2604".to_string(),
            valid_to: "2026-05-14".to_string(),
            icao: "EDDF".to_string(),
            name: "Frankfurt".to_string(),
            latitude: thr_07l_lat,
            longitude: thr_07l_lon,
            elevation_ft: Some(364),
            runways: vec![
                make_rwy("07L", thr_07l_lat, thr_07l_lon, end_07l_lat, end_07l_lon),
                make_rwy("07R", thr_07r_lat, thr_07r_lon, end_07r_lat, end_07r_lon),
            ],
        }
    }

    #[test]
    fn nav_lookup_disambiguates_parallels_by_xtd() {
        let apt = parallel_runway_fixture();
        // Landed 1000 m down 07R, ~5 m right of its centerline.
        // 07L's centerline is ~520 m away → XTD of 07R must win.
        let landing_bearing = 70.0_f64.to_radians();
        let right_perp = landing_bearing + std::f64::consts::FRAC_PI_2;
        // First reach 07R threshold (520 m perp from 07L)…
        let (thr_07r_lat, thr_07r_lon) =
            destination(50.05, 8.55, right_perp, 520.0);
        // …then 1000 m down 07R…
        let (along_lat, along_lon) =
            destination(thr_07r_lat, thr_07r_lon, landing_bearing, 1000.0);
        // …then 5 m right of the 07R centerline.
        let (td_lat, td_lon) = destination(along_lat, along_lon, right_perp, 5.0);

        let m = lookup_runway_in_nav(td_lat, td_lon, 70.0, &apt).expect("must resolve");
        assert_eq!(
            m.runway_ident, "07R",
            "got runway {} with xtd {} (expected 07R, xtd ≈ +5 m)",
            m.runway_ident, m.centerline_distance_m
        );
        assert_eq!(m.side, "RIGHT");
        // Tolerance ±2 m to absorb spherical drift from chained
        // destination() calls (same as `touchdown_offset_right_and_down_runway`).
        assert!(
            (m.centerline_distance_m - 5.0).abs() < 2.0,
            "xtd = {} (expected ≈ +5)",
            m.centerline_distance_m
        );
    }

    #[test]
    fn nav_lookup_picks_other_parallel_when_pilot_offset_is_negative() {
        // Inverse case: pilot lands closer to 07L → must pick 07L
        // not 07R, regardless of array order in the NavAirport.
        let apt = parallel_runway_fixture();
        let landing_bearing = 70.0_f64.to_radians();
        let right_perp = landing_bearing + std::f64::consts::FRAC_PI_2;
        // 1000 m down 07L, 3 m LEFT of 07L centerline.
        let (along_lat, along_lon) =
            destination(50.05, 8.55, landing_bearing, 1000.0);
        let (td_lat, td_lon) =
            destination(along_lat, along_lon, right_perp - std::f64::consts::PI, 3.0);

        let m = lookup_runway_in_nav(td_lat, td_lon, 70.0, &apt).expect("must resolve");
        assert_eq!(
            m.runway_ident, "07L",
            "got runway {} (expected 07L, pilot is between the parallels but closer to L)",
            m.runway_ident
        );
    }

    #[test]
    fn fallback_uses_ourairports_when_nav_lookup_misses() {
        // NavAirport provided but the touchdown is 1000 km away → nav
        // returns None, fallback kicks in and resolves to EDDP/26R from
        // OurAirports.
        let apt = olba_nav_fixture();
        let (m, src) = lookup_runway_with_fallback(
            EDDP_26R_THR_LAT,
            EDDP_26R_THR_LON,
            EDDP_26R_HEADING,
            Some(&apt),
        )
        .expect("OurAirports has EDDP");
        assert_eq!(src, RunwaySource::OurAirportsFallback);
        assert_eq!(m.airport_ident, "EDDP");
    }
}
