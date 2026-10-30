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

// Wie weit die Szenerie-Bahn QUER zur Navdaten-Achse liegen darf,
// damit es dieselbe Bahn sein kann.
//
// Hundert Meter sind grosszügig für Vermessungsunterschiede und zu eng
// für eine Nachbarbahn: Parallelbahnen liegen nach ICAO mindestens
// 210 m auseinander, in der Praxis meist deutlich mehr.
//
// ⚠ Bis v1.7.12 stand hier ein Punkt-zu-Punkt-Abstand der Schwellen mit
// 200 m. Der ist bei MSFS auf drei Annahmen gebaut (Mitte, Kurs, Laenge
// — die Schwellen werden daraus gerechnet) und schlug am 30./31.08.2026
// an DREI Plaetzen fehl (EDDF 25L, YBWW 12, YBCG 14), ohne zu sagen,
// welche der drei nicht stimmte. Der Querabstand haengt nur an der
// Mitte; die Begruendung steht bei `querabstand_m`.
// Wert seit v3: bundle/grenzwerte/szenerie.json -> querabstand_hoechst_m (v2.1.24: 100.0)

// Wie weit die Mitte der Szenerie-Bahn LÄNGS der Achse von der Mitte
// der Navdaten-Bahn abliegen darf.
//
// Vierhundert Meter: Der grösste Versatz, der bei derselben Bahn
// entstehen kann, kommt von einer versetzten Schwelle — sie verschiebt
// den Navdaten-Mittelpunkt um die HÄLFTE des Versatzes. Der grösste im
// Bestand gemessene Versatz sind 573 m (TJPS 12), also 287 m. Eine Bahn
// am anderen Ende liegt dagegen mindestens eine halbe Bahnlänge weit
// weg — bei der kürzesten Piste im Bestand 550 m.
// Wert seit v3: bundle/grenzwerte/szenerie.json -> laengsversatz_hoechst_m (v2.1.24: 400.0)

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
    /// P-4.14 (c): Woher die ACHSE jeder zugeordneten Bahn kommt — je Bahn
    /// aus `uebernommen` ein Eintrag. Geht als `korrekturen` ins
    /// Eingabe-Dokument (P-9.44) und mit dem Flugzustand auf die Platte.
    ///
    /// ⚠ Getrennt von `uebernommen`: Bis P-4.14 hiess „übernommen" bei MSFS
    /// nur Breite/Länge/Schwelle/Belag, die Achse blieb Navigraph — und
    /// `bahn_geometrie_aus_szenerie` hielt sie trotzdem für bestätigt (Fund
    /// CFG2001, FQNP 23). Ein Bericht von vor P-4.14 (ohne dieses Feld)
    /// gilt deshalb als „Achse Navigraph".
    #[serde(default)]
    pub achsen: Vec<AchsenUebernahme>,
    /// P-4.15 (d): Je Navdaten-Bahn OHNE Übernahme die drei nächsten
    /// Szenerie-Bahnen mit Lage ihrer Mitte — damit sich ein Rückfall auf
    /// Navigraph aus dem Bericht erklären lässt (Fall UUDD: Szenerie-Bahnen
    /// unter 1 m entfernt, nur anders benannt).
    #[serde(default)]
    pub verworfene_kandidaten: Vec<VerworfeneSzenerieBahn>,
}

/// P-4.15 (d): Eine nicht genommene Szenerie-Bahn und ihre Lage.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VerworfeneSzenerieBahn {
    /// Navdaten-Bahn, für die gesucht wurde.
    pub bahn: String,
    /// Bezeichner der Szenerie-Bahn.
    pub szenerie_bahn: String,
    /// Mitte der Szenerie-Bahn quer zur Navdaten-Achse (m, + = rechts).
    pub quer_m: f64,
    /// Mitte der Szenerie-Bahn längs zur Navdaten-Mitte (m).
    pub laengs_m: f64,
}

impl UebernahmeBericht {
    /// P-4.14 (b): Stammt die Achse DIESER Bahn aus der Szenerie?
    ///
    /// Nur dann ist sie gegen das geprüft, was der Pilot sieht. Dass Breite
    /// oder Länge übernommen wurden, sagt über die Lage der Mittellinie
    /// nichts.
    pub fn achse_aus_szenerie(&self, bezeichner: &str) -> bool {
        let ziel = normiert(bezeichner);
        self.achsen
            .iter()
            .any(|a| normiert(&a.bahn) == ziel && a.herkunft != AchsenHerkunft::Navigraph)
    }
}

/// Woher die Mittellinie einer Bahn kommt (P-4.14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AchsenHerkunft {
    /// Navdaten (Navigraph), unverändert.
    #[default]
    Navigraph,
    /// X-Plane: beide Schwellen aus der `apt.dat`.
    SzenerieDatei,
    /// MSFS: Navdaten-Achse durch die gemeldete Facility-Mitte gelegt.
    SzenerieMitte,
}

/// Woher der Kurs der Achse kommt (P-4.14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KursHerkunft {
    /// Richtung der Navdaten-Geometrie (Schwelle → Gegenende).
    #[default]
    Navigraph,
    /// X-Plane: aus den beiden Schwellen der `apt.dat`.
    SzenerieDatei,
    /// MSFS: unabhängig bestätigter wahrer Kurs (`kurs_bestaetigt_grad`).
    Bestaetigt,
    /// MSFS: `HEADING` der Facility (rechtweisend, Schattenmodus-Auswertung
    /// 30.10.2026: 1 842 Bahnenden, Median 0,01°).
    Heading,
}

/// P-4.14 (c): Achse einer Bahn — Herkunft, Facility-Mitte, Verschiebung.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AchsenUebernahme {
    /// Bezeichner der Bahn (Navdaten).
    pub bahn: String,
    /// P-4.15 (a): Bezeichner der Szenerie-Bahn, wenn er anders lautet
    /// (Zuordnung über die Lage, etwa UUDD 32L = Navigraph 31L). Die
    /// Navdaten-Bezeichnung bleibt die Identität der Bahn: ILS,
    /// `runway_nav_geometry` und die Leitung suchen über sie.
    pub szenerie_bezeichner: Option<String>,
    pub herkunft: AchsenHerkunft,
    pub kurs_quelle: KursHerkunft,
    /// Mitte der Szenerie-Bahn (Breite, Länge). Bei MSFS die gemeldete
    /// Facility-Mitte (`LATITUDE`/`LONGITUDE`).
    pub szenerie_mitte: Option<(f64, f64)>,
    /// Lage der Szenerie-Mitte zur Navdaten-Mitte QUER zur Navdaten-Achse,
    /// in Metern, positiv = rechts in Landerichtung. Bei
    /// `SzenerieMitte` ist das die angewandte seitliche Verschiebung.
    pub verschiebung_quer_m: Option<f64>,
    /// Angewandte Verschiebung LÄNGS der Achse, in Metern (positiv in
    /// Landerichtung). `0` heisst: nicht angewandt, Grund in `grund`.
    pub verschiebung_laengs_m: Option<f64>,
    /// Gemessener Längsabstand der Szenerie-Mitte zur Navdaten-Mitte.
    pub laengs_gemessen_m: Option<f64>,
    /// Warum etwas nicht (ganz) übernommen wurde.
    pub grund: Option<String>,
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
        if quer > crate::grenzwerte::grenzwerte().szenerie.querabstand_hoechst_m.wert || laengs > crate::grenzwerte::grenzwerte().szenerie.laengsversatz_hoechst_m.wert {
            continue;
        }
        return (Some(b), true);
    }
    (None, bezeichner_passte)
}

