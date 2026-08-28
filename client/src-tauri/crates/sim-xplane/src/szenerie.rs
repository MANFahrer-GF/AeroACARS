//! Bahnen und Rollwege aus der **installierten X-Plane-Szenerie**.
//!
//! # Warum das gebaut wurde
//!
//! Die Landebewertung misst, wie weit das Flugzeug von der Mittellinie
//! entfernt war. Dafür muss feststehen, **wo die Mittellinie liegt** —
//! und diese Angabe kam bisher ausschliesslich aus den Navigationsdaten
//! (Navigraph). Das ist der echte Flughafen, vermessen zu einem AIRAC-
//! Stand. Der Pilot fliegt aber die **Szenerie**, und die ist ein Modell:
//! andere Vermessung, künstlerische Anpassungen, Add-ons.
//!
//! Am 28.08.2026 gegen die hier installierte Szenerie gemessen, über
//! 70.452 Bahnen, die in beiden Quellen stehen:
//!
//! ```text
//! Median der Abweichung          0,03°
//! ab 3° daneben              3.653 Bahnen   (davon 63 % Platzhalter-Kurse)
//! Breite ab 5 m daneben      7.279 Bahnen
//! schlimmster Fall             180°   — Bahn 17 mit Kurs 0,00° gefuehrt
//! ```
//!
//! Für die grosse Mehrheit sind unsere Daten ausgezeichnet. Der Rest ist
//! es nicht, und dort bewerten wir gegen eine Linie, die es im Simulator
//! nicht gibt.
//!
//! **Deshalb: erste Instanz ist der Simulator, die Navdaten sind der
//! Rückfall.** Für Bahnen, die X-Plane gar nicht kennt (57.136 im
//! Bestand), bleibt alles wie bisher.
//!
//! # Warum die Reihenfolge der Szenerie-Pakete zählt
//!
//! ⚠ Zusatz-Szenerien bringen ihre **eigene** `apt.dat` mit und
//! überschreiben die globale. Welche gilt, legt
//! `Custom Scenery/scenery_packs.ini` fest: **frühere Einträge haben
//! Vorrang**, und die globale Szenerie steht dort gar nicht drin — X-Plane
//! lädt sie stillschweigend als letzte.
//!
//! Wer nur die globale liest, bekommt für jeden Add-on-Flughafen die
//! falsche Bahn. Hier nachgemessen an EGPR (Barra): Zusatzszenerie
//! 140,07°, global 139,62° — und das ist ein X-Plane-eigenes Paket.
//! Fremde Add-ons weichen weiter ab.
//!
//! # Format
//!
//! ```text
//! 1  145 0 0 FACT Cape Town Intl          ← Flughafenkopf (auch 16/17)
//! 100 61.00 1 ... 01 -33.96 18.60 0 ... 19 -33.99 18.61 0 ...   ← Landbahn
//! 1201 53.62688 9.98477 both 0 D7_stop    ← Rollweg-Knoten
//! 1202 0 118 oneway taxiway_C D7          ← Rollweg-Kante mit Name
//! ```
//!
//! ⚠ Die `apt.dat` führt **keinen Kurs**. Er wird aus den beiden
//! Schwellenkoordinaten gerechnet — Grosskreis, nicht ebene Näherung.
//!
//! ⚠ Das Typfeld der Kante heisst `taxiway_C`/`taxiway_F` (ICAO-Breiten-
//! klasse), NICHT `taxiway`. Wer auf Gleichheit prüft statt auf den
//! Anfang, misst überall null benannte Kanten.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Ein Bahnende, so wie die Szenerie es beschreibt.
#[derive(Debug, Clone, PartialEq)]
pub struct SzenerieBahn {
    /// Bezeichner dieses Endes, etwa `"27R"`.
    pub bezeichner: String,
    /// Wahrer Kurs in Grad, aus den Schwellen gerechnet.
    pub kurs_grad: f64,
    /// Breite der befestigten Fläche in Metern.
    pub breite_m: f64,
    /// Länge zwischen den beiden Schwellen in Metern.
    pub laenge_m: f64,
    /// Versetzte Schwelle an diesem Ende, in Metern.
    pub versetzte_schwelle_m: f64,
    /// Koordinaten dieses Endes.
    pub schwelle: (f64, f64),
    /// Koordinaten des gegenüberliegenden Endes.
    pub gegenende: (f64, f64),
    /// Belagsschlüssel der `apt.dat` (1 = Asphalt, 2 = Beton, …).
    pub belag_code: u8,
}

