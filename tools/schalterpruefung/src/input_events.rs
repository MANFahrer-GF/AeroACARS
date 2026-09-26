//! SimConnect-Input-Events (MSFS 2024, „B:"-Variablen) — plattformunabhängige
//! Deutung der Rohnachrichten. iniBuilds hält z. B. beim A380 die Stellung
//! mancher Schalter NUR als Input-Event (B:AIRLINER_…), nicht als LVar.
//!
//! Layout aus dem vendorten Header gelesen
//! (`client/src-tauri/crates/sim-msfs/ffi/include/SimConnect.h`), nicht
//! geraten. Alle Strukturen stehen dort unter `#pragma pack(push, 1)`
//! (Z. 450–900), also ohne Füllbytes:
//!
//! * `SIMCONNECT_RECV` (Kopf, 12 Byte): dwSize, dwVersion, dwID.
//! * Z. 643–649 `SIMCONNECT_RECV_LIST_TEMPLATE` : RECV + dwRequestID,
//!   dwArraySize, dwEntryNumber, dwOutOf → 28 Byte.
//! * Z. 826–831 `SIMCONNECT_INPUT_EVENT_DESCRIPTOR`: char Name[64],
//!   UINT64 Hash, SIMCONNECT_INPUT_EVENT_TYPE eType (DWORD, Z. 346) → 76 Byte.
//! * Z. 833–836 `SIMCONNECT_RECV_ENUMERATE_INPUT_EVENTS` : LIST_TEMPLATE +
//!   rgData[dwArraySize] Deskriptoren ab Byte 28.
//! * Z. 838–843 `SIMCONNECT_RECV_GET_INPUT_EVENT` : RECV + dwRequestID +
//!   eType + Value (Z. 77: `SIMCONNECT_DATAV` ist nur Platzhalter; der Wert
//!   beginnt an Byte 20 — double bzw. NUL-terminierter Text).
//! * Z. 845–850 `SIMCONNECT_RECV_SUBSCRIBE_INPUT_EVENT` : RECV + UINT64 Hash +
//!   eType + Value ab Byte 24.
//! * Z. 346–350: eType 0 = DOUBLE, 1 = STRING.
//!
//! Nur Zahlen-Events werden beobachtet; Text-Events werden gezählt und
//! übergangen. `SimConnect_SetInputEvent` wird nie benutzt.

use std::collections::{BTreeSet, HashSet};

pub const KOPF: usize = 12;
pub const LISTE_KOPF: usize = 28;
pub const DESKRIPTOR: usize = 76;
pub const TYP_DOUBLE: u32 = 0;
pub const TYP_STRING: u32 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct Deskriptor {
    pub name: String,
    pub hash: u64,
    pub typ: u32,
}

fn u32_bei(b: &[u8], o: usize) -> Option<u32> {
    b.get(o..o + 4)
        .map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]))
}
fn u64_bei(b: &[u8], o: usize) -> Option<u64> {
    b.get(o..o + 8).map(|x| {
        let mut a = [0u8; 8];
        a.copy_from_slice(x);
        u64::from_le_bytes(a)
    })
}
fn f64_bei(b: &[u8], o: usize) -> Option<f64> {
    u64_bei(b, o).map(f64::from_bits)
}
fn text(b: &[u8]) -> String {
    let ende = b.iter().position(|c| *c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..ende]).trim().to_string()
}

/// Ein Teil der Liste: (RequestID, EntryNumber, OutOf, Deskriptoren).
pub fn enumerate_deuten(roh: &[u8]) -> Option<(u32, u32, u32, Vec<Deskriptor>)> {
    let req = u32_bei(roh, KOPF)?;
    let anzahl = u32_bei(roh, 16)? as usize;
    let nr = u32_bei(roh, 20)?;
    let von = u32_bei(roh, 24)?;
    let mut v = Vec::with_capacity(anzahl);
    for i in 0..anzahl {
        let o = LISTE_KOPF + i * DESKRIPTOR;
        let Some(d) = roh.get(o..o + DESKRIPTOR) else {
            break; // abgeschnitten: nur Vollständiges übernehmen
        };
        v.push(Deskriptor {
            name: text(&d[..64]),
            hash: u64_bei(d, 64)?,
            typ: u32_bei(d, 72)?,
        });
    }
    Some((req, nr, von, v))
}

/// Antwort auf GetInputEvent: (RequestID, Zahlenwert oder None bei Text).
pub fn get_deuten(roh: &[u8]) -> Option<(u32, Option<f64>)> {
    let req = u32_bei(roh, KOPF)?;
    let typ = u32_bei(roh, 16)?;
    let w = if typ == TYP_DOUBLE {
        f64_bei(roh, 20)
    } else {
        None
    };
    Some((req, w))
}

/// Abo-Meldung: (Hash, Zahlenwert oder None bei Text).
pub fn abo_deuten(roh: &[u8]) -> Option<(u64, Option<f64>)> {
    let hash = u64_bei(roh, KOPF)?;
    let typ = u32_bei(roh, 20)?;
    let w = if typ == TYP_DOUBLE {
        f64_bei(roh, 24)
    } else {
        None
    };
    Some((hash, w))
}

/// Sammelt die stückweise gelieferte Liste, bis alle `OutOf` Teile da sind.
#[derive(Debug, Default)]
pub struct ListenSammler {
    teile: BTreeSet<u32>,
    von: Option<u32>,
    hashes: HashSet<u64>,
    pub events: Vec<Deskriptor>,
    pub text_events: usize,
}

