//! Korpus-Nachrechnung für v1.7.0 — **QS-Kriterium 2 der Spezifikation**.
//!
//! Fährt den echten Bestand durch die **echten** Achsen und stellt die neue
//! Bewertung der alten gegenüber. Kein Python-Modell: Genau der Code, der
//! später beim Piloten läuft.
//!
//! # Warum das nicht im normalen Testlauf hängt
//!
//! Der Korpus liegt auf dem VPS, nicht im Repo — knapp 900 Flug-Protokolle
//! mit je mehreren tausend Positionszeilen. Deshalb `#[ignore]` und eine
//! CSV als Eingabe:
//!
//! ```text
//! # das Werkzeug liegt im Repo, nicht auf dem VPS:
//! scp tools/korpus/korpus_export.py live:/tmp/
//! ssh live python3 /tmp/korpus_export.py     # erzeugt /tmp/korpus_v170.csv
//! scp live:/tmp/korpus_v170.csv /tmp/
//! KORPUS=/tmp/korpus_v170.csv cargo test -p landing-scoring --test korpus_v170 -- --ignored --nocapture
//! ```
//!
//! ⚠ **Das Werkzeug gehört ins Repo, nicht nach `/tmp`.** Zwei Fehler darin
//! haben je ein Drittel bzw. ein Viertel aller Landungen falsch gemessen und
//! dabei einen grünen Test erzeugt — siehe §12.6 der Spezifikation. Ein
//! Prüfwerkzeug muss so lesbar und versioniert sein wie der Code, den es prüft.
//!
//! Die CSV enthält je Landung die Eingangsgrössen; die **Projektion** dort ist
//! zeichengleich zu `runway::projiziere_auf_bahn` (Kugelformel, nicht ebene
//! Näherung) und an MPH 9 gegen den vom Client gemeldeten Aufsetzpunkt geprüft.
//!
//! # Was der Lauf beantwortet
//!
//! 1. Wie viele Landungen schlagen auf der neuen Disziplin-Achse überhaupt an?
//!    (Erwartung laut Spec: rund 2 %.)
//! 2. Wer gewinnt, wer verliert gegenüber der alten Auslastungs-Achse?
//! 3. Greifen die Skip-Pfade — und erzeugt keiner davon eine Note?

use landing_scoring::belag::belag_aus_angabe;
use landing_scoring::spurweite::spurweite_m;
use landing_scoring::sub_bahndisziplin::{sub_bahndisziplin, BahndisziplinInput};
use landing_scoring::sub_touchdown_point::{sub_touchdown_point, TouchdownPointInput};
use std::collections::BTreeMap;

struct Zeile {
    pirep: String,
    icao: String,
    rwy: String,
    muster: String,
    td_m: f64,
    lda_m: f64,
    breite_m: f64,
    belag: String,
    max_quer_m: Option<f64>,
    overrun_m: Option<f64>,
    proben: usize,
    gs_start: Option<f64>,
    gs_ende: Option<f64>,
    alt_punkte: Option<u8>,
}

fn lies_korpus(pfad: &str) -> Vec<Zeile> {
    let inhalt = std::fs::read_to_string(pfad)
        .unwrap_or_else(|e| panic!("Korpus {pfad} nicht lesbar: {e}"));
    let mut zeilen = inhalt.lines();
    let kopf: Vec<&str> = zeilen.next().expect("Kopfzeile").split(',').collect();
    let idx = |name: &str| {
        kopf.iter()
            .position(|k| *k == name)
            .unwrap_or_else(|| panic!("Spalte {name} fehlt"))
    };
    let (i_p, i_i, i_r, i_m) = (idx("pirep"), idx("icao"), idx("rwy"), idx("muster"));
    let (i_td, i_lda, i_b) = (idx("td_m"), idx("lda_m"), idx("breite_m"));
    let (i_bel, i_q, i_o, i_pr, i_alt) = (
        idx("belag"),
        idx("max_quer_m"),
        idx("overrun_m"),
        idx("proben"),
        idx("alt_punkte"),
    );
    let (i_gs0, i_gs1) = (idx("gs_start"), idx("gs_ende"));
    zeilen
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split(',').collect();
            let z = |i: usize| f.get(i).copied().unwrap_or("").trim().to_string();
            let n = |i: usize| z(i).parse::<f64>().ok();
            Zeile {
                pirep: z(i_p),
                icao: z(i_i),
                rwy: z(i_r),
                muster: z(i_m),
                td_m: n(i_td).unwrap_or(0.0),
                lda_m: n(i_lda).unwrap_or(0.0),
                breite_m: n(i_b).unwrap_or(0.0),
                belag: z(i_bel),
                max_quer_m: n(i_q),
                overrun_m: n(i_o),
                proben: n(i_pr).unwrap_or(0.0) as usize,
                gs_start: n(i_gs0),
                gs_ende: n(i_gs1),
                alt_punkte: n(i_alt).map(|v| v as u8),
            }
        })
        .collect()
}

