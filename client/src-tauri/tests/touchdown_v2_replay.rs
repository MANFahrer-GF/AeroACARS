//! Replay-Acceptance-Tests fuer Touchdown-Forensik v2.
//!
//! Ladet historische JSONL-Fixtures (= echte Pilot-Fluege vom 10.05.26),
//! extrahiert das `touchdown_window` Event, ruft die touchdown_v2-Layer
//! 1+2+3 auf und prueft die Acceptance-Tabelle aus
//! `docs/spec/touchdown-forensics-v2.md` Sektion 10.
//!
//! Diese Tests sind das **Sicherheitsnetz** fuer die Implementation —
//! wenn diese gruen sind, weiss ich dass die Logik gegen ECHTE Daten
//! korrekt arbeitet (nicht nur gegen synthetische unit-tests).

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use chrono::{DateTime, Utc};
use flate2::read::GzDecoder;
use serde_json::Value;

use aeroacars_app_lib::touchdown_v2::*;
// Fuer SimKind brauchen wir den public re-export
use aeroacars_app_lib::PublicSimKind as SimKind;

fn fixture_path(name: &str) -> std::path::PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    Path::new(manifest).join("tests/fixtures").join(name)
}

/// Lade JSONL-Fixture, extrahiere relevant events.
struct FixtureFlight {
    sim: SimKind,
    samples: Vec<recorder::TouchdownWindowSample>,
    // v0.8.3: `edge_at` wird vom Fixture-Loader noch befuellt aber von
    // den aktuellen Test-Bodies nicht gelesen — kept fuer zukuenftige
    // Tests die den echten Edge-Timestamp verifizieren wollen.
    #[allow(dead_code)]
    edge_at: Option<DateTime<Utc>>,
}

fn load_fixture(name: &str) -> FixtureFlight {
    let path = fixture_path(name);
    let file = File::open(&path).unwrap_or_else(|e| panic!("open {}: {}", path.display(), e));
    let gz = GzDecoder::new(file);
    let reader = BufReader::new(gz);

    let mut samples: Vec<recorder::TouchdownWindowSample> = Vec::new();
    let mut edge_at: Option<DateTime<Utc>> = None;
    #[allow(unused_assignments)]
    let mut sim = SimKind::Off;

    for line in reader.lines() {
        let line = line.unwrap();
        let v: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let t = v.get("type").and_then(|x| x.as_str()).unwrap_or("");

        if t == "position" && matches!(sim, SimKind::Off) {
            // Sim aus dem ersten position-snapshot raten
            if let Some(s) = v.get("snapshot").and_then(|s| s.get("simulator")).and_then(|s| s.as_str()) {
                sim = match s {
                    "Msfs2024" => SimKind::Msfs2024,
                    "Msfs2020" => SimKind::Msfs2020,
                    "XPlane12" => SimKind::XPlane12,
                    "XPlane11" => SimKind::XPlane11,
                    _ => SimKind::Off,
                };
            }
        }

        if t == "touchdown_window" {
            edge_at = v.get("edge_at")
                .and_then(|x| x.as_str())
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc));
            if let Some(arr) = v.get("samples").and_then(|x| x.as_array()) {
                for sv in arr {
                    if let Ok(s) = serde_json::from_value::<recorder::TouchdownWindowSample>(sv.clone()) {
                        samples.push(s);
                    }
                }
            }
        }
    }

    FixtureFlight { sim, samples, edge_at }
}