/// Ein benanntes Rollwegstück.
#[derive(Debug, Clone, PartialEq)]
pub struct SzenerieRollweg {
    pub name: String,
    pub punkte: Vec<(f64, f64)>,
}

/// Was ein Flughafen in der Szenerie hergibt.
#[derive(Debug, Clone, Default)]
pub struct SzenerieFlughafen {
    pub icao: String,
    pub bahnen: Vec<SzenerieBahn>,
    pub rollwege: Vec<SzenerieRollweg>,
    /// Aus welchem Paket die Angaben stammen — für den Bericht und die
    /// Fehlersuche. Ein Add-on-Flughafen sieht anders aus als der globale.
    pub quelle: String,
}

/// Wo X-Plane installiert ist.
///
/// X-Plane legt die Pfade selbst ab; genau diese Datei benutzen auch die
/// Installationsprogramme der Add-ons. Damit findet der Client die
/// Szenerie, **ohne dass der Simulator läuft** — die Daten liegen auf der
/// Platte, nicht im Speicher.
pub fn installationen() -> Vec<PathBuf> {
    let mut kandidaten: Vec<PathBuf> = Vec::new();
    for name in ["x-plane_install_12.txt", "x-plane_install_11.txt"] {
        for basis in ablage_orte() {
            let p = basis.join(name);
            let Ok(inhalt) = std::fs::read_to_string(&p) else {
                continue;
            };
            for zeile in inhalt.lines() {
                let z = zeile.trim();
                if z.is_empty() {
                    continue;
                }
                let pfad = PathBuf::from(z);
                if pfad.is_dir() && !kandidaten.contains(&pfad) {
                    kandidaten.push(pfad);
                }
            }
        }
    }
    kandidaten
}

fn ablage_orte() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(h) = std::env::var_os("HOME").map(PathBuf::from) {
        v.push(h.join("Library/Preferences")); // macOS
        v.push(h.join(".x-plane")); // Linux
    }
    if let Some(l) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        v.push(l); // Windows
    }
    v
}

/// Die `apt.dat`-Dateien einer Installation, **in der Rangfolge, die
/// X-Plane anwendet**.
///
/// Zuerst die Zusatz-Szenerien in der Reihenfolge von
/// `scenery_packs.ini`, danach die globale Szenerie. Deaktivierte Pakete
/// (`SCENERY_PACK_DISABLED`) werden übersprungen — sie sind für X-Plane
/// nicht vorhanden und dürfen es hier auch nicht sein.
pub fn apt_dateien_in_rangfolge(wurzel: &Path) -> Vec<PathBuf> {
    let mut aus = Vec::new();
    let ini = wurzel.join("Custom Scenery/scenery_packs.ini");
    if let Ok(inhalt) = std::fs::read_to_string(&ini) {
        for zeile in inhalt.lines() {
            let z = zeile.trim();
            let Some(rest) = z.strip_prefix("SCENERY_PACK ") else {
                continue; // auch SCENERY_PACK_DISABLED faellt hier heraus
            };
            let rel = rest.trim().trim_end_matches('/');
            if rel.is_empty() {
                continue;
            }
            let p = wurzel.join(rel).join("Earth nav data/apt.dat");
            if p.is_file() {
                aus.push(p);
            }
        }
    }
    // Die globale Szenerie steht NICHT in der ini — X-Plane laedt sie
    // implizit als letzte. Genau so hier.
    for global in [
        "Global Scenery/Global Airports/Earth nav data/apt.dat",
        "Global Scenery/X-Plane Airports/Earth nav data/apt.dat",
    ] {
        let p = wurzel.join(global);
        if p.is_file() {
            aus.push(p);
        }
    }
    aus
}

