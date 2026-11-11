/// v0.5.39: Helper um Felder aus dem landing_analysis-JSON-Value zu lesen.
/// Werden vom Touchdown-Payload-Builder benutzt damit wir nicht jedes Mal
/// die gleichen .get(...).and_then(...).map(...) Kette dranschreiben.
fn ana_f32(v: &Option<serde_json::Value>, key: &str) -> Option<f32> {
    v.as_ref()?.get(key)?.as_f64().map(|x| x as f32)
}
fn ana_i32(v: &Option<serde_json::Value>, key: &str) -> Option<i32> {
    v.as_ref()?.get(key)?.as_i64().map(|x| x as i32)
}
fn ana_u32(v: &Option<serde_json::Value>, key: &str) -> Option<u32> {
    v.as_ref()?.get(key)?.as_u64().map(|x| x as u32)
}
fn ana_bool(v: &Option<serde_json::Value>, key: &str) -> Option<bool> {
    v.as_ref()?.get(key)?.as_bool()
}
/// v0.12.3: String aus der Forensik-Analyse-Map (z. B. `scored_g_method`).
fn ana_str(v: &Option<serde_json::Value>, key: &str) -> Option<String> {
    Some(v.as_ref()?.get(key)?.as_str()?.to_string())
}

// v0.12.3 (LE8): **der eine** Scored-G-Wert, den jeder Pfad eines
// `FlightStats` nutzt — Touchdown-Klassifikation, Activity-/ACARS-Text,
// MQTT-Touchdown-Payload und das `LandingScored`-Event.
//
// Liest den EMA-geglätteten `scored_g` aus der Forensik-Analyse. Gibt es
// kein Touchdown-Fenster (`landing_analysis` ist `None`), fällt der Wert
// definiert auf den rohen Peak zurück (`raw_fallback`, LE8) — **kein**
// Pfad bekommt je `None`/`0` an den Score.
// v0.20 (QS-Fix, fallback_zero-Score-Bug): gibt jetzt `None` zurück statt
// immer irgendeinen Wert — siehe `FlightStats::canonical_peak_g_force`.
// `ana_f32(landing_analysis, "scored_g")` kommt aus der unabhängigen
// touchdown_v2-Forensik-Pipeline; die ist bei einem fallback_zero-Flug
// (kein validierter Touchdown, siehe `canonical_landing_rate_fpm`) von
// selbst leer, muss also nicht extra gegatet werden.
// Kanal-Charakteristik des G-Messwerts, je Simulator — die Grundlage der
// Fairness-Normierung unten.
//
// **Der Befund (14.08.2026, Korpus 577 Landungen).** MSFS und X-Plane
// liefern NICHT dieselbe physikalische Groesse:
//
// * MSFS `G FORCE` ist ein gedaempfter Instrumentenwert. Regression ueber
//   473 Landungen: `G = 1,011 + 0,000844 · |fpm|` → implizierte
//   Abbremszeit **614 ms**.
// * X-Plane `sim/flightmodel2/misc/gforce_normal` ist die frame-genaue
//   Lastziffer. Regression ueber 104 Landungen: `G = 1,002 + 0,001942 ·
//   |fpm|` → **267 ms**.
//
// 614 ms sind fuer eine Fahrwerksfederung physikalisch unmoeglich (Airliner
// ~200-300 ms, GA-Blattfeder ~100-150 ms) — das ist die Signatur eines
// Tiefpasses im MSFS-Kanal. X-Plane liegt genau im plausiblen Bereich.
//
// **Warum eine reine Skalierung genuegt:** die Signalform ist in beiden Sims
// identisch (Median/Peak im 500-ms-Fenster 0,863 gegen 0,855). Es
// unterscheidet sich nur die Amplitude, nicht die Form — also reicht ein
// Amplitudenfaktor, kein Nachbau des Filters.
//
// **Warum MSFS die Referenz ist:** die Punkteschwellen in `sub_g_force`
// (1,20/1,40/1,70/2,10) stammen aus der Zeit, als AeroACARS MSFS-only war,
// und sind nie neu kalibriert worden. Auf X-Plane umzustellen haette jeden
// bestehenden Score verschoben; die Kette auf die kalibrierte Referenz zu
// bringen ist der kleinere und ehrlichere Eingriff.
//
// Der ROHWERT bleibt unangetastet — er wandert unveraendert als
// `peak_g_load` in die Forensik und ins Touchdown-Payload. Normalisiert
// wird ausschliesslich der Wert, der in die BEWERTUNG geht; genau dafuer
// existiert die Trennung `scored_g_load` / `peak_g_load` seit v0.12.3.
//
// Ein neuer Simulator traegt hier seine gemessene Steigung ein — dieselbe
// Messung, dieselbe Herleitung.
// Wert seit v3: bundle/grenzwerte/landung.json -> g_kanal_steigung_msfs (v2.1.24: 0.000_844)
// Wert seit v3: bundle/grenzwerte/landung.json -> g_kanal_steigung_xplane (v2.1.24: 0.001_942)

/// Rechnet einen G-Messwert auf die MSFS-Referenzkette um. `None` als
/// Simulator (oder ein unbekannter) bleibt unveraendert — lieber roh als
/// falsch normiert.
fn g_auf_referenzkette(g: f32, simulator: Option<&str>) -> f32 {
    let faktor = match simulator {
        Some("xplane") => crate::grenzwerte::grenzwerte().landung.g_kanal_steigung_msfs.wert / crate::grenzwerte::grenzwerte().landung.g_kanal_steigung_xplane.wert,
        _ => return g,
    };
    // Nur der Ueberschuss ueber 1 g wird skaliert: 1 g ist in beiden Sims
    // derselbe Ruhezustand, der Filter wirkt nur auf die Auslenkung.
    1.0 + (g - 1.0) * faktor
}

/// Die Sinkrate ist gesperrt — die Landung bekommt keine Note.
///
/// Nicht dasselbe wie "das Fenster reichte nicht" (`landung_abdeckung_fehlt`):
/// Hat MSFS die Aufsetzrate selbst gemeldet, gilt sie, obwohl unser Fenster
/// dünn war. Dann gibt es eine Note, aber keine G-Werte und keine Hopser.
/// Diese Unterscheidung fehlte beiden ersten Fassungen (Codex, zweite
/// Abnahme 13.09.2026): Das Frontend behandelte die Fenster-Markierung als
/// Totalsperre und warf auch gültige MSFS-Landungen aus Verlauf und Teilwerten,
/// während Note und Banner "keine Bewertung" gleichzeitig sichtbar blieben.
fn landung_nicht_bewertbar_json(stats: &FlightStats) -> Option<serde_json::Value> {
    let fehlt = stats.landung_abdeckung_fehlt.as_ref()?;
    if stats.canonical_landing_rate_fpm().is_some() {
        return None;
    }
    serde_json::to_value(fehlt).ok()
}

/// Werte aus dem Aufsetzfenster — oder nichts, wenn es nicht ausreichte.
///
/// Prüfbefund 13.09.2026: Die Sinkrate und der Score waren gesperrt, die
/// G-Forensik daneben (`g_at_edge`, `g_median_post_500ms`, …) lief weiter aus
/// demselben Analyse-JSON in Datensatz, MQTT-Payload und Anzeige. Der Client
/// zeigte dann vollständige G-Kacheln für eine Landung, die laut Note gar
/// nicht gemessen wurde — dieselbe Fehlerklasse, nur eine Anzeige weiter.
fn aus_dem_fenster(stats: &FlightStats, wert: Option<f32>) -> Option<f32> {
    wert.filter(|_| stats.landung_abdeckung_fehlt.is_none())
}

