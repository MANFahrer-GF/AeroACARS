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
    pub lat: f64,
    pub lon: f64,
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
        let point: Option<(f64, f64)> = match (gtype, coords) {
            ("Point", Some(c)) => lonlat(c),
            // Lead-in-Linie: letzter Stützpunkt = Parkposition.
            ("LineString", Some(c)) => c.as_array().and_then(|a| a.last()).and_then(lonlat),
            _ => None,
        };
        if let Some((lon, lat)) = point {
            stands.push(ParkingStand { name, lat, lon });
        }
    }
    stands
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
        .map(|s| (s, ::geo::distance_m(lat, lon, s.lat, s.lon)))
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

    #[test]
    fn malformed_geojson_yields_empty() {
        assert!(parse_stands("not json").is_empty());
        assert!(parse_stands("{}").is_empty());
        assert!(parse_stands(r#"{"features":[{"geometry":null}]}"#).is_empty());
    }
}
