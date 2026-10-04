//! X-Plane-HUD-Band: die Anzeige des MSFS-Panels (`panel.js`) als fertige
//! Textzeilen fuer das Plugin (ADR-0005).
//!
//! Grundsatz: die App rechnet, das Plugin zeichnet. Dieses Modul ist die
//! Portierung von `msfs-panel/.../AeroACARSPanel/panel.js` (`lage`,
//! `zeichne`, `hoehenZelle`, `zeitZelle`, `wxAusDekodiert`, `wxText`,
//! `vsFarbe`, `beladungsFarbe`, `istSoll`, `verstrichen`, `uhrzeitZ`,
//! `alter`, `kennung`, `kuerze`, `nurAscii`, `zahl`, `alsDauer`). Die Eingaben
//! sind dieselben JSON-Werte, die `/panel/status`, `/panel/debrief` und
//! `/panel/activity` liefern — so bleibt das Modul frei von App-Typen.
//!
//! PFLEGE: `panel.js` und dieses Modul sind zwei Quellen DERSELBEN Anzeige.
//! Aenderungen am Band immer in beiden machen (siehe ADR-0005, Abschnitt
//! „Pflege").
//!
//! Garantien (Typ-Invarianten, siehe [`pruefe_datagramm`]): nur `0x20–0x7E`
//! und TAB, hoechstens 4 Zeilen, hoechstens 512 Byte je Zeile, keine TAB oder
//! Zeilenumbrueche im Lauftext. Texte laufen bei der Erzeugung eines [`Lauf`]
//! durch [`nur_ascii`]; nichts anderes kann in ein Datagramm gelangen.

use serde_json::Value;

/// Hoechste Zeilenlaenge in Byte (ohne Zeilenende), ADR-0005.
pub const MAX_ZEILE_BYTES: usize = 512;
/// Hoechstens so viele Zeilen je Band.
pub const MAX_ZEILEN: usize = 4;
/// Zeichenlimit der Ticker-Meldung (`TICKER_MAX` in panel.js).
pub const TICKER_MAX: usize = 100;
/// Takt der Zeitzelle (ETE -> ETA -> FLT). panel.js v3.10: 30 s (v3.9 hatte
/// 8 s; zu hektisch, Pilotenbefund 11.08.2026).
pub const ZEIT_ROTATION_MS: i64 = 30_000;
/// Hoechstens so viele Zellen in der Datenzeile (`K.zellen` in panel.js).
const MAX_ZELLEN: usize = 4;
const BELADUNG_MIN: f64 = 0.98;
const BELADUNG_MAX: f64 = 1.05;

// =============================================================================
// Typen
// =============================================================================

/// Farbe eines Laufs (ein Zeichen auf dem Draht).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Farbe {
    /// `n` normal
    Normal,
    /// `d` gedaempft (Beschriftung)
    Gedaempft,
    /// `g` gut
    Gut,
    /// `w` Warnung
    Warnung,
    /// `b` schlecht
    Schlecht,
    /// `a` Akzent
    Akzent,
}

impl Farbe {
    pub fn zeichen(self) -> char {
        match self {
            Farbe::Normal => 'n',
            Farbe::Gedaempft => 'd',
            Farbe::Gut => 'g',
            Farbe::Warnung => 'w',
            Farbe::Schlecht => 'b',
            Farbe::Akzent => 'a',
        }
    }
}

/// Art eines Laufs (zweites Zeichen auf dem Draht, ADR-0005 Nachtrag): sagt
/// dem Plugin, welches panel.css-Element der Lauf ist (Schrift, Abstand).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Art {
    /// `t` Ticker-Alter (`.aa2-age`)
    Alter,
    /// `m` Ticker-Meldung (`.aa2-msg`)
    Meldung,
    /// `p` Statuspunkt (`.aa2-dot`), Text immer leer
    Punkt,
    /// `i` Kennung (`.aa2-ident`)
    Kennung,
    /// `s` Lage-Text (`.aa2-state`)
    Lage,
    /// `l` Zellen-Beschriftung (`.aa2-lbl`)
    Beschriftung,
    /// `v` Zellen-Wert (`.aa2-val`)
    Wert,
    /// `x` Anhang (`.aa2-tail`)
    Anhang,
    /// `z` Landenote (`.aa2-val.aa2-xl`)
    Note,
    /// `e` Noten-Etikett (`.aa2-band`, Pille)
    Etikett,
}

impl Art {
    pub fn zeichen(self) -> char {
        match self {
            Art::Alter => 't',
            Art::Meldung => 'm',
            Art::Punkt => 'p',
            Art::Kennung => 'i',
            Art::Lage => 's',
            Art::Beschriftung => 'l',
            Art::Wert => 'v',
            Art::Anhang => 'x',
            Art::Note => 'z',
            Art::Etikett => 'e',
        }
    }
}

/// Ein Lauf: Farbe, Art und Text (ASCII, ohne TAB/Zeilenumbruch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lauf {
    farbe: Farbe,
    art: Art,
    text: String,
}

impl Lauf {
    /// Lauf mit Text; der Text geht durch [`nur_ascii`]. Fuer
    /// [`Art::Punkt`] bleibt der Text immer leer.
    pub fn neu(farbe: Farbe, art: Art, text: &str) -> Self {
        let text = if art == Art::Punkt {
            String::new()
        } else {
            nur_ascii(text)
        };
        Self { farbe, art, text }
    }

    /// Gefuellter Statuspunkt in `farbe`.
    pub fn punkt(farbe: Farbe) -> Self {
        Self::neu(farbe, Art::Punkt, "")
    }

    pub fn farbe(&self) -> Farbe {
        self.farbe
    }

    pub fn art(&self) -> Art {
        self.art
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Eine Zeile des Bands: durch TAB getrennte Laeufe.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Zeile {
    laeufe: Vec<Lauf>,
}

impl Zeile {
    pub fn laeufe(&self) -> &[Lauf] {
        &self.laeufe
    }

    /// Drahtform, hoechstens [`MAX_ZEILE_BYTES`] Byte. Zu Langes wird im
    /// letzten passenden Lauf mit `...` gekuerzt (alles ist ASCII, also nie
    /// mitten in einem UTF-8-Zeichen); eine Zeile ohne Laeufe ist leer.
    pub fn zu_text(&self) -> String {
        let mut out = String::new();
        for (i, l) in self.laeufe.iter().enumerate() {
            let sep = usize::from(i > 0);
            if out.len() + sep + 2 > MAX_ZEILE_BYTES {
                break;
            }
            let frei = MAX_ZEILE_BYTES - out.len() - sep - 2;
            if i > 0 {
                out.push('\t');
            }
            out.push(l.farbe.zeichen());
            out.push(l.art.zeichen());
            if l.text.len() <= frei {
                out.push_str(&l.text);
            } else {
                if frei >= 3 {
                    out.push_str(&l.text[..frei - 3]);
                    out.push_str("...");
                }
                break;
            }
        }
        out
    }
}

/// Die zehn Lagen des Bands (`lage()` in panel.js).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lage {
    Getrennt,
    Bereit,
    Pausiert,
    Unterwegs,
    Anflug,
    Auswertung,
    Ergebnis,
    Rollen,
    AmStand,
    Eingereicht,
}

/// Ein fertiges Band: Lage, Ruhig-Flag und Zeilen (Ticker, Datenzeile).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BandFrame {
    pub lage: Lage,
    /// `aa2-quiet`: nur `unterwegs` und `bereit`.
    pub ruhig: bool,
    pub zeilen: Vec<Zeile>,
}

