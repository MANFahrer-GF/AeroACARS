//! Plattformteile. Unter Windows echt (Known Folder, Registry, Vordergrundfenster,
//! Fensteraufnahme, Hotkey); sonst Ersatz, der „nicht verfuegbar“ meldet oder
//! nichts findet — nie einen Fehler beim Aufspueren (P-11.30).

#[cfg(windows)]
mod wgc;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
mod ersatz;
#[cfg(not(windows))]
pub use ersatz::*;

/// Ergebnis der Fensteraufnahme: BGRA, oben links beginnend.
#[derive(Debug, Clone)]
pub struct Aufnahme {
    pub breite: u32,
    pub hoehe: u32,
    pub bgra: Vec<u8>,
    /// Womit aufgenommen wurde: `wgc`, `printwindow` oder `bitblt`.
    pub methode: &'static str,
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::Fehler;

/// So lange darf eine Fensteraufnahme hoechstens dauern (`PrintWindow` kann bei
/// einem haengenden Simulator ohne Ende warten).
pub const AUFNAHME_FRIST: Duration = Duration::from_secs(5);

/// Setzt die Marke zurueck, wenn der Arbeitsfaden endet (auch bei einem Absturz).
struct Frei(&'static AtomicBool);

impl Drop for Frei {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// Fuehrt `arbeit` in einem eigenen Faden aus und wartet hoechstens `frist`.
/// Kommt bis dahin nichts, ist das ein Fehler und der Aufrufer ist frei; der
/// Faden darf haengen bleiben (kein Hilfsprozess), wird aber nie ein zweites Mal
/// gestartet, solange er laeuft (`laeuft`, hoechstens einer gleichzeitig). Ein
/// Ergebnis, das zu spaet kommt, wird verworfen.
pub fn mit_frist<T: Send + 'static>(
    frist: Duration,
    laeuft: &'static AtomicBool,
    arbeit: impl FnOnce() -> Result<T, Fehler> + Send + 'static,
) -> Result<T, Fehler> {
    if laeuft.swap(true, Ordering::SeqCst) {
        return Err(Fehler::Io("Die vorige Aufnahme haengt noch (der Simulator reagiert nicht)".into()));
    }
    let frei = Frei(laeuft);
    let (tx, rx) = mpsc::channel();
    let start = std::thread::Builder::new().name("bild-aufnahme-faden".into()).spawn(move || {
        let ergebnis = arbeit();
        drop(frei);
        let _ = tx.send(ergebnis); // niemand mehr da (Frist vorbei): verworfen
    });
    if let Err(e) = start {
        // Der Faden kam nie zustande; sein Closure samt `Frei` ist hier schon freigegeben.
        return Err(Fehler::Io(format!("Aufnahme-Faden nicht startbar: {e}")));
    }
    match rx.recv_timeout(frist) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Fehler::Io(format!(
            "Der Simulator hat nicht innerhalb von {} s geantwortet; die Aufnahme wurde abgebrochen",
            frist.as_secs().max(1)
        ))),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(Fehler::Io("Die Aufnahme ist abgestuerzt".into())),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn schnelle_arbeit_liefert_ihr_ergebnis_und_gibt_die_marke_frei() {
        static L: AtomicBool = AtomicBool::new(false);
        assert_eq!(mit_frist(Duration::from_secs(2), &L, || Ok(7)).unwrap(), 7);
        assert!(!L.load(Ordering::SeqCst));
        assert!(matches!(mit_frist::<u8>(Duration::from_secs(2), &L, || Err(Fehler::Io("x".into()))), Err(Fehler::Io(_))));
        assert!(!L.load(Ordering::SeqCst));
    }

    /// Fund 4: haengende Arbeit gibt den Aufrufer nach der Frist frei, ein
    /// zweiter haengender Faden entsteht nicht, ein spaetes Ergebnis ist verworfen.
    #[test]
    fn haengende_arbeit_gibt_nach_der_frist_frei_hoechstens_ein_haengender_faden_spaetes_ergebnis_verworfen() {
        static L: AtomicBool = AtomicBool::new(false);
        let loslassen = Arc::new(AtomicBool::new(false));
        let gestartet = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let (l2, g2) = (Arc::clone(&loslassen), Arc::clone(&gestartet));
        let t = std::time::Instant::now();
        let r = mit_frist(Duration::from_millis(200), &L, move || {
            g2.fetch_add(1, Ordering::SeqCst);
            while !l2.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(1)
        });
        assert!(matches!(r, Err(Fehler::Io(_))), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(2), "kehrt nach der Frist zurueck: {:?}", t.elapsed());
        assert!(L.load(Ordering::SeqCst), "der haengende Faden belegt die Marke weiter");
        // Solange er haengt, startet kein zweiter.
        let g3 = Arc::clone(&gestartet);
        let zweiter = mit_frist(Duration::from_millis(200), &L, move || {
            g3.fetch_add(1, Ordering::SeqCst);
            Ok(2)
        });
        assert!(matches!(zweiter, Err(Fehler::Io(_))));
        assert_eq!(gestartet.load(Ordering::SeqCst), 1, "es lief nur der eine Faden");
        // Er kommt spaet zurueck: Ergebnis verworfen, Marke frei, danach geht es wieder.
        loslassen.store(true, Ordering::SeqCst);
        let start = std::time::Instant::now();
        while L.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!L.load(Ordering::SeqCst));
        assert_eq!(mit_frist(Duration::from_secs(2), &L, || Ok(3)).unwrap(), 3);
    }
}