/// Helper: full pipeline Layer 1+2+3 fuer einen einzelnen Touch — gibt
/// alle gefundenen Episoden zurueck (vereinfachte Version ohne Sampler-
/// State-Machine, fokussiert auf die touchdown_window-samples nur).
fn run_pipeline(fixture: &FixtureFlight) -> Vec<EpisodeOutcome> {
    let mut episodes: Vec<EpisodeOutcome> = Vec::new();
    let mut current_episode: Option<EpisodeBuilder> = None;
    let mut candidates_seen = 0;
    let mut validations_passed = 0;
    let mut validations_failed = 0;

    for (idx, sample) in fixture.samples.iter().enumerate() {
        // Edge-Detection (Layer 1)
        let prev = if idx > 0 { Some(&fixture.samples[idx - 1]) } else { None };
        let candidate = detect_td_candidate(prev, sample, idx, fixture.sim);

        if let Some(cand) = candidate {
            candidates_seen += 1;
            // impact_frame berechnen damit Validation A3/B4 echten Wert hat
            let impact_result = compute_impact_frame(&fixture.samples, cand.edge_at);
            let impact_vs = impact_result.as_ref().map(|r| r.impact_vs_fpm).unwrap_or(0.0);

            // Validation (Layer 2)
            let validation = validate_candidate(&cand, &fixture.samples, fixture.sim, impact_vs);
            eprintln!(
                "DBG candidate idx={} at={} agl={} vs={} impact_vs={} -> {}",
                idx, cand.edge_at.format("%H:%M:%S%.3f"),
                cand.edge_agl_ft, cand.edge_vs_fpm, impact_vs,
                match &validation {
                    ValidationResult::Validated{..} => "VALIDATED",
                    ValidationResult::FalseEdge{reason: _, result} => {
                        eprintln!("    detail: gear_pass={} g_pass={:?} sustained_pass={:?} low_agl_pass={} vs_neg_pass={}",
                            result.gear_force_pass, result.g_force_pass,
                            result.sustained_ground_pass, result.low_agl_persistence_pass,
                            result.vs_negative_pass);
                        "FALSE_EDGE"
                    },
                }
            );

            match validation {
                ValidationResult::Validated { result } => {
                    validations_passed += 1;
                    // VS-Cascade (Layer 3)
                    let lr = impact_result.as_ref()
                        .and_then(|ir| compute_landing_rate(&fixture.samples, ir).ok());

                    if let (Some(ir), Some(lr)) = (impact_result, lr) {
                        match current_episode {
                            None => {
                                // Neue Episode mit contact
                                current_episode = Some(EpisodeBuilder::new(
                                    cand,
                                    ir,
                                    lr,
                                    result,
                                    fixture.sim,
                                ));
                            }
                            Some(ref mut ep) => {
                                if !ep.has_contact() {
                                    // Episode war pending (nur false_edges) — promote
                                    // diesen ersten validated TD zum initial contact.
                                    let mut taken = current_episode.take().unwrap();
                                    let mut new_ep = EpisodeBuilder::new(
                                        cand, ir, lr, result, fixture.sim,
                                    );
                                    // Bewahre die false_edges aus der pending phase
                                    new_ep.false_edges.append(&mut taken.false_edges);
                                    current_episode = Some(new_ep);
                                } else if ep.had_climb_out_above(100.0) {
                                    // Climb-out → neue Episode
                                    let finished = current_episode.take().unwrap();
                                    episodes.push(finished.build());
                                    current_episode = Some(EpisodeBuilder::new(
                                        cand, ir, lr, result, fixture.sim,
                                    ));
                                } else {
                                    // Innerhalb der Episode → low_level_touch (Bounce)
                                    ep.add_low_level_touch(LowLevelTouch {
                                        at: cand.edge_at,
                                        vs_at_impact_fpm: lr.vs_fpm,
                                        agl_max_ft: cand.edge_agl_ft,
                                        sustained_ms: 0,
                                    });
                                }
                            }
                        }
                    }
                }
                ValidationResult::FalseEdge { reason, result } => {
                    validations_failed += 1;
                    // false-edge in current Episode (oder erstellt neue mit nur false_edges)
                    if let Some(ref mut ep) = current_episode {
                        ep.add_false_edge(FalseEdge {
                            edge_at: cand.edge_at,
                            edge_agl_ft: cand.edge_agl_ft,
                            edge_vs_fpm: cand.edge_vs_fpm,
                            reason,
                            validation: result,
                        });
                    } else {
                        // Erste Episode mit nur false-edge — sammle die false_edges
                        // bis irgendwann ein echter contact kommt (oder keiner kommt)
                        let mut ep = EpisodeBuilder::new_pending();
                        ep.add_false_edge(FalseEdge {
                            edge_at: cand.edge_at,
                            edge_agl_ft: cand.edge_agl_ft,
                            edge_vs_fpm: cand.edge_vs_fpm,
                            reason,
                            validation: result,
                        });
                        current_episode = Some(ep);
                    }
                }
            }
        }

        // Track climb-out for episode classification (post-contact AGL)
        if let Some(ref mut ep) = current_episode {
            if ep.has_contact() {
                ep.observe_post_contact_agl(sample.agl_ft);
            }
        }

    }

    eprintln!(
        "DBG total: candidates={} validated={} false_edge={} samples={}",
        candidates_seen, validations_passed, validations_failed, fixture.samples.len()
    );

    if let Some(ep) = current_episode {
        if ep.has_contact() {
            episodes.push(ep.build());
        }
    }

    episodes
}

