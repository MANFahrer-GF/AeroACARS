//! Die Rollspur beginnt am Aufsetzpunkt — und der Nachtrag haengt am Weg.
//!
//! # Der Befund
//!
//! Gemessen an ALLEN neun Landungen der ersten v1.7.1-Nacht (24.08.2026,
//! Live-Server) begann die Spur konstant hinter dem Aufsetzpunkt:
//!
//! ```text
//! LGAV 218 m · LEPA 251 · SGAS 213 · EDDK 216 · LOWW 213
//! EPWA 222 · EDDN 439 · EDDB 185 · LSZH 228
//! ```
//!
//! Es fehlte die Aufsetzzone — der Teil, um dessentwillen die Achse
//! gebaut wurde. In der Queransicht stand die Marke „Aufsetzen" im
//! Leeren, ohne Spur darunter.
//!
//! Ursache: Die Spur braucht `runway_match`, und der entsteht erst, wenn
//! der Aufsetzer bestaetigt ist (`elapsed_ms >= 1100`), plus zwei, drei
//! Ticks im langsamen Takt.
//!
//! # Warum das eine QUELLTEXT-Pruefung ist
//!
//! Der Verhaltenstest daneben (in `lib.rs`) ruft
//! `spur_aus_puffer_nachtragen` direkt. Er blieb gruen, als der AUFRUF im
//! Betriebsweg entfernt wurde — er prueft die Funktion, nicht die
//! Verdrahtung. Genau die Luecke, an der schon `runway_exits` haengen
//! geblieben ist: gebaut, getestet, nirgends gerufen.
//!
//! Diese Pruefung schliesst sie: Der Nachtrag muss dort stehen, wo der
//! Bahntreffer entsteht.

use std::fs;

/// Der Rumpf einer Funktion ab ihrer Signatur, ueber die Klammerbilanz.
fn rumpf<'a>(text: &'a str, signatur: &str) -> &'a str {
    let start = text.find(signatur).unwrap_or_else(|| {
        panic!("Funktion `{signatur}` nicht gefunden — wurde sie umbenannt?")
    });
    let rest = &text[start..];
    let mut tiefe = 0i32;
    let mut begonnen = false;
    for (i, c) in rest.char_indices() {
        match c {
            '{' => {
                tiefe += 1;
                begonnen = true;
            }
            '}' => {
                tiefe -= 1;
                if begonnen && tiefe == 0 {
                    return &rest[..=i];
                }
            }
            _ => {}
        }
    }
    rest
}

#[test]
fn der_nachtrag_haengt_dort_wo_der_bahntreffer_entsteht() {
    let quelle = fs::read_to_string("src/lib.rs").expect("lib.rs");
    let korrelation = rumpf(&quelle, "fn correlate_touchdown_runway(");

    assert!(
        korrelation.contains("stats.runway_match = Some("),
        "`correlate_touchdown_runway` setzt den Bahntreffer nicht mehr — \
         diese Pruefung zeigt auf die falsche Stelle"
    );
    assert!(
        korrelation.contains("spur_aus_puffer_nachtragen("),
        "Der Nachtrag wird dort, wo der Bahntreffer entsteht, NICHT \
         gerufen. Ohne ihn beginnt die Spur wieder gut zweihundert Meter \
         hinter dem Aufsetzpunkt — gemessen an allen neun Landungen der \
         ersten v1.7.1-Nacht."
    );

    // Und er muss NACH dem Treffer stehen: vorher gibt es nichts zu
    // projizieren, und die Funktion kehrt still zurueck.
    let i_treffer = korrelation
        .find("stats.runway_match = Some(")
        .expect("Treffer");
    let i_nachtrag = korrelation
        .find("spur_aus_puffer_nachtragen(")
        .expect("Nachtrag");
    assert!(
        i_nachtrag > i_treffer,
        "Der Nachtrag steht VOR dem Bahntreffer. Dann ist `runway_match` \
         noch None, die Funktion kehrt still zurueck, und die Spur bleibt \
         wie vorher — ohne dass irgendetwas fehlschlaegt."
    );
}