impl BandFrame {
    /// `BAND <seq> <ruhig> <zeilen>` plus Zeilen, jede mit `\n` beendet.
    /// `seq` wird auf 0 … 2^31-1 gebracht; mehr als 4 Zeilen werden
    /// abgeschnitten, kein Inhalt ergibt eine leere Zeile.
    pub fn zu_datagramm(&self, seq: u32, ruhig: bool) -> String {
        let mut zeilen: Vec<String> = self
            .zeilen
            .iter()
            .take(MAX_ZEILEN)
            .map(Zeile::zu_text)
            .collect();
        if zeilen.is_empty() {
            zeilen.push(String::new());
        }
        let mut s = format!(
            "BAND {} {} {}\n",
            seq & 0x7FFF_FFFF,
            u8::from(ruhig),
            zeilen.len()
        );
        for z in zeilen {
            s.push_str(&z);
            s.push('\n');
        }
        s
    }
}

/// `BAND <seq> 0 0` — Band ausblenden (Einstellung aus / Abmeldung).
pub fn aus_datagramm(seq: u32) -> String {
    format!("BAND {} 0 0\n", seq & 0x7FFF_FFFF)
}

/// Eingaben von [`baue_band`]: dieselben Daten, die panel.js von den drei
/// `/panel/*`-Routen holt.
#[derive(Debug, Clone, Copy)]
pub struct BandEingabe<'a> {
    /// Ist die Quelle erreichbar? (Aus der App heraus immer `true`.)
    pub verbunden: bool,
    /// `/panel/status`; `None` oder JSON `null` = kein Flug.
    pub status: Option<&'a Value>,
    /// `/panel/debrief` (Landeprotokoll), falls schon geholt.
    pub debrief: Option<&'a Value>,
    /// Juengster Eintrag von `/panel/activity`.
    pub aktivitaet: Option<&'a Value>,
    /// Jetzt, Millisekunden seit 1970 (UTC).
    pub jetzt_ms: i64,
    /// Nur fuer `Getrennt`: Anzeige „Keine Verbindung - host:port".
    pub host: &'a str,
    pub port: u16,
    pub fehler_text: Option<&'a str>,
    pub fehler_seit_ms: Option<i64>,
}

// =============================================================================
// Text-Helfer (Port von nurAscii, kuerze, zahl, ...)
// =============================================================================

/// `nurAscii()` aus panel.js: typografische Zeichen auf ASCII abbilden, der
/// Rest ausserhalb `0x20–0x7E` faellt weg (auch TAB und Zeilenumbruch).
pub fn nur_ascii(t: &str) -> String {
    let mut s = String::with_capacity(t.len());
    for c in t.chars() {
        match c {
            '→' | '▶' | '►' | '➤' => s.push('>'),
            '·' | '–' | '—' | '•' | '●' => s.push('-'),
            '✓' => s.push_str("OK"),
            '°' => {}
            '„' | '“' | '”' => s.push('"'),
            '◀' | '◄' => s.push('<'),
            '×' | '✕' | '✖' => s.push('x'),
            '⚠' => s.push('!'),
            '…' => s.push_str("..."),
            '‚' | '‘' | '’' => s.push('\''),
            'ä' => s.push_str("ae"),
            'ö' => s.push_str("oe"),
            'ü' => s.push_str("ue"),
            'Ä' => s.push_str("Ae"),
            'Ö' => s.push_str("Oe"),
            'Ü' => s.push_str("Ue"),
            'ß' => s.push_str("ss"),
            c if ('\x20'..='\x7e').contains(&c) => s.push(c),
            _ => {}
        }
    }
    s
}

/// `kuerze()`: Ticker-Meldung auf [`TICKER_MAX`] Zeichen (Eingabe ist schon
/// ASCII), an der letzten Wortgrenze, wenn dabei nicht zu viel verloren geht.
pub fn kuerze(t: &str) -> String {
    if t.len() <= TICKER_MAX {
        return t.to_string();
    }
    // lastIndexOf(' ', TICKER_MAX - 3): letztes Leerzeichen bei Index <= 97.
    let obergrenze = (TICKER_MAX - 3).min(t.len() - 1);
    let mut schnitt: i64 = t.as_bytes()[..=obergrenze]
        .iter()
        .rposition(|b| *b == b' ')
        .map_or(-1, |p| p as i64);
    if schnitt < (TICKER_MAX as i64 - 25) {
        schnitt = TICKER_MAX as i64 - 3;
    }
    format!("{}...", &t[..schnitt as usize])
}

/// JavaScripts `Math.round` (halbe Werte nach oben, auch bei negativen).
fn rund(x: f64) -> i64 {
    (x + 0.5).floor() as i64
}

/// Zahl wie JavaScripts `String(n)`: ganze Zahlen ohne Nachkommastellen.
fn js_zahl(x: f64) -> String {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        format!("{}", x as i64)
    } else {
        format!("{x}")
    }
}

/// JavaScripts `toFixed`: auf der exakten Dezimalentwicklung, halbe Werte
/// nach oben (Betrag), negatives Vorzeichen auch bei „-0".
fn to_fixed(x: f64, stellen: usize) -> String {
    let neg = x < 0.0;
    let voll = format!("{:.*}", stellen + 30, x.abs());
    let (ganz, bruch) = voll.split_once('.').unwrap_or((&voll, ""));
    let mut ziffern: Vec<u8> = ganz.bytes().collect();
    ziffern.extend(bruch.bytes().take(stellen));
    let aufrunden = bruch.as_bytes().get(stellen).is_some_and(|b| *b >= b'5');
    if aufrunden {
        let mut i = ziffern.len();
        loop {
            if i == 0 {
                ziffern.insert(0, b'1');
                break;
            }
            i -= 1;
            if ziffern[i] == b'9' {
                ziffern[i] = b'0';
            } else {
                ziffern[i] += 1;
                break;
            }
        }
    }
    let ganz_len = ziffern.len() - stellen;
    let mut s = String::new();
    if neg {
        s.push('-');
    }
    s.push_str(std::str::from_utf8(&ziffern[..ganz_len]).unwrap_or("0"));
    if stellen > 0 {
        s.push('.');
        s.push_str(std::str::from_utf8(&ziffern[ganz_len..]).unwrap_or(""));
    }
    s
}