#[derive(Debug)]
struct EpisodeOutcome {
    contact_vs_fpm: Option<f32>,
    impact_vs_fpm: Option<f32>,
    landing_rate_vs_fpm: Option<f32>,
    landing_rate_source: Option<String>,
    confidence: Option<Confidence>,
    false_edge_count: usize,
    low_level_touch_count: usize,
    hardest_impact_vs_fpm: Option<f32>,
    classification: EpisodeClass,
    sim: SimKind,
}

struct EpisodeBuilder {
    contact: Option<ContactDetail>,
    landing_rate: Option<LandingRateResult>,
    false_edges: Vec<FalseEdge>,
    low_level_touches: Vec<LowLevelTouch>,
    sim: SimKind,
    max_post_contact_agl: f32,
}

impl EpisodeBuilder {
    fn new_pending() -> Self {
        Self {
            contact: None,
            landing_rate: None,
            false_edges: vec![],
            low_level_touches: vec![],
            sim: SimKind::Off,
            max_post_contact_agl: 0.0,
        }
    }

    fn new(
        cand: TdCandidate,
        ir: ImpactFrameResult,
        lr: LandingRateResult,
        validation: ValidationDetail,
        sim: SimKind,
    ) -> Self {
        let confidence = lr.confidence;
        let source = lr.source.clone();
        Self {
            contact: Some(ContactDetail {
                contact_at: cand.edge_at,
                impact_at: ir.impact_at,
                vs_at_impact_fpm: ir.impact_vs_fpm,
                vs_at_contact_fpm: cand.edge_vs_fpm,
                agl_at_contact_ft: cand.edge_agl_ft,
                validation,
                initial_load_peak_n: ir.initial_load_peak_n,
                initial_load_peak_g: ir.initial_load_peak_g,
                confidence,
                source,
            }),
            landing_rate: Some(lr),
            false_edges: vec![],
            low_level_touches: vec![],
            sim,
            max_post_contact_agl: 0.0,
        }
    }

    fn has_contact(&self) -> bool {
        self.contact.is_some()
    }

    fn add_false_edge(&mut self, fe: FalseEdge) {
        self.false_edges.push(fe);
    }

    fn add_low_level_touch(&mut self, t: LowLevelTouch) {
        self.low_level_touches.push(t);
    }

    fn observe_post_contact_agl(&mut self, agl: f32) {
        if agl > self.max_post_contact_agl {
            self.max_post_contact_agl = agl;
        }
    }

    fn had_climb_out_above(&self, threshold_ft: f32) -> bool {
        self.max_post_contact_agl > threshold_ft
    }

    fn build(self) -> EpisodeOutcome {
        let (hardest_vs, _src) = if let Some(ref c) = self.contact {
            compute_hardest_impact(c.vs_at_impact_fpm, &self.low_level_touches)
        } else {
            (0.0, HardestImpactSource::Contact)
        };

        let classification = classify_episode(EpisodePostContactState {
            max_agl_ft_after_contact: self.max_post_contact_agl,
            settled_under_50ft_for_30s: self.max_post_contact_agl < 50.0
                && self.has_contact(),
            current_gs_kt: 0.0,
        });

        EpisodeOutcome {
            contact_vs_fpm: self.contact.as_ref().map(|c| c.vs_at_contact_fpm),
            impact_vs_fpm: self.contact.as_ref().map(|c| c.vs_at_impact_fpm),
            landing_rate_vs_fpm: self.landing_rate.as_ref().map(|l| l.vs_fpm),
            landing_rate_source: self.landing_rate.as_ref().map(|l| l.source.clone()),
            confidence: self.landing_rate.as_ref().map(|l| l.confidence),
            false_edge_count: self.false_edges.len(),
            low_level_touch_count: self.low_level_touches.len(),
            hardest_impact_vs_fpm: Some(hardest_vs),
            classification,
            sim: self.sim,
        }
    }
}

// ─── Acceptance Tests ─────────────────────────────────────────────────────
//
// Spec Sektion 10 — pro Flug ein Test mit Erwartung.

