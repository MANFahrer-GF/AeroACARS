//! Prüfungen des Szenerie-Lesers.
//!
//! Zwei Teile, mit Absicht:
//!
//! * **Festwert-Teil** — läuft überall, auch in der CI, gegen einen
//!   kleinen `apt.dat`-Ausschnitt im Test selbst.
//! * **Teil gegen die echte Installation** — läuft nur, wo X-Plane liegt.
//!   Er ist der eigentliche Beweis: ein selbstgebauter Ausschnitt bestätigt
//!   nur, dass mein Parser meinen eigenen Ausschnitt versteht.
//!
//! ⚠ Der zweite Teil überspringt sich, wenn nichts installiert ist —
//! aber er meldet das. Ein Test, der still nichts prüft, ist schlimmer
//! als keiner.

use sim_xplane::szenerie::{apt_dateien_in_rangfolge, flughafen, installationen, lies_flughafen};
use std::io::Write;

/// Ein Ausschnitt im Format der echten Datei — zwei Bahnen, ein Rollweg.
const AUSSCHNITT: &str = "\
I
1100 Version
1     14 0 0 TEST Testplatz
100 45.11 1 0 0.25 1 0 0 09  53.6000  9.9000 0 0 2 0 0 1 27  53.6000 10.0000 0 0 2 0 0 1
1201 53.60100 9.95000 both 0 A_node
1201 53.60200 9.95100 both 1 B_node
1202 0 1 twoway taxiway_C B3
1202 0 1 twoway taxiway_C
1202 0 1 twoway runway 09
1     14 0 0 ZZZZ Danach
100 30.00 1 0 0.25 1 0 0 18  50.0000  8.0000 0 0 2 0 0 1 36  50.1000  8.0000 0 0 2 0 0 1
";

fn schreibe_ausschnitt() -> std::path::PathBuf {
    let p = std::env::temp_dir().join("aeroacars_szenerie_test_apt.dat");
    let mut f = std::fs::File::create(&p).expect("Testdatei");
    f.write_all(AUSSCHNITT.as_bytes()).expect("schreiben");
    p
}

#[test]
fn liest_bahn_und_rechnet_den_kurs() {
    let p = schreibe_ausschnitt();
    let a = lies_flughafen(&p, "TEST").expect("TEST muss gefunden werden");
    assert_eq!(a.bahnen.len(), 2, "beide Bahnenden");
    let b09 = a.bahnen.iter().find(|b| b.bezeichner == "09").unwrap();
    let b27 = a.bahnen.iter().find(|b| b.bezeichner == "27").unwrap();
    // Zwei Punkte auf demselben Breitengrad: Kurs knapp unter 90°, weil
    // der Grosskreis nach Osten leicht nach Sueden zieht.
    assert!(
        (b09.kurs_grad - 89.96).abs() < 0.1,
        "Kurs 09 = {}",
        b09.kurs_grad
    );
    assert!(
        ((b27.kurs_grad - 269.96).abs()) < 0.1,
        "Kurs 27 = {}",
        b27.kurs_grad
    );
    assert!((b09.breite_m - 45.11).abs() < 0.001);
    // 0,1° Laenge auf 53,6° Breite sind rund 6,6 km.
    assert!(
        (b09.laenge_m - 6614.0).abs() < 60.0,
        "Laenge = {}",
        b09.laenge_m
    );
}

#[test]
fn hoert_beim_naechsten_flughafen_auf() {
    // Sonst liefe der Leser durch die ganze 380-MB-Datei und saugte
    // fremde Bahnen mit ein.
    let p = schreibe_ausschnitt();
    let a = lies_flughafen(&p, "TEST").unwrap();
    assert!(
        a.bahnen.iter().all(|b| b.bezeichner != "18"),
        "Bahn des naechsten Platzes eingesammelt"
    );
}

#[test]
fn nur_benannte_rollwege_und_nur_taxiways() {
    let p = schreibe_ausschnitt();
    let a = lies_flughafen(&p, "TEST").unwrap();
    assert_eq!(a.rollwege.len(), 1, "nur die benannte taxiway-Kante");
    assert_eq!(a.rollwege[0].name, "B3");
}

#[test]
fn unbekannter_platz_gibt_nichts() {
    let p = schreibe_ausschnitt();
    assert!(lies_flughafen(&p, "XXXX").is_none());
}

// ─── Gegen die echte Installation ────────────────────────────────────

fn echte_installation() -> Option<std::path::PathBuf> {
    installationen().into_iter().next()
}

#[test]
fn echte_szenerie_fact() {
    let Some(_) = echte_installation() else {
        eprintln!("übersprungen: keine X-Plane-Installation gefunden");
        return;
    };
    let a = flughafen("FACT").expect("FACT muss in der Szenerie stehen");
    let b19 = a
        .bahnen
        .iter()
        .find(|b| b.bezeichner == "19")
        .expect("Bahn 19");
    // Am 28.08.2026 aus der installierten Szenerie gemessen: 165,25°,
    // 61,0 m breit, 3209 m lang. Unsere Navdaten sagen 165,276° — die
    // beiden sind sich hier einig, und genau das hat die These vom
    // „verdrehten FACT" widerlegt.
    assert!(
        (b19.kurs_grad - 165.25).abs() < 0.1,
        "FACT 19 Kurs = {}",
        b19.kurs_grad
    );
    assert!(
        (b19.breite_m - 61.0).abs() < 0.5,
        "Breite = {}",
        b19.breite_m
    );
    assert!(
        (b19.laenge_m - 3209.0).abs() < 20.0,
        "Laenge = {}",
        b19.laenge_m
    );
}

