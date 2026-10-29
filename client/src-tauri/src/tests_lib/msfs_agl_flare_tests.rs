    use super::*;

    /// Build one `TouchdownWindowSample` at offset `ms` from a base epoch,
    /// with the given AGL (ft), pitch (deg), on_ground flag, and SimVar
    /// V/S (fpm). `vs_fpm` is what the LAGGED SimVar path reads; the AGL
    /// fit reads only `agl_ft` / `at` / `on_ground`.
    /// wie `tw`, aber mit gesetzter Hoehe ueber dem Meeresspiegel.
    #[allow(clippy::too_many_arguments)]
    fn tw_msl(
        base: DateTime<Utc>,
        ms: i64,
        agl_ft: f32,
        msl_ft: f32,
        pitch_deg: f32,
        on_ground: bool,
        vs_fpm: f32,
    ) -> TouchdownWindowSample {
        let mut s = tw(base, ms, agl_ft, pitch_deg, on_ground, vs_fpm);
        s.msl_ft = Some(msl_ft);
        s
    }

    pub(super) fn tw(
        base: DateTime<Utc>,
        ms: i64,
        agl_ft: f32,
        pitch_deg: f32,
        on_ground: bool,
        vs_fpm: f32,
    ) -> TouchdownWindowSample {
        TouchdownWindowSample {
            at: base + chrono::Duration::milliseconds(ms),
            vs_fpm,
            g_force: 1.0,
            g_semibody: None,
            on_ground,
            agl_ft,
            msl_ft: Some(agl_ft + 500.0),
            heading_true_deg: 0.0,
            groundspeed_kt: 120.0,
            indicated_airspeed_kt: 130.0,
            true_airspeed_kt: 130.0,
            lat: 0.0,
            lon: 0.0,
            pitch_deg,
            bank_deg: 0.0,
            gear_normal_force_n: None,
            total_weight_kg: Some(60_000.0),
        }
    }

    /// Synthesize an AGL trace over [-2000, 0] ms at 50 Hz from a piecewise
    /// constant sink-rate profile (fpm, negative = descending). Each entry
    /// `(from_ms, sink_fpm)` applies until the next entry's `from_ms`. AGL
    /// integrates the sink backward from `agl_at_edge_ft` at t=0. `pitch_fn`
    /// supplies pitch per sample; `on_ground` is false until t≥0.
    fn synth_trace(
        base: DateTime<Utc>,
        agl_at_edge_ft: f32,
        profile: &[(i64, f32)],
        pitch_fn: impl Fn(i64) -> f32,
    ) -> Vec<TouchdownWindowSample> {
        // First build AGL forward from -2000 to 0 by integrating sink.
        let dt_ms = 20_i64; // 50 Hz
        let sink_at = |ms: i64| -> f32 {
            let mut cur = profile[0].1;
            for &(from_ms, s) in profile {
                if ms >= from_ms {
                    cur = s;
                }
            }
            cur
        };
        // Integrate forward so AGL hits exactly agl_at_edge_ft at t=0.
        // agl(t+dt) = agl(t) + sink_fpm/60 * (dt/1000)  [sink negative]
        let mut times: Vec<i64> = Vec::new();
        let mut t = -2000_i64;
        while t <= 0 {
            times.push(t);
            t += dt_ms;
        }
        // Compute AGL at each time relative to a running value, then shift
        // so the t=0 sample equals agl_at_edge_ft.
        let mut agls = vec![0.0_f32; times.len()];
        for i in 1..times.len() {
            let dt_sec = (times[i] - times[i - 1]) as f32 / 1000.0;
            agls[i] = agls[i - 1] + sink_at(times[i - 1]) / 60.0 * dt_sec;
        }
        let shift = agl_at_edge_ft - *agls.last().unwrap();
        times
            .iter()
            .zip(agls.iter())
            .map(|(&ms, &a)| {
                let on_ground = ms >= 0;
                tw(base, ms, a + shift, pitch_fn(ms), on_ground, sink_at(ms))
            })
            .collect()
    }

    fn base() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-16T12:00:02.000000Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    /// The AGL fit must recover a known constant sink rate from a clean
    /// linear AGL ramp (lag-free truth), within a few fpm.
    #[test]
    fn agl_fit_recovers_constant_sink() {
        let b = base();
        // constant -300 fpm, flat pitch, AGL 20 ft at edge.
        let s = synth_trace(b, 20.0, &[(-2000, -300.0)], |_| 3.0);
        let edge_ms = b.timestamp_millis();
        // fit at -1000ms should read ~-300 (×cos(3°) ≈ ×0.9986 ⇒ ~-299.6)
        let v = agl_geometric_vs_fpm_at(&s, edge_ms - 1000, 3.0, None).expect("fit");
        assert!(
            (v - (-300.0 * (3.0_f32).to_radians().cos())).abs() < 8.0,
            "got {v}"
        );
    }

    /// Too few samples (or all above the AGL ceiling) → None, never a wild
    /// slope. Guards the noise floor.
    #[test]
    fn agl_fit_rejects_sparse_or_high() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // Only 2 samples in the ±300ms window → below MIN_SAMPLES.
        let sparse = vec![
            tw(b, -1000, 20.0, 3.0, false, -300.0),
            tw(b, -700, 18.0, 3.0, false, -300.0),
        ];
        assert!(agl_geometric_vs_fpm_at(&sparse, edge_ms - 850, 3.0, None).is_none());
        // Dense but all AGL above the ceiling → None.
        let high: Vec<_> = (0..20)
            .map(|i| tw(b, -1000 + i * 20, 500.0, 3.0, false, -300.0))
            .collect();
        assert!(agl_geometric_vs_fpm_at(&high, edge_ms - 800, 3.0, None).is_none());
    }

    /// REAL flare: sink reduces from -560 (early) to -360 (late) as pitch
    /// rises — endpoints must show a genuine reduction ≥ ~150 fpm and the
    /// metric must DETECT it. Mirrors JBU323's geometry.
    #[test]
    fn endpoints_detect_real_flare() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // steep early, shallow late: a deliberate flare.
        let profile = [(-2000, -560.0), (-900, -460.0), (-400, -360.0)];
        let s = synth_trace(b, 16.0, &profile, |ms| {
            // pitch ramps 2.5° → 5.2° over the window
            let frac = ((ms + 2000) as f32 / 2000.0).clamp(0.0, 1.0);
            2.5 + 2.7 * frac
        });
        let (peak, end) = compute_msfs_agl_flare_endpoints(&s, edge_ms - 2000, edge_ms - 100);
        let peak = peak.expect("peak");
        let end = end.expect("end");
        let red = end - peak;
        assert!(
            peak < -500.0,
            "peak should reflect the steep early sink, got {peak}"
        );
        assert!(
            end > -420.0,
            "end should reflect the shallow late sink, got {end}"
        );
        assert!(
            red > 100.0,
            "real flare must show a reduction > 100 fpm, got {red}"
        );
    }

    /// NO flare: constant sink the whole window (firm, pilot never pulled)
    /// → the AGL endpoints must show ~zero reduction. NO PHANTOM.
    #[test]
    fn endpoints_no_phantom_on_constant_sink() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        let s = synth_trace(b, 16.0, &[(-2000, -380.0)], |_| 3.0);
        let (peak, end) = compute_msfs_agl_flare_endpoints(&s, edge_ms - 2000, edge_ms - 100);
        let red = end.unwrap() - peak.unwrap();
        assert!(
            red.abs() < 50.0,
            "constant sink must NOT register a flare, got reduction {red}"
        );
    }

    /// End-to-end on the JSON: a constant-sink MSFS trace must report
    /// flare_detected=false, source "msfs_agl", and the lagged SimVar
    /// values preserved in the `_raw` keys.
    #[test]
    fn analysis_msfs_constant_sink_no_phantom_with_provenance() {
        let b = base();
        // SimVar deliberately lags (reads a different shallow value) so we
        // can prove the published endpoints came from AGL, not vs_fpm.
        let s = synth_trace(b, 16.0, &[(-2000, -380.0)], |_| 3.0);
        let a = compute_landing_analysis(&s, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            a.get("flare_vs_source").and_then(|v| v.as_str()),
            Some("msfs_agl")
        );
        // _raw keys present on MSFS.
        assert!(a.get("peak_vs_pre_flare_fpm_raw").is_some());
        assert!(a.get("vs_at_flare_end_fpm_raw").is_some());
    }

    /// X-Plane output must be byte-identical to the pre-v0.16.22 SimVar
    /// path: NO new flare keys, endpoints from `vs_fpm`.
    #[test]
    fn analysis_xplane_byte_identical_no_new_keys() {
        let b = base();
        let s = synth_trace(b, 16.0, &[(-2000, -560.0), (-900, -360.0)], |_| 3.0);
        let a = compute_landing_analysis(&s, b, Simulator::XPlane12, None);
        assert!(
            a.get("flare_vs_source").is_none(),
            "X-Plane must add no source key"
        );
        assert!(
            a.get("peak_vs_pre_flare_fpm_raw").is_none(),
            "X-Plane must add no _raw key"
        );
        assert!(
            a.get("vs_at_flare_end_fpm_raw").is_none(),
            "X-Plane must add no _raw key"
        );
        // The published endpoints equal the SimVar pitch-corrected values.
        // (cos(3°) ≈ 0.99863; vs_fpm at the flare-window samples.)
        let peak = a
            .get("peak_vs_pre_flare_fpm")
            .and_then(|v| v.as_f64())
            .unwrap();
        assert!(peak < -500.0, "X-Plane peak from SimVar, got {peak}");
    }

    /// FIX 4 (flare_dvs_dt source consistency). On the MSFS AGL path the
    /// published `flare_reduction_fpm` is AGL-derived; `flare_dvs_dt` must use
    /// the SAME source so the two diagnostics agree in SIGN. A genuine flare
    /// (positive reduction) must yield a POSITIVE dVS/dt (sink decaying toward
    /// zero). On X-Plane / Other the dVS/dt stays SimVar-derived (unchanged).
    #[test]
    fn flare_dvs_dt_follows_published_source() {
        // synth_trace writes `vs_fpm` == the geometric sink, so for a clean
        // synthetic AGL == SimVar; to PROVE dvs_dt follows the AGL source we
        // additionally check the SIGN agrees with the AGL reduction.
        let b = base();
        // Real flare: steep early -560 → shallow late -360, pitch rising.
        let s = synth_trace(
            b,
            16.0,
            &[(-2000, -560.0), (-900, -460.0), (-400, -360.0)],
            |ms| {
                let frac = ((ms + 2000) as f32 / 2000.0).clamp(0.0, 1.0);
                2.5 + 2.7 * frac
            },
        );
        let a = compute_landing_analysis(&s, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("flare_vs_source").and_then(|v| v.as_str()),
            Some("msfs_agl")
        );
        let red = a
            .get("flare_reduction_fpm")
            .and_then(|v| v.as_f64())
            .unwrap();
        let dvs = a
            .get("flare_dvs_dt_fpm_per_sec")
            .and_then(|v| v.as_f64())
            .unwrap();
        assert!(red > 100.0, "real flare reduction expected, got {red}");
        // The KEY assertion: dvs_dt sign agrees with the AGL reduction.
        // Pre-v0.16.22 FIX 4 this could disagree (SimVar end vs AGL reduction).
        assert!(
            dvs > 0.0,
            "MSFS AGL dvs_dt must be POSITIVE (consistent with the positive AGL reduction), got {dvs}"
        );

        // X-Plane: dvs_dt stays SimVar-derived. With this same descending-
        // then-shallowing profile the SimVar dvs_dt is also positive, but the
        // point is it is computed from the SimVar pass — assert it is present
        // and finite (byte-identical path; no AGL involvement).
        let ax = compute_landing_analysis(&s, b, Simulator::XPlane12, None);
        let dvs_x = ax.get("flare_dvs_dt_fpm_per_sec").and_then(|v| v.as_f64());
        assert!(
            dvs_x.is_some_and(|v| v.is_finite()),
            "X-Plane dvs_dt must be SimVar-derived and present"
        );
    }

    /// Real-fixture golden: the committed trimmed JBU323 flight. The
    /// recorded (lagged) analysis says flare_detected=false (reduction
    /// ~30 fpm); re-running compute_landing_analysis with MSFS must flip
    /// it to TRUE on the lag-free AGL geometry, while the touchdown rate
    /// (vs_at_edge_fpm) stays the recorded firm value (-423).
    #[test]
    fn jbu323_real_fixture_flare_flips_true_touchdown_unchanged() {
        use flate2::read::GzDecoder;
        use std::io::{BufRead, BufReader};

        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/jbu323_msfs_late_flare.jsonl.gz");
        let file = std::fs::File::open(&path).expect("open jbu323 fixture");
        let reader = BufReader::new(GzDecoder::new(file));

        let mut edge_at: Option<DateTime<Utc>> = None;
        let mut samples: Vec<TouchdownWindowSample> = Vec::new();
        let mut recorded: Option<serde_json::Value> = None;
        for line in reader.lines() {
            let line = line.unwrap();
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                "touchdown_window" => {
                    edge_at = v
                        .get("edge_at")
                        .and_then(|x| x.as_str())
                        .map(|s| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc));
                    for sv in v.get("samples").and_then(|x| x.as_array()).unwrap() {
                        let f = |k: &str| sv.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                        let f64v = |k: &str| sv.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                        samples.push(TouchdownWindowSample {
                            at: DateTime::parse_from_rfc3339(sv["at"].as_str().unwrap())
                                .unwrap()
                                .with_timezone(&Utc),
                            vs_fpm: f("vs_fpm"),
                            g_force: f("g_force"),
                            g_semibody: None,
                            on_ground: sv
                                .get("on_ground")
                                .and_then(|x| x.as_bool())
                                .unwrap_or(false),
                            agl_ft: f("agl_ft"),
                            msl_ft: Some(f("agl_ft") + 500.0),
                            heading_true_deg: f("heading_true_deg"),
                            groundspeed_kt: f("groundspeed_kt"),
                            indicated_airspeed_kt: f("indicated_airspeed_kt"),
                            true_airspeed_kt: f("indicated_airspeed_kt"),
                            lat: f64v("lat"),
                            lon: f64v("lon"),
                            pitch_deg: f("pitch_deg"),
                            bank_deg: f("bank_deg"),
                            gear_normal_force_n: sv
                                .get("gear_normal_force_n")
                                .and_then(|x| x.as_f64())
                                .map(|x| x as f32),
                            total_weight_kg: sv
                                .get("total_weight_kg")
                                .and_then(|x| x.as_f64())
                                .map(|x| x as f32),
                        });
                    }
                }
                "landing_analysis" => recorded = v.get("analysis").cloned(),
                _ => {}
            }
        }
        let edge_at = edge_at.expect("edge_at");
        let recorded = recorded.expect("recorded analysis");

        // The recorded (production, pre-v0.16.22 LAGGED) verdict.
        assert_eq!(
            recorded.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false),
            "recorded lagged analysis said no flare"
        );

        // Re-run with the v0.16.22 MSFS AGL path.
        let a = compute_landing_analysis(&samples, edge_at, Simulator::Msfs2024, None);

        let flare_detected = a.get("flare_detected").and_then(|v| v.as_bool()).unwrap();
        let reduction = a
            .get("flare_reduction_fpm")
            .and_then(|v| v.as_f64())
            .unwrap();
        assert!(
            flare_detected,
            "JBU323 flare must flip to TRUE on AGL geometry"
        );
        assert!(
            (100.0..=260.0).contains(&reduction),
            "JBU323 AGL flare reduction should land ~100-200 fpm, got {reduction}"
        );
        assert_eq!(
            a.get("flare_vs_source").and_then(|v| v.as_str()),
            Some("msfs_agl")
        );

        // v1.6.3: Die Aufsetzrate kommt ab jetzt aus der Höhenkurve. JBU323
        // ist der Beleg, dass das bei einem STEILEN Anflug nichts kaputt
        // macht: aufgezeichnet −423,5 fpm aus dem SimVar-Kanal, gemessen
        // −422,2 aus der Ausgleichsgeraden — 1,3 fpm auseinander. Wo nichts
        // zu dämpfen ist, fallen beide Quellen zusammen; die Umstellung
        // wirkt nur dort, wo der Kanal wirklich hinterherhinkt.
        //
        // Die QS zu diesem Release hat hier einen eigenen Fehler gefunden:
        // eine Zwischenfassung rechnete die Kurve als Differenz zweier
        // Einzelwerte und las dadurch −463,9 — 40 fpm härter, allein aus
        // dem Aufsetz-Frame. Der Test forderte daraufhin „muss härter
        // ausfallen" und hätte die Korrektur zurück auf die Ausgleichs-
        // gerade blockiert. Die belastbare Zusage ist NICHT „härter",
        // sondern „bei steilem Anflug deckungsgleich".
        let edge_recorded = recorded
            .get("vs_at_edge_fpm")
            .and_then(|v| v.as_f64())
            .unwrap();
        let edge_new = a.get("vs_at_edge_fpm").and_then(|v| v.as_f64()).unwrap();
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("hoehenkurve"),
            "die Aufsetzrate muss aus der Höhenkurve kommen"
        );
        assert!(
            (edge_new - edge_recorded).abs() <= 25.0,
            "steiler Anflug: beide Quellen müssen zusammenfallen, aufgezeichnet \
             {edge_recorded}, neu {edge_new}"
        );

        // The lagged SimVar values are preserved as forensic provenance and
        // match the recorded (lagged) endpoints.
        let raw_peak = a
            .get("peak_vs_pre_flare_fpm_raw")
            .and_then(|v| v.as_f64())
            .unwrap();
        let rec_peak = recorded
            .get("peak_vs_pre_flare_fpm")
            .and_then(|v| v.as_f64())
            .unwrap();
        assert!(
            (raw_peak - rec_peak).abs() < 1.0,
            "raw peak must equal recorded lagged peak"
        );
    }

    /// Die Riegel der Höhenkurve, jeder für sich festgenagelt. Runde 4 der
    /// QS fand sie nur EINSEITIG belegt: sie schärfer zu stellen machte
    /// Tests rot, sie abzuschalten nicht. Ein Riegel, den man folgenlos
    /// entfernen kann, ist kein Riegel.
    /// v1.6.9 — die Sinkrate in ihre Bestandteile zerlegt.
    ///
    /// Neapel 24 aus dem Bestand ist der Anschauungsfall: gemessene
    /// −438 fpm, davon +387 fpm Gelaende. Der Pilot ist mit −825 fpm
    /// gesunken, die ansteigende Bahn hat die Zahl geschoent. Hier
    /// nachgebaut mit einer Bahn, die 300 fpm steigt.
    #[test]
    fn sinkrate_zerfaellt_in_eigensinken_und_gelaende() {
        let b = Utc::now();
        // Flugzeug sinkt mit 600 fpm gegen den Meeresspiegel (= 10 ft/s),
        // der Boden steigt mit 300 fpm (= 5 ft/s). Die Hoehe ueber Grund
        // schrumpft dadurch nur mit 300 fpm.
        let samples: Vec<TouchdownWindowSample> = (0..12)
            .map(|i| {
                let t = i as f32 * 0.025; // 25 ms Takt = 40 Hz
                let msl = 1000.0 - 10.0 * t;
                let boden = 900.0 + 5.0 * t;
                tw_msl(b, (t * 1000.0) as i64, msl - boden, msl, 2.0, false, -300.0)
            })
            .collect();
        let edge = b + chrono::Duration::milliseconds(275);
        let a = compute_landing_analysis(&samples, edge, Simulator::Msfs2024, Some(-455.0));
        let hole = |k: &str| a.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);

        // Hoehe ueber Grund = (1000 − 10t) − (900 + 5t) = 100 − 15t,
        // schrumpft also mit 15 ft/s = 900 fpm.
        let gemessen = hole("vs_at_edge_fpm").expect("Hoehenkurve muss tragen");
        assert!(
            (gemessen + 900.0).abs() < 20.0,
            "die gemessene Rate folgt der Hoehe ueber Grund: {gemessen}"
        );
        let eigen = hole("vs_eigensinken_fpm").expect("Eigensinken");
        assert!(
            (eigen + 600.0).abs() < 20.0,
            "gegen den Meeresspiegel sinkt das Flugzeug nur mit 600 fpm: {eigen}"
        );
        let gelaende = hole("vs_gelaende_fpm").expect("Gelaendeanteil");
        assert!(
            (gelaende + 300.0).abs() < 20.0,
            "der steigende Boden macht die Zahl um 300 fpm haerter, \
             der Beitrag ist also NEGATIV: {gelaende}"
        );
        // Die Probe, an der ein falsches Vorzeichen sofort auffaellt.
        assert!(
            (gemessen - (eigen + gelaende)).abs() < 5.0,
            "die Aufschluesselung muss aufgehen: {gemessen} != {eigen} + {gelaende}"
        );
        assert_eq!(hole("vs_sim_referenz_fpm"), Some(-455.0));
    }

    /// v1.6.10 — die Referenz des Simulators darf nie die der VORIGEN
    /// Landung sein.
    ///
    /// `PLANE TOUCHDOWN NORMAL VELOCITY` ist ein Latch: nach einem
    /// Touch-and-Go steht dort der alte Wert, bis der Simulator ihn
    /// ueberschreibt — gemessen ueber 658 MSFS-Landungen im Median erst
    /// 0,29 s nach dem Aufsetzen, in 10 % der Faelle erst nach 3 s. Bei
    /// 8 % der Landungen stand vor dem Aufsetzen bereits ein Wert.
    ///
    /// Geprueft wird hier die reine Entscheidungsregel: uebernommen wird
    /// nur, was sich gegenueber dem Stand vor dem Aufsetzen geaendert hat.
    #[test]
    fn sim_referenz_nimmt_nur_frische_werte() {
        // Die Regel aus dem Sampler, isoliert nachgebaut.
        fn uebernehmen(vorher: Option<f32>, kandidat: Option<f32>) -> Option<f32> {
            let k = kandidat?;
            match vorher {
                Some(v) if (k - v).abs() <= 0.5 => None,
                _ => Some(k),
            }
        }
        // Erste Landung des Fluges: nichts stand vorher, alles gut.
        assert_eq!(uebernehmen(None, Some(-234.0)), Some(-234.0));
        // Nach einem Touch-and-Go steht der alte Wert — und der
        // Simulator hat noch nicht nachgezogen.
        assert_eq!(uebernehmen(Some(-234.0), Some(-234.0)), None);
        // Sobald er nachzieht, wird der neue Wert genommen.
        assert_eq!(uebernehmen(Some(-234.0), Some(-98.0)), Some(-98.0));
        // Rauschen im letzten halben fpm gilt nicht als Aenderung.
        assert_eq!(uebernehmen(Some(-234.0), Some(-234.4)), None);
        assert_eq!(uebernehmen(Some(-234.0), Some(-235.0)), Some(-235.0));
        // Ohne Kandidat bleibt es leer.
        assert_eq!(uebernehmen(Some(-234.0), None), None);
    }

    /// Inventur B5/A (02.10.2026): Ebenes Gelaende steht legitim still. Dort
    /// fiel die ganze Aufschluesselung weg (10 von 60 Landungen), obwohl AGL
    /// und MSL einwandfrei sanken. Jetzt: Gelaendeanteil 0, Eigensinken =
    /// gemessene Rate.
    #[test]
    fn ebenes_gelaende_liefert_die_aufschluesselung_mit_gelaende_null() {
        let b = Utc::now();
        let samples: Vec<TouchdownWindowSample> = (0..12)
            .map(|i| {
                let t = i as f32 * 0.025;
                let msl = 1000.0 - 5.0 * t;
                tw_msl(b, (t * 1000.0) as i64, msl - 900.0, msl, 2.0, false, -300.0)
            })
            .collect();
        let edge = b + chrono::Duration::milliseconds(275);
        let a = compute_landing_analysis(&samples, edge, Simulator::Msfs2024, None);
        let hole = |k: &str| a.get(k).and_then(|v| v.as_f64()).map(|v| v as f32);
        let gemessen = hole("vs_at_edge_fpm").expect("Hoehenkurve");
        let gelaende = hole("vs_gelaende_fpm").expect("ebenes Gelaende hat einen Anteil: 0");
        let eigen = hole("vs_eigensinken_fpm").expect("Eigensinken");
        assert_eq!(gelaende, 0.0, "Boden steht, Beitrag ist null (nicht −0)");
        assert!(gelaende.is_sign_positive());
        assert!((eigen - gemessen).abs() < 5.0, "{eigen} gegen {gemessen}");
    }

    /// QS gegen das Werkzeug von Arderos: es verlangt fuenf
    /// UNTERSCHIEDLICHE Zeitstempel, bevor es eine Ausgleichsrechnung
    /// zulaesst. Meine erste Fassung zaehlte nur Messpunkte — eine
    /// eingefrorene Hoehenspur haette dann „kein Eigensinken, alles
    /// Gelaende" gemeldet, also die Bewegung dem falschen Anteil
    /// zugeschoben.
    #[test]
    fn eingefrorene_hoehenspur_liefert_keine_aufschluesselung() {
        let b = Utc::now();
        // AGL sinkt sauber, aber MSL steht ueber 200 ms still (der Boden
        // „faellt" dadurch scheinbar genauso schnell wie die Hoehe).
        let samples: Vec<TouchdownWindowSample> = (0..12)
            .map(|i| {
                let t = i as f32 * 0.025;
                let agl = 100.0 - 10.0 * t;
                // MSL friert nach dem vierten Messpunkt ein.
                let msl = if i < 4 {
                    1000.0 - 10.0 * t
                } else {
                    1000.0 - 10.0 * 0.075
                };
                tw_msl(b, (t * 1000.0) as i64, agl, msl, 2.0, false, -600.0)
            })
            .collect();
        let edge = b + chrono::Duration::milliseconds(275);
        let a = compute_landing_analysis(&samples, edge, Simulator::Msfs2024, None);
        assert!(
            a.get("vs_at_edge_fpm").and_then(|v| v.as_f64()).is_some(),
            "die Hoehenkurve selbst traegt weiterhin"
        );
        assert!(
            a.get("vs_eigensinken_fpm")
                .map(|v| v.is_null())
                .unwrap_or(true),
            "bei eingefrorener Hoehe darf keine Aufschluesselung erscheinen"
        );
        assert!(a
            .get("vs_gelaende_fpm")
            .map(|v| v.is_null())
            .unwrap_or(true));
    }

    /// Die Probe ist der eigentliche Riegel: was nicht aufgeht, wird nicht
    /// gezeigt. Hier springt die Bodenhoehe mitten im Fenster (Mesh-Kante,
    /// Helipad-Dach) — die drei Groessen passen dann nicht mehr zusammen.
    #[test]
    fn gelaendesprung_faellt_durch_die_probe() {
        let b = Utc::now();
        let samples: Vec<TouchdownWindowSample> = (0..12)
            .map(|i| {
                let t = i as f32 * 0.025;
                let msl = 1000.0 - 10.0 * t;
                // Boden springt nach der Haelfte um 8 ft.
                let boden = if i < 6 { 900.0 } else { 908.0 };
                tw_msl(b, (t * 1000.0) as i64, msl - boden, msl, 2.0, false, -600.0)
            })
            .collect();
        let edge = b + chrono::Duration::milliseconds(275);
        let a = compute_landing_analysis(&samples, edge, Simulator::Msfs2024, None);
        // Gegenprobe: ohne den Sprung geht dieselbe Konstellation durch.
        let sauber: Vec<TouchdownWindowSample> = (0..12)
            .map(|i| {
                let t = i as f32 * 0.025;
                let msl = 1000.0 - 10.0 * t;
                let boden = 900.0 + 2.0 * t;
                tw_msl(b, (t * 1000.0) as i64, msl - boden, msl, 2.0, false, -600.0)
            })
            .collect();
        let a2 = compute_landing_analysis(&sauber, edge, Simulator::Msfs2024, None);
        assert!(
            a2.get("vs_gelaende_fpm").and_then(|v| v.as_f64()).is_some(),
            "die saubere Gegenprobe muss eine Aufschluesselung liefern"
        );
        assert!(
            a.get("vs_gelaende_fpm")
                .map(|v| v.is_null())
                .unwrap_or(true),
            "ein Sprung im Bodenmodell darf nicht als Gelaendeanteil verkauft werden"
        );
    }

    /// Ohne Hoehe ueber dem Meeresspiegel (Aufzeichnungen vor v1.6.9)
    /// bleibt die Aufschluesselung leer — statt eine zu erfinden.
    #[test]
    fn ohne_msl_keine_aufschluesselung() {
        let b = Utc::now();
        let samples: Vec<TouchdownWindowSample> = (0..12)
            .map(|i| {
                let t = i as f32 * 0.025;
                let mut s = tw_msl(
                    b,
                    (t * 1000.0) as i64,
                    100.0 - 5.0 * t,
                    0.0,
                    2.0,
                    false,
                    -300.0,
                );
                s.msl_ft = None;
                s
            })
            .collect();
        let edge = b + chrono::Duration::milliseconds(275);
        let a = compute_landing_analysis(&samples, edge, Simulator::XPlane12, None);
        assert!(a.get("vs_at_edge_fpm").and_then(|v| v.as_f64()).is_some());
        assert!(a
            .get("vs_gelaende_fpm")
            .map(|v| v.is_null())
            .unwrap_or(true));
        assert!(a
            .get("vs_eigensinken_fpm")
            .map(|v| v.is_null())
            .unwrap_or(true));
    }

    #[test]
    fn die_riegel_der_hoehenkurve_greifen_einzeln() {
        let b = base();

        // Zu wenige Messpunkte: drei reichen nicht für eine Gerade.
        let wenige = vec![
            tw(b, -300, 14.0, 2.0, false, -200.0),
            tw(b, -160, 12.0, 2.0, false, -200.0),
            tw(b, -20, 10.0, 2.0, false, -200.0),
            tw(b, 0, 0.0, 2.0, true, -200.0),
            tw(b, 20, 0.0, 2.0, true, -190.0),
        ];
        let a = compute_landing_analysis(&wenige, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("simvar_fallback"),
            "drei Messpunkte dürfen keine Ausgleichsgerade tragen"
        );

        // Zu wenige VERSCHIEDENE Werte: zwei Stufen sind keine Kurve.
        let zwei_stufen = vec![
            tw(b, -300, 12.0, 2.0, false, -200.0),
            tw(b, -220, 12.0, 2.0, false, -200.0),
            tw(b, -140, 11.0, 2.0, false, -200.0),
            tw(b, -60, 11.0, 2.0, false, -200.0),
            tw(b, 0, 0.0, 2.0, true, -200.0),
            tw(b, 20, 0.0, 2.0, true, -190.0),
        ];
        let a = compute_landing_analysis(&zwei_stufen, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("simvar_fallback"),
            "zwei verschiedene Höhenwerte dürfen keine Ausgleichsgerade tragen"
        );

        // Stillstand exakt an der Grenze (120 ms) ist noch erlaubt,
        // knapp darüber nicht mehr. Beide Seiten der Kante geprüft —
        // sonst bliebe ein Verschieben der Konstante folgenlos.
        let mit_pause = |pause_ms: i64| {
            vec![
                tw(b, -300, 15.0, 2.0, false, -200.0),
                tw(b, -300 + pause_ms, 15.0, 2.0, false, -200.0),
                tw(b, -300 + pause_ms + 40, 13.0, 2.0, false, -200.0),
                tw(b, -300 + pause_ms + 80, 11.0, 2.0, false, -200.0),
                tw(b, -60, 9.0, 2.0, false, -200.0),
                tw(b, -20, 8.0, 2.0, false, -200.0),
                tw(b, 0, 0.0, 2.0, true, -200.0),
                tw(b, 20, 0.0, 2.0, true, -190.0),
            ]
        };
        let a = compute_landing_analysis(&mit_pause(120), b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("hoehenkurve"),
            "120 ms Stillstand liegen noch im erlaubten Bereich"
        );
        let a = compute_landing_analysis(&mit_pause(121), b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("simvar_fallback"),
            "121 ms Stillstand müssen die Ausgleichsgerade verwerfen"
        );
    }

    /// Doppelte Zeitstempel sind ein Aufzeichnungs-Artefakt, kein
    /// Stillstand. X-Plane liefert vereinzelt jeden Messpunkt mehrfach mit
    /// derselben Uhrzeit — eine Zaehlung gleicher Werte haette das als
    /// Stillstand gelesen und ausgerechnet die dichtesten Aufzeichnungen
    /// verworfen. Der Test sichert, dass der Riegel in ZEIT rechnet und
    /// nicht wieder auf Messpunkte zurueckgebaut wird.
    #[test]
    fn doppelte_zeitstempel_gelten_nicht_als_stillstand() {
        let b = base();
        let mut s = Vec::new();
        for i in 0..8 {
            let t = -300 + i * 40;
            let agl = 15.0 - i as f32 * 1.5;
            s.push(tw(b, t, agl, 2.0, false, -200.0));
            // derselbe Zeitstempel, derselbe Wert — reines Artefakt
            s.push(tw(b, t, agl, 2.0, false, -200.0));
        }
        s.push(tw(b, 0, 0.0, 2.0, true, -200.0));
        s.push(tw(b, 20, 0.0, 2.0, true, -190.0));
        let a = compute_landing_analysis(&s, b, Simulator::XPlane12, None);
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("hoehenkurve"),
            "verdoppelte Messpunkte dürfen die Kurve nicht verwerfen"
        );
    }

    /// **Die verworfene Höhenkurve bleibt der Unfall-Erkennung erhalten.**
    /// Der Score verwirft sie bei eingefrorener Höhenspur, weil sie dort zu
    /// weich liest. Die Unfall-Erkennung nimmt aber den HÄRTESTEN aller
    /// Messwerte — ihr kann ein zusätzlicher Kandidat nicht schaden, ein
    /// fehlender harter dagegen schon. Ohne diesen Test ließ sich der
    /// Kandidat ersatzlos aus der Liste streichen, ohne dass etwas rot
    /// wurde (QS-Befund Runde 5) — und genau mit diesem Maßstab hat
    /// dieselbe Release-Arbeit an anderer Stelle Code weggeworfen.
    #[test]
    fn verworfene_hoehenkurve_bleibt_der_unfallerkennung_erhalten() {
        let (edge_at, samples, _r) = load_flare_fixture("a320_vrnevnl_frozen_end.jsonl.gz");
        let a = compute_landing_analysis(&samples, edge_at, Simulator::Msfs2024, None);
        assert!(
            a.get("vs_geometrie_fpm").and_then(|v| v.as_f64()).is_none(),
            "Vorbedingung: der Score muss die Kurve hier verwerfen"
        );
        let ungefiltert = a
            .get("vs_geometrie_ungefiltert_fpm")
            .and_then(|v| v.as_f64())
            .expect("die verworfene Kurve muss der Unfall-Erkennung erhalten bleiben");
        assert!(
            ungefiltert < 0.0 && ungefiltert.is_finite(),
            "unbrauchbarer Wert: {ungefiltert}"
        );
    }

    /// **Eine eingefrorene Hoehenspur wird nicht gescort.**
    ///
    /// Diese Aufzeichnung ist der Anlass fuer `AGL_MAX_STILLSTAND_MS`: die
    /// Hoehe steht ueber 19 Messpunkte auf demselben Wert, dann holt der
    /// Simulator −1,49 ft in einem einzigen Schritt nach. Die Ausgleichs-
    /// gerade mittelt das Plateau mit ein und las −45 statt −162 fpm — der
    /// Pilot haette 15 Punkte fuer einen langen Abfangbogen verloren, den er
    /// nie geflogen ist.
    ///
    /// Der Test haelt fest, dass hier auf den Simulator-Kanal
    /// zurueckgefallen wird. **Er stand kurzzeitig andersherum im Baum** und
    /// forderte die falschen −45 fpm als Sollwert: die eine Pruefrunde hatte
    /// den Wert als richtig angenommen, die andere ihn an den Daten
    /// widerlegt (ab fuenf Wiederholungen liest die Kurve in 89 % zu weich,
    /// bei normalen Verlaeufen in 47 %). Ein Test kann einen Fehler auch
    /// festschreiben.
    #[test]
    fn eingefrorene_hoehenspur_faellt_auf_den_simvar_zurueck() {
        let (edge_at, samples, _r) = load_flare_fixture("a320_vrnevnl_frozen_end.jsonl.gz");
        assert!(
            samples.iter().any(|s| s.on_ground) && samples.iter().any(|s| !s.on_ground),
            "Fixture taugt nur als Beleg, wenn beide Sorten vorkommen"
        );
        let a = compute_landing_analysis(&samples, edge_at, Simulator::Msfs2024, None);
        assert!(
            a.get("vs_geometrie_fpm").and_then(|v| v.as_f64()).is_none(),
            "eingefrorene Hoehenspur darf keine Ausgleichsgerade liefern, war {:?}",
            a.get("vs_geometrie_fpm")
        );
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("simvar_fallback"),
            "hier muss der Simulator-Kanal die Bewertung tragen"
        );
        let gescort = a.get("vs_at_edge_fpm").and_then(|v| v.as_f64()).unwrap();
        assert!(
            (gescort - -162.0).abs() < 40.0,
            "erwartet die echten rund −162 fpm, gescort wurde {gescort}"
        );
    }

    /// **Die Ausgleichsgerade, an drei echten Fluegen festgenagelt.** Alle
    /// uebrigen Pruefungen der Sinkrate sind relativ (Quelle A gegen Quelle
    /// B) oder haben 60-fpm-Baender — die Endpunkt-Differenz liegt auf
    /// diesen Aufzeichnungen 15–19 fpm daneben und rutschte durch jede
    /// davon. Enges Band, absolute Werte: das ist die Pruefung, die den
    /// Rueckbau auf zwei Messpunkte bemerkt.
    #[test]
    fn qs_m2_ausgleichsgerade_statt_endpunkten() {
        for (datei, wahrheit) in [
            ("dlh848_msfs_firm_control.jsonl.gz", -377.5_f32),
            ("jbu323_msfs_late_flare.jsonl.gz", -422.7_f32),
            ("dlh304.jsonl.gz", -337.8_f32),
        ] {
            let (edge_at, samples, _r) = load_flare_fixture(datei);
            let a = compute_landing_analysis(&samples, edge_at, Simulator::Msfs2024, None);
            let geo = a.get("vs_geometrie_fpm").and_then(|v| v.as_f64()).unwrap() as f32;
            assert!(
                (geo - wahrheit).abs() < 8.0,
                "{datei}: Ausgleichsgerade erwartet {wahrheit} +-8 fpm, gemessen {geo}"
            );
        }
    }

    /// **Zu kurze Zeitbasis heisst: Finger weg.** Kein Fixture reizt diese
    /// Grenze (alle liegen bei 0,20–0,28 s), sie ist also nur synthetisch
    /// pruefbar — und ohne Pruefung liess sich `AGL_MIN_SPANNE_S` auf null
    /// setzen, ohne dass etwas rot wurde.
    #[test]
    fn qs_m3_zu_kurze_zeitbasis_faellt_auf_simvar_zurueck() {
        let b = base();
        // Nur 4 Messpunkte in 30 ms; die AGL-Quantisierung (0,25-ft-Stufen)
        // taeuscht darin rund 1000 fpm vor, echt sind 150.
        let s = vec![
            tw(b, -40, 12.50, 2.0, false, -150.0),
            tw(b, -30, 12.25, 2.0, false, -150.0),
            tw(b, -20, 12.00, 2.0, false, -150.0),
            tw(b, -10, 12.00, 2.0, false, -150.0),
            tw(b, 0, 0.0, 2.0, true, -150.0),
            tw(b, 20, 0.0, 2.0, true, -140.0),
        ];
        let a = compute_landing_analysis(&s, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("simvar_fallback"),
            "30 ms Zeitbasis duerfen keine Ausgleichsgerade tragen"
        );
        let scored = a.get("vs_at_edge_fpm").and_then(|v| v.as_f64()).unwrap();
        assert!(
            (scored - -150.0).abs() < 30.0,
            "der Rueckfall muss die echten -150 fpm liefern, war {scored}"
        );
    }

    /// **Ein Sprung im Hoehenbezug ist keine Landerate.** Gelaendekante,
    /// Helipad-Dach, Bodenmodell-Wechsel: 25 ft in 250 ms sind rechnerisch
    /// 6000 fpm. Auch diese Grenze reizt kein Fixture, und auch sie liess
    /// sich folgenlos aushebeln.
    #[test]
    fn qs_m4_hoehensprung_ist_keine_landerate() {
        let b = base();
        // Gelaendekante: 25 ft AGL-Sprung in 250 ms = rechnerisch 6000 fpm.
        let mut s: Vec<_> = (0..13)
            .map(|i| tw(b, -250 + i * 20, 30.0 - i as f32 * 2.1, 2.0, false, -200.0))
            .collect();
        s.push(tw(b, 0, 2.0, 2.0, true, -200.0));
        s.push(tw(b, 20, 1.0, 2.0, true, -180.0));
        let a = compute_landing_analysis(&s, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("vs_at_edge_quelle").and_then(|v| v.as_str()),
            Some("simvar_fallback"),
            "ein Sprung im Hoehenbezug darf nicht als Sinkrate gescort werden"
        );
        let scored = a.get("vs_at_edge_fpm").and_then(|v| v.as_f64()).unwrap();
        assert!(
            scored > -400.0,
            "gescort wurde {scored} fpm — der Sprung ist durchgerutscht"
        );
    }

    // ─── v0.16.22 hardening: phantom-flare guard tests ────────────────────

    /// Load a trimmed `.jsonl.gz` fixture (touchdown_window + landing_
    /// analysis) into `(edge_at, samples, recorded_analysis)`. Reads the
    /// FIRST touchdown_window / landing_analysis (the fixtures are single-
    /// window). Shared by the real-phantom golden tests below.
    fn load_flare_fixture(
        fixture: &str,
    ) -> (DateTime<Utc>, Vec<TouchdownWindowSample>, serde_json::Value) {
        use flate2::read::GzDecoder;
        use std::io::{BufRead, BufReader};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let file = std::fs::File::open(&path).unwrap_or_else(|_| panic!("open fixture {fixture}"));
        let reader = BufReader::new(GzDecoder::new(file));
        let mut edge_at: Option<DateTime<Utc>> = None;
        let mut samples: Vec<TouchdownWindowSample> = Vec::new();
        let mut recorded: Option<serde_json::Value> = None;
        for line in reader.lines() {
            let line = line.unwrap();
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                "touchdown_window" if edge_at.is_none() => {
                    edge_at = v
                        .get("edge_at")
                        .and_then(|x| x.as_str())
                        .map(|s| DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc));
                    for sv in v.get("samples").and_then(|x| x.as_array()).unwrap() {
                        let f = |k: &str| sv.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                        let f64v = |k: &str| sv.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
                        samples.push(TouchdownWindowSample {
                            at: DateTime::parse_from_rfc3339(sv["at"].as_str().unwrap())
                                .unwrap()
                                .with_timezone(&Utc),
                            vs_fpm: f("vs_fpm"),
                            g_force: f("g_force"),
                            g_semibody: None,
                            on_ground: sv
                                .get("on_ground")
                                .and_then(|x| x.as_bool())
                                .unwrap_or(false),
                            agl_ft: f("agl_ft"),
                            msl_ft: Some(f("agl_ft") + 500.0),
                            heading_true_deg: f("heading_true_deg"),
                            groundspeed_kt: f("groundspeed_kt"),
                            indicated_airspeed_kt: f("indicated_airspeed_kt"),
                            true_airspeed_kt: f("indicated_airspeed_kt"),
                            lat: f64v("lat"),
                            lon: f64v("lon"),
                            pitch_deg: f("pitch_deg"),
                            bank_deg: f("bank_deg"),
                            gear_normal_force_n: sv
                                .get("gear_normal_force_n")
                                .and_then(|x| x.as_f64())
                                .map(|x| x as f32),
                            total_weight_kg: sv
                                .get("total_weight_kg")
                                .and_then(|x| x.as_f64())
                                .map(|x| x as f32),
                        });
                    }
                }
                "landing_analysis" if recorded.is_none() => {
                    recorded = v.get("analysis").cloned();
                }
                _ => {}
            }
        }
        (
            edge_at.expect("edge_at"),
            samples,
            recorded.expect("recorded"),
        )
    }

    /// Gate 1 (data quality): a synthetic FROZEN-AGL trace (the AGL value
    /// stuck at one number through the flare-end) must mark the AGL path
    /// unreliable so the caller falls back to SimVar.
    #[test]
    fn gate_frozen_agl_marks_unreliable() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // 50 Hz samples but agl_ft frozen at 10.0 the whole window → only 1
        // distinct value (< MSFS_FLARE_MIN_DISTINCT_AGL).
        let s: Vec<_> = (0..100)
            .map(|i| tw(b, -2000 + i * 20, 10.0, 3.0, false, -300.0))
            .collect();
        let g = evaluate_msfs_flare_gates(&s, edge_ms - 2000, edge_ms - 100);
        assert!(!g.agl_reliable, "frozen AGL must be unreliable");
        assert!(g.unreliable_source, "frozen AGL is a data-quality reject");
    }

    /// Gate 1b (data quality): a degraded sample rate (median Δt > 60 ms,
    /// i.e. < ~16 Hz) marks the AGL path unreliable even with many distinct
    /// AGL values.
    #[test]
    fn gate_low_rate_marks_unreliable() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // ~13 Hz (75 ms spacing), distinct descending AGL.
        let s: Vec<_> = (0..26)
            .map(|i| tw(b, -2000 + i * 75, 20.0 - i as f32 * 0.4, 3.0, false, -300.0))
            .collect();
        let g = evaluate_msfs_flare_gates(&s, edge_ms - 2000, edge_ms - 100);
        assert!(!g.agl_reliable, "degraded rate must be unreliable");
        assert!(
            g.unreliable_source,
            "degraded rate is a data-quality reject"
        );
    }

    /// Gate 2 (bounce): an on_ground=true sample inside the window
    /// suppresses the AGL path (a post-touchdown settle, not a flare). The
    /// fallback is the PLAIN simvar source (not "unreliable").
    #[test]
    fn gate_on_ground_in_window_suppresses() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        let mut s: Vec<_> = (0..100)
            .map(|i| {
                tw(
                    b,
                    -2000 + i * 20,
                    20.0 - i as f32 * 0.15,
                    3.0,
                    false,
                    -300.0,
                )
            })
            .collect();
        // Force an on_ground touch mid-window (a bounce).
        s[30].on_ground = true;
        let g = evaluate_msfs_flare_gates(&s, edge_ms - 2000, edge_ms - 100);
        assert!(!g.agl_reliable, "on_ground in window = bounce → suppress");
        assert!(!g.unreliable_source, "bounce is NOT a data-quality reject");
    }

    /// Gate 2 (balloon): a non-monotone AGL rise beyond the tolerance
    /// (without an on_ground sample) is also a bounce/balloon → suppress.
    #[test]
    fn gate_balloon_suppresses() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // Descend, then balloon up ~3 ft, then settle — all airborne.
        let agl = |i: i64| -> f32 {
            let t = i as f32;
            if t < 40.0 {
                15.0 - t * 0.2 // 15 → 7
            } else if t < 60.0 {
                7.0 + (t - 40.0) * 0.15 // balloon up to ~10
            } else {
                10.0 - (t - 60.0) * 0.1
            }
        };
        let s: Vec<_> = (0..100)
            .map(|i| tw(b, -2000 + i * 20, agl(i), 3.0, false, -300.0))
            .collect();
        let g = evaluate_msfs_flare_gates(&s, edge_ms - 2000, edge_ms - 100);
        assert!(!g.agl_reliable, "balloon must suppress the AGL path");
    }

    /// Gate 1c (LOCAL data quality): the whole-window trace is data-rich
    /// (well above the distinct floor) but the AGL FREEZES in the final
    /// ~360 ms — the flare-end fit reads a near-zero slope from the plateau
    /// and INFLATES the reduction. Gate 1a's whole-window count misses this
    /// (the freeze is localized); Gate 1c's end-window distinct count
    /// catches it → unreliable → SimVar fallback. (This is the vrNEVnl
    /// archetype: 25 distinct over the window, frozen only at the end.)
    #[test]
    fn gate_localized_frozen_end_marks_unreliable() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // Genuine descent for the first ~1.5 s (many distinct AGL values),
        // then AGL frozen at 6.0 ft through the flare-end fit window.
        let s: Vec<_> = (0..100)
            .map(|i| {
                let ms = -2000 + i * 20;
                let agl = if ms < -500 {
                    // descend 18 → 6 over [-2000,-500]
                    18.0 - (ms - (-2000)) as f32 / 1500.0 * 12.0
                } else {
                    6.0 // FROZEN for the last 500 ms (overlaps end fit window)
                };
                tw(b, ms, agl, 3.0, false, -300.0)
            })
            .collect();
        let g = evaluate_msfs_flare_gates(&s, edge_ms - 2000, edge_ms - 100);
        assert!(!g.agl_reliable, "frozen flare-END must be unreliable");
        assert!(g.unreliable_source, "frozen end is a data-quality reject");
        // Sanity: the WHOLE-window distinct count is well above Gate 1a's
        // floor, proving Gate 1c (not 1a) is what catches this.
        let distinct = {
            let mut v: Vec<i64> = s
                .iter()
                .filter(|x| {
                    let t = x.at.timestamp_millis();
                    t >= edge_ms - 2000 && t <= edge_ms - 100 && !x.on_ground
                })
                .map(|x| (x.agl_ft as f64 * 100.0).round() as i64)
                .collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        assert!(
            distinct >= crate::grenzwerte::grenzwerte().landung.msfs_flare_min_distinct_agl.wert,
            "whole-window distinct ({distinct}) is above Gate 1a — only Gate 1c catches it"
        );
    }

    /// FIX 1 (Gate 1c span backstop): a NOISY freeze. The AGL descends
    /// genuinely through the first ~1.5 s (many distinct values → Gate 1a
    /// passes), then in the final ~600 ms it DITHERS across ≥4 distinct 0.01
    /// ft buckets (so the Gate 1c bucket-COUNT passes) while the true span is
    /// only ~0.03 ft — a flat plateau. The end-fit slope reads ≈0 →
    /// vs_at_flare_end ≈ 0 → a huge fake reduction → phantom score-100. The
    /// SPAN backstop must catch it where the bucket count cannot.
    #[test]
    fn gate_noisy_freeze_span_backstop_rejects() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // End fit window is [flare_hi-300, flare_hi+300] = [-400, +200] ms.
        // The flare window samples run [-2000, -100]. Build a real descent
        // until -500 ms, then a near-flat noisy plateau through the end.
        // The dither uses 5 distinct 0.01-ft buckets within a ~0.04 ft span.
        let plateau = [6.00_f32, 6.01, 6.02, 6.03, 6.04];
        let s: Vec<_> = (0..96)
            .map(|i| {
                let ms: i64 = -2000 + i * 20; // -2000 .. -100
                let agl = if ms < -500 {
                    // descend 18 → 6 over [-2000,-500]: many distinct values
                    18.0 - (ms - (-2000)) as f32 / 1500.0 * 12.0
                } else {
                    // noisy plateau: cycle the 5 buckets (≥4 distinct, ~0.04 ft span)
                    let idx = (((ms + 500) / 20).rem_euclid(plateau.len() as i64)) as usize;
                    plateau[idx]
                };
                tw(b, ms, agl, 3.0, false, -300.0)
            })
            .collect();
        let g = evaluate_msfs_flare_gates(&s, edge_ms - 2000, edge_ms - 100);
        assert!(
            !g.agl_reliable,
            "noisy multi-bucket freeze (flat span) must be unreliable"
        );
        assert!(g.unreliable_source, "noisy freeze is a data-quality reject");
        // Prove the bucket COUNT alone would NOT have caught it: the end-fit
        // window has ≥ MSFS_FLARE_MIN_END_FIT_DISTINCT_AGL distinct buckets,
        // so only the SPAN backstop rejects here.
        let end_lo = (edge_ms - 100) - crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_half_window_ms.wert;
        let end_hi = (edge_ms - 100) + crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_half_window_ms.wert;
        let mut end_buckets: Vec<i64> = s
            .iter()
            .filter(|x| {
                let t = x.at.timestamp_millis();
                t >= end_lo && t <= end_hi && !x.on_ground
            })
            .map(|x| (x.agl_ft as f64 * 100.0).round() as i64)
            .collect();
        end_buckets.sort_unstable();
        end_buckets.dedup();
        assert!(
            end_buckets.len() >= crate::grenzwerte::grenzwerte().landung.msfs_flare_min_end_fit_distinct_agl.wert,
            "end window has ≥4 distinct buckets ({}) — only the span backstop catches this",
            end_buckets.len()
        );
        // And the end-to-end JSON must NOT publish a phantom: source falls
        // back, no detection, no butter-100.
        let a = compute_landing_analysis(&s, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("flare_vs_source").and_then(|v| v.as_str()),
            Some("simvar_agl_unreliable")
        );
        assert_eq!(
            a.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false)
        );
        let score = a
            .get("flare_quality_score")
            .and_then(|v| v.as_i64())
            .unwrap();
        assert!(
            score < 100,
            "no butter-100 on a noisy-freeze phantom (got {score})"
        );
    }

    /// FIX 2 (post-edge bounce contamination): a clean descent to a touchdown
    /// AT the edge, then a post-edge airborne BALLOON with rising AGL. The
    /// uncapped end-fit would reach edge+200 ms and read those rising
    /// post-edge samples, flattening/reversing the end slope → an inflated
    /// reduction → phantom. The edge-capped fit + the extended balloon gate
    /// must keep this from earning a credited flare.
    #[test]
    fn touchdown_then_balloon_not_credited() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // Steady firm descent -350 fpm to ~0 ft at the edge (t=0), then the
        // aircraft bounces: on_ground at the edge sample, airborne again and
        // ballooning up ~2.5 ft over the next 200 ms.
        let mut s: Vec<TouchdownWindowSample> = Vec::new();
        // Pre-edge descent [-2000, -20] ms, AGL 11.6 → ~0.1, all airborne.
        for i in 0..100 {
            let ms = -2000 + i * 20;
            let agl = 0.1 + (-ms as f32) / 1000.0 * 5.8; // ~-350 fpm geometry
            s.push(tw(b, ms, agl, 4.0, false, -350.0));
        }
        // Edge sample: ground contact at t=0.
        s.push(tw(b, 0, 0.0, 4.0, true, -350.0));
        // Post-edge balloon: airborne again, rising AGL 0.5 → 2.5 ft.
        for i in 1..=10 {
            let ms = i * 20; // +20 .. +200
            let agl = 0.5 + (i as f32) * 0.2; // climbs to 2.5 ft
            s.push(tw(b, ms, agl, 4.0, false, 150.0));
        }
        let flare_lo = edge_ms - 2000;
        let flare_hi = edge_ms - 100;
        // The balloon gate must mark the AGL path unreliable/blocked.
        let g = evaluate_msfs_flare_gates(&s, flare_lo, flare_hi);
        assert!(
            !g.agl_reliable,
            "post-edge balloon must suppress the AGL path (Gate 2 extended)"
        );
        // End-to-end: no credited flare.
        let a = compute_landing_analysis(&s, b, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false),
            "touchdown-then-bounce must NOT produce a credited flare"
        );
    }

    /// Gate 3 (pitch veto): a clean, data-rich descent with a strongly
    /// NOSE-DOWN net pitch trend passes the data/bounce gates but sets the
    /// pitch_nose_down veto. A normal nose-up/flat flare does not.
    #[test]
    fn gate_pitch_nose_down_veto() {
        let b = base();
        let edge_ms = b.timestamp_millis();
        // pitch ramps +3° → −4° across the window (nose-DOWN, like a shed).
        let nose_down: Vec<_> = (0..100)
            .map(|i| {
                let frac = i as f32 / 99.0;
                tw(
                    b,
                    -2000 + i * 20,
                    20.0 - i as f32 * 0.15,
                    3.0 - 7.0 * frac,
                    false,
                    -300.0,
                )
            })
            .collect();
        let g = evaluate_msfs_flare_gates(&nose_down, edge_ms - 2000, edge_ms - 100);
        assert!(
            g.agl_reliable,
            "data is clean — only the pitch veto applies"
        );
        assert!(
            g.pitch_nose_down,
            "strong nose-down trend must set the veto"
        );
        // A held/nose-up flare must NOT trip the veto.
        let nose_up: Vec<_> = (0..100)
            .map(|i| {
                let frac = i as f32 / 99.0;
                tw(
                    b,
                    -2000 + i * 20,
                    20.0 - i as f32 * 0.15,
                    3.0 + 2.0 * frac,
                    false,
                    -300.0,
                )
            })
            .collect();
        let g2 = evaluate_msfs_flare_gates(&nose_up, edge_ms - 2000, edge_ms - 100);
        assert!(!g2.pitch_nose_down, "nose-up flare must not trip the veto");
    }

    /// E195 MXY322 real fixture (frozen AGL). The BARE AGL path would have
    /// produced a phantom "butter" (reduction ~742, score 100); the
    /// data-quality gate must fall back to SimVar — source
    /// "simvar_agl_unreliable", NO 100-score butter, flare_detected=false.
    #[test]
    fn e195_frozen_agl_phantom_suppressed() {
        let (edge_at, samples, recorded) = load_flare_fixture("e195_mxy322_frozen_agl.jsonl.gz");
        // Recorded (SimVar) verdict was already no-flare.
        assert_eq!(
            recorded.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false)
        );
        let a = compute_landing_analysis(&samples, edge_at, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("flare_vs_source").and_then(|v| v.as_str()),
            Some("simvar_agl_unreliable"),
            "frozen E195 must fall back to the SimVar path"
        );
        assert_eq!(
            a.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false),
            "no phantom flare on frozen data"
        );
        // The published reduction is the SimVar (lagged) value, NOT the
        // bare-AGL phantom 742. And the score is not a butter 100.
        let red = a
            .get("flare_reduction_fpm")
            .and_then(|v| v.as_f64())
            .unwrap();
        assert!(
            red < 200.0,
            "frozen phantom reduction must NOT publish (got {red})"
        );
        let score = a
            .get("flare_quality_score")
            .and_then(|v| v.as_i64())
            .unwrap();
        assert!(
            score < 100,
            "no butter-100 on a firm frozen-data arrival (got {score})"
        );
    }

    /// vrNEVnl A320 real fixture (LOCALIZED frozen end). 25 distinct AGL
    /// values over the window (passes Gate 1a), but the trace froze in the
    /// final ~360 ms so the bare AGL path would publish an INFLATED
    /// reduction of ~168 fpm (end read −24 from the plateau instead of the
    /// true −170). Gate 1c must fall back to "simvar_agl_unreliable" with
    /// no detection — the honest SimVar reduction here is ~8 fpm.
    #[test]
    fn vrnevnl_localized_frozen_end_phantom_suppressed() {
        let (edge_at, samples, _recorded) = load_flare_fixture("a320_vrnevnl_frozen_end.jsonl.gz");
        let a = compute_landing_analysis(&samples, edge_at, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("flare_vs_source").and_then(|v| v.as_str()),
            Some("simvar_agl_unreliable"),
            "localized frozen end must fall back (Gate 1c)"
        );
        assert_eq!(
            a.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false),
            "frozen-end inflated reduction must NOT be detected"
        );
        // The published reduction is the honest SimVar value (~8 fpm), not
        // the bare-AGL phantom ~168.
        let red = a
            .get("flare_reduction_fpm")
            .and_then(|v| v.as_f64())
            .unwrap();
        assert!(
            red < 100.0,
            "frozen-end phantom reduction (~168) must NOT publish (got {red})"
        );
    }

    /// DA40 GSG2056 real fixture (bounce/balloon). The aircraft touched
    /// (on_ground=true) and ballooned inside the window; the bounce gate
    /// must suppress detection (post-bounce settle is not a flare). Falls
    /// back to the PLAIN simvar source (the data itself is fine).
    #[test]
    fn da40_bounce_phantom_suppressed() {
        let (edge_at, samples, _recorded) = load_flare_fixture("da40_gsg2056_bounce.jsonl.gz");
        let a = compute_landing_analysis(&samples, edge_at, Simulator::Msfs2024, None);
        assert_eq!(
            a.get("flare_detected").and_then(|v| v.as_bool()),
            Some(false),
            "post-bounce settle must NOT be detected as a flare"
        );
        assert_eq!(
            a.get("flare_vs_source").and_then(|v| v.as_str()),
            Some("simvar"),
            "bounce falls back to SimVar (data quality is fine, not 'unreliable')"
        );
    }
