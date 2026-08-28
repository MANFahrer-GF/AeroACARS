//! Die Bahngeometrie aus der Simulator-Szenerie in die Navdaten ziehen.
//!
//! # Warum überhaupt
//!
//! Die Landebewertung misst den Abstand zur Mittellinie. Wo die liegt,
//! kam bisher nur aus den Navigationsdaten. Das ist der **echte**
//! Flughafen; der Pilot fliegt aber die **Szenerie**.
//!
//! Am 28.08.2026 gegen die installierte X-Plane-Szenerie gemessen, über
//! 70.452 Bahnen, die in beiden Quellen stehen:
//!
//! ```text
//! Median der Abweichung        0,03°
//! ab 3° daneben            3.653 Bahnen  (63 % davon Platzhalter-Kurse)
//! Breite ab 5 m daneben    7.279 Bahnen
//! schlimmster Fall           180°  — Bahn 17 mit Kurs 0,00° geführt
//! ```
//!
//! # Warum ERGÄNZEN und nicht ERSETZEN
//!
//! ⚠ Die Navdaten tragen mehr als Geometrie: ILS, Gleitwinkel,
//! Schwellenüberflughöhe. Die speisen die Anflugbewertung. Würde der
//! ganze Flughafen durch die Szenerie ersetzt, fielen sie weg — die
//! `apt.dat` kennt sie in dieser Form nicht.
//!
//! Deshalb bleibt der Flughafen aus den Navdaten die Grundlage, und nur
//! die **geometrischen** Felder werden überschrieben: Kurs, Breite,
//! Länge, Schwellenkoordinaten, versetzte Schwelle, Belag.
//!
//! # Warum eine Bahn trotz gleichem Bezeichner nicht dieselbe sein muss
//!
//! Bahnen werden umbenannt, wenn die Missweisung wandert. Eine „09" in
//! den Navdaten kann in der Szenerie eine andere Bahn desselben Platzes
//! sein — bei Parallelbahnen liegen sie hunderte Meter auseinander.
//! Deshalb wird nicht nur der Bezeichner verglichen, sondern auch
//! geprüft, ob Schwelle und Kurs überhaupt zusammenpassen können.

use aeroacars_mqtt::navdata::{NavAirport, NavRunway};
use sim_xplane::szenerie::{SzenerieBahn, SzenerieFlughafen};

/// Wie weit die Schwellen auseinanderliegen dürfen, damit es dieselbe
/// Bahn sein kann.
///
/// Zweihundert Meter sind grosszügig für Vermessungsunterschiede und zu
/// eng für eine Nachbarbahn: Parallelbahnen liegen nach ICAO mindestens
/// 210 m auseinander, in der Praxis meist deutlich mehr.
const SCHWELLE_HOECHSTABSTAND_M: f64 = 200.0;

// ⚠ Hier stand einmal ein Riegel auf die Kursabweichung (45°).
//
// Er war zirkulär: Er benutzte den Kurs, den wir gerade als kaputt
// erkannt haben, als Kriterium dafür, ob wir ihn reparieren dürfen.
//
// An BISL nachgemessen (28.08.2026): Bahn 15 steht bei uns mit Kurs
// 0,0°, Bahn 33 mit 360,0° — bei beiden fehlt er schlicht. Die Szenerie
// sagt 135,37° und 315,37°. Der Riegel liess die 33 korrigieren
// (44,6° Unterschied) und verwarf die 15 (135,4°) — **denselben Defekt,
// zwei verschiedene Antworten**, und ausgerechnet der schlimmere Fall
// blieb stehen.
//
// Was die Identität einer Bahn wirklich entscheidet, ist die LAGE.
// Liegt die Schwelle der Szenerie am selben Ort wie unsere, ist es
// dieselbe Bahn — dann darf der Kurs beliebig weit korrigiert werden.
// Liegt sie am anderen Ende (Umbenennung, Parallelbahn), ist es eine
// andere, und dann wird nichts übernommen.

