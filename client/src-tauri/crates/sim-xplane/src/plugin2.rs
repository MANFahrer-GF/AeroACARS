//! Protokoll 2 des AeroACARS-X-Plane-Plugins (ADR-0004, Lernpaket AP7).
//!
//! Das Plugin wird zum **Dataref-Server**: der Client meldet die Namen an,
//! das Plugin sucht sie, meldet je Name „da" (mit Typ) oder „fehlt" und
//! liefert die Werte. Damit fallen die beiden Lügen der bisherigen Wege weg:
//! RREF liefert fuer jeden Namen eine 0 (auch fuer fehlende, gemessen
//! 27.09.2026), die Web-API verliert ganze Abo-Pakete (28.09.2026).
//!
//! Dieses Modul ist **ohne Socket**: Anfragen schreiben, Antworten lesen und
//! die Sitzungs-Uhr ([`Sitzung`]) sind reine Funktionen bzw. ein reiner
//! Zustandsautomat mit der Zeit als Eingabe. Den Socket fuehrt der
//! Plugin-Faden in `premium.rs` (derselbe Socket wie Protokoll 1, siehe
//! dort), was mit den Werten geschieht, entscheidet das [`Ziel`] (der
//! Adapter, `plugin2_ziel.rs`).
//!
//! ## Ablauf einer Sitzung
//!
//! ```text
//! Suche ── HALLO (sofort, dann alle 5 s) ──▶ Antwort „hallo" (p=2, Version ok)
//!   ▲                                              │
//!   │                                              ▼
//!   └── 3 s kein Paket ◀── Offen: ABOs senden, PING alle 2 s, Werte/Status
//! ```
//!
//! Eine Antwort mit zu alter Plugin-Version oder ein `fehler` mit
//! „protokoll" im Grund oeffnet keine Sitzung (`zu_alt`), das Plugin dient
//! dann nur fuer Protokoll 1 (Aufsetzpaket).

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Protokollversion, die dieser Client spricht.
pub const PROTOKOLL: u32 = 2;
/// Steuerport des Plugins (nur Loopback). Protokoll 1 geht weiter an
/// [`crate::PREMIUM_UDP_PORT`].
pub const PLUGIN2_PORT: u16 = 52001;
/// Aelteste Plugin-Version, deren Protokoll 2 wir annehmen.
pub const MIN_PLUGIN: (u32, u32, u32) = (1, 0, 0);
/// Abstand der HALLO-Anfragen, solange keine Sitzung besteht.
pub const HALLO_ABSTAND: Duration = Duration::from_secs(5);
/// Abstand der PINGs in der Sitzung. Das Plugin verwirft alle Abos nach 5 s
/// ohne Anfrage — zwei PINGs passen in dieses Fenster.
pub const PING_ABSTAND: Duration = Duration::from_secs(2);
/// So lange ohne ein Paket, dann gilt die Sitzung als beendet.
pub const STILLE: Duration = Duration::from_secs(3);
/// Grundzeit fuer den Status eines ABO; dazu kommt Zeit je Name, siehe
/// [`bestaetigung_fuer`]. (QS AP7 H1: das Plugin brauchte fuer 8192 Namen
/// laenger als die frueheren festen 2 s — der Client sendete das Abo dann
/// immer wieder neu, und kein Status kam je an.)
pub const BESTAETIGUNG: Duration = Duration::from_secs(2);
/// Hoechste Wartezeit auf einen Status (vor dem Rueckoff).
pub const BESTAETIGUNG_MAX: Duration = Duration::from_secs(15);
/// Grundzeit fuer den Status, wenn das Plugin das ABO mit `abo_empfangen`
/// schon angenommen hat — dann fehlt nur noch die Suche der Namen (bei
/// 6 × 8192 Namen bis ≈ 3,7 s fuer das letzte Abo, gemessen im Plugin).
pub const ANGENOMMEN_WARTEN: Duration = Duration::from_secs(10);
/// Hoechste Wartezeit zwischen zwei Neusendungen (Rueckoff).
pub const RUECKOFF_MAX: Duration = Duration::from_secs(60);
/// Obergrenze der Sendewarteschlange fuer ABO-Datagramme. Darueber wird
/// ein Abo nicht eingereiht, sondern nach seiner Wartezeit neu versucht.
pub const MAX_WARTESCHLANGE: usize = 4 * 1024 * 1024;
/// Groesste Abo-Generation (`g<zahl>`, 1..=2^31-1).
pub const MAX_GEN: u32 = 0x7FFF_FFFF;
/// Groesstes Anfrage-Datagramm, das wir senden. Die ADR erlaubt 64 KiB; wir
/// bleiben deutlich darunter, weil der Empfangspuffer des Plugins unter
/// Windows ab Werk nur 64 KiB fasst — zwei grosse Datagramme hintereinander
/// liefen sonst über.
pub const MAX_ANFRAGE: usize = 16 * 1024;
/// Laengste Anfragezeile einschliesslich Zeilenende.
pub const MAX_ZEILE: usize = 512;
/// Hoechstens so viele Namen je Abo (ADR).
pub const MAX_NAMEN: usize = 8192;
/// Abo-IDs 1..=16 (ADR).
pub const MAX_ABO_ID: u8 = 16;
/// Mindestabstand zwischen zwei grossen Datagrammen (> 1 KiB) — das Plugin
/// liest einmal je Bild; so staut sich bei ihm nichts.
const SENDEABSTAND: Duration = Duration::from_millis(20);
const GROSS: usize = 1024;

// =============================================================================
// Anfragen (Client → Plugin), reine Funktionen
// =============================================================================

/// `HALLO <protokoll> <client-version>`
pub fn anfrage_hallo(client_version: &str) -> Vec<u8> {
    let v: String = client_version
        .chars()
        .filter(|c| c.is_ascii_graphic())
        .take(40)
        .collect();
    let v = if v.is_empty() { "0".to_string() } else { v };
    format!("HALLO {PROTOKOLL} {v}\n").into_bytes()
}

/// `PING`
pub fn anfrage_ping() -> Vec<u8> {
    b"PING\n".to_vec()
}

/// `ENDE-ABO <abo-id>` — bestellt ein Abo ab.
pub fn anfrage_ende_abo(abo: u8) -> Vec<u8> {
    format!("ENDE-ABO {abo}\n").into_bytes()
}

/// `LISTE <anfrage-id>` — alle Dataref-Namen („Flugzeug vermessen").
pub fn anfrage_liste(id: u32) -> Vec<u8> {
    format!("LISTE {id}\n").into_bytes()
}

/// Darf dieser Name angemeldet werden? Nur druckbares ASCII ohne
/// Leerzeichen, eine Zeile hoechstens [`MAX_ZEILE`] Byte (mit `\n`). Ein
/// Index `[n]` am Ende ist erlaubt (das Plugin prueft ihn gegen die Laenge).
///
/// Gleich streng wie der Parser des Plugins (`xplane-plugin/src/anfrage.cpp`,
/// `zerlege_name`): endet der Name auf `]`, muss davor `[<1–10 Ziffern>]`
/// mit Wert ≤ 2^31−1 und ein nicht leerer Grundname stehen. Ein Name, den
/// das Plugin ablehnen wuerde, geht gar nicht erst hinaus.
pub fn name_gueltig(name: &str) -> bool {
    if !name_zeichen_gueltig(name) {
        return false;
    }
    let Some(rest) = name.strip_suffix(']') else {
        return true;
    };
    let Some(auf) = rest.rfind('[') else {
        return false;
    };
    let ziffern = &rest[auf + 1..];
    auf > 0
        && (1..=10).contains(&ziffern.len())
        && ziffern.bytes().all(|b| b.is_ascii_digit())
        && ziffern
            .parse::<u64>()
            .is_ok_and(|x| x <= u64::from(MAX_GEN))
}

fn name_zeichen_gueltig(name: &str) -> bool {
    !name.is_empty() && name.len() < MAX_ZEILE && name.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Ein Abo, fertig zum Senden.
#[derive(Debug, Clone, PartialEq)]
pub struct AboPlan {
    /// Ein oder mehrere Datagramme (`ABO <id> <rate>` bzw. mit
    /// `<teil> <teile>`, wenn es mehrere sind).
    pub datagramme: Vec<Vec<u8>>,
    /// Index auf dem Draht (Reihenfolge der Anmeldung) → Index in der
    /// Namensliste des Aufrufers. Ungueltige Namen werden nicht gesendet und
    /// verschieben deshalb die Zaehlung.
    pub draht_zu_lokal: Vec<usize>,
}

/// Abo in Datagramme zerlegen: je Datagramm hoechstens [`MAX_ANFRAGE`]
/// Byte, hoechstens [`MAX_NAMEN`] Namen insgesamt, Rate 1–50 Hz. Jede
/// Kopfzeile traegt als letztes Wort die Generation `g<gen>`; das Plugin gibt
/// sie in `abo`/`w`/`fehler` zurueck.
pub fn abo_plan(abo: u8, rate_hz: u32, gen: u32, namen: &[String]) -> AboPlan {
    let gen = gen.clamp(1, MAX_GEN);
    let rate = rate_hz.clamp(1, 50);
    let mut draht_zu_lokal = Vec::new();
    let mut koerper: Vec<String> = Vec::new();
    let mut akt = String::new();
    // Platz fuer die laengste Kopfzeile („ABO 16 50 9999 9999 g2147483647\n").
    let platz = MAX_ANFRAGE - 48;
    for (i, n) in namen.iter().enumerate() {
        if draht_zu_lokal.len() >= MAX_NAMEN {
            break;
        }
        if !name_gueltig(n) {
            continue;
        }
        if !akt.is_empty() && akt.len() + n.len() + 1 > platz {
            koerper.push(std::mem::take(&mut akt));
        }
        akt.push_str(n);
        akt.push('\n');
        draht_zu_lokal.push(i);
    }
    if !akt.is_empty() {
        koerper.push(akt);
    }
    let teile = koerper.len();
    let datagramme = koerper
        .into_iter()
        .enumerate()
        .map(|(i, k)| {
            if teile == 1 {
                format!("ABO {abo} {rate} g{gen}\n{k}").into_bytes()
            } else {
                format!("ABO {abo} {rate} {} {teile} g{gen}\n{k}", i + 1).into_bytes()
            }
        })
        .collect();
    AboPlan {
        datagramme,
        draht_zu_lokal,
    }
}

// =============================================================================
// Antworten (Plugin → Client)
// =============================================================================

/// Typ eines Datarefs laut Plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Typ {
    Int,
    Float,
    Double,
    IntArray,
    FloatArray,
    Bytes,
    /// Ein Typ, den dieser Client noch nicht kennt (neueres Plugin).
    Unbekannt,
}

