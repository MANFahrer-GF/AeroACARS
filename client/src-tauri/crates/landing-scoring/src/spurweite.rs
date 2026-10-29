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
fn eintrag(icao: Option<&str>) -> Option<Zeile> {
    let icao = icao?.trim().to_ascii_uppercase();
    tabelle().iter().find(|(code, _, _)| *code == icao).copied()
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
/// Eine Tabellenzeile.
pub type Zeile = (&'static str, f64, f64);

/// Eingebauter Stand = Datei im Paketordner (P-3.54, eine Quelle).
pub const EINGEBAUT: &str = include_str!("../../../../../bundle/tabellen/spurweiten.json");

/// Woher die Tabelle kommt: Der Client meldet beim Start eine Quelle an, die
/// aus seinem einen Paketstand liest (P-3.02); ohne Anmeldung (Tests, Werkzeuge)
/// gilt der eingebaute Stand.
static QUELLE: std::sync::OnceLock<fn() -> &'static [Zeile]> = std::sync::OnceLock::new();

fn tabelle() -> &'static [Zeile] {
    match QUELLE.get() {
        Some(quelle) => quelle(),
        None => {
            static EIGEN: std::sync::OnceLock<&'static [Zeile]> = std::sync::OnceLock::new();
            EIGEN.get_or_init(|| lesen(EINGEBAUT).expect("eingebaute Tabelle spurweiten.json"))
        }
    }
}

/// Meldet die Quelle an (einmal je Lauf). `false`, wenn schon eine gilt.
pub fn quelle_setzen(quelle: fn() -> &'static [Zeile]) -> bool {
    QUELLE.set(quelle).is_ok()
}

/// Prueft `tabellen/spurweiten.json` wie [`lesen`], ohne etwas dauerhaft abzulegen.
pub fn pruefen(json: &str) -> Result<(), String> {
    datei_lesen(json).map(|_| ())
}

/// Liest `tabellen/spurweiten.json` (fail-closed). Das Ergebnis lebt bis
/// Programmende (einmal je Lauf).
pub fn lesen(json: &str) -> Result<&'static [Zeile], String> {
    fn s(x: String) -> &'static str {
        Box::leak(x.into_boxed_str())
    }
    let zeilen: Vec<Zeile> =
        datei_lesen(json)?.muster.into_iter().map(|m| (s(m.icao), m.spur_m, m.spann_m)).collect();
    Ok(Box::leak(zeilen.into()))
}

// Dateiform von `tabellen/spurweiten.json`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Datei {
    format: u32,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
    muster: Vec<MusterDatei>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MusterDatei {
    icao: String,
    spur_m: f64,
    spann_m: f64,
    #[serde(default)]
    #[allow(dead_code)]
    gruppe: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
}

