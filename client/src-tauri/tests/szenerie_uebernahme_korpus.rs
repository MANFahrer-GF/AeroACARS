//! Was die Übernahme am **echten Bestand** bewirken würde.
//!
//! Läuft nicht im normalen Testlauf: Sie braucht die installierte
//! X-Plane-Szenerie und einen Auszug der Navdaten.
//!
//!     ssh live "sqlite3 <db> \"select airport_icao||'|'||designator||...\"" > /tmp/nav_voll.txt
//!     NAVDATEN=/tmp/nav_voll.txt cargo test -p aeroacars-app --release \
//!         --test szenerie_uebernahme_korpus -- --ignored --nocapture
//!
//! Der Lauf beantwortet die eine Frage, die vor dem Umschalten zählt:
//! **Was ändert sich, und ist die Änderung plausibel?**

use std::collections::HashMap;

#[test]
#[ignore = "braucht die installierte Szenerie und einen Navdaten-Auszug"]
fn was_die_uebernahme_am_bestand_bewirkt() {
    let Ok(pfad) = std::env::var("NAVDATEN") else {
        eprintln!("NAVDATEN=<datei> setzen");
        return;
    };
    let Ok(inhalt) = std::fs::read_to_string(&pfad) else {
        eprintln!("Auszug nicht lesbar: {pfad}");
        return;
    };
    let Some(wurzel) = sim_xplane::szenerie::installationen().into_iter().next() else {
        eprintln!("keine X-Plane-Installation");
        return;
    };
    let t = std::time::Instant::now();
    let idx = sim_xplane::szenerie::SzenerieIndex::bauen(&wurzel);
    eprintln!(
        "  Verzeichnis: {:?}, {} Flughäfen",
        t.elapsed(),
        idx.anzahl()
    );

    // Navdaten je Flughafen sammeln.
    let mut je_platz: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    for z in inhalt.lines() {
        let t: Vec<String> = z.split('|').map(|s| s.to_string()).collect();
        if t.len() < 10 {
            continue;
        }
        je_platz.entry(t[0].clone()).or_default().push(t);
    }
    eprintln!("  Flughäfen im Bestand: {}", je_platz.len());

    let mut geprueft = 0usize;
    let mut uebernommen = 0usize;
    let mut verworfen = 0usize;
    let mut ohne_treffer = 0usize;
    let mut nicht_in_szenerie = 0usize;
    let mut kurs_ab_1 = 0usize;
    let mut kurs_ab_3 = 0usize;
    let mut breite_ab_5 = 0usize;
    let mut groesste: Vec<(String, f64)> = Vec::new();

    for (icao, bahnen) in &je_platz {
        let Some(sz) = idx.flughafen(icao) else {
            nicht_in_szenerie += bahnen.len();
            continue;
        };
        let nav = aeroacars_app_lib::szenerie_bahn::test_navairport(icao, bahnen);
        let (aus, b) = aeroacars_app_lib::szenerie_bahn::uebernimm_szenerie(
            &nav,
            &sz,
            aeroacars_app_lib::szenerie_bahn::Quelle::XPlaneDatei,
        );
        geprueft += nav.runways.len();
        uebernommen += b.uebernommen.len();
        verworfen += b.verworfen.len();
        ohne_treffer += b.ohne_treffer.len();

        for (vorher, nachher) in nav.runways.iter().zip(aus.runways.iter()) {
            if !b.uebernommen.contains(&vorher.designator) {
                continue;
            }
            let d = {
                let x =
                    ((vorher.true_course - nachher.true_course) % 360.0 + 540.0) % 360.0 - 180.0;
                x.abs()
            };
            if d >= 1.0 {
                kurs_ab_1 += 1;
            }
            if d >= 3.0 {
                kurs_ab_3 += 1;
                groesste.push((format!("{icao} {}", vorher.designator), d));
            }
            if let (Some(v), Some(n)) = (vorher.width_ft, nachher.width_ft) {
                if ((v - n).abs() as f64) * 0.3048 >= 5.0 {
                    breite_ab_5 += 1;
                }
            }
        }
    }

    groesste.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    eprintln!();
    eprintln!("  Bahnen in beiden Quellen:   {geprueft:>7}");
    eprintln!("    davon uebernommen:        {uebernommen:>7}");
    eprintln!("    Bezeichner passte, Lage nicht (verworfen): {verworfen:>7}");
    eprintln!("    Bezeichner gar nicht gefunden:             {ohne_treffer:>7}");
    eprintln!("  Bahnen ohne Szenerie:       {nicht_in_szenerie:>7}");
    eprintln!();
    eprintln!("  Kurskorrektur ab 1 Grad:    {kurs_ab_1:>7}");
    eprintln!("  Kurskorrektur ab 3 Grad:    {kurs_ab_3:>7}");
    eprintln!("  Breitenkorrektur ab 5 m:    {breite_ab_5:>7}");
    eprintln!();
    eprintln!("  Die zehn groessten Kurskorrekturen:");
    for (name, d) in groesste.iter().take(10) {
        eprintln!("    {name:12} {d:+7.2} Grad");
    }

    // Riegel: Die Uebernahme darf nicht die Mehrheit verwerfen — dann
    // stimmte die Zuordnung nicht.
    assert!(
        verworfen * 5 < uebernommen.max(1),
        "zu viele Verwerfungen: {verworfen} gegen {uebernommen} uebernommen"
    );
}
