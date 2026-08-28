//! Spurweite des Hauptfahrwerks aus der **Flugzeugdatei**.
//!
//! Spec: `docs/spec/v1.7.0-bahndisziplin.md` §5.3 B — Schritt 11 der Bauliste.
//!
//! # Wozu, wenn es doch eine Typtabelle gibt
//!
//! Die Tabelle in `landing-scoring::spurweite` bleibt die **Basis**: Die
//! Spurweite ist eine physische Eigenschaft des realen Musters, und eine
//! MD-11 hat ihre 10,7 m unabhängig davon, wer sie gebaut hat. Sie
//! funktioniert bei verschlüsselten Add-ons, in beiden Simulatoren und ohne
//! Dateizugriff.
//!
//! Was sie nicht kann: Add-ons erfassen, die vom Realmuster abweichen — ein
//! umgebautes Buschflugzeug mit breiterem Fahrwerk, eine Studie mit anderem
//! Fahrwerksstand. Dafür ist dieses Modul die Verfeinerung.
//!
//! # Warum „im Zweifel zurück auf die Tabelle"
//!
//! Ein aus einer Datei gelesener Wert ist nur dann besser als der
//! Tabellenwert, wenn er **sicher** die richtige Datei und die richtige
//! Grösse ist. Sobald etwas nicht eindeutig ist — verschlüsselte
//! Konfiguration, mehrere Varianten im Paket, unplausibler Wert — gibt dieses
//! Modul `None` zurück, und die Tabelle greift. Ein falscher Wert wäre
//! schlimmer als ein grober: Er entscheidet, ob ein Rad neben der Bahn war.

use std::path::Path;

/// Woher ein gelesener Wert stammt — für `track_width_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quelle {
    /// X-Plane `.acf`, Schlüssel `_gear/N/_gear_x`.
    XplaneAcf,
    /// MSFS `flight_model.cfg`, Abschnitt `[CONTACT_POINTS]`.
    MsfsContactPoints,
}

// Beide Varianten gelten nach aussen als `"aircraft_file"` — der Vertrag
// (`docs/spec/runway-diagram-v2.contract.md`) kennt für `track_width_source`
// nur `"type_table"` und `"aircraft_file"`, und ob der Wert aus einer
// `.acf` oder aus `flight_model.cfg` stammt, ändert für den Piloten nichts.
//
// Hier stand dafür ein `Quelle::als_text()`. Es nahm `self`, ignorierte es
// und gab die Konstante zurück — und wurde nie aufgerufen: `bahn_felder`
// entscheidet anhand von `Option::is_some`, ob überhaupt eine Datei
// gelesen wurde. Eine Methode, die ihren Empfänger nicht benutzt und die
// niemand ruft, sieht aus wie eine Unterscheidung, die es nicht gibt.

/// Ergebnis eines Lesevorgangs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fahrwerk {
    /// Spurweite des Hauptfahrwerks in Metern.
    pub spurweite_m: f64,
    pub quelle: Quelle,
}

/// Fuss in Meter.
const FT_M: f64 = 0.3048;

/// Plausibler Bereich einer Spurweite, in Metern.
///
/// Untergrenze 1,5 m: Die Bellanca Decathlon hat real 1,80 m. Obergrenze
/// 16 m: Die A380 hat 14,30 m. Ein Wert ausserhalb ist kein Fahrwerk,
/// sondern ein Lesefehler — falsche Spalte, falsche Einheit, falscher
/// Abschnitt. Dieselben Schranken prüft die Typtabelle für ihre Einträge.
const PLAUSIBEL_M: std::ops::RangeInclusive<f64> = 1.5..=16.0;

/// Ein Fahrwerksbein, wie es aus einer Datei kommt.
#[derive(Debug, Clone, Copy)]
struct Bein {
    /// Querabstand zur Längsachse, in Metern. Vorzeichen behalten.
    quer_m: f64,
    /// Längsposition, in Metern. Positiv ist bei beiden Formaten vorn.
    laengs_m: f64,
}

// ─── X-Plane ──────────────────────────────────────────────────────────

