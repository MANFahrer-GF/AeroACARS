/// Map a numeric landing score (0-100, finer granularity than the
/// 5-tier `LandingScore`) to a letter grade. Same A+/A/B+/B/C/D/F
/// scale as the Landing Analyzer reference tool. Boundaries chosen
/// so a clean butter touchdown (LandingScore::Smooth = 100) earns
/// A+, an Acceptable (80) earns B, Firm (60) earns C, etc.
fn letter_grade(numeric: i32) -> &'static str {
    // v3 (P-4.21): eine Quelle — die Leiter steht im Bewertungsmodul.
    landing_scoring::wasm::gesamt_buchstabe(numeric)
}

/// Render the full callsign as "DLH 155" (airline + space + number) when
/// the airline ICAO is known, falling back to bare "155" otherwise.
fn format_callsign(airline_icao: &str, flight_number: &str) -> String {
    if airline_icao.is_empty() {
        flight_number.to_string()
    } else {
        format!("{} {}", airline_icao, flight_number)
    }
}

/// Render an Option<f32> as kg with sensible "n/a" fallback for the
/// activity-log detail string. Used by the fuel/weight diagnostics.
fn fmt_kg(v: Option<f32>) -> String {
    match v {
        Some(n) if n > 0.0 => format!("{:.0} kg", n),
        Some(_) | None => "n/a".to_string(),
    }
}
fn fmt_kg_f64(v: Option<f64>) -> String {
    match v {
        Some(n) if n > 0.0 => format!("{:.0} kg", n),
        Some(_) | None => "n/a".to_string(),
    }
}

/// Emit a fuel/weight diagnostic line into the activity log at the
/// phase transitions where AeroACARS captures values into PIREP
/// fields. Lets the pilot verify after the fact what the app saw.
///
/// The four meaningful transitions:
///   * Pushback / TaxiOut → "Block fuel" capture window closes (well,
///     it's a running peak, but this is when the pilot rolls off the
///     stand and the value should be locked in).
///   * Takeoff           → TOW + takeoff fuel snapshotted into stats.
///   * Landing           → LDW + landing fuel snapshotted.
///   * BlocksOn          → final fuel + computed Fuel Used.
///
/// Other phases produce no entry.
fn log_fuel_weight_at_phase(
    app: &AppHandle,
    flight: &ActiveFlight,
    phase: FlightPhase,
    snap: &SimSnapshot,
) {
    // Take ALL values out under the lock, then drop it before calling
    // log_activity_handle (which reaches into AppState for the activity
    // log mutex — never hold two mutexes across an external call).
    let (
        block_fuel,
        takeoff_fuel,
        takeoff_weight,
        landing_fuel,
        landing_weight,
        last_fuel,
        blocks_on_reached,
    ) = {
        let stats = flight.stats.lock().expect("flight stats");
        (
            stats.block_fuel_kg,
            stats.takeoff_fuel_kg,
            stats.takeoff_weight_kg,
            stats.landing_fuel_kg,
            stats.landing_weight_kg,
            stats.last_fuel_kg,
            stats.blocks_on_reached,
        )
    };

    // The universal Arrived-fallback skips BlocksOn (see
    // `settle_at_arrival_stand`), and this diagnostic used to be keyed on the
    // BlocksOn transition alone — so every flight that arrived via the fallback
    // silently lost its fuel-used line. Treat "Arrived without ever having
    // passed BlocksOn" as the block-on moment: the aircraft is stopped with the
    // engines shut down, which is exactly the state these numbers describe.
    //
    // `blocks_on_reached` is persisted (see `StatsSnapshot`) precisely so this
    // survives a resume — the in-memory transition list does not, and keying
    // off it would emit a duplicate line on every resumed flight.
    let phase = if phase == FlightPhase::Arrived && !blocks_on_reached {
        FlightPhase::BlocksOn
    } else {
        phase
    };

    let (message, detail) = match phase {
        FlightPhase::Pushback | FlightPhase::TaxiOut => {
            // Block fuel is a running peak — at this point we expect
            // it to roughly match `snap.fuel_total_kg` unless the
            // pilot just defueled. ZFW/payload/empty come straight
            // from the SimVar block.
            (
                "Fuel & Weight @ Block-off".to_string(),
                Some(format!(
                    "Block fuel {} | Live fuel {:.0} kg | ZFW {} | Payload {} | OEW {} | Total {}",
                    fmt_kg(block_fuel),
                    snap.fuel_total_kg,
                    fmt_kg(snap.zfw_kg),
                    fmt_kg(snap.payload_kg),
                    fmt_kg(snap.empty_weight_kg),
                    fmt_kg(snap.total_weight_kg),
                )),
            )
        }
        FlightPhase::Takeoff => (
            "Fuel & Weight @ Takeoff".to_string(),
            Some(format!(
                "Takeoff fuel {} | TOW {} | Block fuel was {}",
                fmt_kg(takeoff_fuel),
                fmt_kg_f64(takeoff_weight),
                fmt_kg(block_fuel),
            )),
        ),
        FlightPhase::Landing => (
            "Fuel & Weight @ Landing".to_string(),
            Some(format!(
                "Landing fuel {} | LDW {} | TOW was {}",
                fmt_kg(landing_fuel),
                fmt_kg_f64(landing_weight),
                fmt_kg_f64(takeoff_weight),
            )),
        ),
        FlightPhase::BlocksOn => {
            // Fuel Used = block fuel (peak before takeoff) minus
            // current fuel. Only show when both are valid and
            // block > current — otherwise the math is meaningless
            // (e.g. defuel during taxi, fuel SimVar missing).
            let fuel_used = match (block_fuel, last_fuel) {
                (Some(b), Some(c)) if b > 0.0 && b > c => Some(b - c),
                _ => None,
            };
            (
                "Fuel & Weight @ Block-on".to_string(),
                Some(format!(
                    "Final fuel {} | Fuel used {} | Landing fuel was {}",
                    fmt_kg(last_fuel),
                    fmt_kg(fuel_used),
                    fmt_kg(landing_fuel),
                )),
            )
        }
        // No diagnostic for other phase transitions.
        _ => return,
    };

    log_activity_handle(app, ActivityLevel::Info, message, detail);
}

