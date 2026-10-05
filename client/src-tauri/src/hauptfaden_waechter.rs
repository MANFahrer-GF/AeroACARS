//! Wächter für den Hauptfaden (Ereignisschleife der Oberfläche).
//!
//! v1.9.24, Anlass Adrian (04.10.2026): Das Fenster fror ein, während Funk,
//! Positionen und Heartbeats im Hintergrund weiterliefen. Im Diagnose-Log
//! stand davon NICHTS — die Ursache (Deadlock beim Sichern der Fensterlage)
//! ließ sich nur aus dem Fehlen einer Zeile erschließen. Damit der nächste
//! Hänger, gleich welcher Ursache, ohne Pilotenmeldung sichtbar ist:
//!
//! Ein Nebenfaden schickt jede Sekunde eine winzige Aufgabe an den
//! Hauptfaden (`run_on_main_thread`) und schaut, ob sie ausgeführt wurde.
//! Bleibt sie länger als [`SCHWELLE_MS`] liegen, steht das im Log und geht
//! an GlitchTip; kommt der Hauptfaden zurück, steht die Dauer im Log.
//!
//! Immer nur EINE Aufgabe unterwegs — ein hängender Hauptfaden sammelt
//! keine Warteschlange an. Schlief der Rechner (der Wächter selbst kam viel
//! zu spät dran), zählt die Pause nicht als Hänger.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Runtime};

/// Ab hier gilt der Hauptfaden als hängend. Großzügig: echte Arbeit auf dem
/// Hauptfaden dauert Millisekunden, ein Hänger dauert, bis der Pilot die
/// App abschießt.
pub(crate) const SCHWELLE_MS: u64 = 8_000;
const TAKT: Duration = Duration::from_secs(1);
/// Kam der Wächter um so viel später dran als geplant, hat der Rechner
/// geschlafen (oder stand still) — dann ist auch der Hauptfaden entschuldigt.
const VERSCHLAFEN_AB_MS: u64 = 5_000;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Bericht {
    Haengt { seit_ms: u64 },
    WiederDa { nach_ms: u64 },
}

/// Reiner Zustand, ohne Uhr und ohne Tauri — testbar.
#[derive(Debug, Default)]
pub(crate) struct Waechter {
    gesendet_seq: u64,
    gesendet_ms: u64,
    gemeldet: bool,
}

impl Waechter {
    /// Ein Takt. `beantwortet_seq`: zuletzt vom Hauptfaden ausgeführte
    /// Aufgabe. `verschlafen`: der Takt selbst kam viel zu spät.
    /// Rückgabe: ggf. ein Bericht und ggf. die Nummer einer neu zu
    /// sendenden Aufgabe.
    pub(crate) fn takt(
        &mut self,
        jetzt_ms: u64,
        beantwortet_seq: u64,
        verschlafen: bool,
    ) -> (Option<Bericht>, Option<u64>) {
        let ausstehend = beantwortet_seq < self.gesendet_seq;
        if ausstehend {
            if verschlafen && !self.gemeldet {
                // Die Pause gehört dem Rechner, nicht dem Hauptfaden.
                self.gesendet_ms = jetzt_ms;
                return (None, None);
            }
            let seit = jetzt_ms.saturating_sub(self.gesendet_ms);
            if seit >= SCHWELLE_MS && !self.gemeldet {
                self.gemeldet = true;
                return (Some(Bericht::Haengt { seit_ms: seit }), None);
            }
            return (None, None);
        }
        let bericht = if self.gemeldet {
            self.gemeldet = false;
            Some(Bericht::WiederDa {
                nach_ms: jetzt_ms.saturating_sub(self.gesendet_ms),
            })
        } else {
            None
        };
        self.gesendet_seq += 1;
        self.gesendet_ms = jetzt_ms;
        (bericht, Some(self.gesendet_seq))
    }
}