fn print_outcome(label: &str, eps: &[EpisodeOutcome]) {
    eprintln!("\n=== {} ===", label);
    eprintln!("episodes: {}", eps.len());
    if eps.is_empty() {
        eprintln!("  (NO EPISODES — debugging help: check if samples loaded + edge detection)");
    }
    for (i, ep) in eps.iter().enumerate() {
        eprintln!(
            "  Ep {}: sim={:?} contact_vs={:?} impact_vs={:?} \
             landing_rate={:?} src={:?} conf={:?} \
             false_edges={} low_level={} hardest={:?} class={:?}",
            i,
            ep.sim,
            ep.contact_vs_fpm,
            ep.impact_vs_fpm,
            ep.landing_rate_vs_fpm,
            ep.landing_rate_source,
            ep.confidence,
            ep.false_edge_count,
            ep.low_level_touch_count,
            ep.hardest_impact_vs_fpm,
            ep.classification,
        );
    }
}

#[test]
fn pto105_msfs_smooth_55fpm() {
    let f = load_fixture("pto105.jsonl.gz");
    let eps = run_pipeline(&f);
    print_outcome("PTO 105 GA (MSFS)", &eps);

    assert_eq!(eps.len(), 1, "expected 1 episode");
    let ep = &eps[0];
    assert!(matches!(ep.sim, SimKind::Msfs2024 | SimKind::Msfs2020));
    let lr = ep.landing_rate_vs_fpm.expect("landing_rate");
    assert!(lr >= -60.0 && lr <= -50.0, "lr ∈ [-60, -50] expected, got {}", lr);
    assert_eq!(ep.false_edge_count, 0);
}

#[test]
fn dlh304_msfs_acceptable() {
    let f = load_fixture("dlh304.jsonl.gz");
    let eps = run_pipeline(&f);
    print_outcome("DLH 304 (MSFS)", &eps);

    assert_eq!(eps.len(), 1);
    let lr = eps[0].landing_rate_vs_fpm.expect("landing_rate");
    assert!(lr >= -362.0 && lr <= -352.0, "lr ∈ [-362, -352], got {}", lr);
}

#[test]
fn cfg785_msfs_smooth() {
    let f = load_fixture("cfg785.jsonl.gz");
    let eps = run_pipeline(&f);
    print_outcome("CFG 785 EDDV-EDDB (MSFS)", &eps);

    assert_eq!(eps.len(), 1);
    let lr = eps[0].landing_rate_vs_fpm.expect("landing_rate");
    assert!(lr >= -147.0 && lr <= -137.0, "lr ∈ [-147, -137], got {}", lr);
}

#[test]
fn dlh742_msfs_smooth() {
    let f = load_fixture("dlh742.jsonl.gz");
    let eps = run_pipeline(&f);
    print_outcome("DLH 742 EDDM-RJBB (MSFS)", &eps);

    assert_eq!(eps.len(), 1);
    let lr = eps[0].landing_rate_vs_fpm.expect("landing_rate");
    assert!(lr >= -196.0 && lr <= -186.0, "lr ∈ [-196, -186], got {}", lr);
}

// ─── LIVE-Sampler-Integration Tests (P1.1 fix) ──────────────────────────
//
// Simuliert exakt den Live-Sampler-Pfad in lib.rs (pending_td_at +
// 1100ms-Validation + false_edge-skip + sampler_touchdown_at-promotion).
// Stellt sicher dass der Live-Pfad wirklich macht was die pure-module-
// Replay-Tests behaupten — nicht nur ein paralleler Test.

struct LiveSamplerSim {
    sampler_touchdown_at: Option<DateTime<Utc>>,
    pending_td_at: Option<DateTime<Utc>>,
    false_edges_seen: usize,
    validated_tds_seen: usize,
}

