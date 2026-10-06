//! Vref beim Aufsetzen — gemessen, wo das Flugzeug sie hergibt, sonst
//! aus dem Landegewicht gerechnet, und nur mit Urteil, wo die Rechnung
//! belegt ist.
//!
//! # Warum es dieses Modul gibt
//!
//! Bis 2.0.5 nahm der Client für jedes Muster **einen festen Wert** an
//! (`typical_vref_kt`, A380 = 145 kt), unabhängig vom Gewicht. Gemessen am
//! 06.10.2026: 94 % aller Vref-Abweichungen kamen aus diesem Pauschalwert.
//! Eine A380 mit 348 t (AIB 427) bekam „−20 kt“ in Rot, obwohl sie bei ihrem
//! Gewicht passend schnell aufsetzte. Thomas: „nichts annehmen, was wir nicht
//! können“.
//!
//! # Rangfolge
//!
//! 1. **Gemessen**: Vref (oder Airbus-VLS der Landestellung) aus dem
//!    Flugzeug — PMDG-FMC, iniBuilds, FBW, FSS E-Jets, Zibo. Zählt nur, wenn
//!    sie über 50 kt liegt und höchstens 25 kt von der Formel abweicht (ein
//!    kaputter Wert soll nicht als Messung durchgehen).
//! 2. **Kalibriert**: eigener Bezugswert aus euren Messungen, für Muster, bei
//!    denen der FAA-Eintrag durchfällt.
//! 3. **FAA bestätigt**: Formel unten, an echten Messungen geprüft.
//! 4. **FAA ungeprüft**: Wert ohne Urteil (die Anzeige färbt ihn nicht).
//! 5. Sonst keine Vref.
//!
//! Die Überziehgeschwindigkeit aus dem Flugmodell (`DESIGN SPEED VS0`) wird
//! ab dieser Fassung je Landung mitgespeichert, geht aber erst in die
//! Rangfolge ein, wenn sie je Add-on an Messungen bestätigt ist.
//!
//! # Die Formel
//!
//! Die Vref ist ein fester Vielfacher der Überziehgeschwindigkeit, und die
//! wächst mit der Wurzel aus dem Gewicht:
//!
//! `Vref = V_bezug × √(Landegewicht ÷ Bezugsgewicht)`
//!
//! Bezug ist die FAA Aircraft Characteristics Database (Stand Oktober 2024):
//! `Approach_Speed_knot` beim höchsten Landegewicht `MALW_lb`.
//!
//! # Wann ein Muster als bestätigt gilt (Thomas, 06.10.2026)
//!
//! Mindestens 5 Landungen von mindestens 2 Piloten mit gemessener Vref; die
//! Formel liegt im Mittel höchstens 3 kt daneben, ohne Schieflage, und
//! 9 von 10 Landungen liegen innerhalb von 5 kt. Das Toleranzband wird um den
//! gemessenen Fehler (90-%-Wert, aufgerundet) breiter.
//!
//! # Nicht übernommene FAA-Zeilen
//!
//! Ausgeschlossen, wenn das ICAO-Kürzel mehrere Flugzeuge deckt oder das
//! FAA-Landegewicht mehr als 5 % vom höchsten Landegewicht der GSG-Flotte
//! (`phpvmsaw_icao_weights.mlw`) abweicht — dann gehört die FAA-Zahl zu
//! einem anderen Flugzeug als dem, das geflogen wird. Stand 06.10.2026:
//!
//! | Muster | Grund |
//! |---|---|
//! | B462 | FAA-Landegewicht 3674 kg, Flotte 36878 kg (−90 %) |
//! | B737 | FAA-Landegewicht 66043 kg, Flotte 58060 kg (+14 %) |
//! | B738 | an Messungen durchgefallen: 33 Landungen, 4 Piloten, Schieflage +3,3 kt (Klappen 30/40 gemischt), Kalibrierung 90 % ≤ 6,4 kt |
//! | B739 | FAA-Landegewicht 32386 kg, Flotte 71350 kg (−55 %) |
//! | B752 | FAA-Landegewicht 89811 kg, Flotte 95100 kg (−6 %) |
//! | B762 | FAA-Landegewicht 117934 kg, Flotte 129273 kg (−9 %) |
//! | B77L | Kürzel steht für 777F und 777-200LR |
//! | DA40 | FAA-Landegewicht 1092 kg, Flotte 1999 kg (−45 %) |
//! | MU2 | FAA-Landegewicht 5001 kg, Flotte 4240 kg (+18 %) |
//! | P28R | FAA-Landegewicht 1315 kg, Flotte 1247 kg (+5 %) |
//!
//! Die Tabelle wird nur per Release geändert; der monatliche Prüfbericht
//! auf live schlägt Änderungen vor.

