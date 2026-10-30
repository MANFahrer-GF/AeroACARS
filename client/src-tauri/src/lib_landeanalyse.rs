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
fn msfs_flare_agl_max_ft() -> f32 {
    crate::grenzwerte::grenzwerte().landung.td_agl_max_at_window_start_ft.wert
}

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

/// Pitch-corrected AGL-geometric V/S (fpm, negative = descending) at the
/// time point `t_ms`, via a least-squares fit of `agl_ft` over the
/// airborne samples in `[t_ms ± MSFS_FLARE_AGL_FIT_HALF_WINDOW_MS]`.
///
/// `fit_hi_cap`: when `Some(cap)`, no sample at or after `cap` ms enters the
/// fit. v0.16.22 hardening: the end point (nearest flare_hi = edge−100 ms)
/// would otherwise reach edge+200 ms; on a touchdown-then-bounce the
/// post-edge airborne balloon has RISING AGL, which flattens/reverses the
/// end slope → an inflated reduction → phantom. Capping at `edge_ms` keeps
/// the end fit on the genuine pre-touchdown descent (a firm landing's
/// post-edge samples are on_ground and already excluded, so it is
/// unaffected). Pass `None` for an uncapped fit.
///
/// Lag-free: AGL/position is not the lagged display-VSI SimVar. Returns
/// `None` when fewer than `MSFS_FLARE_AGL_FIT_MIN_SAMPLES` qualifying
/// samples exist, the AGL is above `MSFS_FLARE_AGL_MAX_FT` at the point,
/// or the time spread is degenerate (all samples share one timestamp).
/// Pure + side-effect-free → unit-testable on every platform.
fn agl_geometric_vs_fpm_at(
    samples: &[TouchdownWindowSample],
    t_ms: i64,
    pitch_deg_at_t: f32,
    fit_hi_cap: Option<i64>,
) -> Option<f32> {
    let lo = t_ms - crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_half_window_ms.wert;
    let hi = t_ms + crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_half_window_ms.wert;
    // Least-squares slope of agl_ft (y) over time-offset in ms (x,
    // centred on t_ms so the numbers stay small). Only airborne samples
    // near the ground — a high sample would be pre-flare descent.
    let mut n = 0_usize;
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    let mut sxx = 0.0_f64;
    let mut sxy = 0.0_f64;
    for s in samples {
        let ts = s.at.timestamp_millis();
        if ts < lo || ts > hi || s.on_ground {
            continue;
        }
        // Do not let the fit cross the touchdown moment (post-edge balloon
        // would corrupt the end slope on a bounce).
        if let Some(cap) = fit_hi_cap {
            if ts >= cap {
                continue;
            }
        }
        if s.agl_ft > msfs_flare_agl_max_ft() || !s.agl_ft.is_finite() {
            continue;
        }
        let x = (ts - t_ms) as f64; // ms, centred
        let y = s.agl_ft as f64; // ft
        n += 1;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    if n < crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_min_samples.wert {
        return None;
    }
    let nf = n as f64;
    let denom = nf * sxx - sx * sx;
    if denom.abs() < 1e-9 {
        return None; // all samples at one timestamp → no slope
    }
    let slope_ft_per_ms = (nf * sxy - sx * sy) / denom;
    let fpm = slope_ft_per_ms * 1000.0 * 60.0; // ft/ms → ft/min
    if !fpm.is_finite() {
        return None;
    }
    // Pitch-correct (× cos(pitch)) — mirrors the SimVar flare path so
    // the metric stays consistent across sources. Tiny here but correct.
    let pitch_rad = (pitch_deg_at_t as f64) * std::f64::consts::PI / 180.0;
    Some((fpm * pitch_rad.cos()) as f32)
}

/// The AGL-derived flare endpoints for MSFS: the steepest (most
/// negative) AGL-geometric V/S anywhere in `[flare_lo, flare_hi]` and
/// the AGL-geometric V/S at the sample nearest `flare_hi` (-100 ms).
/// Both already pitch-corrected. Returns `(peak_vs_pre, vs_at_flare_
/// end)`; either may be `None` if no fit point qualified.
fn compute_msfs_agl_flare_endpoints(
    samples: &[TouchdownWindowSample],
    flare_lo: i64,
    flare_hi: i64,
) -> (Option<f32>, Option<f32>) {
    let mut peak_vs_pre: Option<f32> = None; // most negative AGL slope
    let mut vs_at_flare_end: Option<f32> = None;
    let mut min_dist_to_end = i64::MAX;
    // v0.16.22 hardening: cap every endpoint fit at the touchdown edge so a
    // post-edge bounce/balloon (rising AGL after first ground contact) never
    // contaminates the end slope. flare_hi == edge_ms − 100, so edge_ms is
    // flare_hi + 100; samples at/after that never enter a fit.
    let edge_cap = flare_hi + 100;
    for s in samples {
        let ts = s.at.timestamp_millis();
        if ts < flare_lo || ts > flare_hi || s.on_ground {
            continue;
        }
        let vs_agl = match agl_geometric_vs_fpm_at(samples, ts, s.pitch_deg, Some(edge_cap)) {
            Some(v) => v,
            None => continue,
        };
        if peak_vs_pre.map(|p| vs_agl < p).unwrap_or(true) {
            peak_vs_pre = Some(vs_agl);
        }
        let dist_to_end = (flare_hi - ts).abs();
        if dist_to_end < min_dist_to_end {
            min_dist_to_end = dist_to_end;
            vs_at_flare_end = Some(vs_agl);
        }
    }
    (peak_vs_pre, vs_at_flare_end)
}

/// v0.16.22 hardening: outcome of the phantom-flare guards over the MSFS
/// flare window. `agl_reliable` is false when Gate 1 (data quality) OR
/// Gate 2 (bounce) tripped → the caller MUST fall back to the SimVar path
/// so no garbage AGL endpoint is published. `unreliable_source` is true
/// only when a DATA-QUALITY gate tripped (frozen/degraded AGL) so the
/// provenance string can distinguish that from a plain non-MSFS fallback.
/// `pitch_nose_down` is the Gate-3 veto signal (a strongly nose-down net
/// pitch trend) — it does NOT block the AGL endpoints (the reduction is
/// still computed for forensics), only `flare_detected`.
struct MsfsFlareGates {
    agl_reliable: bool,
    unreliable_source: bool,
    pitch_nose_down: bool,
}

/// Evaluate the v0.16.22 phantom-flare guards over `[flare_lo, flare_hi]`.
/// Pure + side-effect-free → unit-testable. See the gate constants above
/// for the data behind each threshold.
fn evaluate_msfs_flare_gates(
    samples: &[TouchdownWindowSample],
    flare_lo: i64,
    flare_hi: i64,
) -> MsfsFlareGates {
    // Collect the in-window samples (airborne + ground) in time order.
    let mut in_window: Vec<&TouchdownWindowSample> = samples
        .iter()
        .filter(|s| {
            let ts = s.at.timestamp_millis();
            ts >= flare_lo && ts <= flare_hi
        })
        .collect();
    in_window.sort_by_key(|s| s.at.timestamp_millis());

    // ── Gate 2 (bounce, part A): any on_ground=true in the PRE-edge window
    // means a touchdown already happened here — a post-bounce settle, not a
    // flare. (Kept pre-edge: EVERY normal landing is on_ground just past the
    // edge — that is the touchdown itself, not a prior bounce. Extending this
    // to post-edge would wrongly trip on all firm landings; the post-edge
    // bounce signal is the BALLOON below, not on_ground.)
    let any_on_ground = in_window.iter().any(|s| s.on_ground);

    // ── Gate 2 (bounce) extended balloon-scan window. v0.16.22 hardening: a
    // touchdown-then-bounce contacts the ground AT/near the edge, then goes
    // airborne again just past it with RISING AGL. A NORMAL landing has no
    // airborne samples post-edge (it is on the ground), so extending only the
    // BALLOON (non-monotone rise among airborne samples) to flare_hi + the
    // fit half-window (= edge + 200 ms) catches the bounce balloon WITHOUT
    // tripping firm landings. (The fit endpoints are already capped at the
    // edge — this gate is the corroborating veto so a bounce earns no
    // credited flare. Data-quality gates 1a/1b/1c stay on the pre-edge set.)
    let bounce_scan_hi = flare_hi + crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_half_window_ms.wert;

    // Airborne, finite-AGL, near-ground samples in the FLARE window drive the
    // data-quality checks (mirrors the slope fit's qualifying set).
    let air: Vec<&TouchdownWindowSample> = in_window
        .iter()
        .copied()
        .filter(|s| !s.on_ground && s.agl_ft.is_finite() && s.agl_ft <= msfs_flare_agl_max_ft())
        .collect();
    // Airborne set over the EXTENDED balloon-scan window (pre-edge flare
    // window + post-edge fit reach) — used ONLY for the balloon (non-monotone
    // rise) check so a post-edge bounce balloon is visible.
    let mut air_bounce: Vec<&TouchdownWindowSample> = samples
        .iter()
        .filter(|s| {
            let ts = s.at.timestamp_millis();
            ts >= flare_lo
                && ts <= bounce_scan_hi
                && !s.on_ground
                && s.agl_ft.is_finite()
                && s.agl_ft <= msfs_flare_agl_max_ft()
        })
        .collect();
    air_bounce.sort_by_key(|s| s.at.timestamp_millis());

    // ── Gate 1a (data quality): distinct agl_ft count. Quantize to 0.01 ft
    // so float jitter does not inflate the count (the frozen E195 repeats
    // EXACTLY, e.g. 9.96, so quantization is conservative here).
    let mut distinct: Vec<i64> = air
        .iter()
        .map(|s| (s.agl_ft as f64 * 100.0).round() as i64)
        .collect();
    distinct.sort_unstable();
    distinct.dedup();
    let n_distinct = distinct.len();

    // ── Gate 1c (data quality, LOCAL): distinct agl_ft in the flare-END
    // fit window only — catches a freeze that corrupts the end slope while
    // the rest of the window is fine (Gate 1a's whole-window count misses
    // a localized mid/late freeze). Same quantization as Gate 1a.
    let end_lo = flare_hi - crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_half_window_ms.wert;
    let end_hi = flare_hi + crate::grenzwerte::grenzwerte().landung.msfs_flare_agl_fit_half_window_ms.wert;
    let end_agls: Vec<f32> = air
        .iter()
        .filter(|s| {
            let ts = s.at.timestamp_millis();
            ts >= end_lo && ts <= end_hi
        })
        .map(|s| s.agl_ft)
        .collect();
    let mut end_distinct: Vec<i64> = end_agls
        .iter()
        .map(|a| (*a as f64 * 100.0).round() as i64)
        .collect();
    end_distinct.sort_unstable();
    end_distinct.dedup();
    // Only judge the end window if it actually has samples (an empty end
    // window cannot be "frozen" — let the MIN_SAMPLES fit guard handle it).
    let n_end_distinct = end_distinct.len();
    // Gate 1c backstop: the AGL SPAN across the end-fit window. A NOISY
    // freeze dithers across ≥MSFS_FLARE_MIN_END_FIT_DISTINCT_AGL quantized
    // buckets (passing the distinct count) while spanning ~0 true height —
    // the span catches that where bucket-counting cannot.
    let end_agl_span = if end_agls.len() >= 2 {
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for &a in &end_agls {
            if a < lo {
                lo = a;
            }
            if a > hi {
                hi = a;
            }
        }
        Some(hi - lo)
    } else {
        None
    };

    // ── Gate 1b (data quality): median sample Δt over the FULL in-window
    // set (airborne + ground) — a degraded stream is degraded regardless of
    // on_ground. Median is robust to the occasional duplicate-timestamp.
    let mut dts: Vec<i64> = in_window
        .windows(2)
        .map(|w| w[1].at.timestamp_millis() - w[0].at.timestamp_millis())
        .filter(|&d| d >= 0)
        .collect();
    let median_dt = if dts.is_empty() {
        0
    } else {
        dts.sort_unstable();
        dts[dts.len() / 2]
    };

    // ── Gate 2 (bounce, part B): non-monotone balloon. Walk the airborne
    // AGL trace (over the EXTENDED bounce-scan window so a post-edge balloon
    // is visible) tracking the running minimum; if AGL rises more than the
    // tolerance above that minimum, the trace ballooned back up.
    let mut balloon_ft = 0.0_f32;
    let mut running_min = f32::INFINITY;
    for s in &air_bounce {
        if s.agl_ft < running_min {
            running_min = s.agl_ft;
        } else {
            let rise = s.agl_ft - running_min;
            if rise > balloon_ft {
                balloon_ft = rise;
            }
        }
    }

    // Frozen end if EITHER the distinct-bucket count is too low (coarse/
    // stepped freeze) OR the AGL span across the end window is below the
    // physics floor (noisy multi-bucket freeze on a flat plateau). The span
    // backstop is what catches a dither that spreads across ≥4 buckets yet
    // covers near-zero true height (vs_at_flare_end ≈ 0 → fake reduction).
    let end_span_flat = end_agl_span
        .map(|sp| sp < crate::grenzwerte::grenzwerte().landung.msfs_flare_min_end_agl_span_ft.wert)
        .unwrap_or(false);
    let end_frozen = (n_end_distinct > 0 && n_end_distinct < crate::grenzwerte::grenzwerte().landung.msfs_flare_min_end_fit_distinct_agl.wert)
        || end_span_flat;
    let dq_reject = n_distinct < crate::grenzwerte::grenzwerte().landung.msfs_flare_min_distinct_agl.wert
        || median_dt > crate::grenzwerte::grenzwerte().landung.msfs_flare_max_median_dt_ms.wert
        || end_frozen;
    let bounce_reject = any_on_ground || balloon_ft > crate::grenzwerte::grenzwerte().landung.msfs_flare_balloon_tol_ft.wert;

    // ── Gate 3 (pitch corroboration): net window pitch trend. Compare the
    // mean pitch over the first ~300 ms of airborne samples to the mean over
    // the last ~300 ms. A strongly NOSE-DOWN trend vetoes detection.
    let pitch_nose_down = if air.len() >= 2 {
        let first_t = air.first().unwrap().at.timestamp_millis();
        let last_t = air.last().unwrap().at.timestamp_millis();
        let mean_pitch = |pred: &dyn Fn(i64) -> bool| -> Option<f32> {
            let vals: Vec<f32> = air
                .iter()
                .filter(|s| pred(s.at.timestamp_millis()))
                .map(|s| s.pitch_deg)
                .collect();
            if vals.is_empty() {
                None
            } else {
                Some(vals.iter().sum::<f32>() / vals.len() as f32)
            }
        };
        let p_start = mean_pitch(&|t| t <= first_t + 300);
        let p_end = mean_pitch(&|t| t >= last_t - 300);
        match (p_start, p_end) {
            (Some(a), Some(b)) => (b - a) <= crate::grenzwerte::grenzwerte().landung.msfs_flare_pitch_nosedown_veto_deg.wert,
            _ => false,
        }
    } else {
        false
    };

    MsfsFlareGates {
        agl_reliable: !(dq_reject || bounce_reject),
        unreliable_source: dq_reject,
        pitch_nose_down,
    }
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
fn compute_landing_analysis(
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
    let agl_fenster_ms: i64 = crate::grenzwerte::grenzwerte().aufsetzen.luecken_fenster_vor_ms.wert;
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
    let vs_aus_hoehenkurve_mit = |auch_bei_stillstand: bool| -> Option<f32> {
        let lo = edge_ms - agl_fenster_ms;
        // `!s.on_ground` ist nicht kosmetisch: der Kontakt-Frame traegt schon
        // die beginnende Fahrwerkskompression und bei MSFS zusaetzlich einen
        // AGL-Sprung (im Fixture a320_vrnevnl 1,49 ft in 32 ms — das 17-fache
        // der echten Bewegung). Als Endpunkt einer Zweipunkt-Steigung hebt
        // genau dieser eine Frame das Ergebnis um 158 fpm an. `mean_vs_window`
        // direkt darueber und `agl_geometric_vs_fpm_at` filtern aus demselben
        // Grund; der gescorte Wert darf nicht schwaecher geschuetzt sein als
        // die forensische Anzeige.
        let fenster: Vec<&TouchdownWindowSample> = samples
            .iter()
            .filter(|s| {
                let ts = s.at.timestamp_millis();
                ts >= lo && ts <= edge_ms && s.agl_ft.is_finite() && !s.on_ground
            })
            .collect();
        if fenster.len() < crate::grenzwerte::grenzwerte().landung.agl_min_samples.wert {
            return None;
        }
        let verschieden = fenster
            .iter()
            .map(|s| (s.agl_ft * 1000.0).round() as i64)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if verschieden < crate::grenzwerte::grenzwerte().landung.agl_min_verschiedene.wert {
            return None;
        }
        // Laengster Stillstand der Hoehe, in Millisekunden.
        //
        // Die Zeitrechnung erledigt nebenbei ein Aufzeichnungs-Artefakt:
        // X-Plane liefert vereinzelt jeden Messpunkt zwei- bis dreifach
        // mit identischem Zeitstempel. Ein Zaehler gleicher Werte haette
        // das als Stillstand gelesen und ausgerechnet die dichtesten
        // Aufzeichnungen verworfen — bei gleicher Uhrzeit ist die
        // Stillstandsdauer aber schlicht null. Eine eigene Entdopplung
        // waere toter Code (QS-Befund Runde 4: sie liess sich entfernen,
        // ohne dass ein Test rot wurde).
        let laengster_stillstand_ms = {
            let mut best = 0i64;
            let mut start = 0usize;
            for i in 1..fenster.len() {
                let gleich = ((fenster[i].agl_ft - fenster[i - 1].agl_ft).abs() * 1000.0).round()
                    as i64
                    == 0;
                if gleich {
                    best = best.max(
                        fenster[i].at.timestamp_millis() - fenster[start].at.timestamp_millis(),
                    );
                } else {
                    start = i;
                }
            }
            best
        };
        let eingefroren = laengster_stillstand_ms > crate::grenzwerte::grenzwerte().landung.agl_max_stillstand_ms.wert;
        if eingefroren && !auch_bei_stillstand {
            return None;
        }
        let t0 = fenster.first()?.at.timestamp_millis();
        let spanne = (fenster.last()?.at.timestamp_millis() - t0) as f64 / 1000.0;
        if spanne < crate::grenzwerte::grenzwerte().landung.agl_min_spanne_s.wert {
            return None;
        }
        // **Ausgleichsgerade, nicht Endpunkt-Differenz.** Eine Zweipunkt-
        // Steigung haengt komplett an zwei von ~15 Messpunkten: ein einziger
        // Quantisierungsschritt (~0,5 ft) am Rand verschiebt das Ergebnis um
        // ~97 fpm, also mehr als eine halbe Bandbreite der Landerate-Achse.
        // Am Korpus gegengerechnet liest die Endpunkt-Methode in 12 von 15
        // Aufzeichnungen haerter als die Gerade, im Median 25 fpm, im
        // Extremfall 158 fpm. Die Gerade verduennt denselben Randfehler auf
        // etwa ein Sechstel. Der forensische Flare-Pfad rechnet seit v0.16.22
        // aus genau diesem Grund ebenfalls mit Ausgleich.
        let n = fenster.len() as f64;
        let ts: Vec<f64> = fenster
            .iter()
            .map(|s| (s.at.timestamp_millis() - t0) as f64 / 1000.0)
            .collect();
        let ys: Vec<f64> = fenster.iter().map(|s| s.agl_ft as f64).collect();
        let t_mittel = ts.iter().sum::<f64>() / n;
        let y_mittel = ys.iter().sum::<f64>() / n;
        let zaehler: f64 = ts
            .iter()
            .zip(ys.iter())
            .map(|(t, y)| (t - t_mittel) * (y - y_mittel))
            .sum();
        let nenner: f64 = ts.iter().map(|t| (t - t_mittel).powi(2)).sum();
        // Nach dem Spannen-Riegel liegt der Nenner bei mindestens rund
        // 0,02 — diese Pruefung kann also gar nicht mehr greifen. Sie
        // bleibt als Division-durch-null-Riegel stehen, falls jemand
        // AGL_MIN_SPANNE_S spaeter senkt.
        if nenner <= f64::EPSILON {
            return None;
        }
        let fpm = (zaehler / nenner * 60.0) as f32;
        // Nur ein SINKEN ist eine Landerate; ein Steigen im Fenster heisst,
        // dass der Aufsetzpunkt nicht sauber erfasst wurde.
        if !fpm.is_finite() || !(crate::grenzwerte::grenzwerte().landung.agl_max_plausibel_fpm.wert..0.0).contains(&fpm) {
            return None;
        }
        Some(fpm)
    };
    let vs_aus_hoehenkurve = || vs_aus_hoehenkurve_mit(false);
    // Finde das Sample-Paar (last airborne, first on_ground) das den Edge
    // umschließt; lineare Interpolation der VS auf edge_ms.
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
    let vs_aus_simvar = match (last_air, first_ground) {
        (Some(a), Some(g)) => {
            let t_a = a.at.timestamp_millis() as f64;
            let t_g = g.at.timestamp_millis() as f64;
            let t_edge = edge_ms as f64;
            if (t_g - t_a).abs() < 1e-6 {
                Some(a.vs_fpm)
            } else {
                let alpha = ((t_edge - t_a) / (t_g - t_a)).clamp(0.0, 1.0);
                Some((a.vs_fpm as f64 + (g.vs_fpm as f64 - a.vs_fpm as f64) * alpha) as f32)
            }
        }
        _ => None,
    };
    // Höhenkurve führt; der Sim-Messwert bleibt als Rückfallebene für die
    // 0,5 % der Landungen mit unbrauchbarer Höhenspur — und als Forensik.
    let vs_geometrie = vs_aus_hoehenkurve();
    // Fuer die Unfall-Erkennung: dieselbe Rechnung, aber ohne den
    // Stillstands-Riegel. Sie nimmt den haertesten aller Messwerte —
    // ein zusaetzlicher, moeglicherweise zu weicher Kandidat kann ihr
    // nicht schaden, ein FEHLENDER harter dagegen schon.
    let vs_geometrie_ungefiltert = vs_aus_hoehenkurve_mit(true);
    let vs_at_edge = vs_geometrie.or(vs_aus_simvar);

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
    let steigung_ueber_fenster = |wert: &dyn Fn(&TouchdownWindowSample) -> Option<f32>,
                                  bewegung_pruefen: bool|
     -> Option<f32> {
        let lo = edge_ms - agl_fenster_ms;
        let punkte: Vec<(f64, f64)> = samples
            .iter()
            .filter(|s| {
                let ts = s.at.timestamp_millis();
                ts >= lo && ts <= edge_ms && !s.on_ground
            })
            .filter_map(|s| {
                let y = wert(s)?;
                if !y.is_finite() {
                    return None;
                }
                Some((s.at.timestamp_millis() as f64 / 1000.0, y as f64))
            })
            .collect();
        if punkte.len() < crate::grenzwerte::grenzwerte().landung.agl_min_samples.wert {
            return None;
        }
        let spanne = punkte.last()?.0 - punkte.first()?.0;
        if spanne < crate::grenzwerte::grenzwerte().landung.agl_min_spanne_s.wert {
            return None;
        }
        // Dieselben Riegel wie die Hoehenkurve selbst — eine eingefrorene
        // Spur liest sich sonst als „kein Sinkflug" und schiebt die ganze
        // Bewegung dem jeweils anderen Anteil zu. Der Vergleich mit dem
        // Werkzeug von Arderos hat die Luecke aufgedeckt: es verlangt
        // ausdruecklich fuenf UNTERSCHIEDLICHE Zeitstempel, meine erste
        // Fassung zaehlte nur Messpunkte.
        let verschiedene = punkte
            .iter()
            .map(|p| (p.1 * 1000.0).round() as i64)
            .collect::<std::collections::BTreeSet<_>>()
            .len();
        if bewegung_pruefen && verschiedene < crate::grenzwerte::grenzwerte().landung.agl_min_verschiedene.wert {
            return None;
        }
        let mut laengster_stillstand_ms = 0_i64;
        let mut lauf_start = punkte[0].0;
        for paar in punkte.windows(2) {
            if (paar[1].1 - paar[0].1).abs() < 1e-4 {
                let dauer = ((paar[1].0 - lauf_start) * 1000.0).round() as i64;
                laengster_stillstand_ms = laengster_stillstand_ms.max(dauer);
            } else {
                lauf_start = paar[1].0;
            }
        }
        if bewegung_pruefen && laengster_stillstand_ms > crate::grenzwerte::grenzwerte().landung.agl_max_stillstand_ms.wert {
            return None;
        }
        let n = punkte.len() as f64;
        let mx = punkte.iter().map(|p| p.0).sum::<f64>() / n;
        let my = punkte.iter().map(|p| p.1).sum::<f64>() / n;
        let sxx: f64 = punkte.iter().map(|p| (p.0 - mx).powi(2)).sum();
        if sxx < 1e-9 {
            return None;
        }
        let sxy: f64 = punkte.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let fpm = (sxy / sxx) * 60.0;
        if fpm.is_finite() {
            Some(fpm as f32)
        } else {
            None
        }
    };
    // Nur wenn die Hoehenkurve selbst getragen hat — sonst waere die
    // Aufschluesselung eine Erklaerung fuer eine Zahl, die gar nicht von
    // ihr stammt.
    //
    // **Vorzeichen.** `vs_gelaende_fpm` ist der BEITRAG des Gelaendes zur
    // gemessenen Zahl, nicht die Bewegung des Bodens. Steigt der Boden mit
    // 300 fpm, macht er die gemessene Rate um 300 fpm haerter — der Beitrag
    // ist also −300. Damit gilt immer:
    //
    //     gemessen = Eigensinken + Gelaendebeitrag
    //
    // Diese Form ist die einzige, die sich im Bericht ohne Kopfrechnen
    // lesen laesst, und die einzige, in der eine falsche Richtung sofort
    // auffaellt (die Summe geht dann nicht auf).
    let (vs_gelaende, vs_eigensinken) = if let Some(gemessen) = vs_geometrie {
        let boden_bewegung = steigung_ueber_fenster(
            &|s: &TouchdownWindowSample| s.msl_ft.map(|m| m - s.agl_ft),
            // Ebenes Gelaende steht legitim still (Steigung 0).
            false,
        );
        let msl = steigung_ueber_fenster(&|s: &TouchdownWindowSample| s.msl_ft, true);
        match (boden_bewegung, msl) {
            (Some(boden), Some(eigen)) => {
                // `+ 0.0` macht aus −0,0 (ebener Boden) eine ehrliche 0.
                let gelaende = -boden + 0.0;
                // Die Probe: die Aufschluesselung MUSS die gemessene Zahl
                // ergeben. Geht sie nicht auf, stimmt an einer der drei
                // Groessen etwas nicht (eingefrorene Spur, Gelaendesprung,
                // Ausreisser im Bodenmodell) — dann zeigen wir lieber
                // nichts als eine erfundene Erklaerung. Ein Werkzeug, das
                // dieselbe Rechnung macht, saeubert stattdessen Ausreisser
                // aus der Bodenspur; wir verwerfen, weil eine falsche
                // Erklaerung im Landebericht schlimmer ist als keine.
                // Wert seit v3: bundle/grenzwerte/landung.json -> probe_toleranz_fpm (v2.1.24: 20.0)
                // Ueber 3000 fpm ist es kein Gelaende mehr, sondern ein
                // Sprung im Hoehenbezug — dieselbe Grenze wie bei der
                // Hoehenkurve.
                if (gemessen - (eigen + gelaende)).abs() > crate::grenzwerte::grenzwerte().landung.probe_toleranz_fpm.wert
                    || gelaende.abs() > crate::grenzwerte::grenzwerte().landung.agl_max_plausibel_fpm.wert.abs()
                    || eigen.abs() > crate::grenzwerte::grenzwerte().landung.agl_max_plausibel_fpm.wert.abs()
                {
                    (None, None)
                } else {
                    (Some(gelaende), Some(eigen))
                }
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };

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

    // --- Flare-Analyse ---
    // Window: [-2000 ms, -100 ms] vor Edge. Suche steepste Sinkrate +
    // VS-Wert kurz vor Edge.
    let flare_lo = edge_ms - 2000;
    let flare_hi = edge_ms - 100;
    // SimVar pass (pitch-corrected `s.vs_fpm`). On X-Plane/Other this is
    // the published flare metric; on MSFS it lags the airframe and is
    // kept only as the `_raw` forensic provenance (see v0.16.22 block).
    let mut peak_vs_pre_simvar: Option<f32> = None; // most negative
    let mut vs_at_flare_end_simvar: Option<f32> = None; // VS am ts nahe -100ms
    let mut min_dist_to_end = i64::MAX;
    let mut earliest_in_window: Option<&TouchdownWindowSample> = None;
    for s in samples {
        let ts = s.at.timestamp_millis();
        if ts >= flare_lo && ts <= flare_hi && !s.on_ground {
            // Pitch-korrigierte VS verwenden — bei steilem Flare wichtig
            let pitch_rad = (s.pitch_deg as f64) * std::f64::consts::PI / 180.0;
            let vs_corrected = (s.vs_fpm as f64 * pitch_rad.cos()) as f32;
            if peak_vs_pre_simvar.map(|p| vs_corrected < p).unwrap_or(true) {
                peak_vs_pre_simvar = Some(vs_corrected);
            }
            let dist_to_end = (flare_hi - ts).abs();
            if dist_to_end < min_dist_to_end {
                min_dist_to_end = dist_to_end;
                vs_at_flare_end_simvar = Some(vs_corrected);
            }
            if earliest_in_window
                .map(|e| ts < e.at.timestamp_millis())
                .unwrap_or(true)
            {
                earliest_in_window = Some(s);
            }
        }
    }

    // v0.16.22: MSFS flare metric on the lag-free AGL-geometric V/S.
    // For MSFS2020/2024 fixed-wing, replace the lagged SimVar endpoints
    // with the AGL local-linear-fit slope endpoints over the SAME
    // window; keep the SimVar values as `_raw` provenance. X-Plane (raw
    // local_vy) / Other keep the SimVar endpoints → byte-identical.
    // Heli/seaplane are NOT category-gated here (compute_landing_analysis
    // has no category); we gate on the AGL endpoints existing AND being a
    // real reduction — a near-zero-V/S rotorcraft touchdown produces no
    // AGL flare reduction, so the metric stays inert. Fixed-wing scope is
    // additionally enforced by the de-lag/score paths that consume this.
    let is_msfs = matches!(simulator, Simulator::Msfs2020 | Simulator::Msfs2024);
    let (peak_vs_pre_agl, vs_at_flare_end_agl) = if is_msfs {
        compute_msfs_agl_flare_endpoints(samples, flare_lo, flare_hi)
    } else {
        (None, None)
    };
    // v0.16.22 hardening: evaluate the phantom-flare guards (frozen/coarse
    // AGL, bounce/balloon, nose-down pitch). On a data-quality or bounce
    // reject we fall back to the SimVar path so no garbage AGL endpoint is
    // published; the pitch veto is applied to `flare_detected` only.
    let flare_gates = if is_msfs {
        evaluate_msfs_flare_gates(samples, flare_lo, flare_hi)
    } else {
        MsfsFlareGates {
            agl_reliable: true,
            unreliable_source: false,
            pitch_nose_down: false,
        }
    };
    // The endpoints that actually drive reduction / score / detected.
    // MSFS: AGL if BOTH endpoints resolved AND the guards pass, else fall
    // back to SimVar (too few AGL samples, frozen/coarse trace, or a
    // bounce — never worse than the pre-v0.16.22 SimVar behaviour).
    let use_agl = is_msfs
        && peak_vs_pre_agl.is_some()
        && vs_at_flare_end_agl.is_some()
        && flare_gates.agl_reliable;
    let (peak_vs_pre, vs_at_flare_end, flare_vs_source) = if use_agl {
        (peak_vs_pre_agl, vs_at_flare_end_agl, "msfs_agl")
    } else if is_msfs && flare_gates.unreliable_source {
        // A data-quality gate tripped: mark the fallback distinctly so the
        // provenance shows WHY we did not use the AGL geometry.
        (
            peak_vs_pre_simvar,
            vs_at_flare_end_simvar,
            "simvar_agl_unreliable",
        )
    } else {
        (peak_vs_pre_simvar, vs_at_flare_end_simvar, "simvar")
    };

    // Reduktion: positive Zahl = Flare hat Sinkrate verkleinert
    let flare_reduction = match (peak_vs_pre, vs_at_flare_end) {
        (Some(p), Some(v)) => Some(v - p),
        _ => None,
    };
    // dVS/dt grob: (vs_at_flare_end - vs_at_flare_start) / window_dt.
    // Secondary diagnostic. v0.16.22 FIX 4: route this through the SAME
    // endpoint SOURCE as the published `flare_reduction` so the two
    // diagnostics never disagree. On the MSFS AGL path both start and end are
    // AGL-geometric (start = the capped AGL fit at the earliest in-window
    // sample, end = the published AGL `vs_at_flare_end`); on the SimVar
    // fallback (and X-Plane / Other) both stay SimVar → byte-identical to
    // pre-v0.16.22.
    let flare_dvs_dt = match earliest_in_window {
        Some(start) => {
            let dt_sec = (flare_hi - start.at.timestamp_millis()) as f64 / 1000.0;
            if dt_sec > 0.1 {
                // Pick the start/end pair from the SAME source as the reduction.
                let endpoints = if use_agl {
                    // AGL start at the earliest-window timestamp (edge-capped,
                    // matching compute_msfs_agl_flare_endpoints) + AGL end.
                    let start_vs = agl_geometric_vs_fpm_at(
                        samples,
                        start.at.timestamp_millis(),
                        start.pitch_deg,
                        Some(flare_hi + 100),
                    );
                    match (start_vs, vs_at_flare_end) {
                        (Some(sv), Some(ev)) => Some((sv, ev)),
                        _ => None,
                    }
                } else {
                    // SimVar pass: pitch-corrected vs_fpm start + SimVar end.
                    let pitch_rad = (start.pitch_deg as f64) * std::f64::consts::PI / 180.0;
                    let start_vs = (start.vs_fpm as f64 * pitch_rad.cos()) as f32;
                    vs_at_flare_end_simvar.map(|ev| (start_vs, ev))
                };
                endpoints.map(|(start_vs, end_vs)| ((end_vs - start_vs) as f64 / dt_sec) as f32)
            } else {
                None
            }
        }
        None => None,
    };

    // v0.5.41: rebalanced — Endpoint-Score dominiert (was kommt am TD raus),
    // Reduktion gibt Bonus-Punkte (= rettete eine harte Landung wenn aus
    // hohem VS reduziert wurde). Vorherige Skala bestrafte Piloten die
    // schon mit niedriger VS reinkamen (= eigentlich gute Approaches) zu
    // hart, weil "Reduktion >0 fpm" pauschal nur 20 Punkte gab.
    //
    // Endpoint-Score (= VS unmittelbar vor TD, der eigentliche Touchdown-
    // Indikator):
    //   > -75 fpm:       100 (butter)
    //   > -150 fpm:       80 (smooth)
    //   > -300 fpm:       60 (acceptable)
    //   > -500 fpm:       40 (firm)
    //   sonst:            20 (hard/severe)
    //
    // Reduktions-Bonus (= Flare-Manöver hat eine harte Landung gerettet,
    // der Pilot hat gepulled):
    //   > 400 fpm:       +20
    //   > 200 fpm:       +15
    //   > 100 fpm:       +10
    //   >  50 fpm:        +5
    //   sonst:             0
    //
    // Endpoint + Bonus, gecappt [0..100].
    //
    // Beispiele:
    //   - DLH 1404 (Peter, Fenix A320 SL): vs_end=-61, red=59 → 100+5 = 100 ✓
    //   - Hypothetisch B738 perfect: vs_end=-100, red=600 → 80+20 = 100 ✓
    //   - URO 913 (Aerosoft A346): vs_end=-606 (estimated), red=315 → 20+15 = 35
    //   - Bad Pilot: vs_end=-800, red=0 → 20+0 = 20
    let (flare_score, flare_detected) = match (flare_reduction, vs_at_flare_end) {
        (Some(red), Some(final_vs)) => {
            let red = red as f64;
            let final_vs = final_vs as f64;
            let endpoint: f64 = if final_vs > -75.0 {
                100.0
            } else if final_vs > -150.0 {
                80.0
            } else if final_vs > -300.0 {
                60.0
            } else if final_vs > -500.0 {
                40.0
            } else {
                20.0
            };
            let bonus: f64 = if red > 400.0 {
                20.0
            } else if red > 200.0 {
                15.0
            } else if red > 100.0 {
                10.0
            } else if red > 50.0 {
                5.0
            } else {
                0.0
            };
            let score = (endpoint + bonus).max(0.0).min(100.0) as i32;
            // Detection floor. MSFS_FLARE_DETECT_FLOOR_FPM == 50.0, so the
            // value here is byte-identical to the previous `red > 50.0` for
            // X-Plane / Other. v0.16.22: on the MSFS AGL path the pitch
            // veto (Gate 3) additionally withholds detection when the net
            // pitch trend is strongly nose-down (a settle/shed, not a
            // pull) — the reduction/score still publish for forensics.
            let mut detected = red > crate::grenzwerte::grenzwerte().landung.msfs_flare_detect_floor_fpm.wert;
            if is_msfs && use_agl && flare_gates.pitch_nose_down {
                detected = false;
            }
            (Some(score), Some(detected))
        }
        _ => (None, None),
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

    // v0.16.22: provenance of the MSFS AGL flare endpoints — ADDITIVE,
    // MSFS-ONLY. `flare_vs_source` marks whether the published flare
    // endpoints are the lag-free AGL fit ("msfs_agl") or the SimVar
    // fallback ("simvar"); the `_raw` keys carry the lagged display-VSI
    // values for forensics. On X-Plane / Other we add NOTHING so their
    // analysis JSON is byte-identical to pre-v0.16.22 (no new keys).
    if is_msfs {
        if let Some(obj) = analysis.as_object_mut() {
            obj.insert("flare_vs_source".into(), json!(flare_vs_source));
            obj.insert(
                "peak_vs_pre_flare_fpm_raw".into(),
                json!(peak_vs_pre_simvar),
            );
            obj.insert(
                "vs_at_flare_end_fpm_raw".into(),
                json!(vs_at_flare_end_simvar),
            );
        }
    }
    analysis
}