/// Die letzten `n` Zeichen (`slice(-n)`); ASCII.
fn letzte(s: &str, n: usize) -> String {
    let b = s.as_bytes();
    String::from_utf8_lossy(&b[b.len().saturating_sub(n)..]).into_owned()
}

fn feld<'a>(v: Option<&'a Value>, k: &str) -> Option<&'a Value> {
    v?.get(k).filter(|x| !x.is_null())
}

fn zahl_von(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Number(n)) => n.as_f64(),
        _ => None,
    }
}

/// JavaScript-Wahrheitswert.
fn wahr(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Nicht leerer Text (Zahlen wie in JS umgewandelt), sonst `None`.
fn text_von(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Number(n)) => n.as_f64().map(js_zahl),
        _ => None,
    }
}

fn zeit_ms(v: Option<&Value>) -> Option<i64> {
    let Some(Value::String(s)) = v else {
        return None;
    };
    if s.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis())
}

/// `zahl(v, stellen)`: Zahl mit festen Nachkommastellen, sonst `None`.
fn zahl(v: Option<&Value>, stellen: usize) -> Option<String> {
    zahl_von(v).map(|x| to_fixed(x, stellen))
}

/// `alsDauer(min)`: „1h04m".
fn als_dauer(min: f64) -> Option<String> {
    if min < 0.0 {
        return None;
    }
    let h = (min / 60.0).floor();
    let m = min % 60.0;
    let mm = js_zahl(m);
    Some(if h > 0.0 {
        format!("{}h{}{}m", js_zahl(h), if m < 10.0 { "0" } else { "" }, mm)
    } else {
        format!("{mm}m")
    })
}

/// `verstrichen(iso)`: „2 h 5 min" / „45 min".
fn verstrichen(iso: Option<&Value>, jetzt_ms: i64) -> Option<String> {
    let ms = zeit_ms(iso)?;
    let min = (jetzt_ms - ms).div_euclid(60_000);
    if min < 0 {
        return None;
    }
    let h = min / 60;
    Some(format!(
        "{}{} min",
        if h > 0 {
            format!("{h} h ")
        } else {
            String::new()
        },
        min % 60
    ))
}

/// `uhrzeitZ(iso)`: „HH:MM:SSZ" (UTC).
fn uhrzeit_z(iso: Option<&Value>) -> Option<String> {
    let ms = zeit_ms(iso)?;
    let t = ms.div_euclid(1000).rem_euclid(86_400);
    Some(format!(
        "{:02}:{:02}:{:02}Z",
        t / 3600,
        (t % 3600) / 60,
        t % 60
    ))
}

/// `alter(iso)`: „vor 3 s" / „vor 5 min" / „vor 2 h"; leer ohne Zeit.
fn alter(iso: Option<&Value>, jetzt_ms: i64) -> String {
    let Some(ms) = zeit_ms(iso) else {
        return String::new();
    };
    let s = ((jetzt_ms - ms).div_euclid(1000)).max(0);
    if s < 60 {
        return format!("vor {s} s");
    }
    let m = s / 60;
    if m < 60 {
        return format!("vor {m} min");
    }
    format!("vor {} h", m / 60)
}

/// `istSoll(ist, soll)`: „1200 / 1250" oder nur „1200 kg" ohne Planwert.
fn ist_soll(ist: Option<&Value>, soll: Option<&Value>) -> Option<String> {
    let ist = zahl_von(ist)?;
    let links = rund(ist).to_string();
    match zahl_von(soll) {
        Some(s) if s > 0.0 => Some(format!("{links} / {}", rund(s))),
        _ => Some(format!("{links} kg")),
    }
}

fn beladungs_farbe(ist: Option<&Value>, soll: Option<&Value>) -> Farbe {
    match (zahl_von(ist), zahl_von(soll)) {
        (Some(i), Some(s)) if s > 0.0 => {
            let a = i / s;
            if (BELADUNG_MIN..=BELADUNG_MAX).contains(&a) {
                Farbe::Gut
            } else {
                Farbe::Warnung
            }
        }
        _ => Farbe::Normal,
    }
}

/// Sinkraten-Korridor (`vsFarbe`).
fn vs_farbe(live: Option<&Value>, gleitwinkel: Option<&Value>) -> Farbe {
    let Some(vs) = zahl_von(feld(live, "vertical_speed_fpm")) else {
        return Farbe::Normal;
    };
    let gs = zahl_von(feld(live, "gs_kt")).unwrap_or(0.0);
    if gs < 40.0 {
        return Farbe::Normal;
    }
    let grad = match zahl_von(gleitwinkel) {
        Some(g) if (2.0..=7.5).contains(&g) => g,
        _ => 3.0,
    };
    let ziel = gs * (grad * std::f64::consts::PI / 180.0).tan() * 101.3;
    let ist = -vs;
    if ist < ziel * 0.55 || ist > ziel * 1.45 {
        Farbe::Schlecht
    } else if ist < ziel * 0.75 || ist > ziel * 1.25 {
        Farbe::Warnung
    } else {
        Farbe::Gut
    }
}

/// `kennung(s)`: Callsign, sonst Airline + Flugnummer, sonst „AeroACARS".
fn kennung(s: Option<&Value>) -> String {
    if s.is_none() {
        return "AeroACARS".into();
    }
    if let Some(c) = text_von(feld(s, "callsign")) {
        return c;
    }
    let a = text_von(feld(s, "airline_icao")).unwrap_or_default();
    let f = text_von(feld(s, "flight_number")).unwrap_or_default();
    let z = format!("{a}{f}");
    if z.is_empty() {
        "AeroACARS".into()
    } else {
        z
    }
}

// =============================================================================
// Zellen
// =============================================================================

struct Zelle {
    lbl: String,
    val: Option<String>,
    farbe: Farbe,
}

fn zelle(lbl: &str, val: Option<String>, farbe: Farbe) -> Option<Zelle> {
    Some(Zelle {
        lbl: lbl.to_string(),
        val,
        farbe,
    })
}

