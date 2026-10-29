//! Faelle aus Pflichtenheft P-13.10 / Gesamtkonzept Kap. 8.

use std::collections::HashMap;
use std::io::Cursor;

use chrono::{DateTime, Duration, TimeZone, Utc};
use runtime_bundle::*;
use sha2::{Digest, Sha256};

struct Schluessel {
    kp: minisign::KeyPair,
}

impl Schluessel {
    fn neu() -> Self {
        Self { kp: minisign::KeyPair::generate_unencrypted_keypair().unwrap() }
    }
    fn oeffentlich(&self) -> String {
        self.kp.pk.to_base64()
    }
    fn signieren(&self, daten: &[u8]) -> String {
        minisign::sign(Some(&self.kp.pk), &self.kp.sk, Cursor::new(daten), Some("aeroacars"), None)
            .unwrap()
            .into_string()
    }
}

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 1, 12, 0, 0).unwrap()
}

fn kontext(schluessel: &[&Schluessel]) -> Kontext {
    Kontext {
        client_version: "3.0.0".into(),
        faehigkeiten: vec!["wasm-host-1".into()],
        schluessel: schluessel.iter().map(|s| s.oeffentlich()).collect(),
        jetzt: t0(),
    }
}

/// Baut ein Paket: Manifest-Bytes und Dateien.
fn paket(generation: u64, dateien: &[(&str, &[u8])]) -> (Vec<u8>, HashMap<String, Vec<u8>>) {
    let files: Vec<_> = dateien
        .iter()
        .map(|(p, b)| {
            serde_json::json!({
                "path": p, "sha256": hex(&Sha256::digest(b)), "size": b.len(), "kind": "daten"
            })
        })
        .collect();
    let m = serde_json::json!({
        "bundle_schema": 1, "generation": generation,
        "created_at": t0(), "expires_at": t0() + Duration::days(180),
        "client_min": "3.0.0", "client_max": "3.99.99",
        "requires": ["wasm-host-1"], "score_version": 27, "input_schema": 1,
        "files": files,
    });
    let bytes = serde_json::to_vec_pretty(&m).unwrap();
    let inhalt = dateien.iter().map(|(p, b)| (p.to_string(), b.to_vec())).collect();
    (bytes, inhalt)
}

fn manifest_aendern(bytes: &[u8], f: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut v: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    f(&mut v);
    serde_json::to_vec_pretty(&v).unwrap()
}

fn holer(inhalt: &HashMap<String, Vec<u8>>) -> impl FnMut(&Datei) -> Result<Vec<u8>, String> + '_ {
    move |d: &Datei| inhalt.get(&d.path).cloned().ok_or_else(|| "fehlt".to_string())
}

fn ok(_: &Paket) -> Result<(), String> {
    Ok(())
}

fn eingebaut() -> Eingebaut {
    let (m, inhalt) = paket(1, &[("ourairports/airports.csv", b"eingebaut")]);
    Eingebaut { manifest: m, dateien: inhalt }
}

fn installieren(
    a: &Ablage,
    s: &Schluessel,
    k: &Kontext,
    m: &[u8],
    inhalt: &HashMap<String, Vec<u8>>,
) -> Result<RuntimeBundleId, Fehler> {
    let sig = s.signieren(m);
    a.installieren(Eingang { manifest: m, signatur: &sig }, k, &mut holer(inhalt), &ok)
}

#[test]
fn installieren_und_laden() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, inhalt) = paket(5, &[("ourairports/airports.csv", b"EDDF")]);
    let id = installieren(&a, &s, &k, &m, &inhalt).unwrap();
    assert_eq!(id.generation, 5);
    assert_eq!(a.aktuell(), Some(5));
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Aktuell);
    assert_eq!(l.paket.id, id);
    assert_eq!(l.paket.datei("ourairports/airports.csv").unwrap(), b"EDDF");
}

#[test]
fn falsche_signatur_wird_abgelehnt_und_nichts_aendert_sich() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let fremd = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, inhalt) = paket(5, &[("a", b"x")]);
    let err = installieren(&a, &fremd, &k, &m, &inhalt).unwrap_err();
    assert!(matches!(err, Fehler::Signatur), "{err}");
    assert_eq!(a.aktuell(), None);
}

