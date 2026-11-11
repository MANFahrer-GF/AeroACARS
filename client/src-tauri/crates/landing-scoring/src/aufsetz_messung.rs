//! Aufsetz-Messung im Paketmodul (DLH9901, 11.11.2026).
//!
//! # Warum hier und nicht mehr im Kern
//!
//! Bis Client 3.0.1 rechnete der Kern (`lib_landeanalyse.rs`) die Sinkrate
//! am Aufsetzpunkt selbst aus und gab dem Modul nur die fertige Zahl. Eine
//! Fehlmessung liess sich deshalb nur mit einem neuen Client beheben; im
//! Paket lagen bloss Grenzwerte, und die veraendern Tausende Landungen mit
//! (DLH9901: ein Ruckler vor dem 310-ms-Fenster las −661 statt −204 fpm).
//! Hier steht jetzt die ganze Messung: Hoehengerade mit ihren Riegeln, die
//! ungefilterte Gerade der Unfall-Erkennung, das Instrument am Aufsetzpunkt,
//! die Wahl zwischen beiden, die Gelaende-Aufteilung und die Abfang-
//! Endpunkte. Der Client ruft sie ueber den Export `messe` des Pakets auf;
//! fehlt der Export (aeltere Pakete), rechnet er dieselbe Funktion nativ.
//! Damit geht jede kuenftige Aenderung der Messung mit einer Generation raus.
//!
//! # Vertrag
//!
//! - Zeiten in Millisekunden relativ zum Aufsetzen (`at.timestamp_millis()`
//!   minus Kante, wie bisher im Kern).
//! - Die Proben tragen alle Kanaele des Aufsetzfensters; was diese Fassung
//!   nicht liest, wird ignoriert (kein `deny_unknown_fields`), damit eine
//!   spaetere Fassung weitere Kanaele nutzen kann, ohne dass der Client sich
//!   aendert.
//! - Die Grenzwerte kommen in der Anfrage aus dem Paket
//!   (`grenzwerte/landung.json`, `aufsetzen.json`). Ein NEUER Parameter
//!   bekommt hier einen `serde(default)` und steht nur im Modul — der Client
//!   liest die Grenzwert-Dateien mit `deny_unknown_fields` und wuerde an einem
//!   neuen Feld scheitern.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Fassung der Messregeln. 1 = unveraenderter Umzug aus Client 3.0.1;
/// 2 = Ruckler vor dem Fenster zaehlt (siehe [`REGELN`]).
pub const MESS_FASSUNG: u32 = 2;

/// Eine Probe des Aufsetzfensters.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub struct Probe {
    /// Millisekunden relativ zum Aufsetzen (negativ = davor).
    pub t_ms: i64,
    pub vs_fpm: f32,
    pub g_force: f32,
    pub on_ground: bool,
    pub agl_ft: f32,
    #[serde(default)]
    pub msl_ft: Option<f32>,
    pub pitch_deg: f32,
}

/// Grenzwerte der Messung (Werte aus dem Paket).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Parameter {
    /// `aufsetzen.luecken_fenster_vor_ms` (310).
    pub fenster_ms: i64,
    pub agl_min_samples: usize,
    pub agl_min_verschiedene: usize,
    pub agl_max_stillstand_ms: i64,
    pub agl_min_spanne_s: f64,
    pub agl_max_plausibel_fpm: f32,
    pub probe_toleranz_fpm: f32,
    pub td_agl_max_at_window_start_ft: f32,
    pub msfs_flare_agl_fit_half_window_ms: i64,
    pub msfs_flare_agl_fit_min_samples: usize,
    pub msfs_flare_min_distinct_agl: usize,
    pub msfs_flare_max_median_dt_ms: i64,
    pub msfs_flare_min_end_fit_distinct_agl: usize,
    pub msfs_flare_min_end_agl_span_ft: f32,
    pub msfs_flare_balloon_tol_ft: f32,
    pub msfs_flare_pitch_nosedown_veto_deg: f32,
    pub msfs_flare_detect_floor_fpm: f64,
}

/// Anfrage an den Export `messe`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Anfrage {
    /// `Msfs2020`, `Msfs2024`, `XPlane11`, `XPlane12`, `Other`.
    pub simulator: String,
    pub proben: Vec<Probe>,
    pub parameter: Parameter,
}

/// Herkunft der Abfang-Endpunkte, nur bei MSFS (die Schluessel fehlen bei
/// X-Plane und Other ganz, wie bisher).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MsfsAbfangHerkunft {
    pub flare_vs_source: String,
    pub peak_vs_pre_flare_fpm_raw: Option<f32>,
    pub vs_at_flare_end_fpm_raw: Option<f32>,
}

