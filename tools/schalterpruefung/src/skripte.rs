//! Prüfskripte: welche Schalter in welcher Reihenfolge, mit welchen Stellungen.
//!
//! Plattformunabhängig (auf dem Mac getestet).

use serde::Serialize;

/// Schlüsselwort-Gruppen für die Rangfolge der Kandidaten. Ein LVar-Name, der
/// eines davon enthält, rückt nach vorne.
pub const KW_STROBE: &[&str] = &["STROBE", "STRB"];
pub const KW_GURT: &[&str] = &["BELT", "SEAT", "SIGNS"];
pub const KW_BREMSE: &[&str] = &["BRK", "BRAKE", "ABRK"];
pub const KW_XPDR: &[&str] = &["XPDR", "TCAS", "SQWK", "TRANSPONDER"];
pub const KW_SPOILER: &[&str] = &["SPOIL", "SPD", "SPEEDBRAKE", "SPEED_BRAKE"];
pub const KW_APU: &[&str] = &["APU"];
pub const KW_BEACON: &[&str] = &["BEACON", "BCN"];
pub const KW_NAV: &[&str] = &["NAV"];
pub const KW_LANDELICHT: &[&str] = &["LAND", "LDG"];
pub const KW_PARKBREMSE: &[&str] = &["PARK", "BRK", "BRAKE"];

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Schalter {
    /// So heißt der Schalter in der Anweisung an den Piloten.
    pub name: String,
    /// Feste Stellungen in Prüfreihenfolge. Leer = freie Stellungen: der Pilot
    /// benennt jede Stellung selbst (allgemeines Skript).
    pub stellungen: Vec<String>,
    pub schluesselwoerter: Vec<String>,
}

