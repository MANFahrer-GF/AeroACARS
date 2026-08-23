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
    eintrag(icao).map(|(_, spur, _)| spur)
}

/// Spannweite in Metern, nach ICAO-Typcode.
///
/// `None`, wenn das Muster nicht in der Tabelle steht.
///
/// # Wozu die Spannweite gebraucht wird
///
/// Sie geht **nicht** in die Bewertung ein — sie erklärt sie. Im Grössen-
/// vergleich unter dem Diagramm steht sie neben Bahnbreite und Spurweite
/// (Spec §8.3): Bahnbreite 45,1 m · Spannweite MD-11 51,66 m · Spurweite
/// 10,7 m. Die Spannweite ragt dort sichtbar über die Bahnbreite hinaus, und
/// erst dadurch versteht man, warum die Fahrspur so schmal wirkt: Ein
/// Flugzeug, dessen Flügel breiter sind als die Bahn, fährt trotzdem auf
/// einem Streifen von zehn Metern.
pub fn spannweite_m(icao: Option<&str>) -> Option<f64> {
    eintrag(icao).map(|(_, _, spann)| spann)
}

/// Halbe Breite des Fahrwerks bis zur **Reifen-Aussenkante**, in Metern.
///
/// # Warum das nicht die halbe Spurweite ist
///
/// Die Spurweite in den Herstellerangaben ist der Abstand von **Bein-Mitte
/// zu Bein-Mitte**. Der äussere Rand des äussersten Reifens liegt noch eine
/// halbe Radpaketbreite weiter draussen — bei der 737-800 sind das 0,45 m,
/// denn sie trägt zwei Räder nebeneinander je Bein.
///
/// Für die Frage „lief ein Rad neben der befestigten Fläche" zählt genau
/// dieser äussere Rand, nicht die Bein-Mitte. Ohne den Zuschlag meldet die
/// Anzeige „äusseres Rad 7,6 m von der Mitte", wo es in Wirklichkeit 8,0 m
/// sind.
///
/// # Woher die Radpaketbreite kommt
///
/// Aus der Zahl der Räder je Bein, und die hängt an der Grösse:
///
/// | Spurweite | Bauart | Radpaket |
/// |---|---|---|
/// | bis 4 m | ein Rad je Bein | 0,30 m |
/// | 4–8 m | zwei Räder nebeneinander | 0,90 m |
/// | über 8 m | Bogie, zwei Räder quer | 1,10 m |
///
/// Das ist eine **Näherung nach Baugrösse**, keine Herstellerangabe je
/// Muster. Sie ist bewusst grob und liegt eher zu klein als zu gross: Ein
/// zu grosser Zuschlag würde Landungen als „neben der Bahn" melden, die es
/// nicht waren. Die Kantentoleranz von 1,5 m (§5.4) deckt den Restfehler ab.
// Hier stand ein `aussenkante_halb_m(icao)`, das die Spurweite selbst in
// der Typtabelle nachschlug. Es ist entfernt, und zwar nicht nur weil es
// niemand rief:
//
// Seit v1.7.0 kann die Spurweite aus der **Flugzeugdatei** stammen (Spec
// §5.3 C) und weicht dann bewusst vom Realmuster ab. Eine Funktion, die
// nur den ICAO-Code nimmt, kann diesen Wert gar nicht sehen — sie hätte
// still die Tabelle benutzt, während die Anzeige daneben den Wert aus der
// Datei zeigt. Zwei Zahlen für dieselbe Landung, und die bequemere von
// beiden ist die falsche.
//
// Wer die Aussenkante braucht, hat eine Spurweite in der Hand und nimmt
// `aussenkante_halb_aus_spur`.