/// P-4.15 (a): Zuordnung über die LAGE, wenn der Bezeichner bei keiner
/// Szenerie-Bahn passte (`ohne_treffer`).
///
/// # Warum
///
/// UUDD (td 1647, X-Plane, Client 1.9.22): `kein_treffer(bahnen=6,
/// ohne_treffer=6)`. Die Szenerie (Gateway 90987, 2022) führt 14/32,
/// 14L/32R, 14R/32L, Navigraph seit der Missweisungs-Umbenennung 13L/31R,
/// 13C/31C, 13R/31L — auf unter einem Meter dieselben Schwellen. Allein der
/// Bezeichner verhinderte die Übernahme; gerechnet wurde still gegen
/// Navigraph.
///
/// Dieselben Lagegrenzen wie über den Bezeichner (Mitte quer/längs). Welches
/// ENDE es ist, entscheidet die Nähe der Szenerie-Schwelle zur
/// Navdaten-Schwelle (die Mitte ist für beide Enden dieselbe). Szenerie-
/// Bahnen, deren Bezeichner zu einer Navdaten-Bahn passt, gehören dieser
/// und sind hier ausgeschlossen. Nur für `ohne_treffer`, nie für
/// `verworfen` — passte der Bezeichner und die Lage nicht, ist es eine
/// andere Bahn (FACT 19, EDHE).
fn szenerie_bahn_ueber_lage<'a>(
    nav: &NavAirport,
    bahn: &NavRunway,
    sz: &'a SzenerieFlughafen,
) -> Option<&'a SzenerieBahn> {
    let g = &crate::grenzwerte::grenzwerte().szenerie;
    let schwelle = (bahn.threshold.lat, bahn.threshold.lon);
    sz.bahnen
        .iter()
        .filter(|s| {
            let n = normiert(&s.bezeichner);
            !nav.runways.iter().any(|r| normiert(&r.designator) == n)
        })
        .filter(|s| abstand_m(s.schwelle, schwelle) < abstand_m(s.gegenende, schwelle))
        .filter_map(|s| {
            let mitte = ((s.schwelle.0 + s.gegenende.0) / 2.0, (s.schwelle.1 + s.gegenende.1) / 2.0);
            let (laengs, quer) = achsenversatz_m(
                (bahn.threshold.lat, bahn.threshold.lon),
                (bahn.far_end.lat, bahn.far_end.lon),
                mitte,
            );
            (quer <= g.querabstand_hoechst_m.wert && laengs <= g.laengsversatz_hoechst_m.wert).then_some((s, quer))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
}

/// P-4.15 (d): Die nächsten Szenerie-Bahnen einer nicht übernommenen
/// Navdaten-Bahn, mit Lage ihrer Mitte zur Navdaten-Mitte.
fn naechste_kandidaten(bahn: &NavRunway, sz: &SzenerieFlughafen) -> Vec<VerworfeneSzenerieBahn> {
    let Some(r) = Rahmen::aus(bahn) else {
        return Vec::new();
    };
    let mut k: Vec<VerworfeneSzenerieBahn> = sz
        .bahnen
        .iter()
        .map(|s| {
            let mitte = ((s.schwelle.0 + s.gegenende.0) / 2.0, (s.schwelle.1 + s.gegenende.1) / 2.0);
            let (laengs_m, quer_m) = r.lage(mitte);
            VerworfeneSzenerieBahn {
                bahn: bahn.designator.clone(),
                szenerie_bahn: s.bezeichner.clone(),
                quer_m,
                laengs_m,
            }
        })
        .filter(|v| v.quer_m.is_finite() && v.laengs_m.is_finite())
        .collect();
    k.sort_by(|a, b| a.quer_m.hypot(a.laengs_m).total_cmp(&b.quer_m.hypot(b.laengs_m)));
    k.truncate(3);
    k
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

/// Ebener Rahmen um die Mitte einer Navdaten-Bahn (P-4.14).
///
/// Ebene Näherung wie `achsenversatz_m`: auf Bahnlänge millimetergenau.
struct Rahmen {
    mitte: (f64, f64),
    cos_breite: f64,
    /// Einheitsvektor Schwelle → Gegenende (Ost, Nord).
    achse: (f64, f64),
    /// Abstand Schwelle → Gegenende in Metern.
    laenge_m: f64,
}

const M_JE_GRAD_BREITE: f64 = 111_320.0;

impl Rahmen {
    fn aus(bahn: &NavRunway) -> Option<Self> {
        let t = (bahn.threshold.lat, bahn.threshold.lon);
        let e = (bahn.far_end.lat, bahn.far_end.lon);
        let mitte = ((t.0 + e.0) / 2.0, (t.1 + e.1) / 2.0);
        let mut r = Self {
            mitte,
            cos_breite: mitte.0.to_radians().cos(),
            achse: (0.0, 0.0),
            laenge_m: 0.0,
        };
        let (tx, ty) = r.eben(t);
        let (ex, ey) = r.eben(e);
        let laenge_m = (ex - tx).hypot(ey - ty);
        if !laenge_m.is_finite() || laenge_m < 1.0 {
            return None;
        }
        r.achse = ((ex - tx) / laenge_m, (ey - ty) / laenge_m);
        r.laenge_m = laenge_m;
        Some(r)
    }
    fn eben(&self, p: (f64, f64)) -> (f64, f64) {
        (
            (p.1 - self.mitte.1) * M_JE_GRAD_BREITE * self.cos_breite,
            (p.0 - self.mitte.0) * M_JE_GRAD_BREITE,
        )
    }
    fn zurueck(&self, (x, y): (f64, f64)) -> (f64, f64) {
        (
            self.mitte.0 + y / M_JE_GRAD_BREITE,
            self.mitte.1 + x / (M_JE_GRAD_BREITE * self.cos_breite),
        )
    }
    /// (längs, quer) eines Punkts zur Mitte; quer positiv = rechts.
    fn lage(&self, p: (f64, f64)) -> (f64, f64) {
        let (x, y) = self.eben(p);
        let (ux, uy) = self.achse;
        (x * ux + y * uy, x * uy - y * ux)
    }
    /// Kurs der Navdaten-Geometrie in Grad `[0, 360)`.
    fn kurs_grad(&self) -> f64 {
        self.achse.0.atan2(self.achse.1).to_degrees().rem_euclid(360.0)
    }
}

/// Neue Achse einer Bahn: Schwelle, Gegenende und — falls aus der Szenerie
/// — der Kurs.
struct NeueAchse {
    schwelle: (f64, f64),
    gegenende: (f64, f64),
    kurs_grad: Option<f64>,
}

/// P-4.14 (a): Die Navdaten-Achse bei MSFS durch die Facility-Mitte legen.
///
/// # Warum die Mitte
///
/// MSFS meldet je Bahn MITTE, KURS und LÄNGE; die Schwellen der
/// `SzenerieBahn` rechnet `bahn_paar` daraus. Die Mitte ist die einzige
/// direkt gemeldete Lage und hängt an keinem Bezug (missweisend oder
/// rechtweisend) — anders als der Kurs. Navigraph führt manche Schwellen
/// nur auf ganze Bogensekunden: FQNP 05/23 lag 15,6 m (Schwelle 23) bis
/// 17,1 m (Schwelle 05) neben der Bahn, Joel (CFG2001) bekam „neben der
/// Bahn" für eine Landung auf der Mitte.
///
/// # Kurs
///
/// Erst der unabhängig bestätigte Kurs, dann `HEADING` — je nur, wenn er
/// höchstens `kurs_abweichung_hoechst_grad` (Paket) neben `HEADING` bzw.
/// der Navdaten-Geometrie liegt. Sonst bleibt die Richtung der
/// Navdaten-Geometrie, und die Achse wird nur verschoben, nicht gedreht.
/// Verglichen wird mit der GEOMETRIE (Schwelle → Gegenende), nicht mit
/// `true_course`: Gegen die Geometrie wird gemessen, und `true_course`
/// ist bei tausenden Bahnen ein Platzhalter (0,0).
///
/// # Warum längs nur ohne versetzte Schwelle
///
/// Bei 87,5 % der Navigraph-Bahnen mit Versatz steckt der Versatz schon im
/// Schwellenpunkt (`geometry_implied_displacement_ft`). Dann ist die
/// Navdaten-Mitte NICHT die Mitte des Belags, sondern um den halben
/// Versatz-Unterschied verschoben — gewollt, deshalb lässt die Zuordnung
/// 400 m längs zu. Sie auf die Facility-Mitte zu schieben, versetzte die
/// LANDEschwelle (Nullpunkt der Aufsetzzone) um bis zu 287 m (TJPS 12).
/// Längs wird deshalb nur verschoben, wenn beide Enden laut Navdaten und
/// laut Geometrie keinen Versatz haben; sonst nur quer, mit Grund.
///
/// # Plausibilität
///
/// Liegt die Mitte weiter als die halbe Navdaten-Bahnlänge von der
/// Navdaten-Mitte entfernt, bleibt die Navdaten-Achse, mit Grund.
fn msfs_achse(
    nav: &NavAirport,
    orig: &NavRunway,
    s: &SzenerieBahn,
) -> (AchsenUebernahme, Option<NeueAchse>) {
    let mut a = AchsenUebernahme {
        bahn: orig.designator.clone(),
        ..Default::default()
    };
    let Some(r) = Rahmen::aus(orig) else {
        a.grund = Some("navdaten_achse_entartet".into());
        return (a, None);
    };
    let mitte = match (plausibel::koordinate(s.schwelle), plausibel::koordinate(s.gegenende)) {
        (Some(p), Some(q)) => ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0),
        _ => {
            a.grund = Some("mitte_ungueltig".into());
            return (a, None);
        }
    };
    a.szenerie_mitte = Some(mitte);
    let (laengs, quer) = r.lage(mitte);
    a.laengs_gemessen_m = Some(laengs);
    a.verschiebung_quer_m = Some(quer);
    let abstand = laengs.hypot(quer);
    let halbe_m = orig.length_ft as f64 * 0.3048 / 2.0;
    if !(abstand <= halbe_m) {
        a.verschiebung_quer_m = None;
        a.grund = Some(format!(
            "mitte_zu_weit(abstand_m={abstand:.1}, halbe_laenge_m={halbe_m:.1})"
        ));
        return (a, None);
    }

    // Längs nur, wenn kein Versatz in den Navdaten-Schwellen steckt.
    let ohne_versatz = |b: &NavRunway| crate::runway::navigraph_versatz_ft(nav, b) == (0, 0);
    let gegen = nav
        .runways
        .iter()
        .find(|o| o.threshold.lat == orig.far_end.lat && o.threshold.lon == orig.far_end.lon);
    let mut gruende = Vec::new();
    let laengs_anw = match gegen {
        Some(g) if ohne_versatz(orig) && ohne_versatz(g) => laengs,
        Some(_) => {
            gruende.push("laengs_nicht_angewandt(versetzte_schwelle)".to_string());
            0.0
        }
        None => {
            gruende.push("laengs_nicht_angewandt(gegenschwelle_unbekannt)".to_string());
            0.0
        }
    };

    // Kurs (Auftrag P-4.14 Abschnitt 4.1).
    let toleranz = crate::grenzwerte::grenzwerte().szenerie.kurs_abweichung_hoechst_grad.wert;
    let geometrie = r.kurs_grad();
    let heading = plausibel::kurs_grad(s.kurs_grad);
    let bestaetigt = s.kurs_bestaetigt_grad.and_then(plausibel::kurs_grad);
    let nahe = |k: f64, bezug: Option<f64>| bezug.is_some_and(|b| winkelabstand(k, b) <= toleranz);
    let (kurs, quelle) = match (bestaetigt, heading) {
        (Some(b), h) if nahe(b, h) || nahe(b, Some(geometrie)) => (Some(b), KursHerkunft::Bestaetigt),
        (b, Some(h)) if nahe(h, b) || nahe(h, Some(geometrie)) => (Some(h), KursHerkunft::Heading),
        _ => {
            gruende.push("kurs_navigraph(szenerie_kurs_ausser_toleranz)".to_string());
            (None, KursHerkunft::Navigraph)
        }
    };
    let richtung = match kurs {
        Some(k) => (k.to_radians().sin(), k.to_radians().cos()),
        None => r.achse,
    };
    // Die Achse läuft exakt durch die Facility-Mitte (gedreht wird um sie);
    // ihre neue Mitte ist die Facility-Mitte, um den NICHT angewandten
    // Längsanteil entlang der neuen Richtung zurückgesetzt.
    let (mx, my) = r.eben(mitte);
    let rest = laengs - laengs_anw;
    let neue_mitte = (mx - rest * richtung.0, my - rest * richtung.1);
    let halb = r.laenge_m / 2.0;
    let schwelle = r.zurueck((neue_mitte.0 - halb * richtung.0, neue_mitte.1 - halb * richtung.1));
    let gegenende = r.zurueck((neue_mitte.0 + halb * richtung.0, neue_mitte.1 + halb * richtung.1));

    a.herkunft = AchsenHerkunft::SzenerieMitte;
    a.kurs_quelle = quelle;
    a.verschiebung_laengs_m = Some(laengs_anw);
    a.grund = (!gruende.is_empty()).then(|| gruende.join(", "));
    (
        a,
        Some(NeueAchse {
            schwelle,
            gegenende,
            kurs_grad: kurs,
        }),
    )
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
    // daraus. Bis P-4.14 (3.0) kam bei MSFS deshalb nur Laenge, Breite,
    // versetzte Schwelle und Belag aus der Szenerie; die Lage der
    // Mittellinie blieb Navigraph. Das ging an FQNP 05/23 um 15,6–17,1 m
    // daneben (CFG2001, 30.10.2026). Seit P-4.14 wird die Navdaten-Achse
    // durch die gemeldete MITTE gelegt (bezugsfrei); den Kurs nimmt sie
    // nur in enger Toleranz zur Navdaten-Geometrie — siehe `msfs_achse`.
    let achse_belastbar = quelle == Quelle::XPlaneDatei;
    let mut aus = nav.clone();
    let mut b = UebernahmeBericht::default();

    for (i, bahn) in aus.runways.iter_mut().enumerate() {
        // Die UNVERAENDERTE Navdaten-Bahn — Bezug fuer Mitte und Versatz.
        let orig = &nav.runways[i];
        let (mut treffer, bezeichner_passte) = passende_szenerie_bahn(bahn, sz);
        // P-4.15 (a): Passte der Bezeichner nirgends, über die Lage.
        let ueber_lage = treffer.is_none() && !bezeichner_passte;
        if ueber_lage {
            treffer = szenerie_bahn_ueber_lage(nav, orig, sz);
        }
        let Some(s) = treffer else {
            if bezeichner_passte {
                b.verworfen.push(bahn.designator.clone());
            } else {
                b.ohne_treffer.push(bahn.designator.clone());
            }
            b.verworfene_kandidaten.extend(naechste_kandidaten(orig, sz));
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
            let schwelle = plausibel::koordinate(s.schwelle);
            let gegenende = plausibel::koordinate(s.gegenende);
            if let Some((lat, lon)) = schwelle {
                bahn.threshold.lat = lat;
                bahn.threshold.lon = lon;
            }
            if let Some((lat, lon)) = gegenende {
                bahn.far_end.lat = lat;
                bahn.far_end.lon = lon;
            }
            // P-4.14 (c): Herkunft mitschreiben, samt Lage der Mitte zur
            // Navdaten-Mitte (Messung „Navigraph gegen Szenerie").
            let mut a = AchsenUebernahme {
                bahn: bahn.designator.clone(),
                ..Default::default()
            };
            match (schwelle, gegenende) {
                (Some(p), Some(q)) => {
                    let mitte = ((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0);
                    a.herkunft = AchsenHerkunft::SzenerieDatei;
                    a.kurs_quelle = KursHerkunft::SzenerieDatei;
                    a.szenerie_mitte = Some(mitte);
                    if let Some(r) = Rahmen::aus(orig) {
                        let (laengs, quer) = r.lage(mitte);
                        a.verschiebung_quer_m = Some(quer);
                        a.verschiebung_laengs_m = Some(laengs);
                        a.laengs_gemessen_m = Some(laengs);
                    }
                }
                // Nur ein Ende gueltig: die Achse ist ein Gemisch aus beiden
                // Quellen und gilt nicht als aus der Szenerie bestaetigt.
                _ => a.grund = Some("schwelle_ungueltig".into()),
            }
            b.achsen.push(a);
        } else {
            let (a, neu) = msfs_achse(nav, orig, s);
            if let Some(neu) = neu {
                bahn.threshold.lat = neu.schwelle.0;
                bahn.threshold.lon = neu.schwelle.1;
                bahn.far_end.lat = neu.gegenende.0;
                bahn.far_end.lon = neu.gegenende.1;
                if let Some(k) = neu.kurs_grad {
                    bahn.true_course = k;
                }
            }
            b.achsen.push(a);
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
        if ueber_lage {
            if let Some(a) = b.achsen.last_mut() {
                a.szenerie_bezeichner = Some(s.bezeichner.clone());
                a.grund = Some(match a.grund.take() {
                    Some(g) => format!("zuordnung_ueber_lage, {g}"),
                    None => "zuordnung_ueber_lage".to_string(),
                });
            }
        }
        b.uebernommen.push(bahn.designator.clone());
    }

    // ⚠ Gegenrichtungen bit-gleich halten. Die Zuordnung findet die
    // Gegenschwelle per KOORDINATENVERGLEICH (`lookup_runway_in_nav`,
    // `navigraph_versatz_ft`); in den Navdaten ist das Gegenende der 23
    // bit-gleich die Schwelle der 05. `msfs_achse` rechnet beide Enden
    // getrennt — fast gleich, aber nicht bit-gleich, und dann fehlte der
    // Versatz der Gegenschwelle. Die zuerst geführte Richtung gibt die Lage
    // vor, die andere übernimmt sie umgekehrt.
    if !achse_belastbar {
        for i in 0..aus.runways.len() {
            let (ti, ei) = (&nav.runways[i].threshold, &nav.runways[i].far_end);
            let partner = (0..i).find(|&j| {
                let (tj, ej) = (&nav.runways[j].threshold, &nav.runways[j].far_end);
                tj.lat == ei.lat && tj.lon == ei.lon && ej.lat == ti.lat && ej.lon == ti.lon
            });
            let Some(j) = partner else { continue };
            if !(b.achse_aus_szenerie(&aus.runways[i].designator)
                && b.achse_aus_szenerie(&aus.runways[j].designator))
            {
                continue;
            }
            let (t, e) = (
                (aus.runways[j].far_end.lat, aus.runways[j].far_end.lon),
                (aus.runways[j].threshold.lat, aus.runways[j].threshold.lon),
            );
            let vorgabe_kurs = aus.runways[j].true_course;
            let r = &mut aus.runways[i];
            (r.threshold.lat, r.threshold.lon) = t;
            (r.far_end.lat, r.far_end.lon) = e;
            // Der Bericht von i beschreibt jetzt die Geometrie von j.
            let (di, dj) = (aus.runways[i].designator.clone(), aus.runways[j].designator.clone());
            let quelle_j = b.achsen.iter().find(|a| a.bahn == dj).map(|a| a.kurs_quelle);
            if let (Some(q), Some(a)) = (quelle_j, b.achsen.iter_mut().find(|a| a.bahn == di)) {
                a.kurs_quelle = q;
                let zusatz = format!("geometrie_von({dj})");
                a.grund = Some(match a.grund.take() {
                    Some(g) => format!("{g}, {zusatz}"),
                    None => zusatz,
                });
                aus.runways[i].true_course = if q == KursHerkunft::Navigraph {
                    nav.runways[i].true_course
                } else {
                    (vorgabe_kurs + 180.0).rem_euclid(360.0)
                };
            }
        }
    }

    (aus, b)
}

#[cfg(test)]
pub(crate) mod tests {
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
                kurs_bestaetigt_grad: None,
                kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
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
                kurs_bestaetigt_grad: None,
                kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
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
    /// P-3.63/P-13.04: Schwelle `szenerie.querabstand_hoechst_m` (100 m) —
    /// eine Szeneriebahn 85 m quer zur Navdaten-Bahn wird uebernommen, eine
    /// mit 115 m verworfen. Mutationstest in `grenzwerte.rs`.
    pub(crate) fn schwelle_pruefen() {
        let nav = edhe_nav();
        let kurs = nav.runways[0].true_course;
        let verworfen = |quer_m: f64| {
            let sz = szenerie_aus_mitte("09", (53.6459 + quer_m / 111_320.0, 9.7042), kurs, 1100.0);
            uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei).1.verworfen
        };
        assert!(verworfen(85.0).is_empty());
        assert_eq!(verworfen(115.0), vec!["09"]);
        // P-4.14: MSFS-Kurs nur bis 0,5° neben der Navdaten-Geometrie.
        let geo = Rahmen::aus(&nav.runways[0]).unwrap().kurs_grad();
        let quelle = |k: f64| {
            let sz = szenerie_aus_mitte("09", (53.6459, 9.7042), k, 1100.0);
            uebernimm_szenerie(&nav, &sz, Quelle::MsfsFacility).1.achsen[0].kurs_quelle
        };
        assert_eq!(quelle(geo + 0.45), KursHerkunft::Heading);
        assert_eq!(quelle(geo + 0.55), KursHerkunft::Navigraph);
    }

    #[test]
    fn schwelle_querabstand() {
        schwelle_pruefen();
    }

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

    /// P-4.14 (a): Bei MSFS wird die Navdaten-Achse durch die gemeldete
    /// MITTE gelegt — verschoben, nicht durch die (aus dem Kurs
    /// gerechneten) Szenerie-Schwellen ersetzt. Liegt der gemeldete Kurs
    /// ausserhalb der Toleranz (hier 93,72° gegen 90,0°), bleibt die
    /// Richtung der Navdaten, und die Achse wird nur parallel verschoben.
    #[test]
    fn msfs_achse_laeuft_durch_die_facility_mitte() {
        let nav = edhe_nav();
        let alt = nav.runways[0].clone();
        // Szenerie-Bahn 80 m noerdlich, also LINKS der Landerichtung 09.
        let sz = szenerie("09", 93.72, 55.0, (53.6459 + 80.0 / 111_320.0, 9.6942));

        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::MsfsFacility);
        let neu = &aus.runways[0];
        // Parallel 80 m nach Norden, nicht gedreht.
        let nord = 80.0 / 111_320.0;
        assert!((neu.threshold.lat - (alt.threshold.lat + nord)).abs() < 1e-9);
        assert!((neu.far_end.lat - (alt.far_end.lat + nord)).abs() < 1e-9);
        assert!((neu.threshold.lon - alt.threshold.lon).abs() < 1e-9, "laengs verschoben");
        assert!((neu.true_course - alt.true_course).abs() < 1e-9, "Kurs gedreht");
        // Die Mitte der Szenerie liegt jetzt auf der Achse.
        let (_, quer) = crate::runway::projiziere_auf_bahn(
            neu.threshold.lat,
            neu.threshold.lon,
            neu.far_end.lat,
            neu.far_end.lon,
            53.6459 + nord,
            9.7042,
        );
        assert!(quer.abs() < 0.05, "Mitte {quer:.3} m neben der neuen Achse");
        // Bericht: Herkunft, Mitte, Verschiebung (links = negativ), Gruende.
        assert!(b.achse_aus_szenerie("09"));
        let a = &b.achsen[0];
        assert_eq!(a.herkunft, AchsenHerkunft::SzenerieMitte);
        assert_eq!(a.kurs_quelle, KursHerkunft::Navigraph);
        assert!((a.verschiebung_quer_m.unwrap() + 80.0).abs() < 0.05, "{a:?}");
        assert_eq!(a.verschiebung_laengs_m, Some(0.0));
        let grund = a.grund.as_deref().unwrap();
        assert!(grund.contains("kurs_navigraph") && grund.contains("gegenschwelle_unbekannt"), "{grund}");
        // Die Breite kommt weiter aus der Szenerie.
        assert_eq!(neu.width_ft, Some((55.0_f64 / 0.3048).round() as i32));
    }

    /// EDHE 09/27 als Paar — fuer Laengsverschiebung und Paarung.
    fn edhe_paar() -> NavAirport {
        let mut nav = edhe_nav();
        let mut gegen = nav.runways[0].clone();
        gegen.designator = "27".into();
        gegen.true_course = 269.9957;
        gegen.threshold = nav.runways[0].far_end.clone();
        gegen.far_end = nav.runways[0].threshold.clone();
        nav.runways.push(gegen);
        nav
    }

    fn msfs_szenerie(mitte: (f64, f64), heading_09: f64, bestaetigt_09: Option<f64>) -> SzenerieFlughafen {
        let [a, b] = sim_msfs::facility::bahn_paar(mitte, heading_09, 1100.0, 55.0, 1, (9, 0, 0.0), (27, 0, 0.0));
        let mut a = a;
        a.kurs_bestaetigt_grad = bestaetigt_09;
        SzenerieFlughafen {
            icao: "EDHE".into(),
            quelle: "Test".into(),
            bahnen: vec![a, b],
            ..Default::default()
        }
    }

    /// Kurs: bestaetigt vor HEADING, beides nur in der Toleranz (Paket
    /// 0,5°). In der Toleranz wird die Achse um die Mitte gedreht.
    #[test]
    fn msfs_kurs_bestaetigt_vor_heading_nur_in_der_toleranz() {
        let nav = edhe_paar();
        let geo = Rahmen::aus(&nav.runways[0]).unwrap().kurs_grad();
        let mitte = (53.6459, 9.7042);
        let kurs_von = |aus: &NavAirport| Rahmen::aus(&aus.runways[0]).unwrap().kurs_grad();

        // HEADING 0,3° neben der Geometrie: uebernommen, Achse gedreht.
        let (aus, b) = uebernimm_szenerie(&nav, &msfs_szenerie(mitte, geo + 0.3, None), Quelle::MsfsFacility);
        assert_eq!(b.achsen[0].kurs_quelle, KursHerkunft::Heading);
        assert!((kurs_von(&aus) - (geo + 0.3)).abs() < 0.001, "{}", kurs_von(&aus));
        assert!((aus.runways[0].true_course - (geo + 0.3)).abs() < 1e-9);

        // Bestaetigt 0,4° daneben und nahe HEADING: der bestaetigte gewinnt.
        let (aus, b) = uebernimm_szenerie(&nav, &msfs_szenerie(mitte, geo + 0.3, Some(geo + 0.4)), Quelle::MsfsFacility);
        assert_eq!(b.achsen[0].kurs_quelle, KursHerkunft::Bestaetigt);
        assert!((kurs_von(&aus) - (geo + 0.4)).abs() < 0.001);

        // HEADING 0,6° daneben, nichts bestaetigt: Navdaten-Richtung, nur verschoben.
        let (aus, b) = uebernimm_szenerie(&nav, &msfs_szenerie(mitte, geo + 0.6, None), Quelle::MsfsFacility);
        assert_eq!(b.achsen[0].kurs_quelle, KursHerkunft::Navigraph);
        assert!((kurs_von(&aus) - geo).abs() < 1e-6);
        assert!(b.achse_aus_szenerie("09"), "die Lage kommt trotzdem aus der Mitte");
    }

    /// Laengs nur ohne versetzte Schwelle: Steckt der Versatz im
    /// Navdaten-Schwellenpunkt, ist die Navdaten-Mitte gewollt nicht die
    /// Belagsmitte — dann wird nur quer verschoben.
    #[test]
    fn msfs_laengs_nur_ohne_versetzte_schwelle() {
        let nav = edhe_paar();
        let geo = Rahmen::aus(&nav.runways[0]).unwrap().kurs_grad();
        // Mitte 30 m laengs (Landerichtung 09 = Osten) und 10 m nach Sueden (rechts).
        let cos_b = 53.6459_f64.to_radians().cos();
        let mitte = (53.6459 - 10.0 / 111_320.0, 9.7042 + 30.0 / (111_320.0 * cos_b));
        let (aus, b) = uebernimm_szenerie(&nav, &msfs_szenerie(mitte, geo + 0.1, None), Quelle::MsfsFacility);
        let a = &b.achsen[0];
        assert!((a.verschiebung_laengs_m.unwrap() - 30.0).abs() < 0.05, "{a:?}");
        assert!((a.verschiebung_quer_m.unwrap() - 10.0).abs() < 0.05, "{a:?}");
        assert!(a.grund.is_none(), "{a:?}");
        let r = Rahmen::aus(&aus.runways[0]).unwrap();
        assert!(r.lage(mitte).0.abs() < 0.05 && r.lage(mitte).1.abs() < 0.05);
        // ⚠ Gegenrichtung bit-gleich gepaart — sonst findet die Zuordnung
        // den Versatz der Gegenschwelle nicht.
        assert_eq!(aus.runways[1].threshold.lat, aus.runways[0].far_end.lat);
        assert_eq!(aus.runways[1].threshold.lon, aus.runways[0].far_end.lon);
        assert_eq!(aus.runways[1].far_end.lat, aus.runways[0].threshold.lat);
        assert_eq!(aus.runways[1].far_end.lon, aus.runways[0].threshold.lon);

        // Mit versetzter Schwelle an der 27: nur quer, mit Grund.
        let mut versetzt = nav.clone();
        versetzt.runways[1].displaced_threshold_ft = 500;
        let (aus, b) = uebernimm_szenerie(&versetzt, &msfs_szenerie(mitte, geo + 0.1, None), Quelle::MsfsFacility);
        let a = &b.achsen[0];
        assert_eq!(a.verschiebung_laengs_m, Some(0.0));
        assert!((a.laengs_gemessen_m.unwrap() - 30.0).abs() < 0.05);
        assert!(a.grund.as_deref().unwrap().contains("versetzte_schwelle"));
        let (laengs, quer) = Rahmen::aus(&aus.runways[0]).unwrap().lage(mitte);
        assert!((laengs - 30.0).abs() < 0.05 && quer.abs() < 0.05, "{laengs} {quer}");
    }

    /// Plausibilitaet: Mitte weiter als die halbe Navdaten-Bahnlaenge von
    /// der Navdaten-Mitte → Navdaten-Achse, Grund im Bericht. Die Breite
    /// kommt trotzdem — und genau dann darf die Achse NICHT als aus der
    /// Szenerie gelten (P-4.14 b).
    #[test]
    fn msfs_mitte_zu_weit_bleibt_navigraph_mit_grund() {
        let mut nav = edhe_nav();
        // Kurze Bahn: 600 m. Mitte 350 m laengs — die Zuordnung (400 m)
        // passt, die halbe Laenge (300 m) nicht.
        nav.runways[0].length_ft = (600.0 / 0.3048) as i32;
        let cos_b = 53.6459_f64.to_radians().cos();
        let mitte = (53.6459, 9.7042 + 350.0 / (111_320.0 * cos_b));
        let sz = szenerie_aus_mitte("09", mitte, 90.0, 600.0);
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::MsfsFacility);
        assert_eq!(b.uebernommen, vec!["09"]);
        assert_eq!(aus.runways[0].threshold.lon, nav.runways[0].threshold.lon);
        assert_eq!(aus.runways[0].width_ft, Some((55.0_f64 / 0.3048).round() as i32));
        assert!(!b.achse_aus_szenerie("09"));
        let grund = b.achsen[0].grund.as_deref().unwrap();
        assert!(grund.starts_with("mitte_zu_weit(abstand_m=350"), "{grund}");

        // Gegenprobe: 250 m laengs liegt in der halben Laenge.
        let mitte = (53.6459, 9.7042 + 250.0 / (111_320.0 * cos_b));
        let (_, b) = uebernimm_szenerie(&nav, &szenerie_aus_mitte("09", mitte, 90.0, 600.0), Quelle::MsfsFacility);
        assert!(b.achse_aus_szenerie("09"));
    }

    /// UUDD, Navigraph 2610 (`nav_runways` auf live, lesend; 2611 gleich).
    fn uudd_navigraph() -> NavAirport {
        let z = |d: &str, k: f64, l: i32, w: i32, t: (f64, f64), e: (f64, f64)| NavRunway {
            designator: d.into(),
            magnetic_course: k - 11.7,
            true_course: k,
            length_ft: l,
            width_ft: Some(w),
            surface: Some("CON".into()),
            threshold: punkt(t.0, t.1),
            far_end: punkt(e.0, e.1),
            displaced_threshold_ft: 0,
            ils: None,
            glideslope_angle: 3.0,
            tch_ft: 50,
        };
        let (a, b) = ((55.41694444, 37.91536111), (55.39936944, 37.936475));
        let (c, d) = ((55.42451667, 37.91176667), (55.39633611, 37.94561667));
        let (e, f) = ((55.42106111, 37.87214444), (55.39511944, 37.90335));
        NavAirport {
            cycle: "2610".into(),
            valid_to: String::new(),
            icao: "UUDD".into(),
            name: "DOMODEDOVO".into(),
            latitude: 55.4086,
            longitude: 37.9061,
            elevation_ft: Some(588),
            runways: vec![
                z("13C", 145.696, 7776, 174, a, b),
                z("13L", 145.696, 12467, 197, c, d),
                z("13R", 145.657, 11483, 197, e, f),
                z("31C", 325.713, 7776, 174, b, a),
                z("31L", 325.683, 11483, 197, f, e),
                z("31R", 325.724, 12467, 197, d, c),
            ],
        }
    }

    /// UUDD in der X-Plane-Szenerie: Gateway 90987 (2022), Zeilen 100 der
    /// `apt.dat` — Ersatz für die installierte Szenerie des Piloten, die im
    /// Protokoll nicht steht. Die Bezeichner sind die vor der Umbenennung.
    fn uudd_szenerie(namen: [(&str, &str); 3]) -> SzenerieFlughafen {
        let zeilen = [
            (60.0, (55.424633, 37.911833), (55.396436, 37.945803)),
            (53.0, (55.41695, 37.915344), (55.399375, 37.936464)),
            (60.0, (55.421061, 37.872139), (55.395117, 37.903344)),
        ];
        let mut bahnen = Vec::new();
        for ((breite, p, q), (n1, n2)) in zeilen.into_iter().zip(namen) {
            for (name, s, g) in [(n1, p, q), (n2, q, p)] {
                bahnen.push(SzenerieBahn {
                    bezeichner: name.into(),
                    kurs_grad: Rahmen::aus(&NavRunway { threshold: punkt(s.0, s.1), far_end: punkt(g.0, g.1), ..edhe_nav().runways[0].clone() })
                        .unwrap()
                        .kurs_grad(),
                    breite_m: breite,
                    laenge_m: abstand_m(s, g),
                    versetzte_schwelle_m: 0.0,
                    schwelle: s,
                    gegenende: g,
                    belag_code: 2,
                    kurs_bestaetigt_grad: None,
                    kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
                });
            }
        }
        SzenerieFlughafen {
            icao: "UUDD".into(),
            quelle: "Gateway 90987 (Test)".into(),
            bahnen,
            ..Default::default()
        }
    }

    /// P-4.15 (a)/(c): UUDD (td 1647, X-Plane) bekam `kein_treffer(bahnen=6,
    /// ohne_treffer=6)` — nur weil die Szenerie die Bahnen vor der
    /// Umbenennung führt. Über die Lage wird jede Bahn ihrem Szenerie-Ende
    /// zugeordnet; die Navdaten-Bezeichnung bleibt.
    #[test]
    fn uudd_umbenannte_bahnen_werden_ueber_die_lage_zugeordnet() {
        let nav = uudd_navigraph();
        let sz = uudd_szenerie([("14", "32"), ("14L", "32R"), ("14R", "32L")]);
        let (aus, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.uebernommen.len(), 6, "{b:?}");
        assert!(b.ohne_treffer.is_empty() && b.verworfen.is_empty());
        for (navi, szen) in [("31L", "32L"), ("13R", "14R"), ("31R", "32"), ("13C", "14L")] {
            let a = b.achsen.iter().find(|a| a.bahn == navi).unwrap();
            assert_eq!(a.szenerie_bezeichner.as_deref(), Some(szen), "{navi}");
            assert_eq!(a.grund.as_deref(), Some("zuordnung_ueber_lage"));
            assert!(b.achse_aus_szenerie(navi));
        }
        // Die gelandete 31L liegt jetzt auf der Szenerie-Schwelle 32L.
        let l = aus.runways.iter().find(|r| r.designator == "31L").unwrap();
        assert_eq!((l.threshold.lat, l.threshold.lon), (55.395117, 37.903344));
        assert_eq!(l.designator, "31L", "Identitaet bleibt Navigraph (ILS, Leitung)");

        // Gegenprobe: Traegt die Szenerie die Navigraph-Namen, laeuft die
        // Zuordnung wie bisher ueber den Bezeichner — ohne Lage-Vermerk.
        let gleich = uudd_szenerie([("13L", "31R"), ("13C", "31C"), ("13R", "31L")]);
        let (_, b) = uebernimm_szenerie(&nav, &gleich, Quelle::XPlaneDatei);
        assert_eq!(b.uebernommen.len(), 6);
        assert!(b.achsen.iter().all(|a| a.grund.is_none() && a.szenerie_bezeichner.is_none()), "{b:?}");
    }

    /// P-4.15 (d): Findet sich nichts, stehen die naechsten Szenerie-Bahnen
    /// mit ihrer Lage im Bericht. Und eine gleichnamige Bahn an anderer
    /// Stelle (verworfen) wird nicht ueber die Lage „gerettet".
    #[test]
    fn ohne_uebernahme_stehen_die_naechsten_kandidaten_im_bericht() {
        let nav = edhe_nav();
        // Szenerie-Bahn „27" 3 km entfernt: kein Treffer, aber ein Kandidat.
        let sz = szenerie_aus_mitte("27", (53.6459, 9.7042 + 0.045), 270.0, 1100.0);
        let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.ohne_treffer, vec!["09"]);
        let k = &b.verworfene_kandidaten[0];
        assert_eq!((k.bahn.as_str(), k.szenerie_bahn.as_str()), ("09", "27"));
        assert!((k.laengs_m - 0.045 * 111_320.0 * 53.6459_f64.to_radians().cos()).abs() < 1.0, "{k:?}");
        // Verworfen (Bezeichner passt, Lage nicht) bleibt verworfen.
        let sz = szenerie("09", 269.99, 55.0, (53.6459, 9.7033));
        let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        assert_eq!(b.verworfen, vec!["09"]);
        assert!(b.achsen.is_empty());
    }

    /// X-Plane: die Achse kommt aus der Datei und wird so mitgeschrieben.
    #[test]
    fn xplane_achse_wird_als_szenerie_datei_mitgeschrieben() {
        let nav = edhe_nav();
        let sz = szenerie_aus_mitte("09", (53.6459 - 12.0 / 111_320.0, 9.7042), 93.0, 1100.0);
        let (_, b) = uebernimm_szenerie(&nav, &sz, Quelle::XPlaneDatei);
        let a = &b.achsen[0];
        assert_eq!(a.herkunft, AchsenHerkunft::SzenerieDatei);
        assert!((a.verschiebung_quer_m.unwrap() - 12.0).abs() < 0.05, "{a:?}");
        assert!(b.achse_aus_szenerie("09"));
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
    // Neues Verzeichnis, neue Auskünfte — gemerkte Flughäfen sind veraltet.
    if let Ok(mut merk) = gemerkt().lock() {
        merk.clear();
    }
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
///
/// `pub(crate)`, nicht `pub`: v1.7.17 ruft diese Funktion auch fuer den
/// Abflugplatz auf (Stand-Ernte, siehe `dep_szenerie_auskunft` in
/// `lib.rs`) — unabhaengig von `ergaenze_aus_szenerie`, das Navdaten
/// voraussetzt und nur fuer die Bahnkorrektur gedacht ist.
pub(crate) fn szenerie_flughafen(
    icao: &str,
) -> Option<std::sync::Arc<sim_xplane::szenerie::SzenerieFlughafen>> {
    let schluessel = icao.trim().to_ascii_uppercase();
    if let Some((_, treffer)) =
        gemerkter_eintrag(&schluessel).filter(|(alter, _)| *alter < MERKER_GUELTIG)
    {
        return treffer;
    }
    let ergebnis = szenerie_flughafen_lesen(&schluessel);
    if let Ok(mut merk) = gemerkt().lock() {
        merk.insert(schluessel, (std::time::Instant::now(), ergebnis.clone()));
    }
    ergebnis
}

/// Wie lange ein gemerkter Flughafen ohne neues Lesen gilt.
///
/// Erst das Lesen prüft, ob sich die Szenerie geändert hat (`gueltig()` in
/// `verzeichnis_bereitstellen`). Ein Merker ohne Ablauf hätte eine
/// nachinstallierte oder geänderte Szenerie bis zum Neustart verschluckt
/// (Codex-Abnahme 11.10.2026). `vorladen` frischt ab `MERKER_AUFFRISCHEN` im
/// Hintergrund auf — so ist der Eintrag beim Aufsetzen nie abgelaufen und
/// unter der Flugdaten-Sperre wird nicht gelesen.
const MERKER_GUELTIG: std::time::Duration = std::time::Duration::from_secs(600);
const MERKER_AUFFRISCHEN: std::time::Duration = std::time::Duration::from_secs(300);

/// Alter und Inhalt eines gemerkten Flughafens.
fn gemerkter_eintrag(
    schluessel: &str,
) -> Option<(
    std::time::Duration,
    Option<std::sync::Arc<sim_xplane::szenerie::SzenerieFlughafen>>,
)> {
    let merk = gemerkt().lock().ok()?;
    let (zeit, wert) = merk.get(schluessel)?;
    Some((zeit.elapsed(), wert.clone()))
}

/// Schon gelesene Flughäfen, auch die nicht gefundenen, mit Lesezeitpunkt.
///
/// # Warum (AZU 8710, 11.10.2026)
///
/// Die Bahnkorrelation beim Aufsetzen läuft unter der Sperre der Flugdaten.
/// Ohne Merker prüfte jeder Aufruf Größe und Zeitstempel aller apt.dat und
/// las dann den Flughafen aus der Datei — bei Nimbus-KMCO gut eine Sekunde,
/// in der Aufsetz-Sampler und Positionsstrom standen. Mit `vorladen` liegt
/// der Landeplatz beim Aufsetzen schon hier, und der Nachschlag kostet
/// nichts. Geleert wird der Merker, wenn das Verzeichnis neu gebaut wird.
type Merker = std::collections::HashMap<
    String,
    (
        std::time::Instant,
        // Geteilt (`Arc`): Ein Nachschlag unter der Flugdaten-Sperre
        // kopiert nur einen Zeiger, nie den Flughafen (v2.0.17).
        Option<std::sync::Arc<sim_xplane::szenerie::SzenerieFlughafen>>,
    ),
>;

/// Nur das Alter eines gemerkten Flughafens — OHNE ihn zu kopieren.
///
/// ⚠ `vorladen` laeuft in jedem Streamer-Takt. Bis 2.0.12 las es das Alter
/// ueber `gemerkter_eintrag`, und das kopierte dafuer jedes Mal den
/// ganzen Flughafen samt Rollwegen und Staenden unter der Merker-Sperre
/// (12.10.2026).
fn gemerktes_alter(schluessel: &str) -> Option<std::time::Duration> {
    let merk = gemerkt().lock().ok()?;
    merk.get(schluessel).map(|(zeit, _)| zeit.elapsed())
}

fn gemerkt() -> &'static Mutex<Merker> {
    static MERKER: OnceLock<Mutex<Merker>> = OnceLock::new();
    MERKER.get_or_init(|| Mutex::new(Merker::new()))
}

