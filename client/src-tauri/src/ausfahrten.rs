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

/// Eine Ausfahrt: wo ein benannter Rollweg die Bahnkante trifft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ausfahrt {
    /// Kennung des Rollwegs, z. B. `S4`.
    pub name: String,
    /// Distanz ab der Landeschwelle, in Metern.
    pub laengs_m: f64,
    /// Auf welcher Seite: `"left"` oder `"right"` in Landerichtung.
    pub seite: String,
}

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
    let mut beste: Vec<(String, String, f64, f64)> = Vec::new();
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
        let Some(punkte) = geom.and_then(|g| g.get("coordinates")).and_then(|c| c.as_array())
        else {
            continue;
        };

        for p in punkte {
            let Some((lon, lat)) = lonlat(p) else { continue };
            let (laengs, quer) = crate::runway::projiziere_auf_bahn(
                threshold_lat,
                threshold_lon,
                end_lat,
                end_lon,
                lat,
                lon,
            );
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
                .find(|(n, s, _, _)| n == name && s == seite)
            {
                Some(eintrag) if kantenabstand < eintrag.3 => {
                    eintrag.2 = laengs;
                    eintrag.3 = kantenabstand;
                }
                Some(_) => {}
                None => beste.push((
                    name.to_string(),
                    seite.to_string(),
                    laengs,
                    kantenabstand,
                )),
            }
        }
    }

    beste.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    beste
        .into_iter()
        .map(|(name, seite, laengs_m, _)| Ausfahrt {
            name,
            laengs_m: (laengs_m * 10.0).round() / 10.0,
            seite,
        })
        .collect()
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
        let g = karte(&format!("{},{}", rollweg("D8", la, lo), rollweg("D7", ra, ro)));
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
            assert!(ausfahrten_fuer_bahn(g, T.0, T.1, T.2, T.3, 46.0).is_empty(), "{g}");
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
        vor(a, b, quer.abs(), kurs + if quer >= 0.0 { 1.5708 } else { -1.5708 })
    }
}
