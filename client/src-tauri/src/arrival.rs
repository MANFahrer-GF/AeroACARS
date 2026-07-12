//! Arrival site — the single authority on the question "which airport is
//! this aircraft at, and is it the one it was supposed to fly to?".
//!
//! # Why this module exists
//!
//! That question used to be answered ad-hoc at each call site, with each
//! site free to pick its own geometry and its own guards. Three notions of
//! "where an airport is" were in simultaneous use:
//!
//!   * the centroid of the runway layout (`runway::airport_position`),
//!   * the nearest runway threshold (`runway::find_nearest_airports`),
//!   * the airport coordinates phpVMS ships with the bid.
//!
//! They disagree by more than a kilometre at a big field — comparable to the
//! 2 nm radius they were all being compared against. The arrived-fallback
//! managed to use *two of them at once*: "am I near the planned airport?" was
//! answered with the centroid, "which field am I standing on?" with the
//! nearest threshold. At EDDF a stand at Terminal 2 is 2.04 nm from the
//! centroid (→ "not at the planned airport") and 0.30 nm off the 07C
//! threshold (→ "standing at EDDF"). Both statements at once produce the
//! banner a pilot actually saw in v0.19.2:
//!
//!     "Anderer Landeplatz erkannt — Du bist gelandet in EDDF statt
//!      geplant EDDF (~2 nmi vom Ziel entfernt)."
//!
//! The divert-prefetch path had a `nearest == planned → not a divert` guard
//! and was therefore immune; the detection path did not, and was not. That
//! asymmetry is the actual defect: a rule that every call site has to
//! remember to re-implement is a rule that will be forgotten.
//!
//! # The contract
//!
//! One function ([`locate`]) answers the question, with one metric (distance
//! to the nearest runway threshold, [`runway::distance_to_airport_m`]) and one
//! radius ([`ON_FIELD_RADIUS_NM`]). It returns a [`ArrivalSite`] whose variants
//! are mutually exclusive by construction, so "at the planned airport" and "at
//! some other airport" can no longer both be true.
//!
//! A [`DivertHint`] can only be built from an `ArrivalSite` ([`DivertHint::from_site`]),
//! and the struct carries a private field so no other module can construct one
//! by hand. A hint that names the planned airport as the divert target is thus
//! not a bug to be guarded against — it is unrepresentable.

use serde::Serialize;

use crate::runway;

/// How close to an airport's nearest runway threshold an aircraft has to be
/// for us to say it is *on that field*. One radius, used by every consumer —
/// previously this was duplicated as `ARRIVED_FALLBACK_RADIUS_NM` and
/// `DIVERT_DETECT_RADIUS_NM` (both 2.0, with a doc comment on the latter
/// promising they'd stay in sync — nothing enforced it).
///
/// 2 nm covers the stand areas of the biggest fields (EDDF's most remote
/// apron is ~1.1 nm from the nearest threshold, KJFK's ~1.3 nm) with room to
/// spare, while staying far below the distance to any *neighbouring* field.
pub const ON_FIELD_RADIUS_NM: f64 = 2.0;

/// How far out we look for the field an aircraft actually ended up on, when
/// it is demonstrably not on the planned one. Real-world diverts land 20-100
/// nm out; 50 nm covers the sane cases without dragging in half a continent.
pub const NEAREST_SEARCH_RADIUS_NM: f64 = 50.0;

