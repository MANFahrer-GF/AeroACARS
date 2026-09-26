//! SimConnect-Input-Events (MSFS 2024, „B:"-Variablen) — Deutung der
//! Rohnachrichten und Ablauf „aufzählen → abonnieren → Werte halten".
//!
//! Warum: iniBuilds hält beim A380/A350 (MSFS-2024-Fassungen) die Stellung
//! mancher Schalter NUR als Input-Event (`B:AIRLINER_…`), nicht als LVar.
//! Gemessen am 26.09.2026 im echten MSFS 2024 mit dem Werkzeug
//! `tools/schalterpruefung` (Zweig gleichen Namens), dessen Deutung hier
//! übernommen ist.
//!
//! Layout aus dem vendorten Header (`ffi/include/SimConnect.h`), alle
//! Strukturen unter `#pragma pack(push, 1)`, also ohne Füllbytes:
//!
//! * `SIMCONNECT_RECV` (Kopf, 12 Byte): dwSize, dwVersion, dwID.
//! * `SIMCONNECT_RECV_LIST_TEMPLATE`: Kopf + dwRequestID, dwArraySize,
//!   dwEntryNumber, dwOutOf → 28 Byte.
//! * `SIMCONNECT_INPUT_EVENT_DESCRIPTOR` (Z. 826): char Name[64], UINT64
//!   Hash, eType (DWORD) → 76 Byte.
//! * `SIMCONNECT_RECV_ENUMERATE_INPUT_EVENTS` (Z. 833): Listenkopf +
//!   Deskriptoren ab Byte 28.
//! * `SIMCONNECT_RECV_GET_INPUT_EVENT` (Z. 838): Kopf + dwRequestID + eType
//!   + Wert ab Byte 20.
//! * `SIMCONNECT_RECV_SUBSCRIBE_INPUT_EVENT` (Z. 845): Kopf + UINT64 Hash +
//!   eType + Wert ab Byte 24.
//! * eType 0 = DOUBLE, 1 = STRING.
//!
//! Nur LESEN: `SimConnect_SetInputEvent` wird nie benutzt (steht auch nicht
//! in der bindgen-Positivliste). Abonniert wird ausschliesslich, was in
//! [`POSITIVLISTE`] steht.
//!
//! Plattformunabhängig, damit Deutung und Ablauf auch auf Mac/Linux
//! getestet werden; das Verdrahten mit SimConnect steht im Windows-Adapter.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::time::{Duration, Instant};

pub const KOPF: usize = 12;
pub const LISTE_KOPF: usize = 28;
pub const DESKRIPTOR: usize = 76;
pub const TYP_DOUBLE: u32 = 0;
pub const TYP_STRING: u32 = 1;

/// Die Input-Events, die der Client liest. Nur diese werden abonniert und
/// in den Snapshot übernommen; Deutung je Profil in
/// `adapter::telemetry` (Stichwort `B:`).
pub const POSITIVLISTE: &[&str] = &[
    "AIRLINER_LIGHTS_EXT_STROBE",
    "AIRLINER_SIGNS_SEAT_BELTS",
    "AIRLINER_MIP_LG_ABRK_KNOB",
    "AIRLINER_MIP_LG_ABRK_RTO",
    "AIRLINER_LDG_AUTO_BRK",
    "AIRLINER_APU_MASTER_SWITCH",
    "AIRLINER_OVH_LTS_BEACON",
];

/// Wartezeit nach einem Flugzeugwechsel, bevor aufgezählt wird — direkt
/// nach `AircraftLoaded` hat das Flugzeug seine Events evtl. noch nicht
/// angemeldet.
pub const ANLAUF: Duration = Duration::from_secs(3);
/// Kommt die Liste nicht vollständig, gilt der Versuch als gescheitert.
pub const LISTE_FRIST: Duration = Duration::from_secs(10);
/// Abstand zwischen zwei Versuchen, wenn kein Event der Positivliste kam.
pub const WIEDERHOLUNG: Duration = Duration::from_secs(30);
/// Höchstens so viele Versuche je Flugzeug. Muster ohne diese Events (alles
/// ausser iniBuilds/Synaptic) sollen nicht endlos Listen anfordern.
pub const MAX_VERSUCHE: u32 = 4;

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