#[test]
fn zusatzszenerie_schlaegt_die_globale() {
    // ⚠ Der wichtigste Test dieser Datei.
    //
    // EGPR (Barra) liegt in einem eigenen Paket UND in der globalen
    // Szenerie, mit verschiedenen Werten: 140,07° gegen 139,62°. Wer die
    // Rangfolge aus `scenery_packs.ini` ignoriert, bekommt fuer jeden
    // Add-on-Flughafen die falsche Bahn — und merkt es nie, weil beide
    // Zahlen plausibel aussehen.
    let Some(wurzel) = echte_installation() else {
        eprintln!("übersprungen: keine X-Plane-Installation gefunden");
        return;
    };
    let dateien = apt_dateien_in_rangfolge(&wurzel);
    assert!(
        dateien.len() > 1,
        "nur eine apt.dat gefunden — ini gelesen?"
    );
    assert!(
        dateien
            .last()
            .is_some_and(|p| p.to_string_lossy().contains("Global Scenery")),
        "die globale Szenerie muss die LETZTE sein"
    );

    let Some(a) = flughafen("EGPR") else {
        eprintln!("übersprungen: EGPR nicht installiert");
        return;
    };
    let b15 = a.bahnen.iter().find(|b| b.bezeichner == "15").expect("15");
    assert!(
        !a.quelle.contains("Global Scenery"),
        "EGPR kam aus der globalen Szenerie statt aus dem Zusatzpaket: {}",
        a.quelle
    );
    assert!(
        (b15.kurs_grad - 140.07).abs() < 0.05,
        "EGPR 15 aus dem Zusatzpaket waere 140,07°, gelesen: {}",
        b15.kurs_grad
    );
}

#[test]
fn echte_szenerie_hat_benannte_rollwege() {
    let Some(_) = echte_installation() else {
        eprintln!("übersprungen: keine X-Plane-Installation gefunden");
        return;
    };
    let a = flughafen("EDDH").expect("EDDH");
    assert!(
        a.rollwege.len() > 20,
        "EDDH sollte viele benannte Rollwege haben, gelesen: {}",
        a.rollwege.len()
    );
    assert!(
        a.rollwege.iter().any(|r| r.name.starts_with('B')),
        "kein Rollweg mit B-Namen"
    );
}

// ─── Verzeichnis ─────────────────────────────────────────────────────

#[test]
fn verzeichnis_liefert_dasselbe_wie_der_volle_durchlauf() {
    // ⚠ Die wichtigste Prüfung des Verzeichnisses.
    //
    // Der schnelle Weg springt an eine gemerkte Byte-Position. Wenn die
    // um eine Zeile daneben liegt, fehlt der Flughafenkopf und der Leser
    // sammelt stillschweigend die Bahnen des NÄCHSTEN Platzes ein — eine
    // Zahl, die plausibel aussieht und falsch ist.
    //
    // Deshalb wird nicht auf feste Werte geprüft, sondern auf
    // Gleichheit mit dem langsamen Weg.
    let Some(wurzel) = echte_installation() else {
        eprintln!("übersprungen: keine X-Plane-Installation gefunden");
        return;
    };
    let idx = sim_xplane::szenerie::SzenerieIndex::bauen(&wurzel);
    assert!(
        idx.anzahl() > 10_000,
        "Verzeichnis zu klein: {}",
        idx.anzahl()
    );
    assert!(
        idx.gueltig(),
        "frisch gebautes Verzeichnis gilt als ungültig"
    );

    // Ein Querschnitt: gross und klein, global und aus Zusatzpaketen,
    // früh und spät in der Datei.
    for icao in [
        "EDDH", "FACT", "KJFK", "EGPR", "EDDV", "EDHE", "LEPA", "EKVG",
    ] {
        let schnell = idx.flughafen(icao);
        let langsam = flughafen(icao);
        match (&schnell, &langsam) {
            (Some(a), Some(b)) => {
                assert_eq!(a.bahnen, b.bahnen, "{icao}: Bahnen weichen ab");
                assert_eq!(a.rollwege, b.rollwege, "{icao}: Rollwege weichen ab");
                assert_eq!(a.quelle, b.quelle, "{icao}: andere Quelle");
            }
            (None, None) => {}
            _ => panic!("{icao}: einmal gefunden, einmal nicht"),
        }
    }
}

#[test]
fn verzeichnis_haelt_die_rangfolge_ein() {
    // Beim Bauen muss der ERSTE Fund gewinnen. Ein `insert` statt
    // `entry().or_insert()` würde die globale Szenerie das Zusatzpaket
    // überschreiben lassen — und zwar lautlos, weil beide Werte
    // plausibel sind.
    let Some(wurzel) = echte_installation() else {
        eprintln!("übersprungen: keine X-Plane-Installation gefunden");
        return;
    };
    let idx = sim_xplane::szenerie::SzenerieIndex::bauen(&wurzel);
    let Some(a) = idx.flughafen("EGPR") else {
        eprintln!("übersprungen: EGPR nicht installiert");
        return;
    };
    assert!(
        !a.quelle.contains("Global Scenery"),
        "das Verzeichnis zeigt auf die globale Szenerie statt aufs Zusatzpaket: {}",
        a.quelle
    );
}
