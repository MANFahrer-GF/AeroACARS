/// One entry in the touchdown ring buffer (see `FlightStats::snapshot_buffer`).
///
/// Carries enough fields for the post-touchdown analyzer (V/S, G,
/// on-ground edge, AGL for bounce detection) and the touchdown profile
/// reconstruction (heading, GS, IAS, lat/lon for sideslip + runway
/// correlation). Sampled at ~30 Hz so a 5-second buffer holds ~150
/// entries — cheap, but rich enough to produce a per-Touchdown-ms
/// V/S curve identical to what BeatMyLanding ships.
// Several fields here aren't read post-Tier-3 refactor — they were
// the inputs to the old `compute_sideslip_at_touchdown` helper which
// we replaced with the native `atan2(VEL_BODY_X, VEL_BODY_Z)` path.
// We keep them in the buffer because the touchdown_profile mapping
// in step_flight reads them when freezing the profile points (see
// `t_ms`, `vs_fpm`, `g_force`, `agl_ft` etc. all consumed there),
// and `lat`/`lon` are kept as a fallback hook in case we ever need
// to derive sideslip from successive positions for a sim that
// doesn't expose body velocity.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Default)]
struct TelemetrySample {
    at: DateTime<Utc>,
    vs_fpm: f32,
    g_force: f32,
    /// Lernpaket AP1a (29.09.2026): MSFS `SEMIBODY LOADFACTOR Y`, nur zum
    /// Vergleich mit `g_force` — geht in keine Note. `None` bei X-Plane.
    g_semibody: Option<f32>,
    on_ground: bool,
    /// AGL altitude — drives bounce detection (35 ft up / 5 ft return,
    /// BeatMyLanding-aligned). Sourced from `altitude_agl_ft` directly.
    agl_ft: f32,
    /// v1.6.9: Hoehe ueber dem Meeresspiegel, parallel zur Hoehe ueber
    /// Grund. Erst beide zusammen trennen den eigenen Sinkflug vom
    /// Gelaende: Bodenhoehe = MSL − AGL, und deren Aenderung ist der
    /// Anteil, den die Bahn selbst zur gemessenen Sinkrate beitraegt.
    /// Gemessen ueber 818 Landungen: im Median 32 fpm, bei 12 % der
    /// Landungen ueber 100 fpm, im Extremfall 451 fpm (K4S2).
    msl_ft: f32,
    /// True heading at the moment of sample. Used at touchdown to
    /// reconstruct ground track from successive lat/lon pairs and
    /// derive sideslip / crab angle.
    heading_true_deg: f32,
    groundspeed_kt: f32,
    indicated_airspeed_kt: f32,
    /// v1.6.3: Eigengeschwindigkeit zum Aufsetzzeitpunkt. Alle anderen
    /// Groessen dieser Struktur werden aus dem Puffer bevorzugt, weil der
    /// Schnappschuss beim Stempeln schon das Ausrollen zeigen kann — das
    /// gilt fuer die Eigengeschwindigkeit genauso.
    true_airspeed_kt: f32,
    lat: f64,
    lon: f64,
    pitch_deg: f32,
    bank_deg: f32,
    /// v0.7.0 — gear_normal_force_n (X-Plane only via DataRef, MSFS None).
    /// MUST-PASS Anchor in Forensik-v2 X-Plane Validation (Spec Sektion 4.1).
    gear_normal_force_n: Option<f32>,
    /// v0.7.0 — total_weight_kg fuer mass-aware gear-force threshold
    /// (3% × static weight mit 1000N Floor) und Replay-Determinismus.
    total_weight_kg: Option<f32>,
    /// v1.7.36: Tankstand zum Zeitpunkt der Probe.
    ///
    /// Er hat hier gefehlt, und `landing_fuel_kg` war deshalb das EINZIGE
    /// Landefeld, das roh aus dem Live-Schnappschuss kam — waehrend Neigung,
    /// Kurs, Geschwindigkeit und Position alle bewusst aus dem Puffer
    /// gelesen werden, weil der Schnappschuss beim Stempeln schon das
    /// Ausrollen zeigt (im Mittel 0,27 s danach, im schlechtesten Prozent
    /// ueber vier Sekunden).
    ///
    /// Das war lange ein Promille auf 16 770 kg und darum verschmerzbar.
    /// Mit dem Rollsprit nach der Landung als eigener Phase ist es das
    /// nicht mehr: Diese Phase liegt im Median bei 90 kg, dort sind 20 kg
    /// Versatz ueber 20 % — und zwar Sprit, der im Umkehrschub verbrannt
    /// wurde und dem Anflug zugeschlagen wuerde, obwohl das Flugzeug
    /// laengst am Boden war.
    fuel_kg: f32,
}

// Maximum age of any entry in the touchdown ring buffer. 5 s gives
// us roughly the last final-approach segment plus the touchdown
// itself. Longer than ~6 s and we'd start picking up Cruise data;
// shorter than ~3 s and we'd miss the descent rate moments before
// flare.
// Wert seit v3: bundle/grenzwerte/landung.json -> touchdown_buffer_secs (v2.1.24: 5)

// v0.5.39: Wie lange nach dem TD-Edge wir post-TD weiter sampeln + die
// Streamer-Network-IO pausiert lassen. 10 s deckt typische Rollout-
// Phase + erste Reverse/Brake-Action ab. Pre-TD-Buffer (5 s) + dies
// ergibt einen TouchdownWindow von ~15 s, gedumpt als ein Event.
// Wert seit v3: bundle/grenzwerte/landung.json -> touchdown_post_window_ms (v2.1.24: 10_000)

/// v1.7.24 (GAF-707-Race, strukturell geloest — Vereinheitlichung 09/2026):
/// Ab wann `finalize_landing_score_if_due` (und, mit demselben Wert,
/// `announce_landing_score`) aufgibt auf die score-relevanten Sampler-
/// Rohdaten (`touchdown_window_score_data_ready_at`) zu warten und
/// stattdessen mit der vorhandenen Kanonik scort/ankuendigt. Gemessen ab
/// `sampler_touchdown_at`, nicht ab `landing_at` — deckt `TOUCHDOWN_POST_
/// WINDOW_MS` (10s) plus grosszuegige Marge fuer einen verspaeteten
/// Streamer-Tick ab. Ein gemeinsamer Name statt zwei getrennter lokaler
/// Konstanten, damit beide Funktionen nie auseinanderlaufen koennen.
const SAMPLER_DUMP_WAIT_TIMEOUT_MS: i64 = 12_000;

/// Wieviele Telemetrieproben die Replay-Erkennung vorhaelt. Bei der
/// langsamsten Kadenz (3 s im Reiseflug) sind das rund drei Minuten — weit
/// mehr als das Verfahren braucht, und immer noch winzig.
const REPLAY_PROBEN_MAX: usize = 64;

// ---- In-App-Live-Map: geflogener Track (Backend-Akkumulation) ----
//
// PARITÄT mit dem Frontend: Diese drei Werte spiegeln client/src/lib/
// trackStore.ts EXAKT (AIR_MIN_DELTA_DEG / GROUND_MIN_DELTA_DEG / MAX_POINTS).
// Der Streamer akkumuliert den Track jetzt im Backend bei voller Tick-Rate
// (fokus-/fenster-unabhängig → lückenlos auch bei X-Plane-Vollbild); das
// Frontend pollt ihn via `flight_get_track`. Ändert sich hier eine Schwelle,
// MUSS sie in trackStore.ts mitgezogen werden, sonst weicht die gepollte Linie
// von der alten in-memory-Linie ab.

/// Ausdünn-Schwelle (Grad) in der LUFT (~0,002° ≈ 220 m) — Langstrecke
/// überlädt die Linie sonst. Spiegelt AIR_MIN_DELTA_DEG in trackStore.ts.
const TRACK_AIR_MIN_DELTA_DEG: f64 = 0.002;
/// Deutlich feinere Schwelle am BODEN (~0,00015° ≈ 16 m), damit der Taxi-/
/// Rollweg nachvollziehbar bleibt. Spiegelt GROUND_MIN_DELTA_DEG in trackStore.ts.
const TRACK_GROUND_MIN_DELTA_DEG: f64 = 0.00015;
/// Sicherheitskappe pro Flug (älteste Punkte fallen raus). Spiegelt MAX_POINTS
/// in trackStore.ts.
const MAX_TRACK_POINTS: usize = 5000;

/// Einen geflogenen Track-Punkt (lon, lat) in `stats.track` aufnehmen — mit
/// derselben Ausdünn-Logik wie das Frontend (`recordTrackPoint` in
/// client/src/lib/trackStore.ts). No-op bei nicht-endlichen Koordinaten.
///
/// `on_ground` wählt die Schwelle: am Boden fein (Taxi-Weg sichtbar), in der
/// Luft grob (Langstrecke bleibt leicht). Ein Punkt wird aufgenommen, wenn es
/// noch keinen letzten gibt ODER |Δlon| bzw. |Δlat| die Schwelle überschreitet.
/// Bei Überlauf werden die ÄLTESTEN Punkte gedrained (Kappe MAX_TRACK_POINTS).
fn record_track_point(stats: &mut FlightStats, lon: f64, lat: f64, on_ground: bool) {
    if !lon.is_finite() || !lat.is_finite() {
        return;
    }
    let min_delta = if on_ground {
        TRACK_GROUND_MIN_DELTA_DEG
    } else {
        TRACK_AIR_MIN_DELTA_DEG
    };
    let push = match stats.track.last() {
        None => true,
        Some([last_lon, last_lat]) => {
            (last_lon - lon).abs() > min_delta || (last_lat - lat).abs() > min_delta
        }
    };
    if push {
        stats.track.push([lon, lat]);
        if stats.track.len() > MAX_TRACK_POINTS {
            let excess = stats.track.len() - MAX_TRACK_POINTS;
            stats.track.drain(0..excess);
        }
    }
}

/// Wie lange die Bildrate je Abtastung aufbewahrt wird, in Sekunden.
///
/// Muss das ganze Aufsetzfenster abdecken: den Vorlauf aus
/// `snapshot_buffer` plus `TOUCHDOWN_POST_WINDOW_MS` danach, mit Reserve.
const BILDRATE_PUFFER_SECS: i64 = 30;

/// Mindestens, Median und Hoechstwert der Bildrate, mit Anzahl — oder
/// nichts, wenn der Simulator keine einzige geliefert hat.
fn bildrate_zusammenfassung(werte: impl Iterator<Item = f32>) -> Option<(f32, f32, f32, usize)> {
    let mut v: Vec<f32> = werte.filter(|hz| hz.is_finite() && *hz > 0.0).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f32::total_cmp);
    Some((v[0], v[v.len() / 2], v[v.len() - 1], v.len()))
}

/// Die eine Zeile „Bildrate beim Aufsetzen" im Diagnoselog (2.0.14).
///
/// Sie erklaert, warum bei einer Landung wenige neue Messwerte ankommen:
/// Rechnet der Simulator nur 25 Bilder je Sekunde, kann er nicht oefter
/// liefern. Fehlt die Bildrate ganz, steht auch das da — sonst faellt eine
/// stille Verdrahtungsluecke erst Wochen spaeter auf.
fn bildrate_protokollieren(
    pirep_id: &str,
    edge_at: DateTime<Utc>,
    bildrate: Option<(f32, f32, f32, usize)>,
) {
    match bildrate {
        Some((min, median, max, n)) => tracing::info!(
            pirep_id,
            %edge_at,
            n,
            min_hz = format!("{min:.1}"),
            median_hz = format!("{median:.1}"),
            max_hz = format!("{max:.1}"),
            "Bildrate beim Aufsetzen"
        ),
        None => tracing::info!(
            pirep_id,
            %edge_at,
            "Bildrate beim Aufsetzen: Simulator liefert keine"
        ),
    }
}

#[cfg(test)]
#[path = "tests_lib/bildrate_tests.rs"]
mod bildrate_tests;

impl From<TelemetrySample> for TouchdownWindowSample {
    fn from(s: TelemetrySample) -> Self {
        Self {
            at: s.at,
            vs_fpm: s.vs_fpm,
            g_force: s.g_force,
            g_semibody: s.g_semibody,
            on_ground: s.on_ground,
            agl_ft: s.agl_ft,
            msl_ft: Some(s.msl_ft),
            heading_true_deg: s.heading_true_deg,
            groundspeed_kt: s.groundspeed_kt,
            indicated_airspeed_kt: s.indicated_airspeed_kt,
            true_airspeed_kt: s.true_airspeed_kt,
            lat: s.lat,
            lon: s.lon,
            pitch_deg: s.pitch_deg,
            bank_deg: s.bank_deg,
            // v0.7.0 — Forensik-v2 fields propagated to JSONL
            gear_normal_force_n: s.gear_normal_force_n,
            total_weight_kg: s.total_weight_kg,
        }
    }
}

/// One frozen subsample around the touchdown moment, surfaced in the
/// PIREP notes block as a tiny V/S / G curve. Modelled after
/// BeatMyLanding's `LandingTouchdownProfilePoint`. Times are in ms
/// relative to the on-ground edge — negative = before, positive =
/// after.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct TouchdownProfilePoint {
    /// Milliseconds relative to the touchdown edge. Range roughly
    /// −5000..+0 in the captured slice (only the buffer's history).
    t_ms: i32,
    vs_fpm: f32,
    g_force: f32,
    agl_ft: f32,
    on_ground: bool,
    heading_true_deg: f32,
    groundspeed_kt: f32,
    indicated_airspeed_kt: f32,
    pitch_deg: f32,
    bank_deg: f32,
}

/// v0.4.1: Snapshot der letzten bekannten Sim-Werte zum Zeitpunkt
/// als der Streamer den Sim-Disconnect detektiert hat. Gezeigt im
/// Cockpit-Banner + Activity-Log + bei Bedarf für Reposition.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PausedSnapshot {
    pub lat: f64,
    pub lon: f64,
    pub heading_deg: f32,
    pub altitude_ft: f64,
    pub fuel_total_kg: f32,
    pub zfw_kg: Option<f32>,
    /// v0.20 (Process-Integrity, QS-Fix): needed so the resume-discontinuity
    /// check can tell "impossible mid-air fuel increase" apart from "normal
    /// ground refuel that happened to straddle a pause/restart" — see
    /// `is_impossible_discontinuity`. `#[serde(default)]` → `false` for
    /// pre-v0.20 persisted files (safe: falls back to the stricter
    /// pre-fix behavior for old data instead of silently exempting it).
    #[serde(default)]
    pub on_ground: bool,
}

/// Spec sim-disconnect-auto-resume F2 (Pause-Akkumulator):
/// Grund warum der Flug pausiert war.
///
/// Heute existiert nur `SimDisconnect` (Snapshot=None > 30 s). Im
/// v0.3-Master-Spec waren `SimPause` (MSFS-Esc-Pause via SimConnect-
/// Event) und `Replay` (X-Plane-Plugin-Heartbeat) geplant — die sind
/// im MVP bewusst out-of-scope, koennen aber spaeter ohne Daten-
/// migration nachgereicht werden (alle Reasons landen im selben
/// Akkumulator).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum PauseReason {
    SimDisconnect,
    /// Spec v0.7.15 F5/F6: Simulator-eigene Pause (MSFS-Esc-Pause oder
    /// X-Plane `sim/time/paused`). SimConnect / Plugin liefert weiterhin
    /// (eingefrorene) Snapshots — der Disconnect-Pfad wuerde sonst nie
    /// triggern. Verhalten beim Resume identisch zu SimDisconnect:
    /// `apply_pause_resume` schliesst den Block + akkumuliert.
    SimPause,
    ManualResume,
}

/// Spec sim-disconnect-auto-resume F2 (Pause-Akkumulator):
/// Ein abgeschlossener Pause-Block. Wird beim Resume erzeugt und in
/// `FlightStats.pause_segments` angehaengt. `pause_total_duration_secs`
/// haelt die Summe parallel — Segmente sind fuer Audit/Debugging.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PauseSegment {
    started_at: DateTime<Utc>,
    ended_at: DateTime<Utc>,
    duration_secs: i64,
    reason: PauseReason,
    /// Reposition-Distanz (NM) zwischen `paused_last_known` und dem
    /// ersten Post-Resume-Snapshot. `None` wenn entweder kein
    /// `paused_last_known` vorhanden war (frueher Disconnect ohne
    /// guten Snapshot) oder beim Resume kein Snapshot kam.
    #[serde(default)]
    drift_nm: Option<f64>,
    #[serde(default)]
    altitude_delta_ft: Option<f64>,
    #[serde(default)]
    fuel_delta_kg: Option<f64>,
}

