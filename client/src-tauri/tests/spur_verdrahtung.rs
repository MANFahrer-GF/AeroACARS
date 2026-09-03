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

/// Der Quelltext ohne Zeilenkommentare.
///
/// ⚠ Ohne das findet eine Suche nach einer verbotenen Form sich selbst,
/// sobald ein Kommentar die Form nennt — und ein Waechter, der sich
/// selbst findet, ist immer gruen.
fn nur_code(text: &str) -> String {
    // ⚠ Kein `find("//")` je Zeile. Das schnitt mitten in
    // `"https://…"`, und die Klammerbilanz von `produktionsteil` zaehlte
    // `{`/`}` in Zeichenketten mit — 16 Zeilen in `lib.rs` tragen
    // unbalancierte Klammern in Strings, und es ging nur auf, weil sie
    // sich zufaellig paarweise ausglichen (externe QS, 02.09.2026, N10).
    //
    // Zeichenketten werden auf `""` geleert (Rohstrings mit `#`-Zaun
    // eingeschlossen), Zeilen- und Blockkommentare entfernt, Zeichen-
    // literale wie `'{'` geleert. Lebensdauern (`'a`) bleiben stehen.
    let z: Vec<char> = text.chars().collect();
    let mut aus = String::with_capacity(text.len());
    let mut i = 0;
    while i < z.len() {
        let c = z[i];
        // Zeilenkommentar
        if c == '/' && z.get(i + 1) == Some(&'/') {
            while i < z.len() && z[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // Blockkommentar (verschachtelt, wie in Rust)
        if c == '/' && z.get(i + 1) == Some(&'*') {
            let mut tiefe = 1;
            i += 2;
            while i < z.len() && tiefe > 0 {
                if z[i] == '/' && z.get(i + 1) == Some(&'*') {
                    tiefe += 1;
                    i += 2;
                } else if z[i] == '*' && z.get(i + 1) == Some(&'/') {
                    tiefe -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        // Rohstring r"…" / r#"…"#
        if c == 'r' && (z.get(i + 1) == Some(&'"') || z.get(i + 1) == Some(&'#')) {
            let mut j = i + 1;
            let mut zaun = 0;
            while z.get(j) == Some(&'#') {
                zaun += 1;
                j += 1;
            }
            if z.get(j) == Some(&'"') {
                j += 1;
                loop {
                    if j >= z.len() {
                        break;
                    }
                    if z[j] == '"' && (0..zaun).all(|k| z.get(j + 1 + k) == Some(&'#')) {
                        j += 1 + zaun;
                        break;
                    }
                    j += 1;
                }
                aus.push_str("\"\"");
                i = j;
                continue;
            }
        }
        // Zeichenkette
        if c == '"' {
            let mut j = i + 1;
            while j < z.len() {
                if z[j] == '\\' {
                    j += 2;
                    continue;
                }
                if z[j] == '"' {
                    break;
                }
                j += 1;
            }
            aus.push_str("\"\"");
            i = j + 1;
            continue;
        }
        // Zeichenliteral 'x' oder '\n' — NICHT die Lebensdauer 'a
        if c == '\'' {
            let lit = match (z.get(i + 1), z.get(i + 2), z.get(i + 3)) {
                (Some('\\'), Some(_), Some('\'')) => Some(4),
                (Some(x), Some('\''), _) if *x != '\\' => Some(3),
                _ => None,
            };
            if let Some(n) = lit {
                aus.push_str("' '");
                i += n;
                continue;
            }
        }
        aus.push(c);
        i += 1;
    }
    aus
}

/// Der Teil von `lib.rs` VOR dem ersten `#[cfg(test)]`.
///
/// Ein Waechter, der ueber die ganze Datei zaehlt, zaehlt die Tests mit
/// — und die rufen dieselben Funktionen. Dann bleibt er gruen, wenn die
/// Produktion sie nicht mehr ruft.
fn produktionsteil(quelle: &str) -> String {
    // ⚠ `lib.rs` hat 51 Testbloecke, VERSTREUT ueber die ganze Datei —
    // der erste liegt bei Zeile 500. Beim ersten abzuschneiden hiesse,
    // fast die ganze Produktion mit wegzuwerfen; der Waechter meldete
    // dann „0 Stellen" fuer Funktionen, die es gibt. Jeder Block wird
    // einzeln ueber die Klammerbilanz entfernt.
    let mut rest = quelle;
    let mut aus = String::with_capacity(quelle.len());
    while let Some(i) = rest.find("#[cfg(test)]") {
        aus.push_str(&rest[..i]);
        let nach = &rest[i..];
        let Some(auf) = nach.find('{') else {
            break;
        };
        let mut tiefe = 0i32;
        let mut ende = None;
        for (k, c) in nach[auf..].char_indices() {
            match c {
                '{' => tiefe += 1,
                '}' => {
                    tiefe -= 1;
                    if tiefe == 0 {
                        ende = Some(auf + k + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = match ende {
            Some(e) => &nach[e..],
            None => "",
        };
    }
    aus.push_str(rest);
    aus
}

/// Der Rumpf einer Funktion ab ihrer Signatur, ueber die Klammerbilanz.
fn rumpf<'a>(text: &'a str, signatur: &str) -> &'a str {
    let start = text
        .find(signatur)
        .unwrap_or_else(|| panic!("Funktion `{signatur}` nicht gefunden — wurde sie umbenannt?"));
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
    // ⚠ Seit v1.7.15 steht der Kern in `korreliere_bahn`.
    //
    // `correlate_touchdown_runway` bestimmt nur noch die Position und
    // ruft weiter; das Nachholen ruft denselben Kern OHNE Schnappschuss.
    // Der Nachtrag muss dort haengen, wo der Bahntreffer ENTSTEHT —
    // sonst bekommt der nachgeholte Weg ihn nicht.
    let korrelation = rumpf(&quelle, "fn korreliere_bahn(");

    assert!(
        korrelation.contains("stats.runway_match = Some("),
        "`korreliere_bahn` setzt den Bahntreffer nicht mehr — diese \
         Pruefung zeigt auf die falsche Stelle"
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
    for verboten in [
        "bahn_spur.push",
        "BAHN_SPUR_MIN_ABSTAND_M",
        "bahn_kante_m =",
    ] {
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
    // Seit Runde 5 baut der Finalisierungs-Zweig den Block nicht mehr
    // selbst: Er ruft `spur_block`, und DORT steht das `wire(true)` — eine
    // Entscheidung fuer beide Sender. Die Zusicherung bleibt dieselbe:
    // Der Zweig sendet den Block, der als endgueltig markiert ist.
    let final_zweig = rumpf(&quelle, "if stats.rollout_finalized && spur_fertig {");
    let block_bauer = rumpf(&quelle, "fn spur_block(");
    assert!(
        final_zweig.contains("spur_block(&flight, &stats)") && block_bauer.contains(".wire(true)"),
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

/// ⚠ Die Bahn-Herkunft entsteht an GENAU EINER Stelle.
///
/// Zwei Ereignisse tragen dieselbe Feldgruppe: `touchdown_complete` und
/// `touchdown_rollout_finalized`. Bis v1.7.14 zaehlte jede Stelle ihre
/// Felder selbst auf — mit dem Ergebnis, dass sie vier von
/// achtundzwanzig gemeinsam hatten. Bahnlaenge, versetzte Schwelle,
/// Herkunft und die ganze Aufsetzpunkt-Einordnung blieben beim Nachtrag
/// auf dem Stand VOR der Szenerie.
///
/// Der Compiler faengt das NICHT: Beide Stellen bauen gueltige Structs,
/// sie bauen nur verschiedene Werte hinein.
#[test]
fn die_bahn_herkunft_entsteht_nur_einmal() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let bau = format!("{}::{} {{", "aeroacars_mqtt", "BahnHerkunftWire");
    // ⚠ Nur die Struktur-Literale, nicht die Signatur: `-> …Wire {`
    // enthaelt dieselbe Zeichenfolge und haette den Waechter um eins
    // verschoben — also genau um die zweite Stelle, die er suchen soll.
    let stellen = quelle
        .lines()
        .filter(|z| z.trim() == bau && !z.contains("->"))
        .count();
    assert_eq!(
        stellen, 1,
        "Die Feldgruppe wird an {stellen} Stellen gebaut. Genau daran ist \
         sie in v1.7.14 auseinandergelaufen — eine Stelle wurde \
         nachgezogen, die andere nicht. Es gibt `bahn_herkunft(stats)`."
    );

    // Und beide Ereignisse benutzen sie auch.
    //
    // ⚠ Nur im PRODUKTIONSTEIL zaehlen. Die Tests in `lib.rs` rufen
    // `bahn_herkunft(&stats)` viermal — ueber die ganze Datei gezaehlt
    // blieb der Waechter gruen, auch wenn beide Aufrufstellen in der
    // Produktion fehlten (externe QS, 02.09.2026, P2-D).
    let produktion = produktionsteil(&quelle);
    // ⚠ Runde 5 (N28): `bahn_nachtrag_bauen` ruft `bahn_herkunft(stats)`
    // OHNE `&` — die dritte Aufrufstelle war nicht gedeckt. Gezaehlt wird
    // im RUMPF, nicht ueber die Datei: `bahn_upgrade_anwenden` ruft
    // dieselbe Form zweimal fuer den Vorher/Nachher-Vergleich, und eine
    // Dateizaehlung wuerde davon rot, ohne dass etwas fehlt.
    assert!(
        rumpf(&produktion, "fn bahn_nachtrag_bauen(").contains("bahn_herkunft(stats)"),
        "der Einreich-Nachtrag ruft `bahn_herkunft` nicht mehr — dann traegt \
         er die Herkunft nicht"
    );
    assert_eq!(
        produktion.matches("bahn_herkunft(&stats)").count(),
        2,
        "Eines der beiden Touchdown-Ereignisse ruft `bahn_herkunft` nicht \
         — dann traegt es die Korrektur einer spaet eingetroffenen \
         Szenerie nicht mit."
    );
}

/// ⚠ Das Drittel wird bei JEDER Zuordnung nachgefuehrt.
///
/// `drittel_nachfuehren` fuer sich zu testen reicht nicht — genau daran
/// ist `runway_exits` schon einmal haengen geblieben: gebaut, getestet,
/// nirgends gerufen. Der Aufruf muss im Kern der Zuordnung stehen, damit
/// er das Nachholen mitnimmt.
///
/// Und er muss GANZ am Ende stehen: davor gerufen liest er den
/// Bahntreffer der vorigen Runde.
#[test]
fn das_drittel_wird_bei_jeder_zuordnung_nachgefuehrt() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let kern = rumpf(&quelle, "fn korreliere_bahn(");
    let ruf = format!("{}(stats);", "drittel_nachfuehren");
    assert!(
        kern.contains(&ruf),
        "`korreliere_bahn` fuehrt das Drittel nicht nach — dann folgt \
         `landing_touchdown_zone` einer spaet eingetroffenen Bahn nicht, \
         und zwei Webapp-Karten zeigen verschiedene Drittel."
    );

    // Nach dem Aufruf darf kein `stats.runway_match`/`runway_nav_geometry`
    // mehr gesetzt werden — sonst rechnet die Nachfuehrung mit dem
    // vorigen Stand.
    let nach = &kern[kern.find(&ruf).expect("Aufruf") + ruf.len()..];
    for feld in ["stats.runway_match =", "stats.runway_nav_geometry ="] {
        assert!(
            !nach.contains(feld),
            "`{feld}` steht NACH der Nachfuehrung — dann traegt das \
             Drittel den Stand von davor."
        );
    }
}

/// ⚠ Die Revision steht auf der Platte, BEVOR sie auf die Leitung geht.
///
/// Der normale Speichertakt laeuft alle fuenf Positionstakte — das
/// Ereignis geht sofort raus. Bleibt die Platte zurueck und stuerzt die
/// App ab, faengt der fortgesetzte Flug bei einer Revision an, die der
/// Recorder laengst ueberschritten hat: Der Riegel weist dann jede
/// weitere Korrektur ab und tut dabei genau, was in ihm steht.
///
/// Der Test bewacht die REIHENFOLGE, nicht das Vorhandensein — beides
/// nebeneinander waere folgenlos.
#[test]
fn die_revision_liegt_vor_dem_versand_auf_der_platte() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    // ⚠ Die LETZTE Fundstelle — das ist der Streamer.
    //
    // Die frueher im Text stehende gehoert zu `finalize_filed_pirep`,
    // das den offenen Nachtrag beim Einreichen schickt. Dort endet der
    // Flug ohnehin, eine Sicherung waere folgenlos. `find` haette diese
    // getroffen und den Waechter auf die falsche Stelle gerichtet.
    // Seit Runde 12 heisst der Sendeweg `nachtrag_queue::senden` (Ablage
    // zuerst, Datei faellt mit der Zustellmeldung); der Streamer ist
    // weiterhin die LETZTE Aufrufstelle in der Datei.
    let versand = format!("{}::{}(", "nachtrag_queue", "senden");
    let stelle = quelle
        .rfind(&versand)
        .expect("der Streamer schickt den Nachtrag nicht mehr");
    let davor = &quelle[..stelle];
    let sicherung = format!("{}(&app, &flight);", "revision_vor_versand_sichern");
    let letzte_sicherung = davor
        .rfind(&sicherung)
        .expect("vor dem Versand wird nicht gesichert");

    // Sie muss im SELBEN Zweig stehen wie der Versand — also NACH der
    // Sperre, die den Zweig oeffnet. Eine Sicherung irgendwo weiter oben
    // (etwa im Speichertakt) zaehlt nicht: Die koennte ein anderer Weg
    // ueberspringen. Eine Zeilenzahl waere hier das Mittel, nicht die
    // Zusicherung — sie brach, als der MQTT-Riegel dazwischenkam.
    let sperre = format!(
        "if {} != Some((touchdown_at, revision))",
        "last_published_rollout"
    );
    let letzte_sperre = davor
        .rfind(&sperre)
        .expect("die Sperre des Streamers steht nicht mehr vor dem Versand");
    assert!(
        letzte_sicherung > letzte_sperre,
        "die Sicherung liegt VOR der Sperre — also ausserhalb des Zweigs, \
         der sendet. Sie muss zwischen Sperre und Versand stehen."
    );
    // Und NACH der Praesenz-Pruefung, in deren `else`-Zweig: Ohne
    // Verbindung wurde sonst bei jedem Tick die ganze Flugdatei
    // geschrieben, ohne dass je etwas hinausging (N1-Zusatz). Seit Runde 3
    // steht die Sicherung VOR der MQTT-Sperre (blockierende E/A unter
    // einer async-Sperre), aber hinter `mqtt_vorhanden` — die Struktur
    // ist: Pruefung → `else {` → Sicherung → Sperre → Versand.
    let pruefung = davor
        .rfind("let mqtt_vorhanden =")
        .expect("keine Praesenz-Pruefung vor dem Versand");
    assert!(
        letzte_sicherung > pruefung,
        "die Sicherung liegt VOR der Praesenz-Pruefung — sie schreibt dann \
         auch, wenn nichts gesendet wird"
    );
    // Sie liegt im Block, den das `else` der Pruefung oeffnet — nicht im
    // `if !mqtt_vorhanden`-Zweig und nicht dahinter.
    let sonst = davor[pruefung..]
        .find("} else {")
        .map(|k| pruefung + k)
        .expect("die Pruefung hat keinen `else`-Zweig");
    let auf = sonst + davor[sonst..].find('{').expect("Block");
    let zu = blockende(&quelle, auf);
    assert!(
        letzte_sicherung > auf && letzte_sicherung < zu && stelle < zu,
        "Sicherung und Versand liegen nicht im selben `else`-Block der \
         Praesenz-Pruefung"
    );
}

/// ⚠ Wer die Bahn aendert, meldet den Nachtrag an.
///
/// Das Navdaten-Upgrade beim Einreichen hat genau das nicht getan: Es
/// ersetzte dieselben Bahnfelder wie das Nachholen, ohne Revision, ohne
/// Drittel, ohne Nachtrag. Der Recorder uebernimmt vom PIREP nur Punkte
/// und Noten — die Touchdown-Zeile blieb auf der alten Geometrie.
#[test]
fn das_navdaten_upgrade_geht_denselben_weg() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let kern = rumpf(&quelle, "fn bahn_upgrade_anwenden(");
    for (was, nadel) in [
        ("das Drittel nachgefuehrt", "drittel_nachfuehren(stats);"),
        ("die Revision erhoeht", "stats.bahn_revision ="),
        (
            "der Nachtrag angemeldet",
            "stats.bahn_nachtrag_offen = true;",
        ),
    ] {
        assert!(
            kern.contains(nadel),
            "beim Navdaten-Upgrade wird {was} nicht — dann bleibt die \
             Touchdown-Zeile beim Recorder auf der alten Geometrie."
        );
    }

    // Und der Einreich-Trichter schickt ihn auch wirklich.
    let trichter = rumpf(&quelle, "fn finalize_filed_pirep(");
    assert!(
        trichter.contains("bahn_nachtrag_bauen(flight, &stats)")
            && trichter.contains("nachtrag_queue::senden("),
        "`finalize_filed_pirep` schickt den offenen Bahn-Nachtrag nicht \
         — er ist die einzige Stelle, an der beim Einreichen noch eine \
         MQTT-Verbindung liegt."
    );
}

/// ⚠ Die Sperre des Streamers vergleicht die REVISION.
///
/// Bis zur dritten QS-Runde stand dort `korrelierter_szenerie_stand`.
/// Der aendert sich nur, wenn eine neue Szenerie-Lieferung eintrifft —
/// eine Zuordnung aus einem anderen Grund (das Navdaten-Upgrade beim
/// Einreichen) trug damit eine neue Revision auf der Leitung, ohne dass
/// die Sperre eine Kante sah. Der Nachtrag waere unterblieben.
///
/// ⚠ Das laesst sich nicht im Test fahren: Die Sperre lebt in einer
/// lokalen Variablen der Streamer-Schleife. Also wird der Quelltext
/// bewacht — sonst ist diese Zeile die einzige im ganzen Umbau, die man
/// folgenlos zurueckdrehen kann.
#[test]
fn die_sperre_des_streamers_vergleicht_die_revision() {
    let quelle: String = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"))
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    let feld = format!("stats.{}", "bahn_revision");
    let alt = format!("stats.{}", "korrelierter_szenerie_stand");

    // Der Wert, der in die Sperre wandert.
    let tupel = format!("{feld},stats.rollout_distance_m.unwrap_or(0.0),");
    assert!(
        quelle.contains(&tupel),
        "der Nachtrag traegt nicht die Revision in die Sperre — dann \
         entscheidet wieder der Szenerie-Stand, und eine Zuordnung ohne \
         neue Szenerie loest keinen Nachtrag aus"
    );
    assert!(
        !quelle.contains(&format!("{alt},stats.rollout_distance_m.unwrap_or(0.0),")),
        "die Sperre haengt wieder am Szenerie-Stand"
    );

    // Und die Sperre selbst vergleicht genau diesen Wert.
    assert!(
        quelle.contains("last_published_rollout!=Some((touchdown_at,revision))"),
        "die Sperre vergleicht etwas anderes als den mitgefuehrten Wert"
    );
}

/// ⚠ Das Drittel wird an GENAU EINER Stelle gesetzt — in
/// `drittel_nachfuehren`.
///
/// Die rohe Rechnung stand frueher in `step_flight_at`, NACH dem Aufruf
/// der Zuordnung. Wer sie dort wieder einbaut, gewinnt: Sie laeuft
/// spaeter und ueberschreibt die korrigierte Zahl — und der Waechter am
/// Kern der Zuordnung bleibt gruen, weil der Kern sie ja weiter ruft
/// (externe QS, 02.09.2026, P2-E). Deshalb wird die ZUWEISUNG bewacht,
/// nicht der Aufruf.
#[test]
fn das_drittel_wird_nur_an_einer_stelle_gesetzt() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let produktion = produktionsteil(&quelle);
    // ⚠ OHNE Bindungsnamen: Der Streamer bindet `st`, andere Stellen
    // `stats`. Eine Nadel mit `stats.` uebersaehe `st.landing_touchdown_zone =`
    // (externe QS, 02.09.2026, N11).
    let zuweisung = format!(".{} =", "landing_touchdown_zone");
    // ⚠ Zwei Zuweisungen sind erlaubt: die RECHNUNG in `drittel_nachfuehren`
    // und die WIEDERHERSTELLUNG in `apply_to` (`= self.landing_touchdown_zone`,
    // seit Runde 3 persistiert). Die zweite rechnet nichts — sie traegt den
    // gespeicherten Wert zurueck. Alles darueber hinaus ist eine zweite
    // Rechnung, die frueher oder spaeter laeuft und lautlos gewinnt.
    let wiederherstellung = format!(
        ".{} = self.{};",
        "landing_touchdown_zone", "landing_touchdown_zone"
    );
    let stellen = produktion.matches(&zuweisung).count();
    let wieder = produktion.matches(&wiederherstellung).count();
    assert_eq!(
        wieder, 1,
        "die Wiederherstellung aus der Flugdatei fehlt oder steht doppelt"
    );
    assert_eq!(
        stellen - wieder,
        1,
        "`landing_touchdown_zone` wird an {} Stellen GERECHNET — es gibt nur \
         eine Rechnung, `drittel_nachfuehren`.",
        stellen - wieder
    );
    let kern = rumpf(&quelle, "fn drittel_nachfuehren(");
    assert!(
        kern.contains(&zuweisung),
        "die eine Zuweisung liegt nicht in `drittel_nachfuehren`"
    );
    let tick = rumpf(&quelle, "fn step_flight_at(");
    assert!(
        !tick.contains(&zuweisung),
        "`step_flight_at` setzt das Drittel wieder selbst — nach der \
         Zuordnung, also gewinnt die rohe Rechnung"
    );
}

/// ⚠ Sperre und Fahne fallen NUR nach tatsaechlichem Versand — und ohne
/// den restlichen Tick abzuwuergen.
///
/// Erste Fassung (P2-H): `if let Some(handle)` sendete, Sperre und Fahne
/// fielen dahinter auch ohne Versand. Zweite Fassung: `let … else {
/// continue }` — und das `continue` landete in der Hauptschleife des
/// Streamers: kein Heartbeat, keine Positionen, keine Flugdatei mehr,
/// sobald MQTT fehlt (externe QS, 02.09.2026, N1, P0).
///
/// Die Zusicherung ist STRUKTUR: Versand, Sperre und Fahne liegen im
/// selben `Some(handle) =>`-Arm, und in dem Zweig steht kein `continue`.
#[test]
fn die_sperre_faellt_erst_nach_dem_versand() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let versand = format!("{}::{}(", "nachtrag_queue", "senden");
    let stelle = quelle.rfind(&versand).expect("Streamer-Versand");

    // Der Arm, in dem der Versand steht.
    let arm = quelle[..stelle]
        .rfind("Some(handle) =>")
        .expect("der Versand steht in keinem `Some(handle) =>`-Arm");
    let auf = arm + quelle[arm..].find('{').expect("Arm ohne Block");
    let zu = blockende(&quelle, auf);
    assert!(
        stelle > auf && stelle < zu,
        "der Versand liegt nicht im Arm"
    );

    // Sperre und Fahne: nach dem Versand, VOR dem Ende des Arms.
    for (was, nadel) in [
        (
            "die Sperre",
            "last_published_rollout = Some((touchdown_at, revision));",
        ),
        ("die Fahne", "st.bahn_nachtrag_offen = false;"),
    ] {
        let pos = quelle[stelle..].find(nadel).map(|k| stelle + k);
        assert!(
            matches!(pos, Some(p) if p < zu),
            "{was} faellt nicht im Versand-Arm — dann faellt sie auch ohne \
             Versand, oder gar nicht"
        );
    }

    // Und kein `continue` zwischen der Sperr-Pruefung und dem Arm-Ende.
    let pruefung = quelle[..stelle]
        .rfind("if last_published_rollout != Some((touchdown_at, revision))")
        .expect("Sperr-Pruefung");
    assert!(
        !quelle[pruefung..zu].contains("continue"),
        "ein `continue` im Nachtrags-Zweig wuergt den restlichen Tick ab — \
         Heartbeat, Positionen, Flugdatei"
    );
}

/// Das Ende des Blocks, der bei `auf` (einem `{`) beginnt.
fn blockende(text: &str, auf: usize) -> usize {
    let mut tiefe = 0i32;
    for (k, c) in text[auf..].char_indices() {
        match c {
            '{' => tiefe += 1,
            '}' => {
                tiefe -= 1;
                if tiefe == 0 {
                    return auf + k;
                }
            }
            _ => {}
        }
    }
    text.len()
}

/// ⚠ Beim Einreichen faellt die Fahne erst, wenn es einen Handle gibt.
///
/// Dieselbe Regel wie im Streamer, an der Stelle, die die Abhilfe selbst
/// neu angelegt hatte — und dort zuerst verletzt (externe QS, 02.09.2026,
/// N4): Die Fahne fiel beim Bauen, der Handle wurde erst danach geprueft,
/// und ohne MQTT ging die Korrektur ohne Logzeile verloren.
///
/// ⚠ Keine String-Inhalte pruefen: `nur_code` leert Zeichenketten.
/// Die Zusicherung ist Struktur — die Fahne faellt im `(true, true)`-Arm,
/// der `(true, false)`-Arm meldet ueber `tracing::warn!`.
#[test]
fn beim_einreichen_faellt_die_fahne_erst_mit_handle() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let trichter = rumpf(&quelle, "fn finalize_filed_pirep(");
    let arm_ok = trichter
        .find("(true, true) =>")
        .expect("die Fahne haengt nicht mehr an `(offen, handle)`");
    let auf = arm_ok + trichter[arm_ok..].find('{').expect("Arm ohne Block");
    let zu = blockende(trichter, auf);
    let fahne = trichter
        .find("stats.bahn_nachtrag_offen = false;")
        .expect("der Trichter loescht die Fahne nicht mehr");
    assert!(
        fahne > auf && fahne < zu,
        "die Fahne faellt nicht im `(true, true)`-Arm — also auch ohne Handle"
    );
    let arm_ohne = trichter
        .find("(true, false) =>")
        .expect("kein `(true, false)`-Arm — ohne Handle wird nicht gemeldet");
    let auf2 = arm_ohne + trichter[arm_ohne..].find('{').expect("Arm ohne Block");
    let zu2 = blockende(trichter, auf2);
    assert!(
        trichter[auf2..zu2].contains("tracing::warn!"),
        "ohne Handle wird nicht gemeldet — die Korrektur geht lautlos verloren"
    );
}

/// ⚠ Der Spur-Block entsteht an GENAU EINER Stelle — `spur_block` — und
/// beide Sender rufen sie.
///
/// Runde 5 (N24): Streamer und Einreich-Nachtrag entschieden getrennt,
/// ob die Spur mitgeht; der Streamer haette eine VERALTETE Spur (Achs-
/// wechsel nach dem Ausrollen) weiter gesendet. Eine Entscheidung, zwei
/// Aufrufer — sonst laufen sie wieder auseinander, wie die Herkunft in
/// v1.7.14.
#[test]
fn der_spur_block_entsteht_nur_einmal() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let produktion = produktionsteil(&quelle);
    let bau = format!("{}(&self, final_: bool)", "fn wire");
    assert_eq!(
        produktion.matches(&bau).count(),
        1,
        "`BahnFelder::wire` ist umgezogen — die Zaehlung darunter zeigt ins Leere"
    );
    // `.wire(true)` ausserhalb von `spur_block` waere ein zweiter Bauplatz.
    let kern = rumpf(&produktion, "fn spur_block(");
    assert!(
        kern.contains(".wire(true)"),
        "`spur_block` baut den Block nicht mehr"
    );
    let insgesamt = produktion.matches(".wire(true)").count();
    assert_eq!(
        insgesamt, 1,
        "`.wire(true)` steht {insgesamt}-mal in der Produktion — ein zweiter \
         Bauplatz neben `spur_block` entscheidet dann wieder selbst, ob eine \
         veraltete Spur mitgeht"
    );
    assert_eq!(
        produktion.matches("spur_block(&flight, &stats)").count()
            + produktion.matches("spur_block(flight, stats)").count(),
        2,
        "Streamer und Einreich-Nachtrag rufen `spur_block` nicht beide"
    );
}

/// ⚠ Der Nachtrag aus der Warteschlange wird ueber `aus_json` gelesen —
/// nicht ueber `from_value`.
///
/// `#[serde(flatten)] Option<BahnWire>` liest einen fehlenden Block als
/// `Some(default)`: `rollout_final: false`, lauter `null`. Auf dem
/// Warteschlangen-Weg holte das N13 zurueck (Runde 5, N23). `aus_json`
/// stellt `None` wieder her; jede Lesestelle muss sie nehmen.
#[test]
fn der_gequeute_nachtrag_kommt_ueber_aus_json() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let produktion = produktionsteil(&quelle);
    let typ = "TouchdownRolloutFinalizedPayload";
    assert_eq!(
        produktion.matches(&format!("{typ}::aus_json(")).count(),
        3,
        "nicht alle Lesestellen (Einreich-Trichter, Worker-Altbestand, Ablage) nehmen `aus_json`"
    );
    // ⚠ Beide Schreibweisen: der Turbofish UND die Typannotation
    // (`let n: …Payload = serde_json::from_value(json)`) — genau die stand
    // vor der Abhilfe im Worker und rutschte am Turbofish-Muster vorbei
    // (Runde 6, Befund 6). Ueber den leerraumfreien Text, damit ein
    // Umbruch nichts verdeckt.
    let dicht: String = produktion.chars().filter(|c| !c.is_whitespace()).collect();
    for muster in [
        format!("from_value::<aeroacars_mqtt::{typ}>"),
        format!("from_value::<{typ}>"),
        format!("{typ}=serde_json::from_value("),
        format!("{typ}=from_value("),
        format!("Option<{typ}>=serde_json::from_value("),
        format!("Option<aeroacars_mqtt::{typ}>=serde_json::from_value("),
        format!("from_str::<aeroacars_mqtt::{typ}>"),
        format!("{typ}=serde_json::from_str("),
    ] {
        assert_eq!(
            dicht.matches(&muster).count(),
            0,
            "ein rohes `from_value` auf den Nachtrag (`{muster}`) — der fehlende \
             Spur-Block wird dann als leerer Block gesendet"
        );
    }
}

/// ⚠ Ohne Handle geht der Nachtrag in die Ablage — und die Fahne faellt
/// erst, wenn er dort LIEGT.
///
/// Codex (03.09.2026, P1): Der `(true, false)`-Arm meldete nur. Der Flug
/// ist nach dem Einreichen geloescht, die PIREP-Warteschlange entfernt
/// ihre Datei vor dem Senden — nichts konnte die Korrektur spaeter noch
/// schicken. Jetzt: `nachtrag_queue::enqueue`, und `bahn_nachtrag_offen
/// = false` steht NACH dem Aufruf (im Ok-Zweig), nicht davor.
#[test]
fn ohne_handle_geht_der_nachtrag_in_die_ablage() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let trichter = rumpf(&quelle, "fn finalize_filed_pirep(");
    let arm = trichter
        .find("(true, false) =>")
        .expect("kein `(true, false)`-Arm mehr");
    let auf = arm + trichter[arm..].find('{').expect("Arm ohne Block");
    let zu = blockende(trichter, auf);
    let block = &trichter[auf..zu];
    let ablage = block
        .find("nachtrag_queue::enqueue_offline(")
        .expect("ohne Handle landet der Nachtrag nicht in der Ablage — er geht verloren");
    let fahne = block
        .find("stats.bahn_nachtrag_offen = false;")
        .expect("die Fahne faellt im Ablage-Arm nicht — der Nachtrag wird beim naechsten Weg nochmal gebaut");
    assert!(
        fahne > ablage,
        "die Fahne faellt VOR dem Einreihen — schlaegt die Ablage fehl, ist die Korrektur weg"
    );
    // Der Worker-Altbestand (gequeuter Nachtrag ohne Handle) ebenso.
    let rest = &trichter[..auf];
    let _ = rest;
    assert!(
        trichter.matches("nachtrag_queue::enqueue_offline(").count() >= 2,
        "der gequeute Nachtrag ohne Handle geht nicht in die Ablage"
    );
}

/// ⚠ Im Fehlerzweig von `flight_end` faellt die Fahne erst NACH
/// `pirep_queue::enqueue`.
///
/// Codex (03.09.2026, P2): Sie fiel beim Bauen. Schlug das Einreihen fehl,
/// stellte `restore_flight_for_retry` den Flug wieder her — ohne offene
/// Korrektur, die beim naechsten Versuch nicht mehr gebaut wurde.
#[test]
fn im_fehlerzweig_faellt_die_fahne_erst_nach_dem_einreihen() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let ende = rumpf(&quelle, "async fn flight_end(");
    let einreihen = ende
        .find("pirep_queue::enqueue(&app, &queued)")
        .expect("`flight_end` reiht nicht mehr ueber `pirep_queue::enqueue(&app, &queued)` ein");
    let fahnen: Vec<usize> = ende
        .match_indices("bahn_nachtrag_offen = false")
        .map(|(i, _)| i)
        .collect();
    assert!(
        !fahnen.is_empty(),
        "`flight_end` loescht die Fahne nirgends mehr"
    );
    for f in fahnen {
        assert!(
            f > einreihen,
            "die Fahne faellt VOR `pirep_queue::enqueue` — bei einem Einreih-Fehler ist die Korrektur weg"
        );
    }
}

/// ⚠ Der Warteschlangen-Worker leert die Ablage, sobald ein Handle da ist
/// — VOR dem Client-Riegel, denn ein Nachtrag braucht nur MQTT.
#[test]
fn der_worker_leert_die_ablage_vor_dem_client_riegel() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let worker = rumpf(&quelle, "fn spawn_pirep_queue_worker(");
    let drain = worker
        .find("nachtrag_queue::drain(")
        .expect("der Worker leert die Ablage nicht — Nachtraege ohne Handle bleiben liegen");
    let riegel = worker
        .find("let Some(client) = client_opt else")
        .expect("Client-Riegel nicht gefunden — wurde er umgebaut?");
    assert!(
        drain < riegel,
        "die Ablage wird erst NACH dem Client-Riegel geleert — ohne phpVMS-Login bleibt sie liegen"
    );
}

