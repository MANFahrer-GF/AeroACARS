//! Die Bahngeometrie aus der Simulator-Szenerie in die Navdaten ziehen.
//!
//! # Warum überhaupt
//!
//! Die Landebewertung misst den Abstand zur Mittellinie. Wo die liegt,
//! kam bisher nur aus den Navigationsdaten. Das ist der **echte**
//! Flughafen; der Pilot fliegt aber die **Szenerie**.
//!
//! Am 28.08.2026 gegen die installierte X-Plane-Szenerie gemessen, über
//! 70.452 Bahnen, die in beiden Quellen stehen:
//!
//! ```text
//! Median der Abweichung        0,03°
//! ab 3° daneben            3.653 Bahnen  (63 % davon Platzhalter-Kurse)
//! Breite ab 5 m daneben    7.279 Bahnen
//! schlimmster Fall           180°  — Bahn 17 mit Kurs 0,00° geführt
//! ```
//!
//! # Warum ERGÄNZEN und nicht ERSETZEN
//!
//! ⚠ Die Navdaten tragen mehr als Geometrie: ILS, Gleitwinkel,
//! Schwellenüberflughöhe. Die speisen die Anflugbewertung. Würde der
//! ganze Flughafen durch die Szenerie ersetzt, fielen sie weg — die
//! `apt.dat` kennt sie in dieser Form nicht.
//!
//! Deshalb bleibt der Flughafen aus den Navdaten die Grundlage, und nur
//! die **geometrischen** Felder werden überschrieben: Kurs, Breite,
//! Länge, Schwellenkoordinaten, versetzte Schwelle, Belag.
//!
//! # Warum eine Bahn trotz gleichem Bezeichner nicht dieselbe sein muss
//!
//! Bahnen werden umbenannt, wenn die Missweisung wandert. Eine „09" in
//! den Navdaten kann in der Szenerie eine andere Bahn desselben Platzes
//! sein — bei Parallelbahnen liegen sie hunderte Meter auseinander.
//! Deshalb entscheidet nicht der Bezeichner über die Identität, sondern
//! die LAGE der Schwelle. Der Kurs darf das nicht mitentscheiden — siehe
//! die Notiz zum entfernten Kurs-Riegel weiter unten.

use aeroacars_mqtt::navdata::{NavAirport, NavRunway};
use sim_xplane::szenerie::{SzenerieBahn, SzenerieFlughafen};

/// Wie weit die Szenerie-Bahn QUER zur Navdaten-Achse liegen darf,
/// damit es dieselbe Bahn sein kann.
///
/// Hundert Meter sind grosszügig für Vermessungsunterschiede und zu eng
/// für eine Nachbarbahn: Parallelbahnen liegen nach ICAO mindestens
/// 210 m auseinander, in der Praxis meist deutlich mehr.
///
/// ⚠ Bis v1.7.12 stand hier ein Punkt-zu-Punkt-Abstand der Schwellen mit
/// 200 m. Der ist bei MSFS auf drei Annahmen gebaut (Mitte, Kurs, Laenge
/// — die Schwellen werden daraus gerechnet) und schlug am 30./31.08.2026
/// an DREI Plaetzen fehl (EDDF 25L, YBWW 12, YBCG 14), ohne zu sagen,
/// welche der drei nicht stimmte. Der Querabstand haengt nur an der
/// Mitte; die Begruendung steht bei `querabstand_m`.
const QUERABSTAND_HOECHST_M: f64 = 100.0;

/// Wie weit die Mitte der Szenerie-Bahn LÄNGS der Achse von der Mitte
/// der Navdaten-Bahn abliegen darf.
///
/// Vierhundert Meter: Der grösste Versatz, der bei derselben Bahn
/// entstehen kann, kommt von einer versetzten Schwelle — sie verschiebt
/// den Navdaten-Mittelpunkt um die HÄLFTE des Versatzes. Der grösste im
/// Bestand gemessene Versatz sind 573 m (TJPS 12), also 287 m. Eine Bahn
/// am anderen Ende liegt dagegen mindestens eine halbe Bahnlänge weit
/// weg — bei der kürzesten Piste im Bestand 550 m.
const LAENGSVERSATZ_HOECHST_M: f64 = 400.0;

// ⚠ Hier stand einmal ein Riegel auf die Kursabweichung (45°).
//
// Er war zirkulär: Er benutzte den Kurs, den wir gerade als kaputt
// erkannt haben, als Kriterium dafür, ob wir ihn reparieren dürfen.
//
// An BISL nachgemessen (28.08.2026): Bahn 15 steht bei uns mit Kurs
// 0,0°, Bahn 33 mit 360,0° — bei beiden fehlt er schlicht. Die Szenerie
// sagt 135,37° und 315,37°. Der Riegel liess die 33 korrigieren
// (44,6° Unterschied) und verwarf die 15 (135,4°) — **denselben Defekt,
// zwei verschiedene Antworten**, und ausgerechnet der schlimmere Fall
// blieb stehen.
//
// Was die Identität einer Bahn wirklich entscheidet, ist die LAGE.
// Liegt die Schwelle der Szenerie am selben Ort wie unsere, ist es
// dieselbe Bahn — dann darf der Kurs beliebig weit korrigiert werden.
// Liegt sie am anderen Ende (Umbenennung, Parallelbahn), ist es eine
// andere, und dann wird nichts übernommen.

/// Was bei der Übernahme geschah — für den Bericht und die Messung.
///
/// Serialisierbar seit v1.7.15 (Runde 4): Der Bericht ist Eingabe der
/// Bahn-Herkunft auf der Leitung. Ohne Persistenz kippte
/// `bahn_geometrie_quelle` nach einem Neustart von „szenerie" auf
/// „navdaten", und die Korrekturbeträge wurden als `null` gelöscht.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UebernahmeBericht {
    /// Bahnen, deren Geometrie aus der Szenerie kommt.
    pub uebernommen: Vec<String>,
    /// Bahnen, bei denen die Szenerie nichts Passendes hatte.
    pub ohne_treffer: Vec<String>,
    /// Bahnen, bei denen der Bezeichner passte, die Lage aber nicht —
    /// der verdächtigste Fall, deshalb getrennt geführt.
    pub verworfen: Vec<String>,
    /// Grösste Kursabweichung, die übernommen wurde, in Grad.
    pub groesste_kursabweichung_grad: f64,
    /// Grösste Breitenabweichung, die übernommen wurde, in Metern.
    pub groesste_breitenabweichung_m: f64,
    /// Grösste Abweichung der versetzten Schwelle, in Metern.
    ///
    /// ⚠ Der folgenreichste der drei Werte: Die versetzte Schwelle ist
    /// der Nullpunkt der Aufsetzpunkt-Bewertung. Sagt die Szenerie
    /// "keine Schwelle", wo die Navdaten 573 m führen (TJPS 12,
    /// LAN273), verschiebt sich die Bewertung um eine halbe
    /// Bahnlänge — das darf nicht still geschehen.
    pub groesste_schwellenabweichung_m: f64,
}

fn winkelabstand(a: f64, b: f64) -> f64 {
    let d = ((a - b) % 360.0 + 540.0) % 360.0 - 180.0;
    d.abs()
}

fn abstand_m(a: (f64, f64), b: (f64, f64)) -> f64 {
    const R: f64 = 6_371_000.0;
    let (p1, p2) = (a.0.to_radians(), b.0.to_radians());
    let dp = p2 - p1;
    let dl = (b.1 - a.1).to_radians();
    let h = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
}

/// Bezeichner vergleichbar machen: `"09L"`, `"9L"`, `"09l"` sind dasselbe.
fn normiert(b: &str) -> String {
    let t = b.trim().to_ascii_uppercase();
    let ziffern: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest: String = t.chars().skip_while(|c| c.is_ascii_digit()).collect();
    match ziffern.parse::<u32>() {
        Ok(n) => format!("{n:02}{rest}"),
        Err(_) => t,
    }
}

