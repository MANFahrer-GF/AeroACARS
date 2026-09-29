//! Anflug-Forensik ohne Note — Lernpaket AP4/AP5 (29.09.2026).
//!
//! # Was hier gerechnet wird
//!
//! * **AP4 — Gleitpfad:** je Probe des Anflug-Puffers die geometrische
//!   Abweichung vom Gleitpfad der TATSAECHLICH gelandeten Bahn: Entfernung
//!   zur Landeschwelle aus Position und Bahnachse, Sollhoehe aus
//!   Schwellenhoehe + TCH + Entfernung × tan(Winkel). Bisher gab es nur
//!   Soll-V/S gegen Ist-V/S (`approach_vs_deviation_fpm`) — das sagt, ob
//!   man parallel zum Pfad sinkt, aber nicht, ob man AUF ihm ist.
//! * **AP5 — Anflugruhe:** Seitenwechsel der Pfadabweichung, Nick- und
//!   Roll-Unruhe, Schub-Umkehrungen je Minute, getrennt fuer die Tore
//!   1000–500 und 500–200 ft ueber der Schwelle.
//!
//! # Was hier ausdruecklich NICHT passiert
//!
//! Keine Note, keine Unternote, kein Stabilitaets-Gate, kein Deckel liest
//! diese Werte (Beschluss Thomas 29.09.2026: „zunaechst nur Forensik").
//! Die Anzeige ist ein Hinweis im Sinne des Bordbuchs: Erledigtes zaehlen,
//! nie tadeln. Die Werte gehen in den lokalen Datensatz und ins Analyse-
//! JSON des Flug-Logs, NICHT in MQTT/PIREP (Nachrichten > 10 KB brechen ab).
//!
//! # Hoehe
//!
//! `msl_ft` im Anflug-Puffer ist die geometrische (wahre) Hoehe aus dem
//! Simulator — MSFS `PLANE ALTITUDE`, X-Plane `elevation` —, nicht die
//! barometrisch angezeigte. Genau das braucht der Vergleich mit der
//! Schwellenhoehe aus den Navdaten: ein Kaltwetter- oder QNH-Fehler im
//! Hoehenmesser verschiebt diese Rechnung nicht (ein echtes ILS misst den
//! Winkel ebenfalls geometrisch).

use chrono::{DateTime, Utc};
use storage::{AnflugGleitpfad, AnflugRuhe, GleitpfadTor, RuheTor};

use crate::ApproachBufferSample;

/// Ein Dot = 0,35°. So rechnet vmsACARS (voller Ausschlag 0,7° = 2 Dots),
/// und so steht es sinngemaess im ICAO Annex 10: die Gleitweg-Anzeige hat
/// ihren Vollausschlag bei rund 0,12 × Gleitwinkel, bei 3° also 0,36°
/// je Dot. Wir BENOTEN das nicht — die Dots sind nur die gewohnte Einheit.
pub(crate) const GRAD_JE_DOT: f64 = 0.35;

/// Obere Grenze des ausgewerteten Anflugs, ft ueber der Schwelle.
const HOEHE_OBEN_FT: f64 = 1000.0;
/// Grenze zwischen den Toren 1000–500 und 500–200.
const HOEHE_MITTE_FT: f64 = 500.0;
/// Untere Grenze: 200 ft ist die Entscheidungshoehe eines CAT-I-Anflugs.
/// Darunter beginnt der Uebergang in den Sichtflug und das Ausschweben —
/// dort ist eine Pfadabweichung kein Anflugfehler mehr.
const HOEHE_UNTEN_FT: f64 = 200.0;

/// Nur Proben aus den letzten fuenf Minuten vor dem Aufsetzen. Von 1000 ft
/// sinkt selbst ein langsamer GA-Anflug (300 fpm) in gut drei Minuten; ein
/// VORHERIGER Anflug mit Durchstarten liegt dagegen fast immer weiter
/// zurueck (Platzrunde) und darf nicht mitgerechnet werden.
const ANFLUG_FENSTER_S: i64 = 300;

/// Halber Oeffnungswinkel des Anflugsektors um die verlaengerte Achse.
/// ±35° ist die Abdeckung eines Landekurssenders (ICAO Annex 10) — wer
/// ausserhalb steht, hat keinen Bezug zu diesem Gleitpfad.
const SEKTOR_HALBWINKEL_DEG: f64 = 35.0;

/// „Grob in Anflugrichtung": mehr als 90° zwischen Steuerkurs und
/// Bahnrichtung ist Gegen- oder Queranflug, keine Endanflugprobe.
const KURS_TOLERANZ_DEG: f64 = 90.0;

/// Mindestens so viele Proben je Band, sonst keine Kennwerte. Unter 1500 ft
/// tastet der Puffer etwa sekuendlich ab; fuenf Proben sind fuenf Sekunden.
const MIN_PROBEN_JE_TOR: usize = 5;

/// Fehlt die TCH in den Navdaten (der Server schreibt dann 0), gilt 50 ft
/// — der ICAO-Richtwert fuer Verkehrsflugzeuge. Die Unsicherheit ist
/// klein: 10 ft TCH verschieben den Bezugspunkt um rund 190 ft entlang der
/// Bahn, das ist bei 1 NM Entfernung weniger als 0,03°.
const TCH_ANNAHME_FT: f64 = 50.0;