/// ⚠ JEDER Nachtrag geht ueber `nachtrag_queue::senden` — Ablage zuerst,
/// Datei faellt erst mit der Zustellmeldung.
///
/// Codex (03.09.2026, Runde 12, P1): Die Runde-11-Ablage griff nur ohne
/// Handle. Mit Handle ging der Nachtrag fire-and-forget raus, und der
/// Aufrufer las „Handle vorhanden" als „zugestellt". Jetzt gibt es keinen
/// Sendeweg mehr, der an der Ablage vorbeifuehrt: Der einzige Aufruf von
/// `.senden(` auf dem Sender liegt im Modul selbst, und dort steht die
/// Ablage VOR dem Senden und das Loeschen NACH dem `.await` der Meldung.
#[test]
fn jeder_nachtrag_geht_ueber_die_ablage() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let produktion = produktionsteil(&quelle);
    assert_eq!(
        produktion.matches(".touchdown_rollout_finalized(").count(),
        0,
        "ein fire-and-forget-Sendeweg am Handle — er umgeht die Ablage"
    );
    let modul = rumpf(&produktion, "mod nachtrag_queue {");
    let im_modul = modul.matches(".senden(").count();
    let gesamt = produktion.matches(".senden(").count();
    assert!(im_modul >= 2, "das Modul sendet nicht (senden + drain)");
    assert_eq!(
        gesamt, im_modul,
        "ein `.senden(` ausserhalb von `nachtrag_queue` — dort gibt es keine Ablage davor"
    );
    assert!(
        produktion.matches("nachtrag_queue::senden(").count() >= 4,
        "nicht alle vier Aufrufer (Streamer, Einreichen, gequeut, Altbestand) gehen ueber `senden`"
    );
    // Ablage VOR dem Senden, Loeschen NACH der Meldung.
    let senden = rumpf(modul, "pub fn senden(");
    let ablage = senden.find("enqueue(").expect("`senden` legt nicht ab");
    let schicken = senden.find(".senden(").expect("`senden` schickt nicht");
    assert!(
        ablage < schicken,
        "erst senden, dann ablegen — bei totem Kanal ist nichts auf der Platte"
    );
    let warten = senden
        .find(".await")
        .expect("`senden` wartet nicht auf die Meldung");
    let loeschen = senden.find("entfernen(").expect("`senden` raeumt nie auf");
    assert!(loeschen > warten, "die Datei faellt vor der Zustellmeldung");
    // Nur EINE Stelle loescht.
    assert_eq!(
        modul.matches("remove_file(").count(),
        1,
        "mehr als eine Loeschstelle — eine davon geht an `entfernen` vorbei"
    );
    let entfernen = rumpf(modul, "pub fn entfernen(");
    assert!(
        entfernen.contains("remove_file("),
        "die Loeschstelle liegt nicht in `entfernen`"
    );
}

