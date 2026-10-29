//! Messung P-3.23 (Ladezeit beim Start <= 1 s). Nur auf Anforderung:
//! cargo test --release -p runtime-bundle --test ladezeit -- --ignored --nocapture

use std::path::Path;
use std::time::Instant;

use runtime_bundle::bauen::{bauen, Optionen};
use runtime_bundle::*;

#[test]
#[ignore]
fn ladezeit_mit_echten_ourairports() {
    let quelle = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../bundle");
    let ziel = tempfile::tempdir().unwrap();
    let jetzt = chrono::Utc::now();
    let o = Optionen {
        generation: 1,
        erstellt: jetzt,
        gueltig_tage: 180,
        client_min: "0.0.0".into(),
        client_max: "999.0.0".into(),
        requires: vec![],
        score_version: 27,
        input_schema: 1,
        quelle: Default::default(),
    };
    let m = bauen(&quelle, ziel.path(), &o).unwrap();
    let kp = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
    let sig = minisign::sign(Some(&kp.pk), &kp.sk, std::io::Cursor::new(&m), None, None).unwrap().into_string();
    let k = Kontext { client_version: "3.0.0".into(), faehigkeiten: vec![], schluessel: vec![kp.pk.to_base64()], jetzt };
    let ablage_dir = tempfile::tempdir().unwrap();
    let a = Ablage::oeffnen(ablage_dir.path()).unwrap();
    let blobs = ziel.path().join("blobs");
    a.installieren(Eingang { manifest: &m, signatur: &sig }, &k, &mut |d| std::fs::read(blobs.join(&d.sha256)).map_err(|e| e.to_string()), &|_| Ok(())).unwrap();
    let eingebaut = Eingebaut { manifest: m.clone(), dateien: Default::default() };
    let mut zeiten = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let l = a.laden(None, &k, &eingebaut).unwrap();
        for d in &l.paket.manifest.files {
            l.paket.datei(&d.path).unwrap();
        }
        zeiten.push(t.elapsed().as_millis());
    }
    zeiten.sort();
    println!("Laden + alle Hashes (+ einmal jede Datei lesen): Median {} ms, Werte {:?}", zeiten[2], zeiten);
    assert!(zeiten[2] < 1000, "P-3.23: Ladezeit ueber 1 s");
}
