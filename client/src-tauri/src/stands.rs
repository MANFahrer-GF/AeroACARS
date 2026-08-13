//! Stand-/Gate-Erkennung aus den OSM-Bodendaten (`airport_ground`).
//!
//! v1.6 (RYR-1142-Befund, LPPT 2026-08-11): Die BlocksOn-Erkennung hing
//! allein an „Parkbremse + Stillstand" — ein längerer Halt am Rollweg-
//! Haltepunkt H4 wurde als Block-On gewertet, und die FSM kannte keinen
//! Weg zurück. Die nachhaltige Lösung: dieselben OSM-Bodendaten, die die
//! Taxi-Karte zeichnet, kennen jede `parking_position` samt Standnummer.
//! Ein Halt zählt nur noch als Block-On, wenn er IN Standnähe passiert —
//! und liefert dabei gleich den Standnamen für Log und PIREP (der
//! MSFS-eigene `parking_name`-Weg hat sich als tot erwiesen: SimConnect
//! befüllt das Feld nie).
//!
//! Rein positionsbasiert und damit sim-agnostisch: MSFS und X-Plane
//! laufen durch exakt denselben Code.

/// Eine Parkposition aus dem OSM-Ground-Layer.
#[derive(Debug, Clone, PartialEq)]
pub struct ParkingStand {
    /// Standnummer/-name (`ref` in OSM), z. B. "203" oder "A22".
    /// `None` bei ungetaggten Positionen — die Nähe zählt trotzdem
    /// als „an einem Stand", nur eben ohne Namen.
    pub name: Option<String>,
    /// Repräsentativer Punkt (bei Linien: der letzte Stützpunkt =
    /// Stopp-Position). Nur noch Anzeige-/Log-Wert — die NÄHE wird
    /// gegen die volle Geometrie gerechnet, siehe `linie`.
    pub lat: f64,
    pub lon: f64,
    /// Volle Linien-Geometrie `(lat, lon)` bei Lead-in-Linien (bzw.
    /// Umriss bei flächig gemappten Ständen). Feldbefund OCN 1408
    /// (Peter Z, EDDF V172, 13.08.2026): das Flugzeug stand 0,4 m
    /// NEBEN der Lead-in-Linie, aber 79 m vom letzten Stützpunkt —
    /// der alte Ein-Punkt-Abstand verfehlte den 60-m-Radius, und
    /// EDDF ist (wie viele große Plätze) komplett linien-gemappt.
    /// `None` bei Punkt-Geometrien.
    pub linie: Option<Vec<(f64, f64)>>,
}

/// Wie nah (Meter) der Stillstand an einer Parkposition liegen muss,
/// damit er als Block-On zählt. Kalibriert am RYR-1142-Beweisflug:
/// echte Parkposition 17 m vom OSM-Punkt, Nachbar-Stand 34 m,
/// der fehlgedeutete H4-Halt 423 m. 60 m fängt Szenerie-Versatz ab
/// und bleibt weit unter der Rollweg-Distanz.
pub const STAND_CAPTURE_RADIUS_M: f64 = 60.0;