#[derive(Default)]
struct FlightStats {
    /// v3 (P-4.11, P-7.02): Aufsetzfenster der letzten Landung (17 Kanaele)
    /// fuer das Eingabe-Dokument an live. Nur im Speicher.
    landung_fenster: Vec<TouchdownWindowSample>,
    /// v3 (P-4.13): Eingabe, Fenster und Begleitung (meta, Herkunft P-9.44,
    /// roh — Kap. 5.2) jedes Touch-and-Go, eingefroren VOR dem Zuruecksetzen
    /// der Landungsfelder; beim Abgeben je ein Dokument (seq 1 …), die letzte
    /// Landung bekommt die naechste Nummer.
    aufsetzer_eingefroren: Vec<(landing_scoring::LandingScoringInput, Vec<TouchdownWindowSample>, landungs_eingabe::Begleitung)>,
    /// v3 (Kap. 5.2): Simulator, Simfassung, Profil und Kategorie beim
    /// Aufsetzen (`stamp_touchdown_metadata`), fuer `meta` des
    /// Eingabe-Dokuments. Nur Dokumentation, nie in der Rechnung.
    landing_sim_meta: Option<serde_json::Value>,
    /// v3 (Kap. 5.2): Die beim Aufsetzen verbrauchte gemessene Vref
    /// (`vref_gemessen.take()`) mit Quelle — fuer `herkunft.vref` des
    /// Eingabe-Dokuments. Die Rechnung liest sie hier nicht.
    landing_vref_gemessen: Option<(f32, &'static str)>,
    /// v1.6.12 — die letzten Telemetrieproben fuer die Replay-Erkennung.
    ///
    /// Der Aufsetzpuffer taugt dafuer nicht: er reicht nur 2,5 s vor den
    /// Bodenkontakt, das Verfahren braucht ein laengeres Fenster. Hier
    /// laeuft es mit, solange geflogen wird.
    replay_proben: std::collections::VecDeque<replay_erkennung::ReplayProbe>,
    /// Steht der Sim gerade im Verdacht, eine Aufzeichnung abzuspielen?
    /// Wird wie eine Pause behandelt — dieselbe Entscheidung wie im
    /// X-Plane-Adapter fuer `sim/time/is_in_replay`.
    replay_verdacht: bool,
    // Position tracking.
    /// Muster, das in `aircraft_limits_for` fehlte — fuer den Health-Report.
    /// Siehe `ClientHealthReport::unknown_aircraft_icao`.
    unbekanntes_muster: Option<String>,
    /// Das aufgeloeste Flugzeugmuster — EINMAL bestimmt, ueberall gelesen.
    ///
    /// Die MQTT-Pfade haben keinen Sim-Schnappschuss zur Hand und kamen
    /// deshalb nur an die Buchung. Bei EWG248 war die leer, und mit ihr
    /// fielen Spurweite, Spannweite und Randabstand aus — obwohl der
    /// Flugzeugtitel dagewesen waere. Siehe `sim_core::muster_aufloesen`.
    aufgeloestes_muster: Option<String>,
    /// Bordbuch (26.09.2026) — Laufzustand der Cockpit-Routinen. Nur das
    /// Bordbuch liest ihn; Landebewertung und PIREP nie (Schutztest
    /// `bordbuch_beeinflusst_weder_pirep_noch_landung`).
    bordbuch: bordbuch::Zustand,
    /// Kleinste und groesste Klappenstellung des ganzen Fluges. Zusammen sagen
    /// sie, ob sich der Kanal ueberhaupt bewegt hat und wo das Raster dieses
    /// Musters endet — beides braucht die Landekonfiguration am 1000-ft-Tor
    /// (siehe `KonfigKontext`).
    flaps_min_gesehen: Option<f32>,
    flaps_max_gesehen: Option<f32>,
    /// Lief die Spanne oben schon, BEVOR das Flugzeug abgehoben ist?
    ///
    /// Nur dann heisst „hat sich nie bewegt" auch wirklich „der Kanal ist
    /// tot". Nach einem App-Neustart faengt die Spanne bei Null an (die
    /// beiden Felder stehen bewusst nicht in `PersistedFlightStats` — ein
    /// gespeicherter Extremwert waere eine Behauptung ueber Ticks, die
    /// dieser Prozess nie gesehen hat). Wer bei 1500 ft im Endanflug
    /// neustartet und die Klappen schon gesetzt hat, haette sonst eine
    /// korrekte Landekonfiguration verloren: min == max, also „tot", also
    /// nicht bewertbar statt bestanden.
    flaps_lueckenlos_beobachtet: bool,
    last_lat: Option<f64>,
    last_lon: Option<f64>,
    /// Zeitpunkt der Distanz-Basislinie — ohne ihn laesst sich ein Sprung
    /// nicht von einem Flug unterscheiden. Siehe `segment_is_flown`.
    last_pos_at: Option<DateTime<Utc>>,
    /// Sim-Rate der vorigen Probe fuer die Streckenzaehlung. Nicht
    /// persistiert: Nach einem Neustart gilt fuer die erste Probe die
    /// aktuelle Rate (Default 0 verliert gegen sie im `max`), meist ist sie
    /// wegen der Zeitluecke ohnehin eine Luecke.
    last_sim_rate: f32,
    distance_nm: f64,
    position_count: u32,

    /// Ausgedünnter geflogener Track [lon, lat] für die In-App-Live-Map.
    ///
    /// Wird im Position-Streamer bei dessen VOLLER Tick-Rate akkumuliert —
    /// fenster-/fokus-unabhängig, also lückenlos auch wenn das AeroACARS-
    /// Fenster im Hintergrund liegt (X-Plane Vollbild). Der Webview-Poll
    /// (`setInterval`) holte den Track früher aus dem gedrosselten Snapshot-
    /// Stream auf → im Hintergrund entstanden Lücken. Hier ist das Backend die
    /// Quelle der Wahrheit; das Frontend spiegelt ihn nur noch via
    /// `flight_get_track`. Ausgedünnt + gekappt durch `record_track_point` /
    /// MAX_TRACK_POINTS. Runtime-only — FlightStats ist NICHT Serialize, das
    /// Feld wird nicht persistiert und initialisiert via Default leer.
    track: Vec<[f64; 2]>,

    // ---- Phase-FSM state ----
    /// Current flight phase. Starts at Boarding when flight_start fires.
    phase: FlightPhase,
    /// Recent transitions for the flight log.
    transitions: Vec<(DateTime<Utc>, FlightPhase)>,

    // ---- v0.16.12 (#phase-v2): Schatten-Phasen-Engine ----
    /// Fenster-basierte Phasen-Engine v2 — läuft NUR im Schatten-Modus
    /// mit (beobachtet, entscheidet nichts). Lebt pro Flug in den Stats
    /// (Reset = frische FlightStats beim Flugstart). Runtime-only: nach
    /// einem App-Restart wärmt sie sich binnen ≤ 2 Fenstern (~3 min)
    /// wieder auf — Persistenz bewusst nicht Teil von Runde 1.
    shadow_engine: phase_v2::ShadowPhaseEngine,
    /// Letzte Schatten-Phase (für UI-Statuszeile + flight_info). Runtime-only.
    shadow_phase: Option<FlightPhase>,
    /// Letztes Schatten-Segment (z. B. Level während einer Restriction).
    /// Runtime-only.
    shadow_segment: Option<phase_v2::Segment>,
    /// v0.19.1: letzte EFFEKTIVE (v2-bevorzugte) Phase, für die eine ACARS-
    /// "Phase: …"-Textlog-Zeile + der retained MQTT-Phase-Publish bereits
    /// abgesetzt wurden. Vorher waren beide rein an v1-Transitions
    /// (`phase_change`) gekoppelt — bei einer FSM, die (Feld-Fund GSG22
    /// EDLN→EDDL) auf einer En-Route-Phase hängen bleibt, obwohl v2 anhand
    /// echter Kinematik längst weiter ist, blieben Log + Live-Status-Topic
    /// dadurch stumm/veraltet, obwohl andere v2-gespeiste Pfade (Positions-
    /// Payload, phpVMS-Heartbeat, Tray, Cockpit-UI) längst korrekt liefen.
    /// Jetzt Trigger = "die angezeigte Phase hat sich geändert", nicht
    /// "v1 hat intern transitioniert" — deckt beide Fälle ab und vermeidet
    /// Doppel-Log wenn v1 und v2 im selben Tick gemeinsam wechseln.
    /// Runtime-only (kein Persistenz-Bedarf: nach Resume einfach neu
    /// gesetzt beim nächsten Tick, kostet höchstens eine harmlose
    /// Doppel-Zeile).
    last_logged_effective_phase: Option<FlightPhase>,
    /// v0.16.12 (#phase-v2): geplante Cruise-Altitude in ft als
    /// `cruise_ref` der Schatten-Engine. Quelle: `bid.flight.level`
    /// (phpVMS-Flight, FL→ft normalisiert) bzw. `plan.cruise_level_ft`
    /// im Manual-Mode. None wenn die VA das Feld nicht pflegt — die
    /// Engine fällt dann auf Dauer-Heuristiken zurück.
    planned_cruise_alt_ft: Option<f64>,
    /// Snapshot of the previous tick — used to detect on_ground / parking
    /// brake transitions cleanly.
    was_on_ground: Option<bool>,
    was_parking_brake: Option<bool>,

    // ---- v0.12.1 (Stream A) — Boarding→Pushback de-glitch state ----
    /// Reference position captured on the first Boarding tick (or after a
    /// reload gap). The Boarding→Pushback/TaxiOut trigger requires the
    /// aircraft to have physically moved away from this point — a velocity
    /// glitch can fake `groundspeed_kt` but not `lat`/`lon`. Runtime-only.
    boarding_ref_pos: Option<(f64, f64)>,
    /// True if the previous snapshot already had `groundspeed_kt > 0.5`.
    /// The pushback trigger needs movement sustained across 2 ticks, not
    /// a single spike. Runtime-only.
    prev_gs_over_threshold: bool,
    /// Wall-clock time of the previous processed `step_flight` snapshot.
    /// A long gap = sim reload/reposition → the next snapshot is
    /// glitch-suspect (LE2). Runtime-only.
    last_step_at: Option<DateTime<Utc>>,

    // ---- Block / takeoff / landing timestamps (real-time UTC) ----
    block_off_at: Option<DateTime<Utc>>,
    takeoff_at: Option<DateTime<Utc>>,
    landing_at: Option<DateTime<Utc>>,
    block_on_at: Option<DateTime<Utc>>,

    /// First tick at which the universal "we're done" fallback saw all
    /// the conditions satisfied (on-ground, engines off, within 2 nmi
    /// of arrival) — shared by both `engines_off_path` (dwell:
    /// `ARRIVED_FALLBACK_DWELL_SECS`) and `standstill_path` (dwell:
    /// `ARRIVED_STANDSTILL_DWELL_SECS`; `engine_count_unreliable` addons
    /// only). Once now − this ≥ the active path's dwell, the FSM jumps to
    /// Arrived regardless of prior phase. Cleared the moment `conditions_
    /// basic` (either path) stops being true — NOT reset just because the
    /// *which* path is active flips tick to tick, so an aircraft that
    /// occasionally satisfies both paths doesn't have its accumulated
    /// dwell time wiped out by the switch. Powers helicopter flights,
    /// short hops, and emergency landings near destination.
    arrived_fallback_pending_since: Option<DateTime<Utc>>,

    /// Detected divert situation — populated once when the FSM reaches
    /// Arrived OR the universal fallback fires AND the aircraft is
    /// >= `DIVERT_DETECT_RADIUS_NM` from the planned `arr_airport`.
    /// Surfaced via `flight_status` so the cockpit can render a
    /// "Divert detected" banner and let the pilot file with the
    /// correct `arr_airport_id`. None when we landed at the planned
    /// destination as expected.
    divert_hint: Option<DivertHint>,

    /// v0.4.1: Sim-Disconnect-Pause-State.
    ///
    /// Wenn der Simulator mid-flight wegbricht (Crash, Quit, FPS auf 0,
    /// SimConnect/UDP timeout) und mehr als `SIM_DISCONNECT_THRESHOLD_S`
    /// Sekunden ohne brauchbaren Snapshot vergangen sind, friert der
    /// Streamer den Flug ein und wartet auf manuellen Resume-Klick.
    ///
    /// Während Pause:
    ///   * keine Position-Posts mehr an phpVMS (sonst Stale Data)
    ///   * Heartbeat (`/update`) läuft weiter — sonst killt phpVMS' cron
    ///     den PIREP nach `acars.live_time` (~2h)
    ///   * Phase-FSM friert auf dem Stand vor Disconnect
    ///   * Activity-Log + Discord-Webhook deaktiviert für die Dauer
    ///
    /// Beim Disconnect-Übergang loggen wir die letzte bekannte Position
    /// (Lat/Lon/HDG/Alt/Fuel/ZFW) damit der Pilot weiß wohin er nach
    /// dem Sim-Restart re-positionieren soll. Kein 5-NM-Restriction wie
    /// bei smartCARS — der Pilot entscheidet wo er wieder einsteigt.
    paused_since: Option<DateTime<Utc>>,
    /// v1.7.35 Sprit-Auswertung — Messwerte, die nur im Flug entstehen.
    ///
    /// Tankstand am *Vergleichspunkt*: dort, wo die Grosskreis-Reststrecke
    /// zum Ziel erstmals unter die geplante Reststrecke ab TOD faellt
    /// (`sprit_plan_rest_nm`). Ab hier beginnt „Sinkflug + Anflug" — die
    /// Phase, die die Flugsicherung bestimmt. Einmal gesetzt, bleibt es.
    sprit_vergleichspunkt_kg: Option<f32>,
    /// Odometer (`distance_nm`) am Vergleichspunkt — daraus die geflogene
    /// Anflugstrecke.
    sprit_vergleichspunkt_odo_nm: Option<f64>,
    /// v1.7.40: Tankstand beim Überflug je Wegpunkt des Navlogs, in der
    /// Reihenfolge des Überflugs. Siehe `sprit_wegpunkt_tick`.
    sprit_wp_messungen: Vec<SpritWpMessung>,
    /// Der Routenabschnitt (Index des Start-Fixes), auf den das Flugzeug
    /// zuletzt projiziert wurde. Wächst nur — eine Warteschleife führt
    /// nicht zurück.
    sprit_wp_segment: Option<usize>,
    /// Geplante Reststrecke ab TOD (Summe der Navlog-Segmente nach dem
    /// TOD-Fix). `None`, wenn das Navlog die Felder nicht traegt.
    sprit_plan_rest_nm: Option<f64>,
    /// Geplanter Verbrauch bis TOD (Navlog `fuel_totalused` am TOD-Fix).
    sprit_plan_bis_tod_kg: Option<f32>,
    /// Sekunden unterhalb der Schwelle (Platzhoehe + 8 000 ft, gedeckelt
    /// unter der geplanten Reiseflughoehe) — nach dem Vergleichspunkt, nur
    /// in der Luft, ohne Pausen, ohne Neustart-Luecken.
    sprit_zeit_unter_schwelle_s: f64,
    /// Die verwendete Schwelle (ft MSL), zur Anzeige.
    sprit_schwelle_ft: Option<f64>,
    /// Erst `true`, wenn das Flugzeug einmal WEITER vom Ziel weg war als die
    /// geplante Reststrecke ab TOD. Ohne das schnappt der Vergleichspunkt bei
    /// v1.7.36: Was der Plan am gemessenen Vergleichspunkt vorsah —
    /// Verbrauch bis dorthin und Reststrecke ab dort. Beides am ORT
    /// bestimmt (`sprit_plan_am_ort`), nicht am TOD-Fix abgelesen.
    sprit_plan_bis_vergleichspunkt_kg: Option<f32>,
    sprit_plan_rest_ab_vergleichspunkt_nm: Option<f64>,
    /// v1.7.36: Tankstand, als die Triebwerke anliefen, und als sie
    /// ausgingen. An ein Ereignis gerastet — siehe `sprit_boden_marken`.
    engine_start_fuel_kg: Option<f32>,
    engine_off_fuel_kg: Option<f32>,
    /// Seit wann die Triebwerke nach der Landung durchgehend aus sind —
    /// Entprellung fuer `engine_off_fuel_kg`. Fluechtig: Ein Neustart
    /// faengt die Frist neu an, das kostet hoechstens zehn Sekunden.
    sprit_triebwerke_aus_seit: Option<DateTime<Utc>>,
    /// Hoechste bisher erreichte Hoehe (ft MSL) — Rueckfall fuer den
    /// Sinkflug-Test ohne Plan-Reiseflughoehe.
    sprit_hoechste_hoehe_ft: Option<f64>,
    /// v1.7.36: Der Navlog-Trip, AM MESSPUNKT eingefroren — zusammen mit
    /// dem Plan-Anteil bis dorthin. Wird die Route danach noch ersetzt
    /// (Route-Sync, SimBrief-Refresh im Anflug), kaeme der Trip sonst aus
    /// dem neuen Plan und der Anteil aus dem alten (QS-Befund E3).
    sprit_plan_trip_am_vergleichspunkt_kg: Option<f32>,
    /// v1.7.36: Die geflogene Strecke beim Aufsetzen. Die Anflugstrecke
    /// endet dort — der Wegzaehler laeuft am Boden weiter, und jede spaetere
    /// Rechnung haette sonst das Rollen zum Stand mitgezaehlt (QS-Befund E2).
    sprit_landung_odo_nm: Option<f64>,
    /// v1.7.36: Die Aufzeichnung begann in der Luft (Airborne-Rescue hat den
    /// Abhebe-Tankstand gesetzt), und der Plan-Verbrauch bis zu diesem Ort.
    /// Siehe `SpritEingang::einstieg_in_der_luft`.
    sprit_einstieg_in_der_luft: bool,
    sprit_einstieg_ort: Option<(f64, f64)>,
    sprit_plan_bis_einstieg_kg: Option<f32>,
    /// Das Ergebnis — einmal beim Abschluss der Landung gerechnet.
    sprit_auswertung: Option<landing_scoring::sprit::SpritAuswertung>,
    /// v1.7.35: OFP-Sprit-Leiter (Taxi, Contingency, Alternate, Extra) —
    /// bisher nur im Bid-Vorschau-DTO, jetzt auch im Flug, damit die
    /// Auswertung die Leiter zeigen kann.
    planned_taxi_kg: Option<f32>,
    planned_contingency_kg: Option<f32>,
    planned_alternate_burn_kg: Option<f32>,
    planned_extra_kg: Option<f32>,
    /// Snapshot der letzten Werte zum Pause-Zeitpunkt — wird in der UI
    /// + Activity-Log angezeigt damit der Pilot weiß was er für die
    /// Repositionierung braucht. None solange der Flug nicht pausiert.
    paused_last_known: Option<PausedSnapshot>,
    /// Spec v0.7.15 F5/F6: warum ist der Flug gerade pausiert?
    /// Wird beim Pause-Start gesetzt, vom Resume-Helper im PauseSegment
    /// gespeichert und beim Resume zurueck auf None gesetzt. None
    /// solange `paused_since` None ist.
    current_pause_reason: Option<PauseReason>,
    /// v0.20 (Process-Integrity): Ergebnis des `sim_process_alive()`-Checks
    /// (sim-core::process_probe) zum Zeitpunkt, als `SimDisconnect` erkannt
    /// wurde — beantwortet "ist der Sim-Prozess selbst weg, oder läuft er
    /// noch (nur die Datenverbindung ist tot)?". Wie `current_pause_reason`
    /// beim Resume zurueckgesetzt; überlebt einen App-Neustart über
    /// `PersistedFlightStats`, damit `try_resume_flight()` es ebenfalls sieht.
    disconnect_sim_liveness: Option<sim_core::process_probe::ProcessLiveness>,
    /// v0.20 (Process-Integrity): siehe PersistedFlightStats-Feld gleichen Namens.
    last_persisted_snapshot: Option<PausedSnapshot>,
    /// v0.20 (Process-Integrity): siehe PersistedFlightStats-Feld gleichen Namens.
    resume_discontinuity: Option<ResumeDiscontinuity>,
    /// Alle Spruenge dieses Fluges — siehe PersistedFlightStats.
    resume_spruenge: Vec<ResumeDiscontinuity>,
    /// Ein Auto-File-Versuch laeuft oder lief schon. Verhindert, dass die
    /// wiederholte Pruefung in `Arrived` zweimal einreicht.
    auto_file_gestartet: bool,
    /// v0.20 (Process-Integrity): siehe PersistedFlightStats-Feld gleichen Namens.
    app_restart_was_unclean: Option<bool>,
    /// v0.20 (Process-Integrity): siehe PersistedFlightStats-Feld gleichen Namens.
    resume_gap_minutes: Option<i64>,
    /// Siehe PersistedFlightStats — die laengste Neustart-Luecke.
    resume_luecke_max_secs: Option<i64>,
    /// Nach einem Sim-Absturz auf einem anderen Platz am Boden neu geladen,
    /// und der Pilot fliegt von dort weiter („Vom Boden weiterfliegen").
    /// Bis zum neuen Abheben beendet kein Arrived-Rueckfall den Flug, und
    /// der neue Start ueberschreibt die Abflugwerte nicht (GAF 278).
    weiterflug_ab_boden_seit: Option<DateTime<Utc>>,
    /// Wann der Flug nach einem App-Neustart wiederhergestellt wurde. Das
    /// Resume-Gate schlaegt die Zeit bis zum Scharfschalten auf die Luecke
    /// auf — in ihr lief weder Streamer noch Aufsetz-Sampler.
    resume_wiederhergestellt_am: Option<DateTime<Utc>>,
    /// Laeuft schon ein Resume-Gate? Ein zweites „Fortsetzen" (Hauptfenster
    /// UND LAN-Fernbedienung) startete sonst ein zweites Gate, das den
    /// Wartegrund des ersten ueberschrieb (QS 24.09.2026).
    resume_gate_laeuft: bool,
    /// Worauf das Resume-Gate gerade wartet, und seit wie vielen Sekunden.
    /// Fuer die Oberflaeche; `None`, sobald der Flug scharf ist.
    resume_wartet: Option<(String, i64)>,
    /// Dieselbe Luecke in SEKUNDEN, nur fuer die Sprung-Pruefung.
    ///
    /// `resume_gap_minutes` schneidet auf ganze Minuten ab: 105 Sekunden
    /// werden dort zu 60, und die gerechnete Grundgeschwindigkeit steigt
    /// entsprechend — genug fuer einen Fehlalarm nach einem Update-Neustart
    /// im Sinkflug (Cloud-QS 23.09.2026). `None`, wenn die Dauer nicht
    /// bekannt ist; dann gilt der Zweifel fuer den Piloten.
    resume_luecke_secs: Option<i64>,

    /// Spec sim-disconnect-auto-resume F2 (Pause-Akkumulator):
    /// Summe aller Pause-Sekunden seit Flugstart. Wird beim
    /// Resume um die jeweilige Pause-Dauer erhoeht und in
    /// `build_heartbeat_body` von der Wall-Clock-Flugzeit
    /// abgezogen, damit eine 30-min MSFS-Esc-Pause oder ein
    /// 25-min Sim-Crash nicht als 30/25 zusaetzliche Flight-Time-
    /// Minuten im PIREP landen.
    pause_total_duration_secs: i64,
    /// Spec sim-disconnect-auto-resume F2: Liste aller abgeschlossenen
    /// Pause-Bloecke (Start, Ende, Reason, Drift). Audit-/Debug-Daten —
    /// `pause_total_duration_secs` haelt die Summe parallel fuer den
    /// schnellen Flight-Time-Abzug.
    pause_segments: Vec<PauseSegment>,

    /// True once the aircraft has actually been airborne above
    /// `WAS_AIRBORNE_AGL_FT` ft AGL since the flight started. Used to
    /// gate the universal Arrived-fallback and divert-detection so
    /// they NEVER fire pre-flight — caught by a real bug at KFLL
    /// where GSX repositioned the aircraft a few meters at the gate,
    /// `block_off_at` got stamped from the resulting >0.5 kt motion,
    /// and 9 minutes later (engines never started) the fallback
    /// fired with phase=Arrived + a divert banner pointing at
    /// "you're in KFLL, planned was MKJS — divert?". Aircraft had
    /// never even left the gate.
    was_airborne: bool,
    /// Consecutive-tick counter for the airborne dwell filter. Bumps
    /// on every tick where the airborne conditions hold; clears the
    /// moment any condition fails. Hits `WAS_AIRBORNE_DWELL_TICKS`
    /// → `was_airborne` flips true. Live bug 2026-05-03 (B738/GSX
    /// pushback): a single glitch tick at FL538 was enough to flip
    /// the flag. Sustained filter hardens that.
    airborne_dwell_ticks: u8,

    /// Wann das Flugzeug zum ersten Mal nach `tug_done` zum Stehen
    /// kam (gs < 0.5 kt). Triggert das 10-Sekunden-Dwell-Fenster
    /// bevor die Phase auf TaxiOut wechselt — entspricht dem echten
    /// Workflow "Tug ab, Park-Bremse, Funk, dann anrollen". Reset
    /// auf None sobald die Phase auf TaxiOut springt; nicht
    /// persistiert (Pushback überlebt keinen Restart).
    pushback_stopped_at: Option<DateTime<Utc>>,

    /// Seit wann das Flugzeug in der Pushback-Phase VORWÄRTS mit eigenen
    /// Triebwerken rollt, obwohl der Simulator weiter „Pushback läuft"
    /// meldet (Status 0/1/2). Siehe `PUSHBACK_HAENGT_KT`. Zurück auf `None`,
    /// sobald die Bedingung bricht oder die Phase wechselt; nicht
    /// persistiert (wie `pushback_stopped_at`).
    pushback_vorwaerts_seit: Option<DateTime<Utc>>,

    /// Der Pushback-Status stand schon beim ERSTEN Wert des Fluges (am Gate,
    /// Preflight/Boarding) auf „läuft" (≠ 3). Ein funktionierender Simulator
    /// beginnt mit 3 und wechselt erst, wenn der Schlepper ankoppelt — das
    /// geschieht zwar noch in der Boarding-Phase (die endet erst mit der
    /// Bewegung), aber nie beim ersten Wert. Nur ein klemmender Status (Joel,
    /// MSFS 2020: dauerhaft 0) ist von Anfang an aktiv. Erst dann darf
    /// `pushback_haengt_aber_rollt` greifen — sonst hielte er einen echten
    /// Vorwärts-Schlepp mit laufenden Triebwerken für Rollen (Codex
    /// 15.10.2026). Nicht persistiert: Nach einem Neustart zählt der erste
    /// Wert danach; mitten im Pushback greift wie bisher die Luftrettung.
    /// Bewusst offenes Restrisiko: Neustart am Gate bei schon angekoppeltem
    /// Schlepper, danach Vorwärts-Schlepp > 8 kt mit laufenden Triebwerken
    /// 15 s lang — dann wird TaxiOut etwas früher erkannt, sonst nichts.
    pushback_status_klemmt: bool,
    /// Ob der erste Snapshot schon da war (für `pushback_status_klemmt`).
    pushback_status_gesehen: bool,

    /// Wann die Groundspeed in der TakeoffRoll-Phase erstmals unter
    /// `TAKEOFF_ROLL_ABORT_KT` fiel (Dwell-Start der Rückkante nach
    /// TaxiOut — behebt TAKEOFF_ROLL_STUCK, z.B. EGLC-Backtrack-Fall).
    /// Reset sobald die GS wieder über die Schwelle steigt oder die Phase
    /// wechselt; nicht persistiert (überlebt keinen Restart).
    takeoff_roll_slow_since: Option<DateTime<Utc>>,
    /// v0.21.x (Backtrack-Erkennung): höchste bisher in DIESER TakeoffRoll-
    /// Rolle gesehene Groundspeed. Beim Eintritt auf die aktuelle GS gesetzt.
    /// Erreicht der Peak nie `TAKEOFF_ROLL_CONFIRM_KT`, greift die Plateau-
    /// Rückkante. Nicht persistiert. Siehe [`takeoff_roll_stall_check`].
    takeoff_roll_peak_kt: f32,
    /// v0.21.x (Backtrack-Erkennung): Dwell-Start der Plateau-Rückkante —
    /// wann die GS zuletzt aufhörte, neue Höchstwerte zu setzen (unter
    /// CONFIRM_KT). Reset bei jedem neuen Peak. Nicht persistiert.
    takeoff_roll_stall_since: Option<DateTime<Utc>>,

    // ---- Capture at takeoff ----
    takeoff_weight_kg: Option<f64>,
    takeoff_fuel_kg: Option<f32>,
    /// v0.5.16: pitch / bank at takeoff (rotation moment). Submitted
    /// as numeric PIREP custom fields `takeoff-pitch` / `takeoff-roll`
    /// — DisposableSpecial dmaintenance reads these for tail-strike
    /// (pitch > 12.5° default) and wing-strike (roll > 15° default)
    /// detection. Captured the moment `takeoff_at` is stamped.
    takeoff_pitch_deg: Option<f32>,
    takeoff_bank_deg: Option<f32>,

    // ---- Capture at touchdown ----
    landing_rate_fpm: Option<f32>,
    landing_g_force: Option<f32>,
    landing_pitch_deg: Option<f32>,
    /// v0.5.16: bank angle at touchdown — submitted as numeric PIREP
    /// custom field `landing-roll` for the wing-strike maintenance
    /// check (default >15° = strike).
    landing_bank_deg: Option<f32>,
    landing_speed_kt: Option<f32>,
    /// v0.5.17: groundspeed at touchdown. Sent in MQTT touchdown
    /// payload as `gs_kt` so the live-tracking server can compute
    /// real-world rollout distance vs. IAS-derived rollout (which
    /// distorts when there's strong head/tailwind).
    landing_groundspeed_kt: Option<f32>,
    /// Gemessene Eigengeschwindigkeit beim Aufsetzen. Nenner des
    /// Vorhaltewinkels in der Ausrichtungs-Achse (v1.6.3-QS): beide
    /// Simulatoren liefern sie nativ, sie muss also nicht aus Grund-
    /// geschwindigkeit und Gegenwind zusammengerechnet werden.
    landing_true_airspeed_kt: Option<f32>,
    landing_weight_kg: Option<f64>,
    landing_heading_deg: Option<f32>,
    landing_fuel_kg: Option<f32>,
    /// Kam `landing_fuel_kg` aus dem Aufsetz-Puffer? Siehe
    /// `stamp_touchdown_metadata` — der Puffer schlaegt den Schnappschuss,
    /// unabhaengig davon, welcher Stempel zuerst laeuft.
    landing_fuel_puffer: bool,

    // ---- Landing analyzer (Phase I) ----
    /// Most negative VS observed in the touchdown window. The first
    /// on-ground snapshot already gives a good number, but the spike
    /// often lands 1–2 ticks later — we keep refining for ~5 s after
    /// touchdown so the score reflects the worst case.
    landing_peak_vs_fpm: Option<f32>,
    /// Highest G-force observed in the touchdown window.
    landing_peak_g_force: Option<f32>,
    /// v0.7.1 (Spec F4 + P2.2-D): Confidence + Source. Atomar mit
    /// `landing_rate_fpm` via `finalize_landing_rate()`-Helper gesetzt
    /// damit sie zum finalen Wert konsistent bleiben.
    /// "High" | "Medium" | "Low" | "VeryLow"
    landing_confidence: Option<String>,
    /// "vs_at_impact" | "smoothed_500ms" | "smoothed_1000ms" | "pre_flare_peak"
    landing_source: Option<String>,
    /// Gesetzt, wenn die Aufzeichnung im Aufsetzfenster nicht ausreicht.
    ///
    /// Untersuchung 12.09.2026 (CFG 2090): Eine Landung erhielt 97 Punkte
    /// und die Note A+, obwohl zwischen letzter Luftprobe und Bodenkontakt
    /// 0,92 s ohne jede Messung lagen — die Bewertung nahm den ersten Frame
    /// danach. Ist dieses Feld gesetzt, gibt es KEINE Landerate und damit
    /// keine Note: `canonical_landing_rate_fpm` liefert `None`, und die
    /// Bewertung kennt "lieber gar keine Note als eine geschenkte" bereits.
    ///
    /// Das Feld sagt nichts über den Piloten. Es sagt: Wir haben es nicht
    /// gemessen. Siehe `touchdown_v2::pruefe_bewertbarkeit`.
    landung_abdeckung_fehlt: Option<touchdown_v2::FehlendeAbdeckung>,
    /// Wurde „Landung nicht messbar" schon geschrieben? Die Bewertung läuft
    /// bei jedem Takt erneut; ohne diese Merkung stand die Zeile alle drei
    /// Sekunden im Protokoll (MSC1588, 22.09.2026: zehnmal für EINEN
    /// Vorfall — im Report sah das nach zehn Fällen aus).
    landung_unmessbar_gemeldet: bool,
    /// Wie zuverlässig der Sampler lief (`SamplerDiagnose::als_json`).
    /// Steht neben dem Aufsetzfenster im Protokoll und im PIREP — damit
    /// eine dünne Aufzeichnung nicht nur auffällt, sondern auch erklärbar
    /// wird (kam der Client nicht zum Messen, oder lagen die Proben nur
    /// ungünstig?).
    sampler_diagnose: Option<serde_json::Value>,
    /// Bodenhöhe genau dieses Flugzeugs, gemessen beim Rollen. Bezug der
    /// Tiefflug-Prüfung in der Aufsetzerkennung — siehe
    /// `touchdown_v2::BodenhoehenReferenz` (Befund DLH 880).
    bodenhoehe: touchdown_v2::BodenhoehenReferenz,
    /// Die Ablehnungs-Warnung im Finalisierer wurde für die aktuelle
    /// Ablehnung schon geschrieben (nur gegen Protokollflut, nicht gesichert).
    ablehnung_gemeldet: bool,
    /// How many bounces (on_ground → !on_ground → on_ground) we counted
    /// within the touchdown window. >0 implies the pilot didn't put it
    /// down clean.
    bounce_count: u8,
    /// P1/8b (Thomas 22.10.2026): leichte Hopser — >= `BOUNCE_LIGHT_MIN_DAUER_MS`
    /// ununterbrochen ohne Bodenkontakt, >= `BOUNCE_LIGHT_MIN_AGL_FT` und unter
    /// der Hopser-Schwelle (5 ft) ueber der Bodenhoehe, mit zweitem Kontakt.
    /// Gezaehlt wie `bounce_count` (Streamer als Vorwert, 50-Hz-Analyse
    /// ueberschreibt). Teilnote siehe `sub_bounces_mit_leichten`.
    light_bounce_count: u8,
    /// 1-Hz-Streamer: Beginn der laufenden Luftphase nach dem Aufsetzen (ms
    /// seit Aufsetzen) und ihre Hoehe ueber Boden, fuer den leichten Hopser.
    bounce_luft_seit_ms: Option<i64>,
    bounce_luft_max_ft: f64,
    /// Categorised score, computed once when the window closes. The
    /// activity log emits this and we ship it in the PIREP custom fields.
    landing_score: Option<LandingScore>,
    /// True once the score has been announced to the activity log, so
    /// repeated streamer ticks (or a Tauri restart that resumes after
    /// touchdown) don't re-fire the entry.
    landing_score_announced: bool,

    /// Edge-tracking flag for AGL-based bounce detection. Set to true
    /// once the aircraft climbs above `BOUNCE_AGL_THRESHOLD_FT` after
    /// the initial touchdown; a subsequent drop below
    /// `BOUNCE_AGL_RETURN_FT` (within `BOUNCE_WINDOW_SECS`) increments
    /// `bounce_count` and clears this flag, ready to arm again.
    /// Replaces the noisy `was_on_ground && !on_ground` flicker we
    /// used pre-Tier-1, which was tripping on gear-strut oscillation.
    bounce_armed_above_threshold: bool,
    /// Lernpaket AP3: Hoehe ueber Grund beim ersten Bodenkontakt im
    /// Hopser-Fenster — Bezug fuer `BOUNCE_AGL_THRESHOLD_FT`/`_RETURN_FT`.
    bounce_boden_agl_ft: Option<f64>,
    /// Sideslip / crab angle at the moment of touchdown, in degrees.
    /// Computed from `heading_true_deg − groundtrack` where the
    /// ground track is reconstructed from the last few ring-buffer
    /// samples just before touchdown. Positive = aircraft nose right
    /// of track (right crosswind crab); negative = nose left.
    /// `None` until we capture it; `None` also when the buffer can't
    /// produce a track (e.g. taxi-speed touchdown after a roll-out).
    touchdown_sideslip_deg: Option<f32>,
    /// v0.4.4: Sampler-side touchdown capture. Der 50Hz-Touchdown-Sampler
    /// detektiert die `on_ground=false→true`-Flanke direkt (innerhalb
    /// von 20ms) und merkt sich VS am Edge plus VS-Min in den letzten
    /// 500ms davor (= echter Touchdown-Sinkflug, bevor Rollout den
    /// Wert kaputt-mittelt). Wenn step_flight später (bis zu 5s
    /// verspätet im Streamer-Tick) die Edge auch detektiert, nimmt es
    /// diese pre-captured Werte statt den Buffer-Window-Scan zu
    /// versuchen — dann sind die Pre-TD-Samples nämlich schon evicted.
    ///
    /// Live-Bug Pilot-Test 2026-05-06 (DAL93 EDDB→KJFK): X-Plane Land-
    /// ung mit echtem -300 fpm → AeroACARS scorte +35 fpm (smooth)
    /// weil Streamer 5s nach Touchdown wachte und Buffer dann nur
    /// noch Rollout-Samples mit VS≈0 enthielt.
    sampler_touchdown_at: Option<DateTime<Utc>>,
    /// Bug B (Codex, Vereinheitlichung 09/2026): eine explizite `FalseEdge`-
    /// Ablehnung des Samplers ist am Zustand `sampler_touchdown_at == None`
    /// allein nicht von "Sampler hat noch keinen Kandidaten gesehen"
    /// unterscheidbar — beide sind None. Ohne dieses Feld ueberstimmt
    /// `finalize_landing_score_if_due`s FSM-Fallback (`canonical_landing_rate_fpm`)
    /// eine aktive, evidenzbasierte Ablehnung stillschweigend. Nur fuer
    /// Gruende ausser `InsufficientTelemetry` gesetzt (siehe Setzstelle) —
    /// "zu wenig Telemetrie" ist eine legitime "Sampler weiss es nicht,
    /// FSM darf entscheiden"-Situation, keine aktive Ablehnung.
    sampler_touchdown_rejected_reason: Option<touchdown_v2::FalseEdgeReason>,
    sampler_touchdown_vs_fpm: Option<f32>,
    sampler_touchdown_g_force: Option<f32>,
    /// Stammt `sampler_touchdown_vs_fpm` aus der Aufsetzmeldung des
    /// X-Plane-Plugins? Nur dann darf der Wert die Abdeckungssperre
    /// überstehen (`plugin_sinkrate_uebernehmen`).
    sampler_touchdown_vs_vom_plugin: bool,
    /// v0.7.0 (Forensik v2 P0 fix): wenn der Sampler ein TD-Edge detected,
    /// wird er hier zwischengespeichert (nicht direkt sampler_touchdown_at).
    /// Ein subsequent Sampler-Tick (mind. 1.1 sec spaeter) ruft
    /// `touchdown_v2::validate_candidate` mit den post-edge samples auf.
    /// Bei VALIDATED → kopiert nach sampler_touchdown_at + open capture window.
    /// Bei FALSE_EDGE → cleared (ignored, weiter nach naechstem Edge suchen).
    /// Spec: docs/spec/touchdown-forensics-v2.md Sektion 4.
    pending_td_at: Option<DateTime<Utc>>,
    /// v0.7.0 (Round-3 P2 fix): wenn das X-Plane-Plugin den Edge ausloest
    /// (current_premium_touchdown() ist drain pattern via take()), speichern
    /// wir die Premium-VS/G hier zusammen mit pending_td_at.
    /// Vorher (Bug): Premium wurde beim Edge konsumiert, beim spaeteren
    /// VALIDATED-Block war es None → Premium-Override fiel still aus.
    /// Jetzt: Premium-VS/G ueberlebt die 1.1s-Validation-Latenz im Pending-State.
    pending_td_premium_vs: Option<f32>,
    pending_td_premium_g: Option<f32>,
    /// Hat eine Plugin-Aufsetzmeldung diesen Kandidaten schon belegt? Eigener
    /// Merker, weil `pending_td_premium_vs` seit Plugin 1.0 auch nach einer
    /// Meldung leer sein kann (`null` = nicht gemessen) — sonst haengte sich
    /// ein Hopser-Echo an und ueberschriebe das endliche g der ersten
    /// Meldung (Cloud-QS 29.09.2026, N1).
    pending_td_premium_gesehen: bool,
    /// v0.5.5: running peak-descent VS tracker. Updated every sampler
    /// tick while AGL ≤ 250 ft (low-altitude / final approach territory).
    /// Picks the most negative pitch-corrected VS seen ONLY in the
    /// touchdown footprint, NOT across the whole approach.
    ///
    /// v0.5.11 hardening (renamed from `approach_vs_min_fpm`):
    /// the previous variant tracked min across the whole Approach +
    /// Final segment. Pilot's deep analysis exposed the bug class:
    /// a pre-flare descent at -1346 fpm @ 943 ft AGL would win over
    /// the actual gentle touchdown. AGL ≤ 250 ft restricts tracking
    /// to the meaningful zone.
    ///
    /// Used ONLY as a fallback when the AGL-derivative estimator
    /// fails (very sparse RREF, no usable AGL samples). Never wins
    /// against a real AGL-Δ estimate.
    /// Reset on Approach entry so go-arounds don't carry stale data.
    low_agl_vs_min_fpm: Option<f32>,
    /// v0.5.35: V/S des letzten airborne Samples vor dem Touchdown
    /// (mit Timestamp). Verwendet als robusten Fallback wenn der
    /// Lua-30-Sample-Estimator wegen sparse Sampling versagt
    /// (siehe GA-Flug-Bug 2026-05-08: Cessna 152 in X-Plane mit
    /// 0.1 Hz DataRef-Update → Estimator gibt -33 fpm zurueck obwohl
    /// das letzte airborne Sample -360 fpm bei AGL 145 ft hatte).
    /// Wird waehrend Approach/Final/Landing kontinuierlich
    /// upgedated. Reset on TakeoffRoll-Entry.
    last_low_agl_vs_fpm: Option<(f32, DateTime<Utc>)>,
    // ─── v0.5.25 Approach-Stability v2 ─────────────────────────────────
    //
    // Pre-v0.5.25: einfache Standardabweichung ueber Approach+Final-
    // Buffer (5000 ft AGL bis 0). Probleme:
    //   - Vectoring-Bank-Spikes von ATC bestrafen Pilot fuer die
    //     ausgefuehrte Anweisung
    //   - Step-Down-Descents waehrend Initial-Approach (Flaps/Speed-
    //     Down) verfaelschen V/S-Stddev
    //   - σ um Mittelwert misst nicht Glide-Slope-Abweichung
    //   - RWY-Wechsel mid-final bestraft Pilot fuer ATC-Action
    //
    // v0.5.25: scharfes Stable-Approach-Gate (= AGL ≤ 1000 ft) +
    // Glide-Slope-Deviation + Vector-Window-Filter.
    /// Mittlere Abweichung |actual_vs − target_vs(3°)| im 1000-ft-Gate.
    approach_vs_deviation_fpm: Option<f32>,
    /// Maximale Abweichung |actual_vs − target_vs(3°)| im LETZTEN
    /// 500-ft-Gate (= unter 500 ft AGL). Critical-zone Indikator.
    approach_max_vs_deviation_below_500_fpm: Option<f32>,
    /// Bank-Stddev ueber 1000-ft-Gate, gefiltert: Samples aus Vector-
    /// Windows (= 5 sec nach RWY-Change) ausgenommen.
    approach_bank_stddev_filtered_deg: Option<f32>,
    /// v0.20.1: Gegenstueck zur gefilterten Bank-Streuung — dasselbe Fenster,
    /// derselbe Vektor-Filter, dieselbe Mindest-Stichprobe.
    approach_vs_stddev_filtered_fpm: Option<f32>,
    /// True wenn waehrend Approach/Final eine RWY-Aenderung beobachtet
    /// wurde mit AGL < 1500 ft (= "late RWY change", Pilot wurde zu
    /// Maneuver gezwungen, score-relevante Beruecksichtigung).
    approach_runway_changed_late: bool,
    /// True wenn beim Erreichen 1000 ft AGL: VS-Deviation < 200 fpm
    /// AND Bank-Mittelwert < 5°. = Stable-Approach-Gate erreicht.
    approach_stable_at_gate: Option<bool>,
    /// Anzahl Samples die im 1000-ft-Window lagen (= Konfidenz-
    /// Indikator, < 5 Samples = niedrige Konfidenz).
    approach_window_sample_count: Option<u32>,
    /// Arrival-Airport-Elevation aus phpVMS-Cache. Ermoeglicht HAT
    /// (Height Above Touchdown) statt AGL fuer Stable-Approach-Gate-
    /// Window. Kritisch fuer Mountain-Airports (LSGS, LFKB, …) wo AGL
    /// ueber Bergrueeken fluktuiert. None → Fallback auf AGL (mit
    /// niedrigerem Confidence-Score).
    arr_airport_elevation_ft: Option<f32>,
    /// V/S-Jerk: mean |Δvs| zwischen aufeinanderfolgenden Samples im
    /// Gate. Sim-/Aircraft-agnostisches Stabilitaets-Maß (jet vs. GA).
    /// < 100 fpm/tick = stable, > 300 fpm/tick = unstable.
    approach_vs_jerk_fpm: Option<f32>,
    /// IAS-Stddev im Gate-Window. Speed-Stability-Indikator.
    /// < 5 kt = on-target, > 15 kt = unstabil.
    approach_ias_stddev_kt: Option<f32>,
    /// Excessive Sink Flag: True wenn IRGENDEIN Sample im Gate die
    /// Stable-Approach-Sinkrate verletzte. Schwelle = −1000 fpm auf 3°,
    /// mit dem ECHTEN Gleitwinkel skaliert (tan-basiert, z.B. ~−1834 fpm
    /// auf 5,5°), sodass korrekt geflogene Steilanflüge nicht fälschlich
    /// geflaggt werden. FSF-ALAR/FAA-AC-120-71B Stable-Approach-Kriterium.
    ///
    /// v0.19.x FIX: was a plain `bool` (never `None`) while every sibling
    /// field here is `Option`. `compute_approach_stability_v2` bails out
    /// early with fewer than 3 gate samples — a telemetry gap right at the
    /// gate, not an actual measurement — and the struct's `Default` left
    /// this at `false`, indistinguishable from "measured, genuinely no
    /// excessive sink". The frontend's `isLegacy`/missing-band detection
    /// correctly treats `None` as "not measured" for every OTHER field in
    /// this struct; this one field being non-Option meant a gate with too
    /// few samples still showed a confident green "STABLE" pill built from
    /// zero real measurements.
    approach_excessive_sink: Option<bool>,
    /// Stable-Configuration-Flag: Gear voll runter (≥99%) AND Flaps
    /// in Landing-Position (≥70%) am Gate. None bei Konfig-Sample fehlt.
    approach_stable_config: Option<bool>,
    /// Score-Version 20: Fahrt-σ ab 500 ft und Konfiguration am 500-ft-Tor
    /// („spaet stabilisiert", siehe `anflug_urteil::anflug_pruefung`).
    approach_ias_stddev_unter_500_kt: Option<f32>,
    approach_stable_config_500: Option<bool>,
    /// "Nutzte HAT statt AGL?" — fuer UI-Confidence-Indikator.
    approach_used_hat: bool,
    /// v0.5.27: Quelle des Flugplans. None = pre-v0.5.27 (Annahme:
    /// SimBrief), "simbrief" = OFP via SB, "manual" = VFR/manual mode.
    /// Wird im PIREP-Body weitergegeben damit der aeroacars-live-
    /// Monitor "Manual"-Pill anzeigen kann.
    flight_plan_source: Option<&'static str>,

    /// v0.7.7: aktuell aktive SimBrief-OFP-ID die beim `flight_start`
    /// oder letzten `flight_refresh_simbrief` geholt wurde. None bei
    /// VFR/Manual-Flights ohne OFP, oder pre-v0.7.7-PIREPs ohne dieses
    /// Feld. Wird verwendet um "OFP unveraendert"-Feedback im Bid-Tab-
    /// Refresh zu erkennen: nach neuem Fetch wird die alte mit der
    /// neuen ID verglichen.
    /// Spec docs/spec/ofp-refresh-during-boarding.md §6.1.
    simbrief_ofp_id: Option<String>,

    /// v0.7.7: rohes `<params><time_generated>` aus dem SimBrief-OFP
    /// (kein Format parsing — SimBrief liefert typisch Unix-Timestamp-
    /// String, aber wir machen keine Annahmen). Audit-Trail-Datum.
    /// Spec docs/spec/ofp-refresh-during-boarding.md §6.1.
    simbrief_ofp_generated_at: Option<String>,
    /// „Zeiten gegen Flugplan" (13.10.2026): die vier Planzeiten des
    /// geladenen OFP (`plan_*`, die Ist-Felder bleiben hier leer — sie
    /// kommen erst in [`flugzeiten`] aus `block_off_at` & Co. dazu).
    ofp_planzeiten: landing_scoring::flugzeiten::FlugZeiten,
    /// Kabinenansagen (Beta): gebuchte Passagiere aus dem OFP. `Some(0)` ist
    /// ein Frachtflug und schaltet die Ansagen ab, `None` heisst „kein OFP".
    /// Deshalb hier NICHT wie bei den Gewichten `0 → None` filtern.
    pax_count: Option<i32>,
    /// Kabinenansagen (Beta): Sitzplaetze der geplanten Config (OFP) —
    /// entscheidet Bizjet oder Linie.
    max_passengers: Option<i32>,
    /// Kabinenansagen (Beta): phpVMS-Flugart aus dem Bid (J, F, P …).
    flight_type: Option<String>,
    /// Kabinenansagen (Beta): Name der Airline aus dem Bid, fuer den Text.
    /// Die Kennung steht schon in `ActiveFlight::airline_icao`.
    airline_name: Option<String>,
    /// Kabinenansagen (Beta): der Planer dieses Fluges. Nicht gespeichert —
    /// nach einem Neustart entsteht er mit `Planer::fortsetzen` neu. Seit
    /// v3 (Etappe 4) als Modul aus dem Laufzeit-Paket.
    kabinen_planer: Option<kabinenplaner::Planer>,
    /// Zuletzt gesehener `go_around_count`: Der Planer will eine Flanke,
    /// keinen Zaehler.
    kabinen_durchstarts: u32,
    /// Kabinenansagen (Beta): Texte, Ton und Warteschlange dieses Fluges.
    /// Nicht gespeichert — siehe `kabinenansagen::Lauf`.
    kabinen_lauf: kabinenansagen::Lauf,
    /// Kabinenansagen (Beta): Zeitzonen von Abflug und Ziel aus dem OFP
    /// (Minuten gegen UTC). Rueckfall, wenn phpVMS keine Zeitzone kennt.
    utc_versatz_abflug_min: Option<i32>,
    utc_versatz_ziel_min: Option<i32>,
    /// Umleitung (Beta): vom Piloten im Flug gemeldetes neues Ziel (ICAO).
    /// Loest die Umleitungs-Ansage aus. Der PIREP-Divert laeuft weiter ueber
    /// den Banner nach der Landung.
    umleitung_ziel: Option<String>,
    /// Cockpit: erwartete Landung (Unix-s) aus der Planzeit am aktuellen Ort
    /// der Route (`landung_erwartet`). Nur in der Luft, nicht gespeichert —
    /// der naechste Takt rechnet sie neu.
    landung_erwartet: Option<i64>,
    /// Kabinenansagen: erwarteter Sinkflugbeginn (Unix-s), wie
    /// `landung_erwartet`, aber bis zum TOD-Fix des Navlogs.
    tod_erwartet: Option<i64>,
    // ─── v0.5.26 Erweiterte Landung-Metriken ───────────────────────────
    //
    // Phase 1 — Sicherheits-Indikatoren:
    //   * wing_strike_severity_pct: Bank-am-TD vs. typischem Aircraft-
    //     Limit, Severity 0-100% (= % Ausnutzung des Limits)
    //   * float_distance_m: Threshold-Crossing → TD, indiziert Float
    //   * touchdown_zone: 1/2/3 nach FAA (= 1.Drittel, 2.Drittel, 3.Drittel)
    //   * vref_deviation_kt: IAS-am-TD − Vref. Rangfolge siehe
    //     `landing_scoring::vref` (gemessen → kalibriert → FAA)
    //   * stable_at_da: composite-stable-flag aber bei 200 ft AGL/HAT
    landing_wing_strike_severity_pct: Option<f32>,
    landing_float_distance_m: Option<f32>,
    /// Touchdown-Zone-Klassifikation: 1 (erstes Drittel = correct),
    /// 2 (mittleres Drittel = lange-Landung), 3 (drittes Drittel =
    /// ueberschossen). None wenn runway_length unbekannt.
    landing_touchdown_zone: Option<u8>,
    landing_vref_deviation_kt: Option<f32>,
    /// Vref-Quelle (`landing_scoring::vref::VrefErgebnis::quelle`).
    landing_vref_source: Option<&'static str>,
    /// Verbreiterung des Vref-Toleranzbands in kt; `None` = kein Urteil.
    landing_vref_toleranz_kt: Option<f32>,
    /// Vref laut Formel (auch neben einer Messung) — fuer den Pruefbericht.
    landing_vref_formel_kt: Option<f32>,
    /// Klappen beim Aufsetzen in einer der beiden letzten Stufen?
    landing_vref_landeklappen: Option<bool>,
    /// Approach-Stable bei 200 ft AGL/HAT (= ICAO-DA-Gate)
    approach_stable_at_da: Option<bool>,
    /// Score-Version 23: erkanntes spaetes Final / Kurvenanflug. Ab `ab`
    /// zaehlen Querneigung, V/S-Streuung und Gleitpfad; dafuer gilt die
    /// 300-ft-Grenze (`rechtzeitig`). `None` = gerader Anflug.
    approach_spaetes_final: Option<spaetes_final::SpaetesFinal>,
    // Phase 2 — Visualisierung-Daten (nicht direkt im Score, fuer UI)
    // (= verwendet vorhandenen approach_buffer, kein extra Feld noetig)
    // Phase 3 — Aggregate
    /// Anzahl Stall-Warnings im Final + Approach.
    approach_stall_warning_count: u32,
    /// Score-Version 24: Wind im Anflug, Soll-Sinkrate, Sinkgrenze und
    /// laengster Sinklauf im Gate (`ApproachStabilityV2::wind`). Anzeige.
    approach_wind: Option<landing_scoring::anflug_wind::AnflugWind>,
    /// Lernpaket AP4/AP5 (29.09.2026): Gleitpfad-Abweichung und Anflugruhe
    /// dieser Landung — reine Forensik, keine Note liest das. Gesetzt neben
    /// der Stabilitaets-Auswertung, siehe `anflug_forensik_stempeln`.
    anflug_forensik: anflug_forensik::AnflugForensik,
    /// Eigener Ringpuffer der Anflug-Forensik: dieselben Proben wie
    /// `approach_buffer` (gleiche Stelle, gleicher Plausibilitaetsfilter),
    /// aber bis `ANFLUG_FORENSIK_PUFFER_MAX` statt 120 — die 120er-Kappe
    /// traegt ueber `compute_approach_stddev` die Note und bleibt deshalb.
    anflug_forensik_puffer: std::collections::VecDeque<ApproachBufferSample>,
    /// Aufsetzzeit und Fenster des letzten Stempels — fuer das Nachziehen.
    anflug_forensik_td: Option<DateTime<Utc>>,
    anflug_forensik_fenster: Option<chrono::Duration>,
    /// Die Forensik wurde nach einem Bahnwechsel neu gerechnet und steht
    /// noch nicht im Flug-Log (`anflug_forensik_nachtrag_ereignis`).
    anflug_forensik_nachtrag_offen: bool,
    /// Schnitt: Proben bis hier gehoeren zu einem abgebrochenen Anflug
    /// (`anflug_forensik_schneiden`). Und der Schnitt, der beim Stempel galt.
    anflug_forensik_ab: Option<DateTime<Utc>>,
    anflug_forensik_ab_gestempelt: Option<DateTime<Utc>>,
    /// Yaw-Rate am TD (heading-Aenderung pro Sekunde) in deg/sec.
    /// Hoch = Ground-Loop-Risk.
    landing_yaw_rate_deg_per_sec: Option<f32>,
    /// Brake-Energy-Proxy = (TD-IAS² × landing_weight) / rollout_distance.
    /// Indiziert Brake-Pack-Belastung. Seit 2.0.1 nicht mehr gesetzt.
    landing_brake_energy_proxy: Option<f32>,
    /// Mittlere Verzögerung beim Ausrollen in m/s², über die Bodengeschwindigkeit
    /// vom Aufsetzen bis zum Ende des Ausrollens. Ersetzt seit 2.0.1 die
    /// Bremsenergie (kJ/m), die nur das Gewicht abbildete.
    landing_decel_mps2: Option<f32>,
    // ─── v0.5.24 Takeoff-Edge-Capture (50Hz Sampler) ───────────────────
    //
    // Frueher: stats.takeoff_pitch_deg / takeoff_bank_deg wurden im
    // step_flight-Streamer-Tick (3-30s Cadence) gestempelt — also
    // potenziell mehrere Sekunden NACH dem echten Wheels-Up-Frame.
    // Resultat: Pitch wird zu hoch erfasst (= Initial-Climb-Pitch statt
    // Rotations-Pitch), bei tail-strike-empfindlichen Aircraft wie der
    // A321 ist das ein realistischer 2-3°-Versatz der False-Positive-
    // Tail-Strike-Checks im phpVMS DisposableSpecial-Modul triggert.
    //
    // Neu: der 50Hz-Touchdown-Sampler wurde erweitert um auch die
    // umgekehrte Edge (on_ground=true → false = Wheels-Up) zu fangen.
    // Capture im Frame des physischen Lift-Off (binnen 20ms). Werden
    // im step_flight-Phase-Transition als bevorzugte Quelle genutzt
    // statt snap.pitch_deg.
    sampler_takeoff_at: Option<DateTime<Utc>>,
    sampler_takeoff_pitch_deg: Option<f32>,
    sampler_takeoff_bank_deg: Option<f32>,
    // ─── v0.5.23 Touchdown-Forensik ───────────────────────────────────
    //
    // Bei jedem Touchdown laufen MSFS- und X-Plane-Schaetzer parallel
    // (siehe lib.rs ~Zeile 8254). Wir merken uns hier ALLE relevanten
    // Zwischenergebnisse damit der TouchdownPayload-Build sie ans
    // aeroacars-live-Monitor weitergeben kann fuer Forensik-Vergleiche.
    /// "msfs" / "xplane" / "other" — gestempelt im Touchdown-Frame.
    landing_simulator: Option<&'static str>,
    /// Drehflügler/Wasserflugzeug — beim Aufsetzen aus dem Snapshot
    /// bestimmt (dort liegen Kufen-/Schwimmer-Signale), damit die
    /// Ausrichtungs-Achse sie überspringen kann.
    landing_nicht_konventionell: bool,
    /// Lua-Style 30-Sample-Schaetzung in fpm. None wenn Pfad nicht lief.
    landing_vs_estimate_xp_fpm: Option<i32>,
    /// Time-Tier-Schaetzung in fpm. None wenn Pfad nicht lief.
    landing_vs_estimate_msfs_fpm: Option<i32>,
    /// Welcher Pfad hat den finalen vs_fpm geliefert.
    landing_vs_source: Option<&'static str>,
    /// v1.6.10 — der Stand des Sim-Latches VOR dem Aufsetzen. Nur so
    /// laesst sich unterscheiden, ob der Simulator die Zahl fuer DIESE
    /// Landung gesetzt hat oder ob dort noch die des vorigen
    /// Aufsetzens steht (Touch-and-Go, Hopser).
    sim_referenz_vor_aufsetzen: Option<f32>,
    /// v1.6.9 — die vom Simulator selbst gemeldete Aufsetzgeschwindigkeit
    /// (MSFS `PLANE TOUCHDOWN NORMAL VELOCITY`). Reine Gegenprobe fuer den
    /// Landebericht, fliesst in keine Bewertung ein. X-Plane: None.
    landing_sim_referenz_fpm: Option<f32>,
    /// X-Plane Gear-Sampler peak gear_normal_force_n. None auf MSFS.
    landing_gear_force_peak_n: Option<f32>,
    /// Lua-Schaetzer Window-Groesse in ms (None wenn Pfad nicht gewann).
    landing_estimate_window_ms: Option<i32>,
    /// Lua-Schaetzer Sample-Count im Window.
    landing_estimate_sample_count: Option<u32>,
    /// Frozen subsample of the ring buffer covering ±2 s around touchdown,
    /// for V/S-curve reconstruction in the PIREP notes block. Captured
    /// once when the on-ground edge fires; surviving across a Tauri
    /// restart so a resumed flight still has the curve to ship.
    touchdown_profile: Vec<TouchdownProfilePoint>,
    /// Runway-correlation result from the OurAirports CSV lookup,
    /// computed once at the touchdown edge from `(landing_lat,
    /// landing_lon, landing_heading_true_deg)`. Drives the runway
    /// distance / centerline-offset fields in the PIREP. None until
    /// computed; None also when no runway is within ~3 km of the
    /// touchdown coordinate.
    runway_match: Option<runway::RunwayMatch>,
    /// v1.7.8: Die Szenerie-Auskunft des Simulators für den Zielplatz.
    ///
    /// Bei MSFS wird sie beim Anflug angefordert und trifft asynchron
    /// ein; hier legt der Streamer sie ab, damit die Bahnauflösung sie
    /// beim Aufsetzen vorfindet. Bei X-Plane bleibt sie leer — dort
    /// liest der Client die Datei direkt.
    szenerie_auskunft: Option<Arc<sim_core::szenerie::SzenerieFlughafen>>,
    /// v1.7.44: Die Auskunft für den GEPLANTEN Zielplatz — ein eigener
    /// Platz, den ein Überflug nicht wegräumen kann.
    ///
    /// Der Fall aus DLH 373 (KJFK→EDDM, 20.09.2026): Um 15:38 lag EDDM
    /// mit 682 Rollwegen vor. Auf dem Sinkflug korrelierte die Bahn
    /// kurz mit EDML (Landshut, liegt auf der Anflugstrecke), das
    /// Ernteziel wechselte dorthin, und `szenerie_auskunft` wurde
    /// pflichtgemäß verworfen — EDML lieferte 0 Rollwege. Beim Aufsetzen
    /// in EDDM war damit nichts mehr da; die Ausfahrten fehlten, und die
    /// Queransicht stand ohne Bemaßung. Die richtige Auskunft traf vier
    /// Sekunden NACH dem Aufsetzen wieder ein.
    ///
    /// Das Verwerfen bleibt richtig — es verhindert, dass die Rollwege
    /// eines fremden Platzes in die Ausfahrten geraten. Falsch war nur,
    /// dass es die EINZIGE Kopie traf. Hier liegt deshalb zusätzlich die
    /// des geplanten Ziels, und beim Aufsetzen entscheidet weiterhin die
    /// ICAO der gematchten Bahn, welche verwendet wird: Bei einem
    /// Ausweichflug passt dieses Feld nicht und wird ignoriert.
    ziel_szenerie_auskunft: Option<Arc<sim_core::szenerie::SzenerieFlughafen>>,
    /// Stand der Auskunft in `ziel_szenerie_auskunft` — dieselbe
    /// Buchführung wie bei `szenerie_auskunft_stand`.
    ///
    /// Sie schützt gegen VERSPÄTET eintreffende ÄLTERE Lieferungen, nicht
    /// gegen leere: Eine leere Lieferung desselben Platzes mit höherem
    /// Stand ersetzt die gute sehr wohl (siehe `auskunft_ersetzen`). Der
    /// erste Kommentar hier behauptete das Gegenteil (externe Abnahme,
    /// 21.09.2026) — und eine falsche Zusage ist schlimmer als keine.
    ziel_szenerie_auskunft_stand: u32,
    /// v1.7.17: Die Szenerie-Auskunft des Simulators für den
    /// ABFLUGplatz — ein eigenes, unabhängiges Feld, NICHT dasselbe wie
    /// `szenerie_auskunft` oben.
    ///
    /// ⚠ Warum kein gemeinsamer Slot: `szenerie_auskunft` wird schon ab
    /// dem ERSTEN Tick des Fluges für das (dann noch geplante) Ziel
    /// angefragt — nicht erst ab dem Sinkflug, wie ein Kommentar an
    /// `versuchsfenster_fortschreiben` nahelegen könnte; dieser regelt
    /// nur den WIEDERHOLUNGS-Vorrat, nicht den ersten Versuch. Eine
    /// frühe Zustellung für den Zielplatz könnte also potenziell parallel
    /// mit dieser Ernte hier eintreffen. Ein gemeinsames Feld hätte
    /// bedeutet: entweder ein Wettlauf zweier Schreiber auf denselben
    /// Slot (genau die Fehlerklasse, die die Stand/Generation-Buchführung
    /// an `szenerie_auskunft` seit Runde 9 des v1.7.15-Zyklus verhindern
    /// soll), oder die Abflug-Ernte hätte diese Buchführung komplett
    /// mitübernehmen müssen — für einen Fall, der nie einen Zielwechsel
    /// kennt (der Abflugplatz eines Fluges ändert sich nie).
    ///
    /// Getrennt gehalten braucht die Abflug-Ernte keine STAND-Zählung wie
    /// beim Ziel (es gibt genau ein mögliches Ziel, `flight.dpt_airport`,
    /// und es wechselt nie) — aber eine GENERATIONS-Prüfung braucht sie
    /// doch: siehe `dep_szenerie_auskunft_generation` direkt darunter.
    /// (Runde-1-Korrektur: „schon da reicht als einzige Bedingung" war zu
    /// kurz gegriffen — externe Gegenprüfung, Codex, adversarial,
    /// 04.09.2026.) `standliste_fuer` prüft dieses Feld als zusätzliche
    /// Quelle, zusätzlich zu `szenerie_auskunft` — siehe dort.
    dep_szenerie_auskunft: Option<Arc<sim_core::szenerie::SzenerieFlughafen>>,
    /// Die Buch-Generation, unter der `dep_szenerie_auskunft` abgelegt
    /// wurde — vergleichbar mit `szenerie_auskunft_generation`, aber aus
    /// einem ANDEREN Grund noetig: der Abflugplatz selbst wechselt nie,
    /// aber die MSFS-VERBINDUNG kann es waehrend desselben Fluges
    /// (Neustart, Wechsel zwischen 2020/2024) — und eine neue Verbindung
    /// kann eine andere Szenerie mitbringen. Ohne diese Pruefung haette
    /// `dep_szenerie_auskunft` eine Lieferung der ALTEN Verbindung auf
    /// unbestimmte Zeit festgehalten, waehrend `szenerie_auskunft`
    /// (Ziel) dank ihrer eigenen Generations-Pruefung laengst neu
    /// angefragt haette (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026).
    dep_szenerie_auskunft_generation: u32,
    /// v1.7.17 Runde 3 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): die Szenerie-Generation, aus der `dep_gate` abgeleitet
    /// wurde (0, solange nie abgeleitet). Der Nachzieh-Tick vergleicht sie
    /// gegen `dep_szenerie_auskunft_generation` — nur bei einem
    /// tatsaechlichen Unterschied UND weiterhin laufendem Boarding wird
    /// `dep_gate` verworfen und neu abgeleitet. Ohne diesen Vergleich
    /// haette (Runde 2) die Loeschung direkt in der VOR `step_flight`
    /// laufenden Abflug-Ernte gesessen und dabei die PHASE DES VORHERIGEN
    /// Ticks gelesen — genau im Tick des Boarding→Pushback-Uebergangs
    /// haette sie den Riegel geloescht, kurz bevor keine Neuableitung mehr
    /// moeglich war (der Nachzieh-Tick lief mit der FRISCHEN Phase schon
    /// nicht mehr).
    dep_gate_generation: u32,
    /// v1.7.8: Was die Übernahme aus der Simulator-Szenerie bewirkt hat.
    ///
    /// `None` heisst: nicht versucht (anderer Simulator, keine
    /// Installation) oder nichts gefunden. Wandert in die Nutzlast,
    /// damit sich der Wechsel der Quelle **messen** laesst statt ihn zu
    /// glauben — dieselbe Ueberlegung wie beim Korpus-Lauf.
    szenerie_uebernahme: Option<szenerie_bahn::UebernahmeBericht>,
    /// Wie weit die Szenerie-Abfrage beim Simulator gekommen ist.
    ///
    /// ⚠ Ohne das steht am Flug nur "navdaten", und darin stecken drei
    /// verschiedene Faelle. Am 29.08.2026 war deshalb nicht zu sagen,
    /// warum die MSFS-Haelfte bei 5 von 5 Landungen nichts lieferte.
    szenerie_diagnose: Option<String>,
    /// Womit sich der Simulator gemeldet hat (Name + Version).
    sim_kennung: Option<String>,
    /// v0.8.0: Quelle der `runway_match`-Daten. `Navigraph` wenn der
    /// per VPS geladene NavAirport eine Match-fähige RWY lieferte;
    /// `OurAirportsFallback` wenn auf die eingebaute CSV gefallen
    /// wurde. None solange noch kein Touchdown registriert wurde.
    runway_source: Option<runway::RunwaySource>,
    /// v0.8.0: AIRAC-Cycle wenn `runway_source == Navigraph`. Wird
    /// im LandingRecord + TouchdownPayload mit-publiziert damit Webapp
    /// + phpVMS-PIREP-Notes zeigen können auf welchem Cycle die
    /// Bewertung beruht. None bei OurAirports-Fallback.
    runway_nav_cycle: Option<String>,
    /// v0.8.0: kompletter NavRunway-Geometriesatz, falls aus Navigraph
    /// kommend — wird im Folgenden für die TDZ/Aim/TCH/DDS-Assessment-
    /// Berechnungen gebraucht und im LandingRecord/Payload mit
    /// `displaced_threshold_ft / tch_expected_ft / glideslope_angle`
    /// fließen. None bei OurAirports-Fallback (= die OurAirports-CSV
    /// hat diese Felder nicht).
    runway_nav_geometry: Option<aeroacars_mqtt::navdata::NavRunway>,
    /// Score-Version 25: dieselbe Bahn aus Navigraph VOR der Szenerie-
    /// Uebernahme, mit eigenem Versatz — Bezug des ILS-Gleitpfads
    /// (`anflug_forensik_bahn`). None ohne Navigraph-Zuordnung.
    runway_ils_navigraph: Option<IlsNavigraph>,
    /// v0.8.0 F5: AGL (ft) am Threshold-Crossing — beim Touchdown-Edge
    /// im Streamer-Tick aus dem snapshot_buffer extrahiert (= erster
    /// Sample mit `along_track_m_signed >= 0`). None wenn:
    ///   * keine Navigraph-Geometrie verfügbar (= keine genaue
    ///     Threshold-Position),
    ///   * der snapshot_buffer keinen Pre-Threshold-Sample enthält
    ///     (= Buffer-Cadence-Mismatch oder spätes-Engagement),
    ///   * Touchdown im Approach-Pfad gar nicht erreicht wurde.
    runway_tch_actual_ft: Option<f32>,
    /// v0.16.24: the ICAO of the airport the wheels ACTUALLY touched on,
    /// resolved from the touchdown coordinate (`correlate_airport_icao`).
    /// For an on-plan landing this equals `arr_airport`; on a DIVERT it's
    /// the real landing field. Drives the on-demand navdata fetch + the
    /// finalize-with-best re-correlation + the OurAirports-fallback
    /// diagnostics. None until the touchdown runway correlation has run.
    ///
    /// v0.16.24 (QS-2 FIX B): PERSISTED (`PersistedFlightStats`) — a divert can
    /// suspend/resume between touchdown and PIREP filing, and the finalize +
    /// on-demand-fetch gates need this to survive the restart. `#[serde(default)]`
    /// keeps old blobs (saved before the field was persisted) loading as None.
    runway_correlation_icao: Option<String>,
    /// v0.16.24: whether the touchdown runway correlation fell back to the
    /// bundled OurAirports CSV (= the actual landing airport wasn't in the
    /// navdata cache at touchdown). Gates the on-demand
    /// `spawn_navdata_fetch` the streamer fires so finalize-with-best can
    /// upgrade the provisional result once the fetch completes.
    ///
    /// v0.16.24 (QS-2 FIX B): PERSISTED — survives a suspend/resume between a
    /// divert touchdown and PIREP filing so the on-demand-fetch gate isn't
    /// defeated. `#[serde(default)]` → old blobs load as false.
    runway_fallback_at_touchdown: bool,
    /// v0.16.24: ICAO for which the ON-DEMAND touchdown navdata fetch has
    /// already been fired (`maybe_fetch_actual_landing_navdata`). One-shot
    /// guard so a divert airport NOT in the active AIRAC cycle (a 404 never
    /// populates the cache, so `spawn_navdata_fetch`'s contains-key guard
    /// can't suppress it) isn't re-requested every streamer/sampler tick for
    /// the rest of the flight. Runtime-only.
    divert_ondemand_fetch_attempted: Option<String>,
    /// v0.16.24: ICAO of the divert airport for which the approach-phase
    /// pre-fetch (`spawn_navdata_fetch`) was last fired, so a stable
    /// committed approach to a different field warms the cache exactly once
    /// (flicker / overflown-airport guard). Runtime-only.
    divert_prefetch_icao: Option<String>,
    /// Der Stand der am Flug abgelegten Szenerie-Auskunft.
    /// Siehe `auskunft_ersetzen`.
    szenerie_auskunft_stand: u32,
    /// Die zuletzt gerechneten Ausfahrten mit dem Fingerabdruck von
    /// Bodenkarte und Bahn — siehe `ausfahrten_gemerkt`. Nur
    /// Zwischenspeicher: wird weder gespeichert noch gesendet.
    ausfahrten_merker: Mutex<Option<(AusfahrtenSchluessel, Vec<ausfahrten::Ausfahrt>)>>,
    /// Der Stand der Auskunft, mit der zuletzt ZUGEORDNET wurde.
    ///
    /// ⚠ Ein `bool` reichte nicht. Die Ernte nimmt ausdruecklich eine
    /// NEUERE Lieferung desselben Platzes an (`auskunft_ersetzen`) —
    /// sobald aber irgendein frueherer Vergleich einen Bericht erzeugt
    /// hatte, sperrte `szenerie_uebernahme.is_some()` jede
    /// Neuberechnung. Danach lagen Auskunft B und Bericht A nebeneinander
    /// (QS-Befund 2, erste Runde v1.7.15).
    korrelierter_szenerie_stand: u32,
    /// Der Simulator, mit dem beim AUFSETZEN zugeordnet wurde.
    ///
    /// ⚠ Das Nachholen bekommt keinen Schnappschuss und darf sich
    /// deshalb auch keinen besorgen. Fruehere Fassungen reichten den
    /// aktuellen `snap.simulator` durch, mit der Begruendung, der
    /// Simulator wechsele waehrend eines Fluges nicht — das stimmt fuer
    /// den Simulator, aber nicht fuer die VERBINDUNG. Ein Wechsel
    /// zwischen X-Plane und MSFS am selben Rechner erneuert sie, und das
    /// Buch wird dabei ausdruecklich zurueckgesetzt.
    ///
    /// Hier steht, womit die Zuordnung tatsaechlich gerechnet HAT. Ist
    /// das Feld leer, hat es nie eine Zuordnung gegeben — dann gibt es
    /// auch nichts nachzuholen.
    aufsetz_simulator: Option<sim_core::Simulator>,
    /// Wie oft die Bahn fuer DIESEN Flug zugeordnet wurde.
    ///
    /// ⚠ Das ist die Zahl, die auf die Leitung geht — NICHT
    /// `korrelierter_szenerie_stand`. Der stammt aus der
    /// SimConnect-Anfragenummer des `Auftragsbuch`, und die faengt nach
    /// einem App-Neustart wieder bei null an. Der Recorder haette dann
    /// eine tatsaechlich neuere Lieferung als veraltet abgewiesen: Stand 7
    /// gespeichert, Stand 1 kommt an, Riegel zu — und die Korrektur waere
    /// fuer immer verloren, ohne dass irgendwo etwas rot wird.
    ///
    /// Diese Zahl wird persistiert und waechst monoton ueber Neustarts
    /// hinweg. Sie wird zwischen zwei Aufsetzern NICHT zurueckgesetzt;
    /// der Riegel im Recorder haengt ohnehin an der einzelnen Zeile.
    bahn_revision: u32,
    /// Eine Bahnkorrektur wartet auf ihren Nachtrag.
    ///
    /// ⚠ Die Regel: Wer die Bahn aendert, setzt die Fahne; wer sendet,
    /// loescht sie. Ohne sie muesste jede Stelle, die eine Zuordnung
    /// anstoesst, selbst ans Senden denken — und das
    /// Navdaten-Upgrade beim Einreichen hat genau das nicht getan.
    ///
    /// Persistiert (N6/N7): Nach einem Neustart schickt der Streamer den
    /// Nachtrag nur, wenn er `spur_fertig` sieht — und das kann er erst,
    /// seit die Spur mit persistiert wird. Die Fahne haelt die Korrektur
    /// zusaetzlich fuer das Einreichen fest.
    bahn_nachtrag_offen: bool,
    /// Der zuletzt gesendete Nachtrag (Aufsetzzeit, Revision) — seeded
    /// die Streamer-Sperre nach einem Neustart. Persistiert.
    letzter_nachtrag: Option<(i64, u32)>,
    /// Die fertige Spur ist gegen eine Bahn projiziert, die nicht mehr
    /// gilt (Achswechsel nach dem Ausrollen). Sie bleibt lokal, geht aber
    /// nicht mehr auf die Leitung — siehe `spur_block`. Persistiert.
    bahn_spur_bezug_veraltet: bool,
    /// Der Szenerie-Status, wie er bei der letzten Zuordnung stand — die
    /// Auskunft selbst wird nicht persistiert (hunderte Rollwege), der
    /// Status daraus schon (Runde 5, N26). Persistiert.
    szenerie_status_fest: Option<String>,
    /// Die Generation, unter der die Flugkopie entstanden ist.
    /// Siehe `flugkopie_entwerten`.
    // Nur im `cfg(target_os = "windows")`-Block der Szenerie-Ernte
    // geschrieben und gelesen; auf Nicht-Windows existiert dieser
    // Zugriff nicht, weshalb das Feld dort als nie gelesen gilt.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    szenerie_auskunft_generation: u32,
    /// Die Phase beim VORIGEN Durchlauf der Szenerie-Ernte.
    ///
    /// ⚠ Irgendeine Phase, nicht die zuletzt geoeffnete Landephase —
    /// sonst geht `Approach → Holding → Approach` verloren. Siehe
    /// `braucht_neues_versuchsfenster`.
    // Nur im `cfg(target_os = "windows")`-Block der Szenerie-Ernte
    // geschrieben und gelesen; auf Nicht-Windows existiert dieser
    // Zugriff nicht, weshalb das Feld dort als nie gelesen gilt.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    szenerie_vorige_phase: Option<FlightPhase>,
    /// v0.16.24: the diverged field currently accruing stability time
    /// during a committed approach (distinct from `divert_prefetch_icao`,
    /// which is the field already fetched). When the nearest field changes
    /// this is reset along with `divert_prefetch_stable_since`. Runtime-only.
    divert_prefetch_candidate: Option<String>,
    /// v0.16.24: wall-clock instant the current divert-candidate airport
    /// first became the nearest field during a descending approach. The
    /// approach pre-fetch only fires after this has been stable for
    /// `DIVERT_PREFETCH_STABLE_SECS` — debounces an overflown airport that
    /// is briefly nearest mid-descent. Reset when the candidate changes or
    /// the aircraft is no longer on a committed approach. Runtime-only.
    divert_prefetch_stable_since: Option<DateTime<Utc>>,
    // v0.19.3: `planned_arr_position_cache` removed. It existed (v0.16.24) only
    // to memoize a 48k-row linear scan per approach tick; `runway`'s ident index
    // reduced that lookup to a hash hit, so the cache had nothing left to buy.
    /// Latitude at the touchdown edge — captured separately from
    /// `last_lat` so a resume mid-rollout doesn't overwrite it.
    landing_lat: Option<f64>,
    landing_lon: Option<f64>,
    /// True heading at touchdown (used for runway-end disambiguation
    /// in the runway lookup; `landing_heading_deg` is *magnetic*).
    landing_heading_true_deg: Option<f32>,
    /// Headwind (positive) / tailwind (negative) at touchdown in knots.
    /// Sourced from `AIRCRAFT WIND Z` (negated). None when the SimVar
    /// isn't wired or the flight resumed after touchdown.
    landing_headwind_kt: Option<f32>,
    /// Crosswind at touchdown in knots. Positive = from the right.
    /// Sourced from `AIRCRAFT WIND X`.
    landing_crosswind_kt: Option<f32>,
    /// Rechtweisender Wind im Moment des Aufsetzens — Basis der
    /// Seitenwind-Kompensation. Bewusst ZUSAETZLICH zu den koerperfesten
    /// Komponenten daneben: die haengen am Steuerkurs, der beim Ausrollen
    /// weiterdreht, und sie waren bis 14.08.2026 auf beiden Sims
    /// fehlerhaft. Der meteorologische Wind ist steuerkursunabhaengig und
    /// in beiden Simulatoren gleich definiert.
    landing_wind_direction_deg: Option<f32>,
    landing_wind_speed_kt: Option<f32>,

    // ---- Landing Analyzer (Stage 1) ----
    /// Rolling buffer of (V/S, bank) samples collected during the
    /// Approach + Final phases. Capped at ~120 entries (≈ 10-15 min
    /// of data at the position-streamer's 5-8 s cadence). Drained
    /// at the touchdown edge into stddev metrics.
    /// Pre-v0.5.25: nur (vs_fpm, bank_deg). Ab v0.5.25 reicht ein
    /// reicheres Sample fuer korrekte Stable-Approach-Gate-Auswertung
    /// (siehe ApproachSample-Struct). Beide Felder bleiben in den
    /// Touchdown-Stats gepflegt; die alten approach_*_stddev werden
    /// fuer Backward-Compat weiter geliefert, das richtige
    /// Stable-Approach-Maß ist `approach_vs_deviation_fpm`.
    approach_buffer: std::collections::VecDeque<ApproachBufferSample>,
    /// V/S standard deviation (fpm) over the approach window.
    /// Lower = more stable. Computed once at touchdown.
    approach_vs_stddev_fpm: Option<f32>,
    /// Bank-angle standard deviation (degrees) over the approach
    /// window. Lower = smoother flying.
    approach_bank_stddev_deg: Option<f32>,
    /// Rollout distance in meters: accumulated great-circle distance
    /// from the touchdown point until the rollout finalises — whichever
    /// of ~40 kt exit speed, a >30° heading turn-off, or a 5 kt full-stop
    /// fires first (see the `ROLLOUT_*` consts + the `step_flight`
    /// Landing arm). None until first touchdown. Resumed flights mid-
    /// rollout finalise on the next trigger or never (accepted imprecision).
    rollout_distance_m: Option<f64>,
    /// v1.7.0 Bahndisziplin — groesster Betrag des seitlichen Versatzes zur
    /// Bahnmittellinie waehrend des GEWERTETEN Rollwegs, in Metern.
    bahn_max_querversatz_m: Option<f64>,
    /// v1.7.0 — Strecke jenseits des Bahnendes, falls dort noch Fahrt war.
    bahn_overrun_m: Option<f64>,
    /// v1.7.0 — Zahl der Positionsproben im Messfenster. Unter 3 ist die
    /// seitliche Aussage nicht belastbar.
    bahn_proben: u32,
    /// v1.7.0 — true, sobald das Messfenster geschlossen ist (Ausfahrt
    /// eingeleitet oder unter die Messgeschwindigkeit gefallen).
    bahn_fenster_zu: bool,

    /// Bei welcher Distanz das seitliche Messfenster geschlossen hat.
    ///
    /// # Warum das eine eigene Zahl ist
    ///
    /// `bahn_raeum_laengs_m` sagt, wo der KURS abwich — dort endet die
    /// Bewertung. Das Messfenster schliesst aber frueher, naemlich unter
    /// sechzig Knoten (`BAHN_MESS_MIN_GS_KT`), und ab da waechst der
    /// groesste Querversatz nicht mehr mit.
    ///
    /// Bei DLH369 (EDDM 26L, 25.08.2026) lagen zwischen beiden 600 Meter:
    /// Kurswechsel bei 2.251 m, Fenster zu bei rund 1.650 m. Die Anzeige
    /// zeichnete die Bewertungsgrenze bei 2.251 m und daneben einen
    /// Hoechstwert von 12,9 m — waehrend die Spur innerhalb dieser Grenze
    /// bis 15,2 m lief. Beides stimmte fuer sich und widersprach sich
    /// zusammen.
    ///
    /// Mit dieser Zahl kann die Anzeige zeigen, bis WOHIN wirklich
    /// gemessen wurde.
    bahn_fenster_zu_laengs_m: Option<f64>,
    /// v1.7.0 — der gefahrene Streifen als (laengs_m, quer_m), fuer die
    /// Queransicht des Diagramms (Spec §8.3).
    ///
    /// Ausgeduennt auf einen Punkt je `BAHN_SPUR_MIN_ABSTAND_M` und hart
    /// begrenzt auf `BAHN_SPUR_MAX_PUNKTE`. Ohne beides waeren es bei
    /// 50 Hz und dreissig Sekunden Ausrollen fuenfzehnhundert Punkte je
    /// Landung — in jedem gespeicherten Datensatz, in jedem Upload und in
    /// jedem Diagramm, das sie zeichnen soll.
    bahn_spur: Vec<(f32, f32)>,
    /// Die Spuren früherer Durchgänge derselben PIREP, je mit der Bahn,
    /// auf die sie projiziert sind (`"EDDL 23L"`).
    ///
    /// Gesichert beim Durchstarten, bevor `spur_verwerfen` die Spur leert:
    /// Sie gehört nicht in die Bewertung der gewerteten Landung, soll aber
    /// in der Anzeige erscheinen — klar getrennt (EWG9503, 19.09.2026).
    /// Die Bahn steht dabei, weil eine Spur gegen eine ANDERE Bahn in der
    /// Ansicht der gewerteten nichts zu suchen hat.
    bahn_vorige_spuren: Vec<(String, Vec<(f32, f32)>)>,
    /// Die Kennung des Abflughafens — fuer die Abflugzeile der
    /// Wegpunkt-Tabelle. Steht hier, weil die Tabelle auch beim Aufsetzen
    /// eingefroren wird, wo es keinen Zugriff auf den Flug gibt.
    abflug_icao: Option<String>,
    /// Die gesendete Landung, für den Fall, dass sie nicht ankam.
    ///
    /// Bewusst NICHT persistiert: Überlebt der Client den Flug nicht, holt
    /// das hochgeladene Flugprotokoll die Landung nach. Hier geht es um den
    /// Fall, der bisher lautlos war — Client läuft, Leitung riss genau im
    /// Aufsetzmoment (AAL 1331, 20.09.2026).
    landung_payload: Option<Box<aeroacars_mqtt::TouchdownPayload>>,
    /// Hat der Broker die Landung bestätigt (PUBACK)?
    landung_live_bestaetigt: bool,
    /// Was an live als Landung ging: Aufsetz-Zeitpunkt (ms) und Sinkrate.
    /// Anders als `landung_payload` bleibt es nach der Bestätigung stehen —
    /// beim Einreichen vergleicht [`landung_widerspricht_meldung`] den PIREP
    /// damit (20.10.2026, EWG 1599: live −667, phpVMS −55). Nicht persistiert.
    gemeldete_landung: Option<(i64, i32)>,
    /// Die Bewertung der Landung, einmal gerechnet in
    /// [`announce_landing_score`] und von der Meldung und dem
    /// Touchdown-Payload gelesen (Codex-Befund zu P-4.40: vorher rechneten
    /// beide getrennt, mit verschiedenem Ankunftsflughafen). Der PIREP
    /// uebernimmt die gespeicherte Rechnung (`landung_endgueltig`, sonst
    /// `landung_sofort`, P-4.10). Nicht persistiert.
    landung_bewertung: Option<LandungsBewertung>,
    /// v3 (P-4.10): die Modulrechnung der Sofortnote (T+~9 s, am
    /// Aufsetz-Flughafen) — Eingabe und Ausgabe. Nicht persistiert.
    landung_sofort: Option<GespeicherteRechnung>,
    /// v3 (P-4.10): die endgueltige Modulrechnung nach `RolloutFinalized`
    /// (Ausrollen und Rollspur fertig). Nicht persistiert.
    landung_endgueltig: Option<GespeicherteRechnung>,
    /// Die Modulrechnung der laufenden Einreichung ([`PirepRechnung`]),
    /// gesetzt von [`pirep_bewertete_teile`]. Nicht persistiert.
    pirep_rechnung: Option<PirepRechnung>,
    /// Laeuft die Spuraufzeichnung gerade?
    ///
    /// Steuert den Aufzeichnungstakt (`adaptive_tick_interval_v2`). Ohne
    /// dieses Feld musste der Takt an einer Fahrt- oder Phasenschwelle
    /// haengen, und beides traf die Ausfahrt nicht: gemessen an neun
    /// echten Landungen brach die Kadenz bei dreissig Knoten von 0,51 s
    /// auf 3,01 s ein — mitten in der Kurve, weil dort die Phase von
    /// `Landing` auf `TaxiIn` wechselt.
    bahn_spur_laeuft: bool,
    /// Laengsposition, an der die Spur die Bahnkante gequert hat.
    ///
    /// # Warum das nicht dasselbe ist wie `bahn_raeum_laengs_m`
    ///
    /// Beides hiess bis hierher „Raeumpunkt", und die Anzeige hat es
    /// auseinandergehalten, der Client nicht. Ein Flugzeug schwenkt
    /// hunderte Meter vor der Ausfahrt nach aussen: `bahn_raeum_laengs_m`
    /// markiert den Beginn dieses Ausschwenkens (Kursabweichung ueber
    /// `BAHN_KURS_AUSFAHRT_GRAD`) — ab dort wird nicht mehr **bewertet**,
    /// weil die seitliche Lage an der Anweisung des Lotsen haengt.
    /// **Verlassen** hat das Flugzeug die Bahn erst hier, an der Kante.
    ///
    /// Die Anzeige braucht beide: die Marke „Bahn geraeumt" gehoert an
    /// die Kante, die gestrichelte Linie beginnt am Ausschwenken. Mit nur
    /// einem Wert sass die Marke mitten auf der Bahn.
    bahn_kante_laengs_m: Option<f64>,
    /// Fahrt beim Queren der Kante — die Groesse, die der Vertrag
    /// `clearance_speed_kt` nennt („Geschwindigkeit dort").
    bahn_kante_gs_kt: Option<f64>,
    /// Kursabweichung beim Ausschwenken, in Grad (positiv = rechts).
    ///
    /// Gespeichert, weil die SEITE der Ausfahrt sich im Moment des
    /// Kurswechsels noch nicht bestimmen laesst: Der Vertrag verlangt zwei
    /// uebereinstimmende Groessen (fallender Kurs UND wachsender
    /// Querversatz), und die Querbewegung ist dort naturgemaess erst
    /// wenige Meter gross. `bahn_felder` rechnet sie nach, wenn die ganze
    /// Spur vorliegt.
    bahn_raeum_kurs_diff: Option<f64>,
    /// Eine Ausfahrt, die UEBER der Messschwelle eingeleitet wurde — noch
    /// unbestaetigt. Siehe `AusfahrtKandidat`.
    bahn_ausfahrt_kandidat: Option<AusfahrtKandidat>,
    /// Rohe Bodenkarte des Ankunftsflughafens, für die Ausfahrten.
    ///
    /// # Warum sie hier liegt
    ///
    /// Sie kommt im Anflug (die Stand-Erkennung holt sie ohnehin), die
    /// gematchte Bahn erst beim Aufsetzen. Ohne Zwischenspeicher liesse
    /// sich beides nie zusammenbringen — und genau das war der Zustand:
    /// `ausfahrten::ausfahrten_fuer_bahn` war gebaut, getestet und wurde
    /// **nirgends aufgerufen**. Die Anzeige hatte den Platz dafür, bekam
    /// aber nie Daten.
    ///
    /// Grösse: im Mittel 91 KB, im schlimmsten gemessenen Fall 577 KB
    /// (ZSPD). Für die Dauer eines Fluges vertretbar; sie wird beim
    /// Zurücksetzen mit allem anderen frei.
    arr_ground_geojson: Option<Arc<str>>,
    /// v1.7.16 R5: fuer WELCHEN Flughafen `arr_ground_geojson` gilt — ohne
    /// dieses Feld hatte die Karte KEINE Zugehoerigkeit, anders als
    /// `arr_stands`/`arr_stands_icao` direkt daneben. Der Schreib-Riegel
    /// aus Runde 4 (`passt_zum_angefragten_ankunftsziel`) beweist nur den
    /// Moment der Ablage — aendert sich das angefragte Ziel DANACH erneut
    /// (z. B. ein zweiter Divert-Wechsel), blieb die Karte des ALTEN
    /// Ziels unmarkiert liegen und `bahn_felder` las sie ohne eigene
    /// ICAO-Pruefung fuer die Rollweg-Ausfahrten (externe Gegenpruefung,
    /// Codex, adversarial, 04.09.2026, Runde 5).
    arr_ground_geojson_icao: Option<String>,
    /// v1.7.0 — wo die Bahn geraeumt wurde: Laengsposition in Metern.
    /// `None`, solange das Fenster nicht wegen einer Ausfahrt zuging.
    bahn_raeum_laengs_m: Option<f64>,
    /// v1.7.0 — Geschwindigkeit beim Raeumen, in Knoten.
    bahn_raeum_gs_kt: Option<f64>,
    /// v1.7.0 Schritt 11 — Spurweite aus der Flugzeugdatei, in Metern.
    ///
    /// EINMAL beim Flugbeginn gelesen, nicht je Tick: Der Weg dorthin geht
    /// ueber einen Verzeichnis-Scan der Flugzeug-Pakete. `None` heisst, dass
    /// die Datei nicht eindeutig zuzuordnen war — dann gilt die Typtabelle,
    /// und das ist der Normalfall, nicht der Ausnahmefall.
    fahrwerk_spurweite_m: Option<f64>,
    /// v1.7.0 — Seite der Ausfahrt, `"left"` oder `"right"`.
    ///
    /// Nur gesetzt, wenn **zwei unabhaengige Groessen** dasselbe sagen:
    /// die Kursaenderung und die Querbewegung. Stimmt nur eine davon,
    /// bleibt das Feld leer — dann liegt vermutlich ein Achsenfehler vor,
    /// und eine Richtung zu behaupten waere schlimmer als keine (§8.6).
    bahn_raeum_seite: Option<String>,
    /// True once `rollout_distance_m` has been finalised. Stops the
    /// per-tick accumulation in step_flight from continuing past
    /// the actual stop.
    rollout_finalized: bool,
    /// v0.12.4 (Spec LE4): which trigger finalised the rollout —
    /// `"exit_speed"` / `"full_stop"` / `"turned_off_runway"`. The
    /// streamer loop copies it into the `touchdown_rollout_finalized`
    /// MQTT event. Transient (not persisted): after a restart mid-window
    /// the event is published without a reason.
    rollout_finalize_reason: Option<String>,
    /// Last (lat, lon) we accumulated into the rollout distance.
    /// Used to compute the great-circle delta to the next position.
    rollout_last_lat: Option<f64>,
    rollout_last_lon: Option<f64>,

    // ---- Multi-touchdown / Touch-and-Go / Go-Around tracking (Stage 3) ----
    /// Chronological log of every touchdown during the flight.
    ///
    /// On a normal one-landing flight: 1 entry (kind=FinalLanding).
    /// On a training flight with N touch-and-goes: N+1 entries — the
    /// last one is always the FinalLanding, earlier ones are
    /// TouchAndGo. The PIREP `landing_*` native fields and score are
    /// derived from the LAST entry (i.e. the actual final landing),
    /// so a T&G doesn't drag down the score.
    ///
    /// Empty until the first touchdown. Touchdowns are SNAPSHOTS
    /// taken at classification time — the in-progress
    /// `landing_peak_vs_fpm` etc. fields keep being refined for the
    /// CURRENT (= newest) touchdown until the bounce window closes.
    touchdown_events: Vec<TouchdownEvent>,
    /// Haltezeit-Uhr des Wegsteigens nach einem Aufsetzen: seit wann die
    /// Bedingung von [`wegsteigen_nach_aufsetzen`] ununterbrochen steht.
    /// Nach `TOUCH_AND_GO_DWELL_SECS` schliesst
    /// [`landung_beim_wegsteigen_abschliessen`] die Landung ab. `None`
    /// zwischen den Aufsetzern.
    touch_and_go_pending_since: Option<DateTime<Utc>>,
    /// Eine Landung wurde beim Wegsteigen abgeschlossen, die Phase steht
    /// aber noch auf `Landing` — der naechste Phasen-Takt wechselt nach
    /// `Climb`. Gesetzt von [`landung_beim_wegsteigen_abschliessen`], das
    /// auch der Aufsetz-Sampler aus seinem Faden aufruft; den Phasenwechsel
    /// selbst macht `step_flight_at`. Nicht persistiert: nach einem Neustart
    /// faengt der Takt ihn ueber `landing_at == None` in der Luft auf.
    landung_abgebrochen_phase_offen: bool,
    /// Haltezeit-Uhr des Aufsetz-Samplers fuer dasselbe Wegsteigen
    /// ([`steigflug_gehalten`]) — getrennt von `touch_and_go_pending_since`,
    /// weil beide Faeden in eigenem Takt messen. Nicht persistiert.
    sampler_steigflug_seit: Option<DateTime<Utc>>,
    /// Counter of go-arounds (rejected approaches without touchdown
    /// followed by climb-back-out). Surfaced as a custom PIREP field
    /// "Go-Arounds: N". Most flights = 0.
    go_around_count: u32,
    /// Lowest AGL observed during the current Approach/Final phase.
    /// Used by the go-around detector — once we see a sustained
    /// climb-back from this minimum past `GO_AROUND_AGL_RECOVERY_FT`,
    /// we classify it as a GA. Reset whenever Phase enters Climb
    /// (so each new approach gets a fresh minimum to compare against).
    lowest_agl_during_approach_ft: Option<f32>,
    /// Breite, Länge und wahrer Kurs am `lowest_agl_during_approach_ft`.
    /// Gezählt wird ein Durchstarter nur, wenn dort eine Landeschwelle
    /// voraus lag (`runway::schwelle_voraus`, 17.10.2026). `None` (z. B.
    /// nach einem Neustart aus alten Flugdaten) = wie bisher zählen.
    tiefster_anflugpunkt: Option<(f64, f64, f32)>,
    /// First tick where the GA detector saw AGL climbing back past
    /// the recovery threshold. We need this sustained for
    /// `GO_AROUND_DWELL_SECS` before firing — otherwise every brief
    /// climb-correction during a normal approach would count as a GA.
    go_around_climb_pending_since: Option<DateTime<Utc>>,

    /// v0.7.17 (B-010): Dwell-Timer fuer Climb → Descent.
    ///
    /// Bug-Symptom: GSG 105 (X-Plane 12 Cessna 1.000 kg, LDDU→LDSP)
    /// zeigte 6-Sekunden-Phase Descent (17:49:35 Climb→Descent,
    /// 17:49:41 Descent→Approach) — ein einzelner Turbulenz-/Trim-
    /// Spike loeste single-tick Phase-Wechsel aus, AGL stieg sogar
    /// zwischen den Phasen. Fix: Descent-Bedingungen muessen
    /// `DESCENT_DWELL_SECS` durchgehend halten bevor wir umschalten.
    /// Seit 17.10.2026 ungenutzt (Reiseband entscheidet v2); bleibt nur
    /// wegen der gespeicherten Flugdaten älterer Versionen.
    /// Cleared sobald die Bedingungen wieder brechen.
    descent_pending_since: Option<DateTime<Utc>>,

    /// En-Route-Versöhner (siehe `ENROUTE_RECONCILE_DWELL_SECS`): die
    /// v1-Phase, für die die Divergenz-Uhr läuft, + seit wann v1 dort
    /// UNVERÄNDERT steht, während v2 durchgehend (irgendwie) abweicht.
    /// Bewusst NUR an v1 gekoppelt: v2 darf während des Dwells
    /// weiterwandern (Descent→Approach→Final ist bei kurzen, flachen
    /// Anflügen normaler Evidenz-Fortschritt — Replay-Befund BE24: eine
    /// paar-gekoppelte Uhr startete bei jedem v2-Fortschritt neu und
    /// feuerte erst NACH dem Touchdown). Resets: jede v1-Transition
    /// (lebendige FSM), jeder Tick mit Gleichstand/Ineligibilität/
    /// fehlender Evidenz, jeder Reload-Glitch-Tick. Der Phasen-Anteil
    /// deckt auch v1-Wechsel ab, die den normalen Commit-Pfad umgehen
    /// (v0.13.17-Airborne-Rescue mit early return, Resume-Restore).
    /// Runtime-only wie die Schatten-Engine selbst (nach App-Restart
    /// wärmt v2 sich in ≤ 2 Fenstern wieder auf, der Versöhner beginnt
    /// dann einfach frisch).
    enroute_divergence_since: Option<(FlightPhase, DateTime<Utc>)>,

    // ---- v0.5.11 holding-pattern tracking ----
    /// 17.10.2026: Kursverlauf der letzten `HOLDING_KURS_FENSTER_SECS` in der
    /// Luft — (Zeit, wahrer Kurs, Höhenflug über `HOLDING_MIN_AGL_FT`).
    /// Ein- und Austritt der Warteschleife urteilen über die Drehung darin.
    /// Gelöscht bei jedem Phasenwechsel außer in die Warteschleife
    /// (`should_reset_holding_pending`), damit Drehungen einer anderen Phase
    /// keine auslösen. Runtime-only.
    kurs_verlauf: std::collections::VecDeque<(DateTime<Utc>, f32, bool)>,
    /// What phase we entered Holding from (Cruise or Approach).
    /// Restored on Holding exit unless an active descent has begun
    /// (then we go directly to Approach).
    previous_phase_before_holding: Option<FlightPhase>,

    /// Free-form ACARS log lines that `step_flight` wants posted to
    /// phpVMS' `/acars/logs` on the next streamer tick — primarily
    /// used by the Touch-and-Go and Go-Around classifiers since
    /// they fire from inside the FSM (not on the streamer's normal
    /// phase-change branch). Drained empty by the streamer after
    /// each successful POST. Keeps the FSM decoupled from the HTTP
    /// client.
    pending_acars_logs: Vec<String>,

    /// v0.7.17 (B-004): One-shot flag damit die „Aircraft fliegt nach
    /// ARRIVED"-Warnung pro Flug nur einmal gefeuert wird (kein Spam,
    /// kein redundantes flight.stop-Setzen). FlightStats ist nicht
    /// persisted (PersistedFlightStats ist die Persistenz-Schicht);
    /// `Default::default()` setzt es auf false.
    arrived_stale_warning_logged: bool,

    // ---- Landing Analyzer (Stage 2): SimBrief OFP plan ----
    /// Planned block (= ramp) fuel from the SimBrief OFP, in kg.
    /// None when the bid had no SimBrief OFP attached or the fetch
    /// failed. Captured once at flight_start; never updated mid-flight.
    planned_block_fuel_kg: Option<f32>,
    /// Planned trip burn (takeoff → touchdown) from the OFP. Used at
    /// PIREP build to compute fuel-efficiency vs the actual burn.
    planned_burn_kg: Option<f32>,
    /// Planned reserve fuel (alternate + holding).
    planned_reserve_kg: Option<f32>,
    /// Planned zero-fuel weight (kg).
    planned_zfw_kg: Option<f32>,
    /// Planned takeoff weight (kg).
    planned_tow_kg: Option<f32>,
    /// Planned landing weight (kg).
    planned_ldw_kg: Option<f32>,
    /// Planned route string from the OFP.
    planned_route: Option<String>,
    /// ATC-Rufzeichen aus dem geladenen OFP (`<atc><callsign>`), nur wenn es eine
    /// gueltige ICAO-Kennung ist (siehe `ofp_atc_callsign`). Hoppie/PDC meldet sich
    /// damit an — in PaxStudio/SimBrief kann ein anderes Rufzeichen als Airline +
    /// Flugnummer geplant sein (z.B. DLH4TK, EWG9KC). None = kein OFP oder ungueltig.
    planned_atc_callsign: Option<String>,
    /// v0.13.x (In-App-Live-Map): geplante Navlog-Fixes aus dem SimBrief-OFP
    /// (ident + lat/lon + kind, inkl. TOC/TOD falls im Navlog). Beim Flugstart
    /// gesetzt, von der Map-Ansicht via `flight_get_route_fixes` gelesen.
    /// Leer bei VFR/Manual/Resume (kein OFP) → Map faellt auf Great-Circle.
    planned_waypoints: Vec<api_client::RouteFix>,
    /// Planned alternate ICAO from the OFP.
    planned_alternate: Option<String>,
    /// v0.19.3: reference coordinates of the arrival airport — the position the
    /// divert geometry is measured against when the runway table is silent or
    /// wrong. Resolved from Navigraph → phpVMS → OurAirports; see
    /// `airport_reference_pos`.
    planned_arr_ref_pos: Option<(f64, f64)>,
    /// Which source `planned_arr_ref_pos` came from (`ARR_REF_SOURCE_*`). Kept
    /// so a weaker source can never overwrite a better one — Navigraph's fetch
    /// is asynchronous and arrives after the first ticks have already run.
    planned_arr_ref_source: u8,
    // ---- v0.3.0: MAX-Werte aus dem OFP für Overweight-Detection ----
    /// Maximum Zero-Fuel Weight (Strukturlimit). None wenn das OFP
    /// keinen `<max_zfw>`-Eintrag hatte (kommt bei Custom-Subfleets vor).
    planned_max_zfw_kg: Option<f32>,
    /// Maximum Takeoff Weight. Drives Overweight-Warnung im Live-
    /// Loadsheet vor Pushback und Score-Penalty im Landung-Tab.
    planned_max_tow_kg: Option<f32>,
    /// Maximum Landing Weight. Bei Overshoot droht Overweight-
    /// Landing-Inspektion in der echten Welt.
    planned_max_ldw_kg: Option<f32>,

    // ---- METAR snapshots (Phase J.2) ----
    /// Raw METAR text captured at takeoff for the departure airport.
    /// Filled by a fire-and-forget async fetch from the streamer when
    /// `step_flight` transitions into `Takeoff`. None until the fetch
    /// returns (and stays None if the airport isn't in NOAA's set).
    dep_metar_raw: Option<String>,
    /// v1.5.2 (#hud-metar): der DEKODIERTE Snapshot zum selben Abruf —
    /// das In-Sim-HUD zeigt ab v1.5.2 diese Felder statt den Rohtext zu
    /// parsen. Ein Dekoder (metar-Crate), eine Wahrheit; `raw` bleibt
    /// fuer Log und Alt-Widgets erhalten.
    dep_metar_snap: Option<MetarSnapshot>,
    /// Same for the arrival airport at touchdown (Final → Landing).
    arr_metar_raw: Option<String>,
    arr_metar_snap: Option<MetarSnapshot>,
    /// Markers so the streamer only kicks off one fetch per direction
    /// per flight. Set immediately when the spawn fires; cleared never.
    dep_metar_requested: bool,
    arr_metar_requested: bool,
    /// v1.5.1 (F2): eigener Latch für den frühen Descent-Abruf, damit
    /// der Final-Abruf trotzdem noch frisch nachholt (siehe `MetarKind`).
    arr_metar_early_requested: bool,

    // ---- Stand-Erkennung (v1.6, OSM-Ground-Layer) ----
    /// Fetch-Latches wie bei den METAR-Flags: einmal pro Richtung pro
    /// Flug, gesetzt beim Spawn, nie gelöscht. Bewusst NICHT im
    /// Resume-Snapshot — der Streamer ruft den Trigger jeden Tick mit der
    /// aktuellen Phase, und die Arrival-Arme decken Descent bis TaxiIn ab:
    /// jede Resume-Lage, in der das Stand-Gate die Daten noch braucht,
    /// lädt idempotent nach.
    dep_stands_requested: bool,
    /// Geladene Parkpositionen des ABFLUGhafens — Gegenstueck zu
    /// `arr_stands`. Befund OCN 1408 (Peter Z, EDDF, 13.08.2026): die
    /// Abflug-Zuordnung war ein EINMALversuch im Lade-Task; war die
    /// Position in genau dem Moment noch nicht nutzbar, blieb dep_gate
    /// fuer den ganzen Flug leer — obwohl der Stand (V172) in den Daten
    /// 0,4 m neben dem Flieger lag. Jetzt liegt die Liste hier und jeder
    /// Boarding-/Pushback-Tick versucht die Zuordnung erneut.
    dep_stands: Option<Vec<stands::ParkingStand>>,
    /// Fehlversuchs-Zaehler des Bodendaten-Abrufs (dep/arr gemeinsam
    /// gezaehlt je Richtung ueber die requested-Flags): ein gescheiterter
    /// Abruf gibt das requested-Flag zurueck, damit der naechste Tick es
    /// erneut probiert — aber hoechstens so oft, dass ein toter Server
    /// nicht endlos gehämmert wird.
    dep_stand_fetch_attempts: u8,
    arr_stand_fetch_attempts: u8,
    arr_stands_requested: bool,
    /// Für WELCHEN Flughafen die Ankunfts-Standliste angefragt wurde.
    /// Weicht das tatsächliche Ziel ab (Divert!), wird neu angefragt —
    /// vorher blieb bei einer Ausweichlandung für immer die Liste des
    /// GEPLANTEN Ziels liegen und der Stand konnte nie erfasst werden.
    arr_stands_requested_icao: Option<String>,
    /// Der Flughafen, auf dem der Flieger laut `arrival::locate`
    /// TATSÄCHLICH steht — von `settle_at_arrival_stand` bei JEDEM Aufruf
    /// gesetzt, unabhängig davon, ob dabei ein Stand gefunden wurde.
    ///
    /// ⚠ NICHT dasselbe wie `arr_stands_requested_icao`: Letzteres kommt
    /// aus `maybe_spawn_stand_fetch` und hängt am `divert_hint`, das laut
    /// Kommentar dort „erst beim Arrived-Fallback gemintet" wird — bei
    /// einem Divert, der die FSM normal ueber TaxiIn→BlocksOn erreicht
    /// (kein Fallback noetig), bleibt `divert_hint` mitunter fuer immer
    /// `None`, und `arr_stands_requested_icao` zeigt dann dauerhaft auf
    /// den GEPLANTEN statt den echten Platz. `settle_at_arrival_stand`
    /// berechnet `at_icao` dagegen bei JEDEM Aufruf frisch aus der
    /// aktuellen Position — dieses Feld haelt genau diesen Wert fest,
    /// damit der Nachzieh-Tick (`arrival_retry_ziel`) ihn spaeter wieder
    /// hat, auch wenn der erste Versuch mangels Daten leer ausging
    /// (externe Gegenpruefung, Codex, adversarial, 04.09.2026, Runde 2).
    arr_actual_icao: Option<String>,
    /// Parkpositionen des ZIELflughafens, geladen ab Descent. `None` =
    /// (noch) nicht verfügbar → die BlocksOn-Erkennung fällt auf das
    /// alte Kriterium zurück statt zu blockieren.
    arr_stands: Option<Vec<stands::ParkingStand>>,
    /// Für welchen ICAO `arr_stands` gilt — bei einem Divert passt die
    /// Liste nicht zum Flughafen, auf dem der Pilot steht.
    arr_stands_icao: Option<String>,
    /// Rollweg-Mittellinien des Zielflughafens aus derselben Bodenkarte wie
    /// `arr_stands`. Eigene Zuordnung, weil die Stände auch aus der
    /// Szenerie kommen können, wenn OSM keine Parkpositionen kennt.
    arr_rollwege: Option<Vec<Vec<(f64, f64)>>>,
    arr_rollwege_icao: Option<String>,
    /// Beginn (Zeit, lat, lon) eines Bremshalts auf einer Rollweg-
    /// Mittellinie, der in Standnähe liegt, aber noch nicht als Block-On
    /// zählt (siehe `ROLLWEG_HALT_BESTAETIGT_SECS`). Wird er bestätigt, ist
    /// das die Block-On-Zeit — aber nur am selben Ort
    /// (`kandidat_hier`): nach Versetzen oder erneutem Abheben gilt er nicht.
    /// Nicht persistiert: nach Resume beginnt die Frist neu.
    blocks_on_kandidat: Option<(DateTime<Utc>, f64, f64)>,
    /// Einmal-pro-Flug-Latch für die Diagnosezeile "Halt mit Bremse, aber
    /// kein Stand in Reichweite" — der Zustand hält sonst über viele Ticks
    /// an und würde das Log fluten.
    stand_gate_reject_logged: bool,
    /// v1.5.5: Latches für die LIVE-Posts der Stand-Custom-Fields an
    /// phpVMS (Dashboard zeigt den Stand schon im Boarding). Nicht
    /// persistiert — ein Doppel-Post nach Resume überschreibt dasselbe
    /// Feld mit demselben Wert, harmlos. Der BlocksOn-Rückweg setzt den
    /// arr-Latch zurück, damit ein korrigierter Stand nachgepostet wird.
    dep_gate_field_posted: bool,
    arr_gate_field_posted: bool,
    /// v1.7.17 Runde 14 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): `dep_gate_field_posted` wird OPTIMISTISCH gesetzt,
    /// BEVOR die asynchrone Anfrage ueberhaupt lief — ein Fehlschlag
    /// setzt es nie zurueck. Fuer die reine "nicht doppelt posten"-
    /// Idempotenz war das immer harmlos; als Grundlage fuer eine
    /// DESTRUKTIVE Entscheidung (das Feld spaeter explizit leeren) ist ein
    /// bloss VERSUCHTER Post aber keine ausreichende Zusicherung — ein
    /// fehlgeschlagener Versuch haette einen fremden, nie beruehrten
    /// Server-Wert autorisiert geloescht bekommen. Dieses Feld wird
    /// deshalb NUR innerhalb des Callbacks nach `Ok(())` gesetzt und
    /// persistiert (siehe `dep_gate_withdrawal_pending`, das darauf
    /// aufbaut), damit auch ein Resume zwischen erfolgreichem Post und
    /// Boarding-Ende die Bestaetigung nicht verliert.
    dep_gate_field_post_confirmed: bool,
    /// v1.7.17 Runde 13 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): AeroACARS hat selbst einen jetzt veralteten
    /// "Departure Gate"-Wert live gepostet, der noch zurueckgezogen werden
    /// muss. NUR in diesem Fall darf die finale Einreichung ("Departure
    /// Gate" explizit leeren, siehe `build_pirep_fields`) einen leeren
    /// String senden — sonst wuerde JEDER Flug ohne lokal erkannten Stand
    /// (der weit ueberwiegende Normalfall) einen moeglicherweise von
    /// woanders (Dispatcher, anderes Tool) gesetzten Wert zerstoeren, ohne
    /// dass AeroACARS je etwas Falsches gepostet hat.
    ///
    /// ⚠ Runde 14: die Entscheidung, dieses Flag zu setzen, stuetzt sich
    /// jetzt auf `dep_gate_field_post_confirmed`, nicht mehr auf das
    /// optimistische `dep_gate_field_posted` (siehe dessen Doku).
    dep_gate_withdrawal_pending: bool,
    /// v1.7.17 Runde 16 (externe Gegenpruefung, Codex, adversarial,
    /// 04.09.2026): der Boarding-Ende-Riegel hat EINMALIG entschieden,
    /// dass ein Rueckzug noetig IST — konnte es aber noch nicht ausfuehren,
    /// weil der Live-Post zu diesem Zeitpunkt weder bestaetigt noch
    /// definitiv fehlgeschlagen war (noch unterwegs). Diese Entscheidung
    /// wird HIER eingefroren, mit der zu diesem Zeitpunkt gueltigen Lage —
    /// NICHT spaeter neu bewertet.
    ///
    /// ⚠ Der Runde-15-Versuch, dasselbe Problem im Erfolgs-Callback zu
    /// loesen, indem die Bedingung DORT (mit der DANN aktuellen Phase und
    /// dem DANN aktuellen `dep_gate`) neu ausgewertet wurde, hatte zwei
    /// eigene Fehler: (1) `dep_gate` war zu diesem Zeitpunkt schon immer
    /// `None` (der Riegel loescht es sofort, unabhaengig von der
    /// Bestaetigung) — die Neubewertung fand darum NIE etwas zum
    /// Zurueckziehen, exakt in dem Fall, den sie loesen sollte; (2) ein
    /// voellig unrelated, SPAETERER Reconnect nach dem Boarding-Ende
    /// haette eine eigene Generationsabweichung erzeugt, die die
    /// Neubewertung faelschlich als "der verpasste Uebergang" gelesen
    /// haette — und einen laengst historischen, korrekten Stand geloescht.
    /// Ein eingefrorenes Flag statt einer Neubewertung vermeidet beide
    /// Fehlerklassen: der Erfolgs-Callback fuehrt nur noch eine BEREITS
    /// GETROFFENE Entscheidung aus, trifft keine neue.
    dep_gate_withdrawal_awaiting_confirmation: bool,

    // ---- Fuel tracking ----
    block_fuel_kg: Option<f32>,
    last_fuel_kg: Option<f32>,
    // ---- Live-Loadsheet (v0.3.0) ----
    /// Letzter beobachteter ZFW-Wert vom Sim. Treibt das Live-
    /// Loadsheet-Display im Cockpit-Tab während der Boarding-Phase
    /// (Pilot sieht wie ZFW während des Boardings hochläuft).
    /// None wenn das Aircraft-Profil keinen ZFW-SimVar hat (Fenix).
    last_zfw_kg: Option<f32>,
    /// Letzter beobachteter Total-Weight-Wert vom Sim (= TOW während
    /// der Boarding-Phase, sobald komplett beladen).
    last_total_weight_kg: Option<f32>,
    /// True wenn der Loadsheet-Activity-Log-Eintrag beim Block-off
    /// schon emittet wurde — verhindert Duplikate beim Resume.
    loadsheet_logged_at_blockoff: bool,

    /// Highest MSL altitude we've seen while in Cruise (or any step
    /// climb during Cruise). Drives the Cruise → Descent guard so
    /// short ATC step-downs (FL380 → FL360) don't flip the phase to
    /// Descent — only a real TOD drop of >5000 ft from this peak
    /// counts.
    cruise_peak_msl: Option<f32>,
    /// v0.5.9: peak MSL altitude during the Climb phase. Reset on
    /// Climb entry. Used by the Climb → Descent guard — require
    /// >200 ft loss from this peak before transitioning, so single
    /// VS spikes (turbulence, level-off) don't flip the FSM.
    climb_peak_msl: Option<f32>,
    /// Peak MSL altitude across the entire flight, regardless of
    /// phase. Reported as the PIREP `level` field — phpVMS shows it
    /// as "Flt.Level". Updated on every snapshot during Climb /
    /// Cruise / Descent.
    peak_altitude_ft: Option<f32>,

    /// Rolling 5-second ring buffer of (timestamp, V/S, G, on-ground)
    /// for recovering the *true* touchdown values. The single-tick
    /// "first on_ground=true" snapshot routinely caught a bounce
    /// rebound (positive V/S, 1.0 G), missing the actual impact. The
    /// buffer lets us scan the last few seconds and pick the worst
    /// V/S / G that the aircraft actually saw — independent of the
    /// snapshot rate or SimConnect's PLANE TOUCHDOWN * latching.
    snapshot_buffer: std::collections::VecDeque<TelemetrySample>,
    /// 2.0.14 — die Bildrate des Simulators je Abtastung, die letzten
    /// `BILDRATE_PUFFER_SECS` Sekunden. Nur fuer die Zeile „Bildrate beim
    /// Aufsetzen" im Diagnoselog; geht in keine Bewertung.
    bildrate_puffer: std::collections::VecDeque<(DateTime<Utc>, f32)>,

    /// Inventur 02.10.2026 (B1): Langsame Spur fuer die Ueberflughoehe an der
    /// Schwelle (TCH). Der 5-s-`snapshot_buffer` reicht dafuer nicht: Bei
    /// Verkehrsflugzeugen liegen ~10 s zwischen Schwelle und Aufsetzen — die
    /// Probe an der Schwelle war schon heraus, TCH fehlte bei ~85 % der
    /// Landungen (Wert nur bei ≤ 4,7 s). Hier: ~2,5 Hz, nur im Anflug unter
    /// `SCHWELLENSPUR_MAX_AGL_FT`, die letzten `SCHWELLENSPUR_SECS` Sekunden.
    schwellen_spur: std::collections::VecDeque<TelemetrySample>,

    /// Bis wohin der 50-Hz-Puffer schon in die Spur gelegt wurde.
    ///
    /// Ohne diesen Merker wuerde bei jedem Tick der ganze Ringpuffer
    /// (fuenf Sekunden, rund zweihundertfuenfzig Werte) neu durchgerechnet.
    /// Falsche Punkte gaebe das nicht — der Mindestabstand faengt das ab —
    /// aber es waere Arbeit fuer nichts, fuenfmal je Sekunde.
    bahn_spur_bis: Option<DateTime<Utc>>,

    /// v0.5.39: post-touchdown sample collection. Wenn der Sampler den
    /// on_ground-Edge erkennt, sammelt er Samples in diesem Buffer
    /// für TOUCHDOWN_POST_WINDOW_SECS sec. Sobald gefüllt → wird
    /// zusammen mit `snapshot_buffer` als `TouchdownWindow`-Event
    /// gedumpt und beide Buffer geleert (= ready für ggf. zweite
    /// Landung im Touch-and-Go-Pattern).
    post_touchdown_buffer: std::collections::VecDeque<TelemetrySample>,

    /// v0.5.39 (legacy, write-only seit v0.6.0): Marker dass der Sampler
    /// gerade im Touchdown-Capture-Fenster ist. **Wird nicht mehr aktiv
    /// konsumiert** — der Streamer-Tick ist seit v0.6.0 strukturell vom
    /// phpVMS-IO entkoppelt (Memory-Outbox + eigener Worker), also ist
    /// kein "Pause"-Marker mehr nötig. Bleibt nur für Backward-Compat
    /// mit serialisiertem `active_flight.json` älterer Versionen — kann
    /// in einer späteren Major-Version ersatzlos entfernt werden.
    landing_critical_until: Option<DateTime<Utc>>,

    /// v0.5.39: Wann der TouchdownWindow-Buffer-Dump in die JSONL geflusht
    /// wurde. Verhindert Re-Dump im weiteren Sampler-Loop (Rollout/Taxi-In).
    /// Bleibt bis Flight-Ende bzw. zweiter TD im Touch-and-Go-Pattern (dann
    /// reset durch sampler_touchdown_at).
    touchdown_window_dumped_at: Option<DateTime<Utc>>,

    /// GAF-707-Race, strukturell geloest (Vereinheitlichung 09/2026): siehe
    /// Kommentar am gleichnamigen Feld in `PersistedFlightStats`. Getrennt
    /// von `touchdown_window_dumped_at`, weil letzteres frueh gesetzt wird
    /// (Doppel-Dump-Schutz im Sampler-Loop + Resume-Sicherheit) — DIESES
    /// Feld erst, nachdem wirklich alle score-relevanten Rohdaten (VS-
    /// Kaskade, bounce_count) geschrieben sind. `finalize_landing_score_
    /// if_due` wartet auf DIESES Feld, nicht auf `touchdown_window_dumped_at`.
    touchdown_window_score_data_ready_at: Option<DateTime<Utc>>,

    /// v0.5.40: Anti-Flicker für `engines_running`. Aerosoft A340-600 Pro
    /// (und andere Add-ons) zappeln den `GENERAL ENG COMBUSTION` SimVar
    /// pro Tick zwischen 0 und N (sauber observiert: 27 Wechsel in 7 min
    /// Pushback-Phase). Die Phase-FSM-Bedingung `engines_running > 0`
    /// liefert dann zufällig true/false und blockiert Pushback→TaxiOut.
    /// Hier merken wir uns den Zeitpunkt des letzten >0-Reading und
    /// behandeln Engines als "effektiv laufend" für 2 s nach diesem Tick
    /// (Grace-Window). Ändert nichts am detect_telemetry_changes-Logging
    /// (dort hängt's an der bestehenden 5-s-Debounce).
    last_engines_running_above_zero_at: Option<DateTime<Utc>>,

    /// v0.5.40: Track ob `pushback_state` jemals != 3 war seit Flight-
    /// Start. MSFS PUSHBACK STATE = 3 ist der Default ("kein Pushback
    /// aktiv"). Wenn der Pilot mit GSX/manuell rausgeschoben wird oder
    /// der Sim/das Add-on den Wert nie auf 0/1/2 setzt, würde die FSM
    /// fälschlich `tug_done = (state == Some(3))` als "Pushback ist
    /// abgeschlossen" werten und auf 10-s-Stillstand warten — der nie
    /// kommt wenn der Pilot schon rollt. Lösung: nur dann den 3-State
    /// als "tug detached" werten, wenn vorher mal ein 0/1/2 gesehen
    /// wurde. Sonst Fall-back auf old-school engines+gs Heuristik.
    saw_pushback_state_active: bool,

    /// v0.5.41: True sobald die Score-Berechnung mit den high-res-50-Hz-
    /// Daten aus dem Sampler-Buffer abgeschlossen ist (oder per Fallback-
    /// Timeout 12 s post-TD ohne Buffer abgeschlossen wurde). Vorher
    /// blockiert announce_landing_score die Touchdown-MQTT-Emission, damit
    /// der Live-Monitor nicht den ueberholten msfs_simvar_latched-Wert
    /// sieht der bei Fenix A320 oft 30-50% pessimistischer ist als die
    /// physikalisch repraesentative vs_at_edge_fpm (Volanta-equivalent).
    /// Beispiel DLH-1404 EDDF->LDZA Fenix A320 SL: msfs_simvar_latched
    /// -101 fpm, vs_at_edge -66 fpm = exakt Volanta.
    landing_score_finalized: bool,

    /// v0.5.39: Berechnete Forensik-Analyse aus dem TouchdownWindow-Buffer.
    /// Sampler schreibt sie hier rein wenn der Buffer 10s post-TD gedumpt
    /// wird; der Touchdown-Payload-Builder liest sie aus und packt sie ins
    /// MQTT-Touchdown-Event damit der Live-Monitor die Volanta-/DLHv-
    /// equivalenten VS-Werte und den Flare-Score sieht. None bis Sampler
    /// fertig (typisch 10s post-TD).
    landing_analysis: Option<serde_json::Value>,
    /// QS 06.10.2026: die Abfang-Messung zusätzlich gesichert — `landing_
    /// analysis` überlebt keinen Neustart (nicht in `PersistedFlightStats`),
    /// und ohne sie fehlte nach einer Wiederaufnahme die Teilnote samt Deckel
    /// beim Einreichen. Zurückgesetzt wie `landing_analysis` (Touch-and-Go).
    abfangen_gesichert: Option<landing_scoring::abfangen::Abfangen>,
    /// QS 06.10.2026 (Runde 6): Client-Version, die die Landung gemessen
    /// hat — nur nach einem Neustart gesetzt (aus der Sicherung). Sonst
    /// misst die laufende Version (`aufzeichnende_client_version`). Mit dem
    /// Aufsetzzeitpunkt, für den sie gilt — eine spätere Landung (Touch-and-
    /// Go nach dem Neustart) misst die laufende Version.
    client_version_aufsetzen: Option<(String, DateTime<Utc>)>,

    /// True once the "Aircraft: {title}" banner has been emitted to
    /// the activity log for this flight. Persisted across resumes
    /// so a Tauri restart mid-flight doesn't re-fire the banner — it
    /// belongs to the *flight*, not the session.
    aircraft_banner_logged: bool,

    /// When the streamer last successfully posted a position to phpVMS.
    /// Drives the cockpit's LIVE / REC indicator — a long gap means
    /// the network is having trouble (or the streamer is dead) and
    /// the dashboard should warn the pilot before they file a PIREP
    /// without recent telemetry.
    last_position_at: Option<DateTime<Utc>>,
    /// When the streamer last successfully POSTed `/pireps/{id}/update`.
    /// Surfaced via `flight_status` so a debug panel can show "last
    /// keep-alive Xs ago" — useful to diagnose if the heartbeat that
    /// prevents phpVMS's RemoveExpiredLiveFlights cron from killing the
    /// PIREP is actually getting through.
    last_heartbeat_at: Option<DateTime<Utc>>,
    /// Number of positions currently sitting in the offline queue
    /// waiting to be replayed. >0 means we lost the network briefly
    /// and the dashboard should surface that.
    queued_position_count: u32,

    // ---- Gates / runway (from MSFS ATC SimVars) ----
    /// Parking stand the pilot pushed back from. Captured from the OSM
    /// ground data while still in Boarding (nearest NAMED stand within
    /// `STAND_NAME_RADIUS_M`). Survives across the whole flight.
    dep_gate: Option<String>,
    /// Stand the pilot ended up parked at after arrival. Captured when the
    /// aircraft comes to rest at the arrival stand — by the normal
    /// TaxiIn→BlocksOn edge, or by the universal Arrived-fallback, which skips
    /// BlocksOn entirely. Both go through `settle_at_arrival_stand`.
    arr_gate: Option<String>,
    /// v0.19.3: did the flight actually pass through the `BlocksOn` phase, as
    /// opposed to being promoted straight to `Arrived` by the universal
    /// fallback? The block-on fuel/weight diagnostic keys off this, so it is
    /// persisted (`StatsSnapshot`) — the in-memory transition list is
    /// runtime-only and would read as "never blocked on" after any resume.
    blocks_on_reached: bool,
    /// v0.19.3: the airport `arr_gate` was captured at. A stand name is only
    /// meaningful together with its field — see `arr_gate_for`.
    arr_gate_icao: Option<String>,
    /// v0.19.3: (time, groundspeed) reference for the takeoff-roll acceleration
    /// gate — see `takeoff_roll_detected`. Runtime-only: a resumed flight
    /// re-baselines on its next tick, and a flight resumed mid-taxi has nothing
    /// to gain from a stale sample anyway.
    takeoff_roll_gs_ref: Option<(DateTime<Utc>, f64)>,
    /// Consecutive accelerating windows seen so far (see
    /// `TAKEOFF_ROLL_ACCEL_WINDOWS_NEEDED`). Reset by any slowdown or any window
    /// that fails the threshold.
    takeoff_roll_accel_windows: u8,
    /// v0.19.3: the last position the aircraft was actually seen at — as opposed
    /// to `last_lat/lon`, which is the distance-accounting baseline and is
    /// deliberately cleared on a resume. Nothing clears this one, and it is
    /// persisted: it is what tells the filing gate where the aircraft is when the
    /// sim is gone. See `aircraft_position_for_gates`.
    last_known_lat: Option<f64>,
    last_known_lon: Option<f64>,
    /// Zum selben letzten Punkt: Hoehe, Kurs, Tempo, Zeit. Der Wiederaufnahme-
    /// Dialog braucht sie, damit der Pilot nach einem Sim-Absturz dorthin
    /// zurueckstellen kann (SIA 375, 27.09.2026: nur Koordinaten, Hoehe
    /// und Kurs fehlten).
    last_known_alt_ft: Option<f64>,
    last_known_hdg_deg: Option<f32>,
    last_known_gs_kt: Option<f32>,
    last_known_at: Option<DateTime<Utc>>,
    /// `ATC RUNWAY SELECTED` snapshotted at touchdown. Useful for VAs
    /// that grade "did the pilot land on the right runway".
    approach_runway: Option<String>,
    /// v1.5.0 (#msfs-hud): **geschätzte** Landebahn, laufend im Anflug aus
    /// Position und Steuerkurs gegen die Navdaten bestimmt
    /// (`runway::predict_landing_runway`). Existiert, weil der MSFS-Adapter
    /// `selected_runway` gar nicht abonniert und `approach_runway` deshalb
    /// im Daten-Audit vom 11.06.2026 bei 407 von 407 Flügen leer war.
    ///
    /// Bewusst ein EIGENES Feld statt eines Fallbacks in `approach_runway`:
    /// dessen Wert fließt in den Touchdown- und PIREP-Payload, und dort
    /// hat er bereits einen besseren Fallback auf den Korrelations-Match
    /// nach dem Aufsetzen (97,5 % Trefferquote). Eine Live-Schätzung würde
    /// den verdrängen — messbar schlechtere Daten für eine reine
    /// Anzeigebequemlichkeit. Dieses Feld speist ausschließlich die
    /// Live-Anzeige und den Live-Gleitwinkel.
    ///
    /// Laufzeit-only: wird jeden Tick neu bestimmt, ein fortgesetzter Flug
    /// hat auf seinem nächsten Tick wieder einen Wert.
    predicted_runway: Option<String>,
    /// Publizierter Gleitwinkel zu [`Self::predicted_runway`], bereits auf
    /// 2–7,5° plausibilisiert. `None`, wenn die Navdaten nichts Brauchbares
    /// führen — die Anzeige nimmt dann den 3°-Standard.
    predicted_glideslope_deg: Option<f64>,

    // ---- Edge detector for activity log (Phase H.3) ----
    /// Last value we logged to the activity feed. Used to detect when the
    /// pilot changes a knob and emit one log entry per change rather than
    /// repeating the current state every tick.
    last_logged_squawk: Option<u16>,
    last_logged_com1: Option<f32>,
    last_logged_com2: Option<f32>,
    last_logged_nav1: Option<f32>,
    last_logged_nav2: Option<f32>,
    last_logged_lights: Option<LightsState>,
    /// Pro Lampe: (Beginn des Zählfensters, Wechsel darin, Blinkmodus gemeldet).
    ///
    /// Boeings können Landelichter auf FLASH stellen — die Lampe blinkt dann
    /// bauartbedingt. Am Flug EDDP→LTFE (25.07.2026) wurden dadurch **52
    /// Wechsel** protokolliert, 50 davon in fünf Minuten, Median-Halteperiode
    /// 3,0 s. Fünfzig Zeilen "Landing lights ON/OFF" tragen keine Information
    /// und begraben alles andere im Protokoll. Statt jeder Flanke wird das
    /// Blinken einmal als solches benannt.
    light_flicker: std::collections::HashMap<&'static str, (DateTime<Utc>, u8, bool)>,
    /// 3-state strobe selector (0=OFF, 1=AUTO, 2=ON) for aircraft
    /// addons that distinguish AUTO from ON. Lives separate from
    /// `last_logged_lights.strobe` so we don't log a "Strobe lights
    /// ON" entry on top of the more informative "Strobe lights AUTO"
    /// one. Not persisted across resumes — change-detection state
    /// always rebuilds from the first post-resume tick.
    last_logged_strobe_state: Option<u8>,
    last_logged_ap: Option<ApState>,
    /// Debounce: when did we first observe the *current* AP master state?
    /// We only emit a "Autopilot ENGAGED/OFF" log entry if the new state
    /// has held for `AP_DEBOUNCE_SECS`. Stops a misbehaving LVar (pulsed
    /// momentary buttons, sim-engine restarts) from flooding the log.
    pending_ap_master: Option<bool>,
    pending_ap_master_since: Option<DateTime<Utc>>,
    /// A/THR engaged state (v0.16.4 — generic autothrottle, fed by the
    /// A346 ATHR annunciator LVar + Fenix `L:S_FCU_ATHR`). Same
    /// debounce treatment as the AP master: Fenix `S_FCU_*` LVars are
    /// known button-PULSE candidates (B-008), the debounce turns a
    /// pulse into "no entry" instead of an ENGAGED/OFF pair.
    last_logged_autothrottle: Option<bool>,
    pending_autothrottle: Option<bool>,
    pending_autothrottle_since: Option<DateTime<Utc>>,
    last_logged_parking_brake: Option<bool>,
    last_logged_engines_running: Option<u8>,
    /// Pending engine-count change waiting for stability — same
    /// debounce trick as AP_DEBOUNCE_SECS. Fenix in particular
    /// flashes the GENERAL ENG COMBUSTION SimVar during engine
    /// starts and at idle, which fired phantom "Engine shutdown /
    /// started" pairs in the log.
    pending_engines_running: Option<u8>,
    pending_engines_running_since: Option<DateTime<Utc>>,
    last_logged_stall: Option<bool>,
    last_logged_overspeed: Option<bool>,
    /// Last aircraft profile we logged. If the pilot loads a different
    /// airframe mid-session (e.g. swaps from default A20N to Fenix), the
    /// detector flips and we want to emit a one-line announcement so the
    /// user sees the LVar mapping changed.
    last_logged_profile: Option<sim_core::AircraftProfile>,
    /// Flaps-handle detent the last time we emitted a log entry. Stored
    /// as the integer detent (0–5 on Airbus, 0/1/2/3/4 on Boeing —
    /// derived from `flaps_position * 5` and rounded). One log per real
    /// detent change rather than every 0.001 of jitter.
    last_logged_flaps_detent: Option<u8>,
    /// PMDG-Premium-First flap label the last time we emitted a log
    /// entry. Stored as the cockpit-exact Boeing label ("UP"/"1"/"5"/
    /// "15"/"30"/etc.) when a PMDG aircraft is loaded; cleared when
    /// the pilot switches to a non-PMDG aircraft so the legacy
    /// `last_logged_flaps_detent` path takes back over without
    /// firing a duplicate "Flaps ↓ X" entry.
    last_logged_flap_label: Option<String>,
    /// Gear command position discretised — 0 = up, 1 = down. The
    /// 0..1 SimVar value is rounded so a moving gear-handle doesn't
    /// flood the log mid-cycle.
    last_logged_gear_down: Option<bool>,
    /// Speed-brake handle deployed (>5%) — distinct from "armed".
    last_logged_spoilers_deployed: Option<bool>,
    last_logged_spoilers_armed: Option<bool>,
    last_logged_apu_running: Option<bool>,
    last_logged_battery_master: Option<bool>,
    last_logged_avionics_master: Option<bool>,
    last_logged_pitot_heat: Option<bool>,
    last_logged_engine_anti_ice: Option<bool>,
    last_logged_wing_anti_ice: Option<bool>,
    /// Seat belts sign (0=OFF, 1=AUTO, 2=ON). Logged on transitions.
    last_logged_seatbelts_sign: Option<u8>,
    /// No smoking sign (0=OFF, 1=AUTO, 2=ON).
    last_logged_no_smoking_sign: Option<u8>,
    /// v1.5.3 (#ifly-audit): Entprellen der generischen Flap-Logzeile.
    /// Beleg aus der Live-DB (RYR1142, 11.08.2026): der iFly-Hebelwert
    /// springt beim Einfahren 5->UP fuer exakt 3 Sekunden auf 1.0
    /// ("Flaps FULL" bei 217 kt im Log). Ein Label wird erst geloggt,
    /// wenn es FLAP_LOG_STABLE_SECS stabil steht — echte Rastenwechsel
    /// stehen ohnehin, der Spike faellt durch. Gleiches Muster wie der
    /// Engine-Debounce gegen Fenix' Zuend-Pulse.
    pending_flap_label: Option<String>,
    pending_flap_since: Option<DateTime<Utc>>,
    /// Prozentwert zum zuletzt GELOGGTEN Label — nur fuer den
    /// Richtungspfeil (ausfahren/einfahren) des naechsten Eintrags.
    last_logged_flap_pos: Option<f32>,
    /// Autobrake setting label ("OFF" / "LO" / "MED" / "MAX") for
    /// the activity log. Reset on every flight.
    last_logged_autobrake: Option<String>,

    // ---- PMDG SDK tracking (Phase H.4 / v0.2.0) ----
    /// Once-per-flight banner: "PMDG 737-800 detected — premium
    /// telemetry active". Fires the first tick we see PmdgState
    /// in the snapshot, not before (so e.g. the user starts
    /// AeroACARS, then loads PMDG → the banner appears at PMDG
    /// load, not at app start).
    pmdg_detected_logged: bool,
    /// Once-per-flight V-speeds banner — fires when the FMC
    /// finally has all four (V1, VR, V2, VREF) populated. We log
    /// them once at that moment so the timeline shows when the
    /// pilot completed the perf-init.
    pmdg_v_speeds_logged: bool,
    /// FMA mode tracking — combined string ("N1 / VNAV / LNAV")
    /// so a single change in any sub-mode produces one log entry
    /// rather than three.
    last_logged_pmdg_fma: Option<String>,
    /// MCP selected speed — tracked rounded to nearest knot.
    last_logged_pmdg_mcp_speed: Option<i16>,
    /// MCP selected heading.
    last_logged_pmdg_mcp_heading: Option<u16>,
    /// MCP selected altitude.
    last_logged_pmdg_mcp_altitude: Option<u16>,
    /// MCP selected V/S.
    last_logged_pmdg_mcp_vs: Option<i16>,
    /// AT armed state.
    last_logged_pmdg_at_armed: Option<bool>,
    /// AP engaged state (CMD A or B).
    last_logged_pmdg_ap_engaged: Option<bool>,
    /// Takeoff config warning (one-shot — only logs the on-edge,
    /// not the off-edge).
    last_logged_pmdg_to_warning: Option<bool>,

    // ---- v0.2.2 — wider PMDG integration ----
    /// FMC thrust limit mode label tracking (777-specific). Logged
    /// on every change — TO → CLB → CRZ → CON arc tells the VA
    /// admin the pilot worked the thrust schedule properly.
    last_logged_pmdg_thrust_mode: Option<String>,
    /// 777 ECL phase completion tracking — one slot per phase.
    /// Logged on rising edge (false→true) so the activity log
    /// shows "ECL: Preflight complete" once per phase.
    last_logged_pmdg_ecl: [bool; 10],
    /// PMDG-authoritative APU-running bit (777). Distinct from
    /// the standard-SimVar APU tracking which uses RPM heuristics.
    last_logged_pmdg_apu: Option<bool>,
    /// Wheel chocks set (777). Pre-flight ground state.
    last_logged_pmdg_chocks: Option<bool>,
    /// PMDG transponder mode label (v0.2.4). Cockpit-exact mode
    /// selector position — STBY/ALT-OFF/XPNDR/TA/TA-RA — which the
    /// standard `TRANSPONDER STATE` SimVar can't expose.
    last_logged_pmdg_xpdr_mode: Option<String>,

    // ---- PMDG PIREP captures (Phase 5.6 / v0.2.0) ----
    /// PMDG aircraft variant label captured once at flight start.
    /// "737-800" / "737-800 SSW" / "777-300ER" / etc. Surfaced in
    /// the PIREP custom fields as "Aircraft Variant".
    pmdg_variant_label: Option<String>,
    /// V-speeds captured at takeoff roll start (= once
    /// `takeoff_at` is stamped) so the PIREP records the values
    /// the pilot actually rotated against. (V1, VR, V2)
    pmdg_v_speeds_takeoff: Option<(u8, u8, u8)>,
    /// VREF captured at the touchdown moment for the PIREP.
    pmdg_vref_at_landing: Option<u8>,
    /// Vref aus dem Flugzeug (PMDG, iniBuilds, FSS, Zibo) oder die Airbus-VLS
    /// in Landestellung — letzter Wert in der Luft, mit Quelle.
    vref_gemessen: Option<(f32, &'static str)>,
    /// FMC TO-flaps degrees captured at takeoff roll start.
    pmdg_takeoff_flaps_planned: Option<u8>,
    /// FMC LDG-flaps degrees captured at landing entry.
    pmdg_landing_flaps_planned: Option<u8>,
    /// Takeoff-config-warning was active at any point during
    /// TakeoffRoll. PIREP flags this as a discipline issue.
    pmdg_takeoff_config_warning_seen: bool,
    /// Autobrake setting at touchdown.
    pmdg_autobrake_at_landing: Option<String>,
    /// FMC flight number captured at takeoff (for sanity check
    /// against the bid's flight number).
    pmdg_fmc_flight_number: Option<String>,
    /// 777-specific: ECL phases marked complete during the flight.
    /// Captured continuously — final value is the union of all
    /// phases that were ever ticked (NG3 stays None).
    pmdg_ecl_phases_complete: Option<[bool; 10]>,

    // ---- v0.16.10 Premium-Cockpit-Daten — Activity-Log-Tracking ----
    // Quellen: die profile-gated LVar-Mapper (Fenix/FBW/INI-A350/INI-
    // A340/Aerosoft-A346/MD-11) + die PMDG-Premium-Override-Mapper.
    // Alle Tracker folgen dem bestehenden Idiom: erster Wert latcht
    // STUMM (kein Eintrag beim Sim-Connect), danach nur Transitionen —
    // debounced wo Flacker-Gefahr besteht (Annunciator-Tests, LVar-
    // Pulse). Nicht persistiert — Change-Detection-State baut sich nach
    // Resume aus dem ersten Tick neu auf (wie last_logged_* oben).
    /// Generic-FMA-Zeile (lateral | vertical | thrust) — NUR wenn
    /// `snap.pmdg` None ist; PMDG hat seinen eigenen FMA-Logger.
    premium_fma: DebouncedLogState<String>,
    /// Takeoff-V-Speeds (V1/VR/V2 gerundet + FLEX gerundet). Erste
    /// Komplett-Belegung loggt (= FMS-Perf-Init-Zeitstempel), danach
    /// re-log nur bei Wertänderung. Das "· TOGA"-Suffix ist bewusst
    /// NICHT Teil des Latch-Keys — eine Lever-Bewegung nach TOGA soll
    /// die V-Speeds-Zeile nicht erneut loggen.
    last_logged_vspeeds: Option<(i64, i64, i64, Option<i64>)>,
    /// Approach-Speeds (VAPP/VLS/VREF) — nur in Descent/Approach/Final
    /// geloggt. v0.16.10 QS (B1): VLS/VAPP werden auf FBW/INI laufend
    /// aus Gewicht+Konfiguration berechnet → 1-kt-Drift + Flap-Sweep
    /// erzeugten 20-70 Zeilen pro Approach. Der Latch-Key ist deshalb
    /// auf 5-kt-Buckets pro Komponente quantisiert UND debounced
    /// (10 s) — der ERSTE Some-Wert loggt weiterhin sofort
    /// (`update_first_logs`, = FMS-Perf-Eingabe-Zeitstempel).
    premium_approach_speeds: DebouncedLogState<(Option<i64>, Option<i64>, Option<i64>)>,
    /// MASTER CAUTION — Debounce 2 s (Annunciator-Flackern bei
    /// Cockpit-Tests am Boden), erster Wert stumm, nur Transitionen.
    premium_master_caution: DebouncedLogState<bool>,
    /// MASTER WARNING — gleiche Behandlung wie MASTER CAUTION.
    premium_master_warning: DebouncedLogState<bool>,
    /// Seit wann `snap.stall_warning` ununterbrochen `true` ist — Debounce
    /// nur fuer den MQTT-Live-Payload (STALL_WARNING_MQTT_DEBOUNCE_SECS).
    /// Der Accident-Classifier (approach_stall_warning_count, via
    /// step_flight/Approach-Buffer befuellt) liest weiterhin den rohen
    /// SimVar-Wert und ist davon unberuehrt.
    stall_mqtt_true_since: Option<DateTime<Utc>>,
    /// Umkehrschub — einmal pro Landung; re-arm sobald airborne
    /// (Touch-and-Go bekommt so einen eigenen Eintrag).
    last_seen_reverser: Option<bool>,
    reverser_logged_this_landing: bool,
    /// Ground-Spoiler — einmal pro Landung, re-arm airborne.
    last_seen_ground_spoilers: Option<bool>,
    ground_spoilers_logged_this_landing: bool,
    /// Baro STD/QNH — erster Wert latcht stumm, danach Transitionen.
    last_logged_baro_std: Option<bool>,
    /// Flugzeug-eigene FWC-Phase (nur A346) — Debounce 5 s, erster Wert stumm.
    premium_aircraft_phase: DebouncedLogState<String>,
    /// Below-G/S-Alert — einmal pro Approach; re-arm wenn der Alert
    /// > BELOW_GS_REARM_SECS geklärt ist oder die FlightPhase wechselt.
    last_seen_below_gs: Option<bool>,
    below_gs_logged_this_approach: bool,
    below_gs_clear_since: Option<DateTime<Utc>>,
    below_gs_armed_phase: Option<FlightPhase>,
    /// Kabinenhöhen-Warnung — nur Rising-Edge, erster Wert stumm.
    last_logged_cabin_alt_warning: Option<bool>,
    /// Stab-out-of-trim — Debounce 5 s, nur Rising-Edge geloggt.
    premium_stab_out_of_trim: DebouncedLogState<bool>,
    /// Schubhebel-Detent (INI: TOGA/FLX-MCT/CL) — Debounce 3 s,
    /// erster Wert latcht stumm. Profile ohne Gate-LVar liefern None
    /// und loggen nie.
    premium_thrust_gate: DebouncedLogState<String>,
    /// v0.16.10 QS (M5): Connect-Grace für die Premium-Logger. Beim
    /// Sim-Connect lesen die Addon-LVars 0.0 bis das WASM-Modul sie
    /// befüllt — Tick 1 latchte dann Bogus-Werte (Baro "QNH",
    /// MASTER CAUTION false, …), Tick 2 loggte eine Phantom-Transition
    /// ("Baro: STD gesetzt" / "MASTER CAUTION" direkt nach Connect/
    /// Resume). Fix: nach jedem Stream-(Re-)Start ignorieren ALLE
    /// Premium-Logger die Snapshots für `PREMIUM_LOG_WARMUP_SECS` —
    /// erst danach latcht der erste (jetzt echte) Wert stumm.
    /// Re-Start-Erkennung über die Tick-Lücke (`premium_log_last_tick_at`
    /// älter als `PREMIUM_LOG_STREAM_GAP_SECS` — der Streamer tickt
    /// normal alle 0,5-3 s, eine Lücke heißt Disconnect/Pause/Resume).
    /// Nicht persistiert: nach App-Restart ist None → Warmup greift.
    premium_log_last_tick_at: Option<DateTime<Utc>>,
    premium_log_warmup_until: Option<DateTime<Utc>>,

    // ---- v0.16.10 Fuel-Used-Fallback (FOB-Delta) ----
    /// FOB-Baseline beim Flugstart (erster Tick mit Fuel an Bord).
    /// PMDG liefert FUEL TOTAL QUANTITY USED nicht (Live-Befund
    /// GLO4587 2026-06-11) — `snap.fuel_used_kg` wird im Streamer aus
    /// `initial_fob_kg − fuel_total_kg` abgeleitet wenn die SimVar tot
    /// ist. PERSISTIERT (PersistedFlightStats), damit ein App-Restart
    /// mid-flight die Baseline nicht verliert. Re-latcht auf den neuen
    /// Maximalwert wenn FOB > 50 kg über die Baseline steigt
    /// (Mid-Flight-Refuel → keine negativen Deltas).
    initial_fob_kg: Option<f32>,
    // v0.7.13: FCU-Debounce-Felder entfernt — der geplante Switch auf
    // `AUTOPILOT * VAR` SimVars wurde verworfen, der Fenix-LVar-Variant
    // ist als Encoder-Click-Counter unzuverlaessig. Falls FCU-Logging
    // doch jemals revived wird: aus git history rausziehen (commit
    // ggf vor v0.7.13). Audit Q4-2026-05.

    // ─── v0.7.19 GAF-707 Accident-Detection ───────────────────────────
    //
    // Latched-Zustand: gesetzt durch den Sim-Event-Pfad (SOFORT bei
    // `snap.crashed=true`) oder durch den Heuristik-Pfad (am Touchdown-
    // Edge in build_landing_record). Beide Pfade duerfen sich
    // ueberlagern — `SimCrash` ist authoritativ fuer `kind`, Heuristik-
    // Gruende werden gemerged in `accident_reasons`.
    //
    // `CrashReset` aus dem MSFS-Adapter loescht diese Felder NICHT —
    // sie persistieren bis Flight-End/Cleanup/neuer Flight-Start. Spec
    // docs/spec/v0.7.19-gaf707-crash-accident-detection.md §Leit-
    // entscheidung 6.
    accident_detected: bool,
    /// Wann wurde der Accident gelatcht. Sim-Event-Pfad: kann mehrere
    /// Sekunden vor `landing_at` liegen. Heuristik-Pfad: ≈ `landing_at`.
    accident_at: Option<DateTime<Utc>>,
    /// "sim_crash" | "impact" | "off_airport_impact"
    accident_kind: Option<String>,
    /// "high" (Confirmed) | "medium" (Suspected)
    accident_confidence: Option<String>,
    /// Lesbare Begruendung pro Heuristik-Marker. Wird mehrfach
    /// erweitert wenn beide Pfade greifen (z. B. Sim-Event +
    /// Heuristik) und der Worst-Case-Touchdown gewinnt.
    accident_reasons: Vec<String>,
}

/// What kind of touchdown this is from the pilot's perspective.
/// Discriminates real landings from training touch-and-goes so the
/// PIREP score doesn't get unfairly dragged down by deliberate
/// multi-touch flights.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TouchdownKind {
    /// The actual end-of-flight landing. The PIREP `landing_*`
    /// native fields and `landing_score` are derived from this one.
    /// Always the LAST entry in `touchdown_events` when a flight
    /// has been filed normally.
    FinalLanding,
    /// Training / planned multi-touch — aircraft touched the runway
    /// briefly, then climbed back out under power. Recorded for the
    /// audit trail but does NOT influence the score and does NOT
    /// count toward `bounce_count`.
    TouchAndGo,
}

/// One touchdown event during a flight. Captured at the moment the
/// touchdown is classified (= after `FINAL_LANDING_VERBUCHEN_NACH_SECS`
/// on the ground or when rolling off for a normal landing, or when
/// `landung_beim_wegsteigen_abschliessen` closes it on climb-out).
/// Snapshot semantics — the values reflect the analyzer state at
/// classification time and don't update afterwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TouchdownEvent {
    pub timestamp: DateTime<Utc>,
    pub kind: TouchdownKind,
    /// Worst (= most negative) V/S observed in the touchdown window.
    pub peak_vs_fpm: f32,
    /// Highest G-force in the touchdown window.
    pub peak_g: f32,
    /// Touchdown coordinates so the PIREP-detail map can plot all
    /// touchdowns separately.
    pub lat: f64,
    pub lon: f64,
    /// Number of sub-bounces observed for THIS touchdown (= AGL
    /// excursions above the bounce threshold within the bounce
    /// window). A clean T&G has 0 sub-bounces; a sloppy flare
    /// might have 1-2.
    pub sub_bounces: u8,
}

