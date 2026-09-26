//! Bordbuch (26.09.2026) — bestätigt, was der Pilot an den typischen
//! Stellen eines Flugs erledigt hat.
//!
//! Grundsätze (Konzept, von Thomas entschieden):
//! * Zählt Erledigtes. Was nicht passiert ist, heisst „diesmal ohne" —
//!   nie Fehler, nie Punkte, kein Rot.
//! * Kein Einfluss auf Landebewertung, PIREP oder Freigabe. Dieses Modul
//!   liest nur den Snapshot; nichts ausserhalb liest seine Ergebnisse
//!   ausser Anzeige, Speicher und Server-Sicherung.
//! * Was das Flugzeug nicht meldet, heisst „nicht messbar" — mit Grund,
//!   zählt weder mit noch gegen den Piloten, wird nie still weggelassen.
//! * Punktarten: **Pflicht** (abgehakt oder „diesmal ohne") und
//!   **Bestätigung** (Empfehlung — erscheint nur, wenn erledigt).
//! * Regeln hängen an Klasse (Airliner / Business Jet / GA), Ausrüstung
//!   und Tag/Nacht — nicht an IFR/VFR (Recherche 26.09.: 14 CFR 91.209,
//!   91.215, 121.317(b), AIM 4-3-24, 4-1-20, SERA.3215/13010, FSAV §4,
//!   Boeing-737-FCTM, C172S-POH).
//!
//! Alle Zeitfenster rechnen mit Zeitstempeln, nicht mit Takten: der
//! Streamer tickt zwischen 200 ms und 10 s.

use std::collections::BTreeMap;

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::{Deserialize, Serialize};
use sim_core::{AircraftProfile, FlightPhase, SimSnapshot, Simulator};

/// Schema des gespeicherten Eintrags. Hochzählen, wenn sich die Bedeutung
/// eines Feldes ändert.
pub const SCHEMA: u8 = 1;

/// Kulanz nach dem Auslöser: so lange darf der Pilot noch schalten.
const FRIST_BEACON_S: i64 = 15;
const FRIST_START_S: i64 = 10;
const FRIST_FL100_S: i64 = 60;
const FRIST_ROLLEN_S: i64 = 20;
/// Rollen über der Grenze zählt erst, wenn es länger als das dauert
/// (Audit 807 Flüge: 1–10 s beim Übergang zum Startlauf sind normal).
const ROLLEN_UEBER_S: f64 = 10.0;
/// Die ersten Sekunden nach dem Ausrollen gehören zum Abrollweg.
const ABROLLWEG_S: i64 = 30;
/// Reiseflug so lange stabil, bevor die APU geprüft wird.
const APU_NACH_REISEFLUG_S: i64 = 120;
/// Ab dieser Höhe (MSL) gilt ein Flug als „über FL100".
const FL100_UEBER_FT: f64 = 10_500.0;
const FL100_FT: f64 = 10_000.0;
/// Anflugfenster für Spoiler, Autobrake, Anschnallzeichen.
const ANFLUG_AGL_FT: f64 = 1_000.0;
/// Ab dieser Lücke zwischen zwei Takten gilt: pausiert (Fristen verschieben).
const PAUSE_AB_S: i64 = 15;
/// Hinweis im Flug: so lange sichtbar.
pub const HINWEIS_SICHTBAR_S: i64 = 8;
/// Höhenprofil für die Ansicht „Flugprofil": ein Punkt je Minute, höchstens.
const PROFIL_TAKT_S: i64 = 60;
const PROFIL_MAX: usize = 1_200;

// ---------------------------------------------------------------------------
// Grundtypen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Klasse {
    Airliner,
    BusinessJet,
    Ga,
}

/// Pflicht oder Bestätigung — je Regel, Klasse und Tageszeit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Art {
    Pflicht,
    Bestaetigung,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Regel {
    BeaconAnlassen,
    NavLichter,
    ParkbremseGeloest,
    RolltempoAbflug,
    StrobesStart,
    LandelichtStart,
    TransponderStart,
    TcasStart,
    KlappenStart,
    AnschnallStart,
    ApuReiseflug,
    LandelichtAnflug,
    AutobrakeLandung,
    SpoilerLandung,
    AnschnallLandung,
    RolltempoAnkunft,
}

impl Regel {
    pub const ALLE: [Regel; 16] = [
        Regel::BeaconAnlassen,
        Regel::NavLichter,
        Regel::ParkbremseGeloest,
        Regel::RolltempoAbflug,
        Regel::StrobesStart,
        Regel::LandelichtStart,
        Regel::TransponderStart,
        Regel::TcasStart,
        Regel::KlappenStart,
        Regel::AnschnallStart,
        Regel::ApuReiseflug,
        Regel::LandelichtAnflug,
        Regel::AutobrakeLandung,
        Regel::SpoilerLandung,
        Regel::AnschnallLandung,
        Regel::RolltempoAnkunft,
    ];

    /// Einstellungs-Schalter, der die Regel ein- und ausschaltet. Mehrere
    /// Regeln teilen sich einen Schalter (Rolltempo ab/an, Transponder+TCAS,
    /// Anschnallzeichen Start+Landung, Landelicht Start+Anflug).
    pub fn schalter(self) -> Schalter {
        match self {
            Regel::BeaconAnlassen => Schalter::Beacon,
            Regel::NavLichter => Schalter::Nav,
            Regel::ParkbremseGeloest => Schalter::Parkbremse,
            Regel::RolltempoAbflug | Regel::RolltempoAnkunft => Schalter::Rolltempo,
            Regel::StrobesStart => Schalter::Strobes,
            Regel::LandelichtStart | Regel::LandelichtAnflug => Schalter::Landelicht,
            Regel::TransponderStart | Regel::TcasStart => Schalter::Transponder,
            Regel::KlappenStart => Schalter::Klappen,
            Regel::AnschnallStart | Regel::AnschnallLandung => Schalter::Anschnallzeichen,
            Regel::ApuReiseflug => Schalter::Apu,
            Regel::AutobrakeLandung => Schalter::Autobrake,
            Regel::SpoilerLandung => Schalter::Spoiler,
        }
    }

    /// Abschnitt der Checkliste.
    pub fn abschnitt(self) -> Abschnitt {
        match self {
            Regel::BeaconAnlassen | Regel::NavLichter | Regel::ParkbremseGeloest => {
                Abschnitt::VorDemRollen
            }
            Regel::RolltempoAbflug => Abschnitt::Rollen,
            Regel::StrobesStart
            | Regel::LandelichtStart
            | Regel::TransponderStart
            | Regel::TcasStart
            | Regel::KlappenStart
            | Regel::AnschnallStart => Abschnitt::Start,
            Regel::ApuReiseflug => Abschnitt::Reiseflug,
            Regel::LandelichtAnflug
            | Regel::AutobrakeLandung
            | Regel::SpoilerLandung
            | Regel::AnschnallLandung => Abschnitt::Anflug,
            Regel::RolltempoAnkunft => Abschnitt::NachDerLandung,
        }
    }

