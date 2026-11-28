//! Ausfall der Broker-Leitung im Diagnose-Log: melden, ohne es zu fluten.
//!
//! Der Neuaufbau bleibt beim festen 5-s-Takt (eine Position im Flug soll
//! schnell wieder ankommen, ein Verbindungsversuch kostet nichts). Gemeldet
//! wird aber nicht jeder Versuch: Am 24.11.2026 war der Live-Server rund
//! 13 Stunden weg und stand mit 4971× „No route to host“ im Log eines
//! Piloten, dazu je Versuch zwei Abo-Zeilen.

use std::time::Duration;
use tokio::time::Instant;

/// So oft erinnert das Log, dass die Leitung noch fehlt.
pub(crate) const AUSFALL_ERINNERUNG: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(crate) struct AusfallProtokoll {
    /// `Some` = ein Ausfall läuft: (Beginn, letzte Meldung, Fehlversuche).
    lauf: Option<(Instant, Instant, u32)>,
}

impl AusfallProtokoll {
    /// Ein Fehlversuch. `Some` = diese Zeile gehört ins Log: der erste sofort,
    /// danach höchstens alle [`AUSFALL_ERINNERUNG`].
    pub(crate) fn fehlversuch(&mut self, jetzt: Instant, grund: &str) -> Option<String> {
        let Some((seit, gemeldet, versuche)) = self.lauf.as_mut() else {
            self.lauf = Some((jetzt, jetzt, 1));
            return Some(format!(
                "MQTT poll error: {grund} — neuer Versuch alle 5 s, weitere Meldung \
                 frühestens in {} min",
                AUSFALL_ERINNERUNG.as_secs() / 60
            ));
        };
        *versuche += 1;
        if jetzt.duration_since(*gemeldet) < AUSFALL_ERINNERUNG {
            return None;
        }
        *gemeldet = jetzt;
        Some(format!(
            "MQTT weiter getrennt seit {} min ({} Fehlversuche), zuletzt: {grund}",
            jetzt.duration_since(*seit).as_secs() / 60,
            versuche
        ))
    }

    /// Verbindung steht wieder. `Some` nur, wenn vorher ein Ausfall lief.
    pub(crate) fn verbunden(&mut self, jetzt: Instant) -> Option<String> {
        let (seit, _, versuche) = self.lauf.take()?;
        Some(format!(
            "MQTT wieder verbunden nach {} s ({versuche} Fehlversuche)",
            jetzt.duration_since(seit).as_secs()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dreizehn_stunden_ausfall_ergeben_eine_zeile_je_fuenf_minuten() {
        let start = Instant::now();
        let mut p = AusfallProtokoll::default();
        let zeilen: Vec<(u64, String)> = (0..13 * 720u64)
            .filter_map(|i| {
                p.fehlversuch(start + Duration::from_secs(i * 5), "No route to host")
                    .map(|z| (i * 5, z))
            })
            .collect();
        // t = 0 und dann alle 300 s bis kurz vor 13 h.
        assert_eq!(zeilen.len(), 13 * 12);
        assert!(zeilen.windows(2).all(|w| w[1].0 - w[0].0 == 300));
        assert!(zeilen[0].1.contains("No route to host"));
        assert!(
            zeilen[1].1.contains("seit 5 min (61 Fehlversuche)"),
            "{}",
            zeilen[1].1
        );
    }

    #[test]
    fn wieder_verbunden_nennt_dauer_und_beginnt_neu() {
        let start = Instant::now();
        let mut p = AusfallProtokoll::default();
        assert_eq!(p.verbunden(start), None, "ohne Ausfall keine Meldung");
        assert!(p.fehlversuch(start, "a").is_some());
        assert!(p.fehlversuch(start + Duration::from_secs(5), "b").is_none());
        let z = p.verbunden(start + Duration::from_secs(12)).unwrap();
        assert!(z.contains("nach 12 s (2 Fehlversuche)"), "{z}");
        assert_eq!(p.verbunden(start + Duration::from_secs(13)), None);
        assert!(
            p.fehlversuch(start + Duration::from_secs(20), "c")
                .is_some(),
            "der naechste Ausfall meldet sich wieder sofort"
        );
    }
}