/// Grosskreis-Kurs von A nach B, in Grad.
fn kurs_grad(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (p1, p2) = (a.0.to_radians(), b.0.to_radians());
    let dl = (b.1 - a.1).to_radians();
    let y = dl.sin() * p2.cos();
    let x = p1.cos() * p2.sin() - p1.sin() * p2.cos() * dl.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Grosskreis-Abstand in Metern.
fn abstand_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R: f64 = 6_371_000.0;
    let (p1, p2) = (a.0.to_radians(), b.0.to_radians());
    let dp = p2 - p1;
    let dl = (b.1 - a.1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Einen Flughafen aus **einer** `apt.dat` lesen. `None` = nicht enthalten.
///
/// Streamend, weil die globale `apt.dat` rund 380 MB gross ist: Es wird
/// nur der eine Abschnitt gelesen, danach abgebrochen.
pub fn lies_flughafen(datei: &Path, icao: &str) -> Option<SzenerieFlughafen> {
    let f = File::open(datei).ok()?;
    lies_aus_strom(BufReader::with_capacity(1 << 16, f), datei, icao)
}

/// Der gemeinsame Kern — einmal fuer den vollen Durchlauf, einmal fuer
/// den Sprung ins Verzeichnis. Zwei Fassungen davon waeren zwei
/// Fassungen der Format-Kenntnis.
fn lies_aus_strom<R: BufRead>(leser: R, datei: &Path, icao: &str) -> Option<SzenerieFlughafen> {
    let mut im_platz = false;
    let mut aus = SzenerieFlughafen {
        icao: icao.to_string(),
        quelle: datei.display().to_string(),
        ..Default::default()
    };
    let mut knoten: std::collections::HashMap<String, (f64, f64)> =
        std::collections::HashMap::new();

    for zeile in leser.lines() {
        let Ok(zeile) = zeile else { break };
        let t: Vec<&str> = zeile.split_whitespace().collect();
        let Some(&kopf) = t.first() else { continue };

        // 1 = Landflughafen, 16 = Wasserflughafen, 17 = Hubschrauberplatz.
        if matches!(kopf, "1" | "16" | "17") {
            if im_platz {
                break; // der naechste Platz beginnt — wir sind fertig
            }
            im_platz = t.get(4).is_some_and(|c| c.eq_ignore_ascii_case(icao));
            continue;
        }
        if !im_platz {
            continue;
        }

        match kopf {
            "100" if t.len() >= 26 => {
                let (Ok(breite), Ok(belag)) = (t[1].parse::<f64>(), t[2].parse::<u8>()) else {
                    continue;
                };
                let lese_ende = |i: usize| -> Option<(String, (f64, f64), f64)> {
                    Some((
                        t.get(i)?.to_string(),
                        (t.get(i + 1)?.parse().ok()?, t.get(i + 2)?.parse().ok()?),
                        t.get(i + 3)?.parse().ok()?,
                    ))
                };
                let (Some((n1, s1, v1)), Some((n2, s2, v2))) = (lese_ende(8), lese_ende(17)) else {
                    continue;
                };
                let k = kurs_grad(s1, s2);
                let l = abstand_m(s1, s2);
                aus.bahnen.push(SzenerieBahn {
                    bezeichner: n1,
                    kurs_grad: k,
                    breite_m: breite,
                    laenge_m: l,
                    versetzte_schwelle_m: v1,
                    schwelle: s1,
                    gegenende: s2,
                    belag_code: belag,
                });
                aus.bahnen.push(SzenerieBahn {
                    bezeichner: n2,
                    kurs_grad: (k + 180.0) % 360.0,
                    breite_m: breite,
                    laenge_m: l,
                    versetzte_schwelle_m: v2,
                    schwelle: s2,
                    gegenende: s1,
                    belag_code: belag,
                });
            }
            "1201" if t.len() >= 5 => {
                if let (Ok(lat), Ok(lon)) = (t[1].parse::<f64>(), t[2].parse::<f64>()) {
                    knoten.insert(t[4].to_string(), (lon, lat));
                }
            }
            "1202" if t.len() >= 6 => {
                // ⚠ `starts_with`, nicht `==`: das Feld traegt die
                // ICAO-Breitenklasse (`taxiway_C`).
                if !t[4].starts_with("taxiway") {
                    continue;
                }
                let name = t[5..].join(" ").trim().to_string();
                if name.is_empty() {
                    continue;
                }
                if let (Some(a), Some(b)) = (knoten.get(t[1]), knoten.get(t[2])) {
                    aus.rollwege.push(SzenerieRollweg {
                        name,
                        punkte: vec![*a, *b],
                    });
                }
            }
            _ => {}
        }
    }

    if aus.bahnen.is_empty() && aus.rollwege.is_empty() {
        None
    } else {
        Some(aus)
    }
}

/// Den Flughafen aus der Szenerie holen — **das erste Paket gewinnt**.
pub fn flughafen(icao: &str) -> Option<SzenerieFlughafen> {
    for wurzel in installationen() {
        for datei in apt_dateien_in_rangfolge(&wurzel) {
            if let Some(f) = lies_flughafen(&datei, icao) {
                return Some(f);
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────
// Verzeichnis
// ─────────────────────────────────────────────────────────────────────

/// Wo welcher Flughafen in welcher Datei steht.
///
/// # Warum das nötig ist
///
/// Der streamende Leser braucht so lange, wie der Flughafen in der Datei
/// weit hinten steht. Am 28.08.2026 hier gemessen:
///
/// ```text
/// EGPR   0,9 ms   (liegt im ersten Zusatzpaket)
/// EDDH 970    ms
/// KJFK   6,8 s
/// FACT  25,2 s
/// ```
///
/// Fünfundzwanzig Sekunden beim Aufsetzen sind unbrauchbar — und die
/// Dauer hängt an etwas, das mit dem Flug nichts zu tun hat: der Position
/// des Platzes in einer 380-MB-Datei.
///
/// Ein vollständiger Durchlauf über alle Pakete kostet dagegen **919 ms**
/// und findet 38.883 Flughäfen. Also einmal durchgehen, die Byte-Position
/// jedes Platzes merken, danach springen.
///
/// ⚠ Beim Bauen gewinnt der **erste** Fund — das ist dieselbe Rangfolge,
/// die `apt_dateien_in_rangfolge` liefert, und damit die von X-Plane.
#[derive(Debug, Clone, Default)]
pub struct SzenerieIndex {
    eintraege: std::collections::HashMap<String, (PathBuf, u64)>,
    /// Datei, Grösse, Änderungszeit — woran erkannt wird, dass der Index
    /// veraltet ist. Installiert der Pilot ein Add-on, ändert sich beides.
    quellen: Vec<(PathBuf, u64, u64)>,
}

fn stempel(p: &Path) -> Option<(u64, u64)> {
    let m = std::fs::metadata(p).ok()?;
    let zeit = m
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some((m.len(), zeit))
}

impl SzenerieIndex {
    /// Alle Pakete einer Installation einlesen.
    pub fn bauen(wurzel: &Path) -> SzenerieIndex {
        let mut idx = SzenerieIndex::default();
        for datei in apt_dateien_in_rangfolge(wurzel) {
            if let Some((groesse, zeit)) = stempel(&datei) {
                idx.quellen.push((datei.clone(), groesse, zeit));
            }
            let Ok(f) = File::open(&datei) else { continue };
            let mut leser = BufReader::with_capacity(1 << 16, f);
            let mut pos: u64 = 0;
            let mut zeile = String::new();
            loop {
                zeile.clear();
                let gelesen = match leser.read_line(&mut zeile) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n as u64,
                };
                let anfang = pos;
                pos += gelesen;
                // Billig prüfen, bevor zerlegt wird: die Datei hat
                // Millionen Zeilen, und nur ~39.000 sind Flughafenköpfe.
                let ist_kopf =
                    zeile.starts_with("1 ") || zeile.starts_with("16 ") || zeile.starts_with("17 ");
                if !ist_kopf {
                    continue;
                }
                let Some(icao) = zeile.split_whitespace().nth(4) else {
                    continue;
                };
                // ⚠ `entry().or_insert()`, nicht `insert()`: Der erste
                // Fund gewinnt. Ein `insert` würde die globale Szenerie
                // das Zusatzpaket überschreiben lassen — genau falsch
                // herum.
                idx.eintraege
                    .entry(icao.to_ascii_uppercase())
                    .or_insert((datei.clone(), anfang));
            }
        }
        idx
    }

    /// Ist der Index noch gültig, oder hat sich die Szenerie geändert?
    pub fn gueltig(&self) -> bool {
        !self.quellen.is_empty()
            && self
                .quellen
                .iter()
                .all(|(p, g, z)| stempel(p) == Some((*g, *z)))
    }

    pub fn anzahl(&self) -> usize {
        self.eintraege.len()
    }

    /// Den Flughafen holen — Sprung an die gemerkte Stelle.
    pub fn flughafen(&self, icao: &str) -> Option<SzenerieFlughafen> {
        let (datei, pos) = self.eintraege.get(&icao.to_ascii_uppercase())?;
        lies_ab_position(datei, *pos, icao)
    }
}

/// Wie `lies_flughafen`, aber ab einer bekannten Stelle.
fn lies_ab_position(datei: &Path, pos: u64, icao: &str) -> Option<SzenerieFlughafen> {
    use std::io::Seek;
    let mut f = File::open(datei).ok()?;
    f.seek(std::io::SeekFrom::Start(pos)).ok()?;
    lies_aus_strom(BufReader::with_capacity(1 << 16, f), datei, icao)
}