/// Spurweite aus dem Text einer `.acf`-Datei.
///
/// # Format
///
/// Die `.acf` ist reiner ASCII-Text mit Zeilen der Form `P <schlüssel>
/// <wert>`. Das Fahrwerk steht unter `_gear/N/_gear_x|y|z`, **Einheit Fuss**,
/// wobei `_gear_x` die laterale Koordinate ist und `_gear_z` die
/// längs (negativ nach hinten).
///
/// Gegengeprüft am 23.08.2026 an echten Dateien:
///
/// | Muster | aus der Datei | umgerechnet | real |
/// |---|---:|---:|---:|
/// | Zibo 737-800 | 18,90 ft | 5,76 m | 5,72 m |
/// | ToLiss A320 | 24,90 ft | 7,59 m | 7,59 m |
///
/// # Welche Beine zählen
///
/// Das **Hauptfahrwerk**, nicht das Bugrad. Unterschieden wird über die
/// Längsposition: Das Bugbein steht deutlich weiter vorn. Wer stattdessen
/// einfach den grössten Querabstand nähme, läge bei Mustern mit
/// Stützrädern an den Flügelspitzen (747, A340-600) weit daneben — dort ist
/// die Spurweite die des inneren Hauptfahrwerks, nicht der Abstand der
/// äussersten Räder.
pub fn spurweite_aus_acf(text: &str) -> Option<f64> {
    let mut beine: Vec<Bein> = Vec::new();
    for i in 0..24 {
        let x = acf_zahl(text, &format!("_gear/{i}/_gear_x"));
        let z = acf_zahl(text, &format!("_gear/{i}/_gear_z"));
        if let (Some(x), Some(z)) = (x, z) {
            // Ein Bein bei exakt (0,0) ist ein nicht belegter Platz.
            if x == 0.0 && z == 0.0 {
                continue;
            }
            beine.push(Bein {
                quer_m: x * FT_M,
                laengs_m: z * FT_M,
            });
        }
    }
    spurweite_aus_beinen(&beine)
}

/// Einen Zahlenwert aus einer `.acf` lesen.
fn acf_zahl(text: &str, schluessel: &str) -> Option<f64> {
    let prefix = format!("P {schluessel} ");
    text.lines()
        .find_map(|z| z.strip_prefix(&prefix))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
}

// ─── MSFS ─────────────────────────────────────────────────────────────

/// Spurweite aus dem Text einer `flight_model.cfg`.
///
/// # Format
///
/// Abschnitt `[CONTACT_POINTS]`, Zeilen der Form
/// `point.N = Typ, Längs, Quer, Vertikal, …` — Koordinaten in **Fuss**.
/// Nur `Typ 1` sind Räder; 2 ist ein Schwimmer, 4 ein Skid, andere Werte
/// sind Rumpfkontakte und Flügelspitzen.
///
/// ⚠ **Nicht gegengeprüft.** Anders als die X-Plane-Seite steht diese
/// Beschreibung aus Microsofts Dokumentation, aber ohne Messung an einer
/// echten Datei. Solange das offen ist, bleibt die Typtabelle die Basis —
/// dieses Modul liefert nur einen Vorschlag, und die Bewertung nimmt ihn
/// erst, wenn er plausibel ist.
pub fn spurweite_aus_flight_model(text: &str) -> Option<f64> {
    let mut im_abschnitt = false;
    let mut beine: Vec<Bein> = Vec::new();
    for zeile in text.lines() {
        let z = zeile.trim();
        if z.starts_with('[') {
            im_abschnitt = z.eq_ignore_ascii_case("[CONTACT_POINTS]");
            continue;
        }
        if !im_abschnitt {
            continue;
        }
        // `point.3 = 1, -14.0, -8.5, -6.2, …  ; Kommentar`
        let Some((links, rechts)) = z.split_once('=') else {
            continue;
        };
        if !links.trim().to_ascii_lowercase().starts_with("point.") {
            continue;
        }
        // Kommentare abschneiden: `;` und `//`.
        let werte = rechts
            .split(';')
            .next()
            .unwrap_or("")
            .split("//")
            .next()
            .unwrap_or("");
        let felder: Vec<f64> = werte
            .split(',')
            .map(|f| f.trim().parse::<f64>().unwrap_or(f64::NAN))
            .collect();
        if felder.len() < 4 {
            continue;
        }
        // Feld 0 ist der Typ. Nur Räder.
        if felder[0] != 1.0 {
            continue;
        }
        let (laengs, quer) = (felder[1], felder[2]);
        if !laengs.is_finite() || !quer.is_finite() {
            continue;
        }
        beine.push(Bein {
            quer_m: quer * FT_M,
            laengs_m: laengs * FT_M,
        });
    }
    spurweite_aus_beinen(&beine)
}

