//! Ausfahrten einer Bahn — wo Rollwege die Bahnkante treffen.
//!
//! Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.6.
//!
//! # Wozu
//!
//! Die Stummel am Bahnrand machen die Bewertung erst nachvollziehbar: Man
//! sieht, welche Ausfahrt vor der genutzten lag und wie weit davor. Beim
//! Auslöser dieses Umbaus — MPH 9 — war genau das die Frage: Der Pilot hat
//! die erste erreichbare Ausfahrt genommen und dafür 45 Punkte verloren.
//! Ohne die Stummel steht diese Aussage nur im Text.
//!
//! # Woher die Daten kommen
//!
//! Aus der OpenStreetMap-Bodenkarte, die der Client für die Standerkennung
//! ohnehin ab dem Sinkflug lädt. Es braucht **keinen** zusätzlichen Abruf:
//! Das rohe GeoJSON enthält die Rollwege bereits, sie wurden nur nie
//! ausgewertet.
//!
//! Das ist auch der Grund, warum die Berechnung hier steht und nicht auf dem
//! Server: Die Bahn steht erst beim Aufsetzen fest. Ein Serverabruf zu
//! diesem Zeitpunkt käme zu spät für die Anzeige — die Karte dagegen liegt
//! dann schon Minuten im Speicher.
//!
//! # Was NICHT gezeichnet wird
//!
//! Vollständige Rollwege. Bei der Überhöhung der Querachse wäre ein
//! 30°-Schnellabrollweg fast senkrecht dargestellt — eine Behauptung, die
//! der Massstab nicht hergibt. Der Stummel markiert die Position, mehr
//! nicht.

use serde::{Deserialize, Serialize};

/// Ein Punkt des Rollwegs, in Bahn-Koordinaten.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct Verlaufspunkt {
    pub laengs_m: f64,
    pub quer_m: f64,
}

/// Eine Ausfahrt: wo ein benannter Rollweg die Bahnkante trifft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ausfahrt {
    /// Kennung des Rollwegs, z. B. `S4`.
    pub name: String,
    /// Distanz ab der Landeschwelle, in Metern.
    pub laengs_m: f64,
    /// Auf welcher Seite: `"left"` oder `"right"` in Landerichtung.
    pub seite: String,
    /// Wie der Rollweg wirklich verläuft — in Bahn-Koordinaten.
    ///
    /// # Warum das mitkommt
    ///
    /// Bis hierher trug eine Ausfahrt nur ihre Position. Die Queransicht
    /// setzte deshalb einen Stummel an den Bahnrand, und das war richtig
    /// so: Bei sechzehnfacher Überhöhung wäre ein 25°-Schnellabrollweg
    /// fast senkrecht gezeichnet worden.
    ///
    /// Für eine massstabstreue Ansicht dreht sich das um. Thomas zu
    /// DLH369 (EDDM 26L, 25.08.2026): „auf B6 abgerollt, aber das
    /// Abrollen sieht auf der Darstellung ganz anders aus, B6 hat einen
    /// anderen Verlauf." Nachgemessen: Die Ausfahrt lief mit 19,4°, B6
    /// selbst hat 23,7°, gezeichnet waren 80,3°.
    ///
    /// Ohne den Verlauf kann keine Ansicht das richtigstellen — die
    /// Geometrie liegt hier, beim Rechnen, und wurde bisher verworfen.
    ///
    /// Leer, wenn die Bodenkarte für diesen Rollweg nichts hergibt; der
    /// Verbraucher faellt dann auf den Stummel zurueck.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub verlauf: Vec<Verlaufspunkt>,
}

/// Mindestabstand zweier Verlaufspunkte, in Metern.
///
/// Rollwege in OpenStreetMap haben teils Stützpunkte im Meterabstand. Für
/// eine Zeichnung reicht deutlich weniger, und jede Landung traegt diese
/// Punkte durch die Leitung — bei vierzehn Ausfahrten summiert sich das.
const VERLAUF_MIN_ABSTAND_M: f64 = 8.0;