/// Categorised assessment of a touchdown. Computed from peak descent
/// rate, peak G-force and bounce count once the touchdown window
/// closes. Stored on `FlightStats` and shipped both to the activity log
/// and the PIREP custom-fields map for VA review.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum LandingScore {
    /// |V/S| < 60 fpm, G < 1.2, no bounce — butter.
    Smooth,
    /// |V/S| < 240 fpm, G < 1.4 — typical good landing.
    Acceptable,
    /// |V/S| < 600 fpm or G < 1.8 — firm but not damaging.
    Firm,
    /// |V/S| < 1000 fpm or G < 1.8–2.5 — hard landing, inspection due.
    Hard,
    /// |V/S| ≥ 1000 fpm or extreme G — likely damaged.
    Severe,
}

impl LandingScore {
    fn classify(peak_vs_fpm: f32, peak_g: f32, bounces: u8) -> Self {
        let vs = peak_vs_fpm.abs();
        // Severity climbs with either V/S OR G — whichever is worse wins.
        let by_vs = if vs >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_vs_severe_fpm.wert {
            Self::Severe
        } else if vs >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_vs_hard_fpm.wert {
            Self::Hard
        } else if vs >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_vs_firm_fpm.wert {
            Self::Firm
        } else if vs >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_vs_smooth_fpm.wert {
            Self::Acceptable
        } else {
            Self::Smooth
        };
        let by_g = if peak_g >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_g_severe.wert {
            Self::Severe
        } else if peak_g >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_g_hard.wert {
            Self::Hard
        } else if peak_g >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_g_firm.wert {
            Self::Firm
        } else if peak_g >= crate::grenzwerte::grenzwerte().ausrollen.touchdown_g_smooth.wert {
            Self::Acceptable
        } else {
            Self::Smooth
        };
        let mut score = by_vs.max(by_g);
        // Bounces bump the score one step down (Smooth → Acceptable etc.)
        // unless we're already at Severe.
        if bounces > 0 && score < Self::Severe {
            score = match score {
                Self::Smooth => Self::Acceptable,
                Self::Acceptable => Self::Firm,
                Self::Firm => Self::Hard,
                Self::Hard | Self::Severe => Self::Severe,
            };
        }
        score
    }

