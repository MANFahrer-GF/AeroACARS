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

/// Embedded snapshot of the ourairports **airports** table (ident, type,
/// reference point). Same source, same public domain, same refresh cadence.
///
/// v0.19.3: added because every previous attempt to reason about airport
/// geometry was crippled by not having it. The runways table alone cannot tell
/// you where an airport *is*: for 74.7 % of them (one runway, two thresholds)
/// there is no way to tell a good coordinate from a corrupt one, so a repair
/// pass has to guess — and a guess put WAJI 5.2 nm from itself. Worse, 6,446
/// real ICAO airports and effectively every heliport have no usable runway
/// coordinates at all, so the client simply did not know where they were, and
/// fell back on asking phpVMS at runtime (which it might or might not answer).
///
/// A published reference point per airport removes all of that: corrupt
/// thresholds can be *identified* rather than guessed at, and every airport has
/// a position even when its runways don't.
const AIRPORTS_CSV: &str = include_str!("../data/ourairports-airports.csv");

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
    /// v0.19.3: did the CSV actually STATE these headings, or did we compute
    /// them from the two thresholds? It matters for the corrupt-coordinate
    /// repair: a computed heading is derived from the very coordinate we are
    /// trying to repair, so projecting along it would faithfully reproduce the
    /// corruption. A stated heading is independent evidence — and it is precise,
    /// where the runway's NAME is only rounded to 10° (using the name put KCLE's
    /// repaired threshold 529 m from its true position).
    headings_stated: bool,
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

/// Published reference point of an airport (its official ARP), from the
/// embedded airports table. `None` for an ident the table doesn't carry.
///
/// This is the airport's *position* — independent of its runway data, and
/// therefore the thing that lets us judge whether that runway data is any good.
/// It exists for every airport, including the ~6,400 ICAO fields and the 7,000+
/// heliports whose runway rows have no coordinates.
pub fn airport_reference(icao: &str) -> Option<(f64, f64)> {
    airports_by_ident()
        .get(&icao.trim().to_uppercase())
        .map(|a| (a.lat, a.lon))
}

/// The nearest airport whose published reference point is within `max_nm` —
/// EXCLUDING `exclude_icao`. Returns its ident and the distance in nm.
///
/// Answers "is this aircraft sitting on some *other* airport?" even when that
/// airport has no usable runway geometry — which is the case for ~6,400 ICAO
/// fields and effectively every heliport, i.e. exactly the ones the
/// runway-threshold search cannot see. Without this, "the pilot is parked at a
/// neighbouring field" and "the pilot put it down in a meadow short of his
/// destination" look identical, and they must not: the first must not be allowed
/// to file as a normal arrival, the second must.
pub fn nearest_airport_reference(
    lat: f64,
    lon: f64,
    max_nm: f64,
    exclude_icao: &str,
) -> Option<(String, f64)> {
    if !lat.is_finite() || !lon.is_finite() {
        return None;
    }
    let exclude = exclude_icao.trim().to_uppercase();
    let max_m = max_nm * 1852.0;
    // Coarse box first (1° lat ≈ 111 km; longitude shrinks with cos(lat)).
    let lat_span = (max_m / 111_000.0).max(0.05);
    let cos_lat = lat.to_radians().cos().abs().max(0.01);
    let lon_span = (lat_span / cos_lat).min(180.0);

    let mut best: Option<(String, f64)> = None;
    for (icao, entry) in airports_by_ident().iter() {
        if *icao == exclude || !entry.landable {
            continue;
        }
        let (alat, alon) = (entry.lat, entry.lon);
        if (alat - lat).abs() > lat_span || lon_delta_deg(alon, lon) > lon_span {
            continue;
        }
        let d = haversine_m(lat, lon, alat, alon);
        if d > max_m {
            continue;
        }
        if best.as_ref().is_none_or(|(_, bd)| d / 1852.0 < *bd) {
            best = Some((icao.clone(), d / 1852.0));
        }
    }
    best
}

/// One airport's reference point, plus whether an aircraft could actually have
/// come to rest there.
#[derive(Debug, Clone, Copy)]
struct AirportRef {
    lat: f64,
    lon: f64,
    /// A field an AEROPLANE could have come to rest on: an airport or a water
    /// base. Not a closed field (13,332 of those), not a balloonport — and not
    /// a heliport (23,116 of those).
    ///
    /// This exists for one question: "is this aircraft standing on some OTHER
    /// airport?" (`nearest_airport_reference`), which decides whether a pilot may
    /// confirm his planned destination as his actual landing site.
    ///
    /// Counting every ident answers "yes" almost anywhere near a city: 59 % of
    /// plausible off-field spots around a major airport have SOMETHING within
    /// 3 nm. Even at 1 nm, a hospital helipad is enough — and an A340 did not
    /// land on a hospital helipad. Blocking the honest pilot who put it down in a
    /// field short of his destination, because there is a helipad 0.9 nm away, is
    /// exactly the kind of nonsense this whole rewrite exists to end.
    ///
    /// (A helicopter that sets down on another PAD is not covered by this test —
    /// its hint carries no ICAO either, since the runway table has no heliport
    /// geometry. That gap is known and narrow: the flight is a rotorcraft
    /// operation whose pilot is filing by hand anyway.)
    landable: bool,
}