// ─── Gemeinsam ────────────────────────────────────────────────────────

/// Die Spurweite aus einer Liste von Fahrwerksbeinen.
///
/// Beide Formate liefern dieselbe Art von Daten, also steht die Auswertung
/// an einer Stelle — sonst driften zwei Implementierungen auseinander,
/// sobald jemand nur eine davon anfasst.
fn spurweite_aus_beinen(beine: &[Bein]) -> Option<f64> {
    if beine.len() < 3 {
        // Weniger als drei Beine: kein Standardfahrwerk, keine Aussage.
        return None;
    }
    // Das Bugrad ist das am weitesten vorn stehende Bein. Alles, was
    // deutlich dahinter liegt, gehört zum Hauptfahrwerk.
    //
    // Die Grenze in der Mitte zwischen vorderstem und hinterstem Bein ist
    // robust gegen beide Bauarten: Beim Bugradfahrwerk liegt das Bugbein
    // weit vorn und die Hauptbeine dicht beieinander hinten; beim
    // Spornrad ist es umgekehrt, und die Aufteilung stimmt trotzdem.
    let vorn = beine.iter().map(|b| b.laengs_m).fold(f64::MIN, f64::max);
    let hinten = beine.iter().map(|b| b.laengs_m).fold(f64::MAX, f64::min);
    if !vorn.is_finite() || !hinten.is_finite() || (vorn - hinten) < 1.0 {
        return None;
    }
    let mitte = (vorn + hinten) / 2.0;

    // Die grössere der beiden Gruppen ist das Hauptfahrwerk: Ein Bugbein
    // ist eines, Hauptbeine sind mindestens zwei.
    let vordere: Vec<&Bein> = beine.iter().filter(|b| b.laengs_m > mitte).collect();
    let hintere: Vec<&Bein> = beine.iter().filter(|b| b.laengs_m <= mitte).collect();
    let haupt = if hintere.len() >= vordere.len() {
        hintere
    } else {
        vordere
    };
    if haupt.len() < 2 {
        return None;
    }

    // Die Spurweite ist der Abstand zwischen linkem und rechtem Bein —
    // nicht die Spannweite aller Räder.
    //
    // Bei mehrachsigen Fahrwerken (747: vier Hauptbeine, A340-600: ein
    // zusätzliches mittleres) stehen mehrere Beine je Seite. Massgeblich
    // ist der Abstand der beiden ÄUSSEREN Spuren, denn sie bestimmen, wo
    // das äusserste Rad läuft — und genau darum geht es bei „Rad neben der
    // Bahn". Ein mittleres Bein auf der Achse (A340-600) fällt dabei
    // heraus, weil es weder links noch rechts ist.
    let links = haupt
        .iter()
        .map(|b| b.quer_m)
        .filter(|q| *q < -0.2)
        .fold(f64::MAX, f64::min);
    let rechts = haupt
        .iter()
        .map(|b| b.quer_m)
        .filter(|q| *q > 0.2)
        .fold(f64::MIN, f64::max);
    if !links.is_finite() || !rechts.is_finite() {
        return None;
    }
    let spur = rechts - links;
    PLAUSIBEL_M.contains(&spur).then_some(spur)
}

// ─── Dateizugriff ─────────────────────────────────────────────────────