/// ⚠ Der Publisher meldet die Zustellung erst nach Leitungspruefung und
/// Publish — und es gibt keinen unbestaetigten Weg mehr.
#[test]
fn der_publisher_meldet_nur_bei_stehender_leitung() {
    let quelle =
        nur_code(&fs::read_to_string("crates/aeroacars-mqtt/src/lib.rs").expect("mqtt lib.rs"));
    let produktion = produktionsteil(&quelle);
    assert_eq!(
        produktion
            .matches("pub fn touchdown_rollout_finalized(")
            .count(),
        0,
        "der fire-and-forget-Weg am Handle existiert wieder"
    );
    let arm = produktion
        .find("Cmd::TouchdownRolloutFinalized(p, ack) =>")
        .expect("der Publisher-Zweig traegt keine Zustellmeldung");
    let auf = arm + produktion[arm..].find('{').expect("Zweig ohne Block");
    let zu = blockende(&produktion, auf);
    let block = &produktion[auf..zu];
    let leitung = block
        .find("link_rx.borrow()")
        .expect("der Zweig prueft die Leitung nicht");
    let publish = block
        .find("publish_json_bestaetigt(")
        .expect("der Zweig nimmt den unbestaetigten Publish");
    assert!(leitung < publish, "Publish vor der Leitungspruefung");
    // Seit Runde 13 wandert `ack` INS Zustellbuch (`Some(ack)`) und meldet
    // erst mit dem PUBACK; der Zweig selbst meldet nur noch das `false`
    // bei liegender Leitung.
    assert!(
        block.contains("Some(ack)"),
        "die Meldung geht nicht ins Zustellbuch — `true` kaeme ohne PUBACK"
    );
    assert_eq!(
        block.matches("ack.send(true)").count(),
        0,
        "der Publisher meldet `true` selbst — das ist Kanalannahme, kein PUBACK"
    );
}

