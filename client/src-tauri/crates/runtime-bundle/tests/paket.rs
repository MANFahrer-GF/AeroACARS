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

#[test]
fn gebautes_paket_laesst_sich_signiert_installieren() {
    use runtime_bundle::bauen::{bauen, Optionen};
    let quelle = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(quelle.path().join("ourairports")).unwrap();
    std::fs::write(quelle.path().join("ourairports/airports.csv"), b"EDDF").unwrap();
    std::fs::write(quelle.path().join("texte.json"), b"{}").unwrap();
    std::fs::write(quelle.path().join(".DS_Store"), b"x").unwrap();
    let ziel = tempfile::tempdir().unwrap();
    let o = Optionen {
        generation: 9,
        erstellt: t0(),
        gueltig_tage: 180,
        client_min: "3.0.0".into(),
        client_max: "3.99.99".into(),
        requires: vec!["wasm-host-1".into()],
        score_version: 27,
        input_schema: 1,
        quelle: Default::default(),
    };
    let m = bauen(quelle.path(), ziel.path(), &o).unwrap();
    // Gleiche Quelle, gleiche Optionen: byte-gleiches Manifest.
    let ziel2 = tempfile::tempdir().unwrap();
    assert_eq!(m, bauen(quelle.path(), ziel2.path(), &o).unwrap());
    let manifest = Manifest::lesen(&m).unwrap();
    let pfade: Vec<_> = manifest.files.iter().map(|d| d.path.as_str()).collect();
    assert_eq!(pfade, ["ourairports/airports.csv", "texte.json"]);

    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let ablage = tempfile::tempdir().unwrap();
    let a = Ablage::oeffnen(ablage.path()).unwrap();
    let sig = s.signieren(&m);
    let blobs = ziel.path().join("blobs");
    a.installieren(
        Eingang { manifest: &m, signatur: &sig },
        &k,
        &mut |d: &Datei| std::fs::read(blobs.join(&d.sha256)).map_err(|e| e.to_string()),
        &ok,
    )
    .unwrap();
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.paket.datei("ourairports/airports.csv").unwrap(), b"EDDF");
}

// ---- Faelle aus der Codex-Pruefung vom 29.10.2026 ----

#[test]
fn pin_mit_gleicher_generation_aber_anderem_inhalt_gilt_nicht() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, i) = paket(5, &[("a", b"5")]);
    let echt = installieren(&a, &s, &k, &m, &i).unwrap();
    let falsch = RuntimeBundleId { generation: 5, manifest_sha256: "0".repeat(64) };
    assert_ne!(echt, falsch);
    let l = a.laden(Some(&falsch), &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::EingebautStattPin);
}

#[test]
fn aufraeumen_bricht_bei_unlesbarem_behaltenen_manifest_ab_und_loescht_nichts() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    for g in 1..=3u64 {
        let inhalt = format!("stand {g}");
        let (m, i) = paket(g, &[("a", inhalt.as_bytes())]);
        installieren(&a, &s, &k, &m, &i).unwrap();
    }
    std::fs::write(dir.path().join("gen/3/manifest.json"), b"{kaputt").unwrap();
    let blobs_vorher = std::fs::read_dir(dir.path().join("blobs")).unwrap().count();
    assert!(a.aufraeumen(&[], t0() + Duration::days(40)).is_err());
    assert!(dir.path().join("gen/1").exists(), "nichts darf geloescht sein");
    assert_eq!(std::fs::read_dir(dir.path().join("blobs")).unwrap().count(), blobs_vorher);
}

#[test]
fn kaputte_spaetere_datei_eines_mehrteiligen_pakets_faellt_zurueck() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"a5"), ("b", b"b5"), ("c", b"c5")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    let (m6, i6) = paket(6, &[("a", b"a6"), ("b", b"b6"), ("c", b"c6")]);
    installieren(&a, &s, &k, &m6, &i6).unwrap();
    // Nur die letzte Datei von Stand 6 ist kaputt.
    std::fs::write(dir.path().join("blobs").join(hex(&Sha256::digest(b"c6"))), b"xx").unwrap();
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Vorher);
    assert_eq!(l.paket.datei("a").unwrap(), b"a5");
}