/// Wie weit ein Punkt vom MITTELPUNKT der Strecke `a`–`b` abliegt,
/// zerlegt in (längs der Achse, quer zur Achse), in Metern.
///
/// # Warum quer und nicht der Abstand zu einem Punkt
///
/// Die Zuordnung stand bis v1.7.12 auf einer Kette von drei Annahmen:
/// Die Szenerie liefert bei MSFS nur MITTE, KURS und LÄNGE — die beiden
/// Schwellen rechnen wir daraus. Stimmt eine der drei nicht, wandern die
/// gerechneten Schwellen, und ein Punkt-zu-Punkt-Vergleich findet nichts
/// mehr. Welche der drei es war, sieht man dabei nicht.
///
/// Der Querabstand hängt nur an der MITTE — der einzigen Angabe, die
/// direkt geliefert wird:
///
/// * Ein falscher **Kurs** dreht die gerechneten Enden um die Mitte. Die
///   Mitte bleibt, wo sie ist.
/// * Eine falsche **Länge** schiebt die Enden nach aussen. Die Mitte
///   bleibt, wo sie ist.
/// * Eine **versetzte Schwelle** verschiebt den Navdaten-Nullpunkt längs
///   der Bahn. Quer zur Achse ändert das nichts.
///
/// Und er trennt trotzdem sauber: Parallelbahnen liegen nach ICAO
/// mindestens 210 m auseinander.
///
/// ⚠ Der LÄNGS-Anteil wird trotzdem gebraucht — sonst passt eine Bahn am
/// ANDEREN ENDE derselben Achse, die quer ja genau null abliegt. Genau
/// das hält `andere_bahn_am_anderen_ende_wird_verworfen` fest (FACT 19,
/// EDHE): Eine gleich benannte Bahn an anderer Stelle darf die
/// Anflugrichtung nicht stillschweigend umdefinieren.
///
/// ⚠ Ebene Näherung. Auf Bahnlänge (Kilometer) ist der Fehler
/// millimetergross; für Entfernungen jenseits weniger Kilometer taugt
/// sie nicht.
fn achsenversatz_m(a: (f64, f64), b: (f64, f64), p: (f64, f64)) -> (f64, f64) {
    const M_JE_GRAD_BREITE: f64 = 111_320.0;
    let cos_breite = a.0.to_radians().cos();
    let punkt = |q: (f64, f64)| {
        (
            (q.1 - a.1) * M_JE_GRAD_BREITE * cos_breite,
            (q.0 - a.0) * M_JE_GRAD_BREITE,
        )
    };
    let (bx, by) = punkt(b);
    let (px, py) = punkt(p);
    let laenge = (bx * bx + by * by).sqrt();
    if !laenge.is_finite() || laenge < 1.0 {
        // Entartete Achse — dann bleibt nur der Punktabstand, und der
        // zaehlt als laengs, damit er nicht durch die Quer-Pruefung faellt.
        return (abstand_m(a, p), 0.0);
    }
    // Einheitsvektor der Achse.
    let (ex, ey) = (bx / laenge, by / laenge);
    // Vom MITTELPUNKT der Navdaten-Bahn aus messen, nicht von der
    // Schwelle: Ein Schwellenversatz verschiebt die Schwelle, nicht die
    // Mitte.
    let (mx, my) = (bx / 2.0, by / 2.0);
    let (dx, dy) = (px - mx, py - my);
    let laengs = dx * ex + dy * ey;
    let quer = dx * (-ey) + dy * ex;
    (laengs.abs(), quer.abs())
}

/// Die passende Bahn der Szenerie zu einer Navdaten-Bahn finden.
///
/// `None` heisst: nichts Passendes — dann bleibt die Navdaten-Geometrie
/// stehen. Im Zweifel die alte Quelle, nicht die neue.
fn passende_szenerie_bahn<'a>(
    nav: &NavRunway,
    sz: &'a SzenerieFlughafen,
) -> (Option<&'a SzenerieBahn>, bool) {
    let ziel = normiert(&nav.designator);
    let mut bezeichner_passte = false;
    for b in &sz.bahnen {
        if normiert(&b.bezeichner) != ziel {
            continue;
        }
        bezeichner_passte = true;
        // Die MITTE der Szenerie-Bahn — der Mittelpunkt beider Enden.
        // Bei MSFS ist das genau die gelieferte Koordinate: `bahn_paar`
        // rechnet beide Enden symmetrisch um sie herum, ein falscher
        // Kurs oder eine falsche Laenge drehen bzw. schieben die Enden,
        // lassen den Mittelpunkt aber unberuehrt.
        let mitte = (
            (b.schwelle.0 + b.gegenende.0) / 2.0,
            (b.schwelle.1 + b.gegenende.1) / 2.0,
        );
        let (laengs, quer) = achsenversatz_m(
            (nav.threshold.lat, nav.threshold.lon),
            (nav.far_end.lat, nav.far_end.lon),
            mitte,
        );
        if quer > QUERABSTAND_HOECHST_M || laengs > LAENGSVERSATZ_HOECHST_M {
            continue;
        }
        return (Some(b), true);
    }
    (None, bezeichner_passte)
}

/// Belagsschlüssel der `apt.dat` in die Schreibweise der Navdaten.
/// Belagsschlüssel der X-Plane-`apt.dat` in unseren Wortschatz.
///
/// `None` heißt: **die Szenerie weiß es nicht** — dann bleibt der bisher
/// bekannte Belag stehen, statt durch Unwissen ersetzt zu werden.
///
/// ⚠ Die Schlüssel 12–15 standen hier nach der **MSFS**-Aufzählung, obwohl
/// `belag_code` X-Plane-Semantik trägt (der MSFS-Adapter rechnet auf
/// dieselben Schlüssel um). Zwei davon waren um eins verschoben, und der
/// folgenschwerste war **15**: In X-Plane heißt das **transparent** — die
/// Bahn wird von der Szenerie selbst gezeichnet, sehr verbreitet in
/// Zusatzszenerien. Daraus wurde „SNOW", das gilt als unbefestigt, und
/// damit fiel die gesamte Queransicht weg und die seitliche Bewertung
/// wurde übersprungen.
///
/// Gefunden an DAH411 (HKJK 06, A330, 29.08.2026): eine Asphaltbahn in
/// Nairobi kam als unbefestigt an, `bahn_geometrie_quelle = szenerie`.
///
/// Quelle: X-Plane `apt.dat`-Spezifikation, Zeilentyp 100, Feld 3.
fn belag_text(code: u8) -> Option<&'static str> {
    match code {
        1 => Some("ASPH"),
        2 => Some("CONC"),
        3 => Some("TURF"),
        4 => Some("DIRT"),
        5 => Some("GRVL"),
        // Trockener Seeboden — fest, aber nicht befestigt.
        12 => Some("CLAY"),
        13 => Some("WATER"),
        14 => Some("SNOW"),
        // 15 = transparent: KEINE Aussage über den Belag.
        _ => None,
    }
}

/// Prüfungen, die JEDER Wert aus der Szenerie bestehen muss, bevor er
/// einen Navdaten-Wert ersetzen darf.
///
/// # Warum das eine eigene Ebene ist
///
/// Bis v1.7.11 schrieb die Übernahme sieben Felder **bedingungslos** über:
/// Kurs, Länge, Breite, versetzte Schwelle, beide Schwellenkoordinaten und
/// den Belag. Jedes davon kann in einer Szenerie fehlen, null oder Unsinn
/// sein — und dann kostete die Übernahme der Geometrie einen guten Wert.
///
/// Aufgefallen ist das dreimal hintereinander an je EINEM Feld, und
/// dreimal habe ich dieses eine Feld repariert:
///   * v1.7.8  — Kurs-Riegel war zirkulär (entfernt)
///   * v1.7.11 — Belag: X-Plane-Schlüssel 15 heißt „transparent", nicht
///               „Schnee"; eine Asphaltbahn kam als unbefestigt an
///               (DAH411, HKJK 06) und die ganze Queransicht verschwand
///
/// Das Muster ist nicht der Belag, sondern die **bedingungslose
/// Übernahme**. Deshalb gibt es hier für jeden Wertebereich genau eine
/// Prüfung, und die Übernahme kommt an keinem Feld daran vorbei — der
/// Test `kein_feld_wird_ungeprueft_uebernommen` hält das fest.
///
/// `None` heißt immer dasselbe: **die Szenerie weiß es nicht** → der
/// bisherige Wert bleibt stehen.
mod plausibel {
    /// Eine Strecke in Metern: endlich und größer als null.
    pub fn strecke_m(v: f64) -> Option<f64> {
        (v.is_finite() && v > 0.0).then_some(v)
    }

    /// Ein Kurs in Grad: endlich und im Bereich [0, 360).
    ///
    /// ⚠ Genau 0,0 ist HIER gültig — anders als in den Navdaten.
    ///
    /// Ein erster Entwurf hat es abgelehnt, weil 0,0 der Platzhalter ist,
    /// den 3.836 Bahnen im Bestand tragen. Das war die Platzhalter-Logik
    /// der NAVDATEN, auf die Szenerie übertragen, wo sie nicht hingehört:
    ///
    ///   * X-Plane **rechnet** den Kurs aus den beiden Schwellen-
    ///     koordinaten (`kurs_grad(s1, s2)`). Dort ist 0,0 eine echte,
    ///     nach Norden zeigende Bahn — sie abzulehnen hiesse, sie beim
    ///     kaputten Navdaten-Wert zu belassen.
    ///   * MSFS leitet umgekehrt die Schwellen AUS dem Kurs ab. Den Kurs
    ///     zu verwerfen und die daraus abgeleiteten Koordinaten zu
    ///     uebernehmen waere in sich widerspruechlich.
    ///
    /// 360,0 wird abgelehnt, weil es kein gueltiger Wert im Bereich ist —
    /// wer ihn liefert, meint 0 und hat nicht normalisiert.
    pub fn kurs_grad(v: f64) -> Option<f64> {
        (v.is_finite() && (0.0..360.0).contains(&v)).then_some(v)
    }

    /// Eine versetzte Schwelle: endlich und nicht negativ. Null ist hier
    /// ein gültiger Wert — die meisten Bahnen haben keine.
    pub fn versatz_m(v: f64) -> Option<f64> {
        (v.is_finite() && v >= 0.0).then_some(v)
    }

    /// Ein Koordinatenpaar (Breite, Länge).
    ///
    /// ⚠ (0, 0) wird abgelehnt: Das ist kein Flughafen, sondern ein
    /// nicht gefülltes Feld. Der Punkt liegt im Atlantik vor Ghana.
    pub fn koordinate(p: (f64, f64)) -> Option<(f64, f64)> {
        let (lat, lon) = p;
        let gueltig = lat.is_finite()
            && lon.is_finite()
            && (-90.0..=90.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon)
            && !(lat == 0.0 && lon == 0.0);
        gueltig.then_some(p)
    }
}

