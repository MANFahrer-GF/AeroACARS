//! Erkennt, wenn der Simulator eine Aufzeichnung abspielt statt zu fliegen.
//!
//! # Warum
//!
//! X-Plane sagt es uns selbst: `sim/time/is_in_replay` wird seit Spec v0.7.15 F6
//! gelesen und in dieselbe Pause-Logik gefaltet — aus AeroACARS-Sicht ist Replay
//! „die Telemetrie ist nicht echt, also wie Pause behandeln". **MSFS hat kein
//! solches SimVar.** Dort gibt es nur die Flow-Ereignisse des 2024er SDK, und ob
//! unsere per `bindgen` erzeugte Anbindung sie kennt, haengt vom SDK-Stand auf
//! dem Windows-Bauserver ab.
//!
//! Dieses Modul ist der **portable Rueckfall**: es braucht nichts vom Simulator
//! ausser dem, was wir ohnehin lesen, und arbeitet damit fuer MSFS, X-Plane und
//! die Stratos-Bruecke gleich.
//!
//! # Das Verfahren
//!
//! Uebernommen von `msfs24-landing-stats` (Arderos, `ReplayTelemetryDetector.cs`)
//! — nicht neu erfunden. Der Kern ist eine Beobachtung ueber MSFS:
//!
//! > Beim Abspielen setzt der Simulator **Position und Lage** nach, erzeugt aber
//! > **keine passenden Geschwindigkeits- und Luftdaten**.
//!
//! Also widersprechen sich zwei Quellen, die im echten Flug immer uebereinstimmen:
//! die aus aufeinanderfolgenden Positionen *errechnete* Geschwindigkeit und die
//! vom Sim *gemeldete*. Wie eng sie normalerweise zusammenliegen, ist an unserem
//! eigenen Bestand gemessen: ueber 334.007 Probenpaare aus 875 Fluegen betraegt
//! die Abweichung **0,4 % im Median** (p90 1,3 %).
//!
//! Geurteilt wird ueber **Mediane**, nicht ueber einzelne Ausreisser — ein
//! kurzer Aussetzer im Positionsstrom soll niemanden verdaechtigen.
//!
//! # Was dabei NICHT herauskommen darf
//!
//! Ein Fehlalarm waere teurer als eine verpasste Erkennung: er wuerde einem
//! ehrlichen Piloten die Landung entwerten. Deshalb muessen **alle sechs**
//! Bedingungen gleichzeitig zutreffen, und es braucht mindestens
//! [`MIN_PAARE`] Belege. Am gesamten Bestand (875 Fluege, kein einziger Replay)
//! schlaegt das Verfahren **null Mal** an — siehe `replay_erkennung_tests`.

// Noch NICHT verdrahtet: das Modul ist geprueft, aber der Aufrufer fehlt. Wo
// es laufen soll, ist eine Entwurfsfrage — unser Aufsetzpuffer reicht mit 2,5 s
// vor Bodenkontakt NICHT (Arderos nutzt 15 s), es gehoert also an den laufenden
// Telemetriestrom, so wie X-Plane es mit `is_in_replay` haelt.
#![allow(dead_code)]

/// Eine Probe, so wie der Telemetriestrom sie liefert.
#[derive(Debug, Clone, Copy)]
pub struct ReplayProbe {
    /// Sekunden seit einem beliebigen Nullpunkt. Muss monoton steigen.
    pub t_s: f64,
    pub lat: f64,
    pub lon: f64,
    /// Hoehe ueber dem Meeresspiegel in Fuss.
    pub msl_ft: f64,
    /// Vom Simulator GEMELDETE Grundgeschwindigkeit.
    pub groundspeed_kt: f64,
    /// Vom Simulator GEMELDETE angezeigte Fahrt.
    pub ias_kt: f64,
    /// Vom Simulator GEMELDETE Vertikalgeschwindigkeit, Fuss je Sekunde.
    pub vs_fps: f64,
    pub on_ground: bool,
}

/// Kuerzester Abstand zweier Proben, aus denen ein Beleg gebildet wird.
///
/// Kuerzer waere anfaellig fuer Rundung in Position und Zeitstempel: bei 50 Hz
/// und 140 kt liegen zwei Proben 1,2 m auseinander, da schlaegt jede
/// Nachkommastelle durch.
const MIN_PAARDAUER_S: f64 = 0.5;
/// Laengster Abstand. Darueber ist die Gerade zwischen zwei Punkten keine
/// brauchbare Naeherung der geflogenen Bahn mehr.
const MAX_PAARDAUER_S: f64 = 1.5;
/// So viele Belege muessen zusammenkommen, sonst wird nicht geurteilt.
pub const MIN_PAARE: usize = 8;

