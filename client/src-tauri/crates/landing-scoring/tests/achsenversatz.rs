//! Läuft die Rollspur schräg zur Bahnachse, stimmt die ACHSE nicht.
//!
//! # Der Fall
//!
//! FACT 19 (Kapstadt), 24.08.2026, A340-600 in X-Plane. Der Bericht sagte
//! „Aufsetzen 24,6 m links" auf einer 61 m breiten Bahn und „grösster
//! Versatz 35,3 m links" — ein Rad weit im Gras. Das Bildschirmfoto des
//! Piloten zeigt die Maschine mittig.
//!
//! Nachgerechnet stimmte die Zahl: Gegen die Navdaten-Achse WAR die
//! Maschine 24,6 m links. Nur läuft die Rollspur auf dem geraden Teil
//! schräg zu dieser Achse — und ein rollendes Flugzeug folgt der
//! aufgemalten Mittellinie. Die X-Plane-Szenerie von FACT ist gegenüber
//! dem AIRAC-Stand verdreht; auf 3201 m macht das 109 m Querfehler.
//!
//! # Warum die Prüfung mit ECHTEN Spuren läuft
//!
//! Die Schwelle (1,0°) ist gemessen, nicht gegriffen: über die zwölf
//! Landungen desselben Tages lag der Median bei 0,29°, alle ausser FACT
//! unter 0,66°. Eine erfundene Spur würde die Schwelle bestätigen, ohne
//! etwas darüber zu sagen, ob sie im Betrieb trennt. Diese Prüfung nimmt
//! die Spuren, wie sie auf dem Server liegen.

use landing_scoring::sub_bahndisziplin::achsen_abweichung_grad;
use std::collections::BTreeMap;

#[derive(serde::Deserialize)]
struct Spur {
    platz: String,
    cutoff: f64,
    punkte: Vec<[f64; 2]>,
}

fn spuren() -> BTreeMap<String, Spur> {
    let roh = include_str!("echte_spuren.json");
    serde_json::from_str(roh).expect("echte_spuren.json")
}

/// Die Schwelle trennt FACT von allen anderen — mit Abstand nach beiden Seiten.
#[test]
fn die_schwelle_trennt_den_stoerfall_vom_normalfall() {
    let alle = spuren();
    assert!(alle.len() >= 10, "zu wenige echte Spuren für eine Aussage");

    let mut fact = None;
    let mut normal: Vec<(String, f64)> = Vec::new();

    for (id, s) in &alle {
        let punkte: Vec<(f64, f64)> = s.punkte.iter().map(|p| (p[0], p[1])).collect();
        let Some(w) = achsen_abweichung_grad(&punkte, s.cutoff) else {
            continue;
        };
        if s.platz.starts_with("FACT") {
            fact = Some(w);
        } else {
            normal.push((format!("{id} {}", s.platz), w.abs()));
        }
    }

    let fact = fact.expect("FACT 19 fehlt in den Prüfdaten");
    assert!(
        fact.abs() > 1.0,
        "FACT 19 liegt bei {fact:.2}° und schlägt damit nicht an — \
         die Szenerie-Verdrehung bliebe unerkannt"
    );

    let groesster = normal
        .iter()
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .expect("keine normalen Landungen");
    assert!(
        groesster.1 <= 1.0,
        "Die normale Landung {} liegt bei {:.2}° und würde fälschlich \
         übersprungen — die Schwelle ist zu eng",
        groesster.0,
        groesster.1
    );
    // Und der Abstand muss ein Abstand sein, kein Zufall.
    assert!(
        fact.abs() > groesster.1 * 2.0,
        "FACT ({:.2}°) liegt nicht deutlich über dem grössten Normalfall \
         ({} mit {:.2}°) — die Schwelle steht auf der Kippe",
        fact.abs(),
        groesster.0,
        groesster.1
    );
}

/// Entartete Eingaben erfinden keinen Winkel.
#[test]
fn ohne_belastbare_punkte_gibt_es_keinen_winkel() {
    assert_eq!(achsen_abweichung_grad(&[], 1000.0), None);
    // Neun Punkte sind zu wenig — eine Gerade daraus ist Zufall.
    let neun: Vec<(f64, f64)> = (0..9).map(|i| (i as f64 * 10.0, 0.0)).collect();
    assert_eq!(achsen_abweichung_grad(&neun, 1000.0), None);
    // Alle auf derselben Längsposition: keine Steigung bestimmbar.
    let senkrecht: Vec<(f64, f64)> = (0..20).map(|i| (500.0, i as f64)).collect();
    assert_eq!(achsen_abweichung_grad(&senkrecht, 1000.0), None);
    // NaN darf nicht durchschlagen.
    let mut mit_nan: Vec<(f64, f64)> = (0..20).map(|i| (i as f64 * 10.0, 1.0)).collect();
    mit_nan.push((f64::NAN, 5.0));
    assert!(achsen_abweichung_grad(&mit_nan, 1000.0).is_some_and(|w| w.is_finite()));
}

/// Das Fenster endet am Bewertungsende — die Ausfahrt ist kein Achsenfehler.
#[test]
fn die_ausfahrt_zaehlt_nicht_als_achsenfehler() {
    // Gerade Spur bis 1500 m, dann scharf nach rechts.
    let mut punkte: Vec<(f64, f64)> = (0..30).map(|i| (500.0 + i as f64 * 33.0, 0.0)).collect();
    for i in 0..20 {
        punkte.push((1500.0 + i as f64 * 5.0, i as f64 * 4.0));
    }
    let mit_ausfahrt = achsen_abweichung_grad(&punkte, 3000.0).expect("Winkel");
    let ohne = achsen_abweichung_grad(&punkte, 1500.0).expect("Winkel");
    assert!(
        ohne.abs() < 0.1,
        "der gerade Teil ist nicht gerade: {ohne:.2}°"
    );
    assert!(
        mit_ausfahrt.abs() > ohne.abs() + 0.5,
        "die Ausfahrt verzerrt den Winkel nicht ({mit_ausfahrt:.2}° gegen \
         {ohne:.2}°) — dann sagt diese Prüfung nichts"
    );
}