/// Die Geometrie aus der Szenerie übernehmen.
///
/// Gibt den ergänzten Flughafen und einen Bericht zurück. Ist die
/// Szenerie leer oder passt nichts, kommt der Flughafen unverändert
/// zurück — der Rückfall ist immer der bisherige Stand.
pub fn uebernimm_szenerie(
    nav: &NavAirport,
    sz: &SzenerieFlughafen,
    quelle: Quelle,
) -> (NavAirport, UebernahmeBericht) {
    // ⚠ Was uebernommen werden darf, haengt an der QUELLE.
    //
    // X-Plane meldet beide Schwellen als Koordinaten; die `apt.dat`
    // dokumentiert sie als rechtweisend. Da ist alles belastbar.
    //
    // MSFS meldet nur MITTE, KURS und LAENGE — die Schwellen rechnen WIR
    // daraus. Und die Facility-Doku sagt zu `HEADING` nur „the runway
    // heading, in degrees"; ob missweisend oder rechtweisend, steht
    // nirgends. (Die Runway-XML-Doku nennt True North, aber das ist ein
    // anderes Format und beweist nichts ueber die Facility-API.)
    //
    // Seit v1.7.13 verhindert eine Kursabweichung die Zuordnung NICHT
    // mehr — das war noetig, damit ueberhaupt zugeordnet wird. Genau
    // dadurch wuerde ein falscher Kurs aber jetzt UEBERNOMMEN, und aus
    // „keine Bewertung" wuerde eine falsche. Deshalb gilt fuer MSFS:
    //
    //   uebernommen  — Laenge, Breite, versetzte Schwelle, Belag
    //                  (direkt gemeldet, bezugsfrei)
    //   NICHT        — Kurs und beide Schwellenkoordinaten
    //                  (aus dem Kurs gerechnet, Bezug unbewiesen)
    //
    // Die Abweichung wird trotzdem gemessen und mitgeschrieben. Sobald
    // der Bestand zeigt, dass sie der oertlichen Missweisung entspricht
    // (oder eben nicht), ist die Frage entschieden — dann kann der
    // Riegel fallen.
    let achse_belastbar = quelle == Quelle::XPlaneDatei;
    let mut aus = nav.clone();
    let mut b = UebernahmeBericht::default();

    for bahn in &mut aus.runways {
        let (treffer, bezeichner_passte) = passende_szenerie_bahn(bahn, sz);
        let Some(s) = treffer else {
            if bezeichner_passte {
                b.verworfen.push(bahn.designator.clone());
            } else {
                b.ohne_treffer.push(bahn.designator.clone());
            }
            continue;
        };

        let kurs_ab = winkelabstand(bahn.true_course, s.kurs_grad);
        if kurs_ab > b.groesste_kursabweichung_grad {
            b.groesste_kursabweichung_grad = kurs_ab;
        }
        if let Some(w) = bahn.width_ft {
            let breit_ab = (w as f64 * 0.3048 - s.breite_m).abs();
            if breit_ab > b.groesste_breitenabweichung_m {
                b.groesste_breitenabweichung_m = breit_ab;
            }
        }

        // ⚠ Nur Geometrie. ILS, Gleitwinkel und Schwellenhöhe bleiben,
        // wo sie sind — die kennt die `apt.dat` nicht.
        // ⚠ JEDES Feld geht durch `plausibel::`. Kein Wert aus der
        // Szenerie ersetzt einen Navdaten-Wert, ohne geprueft zu sein —
        // siehe die Begruendung am Modul.
        if achse_belastbar {
            if let Some(k) = plausibel::kurs_grad(s.kurs_grad) {
                bahn.true_course = k;
            }
        }
        if let Some(l) = plausibel::strecke_m(s.laenge_m) {
            bahn.length_ft = (l / 0.3048).round() as i32;
        }
        if let Some(w) = plausibel::strecke_m(s.breite_m) {
            bahn.width_ft = Some((w / 0.3048).round() as i32);
        }
        if let Some(v) = plausibel::versatz_m(s.versetzte_schwelle_m) {
            let vorher_m = bahn.displaced_threshold_ft as f64 * 0.3048;
            let abweichung = (v - vorher_m).abs();
            if abweichung > b.groesste_schwellenabweichung_m {
                b.groesste_schwellenabweichung_m = abweichung;
            }
            bahn.displaced_threshold_ft = (v / 0.3048).round() as i32;
        }
        if achse_belastbar {
            if let Some((lat, lon)) = plausibel::koordinate(s.schwelle) {
                bahn.threshold.lat = lat;
                bahn.threshold.lon = lon;
            }
            if let Some((lat, lon)) = plausibel::koordinate(s.gegenende) {
                bahn.far_end.lat = lat;
                bahn.far_end.lon = lon;
            }
        }
        // Der Belag ist der EINE Wert, den die Szenerie nicht bestimmt.
        //
        // # Warum hier anders entschieden wird als bei der Geometrie
        //
        // Die Geometrie kommt aus der Szenerie, weil der Pilot darauf
        // rollt. Der Belag ist etwas anderes: In einer Szenerie ist er
        // oft ein **Darstellungshinweis** — deshalb gibt es den
        // Schlüssel „transparent" überhaupt, mit dem die Szenerie sagt
        // „ich male die Oberfläche selbst". In den Navdaten ist er eine
        // Tatsache über den echten Platz.
        //
        // ⚠ Und die Folgen sind unsymmetrisch: Ein falscher Belag löscht
        // die **gesamte** seitliche Bewertung samt Queransicht, weil eine
        // unbefestigte Bahn nicht seitlich bewertet wird. Ein Wert, der
        // so viel kostet, darf nicht aus der schwächeren Quelle kommen.
        //
        // Gefunden an DAH411 (HKJK 06, Nairobi, 29.08.2026): Die Navdaten
        // sagen `ASP`, die Szenerie überschrieb es mit etwas
        // Unbefestigtem, und die Queransicht verschwand ersatzlos.
        //
        // Also: Die Szenerie füllt nur eine LÜCKE. Weiss sie es nicht
        // (transparent, unbekannter Schlüssel), bleibt es ohnehin beim
        // bisherigen Wert.
        let navdaten_kennen_belag = bahn
            .surface
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty());
        if !navdaten_kennen_belag {
            if let Some(text) = belag_text(s.belag_code) {
                bahn.surface = Some(text.to_string());
            }
        }
        b.uebernommen.push(bahn.designator.clone());
    }

    (aus, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aeroacars_mqtt::navdata::NavPoint;

    fn punkt(lat: f64, lon: f64) -> NavPoint {
        NavPoint {
            lat,
            lon,
            elev_ft: None,
        }
    }

    /// EDHE 09, wie es am 28.08.2026 in unseren Navdaten stand.
    pub(super) fn edhe_nav() -> NavAirport {
        NavAirport {
            cycle: "2608".into(),
            valid_to: "2026-09-24".into(),
            icao: "EDHE".into(),
            name: "Uetersen".into(),
            latitude: 53.6459,
            longitude: 9.7042,
            elevation_ft: Some(21),
            runways: vec![NavRunway {
                designator: "09".into(),
                magnetic_course: 87.0,
                true_course: 89.9957383300858,
                length_ft: 3609,
                width_ft: Some(131),
                surface: Some("ASPH".into()),
                threshold: punkt(53.6459, 9.6942),
                far_end: punkt(53.6459, 9.7142),
                displaced_threshold_ft: 0,
                ils: None,
                glideslope_angle: 3.0,
                tch_ft: 50,
            }],
        }
    }

    fn szenerie(
        bezeichner: &str,
        kurs: f64,
        breite: f64,
        schwelle: (f64, f64),
    ) -> SzenerieFlughafen {
        SzenerieFlughafen {
            staende: Vec::new(),
            icao: "EDHE".into(),
            quelle: "Test".into(),
            rollwege: vec![],
            bahnen: vec![SzenerieBahn {
                bezeichner: bezeichner.into(),
                kurs_grad: kurs,
                breite_m: breite,
                laenge_m: 1100.0,
                versetzte_schwelle_m: 0.0,
                schwelle,
                gegenende: (schwelle.0, schwelle.1 + 0.02),
                belag_code: 1,
            }],
        }
    }

    #[test]
    fn korrigiert_kurs_und_breite() {
        // Der echte Fall: unsere Navdaten fuehren 89,996 Grad (ein aus
        // 87,0 magnetisch abgeleiteter Platzhalter), die Szenerie 93,72.
        let nav = edhe_nav();
        let sz = szenerie("09", 93.72, 55.0, (53.6459, 9.6942));
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.uebernommen, vec!["09"]);
        assert!((aus.runways[0].true_course - 93.72).abs() < 0.001);
        assert_eq!(aus.runways[0].width_ft, Some(180)); // 55 m
        assert!(
            (b.groesste_kursabweichung_grad - 3.724).abs() < 0.01,
            "gemeldete Abweichung {}",
            b.groesste_kursabweichung_grad
        );
    }

    #[test]
    fn ils_und_gleitwinkel_bleiben_erhalten() {
        // ⚠ Die `apt.dat` kennt sie nicht. Wuerde der Flughafen ersetzt
        // statt ergaenzt, fiele die Anflugbewertung aus.
        let mut nav = edhe_nav();
        nav.runways[0].glideslope_angle = 3.2;
        let sz = szenerie("09", 93.72, 55.0, (53.6459, 9.6942));
        let (aus, _) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert!((aus.runways[0].glideslope_angle - 3.2).abs() < 1e-9);
        assert_eq!(aus.runways[0].magnetic_course, 87.0);
    }

    #[test]
    fn gleicher_bezeichner_aber_woanders_wird_verworfen() {
        // Parallelbahnen und umbenannte Bahnen: Der Bezeichner allein
        // reicht nicht. Fuenf Kilometer entfernt ist es eine andere Bahn.
        let nav = edhe_nav();
        let sz = szenerie("09", 93.72, 55.0, (53.7000, 9.6942));
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.verworfen, vec!["09"]);
        assert!(b.uebernommen.is_empty());
        assert!(
            (aus.runways[0].true_course - 89.9957).abs() < 0.001,
            "unveraendert"
        );
    }

    #[test]
    fn ein_voellig_falscher_kurs_wird_an_derselben_stelle_korrigiert() {
        // Der Fall BISL: Bahn 15 mit Kurs 0,0 gefuehrt, in Wahrheit
        // 135,37. Die Schwelle steht an derselben Stelle — es IST
        // dieselbe Bahn, nur ohne Kurs. Genau die gehoert korrigiert.
        //
        // Ein Riegel auf die Kursabweichung haette hier abgelehnt und
        // damit ausgerechnet den schlimmsten Fall stehen lassen.
        let nav = edhe_nav();
        let sz = szenerie("09", 224.99, 55.0, (53.6459, 9.6942));
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.uebernommen, vec!["09"]);
        assert!((aus.runways[0].true_course - 224.99).abs() < 0.001);
    }

    /// Eine Szenerie-Bahn so bauen, wie MSFS sie liefert: aus MITTE,
    /// KURS und LÄNGE werden beide Enden gerechnet.
    ///
    /// ⚠ Genau diese Ableitung ist die Schwachstelle, um die es geht.
    /// Stimmt der Kurs nicht, wandern beide Enden — die Mitte bleibt.
    fn szenerie_aus_mitte(
        bezeichner: &str,
        mitte: (f64, f64),
        kurs: f64,
        laenge_m: f64,
    ) -> SzenerieFlughafen {
        const M_JE_GRAD: f64 = 111_320.0;
        let cos_b = mitte.0.to_radians().cos();
        let halb = laenge_m / 2.0;
        let (sin_k, cos_k) = kurs.to_radians().sin_cos();
        let versatz = |vorzeichen: f64| {
            (
                mitte.0 + vorzeichen * halb * cos_k / M_JE_GRAD,
                mitte.1 + vorzeichen * halb * sin_k / (M_JE_GRAD * cos_b),
            )
        };
        SzenerieFlughafen {
            staende: Vec::new(),
            icao: "EDHE".into(),
            quelle: "Test".into(),
            rollwege: vec![],
            bahnen: vec![SzenerieBahn {
                bezeichner: bezeichner.into(),
                kurs_grad: kurs,
                breite_m: 55.0,
                laenge_m,
                versetzte_schwelle_m: 0.0,
                schwelle: versatz(-1.0),
                gegenende: versatz(1.0),
                belag_code: 1,
            }],
        }
    }

    /// Ein falscher KURS darf die Zuordnung nicht mehr verhindern.
    ///
    /// ⚠ Der Fall, an dem v1.7.12 im echten MSFS gescheitert ist. Die
    /// Doku sagt zu `HEADING` nur „the runway heading, in degrees" — ob
    /// missweisend oder rechtweisend, steht nirgends. In Australien
    /// sind das über 10 Grad Unterschied.
    ///
    /// Frueher wurden daraus die Schwellen gerechnet und Punkt gegen
    /// Punkt verglichen: Bei YBWW lagen sie 262 m auseinander, die alte
    /// Grenze war 200 m — kein Treffer, und nicht erkennbar warum.
    /// Der Mittelpunkt dreht sich nicht mit.
    #[test]
    fn ein_falscher_kurs_verhindert_die_zuordnung_nicht() {
        let nav = edhe_nav();
        let mitte = (53.6459, 9.7042);
        let echt = nav.runways[0].true_course;

        for abweichung in [0.0_f64, 3.0, -3.0, 11.0, -11.0] {
            let sz = szenerie_aus_mitte("09", mitte, echt + abweichung, 1100.0);
            let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
            assert_eq!(
                b.uebernommen,
                vec!["09"],
                "bei {abweichung} Grad Kursabweichung wurde nicht zugeordnet"
            );
        }
    }

    /// Eine falsche LÄNGE ebenso wenig.
    ///
    /// Die Enden wandern nach aussen, die Mitte bleibt.
    #[test]
    fn eine_falsche_laenge_verhindert_die_zuordnung_nicht() {
        let nav = edhe_nav();
        let mitte = (53.6459, 9.7042);
        let echt = nav.runways[0].true_course;
        for laenge in [1100.0_f64, 800.0, 1600.0] {
            let sz = szenerie_aus_mitte("09", mitte, echt, laenge);
            let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
            assert_eq!(
                b.uebernommen,
                vec!["09"],
                "bei {laenge} m Laenge kein Treffer"
            );
        }
    }

    /// Aber eine Bahn, die WIRKLICH woanders liegt, wird weiter verworfen.
    ///
    /// ⚠ Der Gegenpol zu den beiden Tests darueber: Die neue Regel darf
    /// nicht alles durchlassen. Quer trennt Parallelbahnen, laengs
    /// trennt das andere Ende.
    #[test]
    fn eine_wirklich_andere_lage_wird_weiter_verworfen() {
        let nav = edhe_nav();
        let echt = nav.runways[0].true_course;

        // 300 m quer versetzt — eine Parallelbahn.
        let quer = (53.6459 + 300.0 / 111_320.0, 9.7042);
        let sz = szenerie_aus_mitte("09", quer, echt, 1100.0);
        let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(
            b.verworfen,
            vec!["09"],
            "eine Parallelbahn wurde uebernommen"
        );

        // 600 m laengs versetzt — das andere Ende.
        let laengs = (
            53.6459,
            9.7042 + 600.0 / (111_320.0 * 53.6459_f64.to_radians().cos()),
        );
        let sz = szenerie_aus_mitte("09", laengs, echt, 1100.0);
        let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.verworfen, vec!["09"], "das andere Ende wurde uebernommen");
    }

    /// Der Kurs wird NUR aus einer belastbaren Quelle uebernommen.
    ///
    /// ⚠ Seit v1.7.13 verhindert eine Kursabweichung die Zuordnung nicht
    /// mehr — das war noetig, damit im MSFS ueberhaupt zugeordnet wird.
    /// Genau dadurch koennte ein falscher Kurs jetzt UEBERNOMMEN werden,
    /// und aus „keine Bewertung" wuerde eine falsche.
    ///
    /// X-Plane liefert beide Schwellen als Koordinaten und ist als
    /// rechtweisend dokumentiert. MSFS liefert nur Mitte, Kurs und
    /// Laenge — und die Facility-Doku sagt zum Kursbezug nichts.
    #[test]
    fn der_kurs_kommt_nur_aus_einer_belastbaren_quelle() {
        let nav = edhe_nav();
        let alt = nav.runways[0].true_course;
        let sz = szenerie("09", 93.72, 55.0, (53.6459, 9.6942));

        // X-Plane: uebernehmen — genau dafuer ist die Funktion gebaut.
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.uebernommen, vec!["09"]);
        assert!((aus.runways[0].true_course - 93.72).abs() < 0.001);

        // MSFS: zuordnen ja, Kurs nein.
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::MsfsFacility);
        assert_eq!(b.uebernommen, vec!["09"], "MSFS wurde nicht zugeordnet");
        assert!(
            (aus.runways[0].true_course - alt).abs() < 0.001,
            "MSFS-Kurs wurde uebernommen, obwohl sein Bezug unbewiesen ist"
        );
        // Die Abweichung wird trotzdem gemessen.
        assert!(b.groesste_kursabweichung_grad > 3.0);
    }

    /// Auch die SCHWELLEN kommen bei MSFS nicht aus der Szenerie.
    ///
    /// ⚠ Sie werden dort aus Mitte, Kurs und Laenge gerechnet — ein
    /// falscher Kursbezug verschiebt sie mit. Was direkt gemeldet wird
    /// (Breite, Laenge, versetzte Schwelle, Belag), gilt weiterhin.
    #[test]
    fn die_schwellen_kommen_bei_msfs_nicht_aus_der_szenerie() {
        let nav = edhe_nav();
        let alt_lat = nav.runways[0].threshold.lat;
        let alt_lon = nav.runways[0].threshold.lon;
        // Eine Szenerie-Bahn, deren Schwelle 80 m daneben liegt.
        let sz = szenerie("09", 93.72, 55.0, (53.6459 + 80.0 / 111_320.0, 9.6942));

        let (aus, _) = uebernimm_szenerie(&nav, &sz, Quelle::MsfsFacility);
        assert!((aus.runways[0].threshold.lat - alt_lat).abs() < 1e-9);
        assert!((aus.runways[0].threshold.lon - alt_lon).abs() < 1e-9);
        // Die Breite dagegen schon.
        assert_eq!(
            aus.runways[0].width_ft,
            Some((55.0_f64 / 0.3048).round() as i32)
        );
    }

    #[test]
    fn andere_bahn_am_anderen_ende_wird_verworfen() {
        // Umbenennung oder Parallelbahn: gleicher Bezeichner, andere
        // Lage. Hier darf NICHTS uebernommen werden — sonst wuerde die
        // Anflugrichtung stillschweigend umdefiniert.
        //
        // 600 m entfernt: die Laenge einer kleinen Bahn, also genau der
        // Abstand zwischen den beiden Enden derselben Piste.
        let nav = edhe_nav();
        let sz = szenerie("09", 269.99, 55.0, (53.6459, 9.7033));
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.verworfen, vec!["09"]);
        assert!((aus.runways[0].true_course - 89.9957).abs() < 0.001);
    }

    #[test]
    fn bezeichner_werden_normiert_verglichen() {
        let mut nav = edhe_nav();
        nav.runways[0].designator = "9".into();
        let sz = szenerie("09", 93.72, 55.0, (53.6459, 9.6942));
        let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.uebernommen, vec!["9"]);
    }

    #[test]
    fn ohne_treffer_bleibt_alles_wie_es_war() {
        let nav = edhe_nav();
        let sz = szenerie("27", 273.72, 55.0, (53.6459, 9.7142));
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.ohne_treffer, vec!["09"]);
        assert!((aus.runways[0].true_course - 89.9957).abs() < 0.001);
        assert_eq!(aus.runways[0].width_ft, Some(131));
    }

    #[test]
    fn leere_szenerie_aendert_nichts() {
        let nav = edhe_nav();
        let leer = SzenerieFlughafen {
            icao: "EDHE".into(),
            ..Default::default()
        };
        let (aus, b) = uebernimm_szenerie(&nav, &leer, Quelle::XPlaneDatei);
        assert!(b.uebernommen.is_empty());
        assert_eq!(aus.runways[0].true_course, nav.runways[0].true_course);
    }
}

