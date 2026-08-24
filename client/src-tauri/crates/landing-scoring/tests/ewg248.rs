//! Der erste Live-Flug unter v1.7.0 — nachgerechnet mit den echten Werten.
//!
//! # Woher die Zahlen stammen
//!
//! Aus dem gespeicherten Touchdown-Datensatz auf dem Live-Server
//! (`touchdowns.id = 1068`, 24.08.2026, 10:56 UTC): EWG248, EDDL 05R,
//! A220-300 (BCS3), Kennung YL-CSK.
//!
//! Der Flug meldete `rollout: 0 PTS, skipped, reason="surface_unknown"` —
//! keine Querbewertung. Zwei unabhängige Ursachen:
//!
//! 1. **Der Belag** kam als leerer String, weil der Navigraph-Pfad ihn aus
//!    `nav_runways.surface_code` las (0 von 85.058 Zeilen gefüllt) statt
//!    aus der eingebetteten OurAirports-Tabelle, wo EDDL mit `CON` steht.
//! 2. **Das Flugzeugmuster** wurde gar nicht aufgelöst: `landing_vref_
//!    source: "unknown"` entsteht nur, wenn die Grenzen-Tabelle nichts
//!    fand — BCS3 steht dort mit 128 kt. Damit fehlten Spurweite und
//!    Spannweite; die Achse wäre auch nach Fix 1 noch ausgefallen.
//!
//! Diese Prüfung rechnet den Flug mit den Werten, die beide Korrekturen
//! liefern, und verlangt eine echte Note.

use landing_scoring::belag::{belag_aus_angabe, Belag};
use landing_scoring::spurweite::spurweite_m;
use landing_scoring::sub_bahndisziplin::{sub_bahndisziplin, BahndisziplinInput};

/// Die Messwerte, wie sie im Live-Datensatz stehen.
const BAHNBREITE_M: f64 = 45.110_4;
const MAX_VERSATZ_M: f64 = 16.348_484_938_879_846;
const PROBEN: usize = 124;

#[test]
fn ewg248_bekommt_mit_beiden_korrekturen_eine_echte_note() {
    // Fix 1: Der Belag kommt aus der eingebetteten Tabelle — EDDL = CON.
    let belag = belag_aus_angabe(Some("CON"));
    assert_eq!(belag, Belag::Befestigt, "EDDL ist Beton");

    // Fix 2: Das Muster wird aufgelöst — BCS3 steht in der Tabelle.
    let spur = spurweite_m(Some("BCS3")).expect("BCS3 hat eine Spurweite");
    assert!((spur - 6.0).abs() < 0.01, "A220-Spurweite ist 6,0 m");

    let r = sub_bahndisziplin(&BahndisziplinInput {
        max_querversatz_m: Some(MAX_VERSATZ_M),
        bahnbreite_m: Some(BAHNBREITE_M),
        spurweite_m: Some(spur),
        overrun_m: None,
        belag: Some(belag),
        proben: Some(PROBEN),
        // Aus dem Live-Datensatz: airport_source="runway_match",
        // runway_geometry_trusted=true.
        airport_source: Some("runway_match"),
        runway_geometry_trusted: Some(true),
        ..Default::default()
    });

    assert!(
        !r.skipped,
        "die Achse wird immer noch übersprungen: {:?}",
        r.reason
    );
    assert!(
        r.score <= 100,
        "unplausible Punktzahl {}",
        r.score
    );
    // Und der Wert ist nicht geschenkt: Das äussere Rad lag rechnerisch
    // rund 3 m vor der Kante — das ist knapp, und die Note muss das zeigen.
    let halbe = BAHNBREITE_M / 2.0;
    let aussen = MAX_VERSATZ_M + spur / 2.0;
    let rand = halbe - aussen;
    assert!(
        (0.0..6.0).contains(&rand),
        "Randabstand {rand:.1} m — die Annahme dieser Prüfung stimmt nicht mehr"
    );
    assert!(
        r.score < 100,
        "bei {rand:.1} m Randabstand darf es keine volle Punktzahl geben \
         (bekommen: {})",
        r.score
    );
}

/// Die Gegenprobe zum ALTEN Zustand: beide Ursachen einzeln.
#[test]
fn jede_der_beiden_ursachen_allein_haette_die_achse_gekippt() {
    let voll = BahndisziplinInput {
        max_querversatz_m: Some(MAX_VERSATZ_M),
        bahnbreite_m: Some(BAHNBREITE_M),
        spurweite_m: spurweite_m(Some("BCS3")),
        overrun_m: None,
        belag: Some(Belag::Befestigt),
        proben: Some(PROBEN),
        airport_source: Some("runway_match"),
        runway_geometry_trusted: Some(true),
        ..Default::default()
    };

    // Ursache 1: leerer Belag aus den Navdaten.
    let ohne_belag = BahndisziplinInput {
        belag: Some(belag_aus_angabe(Some(""))),
        ..voll
    };
    let r1 = sub_bahndisziplin(&ohne_belag);
    assert!(r1.skipped);
    assert_eq!(r1.reason.as_deref(), Some("surface_unknown"));

    // Ursache 2: Muster nicht aufgelöst → keine Spurweite.
    let ohne_spur = BahndisziplinInput {
        spurweite_m: spurweite_m(None),
        ..voll
    };
    let r2 = sub_bahndisziplin(&ohne_spur);
    assert!(r2.skipped);
    assert_eq!(
        r2.reason.as_deref(),
        Some("track_width_unknown"),
        "auch ohne den Belag-Fehler wäre die Achse ausgefallen"
    );
}
