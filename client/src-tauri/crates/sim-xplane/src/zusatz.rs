//! Zusatzwerte fuer den Telemetrie-Monitor (v1.8).
//!
//! Eigene RREF-Abos ab Index [`ZUSATZ_INDEX_BASE`] — weit ueber dem Katalog
//! (0..~63) und den Profil-Proben (10 000+), damit ein Zusatzpaket nie als
//! Katalogwert oder Probe gedeutet wird. Die Abos bestehen nur, solange der
//! Monitor offen ist; die Feldliste kommt aus der App.

/// Erster RREF-Index der Zusatzwerte.
pub const ZUSATZ_INDEX_BASE: i32 = 20_000;
/// Rate der Zusatzabos. Schnell genug fuer Diagramme, halb so viel Last
/// wie der 50-Hz-Katalog.
pub const ZUSATZ_HZ: i32 = 20;

#[derive(Debug, Default)]
pub struct ZusatzAbos {
    /// (Kanal-ID, DataRef) — Position = Index − BASE.
    felder: Vec<(String, String)>,
    werte: Vec<Option<f32>>,
    /// Steigt bei jeder Aenderung der Liste; der Empfangsthread vergleicht
    /// sie mit dem Stand, den er abonniert hat.
    pub generation: u64,
}

impl ZusatzAbos {
    /// Neue Liste setzen. `true`, wenn sie sich geaendert hat.
    pub fn setzen(&mut self, felder: Vec<(String, String)>) -> bool {
        if felder == self.felder {
            return false;
        }
        self.felder = felder;
        self.werte = vec![None; self.felder.len()];
        self.generation += 1;
        true
    }

    /// DataRefs mit ihrem RREF-Index, wie sie abonniert werden.
    pub fn abos(&self) -> Vec<(i32, String)> {
        self.felder
            .iter()
            .enumerate()
            .map(|(i, (_, d))| (ZUSATZ_INDEX_BASE + i as i32, d.clone()))
            .collect()
    }

    /// Ein empfangenes Paket einsortieren. `false`, wenn der Index nicht zu
    /// den Zusatzwerten gehoert.
    pub fn empfangen(&mut self, index: i32, wert: f32) -> bool {
        if index < ZUSATZ_INDEX_BASE {
            return false;
        }
        if let Some(slot) = self.werte.get_mut((index - ZUSATZ_INDEX_BASE) as usize) {
            *slot = wert.is_finite().then_some(wert);
        }
        true
    }

    /// Verbindung verloren: alte Werte nicht weiter als aktuell zeigen.
    pub fn leeren(&mut self) {
        for v in &mut self.werte {
            *v = None;
        }
    }

    pub fn werte(&self) -> Vec<(String, f64)> {
        self.felder
            .iter()
            .zip(&self.werte)
            .filter_map(|((k, _), v)| v.map(|v| (k.clone(), v as f64)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn liste() -> Vec<(String, String)> {
        vec![
            ("aoa".into(), "sim/flightmodel/position/alpha".into()),
            (
                "n2_1".into(),
                "sim/cockpit2/engine/indicators/N2_percent[0]".into(),
            ),
        ]
    }

    #[test]
    fn indizes_liegen_ueber_katalog_und_proben() {
        let mut z = ZusatzAbos::default();
        z.setzen(liste());
        let abos = z.abos();
        assert_eq!(abos[0].0, ZUSATZ_INDEX_BASE);
        assert_eq!(abos[1].0, ZUSATZ_INDEX_BASE + 1);
        assert!(ZUSATZ_INDEX_BASE > 10_000 + 1_000, "Abstand zu den Proben");
    }

    #[test]
    fn werte_landen_beim_kanal() {
        let mut z = ZusatzAbos::default();
        z.setzen(liste());
        assert!(z.empfangen(ZUSATZ_INDEX_BASE + 1, 81.5));
        assert!(z.empfangen(ZUSATZ_INDEX_BASE, 5.25));
        assert_eq!(z.werte(), vec![("aoa".into(), 5.25), ("n2_1".into(), 81.5)]);
    }

    #[test]
    fn fremde_indizes_werden_nicht_angenommen() {
        let mut z = ZusatzAbos::default();
        z.setzen(liste());
        assert!(!z.empfangen(5, 1.0));
        assert!(!z.empfangen(10_001, 1.0));
        // Im Bereich, aber hinter der Liste (altes Abo nach Verkleinerung).
        assert!(z.empfangen(ZUSATZ_INDEX_BASE + 50, 1.0));
        assert!(z.werte().is_empty());
    }

    #[test]
    fn generation_nur_bei_aenderung() {
        let mut z = ZusatzAbos::default();
        assert!(z.setzen(liste()));
        let g = z.generation;
        assert!(!z.setzen(liste()));
        assert_eq!(z.generation, g);
        assert!(z.setzen(Vec::new()));
        assert!(z.abos().is_empty());
    }

    #[test]
    fn leeren_und_nicht_endliche_werte() {
        let mut z = ZusatzAbos::default();
        z.setzen(liste());
        z.empfangen(ZUSATZ_INDEX_BASE, f32::NAN);
        z.empfangen(ZUSATZ_INDEX_BASE + 1, 3.0);
        assert_eq!(z.werte().len(), 1);
        z.leeren();
        assert!(z.werte().is_empty());
    }
}