/// Startet den Wächter-Faden. Einmal, im Setup.
pub(crate) fn starten<R: Runtime>(app: AppHandle<R>) {
    let beantwortet = Arc::new(AtomicU64::new(0));
    let gestartet = std::thread::Builder::new()
        .name("hauptfaden-waechter".into())
        .spawn(move || {
            let start = Instant::now();
            let mut w = Waechter::default();
            let mut letzter_takt = Instant::now();
            loop {
                std::thread::sleep(TAKT);
                let verspaetung = letzter_takt.elapsed().saturating_sub(TAKT);
                letzter_takt = Instant::now();
                let verschlafen = verspaetung.as_millis() as u64 >= VERSCHLAFEN_AB_MS;
                let jetzt_ms = start.elapsed().as_millis() as u64;
                let (bericht, senden) =
                    w.takt(jetzt_ms, beantwortet.load(Ordering::Acquire), verschlafen);
                match bericht {
                    Some(Bericht::Haengt { seit_ms }) => {
                        tracing::warn!(
                            seit_ms,
                            "Hauptfaden reagiert nicht — Oberfläche eingefroren"
                        );
                        crate::sentry_init::capture_activity(
                            "Hauptfaden reagiert nicht (Oberfläche eingefroren)",
                            Some(&format!("seit {seit_ms} ms")),
                            sentry::Level::Warning,
                        );
                    }
                    Some(Bericht::WiederDa { nach_ms }) => {
                        tracing::warn!(nach_ms, "Hauptfaden reagiert wieder");
                    }
                    None => {}
                }
                if let Some(seq) = senden {
                    let antwort = beantwortet.clone();
                    if app
                        .run_on_main_thread(move || antwort.store(seq, Ordering::Release))
                        .is_err()
                    {
                        // Ereignisschleife beendet: App geht zu.
                        break;
                    }
                }
            }
        });
    if let Err(e) = gestartet {
        tracing::warn!(error = %e, "Hauptfaden-Wächter ließ sich nicht starten");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gesunder_hauptfaden_bleibt_still() {
        let mut w = Waechter::default();
        let mut beantwortet = 0;
        for t in 1..=60u64 {
            let (b, s) = w.takt(t * 1000, beantwortet, false);
            assert_eq!(b, None, "Takt {t}");
            beantwortet = s.expect("jeden Takt eine neue Aufgabe");
        }
    }

    #[test]
    fn haenger_wird_einmal_gemeldet_und_die_erholung_mit_dauer() {
        let mut w = Waechter::default();
        let (_, s) = w.takt(1_000, 0, false);
        assert_eq!(s, Some(1));
        // Aufgabe 1 wird nie ausgeführt.
        for t in 2..=8u64 {
            assert_eq!(
                w.takt(t * 1000, 0, false),
                (None, None),
                "vor der Schwelle, Takt {t}"
            );
        }
        assert_eq!(
            w.takt(9_000, 0, false),
            (Some(Bericht::Haengt { seit_ms: 8_000 }), None)
        );
        // Weiter hängend: keine zweite Meldung, keine neue Aufgabe (keine Warteschlange).
        for t in 10..=30u64 {
            assert_eq!(w.takt(t * 1000, 0, false), (None, None));
        }
        // Hauptfaden kommt zurück.
        assert_eq!(
            w.takt(31_000, 1, false),
            (Some(Bericht::WiederDa { nach_ms: 30_000 }), Some(2))
        );
    }

    #[test]
    fn rechnerschlaf_ist_kein_haenger() {
        let mut w = Waechter::default();
        w.takt(1_000, 0, false);
        // Rechner schlief 1 h; der Wächter kommt spät dran, Aufgabe 1 noch offen.
        assert_eq!(w.takt(3_601_000, 0, true), (None, None));
        // Danach normal beantwortet: nichts zu melden.
        assert_eq!(w.takt(3_602_000, 1, false), (None, Some(2)));
    }

    #[test]
    fn haenger_nach_dem_schlaf_wird_ab_dem_aufwachen_gezaehlt() {
        let mut w = Waechter::default();
        w.takt(1_000, 0, false);
        w.takt(3_601_000, 0, true);
        assert_eq!(w.takt(3_608_000, 0, false), (None, None));
        assert_eq!(
            w.takt(3_609_000, 0, false),
            (Some(Bericht::Haengt { seit_ms: 8_000 }), None)
        );
    }
}
