//! Der Zwischenspeicher haengt am Betriebsweg — nicht nur im Modul.
//!
//! # Warum das eine QUELLTEXT-Pruefung ist
//!
//! Die Einheitstests in `navdata_cache.rs` pruefen das Altern und das
//! Ablegen. Sie bleiben alle gruen, wenn im Abrufweg NIEMAND ablegt oder
//! liest — dann ist der Zwischenspeicher gebaut, getestet und ohne
//! Wirkung. Genau die Luecke, an der schon der Spur-Nachtrag haengen
//! geblieben ist.
//!
//! Diese Pruefung schliesst sie: Das Ablegen muss im Erfolgszweig des
//! Abrufs stehen, das Lesen im Fehlerzweig — und beide nicht vertauscht.
//!
//! # Was hier NICHT geprueft wird
//!
//! Ob der Inhalt stimmt. Das machen die Einheitstests. Hier geht es
//! ausschliesslich darum, DASS der Weg verdrahtet ist.

use std::fs;

fn lib_rs() -> String {
    fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs")).expect("lib.rs lesbar")
}

/// Der Ausschnitt einer Funktion, von ihrer Signatur bis zur naechsten
/// Funktion auf oberster Ebene.
fn funktion<'a>(quelle: &'a str, signatur: &str) -> &'a str {
    let start = quelle
        .find(signatur)
        .unwrap_or_else(|| panic!("Funktion nicht gefunden: {signatur}"));
    let rest = &quelle[start..];
    // Die naechste Zeile, die ganz links mit `async fn`/`fn`/`pub` beginnt.
    let ende = rest
        .match_indices("\n}")
        .next()
        .map(|(i, _)| i + 2)
        .unwrap_or(rest.len());
    &rest[..ende]
}

#[test]
fn ein_erfolgreicher_abruf_legt_ab() {
    let q = lib_rs();
    let f = funktion(&q, "async fn fetch_navdata_for_flight(");
    assert!(
        f.contains("navdata_cache::ablegen("),
        "der Abrufweg legt nichts ab — der Zwischenspeicher bleibt \
         fuer immer leer, und der naechste Flug ohne Netz faellt wieder \
         auf OurAirports zurueck"
    );
}

#[test]
fn ein_gescheiterter_abruf_liest_den_zwischenspeicher() {
    let q = lib_rs();
    let f = funktion(&q, "async fn fetch_navdata_for_flight(");
    assert!(
        f.contains("navdata_cache::holen("),
        "der Fehlerzweig liest nicht aus dem Zwischenspeicher — \
         dann ist das Ablegen reine Beschaeftigung"
    );
}

#[test]
fn abgelegt_wird_im_erfolgs_und_gelesen_im_fehlerzweig() {
    // Vertauscht waere beides einzeln vorhanden und trotzdem sinnlos:
    // Wir wuerden ablegen, was wir gerade nicht bekommen haben, und
    // lesen, was wir gerade frisch in der Hand halten.
    let q = lib_rs();
    let f = funktion(&q, "async fn fetch_navdata_for_flight(");
    let ablegen = f
        .find("navdata_cache::ablegen(")
        .expect("ablegen vorhanden");
    let holen = f.find("navdata_cache::holen(").expect("holen vorhanden");
    let erfolg = f
        .find("Ok(mut airport) =>")
        .expect("Erfolgszweig vorhanden");

    assert!(
        ablegen > erfolg,
        "das Ablegen steht vor dem Erfolgszweig — es kann dort nicht \
         den geholten Flugplatz meinen"
    );
    assert!(
        holen > ablegen,
        "das Lesen steht vor dem Ablegen — dann liegt es nicht im \
         Fehlerzweig, sondern davor"
    );
}

#[test]
fn ein_unbekannter_platz_holt_keinen_alten_stand() {
    // `NotFound` heisst: Navigraph kennt den Platz im aktiven Zyklus
    // nicht. Ein abgelegter Stand ist dann keine bessere Auskunft,
    // sondern eine ueberholte — und wuerde einen geloeschten Flugplatz
    // beliebig lange am Leben halten.
    let q = lib_rs();
    let f = funktion(&q, "async fn fetch_navdata_for_flight(");
    let notfound = f
        .find("NavdataError::NotFound(_)) =>")
        .expect("NotFound-Zweig vorhanden");
    let naechster = f[notfound..]
        .find("Err(e) =>")
        .map(|i| notfound + i)
        .unwrap_or(f.len());
    assert!(
        !f[notfound..naechster].contains("navdata_cache::holen("),
        "der NotFound-Zweig greift auf den Zwischenspeicher zurueck — \
         damit ueberlebt ein aus dem Zyklus gefallener Platz beliebig lange"
    );
}

#[test]
fn ein_alter_stand_wird_nicht_wie_ein_frischer_gemeldet() {
    // Der Pilot muss im Protokoll sehen, dass die Bewertung auf altem
    // Stand steht. Sonst sucht man den Fehler spaeter ueberall ausser
    // dort. `warn` statt `info`, und das Alter muss drinstehen.
    let q = lib_rs();
    let f = funktion(&q, "async fn fetch_navdata_for_flight(");
    let stelle = f
        .find("benutze abgelegten Stand")
        .expect("die Meldung zum abgelegten Stand fehlt ganz");
    let umfeld = &f[stelle.saturating_sub(600)..stelle];
    assert!(
        umfeld.contains("tracing::warn!"),
        "der abgelegte Stand wird als normale Meldung protokolliert — \
         eine Bewertung auf altem Stand darf nicht aussehen wie eine \
         auf heutigem"
    );
    assert!(
        umfeld.contains("alter_tage"),
        "das Alter des Stands steht nicht in der Meldung"
    );

    // ⚠ Diese Pruefung MUSS den ganzen Fehlerzweig ansehen, nicht nur das
    // Umfeld der Meldung. In der ersten Fassung schaute sie 600 Zeichen
    // zurueck — die Zuweisung `let tage = …` steht weiter weg, und die
    // Gegenprobe blieb gruen, als ich genau diesen Fehler wieder einbaute.
    // Ein Waechter, der seinen eigenen Hauptfall verfehlt, ist schlimmer
    // als keiner: Er zaehlt als Deckung.
    let fehlerzweig = {
        let start = f.find("Err(e) =>").expect("Fehlerzweig vorhanden");
        &f[start..]
    };
    assert!(
        !fehlerzweig.contains("alter_tage(0)"),
        "das Alter wird gegen den 1. Januar 1970 gerechnet — dann meldet \
         JEDER abgelegte Stand „0 Tage alt\", also genau das Gegenteil \
         dessen, wofuer die Meldung da ist"
    );
    assert!(
        fehlerzweig.contains("alter_tage_jetzt()"),
        "das Alter wird nicht gegen die Systemuhr gerechnet"
    );
}