// ---- Activity log ----

/// How many position-streamer ticks between persistence flushes. Stats
/// are snapshotted to disk every Nth post so a Tauri crash mid-flight
/// can resume with reasonably fresh distance/fuel/phase data instead of
/// the zeros we wrote at flight_start. 5 ticks ≈ every ~25–150 s
/// depending on phase cadence — fine for the cost of a small JSON write.
const STATS_PERSIST_EVERY_TICKS: u32 = 5;

// Touchdown analyzer window in seconds — how long after the first
// on-ground tick we keep refining peak G-force and peak descent rate
// before locking in the final score. 5 s is enough to catch the spike
// (which can lag the initial contact by a tick or two) without bleeding
// into the rollout. Bounded above by `TOUCHDOWN_BUFFER_SECS` (the ring
// buffer's reach) and below by `TOUCHDOWN_G_WINDOW_MS`.
// Wert seit v3: bundle/grenzwerte/landung.json -> touchdown_window_secs (v2.1.24: 5)

// V/S sampling window around the on-ground edge, in milliseconds.
// We pick the worst-case (most negative) V/S from buffered samples
// within ±`/2` of the touchdown timestamp instead of using a single
// tick. 500 ms matches BeatMyLanding's `TouchdownWindowMs` — large
// enough that the actual contact frame falls inside even when the
// edge detection is one tick late, small enough not to pull in
// pre-flare descent or post-bounce rebound values.
// Wert seit v3: bundle/grenzwerte/landung.json -> touchdown_vs_window_ms (v2.1.24: 500)

// Peak-G search window, in milliseconds AFTER the on-ground edge.
// G spike from gear strut compression typically peaks 100–300 ms
// after first contact and is over within ~600 ms. We use 800 ms —
// tighter than BeatMyLanding's `MaxGWindowMs` (1500) because the
// later ms reach into strut REBOUND territory which inflates G with
// pseudo-impact spikes, hurting the score on otherwise-clean
// landings (the 1.24 G "acceptable" downgrade we kept seeing on
// pilot reports of butter touchdowns).
// Wert seit v3: bundle/grenzwerte/landung.json -> touchdown_g_window_ms (v2.1.24: 800)

// Maximum window in seconds during which we still count subsequent
// liftoffs as bounces of the original touchdown (rather than a fresh
// landing event). Matches BeatMyLanding's `BounceWindow`.
// Wert seit v3: bundle/grenzwerte/landung.json -> bounce_window_secs (v2.1.24: 8)

// Wie lange ein Flugzeug mindestens in der Luft sein muss, damit es als
// Sprung zaehlt.
//
// ⚠ Ein echter Sprung dauert merklich — das Flugzeug steigt, kommt
// zurueck. Ein flatternder Bodenkontakt-Schalter ist nach Millisekunden
// vorbei. Bei THY42 (FALE 24, 30.08.2026) waren die beiden falsch
// gezaehlten "Spruenge" 40 und 160 ms lang, bei einer Hoehenaenderung
// von 0,01 Fuss.
//
// 300 ms ist bewusst grosszuegig: Ein Sprung, der kuerzer ist, hat das
// Flugzeug auch kaum vom Boden gebracht.
// Wert seit v3: bundle/grenzwerte/landung.json -> bounce_min_dauer_ms (v2.1.24: 300)

// P1/8b (Thomas 22.10.2026): Mindestdauer einer Luftphase fuer den
// „leichten Hopser“ (unter der 5-ft-Hopser-Schwelle). Messung am Bestand
// (`handoff/2026-10-22-hopser-messung.md`, 1738 Landungen): keine Phase
// unter 1 s erreicht 2 ft; Flattern bleibt unter 1 ft (AFR1010 7,1 s bei
// 0,05 ft). Gilt fuer den 50-Hz-Pfad und den 1-Hz-Streamer.
// Wert seit v3: bundle/grenzwerte/landung.json -> bounce_light_min_dauer_ms (v2.1.24: 1000)

// P1/8b: Mindesthoehe ueber der Bodenhoehe (erste Probe mit Bodenkontakt)
// fuer den leichten Hopser. Darueber ab `BOUNCE_SCORED_MIN_AGL_FT` ist es
// ein normaler Hopser.
// Wert seit v3: bundle/grenzwerte/landung.json -> bounce_light_min_agl_ft (v2.1.24: 2.0)

/// P1/8b: Regel des leichten Hopsers fuer den 1-Hz-Streamer (eine Luftphase,
/// die mit Bodenkontakt endete). Unter `BOUNCE_AGL_THRESHOLD_FT` — darueber
/// zaehlt der Streamer einen normalen Hopser.
fn leichter_hopser_im_streamer(dauer_ms: i64, max_ueber_boden_ft: f64) -> bool {
    dauer_ms >= crate::grenzwerte::grenzwerte().landung.bounce_light_min_dauer_ms.wert
        && max_ueber_boden_ft >= crate::grenzwerte::grenzwerte().landung.bounce_light_min_agl_ft.wert as f64
        && max_ueber_boden_ft < crate::grenzwerte::grenzwerte().landung.bounce_agl_threshold_ft.wert
}