/// Wie weit der Verlauf vor und hinter der Kante mitgenommen wird.
///
/// Vierhundert Meter hinter der Kante: So weit reicht ein
/// Schnellabrollweg, bis er in den parallelen Rollweg einmündet — bei
/// EDDM B6 sind es rund dreihundert. Davor genügen fünfzig; weiter
/// vorn liegt der Rollweg auf der Bahn und ist dort nicht zu zeichnen.
const VERLAUF_VOR_M: f64 = 50.0;
const VERLAUF_NACH_M: f64 = 400.0;

/// Wie weit der Verlauf ueber die Bahnkante hinaus mitgenommen wird.
///
/// # Warum das noetig wurde
///
/// Bis zur QS am 26.08.2026 war der Verlauf nur LAENGS beschnitten. Quer
/// lief er, so weit die Bodenkarte reichte: In Muenchen begann B6 bei
/// 219,5 Metern neben der Mittellinie, tief im Vorfeld. Die Queransicht
/// zeigt rund dreissig Meter — der Korridor waere zu sieben Achteln
/// ausserhalb des Bildes gewesen.
///
/// Wie viel darueber hinaus sinnvoll ist, sagt die ZEICHNUNG, nicht das
/// Gefuehl: Die Queransicht zeigt neben der Kante nur den gruenen
/// Streifen, `GRUEN_H = 13` Pixel — bei Muenchener Massstab rund drei
/// Meter. Alles Weitere klemmt `querZuY` auf den Rand und laege dort als
/// waagerechter Strich, also als Behauptung, der Rollweg fuehre die Kante
/// entlang.
///
/// Sechs Meter: knapp das Doppelte des Sichtbaren, damit der Korridor den
/// Rand erkennbar erreicht, ohne einen langen unsichtbaren Schwanz
/// mitzuschleppen.
const VERLAUF_QUER_UEBER_KANTE_M: f64 = 6.0;

/// Wie nah ein Stützpunkt an der Bahnkante liegen muss, um als Ausfahrt zu
/// zählen, in Metern.
///
/// Fünfundzwanzig Meter: Rollwege sind in OpenStreetMap als Mittellinie
/// erfasst, und ihr erster Stützpunkt liegt selten exakt auf der Kante —
/// mal ein Stück davor, mal schon auf der Bahn. Enger gefasst fallen echte
/// Ausfahrten heraus; weiter gefasst zählen parallel verlaufende Rollwege
/// mit, die die Bahn nie berühren.
const KANTEN_TOLERANZ_M: f64 = 25.0;

/// Wie weit hinter dem Bahnende noch gesucht wird.
const HINTER_DEM_ENDE_M: f64 = 200.0;