/// Wie belastbar der Bezugswert eines Musters ist.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// An Messungen geprüft; `fehler_kt` verbreitert das Toleranzband.
    Bestaetigt { fehler_kt: f32 },
    /// Bezugswert aus eigenen Messungen statt FAA.
    Kalibriert { fehler_kt: f32 },
    /// Nur FAA, noch nicht an Messungen geprüft — Wert ohne Urteil.
    Ungeprueft,
}

/// Vref beim Aufsetzen und wie weit man ihr trauen kann.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VrefErgebnis {
    pub vref_kt: f32,
    /// `pmdg`, `fbw`, `fmc`, `kalibriert`, `faa`, `faa_ungeprueft`.
    pub quelle: &'static str,
    /// Verbreiterung des Toleranzbands in kt. `None` = kein Urteil (Wert
    /// grau zeigen): ungeprüfte Formel oder Klappen nicht in Landestellung.
    pub toleranz_kt: Option<f32>,
    /// Was die Formel gesagt hätte — bei einer Messung der Vergleichswert
    /// für den monatlichen Prüfbericht (so braucht der Server keine eigene
    /// Tabelle).
    pub formel_kt: Option<f32>,
}

/// Untergrenze für eine gemessene Vref — darunter steht im Flugzeug noch
/// nichts drin (PMDG/Zibo melden 0, bis der Pilot die Vref wählt).
const MESSWERT_MIN_KT: f32 = 50.0;
/// Höchstabstand einer Messung zur Formel, damit sie als Messung zählt.
const MESSWERT_MAX_ABSTAND_KT: f32 = 25.0;

/// Vref aus Bezugstabelle und Landegewicht, ohne Messung.
pub fn vref_aus_gewicht(muster: Option<&str>, gewicht_kg: Option<f64>) -> Option<(f32, Status)> {
    let muster = muster?.trim().to_ascii_uppercase();
    let gewicht = gewicht_kg.filter(|w| *w > 0.0)?;
    let (_, v, w0, status) = TABELLE.iter().find(|(code, ..)| *code == muster)?;
    Some(((*v * (gewicht / *w0).sqrt()) as f32, *status))
}

/// Liegen die Klappen in einer der beiden letzten Stufen (Boeing 30/40,
/// Airbus CONF 3/FULL)? `flap_num_positions` zählt die Rasten ohne UP, der
/// höchste Index ist also gleich der Anzahl. `None`, wenn der Sim es nicht
/// meldet.
pub fn landeklappen(index: Option<u8>, anzahl: Option<u8>) -> Option<bool> {
    match (index, anzahl) {
        (Some(i), Some(n)) if n >= 2 => Some(i + 1 >= n),
        _ => None,
    }
}