/// Nur fuer den Korpus-Lauf: eine  aus dem Textauszug bauen.
///
/// Steht hier und nicht im Test, weil  sonst von aussen nicht
/// vollstaendig konstruierbar waere — und ein zweiter Bauweg waere ein
/// zweiter Ort, an dem Felder vergessen werden koennen.
pub fn test_navairport(icao: &str, zeilen: &[Vec<String>]) -> NavAirport {
    let z = |s: &String| s.parse::<f64>().unwrap_or(0.0);
    NavAirport {
        cycle: String::new(),
        valid_to: String::new(),
        icao: icao.to_string(),
        name: String::new(),
        latitude: 0.0,
        longitude: 0.0,
        elevation_ft: None,
        runways: zeilen
            .iter()
            .map(|t| NavRunway {
                designator: t[1].clone(),
                magnetic_course: z(&t[3]),
                true_course: z(&t[2]),
                length_ft: z(&t[5]) as i32,
                width_ft: if t[4].is_empty() {
                    None
                } else {
                    Some(z(&t[4]) as i32)
                },
                surface: None,
                threshold: aeroacars_mqtt::navdata::NavPoint {
                    lat: z(&t[6]),
                    lon: z(&t[7]),
                    elev_ft: None,
                },
                far_end: aeroacars_mqtt::navdata::NavPoint {
                    lat: z(&t[8]),
                    lon: z(&t[9]),
                    elev_ft: None,
                },
                displaced_threshold_ft: 0,
                ils: None,
                glideslope_angle: 3.0,
                tch_ft: 50,
            })
            .collect(),
    }
}

