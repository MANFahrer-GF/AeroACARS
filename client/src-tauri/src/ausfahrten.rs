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
    /// Der letzte Stützpunkt auf der Bahn vor dem Übergang — nur intern:
    /// Zwei Rollwege, die am selben Punkt abzweigen, sind dieselbe
    /// Abzweigung, auch wenn ihre Schnittpunkte mit der Kante auseinander
    /// liegen (EDDG B und S1, `doppelnamen_zusammenfassen`).
    #[serde(skip)]
    pub abzweig: Option<(f64, f64)>,
}

// Mindestabstand zweier Verlaufspunkte, in Metern.
//
// Rollwege in OpenStreetMap haben teils Stützpunkte im Meterabstand. Für
// eine Zeichnung reicht deutlich weniger, und jede Landung traegt diese
// Punkte durch die Leitung — bei vierzehn Ausfahrten summiert sich das.
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> verlauf_min_abstand_m (v2.1.24: 8.0)

// Wie weit der Verlauf vor und hinter der Kante mitgenommen wird.
//
// Vierhundert Meter hinter der Kante: So weit reicht ein
// Schnellabrollweg, bis er in den parallelen Rollweg einmündet — bei
// EDDM B6 sind es rund dreihundert. Davor genügen fünfzig; weiter
// vorn liegt der Rollweg auf der Bahn und ist dort nicht zu zeichnen.
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> verlauf_vor_m (v2.1.24: 50.0)
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> verlauf_nach_m (v2.1.24: 400.0)

// Wie weit der Verlauf ueber die Bahnkante hinaus mitgenommen wird.
//
// Bis v1.7.39 waren es sechs Meter — passend fuer die ueberhoehte
// Queransicht, die neben der Kante nur einen schmalen Gruenstreifen zeigt.
// Seit es die Lupe „Abrollen im echten Massstab" gibt, wird der Rollweg
// dort gezeichnet, wo das Flugzeug hinrollt: Die Spur laeuft bis achtzig
// Meter neben die Kante (`BAHN_SPUR_RAND_M`), der Rollweg muss darueber
// hinausreichen, sonst endet er mitten in der Kurve.
//
// Die Queransicht beschneidet selbst (`Math.abs(v.quer_m) <= sichtbarM`)
// und bekommt deshalb nichts Zusaetzliches ins Bild.
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> verlauf_quer_ueber_kante_m (v2.1.24: 150.0)

// Wie nah ein Stützpunkt an der Bahnkante liegen muss, um als Ausfahrt zu
// zählen, in Metern.
//
// Fünfundzwanzig Meter: Rollwege sind in OpenStreetMap als Mittellinie
// erfasst, und ihr erster Stützpunkt liegt selten exakt auf der Kante —
// mal ein Stück davor, mal schon auf der Bahn. Enger gefasst fallen echte
// Ausfahrten heraus; weiter gefasst zählen parallel verlaufende Rollwege
// mit, die die Bahn nie berühren.
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> kanten_toleranz_m (v2.1.24: 25.0)



// Wie weit ein Rollweg über die Kante hinausreichen muss, damit er die
// Bahn auf dieser Seite wirklich verlässt, in Metern.
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> draussen_min_m (v2.1.24: 2.0)

// Wie weit zwei Übergänge desselben Namens auf derselben Seite höchstens
// auseinander liegen, um EINE Ausfahrt zu sein, in Metern.
//
// Ein Name kann an einer Bahn zweimal vorkommen: In den OSM-Bodenkarten
// berühren an 30 Bahnseiten zwei getrennte Rollwegstücke gleichen Namens
// dieselbe Seite, über 300 m auseinander (LFPG Z2/Z3, KMIA K2, VABB S1;
// gemessen 28.10.2026 über 473 Karten). Bis hier galt „ein Stummel je Name
// und Seite“, und die zweite Ausfahrt fiel weg. Mehrere Stützpunkte DESSELBEN
// Übergangs liegen dagegen wenige Meter auseinander
// (`ein_rollweg_gibt_einen_stummel_je_seite`).
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> dieselbe_stelle_m (v2.1.24: 150.0)

