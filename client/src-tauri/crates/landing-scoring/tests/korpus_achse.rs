//! Achsen-Korpus — läuft in JEDEM Testlauf, nicht nur auf Zuruf.
//!
//! # Warum es das gibt
//!
//! Am 27.08.2026 meldete die Bahndisziplin bei **9 von 46** Landungen
//! „Die Bahnachse in den Navdaten passt nicht zur Bahn im Simulator" —
//! darunter EDDK, EDDM und EGBB, alles Standardszenerie an bestens
//! kartierten Plätzen. Die Meldung beschuldigte die Daten zu Unrecht.
//!
//! Zwei Ursachen, beide hier festgehalten:
//!
//! 1. Die Ausgleichsgerade lief bis zur **Bahnkante** — der Stelle, an
//!    der das Flugzeug die Bahn schon verlassen hat. Sie lag damit über
//!    dem ganzen Ausschwenken zur Ausfahrt.
//! 2. Ein Winkel allein trennt nicht „unsere Achse ist falsch" von „das
//!    Flugzeug fuhr schräg".
//!
//! # Warum der Vorrat im Repo liegt
//!
//! `korpus_v170.rs` holt seinen Bestand vom VPS und ist deshalb
//! `#[ignore]`. Das ist für 900 Flüge richtig — aber eine Regel, die nur
//! geprüft wird, wenn jemand daran denkt, ist keine Wache. Diese 46
//! Landungen sind klein genug, um mitzureisen (136 KB), und decken 37
//! Flughäfen ab. Erzeugt von `tools/korpus/achsen_export.py`.

use landing_scoring::sub_bahndisziplin::{
    achse_fragwuerdig, achsen_befund, achsen_fenster_bis_m,
};

/// Ab wann ein Rückwärtsschritt kein Messrauschen mehr ist.
const GROSSER_RUECKSPRUNG_M: f64 = 50.0;

struct Landung {
    platz: String,
    breite_m: Option<f64>,
    mess_ende_m: Option<f64>,
    raeum_m: Option<f64>,
    proben: Vec<(f64, f64)>,
}

/// Winziger Leser — genug für dieses Format, ohne serde_json als
/// Testabhängigkeit einzuschleppen.
fn lies_korpus() -> Vec<Landung> {
    let roh = include_str!("achsen_korpus.jsonl");
    roh.lines()
        .filter(|z| !z.trim().is_empty())
        .map(|z| {
            let text = |feld: &str| -> Option<String> {
                let marke = format!("\"{feld}\":\"");
                z.find(&marke).map(|i| {
                    let rest = &z[i + marke.len()..];
                    rest[..rest.find('"').unwrap()].to_string()
                })
            };
            let zahl = |feld: &str| -> Option<f64> {
                let marke = format!("\"{feld}\":");
                let i = z.find(&marke)? + marke.len();
                let rest = &z[i..];
                let ende = rest
                    .find(|c: char| c == ',' || c == '}')
                    .unwrap_or(rest.len());
                rest[..ende].trim().parse::<f64>().ok()
            };
            let proben = {
                let i = z.find("\"proben\":[").unwrap() + "\"proben\":[".len();
                let rest = &z[i..];
                let ende = rest.rfind(']').unwrap();
                rest[..ende]
                    .split("],")
                    .filter_map(|paar| {
                        let p = paar.trim_start_matches('[').trim_end_matches(']');
                        let mut t = p.split(',');
                        Some((
                            t.next()?.trim().parse::<f64>().ok()?,
                            t.next()?.trim().parse::<f64>().ok()?,
                        ))
                    })
                    .collect::<Vec<_>>()
            };
            Landung {
                platz: text("platz").unwrap_or_default(),
                breite_m: zahl("breite_m"),
                mess_ende_m: zahl("mess_ende_m"),
                raeum_m: zahl("raeum_m"),
                proben,
            }
        })
        .collect()
}

/// Wie der Client heute rechnet — Fenster aus der Rangfolge, dann Befund.
fn urteil(l: &Landung) -> Option<bool> {
    let bis = achsen_fenster_bis_m(l.mess_ende_m, l.raeum_m, None)?;
    let b = achsen_befund(&l.proben, bis)?;
    Some(achse_fragwuerdig(b, l.breite_m))
}

