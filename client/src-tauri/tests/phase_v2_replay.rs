//! v0.16.12 (#phase-v2): Golden-Arc-Replay-Tests für die Schatten-
//! Phasen-Engine v2.
//!
//! Replayt die EXISTIERENDEN JSONL-Fixtures (echte, anonymisierte
//! Pilot-Flüge) durch die v2-Engine — exakt so, wie der Streamer sie im
//! Schatten-Modus füttert: Position-Snapshots als Samples, die
//! historisch aufgezeichneten `phase_changed`-Events als „alte FSM"
//! (Sync-Quelle außerhalb des En-Route-Bands).
//!
//! Geprüft wird:
//!   (a) **Kein Flap**: nirgends Descent→Cruise→Descent binnen < 5 min
//!       (die 193-Flüge-Bug-Klasse aus dem Daten-Audit).
//!   (b) **Terminal-Bogen**: wo das Fixture landet, erreicht auch die
//!       Schatten-Engine Approach + Landing.
//!   (c) **Golden Arcs**: der komplette Schatten-Phasen-Bogen als
//!       Erwartungs-String — zukünftige Engine-Änderungen diffen hier
//!       sichtbar.
//!
//! `cruise_ref` ist im Replay None (Fixtures tragen kein Plan-Level) —
//! validiert damit bewusst die ref-losen Dauer-Heuristiken am Korpus.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use serde_json::Value;
use sim_core::FlightPhase;

use aeroacars_app_lib::phase_v2::ShadowPhaseEngine;

fn fixture_path(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push(name);
    p
}