impl ListenSammler {
    pub fn aufnehmen(&mut self, nr: u32, von: u32, d: Vec<Deskriptor>) {
        self.von = Some(von);
        if !self.teile.insert(nr) {
            return; // Teil doppelt geliefert
        }
        for x in d {
            if x.name.is_empty() || !self.hashes.insert(x.hash) {
                continue;
            }
            if x.typ == TYP_DOUBLE {
                self.events.push(x);
            } else if x.typ == TYP_STRING {
                self.text_events += 1;
            }
        }
    }
    pub fn fertig(&self) -> bool {
        matches!(self.von, Some(v) if self.teile.len() as u32 >= v)
    }
    /// Stabile, lesbare Reihenfolge.
    pub fn sortiert(mut self) -> (Vec<Deskriptor>, usize) {
        self.events.sort_by(|a, b| a.name.cmp(&b.name));
        (self.events, self.text_events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kopf(id: u32, groesse: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&(groesse as u32).to_le_bytes());
        v.extend_from_slice(&6u32.to_le_bytes());
        v.extend_from_slice(&id.to_le_bytes());
        v
    }

    fn deskriptor(name: &str, hash: u64, typ: u32) -> Vec<u8> {
        let mut n = [0u8; 64];
        n[..name.len()].copy_from_slice(name.as_bytes());
        let mut v = n.to_vec();
        v.extend_from_slice(&hash.to_le_bytes());
        v.extend_from_slice(&typ.to_le_bytes());
        assert_eq!(v.len(), DESKRIPTOR);
        v
    }

    fn liste(req: u32, nr: u32, von: u32, d: &[(&str, u64, u32)]) -> Vec<u8> {
        let mut v = kopf(40, 0);
        v.extend_from_slice(&req.to_le_bytes());
        v.extend_from_slice(&(d.len() as u32).to_le_bytes());
        v.extend_from_slice(&nr.to_le_bytes());
        v.extend_from_slice(&von.to_le_bytes());
        assert_eq!(v.len(), LISTE_KOPF);
        for (n, h, t) in d {
            v.extend(deskriptor(n, *h, *t));
        }
        v
    }

    #[test]
    fn liste_in_teilen_bis_vollstaendig() {
        let mut s = ListenSammler::default();
        let t1 = liste(
            7,
            0,
            2,
            &[
                ("AIRLINER_LIGHTS_EXT_STROBE", 11, TYP_DOUBLE),
                ("AIRLINER_TEXT", 12, TYP_STRING),
            ],
        );
        let (req, nr, von, d) = enumerate_deuten(&t1).unwrap();
        assert_eq!((req, nr, von, d.len()), (7, 0, 2, 2));
        s.aufnehmen(nr, von, d.clone());
        assert!(!s.fertig());
        s.aufnehmen(nr, von, d); // doppelt: ignorieren
        let t2 = liste(
            7,
            1,
            2,
            &[
                ("AIRLINER_SIGNS_SEAT_BELTS", 13, TYP_DOUBLE),
                ("AIRLINER_LIGHTS_EXT_STROBE", 11, TYP_DOUBLE), // Hash doppelt
            ],
        );
        let (_, nr, von, d) = enumerate_deuten(&t2).unwrap();
        s.aufnehmen(nr, von, d);
        assert!(s.fertig());
        let (ev, texte) = s.sortiert();
        let namen: Vec<&str> = ev.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(
            namen,
            vec!["AIRLINER_LIGHTS_EXT_STROBE", "AIRLINER_SIGNS_SEAT_BELTS"]
        );
        assert_eq!(texte, 1);
        assert_eq!(ev[1].hash, 13);
    }

    #[test]
    fn abgeschnittene_liste_liefert_nur_vollstaendiges() {
        let mut t = liste(1, 0, 1, &[("A_B", 1, 0), ("C_D", 2, 0)]);
        t.truncate(LISTE_KOPF + DESKRIPTOR + 10);
        let (_, _, _, d) = enumerate_deuten(&t).unwrap();
        assert_eq!(d.len(), 1);
        assert!(enumerate_deuten(&t[..20]).is_none());
    }

    #[test]
    fn get_und_abo_werte() {
        let mut g = kopf(41, 0);
        g.extend_from_slice(&99u32.to_le_bytes());
        g.extend_from_slice(&TYP_DOUBLE.to_le_bytes());
        g.extend_from_slice(&2.0f64.to_le_bytes());
        assert_eq!(get_deuten(&g), Some((99, Some(2.0))));
        let mut gs = kopf(41, 0);
        gs.extend_from_slice(&5u32.to_le_bytes());
        gs.extend_from_slice(&TYP_STRING.to_le_bytes());
        gs.extend_from_slice(b"AUTO\0");
        assert_eq!(get_deuten(&gs), Some((5, None)));
        let mut a = kopf(42, 0);
        a.extend_from_slice(&0xABCDu64.to_le_bytes());
        a.extend_from_slice(&TYP_DOUBLE.to_le_bytes());
        a.extend_from_slice(&1.0f64.to_le_bytes());
        assert_eq!(abo_deuten(&a), Some((0xABCD, Some(1.0))));
        assert!(abo_deuten(&a[..16]).is_none());
    }
}