#[test]
fn der_vorrat_ist_vollstaendig() {
    let k = lies_korpus();
    assert_eq!(k.len(), 56, "Korpus unerwartet gross/klein");
    assert!(
        k.iter().all(|l| l.proben.len() >= 10),
        "eine Landung hat zu wenige Proben — Leser kaputt?"
    );
    // Gegen einen stillen Lesefehler: plausible Querlagen.
    for l in &k {
        assert!(
            l.proben.iter().all(|(_, q)| q.abs() < 500.0),
            "{}: unplausible Querlage",
            l.platz
        );
    }

    // ⚠ Die Längsachse läuft NICHT überall vorwärts — und das ist echt.
    //
    // In EKVG (Vágar) fährt das Flugzeug nach der Landung auf der Bahn
    // zurück; der Platz hat kaum Ausfahrten. Und in LEPA stand viermal
    // exakt derselbe Punkt (465,2 m / +2,2 m) zwischen fortlaufenden
    // Proben — ein stecken gebliebener Messwert.
    //
    // Beides am 28.08.2026 aufgefallen, als der Vorrat wuchs. Statt die
    // Daten zu beanstanden, ordnet und entdoppelt `achsen_befund` jetzt
    // selbst. Geprüft wird deshalb die EIGENSCHAFT, die das garantiert:
    // Das Urteil darf nicht davon abhängen, in welcher Reihenfolge die
    // Proben eintrafen.
    let mut rueckwaerts_faelle = 0;
    for l in &k {
        let mut umgedreht = l.proben.clone();
        umgedreht.reverse();
        let bis = achsen_fenster_bis_m(l.mess_ende_m, l.raeum_m, None);
        let a = bis.and_then(|b| achsen_befund(&l.proben, b));
        let b = bis.and_then(|b| achsen_befund(&umgedreht, b));
        match (a, b) {
            (Some(x), Some(y)) => assert!(
                (x.winkel_grad - y.winkel_grad).abs() < 1e-9,
                "{}: Urteil haengt an der Reihenfolge ({} gegen {})",
                l.platz,
                x.winkel_grad,
                y.winkel_grad
            ),
            (None, None) => {}
            _ => panic!("{}: einmal Urteil, einmal nicht — Reihenfolge zaehlt", l.platz),
        }
        // ⚠ Nur GROSSE Rücksprünge zählen.
        //
        // Am Korpus gemessen (28.08.2026): 18 von 56 Landungen laufen
        // streckenweise rückwärts — aber 15 davon um weniger als 5 m.
        // Das ist Messrauschen beim langsamen Ausrollen und völlig
        // normal; eine Wache dagegen wäre nur lästig.
        //
        // Die drei grossen sind etwas anderes: 929 m und 308 m in EKVG
        // (Zurückrollen auf der Bahn) und 207 m in LEPA (ein stecken
        // gebliebener Messpunkt). Werden das plötzlich viele, stimmt
        // etwas mit der Projektion nicht.
        if l
            .proben
            .windows(2)
            .any(|w| w[0].0 - w[1].0 >= GROSSER_RUECKSPRUNG_M)
        {
            rueckwaerts_faelle += 1;
        }
    }
    assert!(
        rueckwaerts_faelle <= 5,
        "{rueckwaerts_faelle} von {} Landungen springen um mehr als {GROSSER_RUECKSPRUNG_M} m zurück",
        k.len()
    );
}

#[test]
fn nur_wenige_landungen_gelten_als_achsenfehler() {
    let k = lies_korpus();
    let auffaellig: Vec<&str> = k
        .iter()
        .filter(|l| urteil(l) == Some(true))
        .map(|l| l.platz.as_str())
        .collect();
    // Vor der Korrektur waren es 9 von 46 (20 %) — darunter EDDK, EDDM
    // und EGBB mit Standardszenerie. Danach 4.
    // Vor der Korrektur 9 von 46 (20 %); nach dem Fensterfix 5; nach der
    // Regel „bleibt die Spur auf der befestigten Fläche" noch 2 von 56 —
    // und das sind EDHE und FACT, beide mit bekannt kaputter Geometrie.
    assert!(
        auffaellig.len() <= 3,
        "zu viele Landungen als Szenerie-Versatz eingestuft: {auffaellig:?}"
    );
}

#[test]
fn plaetze_mit_standardszenerie_werden_nicht_beschuldigt() {
    // Die drei, an denen die Meldung nachweislich falsch war. Sie ist
    // eine Aussage ueber die DATEN — an solchen Plaetzen darf sie nicht
    // fallen, solange die Spur in der mittleren Bahnhaelfte bleibt.
    let k = lies_korpus();
    // EDDV kam am 28.08.2026 dazu: dort wurde EWG6850 um 0,3 m verworfen,
    // weil die erste Fassung der Regel auf den Höchstbetrag statt auf die
    // befestigte Fläche schaute.
    for platz in ["EDDK", "EDDM", "EGBB", "EDDV"] {
        for l in k.iter().filter(|l| l.platz == platz) {
            assert_ne!(
                urteil(l),
                Some(true),
                "{platz} wird zu Unrecht als Szenerie-Versatz gemeldet"
            );
        }
    }
}

