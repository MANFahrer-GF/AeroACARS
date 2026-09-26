//! Halter fuer blinkende Cockpit-Lampen (MASTER WARNING / CAUTION).
//!
//! Log-Durchsicht 26.09.2026: Die Fenix-LVar `L:I_MIP_MASTER_WARNING_CAPT`
//! ist die blinkende Lampe selbst (~0,6 s an / 0,6 s aus). Der Streamer tastet
//! im Reiseflug nur alle 3 s ab; die 2-s-Entprellung dort wurde so zu „zwei
//! gleiche Proben", und aus dem Blinken entstand ein Scheinsignal an/aus alle
//! 6–20 s — ~530 Eintraege in einer Nacht. Der Halter sitzt deshalb im
//! schnellen Adaptertakt: an beim ersten Aufleuchten, aus erst, wenn die
//! Lampe `HALTEZEIT` lang dunkel war.

use std::time::{Duration, Instant};

/// Laenger als eine Blinkperiode und laenger als der langsamste
/// Streamer-Takt (3 s), damit ein dauernd blinkendes Signal fuer den
/// Streamer ununterbrochen „an" ist.
pub const HALTEZEIT: Duration = Duration::from_millis(3_500);

#[derive(Debug, Default, Clone)]
pub struct LampenHalter {
    zuletzt_an: Option<Instant>,
}

impl LampenHalter {
    /// Rohwert der Lampe in diesem Takt → gehaltener Wert. `None` (Lampe
    /// nicht messbar) bleibt `None` und vergisst den Halt.
    pub fn update(&mut self, roh: Option<bool>, jetzt: Instant) -> Option<bool> {
        match roh {
            None => {
                self.zuletzt_an = None;
                None
            }
            Some(true) => {
                self.zuletzt_an = Some(jetzt);
                Some(true)
            }
            Some(false) => Some(
                self.zuletzt_an
                    .is_some_and(|t| jetzt.saturating_duration_since(t) < HALTEZEIT),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blinken_im_fenix_takt_bleibt_durchgehend_an() {
        let t0 = Instant::now();
        let mut h = LampenHalter::default();
        // 20 s Blinken, 0,6 s an / 0,6 s aus, abgetastet mit 30 Hz.
        let mut schritt = 0u64;
        while schritt < 600 {
            let ms = schritt * 33;
            let an = (ms / 600) % 2 == 0;
            let wert = h.update(Some(an), t0 + Duration::from_millis(ms));
            assert_eq!(wert, Some(true), "bei {ms} ms");
            schritt += 1;
        }
    }

    #[test]
    fn aus_erst_nach_haltezeit_dunkel() {
        let t0 = Instant::now();
        let mut h = LampenHalter::default();
        assert_eq!(h.update(Some(true), t0), Some(true));
        assert_eq!(
            h.update(Some(false), t0 + Duration::from_secs(3)),
            Some(true)
        );
        assert_eq!(
            h.update(Some(false), t0 + Duration::from_secs(4)),
            Some(false)
        );
    }

    #[test]
    fn nie_an_bleibt_aus_und_nicht_messbar_bleibt_none() {
        let t0 = Instant::now();
        let mut h = LampenHalter::default();
        assert_eq!(h.update(Some(false), t0), Some(false));
        assert_eq!(h.update(None, t0), None);
    }
}