/// Where an aircraft is, relative to the airport it was supposed to fly to.
///
/// The variants are exhaustive and mutually exclusive: exactly one holds for
/// a given position. This is the whole point of the type — the previous code
/// carried "near the planned field?" and "which field is nearest?" as two
/// independent values and could therefore hold two contradictory beliefs at
/// the same time.
#[derive(Debug, Clone, PartialEq)]
pub enum ArrivalSite {
    /// On the planned field. `distance_nm` is `None` when the planned ICAO
    /// isn't in the runways table at all (obscure strip, scenery-only field):
    /// we cannot measure, so we give the pilot the benefit of the doubt and
    /// treat it as an arrival rather than inventing a divert. That matches
    /// the old fallback's `arr_pos.is_none() ⇒ near_planned` behaviour.
    AtPlanned { distance_nm: Option<f64> },
    /// On a *different* field than planned — a real divert. `icao` is never
    /// equal to the planned ICAO; [`locate`] cannot produce such a value.
    AtOtherAirport {
        icao: String,
        distance_from_planned_nm: f64,
    },
    /// Not on any field we know: too far from the planned airport, and no
    /// other airport's threshold within [`ON_FIELD_RADIUS_NM`]. An off-field
    /// landing, or a field the runways table doesn't have.
    OffAirport { distance_from_planned_nm: f64 },
}

impl ArrivalSite {
    /// True when the aircraft is on the planned field. The *only* way to ask
    /// that question — no caller re-derives it from a distance.
    pub fn is_at_planned(&self) -> bool {
        matches!(self, ArrivalSite::AtPlanned { .. })
    }

    /// Distance from the planned airport in nm, when measurable.
    pub fn distance_from_planned_nm(&self) -> Option<f64> {
        match self {
            ArrivalSite::AtPlanned { distance_nm } => *distance_nm,
            ArrivalSite::AtOtherAirport {
                distance_from_planned_nm,
                ..
            }
            | ArrivalSite::OffAirport {
                distance_from_planned_nm,
            } => Some(*distance_from_planned_nm),
        }
    }
}

/// Determine where the aircraft is relative to its planned destination.
///
/// Both probes — "how far from the planned field" and "which field am I on" —
/// use distance to the nearest runway threshold, so they answer in the same
/// units of the same geometry and cannot disagree about the same airport.
pub fn locate(planned_arr_icao: &str, lat: f64, lon: f64) -> ArrivalSite {
    let planned = planned_arr_icao.trim();
    let Some(dist_planned_nm) =
        runway::distance_to_airport_m(planned, lat, lon).map(|m| m / 1852.0)
    else {
        // Planned field not in the table — unmeasurable, so not a divert.
        return ArrivalSite::AtPlanned { distance_nm: None };
    };

    if dist_planned_nm <= ON_FIELD_RADIUS_NM {
        return ArrivalSite::AtPlanned {
            distance_nm: Some(dist_planned_nm),
        };
    }

    // Off the planned field. Which field, if any, are we on instead?
    let nearest = runway::find_nearest_airports(lat, lon, NEAREST_SEARCH_RADIUS_NM * 1852.0, 1)
        .into_iter()
        .next()
        .filter(|na| na.distance_m / 1852.0 <= ON_FIELD_RADIUS_NM);

    match nearest {
        // Same metric as the planned probe above, so this branch is
        // unreachable in practice (the planned field would have had to be
        // both farther and nearer than the radius). Kept as an explicit,
        // total match rather than an `unwrap` on that reasoning: if the two
        // ever drift apart again, the answer is "we are at the planned
        // field", not "we diverted to where we planned to go".
        Some(na) if na.icao.eq_ignore_ascii_case(planned) => ArrivalSite::AtPlanned {
            distance_nm: Some(dist_planned_nm),
        },
        Some(na) => ArrivalSite::AtOtherAirport {
            icao: na.icao,
            distance_from_planned_nm: dist_planned_nm,
        },
        None => ArrivalSite::OffAirport {
            distance_from_planned_nm: dist_planned_nm,
        },
    }
}

/// Private witness that a `DivertHint` came out of [`DivertHint::from_site`].
/// Its only job is to make `DivertHint { .. }` un-writable outside this
/// module — the invariant "a divert never names the planned airport" is
/// enforced by the compiler, not by every author remembering to check.
#[derive(Debug, Clone, Copy)]
struct Sealed;