#[test]
fn bekannt_falsch_kartierte_bahnen_bleiben_geschuetzt() {
    // EDHE ist der Fall, fuer den die Pruefung urspruenglich gebaut
    // wurde: 45,7 m Versatz, einseitig, auf einer 45 m breiten Bahn.
    // Faellt der weg, benotet die App einen Szenerie-Fehler als
    // Pilotenfehler — der teuerste Fehler, den diese Achse machen kann.
    let k = lies_korpus();
    let edhe: Vec<_> = k.iter().filter(|l| l.platz == "EDHE").collect();
    assert!(!edhe.is_empty(), "EDHE fehlt im Vorrat");
    for l in edhe {
        assert_eq!(
            urteil(l),
            Some(true),
            "EDHE muss uebersprungen bleiben"
        );
    }
}

#[test]
fn nur_wer_die_befestigte_flaeche_verlaesst_gilt_als_achsenfehler() {
    // Der Kern der Regel, an echten Daten: Jede uebersprungene Landung
    // MUSS behaupten, das Flugzeug sei neben der Bahn weitergerollt.
    // Sonst ist die Spur schlicht moeglich — und dann ist ein schraeger
    // Verlauf ein Manoever, kein Szenerie-Versatz.
    let k = lies_korpus();
    for l in k.iter().filter(|l| urteil(l) == Some(true)) {
        let bis = achsen_fenster_bis_m(l.mess_ende_m, l.raeum_m, None).unwrap();
        let b = achsen_befund(&l.proben, bis).unwrap();
        let halbe = l.breite_m.unwrap_or(0.0) / 2.0;
        assert!(
            b.groesster_betrag_m > halbe,
            "{}: uebersprungen, obwohl die Spur mit {:.1} m auf der {:.1} m breiten Bahn blieb",
            l.platz,
            b.groesster_betrag_m,
            halbe * 2.0
        );
    }
}

#[test]
fn eine_kreuzende_spur_weit_draussen_bleibt_geschuetzt() {
    // FACT kreuzt die Mittellinie — die Manöver-Ausnahme greift also
    // beinahe. Was ihn schützt, ist ALLEIN die Betragsgrenze: 35,3 m auf
    // 61 m Breite, das ist mehr als die mittlere Hälfte.
    //
    // Ohne diese Prüfung könnte jemand `MANOEVER_ANTEIL_BREITE`
    // aufweichen, und der Fall, für den die ganze Achse gebaut wurde,
    // würde als Pilotenfehler benotet. Das ist der teuerste Fehler, den
    // diese Rechnung machen kann — ein Szenerie-Versatz kostet dann
    // Punkte.
    let k = lies_korpus();
    let fact: Vec<_> = k.iter().filter(|l| l.platz == "FACT").collect();
    assert!(!fact.is_empty(), "FACT fehlt im Vorrat");
    for l in fact {
        let bis = achsen_fenster_bis_m(l.mess_ende_m, l.raeum_m, None).unwrap();
        let b = achsen_befund(&l.proben, bis).unwrap();
        assert!(
            b.kreuzt_mitte,
            "Annahme geprüft: FACT kreuzt die Mitte — sonst prüft dieser Test nichts"
        );
        assert_eq!(urteil(l), Some(true), "FACT muss uebersprungen bleiben");
    }
}

#[test]
fn das_fenster_macht_den_unterschied() {
    // Die Gegenprobe zur eigentlichen Ursache, an echten Daten: Legt man
    // die Gerade ueber die GANZE Spur (wie es die Bahnkante tat), steigt
    // die Zahl der Verdaechtigungen deutlich.
    let k = lies_korpus();
    let eng = k.iter().filter(|l| urteil(l) == Some(true)).count();
    let weit = k
        .iter()
        .filter(|l| {
            let ende = l.proben.last().map(|(x, _)| *x);
            match (ende, achsen_befund(&l.proben, ende.unwrap_or(0.0))) {
                (Some(_), Some(b)) => achse_fragwuerdig(b, l.breite_m),
                _ => false,
            }
        })
        .count();
    assert!(
        weit > eng,
        "ueber die ganze Spur muessten MEHR Landungen auffallen (eng {eng}, weit {weit})"
    );
}