/// Was bei der Übernahme geschah — für den Bericht und die Messung.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UebernahmeBericht {
    /// Bahnen, deren Geometrie aus der Szenerie kommt.
    pub uebernommen: Vec<String>,
    /// Bahnen, bei denen die Szenerie nichts Passendes hatte.
    pub ohne_treffer: Vec<String>,
    /// Bahnen, bei denen der Bezeichner passte, die Lage aber nicht —
    /// der verdächtigste Fall, deshalb getrennt geführt.
    pub verworfen: Vec<String>,
    /// Grösste Kursabweichung, die übernommen wurde, in Grad.
    pub groesste_kursabweichung_grad: f64,
    /// Grösste Breitenabweichung, die übernommen wurde, in Metern.
    pub groesste_breitenabweichung_m: f64,
}

fn winkelabstand(a: f64, b: f64) -> f64 {
    let d = ((a - b) % 360.0 + 540.0) % 360.0 - 180.0;
    d.abs()
}

fn abstand_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R: f64 = 6_371_000.0;
    let (p1, p2) = (a.0.to_radians(), b.0.to_radians());
    let dp = p2 - p1;
    let dl = (b.1 - a.1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Bezeichner vergleichbar machen: `"09L"`, `"9L"`, `"09l"` sind dasselbe.
fn normiert(b: &str) -> String {
    let t = b.trim().to_ascii_uppercase();
    let ziffern: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest: String = t.chars().skip_while(|c| c.is_ascii_digit()).collect();
    match ziffern.parse::<u32>() {
        Ok(n) => format!("{n:02}{rest}"),
        Err(_) => t,
    }
}

/// Die passende Bahn der Szenerie zu einer Navdaten-Bahn finden.
///
/// `None` heisst: nichts Passendes — dann bleibt die Navdaten-Geometrie
/// stehen. Im Zweifel die alte Quelle, nicht die neue.
fn passende_szenerie_bahn<'a>(
    nav: &NavRunway,
    sz: &'a SzenerieFlughafen,
) -> (Option<&'a SzenerieBahn>, bool) {
    let ziel = normiert(&nav.designator);
    let mut bezeichner_passte = false;
    for b in &sz.bahnen {
        if normiert(&b.bezeichner) != ziel {
            continue;
        }
        bezeichner_passte = true;
        let d = abstand_m(
            (nav.threshold.lat, nav.threshold.lon),
            (b.schwelle.0, b.schwelle.1),
        );
        if d > SCHWELLE_HOECHSTABSTAND_M {
            continue;
        }
        return (Some(b), true);
    }
    (None, bezeichner_passte)
}

/// Belagsschlüssel der `apt.dat` in die Schreibweise der Navdaten.
fn belag_text(code: u8) -> &'static str {
    match code {
        1 => "ASPH",
        2 => "CONC",
        3 => "TURF",
        4 => "DIRT",
        5 => "GRVL",
        12 => "MATS",
        13 => "SAND",
        14 => "WATER",
        15 => "SNOW",
        _ => "UNK",
    }
}

