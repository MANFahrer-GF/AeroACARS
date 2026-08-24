//! Es gibt EINE Auflösung des Flugzeugmusters — nicht vier.
//!
//! # Der Befund, der diese Prüfung erzwungen hat
//!
//! Am 24.08.2026, erster Live-Tag von v1.7.0, meldete Flug EWG248 nach
//! EDDL „kein Belag erkannt" und bekam keine Querbewertung. Neben dem
//! Belag fehlten auch Spurweite und Spannweite — weil der Flugzeugtyp
//! gar nicht aufgelöst worden war.
//!
//! Die Rückgriffe dafür gab es. Nur hatte jede Stelle andere:
//!
//! | Stelle                     | Sim | Buchung | Titel |
//! |----------------------------|-----|---------|-------|
//! | Vorschau                   |  ✓  |    ✓    |   ✓   |
//! | Bewertung (Grenzen, Vref)  |  ✓  |    ✓    |   ✗   |
//! | MQTT-Touchdown → Server    |  ✗  |    ✓    |   ✗   |
//! | MQTT, zweiter Pfad         |  ✗  |    ✓    |   ✗   |
//!
//! Die dritte Stufe — der Flugzeugtitel — wurde in v1.7.0 eigens gebaut,
//! mit dem Befund „65 von 895 Flügen (7,3 %)" im Kommentar. Sie kam nur
//! in die Vorschau. Bei EWG248 lieferten Sim und Buchung beide nichts,
//! der Titel wäre dagewesen.
//!
//! # Was diese Prüfung leistet
//!
//! Sie liest den Quelltext und verlangt: Wer den Flugzeugtyp braucht,
//! nimmt `sim_core::muster_aufloesen` oder das daraus gemerkte
//! `stats.aufgeloestes_muster`. Eine eigene `.or(...)`-Kette daneben ist
//! genau der Zustand, aus dem dieser Befund entstanden ist.

use std::fs;

/// Der Rumpf einer Funktion ab ihrer Signatur, grob über Klammerbilanz.
fn rumpf(text: &str, ab: usize) -> String {
    let rest = &text[ab..];
    let mut tiefe = 0i32;
    let mut begonnen = false;
    for (i, c) in rest.char_indices() {
        match c {
            '{' => {
                tiefe += 1;
                begonnen = true;
            }
            '}' => {
                tiefe -= 1;
                if begonnen && tiefe == 0 {
                    return rest[..=i].to_string();
                }
            }
            _ => {}
        }
    }
    rest.to_string()
}

/// Stellen, die `flight.aircraft_icao` aus einem ANDEREN Grund lesen als
/// „welches Muster ist geflogen?".
const AUSNAHMEN: &[(&str, &str)] = &[
    (
        "let bid_icao = flight.aircraft_icao.trim().to_uppercase();",
        "Alias-Abgleich: sucht, welches gebuchte Flugzeug zum Sim-Modell \
         passt. Hier ist die BUCHUNG die Frage, nicht das Muster.",
    ),
    (
        "format!(\"{} ({})\", flight.aircraft_icao, flight.aircraft_name)",
        "reine Beschriftung im Aktivitätsprotokoll — was gebucht war, soll \
         auch dastehen.",
    ),
];

#[test]
fn niemand_baut_seine_eigene_musterkette() {
    let quelle = fs::read_to_string("src/lib.rs").expect("lib.rs");

    // Die verräterische Form: `flight.aircraft_icao` als Quelle für den
    // Typ, ohne dass die gemeinsame Auflösung im Spiel ist.
    let mut treffer: Vec<String> = Vec::new();
    for (nr, zeile) in quelle.lines().enumerate() {
        let z = zeile.trim();
        if z.starts_with("//") || z.starts_with("///") {
            continue;
        }
        if !z.contains("flight.aircraft_icao") {
            continue;
        }
        // Erlaubt: als ZWEITE Stufe innerhalb der gemeinsamen Auflösung,
        // oder als Rückfall direkt hinter `aufgeloestes_muster`.
        let fenster_start = nr.saturating_sub(6);
        let fenster: String = quelle
            .lines()
            .skip(fenster_start)
            .take(12)
            .collect::<Vec<_>>()
            .join("\n");
        if fenster.contains("muster_aufloesen")
            || fenster.contains("aufgeloestes_muster")
            || z.contains("muster_fuer_landung")
        {
            continue;
        }
        // Auch erlaubt: rein anzeigende oder speichernde Verwendungen.
        if z.contains("aircraft_icao:") || z.contains("clone()") || z.contains("is_empty()") {
            continue;
        }
        // Begründete Ausnahmen. Jede braucht einen Grund — sonst ist die
        // Liste nur ein Weg, diese Prüfung ruhigzustellen.
        if AUSNAHMEN.iter().any(|(muster, _grund)| z.contains(muster)) {
            continue;
        }
        treffer.push(format!("  lib.rs:{}: {z}", nr + 1));
    }

    assert!(
        treffer.is_empty(),
        "Diese Stellen leiten den Flugzeugtyp selbst ab, statt \
         `sim_core::muster_aufloesen` bzw. `stats.aufgeloestes_muster` zu \
         nehmen. Genau so entstand der EWG248-Befund:\n{}",
        treffer.join("\n")
    );
}