#[test]
fn manifest_nach_dem_signieren_veraendert_wird_abgelehnt() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, inhalt) = paket(5, &[("a", b"x")]);
    let sig = s.signieren(&m);
    let boese = manifest_aendern(&m, |v| v["generation"] = 6.into());
    let err = a
        .installieren(Eingang { manifest: &boese, signatur: &sig }, &k, &mut holer(&inhalt), &ok)
        .unwrap_err();
    assert!(matches!(err, Fehler::Signatur), "{err}");
}

#[test]
fn reserveschluessel_wird_angenommen() {
    let dir = tempfile::tempdir().unwrap();
    let aktiv = Schluessel::neu();
    let reserve = Schluessel::neu();
    let k = kontext(&[&aktiv, &reserve]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, inhalt) = paket(5, &[("a", b"x")]);
    installieren(&a, &reserve, &k, &m, &inhalt).unwrap();
    assert_eq!(a.aktuell(), Some(5));
}

#[test]
fn manipulierte_datei_wird_abgelehnt() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, mut inhalt) = paket(5, &[("a", b"echt")]);
    inhalt.insert("a".into(), b"boes".to_vec());
    let err = installieren(&a, &s, &k, &m, &inhalt).unwrap_err();
    assert!(matches!(err, Fehler::Datei { .. }), "{err}");
    assert_eq!(a.aktuell(), None);
}

#[test]
fn aeltere_oder_gleiche_generation_wird_abgelehnt() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"5")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    for g in [4, 5] {
        let (m, i) = paket(g, &[("a", b"alt")]);
        let err = installieren(&a, &s, &k, &m, &i).unwrap_err();
        assert!(matches!(err, Fehler::Veraltet { .. }), "{err}");
    }
    assert_eq!(a.aktuell(), Some(5));
}

#[test]
fn abgelaufener_download_wird_nicht_aktiviert_aktiver_laeuft_weiter() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let mut k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"5")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    // Ein Jahr spaeter: der aktive Stand ist abgelaufen und laedt trotzdem.
    k.jetzt = t0() + Duration::days(365);
    assert_eq!(a.laden(None, &k, &eingebaut()).unwrap().herkunft, Herkunft::Aktuell);
    // Ein neuer, aber abgelaufener Download wird nicht aktiviert.
    let (m6, i6) = paket(6, &[("a", b"6")]);
    let err = installieren(&a, &s, &k, &m6, &i6).unwrap_err();
    assert!(matches!(err, Fehler::Abgelaufen(_)), "{err}");
}

#[test]
fn fehlende_faehigkeit_und_falsche_clientversion() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, i) = paket(5, &[("a", b"x")]);
    let mut k = kontext(&[&s]);
    k.faehigkeiten.clear();
    assert!(matches!(installieren(&a, &s, &k, &m, &i).unwrap_err(), Fehler::Unpassend(_)));
    let mut k = kontext(&[&s]);
    k.client_version = "2.1.24".into();
    assert!(matches!(installieren(&a, &s, &k, &m, &i).unwrap_err(), Fehler::Unpassend(_)));
    assert_eq!(a.aktuell(), None);
}

#[test]
fn pfade_aus_dem_paketordner_heraus_sind_verboten() {
    for p in ["../x", "/etc/passwd", "a\\b", "a//b", "C:/x", "./a", ""] {
        let (m, _) = paket(5, &[(p, b"x")]);
        assert!(Manifest::lesen(&m).is_err(), "Pfad {p:?} wurde angenommen");
    }
}

#[test]
fn selbsttest_fehler_aktiviert_nichts() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"5")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    let (m6, i6) = paket(6, &[("a", b"6")]);
    let sig = s.signieren(&m6);
    let err = a
        .installieren(Eingang { manifest: &m6, signatur: &sig }, &k, &mut holer(&i6), &|_| {
            Err("Note weicht ab".into())
        })
        .unwrap_err();
    assert!(matches!(err, Fehler::Selbsttest(_)), "{err}");
    assert_eq!(a.aktuell(), Some(5));
    assert!(!dir.path().join("gen/6").exists());
}

