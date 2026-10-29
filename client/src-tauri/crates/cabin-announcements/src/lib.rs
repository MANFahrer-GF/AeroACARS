//! Kabinenansagen für AeroACARS (Beta, ab v2.0.25).
//!
//! Pure Logik ohne tauri/reqwest, damit alles ohne Simulator testbar ist:
//! - [`slot`]: der Ansage-Katalog (wer spricht, fest oder KI, Länge)
//! - [`flugart`]: Linie / Bizjet / aus (Fracht)
//! - [`zeit`]: Tageszeit und Ortszeit aus dem SimBrief-Plan
//! - [`planer`]: wann welche Ansage fällig wird
//! - [`warteschlange`]: nichts überlappt, Ruhephase, Verfall
//! - [`drehbuch`]: KI-Texte prüfen, bevor sie gesprochen werden
//! - [`cache`]: Dateinamen für den Audio-Cache
//! - [`musik`]: wann Boarding-Musik spielt und ob ein Lied angefragt wird
//!
//! Grundlage: LEITFADEN_Ansage-Drehbuch.md (Nextcloud,
//! Kabinenansagen_2026-10-17).

pub mod cache;
pub mod drehbuch;
pub mod flugart;
pub mod musik;
pub mod planer;
pub mod reaktion;
pub mod slot;
pub mod warteschlange;
pub mod zeit;

pub use flugart::Flugart;
pub use planer::{Entfall, Faellig, Flugdaten, Planer, Tick, Verfall};
pub use slot::{Rolle, Slot};
pub use warteschlange::Warteschlange;

// ---- Laufzeit-Paket (3.0, P-3.59): `tabellen/kabine.json` ----

/// Ein Wert mit Einheit und Herkunft im Code von v2.1.24 (wie die Grenzwerte
/// des Clients). Gelesen wird nur `wert`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Grenzwert<T> {
    pub wert: T,
    #[allow(dead_code)]
    einheit: String,
    #[allow(dead_code)]
    quelle: String,
}

