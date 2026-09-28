//! „Flugzeug vermessen" (28.09.2026) — Messkanal für L:- und A:-Variablen.
//!
//! Der Pilot schaltet im Cockpit, der Client schaut zu und notiert, welche
//! Werte mitgehen. Dafür liest der Adapter während der Messung viele
//! Variablen auf einmal: die L:-Namen aus dem Aircraft-Scan des Flugzeugs
//! (SimConnect kann L:-Variablen nicht aufzählen) und die Standard-SimVars
//! der Cockpitschalter. Die B:-Input-Events laufen über
//! [`crate::eingabe_events`] im Modus „alle".
//!
//! Aufbau wie beim Telemetrie-Zusatzkanal ([`crate::zusatz`]), aber in
//! Blöcken zu [`BLOCK`] Variablen — jeder Block eine eigene Datendefinition
//! und Anfrage (IDs ab [`ID_BASIS`]). So erprobt im Werkzeug
//! `tools/schalterpruefung` (dort ~3400 LVars). Lehnt der Simulator einen
//! Namen ab, verrutschen die Werte seines Blocks — die Ablehnung wird über
//! die Paketkennung zugeordnet und der Block ohne den Namen neu angelegt.
//!
//! Plattformunabhängig, damit die Logik auch auf Mac/Linux getestet wird;
//! das Verdrahten mit SimConnect steht im Windows-Adapter.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

/// Variablen je Datendefinition.
pub const BLOCK: usize = 200;
/// Datendefinitions- und Anfrage-IDs der Blöcke: `ID_BASIS + Block`.
pub const ID_BASIS: u32 = 5000;
/// Höchstzahl Blöcke (= 8000 Variablen) — schützt den ID-Bereich.
pub const MAX_BLOECKE: usize = 40;
/// Nach einer (Neu-)Registrierung verworfene Anlaufzeit.
pub const ANLAUF: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessFeld {
    /// Anzeigename im Bericht, z. B. `L:INI_SPOILERS_ARMED`.
    pub name: String,
    /// SimVar für SimConnect, z. B. `L:INI_SPOILERS_ARMED` oder `LIGHT BEACON`.
    pub simvar: String,
    pub einheit: String,
}

/// Standard-SimVars der Cockpitschalter, die immer mitgemessen werden.
pub fn standard_felder() -> Vec<MessFeld> {
    [
        ("LIGHT BEACON", "Bool"),
        ("LIGHT STROBE", "Bool"),
        ("LIGHT NAV", "Bool"),
        ("LIGHT LANDING", "Bool"),
        ("LIGHT TAXI", "Bool"),
        ("LIGHT LOGO", "Bool"),
        ("LIGHT WING", "Bool"),
        ("TRANSPONDER STATE:1", "Number"),
        ("CABIN SEATBELTS ALERT SWITCH", "Bool"),
        ("FLAPS HANDLE INDEX", "Number"),
        ("FLAPS HANDLE PERCENT", "Percent over 100"),
        ("SPOILERS ARMED", "Bool"),
        ("SPOILERS HANDLE POSITION", "Percent over 100"),
        ("AUTO BRAKE SWITCH CB", "Number"),
        ("APU SWITCH", "Bool"),
        ("APU PCT RPM", "Percent over 100"),
        ("BRAKE PARKING POSITION", "Bool"),
        ("BRAKE PARKING INDICATOR", "Bool"),
        // Autopilot (Teil „luft", 28.09.2026): Standardwerte, die viele
        // Add-ons NICHT bedienen — genau das soll die Messung zeigen.
        ("AUTOPILOT MASTER", "Bool"),
        ("AUTOPILOT DISENGAGED", "Bool"),
        ("AUTOPILOT FLIGHT DIRECTOR ACTIVE", "Bool"),
        ("AUTOPILOT HEADING LOCK", "Bool"),
        ("AUTOPILOT NAV1 LOCK", "Bool"),
        ("AUTOPILOT ALTITUDE LOCK", "Bool"),
        ("AUTOPILOT VERTICAL HOLD", "Bool"),
        ("AUTOPILOT FLIGHT LEVEL CHANGE", "Bool"),
        ("AUTOPILOT APPROACH HOLD", "Bool"),
        ("AUTOPILOT APPROACH ARM", "Bool"),
        ("AUTOPILOT GLIDESLOPE HOLD", "Bool"),
        ("AUTOPILOT BACKCOURSE HOLD", "Bool"),
        ("AUTOPILOT MANAGED THROTTLE ACTIVE", "Bool"),
        ("AUTOTHROTTLE ACTIVE", "Bool"),
    ]
    .into_iter()
    .map(|(v, e)| MessFeld {
        name: format!("A:{v}"),
        simvar: v.to_string(),
        einheit: e.to_string(),
    })
    .collect()
}

