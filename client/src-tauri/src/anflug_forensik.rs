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
//! barometrisch angezeigte. Ein Kaltwetter- oder QNH-Fehler im Hoehenmesser
//! verschiebt diese Rechnung also nicht.
//!
//! Der Bezug ist trotzdem nicht blind die Schwellenhoehe der Navdaten: Der
//! Pilot fliegt gegen die Welt des SIMULATORS. Liegt deren Boden an der
//! Schwelle um mehr als 20 ft neben der Navigraph-Hoehe (Gelaendenetz,
//! Szenerie), gilt der Sim-Boden — sonst erschiene ein sauber geflogener
//! Anflug durchgehend „zu hoch" oder „zu tief" (QS 29.09.2026, Befund 4).

use chrono::{DateTime, Utc};
use landing_scoring::anflug_forensik::GleitpfadPunkt;
use storage::{AnflugGleitpfad, AnflugRuhe, GleitpfadTor, RuheTor, Scherung};

use crate::ApproachBufferSample;

// Ein Dot bei 3° Gleitwinkel: 0,35° — so rechnet vmsACARS (Vollausschlag
// 0,7° = 2 Dots). Die Anzeige eines echten Gleitwegs skaliert mit dem
// Winkel (ICAO Annex 10: Vollausschlag bei rund 0,12 × θ), deshalb gilt
// allgemein 0,35° × θ/3: bei 3° unveraendert, beim 5,5°-Anflug in London
// City rund 0,64°. Wir BENOTEN das nicht — die Dots sind nur die gewohnte
// Einheit.
// Wert seit v3: bundle/grenzwerte/anflug.json -> grad_je_dot_bei_3_grad (v2.1.24: 0.35)

pub(crate) fn grad_je_dot(winkel_deg: f64) -> f64 {
    crate::grenzwerte::grenzwerte().anflug.grad_je_dot_bei_3_grad.wert * winkel_deg / 3.0
}

// Obere Grenze des ausgewerteten Anflugs, ft ueber der Schwelle.
// Wert seit v3: bundle/grenzwerte/anflug.json -> hoehe_oben_ft (v2.1.24: 1000.0)
// Grenze zwischen den Toren 1000–500 und 500–200.
// Wert seit v3: bundle/grenzwerte/anflug.json -> hoehe_mitte_ft (v2.1.24: 500.0)
// Untere Grenze: 200 ft ist die Entscheidungshoehe eines CAT-I-Anflugs.
// Darunter beginnt der Uebergang in den Sichtflug und das Ausschweben —
// dort ist eine Pfadabweichung kein Anflugfehler mehr.
// Wert seit v3: bundle/grenzwerte/anflug.json -> hoehe_unten_ft (v2.1.24: 200.0)

// Nur Proben aus den letzten fuenf Minuten vor dem Aufsetzen. Von 1000 ft
// sinkt selbst ein langsamer GA-Anflug (300 fpm) in gut drei Minuten; ein
// VORHERIGER Anflug mit Durchstarten liegt dagegen fast immer weiter
// zurueck (Platzrunde) und darf nicht mitgerechnet werden.
// Wert seit v3: bundle/grenzwerte/anflug.json -> anflug_fenster_s (v2.1.24: 300)

// Halber Oeffnungswinkel des Gleitwegsektors, vom Gleitweg-Bezugspunkt
// aus gemessen. Ein echter Gleitwegsender deckt seitlich nur rund ±8° ab
// (ICAO Annex 10) — ausserhalb gibt es keine Gleitweganzeige, also auch
// keine Abweichung. ±10° laesst etwas Luft fuer die Lage des Senders.
// Die ANFLUGRUHE ist an keinen Sektor gebunden (sie braucht keine Bahn).
// Wert seit v3: bundle/grenzwerte/anflug.json -> gleitweg_sektor_halbwinkel_deg (v2.1.24: 10.0)

// „Grob in Anflugrichtung": mehr als 90° zwischen Steuerkurs und
// Bahnrichtung ist Gegen- oder Queranflug, keine Endanflugprobe.
// Wert seit v3: bundle/grenzwerte/anflug.json -> kurs_toleranz_deg (v2.1.24: 90.0)

// Mindestens so viele Proben je Band, sonst keine Kennwerte. Unter 1500 ft
// tastet der Puffer etwa sekuendlich ab; fuenf Proben sind fuenf Sekunden.
// Wert seit v3: bundle/grenzwerte/anflug.json -> min_proben_je_tor (v2.1.24: 5)

// Fehlt die TCH in den Navdaten (der Server schreibt dann 0), gilt 50 ft
// — der ICAO-Richtwert fuer Verkehrsflugzeuge. Die Unsicherheit ist
// klein: 10 ft TCH verschieben den Bezugspunkt um rund 190 ft entlang der
// Bahn, das ist bei 1 NM Entfernung weniger als 0,03°.
// Wert seit v3: bundle/grenzwerte/anflug.json -> tch_annahme_ft (v2.1.24: 50.0)

// Ab dieser Abweichung zwischen Sim-Boden und Navigraph-Schwellenhoehe
// gilt der Sim-Boden. 20 ft sind bei 1 NM rund 0,2° — fast ein Dot. Kleinere
// Unterschiede (Bahnneigung, Rundung der Navdaten auf ganze Fuss) sollen
// den Bezug nicht umschalten.
// Wert seit v3: bundle/grenzwerte/anflug.json -> sim_boden_toleranz_ft (v2.1.24: 20.0)
// Nur Proben UEBER DER BAHN verraten die Bodenhoehe der Sim-Bahn: hoechstens
// so weit HINTER der Landeschwelle …
//
// ⚠ Nicht davor (Nachpruefung 29.09.2026, Befund A): Vor der Schwelle
// liegt Gelaende, Wasser oder eine Klippe — KLGA rund 20 ft ueber dem
// Wasser, LPMA/TNCM/LXGB mit Meer vor der Bahn, dazu jede Senke im Mesh.
// Dort gemessen, schaltete der Bezug grundlos auf „Sim-Boden" und
// verschob den Pfad um fast einen Dot je 20 ft.
// Wert seit v3: bundle/grenzwerte/anflug.json -> sim_boden_laengs_m (v2.1.24: 600.0)
// … und hoechstens so weit neben der Achse (halbe Bahnbreite, mindestens
// 30 m, wenn die Breite unbekannt oder schmaler ist).
// Wert seit v3: bundle/grenzwerte/anflug.json -> sim_boden_quer_min_m (v2.1.24: 30.0)
// Median ueber die schwellennaechsten Proben — eine einzelne Probe ueber
// einem Gebaeude oder einer Bodenwelle soll den Bezug nicht bestimmen.
// Wert seit v3: bundle/grenzwerte/anflug.json -> sim_boden_proben (v2.1.24: 3)
// So viele Proben ueber dem ersten Bahnstueck braucht der Median.
// Wert seit v3: bundle/grenzwerte/anflug.json -> sim_boden_min_proben (v2.1.24: 2)

// Totband fuer den Seitenwechsel: erst ab ±0,1 Dot (bei 3° 0,035°) gilt
// eine Probe als „ueber" oder „unter" dem Pfad. Bei 2 NM sind das rund
// 7 ft — weniger als die Hoehe, um die ein Flugzeug zwischen zwei Proben
// schon durch normale Luftbewegung schwankt. Ohne Totband zaehlte jede
// Probe, die den Pfad nur streift, als Korrektur.
// Wert seit v3: bundle/grenzwerte/anflug.json -> pfad_totband_dots (v2.1.24: 0.1)

// Totband fuer Schub-Umkehrungen: 2 % N1. Eine Autothrottle- oder
// Hebelkorrektur im Endanflug bewegt N1 um 3–10 %; darunter liegen das
// Nachregeln der Triebwerksregelung und die Anzeigeaufloesung. Gezaehlt
// wird erst, wenn N1 sich vom letzten Extremwert um mehr als das Totband
// in die GEGENRICHTUNG bewegt hat (Hysterese).
// Wert seit v3: bundle/grenzwerte/anflug.json -> schub_totband_pct (v2.1.24: 2.0)

// Zeitabstand, in dem zwei Proben eine Nick-/Rollrate bilden duerfen.
// Unter 0,2 s dominiert das Zeitstempel-Rauschen, ueber 5 s ist es keine
// Rate mehr, sondern eine Luecke im Puffer.
// Wert seit v3: bundle/grenzwerte/anflug.json -> rate_dt_min_s (v2.1.24: 0.2)
// Wert seit v3: bundle/grenzwerte/anflug.json -> rate_dt_max_s (v2.1.24: 5.0)

// Eine Rate braucht so viele Paare fuer eine Streuung.
// Wert seit v3: bundle/grenzwerte/anflug.json -> min_raten (v2.1.24: 4)

// Unter dieser Dauer ist „Umkehrungen je Minute" keine Rate.
// Wert seit v3: bundle/grenzwerte/anflug.json -> min_dauer_schub_s (v2.1.24: 10.0)

