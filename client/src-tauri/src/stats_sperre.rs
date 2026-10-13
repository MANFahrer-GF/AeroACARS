//! Wächter um die Flugdaten-Sperre (`ActiveFlight::stats`).
//!
//! v2.0.17, Anlass AIB 431 (13.10.2026): Seit 2.0.14 hatte jeder Flug genau
//! eine Lücke im Aufsetz-Abtaster, kurz nach dem Aufsetzen, und sie wuchs mit
//! der Größe des Platzes (Mumbai 58 ms, Dubai 129 ms). Ursache war Arbeit in
//! Flughafengröße, die die Landungsauswertung UNTER der Sperre erledigte —
//! der Abtaster wartete so lange. Im Log stand davon nichts: Die Takt-Meldung
//! des Abtasters galt nur in der Luft, die des Streamers erst ab 250 ms.
//!
//! Damit der nächste Fall dieser Art ohne Detektivarbeit sichtbar ist, misst
//! dieser Wächter JEDEN Zugriff: wie lange auf die Sperre gewartet und wie
//! lange sie gehalten wurde, mit der Aufrufstelle (`#[track_caller]`). Ab
//! [`SCHWELLE`] steht das im Log, in jeder Flugphase.
//!
//! ⚠ Gemeldet wird erst NACH dem Freigeben der Sperre. Die Logzeile wird
//! synchron geschrieben (Absturz-Forensik, siehe `init_tracing`) — unter der
//! Sperre würde die Messung selbst den Fehler verursachen, den sie misst.
//!
//! Regel für alles, was diese Sperre hält: keine Ein-/Ausgabe, kein
//! Logging, keine Arbeit, die mit der Größe des Flughafens wächst.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::panic::Location;
use std::sync::{LockResult, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Ab hier wird ein Halten oder Warten gemeldet. Der Abtaster misst alle
/// 20 ms — wer die Sperre länger hält, kostet ihn mindestens einen Messwert.
pub(crate) const SCHWELLE: Duration = Duration::from_millis(20);

/// Je Aufrufstelle höchstens eine Meldung in diesem Abstand — ein Dauer-
/// problem soll sichtbar sein, das Protokoll aber nicht fluten.
const MELDEABSTAND: Duration = Duration::from_secs(1);

/// `Mutex` mit Messung. Gleiche Schnittstelle wie `Mutex::lock`, damit jede
/// bestehende Aufrufstelle (`expect`, `unwrap_or_else(into_inner)`, …)
/// unverändert bleibt.
#[derive(Debug, Default)]
pub(crate) struct GemesseneSperre<T> {
    inner: Mutex<T>,
}

impl<T> GemesseneSperre<T> {
    pub(crate) fn new(wert: T) -> Self {
        Self {
            inner: Mutex::new(wert),
        }
    }

    #[track_caller]
    pub(crate) fn lock(&self) -> LockResult<Gehalten<'_, T>> {
        let stelle = Location::caller();
        let warte_start = Instant::now();
        let ergebnis = self.inner.lock();
        let ab = Instant::now();
        let gewartet = ab.duration_since(warte_start);
        let verpacken = |guard| Gehalten {
            guard: Some(guard),
            stelle,
            ab,
            gewartet,
        };
        match ergebnis {
            Ok(g) => Ok(verpacken(g)),
            Err(e) => Err(PoisonError::new(verpacken(e.into_inner()))),
        }
    }
}

/// Der gehaltene Zugriff. Misst beim Freigeben.
pub(crate) struct Gehalten<'a, T> {
    // `Option` nur, damit `drop` die echte Sperre VOR dem Melden freigeben
    // kann. Außerhalb von `drop` immer `Some`.
    guard: Option<MutexGuard<'a, T>>,
    stelle: &'static Location<'static>,
    ab: Instant,
    gewartet: Duration,
}

impl<T> Deref for Gehalten<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard.as_ref().expect("Sperre gehalten")
    }
}

impl<T> DerefMut for Gehalten<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("Sperre gehalten")
    }
}

impl<T> Drop for Gehalten<'_, T> {
    fn drop(&mut self) {
        let gehalten = self.ab.elapsed();
        // Erst freigeben, dann melden.
        drop(self.guard.take());
        #[cfg(test)]
        test_messung::aufzeichnen(self.stelle, gehalten);
        if gehalten >= SCHWELLE || self.gewartet >= SCHWELLE {
            melden(self.stelle, gehalten, self.gewartet);
        }
    }
}

fn melden(stelle: &'static Location<'static>, gehalten: Duration, gewartet: Duration) {
    static ZULETZT: Mutex<Option<HashMap<(&'static str, u32), Instant>>> = Mutex::new(None);
    {
        let mut zuletzt = ZULETZT.lock().unwrap_or_else(|e| e.into_inner());
        let karte = zuletzt.get_or_insert_with(HashMap::new);
        let schluessel = (stelle.file(), stelle.line());
        let jetzt = Instant::now();
        if karte
            .get(&schluessel)
            .is_some_and(|t| jetzt.duration_since(*t) < MELDEABSTAND)
        {
            return;
        }
        karte.insert(schluessel, jetzt);
    }
    tracing::warn!(
        stelle = %format_args!("{}:{}", stelle.file(), stelle.line()),
        gehalten_ms = gehalten.as_secs_f64() * 1000.0,
        gewartet_ms = gewartet.as_secs_f64() * 1000.0,
        "Flugdaten-Sperre lange gehalten oder gewartet"
    );
}

/// Für Tests: alle Haltezeiten dieses Fadens mitschreiben. Nur so lässt sich
/// prüfen, wie lange ein Takt die Sperre hält, ohne die Uhr des ganzen Tests
/// (Aufbau, Szenerie erzeugen) mitzumessen.
#[cfg(test)]
pub(crate) mod test_messung {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static HALTEZEITEN: RefCell<Option<Vec<(&'static Location<'static>, Duration)>>> =
            const { RefCell::new(None) };
    }

    pub(crate) fn beginnen() {
        HALTEZEITEN.with(|h| *h.borrow_mut() = Some(Vec::new()));
    }

    pub(crate) fn beenden() -> Vec<(&'static Location<'static>, Duration)> {
        HALTEZEITEN.with(|h| h.borrow_mut().take().unwrap_or_default())
    }

    pub(super) fn aufzeichnen(stelle: &'static Location<'static>, gehalten: Duration) {
        HALTEZEITEN.with(|h| {
            if let Some(v) = h.borrow_mut().as_mut() {
                v.push((stelle, gehalten));
            }
        });
    }

    #[test]
    fn meldet_stelle_und_haltezeit_und_gibt_vor_dem_melden_frei() {
        let sperre = GemesseneSperre::new(0u32);
        beginnen();
        {
            let mut g = sperre.lock().unwrap();
            *g += 1;
            std::thread::sleep(Duration::from_millis(25));
        }
        let gemessen = beenden();
        assert_eq!(gemessen.len(), 1);
        assert_eq!(gemessen[0].0.file(), file!());
        assert!(gemessen[0].1 >= Duration::from_millis(25));
        // Nach dem Freigeben sofort wieder frei.
        assert_eq!(*sperre.lock().unwrap(), 1);
    }

    #[test]
    fn vergiftete_sperre_bleibt_wie_beim_mutex_nutzbar() {
        let sperre = std::sync::Arc::new(GemesseneSperre::new(5u32));
        let s2 = sperre.clone();
        let _ = std::thread::spawn(move || {
            let _g = s2.lock().unwrap();
            panic!("absichtlich");
        })
        .join();
        let g = sperre.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(*g, 5);
    }
}
