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
///
/// Zuletzt aktualisiert: 2026-08-06, direkt von
/// `https://ourairports.com/data/runways.csv` (48144 Zeilen, Rohformat,
/// unverändertes Spalten-Schema — 1:1 Drop-in). Update-Rezept: Datei neu
/// laden, hier ersetzen, `cargo test --lib runway::` laufen lassen (einige
/// Tests hängen an echten Koordinaten für konkrete Flughäfen).
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
///
/// Zuletzt aktualisiert: 2026-08-06. **Anders als `RUNWAYS_CSV` KEIN
/// Rohformat** — `airports_by_ident()` liest positionell nur die ersten 4
/// Spalten (`ident,type,latitude_deg,longitude_deg`), das rohe
/// `https://ourairports.com/data/airports.csv` hat aber ~19 Spalten in
/// anderer Reihenfolge. Update-Rezept: rohe `airports.csv` laden, per CSV-
/// Reader auf exakt diese 4 benannten Spalten reduzieren (nicht einfach
/// droppen — Spaltenreihenfolge im Rohformat hat sich zwischen 2023 und
/// 2026 bereits einmal geändert, `icao_code` kam neu dazu), Header
/// `ident,type,latitude_deg,longitude_deg` beibehalten, dann hier ersetzen
/// und `cargo test --lib runway::` laufen lassen.
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

/// Projiziert einen Punkt auf die Bahnachse und liefert Längs- und Querabstand.
///
/// # Warum es diese Funktion gibt
///
/// Dieselbe Kugelmathematik stand bis v1.7.0 **viermal** im Modul, die
/// Kreuzabweichung zweimal zeichengleich. Solange alle Kopien dasselbe rechnen,
/// fällt das nicht auf — genau bis jemand eine davon anfasst. Das ist die
/// Fehlerklasse, die bei den Zweitimplementierungen ausserhalb des Clients
/// schon zugeschlagen hat (siehe `docs/spec/v1.7.0-bahndisziplin.md` §9).
///
/// Ab v1.7.0 braucht die Bahndisziplin-Achse die Projektion ausserdem nicht mehr
/// nur für den Aufsetzpunkt, sondern für **jede Position des Rollwegs**. Damit
/// wird aus der Kopie eine gemeinsame Funktion.
///
/// # Die Achse kommt aus der Geometrie
///
/// Gebildet wird sie aus `threshold → end` der Navdaten, **nicht** aus dem
/// gemeldeten `true_course`. Das war schon immer so und ist der Grund, warum die
/// Werte belastbar sind: Ein gerundeter Kurs erzeugt über 3 km Bahn schnell
/// zweistellige Meterfehler, die wie seitliche Bewegung aussehen.
///
/// # Vorzeichen
///
/// * `laengs_m` — positiv in Landerichtung ab der Schwelle, negativ davor
///   (Aufsetzen vor der Schwelle).
/// * `quer_m` — **positiv = rechts** der Achse in Landerichtung.
pub fn projiziere_auf_bahn(
    threshold_lat: f64,
    threshold_lon: f64,
    end_lat: f64,
    end_lon: f64,
    lat: f64,
    lon: f64,
) -> (f64, f64) {
    let theta_ab = initial_bearing_rad(threshold_lat, threshold_lon, end_lat, end_lon);
    let theta_ac = initial_bearing_rad(threshold_lat, threshold_lon, lat, lon);
    let d_ab = haversine_m(threshold_lat, threshold_lon, lat, lon);

    // Kreuzabweichung ueber die Kugel — signiert ueber sin() der
    // Peilungsdifferenz. Positiv = rechts der Achse in Landerichtung.
    let xtd = (d_ab / EARTH_RADIUS_M).sin() * (theta_ac - theta_ab).sin();
    let quer_m = xtd.asin() * EARTH_RADIUS_M;

    // Laengsabstand: Betrag ueber die Kugel, Vorzeichen ueber die
    // Peilungsdifferenz. acos() liefert nie negativ — ohne das Vorzeichen
    // laesen sich Undershoot und Overshoot nicht unterscheiden.
    let cos_arg =
        ((d_ab / EARTH_RADIUS_M).cos() / (quer_m / EARTH_RADIUS_M).cos()).clamp(-1.0, 1.0);
    let laengs_betrag = cos_arg.acos() * EARTH_RADIUS_M;

    let mut diff = theta_ac - theta_ab;
    while diff > std::f64::consts::PI {
        diff -= 2.0 * std::f64::consts::PI;
    }
    while diff <= -std::f64::consts::PI {
        diff += 2.0 * std::f64::consts::PI;
    }
    let laengs_m = if diff.abs() > std::f64::consts::FRAC_PI_2 {
        -laengs_betrag
    } else {
        laengs_betrag
    };

    (laengs_m, quer_m)
}

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
    /// v0.19.x FIX: the CSV carries this (`le_displaced_threshold_ft`
    /// column) but it was never parsed — `RunwayMatch::displaced_threshold_ft`
    /// stayed unset for every OurAirports-fallback match, silently
    /// skipping DDS (pre-threshold / illegal-landing) classification and
    /// the LDA-based rollout-utilization correction for any landing that
    /// didn't resolve via Navigraph.
    le_displaced_threshold_ft: i32,
    he_ident: String,
    he_lat: f64,
    he_lon: f64,
    he_heading_true: f32,
    he_displaced_threshold_ft: i32,
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
    /// Threshold (= landing-direction end) lat/lon.
    ///
    /// Whether this is the physical pavement start or already the LEGAL
    /// landing threshold (on a runway with a displaced threshold) depends
    /// on the source — see `geometry_implied_displaced_threshold_ft`.
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
    /// Signed great-circle along-track distance from `threshold_lat/lon`
    /// (whatever that point represents — see its own doc comment and
    /// `geometry_implied_displaced_threshold_ft`), in feet. Positive =
    /// touchdown PAST that point (the normal case — pilot crossed it on
    /// final and put it down somewhere down the runway). Negative =
    /// touchdown BEFORE it (undershoot). Zero = touchdown exactly on that
    /// point within float precision.
    ///
    /// v0.19.x: on a runway with a displaced threshold where
    /// `threshold_lat/lon` is still the PHYSICAL start, this value is
    /// short of "distance from the LANDING threshold" by the part of
    /// `displaced_threshold_ft` NOT already reflected in
    /// `geometry_implied_displaced_threshold_ft` — `assess_touchdown` in
    /// `lib.rs` applies that one correction before feeding TDZ/Aim
    /// classification. When the geometry already implies the full
    /// displacement, `threshold_lat/lon` already IS the landing threshold
    /// and no correction applies — see v1.7.18 below.
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
    /// Distance from the physical runway start to the LEGAL landing
    /// threshold, in feet. 0 when the runway has no displaced threshold
    /// (the common case) or the source genuinely doesn't state one.
    ///
    /// v0.19.x FIX: this is available from BOTH sources — the OurAirports
    /// CSV carries `le_/he_displaced_threshold_ft` columns, they just
    /// weren't parsed. Before this fix, only a Navigraph-sourced match
    /// (via `NavRunway::displaced_threshold_ft`) could report a displaced
    /// threshold at all, so DDS (pre-threshold / illegal-landing)
    /// classification and the LDA-based rollout correction were silently
    /// skipped for every OurAirports-fallback landing, even when the CSV
    /// had the exact same data Navigraph would have used.
    pub displaced_threshold_ft: i32,
    /// v1.7.18 — wie viel Versatz die GEOMETRIE selbst schon zeigt: dass
    /// `threshold_lat/threshold_lon` bereits um so viele Fuss Richtung
    /// Bahnende von der gemessenen Distanz Schwelle→Gegenschwelle
    /// impliziert wird. 0, wenn die Geometrie keinen Versatz nahelegt.
    ///
    /// # Warum es dieses Feld gibt
    ///
    /// Navigraph (ARINC 424) meldet den Schwellenpunkt einer Bahn mit
    /// Versatz mal SCHON versetzt, mal noch am physischen Bahnanfang —
    /// unabhaengig davon, ob `displaced_threshold_ft` befuellt ist. Am
    /// echten Bestand nachgemessen (`geometry_implied_displacement_ft`,
    /// 40 zufaellige Bahnen mit echtem Versatz): **35 von 40 (87,5 %)**
    /// hatten den Punkt schon versetzt.
    ///
    /// Der fruehere Ansatz (`geometry_hidden_displacement_ft`) hat genau
    /// das ueber die eingebettete OurAirports-CSV zu erraten versucht —
    /// eine DRITTE, viel unzuverlaessigere Quelle. Gemessen am ganzen
    /// Bestand (5.716 Navigraph-Bahnen mit echtem Versatz) hatte
    /// OurAirports nur bei 34 % einen brauchbaren, einigermassen
    /// passenden Wert; bei 66 % riet der Check falsch — und zog den
    /// Versatz ein zweites Mal ab. **Genau dieser Fehler hat FDX2/LEMD
    /// 32L am 06.09.2026 einen normalen Touchdown 552 m VOR der Schwelle
    /// gezeigt**, obwohl der Pilot direkt auf dem Aim-Point aufsetzte.
    ///
    /// Dieses Feld ersetzt den Rate-Versuch durch eine Selbstprobe: es
    /// rechnet aus der GEMESSENEN Distanz Schwelle→Gegenschwelle zurueck,
    /// wie viel Versatz "Laenge minus Gegenschwellen-Versatz minus
    /// gemessene Distanz" ergibt — beide Versaetze aus Navigraph selbst,
    /// keine dritte Quelle noetig. `displaced_threshold_ft.max(dieses
    /// Feld)` ist der volle, informative Versatz (fuer die nutzbare
    /// Laenge); `(displaced_threshold_ft - dieses Feld).max(0)` ist der
    /// Anteil, der von der Aufsetzdistanz NOCH abgezogen werden muss —
    /// steckt der volle Versatz schon in der Geometrie, bleibt davon 0.
    pub geometry_implied_displaced_threshold_ft: i32,
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

