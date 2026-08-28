//! Bahnen und Rollwege aus der **MSFS-Szenerie**, über SimConnect.
//!
//! # Warum
//!
//! Der Pilot fliegt, was installiert ist. Unsere Navigationsdaten
//! beschreiben den echten Flughafen; am 28.08.2026 gegen die installierte
//! X-Plane-Szenerie gehalten, führen **3.836 Bahnen** des neuesten
//! AIRAC-Zyklus `true_course` als 0,0 oder 360,0 — bei 3.329 davon
//! widerspricht das der eigenen Bahnnummer. Dort messen wir gegen eine
//! Achse, die es im Simulator nicht gibt.
//!
//! Für X-Plane wird die installierte `apt.dat` gelesen. MSFS hat keine
//! solche Datei; dort liefert die **Facility-Schnittstelle** dasselbe —
//! aus der geladenen Szenerie, Add-ons eingeschlossen, weil sie im
//! Simulator registriert sind.
//!
//! # Wie die Schnittstelle arbeitet
//!
//! Erst wird eine Definition **aus Feldnamen** zusammengesetzt:
//!
//! ```text
//! AddToFacilityDefinition(def, "OPEN AIRPORT")
//! AddToFacilityDefinition(def, "OPEN RUNWAY")
//! AddToFacilityDefinition(def, "Latitude")
//! …
//! AddToFacilityDefinition(def, "CLOSE RUNWAY")
//! AddToFacilityDefinition(def, "CLOSE AIRPORT")
//! ```
//!
//! Danach `RequestFacilityData(def, req, "EDDH")`. Die Antworten kommen
//! **asynchron** als `SIMCONNECT_RECV_FACILITY_DATA`, je eine je Element,
//! mit `Type` (Flughafen / Bahn / Rollwegpunkt …) und einem Datenblock,
//! dessen Aufbau der Definition folgt. `..._DATA_END` schliesst ab.
//!
//! # ⚠ Die Feldnamen stehen NICHT in der Kopfdatei
//!
//! Sie sind Zeichenketten aus der SDK-Dokumentation. Ein falscher Name
//! wird von SimConnect mit einer Ausnahme quittiert — und genau deshalb
//! wird hier **jede** Ausnahme dem Feld zugeordnet, das sie ausgelöst
//! hat (über `GetLastSentPacketID`, wie bei den Inspector-Watches).
//!
//! Ein stiller Fehlschlag wäre hier besonders teuer: Die Bahn käme dann
//! ohne Breite zurück, und die Breite ist genau das Mass, mit dem
//! entschieden wird, ob eine Rollspur die befestigte Fläche verlässt.

use sim_core::szenerie::{SzenerieBahn, SzenerieFlughafen};

/// Die Felder einer Bahn, in der Reihenfolge, in der sie im Datenblock
/// ankommen.
///
/// ⚠ **Reihenfolge = Speicherlayout.** Wer hier etwas einfügt,
/// verschiebt alles danach; der Parser liest nach Position, nicht nach
/// Namen.
///
/// ⚠ **Die Schreibweise ist GROSS_MIT_UNTERSTRICH.** Sie steht nicht in
/// `SimConnect.h`, sondern in der SDK-Dokumentation
/// (`SimConnect_AddToFacilityDefinition`). Ich hatte sie zuerst als
/// `Latitude`/`Heading`/`PrimaryNumber` geraten — jeder dieser Namen
/// wäre von SimConnect abgelehnt worden, und zwar erst zur Laufzeit auf
/// einer Windows-Maschine.
///
/// `PRIMARY_THRESHOLD` / `SECONDARY_THRESHOLD` sind die versetzte
/// Schwelle des jeweiligen Endes. Die Dokumentation nennt sie nicht
/// ausdrücklich so; der Wert wird deshalb beim ersten echten Lauf gegen
/// die Navdaten gehalten, bevor er in die Bewertung geht.
pub const BAHN_FELDER: &[(&str, FeldTyp)] = &[
    ("LATITUDE", FeldTyp::F64),
    ("LONGITUDE", FeldTyp::F64),
    ("ALTITUDE", FeldTyp::F64),
    ("HEADING", FeldTyp::F32),
    ("LENGTH", FeldTyp::F32),
    ("WIDTH", FeldTyp::F32),
    ("SURFACE", FeldTyp::I32),
    ("PRIMARY_NUMBER", FeldTyp::I32),
    ("PRIMARY_DESIGNATOR", FeldTyp::I32),
    ("SECONDARY_NUMBER", FeldTyp::I32),
    ("SECONDARY_DESIGNATOR", FeldTyp::I32),
    ("PRIMARY_THRESHOLD", FeldTyp::F32),
    ("SECONDARY_THRESHOLD", FeldTyp::F32),
];