    fn label(self) -> &'static str {
        match self {
            Self::Smooth => "smooth",
            Self::Acceptable => "acceptable",
            Self::Firm => "firm",
            Self::Hard => "hard",
            Self::Severe => "severe",
        }
    }

    /// Severity ordering — for `max()` comparisons in `classify`.
    fn severity(self) -> u8 {
        match self {
            Self::Smooth => 0,
            Self::Acceptable => 1,
            Self::Firm => 2,
            Self::Hard => 3,
            Self::Severe => 4,
        }
    }

    /// Numeric score 0..100 for the phpVMS `score` field. Higher is
    /// better. Calibrated so a perfectly butter-smooth touchdown
    /// scores 100 and a structural-damage event scores ~0 — VAs that
    /// publish leaderboards then sort by score in the obvious way.
    fn numeric(self) -> i32 {
        match self {
            Self::Smooth => 100,
            Self::Acceptable => 80,
            Self::Firm => 60,
            Self::Hard => 30,
            Self::Severe => 0,
        }
    }
}

impl PartialOrd for LandingScore {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for LandingScore {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.severity().cmp(&other.severity())
    }
}

/// Snapshot of the exterior lights we track. Compared as a whole so we
/// can emit one log entry per "lights change" rather than one-per-light
/// on a config transition.
///
/// Wing + wheel-well were added in v0.2.4 as part of the PMDG Premium-
/// First sweep. They're optional in spirit because most generic SimVar
/// profiles don't expose them — the activity-log path checks the
/// underlying `light_wing` / `light_wheel_well` for `Some(_)` before
/// emitting a transition entry to avoid noise on aircraft that don't
/// report those switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct LightsState {
    landing: bool,
    beacon: bool,
    strobe: bool,
    taxi: bool,
    nav: bool,
    logo: bool,
    /// PMDG-only on most installations — generic SimVars don't expose
    /// the wing-illumination switch separately on Boeings.
    wing: bool,
    /// NG3-only bonus (737 has a dedicated wheel-well light switch).
    wheel_well: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ApState {
    master: bool,
    heading: bool,
    altitude: bool,
    nav: bool,
    approach: bool,
}