/// Wie `aussenkante_halb_m`, aber aus einer **gegebenen** Spurweite.
///
/// # Warum es diese zweite Fassung braucht
///
/// Seit v1.7.0 kann die Spurweite aus der Flugzeugdatei stammen statt aus
/// der Typtabelle (Spec §5.3 C) — bei einem Add-on, das vom Realmuster
/// abweicht, ist sie die genauere Quelle. `aussenkante_halb_m` schlägt
/// aber in der Tabelle nach und würde den Wert aus der Datei still
/// übergehen. Zwei Quellen für dieselbe Größe, und die Anzeige zeigte die
/// eine, die Bewertung die andere.
///
/// Wer eine Spurweite in der Hand hat, nimmt deshalb diese Funktion.
pub fn aussenkante_halb_aus_spur(spurweite_m: f64) -> f64 {
    spurweite_m / 2.0 + radpaket_m(spurweite_m) / 2.0
}

/// Breite des Radpakets eines Hauptfahrwerksbeins, nach Baugrösse.
fn radpaket_m(spurweite_m: f64) -> f64 {
    if spurweite_m < 4.0 {
        0.30
    } else if spurweite_m <= 8.0 {
        0.90
    } else {
        1.10
    }
}

/// Der Tabelleneintrag zu einem Typcode — eine Suche für beide Masse.
fn eintrag(icao: Option<&str>) -> Option<(&'static str, f64, f64)> {
    let icao = icao?.trim().to_ascii_uppercase();
    TABELLE
        .iter()
        .find(|(code, _, _)| *code == icao)
        .copied()
}