impl Typ {
    fn aus(s: &str) -> Typ {
        match s {
            "i" => Typ::Int,
            "f" => Typ::Float,
            "d" => Typ::Double,
            "vi" => Typ::IntArray,
            "vf" => Typ::FloatArray,
            "b" => Typ::Bytes,
            _ => Typ::Unbekannt,
        }
    }

    /// Liefert dieser Typ Zahlen?
    pub fn zahl(self) -> bool {
        matches!(
            self,
            Typ::Int | Typ::Float | Typ::Double | Typ::IntArray | Typ::FloatArray
        )
    }
}

/// Status eines angemeldeten Namens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameStatus {
    Da { typ: Typ, laenge: u32 },
    Fehlt,
}

impl NameStatus {
    pub fn da(self) -> bool {
        matches!(self, NameStatus::Da { .. })
    }
}

/// Ein gelieferter Wert.
#[derive(Debug, Clone, PartialEq)]
pub enum Wert {
    Zahl(f64),
    /// Array; nicht lesbare Elemente als NaN.
    Liste(Vec<f64>),
    /// Byte-Array als Text (bis zum ersten NUL).
    Text(String),
}

/// Zahl fuer einen angemeldeten Namen. `name[i]` mit geliefertem Array
/// nimmt Element `i` (oder das einzige Element, wenn das Plugin nur dieses
/// schickt); ein Name ohne Index mit Array nimmt Element 0 — wie RREF.
/// Text und nicht endliche Zahlen ergeben `None`: nie einen Wert erfinden.
pub fn zahl_fuer(name: &str, wert: &Wert) -> Option<f64> {
    let x = match wert {
        Wert::Zahl(x) => *x,
        Wert::Liste(l) => {
            let idx = array_index(name).unwrap_or(0);
            if l.len() == 1 {
                l[0]
            } else {
                *l.get(idx)?
            }
        }
        Wert::Text(_) => return None,
    };
    x.is_finite().then_some(x)
}

/// Index aus `name[i]`, sonst `None`.
pub fn array_index(name: &str) -> Option<usize> {
    let rest = name.strip_suffix(']')?;
    let auf = rest.rfind('[')?;
    rest[auf + 1..].parse().ok()
}

/// Eine gelesene Antwort. Indizes sind Draht-Indizes (Reihenfolge der
/// Anmeldung).
#[derive(Debug, Clone, PartialEq)]
pub enum Antwort {
    Hallo {
        plugin: String,
        xplane: Option<u32>,
        xplm: Option<u32>,
    },
    Abo {
        abo: u8,
        /// Generation des Abos, auf das sich der Status bezieht.
        gen: Option<u32>,
        st: Vec<(usize, NameStatus)>,
    },
    Werte {
        abo: u8,
        gen: Option<u32>,
        seq: Option<u64>,
        v: Vec<(usize, Wert)>,
    },
    Flugzeug {
        icao: Option<String>,
        titel: Option<String>,
        pfad: Option<String>,
    },
    Liste {
        id: u32,
        teil: u32,
        teile: u32,
        namen: Vec<String>,
    },
    /// Das Plugin hat ein ABO angenommen (Status folgt). Seit Plugin
    /// 1.0.0 (Cloud-QS AP7) fuer jedes ABO, auch ein identisches.
    AboEmpfangen {
        abo: u8,
        gen: Option<u32>,
        /// Anzahl der angenommenen Namen.
        namen: Option<u32>,
    },
    Fehler {
        grund: String,
        /// Betroffenes Abo, falls genannt.
        abo: Option<u8>,
        /// Betroffene `LISTE`-Anfrage, falls genannt.
        id: Option<u32>,
        /// Generation des betroffenen Abos, falls genannt.
        gen: Option<u32>,
    },
    /// Gueltiges Protokoll-2-Paket unbekannter Art (z. B. `pong`) — zaehlt
    /// als Lebenszeichen.
    Sonstige(String),
}

/// Ist dieses JSON ein Protokoll-2-Paket (`"p":2`)? Protokoll 1 traegt
/// `"v":1` und kein `p`.
pub fn ist_p2(v: &serde_json::Value) -> bool {
    v.get("p").and_then(|p| p.as_u64()) == Some(u64::from(PROTOKOLL))
}

fn text(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k)
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn zahl_u32(v: &serde_json::Value, k: &str) -> Option<u32> {
    v.get(k)
        .and_then(|x| x.as_u64())
        .and_then(|x| u32::try_from(x).ok())
}

fn abo_id(v: &serde_json::Value) -> Option<u8> {
    v.get("abo")
        .and_then(|x| x.as_u64())
        .and_then(|x| u8::try_from(x).ok())
        .filter(|x| (1..=MAX_ABO_ID).contains(x))
}

fn wert_aus(v: &serde_json::Value) -> Option<Wert> {
    match v {
        serde_json::Value::Number(n) => n.as_f64().map(Wert::Zahl),
        serde_json::Value::Array(a) => Some(Wert::Liste(
            a.iter().map(|x| x.as_f64().unwrap_or(f64::NAN)).collect(),
        )),
        serde_json::Value::String(s) => Some(Wert::Text(s.clone())),
        _ => None,
    }
}

/// Ein Protokoll-2-Paket lesen. `None`, wenn es kein Protokoll 2 ist oder
/// die Pflichtfelder fehlen. Einzelne kaputte Eintraege in `st`/`v` werden
/// uebersprungen, nicht das ganze Paket (anders als AcarsConnect, ADR).
pub fn antwort_lesen(v: &serde_json::Value) -> Option<Antwort> {
    if !ist_p2(v) {
        return None;
    }
    let t = v.get("t").and_then(|x| x.as_str())?;
    Some(match t {
        "hallo" => Antwort::Hallo {
            plugin: text(v, "plugin")?,
            xplane: zahl_u32(v, "xplane"),
            xplm: zahl_u32(v, "xplm"),
        },
        "abo" => {
            let abo = abo_id(v)?;
            let st = v
                .get("st")
                .and_then(|x| x.as_array())?
                .iter()
                .filter_map(|e| {
                    let e = e.as_array()?;
                    let i = usize::try_from(e.first()?.as_u64()?).ok()?;
                    let art = e.get(1)?.as_str()?;
                    let s = if art == "fehlt" {
                        NameStatus::Fehlt
                    } else {
                        NameStatus::Da {
                            typ: Typ::aus(art),
                            laenge: e
                                .get(2)
                                .and_then(|l| l.as_u64())
                                .and_then(|l| u32::try_from(l).ok())
                                .unwrap_or(1),
                        }
                    };
                    Some((i, s))
                })
                .collect();
            Antwort::Abo {
                abo,
                gen: zahl_u32(v, "gen"),
                st,
            }
        }
        "w" => {
            let abo = abo_id(v)?;
            let werte = v
                .get("v")
                .and_then(|x| x.as_array())?
                .iter()
                .filter_map(|e| {
                    let e = e.as_array()?;
                    let i = usize::try_from(e.first()?.as_u64()?).ok()?;
                    Some((i, wert_aus(e.get(1)?)?))
                })
                .collect();
            Antwort::Werte {
                abo,
                gen: zahl_u32(v, "gen"),
                seq: v.get("seq").and_then(|s| s.as_u64()),
                v: werte,
            }
        }
        "flugzeug" => Antwort::Flugzeug {
            icao: text(v, "icao"),
            titel: text(v, "titel"),
            pfad: text(v, "pfad"),
        },
        "liste" => Antwort::Liste {
            id: zahl_u32(v, "id")?,
            teil: zahl_u32(v, "teil").unwrap_or(1),
            teile: zahl_u32(v, "teile").unwrap_or(1),
            namen: v
                .get("n")
                .and_then(|x| x.as_array())?
                .iter()
                .filter_map(|n| n.as_str().map(str::to_string))
                .collect(),
        },
        "abo_empfangen" => Antwort::AboEmpfangen {
            abo: abo_id(v)?,
            gen: zahl_u32(v, "gen"),
            namen: zahl_u32(v, "namen"),
        },
        "fehler" => Antwort::Fehler {
            grund: text(v, "grund").unwrap_or_default(),
            abo: abo_id(v),
            id: zahl_u32(v, "id"),
            gen: zahl_u32(v, "gen"),
        },
        other => Antwort::Sonstige(other.to_string()),
    })
}

/// „1.2.3" (auch „1.2.3-beta", „v1.2") → (1, 2, 3); fehlende Teile = 0.
pub fn version_teile(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let mut it = s.split('.').map(|t| {
        let ziffern: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
        ziffern.parse::<u32>().ok()
    });
    let a = it.next()??;
    let b = it.next().flatten().unwrap_or(0);
    let c = it.next().flatten().unwrap_or(0);
    Some((a, b, c))
}

/// Reicht diese Plugin-Version fuer Protokoll 2?
pub fn version_reicht(plugin: &str) -> bool {
    version_teile(plugin).is_some_and(|v| v >= MIN_PLUGIN)
}

// =============================================================================
// Sitzung (Zustandsautomat, ohne Socket)
// =============================================================================

/// Wartezeit auf den Status eines Abos: 2 s + 1 s je 2000 Namen, hoechstens
/// [`BESTAETIGUNG_MAX`]. Gezaehlt ab dem Zeitpunkt, an dem der LETZTE Teil
/// das Haus verlassen hat, nicht ab dem Einreihen.
pub fn bestaetigung_fuer(namen: usize) -> Duration {
    let ms = BESTAETIGUNG.as_millis() as u64 + (namen as u64) / 2;
    Duration::from_millis(ms).min(BESTAETIGUNG_MAX)
}

/// Wartezeit auf den Status, nachdem das Plugin das ABO mit
/// `abo_empfangen` angenommen hat: 10 s + 1 s je 2000 Namen. Grosszuegig,
/// damit viele grosse Abos nicht unnoetig neu gesendet werden.
pub fn status_wartezeit_angenommen(namen: usize) -> Duration {
    ANGENOMMEN_WARTEN + Duration::from_millis((namen as u64) / 2)
}