    fn index(self) -> usize {
        Regel::ALLE.iter().position(|r| *r == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Schalter {
    Beacon,
    Strobes,
    Nav,
    Transponder,
    Landelicht,
    Rolltempo,
    Parkbremse,
    Apu,
    Autobrake,
    Spoiler,
    Anschnallzeichen,
    Klappen,
}

impl Schalter {
    pub const ALLE: [Schalter; 12] = [
        Schalter::Beacon,
        Schalter::Strobes,
        Schalter::Nav,
        Schalter::Transponder,
        Schalter::Landelicht,
        Schalter::Rolltempo,
        Schalter::Parkbremse,
        Schalter::Apu,
        Schalter::Autobrake,
        Schalter::Spoiler,
        Schalter::Anschnallzeichen,
        Schalter::Klappen,
    ];

    /// Voreinstellung = Umfrage-Favoriten, die heute messbar sind
    /// (Forum #41: Strobes, Beacon, Nav, Transponder, Rolltempo, APU).
    pub fn voreinstellung(self) -> bool {
        matches!(
            self,
            Schalter::Beacon
                | Schalter::Strobes
                | Schalter::Nav
                | Schalter::Transponder
                | Schalter::Rolltempo
                | Schalter::Apu
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Abschnitt {
    VorDemRollen,
    Rollen,
    Start,
    Reiseflug,
    Anflug,
    NachDerLandung,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Noch nicht ausgewertet (läuft im Flug).
    Offen,
    Erledigt,
    /// Nicht gemacht — grau, ohne Wertung.
    DiesmalOhne,
    /// Vom Piloten nach dem Flug gesetzt: ATC hat anders angewiesen.
    NachAtc,
    /// Das Flugzeug meldet den Wert nicht (Grund in `grund`).
    NichtMessbar,
    /// Der Punkt kam in diesem Flug nicht vor (z. B. kein Reiseflug über
    /// FL100, Flug vor dem Start abgebrochen) oder passt nicht zur Klasse.
    NichtAnwendbar,
}

impl Status {
    pub fn zaehlt_als_erledigt(self) -> bool {
        matches!(self, Status::Erledigt | Status::NachAtc)
    }
}

/// Warum ein Punkt nicht messbar ist — Code, den die Oberfläche übersetzt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grund {
    /// Das Flugzeug liefert den Wert überhaupt nicht an AeroACARS.
    WertFehlt,
    /// Transponder meldet nur den Modus ohne TCAS-Stellungen.
    KeinTcasModus,
}

// ---------------------------------------------------------------------------
// Einstellungen
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Einstellungen {
    pub schema: u8,
    /// Letzte Änderung — beim Abgleich mit dem Server gewinnt die neuere.
    pub updated_at: Option<DateTime<Utc>>,
    /// Leiser Hinweis im Flug an Schlüsselstellen.
    pub hinweise_im_flug: bool,
    /// Bordbuch auch für Kleinflugzeuge (GA). Voreinstellung: aus.
    pub ga_an: bool,
    /// Bordbuch auch für VFR-Flüge. Voreinstellung: aus.
    pub vfr_an: bool,
    /// Schalter je Regelgruppe. Fehlt ein Eintrag, gilt die Voreinstellung.
    pub regeln: BTreeMap<Schalter, bool>,
    /// Rolltempo-Grenze für Airliner und Business Jets (kt).
    pub rolltempo_kt: f32,
    /// Rolltempo-Grenze für GA (kt). Keine Quelle nennt eine Zahl — eigene
    /// Toleranz.
    pub rolltempo_ga_kt: f32,
    /// Klasse von Hand: Schlüssel = ICAO-Typ oder Kennzeichen (gross).
    pub klassen_override: BTreeMap<String, Klasse>,
}

impl Default for Einstellungen {
    fn default() -> Self {
        Self {
            schema: SCHEMA,
            updated_at: None,
            hinweise_im_flug: true,
            ga_an: false,
            vfr_an: false,
            regeln: BTreeMap::new(),
            rolltempo_kt: 30.0,
            rolltempo_ga_kt: 20.0,
            klassen_override: BTreeMap::new(),
        }
    }
}

impl Einstellungen {
    pub fn an(&self, s: Schalter) -> bool {
        self.regeln
            .get(&s)
            .copied()
            .unwrap_or_else(|| s.voreinstellung())
    }

    /// Werte in sinnvolle Grenzen bringen (Eingaben aus der Oberfläche oder
    /// vom Server).
    pub fn bereinigt(mut self) -> Self {
        let grenze = |v: f32, d: f32| {
            if v.is_finite() && (5.0..=60.0).contains(&v) {
                v
            } else {
                d
            }
        };
        self.rolltempo_kt = grenze(self.rolltempo_kt, 30.0);
        self.rolltempo_ga_kt = grenze(self.rolltempo_ga_kt, 20.0);
        self.klassen_override = self
            .klassen_override
            .into_iter()
            .map(|(k, v)| (k.trim().to_uppercase(), v))
            .filter(|(k, _)| !k.is_empty() && k.len() <= 12)
            .collect();
        self.schema = SCHEMA;
        self
    }

    fn rolltempo_fuer(&self, k: Klasse) -> f32 {
        match k {
            Klasse::Ga => self.rolltempo_ga_kt,
            _ => self.rolltempo_kt,
        }
    }
}

// ---------------------------------------------------------------------------
// Klasse
// ---------------------------------------------------------------------------

/// ICAO-Typen der Business Jets (ICAO Doc 8643, gängige Muster).
const BUSINESS_JETS: &[&str] = &[
    "ASTR", "BE40", "C25A", "C25B", "C25C", "C25M", "C500", "C501", "C510", "C525", "C526", "C550",
    "C551", "C560", "C56X", "C650", "C680", "C68A", "C700", "C750", "CL30", "CL35", "CL60", "E35L",
    "E50P", "E545", "E550", "E55P", "EA50", "F2TH", "F900", "FA10", "FA20", "FA50", "FA7X", "FA8X",
    "G150", "G280", "GA5C", "GA6C", "GALX", "GL5T", "GL7T", "GLEX", "GLF2", "GLF3", "GLF4", "GLF5",
    "GLF6", "H25A", "H25B", "H25C", "HDJT", "LJ23", "LJ24", "LJ25", "LJ31", "LJ35", "LJ40", "LJ45",
    "LJ55", "LJ60", "LJ70", "LJ75", "PC24", "PRM1", "SF50", "WW24",
];

/// ICAO-Typen der Kleinflugzeuge (Kolben, leichte Turboprops) und
/// Hubschrauber — für sie ist das Bordbuch standardmässig aus.
const GA_TYPEN: &[&str] = &[
    "AC11", "AN2", "AS50", "ASK2", "B06", "B407", "BE20", "BE33", "BE35", "BE36", "BE55", "BE58",
    "BE9L", "BE9T", "BL8", "C140", "C150", "C152", "C170", "C172", "C177", "C182", "C206", "C207",
    "C208", "C210", "C310", "C337", "C340", "C414", "C421", "CRUZ", "DA20", "DA40", "DA42", "DA50",
    "DA62", "DHC2", "DHC3", "DHC6", "DR40", "EC35", "EC45", "EV97", "GLID", "H125", "H135", "H145",
    "J3", "KODI", "M20P", "M20T", "P28A", "P28B", "P28R", "P32R", "P46T", "PA18", "PA24", "PA28",
    "PA31", "PA32", "PA34", "PA44", "PA46", "PC12", "PC6T", "PC7", "R22", "R44", "R66", "RV10",
    "RV7", "RV8", "S22T", "S76", "SR20", "SR22", "TBM7", "TBM8", "TBM9", "TOBA", "ULAC",
];

/// Klasse aus Einstellung, Add-on-Profil und ICAO-Typ. Liefert zusätzlich
/// die Quelle (für den Admin: warum diese Klasse).
pub fn klasse_fuer(
    muster: Option<&str>,
    kennzeichen: Option<&str>,
    profil: AircraftProfile,
    einst: &Einstellungen,
) -> (Klasse, &'static str) {
    let norm = |v: Option<&str>| v.map(|s| s.trim().to_uppercase()).filter(|s| !s.is_empty());
    let muster = norm(muster);
    let kennzeichen = norm(kennzeichen);
    if let Some(k) = kennzeichen
        .as_ref()
        .and_then(|k| einst.klassen_override.get(k))
    {
        return (*k, "einstellung_kennzeichen");
    }
    if let Some(k) = muster.as_ref().and_then(|m| einst.klassen_override.get(m)) {
        return (*k, "einstellung_muster");
    }
    match profil {
        AircraftProfile::FsrPhenom300e | AircraftProfile::ContrailFa50 => {
            return (Klasse::BusinessJet, "profil")
        }
        AircraftProfile::Default => {}
        _ => return (Klasse::Airliner, "profil"),
    }
    if let Some(m) = muster.as_deref() {
        if BUSINESS_JETS.contains(&m) {
            return (Klasse::BusinessJet, "icao");
        }
        if GA_TYPEN.contains(&m) {
            return (Klasse::Ga, "icao");
        }
        return (Klasse::Airliner, "icao");
    }
    (Klasse::Airliner, "unbekannt")
}

/// Welche Art hat die Regel in dieser Klasse bei dieser Tageszeit?
/// `None` = passt nicht zur Klasse („–", erscheint gar nicht).
pub fn art(regel: Regel, klasse: Klasse, nacht: bool) -> Option<Art> {
    use Art::*;
    use Klasse::*;
    Some(match (regel, klasse) {
        // 14 CFR 91.209(b), SERA.3215, FAA-H-8083-3C Kap. 2.
        (Regel::BeaconAnlassen, _) => Pflicht,
        // Nachts Pflicht für alle (91.209(a)); tagsüber GA nur Bestätigung.
        (Regel::NavLichter, Ga) if !nacht => Bestaetigung,
        (Regel::NavLichter, _) => Pflicht,
        // C172S-POH „Brakes – RELEASE".
        (Regel::ParkbremseGeloest, _) => Pflicht,
        // Boeing-737-FCTM; GA-Grenze ist eigene Toleranz.
        (Regel::RolltempoAbflug | Regel::RolltempoAnkunft, _) => Pflicht,
        // AIM 4-3-24; C172S-POH „Strobe Lights – AS DESIRED".
        (Regel::StrobesStart, Ga) => Bestaetigung,
        (Regel::StrobesStart, _) => Pflicht,
        (Regel::LandelichtStart | Regel::LandelichtAnflug, Ga) => Bestaetigung,
        (Regel::LandelichtStart | Regel::LandelichtAnflug, _) => Pflicht,
        // 91.215(c), SERA.13010, FSAV §4.
        (Regel::TransponderStart, _) => Pflicht,
        // AIM 4-1-20 „if equipped with TCAS".
        (Regel::TcasStart, Ga) => return None,
        (Regel::TcasStart, _) => Pflicht,
        // Wo das Muster Klappe 0 erlaubt (viele Business Jets), wird nie
        // etwas vermerkt → Bestätigung.
        (Regel::KlappenStart, Airliner) => Pflicht,
        (Regel::KlappenStart, BusinessJet) => Bestaetigung,
        (Regel::KlappenStart, Ga) => return None,
        // 14 CFR 121.317(b); Airbus-SOP „SEAT BELTS – ON/AUTO".
        (Regel::AnschnallStart | Regel::AnschnallLandung, Airliner) => Pflicht,
        (Regel::AnschnallStart | Regel::AnschnallLandung, BusinessJet) => Bestaetigung,
        (Regel::AnschnallStart | Regel::AnschnallLandung, Ga) => return None,
        // Herstellerpraxis mit regulären Ausnahmen (MEL, ETOPS).
        (Regel::ApuReiseflug, Ga) => return None,
        (Regel::ApuReiseflug, _) => Bestaetigung,
        // Boeing-737-FCTM: empfohlen, manuelles Bremsen ist regulär.
        (Regel::AutobrakeLandung, Ga) => return None,
        (Regel::AutobrakeLandung, _) => Bestaetigung,
        // Boeing-737-FCTM „Arm speedbrake".
        (Regel::SpoilerLandung, Airliner) => Pflicht,
        (Regel::SpoilerLandung, BusinessJet) => Bestaetigung,
        (Regel::SpoilerLandung, Ga) => return None,
    })
}

// ---------------------------------------------------------------------------
// Sonnenstand (NOAA-Näherung, genau auf wenige Zehntelgrad)
// ---------------------------------------------------------------------------

/// Sonnenhöhe in Grad über dem Horizont.
pub fn sonnenhoehe(lat: f64, lon: f64, t: DateTime<Utc>) -> f64 {
    let tag = t.ordinal() as f64;
    let stunde = t.hour() as f64 + t.minute() as f64 / 60.0 + t.second() as f64 / 3600.0;
    let jahrtage = if t.year() % 4 == 0 && (t.year() % 100 != 0 || t.year() % 400 == 0) {
        366.0
    } else {
        365.0
    };
    let g = 2.0 * std::f64::consts::PI / jahrtage * (tag - 1.0 + (stunde - 12.0) / 24.0);
    let zeitgl = 229.18
        * (0.000075 + 0.001868 * g.cos()
            - 0.032077 * g.sin()
            - 0.014615 * (2.0 * g).cos()
            - 0.040849 * (2.0 * g).sin());
    let dekl = 0.006918 - 0.399912 * g.cos() + 0.070257 * g.sin() - 0.006758 * (2.0 * g).cos()
        + 0.000907 * (2.0 * g).sin()
        - 0.002697 * (3.0 * g).cos()
        + 0.00148 * (3.0 * g).sin();
    let wahre_ortszeit_min = stunde * 60.0 + zeitgl + 4.0 * lon;
    let stundenwinkel = (wahre_ortszeit_min / 4.0 - 180.0).to_radians();
    let phi = lat.to_radians();
    let cos_zenit = phi.sin() * dekl.sin() + phi.cos() * dekl.cos() * stundenwinkel.cos();
    90.0 - cos_zenit.clamp(-1.0, 1.0).acos().to_degrees()
}

/// Nacht im Sinne der Lichtpflicht. USA (ICAO K…/P…): ab Sonnenuntergang
/// (91.209(a) „sunset to sunrise"). Sonst EU: ab 6° unter dem Horizont
/// (SERA „end of evening civil twilight").
pub fn ist_nacht(sonne_grad: f64, flughafen: Option<&str>) -> bool {
    let usa = flughafen
        .map(|f| {
            let f = f.trim().to_uppercase();
            f.len() == 4 && (f.starts_with('K') || f.starts_with("PA") || f.starts_with("PH"))
        })
        .unwrap_or(false);
    if usa {
        sonne_grad < -0.833
    } else {
        sonne_grad < -6.0
    }
}

// ---------------------------------------------------------------------------
// Punkt, Zustand, Eintrag
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Punkt {
    pub regel: Regel,
    pub schalter: Schalter,
    pub abschnitt: Abschnitt,
    /// `None` = passt nicht zur Klasse (nur für den Admin sichtbar).
    pub art: Option<Art>,
    /// Was gilt — nach einer Pilotenmarkierung „nach ATC" ggf. anders als
    /// `auto_status`.
    pub status: Status,
    /// Messergebnis, bleibt auch nach der Markierung stehen.
    pub auto_status: Status,
    /// Wann der Punkt entschieden wurde.
    pub zeit: Option<DateTime<Utc>>,
    /// Höhe (MSL) im Moment der Entscheidung — für das Flugprofil.
    pub hoehe_ft: Option<i32>,
    /// Was im Cockpit stand, lesbar („TA-RA", „LO", „max 24 kt").
    pub stellung: Option<String>,
    pub grund: Option<Grund>,
    /// Rohwerte im Moment der Entscheidung — für den Admin.
    #[serde(default)]
    pub beleg: BTreeMap<String, serde_json::Value>,
    pub markiert_at: Option<DateTime<Utc>>,
}

impl Punkt {
    fn neu(regel: Regel) -> Self {
        Self {
            regel,
            schalter: regel.schalter(),
            abschnitt: regel.abschnitt(),
            art: None,
            status: Status::Offen,
            auto_status: Status::Offen,
            zeit: None,
            hoehe_ft: None,
            stellung: None,
            grund: None,
            beleg: BTreeMap::new(),
            markiert_at: None,
        }
    }
}

/// Hinweis im Flug („Strobes?"), solange die Kulanz noch läuft.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Hinweis {
    pub regel: Regel,
    pub seit: DateTime<Utc>,
}

/// Kontext pro Takt — was nicht im Snapshot steht.
#[derive(Debug, Clone)]
pub struct Kontext<'a> {
    pub klasse: Klasse,
    pub einstellungen: &'a Einstellungen,
    pub abflug: Option<&'a str>,
    pub ziel: Option<&'a str>,
}

/// Laufzustand während des Flugs — wird mit dem Flug persistiert, damit
/// ein Neustart der App nichts verliert.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Zustand {
    pub schema: u8,
    pub punkte: Vec<Punkt>,
    pub klasse: Option<Klasse>,
    pub klasse_quelle: Option<String>,
    pub nacht_start: Option<bool>,
    pub nacht_landung: Option<bool>,
    /// "sim" oder "rechner" — woher die Uhrzeit für Tag/Nacht kam.
    pub zeitquelle: Option<String>,
    pub hinweis: Option<Hinweis>,
    /// Bereits gezeigte Hinweise (jeder nur einmal je Flug).
    pub hinweise_gezeigt: Vec<Regel>,
    // --- Laufwerte ---
    pub letzte_phase: Option<FlightPhase>,
    pub letzte_zeit: Option<DateTime<Utc>>,
    pub triebwerke_vorher: Option<u8>,
    pub fristen: BTreeMap<Regel, DateTime<Utc>>,
    pub startlauf_ab: Option<DateTime<Utc>>,
    pub rollen_gesehen: bool,
    pub taxi_in_ab: Option<DateTime<Utc>>,
    pub ueber_grenze_seit: Option<DateTime<Utc>>,
    pub rollen_max_abflug_kt: f32,
    pub rollen_max_ankunft_kt: f32,
    pub rollen_laengste_ueber_abflug_s: f64,
    pub rollen_laengste_ueber_ankunft_s: f64,
    pub war_ueber_fl100: bool,
    pub reiseflug_ab: Option<DateTime<Utc>>,
    pub gelandet: bool,
    /// Welche Felder im Flug je einen Wert hatten („nicht messbar"
    /// entscheidet sich daran).
    pub gesehen: BTreeMap<String, bool>,
    pub profil: Vec<(i64, i32)>,
    pub flugzeug: Option<String>,
    pub profil_name: Option<String>,
    pub simulator: Option<String>,
    /// Liefert die Quelle TCAS-Stellungen? Aus dem echten Snapshot gemerkt,
    /// damit auch ein beim Flugende entschiedener Punkt es weiss.
    pub tcas_meldbar: Option<bool>,
    /// VFR-Flug (Flugstart ohne SimBrief). Einmal festgehalten, weil die
    /// Flugplanquelle einen App-Neustart nicht überlebt.
    pub vfr: Option<bool>,
}

/// Abgeschlossenes Bordbuch eines Flugs — lokal gespeichert, zum Server
/// gesichert, auf live.kant.ovh beim PIREP zu sehen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Eintrag {
    pub schema: u8,
    pub pirep_id: String,
    pub erstellt_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub client_version: String,
    pub flug: FlugInfo,
    pub klasse: Klasse,
    pub klasse_quelle: String,
    /// "ifr" | "vfr" — nur angezeigt, ändert keine Regel.
    pub regelwerk: String,
    pub nacht_start: Option<bool>,
    pub nacht_landung: Option<bool>,
    pub zeitquelle: Option<String>,
    /// Beim Flugende eingeschaltete Schalter — der Pilot sah genau diese.
    pub eingeschaltet: Vec<Schalter>,
    /// Bordbuch für diesen Flug aus (GA oder VFR nicht eingeschaltet).
    pub aus_grund: Option<String>,
    pub punkte: Vec<Punkt>,
    pub rollen_max_abflug_kt: f32,
    pub rollen_max_ankunft_kt: f32,
    pub rolltempo_grenze_kt: f32,
    pub profil: Vec<(i64, i32)>,
    /// Nur lokal: vom Server bestätigt.
    #[serde(default)]
    pub synced: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct FlugInfo {
    pub callsign: Option<String>,
    pub dep: Option<String>,
    pub arr: Option<String>,
    pub muster: Option<String>,
    pub titel: Option<String>,
    pub profil: Option<String>,
    pub sim: Option<String>,
}

impl Eintrag {
    /// Pflichtpunkte, die zählen: eingeschaltet, messbar, im Flug vorgekommen.
    pub fn zaehlbare(&self) -> impl Iterator<Item = &Punkt> {
        self.punkte.iter().filter(move |p| {
            p.art == Some(Art::Pflicht)
                && self.eingeschaltet.contains(&p.schalter)
                && !matches!(
                    p.status,
                    Status::NichtMessbar | Status::NichtAnwendbar | Status::Offen
                )
        })
    }

    /// (erledigt, von) — „11 von 13".
    pub fn bilanz(&self) -> (usize, usize) {
        let von = self.zaehlbare().count();
        let ok = self
            .zaehlbare()
            .filter(|p| p.status.zaehlt_als_erledigt())
            .count();
        (ok, von)
    }

    /// Pilot tippt einen offenen Punkt an: „nach ATC-Anweisung" oder zurück.
    /// Nur „diesmal ohne" lässt sich markieren; nicht messbare Punkte nicht.
    pub fn markieren(&mut self, regel: Regel, nach_atc: bool, jetzt: DateTime<Utc>) -> bool {
        let Some(p) = self.punkte.iter_mut().find(|p| p.regel == regel) else {
            return false;
        };
        if p.auto_status != Status::DiesmalOhne {
            return false;
        }
        p.status = if nach_atc {
            Status::NachAtc
        } else {
            Status::DiesmalOhne
        };
        p.markiert_at = nach_atc.then_some(jetzt);
        self.updated_at = jetzt;
        self.synced = false;
        true
    }
}

// ---------------------------------------------------------------------------
// Auswertung pro Takt
// ---------------------------------------------------------------------------

fn phase_am_boden_rollen(p: FlightPhase) -> bool {
    matches!(p, FlightPhase::TaxiOut | FlightPhase::TaxiIn)
}

fn phase_anflug(p: FlightPhase) -> bool {
    matches!(
        p,
        FlightPhase::Descent | FlightPhase::Approach | FlightPhase::Final | FlightPhase::Holding
    )
}

fn phase_nach_start(p: FlightPhase) -> bool {
    !matches!(
        p,
        FlightPhase::Preflight
            | FlightPhase::Boarding
            | FlightPhase::Pushback
            | FlightPhase::TaxiOut
    )
}

/// Transponder mit Höhenübermittlung. „XPNDR" heisst bei PMDG/Airbus/iFly/
/// Zibo „sendet mit Höhe"; die Standardwerte ohne Höhe heissen „ON".
fn xpdr_mit_hoehe(label: &str) -> bool {
    matches!(label, "ALT" | "XPNDR" | "TA" | "TA-RA")
}

fn xpdr_tcas(label: &str) -> bool {
    matches!(label, "TA" | "TA-RA")
}

/// Liefert die Quelle dieses Musters TCAS-Stellungen (TA, TA-RA)?
fn tcas_meldbar(s: &SimSnapshot) -> bool {
    if matches!(s.simulator, Simulator::XPlane11 | Simulator::XPlane12) {
        return true; // XP12-Enum kennt ta_only/ta_ra, Zibo den Drehschalter
    }
    matches!(
        s.aircraft_profile,
        AircraftProfile::Pmdg737
            | AircraftProfile::Pmdg777
            | AircraftProfile::FenixA319
            | AircraftProfile::FenixA320
            | AircraftProfile::FenixA321
            | AircraftProfile::AerosoftA346
            | AircraftProfile::TfdiMd11
            | AircraftProfile::IniA330
            | AircraftProfile::IniA350
            | AircraftProfile::FsLabsA321
            | AircraftProfile::IflyMax8
    )
}

fn autobrake_gesetzt(label: &str) -> bool {
    let l = label.trim().to_uppercase();
    !(l.is_empty()
        || l.starts_with('#')
        || matches!(l.as_str(), "OFF" | "DISARM" | "DISARMED" | "RTO" | "?"))
}

fn strobe_an(s: &SimSnapshot) -> Option<bool> {
    match (s.strobe_state, s.light_strobe) {
        (Some(st), _) => Some(st >= 1), // AUTO zählt als erfüllt
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn b(v: bool) -> serde_json::Value {
    serde_json::Value::Bool(v)
}

fn opt<T: Serialize>(v: &Option<T>) -> serde_json::Value {
    serde_json::to_value(v).unwrap_or(serde_json::Value::Null)
}

/// Rohwerte, die zu einer Regel gehören — für den Admin.
fn beleg_fuer(regel: Regel, s: &SimSnapshot) -> BTreeMap<String, serde_json::Value> {
    let mut m = BTreeMap::new();
    let mut put = |k: &str, v: serde_json::Value| {
        m.insert(k.to_string(), v);
    };
    put(
        "phase_gs_kt",
        serde_json::json!((s.groundspeed_kt * 10.0).round() / 10.0),
    );
    match regel {
        Regel::BeaconAnlassen => {
            put("light_beacon", opt(&s.light_beacon));
            put("engines_running", serde_json::json!(s.engines_running));
            put("pushback_state", opt(&s.pushback_state));
        }
        Regel::NavLichter => put("light_nav", opt(&s.light_nav)),
        Regel::ParkbremseGeloest => put("parking_brake", b(s.parking_brake)),
        Regel::StrobesStart => {
            put("strobe_state", opt(&s.strobe_state));
            put("light_strobe", opt(&s.light_strobe));
        }
        Regel::LandelichtStart | Regel::LandelichtAnflug => {
            put("light_landing", opt(&s.light_landing));
            put("light_taxi", opt(&s.light_taxi));
            put(
                "altitude_msl_ft",
                serde_json::json!(s.altitude_msl_ft.round()),
            );
        }
        Regel::TransponderStart | Regel::TcasStart => {
            put("xpdr_mode_label", opt(&s.xpdr_mode_label));
            put("transponder_code", opt(&s.transponder_code));
        }
        Regel::KlappenStart => {
            put(
                "flaps_position",
                serde_json::json!((s.flaps_position * 1000.0).round() / 1000.0),
            );
            put("flap_handle_index", opt(&s.flap_handle_index));
        }
        Regel::AnschnallStart | Regel::AnschnallLandung => {
            put("seatbelts_sign", opt(&s.seatbelts_sign))
        }
        Regel::ApuReiseflug => {
            put("apu_switch", opt(&s.apu_switch));
            put("apu_pct_rpm", opt(&s.apu_pct_rpm));
        }
        Regel::AutobrakeLandung => put("autobrake", opt(&s.autobrake)),
        Regel::SpoilerLandung => {
            put("spoilers_armed", opt(&s.spoilers_armed));
            put("spoilers_handle_position", opt(&s.spoilers_handle_position));
        }
        Regel::RolltempoAbflug | Regel::RolltempoAnkunft => {}
    }
    put(
        "altitude_agl_ft",
        serde_json::json!(s.altitude_agl_ft.round()),
    );
    m
}

impl Zustand {
    fn sicherstellen(&mut self) {
        if self.punkte.len() != Regel::ALLE.len() {
            let alt = std::mem::take(&mut self.punkte);
            self.punkte = Regel::ALLE
                .iter()
                .map(|r| {
                    alt.iter()
                        .find(|p| p.regel == *r)
                        .cloned()
                        .unwrap_or_else(|| Punkt::neu(*r))
                })
                .collect();
        }
        self.schema = SCHEMA;
    }

    fn punkt(&mut self, r: Regel) -> &mut Punkt {
        self.sicherstellen();
        &mut self.punkte[r.index()]
    }

    fn status(&self, r: Regel) -> Status {
        self.punkte
            .get(r.index())
            .map(|p| p.status)
            .unwrap_or(Status::Offen)
    }

    fn offen(&self, r: Regel) -> bool {
        self.status(r) == Status::Offen
    }

    fn entscheiden(
        &mut self,
        r: Regel,
        status: Status,
        s: &SimSnapshot,
        stellung: Option<String>,
        grund: Option<Grund>,
    ) {
        let p = self.punkt(r);
        if p.status != Status::Offen {
            return;
        }
        p.status = status;
        p.auto_status = status;
        p.zeit = Some(s.timestamp);
        p.hoehe_ft = Some(s.altitude_msl_ft.round() as i32);
        p.stellung = stellung;
        p.grund = grund;
        p.beleg = beleg_fuer(r, s);
        self.fristen.remove(&r);
        if self.hinweis.as_ref().map(|h| h.regel) == Some(r) {
            self.hinweis = None;
        }
    }

    fn gesehen(&self, feld: &str) -> bool {
        self.gesehen.get(feld).copied().unwrap_or(false)
    }

    fn merken(&mut self, s: &SimSnapshot) {
        let felder: [(&str, bool); 10] = [
            ("beacon", s.light_beacon.is_some()),
            ("nav", s.light_nav.is_some()),
            (
                "strobe",
                s.strobe_state.is_some() || s.light_strobe.is_some(),
            ),
            ("landing", s.light_landing.is_some()),
            ("xpdr", s.xpdr_mode_label.is_some()),
            ("autobrake", s.autobrake.is_some()),
            ("spoilers", s.spoilers_armed.is_some()),
            ("seatbelts", s.seatbelts_sign.is_some()),
            ("apu", s.apu_switch.is_some()),
            ("taxi_light", s.light_taxi.is_some()),
        ];
        for (k, da) in felder {
            if da {
                self.gesehen.insert(k.to_string(), true);
            }
        }
    }

    /// Frist starten (einmal) und ggf. einen Hinweis aufsetzen.
    fn frist(&mut self, r: Regel, bis: DateTime<Utc>, jetzt: DateTime<Utc>, hinweis: bool) {
        if !self.offen(r) || self.fristen.contains_key(&r) {
            return;
        }
        self.fristen.insert(r, bis);
        if hinweis && !self.hinweise_gezeigt.contains(&r) {
            self.hinweise_gezeigt.push(r);
            self.hinweis = Some(Hinweis {
                regel: r,
                seit: jetzt,
            });
        }
    }

    fn frist_abgelaufen(&self, r: Regel, jetzt: DateTime<Utc>) -> bool {
        self.fristen
            .get(&r)
            .map(|bis| jetzt > *bis)
            .unwrap_or(false)
    }

    fn frist_laeuft(&self, r: Regel) -> bool {
        self.fristen.contains_key(&r)
    }

    /// Nicht erledigt: „diesmal ohne", wenn das Flugzeug den Wert je
    /// meldete — sonst „nicht messbar".
    fn ohne_oder_nicht_messbar(&mut self, r: Regel, s: &SimSnapshot) {
        if r == Regel::TcasStart && self.gesehen("xpdr") && self.tcas_meldbar == Some(false) {
            self.entscheiden(r, Status::NichtMessbar, s, None, Some(Grund::KeinTcasModus));
            return;
        }
        let feld = feld_der_regel(r);
        if feld.is_none() || feld.map(|f| self.gesehen(f)).unwrap_or(false) {
            let st = match r {
                Regel::AutobrakeLandung => s.autobrake.clone(),
                _ => None,
            };
            self.entscheiden(r, Status::DiesmalOhne, s, st, None);
        } else {
            self.entscheiden(r, Status::NichtMessbar, s, None, Some(Grund::WertFehlt));
        }
    }
}

/// Welches Snapshot-Feld eine Regel braucht (`None` = meldet jedes Flugzeug).
fn feld_der_regel(r: Regel) -> Option<&'static str> {
    Some(match r {
        Regel::BeaconAnlassen => "beacon",
        Regel::NavLichter => "nav",
        Regel::StrobesStart => "strobe",
        Regel::LandelichtStart | Regel::LandelichtAnflug => "landing",
        Regel::TransponderStart | Regel::TcasStart => "xpdr",
        Regel::AnschnallStart | Regel::AnschnallLandung => "seatbelts",
        Regel::ApuReiseflug => "apu",
        Regel::AutobrakeLandung => "autobrake",
        Regel::SpoilerLandung => "spoilers",
        Regel::ParkbremseGeloest
        | Regel::RolltempoAbflug
        | Regel::RolltempoAnkunft
        | Regel::KlappenStart => return None,
    })
}

/// Soll für diese Regel ein Hinweis erscheinen? Nur Pflichtpunkte, nur
/// eingeschaltet, nur wenn der Pilot Hinweise will.
fn hinweis_erlaubt(r: Regel, k: &Kontext, nacht: bool) -> bool {
    k.einstellungen.hinweise_im_flug
        && k.einstellungen.an(r.schalter())
        && art(r, k.klasse, nacht) == Some(Art::Pflicht)
}

/// Eine Prüfung mit Frist: `wert` = aktueller Messwert (`None` = Feld fehlt).
/// Erledigt, sobald `true` innerhalb der Frist; danach „diesmal ohne" bzw.
/// „nicht messbar", wenn das Feld im ganzen Flug nie einen Wert hatte.
fn pruefen_mit_frist(
    z: &mut Zustand,
    r: Regel,
    s: &SimSnapshot,
    wert: Option<bool>,
    feld: &str,
    stellung: Option<String>,
) {
    if !z.offen(r) || !z.frist_laeuft(r) {
        return;
    }
    if wert == Some(true) {
        z.entscheiden(r, Status::Erledigt, s, stellung, None);
    } else if z.frist_abgelaufen(r, s.timestamp) {
        if z.gesehen(feld) {
            z.entscheiden(r, Status::DiesmalOhne, s, stellung, None);
        } else {
            z.entscheiden(r, Status::NichtMessbar, s, None, Some(Grund::WertFehlt));
        }
    }
}

/// Ein Takt. Rein, ohne I/O — der Aufrufer hält den Flug-Lock.
pub fn tick(z: &mut Zustand, s: &SimSnapshot, phase: FlightPhase, k: &Kontext) {
    z.sicherstellen();
    let jetzt = s.timestamp;
    // Lücke seit dem letzten Takt (Pause, Slew, App-Neustart): laufende
    // Fristen um die Lücke verschieben, damit der Pilot nach dem Fortsetzen
    // seine volle Kulanz behält (QS 26.09.2026). Normale Takte sind ≤ 10 s.
    if let Some(vorher_t) = z.letzte_zeit {
        let luecke = jetzt - vorher_t;
        if luecke > chrono::Duration::seconds(PAUSE_AB_S) {
            for bis in z.fristen.values_mut() {
                *bis += luecke;
            }
            z.ueber_grenze_seit = None;
        }
    }
    if z.klasse.is_none() {
        z.klasse = Some(k.klasse);
    }
    let klasse = z.klasse.unwrap_or(k.klasse);
    z.merken(s);
    if s.xpdr_mode_label.is_some() {
        z.tcas_meldbar = Some(tcas_meldbar(s));
    }
    if z.flugzeug.is_none() {
        z.flugzeug = s.aircraft_title.clone().filter(|t| !t.trim().is_empty());
        z.profil_name = Some(format!("{:?}", s.aircraft_profile));
        z.simulator = Some(format!("{:?}", s.simulator));
    }
    let zeit_sonne = s.sim_zeit_utc.unwrap_or(jetzt);
    z.zeitquelle.get_or_insert_with(|| {
        if s.sim_zeit_utc.is_some() {
            "sim"
        } else {
            "rechner"
        }
        .to_string()
    });
    let nacht_jetzt = ist_nacht(sonnenhoehe(s.lat, s.lon, zeit_sonne), k.abflug);
    if !phase_nach_start(phase) {
        // Bis zum Startlauf gilt die Tageszeit am Abflugort.
        z.nacht_start = Some(nacht_jetzt);
    }
    let nacht_start = z.nacht_start.unwrap_or(false);
    // Am Ziel gilt dessen Regel (USA: Sonnenuntergang, sonst −6°).
    let nacht_ziel = ist_nacht(sonnenhoehe(s.lat, s.lon, zeit_sonne), k.ziel.or(k.abflug));
    for p in z.punkte.iter_mut() {
        if p.status == Status::Offen {
            let n = match p.abschnitt {
                Abschnitt::Anflug | Abschnitt::NachDerLandung => nacht_ziel,
                _ => nacht_start,
            };
            p.art = art(p.regel, klasse, n);
        }
    }

    // Höhenprofil (für Variante B).
    if !s.on_ground || z.profil.is_empty() {
        let t = jetzt.timestamp();
        if z.profil
            .last()
            .map(|(lt, _)| t - lt >= PROFIL_TAKT_S)
            .unwrap_or(true)
            && z.profil.len() < PROFIL_MAX
        {
            z.profil.push((t, s.altitude_msl_ft.round() as i32));
        }
    }

    // Hinweis nach HINWEIS_SICHTBAR_S ausblenden.
    if let Some(h) = &z.hinweis {
        if (jetzt - h.seit).num_seconds() >= HINWEIS_SICHTBAR_S {
            z.hinweis = None;
        }
    }

    let vorher = z.letzte_phase;
    let neu_in = |p: FlightPhase| vorher != Some(p) && phase == p;

    // --- Beacon: spätestens wenn das erste Triebwerk läuft (oder beim
    // Pushback). Flug mit schon laufenden Triebwerken begonnen → sofort.
    let tw_vorher = z.triebwerke_vorher.unwrap_or(0);
    let anlassen = (s.engines_running > 0 && tw_vorher == 0)
        || (phase == FlightPhase::Pushback && s.pushback_state.map(|p| p < 3).unwrap_or(false));
    if anlassen && !phase_nach_start(phase) {
        let h = hinweis_erlaubt(Regel::BeaconAnlassen, k, nacht_start);
        z.frist(
            Regel::BeaconAnlassen,
            jetzt + chrono::Duration::seconds(FRIST_BEACON_S),
            jetzt,
            h && s.light_beacon == Some(false),
        );
    }
    z.triebwerke_vorher = Some(s.engines_running);
    let beacon = s.light_beacon;
    pruefen_mit_frist(z, Regel::BeaconAnlassen, s, beacon, "beacon", None);

    // --- Rollen beginnt: Nav-Lichter, Parkbremse.
    let rollt = phase == FlightPhase::TaxiOut && s.on_ground && s.groundspeed_kt > 5.0;
    if rollt && !z.rollen_gesehen {
        z.rollen_gesehen = true;
        let bis = jetzt + chrono::Duration::seconds(FRIST_ROLLEN_S);
        let h = hinweis_erlaubt(Regel::NavLichter, k, nacht_start);
        z.frist(
            Regel::NavLichter,
            bis,
            jetzt,
            h && s.light_nav == Some(false),
        );
        z.frist(Regel::ParkbremseGeloest, bis, jetzt, false);
    }
    pruefen_mit_frist(z, Regel::NavLichter, s, s.light_nav, "nav", None);
    if z.offen(Regel::ParkbremseGeloest) && z.frist_laeuft(Regel::ParkbremseGeloest) {
        // Die Parkbremse meldet jedes Flugzeug — nie „nicht messbar".
        if !s.parking_brake {
            z.entscheiden(Regel::ParkbremseGeloest, Status::Erledigt, s, None, None);
        } else if z.frist_abgelaufen(Regel::ParkbremseGeloest, jetzt) {
            z.entscheiden(Regel::ParkbremseGeloest, Status::DiesmalOhne, s, None, None);
        }
    }

    // --- Rolltempo (Abflug und Ankunft), nie auf Bahn oder Abrollweg.
    let grenze = k.einstellungen.rolltempo_fuer(klasse);
    if neu_in(FlightPhase::TaxiIn) {
        z.taxi_in_ab = Some(jetzt);
    }
    let im_abrollweg = phase == FlightPhase::TaxiIn
        && z.taxi_in_ab
            .map(|t0| (jetzt - t0).num_seconds() < ABROLLWEG_S)
            .unwrap_or(true);
    if phase_am_boden_rollen(phase) && s.on_ground && !im_abrollweg {
        let gs = s.groundspeed_kt;
        let ankunft = phase == FlightPhase::TaxiIn;
        if ankunft {
            z.rollen_max_ankunft_kt = z.rollen_max_ankunft_kt.max(gs);
        } else {
            z.rollen_max_abflug_kt = z.rollen_max_abflug_kt.max(gs);
        }
        if gs > grenze {
            let seit = *z.ueber_grenze_seit.get_or_insert(jetzt);
            let dauer = (jetzt - seit).num_milliseconds() as f64 / 1000.0;
            if ankunft {
                z.rollen_laengste_ueber_ankunft_s = z.rollen_laengste_ueber_ankunft_s.max(dauer);
            } else {
                z.rollen_laengste_ueber_abflug_s = z.rollen_laengste_ueber_abflug_s.max(dauer);
            }
        } else {
            z.ueber_grenze_seit = None;
        }
    } else {
        z.ueber_grenze_seit = None;
    }

    // --- Startlauf: Strobes, Landelicht, Transponder, TCAS, Klappen,
    // Anschnallzeichen; Rolltempo Abflug abschliessen.
    if neu_in(FlightPhase::TakeoffRoll) && z.startlauf_ab.is_none() {
        z.startlauf_ab = Some(jetzt);
        z.ueber_grenze_seit = None;
        let bis = jetzt + chrono::Duration::seconds(FRIST_START_S);
        // Rolltempo Abflug: entschieden mit dem, was beim Rollen war.
        if z.offen(Regel::RolltempoAbflug) {
            let status = if !z.rollen_gesehen {
                Status::NichtAnwendbar
            } else if z.rollen_laengste_ueber_abflug_s > ROLLEN_UEBER_S {
                Status::DiesmalOhne
            } else {
                Status::Erledigt
            };
            let max = z.rollen_max_abflug_kt;
            let st = z.rollen_gesehen.then(|| format!("max {max:.0} kt"));
            z.entscheiden(Regel::RolltempoAbflug, status, s, st, None);
            z.punkt(Regel::RolltempoAbflug)
                .beleg
                .insert("max_kt".into(), serde_json::json!(p_round(max)));
        }
        for (r, frage) in [
            (Regel::StrobesStart, strobe_an(s) == Some(false)),
            (
                Regel::LandelichtStart,
                s.light_landing == Some(false)
                    && !(klasse == Klasse::Ga && s.light_taxi == Some(true)),
            ),
            (
                Regel::TransponderStart,
                s.xpdr_mode_label
                    .as_deref()
                    .map(|l| !xpdr_mit_hoehe(l))
                    .unwrap_or(false),
            ),
            (Regel::TcasStart, false),
            (Regel::KlappenStart, false),
            (Regel::AnschnallStart, false),
        ] {
            let h = hinweis_erlaubt(r, k, nacht_start) && frage;
            z.frist(r, bis, jetzt, h);
        }
    }
    pruefen_mit_frist(z, Regel::StrobesStart, s, strobe_an(s), "strobe", None);
    let landelicht_start = match (s.light_landing, klasse) {
        (Some(true), _) => Some(true),
        (_, Klasse::Ga) if s.light_taxi == Some(true) => Some(true),
        (v, _) => v,
    };
    let feld_ll = if klasse == Klasse::Ga && !z.gesehen("landing") {
        "taxi_light"
    } else {
        "landing"
    };
    pruefen_mit_frist(
        z,
        Regel::LandelichtStart,
        s,
        landelicht_start,
        feld_ll,
        None,
    );
    let label = s.xpdr_mode_label.clone();
    pruefen_mit_frist(
        z,
        Regel::TransponderStart,
        s,
        label.as_deref().map(xpdr_mit_hoehe),
        "xpdr",
        label.clone(),
    );
    if z.offen(Regel::TcasStart) && z.frist_laeuft(Regel::TcasStart) {
        let tcas = label.as_deref().map(xpdr_tcas);
        if tcas == Some(true) {
            z.entscheiden(Regel::TcasStart, Status::Erledigt, s, label.clone(), None);
        } else if z.frist_abgelaufen(Regel::TcasStart, jetzt) {
            if !z.gesehen("xpdr") {
                z.entscheiden(
                    Regel::TcasStart,
                    Status::NichtMessbar,
                    s,
                    None,
                    Some(Grund::WertFehlt),
                );
            } else if !tcas_meldbar(s) {
                z.entscheiden(
                    Regel::TcasStart,
                    Status::NichtMessbar,
                    s,
                    label.clone(),
                    Some(Grund::KeinTcasModus),
                );
            } else {
                z.entscheiden(
                    Regel::TcasStart,
                    Status::DiesmalOhne,
                    s,
                    label.clone(),
                    None,
                );
            }
        }
    }
    if z.offen(Regel::KlappenStart) && z.frist_laeuft(Regel::KlappenStart) {
        // Klappen meldet jedes Flugzeug (flaps_position ist immer da).
        let stellung = s
            .flap_handle_index
            .map(|i| i.to_string())
            .or_else(|| Some(format!("{:.0} %", s.flaps_position * 100.0)));
        if s.flaps_position > 0.01 {
            z.entscheiden(Regel::KlappenStart, Status::Erledigt, s, stellung, None);
        } else if z.frist_abgelaufen(Regel::KlappenStart, jetzt) {
            z.entscheiden(Regel::KlappenStart, Status::DiesmalOhne, s, stellung, None);
        }
    }
    let gurte = s.seatbelts_sign.map(|v| v >= 1);
    let gurte_st = s.seatbelts_sign.map(|v| match v {
        0 => "OFF".to_string(),
        1 => "AUTO".to_string(),
        _ => "ON".to_string(),
    });
    pruefen_mit_frist(
        z,
        Regel::AnschnallStart,
        s,
        gurte,
        "seatbelts",
        gurte_st.clone(),
    );

    // --- Reiseflug: APU aus (Bestätigung).
    if phase == FlightPhase::Cruise {
        z.reiseflug_ab.get_or_insert(jetzt);
    }
    let apu_faellig = z
        .reiseflug_ab
        .map(|t0| (jetzt - t0).num_seconds() >= APU_NACH_REISEFLUG_S)
        .unwrap_or(false)
        || (phase_anflug(phase) && z.startlauf_ab.is_some());
    if apu_faellig && z.offen(Regel::ApuReiseflug) && !s.on_ground {
        match s.apu_switch {
            Some(false) => z.entscheiden(Regel::ApuReiseflug, Status::Erledigt, s, None, None),
            Some(true) => z.entscheiden(Regel::ApuReiseflug, Status::DiesmalOhne, s, None, None),
            None if !z.gesehen("apu") => z.entscheiden(
                Regel::ApuReiseflug,
                Status::NichtMessbar,
                s,
                None,
                Some(Grund::WertFehlt),
            ),
            None => {}
        }
    }

    // --- Anflug: Landelicht (unter FL100 bzw. im Endanflug).
    if !s.on_ground && s.altitude_msl_ft > FL100_UEBER_FT {
        z.war_ueber_fl100 = true;
    }
    let nacht_hier = nacht_ziel;
    if !s.on_ground && z.startlauf_ab.is_some() {
        let unter_fl100 = z.war_ueber_fl100
            && s.altitude_msl_ft < FL100_FT
            && (phase_anflug(phase) || s.vertical_speed_fpm < -300.0);
        let tief_im_anflug = !z.war_ueber_fl100
            && s.altitude_agl_ft < ANFLUG_AGL_FT
            && matches!(phase, FlightPhase::Approach | FlightPhase::Final);
        if unter_fl100 || tief_im_anflug {
            let frist = if unter_fl100 {
                FRIST_FL100_S
            } else {
                FRIST_START_S
            };
            let h = hinweis_erlaubt(Regel::LandelichtAnflug, k, nacht_hier)
                && s.light_landing == Some(false);
            z.frist(
                Regel::LandelichtAnflug,
                jetzt + chrono::Duration::seconds(frist),
                jetzt,
                h,
            );
        }
    }
    let landelicht_anflug = match (s.light_landing, klasse) {
        (Some(true), _) => Some(true),
        (_, Klasse::Ga) if s.light_taxi == Some(true) => Some(true),
        (v, _) => v,
    };
    pruefen_mit_frist(
        z,
        Regel::LandelichtAnflug,
        s,
        landelicht_anflug,
        feld_ll,
        None,
    );

    // --- Endanflug unter 1000 ft AGL bis zum Aufsetzen: Spoiler,
    // Autobrake, Anschnallzeichen. Erledigt, sobald einmal gesehen.
    let im_endanflug = !s.on_ground
        && z.startlauf_ab.is_some()
        && s.altitude_agl_ft < ANFLUG_AGL_FT
        && matches!(
            phase,
            FlightPhase::Approach | FlightPhase::Final | FlightPhase::Landing
        );
    if im_endanflug {
        z.nacht_landung = Some(nacht_hier);
        for r in [
            Regel::SpoilerLandung,
            Regel::AutobrakeLandung,
            Regel::AnschnallLandung,
        ] {
            if z.offen(r) && !z.frist_laeuft(r) {
                // Frist bis zum Aufsetzen — hier nur als „läuft" markiert.
                let h = hinweis_erlaubt(r, k, nacht_hier)
                    && r == Regel::SpoilerLandung
                    && s.spoilers_armed == Some(false);
                z.frist(r, jetzt + chrono::Duration::days(1), jetzt, h);
            }
        }
        if s.spoilers_armed == Some(true) {
            z.entscheiden(
                Regel::SpoilerLandung,
                Status::Erledigt,
                s,
                Some("ARMED".into()),
                None,
            );
        }
        if let Some(ab) = s.autobrake.as_deref().filter(|l| autobrake_gesetzt(l)) {
            z.entscheiden(
                Regel::AutobrakeLandung,
                Status::Erledigt,
                s,
                Some(ab.to_string()),
                None,
            );
        }
        if gurte == Some(true) {
            z.entscheiden(Regel::AnschnallLandung, Status::Erledigt, s, gurte_st, None);
        }
    }
    // Ausgerollt: was in keinem Endanflug kam, ist entschieden. Bewusst
    // NICHT beim ersten Bodenkontakt — ein Touch-and-go, ein Aufsetzer mit
    // Durchstart oder ein Hüpfer würde sonst die Punkte einfrieren, bevor
    // die eigentliche Landung (mit gesetzten Spoilern) kommt (QS 26.09.2026).
    let ausgerollt = s.on_ground
        && z.startlauf_ab.is_some()
        && matches!(
            phase,
            FlightPhase::TaxiIn | FlightPhase::BlocksOn | FlightPhase::Arrived
        );
    if ausgerollt {
        z.gelandet = true;
        for r in [
            Regel::SpoilerLandung,
            Regel::AutobrakeLandung,
            Regel::AnschnallLandung,
        ] {
            if z.offen(r) && z.frist_laeuft(r) {
                z.ohne_oder_nicht_messbar(r, s);
            }
        }
    }

    z.letzte_phase = Some(phase);
    z.letzte_zeit = Some(jetzt);
}

fn p_round(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

/// Flugende: offene Punkte entscheiden, Einstellungen festhalten.
#[allow(clippy::too_many_arguments)]
pub fn abschliessen(
    z: &Zustand,
    pirep_id: &str,
    flug: FlugInfo,
    regelwerk_vfr: bool,
    einst: &Einstellungen,
    klasse_fallback: (Klasse, &'static str),
    client_version: &str,
    jetzt: DateTime<Utc>,
) -> Eintrag {
    let mut z = z.clone();
    z.sicherstellen();
    let klasse = z.klasse.unwrap_or(klasse_fallback.0);
    let klasse_quelle = z
        .klasse_quelle
        .clone()
        .unwrap_or_else(|| klasse_fallback.1.to_string());
    // Rolltempo Ankunft: nur wenn wirklich gerollt wurde.
    {
        let rollen_ankunft = z.rollen_max_ankunft_kt > 5.0;
        let status = if !rollen_ankunft {
            Status::NichtAnwendbar
        } else if z.rollen_laengste_ueber_ankunft_s > ROLLEN_UEBER_S {
            Status::DiesmalOhne
        } else {
            Status::Erledigt
        };
        let max = z.rollen_max_ankunft_kt;
        let laengste = z.rollen_laengste_ueber_ankunft_s;
        let p = z.punkt(Regel::RolltempoAnkunft);
        if p.status == Status::Offen {
            p.status = status;
            p.auto_status = status;
            p.zeit = Some(jetzt);
            p.stellung = rollen_ankunft.then(|| format!("max {max:.0} kt"));
            p.beleg
                .insert("max_kt".into(), serde_json::json!(p_round(max)));
            p.beleg.insert(
                "laengste_ueber_grenze_s".into(),
                serde_json::json!((laengste * 10.0).round() / 10.0),
            );
        }
    }
    {
        let laengste = z.rollen_laengste_ueber_abflug_s;
        let p = z.punkt(Regel::RolltempoAbflug);
        p.beleg.insert(
            "laengste_ueber_grenze_s".into(),
            serde_json::json!((laengste * 10.0).round() / 10.0),
        );
    }
    // Punkte mit laufender Frist (z. B. Flug endete vor dem Rollen zum
    // Stand) sind vorgekommen — entscheiden statt „kam nicht vor".
    let laufend: Vec<Regel> = z.fristen.keys().copied().collect();
    if let Some(letzte) = z.letzte_zeit {
        let mut stand = SimSnapshot::default();
        stand.timestamp = letzte;
        for r in laufend {
            if z.offen(r) {
                z.ohne_oder_nicht_messbar(r, &stand);
                // Kein echter Snapshot zur Hand — ehrlich vermerken statt
                // Nullwerte als Rohwerte zu zeigen (Admin-Ansicht).
                let p = z.punkt(r);
                p.beleg.clear();
                p.beleg.insert(
                    "entschieden".into(),
                    serde_json::json!("beim Flugende, kein Messwert in diesem Moment"),
                );
            }
        }
    }
    let nacht = z.nacht_start.unwrap_or(false);
    for p in z.punkte.iter_mut() {
        if p.status == Status::Offen {
            // Nie ausgelöst (Flug endete vorher, kein Reiseflug über FL100 …).
            p.status = Status::NichtAnwendbar;
            p.auto_status = Status::NichtAnwendbar;
        }
        let nacht_hier = match p.abschnitt {
            Abschnitt::Anflug | Abschnitt::NachDerLandung => z.nacht_landung.unwrap_or(nacht),
            _ => nacht,
        };
        p.art = art(p.regel, klasse, nacht_hier);
        if p.art.is_none() && p.status != Status::NichtMessbar {
            p.status = Status::NichtAnwendbar;
        }
    }
    let aus_grund = if klasse == Klasse::Ga && !einst.ga_an {
        Some("ga".to_string())
    } else if regelwerk_vfr && !einst.vfr_an {
        Some("vfr".to_string())
    } else {
        None
    };
    Eintrag {
        schema: SCHEMA,
        pirep_id: pirep_id.to_string(),
        erstellt_at: jetzt,
        updated_at: jetzt,
        client_version: client_version.to_string(),
        flug,
        klasse,
        klasse_quelle,
        regelwerk: if regelwerk_vfr { "vfr" } else { "ifr" }.to_string(),
        nacht_start: z.nacht_start,
        nacht_landung: z.nacht_landung,
        zeitquelle: z.zeitquelle.clone(),
        eingeschaltet: Schalter::ALLE
            .iter()
            .copied()
            .filter(|s| einst.an(*s))
            .collect(),
        aus_grund,
        punkte: z.punkte.clone(),
        rollen_max_abflug_kt: p_round(z.rollen_max_abflug_kt),
        rollen_max_ankunft_kt: p_round(z.rollen_max_ankunft_kt),
        rolltempo_grenze_kt: einst.rolltempo_fuer(klasse),
        profil: z.profil.clone(),
        synced: false,
    }
}

/// Für die Anzeige im laufenden Flug: derselbe Aufbau wie ein Eintrag,
/// offene Punkte bleiben offen.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LiveAnsicht {
    pub klasse: Option<Klasse>,
    pub nacht_start: Option<bool>,
    pub eingeschaltet: Vec<Schalter>,
    pub aus_grund: Option<String>,
    pub punkte: Vec<Punkt>,
    pub hinweis: Option<Hinweis>,
    pub profil: Vec<(i64, i32)>,
    pub rolltempo_grenze_kt: f32,
    /// Der Pilot will Hinweise und kurze Haken im Flug.
    pub hinweise_im_flug: bool,
}

pub fn live_ansicht(z: &Zustand, einst: &Einstellungen, vfr: bool) -> LiveAnsicht {
    let mut z = z.clone();
    z.sicherstellen();
    let klasse = z.klasse;
    let aus_grund = match klasse {
        Some(Klasse::Ga) if !einst.ga_an => Some("ga".to_string()),
        _ if vfr && !einst.vfr_an => Some("vfr".to_string()),
        _ => None,
    };
    let hinweis = if aus_grund.is_none() && einst.hinweise_im_flug {
        z.hinweis.clone()
    } else {
        None
    };
    LiveAnsicht {
        klasse,
        nacht_start: z.nacht_start,
        eingeschaltet: Schalter::ALLE
            .iter()
            .copied()
            .filter(|s| einst.an(*s))
            .collect(),
        aus_grund,
        punkte: z.punkte,
        hinweis,
        profil: z.profil,
        rolltempo_grenze_kt: einst.rolltempo_fuer(klasse.unwrap_or(Klasse::Airliner)),
        hinweise_im_flug: einst.hinweise_im_flug,
    }
}

// ---------------------------------------------------------------------------
// Speicher (lokal, je Pilot)
// ---------------------------------------------------------------------------

/// Höchstens so viele Einträge lokal (älteste fliegen raus).
const MAX_EINTRAEGE: usize = 3_000;

/// Ein Lese-Ändern-Schreiben zur Zeit — Flugende, Markieren und Server-
/// Abgleich können sich sonst gegenseitig überschreiben.
static SPEICHER_SPERRE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Lokaler Speicher unter `<app_data>/bordbuch/<pilot>/` — je Pilot
/// getrennt, damit ein zweiter Pilot am selben Rechner weder Verlauf noch
/// Einstellungen des ersten sieht.
pub struct Speicher {
    eintraege: std::path::PathBuf,
    einstellungen: std::path::PathBuf,
}

fn atomar_schreiben(pfad: &std::path::Path, inhalt: &[u8]) -> std::io::Result<()> {
    let tmp = pfad.with_extension("json.tmp");
    std::fs::write(&tmp, inhalt)?;
    std::fs::rename(&tmp, pfad)
}

impl Speicher {
    pub fn oeffnen(app_data: &std::path::Path, pilot: &str) -> std::io::Result<Self> {
        let sauber: String = pilot
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .take(40)
            .collect();
        let sauber = if sauber.is_empty() {
            "ohne_anmeldung".to_string()
        } else {
            sauber
        };
        let dir = app_data.join("bordbuch").join(sauber);
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            eintraege: dir.join("eintraege.json"),
            einstellungen: dir.join("einstellungen.json"),
        })
    }

    fn lesen(&self) -> Vec<Eintrag> {
        let Ok(roh) = std::fs::read(&self.eintraege) else {
            return Vec::new();
        };
        match serde_json::from_slice::<Vec<serde_json::Value>>(&roh) {
            // Einzeln lesen: ein kaputter Eintrag (ältere/neuere Fassung)
            // darf den Rest nicht mitreissen.
            Ok(werte) => werte
                .into_iter()
                .filter_map(|v| serde_json::from_value(v).ok())
                .collect(),
            Err(e) => {
                // Datei beschädigt: beiseitelegen statt überschreiben.
                let kaputt = self.eintraege.with_extension("json.kaputt");
                let _ = std::fs::copy(&self.eintraege, kaputt);
                tracing::warn!(error = %e, "Bordbuch-Datei nicht lesbar — beiseitegelegt");
                Vec::new()
            }
        }
    }

    fn schreiben(&self, mut alle: Vec<Eintrag>) -> std::io::Result<()> {
        alle.sort_by(|a, b| a.erstellt_at.cmp(&b.erstellt_at));
        if alle.len() > MAX_EINTRAEGE {
            let weg = alle.len() - MAX_EINTRAEGE;
            alle.drain(0..weg);
        }
        let json = serde_json::to_vec(&alle).map_err(std::io::Error::other)?;
        atomar_schreiben(&self.eintraege, &json)
    }

    /// Alle Einträge, neuester zuerst.
    pub fn alle(&self) -> Vec<Eintrag> {
        let _g = SPEICHER_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let mut v = self.lesen();
        v.sort_by(|a, b| b.erstellt_at.cmp(&a.erstellt_at));
        v
    }

    pub fn holen(&self, pirep_id: &str) -> Option<Eintrag> {
        let _g = SPEICHER_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        self.lesen().into_iter().find(|e| e.pirep_id == pirep_id)
    }

    /// Eintrag anlegen oder ersetzen (gleiche PIREP).
    pub fn speichern(&self, e: Eintrag) -> std::io::Result<()> {
        let _g = SPEICHER_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let mut alle = self.lesen();
        alle.retain(|x| x.pirep_id != e.pirep_id);
        alle.push(e);
        self.schreiben(alle)
    }

    /// Einen Eintrag ändern (Markieren „nach ATC", Sync-Vermerk).
    pub fn aendern<F: FnOnce(&mut Eintrag) -> bool>(
        &self,
        pirep_id: &str,
        f: F,
    ) -> std::io::Result<Option<Eintrag>> {
        let _g = SPEICHER_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let mut alle = self.lesen();
        let Some(e) = alle.iter_mut().find(|e| e.pirep_id == pirep_id) else {
            return Ok(None);
        };
        if !f(e) {
            return Ok(None);
        }
        let neu = e.clone();
        self.schreiben(alle)?;
        Ok(Some(neu))
    }

    /// Serverstand einmischen: je PIREP gewinnt das neuere `updated_at`.
    /// Liefert, wie viele Einträge neu oder aktualisiert wurden.
    pub fn zusammenfuehren(&self, vom_server: Vec<Eintrag>) -> std::io::Result<usize> {
        let _g = SPEICHER_SPERRE.lock().unwrap_or_else(|e| e.into_inner());
        let mut alle = self.lesen();
        let mut geaendert = 0;
        for mut s in vom_server {
            s.synced = true;
            match alle.iter_mut().find(|l| l.pirep_id == s.pirep_id) {
                Some(l) if l.updated_at >= s.updated_at => {
                    if l.updated_at == s.updated_at && !l.synced {
                        l.synced = true;
                    }
                }
                Some(l) => {
                    *l = s;
                    geaendert += 1;
                }
                None => {
                    alle.push(s);
                    geaendert += 1;
                }
            }
        }
        self.schreiben(alle)?;
        Ok(geaendert)
    }

    pub fn einstellungen(&self) -> Einstellungen {
        std::fs::read(&self.einstellungen)
            .ok()
            .and_then(|r| serde_json::from_slice::<Einstellungen>(&r).ok())
            .map(Einstellungen::bereinigt)
            .unwrap_or_default()
    }

    /// `None` = es gibt noch keine gespeicherten Einstellungen.
    pub fn einstellungen_vorhanden(&self) -> Option<Einstellungen> {
        std::fs::read(&self.einstellungen)
            .ok()
            .and_then(|r| serde_json::from_slice::<Einstellungen>(&r).ok())
            .map(Einstellungen::bereinigt)
    }

    pub fn einstellungen_speichern(&self, e: &Einstellungen) -> std::io::Result<()> {
        let json =
            serde_json::to_vec_pretty(&e.clone().bereinigt()).map_err(std::io::Error::other)?;
        atomar_schreiben(&self.einstellungen, &json)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t0() -> DateTime<Utc> {
        // 26.09.2026 12:00 UTC — Frankfurt am Mittag: Tag.
        Utc.with_ymd_and_hms(2026, 9, 26, 12, 0, 0).unwrap()
    }

    fn snap(sek: i64) -> SimSnapshot {
        let mut s = SimSnapshot::default();
        s.timestamp = t0() + chrono::Duration::seconds(sek);
        s.lat = 50.03;
        s.lon = 8.56;
        s.on_ground = true;
        s.parking_brake = true;
        s.aircraft_profile = AircraftProfile::FenixA320;
        s.aircraft_title = Some("Fenix A320 CFM".into());
        s.light_beacon = Some(false);
        s.light_nav = Some(false);
        s.light_strobe = Some(false);
        s.strobe_state = Some(0);
        s.light_landing = Some(false);
        s.light_taxi = Some(false);
        s.xpdr_mode_label = Some("STBY".into());
        s.seatbelts_sign = Some(0);
        s.spoilers_armed = Some(false);
        s.autobrake = Some("OFF".into());
        s.apu_switch = Some(true);
        s
    }

    /// Takte im 5-s-Raster bis `ziel_s` — wie der echte Streamer. Grössere
    /// Sprünge gelten als Pause und verschieben die Fristen.
    fn takte_bis(
        z: &mut Zustand,
        s: &mut SimSnapshot,
        phase: FlightPhase,
        k: &Kontext,
        ziel_s: i64,
    ) {
        let mut t = z.letzte_zeit.map(|l| (l - t0()).num_seconds()).unwrap_or(0);
        while t < ziel_s {
            t = (t + 5).min(ziel_s);
            s.timestamp = t0() + chrono::Duration::seconds(t);
            tick(z, s, phase, k);
        }
    }

    fn ctx(e: &Einstellungen) -> Kontext<'_> {
        Kontext {
            klasse: Klasse::Airliner,
            einstellungen: e,
            abflug: Some("EDDF"),
            ziel: Some("EDDM"),
        }
    }

    /// Ein ganzer, sauberer Airliner-Flug: alles erledigt → 100 %.
    fn sauberer_flug(z: &mut Zustand, e: &Einstellungen) {
        let k = ctx(e);
        let mut t = 0;
        let mut s = snap(t);
        tick(z, &s, FlightPhase::Preflight, &k);
        // Beacon an, dann Triebwerke.
        t += 30;
        s = snap(t);
        s.light_beacon = Some(true);
        s.light_nav = Some(true);
        tick(z, &s, FlightPhase::Pushback, &k);
        t += 60;
        s.timestamp = t0() + chrono::Duration::seconds(t);
        s.engines_running = 2;
        tick(z, &s, FlightPhase::Pushback, &k);
        // Rollen mit 20 kt.
        for i in 0..20 {
            t += 5;
            s.timestamp = t0() + chrono::Duration::seconds(t);
            s.parking_brake = false;
            s.groundspeed_kt = if i == 0 { 8.0 } else { 20.0 };
            s.seatbelts_sign = Some(2);
            tick(z, &s, FlightPhase::TaxiOut, &k);
        }
        // Line-up: Strobes, Landelicht, TA/RA, Klappen.
        t += 5;
        s.timestamp = t0() + chrono::Duration::seconds(t);
        s.strobe_state = Some(2);
        s.light_strobe = Some(true);
        s.light_landing = Some(true);
        s.xpdr_mode_label = Some("TA-RA".into());
        s.flaps_position = 0.25;
        s.flap_handle_index = Some(1);
        s.groundspeed_kt = 40.0;
        tick(z, &s, FlightPhase::TakeoffRoll, &k);
        // Steigflug über FL100, Reiseflug, APU aus.
        s.on_ground = false;
        s.apu_switch = Some(false);
        s.altitude_msl_ft = 36_000.0;
        s.altitude_agl_ft = 35_500.0;
        for _ in 0..5 {
            t += 60;
            s.timestamp = t0() + chrono::Duration::seconds(t);
            tick(z, &s, FlightPhase::Cruise, &k);
        }
        // Sinkflug durch FL100 mit Landelicht (war noch an).
        t += 60;
        s.timestamp = t0() + chrono::Duration::seconds(t);
        s.altitude_msl_ft = 9_000.0;
        s.altitude_agl_ft = 8_500.0;
        s.vertical_speed_fpm = -1500.0;
        tick(z, &s, FlightPhase::Descent, &k);
        // Endanflug.
        t += 300;
        s.timestamp = t0() + chrono::Duration::seconds(t);
        s.altitude_msl_ft = 2_300.0;
        s.altitude_agl_ft = 800.0;
        s.spoilers_armed = Some(true);
        s.autobrake = Some("LO".into());
        tick(z, &s, FlightPhase::Final, &k);
        // Aufsetzen.
        t += 60;
        s.timestamp = t0() + chrono::Duration::seconds(t);
        s.on_ground = true;
        s.altitude_agl_ft = 0.0;
        s.groundspeed_kt = 120.0;
        tick(z, &s, FlightPhase::Landing, &k);
        // Rollen zum Stand: 35 kt in den ersten 20 s (Abrollweg), danach 15.
        for i in 0..20 {
            t += 5;
            s.timestamp = t0() + chrono::Duration::seconds(t);
            s.groundspeed_kt = if i < 4 { 35.0 } else { 15.0 };
            tick(z, &s, FlightPhase::TaxiIn, &k);
        }
    }

    fn eintrag(z: &Zustand, e: &Einstellungen) -> Eintrag {
        abschliessen(
            z,
            "P1",
            FlugInfo::default(),
            false,
            e,
            (Klasse::Airliner, "test"),
            "test",
            t0() + chrono::Duration::hours(3),
        )
    }

    fn status(e: &Eintrag, r: Regel) -> Status {
        e.punkte.iter().find(|p| p.regel == r).unwrap().status
    }

    #[test]
    fn sauberer_flug_alles_erledigt() {
        let mut e = Einstellungen::default();
        for s in Schalter::ALLE {
            e.regeln.insert(s, true);
        }
        let mut z = Zustand::default();
        sauberer_flug(&mut z, &e);
        let en = eintrag(&z, &e);
        for p in &en.punkte {
            assert_eq!(p.status, Status::Erledigt, "{:?}: {:?}", p.regel, p);
        }
        let (ok, von) = en.bilanz();
        assert_eq!(ok, von);
        // Pflichtpunkte des Airliners (APU + Autobrake sind Bestätigung).
        assert_eq!(von, 14);
        assert_eq!(
            en.punkte
                .iter()
                .find(|p| p.regel == Regel::TransponderStart)
                .unwrap()
                .stellung
                .as_deref(),
            Some("TA-RA")
        );
    }

    #[test]
    fn beacon_zu_spaet_ist_diesmal_ohne_und_frist_gilt() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        // Beacon 10 s nach dem Anlassen → noch in der Frist.
        let mut z = Zustand::default();
        let mut s = snap(0);
        tick(&mut z, &s, FlightPhase::Preflight, &k);
        s = snap(5);
        s.engines_running = 1;
        tick(&mut z, &s, FlightPhase::Pushback, &k);
        assert_eq!(
            z.hinweis.as_ref().map(|h| h.regel),
            Some(Regel::BeaconAnlassen)
        );
        s = snap(15);
        s.engines_running = 1;
        s.light_beacon = Some(true);
        tick(&mut z, &s, FlightPhase::Pushback, &k);
        assert_eq!(z.status(Regel::BeaconAnlassen), Status::Erledigt);
        assert!(z.hinweis.is_none(), "Hinweis verschwindet, sobald erledigt");
        // 40 s später → diesmal ohne.
        let mut z = Zustand::default();
        tick(&mut z, &snap(0), FlightPhase::Preflight, &k);
        let mut s = snap(5);
        s.engines_running = 1;
        tick(&mut z, &s, FlightPhase::Pushback, &k);
        let mut s = snap(5);
        s.engines_running = 1;
        takte_bis(&mut z, &mut s, FlightPhase::Pushback, &k, 45);
        assert_eq!(z.status(Regel::BeaconAnlassen), Status::DiesmalOhne);
    }

    #[test]
    fn feld_fehlt_heisst_nicht_messbar_nie_diesmal_ohne() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.light_beacon = None;
        tick(&mut z, &s, FlightPhase::Preflight, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        s.engines_running = 1;
        tick(&mut z, &s, FlightPhase::Pushback, &k);
        assert!(z.hinweis.is_none(), "kein Hinweis ohne Messwert");
        takte_bis(&mut z, &mut s, FlightPhase::Pushback, &k, 60);
        let p = &z.punkte[Regel::BeaconAnlassen.index()];
        assert_eq!(p.status, Status::NichtMessbar);
        assert_eq!(p.grund, Some(Grund::WertFehlt));
        let en = eintrag(&z, &e);
        assert!(en.zaehlbare().all(|p| p.regel != Regel::BeaconAnlassen));
    }

    #[test]
    fn rolltempo_zaehlt_erst_ueber_zehn_sekunden() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let lauf = |ueber_s: i64| {
            let mut z = Zustand::default();
            let mut t = 0;
            let mut s = snap(0);
            s.engines_running = 2;
            s.parking_brake = false;
            tick(&mut z, &s, FlightPhase::TaxiOut, &k);
            for i in 0..40 {
                t += 1;
                s.timestamp = t0() + chrono::Duration::seconds(t);
                s.groundspeed_kt = if i < ueber_s { 34.0 } else { 18.0 };
                tick(&mut z, &s, FlightPhase::TaxiOut, &k);
            }
            t += 1;
            s.timestamp = t0() + chrono::Duration::seconds(t);
            tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
            z.status(Regel::RolltempoAbflug)
        };
        assert_eq!(lauf(8), Status::Erledigt, "8 s über 30 kt sind normal");
        assert_eq!(lauf(15), Status::DiesmalOhne);
    }