/// Die Felder der Rollwege — drei Listen, die zusammengehören.
///
/// MSFS beschreibt sie wie X-Planes `1201`/`1202`: Punkte, Kanten mit
/// Verweisen auf die Punkte, und eine Namensliste.
///
/// ⚠ **`TAXI_POINT` führt KEINE Koordinaten.** Es gibt nur `BIAS_X` und
/// `BIAS_Z` — Meter nach Osten und Norden, bezogen auf den
/// Referenzpunkt des Flughafens. Wer sie für Länge und Breite hält,
/// legt jeden Rollweg irgendwo in den Golf von Guinea. Die Umrechnung
/// braucht also zuerst `AIRPORT`.
pub const ROLLWEG_PUNKT_FELDER: &[(&str, FeldTyp)] = &[
    ("TYPE", FeldTyp::I32),
    ("BIAS_X", FeldTyp::F32),
    ("BIAS_Z", FeldTyp::F32),
];

/// Kanten: `START`/`END` verweisen auf Punkte, `NAME_INDEX` auf die
/// Namensliste.
pub const ROLLWEG_KANTE_FELDER: &[(&str, FeldTyp)] = &[
    ("TYPE", FeldTyp::I32),
    ("WIDTH", FeldTyp::F32),
    ("START", FeldTyp::I32),
    ("END", FeldTyp::I32),
    ("NAME_INDEX", FeldTyp::I32),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeldTyp {
    F64,
    F32,
    I32,
}

impl FeldTyp {
    pub const fn groesse(self) -> usize {
        match self {
            FeldTyp::F64 => 8,
            FeldTyp::F32 => 4,
            FeldTyp::I32 => 4,
        }
    }
}

/// Der Bezeichner eines Bahnendes aus Nummer und Kennbuchstabe.
///
/// MSFS liefert beides als Zahl: die Nummer (1–36) und einen Schlüssel
/// für L/C/R. Die Zuordnung folgt der SDK-Aufzählung
/// `RUNWAY_DESIGNATOR`.
pub fn bezeichner(nummer: i32, kennung: i32) -> String {
    let buchstabe = match kennung {
        1 => "L",
        2 => "R",
        3 => "C",
        4 => "W", // Wasser
        5 => "A",
        6 => "B",
        _ => "",
    };
    format!("{nummer:02}{buchstabe}")
}

/// Belagsschlüssel von MSFS auf die Schreibweise der `apt.dat`.
///
/// Beide Adapter liefern dieselbe Aufzählung, damit die Auswertung eine
/// Sprache spricht. Unbekanntes wird zu 0 — „nicht zuzuordnen" ist eine
/// Aussage, ein geratener Asphalt wäre keine.
pub fn belag_code(msfs: i32) -> u8 {
    match msfs {
        0 | 1 | 2 => 1, // Beton/Asphalt/Bitumen -> befestigt
        3 | 4 => 3,     // Gras, Erde
        5 | 6 => 5,     // Kies, Schotter
        7 => 14,        // Wasser
        8 => 15,        // Schnee/Eis
        _ => 0,
    }
}

/// Einen Datenblock in ein Bahn-Paar umrechnen.
///
/// MSFS beschreibt die Bahn als **eine** Einheit mit Mittelpunkt, Kurs,
/// Länge und beiden Enden. Wir brauchen beide Enden einzeln, wie die
/// `apt.dat` sie liefert — also werden die Schwellen aus Mittelpunkt,
/// Kurs und halber Länge gerechnet.
///
/// ⚠ `Latitude`/`Longitude` sind die MITTE der Bahn, nicht eine
/// Schwelle. Wer das verwechselt, legt beide Enden auf denselben Punkt
/// und der Abnehmer verwirft die Bahn als „liegt woanders".
pub fn bahn_paar(
    mitte: (f64, f64),
    kurs_grad: f64,
    laenge_m: f64,
    breite_m: f64,
    belag: u8,
    prim: (i32, i32, f64),
    sek: (i32, i32, f64),
) -> [SzenerieBahn; 2] {
    let halb = laenge_m / 2.0;
    let prim_schwelle = versetze(mitte, (kurs_grad + 180.0) % 360.0, halb);
    let sek_schwelle = versetze(mitte, kurs_grad, halb);
    [
        SzenerieBahn {
            bezeichner: bezeichner(prim.0, prim.1),
            kurs_grad,
            breite_m,
            laenge_m,
            versetzte_schwelle_m: prim.2,
            schwelle: prim_schwelle,
            gegenende: sek_schwelle,
            belag_code: belag,
        },
        SzenerieBahn {
            bezeichner: bezeichner(sek.0, sek.1),
            kurs_grad: (kurs_grad + 180.0) % 360.0,
            breite_m,
            laenge_m,
            versetzte_schwelle_m: sek.2,
            schwelle: sek_schwelle,
            gegenende: prim_schwelle,
            belag_code: belag,
        },
    ]
}

/// Einen Punkt um `strecke_m` in Richtung `kurs_grad` verschieben.
fn versetze(punkt: (f64, f64), kurs_grad: f64, strecke_m: f64) -> (f64, f64) {
    const R: f64 = 6_371_000.0;
    let (lat, lon) = (punkt.0.to_radians(), punkt.1.to_radians());
    let k = kurs_grad.to_radians();
    let d = strecke_m / R;
    let lat2 = (lat.sin() * d.cos() + lat.cos() * d.sin() * k.cos()).asin();
    let lon2 = lon + (k.sin() * d.sin() * lat.cos()).atan2(d.cos() - lat.sin() * lat2.sin());
    (lat2.to_degrees(), lon2.to_degrees())
}

/// Einen Rollwegpunkt aus `BIAS_X`/`BIAS_Z` in Koordinaten umrechnen.
///
/// ⚠ MSFS gibt Rollwegpunkte NICHT als Länge und Breite, sondern als
/// Versatz in Metern gegen den Referenzpunkt des Flughafens:
/// `BIAS_X` nach Osten, `BIAS_Z` nach Norden.
///
/// Wer die Werte für Koordinaten hält, legt jeden Rollweg irgendwo in
/// den Golf von Guinea — plausible Zahlen, völlig falscher Ort. Genau
/// dieselbe Klasse wie das vertauschte Koordinatenpaar heute Nachmittag,
/// nur unauffälliger.
pub fn punkt_aus_versatz(referenz: (f64, f64), bias_ost_m: f64, bias_nord_m: f64) -> (f64, f64) {
    const R: f64 = 6_371_000.0;
    let lat = referenz.0 + (bias_nord_m / R).to_degrees();
    // Ein Längengrad wird zu den Polen hin kürzer.
    let lon =
        referenz.1 + (bias_ost_m / (R * referenz.0.to_radians().cos().max(1e-9))).to_degrees();
    (lat, lon)
}

/// Sammelt die asynchron eintreffenden Elemente zu einem Flughafen.
#[derive(Debug, Default)]
pub struct Sammler {
    pub icao: String,
    pub bahnen: Vec<SzenerieBahn>,
    /// Feldnamen, die SimConnect abgelehnt hat. Leer ist der Normalfall;
    /// nicht leer heisst, dass die Definition angepasst werden muss.
    pub abgelehnte_felder: Vec<String>,
}

impl Sammler {
    pub fn neu(icao: &str) -> Sammler {
        Sammler {
            icao: icao.to_string(),
            ..Default::default()
        }
    }

    pub fn fertig(self) -> SzenerieFlughafen {
        SzenerieFlughafen {
            icao: self.icao,
            bahnen: self.bahnen,
            rollwege: Vec::new(),
            quelle: "msfs".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bezeichner_aus_nummer_und_kennung() {
        assert_eq!(bezeichner(9, 0), "09");
        assert_eq!(bezeichner(9, 1), "09L");
        assert_eq!(bezeichner(27, 2), "27R");
        assert_eq!(bezeichner(4, 3), "04C");
        // Zweistellig ohne fuehrende Null waere ein anderer Bezeichner
        // als in den Navdaten — dann fiele der Vergleich auseinander.
        assert_eq!(bezeichner(1, 0), "01");
    }

    #[test]
    fn belag_wird_auf_eine_sprache_gebracht() {
        assert_eq!(belag_code(0), 1, "Beton -> befestigt");
        assert_eq!(belag_code(2), 1, "Bitumen -> befestigt");
        assert_eq!(belag_code(3), 3, "Gras");
        assert_eq!(belag_code(7), 14, "Wasser");
        // ⚠ Unbekanntes wird 0 (= nicht zuzuordnen), NICHT Asphalt.
        // Ein geratener Belag waere eine Aussage, die wir nicht haben —
        // und die seitliche Bewertung haengt daran.
        assert_eq!(belag_code(99), 0);
    }

    fn abstand_m(a: (f64, f64), b: (f64, f64)) -> f64 {
        const R: f64 = 6_371_000.0;
        let (p1, p2) = (a.0.to_radians(), b.0.to_radians());
        let dp = p2 - p1;
        let dl = (b.1 - a.1).to_radians();
        let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
        2.0 * R * h.sqrt().asin()
    }

    #[test]
    fn schwellen_werden_aus_der_mitte_gerechnet() {
        // ⚠ MSFS gibt die MITTE der Bahn, nicht eine Schwelle. Wer das
        // verwechselt, legt beide Enden auf denselben Punkt — und der
        // Abnehmer verwirft die Bahn dann als „liegt woanders", genau
        // wie beim vertauschten Koordinatenpaar am selben Tag.
        let mitte = (53.6304, 9.9882); // EDDH, ungefaehr
        let [a, b] = bahn_paar(mitte, 90.0, 3000.0, 46.0, 1, (9, 0, 0.0), (27, 0, 0.0));
        assert_eq!(a.bezeichner, "09");
        assert_eq!(b.bezeichner, "27");
        // Die beiden Schwellen muessen die Bahnlaenge auseinanderliegen.
        let d = abstand_m(a.schwelle, b.schwelle);
        assert!((d - 3000.0).abs() < 5.0, "Schwellenabstand {d:.1} m");
        // Und die Mitte muss dazwischen liegen.
        let d1 = abstand_m(mitte, a.schwelle);
        let d2 = abstand_m(mitte, b.schwelle);
        assert!((d1 - 1500.0).abs() < 5.0 && (d2 - 1500.0).abs() < 5.0);
        // Gegenrichtung stimmt.
        assert!((b.kurs_grad - 270.0).abs() < 1e-9);
        assert_eq!(a.gegenende, b.schwelle);
    }

    #[test]
    fn die_rechnung_reproduziert_eine_echte_bahn() {
        // ⚠ Der eigentliche Test, und er braucht keine Windows-Maschine.
        //
        // MSFS und X-Plane beschreiben denselben echten Flughafen. Wenn
        // die Umrechnung „Mitte + Kurs + Laenge -> zwei Schwellen"
        // stimmt, muss sie die Schwellen reproduzieren, die in der
        // installierten X-Plane-Szenerie stehen — die haben wir hier.
        //
        // Damit ist die einzige nicht-triviale Rechnung des
        // MSFS-Adapters geprueft, lange bevor ein Pilot fliegt.
        let Some(_) = sim_xplane_pfad() else {
            eprintln!("uebersprungen: keine X-Plane-Installation");
            return;
        };
        for icao in ["EDDH", "EDDV", "KJFK"] {
            let Some(f) = sim_xplane::szenerie::flughafen(icao) else {
                continue;
            };
            // Bahnen paarweise: jedes Ende kennt sein Gegenende.
            for bahn in f.bahnen.iter() {
                let mitte = (
                    (bahn.schwelle.0 + bahn.gegenende.0) / 2.0,
                    (bahn.schwelle.1 + bahn.gegenende.1) / 2.0,
                );
                let [a, _] = bahn_paar(
                    mitte,
                    bahn.kurs_grad,
                    bahn.laenge_m,
                    bahn.breite_m,
                    bahn.belag_code,
                    (0, 0, 0.0),
                    (0, 0, 0.0),
                );
                let fehler = abstand_m(a.schwelle, bahn.schwelle);
                assert!(
                    fehler < 5.0,
                    "{icao} {}: rekonstruierte Schwelle liegt {fehler:.1} m daneben",
                    bahn.bezeichner
                );
            }
        }
    }

    fn sim_xplane_pfad() -> Option<std::path::PathBuf> {
        sim_xplane::szenerie::installationen().into_iter().next()
    }
}

#[cfg(test)]
mod feldnamen_tests {
    use super::*;

    /// ⚠ Die Wache gegen den Fehler, den ich selbst gemacht habe.
    ///
    /// Ich hatte die Feldnamen als `Latitude`/`Heading`/`PrimaryNumber`
    /// geraten. Richtig ist `LATITUDE`/`HEADING`/`PRIMARY_NUMBER` —
    /// GROSS mit Unterstrich, laut SDK-Dokumentation. Jeder geratene
    /// Name wäre von SimConnect abgelehnt worden, und zwar erst zur
    /// Laufzeit auf einer Windows-Maschine, die ich hier nicht habe.
    ///
    /// Diese Prüfung kostet nichts und fängt jede kuenftige Vermutung.
    #[test]
    fn feldnamen_sind_gross_mit_unterstrich() {
        for liste in [BAHN_FELDER, ROLLWEG_PUNKT_FELDER, ROLLWEG_KANTE_FELDER] {
            for (name, _) in liste {
                assert!(
                    !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                        && name.chars().next().is_some_and(|c| c.is_ascii_uppercase()),
                    "Feldname {name:?} ist nicht GROSS_MIT_UNTERSTRICH — \
                     SimConnect lehnt ihn ab, und zwar erst zur Laufzeit"
                );
            }
        }
    }

    #[test]
    fn die_bahn_traegt_die_felder_die_die_bewertung_braucht() {
        // Ohne WIDTH fehlt genau das Mass, mit dem entschieden wird, ob
        // eine Rollspur die befestigte Flaeche verlaesst. Ohne HEADING
        // gibt es keine Achse.
        let namen: Vec<&str> = BAHN_FELDER.iter().map(|(n, _)| *n).collect();
        for pflicht in [
            "LATITUDE",
            "LONGITUDE",
            "HEADING",
            "LENGTH",
            "WIDTH",
            "SURFACE",
            "PRIMARY_NUMBER",
            "SECONDARY_NUMBER",
        ] {
            assert!(namen.contains(&pflicht), "Feld {pflicht} fehlt");
        }
    }

    #[test]
    fn versatz_wird_zu_koordinaten() {
        // EDDH, Referenzpunkt ungefaehr.
        let referenz = (53.6304, 9.9882);
        let p = punkt_aus_versatz(referenz, 1000.0, 0.0);
        assert!(
            (p.0 - referenz.0).abs() < 1e-9,
            "Nordwert darf sich nicht aendern"
        );
        assert!(p.1 > referenz.1, "1000 m Ost muessen die Laenge erhoehen");
        // Auf 53,6 Grad Breite sind 1000 m Ost rund 0,0151 Grad.
        assert!(
            ((p.1 - referenz.1) - 0.0151).abs() < 0.001,
            "Laengenzuwachs {:.5}",
            p.1 - referenz.1
        );
        let q = punkt_aus_versatz(referenz, 0.0, 1000.0);
        assert!(
            (q.1 - referenz.1).abs() < 1e-9,
            "Ostwert darf sich nicht aendern"
        );
        assert!(
            ((q.0 - referenz.0) - 0.00899).abs() < 0.0005,
            "Breitenzuwachs {:.5}",
            q.0 - referenz.0
        );
    }
}
