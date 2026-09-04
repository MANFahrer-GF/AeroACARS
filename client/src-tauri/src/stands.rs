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
    ///
    /// Bewusster Trade-off: Lead-in-Linien BEGINNEN am Rollweg — ein
    /// Parkbrems-Halt am Linienanfang kann damit als „am Stand" zählen.
    /// Das fängt der BlocksOn→TaxiIn-Rückweg (wer weiterrollt, verwirft
    /// Block-On-Zeit UND Stand); der Endzustand bleibt korrekt.
    pub linie: Option<Vec<(f64, f64)>>,
    /// `true`, wenn die Geometrie eine Fläche war (Polygon-Außenring in
    /// `linie`): dann gilt „drin = 0 m", nicht nur Ringnähe.
    pub flaeche: bool,
}

/// Wie nah (Meter) der Stillstand an einer Parkposition liegen muss,
/// damit er als Block-On zählt. Kalibriert am RYR-1142-Beweisflug:
/// echte Parkposition 17 m vom OSM-Punkt, Nachbar-Stand 34 m,
/// der fehlgedeutete H4-Halt 423 m. 60 m fängt Szenerie-Versatz ab
/// und bleibt weit unter der Rollweg-Distanz.
pub const STAND_CAPTURE_RADIUS_M: f64 = 60.0;

/// Radius für die NAMENS-Zuordnung. War in der v1.6.1-QS testweise auf
/// 30 m verengt (Sorge: falscher Nachbar-Name auf einer ungetaggten
/// Position). REVERTIERT nach dem ersten Live-Tag mit echten Daten:
/// LGAV-Ankunft (13.08.2026), Stand C37 war der naechstliegende Punkt
/// UEBERHAUPT (kein naeherer unbenannter Kandidat), aber 40,2 m entfernt
/// — mit 30 m blieb arr_gate leer, obwohl die Zuordnung eindeutig war.
/// Der Schutz vor Fehlbenennung kommt seit der v1.6.2-QS nicht mehr aus
/// dem Radius, sondern aus `STAND_NAME_VORSPRUNG_M`: liegt ein UNBENANNTER
/// Stand klar naeher, steht der Flieger dort und der benannte Nachbar
/// bekommt den Namen nicht. Damit gilt beides — LGAV/C37 wird benannt,
/// und am dichten Vorfeld wandert kein Nachbarname ins PIREP.
pub const STAND_NAME_RADIUS_M: f64 = 60.0;

/// Wieviel naeher ein unbenannter Stand liegen muss, damit er den benannten
/// Namen verwirft. Kleine Unterschiede sind Szenerie-Rauschen; erst ein
/// klarer Vorsprung heisst „der Flieger steht woanders".
const STAND_NAME_VORSPRUNG_M: f64 = 10.0;

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
        let Some(geom) = f.get("geometry") else {
            continue;
        };
        let gtype = geom.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let coords = geom.get("coordinates");
        // Punkt: die Stopp-Position selbst. Linie: letzter Stützpunkt als
        // Repräsentant, aber die VOLLE Linie wandert mit in den Stand —
        // Flugzeuge stehen auf dem ganzen Lead-in, nicht nur am Ende
        // (EDDF-Befund, s. Struct-Doku). Polygon: Außenring wie eine Linie.
        let (point, linie): (Option<(f64, f64)>, Option<Vec<(f64, f64)>>) = match (gtype, coords) {
            ("Point", Some(c)) => (lonlat(c), None),
            ("LineString", Some(c)) => {
                let pts = linien_punkte(c);
                // Degenerierte Ein-Punkt-"Linie": als Punkt-Stand
                // weiterleben lassen (QS-Befund — vorher fiel er weg).
                let punkt = pts
                    .as_ref()
                    .and_then(|p| p.last().copied())
                    .or_else(|| {
                        c.as_array()
                            .and_then(|a| a.last())
                            .and_then(lonlat)
                            .map(|(lon, lat)| (lat, lon))
                    })
                    .map(|(la, lo)| (lo, la));
                (punkt, pts)
            }
            ("Polygon", Some(c)) => {
                let pts = c
                    .as_array()
                    .and_then(|rings| rings.first())
                    .and_then(linien_punkte);
                (
                    pts.as_ref()
                        .and_then(|p| p.last().copied().map(|(la, lo)| (lo, la))),
                    pts,
                )
            }
            _ => (None, None),
        };
        if let Some((lon, lat)) = point {
            let flaeche = gtype == "Polygon";
            stands.push(ParkingStand {
                name,
                lat,
                lon,
                linie,
                flaeche,
            });
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
    if pts.len() >= 2 {
        Some(pts)
    } else {
        None
    }
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
    // Flächig gemappter Stand: WER DRAUF STEHT, ist am Stand — Abstand 0.
    // (QS-Befund: große GA-Aprons als ein Polygon; die Ringnähe allein
    // verfehlte die Mitte.) Ray-Cast in der lokalen Projektion.
    if stand.flaeche {
        let ring: Vec<(f64, f64)> = linie.iter().map(|&q| proj(q)).collect();
        if punkt_in_ring(p, &ring) {
            return 0.0;
        }
    }
    let mut best = f64::INFINITY;
    for seg in linie.windows(2) {
        let a = proj(seg[0]);
        let b = proj(seg[1]);
        best = best.min(punkt_strecke_m(p, a, b));
    }
    best
}

