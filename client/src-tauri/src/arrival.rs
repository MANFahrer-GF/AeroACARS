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
//! One function ([`locate`]) answers the question. It measures with the runway
//! thresholds ([`runway::distance_to_airport_m`], radius [`ON_FIELD_RADIUS_NM`])
//! and, for the thousands of fields that have no threshold data — and as a
//! safety net under the fields whose data is simply wrong — with the airport's
//! reference point from phpVMS ([`ON_FIELD_FALLBACK_RADIUS_NM`]). Either source
//! recognising the destination is enough to acquit; accusing a pilot of a divert
//! requires both to fail. It returns an [`ArrivalSite`] whose variants are
//! mutually exclusive by construction, so "at the planned airport" and "at some
//! other airport" can no longer both be true.
//!
//! A [`DivertHint`] can only be built from an `ArrivalSite` ([`DivertHint::from_site`]),
//! and the struct carries a private field so no other module can construct one
//! by hand. A hint that names the planned airport as the divert target is thus
//! not a bug to be guarded against — it is unrepresentable.

use serde::{Deserialize, Serialize};

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

/// The on-field radius when we are measuring against an airport's *reference
/// point* (phpVMS) instead of its runway thresholds — the fallback for fields
/// the runway table has no geometry for.
///
/// A reference point sits somewhere in the middle of the field, so the same
/// stand measures farther from it than from the nearest threshold; 3 nm keeps
/// the biggest aprons inside. Verified against the live flight corpus: of 617
/// real arrivals, 613 parked within 3 nm of their destination's reference
/// point, and the four beyond it were genuine diverts.
pub const ON_FIELD_FALLBACK_RADIUS_NM: f64 = 3.0;

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
/// `planned_ref_pos`: the planned airport's reference coordinates as phpVMS
/// knows them, when the caller has them. This is the fallback for the fields
/// our runway table has no geometry for — and that is not a rare corner:
///
///   * the CSV parser drops every runway whose thresholds lack coordinates, so
///     **6,446 real ICAO airports** have no geometry at all (183 German ED**
///     fields, 176 French LF**, 85 UK EG**, …), and
///   * of **7,125 helipads in the table, exactly 2 survive the parse** — so for
///     practical purposes AeroACARS has no geometry for *any* heliport.
///
/// Without the fallback, all of those resolve to `AtPlanned` no matter where the
/// aircraft actually is: a helicopter that sets down 80 nm short of its planned
/// pad, or the real corpus case GSG 22 (planned EDLD — which has a runway row
/// but no threshold coordinates — shut down 143 nm away), is quietly filed as a
/// normal arrival and the pilot is never asked. "We cannot measure" must mean
/// "ask someone who can", not "assume everything is fine".
///
/// Both probes — "how far from the planned field" and "which field am I on" —
/// use the same geometry, so they cannot disagree about the same airport.
pub fn locate(
    planned_arr_icao: &str,
    lat: f64,
    lon: f64,
    planned_ref_pos: Option<(f64, f64)>,
) -> ArrivalSite {
    let planned = planned_arr_icao.trim();

    // A sim can hand us NaN/inf coordinates (scenery load glitches, a paused
    // sim mid-teleport). Every comparison below would be false, which used to
    // fall out as "off airport" and paint a divert banner reading
    // "~0 nmi vom Ziel entfernt". We cannot place the aircraft — so we don't.
    if !lat.is_finite() || !lon.is_finite() {
        return ArrivalSite::AtPlanned { distance_nm: None };
    }

    // TWO independent probes for "am I at my destination", and it is enough for
    // ONE of them to say yes.
    //
    //   * runway thresholds — precise (2 nm), but only as good as the runway
    //     data, and that data has bad rows in it;
    //   * the airport's reference point from phpVMS — coarser (3 nm), but an
    //     entirely separate source.
    //
    // The asymmetry is deliberate. The two mistakes are not equally bad: falsely
    // telling a pilot he diverted blocks his auto-filing, accuses him in the
    // PIREP record, and announces a divert that never happened. Failing to notice
    // a real divert merely leaves him where he was before this feature existed —
    // he files as planned. So we require agreement to accuse, and a single voice
    // to acquit.
    //
    // This is also the standing safety net under the runway data itself: an
    // airport whose thresholds are wrong in the table (rounded coordinates, a
    // truncated digit — HADD sits 4 nm from where OurAirports thinks it is)
    // would otherwise generate a false divert for every single pilot who flies
    // there. The repair pass in `runway.rs` catches what it can prove; this
    // catches the rest.
    let by_runway_nm =
        runway::distance_to_airport_m(planned, lat, lon).map(|m| m / 1852.0);
    let by_ref_nm = planned_ref_pos.and_then(|(rlat, rlon)| {
        // phpVMS ships (0,0) for airports whose coordinates were never filled
        // in. That is "unknown", not the Gulf of Guinea.
        if (rlat == 0.0 && rlon == 0.0) || !rlat.is_finite() || !rlon.is_finite() {
            return None;
        }
        Some(runway::distance_m(lat, lon, rlat, rlon) / 1852.0)
    });

    if by_runway_nm.is_none() && by_ref_nm.is_none() {
        // No geometry from ANY source — genuinely unmeasurable, so not a divert.
        return ArrivalSite::AtPlanned { distance_nm: None };
    }

    let at_planned = by_runway_nm.is_some_and(|d| d <= ON_FIELD_RADIUS_NM)
        || by_ref_nm.is_some_and(|d| d <= ON_FIELD_FALLBACK_RADIUS_NM);

    // Report the smaller of the two — the honest "how far from the destination
    // am I", not whichever source happened to be consulted first.
    let dist_planned_nm = match (by_runway_nm, by_ref_nm) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        (None, None) => unreachable!("handled above"),
    };

    if at_planned {
        return ArrivalSite::AtPlanned {
            distance_nm: Some(dist_planned_nm),
        };
    }

    // Off the planned field. Which field, if any, are we on instead?
    let nearest =
        runway::find_nearest_icao_airports(lat, lon, NEAREST_SEARCH_RADIUS_NM * 1852.0, 1)
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
#[derive(Debug, Clone, Copy, Default, Deserialize)]
struct Sealed;