// ─────────────────────────────────────────────────────────────────────
// Anschluss an den Flug
// ─────────────────────────────────────────────────────────────────────

use std::sync::{Mutex, OnceLock};

/// Das Verzeichnis der installierten Szenerie, einmal gebaut.
///
/// Der Aufbau kostet rund eine halbe Sekunde ueber 380 MB. Pro Landung
/// waere das absurd, pro Programmlauf ist es nichts — und wenn der Pilot
/// zwischendurch ein Add-on installiert, faellt das ueber Groesse und
/// Aenderungszeit der Quelldateien auf, und es wird neu gebaut.
static VERZEICHNIS: OnceLock<Mutex<Option<sim_xplane::szenerie::SzenerieIndex>>> = OnceLock::new();

/// Das Szenerie-Verzeichnis bereitstellen, falls es fehlt oder veraltet ist.
///
/// # Warum das eine eigene Funktion ist
///
/// ⚠ Der Bau liest die X-Plane-Installation des Piloten
/// (`~/Library/Preferences/x-plane_install_12.txt` nennt den Ort, dann
/// `Earth nav data/apt.dat` und die Custom-Scenery). Liegt X-Plane auf
/// dem Schreibtisch, in Dokumenten, Downloads, iCloud oder auf einem
/// externen Laufwerk, fragt **macOS beim ersten Zugriff um Erlaubnis**.
///
/// Bis v1.7.12 geschah dieser erste Zugriff faul — und zwar in der
/// Landungs-Korrelation. Der Dialog kam damit im Aufsetzmoment
/// (Pilotenmeldung, 31.08.2026). Dasselbe gilt fuer die Bauzeit: Sie
/// fiel mitten in den Anflug.
///
/// Deshalb ist das Bereitstellen jetzt von der Abfrage getrennt und
/// wird beim Flugbeginn angestossen (`szenerie_vorbereiten`). Am Gate
/// darf ein Dialog erscheinen; auf 500 Fuss nicht.
fn verzeichnis_bereitstellen() -> bool {
    let zelle = VERZEICHNIS.get_or_init(|| Mutex::new(None));
    let Ok(mut halter) = zelle.lock() else {
        return false;
    };
    let neu_bauen = match halter.as_ref() {
        Some(idx) => !idx.gueltig(),
        None => true,
    };
    if !neu_bauen {
        return true;
    }
    let Some(wurzel) = sim_xplane::szenerie::installationen().into_iter().next() else {
        return false;
    };
    let t = std::time::Instant::now();
    let idx = sim_xplane::szenerie::SzenerieIndex::bauen(&wurzel);
    tracing::info!(
        flughaefen = idx.anzahl(),
        dauer_ms = t.elapsed().as_millis(),
        "Szenerie-Verzeichnis gebaut"
    );
    *halter = Some(idx);
    true
}

/// Das Verzeichnis im Voraus bauen — beim PROGRAMMSTART.
///
/// # Warum beim Start und nicht beim Flugbeginn
///
/// ⚠ Auf macOS fragt das System beim ersten Zugriff auf die
/// X-Plane-Installation um Erlaubnis, wenn diese an einem geschuetzten
/// Ort liegt (Schreibtisch, Dokumente, Downloads, iCloud, externes
/// Laufwerk). Die Erlaubnis wird EINMAL erteilt und bleibt — aber der
/// Zeitpunkt der Frage ist der erste Zugriff.
///
/// Bis v1.7.12 war das die Landungs-Korrelation: Der Dialog erschien im
/// Aufsetzmoment (Pilotenmeldung 31.08.2026). Beim Flugbeginn waere es
/// besser, beim Programmstart ist es richtig — da sitzt der Pilot noch
/// am Schreibtisch und kein Flug haengt daran.
///
/// Der Simulator spielt dabei keine Rolle: Wer X-Plane installiert hat,
/// braucht das Verzeichnis frueher oder spaeter. Ist keines da, kostet
/// der Versuch nichts und die Funktion schweigt.
///
/// Laeuft in einem eigenen Faden — der Bau dauert je nach Umfang der
/// installierten Szenerie mehrere Sekunden.
pub fn szenerie_vorbereiten() {
    std::thread::spawn(|| {
        if !verzeichnis_bereitstellen() {
            tracing::info!("keine X-Plane-Installation gefunden — Szenerie bleibt aus");
        }
    });
}

/// Den Flughafen aus der Szenerie holen, mit Verzeichnis.
fn szenerie_flughafen(icao: &str) -> Option<sim_xplane::szenerie::SzenerieFlughafen> {
    // ⚠ Baut das Verzeichnis notfalls hier — der Vorabbau beim
    // Flugbeginn ist die Regel, nicht die Garantie: Ein Flug, der aus
    // einer Wiederaufnahme kommt, oder ein Simulatorwechsel mitten im
    // Flug haetten ihn sonst nicht gesehen.
    verzeichnis_bereitstellen();
    let zelle = VERZEICHNIS.get_or_init(|| Mutex::new(None));
    let halter = zelle.lock().ok()?;
    halter.as_ref()?.flughafen(icao)
}

/// Woher die Szenerie fuer diesen Simulator kommt.
///
/// ⚠ Die Quellen sind NICHT austauschbar. Die `apt.dat` beschreibt die
/// X-Plane-Welt; wer MSFS fliegt, hat eine andere Szenerie, und sie
/// dort zu benutzen waere schlimmer als gar keine Korrektur — sie saehe
/// plausibel aus und waere falsch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quelle {
    /// Installierte `apt.dat`, vom Client selbst gelesen.
    XPlaneDatei,
    /// SimConnect-Facility-Auskunft, vorher angefordert.
    MsfsFacility,
    /// Kein Weg — es bleibt bei den Navdaten.
    Keine,
}

pub fn quelle_fuer(simulator: sim_core::Simulator) -> Quelle {
    match simulator {
        sim_core::Simulator::XPlane11 | sim_core::Simulator::XPlane12 => Quelle::XPlaneDatei,
        sim_core::Simulator::Msfs2020 | sim_core::Simulator::Msfs2024 => Quelle::MsfsFacility,
        _ => Quelle::Keine,
    }
}

/// Alt-Name, damit vorhandene Aufrufe weiterlaufen.
pub fn gilt_fuer(simulator: sim_core::Simulator) -> bool {
    quelle_fuer(simulator) != Quelle::Keine
}