/// Einen Flughafen im Hintergrund lesen, damit der spätere Nachschlag
/// (beim Aufsetzen unter der Flugdaten-Sperre) aus dem Merker kommt.
///
/// Billig genug für jeden Streamer-Takt: Ist der Flughafen frisch gemerkt
/// oder schon unterwegs, passiert nichts.
pub(crate) fn vorladen(icao: &str) {
    static UNTERWEGS: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let schluessel = icao.trim().to_ascii_uppercase();
    // Nicht nur vierstellig: Ausweichplätze heißen auch `48FA` oder `US-4991`
    // (`correlate_airport_icao`, Codex-Nachprüfung 11.10.2026).
    if schluessel.is_empty() {
        return;
    }
    if gemerktes_alter(&schluessel).is_some_and(|alter| alter < MERKER_AUFFRISCHEN) {
        return;
    }
    let unterwegs = UNTERWEGS.get_or_init(|| Mutex::new(std::collections::HashSet::new()));
    if !unterwegs
        .lock()
        .map(|mut u| u.insert(schluessel.clone()))
        .unwrap_or(false)
    {
        return;
    }
    std::thread::spawn(move || {
        let ergebnis = szenerie_flughafen_lesen(&schluessel);
        if let Ok(mut merk) = gemerkt().lock() {
            merk.insert(schluessel.clone(), (std::time::Instant::now(), ergebnis));
        }
        if let Ok(mut u) = unterwegs.lock() {
            u.remove(&schluessel);
        }
    });
}

