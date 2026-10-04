//! Tests fuer den Bandversand der Sitzung (ADR-0005): nur mit Plugin >= 1.1.0,
//! 1x/s und sofort bei Lagewechsel, `seq` streng aufsteigend, bei
//! ausgeschalteter Einstellung genau einmal `BAND <seq> 0 0`.

use super::*;
use crate::hud_band::{BandFrame, BandSlot, Lage, Zeile};
use parking_lot::Mutex;

struct BandZiel {
    slot: Arc<BandSlot>,
    ereignisse: Mutex<Vec<Ereignis>>,
}

impl BandZiel {
    fn neu() -> Self {
        Self {
            slot: Arc::new(BandSlot::default()),
            ereignisse: Mutex::new(Vec::new()),
        }
    }
}

impl Ziel for BandZiel {
    fn wunsch_generation(&self) -> u64 {
        0
    }
    fn wuensche(&self) -> Vec<AboWunsch> {
        Vec::new()
    }
    fn ereignis(&self, e: Ereignis) {
        self.ereignisse.lock().push(e);
    }
    fn band(&self) -> Option<(u64, Arc<BandFrame>)> {
        self.slot.holen()
    }
    fn band_wunsch(&self) -> bool {
        self.slot.wunsch()
    }
    fn band_bereit(&self, b: bool) {
        self.slot.set_bereit(b);
    }
}

fn frame(lage: Lage, ruhig: bool) -> BandFrame {
    BandFrame {
        lage,
        ruhig,
        zeilen: vec![Zeile::default()],
    }
}

fn hallo(v: &str) -> Antwort {
    Antwort::Hallo {
        plugin: v.into(),
        xplane: Some(12100),
        xplm: Some(430),
    }
}

/// Ein Takt zur Zeit `t`; vorher ein Lebenszeichen, damit die 3-s-Stille nicht
/// zuschlaegt. Liefert die BAND-Datagramme.
fn schritt(s: &mut Sitzung, z: &BandZiel, t: Instant) -> Vec<String> {
    s.empfangen(Antwort::Sonstige("pong".into()), t, z);
    s.takt(t, z)
        .into_iter()
        .map(|d| String::from_utf8(d).unwrap())
        .filter(|d| d.starts_with("BAND"))
        .collect()
}

