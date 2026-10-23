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
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Zustand des Players unter EINER Sperre (Codex: getrennte Atomics ließen
/// einen alten Faden das Ende einer neuen Reaktion überschreiben).
struct Zustand {
    /// Erhöht sich mit jedem Auftrag und mit `aus()`; ein Faden mit älterer
    /// Fassung endet und meldet nichts mehr.
    fassung: u64,
    laeuft: bool,
    /// Warum die letzte Reaktion endete — holt der Kabinen-Takt fürs
    /// Protokoll ab.
    ende: Option<&'static str>,
}

static ZUSTAND: Mutex<Zustand> = Mutex::new(Zustand { fassung: 0, laeuft: false, ende: None });

fn zustand() -> std::sync::MutexGuard<'static, Zustand> {
    ZUSTAND.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Gilt der Auftrag `fassung` noch?
fn aktuell(fassung: u64) -> bool {
    zustand().fassung == fassung
}

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
    let fassung = {
        let mut z = zustand();
        z.fassung += 1;
        z.laeuft = true;
        // Ein nicht abgeholtes Ende gehört zur vorigen Reaktion.
        z.ende = None;
        z.fassung
    };
    let _ = std::thread::Builder::new().name("kabinen-reaktion".into()).spawn(move || {
        let grund = match wiedergabe::abspielen(&datei, geraet.as_deref(), fassung) {
            Ok(g) => g,
            Err(e) => {
                tracing::warn!(datei = %datei.display(), error = %e, "kabinen-reaktion: Wiedergabe fehlgeschlagen");
                "fehler"
            }
        };
        faden_ende(fassung, grund);
    });
}

/// Ende eines Wiedergabefadens: zählt nur, wenn sein Auftrag noch gilt.
fn faden_ende(fassung: u64, grund: &'static str) {
    let mut z = zustand();
    if z.fassung == fassung {
        z.laeuft = false;
        z.ende = Some(grund);
    }
}

/// Sofort still (Flugende, Ansagen aus, Stumm, Schalter aus).
pub(crate) fn aus() {
    let mut z = zustand();
    if z.laeuft {
        z.fassung += 1;
        z.laeuft = false;
        z.ende = Some("aus");
    }
}

/// Ende der letzten Reaktion, einmal: `gespielt`, `ansage`, `musik`, `aus`,
/// `fehler`.
pub(crate) fn ende_abholen() -> Option<&'static str> {
    zustand().ende.take()
}

/// Was die Reaktion verdrängt: eine Ansage (auch kurz danach) oder die Musik.
/// Schon ein frischer Musik-Auftrag zählt, nicht erst hörbare Musik — sonst
/// liefen beide kurz zusammen, wenn die Musik im selben Takt startet; nach dem
/// Anlauf nur noch hörbare Musik (Codex).
pub(crate) fn verdraengt(jetzt_ms: u64) -> Option<&'static str> {
    if crate::kabinen_musik::ansage_haelt(jetzt_ms) {
        Some("ansage")
    } else if crate::kabinen_musik::flug_hoerbar_oder_anlauf(jetzt_ms) {
        Some("musik")
    } else {
        None
    }
}