#[test]
fn abgelehnte_installationen_lassen_den_ganzen_alten_zustand_stehen() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let mut k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    for g in [4u64, 5] {
        let inhalt = format!("{g}");
        let (m, i) = paket(g, &[("a", inhalt.as_bytes())]);
        installieren(&a, &s, &k, &m, &i).unwrap();
    }
    let zustand = |a: &Ablage| (a.aktuell(), a.vorher(), a.laden(None, &kontext(&[&s]), &eingebaut()).unwrap().paket.datei("a").unwrap());
    let vorher = zustand(&a);
    // Selbsttest scheitert.
    let (m6, i6) = paket(6, &[("a", b"6")]);
    let sig = s.signieren(&m6);
    let _ = a.installieren(Eingang { manifest: &m6, signatur: &sig }, &k, &mut holer(&i6), &|_| Err("nein".into()));
    assert_eq!(zustand(&a), vorher);
    // Abgelaufen.
    k.jetzt = t0() + Duration::days(400);
    let (m7, i7) = paket(7, &[("a", b"7")]);
    assert!(installieren(&a, &s, &k, &m7, &i7).is_err());
    assert_eq!(zustand(&a), vorher);
}

#[test]
fn plattformfallen_in_pfaden_sind_verboten() {
    for p in ["CON", "a/nul.txt", "com1", "LPT9.csv", "a./b", "x/y.", "ä.csv", "a b", "a?b"] {
        let (m, _) = paket(5, &[(p, b"x")]);
        assert!(Manifest::lesen(&m).is_err(), "Pfad {p:?} wurde angenommen");
    }
    let (m, _) = paket(5, &[("Texte/de.json", b"x"), ("texte/DE.json", b"y")]);
    assert!(Manifest::lesen(&m).is_err(), "Gross-/Kleinschreibung darf nicht unterscheiden");
    for p in ["ourairports/airports.csv", "module/bewertung.wasm", "texte/de-DE.json", "con_tabelle.json"] {
        let (m, _) = paket(5, &[(p, b"x")]);
        assert!(Manifest::lesen(&m).is_ok(), "Pfad {p:?} wurde abgelehnt");
    }
}

#[test]
fn vorabversion_liegt_vor_dem_release_und_grenzen_gelten_genau() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m, i) = paket(5, &[("a", b"x")]);
    let mut k = kontext(&[&s]);
    k.client_version = "3.0.0-beta.1".into();
    assert!(matches!(installieren(&a, &s, &k, &m, &i).unwrap_err(), Fehler::Unpassend(_)));
    for v in ["3.0.0", "3.99.99"] {
        let dir = tempfile::tempdir().unwrap();
        let a = Ablage::oeffnen(dir.path()).unwrap();
        let mut k = kontext(&[&s]);
        k.client_version = v.into();
        installieren(&a, &s, &k, &m, &i).unwrap();
    }
    let mut k = kontext(&[&s]);
    k.client_version = "drei".into();
    assert!(installieren(&a, &s, &k, &m, &i).is_err());
}

#[test]
fn umgekehrter_versionsbereich_ist_ungueltig() {
    let (m, _) = paket(5, &[("a", b"x")]);
    let m = manifest_aendern(&m, |v| v["client_min"] = "4.0.0".into());
    assert!(matches!(Manifest::lesen(&m).unwrap_err(), Fehler::Manifest(_)));
}

#[test]
fn kaputter_zeiger_sperrt_installation_und_laden_faellt_zurueck() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"5")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    std::fs::write(dir.path().join("zeiger/stand.json"), b"{halb").unwrap();
    // Eine alte Generation darf nicht durchrutschen, nur weil der Zeiger kaputt ist.
    let (m3, i3) = paket(3, &[("a", b"3")]);
    assert!(installieren(&a, &s, &k, &m3, &i3).is_err());
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Eingebaut);
    assert!(l.hinweise.iter().any(|h| h.contains("Zeiger")));
}

#[test]
fn vorhandener_generationsordner_mit_anderem_inhalt_wird_nicht_geloescht() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"5")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    // Ordner gen/6 existiert schon (z. B. gepinnt), Zeiger steht noch auf 5.
    std::fs::create_dir_all(dir.path().join("gen/6")).unwrap();
    std::fs::write(dir.path().join("gen/6/manifest.json"), b"anderer stand").unwrap();
    let (m6, i6) = paket(6, &[("a", b"6")]);
    assert!(installieren(&a, &s, &k, &m6, &i6).is_err());
    assert_eq!(std::fs::read(dir.path().join("gen/6/manifest.json")).unwrap(), b"anderer stand");
    assert_eq!(a.aktuell(), Some(5));
}