impl Schalter {
    fn neu(name: &str, stellungen: &[&str], kw: &[&str]) -> Self {
        Self {
            name: name.to_string(),
            stellungen: stellungen.iter().map(|s| s.to_string()).collect(),
            schluesselwoerter: kw.iter().map(|s| s.to_string()).collect(),
        }
    }
    fn frei(name: &str, kw: &[&str]) -> Self {
        Self::neu(name, &[], kw)
    }
    pub fn ist_frei(&self) -> bool {
        self.stellungen.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Skript {
    pub kennung: &'static str,
    pub name: &'static str,
    pub schalter: Vec<Schalter>,
}

pub fn a380() -> Skript {
    Skript {
        kennung: "a380",
        name: "iniBuilds A380",
        schalter: vec![
            Schalter::neu("STROBE-Lichtschalter", &["OFF", "AUTO", "ON"], KW_STROBE),
            Schalter::neu(
                "Anschnallzeichen (SEAT BELTS)",
                &["OFF", "AUTO", "ON"],
                KW_GURT,
            ),
            Schalter::neu(
                "AUTOBRAKE-Drehschalter",
                &["DISARM", "BTV", "LO", "L2", "L3", "HI"],
                KW_BREMSE,
            ),
            Schalter::neu(
                "RTO-Taste (Autobrake)",
                &["aus", "gedrückt"],
                &["RTO", "BRK", "BRAKE", "ABRK"],
            ),
            Schalter::neu(
                "Transponder/TCAS-Wahlschalter",
                &["STBY", "TA", "TA/RA"],
                KW_XPDR,
            ),
            Schalter::neu("Spoiler-Hebel", &["eingefahren", "ARMED"], KW_SPOILER),
            Schalter::neu("APU MASTER-Taste", &["aus", "an"], KW_APU),
        ],
    }
}

pub fn a350() -> Skript {
    Skript {
        kennung: "a350",
        name: "iniBuilds A350",
        schalter: vec![
            Schalter::neu(
                "Autobrake im OIS",
                &["OFF", "LO", "2", "3", "HI", "BTV"],
                KW_BREMSE,
            ),
            Schalter::neu(
                "Taste LDG AUTO BRK",
                &["aus", "an"],
                &["BRK", "BRAKE", "ABRK", "LDG"],
            ),
            Schalter::neu(
                "Anschnallzeichen (SEAT BELTS)",
                &["OFF", "AUTO", "ON"],
                KW_GURT,
            ),
            Schalter::neu("STROBE-Lichtschalter", &["OFF", "AUTO", "ON"], KW_STROBE),
        ],
    }
}

pub fn a330() -> Skript {
    Skript {
        kennung: "a330",
        name: "iniBuilds A330 (MSFS 2024)",
        schalter: vec![
            Schalter::neu("STROBE-Lichtschalter", &["OFF", "AUTO", "ON"], KW_STROBE),
            Schalter::neu(
                "Anschnallzeichen (SEAT BELTS)",
                &["OFF", "AUTO", "ON"],
                KW_GURT,
            ),
            Schalter::neu("AUTOBRAKE", &["OFF", "LO", "MED", "MAX"], KW_BREMSE),
            Schalter::neu("Transponder (ATC)", &["STBY", "AUTO", "ON"], KW_XPDR),
            Schalter::neu("TCAS-Wahlschalter", &["STBY", "TA", "TA-RA"], KW_XPDR),
            Schalter::neu("Spoiler-Hebel", &["eingefahren", "ARMED"], KW_SPOILER),
            Schalter::neu("APU MASTER-Taste", &["aus", "an"], KW_APU),
            Schalter::neu("BEACON-Lichtschalter", &["aus", "an"], KW_BEACON),
        ],
    }
}

pub fn a220() -> Skript {
    Skript {
        kennung: "a220",
        name: "Synaptic A220",
        schalter: vec![
            Schalter::neu(
                "Transponder-Wahlschalter",
                &["STBY", "ALT ON", "ALT OFF", "TA ONLY", "TA/RA"],
                KW_XPDR,
            ),
            // „ARMED falls vorhanden": Hat der Hebel keine ARMED-Raste, legt
            // der Pilot ihn einfach auf eingefahren — die Auswertung meldet
            // dann ehrlich „kein Unterschied".
            Schalter::neu(
                "Spoiler-Hebel",
                &[
                    "eingefahren",
                    "ARMED (falls vorhanden, sonst eingefahren lassen)",
                ],
                KW_SPOILER,
            ),
            Schalter::neu("BEACON-Lichtschalter", &["aus", "an"], KW_BEACON),
        ],
    }
}

pub fn allgemein() -> Skript {
    Skript {
        kennung: "allgemein",
        name: "Allgemeines Skript (jedes Flugzeug)",
        schalter: vec![
            Schalter::frei("BEACON-Licht", KW_BEACON),
            Schalter::frei("STROBE-Licht", KW_STROBE),
            Schalter::frei("NAV-Licht (Positionslichter)", KW_NAV),
            Schalter::frei("Landelicht", KW_LANDELICHT),
            Schalter::frei("Anschnallzeichen", KW_GURT),
            Schalter::frei("Transponder", KW_XPDR),
            Schalter::frei("Autobrake", KW_BREMSE),
            Schalter::frei("Spoiler-Hebel", KW_SPOILER),
            Schalter::frei("APU", KW_APU),
            Schalter::frei("Parkbremse", KW_PARKBREMSE),
        ],
    }
}

// Zusatz-Namenslisten: MobiFlight listet nur LVar-IDs 0..999 (Module.cpp
// Z. 228), iniBuilds- und Synaptic-Muster haben mehr. Diese Namen werden
// zusätzlich direkt abonniert. Herkunft: A350-WASM-Strings + Behavior-XML
// (ini.txt), lesbare Bruchstücke der A380-Archive (a380_frag.txt),
// Synaptic-A220-Paket (a220.txt). Namen, die es im geladenen Flugzeug nicht
// gibt, liefern nur konstant 0 und werden nie Kandidat.
const NAMEN_INI: &str = include_str!("../namen/ini.txt");
const NAMEN_A380_FRAG: &str = include_str!("../namen/a380_frag.txt");
const NAMEN_A220: &str = include_str!("../namen/a220.txt");

/// Zusätzlich direkt zu abonnierende LVar-Namen (ohne `L:`) je Skript.
pub fn zusatzliste(kennung: &str) -> Vec<String> {
    let roh = match kennung {
        "a350" => NAMEN_INI.to_string(),
        "a380" | "a330" => format!("{NAMEN_INI}\n{NAMEN_A380_FRAG}"),
        "a220" => NAMEN_A220.to_string(),
        _ => String::new(),
    };
    crate::mobiflight::zusatz_filtern(&roh)
}

/// Alle Skripte in Menü-Reihenfolge (das allgemeine zuletzt).
pub fn alle() -> Vec<Skript> {
    vec![a380(), a350(), a330(), a220(), allgemein()]
}

/// Skript anhand des Flugzeugtitels (`TITLE`) und optional der ICAO-Kennung.
/// `None` = kein passendes Spezialskript → Menü zeigen.
pub fn fuer_titel(titel: &str, icao: Option<&str>) -> Option<Skript> {
    let t = titel.to_ascii_uppercase();
    let i = icao.unwrap_or("").to_ascii_uppercase();
    let hat = |s: &str| t.contains(s);
    if hat("A380") || i == "A388" {
        Some(a380())
    } else if hat("A350") || i == "A359" || i == "A35K" {
        Some(a350())
    } else if hat("A330") || i.starts_with("A33") {
        Some(a330())
    } else if hat("A220") || hat("BD-500") || hat("BD500") || i == "BCS1" || i == "BCS3" {
        Some(a220())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titel_waehlt_das_richtige_skript() {
        assert_eq!(
            fuer_titel("A380-800 RR Basic", None).unwrap().kennung,
            "a380"
        );
        assert_eq!(
            fuer_titel("A350-900 (No Cabin)", None).unwrap().kennung,
            "a350"
        );
        assert_eq!(fuer_titel("A330-300 (RR)", None).unwrap().kennung, "a330");
        assert_eq!(
            fuer_titel("Synaptic A220-300 Air Baltic", None)
                .unwrap()
                .kennung,
            "a220"
        );
        assert_eq!(fuer_titel("a220-100 swiss", None).unwrap().kennung, "a220");
    }

    #[test]
    fn unbekannter_titel_fuehrt_ins_menue() {
        assert!(fuer_titel("PMDG 737-800 Lufthansa", Some("B738")).is_none());
        assert!(fuer_titel("", None).is_none());
    }

    #[test]
    fn icao_hilft_wenn_der_titel_nichts_sagt() {
        assert_eq!(
            fuer_titel("Livery XYZ", Some("A388")).unwrap().kennung,
            "a380"
        );
        assert_eq!(
            fuer_titel("Livery XYZ", Some("BCS3")).unwrap().kennung,
            "a220"
        );
    }

    #[test]
    fn skripte_haben_die_verlangten_stellungen() {
        let a = a380();
        assert_eq!(a.schalter.len(), 7);
        assert_eq!(
            a.schalter[2].stellungen,
            vec!["DISARM", "BTV", "LO", "L2", "L3", "HI"]
        );
        assert_eq!(
            a350().schalter[0].stellungen,
            vec!["OFF", "LO", "2", "3", "HI", "BTV"]
        );
        assert_eq!(a330().schalter.len(), 8);
        assert_eq!(a220().schalter[0].stellungen.len(), 5);
        let g = allgemein();
        assert_eq!(g.schalter.len(), 10);
        assert!(g.schalter.iter().all(|s| s.ist_frei()));
        // Jedes Skript hat Schlüsselwörter für jeden Schalter.
        for s in alle() {
            assert!(
                s.schalter.iter().all(|x| !x.schluesselwoerter.is_empty()),
                "{}",
                s.name
            );
        }
    }

    #[test]
    fn zusatzlisten_je_skript() {
        let a380 = zusatzliste(fuer_titel("A380-800 RR Basic", None).unwrap().kennung);
        assert!(a380.iter().any(|n| n == "INI_LIGHTS_STROBE"));
        // Nur in a380_frag.txt, nicht in ini.txt:
        assert!(!NAMEN_INI.lines().any(|z| z.trim() == "INI_A380_COPILOT"));
        assert!(a380.iter().any(|n| n == "INI_A380_COPILOT"));
        let a350 = zusatzliste("a350");
        assert!(a350.iter().any(|n| n == "INI_LIGHTS_STROBE"));
        assert!(!a350.iter().any(|n| n == "INI_A380_COPILOT"));
        assert_eq!(zusatzliste("a330"), a380);
        let a220 = zusatzliste("a220");
        assert!(a220.len() > 400, "{}", a220.len());
        assert!(
            a220.iter().all(|n| n.starts_with("A22X")),
            "Fremdnamen in a220.txt"
        );
        assert!(a220.iter().any(|n| n.contains(' ')));
        assert!(zusatzliste("allgemein").is_empty());
        // Alles abonnierbar, keine Dubletten.
        for l in [&a380, &a220] {
            assert!(l.iter().all(|n| crate::mobiflight::lvar_code(n).is_some()));
            let mut u: Vec<String> = l.iter().map(|n| n.to_ascii_uppercase()).collect();
            u.sort();
            u.dedup();
            assert_eq!(u.len(), l.len());
        }
    }
}