impl FlightStats {
    fn new() -> Self {
        Self {
            phase: FlightPhase::Boarding,
            ..Self::default()
        }
    }
}

/// Cached divert detection result, surfaced via `flight_status` so the
/// cockpit UI can render a "you landed at X, not the planned Y"
/// banner with action buttons. Populated once per flight when the FSM
/// reaches Arrived AND the aircraft is not on the planned field.
///
/// The type itself lives in [`crate::arrival`], which is the only module that
/// can construct one — see the invariant documented there ("a divert can never
/// name the planned airport"). Re-exported here because it is part of the
/// `flight_status` payload the cockpit consumes.
pub use arrival::DivertHint;

/// v0.12.5 (Spec v0.12.5-divert-and-manual-pirep.md, LE3): Vorbefüll-
/// Werte für das manuelle Filing-Formular (`ManualFileDialog`). Alle aus
/// FSM-Stats bzw. `DivertHint` — der Pilot korrigiert nur Abweichungen,
/// statt jedes Feld blank einzutippen. Alle Werte optional; `null` =
/// FSM hat den Wert (noch) nicht.
#[derive(Serialize)]
pub struct ManualFilingDefaults {
    /// FSM-gemessene Distanz in NM.
    distance_nm: Option<f64>,
    /// FSM Peak-Altitude, auf 100 ft gerundet.
    cruise_level_ft: Option<i32>,
    /// Flugzeit Takeoff→Landing in Minuten.
    flight_time_minutes: Option<i32>,
    /// FSM Touchdown-Rate (canonical edge value).
    landing_rate_fpm: Option<f32>,
    /// FSM Block-Off-Zeit (RFC-3339).
    block_off_at: Option<String>,
    /// FSM Block-On-Zeit (RFC-3339).
    block_on_at: Option<String>,
    /// FSM Block-Fuel in kg.
    block_fuel_kg: Option<f32>,
    /// Resttreibstoff im Tank (kg) — letzter beobachteter Sim-Fuel.
    /// LE4: das UI-Feld fragt „noch im Tank", nicht „verbraucht".
    remaining_fuel_kg: Option<f32>,
    /// Erkanntes Ausweich-ICAO aus dem `DivertHint`. None = kein Divert.
    divert_to: Option<String>,
    /// Der ursprünglich geplante Zielflughafen (Bid/Flugplan).
    planned_arr_icao: String,
    /// true wenn ein Divert erkannt wurde → Begründung ist Pflicht.
    requires_reason: bool,
}

