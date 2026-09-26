//! Telemetrie-Monitor (v1.8): der Datenstrom fuer den Tab „Telemetrie".
//!
//! Aus jedem Messpunkt wird eine flache Werteliste gebaut — ein Eintrag je
//! Kanal im [`KATALOG`]. Die Oberflaeche bekommt den Katalog einmal beim
//! Oeffnen und danach nur noch die Wertelisten ([`Frame`]); Verlauf,
//! Diagramme, Watch-Panel und CSV arbeiten alle auf derselben Form.
//!
//! Drei Quellen fuellen die Kanaele:
//!   * der gewoehnliche [`SimSnapshot`] (Simulator und Addon-LVars),
//!   * Zusatzwerte, die NUR gelesen werden, solange der Monitor offen ist
//!     (MSFS: eigene Datendefinition, X-Plane: eigener Nummernbereich),
//!   * daraus berechnete Werte (Vorhaltewinkel, Gegenwind, Sprit je Tank …).
//!
//! Die Landebewertung liest diese Werte nicht. Der Monitor ist reine
//! Anzeige; ein Fehler hier darf keinen Flug beeinflussen.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use sim_core::SimSnapshot;

/// Verlauf: 5 Minuten bei 10 Hz.
pub const VERLAUF_HZ: u32 = 10;
pub const VERLAUF_MAX: usize = 5 * 60 * VERLAUF_HZ as usize;
/// Takt des Live-Stroms an die Oberflaeche.
pub const STROM_TAKT: Duration = Duration::from_millis(50);
/// v1.8.1: Praefix der Tablets (LAN-Bruecke) in der Zuschauerliste. Jedes
/// Tablet meldet sich als `lan:<geraet>` — schliesst eines den Monitor,
/// laeuft der Strom fuer die anderen ohne Aussetzer weiter.
pub const LAN_ZUSCHAUER: &str = "lan";

/// Zuschauerkennung eines Tablets. Die Geraetekennung kommt von der
/// Oberflaeche; sie wird gekuerzt und auf harmlose Zeichen begrenzt, weil
/// sie nur als Schluessel dient.
pub fn lan_zuschauer(geraet: Option<&str>) -> String {
    let rein: String = geraet
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(40)
        .collect();
    if rein.is_empty() {
        LAN_ZUSCHAUER.to_string()
    } else {
        format!("{LAN_ZUSCHAUER}:{rein}")
    }
}
/// Jeder wievielte Takt geht ans Tablet: 20 Hz / 2 = 10 Frames je Sekunde.
pub const LAN_JEDER_NTE: u32 = 2;

/// Ohne Lebenszeichen der Oberflaeche endet der Strom nach dieser Zeit —
/// ein abgestuerztes oder geschlossenes Fenster haelt ihn nicht offen.
pub const HALTEN_DAUER: Duration = Duration::from_secs(15);

const FT_S2_JE_G: f64 = 32.174;
const LB_JE_KG: f64 = 2.204_622_6;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Quelle {
    /// Standardwert des Simulators, liest AeroACARS ohnehin.
    Sim,
    /// Kommt vom Addon (LVar, PMDG-SDK, Plugin).
    Addon,
    /// Rechnet AeroACARS selbst.
    Berechnet,
    /// Zusatzwert, nur gelesen solange der Monitor offen ist.
    Zusatz,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Art {
    Zahl,
    Schalter,
    Text,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Gruppe {
    Flug,
    Aero,
    Wind,
    Triebwerke,
    Sprit,
    Fahrwerk,
    Steuerung,
    Autopilot,
    Anflug,
    Systeme,
    Licht,
    Funk,
    Umgebung,
    Sim,
    Flugzeug,
}

/// MSFS-Quelle eines Zusatzwerts: SimVar, angeforderte Einheit, Faktor.
///
/// SimVar und Einheit liest nur der Windows-Pfad (`msfs_zusatzfelder`);
/// auf dem Mac gelten sie sonst als tot.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub struct MsfsQuelle {
    pub simvar: &'static str,
    pub einheit: &'static str,
    pub faktor: f64,
}

/// X-Plane-Quelle eines Zusatzwerts: DataRef (mit `[i]`), Faktor.
#[derive(Debug, Clone, Copy)]
pub struct XpQuelle {
    pub dataref: &'static str,
    pub faktor: f64,
}

pub struct Kontext<'a> {
    pub s: &'a SimSnapshot,
    /// Zusatzwerte je Kanal-ID, Faktor schon angewendet.
    pub z: &'a HashMap<String, f64>,
}

impl Kontext<'_> {
    fn z(&self, id: &str) -> Option<f64> {
        self.z.get(id).copied().filter(|v| v.is_finite())
    }
}

type ZahlFn = fn(&Kontext) -> Option<f64>;
type TextFn = fn(&Kontext) -> Option<String>;

pub struct Kanal {
    pub id: &'static str,
    pub gruppe: Gruppe,
    pub einheit: &'static str,
    pub stellen: u8,
    pub quelle: Quelle,
    pub art: Art,
    /// Nur Rechengrundlage, nicht in der Liste zeigen (z. B. Tankinhalt in
    /// Gallonen, aus dem die kg-Kanaele entstehen).
    pub intern: bool,
    zahl: Option<ZahlFn>,
    text: Option<TextFn>,
    pub msfs: Option<MsfsQuelle>,
    pub xplane: Option<XpQuelle>,
}

const fn k(id: &'static str, gruppe: Gruppe, einheit: &'static str, stellen: u8) -> Kanal {
    Kanal {
        id,
        gruppe,
        einheit,
        stellen,
        quelle: Quelle::Sim,
        art: Art::Zahl,
        intern: false,
        zahl: None,
        text: None,
        msfs: None,
        xplane: None,
    }
}

impl Kanal {
    const fn zahl(mut self, f: ZahlFn) -> Self {
        self.zahl = Some(f);
        self
    }
    const fn text(mut self, f: TextFn) -> Self {
        self.text = Some(f);
        self.art = Art::Text;
        self
    }
    const fn schalter(mut self) -> Self {
        self.art = Art::Schalter;
        self
    }
    const fn q(mut self, q: Quelle) -> Self {
        self.quelle = q;
        self
    }
    const fn intern(mut self) -> Self {
        self.intern = true;
        self
    }
    const fn msfs(mut self, simvar: &'static str, einheit: &'static str, faktor: f64) -> Self {
        self.msfs = Some(MsfsQuelle {
            simvar,
            einheit,
            faktor,
        });
        self
    }
    const fn xp(mut self, dataref: &'static str, faktor: f64) -> Self {
        self.xplane = Some(XpQuelle { dataref, faktor });
        self
    }

    fn zahl_wert(&self, k: &Kontext) -> Option<f64> {
        self.zahl
            .and_then(|f| f(k))
            .or_else(|| k.z(self.id))
            .filter(|v| v.is_finite())
    }
}

fn f(v: f32) -> Option<f64> {
    Some(v as f64)
}
fn of(v: Option<f32>) -> Option<f64> {
    v.map(|x| x as f64)
}
fn b(v: bool) -> Option<f64> {
    Some(if v { 1.0 } else { 0.0 })
}
fn ob(v: Option<bool>) -> Option<f64> {
    v.map(|x| if x { 1.0 } else { 0.0 })
}

fn n1<const I: usize>(k: &Kontext) -> Option<f64> {
    if let Some(es) = &k.s.engine_signals {
        if let Some(v) = es.n1_pct.get(I) {
            return Some(*v);
        }
    }
    k.s.eng_n1_pct.as_ref().and_then(|v| v.get(I).copied())
}
fn ff<const I: usize>(k: &Kontext) -> Option<f64> {
    k.s.engine_signals
        .as_ref()
        .and_then(|es| es.fuel_flow_pph.get(I).copied())
        .map(|pph| pph / LB_JE_KG)
}
fn laeuft<const I: usize>(k: &Kontext) -> Option<f64> {
    let es = k.s.engine_signals.as_ref()?;
    let an = es.eng_combustion.get(I).copied().unwrap_or(false)
        || es.combustion_ex1.get(I).copied().unwrap_or(false)
        || es.general_combustion.get(I).copied().unwrap_or(false);
    b(an)
}
fn tank_pmdg<const I: usize>(k: &Kontext) -> Option<f64> {
    k.s.fuel_per_tank_kg
        .as_ref()
        .and_then(|v| v.get(I).copied())
}
/// MSFS-Tankinhalt in Gallonen × Gewicht je Gallone. Bei PMDG liegen die
/// Tanks schon in kg vor und gehen vor.
fn tank_msfs(k: &Kontext, gal_id: &str, pmdg_idx: usize) -> Option<f64> {
    if let Some(v) = k.s.fuel_per_tank_kg.as_ref().and_then(|v| v.get(pmdg_idx)) {
        return Some(*v);
    }
    let gal = k.z(gal_id)?;
    let lb_je_gal = k.z("sprit_lb_je_gal")?;
    Some(gal * lb_je_gal / LB_JE_KG)
}