/// Totband fuer den Seitenwechsel: erst ab ±0,1 Dot (0,035°) gilt eine
/// Probe als „ueber" oder „unter" dem Pfad. Bei 2 NM sind das rund 7 ft —
/// weniger als die Hoehe, um die ein Flugzeug zwischen zwei Proben schon
/// durch normale Luftbewegung schwankt. Ohne Totband zaehlte jede
/// Probe, die den Pfad nur streift, als Korrektur.
const PFAD_TOTBAND_DOTS: f64 = 0.1;

/// Totband fuer Schub-Umkehrungen: 2 % N1. Eine Autothrottle- oder
/// Hebelkorrektur im Endanflug bewegt N1 um 3–10 %; darunter liegen das
/// Nachregeln der Triebwerksregelung und die Anzeigeaufloesung. Gezaehlt
/// wird erst, wenn N1 sich vom letzten Extremwert um mehr als das Totband
/// in die GEGENRICHTUNG bewegt hat (Hysterese).
const SCHUB_TOTBAND_PCT: f64 = 2.0;

/// Zeitabstand, in dem zwei Proben eine Nick-/Rollrate bilden duerfen.
/// Unter 0,2 s dominiert das Zeitstempel-Rauschen, ueber 5 s ist es keine
/// Rate mehr, sondern eine Luecke im Puffer.
const RATE_DT_MIN_S: f64 = 0.2;
const RATE_DT_MAX_S: f64 = 5.0;

/// Eine Rate braucht so viele Paare fuer eine Streuung.
const MIN_RATEN: usize = 4;

/// Unter dieser Dauer ist „Umkehrungen je Minute" keine Rate.
const MIN_DAUER_SCHUB_S: f64 = 10.0;

pub(crate) const QUELLE_NAVIGRAPH_ILS: &str = "navigraph_ils";
pub(crate) const QUELLE_NAVIGRAPH_BAHN: &str = "navigraph_bahn";
pub(crate) const QUELLE_ANGENOMMEN_3GRAD: &str = "angenommen_3grad";

const GRUND_KEINE_BAHN: &str = "keine_bahn";
const GRUND_SCHWELLENHOEHE_FEHLT: &str = "schwellenhoehe_fehlt";
const GRUND_KEINE_PROBEN: &str = "keine_proben";

const FT_JE_M: f64 = 3.280_839_895;

/// Die Bahn, gegen die gerechnet wird — aus den Navigraph-Navdaten der
/// gelandeten Bahn (`stats.runway_nav_geometry`).
#[derive(Debug, Clone)]
pub(crate) struct Bahnbezug {
    /// Schwellenpunkt der Navdaten (kann der physische Bahnanfang sein).
    pub schwelle_lat: f64,
    pub schwelle_lon: f64,
    pub ende_lat: f64,
    pub ende_lon: f64,
    /// Versetzte Schwelle, soweit NICHT schon in der Geometrie — dieselbe
    /// Groesse, die `assess_touchdown` von der Aufsetzdistanz abzieht.
    pub versatz_ft: f64,
    /// Hoehe der Landeschwelle (DFD `landing_threshold_elevation`).
    pub schwellenhoehe_ft: Option<f64>,
    /// TCH laut Navdaten; `None` oder ≤ 0 = unbekannt.
    pub tch_ft: Option<f64>,
    /// Gleitwinkel laut Navdaten.
    pub winkel_deg: f64,
    /// Die Bahn hat eine ILS-Anlage (`NavRunway::ils`).
    pub hat_ils: bool,
}

impl Bahnbezug {
    /// Aus der Navdaten-Bahn. ⚠ `glideslope_angle` und `tch_ft` tragen
    /// serde-Standardwerte (3,0 / 50) — am Wert laesst sich NICHT ablesen,
    /// ob er echt ist. Deshalb entscheidet `ils.is_some()` ueber die Quelle,
    /// und eine TCH von 0 (so schreibt der Server „fehlt") gilt als
    /// unbekannt.
    pub fn aus_navdaten(nav: &aeroacars_mqtt::navdata::NavRunway, versatz_ft: f64) -> Self {
        Self {
            schwelle_lat: nav.threshold.lat,
            schwelle_lon: nav.threshold.lon,
            ende_lat: nav.far_end.lat,
            ende_lon: nav.far_end.lon,
            versatz_ft: versatz_ft.max(0.0),
            schwellenhoehe_ft: nav.threshold.elev_ft.map(f64::from),
            tch_ft: Some(nav.tch_ft as f64).filter(|t| *t > 0.0),
            winkel_deg: nav.glideslope_angle,
            hat_ils: nav.ils.is_some(),
        }
    }
}

/// Eine Probe, fertig fuer beide Auswertungen.
#[derive(Debug, Clone, Copy)]
struct Punkt {
    at: DateTime<Utc>,
    hoehe_ft: f64,
    /// Pfadabweichung in Dots / ft — `None`, wenn die Probe nicht vor der
    /// Schwelle im Anflugsektor lag oder es keinen Pfad gibt.
    abw_dots: Option<f64>,
    abw_ft: Option<f64>,
    pitch_deg: Option<f64>,
    bank_deg: f64,
    n1_pct: Option<f64>,
}

/// Ergebnis beider Arbeitspakete fuer eine Landung.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AnflugForensik {
    pub gleitpfad: Option<AnflugGleitpfad>,
    pub ruhe: Option<AnflugRuhe>,
}