fn szenerie_flughafen_lesen(
    icao: &str,
) -> Option<std::sync::Arc<sim_xplane::szenerie::SzenerieFlughafen>> {
    // ⚠ Baut das Verzeichnis notfalls hier — der Vorabbau beim
    // Flugbeginn ist die Regel, nicht die Garantie: Ein Flug, der aus
    // einer Wiederaufnahme kommt, oder ein Simulatorwechsel mitten im
    // Flug haetten ihn sonst nicht gesehen.
    verzeichnis_bereitstellen();
    let zelle = VERZEICHNIS.get_or_init(|| Mutex::new(None));
    let halter = zelle.lock().ok()?;
    halter.as_ref()?.flughafen(icao).map(std::sync::Arc::new)
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
    msfs_auskunft: Option<std::sync::Arc<sim_core::szenerie::SzenerieFlughafen>>,
) -> (
    Option<NavAirport>,
    Option<UebernahmeBericht>,
    Option<std::sync::Arc<sim_core::szenerie::SzenerieFlughafen>>,
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

    /// Das Alter kommt ohne den Flughafen — und nur fuer gemerkte Plaetze.
    #[test]
    fn das_alter_eines_gemerkten_flughafens() {
        // Ohne Zeitrechnung in die Vergangenheit: `Instant - Duration`
        // bricht ab, wenn der Rechner kuerzer laeuft als die Spanne.
        gemerkt()
            .lock()
            .expect("Merker")
            .insert("ZZALT1".to_string(), (std::time::Instant::now(), None));
        let alter = gemerktes_alter("ZZALT1").expect("gemerkt");
        assert!(
            alter < MERKER_AUFFRISCHEN,
            "ein frisch gemerkter Platz gilt schon als faellig"
        );
        assert!(
            gemerktes_alter("ZZALT2").is_none(),
            "ungemerkter Platz hat ein Alter"
        );
    }

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
                kurs_bestaetigt_grad: None,
                kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
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
            crate::arc_some(msfs_auskunft("EDHE", 93.72)),
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
            crate::arc_some(msfs_auskunft("EDDH", 233.0)),
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
            crate::arc_some(sim_core::szenerie::SzenerieFlughafen {
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
                    kurs_bestaetigt_grad: None,
                    kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
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
            crate::arc_some(msfs_auskunft("EDHE", 200.0)),
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
            // (Breite, Länge) — Hamburg. Der zweite Punkt liegt 125 m links
            // neben der Testbahn: Seit 2.1.8 zählt nur ein Rollweg, der die
            // Bahn wirklich verlässt (#1936); bis dahin lag er auf ihr.
            punkte: vec![(53.6304, 9.9882), (53.6312, 9.9870)],
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

    // v3 (Etappe 0): `lib.rs` samt ausgelagerter Teile (`build.rs`).
    const LIB: &str = crate::quelle_gesamt::PRODUKTION;

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
                kurs_bestaetigt_grad: None,
                kurs_quelle: sim_core::szenerie::KursQuelle::Unbestaetigt,
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