/// Zeiten und Schwellen von Planer, Reaktion, Warteschlange und Musik.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Werte {
    /// Grenze für „Kurzstrecke“: darunter nur die Service-Ansage (Reiseflug 25),
    /// keine Zwischenstände (Thomas 18.10.2026).
    pub kurzstrecke_min: Grenzwert<u32>,
    /// Kurzstrecke: Die Service-Ansage braucht so viel Restflugzeit, sonst
    /// entfällt sie. Der Kapitän sagt den Sinkflug bei 25 min Rest an; dazu kommen
    /// die Ansage selbst und die Pause zwischen zwei Ansagen.
    pub kurzstrecke_service_rest_min: Grenzwert<f64>,
    /// Angenommene Dauer von Sinkflug und Anflug ohne TOD aus dem OFP.
    pub sinkflug_min: Grenzwert<f64>,
    /// Mindestabstand zwischen zwei Reiseflug-Ansagen.
    pub reiseflug_abstand_s: Grenzwert<i64>,
    /// Mindestabstand zwischen zwei Turbulenz-Ansagen.
    pub turbulenz_abstand_s: Grenzwert<i64>,
    /// Verspätungsansage ab so vielen Minuten …
    pub verspaetung_ab_min: Grenzwert<i32>,
    /// … aber nicht darüber: das ist Fliegen außerhalb des Plans.
    pub verspaetung_bis_min: Grenzwert<i32>,
    /// Durchstart-Erklärung erst so viele Sekunden nach dem Durchstarten.
    pub durchstart_verzoegerung_s: Grenzwert<i64>,
    /// Warteschleifen-Ansage, sobald die Schleife so lange läuft.
    pub warteschleife_nach_s: Grenzwert<i64>,
    /// Die Kabine bereitet die Landung frühestens so lange nach der Sinkflug-
    /// Ansage des Kapitäns vor (Thomas 18.10.2026: „sofort danach die Kabine,
    /// das geht nicht“) …
    pub kabine_nach_kapitaen_min_s: Grenzwert<i64>,
    /// Die Kabine bereitet die Landung erst unter dieser Höhe im Sinkflug vor
    /// (Thomas 22.10.2026: kam bei 6 von 15 Flügen über 28.000 ft, weil sie
    /// „spätestens 5 min nach dem Kapitän“ auch auf Reiseflughöhe fällig wurde).
    pub kabine_sinkflug_unter_ft: Grenzwert<f64>,
    /// Der Kapitän kündigt den Sinkflug so lange vor dem Top of Descent an
    /// (Thomas 22.10.2026: bis zu 5,7 min davor war zu früh; „lieber später als
    /// zu früh“) …
    pub kapitaen_vor_tod_s: Grenzwert<i64>,
    /// … bzw. so viele Minuten vor der erwarteten Landung (vorher 25).
    pub kapitaen_vor_landung_min: Grenzwert<f64>,
    /// Die Phase „Sinkflug“ allein löst die Ansage nur aus, solange die Restzeit
    /// höchstens so lang ist (GAF278 OMDB–LOWW 24.10.2026: Ein Stufen-Sinkflug von
    /// 2,5 min mitten im Reiseflug spielte sie 259 min vor dem Endanflug, „noch
    /// 265 min“). Gemessen über 90 Flüge: gespielt 10–52 min vor dem Endanflug,
    /// Restzeit beim Abspielen höchstens 40 min (eine Umleitung 60). Nach einer
    /// Umleitung gilt die Grenze nicht (die Restzeit meint das alte Ziel).
    pub phase_sinkflug_hoechstens_rest_min: Grenzwert<f64>,
    /// Ende des Bordverkaufs bzw. zweiter Service so lange vor dem Top of Descent
    /// (Thomas 18.10.2026: „10, 15 Minuten vor Top of Descent“).
    pub reiseflug80_vor_tod_s: Grenzwert<i64>,
    /// Der zweite Service (Modul-Art `zweiter_service`) beginnt so lange vor dem
    /// Top of Descent (Thomas 19.10.2026: „so anderthalb Stunden, bevor wir mit
    /// dem Sinkflug sind“) — das Ende des Bordverkaufs bleibt bei
    /// [`REISEFLUG80_VOR_TOD_S`].
    pub zweiter_service_vor_tod_s: Grenzwert<i64>,
    /// Zweiter Service nur, wenn danach noch so viel Restflugzeit bleibt (min):
    /// 30 min Service vor dem Sinkflug plus Sinkflug und Anflug.
    pub zweiter_service_rest_min: Grenzwert<f64>,
    /// „Plätze für die Landung“ im Anflug spätestens unter dieser Höhe über Grund,
    /// wenn das Fahrwerk nicht vorher ausfährt.
    pub plaetze_landung_agl_ft: Grenzwert<f64>,
    /// Boarding-Begrüßung, sobald die Zuladung so weit über dem Bezugswert liegt
    /// (erster ZFW-Wert der Phase Boarding) … (Thomas 19.10.2026: „Gewicht steigt
    /// → 30 s warten → abspielen“; jeder Anstieg zählt, auch Fracht zuerst)
    pub boarding_zuladung_kg: Grenzwert<f64>,
    /// … und das ununterbrochen so lange (Sekunden).
    pub boarding_zuladung_s: Grenzwert<i64>,
    /// Boarding-Begrüßung auch, wenn der ZFW so nah am geplanten ZFW des OFP
    /// liegt (der größere der beiden Werte) … (20.10.2026, Peter SVA 179: „Load
    /// instant“ von der Standardladung des Add-ons herunter auf den Plan; an 430
    /// Flügen gemessen: ohne Boarding-Musik 16 % statt 42 %, siehe
    /// handoff/2026-10-20-boarding-erkennung.md)
    pub boarding_ofp_kg: Grenzwert<f64>,
    pub boarding_ofp_anteil: Grenzwert<f64>,
    /// … und das ununterbrochen so lange (Sekunden).
    pub boarding_ofp_s: Grenzwert<i64>,
    /// „Rollt“ für „Plätze für den Start“ ab dieser Geschwindigkeit über Grund
    /// (Thomas 19.10.2026, Entscheidung 3).
    pub rollt_ab_kt: Grenzwert<f64>,
    /// Erste Service-Ansage außerhalb der Kurzstrecke so lange nach Erreichen der
    /// Reiseflughöhe (v2.0.28, Peter: „Service auf Langstrecke zu spät“).
    pub service_nach_reiseflug_s: Grenzwert<i64>,
    /// So lange gilt der zuletzt aus der Route bekannte TOD ohne neuen Wert.
    pub tod_frisch_s: Grenzwert<i64>,
    /// Schalter AUS zählt für „Anschnallzeichen aus“ erst, wenn er so lange
    /// ununterbrochen aus steht. BTI 252 (A220, 27.10.2026): 6 s AUS beim
    /// Sinkflugbeginn (ON → OFF → ON) meldete die Ansage; nur die Pause
    /// verhinderte sie. Auch der Weg über AUTO dauerte beim Boarding 6 s.
    pub anschnall_aus_stabil_s: Grenzwert<i64>,
    /// Ab diesem Sinken (fpm) wird „Anschnallzeichen aus“ nicht mehr gemeldet:
    /// Abschnallen ansagen, während das Flugzeug sinkt, ist falsch (BTI 252:
    /// −3.160 fpm, Phase stand noch auf Reiseflug).
    pub anschnall_aus_sinken_fpm: Grenzwert<f64>,
    /// A+ auf einem Ferienflieger: so oft Applaus.
    pub applaus_ferien: Grenzwert<f64>,
    /// A+ sonst: etwa jedes zweite Mal.
    pub applaus_sonst: Grenzwert<f64>,
    /// A: so oft zufriedenes Murmeln.
    pub murmeln_bei_a: Grenzwert<f64>,
    /// Pause zwischen fester Note und Applaus/Murmeln (s), zufällig dazwischen —
    /// es soll überraschen, nicht wie ein Taster wirken.
    pub pause_min_s: Grenzwert<f64>,
    pub pause_max_s: Grenzwert<f64>,
    /// Raunen kommt sofort nach der Note.
    pub pause_raunen_s: Grenzwert<f64>,
    /// Nach dem Aufsetzen so lange darf die Reaktion noch beginnen; später
    /// (Neustart, lange Ansage davor) entfällt sie — sie gehört zum Aufsetzen.
    /// Die Note steht nach ~9 s fest, „nach der Landung“ kam gemessen nach
    /// 35–52 s.
    pub spaetestens_nach_aufsetzen_s: Grenzwert<i64>,
    /// Ruhe nach einer gesprochenen Ansage, bevor die nächste reguläre kommt.
    /// Kommandos, Gong, Turbulenz und Durchstarten warten nicht ([`Slot::wartet_pause`]).
    pub pause_nach_ansage_s: Grenzwert<i64>,
    /// Ausnahme: Die Sicherheitshinweise folgen der Begrüßung „Türen zu“ nach
    /// höchstens so vielen Sekunden (Thomas 18.10.2026); alles andere wartet 90 s.
    pub pause_begruessung_sicherheit_s: Grenzwert<i64>,
    /// Am Boden folgt die Kapitän-Begrüßung den Sicherheitshinweisen nach
    /// höchstens so vielen Sekunden — sonst rutscht sie bei kurzem Rollweg hinter
    /// den Start (v2.0.28, QAF446). Reicht es nicht, spricht er nach dem Start.
    pub pause_sicherheit_kapitaen_s: Grenzwert<i64>,
    /// Nachfrist nach dem Zurückhalten (s): Spielt der Slot darin nicht (etwa
    /// wegen der Ruhephase), verfällt er wie gewohnt.
    pub nachfrist_s: Grenzwert<i64>,
    /// Musik nach der Landung endet spätestens so lange nach „Aussteigen“ …
    pub ankunft_nach_aussteigen_s: Grenzwert<i64>,
    /// … ohne Aussteigen-Ansage so lange nach „Triebwerke aus“.
    pub ankunft_nach_triebwerke_aus_s: Grenzwert<i64>,
}