impl AnflugForensik {
    /// Traegt beide Befunde in das Analyse-JSON (`landing_analysis`) ein.
    /// Nur wenn es ein Objekt ist — ein fremdes Format wird nicht angefasst.
    pub fn in_analyse_json(&self, analyse: &mut serde_json::Value) {
        let Some(obj) = analyse.as_object_mut() else {
            return;
        };
        if let Some(g) = &self.gleitpfad {
            if let Ok(v) = serde_json::to_value(g) {
                obj.insert("anflug_gleitpfad".to_string(), v);
            }
        }
        if let Some(r) = &self.ruhe {
            if let Ok(v) = serde_json::to_value(r) {
                obj.insert("anflug_ruhe".to_string(), v);
            }
        }
    }
}

/// Welche Quelle der Bezugspfad hat.
///
/// `navigraph_bahn` heisst: Navigraph-Bahn ohne ILS. Deren Winkel ist auf
/// dem Server fast immer der 3°-Rueckfall (siehe `dfdNavFill.ts`:
/// `anlage?.gs ?? GLEITWEG_STANDARD_GRAD`) — die Geometrie (Schwelle,
/// Hoehe, TCH) ist aber echt, und das ist der groessere Teil der Rechnung.
pub(crate) fn quelle(bahn: Option<&Bahnbezug>) -> &'static str {
    match bahn {
        Some(b) if b.hat_ils => QUELLE_NAVIGRAPH_ILS,
        Some(_) => QUELLE_NAVIGRAPH_BAHN,
        None => QUELLE_ANGENOMMEN_3GRAD,
    }
}

/// Wertet den Anflug-Puffer aus.
///
/// * `bahn`: Navdaten der gelandeten Bahn; `None` → Quelle
///   `angenommen_3grad`, KEINE Gleitpfad-Werte (ohne Schwelle gibt es
///   nichts zu rechnen — erfinden wir keine).
/// * `td`: Aufsetzzeitpunkt; nur Proben davor zaehlen.
/// * `platzhoehe_ft`: Rueckfall fuer die Hoehenbaender der Anflugruhe,
///   wenn die Schwellenhoehe fehlt (dieselbe Bezugshoehe wie das
///   Stabilitaets-Gate).
pub(crate) fn auswerten(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    bahn: Option<&Bahnbezug>,
    td: Option<DateTime<Utc>>,
    platzhoehe_ft: Option<f64>,
) -> AnflugForensik {
    let quelle = quelle(bahn);
    let winkel_deg = bahn
        .map(|b| b.winkel_deg)
        // Dieselbe Plausibilisierung wie ueberall im Client (2,0–7,5°).
        .filter(|g| (2.0..=7.5).contains(g))
        .unwrap_or(3.0);
    let (tch_ft, tch_angenommen) = match bahn.and_then(|b| b.tch_ft) {
        Some(t) => (t, false),
        None => (TCH_ANNAHME_FT, true),
    };
    let schwellenhoehe = bahn.and_then(|b| b.schwellenhoehe_ft);

    let mut gleitpfad = AnflugGleitpfad {
        quelle: quelle.to_string(),
        winkel_deg: winkel_deg as f32,
        tch_ft: tch_ft as f32,
        tch_angenommen,
        schwellenhoehe_ft: schwellenhoehe.map(|h| h as f32),
        versatz_ft: bahn.map(|b| b.versatz_ft as f32).unwrap_or(0.0),
        ..Default::default()
    };

    // Hoehenbezug der Baender: Schwelle, sonst Platz.
    let (bezug_ft, bezug_name) = match (schwellenhoehe, platzhoehe_ft) {
        (Some(s), _) => (Some(s), "schwelle"),
        (None, Some(p)) => (Some(p), "platz"),
        (None, None) => (None, "platz"),
    };

    let pfad = match (bahn, schwellenhoehe) {
        (None, _) => {
            gleitpfad.grund_ohne_werte = Some(GRUND_KEINE_BAHN.to_string());
            None
        }
        (Some(_), None) => {
            gleitpfad.grund_ohne_werte = Some(GRUND_SCHWELLENHOEHE_FEHLT.to_string());
            None
        }
        (Some(b), Some(_)) => Some(Pfad::neu(b, winkel_deg, tch_ft)),
    };

    let Some(bezug_ft) = bezug_ft else {
        return AnflugForensik {
            gleitpfad: Some(gleitpfad),
            ruhe: None,
        };
    };

    let punkte = punkte_im_anflug(buf, td, bezug_ft, pfad.as_ref());

    if pfad.is_some() {
        gleitpfad.gesamt = gleitpfad_tor(&punkte, HOEHE_UNTEN_FT, HOEHE_OBEN_FT);
        gleitpfad.tor_1000_500 = gleitpfad_tor(&punkte, HOEHE_MITTE_FT, HOEHE_OBEN_FT);
        gleitpfad.tor_500_200 = gleitpfad_tor(&punkte, HOEHE_UNTEN_FT, HOEHE_MITTE_FT);
        if gleitpfad.gesamt.is_none() {
            gleitpfad.grund_ohne_werte = Some(GRUND_KEINE_PROBEN.to_string());
        }
    }

    let ruhe_1000_500 = ruhe_tor(&punkte, HOEHE_MITTE_FT, HOEHE_OBEN_FT);
    let ruhe_500_200 = ruhe_tor(&punkte, HOEHE_UNTEN_FT, HOEHE_MITTE_FT);
    let ruhe = (ruhe_1000_500.is_some() || ruhe_500_200.is_some()).then(|| AnflugRuhe {
        hoehenbezug: bezug_name.to_string(),
        tor_1000_500: ruhe_1000_500,
        tor_500_200: ruhe_500_200,
    });

    AnflugForensik {
        gleitpfad: Some(gleitpfad),
        ruhe,
    }
}