/// Ausfahrten aus einer Bodenkarte für eine bestimmte Bahn.
///
/// `geojson` ist die rohe Karte, wie sie vom Server kommt. Die Bahn wird
/// über Schwelle und Bahnende beschrieben — dieselben Werte, die auch die
/// Bahnprojektion benutzt.
pub fn ausfahrten_fuer_bahn(
    geojson: &str,
    threshold_lat: f64,
    threshold_lon: f64,
    end_lat: f64,
    end_lon: f64,
    breite_m: f64,
) -> Vec<Ausfahrt> {
    let Ok(wurzel) = serde_json::from_str::<serde_json::Value>(geojson) else {
        return Vec::new();
    };
    let Some(merkmale) = wurzel.get("features").and_then(|f| f.as_array()) else {
        return Vec::new();
    };
    let halbe_breite = (breite_m / 2.0).max(10.0);
    let bahnlaenge = ::geo::distance_m(threshold_lat, threshold_lon, end_lat, end_lon);

    // Je Name und Seite den Punkt mit dem kleinsten Kantenabstand.
    //
    // Ein Rollweg berührt die Bahn oft mit mehreren Stützpunkten; ohne diese
    // Auswahl bekäme derselbe Rollweg mehrere Stummel nebeneinander.
    let mut beste: Vec<(String, String, f64, f64, Vec<(f64, f64)>)> = Vec::new();
    for m in merkmale {
        let props = m.get("properties");
        if props.and_then(|p| p.get("k")).and_then(|k| k.as_str()) != Some("taxiway") {
            continue;
        }
        let name = props
            .and_then(|p| p.get("r"))
            .and_then(|r| r.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        // Unbenannte Rollwege werden übersprungen: Ein Stummel ohne Kennung
        // sagt nichts, das die Spur nicht schon zeigt.
        let Some(name) = name else { continue };

        let geom = m.get("geometry");
        if geom.and_then(|g| g.get("type")).and_then(|t| t.as_str()) != Some("LineString") {
            continue;
        }
        let Some(punkte) = geom
            .and_then(|g| g.get("coordinates"))
            .and_then(|c| c.as_array())
        else {
            continue;
        };

        // Den ganzen Weg EINMAL projizieren — er wird zweimal gebraucht:
        // fuer den Kantentreffer und fuer den Verlauf.
        let projiziert: Vec<(f64, f64)> = punkte
            .iter()
            .filter_map(lonlat)
            .map(|(lon, lat)| {
                crate::runway::projiziere_auf_bahn(
                    threshold_lat,
                    threshold_lon,
                    end_lat,
                    end_lon,
                    lat,
                    lon,
                )
            })
            .collect();

        for &(laengs, quer) in &projiziert {
            if laengs < 20.0 || laengs > bahnlaenge + HINTER_DEM_ENDE_M {
                continue;
            }
            let kantenabstand = (quer.abs() - halbe_breite).abs();
            if kantenabstand > KANTEN_TOLERANZ_M {
                continue;
            }
            let seite = if quer > 0.0 { "right" } else { "left" };
            match beste
                .iter_mut()
                .find(|(n, s, _, _, _)| n == name && s == seite)
            {
                Some(eintrag) if kantenabstand < eintrag.3 => {
                    eintrag.2 = laengs;
                    eintrag.3 = kantenabstand;
                    eintrag.4 = projiziert.clone();
                }
                Some(_) => {}
                None => beste.push((
                    name.to_string(),
                    seite.to_string(),
                    laengs,
                    kantenabstand,
                    projiziert.clone(),
                )),
            }
        }
    }

    beste.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    beste
        .into_iter()
        .map(|(name, seite, laengs_m, _, roh)| Ausfahrt {
            name,
            laengs_m: (laengs_m * 10.0).round() / 10.0,
            seite,
            verlauf: verlauf_ausduennen(&roh, laengs_m, halbe_breite),
        })
        .collect()
}

/// Den Verlauf auf das beschneiden, was gezeichnet wird — und ausduennen.
///
/// Beschnitten wird um die KANTE herum, nicht um die ganze Bahn: Was
/// zweihundert Meter vor der Ausfahrt neben der Bahn liegt, gehoert zu
/// einem anderen Teil des Rollwegs und wuerde die Zeichnung nur
/// zukleistern.
fn verlauf_ausduennen(
    roh: &[(f64, f64)],
    kante_laengs: f64,
    halbe_breite_m: f64,
) -> Vec<Verlaufspunkt> {
    let quer_max = halbe_breite_m + VERLAUF_QUER_UEBER_KANTE_M;
    let mut aus: Vec<Verlaufspunkt> = Vec::new();
    for &(laengs, quer) in roh {
        if laengs < kante_laengs - VERLAUF_VOR_M || laengs > kante_laengs + VERLAUF_NACH_M {
            continue;
        }
        // Und quer: was weit neben der Bahn liegt, wird nie gezeichnet.
        if quer.abs() > quer_max {
            continue;
        }
        if let Some(letzter) = aus.last() {
            let d = (laengs - letzter.laengs_m).hypot(quer - letzter.quer_m);
            if d < VERLAUF_MIN_ABSTAND_M {
                continue;
            }
        }
        aus.push(Verlaufspunkt {
            laengs_m: (laengs * 10.0).round() / 10.0,
            quer_m: (quer * 10.0).round() / 10.0,
        });
    }
    // Ein einzelner Punkt ist keine Linie.
    if aus.len() < 2 {
        aus.clear();
        return aus;
    }

    // Von der Bahn WEG, nicht auf sie zu.
    //
    // Die Reihenfolge kommt aus der Bodenkarte und meint nichts: OSM
    // zeichnet einen Rollweg mal vom Vorfeld zur Bahn, mal andersherum.
    // In Muenchen lief B6 von aussen nach innen, B7 andersherum — auf
    // demselben Flughafen, an derselben Bahn.
    //
    // Fuer die Anzeige ist die Richtung nicht gleichgueltig: Der
    // Korridor soll dort beginnen, wo das Flugzeug die Bahn verlaesst.
    // Die Punktfolge wird deshalb umgedreht, wenn ihr Ende naeher an der
    // Mittellinie liegt als ihr Anfang. Die Geometrie bleibt dabei
    // unangetastet — es wird nicht sortiert, nur gedreht.
    let (Some(erster), Some(letzter)) = (aus.first(), aus.last()) else {
        return aus;
    };
    if letzter.quer_m.abs() < erster.quer_m.abs() {
        aus.reverse();
    }
    aus
}

fn lonlat(v: &serde_json::Value) -> Option<(f64, f64)> {
    let a = v.as_array()?;
    Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EDDH 23, aus den Navdaten.
    const T: (f64, f64, f64, f64) = (53.636011, 9.999656, 53.619958, 9.967167);

    fn karte(merkmale: &str) -> String {
        format!(r#"{{"type":"FeatureCollection","features":[{merkmale}]}}"#)
    }

    /// Ein Rollweg, der die Bahn bei etwa 900 m links trifft.
    fn rollweg(name: &str, lat: f64, lon: f64) -> String {
        format!(
            r#"{{"type":"Feature","properties":{{"k":"taxiway","r":"{name}"}},
                 "geometry":{{"type":"LineString","coordinates":[[{lon},{lat}],[{lon2},{lat2}]]}}}}"#,
            lon2 = lon + 0.002,
            lat2 = lat + 0.002,
        )
    }

    /// Ein Rollweg aus beliebig vielen Punkten in Bahn-Koordinaten.
    fn rollweg_aus(name: &str, punkte: &[(f64, f64)]) -> String {
        let ko: Vec<String> = punkte
            .iter()
            .map(|&(lg, qr)| {
                let (lat, lon) = punkt_auf_bahn(lg, qr);
                format!("[{lon},{lat}]")
            })
            .collect();
        format!(
            r#"{{"type":"Feature","properties":{{"k":"taxiway","r":"{name}"}},
                 "geometry":{{"type":"LineString","coordinates":[{}]}}}}"#,
            ko.join(",")
        )
    }

    // ── Der Verlauf ──────────────────────────────────────────────────
    //
    // Befund Thomas zu DLH369 (EDDM 26L, 25.08.2026): „auf B6 abgerollt,
    // aber das Abrollen sieht auf der Darstellung ganz anders aus, B6 hat
    // einen anderen Verlauf." Gemessen lief die Ausfahrt mit 19,4°, B6
    // selbst hat 23,7° — gezeichnet waren 80,3°, weil die Queransicht
    // sechzehnfach ueberhoeht ist.
    //
    // Ohne den Verlauf kann keine Ansicht das richtigstellen. Er liegt
    // hier, beim Rechnen, und wurde bisher verworfen.

    #[test]
    fn der_verlauf_kommt_mit() {
        // Ein Schnellabrollweg, wie B6 ihn hat: an der Kante beginnend,
        // dann flach nach aussen.
        // Die Punkte liegen im gezeichneten Bereich: Bahnhalbbreite 23 m
        // plus sechs. Was weiter aussen liegt, wird bewusst verworfen —
        // die Queransicht zeigt es nicht (siehe
        // `VERLAUF_QUER_UEBER_KANTE_M`).
        let g = karte(&rollweg_aus(
            "B6",
            &[
                (860.0, -4.0),
                (880.0, -10.0),
                (900.0, -16.0),
                (920.0, -22.0),
                (945.0, -28.0),
                (980.0, -40.0),
            ],
        ));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!(
            a[0].verlauf.len() >= 4,
            "nur {} Verlaufspunkte — die Geometrie wird verworfen",
            a[0].verlauf.len()
        );
        // Der Verlauf muss NACH AUSSEN laufen, nicht zurueck.
        let erst = a[0].verlauf.first().unwrap();
        let letzt = a[0].verlauf.last().unwrap();
        assert!(letzt.laengs_m > erst.laengs_m);
        assert!(letzt.quer_m.abs() > erst.quer_m.abs());
    }

    #[test]
    fn der_verlauf_traegt_den_echten_winkel() {
        // Das ist der Punkt der ganzen Uebung: Ein 25-Grad-Rollweg muss
        // als 25 Grad ankommen, nicht als 80.
        // Ein 25-Grad-Rollweg, aufgeloest im gezeichneten Bereich.
        let g = karte(&rollweg_aus(
            "B6",
            &[
                (880.0, -4.0),
                (900.0, -13.3),
                (920.0, -22.6),
                (933.0, -28.7),
            ],
        ));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        let v = &a[0].verlauf;
        let dl = v.last().unwrap().laengs_m - v.first().unwrap().laengs_m;
        let dq = v.last().unwrap().quer_m - v.first().unwrap().quer_m;
        let grad = dq.abs().atan2(dl).to_degrees();
        assert!(
            (grad - 25.0).abs() < 3.0,
            "{grad:.1}° statt 25° — der Verlauf ist verzerrt"
        );
    }

    #[test]
    fn dichte_stuetzpunkte_werden_ausgeduennt() {
        // OpenStreetMap hat teils Punkte im Meterabstand. Jede Landung
        // traegt sie durch die Leitung; bei vierzehn Ausfahrten summiert
        // sich das.
        // Der Rollweg laeuft flach nach aussen und bleibt dabei im
        // gezeichneten Bereich — sonst prueft der Test die
        // Querbeschneidung statt der Ausduennung.
        let dicht: Vec<(f64, f64)> = (0..120)
            .map(|i| (860.0 + i as f64 * 2.0, -(i as f64) * 0.24))
            .collect();
        let g = karte(&rollweg_aus("B6", &dicht));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        let v = &a[0].verlauf;
        assert!(v.len() < 40, "{} Punkte — zu dicht", v.len());
        assert!(v.len() > 5, "{} Punkte — zu duenn", v.len());
        for w in v.windows(2) {
            let d = (w[1].laengs_m - w[0].laengs_m).hypot(w[1].quer_m - w[0].quer_m);
            assert!(d >= VERLAUF_MIN_ABSTAND_M - 0.5, "{d:.1} m Abstand");
        }
    }

    #[test]
    fn weit_entferntes_wird_abgeschnitten() {
        // Was zweihundert Meter VOR der Ausfahrt neben der Bahn liegt,
        // gehoert zu einem anderen Teil des Rollwegs und wuerde die
        // Zeichnung zukleistern.
        let g = karte(&rollweg_aus(
            "B6",
            &[
                (100.0, -80.0), // weit davor — muss weg
                (900.0, -23.0), // die Kante
                (1000.0, -60.0),
                (2500.0, -300.0), // weit dahinter — muss weg
            ],
        ));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        for v in &a[0].verlauf {
            assert!(
                v.laengs_m > 800.0 && v.laengs_m < 1400.0,
                "{} m liegt ausserhalb des Ausschnitts",
                v.laengs_m
            );
        }
    }

    #[test]
    fn ein_einzelner_punkt_ist_keine_linie() {
        // Bleibt nach dem Beschneiden nur ein Punkt uebrig, gibt es
        // nichts zu zeichnen — dann lieber gar keinen Verlauf als einen,
        // den die Anzeige zu einer Linie verlaengert.
        let (lat, lon) = punkt_auf_bahn(900.0, -23.0);
        let g = karte(&rollweg("D8", lat, lon));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1);
        // Der Zwei-Punkte-Helfer legt den zweiten Punkt weit weg —
        // entweder er faellt aus dem Ausschnitt, oder es sind zwei.
        assert!(a[0].verlauf.is_empty() || a[0].verlauf.len() >= 2);
    }

    #[test]
    fn findet_einen_rollweg_an_der_kante() {
        // Ein Punkt rund 23 m links der Mittellinie bei etwa 900 m — genau
        // auf der Kante einer 46-m-Bahn.
        let (lat, lon) = punkt_auf_bahn(900.0, -23.0);
        let g = karte(&rollweg("D8", lat, lon));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0].name, "D8");
        assert_eq!(a[0].seite, "left");
        assert!((a[0].laengs_m - 900.0).abs() < 15.0, "{}", a[0].laengs_m);
    }

    #[test]
    fn ignoriert_was_die_bahn_nicht_beruehrt() {
        // Ein Rollweg, der PARALLEL zur Bahn verläuft und sie nie berührt.
        //
        // Beide Stützpunkte liegen hundert Meter seitlich — das ist der
        // Fall, den die Kantentoleranz ausschliessen muss. (Der erste
        // Anlauf dieses Tests nutzte den Zwei-Punkte-Helfer, dessen
        // zweiter Punkt schräg versetzt lag und die Kante zufällig traf.
        // Ein Test, der aus Versehen etwas anderes prüft, prüft nichts.)
        let (lat1, lon1) = punkt_auf_bahn(900.0, -100.0);
        let (lat2, lon2) = punkt_auf_bahn(1200.0, -100.0);
        let g = karte(&format!(
            r#"{{"type":"Feature","properties":{{"k":"taxiway","r":"P1"}},
                 "geometry":{{"type":"LineString","coordinates":[[{lon1},{lat1}],[{lon2},{lat2}]]}}}}"#
        ));
        assert!(
            ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0).is_empty(),
            "ein paralleler Rollweg ist keine Ausfahrt"
        );
    }

    #[test]
    fn ignoriert_unbenannte_rollwege() {
        // Ein Stummel ohne Kennung sagt nichts, das die Spur nicht zeigt.
        let (lat, lon) = punkt_auf_bahn(900.0, -23.0);
        let g = karte(&format!(
            r#"{{"type":"Feature","properties":{{"k":"taxiway"}},
                 "geometry":{{"type":"LineString","coordinates":[[{lon},{lat}],[{lon},{lat}]]}}}}"#
        ));
        assert!(ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0).is_empty());
    }

    #[test]
    fn ein_rollweg_gibt_einen_stummel_je_seite() {
        // Ein Rollweg beruehrt die Bahn oft mit mehreren Stuetzpunkten.
        // Ohne Auswahl bekaeme er mehrere Stummel nebeneinander.
        let (lat1, lon1) = punkt_auf_bahn(900.0, -23.0);
        let (lat2, lon2) = punkt_auf_bahn(910.0, -24.0);
        let (lat3, lon3) = punkt_auf_bahn(920.0, -26.0);
        let g = karte(&format!(
            r#"{{"type":"Feature","properties":{{"k":"taxiway","r":"D8"}},
                 "geometry":{{"type":"LineString","coordinates":[[{lon1},{lat1}],[{lon2},{lat2}],[{lon3},{lat3}]]}}}}"#
        ));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1, "{a:?}");
    }

    #[test]
    fn beide_seiten_werden_unterschieden() {
        let (la, lo) = punkt_auf_bahn(900.0, -23.0);
        let (ra, ro) = punkt_auf_bahn(1200.0, 23.0);
        let g = karte(&format!(
            "{},{}",
            rollweg("D8", la, lo),
            rollweg("D7", ra, ro)
        ));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 2, "{a:?}");
        // Nach Laengsposition sortiert.
        assert_eq!(a[0].name, "D8");
        assert_eq!(a[0].seite, "left");
        assert_eq!(a[1].seite, "right");
    }

    #[test]
    fn kaputte_karte_liefert_nichts_statt_zu_stuerzen() {
        for g in ["", "{}", "nicht json", r#"{"features":"x"}"#] {
            assert!(
                ausfahrten_fuer_bahn(g, T.0, T.1, T.2, T.3, 46.0).is_empty(),
                "{g}"
            );
        }
    }

    #[test]
    fn nur_taxiways_zaehlen() {
        // Haltepunkte und Staende liegen ebenfalls nahe der Bahn.
        let (lat, lon) = punkt_auf_bahn(900.0, -23.0);
        let g = karte(&format!(
            r#"{{"type":"Feature","properties":{{"k":"holding_position","r":"H1"}},
                 "geometry":{{"type":"LineString","coordinates":[[{lon},{lat}],[{lon},{lat}]]}}}}"#
        ));
        assert!(ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0).is_empty());
    }

    /// Ein Punkt auf der Bahnachse: `laengs` Meter nach der Schwelle,
    /// `quer` Meter seitlich (positiv rechts in Landerichtung).
    fn punkt_auf_bahn(laengs: f64, quer: f64) -> (f64, f64) {
        // Kurs der Bahn.
        let dlat = (T.2 - T.0).to_radians();
        let dlon = (T.3 - T.1).to_radians();
        let m1 = T.0.to_radians();
        let m2 = T.2.to_radians();
        let y = dlon.sin() * m2.cos();
        let x = m1.cos() * m2.sin() - m1.sin() * m2.cos() * dlon.cos();
        let _ = dlat;
        let kurs = y.atan2(x);
        // Erst laengs, dann quer (Kurs + 90°).
        const R: f64 = 6_371_008.8;
        let vor = |lat: f64, lon: f64, d: f64, k: f64| {
            let p1 = lat.to_radians();
            let l1 = lon.to_radians();
            let p2 = (p1.sin() * (d / R).cos() + p1.cos() * (d / R).sin() * k.cos()).asin();
            let l2 = l1
                + (k.sin() * (d / R).sin() * p1.cos()).atan2((d / R).cos() - p1.sin() * p2.sin());
            (p2.to_degrees(), l2.to_degrees())
        };
        let (a, b) = vor(T.0, T.1, laengs, kurs);
        vor(
            a,
            b,
            quer.abs(),
            kurs + if quer >= 0.0 {
                std::f64::consts::FRAC_PI_2
            } else {
                -std::f64::consts::FRAC_PI_2
            },
        )
    }
}