/// Parkpositionen aus dem `airport_ground`-GeoJSON ziehen.
///
/// Der Server liefert minifizierte Properties (`k` = kind, `r` = ref);
/// zur Robustheit werden auch die Langformen akzeptiert. `Point`-Features
/// sind die Stopp-Position selbst; bei `LineString`s (Lead-in-Linien)
/// ist der LETZTE Stützpunkt die Parkposition — der erste liegt am
/// Rollweg und wäre als Näherungsziel falsch.
pub fn parse_stands(geojson: &str) -> Vec<ParkingStand> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(geojson) else {
        return Vec::new();
    };
    let Some(features) = root.get("features").and_then(|f| f.as_array()) else {
        return Vec::new();
    };
    let mut stands = Vec::new();
    for f in features {
        let props = f.get("properties").cloned().unwrap_or_default();
        let kind = props
            .get("k")
            .or_else(|| props.get("kind"))
            .or_else(|| props.get("aeroway"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if kind != "parking_position" {
            continue;
        }
        let name = props
            .get("r")
            .or_else(|| props.get("ref"))
            .or_else(|| props.get("name"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let Some(geom) = f.get("geometry") else { continue };
        let gtype = geom.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let coords = geom.get("coordinates");
        // Punkt: die Stopp-Position selbst. Linie: letzter Stützpunkt als
        // Repräsentant, aber die VOLLE Linie wandert mit in den Stand —
        // Flugzeuge stehen auf dem ganzen Lead-in, nicht nur am Ende
        // (EDDF-Befund, s. Struct-Doku). Polygon: Außenring wie eine Linie.
        let (point, linie): (Option<(f64, f64)>, Option<Vec<(f64, f64)>>) =
            match (gtype, coords) {
                ("Point", Some(c)) => (lonlat(c), None),
                ("LineString", Some(c)) => {
                    let pts = linien_punkte(c);
                    (pts.as_ref().and_then(|p| p.last().copied().map(|(la, lo)| (lo, la))), pts)
                }
                ("Polygon", Some(c)) => {
                    let pts = c.as_array().and_then(|rings| rings.first()).and_then(linien_punkte);
                    (pts.as_ref().and_then(|p| p.last().copied().map(|(la, lo)| (lo, la))), pts)
                }
                _ => (None, None),
            };
        if let Some((lon, lat)) = point {
            stands.push(ParkingStand { name, lat, lon, linie });
        }
    }
    stands
}

/// Alle Stützpunkte einer Koordinatenliste als `(lat, lon)` — `None`,
/// wenn weniger als zwei brauchbare Punkte übrig bleiben (eine „Linie"
/// aus einem Punkt ist ein Punkt).
fn linien_punkte(c: &serde_json::Value) -> Option<Vec<(f64, f64)>> {
    let pts: Vec<(f64, f64)> = c
        .as_array()?
        .iter()
        .filter_map(lonlat)
        .map(|(lon, lat)| (lat, lon))
        .collect();
    if pts.len() >= 2 { Some(pts) } else { None }
}

/// Abstand Flugzeug → Stand in Metern: gegen die volle Geometrie.
/// Linien werden segmentweise gerechnet (Punkt-zu-Strecke in einer
/// lokalen ebenen Projektion — auf Vorfeld-Skalen exakt genug),
/// Punkte wie bisher über die Haversine-Näherung.
pub fn abstand_m(stand: &ParkingStand, lat: f64, lon: f64) -> f64 {
    let Some(linie) = stand.linie.as_ref() else {
        return ::geo::distance_m(lat, lon, stand.lat, stand.lon);
    };
    // Lokale Projektion um die Flugzeugposition: 1° Breite ≈ 110 540 m,
    // 1° Länge ≈ 111 320 m · cos(Breite).
    let mx = 111_320.0 * lat.to_radians().cos();
    let my = 110_540.0;
    let p = (0.0, 0.0);
    let proj = |(la, lo): (f64, f64)| ((lo - lon) * mx, (la - lat) * my);
    let mut best = f64::INFINITY;
    for seg in linie.windows(2) {
        let a = proj(seg[0]);
        let b = proj(seg[1]);
        best = best.min(punkt_strecke_m(p, a, b));
    }
    best
}

/// Abstand Punkt→Strecke in der Ebene (Meter-Koordinaten).
fn punkt_strecke_m(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (px, py) = p;
    let (ax, ay) = a;
    let (bx, by) = b;
    let (dx, dy) = (bx - ax, by - ay);
    let len2 = dx * dx + dy * dy;
    let t = if len2 <= f64::EPSILON {
        0.0
    } else {
        (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0)
    };
    let (cx, cy) = (ax + t * dx, ay + t * dy);
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

fn lonlat(v: &serde_json::Value) -> Option<(f64, f64)> {
    let a = v.as_array()?;
    let lon = a.first()?.as_f64()?;
    let lat = a.get(1)?.as_f64()?;
    if lon.is_finite() && lat.is_finite() {
        Some((lon, lat))
    } else {
        None
    }
}

/// Nächste Parkposition zur gegebenen Flugzeugposition, mit Distanz in
/// Metern. `None` bei leerer Standliste.
pub fn nearest(stands: &[ParkingStand], lat: f64, lon: f64) -> Option<(&ParkingStand, f64)> {
    stands
        .iter()
        .map(|s| (s, abstand_m(s, lat, lon)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Der nächste BENANNTE Stand im Radius — für die Gate-Beschriftung.
/// `stand_at` kann einen namenlosen Nachbarn liefern (ungetaggte
/// `parking_position` 10 m näher als der getaggte Stand); für die Frage
/// „steht der Flieger an einem Stand?" ist das richtig, für den NAMEN
/// im PIREP wäre es ein leeres Feld, obwohl der richtige Name 20 m
/// weiter bereitliegt.
pub fn benannter_stand_bei(
    stands: &[ParkingStand],
    lat: f64,
    lon: f64,
) -> Option<(&ParkingStand, f64)> {
    stands
        .iter()
        .filter(|s| s.name.is_some())
        .map(|s| (s, abstand_m(s, lat, lon)))
        .filter(|(_, d)| *d <= STAND_CAPTURE_RADIUS_M)
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

/// Der Stand, AN dem das Flugzeug gerade steht — `Some` nur innerhalb
/// von [`STAND_CAPTURE_RADIUS_M`].
pub fn stand_at(stands: &[ParkingStand], lat: f64, lon: f64) -> Option<&ParkingStand> {
    match nearest(stands, lat, lon) {
        Some((s, d)) if d <= STAND_CAPTURE_RADIUS_M => Some(s),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ausschnitt aus dem echten LPPT-Ground-Layer (Server-Wire-Format,
    /// minifizierte Props) — Stand 203 als Point UND als Lead-in-Linie,
    /// dazu ein Holding-Point und ein Gate, die NICHT zählen dürfen.
    const LPPT_SNIPPET: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"k":"holding_position","r":"S4"},"geometry":{"type":"Point","coordinates":[-9.129008,38.797733]}},
      {"type":"Feature","properties":{"k":"parking_position","r":"203"},"geometry":{"type":"Point","coordinates":[-9.136807,38.764528]}},
      {"type":"Feature","properties":{"k":"gate","r":"203"},"geometry":{"type":"Point","coordinates":[-9.137135,38.764134]}},
      {"type":"Feature","properties":{"k":"parking_position","r":"204"},"geometry":{"type":"LineString","coordinates":[[-9.137447,38.765311],[-9.137202,38.764391]]}},
      {"type":"Feature","properties":{"k":"parking_position"},"geometry":{"type":"Point","coordinates":[-9.135,38.766]}}
    ]}"#;

    #[test]
    fn parses_only_parking_positions() {
        let stands = parse_stands(LPPT_SNIPPET);
        // 2 Points + 1 LineString-Endpunkt; holding_position + gate fliegen raus.
        assert_eq!(stands.len(), 3);
        assert_eq!(stands[0].name.as_deref(), Some("203"));
        // LineString: LETZTER Punkt ist die Parkposition.
        let s204 = &stands[1];
        assert_eq!(s204.name.as_deref(), Some("204"));
        assert!((s204.lat - 38.764391).abs() < 1e-9);
        // Ungetaggte Position bleibt drin, nur ohne Namen.
        assert_eq!(stands[2].name, None);
    }

    #[test]
    fn ryr1142_regression_h4_hold_is_not_a_stand() {
        let stands = parse_stands(LPPT_SNIPPET);
        // Der echte H4-Halt vom 2026-08-11 (423 m vom nächsten Stand):
        assert!(stand_at(&stands, 38.7830, -9.1327).is_none());
        // Die echte Parkposition an Stand 203 (17 m):
        let s = stand_at(&stands, 38.7646843, -9.1368409).expect("am Stand");
        assert_eq!(s.name.as_deref(), Some("203"));
    }

    /// Der echte V172-Lead-in aus dem EDDF-Ground-Layer (OSM) plus die
    /// Nachbarlinie V171B — und Peters echte Parkposition vom 13.08.2026
    /// (OCN 1408): 0,4 m neben der Linie, 79 m vom letzten Stützpunkt.
    const EDDF_V172: &str = r#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"k":"parking_position","r":"V172"},"geometry":{"type":"LineString","coordinates":[[8.540982,50.037701],[8.541441,50.036901],[8.541755,50.036356]]}},
      {"type":"Feature","properties":{"k":"parking_position","r":"V171B"},"geometry":{"type":"LineString","coordinates":[[8.541443,50.037813],[8.541871,50.037056],[8.5422,50.036474]]}}
    ]}"#;

    #[test]
    fn ocn1408_regression_aircraft_midway_on_leadin_matches() {
        let stands = parse_stands(EDDF_V172);
        let (lat, lon) = (50.0370253271194, 8.54136441317754);
        // Die Regression: der Ein-Punkt-Abstand (letzter Stützpunkt) wäre
        // ~79 m und damit außerhalb des 60-m-Radius. Gegen die volle
        // Linie gerechnet sind es unter 2 m.
        let s = stand_at(&stands, lat, lon).expect("Peter steht AN V172");
        assert_eq!(s.name.as_deref(), Some("V172"));
        let (_, d) = nearest(&stands, lat, lon).unwrap();
        assert!(d < 2.0, "Linienabstand muss unter 2 m liegen, war {d}");
    }

    #[test]
    fn benannter_stand_ueberspringt_namenlosen_nachbarn() {
        // Namenloser Punkt 10 m neben dem Flieger, benannter Stand 25 m —
        // stand_at liefert den namenlosen (korrekt für die Nähe-Frage),
        // benannter_stand_bei liefert den Namen fürs PIREP.
        let stands = vec![
            ParkingStand { name: None, lat: 50.00009, lon: 8.0, linie: None },
            ParkingStand { name: Some("A22".into()), lat: 50.00022, lon: 8.0, linie: None },
        ];
        let bei = stand_at(&stands, 50.0, 8.0).expect("in Standnähe");
        assert_eq!(bei.name, None);
        let (benannt, d) = benannter_stand_bei(&stands, 50.0, 8.0).expect("benannt");
        assert_eq!(benannt.name.as_deref(), Some("A22"));
        assert!(d < 30.0);
    }

    #[test]
    fn punkt_strecke_klemmt_an_den_enden() {
        // Hinter dem Linienende zählt der Endpunkt, nicht die Verlängerung.
        let d = punkt_strecke_m((10.0, 0.0), (0.0, 0.0), (5.0, 0.0));
        assert!((d - 5.0).abs() < 1e-9);
        // Mitten auf der Strecke: senkrechter Abstand.
        let d2 = punkt_strecke_m((2.5, 3.0), (0.0, 0.0), (5.0, 0.0));
        assert!((d2 - 3.0).abs() < 1e-9);
    }

    #[test]
    fn malformed_geojson_yields_empty() {
        assert!(parse_stands("not json").is_empty());
        assert!(parse_stands("{}").is_empty());
        assert!(parse_stands(r#"{"features":[{"geometry":null}]}"#).is_empty());
    }
}
