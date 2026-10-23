//! Kabinen-Reaktionen (Thomas 23.10.2026): Applaus, Murmeln oder Raunen
//! nach der Landung, leise durch die Cockpittür. WANN und WAS entscheidet
//! `kabinenansagen` mit `cabin_announcements::reaktion`; hier liegt nur die
//! Wiedergabe:
//! - einmal, ein eigener cpal-Strom auf dem Ausgabegerät der Ansagen;
//! - Pegel = Ansagen-Lautstärke × [`PEGEL`] (kein eigener Regler);
//! - Einblenden steckt in der Aufnahme; beginnt eine Ansage oder die Musik,
//!   blendet die Reaktion über [`AUSBLENDEN_S`] aus und endet — sie kommt
//!   anders als die Musik nicht wieder.
//! - Die Aufnahmen liegen als Ressource im Programm (`kabine-reaktionen/`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::kabinen_musik::{Dekoder, Umrechner};

/// Anteil der Ansagen-Lautstärke. Gemessen 23.10.2026: normale Ansagen
/// ~ −25 LUFS, Reaktionen −23 LUFS (Murmeln −25) → 0,7 (≈ −3 dB) legt sie
/// etwa auf Ansagen-Höhe.
pub(crate) const PEGEL: f32 = 0.7;
/// Ausblenden, wenn eine Ansage oder die Musik beginnt (s).
pub(crate) const AUSBLENDEN_S: f32 = 2.0;
/// Ordner der Aufnahmen unter den Programm-Ressourcen.
pub(crate) const ORDNER: &str = "kabine-reaktionen";

/// Erhöht sich mit jedem Auftrag; ein Faden mit älterer Fassung endet.
static FASSUNG: AtomicU64 = AtomicU64::new(0);
static LAEUFT: AtomicBool = AtomicBool::new(false);
/// Warum die letzte Reaktion endete — holt der Kabinen-Takt fürs Protokoll ab.
static ENDE: Mutex<Option<&'static str>> = Mutex::new(None);

/// Aufnahmen einer Art (`<art>-<n>.mp3`), sortiert.
pub(crate) fn aufnahmen(ordner: &Path, art: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(ordner)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mp3"))
                && p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.strip_prefix(art)).is_some_and(|r| r.starts_with('-'))
        })
        .collect();
    v.sort();
    v
}

/// Spielt `datei` einmal. Eine laufende Reaktion wird ersetzt.
pub(crate) fn spielen(datei: PathBuf, geraet: Option<String>) {
    let fassung = FASSUNG.fetch_add(1, Ordering::SeqCst) + 1;
    LAEUFT.store(true, Ordering::SeqCst);
    let _ = std::thread::Builder::new().name("kabinen-reaktion".into()).spawn(move || {
        let grund = match wiedergabe::abspielen(&datei, geraet.as_deref(), fassung) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(datei = %datei.display(), error = %e, "kabinen-reaktion: Wiedergabe fehlgeschlagen");
                "fehler"
            }
        };
        if FASSUNG.load(Ordering::SeqCst) == fassung {
            LAEUFT.store(false, Ordering::SeqCst);
            *ENDE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(grund);
        }
    });
}

/// Sofort still (Flugende, Ansagen aus, Stumm, Schalter aus).
pub(crate) fn aus() {
    if LAEUFT.swap(false, Ordering::SeqCst) {
        FASSUNG.fetch_add(1, Ordering::SeqCst);
        *ENDE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some("aus");
    }
}

/// Ende der letzten Reaktion, einmal: `gespielt`, `ansage`, `musik`, `aus`,
/// `fehler`.
pub(crate) fn ende_abholen() -> Option<&'static str> {
    ENDE.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
}

/// Was die Reaktion verdrängt: eine Ansage (auch kurz danach) oder die Musik.
pub(crate) fn verdraengt(jetzt_ms: u64) -> Option<&'static str> {
    if crate::kabinen_musik::ansage_haelt(jetzt_ms) {
        Some("ansage")
    } else if crate::kabinen_musik::flug_aktiv() && crate::kabinen_musik::hoerbar() {
        Some("musik")
    } else {
        None
    }
}