/// Ab hier bewegt sich das Flugzeug mit Flugtempo durch die Landschaft.
const MIN_ERRECHNETE_GS_KT: f64 = 30.0;
/// Bis hierher meldet der Sim „steht praktisch".
const MAX_GEMELDETE_GS_KT: f64 = 5.0;
/// Bis hierher meldet der Sim „keine nennenswerte Fahrt".
const MAX_GEMELDETE_IAS_KT: f64 = 30.0;
/// Der errechnete Wert muss den gemeldeten um diesen Faktor uebertreffen.
/// Faengt den Fall ab, dass beide klein sind und das Verhaeltnis zufaellig passt.
const MIN_VERHAELTNIS: f64 = 4.0;
/// Die Hoehe muss sich spuerbar aendern — sonst ist es ein Standbild.
const MIN_HOEHENRATE_FPS: f64 = 3.0;
/// Und die gemeldete Sinkrate muss dieser Aenderung deutlich widersprechen.
const MIN_VS_FEHLER_FPS: f64 = 5.0;

/// Grosskreisentfernung in nautischen Meilen.
fn distanz_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R_NM: f64 = 3440.065;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = p2 - p1;
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R_NM * a.sqrt().min(1.0).asin()
}

fn median(mut werte: Vec<f64>) -> Option<f64> {
    werte.retain(|v| v.is_finite());
    if werte.is_empty() {
        return None;
    }
    werte.sort_by(|a, b| a.partial_cmp(b).expect("keine NaN mehr"));
    let m = werte.len() / 2;
    Some(if werte.len() % 2 == 0 {
        (werte[m - 1] + werte[m]) / 2.0
    } else {
        werte[m]
    })
}

fn brauchbar(p: &ReplayProbe) -> bool {
    p.t_s.is_finite()
        && p.lat.is_finite()
        && (-90.0..=90.0).contains(&p.lat)
        && p.lon.is_finite()
        && (-180.0..=180.0).contains(&p.lon)
        && p.msl_ft.is_finite()
        && p.groundspeed_kt.is_finite()
        && p.ias_kt.is_finite()
        && p.vs_fps.is_finite()
}

/// Das Urteil samt der Zahlen, die dazu gefuehrt haben.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReplayBefund {
    pub ist_replay: bool,
    pub belege: usize,
    /// `None`, wenn zu wenige Belege fuer ein Urteil zusammenkamen.
    pub median_errechnete_gs_kt: Option<f64>,
    pub median_gemeldete_gs_kt: Option<f64>,
}