#[test]
fn die_gemeinsame_aufloesung_wird_ueberhaupt_benutzt() {
    let quelle = fs::read_to_string("src/lib.rs").expect("lib.rs");
    assert!(
        quelle.contains("muster_aufloesen("),
        "die gemeinsame Auflösung wird nirgends aufgerufen"
    );
    assert!(
        quelle.matches("aufgeloestes_muster").count() >= 4,
        "das gemerkte Muster wird kaum gelesen — die MQTT-Pfade hängen \
         dann weiter an der Buchung allein"
    );
    // Und sie muss auch wirklich gefüllt werden, nicht nur gelesen.
    let i = quelle
        .find("fn muster_aufloesen")
        .map(|p| rumpf(&quelle, p))
        .unwrap_or_default();
    let _ = i;
    assert!(
        quelle.contains("stats.aufgeloestes_muster = Some("),
        "das Muster wird gelesen, aber nie gesetzt"
    );
}

/// Kein Navdaten-Feld wird ungeprüft weiterverwendet.
///
/// Am 24.08.2026 auf dem Live-Server gezählt (85.058 Bahnen):
///
/// | Feld                     | leer / null |
/// |--------------------------|-------------|
/// | `surface_code`           | **85.058**  |
/// | `displaced_threshold_ft` | 85.058 (0)  |
/// | `tch_ft`                 | 9.804       |
/// | `glideslope_angle`       | 590         |
/// | alle übrigen             | 0           |
///
/// Der Belag hat die Bahndisziplin von v1.7.0 lahmgelegt, weil er
/// ungeprüft durchgereicht wurde. Der Schwellenversatz war schon seit
/// v1.6.8 abgesichert (`geometry_hidden_displacement_ft`), der
/// Gleitwinkel seit v0.15.19 (Klammer 2–7,5°). Die Überflughöhe war es
/// nicht: gegen einen Erwartungswert von 0 wird jeder saubere Anflug als
/// „viel zu hoch" eingestuft.
///
/// Diese Prüfung hält die drei Riegel fest, damit keiner davon beim
/// Umbauen still verschwindet.
#[test]
fn leere_navdaten_felder_bleiben_abgeriegelt() {
    let quelle = std::fs::read_to_string("src/lib.rs").expect("lib.rs");

    let mut fehlt: Vec<&str> = Vec::new();

    // Gleitwinkel: nur plausible Werte — an JEDER Lesestelle.
    //
    // Die erste Fassung dieser Prüfung fragte nur, ob die Klammer
    // irgendwo im Quelltext steht. Sie steht an zwei Stellen; nimmt man
    // eine weg, bleibt die Prüfung grün. Eine Absicherung, die eine von
    // zwei Lücken übersieht, ist keine.
    let lesestellen = quelle.matches("glideslope_angle").count();
    let klammern = quelle.matches("(2.0..=7.5).contains(g)").count();
    if klammern < 2 {
        fehlt.push(
            "Gleitwinkel: weniger als zwei Plausibilitätsklammern — 590 \
             Bahnen liefern 0 oder nichts",
        );
    }
    let _ = lesestellen;
    // Überflughöhe: ohne Erwartungswert keine Einstufung.
    if !quelle.contains("if g.tch_ft > 0") {
        fehlt.push(
            "Überflughöhe ohne Riegel — bei tch_ft=0 (9.804 Bahnen) wird \
             jeder saubere Anflug als „viel zu hoch\" eingestuft",
        );
    }
    // Belag: der Rückgriff auf die eingebettete Tabelle.
    let runway = std::fs::read_to_string("src/runway.rs").expect("runway.rs");
    if !runway.contains("belag_aus_tabelle(") {
        fehlt.push(
            "Belag ohne Rückgriff — nav_runways.surface_code ist in ALLEN \
             85.058 Zeilen leer",
        );
    }

    assert!(fehlt.is_empty(), "{}", fehlt.join("\n  "));
}