/// Ab wann ein bestaetigtes Abo als „Werte versiegt" gilt und identisch
/// neu gesendet wird (QS AP7, Befund N2 der Nachpruefung).
///
/// * Abo 1 (Katalog, 50 Hz, ~100 Namen): fest [`STILLE`] = 3 s — wie die
///   Sitzung selbst; dort zaehlt schnelles Wiederanmelden.
/// * sonst das Groesste aus
///   - 3 s (Untergrenze wie oben),
///   - 5 Perioden der Rate (5 Hz → 1 s; 1 Hz → 5 s): vier ausgefallene
///     Lieferrunden sind noch kein Versiegen,
///   - 1 s + 1 s je 2000 Namen: das Plugin verteilt grosse Abos wegen seines
///     Zeitbudgets (1 ms je Bild, Rundlauf ueber alle Abos) ueber viele
///     Bilder; bei 14 Mess-Abos zu je 8192 Namen lagen 4–5 s zwischen zwei
///     Runden eines Abos — 1 s + 4,1 s = 5,1 s deckt das. Mit festen 3 s
///     wurde laufend identisch neu gesendet (je ~25 × 16 KB), was mit den
///     Werten konkurrierte und die 4-MiB-Grenze der Warteschlange erreichen
///     konnte.
pub fn stille_fuer(abo: u8, rate_hz: u32, namen: usize) -> Duration {
    if abo == 1 {
        return STILLE;
    }
    let perioden = Duration::from_millis(5_000 / u64::from(rate_hz.clamp(1, 50)));
    // 1 s + 2 s je 2000 Namen: Bei 14 × 8192 Namen lag die groesste Luecke
    // zwischen zwei Lieferrunden bei 5,04 s, die alte Schwelle (1 s je 2000
    // Namen) bei 5,1 s — zu knapp fuer langsamere Suchen im echten X-Plane
    // (Nachpruefung AP7, 29.09.2026). Ein zu fruehes Neusenden setzt nichts
    // zurueck, kostet aber Verkehr.
    let je_namen = Duration::from_millis(1_000 + namen as u64);
    STILLE.max(perioden).max(je_namen)
}

/// Wartezeit nach der `versuche`-ten Sendung: nach der ersten und der
/// ersten Wiederholung je die Grundzeit, danach verdoppelt bis
/// [`RUECKOFF_MAX`].
fn wartezeit(grund: Duration, versuche: u32) -> Duration {
    let faktor = 1u32 << versuche.saturating_sub(2).min(6);
    (grund * faktor).min(RUECKOFF_MAX.max(grund))
}

/// Ein gewuenschtes Abo.
#[derive(Debug, Clone)]
pub struct AboWunsch {
    pub id: u8,
    pub rate: u32,
    pub namen: Arc<Vec<String>>,
}

/// Was die Sitzung dem Ziel meldet. Indizes sind bereits in die
/// Namensliste des Wunsches uebersetzt; `namen` ist genau die Liste, auf die
/// sie sich beziehen (das Ziel prueft damit, ob sie noch aktuell ist).
#[derive(Debug, Clone)]
pub enum Ereignis {
    SitzungAuf {
        plugin: String,
        xplane: Option<u32>,
    },
    SitzungZu,
    Status {
        abo: u8,
        namen: Arc<Vec<String>>,
        st: Vec<(usize, NameStatus)>,
    },
    Werte {
        abo: u8,
        namen: Arc<Vec<String>>,
        v: Vec<(usize, Wert)>,
    },
    Flugzeug {
        icao: Option<String>,
        titel: Option<String>,
        pfad: Option<String>,
    },
    Liste {
        id: u32,
        teil: u32,
        teile: u32,
        namen: Vec<String>,
    },
    Fehler {
        grund: String,
        abo: Option<u8>,
        id: Option<u32>,
    },
    /// Auf dieses Abo kam nach der Wartezeit und einer Wiederholung kein
    /// Status. Die Sitzung versucht es mit Rueckoff weiter; wer nicht warten
    /// kann (Vermessung), weicht aus.
    AboOhneAntwort {
        abo: u8,
        namen: Arc<Vec<String>>,
    },
}

/// Empfaenger der Sitzung (der Adapter).
pub trait Ziel: Send + Sync {
    /// Steigt, sobald sich die gewuenschten Abos aendern.
    fn wunsch_generation(&self) -> u64;
    /// Die gewuenschten Abos (nur gerufen, wenn sich die Generation aendert).
    fn wuensche(&self) -> Vec<AboWunsch>;
    fn ereignis(&self, e: Ereignis);
    /// Weitere Anfragen (z. B. `LISTE`), die mit dem naechsten Takt
    /// hinausgehen. Nur in einer offenen Sitzung abgeholt.
    fn anfragen(&self) -> Vec<Vec<u8>> {
        Vec::new()
    }
}

/// Stand der Sitzung fuer die Oberflaeche.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SitzungsInfo {
    pub offen: bool,
    /// Hat in diesem Lauf je eine Sitzung bestanden?
    pub je_offen: bool,
    pub plugin_version: Option<String>,
    pub xplane_version: Option<u32>,
    /// Plugin antwortet, aber zu alt bzw. falsches Protokoll.
    pub zu_alt: bool,
    /// HALLOs seit der letzten Antwort.
    pub hallos_ohne_antwort: u32,
}

struct Gesendet {
    rate: u32,
    namen: Arc<Vec<String>>,
    /// Generation dieses Inhalts; ein identisches Neusenden behaelt sie.
    gen: u32,
    draht_zu_lokal: Vec<usize>,
    datagramme: Vec<Vec<u8>>,
    /// Teile noch in der Warteschlange — die Wartezeit laeuft noch nicht.
    unterwegs: bool,
    /// Letzter Teil hinaus (bzw. Einreihen abgelehnt).
    gesendet_um: Instant,
    /// Sendungen seit dem letzten Status.
    versuche: u32,
    /// `AboOhneAntwort` schon gemeldet?
    gemeldet: bool,
    /// Status zu dieser Generation erhalten?
    bestaetigt: bool,
    /// `abo_empfangen` zu dieser Generation erhalten (Plugin hat das Abo)?
    angenommen: bool,
    /// Letzter Status oder letzte Werte.
    lebenszeichen: Option<Instant>,
    /// Mindestens ein Name ist da — dann muessen auch Werte kommen.
    namen_da: bool,
    /// Je Drahtindex: als „fehlt" gemeldet. Werte dafuer werden verworfen.
    fehlt: Vec<bool>,
}

/// Ein ABO-Datagramm in der Warteschlange.
struct Raus {
    daten: Vec<u8>,
    abo: u8,
    letzter: bool,
}

/// Die Sitzung mit dem Plugin. Wird vom Plugin-Faden gefuehrt: `takt` in
/// jedem Durchlauf, `empfangen` fuer jedes Protokoll-2-Paket.
///
/// Zwei Warteschlangen: `vorrang` (HALLO, PING, ENDE-ABO, LISTE — klein,
/// geht in jedem Takt ganz hinaus) und `ausgang` (ABO-Datagramme, grosse mit
/// [`SENDEABSTAND`], hoechstens [`MAX_WARTESCHLANGE`]). So verdraengt ein
/// grosses Vermessungs-Abo nie den PING, und das Plugin verwirft die Sitzung
/// nicht nach 5 s scheinbarer Stille.
pub struct Sitzung {
    client_version: String,
    offen: bool,
    letztes_hallo: Option<Instant>,
    letzter_ping: Option<Instant>,
    letztes_paket: Option<Instant>,
    wunsch_gen: Option<u64>,
    abos: HashMap<u8, Gesendet>,
    vorrang: VecDeque<Vec<u8>>,
    ausgang: VecDeque<Raus>,
    ausgang_bytes: usize,
    letzter_grosser: Option<Instant>,
    gen_zaehler: u32,
    /// Nach dem Oeffnen: ungewuenschte IDs abbestellen (Abos einer frueheren
    /// Sitzung, die das Plugin beim HALLO vom selben Port behaelt).
    aufraeumen: bool,
    info: SitzungsInfo,
}

impl Sitzung {
    pub fn neu(client_version: &str) -> Self {
        Self {
            client_version: client_version.to_string(),
            offen: false,
            letztes_hallo: None,
            letzter_ping: None,
            letztes_paket: None,
            wunsch_gen: None,
            abos: HashMap::new(),
            vorrang: VecDeque::new(),
            ausgang: VecDeque::new(),
            ausgang_bytes: 0,
            letzter_grosser: None,
            gen_zaehler: 0,
            aufraeumen: false,
            info: SitzungsInfo::default(),
        }
    }

    pub fn info(&self) -> &SitzungsInfo {
        &self.info
    }

    pub fn offen(&self) -> bool {
        self.offen
    }

    fn naechste_gen(&mut self) -> u32 {
        self.gen_zaehler = if self.gen_zaehler >= MAX_GEN {
            1
        } else {
            self.gen_zaehler + 1
        };
        self.gen_zaehler
    }

    /// Zeitgesteuerte Arbeit. Liefert die jetzt zu sendenden Datagramme.
    pub fn takt(&mut self, jetzt: Instant, ziel: &dyn Ziel) -> Vec<Vec<u8>> {
        if self.offen
            && self
                .letztes_paket
                .is_none_or(|t| jetzt.saturating_duration_since(t) > STILLE)
        {
            tracing::info!("X-Plane-Plugin (Protokoll 2): 3 s ohne Paket — Sitzung beendet");
            self.schliessen(ziel);
        }
        if !self.offen {
            let faellig = self
                .letztes_hallo
                .is_none_or(|t| jetzt.saturating_duration_since(t) >= HALLO_ABSTAND);
            if faellig {
                if self.letztes_hallo.is_some() {
                    self.info.hallos_ohne_antwort = self.info.hallos_ohne_antwort.saturating_add(1);
                }
                self.letztes_hallo = Some(jetzt);
                self.vorrang.push_back(anfrage_hallo(&self.client_version));
            }
        } else {
            if self
                .letzter_ping
                .is_none_or(|t| jetzt.saturating_duration_since(t) >= PING_ABSTAND)
            {
                self.letzter_ping = Some(jetzt);
                self.vorrang.push_back(anfrage_ping());
            }
            let gen = ziel.wunsch_generation();
            if self.wunsch_gen != Some(gen) {
                self.wunsch_gen = Some(gen);
                self.abgleichen(ziel.wuensche(), jetzt);
            }
            self.nachsenden(jetzt, ziel);
            self.vorrang.extend(ziel.anfragen());
        }
        self.ausgang_leeren(jetzt)
    }

