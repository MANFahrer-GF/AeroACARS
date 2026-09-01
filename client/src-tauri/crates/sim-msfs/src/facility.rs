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
/// ⚠ Hier stehen NUR die Felder des flachen Bahnsatzes. Die versetzte
/// Schwelle gehoert NICHT dazu — sie kommt als eigener PAVEMENT-Satz,
/// siehe [`PAVEMENT_FELDER`].
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
];

/// Die drei Felder eines PAVEMENT-Untersatzes.
///
/// # Warum das eine EIGENE Liste ist
///
/// `PAVEMENT` ist eine eigene Satzart mit eigenem Wert in
/// `SIMCONNECT_FACILITY_DATA_TYPE` (`SIMCONNECT_FACILITY_DATA_PAVEMENT`,
/// im SDK-Header Zeile 338). Die Felder kommen also in einer EIGENEN
/// Nachricht nach ihrem Bahnsatz — nicht eingebettet in ihm.
///
/// ⚠ Genau hier lag der zweite Fehlversuch (30.08.2026): Ich hatte den
/// Enum-Block nur bis Zeile 335 gelesen, das Fehlen von PAVEMENT als
/// Tatsache gemeldet und die sechs Felder ins flache Bahnraster
/// gehaengt. Folge waere gewesen: Der Bahnsatz kommt mit 56 Bytes, das
/// Raster verlangt 80 — jede MSFS-Bahn faellt durch, genau wie bei
/// v1.7.8. Die Tests bauten ihren 80-Byte-Block selbst und haben die
/// falsche Annahme nur bestaetigt.
///
/// Laut SDK-Doku: LENGTH (FLOAT32), WIDTH (FLOAT32), ENABLE (INT32).
pub const PAVEMENT_FELDER: &[(&str, FeldTyp)] = &[
    ("LENGTH", FeldTyp::F32),
    ("WIDTH", FeldTyp::F32),
    ("ENABLE", FeldTyp::I32),
];

/// Die Anmeldung des Bahnsatzes — Feldnamen UND Gruppenmarken, in der
/// Reihenfolge, in der sie an `AddToFacilityDefinition` gehen.
///
/// ⚠ Getrennt von `BAHN_FELDER`, weil OPEN/CLOSE nur die Definition
/// betreffen und KEINE Bytes liefern. Wer eines von beiden aendert, muss
/// das andere mit aendern — der Test
/// `definition_und_raster_passen_zusammen` haelt das fest.
pub const BAHN_DEFINITION: &[&str] = &[
    "LATITUDE",
    "LONGITUDE",
    "ALTITUDE",
    "HEADING",
    "LENGTH",
    "WIDTH",
    "SURFACE",
    "PRIMARY_NUMBER",
    "PRIMARY_DESIGNATOR",
    "SECONDARY_NUMBER",
    "SECONDARY_DESIGNATOR",
    "OPEN PRIMARY_THRESHOLD",
    "LENGTH",
    "WIDTH",
    "ENABLE",
    "CLOSE PRIMARY_THRESHOLD",
    "OPEN SECONDARY_THRESHOLD",
    "LENGTH",
    "WIDTH",
    "ENABLE",
    "CLOSE SECONDARY_THRESHOLD",
];

/// Der Referenzpunkt des Flughafens.
///
/// ⚠ Pflicht, sobald Rollwege angefordert werden: `TAXI_POINT` liefert
/// nur einen Versatz in Metern gegen diesen Punkt, keine Koordinaten.
pub const FLUGHAFEN_FELDER: &[(&str, FeldTyp)] = &[
    ("LATITUDE", FeldTyp::F64),
    ("LONGITUDE", FeldTyp::F64),
    ("ALTITUDE", FeldTyp::F64),
];

/// Die Namensliste. `TAXI_PATH::NAME_INDEX` zeigt hierhin.
///
/// ⚠ `NAME` ist eine Zeichenkette und liegt NICHT im festen Raster der
/// übrigen Felder — sie wird deshalb getrennt gelesen, siehe
/// `name_aus_bytes`.
pub const ROLLWEG_NAME_FELDER: &[(&str, FeldTyp)] = &[("NAME", FeldTyp::Text)];

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
    // ⚠ Laut SDK-Doku ein **UINT32**. Als `I32` gelesen ist das
    // unbedenklich — gleiche Groesse, und ein Index in eine Namensliste
    // wird nie ueber zwei Milliarden gross. Der Zusammenbau prueft
    // ohnehin auf `>= 0` und verwirft alles andere.
    ("NAME_INDEX", FeldTyp::I32),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeldTyp {
    F64,
    F32,
    I32,
    /// Eine Zeichenkette. Sie hat keine feste Groesse und darf deshalb
    /// nur als LETZTES Feld einer Liste stehen — sonst laesst sich
    /// nicht sagen, wo das naechste beginnt.
    Text,
}