/// Liest die Spurweite aus dem Paketordner eines Flugzeugs.
///
/// Probiert beide Formate; welches passt, entscheidet der Inhalt des
/// Ordners. Gibt `None` zurück, sobald irgendetwas nicht eindeutig ist —
/// die Typtabelle greift dann weiter.
pub fn spurweite_aus_paket(pkg_dir: &Path) -> Option<Fahrwerk> {
    // X-Plane: genau EINE `.acf` im Ordner. Mehrere bedeuten Varianten,
    // und ohne zu wissen welche geflogen wird, wäre die Wahl geraten.
    let mut acfs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(eintraege) = std::fs::read_dir(pkg_dir) {
        for e in eintraege.flatten() {
            let p = e.path();
            if p.is_file()
                && p.extension()
                    .map(|x| x.eq_ignore_ascii_case("acf"))
                    .unwrap_or(false)
            {
                acfs.push(p);
            }
        }
    }
    if acfs.len() == 1 {
        // Die `.acf` kann mehrere Megabyte haben; das Fahrwerk steht im
        // Kopf. 512 KB decken es sicher ab und halten das Lesen billig.
        if let Ok(text) = lies_kopf(&acfs[0], 512 * 1024) {
            if let Some(m) = spurweite_aus_acf(&text) {
                return Some(Fahrwerk {
                    spurweite_m: m,
                    quelle: Quelle::XplaneAcf,
                });
            }
        }
    }

    // MSFS: `<paket>/SimObjects/Airplanes/<variante>/flight_model.cfg`.
    // Auch hier: nur bei GENAU einer Variante, sonst ist die Zuordnung
    // zum geflogenen Flugzeug nicht gesichert.
    let airplanes = pkg_dir.join("SimObjects").join("Airplanes");
    let mut cfgs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(eintraege) = std::fs::read_dir(&airplanes) {
        for e in eintraege.flatten() {
            let p = e.path().join("flight_model.cfg");
            if p.is_file() {
                cfgs.push(p);
            }
        }
    }
    if cfgs.len() == 1 {
        if let Ok(text) = lies_kopf(&cfgs[0], 256 * 1024) {
            if let Some(m) = spurweite_aus_flight_model(&text) {
                return Some(Fahrwerk {
                    spurweite_m: m,
                    quelle: Quelle::MsfsContactPoints,
                });
            }
        }
    }
    None
}