    #[test]
    fn rolltempo_abrollweg_zaehlt_nicht() {
        let mut e = Einstellungen::default();
        e.regeln.insert(Schalter::Rolltempo, true);
        let mut z = Zustand::default();
        sauberer_flug(&mut z, &e);
        let en = eintrag(&z, &e);
        assert_eq!(status(&en, Regel::RolltempoAnkunft), Status::Erledigt);
    }

    #[test]
    fn strobe_auto_zaehlt_als_erfuellt() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.engines_running = 2;
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        s.strobe_state = Some(1); // AUTO, am Boden noch dunkel
        s.light_strobe = Some(false);
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        assert_eq!(z.status(Regel::StrobesStart), Status::Erledigt);
    }

    #[test]
    fn strobes_hinweis_als_frage_und_nur_einmal() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.engines_running = 2;
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        s.xpdr_mode_label = Some("TA-RA".into());
        s.light_landing = Some(true);
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        assert_eq!(
            z.hinweis.as_ref().map(|h| h.regel),
            Some(Regel::StrobesStart)
        );
        // Nach 8 s weg, auch wenn nicht erledigt.
        s.timestamp = t0() + chrono::Duration::seconds(14);
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        assert!(z.hinweis.is_none());
        s.timestamp = t0() + chrono::Duration::seconds(20);
        tick(&mut z, &s, FlightPhase::Takeoff, &k);
        assert_eq!(z.status(Regel::StrobesStart), Status::DiesmalOhne);
    }

    #[test]
    fn hinweise_aus_heisst_kein_hinweis() {
        let mut e = Einstellungen::default();
        e.hinweise_im_flug = false;
        let k = ctx(&e);
        let mut z = Zustand::default();
        tick(&mut z, &snap(0), FlightPhase::Preflight, &k);
        let mut s = snap(5);
        s.engines_running = 1;
        tick(&mut z, &s, FlightPhase::Pushback, &k);
        assert!(z.hinweis.is_none());
    }

    #[test]
    fn transponder_on_ohne_hoehe_ist_diesmal_ohne_xpndr_mit() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        for (label, want) in [
            ("ON", Status::DiesmalOhne),
            ("STBY", Status::DiesmalOhne),
            ("XPNDR", Status::Erledigt),
            ("ALT", Status::Erledigt),
            ("TA", Status::Erledigt),
        ] {
            let mut z = Zustand::default();
            let mut s = snap(0);
            s.engines_running = 2;
            tick(&mut z, &s, FlightPhase::TaxiOut, &k);
            s.timestamp = t0() + chrono::Duration::seconds(5);
            s.xpdr_mode_label = Some(label.into());
            tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
            takte_bis(&mut z, &mut s, FlightPhase::Takeoff, &k, 30);
            assert_eq!(z.status(Regel::TransponderStart), want, "{label}");
        }
    }

    #[test]
    fn tcas_nicht_messbar_wenn_quelle_keinen_tcas_modus_kennt() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.aircraft_profile = AircraftProfile::IniA380; // Standard-Rückfall: ALT
        s.engines_running = 4;
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        s.xpdr_mode_label = Some("ALT".into());
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        takte_bis(&mut z, &mut s, FlightPhase::Takeoff, &k, 30);
        let p = &z.punkte[Regel::TcasStart.index()];
        assert_eq!(p.status, Status::NichtMessbar);
        assert_eq!(p.grund, Some(Grund::KeinTcasModus));
        assert_eq!(z.status(Regel::TransponderStart), Status::Erledigt);
    }

    #[test]
    fn nach_atc_markieren_nur_bei_diesmal_ohne() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.engines_running = 2;
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        s.xpdr_mode_label = Some("TA-RA".into());
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        takte_bis(&mut z, &mut s, FlightPhase::Takeoff, &k, 30);
        let mut en = eintrag(&z, &e);
        assert_eq!(status(&en, Regel::StrobesStart), Status::DiesmalOhne);
        let (ok0, _) = en.bilanz();
        assert!(en.markieren(Regel::StrobesStart, true, t0()));
        assert_eq!(status(&en, Regel::StrobesStart), Status::NachAtc);
        assert_eq!(en.bilanz().0, ok0 + 1, "nach ATC zählt als erledigt");
        // Erledigte Punkte lassen sich nicht umschreiben.
        assert!(!en.markieren(Regel::TransponderStart, true, t0()));
        // Zurücknehmen geht.
        assert!(en.markieren(Regel::StrobesStart, false, t0()));
        assert_eq!(status(&en, Regel::StrobesStart), Status::DiesmalOhne);
    }

    #[test]
    fn ga_klasse_und_voreinstellung_aus() {
        let e = Einstellungen::default();
        let (kl, q) = klasse_fuer(Some("C172"), None, AircraftProfile::Default, &e);
        assert_eq!((kl, q), (Klasse::Ga, "icao"));
        assert_eq!(
            klasse_fuer(Some("C700"), None, AircraftProfile::Default, &e).0,
            Klasse::BusinessJet
        );
        assert_eq!(
            klasse_fuer(Some("A20N"), None, AircraftProfile::Default, &e).0,
            Klasse::Airliner
        );
        // Add-on-Profil schlägt die (evtl. leere) ICAO.
        assert_eq!(
            klasse_fuer(None, None, AircraftProfile::FsrPhenom300e, &e).0,
            Klasse::BusinessJet
        );
        let mut e2 = e.clone();
        e2.klassen_override
            .insert("D-EABC".into(), Klasse::BusinessJet);
        assert_eq!(
            klasse_fuer(Some("C172"), Some("d-eabc"), AircraftProfile::Default, &e2),
            (Klasse::BusinessJet, "einstellung_kennzeichen")
        );
        // GA: Bordbuch aus, solange nicht eingeschaltet.
        let z = Zustand {
            klasse: Some(Klasse::Ga),
            ..Default::default()
        };
        let en = abschliessen(
            &z,
            "P",
            FlugInfo::default(),
            false,
            &e,
            (Klasse::Ga, "icao"),
            "t",
            t0(),
        );
        assert_eq!(en.aus_grund.as_deref(), Some("ga"));
        // GA: TCAS/Klappen/APU/Autobrake/Spoiler/Anschnallen passen nicht.
        for r in [
            Regel::TcasStart,
            Regel::KlappenStart,
            Regel::ApuReiseflug,
            Regel::AutobrakeLandung,
            Regel::SpoilerLandung,
            Regel::AnschnallStart,
        ] {
            assert_eq!(art(r, Klasse::Ga, false), None, "{r:?}");
        }
        assert_eq!(
            art(Regel::NavLichter, Klasse::Ga, false),
            Some(Art::Bestaetigung)
        );
        assert_eq!(art(Regel::NavLichter, Klasse::Ga, true), Some(Art::Pflicht));
    }

    #[test]
    fn vfr_aus_solange_nicht_eingeschaltet() {
        let e = Einstellungen::default();
        let en = abschliessen(
            &Zustand::default(),
            "P",
            FlugInfo::default(),
            true,
            &e,
            (Klasse::Airliner, "icao"),
            "t",
            t0(),
        );
        assert_eq!(en.aus_grund.as_deref(), Some("vfr"));
        assert_eq!(en.regelwerk, "vfr");
    }

    #[test]
    fn nie_ausgeloeste_punkte_sind_nicht_anwendbar() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        tick(&mut z, &snap(0), FlightPhase::Preflight, &k);
        let en = eintrag(&z, &e);
        for p in &en.punkte {
            assert_eq!(p.status, Status::NichtAnwendbar, "{:?}", p.regel);
        }
        assert_eq!(en.bilanz(), (0, 0));
    }

    #[test]
    fn autobrake_labels() {
        for l in [
            "LO", "MED", "MAX", "1", "3", "BTV", "L2", "HI", "ARMED", "MAX AUTO",
        ] {
            assert!(autobrake_gesetzt(l), "{l}");
        }
        for l in ["OFF", "DISARM", "RTO", "", "#3", "?"] {
            assert!(!autobrake_gesetzt(l), "{l}");
        }
    }

    #[test]
    fn sonnenstand_und_nacht() {
        // Frankfurt 26.09. 12:00 UTC: Sonne hoch (~39°).
        let h = sonnenhoehe(50.03, 8.56, t0());
        assert!((35.0..45.0).contains(&h), "{h}");
        // Frankfurt 26.09. 22:00 UTC: tief unter dem Horizont.
        let n = sonnenhoehe(
            50.03,
            8.56,
            Utc.with_ymd_and_hms(2026, 9, 26, 22, 0, 0).unwrap(),
        );
        assert!(n < -20.0, "{n}");
        assert!(ist_nacht(n, Some("EDDF")));
        assert!(!ist_nacht(h, Some("EDDF")));
        // Dämmerung −3°: in den USA Nacht, in der EU noch nicht.
        assert!(ist_nacht(-3.0, Some("KJFK")));
        assert!(!ist_nacht(-3.0, Some("EDDF")));
    }

    #[test]
    fn nacht_macht_nav_fuer_ga_zur_pflicht() {
        let mut e = Einstellungen::default();
        e.ga_an = true;
        let k = Kontext {
            klasse: Klasse::Ga,
            einstellungen: &e,
            abflug: Some("EDDF"),
            ziel: None,
        };
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.sim_zeit_utc = Some(Utc.with_ymd_and_hms(2026, 9, 26, 22, 0, 0).unwrap());
        tick(&mut z, &s, FlightPhase::Preflight, &k);
        assert_eq!(z.nacht_start, Some(true));
        assert_eq!(z.zeitquelle.as_deref(), Some("sim"));
        assert_eq!(z.punkte[Regel::NavLichter.index()].art, Some(Art::Pflicht));
    }

    #[test]
    fn einstellungen_voreinstellung_und_bereinigung() {
        let e = Einstellungen::default();
        assert!(e.an(Schalter::Beacon) && e.an(Schalter::Apu));
        assert!(!e.an(Schalter::Spoiler) && !e.an(Schalter::Klappen));
        let mut e2 = e.clone();
        e2.rolltempo_kt = 500.0;
        e2.klassen_override.insert(" c172 ".into(), Klasse::Ga);
        let b = e2.bereinigt();
        assert_eq!(b.rolltempo_kt, 30.0);
        assert!(b.klassen_override.contains_key("C172"));
        // Serde: fehlende Felder = Voreinstellung (ältere Datei vom Server).
        let alt: Einstellungen = serde_json::from_str(r#"{"hinweise_im_flug":false}"#).unwrap();
        assert!(!alt.hinweise_im_flug);
        assert_eq!(alt.rolltempo_ga_kt, 20.0);
    }

    #[test]
    fn eintrag_json_rundreise() {
        let mut e = Einstellungen::default();
        e.regeln.insert(Schalter::Spoiler, true);
        let mut z = Zustand::default();
        sauberer_flug(&mut z, &e);
        let en = eintrag(&z, &e);
        let j = serde_json::to_string(&en).unwrap();
        let zur: Eintrag = serde_json::from_str(&j).unwrap();
        assert_eq!(zur, en);
        assert!(j.contains("\"regel\":\"spoiler_landung\""));
        assert!(j.contains("\"status\":\"erledigt\""));
        // Zustand ebenso (Persistenz beim Neustart der App).
        let zj = serde_json::to_string(&z).unwrap();
        let zz: Zustand = serde_json::from_str(&zj).unwrap();
        assert_eq!(zz, z);
    }

    #[test]
    fn speicher_je_pilot_getrennt_und_neuere_fassung_gewinnt() {
        let dir = std::env::temp_dir().join(format!(
            "bordbuch-test-{}",
            std::process::id() as u64 * 1000 + t0().timestamp_subsec_nanos() as u64
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let a = Speicher::oeffnen(&dir, "123").unwrap();
        let b = Speicher::oeffnen(&dir, "456").unwrap();
        let e = Einstellungen::default();
        let mut en = eintrag(&Zustand::default(), &e);
        en.pirep_id = "X1".into();
        a.speichern(en.clone()).unwrap();
        assert_eq!(a.alle().len(), 1);
        assert!(b.alle().is_empty(), "anderer Pilot sieht nichts");
        // Markieren über aendern.
        let neu = a
            .aendern("X1", |x| {
                x.updated_at = x.updated_at + chrono::Duration::minutes(1);
                true
            })
            .unwrap()
            .unwrap();
        // Server hat eine ältere Fassung → lokal bleibt.
        let mut alt = en.clone();
        alt.updated_at = en.updated_at - chrono::Duration::hours(1);
        assert_eq!(a.zusammenfuehren(vec![alt]).unwrap(), 0);
        assert_eq!(a.holen("X1").unwrap().updated_at, neu.updated_at);
        // Server hat eine neuere → sie gewinnt und gilt als gesichert.
        let mut juenger = en.clone();
        juenger.updated_at = neu.updated_at + chrono::Duration::hours(1);
        juenger.regelwerk = "vfr".into();
        assert_eq!(a.zusammenfuehren(vec![juenger]).unwrap(), 1);
        let h = a.holen("X1").unwrap();
        assert_eq!(h.regelwerk, "vfr");
        assert!(h.synced);
        // Einstellungen je Pilot.
        let mut ea = Einstellungen::default();
        ea.hinweise_im_flug = false;
        a.einstellungen_speichern(&ea).unwrap();
        assert!(!a.einstellungen().hinweise_im_flug);
        assert!(b.einstellungen().hinweise_im_flug);
        assert!(b.einstellungen_vorhanden().is_none());
        // Beschädigte Datei → leer + beiseitegelegt, nichts panikt.
        std::fs::write(&a.eintraege, b"{kaputt").unwrap();
        assert!(a.alle().is_empty());
        assert!(a.eintraege.with_extension("json.kaputt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// QS 26.09.2026: Touch-and-go / Aufsetzer mit Durchstart friert die
    /// Anflugpunkte nicht ein — die echte Landung mit Spoilern zählt.
    #[test]
    fn touch_and_go_friert_anflugpunkte_nicht_ein() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.engines_running = 2;
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        // Erster Anflug ohne Spoiler, Aufsetzer, Durchstart.
        s.on_ground = false;
        s.altitude_agl_ft = 600.0;
        s.timestamp = t0() + chrono::Duration::seconds(600);
        tick(&mut z, &s, FlightPhase::Final, &k);
        s.on_ground = true;
        s.altitude_agl_ft = 0.0;
        s.timestamp = t0() + chrono::Duration::seconds(620);
        tick(&mut z, &s, FlightPhase::Landing, &k);
        assert_eq!(
            z.status(Regel::SpoilerLandung),
            Status::Offen,
            "Aufsetzer entscheidet nicht"
        );
        s.on_ground = false;
        s.altitude_agl_ft = 1500.0;
        s.timestamp = t0() + chrono::Duration::seconds(640);
        tick(&mut z, &s, FlightPhase::Climb, &k);
        // Zweiter Anflug mit Spoilern, dann richtige Landung.
        s.altitude_agl_ft = 700.0;
        s.spoilers_armed = Some(true);
        s.timestamp = t0() + chrono::Duration::seconds(1200);
        tick(&mut z, &s, FlightPhase::Final, &k);
        assert_eq!(z.status(Regel::SpoilerLandung), Status::Erledigt);
        // Ohne Spoiler bis zum Ausrollen → erst dann diesmal ohne.
        let mut z2 = Zustand::default();
        let mut s2 = snap(0);
        s2.engines_running = 2;
        tick(&mut z2, &s2, FlightPhase::TaxiOut, &k);
        s2.timestamp = t0() + chrono::Duration::seconds(5);
        tick(&mut z2, &s2, FlightPhase::TakeoffRoll, &k);
        s2.on_ground = false;
        s2.altitude_agl_ft = 600.0;
        s2.timestamp = t0() + chrono::Duration::seconds(600);
        tick(&mut z2, &s2, FlightPhase::Final, &k);
        s2.on_ground = true;
        s2.timestamp = t0() + chrono::Duration::seconds(640);
        tick(&mut z2, &s2, FlightPhase::Landing, &k);
        assert_eq!(z2.status(Regel::SpoilerLandung), Status::Offen);
        s2.timestamp = t0() + chrono::Duration::seconds(700);
        tick(&mut z2, &s2, FlightPhase::TaxiIn, &k);
        assert_eq!(z2.status(Regel::SpoilerLandung), Status::DiesmalOhne);
    }

    /// Flug endet (Einreichen) noch vor dem Rollen zum Stand: die Anflug-
    /// punkte sind vorgekommen → entschieden, nicht „kam nicht vor".
    #[test]
    fn flugende_vor_dem_ausrollen_entscheidet_laufende_punkte() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.engines_running = 2;
        s.spoilers_armed = None; // dieses Flugzeug meldet die Spoiler nie
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        s.on_ground = false;
        s.altitude_agl_ft = 600.0;
        s.timestamp = t0() + chrono::Duration::seconds(600);
        tick(&mut z, &s, FlightPhase::Final, &k);
        let en = eintrag(&z, &e);
        assert_eq!(status(&en, Regel::SpoilerLandung), Status::NichtMessbar);
        assert_eq!(status(&en, Regel::AnschnallLandung), Status::DiesmalOhne);
        let p = en
            .punkte
            .iter()
            .find(|p| p.regel == Regel::AnschnallLandung)
            .unwrap();
        assert!(p.beleg.contains_key("entschieden"), "keine Schein-Rohwerte");
        assert!(!p.beleg.contains_key("seatbelts_sign"));
    }

    /// Flugende mitten in der TCAS-Frist bei einem Muster ohne TCAS-Stellung:
    /// „nicht messbar", nicht „diesmal ohne".
    #[test]
    fn flugende_in_tcas_frist_bleibt_nicht_messbar() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.aircraft_profile = AircraftProfile::IniA380;
        s.xpdr_mode_label = Some("ALT".into());
        s.engines_running = 4;
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        let en = eintrag(&z, &e);
        let p = en
            .punkte
            .iter()
            .find(|p| p.regel == Regel::TcasStart)
            .unwrap();
        assert_eq!(
            (p.status, p.grund),
            (Status::NichtMessbar, Some(Grund::KeinTcasModus))
        );
    }

    /// Pause mitten in der Kulanz: die Frist verlängert sich um die Pause.
    #[test]
    fn pause_verlaengert_die_frist() {
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        tick(&mut z, &snap(0), FlightPhase::Preflight, &k);
        let mut s = snap(5);
        s.engines_running = 1;
        tick(&mut z, &s, FlightPhase::Pushback, &k);
        // 5 Minuten Pause (keine Takte), danach Beacon an.
        let mut s = snap(305);
        s.engines_running = 1;
        s.light_beacon = Some(true);
        tick(&mut z, &s, FlightPhase::Pushback, &k);
        assert_eq!(z.status(Regel::BeaconAnlassen), Status::Erledigt);
    }

    #[test]
    fn landelicht_anflug_ohne_fl100_im_endanflug() {
        // Kurzer Flug unter FL100 → geprüft unter 1000 ft AGL.
        let e = Einstellungen::default();
        let k = ctx(&e);
        let mut z = Zustand::default();
        let mut s = snap(0);
        s.engines_running = 2;
        tick(&mut z, &s, FlightPhase::TaxiOut, &k);
        s.timestamp = t0() + chrono::Duration::seconds(5);
        tick(&mut z, &s, FlightPhase::TakeoffRoll, &k);
        s.on_ground = false;
        s.altitude_msl_ft = 6000.0;
        s.altitude_agl_ft = 5000.0;
        s.timestamp = t0() + chrono::Duration::seconds(600);
        tick(&mut z, &s, FlightPhase::Cruise, &k);
        s.altitude_agl_ft = 900.0;
        s.light_landing = Some(true);
        s.timestamp = t0() + chrono::Duration::seconds(1200);
        tick(&mut z, &s, FlightPhase::Final, &k);
        assert_eq!(z.status(Regel::LandelichtAnflug), Status::Erledigt);
    }
}
