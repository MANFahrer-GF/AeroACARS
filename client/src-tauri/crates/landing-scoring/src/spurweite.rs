//! Spurweite des Hauptfahrwerks je Muster — Grundlage für „Rad neben der Bahn".
//!
//! # Warum das gebraucht wird
//!
//! Aus der Telemetrie kennen wir nur **einen** Punkt des Flugzeugs, den
//! Referenzpunkt. Ob ein *Rad* neben der befestigten Fläche lief, lässt sich
//! daraus erst sagen, wenn man weiss, wie weit die Räder vom Referenzpunkt
//! entfernt stehen.
//!
//! # Warum eine Tabelle die richtige Basis ist
//!
//! Die Spurweite ist eine **physische Eigenschaft des realen Musters**, nicht
//! des Add-ons. Eine MD-11 hat ihre 10,7 m, unabhängig davon, wer sie gebaut
//! hat. Damit ist die Tabelle nicht der Lückenfüller, sondern die robusteste
//! Quelle: Sie funktioniert bei verschlüsselten Add-ons, in beiden Simulatoren
//! und ohne Dateizugriff.
//!
//! Die exakte Ableitung aus der Flugzeugdatei (X-Plane `.acf`, MSFS
//! `flight_model.cfg`) bleibt als *Verfeinerung* vorgesehen — sie fängt
//! Add-ons, die vom Realmuster abweichen. Siehe
//! `docs/spec/v1.7.0-bahndisziplin.md` §5.3.
//!
//! # Genauigkeit
//!
//! Die Werte stammen aus den Herstellerangaben (Airport Planning Manuals bzw.
//! ICAO Doc 8643-Kategorien). Sie müssen nicht auf den Zentimeter stimmen: Die
//! Achse arbeitet mit einer **Kantentoleranz von 1,5 m** (§5.4), weil schon die
//! Bahnbreite in den Daten gerundet ist. Eine Abweichung von einigen
//! Dezimetern ändert nichts an der Note.
//!
//! # Grundsatz: im Zweifel `None`
//!
//! Ein geratener Wert wäre schlimmer als keiner. Fehlt das Muster, entfällt die
//! seitliche Bewertung sichtbar (`track_width_unknown`) — sie wird nicht mit
//! einem Mittelwert überbrückt.

/// Spurweite des Hauptfahrwerks in Metern, nach ICAO-Typcode.
///
/// `None`, wenn das Muster nicht in der Tabelle steht.
pub fn spurweite_m(icao: Option<&str>) -> Option<f64> {
    let icao = icao?.trim().to_ascii_uppercase();
    TABELLE
        .iter()
        .find(|(code, _)| *code == icao)
        .map(|(_, m)| *m)
}