/// Ein Teil der Liste (komplette Rohnachricht ab `dwSize`):
/// (RequestID, EntryNumber, OutOf, Deskriptoren).
pub fn enumerate_deuten(roh: &[u8]) -> Option<(u32, u32, u32, Vec<Deskriptor>)> {
    let req = u32_bei(roh, KOPF)?;
    let anzahl = u32_bei(roh, 16)? as usize;
    let nr = u32_bei(roh, 20)?;
    let von = u32_bei(roh, 24)?;
    let mut v = Vec::with_capacity(anzahl.min(4096));
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
struct ListenSammler {
    teile: BTreeSet<u32>,
    von: Option<u32>,
    hashes: HashSet<u64>,
    treffer: Vec<Deskriptor>,
}

impl ListenSammler {
    fn aufnehmen(&mut self, nr: u32, von: u32, d: Vec<Deskriptor>) {
        self.von = Some(von);
        if !self.teile.insert(nr) {
            return; // Teil doppelt geliefert
        }
        for x in d {
            if x.typ != TYP_DOUBLE || x.name.is_empty() || !self.hashes.insert(x.hash) {
                continue;
            }
            if POSITIVLISTE.contains(&x.name.as_str()) {
                self.treffer.push(x);
            }
        }
    }
    fn fertig(&self) -> bool {
        matches!(self.von, Some(v) if self.teile.len() as u32 >= v)
    }
}

/// Was der Adapter nach einer vollständigen Liste tun soll.
#[derive(Debug, Clone, PartialEq)]
pub struct Abos {
    /// Zu abonnierende Events: (Hash, Name, RequestID fuer das einmalige
    /// `GetInputEvent`, das den Startwert holt).
    pub neu: Vec<(u64, String, u32)>,
}

/// Zustand der Input-Events einer Verbindung. Bei Flugzeugwechsel und
/// Neuverbinden verworfen.
#[derive(Debug, Default)]
pub struct EingabeState {
    /// Laufende Aufzählung: RequestID + Sammler + Startzeit.
    laufend: Option<(u32, ListenSammler, Instant)>,
    /// Zähler fuer RequestIDs — eine verspätete Liste oder Antwort eines
    /// früheren Durchgangs wird daran erkannt und verworfen.
    generation: u32,
    /// Ab wann die nächste Aufzählung fällig ist (`None` = nichts zu tun).
    faellig_ab: Option<Instant>,
    versuche: u32,
    /// Der Simulator kennt keine Input-Events (MSFS 2020) — nicht mehr fragen.
    nicht_verfuegbar: bool,
    /// Abonnierte Events: Hash → Name.
    abonniert: HashMap<u64, String>,
    /// Offene `GetInputEvent`-Anfragen: RequestID → Name.
    get_anfragen: HashMap<u32, String>,
    /// Letzte Werte je Name (nur Positivliste).
    werte: BTreeMap<String, f64>,
}

impl EingabeState {
    /// Flugzeug geladen/gewechselt: alles verwerfen, nach [`ANLAUF`] neu
    /// aufzählen. Liefert die Hashes, deren Abo abgemeldet werden soll.
    pub fn flugzeug_gewechselt(&mut self, jetzt: Instant) -> Vec<u64> {
        let alt: Vec<u64> = self.abonniert.keys().copied().collect();
        self.abonniert.clear();
        self.get_anfragen.clear();
        self.werte.clear();
        self.laufend = None;
        self.versuche = 0;
        if !self.nicht_verfuegbar {
            self.faellig_ab = Some(jetzt + ANLAUF);
        }
        alt
    }

    /// Muss jetzt aufgezählt werden? Dann die RequestID dafür (der Zustand
    /// merkt sich den Versuch).
    pub fn aufzaehlung_starten(&mut self, jetzt: Instant) -> Option<u32> {
        if self.nicht_verfuegbar || self.laufend.is_some() {
            return None;
        }
        let f = self.faellig_ab?;
        if jetzt < f {
            return None;
        }
        self.faellig_ab = None;
        self.versuche += 1;
        self.generation = self.generation.wrapping_add(1) % 100_000;
        let req = self.generation;
        self.laufend = Some((req, ListenSammler::default(), jetzt));
        Some(req)
    }

    /// Der Aufruf `EnumerateInputEvents` selbst ist gescheitert oder der
    /// Simulator hat ihn mit einer Ausnahme abgelehnt: kein MSFS 2024.
    pub fn nicht_verfuegbar(&mut self) {
        self.nicht_verfuegbar = true;
        self.laufend = None;
        self.faellig_ab = None;
    }

    pub fn ist_verfuegbar(&self) -> bool {
        !self.nicht_verfuegbar
    }

    /// Frist fuer eine laufende Aufzählung prüfen.
    pub fn frist_pruefen(&mut self, jetzt: Instant) {
        if let Some((_, _, start)) = &self.laufend {
            if jetzt.duration_since(*start) > LISTE_FRIST {
                self.laufend = None;
                self.nochmal(jetzt);
            }
        }
    }

    fn nochmal(&mut self, jetzt: Instant) {
        if self.versuche < MAX_VERSUCHE && !self.nicht_verfuegbar {
            self.faellig_ab = Some(jetzt + WIEDERHOLUNG);
        }
    }

    /// Ein Teil der Liste kam an. Ist sie vollständig, kommen die zu
    /// abonnierenden Events zurück.
    pub fn liste_aufnehmen(&mut self, roh: &[u8], jetzt: Instant) -> Option<Abos> {
        let (req, nr, von, d) = enumerate_deuten(roh)?;
        let (lauf_req, sammler, _) = self.laufend.as_mut()?;
        if *lauf_req != req {
            return None; // verspätete Liste eines früheren Durchgangs
        }
        sammler.aufnehmen(nr, von, d);
        if !sammler.fertig() {
            return None;
        }
        let (_, sammler, _) = self.laufend.take()?;
        let mut neu = Vec::new();
        for d in sammler.treffer {
            let idx = POSITIVLISTE.iter().position(|n| *n == d.name).unwrap_or(0) as u32;
            let get_req = self.generation * 16 + idx;
            self.abonniert.insert(d.hash, d.name.clone());
            self.get_anfragen.insert(get_req, d.name.clone());
            neu.push((d.hash, d.name, get_req));
        }
        if neu.is_empty() {
            // Flugzeug ohne diese Events — oder es hat sie noch nicht
            // angemeldet. Ein paar Mal nachfragen, dann Ruhe.
            self.nochmal(jetzt);
        }
        Some(Abos { neu })
    }

    /// Abo-Meldung: Wert übernehmen, wenn das Event zum aktuellen Flugzeug
    /// gehört.
    pub fn abo_aufnehmen(&mut self, roh: &[u8]) {
        if let Some((hash, Some(w))) = abo_deuten(roh) {
            if let Some(name) = self.abonniert.get(&hash) {
                if w.is_finite() {
                    self.werte.insert(name.clone(), w);
                }
            }
        }
    }

    /// Antwort auf das einmalige `GetInputEvent` (Startwert).
    pub fn get_aufnehmen(&mut self, roh: &[u8]) {
        if let Some((req, w)) = get_deuten(roh) {
            if let Some(name) = self.get_anfragen.remove(&req) {
                if let Some(w) = w.filter(|w| w.is_finite()) {
                    // Ein Abo-Wert, der schon da ist, ist mindestens so
                    // frisch — nicht überschreiben.
                    self.werte.entry(name).or_insert(w);
                }
            }
        }
    }

    /// Aktuelle Werte (Name ohne `B:`-Präfix → Wert).
    pub fn werte(&self) -> &BTreeMap<String, f64> {
        &self.werte
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kopf(id: u32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&0u32.to_le_bytes());
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

    /// Nachricht `SIMCONNECT_RECV_ENUMERATE_INPUT_EVENTS` wie vom Simulator.
    pub(crate) fn liste(req: u32, nr: u32, von: u32, d: &[(&str, u64, u32)]) -> Vec<u8> {
        let mut v = kopf(40);
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

    /// Nachricht `SIMCONNECT_RECV_SUBSCRIBE_INPUT_EVENT`.
    pub(crate) fn abo(hash: u64, w: f64) -> Vec<u8> {
        let mut a = kopf(42);
        a.extend_from_slice(&hash.to_le_bytes());
        a.extend_from_slice(&TYP_DOUBLE.to_le_bytes());
        a.extend_from_slice(&w.to_le_bytes());
        a
    }

    fn get(req: u32, w: f64) -> Vec<u8> {
        let mut g = kopf(41);
        g.extend_from_slice(&req.to_le_bytes());
        g.extend_from_slice(&TYP_DOUBLE.to_le_bytes());
        g.extend_from_slice(&w.to_le_bytes());
        g
    }

    #[test]
    fn byte_layout_der_drei_nachrichten() {
        let t = liste(
            7,
            0,
            1,
            &[
                ("AIRLINER_LIGHTS_EXT_STROBE", 11, TYP_DOUBLE),
                ("AIRLINER_TEXT", 12, TYP_STRING),
            ],
        );
        // Deskriptor 1 beginnt bei 28, Hash bei 28+64, Typ bei 28+72.
        assert_eq!(&t[28..54], b"AIRLINER_LIGHTS_EXT_STROBE");
        let (req, nr, von, d) = enumerate_deuten(&t).unwrap();
        assert_eq!((req, nr, von), (7, 0, 1));
        assert_eq!(d[0].hash, 11);
        assert_eq!(d[1].typ, TYP_STRING);

        let a = abo(0xABCD_EF01_2345_6789, 2.0);
        assert_eq!(a.len(), 32, "Wert ab Byte 24");
        assert_eq!(abo_deuten(&a), Some((0xABCD_EF01_2345_6789, Some(2.0))));
        assert!(abo_deuten(&a[..16]).is_none());

        let g = get(99, 5.0);
        assert_eq!(g.len(), 28, "Wert ab Byte 20");
        assert_eq!(get_deuten(&g), Some((99, Some(5.0))));
        let mut gs = kopf(41);
        gs.extend_from_slice(&5u32.to_le_bytes());
        gs.extend_from_slice(&TYP_STRING.to_le_bytes());
        gs.extend_from_slice(b"AUTO\0");
        assert_eq!(get_deuten(&gs), Some((5, None)));
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
    fn nur_positivliste_wird_abonniert_liste_in_teilen() {
        let t0 = Instant::now();
        let mut s = EingabeState::default();
        assert!(s.flugzeug_gewechselt(t0).is_empty());
        assert_eq!(s.aufzaehlung_starten(t0), None, "erst nach dem Anlauf");
        let req = s.aufzaehlung_starten(t0 + ANLAUF).unwrap();
        let teil1 = liste(
            req,
            0,
            2,
            &[
                ("AIRLINER_LIGHTS_EXT_STROBE", 11, TYP_DOUBLE),
                ("AIRLINER_IRGENDWAS", 12, TYP_DOUBLE),
                ("AIRLINER_SIGNS_SEAT_BELTS", 13, TYP_STRING), // Text → nein
            ],
        );
        assert_eq!(s.liste_aufnehmen(&teil1, t0), None, "Teil 2 fehlt");
        let teil2 = liste(req, 1, 2, &[("AIRLINER_MIP_LG_ABRK_KNOB", 14, TYP_DOUBLE)]);
        let abos = s.liste_aufnehmen(&teil2, t0).unwrap();
        let namen: Vec<&str> = abos.neu.iter().map(|(_, n, _)| n.as_str()).collect();
        assert_eq!(
            namen,
            vec!["AIRLINER_LIGHTS_EXT_STROBE", "AIRLINER_MIP_LG_ABRK_KNOB"]
        );

        // Gemessener A380-Wert: Strobe OFF = 2.
        s.abo_aufnehmen(&abo(11, 2.0));
        s.abo_aufnehmen(&abo(12, 1.0)); // nicht abonniert → ignoriert
        let (_, _, get_req) = abos.neu[1].clone();
        s.get_aufnehmen(&get(get_req, 5.0)); // Startwert Knopf HI
        assert_eq!(s.werte().get("AIRLINER_LIGHTS_EXT_STROBE"), Some(&2.0));
        assert_eq!(s.werte().get("AIRLINER_MIP_LG_ABRK_KNOB"), Some(&5.0));
        assert_eq!(s.werte().len(), 2);

        // Flugzeugwechsel: Werte weg, alte Hashes zum Abmelden.
        let mut weg = s.flugzeug_gewechselt(t0);
        weg.sort();
        assert_eq!(weg, vec![11, 14]);
        assert!(s.werte().is_empty());
        s.abo_aufnehmen(&abo(11, 0.0)); // verspätete Meldung des alten
        assert!(s.werte().is_empty());
    }

    #[test]
    fn verspaetete_liste_eines_frueheren_durchgangs_zaehlt_nicht() {
        let t0 = Instant::now();
        let mut s = EingabeState::default();
        s.flugzeug_gewechselt(t0);
        let alt = s.aufzaehlung_starten(t0 + ANLAUF).unwrap();
        s.flugzeug_gewechselt(t0 + ANLAUF);
        let neu = s.aufzaehlung_starten(t0 + ANLAUF * 2).unwrap();
        assert_ne!(alt, neu);
        let l = liste(alt, 0, 1, &[("AIRLINER_APU_MASTER_SWITCH", 5, 0)]);
        assert_eq!(s.liste_aufnehmen(&l, t0), None);
    }

    #[test]
    fn ohne_treffer_begrenzt_wiederholen_ohne_input_events_ruhe() {
        let t0 = Instant::now();
        let mut s = EingabeState::default();
        s.flugzeug_gewechselt(t0);
        let mut t = t0 + ANLAUF;
        let mut versuche = 0;
        while let Some(req) = s.aufzaehlung_starten(t) {
            versuche += 1;
            let l = liste(req, 0, 1, &[("FENIX_ETWAS", 1, 0)]);
            assert_eq!(s.liste_aufnehmen(&l, t).unwrap().neu, vec![]);
            t += WIEDERHOLUNG;
        }
        assert_eq!(versuche, MAX_VERSUCHE);

        // MSFS 2020: Frist laeuft ab, dann meldet der Adapter "nicht
        // verfuegbar" — danach wird nie mehr gefragt, Werte bleiben leer.
        let mut s = EingabeState::default();
        s.flugzeug_gewechselt(t0);
        assert!(s.aufzaehlung_starten(t0 + ANLAUF).is_some());
        s.frist_pruefen(t0 + ANLAUF + LISTE_FRIST + Duration::from_secs(1));
        s.nicht_verfuegbar();
        s.flugzeug_gewechselt(t0 + WIEDERHOLUNG * 3);
        assert_eq!(s.aufzaehlung_starten(t0 + WIEDERHOLUNG * 9), None);
        assert!(s.werte().is_empty());
    }
}