fn jetzt_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Ein Abtastschritt: ohne `weg` der eingestellte Pegel `voll` (das
/// Einblenden steckt in der Aufnahme); mit `weg` von dem Pegel, bei dem das
/// Ausblenden begann (`ab`, 0 = noch nicht begonnen), linear in
/// [`AUSBLENDEN_S`] auf 0 — auch wenn währenddessen die Lautstärke
/// verstellt wird (Codex).
pub(crate) fn pegel_schritt(pegel: f32, weg: bool, voll: f32, ab: &mut f32, rate: f32) -> f32 {
    if !weg {
        *ab = 0.0;
        return voll;
    }
    if *ab <= 0.0 {
        *ab = pegel.max(0.001);
    }
    (pegel - *ab / (rate * AUSBLENDEN_S)).max(0.0)
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
            let mut ab = 0.0f32;
            dev.build_output_stream(
                cfg,
                move |aus: &mut [T], _: &cpal::OutputCallbackInfo| {
                    let voll = crate::kabinenansagen::ausgabe::lautstaerke() * PEGEL;
                    let ist_weg = weg.load(Ordering::Relaxed);
                    // Nie blockierend warten (Echtzeit-Faden): belegt → Block still.
                    let Ok(mut q) = puffer.try_lock() else {
                        for k in aus.iter_mut() {
                            *k = T::from_sample(0.0);
                        }
                        return;
                    };
                    for rahmen in aus.chunks_mut(kanaele) {
                        pegel = pegel_schritt(pegel, ist_weg, voll, &mut ab, rate);
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
            if !aktuell(fassung) {
                return Ok("aus");
            }
            if let Some(grund) = verdraengt(jetzt_ms()) {
                weg.store(true, Ordering::Relaxed);
                let bis = std::time::Instant::now() + std::time::Duration::from_secs_f32(AUSBLENDEN_S + 0.5);
                while std::time::Instant::now() < bis && !still.load(Ordering::Relaxed) && aktuell(fassung) {
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
        let mut ab = 0.0;
        // Ohne Ausblenden sofort der eingestellte Pegel.
        assert_eq!(pegel_schritt(0.0, false, 0.56, &mut ab, rate), 0.56);
        let soll = (rate * AUSBLENDEN_S) as i32;
        // Abwärts: vom Pegel bei Beginn bis 0 genau AUSBLENDEN_S — auch leise
        // (der echte Test fand 1,15 s bei 0,56, als der Schritt auf 1,0
        // gerechnet war) und auch, wenn die Lautstärke währenddessen auf 0
        // gedreht wird (Codex: sonst ~112 s).
        for (voll, danach) in [(1.0f32, 1.0f32), (0.56, 0.56), (0.1, 0.1), (0.56, 0.0)] {
            let (mut p, mut ab, mut schritte) = (voll, 0.0, 0);
            while p > 0.0 {
                p = pegel_schritt(p, true, if schritte == 0 { voll } else { danach }, &mut ab, rate);
                schritte += 1;
                assert!(schritte < soll * 2, "{voll}→{danach}: läuft zu lange");
            }
            assert!((schritte - soll).abs() <= 1, "{voll}→{danach}: {schritte} statt {soll}");
        }
        // Zurück auf „nicht weg“: neuer Beginn beim nächsten Mal.
        assert_eq!(pegel_schritt(0.3, false, 0.56, &mut ab, rate), 0.56);
        assert_eq!(ab, 0.0);
    }

    /// Codex: Ein alter Faden darf weder das Ende noch den Lauf-Zustand einer
    /// neuen Reaktion verändern; `aus()` stoppt die neue sicher.
    #[test]
    fn alter_faden_ueberschreibt_die_neue_reaktion_nicht() {
        let alt = {
            let mut z = zustand();
            z.fassung += 1;
            z.laeuft = true;
            z.fassung
        };
        // Neue Reaktion (wie `spielen`, ohne Faden).
        let neu = {
            let mut z = zustand();
            z.fassung += 1;
            z.laeuft = true;
            z.ende = None;
            z.fassung
        };
        faden_ende(alt, "gespielt");
        assert!(zustand().laeuft, "neue Reaktion gilt weiter als laufend");
        assert_eq!(ende_abholen(), None, "altes Ende nicht der neuen zugeordnet");
        aus();
        assert!(!aktuell(neu), "aus() beendet die neue Fassung");
        assert_eq!(ende_abholen(), Some("aus"));
        faden_ende(neu, "gespielt");
        assert_eq!(ende_abholen(), None, "Faden nach aus() meldet nichts mehr");
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

    /// Echt auf dem Ausgabegerät (nur von Hand, Windows/macOS):
    /// `cargo test -p aeroacars-app echte_reaktion -- --ignored --nocapture`
    /// Spielt eine Aufnahme ganz, dann eine zweite, in die nach 2 s eine
    /// Ansage fällt — die muss ausblenden und mit „ansage“ enden.
    #[test]
    #[ignore]
    fn echte_reaktion_spielen_und_der_ansage_weichen() {
        let ordner = Path::new(env!("CARGO_MANIFEST_DIR")).join(ORDNER);
        let kurz = aufnahmen(&ordner, "raunen-hart").remove(0);
        let lang = aufnahmen(&ordner, "applaus-begeistert").remove(0);
        let warte_ende = |max_s: f32| {
            let t = std::time::Instant::now();
            loop {
                if let Some(g) = ende_abholen() {
                    return (g, t.elapsed().as_secs_f32());
                }
                assert!(t.elapsed().as_secs_f32() < max_s, "kein Ende nach {max_s} s");
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        };
        let dauer = Dekoder::oeffnen(&kurz).unwrap().dauer_s().unwrap() as f32;
        spielen(kurz, None);
        let (g, s) = warte_ende(dauer + 3.0);
        println!("ganz: {g} nach {s:.2} s (Aufnahme {dauer:.2} s)");
        assert_eq!(g, "gespielt");
        assert!((s - dauer).abs() < 1.5, "{s} vs {dauer}");

        spielen(lang, None);
        std::thread::sleep(std::time::Duration::from_secs(2));
        let _ansage = crate::kabinen_musik::AnsageLaeuft::neu();
        let (g, s) = warte_ende(5.0);
        println!("Ansage: {g} nach {s:.2} s");
        assert_eq!(g, "ansage");
        assert!((1.5..=3.0).contains(&s), "Ausblenden {s} s");
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