/// Listen fuer Flugart und Reaktion.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listen {
    /// Muster ohne Kabinenbesatzung (ICAO-Typ): Business-Jets,
    /// Kleinflugzeuge, Turboprops ohne Flugbegleiter, Hubschrauber, Militaer.
    pub ohne_kabinencrew: Vec<String>,
    /// phpVMS-Flugarten ohne Passagiere (F, A, H, M, P).
    pub ohne_passagiere: Vec<String>,
    /// Ferienairlines (ICAO, aus der GSG-Datenbank; abgenommen 23.10.2026).
    pub ferienairlines: Vec<String>,
    /// Urlaubsziele (ICAO; abgenommen 23.10.2026). Grossstaedte bewusst nicht.
    pub urlaubsziele: Vec<String>,
}

/// Inhalt von `tabellen/kabine.json`.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tabelle {
    format: u32,
    #[serde(default)]
    #[allow(dead_code)]
    notiz: Vec<String>,
    pub listen: Listen,
    pub werte: Werte,
}

/// Eingebauter Stand = Datei im Paketordner (eine Quelle).
pub const EINGEBAUT: &str = include_str!("../../../../../bundle/tabellen/kabine.json");

static QUELLE: std::sync::OnceLock<fn() -> &'static Tabelle> = std::sync::OnceLock::new();