#[test]
#[ignore = "braucht den VPS-Korpus — siehe Modul-Doku"]
fn korpus_nachrechnung() {
    let pfad = std::env::var("KORPUS").expect("KORPUS=<csv> setzen");
    let zeilen = lies_korpus(&pfad);
    assert!(!zeilen.is_empty(), "Korpus ist leer");

    // ── Zuerst: taugen die Eingangsdaten überhaupt? ──────────────────
    //
    // Warum das VOR jeder Auswertung steht: Beim ersten Lauf war dieser Test
    // grün, während das Prüfwerkzeug bei **281 von 802 Landungen (35 %)** den
    // Startlauf statt des Landerollens gemessen hatte (Spec §12.6, Fehler 1).
    // Ein grüner Test auf falschen Daten ist schlimmer als ein roter — er
    // erzeugt Vertrauen, das nicht gedeckt ist.
    //
    // Die Prüfung ist physikalisch, nicht statistisch: Beim Ausrollen wird
    // ein Flugzeug **langsamer**. Steigt die Geschwindigkeit über das
    // Messfenster hinweg, misst das Fenster etwas anderes als eine Landung.
    // Die 10 kt Spielraum decken Messrauschen und einen kurzen Schub beim
    // Verlassen der Bahn ab; der EDDL-Fall stieg um 26 kt.
    {
        let mut beschleunigt: Vec<String> = Vec::new();
        for z in &zeilen {
            if let (Some(a), Some(e)) = (z.gs_start, z.gs_ende) {
                if e > a + 10.0 {
                    beschleunigt.push(format!(
                        "{} {} {} — {a:.0} → {e:.0} kt (+{:.0})",
                        z.pirep,
                        z.icao,
                        z.rwy,
                        e - a
                    ));
                }
            }
        }
        if !beschleunigt.is_empty() {
            println!("\nMessfenster, in denen das Flugzeug SCHNELLER wurde:");
            for b in beschleunigt.iter().take(15) {
                println!("   {b}");
            }
            panic!(
                "{} von {} Messfenstern zeigen Beschleunigung — das ist kein \
                 Ausrollen. Der Korpus-Export misst am falschen Punkt.",
                beschleunigt.len(),
                zeilen.len()
            );
        }
    }

    let mut disziplin: BTreeMap<&str, usize> = BTreeMap::new();
    let mut aufsetz: BTreeMap<&str, usize> = BTreeMap::new();
    let mut skips: BTreeMap<String, usize> = BTreeMap::new();
    let mut besser = 0usize;
    let mut schlechter = 0usize;
    let mut gleich = 0usize;
    let mut ohne_vergleich = 0usize;
    let mut verlierer: Vec<(String, u8, u8, String)> = Vec::new();

    for z in &zeilen {
        let d = sub_bahndisziplin(&BahndisziplinInput {
            max_querversatz_m: z.max_quer_m,
            bahnbreite_m: Some(z.breite_m).filter(|b| *b > 0.0),
            spurweite_m: spurweite_m(Some(&z.muster)),
            overrun_m: z.overrun_m,
            belag: Some(belag_aus_angabe(Some(&z.belag))),
            airport_source: Some("runway_match"),
            runway_geometry_trusted: Some(true),            achsen_abweichung_grad: None,
        proben: Some(z.proben),
        });

        // Aim/TDZ nach denselben Regeln wie `runway_assessment`.
        let aim = if z.lda_m >= 2400.0 { 400.0 } else { 300.0 };
        let tdz = if z.lda_m >= 1200.0 {
            Some(900.0_f64.min(z.lda_m / 3.0))
        } else {
            None
        };
        let a = sub_touchdown_point(&TouchdownPointInput {
            td_distance_from_threshold_m: Some(z.td_m),
            aim_point_m: Some(aim),
            tdz_end_m: tdz,
            lda_m: Some(z.lda_m),
            airport_source: Some("runway_match"),
            runway_geometry_trusted: Some(true),
        });

        if d.skipped {
            *skips.entry(d.reason.clone().unwrap_or_default()).or_default() += 1;
        } else {
            let grund = d
                .rationale_key
                .as_deref()
                .and_then(|k| k.strip_prefix("landing.rat."))
                .unwrap_or("?");
            *disziplin.entry(Box::leak(grund.to_string().into_boxed_str())).or_default() += 1;
        }
        if !a.skipped {
            let grund = a
                .rationale_key
                .as_deref()
                .and_then(|k| k.strip_prefix("landing.rat."))
                .unwrap_or("?");
            *aufsetz.entry(Box::leak(grund.to_string().into_boxed_str())).or_default() += 1;
        }

        match (z.alt_punkte, d.skipped) {
            (Some(alt), false) => {
                if d.points > alt {
                    besser += 1;
                } else if d.points < alt {
                    schlechter += 1;
                    verlierer.push((
                        format!("{} {} {}", z.pirep, z.icao, z.rwy),
                        alt,
                        d.points,
                        d.value.clone().unwrap_or_default(),
                    ));
                } else {
                    gleich += 1;
                }
            }
            _ => ohne_vergleich += 1,
        }
    }

    let n = zeilen.len();
    println!("\n════ Korpus v1.7.0 — {n} Landungen ════\n");

    println!("Bahndisziplin (neu):");
    for (grund, anzahl) in &disziplin {
        println!("   {grund:<18} {anzahl:>4}   {:>5.1} %", 100.0 * *anzahl as f64 / n as f64);
    }
    println!("Übersprungen:");
    for (grund, anzahl) in &skips {
        println!("   {grund:<18} {anzahl:>4}   {:>5.1} %", 100.0 * *anzahl as f64 / n as f64);
    }

    println!("\nAufsetzpunkt (neue Achse):");
    for (grund, anzahl) in &aufsetz {
        println!("   {grund:<20} {anzahl:>4}   {:>5.1} %", 100.0 * *anzahl as f64 / n as f64);
    }

    println!("\nGegenüber der alten Auslastungs-Achse:");
    println!("   besser        {besser:>4}");
    println!("   unverändert   {gleich:>4}");
    println!("   schlechter    {schlechter:>4}");
    println!("   kein Vergleich{ohne_vergleich:>4}");

    if !verlierer.is_empty() {
        println!("\nWer verliert — die zehn grössten Abstände:");
        verlierer.sort_by_key(|(_, alt, neu, _)| *neu as i32 - *alt as i32);
        for (wer, alt, neu, wert) in verlierer.iter().take(10) {
            println!("   {wer:<34} {alt:>3} → {neu:>3}   {wert}");
        }
    }

    // ── Zusicherungen der Spezifikation ──────────────────────────────
    let angeschlagen: usize = disziplin
        .iter()
        .filter(|(g, _)| **g != "centered")
        .map(|(_, a)| *a)
        .sum();
    let quote = 100.0 * angeschlagen as f64 / n as f64;
    println!("\nAchse schlägt an bei {angeschlagen} von {n} = {quote:.1} %");
    assert!(
        quote < 15.0,
        "Die Disziplin-Achse soll die Ausnahme melden, nicht die Regel — {quote:.1} % ist zu viel"
    );

    let daneben = disziplin.get("off_pavement").copied().unwrap_or(0);
    let daneben_quote = 100.0 * daneben as f64 / n as f64;
    println!("davon „Rad neben der Bahn\": {daneben} = {daneben_quote:.1} %");
    assert!(
        daneben_quote < 3.0,
        "QS-Kriterium 4: über 3 % bedeutet, dass reguläres Ausfahren mitgezählt wird ({daneben_quote:.1} %)"
    );
}