/// Die Geometrie aus der Szenerie übernehmen.
///
/// Gibt den ergänzten Flughafen und einen Bericht zurück. Ist die
/// Szenerie leer oder passt nichts, kommt der Flughafen unverändert
/// zurück — der Rückfall ist immer der bisherige Stand.
pub fn uebernimm_szenerie(
    nav: &NavAirport,
    sz: &SzenerieFlughafen,
) -> (NavAirport, UebernahmeBericht) {
    let mut aus = nav.clone();
    let mut b = UebernahmeBericht::default();

    for bahn in &mut aus.runways {
        let (treffer, bezeichner_passte) = passende_szenerie_bahn(bahn, sz);
        let Some(s) = treffer else {
            if bezeichner_passte {
                b.verworfen.push(bahn.designator.clone());
            } else {
                b.ohne_treffer.push(bahn.designator.clone());
            }
            continue;
        };

        let kurs_ab = winkelabstand(bahn.true_course, s.kurs_grad);
        if kurs_ab > b.groesste_kursabweichung_grad {
            b.groesste_kursabweichung_grad = kurs_ab;
        }
        if let Some(w) = bahn.width_ft {
            let breit_ab = (w as f64 * 0.3048 - s.breite_m).abs();
            if breit_ab > b.groesste_breitenabweichung_m {
                b.groesste_breitenabweichung_m = breit_ab;
            }
        }

        // ⚠ Nur Geometrie. ILS, Gleitwinkel und Schwellenhöhe bleiben,
        // wo sie sind — die kennt die `apt.dat` nicht.
        bahn.true_course = s.kurs_grad;
        bahn.length_ft = (s.laenge_m / 0.3048).round() as i32;
        bahn.width_ft = Some((s.breite_m / 0.3048).round() as i32);
        bahn.displaced_threshold_ft = (s.versetzte_schwelle_m / 0.3048).round() as i32;
        bahn.threshold.lat = s.schwelle.0;
        bahn.threshold.lon = s.schwelle.1;
        bahn.far_end.lat = s.gegenende.0;
        bahn.far_end.lon = s.gegenende.1;
        bahn.surface = Some(belag_text(s.belag_code).to_string());
        b.uebernommen.push(bahn.designator.clone());
    }

    (aus, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroacars_mqtt::navdata::NavPoint;

    fn punkt(lat: f64, lon: f64) -> NavPoint {
        NavPoint {
            lat,
            lon,
            elev_ft: None,
        }
    }

    /// EDHE 09, wie es am 28.08.2026 in unseren Navdaten stand.
    pub(super) fn edhe_nav() -> NavAirport {
        NavAirport {
            cycle: "2608".into(),
            valid_to: "2026-09-24".into(),
            icao: "EDHE".into(),
            name: "Uetersen".into(),
            latitude: 53.6459,
            longitude: 9.7042,
            elevation_ft: Some(21),
            runways: vec![NavRunway {
                designator: "09".into(),
                magnetic_course: 87.0,
                true_course: 89.9957383300858,
                length_ft: 3609,
                width_ft: Some(131),
                surface: Some("ASPH".into()),
                threshold: punkt(53.6459, 9.6942),
                far_end: punkt(53.6459, 9.7142),
                displaced_threshold_ft: 0,
                ils: None,
                glideslope_angle: 3.0,
                tch_ft: 50,
            }],
        }
    }

    fn szenerie(
        bezeichner: &str,
        kurs: f64,
        breite: f64,
        schwelle: (f64, f64),
    ) -> SzenerieFlughafen {
        SzenerieFlughafen {
            icao: "EDHE".into(),
            quelle: "Test".into(),
            rollwege: vec![],
            bahnen: vec![SzenerieBahn {
                bezeichner: bezeichner.into(),
                kurs_grad: kurs,
                breite_m: breite,
                laenge_m: 1100.0,
                versetzte_schwelle_m: 0.0,
                schwelle,
                gegenende: (schwelle.0, schwelle.1 + 0.02),
                belag_code: 1,
            }],
        }
    }

    #[test]
    fn korrigiert_kurs_und_breite() {
        // Der echte Fall: unsere Navdaten fuehren 89,996 Grad (ein aus
        // 87,0 magnetisch abgeleiteter Platzhalter), die Szenerie 93,72.
        let nav = edhe_nav();
        let sz = szenerie("09", 93.72, 55.0, (53.6459, 9.6942));
        let (aus, b) = uebernimm_szenerie(&nav, &sz);
        assert_eq!(b.uebernommen, vec!["09"]);
        assert!((aus.runways[0].true_course - 93.72).abs() < 0.001);
        assert_eq!(aus.runways[0].width_ft, Some(180)); // 55 m
        assert!(
            (b.groesste_kursabweichung_grad - 3.724).abs() < 0.01,
            "gemeldete Abweichung {}",
            b.groesste_kursabweichung_grad
        );
    }

    #[test]
    fn ils_und_gleitwinkel_bleiben_erhalten() {
        // ⚠ Die `apt.dat` kennt sie nicht. Wuerde der Flughafen ersetzt
        // statt ergaenzt, fiele die Anflugbewertung aus.
        let mut nav = edhe_nav();
        nav.runways[0].glideslope_angle = 3.2;
        let sz = szenerie("09", 93.72, 55.0, (53.6459, 9.6942));
        let (aus, _) = uebernimm_szenerie(&nav, &sz);
        assert!((aus.runways[0].glideslope_angle - 3.2).abs() < 1e-9);
        assert_eq!(aus.runways[0].magnetic_course, 87.0);
    }

    #[test]
    fn gleicher_bezeichner_aber_woanders_wird_verworfen() {
        // Parallelbahnen und umbenannte Bahnen: Der Bezeichner allein
        // reicht nicht. Fuenf Kilometer entfernt ist es eine andere Bahn.
        let nav = edhe_nav();
        let sz = szenerie("09", 93.72, 55.0, (53.7000, 9.6942));
        let (aus, b) = uebernimm_szenerie(&nav, &sz);
        assert_eq!(b.verworfen, vec!["09"]);
        assert!(b.uebernommen.is_empty());
        assert!(
            (aus.runways[0].true_course - 89.9957).abs() < 0.001,
            "unveraendert"
        );
    }

    #[test]
    fn ein_voellig_falscher_kurs_wird_an_derselben_stelle_korrigiert() {
        // Der Fall BISL: Bahn 15 mit Kurs 0,0 gefuehrt, in Wahrheit
        // 135,37. Die Schwelle steht an derselben Stelle — es IST
        // dieselbe Bahn, nur ohne Kurs. Genau die gehoert korrigiert.
        //
        // Ein Riegel auf die Kursabweichung haette hier abgelehnt und
        // damit ausgerechnet den schlimmsten Fall stehen lassen.
        let nav = edhe_nav();
        let sz = szenerie("09", 224.99, 55.0, (53.6459, 9.6942));
        let (aus, b) = uebernimm_szenerie(&nav, &sz);
        assert_eq!(b.uebernommen, vec!["09"]);
        assert!((aus.runways[0].true_course - 224.99).abs() < 0.001);
    }

    #[test]
    fn andere_bahn_am_anderen_ende_wird_verworfen() {
        // Umbenennung oder Parallelbahn: gleicher Bezeichner, andere
        // Lage. Hier darf NICHTS uebernommen werden — sonst wuerde die
        // Anflugrichtung stillschweigend umdefiniert.
        //
        // 600 m entfernt: die Laenge einer kleinen Bahn, also genau der
        // Abstand zwischen den beiden Enden derselben Piste.
        let nav = edhe_nav();
        let sz = szenerie("09", 269.99, 55.0, (53.6459, 9.7033));
        let (aus, b) = uebernimm_szenerie(&nav, &sz);
        assert_eq!(b.verworfen, vec!["09"]);
        assert!((aus.runways[0].true_course - 89.9957).abs() < 0.001);
    }

    #[test]
    fn bezeichner_werden_normiert_verglichen() {
        let mut nav = edhe_nav();
        nav.runways[0].designator = "9".into();
        let sz = szenerie("09", 93.72, 55.0, (53.6459, 9.6942));
        let (_, b) = uebernimm_szenerie(&nav, &sz);
        assert_eq!(b.uebernommen, vec!["9"]);
    }

    #[test]
    fn ohne_treffer_bleibt_alles_wie_es_war() {
        let nav = edhe_nav();
        let sz = szenerie("27", 273.72, 55.0, (53.6459, 9.7142));
        let (aus, b) = uebernimm_szenerie(&nav, &sz);
        assert_eq!(b.ohne_treffer, vec!["09"]);
        assert!((aus.runways[0].true_course - 89.9957).abs() < 0.001);
        assert_eq!(aus.runways[0].width_ft, Some(131));
    }

    #[test]
    fn leere_szenerie_aendert_nichts() {
        let nav = edhe_nav();
        let leer = SzenerieFlughafen {
            icao: "EDHE".into(),
            ..Default::default()
        };
        let (aus, b) = uebernimm_szenerie(&nav, &leer);
        assert!(b.uebernommen.is_empty());
        assert_eq!(aus.runways[0].true_course, nav.runways[0].true_course);
    }
}

/// Nur fuer den Korpus-Lauf: eine  aus dem Textauszug bauen.
///
/// Steht hier und nicht im Test, weil  sonst von aussen nicht
/// vollstaendig konstruierbar waere — und ein zweiter Bauweg waere ein
/// zweiter Ort, an dem Felder vergessen werden koennen.
pub fn test_navairport(icao: &str, zeilen: &[Vec<String>]) -> NavAirport {
    let z = |s: &String| s.parse::<f64>().unwrap_or(0.0);
    NavAirport {
        cycle: String::new(),
        valid_to: String::new(),
        icao: icao.to_string(),
        name: String::new(),
        latitude: 0.0,
        longitude: 0.0,
        elevation_ft: None,
        runways: zeilen
            .iter()
            .map(|t| NavRunway {
                designator: t[1].clone(),
                magnetic_course: z(&t[3]),
                true_course: z(&t[2]),
                length_ft: z(&t[5]) as i32,
                width_ft: if t[4].is_empty() {
                    None
                } else {
                    Some(z(&t[4]) as i32)
                },
                surface: None,
                threshold: aeroacars_mqtt::navdata::NavPoint {
                    lat: z(&t[6]),
                    lon: z(&t[7]),
                    elev_ft: None,
                },
                far_end: aeroacars_mqtt::navdata::NavPoint {
                    lat: z(&t[8]),
                    lon: z(&t[9]),
                    elev_ft: None,
                },
                displaced_threshold_ft: 0,
                ils: None,
                glideslope_angle: 3.0,
                tch_ft: 50,
            })
            .collect(),
    }
}

// ─────────────────────────────────────────────────────────────────────
// Anschluss an den Flug
// ─────────────────────────────────────────────────────────────────────

use std::sync::{Mutex, OnceLock};

/// Das Verzeichnis der installierten Szenerie, einmal gebaut.
///
/// Der Aufbau kostet rund eine halbe Sekunde ueber 380 MB. Pro Landung
/// waere das absurd, pro Programmlauf ist es nichts — und wenn der Pilot
/// zwischendurch ein Add-on installiert, faellt das ueber Groesse und
/// Aenderungszeit der Quelldateien auf, und es wird neu gebaut.
static VERZEICHNIS: OnceLock<Mutex<Option<sim_xplane::szenerie::SzenerieIndex>>> = OnceLock::new();

/// Den Flughafen aus der Szenerie holen, mit Verzeichnis.
fn szenerie_flughafen(icao: &str) -> Option<sim_xplane::szenerie::SzenerieFlughafen> {
    let zelle = VERZEICHNIS.get_or_init(|| Mutex::new(None));
    let mut halter = zelle.lock().ok()?;
    let neu_bauen = match halter.as_ref() {
        Some(idx) => !idx.gueltig(),
        None => true,
    };
    if neu_bauen {
        let wurzel = sim_xplane::szenerie::installationen().into_iter().next()?;
        let t = std::time::Instant::now();
        let idx = sim_xplane::szenerie::SzenerieIndex::bauen(&wurzel);
        tracing::info!(
            flughaefen = idx.anzahl(),
            dauer_ms = t.elapsed().as_millis(),
            "Szenerie-Verzeichnis gebaut"
        );
        *halter = Some(idx);
    }
    halter.as_ref()?.flughafen(icao)
}

/// Gilt die Szenerie fuer diesen Simulator?
///
/// ⚠ Nur bei X-Plane. Die `apt.dat` beschreibt die X-Plane-Welt; wer
/// MSFS fliegt, hat eine andere Szenerie, und die hier zu benutzen waere
/// schlimmer als gar keine Korrektur. Fuer MSFS kommt der eigene Weg
/// ueber die SimConnect-Facility-Schnittstelle.
pub fn gilt_fuer(simulator: sim_core::Simulator) -> bool {
    matches!(
        simulator,
        sim_core::Simulator::XPlane11 | sim_core::Simulator::XPlane12
    )
}

/// Der Anschluss: Navdaten mit der Szenerie ergaenzen, wenn beides passt.
///
/// Gibt den (moeglicherweise ergaenzten) Flughafen und den Bericht
/// zurueck. Passiert nichts, ist der Bericht leer und der Flughafen
/// unveraendert — der Rueckfall ist immer der bisherige Stand.
pub fn ergaenze_aus_szenerie(
    simulator: sim_core::Simulator,
    icao: &str,
    nav: Option<NavAirport>,
) -> (Option<NavAirport>, Option<UebernahmeBericht>) {
    if !gilt_fuer(simulator) {
        return (nav, None);
    }
    let Some(nav) = nav else {
        // Ohne Navdaten gibt es nichts zu ergaenzen. Einen Flughafen
        // ALLEIN aus der Szenerie zu bauen waere moeglich, aber dann
        // fehlten ILS, Gleitwinkel und Schwellenhoehe — und die Anzeige
        // haette stillschweigend weniger als vorher.
        return (None, None);
    };
    let Some(sz) = szenerie_flughafen(icao) else {
        return (Some(nav), None);
    };
    let (ergaenzt, bericht) = uebernimm_szenerie(&nav, &sz);
    if bericht.uebernommen.is_empty() {
        return (Some(nav), Some(bericht));
    }
    tracing::info!(
        icao,
        uebernommen = bericht.uebernommen.len(),
        verworfen = bericht.verworfen.len(),
        kurs_grad = bericht.groesste_kursabweichung_grad,
        breite_m = bericht.groesste_breitenabweichung_m,
        quelle = %sz.quelle,
        "Bahngeometrie aus der Szenerie uebernommen"
    );
    (Some(ergaenzt), Some(bericht))
}

#[cfg(test)]
mod anschluss_tests {
    use super::*;
    use sim_core::Simulator;

    fn nav_edhe() -> NavAirport {
        super::tests::edhe_nav()
    }

    #[test]
    fn bei_msfs_passiert_nichts() {
        // ⚠ Die `apt.dat` beschreibt die X-Plane-Welt. Wer MSFS fliegt,
        // hat eine andere Szenerie — die hier zu benutzen waere
        // schlimmer als gar keine Korrektur, weil sie plausibel
        // aussieht und falsch ist.
        for sim in [Simulator::Msfs2020, Simulator::Msfs2024, Simulator::Other] {
            let vorher = nav_edhe();
            let (nachher, bericht) = ergaenze_aus_szenerie(sim, "EDHE", Some(vorher.clone()));
            assert!(
                bericht.is_none(),
                "{sim:?}: Bericht trotz falschem Simulator"
            );
            assert_eq!(
                nachher.unwrap().runways[0].true_course,
                vorher.runways[0].true_course,
                "{sim:?}: Kurs veraendert"
            );
        }
    }

    #[test]
    fn ohne_navdaten_wird_nichts_erfunden() {
        // Einen Flughafen ALLEIN aus der Szenerie zu bauen waere
        // moeglich — dann fehlten aber ILS, Gleitwinkel und
        // Schwellenhoehe, und die Anzeige haette stillschweigend
        // weniger als vorher.
        let (nachher, bericht) = ergaenze_aus_szenerie(Simulator::XPlane12, "EDHE", None);
        assert!(nachher.is_none());
        assert!(bericht.is_none());
    }

    #[test]
    fn gilt_fuer_trennt_die_simulatoren() {
        assert!(gilt_fuer(Simulator::XPlane11));
        assert!(gilt_fuer(Simulator::XPlane12));
        assert!(!gilt_fuer(Simulator::Msfs2020));
        assert!(!gilt_fuer(Simulator::Msfs2024));
        assert!(!gilt_fuer(Simulator::Other));
    }

    #[test]
    fn mit_xplane_wird_der_kurs_wirklich_korrigiert() {
        // Gegen die hier installierte Szenerie. Ohne Installation
        // ueberspringt sich der Test — sichtbar, nicht still.
        if sim_xplane::szenerie::installationen().is_empty() {
            eprintln!("uebersprungen: keine X-Plane-Installation");
            return;
        }
        let vorher = nav_edhe();
        let (nachher, bericht) =
            ergaenze_aus_szenerie(Simulator::XPlane12, "EDHE", Some(vorher.clone()));
        let Some(b) = bericht else {
            panic!("kein Bericht — Szenerie nicht gefunden?");
        };
        assert!(
            b.uebernommen.contains(&"09".to_string()),
            "EDHE 09 nicht uebernommen: {b:?}"
        );
        let n = nachher.unwrap();
        // Unsere Navdaten fuehren 89,996 Grad (aus 87,0 magnetisch
        // abgeleitet), die installierte Szenerie 93,72.
        assert!(
            (n.runways[0].true_course - 93.72).abs() < 0.2,
            "Kurs nach der Uebernahme: {}",
            n.runways[0].true_course
        );
        assert!(
            b.groesste_kursabweichung_grad > 3.0,
            "gemeldete Abweichung zu klein: {}",
            b.groesste_kursabweichung_grad
        );
    }
}