impl FeldTyp {
    pub const fn groesse(self) -> usize {
        match self {
            FeldTyp::F64 => 8,
            FeldTyp::F32 => 4,
            FeldTyp::I32 => 4,
            // Nicht sinnvoll bestimmbar — siehe `Text`.
            FeldTyp::Text => 0,
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
///
/// # ⚠ Die beiden Simulatoren zählen VERSCHIEDEN
///
/// Bis v1.7.12 stand hier die Aufzählung der `apt.dat` — also
/// X-Plane-Bedeutungen, angewandt auf MSFS-Zahlen. Das war nicht ein
/// verschobener Wert, sondern eine andere Tabelle:
///
///   Zahl   MSFS            hier gelesen als    Folge
///   1      GRASS           Asphalt             Gras galt als befestigt
///   2      WATER FSX       Bitumen             Wasser galt als befestigt
///   4      ASPHALT         Erde                **Asphalt verlor die
///                                              seitliche Bewertung**
///   7      HARD TURF       Wasser              Rasen galt als Wasser
///
/// Getroffen hat es nur Bahnen ohne Belag in den Navdaten (seit v1.7.12
/// füllt die Szenerie dort nur Lücken) — dort aber voll.
///
/// # Die maßgebliche Tabelle
///
/// Aus der SimConnect-Doku (`SimConnect_AddToFacilityDefinition`, Feld
/// `SURFACE`). Die Bahn-Liste fehlt dort im Fließtext; die vollständige
/// Aufzählung steht beim gleichnamigen Feld des HELIPAD-Eintrags:
///
///   0 CONCRETE · 1 GRASS · 2 WATER FSX · 3 GRASS BUMPY · 4 ASPHALT
///   5 SHORT GRASS · 6 LONG GRASS · 7 HARD TURF · 8 SNOW · 9 ICE
///   10 URBAN · 11 FOREST · 12 DIRT · 13 CORAL · 14 GRAVEL
///   15 OIL TREATED · 16 STEEL MATS · 17 BITUMINUS · 18 BRICK
///   19 MACADAM · 20 PLANKS · 21 SAND · 22 SHALE · 23 TARMAC
///   24 WRIGHT FLYER TRACK · 26 OCEAN · 27 WATER · 28 POND · 29 LAKE
///   30 RIVER · 31 WASTE WATER · 32 PAINT · 254 UNKNOWN · 255 UNDEFINED
///
/// ⚠ 25 fehlt in der Doku. Es wird deshalb wie alles Unbekannte
/// behandelt und NICHT geraten.
///
/// Was hier bewusst zu 0 wird, obwohl es einen Namen hat: URBAN,
/// FOREST, PLANKS, STEEL MATS, PAINT, WRIGHT FLYER TRACK. Für sie gibt
/// es in der `apt.dat` keine Entsprechung, und ein hingebogener Wert
/// wäre schlimmer als keiner — die seitliche Bewertung hängt daran, ob
/// eine Kante überhaupt eine belastbare Grenze ist.
pub fn belag_code(msfs: i32) -> u8 {
    match msfs {
        // Befestigt.
        0 => 2,                     // CONCRETE
        4 | 17 | 18 | 19 | 23 => 1, // ASPHALT, BITUMINUS, BRICK, MACADAM, TARMAC
        // Gras in allen Abstufungen.
        1 | 3 | 5 | 6 | 7 => 3, // GRASS, GRASS BUMPY, SHORT/LONG GRASS, HARD TURF
        // Lose Oberflaechen.
        12 | 15 | 21 => 4, // DIRT, OIL TREATED, SAND
        13 | 14 | 22 => 5, // CORAL, GRAVEL, SHALE
        // Wasser in allen Formen — auch die FSX-Altlast auf der 2.
        2 | 26 | 27 | 28 | 29 | 30 | 31 => 13,
        8 | 9 => 14, // SNOW, ICE
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

/// Ein Wert aus einem Facility-Datenblock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Wert {
    F64(f64),
    F32(f32),
    I32(i32),
}

impl Wert {
    pub fn als_f64(self) -> f64 {
        match self {
            Wert::F64(v) => v,
            Wert::F32(v) => v as f64,
            Wert::I32(v) => v as f64,
        }
    }
    pub fn als_i32(self) -> i32 {
        match self {
            Wert::F64(v) => v as i32,
            Wert::F32(v) => v as i32,
            Wert::I32(v) => v,
        }
    }
}

/// Einen Datenblock nach einer Felddefinition zerlegen.
///
/// # Warum das hier steht und nicht im Windows-Teil
///
/// Weil es die einzige Stelle ist, an der etwas still schiefgehen kann,
/// ohne dass ein Fehler auftritt: Ein Feld zu viel, eines zu wenig oder
/// eine falsche Groesse verschiebt alles danach — und heraus kommen
/// plausible Zahlen an falschen Stellen. Der FFI-Aufruf drumherum
/// scheitert dagegen laut.
///
/// Hier ist es ohne Simulator pruefbar, auf jedem Rechner.
///
/// ⚠ SimConnect richtet die Werte an ihrer eigenen Groesse aus: ein
/// `f64` beginnt an einer durch 8 teilbaren Stelle. Wer stumpf
/// aneinanderreiht, liest ab dem ersten `f64` nach einem `f32` Unsinn.
pub fn zerlege(felder: &[(&str, FeldTyp)], bytes: &[u8]) -> Option<Vec<Wert>> {
    let mut aus = Vec::with_capacity(felder.len());
    let mut pos = 0usize;
    for (_, typ) in felder {
        if *typ == FeldTyp::Text {
            // Eine Zeichenkette hat keine feste Groesse; sie gehoert
            // nicht in dieses Raster. `name_aus_bytes` liest sie.
            return None;
        }
        let g = typ.groesse();
        // Ausrichtung auf die eigene Groesse.
        pos = pos.div_ceil(g) * g;
        if pos + g > bytes.len() {
            return None;
        }
        let scheibe = &bytes[pos..pos + g];
        aus.push(match typ {
            FeldTyp::F64 => Wert::F64(f64::from_le_bytes(scheibe.try_into().ok()?)),
            FeldTyp::F32 => Wert::F32(f32::from_le_bytes(scheibe.try_into().ok()?)),
            FeldTyp::I32 => Wert::I32(i32::from_le_bytes(scheibe.try_into().ok()?)),
            // Oben schon abgewiesen — hier nur, damit der Uebersetzer
            // sieht, dass der Fall behandelt ist.
            FeldTyp::Text => return None,
        });
        pos += g;
    }
    Some(aus)
}

/// Einen eingetroffenen PAVEMENT-Satz an sein Bahnende haengen.
///
/// `bahnen` ist der laufende Sammler; die letzten ZWEI Eintraege sind
/// das zuletzt gelesene Bahnenpaar (primaeres Ende zuerst).
/// `wievielter` zaehlt, der wievielte PAVEMENT-Satz seit diesem
/// Bahnsatz eingetroffen ist: 0 = PRIMARY_THRESHOLD,
/// 1 = SECONDARY_THRESHOLD. Weitere werden verworfen.
///
/// # Was `ENABLE` bedeutet
///
/// ⚠ `ENABLE = 0` ist eine AUSSAGE ("an diesem Ende gibt es keine
/// versetzte Schwelle"), kein Schweigen — sie schlaegt darum den
/// Navdaten-Wert, wie jede andere Zahl aus der Szenerie auch (siehe
/// [[aeroacars-bahn-aus-der-szenerie]]: Der Simulator ist die erste
/// Instanz, weil der Pilot dort landet).
///
/// Der Unterschied ist nicht theoretisch: Bei LAN273 (TJPS 12,
/// 30.08.2026) fuehren die Navdaten 573 m versetzte Schwelle, der Pilot
/// sagt, im Simulator sei dort keine. Genau diese Frage beantwortet
/// `ENABLE`.
///
/// Kommt gar kein PAVEMENT-Satz, bleibt die Schwelle NaN — dann wissen
/// wir nichts, und der Navdaten-Wert gilt weiter.
pub fn pavement_anhaengen(bahnen: &mut [SzenerieBahn], wievielter: usize, bytes: &[u8]) -> bool {
    if wievielter > 1 || bahnen.len() < 2 {
        return false;
    }
    let Some(w) = zerlege(PAVEMENT_FELDER, bytes) else {
        return false;
    };
    // Von hinten: die letzten zwei Eintraege sind das aktuelle Paar.
    let ziel = bahnen.len() - 2 + wievielter;
    let laenge = w[0].als_f64();
    let aktiv = w[2].als_i32() != 0;
    // ⚠ Nicht nur ENABLE pruefen: Eine "aktive" Schwelle mit Laenge 0
    // ist dasselbe wie keine, eine negative waere Unsinn. Dann lieber
    // nichts sagen als etwas Falsches.
    bahnen[ziel].versetzte_schwelle_m = if !aktiv {
        0.0
    } else if laenge.is_finite() && laenge > 0.0 {
        laenge
    } else {
        f64::NAN
    };
    true
}

/// Der Sammler fuer eine Facility-Lieferung: Bahnsaetze und die
/// PAVEMENT-Saetze, die zu ihnen gehoeren.
///
/// # Warum das hier steht und nicht im Verteiler
///
/// Der Verteiler in `adapter.rs` liegt hinter `cfg(target_os =
/// "windows")` und wird auf dem Mac gar nicht uebersetzt — dort ist
/// nichts pruefbar. Die REIHENFOLGE ist aber genau das, was schiefgehen
/// kann: Ein PAVEMENT-Satz gehoert zum zuletzt gelesenen Bahnsatz, und
/// wer den Zaehler nicht bei jeder neuen Bahn zurueckstellt, haengt die
/// Schwelle der zweiten Bahn an das falsche Ende der ersten.
///
/// Deshalb fuehrt der Sammler den Zustand, und der Verteiler ruft nur
/// noch. Eine Implementierung, an einem Ort, auf jedem Rechner
/// pruefbar.
#[derive(Debug, Default)]
pub struct Szeneriesammler {
    bahnen: Vec<SzenerieBahn>,
    /// Der wievielte PAVEMENT-Satz seit dem letzten Bahnsatz.
    /// 0 = PRIMARY_THRESHOLD, 1 = SECONDARY_THRESHOLD.
    pavement_zaehler: usize,
    /// Ob der zuletzt gelesene Bahnsatz brauchbar war.
    ///
    /// ⚠ Ohne dieses Feld landen die PAVEMENT-Saetze eines UNLESBAREN
    /// Bahnsatzes auf der VORHERIGEN Bahn — sie ist dann ja die letzte
    /// im Sammler. Der Fehler ist doppelt still: Die kaputte Bahn fehlt
    /// ohnehin, und die intakte bekommt eine fremde Schwelle
    /// untergeschoben. Gefunden vom Test
    /// `ein_unlesbarer_bahnsatz_stellt_den_zaehler_trotzdem_zurueck`.
    bahn_gueltig: bool,
}

/// Alles, was zu EINER Facility-Anfrage gehoert.
///
/// ⚠ Bis v1.7.14 lagen Sammler und Rollweglisten als einzelne Variablen
/// neben der Schleife — geteilt von allen Anfragen. Zusammen mit einer
/// festen Anfragekennung hiess das: Kommt eine Antwort nach der
/// Wartezeit, waehrend schon die naechste laeuft, mischen sich beide
/// Lieferungen in denselben Listen. Das SDK schickt bis zum Endesatz
/// mehrere Nachrichten und gibt in jeder die clientdefinierte Kennung
/// zurueck — die Trennung ist vorgesehen, sie wurde nur nicht benutzt.
#[derive(Debug, Default)]
pub struct Lieferung {
    pub sammler: Szeneriesammler,
    /// Referenzpunkt des Flughafens — ohne ihn sind die Rollwegpunkte
    /// nicht umrechenbar.
    pub referenz: Option<(f64, f64)>,
    /// ⚠ Die drei Listen haengen ueber INDIZES zusammen. Wer sortiert,
    /// filtert oder zwei Lieferungen vermischt, verschiebt die Verweise.
    pub punkte: Vec<(f64, f64)>,
    pub namen: Vec<String>,
    pub kanten: Vec<(usize, usize, usize)>,
}

/// Wie viele Lieferungen gleichzeitig offen bleiben duerfen.
///
/// Vier reichen: Start, Ziel, Ausweich und eine verspaetete. Mehr waere
/// ein Leck, weniger wuerde die verspaetete verwerfen, um die es geht.
pub const LIEFERUNGEN_MAX: usize = 4;

/// Die offenen Lieferungen, nach Anfragekennung getrennt.
#[derive(Debug, Default)]
pub struct Lieferungen {
    offen: Vec<(u32, Lieferung)>,
}

impl Lieferungen {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Eine neue Lieferung beginnen. Eine gleiche Kennung wird ersetzt.
    pub fn eroeffnen(&mut self, id: u32) {
        self.offen.retain(|(k, _)| *k != id);
        self.offen.push((id, Lieferung::default()));
        while self.offen.len() > LIEFERUNGEN_MAX {
            self.offen.remove(0);
        }
    }

    /// Die Lieferung zu dieser Kennung — oder nichts.
    ///
    /// ⚠ `None` heisst: verwerfen, nicht raten. Eine Nachricht ohne
    /// zugehoerige Anfrage gehoert niemandem.
    pub fn zu(&mut self, id: u32) -> Option<&mut Lieferung> {
        self.offen
            .iter_mut()
            .find(|(k, _)| *k == id)
            .map(|(_, l)| l)
    }

    /// Die Lieferung herausnehmen und schliessen.
    pub fn abschliessen(&mut self, id: u32) -> Option<Lieferung> {
        let pos = self.offen.iter().position(|(k, _)| *k == id)?;
        Some(self.offen.remove(pos).1)
    }

    pub fn anzahl_offen(&self) -> usize {
        self.offen.len()
    }
}

/// Zu welchem Auftrag eine SimConnect-Ausnahme gehoert.
///
/// ⚠ MSFS meldet eine zurueckgewiesene Facility-Anfrage NICHT beim
/// Aufruf, sondern spaeter und asynchron — und verweist dabei auf die
/// Paketkennung des Aufrufs. Ohne diese Zuordnung wird die Ausnahme nur
/// protokolliert, der Auftrag bleibt „unterwegs", und das Buch fragt
/// zehnmal nach einem Platz, den der Simulator gar nicht kennt
/// (QS-Befund 4, 01.09.2026).
///
/// Dasselbe Muster benutzt der Inspektor fuer seine Feldnamen.
pub fn auftrag_zu_paket(paare: &[(u32, u32)], send_id: u32) -> Option<u32> {
    paare
        .iter()
        .rev()
        .find(|(paket, _)| *paket == send_id)
        .map(|(_, auftrag)| *auftrag)
}

/// Zu welchem Facility-FELDNAMEN eine Ausnahme gehoert.
///
/// ⚠ Das SDK nennt eine asynchrone Ausnahme ausdruecklich als moeglichen
/// Ausgang von `AddToFacilityDefinition` — ein `hr == 0` beim Aufruf
/// sagt nichts ueber den Feldnamen. Ohne diese Zuordnung deutet der
/// Ausnahmezweig ihren `index` ueber die TELEMETRIE-Feldliste und nennt
/// einen fremden SimVar-Namen (QS-Befund 4, zweite Runde).
pub fn feld_zu_paket(paare: &[(u32, String)], send_id: u32) -> Option<String> {
    paare
        .iter()
        .rev()
        .find(|(paket, _)| *paket == send_id)
        .map(|(_, feld)| feld.clone())
}

/// Was eine SimConnect-Ausnahme fuer den Facility-Weg bedeutet.
///
/// ⚠ Bis v1.7.14 wurde JEDE Ausnahme dem Flughafen angelastet und der
/// Platz dauerhaft auf „abgelehnt" gesetzt — und ein neues Fenster laesst
/// abgelehnte Plaetze bewusst geschlossen. Damit sperrte eine
/// voruebergehende Ueberlast den Platz fuer den Rest des Fluges aus, und
/// ein Fehler der DEFINITION sah aus wie ein Problem des Platzes
/// (QS-Befund 3, fuenfte Runde).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ausnahmeart {
    /// Die Definition taugt nicht — der ganze Weg ist zu.
    Definition,
    /// Dieser Platz ist ungueltig (ICAO/Region) — dauerhaft ablehnen.
    Platz,
    /// Voruebergehend — erneut versuchen ist richtig.
    Voruebergehend,
    /// Nicht zuzuordnen — protokollieren, sonst nichts.
    Unklar,
}

// Nummern aus `SIMCONNECT_EXCEPTION` im mitgelieferten SDK-Header
// (`ffi/include/SimConnect.h`). Die Aufzaehlung ist fortlaufend ab
// NONE = 0; die Werte hier sind gezaehlt, nicht geraten.
const EXC_ERROR: u32 = 1;
const EXC_SIZE_MISMATCH: u32 = 2;
const EXC_UNRECOGNIZED_ID: u32 = 3;
const EXC_NAME_UNRECOGNIZED: u32 = 7;
const EXC_TOO_MANY_REQUESTS: u32 = 12;
const EXC_INVALID_DATA_TYPE: u32 = 18;
const EXC_INVALID_DATA_SIZE: u32 = 19;
const EXC_DATA_ERROR: u32 = 20;
const EXC_INVALID_ENUM: u32 = 27;
const EXC_DEFINITION_ERROR: u32 = 28;
const EXC_DATUM_ID: u32 = 30;
const EXC_INTERNAL: u32 = 44;

/// Eine Ausnahme einordnen.
pub fn ausnahme_einordnen(exception: u32) -> Ausnahmeart {
    match exception {
        // Die Definition selbst ist unbrauchbar — ein falscher Feldname,
        // ein falscher Datentyp, eine unbekannte Kennung. Weiterfragen
        // liefert nur Antworten, die niemand deuten kann.
        EXC_UNRECOGNIZED_ID
        | EXC_NAME_UNRECOGNIZED
        | EXC_SIZE_MISMATCH
        | EXC_INVALID_DATA_TYPE
        | EXC_INVALID_DATA_SIZE
        | EXC_DATA_ERROR
        | EXC_INVALID_ENUM
        | EXC_DEFINITION_ERROR
        | EXC_DATUM_ID => Ausnahmeart::Definition,
        // Der Platz ist ungueltig — ICAO oder Region kennt der Simulator
        // nicht. Ihn erneut zu fragen aendert daran nichts.
        EXC_ERROR => Ausnahmeart::Platz,
        // ⚠ Voruebergehend. Genau hier lag der Fehler: Eine Ueberlast
        // sperrte den Platz fuer den ganzen Flug aus.
        EXC_TOO_MANY_REQUESTS | EXC_INTERNAL => Ausnahmeart::Voruebergehend,
        _ => Ausnahmeart::Unklar,
    }
}

/// Wie viele Paketkennungen zurueckverfolgt werden.
pub const PAKETE_GEDAECHTNIS: usize = 16;

#[cfg(test)]
mod paketzuordnung_tests {
    use super::*;

    #[test]
    fn die_ausnahme_findet_ihren_auftrag() {
        let paare = [(77u32, 3u32), (78, 4)];
        assert_eq!(auftrag_zu_paket(&paare, 77), Some(3));
        assert_eq!(auftrag_zu_paket(&paare, 78), Some(4));
    }

    /// ⚠ Eine fremde Paketkennung darf NICHT auf den letzten Auftrag
    /// fallen — sonst weist eine Telemetrie-Ausnahme den Szenerie-Auftrag
    /// zurueck.
    #[test]
    fn eine_fremde_kennung_trifft_keinen_auftrag() {
        assert_eq!(auftrag_zu_paket(&[(77, 3)], 99), None);
        assert_eq!(auftrag_zu_paket(&[], 77), None);
    }

    /// Dasselbe fuer die Feldnamen der Definition.
    #[test]
    fn die_ausnahme_findet_ihren_feldnamen() {
        let paare = [
            (41u32, "PRIMARY_NUMBER".to_string()),
            (42, "WIDTH".to_string()),
        ];
        assert_eq!(feld_zu_paket(&paare, 42).as_deref(), Some("WIDTH"));
        assert_eq!(feld_zu_paket(&paare, 41).as_deref(), Some("PRIMARY_NUMBER"));
    }

    /// ⚠ Und eine fremde Kennung trifft KEIN Feld — sonst waere jede
    /// Telemetrie-Ausnahme plötzlich ein Facility-Feldfehler.
    #[test]
    fn eine_fremde_kennung_trifft_kein_feld() {
        let paare = [(41u32, "WIDTH".to_string())];
        assert_eq!(feld_zu_paket(&paare, 99), None);
        assert_eq!(feld_zu_paket(&[], 41), None);
    }

    /// ⚠ QS-Befund 3, fuenfte Runde: Nicht jede Ausnahme gehoert dem
    /// Platz.
    #[test]
    fn ausnahmen_werden_nach_wirkung_eingeordnet() {
        use Ausnahmeart::*;
        // Die Definition taugt nicht — der ganze Weg ist zu.
        for e in [3u32, 7, 2, 18, 19, 20, 27, 28, 30] {
            assert_eq!(ausnahme_einordnen(e), Definition, "Ausnahme {e}");
        }
        // Dieser Platz ist ungueltig.
        assert_eq!(ausnahme_einordnen(1), Platz);
        // ⚠ Voruebergehend — genau die, die vorher den Platz fuer den
        // ganzen Flug aussperrten.
        assert_eq!(ausnahme_einordnen(12), Voruebergehend, "TOO_MANY_REQUESTS");
        assert_eq!(ausnahme_einordnen(44), Voruebergehend, "INTERNAL");
        // Alles andere: protokollieren, sonst nichts.
        assert_eq!(ausnahme_einordnen(99), Unklar);
        assert_eq!(ausnahme_einordnen(0), Unklar, "NONE ist keine Ausnahme");
    }

    /// ⚠ Und die Nummern stimmen mit dem SDK-Header ueberein.
    ///
    /// Die Aufzaehlung ist fortlaufend; die Werte sind gezaehlt, nicht
    /// geraten. Verschiebt eine neue SDK-Fassung die Liste, faellt es
    /// hier auf und nicht erst an einem falsch eingeordneten Fehler.
    #[test]
    fn die_ausnahmenummern_stammen_aus_dem_header() {
        let header = include_str!("../ffi/include/SimConnect.h");
        let anfang = header
            .find("SIMCONNECT_EXCEPTION_NONE")
            .expect("Aufzaehlung fehlt");
        let ende = header[anfang..].find('}').expect("Ende fehlt") + anfang;
        let namen: Vec<&str> = header[anfang..ende]
            .lines()
            .filter_map(|z| z.trim().strip_suffix(','))
            .filter(|z| z.starts_with("SIMCONNECT_EXCEPTION_"))
            .collect();
        for (nr, name) in [
            (EXC_ERROR, "SIMCONNECT_EXCEPTION_ERROR"),
            (EXC_SIZE_MISMATCH, "SIMCONNECT_EXCEPTION_SIZE_MISMATCH"),
            (EXC_UNRECOGNIZED_ID, "SIMCONNECT_EXCEPTION_UNRECOGNIZED_ID"),
            (
                EXC_NAME_UNRECOGNIZED,
                "SIMCONNECT_EXCEPTION_NAME_UNRECOGNIZED",
            ),
            (
                EXC_TOO_MANY_REQUESTS,
                "SIMCONNECT_EXCEPTION_TOO_MANY_REQUESTS",
            ),
            (
                EXC_INVALID_DATA_TYPE,
                "SIMCONNECT_EXCEPTION_INVALID_DATA_TYPE",
            ),
            (
                EXC_INVALID_DATA_SIZE,
                "SIMCONNECT_EXCEPTION_INVALID_DATA_SIZE",
            ),
            (EXC_DATA_ERROR, "SIMCONNECT_EXCEPTION_DATA_ERROR"),
            (EXC_INVALID_ENUM, "SIMCONNECT_EXCEPTION_INVALID_ENUM"),
            (
                EXC_DEFINITION_ERROR,
                "SIMCONNECT_EXCEPTION_DEFINITION_ERROR",
            ),
            (EXC_DATUM_ID, "SIMCONNECT_EXCEPTION_DATUM_ID"),
            (EXC_INTERNAL, "SIMCONNECT_EXCEPTION_INTERNAL"),
        ] {
            assert_eq!(
                namen.get(nr as usize).copied(),
                Some(name),
                "Nummer {nr} ist im Header nicht {name} — die Aufzaehlung \
                 hat sich verschoben"
            );
        }
    }

    /// Bei doppelter Kennung gilt die juengste Zuordnung.
    #[test]
    fn die_juengste_zuordnung_gilt() {
        assert_eq!(auftrag_zu_paket(&[(77, 3), (77, 9)], 77), Some(9));
    }
}

#[cfg(test)]
mod lieferungen_tests {
    use super::*;

    /// ⚠ Der Kern von QS-Befund 1: Zwei Lieferungen duerfen sich nicht
    /// mischen.
    ///
    /// Bis v1.7.14 lagen Punkte, Namen und Kanten in EINER Liste neben
    /// der Schleife. Kommt EDDF nach der Wartezeit, waehrend LEZL laeuft,
    /// landeten beide darin — und weil `START`/`END` POSITIONEN in genau
    /// diesen Listen sind, zeigt danach jede Kante auf einen fremden
    /// Punkt.
    #[test]
    fn zwei_lieferungen_mischen_sich_nicht() {
        let mut l = Lieferungen::neu();
        l.eroeffnen(1001);
        l.eroeffnen(1002);

        l.zu(1001).expect("1001 offen").punkte.push((50.0, 8.0));
        l.zu(1002).expect("1002 offen").punkte.push((37.0, -6.0));
        l.zu(1001).expect("1001 offen").namen.push("A".into());

        let eddf = l.abschliessen(1001).expect("1001 abschliessbar");
        assert_eq!(eddf.punkte, vec![(50.0, 8.0)]);
        assert_eq!(eddf.namen, vec!["A".to_string()]);

        let lezl = l.abschliessen(1002).expect("1002 abschliessbar");
        assert_eq!(lezl.punkte, vec![(37.0, -6.0)]);
        assert!(lezl.namen.is_empty(), "der Name der anderen Lieferung");
    }

    /// Eine Nachricht ohne offene Lieferung gehoert niemandem.
    #[test]
    fn eine_fremde_kennung_hat_keine_lieferung() {
        let mut l = Lieferungen::neu();
        l.eroeffnen(1001);
        assert!(l.zu(1002).is_none());
        assert!(l.abschliessen(1002).is_none());
    }

    /// Abgeschlossen ist abgeschlossen — kein zweites Mal.
    ///
    /// ⚠ Sonst koennte ein doppelter Endesatz dieselbe Auskunft zweimal
    /// ablegen.
    #[test]
    fn eine_lieferung_wird_nur_einmal_abgeschlossen() {
        let mut l = Lieferungen::neu();
        l.eroeffnen(1001);
        assert!(l.abschliessen(1001).is_some());
        assert!(l.abschliessen(1001).is_none());
    }

    /// Die Ablage waechst nicht unbegrenzt — die aelteste faellt raus.
    #[test]
    fn die_ablage_ist_begrenzt() {
        let mut l = Lieferungen::neu();
        for id in 0..(LIEFERUNGEN_MAX as u32 + 2) {
            l.eroeffnen(1000 + id);
        }
        assert_eq!(l.anzahl_offen(), LIEFERUNGEN_MAX);
        assert!(l.zu(1000).is_none(), "die aelteste blieb liegen");
        assert!(l.zu(1000 + LIEFERUNGEN_MAX as u32 + 1).is_some());
    }

    /// Dieselbe Kennung noch einmal eroeffnen faengt von vorn an.
    ///
    /// ⚠ Sonst truege ein Wiederholungsversuch die halbe Antwort des
    /// vorigen mit sich.
    #[test]
    fn ein_zweiter_versuch_faengt_leer_an() {
        let mut l = Lieferungen::neu();
        l.eroeffnen(1001);
        l.zu(1001).expect("offen").punkte.push((1.0, 2.0));
        l.eroeffnen(1001);
        assert!(l.zu(1001).expect("offen").punkte.is_empty());
        assert_eq!(l.anzahl_offen(), 1);
    }
}

impl Szeneriesammler {
    pub fn neu() -> Self {
        Self::default()
    }

    /// Ein RUNWAY-Satz. Gibt zurueck, ob er gelesen werden konnte.
    pub fn bahnsatz(&mut self, bytes: &[u8]) -> bool {
        // ⚠ ZUERST zuruecksetzen, auch wenn der Satz unlesbar ist: Die
        // PAVEMENT-Saetze, die jetzt kommen, gehoeren zu IHM. Wer den
        // Zaehler nur im Erfolgsfall stellt, haengt sie an die
        // vorherige Bahn.
        self.pavement_zaehler = 0;
        self.bahn_gueltig = false;
        match zerlege(BAHN_FELDER, bytes).and_then(|w| bahn_aus_werten(&w)) {
            Some(paar) => {
                self.bahnen.extend(paar);
                self.bahn_gueltig = true;
                true
            }
            None => false,
        }
    }

    /// Ein PAVEMENT-Satz. Gibt zurueck, ob er zugeordnet werden konnte.
    pub fn pavementsatz(&mut self, bytes: &[u8]) -> bool {
        let nummer = self.pavement_zaehler;
        self.pavement_zaehler += 1;
        // ⚠ Gehoert der Satz zu einer Bahn, die wir nicht lesen konnten,
        // wird er VERWORFEN — nicht der vorherigen Bahn angehaengt.
        if !self.bahn_gueltig {
            return false;
        }
        pavement_anhaengen(&mut self.bahnen, nummer, bytes)
    }

    pub fn fertig(self) -> Vec<SzenerieBahn> {
        self.bahnen
    }

    /// Wie viele Bahnen bisher gelesen wurden (zwei je Bahnsatz — je
    /// Ende eine).
    pub fn anzahl(&self) -> usize {
        self.bahnen.len()
    }
}

/// Aus einem zerlegten Bahn-Datenblock das Bahnenpaar bauen.
pub fn bahn_aus_werten(w: &[Wert]) -> Option<[SzenerieBahn; 2]> {
    if w.len() < BAHN_FELDER.len() {
        return None;
    }
    let mitte = (w[0].als_f64(), w[1].als_f64());
    let kurs = w[3].als_f64();
    let laenge = w[4].als_f64();
    let breite = w[5].als_f64();
    let belag = belag_code(w[6].als_i32());
    Some(bahn_paar(
        mitte,
        kurs,
        laenge,
        breite,
        belag,
        // ⚠ NaN, nicht 0,0: Die versetzte Schwelle steht NICHT im
        // flachen Bahnsatz — sie kommt als eigener PAVEMENT-Satz
        // hinterher (siehe `PAVEMENT_FELDER`). Eine 0 waere hier eine
        // AUSSAGE ("keine versetzte Schwelle") und wuerde den echten
        // Navdaten-Wert ueberschreiben, bevor der PAVEMENT-Satz
        // ueberhaupt eingetroffen ist. NaN faellt durch
        // `plausibel::versatz_m` und laesst ihn stehen.
        (w[7].als_i32(), w[8].als_i32(), f64::NAN),
        (w[9].als_i32(), w[10].als_i32(), f64::NAN),
    ))
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
        // ⚠ Diese Werte stehen so in der SimConnect-Doku. Bis v1.7.12
        // stand hier die X-Plane-Aufzählung, angewandt auf MSFS-Zahlen —
        // und der Test schrieb sie fest, statt sie zu finden.
        assert_eq!(belag_code(0), 2, "CONCRETE -> Beton");
        assert_eq!(belag_code(4), 1, "ASPHALT -> Asphalt");
        assert_eq!(belag_code(23), 1, "TARMAC -> Asphalt");
        assert_eq!(belag_code(1), 3, "GRASS -> Rasen");
        assert_eq!(belag_code(7), 3, "HARD TURF -> Rasen");
        assert_eq!(belag_code(2), 13, "WATER FSX -> Wasser");
        assert_eq!(belag_code(27), 13, "WATER -> Wasser");
        assert_eq!(belag_code(14), 5, "GRAVEL -> Kies");
        assert_eq!(belag_code(12), 4, "DIRT -> Erde");
        assert_eq!(belag_code(9), 14, "ICE -> Schnee/Eis");
        // ⚠ Unbekanntes wird 0 (= nicht zuzuordnen), NICHT Asphalt.
        // Ein geratener Belag waere eine Aussage, die wir nicht haben —
        // und die seitliche Bewertung haengt daran.
        assert_eq!(belag_code(99), 0);
        assert_eq!(belag_code(254), 0, "UNKNOWN bleibt unbekannt");
        assert_eq!(belag_code(255), 0, "UNDEFINED bleibt unbekannt");
        assert_eq!(belag_code(25), 0, "25 fehlt in der Doku — nicht raten");
        for ohne_entsprechung in [10, 11, 16, 20, 24, 32] {
            assert_eq!(
                belag_code(ohne_entsprechung),
                0,
                "{ohne_entsprechung} hat keine apt.dat-Entsprechung und darf \
                 nicht hingebogen werden"
            );
        }
    }

    /// Die beiden Simulatoren zählen verschieden — das muss so bleiben.
    ///
    /// ⚠ Der Fehler war nicht ein verschobener Wert, sondern eine
    /// ANDERE Tabelle: X-Plane-Bedeutungen auf MSFS-Zahlen. Dieser Test
    /// hält die vier Stellen fest, an denen sich die beiden am
    /// deutlichsten widersprechen — wer die X-Plane-Aufzählung wieder
    /// einsetzt, wird an allen vieren rot.
    #[test]
    fn die_msfs_tabelle_ist_nicht_die_der_apt_dat() {
        // In der apt.dat: 1 = Asphalt, 2 = Beton, 4 = Erde, 5 = Kies.
        // In MSFS: 1 = Gras, 2 = Wasser, 4 = Asphalt, 5 = kurzes Gras.
        assert_eq!(belag_code(1), 3, "MSFS 1 ist GRASS, nicht Asphalt");
        assert_eq!(belag_code(2), 13, "MSFS 2 ist WATER, nicht Beton");
        assert_eq!(belag_code(4), 1, "MSFS 4 ist ASPHALT, nicht Erde");
        assert_eq!(belag_code(5), 3, "MSFS 5 ist SHORT GRASS, nicht Kies");
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
    /// Anmeldung und Byte-Raster duerfen nicht auseinanderlaufen.
    ///
    /// ⚠ Das ist die Naht, an der v1.7.8 gebrochen ist: Damals standen
    /// zwei Namen in der Anmeldung, die keine eigenen Bytes liefern —
    /// jeder Bahnsatz war danach um zwei Felder verschoben, und ALLE
    /// MSFS-Bahndaten waren still unbrauchbar. Seit die Anmeldung eine
    /// eigene Liste ist, kann dieselbe Klasse nur noch hier auffallen.
    #[test]
    fn definition_und_raster_passen_zusammen() {
        // Aus der Anmeldung die Namen ziehen, die WIRKLICH Bytes
        // liefern: alles ausser den OPEN/CLOSE-Marken.
        let liefernde: Vec<&str> = BAHN_DEFINITION
            .iter()
            .copied()
            .filter(|t| !t.starts_with("OPEN ") && !t.starts_with("CLOSE "))
            .collect();

        // ⚠ Die Anmeldung liefert den flachen Bahnsatz UND die beiden
        // PAVEMENT-Untersaetze. Die Bytes kommen aber getrennt: der
        // Bahnsatz als RUNWAY-Nachricht, jeder Untersatz als eigene
        // PAVEMENT-Nachricht. Deshalb wird hier gegen BEIDE Raster
        // geprueft, in genau dieser Reihenfolge.
        let erwartet: Vec<&str> = BAHN_FELDER
            .iter()
            .map(|(n, _)| *n)
            .chain(PAVEMENT_FELDER.iter().map(|(n, _)| *n))
            .chain(PAVEMENT_FELDER.iter().map(|(n, _)| *n))
            .collect();

        assert_eq!(
            liefernde.len(),
            erwartet.len(),
            "Anmeldung liefert {} Werte, die Raster erwarten {} — \
             jeder Satz waere ab hier verschoben",
            liefernde.len(),
            erwartet.len()
        );

        for (i, name) in erwartet.iter().enumerate() {
            assert_eq!(
                liefernde[i], *name,
                "Feld {i} heisst in der Anmeldung {:?}, im Raster {name:?}",
                liefernde[i]
            );
        }
    }

    /// Die Untersaetze muessen sauber geklammert sein.
    ///
    /// ⚠ Ein fehlendes CLOSE nimmt SimConnect an und schweigt; die
    /// Folgefelder landen dann im falschen Satz.
    #[test]
    fn jeder_untersatz_wird_wieder_geschlossen() {
        let mut offen: Vec<&str> = Vec::new();
        for token in BAHN_DEFINITION {
            if let Some(name) = token.strip_prefix("OPEN ") {
                offen.push(name);
            } else if let Some(name) = token.strip_prefix("CLOSE ") {
                assert_eq!(
                    offen.pop(),
                    Some(name),
                    "CLOSE {name} passt zu keinem offenen Untersatz"
                );
            }
        }
        assert!(offen.is_empty(), "nicht geschlossen: {offen:?}");
    }

    #[test]
    fn feldnamen_sind_gross_mit_unterstrich() {
        // ⚠ Geprueft werden die Namen, die WIRKLICH an SimConnect gehen.
        // Fuer die Bahn ist das seit v1.7.12 `BAHN_DEFINITION` (mit den
        // OPEN/CLOSE-Marken); `BAHN_FELDER` traegt nur noch das
        // Byte-Raster und darf sprechende Namen wie
        // `PRIMARY_THRESHOLD.LENGTH` fuehren.
        for token in BAHN_DEFINITION {
            let name = token
                .strip_prefix("OPEN ")
                .or_else(|| token.strip_prefix("CLOSE "))
                .unwrap_or(token);
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
        for liste in [ROLLWEG_PUNKT_FELDER, ROLLWEG_KANTE_FELDER] {
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

#[cfg(test)]
mod zerleger_tests {
    use super::*;

    /// Einen Datenblock so bauen, wie SimConnect ihn liefert —
    /// einschliesslich Ausrichtung.
    fn baue(felder: &[(&str, FeldTyp)], werte: &[Wert]) -> Vec<u8> {
        let mut b: Vec<u8> = Vec::new();
        for ((_, typ), w) in felder.iter().zip(werte) {
            let g = typ.groesse();
            while b.len() % g != 0 {
                b.push(0);
            }
            match (typ, w) {
                (FeldTyp::F64, Wert::F64(v)) => b.extend_from_slice(&v.to_le_bytes()),
                (FeldTyp::F32, Wert::F32(v)) => b.extend_from_slice(&v.to_le_bytes()),
                (FeldTyp::I32, Wert::I32(v)) => b.extend_from_slice(&v.to_le_bytes()),
                _ => panic!("Testdaten passen nicht zur Definition"),
            }
        }
        b
    }

    fn eddh_werte() -> Vec<Wert> {
        vec![
            Wert::F64(53.6304), // LATITUDE  (Mitte)
            Wert::F64(9.9882),  // LONGITUDE
            Wert::F64(16.0),    // ALTITUDE
            Wert::F32(52.5),    // HEADING
            Wert::F32(3250.0),  // LENGTH
            Wert::F32(46.0),    // WIDTH
            Wert::I32(0),       // SURFACE (Beton)
            Wert::I32(5),       // PRIMARY_NUMBER
            Wert::I32(0),       // PRIMARY_DESIGNATOR
            Wert::I32(23),      // SECONDARY_NUMBER
            Wert::I32(0),       // SECONDARY_DESIGNATOR
                                // ⚠ Hier standen PRIMARY_/SECONDARY_THRESHOLD als F32 — und
                                // genau das machte diesen Test wertlos: Er baute die Bytes
                                // aus DERSELBEN Feldliste, gegen die er sie dann prueft. Ein
                                // Rundlauf gegen die eigene Annahme kann eine falsche
                                // Annahme nicht finden. Beide Felder sind laut SDK STRUCTs
                                // und kommen im flachen Satz gar nicht vor.
        ]
    }

    /// Die Laenge des flachen Bahnsatzes — aus der SDK-DOKU, nicht aus
    /// unserer Feldliste.
    ///
    /// ⚠ Das ist der Wert, der den Fehler gefunden haette. Er ist bewusst
    /// von Hand ausgerechnet: 3 x FLOAT64 (LAT/LON/ALT) + 3 x FLOAT32
    /// (HEADING/LENGTH/WIDTH) + 5 x INT32 (SURFACE, PRIMARY_NUMBER,
    /// PRIMARY_DESIGNATOR, SECONDARY_NUMBER, SECONDARY_DESIGNATOR).
    /// Wer ein Feld ergaenzt, muss diese Zahl bewusst mit aendern — und
    /// dabei in der Doku nachsehen, ob das Feld ueberhaupt flach kommt.
    // 3x FLOAT64 + 3x FLOAT32 + 5x INT32. OHNE die versetzte Schwelle —
    // die kommt als eigener PAVEMENT-Satz.
    const BAHNSATZ_BYTES_LAUT_SDK: usize = 3 * 8 + 3 * 4 + 5 * 4;

    #[test]
    fn das_bahnraster_passt_zur_sdk_doku() {
        // ⚠ DIE Wache gegen den Fehler, der die MSFS-Szenerie seit
        // v1.7.8 wirkungslos machte: Zwei STRUCT-Felder waren als F32
        // deklariert, das Raster erwartete 64 statt 56 Bytes, und JEDER
        // Bahnsatz fiel beim Zerlegen durch — lautlos.
        //
        // Die erwartete Zahl kommt aus der Doku, nicht aus BAHN_FELDER.
        // Ein Test, der beides aus derselben Quelle nimmt, kann eine
        // falsche Quelle nicht finden.
        let gebraucht: usize = BAHN_FELDER.iter().map(|(_, t)| t.groesse()).sum();
        assert_eq!(
            gebraucht, BAHNSATZ_BYTES_LAUT_SDK,
            "das Bahnraster erwartet {gebraucht} Bytes, die SDK-Doku \
             beschreibt {BAHNSATZ_BYTES_LAUT_SDK} — ein Feld ist zu viel, \
             zu wenig, oder hat den falschen Typ (STRUCT-Felder kommen \
             NICHT im flachen Satz)"
        );
    }

    /// Der PAVEMENT-Satz hat sein eigenes Raster — und ist KEIN Teil des
    /// Bahnsatzes.
    ///
    /// ⚠ Genau diese Verwechslung war der zweite Fehlversuch am
    /// 30.08.2026: sechs PAVEMENT-Felder ins flache Bahnraster gehaengt,
    /// weil ich den Enum-Block der SDK nur bis Zeile 335 gelesen und das
    /// Fehlen von `SIMCONNECT_FACILITY_DATA_PAVEMENT` (Zeile 338) als
    /// Tatsache gemeldet hatte. Der Bahnsatz kommt mit 56 Bytes, das
    /// Raster haette 80 verlangt — jede MSFS-Bahn waere durchgefallen,
    /// genau wie bei v1.7.8.
    #[test]
    fn das_pavementraster_ist_getrennt_und_passt_zur_sdk_doku() {
        // LENGTH (FLOAT32) + WIDTH (FLOAT32) + ENABLE (INT32).
        const PAVEMENT_BYTES_LAUT_SDK: usize = 2 * 4 + 4;
        let gebraucht: usize = PAVEMENT_FELDER.iter().map(|(_, t)| t.groesse()).sum();
        assert_eq!(gebraucht, PAVEMENT_BYTES_LAUT_SDK);

        // Und keines dieser Felder darf im Bahnraster stehen.
        for (name, _) in BAHN_FELDER {
            assert!(
                !name.contains("THRESHOLD"),
                "{name} gehoert in den PAVEMENT-Satz, nicht ins Bahnraster — \
                 dort verschiebt es jeden Bahnsatz um seine Bytes"
            );
        }
    }

    #[test]
    fn zerlegen_gibt_die_werte_zurueck_die_hineingingen() {
        // ⚠ Die eigentliche Gefahr: Ein Feld zu viel oder zu wenig
        // verschiebt ALLES danach, und heraus kommen plausible Zahlen an
        // falschen Stellen. Kein Fehler, kein Log — nur eine Bahn, die
        // 46 Meter breit ist, weil dort zufaellig die Laenge stand.
        let werte = eddh_werte();
        let bytes = baue(BAHN_FELDER, &werte);
        let zurueck = zerlege(BAHN_FELDER, &bytes).expect("zerlegbar");
        assert_eq!(zurueck.len(), werte.len());
        for (i, (a, b)) in werte.iter().zip(zurueck.iter()).enumerate() {
            assert_eq!(a, b, "Feld {} ({})", i, BAHN_FELDER[i].0);
        }
    }

    #[test]
    fn ausrichtung_wird_beachtet() {
        // Ein f64 beginnt an einer durch 8 teilbaren Stelle. Wer stumpf
        // aneinanderreiht, liest ab dem ersten f64 nach einem f32
        // Unsinn — und zwar ohne Fehlermeldung.
        let felder: &[(&str, FeldTyp)] = &[
            ("A", FeldTyp::F32),
            ("B", FeldTyp::F64),
            ("C", FeldTyp::I32),
        ];
        let werte = vec![Wert::F32(1.5), Wert::F64(2.25), Wert::I32(7)];
        let bytes = baue(felder, &werte);
        // 4 Byte f32 + 4 Byte Fuellung + 8 Byte f64 + 4 Byte i32
        assert_eq!(bytes.len(), 20, "Ausrichtung nicht wie erwartet");
        assert_eq!(zerlege(felder, &bytes).unwrap(), werte);
    }

    #[test]
    fn zu_kurzer_block_gibt_nichts_statt_muell() {
        let bytes = baue(BAHN_FELDER, &eddh_werte());
        let kurz = &bytes[..bytes.len() - 4];
        assert!(
            zerlege(BAHN_FELDER, kurz).is_none(),
            "abgeschnittener Block muss abgelehnt werden, nicht halb gelesen"
        );
    }

    #[test]
    fn aus_werten_wird_ein_bahnenpaar() {
        let werte = eddh_werte();
        let [a, b] = bahn_aus_werten(&werte).expect("Bahnenpaar");
        assert_eq!(a.bezeichner, "05");
        assert_eq!(b.bezeichner, "23");
        assert!((a.kurs_grad - 52.5).abs() < 1e-6);
        assert!((b.kurs_grad - 232.5).abs() < 1e-6);
        assert!((a.breite_m - 46.0).abs() < 1e-6);
        // ⚠ Die versetzte Schwelle ist NaN, nicht 0,0.
        //
        // MSFS liefert sie im flachen Bahnsatz nicht — sie ist ein
        // PAVEMENT-Untersatz. Eine 0 waere eine AUSSAGE ("keine versetzte
        // Schwelle") und wuerde den echten Navdaten-Wert ueberschreiben.
        // NaN faellt durch `plausibel::versatz_m` und laesst ihn stehen.
        // ⚠ Aus dem FLACHEN Bahnsatz kommt keine Schwelle — sie ist
        // eine eigene PAVEMENT-Nachricht. Bis die eintrifft, darf hier
        // nichts behauptet werden (siehe `pavement_saetze_kommen_getrennt`).
        assert!(a.versetzte_schwelle_m.is_nan());
        assert!(b.versetzte_schwelle_m.is_nan());
        // CONCRETE (MSFS 0) -> apt.dat 2 = CONC. Befestigt, aber Beton,
        // nicht Asphalt: Die apt.dat unterscheidet beides.
        assert_eq!(a.belag_code, 2, "Beton muss als Beton ankommen");
    }

    /// Der ganze Nachrichtenstrom, so wie der Simulator ihn schickt.
    ///
    /// ⚠ Das ist der Test, der beim ersten Anlauf gefehlt hat. Zwei
    /// Bahnen hintereinander, jede mit ihren beiden PAVEMENT-Saetzen —
    /// wer den Zaehler nicht bei JEDEM Bahnsatz zuruecksetzt, haengt
    /// die Schwelle der zweiten Bahn an das falsche Ende der ersten.
    /// Der Fehler faellt sonst erst 573 m spaeter auf.
    #[test]
    fn ein_ganzer_nachrichtenstrom_ordnet_jede_schwelle_ihrem_ende_zu() {
        let bahn = bytes_aus_werten(&eddh_werte());
        let mut sammler = Szeneriesammler::neu();

        // Erste Bahn: 120 m am primaeren Ende, nichts am sekundaeren.
        assert!(sammler.bahnsatz(&bahn));
        assert!(sammler.pavementsatz(&pavement_bytes(120.0, 45.0, 1)));
        assert!(sammler.pavementsatz(&pavement_bytes(0.0, 0.0, 0)));

        // Zweite Bahn: nichts am primaeren, 300 m am sekundaeren.
        assert!(sammler.bahnsatz(&bahn));
        assert!(sammler.pavementsatz(&pavement_bytes(0.0, 0.0, 0)));
        assert!(sammler.pavementsatz(&pavement_bytes(300.0, 45.0, 1)));

        let b = sammler.fertig();
        assert_eq!(b.len(), 4, "zwei Bahnsaetze ergeben vier Enden");
        assert!((b[0].versetzte_schwelle_m - 120.0).abs() < 1e-6);
        assert_eq!(b[1].versetzte_schwelle_m, 0.0);
        assert_eq!(b[2].versetzte_schwelle_m, 0.0);
        assert!((b[3].versetzte_schwelle_m - 300.0).abs() < 1e-6);
    }

    /// Ein unlesbarer Bahnsatz darf die folgenden Schwellen nicht an die
    /// vorherige Bahn haengen.
    #[test]
    fn ein_unlesbarer_bahnsatz_stellt_den_zaehler_trotzdem_zurueck() {
        let mut sammler = Szeneriesammler::neu();
        assert!(sammler.bahnsatz(&bytes_aus_werten(&eddh_werte())));
        assert!(sammler.pavementsatz(&pavement_bytes(120.0, 45.0, 1)));

        // Ein zu kurzer Bahnsatz — er liefert keine Bahn.
        assert!(!sammler.bahnsatz(&[0u8; 8]));
        // Der folgende PAVEMENT-Satz gehoert zu IHM, nicht zur ersten
        // Bahn. Er zaehlt als der erste seit diesem Bahnsatz und
        // ueberschriebe sonst die schon gesetzten 120 m.
        assert!(
            !sammler.pavementsatz(&pavement_bytes(999.0, 45.0, 1)),
            "ein Satz zu einer unlesbaren Bahn muss verworfen werden"
        );

        let b = sammler.fertig();
        assert!(
            (b[0].versetzte_schwelle_m - 120.0).abs() < 1e-6,
            "die Schwelle der ersten Bahn wurde ueberschrieben: {}",
            b[0].versetzte_schwelle_m
        );
    }

    /// Werte in den Byte-Strom giessen, wie der Simulator ihn schickt.
    fn bytes_aus_werten(w: &[Wert]) -> Vec<u8> {
        let mut b = Vec::new();
        for wert in w {
            match wert {
                Wert::F64(v) => b.extend_from_slice(&v.to_le_bytes()),
                Wert::F32(v) => b.extend_from_slice(&v.to_le_bytes()),
                Wert::I32(v) => b.extend_from_slice(&v.to_le_bytes()),
            }
        }
        b
    }

    /// Der PAVEMENT-Satz kommt GETRENNT — und wird richtig zugeordnet.
    ///
    /// ⚠ Dieser Test bildet den echten Ablauf nach: erst der flache
    /// Bahnsatz (56 Bytes), dann zwei eigene PAVEMENT-Nachrichten
    /// (je 12 Bytes). Der erste Anlauf am 30.08.2026 hat die sechs
    /// Felder in den Bahnsatz gehaengt und seinen 80-Byte-Block im Test
    /// SELBST gebaut — damit bestaetigte der gruene Test nur die falsche
    /// Annahme. `PAVEMENT` ist eine eigene Satzart
    /// (`SIMCONNECT_FACILITY_DATA_PAVEMENT`, SDK-Header Zeile 338).
    #[test]
    fn pavement_saetze_kommen_getrennt() {
        let mut bahnen: Vec<SzenerieBahn> =
            bahn_aus_werten(&eddh_werte()).expect("Bahnenpaar").into();

        // Ohne PAVEMENT-Satz wird nichts behauptet.
        assert!(bahnen[0].versetzte_schwelle_m.is_nan());

        // PRIMARY_THRESHOLD: 120 m, aktiv.
        assert!(pavement_anhaengen(
            &mut bahnen,
            0,
            &pavement_bytes(120.0, 45.0, 1)
        ));
        assert!((bahnen[0].versetzte_schwelle_m - 120.0).abs() < 1e-6);
        assert!(
            bahnen[1].versetzte_schwelle_m.is_nan(),
            "das andere Ende wurde mit angefasst"
        );

        // SECONDARY_THRESHOLD: abgeschaltet → 0,0. Das ist die AUSSAGE
        // "keine versetzte Schwelle" und muss den Navdaten-Wert
        // schlagen, nicht NaN (das waere Schweigen).
        assert!(pavement_anhaengen(
            &mut bahnen,
            1,
            &pavement_bytes(0.0, 0.0, 0)
        ));
        assert_eq!(bahnen[1].versetzte_schwelle_m, 0.0);
    }

    /// Eine aktive Schwelle mit Unsinns-Laenge sagt lieber nichts.
    #[test]
    fn aktive_schwelle_ohne_brauchbare_laenge_schweigt() {
        let mut bahnen: Vec<SzenerieBahn> =
            bahn_aus_werten(&eddh_werte()).expect("Bahnenpaar").into();
        assert!(pavement_anhaengen(
            &mut bahnen,
            0,
            &pavement_bytes(-1.0, 45.0, 1)
        ));
        assert!(bahnen[0].versetzte_schwelle_m.is_nan());
    }

    /// Ein PAVEMENT-Satz ohne Bahn davor darf nichts anfassen.
    ///
    /// ⚠ Sonst haengt die Schwelle der zweiten Bahn am falschen Ende der
    /// ersten — ein Fehler, der erst 573 m spaeter auffaellt.
    #[test]
    fn pavement_ohne_bahn_wird_verworfen() {
        let mut leer: Vec<SzenerieBahn> = Vec::new();
        assert!(!pavement_anhaengen(
            &mut leer,
            0,
            &pavement_bytes(120.0, 45.0, 1)
        ));

        // Und ein dritter Satz zu einem Paar ebenfalls: Es gibt nur
        // zwei Enden.
        let mut bahnen: Vec<SzenerieBahn> =
            bahn_aus_werten(&eddh_werte()).expect("Bahnenpaar").into();
        assert!(!pavement_anhaengen(
            &mut bahnen,
            2,
            &pavement_bytes(120.0, 45.0, 1)
        ));
        assert!(bahnen[0].versetzte_schwelle_m.is_nan());
        assert!(bahnen[1].versetzte_schwelle_m.is_nan());
    }

    /// Die Bytes eines PAVEMENT-Satzes, so wie der Simulator sie schickt.
    fn pavement_bytes(laenge_m: f32, breite_m: f32, enable: i32) -> Vec<u8> {
        let mut b = Vec::with_capacity(12);
        b.extend_from_slice(&laenge_m.to_le_bytes());
        b.extend_from_slice(&breite_m.to_le_bytes());
        b.extend_from_slice(&enable.to_le_bytes());
        b
    }

    #[test]
    fn ein_verschobenes_feld_faellt_auf() {
        // Gegenprobe zur Gegenprobe: Wenn die Definition um ein Feld
        // verschoben ist, muessen die Werte NICHT mehr stimmen. Sonst
        // prueft der Test oben nichts.
        let werte = eddh_werte();
        let bytes = baue(BAHN_FELDER, &werte);
        let verschoben: Vec<(&str, FeldTyp)> = std::iter::once(("EXTRA", FeldTyp::I32))
            .chain(BAHN_FELDER.iter().copied())
            .collect();
        let zurueck = zerlege(&verschoben, &bytes);
        match zurueck {
            None => {}
            Some(v) => assert_ne!(
                v[1].als_f64(),
                53.6304,
                "verschobene Definition liefert trotzdem den richtigen Wert — \
                 dann prueft der Round-Trip-Test nichts"
            ),
        }
    }
}

#[cfg(test)]
mod verdrahtung_tests {
    //! ⚠ Prüfungen über den Quelltext des Windows-Teils.
    //!
    //! `adapter.rs` wird auf macOS und Linux gar nicht übersetzt — dort
    //! kann kein normaler Test hineinschauen. Ein Quelltext-Vergleich
    //! kann es, und er fängt genau die Fehler, die sonst erst auf einer
    //! Windows-Maschine auffallen würden.
    //!
    //! Das ersetzt die CI nicht. Es verkürzt nur die Schleife.

    const ADAPTER: &str = include_str!("adapter.rs");

    fn ohne_leerraum(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    fn die_definition_benutzt_die_feldliste() {
        // Eine von Hand abgeschriebene Liste im Adapter waere eine
        // zweite Wahrheit — und sie würde beim ersten Zusatzfeld
        // auseinanderlaufen, ohne dass etwas anschlägt.
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("facility::BAHN_DEFINITION"),
            "register_facility baut die Definition nicht aus BAHN_DEFINITION"
        );
        // ⚠ Und NICHT aus dem Byte-Raster: `BAHN_FELDER` kennt die
        // OPEN/CLOSE-Marken der PAVEMENT-Untersaetze nicht. Wer es hier
        // einsetzt, fordert die versetzte Schwelle gar nicht erst an.
        assert!(
            !a.contains("facility::BAHN_FELDER.iter()"),
            "die Anmeldung benutzt das Byte-Raster statt der Definition"
        );
    }

    /// Der Verteiler muss die eigene Satzart auch wirklich behandeln.
    ///
    /// ⚠ Eine angeforderte, aber nie ausgewertete Satzart faellt nicht
    /// auf: Die Lieferung kommt vollstaendig an, der Zweig fehlt, und
    /// die versetzte Schwelle bleibt einfach leer.
    #[test]
    fn der_verteiler_behandelt_pavement() {
        let a = ohne_leerraum(ADAPTER);
        let nadel = ohne_leerraum(&format!("sys::FACILITY_DATA_{}", "PAVEMENT"));
        assert!(
            a.contains(&nadel),
            "der Verteiler kennt die PAVEMENT-Satzart nicht — die \
             versetzte Schwelle kaeme nie an"
        );
    }

    #[test]
    fn die_klammern_der_definition_stehen_vollstaendig() {
        // Ohne OPEN/CLOSE-Paare liefert SimConnect nichts oder etwas
        // anderes, als man erwartet — und die Antwort sähe leer aus,
        // nicht falsch.
        for marke in [
            "\"OPEN AIRPORT\"",
            "\"OPEN RUNWAY\"",
            "\"CLOSE RUNWAY\"",
            "\"CLOSE AIRPORT\"",
        ] {
            assert!(
                ADAPTER.contains(marke),
                "{marke} fehlt in der Facility-Definition"
            );
        }
    }

    #[test]
    fn ein_abgelehntes_feld_ist_ein_harter_fehler() {
        // Fehlt WIDTH, kommt die Bahn ohne Breite zurueck — und die
        // Breite ist genau das Mass, mit dem entschieden wird, ob eine
        // Rollspur die befestigte Flaeche verlaesst. Ein „best effort"
        // waere hier ein stiller Datenverlust.
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("AddToFacilityDefinitionfuer"),
            "kein Fehlertext fuer ein abgelehntes Feld — dann faellt ein \
             falscher Feldname nur im Log auf, wenn ueberhaupt"
        );
        assert!(
            a.contains("returnErr(format!(\"AddToFacilityDefinition"),
            "abgelehntes Feld wird nicht als Fehler zurueckgegeben"
        );
    }

    #[test]
    fn die_lieferung_wird_erst_am_ende_sichtbar() {
        // ⚠ Die Antworten kommen stueckweise. Wuerde der Sammler schon
        // zwischendurch veroeffentlicht, saehe ein Flughafen mit sechs
        // Bahnen zeitweise aus wie einer mit einer — und die Bewertung
        // maesse gegen die falsche, ohne dass etwas anschlaegt.
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("shared.szenerie.lock().geliefert_zu_kennung("),
            "die Szenerie wird nirgends veroeffentlicht — oder nicht ueber \
             die Kennung, dann traegt eine verspaetete Antwort den Namen \
             des laufenden Auftrags"
        );
        // Die Veroeffentlichung muss im ENDE-Zweig stehen, nicht im
        // Element-Zweig.
        let ende = a
            .find("DispatchMsg::FacilityDataEnde")
            .expect("Ende-Zweig fehlt");
        let veroeffentlichung = a
            .find("shared.szenerie.lock().geliefert_zu_kennung(")
            .expect("Veroeffentlichung fehlt");
        assert!(
            veroeffentlichung > ende,
            "die Szenerie wird veroeffentlicht, bevor die Lieferung vollstaendig ist"
        );
    }

    #[test]
    fn die_facility_hat_eigene_kennungen() {
        // Teilte sie sich die Kennung mit der Telemetrie, wuerde ein
        // abgelehnter Feldname deren Layout verschieben.
        assert!(ADAPTER.contains("FACILITY_DEFINITION_ID"));
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("FACILITY_DEFINITION_ID:sys::SIMCONNECT_DATA_DEFINITION_ID=10"),
            "Facility-Definition benutzt nicht ihre eigene Kennung"
        );
        // ⚠ Und je VERSUCH eine eigene Anfragekennung, nicht eine feste
        // fuer alle. Mit einer festen liess sich eine nach der
        // Wartezeit eintreffende Antwort nicht mehr von der laufenden
        // unterscheiden (QS-Befund 1, 01.09.2026).
        assert!(
            !ADAPTER.contains("FACILITY_REQUEST_ID"),
            "die feste Anfragekennung ist zurueck — damit lassen sich \
             verspaetete Antworten nicht mehr zuordnen"
        );
        assert!(
            a.contains("FACILITY_REQUEST_BASE+auftrag_id"),
            "die Anfragekennung wird nicht je Versuch gebildet"
        );
    }
}

/// Einen Namen aus einem `TAXI_NAME`-Block lesen.
///
/// SimConnect liefert Zeichenketten null-terminiert. Alles hinter der
/// ersten Null gehoert nicht dazu — wer den ganzen Puffer nimmt,
/// bekommt Namen mit Fuellbytes daran.
pub fn name_aus_bytes(bytes: &[u8]) -> String {
    let ende = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..ende]).trim().to_string()
}

/// Die drei Rollweg-Listen zu benannten Strecken zusammensetzen.
///
/// # Warum das eine eigene Funktion ist
///
/// Weil hier drei Indexräume aufeinandertreffen: Kanten verweisen über
/// `START`/`END` auf Punkte und über `NAME_INDEX` auf Namen. Ein
/// Index daneben ergibt eine Strecke, die es nicht gibt — mit
/// plausiblen Koordinaten.
///
/// Kanten ohne Namen fallen weg: Für die Frage „auf welcher Ausfahrt
/// bin ich abgerollt?" tragen sie nichts bei.
pub fn rollwege_zusammensetzen(
    referenz: (f64, f64),
    punkte_versatz: &[(f64, f64)],
    namen: &[String],
    kanten: &[(usize, usize, usize)],
) -> Vec<sim_core::szenerie::SzenerieRollweg> {
    let punkte: Vec<(f64, f64)> = punkte_versatz
        .iter()
        .map(|(ost, nord)| punkt_aus_versatz(referenz, *ost, *nord))
        .collect();
    let mut aus = Vec::new();
    for (a, b, n) in kanten {
        let (Some(pa), Some(pb)) = (punkte.get(*a), punkte.get(*b)) else {
            continue;
        };
        let Some(name) = namen.get(*n) else { continue };
        if name.is_empty() {
            continue;
        }
        aus.push(sim_core::szenerie::SzenerieRollweg {
            name: name.clone(),
            punkte: vec![*pa, *pb],
        });
    }
    aus
}

#[cfg(test)]
mod rollweg_tests {
    use super::*;

    #[test]
    fn namen_enden_an_der_null() {
        // SimConnect liefert Zeichenketten null-terminiert. Wer den
        // ganzen Puffer nimmt, bekommt Namen mit Fuellbytes daran — und
        // „B3\0\0\0" ist nicht „B3".
        assert_eq!(name_aus_bytes(b"B3\0\0\0\0"), "B3");
        assert_eq!(name_aus_bytes(b"TWY ALPHA\0"), "TWY ALPHA");
        assert_eq!(name_aus_bytes(b""), "");
        assert_eq!(name_aus_bytes(b"  A1  \0"), "A1");
    }

    #[test]
    fn kanten_werden_ueber_indizes_verknuepft() {
        let referenz = (53.6304, 9.9882);
        let punkte = vec![(0.0, 0.0), (500.0, 0.0), (500.0, 500.0)];
        let namen = vec!["A".to_string(), "B3".to_string()];
        let kanten = vec![(0usize, 1usize, 1usize), (1, 2, 0)];
        let r = rollwege_zusammensetzen(referenz, &punkte, &namen, &kanten);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].name, "B3");
        assert_eq!(r[1].name, "A");
        // Erster Punkt der ersten Kante ist der Referenzpunkt selbst.
        assert!((r[0].punkte[0].0 - referenz.0).abs() < 1e-9);
        assert!((r[0].punkte[0].1 - referenz.1).abs() < 1e-9);
        // Und der zweite liegt oestlich davon.
        assert!(r[0].punkte[1].1 > referenz.1);
    }