// AGL altitude (ft) the aircraft must climb above before we can
// detect a bounce. Below this, on-ground flickers are noise (gear
// strut oscillation, sloppy SimVar updates).
//
// v0.8.3 (#3): von 35 ft auf 15 ft gesenkt. Vorher war der Wert an
// BeatMyLanding's `BounceRadioAltThresholdFeet` (35) ausgerichtet, was
// zur Folge hatte dass Standard-Bounces von 5-15 ft (= das was Piloten
// als „typischer Hopser" wahrnehmen) NIE gezaehlt wurden. In 14 Tagen
// Produktion: 183 Touchdowns, **5** Bounces erkannt (2.7 %), null aus
// X-Plane, alle 5 nur bei extrem harten MSFS-Landungen (vs <= -500 fpm).
// Reported von Adrian (2026-05-14 PR-Kontext, Beispiel #167).
//
// Wert = `touchdown_v2::BOUNCE_SCORED_MIN_AGL_FT`, damit der
// live-Streamer-Tally und der Forensics-Override-Pfad gegen dieselbe
// Schwelle messen. f64 to match `SimSnapshot::altitude_agl_ft`.
//
// Lernpaket AP3 (29.09.2026): 5 ft und seitdem RELATIV zur Bodenhoehe
// beim ersten Bodenkontakt (`FlightStats::bounce_boden_agl_ft`), wie der
// 50-Hz-Pfad seit dem THY42-Fix. Die Hoehe ueber Grund wird am
// Schwerpunkt gemessen — ein grosses Flugzeug steht am Boden schon bei
// rund 20 ft (td 1403: 19,6 ft); absolut gemessen koennte es nie unter
// eine Rueckkehrschwelle von 5 ft fallen.
// Wert seit v3: bundle/grenzwerte/landung.json -> bounce_agl_threshold_ft (v2.1.24: 5.0)

// Hoehe ueber der Bodenhoehe (ft), unter die das Flugzeug nach dem
// Scharfschalten zurueck muss, damit ein Hopser zaehlt — oder es meldet
// wieder Bodenkontakt. Relativ wie `BOUNCE_AGL_THRESHOLD_FT`.
// Wert seit v3: bundle/grenzwerte/landung.json -> bounce_agl_return_ft (v2.1.24: 2.0)

/// Max samples retained in `FlightStats::approach_buffer`.
///
/// ⚠ Die alte Rechnung „5–8 s je Probe, 120 Proben ≈ 10–15 min" stimmt
/// nicht mehr: `push_approach_sample` laeuft je `step_flight`-Takt, und der
/// ist unter 1500 ft etwa 1 s bzw. 0,75 s. 120 Proben decken damit nur rund
/// 1,5–2 min — bei 120 kt beginnt das Fenster schon unter 1000 ft (QS
/// 29.09.2026, Lernpaket AP4, Befund 3). Die Kappe bleibt trotzdem: ueber
/// `compute_approach_stddev` → `sub_stability` traegt dieser Puffer die
/// Note, und die aendert das Lernpaket nicht. Die Forensik hat ihren
/// eigenen, laengeren Puffer (`ANFLUG_FORENSIK_PUFFER_MAX`).
const APPROACH_BUFFER_MAX: usize = 120;

/// Laenge des Forensik-Puffers (Lernpaket AP4/AP5): 400 Proben ≈ 5 min bei
/// 0,75 s — so viel, wie die Auswertung ohnehin hoechstens ansieht
/// (`ANFLUG_FENSTER_S`). Ein GA-Anflug mit 300 fpm braucht von 1000 ft gut
/// drei Minuten.
const ANFLUG_FORENSIK_PUFFER_MAX: usize = 400;

/// Was mit einer Aufsetzmeldung des X-Plane-Plugins geschah.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PremiumTd {
    /// Ausserhalb der Flugphase oder Aufsetzen schon abgeschlossen.
    Verworfen,
    /// Das Plugin ist der erste Edge — neuer Kandidat.
    NeuerKandidat,
    /// An den Kandidaten des RREF-/Fahrwerkskraft-Edges angehaengt.
    Angehaengt { pending_age_ms: i64 },
    /// Der Kandidat traegt schon eine Plugin-Meldung (Hopser-Echo).
    Wiederholt,
}

/// Aufsetzmeldung des Plugins in den Kandidaten uebernehmen.
///
/// `vs_fpm`/`g` sind `NaN`, wenn das Plugin `null` schickte (nicht
/// gemessen, Plugin ab 1.0) — dann bleibt die Stelle leer und beim
/// VALIDATED gilt der Sampler-Wert (`td_werte_mit_premium`). Ob der
/// Kandidat schon eine Plugin-Meldung hat, entscheidet der eigene Merker
/// `pending_td_premium_gesehen`, nicht `pending_td_premium_vs` (das darf nach
/// einer Meldung leer sein).
fn premium_td_uebernehmen(
    stats: &mut FlightStats,
    vs_fpm: f32,
    g: f32,
    capturable_phase: bool,
    now: DateTime<Utc>,
) -> PremiumTd {
    if !capturable_phase || stats.sampler_touchdown_at.is_some() {
        return PremiumTd::Verworfen;
    }
    if stats.pending_td_premium_gesehen {
        // Erste Premium-Messung war frame-genauer.
        return PremiumTd::Wiederholt;
    }
    let ergebnis = match stats.pending_td_at {
        None => {
            stats.pending_td_at = Some(now);
            PremiumTd::NeuerKandidat
        }
        // Round-4 P2 fix: RREF/gear-force hat pending_td_at gesetzt, das
        // Premium-Event kommt einen Tick spaeter — anhaengen, pending_td_at
        // bleibt am frueheren Edge.
        Some(t) => PremiumTd::Angehaengt {
            pending_age_ms: (now - t).num_milliseconds(),
        },
    };
    stats.pending_td_premium_vs = Some(vs_fpm).filter(|v| v.is_finite());
    stats.pending_td_premium_g = Some(g).filter(|v| v.is_finite());
    stats.pending_td_premium_gesehen = true;
    ergebnis
}