/// Hopser aus dem Aufsetzfenster — oder keine, wenn es nicht ausreichte.
///
/// Codex, dritte Abnahme 13.09.2026: Nach Sinkrate, Note und G-Werten liefen
/// die Hopser noch über drei Wege weiter — in die diskrete Touchdown-Klasse
/// (`LandingScore::classify`), in den MQTT-Payload und ins ACARS-Log. Bei
/// gültiger MSFS-Sinkrate wird dieser Payload tatsächlich versendet; die
/// Empfänger sahen dann scheinbar gültige Hopser ohne jede Kennzeichnung.
/// Ein Hopser zwischen zwei fehlenden Proben bleibt unsichtbar, und "0"
/// läse sich als saubere Landung.
fn hopser_aus_dem_fenster(stats: &FlightStats) -> Option<u32> {
    if stats.landung_abdeckung_fehlt.is_some() {
        return None;
    }
    Some(stats.bounce_count as u32)
}

fn score_g_for_stats(stats: &FlightStats) -> Option<recorder::ScoredG> {
    // ⚠ Zuerst die Sperre, dann erst die Analyse.
    //
    // Codex-Befund 12.09.2026: Diese Funktion griff VOR der kanonischen
    // G-Sperre direkt auf `landing_analysis.scored_g` zu — und `unwrap_or`
    // setzte den dort verworfenen Rohwert gleich wieder ein. Über diesen Weg
    // entstanden G-Teilscore und das phpVMS-Feld "Landing G-Force" auch für
    // Landungen, deren Aufsetzfenster gar nicht ausreichend aufgezeichnet
    // wurde. Der geglättete Wert stammt aus denselben Proben wie der rohe.
    if stats.landung_abdeckung_fehlt.is_some() {
        return None;
    }
    if let Some(scored) = ana_f32(&stats.landing_analysis, "scored_g") {
        let method = match ana_str(&stats.landing_analysis, "scored_g_method").as_deref() {
            Some("raw_fallback") => recorder::ScoredGMethod::RawFallback,
            _ => recorder::ScoredGMethod::EmaMax,
        };
        return Some(recorder::ScoredG {
            scored_g: scored,
            raw_peak: stats.canonical_peak_g_force().unwrap_or(scored),
            method,
        });
    }
    stats
        .canonical_peak_g_force()
        .map(recorder::scored_g_raw_fallback)
}

/// Der G-Wert **fuer die Punktevergabe** — `score_g_for_stats` plus die
/// Umrechnung auf die Referenz-Messkette (siehe `g_auf_referenzkette`).
///
/// **Warum getrennt von `score_g_for_stats` (QS-Befund v1.6.2).** Der erste
/// Wurf normierte direkt in `score_g_for_stats` — und traf damit weit mehr
/// als die Bewertung: dieselbe Funktion speist das phpVMS-Feld
/// „Landing G-Force", das das dmaintenance-Plugin fuer die Schadens-
/// Strafberechnung liest, dazu Activity-Log, ACARS-Text und den lokalen
/// Flugbericht. Ein X-Plane-Pilot mit real 1,72 g waere dort als 1,31 g
/// erschienen — die Schadens-Erdung haette nicht mehr ausgeloest, waehrend
/// derselbe Stoss auf MSFS erdet. Genau die Ungerechtigkeit, die der Fix
/// beseitigen soll, nur andersherum.
///
/// Deshalb: **alles, was eine physikalische Aussage macht, bekommt den
/// gemessenen Wert. Nur die Punktevergabe bekommt den umgerechneten.**
fn scored_g_fuer_punkte(stats: &FlightStats) -> Option<f32> {
    score_g_for_stats(stats).map(|s| g_auf_referenzkette(s.scored_g, stats.landing_simulator))
}

// ======================================================================
// v0.16.22 — MSFS flare metric on the lag-free AGL-geometric V/S
// ======================================================================
//
// PROBLEM. The flare metric in `compute_landing_analysis` derives
// `peak_vs_pre` / `vs_at_flare_end` (window [-2000ms, -100ms]) from
// `s.vs_fpm`. On MSFS that field is the DISPLAY vertical-speed SimVar,
// which lags the airframe ~0.5-0.7 s through the flare (same lag the
// v0.16.21 touchdown de-lag corrects at the edge). So at -100 ms the
// SimVar still reflects the descent ~0.6 s earlier — BEFORE a late
// flare took effect — the reduction reads near zero and the metric
// reports "no flare" even when the pilot genuinely pulled.
//
// Proven on JBU323 (Fenix A320, MSFS2024): lagged peak -469 / end -440
// → reduction 30 fpm → flare_detected=FALSE. But the lag-free AGL-
// geometric derivative over the SAME window shows the real profile
// (pre-flare ~-569, mid-flare ~-370) → real reduction ~130-200 fpm; the
// pitch rose 2.5°→5.2°. The pilot DID flare — it was just late, so the
// aircraft still settled firm (edge -423, peak_g 1.43). The de-lag fix
// already credits the firmer-than-displayed *touchdown*; this fix
// credits the *flare* the lag hid, without softening the touchdown.
//
// THE FIX. For MSFS fixed-wing, compute `peak_vs_pre` / `vs_at_flare_
// end` from the AGL trace via a SMOOTHED LOCAL LINEAR FIT (least-squares
// slope of agl_ft over a ±MSFS_FLARE_AGL_FIT_HALF_WINDOW_MS window of
// samples) rather than raw consecutive-sample diffs (too noisy). The
// slope (ft/min) is the geometric sink at that instant — lag-free,
// because position/AGL is not the lagged SimVar. X-Plane (raw local_vy,
// far less lagged) and Other keep the existing `s.vs_fpm` path so their
// analysis output is byte-identical.
//
// WHY THE FIT IS NOISE-ROBUST. A single-sample AGL diff at 50 Hz is
// dominated by AGL quantization (MSFS reports AGL in ~0.5-0.6 ft steps).
// A least-squares slope over ±300 ms (typically 15-20 samples) averages
// that quantization out — the residual after the linear trend is the
// noise floor, and the slope is the trend. On the corpus the detection
// verdict is identical at half-windows of 250/300/350 ms for every
// flight (only JBU323's reduction MAGNITUDE moves, 116-207 fpm, all
// well above the 50 fpm threshold). We pick the STEEPEST fitted slope in
// the window as `peak_vs_pre`; on JBU323 that peak (-659) is supported
// by a plateau of neighbours in the -580..-660 band, not a lone spike.
//
// NO PHANTOM FLARES. The reduction is a REAL geometric reduction in the
// AGL trace, not a SimVar lag artifact. A firm landing with no real
// flare (DLH848: AGL slope flat at -382 across the window) yields
// reduction 0 → flare_detected=false, unchanged. The endpoint score
// still reflects the firm touchdown — we credit the reduction, never
// turn a firm landing soft.