    #[test]
    fn ein_index_daneben_erzeugt_keine_strecke() {
        // ⚠ Drei Indexraeume treffen aufeinander. Ein Index daneben
        // ergaebe eine Strecke, die es nicht gibt — mit plausiblen
        // Koordinaten. Lieber nichts als etwas Erfundenes.
        let referenz = (53.6304, 9.9882);
        let punkte = vec![(0.0, 0.0), (500.0, 0.0)];
        let namen = vec!["A".to_string()];
        for kante in [(0usize, 9usize, 0usize), (9, 0, 0), (0, 1, 9)] {
            let r = rollwege_zusammensetzen(referenz, &punkte, &namen, &[kante]);
            assert!(
                r.is_empty(),
                "Kante {kante:?} haette verworfen werden muessen"
            );
        }
    }

    #[test]
    fn namenlose_kanten_fallen_weg() {
        // Fuer die Frage „auf welcher Ausfahrt bin ich abgerollt?"
        // tragen sie nichts bei, und eine leere Beschriftung in der
        // Anzeige ist schlechter als keine Strecke.
        let r = rollwege_zusammensetzen(
            (53.6304, 9.9882),
            &[(0.0, 0.0), (100.0, 0.0)],
            &["".to_string()],
            &[(0, 1, 0)],
        );
        assert!(r.is_empty());
    }