/// v0.21 (#msfs-panel): live cockpit telemetry for the MSFS in-sim toolbar
/// panel's approach-monitor view. Sourced 1:1 from the current
/// `SimSnapshot` — no new computation, just the subset the panel's
/// live_telemetry card needs (docs/spec/msfs-ingame-landing-debrief-panel.v1.yaml
/// ui_requirements.approach_monitor_view). `None` on `ActiveFlightInfo`
/// whenever the sim isn't currently reachable (mirrors how `sim_fuel_kg`
/// etc. already go stale/absent on disconnect).
#[derive(Debug, Clone, Serialize)]
pub struct PanelLiveSnapshot {
    vertical_speed_fpm: f32,
    g_force: f32,
    ias_kt: f32,
    gs_kt: f32,
    pitch_deg: f32,
    bank_deg: f32,
    wind_dir_deg: Option<f32>,
    wind_speed_kt: Option<f32>,
    oat_c: Option<f32>,
    /// v1.5.0 (#msfs-hud): Höhe über Grund. Die mit Abstand wichtigste
    /// Ergänzung für die Anflug-Anzeige — ohne sie hat eine Sinkrate keinen
    /// Bezugspunkt.
    altitude_agl_ft: f64,
    /// v1.5.0 (#msfs-hud): Windkomponenten auf die Flugzeugachsen gedreht,
    /// wie sie MSFS nativ liefert. **Exakt dieselbe Vorzeichenkonvention wie
    /// `landing_headwind_kt`/`landing_crosswind_kt` im Landeprotokoll**
    /// (positiv = Gegenwind bzw. Wind von rechts) — sonst zeigte der Anflug
    /// andere Vorzeichen als das Ergebnis zwei Minuten später.
    ///
    /// `None` bei Sims und Addons, die die SimVars nicht führen.
    headwind_kt: Option<f32>,
    crosswind_kt: Option<f32>,
    /// v1.5.0 (#msfs-hud): Tankinhalt für die Boarding-Anzeige.
    fuel_total_kg: f32,
    /// v1.5.1 (#hud-pilotenfeedback F1): Barometrik für die
    /// Flight-Level-Anzeige. `altitude_msl_ft` ist die wahre Höhe (bisher
    /// hatte das Panel oberhalb des Anflugs nur AGL — über den Alpen ist
    /// das eine seltsame Zahl), `altitude_pressure_ft` die Druckhöhe, aus
    /// der das Panel FLxxx rechnet, `qnh_hpa` erlaubt die
    /// Standard-QNH-Erkennung (1013 → Pilot fliegt nach FL, egal wie hoch).
    altitude_msl_ft: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    altitude_pressure_ft: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    qnh_hpa: Option<f32>,
    /// v1.5.1 (#hud-pilotenfeedback F3): Restflugzeit in Minuten,
    /// Großkreis zum Ziel / Groundspeed. `None` am Boden und unter 30 kt
    /// (Lehre aus v0.19.3: NIE `distance_nm` benutzen — das ist die schon
    /// GEFLOGENE Strecke; damit startete die ETA einst bei null und wuchs
    /// den ganzen Flug). Berechnung in [`panel_ete_min`], gesetzt im
    /// `flight_status`-Command, weil `from_snapshot` den Zielflughafen
    /// nicht kennt.
    #[serde(skip_serializing_if = "Option::is_none")]
    ete_min: Option<u32>,
}