fn jetzt_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Ein Abtastschritt zum Ziel: aufwärts sofort (das Einblenden steckt in der
/// Aufnahme), abwärts über [`AUSBLENDEN_S`].
pub(crate) fn pegel_schritt(pegel: f32, ziel: f32, rate: f32) -> f32 {
    if ziel >= pegel {
        ziel
    } else {
        (pegel - 1.0 / (rate * AUSBLENDEN_S)).max(ziel)
    }
}

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod wiedergabe {
    use super::*;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    /// So viel liegt höchstens dekodiert bereit (Sekunden).
    const VORRAT_S: f32 = 1.0;

    pub(super) fn abspielen(datei: &Path, geraet: Option<&str>, fassung: u64) -> Result<&'static str, String> {
        let mut dek = Dekoder::oeffnen(datei)?;
        let host = cpal::default_host();
        let gewaehlt = geraet.and_then(|id| id.parse::<cpal::DeviceId>().ok()).and_then(|id| host.device_by_id(&id));
        let dev = gewaehlt.or_else(|| host.default_output_device()).ok_or("kein Ausgabegeraet")?;
        let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
        let kanaele = usize::from(cfg.channels()).max(1);
        let rate = cfg.sample_rate();
        let puffer = Arc::new(Mutex::new(VecDeque::<f32>::new()));
        let weg = Arc::new(AtomicBool::new(false));
        let still = Arc::new(AtomicBool::new(false));

        fn bauen<T: cpal::SizedSample + cpal::FromSample<f32>>(
            dev: &cpal::Device,
            cfg: cpal::StreamConfig,
            kanaele: usize,
            puffer: Arc<Mutex<VecDeque<f32>>>,
            weg: Arc<AtomicBool>,
            still: Arc<AtomicBool>,
        ) -> Result<cpal::Stream, String> {
            let rate = cfg.sample_rate.max(1) as f32;
            let mut pegel = crate::kabinenansagen::ausgabe::lautstaerke() * PEGEL;
            dev.build_output_stream(
                cfg,
                move |aus: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let ziel = if weg.load(Ordering::Relaxed) { 0.0 } else { crate::kabinenansagen::ausgabe::lautstaerke() * PEGEL };
                    // Nie blockierend warten (Echtzeit-Faden): belegt → Block still.
                    let Ok(mut q) = puffer.try_lock() else {
                        for k in aus.iter_mut() {
                            *k = T::from_sample(0.0);
                        }
                        return;
                    };
                    for rahmen in aus.chunks_mut(kanaele) {
                        pegel = pegel_schritt(pegel, ziel, rate);
                        for k in rahmen {
                            let x = q.pop_front().unwrap_or(0.0);
                            *k = T::from_sample((x * pegel).clamp(-1.0, 1.0));
                        }
                    }
                    still.store(pegel <= 0.0, Ordering::Relaxed);
                },
                move |e| tracing::warn!(error = %e, "kabinen-reaktion: Ausgabegeraet meldet einen Fehler"),
                None,
            )
            .map_err(|e| e.to_string())
        }

        let (p, w, s, c) = (Arc::clone(&puffer), Arc::clone(&weg), Arc::clone(&still), cfg.config());
        let strom = match cfg.sample_format() {
            cpal::SampleFormat::F32 => bauen::<f32>(&dev, c, kanaele, p, w, s),
            cpal::SampleFormat::I16 => bauen::<i16>(&dev, c, kanaele, p, w, s),
            cpal::SampleFormat::U16 => bauen::<u16>(&dev, c, kanaele, p, w, s),
            cpal::SampleFormat::I32 => bauen::<i32>(&dev, c, kanaele, p, w, s),
            andere => return Err(format!("Tonformat {andere:?} nicht unterstuetzt")),
        }?;
        strom.play().map_err(|e| e.to_string())?;

        let mut um = Umrechner::neu(rate, kanaele);
        let mut dekodiert = false;
        loop {
            if FASSUNG.load(Ordering::SeqCst) != fassung {
                return Ok("aus");
            }
            if let Some(grund) = verdraengt(jetzt_ms()) {
                weg.store(true, Ordering::Relaxed);
                let bis = std::time::Instant::now() + std::time::Duration::from_secs_f32(AUSBLENDEN_S + 0.5);
                while std::time::Instant::now() < bis && !still.load(Ordering::Relaxed) && FASSUNG.load(Ordering::SeqCst) == fassung {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                return Ok(grund);
            }
            let vorrat = puffer.lock().map(|q| q.len()).unwrap_or(0);
            if dekodiert {
                if vorrat == 0 {
                    return Ok("gespielt");
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
            if vorrat as f32 >= VORRAT_S * rate as f32 * kanaele as f32 {
                std::thread::sleep(std::time::Duration::from_millis(20));
                continue;
            }
            match dek.naechstes() {
                Some((r, k, proben)) => {
                    let mut neu = VecDeque::new();
                    um.umrechnen(r, k, &proben, &mut neu);
                    puffer.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(neu);
                }
                None => dekodiert = true,
            }
        }
    }
}

/// Linux: Der Client läuft dort nicht, nur der CI-Check.
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
mod wiedergabe {
    use super::*;
    pub(super) fn abspielen(_: &Path, _: Option<&str>, _: u64) -> Result<&'static str, String> {
        let _ = (Dekoder::oeffnen as fn(&Path) -> Result<Dekoder, String>, Umrechner::neu as fn(u32, usize) -> Umrechner, jetzt_ms);
        Err("nur unter Windows und macOS".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ausblenden_dauert_zwei_sekunden_einblenden_steckt_in_der_aufnahme() {
        let rate = 1000.0;
        // Aufwärts sofort.
        assert_eq!(pegel_schritt(0.0, 0.56, rate), 0.56);
        // Abwärts: von 1,0 bis 0 genau AUSBLENDEN_S.
        let mut p = 1.0;
        let mut schritte = 0;
        while p > 0.0 {
            p = pegel_schritt(p, 0.0, rate);
            schritte += 1;
        }
        assert_eq!(schritte, (rate * AUSBLENDEN_S) as usize);
    }

    #[test]
    fn aufnahmen_nach_art_ohne_verwechslung() {
        let d = std::env::temp_dir().join(format!("kabinen-reaktion-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for n in ["applaus-begeistert-2.mp3", "applaus-begeistert-4.mp3", "applaus-begeisterter-1.mp3", "raunen-hart-2.mp3", "applaus-begeistert-9.wav"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        let namen = |art| aufnahmen(&d, art).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert_eq!(namen("applaus-begeistert"), ["applaus-begeistert-2.mp3", "applaus-begeistert-4.mp3"]);
        assert_eq!(namen("raunen-hart"), ["raunen-hart-2.mp3"]);
        assert!(namen("murmeln-zufrieden").is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Die mitgelieferten Aufnahmen: jede Art hat welche, und jede lässt sich
    /// dekodieren.
    #[test]
    fn mitgelieferte_aufnahmen_vollstaendig_und_lesbar() {
        let ordner = Path::new(env!("CARGO_MANIFEST_DIR")).join(ORDNER);
        for art in ["applaus-ferienflieger", "applaus-begeistert", "applaus-hoeflich", "murmeln-zufrieden", "raunen-hart", "raunen-hopser"] {
            let v = aufnahmen(&ordner, art);
            assert!(!v.is_empty(), "keine Aufnahme für {art}");
            for p in v {
                let d = Dekoder::oeffnen(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                let s = d.dauer_s().unwrap_or(0.0);
                assert!((5.0..=20.0).contains(&s), "{}: {s} s", p.display());
            }
        }
    }
}
