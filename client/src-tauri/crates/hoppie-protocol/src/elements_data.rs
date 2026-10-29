//! The actual GOLD uplink (UM) / downlink (DM) element table.
//!
//! Seit 3.0 (Etappe 1b, P-3.57) liegt die Tabelle im Laufzeit-Paket unter
//! `tabellen/cpdlc.json`; die frueheren Konstanten `HOPPIE_*`/`GOLD_*_TABLE`
//! sind dort die Abschnitte in dieser Reihenfolge. Herkunft und Patches unten
//! gelten fuer den Inhalt der Datei.
//!
//! Two sources, chained together by [`crate::elements::um_table`]/
//! [`crate::elements::dm_table`]:
//!
//! - `HOPPIE_UM_TABLE`/`HOPPIE_DM_TABLE`: a handful of Hoppie-network-
//!   specific rows with no GOLD equivalent — the network's simplified
//!   plain-text logon handshake (real ATN/FANS CPDLC logon is a binary
//!   application-layer handshake, not a text message).
//! - `GOLD_UM_TABLE`/`GOLD_DM_TABLE`: the real ICAO GOLD message-
//!   element catalog (198 UM + 89 DM = 287 rows), DATA-transcribed from
//!   `skiselkov/libcpdlc`'s `cpdlc_msg_infos[]` (`src/cpdlc_infos.c`,
//!   MIT, C) via a one-off Python extraction script — not hand-typed,
//!   and not copying libcpdlc's code (which is ASN.1/offsetof-based C
//!   struct wiring irrelevant here). Each row's `id` is the real GOLD
//!   element number (e.g. `"UM74"`, `"DM67b"`); `template`/
//!   `placeholders`/`response` are transcribed from libcpdlc's
//!   `.text`/`.args`/`.resp` fields.
//!
//! ## Five template patches
//!
//! libcpdlc's own `.text` field is a human-readable display string
//! and, for five elements, is genuinely missing a trailing placeholder
//! that its `.args` array (the structural field) declares — e.g. UM63
//! declares 4 args (time, position, altitude, speed) but its `.text`
//! only shows three `[...]` markers. Left as-is, `resolve()` would
//! silently drop the extra value instead of erroring, since it only
//! checks placeholder *count*, not how many actually appear in the
//! template. Five templates (`UM63`, `UM91`, `DM16`, `DM17`, `DM27`)
//! are therefore patched to include the missing `@N`, phrased to match
//! the closest structurally-identical sibling element in the same
//! table (documented at each patched entry below) — not a guess out of
//! nowhere, but not verbatim libcpdlc text either.
//!
//! Every `ElementSpec` is `'static` data — zero per-element match arms,
//! see `elements.rs`'s module docs for why.

use crate::cpdlc::ResponseRequirement;
use crate::elements::{Direction, ElementSpec, PlaceholderKind};

/// Kennungen, die der Code fest benutzt (Logon/Logoff, Standby, Antworten,
/// Freitext, Logon-Antworten; teils mit `expect`). Ein Stand ohne eine davon
/// wird abgewiesen, statt spaeter im Flug abzustuerzen.
const PFLICHT: &[&str] = &[
    "UM_LOGON_ACCEPTED",
    "UM0",
    "UM160",
    "UM161",
    "DM_REQUEST_LOGON",
    "DM_LOGOFF",
    "DM0",
    "DM1",
    "DM2",
    "DM22",
    "DM67",
    "DM67b",
];

/// Eingebauter Stand = Datei im Paketordner (P-3.57, eine Quelle).
pub const EINGEBAUT: &str = include_str!("../../../../../bundle/tabellen/cpdlc.json");

/// Woher die Tabelle kommt. Der Client meldet beim Start eine Quelle an, die
/// aus SEINEM einen Paketstand liest (P-3.02); ohne Anmeldung (Tests dieses
/// Crates) gilt der eingebaute Stand.
static QUELLE: std::sync::OnceLock<fn() -> &'static [ElementSpec]> = std::sync::OnceLock::new();