impl PanelLiveSnapshot {
    fn from_snapshot(snap: &SimSnapshot) -> Self {
        Self {
            vertical_speed_fpm: snap.vertical_speed_fpm,
            g_force: snap.g_force,
            ias_kt: snap.indicated_airspeed_kt,
            gs_kt: snap.groundspeed_kt,
            pitch_deg: snap.pitch_deg,
            bank_deg: snap.bank_deg,
            wind_dir_deg: snap.wind_direction_deg,
            wind_speed_kt: snap.wind_speed_kt,
            oat_c: snap.outside_air_temp_c,
            altitude_agl_ft: snap.altitude_agl_ft,
            // Z ist so vorzeichenbehaftet, dass positiv = Rückenwind heißt,
            // Gegenwind ist also -Z. Siehe die identische Ableitung im
            // Touchdown-Pfad (`stats.landing_headwind_kt`).
            headwind_kt: snap.aircraft_wind_z_kt.map(|z| -z),
            crosswind_kt: snap.aircraft_wind_x_kt,
            fuel_total_kg: snap.fuel_total_kg,
            altitude_msl_ft: snap.altitude_msl_ft,
            altitude_pressure_ft: snap.altitude_pressure_ft,
            qnh_hpa: snap.qnh_hpa,
            ete_min: None,
        }
    }
}