fn airports_by_ident() -> &'static std::collections::HashMap<String, AirportRef> {
    static CELL: OnceLock<std::collections::HashMap<String, AirportRef>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(AIRPORTS_CSV.as_bytes());
        let mut map = std::collections::HashMap::with_capacity(90_000);
        for rec in rdr.records().flatten() {
            let (Some(ident), Some(kind), Some(lat), Some(lon)) =
                (rec.get(0), rec.get(1), rec.get(2), rec.get(3))
            else {
                continue;
            };
            let (Ok(lat), Ok(lon)) = (lat.parse::<f64>(), lon.parse::<f64>()) else {
                continue;
            };
            if !lat.is_finite() || !lon.is_finite() {
                continue;
            }
            let landable = matches!(
                kind,
                "large_airport" | "medium_airport" | "small_airport" | "seaplane_base"
            );
            map.insert(
                ident.trim().to_uppercase(),
                AirportRef { lat, lon, landable },
            );
        }
        tracing::debug!(count = map.len(), "airport reference points parsed");
        map
    })
}

/// Ident → row indices, built once alongside the table. Turns "give me the
/// runways of EDDF" from a 48k-row linear scan into a hash lookup plus a
/// handful of rows.
///
/// This is what makes a per-tick `distance_to_airport_m` affordable, and it
/// is why the callers that used to memoize an airport's position to dodge the
/// scan (`divert_prefetch_decision`) no longer need to: the scan is gone.
fn runways_by_ident() -> &'static std::collections::HashMap<String, Vec<u32>> {
    static CELL: OnceLock<std::collections::HashMap<String, Vec<u32>>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut map: std::collections::HashMap<String, Vec<u32>> =
            std::collections::HashMap::with_capacity(24_000);
        for (i, row) in runways().iter().enumerate() {
            map.entry(row.airport_ident.to_uppercase())
                .or_default()
                .push(i as u32);
        }
        map
    })
}

/// Rows belonging to one airport ident (case-insensitive). Empty slice when
/// the ident isn't in the table.
fn rows_for_airport(icao: &str) -> impl Iterator<Item = &'static RunwayRow> {
    let table = runways();
    let idx = runways_by_ident()
        .get(&icao.trim().to_uppercase())
        .map(|v| v.as_slice())
        .unwrap_or(&[]);
    idx.iter().map(move |i| &table[*i as usize])
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
            let le_heading_csv = parse_f32(record.get(12));
            let he_heading_csv = parse_f32(record.get(18));
            let headings_stated = le_heading_csv.is_some() && he_heading_csv.is_some();
            let le_heading = le_heading_csv
                .unwrap_or_else(|| initial_bearing_deg(le_lat, le_lon, he_lat, he_lon) as f32);
            let he_ident = record.get(14).unwrap_or("").to_string();
            let he_heading = he_heading_csv
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
                headings_stated,
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
        let mut final_out: Vec<RunwayRow> = out
            .into_iter()
            .enumerate()
            .filter_map(|(i, r)| if to_drop[i] { None } else { Some(r) })
            .collect();
        tracing::debug!(count = final_out.len(), "runway table after ICAO dedupe");
        repair_corrupt_thresholds(&mut final_out);
        final_out
    })
}
/// A runway is, by definition, one runway-length from end to end. When the two
/// stored thresholds are much farther apart than that, one of them is wrong —
/// this is what identifies a corrupt row, and it needs no outside reference.
///
/// 3× the stated length (or 8 km when no length is given) is generous enough
/// that no real runway trips it, and tight enough to catch KCLE's 06R threshold,
/// which is stored 4 nm from the field: its row spans 13.3 km for a 3.0 km
/// runway.
fn row_is_internally_impossible(r: &RunwayRow) -> bool {
    let end_to_end_m = haversine_m(r.le_lat, r.le_lon, r.he_lat, r.he_lon);
    let length_m = r.length_ft as f64 * 0.3048;
    let plausible_m = if length_m > 0.0 {
        (length_m * 3.0).max(2_000.0)
    } else {
        8_000.0
    };
    end_to_end_m > plausible_m
}