/// Sinkrate und g des bestaetigten Aufsetzens: Plugin-Werte, wo endlich
/// gemessen, sonst die Werte des Samplers.
fn td_werte_mit_premium(stats: &FlightStats, impact_vs: f32, g_peak: f32) -> (f32, f32) {
    (
        stats
            .pending_td_premium_vs
            .filter(|v| v.is_finite())
            .unwrap_or(impact_vs),
        stats
            .pending_td_premium_g
            .filter(|v| v.is_finite())
            .unwrap_or(g_peak),
    )
}

#[cfg(test)]
#[path = "tests_lib/premium_td_tests.rs"]
mod premium_td_tests;

/// 2.0.28 — Aufsetzer waehrend der Pause nach einem Sim-Neuladen (DLA 8260,
/// 12.10.2026, siehe `aufsetz_erkennung_ruht_nach_reload`).
#[cfg(test)]
#[path = "tests_lib/aufsetzen_in_pause_tests.rs"]
mod aufsetzen_in_pause_tests;

/// Mittleres N1 der Triebwerke, die laufen (N1 ≥ 5 %).
///
/// `eng_n1_pct` ist ein positionserhaltendes Praefix-Feld: ein stehendes
/// Triebwerk steht darin mit ~0. Mitgemittelt halbierte es das Mittel — und
/// damit jede Schubaenderung, die Hysterese von 2 % N1 wirkte dann wie 4 %.
/// Deshalb zaehlen nur Werte ab 5 % (Leerlauf-N1 liegt bei ~20 %, dieselbe
/// Grenze wie die „lebt"-Pruefung im MSFS-Adapter).
fn n1_mittel_laufend(n1: Option<&[f64]>) -> Option<f32> {
    let laufend: Vec<f64> = n1?
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v >= 5.0)
        .collect();
    if laufend.is_empty() {
        return None;
    }
    Some((laufend.iter().sum::<f64>() / laufend.len() as f64) as f32)
}

// Primary rollout-end trigger: groundspeed at which we consider the
// pilot has finished using the runway and is about to clear at a
// high-speed taxiway exit. Real pilots almost never decelerate to a
// full stop on the runway — they brake to ~40 kt and turn off at the
// next exit. Using a 40 kt threshold gives a "runway distance used"
// metric that matches what other ACARS tools (BeatMyLanding,
// vmsACARS) and FOQA reports show.
//
// Source for 40 kt: ICAO Doc 9981 (Procedures for Air Navigation
// Services — Aerodromes) recommends rapid-exit taxiways be designed
// for a 65 km/h (≈ 35 kt) exit speed; FAA AC 150/5300-13B uses 60 kt
// for high-speed RETs but pilots routinely brake further to ~40 kt
// before committing to the turn.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> rollout_exit_gs_kt (v2.1.24: 40.0)

// Secondary trigger: full stop on the runway (rare in practice — long
// rwys at uncontrolled fields, GA touch-and-go's gone wrong). Kept
// as a hard floor so a pilot who really does brake to a stop still
// gets a finalised metric.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> rollout_stop_gs_kt (v2.1.24: 5.0)

// Tertiary trigger: heading has rotated this many degrees from the
// touchdown heading, meaning the aircraft has clearly turned onto a
// taxiway and is no longer using the runway. Stops the accumulator
// from counting the taxi-out distance against the rollout figure.
// Computed signed (wraparound-safe) and compared in absolute value.
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> rollout_heading_deviation_deg (v2.1.24: 30.0)

/// Hard-landing thresholds, ordered worst-first. The first row that
/// the peak |V/S| or G-force breaches wins; combined with
/// `bounce_count` this maps to a `LandingScore`.
///
/// Sources verified rather than guessed (the older 60 / 240 / 600 fpm
/// table downgraded clean -91 fpm landings to Acceptable, which the
/// pilot rightly called out as unrealistic):
///
///   * **Boeing 737 FCOM**: Hard-Landing-Inspection > 600 fpm OR
///     > 1.7 G. Severe > 1000 fpm OR > 2.6 G.
///   * **Airbus A320 FCOM**: Max recommended TD sink 360 fpm; hard-
///     landing inspection ≈ 600 fpm OR 2.6 G.
///   * **vmsACARS default rules.yml** (extracted from the shipped PHP
///     module): single `HARD_LANDING` rule at parameter = 500 fpm.
///   * **Lufthansa FOQA category bands** (publicly documented):
///       Soft   < 200 fpm
///       Normal 200–400 fpm
///       Firm   400–600 fpm
///       Hard   > 600 fpm
///       Heavy  > 1000 fpm
///   * **Community ACARS conventions** (Smartcars, BeatMyLanding
///     toast colour bands, LandingRate.com): butter < 200 fpm,
///     good < 400, ok < 600, bad above.
///
/// Consensus → these tiers:
///
/// ```text
///                  V/S        G
///   Smooth         < 200 fpm  < 1.20 G   (butter / greaser)
///   Acceptable     < 400 fpm  < 1.40 G   (normal LH FOQA)
///   Firm           < 600 fpm  < 1.70 G   (firm but accepted)
///   Hard           < 1000 fpm < 2.10 G   (FCOM inspection trigger)
///   Severe         ≥ 1000 fpm ≥ 2.10 G   (structural concern)
/// ```
// V/S boundaries — fpm, |abs| at touchdown:
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_vs_severe_fpm (v2.1.24: 1000.0)
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_vs_hard_fpm (v2.1.24: 600.0)
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_vs_firm_fpm (v2.1.24: 400.0)
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_vs_smooth_fpm (v2.1.24: 200.0)
// G boundaries — peak G in the touchdown window:
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_g_severe (v2.1.24: 2.10)
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_g_hard (v2.1.24: 1.70)
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_g_firm (v2.1.24: 1.40)
// Wert seit v3: bundle/grenzwerte/ausrollen.json -> touchdown_g_smooth (v2.1.24: 1.20)