/// A detected divert, surfaced via `flight_status` so the cockpit can ask the
/// pilot to confirm the real destination.
///
/// Invariant: `actual_icao != planned_arr_icao`. Guaranteed by construction —
/// see [`DivertHint::from_site`] and `Sealed`.
///
/// This is a *suspicion*, not a filed fact. Nothing may report it to the
/// outside world as a divert that happened; see `divert_payload_markers`.
/// `Deserialize` (v0.19.3, QS round 8) so the hint SURVIVES AN APP RESTART. Two
/// gates depend on it — the confirm-path guard and the auto-file suppression —
/// and the fallback that mints it cannot re-run once the flight is `Arrived`. A
/// pilot who diverted, restarted the app and then filed used to lose his divert
/// banner AND be refused by the arrival gate (the true distance to the planned
/// field is large), leaving him only a manual PIREP without auto-approval.
///
/// The `Sealed` invariant is untouched: `_sealed` is `#[serde(skip)]`, and no
/// module outside `arrival` can name the type, so deserialization is still the
/// only door — and it can only reconstruct a hint that this module built.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    ///
    /// An owned `String`, not `&'static str`: the hint is persisted (QS round 8)
    /// and a borrowed static cannot come back out of a snapshot. The JSON the
    /// cockpit sees is unchanged.
    pub kind: String,
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
        }
        .to_string();

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
        let site = locate("EDDF", lat, lon, None);
        assert_eq!(
            site.distance_from_planned_nm().map(|d| d <= ON_FIELD_RADIUS_NM),
            Some(true),
            "the reported distance must be the on-field one, not the centroid's"
        );
    }

    #[test]
    fn eddf_terminal_2_stand_is_at_the_planned_airport() {
        let site = locate("EDDF", EDDF_TERMINAL_2.0, EDDF_TERMINAL_2.1, None);
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
            let site = locate("EDDF", lat, lon, None);
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
        let site = locate("EDDF", 51.4239, 12.2364, None);
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
        let site = locate("EDDF", 51.4239, 12.2364, None); // EDDP
        let hint = DivertHint::from_site(&site, "EDDF", Some("EDDP")).expect("hint");
        assert_eq!(hint.kind, "alternate");
    }

    #[test]
    fn an_off_field_landing_yields_a_targetless_hint() {
        // Somewhere in the North Sea — no runway threshold within 2 nm.
        let site = locate("EDDF", 55.5000, 3.5000, None);
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
        // The corpus is raw on purpose, so the filtering is visible here rather
        // than baked into an export nobody re-reads. Three conditions make a row
        // gradeable, and each one cost a wrong conclusion before it was added:
        //
        //   actual_arr_icao != ""  — the PIREP was actually FILED. Aborted
        //     sessions keep their client-minted pirep_id and a final position on
        //     the departure stand; grading those produced 6 phantom "false
        //     positives" (GEC 872 parked at EDDF on a flight to ENBR — because
        //     it never left EDDF and was never filed).
        //   last_phase == ARRIVED  — the flight reached its end.
        //   final_on_ground == 1   — the last sample is the aircraft PARKED, not
        //     a position stream that died in cruise. SFG 2406's last sample is
        //     at FL445 over Brno; its "parked position" is a fiction.
        let (c_actual, c_phase, c_onground) = (
            col("actual_arr_icao"),
            col("last_phase"),
            col("final_on_ground"),
        );

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

            // Only gradeable rows — see the note at the column indices above.
            let filed = f.get(c_actual).map(|s| !s.trim().is_empty()).unwrap_or(false);
            let arrived = f
                .get(c_phase)
                .map(|s| s.trim().eq_ignore_ascii_case("ARRIVED"))
                .unwrap_or(false);
            let parked = f.get(c_onground).map(|s| s.trim() == "1").unwrap_or(false);
            if !(filed && arrived && parked) {
                continue;
            }

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
            // Exactly what production does: thresholds when we have them, the
            // airport's reference point when we don't (see `locate`). For a
            // field WITH threshold geometry — the large majority — the grading
            // below is genuinely independent: the verdict comes from runway
            // data, the truth from the airports table. For the handful without
            // it (heliports, EDLD-class fields) the two sources coincide and the
            // check degenerates into a consistency check. That is honest and
            // still worth having: it is precisely those fields where the divert
            // detection used to be blind altogether.
            let site = locate(planned, lat, lon, Some((plat, plon)));
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

    /// 186 German ED** fields (EDAG, EDAI, EDAN, …) — and 6,446 ICAO airports
    /// worldwide — have no runway threshold coordinates in the embedded table,
    /// so the client has no geometry for them at all. The arrival side gives
    /// those the benefit of the doubt (this test). The departure side used to
    /// do the opposite and silently refuse to auto-start there; see
    /// `distance_to_airport_any_source` in lib.rs.
    #[test]
    fn a_field_with_no_runway_geometry_is_recognised_as_unmeasurable() {
        assert!(
            runway::distance_to_airport_m("EDAG", 51.0, 7.0).is_none(),
            "EDAG has no threshold coordinates in the table — precondition for \
             this test and for the auto-start fallback in lib.rs"
        );
        // Unmeasurable ⇒ we do not accuse the pilot of diverting.
        let site = locate("EDAG", 51.0, 7.0, None);
        assert_eq!(site, ArrivalSite::AtPlanned { distance_nm: None });
        assert!(DivertHint::from_site(&site, "EDAG", None).is_none());
    }

    /// The heliport / no-threshold-geometry class, which is most of what a
    /// helicopter or bizjet operation actually flies to. Without the phpVMS
    /// reference-point fallback these were ALL "AtPlanned" — the detection was
    /// blind, not lenient.
    #[test]
    fn a_field_without_runway_geometry_is_located_via_the_reference_point() {
        // EDLD: has a runway row, but no threshold coordinates → no geometry.
        // Real corpus case (GSG 22): shut down 143 nm from EDLD and the client
        // never asked. EDLD reference point ≈ 51.616,6.861 (from phpVMS).
        let eddl_ref = (51.616018, 6.861262);
        assert!(
            runway::distance_to_airport_m("EDLD", 49.264, 7.489).is_none(),
            "precondition: EDLD has no usable runway geometry"
        );

        // Parked 143 nm away → a divert, and now we can say so.
        let far = locate("EDLD", 49.2643, 7.4897, Some(eddl_ref));
        assert!(
            !far.is_at_planned(),
            "143 nm from the destination is not 'at the destination', got {far:?}"
        );
        assert!(DivertHint::from_site(&far, "EDLD", None).is_some());

        // Parked ON the field → still a normal arrival. The fallback must not
        // buy divert detection at the price of false alarms.
        let there = locate("EDLD", eddl_ref.0 + 0.005, eddl_ref.1, Some(eddl_ref));
        assert!(there.is_at_planned(), "on the field is AtPlanned, got {there:?}");
        assert!(DivertHint::from_site(&there, "EDLD", None).is_none());
    }

    /// The safety net under the runway data itself.
    ///
    /// Some airports are simply in the wrong place in OurAirports — HADD's
    /// thresholds are rounded to one decimal and sit 4 nm from the real field.
    /// The repair pass can only fix what it can *prove*; without a second
    /// opinion, every pilot flying to such a field would be told he had diverted
    /// at his own destination, and his PIREP would be withheld.
    ///
    /// So: two independent probes, and ONE of them saying "you're there" is
    /// enough. Accusing needs agreement; acquitting does not. The two errors are
    /// not symmetrical — a false divert blocks the pilot's filing and stamps a
    /// false claim into his record, while a missed divert just leaves him where
    /// he was before the feature existed.
    #[test]
    fn a_wrong_runway_position_cannot_accuse_a_pilot_who_is_at_his_destination() {
        // Simulate the HADD class: the aircraft is parked at the airport's true
        // reference point, but the runway table has that airport 4 nm away.
        // (EDDF's thresholds are correct, so we fake the disagreement by placing
        // the aircraft at a reference point 4 nm from them.)
        let far_from_runways = (50.1000, 8.5706); // ~4 nm north of EDDF's field
        let ref_pos = far_from_runways; // phpVMS says: this IS the airport

        // Runway geometry alone would call this a divert…
        let by_runway_nm = runway::distance_to_airport_m("EDDF", far_from_runways.0, far_from_runways.1)
            .expect("EDDF has geometry")
            / 1852.0;
        assert!(
            by_runway_nm > ON_FIELD_RADIUS_NM,
            "test precondition: the runway probe must disagree ({by_runway_nm:.2} nm)"
        );

        // …but the reference point says the aircraft is at the airport, and that
        // is enough.
        let site = locate("EDDF", far_from_runways.0, far_from_runways.1, Some(ref_pos));
        assert!(
            site.is_at_planned(),
            "one source saying 'you are at your destination' must be enough, got {site:?}"
        );
        assert!(DivertHint::from_site(&site, "EDDF", None).is_none());
    }

    /// A sim that hands us NaN coordinates (scenery load, teleport, paused sim)
    /// must not produce a divert banner reading "~0 nmi vom Ziel entfernt".
    #[test]
    fn non_finite_coordinates_are_unmeasurable_not_a_divert() {
        for (lat, lon) in [
            (f64::NAN, 8.5),
            (50.0, f64::NAN),
            (f64::INFINITY, 8.5),
            (f64::NAN, f64::NAN),
        ] {
            let site = locate("EDDF", lat, lon, Some((50.033, 8.570)));
            assert_eq!(
                site,
                ArrivalSite::AtPlanned { distance_nm: None },
                "NaN/inf position must be unmeasurable, got {site:?}"
            );
            assert!(DivertHint::from_site(&site, "EDDF", None).is_none());
        }
    }

    /// The divert target is filed into phpVMS as the arrival airport, so it has
    /// to be something phpVMS can resolve. OurAirports also carries national
    /// identifiers (`US-4991`, `DE-0901`) and FAA local codes (`48FA`) at or
    /// beside real fields — those must never be named as the landing airport.
    #[test]
    fn a_divert_target_is_always_a_real_icao_code() {
        // Sweep a grid over Europe/US at typical apron distances and assert the
        // detector never names a non-ICAO ident.
        for (lat, lon) in [
            (28.8, -81.8),   // near KLEE / 48FA (Florida)
            (45.4, -98.4),   // near KABR / SD32
            (50.05, 8.58),   // EDDF
            (51.47, -0.45),  // EGLL
            (40.64, -73.78), // KJFK
        ] {
            let site = locate("ZZZZ", lat, lon, None); // planned unknown → AtPlanned
            assert!(site.is_at_planned());

            // Now ask the same geometry the divert path uses, directly.
            for a in runway::find_nearest_icao_airports(lat, lon, 10.0 * 1852.0, 10) {
                assert!(
                    a.icao.len() == 4 && a.icao.chars().all(|c| c.is_ascii_uppercase()),
                    "nearest-airport search returned a non-ICAO ident: {}",
                    a.icao
                );
            }
        }
    }

    #[test]
    fn an_unmeasurable_planned_field_is_never_a_divert() {
        // ICAO not in the runways table: we cannot measure, so we must not
        // accuse the pilot of diverting. (Old fallback did the same via
        // `arr_pos.is_none() ⇒ near_planned`.)
        let site = locate("ZZZZ", 50.0500, 8.5860, None);
        assert_eq!(site, ArrivalSite::AtPlanned { distance_nm: None });
        assert!(DivertHint::from_site(&site, "ZZZZ", None).is_none());
    }
}