/// A runway this far from its airport's published reference point is not that
/// airport's runway (UUMU has one in Belgorod, 319 nm away; 12WV has one in
/// Florida, 480 nm).
///
/// It must stay well clear of legitimate sprawl: measured over all 29,410
/// thresholds that have a reference point, 99 % are within 1.71 nm and the
/// largest genuine outlier is EHAM's Polderbaan at 3.77 nm. The corrupt ones do
/// not sit just past the edge — they are hundreds or thousands of nautical miles
/// out. 10 nm sits in the empty gap between the two populations.
///
/// Note this canNOT be used to detect KCLE's corruption: its bad threshold is
/// 4.0 nm from the field — *inside* the legitimate band, and closer in than
/// EHAM's Polderbaan. That is why corruption is identified by
/// `row_is_internally_impossible` and the reference point is used only to decide
/// WHICH end of a broken row is the bad one.
const RUNWAY_MISPLACED_NM: f64 = 10.0;

/// Repair — and where that's impossible, discard — thresholds that OurAirports
/// has in the wrong place.
///
/// Two real corruptions, both of which poison everything downstream:
///
///   * **A truncated coordinate.** KCLE's 06R threshold is stored as
///     41.300/-81.800 — four nautical miles south-east of the field. Its 24L end
///     is perfectly correct.
///   * **A wholly misplaced runway.** One UUMU row sits at 50.648/36.576 —
///     Belgorod, 319 nm away.
///
/// Either drags the airport's geometry to a phantom location, so
/// `arrival::locate` would treat a 2 nm circle around the phantom as "on the
/// field", and a genuine divert there would be filed as a normal arrival without
/// ever asking the pilot.
///
/// # The two questions, kept apart
///
/// **Is this row broken?** Answered from the row itself — its ends are farther
/// apart than the runway is long. Nothing external, so a sprawling airport
/// cannot be mistaken for bad data. (Three earlier attempts at this pass failed
/// precisely by conflating the two questions: judging "broken" by distance from
/// some centre threw away EHAM's Polderbaan, which is legitimately 3.8 nm from
/// the terminal, while missing KCLE's bad threshold, which is only 4.0 nm out.)
///
/// **Which end is broken?** Answered by the published reference point — the one
/// piece of evidence that is independent of the runway data. Without it the
/// question is unanswerable, and the two earlier attempts guessed: one dropped
/// the whole row (losing KCLE's good 24L threshold, and with it a real pilot's
/// runway match), the other took the "median" of the two ends — which is simply
/// the larger coordinate — and at WAJI declared the GOOD threshold the outlier,
/// moving a working airport 5.2 nm from itself.
fn repair_corrupt_thresholds(rows: &mut Vec<RunwayRow>) {
    let misplaced_m = RUNWAY_MISPLACED_NM * 1852.0;
    let mut repaired = 0_u32;
    let mut dropped = 0_u32;

    // Thresholds per airport, so a suspect runway can be checked against its
    // siblings before we throw it away. This matters: sometimes it is the
    // *reference point* that is wrong, not the runway. OurAirports puts FAHS's
    // reference point 2,446 nm from the airport while its two runways are
    // correct (verified against Navigraph, which agrees with the runways to
    // within 1 nm). Judging by the reference point alone would have discarded
    // two perfectly good runways.
    let siblings: std::collections::HashMap<String, Vec<(f64, f64)>> = {
        let mut m: std::collections::HashMap<String, Vec<(f64, f64)>> =
            std::collections::HashMap::new();
        for r in rows.iter() {
            let e = m.entry(r.airport_ident.to_uppercase()).or_default();
            e.push((r.le_lat, r.le_lon));
            e.push((r.he_lat, r.he_lon));
        }
        m
    };

    rows.retain_mut(|r| {
        let reference = airport_reference(&r.airport_ident);

        // A runway that is internally consistent but sits far from the airport
        // is either a misfiled runway (UUMU has one in Belgorod) or the symptom
        // of a wrong reference point (FAHS). Ask the other runways which it is:
        // a runway that agrees with its siblings is corroborated, and then the
        // reference point is the odd one out.
        if let Some((alat, alon)) = reference {
            let le_m = haversine_m(r.le_lat, r.le_lon, alat, alon);
            let he_m = haversine_m(r.he_lat, r.he_lon, alat, alon);
            if le_m > misplaced_m && he_m > misplaced_m {
                let corroborated = siblings
                    .get(&r.airport_ident.to_uppercase())
                    .map(|pts| {
                        pts.iter()
                            .filter(|(plat, plon)| {
                                // Not this row's own two thresholds.
                                haversine_m(*plat, *plon, r.le_lat, r.le_lon) > 1.0
                                    && haversine_m(*plat, *plon, r.he_lat, r.he_lon) > 1.0
                            })
                            .any(|(plat, plon)| {
                                haversine_m(*plat, *plon, r.le_lat, r.le_lon) <= misplaced_m
                            })
                    })
                    .unwrap_or(false);
                if !corroborated {
                    dropped += 1;
                    tracing::debug!(
                        ident = %r.airport_ident,
                        "runway row dropped: far from the airport and unsupported by any \
                         other runway there"
                    );
                    return false;
                }
                // Corroborated: the runways agree with each other and it is the
                // reference point that is wrong. Keep the runway, and do NOT let
                // that reference point decide anything else about this row.
                return true;
            }
        }

        if !row_is_internally_impossible(r) {
            return true;
        }

        // The row is broken. Which end?
        let Some((alat, alon)) = reference else {
            // No reference point → unanswerable. Drop the row rather than guess;
            // `arrival::locate` still places the airport by other means.
            dropped += 1;
            tracing::debug!(
                ident = %r.airport_ident,
                "runway row dropped: ends implausibly far apart and no reference point                  to tell us which one is wrong"
            );
            return false;
        };
        let le_m = haversine_m(r.le_lat, r.le_lon, alat, alon);
        let he_m = haversine_m(r.he_lat, r.he_lon, alat, alon);
        let le_bad = le_m > he_m;

        let length_m = r.length_ft as f64 * 0.3048;
        if length_m < 50.0 {
            dropped += 1;
            return false;
        }

        // Heading source, in order of trustworthiness:
        //   1. the CSV's stated heading — precise, and (unlike a bearing computed
        //      between the thresholds) not derived from the corrupt coordinate we
        //      are repairing;
        //   2. the runway's NAME ("24L" → 240°) — independent, but magnetic and
        //      rounded to 10°, which at KCLE alone would put the rebuilt threshold
        //      529 m off. A last resort, not a default.
        let (good_lat, good_lon, hdg) = if le_bad {
            let stated = r.headings_stated.then_some(r.he_heading_true as f64);
            let Some(h) = stated.or_else(|| heading_from_ident(&r.he_ident)) else {
                dropped += 1;
                return false;
            };
            (r.he_lat, r.he_lon, h)
        } else {
            let stated = r.headings_stated.then_some(r.le_heading_true as f64);
            let Some(h) = stated.or_else(|| heading_from_ident(&r.le_ident)) else {
                dropped += 1;
                return false;
            };
            (r.le_lat, r.le_lon, h)
        };

        let (lat, lon) = project(good_lat, good_lon, hdg, length_m);
        // The rebuilt threshold has to be plausibly at the airport. If it isn't,
        // our inputs were worse than we thought — drop the row rather than
        // publish an invented coordinate.
        if haversine_m(lat, lon, alat, alon) > misplaced_m {
            dropped += 1;
            tracing::debug!(
                ident = %r.airport_ident,
                "runway row dropped: reconstruction landed nowhere near the airport"
            );
            return false;
        }

        if le_bad {
            r.le_lat = lat;
            r.le_lon = lon;
        } else {
            r.he_lat = lat;
            r.he_lon = lon;
        }
        repaired += 1;
        tracing::debug!(
            ident = %r.airport_ident,
            runway = %r.le_ident,
            "runway threshold reconstructed from the opposite end"
        );
        true
    });
    tracing::debug!(repaired, dropped, "runway threshold repair pass");
}