/// Der Anschluss: Navdaten mit der Szenerie ergaenzen, wenn beides passt.
///
/// Gibt den (moeglicherweise ergaenzten) Flughafen und den Bericht
/// zurueck. Passiert nichts, ist der Bericht leer und der Flughafen
/// unveraendert — der Rueckfall ist immer der bisherige Stand.
pub fn ergaenze_aus_szenerie(
    simulator: sim_core::Simulator,
    icao: &str,
    nav: Option<NavAirport>,
    msfs_auskunft: Option<sim_core::szenerie::SzenerieFlughafen>,
) -> (
    Option<NavAirport>,
    Option<UebernahmeBericht>,
    Option<sim_core::szenerie::SzenerieFlughafen>,
) {
    let quelle = quelle_fuer(simulator);
    if quelle == Quelle::Keine {
        return (nav, None, None);
    }
    let Some(nav) = nav else {
        // Ohne Navdaten gibt es nichts zu ergaenzen. Einen Flughafen
        // ALLEIN aus der Szenerie zu bauen waere moeglich, aber dann
        // fehlten ILS, Gleitwinkel und Schwellenhoehe — und die Anzeige
        // haette stillschweigend weniger als vorher.
        return (None, None, None);
    };
    let auskunft = match quelle {
        Quelle::XPlaneDatei => szenerie_flughafen(icao),
        // ⚠ Nur nehmen, wenn sie zu DIESEM Platz gehoert. Nach einem
        // Divert liegt sonst die Auskunft des geplanten Ziels da —
        // plausible Zahlen, falscher Flughafen.
        Quelle::MsfsFacility => {
            msfs_auskunft.filter(|a| a.icao.eq_ignore_ascii_case(icao) && !a.bahnen.is_empty())
        }
        Quelle::Keine => None,
    };
    let Some(sz) = auskunft else {
        return (Some(nav), None, None);
    };
    let (ergaenzt, bericht) = uebernimm_szenerie(&nav, &sz, quelle);
    if bericht.uebernommen.is_empty() {
        // ⚠ Die Szenerie wird TROTZDEM zurueckgegeben: Auch wenn keine
        // Bahn uebernommen wurde, koennen ihre Rollwege die Ausfahrten
        // speisen. Sie hier fallen zu lassen waere ein stiller Verlust
        // — die Bahn ist nur ein Teil der Auskunft.
        return (Some(nav), Some(bericht), Some(sz));
    }
    tracing::info!(
        icao,
        uebernommen = bericht.uebernommen.len(),
        verworfen = bericht.verworfen.len(),
        kurs_grad = bericht.groesste_kursabweichung_grad,
        breite_m = bericht.groesste_breitenabweichung_m,
        schwelle_m = bericht.groesste_schwellenabweichung_m,
        quelle = %sz.quelle,
        "Bahngeometrie aus der Szenerie uebernommen"
    );
    (Some(ergaenzt), Some(bericht), Some(sz))
}

#[cfg(test)]
mod anschluss_tests {
    use super::*;
    use sim_core::Simulator;

    fn nav_edhe() -> NavAirport {
        super::tests::edhe_nav()
    }

    #[test]
    fn bei_msfs_passiert_nichts() {
        // ⚠ Die `apt.dat` beschreibt die X-Plane-Welt. Wer MSFS fliegt,
        // hat eine andere Szenerie — die hier zu benutzen waere
        // schlimmer als gar keine Korrektur, weil sie plausibel
        // aussieht und falsch ist.
        for sim in [Simulator::Msfs2020, Simulator::Msfs2024, Simulator::Other] {
            let vorher = nav_edhe();
            let (nachher, bericht, _) =
                ergaenze_aus_szenerie(sim, "EDHE", Some(vorher.clone()), None);
            assert!(
                bericht.is_none(),
                "{sim:?}: Bericht trotz falschem Simulator"
            );
            assert_eq!(
                nachher.unwrap().runways[0].true_course,
                vorher.runways[0].true_course,
                "{sim:?}: Kurs veraendert"
            );
        }
    }

    #[test]
    fn ohne_navdaten_wird_nichts_erfunden() {
        // Einen Flughafen ALLEIN aus der Szenerie zu bauen waere
        // moeglich — dann fehlten aber ILS, Gleitwinkel und
        // Schwellenhoehe, und die Anzeige haette stillschweigend
        // weniger als vorher.
        let (nachher, bericht, _) = ergaenze_aus_szenerie(Simulator::XPlane12, "EDHE", None, None);
        assert!(nachher.is_none());
        assert!(bericht.is_none());
    }

    #[test]
    fn jede_quelle_gehoert_zu_ihrem_simulator() {
        // ⚠ Die Quellen sind NICHT austauschbar. Die `apt.dat` fuer
        // einen MSFS-Flug zu lesen waere schlimmer als gar keine
        // Korrektur — plausible Zahlen aus der falschen Welt.
        assert_eq!(quelle_fuer(Simulator::XPlane11), Quelle::XPlaneDatei);
        assert_eq!(quelle_fuer(Simulator::XPlane12), Quelle::XPlaneDatei);
        assert_eq!(quelle_fuer(Simulator::Msfs2020), Quelle::MsfsFacility);
        assert_eq!(quelle_fuer(Simulator::Msfs2024), Quelle::MsfsFacility);
        assert_eq!(quelle_fuer(Simulator::Other), Quelle::Keine);
    }

    fn msfs_auskunft(icao: &str, kurs: f64) -> sim_core::szenerie::SzenerieFlughafen {
        sim_core::szenerie::SzenerieFlughafen {
            staende: Vec::new(),
            icao: icao.to_string(),
            quelle: "msfs".into(),
            rollwege: vec![],
            bahnen: vec![SzenerieBahn {
                bezeichner: "09".into(),
                kurs_grad: kurs,
                breite_m: 55.0,
                laenge_m: 1100.0,
                versetzte_schwelle_m: 0.0,
                schwelle: (53.6459, 9.6942),
                gegenende: (53.6459, 9.7142),
                belag_code: 1,
            }],
        }
    }

    #[test]
    fn msfs_nimmt_die_angeforderte_auskunft() {
        let vorher = nav_edhe();
        let alter_kurs = vorher.runways[0].true_course;
        let (nachher, bericht, _) = ergaenze_aus_szenerie(
            Simulator::Msfs2024,
            "EDHE",
            Some(vorher),
            Some(msfs_auskunft("EDHE", 93.72)),
        );
        let b = bericht.expect("Bericht");
        assert_eq!(b.uebernommen, vec!["09"]);
        let bahn = &nachher.unwrap().runways[0];

        // ⚠ Der KURS bleibt bei den Navdaten — auch wenn zugeordnet
        // wurde. Die Facility-Doku sagt nicht, ob `HEADING` missweisend
        // oder rechtweisend ist; ein uebernommener Kurs koennte die
        // Bewertungsachse um die oertliche Missweisung verdrehen.
        assert!(
            (bahn.true_course - alter_kurs).abs() < 0.001,
            "MSFS-Kurs wurde uebernommen, obwohl sein Bezug unbewiesen ist"
        );

        // Die Breite dagegen meldet der Simulator direkt — die gilt.
        assert_eq!(bahn.width_ft, Some((55.0_f64 / 0.3048).round() as i32));

        // Und die Abweichung wird gemessen, damit der Bestand die Frage
        // spaeter entscheiden kann.
        assert!(b.groesste_kursabweichung_grad > 3.0);
    }

    #[test]
    fn msfs_verwirft_die_auskunft_eines_anderen_platzes() {
        // ⚠ Nach einem Divert liegt sonst die Auskunft des GEPLANTEN
        // Ziels da — plausible Zahlen, falscher Flughafen.
        let vorher = nav_edhe();
        let (nachher, bericht, _) = ergaenze_aus_szenerie(
            Simulator::Msfs2024,
            "EDHE",
            Some(vorher.clone()),
            Some(msfs_auskunft("EDDH", 233.0)),
        );
        assert!(bericht.is_none(), "fremde Auskunft wurde benutzt");
        assert_eq!(
            nachher.unwrap().runways[0].true_course,
            vorher.runways[0].true_course
        );
    }

    #[test]
    fn msfs_ohne_auskunft_bleibt_bei_den_navdaten() {
        let vorher = nav_edhe();
        let (nachher, bericht, _) =
            ergaenze_aus_szenerie(Simulator::Msfs2024, "EDHE", Some(vorher.clone()), None);
        assert!(bericht.is_none());
        assert_eq!(
            nachher.unwrap().runways[0].true_course,
            vorher.runways[0].true_course
        );
    }

    #[test]
    fn die_benutzte_szenerie_wird_zurueckgegeben() {
        // ⚠ Ohne sie kaemen die Rollwege nie bei den Ausfahrten an: Die
        // Bahn waere korrigiert, die Ausfahrten kaemen weiter aus
        // OpenStreetMap. Bei X-Plane liest die Funktion die Datei
        // selbst — der Aufrufer hat sie sonst gar nicht.
        if sim_xplane::szenerie::installationen().is_empty() {
            eprintln!("uebersprungen: keine X-Plane-Installation");
            return;
        }
        let (_, _, sz) = ergaenze_aus_szenerie(Simulator::XPlane12, "EDDH", Some(nav_edhe()), None);
        let sz = sz.expect("die gelesene Szenerie muss zurueckkommen");
        assert!(!sz.rollwege.is_empty(), "EDDH ohne Rollwege?");
    }

    #[test]
    fn auch_ohne_uebernommene_bahn_kommt_die_szenerie_zurueck() {
        // Die Bahn ist nur ein Teil der Auskunft. Passt keine, sind die
        // Rollwege trotzdem brauchbar.
        let nav = nav_edhe();
        let (_, bericht, sz) = ergaenze_aus_szenerie(
            Simulator::Msfs2024,
            "EDHE",
            Some(nav),
            Some(sim_core::szenerie::SzenerieFlughafen {
                staende: Vec::new(),
                icao: "EDHE".into(),
                quelle: "msfs".into(),
                bahnen: vec![SzenerieBahn {
                    bezeichner: "27".into(),
                    kurs_grad: 273.0,
                    breite_m: 55.0,
                    laenge_m: 1100.0,
                    versetzte_schwelle_m: 0.0,
                    schwelle: (53.6459, 9.7142),
                    gegenende: (53.6459, 9.6942),
                    belag_code: 1,
                }],
                rollwege: vec![sim_core::szenerie::SzenerieRollweg {
                    name: "B3".into(),
                    punkte: vec![(53.6459, 9.70), (53.6465, 9.701)],
                }],
            }),
        );
        assert!(bericht.is_some_and(|b| b.uebernommen.is_empty()));
        let sz = sz.expect("Szenerie trotz nicht uebernommener Bahn");
        assert_eq!(sz.rollwege.len(), 1);
    }