// Half-window (ms) of AGL samples used for each local linear-fit slope.
// ±300 ms ≈ 15-20 samples at 50 Hz — wide enough to average out MSFS's
// ~0.5 ft AGL quantization, narrow enough to resolve the flare's sink
// reduction. Corpus-stable: detection verdict identical at 250/350 too.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_agl_fit_half_window_ms (v2.1.24: 300)
// Minimum airborne AGL samples inside a fit window for the slope to be
// trusted. Below this the least-squares slope is too sensitive to a
// single quantization step; we skip that time point.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_agl_fit_min_samples (v2.1.24: 4)
/// AGL above this (ft) at a fit point means we are not yet in the flare/
/// short-final geometry — the same guard the touchdown AGL estimator
/// uses for window-start. Keeps a high pre-flare descent out of the
/// flare-window slope set. Reuses TD_AGL_MAX_AT_WINDOW_START_FT.
// v3: aus einem Grenzwert abgeleitet, deshalb Funktion statt Konstante.
// ─── v0.16.22 hardening: phantom-flare guards ────────────────────────────
//
// Adversarial QS found two phantom "butter" flares the bare AGL path
// produced. Both are DATA pathologies, not piloting:
//   • E195 MXY322 — the AGL trace FROZE at 9.96 ft for ~14 samples
//     (-466…-31 ms), so the flare-end fit slope read ~0 → vs_at_flare_end
//     ≈ 0 → reduction 742 → score 100, on a firm -315 fpm / 1.46 g arrival
//     with NOSE-DOWN pitch. The whole flare window had only 4 DISTINCT
//     agl_ft values.
//   • DA40 GSG2056 — touched at -1626 ms (on_ground=true, g 1.3),
//     ballooned ~2.4 ft, settled. The credited "reduction" was the
//     post-bounce settle, pitch -6° nose-DOWN at flare-end.
//
// A 316-landing VPS sweep + the local corpus drove the thresholds below.
// Gate 1 (data quality) + Gate 2 (bounce) suppress BOTH phantoms AND the
// near-frozen B738 (7 distinct) QS also flagged, while NOT touching any
// of 115 real flares (incl. JBU323, the FSLabs A321, the Baron 58P).
//
// Gate 1a — minimum DISTINCT `agl_ft` values among the airborne flare-
// window samples for the AGL geometry to be trusted. Below this the trace
// is too coarse to differentiate (a step/plateau, not a slope). VPS data:
// the only two flights under 8 in 316 landings are the two frozen
// phantoms (4 and 7 distinct); the lowest REAL flare has 18 distinct —
// a 2.25× margin. On reject we fall back to the SimVar path so no garbage
// AGL endpoint is published.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_min_distinct_agl (v2.1.24: 8)
// Gate 1b — maximum MEDIAN sample Δt (ms) inside the flare window. Above
// this the effective rate is < ~16 Hz and a local slope is unreliable.
// All healthy captures in the corpus sit at ~31 ms (32 Hz); this is a
// safety net for genuinely degraded capture (it fires on none of the 316
// — the frozen E195 still streamed at 31 ms, so distinct-count, not Δt,
// is what catches it). Reject → SimVar fallback.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_max_median_dt_ms (v2.1.24: 60)
// Gate 1c — minimum DISTINCT `agl_ft` values in the FLARE-END fit window
// (±MSFS_FLARE_AGL_FIT_HALF_WINDOW_MS around flare_hi). This is the LOCAL
// version of Gate 1a: it catches a freeze that corrupts ONLY the flare-
// end slope while the rest of the window streamed fine. Such a freeze
// reads vs_at_flare_end ≈ 0 from a plateau and INFLATES the reduction
// (E195 phantom whole-window; vrNEVnl phantom mid-window with 25 distinct
// overall, which Gate 1a misses; pVGZW real-but-corrupt — a genuine flare
// whose AGL froze in the final ~360 ms so the AGL ENDPOINT is garbage and
// the honest SimVar value should publish instead). VPS data: the three
// frozen-end phantoms have 1-3 distinct values in the end-fit window; the
// largest REAL flares (reduction 500+) have 8 — a clean 2× margin at 4.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_min_end_fit_distinct_agl (v2.1.24: 4)
// Gate 1c BACKSTOP — minimum AGL SPAN (ft) in the flare-END fit window
// (±MSFS_FLARE_AGL_FIT_HALF_WINDOW_MS around flare_hi). The distinct-bucket
// count above (Gate 1c) is fooled by a NOISY freeze: a trace that dithers
// across ≥4 quantized 0.01 ft buckets while spanning near-zero TRUE height
// passes the bucket count yet still reads vs_at_flare_end ≈ 0 from the flat
// plateau → a huge fake reduction → phantom score-100. The span check is the
// physics floor that bucket-counting cannot give: in the final ±300 ms a
// REAL flare descends through real height. On the corpus the slowest REAL
// flare-end (extra_y8Y5, end −95 fpm) covers ≈0.95 ft in that 600 ms window
// and the JBU323/jbu322 flares span 4-6 ft; a frozen/near-frozen plateau
// spans ~0. 0.3 ft sits an order of magnitude below the slowest real end
// span yet an order of magnitude above quantization+jitter (~0.01-0.05 ft),
// so it fires ONLY on a true plateau. Reject → SimVar fallback.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_min_end_agl_span_ft (v2.1.24: 0.3)
// Gate 2 — balloon tolerance (ft). If the airborne AGL trace rises by
// more than this above a prior in-window minimum it is non-monotone (a
// bounce/balloon), not a descent+flare. DA40 GSG2056 balloons ~2.4 ft;
// every real flare in the corpus stays ≤ ~0.25 ft. Reject → SimVar
// fallback. (An on_ground=true sample anywhere in the window is a
// separate, unconditional bounce reject.)
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_balloon_tol_ft (v2.1.24: 1.5)
// Gate 3 — pitch-corroboration veto. `flare_detected` is additionally
// withheld when the net pitch trend across the window is strongly
// NOSE-DOWN (a settle/shed signature, not a pull). Tuned conservatively:
// real held-attitude flares on the VPS show window pitch-deltas as low as
// −0.6° (the pilot sets a flare attitude early and HOLDS it while the
// sink decays in ground effect), so a positive "+Δ required" gate would
// wrongly suppress real flares — Gates 1+2 are the primary phantom
// defense. This veto only trips on a clear nose-down trend, below every
// real flare observed, and is belt-and-suspenders for a future bounce
// that somehow evades Gate 2.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_pitch_nosedown_veto_deg (v2.1.24: -1.0)
// The flare-source detection floor (fpm of reduction). Kept at 50 (NOT
// nudged to 60): the real FSLabs A321 flare in the QS set reduces 58.5
// fpm and MUST stay detected, so 60 would drop a real marginal flare.
// The fragile band is instead cleaned by Gates 1-3, which remove the
// frozen/bounce members of that band without touching real soft flares.
// Wert seit v3: bundle/grenzwerte/landung.json -> msfs_flare_detect_floor_fpm (v2.1.24: 50.0)

// Seit dem 11.11.2026 (DLH9901) stehen Rechnung und Riegel im Crate
// `landing-scoring` (`aufsetz_messung`) und laufen ueber den Paket-Export
// `messe`; die Kommentare oben begruenden die Schwellen weiter. Hier bleiben
// die Anfrage an das Modul und duenne Test-Huellen fuer die bestehenden
// Riegel-Tests (sie rechnen gegen dieselbe Quelle).