// Windscherung (28.10.2026, Idee aus LocalTC, Schwellen nachgemessen):
// Gegenwind UND angezeigte Fahrt aendern sich in 4–5,5 s gleichsinnig um je
// mindestens 15 kt, zwischen 30 und 1500 ft ueber dem Bezug.
//
// Gemessen ueber 1857 Client-Flugprotokolle: 3 Treffer, alle Airliner
// (XRNDLE A321 in 1480 ft: Gegenwind −31 kt, Fahrt −29 kt). Der Gegenwind
// kommt aus dem Simulator (`aircraft_wind_z_kt`), nicht aus TAS − GS —
// letzteres meldete auch einen Kampfjet im Sturzflug und eine C172 mit
// −10 000 fpm. Dass sich die Fahrt MIT dem Wind aendert, trennt die
// Scherung vom Piloten, der Schub gibt oder wegnimmt. Boeen, die schneller
// hin- und herspringen (q3B30), erfasst die Regel bewusst nicht.
// Wert seit v3: bundle/grenzwerte/anflug.json -> scherung_min_kt (v2.1.24: 15.0)
// Wert seit v3: bundle/grenzwerte/anflug.json -> scherung_dt_min_s (v2.1.24: 4.0)
// Wert seit v3: bundle/grenzwerte/anflug.json -> scherung_dt_max_s (v2.1.24: 5.5)
// Wert seit v3: bundle/grenzwerte/anflug.json -> scherung_hoehe_unten_ft (v2.1.24: 30.0)
// Wert seit v3: bundle/grenzwerte/anflug.json -> scherung_hoehe_oben_ft (v2.1.24: 1500.0)
// Treffer innerhalb dieser Zeit gehoeren zu EINER Scherung; gemeldet wird
// die staerkste.
// Wert seit v3: bundle/grenzwerte/anflug.json -> scherung_entprellen_s (v2.1.24: 60.0)
// Hoechstens so viele, die staerksten: Budget unter 256 Bytes
// (`zwei_scherungen_bleiben_unter_256_bytes` in landing-scoring). Gemessen
// kam nie mehr als eine je Flug vor.
// Wert seit v3: bundle/grenzwerte/anflug.json -> scherungen_max (v2.1.24: 2)

pub(crate) const QUELLE_NAVIGRAPH_ILS: &str = "navigraph_ils";
pub(crate) const QUELLE_NAVIGRAPH_BAHN: &str = "navigraph_bahn";
pub(crate) const QUELLE_ANGENOMMEN_3GRAD: &str = "angenommen_3grad";

const GRUND_KEINE_BAHN: &str = "keine_bahn";
const GRUND_SCHWELLENHOEHE_FEHLT: &str = "schwellenhoehe_fehlt";
const GRUND_KEINE_PROBEN: &str = "keine_proben";

const SCHUB_GRUND_KEIN_N1: &str = "kein_n1";
const SCHUB_GRUND_ZU_KURZ: &str = "zu_kurz";

const HOEHENBEZUG_NAVIGRAPH: &str = "navigraph";
const HOEHENBEZUG_SIM_BODEN: &str = "sim_boden";

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
    /// Halbe Bahnbreite in Metern, mindestens `SIM_BODEN_QUER_MIN_M` —
    /// nur fuer die Sim-Boden-Messung ueber der Bahn.
    pub halbe_breite_m: f64,
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
            halbe_breite_m: nav
                .width_ft
                .map(|w| w as f64 / FT_JE_M / 2.0)
                .unwrap_or(0.0)
                .max(crate::grenzwerte::grenzwerte().anflug.sim_boden_quer_min_m.wert),
        }
    }

    /// Laengs- und Querabstand einer Position zur LANDEschwelle, in Metern.
    /// Laengs positiv in Landerichtung (hinter der Schwelle), negativ davor.
    fn zur_landeschwelle_m(&self, lat: f64, lon: f64) -> (f64, f64) {
        let (laengs_m, quer_m) = crate::runway::projiziere_auf_bahn(
            self.schwelle_lat,
            self.schwelle_lon,
            self.ende_lat,
            self.ende_lon,
            lat,
            lon,
        );
        (laengs_m - self.versatz_ft / FT_JE_M, quer_m)
    }
}