    #[test]
    fn text_gehoert_nicht_ins_feste_raster() {
        // Eine Zeichenkette hat keine feste Groesse. Wuerde `zerlege`
        // sie mitrechnen, verschoebe sich alles danach.
        let felder: &[(&str, FeldTyp)] = &[("NAME", FeldTyp::Text)];
        assert!(zerlege(felder, b"B3\0").is_none());
    }
}

#[cfg(test)]
mod anschluss_verdrahtung_tests {
    //! Wachen ueber den Windows-Teil, die auch ohne Windows laufen.

    const ADAPTER: &str = include_str!("adapter.rs");

    fn ohne_leerraum(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    fn die_definition_wird_auch_registriert() {
        // ⚠ Genau die Luecke, die ich gebaut hatte: `register_facility`
        // war definiert und nirgends aufgerufen. Ohne den Aufruf gaebe
        // es die Definition nicht — und jede Anfrage liefe ins Leere,
        // OHNE Fehler: `RequestFacilityData` scheitert dann nicht, es
        // kommt nur nie eine Antwort.
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("conn.register_facility()"),
            "register_facility wird nirgends aufgerufen"
        );
    }

    #[test]
    fn die_anfrage_laeuft_im_verbindungsfaden() {
        // `szenerie_anfordern` laeuft im Aufrufer-Faden und darf
        // SimConnect nicht anfassen. Der Griff gehoert dem Faden.
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("conn.request_facility(&icao,request_id)"),
            "die Anfrage wird nicht im Verbindungsfaden gestellt — oder \
             ohne eigene Kennung, dann ist sie nicht zuzuordnen"
        );
        // ⚠ Seit v1.7.14 kommt der Auftrag aus dem Buch, nicht aus einer
        // Flagge. Die Flagge sagte nur "jemand hat etwas angemeldet" —
        // eine Wiederholung nach ausgebliebener Antwort haette sie nie
        // ausgeloest, und genau daran ist EDDF-LEZL gescheitert.
        assert!(
            a.contains("shared.szenerie.lock().naechsten_stellen(jetzt_ms)"),
            "der faellige Auftrag wird nicht aus dem Buch geholt — dann \
             haengt die Anfrage wieder an einer Flagge und wird nie \
             wiederholt"
        );
        // ⚠ Seit der fuenften Runde in EINEM Zug: `naechsten_stellen`.
        // Getrennt waere es ein Riss — dazwischen kann ein Flugwechsel
        // das Buch leeren, und der Auftrag entstuende fuer einen Platz,
        // den es nicht mehr gibt.
        assert!(
            !a.contains("shared.szenerie.lock().gestellt("),
            "Auswaehlen und Stellen sind wieder getrennt"
        );
    }

    /// ⚠ Eine neue Verbindung schneidet den Anfragezustand ab.
    ///
    /// Ohne das gaelte nach einem Simulator-Neustart oder einem Wechsel
    /// zwischen MSFS 2020 und 2024 die Szenerie der VORIGEN Verbindung
    /// als „geliefert" und wuerde nie erneut angefordert; verbrauchte
    /// Versuche und dauerhafte Ablehnungen ueberdauerten ebenfalls
    /// (QS-Befund 2, dritte Runde).
    #[test]
    fn eine_neue_verbindung_leert_das_auftragsbuch() {
        // ⚠ NICHT einfach die erste Fundstelle nehmen: Der Adapter
        // hat weiter oben eine Methode gleichen Namens. Gesucht ist die
        // AUFRUFSTELLE im Verbindungszweig — also eine Fundstelle
        // DAHINTER, und zwar dicht dahinter.
        let a = ohne_leerraum(ADAPTER);
        let oeffnen = a
            .find("SimConnect_Opensucceeded")
            .expect("Verbindungszweig fehlt");
        let danach = &a[oeffnen..];
        // ⚠ `verbindung_zuruecksetzen` — der Flug-Schnitt waere hier
        // falsch, er behaelt den Definitionsfehler.
        let leeren = danach
            .find("shared.szenerie.lock().verbindung_zuruecksetzen()")
            .expect("das Buch wird im Verbindungszweig nicht geleert");
        assert!(
            leeren < 600,
            "das Leeren steht {leeren} Zeichen hinter dem Verbindungsaufbau — \
             das ist nicht mehr derselbe Zweig"
        );
    }

    /// ⚠ Auch der SYNCHRONE Fehlschlag schliesst den Weg.
    ///
    /// `register_facility()` kann sofort scheitern. Vorher wurde dann
    /// nur gewarnt: Die Definition war nachweislich nicht registriert,
    /// das Buch wusste nichts davon und stellte weiter Anfragen. Der
    /// harte Riegel griff ausschliesslich bei den spaeteren,
    /// asynchronen Ausnahmen (QS-Befund 3, vierte Runde).
    #[test]
    fn auch_ein_sofortiger_definitionsfehler_schliesst_den_weg() {
        let a = ohne_leerraum(ADAPTER);
        let fehlschlag = a
            .find("Err(e)=conn.register_facility()")
            .expect("Aufruf von register_facility fehlt");
        let danach = &a[fehlschlag..];
        let riegel = danach.find("definition_abgelehnt(").expect(
            "der sofortige Fehlschlag erreicht das Buch nicht — \
                     es fragt weiter mit einer Definition, die es nicht gibt",
        );
        assert!(
            riegel < 700,
            "der Riegel steht {riegel} Zeichen hinter dem Fehlschlag — das \
             ist nicht mehr derselbe Zweig"
        );
    }

    /// ⚠ Und eine neue VERBINDUNG loescht den Definitionsfehler, ein
    /// blosser Flugwechsel nicht.
    ///
    /// Die Felddefinition wird je Verbindung registriert. Loeschte ein
    /// Flugwechsel den Fehler, fraege dieselbe Verbindung mit derselben
    /// abgelehnten Definition weiter (QS-Befund 2, vierte Runde).
    #[test]
    fn die_verbindung_setzt_anders_zurueck_als_der_flug() {
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("shared.szenerie.lock().verbindung_zuruecksetzen()"),
            "die neue Verbindung raeumt den Definitionsfehler nicht weg"
        );
        assert!(
            a.contains(
                "pubfnszenerie_zuruecksetzen(&self){self.shared.szenerie.lock().zuruecksetzen()"
            ),
            "der Flug-Schnitt benutzt nicht den flugweiten Weg — dann \
             verliert ein Flugwechsel den Definitionsfehler der Verbindung"
        );
    }

    /// ⚠ Ein abgelehntes Feld schliesst den Weg — hart, nicht weich.
    ///
    /// Vorher stand im Ausnahmezweig nur ein `continue`: Der Feldfehler
    /// war erkannt und benannt, der Facility-Weg lief weiter, und
    /// Auftraege meldeten „unterwegs" oder „geliefert" (QS-Befund 4,
    /// dritte Runde).
    #[test]
    fn ein_abgelehntes_feld_stoppt_den_facility_weg() {
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("shared.szenerie.lock().definition_abgelehnt(feld,"),
            "der Feldfehler wird nur protokolliert — der Weg laeuft weiter"
        );
    }

    /// ⚠ Dieser Waechter stand bis v1.7.13 auf dem Kopf.
    ///
    /// Er verlangte, dass eine Anmeldung die vorhandene Auskunft
    /// LOESCHT — mit der Begruendung, sonst wuerde nach einem Divert die
    /// Bahn des geplanten Ziels benutzt. Die Sorge war richtig, das
    /// Mittel falsch: Weil Start und Ziel beim Flugbeginn nacheinander
    /// angemeldet werden, loeschte die zweite Anmeldung die Antwort der
    /// ersten. Am 01.09.2026 (EDDF→LEZL) lag beim Aufsetzen in Sevilla
    /// die Szenerie Frankfurts vor.
    ///
    /// Richtig ist die Trennung nach Platz: Jeder behaelt seine eigene
    /// Antwort, und herausgegeben wird nur die des gefragten Platzes.
    /// Das leistet `Auftragsbuch::auskunft` — dort steht auch der
    /// Verhaltenstest dazu (`kein_rueckfall_auf_irgendeinen_platz`).
    /// Hier bleibt nur, dass das alte Mittel nicht zurueckkommt.
    #[test]
    fn eine_anmeldung_loescht_keine_fremde_auskunft() {
        let a = ohne_leerraum(ADAPTER);
        assert!(
            !a.contains("*self.shared.szenerie.lock()=None;"),
            "die Anmeldung loescht wieder das ganze Fach — damit \
             verdraengen sich Start und Ziel gegenseitig (EDDF-LEZL, \
             01.09.2026)"
        );
        assert!(
            a.contains("self.shared.szenerie.lock().wunsch_mit_rang(&icao,rang)"),
            "die Anmeldung geht nicht ins Buch"
        );
    }

    #[test]
    fn die_rollwege_stehen_in_der_definition() {
        for marke in [
            "\"OPEN TAXI_POINT\"",
            "\"CLOSE TAXI_POINT\"",
            "\"OPEN TAXI_NAME\"",
            "\"OPEN TAXI_PATH\"",
        ] {
            assert!(ADAPTER.contains(marke), "{marke} fehlt in der Definition");
        }
        // Und der Referenzpunkt, ohne den die Punkte nicht umrechenbar
        // sind.
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("facility::FLUGHAFEN_FELDER"),
            "ohne den Referenzpunkt sind BIAS_X/BIAS_Z nutzlos"
        );
    }
}

