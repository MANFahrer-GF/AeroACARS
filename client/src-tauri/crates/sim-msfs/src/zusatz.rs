//! Zusatzwerte fuer den Telemetrie-Monitor (v1.8).
//!
//! Eigene Datendefinition (#4), die nur existiert, solange der Monitor
//! offen ist. Die Feldliste kommt aus der App (Kanal-ID, SimVar, Einheit).
//!
//! Warum die Vorsicht: Lehnt SimConnect eine SimVar ab (MSFS 2020 kennt
//! eine 2024-Variable nicht, ein Tippfehler), kommt die Ablehnung
//! asynchron als Ausnahme — und der Datenblock ist danach ein Feld kuerzer,
//! alle folgenden Werte verrutschen (siehe Crate-Doku `lib.rs`). Deshalb:
//!   * Die Ausnahme wird ueber die Paketkennung dem Feld zugeordnet, das
//!     Feld wandert in die Ablehnungsliste, die Definition wird ohne es neu
//!     angelegt.
//!   * Bis dahin werden eintreffende Bloecke verworfen (`gueltig = false`),
//!     statt verrutschte Werte anzuzeigen.
//!   * Nach jeder Registrierung eine kurze Anlaufzeit, in der Bloecke
//!     ebenfalls verworfen werden — eine Ausnahme kann den ersten Daten
//!     knapp hinterherlaufen.
//!
//! Plattformunabhaengig, damit die Logik auch auf dem Mac/Linux getestet
//! wird; das Verdrahten mit SimConnect steht im Windows-Adapter.

use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Anlaufzeit nach einer (Neu-)Registrierung.
pub const ANLAUF: Duration = Duration::from_millis(600);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZusatzFeld {
    pub kanal: String,
    pub simvar: String,
    pub einheit: String,
}

#[derive(Debug, Default)]
pub struct ZusatzState {
    /// Von der App gewuenschte Felder (leer = Monitor zu).
    gewuenscht: Vec<ZusatzFeld>,
    /// SimVar-Namen, die SimConnect abgelehnt hat. Bleiben fuer die
    /// Dauer der Verbindung gesperrt.
    abgelehnt: HashSet<String>,
    /// Indizes in `gewuenscht`, in der Reihenfolge der Definition.
    registriert: Vec<usize>,
    /// (Paketkennung, Index in `gewuenscht`) je AddToDataDefinition.
    paketkennungen: Vec<(u32, usize)>,
    werte: Vec<Option<f64>>,
    /// Definition muss neu angelegt (oder geloescht) werden.
    pub dirty: bool,
    gueltig_ab: Option<Instant>,
}

impl ZusatzState {
    /// Neue Feldliste setzen. Gleiche Liste = keine Neuregistrierung.
    pub fn setzen(&mut self, felder: Vec<ZusatzFeld>) {
        if felder == self.gewuenscht {
            return;
        }
        self.gewuenscht = felder;
        self.werte = vec![None; self.gewuenscht.len()];
        self.registriert.clear();
        self.paketkennungen.clear();
        self.gueltig_ab = None;
        self.dirty = true;
    }

    pub fn aktiv(&self) -> bool {
        !self.gewuenscht.is_empty()
    }

    /// Nach einem Neuaufbau der Verbindung: Definitionen sind weg, die
    /// Ablehnungen gelten fuer die neue Verbindung nicht mehr zwingend
    /// (anderer Simulator, anderes Flugzeug).
    pub fn neue_verbindung(&mut self) {
        self.abgelehnt.clear();
        self.registriert.clear();
        self.paketkennungen.clear();
        self.werte = vec![None; self.gewuenscht.len()];
        self.gueltig_ab = None;
        self.dirty = self.aktiv();
    }

    /// Felder, die jetzt registriert werden sollen: (Index, Feld).
    pub fn zu_registrieren(&self) -> Vec<(usize, ZusatzFeld)> {
        self.gewuenscht
            .iter()
            .enumerate()
            .filter(|(_, f)| !self.abgelehnt.contains(&f.simvar))
            .map(|(i, f)| (i, f.clone()))
            .collect()
    }

    /// Ergebnis einer Registrierung eintragen.
    pub fn registriert(
        &mut self,
        reihenfolge: Vec<usize>,
        kennungen: Vec<(u32, usize)>,
        jetzt: Instant,
    ) {
        self.registriert = reihenfolge;
        self.paketkennungen = kennungen;
        self.werte = vec![None; self.gewuenscht.len()];
        self.gueltig_ab = Some(jetzt + ANLAUF);
        self.dirty = false;
    }

    /// Eine SimConnect-Ausnahme pruefen. Gehoert sie zu einem Zusatzfeld,
    /// kommt dessen SimVar-Name zurueck und die Definition ist neu
    /// anzulegen.
    pub fn ausnahme(&mut self, paketkennung: u32) -> Option<String> {
        let idx = self
            .paketkennungen
            .iter()
            .find(|(k, _)| *k == paketkennung)
            .map(|(_, i)| *i)?;
        let simvar = self.gewuenscht.get(idx)?.simvar.clone();
        self.abgelehnt.insert(simvar.clone());
        self.gueltig_ab = None;
        self.werte = vec![None; self.gewuenscht.len()];
        self.dirty = true;
        Some(simvar)
    }