/// Eine Probe, fertig fuer beide Auswertungen.
#[derive(Debug, Clone, Copy)]
struct Punkt {
    at: DateTime<Utc>,
    hoehe_ft: f64,
    /// Pfadabweichung in Dots / ft — `None`, wenn die Probe nicht vor der
    /// Schwelle im Gleitwegsektor lag oder es keinen Pfad gibt.
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
/// * `buf`: der Forensik-Puffer (`FlightStats::anflug_forensik_puffer`),
///   ggf. schon auf ein Zeitfenster begrenzt.
/// * `bahn`: Navdaten der gelandeten Bahn; `None` → Quelle
///   `angenommen_3grad`, KEINE Gleitpfad-Werte und auch kein Winkel/TCH
///   (ohne Schwelle gibt es nichts zu rechnen — erfinden wir keine).
/// * `td`: Aufsetzzeitpunkt; nur Proben davor zaehlen.
/// * `platzhoehe_ft`: Rueckfall fuer die Hoehenbaender der Anflugruhe,
///   wenn es keinen Schwellenbezug gibt (dieselbe Bezugshoehe wie das
///   Stabilitaets-Gate).
// Seit Score-Version 23 rufen nur noch Tests die Fassung ohne spaetes Final.
#[cfg(test)]
pub(crate) fn auswerten(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    bahn: Option<&Bahnbezug>,
    td: Option<DateTime<Utc>>,
    platzhoehe_ft: Option<f64>,
) -> AnflugForensik {
    auswerten_ab_final(buf, bahn, td, platzhoehe_ft, None)
}

/// Wie `auswerten`; mit `final_ab` (Score-Version 23, spaetes Final /
/// Kurvenanflug) zaehlen fuer die Gleitpfad-TORE nur Proben ab dem
/// Ausrichten auf die Bahn — vorher liegt das Flugzeug verfahrensbedingt
/// neben und ueber dem Pfad. Der Verlauf (Dot-Streifen) und die Anflugruhe
/// bleiben vollstaendig.
pub(crate) fn auswerten_ab_final(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    bahn: Option<&Bahnbezug>,
    td: Option<DateTime<Utc>>,
    platzhoehe_ft: Option<f64>,
    final_ab: Option<DateTime<Utc>>,
) -> AnflugForensik {
    let mut gleitpfad = AnflugGleitpfad {
        quelle: quelle(bahn).to_string(),
        ..Default::default()
    };
    let proben = im_fenster(buf, td);

    // Pfad und Hoehenbezug — nur mit Bahn.
    let mut pfad: Option<Pfad> = None;
    let mut schwellenbezug: Option<(f64, &'static str)> = None;
    match bahn {
        None => gleitpfad.grund_ohne_werte = Some(GRUND_KEINE_BAHN.to_string()),
        Some(b) => {
            // Dieselbe Plausibilisierung wie ueberall im Client (2,0–7,5°).
            let winkel_deg = Some(b.winkel_deg)
                .filter(|g| (2.0..=7.5).contains(g))
                .unwrap_or(3.0);
            let (tch_ft, tch_angenommen) = match b.tch_ft {
                Some(t) => (t, false),
                None => (crate::grenzwerte::grenzwerte().anflug.tch_annahme_ft.wert, true),
            };
            gleitpfad.winkel_deg = Some(winkel_deg as f32);
            gleitpfad.tch_ft = Some(tch_ft as f32);
            gleitpfad.tch_angenommen = tch_angenommen;
            gleitpfad.grad_je_dot = Some(runden(grad_je_dot(winkel_deg), 4));
            gleitpfad.versatz_ft = Some(b.versatz_ft as f32);
            gleitpfad.schwellenhoehe_navigraph_ft = b.schwellenhoehe_ft.map(|h| h as f32);
            let sim_boden = sim_boden_an_schwelle(&proben, b);
            gleitpfad.sim_boden_ft = sim_boden.map(|h| runden(h, 1));
            let bezug = match (b.schwellenhoehe_ft, sim_boden) {
                (Some(n), Some(s)) if (s - n).abs() > crate::grenzwerte::grenzwerte().anflug.sim_boden_toleranz_ft.wert => {
                    Some((s, HOEHENBEZUG_SIM_BODEN))
                }
                (Some(n), _) => Some((n, HOEHENBEZUG_NAVIGRAPH)),
                // Navdaten ohne Schwellenhoehe: der Sim-Boden ist dann der
                // einzige Bezug — und der, gegen den geflogen wurde.
                (None, Some(s)) => Some((s, HOEHENBEZUG_SIM_BODEN)),
                (None, None) => None,
            };
            match bezug {
                Some((hoehe, name)) => {
                    gleitpfad.hoehenbezug = Some(name.to_string());
                    pfad = Some(Pfad::neu(b, hoehe, winkel_deg, tch_ft));
                    schwellenbezug = Some((hoehe, name));
                }
                None => {
                    gleitpfad.grund_ohne_werte = Some(GRUND_SCHWELLENHOEHE_FEHLT.to_string());
                }
            }
        }
    }

    // Hoehenbezug der Baender: Schwelle (Navigraph oder Sim-Boden), sonst Platz.
    let bezug = match (schwellenbezug, platzhoehe_ft) {
        (Some((h, HOEHENBEZUG_SIM_BODEN)), _) => Some((h, "sim_boden")),
        (Some((h, _)), _) => Some((h, "schwelle")),
        (None, Some(p)) => Some((p, "platz")),
        (None, None) => None,
    };
    let Some((bezug_ft, bezug_name)) = bezug else {
        return AnflugForensik {
            gleitpfad: Some(gleitpfad),
            ruhe: None,
        };
    };

    let punkte = punkte_im_anflug(&proben, bezug_ft, pfad.as_ref());

    if pfad.is_some() {
        let tor_punkte: Vec<Punkt> = match final_ab {
            Some(ab) => punkte.iter().filter(|p| p.at >= ab).cloned().collect(),
            None => punkte.clone(),
        };
        gleitpfad.gesamt = gleitpfad_tor(&tor_punkte, crate::grenzwerte::grenzwerte().anflug.hoehe_unten_ft.wert, crate::grenzwerte::grenzwerte().anflug.hoehe_oben_ft.wert);
        gleitpfad.tor_1000_500 = gleitpfad_tor(&tor_punkte, crate::grenzwerte::grenzwerte().anflug.hoehe_mitte_ft.wert, crate::grenzwerte::grenzwerte().anflug.hoehe_oben_ft.wert);
        gleitpfad.tor_500_200 = gleitpfad_tor(&tor_punkte, crate::grenzwerte::grenzwerte().anflug.hoehe_unten_ft.wert, crate::grenzwerte::grenzwerte().anflug.hoehe_mitte_ft.wert);
        if gleitpfad.gesamt.is_none() {
            gleitpfad.grund_ohne_werte = Some(GRUND_KEINE_PROBEN.to_string());
        }
    }
    if let (Some(p), Some(t)) = (pfad.as_ref(), td) {
        gleitpfad.verlauf = gleitpfad_verlauf(&proben, bezug_ft, p, t);
    }

    let ruhe_1000_500 = ruhe_tor(&punkte, crate::grenzwerte::grenzwerte().anflug.hoehe_mitte_ft.wert, crate::grenzwerte::grenzwerte().anflug.hoehe_oben_ft.wert);
    let ruhe_500_200 = ruhe_tor(&punkte, crate::grenzwerte::grenzwerte().anflug.hoehe_unten_ft.wert, crate::grenzwerte::grenzwerte().anflug.hoehe_mitte_ft.wert);
    let scherungen = scherungen(&proben, bezug_ft, td);
    let ruhe = (ruhe_1000_500.is_some() || ruhe_500_200.is_some() || !scherungen.is_empty())
        .then(|| AnflugRuhe {
            hoehenbezug: bezug_name.to_string(),
            tor_1000_500: ruhe_1000_500,
            tor_500_200: ruhe_500_200,
            scherungen,
        });

    AnflugForensik {
        gleitpfad: Some(gleitpfad),
        ruhe,
    }
}

/// Proben vor dem Aufsetzen, hoechstens `ANFLUG_FENSTER_S` alt.
fn im_fenster(
    buf: &std::collections::VecDeque<ApproachBufferSample>,
    td: Option<DateTime<Utc>>,
) -> Vec<&ApproachBufferSample> {
    let Some(anker) = td.or_else(|| buf.back().map(|s| s.at)) else {
        return Vec::new();
    };
    let fruehestens = anker - chrono::Duration::seconds(crate::grenzwerte::grenzwerte().anflug.anflug_fenster_s.wert);
    buf.iter()
        .filter(|s| {
            s.at >= fruehestens
                && if td.is_some() {
                    s.at < anker
                } else {
                    s.at <= anker
                }
        })
        .collect()
}

/// Bodenhoehe der Sim-BAHN an der Landeschwelle: `msl − agl` der (bis zu
/// drei) schwellennaechsten Proben ueber dem ersten Bahnstueck (0–600 m
/// hinter der Landeschwelle, innerhalb der Bahnbreite), Median; mindestens
/// zwei Proben. Sonst ersatzweise die juengste Probe in genau diesem
/// Stueck — nie eine weiter hinten auf der Bahn: Der Aufsetzpunkt liegt oft
/// 400–1200 m hinter der Schwelle, bei 1–1,5 % Bahnneigung sind das
/// 13–40 ft, und der Bezug schaltete faelschlich um (Cloud-QS 29.09.2026).
/// `None`, wenn keine Probe ueber dem ersten Bahnstueck liegt.
fn sim_boden_an_schwelle(proben: &[&ApproachBufferSample], bahn: &Bahnbezug) -> Option<f64> {
    let (bahn_ende_m, _) = bahn.zur_landeschwelle_m(bahn.ende_lat, bahn.ende_lon);
    // (laengs hinter der Landeschwelle, Bodenhoehe, Zeit)
    let ueber_bahn: Vec<(f64, f64, DateTime<Utc>)> = proben
        .iter()
        .filter_map(|s| {
            let (lat, lon) = (s.lat?, s.lon?);
            let boden = s.msl_ft as f64 - s.agl_ft as f64;
            if !boden.is_finite() {
                return None;
            }
            let (laengs_m, quer_m) = bahn.zur_landeschwelle_m(lat, lon);
            (laengs_m >= 0.0 && laengs_m <= bahn_ende_m && quer_m.abs() <= bahn.halbe_breite_m)
                .then_some((laengs_m, boden, s.at))
        })
        .collect();
    let nah_mit_zeit: Vec<(f64, f64, DateTime<Utc>)> = ueber_bahn
        .into_iter()
        .filter(|p| p.0 <= crate::grenzwerte::grenzwerte().anflug.sim_boden_laengs_m.wert)
        .collect();
    // Ersatz bei weniger als zwei Proben: die juengste im selben Stueck.
    let ersatz = nah_mit_zeit.iter().max_by_key(|p| p.2).map(|p| p.1);
    let mut nah: Vec<(f64, f64)> = nah_mit_zeit.iter().map(|p| (p.0, p.1)).collect();
    if nah.len() >= crate::grenzwerte::grenzwerte().anflug.sim_boden_min_proben.wert {
        nah.sort_by(|a, b| a.0.total_cmp(&b.0));
        nah.truncate(crate::grenzwerte::grenzwerte().anflug.sim_boden_proben.wert);
        let mut boeden: Vec<f64> = nah.into_iter().map(|n| n.1).collect();
        boeden.sort_by(|a, b| a.total_cmp(b));
        let n = boeden.len();
        return Some(if n % 2 == 1 {
            boeden[n / 2]
        } else {
            (boeden[n / 2 - 1] + boeden[n / 2]) / 2.0
        });
    }
    ersatz
}

/// Der Gleitpfad als Gerade durch die Landeschwelle in TCH-Hoehe.
struct Pfad {
    bahn: Bahnbezug,
    bezug_ft: f64,
    winkel_rad: f64,
    tch_ft: f64,
    grad_je_dot: f64,
    achse_deg: f64,
}

impl Pfad {
    fn neu(bahn: &Bahnbezug, bezug_ft: f64, winkel_deg: f64, tch_ft: f64) -> Self {
        Self {
            achse_deg: peilung_deg(
                bahn.schwelle_lat,
                bahn.schwelle_lon,
                bahn.ende_lat,
                bahn.ende_lon,
            ),
            bahn: bahn.clone(),
            bezug_ft,
            winkel_rad: winkel_deg.to_radians(),
            tch_ft,
            grad_je_dot: grad_je_dot(winkel_deg),
        }
    }

    /// Abweichung (Dots, ft) einer Probe, oder `None`, wenn sie nicht vor
    /// der Landeschwelle im Gleitwegsektor liegt.
    ///
    /// # Welcher Winkel
    ///
    /// Ein ILS-Gleitweg ist eine WINKEL-Anzeige um den Gleitweg-Bezugspunkt
    /// (GPI) — dort, wo der Pfad die Bahnoberflaeche trifft, TCH / tan(θ)
    /// hinter der Schwelle. Gerechnet wird deshalb der Hoehenwinkel vom GPI
    /// aus: `atan(h / r)` mit h = Hoehe ueber der Schwelle und
    /// r = `hypot(d + GPI, quer)` (d = Entfernung VOR der Schwelle, quer =
    /// seitlicher Versatz). Die Abweichung ist dieser Winkel − θ. Wer genau
    /// auf der Geraden `h = TCH + d·tan θ` fliegt, hat damit exakt 0° —
    /// unabhaengig von der Entfernung. Das ist die Geometrie, die ein
    /// Gleitwegsender abbildet, nicht dessen Messung: die tatsaechliche
    /// Antennenlage (seitlich neben der Bahn) und die Form seines Strahls
    /// kennen die Navdaten nicht. Der Winkel von der SCHWELLE aus
    /// (`atan(h/d)`) waere auf dem Pfad nie 0 und liefe kurz vor der
    /// Schwelle gegen 90°.
    fn abweichung(&self, s: &ApproachBufferSample) -> Option<(f64, f64)> {
        let (lat, lon) = (s.lat?, s.lon?);
        let (laengs_m, quer_m) = self.bahn.zur_landeschwelle_m(lat, lon);
        let d_ft = -laengs_m * FT_JE_M;
        if d_ft <= 0.0 {
            return None; // ueber oder hinter der Schwelle
        }
        if winkel_diff_deg(s.heading_true_deg as f64, self.achse_deg) > crate::grenzwerte::grenzwerte().anflug.kurs_toleranz_deg.wert {
            return None; // nicht in Anflugrichtung
        }
        let tan_w = self.winkel_rad.tan();
        let gpi_ft = self.tch_ft / tan_w;
        let laengs_ab_gpi_ft = d_ft + gpi_ft;
        let quer_ft = quer_m * FT_JE_M;
        if quer_ft.abs().atan2(laengs_ab_gpi_ft).to_degrees() > crate::grenzwerte::grenzwerte().anflug.gleitweg_sektor_halbwinkel_deg.wert {
            return None; // ausserhalb des Gleitwegsektors
        }
        let r_ft = laengs_ab_gpi_ft.hypot(quer_ft);
        let hoehe_ft = s.msl_ft as f64 - self.bezug_ft;
        let abw_ft = hoehe_ft - r_ft * tan_w;
        let abw_deg = (hoehe_ft.atan2(r_ft) - self.winkel_rad).to_degrees();
        Some((abw_deg / self.grad_je_dot, abw_ft))
    }
}

fn punkte_im_anflug(
    proben: &[&ApproachBufferSample],
    bezug_ft: f64,
    pfad: Option<&Pfad>,
) -> Vec<Punkt> {
    proben
        .iter()
        .filter_map(|s| {
            let hoehe_ft = s.msl_ft as f64 - bezug_ft;
            if !hoehe_ft.is_finite() || !(crate::grenzwerte::grenzwerte().anflug.hoehe_unten_ft.wert..=crate::grenzwerte::grenzwerte().anflug.hoehe_oben_ft.wert).contains(&hoehe_ft) {
                return None;
            }
            let abw = pfad.and_then(|p| p.abweichung(s));
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

/// Mindestabstand zweier Punkte der Gleitpfad-Kurve.
const VERLAUF_TAKT_S: f64 = 1.0;

/// Die Gleitpfad-Kurve fuer den Dot-Streifen der Anfluggrafik: von 1000 ft
/// bis zur Schwelle (danach gibt es keinen Pfad mehr), hoechstens ein Punkt
/// je Sekunde. Dieselbe Abweichung wie in den Toren, nur nicht gemittelt.
fn gleitpfad_verlauf(
    proben: &[&ApproachBufferSample],
    bezug_ft: f64,
    pfad: &Pfad,
    td: DateTime<Utc>,
) -> Vec<GleitpfadPunkt> {
    let mut raus: Vec<GleitpfadPunkt> = Vec::new();
    let mut letzte: Option<DateTime<Utc>> = None;
    for s in proben {
        let hoehe_ft = s.msl_ft as f64 - bezug_ft;
        if !hoehe_ft.is_finite() || !(0.0..=crate::grenzwerte::grenzwerte().anflug.hoehe_oben_ft.wert).contains(&hoehe_ft) {
            continue;
        }
        if letzte.is_some_and(|l| sekunden(l, s.at) < VERLAUF_TAKT_S) {
            continue;
        }
        let Some((dots, _)) = pfad.abweichung(s) else {
            continue;
        };
        letzte = Some(s.at);
        raus.push(GleitpfadPunkt {
            t: runden(sekunden(td, s.at), 1),
            h: runden(hoehe_ft, 0),
            d: runden(dots, 2),
        });
    }
    raus
}

/// Band `[unten, oben]`; das obere Tor schliesst die Mitte aus, damit
/// keine Probe in beiden Toren zaehlt.
fn im_band(p: &Punkt, unten: f64, oben: f64) -> bool {
    if unten >= crate::grenzwerte::grenzwerte().anflug.hoehe_mitte_ft.wert {
        p.hoehe_ft > unten && p.hoehe_ft <= oben
    } else {
        p.hoehe_ft >= unten && p.hoehe_ft <= oben
    }
}

fn gleitpfad_tor(punkte: &[Punkt], unten: f64, oben: f64) -> Option<GleitpfadTor> {
    // (Dots, ft, Hoehe) JE PROBE — Dots und Fuss der groessten Abweichung
    // muessen von derselben Probe stammen (QS 29.09.2026, Befund 1: zwei
    // getrennte Maxima zeigten „−1,2 Dots (+60 ft)").
    let werte: Vec<(f64, f64, f64)> = punkte
        .iter()
        .filter(|p| im_band(p, unten, oben))
        .filter_map(|p| Some((p.abw_dots?, p.abw_ft?, p.hoehe_ft)))
        .collect();
    if werte.len() < crate::grenzwerte::grenzwerte().anflug.min_proben_je_tor.wert {
        return None;
    }
    let mittel = werte.iter().map(|w| w.0.abs()).sum::<f64>() / werte.len() as f64;
    let groesste = werte.iter().copied().fold((0.0_f64, 0.0_f64), |a, w| {
        if w.0.abs() > a.0.abs() {
            (w.0, w.1)
        } else {
            a
        }
    });
    let oberste = werte.iter().map(|w| w.2).fold(f64::MIN, f64::max);
    Some(GleitpfadTor {
        proben: werte.len() as u32,
        mittel_abs_dots: runden(mittel, 2),
        max_dots: runden(groesste.0, 2),
        max_abw_ft: runden(groesste.1, 1),
        oberste_hoehe_ft: Some(runden(oberste, 0)),
    })
}

fn ruhe_tor(punkte: &[Punkt], unten: f64, oben: f64) -> Option<RuheTor> {
    let band: Vec<&Punkt> = punkte.iter().filter(|p| im_band(p, unten, oben)).collect();
    if band.len() < crate::grenzwerte::grenzwerte().anflug.min_proben_je_tor.wert {
        return None;
    }
    let dauer_s = sekunden(band[0].at, band[band.len() - 1].at);
    let oberste = band.iter().map(|p| p.hoehe_ft).fold(f64::MIN, f64::max);

    let abw: Vec<f64> = band.iter().filter_map(|p| p.abw_dots).collect();
    let pfad_vorzeichenwechsel =
        (abw.len() >= crate::grenzwerte::grenzwerte().anflug.min_proben_je_tor.wert).then(|| seitenwechsel(&abw, crate::grenzwerte::grenzwerte().anflug.pfad_totband_dots.wert));

    let nick: Vec<(DateTime<Utc>, f64)> = band
        .iter()
        .filter_map(|p| Some((p.at, p.pitch_deg?)))
        .collect();
    let roll: Vec<(DateTime<Utc>, f64)> = band.iter().map(|p| (p.at, p.bank_deg)).collect();

    let n1: Vec<(DateTime<Utc>, f64)> = band
        .iter()
        .filter_map(|p| Some((p.at, p.n1_pct?)))
        .collect();
    let (schub_umkehr_pro_min, schub_grund) = if n1.is_empty() {
        (None, Some(SCHUB_GRUND_KEIN_N1))
    } else {
        let dauer = sekunden(n1[0].0, n1[n1.len() - 1].0);
        if n1.len() < crate::grenzwerte::grenzwerte().anflug.min_proben_je_tor.wert || dauer < crate::grenzwerte::grenzwerte().anflug.min_dauer_schub_s.wert {
            (None, Some(SCHUB_GRUND_ZU_KURZ))
        } else {
            let werte: Vec<f64> = n1.iter().map(|w| w.1).collect();
            let rate = umkehrungen(&werte, crate::grenzwerte::grenzwerte().anflug.schub_totband_pct.wert) as f64 / (dauer / 60.0);
            (Some(runden(rate, 2)), None)
        }
    };

    Some(RuheTor {
        proben: band.len() as u32,
        dauer_s: runden(dauer_s, 1),
        oberste_hoehe_ft: Some(runden(oberste, 0)),
        pfad_vorzeichenwechsel,
        nick_unruhe_deg_s: raten_streuung(&nick).map(|v| runden(v, 2)),
        roll_unruhe_deg_s: raten_streuung(&roll).map(|v| runden(v, 2)),
        schub_umkehr_pro_min,
        schub_grund: schub_grund.map(str::to_string),
    })
}

/// Windscherungen im Anflug, siehe `SCHERUNG_MIN_KT`. Je Probe wird mit der
/// Probe 4–5,5 s davor verglichen; Proben ohne Gegenwind zaehlen nicht.
fn scherungen(
    proben: &[&ApproachBufferSample],
    bezug_ft: f64,
    td: Option<DateTime<Utc>>,
) -> Vec<Scherung> {
    let anker = td.or_else(|| proben.last().map(|s| s.at));
    let mut treffer: Vec<(DateTime<Utc>, Scherung)> = Vec::new();
    for (i, b) in proben.iter().enumerate() {
        let hoehe_ft = b.msl_ft as f64 - bezug_ft;
        let Some(wind_b) = b.headwind_kt.map(f64::from) else { continue };
        if !(crate::grenzwerte::grenzwerte().anflug.scherung_hoehe_unten_ft.wert..=crate::grenzwerte::grenzwerte().anflug.scherung_hoehe_oben_ft.wert).contains(&hoehe_ft) {
            continue;
        }
        let vorher = proben[..i].iter().rev().find(|a| {
            let dt = sekunden(a.at, b.at);
            (crate::grenzwerte::grenzwerte().anflug.scherung_dt_min_s.wert..=crate::grenzwerte::grenzwerte().anflug.scherung_dt_max_s.wert).contains(&dt) && a.headwind_kt.is_some()
        });
        let Some(a) = vorher else { continue };
        let wind = wind_b - a.headwind_kt.map(f64::from).unwrap_or(wind_b);
        let fahrt = b.ias_kt as f64 - a.ias_kt as f64;
        // `!(… >= …)` statt `< …`: ein NaN faellt so heraus, statt zu zaehlen.
        if !(wind.abs() >= crate::grenzwerte::grenzwerte().anflug.scherung_min_kt.wert && fahrt.abs() >= crate::grenzwerte::grenzwerte().anflug.scherung_min_kt.wert && wind * fahrt > 0.0) {
            continue;
        }
        let s = Scherung {
            t: anker.map_or(0.0, |t| runden(-sekunden(b.at, t), 1)),
            hoehe_ft: runden(hoehe_ft, 0),
            gegenwind_kt: runden(wind, 0),
            fahrt_kt: runden(fahrt, 0),
            dauer_s: runden(sekunden(a.at, b.at), 1),
        };
        match treffer.last_mut() {
            Some((zuletzt, alt)) if sekunden(*zuletzt, b.at) <= crate::grenzwerte::grenzwerte().anflug.scherung_entprellen_s.wert => {
                if s.gegenwind_kt.abs() > alt.gegenwind_kt.abs() {
                    *alt = s;
                }
                *zuletzt = b.at;
            }
            _ => treffer.push((b.at, s)),
        }
    }
    // Die staerksten behalten (bei Gleichstand die fruehere), in zeitlicher
    // Reihenfolge.
    let mut nach_staerke: Vec<(usize, Scherung)> =
        treffer.into_iter().map(|(_, s)| s).enumerate().collect();
    nach_staerke.sort_by(|(i, x), (j, y)| {
        y.gegenwind_kt
            .abs()
            .partial_cmp(&x.gegenwind_kt.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(i.cmp(j))
    });
    nach_staerke.truncate(crate::grenzwerte::grenzwerte().anflug.scherungen_max.wert);
    nach_staerke.sort_by_key(|(i, _)| *i);
    nach_staerke.into_iter().map(|(_, s)| s).collect()
}

/// Rundet fuer die Ablage. Die Anzeige braucht hoechstens eine
/// Nachkommastelle; ungerundete f32 („0.42341217") machten beide Bloecke im
/// PIREP ueber 1 KB gross, ohne eine einzige Information mehr zu tragen.
fn runden(v: f64, stellen: i32) -> f32 {
    let f = 10f64.powi(stellen);
    ((v * f).round() / f) as f32
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
/// gleichmaessige Drehung (konstante Rate) hat die Streuung 0. Das Ein- und
/// Ausleiten einer Kurve oder das Abfangen ist aber eine Aenderung der Rate
/// und ZAEHLT mit — der Wert misst Bewegung um die Achse, nicht nur ein
/// Pendeln. Deshalb ist er ein Hinweis, keine Note.
fn raten_streuung(folge: &[(DateTime<Utc>, f64)]) -> Option<f64> {
    let raten: Vec<f64> = folge
        .windows(2)
        .filter_map(|w| {
            let dt = sekunden(w[0].0, w[1].0);
            (crate::grenzwerte::grenzwerte().anflug.rate_dt_min_s.wert..=crate::grenzwerte::grenzwerte().anflug.rate_dt_max_s.wert)
                .contains(&dt)
                .then(|| (w[1].1 - w[0].1) / dt)
        })
        .collect();
    if raten.len() < crate::grenzwerte::grenzwerte().anflug.min_raten.wert {
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
pub(crate) mod tests {
    use super::*;
    use chrono::TimeZone;
    use std::collections::VecDeque;

    // Standardbahn von Sued nach Nord (Kurs 360°): Meridiane sind
    // Grosskreise, die Sollgeometrie laesst sich damit exakt hinschreiben.
    // Anflug von Sueden. Eine Ost-West-Bahn hat ihren eigenen Test.
    const SCHWELLE_LAT: f64 = 50.0;
    const SCHWELLE_LON: f64 = 8.0;
    const SCHWELLE_ELEV: f64 = 300.0;
    /// Meter je Breitengrad auf der Kugel, mit der `projiziere_auf_bahn`
    /// rechnet (R = 6 371 000 m).
    const M_JE_GRAD: f64 = 6_371_000.0 * std::f64::consts::PI / 180.0;
    const FT_JE_NM: f64 = 1852.0 / 0.3048;

    fn td() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 29, 12, 0, 0).unwrap()
    }

    fn bahn_mit(ils: bool, tch: Option<f64>, versatz_ft: f64, winkel: f64) -> Bahnbezug {
        Bahnbezug {
            schwelle_lat: SCHWELLE_LAT,
            schwelle_lon: SCHWELLE_LON,
            ende_lat: SCHWELLE_LAT + 3000.0 / M_JE_GRAD,
            ende_lon: SCHWELLE_LON,
            versatz_ft,
            schwellenhoehe_ft: Some(SCHWELLE_ELEV),
            tch_ft: tch,
            winkel_deg: winkel,
            hat_ils: ils,
            halbe_breite_m: 30.0,
        }
    }

    fn bahn(ils: bool, tch: Option<f64>, versatz_ft: f64) -> Bahnbezug {
        bahn_mit(ils, tch, versatz_ft, 3.0)
    }

    /// Freie Probe: Position, Kurs, wahre Hoehe und Sim-Bodenhoehe.
    fn probe_bei(
        t_s: f64,
        lat: f64,
        lon: f64,
        kurs: f32,
        boden: f64,
        h_ft: f64,
    ) -> ApproachBufferSample {
        ApproachBufferSample {
            at: td() - chrono::Duration::milliseconds((t_s * 1000.0) as i64),
            agl_ft: h_ft as f32,
            msl_ft: (boden + h_ft) as f32,
            gs_kt: 140.0,
            ias_kt: 140.0,
            vs_fpm: -700.0,
            bank_deg: 0.0,
            heading_true_deg: kurs,
            gear_position: 1.0,
            flaps_position: 1.0,
            selected_runway: None,
            stall_warning: false,
            lat: Some(lat),
            lon: Some(lon),
            pitch_deg: Some(2.5),
            n1_mittel_pct: Some(60.0),
            headwind_kt: None,
            crosswind_kt: None,
        }
    }

    /// Probe `d_ft` VOR dem Punkt `nullpunkt_ft` (Fuss nordwaerts ab der
    /// Navdaten-Schwelle) in `h_ft` ueber der Schwelle; `t_s` Sekunden vor
    /// dem Aufsetzen.
    fn probe(t_s: f64, nullpunkt_ft: f64, d_ft: f64, h_ft: f64) -> ApproachBufferSample {
        let nord_m = (nullpunkt_ft - d_ft) / FT_JE_M;
        probe_bei(
            t_s,
            SCHWELLE_LAT + nord_m / M_JE_GRAD,
            SCHWELLE_LON,
            0.0,
            SCHWELLE_ELEV,
            h_ft,
        )
    }

    /// Ein Anflug von 1000 bis 200 ft, Proben im Sekundentakt, jede Probe
    /// genau `winkel_abw_deg` ueber dem Pfad mit Winkel `winkel` (vom GPI
    /// aus gesehen).
    fn anflug_mit(
        nullpunkt_ft: f64,
        tch: f64,
        winkel: f64,
        winkel_abw_deg: f64,
    ) -> VecDeque<ApproachBufferSample> {
        let gpi = tch / winkel.to_radians().tan();
        let ist = (winkel + winkel_abw_deg).to_radians();
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

    fn anflug(nullpunkt_ft: f64, tch: f64, winkel_abw_deg: f64) -> VecDeque<ApproachBufferSample> {
        anflug_mit(nullpunkt_ft, tch, 3.0, winkel_abw_deg)
    }

    #[test]
    fn genau_auf_dem_pfad_ist_null_dots() {
        let b = bahn(true, Some(50.0), 0.0);
        let f = auswerten(&anflug(0.0, 50.0, 0.0), Some(&b), Some(td()), None);
        let g = f.gleitpfad.unwrap();
        assert_eq!(g.quelle, "navigraph_ils");
        assert_eq!(g.grund_ohne_werte, None);
        assert_eq!(g.hoehenbezug.as_deref(), Some("navigraph"));
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

    /// Unabhaengig von der GPI-Formel: Hoehen per Hand gerechnet,
    /// h = 50 ft + d · tan 3° (tan 3° = 0,0524078), auf 0,1 ft gerundet.
    #[test]
    fn handgerechnete_punkte_bis_drei_meilen_liegen_auf_dem_pfad() {
        let b = bahn(true, Some(50.0), 0.0);
        let punkte = [
            (2.75, 925.7),
            (2.5, 846.1),
            (2.0, 686.9),
            (1.5, 527.7),
            (1.0, 368.4),
        ];
        let buf: VecDeque<_> = punkte
            .iter()
            .enumerate()
            .map(|(i, (nm, h))| probe(60.0 - 10.0 * i as f64, 0.0, nm * FT_JE_NM, *h))
            .collect();
        let ges = auswerten(&buf, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap()
            .gesamt
            .unwrap();
        assert_eq!(ges.proben, 5);
        assert!(ges.max_dots.abs() < 0.005, "{}", ges.max_dots);
        assert!(ges.max_abw_ft.abs() < 0.2, "{} ft", ges.max_abw_ft);

        // 30 ft hoeher bei 2 NM (12 152 ft vor dem GPI-Bezug + 954 ft):
        // atan(716,9 / 13 106) = 3,131° → +0,131° = +0,37 Dots.
        let mut hoch = buf.clone();
        hoch[2].msl_ft += 30.0;
        let ges = auswerten(&hoch, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap()
            .gesamt
            .unwrap();
        assert!((ges.max_dots - 0.374).abs() < 0.01, "{}", ges.max_dots);
        assert!((ges.max_abw_ft - 30.0).abs() < 0.2, "{}", ges.max_abw_ft);
    }

    /// Dot-Streifen (05.10.2026): die Kurve je Probe — gleiche Dots wie die
    /// Tore, Zeit relativ zum Aufsetzen, hoechstens eine Probe je Sekunde.
    #[test]
    fn verlauf_traegt_dots_je_probe_im_sekundentakt() {
        let b = bahn(true, Some(50.0), 0.0);
        let g = auswerten(&anflug(0.0, 50.0, 0.35), Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap();
        assert_eq!(
            g.verlauf.len(),
            81,
            "jede Sekundenprobe von 1000 bis 200 ft"
        );
        let erster = &g.verlauf[0];
        assert_eq!((erster.t, erster.h), (-90.0, 1000.0));
        let letzter = g.verlauf.last().unwrap();
        assert_eq!((letzter.t, letzter.h), (-10.0, 200.0));
        for p in &g.verlauf {
            assert!(
                (p.d - 1.0).abs() < 0.02,
                "0,35 deg darueber = +1 Dot: {p:?}"
            );
        }
        // Doppelt so dichte Proben: der Takt duennt auf eine je Sekunde aus.
        let dicht: VecDeque<_> = (0..=160)
            .map(|i| {
                let h = 1000.0 - 800.0 * i as f64 / 160.0;
                let d = h / (3.35f64).to_radians().tan() - 50.0 / (3.0f64).to_radians().tan();
                probe(90.0 - i as f64 * 0.5, 0.0, d, h)
            })
            .collect();
        let v = auswerten(&dicht, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap()
            .verlauf;
        assert_eq!(v.len(), 81, "0,5-s-Proben auf 1 s ausgeduennt");
        // Ohne Bahn gibt es keinen Pfad und keine Kurve.
        let ohne = auswerten(&anflug(0.0, 50.0, 0.0), None, Some(td()), None);
        assert!(ohne.gleitpfad.unwrap().verlauf.is_empty());
    }

    #[test]
    fn null_komma_35_grad_darueber_ist_plus_ein_dot() {
        let b = bahn(true, Some(50.0), 0.0);
        let f = auswerten(&anflug(0.0, 50.0, 0.35), Some(&b), Some(td()), None);
        let g = f.gleitpfad.unwrap();
        assert_eq!(g.grad_je_dot, Some(0.35));
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
    fn steilanflug_skaliert_den_dot_mit_dem_winkel() {
        // London City: 5,5°. Ein Dot = 0,35 × 5,5/3 = 0,6417°.
        let b = bahn_mit(true, Some(50.0), 0.0, 5.5);
        let g = auswerten(
            &anflug_mit(0.0, 50.0, 5.5, 0.6417),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert!((g.grad_je_dot.unwrap() - 0.6417).abs() < 0.001);
        let ges = g.gesamt.unwrap();
        assert!((ges.max_dots - 1.0).abs() < 0.02, "{}", ges.max_dots);
        // 0,35° ueber einem 5,5°-Pfad ist nur gut ein halber Dot.
        let ges = auswerten(
            &anflug_mit(0.0, 50.0, 5.5, 0.35),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap()
        .gesamt
        .unwrap();
        assert!((ges.max_dots - 0.545).abs() < 0.02, "{}", ges.max_dots);
    }

    #[test]
    fn groesste_abweichung_nennt_dots_und_fuss_derselben_probe() {
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = anflug(0.0, 50.0, 0.0);
        // Oben weit draussen +60 ft: kleiner Winkel (≈ +0,5 Dots) …
        let oben = buf.iter().position(|s| s.agl_ft <= 900.0).unwrap();
        buf[oben].msl_ft += 60.0;
        // … unten nah an der Schwelle −45 ft: grosser Winkel (≈ −1,5 Dots).
        let unten = buf.iter().position(|s| s.agl_ft <= 250.0).unwrap();
        buf[unten].msl_ft -= 45.0;
        let ges = auswerten(&buf, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap()
            .gesamt
            .unwrap();
        assert!(
            ges.max_dots < -1.4,
            "Dot-Maximum ist die untere Probe: {}",
            ges.max_dots
        );
        assert!(
            (ges.max_abw_ft + 45.0).abs() < 0.5,
            "Fuss derselben Probe, nicht das +60-ft-Maximum: {}",
            ges.max_abw_ft
        );
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

    /// Seitlicher Versatz: 20° neben der Achse (vom GPI aus) liegt ausserhalb
    /// jedes Gleitwegsenders — kein Gleitpfadwert, aber die Anflugruhe zaehlt
    /// die Proben weiter. 5° daneben auf dem Pfad (Abstand ueber `hypot`)
    /// ergibt 0 Dots.
    #[test]
    fn querversatz_20_grad_faellt_aus_dem_gleitpfad_aber_nicht_aus_der_ruhe() {
        let b = bahn(true, Some(50.0), 0.0);
        let tan3 = 3.0_f64.to_radians().tan();
        let gpi = 50.0 / tan3;
        let versetzt = |seitwinkel: f64| -> VecDeque<ApproachBufferSample> {
            (0..=40)
                .map(|i| {
                    let h = 1000.0 - 20.0 * i as f64;
                    let r = h / tan3; // Abstand vom GPI auf dem Pfad
                    let laengs = r * seitwinkel.to_radians().cos() - gpi;
                    let quer_m = r * seitwinkel.to_radians().sin() / FT_JE_M;
                    let lat = SCHWELLE_LAT - laengs / FT_JE_M / M_JE_GRAD;
                    let lon = SCHWELLE_LON + quer_m / (M_JE_GRAD * SCHWELLE_LAT.to_radians().cos());
                    probe_bei(50.0 - i as f64, lat, lon, 0.0, SCHWELLE_ELEV, h)
                })
                .collect()
        };
        let f = auswerten(&versetzt(20.0), Some(&b), Some(td()), None);
        let g = f.gleitpfad.unwrap();
        assert!(g.gesamt.is_none());
        assert_eq!(g.grund_ohne_werte.as_deref(), Some("keine_proben"));
        let r = f.ruhe.expect("Ruhe braucht keinen Sektor");
        assert!(r.tor_1000_500.unwrap().nick_unruhe_deg_s.is_some());

        let ges = auswerten(&versetzt(5.0), Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap()
            .gesamt
            .unwrap();
        assert!(ges.proben >= 38, "{}", ges.proben);
        assert!(ges.max_dots.abs() < 0.03, "{}", ges.max_dots);
    }

    #[test]
    fn gegenkurs_ist_keine_endanflugprobe() {
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = anflug(0.0, 50.0, 0.0);
        for s in buf.iter_mut() {
            s.heading_true_deg = 180.0;
        }
        let g = auswerten(&buf, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap();
        assert!(g.gesamt.is_none());
        assert_eq!(g.grund_ohne_werte.as_deref(), Some("keine_proben"));
    }

    #[test]
    fn ost_west_bahn_rechnet_genauso() {
        // Bahn 27: Schwelle im Osten, Landerichtung West (270°), Anflug von
        // Osten ueber einen Breitenkreis.
        let m_je_grad_lon = M_JE_GRAD * SCHWELLE_LAT.to_radians().cos();
        let b = Bahnbezug {
            ende_lat: SCHWELLE_LAT,
            ende_lon: SCHWELLE_LON - 3000.0 / m_je_grad_lon,
            ..bahn(true, Some(50.0), 0.0)
        };
        let tan3 = 3.0_f64.to_radians().tan();
        let buf: VecDeque<_> = (0..=40)
            .map(|i| {
                let h = 1000.0 - 20.0 * i as f64;
                let d_ft = (h - 50.0) / tan3;
                let lon = SCHWELLE_LON + d_ft / FT_JE_M / m_je_grad_lon;
                probe_bei(50.0 - i as f64, SCHWELLE_LAT, lon, 270.0, SCHWELLE_ELEV, h)
            })
            .collect();
        let ges = auswerten(&buf, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap()
            .gesamt
            .unwrap();
        assert!(ges.proben >= 38, "{}", ges.proben);
        assert!(ges.max_dots.abs() < 0.02, "{}", ges.max_dots);
    }

    /// Anflug 1000 → 60 ft genau auf dem Pfad der SIM-Bahn (Bodenhoehe
    /// `bahn_boden`); das Gelaende VOR der Schwelle liegt auf `vorfeld`
    /// (dort misst `agl` gegen das Vorfeld). Mit `ueber_bahn` kommen
    /// `n_bahn` Proben ueber dem ersten Bahnstueck dazu (50 … 550 m hinter
    /// der Schwelle, 30 … 10 ft).
    fn mit_boden(bahn_boden: f64, vorfeld: f64, n_bahn: usize) -> VecDeque<ApproachBufferSample> {
        let tan3 = 3.0_f64.to_radians().tan();
        let mut buf = VecDeque::new();
        let mut h = 1000.0;
        let mut t = 100.0;
        while h >= 60.0 {
            let d_ft = (h - 50.0) / tan3;
            let lat = SCHWELLE_LAT - d_ft / FT_JE_M / M_JE_GRAD;
            let mut p = probe_bei(t, lat, SCHWELLE_LON, 0.0, bahn_boden, h);
            p.agl_ft = (bahn_boden + h - vorfeld) as f32;
            buf.push_back(p);
            h -= 10.0;
            t -= 1.0;
        }
        for i in 0..n_bahn {
            let laengs_m = 50.0 + 100.0 * i as f64;
            let lat = SCHWELLE_LAT + laengs_m / M_JE_GRAD;
            let hoehe = 30.0 - 4.0 * i as f64;
            buf.push_back(probe_bei(t, lat, SCHWELLE_LON, 0.0, bahn_boden, hoehe));
            t -= 1.0;
        }
        buf
    }

    /// Befund 4: Die Sim-Bahn liegt 30 ft ueber der Navigraph-Hoehe; der
    /// Pilot fliegt sauber gegen die Sim-Welt.
    #[test]
    fn sim_boden_30_ft_daneben_wird_zum_bezug() {
        let b = bahn(true, Some(50.0), 0.0);
        let sim_boden = SCHWELLE_ELEV + 30.0;
        let g = auswerten(
            &mit_boden(sim_boden, sim_boden, 4),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert_eq!(g.hoehenbezug.as_deref(), Some("sim_boden"));
        assert_eq!(g.schwellenhoehe_navigraph_ft, Some(300.0));
        assert_eq!(g.sim_boden_ft, Some(330.0));
        assert!(g.gesamt.unwrap().max_dots.abs() < 0.02);

        // Ersatz: nur EINE Probe ueber der Bahn (Aufsetzpunkt) reicht dann.
        let g = auswerten(
            &mit_boden(sim_boden, sim_boden, 1),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert_eq!(g.hoehenbezug.as_deref(), Some("sim_boden"));

        // Gegenprobe 1: ohne Proben ueber der Bahn bleibt Navigraph der
        // Bezug — und derselbe Anflug liegt dann 30 ft zu hoch.
        let g = auswerten(
            &mit_boden(sim_boden, sim_boden, 0),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert_eq!(g.hoehenbezug.as_deref(), Some("navigraph"));
        assert_eq!(g.sim_boden_ft, None);
        assert!((g.gesamt.unwrap().max_abw_ft - 30.0).abs() < 0.5);

        // Gegenprobe 2: 10 ft Unterschied schalten nicht um.
        let g = auswerten(
            &mit_boden(SCHWELLE_ELEV + 10.0, SCHWELLE_ELEV + 10.0, 4),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert_eq!(g.hoehenbezug.as_deref(), Some("navigraph"));
        assert_eq!(g.sim_boden_ft, Some(310.0));
    }

    /// Befund A der Nachpruefung: Gelaende (Wasser, Senke) VOR der Schwelle
    /// liegt 40 ft tiefer, die Bahn selbst auf Navigraph-Hoehe. Die Proben
    /// ueber dem Vorfeld duerfen den Bezug NICHT umschalten.
    #[test]
    fn gelaende_vor_der_schwelle_schaltet_den_bezug_nicht_um() {
        let b = bahn(true, Some(50.0), 0.0);
        let g = auswerten(
            &mit_boden(SCHWELLE_ELEV, SCHWELLE_ELEV - 40.0, 4),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert_eq!(g.hoehenbezug.as_deref(), Some("navigraph"));
        assert_eq!(g.sim_boden_ft, Some(300.0), "gemessen ueber der Bahn");
        assert!(g.gesamt.unwrap().max_dots.abs() < 0.02);
        // Gegenprobe: ganz ohne Bahnproben gibt es auch keinen Sim-Boden —
        // das Vorfeld allein zaehlt nie.
        let g = auswerten(
            &mit_boden(SCHWELLE_ELEV, SCHWELLE_ELEV - 40.0, 0),
            Some(&b),
            Some(td()),
            None,
        )
        .gleitpfad
        .unwrap();
        assert_eq!(g.sim_boden_ft, None);
    }

    /// Cloud-QS 29.09.2026 (zweite Nachpruefung): Liegt ueber dem ersten
    /// Bahnstueck keine Probe, darf der Ersatz NICHT vom Aufsetzpunkt weiter
    /// hinten kommen. Hier steigt die Bahn bis 900 m um 30 ft (geneigte
    /// Bahn) — der Bezug muss bei Navigraph bleiben.
    #[test]
    fn geneigte_bahn_aufsetzpunkt_schaltet_den_bezug_nicht_um() {
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = mit_boden(SCHWELLE_ELEV, SCHWELLE_ELEV, 0);
        buf.push_back(probe_bei(
            0.5,
            SCHWELLE_LAT + 900.0 / M_JE_GRAD,
            SCHWELLE_LON,
            0.0,
            SCHWELLE_ELEV + 30.0,
            2.0,
        ));
        let g = auswerten(&buf, Some(&b), Some(td()), None)
            .gleitpfad
            .unwrap();
        assert_eq!(g.hoehenbezug.as_deref(), Some("navigraph"));
        assert_eq!(g.sim_boden_ft, None);
        assert!(g.gesamt.unwrap().max_dots.abs() < 0.02);
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
        // Nichts, was eine Rechnung vortaeuscht.
        assert_eq!(g.winkel_deg, None);
        assert_eq!(g.tch_ft, None);
        assert_eq!(g.grad_je_dot, None);
        assert_eq!(g.hoehenbezug, None);
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
        assert_eq!(g.tch_ft, Some(50.0));

        // Schwellenhoehe fehlt und kein Sim-Boden an der Schwelle → Quelle
        // ja, Werte nein, mit Grund.
        let mut ohne_hoehe = bahn(true, Some(50.0), 0.0);
        ohne_hoehe.schwellenhoehe_ft = None;
        let g = auswerten(&anflug(0.0, 50.0, 0.0), Some(&ohne_hoehe), Some(td()), None)
            .gleitpfad
            .unwrap();
        assert_eq!(g.grund_ohne_werte.as_deref(), Some("schwellenhoehe_fehlt"));
        assert!(g.gesamt.is_none());
    }

    #[test]
    fn oberste_hoehe_zeigt_ein_unvollstaendig_erfasstes_tor() {
        let b = bahn(true, Some(50.0), 0.0);
        // Der Puffer beginnt erst bei 700 ft.
        let buf: VecDeque<_> = anflug(0.0, 50.0, 0.0)
            .into_iter()
            .filter(|s| s.agl_ft <= 700.0)
            .collect();
        let f = auswerten(&buf, Some(&b), Some(td()), None);
        let g = f.gleitpfad.unwrap();
        let oben = g.tor_1000_500.unwrap().oberste_hoehe_ft.unwrap();
        assert!((oben - 700.0).abs() < 1.0, "{}", oben);
        let unten = g.tor_500_200.unwrap().oberste_hoehe_ft.unwrap();
        assert!((unten - 500.0).abs() < 1.0, "{}", unten);
        let r = f
            .ruhe
            .unwrap()
            .tor_1000_500
            .unwrap()
            .oberste_hoehe_ft
            .unwrap();
        assert!((r - 700.0).abs() < 1.0, "{}", r);
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
        assert_eq!(oben.schub_grund, None);
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
    fn schubgrund_unterscheidet_kein_n1_und_zu_kurz() {
        let b = bahn(true, Some(50.0), 0.0);
        // X-Plane und Kolbenmotoren liefern kein N1 — dann fehlt der Wert,
        // statt 0 Umkehrungen zu behaupten.
        let mut buf = anflug(0.0, 50.0, 0.0);
        for s in buf.iter_mut() {
            s.n1_mittel_pct = None;
        }
        let r = auswerten(&buf, Some(&b), Some(td()), None).ruhe.unwrap();
        let oben = r.tor_1000_500.unwrap();
        assert_eq!(oben.schub_umkehr_pro_min, None);
        assert_eq!(oben.schub_grund.as_deref(), Some("kein_n1"));

        // N1 nur in drei Proben des oberen Tors → gemessen, aber zu kurz.
        let mut n = 0;
        for s in buf.iter_mut() {
            if s.agl_ft > 500.0 && n < 3 {
                s.n1_mittel_pct = Some(60.0);
                n += 1;
            }
        }
        let r = auswerten(&buf, Some(&b), Some(td()), None).ruhe.unwrap();
        assert_eq!(
            r.tor_1000_500.unwrap().schub_grund.as_deref(),
            Some("zu_kurz")
        );
        assert_eq!(
            r.tor_500_200.unwrap().schub_grund.as_deref(),
            Some("kein_n1")
        );
    }

    /// Was der Client wirklich ablegt, passt in das PIREP-Budget (< 1 KB
    /// fuer beide Bloecke) — auch bei krummen Messwerten und mit allen
    /// Feldern belegt (Sim-Boden, drei Tore, Schub).
    #[test]
    fn echte_auswertung_bleibt_gerundet_unter_einem_kilobyte() {
        let b = bahn(true, Some(50.0), 0.0);
        let mut buf = anflug(0.0, 50.0, 0.1234567);
        for (i, s) in buf.iter_mut().enumerate() {
            s.msl_ft += (i as f32 * 0.731).sin() * 7.3;
            s.pitch_deg = Some(2.5 + (i as f32 * 1.37).sin() * 0.83);
            s.bank_deg = (i as f32 * 0.91).cos() * 1.77;
            s.n1_mittel_pct = Some(58.0 + (i as f32 * 0.43).sin() * 4.1);
            // Sim-Boden 33,3 ft ueber Navigraph (nur msl − agl zaehlt).
            s.agl_ft -= 33.3;
        }
        // Proben ueber dem ersten Bahnstueck fuer den Sim-Boden.
        for (i, (d, h)) in [(-300.0, 60.0), (-900.0, 50.0), (-1500.0, 45.0)]
            .iter()
            .enumerate()
        {
            let mut p = probe(8.0 - i as f64, 0.0, *d, *h);
            p.agl_ft -= 33.3;
            buf.push_back(p);
        }
        let f = auswerten(&buf, Some(&b), Some(td()), None);
        let g = f.gleitpfad.unwrap();
        assert_eq!(g.hoehenbezug.as_deref(), Some("sim_boden"));
        let r = f.ruhe.unwrap();
        assert!(r
            .tor_500_200
            .as_ref()
            .unwrap()
            .schub_umkehr_pro_min
            .is_some());
        let tor = g.gesamt.as_ref().unwrap();
        assert_eq!(
            tor.max_dots,
            (tor.max_dots * 100.0).round() / 100.0,
            "auf 0,01 gerundet"
        );
        // Die Kurve (Dot-Streifen, 05.10.2026) reist mit, ist aber eine
        // eigene Groesse: hoechstens ein Punkt je Sekunde ab 1000 ft. Die
        // Zusammenfassung bleibt unter 1 KB wie bisher.
        let kurve = serde_json::to_string(&g.verlauf).unwrap().len();
        let ohne_kurve = AnflugGleitpfad {
            verlauf: Vec::new(),
            ..g.clone()
        };
        let laenge = serde_json::to_string(&ohne_kurve).unwrap().len()
            + serde_json::to_string(&r).unwrap().len();
        assert!(laenge < 1024, "{laenge} Bytes");
        assert!(!g.verlauf.is_empty());
        let je_punkt = kurve / g.verlauf.len();
        assert!(je_punkt <= 32, "{je_punkt} Bytes je Punkt ({kurve} gesamt)");
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

    // ── Windscherung (28.10.2026) ────────────────────────────────────

    /// Probe ohne Bahnbezug: Hoehe ueber Platz 0 ft, Fahrt und Gegenwind frei.
    fn wind_probe(t_s: f64, h_ft: f64, ias: f32, gegenwind: Option<f32>) -> ApproachBufferSample {
        let mut p = probe_bei(t_s, SCHWELLE_LAT, SCHWELLE_LON, 0.0, 0.0, h_ft);
        p.ias_kt = ias;
        p.headwind_kt = gegenwind;
        p
    }

    fn scherungen_aus(proben: Vec<ApproachBufferSample>) -> Vec<Scherung> {
        let buf: VecDeque<_> = proben.into_iter().collect();
        auswerten(&buf, None, Some(td()), Some(0.0))
            .ruhe
            .map(|r| r.scherungen)
            .unwrap_or_default()
    }

    /// Ruhiger Anflug im Sekundentakt, 1400 → 400 ft, 140 kt, 20 kt Gegenwind.
    fn ruhig() -> Vec<ApproachBufferSample> {
        (0..=100)
            .map(|i| wind_probe(160.0 - i as f64, 1400.0 - 10.0 * i as f64, 140.0, Some(20.0)))
            .collect()
    }

    #[test]
    fn scherung_xrndle_echte_werte() {
        // XRNDLE A321 (29.09.2026), Client-Protokoll im Sekundenraster:
        // (Sekunde, ft, IAS, aircraft_wind_z_kt). Gegenwind = −wind_z.
        let roh = [
            (0.0, 1604.0, 160.0, -46.0),
            (1.0, 1574.0, 158.0, -44.0),
            (3.0, 1561.0, 156.0, -44.0),
            (4.0, 1561.0, 155.0, -42.0),
            (6.0, 1572.0, 155.0, -42.0),
            (7.0, 1559.0, 153.0, -41.0),
            (9.0, 1539.0, 137.0, -23.0),
            (10.0, 1516.0, 126.0, -11.0),
            (12.0, 1480.0, 125.0, -10.0),
            (13.0, 1461.0, 132.0, -16.0),
            (14.0, 1426.0, 134.0, -19.0),
            (15.0, 1403.0, 140.0, -21.0),
            (16.0, 1370.0, 144.0, -24.0),
            (17.0, 1320.0, 142.0, -19.0),
        ];
        let proben = roh
            .iter()
            .map(|&(s, h, ias, wz)| wind_probe(100.0 - s, h, ias as f32, Some(-(wz as f32))))
            .collect();
        let s = scherungen_aus(proben);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].hoehe_ft, 1480.0);
        assert_eq!(s[0].gegenwind_kt, -31.0);
        assert_eq!(s[0].fahrt_kt, -28.0);
        assert_eq!(s[0].dauer_s, 5.0);
        assert_eq!(s[0].t, -88.0);
    }

    #[test]
    fn scherung_ruhiger_anflug_hat_keine() {
        assert!(scherungen_aus(ruhig()).is_empty());
    }

    #[test]
    fn scherung_fahrt_ohne_wind_ist_der_pilot() {
        // 30 kt Fahrtverlust in 5 s bei gleichem Wind: Schub weg, keine Scherung.
        let mut p = ruhig();
        for q in p.iter_mut().skip(50) {
            q.ias_kt = 110.0;
        }
        assert!(scherungen_aus(p).is_empty());
    }

    #[test]
    fn scherung_wind_und_fahrt_gegensinnig_zaehlt_nicht() {
        let mut p = ruhig();
        for q in p.iter_mut().skip(50) {
            q.headwind_kt = Some(40.0);
            q.ias_kt = 120.0;
        }
        assert!(scherungen_aus(p).is_empty());
    }

    /// P-3.63/P-13.04: Schwelle `anflug.scherung_min_kt` (15 kt) — ein
    /// gleichsinniger Sprung von 14 kt zaehlt nicht, einer von 16 kt schon.
    /// Mutationstest in `grenzwerte.rs`.
    pub(crate) fn schwelle_pruefen() {
        let sprung = |kt: f32| {
            let mut p = ruhig();
            for q in p.iter_mut().skip(50) {
                q.headwind_kt = Some(20.0 - kt);
                q.ias_kt = 140.0 - kt;
            }
            scherungen_aus(p).len()
        };
        assert_eq!(sprung(14.0), 0);
        assert_eq!(sprung(16.0), 1);
    }

    #[test]
    fn schwelle_scherung_min() {
        schwelle_pruefen();
    }

    #[test]
    fn scherung_gegenprobe_gleichsinnig_zaehlt() {
        // Dieselbe Folge wie oben, nur gleichsinnig — sonst prueften die
        // beiden Tests davor nichts.
        let mut p = ruhig();
        for q in p.iter_mut().skip(50) {
            q.headwind_kt = Some(0.0);
            q.ias_kt = 120.0;
        }
        let s = scherungen_aus(p);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].gegenwind_kt, -20.0);
        assert_eq!(s[0].fahrt_kt, -20.0);
    }

    #[test]
    fn scherung_ueber_1500_ft_und_ohne_wind_zaehlt_nicht() {
        let hoch: Vec<_> = ruhig()
            .into_iter()
            .enumerate()
            .map(|(i, mut q)| {
                q.msl_ft += 1000.0;
                if i >= 50 {
                    q.headwind_kt = Some(0.0);
                    q.ias_kt = 120.0;
                }
                q
            })
            .collect();
        assert!(scherungen_aus(hoch).is_empty());

        let ohne_wind: Vec<_> = ruhig()
            .into_iter()
            .enumerate()
            .map(|(i, mut q)| {
                q.headwind_kt = None;
                if i >= 50 {
                    q.ias_kt = 120.0;
                }
                q
            })
            .collect();
        assert!(scherungen_aus(ohne_wind).is_empty());
    }

    #[test]
    fn scherung_eine_je_minute_die_staerkste() {
        // Zwei Spruenge 20 s auseinander: eine Scherung, die staerkere.
        let mut p = ruhig();
        for (i, q) in p.iter_mut().enumerate() {
            if (40..60).contains(&i) {
                q.headwind_kt = Some(0.0);
                q.ias_kt = 120.0;
            } else if i >= 60 {
                q.headwind_kt = Some(-25.0);
                q.ias_kt = 95.0;
            }
        }
        let s = scherungen_aus(p);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(s[0].gegenwind_kt, -25.0);
    }

    #[test]
    fn scherung_hoechstens_zwei_die_staerksten_in_zeitfolge() {
        // Drei Spruenge je 70 s auseinander (je eigene Scherung), Staerke
        // 20, 30, 25 kt: die beiden staerkeren bleiben, in Zeitfolge.
        let p: Vec<_> = (0..=240)
            .map(|i| {
                let (wind, fahrt) = match i {
                    0..=29 => (40.0, 160.0),
                    30..=99 => (20.0, 140.0),
                    100..=169 => (-10.0, 110.0),
                    _ => (-35.0, 85.0),
                };
                wind_probe(250.0 - i as f64, 1450.0 - 5.0 * i as f64, fahrt, Some(wind))
            })
            .collect();
        let s = scherungen_aus(p);
        let staerke: Vec<f32> = s.iter().map(|x| x.gegenwind_kt).collect();
        assert_eq!(staerke, vec![-30.0, -25.0], "{s:?}");
    }

    #[test]
    fn scherung_gleichstand_verdraengt_nicht_die_staerkste() {
        // QS 28.10.2026: Staerke 25, 25, 30 — frueher blieben 25, 25.
        let p: Vec<_> = (0..=240)
            .map(|i| {
                let (wind, fahrt) = match i {
                    0..=29 => (40.0, 160.0),
                    30..=99 => (15.0, 135.0),
                    100..=169 => (-10.0, 110.0),
                    _ => (-40.0, 80.0),
                };
                wind_probe(250.0 - i as f64, 1450.0 - 5.0 * i as f64, fahrt, Some(wind))
            })
            .collect();
        let s = scherungen_aus(p);
        let staerke: Vec<f32> = s.iter().map(|x| x.gegenwind_kt).collect();
        assert_eq!(staerke, vec![-25.0, -30.0], "{s:?}");
    }
}