/// Wo ein Rollwegzug die Bahn verlässt: (laengs, Kantenabstand, Seite).
///
/// # Nur echte Ausfahrten (#1936, EDDG 25, 22.10.2026)
///
/// Bis 2.1.7 zählte JEDER Stützpunkt bis 25 m von der Kante — auch einer
/// auf der Mittellinie einer schmalen Bahn (22,6 m von der Kante), und die
/// Seite kam aus dem Vorzeichen dieses Punktes. Die MSFS-Szenerie führt
/// B und S1 in Dortmund ab der Mittellinie (0,3 m rechts davon) auf der
/// Bahn bis zum Bahnende, wo sie nach links abbiegen. Gezeigt wurden „B/S1“
/// rechts bei 1839,6 m — eine Ausfahrt, die es nicht gibt.
///
/// # Der Übergang, nicht der kantennächste Punkt (Codex 22.10.2026)
///
/// Eine Ausfahrt ist die Stelle, an der der Linienzug von der Bahn nach
/// aussen übergeht und dort mindestens `DRAUSSEN_MIN_M` über die Kante
/// hinausreicht. Läuft ein Rollweg lange knapp innen an der Kante entlang,
/// zählt erst der Übergang am Ende, nicht sein erster kantennaher Punkt.
/// Die Stelle ist immer der Schnittpunkt des Segments mit der Kante
/// (Codex-Nachprüfung 2.1.8: der letzte Punkt auf der Bahn lag bei flachen
/// Rollwegen bis zu Hunderte Meter zu früh).
///
/// Ein Zug, der nur von aussen an die Kante heranführt und dort endet (OSM
/// endet oft an der Kante), zählt an seinem Endpunkt, wenn er von dort
/// nach aussen führt.
fn kantenpunkte(
    zug: &[(f64, f64)],
    halbe_breite: f64,
) -> Vec<(f64, f64, &'static str, Option<(f64, f64)>)> {
    let mut aus = Vec::new();
    let n = zug.len();
    for (seite, s) in [("left", -1.0_f64), ("right", 1.0_f64)] {
        // Aussen positiv, auf der Bahn negativ — bezogen auf DIESE Kante.
        let d = |i: usize| s * zug[i].1 - halbe_breite;
        // Reicht der Lauf ausserhalb ab Index `i` (in Richtung `schritt`)
        // weit genug hinaus, bevor er wieder auf die Bahn kommt?
        let reicht_hinaus = |i: usize, schritt: isize| {
            let mut j = i as isize;
            while j >= 0 && (j as usize) < n && d(j as usize) >= 0.0 {
                if d(j as usize) >= crate::grenzwerte::grenzwerte().ausfahrten.draussen_min_m.wert {
                    return true;
                }
                j += schritt;
            }
            false
        };
        for i in 0..n.saturating_sub(1) {
            let (innen, aussen, schritt) = match (d(i) < 0.0, d(i + 1) < 0.0) {
                (true, false) => (i, i + 1, 1isize),
                (false, true) => (i + 1, i, -1isize),
                _ => continue,
            };
            if !reicht_hinaus(aussen, schritt) {
                continue;
            }
            // Immer der Schnittpunkt des Segments mit der Kante (Codex
            // Runde 2: ein langes flaches Segment schneidet erst weit hinter
            // seinem letzten inneren Punkt).
            let di = d(innen);
            let (a, b) = (zug[innen], zug[aussen]);
            let t = di / (di - d(aussen));
            aus.push((a.0 + t * (b.0 - a.0), 0.0, seite, Some(a)));
        }
        // Endpunkte, die von aussen an die Kante heranführen: der Endpunkt
        // liegt der Kante am nächsten, der Zug reicht von dort hinaus.
        if n >= 2 {
            for (ende, nachbar, schritt) in [(0usize, 1usize, 1isize), (n - 1, n - 2, -1isize)] {
                let de = d(ende);
                if (0.0..=crate::grenzwerte::grenzwerte().ausfahrten.kanten_toleranz_m.wert).contains(&de) && de <= d(nachbar) && reicht_hinaus(ende, schritt) {
                    aus.push((zug[ende].0, de, seite, Some(zug[ende])));
                }
            }
        }
    }
    aus
}

// Wie weit zwei Ausfahrten derselben Seite auseinander liegen dürfen und
// noch dieselbe Abzweigung sind, in Metern: derselbe Stützpunkt (B und S1
// in EDDG liegen beide bei 1947,0 m). Getrennte Rollwege wenige Meter
// auseinander (Frankfurt R11/M19/R13) bleiben getrennt und werden in der
// Anzeige gezählt.
// Wert seit v3: bundle/grenzwerte/ausfahrten.json -> dieselbe_abzweigung_m (v2.1.24: 0.5)