// AP master toggles only emit a log entry once they've held for this
// many seconds. Stops a flickering / pulsed LVar (Fenix's momentary
// `S_FCU_AP*` was the original culprit) from spamming the log with
// alternating ENGAGED / OFF lines.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> ap_debounce_secs (v2.1.24: 2)

// ---- v0.16.10 Premium-Cockpit-Daten — Debounce-Fenster ----
// Generic-FMA-Zeile (Fenix/FBW/INI/…): 3 s wie beim PMDG-Pendant —
// FMA-Sub-Modes wechseln bei Mode-Reversions gern mehrfach pro Sekunde.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> premium_fma_debounce_secs (v2.1.24: 3)
// MASTER CAUTION / MASTER WARNING: Annunciator-Lampen flackern bei
// Cockpit-Tests am Boden — 2 s Stabilität bevor eine Transition loggt.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> master_annunciator_debounce_secs (v2.1.24: 2)
// Flugzeug-eigene FWC-Phase: rein informativ, 5 s Debounce reichen.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> aircraft_phase_debounce_secs (v2.1.24: 5)
// Stab-out-of-trim-Annunciator: pulst während Trim-Läufen.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> stab_trim_debounce_secs (v2.1.24: 5)
// Schubhebel-Detent (TOGA/FLX-MCT/CL): der Hebel wandert beim Setzen
// durch Zwischen-Detents — 3 s Stabilität pro Gate.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> thrust_gate_debounce_secs (v2.1.24: 3)
// v0.16.10 QS (B1): Approach-Speeds — VLS/VAPP driften auf FBW/INI
// laufend (Gewichts-/Konfigurations-Neuberechnung); 10 s Stabilität
// pro 5-kt-Bucket bevor eine Änderung loggt.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> approach_speeds_debounce_secs (v2.1.24: 10)
// v0.16.10 QS (B1): Bucket-Breite des Approach-Speeds-Latch-Keys.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> approach_speeds_bucket_kt (v2.1.24: 5.0)
// v0.16.10 QS (M5): Connect-Grace — so lange nach einem Stream-
// (Re-)Start ignorieren die Premium-Logger alle Snapshots (Addon-
// LVars lesen direkt nach Connect 0.0 bis das WASM sie befüllt).
// Wert seit v3: bundle/grenzwerte/cockpit.json -> premium_log_warmup_secs (v2.1.24: 10)
// v0.16.10 QS (M5): Tick-Lücke ab der ein Stream-(Re-)Start
// angenommen wird. Der Streamer tickt adaptiv alle 0,5-3 s — alles
// darüber heißt Sim-Disconnect/Pause/App-Resume.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> premium_log_stream_gap_secs (v2.1.24: 15)
// v0.16.10 QS (Minor 6): Umkehrschub-/Ground-Spoiler-Logger brauchen
// Groundspeed über dieser Schwelle an der Rising-Edge — ein Lever-
// Check beim Taxi (Reverser kurz gezogen, Spoiler-Test) soll keine
// "Umkehrschub aktiviert"-Zeile erzeugen.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> reverser_spoiler_min_gs_kt (v2.1.24: 40.0)
// Below-G/S-Alert: re-arm sobald der Alert so lange geklärt ist
// (einmal-pro-Approach-Latch, ein neuer Sinkflug unter den G/S soll
// wieder loggen dürfen).
// Wert seit v3: bundle/grenzwerte/cockpit.json -> below_gs_rearm_secs (v2.1.24: 30)
// v0.X — Diagnostics-Fund 2026-07-26: MSFS' natives STALL-WARNING-
// SimVar ist bei Payware mit eigenem Flugmodell (Fenix/FSReborn/
// ToLiss/PMDG) bekannt unzuverlaessig und kann bei ganz normaler
// Reise-/Anfluggeschwindigkeit kurz auf `true` flackern (Legacy-
// Flugmodell-Artefakt, kein echter Stall des tatsaechlich simulierten
// Flugzeugs — GSG-Live-Diagnostics zeigte dadurch 7,5% aller Fluege
// faelschlich als "Stall"). Nur ins Live-Map/Diagnostics-Payload
// veroeffentlichen wenn das SimVar mind. diese Zeit am Stueck true war.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> stall_warning_mqtt_debounce_secs (v2.1.24: 3)

/// v0.16.10: Generischer Latch+Debounce-Tracker für Premium-Activity-
/// Log-Zeilen. Gleiches Idiom wie der AP-Master-Debounce
/// (`AP_DEBOUNCE_SECS`-Block in `detect_telemetry_changes`), nur
/// wiederverwendbar statt drei Felder pro Logger:
///   * der ERSTE beobachtete Wert latcht IMMER stumm (kein Eintrag
///     beim Sim-Connect / App-Resume),
///   * eine Änderung wird erst publiziert wenn der neue Wert
///     `debounce_secs` stabil gehalten wurde — ein LVar-Puls kürzer
///     als das Fenster produziert GAR KEINEN Eintrag,
///   * Rückkehr zum gelatchten Wert räumt den Pending-State ab.
#[derive(Debug, Clone, Default)]
struct DebouncedLogState<T: PartialEq + Clone> {
    last_logged: Option<T>,
    pending: Option<T>,
    pending_since: Option<DateTime<Utc>>,
}