/// Vref nach der Rangfolge oben.
///
/// `gemessen`: Wert aus dem Flugzeug und seine Quelle (`pmdg`/`fbw`/`fmc`).
/// `klappen_in_landestellung`: `Some(false)` nimmt das Urteil weg, `None`
/// (unbekannt) lässt es stehen.
pub fn vref_bestimmen(
    muster: Option<&str>,
    gewicht_kg: Option<f64>,
    gemessen: Option<(f32, &'static str)>,
    klappen_in_landestellung: Option<bool>,
) -> Option<VrefErgebnis> {
    let formel = vref_aus_gewicht(muster, gewicht_kg);
    let messung = gemessen.filter(|(v, _)| {
        *v > MESSWERT_MIN_KT && formel.is_none_or(|(f, _)| (v - f).abs() <= MESSWERT_MAX_ABSTAND_KT)
    });
    let mut ergebnis = match (messung, formel) {
        (Some((v, quelle)), f) => VrefErgebnis {
            vref_kt: v,
            quelle,
            toleranz_kt: Some(0.0),
            formel_kt: f.map(|(f, _)| f),
        },
        (None, Some((v, Status::Bestaetigt { fehler_kt }))) => VrefErgebnis {
            vref_kt: v,
            quelle: "faa",
            toleranz_kt: Some(fehler_kt),
            formel_kt: Some(v),
        },
        (None, Some((v, Status::Kalibriert { fehler_kt }))) => VrefErgebnis {
            vref_kt: v,
            quelle: "kalibriert",
            toleranz_kt: Some(fehler_kt),
            formel_kt: Some(v),
        },
        (None, Some((v, Status::Ungeprueft))) => VrefErgebnis {
            vref_kt: v,
            quelle: "faa_ungeprueft",
            toleranz_kt: None,
            formel_kt: Some(v),
        },
        (None, None) => return None,
    };
    if klappen_in_landestellung == Some(false) {
        ergebnis.toleranz_kt = None;
    }
    Some(ergebnis)
}

/// ICAO → (Bezugsgeschwindigkeit kt, Bezugsgewicht kg, Status).
///
/// FAA Aircraft Characteristics Database, Oktober 2024, für alle Muster der
/// GSG-Flotte (`phpvmsaircraft.icao`, 06.10.2026) mit beiden Zahlen und ohne
/// Ausschlussgrund. Gewicht = `MALW_lb` × 0,45359237, gerundet.
/// Messungen: PMDG-FMC-Vref aus allen Live-Landungen bis 06.10.2026.
#[rustfmt::skip]
const TABELLE: &[(&str, f64, f64, Status)] = &[
    ("A124", 151.0, 330000.0, Status::Ungeprueft),
    ("A20N", 137.0, 67400.0, Status::Ungeprueft),
    ("A21N", 136.0, 79200.0, Status::Ungeprueft),
    ("A306", 137.0, 137996.0, Status::Ungeprueft),
    ("A319", 126.0, 61000.0, Status::Ungeprueft),
    ("A320", 136.0, 66000.0, Status::Ungeprueft),
    ("A321", 142.0, 77800.0, Status::Ungeprueft),
    ("A332", 136.0, 182000.0, Status::Ungeprueft),
    ("A333", 137.0, 187000.0, Status::Ungeprueft),
    ("A339", 140.0, 190999.0, Status::Ungeprueft),
    ("A343", 145.0, 192000.0, Status::Ungeprueft),
    ("A346", 153.0, 265000.0, Status::Ungeprueft),
    ("A359", 140.0, 207000.0, Status::Ungeprueft),
    ("A35K", 147.0, 233000.0, Status::Ungeprueft),
    ("A388", 138.0, 394000.0, Status::Ungeprueft),
    ("A400", 130.0, 122999.0, Status::Ungeprueft),
    ("AC11", 70.0, 1424.0, Status::Ungeprueft),
    ("AEST", 96.0, 2722.0, Status::Ungeprueft),
    ("AT76", 113.0, 22350.0, Status::Ungeprueft),
    ("B350", 107.0, 6804.0, Status::Ungeprueft),
    ("B38M", 145.0, 69309.0, Status::Ungeprueft),
    ("B735", 128.0, 49895.0, Status::Ungeprueft),
    ("B736", 125.0, 55111.0, Status::Ungeprueft),
    ("B742", 150.0, 285763.0, Status::Ungeprueft),
    ("B744", 157.0, 285763.0, Status::Ungeprueft),
    ("B748", 159.0, 312072.0, Status::Ungeprueft),
    ("B753", 143.0, 101605.0, Status::Ungeprueft),
    ("B763", 140.0, 145150.0, Status::Ungeprueft),
    ("B764", 150.0, 158757.0, Status::Ungeprueft),
    ("B772", 140.0, 213188.0, Status::Ungeprueft),
    ("B77W", 149.0, 251290.0, Status::Bestaetigt { fehler_kt: 2.0 }),  // 5 Landungen, 2 Piloten: Mittel 0,9 kt, 90 % ≤ 1,8 kt
    ("B789", 144.0, 192777.0, Status::Ungeprueft),
    ("B78X", 149.0, 201849.0, Status::Ungeprueft),
    ("BCS3", 135.0, 60600.0, Status::Ungeprueft),
    ("BE24", 78.0, 1247.0, Status::Ungeprueft),
    ("BE35", 72.0, 1542.0, Status::Ungeprueft),
    ("BE36", 77.0, 1656.0, Status::Ungeprueft),
    ("BE58", 95.0, 2449.0, Status::Ungeprueft),
    ("BE60", 98.0, 3073.0, Status::Ungeprueft),
    ("C152", 56.0, 760.0, Status::Ungeprueft),
    ("C172", 62.0, 1111.0, Status::Ungeprueft),
    ("C182", 65.0, 1338.0, Status::Ungeprueft),
    ("C185", 64.0, 1520.0, Status::Ungeprueft),
    ("C208", 79.0, 3538.0, Status::Ungeprueft),
    ("C25A", 114.0, 5228.0, Status::Ungeprueft),
    ("C25C", 111.0, 7103.0, Status::Ungeprueft),
    ("C404", 96.0, 3674.0, Status::Ungeprueft),
    ("C414", 95.0, 3062.0, Status::Ungeprueft),
    ("C525", 108.0, 4491.0, Status::Ungeprueft),
    ("C680", 108.0, 12292.0, Status::Ungeprueft),
    ("C750", 131.0, 14424.0, Status::Ungeprueft),
    ("CL30", 126.0, 15309.0, Status::Ungeprueft),
    ("CL60", 137.0, 17237.0, Status::Ungeprueft),
    ("DA42", 88.0, 1700.0, Status::Ungeprueft),
    ("DH8D", 125.0, 28009.0, Status::Ungeprueft),
    ("E135", 124.0, 18700.0, Status::Ungeprueft),
    ("E145", 124.0, 18700.0, Status::Ungeprueft),
    ("E170", 124.0, 33300.0, Status::Ungeprueft),
    ("E190", 124.0, 43000.0, Status::Ungeprueft),
    ("E195", 135.0, 45800.0, Status::Ungeprueft),
    ("E55P", 116.0, 7568.0, Status::Ungeprueft),
    ("E75L", 126.0, 34000.0, Status::Ungeprueft),
    ("FA50", 124.0, 16200.0, Status::Ungeprueft),
    ("GLF5", 136.0, 34156.0, Status::Ungeprueft),
    ("GLF6", 137.0, 37875.0, Status::Ungeprueft),
    ("H25B", 137.0, 10591.0, Status::Ungeprueft),
    ("HDJT", 111.0, 4472.0, Status::Ungeprueft),
    ("LJ35", 128.0, 6486.0, Status::Ungeprueft),
    ("MD11", 158.0, 195045.0, Status::Ungeprueft),
    ("MD88", 130.0, 58967.0, Status::Ungeprueft),
    ("P180", 121.0, 5216.0, Status::Ungeprueft),
    ("P68", 73.0, 1890.0, Status::Ungeprueft),
    ("PA24", 75.0, 1315.0, Status::Ungeprueft),
    ("PA34", 81.0, 2047.0, Status::Ungeprueft),
    ("PC12", 85.0, 4500.0, Status::Ungeprueft),
    ("RJ85", 122.0, 38555.0, Status::Ungeprueft),
    ("TBM9", 85.0, 3186.0, Status::Ungeprueft),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a380_mit_348_t_rechnet_aus_dem_gewicht_statt_pauschal() {
        // AIB 427, 06.10.2026: 125 kt bei 348 525 kg. Pauschal hiess das
        // −20 kt; mit Gewicht sind es rund −5 kt — ohne Urteil, weil der
        // A380 noch nicht an Messungen bestätigt ist.
        let e = vref_bestimmen(Some("A388"), Some(348_525.0), None, Some(true)).unwrap();
        assert!((e.vref_kt - 129.8).abs() < 0.2, "{}", e.vref_kt);
        assert_eq!(e.quelle, "faa_ungeprueft");
        assert_eq!(e.toleranz_kt, None);
    }

    #[test]
    fn bestaetigtes_muster_bekommt_urteil_mit_breiterem_band() {
        // 777-300ER, PMDG-Messung 245 t → FMC 148 kt.
        let e = vref_bestimmen(Some("B77W"), Some(245_000.0), None, None).unwrap();
        assert!((e.vref_kt - 147.1).abs() < 0.2, "{}", e.vref_kt);
        assert_eq!((e.quelle, e.toleranz_kt), ("faa", Some(2.0)));
    }

    #[test]
    fn messung_geht_vor_und_hat_kein_zusatzband() {
        let e = vref_bestimmen(
            Some("A388"),
            Some(348_525.0),
            Some((131.0, "fmc")),
            Some(true),
        )
        .unwrap();
        assert_eq!(
            (e.vref_kt, e.quelle, e.toleranz_kt),
            (131.0, "fmc", Some(0.0))
        );
        // Die Formel reist für den Prüfbericht mit.
        assert!((e.formel_kt.unwrap() - 129.8).abs() < 0.2);
    }

    #[test]
    fn kaputte_messung_zaehlt_nicht() {
        // 0 = noch nicht eingegeben; 80 kt bei einer A380 = 50 kt neben der Formel.
        for falsch in [0.0, 80.0] {
            let e =
                vref_bestimmen(Some("A388"), Some(348_525.0), Some((falsch, "fmc")), None).unwrap();
            assert_eq!(e.quelle, "faa_ungeprueft", "{falsch}");
        }
        // Ohne Formel gilt nur die Untergrenze.
        let e = vref_bestimmen(Some("ZZZZ"), Some(1.0), Some((132.0, "pmdg")), None).unwrap();
        assert_eq!(e.quelle, "pmdg");
        assert_eq!(
            vref_bestimmen(Some("ZZZZ"), Some(1.0), Some((0.0, "pmdg")), None),
            None
        );
    }

    #[test]
    fn ausgeschlossene_und_unbekannte_muster_haben_keine_vref() {
        for m in ["B738", "B77L", "B737", "B739", "ZZZZ"] {
            assert_eq!(
                vref_bestimmen(Some(m), Some(60_000.0), None, None),
                None,
                "{m}"
            );
        }
        assert_eq!(vref_bestimmen(None, Some(60_000.0), None, None), None);
        assert_eq!(vref_bestimmen(Some("A320"), None, None, None), None);
    }

    #[test]
    fn ohne_landeklappen_kein_urteil() {
        let e = vref_bestimmen(
            Some("B77W"),
            Some(245_000.0),
            Some((148.0, "pmdg")),
            Some(false),
        )
        .unwrap();
        assert_eq!((e.vref_kt, e.toleranz_kt), (148.0, None));
    }

    #[test]
    fn landeklappen_sind_die_letzten_beiden_stufen() {
        // 737: 8 Rasten ohne UP → 30 = 7, 40 = 8. Airbus: 4 → CONF 3 = 3, FULL = 4.
        assert_eq!(landeklappen(Some(7), Some(8)), Some(true));
        assert_eq!(landeklappen(Some(8), Some(8)), Some(true));
        assert_eq!(landeklappen(Some(6), Some(8)), Some(false));
        assert_eq!(landeklappen(Some(3), Some(4)), Some(true));
        assert_eq!(landeklappen(Some(2), Some(4)), Some(false));
        assert_eq!(landeklappen(None, Some(4)), None);
        assert_eq!(landeklappen(Some(1), Some(1)), None);
    }

    #[test]
    fn tabelle_hat_jedes_muster_nur_einmal_und_plausible_werte() {
        let mut gesehen = std::collections::HashSet::new();
        for (m, v, w, _) in TABELLE {
            assert!(gesehen.insert(*m), "doppelt: {m}");
            assert!((50.0..=200.0).contains(v), "{m}: {v}");
            assert!(*w > 500.0, "{m}: {w}");
        }
    }
}