/// Doppelnamen an derselben Abzweigung → EIN Name (Thomas, 22.10.2026).
///
/// Die Szenerie von EDDG führt „B“ und „S1“ mit denselben Stützpunkten;
/// gezeigt wurde „B/S1“, im Kopf aber „B“. Entscheidung: ein Name — der,
/// den die Navigationsdaten (die Bodenkarte des Servers, OSM) kennen
/// (`bekannt`), sonst der kürzere, bei Gleichstand der alphabetisch erste.
/// Dieselbe Regel rechnet die Anzeige für Flüge vor 2.1.8
/// (`doppelnamenZusammenfassen` in `bahnHilfen.ts`), dort ohne Karte.
pub fn doppelnamen_zusammenfassen(
    liste: Vec<Ausfahrt>,
    bekannt: impl Fn(&str) -> bool,
) -> Vec<Ausfahrt> {
    let rang = |a: &Ausfahrt| (!bekannt(&a.name), a.name.chars().count(), a.name.clone());
    let mut aus: Vec<Ausfahrt> = Vec::with_capacity(liste.len());
    for a in liste {
        match aus
            .iter_mut()
            .find(|b| {
                let gleicher_abzweig = match (a.abzweig, b.abzweig) {
                    (Some(p), Some(q)) => (p.0 - q.0).hypot(p.1 - q.1) <= crate::grenzwerte::grenzwerte().ausfahrten.dieselbe_abzweigung_m.wert,
                    _ => false,
                };
                b.seite == a.seite
                    && (gleicher_abzweig || (b.laengs_m - a.laengs_m).abs() <= crate::grenzwerte::grenzwerte().ausfahrten.dieselbe_abzweigung_m.wert)
            })
        {
            Some(b) => {
                if rang(&a) < rang(b) {
                    *b = a;
                }
            }
            None => aus.push(a),
        }
    }
    aus
}