/// ICAO-Typcode → Spurweite Hauptfahrwerk in Metern.
///
/// Sortiert nach Hersteller und Grösse, damit Lücken beim Lesen auffallen.
/// Quelle: Airport Planning Manuals der Hersteller.
const TABELLE: &[(&str, f64)] = &[
    // ── Airbus ────────────────────────────────────────────────────────
    ("A318", 7.59),
    ("A319", 7.59),
    ("A320", 7.59),
    ("A321", 7.59),
    ("A19N", 7.59),
    ("A20N", 7.59),
    ("A21N", 7.59),
    ("A332", 10.69),
    ("A333", 10.69),
    ("A338", 10.69),
    ("A339", 10.69),
    ("A342", 10.69),
    ("A343", 10.69),
    ("A345", 12.60),
    ("A346", 12.60),
    ("A359", 10.70),
    ("A35K", 10.70),
    ("A388", 14.30),
    ("BCS1", 6.00), // A220-100
    ("BCS3", 6.00), // A220-300
    // ── Boeing ────────────────────────────────────────────────────────
    ("B712", 5.03),
    ("B733", 5.23),
    ("B734", 5.23),
    ("B735", 5.23),
    ("B736", 5.72),
    ("B737", 5.72),
    ("B738", 5.72),
    ("B739", 5.72),
    ("B37M", 5.72),
    ("B38M", 5.72),
    ("B39M", 5.72),
    ("B741", 11.00),
    ("B742", 11.00),
    ("B743", 11.00),
    ("B744", 11.00),
    ("B748", 12.60),
    ("B752", 7.32),
    ("B753", 7.32),
    ("B762", 9.30),
    ("B763", 9.30),
    ("B764", 9.30),
    ("B772", 10.97),
    ("B773", 10.97),
    ("B77F", 10.97),
    ("B77L", 10.97),
    ("B77W", 10.97),
    ("B788", 9.75),
    ("B789", 9.75),
    ("B78X", 9.75),
    // ── McDonnell Douglas ─────────────────────────────────────────────
    ("MD11", 10.70), // der MPH-9-Fall
    ("MD1F", 10.70),
    ("MD82", 5.08),
    ("MD83", 5.08),
    ("MD88", 5.08),
    ("MD90", 5.08),
    // ── Embraer / Bombardier / Regional ───────────────────────────────
    ("E170", 5.30),
    ("E75L", 5.30),
    ("E75S", 5.30),
    ("E190", 5.30),
    ("E195", 5.30),
    ("E290", 5.30),
    ("E295", 5.30),
    ("CRJ2", 3.54),
    ("CRJ7", 4.24),
    ("CRJ9", 4.24),
    ("CRJX", 4.24),
    ("AT43", 4.10),
    ("AT45", 4.10),
    ("AT72", 4.10),
    ("AT76", 4.10),
    ("DH8A", 7.87),
    ("DH8C", 7.87),
    ("DH8D", 7.87),
    ("SF34", 6.71),
    // ── Frachter / Sonstige Grossflugzeuge ────────────────────────────
    ("A124", 8.00),
    ("A225", 8.00),
    ("IL96", 10.40),
    ("L101", 12.75),
    // ── Geschäftsreise ────────────────────────────────────────────────
    ("C25A", 3.30),
    ("C25B", 3.30),
    ("C25C", 3.30),
    ("C510", 2.90),
    ("C680", 4.11),
    ("C700", 4.30),
    ("CL30", 3.00),
    ("CL35", 3.00),
    ("CL60", 3.20),
    ("E55P", 3.20),
    ("FA50", 3.60),
    ("FA7X", 4.20),
    ("GLF5", 4.30),
    ("GLF6", 4.30),
    ("P180", 3.30),
    ("SF50", 3.20),
    // ── Leichtflugzeuge ───────────────────────────────────────────────
    ("C152", 2.30),
    ("C172", 2.50),
    ("C182", 2.90),
    ("C208", 3.60),
    ("BE20", 5.30),
    ("BE58", 3.20),
    ("DA40", 2.30),
    ("DA42", 2.60),
    ("P28A", 3.00),
    ("SR22", 2.70),
    ("TBM9", 3.90),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mph9_die_md11() {
        // Der Auslöser: PH-MCU, MD-11F. Reale Spurweite 10,7 m.
        assert_eq!(spurweite_m(Some("MD11")), Some(10.70));
        assert_eq!(spurweite_m(Some("md11")), Some(10.70), "Kleinschreibung");
        assert_eq!(spurweite_m(Some("  MD11 ")), Some(10.70), "Leerzeichen");
    }

    #[test]
    fn stimmt_mit_den_flugzeugdateien_ueberein() {
        // Gegengeprüft an echten X-Plane-.acf-Dateien (23.08.2026):
        // Zibo 737-800  = 18,90 ft = 5,76 m   (Tabelle 5,72 — real)
        // ToLiss A320   = 24,90 ft = 7,59 m   (Tabelle 7,59 — exakt)
        let b738 = spurweite_m(Some("B738")).unwrap();
        assert!((b738 - 5.76).abs() < 0.10, "B738 {b738} gegen .acf 5,76 m");
        let a320 = spurweite_m(Some("A320")).unwrap();
        assert!((a320 - 7.59).abs() < 0.05, "A320 {a320} gegen .acf 7,59 m");
    }

    #[test]
    fn deckt_die_gsg_flotte_ab() {
        // Muster, die im Bestand tatsächlich vorkommen — jedes ohne Wert
        // bedeutet eine Landung ohne seitliche Bewertung.
        for icao in [
            "BCS3", "A320", "A21N", "A333", "A343", "B738", "B744", "B748", "B763", "B77W",
            "B78X", "MD11", "AT76", "E195", "CRJ9", "C172", "C182", "P180", "FA50", "C680",
            "SF50", "L101",
        ] {
            assert!(
                spurweite_m(Some(icao)).is_some(),
                "{icao} fehlt in der Spurweiten-Tabelle"
            );
        }
    }

    #[test]
    fn plausibel_gross_und_klein() {
        // Die Ordnung muss stimmen: je grösser das Muster, desto breiter die Spur.
        let a388 = spurweite_m(Some("A388")).unwrap();
        let b738 = spurweite_m(Some("B738")).unwrap();
        let c172 = spurweite_m(Some("C172")).unwrap();
        assert!(a388 > b738 && b738 > c172, "{a388} > {b738} > {c172}");
        // Kein Wert darf ausserhalb des physikalisch Sinnvollen liegen.
        for (code, m) in TABELLE {
            assert!(
                (2.0..=16.0).contains(m),
                "{code}: {m} m ist keine plausible Spurweite"
            );
        }
    }

    #[test]
    fn unbekannt_liefert_nichts() {
        // Ein geratener Wert waere schlimmer als keiner — er wuerde geglaubt.
        for icao in [None, Some(""), Some("   "), Some("XXXX"), Some("A999")] {
            assert_eq!(spurweite_m(icao), None, "{icao:?} darf keinen Wert liefern");
        }
    }

    #[test]
    fn keine_doppelten_eintraege() {
        let mut codes: Vec<&str> = TABELLE.iter().map(|(c, _)| *c).collect();
        let vorher = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(vorher, codes.len(), "doppelte Muster in der Tabelle");
    }
}