/// `hoehenZelle(live)`: FL / ALT / AGL, phasenrichtig.
fn hoehen_zelle(live: Option<&Value>) -> Option<Zelle> {
    if live.is_none() {
        return zelle("AGL", None, Farbe::Normal);
    }
    let p = zahl_von(feld(live, "altitude_pressure_ft"));
    let qnh_std = zahl_von(feld(live, "qnh_hpa")).is_some_and(|q| (q - 1013.25).abs() < 0.6);
    if let Some(p) = p {
        if p >= 17500.0 || qnh_std {
            let fl = rund(p / 100.0);
            let v = if fl < 100 {
                letzte(&format!("00{fl}"), 3)
            } else {
                fl.to_string()
            };
            return zelle("FL", Some(v), Farbe::Normal);
        }
    }
    if let Some(msl) = zahl_von(feld(live, "altitude_msl_ft")) {
        return zelle(
            "ALT",
            Some(format!("{} ft", rund(msl / 100.0) * 100)),
            Farbe::Normal,
        );
    }
    zelle(
        "AGL",
        zahl_von(feld(live, "altitude_agl_ft")).map(|a| format!("{} ft", rund(a / 100.0) * 100)),
        Farbe::Normal,
    )
}

/// `zeitZelle(s, live)`: rotiert ETE -> ETA -> FLT im Takt
/// [`ZEIT_ROTATION_MS`]; Kandidaten ohne Daten fallen aus der Rotation.
fn zeit_zelle(s: Option<&Value>, live: Option<&Value>, jetzt_ms: i64) -> Option<Zelle> {
    let mut k: Vec<(&str, Option<String>)> = Vec::new();
    if let Some(ete) = zahl_von(feld(live, "ete_min")) {
        k.push(("ETE", als_dauer(ete)));
        let ms = jetzt_ms.saturating_add((ete * 60_000.0).trunc() as i64);
        let min_des_tages = ms.div_euclid(60_000).rem_euclid(1440);
        k.push((
            "ETA",
            Some(format!(
                "{:02}:{:02}z",
                min_des_tages / 60,
                min_des_tages % 60
            )),
        ));
    }
    if wahr(feld(s, "takeoff_at")) {
        k.push(("FLT", verstrichen(feld(s, "takeoff_at"), jetzt_ms)));
    }
    if k.is_empty() {
        return None;
    }
    let i = (jetzt_ms.div_euclid(ZEIT_ROTATION_MS)).rem_euclid(k.len() as i64) as usize;
    let (l, v) = k.swap_remove(i);
    zelle(l, v, Farbe::Normal)
}

// =============================================================================
// Wetter
// =============================================================================

/// `wxAusDekodiert(d)`: Kurzform aus den vom Server dekodierten Feldern.
fn wx_aus_dekodiert(d: Option<&Value>) -> Option<String> {
    d?;
    let mut teile: Vec<String> = Vec::new();
    if let Some(i) = text_von(feld(d, "icao")) {
        teile.push(nur_ascii(&i));
    }
    if let Some(w) = text_von(feld(d, "weather")) {
        teile.push(nur_ascii(&w));
    }
    if let Some(ws) = zahl_von(feld(d, "wind_speed_kt")) {
        let richtung = match zahl_von(feld(d, "wind_direction_deg")) {
            Some(r) => letzte(&format!("00{}", rund(r)), 3),
            None => "VRB".to_string(),
        };
        let boe = zahl_von(feld(d, "gust_kt")).map_or(String::new(), |g| format!("G{}", rund(g)));
        teile.push(format!("{richtung}/{}{boe}kt", rund(ws)));
    }
    if let Some(v) = zahl_von(feld(d, "visibility_m")) {
        teile.push(if v >= 9999.0 {
            "VIS 10km+".to_string()
        } else if v >= 5000.0 {
            format!("VIS {}km", rund(v / 1000.0))
        } else {
            format!("VIS {}m", js_zahl(v))
        });
    }
    if let Some(Value::Array(schichten)) = feld(d, "cloud_layers") {
        // Tiefste Schicht, die eine Decke bildet; sonst tiefste ueberhaupt.
        let mut beste: Option<(String, f64, bool)> = None;
        for c in schichten {
            let Some(base) = zahl_von(c.get("base_ft")) else {
                continue;
            };
            let cover = c
                .get("cover")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let deckt =
                cover.starts_with("BKN") || cover.starts_with("OVC") || cover.starts_with("VV");
            let nehmen = match &beste {
                None => true,
                Some((_, bb, bd)) => (deckt && !*bd) || (deckt == *bd && base < *bb),
            };
            if nehmen {
                beste = Some((cover, base, deckt));
            }
        }
        if let Some((cover, base, _)) = beste {
            teile.push(format!(
                "{}{}",
                nur_ascii(&cover),
                letzte(&format!("00{}", rund(base / 100.0)), 3)
            ));
        }
    }
    if let (Some(t), Some(td)) = (
        zahl_von(feld(d, "temperature_c")),
        zahl_von(feld(d, "dewpoint_c")),
    ) {
        let g = |x: f64| {
            let r = rund(x);
            format!(
                "{}{}",
                if r < 0 { "M" } else { "" },
                letzte(&format!("0{}", r.abs()), 2)
            )
        };
        teile.push(format!("{}/{}", g(t), g(td)));
    }
    if let Some(q) = zahl_von(feld(d, "qnh_hpa")) {
        teile.push(format!("Q{}", rund(q)));
    }
    (teile.len() >= 2).then(|| teile.join(" "))
}

