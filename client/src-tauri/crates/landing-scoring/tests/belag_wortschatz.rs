//! Der Server schreibt nur Beläge, die der Client auch deutet.
//!
//! # Der Befund
//!
//! Seit der Live-Server die Bahnbeläge aus Navigraphs Datenbank liefert,
//! stehen dort Schlüssel, die auf dem Server entstehen und hier gedeutet
//! werden müssen. Der erste Anlauf schrieb `GRVL` für Kies — der Client
//! kennt `GRAV` und `GVL`, aber nicht `GRVL`.
//!
//! Folge: 2.286 Bahnen wären als „Belag unbekannt" gemeldet worden statt
//! als Kiespiste. Die Bewertung wäre dieselbe geblieben (auf beidem wird
//! nicht seitlich bewertet), die Begründung im Bericht aber falsch — und
//! genau deshalb hätte es niemand bemerkt.
//!
//! # Warum die Liste hier steht und nicht dort
//!
//! Zwei Programme, zwei Sprachen, ein Wortschatz. Die Deutung wohnt
//! hier, also gehört die Prüfung hierher. Auf der Serverseite steht in
//! `dfdNavFill.ts` ein Verweis auf diese Datei.
//!
//! Wer dort einen Schlüssel ändert, muss ihn hier eintragen — sonst
//! meldet sich diese Prüfung.

use landing_scoring::belag::{belag_aus_angabe, Belag};

/// Jeder Wert aus `BELAG` in `dfdNavFill.ts`, mit der Deutung, die er
/// haben MUSS. Reihenfolge wie dort.
const WAS_DER_SERVER_SCHREIBT: &[(&str, Belag)] = &[
    ("ASP", Belag::Befestigt),    // Navigraph 100, 101, 104, 105, 106
    ("CONC", Belag::Befestigt),   // Navigraph 103, 18
    ("TURF", Belag::Unbefestigt), // Navigraph 4, 19, 17, 3
    ("GRAV", Belag::Unbefestigt), // Navigraph 5
    ("CORAL", Belag::Unbefestigt), // Navigraph 2
    ("ICE", Belag::Unbefestigt),  // Navigraph 6
    ("WATER", Belag::Wasser),     // Navigraph 20
];

#[test]
fn jeder_server_schluessel_wird_gedeutet() {
    for (schluessel, erwartet) in WAS_DER_SERVER_SCHREIBT {
        let ist = belag_aus_angabe(Some(schluessel));
        assert_eq!(
            ist, *erwartet,
            "„{schluessel}\" wird als {ist:?} gedeutet statt als {erwartet:?} — \
             der Server schreibt das, der Client versteht es nicht"
        );
    }
}

#[test]
fn kein_server_schluessel_landet_auf_unbekannt() {
    // Die schlimmste Form des Fehlers: Der Wert kommt an, sieht richtig
    // aus, und die Bewertung meldet trotzdem „Belag unbekannt".
    for (schluessel, _) in WAS_DER_SERVER_SCHREIBT {
        assert_ne!(
            belag_aus_angabe(Some(schluessel)),
            Belag::Unbekannt,
            "„{schluessel}\" faellt durch"
        );
    }
}

#[test]
fn die_kiesschreibweisen_werden_alle_erkannt() {
    // OurAirports schreibt Kies auf vier Arten. Gemessen ueber die
    // eingebettete Tabelle: 319x GRVL, 79x GRVL-G, 47x GRVL-F, 37x GRV —
    // zusammen 482 Bahnen, die vorher durchfielen.
    for s in ["GRVL", "GRVL-G", "GRVL-F", "GRV", "GRAV", "GVL", "GRAVEL"] {
        assert_eq!(
            belag_aus_angabe(Some(s)),
            Belag::Unbefestigt,
            "Kies-Schreibweise „{s}\" faellt durch"
        );
    }
}

#[test]
fn picarra_ueberlebt_die_normalisierung() {
    // Brasilianischer Lateritkies, 108 Bahnen. Das Ç faellt bei der
    // Normalisierung weg — die Praefixliste muss die Form OHNE Cedille
    // enthalten, sonst greift sie nie.
    assert_eq!(belag_aus_angabe(Some("PIÇARRA")), Belag::Unbefestigt);
    assert_eq!(belag_aus_angabe(Some("Piçarra")), Belag::Unbefestigt);
}

#[test]
fn wirklich_unklares_bleibt_unklar() {
    // Gegenprobe: Die Liste darf nicht so weit gefasst sein, dass sie
    // alles verschluckt. Einzelbuchstaben und `UNK` sind in den Daten
    // haeufig und sagen nichts — sie MUESSEN unbekannt bleiben, sonst
    // behauptet der Bericht eine Kenntnis, die es nicht gibt.
    for s in ["UNK", "X", "N", "", "?", "1"] {
        assert_eq!(
            belag_aus_angabe(Some(s)),
            Belag::Unbekannt,
            "„{s}\" wird faelschlich gedeutet"
        );
    }
    assert_eq!(belag_aus_angabe(None), Belag::Unbekannt);
}

#[test]
fn nur_befestigtes_wird_seitlich_bewertet() {
    // Der Grund, warum die Deutung ueberhaupt zaehlt.
    for (schluessel, erwartet) in WAS_DER_SERVER_SCHREIBT {
        let darf = belag_aus_angabe(Some(schluessel)).seitlich_bewertbar();
        assert_eq!(
            darf,
            *erwartet == Belag::Befestigt,
            "„{schluessel}\": seitliche Bewertung {darf}, Belag {erwartet:?}"
        );
    }
}