impl<T: PartialEq + Clone> DebouncedLogState<T> {
    /// Feed den aktuell beobachteten Wert; `true` ⇒ Transition ist
    /// debounced-stabil und soll JETZT geloggt werden (der Wert ist
    /// dann bereits als `last_logged` committed).
    fn update(&mut self, value: T, now: DateTime<Utc>, debounce_secs: i64) -> bool {
        if self.last_logged.as_ref() == Some(&value) {
            // Wert stimmt wieder mit dem gelatchten überein → Pending-
            // Änderung (falls vorhanden) war ein Puls, verwerfen.
            self.pending = None;
            self.pending_since = None;
            return false;
        }
        if self.last_logged.is_none() {
            // Erster Wert überhaupt — stumm latchen.
            self.last_logged = Some(value);
            self.pending = None;
            self.pending_since = None;
            return false;
        }
        if self.pending.as_ref() != Some(&value) {
            // Neue (oder andere) Änderung — Debounce-Timer starten.
            self.pending = Some(value);
            self.pending_since = Some(now);
            return false;
        }
        match self.pending_since {
            Some(since) if (now - since).num_seconds() >= debounce_secs => {
                self.last_logged = Some(value);
                self.pending = None;
                self.pending_since = None;
                true
            }
            _ => false,
        }
    }

    /// v0.16.10 QS (B1): wie `update`, aber der ERSTE beobachtete Wert
    /// loggt SOFORT statt stumm zu latchen — für Zeilen, deren erstes
    /// Auftauchen selbst das informative Ereignis ist (Approach-Speeds:
    /// der erste Some-Wert = FMS-Perf-Eingabe-Zeitstempel).
    fn update_first_logs(&mut self, value: T, now: DateTime<Utc>, debounce_secs: i64) -> bool {
        if self.last_logged.is_none() {
            self.last_logged = Some(value);
            return true;
        }
        self.update(value, now, debounce_secs)
    }
}

/// v0.16.10 QS (M5): Connect-Grace-Gate für die Premium-Logger.
/// Liefert `true` solange Snapshots IGNORIERT werden sollen. Mechanik:
///   * jede Tick-Lücke > `PREMIUM_LOG_STREAM_GAP_SECS` (oder der
///     allererste Tick) gilt als Stream-(Re-)Start und öffnet ein
///     `PREMIUM_LOG_WARMUP_SECS`-Fenster,
///   * innerhalb des Fensters latcht und loggt NICHTS (die Addon-LVars
///     lesen nach Connect/Resume 0.0 bis das WASM sie befüllt),
///   * nach dem Fenster latcht der erste — jetzt echte — Wert über die
///     bestehende First-Silent-Semantik der Tracker.
fn premium_log_warmup_active(
    last_tick_at: &mut Option<DateTime<Utc>>,
    warmup_until: &mut Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    let stream_restarted = match last_tick_at.replace(now) {
        None => true,
        Some(prev) => (now - prev).num_seconds() > crate::grenzwerte::grenzwerte().cockpit.premium_log_stream_gap_secs.wert,
    };
    if stream_restarted {
        *warmup_until = Some(now + chrono::Duration::seconds(crate::grenzwerte::grenzwerte().cockpit.premium_log_warmup_secs.wert));
    }
    match *warmup_until {
        Some(until) if now < until => true,
        Some(_) => {
            *warmup_until = None;
            false
        }
        None => false,
    }
}

// Suppress repeated "same" activity entries fired within this window.
// React StrictMode in dev double-mounts effects, so the login + session-
// restore commands run twice on startup; we don't want the user to see
// every event duplicated. Five seconds is wide enough to dedupe those
// without merging genuine repeats during a long flight.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> activity_dedupe_window_secs (v2.1.24: 5)

fn would_dedupe(log: &VecDeque<ActivityEntry>, now: DateTime<Utc>, message: &str) -> bool {
    log.back().is_some_and(|prev| {
        prev.message == message
            && (now - prev.timestamp).num_seconds() < crate::grenzwerte::grenzwerte().cockpit.activity_dedupe_window_secs.wert
    })
}

/// Push an entry into the in-memory activity log. Caps the buffer at
/// `ACTIVITY_LOG_CAPACITY` so a long-running session doesn't leak memory.
/// Also mirrors the entry to `tracing` at the matching level so command-
/// line debugging stays consistent. Identical messages within
/// `ACTIVITY_DEDUPE_WINDOW_SECS` of each other collapse to a single line.
fn log_activity(
    state: &tauri::State<'_, AppState>,
    level: ActivityLevel,
    message: impl Into<String>,
    detail: Option<String>,
) {
    let now = Utc::now();
    let message = message.into();
    {
        let log = state.activity_log.lock().expect("activity_log lock");
        if would_dedupe(&log, now, &message) {
            return;
        }
    }
    let entry = ActivityEntry {
        timestamp: now,
        level,
        message,
        detail,
    };
    match level {
        ActivityLevel::Info => {
            tracing::info!(message = %entry.message, detail = ?entry.detail, "activity")
        }
        ActivityLevel::Warn => {
            tracing::warn!(message = %entry.message, detail = ?entry.detail, "activity");
            sentry_init::capture_activity(
                &entry.message,
                entry.detail.as_deref(),
                sentry::Level::Warning,
            );
        }
        ActivityLevel::Error => {
            tracing::error!(message = %entry.message, detail = ?entry.detail, "activity");
            sentry_init::capture_activity(
                &entry.message,
                entry.detail.as_deref(),
                sentry::Level::Error,
            );
        }
    }
    let mut log = state.activity_log.lock().expect("activity_log lock");
    log.push_back(entry);
    while log.len() > ACTIVITY_LOG_CAPACITY {
        log.pop_front();
    }
    save_activity_log(&log);
}