/// A detected divert, surfaced via `flight_status` so the cockpit can ask the
/// pilot to confirm the real destination.
///
/// Invariant: `actual_icao != planned_arr_icao`. Guaranteed by construction —
/// see [`DivertHint::from_site`] and `Sealed`.
///
/// This is a *suspicion*, not a filed fact. Nothing may report it to the
/// outside world as a divert that happened; see `divert_payload_markers`.
#[derive(Debug, Clone, Serialize)]
pub struct DivertHint {
    /// Best-guess actual landing airport. `None` when the aircraft is off
    /// any known field (private strip, off-DB military, scenery-only) — the
    /// pilot then picks the field by hand.
    pub actual_icao: Option<String>,
    /// What the bid had as the planned destination.
    pub planned_arr_icao: String,
    /// What the bid had as the planned alternate, if any. When the actual
    /// field is the planned alternate we can say "diverted to your alternate"
    /// with high confidence.
    pub planned_alt_icao: Option<String>,
    /// Distance from the aircraft to the planned arrival, in nautical miles.
    pub distance_to_planned_nmi: f64,
    /// "alternate" (it's the filed alternate), "nearest" (closest field in
    /// the DB), or "unknown" (no field found — manual override needed).
    pub kind: &'static str,
    #[serde(skip)]
    _sealed: Sealed,
}