    /// Abos ohne Status (nach ihrer Wartezeit) oder mit versiegten Werten neu
    /// senden — identisch, gleiche Generation, mit Rueckoff.
    fn nachsenden(&mut self, jetzt: Instant, ziel: &dyn Ziel) {
        let mut neu: Vec<u8> = Vec::new();
        let mut melden: Vec<(u8, Arc<Vec<String>>)> = Vec::new();
        for (id, g) in self.abos.iter_mut() {
            if g.unterwegs {
                continue;
            }
            let seit = jetzt.saturating_duration_since(g.gesendet_um);
            if g.bestaetigt {
                // Werte versiegt, obwohl Namen da sind.
                let stille = stille_fuer(*id, g.rate, g.draht_zu_lokal.len());
                if g.namen_da
                    && g.lebenszeichen
                        .is_none_or(|t| jetzt.saturating_duration_since(t) > stille)
                    && seit > stille
                {
                    neu.push(*id);
                }
                continue;
            }
            let grund = if g.angenommen {
                status_wartezeit_angenommen(g.draht_zu_lokal.len())
            } else {
                bestaetigung_fuer(g.draht_zu_lokal.len())
            };
            if seit > wartezeit(grund, g.versuche) {
                if g.versuche >= 2 && !g.gemeldet {
                    // Wartezeit + eine Wiederholung ohne Status: melden, JETZT
                    // nicht erneut senden — das Ziel entscheidet (Vermessung
                    // zieht zurueck). Sonst naechster Versuch nach Rueckoff.
                    g.gemeldet = true;
                    g.versuche = g.versuche.saturating_add(1);
                    g.gesendet_um = jetzt;
                    melden.push((*id, Arc::clone(&g.namen)));
                } else {
                    neu.push(*id);
                }
            }
        }
        for (abo, namen) in melden {
            tracing::info!(abo, "X-Plane-Plugin: Abo bleibt ohne Status");
            ziel.ereignis(Ereignis::AboOhneAntwort { abo, namen });
        }
        for id in neu {
            // Das Ziel kann das Abo inzwischen zurueckgezogen haben.
            if self.abos.contains_key(&id) {
                tracing::info!(
                    abo = id,
                    "X-Plane-Plugin: Abo ohne Antwort — erneut gesendet"
                );
                self.senden(id, jetzt);
            }
        }
    }

    /// Abo `id` (erneut) einreihen. Alte, noch wartende Teile desselben Abos
    /// fliegen vorher raus (Neuplanen). Passt es nicht mehr in die
    /// Warteschlange, gilt es als gesendet und kommt nach seiner Wartezeit
    /// wieder dran.
    fn senden(&mut self, id: u8, jetzt: Instant) {
        self.aus_warteschlange(id);
        let Some(g) = self.abos.get_mut(&id) else {
            return;
        };
        g.versuche = g.versuche.saturating_add(1);
        g.bestaetigt = false;
        g.angenommen = false;
        g.lebenszeichen = None;
        let groesse: usize = g.datagramme.iter().map(Vec::len).sum();
        if self.ausgang_bytes + groesse > MAX_WARTESCHLANGE {
            tracing::warn!(
                abo = id,
                bytes = groesse,
                "X-Plane-Plugin: Sendewarteschlange voll — Abo spaeter"
            );
            g.unterwegs = false;
            g.gesendet_um = jetzt;
            return;
        }
        let n = g.datagramme.len();
        for (i, d) in g.datagramme.iter().enumerate() {
            self.ausgang.push_back(Raus {
                daten: d.clone(),
                abo: id,
                letzter: i + 1 == n,
            });
        }
        self.ausgang_bytes += groesse;
        g.unterwegs = true;
    }

    fn aus_warteschlange(&mut self, id: u8) {
        let mut weg = 0;
        self.ausgang.retain(|r| {
            let bleibt = r.abo != id;
            if !bleibt {
                weg += r.daten.len();
            }
            bleibt
        });
        self.ausgang_bytes = self.ausgang_bytes.saturating_sub(weg);
    }

    /// Vorrang ganz; danach ABO-Datagramme, grosse mit [`SENDEABSTAND`]. Mit
    /// dem letzten Teil eines Abos beginnt dessen Wartezeit.
    fn ausgang_leeren(&mut self, jetzt: Instant) -> Vec<Vec<u8>> {
        let mut raus: Vec<Vec<u8>> = self.vorrang.drain(..).collect();
        while let Some(r) = self.ausgang.front() {
            if r.daten.len() > GROSS {
                if self
                    .letzter_grosser
                    .is_some_and(|t| jetzt.saturating_duration_since(t) < SENDEABSTAND)
                {
                    break;
                }
                self.letzter_grosser = Some(jetzt);
            }
            let Some(r) = self.ausgang.pop_front() else {
                break;
            };
            self.ausgang_bytes = self.ausgang_bytes.saturating_sub(r.daten.len());
            if r.letzter {
                if let Some(g) = self.abos.get_mut(&r.abo) {
                    g.unterwegs = false;
                    g.gesendet_um = jetzt;
                }
            }
            raus.push(r.daten);
        }
        raus
    }

    /// Gewuenschte Abos mit den gesendeten abgleichen: neue/geaenderte
    /// senden (neue Generation), weggefallene abbestellen.
    fn abgleichen(&mut self, wuensche: Vec<AboWunsch>, jetzt: Instant) {
        let mut behalten: HashSet<u8> = HashSet::new();
        for w in wuensche {
            if !(1..=MAX_ABO_ID).contains(&w.id) || w.namen.is_empty() {
                continue;
            }
            if let Some(g) = self.abos.get(&w.id) {
                if g.rate == w.rate && (Arc::ptr_eq(&g.namen, &w.namen) || *g.namen == *w.namen) {
                    behalten.insert(w.id);
                    continue;
                }
            }
            let gen = self.naechste_gen();
            let plan = abo_plan(w.id, w.rate, gen, &w.namen);
            if plan.draht_zu_lokal.is_empty() {
                continue;
            }
            behalten.insert(w.id);
            tracing::info!(
                abo = w.id,
                gen,
                namen = plan.draht_zu_lokal.len(),
                datagramme = plan.datagramme.len(),
                rate = w.rate,
                "X-Plane-Plugin: Abo angemeldet"
            );
            let n = plan.draht_zu_lokal.len();
            self.abos.insert(
                w.id,
                Gesendet {
                    rate: w.rate,
                    namen: w.namen,
                    gen,
                    draht_zu_lokal: plan.draht_zu_lokal,
                    datagramme: plan.datagramme,
                    unterwegs: false,
                    gesendet_um: jetzt,
                    versuche: 0,
                    gemeldet: false,
                    bestaetigt: false,
                    angenommen: false,
                    lebenszeichen: None,
                    namen_da: false,
                    fehlt: vec![false; n],
                },
            );
            self.senden(w.id, jetzt);
        }
        let weg: Vec<u8> = self
            .abos
            .keys()
            .copied()
            .filter(|id| !behalten.contains(id))
            .collect();
        for id in weg {
            self.abos.remove(&id);
            self.aus_warteschlange(id);
            self.vorrang.push_back(anfrage_ende_abo(id));
        }
        if std::mem::take(&mut self.aufraeumen) {
            // Nach dem (Wieder-)Oeffnen: alles abbestellen, was wir nicht
            // wollen — das Plugin behaelt beim HALLO vom selben Port die Abos
            // der vorigen Sitzung, sie liefen sonst verwaist weiter. Geht
            // ueber den Vorrang VOR den neuen ABOs hinaus.
            for id in 1..=MAX_ABO_ID {
                if !behalten.contains(&id) {
                    self.vorrang.push_back(anfrage_ende_abo(id));
                }
            }
        }
    }

    /// Ein Protokoll-2-Paket verarbeiten.
    pub fn empfangen(&mut self, a: Antwort, jetzt: Instant, ziel: &dyn Ziel) {
        if let Antwort::Hallo { plugin, xplane, .. } = &a {
            self.info.hallos_ohne_antwort = 0;
            self.info.plugin_version = Some(plugin.clone());
            self.info.xplane_version = *xplane;
            if !version_reicht(plugin) {
                if !self.info.zu_alt {
                    tracing::warn!(
                        plugin = %plugin,
                        "X-Plane-Plugin zu alt fuer Protokoll 2 — nur Aufsetzpaket (Protokoll 1)"
                    );
                }
                self.info.zu_alt = true;
                return;
            }
            self.info.zu_alt = false;
            self.letztes_paket = Some(jetzt);
            if !self.offen {
                self.offen = true;
                self.info.offen = true;
                self.info.je_offen = true;
                self.letzter_ping = Some(jetzt);
                self.abos.clear();
                self.ausgang.clear();
                self.ausgang_bytes = 0;
                self.vorrang.clear();
                self.wunsch_gen = None;
                self.aufraeumen = true;
                tracing::info!(
                    plugin = %plugin,
                    xplane = ?xplane,
                    "X-Plane-Plugin (Protokoll 2): Sitzung offen"
                );
                ziel.ereignis(Ereignis::SitzungAuf {
                    plugin: plugin.clone(),
                    xplane: *xplane,
                });
            }
            return;
        }
        if !self.offen {
            if let Antwort::Fehler { grund, .. } = &a {
                if grund.to_ascii_lowercase().contains("protokoll") {
                    self.info.zu_alt = true;
                    self.info.hallos_ohne_antwort = 0;
                    tracing::warn!(grund = %grund, "X-Plane-Plugin lehnt Protokoll 2 ab");
                }
            }
            // Reste einer frueheren Sitzung (oder eines anderen Clients).
            return;
        }
        self.letztes_paket = Some(jetzt);
        match a {
            Antwort::Hallo { .. } => {}
            Antwort::Abo { abo, gen, st } => {
                let Some(g) = self.abos.get_mut(&abo) else {
                    return;
                };
                // Status zu einem frueheren Inhalt dieser ID — nicht als
                // Bestaetigung des neuen werten.
                if gen.is_some_and(|x| x != g.gen) {
                    return;
                }
                g.bestaetigt = true;
                g.versuche = 0;
                g.gemeldet = false;
                g.lebenszeichen = Some(jetzt);
                let mut lokal = Vec::with_capacity(st.len());
                for (i, s) in st {
                    if let Some(&l) = g.draht_zu_lokal.get(i) {
                        if let Some(f) = g.fehlt.get_mut(i) {
                            *f = !s.da();
                        }
                        if s.da() {
                            g.namen_da = true;
                        }
                        lokal.push((l, s));
                    }
                }
                ziel.ereignis(Ereignis::Status {
                    abo,
                    namen: Arc::clone(&g.namen),
                    st: lokal,
                });
            }
            Antwort::Werte { abo, gen, v, .. } => {
                let Some(g) = self.abos.get_mut(&abo) else {
                    return;
                };
                match gen {
                    Some(x) if x != g.gen => return,
                    None if !g.bestaetigt => return,
                    _ => {}
                }
                g.lebenszeichen = Some(jetzt);
                g.namen_da = true;
                // Ein Wert fuer einen als „fehlt" gemeldeten Namen wird
                // verworfen (QS AP7 N1) — nie einen Wert erfinden.
                let v: Vec<(usize, Wert)> = v
                    .into_iter()
                    .filter(|(i, _)| !g.fehlt.get(*i).copied().unwrap_or(false))
                    .filter_map(|(i, w)| g.draht_zu_lokal.get(i).map(|&l| (l, w)))
                    .collect();
                ziel.ereignis(Ereignis::Werte {
                    abo,
                    namen: Arc::clone(&g.namen),
                    v,
                });
            }
            Antwort::AboEmpfangen { abo, gen, namen } => {
                self.abo_empfangen(abo, gen, namen, jetzt);
            }
            Antwort::Flugzeug { icao, titel, pfad } => {
                ziel.ereignis(Ereignis::Flugzeug { icao, titel, pfad })
            }
            Antwort::Liste {
                id,
                teil,
                teile,
                namen,
            } => ziel.ereignis(Ereignis::Liste {
                id,
                teil,
                teile,
                namen,
            }),
            Antwort::Fehler {
                grund,
                abo,
                id,
                gen,
            } => {
                if grund == "kein_hallo" {
                    // Das Plugin kennt uns nicht (mehr): X-Plane neu gestartet,
                    // Plugin neu geladen oder uns nach 5 s Stille vergessen.
                    // PINGs hielten die Sitzung sonst scheinbar am Leben —
                    // sofort schliessen und neu anmelden.
                    tracing::info!("X-Plane-Plugin kennt diesen Client nicht mehr — neues HALLO");
                    self.schliessen(ziel);
                    return;
                }
                // Fehler zu einem frueheren Inhalt dieser Abo-ID: erledigt.
                if let (Some(a), Some(x)) = (abo, gen) {
                    if self.abos.get(&a).is_some_and(|g| g.gen != x) {
                        return;
                    }
                }
                tracing::info!(grund = %grund, ?abo, ?id, ?gen, "X-Plane-Plugin meldet Fehler");
                ziel.ereignis(Ereignis::Fehler { grund, abo, id });
            }
            Antwort::Sonstige(_) => {}
        }
    }