fn offene_sitzung(version: &str, z: &BandZiel, t0: Instant) -> Sitzung {
    let mut s = Sitzung::neu("1.9.19");
    s.band_seq_setzen(100);
    s.empfangen(hallo(version), t0, z);
    assert!(s.offen());
    s
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

#[test]
fn altes_plugin_bekommt_nie_ein_band() {
    let z = BandZiel::neu();
    z.slot.setze(frame(Lage::Bereit, true));
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.0.0", &z, t0);
    for i in 0..30 {
        assert!(schritt(&mut s, &z, t0 + ms(i * 200)).is_empty());
    }
    assert!(!z.slot.bereit(), "App soll gar nicht erst bauen");
}

#[test]
fn band_einmal_pro_sekunde_und_sofort_bei_lagewechsel() {
    let z = BandZiel::neu();
    z.slot.setze(frame(Lage::Unterwegs, true));
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.1.0", &z, t0);
    // Erstes Band sofort; Zeichen: seq 100, ruhig 1, eine leere Zeile.
    let d = schritt(&mut s, &z, t0 + ms(100));
    assert_eq!(d, vec!["BAND 100 1 1\n\n".to_string()]);
    assert!(z.slot.bereit());
    // Neue Version, gleiche Lage, unter 1 s: nichts.
    z.slot.setze(frame(Lage::Unterwegs, true));
    assert!(schritt(&mut s, &z, t0 + ms(600)).is_empty());
    // Nach 1 s geht es hinaus.
    let d = schritt(&mut s, &z, t0 + ms(1200));
    assert_eq!(d, vec!["BAND 101 1 1\n\n".to_string()]);
    // Lagewechsel: sofort, auch unter 1 s; ruhig 0.
    z.slot.setze(frame(Lage::Anflug, false));
    let d = schritt(&mut s, &z, t0 + ms(1400));
    assert_eq!(d, vec!["BAND 102 0 1\n\n".to_string()]);
    // Dieselbe Version wird nie zweimal gesendet.
    assert!(schritt(&mut s, &z, t0 + ms(5000)).is_empty());
}

#[test]
fn einstellung_aus_sendet_einmal_null_null_und_seq_steigt_weiter() {
    let z = BandZiel::neu();
    z.slot.setze(frame(Lage::Bereit, true));
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.1.0", &z, t0);
    assert_eq!(schritt(&mut s, &z, t0 + ms(100)), vec!["BAND 100 1 1\n\n"]);
    // Ausschalten: genau einmal BAND 101 0 0, danach nichts mehr.
    z.slot.set_wunsch(false);
    assert_eq!(schritt(&mut s, &z, t0 + ms(300)), vec!["BAND 101 0 0\n"]);
    z.slot.setze(frame(Lage::Anflug, false));
    for i in 0..20 {
        assert!(
            schritt(&mut s, &z, t0 + ms(400 + i * 300)).is_empty(),
            "bei ausgeschalteter Einstellung kein Versand"
        );
    }
    assert!(!z.slot.bereit());
    // Wieder an: das (neue) Band geht sofort hinaus, seq geht weiter hoch.
    z.slot.set_wunsch(true);
    z.slot.setze(frame(Lage::Anflug, false));
    assert_eq!(schritt(&mut s, &z, t0 + ms(7000)), vec!["BAND 102 0 1\n\n"]);
}

#[test]
fn einstellung_von_anfang_an_aus_sendet_gar_nichts() {
    let z = BandZiel::neu();
    z.slot.set_wunsch(false);
    z.slot.setze(frame(Lage::Bereit, true));
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.1.0", &z, t0);
    for i in 0..10 {
        assert!(schritt(&mut s, &z, t0 + ms(i * 300)).is_empty());
    }
    assert!(s.beenden().iter().all(|d| !d.starts_with(b"BAND")));
}

#[test]
fn sauberes_abmelden_blendet_das_band_aus() {
    let z = BandZiel::neu();
    z.slot.setze(frame(Lage::Bereit, true));
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.1.0", &z, t0);
    assert_eq!(schritt(&mut s, &z, t0 + ms(100)).len(), 1);
    let ende: Vec<String> = s
        .beenden()
        .into_iter()
        .map(|d| String::from_utf8(d).unwrap())
        .collect();
    assert_eq!(ende, vec!["BAND 101 0 0\n"]);
}

#[test]
fn nach_ausschalten_meldet_abmelden_nicht_noch_einmal() {
    let z = BandZiel::neu();
    z.slot.setze(frame(Lage::Bereit, true));
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.1.0", &z, t0);
    schritt(&mut s, &z, t0 + ms(100));
    z.slot.set_wunsch(false);
    assert_eq!(schritt(&mut s, &z, t0 + ms(300)).len(), 1);
    assert!(s.beenden().iter().all(|d| !d.starts_with(b"BAND")));
}

#[test]
fn band_ungueltig_geht_ins_log_und_nicht_an_das_ziel() {
    let z = BandZiel::neu();
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.1.0", &z, t0);
    let vorher = z.ereignisse.lock().len();
    s.empfangen(
        Antwort::Fehler {
            grund: "band_ungueltig".into(),
            abo: None,
            id: None,
            gen: None,
        },
        t0 + ms(10),
        &z,
    );
    assert_eq!(z.ereignisse.lock().len(), vorher, "kein Nutzerfehler");
    assert!(s.offen(), "Sitzung bleibt");
    // Gegenprobe: ein anderer Fehler wird weitergereicht.
    s.empfangen(
        Antwort::Fehler {
            grund: "rate_ungueltig".into(),
            abo: Some(1),
            id: None,
            gen: None,
        },
        t0 + ms(20),
        &z,
    );
    assert_eq!(z.ereignisse.lock().len(), vorher + 1);
}

#[test]
fn neue_sitzung_zaehlt_seq_weiter() {
    let z = BandZiel::neu();
    z.slot.setze(frame(Lage::Bereit, true));
    let t0 = Instant::now();
    let mut s = offene_sitzung("1.1.0", &z, t0);
    assert_eq!(schritt(&mut s, &z, t0 + ms(100)), vec!["BAND 100 1 1\n\n"]);
    // kein_hallo: Sitzung zu, neues HALLO, Band geht weiter mit hoeherer seq.
    s.empfangen(
        Antwort::Fehler {
            grund: "kein_hallo".into(),
            abo: None,
            id: None,
            gen: None,
        },
        t0 + ms(200),
        &z,
    );
    assert!(!s.offen());
    s.empfangen(hallo("1.1.0"), t0 + ms(300), &z);
    z.slot.setze(frame(Lage::Bereit, true));
    assert_eq!(schritt(&mut s, &z, t0 + ms(500)), vec!["BAND 101 1 1\n\n"]);
}

#[test]
fn kann_band_ab_1_1_0() {
    assert!(!kann_band("1.0.0"));
    assert!(!kann_band("1.0.9"));
    assert!(kann_band("1.1.0"));
    assert!(kann_band("1.2.0-beta"));
    assert!(kann_band("2.0.0"));
    assert!(!kann_band("quatsch"));
}