/// Vorhaltewinkel aus der Koerpergeschwindigkeit (seitlich / laengs).
fn vorhalt(k: &Kontext) -> Option<f64> {
    let vx = k.s.velocity_body_x_fps? as f64;
    let vz = k.s.velocity_body_z_fps? as f64;
    if vz.abs() < 10.0 {
        return None;
    }
    Some(vx.atan2(vz).to_degrees())
}
/// Bahnneigungswinkel aus V/S und Grundgeschwindigkeit.
fn bahnwinkel(k: &Kontext) -> Option<f64> {
    let gs = k.s.groundspeed_kt as f64;
    if gs < 30.0 {
        return None;
    }
    let vs_kt = k.s.vertical_speed_fpm as f64 / 101.268;
    Some(vs_kt.atan2(gs).to_degrees())
}
/// Abweichung von der Standardatmosphaere.
fn isa_abweichung(k: &Kontext) -> Option<f64> {
    let oat = k.s.outside_air_temp_c? as f64;
    let hoehe = k.s.altitude_pressure_ft.unwrap_or(k.s.altitude_msl_ft);
    let isa = if hoehe < 36_089.0 {
        15.0 - 1.9812 * hoehe / 1000.0
    } else {
        -56.5
    };
    Some(oat - isa)
}
/// Energiehoehe: Hoehe plus Fahrt als Hoehe (TAS²/2g), in ft.
fn energiehoehe(k: &Kontext) -> Option<f64> {
    let tas_fps = k.s.true_airspeed_kt as f64 * 1.687_81;
    Some(k.s.altitude_msl_ft + tas_fps * tas_fps / (2.0 * FT_S2_JE_G))
}
fn ff_summe(k: &Kontext) -> Option<f64> {
    if let Some(v) = k.s.fuel_flow_kg_per_h {
        return Some(v as f64);
    }
    let summe: f64 = (0..4).filter_map(|i| k.z(XP_FF[i])).sum::<f64>();
    (summe > 0.0).then_some(summe)
}
const XP_FF: [&str; 4] = ["ff_1", "ff_2", "ff_3", "ff_4"];

fn fps_aus_periode(k: &Kontext) -> Option<f64> {
    let p = k.z("sim_bildperiode")?;
    (p > 0.0).then(|| 1.0 / p)
}