/// The runway's heading, taken from its NAME ("24L" → 240°) — the one piece of
/// information a corrupt coordinate cannot have contaminated.
///
/// Deliberately NOT the stored `*_heading_true`: when the CSV omits a heading,
/// the parser fills it with the bearing computed *between the two thresholds*
/// (see the parse loop) — and on a row we are repairing, one of those two is the
/// corrupt one. Projecting along that bearing would faithfully reproduce the
/// corruption. Names like "H1", "ALL" or "N/A" yield `None`, and the row is then
/// dropped rather than guessed at.
fn heading_from_ident(ident: &str) -> Option<f64> {
    let digits: String = ident
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let n: f64 = digits.parse().ok()?;
    if (1.0..=36.0).contains(&n) {
        Some(n * 10.0)
    } else {
        None
    }
}

/// Point reached by travelling `distance_m` from (lat, lon) along `bearing_deg`.
fn project(lat: f64, lon: f64, bearing_deg: f64, distance_m: f64) -> (f64, f64) {
    let ang = distance_m / EARTH_RADIUS_M;
    let (br, p1, l1) = (
        bearing_deg.to_radians(),
        lat.to_radians(),
        lon.to_radians(),
    );
    let p2 = (p1.sin() * ang.cos() + p1.cos() * ang.sin() * br.cos()).asin();
    let l2 = l1
        + (br.sin() * ang.sin() * p1.cos()).atan2(ang.cos() - p1.sin() * p2.sin());
    (p2.to_degrees(), l2.to_degrees())
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
    let mut sum_lat = 0.0_f64;
    let mut sum_lon = 0.0_f64;
    let mut count = 0_u32;
    for row in rows_for_airport(icao) {
        sum_lat += (row.le_lat + row.he_lat) / 2.0;
        sum_lon += (row.le_lon + row.he_lon) / 2.0;
        count += 1;
    }
    if count == 0 {
        None
    } else {
        Some((sum_lat / count as f64, sum_lon / count as f64))
    }
}