/// v1.5.1 (#hud-pilotenfeedback F3): Restflugzeit für das In-Sim-HUD.
///
/// Portiert aus `LiveMapView.tsx` (`nav.eta`), inklusive der dort teuer
/// gelernten Regel: Großkreis Flugzeug→Ziel, NICHT die akkumulierte
/// `distance_nm` (v0.19.3-Fehlbild: ETA begann bei null und wuchs — dem
/// Piloten im Endanflug wurde eine weitere Stunde versprochen). Dasselbe
/// 30-kt-Tor wie dort: darunter ist Groundspeed Rollverkehr, keine
/// Reisegeschwindigkeit, und die Division ergäbe Phantasiewerte.
fn panel_ete_min(
    snap: &SimSnapshot,
    arr_icao: &str,
    planned_flight_min: Option<i32>,
) -> Option<u32> {
    if snap.groundspeed_kt <= 30.0 {
        return None;
    }
    let (alat, alon) = runway::airport_position(arr_icao)?;
    let dist_nm = runway::distance_nm(snap.lat, snap.lon, alat, alon);
    let min = (dist_nm / f64::from(snap.groundspeed_kt)) * 60.0;
    // Über 24 h ist es kein Flug mehr, sondern ein Messfehler.
    if !min.is_finite() || min < 0.0 || min > 1440.0 {
        return None;
    }
    // v1.5.3 (Thomas' Feldbefund: "sehr hoch beim Start"): erst zeigen,
    // wenn die Hochrechnung unter das 1,3-fache der GEPLANTEN Flugzeit
    // faellt. Im Steigflug mit halber Reise-GS ist die Momentan-ETE
    // rechnerisch korrekt, aber als Prognose wertlos — dann lieber
    // keine Zahl (die Zeitzelle rotiert derweil mit FLT weiter). Ohne
    // Planzeit (Freiflug) bleibt das alte Verhalten.
    if let Some(plan) = planned_flight_min {
        if plan > 0 && min > f64::from(plan) * 1.3 {
            return None;
        }
    }
    Some(min.round() as u32)
}

#[cfg(test)]
#[path = "tests_lib/panel_ete_tests.rs"]
mod panel_ete_tests;

#[cfg(test)]
#[path = "tests_lib/panel_live_snapshot_tests.rs"]
mod panel_live_snapshot_tests;

#[derive(Serialize)]
pub struct ActiveFlightInfo {
    pirep_id: String,
    bid_id: i64,
    started_at: String,
    airline_icao: String,
    planned_registration: String,
    /// ICAO type of the phpVMS-planned aircraft (e.g. "A320"), looked up via
    /// the same `get_aircraft` call as `planned_registration`. Empty when no
    /// matching bid/OFP aircraft could be resolved.
    aircraft_icao: String,
    /// Full type name of the phpVMS-planned aircraft (e.g. "Airbus A320-200").
    /// Field feedback (2026-08-03): the Karte-Flugwerte-card footer used to
    /// show the SIM's own `aircraft_icao`/`aircraft_registration` (from
    /// `SimSnapshot`) here — wrong source, since the pilot wants to see what
    /// phpVMS actually planned/booked, not whatever's loaded in the sim.
    aircraft_name: String,
    flight_number: String,
    /// `Bid.flight.callsign` (z.B. "7ME"), falls phpVMS es fuellt — reuses
    /// `ActiveFlight::bid_callsign` (bislang nur fuer den OFP-Match genutzt,
    /// siehe dessen Doc-Kommentar). UI-Komponenten sollen dies dem
    /// `airline_icao + flight_number`-Callsign VORZIEHEN, exakt wie phpVMS'
    /// eigener `Flight::atc()`-Accessor (`app/Models/Flight.php`): Freiflug-
    /// Bookings (DisposableSpecial) speichern oft `flight_number=0` (kein
    /// FIXBARER Formelwert — anders als z.B. Block-Zeiten) und tragen den
    /// echten Identifier stattdessen im Callsign. `${icao}${flight_number}`
    /// ohne diesen Fallback zeigte dann z.B. "CFG0" statt "CFG7ME" (Pilot-
    /// Befund Ralf T., GSG0016, 2026-07-28).
    #[serde(skip_serializing_if = "Option::is_none")]
    callsign: Option<String>,
    dpt_airport: String,
    arr_airport: String,
    distance_nm: f64,
    position_count: u32,
    /// snake_case name of the current `FlightPhase` (e.g. "boarding", "climb").
    phase: String,
    /// v0.16.12 (#phase-v2): snake_case-Phase der SCHATTEN-Engine v2 —
    /// reine Beobachtung (Verifikations-Hilfe in der UI), nie
    /// entscheidungsrelevant. None bis der erste Schatten-Tick lief.
    #[serde(skip_serializing_if = "Option::is_none")]
    shadow_phase: Option<String>,
    /// v0.16.12 (#phase-v2): Fenster-Segment der Schatten-Engine
    /// ("level" während einer ATC-Restriction etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    shadow_segment: Option<String>,
    /// ISO-8601 timestamps captured at major flight events. Each is `null`
    /// until the corresponding transition fires.
    block_off_at: Option<String>,
    takeoff_at: Option<String>,
    landing_at: Option<String>,
    block_on_at: Option<String>,
    /// „Zeiten gegen Flugplan": Plan (SimBrief) und Ist als Unix-s — dieselbe
    /// Form wie im Landungsdatensatz, damit dieselbe Anzeige-Logik rechnet.
    flugzeiten: landing_scoring::flugzeiten::FlugZeiten,
    /// Erwartete Landung (Unix-s) aus der Planzeit am aktuellen Ort.
    landung_erwartet: Option<i64>,
    landing_rate_fpm: Option<f32>,
    landing_g_force: Option<f32>,
    /// Set to `true` exactly ONCE — the first time the UI reads the flight
    /// status after an automatic resume (disk or remote). Lets the dashboard
    /// surface a one-time banner with a 10-second cancel window.
    was_just_resumed: bool,
    /// v0.12.1 (Stream E): true when, on a resume, the current sim position
    /// looks like a glitchy crash-reload (persisted phase airborne but sim
    /// on-ground, or implausibly large drift). The resume banner then
    /// disables its 10-second auto-confirm and requires an explicit click.
    resume_position_suspect: bool,
    /// v0.13.0 Stream F (LE22-LE26): wenn `was_just_resumed=true`, hier alle
    /// gespeicherten State-Werte für die Banner-Anzeige damit der Pilot weiß
    /// WOHIN er sich im Sim repositionieren muss UND mit welchem Fuel/Weight.
    /// Sehr wichtig: MSFS setzt Fuel beim Reload oft auf Default zurück —
    /// der Pilot muss das manuell wieder einstellen.
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_lat: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_lon: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_alt_ft: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_heading_deg: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_gs_kt: Option<i32>,
    /// Wann der gespeicherte Punkt zuletzt aktualisiert wurde (UTC, RFC 3339).
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_fuel_kg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_zfw_kg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_total_weight_kg: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_known_aircraft_icao: Option<String>,
    /// Departure stand from MSFS `ATC PARKING NAME` (snapshotted at
    /// the start of the flight). Empty until captured.
    dep_gate: Option<String>,
    /// Arrival stand from MSFS `ATC PARKING NAME` after BlocksOn.
    arr_gate: Option<String>,
    /// `ATC RUNWAY SELECTED` snapshotted while on Final.
    approach_runway: Option<String>,
    /// v0.15.19: Plausibilitäts-geclampter (2–7,5°) Gleitwinkel der Approach-
    /// Runway aus den Live-Navdaten. Füttert das Stable-Approach-Banner, damit
    /// es seine V/S-Schwellen mit demselben `gs_factor = tan(g)/tan(3°)`
    /// skaliert wie das Post-Flight-Scoring (`compute_approach_stability_v2`).
    /// None → Runway/Winkel unbekannt oder unplausibel → Banner bleibt beim
    /// 3°-Default (unverändert für normale Anflüge).
    approach_glideslope_angle: Option<f64>,
    /// v1.5.1 (#hud-pilotenfeedback F2): Roh-METARs für das In-Sim-HUD.
    /// Abflugwetter ab Boarding, Zielwetter ab Sinkflug (Final holt
    /// frisch nach und überschreibt). Rohtext, die Anzeige kürzt selbst.
    #[serde(skip_serializing_if = "Option::is_none")]
    dep_metar: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arr_metar: Option<String>,
    /// v1.5.2 (#hud-metar): dieselben Abrufe DEKODIERT (Wind, Sicht,
    /// Wetter, Wolken, Temperatur, QNH) — die Anzeige formatiert nur
    /// noch, geparst wird ausschliesslich im metar-Crate.
    #[serde(skip_serializing_if = "Option::is_none")]
    dep_metar_decoded: Option<MetarSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arr_metar_decoded: Option<MetarSnapshot>,
    /// v1.5.0 (#msfs-hud): **geschätzte** Landebahn im Anflug, aus Position
    /// und Steuerkurs gegen die Navdaten bestimmt. Siehe
    /// `FlightStats::predicted_runway` — insbesondere, warum das ein eigenes
    /// Feld neben `approach_runway` ist und niemals in den PIREP fließt.
    ///
    /// `None` heißt „noch nicht entschieden“, nicht „Fehler“: am Boden und
    /// früh im Flug ist das der Normalfall.
    #[serde(skip_serializing_if = "Option::is_none")]
    predicted_runway: Option<String>,
    /// v1.3.5 (#Hard-Landing-Banner): mirrors `FlightStats.landing_score_
    /// finalized` — true once the post-touchdown vertical-speed refinement
    /// (edge-value recompute, ~9-12s after touchdown) is done. The live
    /// hard-landing banner waits for this instead of showing the raw
    /// SimVar-latched estimate immediately (field report 31.07.2026: raw
    /// SimVar read -770 fpm at touchdown, the refined/scored value for the
    /// SAME touchdown was -455 fpm — the banner showed the wrong number
    /// for ~9 seconds before the Logbook/PIREP showed the real one).
    /// `landing_rate_fpm` below can already hold a plausible early value
    /// before this flips true (the edge-recompute cascade has an earlier,
    /// less-refined fallback) — this flag is the actual "won't change
    /// again" signal, not mere presence of a value.
    landing_score_finalized: bool,
    /// v0.21 (#msfs-panel): live cockpit telemetry for the MSFS in-sim
    /// panel's approach-monitor card. `None` when the sim snapshot isn't
    /// currently available (matches every other `sim_*` live field's
    /// staleness behavior). Not read by the existing desktop UI — added
    /// purely for the panel; see `PanelLiveSnapshot`.
    #[serde(skip_serializing_if = "Option::is_none")]
    live: Option<PanelLiveSnapshot>,
    /// ISO-8601 UTC timestamp of the last successful position-post.
    /// Powers the cockpit's LIVE recording indicator — "X seconds
    /// ago" derived client-side.
    last_position_at: Option<String>,
    /// ISO-8601 UTC timestamp of the last successful PIREP heartbeat
    /// (`POST /pireps/{id}/update`). Surfaced for the debug panel so
    /// the pilot can see the keep-alive is firing — without it, phpVMS
    /// soft-deletes the PIREP after `acars.live_time` hours of inactive
    /// `updated_at`.
    last_heartbeat_at: Option<String>,
    /// Number of positions sitting in the offline queue. Non-zero
    /// means the streamer hit a network failure recently and the
    /// dashboard should warn the pilot.
    queued_position_count: u32,
    /// v0.6.2 — Connection-Health zum phpVMS-Server.
    /// Werte: "live" (letzter POST OK) / "failing" (letzter POST scheiterte).
    /// Frontend nutzt das + queued_position_count für 3 Status-Anzeige:
    ///   - live + queued=0  → "Live" (gruen)
    ///   - live + queued>0  → "Sync" (blau, normaler Backlog)
    ///   - failing          → "Offline" (rot, echte Verbindung weg)
    connection_state: String,
    /// v0.4.1: ISO-8601 UTC timestamp wann der Streamer den Sim-
    /// Disconnect detektiert und den Flug pausiert hat. None = Flug
    /// läuft normal. Some(...) = Cockpit-Tab zeigt Resume-Banner.
    paused_since: Option<String>,
    /// v0.4.1: Snapshot der letzten bekannten Sim-Werte (LAT/LON/HDG/
    /// ALT/Fuel/ZFW) vor dem Disconnect. Pilot nutzt das um sein
    /// Flugzeug nach Sim-Restart wieder an die richtige Stelle zu
    /// setzen. None solange nicht pausiert.
    paused_last_known: Option<PausedSnapshot>,
    /// Divert hint when the FSM noticed the aircraft landed somewhere
    /// other than the planned `arr_airport`. The cockpit renders a
    /// banner ("you landed at LFBO, planned was LEBL — file as divert
    /// to LFBO?") with action buttons. None for normal arrivals.
    divert_hint: Option<DivertHint>,
    /// Umleitung (Beta): vom Piloten im Flug gemeldetes neues Ziel.
    umleitung_ziel: Option<String>,
    /// Beim Wiederaufnehmen wurde ein physikalisch unmoeglicher Sprung
    /// erkannt (Sprit/Hoehe/Drift, s. `is_impossible_discontinuity`): Der
    /// Sim wurde neu geladen, der Flug ging so nicht weiter. Dann gibt die
    /// App NICHT mehr von selbst ab, sondern das Cockpit fragt den Piloten
    /// (Banner). Anlass MSC1588, 22.09.2026: Nach dem Neuladen am Zielplatz
    /// liefen die Phasen im Stand bis „Arrived" durch, und der PIREP ging
    /// mit voller Strecke und ohne je gemessene Landung raus.
    unmoeglicher_sprung: bool,
    /// Warum der Pilot entscheiden muss: `sprung` oder `landung_fehlt`.
    /// `None` = die App darf selbst einreichen.
    abgabe_sperre: Option<String>,
    /// Worauf das Resume-Gate wartet (Text fuer den Piloten), solange der
    /// Flug nach einem Neustart noch nicht scharf ist.
    resume_wartet_grund: Option<String>,
    /// Seit wie vielen Sekunden das Gate wartet.
    resume_wartet_s: Option<i64>,
    /// Laeuft ein Resume-Gate? Dann fragt der Banner nicht erneut.
    resume_gate_laeuft: bool,
    /// Der Flieger steht nach dem Absturz auf einem anderen Platz am Boden —
    /// der Banner bietet „Vom Boden weiterfliegen" an (GAF 278).
    resume_vom_boden_moeglich: bool,
    /// Number of touch-and-go events recorded so far. Always 0 on a
    /// routine A→B; non-zero on training flights or unstable approaches
    /// where the pilot bounced and went around. Surfaced as a small
    /// counter chip in the cockpit during/after Landing phase.
    touch_and_go_count: u32,
    /// Number of confirmed go-arounds (sustained climb-back from low
    /// approach). Independent of T&G — a missed-approach without
    /// ground contact only bumps this counter, not the T&G one.
    go_around_count: u32,
    // ---- v0.3.0 — SimBrief OFP Plan-Werte für Soll/Ist-Vergleich ----
    /// Plan-Block-Fuel aus dem SimBrief OFP (kg). None wenn der Pilot
    /// keine SimBrief-Verbindung im phpVMS-Profil hat oder das OFP
    /// nicht abgerufen werden konnte.
    planned_block_fuel_kg: Option<f32>,
    /// Plan-Trip-Burn aus dem SimBrief OFP (kg).
    planned_burn_kg: Option<f32>,
    /// Plan-Reserve-Fuel (kg).
    planned_reserve_kg: Option<f32>,
    /// Plan-ZFW (kg).
    planned_zfw_kg: Option<f32>,
    /// Plan-TOW (kg).
    planned_tow_kg: Option<f32>,
    /// Plan-LDW (kg).
    planned_ldw_kg: Option<f32>,
    /// Plan-Route aus dem OFP, ICAO-codiert. Wird im Briefing-Tab
    /// als monospace-string angezeigt.
    planned_route: Option<String>,
    /// Geplanter Alternate-Flughafen (ICAO).
    planned_alternate: Option<String>,
    // ---- v0.3.0: MAX-Werte für Overweight-Detection ----
    /// Maximum Zero-Fuel Weight (Strukturlimit). None bei Custom-
    /// Subfleets ohne MAX-Daten im OFP.
    planned_max_zfw_kg: Option<f32>,
    /// Maximum Takeoff Weight.
    planned_max_tow_kg: Option<f32>,
    /// Maximum Landing Weight.
    planned_max_ldw_kg: Option<f32>,
    // ---- v0.3.0: Live-Loadsheet-Werte aus dem aktuellen Sim-Snapshot ----
    /// Aktuelles Block-Fuel im Tank (kg). Live-Wert aus dem Sim,
    /// updated bei jedem Snapshot. Driver für die "Tank-Vorgang läuft"-
    /// Anzeige im Cockpit-Tab vor Pushback.
    sim_fuel_kg: Option<f32>,
    /// Aktuelles ZFW (kg). None wenn der Sim/das Aircraft-Profil das
    /// nicht meldet (z.B. Fenix mit Standard-SimVars).
    sim_zfw_kg: Option<f32>,
    /// Aktuelles Total-Weight (= TOW wenn am Boden voll beladen).
    /// None wenn der Sim das nicht meldet.
    sim_tow_kg: Option<f32>,

    // v0.7.19 GAF-707 Accident-Detection (QS-R1 Finding 3): UI muss
    // schon waehrend des aktiven Flugs wissen ob ein Accident gelatcht
    // wurde — damit der Flight-End-Dialog "War das ein Absturz?" greifen
    // kann statt den PIREP blind zu filen. Felder spiegeln 1:1 die
    // FlightStats-Werte aus dem Klassifikator-Pfad. Spec §AeroACARS
    // Client Tab "Landung" + §Active Flight / Flight End.
    accident_detected: bool,
    /// "sim_crash" | "impact" | "off_airport_impact" | None
    accident_kind: Option<String>,
    /// "high" (Confirmed) | "medium" (Suspected) | None
    accident_confidence: Option<String>,
    accident_reasons: Vec<String>,
    accident_at: Option<String>,

    /// v0.12.5 (LE3): Vorbefüll-Werte fürs manuelle Filing-Formular.
    manual_filing_defaults: ManualFilingDefaults,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SiteConfig {
    url: String,
}

#[derive(Serialize)]
pub struct LoginResult {
    profile: Profile,
    base_url: String,
}

/// Errors returned to the UI in a serializable shape.
/// `code` is a stable, machine-readable identifier the frontend uses for i18n.
/// `details` carries optional structured payload (e.g. list of missing
/// validation fields) so the UI can render a richer error than just a string.
#[derive(Debug, Serialize)]
pub struct UiError {
    // v0.16.0 (#LAN-Remote): `pub(crate)` so the `remote` module can read
    // `code`/`message` when building the HTTP 422 `{code,message}` body
    // for the LAN bridge. Still serializes identically to the Tauri IPC
    // reject shape.
    pub(crate) code: String,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl UiError {
    fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

impl From<ApiError> for UiError {
    fn from(err: ApiError) -> Self {
        Self {
            code: err.code().to_string(),
            message: err.to_string(),
            details: None,
        }
    }
}

// ---- Helpers ----