/// Und der Nachtrag benutzt dieselbe Ausduennung wie der Live-Takt.
///
/// Eine zweite Spur-Logik waere genau die Zweitimplementierung, die
/// dieses Projekt schon mehrfach teuer bezahlt hat: dieselbe Frage, zwei
/// Antworten, und die Abweichung faellt erst auf, wenn jemand beide
/// nebeneinanderlegt.
#[test]
fn der_nachtrag_baut_keine_zweite_spur_logik() {
    let quelle = fs::read_to_string("src/lib.rs").expect("lib.rs");
    // Seit dem fortlaufenden Abschoepfen reicht `spur_aus_puffer_nachtragen`
    // nur noch weiter. Geprueft wird die Stelle, die die Arbeit macht —
    // die Absicht des Waechters ist dieselbe geblieben: EINE Ausduennung,
    // nicht zwei, die gegeneinander driften.
    let nachtrag = rumpf(&quelle, "fn spur_aus_puffer_abschoepfen(");

    assert!(
        nachtrag.contains("spur_fortschreiben("),
        "Der Nachtrag ruft `spur_fortschreiben` nicht — er baut seine \
         eigene Ausduennung, und die driftet gegen den Live-Takt."
    );
    for verboten in ["bahn_spur.push", "BAHN_SPUR_MIN_ABSTAND_M", "bahn_kante_m ="] {
        assert!(
            !nachtrag.contains(verboten),
            "Der Nachtrag fasst `{verboten}` selbst an, statt es \
             `spur_fortschreiben` zu ueberlassen."
        );
    }
}

/// `touchdown_complete` darf sich nicht als endgültig ausgeben.
///
/// # Der Fall
///
/// EDDB 06L, 24.08.2026 (Landung 1079): Zwölf von dreizehn Landungen des
/// Tages bekamen ihre Finalisierung, diese eine nicht. Der Bericht zeigte
/// deshalb den Zwischenstand von neun Sekunden nach dem Aufsetzen — und
/// zwar so, als wäre er das Endergebnis:
///
///   * 482 m Ausrollstrecke (nachgerechnet 0,42 g — mehr, als ein
///     Verkehrsflugzeug bremsen kann),
///   * eine Spur, die mitten auf der 3600-m-Bahn aufhört,
///   * kein Räumpunkt.
///
/// Nichts davon war falsch gemessen. Es war nur noch nicht fertig, und
/// niemand konnte das sehen.
///
/// `rollout_final` unterscheidet beides. Diese Prüfung hält fest, dass
/// die zwei Sendestellen es richtig herum setzen — vertauscht wäre der
/// Zustand schlimmer als vorher: Dann behauptete die Finalisierung, sie
/// sei vorläufig, und der Zwischenstand, er sei fertig.
#[test]
fn der_zwischenstand_gibt_sich_nicht_als_endgueltig_aus() {
    let quelle = fs::read_to_string("src/lib.rs").expect("lib.rs");

    let mut vorlaeufig = 0;
    let mut endgueltig = 0;
    for zeile in quelle.lines() {
        if zeile.contains(".wire(false)") {
            vorlaeufig += 1;
        }
        if zeile.contains(".wire(true)") {
            endgueltig += 1;
        }
    }
    assert!(
        vorlaeufig >= 1,
        "Keine Sendestelle markiert ihren Stand als vorläufig. \
         `touchdown_complete` geht neun Sekunden nach dem Aufsetzen raus — \
         da rollt das Flugzeug noch."
    );
    assert!(
        endgueltig >= 1,
        "Keine Sendestelle markiert ihren Stand als endgültig — dann gilt \
         jeder Bericht als vorläufig und der Hinweis verliert seine Aussage."
    );

    // Und die Zuordnung muss stimmen: Der Aufruf im Finalisierungs-Zweig
    // trägt `true`, der daneben `false`.
    let final_zweig = rumpf(&quelle, "if stats.rollout_finalized && spur_fertig {");
    assert!(
        final_zweig.contains(".wire(true)"),
        "Der Finalisierungs-Zweig sendet nicht `wire(true)` — dann gilt das \
         Endergebnis als Zwischenstand."
    );
    assert!(
        !final_zweig.contains(".wire(false)"),
        "Im Finalisierungs-Zweig steht `wire(false)` — vertauscht."
    );
}

