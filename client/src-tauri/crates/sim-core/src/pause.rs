//! Entprellung der Simulator-Pause.
//!
//! LTU 154 (Michael K, 26.10.2026, EDDF 25R) und EDDK 31R am Vortag: MSFS
//! 2024 meldete 150–250 ms nach dem Aufsetzen `Pause_EX1` an → aus → an →
//! aus, jeder Puls 35–140 ms lang. Der Pilot hat nichts gedrueckt. Der
//! Streamer erwischte ein „an", hielt die Aufzeichnung an, und die
//! Lage-Pruefung sah danach „vorher in der Luft, jetzt am Boden". Kurzes
//! Flackern ist haeufig: In den Diagnose-Logs seit 20.09. dauern rund drei
//! Viertel aller Pausen keine Sekunde.
//!
//! Eine Pause gilt deshalb erst, wenn sie `ENTPRELLZEIT` ohne Unterbrechung
//! ansteht. Ein Ende gilt sofort. Preis: Eine echte Pause sehen die Leser
//! von `snap.paused` erst nach dieser Frist — darum kurz, viermal das
//! laengste gemessene Flackern (Codex-Pruefung 26.10.2026).
//!
//! ⚠ Ausgewertet wird beim LESEN des Schnappschusses, nicht beim Bauen.
//! Schickt der Simulator waehrend einer echten Pause keine Daten mehr,
//! bliebe ein beim Bauen entprellter Wert fuer immer „laeuft".

use std::time::{Duration, Instant};

pub const ENTPRELLZEIT: Duration = Duration::from_secs(1);

#[derive(Debug, Default, Clone)]
pub struct PausenEntpreller {
    /// Seit wann der Simulator ununterbrochen „pausiert" meldet.
    pausiert_seit: Option<Instant>,
}

impl PausenEntpreller {
    /// Meldung des Simulators. Ein wiederholtes „an" waehrend einer
    /// laufenden Pause (anderer Pausen-Typ) startet die Frist nicht neu.
    pub fn melden(&mut self, pausiert: bool, jetzt: Instant) {
        if !pausiert {
            self.pausiert_seit = None;
        } else if self.pausiert_seit.is_none() {
            self.pausiert_seit = Some(jetzt);
        }
    }

    /// Gilt die Pause zum Zeitpunkt `jetzt`?
    pub fn pausiert(&self, jetzt: Instant) -> bool {
        self.pausiert_seit
            .is_some_and(|seit| jetzt.saturating_duration_since(seit) >= ENTPRELLZEIT)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    /// Die echten Zeitpunkte aus LTU 154 (17:19:31.392 … .642 UTC), gelesen
    /// zum Zeitpunkt des Streamer-Takts (.636), in dem die Aufzeichnung
    /// anhielt — und danach.
    #[test]
    fn ltu154_flackern_beim_aufsetzen_ist_keine_pause() {
        let t0 = Instant::now();
        let mut e = PausenEntpreller::default();
        e.melden(true, ms(t0, 392));
        assert!(!e.pausiert(ms(t0, 400)));
        e.melden(false, ms(t0, 425));
        e.melden(true, ms(t0, 501));
        assert!(!e.pausiert(ms(t0, 636)), "der Takt, der den Flug anhielt");
        e.melden(false, ms(t0, 642));
        for t in [700, 1_500, 3_000, 10_000] {
            assert!(!e.pausiert(ms(t0, t)), "bei {t} ms");
        }
    }

    #[test]
    fn echte_pause_gilt_nach_der_frist_und_endet_sofort() {
        let t0 = Instant::now();
        let mut e = PausenEntpreller::default();
        e.melden(true, t0);
        assert!(!e.pausiert(ms(t0, 999)));
        assert!(e.pausiert(ms(t0, 1_000)));
        // Kein neuer Wert vom Simulator — die Pause bleibt beim Lesen erkannt.
        assert!(e.pausiert(ms(t0, 600_000)));
        e.melden(false, ms(t0, 600_001));
        assert!(!e.pausiert(ms(t0, 600_002)));
    }

    #[test]
    fn wiederholtes_an_startet_die_frist_nicht_neu() {
        let t0 = Instant::now();
        let mut e = PausenEntpreller::default();
        e.melden(true, t0);
        e.melden(true, ms(t0, 800));
        assert!(e.pausiert(ms(t0, 1_000)));
    }

    #[test]
    fn neues_an_nach_aus_startet_die_frist_neu() {
        let t0 = Instant::now();
        let mut e = PausenEntpreller::default();
        e.melden(true, t0);
        e.melden(false, ms(t0, 900));
        e.melden(true, ms(t0, 950));
        assert!(!e.pausiert(ms(t0, 1_500)));
        assert!(e.pausiert(ms(t0, 1_950)));
    }
}