/// ⚠ Zustellung heisst PUBACK (Runde 13, High 1).
///
/// Jeder Publish geht durch `publish_registriert` (Schloss + Eintrag ins
/// Zustellbuch), der Drive-Loop traegt `Outgoing::Publish` und `PubAck`
/// ein, und bei jedem Leitungsabriss wird das Buch bereinigt. Fehlt eine
/// der vier Stellen, ist das Buch versetzt und bestaetigt fremde Pakete.
#[test]
fn die_zustellung_ist_das_puback() {
    let quelle =
        nur_code(&fs::read_to_string("crates/aeroacars-mqtt/src/lib.rs").expect("mqtt lib.rs"));
    let produktion = produktionsteil(&quelle);
    assert_eq!(
        produktion.matches(".publish(").count(),
        0,
        "ein `publish().await` am Client — der geht am Zustellbuch vorbei"
    );
    let registriert = rumpf(&produktion, "fn publish_registriert(");
    let wartend = rumpf(&produktion, "async fn publish_registriert_wartend(");
    let im_weg =
        registriert.matches(".try_publish(").count() + wartend.matches(".try_publish(").count();
    assert_eq!(
        produktion.matches(".try_publish(").count(),
        im_weg,
        "ein `try_publish` ausserhalb von `publish_registriert*` — ohne Eintrag ins Buch"
    );
    assert_eq!(im_weg, 2, "die beiden registrierten Wege fehlen");
    let ok_arm = registriert.find("Ok(()) =>").expect("kein Ok-Arm");
    let eintrag = registriert
        .find("registrieren(")
        .expect("`publish_registriert` traegt nicht ein");
    assert!(
        eintrag > ok_arm,
        "Eintrag vor der Annahme — ein abgelehnter Publish steht im Buch"
    );
    assert_eq!(
        registriert.matches("registrieren(").count(),
        1,
        "mehr als ein Eintrag je Publish"
    );
    assert!(
        produktion.contains("Outgoing::Publish(pkid)))")
            && produktion.contains(".ausgegangen(pkid)"),
        "der Drive-Loop traegt `Outgoing::Publish` nicht ein"
    );
    assert!(
        produktion.contains("Packet::PubAck(ack)))")
            && produktion.contains(".bestaetigt(ack.pkid)"),
        "der Drive-Loop traegt das PUBACK nicht ein"
    );
    assert!(
        produktion.matches("zustellbuch_leitung_weg(").count() >= 3,
        "nicht beide Abriss-Stellen (Watchdog, Poll-Fehler) bereinigen das Buch"
    );
}