    /// `abo_empfangen`: Das Plugin hat das Abo dieser Generation. Ab jetzt
    /// gilt die grosszuegige Status-Wartezeit. Nennt es eine andere
    /// Namenszahl, als wir gesendet haben (Teil verloren/verstuemmelt),
    /// wird das Abo mit neuer Generation neu gesendet.
    fn abo_empfangen(&mut self, abo: u8, gen: Option<u32>, namen: Option<u32>, jetzt: Instant) {
        let neu_planen = {
            let Some(g) = self.abos.get_mut(&abo) else {
                return;
            };
            if gen.is_some_and(|x| x != g.gen) {
                return;
            }
            let gesendet = g.draht_zu_lokal.len();
            if namen.is_some_and(|n| n as usize != gesendet) {
                tracing::warn!(
                    abo,
                    gesendet,
                    angenommen = ?namen,
                    "X-Plane-Plugin: Abo mit anderer Namenszahl angenommen — neu mit neuer Generation"
                );
                true
            } else {
                if !g.bestaetigt {
                    g.angenommen = true;
                    g.unterwegs = false;
                    g.gesendet_um = jetzt;
                }
                false
            }
        };
        if neu_planen {
            let gen = self.naechste_gen();
            if let Some(g) = self.abos.get_mut(&abo) {
                let plan = abo_plan(abo, g.rate, gen, &g.namen);
                g.gen = gen;
                g.fehlt = vec![false; plan.draht_zu_lokal.len()];
                g.draht_zu_lokal = plan.draht_zu_lokal;
                g.datagramme = plan.datagramme;
                g.versuche = 0;
                g.gemeldet = false;
                g.namen_da = false;
            }
            self.senden(abo, jetzt);
        }
    }

    fn schliessen(&mut self, ziel: &dyn Ziel) {
        self.offen = false;
        self.info.offen = false;
        self.abos.clear();
        self.ausgang.clear();
        self.ausgang_bytes = 0;
        self.vorrang.clear();
        self.letztes_hallo = None;
        self.letzter_ping = None;
        self.wunsch_gen = None;
        ziel.ereignis(Ereignis::SitzungZu);
    }