impl DivertHint {
    /// The only way to build a `DivertHint`. Returns `None` for an
    /// [`ArrivalSite::AtPlanned`] — an aircraft on its planned field has not
    /// diverted, whatever any distance figure might suggest.
    pub fn from_site(
        site: &ArrivalSite,
        planned_arr_icao: &str,
        planned_alt_icao: Option<&str>,
    ) -> Option<DivertHint> {
        let (actual_icao, distance_to_planned_nmi) = match site {
            ArrivalSite::AtPlanned { .. } => return None,
            ArrivalSite::AtOtherAirport {
                icao,
                distance_from_planned_nm,
            } => (Some(icao.clone()), *distance_from_planned_nm),
            ArrivalSite::OffAirport {
                distance_from_planned_nm,
            } => (None, *distance_from_planned_nm),
        };

        let alt_match = actual_icao
            .as_deref()
            .zip(planned_alt_icao)
            .map(|(a, b)| a.eq_ignore_ascii_case(b.trim()))
            .unwrap_or(false);
        let kind = if alt_match {
            "alternate"
        } else if actual_icao.is_some() {
            "nearest"
        } else {
            "unknown"
        };

        Some(DivertHint {
            actual_icao,
            planned_arr_icao: planned_arr_icao.to_string(),
            planned_alt_icao: planned_alt_icao.map(|s| s.to_string()),
            distance_to_planned_nmi,
            kind,
            _sealed: Sealed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stand at EDDF Terminal 2 — the exact geometry from the field report
    /// (pilot parked at EDDF after a planned EDDF arrival, got told he had
    /// diverted to EDDF). 2.04 nm from the runway-layout centroid, 0.30 nm
    /// from the 07C threshold. The centroid metric said "not at the planned
    /// airport"; the only correct answer is that he is standing at EDDF.
    const EDDF_TERMINAL_2: (f64, f64) = (50.0500, 8.5860);

    /// Pins the METRIC, not just the outcome.
    ///
    /// The two defences in this module are independent: the geometry (measure
    /// against the nearest threshold) and the invariant (a divert can't name
    /// the planned field). The invariant alone is enough to keep the EDDF
    /// banner from ever appearing again — which means an outcome-only test
    /// passes even with the broken centroid metric restored, and would let it
    /// creep back. It must not creep back: the centroid is still wrong for the
    /// distance we *report* to the pilot ("~2 nmi vom Ziel entfernt" while
    /// standing on the field), and at a field with a neighbouring airstrip
    /// inside 2 nm it would pick the wrong airport outright.
    ///
    /// So: assert the numbers directly. If someone swaps the metric back, this
    /// fails, whatever the invariant says.
    #[test]
    fn the_on_field_probe_measures_thresholds_not_the_runway_centroid() {
        let (lat, lon) = EDDF_TERMINAL_2;

        let threshold_nm = runway::distance_to_airport_m("EDDF", lat, lon)
            .expect("EDDF is in the table")
            / 1852.0;
        let (c_lat, c_lon) = runway::airport_position("EDDF").expect("EDDF centroid");
        let centroid_nm = runway::distance_m(lat, lon, c_lat, c_lon) / 1852.0;

        assert!(
            threshold_nm <= ON_FIELD_RADIUS_NM,
            "a T2 stand is on the field: {threshold_nm:.2} nm from the nearest threshold"
        );
        assert!(
            centroid_nm > ON_FIELD_RADIUS_NM,
            "the centroid metric, which we must NOT use, puts that same stand \
             {centroid_nm:.2} nm away — outside the {ON_FIELD_RADIUS_NM} nm radius. \
             That contradiction is the whole bug."
        );

        // And the distance we report to the pilot is the honest one.
        let site = locate("EDDF", lat, lon);
        assert_eq!(
            site.distance_from_planned_nm().map(|d| d <= ON_FIELD_RADIUS_NM),
            Some(true),
            "the reported distance must be the on-field one, not the centroid's"
        );
    }

    #[test]
    fn eddf_terminal_2_stand_is_at_the_planned_airport() {
        let site = locate("EDDF", EDDF_TERMINAL_2.0, EDDF_TERMINAL_2.1);
        assert!(
            site.is_at_planned(),
            "a stand at EDDF T2 must be AtPlanned for a planned EDDF arrival, got {site:?}"
        );
        assert!(DivertHint::from_site(&site, "EDDF", None).is_none());
    }

    /// The regression the whole module exists for: the banner that told a
    /// pilot he had landed "in EDDF instead of planned EDDF".
    #[test]
    fn a_divert_can_never_name_the_planned_airport() {
        // Every position on or around the planned field, at every distance
        // the old code would have called "far": none may yield a hint whose
        // target is the planned field itself.
        for (lat, lon) in [
            EDDF_TERMINAL_2,
            (50.0333, 8.5706), // ARP
            (50.0264, 8.5431), // 18/36 threshold area, far corner of the field
            (50.0379, 8.5622), // centroid-ish
        ] {
            let site = locate("EDDF", lat, lon);
            if let Some(hint) = DivertHint::from_site(&site, "EDDF", None) {
                assert_ne!(
                    hint.actual_icao.as_deref(),
                    Some("EDDF"),
                    "hint at {lat},{lon} names the planned field as the divert target"
                );
            }
        }
    }

    #[test]
    fn a_real_divert_is_still_detected() {
        // Parked at EDDP (Leipzig) on a flight planned to EDDF.
        let site = locate("EDDF", 51.4239, 12.2364);
        let ArrivalSite::AtOtherAirport { ref icao, .. } = site else {
            panic!("EDDP parking on an EDDF flight must be AtOtherAirport, got {site:?}");
        };
        assert_eq!(icao, "EDDP");

        let hint = DivertHint::from_site(&site, "EDDF", None).expect("real divert yields a hint");
        assert_eq!(hint.actual_icao.as_deref(), Some("EDDP"));
        assert_eq!(hint.kind, "nearest");
        assert!(hint.distance_to_planned_nmi > 100.0);
    }

    #[test]
    fn diverting_to_the_filed_alternate_is_labelled_as_such() {
        let site = locate("EDDF", 51.4239, 12.2364); // EDDP
        let hint = DivertHint::from_site(&site, "EDDF", Some("EDDP")).expect("hint");
        assert_eq!(hint.kind, "alternate");
    }

    #[test]
    fn an_off_field_landing_yields_a_targetless_hint() {
        // Somewhere in the North Sea — no runway threshold within 2 nm.
        let site = locate("EDDF", 55.5000, 3.5000);
        assert!(matches!(site, ArrivalSite::OffAirport { .. }), "{site:?}");
        let hint = DivertHint::from_site(&site, "EDDF", None).expect("hint");
        assert!(hint.actual_icao.is_none());
        assert_eq!(hint.kind, "unknown");
    }

    /// Replay of the real flight corpus from the live recorder — every flight
    /// AeroACARS has ever recorded, evaluated at its actual final parked
    /// position against its actual planned destination.
    ///
    /// Fixtures prove the cases we thought of. This proves the cases pilots
    /// actually flew, and it is what tells us the EDDF geometry bug was one
    /// airport's quirk or a trap waiting at others.
    ///
    /// # Ground truth is deliberately NOT our own geometry
    ///
    /// Grading `locate()` by asking `locate()` where the aircraft is would be
    /// circular. The corpus therefore carries `planned_arr_lat/lon` — the
    /// planned airport's reference point from the recorder's `airports` table,
    /// an entirely different data source from the embedded OurAirports runway
    /// table the client measures against. The verdicts:
    ///
    ///   * within `TRUTH_AT_AIRPORT_NM` of the planned ARP → the aircraft was
    ///     unambiguously parked at its destination. A divert hint here is the
    ///     EDDF bug. Hard failure.
    ///   * beyond `TRUTH_ELSEWHERE_NM` → the aircraft was unambiguously NOT at
    ///     its destination. No hint here means the detection went blind. Hard
    ///     failure.
    ///   * in between → an apron on a sprawling field, or a genuinely marginal
    ///     case. Reported, not asserted; a test that fails on ambiguity teaches
    ///     people to ignore it.
    ///
    /// Note that "the PIREP was filed as planned" is NOT ground truth for "the
    /// aircraft was at the planned field" — a first cut of this test assumed it
    /// was and reported 7 false positives that turned out to be real. GSG 0
    /// (7op4EybywvaWVnLr) filed as planned EDHI while parked 0.66 nm from EDHL:
    /// the pilot landed at Lübeck and filed for Finkenwerder anyway. Raising a
    /// divert hint there is correct behaviour, not a bug.
    ///
    /// Not run in CI — it needs the exported corpus:
    ///
    ///     AEROACARS_CORPUS=/root/Claude/aeroacars-src/corpus-arrivals.csv \
    ///       cargo test --lib corpus -- --ignored --nocapture
    #[test]
    #[ignore = "needs the exported flight corpus (AEROACARS_CORPUS)"]
    fn corpus_geometry_matches_independent_ground_truth() {
        /// Within this of the planned airport's published reference point, the
        /// aircraft is at its destination — no argument. Generous enough to
        /// cover the remotest apron of the biggest field.
        const TRUTH_AT_AIRPORT_NM: f64 = 3.0;
        /// Beyond this, it is somewhere else entirely — no argument.
        const TRUTH_ELSEWHERE_NM: f64 = 10.0;

        let path = std::env::var("AEROACARS_CORPUS")
            .expect("set AEROACARS_CORPUS to the exported corpus CSV");
        let csv = std::fs::read_to_string(&path).expect("read corpus");

        let header: Vec<&str> = csv.lines().next().expect("header").split(',').collect();
        let col = |name: &str| -> usize {
            header
                .iter()
                .position(|h| h.trim() == name)
                .unwrap_or_else(|| panic!("corpus is missing the `{name}` column"))
        };
        let (c_pirep, c_flight, c_planned) = (col("pirep_id"), col("flight_number"), col("planned_arr_icao"));
        let (c_plat, c_plon) = (col("planned_arr_lat"), col("planned_arr_lon"));
        let (c_flat, c_flon) = (col("final_lat"), col("final_lon"));

        let mut checked = 0_u32;
        let mut at_airport = 0_u32;
        let mut elsewhere = 0_u32;
        let mut ambiguous: Vec<String> = Vec::new();
        let mut false_positives: Vec<String> = Vec::new();
        let mut missed: Vec<String> = Vec::new();

        for line in csv.lines().skip(1).filter(|l| !l.trim().is_empty()) {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() <= c_flon {
                continue;
            }
            let (pirep, flight_no, planned) =
                (f[c_pirep].trim(), f[c_flight].trim(), f[c_planned].trim());
            let (Ok(lat), Ok(lon)) = (
                f[c_flat].trim().parse::<f64>(),
                f[c_flon].trim().parse::<f64>(),
            ) else {
                continue;
            };
            let (Ok(plat), Ok(plon)) = (
                f[c_plat].trim().parse::<f64>(),
                f[c_plon].trim().parse::<f64>(),
            ) else {
                continue; // no independent truth for this airport → cannot grade
            };
            // Null Island and other junk fixes: no position, nothing to grade.
            if planned.is_empty() || (lat == 0.0 && lon == 0.0) || (plat == 0.0 && plon == 0.0) {
                continue;
            }
            checked += 1;

            let truth_nm = runway::distance_m(lat, lon, plat, plon) / 1852.0;
            let site = locate(planned, lat, lon);
            let hint = DivertHint::from_site(&site, planned, None);

            let describe = |h: &DivertHint| {
                format!(
                    "{flight_no} ({pirep}): planned {planned}, parked {truth_nm:.2} nm from its \
                     reference point → hint says {} ({:.2} nm)",
                    h.actual_icao.as_deref().unwrap_or("(off-field)"),
                    h.distance_to_planned_nmi
                )
            };

            if truth_nm <= TRUTH_AT_AIRPORT_NM {
                at_airport += 1;
                if let Some(h) = &hint {
                    false_positives.push(describe(h));
                }
            } else if truth_nm >= TRUTH_ELSEWHERE_NM {
                elsewhere += 1;
                if hint.is_none() {
                    missed.push(format!(
                        "{flight_no} ({pirep}): planned {planned}, parked {truth_nm:.2} nm away — \
                         detection stayed silent"
                    ));
                }
            } else {
                ambiguous.push(format!(
                    "{flight_no} ({pirep}): planned {planned}, {truth_nm:.2} nm from reference \
                     point, hint={}",
                    hint.as_ref()
                        .map(|h| h.actual_icao.as_deref().unwrap_or("(off-field)"))
                        .unwrap_or("none")
                ));
            }
        }

        println!("corpus: {checked} flights graded against independent airport coordinates");
        println!("  parked AT the planned airport (≤{TRUTH_AT_AIRPORT_NM} nm): {at_airport}");
        println!("  parked ELSEWHERE (≥{TRUTH_ELSEWHERE_NM} nm)            : {elsewhere}");
        println!("  ambiguous band (reported, not asserted)      : {}", ambiguous.len());
        for e in &ambiguous {
            println!("    ~ {e}");
        }

        assert!(checked > 100, "corpus looks too small ({checked} rows) — bad export?");
        assert!(
            false_positives.is_empty(),
            "an aircraft parked at its planned airport must NEVER be told it diverted. \
             {} false positive(s):\n  {}",
            false_positives.len(),
            false_positives.join("\n  ")
        );
        assert!(
            missed.is_empty(),
            "an aircraft parked far from its planned airport MUST be offered the divert. \
             {} missed:\n  {}",
            missed.len(),
            missed.join("\n  ")
        );
    }

    #[test]
    fn an_unmeasurable_planned_field_is_never_a_divert() {
        // ICAO not in the runways table: we cannot measure, so we must not
        // accuse the pilot of diverting. (Old fallback did the same via
        // `arr_pos.is_none() ⇒ near_planned`.)
        let site = locate("ZZZZ", 50.0500, 8.5860);
        assert_eq!(site, ArrivalSite::AtPlanned { distance_nm: None });
        assert!(DivertHint::from_site(&site, "ZZZZ", None).is_none());
    }
}