/// ⚠ Ohne Ablage kein Senden; Sperre und Fahne fallen nur bei Erfolg
/// (Runde 13, High 2). Loeschen nur bei gelesener UND gleicher Revision
/// (High 3).
#[test]
fn ohne_ablage_wird_nicht_gesendet_und_die_sperre_bleibt() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let produktion = produktionsteil(&quelle);
    let senden = rumpf(&produktion, "pub fn senden(");
    let ablage = senden.find("enqueue(").expect("`senden` legt nicht ab");
    let schicken = senden.find(".senden(").expect("`senden` schickt nicht");
    let abbruch = senden
        .find("return Err(")
        .expect("`senden` bricht bei fehlgeschlagener Ablage nicht ab");
    assert!(
        ablage < abbruch && abbruch < schicken,
        "der Abbruch liegt nicht zwischen Ablage und Senden"
    );
    let versand = produktion
        .rfind("nachtrag_queue::senden(")
        .expect("Streamer-Versand");
    let arm = produktion[..versand].rfind("Some(handle) =>").expect("Arm");
    let auf = arm + produktion[arm..].find('{').expect("Block");
    let zu = blockende(&produktion, auf);
    let block = &produktion[auf..zu];
    let sperre = "last_published_rollout = Some((touchdown_at, revision));";
    assert_eq!(
        block.matches(sperre).count(),
        1,
        "die Sperre steht mehrfach — eine davon ohne Erfolg"
    );
    let ok_arm = block
        .find("Ok(()) =>")
        .expect("der Streamer wertet das Ergebnis von `senden` nicht aus");
    assert!(
        block.find(sperre).expect("Sperre") > ok_arm,
        "die Sperre faellt vor dem Ok-Arm"
    );
    // High 2 (Runde 14): Die Revision steht im NAMEN — das PUBACK loescht
    // genau die gesendete Datei. Kein Lesen-dann-Loeschen mehr, also auch
    // kein Fenster, in dem der Streamer eine neuere Datei unterschiebt.
    // ⚠ Die Nadel ist das FORMAT, nicht das Wort „revision": Ein
    // `let _ = revision;` haette den alten Test gruen gelassen (bei der
    // Gegenprobe zu Runde 14 aufgefallen). Weil `nur_code` Zeichenketten
    // leert, wird die Rohquelle gelesen — nur fuer diese eine Nadel.
    let roh = fs::read_to_string("src/lib.rs").expect("lib.rs");
    let name_roh = rumpf(&roh, "pub fn dateiname(");
    assert!(
        name_roh.contains("-r{"),
        "der Dateiname traegt die Revision nicht — das Lese-dann-Loesche-Fenster ist zurueck"
    );
    let name = rumpf(&produktion, "pub fn dateiname(");
    assert!(
        name.contains("revision"),
        "`dateiname` nimmt die Revision nicht mehr entgegen"
    );
    let entfernen = rumpf(&produktion, "pub fn entfernen(");
    assert!(
        !entfernen.contains("read_to_string("),
        "`entfernen` liest vor dem Loeschen — genau das Fenster, das die Revision im Namen schliesst"
    );
}