    #[test]
    fn xplane_ignoriert_eine_msfs_auskunft() {
        // Der Adapter koennte eine alte Auskunft halten, wenn der Pilot
        // den Simulator wechselt. Sie darf dann nicht greifen.
        if sim_xplane::szenerie::installationen().is_empty() {
            eprintln!("uebersprungen: keine X-Plane-Installation");
            return;
        }
        let (nachher, _, _) = ergaenze_aus_szenerie(
            Simulator::XPlane12,
            "EDHE",
            Some(nav_edhe()),
            Some(msfs_auskunft("EDHE", 200.0)),
        );
        // 200 Grad waere die MSFS-Auskunft; die Datei sagt rund 93,7.
        let k = nachher.unwrap().runways[0].true_course;
        assert!(
            (k - 200.0).abs() > 50.0,
            "MSFS-Auskunft griff bei X-Plane: {k}"
        );
    }

    #[test]
    fn mit_xplane_wird_der_kurs_wirklich_korrigiert() {
        // Gegen die hier installierte Szenerie. Ohne Installation
        // ueberspringt sich der Test — sichtbar, nicht still.
        if sim_xplane::szenerie::installationen().is_empty() {
            eprintln!("uebersprungen: keine X-Plane-Installation");
            return;
        }
        let vorher = nav_edhe();
        let (nachher, bericht, _) =
            ergaenze_aus_szenerie(Simulator::XPlane12, "EDHE", Some(vorher.clone()), None);
        let Some(b) = bericht else {
            panic!("kein Bericht — Szenerie nicht gefunden?");
        };
        assert!(
            b.uebernommen.contains(&"09".to_string()),
            "EDHE 09 nicht uebernommen: {b:?}"
        );
        let n = nachher.unwrap();
        // Unsere Navdaten fuehren 89,996 Grad (aus 87,0 magnetisch
        // abgeleitet), die installierte Szenerie 93,72.
        assert!(
            (n.runways[0].true_course - 93.72).abs() < 0.2,
            "Kurs nach der Uebernahme: {}",
            n.runways[0].true_course
        );
        assert!(
            b.groesste_kursabweichung_grad > 3.0,
            "gemeldete Abweichung zu klein: {}",
            b.groesste_kursabweichung_grad
        );
    }
}

/// Die Rollwege der Szenerie in die Bodenkarten-Form bringen.
///
/// # Warum
///
/// Die Ausfahrten werden heute aus einer GeoJSON-Bodenkarte gelesen, die
/// der Server aus OpenStreetMap baut — einer **dritten** Welt, die weder
/// mit unseren Navdaten noch mit der Szenerie identisch ist. Der Pilot
/// rollt auf „B3" und der Bericht sagt „RWY Ende", weil OSM den Weg
/// nicht kennt: 167 namenlose Ausfahrten an 68 Plätzen, sieben Plätze
/// ganz ohne.
///
/// Die Szenerie kennt sie. In der installierten X-Plane-Szenerie stehen
/// 243.945 benannte Rollwegkanten an 6.114 Flughäfen — das Dreissigfache
/// unseres bisherigen Bestands. Bringt man sie in dieselbe Form, liest
/// die vorhandene Ausfahrtserkennung sie unverändert.
///
/// ⚠ **GeoJSON ist `[Länge, Breite]`**, unsere Punkte sind
/// `(Breite, Länge)`. Vertauscht landet jeder Rollweg im Golf von
/// Guinea, und die Ausfahrtserkennung findet schlicht nichts — kein
/// Fehler, keine Meldung, nur eine leere Liste. Dieselbe Klasse, die
/// heute schon 75.610 Bahnen verworfen hat.
pub fn rollwege_als_bodenkarte(rollwege: &[sim_core::szenerie::SzenerieRollweg]) -> String {
    let mut merkmale: Vec<String> = Vec::with_capacity(rollwege.len());
    for r in rollwege {
        if r.name.trim().is_empty() || r.punkte.len() < 2 {
            continue;
        }
        let punkte: Vec<String> = r
            .punkte
            .iter()
            // (Breite, Länge) -> [Länge, Breite]
            .map(|(lat, lon)| format!("[{lon},{lat}]"))
            .collect();
        let name = r.name.replace('\\', "").replace('"', "");
        merkmale.push(format!(
            r#"{{"type":"Feature","properties":{{"k":"taxiway","r":"{name}"}},"geometry":{{"type":"LineString","coordinates":[{}]}}}}"#,
            punkte.join(",")
        ));
    }
    format!(
        r#"{{"type":"FeatureCollection","features":[{}]}}"#,
        merkmale.join(",")
    )
}

#[cfg(test)]
mod bodenkarte_tests {
    use super::*;
    use sim_core::szenerie::SzenerieRollweg;

    fn weg(name: &str) -> SzenerieRollweg {
        SzenerieRollweg {
            name: name.to_string(),
            // (Breite, Länge) — Hamburg.
            punkte: vec![(53.6304, 9.9882), (53.6310, 9.9890)],
        }
    }

    #[test]
    fn koordinaten_werden_gedreht() {
        // ⚠ GeoJSON ist [Länge, Breite]. Vertauscht liegt der Rollweg im
        // Golf von Guinea, und die Ausfahrtserkennung findet nichts —
        // ohne Fehler, ohne Meldung.
        let g = rollwege_als_bodenkarte(&[weg("B3")]);
        assert!(
            g.contains("[9.9882,53.6304]"),
            "Koordinaten nicht als [Länge, Breite]: {g}"
        );
        assert!(
            !g.contains("[53.6304,9.9882]"),
            "Koordinaten stehen vertauscht drin"
        );
    }

    #[test]
    fn die_vorhandene_ausfahrtserkennung_liest_das_ergebnis() {
        // Der eigentliche Beweis: nicht dass es gültiges JSON ist,
        // sondern dass der bestehende Leser damit etwas findet.
        let g = rollwege_als_bodenkarte(&[weg("B3"), weg("A1")]);
        let gefunden = crate::ausfahrten::ausfahrten_fuer_bahn(
            &g, 53.6280, 9.9860, // Schwelle
            53.6340, 9.9920, // Bahnende
            45.0,
        );
        assert!(
            !gefunden.is_empty(),
            "die Ausfahrtserkennung findet in der erzeugten Karte nichts"
        );
        assert!(gefunden.iter().any(|a| a.name == "B3"));
    }

    #[test]
    fn namenlose_und_zu_kurze_wege_fallen_weg() {
        let leer = SzenerieRollweg {
            name: "".into(),
            punkte: vec![(53.6, 9.9), (53.61, 9.91)],
        };
        let kurz = SzenerieRollweg {
            name: "C".into(),
            punkte: vec![(53.6, 9.9)],
        };
        let g = rollwege_als_bodenkarte(&[leer, kurz]);
        assert_eq!(g, r#"{"type":"FeatureCollection","features":[]}"#);
    }

    #[test]
    fn anfuehrungszeichen_im_namen_zerlegen_das_json_nicht() {
        let g = rollwege_als_bodenkarte(&[SzenerieRollweg {
            name: "A\"1".into(),
            punkte: vec![(53.6, 9.9), (53.61, 9.91)],
        }]);
        assert!(
            serde_json::from_str::<serde_json::Value>(&g).is_ok(),
            "erzeugtes JSON ist ungueltig: {g}"
        );
    }
}

#[cfg(test)]
mod ausfahrt_verdrahtung_tests {
    //! Wachen über den Anschluss der Szenerie-Rollwege an die Ausfahrten.

    const LIB: &str = include_str!("lib.rs");

    fn ohne_leerraum(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    #[test]
    fn die_szenerie_rollwege_werden_benutzt() {
        // Ohne diesen Anschluss waeren sie eingesammelt und nutzlos —
        // die Ausfahrten kaemen weiter aus OpenStreetMap.
        let a = ohne_leerraum(LIB);
        assert!(
            a.contains("szenerie_bahn::rollwege_als_bodenkarte("),
            "die Szenerie-Rollwege erreichen die Ausfahrtserkennung nicht"
        );
    }

    #[test]
    fn die_gelesene_szenerie_wird_am_flug_abgelegt() {
        // ⚠ Ohne diese Zeile bliebe die Rueckgabe liegen: Bei X-Plane
        // liest `ergaenze_aus_szenerie` die Datei selbst, und wenn der
        // Aufrufer sie nicht behaelt, kommen ihre Rollwege nie bei den
        // Ausfahrten an. Die Bahn waere korrigiert, die Ausfahrten
        // kaemen weiter aus OpenStreetMap — und niemand saehe warum.
        //
        // Genau das ist beim Bauen passiert, und keine Wache hat es
        // gefangen. Diese hier tut es.
        let a = ohne_leerraum(LIB);
        assert!(
            a.contains("stats.szenerie_auskunft=Some(sz);"),
            "die von `ergaenze_aus_szenerie` gelesene Szenerie wird nicht am Flug abgelegt"
        );
    }

    #[test]
    fn eine_leere_szenerie_verdraengt_die_serverkarte_nicht() {
        // ⚠ Eine leere Szenerie-Karte waere schlechter als eine gute
        // OSM-Karte: Die Anzeige zeigte dann gar keine Ausfahrten und
        // saehe aus wie „diese Bahn hat keine".
        let a = ohne_leerraum(LIB);
        assert!(
            a.contains(".filter(|a|!a.rollwege.is_empty())"),
            "eine leere Rollwegliste wuerde die Serverkarte verdraengen"
        );
        // v1.7.16 R5: der direkte Rueckfall `.or(stats.arr_ground_geojson
        // .as_deref())` bekam eine ICAO-Zugehoerigkeitspruefung
        // (`arr_karte`) vorgeschaltet — die Wache prueft seither BEIDE
        // Haelften: dass die Serverkarte weiterhin die Grundlage ist UND
        // dass sie weiterhin der letzte Rueckfall bleibt.
        assert!(
            a.contains("letarr_karte=match(&stats.arr_ground_geojson"),
            "arr_ground_geojson ist nicht mehr die Grundlage der Ankunfts-Bodenkarte"
        );
        assert!(
            a.contains(".or(arr_karte)"),
            "ohne Szenerie faellt es nicht auf die (ICAO-geprüfte) Serverkarte zurueck"
        );
    }
}

#[cfg(test)]
mod belag_uebernahme_tests {
    use super::*;