/// Die genutzte Ausfahrt: auf der Räumseite, höchstens 120 m vom
/// Räumpunkt, die nächste (DLH369 EDDM: B6, nicht das 86 m entfernte B7).
/// Dieselbe Auswahl wie bis 2.1.7 in der Anzeige — jetzt EINMAL im Client,
/// als eigenes Feld (`clearance_exit`), damit Kopf, Bild, Lupe, PDF und
/// Webapp denselben Namen zeigen.
///
/// `lage` liest Seite und Längsposition — die Liste liegt im Client als
/// `Ausfahrt` und als `storage::RunwayExit` vor.
pub fn genutzte_ausfahrt<'a, T>(
    liste: &'a [T],
    raeum_m: f64,
    seite: &str,
    lage: impl Fn(&T) -> (&str, f64),
) -> Option<&'a T> {
    let abstand = |a: &T| (lage(a).1 - raeum_m).abs();
    liste
        .iter()
        .filter(|a| lage(a).0 == seite && abstand(a) < 120.0)
        .min_by(|a, b| abstand(a).partial_cmp(&abstand(b)).unwrap_or(std::cmp::Ordering::Equal))
}

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
    let mut beste: Vec<(String, String, f64, f64, Vec<(f64, f64)>, Option<(f64, f64)>)> = Vec::new();
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

        for (laengs, kantenabstand, seite, abzweig) in kantenpunkte(&projiziert, halbe_breite) {
            // Nur entlang der Bahn: Ein Abzweig hinter dem Bahnende oder
            // vor dem Bahnanfang ist keine Seitenausfahrt (Codex 22.10.2026).
            if laengs < 0.0 || laengs > bahnlaenge {
                continue;
            }
            match beste
                .iter_mut()
                .find(|(n, s, l, _, _, _)| {
                    n == name && s == seite && (l - laengs).abs() <= crate::grenzwerte::grenzwerte().ausfahrten.dieselbe_stelle_m.wert
                })
            {
                Some(eintrag) if kantenabstand < eintrag.3 => {
                    eintrag.2 = laengs;
                    eintrag.3 = kantenabstand;
                    eintrag.4 = projiziert.clone();
                    eintrag.5 = abzweig;
                }
                Some(_) => {}
                None => beste.push((
                    name.to_string(),
                    seite.to_string(),
                    laengs,
                    kantenabstand,
                    projiziert.clone(),
                    abzweig,
                )),
            }
        }
    }

    beste.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));
    beste
        .into_iter()
        .map(|(name, seite, laengs_m, _, roh, abzweig)| Ausfahrt {
            name,
            laengs_m: (laengs_m * 10.0).round() / 10.0,
            seite,
            verlauf: verlauf_ausduennen(&roh, laengs_m, halbe_breite),
            abzweig,
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
    let quer_max = halbe_breite_m + crate::grenzwerte::grenzwerte().ausfahrten.verlauf_quer_ueber_kante_m.wert;
    let fenster = (
        kante_laengs - crate::grenzwerte::grenzwerte().ausfahrten.verlauf_vor_m.wert,
        kante_laengs + crate::grenzwerte::grenzwerte().ausfahrten.verlauf_nach_m.wert,
        -quer_max,
        quer_max,
    );

    // Abschnitte zuschneiden, nicht Stuetzpunkte filtern.
    //
    // OpenStreetMap zeichnet einen Schnellabrollweg oft als EINE Gerade mit
    // zwei Stuetzpunkten — einer auf der Bahn, einer weit draussen am
    // parallelen Rollweg. Wer nur Punkte im Fenster behaelt, behaelt davon
    // einen, und „ein Punkt ist keine Linie" verwirft den Rest. So fehlte
    // der Verlauf bei zwei Dritteln aller Landungen (214 von 313 mit
    // Ausfahrten, 19.09.2026), darunter L6 in EDDL bei EWG9503.
    //
    // Zugeschnitten wird jeder Abschnitt am Fensterrand; die Schnittpunkte
    // kommen als echte Punkte dazu. Faellt ein Rollweg aus dem Fenster und
    // kommt wieder herein, entstehen zwei Stuecke — behalten wird das, das
    // der Kante am naechsten liegt, statt beide mit einer Geraden zu
    // verbinden, die es nicht gibt.
    let mut stuecke: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut offen = false;
    for w in roh.windows(2) {
        match abschnitt_zuschneiden(w[0], w[1], fenster) {
            Some((a, b)) => {
                if !offen {
                    stuecke.push(vec![a]);
                    offen = true;
                }
                let st = stuecke.last_mut().expect("eben angelegt");
                if st.last() != Some(&a) {
                    st.push(a);
                }
                st.push(b);
                // Endet der Abschnitt am Rand, ist das Stueck zu.
                if b != w[1] {
                    offen = false;
                }
            }
            None => offen = false,
        }
    }
    let kante = |p: &(f64, f64)| (p.0 - kante_laengs).hypot(p.1.abs() - halbe_breite_m);
    let Some(stueck) = stuecke.into_iter().min_by(|a, b| {
        let da = a.iter().map(kante).fold(f64::MAX, f64::min);
        let db = b.iter().map(kante).fold(f64::MAX, f64::min);
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        return Vec::new();
    };

    let mut aus: Vec<Verlaufspunkt> = Vec::new();
    let n = stueck.len();
    for (i, &(laengs, quer)) in stueck.iter().enumerate() {
        // Der letzte Punkt bleibt immer — sonst endet eine Gerade vor ihrem
        // Ende, und ein Rollweg aus zwei Stuetzpunkten wird kuerzer
        // gezeichnet, als er ist.
        if let Some(letzter) = aus.last() {
            let d = (laengs - letzter.laengs_m).hypot(quer - letzter.quer_m);
            if d < crate::grenzwerte::grenzwerte().ausfahrten.verlauf_min_abstand_m.wert && i + 1 < n {
                continue;
            }
            // Der letzte Punkt ersetzt einen zu nahen Vorgänger (ausser dem
            // ersten), statt zwei Punkte dicht nebeneinander zu lassen.
            if d < crate::grenzwerte::grenzwerte().ausfahrten.verlauf_min_abstand_m.wert && aus.len() > 1 {
                aus.pop();
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

/// Schneidet die Strecke `a`–`b` auf das Rechteck
/// `(laengs_min, laengs_max, quer_min, quer_max)` zu (Liang–Barsky).
/// `None`, wenn nichts davon im Rechteck liegt.
fn abschnitt_zuschneiden(
    a: (f64, f64),
    b: (f64, f64),
    (l0, l1, q0, q1): (f64, f64, f64, f64),
) -> Option<((f64, f64), (f64, f64))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0_f64, 1.0_f64);
    for (p, q) in [
        (-dx, a.0 - l0),
        (dx, l1 - a.0),
        (-dy, a.1 - q0),
        (dy, q1 - a.1),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    if t0 > t1 {
        return None;
    }
    let punkt = |t: f64| {
        if t == 0.0 {
            a
        } else if t == 1.0 {
            b
        } else {
            (a.0 + t * dx, a.1 + t * dy)
        }
    };
    Some((punkt(t0), punkt(t1)))
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

    /// Ein Rollweg, der die Bahn bei `laengs` an der Kante `quer` trifft und
    /// 80 m nach aussen führt.
    ///
    /// Bis 2.1.7 lag der zweite Punkt pauschal 0,002° nördlich und östlich —
    /// an EDDH 23 quer über die Bahn auf die andere Seite. Das zählte nicht,
    /// solange jeder Punkt nahe der Kante eine Ausfahrt war; seit nur noch
    /// Rollwege zählen, die die Bahn wirklich verlassen (#1936), muss die
    /// Testgeometrie das auch tun.
    fn rollweg(name: &str, laengs: f64, quer: f64) -> String {
        rollweg_aus(name, &[(laengs, quer), (laengs, quer + quer.signum() * 80.0)])
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
            assert!(d >= crate::grenzwerte::grenzwerte().ausfahrten.verlauf_min_abstand_m.wert - 0.5, "{d:.1} m Abstand");
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
    fn eine_lange_gerade_wird_zugeschnitten_statt_verworfen() {
        // EDDL L6 bei EWG9503: OSM fuehrt den Schnellabrollweg als eine
        // Gerade von der Bahn bis weit hinaus. Nur EIN Stuetzpunkt lag im
        // Fenster — der Verlauf wurde verworfen, die Lupe haette keinen
        // Rollweg gehabt.
        let g = karte(&rollweg_aus("L6", &[(1900.0, -10.0), (2700.0, -520.0)]));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1, "{a:?}");
        let v = &a[0].verlauf;
        assert!(v.len() >= 2, "Verlauf verworfen: {v:?}");
        // Er reicht bis an den Fensterrand, nicht nur bis zur Kante.
        let weitest = v.iter().map(|p| p.quer_m.abs()).fold(0.0, f64::max);
        assert!(weitest > 100.0, "endet bei {weitest:.0} m neben der Mitte");
        // Und der Winkel bleibt der der Geraden (atan(510/800) ≈ 32,5°).
        let (e, l) = (v.first().unwrap(), v.last().unwrap());
        let grad = (l.quer_m - e.quer_m)
            .abs()
            .atan2(l.laengs_m - e.laengs_m)
            .to_degrees();
        assert!((grad - 32.5).abs() < 2.0, "{grad:.1}°");
    }

    #[test]
    fn zugeschnitten_wird_am_fensterrand() {
        let f = (0.0, 100.0, -10.0, 10.0);
        assert_eq!(
            abschnitt_zuschneiden((-50.0, 0.0), (50.0, 0.0), f),
            Some(((0.0, 0.0), (50.0, 0.0)))
        );
        assert_eq!(abschnitt_zuschneiden((50.0, 20.0), (50.0, 30.0), f), None);
        let (a, b) = abschnitt_zuschneiden((50.0, 0.0), (50.0, 40.0), f).unwrap();
        assert_eq!(a, (50.0, 0.0));
        assert!((b.1 - 10.0).abs() < 1e-9);
    }

    #[test]
    fn ein_einzelner_punkt_ist_keine_linie() {
        // Bleibt nach dem Beschneiden nur ein Punkt uebrig, gibt es
        // nichts zu zeichnen — dann lieber gar keinen Verlauf als einen,
        // den die Anzeige zu einer Linie verlaengert.
        let g = karte(&rollweg("D8", 900.0, -23.0));
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
        let g = karte(&rollweg("D8", 900.0, -23.0));
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
    fn gleicher_name_an_zwei_stellen_derselben_seite_bleibt_zweimal() {
        // LFPG Z2, KMIA K2 (OSM, 28.10.2026): zwei getrennte Stücke gleichen
        // Namens verlassen dieselbe Seite 300 m und mehr auseinander.
        let g = karte(&format!(
            "{},{}",
            rollweg("K2", 900.0, 23.0),
            rollweg("K2", 1300.0, 23.0)
        ));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 2, "{a:?}");
        assert!(a.iter().all(|x| x.name == "K2" && x.seite == "right"), "{a:?}");
        assert!((a[1].laengs_m - a[0].laengs_m - 400.0).abs() < 30.0, "{a:?}");
    }

    #[test]
    fn beide_seiten_werden_unterschieden() {
        let g = karte(&format!(
            "{},{}",
            rollweg("D8", 900.0, -23.0),
            rollweg("D7", 1200.0, 23.0)
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

    // ── #1936 (EDDG 25, MSFS-Szenerie, 22.10.2026) ──────────────────
    //
    // Die Szenerie liefert zwei Rollwegzüge „B“ und „S1“ mit denselben
    // Stützpunkten: Sie beginnen auf der Mittellinie bei 1839,6 m (knapp
    // RECHTS davon), laufen auf der Bahn bis 1947 m und verlassen sie dort
    // nach links. Die OSM-Karte von EDDG zeigt dasselbe für B (Mittellinie
    // ab 1838 m, links hinaus bei 1943 m) und kennt kein S1.
    //
    // Gespeichert waren „B 1839,6 rechts“ und „S1 1839,6 rechts“: Der
    // Mittellinienpunkt lag 22,6 m von der rechten Kante — innerhalb der
    // 25-m-Toleranz — und galt als Ausfahrt nach rechts. Thomas: „gleiche
    // Ausfahrtsnamen auf beiden Seiten?“. Ein Rollwegstück auf der Bahn ist
    // keine Ausfahrt; nur wo der Rollweg die Bahn wirklich verlässt.
    // Bahn hier: EDDH-Geometrie mit 46 m (EDDG: 45,1 m) — die Form zählt.

    /// Wie die Szenerie B und S1 an EDDG 25 liefert (Bahn-Koordinaten).
    fn eddg_szenerie() -> String {
        karte(&format!(
            "{},{}",
            rollweg_aus(
                "B",
                &[
                    (1839.6, 0.3),
                    (1904.0, -1.0),
                    (1926.0, -8.0),
                    (1941.0, -21.0),
                    (1947.0, -22.0),
                    (1950.0, -28.0),
                    (1952.0, -56.0),
                ],
            ),
            rollweg_aus(
                "S1",
                &[(1839.6, 0.3), (1904.0, -1.0), (1947.0, -22.0), (1897.0, -37.0)],
            ),
        ))
    }

    #[test]
    fn eddg_ein_rollwegstueck_auf_der_bahn_ist_keine_ausfahrt() {
        let a = ausfahrten_fuer_bahn(&eddg_szenerie(), T.0, T.1, T.2, T.3, 46.0);
        assert!(
            a.iter().all(|x| x.seite != "right"),
            "Phantom-Ausfahrt rechts — B und S1 verlassen die Bahn dort nicht: {a:?}"
        );
        // Die echte Ausfahrt bleibt: links bei rund 1947 m.
        assert!(
            a.iter()
                .any(|x| x.name == "B" && x.seite == "left" && (x.laengs_m - 1947.0).abs() < 8.0),
            "{a:?}"
        );
    }

    fn lage(a: &Ausfahrt) -> (&str, f64) {
        (a.seite.as_str(), a.laengs_m)
    }

    #[test]
    fn codex_entlang_der_kante_zaehlt_erst_der_uebergang() {
        // Codex 22.10.2026 (Befund 2): Ein Rollweg läuft 300 m knapp innen
        // an der Kante entlang und verlässt die Bahn erst bei 1300 m. Die
        // Ausfahrt ist der Übergang, nicht der erste kantennahe Punkt.
        let g = karte(&rollweg_aus(
            "K",
            &[(1000.0, -20.0), (1100.0, -20.5), (1200.0, -20.0), (1300.0, -20.0), (1310.0, -40.0), (1320.0, -90.0)],
        ));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!((a[0].laengs_m - 1300.0).abs() < 15.0, "Ausfahrt bei {} statt ~1300 m", a[0].laengs_m);
    }

    #[test]
    fn codex_flaches_segment_schnitt_mit_der_kante() {
        // Codex Runde 2: Der letzte Punkt auf der Bahn liegt 3 m innen bei
        // 1000 m, das Segment bis 2000 m schneidet die Kante erst bei
        // ~1600 m. Die Ausfahrt ist der Schnittpunkt, nicht 1000 m.
        let g = karte(&rollweg_aus("F", &[(1000.0, -20.0), (2000.0, -25.0), (2010.0, -70.0)]));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1, "{a:?}");
        assert!((a[0].laengs_m - 1600.0).abs() < 10.0, "Ausfahrt bei {} statt ~1600 m", a[0].laengs_m);
    }

    #[test]
    fn codex_abzweig_hinter_dem_bahnende_ist_keine_seitenausfahrt() {
        // Codex 22.10.2026 (Befund 3): Ein Rollweg läuft auf der Achse über
        // das Bahnende hinaus und biegt 100 m dahinter ab — keine linke
        // Ausfahrt der Bahn. Am Bahnanfang gilt dasselbe (100 m davor).
        let l = ::geo::distance_m(T.0, T.1, T.2, T.3);
        let hinten = karte(&rollweg_aus("E", &[(l - 100.0, 0.0), (l + 100.0, 0.0), (l + 100.0, -90.0)]));
        let vorn = karte(&rollweg_aus("V", &[(100.0, 0.0), (-100.0, 0.0), (-100.0, 90.0)]));
        for g in [hinten, vorn] {
            let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
            assert!(a.is_empty(), "{a:?}");
        }
    }

    #[test]
    fn eddg_doppelname_an_derselben_abzweigung_wird_ein_name() {
        let a = ausfahrten_fuer_bahn(&eddg_szenerie(), T.0, T.1, T.2, T.3, 46.0);
        // Ohne Karte: der kürzere — „B“, nicht „B/S1“ und nicht „S1“.
        let eins = doppelnamen_zusammenfassen(a.clone(), |_| false);
        let links: Vec<&str> = eins
            .iter()
            .filter(|x| x.seite == "left")
            .map(|x| x.name.as_str())
            .collect();
        assert_eq!(links, ["B"], "{eins:?}");
        // Kennt die Bodenkarte nur S1, gewinnt S1 (Karte vor „kürzer“).
        let karte = doppelnamen_zusammenfassen(a, |n| n == "S1");
        assert!(karte.iter().any(|x| x.name == "S1"), "{karte:?}");
        assert!(karte.iter().all(|x| x.name != "B"), "{karte:?}");
        // Die genutzte Ausfahrt am Räumpunkt 1946,4 m links: B.
        assert_eq!(
            genutzte_ausfahrt(&eins, 1946.4, "left", lage).map(|x| x.name.as_str()),
            Some("B")
        );
        assert!(genutzte_ausfahrt(&eins, 1946.4, "right", lage).is_none());
    }

    #[test]
    fn verschiedene_abzweigungen_bleiben_getrennt() {
        // Zwei Rollwege 30 m auseinander sind zwei Ausfahrten, keine
        // Doppelnamen (Frankfurt M17/M19 liegen dicht, aber getrennt).
        let a = ausfahrten_fuer_bahn(
            &karte(&format!("{},{}", rollweg("M17", 1500.0, -23.0), rollweg("M19", 1530.0, -23.0))),
            T.0,
            T.1,
            T.2,
            T.3,
            46.0,
        );
        assert_eq!(doppelnamen_zusammenfassen(a, |_| false).len(), 2);
    }

    #[test]
    fn schnellabrollweg_von_der_mittellinie_zaehlt_an_der_kante() {
        // OSM: eine Gerade von der Mittellinie weit hinaus. Die Stelle ist
        // der Schnittpunkt mit der Kante, die Seite die, auf der er die
        // Bahn verlässt — nicht das Vorzeichen des Mittellinienpunkts.
        let g = karte(&rollweg_aus("S4", &[(1000.0, -0.2), (1200.0, 120.0)]));
        let a = ausfahrten_fuer_bahn(&g, T.0, T.1, T.2, T.3, 46.0);
        assert_eq!(a.len(), 1, "{a:?}");
        assert_eq!(a[0].seite, "right");
        assert!((a[0].laengs_m - 1038.6).abs() < 3.0, "{}", a[0].laengs_m);
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