/// L:-Felder aus einer Namensliste (ohne `L:`-Präfix oder mit). Unsaubere
/// Namen (Steuerzeichen, Komma, Semikolon) fallen weg, Doppelte auch.
pub fn l_felder(namen: &[String]) -> Vec<MessFeld> {
    let mut gesehen = HashSet::new();
    namen
        .iter()
        .map(|n| n.trim().trim_start_matches("L:").trim())
        .filter(|n| {
            !n.is_empty()
                && n.len() <= 120
                && !n.chars().any(|c| c.is_control() || c == ',' || c == ';')
        })
        .filter(|n| gesehen.insert(n.to_string()))
        .take(BLOCK * MAX_BLOECKE - 100)
        .map(|n| MessFeld {
            name: format!("L:{n}"),
            simvar: format!("L:{n}"),
            einheit: "Number".into(),
        })
        .collect()
}

#[derive(Debug, Default)]
pub struct MessState {
    gewuenscht: Vec<MessFeld>,
    abgelehnt: HashSet<String>,
    /// Aktive Blöcke: Indizes in `gewuenscht`, in Definitionsreihenfolge.
    bloecke: Vec<Vec<usize>>,
    /// Paketkennung → (Block, Position).
    kennungen: HashMap<u32, (usize, usize)>,
    werte: HashMap<usize, f64>,
    gueltig_ab: Option<Instant>,
    /// Wie viele Blöcke zuletzt beim Simulator angelegt waren — die müssen
    /// beim Umbau erst gestoppt und geleert werden.
    angelegt: usize,
    /// Definition muss (neu) angelegt oder abgebaut werden.
    pub dirty: bool,
}

impl MessState {
    /// Neue Messliste (leer = Messung vorbei).
    pub fn setzen(&mut self, felder: Vec<MessFeld>) {
        self.gewuenscht = felder;
        self.werte.clear();
        self.abgelehnt.clear();
        self.gueltig_ab = None;
        self.dirty = true;
    }

    pub fn aktiv(&self) -> bool {
        !self.gewuenscht.is_empty()
    }

    /// Nach einer neuen SimConnect-Verbindung: alles neu anlegen.
    pub fn neue_verbindung(&mut self) {
        self.bloecke.clear();
        self.kennungen.clear();
        self.werte.clear();
        self.angelegt = 0;
        self.gueltig_ab = None;
        self.dirty = self.aktiv();
    }

    /// Was jetzt anzulegen ist: Blöcke aus (Index, Feld) und die Zahl der
    /// bisher angelegten Blöcke, die vorher abzubauen sind.
    pub fn zu_registrieren(&self) -> (Vec<Vec<(usize, MessFeld)>>, usize) {
        let aktiv: Vec<(usize, MessFeld)> = self
            .gewuenscht
            .iter()
            .enumerate()
            .filter(|(_, f)| !self.abgelehnt.contains(&f.simvar))
            .map(|(i, f)| (i, f.clone()))
            .collect();
        let bloecke = aktiv
            .chunks(BLOCK)
            .take(MAX_BLOECKE)
            .map(|c| c.to_vec())
            .collect();
        (bloecke, self.angelegt)
    }

    /// Ergebnis einer Registrierung: Blockbelegung und Paketkennungen.
    pub fn registriert(
        &mut self,
        bloecke: Vec<Vec<usize>>,
        kennungen: Vec<(u32, usize, usize)>,
        jetzt: Instant,
    ) {
        self.angelegt = bloecke.len();
        self.bloecke = bloecke;
        self.kennungen = kennungen.into_iter().map(|(k, b, p)| (k, (b, p))).collect();
        self.werte.clear();
        self.gueltig_ab = Some(jetzt + ANLAUF);
        self.dirty = false;
    }

    /// Eine Ausnahme des Simulators: gehört sie zu einem Messfeld, wird es
    /// aussortiert und die Liste neu angelegt. Liefert den Namen.
    pub fn ausnahme(&mut self, paketkennung: u32) -> Option<String> {
        let (b, p) = *self.kennungen.get(&paketkennung)?;
        let idx = *self.bloecke.get(b)?.get(p)?;
        let simvar = self.gewuenscht.get(idx)?.simvar.clone();
        self.abgelehnt.insert(simvar.clone());
        self.werte.clear();
        self.gueltig_ab = None;
        self.dirty = true;
        Some(simvar)
    }

    /// Gehört diese Anfrage-ID zum Messkanal?
    pub fn ist_mess_anfrage(request_id: u32) -> bool {
        (ID_BASIS..ID_BASIS + MAX_BLOECKE as u32).contains(&request_id)
    }

    /// Datenblock eines Messblocks einlesen.
    pub fn einlesen(&mut self, request_id: u32, bytes: &[u8], jetzt: Instant) {
        if self.gueltig_ab.is_none_or(|t| jetzt < t) {
            return;
        }
        let Some(b) = request_id.checked_sub(ID_BASIS) else {
            return;
        };
        let Some(block) = self.bloecke.get(b as usize) else {
            return;
        };
        if bytes.len() < block.len() * 8 {
            return; // ein Feld fehlt, die Ausnahme ist unterwegs
        }
        for (pos, idx) in block.iter().enumerate() {
            let mut w = [0u8; 8];
            w.copy_from_slice(&bytes[pos * 8..pos * 8 + 8]);
            let v = f64::from_le_bytes(w);
            if v.is_finite() {
                self.werte.insert(*idx, v);
            } else {
                self.werte.remove(idx);
            }
        }
    }