/// Ergebnis der Messung — dieselben Schluessel wie in `landing_analysis`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Messung {
    pub messung_fassung: u32,
    pub vs_at_edge_fpm: Option<f32>,
    pub vs_at_edge_quelle: String,
    pub vs_geometrie_fpm: Option<f32>,
    pub vs_geometrie_ungefiltert_fpm: Option<f32>,
    pub vs_simvar_edge_fpm: Option<f32>,
    pub vs_gelaende_fpm: Option<f32>,
    pub vs_eigensinken_fpm: Option<f32>,
    pub peak_vs_pre_flare_fpm: Option<f32>,
    pub vs_at_flare_end_fpm: Option<f32>,
    pub flare_reduction_fpm: Option<f32>,
    pub flare_dvs_dt_fpm_per_sec: Option<f32>,
    pub flare_quality_score: Option<i32>,
    pub flare_detected: Option<bool>,
    #[serde(default)]
    pub msfs: Option<MsfsAbfangHerkunft>,
}

/// JSON-Schnittstelle des Exports `messe`: Anfrage hinein, [`Messung`] oder
/// `{"fehler": …}` heraus.
pub fn messen_json(eingabe: &str) -> String {
    match serde_json::from_str::<Anfrage>(eingabe) {
        Ok(a) => match messen(&a) {
            Some(m) => serde_json::to_string(&m).unwrap_or_else(|x| fehler(&x.to_string())),
            None => fehler("keine Proben"),
        },
        Err(x) => fehler(&format!("Anfrage ungueltig: {x}")),
    }
}

fn fehler(text: &str) -> String {
    serde_json::json!({ "fehler": text }).to_string()
}

/// Wie weit ein Hoehen-Stillstand vor das Messfenster zurueck gezaehlt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rueckblick {
    /// Nur innerhalb des Fensters (Fassung 1, Client 3.0.1).
    Keiner,
    /// Steht vor dem Fenster das GANZE Bild still (Hoehe, Sinkrate, G und
    /// Pitch unveraendert), zaehlt der Halt in voller Laenge (Fassung 2).
    Bild,
}

/// Die Regeln einer Fassung.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Regeln {
    pub rueckblick: Rueckblick,
    /// Stillstands-Riegel auch im End-Fit-Fenster des Abfangens.
    pub abfang_stillstand: bool,
}

/// Fassung 1 (Client 3.0.1) — fuer Gegenproben an Altdaten: dieselben
/// Regeln ergeben dort die gespeicherte Zahl.
pub const REGELN_FASSUNG_1: Regeln = Regeln { rueckblick: Rueckblick::Keiner, abfang_stillstand: false };

/// Die Regeln dieser Fassung (2, DLH9901 11.11.2026).
///
/// **Der Fehler.** Der Simulator ruckelte kurz vor dem Aufsetzen: von −571
/// bis −285 ms kam dasselbe Bild (Hoehe, Sinkrate, G, Pitch, Position), danach
/// der Weg am Stueck mit Empfangs- statt Simulatorzeit. Vom Halt lagen nur
/// 20 ms im 310-ms-Fenster; der Stillstands-Riegel (120 ms) sah ihn nicht,
/// und die Gerade las −661 fpm bei −204 am Instrument und 1,21 g. Der
/// Abfang-Pfad las aus demselben Grund −632 statt −198.
///
/// **Die Regel.** Ein Halt, der vor dem Fenster beginnt, zaehlt in voller
/// Laenge — aber nur, wenn das ganze Bild stand. Bleibt nur die Hoehe stehen
/// (normale Nachfuehrung mit halber Rate, die Sinkrate laeuft weiter), gilt
/// wie bisher nur der Teil im Fenster. Derselbe Riegel gilt im End-Fit-
/// Fenster des Abfangens; verworfen wird dort auf das Instrument
/// (`simvar_agl_unreliable`), wie bei den anderen Datenqualitaets-Riegeln.
///
/// **Gemessen** an 1296 Client-Protokollen mit dieser Messart (live,
/// 11.11.2026; Fassung 1 rechnet alle 1296 auf die gespeicherte Zahl):
/// Sinkrate aendert sich bei 3 Landungen, Abfang bei 6, alle mit einem
/// Bild-Stillstand von 121–266 ms am Fenster. Eine Grenzwert-Aenderung
/// (`agl_min_verschiedene` 5 oder `agl_max_stillstand_ms` 80) haette DLH9901
/// nur zusammen mit 86–94 anderen Landungen getroffen. Die Rueckblick-Regel
/// nur ueber die Hoehe aenderte am Korpus dieselben drei, greift aber auch bei
/// normaler Nachfuehrung; die Bild-Regel ist enger und physikalisch begruendet.
pub const REGELN: Regeln = Regeln { rueckblick: Rueckblick::Bild, abfang_stillstand: true };

/// Beginn des Stillstands, in dem `erste` steht, vor ihr zurueck gesucht.
fn lauf_beginn(samples: &[Probe], erste: &Probe, regel: Rueckblick) -> i64 {
    let mut beginn = erste.t_ms;
    if regel == Rueckblick::Keiner {
        return beginn;
    }
    for s in samples.iter().rev().filter(|s| s.t_ms < erste.t_ms) {
        let gleiche_hoehe = s.agl_ft.is_finite() && ((s.agl_ft - erste.agl_ft).abs() * 1000.0).round() as i64 == 0;
        if s.on_ground || !gleiche_hoehe {
            break;
        }
        // Rueckblick::Bild: nur ein ganz stehendes Bild zaehlt mit.
        if !(s.vs_fpm == erste.vs_fpm && s.g_force == erste.g_force && s.pitch_deg == erste.pitch_deg) {
            break;
        }
        beginn = s.t_ms;
    }
    beginn
}