/// Der Gleitpfad als Gerade durch die Schwelle in TCH-Hoehe.
struct Pfad {
    bahn: Bahnbezug,
    winkel_rad: f64,
    tch_ft: f64,
    achse_deg: f64,
}

impl Pfad {
    fn neu(bahn: &Bahnbezug, winkel_deg: f64, tch_ft: f64) -> Self {
        Self {
            achse_deg: peilung_deg(
                bahn.schwelle_lat,
                bahn.schwelle_lon,
                bahn.ende_lat,
                bahn.ende_lon,
            ),
            bahn: bahn.clone(),
            winkel_rad: winkel_deg.to_radians(),
            tch_ft,
        }
    }

    /// Abweichung (Dots, ft) einer Probe, oder `None`, wenn sie nicht vor
    /// der Landeschwelle im Anflugsektor liegt.
    ///
    /// # Welcher Winkel
    ///
    /// Ein ILS-Gleitweg ist eine WINKEL-Anzeige um den Gleitweg-Bezugspunkt
    /// (GPI) — dort, wo der Pfad die Bahnoberflaeche trifft, TCH / tan(θ)
    /// hinter der Schwelle. Der Ist-Winkel ist also
    /// `atan(h / (d + TCH/tan θ))` mit h = Hoehe ueber der Schwelle und d =
    /// Entfernung VOR der Schwelle; die Abweichung ist Ist-Winkel − θ. Wer
    /// genau auf der Geraden `h = TCH + d·tan θ` fliegt, hat damit exakt 0°
    /// — unabhaengig von der Entfernung. Der Winkel von der SCHWELLE aus
    /// (`atan((h−0)/d)`) waere dagegen auf dem Pfad nie 0 und liefe kurz vor
    /// der Schwelle gegen 90°.
    fn abweichung(&self, s: &ApproachBufferSample, hoehe_ft: f64) -> Option<(f64, f64)> {
        let (lat, lon) = (s.lat?, s.lon?);
        let (laengs_m, quer_m) = crate::runway::projiziere_auf_bahn(
            self.bahn.schwelle_lat,
            self.bahn.schwelle_lon,
            self.bahn.ende_lat,
            self.bahn.ende_lon,
            lat,
            lon,
        );
        // Entfernung VOR der Landeschwelle: die Navdaten-Schwelle, um den
        // Versatz Richtung Bahn verschoben.
        let d_m = self.bahn.versatz_ft / FT_JE_M - laengs_m;
        if d_m <= 0.0 {
            return None; // ueber oder hinter der Schwelle
        }
        if quer_m.abs() > d_m * SEKTOR_HALBWINKEL_DEG.to_radians().tan() {
            return None; // ausserhalb des Anflugsektors
        }
        if winkel_diff_deg(s.heading_true_deg as f64, self.achse_deg) > KURS_TOLERANZ_DEG {
            return None; // nicht in Anflugrichtung
        }
        let d_ft = d_m * FT_JE_M;
        let tan_w = self.winkel_rad.tan();
        let soll_ft = self.tch_ft + d_ft * tan_w;
        let abw_ft = hoehe_ft - soll_ft;
        let gpi_ft = self.tch_ft / tan_w;
        let ist_rad = hoehe_ft.atan2(d_ft + gpi_ft);
        let abw_deg = (ist_rad - self.winkel_rad).to_degrees();
        Some((abw_deg / GRAD_JE_DOT, abw_ft))
    }
}

fn punkte_im_anflug(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    td: Option<DateTime<Utc>>,
    bezug_ft: f64,
    pfad: Option<&Pfad>,
) -> Vec<Punkt> {
    let anker = match td.or_else(|| buf.back().map(|s| s.at)) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let fruehestens = anker - chrono::Duration::seconds(ANFLUG_FENSTER_S);
    buf.iter()
        .filter(|s| {
            s.at >= fruehestens
                && if td.is_some() {
                    s.at < anker
                } else {
                    s.at <= anker
                }
        })
        .filter_map(|s| {
            let hoehe_ft = s.msl_ft as f64 - bezug_ft;
            if !hoehe_ft.is_finite() || !(HOEHE_UNTEN_FT..=HOEHE_OBEN_FT).contains(&hoehe_ft) {
                return None;
            }
            let abw = pfad.and_then(|p| p.abweichung(s, hoehe_ft));
            Some(Punkt {
                at: s.at,
                hoehe_ft,
                abw_dots: abw.map(|a| a.0),
                abw_ft: abw.map(|a| a.1),
                pitch_deg: s.pitch_deg.map(f64::from).filter(|p| p.is_finite()),
                bank_deg: s.bank_deg as f64,
                n1_pct: s.n1_mittel_pct.map(f64::from).filter(|n| n.is_finite()),
            })
        })
        .collect()
}

/// Band `[unten, oben]`; das obere Tor schliesst die Mitte aus, damit
/// keine Probe in beiden Toren zaehlt.
fn im_band(p: &Punkt, unten: f64, oben: f64) -> bool {
    if unten >= HOEHE_MITTE_FT {
        p.hoehe_ft > unten && p.hoehe_ft <= oben
    } else {
        p.hoehe_ft >= unten && p.hoehe_ft <= oben
    }
}