#[cfg(test)]
mod rollweg_verdrahtung_tests {
    //! Wachen über den Rollweg-Einsammler im Windows-Teil.

    const ADAPTER: &str = include_str!("adapter.rs");

    fn ohne_leerraum(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    fn alle_drei_listen_werden_eingesammelt() {
        let a = ohne_leerraum(ADAPTER);
        for marke in [
            "sys::FACILITY_DATA_AIRPORT",
            "sys::FACILITY_DATA_TAXI_POINT",
            "sys::FACILITY_DATA_TAXI_NAME",
            "sys::FACILITY_DATA_TAXI_PATH",
        ] {
            assert!(
                a.contains(&ohne_leerraum(marke)),
                "{marke} wird nicht eingesammelt — die Rollwege blieben leer"
            );
        }
    }

    #[test]
    fn ein_unlesbarer_punkt_belegt_trotzdem_seinen_platz() {
        // ⚠ `START`/`END` sind POSITIONEN in der Punktliste. Wer einen
        // unlesbaren Eintrag auslaesst, verschiebt jede Kante danach auf
        // einen anderen Punkt — und heraus kommen Rollwege, die es gibt,
        // nur woanders.
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("None=>lieferung.punkte.push((f64::NAN,f64::NAN))"),
            "ein unlesbarer Rollwegpunkt wird uebersprungen statt platzhaltend \
             eingefuegt — die Indizes verschieben sich"
        );
    }