#[test]
fn kaputte_datei_auf_der_platte_faellt_auf_vorher_zurueck() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"fuenf")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    let (m6, i6) = paket(6, &[("a", b"sechs")]);
    installieren(&a, &s, &k, &m6, &i6).unwrap();
    assert_eq!(a.vorher(), Some(5));
    let h6 = hex(&Sha256::digest(b"sechs"));
    std::fs::write(dir.path().join("blobs").join(h6), b"kaputt").unwrap();
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Vorher);
    assert_eq!(l.paket.datei("a").unwrap(), b"fuenf");
    assert_eq!(l.hinweise.len(), 1);
}

#[test]
fn ohne_ablage_gilt_der_eingebaute_stand() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Eingebaut);
    assert_eq!(l.paket.datei("ourairports/airports.csv").unwrap(), b"eingebaut");
}

#[test]
fn offener_flug_laedt_genau_seinen_stand() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"5")]);
    let pin = installieren(&a, &s, &k, &m5, &i5).unwrap();
    let (m6, i6) = paket(6, &[("a", b"6")]);
    installieren(&a, &s, &k, &m6, &i6).unwrap();
    let l = a.laden(Some(&pin), &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Pin);
    assert_eq!(l.paket.datei("a").unwrap(), b"5");
    // Stand des Flugs fehlt: eingebaut, als solcher erkennbar.
    std::fs::remove_dir_all(dir.path().join("gen/5")).unwrap();
    let l = a.laden(Some(&pin), &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::EingebautStattPin);
}

#[test]
fn nur_geaenderte_dateien_werden_geholt() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("gross", b"7,5 MB OurAirports"), ("profil", b"v1")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    let (m6, i6) = paket(6, &[("gross", b"7,5 MB OurAirports"), ("profil", b"v2")]);
    let sig = s.signieren(&m6);
    let mut geholt = Vec::new();
    a.installieren(
        Eingang { manifest: &m6, signatur: &sig },
        &k,
        &mut |d: &Datei| {
            geholt.push(d.path.clone());
            Ok(i6[&d.path].clone())
        },
        &ok,
    )
    .unwrap();
    assert_eq!(geholt, vec!["profil".to_string()]);
}

#[test]
fn aufraeumen_schont_pin_aktuell_vorher_und_junge_staende() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    for g in 1..=5u64 {
        let inhalt = format!("stand {g}");
        let (m, i) = paket(g, &[("a", inhalt.as_bytes())]);
        installieren(&a, &s, &k, &m, &i).unwrap();
    }
    // Heute: alles juenger als 30 Tage -> nichts weg.
    assert_eq!(a.aufraeumen(&[2], t0()).unwrap(), 0);
    // In 40 Tagen: 1 und 3 gehen, 2 (Pin), 4 (vorher), 5 (aktuell) bleiben.
    assert_eq!(a.aufraeumen(&[2], t0() + Duration::days(40)).unwrap(), 2);
    for (g, da) in [(1, false), (2, true), (3, false), (4, true), (5, true)] {
        assert_eq!(dir.path().join(format!("gen/{g}")).exists(), da, "gen {g}");
    }
    let blobs = std::fs::read_dir(dir.path().join("blobs")).unwrap().count();
    assert_eq!(blobs, 3, "Dateien von 1 und 3 muessen weg sein");
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.paket.datei("a").unwrap(), b"stand 5");
}

#[test]
fn liegengebliebener_zwischenstand_stoert_nicht() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    // Absturz waehrend einer frueheren Installation: halber Ordner im tmp.
    std::fs::create_dir_all(dir.path().join("tmp/gen-7")).unwrap();
    std::fs::write(dir.path().join("tmp/gen-7/manifest.json"), b"{halb").unwrap();
    let (m, i) = paket(7, &[("a", b"x")]);
    installieren(&a, &s, &k, &m, &i).unwrap();
    assert_eq!(a.laden(None, &k, &eingebaut()).unwrap().herkunft, Herkunft::Aktuell);
}