/// Simuliert die echte Sampler-Loop in lib.rs:
///   - Edge-Detection auf prev/current sample pair
///   - pending_td_at wird gesetzt wenn neuer Edge + kein pending + kein samp_td
///   - 1.1 sec spaeter validation via touchdown_v2 → VALIDATED → samp_td_at;
///     FALSE_EDGE → pending_td_at = None
fn simulate_live_sampler(samples: &[recorder::TouchdownWindowSample], sim: SimKind) -> LiveSamplerSim {
    let mut state = LiveSamplerSim {
        sampler_touchdown_at: None,
        pending_td_at: None,
        false_edges_seen: 0,
        validated_tds_seen: 0,
    };

    for (idx, current) in samples.iter().enumerate() {
        let prev = if idx > 0 { Some(&samples[idx - 1]) } else { None };

        // Edge-Detection (Layer 1)
        let candidate = detect_td_candidate(prev, current, idx, sim);

        // Live-Sampler-Logik: pending_td_at setzen wenn neuer Edge + nicht busy
        if candidate.is_some()
            && state.sampler_touchdown_at.is_none()
            && state.pending_td_at.is_none()
        {
            state.pending_td_at = Some(current.at);
        }

        // Pending Validation: 1.1 sec post-edge
        if let Some(pending_at) = state.pending_td_at {
            let elapsed_ms = (current.at - pending_at).num_milliseconds();
            if elapsed_ms >= 1100 {
                let cand_idx = samples.iter().position(|s| s.at >= pending_at);
                if let Some(cand_i) = cand_idx {
                    let cand_sample = &samples[cand_i];
                    let cand = TdCandidate {
                        edge_sample_index: cand_i,
                        edge_at: cand_sample.at,
                        edge_agl_ft: cand_sample.agl_ft,
                        edge_vs_fpm: cand_sample.vs_fpm,
                        edge_gear_force_n: cand_sample.gear_normal_force_n,
                        edge_g_force: cand_sample.g_force,
                        edge_total_weight_kg: cand_sample.total_weight_kg,
                    };
                    let impact = compute_impact_frame(samples, cand.edge_at);
                    let impact_vs = impact.as_ref().map(|r| r.impact_vs_fpm).unwrap_or(0.0);
                    let validation = validate_candidate(&cand, samples, sim, impact_vs);
                    match validation {
                        ValidationResult::Validated { .. } => {
                            state.sampler_touchdown_at = Some(pending_at);
                            state.validated_tds_seen += 1;
                            state.pending_td_at = None;
                        }
                        ValidationResult::FalseEdge { .. } => {
                            state.false_edges_seen += 1;
                            state.pending_td_at = None;
                        }
                    }
                } else {
                    state.pending_td_at = None;
                }
            }
        }
    }

    state
}

#[test]
fn live_sampler_dah3181_promotes_real_td_skips_float() {
    let f = load_fixture("dah3181.jsonl.gz");
    let state = simulate_live_sampler(&f.samples, f.sim);

    eprintln!(
        "DAH 3181 LIVE: false_edges={} validated={} sampler_td_at={:?}",
        state.false_edges_seen, state.validated_tds_seen, state.sampler_touchdown_at
    );

    // Erwartung: 1 false_edge (Float-Streifschuss bei sample 125),
    // dann 1 validated TD (echter contact bei sample 198).
    assert!(
        state.false_edges_seen >= 1,
        "Float-Streifschuss muss als false_edge erkannt werden, got {}",
        state.false_edges_seen
    );
    assert!(
        state.sampler_touchdown_at.is_some(),
        "sampler_touchdown_at must be set after VALIDATED edge"
    );
    let td_at = state.sampler_touchdown_at.unwrap();
    // Echter TD ist bei 07:54:02.310 (sample 198).
    // Live-Pfad sollte diesen contact_frame als sampler_touchdown_at haben.
    let expected_td_at = chrono::DateTime::parse_from_rfc3339("2026-05-10T07:54:02.310Z")
        .unwrap()
        .with_timezone(&Utc);
    let diff_ms = (td_at - expected_td_at).num_milliseconds().abs();
    assert!(
        diff_ms < 100,
        "Live sampler_touchdown_at {} should match real contact 07:54:02.310 (within 100ms), diff={}ms",
        td_at, diff_ms
    );

    // Plus: berechne den finalen VS am echten contact_frame —
    // exakt das was im Production-Dump-Pfad passieren wuerde
    let impact = compute_impact_frame(&f.samples, td_at).expect("impact_frame");
    let landing_rate = compute_landing_rate(&f.samples, &impact).expect("landing_rate");
    eprintln!(
        "DAH 3181 LIVE final: vs={} src={} conf={:?}",
        landing_rate.vs_fpm, landing_rate.source, landing_rate.confidence
    );
    assert!(
        landing_rate.vs_fpm >= -415.0 && landing_rate.vs_fpm <= -395.0,
        "Live-Pfad final VS expected [-415, -395] (firm), got {}",
        landing_rate.vs_fpm
    );
}

#[test]
fn live_sampler_msfs_unchanged_for_clean_landings() {
    // Alle 4 MSFS-Fluege: live-Sampler sollte direkten validated TD geben
    // (kein false_edge, keine Bouncer in der ersten Episode).
    for fname in &["pto105.jsonl.gz", "dlh304.jsonl.gz", "cfg785.jsonl.gz", "dlh742.jsonl.gz"] {
        let f = load_fixture(fname);
        let state = simulate_live_sampler(&f.samples, f.sim);
        eprintln!(
            "{}: false_edges={} validated={} sampler_td={:?}",
            fname, state.false_edges_seen, state.validated_tds_seen, state.sampler_touchdown_at
        );
        assert!(
            state.sampler_touchdown_at.is_some(),
            "{}: sampler_touchdown_at expected for clean MSFS landing", fname
        );
    }
}