/// Laengster Stillstand der Hoehe in ms ueber `fenster` (zeitlich sortiert);
/// der erste Lauf beginnt bei `vorlauf_beginn` (gleiche Zeitstempel = 0 ms).
fn laengster_stillstand_ms(fenster: &[&Probe], vorlauf_beginn: i64) -> i64 {
    let Some(erste) = fenster.first() else {
        return 0;
    };
    let mut best = erste.t_ms - vorlauf_beginn;
    let mut start_t = vorlauf_beginn;
    for i in 1..fenster.len() {
        let gleich = ((fenster[i].agl_ft - fenster[i - 1].agl_ft).abs() * 1000.0).round() as i64 == 0;
        if gleich {
            best = best.max(fenster[i].t_ms - start_t);
        } else {
            start_t = fenster[i].t_ms;
        }
    }
    best
}

/// Die Messung. `None` ohne Proben (der Kern meldet dann `empty_buffer`).
pub fn messen(a: &Anfrage) -> Option<Messung> {
    messen_mit(a, REGELN)
}

/// Die Messung mit den Regeln `r` (Korpus-Vergleich).
pub fn messen_mit(a: &Anfrage, r: Regeln) -> Option<Messung> {
    let samples = &a.proben;
    if samples.is_empty() {
        return None;
    }
    let p = &a.parameter;
    let edge_ms = 0_i64;

    // --- Hoehenkurve (die gescorte Quelle, v1.6.3; Begruendung im Kern-
    // Kommentar von `compute_landing_analysis`). `auch_bei_stillstand`
    // schaltet den Stillstands-Riegel ab (Unfall-Erkennung). ---
    let vs_aus_hoehenkurve_mit = |auch_bei_stillstand: bool| -> Option<f32> {
        let lo = edge_ms - p.fenster_ms;
        // `!on_ground`: der Kontakt-Frame traegt schon die Fahrwerks-
        // kompression und bei MSFS einen AGL-Sprung.
        let fenster: Vec<&Probe> = samples
            .iter()
            .filter(|s| s.t_ms >= lo && s.t_ms <= edge_ms && s.agl_ft.is_finite() && !s.on_ground)
            .collect();
        if fenster.len() < p.agl_min_samples {
            return None;
        }
        let verschieden = fenster
            .iter()
            .map(|s| (s.agl_ft * 1000.0).round() as i64)
            .collect::<BTreeSet<_>>()
            .len();
        if verschieden < p.agl_min_verschiedene {
            return None;
        }
        // Laengster Stillstand der Hoehe in ms (gleiche Zeitstempel = 0 ms).
        let stillstand = laengster_stillstand_ms(&fenster, lauf_beginn(samples, fenster[0], r.rueckblick));
        let eingefroren = stillstand > p.agl_max_stillstand_ms;
        if eingefroren && !auch_bei_stillstand {
            return None;
        }
        let t0 = fenster.first()?.t_ms;
        let spanne = (fenster.last()?.t_ms - t0) as f64 / 1000.0;
        if spanne < p.agl_min_spanne_s {
            return None;
        }
        // Ausgleichsgerade, nicht Endpunkt-Differenz.
        let n = fenster.len() as f64;
        let ts: Vec<f64> = fenster.iter().map(|s| (s.t_ms - t0) as f64 / 1000.0).collect();
        let ys: Vec<f64> = fenster.iter().map(|s| s.agl_ft as f64).collect();
        let t_mittel = ts.iter().sum::<f64>() / n;
        let y_mittel = ys.iter().sum::<f64>() / n;
        let zaehler: f64 = ts.iter().zip(ys.iter()).map(|(t, y)| (t - t_mittel) * (y - y_mittel)).sum();
        let nenner: f64 = ts.iter().map(|t| (t - t_mittel).powi(2)).sum();
        if nenner <= f64::EPSILON {
            return None;
        }
        let fpm = (zaehler / nenner * 60.0) as f32;
        // Nur ein SINKEN ist eine Landerate.
        if !fpm.is_finite() || !(p.agl_max_plausibel_fpm..0.0).contains(&fpm) {
            return None;
        }
        Some(fpm)
    };

    // Instrument am Aufsetzpunkt: Probenpaar (letzte in der Luft, erste am
    // Boden), lineare Interpolation auf die Kante.
    let mut last_air: Option<&Probe> = None;
    let mut first_ground: Option<&Probe> = None;
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
            let t_a = a.t_ms as f64;
            let t_g = g.t_ms as f64;
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
    let vs_geometrie = vs_aus_hoehenkurve_mit(false);
    let vs_geometrie_ungefiltert = vs_aus_hoehenkurve_mit(true);
    // Hoehenkurve fuehrt, das Instrument ist Rueckfall.
    let vs_at_edge = vs_geometrie.or(vs_aus_simvar);

    // --- Gelaende-Aufteilung (v1.6.9, nur Anzeige) ---
    let steigung_ueber_fenster = |wert: &dyn Fn(&Probe) -> Option<f32>, bewegung_pruefen: bool| -> Option<f32> {
        let lo = edge_ms - p.fenster_ms;
        let punkte: Vec<(f64, f64)> = samples
            .iter()
            .filter(|s| s.t_ms >= lo && s.t_ms <= edge_ms && !s.on_ground)
            .filter_map(|s| {
                let y = wert(s)?;
                if !y.is_finite() {
                    return None;
                }
                Some((s.t_ms as f64 / 1000.0, y as f64))
            })
            .collect();
        if punkte.len() < p.agl_min_samples {
            return None;
        }
        let spanne = punkte.last()?.0 - punkte.first()?.0;
        if spanne < p.agl_min_spanne_s {
            return None;
        }
        let verschiedene = punkte.iter().map(|q| (q.1 * 1000.0).round() as i64).collect::<BTreeSet<_>>().len();
        if bewegung_pruefen && verschiedene < p.agl_min_verschiedene {
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
        if bewegung_pruefen && laengster_stillstand_ms > p.agl_max_stillstand_ms {
            return None;
        }
        let n = punkte.len() as f64;
        let mx = punkte.iter().map(|q| q.0).sum::<f64>() / n;
        let my = punkte.iter().map(|q| q.1).sum::<f64>() / n;
        let sxx: f64 = punkte.iter().map(|q| (q.0 - mx).powi(2)).sum();
        if sxx < 1e-9 {
            return None;
        }
        let sxy: f64 = punkte.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
        let fpm = (sxy / sxx) * 60.0;
        if fpm.is_finite() {
            Some(fpm as f32)
        } else {
            None
        }
    };
    let (vs_gelaende, vs_eigensinken) = if let Some(gemessen) = vs_geometrie {
        let boden_bewegung = steigung_ueber_fenster(&|s: &Probe| s.msl_ft.map(|m| m - s.agl_ft), false);
        let msl = steigung_ueber_fenster(&|s: &Probe| s.msl_ft, true);
        match (boden_bewegung, msl) {
            (Some(boden), Some(eigen)) => {
                let gelaende = -boden + 0.0;
                // Die Probe: die Aufschluesselung MUSS die gemessene Zahl ergeben.
                if (gemessen - (eigen + gelaende)).abs() > p.probe_toleranz_fpm
                    || gelaende.abs() > p.agl_max_plausibel_fpm.abs()
                    || eigen.abs() > p.agl_max_plausibel_fpm.abs()
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

    // --- Abfangen: Fenster [-2000, -100] ms ---
    let flare_lo = edge_ms - 2000;
    let flare_hi = edge_ms - 100;
    let mut peak_vs_pre_simvar: Option<f32> = None;
    let mut vs_at_flare_end_simvar: Option<f32> = None;
    let mut min_dist_to_end = i64::MAX;
    let mut earliest_in_window: Option<&Probe> = None;
    for s in samples {
        let ts = s.t_ms;
        if ts >= flare_lo && ts <= flare_hi && !s.on_ground {
            let pitch_rad = (s.pitch_deg as f64) * std::f64::consts::PI / 180.0;
            let vs_corrected = (s.vs_fpm as f64 * pitch_rad.cos()) as f32;
            if peak_vs_pre_simvar.map(|q| vs_corrected < q).unwrap_or(true) {
                peak_vs_pre_simvar = Some(vs_corrected);
            }
            let dist_to_end = (flare_hi - ts).abs();
            if dist_to_end < min_dist_to_end {
                min_dist_to_end = dist_to_end;
                vs_at_flare_end_simvar = Some(vs_corrected);
            }
            if earliest_in_window.map(|e| ts < e.t_ms).unwrap_or(true) {
                earliest_in_window = Some(s);
            }
        }
    }
    let is_msfs = matches!(a.simulator.as_str(), "Msfs2020" | "Msfs2024");
    let (peak_vs_pre_agl, vs_at_flare_end_agl) =
        if is_msfs { msfs_agl_flare_endpoints(samples, p, flare_lo, flare_hi) } else { (None, None) };
    let flare_gates = if is_msfs {
        flare_gates_mit(samples, p, flare_lo, flare_hi, r)
    } else {
        MsfsFlareGates { agl_reliable: true, unreliable_source: false, pitch_nose_down: false }
    };
    let use_agl = is_msfs && peak_vs_pre_agl.is_some() && vs_at_flare_end_agl.is_some() && flare_gates.agl_reliable;
    let (peak_vs_pre, vs_at_flare_end, flare_vs_source) = if use_agl {
        (peak_vs_pre_agl, vs_at_flare_end_agl, "msfs_agl")
    } else if is_msfs && flare_gates.unreliable_source {
        (peak_vs_pre_simvar, vs_at_flare_end_simvar, "simvar_agl_unreliable")
    } else {
        (peak_vs_pre_simvar, vs_at_flare_end_simvar, "simvar")
    };
    let flare_reduction = match (peak_vs_pre, vs_at_flare_end) {
        (Some(pk), Some(v)) => Some(v - pk),
        _ => None,
    };
    let flare_dvs_dt = match earliest_in_window {
        Some(start) => {
            let dt_sec = (flare_hi - start.t_ms) as f64 / 1000.0;
            if dt_sec > 0.1 {
                let endpoints = if use_agl {
                    let start_vs = agl_geometric_vs_fpm_at(samples, p, start.t_ms, start.pitch_deg, Some(flare_hi + 100));
                    match (start_vs, vs_at_flare_end) {
                        (Some(sv), Some(ev)) => Some((sv, ev)),
                        _ => None,
                    }
                } else {
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
    // Endpunkt-Wertung + Reduktions-Bonus (v0.5.41), gedeckelt 0..100.
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
            let mut detected = red > p.msfs_flare_detect_floor_fpm;
            if is_msfs && use_agl && flare_gates.pitch_nose_down {
                detected = false;
            }
            (Some(score), Some(detected))
        }
        _ => (None, None),
    };

    Some(Messung {
        messung_fassung: MESS_FASSUNG,
        vs_at_edge_fpm: vs_at_edge,
        vs_at_edge_quelle: if vs_geometrie.is_some() { "hoehenkurve" } else { "simvar_fallback" }.into(),
        vs_geometrie_fpm: vs_geometrie,
        vs_geometrie_ungefiltert_fpm: vs_geometrie_ungefiltert,
        vs_simvar_edge_fpm: vs_aus_simvar,
        vs_gelaende_fpm: vs_gelaende,
        vs_eigensinken_fpm: vs_eigensinken,
        peak_vs_pre_flare_fpm: peak_vs_pre,
        vs_at_flare_end_fpm: vs_at_flare_end,
        flare_reduction_fpm: flare_reduction,
        flare_dvs_dt_fpm_per_sec: flare_dvs_dt,
        flare_quality_score: flare_score,
        flare_detected,
        msfs: is_msfs.then(|| MsfsAbfangHerkunft {
            flare_vs_source: flare_vs_source.into(),
            peak_vs_pre_flare_fpm_raw: peak_vs_pre_simvar,
            vs_at_flare_end_fpm_raw: vs_at_flare_end_simvar,
        }),
    })
}

/// Lagfreie Sinkrate aus der Hoehenspur am Zeitpunkt `t_ms`: Ausgleichs-
/// gerade ueber die Luftproben in `t_ms ± msfs_flare_agl_fit_half_window_ms`,
/// nie ueber `fit_hi_cap` hinaus (ein Hopser nach der Kante wuerde die
/// End-Steigung verfaelschen). Pitch-korrigiert wie der Instrumentenpfad.
pub fn agl_geometric_vs_fpm_at(
    samples: &[Probe],
    p: &Parameter,
    t_ms: i64,
    pitch_deg_at_t: f32,
    fit_hi_cap: Option<i64>,
) -> Option<f32> {
    let lo = t_ms - p.msfs_flare_agl_fit_half_window_ms;
    let hi = t_ms + p.msfs_flare_agl_fit_half_window_ms;
    let mut n = 0_usize;
    let mut sx = 0.0_f64;
    let mut sy = 0.0_f64;
    let mut sxx = 0.0_f64;
    let mut sxy = 0.0_f64;
    // Laufzeit (die Messung laeuft im Abtaster unter der Flugsperre): bei
    // zeitlich sortierten Proben (Normalfall) nur der Ausschnitt [lo, hi] —
    // dieselben Proben in derselben Reihenfolge, also dasselbe Ergebnis.
    let ausschnitt = if samples.windows(2).all(|w| w[0].t_ms <= w[1].t_ms) {
        &samples[samples.partition_point(|s| s.t_ms < lo)..samples.partition_point(|s| s.t_ms <= hi)]
    } else {
        samples
    };
    for s in ausschnitt {
        let ts = s.t_ms;
        if ts < lo || ts > hi || s.on_ground {
            continue;
        }
        if let Some(cap) = fit_hi_cap {
            if ts >= cap {
                continue;
            }
        }
        if s.agl_ft > p.td_agl_max_at_window_start_ft || !s.agl_ft.is_finite() {
            continue;
        }
        let x = (ts - t_ms) as f64;
        let y = s.agl_ft as f64;
        n += 1;
        sx += x;
        sy += y;
        sxx += x * x;
        sxy += x * y;
    }
    if n < p.msfs_flare_agl_fit_min_samples {
        return None;
    }
    let nf = n as f64;
    let denom = nf * sxx - sx * sx;
    if denom.abs() < 1e-9 {
        return None;
    }
    let slope_ft_per_ms = (nf * sxy - sx * sy) / denom;
    let fpm = slope_ft_per_ms * 1000.0 * 60.0;
    if !fpm.is_finite() {
        return None;
    }
    let pitch_rad = (pitch_deg_at_t as f64) * std::f64::consts::PI / 180.0;
    Some((fpm * pitch_rad.cos()) as f32)
}

/// Steilste Hoehen-Sinkrate in `[flare_lo, flare_hi]` und die am Punkt
/// naechst `flare_hi`; alle Fits an der Kante gedeckelt.
pub fn msfs_agl_flare_endpoints(samples: &[Probe], p: &Parameter, flare_lo: i64, flare_hi: i64) -> (Option<f32>, Option<f32>) {
    let mut peak_vs_pre: Option<f32> = None;
    let mut vs_at_flare_end: Option<f32> = None;
    let mut min_dist_to_end = i64::MAX;
    let edge_cap = flare_hi + 100;
    for s in samples {
        let ts = s.t_ms;
        if ts < flare_lo || ts > flare_hi || s.on_ground {
            continue;
        }
        let vs_agl = match agl_geometric_vs_fpm_at(samples, p, ts, s.pitch_deg, Some(edge_cap)) {
            Some(v) => v,
            None => continue,
        };
        if peak_vs_pre.map(|q| vs_agl < q).unwrap_or(true) {
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

/// Ergebnis der Phantom-Flare-Riegel.
pub struct MsfsFlareGates {
    /// Weder Datenqualitaet (1) noch Hopser (2) haben verworfen.
    pub agl_reliable: bool,
    /// Ein Datenqualitaets-Riegel hat verworfen.
    pub unreliable_source: bool,
    /// Nase-runter-Veto (3).
    pub pitch_nose_down: bool,
}

/// Phantom-Flare-Riegel (v0.16.22): Datenqualitaet (1a/1b/1c), Hopser (2),
/// Nase-runter-Veto (3). Begruendung der Schwellen im Kern-Kommentar.
pub fn msfs_flare_gates(samples: &[Probe], p: &Parameter, flare_lo: i64, flare_hi: i64) -> MsfsFlareGates {
    flare_gates_mit(samples, p, flare_lo, flare_hi, REGELN)
}

fn flare_gates_mit(samples: &[Probe], p: &Parameter, flare_lo: i64, flare_hi: i64, r: Regeln) -> MsfsFlareGates {
    let mut in_window: Vec<&Probe> = samples.iter().filter(|s| s.t_ms >= flare_lo && s.t_ms <= flare_hi).collect();
    in_window.sort_by_key(|s| s.t_ms);
    let any_on_ground = in_window.iter().any(|s| s.on_ground);
    let half = p.msfs_flare_agl_fit_half_window_ms;
    let max_ft = p.td_agl_max_at_window_start_ft;
    let bounce_scan_hi = flare_hi + half;
    let air: Vec<&Probe> =
        in_window.iter().copied().filter(|s| !s.on_ground && s.agl_ft.is_finite() && s.agl_ft <= max_ft).collect();
    let mut air_bounce: Vec<&Probe> = samples
        .iter()
        .filter(|s| s.t_ms >= flare_lo && s.t_ms <= bounce_scan_hi && !s.on_ground && s.agl_ft.is_finite() && s.agl_ft <= max_ft)
        .collect();
    air_bounce.sort_by_key(|s| s.t_ms);

    let mut distinct: Vec<i64> = air.iter().map(|s| (s.agl_ft as f64 * 100.0).round() as i64).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let n_distinct = distinct.len();

    let end_lo = flare_hi - half;
    let end_hi = flare_hi + half;
    let end_agls: Vec<f32> = air.iter().filter(|s| s.t_ms >= end_lo && s.t_ms <= end_hi).map(|s| s.agl_ft).collect();
    let mut end_distinct: Vec<i64> = end_agls.iter().map(|a| (*a as f64 * 100.0).round() as i64).collect();
    end_distinct.sort_unstable();
    end_distinct.dedup();
    let n_end_distinct = end_distinct.len();
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

    let mut dts: Vec<i64> = in_window.windows(2).map(|w| w[1].t_ms - w[0].t_ms).filter(|&d| d >= 0).collect();
    let median_dt = if dts.is_empty() {
        0
    } else {
        dts.sort_unstable();
        dts[dts.len() / 2]
    };

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

    let end_span_flat = end_agl_span.map(|sp| sp < p.msfs_flare_min_end_agl_span_ft).unwrap_or(false);
    // Stillstand im End-Fit-Fenster (mit derselben Rueckblick-Regel wie die
    // Hoehenkurve): ein gehaltenes Bild, nach dem der Weg am Stueck kommt,
    // macht die End-Steigung genauso falsch wie die Aufsetz-Steigung.
    let end_still = r.abfang_stillstand && {
        let end_air: Vec<&Probe> = air.iter().copied().filter(|s| s.t_ms >= end_lo && s.t_ms <= end_hi).collect();
        match end_air.first() {
            Some(erste) => laengster_stillstand_ms(&end_air, lauf_beginn(samples, erste, r.rueckblick)) > p.agl_max_stillstand_ms,
            None => false,
        }
    };
    let end_frozen = (n_end_distinct > 0 && n_end_distinct < p.msfs_flare_min_end_fit_distinct_agl) || end_span_flat || end_still;
    let dq_reject = n_distinct < p.msfs_flare_min_distinct_agl || median_dt > p.msfs_flare_max_median_dt_ms || end_frozen;
    let bounce_reject = any_on_ground || balloon_ft > p.msfs_flare_balloon_tol_ft;

    let pitch_nose_down = if air.len() >= 2 {
        let first_t = air.first().map(|s| s.t_ms).unwrap_or(0);
        let last_t = air.last().map(|s| s.t_ms).unwrap_or(0);
        let mean_pitch = |pred: &dyn Fn(i64) -> bool| -> Option<f32> {
            let vals: Vec<f32> = air.iter().filter(|s| pred(s.t_ms)).map(|s| s.pitch_deg).collect();
            if vals.is_empty() {
                None
            } else {
                Some(vals.iter().sum::<f32>() / vals.len() as f32)
            }
        };
        let p_start = mean_pitch(&|t| t <= first_t + 300);
        let p_end = mean_pitch(&|t| t >= last_t - 300);
        match (p_start, p_end) {
            (Some(x), Some(y)) => (y - x) <= p.msfs_flare_pitch_nosedown_veto_deg,
            _ => false,
        }
    } else {
        false
    };

    MsfsFlareGates { agl_reliable: !(dq_reject || bounce_reject), unreliable_source: dq_reject, pitch_nose_down }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn parameter() -> Parameter {
        Parameter {
            fenster_ms: 310,
            agl_min_samples: 4,
            agl_min_verschiedene: 3,
            agl_max_stillstand_ms: 120,
            agl_min_spanne_s: 0.15,
            agl_max_plausibel_fpm: -3000.0,
            probe_toleranz_fpm: 20.0,
            td_agl_max_at_window_start_ft: 250.0,
            msfs_flare_agl_fit_half_window_ms: 300,
            msfs_flare_agl_fit_min_samples: 4,
            msfs_flare_min_distinct_agl: 8,
            msfs_flare_max_median_dt_ms: 60,
            msfs_flare_min_end_fit_distinct_agl: 4,
            msfs_flare_min_end_agl_span_ft: 0.3,
            msfs_flare_balloon_tol_ft: 1.5,
            msfs_flare_pitch_nosedown_veto_deg: -1.0,
            msfs_flare_detect_floor_fpm: 50.0,
        }
    }

    fn sinkflug(vs_fpm: f32) -> Vec<Probe> {
        (-2500..=500)
            .step_by(20)
            .map(|t: i64| {
                let luft = t < 0;
                Probe {
                    t_ms: t,
                    vs_fpm,
                    g_force: 1.0,
                    on_ground: !luft,
                    agl_ft: if luft { (vs_fpm / 60_000.0) * t as f32 } else { 0.0 },
                    msl_ft: None,
                    pitch_deg: 2.0,
                }
            })
            .collect()
    }

    #[test]
    fn gleichmaessiger_sinkflug_liest_die_sinkrate() {
        let m = messen(&Anfrage { simulator: "Msfs2024".into(), proben: sinkflug(-240.0), parameter: parameter() }).unwrap();
        assert_eq!(m.vs_at_edge_quelle, "hoehenkurve");
        let vs = m.vs_at_edge_fpm.unwrap();
        assert!((vs + 240.0).abs() < 1.0, "{vs}");
        assert!(m.msfs.is_some(), "MSFS traegt die Abfang-Herkunft");
        let x = messen(&Anfrage { simulator: "XPlane12".into(), proben: sinkflug(-240.0), parameter: parameter() }).unwrap();
        assert!(x.msfs.is_none(), "X-Plane ohne MSFS-Schluessel");
    }

    #[test]
    fn json_wie_nativ_und_unbekannte_felder_erlaubt() {
        let a = Anfrage { simulator: "Msfs2024".into(), proben: sinkflug(-300.0), parameter: parameter() };
        let mut v = serde_json::to_value(&a).unwrap();
        // Spaetere Kanaele und Parameter duerfen in der Anfrage stehen.
        v["proben"][0]["lat"] = serde_json::json!(48.1);
        v["parameter"]["neu_in_spaeterer_fassung"] = serde_json::json!(1);
        let ueber_json: Messung = serde_json::from_str(&messen_json(&v.to_string())).unwrap();
        assert_eq!(ueber_json, messen(&a).unwrap());
    }

    /// Die echten Sim-Bilder von DLH9901 (Wertwechsel aus dem Protokoll),
    /// mit 20-ms-Takt gehalten wie im Aufsetzfenster. `vs_laeuft`: die
    /// Sinkrate aendert sich in jeder Probe (nur die Hoehe steht — normale
    /// Nachfuehrung, kein Ruckler).
    fn dlh9901(vs_laeuft: bool) -> Vec<Probe> {
        // (ab ms, Hoehe ft, Sinkrate fpm, G, am Boden)
        let bilder: [(i64, f32, f32, f32, bool); 10] = [
            (-1080, 22.927, 13.67, 0.898, false),
            (-1019, 22.717, 1.06, 0.893, false),
            (-898, 21.575, -55.39, 0.885, false),
            (-836, 21.088, -75.93, 0.886, false),
            (-755, 20.854, -85.33, 0.887, false),
            (-571, 20.5, -98.99, 0.888, false),
            (-285, 19.573, -132.47, 0.894, false),
            (-184, 18.002, -182.31, 0.913, false),
            (-102, 17.446, -198.01, 0.920, false),
            (0, 17.007, -204.49, 1.128, true),
        ];
        (-1080..=300)
            .step_by(20)
            .map(|t: i64| {
                let b = bilder.iter().rev().find(|x| x.0 <= t).unwrap();
                Probe {
                    t_ms: t,
                    vs_fpm: if vs_laeuft { b.2 - (t - b.0) as f32 * 0.01 } else { b.2 },
                    g_force: b.3,
                    on_ground: b.4,
                    agl_ft: b.1,
                    msl_ft: None,
                    pitch_deg: 3.4,
                }
            })
            .collect()
    }

    fn anfrage(proben: Vec<Probe>) -> Anfrage {
        Anfrage { simulator: "Msfs2024".into(), proben, parameter: parameter() }
    }

    /// DLH9901: Der Ruckler beginnt vor dem 310-ms-Fenster. Fassung 2 liest
    /// das Instrument (−204), Fassung 1 las −661; die ungefilterte Gerade
    /// der Unfall-Erkennung bleibt hart (nie weichspuelen). Der Abfang-Pfad
    /// faellt aus demselben Grund aufs Instrument zurueck.
    #[test]
    fn ruckler_vor_dem_fenster_liest_das_instrument() {
        let m = messen(&anfrage(dlh9901(false))).unwrap();
        assert_eq!(m.messung_fassung, 2);
        assert_eq!(m.vs_at_edge_quelle, "simvar_fallback");
        let vs = m.vs_at_edge_fpm.unwrap();
        assert!((-215.0..=-190.0).contains(&vs), "{vs}");
        assert!(m.vs_geometrie_ungefiltert_fpm.unwrap() < -550.0);
        let h = m.msfs.as_ref().unwrap();
        assert_eq!(h.flare_vs_source, "simvar_agl_unreliable");
        let ende = m.vs_at_flare_end_fpm.unwrap();
        assert!((-215.0..=-180.0).contains(&ende), "Abfang-Ende {ende}");

        let alt = messen_mit(&anfrage(dlh9901(false)), REGELN_FASSUNG_1).unwrap();
        assert_eq!(alt.vs_at_edge_quelle, "hoehenkurve");
        assert!(alt.vs_at_edge_fpm.unwrap() < -550.0, "Gegenprobe: Fassung 1 las hart");
        assert_eq!(alt.msfs.unwrap().flare_vs_source, "msfs_agl");
    }

    /// Steht nur die Hoehe (die Sinkrate laeuft weiter), ist es kein
    /// Ruckler: Fassung 2 rechnet wie Fassung 1.
    #[test]
    fn nur_die_hoehe_steht_kein_rueckblick() {
        let neu = messen(&anfrage(dlh9901(true))).unwrap();
        let alt = messen_mit(&anfrage(dlh9901(true)), REGELN_FASSUNG_1).unwrap();
        assert_eq!(neu.vs_at_edge_fpm, alt.vs_at_edge_fpm);
        assert_eq!(neu.vs_at_edge_quelle, "hoehenkurve");
    }

    /// Kante des Riegels mit Rueckblick: 120 ms Halt bleibt, 121 ms verwirft.
    #[test]
    fn rueckblick_kante_120_121() {
        let mit_pause = |pause: i64| {
            let p = |t: i64, agl: f32, boden: bool| Probe { t_ms: t, vs_fpm: -200.0, g_force: 1.0, on_ground: boden, agl_ft: agl, msl_ft: None, pitch_deg: 2.0 };
            vec![p(-340, 15.0, false), p(-340 + pause, 15.0, false), p(-180, 13.0, false), p(-140, 11.0, false), p(-60, 9.0, false), p(-20, 8.0, false), p(0, 0.0, true), p(20, 0.0, true)]
        };
        assert_eq!(messen(&anfrage(mit_pause(120))).unwrap().vs_at_edge_quelle, "hoehenkurve");
        assert_eq!(messen(&anfrage(mit_pause(121))).unwrap().vs_at_edge_quelle, "simvar_fallback");
    }

    #[test]
    fn leere_oder_kaputte_anfrage_gibt_fehler() {
        let v: serde_json::Value = serde_json::from_str(&messen_json("{}")).unwrap();
        assert!(v["fehler"].as_str().unwrap().contains("ungueltig"));
        let leer = Anfrage { simulator: "Msfs2024".into(), proben: vec![], parameter: parameter() };
        let v: serde_json::Value = serde_json::from_str(&messen_json(&serde_json::to_string(&leer).unwrap())).unwrap();
        assert_eq!(v["fehler"], "keine Proben");
    }
}