fn gleitpfad_tor(punkte: &[Punkt], unten: f64, oben: f64) -> Option<GleitpfadTor> {
    let werte: Vec<(f64, f64)> = punkte
        .iter()
        .filter(|p| im_band(p, unten, oben))
        .filter_map(|p| Some((p.abw_dots?, p.abw_ft?)))
        .collect();
    if werte.len() < MIN_PROBEN_JE_TOR {
        return None;
    }
    let mittel = werte.iter().map(|w| w.0.abs()).sum::<f64>() / werte.len() as f64;
    let max_dots = werte
        .iter()
        .map(|w| w.0)
        .fold(0.0_f64, |a, b| if b.abs() > a.abs() { b } else { a });
    let max_ft = werte
        .iter()
        .map(|w| w.1)
        .fold(0.0_f64, |a, b| if b.abs() > a.abs() { b } else { a });
    Some(GleitpfadTor {
        proben: werte.len() as u32,
        mittel_abs_dots: mittel as f32,
        max_dots: max_dots as f32,
        max_abw_ft: max_ft as f32,
    })
}

fn ruhe_tor(punkte: &[Punkt], unten: f64, oben: f64) -> Option<RuheTor> {
    let band: Vec<&Punkt> = punkte.iter().filter(|p| im_band(p, unten, oben)).collect();
    if band.len() < MIN_PROBEN_JE_TOR {
        return None;
    }
    let dauer_s = sekunden(band[0].at, band[band.len() - 1].at);

    let abw: Vec<f64> = band.iter().filter_map(|p| p.abw_dots).collect();
    let pfad_vorzeichenwechsel =
        (abw.len() >= MIN_PROBEN_JE_TOR).then(|| seitenwechsel(&abw, PFAD_TOTBAND_DOTS));

    let nick: Vec<(DateTime<Utc>, f64)> = band
        .iter()
        .filter_map(|p| Some((p.at, p.pitch_deg?)))
        .collect();
    let roll: Vec<(DateTime<Utc>, f64)> = band.iter().map(|p| (p.at, p.bank_deg)).collect();

    let n1: Vec<(DateTime<Utc>, f64)> = band
        .iter()
        .filter_map(|p| Some((p.at, p.n1_pct?)))
        .collect();
    let schub_umkehr_pro_min = if n1.len() >= MIN_PROBEN_JE_TOR {
        let dauer = sekunden(n1[0].0, n1[n1.len() - 1].0);
        (dauer >= MIN_DAUER_SCHUB_S).then(|| {
            let werte: Vec<f64> = n1.iter().map(|w| w.1).collect();
            (umkehrungen(&werte, SCHUB_TOTBAND_PCT) as f64 / (dauer / 60.0)) as f32
        })
    } else {
        None
    };

    Some(RuheTor {
        proben: band.len() as u32,
        dauer_s: dauer_s as f32,
        pfad_vorzeichenwechsel,
        nick_unruhe_deg_s: raten_streuung(&nick).map(|v| v as f32),
        roll_unruhe_deg_s: raten_streuung(&roll).map(|v| v as f32),
        schub_umkehr_pro_min,
    })
}

fn sekunden(a: DateTime<Utc>, b: DateTime<Utc>) -> f64 {
    (b - a).num_milliseconds() as f64 / 1000.0
}

/// Wie oft die Folge die Seite wechselt (ueber/unter dem Pfad). Werte im
/// Totband gehoeren zu keiner Seite und aendern nichts.
fn seitenwechsel(werte: &[f64], totband: f64) -> u32 {
    let mut seite: Option<bool> = None;
    let mut wechsel = 0;
    for &w in werte {
        let jetzt = if w > totband {
            Some(true)
        } else if w < -totband {
            Some(false)
        } else {
            None
        };
        if let Some(j) = jetzt {
            if seite.is_some_and(|s| s != j) {
                wechsel += 1;
            }
            seite = Some(j);
        }
    }
    wechsel
}

/// Richtungswechsel mit Hysterese: eine Richtung gilt erst, wenn sich der
/// Wert vom letzten Extremwert um mehr als das Totband entfernt hat.
fn umkehrungen(werte: &[f64], totband: f64) -> u32 {
    let Some(&erster) = werte.first() else {
        return 0;
    };
    let mut extrem = erster;
    let mut richtung: Option<bool> = None; // true = steigend
    let mut anzahl = 0;
    for &w in &werte[1..] {
        match richtung {
            None => {
                if (w - extrem).abs() > totband {
                    richtung = Some(w > extrem);
                    extrem = w;
                }
            }
            Some(true) => {
                if w > extrem {
                    extrem = w;
                } else if extrem - w > totband {
                    anzahl += 1;
                    richtung = Some(false);
                    extrem = w;
                }
            }
            Some(false) => {
                if w < extrem {
                    extrem = w;
                } else if w - extrem > totband {
                    anzahl += 1;
                    richtung = Some(true);
                    extrem = w;
                }
            }
        }
    }
    anzahl
}