/// Absolute longitude difference in degrees, wrapped across the antimeridian.
///
/// A naive `(a - b).abs()` makes 179.5°E and 179.5°W look 359° apart instead of
/// 1°, so any bounding-box filter using it silently drops everything on the
/// other side of the dateline. Divert searches in the Pacific (Aleutians, Fiji,
/// NZ) returned an empty list because of this.
fn lon_delta_deg(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// Distance in meters from a point to the *nearest runway threshold* of
/// the given airport — the one metric the whole app uses to answer "is
/// the aircraft on this field". Returns `None` when the ident isn't in
/// the embedded table.
///
/// Why not `airport_position()`: that returns the centroid of the runway
/// layout, which is not a point on the field in any useful sense at a
/// large airport. At EDDF the centroid is dragged ~1.5 nm south-west by
/// runway 18 (Startbahn West), so a stand at Terminal 2 measures 2.04 nm
/// from the centroid while sitting 0.30 nm off the 07C threshold. Feeding
/// centroid distance into an on-field radius while `find_nearest_airports`
/// feeds threshold distance into the *same* radius is what produced the
/// "landed at EDDF instead of planned EDDF" divert banner. Both probes now
/// answer with the same geometry, so they can no longer contradict each
/// other about the same airport.
///
/// This is deliberately the same `min(le, he)` per-runway measure that
/// `find_nearest_airports` uses — see the note there.
pub fn distance_to_airport_m(icao: &str, lat: f64, lon: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for row in rows_for_airport(icao) {
        let d = haversine_m(lat, lon, row.le_lat, row.le_lon)
            .min(haversine_m(lat, lon, row.he_lat, row.he_lon));
        best = Some(best.map_or(d, |b: f64| b.min(d)));
    }
    best
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
///
/// Includes national/local identifiers (`US-4991`, `48FA`). That is what the
/// runway-correlation paths want — a pilot who lands on a numbered FAA strip
/// landed *there*, and calling it by the ICAO field 20 nm away would be a lie.
/// Callers that will hand the answer to phpVMS as an arrival airport must use
/// [`find_nearest_icao_airports`] instead; see the note there.
pub fn find_nearest_airports(
    lat: f64,
    lon: f64,
    max_radius_m: f64,
    limit: usize,
) -> Vec<NearestAirport> {
    find_nearest(lat, lon, max_radius_m, limit, false)
}

/// Same, but only real ICAO airports.
///
/// This is the list a divert can be *named* from. The name goes into the banner,
/// into `flight_end(divert_to)`, and from there into phpVMS's `arr_airport_id` —
/// which cannot resolve "48FA". A pilot diverting to KLEE (Leesburg) would be
/// told he landed at 48FA, whose threshold sits 964 m from the apron.
///
/// v0.19.3 first put this filter inside `find_nearest_airports` itself, which
/// was the wrong layer: it also blinded `correlate_airport_icao` and
/// `resolve_touchdown_airport`, so a pilot landing ON a non-ICAO strip had his
/// touchdown attributed to an ICAO field up to 25 nm away. The constraint
/// belongs where the ICAO code is *used as an airport identity phpVMS must
/// accept*, not in the shared geometry primitive.
///
/// When the field a pilot actually used has no ICAO code, the honest answer is
/// "we don't know which field" — the divert banner then asks him to pick one.
pub fn find_nearest_icao_airports(
    lat: f64,
    lon: f64,
    max_radius_m: f64,
    limit: usize,
) -> Vec<NearestAirport> {
    find_nearest(lat, lon, max_radius_m, limit, true)
}

fn find_nearest(
    lat: f64,
    lon: f64,
    max_radius_m: f64,
    limit: usize,
    icao_only: bool,
) -> Vec<NearestAirport> {
    // QS round 8: without this, a NaN query made EVERY comparison below false —
    // so nothing was filtered out, all 14.7k rows came back with `distance_m =
    // NaN`, the sort degenerated into hash order, and the caller was handed five
    // arbitrary airports from anywhere on Earth as "the nearest fields". The
    // 50 Hz touchdown sampler is not behind the streamer's snapshot gate, so a
    // NaN sample really can reach here and put the wrong airport in a PIREP.
    if !lat.is_finite() || !lon.is_finite() {
        return Vec::new();
    }
    use std::collections::HashMap;
    let table = runways();
    let mut by_apt: HashMap<&str, (f64, f64, f64, f32)> = HashMap::new();

    // Coarse bounding-box pre-filter so we don't haversine the entire world
    // catalog. Latitude is easy: 1° ≈ 111 km everywhere.
    let lat_span_deg = (max_radius_m / 111_000.0).max(0.5);
    // Longitude is NOT. A degree of longitude shrinks with cos(latitude), so a
    // box that is `lat_span_deg` wide in longitude covers less and less ground
    // the further north you go.
    //
    // v0.19.3: this used the same span for both axes, which quietly truncated
    // every search away from the equator — a nominal 50 nm divert search
    // reached only ~36 nm east/west at Frankfurt (50°N) and ~24 nm at
    // Reykjavík (64°N). The pilot's manual divert list was simply missing
    // fields. Scale by 1/cos(lat), clamped for the poles where cos(lat) → 0
    // and the correction blows up (there, just take the whole longitude band —
    // there is nothing to filter out that far north anyway).
    let cos_lat = lat.to_radians().cos().abs().max(0.01);
    let lon_span_deg = (lat_span_deg / cos_lat).min(180.0);

    for row in table.iter() {
        // Only real ICAO idents when the caller will use the answer as an
        // airport identity phpVMS has to accept — see `find_nearest_icao_airports`.
        if icao_only && !looks_like_icao(&row.airport_ident) {
            continue;
        }
        let approx_lat = (row.le_lat + row.he_lat) / 2.0;
        let approx_lon = (row.le_lon + row.he_lon) / 2.0;
        if (approx_lat - lat).abs() > lat_span_deg
            || lon_delta_deg(approx_lon, lon) > lon_span_deg
        {
            continue;
        }
        // Use the closer of the two threshold positions for each runway as that
        // runway's distance to the query. The pilot touched down somewhere on
        // the field — the nearer threshold is the better proxy than the
        // centroid.
        let d_le = haversine_m(lat, lon, row.le_lat, row.le_lon);
        let d_he = haversine_m(lat, lon, row.he_lat, row.he_lon);
        // The point the distance actually refers to. `NearestAirport.lat/lon`
        // reports THIS, so that the coordinates and the distance next to them
        // describe the same place — they used to be the runway midpoint while
        // the distance was to the threshold, ~2 km apart at EDDF (harmless
        // while nothing plotted the pin; a bug waiting for the first map that
        // does).
        let (d, near_lat, near_lon) = if d_le <= d_he {
            (d_le, row.le_lat, row.le_lon)
        } else {
            (d_he, row.he_lat, row.he_lon)
        };
        if d > max_radius_m {
            continue;
        }
        let entry = by_apt
            .entry(&row.airport_ident)
            .or_insert((near_lat, near_lon, d, 0.0));
        if d < entry.2 {
            entry.0 = near_lat;
            entry.1 = near_lon;
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
mod geo_search_tests {
    use super::*;

    /// KCLE's 06R threshold is stored 4 nm off the field; its 24L end is fine.
    /// The repair must keep the runway usable (a pilot landing on 24L still
    /// gets a match) AND stop the bad coordinate from dragging the airport's
    /// geometry to a phantom location.
    #[test]
    fn a_truncated_threshold_is_reconstructed_not_thrown_away() {
        // The airport must still know its 06R/24L runway.
        let rows: Vec<_> = rows_for_airport("KCLE")
            .filter(|r| r.le_ident == "06R" || r.he_ident == "24L")
            .collect();
        assert!(
            !rows.is_empty(),
            "KCLE 06R/24L must survive — dropping the row costs a real landing its runway match"
        );

        // And every one of its thresholds must now sit ON the field. KCLE's
        // reference point is 41.4117/-81.8498.
        for r in rows {
            for (lat, lon, end) in [
                (r.le_lat, r.le_lon, "06R"),
                (r.he_lat, r.he_lon, "24L"),
            ] {
                let off_nm = haversine_m(lat, lon, 41.4117, -81.8498) / 1852.0;
                assert!(
                    off_nm < 2.0,
                    "KCLE {end} is {off_nm:.2} nm from the airport — corrupt coordinate not repaired"
                );
            }
        }

        // The bad coordinate must no longer make a point 4 nm off the field
        // look like "at KCLE".
        let phantom_nm = distance_to_airport_m("KCLE", 41.300, -81.800)
            .expect("KCLE has geometry")
            / 1852.0;
        assert!(
            phantom_nm > 2.0,
            "the phantom threshold still makes a point {phantom_nm:.2} nm off the field read as on-field"
        );
    }

    /// The one that got away in QA round 3, and the reason this test exists.
    ///
    /// 74.7 % of airports have a SINGLE runway — two thresholds, no independent
    /// reference. An earlier cut of the repair pass took their "median", which
    /// per axis is just the larger of the two coordinates: a coin flip about
    /// which end is the truth. At WAJI (Mararena Sarmi) it lost that flip,
    /// declared the GOOD threshold an outlier and projected it next to the
    /// corrupt one — moving a working airport 5.2 nm from where it is. Every
    /// pilot flying there would then have been told he had diverted, at his own
    /// destination, and auto-start would have refused to fire from its apron.
    ///
    /// Where we cannot tell which end is wrong, we must not guess.
    #[test]
    fn a_single_runway_airport_is_never_guessed_at() {
        // WAJI's real reference point: -1.873077 / 138.749002.
        const WAJI: (f64, f64) = (-1.873077, 138.749002);

        for r in rows_for_airport("WAJI") {
            for (lat, lon) in [(r.le_lat, r.le_lon), (r.he_lat, r.he_lon)] {
                let off_nm = haversine_m(lat, lon, WAJI.0, WAJI.1) / 1852.0;
                assert!(
                    off_nm < 2.0,
                    "WAJI threshold sits {off_nm:.2} nm from the airport — the repair \
                     pass invented a position instead of dropping the row"
                );
            }
        }

        // Whatever we kept must not place the airport somewhere it isn't: an
        // aircraft parked ON WAJI has to read as being on WAJI (or the geometry
        // has to be absent, so the phpVMS reference point takes over).
        match distance_to_airport_m("WAJI", WAJI.0, WAJI.1) {
            Some(m) => assert!(
                m / 1852.0 <= 2.0,
                "an aircraft parked at WAJI reads {:.2} nm away — false divert at its own \
                 destination",
                m / 1852.0
            ),
            None => { /* geometry dropped — the reference-point fallback places it */ }
        }
    }

    /// A runway row that is internally consistent but sits in another country
    /// (UUMU has one in Belgorod, 319 nm away) is a fabrication — there is
    /// nothing to reconstruct from, so it must be discarded outright.
    #[test]
    fn a_wholly_misplaced_runway_is_discarded() {
        // UUMU (Chkalovsky) is at 55.89/38.04. The CSV carries a second runway
        // row at 50.65/36.58 — Belgorod, 319 nm south — internally consistent
        // and therefore invisible to any per-row plausibility check.
        let rows: Vec<_> = rows_for_airport("UUMU").collect();
        assert!(!rows.is_empty(), "UUMU must keep its real runway");
        for r in rows {
            let off_nm = haversine_m(r.le_lat, r.le_lon, 55.8898, 38.0435) / 1852.0;
            assert!(
                off_nm < 5.0,
                "a UUMU runway is still {off_nm:.0} nm from the airport — misplaced row not discarded"
            );
        }
        // And the phantom must no longer answer "yes, you're at UUMU" for an
        // aircraft parked in Belgorod.
        let belgorod_nm = distance_to_airport_m("UUMU", 50.6485, 36.5757)
            .expect("UUMU has geometry")
            / 1852.0;
        assert!(
            belgorod_nm > 100.0,
            "Belgorod still reads as {belgorod_nm:.0} nm from UUMU"
        );
    }

    /// Sometimes it is the REFERENCE POINT that is wrong, not the runway — and
    /// then throwing the runway away is the mistake.
    ///
    /// OurAirports puts FAHS's reference point 2,446 nm from the airport, while
    /// its two runways are correct (Navigraph, the authoritative source Thomas
    /// re-uploads every AIRAC cycle, agrees with the runways to within 1 nm).
    /// A rule that judged runways purely by their distance from the reference
    /// point would have discarded both.
    ///
    /// The tie-breaker is corroboration: runways that agree with each other
    /// outvote a lone reference point.
    #[test]
    fn a_wrong_reference_point_does_not_cost_an_airport_its_runways() {
        let n = rows_for_airport("FAHS").count();
        assert!(
            n >= 2,
            "FAHS must keep its runways — the reference point is the thing that is \
             wrong there, and the runways corroborate each other (kept: {n})"
        );
    }

    /// "Is the aircraft standing on some OTHER airport?" — the question that
    /// decides whether a pilot may confirm his planned destination as his actual
    /// landing site (see `standing_on_another_field` in lib.rs).
    ///
    /// It has to say YES at a neighbouring airport, and NO in a field. The first
    /// cut counted every ident in the table — including 23,116 heliports and
    /// 13,332 CLOSED fields — within 3 nm, which answers "yes" for 59 % of
    /// plausible off-field spots around a major airport. That would have blocked
    /// the honest pilot this path exists to serve.
    #[test]
    fn standing_on_a_neighbouring_airport_is_recognised() {
        // Parked at LFPB (Le Bourget) on a flight planned to LFPG. 5.4 nm apart.
        let lfpb = (48.9694, 2.4414);
        let hit = nearest_airport_reference(lfpb.0, lfpb.1, 1.0, "LFPG");
        assert_eq!(
            hit.as_ref().map(|(i, _)| i.as_str()),
            Some("LFPB"),
            "an aircraft parked at Le Bourget is standing on Le Bourget"
        );
    }

    #[test]
    fn a_field_short_of_the_destination_is_not_another_airport() {
        // ~6 nm north-east of EDDF, off-airport (the Frankfurt city forest).
        let off_field = (50.1100, 8.6600);
        let hit = nearest_airport_reference(off_field.0, off_field.1, 1.0, "EDDF");
        assert!(
            hit.is_none(),
            "an off-field landing near the destination must not read as 'standing \
             on another airport' (got {hit:?})"
        );
    }

    /// Closed fields and helipads are not places an AEROPLANE comes to rest —
    /// but they stay in the table, because they still have reference points.
    #[test]
    fn heliports_and_closed_fields_are_not_places_an_aeroplane_parks() {
        let idx = airports_by_ident();
        let not_landable = idx.values().filter(|a| !a.landable).count();
        assert!(
            not_landable > 30_000,
            "heliports (23k) and closed fields (13k) must not count as somewhere an \
             aeroplane could be standing: {not_landable}"
        );
        // …and they are still reachable as reference points.
        assert!(airport_reference("EDDF").is_some());
    }

    #[test]
    fn longitude_delta_wraps_the_antimeridian() {
        assert!((lon_delta_deg(179.5, -179.5) - 1.0).abs() < 1e-9);
        assert!((lon_delta_deg(-179.5, 179.5) - 1.0).abs() < 1e-9);
        assert!((lon_delta_deg(10.0, 8.0) - 2.0).abs() < 1e-9);
        assert!((lon_delta_deg(-170.0, 170.0) - 20.0).abs() < 1e-9);
    }

    /// The search box must not shrink east-west as you go north. At Frankfurt
    /// (50°N) an un-corrected box covered only ~36 nm of a nominal 50 nm
    /// search, so the divert picker was silently missing fields.
    #[test]
    fn the_search_radius_holds_up_at_northern_latitudes() {
        // EDDF (50.03N, 8.57E). EDRK (Koblenz-Winningen) is 43.9 nm away —
        // comfortably inside a 50 nm search — but 1.05° of longitude west,
        // which the old un-scaled bounding box (0.926°) cut off. At 50°N that
        // box only reached ~36 nm east-west of a nominal 50 nm search.
        let found = find_nearest_airports(50.0333, 8.5706, 50.0 * 1852.0, 60);
        let idents: Vec<&str> = found.iter().map(|a| a.icao.as_str()).collect();
        assert!(
            idents.contains(&"EDRK"),
            "a 50 nm search from EDDF must reach EDRK (43.9 nm west) — the \
             longitude box has to scale with 1/cos(lat) (found: {idents:?})"
        );
    }

    /// A search right on the dateline must see both sides of it.
    #[test]
    fn a_search_on_the_dateline_sees_both_sides() {
        // NFFN (Nadi, Fiji) sits at ~177.4E. Query from just EAST of the
        // antimeridian (i.e. negative longitude, ~179.9W): Nadi is ~150 nm
        // away in reality, so a generous search must still find *something*
        // west of the line rather than returning an empty list.
        let near_line = find_nearest_airports(-17.75, -179.9, 200.0 * 1852.0, 10);
        assert!(
            near_line.iter().any(|a| a.lon > 170.0),
            "a search just east of the dateline must reach airports west of it \
             (got: {:?})",
            near_line.iter().map(|a| (&a.icao, a.lon)).collect::<Vec<_>>()
        );
    }

    /// `NearestAirport.lat/lon` must describe the same point `distance_m`
    /// measures to — otherwise anything that plots the pin lands ~2 km off.
    #[test]
    fn the_reported_position_is_the_point_the_distance_refers_to() {
        let from = (50.0500, 8.5860); // EDDF Terminal 2
        let eddf = find_nearest_airports(from.0, from.1, 5.0 * 1852.0, 5)
            .into_iter()
            .find(|a| a.icao == "EDDF")
            .expect("EDDF found");
        let recomputed = haversine_m(from.0, from.1, eddf.lat, eddf.lon);
        assert!(
            (recomputed - eddf.distance_m).abs() < 1.0,
            "distance_m ({:.0} m) must be the distance to the reported lat/lon \
             ({:.0} m)",
            eddf.distance_m,
            recomputed
        );
    }
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