#[test]
fn gleichzeitige_installationen_enden_nie_auf_einer_kleineren_generation() {
    let dir = tempfile::tempdir().unwrap();
    let s = std::sync::Arc::new(Schluessel::neu());
    let mut faeden = Vec::new();
    for g in 1..=8u64 {
        let pfad = dir.path().to_path_buf();
        let s = s.clone();
        faeden.push(std::thread::spawn(move || {
            let a = Ablage::oeffnen(&pfad).unwrap();
            let k = kontext(&[&s]);
            let inhalt = format!("stand {g}");
            let (m, i) = paket(g, &[("a", inhalt.as_bytes())]);
            let _ = installieren(&a, &s, &k, &m, &i);
        }));
    }
    for f in faeden {
        f.join().unwrap();
    }
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let k = kontext(&[&s]);
    let aktuell = a.aktuell().unwrap();
    let l = a.laden(None, &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Aktuell);
    assert_eq!(l.paket.datei("a").unwrap(), format!("stand {aktuell}").as_bytes());
    // Generation 8 wird immer angenommen (nichts ist groesser), und danach
    // muss jede kleinere abgelehnt werden. Ohne Sperre konnte eine 7, die den
    // alten Zeiger vor der 8 gelesen hatte, die 8 ueberschreiben.
    assert_eq!(aktuell, 8);
}

#[test]
fn fehlende_nennt_nur_was_lokal_fehlt_oder_kaputt_ist() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"bleibt"), ("b", b"alt")]);
    installieren(&a, &s, &k, &m5, &i5).unwrap();
    let (m6, _) = paket(6, &[("a", b"bleibt"), ("b", b"neu"), ("c", b"dazu")]);
    let fehlt: Vec<_> = a.fehlende(&Manifest::lesen(&m6).unwrap()).into_iter().map(|d| d.path).collect();
    assert_eq!(fehlt, ["b", "c"]);
    std::fs::write(dir.path().join("blobs").join(hex(&Sha256::digest(b"bleibt"))), b"kaputt").unwrap();
    assert_eq!(a.fehlende(&Manifest::lesen(&m6).unwrap()).len(), 3);
}

#[test]
fn fehlender_stand_eines_offenen_flugs_wird_genau_nachgeholt() {
    let dir = tempfile::tempdir().unwrap();
    let s = Schluessel::neu();
    let mut k = kontext(&[&s]);
    let a = Ablage::oeffnen(dir.path()).unwrap();
    let (m5, i5) = paket(5, &[("a", b"fuenf")]);
    let pin = installieren(&a, &s, &k, &m5, &i5).unwrap();
    let (m6, i6) = paket(6, &[("a", b"sechs")]);
    installieren(&a, &s, &k, &m6, &i6).unwrap();
    std::fs::remove_dir_all(dir.path().join("gen/5")).unwrap();
    assert_eq!(a.laden(Some(&pin), &k, &eingebaut()).unwrap().herkunft, Herkunft::EingebautStattPin);
    // Ein Jahr spaeter, Stand 5 laengst abgelaufen: Nachholen geht trotzdem.
    k.jetzt = t0() + Duration::days(365);
    let sig = s.signieren(&m5);
    // Falsches Manifest (anderer Stand) wird abgelehnt.
    let sig6 = s.signieren(&m6);
    assert!(a.nachinstallieren(Eingang { manifest: &m6, signatur: &sig6 }, &pin, &k, &mut holer(&i6)).is_err());
    let id = a.nachinstallieren(Eingang { manifest: &m5, signatur: &sig }, &pin, &k, &mut holer(&i5)).unwrap();
    assert_eq!(id, pin);
    let l = a.laden(Some(&pin), &k, &eingebaut()).unwrap();
    assert_eq!(l.herkunft, Herkunft::Pin);
    assert_eq!(l.paket.datei("a").unwrap(), b"fuenf");
    // Zeiger unberuehrt.
    assert_eq!((a.aktuell(), a.vorher()), (Some(6), Some(5)));
}

/// Beta 1 heisst `3.0.0-beta.N` (Tag `v3.0.0-beta.N`). Diese Vorabversion
/// muss in den Bereich des eingebauten Stands (0.0.0..=999.0.0,
/// `laufzeit_paket.rs`) UND in den der CI-Pakete (3.0.0-0..=3.99.99,
/// `paket-bauen`) fallen — sonst lehnt `passt()` den eingebauten Stand ab
/// und der Client kaeme nicht hoch (Advisor 29.10.). 2.x faellt nicht in
/// den CI-Bereich.
#[test]
fn vorabversion_3_0_liegt_in_beiden_bereichen() {
    use runtime_bundle::manifest::version;
    for v in ["3.0.0-beta.1", "3.0.0-beta.12", "3.0.0-rc.1", "3.0.0", "3.4.2"] {
        let v = version(v).unwrap();
        assert!(v >= version("0.0.0").unwrap() && v <= version("999.0.0").unwrap(), "{v} eingebaut");
        assert!(v >= version("3.0.0-0").unwrap() && v <= version("3.99.99").unwrap(), "{v} CI");
    }
    assert!(version("2.1.24").unwrap() < version("3.0.0-0").unwrap());
}
