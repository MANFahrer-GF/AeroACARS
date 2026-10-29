    use super::*;
    use flate2::read::GzDecoder;
    use std::collections::VecDeque;
    use std::io::{BufRead, BufReader};
    use std::path::Path;

    fn fixture_path(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// A trimmed real flight reconstructed from the committed fixture.
    /// Die Felder `analysis` und `scored_bounce_count` liest aktuell kein
    /// Test — sie bleiben, weil sie die aufgezeichnete Wahrheit der Fixture
    /// tragen und der naechste Replay-Test sie sofort braucht.
    #[allow(dead_code)]
    struct ReplayFlight {
        /// The recorded simulator, parsed from the `touchdown_detected`
        /// event's `sim` field — drives the de-lag sim gate.
        simulator: Simulator,
        /// The recorded touchdown edge timestamp (`landing_analysis.edge_at`
        /// == `touchdown_detected.contact_at`); the de-lag uses this as
        /// `touchdown_at`.
        edge_at: DateTime<Utc>,
        /// The raw, PRE-de-lag `landing_analysis.analysis` object (carries
        /// `vs_at_edge_fpm` + `peak_g_post_500ms`).
        analysis: serde_json::Value,
        /// The real pre-TD ring buffer reconstructed verbatim from
        /// `touchdown_window.samples`, as `TelemetrySample`s — exactly what
        /// the live de-lag estimator consumes.
        buffer: VecDeque<TelemetrySample>,
        /// The de-bounced bounce count the recorder actually scored with
        /// (`landing_analysis.scored_bounce_count`) — used to reproduce the
        /// class-band move faithfully (not a hand-picked 0).
        scored_bounce_count: u8,
    }

    fn parse_rfc3339(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s)
            .unwrap_or_else(|e| panic!("bad timestamp {s:?}: {e}"))
            .with_timezone(&Utc)
    }

    /// Reconstruct a `TelemetrySample` field-by-field from a recorded
    /// `touchdown_window` sample object. We build the crate-private struct
    /// directly (this is an in-crate test) rather than deriving Deserialize
    /// on the production type — the public API stays untouched. Every field
    /// the AGL estimator reads (`at`, `agl_ft`, `on_ground`) comes verbatim
    /// from the log.
    fn sample_from_json(v: &serde_json::Value) -> TelemetrySample {
        let f = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
        let f64v = |k: &str| v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0);
        TelemetrySample {
            // Aus den echten Daten, wenn das Protokoll den Sprit traegt —
            // dieser Replay soll den Flug nachbilden, nicht ausdenken.
            fuel_kg: f("fuel_total_kg"),
            at: parse_rfc3339(v["at"].as_str().expect("sample.at")),
            vs_fpm: f("vs_fpm"),
            g_force: f("g_force"),
            g_semibody: None,
            on_ground: v
                .get("on_ground")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
            agl_ft: f("agl_ft"),
            msl_ft: f("agl_ft") + 500.0,
            heading_true_deg: f("heading_true_deg"),
            groundspeed_kt: f("groundspeed_kt"),
            indicated_airspeed_kt: f("indicated_airspeed_kt"),
            true_airspeed_kt: f("indicated_airspeed_kt"),
            lat: f64v("lat"),
            lon: f64v("lon"),
            pitch_deg: f("pitch_deg"),
            bank_deg: f("bank_deg"),
            gear_normal_force_n: v
                .get("gear_normal_force_n")
                .and_then(|x| x.as_f64())
                .map(|x| x as f32),
            total_weight_kg: v
                .get("total_weight_kg")
                .and_then(|x| x.as_f64())
                .map(|x| x as f32),
        }
    }

    fn sim_from_str(s: &str) -> Simulator {
        match s {
            "Msfs2024" => Simulator::Msfs2024,
            "Msfs2020" => Simulator::Msfs2020,
            "XPlane12" => Simulator::XPlane12,
            "XPlane11" => Simulator::XPlane11,
            other => panic!("unexpected sim {other:?} in fixture"),
        }
    }

    fn load_replay(name: &str) -> ReplayFlight {
        let path = fixture_path(name);
        let file =
            std::fs::File::open(&path).unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        let reader = BufReader::new(GzDecoder::new(file));

        let mut simulator: Option<Simulator> = None;
        let mut edge_at: Option<DateTime<Utc>> = None;
        let mut analysis: Option<serde_json::Value> = None;
        let mut buffer: VecDeque<TelemetrySample> = VecDeque::new();
        let mut scored_bounce_count: u8 = 0;

        for line in reader.lines() {
            let line = line.unwrap();
            if line.trim().is_empty() {
                continue;
            }
            let v: serde_json::Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("bad json line in {name}: {e}"));
            match v.get("type").and_then(|x| x.as_str()).unwrap_or("") {
                "touchdown_detected" => {
                    if let Some(s) = v.get("sim").and_then(|x| x.as_str()) {
                        simulator = Some(sim_from_str(s));
                    }
                }
                "touchdown_window" => {
                    edge_at = v.get("edge_at").and_then(|x| x.as_str()).map(parse_rfc3339);
                    for sv in v
                        .get("samples")
                        .and_then(|x| x.as_array())
                        .expect("touchdown_window.samples")
                    {
                        buffer.push_back(sample_from_json(sv));
                    }
                }
                "landing_analysis" => {
                    if edge_at.is_none() {
                        edge_at = v.get("edge_at").and_then(|x| x.as_str()).map(parse_rfc3339);
                    }
                    analysis = v.get("analysis").cloned();
                    if let Some(a) = &analysis {
                        if let Some(n) = a.get("scored_bounce_count").and_then(|x| x.as_u64()) {
                            scored_bounce_count = n as u8;
                        }
                    }
                }
                _ => {}
            }
        }

        ReplayFlight {
            simulator: simulator.expect("fixture missing touchdown_detected.sim"),
            edge_at: edge_at.expect("fixture missing edge_at"),
            analysis: analysis.expect("fixture missing landing_analysis"),
            buffer,
            scored_bounce_count,
        }
    }

    /// Pull a number out of an analysis object (post-mutation reads).
    fn num(analysis: &serde_json::Value, key: &str) -> Option<f32> {
        analysis.get(key).and_then(|v| v.as_f64()).map(|x| x as f32)
    }

    /// v1.6.3: Beweis an ECHTEN Flügen, dass die Höhenkurve die
    /// dokumentierte Wahrheit trifft — dieselben zwei Aufzeichnungen, an
    /// denen v0.16.21 seinerzeit die (nie gelaufene) Entlagung kalibriert
    /// hat. Der gedämpfte SimVar-Kanal las damals 441 bzw. 422 fpm, die
    /// aus der Höhe abgeleitete Wahrheit lag bei rund 251 bzw. 263.
    ///
    /// Der Test läuft durch `compute_landing_analysis`, also den Weg, den
    /// echte Daten nehmen — nicht gegen eine Hilfsfunktion. Genau daran ist
    /// die alte Entlagung gescheitert: 20 grüne Tests, im Feld nie gelaufen.
    fn assert_hoehenkurve_trifft(datei: &str, wahrheit_fpm: f32, band: f32) {
        let f = load_replay(datei);
        let samples: Vec<TouchdownWindowSample> =
            f.buffer.iter().cloned().map(Into::into).collect();
        let analysis = compute_landing_analysis(&samples, f.edge_at, f.simulator, None);
        let gescort = num(&analysis, "vs_at_edge_fpm").expect("vs_at_edge_fpm");
        let quelle = analysis
            .get("vs_at_edge_quelle")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(
            quelle, "hoehenkurve",
            "{datei}: die Höhenkurve muss die gescorte Quelle sein, war {quelle}"
        );
        assert!(
            (gescort - wahrheit_fpm).abs() <= band,
            "{datei}: erwartet {wahrheit_fpm} ±{band} fpm, gemessen {gescort}"
        );
    }

    #[test]
    fn jbu322_hoehenkurve_trifft_die_wahrheit() {
        assert_hoehenkurve_trifft("jbu322_msfs_delag_fire.jsonl.gz", -251.0, 60.0);
    }

    #[test]
    fn ewg906_hoehenkurve_trifft_die_wahrheit() {
        assert_hoehenkurve_trifft("ewg906_msfs_delag_fire.jsonl.gz", -263.0, 60.0);
    }

    /// Die eigentliche Sicherheitsfrage: **spuelt die neue Quelle eine
    /// harte Landung weich?** DLH848 ist eine echt firme MSFS-Landung; der
    /// gedaempfte Kanal las sie mit 286 fpm zu weich. Die Hoehenkurve muss
    /// mindestens so hart lesen wie der Simulator — nie weicher.
    ///
    /// Diese Aufzeichnung lag nach dem Entfernen der Entlagung verwaist im
    /// Repo: ihr Test war mitgeloescht, die Datei geblieben. Genau der Fall,
    /// den v1.6.3 nicht wiederholen darf — Pruefungen duerfen nicht mit dem
    /// Code verschwinden, den sie abgesichert haben (QS-Befund v1.6.3).
    #[test]
    fn dlh848_firme_msfs_landung_wird_nicht_weichgespuelt() {
        let f = load_replay("dlh848_msfs_firm_control.jsonl.gz");
        let samples: Vec<TouchdownWindowSample> =
            f.buffer.iter().cloned().map(Into::into).collect();
        let analysis = compute_landing_analysis(&samples, f.edge_at, f.simulator, None);
        let geo = num(&analysis, "vs_geometrie_fpm").expect("Hoehenkurve fehlt");
        let simvar = num(&analysis, "vs_simvar_edge_fpm").expect("SimVar fehlt");
        assert!(
            geo <= simvar,
            "firme Landung darf nicht weicher gelesen werden: Kurve {geo}, SimVar {simvar}"
        );
        // Und sie muss klar im firmen Band bleiben, nicht nur knapp.
        assert!(
            geo < -300.0,
            "DLH848 ist firm; die Kurve liest {geo} fpm und damit zu weich"
        );
    }

    /// Der **einzige** Replay-Test auf X-Plane-Daten. Ohne ihn stellt v1.6.3
    /// X-Plane erstmals auf dieselbe Quelle um, ohne dass ein echter
    /// X-Plane-Flug das je bestaetigt — die Testlage waere schlechter als
    /// die Aenderung. ITY324 ist eine steile Landung (863 fpm gemeldet):
    /// hier ist der Simulator-Kanal ausnahmsweise gut, weil bei steilem
    /// Anflug nichts zu daempfen ist. Die Kurve muss das reproduzieren
    /// statt eine eigene Wahrheit zu erfinden.
    #[test]
    fn ity324_steile_xplane_landung_bleibt_steil() {
        let f = load_replay("ity324_xplane_steep_control.jsonl.gz");
        assert!(
            matches!(f.simulator, Simulator::XPlane11 | Simulator::XPlane12),
            "Fixture muss X-Plane sein, war {:?}",
            f.simulator
        );
        let samples: Vec<TouchdownWindowSample> =
            f.buffer.iter().cloned().map(Into::into).collect();
        let analysis = compute_landing_analysis(&samples, f.edge_at, f.simulator, None);
        let geo = num(&analysis, "vs_geometrie_fpm").expect("Hoehenkurve fehlt");
        let simvar = num(&analysis, "vs_simvar_edge_fpm").expect("SimVar fehlt");
        assert!(
            (geo - simvar).abs() < 60.0,
            "bei steilem Anflug muessen beide Quellen zusammenfallen: \
             Kurve {geo}, SimVar {simvar}"
        );
        assert!(geo < -700.0, "steile Landung, gemessen aber nur {geo} fpm");
    }

    /// Der Kontakt-Frame darf die Steigung nicht bestimmen. Ohne den
    /// `!on_ground`-Filter hob im Fixture a320_vrnevnl ein einzelner
    /// 1,49-ft-Sprung beim Aufsetzen das Ergebnis um 158 fpm an — bei einer
    /// Zweipunkt-Steigung waere das die halbe Punktzahl der Achse.
    #[test]
    fn bodenkontakt_samples_zaehlen_nicht_in_die_sinkrate() {
        let f = load_replay("jbu322_msfs_delag_fire.jsonl.gz");
        let samples: Vec<TouchdownWindowSample> =
            f.buffer.iter().cloned().map(Into::into).collect();
        let mit_boden = samples.iter().filter(|s| s.on_ground).count();
        assert!(
            mit_boden > 0,
            "Fixture taugt nicht als Beleg: enthaelt keine Boden-Samples"
        );
        let analysis = compute_landing_analysis(&samples, f.edge_at, f.simulator, None);
        let geo = num(&analysis, "vs_geometrie_fpm").expect("Hoehenkurve fehlt");
        // Fahrwerkskompression liest immer HAERTER; ein Wert jenseits des
        // Plausiblen waere das Zeichen, dass der Filter nicht greift.
        assert!(
            geo > -600.0 && geo < 0.0,
            "Kurve liest {geo} fpm — riecht nach mitgemessener Fahrwerkskompression"
        );
    }