/// Prueft einen Abschnitt des Telemetriestroms.
///
/// Erwartet Proben in zeitlicher Reihenfolge. Bodenproben zaehlen nicht mit —
/// am Boden ist „gemeldete Geschwindigkeit nahe null" der Normalfall und kein
/// Widerspruch.
pub fn pruefe_replay(proben: &[ReplayProbe]) -> ReplayBefund {
    let leer = ReplayBefund {
        ist_replay: false,
        belege: 0,
        median_errechnete_gs_kt: None,
        median_gemeldete_gs_kt: None,
    };
    if proben.len() < MIN_PAARE + 1 {
        return leer;
    }

    let mut errechnet_gs = Vec::new();
    let mut gemeldet_gs = Vec::new();
    let mut gemeldet_ias = Vec::new();
    let mut errechnete_hoehenrate = Vec::new();
    let mut vs_fehler = Vec::new();

    let mut j = 0usize;
    for i in 0..proben.len() {
        let a = &proben[i];
        if a.on_ground || !brauchbar(a) {
            continue;
        }
        j = j.max(i + 1);
        while j < proben.len() && proben[j].t_s - a.t_s < MIN_PAARDAUER_S {
            j += 1;
        }
        if j >= proben.len() {
            break;
        }
        let b = &proben[j];
        let dauer = b.t_s - a.t_s;
        if b.on_ground || !brauchbar(b) || dauer <= 0.0 || dauer > MAX_PAARDAUER_S {
            continue;
        }

        errechnet_gs.push(distanz_nm(a.lat, a.lon, b.lat, b.lon) * 3600.0 / dauer);
        gemeldet_gs.push((a.groundspeed_kt.abs() + b.groundspeed_kt.abs()) / 2.0);
        gemeldet_ias.push((a.ias_kt.abs() + b.ias_kt.abs()) / 2.0);
        let rate = (b.msl_ft - a.msl_ft) / dauer;
        errechnete_hoehenrate.push(rate);
        vs_fehler.push((rate - (a.vs_fps + b.vs_fps) / 2.0).abs());
    }

    let belege = errechnet_gs.len();
    if belege < MIN_PAARE {
        return ReplayBefund { belege, ..leer };
    }

    let (Some(m_err), Some(m_gem), Some(m_ias), Some(m_hoehe), Some(m_vs)) = (
        median(errechnet_gs),
        median(gemeldet_gs),
        median(gemeldet_ias),
        median(errechnete_hoehenrate.iter().map(|v| v.abs()).collect()),
        median(vs_fehler),
    ) else {
        return ReplayBefund { belege, ..leer };
    };

    let ist_replay = m_err >= MIN_ERRECHNETE_GS_KT
        && m_gem <= MAX_GEMELDETE_GS_KT
        && m_ias <= MAX_GEMELDETE_IAS_KT
        && m_err >= m_gem * MIN_VERHAELTNIS
        && m_hoehe >= MIN_HOEHENRATE_FPS
        && m_vs >= MIN_VS_FEHLER_FPS;

    ReplayBefund {
        ist_replay,
        belege,
        median_errechnete_gs_kt: Some(m_err),
        median_gemeldete_gs_kt: Some(m_gem),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Baut einen Anflug: sinkt mit `vs_fps`, fliegt mit `gs_kt`.
    /// `echt = false` stellt Replay nach — Position bewegt sich, die
    /// gemeldeten Werte bleiben auf null.
    fn anflug(n: usize, gs_kt: f64, vs_fps: f64, echt: bool) -> Vec<ReplayProbe> {
        let dt = 0.1; // 10 Hz reicht: die Paare sind 0,5–1,5 s auseinander
        let mut v = Vec::with_capacity(n);
        let mut lat = 50.0_f64;
        let mut msl = 3000.0_f64;
        for i in 0..n {
            lat += gs_kt / 3600.0 / 60.0 * dt;
            msl += vs_fps * dt;
            v.push(ReplayProbe {
                t_s: i as f64 * dt,
                lat,
                lon: 8.0,
                msl_ft: msl,
                groundspeed_kt: if echt { gs_kt } else { 0.0 },
                ias_kt: if echt { (gs_kt - 10.0).max(0.0) } else { 0.0 },
                vs_fps: if echt { vs_fps } else { 0.0 },
                on_ground: false,
            });
        }
        v
    }

    #[test]
    fn echter_anflug_ist_kein_replay() {
        let b = pruefe_replay(&anflug(200, 140.0, -12.0, true));
        assert!(!b.ist_replay, "echter Anflug faelschlich als Replay: {b:?}");
        assert!(b.belege >= MIN_PAARE, "zu wenige Belege gesammelt: {b:?}");
    }

    #[test]
    fn replay_wird_erkannt() {
        let b = pruefe_replay(&anflug(200, 140.0, -12.0, false));
        assert!(b.ist_replay, "Replay nicht erkannt: {b:?}");
        assert!(b.median_errechnete_gs_kt.expect("Median") > 100.0);
        assert!(b.median_gemeldete_gs_kt.expect("Median") < 1.0);
    }

    #[test]
    fn am_boden_wird_nicht_geurteilt() {
        let mut p = anflug(200, 140.0, -12.0, false);
        for x in p.iter_mut() {
            x.on_ground = true;
        }
        let b = pruefe_replay(&p);
        assert!(!b.ist_replay);
        assert_eq!(b.belege, 0, "Bodenproben duerfen keine Belege liefern");
    }

    #[test]
    fn zu_wenige_proben_urteilen_nicht() {
        let b = pruefe_replay(&anflug(5, 140.0, -12.0, false));
        assert!(!b.ist_replay);
        assert!(b.belege < MIN_PAARE);
        assert!(b.median_errechnete_gs_kt.is_none());
    }

    #[test]
    fn langsames_rollen_in_der_luft_ist_kein_replay() {
        let b = pruefe_replay(&anflug(200, 20.0, -1.0, true));
        assert!(!b.ist_replay, "langsamer Flug faelschlich als Replay: {b:?}");
    }

    #[test]
    fn standbild_ist_kein_replay() {
        let mut p = anflug(200, 0.0, 0.0, false);
        for x in p.iter_mut() {
            x.msl_ft = 3000.0;
        }
        let b = pruefe_replay(&p);
        assert!(!b.ist_replay);
    }

    #[test]
    fn unbrauchbare_werte_kippen_nichts() {
        let mut p = anflug(200, 140.0, -12.0, true);
        p[10].lat = f64::NAN;
        p[20].lon = 999.0;
        p[30].vs_fps = f64::INFINITY;
        let b = pruefe_replay(&p);
        assert!(!b.ist_replay);
    }

    #[test]
    fn ein_einzelner_aussetzer_reicht_nicht() {
        // Der Kern der Median-Entscheidung: ein paar kaputte Proben in einem
        // sonst gesunden Anflug duerfen niemanden verdaechtigen. Mit einer
        // Trefferzahl statt Medianen waere das ein Fehlalarm gewesen.
        let mut p = anflug(200, 140.0, -12.0, true);
        for x in p.iter_mut().take(40) {
            x.groundspeed_kt = 0.0;
            x.ias_kt = 0.0;
            x.vs_fps = 0.0;
        }
        let b = pruefe_replay(&p);
        assert!(!b.ist_replay, "20 % Aussetzer duerfen nicht reichen: {b:?}");
    }
}