/// Streuung (Standardabweichung) der Aenderungsrate in °/s. Eine
/// gleichmaessige Drehung hat die Streuung 0 — gemessen wird das Hin und
/// Her, nicht das Einleiten einer Kurve oder das Abfangen.
fn raten_streuung(folge: &[(DateTime<Utc>, f64)]) -> Option<f64> {
    let raten: Vec<f64> = folge
        .windows(2)
        .filter_map(|w| {
            let dt = sekunden(w[0].0, w[1].0);
            (RATE_DT_MIN_S..=RATE_DT_MAX_S)
                .contains(&dt)
                .then(|| (w[1].1 - w[0].1) / dt)
        })
        .collect();
    if raten.len() < MIN_RATEN {
        return None;
    }
    let n = raten.len() as f64;
    let mittel = raten.iter().sum::<f64>() / n;
    let varianz = raten.iter().map(|r| (r - mittel).powi(2)).sum::<f64>() / n;
    Some(varianz.sqrt())
}

/// Anfangspeilung von A nach B in Grad (0–360).
fn peilung_deg(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dl = (lon2 - lon1).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

fn winkel_diff_deg(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::VecDeque;

    // Bahn von Sued nach Nord (Kurs 360°): Meridiane sind Grosskreise, die
    // Sollgeometrie laesst sich damit exakt hinschreiben. Anflug von Sueden.
    const SCHWELLE_LAT: f64 = 50.0;
    const SCHWELLE_LON: f64 = 8.0;
    const SCHWELLE_ELEV: f64 = 300.0;
    /// Meter je Breitengrad auf der Kugel, mit der `projiziere_auf_bahn`
    /// rechnet (R = 6 371 000 m).
    const M_JE_GRAD: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;

    fn td() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap()
    }

    fn bahn(ils: bool, tch: Option<f64>, versatz_ft: f64) -> Bahnbezug {
        Bahnbezug {
            schwelle_lat: SCHWELLE_LAT,
            schwelle_lon: SCHWELLE_LON,
            ende_lat: SCHWELLE_LAT + 3000.0 / M_JE_GRAD,
            ende_lon: SCHWELLE_LON,
            versatz_ft,
            schwellenhoehe_ft: Some(SCHWELLE_ELEV),
            tch_ft: tch,
            winkel_deg: 3.0,
            hat_ils: ils,
        }
    }

    /// Probe `d_ft` VOR dem Punkt `nullpunkt_ft` (Fuss nordwaerts ab der
    /// Navdaten-Schwelle) in `h_ft` ueber der Schwelle; `t_s` Sekunden vor
    /// dem Aufsetzen.
    fn probe(t_s: f64, nullpunkt_ft: f64, d_ft: f64, h_ft: f64) -> ApproachBufferSample {
        let nord_m = (nullpunkt_ft - d_ft) / FT_JE_M;
        ApproachBufferSample {
            at: td() - chrono::Duration::milliseconds((t_s * 1000.0) as i64),
            agl_ft: h_ft as f32,
            msl_ft: (SCHWELLE_ELEV + h_ft) as f32,
            gs_kt: 140.0,
            ias_kt: 140.0,
            vs_fpm: -700.0,
            bank_deg: 0.0,
            heading_true_deg: 0.0,
            gear_position: 1.0,
            flaps_position: 1.0,
            selected_runway: None,
            stall_warning: false,
            lat: Some(SCHWELLE_LAT + nord_m / M_JE_GRAD),
            lon: Some(SCHWELLE_LON),
            pitch_deg: Some(2.5),
            n1_mittel_pct: Some(60.0),
        }
    }

    /// Ein Anflug von 1000 bis 200 ft, Proben im Sekundentakt, jede Probe
    /// genau `winkel_abw_deg` ueber dem 3°-Pfad (vom GPI aus gesehen).
    fn anflug(nullpunkt_ft: f64, tch: f64, winkel_abw_deg: f64) -> VecDeque<ApproachBufferSample> {
        let w = 3.0_f64.to_radians();
        let gpi = tch / w.tan();
        let ist = (3.0 + winkel_abw_deg).to_radians();
        let mut buf = VecDeque::new();
        let n = 80;
        for i in 0..=n {
            let h = 1000.0 - 800.0 * i as f64 / n as f64;
            // h = (d + gpi) · tan(ist)  →  d = h / tan(ist) − gpi
            let d = h / ist.tan() - gpi;
            buf.push_back(probe((n - i) as f64 + 10.0, nullpunkt_ft, d, h));
        }
        buf
    }

    #[test]
    fn genau_auf_dem_pfad_ist_null_dots() {
        let b = bahn(true, Some(50.0), 0.0);
        let f = auswerten(&anflug(0.0, 50.0, 0.0), Some(&b), Some(td()), None);
        let g = f.gleitpfad.unwrap();
        assert_eq!(g.quelle, "navigraph_ils");
        assert_eq!(g.grund_ohne_werte, None);
        let ges = g.gesamt.unwrap();
        assert!(ges.proben >= 75, "fast alle Proben zaehlen: {}", ges.proben);
        assert!(
            ges.mittel_abs_dots < 0.01,
            "auf dem Pfad: {}",
            ges.mittel_abs_dots
        );
        assert!(ges.max_dots.abs() < 0.02, "auf dem Pfad: {}", ges.max_dots);
        assert!(
            ges.max_abw_ft.abs() < 3.0,
            "auf dem Pfad: {} ft",
            ges.max_abw_ft
        );
    }

    #[test]
    fn null_komma_35_grad_darueber_ist_plus_ein_dot() {
        let b = bahn(true, Some(50.0), 0.0);
        let f = auswerten(&anflug(0.0, 50.0, 0.35), Some(&b), Some(td()), None);
        let g = f.gleitpfad.unwrap();
        for tor in [
            g.gesamt.unwrap(),
            g.tor_1000_500.unwrap(),
            g.tor_500_200.unwrap(),
        ] {
            assert!((tor.max_dots - 1.0).abs() < 0.02, "max {}", tor.max_dots);
            assert!(
                (tor.mittel_abs_dots - 1.0).abs() < 0.02,
                "mittel {}",
                tor.mittel_abs_dots
            );
            assert!(tor.max_abw_ft > 0.0, "ueber dem Pfad = positive Fuss");
        }
        // Darunter: negatives Vorzeichen.
        let f = auswerten(&anflug(0.0, 50.0, -0.7), Some(&b), Some(td()), None);
        let ges = f.gleitpfad.unwrap().gesamt.unwrap();
        assert!(
            (ges.max_dots + 2.0).abs() < 0.03,
            "−0,7° = −2 Dots: {}",
            ges.max_dots
        );
        assert!(ges.max_abw_ft < 0.0);
    }

    #[test]
    fn versetzte_schwelle_verschiebt_den_nullpunkt() {
        // Die Navdaten-Schwelle ist der Bahnanfang, die Landeschwelle liegt
        // 1000 ft weiter. Der Pilot fliegt exakt auf den Pfad der
        // LANDEschwelle.
        let buf = anflug(1000.0, 50.0, 0.0);
        let mit = auswerten(
            &buf,
            Some(&bahn(true, Some(50.0), 1000.0)),
            Some(td()),
            None,
        );
        let ges = mit.gleitpfad.unwrap().gesamt.unwrap();
        assert!(
            ges.max_dots.abs() < 0.02,
            "gegen die Landeschwelle: {}",
            ges.max_dots
        );

        // Gegenprobe: gegen die Navdaten-Schwelle (1000 ft frueher) laege
        // derselbe Anflug deutlich ZU HOCH — sonst pruefte dieser Test nichts.
        let ohne = auswerten(&buf, Some(&bahn(true, Some(50.0), 0.0)), Some(td()), None);
        let ges = ohne.gleitpfad.unwrap().gesamt.unwrap();
        assert!(ges.max_dots > 0.3, "ohne Versatz zu hoch: {}", ges.max_dots);
    }

    #[test]
    fn probe_hinter_der_schwelle_zaehlt_nicht() {
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = VecDeque::new();
        // Sechs saubere Proben vor der Schwelle …
        for i in 0..6 {
            let h = 900.0 - 100.0 * i as f64;
            let d = (h - 50.0) / 3.0_f64.to_radians().tan();
            buf.push_back(probe(60.0 - i as f64, 0.0, d, h));
        }
        // … und drei in Bandhoehe UEBER der Bahn (z. B. Durchstarten im
        // Tiefflug) — jede davon waere grob „zu hoch".
        for i in 0..3 {
            buf.push_back(probe(
                20.0 - i as f64,
                0.0,
                -2000.0 - 500.0 * i as f64,
                400.0,
            ));
        }
        let g = auswerten(&buf, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap();
        let ges = g.gesamt.unwrap();
        assert_eq!(ges.proben, 6);
        assert!(ges.max_dots.abs() < 0.02, "{}", ges.max_dots);
    }

    #[test]
    fn probe_nach_dem_aufsetzen_und_vorheriger_anflug_zaehlen_nicht() {
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = anflug(0.0, 50.0, 0.0);
        // Ein frueherer Anflug (vor dem Durchstarten), weit zu hoch.
        buf.push_front(probe(900.0, 0.0, 10_000.0, 900.0));
        // Ein Wert nach dem Aufsetzen.
        let mut nach = probe(0.0, 0.0, 10_000.0, 900.0);
        nach.at = td() + chrono::Duration::seconds(5);
        buf.push_back(nach);
        let ges = auswerten(&buf, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap()
            .gesamt
            .unwrap();
        assert!(
            ges.max_dots.abs() < 0.02,
            "fremde Proben mitgezaehlt: {}",
            ges.max_dots
        );
    }

    #[test]
    fn ohne_navdaten_nur_quelle_und_keine_werte() {
        let f = auswerten(
            &anflug(0.0, 50.0, 0.0),
            None,
            Some(td()),
            Some(SCHWELLE_ELEV),
        );
        let g = f.gleitpfad.unwrap();
        assert_eq!(g.quelle, "angenommen_3grad");
        assert_eq!(g.grund_ohne_werte.as_deref(), Some("keine_bahn"));
        assert!(g.gesamt.is_none() && g.tor_1000_500.is_none() && g.tor_500_200.is_none());
        // Die Anflugruhe laeuft ueber die Platzhoehe weiter — nur ohne
        // Seitenwechsel, denn der braucht einen Pfad.
        let r = f.ruhe.unwrap();
        assert_eq!(r.hoehenbezug, "platz");
        assert_eq!(r.tor_500_200.unwrap().pfad_vorzeichenwechsel, None);
    }

    #[test]
    fn quelle_und_tch_folgen_den_navdaten_nicht_den_standardwerten() {
        // Bahn ohne ILS → navigraph_bahn.
        let g = auswerten(
            &anflug(0.0, 50.0, 0.0),
            Some(&bahn(false, Some(50.0), 0.0)),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert_eq!(g.quelle, "navigraph_bahn");
        assert!(!g.tch_angenommen);

        // TCH fehlt (Server schreibt 0) → 50 ft angenommen UND markiert.
        let nav: aeroacars_mqtt::navdata::NavRunway = serde_json::from_value(serde_json::json!({
            "designator": "36", "magnetic_course": 360.0, "true_course": 360.0,
            "length_ft": 9000,
            "threshold": {"lat": 50.0, "lon": 8.0, "elev_ft": 300},
            "end": {"lat": 50.03, "lon": 8.0, "elev_ft": 310},
            "ils": {"freq_mhz": 110.1, "course": 360.0, "category": 1},
            "glideslope_angle": 3.0, "tch_ft": 0
        }))
        .unwrap();
        let b = Bahnbezug::aus_navdaten(&nav, 0.0);
        let g = auswerten(&anflug(0.0, 50.0, 0.0), Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap();
        assert_eq!(g.quelle, "navigraph_ils");
        assert!(g.tch_angenommen);
        assert_eq!(g.tch_ft, 50.0);

        // Schwellenhoehe fehlt → Quelle ja, Werte nein, mit Grund.
        let mut ohne_hoehe = bahn(true, Some(50.0), 0.0);
        ohne_hoehe.schwellenhoehe_ft = None;
        let g = auswerten(&anflug(0.0, 50.0, 0.0), Some(&ohne_hoehe), Some(td()), None)
            .gleitpfad
            .unwrap();
        assert_eq!(g.grund_ohne_werte.as_deref(), Some("schwellenhoehe_fehlt"));
        assert!(g.gesamt.is_none());
    }

    #[test]
    fn seitenwechsel_zaehlt_nur_ausserhalb_des_totbands() {
        assert_eq!(seitenwechsel(&[0.5, 0.05, -0.05, -0.5, 0.5], 0.1), 2);
        // Rauschen um den Pfad ist kein Seitenwechsel.
        assert_eq!(seitenwechsel(&[0.05, -0.05, 0.08, -0.09, 0.0], 0.1), 0);
    }

    #[test]
    fn schub_umkehrungen_mit_hysterese() {
        assert_eq!(umkehrungen(&[60.0, 65.0, 60.0, 65.0], 2.0), 2);
        assert_eq!(umkehrungen(&[60.0, 61.0, 60.0, 61.5, 60.2], 2.0), 0);
        // Langsames Hochlaufen ueber viele kleine Schritte ist EINE Richtung.
        assert_eq!(umkehrungen(&[55.0, 56.0, 57.0, 58.0, 59.0, 60.0], 2.0), 0);
    }

    #[test]
    fn ruhe_je_tor_mit_pfad_nick_und_schub() {
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = anflug(0.0, 50.0, 0.0);
        for (i, s) in buf.iter_mut().enumerate() {
            // Unteres Tor (ab 500 ft): Nick pendelt ±1°, Schub springt
            // zwischen 55 und 65 %.
            if s.msl_ft as f64 - SCHWELLE_ELEV <= 500.0 {
                s.pitch_deg = Some(if i % 2 == 0 { 3.5 } else { 1.5 });
                s.n1_mittel_pct = Some(if (i / 4) % 2 == 0 { 55.0 } else { 65.0 });
            }
        }
        let r = auswerten(&buf, Some(&b), Some(td()), None).ruhe.unwrap();
        assert_eq!(r.hoehenbezug, "schwelle");
        let oben = r.tor_1000_500.unwrap();
        let unten = r.tor_500_200.unwrap();
        assert_eq!(oben.nick_unruhe_deg_s, Some(0.0), "gleichmaessiger Nick");
        assert_eq!(oben.schub_umkehr_pro_min, Some(0.0));
        assert_eq!(oben.pfad_vorzeichenwechsel, Some(0), "auf dem Pfad");
        assert!(
            unten.nick_unruhe_deg_s.unwrap() > 1.5,
            "{:?}",
            unten.nick_unruhe_deg_s
        );
        assert!(
            unten.schub_umkehr_pro_min.unwrap() > 5.0,
            "{:?}",
            unten.schub_umkehr_pro_min
        );
        assert_eq!(unten.roll_unruhe_deg_s, Some(0.0));
    }

    #[test]
    fn ohne_n1_kein_schubwert() {
        // X-Plane und Kolbenmotoren liefern kein N1 — dann fehlt der Wert,
        // statt 0 Umkehrungen zu behaupten.
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = anflug(0.0, 50.0, 0.0);
        for s in buf.iter_mut() {
            s.n1_mittel_pct = None;
        }
        let r = auswerten(&buf, Some(&b), Some(td()), None).ruhe.unwrap();
        assert_eq!(r.tor_1000_500.unwrap().schub_umkehr_pro_min, None);
        assert_eq!(r.tor_500_200.unwrap().schub_umkehr_pro_min, None);
    }

    #[test]
    fn analyse_json_bekommt_beide_befunde() {
        let b = bahn(true, Some(50.0), 0.0);
        let f = auswerten(&anflug(0.0, 50.0, 0.35), Some(&b), Some(td()), None);
        let mut analyse = serde_json::json!({ "vs_at_edge_fpm": -150.0 });
        f.in_analyse_json(&mut analyse);
        assert_eq!(analyse["vs_at_edge_fpm"], -150.0, "Bestand bleibt");
        assert_eq!(analyse["anflug_gleitpfad"]["quelle"], "navigraph_ils");
        let max = analyse["anflug_gleitpfad"]["gesamt"]["max_dots"]
            .as_f64()
            .unwrap();
        assert!((max - 1.0).abs() < 0.02);
        assert!(analyse["anflug_ruhe"]["tor_500_200"].is_object());
    }
}