/// Punkt-in-Polygon (Ray-Cast) in ebenen Koordinaten. Der Ring muss
/// nicht explizit geschlossen sein — das letzte Segment wird ergänzt.
fn punkt_in_ring(p: (f64, f64), ring: &[(f64, f64)]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    let (px, py) = p;
    let mut drin = false;
    let mut j = ring.len() - 1;
    for i in 0..ring.len() {
        let (xi, yi) = ring[i];
        let (xj, yj) = ring[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            drin = !drin;
        }
        j = i;
    }
    drin
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

/// Parkpositionen aus der Szenerie des Simulators — X-Plane `apt.dat`
/// (Zeilencode `1300`) oder MSFS' `TAXI_PARKING`-Facility-Daten.
///
/// Beide sind die ERSTE Instanz (der Szenerie-Entwickler selbst), nicht
/// OpenStreetMap. Direkte Abbildung, kein GeoJSON-Umweg — Szenerie-Stände
/// sind immer Punkte, nie Linien oder Flächen wie manche OSM-Positionen.
pub fn aus_szenerie(staende: &[sim_core::szenerie::SzenerieStand]) -> Vec<ParkingStand> {
    staende
        .iter()
        .map(|s| ParkingStand {
            name: s.name.clone(),
            lat: s.lat,
            lon: s.lon,
            linie: None,
            flaeche: false,
        })
        .collect()
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
    let benannt = stands
        .iter()
        .filter(|s| s.name.is_some())
        .map(|s| (s, abstand_m(s, lat, lon)))
        .filter(|(_, d)| *d <= STAND_NAME_RADIUS_M)
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    // Steht ein UNBENANNTER Stand naeher, gehoert der Flieger dorthin — dann
    // ist der benannte Nachbar der falsche Name (QS-Befund v1.6.2: der weite
    // Radius allein liess sonst an dichten Vorfeldern, wo 40-55 m Standabstand
    // normal sind, den Nachbarnamen ins PIREP wandern). Ein leeres Feld ist
    // besser als ein falscher Stand.
    let naechster = nearest(stands, lat, lon)?;
    if naechster.1 + STAND_NAME_VORSPRUNG_M < benannt.1 {
        return None;
    }
    Some(benannt)
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
    fn benannter_stand_gewinnt_bei_aehnlicher_entfernung() {
        // Namenloser Punkt 10 m, benannter Stand 15 m: der Vorsprung des
        // namenlosen liegt unter STAND_NAME_VORSPRUNG_M — das ist
        // Szenerie-Rauschen, der Name gilt. (Der Gegenfall, ein klar
        // naeherer unbenannter Stand, steht in
        // `naeherer_unbenannter_stand_verwirft_den_nachbarnamen`.)
        let stands = vec![
            ParkingStand {
                name: None,
                lat: 50.00009,
                lon: 8.0,
                linie: None,
                flaeche: false,
            },
            ParkingStand {
                name: Some("A22".into()),
                lat: 50.000_135,
                lon: 8.0,
                linie: None,
                flaeche: false,
            },
        ];
        let bei = stand_at(&stands, 50.0, 8.0).expect("in Standnähe");
        assert_eq!(bei.name, None, "die Naehe-Frage gewinnt der naechste Punkt");
        let (benannt, d) = benannter_stand_bei(&stands, 50.0, 8.0).expect("benannt");
        assert_eq!(benannt.name.as_deref(), Some("A22"));
        assert!(d < 30.0);
    }

    #[test]
    fn namens_radius_findet_benannten_nachbarn_im_capture_radius() {
        // Ungetaggte Position unterm Flieger, benannter Nachbar 45 m
        // weiter (innerhalb STAND_CAPTURE_RADIUS_M = STAND_NAME_RADIUS_M):
        // der Name wird verwendet. Live-Beleg LGAV/C37 (13.08.2026): der
        // naechste — und einzige — Kandidat lag 40,2 m entfernt; ein enger
        // Namensradius liess das Feld leer, obwohl die Zuordnung eindeutig
        // war. `benannter_stand_bei` waehlt immer den NAECHSTEN benannten
        // Stand, das schuetzt vor Fehlbenennung unabhaengig vom Radius.
        // Live-Beleg LGAV/C37: der benannte Stand war der NAECHSTE Kandidat
        // ueberhaupt, nur 40 m weg — kein naeherer unbenannter dazwischen.
        let stands = vec![
            ParkingStand {
                name: Some("C37".into()),
                lat: 50.00036,
                lon: 8.0,
                linie: None,
                flaeche: false,
            },
            ParkingStand {
                name: Some("C39".into()),
                lat: 50.00057,
                lon: 8.0,
                linie: None,
                flaeche: false,
            },
        ];
        assert!(stand_at(&stands, 50.0, 8.0).is_some(), "Naehe ja");
        let (s, _) = benannter_stand_bei(&stands, 50.0, 8.0).expect("Name im Capture-Radius");
        assert_eq!(s.name.as_deref(), Some("C37"));
    }

    #[test]
    fn naeherer_unbenannter_stand_verwirft_den_nachbarnamen() {
        // Dichtes Vorfeld: Flieger steht auf einer ungetaggten Position,
        // benannter Nachbar 45 m weiter. Der Name gehoert NICHT ins PIREP.
        let stands = vec![
            ParkingStand {
                name: None,
                lat: 50.0,
                lon: 8.0,
                linie: None,
                flaeche: false,
            },
            ParkingStand {
                name: Some("B10".into()),
                lat: 50.00041,
                lon: 8.0,
                linie: None,
                flaeche: false,
            },
        ];
        assert!(stand_at(&stands, 50.0, 8.0).is_some(), "Naehe ja");
        assert!(
            benannter_stand_bei(&stands, 50.0, 8.0).is_none(),
            "naeherer unbenannter Stand muss den Nachbarnamen verwerfen"
        );
    }

    #[test]
    fn namens_radius_ignoriert_stand_ausserhalb_des_capture_radius() {
        // Weiterhin eine Grenze: ein Stand jenseits STAND_CAPTURE_RADIUS_M
        // (60 m) ist zu weit weg, um noch "derselbe Stand" zu sein.
        let stands = vec![ParkingStand {
            name: Some("Z9".into()),
            lat: 50.00061, // ~68 m
            lon: 8.0,
            linie: None,
            flaeche: false,
        }];
        assert!(benannter_stand_bei(&stands, 50.0, 8.0).is_none());
    }

    #[test]
    fn polygon_stand_mitte_zaehlt_als_drauf() {
        // ~220x220-m-Apron als ein parking_position-Polygon: die Mitte
        // ist >60 m vom Ring entfernt und muss trotzdem matchen.
        let gj = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{"k":"parking_position","r":"APRON"},"geometry":{"type":"Polygon","coordinates":[[[8.0,50.0],[8.003,50.0],[8.003,50.002],[8.0,50.002],[8.0,50.0]]]}}
        ]}"#;
        let stands = parse_stands(gj);
        assert_eq!(stands.len(), 1);
        let s = stand_at(&stands, 50.001, 8.0015).expect("mitten drauf");
        assert_eq!(s.name.as_deref(), Some("APRON"));
    }

    #[test]
    fn einpunkt_linestring_bleibt_als_punkt_stand() {
        let gj = r#"{"type":"FeatureCollection","features":[
          {"type":"Feature","properties":{"k":"parking_position","r":"X1"},"geometry":{"type":"LineString","coordinates":[[8.0,50.0]]}}
        ]}"#;
        let stands = parse_stands(gj);
        assert_eq!(stands.len(), 1);
        assert_eq!(stands[0].linie, None);
        assert!(stand_at(&stands, 50.0, 8.0).is_some());
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

    #[test]
    fn aus_szenerie_bildet_direkt_ohne_geometrie_ab() {
        let sz = vec![
            sim_core::szenerie::SzenerieStand {
                name: Some("A1".into()),
                lat: 50.0,
                lon: 8.0,
            },
            sim_core::szenerie::SzenerieStand {
                name: None,
                lat: 50.001,
                lon: 8.001,
            },
        ];
        let stands = aus_szenerie(&sz);
        assert_eq!(stands.len(), 2);
        assert_eq!(stands[0].name.as_deref(), Some("A1"));
        assert_eq!(stands[0].linie, None);
        assert!(!stands[0].flaeche);
        assert_eq!(stands[1].name, None);
        // Die Naehe-Frage funktioniert wie bei OSM-Staenden ohne Weiteres.
        let s = stand_at(&stands, 50.0, 8.0).expect("am Szenerie-Stand");
        assert_eq!(s.name.as_deref(), Some("A1"));
    }
}
