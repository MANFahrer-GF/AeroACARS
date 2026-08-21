//! Musterkennungen, wie sie im Feld wirklich ankamen.
//!
//! Alle Werte hier stammen aus einer Auswertung der 875 hochgeladenen
//! Flug-Logs (21.08.2026) — nicht aus der Fantasie. Es sind **jene 28**, die
//! `normalize_icao_type` passieren mussten und es teilweise nicht taten.
//!
//! # Warum das ein eigener Test ist
//!
//! Die Reinigung ist über Monate gewachsen, jedes Mal ausgelöst durch einen
//! neuen Einzelfall aus dem Betrieb. Ein Test aus Feldwerten hält fest, was
//! tatsächlich vorkommt, statt was man sich vorstellt — und macht sichtbar,
//! wenn eine spätere Vereinfachung eine der Formen wieder fallen lässt.
//!
//! # Der Anteil, um den es geht
//!
//! Über den Korpus lag der Anteil unbrauchbarer Kennungen bei 51 % (Mai 2026)
//! und ist auf 0 % gefallen (August, 123 Flüge). Betroffen war ausschließlich
//! MSFS 2024; X-Plane liefert saubere Werte. Eine unbrauchbare Kennung macht
//! Kategorie-Erkennung und Profil-Zuordnung blind — das Flugzeug erscheint
//! als „?".
//!
//! # Drei Schreibweisen, nicht eine
//!
//! Das war der Kern des Befunds vom 21.08.2026:
//! * `ATCCOM.AC_MODEL A320.0.text`    — Leerzeichen
//! * `ATCCOM.AC_MODEL_BE58.0.text`    — Unterstrich
//! * `AIRCRAFT.ATC_MODEL_SF50.0.text` — anderes Präfix, anderes Token
//!
//! Die dritte fiel durch, weil die Reinigung nur nach `AC_MODEL` suchte.

/// Jeder Wert, der im Korpus als unbrauchbar auffiel, mit dem Kürzel, das
/// dabei herauskommen muss. Die Häufigkeit steht dabei, damit erkennbar
/// bleibt, was Alltag ist und was Einzelfall.
const FELDWERTE: &[(&str, &str, u32)] = &[
    ("PHENOM 300E", "E55P", 38),
    ("ATCCOM.AC_MODEL A320.0.text", "A320", 32),
    ("ATCCOM.AC_MODEL A321.0.text", "A321", 30),
    ("A350-900", "A359", 14),
    ("Phenom 300E", "E55P", 13),
    ("ATCCOM.AC_MODEL A319.0.text", "A319", 11),
    ("ATCCOM.AC_MODEL B738.0.text", "B738", 6),
    ("FALCON 50", "FA50", 6),
    ("ATCCOM.AC_MODEL_BE58.0.text", "BE58", 6),
    ("ATCCOM.AC_MODEL AEST.0.text", "AEST", 5),
    ("ATCCOM.AC_MODEL C208.0.text", "C208", 5),
    // C680+ ist die Sovereign+ und traegt weiterhin C680. C68A waere die
    // Citation Latitude — ein anderes Flugzeug.
    ("C680+", "C680", 5),
    ("$$:C750", "C750", 4),
    ("ATCCOM.AC_MODEL B77L.0.text", "B77L", 4),
    ("AIRCRAFT.ATC_MODEL_SF50.0.text", "SF50", 4),
    ("ATCCOM.AC_MODEL BE36.0.text", "BE36", 3),
    ("ATCCOM.AC_MODEL A330.0.text", "A330", 3),
    ("A350-1000", "A35K", 2),
    ("A340-300", "A343", 2),
    ("A350-900 ULR", "A359", 2),
    ("ATCCOM.AC_MODEL C172.0.text", "C172", 1),
    ("ATCCOM.AC_MODEL MU2.0.text", "MU2", 1),
    ("ATCCOM.AC_MODEL RJ85.0.text", "RJ85", 1),
    ("ATCCOM.AC_MODEL B736.0.text", "B736", 1),
    ("GA-8", "GA8", 1),
    ("ATCCOM.AC_MODEL B772.0.text", "B772", 1),
    ("ATCCOM.AC_MODEL C185.0.text", "C185", 1),
    ("ATCCOM.AC_MODEL A380.0.text", "A380", 1),
];

#[test]
fn jede_im_feld_beobachtete_kennung_wird_gereinigt() {
    let mut offen = Vec::new();
    let mut falsch = Vec::new();
    for &(roh, erwartet, anzahl) in FELDWERTE {
        match sim_core::normalize_icao_type(roh) {
            None => offen.push((roh, anzahl)),
            Some(v) if v != erwartet => falsch.push((roh, v, erwartet)),
            Some(_) => {}
        }
    }
    assert!(
        offen.is_empty(),
        "diese Kennungen bleiben unbrauchbar (Flugzeug erscheint als „?\"): {offen:?}"
    );
    assert!(
        falsch.is_empty(),
        "falsch zugeordnet — das verzieht Profil und Gewichte: {falsch:?}"
    );
}

#[test]
fn die_drei_schreibweisen_werden_alle_erkannt() {
    // Gegenprobe zum Test darueber: er wuerde auch dann gruen bleiben, wenn
    // eine Schreibweise nur zufaellig ueber die Modellnamen-Tabelle laeuft.
    // Hier steht ausdruecklich, dass die TOKEN-Zerlegung alle drei kann.
    for (roh, erwartet) in [
        ("ATCCOM.AC_MODEL A320.0.text", "A320"),
        ("ATCCOM.AC_MODEL_BE58.0.text", "BE58"),
        ("AIRCRAFT.ATC_MODEL_SF50.0.text", "SF50"),
    ] {
        assert_eq!(
            sim_core::clean_atc_model(roh).as_deref(),
            Some(erwartet),
            "Schreibweise nicht zerlegt: {roh}"
        );
    }
}

#[test]
fn echter_muell_bleibt_muell() {
    // Die andere Richtung: die Reinigung darf nicht alles durchwinken, sonst
    // landet Unsinn als Musterkennung im Landebericht.
    for roh in ["", "   ", "NONE", "NULL", "N/A", "$$:", "ATCCOM..text"] {
        assert_eq!(
            sim_core::normalize_icao_type(roh),
            None,
            "Muell wurde als Kennung akzeptiert: {roh:?}"
        );
    }
}