/// Alle Elemente in Dateireihenfolge (Hoppie-UM, GOLD-UM, Hoppie-DM,
/// GOLD-DM); [`crate::elements::um_table`]/[`crate::elements::dm_table`]
/// filtern nach Richtung.
pub fn elemente() -> &'static [ElementSpec] {
    match QUELLE.get() {
        Some(quelle) => quelle(),
        None => {
            static EIGEN: std::sync::OnceLock<&'static [ElementSpec]> = std::sync::OnceLock::new();
            EIGEN.get_or_init(|| tabelle_lesen(EINGEBAUT).expect("eingebaute CPDLC-Tabelle"))
        }
    }
}

/// Meldet die Quelle an (einmal je Lauf). `false`, wenn schon eine gilt.
pub fn tabelle_quelle_setzen(quelle: fn() -> &'static [ElementSpec]) -> bool {
    QUELLE.set(quelle).is_ok()
}

/// Prueft `tabellen/cpdlc.json` wie [`tabelle_lesen`], ohne etwas dauerhaft
/// abzulegen (Selbsttest eines heruntergeladenen Stands).
pub fn tabelle_pruefen(json: &str) -> Result<(), String> {
    datei_lesen(json).map(|_| ())
}

/// Liest `tabellen/cpdlc.json` (fail-closed, P-3.65). Das Ergebnis lebt bis
/// Programmende (einmal je Lauf).
pub fn tabelle_lesen(json: &str) -> Result<&'static [ElementSpec], String> {
    fn s(x: String) -> &'static str {
        Box::leak(x.into_boxed_str())
    }
    let zeilen: Vec<ElementSpec> = datei_lesen(json)?
        .elemente
        .into_iter()
        .map(|z| ElementSpec {
            id: s(z.id),
            direction: z.direction,
            template: s(z.template),
            placeholders: Box::leak(z.placeholders.into()),
            response: z.response,
        })
        .collect();
    Ok(Box::leak(zeilen.into()))
}

// Dateiform von `tabellen/cpdlc.json`.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Datei {
    format: u32,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
    elemente: Vec<ZeileDatei>,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ZeileDatei {
    id: String,
    direction: Direction,
    template: String,
    placeholders: Vec<PlaceholderKind>,
    response: ResponseRequirement,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
}