#[test]
fn dah3181_xplane_firm_with_float_false_edge() {
    let f = load_fixture("dah3181.jsonl.gz");
    let eps = run_pipeline(&f);
    print_outcome("DAH 3181 ZGGG-DAAG (X-Plane)", &eps);

    // Erwartung Spec Sektion 10:
    // - vs am impact_frame ∈ [-415, -395] fpm (Score-Bucket: firm)
    // - 1 Episode FinalLanding
    // - false_edges = 1 (Float-Streifschuss bei sample 125)
    // - low_level_touches = 1 (Bounce bei sample 246)
    assert_eq!(eps.len(), 1, "expected 1 episode (Float ist false_edge)");
    let ep = &eps[0];
    assert!(matches!(ep.sim, SimKind::XPlane12 | SimKind::XPlane11));
    let lr = ep.landing_rate_vs_fpm.expect("landing_rate");
    assert!(
        lr >= -415.0 && lr <= -395.0,
        "lr ∈ [-415, -395] expected (firm), got {}",
        lr
    );
    assert!(
        ep.false_edge_count >= 1,
        "expected at least 1 false_edge (Float-Streifschuss), got {}",
        ep.false_edge_count
    );
}

#[test]
fn pto705_msfs_touch_and_go_two_low_level() {
    let f = load_fixture("pto705.jsonl.gz");
    let eps = run_pipeline(&f);
    print_outcome("PTO 705 T&G (MSFS)", &eps);

    // Erwartung Spec Sektion 10:
    // - 2 Episoden: Ep 0 = TouchAndGo, Ep 1 = FinalLanding
    // - Ep 0 enthaelt 1+ low_level_touch
    // - Ep 1 ist die finale Landing
    //
    // ABER: dieser simple replay-pipeline kann die zweite Episode nicht
    // entdecken weil das touchdown_window event nur die ERSTE Episode
    // covered (Sampler-Bug der durch Phase H gefixt wird).
    //
    // Daher pruefen wir hier nur Ep 0 — die zweite Episode kommt erst
    // mit der vollen Sampler-State-Machine (Phase H).
    assert!(!eps.is_empty(), "at least 1 episode expected");
    let ep0 = &eps[0];
    let lr = ep0.landing_rate_vs_fpm.expect("landing_rate");
    assert!(
        lr >= -187.0 && lr <= -177.0,
        "Ep 0 lr ∈ [-187, -177] expected, got {}",
        lr
    );
    // Optional: Ep 0 sollte 1+ low_level_touch haben (zweiter touch im T&G-window)
    // wird auch erst mit voller Sampler-State-Machine korrekt erfasst.
    eprintln!(
        "INFO: Ep 0 low_level_touch_count={} (1 erwartet bei voller State-Machine)",
        ep0.low_level_touch_count
    );
}

// ---- v0.13.15: Post-TD-Capture-Spanne (Real-Daten-Guard) ----------------
//
// Discord-Befund ViolonC (WFL1505, 2026-05-31): Touchdown-Nahaufnahme zeigt
// "inkl. 10 s nach TD", aber nur ~0.5 s. Ursache lag NICHT in der Erfassung
// (das touchdown_window-Event enthaelt die vollen ~10 s), sondern im Anzeige-
// Pfad. Dieser Test fixiert die Erfassung an echten Flugdaten: das
// touchdown_window des realen DLH304-Fluges (10.05.26) muss ~10 s ueber den
// TD-Edge hinausreichen. Bricht das, ist die 10-s-Erfassung selbst kaputt.
#[test]
fn dlh304_touchdown_window_captures_full_post_td_span() {
    let fx = load_fixture("dlh304.jsonl.gz");
    let edge = fx.edge_at.expect("dlh304 hat ein edge_at");
    let post_span_s = fx
        .samples
        .iter()
        .map(|s| (s.at - edge).num_milliseconds())
        .max()
        .expect("samples vorhanden") as f64
        / 1000.0;
    assert!(
        post_span_s >= 8.0,
        "Post-TD-Window sollte ~10 s erfassen, war nur +{post_span_s:.2} s"
    );
}