    #[test]
    fn zusammengesetzt_wird_erst_am_ende() {
        // Vorher sind die drei Listen nicht vollstaendig: Eine Kante
        // koennte auf einen Punkt zeigen, der noch nicht da ist.
        let a = ohne_leerraum(ADAPTER);
        let ende = a
            .find("DispatchMsg::FacilityDataEnde")
            .expect("Ende-Zweig fehlt");
        let zusammenbau = a
            .find("facility::rollwege_zusammensetzen(")
            .expect("Zusammenbau fehlt");
        assert!(
            zusammenbau > ende,
            "die Rollwege werden zusammengesetzt, bevor die Lieferung vollstaendig ist"
        );
    }

    /// ⚠ Dieser Waechter prueft seit v1.7.14 eine ZUSICHERUNG statt
    /// eines Mittels.
    ///
    /// Vorher verlangte er drei `clear()`-Aufrufe am Ende des
    /// Ende-Zweigs. Das Mittel taugte nur, solange es EINE gemeinsame
    /// Liste gab — und genau die war der Fehler: Zwei Lieferungen, die
    /// sich zeitlich ueberlappen, mischten sich darin. Jetzt gehoert
    /// jede Liste ihrer Lieferung, und die Lieferung wird beim Ende
    /// herausgenommen. Damit KANN nichts ueberdauern.
    ///
    /// Die Zusicherung selbst — Listen einer Lieferung bleiben unter
    /// sich — liegt als Verhaltenstest in `lieferungen_tests`.
    #[test]
    fn keine_liste_ueberdauert_eine_lieferung() {
        let a = ohne_leerraum(ADAPTER);
        assert!(
            a.contains("facility_lieferungen.abschliessen(request_id)"),
            "die Lieferung wird am Ende nicht herausgenommen — ihre Listen \
             blieben stehen und der naechste Flughafen erbte sie"
        );
        for verboten in [
            "facility_punkte",
            "facility_namen",
            "facility_kanten",
            "facility_referenz",
        ] {
            assert!(
                !a.contains(verboten),
                "{verboten} ist wieder eine gemeinsame Liste — zwei sich \
                 ueberlappende Lieferungen mischen sich darin"
            );
        }
    }
}