/// Der Belag dieser Bahn aus der eingebetteten OurAirports-Tabelle.
///
/// # Warum das gebraucht wird
///
/// Die Navdaten tragen ein `surface`-Feld, und der Server fuellt es aus
/// `nav_runways.surface_code`. Dieses Feld ist **in allen 85.058 Zeilen
/// leer** — am 24.08.2026 auf dem Live-Server nachgezaehlt, null Treffer.
///
/// Auf dem Navigraph-Pfad wurde daraus per `unwrap_or_default()` der
/// leere String. Der faellt durch jede Belagspruefung, ergibt
/// `Belag::Unbekannt` und damit `surface_unknown` — die seitliche
/// Bewertung entfiel. Nicht bei Sonderfaellen, sondern bei **jedem** Flug
/// zu einem Flughafen, der in den Navdaten steht, also praktisch bei
/// jedem echten Flug. Gemeldet am ersten Live-Tag von v1.7.0, EDDL.
///
/// Die richtige Angabe lag die ganze Zeit daneben: `EDDL` steht in
/// `data/ourairports-runways.csv` mit `CON`, und 47.658 der 48.162
/// Bahnen dort haben einen Belag. Der CSV-Pfad las sie immer; der
/// Navigraph-Pfad nie.
///
/// Die Bahnkennung wird beidseitig verglichen, weil eine Zeile beide
/// Enden fuehrt (`05L` und `23R`) — der Belag gilt fuer die ganze Bahn.
fn belag_aus_tabelle(icao: &str, bahn: &str) -> Option<String> {
    let gesucht = bahn.trim().to_uppercase();
    belaege()
        .get(&(icao.trim().to_uppercase(), gesucht))
        .cloned()
}

/// Belag je (Flugplatz, Bahn) — aus **allen** Zeilen der Tabelle.
///
/// # Warum das eine EIGENE Tabelle ist
///
/// `runways()` verwirft jede Zeile ohne Koordinaten an beiden Enden.
/// Für die Bahnzuordnung ist das richtig — ohne Punkte ist eine Bahn
/// nicht zu treffen. Für den BELAG ist es falsch: Der steht in der
/// Zeile, ganz gleich ob Koordinaten dabei sind.
///
/// Ausgezählt über die eingebettete Tabelle (25.08.2026):
///
/// ```text
/// 48.143 Bahnen gesamt
/// 32.488 ohne Koordinaten  (67,5 %)  → fielen ganz heraus
/// 32.034 davon MIT Belagsangabe, an 29.520 Flugplätzen
/// ```
///
/// Zwei Drittel der Belagsangaben waren damit unerreichbar. Aufgefallen
/// an GSG1321 (EDBH→EDHE, 25.08.2026): EDHE/Uetersen ist eine Graspiste,
/// OurAirports führt sie als `GRASS` — und der Bericht meldete „Belag
/// unbekannt", weil die EDHE-Zeile keine Koordinaten hat.
///
/// Der Schlüssel enthält beide Bahnenden, weil eine Zeile beide führt
/// (`09` und `27`) und der Belag für die ganze Bahn gilt.
fn belaege() -> &'static std::collections::HashMap<(String, String), String> {
    static CELL: OnceLock<std::collections::HashMap<(String, String), String>> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut map = std::collections::HashMap::with_capacity(64_000);
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(RUNWAYS_CSV.as_bytes());
        for record in rdr.records().flatten() {
            // Geschlossene Bahnen bleiben draussen — auf ihnen landet
            // niemand, und ihr Belag wuerde eine offene Bahn gleichen
            // Namens ueberschreiben.
            if record.get(7).unwrap_or("0") == "1" {
                continue;
            }
            let belag = record.get(5).unwrap_or("").trim();
            if belag.is_empty() {
                continue;
            }
            let icao = record.get(2).unwrap_or("").trim().to_uppercase();
            if icao.is_empty() {
                continue;
            }
            for spalte in [8usize, 14usize] {
                let bahn = record.get(spalte).unwrap_or("").trim().to_uppercase();
                if !bahn.is_empty() {
                    map.insert((icao.clone(), bahn), belag.to_string());
                }
            }
        }
        map
    })
}