    #[test]
    fn eine_transparente_bahn_sagt_nichts_ueber_den_belag() {
        // ⚠ X-Plane-Schluessel 15 = transparent: Die Szenerie zeichnet den
        // Belag selbst. Das ist KEINE Aussage ueber ihn. Vorher wurde
        // daraus "SNOW" — unbefestigt — und die Queransicht verschwand.
        assert_eq!(belag_text(15), None);
        // Unbekannte Schluessel ebenso.
        assert_eq!(belag_text(99), None);
    }

    #[test]
    fn die_schluessel_folgen_x_plane_nicht_msfs() {
        // 13 = Wasser, 14 = Schnee/Eis. Vorher standen hier die
        // MSFS-Werte, um eins verschoben.
        assert_eq!(belag_text(13), Some("WATER"));
        assert_eq!(belag_text(14), Some("SNOW"));
        assert_eq!(belag_text(1), Some("ASPH"));
        assert_eq!(belag_text(2), Some("CONC"));
    }

    #[test]
    fn ein_unbekannter_belag_ueberschreibt_den_bekannten_nicht() {
        // Der eigentliche Schaden: Die Uebernahme der GEOMETRIE kostete
        // nebenbei den BELAG, und daran haengt die seitliche Bewertung.
        let q: String = include_str!("szenerie_bahn.rs")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let nadel = format!(
            "{}{}",
            "if!navdaten_kennen_belag{", "ifletSome(text)=belag_text(s.belag_code){"
        );
        assert!(
            q.contains(&nadel),
            "die Szenerie ueberschreibt einen bekannten Navdaten-Belag — das \
             loescht die gesamte seitliche Bewertung, wenn sie sich irrt"
        );
    }
}

#[cfg(test)]
mod plausibel_tests {
    use super::*;

    #[test]
    fn kein_feld_wird_ungeprueft_uebernommen() {
        // ⚠ DIE Wache gegen die Wiederholung.
        //
        // Dreimal hintereinander hat ein einzelnes Feld aus der Szenerie
        // einen guten Navdaten-Wert ersetzt, und dreimal habe ich dieses
        // eine Feld repariert statt die Regel. Das Muster ist nicht der
        // Kurs und nicht der Belag — es ist die BEDINGUNGSLOSE Uebernahme.
        //
        // Diese Pruefung faengt jedes kuenftige `bahn.x = s.y;`. Wer ein
        // Feld ergaenzt, muss es durch `plausibel::` fuehren.
        let q: String = include_str!("szenerie_bahn.rs")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let start = q
            .find("pubfnuebernimm_szenerie")
            .expect("uebernimm_szenerie nicht gefunden");
        let ende = start
            + q[start..]
                .find("(aus,b)")
                .expect("Ende der Funktion nicht gefunden");
        let rumpf = &q[start..ende];

        // Nadel zur Laufzeit — sonst findet der Test sich selbst.
        let direkt = format!("{}{}", "=", "s.");
        assert!(
            !rumpf.contains(&direkt),
            "ein Wert aus der Szenerie wird direkt zugewiesen, ohne durch \
             `plausibel::` zu gehen"
        );
        for pruefung in [
            "plausibel::kurs_grad(",
            "plausibel::strecke_m(",
            "plausibel::versatz_m(",
            "plausibel::koordinate(",
        ] {
            assert!(
                rumpf.contains(pruefung),
                "die Pruefung {pruefung} wird nicht mehr benutzt"
            );
        }
    }

    fn nav_mit_belag(belag: Option<&str>) -> NavAirport {
        let mut nav = super::tests::edhe_nav();
        nav.runways[0].surface = belag.map(|t| t.to_string());
        nav
    }

    fn szenerie_mit_belag(code: u8) -> SzenerieFlughafen {
        // Dieselbe Lage wie EDHE 09, damit die Zuordnung sicher trifft —
        // geprueft wird hier NUR der Belag.
        SzenerieFlughafen {
            staende: Vec::new(),
            icao: "EDHE".to_string(),
            bahnen: vec![SzenerieBahn {
                bezeichner: "09".to_string(),
                kurs_grad: 89.9957383300858,
                breite_m: 40.0,
                laenge_m: 1100.0,
                versetzte_schwelle_m: 0.0,
                schwelle: (53.6459, 9.6942),
                gegenende: (53.6459, 9.7142),
                belag_code: code,
            }],
            rollwege: Vec::new(),
            quelle: "xplane".to_string(),
        }
    }

    #[test]
    fn ein_bekannter_navdaten_belag_wird_nicht_ueberschrieben() {
        // ⚠ DAH411 (HKJK 06): Navdaten sagen `ASP`, die Szenerie
        // ueberschrieb es mit etwas Unbefestigtem — und die gesamte
        // seitliche Bewertung samt Queransicht fiel weg. Ein Wert mit
        // solchen Folgen darf nicht aus der schwaecheren Quelle kommen.
        let nav = nav_mit_belag(Some("ASP"));
        // Szenerie meldet Gras (Schluessel 3) — also etwas Unbefestigtes.
        let sz = szenerie_mit_belag(3);
        let (aus, _) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(
            aus.runways[0].surface.as_deref(),
            Some("ASP"),
            "die Szenerie hat den bekannten Navdaten-Belag ueberschrieben"
        );
    }

    #[test]
    fn eine_luecke_in_den_navdaten_fuellt_die_szenerie() {
        // Der Gegenfall: Kennen die Navdaten den Belag NICHT, ist die
        // Szenerie besser als nichts. HKJK hat im Bestand drei Zeilen,
        // zwei davon ohne Belag — genau dieser Fall kommt vor.
        for leer in [None, Some(""), Some("   ")] {
            let nav = nav_mit_belag(leer);
            let sz = szenerie_mit_belag(1); // Asphalt
            let (aus, _) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
            assert_eq!(
                aus.runways[0].surface.as_deref(),
                Some("ASPH"),
                "die Luecke wurde nicht gefuellt (Navdaten: {leer:?})"
            );
        }
    }

    #[test]
    fn eine_nach_norden_zeigende_bahn_wird_nicht_verworfen() {
        // ⚠ Erster Entwurf lehnte 0,0 ab — das war die Platzhalter-Logik
        // der NAVDATEN, auf die Szenerie uebertragen. X-Plane RECHNET den
        // Kurs aus den Schwellenkoordinaten; 0,0 ist dort eine echte
        // Nordbahn. Sie abzulehnen hiesse, sie beim kaputten
        // Navdaten-Wert zu belassen — also genau das Gegenteil dessen,
        // wofuer die Uebernahme gebaut wurde.
        assert_eq!(plausibel::kurs_grad(0.0), Some(0.0));
        assert_eq!(plausibel::kurs_grad(232.7), Some(232.7));
        assert_eq!(plausibel::kurs_grad(359.99), Some(359.99));
        // Ausserhalb des Bereichs bleibt abgelehnt.
        assert_eq!(plausibel::kurs_grad(360.0), None);
        assert_eq!(plausibel::kurs_grad(-1.0), None);
        assert_eq!(plausibel::kurs_grad(f64::NAN), None);
    }

    #[test]
    fn eine_bahn_ohne_laenge_oder_breite_wird_abgelehnt() {
        assert_eq!(plausibel::strecke_m(0.0), None);
        assert_eq!(plausibel::strecke_m(-5.0), None);
        assert_eq!(plausibel::strecke_m(f64::INFINITY), None);
        assert_eq!(plausibel::strecke_m(3500.0), Some(3500.0));
    }

    #[test]
    fn null_null_ist_kein_flughafen() {
        // Der Punkt liegt im Atlantik vor Ghana — ein nicht gefuelltes
        // Feld, kein Ort.
        assert_eq!(plausibel::koordinate((0.0, 0.0)), None);
        assert_eq!(plausibel::koordinate((91.0, 10.0)), None);
        assert_eq!(plausibel::koordinate((f64::NAN, 10.0)), None);
        assert_eq!(plausibel::koordinate((36.69, 3.21)), Some((36.69, 3.21)));
    }

    #[test]
    fn eine_schwelle_ohne_versatz_ist_gueltig() {
        // Null ist hier ECHT — die meisten Bahnen haben keinen Versatz.
        assert_eq!(plausibel::versatz_m(0.0), Some(0.0));
        assert_eq!(plausibel::versatz_m(-1.0), None);
    }
}