    /// Beim Beenden des Clients: alle Abos abbestellen (das Plugin wuerde
    /// sie sonst erst nach 5 s ohne PING verwerfen).
    pub fn beenden(&mut self) -> Vec<Vec<u8>> {
        if !self.offen {
            return Vec::new();
        }
        let mut ids: Vec<u8> = self.abos.keys().copied().collect();
        ids.sort_unstable();
        self.abos.clear();
        self.ausgang.clear();
        self.ausgang_bytes = 0;
        self.offen = false;
        self.info.offen = false;
        ids.into_iter().map(anfrage_ende_abo).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    fn json(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    fn namen(n: &[&str]) -> Vec<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    fn da() -> NameStatus {
        NameStatus::Da {
            typ: Typ::Float,
            laenge: 1,
        }
    }

    #[test]
    fn anfragen_sind_textzeilen() {
        assert_eq!(anfrage_hallo("1.9.11"), b"HALLO 2 1.9.11\n");
        assert_eq!(anfrage_hallo("1.9 beta\n"), b"HALLO 2 1.9beta\n");
        assert_eq!(anfrage_ping(), b"PING\n");
        assert_eq!(anfrage_ende_abo(3), b"ENDE-ABO 3\n");
        assert_eq!(anfrage_liste(7), b"LISTE 7\n");
    }

    /// Gleiche Regeln wie `zerlege_name` im Plugin (anfrage.cpp).
    #[test]
    fn namen_werden_streng_geprueft() {
        assert!(name_gueltig("sim/flightmodel/position/latitude"));
        assert!(name_gueltig("AirbusFBW/OHPLightSwitches[7]"));
        assert!(name_gueltig("sim/a[2147483647]"));
        assert!(name_gueltig("sim/offen[")); // kein `]` am Ende: gewoehnlicher Name
        assert!(!name_gueltig(""));
        assert!(!name_gueltig("mit leerzeichen"));
        assert!(!name_gueltig("zeile\numbruch"));
        assert!(!name_gueltig("umlaut/ä"));
        assert!(!name_gueltig(&"a".repeat(MAX_ZEILE)));
        assert!(name_gueltig(&"a".repeat(MAX_ZEILE - 1)));
        assert!(!name_gueltig("sim/a]"), "] ohne [");
        assert!(!name_gueltig("sim/a[]"), "leerer Index");
        assert!(!name_gueltig("sim/a[x]"));
        assert!(!name_gueltig("sim/a[-1]"));
        assert!(!name_gueltig("sim/a[2147483648]"), "ueber 2^31-1");
        assert!(!name_gueltig("sim/a[12345678901]"), "mehr als 10 Ziffern");
        assert!(!name_gueltig("[3]"), "ohne Grundname");
    }

    #[test]
    fn kleines_abo_ist_ein_datagramm() {
        let p = abo_plan(1, 50, 4, &namen(&["sim/a", "sim/b[2]"]));
        assert_eq!(
            p.datagramme,
            vec![b"ABO 1 50 g4\nsim/a\nsim/b[2]\n".to_vec()]
        );
        assert_eq!(p.draht_zu_lokal, vec![0, 1]);
    }

    /// Ungueltige Namen gehen nicht hinaus, die Zuordnung Draht → lokal
    /// ueberspringt sie.
    #[test]
    fn ungueltige_namen_verschieben_die_zuordnung() {
        let p = abo_plan(2, 99, 1, &namen(&["sim/a", "kaputt name", "sim/c"]));
        assert_eq!(p.datagramme, vec![b"ABO 2 50 g1\nsim/a\nsim/c\n".to_vec()]);
        assert_eq!(p.draht_zu_lokal, vec![0, 2]);
        assert!(abo_plan(2, 0, 1, &namen(&["x y"])).datagramme.is_empty());
        assert!(
            String::from_utf8(abo_plan(2, 0, 0, &namen(&["sim/a"])).datagramme[0].clone())
                .unwrap()
                .starts_with("ABO 2 1 g1\n")
        );
    }

    /// Grosse Abos: mehrteilig, jedes Datagramm unter der Grenze, jede
    /// Zeile ≤ 512 Byte, Reihenfolge und Anzahl bleiben, hoechstens 8192,
    /// jede Kopfzeile mit derselben Generation.
    #[test]
    fn grosses_abo_wird_geteilt() {
        let alle: Vec<String> = (0..9000)
            .map(|i| format!("laminar/B738/irgendwas/sehr/langer/name_{i:05}"))
            .collect();
        let p = abo_plan(3, 5, MAX_GEN, &alle);
        assert_eq!(p.draht_zu_lokal.len(), MAX_NAMEN);
        assert!(p.datagramme.len() > 1);
        let teile = p.datagramme.len();
        let mut gelesen = Vec::new();
        for (i, d) in p.datagramme.iter().enumerate() {
            assert!(d.len() <= MAX_ANFRAGE, "{} Byte", d.len());
            let s = String::from_utf8(d.clone()).unwrap();
            let mut zeilen = s.lines();
            assert_eq!(
                zeilen.next().unwrap(),
                format!("ABO 3 5 {} {teile} g{MAX_GEN}", i + 1)
            );
            for z in zeilen {
                assert!(z.len() < MAX_ZEILE);
                gelesen.push(z.to_string());
            }
        }
        assert_eq!(gelesen.len(), MAX_NAMEN);
        assert_eq!(gelesen[0], alle[0]);
        assert_eq!(gelesen[MAX_NAMEN - 1], alle[MAX_NAMEN - 1]);
    }

    #[test]
    fn wartezeit_waechst_mit_namen_und_versuchen() {
        assert_eq!(bestaetigung_fuer(0), Duration::from_secs(2));
        assert_eq!(bestaetigung_fuer(2000), Duration::from_secs(3));
        assert_eq!(bestaetigung_fuer(8192), Duration::from_millis(6096));
        assert_eq!(bestaetigung_fuer(100_000), BESTAETIGUNG_MAX);
        let t = Duration::from_secs(3);
        assert_eq!(wartezeit(t, 1), t);
        assert_eq!(wartezeit(t, 2), t);
        assert_eq!(wartezeit(t, 3), t * 2);
        assert_eq!(wartezeit(t, 4), t * 4);
        assert_eq!(wartezeit(t, 30), RUECKOFF_MAX);
    }

    #[test]
    fn hallo_und_versionen() {
        let a = antwort_lesen(&json(
            r#"{"p":2,"t":"hallo","plugin":"1.0.0","xplane":12100,"xplm":430}"#,
        ))
        .unwrap();
        assert_eq!(
            a,
            Antwort::Hallo {
                plugin: "1.0.0".into(),
                xplane: Some(12100),
                xplm: Some(430)
            }
        );
        assert_eq!(version_teile("1.2.3"), Some((1, 2, 3)));
        assert_eq!(version_teile("v1.2"), Some((1, 2, 0)));
        assert_eq!(version_teile("1.0.0-beta.2"), Some((1, 0, 0)));
        assert_eq!(version_teile("abc"), None);
        assert!(version_reicht("1.0.0"));
        assert!(version_reicht("2.0"));
        assert!(!version_reicht("0.5.11"));
        assert!(!version_reicht(""));
    }

    /// Protokoll 1 (`"v":1`, kein `p`) wird nicht als Protokoll 2 gelesen.
    #[test]
    fn protokoll_1_ist_kein_protokoll_2() {
        let p1 = json(r#"{"v":1,"type":"touchdown","captured_vs_fpm":-300.0}"#);
        assert!(!ist_p2(&p1));
        assert!(antwort_lesen(&p1).is_none());
        assert!(antwort_lesen(&json(r#"{"p":3,"t":"hallo","plugin":"9.0.0"}"#)).is_none());
    }

    /// Beispiele aus ADR-0004 (mit der Generation der Protokoll-Ergaenzung).
    #[test]
    fn beispiele_aus_der_adr() {
        let a = antwort_lesen(&json(
            r#"{"p":2,"t":"abo","abo":1,"gen":4,"teil":1,"teile":1,
 "st":[[0,"f",1],[1,"fehlt"],[2,"d",1],[3,"b",40],[4,"vf",8]]}"#,
        ))
        .unwrap();
        assert_eq!(
            a,
            Antwort::Abo {
                abo: 1,
                gen: Some(4),
                st: vec![
                    (0, da()),
                    (1, NameStatus::Fehlt),
                    (
                        2,
                        NameStatus::Da {
                            typ: Typ::Double,
                            laenge: 1
                        }
                    ),
                    (
                        3,
                        NameStatus::Da {
                            typ: Typ::Bytes,
                            laenge: 40
                        }
                    ),
                    (
                        4,
                        NameStatus::Da {
                            typ: Typ::FloatArray,
                            laenge: 8
                        }
                    ),
                ]
            }
        );
        let w = antwort_lesen(&json(
            r#"{"p":2,"t":"w","abo":1,"gen":4,"seq":812,"teil":1,"teile":1,"v":[[0,51.2345678],[2,8.5],[3,"A20N"],[4,[0,0,1]]]}"#,
        ))
        .unwrap();
        assert_eq!(
            w,
            Antwort::Werte {
                abo: 1,
                gen: Some(4),
                seq: Some(812),
                v: vec![
                    (0, Wert::Zahl(51.2345678)),
                    (2, Wert::Zahl(8.5)),
                    (3, Wert::Text("A20N".into())),
                    (4, Wert::Liste(vec![0.0, 0.0, 1.0])),
                ]
            }
        );
        // Doppelte Genauigkeit bleibt erhalten.
        if let Antwort::Werte { v, .. } = &w {
            assert_eq!(v[0].1, Wert::Zahl(51.234_567_8_f64));
        }
        assert_eq!(
            antwort_lesen(&json(
                r#"{"p":2,"t":"flugzeug","icao":"A20N","titel":"A320neo","pfad":"Aircraft/x/a320.acf"}"#
            )),
            Some(Antwort::Flugzeug {
                icao: Some("A20N".into()),
                titel: Some("A320neo".into()),
                pfad: Some("Aircraft/x/a320.acf".into())
            })
        );
        assert_eq!(
            antwort_lesen(&json(
                r#"{"p":2,"t":"liste","id":7,"teil":3,"teile":40,"n":["sim/a","sim/b"]}"#
            )),
            Some(Antwort::Liste {
                id: 7,
                teil: 3,
                teile: 40,
                namen: namen(&["sim/a", "sim/b"])
            })
        );
        assert_eq!(
            antwort_lesen(&json(r#"{"p":2,"t":"fehler","grund":"zeile_zu_lang"}"#)),
            Some(Antwort::Fehler {
                grund: "zeile_zu_lang".into(),
                abo: None,
                id: None,
                gen: None
            })
        );
    }

    /// Ein kaputter Eintrag kostet nur sich selbst, nicht das Paket.
    #[test]
    fn kaputte_eintraege_werden_uebersprungen() {
        let w = antwort_lesen(&json(
            r#"{"p":2,"t":"w","abo":1,"v":[[0,1.5],["x",2],[1],[2,null],[-1,3],[3,4]]}"#,
        ))
        .unwrap();
        assert_eq!(
            w,
            Antwort::Werte {
                abo: 1,
                gen: None,
                seq: None,
                v: vec![(0, Wert::Zahl(1.5)), (3, Wert::Zahl(4.0))]
            }
        );
        // Abo-ID ausserhalb 1..=16 → ganzes Paket ungueltig.
        assert!(antwort_lesen(&json(r#"{"p":2,"t":"w","abo":17,"v":[[0,1]]}"#)).is_none());
        assert!(antwort_lesen(&json(r#"{"p":2,"t":"w","abo":0,"v":[[0,1]]}"#)).is_none());
        assert_eq!(
            antwort_lesen(&json(r#"{"p":2,"t":"pong"}"#)),
            Some(Antwort::Sonstige("pong".into()))
        );
    }

    #[test]
    fn zahlen_fuer_namen() {
        assert_eq!(zahl_fuer("sim/a", &Wert::Zahl(2.5)), Some(2.5));
        assert_eq!(zahl_fuer("sim/a", &Wert::Zahl(f64::NAN)), None);
        assert_eq!(
            zahl_fuer("sim/a[2]", &Wert::Liste(vec![0.0, 1.0, 7.0])),
            Some(7.0)
        );
        assert_eq!(zahl_fuer("sim/a[2]", &Wert::Liste(vec![7.0])), Some(7.0));
        assert_eq!(zahl_fuer("sim/a[5]", &Wert::Liste(vec![0.0, 1.0])), None);
        assert_eq!(zahl_fuer("sim/a", &Wert::Liste(vec![3.0, 1.0])), Some(3.0));
        assert_eq!(zahl_fuer("sim/a", &Wert::Text("A20N".into())), None);
        assert_eq!(array_index("sim/a[12]"), Some(12));
        assert_eq!(array_index("sim/a"), None);
    }

    // ---- Sitzung ----

    #[derive(Default)]
    struct TestZiel {
        gen: Mutex<u64>,
        wuensche: Mutex<Vec<AboWunsch>>,
        ereignisse: Mutex<Vec<Ereignis>>,
    }

    impl Ziel for TestZiel {
        fn wunsch_generation(&self) -> u64 {
            *self.gen.lock()
        }
        fn wuensche(&self) -> Vec<AboWunsch> {
            self.wuensche.lock().clone()
        }
        fn ereignis(&self, e: Ereignis) {
            self.ereignisse.lock().push(e);
        }
    }

    fn hallo_antwort(v: &str) -> Antwort {
        Antwort::Hallo {
            plugin: v.into(),
            xplane: Some(12100),
            xplm: Some(430),
        }
    }

    fn text_von(d: &[Vec<u8>]) -> Vec<String> {
        d.iter()
            .map(|x| String::from_utf8(x.clone()).unwrap())
            .collect()
    }

    /// Die ENDE-ABO des Aufraeumens nach dem Oeffnen (IDs 1..=16 ohne die
    /// gewuenschten) herausfiltern.
    fn ohne_aufraeumen(d: Vec<String>) -> Vec<String> {
        d.into_iter()
            .filter(|x| !x.starts_with("ENDE-ABO"))
            .collect()
    }

    fn status(abo: u8, gen: Option<u32>, st: Vec<(usize, NameStatus)>) -> Antwort {
        Antwort::Abo { abo, gen, st }
    }

    fn werte(abo: u8, gen: Option<u32>, v: Vec<(usize, Wert)>) -> Antwort {
        Antwort::Werte {
            abo,
            gen,
            seq: None,
            v,
        }
    }

    /// Ganzer Lebenslauf: HALLO, Sitzung auf, ABO, Status, Werte, PING,
    /// Stille → Sitzung zu → wieder HALLO.
    #[test]
    fn lebenslauf_einer_sitzung() {
        let z = TestZiel::default();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 1,
            rate: 50,
            namen: Arc::new(namen(&["sim/a", "kaputt name", "sim/c"])),
        }];
        let mut s = Sitzung::neu("1.9.11");
        let t0 = Instant::now();
        assert_eq!(text_von(&s.takt(t0, &z)), vec!["HALLO 2 1.9.11\n"]);
        // Innerhalb 5 s kein zweites HALLO.
        assert!(s.takt(t0 + Duration::from_secs(4), &z).is_empty());
        assert_eq!(
            text_von(&s.takt(t0 + Duration::from_secs(5), &z)),
            vec!["HALLO 2 1.9.11\n"]
        );
        assert_eq!(s.info().hallos_ohne_antwort, 1);

        let t1 = t0 + Duration::from_secs(6);
        s.empfangen(hallo_antwort("1.0.0"), t1, &z);
        assert!(s.offen());
        assert_eq!(s.info().hallos_ohne_antwort, 0);
        assert!(matches!(
            z.ereignisse.lock()[0],
            Ereignis::SitzungAuf { .. }
        ));
        let raus = text_von(&s.takt(t1, &z));
        // M1: erst alle ungewuenschten IDs abbestellen, dann das neue ABO.
        let ende: Vec<&String> = raus.iter().filter(|x| x.starts_with("ENDE-ABO")).collect();
        assert_eq!(ende.len(), 15);
        assert!(!raus.contains(&"ENDE-ABO 1\n".to_string()));
        assert_eq!(raus.last().unwrap(), "ABO 1 50 g1\nsim/a\nsim/c\n");

        // Werte vor dem Status (ohne Generation) gehoeren womoeglich zu
        // einem alten Abo.
        s.empfangen(werte(1, None, vec![(0, Wert::Zahl(1.0))]), t1, &z);
        assert_eq!(z.ereignisse.lock().len(), 1);
        s.empfangen(
            status(1, Some(1), vec![(0, NameStatus::Fehlt), (1, da())]),
            t1,
            &z,
        );
        // N1: Wert fuer den als „fehlt" gemeldeten Draht-Index 0 verworfen.
        s.empfangen(
            werte(
                1,
                Some(1),
                vec![
                    (0, Wert::Zahl(7.0)),
                    (1, Wert::Zahl(4.0)),
                    (9, Wert::Zahl(9.0)),
                ],
            ),
            t1,
            &z,
        );
        {
            let e = z.ereignisse.lock();
            match &e[1] {
                Ereignis::Status { abo: 1, st, .. } => {
                    assert_eq!(st, &vec![(0, NameStatus::Fehlt), (2, da())])
                }
                x => panic!("{x:?}"),
            }
            match &e[2] {
                Ereignis::Werte { abo: 1, v, .. } => assert_eq!(v, &vec![(2, Wert::Zahl(4.0))]),
                x => panic!("{x:?}"),
            }
        }
        // PING nach 2 s.
        let t2 = t1 + Duration::from_millis(2100);
        s.empfangen(Antwort::Sonstige("pong".into()), t2, &z);
        s.empfangen(werte(1, Some(1), vec![(1, Wert::Zahl(4.0))]), t2, &z);
        assert_eq!(text_von(&s.takt(t2, &z)), vec!["PING\n"]);
        // 3 s Stille → zu, sofort neues HALLO.
        let t3 = t2 + Duration::from_millis(3100);
        assert_eq!(text_von(&s.takt(t3, &z)), vec!["HALLO 2 1.9.11\n"]);
        assert!(!s.offen());
        assert!(matches!(
            z.ereignisse.lock().last(),
            Some(Ereignis::SitzungZu)
        ));
        assert!(s.info().je_offen);
    }

    /// Altes Plugin: Antwort mit zu kleiner Version oeffnet keine Sitzung.
    #[test]
    fn zu_altes_plugin_oeffnet_keine_sitzung() {
        let z = TestZiel::default();
        let mut s = Sitzung::neu("1.9.11");
        let t0 = Instant::now();
        s.takt(t0, &z);
        s.empfangen(hallo_antwort("0.9.0"), t0, &z);
        assert!(!s.offen());
        assert!(s.info().zu_alt);
        assert!(z.ereignisse.lock().is_empty());
        // Werte ohne Sitzung werden nicht weitergereicht.
        s.empfangen(werte(1, None, vec![(0, Wert::Zahl(1.0))]), t0, &z);
        assert!(z.ereignisse.lock().is_empty());
        // Fehler „protokoll" ohne Sitzung → ebenfalls zu alt.
        let mut s2 = Sitzung::neu("1.9.11");
        s2.empfangen(
            Antwort::Fehler {
                grund: "protokoll_unbekannt".into(),
                abo: None,
                id: None,
                gen: None,
            },
            t0,
            &z,
        );
        assert!(s2.info().zu_alt);
    }

    /// `kein_hallo` in der Sitzung: sofort zu, beim naechsten Takt HALLO.
    /// Fehler mit Abo/ID/Generation werden mit diesen Angaben gelesen.
    #[test]
    fn kein_hallo_schliesst_die_sitzung() {
        assert_eq!(
            antwort_lesen(&json(
                r#"{"p":2,"t":"fehler","grund":"liste_nicht_verfuegbar","id":4}"#
            )),
            Some(Antwort::Fehler {
                grund: "liste_nicht_verfuegbar".into(),
                abo: None,
                id: Some(4),
                gen: None
            })
        );
        assert_eq!(
            antwort_lesen(&json(
                r#"{"p":2,"t":"fehler","grund":"speicher","abo":3,"gen":9}"#
            )),
            Some(Antwort::Fehler {
                grund: "speicher".into(),
                abo: Some(3),
                id: None,
                gen: Some(9)
            })
        );
        let z = TestZiel::default();
        let mut s = Sitzung::neu("1");
        let t = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t, &z);
        assert!(s.offen());
        s.empfangen(
            Antwort::Fehler {
                grund: "kein_hallo".into(),
                abo: None,
                id: None,
                gen: None,
            },
            t,
            &z,
        );
        assert!(!s.offen());
        assert!(matches!(
            z.ereignisse.lock().last(),
            Some(Ereignis::SitzungZu)
        ));
        assert_eq!(text_von(&s.takt(t, &z)), vec!["HALLO 2 1\n"]);
    }

    /// Wunschaenderung: geaendertes Abo neu (neue Generation), weggefallenes
    /// abbestellt, unveraendertes nicht erneut gesendet.
    #[test]
    fn abgleich_der_wuensche() {
        let z = TestZiel::default();
        let katalog = Arc::new(namen(&["sim/a"]));
        *z.wuensche.lock() = vec![
            AboWunsch {
                id: 1,
                rate: 50,
                namen: Arc::clone(&katalog),
            },
            AboWunsch {
                id: 2,
                rate: 20,
                namen: Arc::new(namen(&["sim/z"])),
            },
        ];
        let mut s = Sitzung::neu("1");
        let t = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t, &z);
        let mut erste = ohne_aufraeumen(text_von(&s.takt(t, &z)));
        erste.sort();
        assert_eq!(erste, vec!["ABO 1 50 g1\nsim/a\n", "ABO 2 20 g2\nsim/z\n"]);
        // Gleiche Generation: nichts.
        assert!(s.takt(t, &z).is_empty());
        *z.wuensche.lock() = vec![
            AboWunsch {
                id: 1,
                rate: 50,
                namen: Arc::new(namen(&["sim/a"])),
            },
            AboWunsch {
                id: 3,
                rate: 5,
                namen: Arc::new(namen(&["sim/m"])),
            },
        ];
        *z.gen.lock() += 1;
        let mut zweite = text_von(&s.takt(t, &z));
        zweite.sort();
        assert_eq!(zweite, vec!["ABO 3 5 g3\nsim/m\n", "ENDE-ABO 2\n"]);
        assert_eq!(text_von(&s.beenden()), vec!["ENDE-ABO 1\n", "ENDE-ABO 3\n"]);
    }

    /// N2: Status/Werte einer frueheren Generation derselben ID bestaetigen
    /// den neuen Inhalt nicht.
    #[test]
    fn alte_generation_wird_ignoriert() {
        let z = TestZiel::default();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 2,
            rate: 20,
            namen: Arc::new(namen(&["sim/alt"])),
        }];
        let mut s = Sitzung::neu("1");
        let t = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t, &z);
        s.takt(t, &z); // g1
        *z.wuensche.lock() = vec![AboWunsch {
            id: 2,
            rate: 20,
            namen: Arc::new(namen(&["sim/neu"])),
        }];
        *z.gen.lock() += 1;
        assert!(text_von(&s.takt(t, &z)).contains(&"ABO 2 20 g2\nsim/neu\n".to_string()));
        let vorher = z.ereignisse.lock().len();
        s.empfangen(status(2, Some(1), vec![(0, da())]), t, &z);
        s.empfangen(werte(2, Some(1), vec![(0, Wert::Zahl(1.0))]), t, &z);
        assert_eq!(
            z.ereignisse.lock().len(),
            vorher,
            "alte Generation durchgelassen"
        );
        s.empfangen(status(2, Some(2), vec![(0, da())]), t, &z);
        s.empfangen(werte(2, Some(2), vec![(0, Wert::Zahl(2.0))]), t, &z);
        assert_eq!(z.ereignisse.lock().len(), vorher + 2);
    }

    /// Kein Status → nach der Wartezeit identisch (gleiche Generation) neu;
    /// versiegen die Werte trotz vorhandener Namen, ebenfalls.
    #[test]
    fn abo_ohne_antwort_wird_wiederholt() {
        let z = TestZiel::default();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 1,
            rate: 50,
            namen: Arc::new(namen(&["sim/a"])),
        }];
        let mut s = Sitzung::neu("1");
        let t = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t, &z);
        assert_eq!(ohne_aufraeumen(text_von(&s.takt(t, &z))).len(), 1);
        // Lebenszeichen ohne Status (pong), damit die Sitzung offen bleibt.
        let t1 = t + Duration::from_millis(2100);
        s.empfangen(Antwort::Sonstige("pong".into()), t1, &z);
        let raus = text_von(&s.takt(t1, &z));
        assert!(
            raus.contains(&"ABO 1 50 g1\nsim/a\n".to_string()),
            "{raus:?}"
        );
        s.empfangen(status(1, Some(1), vec![(0, da())]), t1, &z);
        // Nur pong, keine Werte: nach 3 s wird neu angemeldet.
        let t2 = t1 + Duration::from_millis(3100);
        s.empfangen(Antwort::Sonstige("pong".into()), t2, &z);
        let raus = text_von(&s.takt(t2, &z));
        assert!(
            raus.contains(&"ABO 1 50 g1\nsim/a\n".to_string()),
            "{raus:?}"
        );
    }

    /// Simuliert ein LANGSAMES Plugin mit einem Mess-Abo aus 8192 Namen
    /// (H1). Bewiesen wird:
    /// * die Wartezeit waechst mit der Namenszahl, kein Neusenden nach 2 s;
    /// * ohne Status: eine Wiederholung, dann `AboOhneAntwort`, danach nur
    ///   noch mit Rueckoff — kein Endlos-Neusenden;
    /// * PINGs gehen auch mitten im grossen ABO-Schub hinaus.
    #[test]
    fn langsames_plugin_kein_endloses_neusenden() {
        let z = TestZiel::default();
        let viele: Vec<String> = (0..8192).map(|i| format!("sim/mess/wert_{i:05}")).collect();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 3,
            rate: 5,
            namen: Arc::new(viele),
        }];
        let mut s = Sitzung::neu("1");
        let t0 = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t0, &z);
        let mut abo_kopfzeilen: Vec<(Duration, String)> = Vec::new();
        let mut pings: Vec<Duration> = Vec::new();
        let mut schritt = Duration::ZERO;
        // 60 s in 20-ms-Schritten; das Plugin haelt die Sitzung mit pong am
        // Leben, antwortet aber nie mit einem Status.
        while schritt < Duration::from_secs(60) {
            let jetzt = t0 + schritt;
            s.empfangen(Antwort::Sonstige("pong".into()), jetzt, &z);
            for d in text_von(&s.takt(jetzt, &z)) {
                if d.starts_with("PING") {
                    pings.push(schritt);
                } else if d.starts_with("ABO 3 5 1 ") {
                    abo_kopfzeilen.push((schritt, d.lines().next().unwrap().to_string()));
                }
            }
            schritt += Duration::from_millis(20);
        }
        // Erste Sendung, 1 Wiederholung nach ≈ 6,1 s, Meldung ohne Sendung
        // nach ≈ 12,2 s, dann Rueckoff (12,2 s, 24,4 s …): 60 s → 4 Sendungen.
        let zeiten: Vec<f64> = abo_kopfzeilen
            .iter()
            .map(|(t, _)| t.as_secs_f64())
            .collect();
        assert_eq!(zeiten.len(), 4, "{zeiten:?}");
        assert!(zeiten[2] - zeiten[1] > 17.0, "kein Rueckoff: {zeiten:?}");
        assert!(
            zeiten[1] - zeiten[0] > 5.5,
            "zu frueh wiederholt: {zeiten:?}"
        );
        // Identisches Neusenden: dieselbe Generation.
        assert!(abo_kopfzeilen.iter().all(|(_, k)| k.ends_with(" g1")));
        let meldungen = z
            .ereignisse
            .lock()
            .iter()
            .filter(|e| matches!(e, Ereignis::AboOhneAntwort { abo: 3, .. }))
            .count();
        assert_eq!(meldungen, 1, "AboOhneAntwort genau einmal");
        // PINGs regelmaessig, auch waehrend die ersten Teile hinausgehen.
        assert!(pings
            .windows(2)
            .all(|w| (w[1] - w[0]) <= Duration::from_millis(2100)));
        assert!(pings.len() >= 28, "{}", pings.len());
    }

    /// Dasselbe langsame Plugin, aber es antwortet nach 5 s (ein identisches
    /// Neu-ABO setzt es nicht zurueck): genau eine Sendung, Status kommt an.
    #[test]
    fn langsames_plugin_mit_spaetem_status() {
        let z = TestZiel::default();
        let viele: Vec<String> = (0..8192).map(|i| format!("sim/mess/wert_{i:05}")).collect();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 3,
            rate: 5,
            namen: Arc::new(viele),
        }];
        let mut s = Sitzung::neu("1");
        let t0 = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t0, &z);
        let mut sendungen = 0;
        let mut schritt = Duration::ZERO;
        while schritt < Duration::from_secs(20) {
            let jetzt = t0 + schritt;
            s.empfangen(Antwort::Sonstige("pong".into()), jetzt, &z);
            if schritt == Duration::from_secs(5) {
                s.empfangen(status(3, Some(1), vec![(0, da())]), jetzt, &z);
            }
            if schritt >= Duration::from_secs(5) {
                s.empfangen(werte(3, Some(1), vec![(0, Wert::Zahl(1.0))]), jetzt, &z);
            }
            sendungen += text_von(&s.takt(jetzt, &z))
                .iter()
                .filter(|d| d.starts_with("ABO 3 5 1 "))
                .count();
            schritt += Duration::from_millis(20);
        }
        assert_eq!(sendungen, 1);
        assert!(!z
            .ereignisse
            .lock()
            .iter()
            .any(|e| matches!(e, Ereignis::AboOhneAntwort { .. })));
    }

    /// `abo_empfangen` (Plugin ab Cloud-QS-Stand): das Abo ist angenommen,
    /// der Status kommt spaet (hier 9 s — ohne Annahme waere nach ≈ 6,1 s
    /// neu gesendet worden; gemessen im Plugin bis ≈ 3,7 s). Kein Neusenden,
    /// keine Meldung.
    #[test]
    fn abo_empfangen_verlaengert_die_wartezeit() {
        assert_eq!(
            antwort_lesen(&json(
                r#"{"p":2,"t":"abo_empfangen","abo":3,"gen":7,"namen":8192}"#
            )),
            Some(Antwort::AboEmpfangen {
                abo: 3,
                gen: Some(7),
                namen: Some(8192)
            })
        );
        assert_eq!(
            status_wartezeit_angenommen(8192),
            Duration::from_millis(14_096)
        );
        let z = TestZiel::default();
        let viele: Vec<String> = (0..8192).map(|i| format!("sim/mess/wert_{i:05}")).collect();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 3,
            rate: 5,
            namen: Arc::new(viele),
        }];
        let mut s = Sitzung::neu("1");
        let t0 = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t0, &z);
        let mut sendungen = 0;
        let mut schritt = Duration::ZERO;
        while schritt < Duration::from_secs(20) {
            let jetzt = t0 + schritt;
            s.empfangen(Antwort::Sonstige("pong".into()), jetzt, &z);
            sendungen += text_von(&s.takt(jetzt, &z))
                .iter()
                .filter(|d| d.starts_with("ABO 3 5 1 "))
                .count();
            if schritt == Duration::from_millis(400) {
                s.empfangen(
                    Antwort::AboEmpfangen {
                        abo: 3,
                        gen: Some(1),
                        namen: Some(8192),
                    },
                    jetzt,
                    &z,
                );
            }
            if schritt == Duration::from_secs(9) {
                s.empfangen(status(3, Some(1), vec![(0, da())]), jetzt, &z);
            }
            if schritt >= Duration::from_secs(9) {
                s.empfangen(werte(3, Some(1), vec![(0, Wert::Zahl(1.0))]), jetzt, &z);
            }
            schritt += Duration::from_millis(20);
        }
        assert_eq!(sendungen, 1, "unnoetig neu gesendet");
        assert!(!z
            .ereignisse
            .lock()
            .iter()
            .any(|e| matches!(e, Ereignis::AboOhneAntwort { .. })));
    }

    /// `abo_empfangen` mit anderer Namenszahl oder fremder Generation.
    #[test]
    fn abo_empfangen_mit_falscher_namenszahl_sendet_neu() {
        let z = TestZiel::default();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 2,
            rate: 20,
            namen: Arc::new(namen(&["sim/a", "sim/b", "sim/c"])),
        }];
        let mut s = Sitzung::neu("1");
        let t = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t, &z);
        let erste = ohne_aufraeumen(text_von(&s.takt(t, &z)));
        assert_eq!(erste, vec!["ABO 2 20 g1\nsim/a\nsim/b\nsim/c\n"]);
        // Fremde Generation: nichts.
        s.empfangen(
            Antwort::AboEmpfangen {
                abo: 2,
                gen: Some(9),
                namen: Some(1),
            },
            t,
            &z,
        );
        assert!(s.takt(t, &z).is_empty());
        // Richtige Generation, falsche Zahl → neu mit g2.
        s.empfangen(
            Antwort::AboEmpfangen {
                abo: 2,
                gen: Some(1),
                namen: Some(2),
            },
            t,
            &z,
        );
        assert_eq!(
            text_von(&s.takt(t, &z)),
            vec!["ABO 2 20 g2\nsim/a\nsim/b\nsim/c\n"]
        );
        // Status zur alten Generation zaehlt nicht mehr.
        let vorher = z.ereignisse.lock().len();
        s.empfangen(status(2, Some(1), vec![(0, da())]), t, &z);
        assert_eq!(z.ereignisse.lock().len(), vorher);
        // Passende Annahme: kein weiteres Senden.
        s.empfangen(
            Antwort::AboEmpfangen {
                abo: 2,
                gen: Some(2),
                namen: Some(3),
            },
            t,
            &z,
        );
        // Lebenszeichen, sonst schloesse die Sitzung nach 3 s Stille.
        s.empfangen(
            Antwort::Sonstige("pong".into()),
            t + Duration::from_secs(5),
            &z,
        );
        assert_eq!(
            text_von(&s.takt(t + Duration::from_secs(5), &z)),
            vec!["PING\n"]
        );
    }

    /// Unbekannte Antworttypen eines neueren Plugins: gelesen als
    /// `Sonstige`, zaehlen als Lebenszeichen, loesen sonst nichts aus.
    #[test]
    fn unbekannte_typen_werden_still_ignoriert() {
        let a = antwort_lesen(&json(r#"{"p":2,"t":"zukunft","abo":1,"x":[1,2]}"#)).unwrap();
        assert_eq!(a, Antwort::Sonstige("zukunft".into()));
        let z = TestZiel::default();
        let mut s = Sitzung::neu("1");
        let t = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t, &z);
        let vorher = z.ereignisse.lock().len();
        s.empfangen(a, t + Duration::from_millis(2900), &z);
        assert_eq!(z.ereignisse.lock().len(), vorher);
        // Lebenszeichen: bei 5 s noch offen.
        s.takt(t + Duration::from_secs(5), &z);
        assert!(s.offen());
    }

    #[test]
    fn stille_schwelle_je_abo() {
        assert_eq!(stille_fuer(1, 50, 120), STILLE);
        assert_eq!(stille_fuer(1, 50, 8192), STILLE, "Katalog bleibt bei 3 s");
        assert_eq!(stille_fuer(2, 20, 10), STILLE);
        assert_eq!(stille_fuer(3, 1, 10), Duration::from_secs(5));
        assert_eq!(stille_fuer(3, 5, 8192), Duration::from_millis(9_192));
    }

    /// Nachpruefung N2: 8192-Namen-Mess-Abo, das Plugin liefert nur alle
    /// 4,5 s eine Runde (Rundlauf ueber 14 grosse Abos) — kein Neusenden.
    /// Bleiben die Werte dann ganz aus, wird nach der Schwelle neu gesendet.
    #[test]
    fn langsame_lieferrunden_sind_kein_versiegen() {
        let z = TestZiel::default();
        let viele: Vec<String> = (0..8192).map(|i| format!("sim/mess/wert_{i:05}")).collect();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 5,
            rate: 5,
            namen: Arc::new(viele),
        }];
        let mut s = Sitzung::neu("1");
        let t0 = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t0, &z);
        let mut sendungen: Vec<Duration> = Vec::new();
        let mut schritt = Duration::ZERO;
        let mut naechste_runde = Duration::from_secs(1);
        while schritt < Duration::from_secs(40) {
            let jetzt = t0 + schritt;
            s.empfangen(Antwort::Sonstige("pong".into()), jetzt, &z);
            if schritt == Duration::from_millis(500) {
                s.empfangen(status(5, Some(1), vec![(0, da())]), jetzt, &z);
            }
            // Lieferrunden alle 4,5 s bis 25 s, dann nichts mehr.
            if schritt >= naechste_runde && schritt < Duration::from_secs(25) {
                s.empfangen(werte(5, Some(1), vec![(0, Wert::Zahl(1.0))]), jetzt, &z);
                naechste_runde += Duration::from_millis(4500);
            }
            if text_von(&s.takt(jetzt, &z))
                .iter()
                .any(|d| d.starts_with("ABO 5 5 1 "))
            {
                sendungen.push(schritt);
            }
            schritt += Duration::from_millis(20);
        }
        assert_eq!(sendungen[0], Duration::ZERO);
        assert!(
            sendungen[1..].iter().all(|t| *t > Duration::from_secs(25)),
            "Neusenden trotz Lieferung: {sendungen:?}"
        );
        assert!(
            sendungen.len() >= 2,
            "versiegte Werte nicht bemerkt: {sendungen:?}"
        );
    }

    /// Grosse Datagramme gehen mit Abstand hinaus; Vorrang (PING, ENDE-ABO)
    /// wartet nicht hinter ihnen.
    #[test]
    fn grosse_datagramme_mit_abstand_vorrang_sofort() {
        let z = TestZiel::default();
        let viele: Vec<String> = (0..3000).map(|i| format!("sim/name/{i:06}")).collect();
        *z.wuensche.lock() = vec![AboWunsch {
            id: 3,
            rate: 5,
            namen: Arc::new(viele),
        }];
        let mut s = Sitzung::neu("1");
        let t = Instant::now();
        s.empfangen(hallo_antwort("1.0.0"), t, &z);
        let erste = ohne_aufraeumen(text_von(&s.takt(t, &z)));
        // Nur das erste grosse; der Rest wartet.
        assert_eq!(erste.len(), 1);
        assert!(erste[0].len() > GROSS);
        assert!(s.takt(t + Duration::from_millis(5), &z).is_empty());
        // Wunsch zurueckgezogen: ENDE-ABO sofort, wartende Teile verworfen.
        *z.wuensche.lock() = Vec::new();
        *z.gen.lock() += 1;
        assert_eq!(
            text_von(&s.takt(t + Duration::from_millis(10), &z)),
            vec!["ENDE-ABO 3\n"]
        );
        assert!(s.takt(t + Duration::from_millis(100), &z).is_empty());
        assert_eq!(s.ausgang_bytes, 0);
    }
}
