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

/// Und insgesamt höchstens so viele je Programmlauf. Hält eine Stelle auf
/// einem langsamen Rechner die Sperre in jedem Takt 22 ms, wären es sonst
/// tausende Zeilen je Flugstunde.
const MELDUNGEN_HOECHSTENS: u32 = 200;

/// So viele Zwischenstände merkt sich ein gehaltener Zugriff höchstens.
const ZWISCHENSTAENDE: usize = 16;

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
            #[cfg(test)]
            sperre: &self.inner,
            stelle,
            ab,
            gewartet,
            zwischenstaende: [None; ZWISCHENSTAENDE],
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
    // Nur für den Test der Reihenfolge „erst freigeben, dann melden“.
    #[cfg(test)]
    sperre: &'a Mutex<T>,
    stelle: &'static Location<'static>,
    ab: Instant,
    gewartet: Duration,
    zwischenstaende: [Option<(&'static str, Instant)>; ZWISCHENSTAENDE],
}

impl<T> Gehalten<'_, T> {
    /// Ein Abschnitt innerhalb des gehaltenen Zugriffs ist fertig.
    ///
    /// v2.0.18: Der Wächter nannte bisher nur die Stelle, an der die Sperre
    /// genommen wurde — bei `step_flight_at` ist das eine Funktion über
    /// 2500 Zeilen. Mit Zwischenständen sagt eine lange Haltezeit auch,
    /// welcher Teil sie gekostet hat (`teile=sprit:0.4 aufsetzen_bahn:118.2
    /// …`). Kostet je Aufruf eine Uhrzeit; ausgewertet wird nur, wenn
    /// gemeldet wird. Mehr als `ZWISCHENSTAENDE` werden verworfen.
    pub(crate) fn zwischenstand(&mut self, name: &'static str) {
        if let Some(frei) = self.zwischenstaende.iter_mut().find(|z| z.is_none()) {
            *frei = Some((name, Instant::now()));
        }
    }

    /// `name:ms` je Abschnitt (Zeit seit dem vorigen), dazu `rest`.
    fn teile(&self, ende: Instant) -> String {
        let mut davor = self.ab;
        let mut teile = Vec::new();
        for (name, t) in self.zwischenstaende.iter().flatten() {
            teile.push(format!(
                "{name}:{:.1}",
                t.duration_since(davor).as_secs_f64() * 1000.0
            ));
            davor = *t;
        }
        if teile.is_empty() {
            return String::new();
        }
        teile.push(format!(
            "rest:{:.1}",
            ende.duration_since(davor).as_secs_f64() * 1000.0
        ));
        teile.join(" ")
    }
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
        let ende = Instant::now();
        let gehalten = ende.duration_since(self.ab);
        // Erst freigeben, dann melden.
        drop(self.guard.take());
        if gehalten >= SCHWELLE || self.gewartet >= SCHWELLE {
            #[cfg(test)]
            test_messung::beim_melden_frei(self.sperre.try_lock().is_ok());
            let teile = self.teile(ende);
            #[cfg(test)]
            test_messung::teile_merken(&teile);
            melden(self.stelle, gehalten, self.gewartet, &teile);
        }
        #[cfg(test)]
        test_messung::aufzeichnen(
            self.stelle,
            gehalten,
            self.zwischenstaende
                .iter()
                .flatten()
                .map(|(n, _)| *n)
                .collect(),
        );
    }
}

fn melden(stelle: &'static Location<'static>, gehalten: Duration, gewartet: Duration, teile: &str) {
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
    static GEMELDET: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let nummer = GEMELDET.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if nummer > MELDUNGEN_HOECHSTENS {
        return;
    }
    tracing::warn!(
        stelle = %format_args!("{}:{}", stelle.file(), stelle.line()),
        meldung = nummer,
        letzte = nummer == MELDUNGEN_HOECHSTENS,
        gehalten_ms = gehalten.as_secs_f64() * 1000.0,
        gewartet_ms = gewartet.as_secs_f64() * 1000.0,
        teile = %teile,
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

    thread_local! {
        static BEIM_MELDEN_FREI: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    }

    thread_local! {
        static GEMELDETE_TEILE: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(super) fn teile_merken(teile: &str) {
        GEMELDETE_TEILE.with(|z| *z.borrow_mut() = Some(teile.to_string()));
    }

    pub(super) fn beim_melden_frei(frei: bool) {
        BEIM_MELDEN_FREI.with(|z| z.set(Some(frei)));
    }

    pub(crate) fn beginnen() {
        ABSCHNITTE.with(|a| a.borrow_mut().clear());
        HALTEZEITEN.with(|h| *h.borrow_mut() = Some(Vec::new()));
    }

    pub(crate) fn beenden() -> Vec<(&'static Location<'static>, Duration)> {
        HALTEZEITEN.with(|h| h.borrow_mut().take().unwrap_or_default())
    }

    thread_local! {
        static ABSCHNITTE: RefCell<Vec<Vec<&'static str>>> = const { RefCell::new(Vec::new()) };
    }

    /// Die Zwischenstände aller seit `beginnen` gehaltenen Zugriffe.
    pub(crate) fn abschnitte() -> Vec<Vec<&'static str>> {
        ABSCHNITTE.with(|a| a.borrow().clone())
    }

    pub(super) fn aufzeichnen(
        stelle: &'static Location<'static>,
        gehalten: Duration,
        abschnitte: Vec<&'static str>,
    ) {
        if HALTEZEITEN.with(|h| h.borrow().is_some()) {
            ABSCHNITTE.with(|a| a.borrow_mut().push(abschnitte));
        }
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
        assert_eq!(
            BEIM_MELDEN_FREI.with(|z| z.get()),
            Some(true),
            "beim Melden war die Sperre noch gehalten (oder es wurde nicht gemeldet)"
        );
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

    /// v2.0.18: Eine lange Haltezeit nennt ihre Abschnitte — und der lange
    /// Abschnitt traegt die Zeit, nicht ein anderer.
    #[test]
    fn eine_lange_haltezeit_nennt_ihre_abschnitte() {
        let sperre = GemesseneSperre::new(0u32);
        {
            let mut g = sperre.lock().unwrap();
            g.zwischenstand("schnell");
            std::thread::sleep(Duration::from_millis(30));
            g.zwischenstand("langsam");
        }
        let teile = GEMELDETE_TEILE
            .with(|z| z.borrow_mut().take())
            .expect("30 ms gehalten, aber nicht gemeldet");
        let ms = |name: &str| -> f64 {
            teile
                .split(' ')
                .find_map(|t| t.strip_prefix(&format!("{name}:")))
                .unwrap_or_else(|| panic!("{name} fehlt in {teile:?}"))
                .parse()
                .unwrap()
        };
        // Ohne Obergrenzen fuer die kurzen Abschnitte: Eine Unterbrechung
        // durch den Scheduler darf den Test nicht rot machen (Codex 2.0.18,
        // P3). Geprueft wird die Zuordnung — die 30 ms gehoeren zu
        // `langsam`, und die Abschnitte stehen in Ablaufreihenfolge.
        assert!(ms("langsam") >= 29.0, "{teile}");
        let reihenfolge: Vec<&str> = teile
            .split(' ')
            .map(|t| t.split(':').next().unwrap())
            .collect();
        assert_eq!(reihenfolge, ["schnell", "langsam", "rest"], "{teile}");
    }
}