/// ⚠ Einreichen: die Ablage liegt VOR der Fahne und VOR dem Einreichen
/// (Runde 14, High 1).
///
/// Der Flug ist nach dem Einreichen weg; einen naechsten Tick gibt es
/// nicht. Also faellt die Fahne im `(true, true)`-Arm erst nach `enqueue`,
/// und `flight_end` legt den offenen Nachtrag ab, bevor es den Flugbericht
/// einreicht.
#[test]
fn beim_einreichen_liegt_die_ablage_vor_fahne_und_einreichen() {
    let quelle = nur_code(&fs::read_to_string("src/lib.rs").expect("lib.rs"));
    let trichter = rumpf(&quelle, "fn finalize_filed_pirep(");
    let arm = trichter
        .find("(true, true) =>")
        .expect("kein `(true, true)`-Arm");
    let auf = arm + trichter[arm..].find('{').expect("Arm ohne Block");
    let zu = blockende(trichter, auf);
    let block = &trichter[auf..zu];
    let ablage = block
        .find("nachtrag_queue::enqueue(")
        .expect("der Arm legt nicht ab, bevor die Fahne faellt");
    let fahne = block
        .find("stats.bahn_nachtrag_offen = false;")
        .expect("die Fahne faellt im Arm nicht");
    assert!(
        ablage < fahne,
        "die Fahne faellt vor der Ablage — bei voller Platte ist die Korrektur weg"
    );
    let ende = rumpf(&quelle, "async fn flight_end(");
    let einreichen = ende
        .find("file_pirep_with_retry(")
        .expect("`flight_end` reicht nicht mehr ueber `file_pirep_with_retry` ein");
    assert!(
        ende[..einreichen].contains("nachtrag_queue::enqueue_offline("),
        "`flight_end` legt den offenen Nachtrag nicht VOR dem Einreichen ab"
    );
}