/// Liest und prueft die Regeln, die frueher nur Tests der festen Tabelle
/// waren — jetzt muss jeder geladene Stand sie erfuellen: eindeutige Kennung,
/// Richtung passend zum Praefix UM/DM, je deklariertem Platzhalter genau ein
/// `@N` in der Vorlage (sonst verschluckt `resolve()` Werte, siehe oben).
fn datei_lesen(json: &str) -> Result<Datei, String> {
    let d: Datei = serde_json::from_str(json).map_err(|e| format!("tabellen/cpdlc.json: {e}"))?;
    if d.format != 1 {
        return Err(format!("tabellen/cpdlc.json: Format {} unbekannt", d.format));
    }
    let mut ids = std::collections::HashSet::new();
    for z in &d.elemente {
        if !ids.insert(z.id.as_str()) {
            return Err(format!("tabellen/cpdlc.json: {} doppelt", z.id));
        }
        let richtig = match z.direction {
            Direction::Uplink => z.id.starts_with("UM"),
            Direction::Downlink => z.id.starts_with("DM"),
        };
        if !richtig {
            return Err(format!("tabellen/cpdlc.json: {} passt nicht zu {:?}", z.id, z.direction));
        }
        // Jede Marke `@N` genau einmal, N = 1..=Anzahl; kein `@` sonst
        // (Codex 29.10.: `@99` oder doppelte Marken gingen sonst auf den Wire).
        let mut marken = Vec::new();
        let mut rest = z.template.as_str();
        while let Some(i) = rest.find('@') {
            rest = &rest[i + 1..];
            let ziffern = rest.bytes().take_while(u8::is_ascii_digit).count();
            marken.push(rest[..ziffern].parse::<usize>().unwrap_or(0));
            rest = &rest[ziffern..];
        }
        marken.sort_unstable();
        if z.template.is_empty() || !marken.iter().copied().eq(1..=z.placeholders.len()) {
            return Err(format!("tabellen/cpdlc.json: {} Vorlage passt nicht zu den Platzhaltern", z.id));
        }
    }
    if let Some(fehlt) = PFLICHT.iter().find(|p| !ids.contains(**p)) {
        return Err(format!("tabellen/cpdlc.json: Pflichtelement {fehlt} fehlt"));
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::{dm_table, um_table};

    /// Die Tabelle im Paket ist genau die der Konstanten HOPPIE_UM/GOLD_UM/
    /// HOPPIE_DM/GOLD_DM_TABLE aus 2.1.24 (Etappe 1b): Abdruck (FNV-1a) der
    /// Debug-Ausgabe, vor dem Entfernen gegen sie verglichen. Aendert sich
    /// die Tabelle bewusst, wird der Abdruck mit Begruendung nachgezogen.
    #[test]
    fn tabelle_wie_2_1_24() {
        let text = format!("{:?}", tabelle_lesen(EINGEBAUT).unwrap());
        let mut h: u64 = 0xcbf29ce484222325;
        for b in text.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        assert_eq!((h, text.len()), (0x0c55f1abcac4f622, 38414));
    }

    #[test]
    fn eingebaute_tabelle_hat_beide_richtungen_in_voller_groesse() {
        // Loose sanity bound wie frueher fuer GOLD_UM/DM_TABLE.
        assert!(um_table().count() > 150);
        assert!(dm_table().count() > 60);
        assert!(um_table().all(|e| e.direction == Direction::Uplink));
        assert!(dm_table().all(|e| e.direction == Direction::Downlink));
    }

    fn verbogen(f: impl FnOnce(&mut serde_json::Value)) -> Result<(), String> {
        let mut v: serde_json::Value = serde_json::from_str(EINGEBAUT).unwrap();
        f(&mut v);
        tabelle_pruefen(&v.to_string())
    }

    /// Jede Regel, die frueher nur ein Test der festen Tabelle war, weist
    /// jetzt einen geladenen Stand ab.
    #[test]
    fn regelverstoss_ist_ein_lesefehler() {
        assert_eq!(verbogen(|_| {}), Ok(()));
        // doppelte Kennung
        assert!(verbogen(|v| {
            let erste = v["elemente"][0].clone();
            v["elemente"].as_array_mut().unwrap().push(erste);
        })
        .is_err());
        // Richtung passt nicht zum Praefix
        assert!(verbogen(|v| v["elemente"][0]["direction"] = "Downlink".into()).is_err());
        // Platzhalter ohne @N in der Vorlage (der Fehler der fuenf Patches)
        assert!(verbogen(|v| {
            let z = v["elemente"].as_array_mut().unwrap().iter_mut().find(|z| z["id"] == "UM63").unwrap();
            z["template"] = "AT @1 CROSS @2 AT AND MAINTAIN @3".into();
        })
        .is_err());
        // zusaetzliche, doppelte oder blanke Marke
        for vorlage in ["AT @1 CROSS @2 AT AND MAINTAIN @3 AT @4 @99", "AT @1 CROSS @2 AT @2 MAINTAIN @3 AT @4", "AT @1 CROSS @2 AT @ MAINTAIN @3 AT @4"] {
            assert!(verbogen(|v| {
                let z = v["elemente"].as_array_mut().unwrap().iter_mut().find(|z| z["id"] == "UM63").unwrap();
                z["template"] = vorlage.into();
            })
            .is_err(), "{vorlage}");
        }
        // Pflichtelement fehlt (Logoff wird mit `expect` gesucht)
        assert!(verbogen(|v| v["elemente"].as_array_mut().unwrap().retain(|z| z["id"] != "DM_LOGOFF")).is_err());
        // fremdes Feld, unbekanntes Format
        assert!(verbogen(|v| v["elemente"][0]["extra"] = 1.into()).is_err());
        assert!(verbogen(|v| v["format"] = 2.into()).is_err());
    }
}