/// Liest und prueft: ein Eintrag je Muster, Kennung in Grossbuchstaben,
/// Spurweite > 0 und nie breiter als die Spannweite.
fn datei_lesen(json: &str) -> Result<Datei, String> {
    let d: Datei = serde_json::from_str(json).map_err(|e| format!("tabellen/spurweiten.json: {e}"))?;
    if d.format != 1 {
        return Err(format!("tabellen/spurweiten.json: Format {} unbekannt", d.format));
    }
    let mut gesehen = std::collections::HashSet::new();
    for m in &d.muster {
        if m.icao.is_empty() || m.icao != m.icao.trim().to_ascii_uppercase() || !gesehen.insert(m.icao.as_str()) {
            return Err(format!("tabellen/spurweiten.json: Kennung {:?} ungueltig oder doppelt", m.icao));
        }
        if !(m.spur_m.is_finite() && m.spur_m > 0.0 && m.spann_m.is_finite() && m.spann_m >= m.spur_m) {
            return Err(format!("tabellen/spurweiten.json: {} Masse unplausibel", m.icao));
        }
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    /// Die Tabelle im Paket ist genau die Konstante `TABELLE` aus 2.1.24
    /// (Etappe 1b): Abdruck (FNV-1a) der Debug-Ausgabe, vor dem Entfernen
    /// gegen sie verglichen. Bewusste Aenderung: Abdruck mit Begruendung
    /// nachziehen.
    #[test]
    fn tabelle_wie_2_1_24() {
        let text = format!("{:?}", super::lesen(super::EINGEBAUT).unwrap());
        let mut h: u64 = 0xcbf29ce484222325;
        for b in text.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        assert_eq!((h, text.len()), (0xe33981d7e47910af, 3889));
    }

    #[test]
    fn regelverstoss_ist_ein_lesefehler() {
        let gut: serde_json::Value = serde_json::from_str(super::EINGEBAUT).unwrap();
        assert!(super::pruefen(&gut.to_string()).is_ok());
        let mut doppelt = gut.clone();
        let erste = doppelt["muster"][0].clone();
        doppelt["muster"].as_array_mut().unwrap().push(erste);
        assert!(super::pruefen(&doppelt.to_string()).is_err());
        let mut null = gut.clone();
        null["muster"][0]["spur_m"] = 0.0.into();
        assert!(super::pruefen(&null.to_string()).is_err());
        let mut fremd = gut;
        fremd["muster"][0]["extra"] = 1.into();
        assert!(super::pruefen(&fremd.to_string()).is_err());
    }


    use super::*;

    #[test]
    fn mph9_die_md11() {
        // Der Auslöser: PH-MCU, MD-11F. Reale Spurweite 10,7 m.
        assert_eq!(spurweite_m(Some("MD11")), Some(10.70));
        assert_eq!(spurweite_m(Some("md11")), Some(10.70), "Kleinschreibung");
        assert_eq!(spurweite_m(Some("  MD11 ")), Some(10.70), "Leerzeichen");
    }

    #[test]
    fn gsg1152_die_duke() {
        // Der Auslöser: Sven M., BE60 (Beechcraft Duke), 2026-09-11 —
        // "Spurweite dieses Musters ist nicht hinterlegt". Werte aus dem
        // Original: Beechcraft Duke 60 Series Maintenance Manual, Kapitel
        // 6, Figure 1 (11'2.4" Spurweite, 39'3.25" Spannweite).
        assert_eq!(spurweite_m(Some("BE60")), Some(3.41));
        assert_eq!(spurweite_m(Some("be60")), Some(3.41), "Kleinschreibung");
    }

    #[test]
    fn gsg1152_nachtrag_drei_weitere_echte_luecken() {
        // Beim GSG1152-Fix aufgedeckt: Diff des LIVE-phpVMS-Bestands gegen
        // den 23.08.-Schnappschuss fand 13 neue Subfleet-Muster, davon 3
        // echte Luecken (10 waren Subfleet-Namensschema-Artefakte, siehe
        // Kommentar an der Tabelle). GA8: Owners/Pilots Info Manual
        // Figure 1-1. YK18 (Jak-18T, nicht die alte Jak-18/18A):
        // Hersteller-Datenblatt smaz.ru.
        assert_eq!(spurweite_m(Some("GA8")), Some(2.79));
        assert_eq!(spurweite_m(Some("YK18")), Some(3.12));
        // DA50 bewusst NICHT eingetragen — siehe Kommentar an der Tabelle.
        assert_eq!(
            spurweite_m(Some("DA50")),
            None,
            "Spurweite unbelegt, bleibt None"
        );
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
            "BCS3", "A320", "A21N", "A333", "A343", "B738", "B744", "B748", "B763", "B77W", "B78X",
            "MD11", "AT76", "E195", "CRJ9", "C172", "C182", "P180", "FA50", "C680", "SF50", "L101",
            "BE60", "GA8", "YK18",
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
        for (code, spur, spann) in tabelle() {
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
        for (code, m, _) in tabelle() {
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
        // Auswahl. Beim Abgleich am 23.08.2026 fehlten 25 von 93 Mustern,
        // beim Nachtrag zu GSG1152 (11.09.2026) drei weitere (siehe Kommentar
        // an der Liste selbst: die alte Liste war ausserdem aus dem falschen
        // Feld erzeugt, `phpvmssubfleets.type` statt `Aircraft.icao`).
        //
        // `BEKANNTE_LUECKEN`: bewusst dokumentierte, noch ungeloeste Faelle
        // — NICHT stillschweigend uebersprungen, sondern explizit benannt,
        // damit diese Prüfung weiter jedes NEUE fehlende Muster faengt, ohne
        // wegen eines bereits bekannten, recherchierten Falls dauerhaft rot
        // zu bleiben. Ein Muster hier zu listen ist keine Bequemlichkeit —
        // jeder Eintrag braucht denselben Rechercheaufwand wie ein echter
        // Tabelleneintrag, nur dass das Ergebnis "nicht auffindbar" war.
        const BEKANNTE_LUECKEN: &[&str] = &[
            // Diamond DA50 RG: Spannweite gesichert (EASA-Musterzulassung
            // EASA.A.639, 13,41 m), aber die Spurweite steht in keiner
            // oeffentlich zugaenglichen Quelle (Hersteller-Datenblaetter,
            // AFM/POH-Auszuege, AOPA-Guide — keine zeigt eine Frontansicht
            // mit Spurweiten-Bemassung). CS-23-Musterzulassungen fuehren das
            // Mass typischerweise gar nicht. Recherchiert 2026-09-11.
            "DA50",
        ];
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
            if BEKANNTE_LUECKEN.contains(&muster) {
                continue;
            }
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
        assert!(
            aussen > spur / 2.0,
            "die Aussenkante liegt immer weiter draussen"
        );
    }

    #[test]
    fn der_zuschlag_waechst_mit_der_baugroesse() {
        // Ein Kleinflugzeug hat ein Rad je Bein, ein Verkehrsflugzeug zwei,
        // ein Grossraumflugzeug einen Bogie. Der Zuschlag muss dieser
        // Ordnung folgen — sonst bekaeme eine C172 denselben wie eine 747.
        let c172 = aussenkante_halb_aus_spur(spurweite_m(Some("C172")).unwrap())
            - spurweite_m(Some("C172")).unwrap() / 2.0;
        let b738 = aussenkante_halb_aus_spur(spurweite_m(Some("B738")).unwrap())
            - spurweite_m(Some("B738")).unwrap() / 2.0;
        let b744 = aussenkante_halb_aus_spur(spurweite_m(Some("B744")).unwrap())
            - spurweite_m(Some("B744")).unwrap() / 2.0;
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
        let breitester = tabelle()
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
        assert!(
            a346 > spurweite_m(Some("A343")).unwrap(),
            "die -600 steht breiter"
        );
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
        let mut codes: Vec<&str> = tabelle().iter().map(|(c, _, _)| *c).collect();
        let vorher = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(vorher, codes.len(), "doppelte Muster in der Tabelle");
    }
}