/// ICAO-Typcode → (Spurweite Hauptfahrwerk, Spannweite), beide in Metern.
///
/// # Welche Spurweite, wenn es mehrere Fahrwerke gibt
///
/// Die **äusserste**. Mehrere Muster tragen mehr als ein Hauptfahrwerkspaar:
///
/// | Muster | Hauptbeine | massgeblich |
/// |---|---|---|
/// | A380 | zwei am Flügel, zwei am Rumpf | die Flügelbeine, 14,30 m |
/// | B777 | zwei, je sechs Räder | 10,97 m |
/// | A340-600 | zwei aussen, eines mittig | die äusseren, 12,60 m |
/// | B747 | vier, zwei je Seite hintereinander | 11,00 m |
///
/// Der Grund ist die Frage, die diese Achse stellt: Lief ein Rad neben der
/// befestigten Fläche? Das entscheidet immer das äusserste Rad. Ein
/// mittleres oder inneres Bein steht weiter innen und kann die Kante nicht
/// zuerst erreichen — es ist für diese Bewertung ohne Belang.
///
/// Die Leseroutine für Flugzeugdateien (`fahrwerk::spurweite_aus_beinen`)
/// folgt derselben Regel: Sie nimmt den Abstand der äusseren Spuren, nicht
/// den der inneren und nicht die Spannweite aller Räder.
///
/// Sortiert nach Hersteller und Grösse, damit Lücken beim Lesen auffallen.
/// Quelle: Airport Planning Manuals der Hersteller, ICAO Doc 8643.
///
/// **Ein Eintrag je Muster, nicht zwei Tabellen.** Zwei getrennte Listen
/// driften auseinander, sobald jemand nur eine davon ergänzt — dieselbe
/// Fehlerklasse, gegen die §8.4 der Spezifikation eine gemeinsame
/// Projektionsfunktion vorschreibt. Ein Test hält beide Spalten zusammen.
const TABELLE: &[(&str, f64, f64)] = &[
    // ── Airbus ────────────────────────────────────────────────────────
    ("A318", 7.59, 34.10),
    ("A319", 7.59, 35.80),
    ("A320", 7.59, 35.80),
    ("A321", 7.59, 35.80),
    ("A19N", 7.59, 35.80),
    ("A20N", 7.59, 35.80),
    ("A21N", 7.59, 35.80),
    ("A332", 10.69, 60.30),
    ("A333", 10.69, 60.30),
    ("A338", 10.69, 64.00),
    ("A339", 10.69, 64.00),
    ("A342", 10.69, 60.30),
    ("A343", 10.69, 60.30),
    ("A345", 12.60, 63.45),
    ("A346", 12.60, 63.45),
    ("A359", 10.70, 64.75),
    ("A35K", 10.70, 64.75),
    ("A388", 14.30, 79.75),
    ("BCS1", 6.00, 35.10), // A220-100
    ("BCS3", 6.00, 35.10), // A220-300
    // ── Boeing ────────────────────────────────────────────────────────
    ("B712", 5.03, 28.45),
    ("B733", 5.23, 28.88),
    ("B734", 5.23, 28.88),
    ("B735", 5.23, 28.88),
    ("B736", 5.72, 34.32),
    ("B737", 5.72, 34.32),
    ("B738", 5.72, 34.32),
    ("B739", 5.72, 34.32),
    ("B37M", 5.72, 35.92),
    ("B38M", 5.72, 35.92),
    ("B39M", 5.72, 35.92),
    ("B741", 11.00, 59.64),
    ("B742", 11.00, 59.64),
    ("B743", 11.00, 59.64),
    ("B744", 11.00, 64.44),
    ("B748", 12.60, 68.40),
    ("B752", 7.32, 38.05),
    ("B753", 7.32, 38.05),
    ("B762", 9.30, 47.57),
    ("B763", 9.30, 47.57),
    ("B764", 9.30, 51.92),
    ("B772", 10.97, 60.93),
    ("B773", 10.97, 60.93),
    ("B77F", 10.97, 64.80),
    ("B77L", 10.97, 64.80),
    ("B77W", 10.97, 64.80),
    ("B788", 9.75, 60.12),
    ("B789", 9.75, 60.12),
    ("B78X", 9.75, 60.12),
    // ── McDonnell Douglas ─────────────────────────────────────────────
    ("MD11", 10.70, 51.66), // der MPH-9-Fall
    ("MD1F", 10.70, 51.66),
    ("MD82", 5.08, 32.85),
    ("MD83", 5.08, 32.85),
    ("MD88", 5.08, 32.85),
    ("MD90", 5.08, 32.87),
    // ── Embraer / Bombardier / Regional ───────────────────────────────
    ("E170", 5.30, 26.00),
    ("E75L", 5.30, 26.00),
    ("E75S", 5.30, 26.00),
    ("E190", 5.30, 28.72),
    ("E195", 5.30, 28.72),
    ("E290", 5.30, 33.72),
    ("E295", 5.30, 35.10),
    ("CRJ2", 3.54, 21.21),
    ("CRJ7", 4.24, 23.24),
    ("CRJ9", 4.24, 24.85),
    ("CRJX", 4.24, 26.18),
    ("AT43", 4.10, 24.57),
    ("AT45", 4.10, 24.57),
    ("AT72", 4.10, 27.05),
    ("AT76", 4.10, 27.05),
    ("DH8A", 7.87, 25.91),
    ("DH8C", 7.87, 27.43),
    ("DH8D", 7.87, 28.42),
    ("SF34", 6.71, 21.44),
    // ── Frachter / Sonstige Grossflugzeuge ────────────────────────────
    ("A124", 8.00, 73.30),
    ("A225", 8.00, 88.40),
    ("IL96", 10.40, 60.11),
    ("L101", 12.75, 47.35),
    // ── Geschäftsreise ────────────────────────────────────────────────
    ("C25A", 3.30, 15.90),
    ("C25B", 3.30, 16.98),
    ("C25C", 3.30, 17.20),
    ("C510", 2.90, 12.37),
    ("C680", 4.11, 19.24),
    ("C700", 4.30, 21.00),
    ("CL30", 3.00, 19.46),
    ("CL35", 3.00, 21.00),
    ("CL60", 3.20, 19.61),
    ("E55P", 3.20, 16.20),
    ("FA50", 3.60, 18.86),
    ("FA7X", 4.20, 26.21),
    ("GLF5", 4.30, 28.50),
    ("GLF6", 4.30, 30.36),
    ("P180", 3.30, 14.03),
    ("SF50", 3.20, 11.76),
    // ── Nachtrag aus dem Korpus-Lauf 23.08.2026 ───────────────────────
    // Diese Muster tauchten im Bestand auf und fehlten. Ohne sie entfiel die
    // seitliche Bewertung — im ersten Lauf waren das 27,8 % aller Landungen.
    ("A306", 10.69, 44.84),  // A300-600
    ("A310", 10.69, 43.90),
    ("A30B", 10.69, 44.84),
    ("A400", 8.50, 42.40),   // A400M
    ("F28", 5.80, 25.07),    // Fokker F28
    ("F70", 5.04, 28.08),
    ("F100", 5.04, 28.08),
    ("C750", 5.61, 19.38),   // Citation X
    ("HA4T", 3.00, 12.12),   // HondaJet HA-420
    ("AC11", 2.90, 9.75),   // Commander 114
    ("AEST", 3.30, 10.67),   // Aerostar
    ("PA24", 3.10, 10.97),   // Comanche
    ("PA34", 3.60, 11.85),   // Seneca
    ("PA44", 3.20, 11.75),   // Seminole
    ("BE24", 3.00, 10.00),   // Sierra
    ("BE36", 3.10, 10.21),   // Bonanza A36
    ("BE33", 3.00, 10.21),
    ("BE9L", 4.30, 15.32),   // King Air 90
    ("B350", 5.30, 17.65),   // King Air 350
    ("C25M", 3.30, 15.90),
    ("C56X", 5.28, 17.17),   // Citation Excel
    ("C525", 3.20, 14.26),
    ("E50P", 3.20, 12.30),   // Phenom 100
    ("LJ35", 2.50, 12.04),
    ("LJ45", 2.60, 14.58),
    ("H25B", 3.10, 15.66),   // Hawker 800
    ("EA50", 2.20, 11.43),   // Eclipse
    ("SR20", 2.70, 11.68),
    ("C210", 3.10, 11.20),
    ("C206", 2.80, 10.92),
    ("C185", 2.50, 10.92),
    ("PC12", 4.50, 16.28),
    ("PC24", 4.20, 17.00),
    ("TBM8", 3.90, 12.68),
    ("DHC6", 4.30, 19.81),   // Twin Otter
    ("DHC2", 3.30, 14.63),   // Beaver
    ("AN2", 3.36, 18.18),
    ("RV10", 2.70, 9.63),
    ("M20P", 2.70, 10.67),   // Mooney
    ("BL8", 1.80, 9.75),    // Bellanca Decathlon
    // ── Leichtflugzeuge ───────────────────────────────────────────────
    ("C152", 2.30, 10.00),
    ("C172", 2.50, 11.00),
    ("C182", 2.90, 10.97),
    ("C208", 3.60, 15.88),
    ("BE20", 5.30, 16.61),
    ("BE58", 3.20, 11.53),
    ("DA40", 2.30, 11.94),
    ("DA42", 2.60, 13.55),
    ("P28A", 3.00, 10.67),
    ("SR22", 2.70, 11.68),
    ("TBM9", 3.90, 12.82),
    // ── Nachtrag aus der GSG-Flotte (23.08.2026) ──────────────────────
    //
    // Abgeglichen gegen alle 656 Subfleets in phpVMS: 93 verschiedene
    // Muster, davon fehlten 25. Die Liste ist der RUECKFALL — gelesen wird
    // zuerst aus der Flugzeugdatei (§5.3 B). Sie muss trotzdem vollstaendig
    // sein: Ohne Eintrag entfaellt die seitliche Bewertung, und der Pilot
    // sieht „Spurweite nicht hinterlegt" statt einer Note.
    ("B767", 9.30, 47.57),  // Sammelkennung, Werte der -300
    ("B757", 7.32, 38.05),  // Sammelkennung
    ("B777", 10.97, 60.93),  // Sammelkennung, Werte der -200
    ("A20", 7.59, 35.80),  // verkuerzte A320-Kennung aus den Subfleet-Codes
    ("BE35", 2.90, 10.00),  // Bonanza 35
    ("B36", 3.10, 10.21),  // Bonanza A36, zweite Schreibweise zu BE36
    ("B58T", 3.20, 11.53),  // Baron 58TC, zweite Schreibweise zu BE58
    ("P28R", 3.20, 10.67),  // Piper Arrow
    ("E135", 4.10, 20.04),  // ERJ-135
    ("E145", 4.10, 20.04),  // ERJ-145
    ("E13L", 4.10, 21.17),  // Legacy 600 auf ERJ-135-Basis
    ("E175", 5.30, 26.00),  // zweite Schreibweise zu E75L
    ("RJ85", 4.72, 26.21),  // Avro RJ85
    ("B463", 4.72, 26.21),  // BAe 146-300
    ("748", 5.79, 30.02),  // HS 748
    ("CJ4", 3.50, 15.08),  // Citation CJ4
    ("HDJT", 3.00, 12.12),  // HondaJet, zweite Schreibweise zu HA4T
    ("C414", 3.50, 13.45),  // Cessna 414
    ("MU2", 2.44, 11.94),  // Mitsubishi MU-2
    ("VL3", 1.60, 8.43),  // JMB VL-3, Ultraleicht
    ("CONC", 7.72, 25.60),  // Concorde
    // Eurofighter: 5,00 m ist die SPURWEITE, 5,80 m waere der Radstand —
    // die beiden zu verwechseln ist bei Deltafluglern leicht, weil das
    // Fahrwerk dort im Verhaeltnis zur kurzen Spannweite breit steht. Der
    // Plausibilitaetstest (Spannweite ueber dem Doppelten der Spurweite)
    // hat den Fehler gefangen: Mit 5,80 lag das Verhaeltnis bei 1,89.
    ("EUFI", 5.00, 10.95),
    ("H145", 2.00, 11.00),  // Hubschrauber
    ("A109", 2.10, 11.00),  // Hubschrauber
    ("A139", 2.78, 13.80),  // Hubschrauber
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
    fn spannweite_passt_zur_spurweite() {
        // Die Spannweite ist IMMER groesser als die Spurweite -- und zwar
        // deutlich. Faktor 2 ist die konservative Untergrenze: Beim A388
        // (14,30 m Spur, 79,75 m Spannweite) sind es 5,6, beim engsten
        // Muster der Tabelle noch immer ueber 2.
        for (code, spur, spann) in TABELLE {
            assert!(
                *spann > *spur * 2.0,
                "{code}: Spannweite {spann} m gegen Spurweite {spur} m -- \
                 vertauscht oder vertippt?"
            );
            // Obergrenze: die An-225 hatte 88,4 m. Alles darueber ist ein
            // Tippfehler, kein Flugzeug.
            assert!(
                (5.0..=90.0).contains(spann),
                "{code}: {spann} m ist keine plausible Spannweite"
            );
        }
    }

    #[test]
    fn spannweite_der_bekannten_muster() {
        // Stichproben gegen die Herstellerangaben. Die MD-11 ist der
        // MPH-9-Fall und steht im Groessenvergleich der Spezifikation.
        assert_eq!(spannweite_m(Some("MD11")), Some(51.66));
        assert_eq!(spannweite_m(Some("A388")), Some(79.75));
        assert_eq!(spannweite_m(Some("C172")), Some(11.00));
        assert_eq!(spannweite_m(Some("md11")), Some(51.66), "Kleinschreibung");
        assert_eq!(spannweite_m(Some("XXXX")), None, "unbekannt liefert nichts");
    }

    #[test]
    fn die_md11_ragt_ueber_die_bahn() {
        // Der Grund, warum die Spannweite ueberhaupt angezeigt wird: Bei
        // EDDH (46 m breit) ist die MD-11 mit 51,66 m breiter als die Bahn.
        // Geht dieser Vergleich verloren, verliert der Groessenbalken unter
        // dem Diagramm seine Aussage.
        let spann = spannweite_m(Some("MD11")).unwrap();
        assert!(spann > 46.0, "MD-11 {spann} m gegen 46 m Bahnbreite");
        let spur = spurweite_m(Some("MD11")).unwrap();
        assert!(spur < 46.0 / 2.0, "die Fahrspur passt trotzdem bequem");
    }

    #[test]
    fn plausibel_gross_und_klein() {
        // Die Ordnung muss stimmen: je grösser das Muster, desto breiter die Spur.
        let a388 = spurweite_m(Some("A388")).unwrap();
        let b738 = spurweite_m(Some("B738")).unwrap();
        let c172 = spurweite_m(Some("C172")).unwrap();
        assert!(a388 > b738 && b738 > c172, "{a388} > {b738} > {c172}");
        // Kein Wert darf ausserhalb des physikalisch Sinnvollen liegen.
        for (code, m, _) in TABELLE {
            assert!(
                // Untergrenze 1,5 m: die Bellanca Decathlon hat real 1,80 m.
                // Die Schranke prueft Tippfehler, nicht die Physik kleiner
                // Muster.
                (1.5..=16.0).contains(m),
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
    fn jedes_flottenmuster_hat_einen_eintrag() {
        // Die Liste ist der RUECKFALL. Gelesen wird zuerst aus der
        // Flugzeugdatei (§5.3 B) — aber die gelingt nur bei unverschluesselten
        // Add-ons mit eindeutiger Zuordnung. Ueberall sonst traegt diese
        // Tabelle, und fehlt dort ein Muster, entfaellt die seitliche
        // Bewertung: Der Pilot sieht „Spurweite nicht hinterlegt" statt einer
        // Note.
        //
        // Geprueft gegen die echte Flotte aus phpVMS, nicht gegen eine
        // Auswahl. Beim Abgleich am 23.08.2026 fehlten 25 von 93 Mustern.
        let liste = include_str!("../tests/daten/gsg-flotte.txt");
        let mut fehlend: Vec<(&str, u32)> = Vec::new();
        for zeile in liste.lines() {
            let z = zeile.trim();
            if z.is_empty() || z.starts_with('#') {
                continue;
            }
            let mut teile = z.split_whitespace();
            let (Some(muster), anzahl) = (teile.next(), teile.next()) else {
                continue;
            };
            if spurweite_m(Some(muster)).is_none() {
                fehlend.push((muster, anzahl.and_then(|a| a.parse().ok()).unwrap_or(0)));
            }
        }
        assert!(
            fehlend.is_empty(),
            "{} Muster der Flotte ohne Eintrag: {:?}",
            fehlend.len(),
            fehlend
        );
    }

    #[test]
    fn die_flottenliste_ist_lesbar() {
        // Gegenprobe zum Test darueber: Waere die Liste leer oder kaputt,
        // ginge er durch, ohne etwas zu pruefen.
        let liste = include_str!("../tests/daten/gsg-flotte.txt");
        let muster = liste
            .lines()
            .filter(|z| !z.trim().is_empty() && !z.trim_start().starts_with('#'))
            .count();
        assert!(muster > 50, "nur {muster} Muster in der Flottenliste");
    }

    #[test]
    fn die_aussenkante_liegt_hinter_der_halben_spurweite() {
        // Der Punkt, den Thomas gefunden hat: 5,72 m ist Bein-Mitte zu
        // Bein-Mitte. Der Reifenrand liegt weiter draussen — bei der
        // 737-800 um 0,45 m, weil sie zwei Räder je Bein trägt.
        let spur = spurweite_m(Some("B738")).unwrap();
        let aussen = aussenkante_halb_aus_spur(spurweite_m(Some("B738")).unwrap());
        assert!((spur - 5.72).abs() < 0.01);
        assert!(
            (aussen - (2.86 + 0.45)).abs() < 0.01,
            "{aussen} m — erwartet 3,31 m"
        );
        assert!(aussen > spur / 2.0, "die Aussenkante liegt immer weiter draussen");
    }

    #[test]
    fn der_zuschlag_waechst_mit_der_baugroesse() {
        // Ein Kleinflugzeug hat ein Rad je Bein, ein Verkehrsflugzeug zwei,
        // ein Grossraumflugzeug einen Bogie. Der Zuschlag muss dieser
        // Ordnung folgen — sonst bekaeme eine C172 denselben wie eine 747.
        let c172 = aussenkante_halb_aus_spur(spurweite_m(Some("C172")).unwrap()) - spurweite_m(Some("C172")).unwrap() / 2.0;
        let b738 = aussenkante_halb_aus_spur(spurweite_m(Some("B738")).unwrap()) - spurweite_m(Some("B738")).unwrap() / 2.0;
        let b744 = aussenkante_halb_aus_spur(spurweite_m(Some("B744")).unwrap()) - spurweite_m(Some("B744")).unwrap() / 2.0;
        assert!(c172 < b738, "{c172} < {b738}");
        assert!(b738 < b744, "{b738} < {b744}");
        // Und keiner ist so gross, dass er die Bewertung tragen wuerde:
        // Der groesste Zuschlag liegt unter der Kantentoleranz von 1,5 m.
        assert!(b744 < 1.5, "{b744} m Zuschlag ist zu viel");
    }

    #[test]
    fn mehrfache_fahrwerke_fuehren_die_aeussersten() {
        // Wo mehrere Hauptbeine stehen, zaehlt das aeusserste — es
        // entscheidet, ob ein Rad neben der Bahn lief.
        //
        // A380: zwei Beine am Fluegel, zwei am Rumpf. Die Fluegelbeine
        // stehen aussen, ihre Spurweite betraegt 14,30 m. Waere hier der
        // engere Rumpfabstand eingetragen, meldete die Achse ein Rad auf
        // der Bahn, das im Gras lief.
        let a388 = spurweite_m(Some("A388")).unwrap();
        assert!((a388 - 14.30).abs() < 0.01, "{a388}");
        // Und er muss der breiteste Eintrag der Tabelle sein: Kein Muster
        // im Bestand hat ein breiteres Fahrwerk.
        let breitester = TABELLE
            .iter()
            .map(|(_, spur, _)| *spur)
            .fold(f64::MIN, f64::max);
        assert!(
            (a388 - breitester).abs() < 0.01,
            "A388 {a388} gegen den breitesten Eintrag {breitester}"
        );

        // A340-600: ein zusaetzliches Bein MITTIG. Es darf die Spurweite
        // nicht verkleinern — massgeblich sind die aeusseren.
        let a346 = spurweite_m(Some("A346")).unwrap();
        assert!((a346 - 12.60).abs() < 0.01, "{a346}");
        assert!(a346 > spurweite_m(Some("A343")).unwrap(), "die -600 steht breiter");
    }

    #[test]
    fn ohne_muster_keine_aussenkante() {
        // Dieselbe Regel wie bei der Spurweite: Im Zweifel nichts, damit
        // die seitliche Bewertung sichtbar entfaellt statt zu raten.
        //
        // Die Kette ist jetzt zweistufig — erst die Spurweite finden, dann
        // die Aussenkante daraus rechnen. Ohne Muster endet sie schon im
        // ersten Schritt, und der zweite wird gar nicht erreicht.
        assert_eq!(spurweite_m(Some("XXXX")), None);
        assert_eq!(spurweite_m(None), None);
    }

    #[test]
    fn keine_doppelten_eintraege() {
        let mut codes: Vec<&str> = TABELLE.iter().map(|(c, _, _)| *c).collect();
        let vorher = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(vorher, codes.len(), "doppelte Muster in der Tabelle");
    }
}