/// ⚠ Die Bereinigung traegt gepufferte Ereignisse EIN, bevor sie
/// abschreibt (Runde 14, High 3).
///
/// rumqttc legt `Outgoing::Publish(pkid)` in `state.events`, bevor es
/// schreibt. Ein Schreibfehler laesst es dort liegen; kaeme es nach dem
/// Reconnect zurueck, saehe das Buch einen frischen Eintrag als unterwegs,
/// und ein spaeteres PUBACK derselben pkid traefe den Falschen.
#[test]
fn die_bereinigung_leert_die_ereignisschlange() {
    let quelle =
        nur_code(&fs::read_to_string("crates/aeroacars-mqtt/src/lib.rs").expect("mqtt lib.rs"));
    let produktion = produktionsteil(&quelle);
    let f = rumpf(&produktion, "fn zustellbuch_leitung_weg(");
    let drain = f
        .find("state.events.drain(")
        .expect("die Bereinigung leert `state.events` nicht — das Geist-Ereignis bleibt");
    let abschreiben = f
        .find("verbindung_weg(")
        .expect("die Bereinigung schreibt nichts ab");
    assert!(
        drain < abschreiben,
        "erst abschreiben, dann die Ereignisse eintragen — der Geist bleibt im Buch"
    );
    assert!(
        f.contains(".ausgegangen(pkid)"),
        "das gepufferte `Outgoing::Publish` wird nicht eingetragen"
    );
    assert!(
        f.contains("pending.clear()"),
        "`pending` bleibt stehen — ein zweiter Abriss zaehlt dieselben Auftraege nochmal"
    );
}