/// Anfrage an die Aufsetz-Messung: alle Kanaele jeder Probe plus `t_ms`
/// (Millisekunden relativ zur Kante, wie bisher gerechnet) und die
/// Grenzwerte des geltenden Pakets.
fn mess_anfrage(samples: &[TouchdownWindowSample], edge_ms: i64, simulator: Simulator) -> serde_json::Value {
    let g = &crate::grenzwerte::grenzwerte().landung;
    let parameter = landing_scoring::aufsetz_messung::Parameter {
        fenster_ms: crate::grenzwerte::grenzwerte().aufsetzen.luecken_fenster_vor_ms.wert,
        agl_min_samples: g.agl_min_samples.wert,
        agl_min_verschiedene: g.agl_min_verschiedene.wert,
        agl_max_stillstand_ms: g.agl_max_stillstand_ms.wert,
        agl_min_spanne_s: g.agl_min_spanne_s.wert,
        agl_max_plausibel_fpm: g.agl_max_plausibel_fpm.wert,
        probe_toleranz_fpm: g.probe_toleranz_fpm.wert,
        td_agl_max_at_window_start_ft: g.td_agl_max_at_window_start_ft.wert,
        msfs_flare_agl_fit_half_window_ms: g.msfs_flare_agl_fit_half_window_ms.wert,
        msfs_flare_agl_fit_min_samples: g.msfs_flare_agl_fit_min_samples.wert,
        msfs_flare_min_distinct_agl: g.msfs_flare_min_distinct_agl.wert,
        msfs_flare_max_median_dt_ms: g.msfs_flare_max_median_dt_ms.wert,
        msfs_flare_min_end_fit_distinct_agl: g.msfs_flare_min_end_fit_distinct_agl.wert,
        msfs_flare_min_end_agl_span_ft: g.msfs_flare_min_end_agl_span_ft.wert,
        msfs_flare_balloon_tol_ft: g.msfs_flare_balloon_tol_ft.wert,
        msfs_flare_pitch_nosedown_veto_deg: g.msfs_flare_pitch_nosedown_veto_deg.wert,
        msfs_flare_detect_floor_fpm: g.msfs_flare_detect_floor_fpm.wert,
    };
    let proben: Vec<serde_json::Value> = samples
        .iter()
        .map(|s| {
            let mut v = serde_json::to_value(s).unwrap_or(serde_json::Value::Null);
            if let Some(o) = v.as_object_mut() {
                o.insert("t_ms".into(), (s.at.timestamp_millis() - edge_ms).into());
            }
            v
        })
        .collect();
    let simulator = serde_json::to_value(simulator)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "Other".into());
    serde_json::json!({ "simulator": simulator, "proben": proben, "parameter": parameter })
}

/// Nur Tests: Probe mit absoluter Zeit (die Riegel rechnen nur mit
/// Differenzen, der Nullpunkt ist gleichgueltig).
#[cfg(test)]
fn als_proben(samples: &[TouchdownWindowSample]) -> Vec<landing_scoring::aufsetz_messung::Probe> {
    let anfrage = mess_anfrage(samples, 0, Simulator::Msfs2024);
    serde_json::from_value::<landing_scoring::aufsetz_messung::Anfrage>(anfrage).expect("Anfrage").proben
}

#[cfg(test)]
fn mess_parameter() -> landing_scoring::aufsetz_messung::Parameter {
    serde_json::from_value(mess_anfrage(&[], 0, Simulator::Msfs2024)["parameter"].clone()).expect("Parameter")
}

#[cfg(test)]
fn agl_geometric_vs_fpm_at(
    samples: &[TouchdownWindowSample],
    t_ms: i64,
    pitch_deg_at_t: f32,
    fit_hi_cap: Option<i64>,
) -> Option<f32> {
    landing_scoring::aufsetz_messung::agl_geometric_vs_fpm_at(&als_proben(samples), &mess_parameter(), t_ms, pitch_deg_at_t, fit_hi_cap)
}

#[cfg(test)]
fn compute_msfs_agl_flare_endpoints(samples: &[TouchdownWindowSample], flare_lo: i64, flare_hi: i64) -> (Option<f32>, Option<f32>) {
    landing_scoring::aufsetz_messung::msfs_agl_flare_endpoints(&als_proben(samples), &mess_parameter(), flare_lo, flare_hi)
}

#[cfg(test)]
fn evaluate_msfs_flare_gates(
    samples: &[TouchdownWindowSample],
    flare_lo: i64,
    flare_hi: i64,
) -> landing_scoring::aufsetz_messung::MsfsFlareGates {
    landing_scoring::aufsetz_messung::msfs_flare_gates(&als_proben(samples), &mess_parameter(), flare_lo, flare_hi)
}

/// v0.5.39: Forensik-Bewertung der Landung aus dem 50-Hz-Sample-Buffer
/// um den TD-Edge. Gibt JSON-Map zurück, die im LandingAnalysis-Event in
/// die JSONL geschrieben + im MQTT-Touchdown-Payload mit-publisht wird.
///
/// Berechnete Felder:
///   - vs_at_edge_fpm           — interpolierte VS exakt am on_ground-Edge
///   - vs_smoothed_250ms_fpm    — Mean über letzte 250 ms vor Edge
///   - vs_smoothed_500ms_fpm    — 500 ms (= Volanta-Style)
///   - vs_smoothed_1000ms_fpm   — 1000 ms (= DLHv-Style)
///   - vs_smoothed_1500ms_fpm   — 1500 ms
///   - peak_g_post_500ms        — Max G im 500 ms post-Edge (Gear-Spike)
///   - peak_g_post_1000ms       — Max G im 1000 ms post-Edge
///   - peak_vs_pre_flare_fpm    — steepste Sinkrate in [-2000, -100 ms]
///   - vs_at_flare_end_fpm      — VS bei -100 ms (kurz vor TD)
///   - flare_reduction_fpm      — peak_vs_pre_flare - vs_at_flare_end
///   - flare_dvs_dt_fpm_per_sec — Steigungs-Rate der VS-Reduktion
///   - flare_quality_score      — 0..100, höher = besser geflared
///   - flare_detected           — bool, true wenn signifikante Reduktion
///
/// v0.16.22: Auf MSFS (fixed-wing) werden die Flare-Endpunkte
/// `peak_vs_pre_flare_fpm` / `vs_at_flare_end_fpm` aus der LAG-FREIEN
/// AGL-Geometrie (lokaler Linear-Fit der agl_ft-Spur) berechnet statt
/// aus dem lagbehafteten `s.vs_fpm`-Display-SimVar — sonst meldet der
/// Flare-Metrik bei spät/firm geflareten Landungen faelschlich "kein
/// Flare". Die roh-lagbehafteten Werte bleiben als Forensik-Keys
/// (`peak_vs_pre_flare_fpm_raw`, `vs_at_flare_end_fpm_raw`); `flare_vs_
/// source` markiert die Quelle ("msfs_agl" | "simvar"). X-Plane (raw
/// local_vy, kaum Lag) und Other behalten den SimVar-Pfad — ihre
/// Analyse-Ausgabe ist byte-identisch. Touchdown-Rate/Klasse und der
/// Master-Score bleiben unveraendert (Flare ist forensisch/Coaching).
///   - bounce_count             — Anzahl on_ground=False Excursionen post-TD
///   - bounce_max_agl_ft        — höchster Bounce
///   - sample_count             — Anzahl Samples im Buffer
///   - pre_edge_sample_count    — davon vor Edge (= Pre-TD-Coverage)
///   - post_edge_sample_count   — davon nach Edge
#[cfg_attr(not(test), allow(dead_code))]
fn compute_landing_analysis(
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
    simulator: Simulator,
    sim_referenz: Option<f32>,
) -> serde_json::Value {
    compute_landing_analysis_fuer(None, samples, edge_at, simulator, sim_referenz)
}

