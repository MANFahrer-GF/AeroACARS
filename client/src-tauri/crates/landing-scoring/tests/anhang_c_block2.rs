//! Pflichtenheft P-16.01, Anhang C Block 2 (Landebewertung, Anflug, Bahn,
//! Sprit): je Regel ein Test, benannt nach der Regelnummer. Nur oeffentliche
//! Schnittstelle des Crates — die Bewertung selbst bleibt unberuehrt (WASM-
//! Modul `bewertung.wasm`).

use landing_scoring::flugzeiten::FlugZeiten;
use landing_scoring::{aggregate_master_score, Band, SubScoreEntry};

fn teil(key: &str, punkte: u8) -> SubScoreEntry {
    SubScoreEntry::scored(key, "x", punkte, String::new(), "x", Band::Good)
}

/// Gesamtnote aus Sinkrate (100, Gewicht 3) und einer Achse mit 80 Punkten.
/// Werte >= 75, damit kein Teilabzug greift — die Zahl zeigt allein das
/// Gewicht: 1 → 95, 2 → 92, 3 → 90.
fn mit_achse(key: &str) -> u8 {
    aggregate_master_score(&[teil("landing_rate", 100), teil(key, 80)]).unwrap()
}

#[test]
fn x2_045_aufsetzpunkt_und_achse_gewicht_1_abfangen_gewicht_2() {
    assert_eq!(mit_achse("touchdown_point"), 95, "touchdown_point Gewicht 1");
    assert_eq!(mit_achse("alignment"), 95, "alignment Gewicht 1");
    assert_eq!(mit_achse("abfangen"), 92, "abfangen Gewicht 2");
    // Gegenprobe Messgroesse: die Sinkrate selbst traegt 3.
    assert_eq!(
        aggregate_master_score(&[teil("landing_rate", 80), teil("touchdown_point", 100)]),
        Some(85)
    );
}

#[test]
fn x2_051_flugzeiten_tragen_nur_zeitpunkte_kein_puenktlich_urteil() {
    let z = FlugZeiten {
        plan_off_block: Some(1),
        plan_abheben: Some(2),
        plan_aufsetzen: Some(3),
        plan_on_block: Some(4),
        off_block: Some(5),
        abheben: Some(6),
        aufsetzen: Some(7),
        on_block: Some(8),
        ofp_erstellt: Some(0),
    };
    let json = serde_json::to_value(z).unwrap();
    let mut schluessel: Vec<&str> = json.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    schluessel.sort_unstable();
    assert_eq!(
        schluessel,
        [
            "abheben",
            "aufsetzen",
            "off_block",
            "ofp_erstellt",
            "on_block",
            "plan_abheben",
            "plan_aufsetzen",
            "plan_off_block",
            "plan_on_block",
        ],
        "nur Zeitpunkte — Abweichung und Urteil rechnet die Anzeige"
    );
    assert!(json.as_object().unwrap().values().all(|v| v.is_i64()));
}

#[test]
fn x2_052_nachtraege_ga8_yk18_da50_und_kein_subfleet_code() {
    use landing_scoring::spurweite::{spannweite_m, spurweite_m};
    assert_eq!((spurweite_m(Some("GA8")), spannweite_m(Some("GA8"))), (Some(2.79), Some(12.41)));
    assert_eq!((spurweite_m(Some("YK18")), spannweite_m(Some("YK18"))), (Some(3.12), Some(11.16)));
    // DA50: Spurweite unbelegt → keine Zahl (lieber kein Zuschlag als ein falscher).
    assert_eq!(spurweite_m(Some("DA50")), None);
    // Ein Subfleet-Code ist keine Musterkennung: im Zweifel nichts.
    assert_eq!(spurweite_m(Some("B739ER")), None);
    assert!(spurweite_m(Some("B739")).is_some(), "die ICAO-Kennung selbst ist bekannt");
}