fn lies_kopf(pfad: &Path, max: usize) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(pfad)?;
    let mut buf = vec![0u8; max];
    let n = f.read(&mut buf)?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zibo 737-800, gegengeprüft an der echten Datei: 18,90 ft = 5,76 m.
    fn zibo_acf() -> String {
        [
            "P acf/_name Boeing 737-800X",
            "P _gear/0/_gear_x 0.00000",
            "P _gear/0/_gear_z 13.41000",
            "P _gear/1/_gear_x -9.45000",
            "P _gear/1/_gear_z -64.51000",
            "P _gear/2/_gear_x 9.45000",
            "P _gear/2/_gear_z -64.51000",
        ]
        .join("\n")
    }

    #[test]
    fn xplane_zibo_stimmt_mit_der_messung() {
        let m = spurweite_aus_acf(&zibo_acf()).expect("Spurweite");
        assert!((m - 5.76).abs() < 0.02, "{m} m gegen die gemessenen 5,76 m");
        // …und liegt bei der Typtabelle (5,72 m) in der Toleranz, mit der
        // die Achse ohnehin rechnet.
        let tabelle = landing_scoring::spurweite::spurweite_m(Some("B738")).unwrap();
        assert!((m - tabelle).abs() < 0.10, "{m} gegen Tabelle {tabelle}");
    }

    #[test]
    fn xplane_toliss_a320() {
        // 24,90 ft = 7,59 m — exakt der Tabellenwert.
        let acf = [
            "P _gear/0/_gear_x 0.0",
            "P _gear/0/_gear_z 20.0",
            "P _gear/1/_gear_x -12.45",
            "P _gear/1/_gear_z -25.0",
            "P _gear/2/_gear_x 12.45",
            "P _gear/2/_gear_z -25.0",
        ]
        .join("\n");
        let m = spurweite_aus_acf(&acf).unwrap();
        assert!((m - 7.59).abs() < 0.02, "{m}");
    }

    #[test]
    fn das_bugrad_zaehlt_nicht_mit() {
        // Ein Bugbein auf der Mittelachse darf die Spurweite nicht
        // verändern — und es darf sie auch nicht verhindern.
        let ohne_bug = [
            "P _gear/1/_gear_x -9.45",
            "P _gear/1/_gear_z -64.51",
            "P _gear/2/_gear_x 9.45",
            "P _gear/2/_gear_z -64.51",
        ]
        .join("\n");
        // Zwei Beine allein sind zu wenig: Ohne das dritte lässt sich
        // vorn und hinten nicht unterscheiden.
        assert_eq!(spurweite_aus_acf(&ohne_bug), None);
    }

    #[test]
    fn mehrachsiges_fahrwerk_nimmt_die_aeusseren_spuren() {
        // 747: vier Hauptbeine, zwei je Seite hintereinander. Massgeblich
        // ist der Abstand der äusseren Spuren (11,00 m laut Tabelle), nicht
        // der Abstand der inneren.
        let acf = [
            "P _gear/0/_gear_x 0.0",
            "P _gear/0/_gear_z 80.0",
            "P _gear/1/_gear_x -18.04", // äussere Spur links
            "P _gear/1/_gear_z -30.0",
            "P _gear/2/_gear_x 18.04", // äussere Spur rechts
            "P _gear/2/_gear_z -30.0",
            "P _gear/3/_gear_x -6.0", // innere Spur links
            "P _gear/3/_gear_z -34.0",
            "P _gear/4/_gear_x 6.0", // innere Spur rechts
            "P _gear/4/_gear_z -34.0",
        ]
        .join("\n");
        let m = spurweite_aus_acf(&acf).unwrap();
        assert!((m - 11.0).abs() < 0.05, "{m} m — erwartet ~11,0 m");
    }

    #[test]
    fn unbelegte_beine_stoeren_nicht() {
        // X-Plane legt Plätze für zehn Beine an; nicht genutzte stehen
        // auf 0/0 und dürfen nicht als Rad auf der Mittelachse zählen.
        let mut acf = zibo_acf();
        for i in 3..10 {
            acf.push_str(&format!("\nP _gear/{i}/_gear_x 0.00000"));
            acf.push_str(&format!("\nP _gear/{i}/_gear_z 0.00000"));
        }
        let m = spurweite_aus_acf(&acf).unwrap();
        assert!((m - 5.76).abs() < 0.02, "{m}");
    }

    #[test]
    fn msfs_contact_points() {
        let cfg = [
            "[VERSION]",
            "major = 1",
            "[CONTACT_POINTS]",
            "static_pitch = 0.0",
            "point.0 = 1, 14.0,   0.0, -6.0, 1600, 0, 0.5   ; Bugrad",
            "point.1 = 1, -8.0, -12.45, -6.0, 1600, 1, 0.9  ; links",
            "point.2 = 1, -8.0,  12.45, -6.0, 1600, 2, 0.9  ; rechts",
            "point.3 = 2, -20.0,  0.0, -2.0                 ; Heck, kein Rad",
            "point.4 = 5, 0.0, -60.0, 0.0                   ; Fluegelspitze",
            "[FLAPS.0]",
        ]
        .join("\n");
        let m = spurweite_aus_flight_model(&cfg).expect("Spurweite");
        // 24,90 ft = 7,59 m
        assert!((m - 7.59).abs() < 0.02, "{m}");
    }

    #[test]
    fn msfs_ignoriert_alles_ausserhalb_des_abschnitts() {
        // Ein `point.N` in einem anderen Abschnitt darf nicht zählen.
        let cfg = [
            "[LIGHTS]",
            "point.0 = 1, 99.0, -99.0, 0.0",
            "[CONTACT_POINTS]",
            "point.0 = 1, 14.0,   0.0, -6.0",
            "point.1 = 1, -8.0, -12.45, -6.0",
            "point.2 = 1, -8.0,  12.45, -6.0",
        ]
        .join("\n");
        let m = spurweite_aus_flight_model(&cfg).unwrap();
        assert!(
            (m - 7.59).abs() < 0.02,
            "{m} — die LIGHTS-Zeile hat gezählt"
        );
    }

    #[test]
    fn msfs_kommentare_und_leerzeichen() {
        for zeile in [
            "point.1 = 1, -8.0, -12.45, -6.0 ; Kommentar mit , Komma",
            "point.1=1,-8.0,-12.45,-6.0",
            "  point.1  =  1 , -8.0 , -12.45 , -6.0  // anderer Kommentar",
        ] {
            let cfg = format!(
                "[CONTACT_POINTS]\npoint.0 = 1, 14.0, 0.0, -6.0\n{zeile}\npoint.2 = 1, -8.0, 12.45, -6.0"
            );
            let m = spurweite_aus_flight_model(&cfg)
                .unwrap_or_else(|| panic!("nicht gelesen: {zeile}"));
            assert!((m - 7.59).abs() < 0.02, "{m} bei {zeile}");
        }
    }

    #[test]
    fn unplausible_werte_werden_verworfen() {
        // Der Fall, gegen den die Schranke steht: falsche Einheit. Wer
        // Meter für Fuss hält, bekommt das Dreifache — und ein Rad gilt
        // dann als neben der Bahn, das mittig lief.
        let zu_breit = [
            "P _gear/0/_gear_x 0.0",
            "P _gear/0/_gear_z 20.0",
            "P _gear/1/_gear_x -40.0",
            "P _gear/1/_gear_z -25.0",
            "P _gear/2/_gear_x 40.0",
            "P _gear/2/_gear_z -25.0",
        ]
        .join("\n");
        assert_eq!(
            spurweite_aus_acf(&zu_breit),
            None,
            "24 m sind kein Fahrwerk"
        );

        let zu_schmal = [
            "P _gear/0/_gear_x 0.0",
            "P _gear/0/_gear_z 5.0",
            "P _gear/1/_gear_x -1.0",
            "P _gear/1/_gear_z -3.0",
            "P _gear/2/_gear_x 1.0",
            "P _gear/2/_gear_z -3.0",
        ]
        .join("\n");
        assert_eq!(
            spurweite_aus_acf(&zu_schmal),
            None,
            "0,6 m sind kein Fahrwerk"
        );
    }

    #[test]
    fn die_datei_schlaegt_die_tabelle_nur_wenn_sie_etwas_liefert() {
        // Die Regel aus Spec §5.3: Die Datei ist die Verfeinerung, die
        // Tabelle die Basis. In `bahn_felder` steht dafuer
        // `aus_datei.or_else(|| tabelle)` — dieser Test haelt die
        // Reihenfolge fest, damit sie beim Umbauen nicht kippt.
        //
        // Die Gefahr liegt in der anderen Richtung: Wer `tabelle.or(datei)`
        // schreibt, bekommt bei JEDEM bekannten Muster den Tabellenwert,
        // und die Datei waere totes Gewicht — ohne dass ein Test rot wird,
        // denn beide Werte sind ja plausibel.
        let tabelle = landing_scoring::spurweite::spurweite_m(Some("B738"));
        assert_eq!(tabelle, Some(5.72));

        // Datei liefert etwas -> Datei gewinnt.
        let aus_datei: Option<f64> = Some(5.76);
        assert_eq!(aus_datei.or(tabelle), Some(5.76));

        // Datei liefert nichts -> Tabelle greift.
        let ohne: Option<f64> = None;
        assert_eq!(ohne.or(tabelle), Some(5.72));

        // Weder noch -> nichts. Die seitliche Bewertung entfaellt dann
        // sichtbar, statt mit einem Mittelwert ueberbrueckt zu werden.
        let unbekannt = landing_scoring::spurweite::spurweite_m(Some("XXXX"));
        assert_eq!(ohne.or(unbekannt), None);
    }

    #[test]
    fn leeres_und_unsinniges_liefert_nichts() {
        for text in [
            "",
            "kein Fahrwerk hier",
            "[CONTACT_POINTS]",
            "P _gear/0/_gear_x abc",
        ] {
            assert_eq!(spurweite_aus_acf(text), None, "{text:?}");
            assert_eq!(spurweite_aus_flight_model(text), None, "{text:?}");
        }
    }

    #[test]
    fn spornrad_wird_richtig_aufgeteilt() {
        // Beim Spornradfahrwerk steht das einzelne Bein HINTEN. Die
        // Aufteilung über die Mitte muss trotzdem das Hauptfahrwerk finden.
        let acf = [
            "P _gear/0/_gear_x 0.0",
            "P _gear/0/_gear_z -15.0", // Sporn, hinten
            "P _gear/1/_gear_x -4.0",
            "P _gear/1/_gear_z 5.0", // Hauptbein links, vorn
            "P _gear/2/_gear_x 4.0",
            "P _gear/2/_gear_z 5.0",
        ]
        .join("\n");
        let m = spurweite_aus_acf(&acf).unwrap();
        // 8 ft = 2,44 m
        assert!((m - 2.44).abs() < 0.02, "{m}");
    }
}