fn alle_ziffern(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `^(\d{3}|VRB)\d{2,3}(G\d{2,3})?(KT|MPS)$`
fn ist_wind(w: &str) -> bool {
    let rest = if let Some(r) = w.strip_prefix("VRB") {
        r
    } else if w.len() >= 3 && alle_ziffern(&w[..3]) {
        &w[3..]
    } else {
        return false;
    };
    for n in [3usize, 2] {
        if rest.len() < n || !alle_ziffern(&rest[..n]) {
            continue;
        }
        let r = &rest[n..];
        if r == "KT" || r == "MPS" {
            return true;
        }
        if let Some(g) = r.strip_prefix('G') {
            for m in [3usize, 2] {
                if g.len() >= m && alle_ziffern(&g[..m]) {
                    let e = &g[m..];
                    if e == "KT" || e == "MPS" {
                        return true;
                    }
                }
            }
        }
    }
    false
}

/// `^(VC)?[+-]?(MI|PR|BC|DR|BL|SH|TS|FZ)?(DZ|RA|...)+$`
fn ist_wetter(w: &str) -> bool {
    const DESK: [&str; 8] = ["MI", "PR", "BC", "DR", "BL", "SH", "TS", "FZ"];
    const PHAEN: [&str; 21] = [
        "DZ", "RA", "SN", "SG", "IC", "PL", "GR", "GS", "UP", "BR", "FG", "FU", "VA", "DU", "SA",
        "HZ", "PO", "SQ", "FC", "SS", "DS",
    ];
    fn phaenomene(r: &str) -> bool {
        !r.is_empty()
            && r.len() % 2 == 0
            && (0..r.len() / 2).all(|i| PHAEN.contains(&&r[2 * i..2 * i + 2]))
    }
    let mut starts = vec![w];
    if let Some(r) = w.strip_prefix("VC") {
        starts.push(r);
    }
    for s in starts {
        let s = s.strip_prefix(['+', '-']).unwrap_or(s);
        if phaenomene(s) {
            return true;
        }
        if s.len() >= 2 && DESK.contains(&&s[..2]) && phaenomene(&s[2..]) {
            return true;
        }
    }
    false
}

/// `^(FEW|SCT|BKN|OVC|VV)(\d{3}|\/\/\/)(CB|TCU)?$` -> (Abdeckung, Hoehe, CB/TCU)
fn ist_wolke(w: &str) -> Option<(&str, &str, &str)> {
    for p in ["FEW", "SCT", "BKN", "OVC", "VV"] {
        let Some(r) = w.strip_prefix(p) else {
            continue;
        };
        if r.len() < 3 {
            continue;
        }
        let h = &r[..3];
        if !(alle_ziffern(h) || h == "///") {
            continue;
        }
        let e = &r[3..];
        if e.is_empty() || e == "CB" || e == "TCU" {
            return Some((p, h, e));
        }
    }
    None
}

/// `^M?\d{2}\/M?\d{2}$`
fn ist_temp(w: &str) -> bool {
    let teil = |s: &str| {
        let s = s.strip_prefix('M').unwrap_or(s);
        s.len() == 2 && alle_ziffern(s)
    };
    w.split_once('/').is_some_and(|(a, b)| teil(a) && teil(b))
}

/// `^R\d{2}[LRC]?\/`
fn ist_rvr(w: &str) -> bool {
    let b = w.as_bytes();
    if b.len() < 4 || b[0] != b'R' || !b[1].is_ascii_digit() || !b[2].is_ascii_digit() {
        return false;
    }
    let r = &b[3..];
    r[0] == b'/' || (matches!(r[0], b'L' | b'R' | b'C') && r.get(1) == Some(&b'/'))
}

fn wort_zeichen(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `wxText(raw)`: METAR-Rohtext parsen (Rueckfall fuer Server ohne
/// dekodierte Felder).
fn wx_text(raw: Option<&Value>) -> Option<String> {
    let raw = match raw {
        Some(Value::String(s)) if !s.is_empty() => s.as_str(),
        Some(Value::Number(n)) => return wx_text(Some(&Value::String(js_zahl(n.as_f64()?)))),
        _ => return None,
    };
    let ascii = nur_ascii(raw);
    let mut t = ascii
        .split(' ')
        .filter(|x| !x.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let mut station: Option<&str> = None;
    let (mut wind, mut sicht, mut temp, mut druck) = (None, None, None, None);
    let mut wetter: Vec<&str> = Vec::new();
    let mut wolke_tief: Option<String> = None;
    let mut wolke_tief_ft = f64::INFINITY;
    let mut konvektiv: Option<&str> = None;
    let mut kern = 0;
    let tok: Vec<&str> = t.split(' ').collect();
    for (i, w) in tok.iter().copied().enumerate() {
        if matches!(w, "BECMG" | "TEMPO" | "NOSIG" | "RMK") {
            break;
        }
        if matches!(w, "METAR" | "SPECI" | "AUTO" | "COR") {
            continue;
        }
        if i < 2 && station.is_none() && w.len() == 4 && w.bytes().all(|b| b.is_ascii_uppercase()) {
            station = Some(w);
            continue;
        }
        if w.len() == 7 && alle_ziffern(&w[..6]) && w.ends_with('Z') {
            continue;
        }
        if w.len() == 7 && alle_ziffern(&w[..3]) && w.as_bytes()[3] == b'V' && alle_ziffern(&w[4..])
        {
            continue;
        }
        if ist_rvr(w) {
            continue;
        }
        if ist_wind(w) {
            wind = Some(w);
            kern += 1;
            continue;
        }
        if w == "CAVOK" {
            sicht = Some("CAVOK");
            kern += 1;
            continue;
        }
        if (w.len() == 4 && alle_ziffern(w))
            || (w.len() == 7 && alle_ziffern(&w[..4]) && &w[4..] == "NDV")
        {
            if &w[..4] != "9999" {
                sicht = Some(&w[..4]);
            }
            kern += 1;
            continue;
        }
        if w.ends_with("SM") {
            if w != "10SM" {
                sicht = Some(w);
            }
            kern += 1;
            continue;
        }
        if ist_wetter(w) {
            if wetter.len() < 3 {
                wetter.push(w);
            }
            kern += 1;
            continue;
        }
        if let Some((art, hoehe, zusatz)) = ist_wolke(w) {
            kern += 1;
            if !zusatz.is_empty() {
                konvektiv = Some(zusatz);
            }
            if matches!(art, "BKN" | "OVC" | "VV") && alle_ziffern(hoehe) {
                let ft: f64 = hoehe.parse().unwrap_or(f64::INFINITY);
                if ft < wolke_tief_ft {
                    wolke_tief_ft = ft;
                    wolke_tief = Some(format!("{art}{hoehe}"));
                }
            }
            continue;
        }
        if ist_temp(w) {
            temp = Some(w);
            kern += 1;
            continue;
        }
        if w.len() == 5 && matches!(w.as_bytes()[0], b'Q' | b'A') && alle_ziffern(&w[1..]) {
            druck = Some(w);
            kern += 1;
            continue;
        }
    }
    if kern < 2 {
        // Rueckfall: Rohtext ohne Fuellwoerter, an der Wortgrenze gekuerzt.
        for p in ["METAR ", "SPECI "] {
            if let Some(r) = t.strip_prefix(p) {
                t = r.to_string();
                break;
            }
        }
        // \b\d{6}Z\b ?  (erstes Vorkommen)
        let b = t.as_bytes().to_vec();
        let mut p = 0;
        while p + 7 <= b.len() {
            if alle_ziffern(&t[p..p + 6])
                && b[p + 6] == b'Z'
                && (p == 0 || !wort_zeichen(b[p - 1]))
                && b.get(p + 7).is_none_or(|c| !wort_zeichen(*c))
            {
                let mut ende = p + 7;
                if b.get(ende) == Some(&b' ') {
                    ende += 1;
                }
                t = format!("{}{}", &t[..p], &t[ende..]);
                break;
            }
            p += 1;
        }
        // ` (NOSIG|RMK .*)$`
        let mut schnitt: Option<usize> = t.find(" RMK ");
        if t.ends_with(" NOSIG") {
            let q = t.len() - 6;
            schnitt = Some(schnitt.map_or(q, |s| s.min(q)));
        }
        if let Some(s) = schnitt {
            t.truncate(s);
        }
        if t.len() <= 48 {
            return Some(t);
        }
        let mut schnitt: i64 = t.as_bytes()[..=45]
            .iter()
            .rposition(|c| *c == b' ')
            .map_or(-1, |q| q as i64);
        if schnitt < 28 {
            schnitt = 45;
        }
        return Some(format!("{}...", &t[..schnitt as usize]));
    }
    let mut teile: Vec<String> = Vec::new();
    if let Some(s) = station {
        teile.push(s.to_string());
    }
    if let Some(w) = wind {
        teile.push(w.to_string());
    }
    if let Some(s) = sicht {
        teile.push(s.to_string());
    }
    teile.extend(wetter.iter().map(|s| s.to_string()));
    if let Some(w) = wolke_tief {
        teile.push(w);
    }
    if let Some(k) = konvektiv {
        teile.push(k.to_string());
    }
    if let Some(t) = temp {
        teile.push(t.to_string());
    }
    if let Some(d) = druck {
        teile.push(d.to_string());
    }
    Some(teile.join(" "))
}

fn wx(dekodiert: Option<&Value>, roh: Option<&Value>) -> Option<String> {
    wx_aus_dekodiert(dekodiert).or_else(|| wx_text(roh))
}

// =============================================================================
// Lage und Aufbau
// =============================================================================

const APPROACH: [&str; 2] = ["approach", "final"];
const NACH_TOUCHDOWN: [&str; 5] = [
    "landing",
    "taxi_in",
    "blocks_on",
    "arrived",
    "pirep_submitted",
];

fn phase_text(p: &str) -> Option<&'static str> {
    Some(match p {
        "preflight" => "Vorbereitung",
        "boarding" => "Boarding",
        "pushback" => "Pushback",
        "taxi_out" => "Rollen",
        "takeoff_roll" => "Startlauf",
        "takeoff" => "Start",
        "climb" => "Steigflug",
        "cruise" => "Reiseflug",
        "holding" => "Warteschleife",
        "descent" => "Sinkflug",
        "approach" => "Anflug",
        "final" => "Endanflug",
        "landing" => "Landung",
        "taxi_in" => "Rollen zum Stand",
        "blocks_on" => "Am Stand",
        "arrived" => "Angekommen",
        "pirep_submitted" => "PIREP eingereicht",
        _ => return None,
    })
}

/// `lage()` aus panel.js.
pub fn lage(verbunden: bool, status: Option<&Value>) -> Lage {
    if !verbunden {
        return Lage::Getrennt;
    }
    let s = match status {
        Some(v) if wahr(Some(v)) => Some(v),
        _ => None,
    };
    let Some(s) = s else {
        return Lage::Bereit;
    };
    if wahr(feld(Some(s), "paused_since")) {
        return Lage::Pausiert;
    }
    let ph = feld(Some(s), "phase").and_then(Value::as_str).unwrap_or("");
    if NACH_TOUCHDOWN.contains(&ph) {
        if !wahr(feld(Some(s), "landing_score_finalized")) {
            return Lage::Auswertung;
        }
        return match ph {
            "pirep_submitted" => Lage::Eingereicht,
            "blocks_on" | "arrived" => Lage::AmStand,
            "taxi_in" => Lage::Rollen,
            _ => Lage::Ergebnis,
        };
    }
    if APPROACH.contains(&ph) {
        return Lage::Anflug;
    }
    Lage::Unterwegs
}

fn punkt_farbe(verbunden: bool, s: Option<&Value>) -> Farbe {
    if !verbunden {
        return Farbe::Schlecht;
    }
    if !wahr(s) {
        return Farbe::Gedaempft;
    }
    if wahr(feld(s, "paused_since")) {
        return Farbe::Akzent;
    }
    if feld(s, "connection_state").and_then(Value::as_str) == Some("failing") {
        return Farbe::Schlecht;
    }
    if zahl_von(feld(s, "queued_position_count")).is_some_and(|q| q > 0.0) {
        return Farbe::Akzent;
    }
    Farbe::Gut
}

fn route(s: Option<&Value>) -> String {
    format!(
        "{} > {}",
        text_von(feld(s, "dpt_airport")).unwrap_or_else(|| "----".into()),
        text_von(feld(s, "arr_airport")).unwrap_or_else(|| "----".into())
    )
}

/// Zellen in Laeufe: nur Zellen mit Wert, hoechstens vier.
fn zellen_laeufe(liste: Vec<Option<Zelle>>, aus: &mut Vec<Lauf>) {
    for z in liste.into_iter().flatten().take(MAX_ZELLEN) {
        if let Some(v) = z.val {
            aus.push(Lauf::neu(Farbe::Gedaempft, Art::Beschriftung, &z.lbl));
            aus.push(Lauf::neu(z.farbe, Art::Wert, &v));
        }
    }
}

fn ticker_zeile(e: &BandEingabe) -> Zeile {
    let a = if e.verbunden { e.aktivitaet } else { None };
    let Some(a) = a.filter(|a| !a.is_null()) else {
        return Zeile::default();
    };
    let mut laeufe = Vec::new();
    let alter_text = alter(a.get("timestamp"), e.jetzt_ms);
    if !alter_text.is_empty() {
        laeufe.push(Lauf::neu(Farbe::Normal, Art::Alter, &alter_text));
    }
    let msg = a.get("message").and_then(Value::as_str).unwrap_or("");
    let text = match text_von(a.get("detail")) {
        Some(d) => format!("{msg} - {d}"),
        None => msg.to_string(),
    };
    let stufe = a
        .get("level")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let farbe = match stufe.as_str() {
        "warn" => Farbe::Warnung,
        "error" => Farbe::Schlecht,
        _ => Farbe::Gedaempft,
    };
    laeufe.push(Lauf::neu(farbe, Art::Meldung, &kuerze(&nur_ascii(&text))));
    Zeile { laeufe }
}

/// Baut das Band aus den Eingaben (Port von `zeichne()` + `zeichneTicker()`).
pub fn baue_band(e: &BandEingabe) -> BandFrame {
    let status = e.status.filter(|v| wahr(Some(v)));
    let s = status;
    let live = feld(s, "live");
    let d = e.debrief.filter(|v| wahr(Some(v)));
    let l = lage(e.verbunden, status);
    let ruhig = matches!(l, Lage::Unterwegs | Lage::Bereit);

    let mut z: Vec<Lauf> = vec![Lauf::punkt(punkt_farbe(e.verbunden, s))];
    let ident = if matches!(l, Lage::Getrennt | Lage::Bereit) {
        "AeroACARS".to_string()
    } else {
        kennung(s)
    };
    z.push(Lauf::neu(Farbe::Normal, Art::Kennung, &ident));

    let mut tail = String::new();
    let state = |z: &mut Vec<Lauf>, t: &str, f: Farbe| z.push(Lauf::neu(f, Art::Lage, t));

    match l {
        Lage::Getrennt => {
            state(
                &mut z,
                &format!("Keine Verbindung - {}:{}", e.host, e.port),
                Farbe::Normal,
            );
            let seit = e
                .fehler_seit_ms
                .map(|t| format!("{} s", rund((e.jetzt_ms - t) as f64 / 1000.0)));
            let grund = e.fehler_text.filter(|t| !t.is_empty()).map(|t| {
                let a = nur_ascii(t);
                a.chars().take(40).collect::<String>()
            });
            zellen_laeufe(
                vec![
                    zelle("seit", seit, Farbe::Normal),
                    zelle("Grund", grund, Farbe::Normal),
                ],
                &mut z,
            );
        }
        Lage::Bereit => state(&mut z, "Bereit - wartet auf Flug", Farbe::Normal),
        Lage::Pausiert => {
            state(&mut z, "Sim getrennt - Flug pausiert", Farbe::Warnung);
            zellen_laeufe(
                vec![
                    zelle(
                        "seit",
                        verstrichen(feld(s, "paused_since"), e.jetzt_ms),
                        Farbe::Normal,
                    ),
                    zelle("Route", Some(route(s)), Farbe::Normal),
                ],
                &mut z,
            );
            tail = "In AeroACARS fortsetzen".into();
        }
        Lage::Unterwegs => {
            let ph = feld(s, "phase").and_then(Value::as_str).unwrap_or("");
            if wahr(feld(s, "takeoff_at")) {
                let msl = zahl_von(feld(live, "altitude_msl_ft"));
                let wx_sink = if ph == "descent" && msl.is_none_or(|m| m > 10000.0) {
                    wx(feld(s, "arr_metar_decoded"), feld(s, "arr_metar"))
                } else {
                    None
                };
                zellen_laeufe(
                    vec![
                        zelle("Route", Some(route(s)), Farbe::Normal),
                        hoehen_zelle(live),
                        zeit_zelle(s, live, e.jetzt_ms),
                        wx_sink.and_then(|w| zelle("WX", Some(w), Farbe::Normal)),
                    ],
                    &mut z,
                );
            } else {
                let fuel = feld(s, "sim_fuel_kg");
                let pf = feld(s, "planned_block_fuel_kg");
                let zfw = feld(s, "sim_zfw_kg");
                let pz = feld(s, "planned_zfw_kg");
                zellen_laeufe(
                    vec![
                        zelle("Route", Some(route(s)), Farbe::Normal),
                        zelle("Fuel", ist_soll(fuel, pf), beladungs_farbe(fuel, pf)),
                        zelle("ZFW", ist_soll(zfw, pz), beladungs_farbe(zfw, pz)),
                        zelle(
                            "WX",
                            wx(feld(s, "dep_metar_decoded"), feld(s, "dep_metar")),
                            Farbe::Normal,
                        ),
                    ],
                    &mut z,
                );
            }
            tail = phase_text(ph).map_or_else(|| ph.to_string(), str::to_string);
        }
        Lage::Anflug => {
            zellen_laeufe(
                vec![
                    zelle(
                        "V/S",
                        zahl(feld(live, "vertical_speed_fpm"), 0),
                        vs_farbe(live, feld(s, "approach_glideslope_angle")),
                    ),
                    zelle("IAS", zahl(feld(live, "ias_kt"), 0), Farbe::Normal),
                    zelle("AGL", zahl(feld(live, "altitude_agl_ft"), 0), Farbe::Normal),
                    zelle("Bank", zahl(feld(live, "bank_deg"), 1), Farbe::Normal),
                ],
                &mut z,
            );
            // Wind als Text statt Pfeil: HW/TW = Gegen-/Rueckenwind,
            // XW = Querwind, R/L = von rechts/links.
            if let (Some(gegen), Some(quer)) = (
                zahl_von(feld(live, "headwind_kt")),
                zahl_von(feld(live, "crosswind_kt")),
            ) {
                z.push(Lauf::neu(Farbe::Gedaempft, Art::Beschriftung, "Wind"));
                z.push(Lauf::neu(
                    Farbe::Normal,
                    Art::Wert,
                    &format!(
                        "{} {}",
                        if gegen >= 0.0 { "HW" } else { "TW" },
                        rund(gegen.abs())
                    ),
                ));
                let q = quer.abs();
                let seite = if rund(q) == 0 {
                    ""
                } else if quer > 0.0 {
                    " R"
                } else {
                    " L"
                };
                let f = if q >= 25.0 {
                    Farbe::Schlecht
                } else if q >= 15.0 {
                    Farbe::Warnung
                } else {
                    Farbe::Normal
                };
                z.push(Lauf::neu(f, Art::Wert, &format!("XW {}{seite}", rund(q))));
            }
            if let Some(r) = text_von(feld(s, "predicted_runway")) {
                tail = format!("~RWY {r}");
            }
        }
        Lage::Auswertung => state(&mut z, "Landung wird ausgewertet", Farbe::Normal),
        Lage::Ergebnis | Lage::Rollen => {
            let wert = feld(d, "score_numeric")
                .map(|v| match v {
                    Value::Number(n) => n.as_f64().map_or("--".into(), js_zahl),
                    Value::String(t) => t.clone(),
                    _ => "--".into(),
                })
                .unwrap_or_else(|| "--".into());
            z.push(Lauf::neu(Farbe::Normal, Art::Note, &wert));
            let etikett = text_von(feld(d, "score_label")).unwrap_or_default();
            let band = if etikett.is_empty() {
                "--".to_string()
            } else {
                etikett.to_uppercase()
            };
            let bf = match etikett.to_lowercase().as_str() {
                "firm" => Farbe::Warnung,
                "hard" | "severe" => Farbe::Schlecht,
                _ => Farbe::Gut,
            };
            z.push(Lauf::neu(bf, Art::Etikett, &band));
            let rate = d.and_then(|_| zahl(feld(d, "landing_rate_fpm"), 0));
            if l == Lage::Rollen {
                zellen_laeufe(vec![zelle("Rate", rate, Farbe::Normal)], &mut z);
                tail = "Rollen zum Stand".into();
            } else {
                let g = d.and_then(|_| {
                    zahl(
                        feld(d, "landing_scored_g_force").or_else(|| feld(d, "landing_g_force")),
                        2,
                    )
                });
                let bounces = feld(d, "bounce_count").map(|v| match v {
                    Value::Number(n) => n.as_f64().map_or(String::new(), js_zahl),
                    Value::String(t) => t.clone(),
                    other => other.to_string(),
                });
                let bahn = text_von(feld(feld(d, "runway_match"), "runway_ident"));
                zellen_laeufe(
                    vec![
                        zelle("Rate", rate, Farbe::Normal),
                        zelle("G", g, Farbe::Normal),
                        zelle("Bounces", bounces, Farbe::Normal),
                        zelle("Bahn", bahn, Farbe::Normal),
                    ],
                    &mut z,
                );
            }
        }
        Lage::AmStand => {
            state(&mut z, "Am Stand", Farbe::Normal);
            zellen_laeufe(
                vec![
                    zelle("Block an", uhrzeit_z(feld(s, "block_on_at")), Farbe::Normal),
                    zelle(
                        "Blockzeit",
                        verstrichen(feld(s, "block_off_at"), e.jetzt_ms),
                        Farbe::Normal,
                    ),
                ],
                &mut z,
            );
        }
        Lage::Eingereicht => {
            let offen = zahl_von(feld(s, "queued_position_count")).unwrap_or(0.0);
            let sauber = feld(s, "connection_state").and_then(Value::as_str) != Some("failing")
                && offen == 0.0;
            if sauber {
                state(&mut z, "PIREP eingereicht", Farbe::Gut);
                zellen_laeufe(
                    vec![zelle("Gesendet", Some("OK".into()), Farbe::Normal)],
                    &mut z,
                );
            } else {
                state(&mut z, "PIREP wartet", Farbe::Warnung);
                zellen_laeufe(
                    vec![zelle("Offen", Some(js_zahl(offen)), Farbe::Normal)],
                    &mut z,
                );
            }
        }
    }
    if !tail.is_empty() {
        z.push(Lauf::neu(Farbe::Gedaempft, Art::Anhang, &tail));
    }
    BandFrame {
        lage: l,
        ruhig,
        zeilen: vec![ticker_zeile(e), Zeile { laeufe: z }],
    }
}

/// Uebergabeplatz zwischen der App (baut das Band im 500-ms-Takt) und der
/// Plugin-Sitzung (sendet es). Die Sitzung laeuft auf dem Plugin-Faden und
/// darf die App-Zustaende nicht selbst anfassen (Sperrfolge: `XPlaneAdapter::
/// stop` haelt die Sim-Sperre und wartet auf genau diesen Faden) — deshalb
/// holt sie nur das fertige Band von hier.
pub struct BandSlot {
    inner: std::sync::Mutex<(u64, Option<std::sync::Arc<BandFrame>>)>,
    wunsch: std::sync::atomic::AtomicBool,
    bereit: std::sync::atomic::AtomicBool,
}

impl Default for BandSlot {
    fn default() -> Self {
        Self {
            inner: std::sync::Mutex::new((0, None)),
            // Standard: „X-Plane-Band senden" ist an.
            wunsch: std::sync::atomic::AtomicBool::new(true),
            bereit: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl BandSlot {
    /// Neues Band ablegen (Version steigt).
    pub fn setze(&self, f: BandFrame) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.0 += 1;
        g.1 = Some(std::sync::Arc::new(f));
    }

    /// Juengstes Band samt Version.
    pub fn holen(&self) -> Option<(u64, std::sync::Arc<BandFrame>)> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.1.as_ref().map(|f| (g.0, std::sync::Arc::clone(f)))
    }

    /// Einstellung „X-Plane-Band senden".
    pub fn set_wunsch(&self, an: bool) {
        self.wunsch.store(an, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn wunsch(&self) -> bool {
        self.wunsch.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Setzt die Sitzung: Plugin angemeldet und kann BAND (>= 1.1.0)?
    /// Nur dann lohnt es sich fuer die App, ein Band zu bauen.
    pub fn set_bereit(&self, b: bool) {
        self.bereit.store(b, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn bereit(&self) -> bool {
        self.bereit.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Prueft ein Datagramm gegen die Regeln des ADR-0005 (Gegenstueck zum Parser
/// des Plugins). `Ok(zeilenzahl)` oder der erste Verstoss.
pub fn pruefe_datagramm(d: &str) -> Result<usize, String> {
    let b = d.as_bytes();
    if b.iter()
        .any(|c| !(0x20..=0x7e).contains(c) && *c != b'\t' && *c != b'\n')
    {
        return Err("Zeichen ausserhalb 0x20-0x7E/TAB/LF".into());
    }
    let Some(rest) = d.strip_suffix('\n') else {
        return Err("kein Zeilenende am Schluss".into());
    };
    let mut teile = rest.split('\n');
    let kopf: Vec<&str> = teile.next().unwrap_or("").split(' ').collect();
    if kopf.len() != 4 || kopf[0] != "BAND" {
        return Err(format!("Kopf ungueltig: {kopf:?}"));
    }
    let seq: u64 = kopf[1].parse().map_err(|_| "seq keine Zahl".to_string())?;
    if seq > 0x7FFF_FFFF {
        return Err("seq zu gross".into());
    }
    if kopf[2] != "0" && kopf[2] != "1" {
        return Err("ruhig weder 0 noch 1".into());
    }
    let n: usize = kopf[3]
        .parse()
        .map_err(|_| "zeilen keine Zahl".to_string())?;
    if n > MAX_ZEILEN {
        return Err("mehr als 4 Zeilen".into());
    }
    let zeilen: Vec<&str> = teile.collect();
    if zeilen.len() != n {
        return Err(format!("{} Zeilen angegeben, {} gesendet", n, zeilen.len()));
    }
    for z in zeilen {
        if z.len() > MAX_ZEILE_BYTES {
            return Err("Zeile laenger als 512 Byte".into());
        }
        if z.is_empty() {
            continue; // Zeile ohne Laeufe
        }
        for lauf in z.split('\t') {
            let b = lauf.as_bytes();
            if b.len() < 2 {
                return Err("Lauf kuerzer als Farbe + Art".into());
            }
            if !matches!(b[0], b'n' | b'd' | b'g' | b'w' | b'b' | b'a') {
                return Err(format!("unbekannte Farbe {:?}", b[0] as char));
            }
            if !matches!(
                b[1],
                b't' | b'm' | b'p' | b'i' | b's' | b'l' | b'v' | b'x' | b'z' | b'e'
            ) {
                return Err(format!("unbekannte Art {:?}", b[1] as char));
            }
            if b[1] == b'p' && b.len() != 2 {
                return Err("Statuspunkt mit Text".into());
            }
        }
    }
    Ok(n)
}

#[cfg(test)]
#[path = "hud_band_tests.rs"]
mod tests;