/// Wie viel Versatz der LANDE-Schwelle die GEOMETRIE selbst schon zeigt —
/// unabhaengig davon, was `displaced_threshold_ft` sagt. 0, wenn die
/// Geometrie keinen Versatz nahelegt (Schwellenpunkt = physischer
/// Bahnanfang).
///
/// **Hintergrund (v1.7.18).** Navigraph (ARINC 424) meldet den
/// Schwellenpunkt einer Bahn mit Versatz mal SCHON versetzt (LEMD 32L,
/// OLBA 35, TJPS 12 — alle drei geprueft), mal noch am physischen
/// Bahnanfang — unabhaengig davon, ob `displaced_threshold_ft` befuellt
/// ist. Der fruehere Ansatz (`geometry_hidden_displacement_ft`, entfernt)
/// hat genau das ueber die eingebettete OurAirports-CSV zu erraten
/// versucht — eine DRITTE, viel unzuverlaessigere Quelle: von 5.716
/// Navigraph-Bahnen mit echtem Versatz hatte OurAirports nur bei 34 %
/// einen brauchbaren Wert, bei 66 % riet der Check falsch und zog den
/// Versatz ein zweites Mal ab. Genau dieser Fehler zeigte FDX2/LEMD 32L
/// am 06.09.2026 einen normalen Touchdown 552 m VOR der Schwelle, obwohl
/// der Pilot direkt auf dem Aim-Point aufsetzte.
///
/// Diese Funktion fragt stattdessen NUR Navigraph selbst: `far_end` ist
/// die GEGENUEBERLIEGENDE Schwelle desselben Bahnpaars (bit-identisch mit
/// deren eigenem `threshold`), `far_end_displaced_threshold_ft` ist DEREN
/// eigener Versatz. "Bahnlaenge minus Gegenschwellen-Versatz minus
/// gemessene Distanz Schwelle→Gegenschwelle" ist genau der Versatz, der
/// schon in UNSEREM Schwellenpunkt steckt — egal was `displaced_
/// threshold_ft` dazu sagt. Das erkennt auch den Fall, den v1.6.8 als
/// Zukunftsrisiko dokumentiert hatte: ein kuenftiger Zyklus, der die
/// Geometrie verschiebt, das Zahlenfeld aber auf 0 laesst.
///
/// Ergebnis wird auf `[0, length_ft/2]` geklammert: negativ (Messrauschen
/// bei keinem Versatz) wird 0, mehr als die halbe Bahn waere kein
/// plausibler Versatz mehr.
///
/// Am Bestand geprueft (40 zufaellige Navigraph-Bahnen mit echtem
/// Versatz): 35 von 40 (87,5 %) hatten den Punkt schon versetzt, mit
/// einer Abweichung von hoechstens 13,7 m gegen den erwarteten Versatz;
/// die vier physischen Faelle wichen mindestens 30,8 m ab (der Rest lag
/// bei 0, korrekt erkannt als "kein Versatz in der Geometrie").
pub fn geometry_implied_displacement_ft(
    threshold_lat: f64,
    threshold_lon: f64,
    end_lat: f64,
    end_lon: f64,
    length_ft: f32,
    far_end_displaced_threshold_ft: i32,
) -> i32 {
    // `!length_ft.is_finite()` MUSS vor dem Groessenvergleich stehen —
    // unter NaN waeren die Vergleiche unten wirkungslos (dieselbe Falle,
    // die den Vorgaenger dieser Funktion einmal im Review erwischt hat).
    if !length_ft.is_finite() || length_ft <= 0.0 {
        return 0;
    }
    let geometrie_m = haversine_m(threshold_lat, threshold_lon, end_lat, end_lon);
    if !geometrie_m.is_finite() {
        return 0;
    }
    let gegen_m = far_end_displaced_threshold_ft.max(0) as f64 * 0.3048;
    let laenge_m = length_ft as f64 * 0.3048;
    let implied_m = laenge_m - gegen_m - geometrie_m;
    // Mindestgroesse: Koordinaten-Rundung allein erzeugt schon ein paar
    // Meter Rest, auch OHNE jeden Versatz (CRG3 33: 3,4 m, EDDB 06R:
    // 11,6 m). Unter `MIN_PLAUSIBEL_M` ist das Messrauschen, kein echter
    // Versatz — der kleinste ECHTE Versatz in der 40-Bahnen-Stichprobe
    // lag bei 98 ft (30 m), mit klarem Abstand nach oben.
    const MIN_PLAUSIBEL_M: f64 = 20.0;
    if implied_m <= MIN_PLAUSIBEL_M || implied_m >= laenge_m * 0.5 {
        return 0;
    }
    (implied_m / 0.3048).round() as i32
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
            // v0.19.x FIX: le_displaced_threshold_ft / he_displaced_threshold_ft
            // (columns 13 / 19) — present in the CSV, previously never read.
            let le_displaced_threshold_ft = parse_f32(record.get(13)).unwrap_or(0.0) as i32;
            let he_heading_csv = parse_f32(record.get(18));
            let headings_stated = le_heading_csv.is_some() && he_heading_csv.is_some();
            let le_heading = le_heading_csv
                .unwrap_or_else(|| initial_bearing_deg(le_lat, le_lon, he_lat, he_lon) as f32);
            let he_ident = record.get(14).unwrap_or("").to_string();
            let he_heading = he_heading_csv
                .unwrap_or_else(|| initial_bearing_deg(he_lat, he_lon, le_lat, le_lon) as f32);
            let he_displaced_threshold_ft = parse_f32(record.get(19)).unwrap_or(0.0) as i32;

            out.push(RunwayRow {
                airport_ident,
                length_ft,
                width_ft,
                surface,
                le_ident,
                le_lat,
                le_lon,
                le_heading_true: le_heading,
                le_displaced_threshold_ft,
                he_ident,
                he_lat,
                he_lon,
                he_heading_true: he_heading,
                he_displaced_threshold_ft,
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
    let (br, p1, l1) = (bearing_deg.to_radians(), lat.to_radians(), lon.to_radians());
    let p2 = (p1.sin() * ang.cos() + p1.cos() * ang.sin() * br.cos()).asin();
    let l2 = l1 + (br.sin() * ang.sin() * p1.cos()).atan2(ang.cos() - p1.sin() * p2.sin());
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
        if (approx_lat - lat).abs() > lat_span_deg || lon_delta_deg(approx_lon, lon) > lon_span_deg
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
pub fn lookup_runway(lat: f64, lon: f64, aircraft_heading_true_deg: f32) -> Option<RunwayMatch> {
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
        let (
            threshold_lat,
            threshold_lon,
            end_lat,
            end_lon,
            runway_ident,
            runway_heading,
            displaced_threshold_ft,
        ) = if le_diff <= he_diff {
            (
                row.le_lat,
                row.le_lon,
                row.he_lat,
                row.he_lon,
                row.le_ident.clone(),
                row.le_heading_true,
                row.le_displaced_threshold_ft,
            )
        } else {
            (
                row.he_lat,
                row.he_lon,
                row.le_lat,
                row.le_lon,
                row.he_ident.clone(),
                row.he_heading_true,
                row.he_displaced_threshold_ft,
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
        // v1.7.0: eine gemeinsame Projektion statt vier Kopien der
        // Kugelmathematik — siehe `projiziere_auf_bahn`.
        let (along_signed_m, xtd_m) =
            projiziere_auf_bahn(threshold_lat, threshold_lon, end_lat, end_lon, lat, lon);
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
            displaced_threshold_ft,
            // OurAirports' own convention: `le_/he_latitude_deg` is always
            // the physical runway end, never the displaced threshold —
            // see the struct doc. Always 0 here, unconditionally.
            geometry_implied_displaced_threshold_ft: 0,
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

/// v0.21.x Feld-Report (Thomas, EFTP/BTI357): der Touchdown wurde gegen
/// "24C" statt der echten Bahn "24" gematcht. Ursache: die Aerosoft-DFD-
/// Navdata enthält für manche Flughäfen einen zusätzlichen, fehlerhaften
/// Bahn-Eintrag mit fast identischer Peilung wie eine reale Bahn — aber
/// LÄNGS (nicht seitlich) versetzt, mit abweichender Länge. Ein
/// systematischer Abgleich gegen alle ~42k Navigraph-Bahnen (Zyklus 2607)
/// fand 38 betroffene Flughäfen weltweit (EFTP, EGSS, LFxx-Kleinflugplätze,
/// EGXY u.a.) — siehe internes Audit vom 2026-08-06.
///
/// Ein reiner Abstands-Schwellenwert reicht NICHT als Kriterium: viele
/// echte, eng beieinanderliegende Parallelbahnen (z. B. EGCB 08L/08R nur
/// 31 m auseinander) hätten sonst false positives erzeugt. Das eigentliche
/// Unterscheidungsmerkmal: bei den Phantom-Fällen existiert eine
/// UNSUFFIXIERTE Basis-Bahn ("24") UND eine verdächtige Variante ("24C"),
/// wobei die verdächtige Variante der öffentlichen OurAirports-Referenz
/// (dieselbe, die auch der Fallback-Pfad nutzt) unbekannt ist — echte
/// Parallelbahnen sind dort so gut wie immer BEIDE gepflegt.
///
/// Kandidaten am selben Flughafen werden paarweise verglichen; hat einer
/// eine nahezu identische Peilung (< 5°) zu einem anderen UND ist er der
/// OurAirports-Referenz unbekannt, während der andere dort bestätigt ist,
/// wird der unbestätigte Kandidat verworfen. Kennt OurAirports keinen von
/// beiden (kleine, dort unerfasste Plätze) oder beide, bleibt die Liste
/// unverändert — im Zweifel wird nichts entfernt.
///
/// **Läuft EINMAL pro Airport beim Navdata-Fetch** (`spawn_navdata_fetch`
/// in lib.rs, direkt vor dem Cache-Insert), NICHT in jedem einzelnen
/// Konsumenten. Grund (Code-Review 2026-08-06): `lookup_runway_in_nav`
/// war nicht der einzige Leser von `NavAirport.runways` — auch
/// `runway_glideslope_for`/`resolve_approach_glideslope_deg` (lib.rs, live
/// im Anflug-Banner) las die Rohliste direkt und wäre von genau demselben
/// Phantom-Bug betroffen geblieben, wenn der Dedupe nur im Matcher gelaufen
/// wäre. Beide (und jeder künftige Konsument) lesen `flight.navdata`, also
/// reicht ein einziger Reinigungspunkt an der Cache-Grenze — kein
/// Sonderfall pro Aufrufer.
pub(crate) fn dedupe_near_duplicate_nav_runways(
    icao: &str,
    runways: Vec<aeroacars_mqtt::navdata::NavRunway>,
) -> Vec<aeroacars_mqtt::navdata::NavRunway> {
    // Einmal pro Bahn berechnen statt einmal pro Paar (bis zu n-1 Mal
    // wiederholt) — bei n Bahnen O(n) statt O(n²) OurAirports-Lookups.
    let known: Vec<bool> = runways
        .iter()
        .map(|r| ourairports_has_runway_designator_for(icao, &r.designator))
        .collect();
    let mut discard: Vec<bool> = vec![false; runways.len()];
    for i in 0..runways.len() {
        for j in (i + 1)..runways.len() {
            if discard[i] || discard[j] {
                continue;
            }
            if heading_diff(runways[i].true_course as f32, runways[j].true_course as f32) >= 5.0 {
                continue;
            }
            match (known[i], known[j]) {
                (true, false) => discard[j] = true,
                (false, true) => discard[i] = true,
                // Beide bekannt oder beide unbekannt → keine Aussage
                // möglich, Liste bleibt unveraendert fuer dieses Paar.
                _ => {}
            }
        }
    }
    runways
        .into_iter()
        .zip(discard)
        .filter_map(|(r, d)| if d { None } else { Some(r) })
        .collect()
}

/// Ob `designator` (z. B. "24C") am gegebenen Flughafen in der
/// eingebauten OurAirports-Referenz als `le_ident` oder `he_ident`
/// vorkommt. Nutzt denselben Airport-Index wie `rows_for_airport` (statt
/// eines eigenen linearen Scans über die volle 48k-Zeilen-Tabelle) und
/// normalisiert den Designator genau wie `rows_for_airport` den ICAO
/// normalisiert — ansonsten könnte ein Groß-/Kleinschreibungs- oder
/// Whitespace-Unterschied einen echten Treffer als "unbekannt" verfehlen.
fn ourairports_has_runway_designator_for(icao: &str, designator: &str) -> bool {
    let designator = designator.trim().to_uppercase();
    rows_for_airport(icao).any(|r| {
        r.le_ident.trim().to_uppercase() == designator
            || r.he_ident.trim().to_uppercase() == designator
    })
}

/// Wie `lookup_runway`, aber gegen die NavRunway-Liste eines per VPS
/// geladenen Airports. Mathematik ist identisch — die Quelle ist nur
/// genauer (Jeppesen-Threshold-Koordinaten statt Community-CSV).
///
/// Verhalten:
///   * Erwartet `airport.runways` bereits bereinigt von Phantom-Duplikaten
///     (siehe `dedupe_near_duplicate_nav_runways`) — das läuft einmalig
///     beim Navdata-Fetch, nicht hier, damit auch andere Konsumenten des
///     `flight.navdata`-Caches (z. B. `runway_glideslope_for` in lib.rs)
///     dieselbe bereinigte Liste sehen.
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
        // Der Belag aus den Navdaten — und wenn er fehlt, aus der
        // eingebetteten Tabelle. Er fehlt IMMER: `nav_runways.surface_code`
        // ist in allen 85.058 Zeilen leer (siehe `belag_aus_tabelle`).
        let surface = rwy
            .surface
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| belag_aus_tabelle(&airport.icao, &rwy.designator))
            .unwrap_or_default();

        let d_threshold = haversine_m(threshold_lat, threshold_lon, lat, lon);
        if d_threshold > DEFAULT_MAX_DISTANCE_M + (length_ft as f64 * 0.3048) {
            continue;
        }

        // v1.7.0: dieselbe gemeinsame Projektion wie der CSV-Pfad. Der
        // Kommentar hier lautete "Kept verbatim so MS713-equivalent calls
        // reproduce identical signs" — genau diese Zusicherung haelt eine
        // gemeinsame Funktion besser als zwei zeichengleiche Kopien.
        let (along_signed_m, xtd_m) =
            projiziere_auf_bahn(threshold_lat, threshold_lon, end_lat, end_lon, lat, lon);
        let along_ft = along_signed_m * 3.280_839_895;
        let centerline_distance_abs_ft = xtd_m.abs() * 3.280_839_895;
        let side = if xtd_m.abs() < CENTERLINE_TOLERANCE_M {
            "CENTER"
        } else if xtd_m > 0.0 {
            "RIGHT"
        } else {
            "LEFT"
        };

        // Der Versatz der GEGENSCHWELLE, aus derselben Navigraph-Liste —
        // gebraucht fuer die Selbstprobe unten. `far_end` ist bit-
        // identisch mit dem `threshold` der Gegenbahn (siehe
        // `geometry_implied_displacement_ft`), also reicht ein
        // Koordinatenvergleich statt einer zweiten Kennungs-Zuordnung
        // (07L↔25R, 12↔30, …), die hier nicht dupliziert werden muss.
        let gegenschwelle_versatz_ft = airport
            .runways
            .iter()
            .find(|other| other.threshold.lat == end_lat && other.threshold.lon == end_lon)
            .map(|other| other.displaced_threshold_ft)
            .unwrap_or(0);

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
            displaced_threshold_ft: rwy.displaced_threshold_ft,
            geometry_implied_displaced_threshold_ft: geometry_implied_displacement_ft(
                threshold_lat,
                threshold_lon,
                end_lat,
                end_lon,
                length_ft,
                gegenschwelle_versatz_ft,
            ),
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
    let cos_arg =
        ((d_threshold / EARTH_RADIUS_M).cos() / (xtd / EARTH_RADIUS_M).cos()).clamp(-1.0, 1.0);
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

// ---- Live-Bahnvorhersage im Endanflug (#msfs-hud) -------------------------

/// Maximale Kursabweichung zwischen Flugzeug und Bahn, damit eine Bahn
/// überhaupt als Anflugziel in Frage kommt. 30° lässt ein Aufschalten aus
/// dem Base-Leg heraus noch zu, schließt Gegenanflug (180°) und Queranflug
/// (90°) aber sicher aus.
const PREDICT_MAX_HEADING_DIFF_DEG: f32 = 30.0;
/// Ab welcher Entfernung zur Schwelle gar nicht erst vorhergesagt wird
/// (15 NM). Weiter draußen ist die Bahnwahl bei Parallelbahnen ohnehin
/// noch nicht entschieden.
const PREDICT_MAX_THRESHOLD_DISTANCE_M: f64 = 27_780.0;
/// Maximaler seitlicher Versatz zur verlängerten Mittellinie (2 NM).
const PREDICT_MAX_CENTERLINE_OFFSET_M: f64 = 3_704.0;

/// Vorhergesagte Landebahn, solange der Flug noch in der Luft ist.
///
/// **Ausdrücklich eine Schätzung.** Der genaue Bahn-Ident kommt weiterhin
/// erst beim Aufsetzen aus [`lookup_runway`] / dem Korrelations-Match
/// (97,5 % Trefferquote laut Daten-Audit vom 2026-06-11). Dieser Wert hier
/// existiert nur, weil MSFS `ATC RUNWAY SELECTED` nicht liefert — der
/// Adapter setzt `selected_runway` fest auf `None`, weshalb
/// `stats.approach_runway` im selben Audit bei 407 von 407 Flügen leer war.
///
/// Konsequenz: dieser Wert darf **nie** in den Touchdown- oder PIREP-Payload
/// fließen. Er speist ausschließlich die Live-Anzeige und den Live-Gleitwinkel.
#[derive(Debug, Clone, PartialEq)]
pub struct PredictedRunway {
    pub designator: String,
    /// Publizierter Gleitwinkel, bereits auf 2–7,5° plausibilisiert.
    /// `None`, wenn die Navdaten keinen brauchbaren Wert führen — der
    /// Aufrufer fällt dann auf den 3°-Standard zurück.
    pub glideslope_angle: Option<f64>,
    pub distance_to_threshold_m: f64,
    /// Seitlicher Versatz zur Mittellinie in Metern, vorzeichenbehaftet:
    /// positiv = rechts der Anfluglinie.
    pub centerline_offset_m: f64,
}

/// Rät die Landebahn aus Position und Steuerkurs gegen die Navdaten des
/// Zielflughafens (die beim Flugstart in den Per-Flug-Cache geladen werden,
/// im Anflug also ohne Netz verfügbar sind).
///
/// Auswahl in drei Stufen: erst Bahnen verwerfen, deren Richtung nicht zum
/// Steuerkurs passt, dann die zu weit entfernten oder seitlich zu weit
/// abliegenden, und unter dem Rest die mit dem kleinsten Abstand zur
/// Mittellinie nehmen. Genau dieser letzte Schritt trennt Parallelbahnen
/// (26L/26R) — die Schwellenentfernung allein kann das nicht, weil beide
/// Schwellen praktisch nebeneinander liegen.
///
/// `None` heißt schlicht „noch nicht entschieden“, nicht „Fehler“: früh im
/// Anflug ist das der normale Zustand.
pub fn predict_landing_runway(
    runways: &[aeroacars_mqtt::navdata::NavRunway],
    lat: f64,
    lon: f64,
    heading_true_deg: f32,
) -> Option<PredictedRunway> {
    let mut best: Option<(PredictedRunway, f64)> = None;

    for rw in runways {
        if heading_diff(heading_true_deg, rw.true_course as f32) > PREDICT_MAX_HEADING_DIFF_DEG {
            continue;
        }

        let (t_lat, t_lon) = (rw.threshold.lat, rw.threshold.lon);
        let (e_lat, e_lon) = (rw.far_end.lat, rw.far_end.lon);

        let d_threshold = haversine_m(t_lat, t_lon, lat, lon);
        if d_threshold > PREDICT_MAX_THRESHOLD_DISTANCE_M {
            continue;
        }

        let theta_ab = initial_bearing_rad(t_lat, t_lon, e_lat, e_lon);
        let theta_ac = initial_bearing_rad(t_lat, t_lon, lat, lon);
        let xtd_m = ((d_threshold / EARTH_RADIUS_M).sin() * (theta_ac - theta_ab).sin()).asin()
            * EARTH_RADIUS_M;
        if xtd_m.abs() > PREDICT_MAX_CENTERLINE_OFFSET_M {
            continue;
        }

        // Über das Bahnende hinaus ist es kein Anflug mehr auf diese Bahn.
        let along_m = along_track_m_signed(t_lat, t_lon, e_lat, e_lon, lat, lon);
        if along_m > rw.length_ft as f64 * 0.3048 {
            continue;
        }

        let candidate = PredictedRunway {
            designator: rw.designator.trim().to_string(),
            glideslope_angle: Some(rw.glideslope_angle).filter(|g| (2.0..=7.5).contains(g)),
            distance_to_threshold_m: d_threshold,
            centerline_offset_m: xtd_m,
        };
        let score = xtd_m.abs();
        match &best {
            Some((_, best_score)) if *best_score <= score => {}
            _ => best = Some((candidate, score)),
        }
    }

    best.map(|(m, _)| m)
}

/// v1.5.1 (#hud-pilotenfeedback F3): Großkreis-Distanz in Seemeilen —
/// die einzige öffentliche Distanz-Schnittstelle dieses Moduls. Bewusst
/// ein Wrapper statt `haversine_m` zu öffnen: die Meter-Variante bleibt
/// ein Implementierungsdetail der Bahn-Korrelation.
pub fn distance_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    haversine_m(lat1, lon1, lat2, lon2) / 1852.0
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
            for (lat, lon, end) in [(r.le_lat, r.le_lon, "06R"), (r.he_lat, r.he_lon, "24L")] {
                let off_nm = haversine_m(lat, lon, 41.4117, -81.8498) / 1852.0;
                assert!(
                    off_nm < 2.0,
                    "KCLE {end} is {off_nm:.2} nm from the airport — corrupt coordinate not repaired"
                );
            }
        }

        // The bad coordinate must no longer make a point 4 nm off the field
        // look like "at KCLE".
        let phantom_nm =
            distance_to_airport_m("KCLE", 41.300, -81.800).expect("KCLE has geometry") / 1852.0;
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
        let belgorod_nm =
            distance_to_airport_m("UUMU", 50.6485, 36.5757).expect("UUMU has geometry") / 1852.0;
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
            near_line
                .iter()
                .map(|a| (&a.icao, a.lon))
                .collect::<Vec<_>>()
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

    // v0.19.x FIX: le_/he_displaced_threshold_ft (CSV columns 13/19) exist
    // in the bundled data but were never parsed — RunwayMatch always
    // reported 0, silently skipping DDS classification and the LDA
    // rollout correction for every OurAirports-fallback landing, even on
    // a runway the CSV itself says has a real displaced threshold.
    //
    // OLBA runway 35 from the bundled CSV: le="17" 33.838199615478516,
    // 35.487098693847656, hdg 177, no displacement; he="35"
    // 33.80929946899414, 35.48889923095703, hdg 357, displaced 2788 ft.
    const OLBA_35_THR_LAT: f64 = 33.809_299_468_994_14;
    const OLBA_35_THR_LON: f64 = 35.488_899_230_957_03;

    #[test]
    fn csv_source_reports_the_displaced_threshold_when_the_csv_states_one() {
        let m = lookup_runway(OLBA_35_THR_LAT, OLBA_35_THR_LON, 357.0)
            .expect("should resolve to OLBA/35");
        assert_eq!(m.airport_ident, "OLBA");
        assert_eq!(m.runway_ident, "35");
        assert_eq!(
            m.displaced_threshold_ft, 2788,
            "OLBA/35 has a real displaced threshold in the bundled CSV — must not silently read as 0"
        );
    }

    // ── v1.7.18: geometry_implied_displacement_ft gegen echte Bahnen ──
    //
    // Alle Koordinaten/Laengen/Versaetze unten stammen aus der echten
    // Navigraph-AIRAC-2609-Datenbank (`tbl_runways`, Stand 06.09.2026,
    // `~/NAVI_AIR/realtr/navdb.s3db`) bzw. sind unabhaengig im Web
    // nachgeprueft (LEMD 32L: 3045 ft Versatz, FAA/Jeppesen-Quellen).
    // Das ist der Kern der Regression: diese drei Bahnen sind die
    // FDX2/LEMD-32L-Klasse von Fehler ("Aufsetzen 552 m vor der Schwelle
    // gemeldet, obwohl der Pilot auf dem Aim-Point aufsetzte").

    #[test]
    fn lemd_32l_zeigt_den_versatz_schon_in_der_geometrie() {
        // Ausloeser dieses Umbaus: FDX2/LEMD 32L, 06.09.2026. Schwelle
        // 32L (40.46308333, -3.55389444), Gegenschwelle 14R
        // (40.48486111, -3.57601111, kein eigener Versatz), Bahn
        // 13084 ft. Real veroeffentlichter Versatz: 3045 ft (928 m).
        let ft = geometry_implied_displacement_ft(
            40.463_083_33,
            -3.553_894_44,
            40.484_861_11,
            -3.576_011_11,
            13084.0,
            0,
        );
        assert!(
            (ft - 3045).abs() <= 10,
            "LEMD 32L: erwarteter Versatz ~3045 ft aus der Geometrie, war {ft}"
        );
    }

    #[test]
    fn olba_35_zeigt_den_versatz_schon_in_der_geometrie() {
        // OLBA 35 (33.81665278, 35.488375), Gegenschwelle 17
        // (33.83836389, 35.48697778, kein eigener Versatz), Bahn
        // 10663 ft, Navigraph-Feld 2690 ft.
        let ft = geometry_implied_displacement_ft(
            33.816_652_78,
            35.488_375,
            33.838_363_89,
            35.486_977_78,
            10663.0,
            0,
        );
        assert!(
            (ft - 2690).abs() <= 50,
            "OLBA 35: erwarteter Versatz ~2690 ft aus der Geometrie, war {ft}"
        );
    }

    #[test]
    fn tjps_12_zeigt_den_versatz_trotz_versetzter_gegenschwelle() {
        // TJPS 12 (Flug LAN273, 30.08.2026 — der historische Ausloeser
        // von v1.7.12): Schwelle 12 (18.01057778, -66.57032778),
        // Gegenschwelle 30 (18.00559722, -66.55423611) mit EIGENEM
        // Versatz von 247 ft — die Selbstprobe muss den auch abziehen,
        // sonst kommt ein um 247 ft zu grosser Wert heraus. Bahn 8002 ft,
        // Navigraph-Feld fuer 12: 1879 ft.
        let ft = geometry_implied_displacement_ft(
            18.010_577_78,
            -66.570_327_78,
            18.005_597_22,
            -66.554_236_11,
            8002.0,
            247,
        );
        assert!(
            (ft - 1879).abs() <= 15,
            "TJPS 12: erwarteter Versatz ~1879 ft aus der Geometrie, war {ft}"
        );
    }

    #[test]
    fn physische_schwelle_liefert_null_versatz_aus_der_geometrie() {
        // CRG3 33 und CPL4 25: beides Faelle aus einer 40-Bahnen-
        // Stichprobe, in denen der Navigraph-Schwellenpunkt NICHT
        // versetzt ist (Feld 200 bzw. 100 ft, aber die Geometrie misst
        // die volle Bahn) — die Funktion darf hier nichts erfinden.
        assert_eq!(
            geometry_implied_displacement_ft(45.474_833_33, -73.297_736_11, 45.480_991_67, -73.305_441_67, 3000.0, 0),
            0,
            "CRG3 33: Geometrie zeigt keinen Versatz"
        );
        assert_eq!(
            geometry_implied_displacement_ft(43.288_875, -81.710_533_33, 43.285_716_67, -81.718_208_33, 2340.0, 0),
            0,
            "CPL4 25: Geometrie zeigt keinen Versatz"
        );
    }

    #[test]
    fn kein_versatz_gemeldet_und_keiner_in_der_geometrie() {
        // EDDB 06R/24L: beide Enden ohne Versatz, Geometrie misst die
        // volle Bahn (13123 ft) — nichts zu erkennen.
        assert_eq!(
            geometry_implied_displacement_ft(
                52.345_433_33,
                13.468_427_78,
                52.358_425,
                13.523_161_11,
                13123.0,
                0
            ),
            0
        );
    }

    #[test]
    fn unsinnige_eingaben_liefern_null_statt_zu_verrutschen() {
        // Review-Lehre aus dem Vorgaenger: `!length_ft.is_finite()` MUSS
        // vor dem Groessenvergleich stehen, sonst rutscht NaN durch.
        assert_eq!(
            geometry_implied_displacement_ft(40.0, -3.0, 40.02, -3.02, f32::NAN, 0),
            0
        );
        assert_eq!(
            geometry_implied_displacement_ft(40.0, -3.0, 40.02, -3.02, f32::INFINITY, 0),
            0
        );
        assert_eq!(
            geometry_implied_displacement_ft(40.0, -3.0, 40.02, -3.02, -13084.0, 0),
            0
        );
        assert_eq!(
            geometry_implied_displacement_ft(40.0, -3.0, 40.02, -3.02, 0.0, 0),
            0
        );
        // NaN-Koordinaten machen die gemessene Distanz zu NaN.
        assert_eq!(
            geometry_implied_displacement_ft(f64::NAN, -3.0, 40.02, -3.02, 13084.0, 0),
            0
        );
    }

    #[test]
    fn ein_geometrisch_unplausibler_versatz_ueber_halber_bahn_wird_verworfen() {
        // Schwelle und Gegenschwelle praktisch am selben Punkt (Rundungs-
        // fehler in Testkoordinaten) taeuschen einen Versatz von fast der
        // ganzen Bahnlaenge vor — das ist kein echter Versatz, sondern
        // ein Datenfehler. Grenze: length_ft/2.
        let ft = geometry_implied_displacement_ft(40.0, -3.0, 40.0, -3.0, 13084.0, 0);
        assert_eq!(ft, 0, "ein Versatz ueber der halben Bahnlaenge ist unplausibel");
    }

    #[test]
    fn csv_source_reports_zero_displacement_for_a_runway_with_none() {
        let m = lookup_runway(EDDP_26R_THR_LAT, EDDP_26R_THR_LON, EDDP_26R_HEADING)
            .expect("should resolve to EDDP/26R");
        assert_eq!(m.displaced_threshold_ft, 0);
    }

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

        let (lat1, lon1) = destination(EDDP_26R_THR_LAT, EDDP_26R_THR_LON, landing_bearing, 1000.0);
        let (lat2, lon2) = destination(lat1, lon1, right_bearing, 10.0);

        let m = lookup_runway(lat2, lon2, EDDP_26R_HEADING).expect("should resolve to EDDP/26R");
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
        let phi2 = (phi1.sin() * delta.cos() + phi1.cos() * delta.sin() * bearing_rad.cos()).asin();
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
        let (lat, lon) = destination(EDDP_26R_THR_LAT, EDDP_26R_THR_LON, approach_bearing, 200.0);
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

    // ─── v0.21.x: EFTP phantom-runway dedupe (Thomas field report,
    // BTI357 EDDH2607) ───────────────────────────────────────────────
    //
    // Real production data pulled from the live navdata DB (AIRAC 2607,
    // 2026-08-06): EFTP genuinely has one physical runway, "06"/"24"
    // (2700 m, matches OurAirports exactly). The Aerosoft-DFD source
    // additionally carries phantom "06C"/"24C" entries — same heading,
    // shorter, threshold ~300-420 m further down the same physical
    // strip. OurAirports has never heard of "06C"/"24C" for EFTP.
    /// EDDL, so wie die Navdaten es wirklich liefern: **ohne Belag**.
    ///
    /// Die Geometrie stammt aus `data/ourairports-runways.csv`, damit der
    /// Treffer sitzt; `surface: None` ist der Punkt. Genau so kommt jede
    /// Bahn aus dem Navdaten-Endpunkt — `nav_runways.surface_code` ist in
    /// allen 85.058 Zeilen leer.
    fn eddl_nav_ohne_belag() -> NavAirport {
        let rwy = |des: &str, tc: f64, t: (f64, f64), e: (f64, f64)| NavRunway {
            designator: des.to_string(),
            magnetic_course: 0.0,
            true_course: tc,
            length_ft: 9842,
            width_ft: Some(148),
            // DER PUNKT: die Navdaten tragen hier nichts.
            surface: None,
            threshold: NavPoint {
                lat: t.0,
                lon: t.1,
                elev_ft: None,
            },
            far_end: NavPoint {
                lat: e.0,
                lon: e.1,
                elev_ft: None,
            },
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: 3.0,
            tch_ft: 50,
        };
        NavAirport {
            cycle: "2604".to_string(),
            valid_to: "2026-09-03".to_string(),
            icao: "EDDL".to_string(),
            name: "Düsseldorf".to_string(),
            latitude: 51.289,
            longitude: 6.767,
            elevation_ft: Some(147),
            runways: vec![
                rwy("05R", 53.0, (51.279598, 6.751990), (51.295898, 6.786220)),
                rwy("23L", 233.0, (51.295898, 6.786220), (51.279598, 6.751990)),
            ],
        }
    }

    /// Der Befund vom ersten Live-Tag: EDDL ohne Belag, keine Querbewertung.
    ///
    /// Die Navdaten liefern `surface: None`, `unwrap_or_default()` machte
    /// daraus den leeren String, und der ergibt `Belag::Unbekannt` →
    /// `surface_unknown`. Betroffen war nicht EDDL, sondern **jeder**
    /// Flughafen in den Navdaten — also praktisch jeder echte Flug.
    ///
    /// Die Angabe lag daneben in der eingebetteten Tabelle: EDDL = `CON`.
    #[test]
    fn eddl_holt_den_belag_aus_der_tabelle_wenn_die_navdaten_keinen_haben() {
        let apt = eddl_nav_ohne_belag();
        // Aufsetzen auf 05R, kurz hinter der Schwelle, auf Bahnkurs.
        let m = lookup_runway_in_nav(51.2805, 6.7535, 53.0, &apt)
            .expect("EDDL 05R muss getroffen werden");
        assert_eq!(m.runway_ident, "05R");
        assert_eq!(
            m.surface, "CON",
            "Die Navdaten tragen keinen Belag; er muss aus der eingebetteten \
             OurAirports-Tabelle kommen. Ohne diesen Rückgriff steht hier der \
             leere String, und die seitliche Bewertung entfällt — bei jedem \
             Flug zu einem Navdaten-Flughafen."
        );
        // Und die Kette bis zur Bewertung.
        let belag = landing_scoring::belag::belag_aus_angabe(Some(&m.surface));
        assert!(
            belag.seitlich_bewertbar(),
            "EDDL ist Beton — die Querbewertung muss laufen"
        );
    }

    /// Die Gegenrichtung: Trägt die Navdaten-Zeile einen Belag, gilt der.
    #[test]
    fn navdaten_belag_hat_vorrang_vor_der_tabelle() {
        let mut apt = eddl_nav_ohne_belag();
        apt.runways[0].surface = Some("GRS".to_string());
        let m = lookup_runway_in_nav(51.2805, 6.7535, 53.0, &apt).expect("Treffer");
        assert_eq!(
            m.surface, "GRS",
            "Ein vorhandener Navdaten-Belag darf nicht überschrieben werden"
        );
    }

    fn eftp_nav_fixture() -> NavAirport {
        let rwy = |des: &str, tc: f64, length_ft: i32, t: (f64, f64), e: (f64, f64)| NavRunway {
            designator: des.to_string(),
            magnetic_course: 0.0,
            true_course: tc,
            length_ft,
            width_ft: Some(148),
            surface: Some("ASP".to_string()),
            threshold: NavPoint {
                lat: t.0,
                lon: t.1,
                elev_ft: None,
            },
            far_end: NavPoint {
                lat: e.0,
                lon: e.1,
                elev_ft: None,
            },
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: 3.0,
            tch_ft: 50,
        };
        NavAirport {
            cycle: "2607".to_string(),
            valid_to: "2026-08-06".to_string(),
            icao: "EFTP".to_string(),
            name: "Tampere-Pirkkala".to_string(),
            latitude: 61.414,
            longitude: 23.604,
            elevation_ft: Some(390),
            runways: vec![
                rwy(
                    "06",
                    64.4049516350735,
                    8858,
                    (61.408922, 23.581586),
                    (61.419369, 23.627208),
                ),
                rwy(
                    "06C",
                    64.4098729375956,
                    7470,
                    (61.410561, 23.588731),
                    (61.418208, 23.622125),
                ),
                rwy(
                    "24",
                    244.445012365508,
                    8858,
                    (61.419369, 23.627208),
                    (61.408922, 23.581586),
                ),
                rwy(
                    "24C",
                    244.439196313664,
                    7871,
                    (61.418208, 23.622125),
                    (61.410561, 23.588731),
                ),
            ],
        }
    }

    #[test]
    fn dedupe_drops_the_phantom_c_variants_eftp_confirms_the_plain_ones() {
        let apt = eftp_nav_fixture();
        let kept = dedupe_near_duplicate_nav_runways(&apt.icao, apt.runways);
        let idents: Vec<&str> = kept.iter().map(|r| r.designator.as_str()).collect();
        assert_eq!(
            idents,
            vec!["06", "24"],
            "06C/24C are phantom entries unknown to OurAirports and must be dropped"
        );
    }

    #[test]
    fn eftp_touchdown_near_the_24c_phantom_threshold_now_matches_real_24() {
        // v0.21.x: dedupe now runs once at the navdata-fetch boundary
        // (`spawn_navdata_fetch` in lib.rs), not inside `lookup_runway_in_nav`
        // itself — so the test mirrors the real pipeline shape: fetch, dedupe,
        // *then* look up, instead of relying on the matcher to clean up after it.
        let mut apt = eftp_nav_fixture();
        apt.runways = dedupe_near_duplicate_nav_runways(&apt.icao, apt.runways);
        // Exact touchdown coordinate from the field report (BTI357,
        // 2026-08-05T22:15:27Z) — sits almost exactly ON the phantom
        // "24C" threshold (61.418208, 23.622125), which is what made the
        // pre-fix matcher pick it over the real "24".
        let m = lookup_runway_in_nav(61.4180153, 23.6209501, 242.77, &apt)
            .expect("touchdown near EFTP should resolve");
        assert_eq!(m.airport_ident, "EFTP");
        assert_eq!(
            m.runway_ident, "24",
            "must match the real runway 24, not the phantom 24C \
             (pre-fix behaviour matched 24C because of its closer XTD)"
        );
        assert_eq!(
            m.length_ft, 8858.0,
            "must carry the real runway's length, not 24C's shorter phantom length"
        );
    }

    #[test]
    fn dedupe_leaves_real_parallel_runways_untouched_when_ourairports_confirms_both() {
        // PAFA-style case: an unsuffixed "20" AND a suffixed "20L" BOTH
        // exist in OurAirports for the same airport (real secondary
        // strip, not a phantom) — dedupe must not remove either.
        let rwy = |des: &str, tc: f64, t: (f64, f64), e: (f64, f64)| NavRunway {
            designator: des.to_string(),
            magnetic_course: 0.0,
            true_course: tc,
            length_ft: 4510,
            width_ft: Some(75),
            surface: Some("ASP".to_string()),
            threshold: NavPoint {
                lat: t.0,
                lon: t.1,
                elev_ft: None,
            },
            far_end: NavPoint {
                lat: e.0,
                lon: e.1,
                elev_ft: None,
            },
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: 3.0,
            tch_ft: 50,
        };
        let runways = vec![
            rwy(
                "20",
                31.0669516863285,
                (64.815556, -147.856389),
                (64.803889, -147.849722),
            ),
            rwy(
                "20L",
                31.0669516863285,
                (64.815, -147.855),
                (64.803, -147.848),
            ),
        ];
        let kept = dedupe_near_duplicate_nav_runways("PAFA", runways);
        assert_eq!(
            kept.len(),
            2,
            "both are real, OurAirports-confirmed runways — dedupe must not touch them"
        );
    }

    #[test]
    fn dedupe_leaves_unverifiable_small_airport_runways_untouched() {
        // Neither designator is known to OurAirports (small/uncovered
        // field) — dedupe can't tell duplicate from real secondary strip,
        // so it must default to leaving the list unchanged.
        let rwy = |des: &str, t: (f64, f64), e: (f64, f64)| NavRunway {
            designator: des.to_string(),
            magnetic_course: 0.0,
            true_course: 90.0,
            length_ft: 2000,
            width_ft: Some(60),
            surface: Some("GRS".to_string()),
            threshold: NavPoint {
                lat: t.0,
                lon: t.1,
                elev_ft: None,
            },
            far_end: NavPoint {
                lat: e.0,
                lon: e.1,
                elev_ft: None,
            },
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: 3.0,
            tch_ft: 50,
        };
        let runways = vec![
            rwy("09", (0.0, 0.0), (0.0, 0.01)),
            rwy("09C", (0.0001, 0.0), (0.0001, 0.01)),
        ];
        let kept = dedupe_near_duplicate_nav_runways("ZZZZ", runways);
        assert_eq!(
            kept.len(),
            2,
            "neither confirmed nor refuted by OurAirports — must not guess"
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
        let m =
            lookup_runway_in_nav(33.809_288, 35.488_861, 355.0, &apt).expect("RWY 35 threshold");
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
        let (m, src) = lookup_runway_with_fallback(33.838_364, 35.486_978, 175.0, Some(&apt))
            .expect("should match Navigraph runway");
        assert_eq!(src, RunwaySource::Navigraph);
        assert_eq!(m.runway_ident, "17");
    }

    /// Real AIRAC 2609 data for LEMD 32L/14R (`tbl_runways`, checked
    /// 06.09.2026). Unlike `olba_nav_fixture` (a synthetic pre-2608
    /// layout with 35's threshold at 17's pavement end), this mirrors
    /// what the live server actually delivers today: both ends' `threshold`
    /// already sit at their own landing threshold, AND the numeric field
    /// is populated — exactly the pattern that broke FDX2/LEMD 32L.
    fn lemd_nav_fixture() -> NavAirport {
        NavAirport {
            cycle: "2609".to_string(),
            valid_to: "2026-10-01".to_string(),
            icao: "LEMD".to_string(),
            name: "Adolfo Suarez Madrid-Barajas".to_string(),
            latitude: 40.472_222,
            longitude: -3.560_833,
            elevation_ft: Some(1998),
            runways: vec![
                NavRunway {
                    designator: "32L".to_string(),
                    magnetic_course: 320.5,
                    true_course: 322.32,
                    length_ft: 13084,
                    width_ft: Some(197),
                    surface: Some("ASP".to_string()),
                    threshold: NavPoint {
                        lat: 40.463_083_33,
                        lon: -3.553_894_44,
                        elev_ft: Some(1909),
                    },
                    far_end: NavPoint {
                        lat: 40.484_861_11,
                        lon: -3.576_011_11,
                        elev_ft: Some(1995),
                    },
                    displaced_threshold_ft: 3045,
                    ils: None,
                    glideslope_angle: 3.0,
                    tch_ft: 50,
                },
                NavRunway {
                    designator: "14R".to_string(),
                    magnetic_course: 140.3,
                    true_course: 142.305,
                    length_ft: 13084,
                    width_ft: Some(197),
                    surface: Some("ASP".to_string()),
                    threshold: NavPoint {
                        lat: 40.484_861_11,
                        lon: -3.576_011_11,
                        elev_ft: Some(1995),
                    },
                    far_end: NavPoint {
                        lat: 40.463_083_33,
                        lon: -3.553_894_44,
                        elev_ft: Some(1909),
                    },
                    displaced_threshold_ft: 0,
                    ils: None,
                    glideslope_angle: 3.0,
                    tch_ft: 50,
                },
            ],
        }
    }

    /// End-to-end regression for the exact bug FDX2 hit on LEMD 32L,
    /// 06.09.2026: a touchdown 376 m past the real landing threshold
    /// (near the aim point — a good landing) must NOT be reported as
    /// 552 m BEFORE the threshold in a forbidden pre-threshold zone.
    #[test]
    fn fdx2_lemd_32l_touchdown_near_aim_point_is_not_a_pre_threshold_violation() {
        let apt = lemd_nav_fixture();
        let bearing_rad = 322.32_f64.to_radians();
        let (lat, lon) = destination(40.463_083_33, -3.553_894_44, bearing_rad, 376.0);
        let m = lookup_runway_in_nav(lat, lon, 322.0, &apt).expect("should resolve to LEMD 32L");
        assert_eq!(m.runway_ident, "32L");
        assert!(
            m.geometry_implied_displaced_threshold_ft > 2900,
            "geometry must recognise LEMD 32L's threshold is already the \
             landing threshold, got {}",
            m.geometry_implied_displaced_threshold_ft
        );
        assert!(
            (m.touchdown_distance_from_threshold_ft * 0.3048 - 376.0).abs() < 5.0,
            "raw along-track distance should be ~376 m, was {} ft",
            m.touchdown_distance_from_threshold_ft
        );

        let stats = crate::FlightStats {
            runway_match: Some(m),
            ..Default::default()
        };
        let assessed = crate::assess_touchdown(&stats);
        let td = assessed
            .td_distance_from_threshold_m
            .expect("runway matched");
        assert!(
            td > 300.0 && td < 450.0,
            "the bug reported this as -552 m (before the threshold); \
             it must show ~376 m past it, got {td:.0} m"
        );
        assert!(
            !assessed.dds.expect("dds classified").in_pre_threshold_zone,
            "a touchdown near the aim point must never be flagged as a \
             pre-threshold/illegal-landing violation"
        );
    }

    #[test]
    fn fallback_uses_ourairports_when_nav_none() {
        // No NavAirport provided → falls back to bundled CSV. EDDP/26R
        // is in OurAirports, so we get a match flagged as fallback.
        let (m, src) =
            lookup_runway_with_fallback(EDDP_26R_THR_LAT, EDDP_26R_THR_LON, EDDP_26R_HEADING, None)
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
        let (thr_07r_lat, thr_07r_lon) = destination(thr_07l_lat, thr_07l_lon, perp_right, 520.0);
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
        let (thr_07r_lat, thr_07r_lon) = destination(50.05, 8.55, right_perp, 520.0);
        // …then 1000 m down 07R…
        let (along_lat, along_lon) = destination(thr_07r_lat, thr_07r_lon, landing_bearing, 1000.0);
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
        let (along_lat, along_lon) = destination(50.05, 8.55, landing_bearing, 1000.0);
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

    // ---- Live-Bahnvorhersage (#msfs-hud) ---------------------------------

    /// Baut eine gerade Nord-Süd-Bahn (Kurs 360°) mit gegebener Schwelle.
    /// Länge 3000 m, damit die „hinter dem Bahnende“-Prüfung greifbar ist.
    fn nav_rw(designator: &str, thr_lat: f64, thr_lon: f64, course: f64, gs: f64) -> NavRunway {
        // Bahnende 3000 m in Kursrichtung — für die Testbreiten reicht die
        // Näherung über Grad-Latitude bzw. -Longitude.
        let (end_lat, end_lon) = if (course - 360.0).abs() < 0.5 || course < 0.5 {
            (thr_lat + 3000.0 / 111_320.0, thr_lon)
        } else {
            // 090° — nach Osten.
            (
                thr_lat,
                thr_lon + 3000.0 / (111_320.0 * thr_lat.to_radians().cos()),
            )
        };
        NavRunway {
            designator: designator.to_string(),
            magnetic_course: course,
            true_course: course,
            length_ft: 9843, // 3000 m
            width_ft: Some(150),
            surface: Some("ASP".into()),
            threshold: NavPoint {
                lat: thr_lat,
                lon: thr_lon,
                elev_ft: None,
            },
            far_end: NavPoint {
                lat: end_lat,
                lon: end_lon,
                elev_ft: None,
            },
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: gs,
            tch_ft: 50,
        }
    }

    /// 6 NM südlich der Schwelle, auf der verlängerten Mittellinie, Kurs 360°.
    const SIX_NM_DEG: f64 = 11_112.0 / 111_320.0;

    #[test]
    fn predicts_the_runway_the_aircraft_is_lined_up_with() {
        let rws = vec![nav_rw("36", 50.0, 8.0, 360.0, 3.0)];
        let p = predict_landing_runway(&rws, 50.0 - SIX_NM_DEG, 8.0, 360.0)
            .expect("aligned final should predict a runway");
        assert_eq!(p.designator, "36");
        assert_eq!(p.glideslope_angle, Some(3.0));
        assert!(
            p.centerline_offset_m.abs() < 50.0,
            "offset {}",
            p.centerline_offset_m
        );
    }

    #[test]
    fn picks_the_parallel_runway_the_aircraft_is_actually_tracking() {
        // Zwei Parallelbahnen, 500 m auseinander. Die Schwellenentfernung
        // ist für beide praktisch gleich — nur der Mittellinien-Abstand
        // trennt sie. Genau dafür ist die Auswahl so gebaut.
        let lon_500m = 500.0 / (111_320.0 * 50.0_f64.to_radians().cos());
        let rws = vec![
            nav_rw("36L", 50.0, 8.0, 360.0, 3.0),
            nav_rw("36R", 50.0, 8.0 + lon_500m, 360.0, 3.0),
        ];
        let on_left = predict_landing_runway(&rws, 50.0 - SIX_NM_DEG, 8.0, 360.0).unwrap();
        assert_eq!(on_left.designator, "36L");
        let on_right =
            predict_landing_runway(&rws, 50.0 - SIX_NM_DEG, 8.0 + lon_500m, 360.0).unwrap();
        assert_eq!(on_right.designator, "36R");
    }

    #[test]
    fn refuses_to_guess_on_downwind_and_base() {
        let rws = vec![nav_rw("36", 50.0, 8.0, 360.0, 3.0)];
        // Gegenanflug: gleiche Position, Kurs 180°.
        assert!(predict_landing_runway(&rws, 50.0 - SIX_NM_DEG, 8.0, 180.0).is_none());
        // Queranflug: Kurs 090°.
        assert!(predict_landing_runway(&rws, 50.0 - SIX_NM_DEG, 8.0, 90.0).is_none());
    }

    #[test]
    fn refuses_to_guess_from_too_far_out_or_too_far_off_centerline() {
        let rws = vec![nav_rw("36", 50.0, 8.0, 360.0, 3.0)];
        // 20 NM raus (Grenze liegt bei 15).
        let twenty_nm_deg = 37_040.0 / 111_320.0;
        assert!(predict_landing_runway(&rws, 50.0 - twenty_nm_deg, 8.0, 360.0).is_none());
        // Auf Höhe 6 NM, aber 3 NM seitlich versetzt (Grenze 2 NM).
        let three_nm_lon = 5_556.0 / (111_320.0 * 50.0_f64.to_radians().cos());
        assert!(
            predict_landing_runway(&rws, 50.0 - SIX_NM_DEG, 8.0 + three_nm_lon, 360.0).is_none()
        );
    }

    #[test]
    fn drops_an_implausible_glideslope_instead_of_passing_it_on() {
        // Navdaten mit 9° — außerhalb der 2–7,5°-Plausibilität. Die Bahn
        // bleibt gültig, nur der Winkel fällt weg; der Aufrufer nimmt dann
        // seinen 3°-Standard.
        let rws = vec![nav_rw("36", 50.0, 8.0, 360.0, 9.0)];
        let p = predict_landing_runway(&rws, 50.0 - SIX_NM_DEG, 8.0, 360.0).unwrap();
        assert_eq!(p.designator, "36");
        assert_eq!(p.glideslope_angle, None);
    }

    #[test]
    fn stops_predicting_once_past_the_far_end() {
        // 500 m hinter dem Bahnende (Bahn ist 3000 m lang).
        let rws = vec![nav_rw("36", 50.0, 8.0, 360.0, 3.0)];
        let past = 50.0 + 3500.0 / 111_320.0;
        assert!(predict_landing_runway(&rws, past, 8.0, 360.0).is_none());
    }

    // ── projiziere_auf_bahn: an echten Bahndaten geprueft ────────────────────

    /// EHAM 06 aus den Navdaten (AIRAC 2608), Schwelle -> Bahnende.
    const EHAM06: (f64, f64, f64, f64) = (52.289106, 4.737225, 52.304350, 4.776925);

    /// Hinweis zur Aussagekraft: Dieser Test allein faengt einen Winkelfehler
    /// NICHT — bei 327 m Abstand schlaegt ein halbes Zehntelgrad nur mit 0,3 m
    /// durch. Gegengeprueft am 23.08.2026 durch Ersetzen der Geometrie-Achse
    /// durch einen festen Kurs von 58,0 Grad: dieser Test blieb gruen, sechs
    /// andere wurden rot (darunter `bahnende_liegt_bei_der_nutzbaren_laenge`,
    /// das ueber die volle Bahnlaenge misst). Die Absicherung traegt also das
    /// Bundel, nicht dieser Fall.
    #[test]
    fn mph9_aufsetzpunkt_trifft_den_gemeldeten_wert() {
        // MPH 9, 22.08.2026. Der Client meldete im Touchdown-Payload
        // td_distance_from_threshold_m = 327,13 und einen Mittellinienversatz
        // von 1,04 m links. Beides muss aus der Geometrie herauskommen.
        let (laengs, quer) = projiziere_auf_bahn(
            EHAM06.0,
            EHAM06.1,
            EHAM06.2,
            EHAM06.3,
            52.290678868045866,
            4.741289635870915,
        );
        assert!(
            (laengs - 327.1).abs() < 2.0,
            "laengs {laengs:.1} m, erwartet ~327 m"
        );
        assert!(
            quer < 0.0 && quer.abs() < 3.0,
            "quer {quer:.2} m — erwartet knapp links (negativ)"
        );
    }

    #[test]
    fn vorzeichen_rechts_ist_positiv() {
        // Punkt 50 m rechts der Achse, 1000 m hinter der Schwelle.
        // EHAM 06 laeuft nach Nordosten (~58 Grad), rechts davon ist Suedosten.
        let kurs = 58.06_f64.to_radians();
        let (lat0, lon0) = (EHAM06.0, EHAM06.1);
        let cosf = lat0.to_radians().cos();
        // 1000 m entlang + 50 m rechts
        let dn = 1000.0 * kurs.cos() + 50.0 * (kurs + std::f64::consts::FRAC_PI_2).cos();
        let de = 1000.0 * kurs.sin() + 50.0 * (kurs + std::f64::consts::FRAC_PI_2).sin();
        let lat = lat0 + dn / 110_540.0;
        let lon = lon0 + de / (111_320.0 * cosf);
        let (laengs, quer) = projiziere_auf_bahn(EHAM06.0, EHAM06.1, EHAM06.2, EHAM06.3, lat, lon);
        assert!((laengs - 1000.0).abs() < 5.0, "laengs {laengs:.1}");
        assert!(quer > 0.0, "rechts muss positiv sein, ist {quer:.2}");
        assert!((quer - 50.0).abs() < 2.0, "quer {quer:.2}, erwartet ~50");
    }

    #[test]
    fn vor_der_schwelle_ist_negativ() {
        // 200 m VOR der Schwelle auf der Achse — muss negativ herauskommen,
        // sonst laesst sich Undershoot nicht von Overshoot unterscheiden.
        let kurs = 58.06_f64.to_radians();
        let lat = EHAM06.0 - 200.0 * kurs.cos() / 110_540.0;
        let lon = EHAM06.1 - 200.0 * kurs.sin() / (111_320.0 * EHAM06.0.to_radians().cos());
        let (laengs, _) = projiziere_auf_bahn(EHAM06.0, EHAM06.1, EHAM06.2, EHAM06.3, lat, lon);
        assert!(
            laengs < 0.0,
            "vor der Schwelle muss negativ sein, ist {laengs:.1}"
        );
        assert!(
            (laengs + 200.0).abs() < 5.0,
            "laengs {laengs:.1}, erwartet ~-200"
        );
    }

    #[test]
    fn bahnende_liegt_bei_der_nutzbaren_laenge() {
        // Der Endpunkt der Achse muss die Bahnlaenge ergeben. Navigraph fuehrt
        // EHAM 06 mit bereits versetzter Schwelle, daher ~3185 m (LDA), nicht
        // die vollen 3439 m. Genau diese Konvention traegt die ganze Bewertung.
        let (laengs, quer) =
            projiziere_auf_bahn(EHAM06.0, EHAM06.1, EHAM06.2, EHAM06.3, EHAM06.2, EHAM06.3);
        assert!(
            (laengs - 3185.0).abs() < 15.0,
            "laengs {laengs:.0} m, erwartet ~3185"
        );
        assert!(
            quer.abs() < 0.5,
            "das Bahnende liegt auf der Achse, quer {quer:.2}"
        );
    }
}

#[cfg(test)]
mod belag_ohne_koordinaten {
    //! Der Belag haengt NICHT an den Koordinaten.
    //!
    //! Befund an GSG1321 (EDBH→EDHE, 25.08.2026): EDHE/Uetersen ist eine
    //! Graspiste, OurAirports fuehrt sie als `GRASS` — und der Bericht
    //! meldete „Belag unbekannt". Ursache: Die EDHE-Zeile hat keine
    //! Koordinaten, und `runways()` verwirft solche Zeilen. Fuer die
    //! Bahnzuordnung richtig, fuer den Belag falsch.
    //!
    //! Ausmass: 32.488 der 48.143 Zeilen haben keine Koordinaten, davon
    //! 32.034 MIT Belagsangabe an 29.520 Flugplaetzen. Zwei Drittel aller
    //! Belagsangaben waren unerreichbar.
    use super::*;

    #[test]
    fn edhe_ist_gras_obwohl_die_zeile_keine_koordinaten_hat() {
        assert_eq!(belag_aus_tabelle("EDHE", "09").as_deref(), Some("GRASS"));
        assert_eq!(belag_aus_tabelle("EDHE", "27").as_deref(), Some("GRASS"));
        // Und die Zeile faellt weiterhin aus der Bahnzuordnung — dort
        // waere sie ohne Punkte auch nutzlos.
        assert_eq!(rows_for_airport("EDHE").count(), 0);
    }

    #[test]
    fn beide_bahnenden_finden_denselben_belag() {
        // Eine Zeile fuehrt beide Enden; der Belag gilt fuer die ganze
        // Bahn. Wer nur ein Ende einträgt, verliert die Haelfte.
        for (icao, a, b) in [("EDDF", "07C", "25C"), ("EDDL", "05R", "23L")] {
            let x = belag_aus_tabelle(icao, a);
            let y = belag_aus_tabelle(icao, b);
            assert!(x.is_some(), "{icao} {a} ohne Belag");
            assert_eq!(x, y, "{icao}: {a} und {b} melden verschiedene Belaege");
        }
    }

    #[test]
    fn kleinschreibung_und_leerzeichen_stoeren_nicht() {
        assert_eq!(belag_aus_tabelle("edhe", " 09 ").as_deref(), Some("GRASS"));
    }

    #[test]
    fn ein_unbekannter_platz_liefert_nichts() {
        assert_eq!(belag_aus_tabelle("XXXX", "09"), None);
        assert_eq!(belag_aus_tabelle("EDHE", "18"), None);
    }

    #[test]
    fn deutlich_mehr_belaege_als_zuordenbare_bahnen() {
        // Die Zahl, um die es geht: Der Belag-Nachschlag muss die
        // koordinatenlosen Zeilen enthalten, sonst ist er nur eine
        // umstaendlichere Fassung des alten Wegs.
        let mit_koordinaten = runways().len();
        let mit_belag = belaege().len();
        assert!(
            mit_belag > mit_koordinaten,
            "nur {mit_belag} Belaege gegen {mit_koordinaten} zuordenbare Bahnen — \
             die koordinatenlosen Zeilen fehlen"
        );
    }

    #[test]
    fn geschlossene_bahnen_bleiben_draussen() {
        // Sonst ueberschreibt eine stillgelegte Bahn den Belag einer
        // offenen mit demselben Namen.
        //
        // ⚠ Geprueft wird gegen die ROHTABELLE, nicht gegen
        // `rows_for_airport`. Die erste Fassung suchte die offene
        // Gegen-Bahn dort — und `rows_for_airport` kennt nur Zeilen MIT
        // Koordinaten, also genau die, um die es hier nicht geht. Sie
        // meldete prompt einen Treffer, den es nicht gab.
        use std::collections::{HashMap, HashSet};
        let mut offen: HashSet<(String, String)> = HashSet::new();
        let mut nur_geschlossen: HashMap<(String, String), String> = HashMap::new();
        let mut rdr = csv::ReaderBuilder::new()
            .has_headers(true)
            .from_reader(RUNWAYS_CSV.as_bytes());
        for record in rdr.records().flatten() {
            let icao = record.get(2).unwrap_or("").trim().to_uppercase();
            if icao.is_empty() {
                continue;
            }
            let zu = record.get(7).unwrap_or("0") == "1";
            let belag = record.get(5).unwrap_or("").trim().to_string();
            for spalte in [8usize, 14usize] {
                let bahn = record.get(spalte).unwrap_or("").trim().to_uppercase();
                if bahn.is_empty() {
                    continue;
                }
                if zu {
                    if !belag.is_empty() {
                        nur_geschlossen.insert((icao.clone(), bahn), belag.clone());
                    }
                } else {
                    offen.insert((icao.clone(), bahn));
                }
            }
        }
        let durchgerutscht: Vec<_> = nur_geschlossen
            .keys()
            .filter(|k| !offen.contains(*k) && belaege().contains_key(*k))
            .take(5)
            .collect();
        assert!(
            durchgerutscht.is_empty(),
            "geschlossene Bahnen im Belag-Nachschlag: {durchgerutscht:?}"
        );
        // Gegenprobe: Es GIBT solche Faelle, der Test laeuft also nicht
        // ins Leere. Gemessen 1.984 Schluessel, die es nur geschlossen gibt.
        let nur_zu = nur_geschlossen
            .keys()
            .filter(|k| !offen.contains(*k))
            .count();
        assert!(
            nur_zu > 1_000,
            "nur {nur_zu} rein geschlossene Bahnen — pruefe die Tabelle"
        );
    }
}