fn parse_ts(v: &Value) -> Option<DateTime<Utc>> {
    v.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

fn parse_phase(v: &Value) -> Option<FlightPhase> {
    serde_json::from_value::<FlightPhase>(v.clone()).ok()
}

/// Ein Replay-Event in chronologischer Datei-Reihenfolge.
enum ReplayEvent {
    /// `phase_changed` → neue „alte FSM"-Phase ab diesem Zeitpunkt.
    OldPhase(FlightPhase),
    /// `position` → ein Schatten-Engine-Tick.
    Position {
        t: DateTime<Utc>,
        alt_msl_ft: f64,
        agl_ft: f64,
        vs_fpm: f32,
        on_ground: bool,
    },
}

/// Fixture laden. Bewusst KEINE volle SimSnapshot-Deserialisierung —
/// ältere Fixtures kennen neuere Pflichtfelder nicht; die Engine braucht
/// ohnehin nur die 4 kinematischen Werte (wie der echte Streamer-Hook).
fn load_replay(name: &str) -> (FlightPhase, Vec<ReplayEvent>) {
    let f = File::open(fixture_path(name)).expect("fixture file");
    let r = BufReader::new(GzDecoder::new(f));
    let mut events = Vec::new();
    let mut initial_old: Option<FlightPhase> = None;
    for line in r.lines() {
        let Ok(line) = line else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        match v["type"].as_str().unwrap_or("") {
            "phase_changed" => {
                if initial_old.is_none() {
                    initial_old = parse_phase(&v["from"]);
                }
                if let Some(p) = parse_phase(&v["to"]) {
                    events.push(ReplayEvent::OldPhase(p));
                }
            }
            "position" => {
                let s = &v["snapshot"];
                let (Some(t), Some(alt), Some(agl), Some(vs), Some(og)) = (
                    parse_ts(&v["timestamp"]),
                    s["altitude_msl_ft"].as_f64(),
                    s["altitude_agl_ft"].as_f64(),
                    s["vertical_speed_fpm"].as_f64(),
                    s["on_ground"].as_bool(),
                ) else {
                    continue;
                };
                events.push(ReplayEvent::Position {
                    t,
                    alt_msl_ft: alt,
                    agl_ft: agl,
                    vs_fpm: vs as f32,
                    on_ground: og,
                });
            }
            _ => {}
        }
    }
    // Flüge starten in der echten FSM bei Boarding; Fixtures, die
    // mid-flight beginnen (phase_valid_holding), liefern das `from`
    // ihres ersten phase_changed.
    (initial_old.unwrap_or(FlightPhase::Boarding), events)
}

/// Replay: liefert die Schatten-Transitions als (Zeit, Phase)-Liste
/// (inkl. Start-Phase des ersten Ticks).
fn run_shadow(name: &str) -> Vec<(DateTime<Utc>, FlightPhase)> {
    let (mut old_phase, events) = load_replay(name);
    let mut engine = ShadowPhaseEngine::default();
    let mut arc: Vec<(DateTime<Utc>, FlightPhase)> = Vec::new();
    for ev in events {
        match ev {
            ReplayEvent::OldPhase(p) => old_phase = p,
            ReplayEvent::Position { t, alt_msl_ft, agl_ft, vs_fpm, on_ground } => {
                let (shadow, _segment) =
                    engine.step(t, alt_msl_ft, agl_ft, vs_fpm, on_ground, old_phase, None);
                if arc.last().map(|(_, p)| *p) != Some(shadow) {
                    arc.push((t, shadow));
                }
            }
        }
    }
    arc
}

fn arc_string(arc: &[(DateTime<Utc>, FlightPhase)]) -> String {
    arc.iter()
        .map(|(_, p)| format!("{p:?}"))
        .collect::<Vec<_>>()
        .join(">")
}

/// (a) Die 193-Flüge-Bug-Klasse: Descent→Cruise gefolgt von
/// Cruise→Descent binnen < 5 min darf NIRGENDS vorkommen.
fn assert_no_descent_cruise_flap(name: &str, arc: &[(DateTime<Utc>, FlightPhase)]) {
    for w in arc.windows(3) {
        let [(_, a), (t1, b), (t2, c)] = w else { continue };
        if *a == FlightPhase::Descent
            && *b == FlightPhase::Cruise
            && *c == FlightPhase::Descent
        {
            let secs = (*t2 - *t1).num_seconds();
            assert!(
                secs >= 300,
                "{name}: Descent→Cruise→Descent-Flap binnen {secs}s (< 5 min)"
            );
        }
    }
}

/// (b) Terminal-Bogen: landet das Fixture (alte FSM erreichte Landing),
/// muss auch der Schatten-Bogen Approach + Landing enthalten.
fn assert_terminal_arc(name: &str, arc: &[(DateTime<Utc>, FlightPhase)]) {
    let has = |p: FlightPhase| arc.iter().any(|(_, q)| *q == p);
    assert!(has(FlightPhase::Approach), "{name}: Schatten-Bogen ohne Approach");
    assert!(has(FlightPhase::Landing), "{name}: Schatten-Bogen ohne Landing");
}

/// (c) Golden Arc: kompletter Schatten-Bogen als Erwartungs-String.
/// Bei bewussten Engine-Änderungen hier aktualisieren — der Diff zeigt
/// die Verhaltensänderung pro Real-Flug explizit.
fn assert_golden(name: &str, expected: &str) -> Vec<(DateTime<Utc>, FlightPhase)> {
    let arc = run_shadow(name);
    let actual = arc_string(&arc);
    assert_eq!(
        actual, expected,
        "{name}: Schatten-Arc weicht vom Golden-Arc ab\n  actual:   {actual}\n  expected: {expected}"
    );
    assert_no_descent_cruise_flap(name, &arc);
    arc
}

// ─── Volle Airline-Flüge (Boarding → Arrived) ───────────────────────────
//
// Replay-Artefakt, das mehrere Goldens teilt: nach dem letzten
// `phase_changed` (→ Arrived) enthalten einige Fixtures keine
// `position`-Events mehr — die Schatten-Engine tickt dann nie mit
// old==Arrived und der Golden-Arc endet bei BlocksOn. Im Live-Betrieb
// tickt der Streamer weiter (30-s-Heartbeat) und synct Arrived sofort.

#[test]
fn golden_arc_cfg785() {
    // Kurzstrecken-Hop mit nur ~5 min „Cruise" auf FL225, in der das
    // Aircraft bereits wieder 5000 ft verlor — die alte FSM-Cruise war
    // hier selbst ein Premature-Latch. Ohne cruise_ref (Replay: None)
    // braucht v2 ≥ 240 s gehaltenes Level-Segment → der kurze Level-
    // Abschnitt qualifiziert nicht, der Bogen geht ehrlich
    // Climb→Descent. Mit Plan-Level (Produktion) wäre der Level-
    // Abschnitt am ref sofort Cruise.
    let arc = assert_golden(
        "cfg785.jsonl.gz",
        "Boarding>Pushback>TaxiOut>TakeoffRoll>Takeoff>Climb>Descent>Approach>Final>Landing>TaxiIn>BlocksOn",
    );
    assert_terminal_arc("cfg785", &arc);
}

#[test]
fn golden_arc_dah3181() {
    // Langstrecke mit Step-Climbs: ohne cruise_ref klassifiziert v2
    // die ≥ 60 s gehaltenen Climbing-Segmente zwischen den Levels
    // ehrlich als Climb (Cruise>Climb>Cruise). Mit Plan-Level bleibt
    // ein Step-Climb am/über ref durchgehend Cruise (Unit-Test
    // `step_climb_at_ref_stays_cruise`).
    let arc = assert_golden(
        "dah3181.jsonl.gz",
        "Boarding>Pushback>TaxiOut>TakeoffRoll>Takeoff>Climb>Cruise>Climb>Cruise>Climb>Cruise>Descent>Approach>Final>Landing>TaxiIn>BlocksOn>Arrived",
    );
    assert_terminal_arc("dah3181", &arc);
}

#[test]
fn golden_arc_dlh304() {
    let arc = assert_golden(
        "dlh304.jsonl.gz",
        "Boarding>Pushback>TaxiOut>TakeoffRoll>Takeoff>Climb>Cruise>Descent>Approach>Final>Landing>TaxiIn>BlocksOn",
    );
    assert_terminal_arc("dlh304", &arc);
}

#[test]
fn golden_arc_dlh742_holding_flight() {
    // Das Fixture enthält 3 echte Holding-Episoden der alten FSM —
    // v2 modelliert Holding nicht (dokumentierter Diff) und bleibt
    // währenddessen Cruise. Die Cruise>Climb-Episoden sind ref-lose
    // Step-Climbs (siehe dah3181).
    let arc = assert_golden(
        "dlh742.jsonl.gz",
        "Boarding>Pushback>TaxiOut>TakeoffRoll>Takeoff>Climb>Cruise>Climb>Cruise>Climb>Cruise>Climb>Cruise>Descent>Approach>Final>Landing>TaxiIn>BlocksOn",
    );
    assert_terminal_arc("dlh742", &arc);
    assert!(
        !arc.iter().any(|(_, p)| *p == FlightPhase::Holding),
        "v2 darf in Runde 1 nie Holding melden"
    );
}

// ─── Kurzflüge + Go-Around ──────────────────────────────────────────────

#[test]
fn golden_arc_pto105() {
    // Kurzflug ohne Cruise (auch die alte FSM sah keinen) — v2 deckungsgleich.
    let arc = assert_golden(
        "pto105.jsonl.gz",
        "Boarding>TaxiOut>TakeoffRoll>Takeoff>Climb>Descent>Approach>Final>Landing>TaxiIn>BlocksOn",
    );
    assert_terminal_arc("pto105", &arc);
}

#[test]
fn golden_arc_pto705_go_around() {
    // Echter Go-Around: alte FSM flog Final→Climb→…→Final→Landing.
    // v2 reproduziert den kompletten Doppel-Anflug deckungsgleich.
    let arc = assert_golden(
        "pto705.jsonl.gz",
        "Boarding>TaxiOut>TakeoffRoll>Takeoff>Climb>Descent>Approach>Final>Climb>Descent>Approach>Final>Landing>TaxiIn>BlocksOn",
    );
    assert_terminal_arc("pto705", &arc);
}

#[test]
fn golden_arc_phase_holding_pending_leak() {
    // Anflug-Ausschnitt (Positions beginnen erst bei 723 ft AGL im
    // Sinkflug und enden im Go-Around-Steigflug): v2 synct auf
    // Approach, folgt nach Final und in den GA-Climb — und zeigt nie
    // die (Bug-)Holding-Episode der alten FSM.
    let arc = assert_golden(
        "phase_holding_pending_leak.jsonl.gz",
        "Approach>Final>Climb",
    );
    assert!(!arc.iter().any(|(_, p)| *p == FlightPhase::Holding));
}

// ─── Ausschnitts-Fixtures (kein kompletter Flug) ────────────────────────

#[test]
fn golden_arc_phase_valid_holding_window() {
    // Cruise-Ausschnitt mit echtem Hold: v2 bleibt durchgehend Cruise.
    let arc = assert_golden("phase_valid_holding.jsonl.gz", "Cruise");
    assert_eq!(arc.len(), 1);
}

#[test]
fn golden_arc_phase_arrived_fallback_rolling() {
    // Bug-Fixture: alte FSM hing airborne in Pushback fest und sprang
    // per Universal-Fallback auf Arrived. v2 spiegelt das Boden-/
    // Terminal-Band 1:1 — alle Positions liegen zwischen den
    // phase_changed-Events (→Pushback / →Arrived), daher besteht der
    // Schatten-Bogen nur aus Pushback. Der Fix dieser Klasse gehört
    // der alten FSM (v0.7.5) — v2 ändert am Boden-Band bewusst nichts.
    assert_golden("phase_arrived_fallback_rolling.jsonl.gz", "Pushback");
}