/// Der Kanalkatalog. Reihenfolge = Reihenfolge in der Werteliste.
///
/// ⚠ Nur anhaengen oder IDs neu vergeben, wenn auch die Texte in allen drei
/// Sprachen (`telemetrie.kanal.<id>`) mitgezogen werden — der
/// Vollstaendigkeitstest der Oberflaeche prueft das.
pub static KATALOG: &[Kanal] = &[
    // ---- Flugzustand ----
    k("hoehe_msl", Gruppe::Flug, "ft", 0).zahl(|k| Some(k.s.altitude_msl_ft)),
    k("hoehe_agl", Gruppe::Flug, "ft", 0).zahl(|k| Some(k.s.altitude_agl_ft)),
    k("hoehe_angezeigt", Gruppe::Flug, "ft", 0).zahl(|k| k.s.altitude_indicated_ft),
    k("hoehe_druck", Gruppe::Flug, "ft", 0)
        .zahl(|k| k.s.altitude_pressure_ft)
        .xp("sim/flightmodel2/position/pressure_altitude", 1.0),
    k("radarhoehe", Gruppe::Flug, "ft", 0)
        .q(Quelle::Zusatz)
        .msfs("RADIO HEIGHT", "feet", 1.0)
        .xp(
            "sim/cockpit2/gauges/indicators/radio_altimeter_height_ft_pilot",
            1.0,
        ),
    k("ias", Gruppe::Flug, "kt", 0).zahl(|k| f(k.s.indicated_airspeed_kt)),
    k("tas", Gruppe::Flug, "kt", 0).zahl(|k| f(k.s.true_airspeed_kt)),
    k("gs", Gruppe::Flug, "kt", 0).zahl(|k| f(k.s.groundspeed_kt)),
    k("mach", Gruppe::Flug, "", 3).zahl(|k| of(k.s.mach)),
    k("vs", Gruppe::Flug, "fpm", 0).zahl(|k| f(k.s.vertical_speed_fpm)),
    k("vs_roh", Gruppe::Flug, "fpm", 0).zahl(|k| of(k.s.vertical_speed_raw_fpm)),
    k("pitch", Gruppe::Flug, "°", 1).zahl(|k| f(k.s.pitch_deg)),
    k("bank", Gruppe::Flug, "°", 1).zahl(|k| f(k.s.bank_deg)),
    k("kurs_mw", Gruppe::Flug, "°", 0).zahl(|k| f(k.s.heading_deg_magnetic)),
    k("kurs_rw", Gruppe::Flug, "°", 0).zahl(|k| f(k.s.heading_deg_true)),
    k("vorhaltewinkel", Gruppe::Flug, "°", 1)
        .q(Quelle::Berechnet)
        .zahl(vorhalt),
    k("bahnwinkel", Gruppe::Flug, "°", 2)
        .q(Quelle::Berechnet)
        .zahl(bahnwinkel),
    k("energiehoehe", Gruppe::Flug, "ft", 0)
        .q(Quelle::Berechnet)
        .zahl(energiehoehe),
    // ---- Aerodynamik ----
    k("aoa", Gruppe::Aero, "°", 1)
        .q(Quelle::Zusatz)
        .msfs("INCIDENCE ALPHA", "degrees", 1.0)
        .xp("sim/flightmodel/position/alpha", 1.0),
    k("schiebewinkel", Gruppe::Aero, "°", 1)
        .q(Quelle::Zusatz)
        .msfs("INCIDENCE BETA", "degrees", 1.0)
        .xp("sim/flightmodel/position/beta", 1.0),
    k("aoa_abriss", Gruppe::Aero, "°", 1)
        .q(Quelle::Zusatz)
        .msfs("STALL ALPHA", "degrees", 1.0),
    k("g", Gruppe::Aero, "g", 2).zahl(|k| f(k.s.g_force)),
    k("g_laengs", Gruppe::Aero, "g", 2)
        .q(Quelle::Zusatz)
        .msfs(
            "ACCELERATION BODY Z",
            "feet per second squared",
            1.0 / FT_S2_JE_G,
        )
        .xp("sim/flightmodel/forces/g_axil", 1.0),
    k("g_seitlich", Gruppe::Aero, "g", 2)
        .q(Quelle::Zusatz)
        .msfs(
            "ACCELERATION BODY X",
            "feet per second squared",
            1.0 / FT_S2_JE_G,
        )
        .xp("sim/flightmodel/forces/g_side", 1.0),
    k("rate_rollen", Gruppe::Aero, "°/s", 1)
        .q(Quelle::Zusatz)
        .msfs("ROTATION VELOCITY BODY Z", "degrees per second", 1.0)
        .xp("sim/flightmodel/position/P", 1.0),
    k("rate_nicken", Gruppe::Aero, "°/s", 1)
        .q(Quelle::Zusatz)
        .msfs("ROTATION VELOCITY BODY X", "degrees per second", 1.0)
        .xp("sim/flightmodel/position/Q", 1.0),
    k("rate_gieren", Gruppe::Aero, "°/s", 1)
        .q(Quelle::Zusatz)
        .msfs("ROTATION VELOCITY BODY Y", "degrees per second", 1.0)
        .xp("sim/flightmodel/position/R", 1.0),
    k("kugel", Gruppe::Aero, "", 2).q(Quelle::Zusatz).msfs(
        "TURN COORDINATOR BALL",
        "position",
        1.0,
    ),
    k("vs0", Gruppe::Aero, "kt", 0)
        .q(Quelle::Zusatz)
        .msfs("DESIGN SPEED VS0", "knots", 1.0)
        .xp("sim/aircraft/view/acf_Vso", 1.0),
    k("vs1", Gruppe::Aero, "kt", 0)
        .q(Quelle::Zusatz)
        .msfs("DESIGN SPEED VS1", "knots", 1.0)
        .xp("sim/aircraft/view/acf_Vs", 1.0),
    k("vfe", Gruppe::Aero, "kt", 0)
        .q(Quelle::Zusatz)
        .msfs("FLAPS CURRENT SPEED LIMITATION", "knots", 1.0)
        .xp("sim/aircraft/view/acf_Vfe", 1.0),
    k("vmo", Gruppe::Aero, "kt", 0)
        .q(Quelle::Zusatz)
        .msfs("AIRSPEED BARBER POLE", "knots", 1.0)
        .xp("sim/aircraft/view/acf_Vne", 1.0),
    k("g_max", Gruppe::Aero, "g", 2)
        .q(Quelle::Zusatz)
        .msfs("MAX G FORCE", "gforce", 1.0),
    k("g_min", Gruppe::Aero, "g", 2)
        .q(Quelle::Zusatz)
        .msfs("MIN G FORCE", "gforce", 1.0),
    // ---- Wind ----
    k("wind_richtung", Gruppe::Wind, "°", 0).zahl(|k| of(k.s.wind_direction_deg)),
    k("wind_staerke", Gruppe::Wind, "kt", 0).zahl(|k| of(k.s.wind_speed_kt)),
    k("gegenwind", Gruppe::Wind, "kt", 0)
        .q(Quelle::Berechnet)
        .zahl(|k| k.s.aircraft_wind_z_kt.map(|z| -(z as f64))),
    k("seitenwind", Gruppe::Wind, "kt", 0)
        .q(Quelle::Berechnet)
        .zahl(|k| of(k.s.aircraft_wind_x_kt)),
    k("vertikalwind", Gruppe::Wind, "fpm", 0)
        .q(Quelle::Zusatz)
        .msfs("AMBIENT WIND Y", "feet per minute", 1.0)
        .xp("sim/weather/aircraft/wind_now_y_msc", 196.850_4),
    k("oat", Gruppe::Wind, "°C", 0).zahl(|k| of(k.s.outside_air_temp_c)),
    k("tat", Gruppe::Wind, "°C", 0)
        .zahl(|k| of(k.s.total_air_temp_c))
        .xp("sim/weather/aircraft/temperature_leadingedge_deg_c", 1.0),
    k("isa_abweichung", Gruppe::Wind, "°C", 0)
        .q(Quelle::Berechnet)
        .zahl(isa_abweichung),
    k("qnh", Gruppe::Wind, "hPa", 0).zahl(|k| of(k.s.qnh_hpa)),
    k("dichtehoehe", Gruppe::Wind, "ft", 0)
        .q(Quelle::Zusatz)
        .msfs("DENSITY ALTITUDE", "feet", 1.0),
    // ---- Triebwerke ----
    k("triebwerke_anzahl", Gruppe::Triebwerke, "", 0)
        .q(Quelle::Zusatz)
        .msfs("NUMBER OF ENGINES", "number", 1.0)
        .xp("sim/aircraft/engine/acf_num_engines", 1.0),
    k("triebwerke_laufen", Gruppe::Triebwerke, "", 0).zahl(|k| Some(k.s.engines_running as f64)),
    k("n1_1", Gruppe::Triebwerke, "%", 1)
        .zahl(n1::<0>)
        .xp("sim/cockpit2/engine/indicators/N1_percent[0]", 1.0),
    k("n1_2", Gruppe::Triebwerke, "%", 1)
        .zahl(n1::<1>)
        .xp("sim/cockpit2/engine/indicators/N1_percent[1]", 1.0),
    k("n1_3", Gruppe::Triebwerke, "%", 1)
        .zahl(n1::<2>)
        .xp("sim/cockpit2/engine/indicators/N1_percent[2]", 1.0),
    k("n1_4", Gruppe::Triebwerke, "%", 1)
        .zahl(n1::<3>)
        .xp("sim/cockpit2/engine/indicators/N1_percent[3]", 1.0),
    k("n2_1", Gruppe::Triebwerke, "%", 1)
        .q(Quelle::Zusatz)
        .msfs("TURB ENG N2:1", "percent", 1.0)
        .xp("sim/cockpit2/engine/indicators/N2_percent[0]", 1.0),
    k("n2_2", Gruppe::Triebwerke, "%", 1)
        .q(Quelle::Zusatz)
        .msfs("TURB ENG N2:2", "percent", 1.0)
        .xp("sim/cockpit2/engine/indicators/N2_percent[1]", 1.0),
    k("n2_3", Gruppe::Triebwerke, "%", 1)
        .q(Quelle::Zusatz)
        .msfs("TURB ENG N2:3", "percent", 1.0)
        .xp("sim/cockpit2/engine/indicators/N2_percent[2]", 1.0),
    k("n2_4", Gruppe::Triebwerke, "%", 1)
        .q(Quelle::Zusatz)
        .msfs("TURB ENG N2:4", "percent", 1.0)
        .xp("sim/cockpit2/engine/indicators/N2_percent[3]", 1.0),
    k("egt_1", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG EXHAUST GAS TEMPERATURE:1", "celsius", 1.0)
        .xp("sim/flightmodel2/engines/EGT_deg_cel[0]", 1.0),
    k("egt_2", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG EXHAUST GAS TEMPERATURE:2", "celsius", 1.0)
        .xp("sim/flightmodel2/engines/EGT_deg_cel[1]", 1.0),
    k("egt_3", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG EXHAUST GAS TEMPERATURE:3", "celsius", 1.0)
        .xp("sim/flightmodel2/engines/EGT_deg_cel[2]", 1.0),
    k("egt_4", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG EXHAUST GAS TEMPERATURE:4", "celsius", 1.0)
        .xp("sim/flightmodel2/engines/EGT_deg_cel[3]", 1.0),
    k("ff_1", Gruppe::Triebwerke, "kg/h", 0)
        .zahl(ff::<0>)
        .xp("sim/cockpit2/engine/indicators/fuel_flow_kg_sec[0]", 3600.0),
    k("ff_2", Gruppe::Triebwerke, "kg/h", 0)
        .zahl(ff::<1>)
        .xp("sim/cockpit2/engine/indicators/fuel_flow_kg_sec[1]", 3600.0),
    k("ff_3", Gruppe::Triebwerke, "kg/h", 0)
        .zahl(ff::<2>)
        .xp("sim/cockpit2/engine/indicators/fuel_flow_kg_sec[2]", 3600.0),
    k("ff_4", Gruppe::Triebwerke, "kg/h", 0)
        .zahl(ff::<3>)
        .xp("sim/cockpit2/engine/indicators/fuel_flow_kg_sec[3]", 3600.0),
    k("oeldruck_1", Gruppe::Triebwerke, "psi", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL PRESSURE:1", "psi", 1.0)
        .xp("sim/cockpit2/engine/indicators/oil_pressure_psi[0]", 1.0),
    k("oeldruck_2", Gruppe::Triebwerke, "psi", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL PRESSURE:2", "psi", 1.0)
        .xp("sim/cockpit2/engine/indicators/oil_pressure_psi[1]", 1.0),
    k("oeldruck_3", Gruppe::Triebwerke, "psi", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL PRESSURE:3", "psi", 1.0)
        .xp("sim/cockpit2/engine/indicators/oil_pressure_psi[2]", 1.0),
    k("oeldruck_4", Gruppe::Triebwerke, "psi", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL PRESSURE:4", "psi", 1.0)
        .xp("sim/cockpit2/engine/indicators/oil_pressure_psi[3]", 1.0),
    k("oeltemp_1", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL TEMPERATURE:1", "celsius", 1.0),
    k("oeltemp_2", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL TEMPERATURE:2", "celsius", 1.0),
    k("oeltemp_3", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL TEMPERATURE:3", "celsius", 1.0),
    k("oeltemp_4", Gruppe::Triebwerke, "°C", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG OIL TEMPERATURE:4", "celsius", 1.0),
    k("schubhebel_1", Gruppe::Triebwerke, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG THROTTLE LEVER POSITION:1", "percent", 1.0)
        .xp("sim/cockpit2/engine/actuators/throttle_ratio[0]", 100.0),
    k("schubhebel_2", Gruppe::Triebwerke, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG THROTTLE LEVER POSITION:2", "percent", 1.0)
        .xp("sim/cockpit2/engine/actuators/throttle_ratio[1]", 100.0),
    k("schubhebel_3", Gruppe::Triebwerke, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG THROTTLE LEVER POSITION:3", "percent", 1.0)
        .xp("sim/cockpit2/engine/actuators/throttle_ratio[2]", 100.0),
    k("schubhebel_4", Gruppe::Triebwerke, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("GENERAL ENG THROTTLE LEVER POSITION:4", "percent", 1.0)
        .xp("sim/cockpit2/engine/actuators/throttle_ratio[3]", 100.0),
    k("umkehr_1", Gruppe::Triebwerke, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("TURB ENG REVERSE NOZZLE PERCENT:1", "percent", 1.0)
        .xp(
            "sim/flightmodel2/engines/thrust_reverser_deploy_ratio[0]",
            100.0,
        ),
    k("umkehr_2", Gruppe::Triebwerke, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("TURB ENG REVERSE NOZZLE PERCENT:2", "percent", 1.0)
        .xp(
            "sim/flightmodel2/engines/thrust_reverser_deploy_ratio[1]",
            100.0,
        ),
    k("laeuft_1", Gruppe::Triebwerke, "", 0)
        .schalter()
        .zahl(laeuft::<0>),
    k("laeuft_2", Gruppe::Triebwerke, "", 0)
        .schalter()
        .zahl(laeuft::<1>),
    k("laeuft_3", Gruppe::Triebwerke, "", 0)
        .schalter()
        .zahl(laeuft::<2>),
    k("laeuft_4", Gruppe::Triebwerke, "", 0)
        .schalter()
        .zahl(laeuft::<3>),
    k("umkehrschub", Gruppe::Triebwerke, "", 0)
        .q(Quelle::Addon)
        .schalter()
        .zahl(|k| ob(k.s.reverser_deployed)),
    k("apu_drehzahl", Gruppe::Triebwerke, "%", 0).zahl(|k| of(k.s.apu_pct_rpm)),
    // ---- Sprit und Gewicht ----
    k("sprit_gesamt", Gruppe::Sprit, "kg", 0).zahl(|k| f(k.s.fuel_total_kg)),
    k("sprit_verbraucht", Gruppe::Sprit, "kg", 0).zahl(|k| f(k.s.fuel_used_kg)),
    k("ff_gesamt", Gruppe::Sprit, "kg/h", 0).zahl(ff_summe),
    k("tank_links", Gruppe::Sprit, "kg", 0)
        .q(Quelle::Berechnet)
        .zahl(|k| tank_msfs(k, "tank_links_gal", 0)),
    k("tank_mitte", Gruppe::Sprit, "kg", 0)
        .q(Quelle::Berechnet)
        .zahl(|k| tank_msfs(k, "tank_mitte_gal", 1)),
    k("tank_rechts", Gruppe::Sprit, "kg", 0)
        .q(Quelle::Berechnet)
        .zahl(|k| tank_msfs(k, "tank_rechts_gal", 2)),
    k("tank_4", Gruppe::Sprit, "kg", 0)
        .q(Quelle::Addon)
        .zahl(tank_pmdg::<3>),
    k("tank_links_gal", Gruppe::Sprit, "gal", 0).intern().msfs(
        "FUEL TANK LEFT MAIN QUANTITY",
        "gallons",
        1.0,
    ),
    k("tank_mitte_gal", Gruppe::Sprit, "gal", 0).intern().msfs(
        "FUEL TANK CENTER QUANTITY",
        "gallons",
        1.0,
    ),
    k("tank_rechts_gal", Gruppe::Sprit, "gal", 0).intern().msfs(
        "FUEL TANK RIGHT MAIN QUANTITY",
        "gallons",
        1.0,
    ),
    k("sprit_lb_je_gal", Gruppe::Sprit, "lb", 2).intern().msfs(
        "FUEL WEIGHT PER GALLON",
        "pounds",
        1.0,
    ),
    k("xp_tank_1", Gruppe::Sprit, "kg", 0)
        .q(Quelle::Zusatz)
        .xp("sim/flightmodel/weight/m_fuel[0]", 1.0),
    k("xp_tank_2", Gruppe::Sprit, "kg", 0)
        .q(Quelle::Zusatz)
        .xp("sim/flightmodel/weight/m_fuel[1]", 1.0),
    k("xp_tank_3", Gruppe::Sprit, "kg", 0)
        .q(Quelle::Zusatz)
        .xp("sim/flightmodel/weight/m_fuel[2]", 1.0),
    k("gewicht", Gruppe::Sprit, "kg", 0).zahl(|k| of(k.s.total_weight_kg)),
    k("zfw", Gruppe::Sprit, "kg", 0).zahl(|k| of(k.s.zfw_kg)),
    k("zuladung", Gruppe::Sprit, "kg", 0).zahl(|k| of(k.s.payload_kg)),
    k("schwerpunkt", Gruppe::Sprit, "% MAC", 1)
        .q(Quelle::Zusatz)
        .msfs("CG PERCENT", "percent", 1.0)
        .xp("sim/flightmodel2/misc/cg_offset_z_mac", 1.0),
    // ---- Fahrwerk und Bremsen ----
    k("fahrwerk", Gruppe::Fahrwerk, "%", 0).zahl(|k| f(k.s.gear_position * 100.0)),
    k("fahrwerkshebel", Gruppe::Fahrwerk, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("GEAR HANDLE POSITION", "bool", 1.0)
        .xp("sim/cockpit2/controls/gear_handle_down", 1.0),
    k("am_boden", Gruppe::Fahrwerk, "", 0)
        .schalter()
        .zahl(|k| b(k.s.on_ground)),
    k("boden_bug", Gruppe::Fahrwerk, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("GEAR IS ON GROUND:0", "bool", 1.0)
        .xp("sim/flightmodel2/gear/on_ground[0]", 1.0),
    k("boden_links", Gruppe::Fahrwerk, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("GEAR IS ON GROUND:1", "bool", 1.0)
        .xp("sim/flightmodel2/gear/on_ground[1]", 1.0),
    k("boden_rechts", Gruppe::Fahrwerk, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("GEAR IS ON GROUND:2", "bool", 1.0)
        .xp("sim/flightmodel2/gear/on_ground[2]", 1.0),
    k("einfederung_bug", Gruppe::Fahrwerk, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("CONTACT POINT COMPRESSION:0", "position", 100.0),
    k("einfederung_links", Gruppe::Fahrwerk, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("CONTACT POINT COMPRESSION:1", "position", 100.0),
    k("einfederung_rechts", Gruppe::Fahrwerk, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("CONTACT POINT COMPRESSION:2", "position", 100.0),
    k("fahrwerk_kraft", Gruppe::Fahrwerk, "kN", 1)
        .zahl(|k| k.s.gear_normal_force_n.map(|n| n as f64 / 1000.0)),
    k("bremse_links", Gruppe::Fahrwerk, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("BRAKE LEFT POSITION", "position", 100.0)
        .xp("sim/cockpit2/controls/left_brake_ratio", 100.0),
    k("bremse_rechts", Gruppe::Fahrwerk, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("BRAKE RIGHT POSITION", "position", 100.0)
        .xp("sim/cockpit2/controls/right_brake_ratio", 100.0),
    k("parkbremse", Gruppe::Fahrwerk, "", 0)
        .schalter()
        .zahl(|k| b(k.s.parking_brake)),
    k("autobrake", Gruppe::Fahrwerk, "", 0).text(|k| k.s.autobrake.clone()),
    k("untergrund", Gruppe::Fahrwerk, "", 0)
        .q(Quelle::Zusatz)
        .msfs("SURFACE TYPE", "enum", 1.0),
    k("auf_bahn", Gruppe::Fahrwerk, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("ON ANY RUNWAY", "bool", 1.0),
    // ---- Steuerung ----
    k("klappen", Gruppe::Steuerung, "%", 0).zahl(|k| f(k.s.flaps_position * 100.0)),
    k("klappen_stufe", Gruppe::Steuerung, "", 0).zahl(|k| k.s.flap_handle_index.map(|v| v as f64)),
    k("spoiler", Gruppe::Steuerung, "%", 0)
        .zahl(|k| of(k.s.spoilers_handle_position).map(|v| v * 100.0)),
    k("spoiler_armed", Gruppe::Steuerung, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.spoilers_armed)),
    k("bodenspoiler", Gruppe::Steuerung, "", 0)
        .q(Quelle::Addon)
        .schalter()
        .zahl(|k| ob(k.s.ground_spoilers_active)),
    k("hoehenruder", Gruppe::Steuerung, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("ELEVATOR POSITION", "position", 100.0)
        .xp("sim/cockpit2/controls/yoke_pitch_ratio", 100.0),
    k("querruder", Gruppe::Steuerung, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("AILERON POSITION", "position", 100.0)
        .xp("sim/cockpit2/controls/yoke_roll_ratio", 100.0),
    k("seitenruder", Gruppe::Steuerung, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("RUDDER POSITION", "position", 100.0)
        .xp("sim/cockpit2/controls/yoke_heading_ratio", 100.0),
    k("trimmung", Gruppe::Steuerung, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("ELEVATOR TRIM PCT", "percent", 1.0)
        .xp("sim/cockpit2/controls/elevator_trim", 100.0),
    // ---- Autopilot ----
    k("ap", Gruppe::Autopilot, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.autopilot_master)),
    k("athr", Gruppe::Autopilot, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.autothrottle_on)),
    k("fma_lateral", Gruppe::Autopilot, "", 0)
        .q(Quelle::Addon)
        .text(|k| k.s.fma_lateral_mode.clone()),
    k("fma_vertikal", Gruppe::Autopilot, "", 0)
        .q(Quelle::Addon)
        .text(|k| k.s.fma_vertical_mode.clone()),
    k("fma_schub", Gruppe::Autopilot, "", 0)
        .q(Quelle::Addon)
        .text(|k| k.s.fma_thrust_mode.clone()),
    k("soll_hoehe", Gruppe::Autopilot, "ft", 0)
        .zahl(|k| k.s.fcu_selected_altitude_ft.map(|v| v as f64))
        .msfs("AUTOPILOT ALTITUDE LOCK VAR", "feet", 1.0)
        .xp("sim/cockpit2/autopilot/altitude_dial_ft", 1.0),
    k("soll_fahrt", Gruppe::Autopilot, "kt", 0)
        .zahl(|k| k.s.fcu_selected_speed_kt.map(|v| v as f64))
        .msfs("AUTOPILOT AIRSPEED HOLD VAR", "knots", 1.0)
        .xp("sim/cockpit2/autopilot/airspeed_dial_kts_mach", 1.0),
    k("soll_vs", Gruppe::Autopilot, "fpm", 0)
        .zahl(|k| k.s.fcu_selected_vs_fpm.map(|v| v as f64))
        .msfs("AUTOPILOT VERTICAL HOLD VAR", "feet per minute", 1.0)
        .xp("sim/cockpit2/autopilot/vvi_dial_fpm", 1.0),
    k("soll_kurs", Gruppe::Autopilot, "°", 0)
        .zahl(|k| k.s.fcu_selected_heading_deg.map(|v| v as f64))
        .msfs("AUTOPILOT HEADING LOCK DIR", "degrees", 1.0)
        .xp("sim/cockpit2/autopilot/heading_dial_deg_mag_pilot", 1.0),
    k("v1", Gruppe::Autopilot, "kt", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.v1_kt),
    k("vr", Gruppe::Autopilot, "kt", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.vr_kt),
    k("v2", Gruppe::Autopilot, "kt", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.v2_kt),
    k("vapp", Gruppe::Autopilot, "kt", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.vapp_kt),
    k("vls", Gruppe::Autopilot, "kt", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.vls_kt),
    k("vref", Gruppe::Autopilot, "kt", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.vref_kt),
    k("flex", Gruppe::Autopilot, "°C", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.flex_temp_c),
    k("schubrast", Gruppe::Autopilot, "", 0)
        .q(Quelle::Addon)
        .text(|k| k.s.thrust_gate.clone()),
    // ---- Anflug und Navigation ----
    k("loc_ablage", Gruppe::Anflug, "dots", 2)
        .q(Quelle::Zusatz)
        .msfs("NAV CDI:1", "number", 2.0 / 127.0)
        .xp("sim/cockpit2/radios/indicators/nav1_hdef_dots_pilot", 1.0),
    k("gs_ablage", Gruppe::Anflug, "dots", 2)
        .q(Quelle::Zusatz)
        .msfs("NAV GSI:1", "number", 2.0 / 127.0)
        .xp("sim/cockpit2/radios/indicators/nav1_vdef_dots_pilot", 1.0),
    k("loc_empfang", Gruppe::Anflug, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("NAV HAS LOCALIZER:1", "bool", 1.0)
        .xp(
            "sim/cockpit2/radios/indicators/nav1_display_horizontal",
            1.0,
        ),
    k("gs_empfang", Gruppe::Anflug, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("NAV HAS GLIDE SLOPE:1", "bool", 1.0)
        .xp("sim/cockpit2/radios/indicators/nav1_display_vertical", 1.0),
    k("dme", Gruppe::Anflug, "NM", 1)
        .q(Quelle::Zusatz)
        .msfs("NAV DME:1", "nautical miles", 1.0)
        .xp("sim/cockpit2/radios/indicators/nav1_dme_distance_nm", 1.0),
    k("unter_gs", Gruppe::Anflug, "", 0)
        .q(Quelle::Addon)
        .schalter()
        .zahl(|k| ob(k.s.below_gs_alert)),
    k("minimum", Gruppe::Anflug, "ft", 0)
        .q(Quelle::Addon)
        .zahl(|k| k.s.minimums_baro_ft),
    // ---- Systeme ----
    k("kabinenhoehe", Gruppe::Systeme, "ft", 0)
        .q(Quelle::Zusatz)
        .msfs("PRESSURIZATION CABIN ALTITUDE", "feet", 1.0)
        .xp(
            "sim/cockpit2/pressurization/indicators/cabin_altitude_ft",
            1.0,
        ),
    k("kabine_vs", Gruppe::Systeme, "fpm", 0)
        .q(Quelle::Zusatz)
        .msfs("PRESSURIZATION CABIN ALTITUDE RATE", "feet per minute", 1.0)
        .xp("sim/cockpit2/pressurization/indicators/cabin_vvi_fpm", 1.0),
    k("differenzdruck", Gruppe::Systeme, "psi", 1)
        .q(Quelle::Zusatz)
        .msfs("PRESSURIZATION PRESSURE DIFFERENTIAL", "psi", 1.0)
        .xp(
            "sim/cockpit2/pressurization/indicators/pressure_diffential_psi",
            1.0,
        ),
    k("hydraulik", Gruppe::Systeme, "psi", 0)
        .q(Quelle::Zusatz)
        .msfs("HYDRAULIC PRESSURE:1", "psi", 1.0)
        .xp(
            "sim/cockpit2/hydraulics/indicators/hydraulic_pressure_1",
            1.0,
        ),
    k("batteriespannung", Gruppe::Systeme, "V", 1)
        .q(Quelle::Zusatz)
        .msfs("ELECTRICAL BATTERY VOLTAGE:1", "volts", 1.0)
        .xp(
            "sim/cockpit2/electrical/battery_voltage_actual_volts[0]",
            1.0,
        ),
    k("aussenstrom", Gruppe::Systeme, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("EXTERNAL POWER ON:1", "bool", 1.0)
        .xp("sim/cockpit2/electrical/GPU_generator_on", 1.0),
    k("batterie", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.battery_master)),
    k("avionik", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.avionics_master)),
    k("apu", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.apu_switch)),
    k("pitotheizung", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.pitot_heat)),
    k("enteisung_tw", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.engine_anti_ice)),
    k("enteisung_fl", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.wing_anti_ice)),
    k("vereisung", Gruppe::Systeme, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("STRUCTURAL ICE PCT", "percent", 1.0),
    // X-Plane: `frm_ice` ist laut X-Plane die LINKE Tragflaeche, nicht die
    // ganze Zelle — eigener Kanal, damit die Beschriftung stimmt.
    k("vereisung_fl_links", Gruppe::Systeme, "%", 0)
        .q(Quelle::Zusatz)
        .xp("sim/flightmodel/failures/frm_ice", 100.0),
    k("pitot_vereisung", Gruppe::Systeme, "%", 0)
        .q(Quelle::Zusatz)
        .msfs("PITOT ICE PCT", "percent", 1.0)
        .xp("sim/flightmodel/failures/pitot_ice", 100.0),
    k("master_caution", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.master_caution))
        .xp("sim/cockpit2/annunciators/master_caution", 1.0),
    k("master_warning", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.master_warning))
        .xp("sim/cockpit2/annunciators/master_warning", 1.0),
    k("ueberziehwarnung", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| b(k.s.stall_warning)),
    k("ueberdrehzahl", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| b(k.s.overspeed_warning)),
    k("anschnallzeichen", Gruppe::Systeme, "", 0)
        .schalter()
        .zahl(|k| k.s.seatbelts_sign.map(|v| v as f64)),
    // ---- Licht ----
    k("licht_lande", Gruppe::Licht, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.light_landing)),
    k("licht_beacon", Gruppe::Licht, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.light_beacon)),
    k("licht_strobe", Gruppe::Licht, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.light_strobe)),
    k("licht_taxi", Gruppe::Licht, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.light_taxi)),
    k("licht_nav", Gruppe::Licht, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.light_nav)),
    k("licht_logo", Gruppe::Licht, "", 0)
        .schalter()
        .zahl(|k| ob(k.s.light_logo)),
    // ---- Funk ----
    k("squawk", Gruppe::Funk, "", 0)
        .zahl(|k| k.s.transponder_code.map(|v| v as f64))
        .xp("sim/cockpit2/radios/actuators/transponder_code", 1.0),
    k("transponder", Gruppe::Funk, "", 0).text(|k| k.s.xpdr_mode_label.clone()),
    k("com1", Gruppe::Funk, "MHz", 3).zahl(|k| of(k.s.com1_mhz)),
    k("com2", Gruppe::Funk, "MHz", 3).zahl(|k| of(k.s.com2_mhz)),
    k("nav1", Gruppe::Funk, "MHz", 2).zahl(|k| of(k.s.nav1_mhz)),
    k("nav2", Gruppe::Funk, "MHz", 2).zahl(|k| of(k.s.nav2_mhz)),
    // ---- Umgebung ----
    k("sicht", Gruppe::Umgebung, "m", 0)
        .q(Quelle::Zusatz)
        .msfs("AMBIENT VISIBILITY", "meters", 1.0)
        .xp("sim/weather/aircraft/visibility_reported_sm", 1609.344),
    k("niederschlag", Gruppe::Umgebung, "mm", 1)
        .q(Quelle::Zusatz)
        .msfs("AMBIENT PRECIP RATE", "millimeters of water", 1.0),
    k("in_wolke", Gruppe::Umgebung, "", 0)
        .q(Quelle::Zusatz)
        .schalter()
        .msfs("AMBIENT IN CLOUD", "bool", 1.0),
    k("bodenhoehe", Gruppe::Umgebung, "ft", 0)
        .q(Quelle::Zusatz)
        .msfs("GROUND ALTITUDE", "feet", 1.0),
    k("qnh_meer", Gruppe::Umgebung, "hPa", 0)
        .q(Quelle::Zusatz)
        .msfs("SEA LEVEL PRESSURE", "millibars", 1.0),
    // ---- Sim ----
    k("pause", Gruppe::Sim, "", 0)
        .schalter()
        .zahl(|k| b(k.s.paused)),
    k("sim_rate", Gruppe::Sim, "×", 2)
        .zahl(|k| f(k.s.simulation_rate))
        .xp("sim/time/sim_speed", 1.0),
    k("slew", Gruppe::Sim, "", 0)
        .schalter()
        .zahl(|k| b(k.s.slew_mode)),
    k("absturz", Gruppe::Sim, "", 0)
        .schalter()
        .zahl(|k| b(k.s.crashed))
        .xp("sim/flightmodel2/misc/has_crashed", 1.0),
    k("sim_bildperiode", Gruppe::Sim, "s", 3)
        .intern()
        .xp("sim/time/framerate_period", 1.0),
    k("bildrate", Gruppe::Sim, "fps", 0)
        .q(Quelle::Berechnet)
        .zahl(fps_aus_periode),
    k("phase_flugzeug", Gruppe::Sim, "", 0)
        .q(Quelle::Addon)
        .text(|k| k.s.flight_phase_aircraft.clone()),
    k("phase_acars", Gruppe::Sim, "", 0)
        .q(Quelle::Berechnet)
        .text(|k| k.s.shadow_phase.clone()),
    // ---- Flugzeug ----
    k("flugzeug", Gruppe::Flugzeug, "", 0).text(|k| k.s.aircraft_title.clone()),
    k("muster", Gruppe::Flugzeug, "", 0).text(|k| k.s.aircraft_icao.clone()),
    k("kennzeichen", Gruppe::Flugzeug, "", 0).text(|k| k.s.aircraft_registration.clone()),
];

/// Katalog-Eintrag, wie ihn die Oberflaeche bekommt.
#[derive(Debug, Clone, Serialize)]
pub struct KanalDto {
    pub id: &'static str,
    pub gruppe: Gruppe,
    pub einheit: &'static str,
    pub stellen: u8,
    pub quelle: Quelle,
    pub art: Art,
}

#[derive(Debug, Clone, Serialize)]
pub struct KatalogDto {
    /// Zahl- und Schalterkanaele, in der Reihenfolge von `Frame::z`.
    pub zahlen: Vec<KanalDto>,
    /// Textkanaele, in der Reihenfolge von `Frame::s`.
    pub texte: Vec<KanalDto>,
}

fn dto(k: &Kanal) -> KanalDto {
    KanalDto {
        id: k.id,
        gruppe: k.gruppe,
        einheit: k.einheit,
        stellen: k.stellen,
        quelle: k.quelle,
        art: k.art,
    }
}

fn zahl_kanaele() -> impl Iterator<Item = &'static Kanal> {
    KATALOG.iter().filter(|k| k.art != Art::Text && !k.intern)
}
fn text_kanaele() -> impl Iterator<Item = &'static Kanal> {
    KATALOG.iter().filter(|k| k.art == Art::Text && !k.intern)
}

pub fn katalog() -> KatalogDto {
    KatalogDto {
        zahlen: zahl_kanaele().map(dto).collect(),
        texte: text_kanaele().map(dto).collect(),
    }
}

/// Ein Messpunkt: Zeit in ms seit Epoche, Zahlen und Texte in
/// Katalog-Reihenfolge. `None` = dieser Wert fehlt gerade.
#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub t: i64,
    pub z: Vec<Option<f32>>,
    pub s: Vec<Option<String>>,
}

fn runden(v: f64, stellen: u8) -> f32 {
    // Eine Stelle mehr als angezeigt, damit Min/Max/Mittel nicht springen.
    let m = 10f64.powi(stellen as i32 + 1);
    ((v * m).round() / m) as f32
}

pub fn frame(s: &SimSnapshot, zusatz: &HashMap<String, f64>) -> Frame {
    let k = Kontext { s, z: zusatz };
    Frame {
        t: s.timestamp.timestamp_millis(),
        z: zahl_kanaele()
            .map(|kanal| kanal.zahl_wert(&k).map(|v| runden(v, kanal.stellen)))
            .collect(),
        s: text_kanaele()
            .map(|kanal| {
                kanal
                    .text
                    .and_then(|f| f(&k))
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
            })
            .collect(),
    }
}

/// Zusatzfelder fuer MSFS: (Kanal-ID, SimVar, Einheit). Nur unter Windows
/// aufgerufen; der Test `msfs_felder_sind_vollstaendig` haelt sie auch auf
/// dem Mac lebendig.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn msfs_zusatzfelder() -> Vec<(String, String, String)> {
    KATALOG
        .iter()
        .filter_map(|k| {
            k.msfs.map(|q| {
                (
                    k.id.to_string(),
                    q.simvar.to_string(),
                    q.einheit.to_string(),
                )
            })
        })
        .collect()
}

/// Zusatzfelder fuer X-Plane: (Kanal-ID, DataRef).
pub fn xplane_zusatzfelder() -> Vec<(String, String)> {
    KATALOG
        .iter()
        .filter_map(|k| k.xplane.map(|q| (k.id.to_string(), q.dataref.to_string())))
        .collect()
}

/// Rohwerte der Adapter mit dem Faktor des Kanals verrechnen.
pub fn zusatz_umrechnen(roh: Vec<(String, f64)>, xplane: bool) -> HashMap<String, f64> {
    let mut aus = HashMap::with_capacity(roh.len());
    for (id, wert) in roh {
        let Some(kanal) = KATALOG.iter().find(|k| k.id == id) else {
            continue;
        };
        let faktor = if xplane {
            kanal.xplane.map(|q| q.faktor)
        } else {
            kanal.msfs.map(|q| q.faktor)
        };
        if let Some(fk) = faktor {
            aus.insert(id, wert * fk);
        }
    }
    aus
}

/// Zustand des Monitors: Verlauf und ob gerade jemand zuschaut.
#[derive(Default)]
pub struct Monitor {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    verlauf: VecDeque<Frame>,
    /// Zuschauer (Fensterkennung) → gueltig bis. Je Fenster, damit das
    /// Schliessen des Tabs nicht den Strom des eigenen Fensters beendet.
    zuschauer: HashMap<String, Instant>,
    letzte_aufnahme: Option<Instant>,
}

impl Monitor {
    pub fn halten(&self, wer: &str, jetzt: Instant) {
        if let Ok(mut g) = self.inner.lock() {
            g.zuschauer.insert(wer.to_string(), jetzt + HALTEN_DAUER);
        }
    }
    pub fn beenden(&self, wer: &str) {
        if let Ok(mut g) = self.inner.lock() {
            g.zuschauer.remove(wer);
        }
    }
    /// Schaut irgendein Zuschauer mit diesem Praefix zu (alle Tablets)?
    pub fn aktiv_mit_praefix(&self, praefix: &str, jetzt: Instant) -> bool {
        self.inner
            .lock()
            .map(|g| {
                g.zuschauer
                    .iter()
                    .any(|(k, bis)| k.starts_with(praefix) && *bis > jetzt)
            })
            .unwrap_or(false)
    }
    /// Schaut dieser Zuschauer (Fenster oder „lan") gerade zu?
    pub fn aktiv_fuer(&self, wer: &str, jetzt: Instant) -> bool {
        self.inner
            .lock()
            .map(|g| g.zuschauer.get(wer).is_some_and(|bis| *bis > jetzt))
            .unwrap_or(false)
    }
    pub fn aktiv(&self, jetzt: Instant) -> bool {
        self.inner
            .lock()
            .map(|mut g| {
                g.zuschauer.retain(|_, bis| *bis > jetzt);
                !g.zuschauer.is_empty()
            })
            .unwrap_or(false)
    }
    /// Nimmt den Frame in den Verlauf auf, hoechstens mit [`VERLAUF_HZ`].
    pub fn aufnehmen(&self, frame: &Frame, jetzt: Instant) {
        let Ok(mut g) = self.inner.lock() else {
            return;
        };
        let abstand = Duration::from_millis(1000 / VERLAUF_HZ as u64);
        // Kleine Toleranz, damit ein 50-ms-Takt nicht jeden zweiten 100-ms-
        // Schritt knapp verfehlt.
        if g.letzte_aufnahme
            .is_some_and(|t| jetzt.duration_since(t) + Duration::from_millis(10) < abstand)
        {
            return;
        }
        // Derselbe Messpunkt nicht zweimal: Steht der Simulator (Pause,
        // eingefrorene Daten), liefert der Adapter denselben Snapshot mit
        // derselben Zeit — der Verlauf fuellte sich sonst mit Kopien, und die
        // Oberflaeche bekam beim Nachladen doppelte Zeitstempel (Codex).
        if g.verlauf.back().is_some_and(|l| l.t >= frame.t) {
            return;
        }
        g.letzte_aufnahme = Some(jetzt);
        if g.verlauf.len() >= VERLAUF_MAX {
            g.verlauf.pop_front();
        }
        g.verlauf.push_back(frame.clone());
    }
    pub fn verlauf(&self) -> Vec<Frame> {
        self.inner
            .lock()
            .map(|g| g.verlauf.iter().cloned().collect())
            .unwrap_or_default()
    }
    /// Bei Wechsel des Flugzeugs oder Simulators passt der alte Verlauf
    /// nicht mehr zu den neuen Werten.
    pub fn verlauf_leeren(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.verlauf.clear();
            g.letzte_aufnahme = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_core::EngineSignals;

    fn snap() -> SimSnapshot {
        SimSnapshot {
            altitude_msl_ft: 1200.0,
            indicated_airspeed_kt: 137.4,
            true_airspeed_kt: 140.0,
            groundspeed_kt: 130.0,
            vertical_speed_fpm: -700.0,
            ..SimSnapshot::default()
        }
    }

    fn wert(f: &Frame, id: &str) -> Option<f32> {
        let i = zahl_kanaele().position(|k| k.id == id).expect(id);
        f.z[i]
    }

    #[test]
    fn ids_sind_eindeutig() {
        let mut ids: Vec<_> = KATALOG.iter().map(|k| k.id).collect();
        ids.sort();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "doppelte Kanal-ID im Katalog");
    }

    #[test]
    fn frame_passt_zum_katalog() {
        let f = frame(&snap(), &HashMap::new());
        let kat = katalog();
        assert_eq!(f.z.len(), kat.zahlen.len());
        assert_eq!(f.s.len(), kat.texte.len());
        assert!(kat.zahlen.iter().all(|k| k.art != Art::Text));
        assert!(kat.texte.iter().all(|k| k.art == Art::Text));
    }

    #[test]
    fn interne_kanaele_bleiben_unsichtbar() {
        let kat = katalog();
        assert!(!kat.zahlen.iter().any(|k| k.id == "tank_links_gal"));
        assert!(!kat.zahlen.iter().any(|k| k.id == "sim_bildperiode"));
    }

    #[test]
    fn snapshot_werte_kommen_an() {
        let f = frame(&snap(), &HashMap::new());
        assert_eq!(wert(&f, "ias"), Some(137.4));
        assert_eq!(wert(&f, "vs"), Some(-700.0));
        assert_eq!(wert(&f, "aoa"), None, "ohne Zusatzwert kein AoA");
    }

    #[test]
    fn zusatz_mit_faktor() {
        // X-Plane liefert Schubhebel als Anteil 0..1, angezeigt wird %.
        let roh = vec![("schubhebel_1".to_string(), 0.42), ("aoa".to_string(), 5.5)];
        let z = zusatz_umrechnen(roh, true);
        let f = frame(&snap(), &z);
        assert_eq!(wert(&f, "schubhebel_1"), Some(42.0));
        assert_eq!(wert(&f, "aoa"), Some(5.5));
    }

    #[test]
    fn msfs_beschleunigung_wird_zu_g() {
        let roh = vec![("g_laengs".to_string(), -FT_S2_JE_G * 0.25)];
        let z = zusatz_umrechnen(roh, false);
        let f = frame(&snap(), &z);
        assert_eq!(wert(&f, "g_laengs"), Some(-0.25));
    }

    #[test]
    fn unbekannte_zusatz_ids_werden_ignoriert() {
        let z = zusatz_umrechnen(vec![("gibt_es_nicht".into(), 1.0)], false);
        assert!(z.is_empty());
    }

    #[test]
    fn snapshot_geht_vor_zusatz() {
        // FCU-Sollwert des Addons schlaegt den Standard-AP-Wert.
        let s = SimSnapshot {
            fcu_selected_altitude_ft: Some(5000),
            ..snap()
        };
        let z = zusatz_umrechnen(vec![("soll_hoehe".into(), 3000.0)], false);
        assert_eq!(wert(&frame(&s, &z), "soll_hoehe"), Some(5000.0));
        assert_eq!(wert(&frame(&snap(), &z), "soll_hoehe"), Some(3000.0));
    }

    #[test]
    fn triebwerke_einzeln_aus_den_rohsignalen() {
        let s = SimSnapshot {
            engine_signals: Some(EngineSignals {
                general_combustion: vec![true, false],
                combustion_ex1: vec![false, false],
                eng_combustion: vec![true, false],
                n1_pct: vec![45.5, 21.0],
                fuel_flow_pph: vec![2204.6226, 0.0],
            }),
            ..snap()
        };
        let f = frame(&s, &HashMap::new());
        assert_eq!(wert(&f, "n1_1"), Some(45.5));
        assert_eq!(wert(&f, "n1_2"), Some(21.0));
        assert_eq!(wert(&f, "ff_1"), Some(1000.0));
        assert_eq!(wert(&f, "laeuft_1"), Some(1.0));
        assert_eq!(wert(&f, "laeuft_2"), Some(0.0));
        assert_eq!(wert(&f, "n1_3"), None);
    }

    #[test]
    fn tank_aus_gallonen() {
        let z = zusatz_umrechnen(
            vec![
                ("tank_links_gal".into(), 1000.0),
                ("sprit_lb_je_gal".into(), 6.7),
            ],
            false,
        );
        let f = frame(&snap(), &z);
        let kg = wert(&f, "tank_links").unwrap();
        assert!((kg - 3039.0).abs() < 1.0, "{kg}");
        // PMDG-Tanks in kg gehen vor.
        let s = SimSnapshot {
            fuel_per_tank_kg: Some(vec![2000.0, 500.0, 2000.0]),
            ..snap()
        };
        assert_eq!(wert(&frame(&s, &z), "tank_links"), Some(2000.0));
    }

    #[test]
    fn gegenwind_ist_negativer_rueckenwind() {
        let s = SimSnapshot {
            aircraft_wind_z_kt: Some(-12.0),
            aircraft_wind_x_kt: Some(4.0),
            ..snap()
        };
        let f = frame(&s, &HashMap::new());
        assert_eq!(wert(&f, "gegenwind"), Some(12.0));
        assert_eq!(wert(&f, "seitenwind"), Some(4.0));
    }

    #[test]
    fn nicht_endliche_werte_fallen_raus() {
        let s = SimSnapshot {
            mach: Some(f32::NAN),
            ..snap()
        };
        assert_eq!(wert(&frame(&s, &HashMap::new()), "mach"), None);
    }

    #[test]
    fn texte_werden_getrimmt_und_leer_zu_none() {
        let s = SimSnapshot {
            fma_lateral_mode: Some(" LOC ".into()),
            fma_vertical_mode: Some("  ".into()),
            ..snap()
        };
        let f = frame(&s, &HashMap::new());
        let i = |id| text_kanaele().position(|k| k.id == id).unwrap();
        assert_eq!(f.s[i("fma_lateral")].as_deref(), Some("LOC"));
        assert_eq!(f.s[i("fma_vertikal")], None);
    }

    #[test]
    fn verlauf_nimmt_hoechstens_zehnmal_je_sekunde_auf() {
        let m = Monitor::default();
        let t0 = Instant::now();
        for i in 0..40u64 {
            let mut f = frame(&snap(), &HashMap::new());
            f.t = i as i64 * 50;
            m.aufnehmen(&f, t0 + Duration::from_millis(i * 50));
        }
        // 2 s bei 50-ms-Takt = 40 Ticks → 20 Aufnahmen.
        assert_eq!(m.verlauf().len(), 20);
    }

    #[test]
    fn derselbe_messpunkt_kommt_nur_einmal_in_den_verlauf() {
        // Stehender Simulator: gleiche Zeit, Takt laeuft weiter.
        let m = Monitor::default();
        let f = frame(&snap(), &HashMap::new());
        let t0 = Instant::now();
        for i in 0..40u64 {
            m.aufnehmen(&f, t0 + Duration::from_millis(i * 100));
        }
        assert_eq!(m.verlauf().len(), 1);
    }

    #[test]
    fn verlauf_ist_begrenzt() {
        let m = Monitor::default();
        let t0 = Instant::now();
        for i in 0..(VERLAUF_MAX as u64 + 50) {
            let mut f = frame(&snap(), &HashMap::new());
            f.t = i as i64 * 100;
            m.aufnehmen(&f, t0 + Duration::from_millis(i * 100));
        }
        assert_eq!(m.verlauf().len(), VERLAUF_MAX);
    }

    #[test]
    fn halten_laeuft_ab() {
        let m = Monitor::default();
        let t0 = Instant::now();
        assert!(!m.aktiv(t0));
        m.halten("main", t0);
        assert!(m.aktiv(t0 + Duration::from_secs(5)));
        assert!(!m.aktiv(t0 + HALTEN_DAUER + Duration::from_millis(1)));
        m.halten("main", t0);
        m.beenden("main");
        assert!(!m.aktiv(t0));
    }

    #[test]
    fn tablets_stoeren_sich_nicht() {
        let m = Monitor::default();
        let t0 = Instant::now();
        let a = lan_zuschauer(Some("ipad-1"));
        let b = lan_zuschauer(Some("ipad-2"));
        m.halten(&a, t0);
        m.halten(&b, t0);
        m.beenden(&a);
        assert!(m.aktiv_mit_praefix(LAN_ZUSCHAUER, t0), "ipad-2 schaut noch");
        m.beenden(&b);
        assert!(!m.aktiv_mit_praefix(LAN_ZUSCHAUER, t0));
        // Fenster zaehlen nicht als Tablet.
        m.halten("main", t0);
        assert!(!m.aktiv_mit_praefix(LAN_ZUSCHAUER, t0));
    }

    #[test]
    fn geraetekennung_wird_bereinigt() {
        assert_eq!(lan_zuschauer(None), "lan");
        assert_eq!(lan_zuschauer(Some("")), "lan");
        assert_eq!(lan_zuschauer(Some("ab-12")), "lan:ab-12");
        assert_eq!(lan_zuschauer(Some("a/b c\\d")), "lan:abcd");
        assert_eq!(lan_zuschauer(Some(&"x".repeat(100))).len(), 4 + 40);
    }

    #[test]
    fn zuschauer_einzeln_abfragbar() {
        let m = Monitor::default();
        let t0 = Instant::now();
        m.halten(LAN_ZUSCHAUER, t0);
        assert!(m.aktiv_fuer(LAN_ZUSCHAUER, t0));
        assert!(!m.aktiv_fuer("main", t0));
        assert!(!m.aktiv_fuer(LAN_ZUSCHAUER, t0 + HALTEN_DAUER + Duration::from_millis(1)));
    }

    #[test]
    fn tab_schliessen_beendet_nicht_das_eigene_fenster() {
        let m = Monitor::default();
        let t0 = Instant::now();
        m.halten("main", t0);
        m.halten("telemetrie", t0);
        m.beenden("main");
        assert!(m.aktiv(t0), "das eigene Fenster schaut noch zu");
        m.beenden("telemetrie");
        assert!(!m.aktiv(t0));
    }

    #[test]
    fn vorschau_katalog_der_oberflaeche_passt() {
        // Die Vorschau (`?vorschau=telemetrie`) und die Texttests der
        // Oberflaeche lesen diese Datei. Laeuft sie dem Katalog davon, fehlen
        // dort Kanaele — dann die Datei aus dem Katalog neu erzeugen.
        let datei: serde_json::Value = serde_json::from_str(include_str!(
            "../../src/components/telemetrie/vorschauKatalog.json"
        ))
        .expect("vorschauKatalog.json ist gueltiges JSON");
        let echt = serde_json::to_value(katalog()).expect("Katalog serialisierbar");
        assert_eq!(
            datei, echt,
            "vorschauKatalog.json passt nicht mehr zum Katalog"
        );
    }

    #[test]
    fn msfs_felder_sind_vollstaendig() {
        let felder = msfs_zusatzfelder();
        assert!(felder
            .iter()
            .any(|(id, sv, _)| id == "aoa" && sv == "INCIDENCE ALPHA"));
        // Jede SimVar hat eine Einheit, jede ID gibt es im Katalog.
        for (id, sv, einheit) in &felder {
            assert!(!einheit.is_empty(), "{sv} ohne Einheit");
            assert!(KATALOG.iter().any(|k| k.id == id));
        }
    }

    #[test]
    fn jede_quelle_hat_einen_weg_zum_wert() {
        // Ein Zusatzkanal ohne Snapshot-Funktion braucht mindestens eine
        // Simulator-Quelle, sonst bleibt er immer leer.
        for kanal in KATALOG {
            let hat = kanal.zahl.is_some()
                || kanal.text.is_some()
                || kanal.msfs.is_some()
                || kanal.xplane.is_some();
            assert!(hat, "Kanal {} hat keine Quelle", kanal.id);
        }
    }
}