/// Die geltende Kabinen-Tabelle. Der Client meldet beim Start eine Quelle an,
/// die aus seinem einen Paketstand liest (P-3.02); ohne Anmeldung (Tests)
/// gilt der eingebaute Stand.
pub fn tabelle() -> &'static Tabelle {
    match QUELLE.get() {
        Some(quelle) => quelle(),
        None => {
            static EIGEN: std::sync::OnceLock<Tabelle> = std::sync::OnceLock::new();
            EIGEN.get_or_init(|| lesen(EINGEBAUT).expect("eingebaute Kabinen-Tabelle"))
        }
    }
}

/// Meldet die Quelle an (einmal je Lauf). `false`, wenn schon eine gilt.
pub fn quelle_setzen(quelle: fn() -> &'static Tabelle) -> bool {
    QUELLE.set(quelle).is_ok()
}

/// Liest `tabellen/kabine.json` (fail-closed): Format 1, Kennungen in
/// Grossbuchstaben und eindeutig je Liste.
pub fn lesen(json: &str) -> Result<Tabelle, String> {
    let t: Tabelle = serde_json::from_str(json).map_err(|e| format!("tabellen/kabine.json: {e}"))?;
    if t.format != 1 {
        return Err(format!("tabellen/kabine.json: Format {} unbekannt", t.format));
    }
    let l = &t.listen;
    for (name, liste) in [
        ("ohne_kabinencrew", &l.ohne_kabinencrew),
        ("ohne_passagiere", &l.ohne_passagiere),
        ("ferienairlines", &l.ferienairlines),
        ("urlaubsziele", &l.urlaubsziele),
    ] {
        let mut gesehen = std::collections::HashSet::new();
        for k in liste {
            if k.is_empty() || *k != k.trim().to_ascii_uppercase() || !gesehen.insert(k.as_str()) {
                return Err(format!("tabellen/kabine.json: {name}: {k:?} ungueltig oder doppelt"));
            }
        }
    }
    Ok(t)
}