/// Same as `log_activity` but takes an `AppHandle` for use inside the
/// streamer task (which has no `tauri::State` handle).
pub(crate) fn log_activity_handle(
    app: &AppHandle,
    level: ActivityLevel,
    message: impl Into<String>,
    detail: Option<String>,
) {
    log_activity_mit_meldung(app, level, message.into(), detail, true);
}


fn log_activity_mit_meldung(
    app: &AppHandle,
    level: ActivityLevel,
    message: String,
    detail: Option<String>,
    an_glitchtip: bool,
) {
    let now = Utc::now();
    let state = app.state::<AppState>();
    {
        let log = state.activity_log.lock().expect("activity_log lock");
        if would_dedupe(&log, now, &message) {
            return;
        }
    }
    let entry = ActivityEntry {
        timestamp: now,
        level,
        message,
        detail,
    };
    match level {
        ActivityLevel::Info => {
            tracing::info!(message = %entry.message, detail = ?entry.detail, "activity")
        }
        ActivityLevel::Warn => {
            tracing::warn!(message = %entry.message, detail = ?entry.detail, "activity");
            if an_glitchtip {
                sentry_init::capture_activity(
                    &entry.message,
                    entry.detail.as_deref(),
                    sentry::Level::Warning,
                );
            }
        }
        ActivityLevel::Error => {
            tracing::error!(message = %entry.message, detail = ?entry.detail, "activity");
            if an_glitchtip {
                sentry_init::capture_activity(
                    &entry.message,
                    entry.detail.as_deref(),
                    sentry::Level::Error,
                );
            }
        }
    }
    let mut log = state.activity_log.lock().expect("activity_log lock");
    log.push_back(entry);
    while log.len() > ACTIVITY_LOG_CAPACITY {
        log.pop_front();
    }
    save_activity_log(&log);
}

/// v0.20 (Process-Integrity): same as `log_activity_handle`, but ALSO
/// appends a `FlightLogEvent::Activity` row to the per-flight JSONL via
/// `record_event()`. Until now that variant existed in the recorder crate
/// but was never constructed anywhere — pause/resume/restart events only
/// ever reached the two mutable, OVERWRITTEN local files
/// (`activity_log.json`, `active_flight.json`) plus Sentry (Warn/Error
/// only). A reviewer looking at the durable per-flight JSONL later had no
/// record any of this ever happened. This gives pause/resume/restart/
/// discontinuity events a permanent home alongside every other
/// `FlightLogEvent` (phase changes, touchdowns, ...).
fn log_activity_and_record(
    app: &AppHandle,
    pirep_id: &str,
    level: ActivityLevel,
    message: impl Into<String>,
    detail: Option<String>,
) {
    let message = message.into();
    let level_str = match level {
        ActivityLevel::Info => "info",
        ActivityLevel::Warn => "warn",
        ActivityLevel::Error => "error",
    };
    record_event(
        app,
        pirep_id,
        &FlightLogEvent::Activity {
            timestamp: Utc::now(),
            level: level_str.to_string(),
            message: message.clone(),
            detail: detail.clone(),
        },
    );
    log_activity_handle(app, level, message, detail);
}

/// Was mit einem Lichtwechsel im Aktivitätsprotokoll geschehen soll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlickerVerdict {
    /// Normaler Schaltvorgang — protokollieren.
    Log,
    /// Blinken gerade erkannt — EINMAL benennen.
    AnnounceFlashing,
    /// Weiterer Wechsel innerhalb eines laufenden Blinkens — schlucken.
    Suppress,
}

// Fenster, über das Wechsel gezählt werden.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> flicker_window_secs (v2.1.24: 30)
// Ab so vielen Wechseln im Fenster gilt die Lampe als blinkend. Vier
// Wechsel in 30 s (= Halteperioden unter ~8 s) schafft kein Mensch am
// Schalter; der gemessene FLASH-Takt lag bei 3 s.
// Wert seit v3: bundle/grenzwerte/cockpit.json -> flicker_min_changes (v2.1.24: 4)

/// Entscheidet, ob ein Lichtwechsel protokolliert, als Blinken gemeldet oder
/// geschluckt wird.
///
/// Bewusst zustandsbehaftet pro Lampe: Ein Flugzeug kann die Landelichter
/// blinken lassen, während die Nav-Lichter normal geschaltet werden — ein
/// gemeinsamer Zähler würde die normalen Schaltvorgänge mitschlucken.
///
/// Das Fenster läuft weiter, solange Wechsel kommen; bleibt es
/// [`FLICKER_WINDOW_SECS`] still, gilt das Blinken als beendet und der
/// nächste Wechsel wird wieder normal protokolliert.
pub(crate) fn flicker_verdict(
    state: &mut std::collections::HashMap<&'static str, (DateTime<Utc>, u8, bool)>,
    lamp: &'static str,
    now: DateTime<Utc>,
) -> FlickerVerdict {
    let entry = state.entry(lamp).or_insert((now, 0, false));
    let (last_change, count, announced) = *entry;

    // Gemessen wird der Abstand zum LETZTEN Wechsel, nicht zum ersten der
    // Serie: Eine blinkende Lampe soll als EINE Serie gelten, solange sie
    // blinkt. Mit einem festen Fenster ab dem ersten Wechsel lief die Serie
    // alle 30 s ab und "Blinkmodus" wurde erneut gemeldet — bei den
    // gemessenen fünf Minuten zehnmal statt einmal.
    if (now - last_change).num_seconds() > crate::grenzwerte::grenzwerte().cockpit.flicker_window_secs.wert {
        *entry = (now, 1, false);
        return FlickerVerdict::Log;
    }

    let count = count.saturating_add(1);
    *entry = (now, count, announced);

    if count < crate::grenzwerte::grenzwerte().cockpit.flicker_min_changes.wert {
        FlickerVerdict::Log
    } else if !announced {
        entry.2 = true;
        FlickerVerdict::AnnounceFlashing
    } else {
        FlickerVerdict::Suppress
    }
}