#[cfg(test)]
mod qs_eddm {
    /// Der Verlauf an einer ECHTEN Bodenkarte — Muenchen 26L.
    ///
    /// Braucht `/tmp/eddm_ground.json` (vom Live-Server). Ohne die Datei
    /// uebersprungen, deshalb `#[ignore]`: In der CI liegt sie nicht, und ein
    /// Test, der dort still nichts prueft, waere schlimmer als keiner.
    ///
    /// Lauf: `cargo test -p aeroacars-app --lib qs_eddm -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn qs_eddm_verlauf_am_echten_flughafen() {
        let Ok(karte) = std::fs::read_to_string("/tmp/eddm_ground.json") else {
            println!("keine EDDM-Karte in /tmp — uebersprungen");
            return;
        };
        // EDDM 26L, echte Geometrie.
        let alle = super::ausfahrten_fuer_bahn(
            &karte,
            48.34479722,
            11.80461389,
            48.34066944,
            11.75101667,
            60.0,
        );
        let mit = alle.iter().filter(|a| !a.verlauf.is_empty()).count();
        let punkte: usize = alle.iter().map(|a| a.verlauf.len()).sum();
        let voll = serde_json::to_string(&alle).unwrap().len();
        let mut nur_eine = alle.clone();
        // So wie `bahn_felder` zuschneidet: nur die genommene Ausfahrt.
        let raeum = 2345.0_f64;
        for a in nur_eine.iter_mut() {
            if a.seite != "right" || (a.laengs_m - raeum).abs() > 200.0 {
                a.verlauf.clear();
            }
        }
        let knapp = serde_json::to_string(&nur_eine).unwrap().len();
        println!(
            "EDDM 26L: {} Ausfahrten, {} mit Verlauf, {} Punkte",
            alle.len(),
            mit,
            punkte
        );
        println!("  ungekuerzt: {} B   zugeschnitten: {} B", voll, knapp);
        for a in alle.iter().take(6) {
            println!(
                "  {:>8} bei {:>7.0} m {:>5}  Verlauf {} Punkte",
                a.name,
                a.laengs_m,
                a.seite,
                a.verlauf.len()
            );
        }
        // Fuer die Anzeige-Pruefung herausschreiben.
        std::fs::write(
            "/tmp/eddm_ausfahrten.json",
            serde_json::to_string(&nur_eine).unwrap(),
        )
        .ok();
        assert!(!alle.is_empty(), "keine Ausfahrten aus der echten Karte");
        assert!(mit > 0, "keine einzige Ausfahrt traegt einen Verlauf");
    }

    /// Wie viel Korridor der echte Bestand hergibt.
    ///
    /// Laeuft nur, wenn `/tmp/korridor_stich.json` daliegt (vom
    /// Live-Server gezogen). Kein Teil der normalen Reihe.
    #[test]
    #[ignore]
    fn qs_korridor_am_bestand() {
        let Ok(roh) = std::fs::read_to_string("/tmp/korridor_stich.json") else {
            println!("keine Stichprobe — uebersprungen");
            return;
        };
        let daten: serde_json::Value = serde_json::from_str(&roh).unwrap();
        let mut bahnen = 0usize;
        let mut ausfahrten_gesamt = 0usize;
        let mut mit_verlauf = 0usize;
        let mut erreicht_mitte = 0usize;
        let mut punkte: Vec<usize> = Vec::new();
        for z in daten.as_array().unwrap() {
            let karte = z["geojson"].as_str().unwrap();
            let (tlat, tlon) = (z["tlat"].as_f64().unwrap(), z["tlon"].as_f64().unwrap());
            let (elat, elon) = (z["elat"].as_f64().unwrap(), z["elon"].as_f64().unwrap());
            let breite = z["width_ft"].as_f64().unwrap() * 0.3048;
            let a = super::ausfahrten_fuer_bahn(karte, tlat, tlon, elat, elon, breite);
            if a.is_empty() {
                continue;
            }
            bahnen += 1;
            for x in &a {
                ausfahrten_gesamt += 1;
                if x.verlauf.len() >= 2 {
                    mit_verlauf += 1;
                    punkte.push(x.verlauf.len());
                    // „Voller" Korridor: reicht bis in die Naehe der Mitte.
                    if x.verlauf.iter().any(|v| v.quer_m.abs() <= 10.0) {
                        erreicht_mitte += 1;
                    }
                }
            }
        }
        punkte.sort_unstable();
        let median = punkte.get(punkte.len() / 2).copied().unwrap_or(0);
        println!("Bahnen mit Ausfahrten:      {bahnen}");
        println!("Ausfahrten gesamt:          {ausfahrten_gesamt}");
        println!(
            "davon mit Korridor (>=2 P): {mit_verlauf}  ({:.0} %)",
            100.0 * mit_verlauf as f64 / ausfahrten_gesamt.max(1) as f64
        );
        println!(
            "davon bis zur Mitte:        {erreicht_mitte}  ({:.0} % aller)",
            100.0 * erreicht_mitte as f64 / ausfahrten_gesamt.max(1) as f64
        );
        println!("Punkte je Korridor, Median: {median}");
    }
}
