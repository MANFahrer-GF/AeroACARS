    use super::*;
    use std::sync::Barrier;
    use std::thread;

    #[test]
    fn schreibt_wenn_nicht_gestoppt() {
        let lock = Mutex::new(());
        let stop = AtomicBool::new(false);
        let ergebnis = schreiben_falls_aktiv(&lock, &stop, || 42);
        assert_eq!(ergebnis, Some(42));
    }

    #[test]
    fn schreibt_nicht_wenn_bereits_gestoppt() {
        let lock = Mutex::new(());
        let stop = AtomicBool::new(true);
        let mut ausgefuehrt = false;
        let ergebnis = schreiben_falls_aktiv(&lock, &stop, || {
            ausgefuehrt = true;
        });
        assert_eq!(ergebnis, None);
        assert!(
            !ausgefuehrt,
            "die Schreib-Funktion darf gar nicht erst laufen"
        );
    }

    /// Der eigentliche Befund (Codex, externe Gegenpruefung, Runde 17):
    /// ein Schreibzugriff, der seinen `stop`-Check schon bestanden hat und
    /// noch mitten im (hier absichtlich verzoegerten) Schreiben ist, darf
    /// eine GLEICHZEITIGE Loeschung nicht ueberholen — beide kritischen
    /// Abschnitte (Schreiben, Loeschen) duerfen sich NIE zeitlich
    /// ueberlappen. Mit `sleep` im Kern beider Abschnitte kuenstlich
    /// vergroessertes Rennfenster, ueber viele Durchlaeufe wiederholt —
    /// ein fehlendes/kaputtes Schloss wuerde hier zuverlaessig eine
    /// Ueberlappung zeigen.
    #[test]
    fn schreiben_und_loeschen_ueberlappen_sich_nie() {
        for _ in 0..200 {
            let lock = Arc::new(Mutex::new(()));
            let stop = Arc::new(AtomicBool::new(false));
            let gleichzeitig_drin = Arc::new(AtomicBool::new(false));
            let ueberlappung_gesehen = Arc::new(AtomicBool::new(false));
            let barrier = Arc::new(Barrier::new(2));

            let betrete_und_pruefe = {
                let gleichzeitig_drin = Arc::clone(&gleichzeitig_drin);
                let ueberlappung_gesehen = Arc::clone(&ueberlappung_gesehen);
                move || {
                    if gleichzeitig_drin.swap(true, Ordering::SeqCst) {
                        ueberlappung_gesehen.store(true, Ordering::SeqCst);
                    }
                    thread::sleep(std::time::Duration::from_micros(200));
                    gleichzeitig_drin.store(false, Ordering::SeqCst);
                }
            };

            let schreiber = {
                let lock = Arc::clone(&lock);
                let stop = Arc::clone(&stop);
                let betrete_und_pruefe = betrete_und_pruefe.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    schreiben_falls_aktiv(&lock, &stop, betrete_und_pruefe);
                })
            };
            let loescher = {
                let lock = Arc::clone(&lock);
                let stop = Arc::clone(&stop);
                let betrete_und_pruefe = betrete_und_pruefe.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    let _guard = lock.lock().unwrap_or_else(|p| p.into_inner());
                    stop.store(true, Ordering::Relaxed);
                    betrete_und_pruefe();
                })
            };
            schreiber.join().unwrap();
            loescher.join().unwrap();

            assert!(
                !ueberlappung_gesehen.load(Ordering::SeqCst),
                "Schreiben und Loeschen liefen gleichzeitig — das Schloss schuetzt nicht"
            );
        }
    }