/// Append one datalink message to the running flight's log.
///
/// No-op when no flight is active — without ACARS running there is no log
/// to write into, and a datalink conversation held outside a flight isn't
/// flight forensics. Deliberately best-effort and silent: a failed log
/// write must never disturb the actual message exchange with ATC.
///
/// `text` is the wire text. Callers must never pass anything derived from
/// the Hoppie logon code.
pub(crate) fn record_datalink(
    app: &AppHandle,
    direction: &str,
    channel: &str,
    station: Option<String>,
    min: Option<u32>,
    mrn: Option<u32>,
    response_code: Option<String>,
    text: String,
) {
    let pirep_id = {
        let state = app.state::<AppState>();
        let guard = state.active_flight.lock().expect("active_flight lock");
        match guard.as_ref() {
            Some(f) => f.pirep_id.clone(),
            None => return,
        }
    };
    record_event(
        app,
        &pirep_id,
        &FlightLogEvent::Datalink {
            timestamp: Utc::now(),
            direction: direction.to_string(),
            channel: channel.to_string(),
            station,
            min,
            mrn,
            response_code,
            text,
        },
    );
}

/// `GET` the entire activity log. Frontend polls this every couple of
/// seconds; `ACTIVITY_LOG_CAPACITY` keeps the payload bounded.
#[tauri::command(async)]
fn activity_log_get(state: tauri::State<'_, AppState>) -> Vec<ActivityEntry> {
    let log = state.activity_log.lock().expect("activity_log lock");
    log.iter().cloned().collect()
}

/// v1.5.0 (#msfs-hud): die jüngsten `limit` Einträge, **neueste zuerst**.
///
/// Eigener Zugriff neben [`activity_log_get`], weil das In-Sim-HUD einen
/// laufenden Ticker füttert und dafür ein bis zwei Zeilen braucht, nicht
/// tausend. Die Sortierung ist hier umgedreht, damit die Anzeige einfach
/// den ersten Eintrag nehmen kann, statt die Länge zu kennen.
/// v1.5.0 (#msfs-hud, QS 09.08.2026): Diagnose-Schalter für den
/// Panel-Server. Die Beta-Abstürze sind ungeklärt; der Schalter erlaubt
/// den A/B-Test „stürzt es auch OHNE Panel-Server ab?", ohne auf v1.4.7
/// zurückzumüssen. Wirkt beim nächsten App-Start (bewusst kein
/// Laufzeit-Stopp/-Start — siehe `panel_server::set_enabled`).
#[tauri::command(async)]
fn panel_server_get_enabled(app: AppHandle) -> bool {
    panel_server::is_enabled(&app)
}

/// Einstellung „X-Plane-Band senden" (ADR-0005), Standard an, wirkt sofort.
#[tauri::command(async)]
fn xplane_band_get_enabled(app: AppHandle) -> bool {
    xplane_band::is_enabled(&app)
}

#[tauri::command]
fn xplane_band_set_enabled(app: AppHandle, enabled: bool) -> Result<bool, UiError> {
    xplane_band::set_enabled(&app, enabled).map_err(|e| UiError::new("xplane_band_config", e))?;
    tracing::info!(enabled, "xplane_band: setting changed");
    Ok(enabled)
}

#[tauri::command]
fn panel_server_set_enabled(app: AppHandle, enabled: bool) -> Result<bool, UiError> {
    panel_server::set_enabled(&app, enabled).map_err(|e| UiError::new("panel_server_config", e))?;
    tracing::info!(
        enabled,
        "panel_server: setting changed (takes effect on next start)"
    );
    Ok(enabled)
}

pub(crate) fn activity_log_tail(state: &AppState, limit: usize) -> Vec<ActivityEntry> {
    let log = state.activity_log.lock().expect("activity_log lock");
    log.iter().rev().take(limit).cloned().collect()
}

/// Wipe the activity log. Useful when the pilot starts a fresh session
/// and doesn't want the previous flight's chatter cluttering the panel.
#[tauri::command(async)]
fn activity_log_clear(state: tauri::State<'_, AppState>) {
    let mut log = state.activity_log.lock().expect("activity_log lock");
    log.clear();
    // Also persist the cleared state — otherwise a restart would
    // restore the pre-clear contents from disk.
    save_activity_log(&log);
}

// v0.13.7: activity_log_add Tauri-Command entfernt — wurde von v0.7.17 fuer
// Frontend-Auto-File-Failure-Logging eingefuehrt, aber nie aus dem Frontend
// aufgerufen. Backend-Auto-File (v0.12.6+) logged direkt via log_activity().
// Audit Q4-2026-05.

// ---- Landing-history commands (Landing tab) ----