// ── Die Aufloesung der Spur haengt am Abtaster, nicht am Sendetakt ───
//
// # Der Befund (Thomas, 25.08.2026)
//
// „Aber die Datenpunkte-Wolke ist auch nicht gut → zum Anfang und Ende
// kaum welche." Nachgemessen an Flug #1081 (KDAL/13L):
//
// ```text
//  740–1038 m (Aufsetzzone)    7 Punkte   42,6 m Abstand
// 1038–1336 m                 23 Punkte   13,0 m
// 1932–2230 m                 49 Punkte    6,1 m
// groesste Luecke                        222,4 m
// ```
//
// Ueber fuenfzehn Landungen des Live-Korpus: Median 13–14 m in den
// ersten dreihundert Metern, groesste Luecke je Fassung 73–96 m.
//
// Ursache: Die Spur wurde aus dem Streamer-Tick gefuettert (zwei Hertz
// vor dem Aufsetzen, fuenf im erkannten Ausrollen). Der 50-Hz-Puffer,
// aus dem der Aufsetzer selbst erkannt wird, wurde nur EINMAL angezapft
// — beim Nachtrag nach der Bahnzuordnung, und der deckt kaum eine
// Sekunde ab.

#[test]
fn der_puffer_wird_fortlaufend_abgeschoepft_nicht_nur_einmal() {
    let q = fs::read_to_string("src/lib.rs").expect("lib.rs");
    let tick = rumpf(&q, "fn bahndisziplin_tick(");
    assert!(
        tick.contains("spur_aus_puffer_abschoepfen("),
        "der Takt schoepft den 50-Hz-Puffer nicht nach — die Spur haengt \
         wieder am Sendetakt, und die Aufsetzzone bekommt die wenigsten \
         Punkte der ganzen Landung"
    );
}

#[test]
fn erst_der_puffer_dann_der_aktuelle_wert() {
    // Der Puffer traegt die Werte ZWISCHEN dem letzten Tick und jetzt.
    // Kaemen sie danach, liefen die Punkte rueckwaerts und die Zeichnung
    // zeigte einen Zickzack, den es nie gab.
    let q = fs::read_to_string("src/lib.rs").expect("lib.rs");
    let tick = rumpf(&q, "fn bahndisziplin_tick(");
    let puffer = tick
        .find("spur_aus_puffer_abschoepfen(")
        .expect("Abschoepfen vorhanden");
    let jetzt = tick
        .find("spur_fortschreiben(stats, snap.groundspeed_kt")
        .expect("Fortschreibung vorhanden");
    assert!(
        puffer < jetzt,
        "der aktuelle Wert wird VOR dem Puffer gelegt — die Punkte laufen \
         rueckwaerts"
    );
}

#[test]
fn der_merker_verhindert_das_doppelte_durchrechnen() {
    // Ohne ihn wuerde bei jedem Tick der ganze Ringpuffer neu
    // durchgerechnet. Falsche Punkte gaebe das nicht (der Mindestabstand
    // faengt das ab), aber es waere Arbeit fuer nichts, fuenfmal je
    // Sekunde.
    let q = fs::read_to_string("src/lib.rs").expect("lib.rs");
    let f = rumpf(&q, "fn spur_aus_puffer_abschoepfen(");
    assert!(
        f.contains("bahn_spur_bis"),
        "kein Merker — der Puffer wird bei jedem Tick komplett neu gelesen"
    );
    assert!(
        f.contains("stats.bahn_spur_bis = Some(at)"),
        "der Merker wird nie fortgeschrieben"
    );
}

#[test]
fn der_nachtrag_geht_denselben_weg() {
    // Zwei Wege in dieselbe Ablage waeren zwei Stellen, an denen die
    // Reihenfolge kaputtgehen kann. Der Nachtrag ist deshalb nur noch
    // ein Aufruf des Abschoepfens.
    let q = fs::read_to_string("src/lib.rs").expect("lib.rs");
    let f = rumpf(&q, "fn spur_aus_puffer_nachtragen(");
    assert!(
        f.contains("spur_aus_puffer_abschoepfen("),
        "der Nachtrag hat wieder eine eigene Schleife"
    );
}