    /// Aktuelle Werte: (Name, Wert).
    pub fn werte(&self) -> Vec<(String, f64)> {
        self.werte
            .iter()
            .filter_map(|(i, v)| self.gewuenscht.get(*i).map(|f| (f.name.clone(), *v)))
            .collect()
    }

    /// Zahl der Felder, die gerade gemessen werden (ohne abgelehnte).
    pub fn anzahl(&self) -> usize {
        self.gewuenscht.len() - self.abgelehnt.len().min(self.gewuenscht.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn werte_block(v: &[f64]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    #[test]
    fn namen_werden_gesaeubert_und_entdoppelt() {
        let f = l_felder(&[
            "INI_SPOILERS_ARMED".into(),
            "L:INI_SPOILERS_ARMED".into(),
            "bad;name".into(),
            " ".into(),
            "A32NX_OVHD_EXTLT_STROBE".into(),
        ]);
        let namen: Vec<&str> = f.iter().map(|f| f.simvar.as_str()).collect();
        assert_eq!(namen, ["L:INI_SPOILERS_ARMED", "L:A32NX_OVHD_EXTLT_STROBE"]);
    }

    #[test]
    fn bloecke_zu_200_und_werte_je_block() {
        let mut s = MessState::default();
        let namen: Vec<String> = (0..450).map(|i| format!("X_{i}")).collect();
        s.setzen(l_felder(&namen));
        let (bloecke, alt) = s.zu_registrieren();
        assert_eq!(alt, 0);
        assert_eq!(
            bloecke.iter().map(Vec::len).collect::<Vec<_>>(),
            [200, 200, 50]
        );
        let belegung: Vec<Vec<usize>> = bloecke
            .iter()
            .map(|b| b.iter().map(|(i, _)| *i).collect())
            .collect();
        let t0 = Instant::now();
        s.registriert(belegung, vec![(77, 1, 3)], t0);
        assert!(!s.dirty);
        // Während der Anlaufzeit nichts übernehmen.
        s.einlesen(ID_BASIS + 2, &werte_block(&[1.0; 50]), t0);
        assert!(s.werte().is_empty());
        let spaeter = t0 + ANLAUF + Duration::from_millis(1);
        let mut b2 = vec![0.0; 50];
        b2[7] = 2.0;
        s.einlesen(ID_BASIS + 2, &werte_block(&b2), spaeter);
        let w: HashMap<String, f64> = s.werte().into_iter().collect();
        assert_eq!(w.get("L:X_407"), Some(&2.0));
        assert_eq!(w.len(), 50);
        // Zu kurzer Block (Ausnahme unterwegs) → nichts.
        s.einlesen(ID_BASIS, &werte_block(&[1.0; 10]), spaeter);
        assert_eq!(s.werte().len(), 50);
    }

    #[test]
    fn abgelehnter_name_wird_aussortiert() {
        let mut s = MessState::default();
        s.setzen(l_felder(&["A".into(), "B".into(), "C".into()]));
        let (bloecke, _) = s.zu_registrieren();
        let belegung = vec![bloecke[0].iter().map(|(i, _)| *i).collect()];
        s.registriert(
            belegung,
            vec![(10, 0, 0), (11, 0, 1), (12, 0, 2)],
            Instant::now(),
        );
        assert_eq!(s.ausnahme(11).as_deref(), Some("L:B"));
        assert!(s.dirty);
        let (bloecke, alt) = s.zu_registrieren();
        assert_eq!(alt, 1, "der alte Block muss abgebaut werden");
        let namen: Vec<&str> = bloecke[0].iter().map(|(_, f)| f.simvar.as_str()).collect();
        assert_eq!(namen, ["L:A", "L:C"]);
        assert_eq!(s.ausnahme(999), None);
    }

    #[test]
    fn stoppen_baut_alles_ab() {
        let mut s = MessState::default();
        s.setzen(standard_felder());
        let (b, _) = s.zu_registrieren();
        s.registriert(
            vec![b[0].iter().map(|(i, _)| *i).collect()],
            vec![],
            Instant::now(),
        );
        s.setzen(Vec::new());
        assert!(s.dirty && !s.aktiv());
        let (b, alt) = s.zu_registrieren();
        assert!(b.is_empty());
        assert_eq!(alt, 1);
    }

    #[test]
    fn nur_eigene_anfrage_ids() {
        assert!(MessState::ist_mess_anfrage(ID_BASIS));
        assert!(MessState::ist_mess_anfrage(
            ID_BASIS + MAX_BLOECKE as u32 - 1
        ));
        assert!(!MessState::ist_mess_anfrage(4));
        assert!(!MessState::ist_mess_anfrage(ID_BASIS + MAX_BLOECKE as u32));
    }
}