/// Wie [`compute_landing_analysis`], gemessen mit dem Modul des Flugs
/// `pirep_id` (gepinnter oder nachgeholter Stand, wie die Note).
fn compute_landing_analysis_fuer(
    pirep_id: Option<&str>,
    samples: &[TouchdownWindowSample],
    edge_at: DateTime<Utc>,
    simulator: Simulator,
    // v1.6.9 — die vom Simulator gemeldete Aufsetzgeschwindigkeit, falls
    // vorhanden (MSFS). Wandert unveraendert in die Analyse, damit der
    // Landebericht unsere Zahl und die des Simulators nebeneinander
    // zeigen kann.
    sim_referenz: Option<f32>,
) -> serde_json::Value {
    use serde_json::json;
    if samples.is_empty() {
        return json!({ "error": "empty_buffer" });
    }
    let edge_ms = edge_at.timestamp_millis();

    // --- Multi-Window VS-Mittel (nur airborne-Samples vor Edge mit
    // negativer V/S — das ist die "reine Sinkrate"). Positive Samples
    // (= Float-Effekt, Ground-Effect-Bumps, Ballooning kurz vor TD)
    // wuerden den Mittelwert sonst Richtung 0 verfaelschen und einen
    // sanfteren Touchdown vortaeuschen als physikalisch passiert ist.
    // Volanta/DLHv filtern aehnlich. ---
    let mean_vs_window = |window_ms: i64| -> Option<f32> {
        let lo = edge_ms - window_ms;
        let mut sum = 0.0_f64;
        let mut n = 0_u32;
        for s in samples {
            let ts = s.at.timestamp_millis();
            if ts >= lo && ts <= edge_ms && !s.on_ground && s.vs_fpm < 0.0 {
                sum += s.vs_fpm as f64;
                n += 1;
            }
        }
        if n > 0 {
            Some((sum / n as f64) as f32)
        } else {
            None
        }
    };
    let vs_250 = mean_vs_window(250);
    let vs_500 = mean_vs_window(500);
    let vs_1000 = mean_vs_window(1000);
    let vs_1500 = mean_vs_window(1500);

    // --- Sinkrate aus der HÖHENKURVE (v1.6.3, die gescorte Quelle) ---
    //
    // **Warum nicht mehr der V/S-Messwert des Sims.** Pilotenbefund Adrian
    // (14.08.2026): „Die Dämpfung im MSFS ist einer der Gründe, warum ich
    // da nur bei long landings niedrige Landeraten habe." An 764
    // gespeicherten Aufsetz-Fenstern nachgemessen — der Fehler des
    // SimVar-Kanals hängt an der Abfangstrecke:
    //
    //   MSFS, kurz aufgesetzt:     359 fpm gemeldet, 268 tatsächlich
    //   MSFS, lang abgefangen:     189 fpm gemeldet, 217 tatsächlich
    //   MSFS, sehr lang:           113 fpm gemeldet, 137 tatsächlich
    //
    // Die Verzerrung dreht sich also mit dem Abfangbogen um. Besonders
    // bitter: seit v1.4.4 soll zu weiches Aufsetzen Punkte kosten (langer
    // Abfangbogen → späte Bremswirkung) — der Messfehler hob genau diese
    // Landungen wieder ins volle Punkteband.
    //
    // **Es IST ein MSFS-Phänomen.** Eine frühere Fassung dieses Kommentars
    // behauptete das Gegenteil („kein MSFS-Sonderfall, generell die bessere
    // Messung"); nach Simulator getrennt nachgerechnet stimmt das nicht.
    // In X-Plane liegen gemeldeter und tatsächlicher Wert praktisch
    // aufeinander (kurz 464 gegen 475 fpm, lang 215 gegen 220) — dort gibt
    // es fast nichts zu korrigieren. Der Hinweis des Piloten, der das
    // ausgelöst hat, benannte MSFS von Anfang an richtig.
    //
    // **Warum die Höhenkurve die Wahrheit besser trifft.** Die G-Kraft
    // misst denselben Stoß unabhängig und dient als Schiedsrichter.
    // Übereinstimmung mit ihr (748 Fenster, mit allen Riegeln dieses
    // Codes gerechnet):
    //
    //                        SimVar   Höhengeometrie
    //   MSFS                  0,801        0,930
    //   X-Plane               0,720        0,845
    //
    // Die Geometrie gewinnt in BEIDEN Simulatoren — sie ist also überall
    // die richtige Quelle, auch wenn der behobene Fehler fast nur MSFS
    // betrifft. Damit verschwindet zugleich die letzte sim-spezifische
    // Verzweigung im Sinkraten-Pfad.
    //
    // **Fensterbreite 310 ms** — am Korpus optimiert (kürzer wird es
    // verrauscht, länger mittelt es den späten flachen Teil des Abfangens
    // ein und liest zu nah an Null).
    // Dieselbe Konstante begrenzt die Lückenprüfung der Bewertbarkeit.
    // Wert seit v3: bundle/grenzwerte/aufsetzen.json -> luecken_fenster_vor_ms (v2.1.24: 310)
    // Weniger Messpunkte im Fenster → keine belastbare Steigung.
    // Wert seit v3: bundle/grenzwerte/landung.json -> agl_min_samples (v2.1.24: 4)
    // Manche Zusatzflugzeuge frieren die Hoehe ein. Ohne Bewegung in der
    // Kurve ist die Ableitung wertlos (im Korpus: 0,5 % der Landungen).
    // Wert seit v3: bundle/grenzwerte/landung.json -> agl_min_verschiedene (v2.1.24: 3)
    // Wie lange die Hoehe hoechstens stillstehen darf, in Millisekunden.
    //
    // **Warum das zusaetzlich zu `AGL_MIN_VERSCHIEDENE` noetig ist.** Die
    // Hoehe wird oft mit halber Bildrate nachgefuehrt; kurze Haltezeiten
    // sind der Normalfall und harmlos. Laengere sind es nicht: die
    // verschluckte Bewegung holt der Simulator danach am Stueck nach, und
    // die Ausgleichsgerade mittelt das Plateau mit ein — sie liest dann zu
    // FLACH, also zu weich.
    //
    // **Warum in Zeit und nicht in Messpunkten.** Die Abtastrate schwankt
    // um mehr als das Vierfache (MSFS median 36, X-Plane 45, im Extrem
    // 146 Werte je Sekunde). Fuenf gleiche Werte sind bei 36 Hz ein echter
    // Stillstand von 140 ms, bei 146 Hz aber nur 34 ms ganz normaler
    // Ueberabtastung. Eine Zaehlgrenze verwirft deshalb genau die
    // Aufzeichnungen mit der BESTEN Datenlage. Eine erste Fassung dieses
    // Riegels zaehlte Messpunkte und kostete unterm Strich Punkte
    // (QS-Befund Runde 4).
    //
    // **Die Grenze ist gemessen, nicht geschaetzt.** Ueber 748 Fenster mit
    // der G-Kraft als unabhaengigem Schiedsrichter: bis 40 ms Stillstand
    // trifft die Hoehenkurve in 70 % der Faelle naeher als der
    // Simulator-Kanal, bei 41–80 ms in 58 %, bei 81–120 ms in 79 % — erst
    // ab 121 ms kippt es auf 43 %, dort ist der Kanal also besser.
    // Betroffen sind 0,9 % der Landungen.
    //
    // Anlass war die Aufzeichnung `a320_vrnevnl_frozen_end`: die Hoehe
    // steht dort ueber eine halbe Sekunde still, danach kommt der ganze
    // Weg in einem Schritt. Die Gerade las −45 statt −162 fpm — der Pilot
    // haette 15 Punkte fuer einen langen Abfangbogen verloren, den er nie
    // geflogen ist.
    // Wert seit v3: bundle/grenzwerte/landung.json -> agl_max_stillstand_ms (v2.1.24: 120)
    // Die Ausgleichsgerade braucht eine echte Zeitbasis. Gemessen liefert
    // MSFS rund 32, X-Plane rund 43 Hoehenwerte je Sekunde; 0,15 s sind
    // also etwa 5 bis 7 Messpunkte. Darunter bestimmt der Abstand zweier
    // Nachfuehrschritte den Anstieg vollstaendig.
    // Wert seit v3: bundle/grenzwerte/landung.json -> agl_min_spanne_s (v2.1.24: 0.15)
    // Ueber diesem Betrag ist es kein Sinkflug mehr, sondern ein Sprung im
    // Hoehenbezug (Gelaendekante, Helipad-Dach, Bodenmodell-Wechsel).
    // Deckt sich mit `VS_FLOOR_FPM` der kanonischen Landerate.
    // Wert seit v3: bundle/grenzwerte/landung.json -> agl_max_plausibel_fpm (v2.1.24: -3000.0)

    // `auch_bei_stillstand` schaltet den Stillstands-Riegel ab. Der Score
    // laesst ihn an (eine eingefrorene Hoehenspur liest zu weich, siehe
    // `AGL_MAX_STILLSTAND_MS`); die Unfall-Erkennung braucht den Wert
    // trotzdem, weil sie den HAERTESTEN aller Messwerte nimmt und ein zu
    // weicher Kandidat ihr nicht schaden kann. Ohne diesen zweiten Aufruf
    // verlaere sie bei eingefrorener Spur genau den Kandidaten, den der
    // Kommentar dort ausdruecklich zusagt (QS-Befund Runde 4).
    // DLH9901 (11.11.2026): Hoehengerade, Riegel, Instrument, Wahl und
    // Abfang-Endpunkte rechnet die Aufsetz-Messung des Pakets (Export
    // `messe`), sonst dieselbe Funktion nativ (aelteres Paket ohne Export).
    let (messung, messung_quelle) = crate::bewertung::messen(pirep_id, &mess_anfrage(samples, edge_ms, simulator));
    let vs_geometrie = messung.vs_geometrie_fpm;
    let vs_geometrie_ungefiltert = messung.vs_geometrie_ungefiltert_fpm;
    let vs_aus_simvar = messung.vs_simvar_edge_fpm;
    let vs_at_edge = messung.vs_at_edge_fpm;
    let vs_gelaende = messung.vs_gelaende_fpm;
    let vs_eigensinken = messung.vs_eigensinken_fpm;
    let peak_vs_pre = messung.peak_vs_pre_flare_fpm;
    let vs_at_flare_end = messung.vs_at_flare_end_fpm;
    let flare_reduction = messung.flare_reduction_fpm;
    let flare_dvs_dt = messung.flare_dvs_dt_fpm_per_sec;
    let flare_score = messung.flare_quality_score;
    let flare_detected = messung.flare_detected;
    // Das Probenpaar um die Kante braucht weiter die G-Kraft am Aufsetzpunkt.

    let mut last_air: Option<&TouchdownWindowSample> = None;
    let mut first_ground: Option<&TouchdownWindowSample> = None;
    for s in samples {
        if !s.on_ground {
            last_air = Some(s);
        } else if last_air.is_some() && first_ground.is_none() {
            first_ground = Some(s);
            break;
        }
    }

    // ── v1.6.9: woraus die Sinkrate besteht ──────────────────────────
    //
    // Die Hoehenkurve misst die Annaeherung an die OBERFLAECHE. Steigt
    // der Boden unter dem Flugzeug, steckt diese Bewegung mit in der
    // Zahl. Ueber 818 aufgezeichnete Landungen traegt das Gelaende im
    // Median 32 fpm bei, bei 12 % ueber 100 fpm, im Extremfall 451 fpm
    // (K4S2) — bei Bandbreiten von 90-250 fpm ist das ein ganzes Band.
    //
    // Bodenhoehe = MSL − AGL. Dieselbe Ausgleichsgerade, dasselbe
    // Fenster, dieselben Riegel wie oben — nur ueber eine andere Groesse.
    // Beides wird NUR ANGEZEIGT und geht in keine Punktzahl ein: erst
    // messen, dann entscheiden.
    //
    // v1.9.17 (Inventur 02.10.2026, B5/A): `bewegung_pruefen` schaltet die
    // Riegel „genug verschiedene Werte" und „kein Stillstand" ein. Fuer die
    // Hoehe ueber dem Meeresspiegel gelten sie (eingefrorene Spur = wertlos),
    // fuer die BODENHOEHE nicht: Ebenes Gelaende steht legitim still. Die
    // Riegel verwarfen dort die ganze Aufschluesselung — bei 10 von 60
    // Landungen (flache Plaetze), obwohl AGL und MSL einwandfrei waren.
    // --- Peak G post-TD (Gear-Compression sucht nach Edge) ---
    let peak_g_window = |window_ms: i64| -> Option<f32> {
        let hi = edge_ms + window_ms;
        let mut peak = 0.0_f32;
        let mut found = false;
        for s in samples {
            let ts = s.at.timestamp_millis();
            if ts >= edge_ms && ts <= hi {
                if s.g_force > peak {
                    peak = s.g_force;
                }
                found = true;
            }
        }
        if found {
            Some(peak)
        } else {
            None
        }
    };
    let pg_500 = peak_g_window(500);
    let pg_1000 = peak_g_window(1000);

    // Lernpaket AP1a (29.09.2026): Vergleich zweier MSFS-G-Kanaele. Fuer
    // jeden Kanal die Spitze im Fenster −500 ms … +2000 ms um die Kante und
    // ihr Versatz zur Kante. Kommt die Spitze von `SEMIBODY LOADFACTOR Y`
    // ueber viele Landungen deutlich frueher als die von `G FORCE`, ist er
    // der verzoegerungsaermere Kanal (AP1b). Rein forensisch, keine Note.
    let g_spitze_mit_versatz = |kanal: &dyn Fn(&TouchdownWindowSample) -> Option<f32>| {
        let mut best: Option<(f32, i64)> = None;
        for s in samples {
            let dt = s.at.timestamp_millis() - edge_ms;
            if !(-500..=2000).contains(&dt) {
                continue;
            }
            if let Some(g) = kanal(s) {
                if best.map_or(true, |(b, _)| g > b) {
                    best = Some((g, dt));
                }
            }
        }
        best
    };
    let g_force_spitze = g_spitze_mit_versatz(&|s| Some(s.g_force));
    let semibody_spitze = g_spitze_mit_versatz(&|s| s.g_semibody);
    let semibody_post = |window_ms: i64| -> Option<f32> {
        samples
            .iter()
            .filter(|s| {
                let ts = s.at.timestamp_millis();
                ts >= edge_ms && ts <= edge_ms + window_ms
            })
            .filter_map(|s| s.g_semibody)
            .fold(None, |acc: Option<f32>, g| {
                Some(acc.map_or(g, |a| a.max(g)))
            })
    };
    let semibody_500 = semibody_post(500);
    let semibody_1000 = semibody_post(1000);

    // v0.7.17 (B-009): G-Force-Forensik — analog Sinkrate-Forensik.
    //
    // Sample-basierte robuste Statistiken statt naivem max(), damit
    // Sim-Strut-Compression-Spikes (X-Plane + MSFS) nicht den Score
    // zerschiessen.

    // G im interpolierten Edge-Frame (analog vs_at_edge).
    let g_at_edge: Option<f32> = match (last_air, first_ground) {
        (Some(a), Some(g)) => {
            let t_a = a.at.timestamp_millis() as f64;
            let t_g = g.at.timestamp_millis() as f64;
            let t_edge = edge_ms as f64;
            if (t_g - t_a).abs() < 1e-6 {
                Some(a.g_force)
            } else {
                let alpha = ((t_edge - t_a) / (t_g - t_a)).clamp(0.0, 1.0);
                Some((a.g_force as f64 + (g.g_force as f64 - a.g_force as f64) * alpha) as f32)
            }
        }
        _ => None,
    };

    // Hilfsfunktion: G-Samples im Post-Edge-Fenster sammeln.
    let collect_g_post = |window_ms: i64| -> Vec<f32> {
        let hi = edge_ms + window_ms;
        samples
            .iter()
            .filter_map(|s| {
                let ts = s.at.timestamp_millis();
                if ts >= edge_ms && ts <= hi {
                    Some(s.g_force)
                } else {
                    None
                }
            })
            .collect()
    };

    // Mean / Median / 95p — robuste Statistiken.
    fn mean_of(xs: &[f32]) -> Option<f32> {
        if xs.is_empty() {
            return None;
        }
        let sum: f64 = xs.iter().map(|x| *x as f64).sum();
        Some((sum / xs.len() as f64) as f32)
    }
    fn percentile_of(xs: &[f32], p: f32) -> Option<f32> {
        if xs.is_empty() {
            return None;
        }
        let mut sorted: Vec<f32> = xs.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let idx = ((sorted.len() as f32 - 1.0) * p).round() as usize;
        Some(sorted[idx.min(sorted.len() - 1)])
    }

    let g_250_samples = collect_g_post(250);
    let g_500_samples = collect_g_post(500);
    let g_smoothed_250ms_post = mean_of(&g_250_samples);
    let g_median_post_500ms = percentile_of(&g_500_samples, 0.5);
    let g_p95_post_500ms = percentile_of(&g_500_samples, 0.95);

    // Max Gear-Normal-Force im Post-Edge-Fenster (Strut-Compression-Mass).
    // gear_normal_force_n ist Option<f32> (X-Plane liefert es, MSFS None).
    let max_gear_force_n: Option<f32> = {
        let hi = edge_ms + 1000; // 1 s nach Edge
        let mut peak = 0.0_f32;
        let mut found = false;
        for s in samples {
            let ts = s.at.timestamp_millis();
            if ts >= edge_ms && ts <= hi {
                if let Some(f) = s.gear_normal_force_n {
                    if f > peak {
                        peak = f;
                        found = true;
                    }
                }
            }
        }
        if found {
            Some(peak)
        } else {
            None
        }
    };

    // --- Bounce-Profile ---
    // Post-TD: zähle on_ground=False Excursionen mit AGL-Höhe.
    //
    // v0.7.6 P1-2: Zwei Counts werden parallel gefuehrt damit Forensik
    // (kleine Hopser sichtbar) und Score (nur "echte" Bounces bestraft)
    // dieselbe AGL-Datenbasis aber unterschiedliche Schwellen nutzen.
    // Konstanten in `touchdown_v2.rs`. Spec docs/spec/v0.7.6-landing-
    // payload-consistency.md §3 P1-2.
    // ⚠ Die Sprunghoehe zaehlt RELATIV zur Bodenhoehe des Musters.
    //
    // # Der Fehler, den das behebt (THY42, FALE 24, 30.08.2026)
    //
    // Hier stand `current_bounce_max = s.agl_ft` — die ABSOLUTE Hoehe.
    // Das setzt voraus, dass ein Flugzeug am Boden 0 ft AGL meldet. Der
    // Simulator misst AGL aber vom Modellursprung, nicht von den Raedern:
    // Eine A350 auf dem Fahrwerk meldet **17,8 ft**. Damit lag sie
    // dauerhaft ueber der gewerteten Schwelle (15 ft), und jedes Flattern
    // des Bodenkontakt-Schalters wurde zum gewerteten Sprung.
    //
    // Gemessen an dem Flug: zwei "Spruenge" von 0,16 s und 0,04 s, dabei
    // aenderte sich die Hoehe um 0,01 ft und die G-Kraft blieb bei 0,97.
    // Aufgesetzt wurde mit 171 fpm und 1,16 G — fuer einen Sprung auf 18
    // Fuss fehlt die Energie um ein Vielfaches. Die drei ECHTEN Spruenge
    // im Bestand haben 2,8 bis 3,9 G.
    //
    // Der Fehler trifft gezielt Grossraumflugzeuge: Bei kleinerem Geraet
    // liegt der Modellursprung nahe am Fahrwerk und AGL am Boden bei
    // 0-5 ft, unter der Schwelle. Deshalb ist er nie aufgefallen.
    //
    // Bezugshoehe ist die AGL beim Aufsetzen — der Moment, in dem das
    // Flugzeug nachweislich am Boden war.
    let boden_agl_ft: f32 = samples
        .iter()
        .find(|s| s.at.timestamp_millis() >= edge_ms && s.on_ground)
        .map(|s| s.agl_ft)
        .unwrap_or(0.0);

    let mut forensic_bounces = 0_u32;
    let mut scored_bounces = 0_u32;
    let mut scored_light_bounces = 0_u32;
    let mut bounce_max_agl: f32 = 0.0;
    let mut in_bounce = false;
    let mut current_bounce_max: f32 = 0.0;
    let mut bounce_start_ms: i64 = 0;
    for s in samples {
        let ts = s.at.timestamp_millis();
        if ts < edge_ms {
            continue;
        }
        // Hoehe UEBER dem Boden, nicht ueber dem Gelaende.
        let hoehe = (s.agl_ft - boden_agl_ft).max(0.0);
        if !s.on_ground {
            if !in_bounce {
                in_bounce = true;
                bounce_start_ms = ts;
                current_bounce_max = hoehe;
            } else if hoehe > current_bounce_max {
                current_bounce_max = hoehe;
            }
        } else if in_bounce {
            // ⚠ Zweiter Riegel: die DAUER. Ein echter Sprung dauert
            // merklich; ein flatternder Schalter ist nach Millisekunden
            // vorbei. Bei THY42 waren es 40 und 160 ms.
            let dauer_ms = ts - bounce_start_ms;
            if dauer_ms >= crate::grenzwerte::grenzwerte().landung.bounce_min_dauer_ms.wert {
                if current_bounce_max >= crate::grenzwerte::grenzwerte().aufsetzen.bounce_forensic_min_agl_ft.wert {
                    forensic_bounces += 1;
                    if current_bounce_max > bounce_max_agl {
                        bounce_max_agl = current_bounce_max;
                    }
                }
                if current_bounce_max >= crate::grenzwerte::grenzwerte().aufsetzen.bounce_scored_min_agl_ft.wert {
                    scored_bounces += 1;
                } else if dauer_ms >= crate::grenzwerte::grenzwerte().landung.bounce_light_min_dauer_ms.wert
                    && current_bounce_max >= crate::grenzwerte::grenzwerte().landung.bounce_light_min_agl_ft.wert
                {
                    // P1/8b: leichter Hopser — lange in der Luft, aber unter
                    // der Hopser-Schwelle. Nie zusaetzlich zu einem normalen.
                    scored_light_bounces += 1;
                }
            }
            in_bounce = false;
            current_bounce_max = 0.0;
        }
    }

    let pre_count = samples
        .iter()
        .filter(|s| s.at.timestamp_millis() < edge_ms)
        .count();
    let post_count = samples.len() - pre_count;

    // v0.12.3 (LE1–LE3): FOQA-konformer gescorter G-Wert — EMA-geglätteter
    // Fenster-Peak statt rohem 50-Hz-Einzelframe-Peak. Einzige
    // Berechnungsstelle (LE8); alle Consumer lesen `scored_g` aus dieser
    // Analyse-Map.
    let scored = recorder::compute_scored_g(samples, edge_at);

    let mut analysis = json!({
        "vs_at_edge_fpm": vs_at_edge,
        // Forensik: beide Kandidaten getrennt, damit jede Landung
        // nachträglich mit beiden Verfahren nachgerechnet werden kann —
        // das ersetzt ein Rollback-Werkzeug.
        "vs_geometrie_fpm": vs_geometrie,
        // v1.6.9 — die Bestandteile der gemessenen Sinkrate. Positiver
        // Gelaendewert = der Boden steigt dem Flugzeug entgegen und macht
        // die gemessene Rate haerter, als der Pilot geflogen ist.
        // `vs_eigensinken_fpm` ist die Bewegung des Flugzeugs gegen den
        // Meeresspiegel, also ohne Gelaendeanteil.
        "vs_gelaende_fpm": vs_gelaende,
        "vs_eigensinken_fpm": vs_eigensinken,
        // Die Referenz des Simulators (MSFS `PLANE TOUCHDOWN NORMAL
        // VELOCITY`) — reine Gegenprobe, geht in keine Bewertung ein.
        // Ueber 537 Landungen ohne Hopser: unsere Hoehenkurve liegt im
        // Median +4 fpm daneben, 45 % innerhalb 50 fpm, 70 % innerhalb
        // 100 fpm. X-Plane liefert nichts Vergleichbares.
        "vs_sim_referenz_fpm": sim_referenz,
        // Ohne Stillstands-Riegel — Sicherheitsnetz der Unfall-Erkennung
        // und zugleich die Gegenprobe, mit der sich eine verworfene
        // Hoehenkurve nachtraeglich noch nachrechnen laesst.
        "vs_geometrie_ungefiltert_fpm": vs_geometrie_ungefiltert,
        "vs_simvar_edge_fpm": vs_aus_simvar,
        "vs_at_edge_quelle": if vs_geometrie.is_some() { "hoehenkurve" } else { "simvar_fallback" },
        "vs_smoothed_250ms_fpm": vs_250,
        "vs_smoothed_500ms_fpm": vs_500,
        "vs_smoothed_1000ms_fpm": vs_1000,
        "vs_smoothed_1500ms_fpm": vs_1500,
        "peak_g_post_500ms": pg_500,
        "peak_g_post_1000ms": pg_1000,
        // v0.12.3 (LE1): EMA-geglätteter Scored-G (FOQA-Methode) — der
        // Wert, auf dem die Landung gescort wird. `peak_g_post_*` bleibt
        // der rohe Forensik-Peak.
        "scored_g": scored.scored_g,
        "scored_g_method": scored.method.as_str(),
        // v0.7.17 (B-009): G-Force-Forensik
        "g_at_edge": g_at_edge,
        // Lernpaket AP1a (29.09.2026): zweiter MSFS-G-Kanal zum Vergleich.
        // Versatz in ms relativ zur Kante (negativ = davor). Nur Forensik.
        "g_force_spitze": g_force_spitze.map(|(g, _)| g),
        "g_force_spitze_versatz_ms": g_force_spitze.map(|(_, dt)| dt),
        "semibody_g_spitze": semibody_spitze.map(|(g, _)| g),
        "semibody_g_spitze_versatz_ms": semibody_spitze.map(|(_, dt)| dt),
        "semibody_g_peak_post_500ms": semibody_500,
        "semibody_g_peak_post_1000ms": semibody_1000,
        "g_smoothed_250ms_post": g_smoothed_250ms_post,
        "g_median_post_500ms": g_median_post_500ms,
        "g_p95_post_500ms": g_p95_post_500ms,
        "max_gear_force_n": max_gear_force_n,
        "peak_vs_pre_flare_fpm": peak_vs_pre,
        "vs_at_flare_end_fpm": vs_at_flare_end,
        "flare_reduction_fpm": flare_reduction,
        "flare_dvs_dt_fpm_per_sec": flare_dvs_dt,
        "flare_quality_score": flare_score,
        "flare_detected": flare_detected,
        // v0.7.6 P1-2: bounce_count BLEIBT auf forensic count fuer
        // Backward-Compat (alte Reader/Replays). Neuer Konsument soll
        // explizit forensic_bounce_count oder scored_bounce_count lesen.
        "bounce_count": forensic_bounces,
        "forensic_bounce_count": forensic_bounces,
        "scored_bounce_count": scored_bounces,
        "scored_light_bounce_count": scored_light_bounces,
        "bounce_max_agl_ft": if bounce_max_agl > 0.0 { Some(bounce_max_agl) } else { None },
        "sample_count": samples.len(),
        "pre_edge_sample_count": pre_count,
        "post_edge_sample_count": post_count,
    });

    // v0.16.22: Herkunft der MSFS-Abfang-Endpunkte — nur bei MSFS, bei
    // X-Plane / Other fehlen die Schluessel wie bisher. Dazu, wer gemessen
    // hat (Paketmodul, nachgeholter Stand oder nativ) und mit welcher Fassung.
    if let Some(obj) = analysis.as_object_mut() {
        if let Some(h) = &messung.msfs {
            obj.insert("flare_vs_source".into(), json!(h.flare_vs_source));
            obj.insert("peak_vs_pre_flare_fpm_raw".into(), json!(h.peak_vs_pre_flare_fpm_raw));
            obj.insert("vs_at_flare_end_fpm_raw".into(), json!(h.vs_at_flare_end_fpm_raw));
        }
        obj.insert("messung_quelle".into(), json!(messung_quelle));
        obj.insert("messung_fassung".into(), json!(messung.messung_fassung));
    }
    analysis
}