    /// Datenblock der Definition #4 einlesen (je Feld ein FLOAT64).
    pub fn einlesen(&mut self, bytes: &[u8], jetzt: Instant) {
        if !self.gueltig_ab.is_some_and(|t| jetzt >= t) {
            return;
        }
        if bytes.len() < self.registriert.len() * 8 {
            // Kuerzer als erwartet: ein Feld fehlt, die Ausnahme ist
            // unterwegs. Lieber nichts als verrutschte Werte.
            return;
        }
        for (pos, idx) in self.registriert.iter().enumerate() {
            let off = pos * 8;
            let mut b = [0u8; 8];
            b.copy_from_slice(&bytes[off..off + 8]);
            let v = f64::from_le_bytes(b);
            if let Some(slot) = self.werte.get_mut(*idx) {
                *slot = v.is_finite().then_some(v);
            }
        }
    }

    /// Aktuelle Werte: (Kanal-ID, Rohwert in der angeforderten Einheit).
    pub fn werte(&self) -> Vec<(String, f64)> {
        self.gewuenscht
            .iter()
            .zip(&self.werte)
            .filter_map(|(f, v)| v.map(|v| (f.kanal.clone(), v)))
            .collect()
    }

    pub fn abgelehnt(&self) -> Vec<String> {
        let mut v: Vec<_> = self.abgelehnt.iter().cloned().collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feld(k: &str, v: &str) -> ZusatzFeld {
        ZusatzFeld {
            kanal: k.into(),
            simvar: v.into(),
            einheit: "number".into(),
        }
    }

    fn block(werte: &[f64]) -> Vec<u8> {
        werte.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    fn eingerichtet() -> (ZusatzState, Instant) {
        let mut z = ZusatzState::default();
        z.setzen(vec![feld("a", "A"), feld("b", "B"), feld("c", "C")]);
        let t = Instant::now();
        let reg: Vec<usize> = z.zu_registrieren().iter().map(|(i, _)| *i).collect();
        z.registriert(reg, vec![(10, 0), (11, 1), (12, 2)], t);
        (z, t + ANLAUF)
    }

    #[test]
    fn werte_landen_beim_richtigen_kanal() {
        let (mut z, t) = eingerichtet();
        z.einlesen(&block(&[1.0, 2.0, 3.0]), t);
        assert_eq!(
            z.werte(),
            vec![("a".into(), 1.0), ("b".into(), 2.0), ("c".into(), 3.0)]
        );
    }

    #[test]
    fn in_der_anlaufzeit_wird_nichts_uebernommen() {
        let (mut z, t) = eingerichtet();
        z.einlesen(&block(&[1.0, 2.0, 3.0]), t - Duration::from_millis(100));
        assert!(z.werte().is_empty());
    }

    #[test]
    fn abgelehntes_feld_fliegt_raus_und_nichts_verrutscht() {
        let (mut z, t) = eingerichtet();
        z.einlesen(&block(&[1.0, 2.0, 3.0]), t);
        assert_eq!(z.ausnahme(11).as_deref(), Some("B"));
        assert!(z.dirty);
        assert!(z.werte().is_empty(), "alte Werte nach Ausnahme verworfen");
        // Ein verkuerzter Block vor der Neuregistrierung wird verworfen.
        z.einlesen(&block(&[1.0, 3.0]), t + Duration::from_secs(1));
        assert!(z.werte().is_empty());
        // Neu registriert ohne B.
        let reg: Vec<usize> = z.zu_registrieren().iter().map(|(i, _)| *i).collect();
        assert_eq!(reg, vec![0, 2]);
        let t2 = t + Duration::from_secs(2);
        z.registriert(reg, vec![(20, 0), (21, 2)], t2);
        z.einlesen(&block(&[1.0, 3.0]), t2 + ANLAUF);
        assert_eq!(z.werte(), vec![("a".into(), 1.0), ("c".into(), 3.0)]);
        assert_eq!(z.abgelehnt(), vec!["B".to_string()]);
    }

    #[test]
    fn fremde_ausnahme_wird_ignoriert() {
        let (mut z, _) = eingerichtet();
        assert_eq!(z.ausnahme(999), None);
        assert!(!z.dirty);
    }

    #[test]
    fn zu_kurzer_block_wird_verworfen() {
        let (mut z, t) = eingerichtet();
        z.einlesen(&block(&[1.0, 2.0]), t);
        assert!(z.werte().is_empty());
    }

    #[test]
    fn gleiche_liste_loest_keine_neuregistrierung_aus() {
        let (mut z, _) = eingerichtet();
        z.setzen(vec![feld("a", "A"), feld("b", "B"), feld("c", "C")]);
        assert!(!z.dirty);
        z.setzen(Vec::new());
        assert!(z.dirty);
        assert!(!z.aktiv());
    }

    #[test]
    fn neue_verbindung_gibt_ablehnungen_frei() {
        let (mut z, _) = eingerichtet();
        z.ausnahme(11);
        z.neue_verbindung();
        assert!(z.abgelehnt().is_empty());
        assert!(z.dirty);
        assert_eq!(z.zu_registrieren().len(), 3);
    }

    #[test]
    fn nicht_endliche_werte_werden_none() {
        let (mut z, t) = eingerichtet();
        z.einlesen(&block(&[f64::NAN, 2.0, f64::INFINITY]), t);
        assert_eq!(z.werte(), vec![("b".into(), 2.0)]);
    }
}
