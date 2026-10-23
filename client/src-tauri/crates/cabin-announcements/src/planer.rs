//! Der Planer: bekommt jeden Tick den Flugzustand und meldet, welche
//! Ansagen jetzt fällig werden (Leitfaden Abschnitte 2, 2b, 3).
//!
//! Er spielt nichts ab und kennt keine Texte. Was er meldet, landet in der
//! [`Warteschlange`](crate::warteschlange::Warteschlange); die entscheidet,
//! was wann tatsächlich läuft und was verfällt.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sim_core::FlightPhase;

use crate::flugart::Flugart;
use crate::slot::Slot;

/// Rang einer Phase im Flugablauf, für „ab Phase X“ / „vor Phase X“.
/// Holding liegt zwischen Cruise und Descent.
pub fn rang(p: FlightPhase) -> u8 {
    use FlightPhase::*;
    match p {
        Preflight => 0,
        Boarding => 1,
        Pushback => 2,
        TaxiOut => 3,
        TakeoffRoll => 4,
        Takeoff => 5,
        Climb => 6,
        Cruise => 7,
        Holding => 8,
        Descent => 9,
        Approach => 10,
        Final => 11,
        Landing => 12,
        TaxiIn => 13,
        BlocksOn => 14,
        Arrived => 15,
        PirepSubmitted => 16,
    }
}

/// Der Flugzustand eines Ticks. Der Host füllt ihn aus dem SimSnapshot und
/// dem aktiven Flug.
#[derive(Debug, Clone, Default)]
pub struct Tick {
    /// Echtzeit in UTC-Epoch-Sekunden — nur für Abstände und Verzögerungen,
    /// nicht für Tageszeiten (die kommen aus dem Plan).
    pub jetzt: i64,
    pub phase: FlightPhase,
    pub hoehe_ft: f64,
    pub vs_fpm: f64,
    pub am_boden: bool,
    /// 0 = aus, 1 = AUTO, 2 = an.
    pub anschnallzeichen: Option<u8>,
    pub pushback_aktiv: bool,
    /// Restflugzeit in Minuten.
    pub rest_min: Option<f64>,
    /// Aktuelles Ziel (für Umleitungen).
    pub ziel_icao: Option<String>,
    /// Abweichung vom Plan-Off-Block in Minuten, + = später (aus dem
    /// Zeiten-Band). `None` ohne Plan.
    pub abweichung_off_block_min: Option<i32>,
    /// AeroACARS hat ein Durchstarten erkannt (eigene Erkennung des Hosts).
    /// Der Planer leitet es zusätzlich aus Final → Luftphase ab.
    pub durchgestartet: bool,
    /// Beacon an (Türen zu, Triebwerksstart); `None` = meldet das Flugzeug nicht.
    pub beacon: Option<bool>,
    /// Landelicht an; `None` = meldet das Flugzeug nicht.
    pub landelicht: Option<bool>,
    /// Fahrwerk ausgefahren; `None` = unbekannt.
    pub fahrwerk_unten: Option<bool>,
    /// Höhe über Grund (ft); `None` = unbekannt.
    pub hoehe_agl_ft: Option<f64>,
    /// Mindestens ein Triebwerk läuft; `None` = meldet das Flugzeug nicht.
    pub triebwerke_an: Option<bool>,
    /// Erwarteter Beginn des Sinkflugs (UTC-Epoch) aus der Planzeit am
    /// TOD-Fix und der aktuellen Position auf der Route; `None` = neben der
    /// Route oder ohne TOD-Fix.
    pub tod_erwartet: Option<i64>,
    /// Leergewicht mit Zuladung (ZFW) im Sim, kg; `None` = unbekannt.
    pub zfw_kg: Option<f64>,
    /// Geplanter ZFW aus dem OFP, kg; `None` = kein OFP.
    pub planned_zfw_kg: Option<f64>,
    /// Geschwindigkeit über Grund (kt); `None` = unbekannt (2.1.1, „rollt“).
    pub bodengeschwindigkeit_kt: Option<f64>,
}

/// Was der Planer vom Flug einmal zu Beginn wissen muss.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flugdaten {
    pub flugart: Option<Flugart>,
    pub geplante_flugzeit_min: Option<u32>,
    /// Top of Descent laut OFP als Abstand vom Abheben (s, Navlog
    /// `time_total` am TOD-Fix). Als Abstand, nicht als Uhrzeit: Ein
    /// verspäteter Start verschiebt den TOD mit, sonst käme die
    /// Sinkflug-Ansage direkt nach dem Abheben.
    pub tod_nach_abheben_s: Option<i64>,
    pub ziel_plan_icao: Option<String>,
    pub reiseflughoehe_ft: Option<u32>,
    /// Abheben (UTC-Epoch-Sekunden), falls schon bekannt — beim
    /// Wiedereinstieg aus `stats.takeoff_at`, damit 25/50/80 % vom echten
    /// Abheben zählen und nicht vom Neustart.
    pub abheben_epoch: Option<i64>,
}

/// Phase, ab der ein Slot nicht mehr ausgelöst wird (wie der `Verfall` in
/// [`Planer::tick`]); `None` = an keine Phase gebunden.
fn spaetestens_vor(slot: Slot) -> Option<FlightPhase> {
    use FlightPhase as P;
    use Slot as S;
    Some(match slot {
        // 2.1.1 (Thomas 19.10.: nichts verfällt vor dem Pushback): Kam die
        // Begrüßung bis „Türen zu“ nicht, kommt sie dort noch.
        S::Boarding | S::BoardingAbgeschlossen | S::BegruessungTuerenZu | S::Sicherheit => P::TakeoffRoll,
        // Kommt sie am Boden nicht mehr dran, nach dem Start (Thomas 18.10.2026).
        S::KapitaenBegruessung => P::Descent,
        S::AnschnallzeichenAus | S::Reiseflug25 | S::Reiseflug50 | S::Reiseflug80 => P::Descent,
        S::EndeService => P::Approach,
        S::KapitaenSinkflug | S::KabineSinkflug => P::Final,
        S::NachDerLandung => P::BlocksOn,
        S::Aussteigen => P::PirepSubmitted,
        _ => return None,
    })
}

/// Phase, ab der ein Slot ausgelöst wird. Beim Wiedereinstieg gilt alles
/// davor als erledigt.
fn ausloese_rang(slot: Slot) -> u8 {
    use FlightPhase as P;
    use Slot as S;
    rang(match slot {
        S::Boarding | S::BoardingAbgeschlossen | S::VerspaetungGate => P::Boarding,
        S::BegruessungTuerenZu | S::KommandoTuerenScharf | S::Sicherheit => P::Pushback,
        S::KapitaenBegruessung => P::TaxiOut,
        S::KommandoStart => P::TakeoffRoll,
        S::Chime10000Steigflug | S::AnschnallzeichenAus => P::Climb,
        S::Reiseflug25
        | S::Reiseflug50
        | S::Reiseflug80
        | S::Turbulenz
        | S::TurbulenzVorbei => P::Cruise,
        S::Warteschleife => P::Holding,
        // Spätestens beim Sinkflugbeginn: nach einem Neustart im Sinkflug
        // kommt sie noch (Codex 2.1.0); schon Gespieltes meldet der Host.
        S::EndeService
        | S::KapitaenSinkflug
        | S::KabineSinkflug
        | S::Chime10000Sinkflug
        | S::KommandoLandungVorbereiten
        | S::Umleitung
        | S::Durchstarten => P::Descent,
        S::KommandoPlaetzeLandung => P::Final,
        S::NachDerLandung => P::TaxiIn,
        S::KommandoTuerenEntschaerfen => P::BlocksOn,
        S::Aussteigen => P::Arrived,
    })
}

/// Wann eine fällige Ansage verfällt, wenn sie bis dahin nicht lief.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verfall {
    /// Sobald diese Phase (oder eine spätere) erreicht ist.
    AbPhase(FlightPhase),
    /// Zu diesem Zeitpunkt (UTC-Epoch-Sekunden).
    Um(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Faellig {
    pub slot: Slot,
    pub verfall: Verfall,
    /// Darf im Steigflug trotz Ruhephase spielen: „Anschnallzeichen aus“,
    /// wenn der Pilot den Schalter wirklich auf AUS stellt (v2.0.28).
    pub trotz_ruhephase: bool,
}

/// Warum der Planer eine Ansage nicht meldet, obwohl sie dran gewesen wäre —
/// die Seite zeigt sie in „Bisher“, statt dass sie still fehlt (beta.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Entfall {
    /// Zu nah am Sinkflug.
    ZuWenigZeit,
    /// Eine spätere Reiseflug-Ansage war schon dran (Reiseflug spät erkannt).
    Uebersprungen,
}

/// Stand der Sinkflug-Ansage des Kapitäns: Die Kabine folgt mit Abstand
/// zu ihrem wirklichen Ende, nicht zur Planung (Codex beta.6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum KapitaenSinkflug {
    #[default]
    Offen,
    /// Gemeldet, aber noch nicht gespielt (wartet auf Ton, Pause …).
    Gemeldet,
    /// Zu Ende gespielt um (UTC-Epoch).
    Gespielt(i64),
    /// Kommt nicht (stumm, abgebrochen, Fehler, verfallen).
    KommtNicht,
}

/// Stand des Service (Reiseflug 25): „Ende des Service“ folgt nur einem
/// wirklich gespielten (EWG 221: ohne Service kam beim Sinkflug „Der
/// Service ist beendet“ in die Schlange). Wie [`KapitaenSinkflug`] meldet
/// der Host das Ende.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Service {
    #[default]
    Offen,
    /// Gemeldet, das Ende steht noch aus (wartet auf Ton, Pause, spielt …).
    Gemeldet,
    /// Zu Ende gespielt.
    Gespielt,
    /// Kommt nicht (abgewählt, stumm, abgebrochen, Fehler, verfallen).
    KommtNicht,
}

/// Grenze für „Kurzstrecke“: darunter nur die Service-Ansage (Reiseflug 25),
/// keine Zwischenstände (Thomas 18.10.2026).
pub const KURZSTRECKE_MIN: u32 = 60;
/// Kurzstrecke: Die Service-Ansage braucht so viel Restflugzeit, sonst
/// entfällt sie. Der Kapitän sagt den Sinkflug bei 25 min Rest an; dazu kommen
/// die Ansage selbst und die Pause zwischen zwei Ansagen.
pub const KURZSTRECKE_SERVICE_REST_MIN: f64 = 28.0;
/// Angenommene Dauer von Sinkflug und Anflug ohne TOD aus dem OFP.
pub const SINKFLUG_MIN: f64 = 30.0;
/// Mindestabstand zwischen zwei Reiseflug-Ansagen.
pub const REISEFLUG_ABSTAND_S: i64 = 15 * 60;
/// Mindestabstand zwischen zwei Turbulenz-Ansagen.
pub const TURBULENZ_ABSTAND_S: i64 = 10 * 60;
/// Verspätungsansage ab so vielen Minuten …
pub const VERSPAETUNG_AB_MIN: i32 = 15;
/// … aber nicht darüber: das ist Fliegen außerhalb des Plans.
pub const VERSPAETUNG_BIS_MIN: i32 = 180;
/// Durchstart-Erklärung erst so viele Sekunden nach dem Durchstarten.
pub const DURCHSTART_VERZOEGERUNG_S: i64 = 150;
/// Warteschleifen-Ansage, sobald die Schleife so lange läuft.
pub const WARTESCHLEIFE_NACH_S: i64 = 120;
/// Die Kabine bereitet die Landung frühestens so lange nach der Sinkflug-
/// Ansage des Kapitäns vor (Thomas 18.10.2026: „sofort danach die Kabine,
/// das geht nicht“) …
pub const KABINE_NACH_KAPITAEN_MIN_S: i64 = 120;
/// Die Kabine bereitet die Landung erst unter dieser Höhe im Sinkflug vor
/// (Thomas 22.10.2026: kam bei 6 von 15 Flügen über 28.000 ft, weil sie
/// „spätestens 5 min nach dem Kapitän“ auch auf Reiseflughöhe fällig wurde).
pub const KABINE_SINKFLUG_UNTER_FT: f64 = 20_000.0;
/// Der Kapitän kündigt den Sinkflug so lange vor dem Top of Descent an
/// (Thomas 22.10.2026: bis zu 5,7 min davor war zu früh; „lieber später als
/// zu früh“) …
pub const KAPITAEN_VOR_TOD_S: i64 = 90;
/// … bzw. so viele Minuten vor der erwarteten Landung (vorher 25).
pub const KAPITAEN_VOR_LANDUNG_MIN: f64 = 20.0;
/// Ende des Bordverkaufs bzw. zweiter Service so lange vor dem Top of Descent
/// (Thomas 18.10.2026: „10, 15 Minuten vor Top of Descent“).
pub const REISEFLUG80_VOR_TOD_S: i64 = 12 * 60;
/// Der zweite Service (Modul-Art `zweiter_service`) beginnt so lange vor dem
/// Top of Descent (Thomas 19.10.2026: „so anderthalb Stunden, bevor wir mit
/// dem Sinkflug sind“) — das Ende des Bordverkaufs bleibt bei
/// [`REISEFLUG80_VOR_TOD_S`].
pub const ZWEITER_SERVICE_VOR_TOD_S: i64 = 90 * 60;
/// Zweiter Service nur, wenn danach noch so viel Restflugzeit bleibt (min):
/// 30 min Service vor dem Sinkflug plus Sinkflug und Anflug.
pub const ZWEITER_SERVICE_REST_MIN: f64 = 60.0;
/// „Plätze für die Landung“ im Anflug spätestens unter dieser Höhe über Grund,
/// wenn das Fahrwerk nicht vorher ausfährt.
pub const PLAETZE_LANDUNG_AGL_FT: f64 = 2_000.0;
/// Boarding-Begrüßung, sobald die Zuladung so weit über dem Bezugswert liegt
/// (erster ZFW-Wert der Phase Boarding) … (Thomas 19.10.2026: „Gewicht steigt
/// → 30 s warten → abspielen“; jeder Anstieg zählt, auch Fracht zuerst)
pub const BOARDING_ZULADUNG_KG: f64 = 200.0;
/// … und das ununterbrochen so lange (Sekunden).
pub const BOARDING_ZULADUNG_S: i64 = 30;
/// Boarding-Begrüßung auch, wenn der ZFW so nah am geplanten ZFW des OFP
/// liegt (der größere der beiden Werte) … (20.10.2026, Peter SVA 179: „Load
/// instant“ von der Standardladung des Add-ons herunter auf den Plan; an 430
/// Flügen gemessen: ohne Boarding-Musik 16 % statt 42 %, siehe
/// handoff/2026-10-20-boarding-erkennung.md)
pub const BOARDING_OFP_KG: f64 = 300.0;
pub const BOARDING_OFP_ANTEIL: f64 = 0.01;
/// … und das ununterbrochen so lange (Sekunden).
pub const BOARDING_OFP_S: i64 = 60;
/// „Rollt“ für „Plätze für den Start“ ab dieser Geschwindigkeit über Grund
/// (Thomas 19.10.2026, Entscheidung 3).
pub const ROLLT_AB_KT: f64 = 3.0;
/// Erste Service-Ansage außerhalb der Kurzstrecke so lange nach Erreichen der
/// Reiseflughöhe (v2.0.28, Peter: „Service auf Langstrecke zu spät“).
pub const SERVICE_NACH_REISEFLUG_S: i64 = 10 * 60;
/// So lange gilt der zuletzt aus der Route bekannte TOD ohne neuen Wert.
pub const TOD_FRISCH_S: i64 = 10 * 60;

#[derive(Debug, Default)]
pub struct Planer {
    flug: Flugdaten,
    gemeldet: HashSet<Slot>,
    letzte_phase: Option<FlightPhase>,
    letzte_hoehe_ft: Option<f64>,
    letztes_zeichen: Option<u8>,
    abheben_epoch: Option<i64>,
    letzte_reiseflug_ansage: Option<i64>,
    letzte_turbulenz: Option<i64>,
    turbulenz_aktiv: bool,
    warteschleife_seit: Option<i64>,
    warteschleife_gemeldet: bool,
    durchstart_faellig_um: Option<i64>,
    /// Ein Durchstarten ist erkannt und noch nicht durch einen neuen
    /// Endanflug abgeschlossen (verhindert Doppelmeldung mit dem Host-Signal).
    durchstart_laeuft: bool,
    /// Der Steigflug ist vorbei: Phase Cruise erreicht oder in der Luft
    /// direkt vom Steig- in den Sinkflug gewechselt. Einmal gesetzt, bleibt
    /// er (Step-Climb, Durchstarter). Kapitän und Kabine im Sinkflug und der
    /// Gong im Sinkflug hängen daran, nicht an der Phase Cruise selbst:
    /// Wer kurz unter der OFP-Höhe fliegt, bekommt nach phase_v2 erst nach
    /// 10 min Cruise (Lasse NOZ 607, 20.10.2026: 8 min FL250 statt FL330,
    /// Climb → Descent, Sinkflug-Ansagen fehlten). Service und Turbulenz
    /// bleiben an der echten Phase Cruise.
    steigflug_vorbei: bool,
    /// Zuletzt bekannter TOD aus der Route ([`Tick::tod_erwartet`]) und wann
    /// er kam. Er bleibt höchstens [`TOD_FRISCH_S`] stehen, wenn die Route
    /// verlassen wird (Direct), und wird mit einer Umleitung gelöscht.
    /// Die starre OFP-Uhrzeit (Abheben + TOD-Abstand) löst den Kapitän nicht
    /// mehr aus: Sie kam bis zu 35 min vor dem echten Sinkflug (20.10.2026,
    /// 5 von 19 Flügen), die Route 0,7–7 min. Ohne TOD entscheiden die
    /// Restzeit oder der Sinkflug selbst – lieber später als zu früh.
    tod_live: Option<i64>,
    tod_live_um: i64,
    /// Umgeleitet (in diesem Takt): Landezeit und TOD des Hosts beziehen sich
    /// noch auf die alte Route und zählen nicht (Kapitän mit dem Sinkflug).
    umgeleitet: bool,
    /// Der im aktuellen Takt gültige TOD (frisch), für Auslöser und Anzeige.
    tod_aktuell: Option<i64>,
    /// Höchste Höhe in der Luft (ft), für den nachgeholten Gong.
    hoechste_hoehe_ft: Option<f64>,
    /// Die Kabine folgt dem Kapitän erst später ([`KABINE_NACH_KAPITAEN_MIN_S`]).
    kapitaen_sinkflug: KapitaenSinkflug,
    /// Lief der Service? Daran hängt „Ende des Service“.
    service: Service,
    /// Nicht gemeldete Ansagen mit Grund, bis der Host sie abholt.
    entfallen: Vec<(Slot, Entfall)>,
    /// Wiedereinstieg im Reiseflug: Was beim ersten Reiseflug-Takt schon
    /// vorbei ist, lief vor dem Neustart (oder nie) — still erledigt, nicht
    /// als „entfallen“ (Pilotendurchlauf beta.6).
    reiseflug_still_nachziehen: bool,
    /// Zuletzt angesagtes Umleitungsziel: Ein neues Ziel (der Pilot ändert
    /// die Umleitung) wird noch einmal angesagt, dasselbe nicht.
    angesagte_umleitung: Option<String>,
    /// Vom Piloten abgewählte Ansagen: gelten beim Auslösen als erledigt,
    /// ohne gemeldet zu werden — die Abläufe dahinter (Service nach
    /// „Anschnallzeichen aus“, Kabine nach dem Kapitän) laufen weiter.
    abgewaehlt: HashSet<Slot>,
    tueren_zu: bool,
    letztes_beacon: Option<bool>,
    letztes_fahrwerk: Option<bool>,
    /// Landelicht war seit dem Verlassen des Stands einmal aus (v2.0.28).
    landelicht_war_aus: bool,
    /// Erster Reiseflug-Takt (UTC-Epoch) für den frühen Service.
    reiseflug_seit: Option<i64>,
    /// Bezugswert der Zuladung: erster ZFW-Wert der Phase Boarding (auch
    /// nach einem Neustart im Boarding neu genommen).
    zfw_start_kg: Option<f64>,
    /// Seit wann die Zuladung ununterbrochen über Bezug +
    /// [`BOARDING_ZULADUNG_KG`] liegt; fällt sie darunter, wieder `None`.
    zuladung_steigt_seit: Option<i64>,
    /// Seit wann der ZFW ununterbrochen am geplanten ZFW liegt
    /// ([`BOARDING_OFP_KG`], [`BOARDING_OFP_ANTEIL`]).
    am_ofp_seit: Option<i64>,
    /// Höchster ZFW seit dem Bezug; daran wird Entladen gemessen.
    zfw_hoch_kg: Option<f64>,
    /// Seit wann der ZFW ununterbrochen [`BOARDING_ZULADUNG_KG`] unter dem
    /// Höchstwert liegt (Entladen, erst nach [`BOARDING_ZULADUNG_S`] gültig).
    zuladung_sinkt_seit: Option<i64>,
    /// Die Abhebezeit ist echt (aus den Flugdaten oder im Steigflug
    /// beobachtet), nicht erst beim Wiedereinstieg im Reiseflug gesetzt.
    abheben_gesehen: bool,
    /// Art von „Reiseflug 80“ laut Modul: `Some(true)` = zweiter Service,
    /// `Some(false)` = Ende des Bordverkaufs, `None` = noch unbekannt.
    zweiter_service: Option<bool>,
    /// Wiedereinstieg nach dem Boarding (Neustart): Ein beobachtetes Abheben
    /// gibt es dann nicht.
    wiedereinstieg: bool,
}

impl Planer {
    pub fn neu(flug: Flugdaten) -> Self {
        let abheben_epoch = flug.abheben_epoch;
        Planer { flug, abheben_epoch, abheben_gesehen: abheben_epoch.is_some(), ..Default::default() }
    }

    /// Wiedereinstieg mitten im Flug (Neustart des Clients): alles, was vor
    /// der aktuellen Phase dran gewesen wäre, gilt als erledigt — sonst
    /// begrüßt die Kabine im Reiseflug noch einmal. Die %-Ansagen rechnen
    /// vom echten Abheben weiter (siehe [`Flugdaten::abheben_epoch`]).
    pub fn fortsetzen(flug: Flugdaten, phase: FlightPhase) -> Self {
        let mut p = Planer::neu(flug);
        let r = rang(phase);
        // Der Host legt den Planer auch beim normalen Flugstart mit
        // `fortsetzen` an (Codex 2.0.28). Bis zum Boarding gilt der erste
        // ZFW-Wert als Bezug: nach einem Neustart mitten im Boarding kommt
        // die Begrüßung beim nächsten Anstieg oder beim Beacon.
        p.wiedereinstieg = r > rang(FlightPhase::Boarding);
        for s in Slot::ALLE {
            // Umleitung und Durchstarten sind Ereignisse, kein Ablauf: Sie
            // koennen nach dem Wiedereinstieg noch kommen (typisch im
            // Sinkflug oder Anflug).
            let ereignis = matches!(s, Slot::Umleitung | Slot::Durchstarten);
            if s.einmalig() && !ereignis && ausloese_rang(s) < r {
                p.gemeldet.insert(s);
            }
        }
        if r >= rang(FlightPhase::Cruise) {
            p.steigflug_vorbei = true;
            p.gemeldet.insert(Slot::AnschnallzeichenAus);
            p.reiseflug_still_nachziehen = true;
        }
        p.letzte_phase = Some(phase);
        p
    }

    fn flugart(&self) -> Flugart {
        self.flug.flugart.unwrap_or(Flugart::Linie)
    }

    fn kurzstrecke(&self) -> bool {
        self.flug
            .geplante_flugzeit_min
            .is_none_or(|m| m < KURZSTRECKE_MIN)
    }

    /// Kommt dieser Reiseflug-Slot nach der Flugdauer überhaupt? Kurzstrecke
    /// mit bekannter Flugzeit: nur der Service (R25); unbekannte Flugzeit:
    /// keiner (ohne Planzeit gibt es keinen Anhalt).
    /// Restflugzeit (min): vom Host (Route/ETA), sonst aus Planzeit und
    /// Abheben — damit die Reserven auch ohne Navlog-Wegpunkte greifen
    /// (Codex-Review PR #159).
    pub fn restflug_min(&self, jetzt: i64, rest_min: Option<f64>) -> Option<f64> {
        rest_min.or_else(|| {
            let plan = f64::from(self.flug.geplante_flugzeit_min?);
            if !self.abheben_gesehen {
                return None;
            }
            let ab = self.abheben_epoch?;
            Some((plan - (jetzt - ab) as f64 / 60.0).max(0.0))
        })
    }

    /// Nach einem Neustart: Ansagen, die laut Verlauf schon liefen, gelten als
    /// erledigt (sonst käme z. B. „Ende des Service“ im Sinkflug doppelt).
    pub fn schon_gespielt(&mut self, slots: impl IntoIterator<Item = Slot>) {
        for s in slots {
            if s.einmalig() {
                self.gemeldet.insert(s);
            }
        }
    }

    /// Der Host meldet die Art von „Reiseflug 80“, sobald das Drehbuch sie
    /// liefert (v2.0.28): zweiter Service 90 min vor dem Sinkflug, Ende des
    /// Bordverkaufs 12 min davor.
    pub fn reiseflug80_art(&mut self, zweiter_service: Option<bool>) {
        if zweiter_service.is_some() {
            self.zweiter_service = zweiter_service;
        }
    }

    fn reiseflug_passt(&self, slot: Slot) -> bool {
        !self.kurzstrecke() || (slot == Slot::Reiseflug25 && self.flugzeit_bekannt())
    }

    /// Wann nach dem Abheben die Reiseflug-Ansagen dran sind (s), in
    /// Flugreihenfolge: 25 % und 50 % der Zeit bis zum Sinkflug, die dritte
    /// fest vor dem Top of Descent ([`REISEFLUG80_VOR_TOD_S`]). Bezugsgröße
    /// ist der TOD aus dem OFP, sonst Planzeit minus 30 min Sinkflug.
    /// Zeit vom Abheben bis zum Sinkflug (s): TOD aus dem OFP, sonst
    /// Planzeit minus [`SINKFLUG_MIN`].
    fn bis_sinkflug_s(&self) -> Option<f64> {
        let plan = self.flug.geplante_flugzeit_min?;
        Some(match self.flug.tod_nach_abheben_s {
            Some(tod) if tod > 0 => tod as f64,
            _ => (f64::from(plan) - SINKFLUG_MIN).max(1.0) * 60.0,
        })
    }

    fn reiseflug_schwellen(&self) -> Option<[(i64, Slot); 3]> {
        let bis_sinkflug_s = self.bis_sinkflug_s()?;
        let r25 = (0.25 * bis_sinkflug_s).round() as i64;
        let r50 = (0.50 * bis_sinkflug_s).round() as i64;
        let r80 = if self.zweiter_service == Some(true) {
            // Zweiter Service 90 min vor dem Sinkflug, mit dem üblichen
            // Abstand nach Reiseflug 50 (sonst gälte Reiseflug 50 als
            // übersprungen). Reicht die Zeit nicht, entfällt er (Reserve in
            // `melde_reiseflug`).
            (bis_sinkflug_s.round() as i64 - ZWEITER_SERVICE_VOR_TOD_S).max(r50 + REISEFLUG_ABSTAND_S)
        } else {
            (bis_sinkflug_s.round() as i64 - REISEFLUG80_VOR_TOD_S).max(r50)
        };
        Some([(r25, Slot::Reiseflug25), (r50, Slot::Reiseflug50), (r80, Slot::Reiseflug80)])
    }

    /// Ansagen, die seit dem letzten Aufruf entfallen sind (für „Bisher“).
    pub fn entfallene_abholen(&mut self) -> Vec<(Slot, Entfall)> {
        std::mem::take(&mut self.entfallen)
    }

    /// Der Host meldet das Ende der Sinkflug-Ansage des Kapitäns: `gespielt`
    /// = wirklich zu Ende gespielt; sonst (stumm, abgebrochen, Fehler,
    /// verfallen) richtet sich die Kabine nur nach Höhe und Anflug.
    pub fn kapitaen_sinkflug_ende(&mut self, gespielt: bool, jetzt: i64) {
        if matches!(self.kapitaen_sinkflug, KapitaenSinkflug::Offen | KapitaenSinkflug::Gemeldet) {
            self.kapitaen_sinkflug = if gespielt { KapitaenSinkflug::Gespielt(jetzt) } else { KapitaenSinkflug::KommtNicht };
        }
    }

    /// Der Host meldet das Ende des Service (Reiseflug 25), wie
    /// [`Self::kapitaen_sinkflug_ende`]: Nur `gespielt` gibt „Ende des
    /// Service“ frei. Nach einem Neustart meldet er so einen laut Verlauf
    /// gespielten Service.
    pub fn service_ende(&mut self, gespielt: bool) {
        if matches!(self.service, Service::Offen | Service::Gemeldet) {
            self.service = if gespielt { Service::Gespielt } else { Service::KommtNicht };
        }
    }

    fn entfaellt(&mut self, slot: Slot, grund: Entfall) {
        if !self.schon(slot) && self.flugart().erlaubt(slot) {
            self.gemeldet.insert(slot);
            self.entfallen.push((slot, grund));
        }
    }

    /// Ruhephase (Sterile Cockpit): nur Kommandos und Chimes. Start- und
    /// Landelauf immer; unter 10.000 ft im Steig- und Sinkflug nur, wenn
    /// der Reiseflug überhaupt darüber liegt (sonst wäre ein Flug in
    /// FL090 komplett stumm).
    pub fn ruhephase(&self, t: &Tick) -> bool {
        use FlightPhase::*;
        if matches!(t.phase, TakeoffRoll | Takeoff | Final | Landing) {
            return true;
        }
        let hoch = self.flug.reiseflughoehe_ft.is_none_or(|h| h >= 12_000);
        hoch && !t.am_boden
            && t.hoehe_ft < 10_000.0
            && matches!(t.phase, Climb | Descent | Approach)
    }

    /// Im Sinkflug noch über 10.000 ft, aber mit der aktuellen Sinkrate in
    /// höchstens [`crate::warteschlange::PAUSE_NACH_ANSAGE_S`] darunter — nur
    /// bei Flügen mit Ruhephase (Reiseflug ab 12.000 ft, wie [`Self::ruhephase`]).
    fn kurz_vor_ruhephase(&self, t: &Tick) -> bool {
        let hoch = self.flug.reiseflughoehe_ft.is_none_or(|h| h >= 12_000);
        hoch && !t.am_boden
            && rang(t.phase) >= rang(FlightPhase::Descent)
            && t.hoehe_ft >= 10_000.0
            && t.vs_fpm < 0.0
            && (t.hoehe_ft - 10_000.0) / -t.vs_fpm * 60.0 <= crate::warteschlange::PAUSE_NACH_ANSAGE_S as f64
    }

    fn melde(&mut self, aus: &mut Vec<Faellig>, slot: Slot, verfall: Verfall) {
        if slot.einmalig() && self.gemeldet.contains(&slot) {
            return;
        }
        if !self.flugart().erlaubt(slot) {
            return;
        }
        self.gemeldet.insert(slot);
        if self.abgewaehlt.contains(&slot) {
            if slot == Slot::KapitaenSinkflug {
                self.kapitaen_sinkflug = KapitaenSinkflug::KommtNicht;
            }
            if slot == Slot::Reiseflug25 {
                self.service = Service::KommtNicht;
            }
            return;
        }
        if slot == Slot::Reiseflug25 {
            self.service = Service::Gemeldet;
        }
        aus.push(Faellig { slot, verfall, trotz_ruhephase: false });
    }

    /// Ansagen, die der Pilot abgewählt hat (GSX, Cockpit 2 Ground, eigene
    /// Ansagen des Flugzeugs …). Sie kommen nicht und werden nicht angefragt.
    pub fn abwaehlen(&mut self, slots: impl IntoIterator<Item = Slot>) {
        self.abgewaehlt = slots.into_iter().collect();
    }

    /// TOD als Uhrzeit — erst bekannt, wenn das Flugzeug abgehoben hat.
    /// TOD aus der Position auf der Route, solange er frisch ist.
    fn tod(&self) -> Option<i64> {
        self.tod_aktuell
    }

    fn schon(&self, slot: Slot) -> bool {
        self.gemeldet.contains(&slot)
    }

    /// Ist dieser Slot auf diesem Flug schon ausgelöst (oder übersprungen)?
    pub fn erledigt(&self, slot: Slot) -> bool {
        self.schon(slot)
    }

    /// „Türen zu“ ist erkannt (Beacon, Pushback oder Rollen) — dort endet
    /// die Boarding-Musik (2.1.1 G).
    pub fn tueren_zu(&self) -> bool {
        self.tueren_zu
    }

    /// Für die Anzeige „Als Nächstes“: kommt dieser Slot auf diesem Flug
    /// noch (erlaubt und noch nicht ausgelöst oder übersprungen)?
    /// `phase` ist die aktuelle Flugphase: Was nur bis zu einer Phase
    /// ausgelöst wird, kommt danach nicht mehr — auch dann nicht, wenn die
    /// Phasenerkennung die auslösende Phase übersprungen hat (Codex-QS).
    pub fn kommt_noch(&self, slot: Slot, phase: FlightPhase) -> bool {
        let reiseflug = matches!(slot, Slot::Reiseflug25 | Slot::Reiseflug50 | Slot::Reiseflug80);
        self.flugart().erlaubt(slot)
            && !self.abgewaehlt.contains(&slot)
            && !self.schon(slot)
            && (!reiseflug || self.reiseflug_passt(slot))
            // Reiseflug unter 10.000 ft: Der Gong kommt nie.
            && !(matches!(slot, Slot::Chime10000Steigflug | Slot::Chime10000Sinkflug)
                && self.flug.reiseflughoehe_ft.is_some_and(|h| h < 10_000))
            && spaetestens_vor(slot).is_none_or(|grenze| rang(phase) < rang(grenze))
    }

    /// Ohne bekannte Flugzeit zählt der Flug als Kurzstrecke (ohne
    /// Reiseflug-Ansagen, auch ohne Service); die Seite nennt das getrennt.
    pub fn flugzeit_bekannt(&self) -> bool {
        self.flug.geplante_flugzeit_min.is_some()
    }

    pub fn ist_kurzstrecke(&self) -> bool {
        self.kurzstrecke()
    }

    /// Voraussichtliche Uhrzeit (UTC-Epoch-Sekunden) einer noch kommenden
    /// Ansage — nur, wo es eine Quelle gibt: Reiseflug 25/50/80 aus Abheben
    /// und Zeit bis zum Sinkflug (wie [`Self::tick`], mit Mindestabstand),
    /// Sinkflug aus TOD bzw. erwarteter Landung. Boden, Landung und Ereignisse
    /// hängen an Phasen: `None`, keine erfundene Uhrzeit.
    pub fn zeitpunkt(&self, slot: Slot, landung_erwartet: Option<i64>) -> Option<i64> {
        if self.schon(slot) {
            return None;
        }
        match slot {
            Slot::Reiseflug25 | Slot::Reiseflug50 | Slot::Reiseflug80 => {
                // Kurzstrecke: Der Service hängt am Anschnallzeichen, nicht an
                // einer Uhrzeit (keine erfundene Zeit).
                if self.kurzstrecke() {
                    return None;
                }
                let ab = self.abheben_epoch?;
                let mut vorige = self.letzte_reiseflug_ansage;
                for (nach_s, s) in self.reiseflug_schwellen()? {
                    if self.schon(s) {
                        continue;
                    }
                    let mut t = ab + nach_s;
                    if s == Slot::Reiseflug25 && self.schon(Slot::AnschnallzeichenAus) {
                        if let Some(rs) = self.reiseflug_seit {
                            t = t.min(rs + SERVICE_NACH_REISEFLUG_S);
                        }
                    }
                    if let Some(v) = vorige {
                        t = t.max(v + REISEFLUG_ABSTAND_S);
                    }
                    if s == slot {
                        return Some(t);
                    }
                    vorige = Some(t);
                }
                None
            }
            Slot::EndeService => {
                // Hängt am Service (2.1.1 C): solange er nicht durch ist,
                // keine Uhrzeit, die nicht stimmen muss.
                if !self.schon(Slot::Reiseflug25) {
                    return None;
                }
                let b = self.bis_sinkflug_s()?;
                Some(self.abheben_epoch? + b.round() as i64 - REISEFLUG80_VOR_TOD_S)
            }
            // Kabine: nach dem Ende der Kapitänsansage (frühestens 2 min);
            // vorher keine eigene Uhrzeit — die Seite sagt „nach dem Kapitän“
            // (Thomas AIB437: beide zeigten dieselbe Zeit, die Kabine kam später).
            Slot::KabineSinkflug => match self.kapitaen_sinkflug {
                KapitaenSinkflug::Gespielt(k) => Some(k + KABINE_NACH_KAPITAEN_MIN_S),
                _ => None,
            },
            // Vor dem Ende des Steigflugs keine Uhrzeit: Die Ansage wartet
            // darauf, die Seite sagt „kurz vor dem Sinkflug“ (Lasse NOZ 607:
            // „Spielt ca. 17:55“ verstrich ohne Ansage).
            Slot::KapitaenSinkflug if !self.steigflug_vorbei => None,
            Slot::KapitaenSinkflug => self.kapitaen_sinkflug_prognose(landung_erwartet),
            _ => None,
        }
    }

    /// Voraussichtlicher Abspielzeitpunkt der Sinkflug-Ansage des Kapitäns
    /// (UTC-Epoch-Sekunden) aus denselben Quellen wie der Auslöser in
    /// [`Self::tick`]: TOD − [`KAPITAEN_VOR_TOD_S`] bzw. Landung −
    /// [`KAPITAEN_VOR_LANDUNG_MIN`], das frühere; nach einer Umleitung zählt
    /// die Landezeit des Hosts nicht. Anders als [`Self::zeitpunkt`] auch im
    /// Steigflug: Auf der Kurzstrecke wird der Ankunftstext gleich nach dem
    /// Abheben bestellt, und seine Restzeit soll zum Abspielen stimmen
    /// (23.10.2026, 37 Flüge: meist 15–20 min zu hoch). `None`, wenn die
    /// Ansage erledigt ist oder es keine Quelle gibt.
    pub fn kapitaen_sinkflug_prognose(&self, landung_erwartet: Option<i64>) -> Option<i64> {
        if self.schon(Slot::KapitaenSinkflug) {
            return None;
        }
        let aus_tod = self.tod().map(|t| t - KAPITAEN_VOR_TOD_S);
        let aus_landung = landung_erwartet.filter(|_| !self.umgeleitet).map(|l| l - (KAPITAEN_VOR_LANDUNG_MIN * 60.0) as i64);
        match (aus_tod, aus_landung) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Verarbeitet einen Tick und liefert die neu fälligen Ansagen in
    /// Flugreihenfolge.
    pub fn tick(&mut self, t: &Tick) -> Vec<Faellig> {
        use FlightPhase::*;
        let mut aus = Vec::new();
        if self.flugart() == Flugart::Aus {
            return aus;
        }
        let r = rang(t.phase);
        let vorher = self.letzte_phase;
        // Je Takt neu: Kehrt der Pilot zum Planziel zurück, gilt die Route wieder.
        self.umgeleitet = match (&self.flug.ziel_plan_icao, &t.ziel_icao) {
            (Some(plan), Some(ziel)) => !ziel.trim().is_empty() && !ziel.trim().eq_ignore_ascii_case(plan.trim()),
            _ => false,
        };
        if self.umgeleitet {
            self.tod_live = None;
        } else if let Some(tod) = t.tod_erwartet {
            self.tod_live = Some(tod);
            self.tod_live_um = t.jetzt;
        }
        self.tod_aktuell = self.tod_live.filter(|_| t.jetzt - self.tod_live_um <= TOD_FRISCH_S);
        let in_luft = !t.am_boden;

        // ---- Boden vor dem Start ----
        // Boarding-Begrüßung (Thomas 19.10.2026, Pflichtenheft 2.1.1 A):
        // „Gewicht steigt → 30 s warten → abspielen. Anschnallzeichen löst
        // nicht mehr aus. Wenn das nicht gelaufen ist, ans Beacon klemmen.“
        // Bezug ist der erste ZFW-Wert der Phase Boarding, ohne OFP- und
        // ohne 5-%-Bedingung; den Rückfall am Beacon macht „Türen zu“ unten.
        if t.phase == Boarding {
            if self.zfw_start_kg.is_none() {
                self.zfw_start_kg = t.zfw_kg;
            }
            // Sinkt die Zuladung unter den Höchstwert, wird entladen
            // (Standardladung des Add-ons weg, danach GSX): Das ist kein
            // Boarding. Hält das Absinken 30 s, ist der niedrigere Wert der
            // neue Bezug für den Anstieg; ein einzelner Ausreißer nach unten
            // ändert nichts (20.10.2026, Codex).
            let mut entlaedt = false;
            if let (Some(z), Some(s)) = (t.zfw_kg, self.zfw_start_kg) {
                let hoch = self.zfw_hoch_kg.map_or(s.max(z), |h| h.max(z));
                self.zfw_hoch_kg = Some(hoch);
                if z <= hoch - BOARDING_ZULADUNG_KG {
                    entlaedt = true;
                    let ab = *self.zuladung_sinkt_seit.get_or_insert(t.jetzt);
                    if t.jetzt - ab >= BOARDING_ZULADUNG_S {
                        self.zfw_start_kg = Some(z);
                        self.zfw_hoch_kg = Some(z);
                        self.zuladung_sinkt_seit = None;
                    }
                } else {
                    self.zuladung_sinkt_seit = None;
                }
            }
            let ueber = !entlaedt && t.zfw_kg.zip(self.zfw_start_kg).is_some_and(|(z, s)| z >= s + BOARDING_ZULADUNG_KG);
            if ueber {
                let ab = *self.zuladung_steigt_seit.get_or_insert(t.jetzt);
                if t.jetzt - ab >= BOARDING_ZULADUNG_S {
                    self.melde(&mut aus, Slot::Boarding, Verfall::AbPhase(TakeoffRoll));
                }
            } else {
                // Ausreißer: Die 30 s beginnen beim nächsten Anstieg von vorn.
                self.zuladung_steigt_seit = None;
            }
            // Zuladung passt zum OFP: Das Flugzeug ist beladen, auch wenn es
            // schon beim Start von AeroACARS voll war oder per Sofortladung
            // nach unten auf den Plan ging.
            let am_ofp = t
                .zfw_kg
                .zip(t.planned_zfw_kg)
                .is_some_and(|(z, p)| p > 0.0 && (z - p).abs() <= (p * BOARDING_OFP_ANTEIL).max(BOARDING_OFP_KG));
            if am_ofp {
                let ab = *self.am_ofp_seit.get_or_insert(t.jetzt);
                if t.jetzt - ab >= BOARDING_OFP_S {
                    self.melde(&mut aus, Slot::Boarding, Verfall::AbPhase(TakeoffRoll));
                }
            } else {
                self.am_ofp_seit = None;
            }
        }
        if let Some(abw) = t.abweichung_off_block_min {
            // Nur noch am Gate: ab dem Pushback trägt die Kapitäns-
            // Begrüßung die Verspätung mit.
            if t.am_boden
                && r < rang(Pushback)
                && !t.pushback_aktiv
                && (VERSPAETUNG_AB_MIN..=VERSPAETUNG_BIS_MIN).contains(&abw)
            {
                self.melde(&mut aus, Slot::VerspaetungGate, Verfall::AbPhase(TakeoffRoll));
            }
        }
        // Türen zu: Beacon geht an (in echt: Türen zu → Beacon → Triebwerks-
        // start), sonst Pushback oder Rollen. Cockpit-Schalter nur als
        // Flanke und mit Phase als Rückfall — je Flugzeug fehlen sie oder
        // sind anders belegt (Regel 17.10.; Michael BER181: ohne Pushback
        // kam der Block erst beim Rollen).
        let beacon_an = t.am_boden && t.beacon == Some(true) && self.letztes_beacon == Some(false);
        if (beacon_an || t.pushback_aktiv || r >= rang(Pushback)) && r < rang(TakeoffRoll) {
            self.tueren_zu = true;
        }
        if self.tueren_zu && r < rang(TakeoffRoll) {
            // Rückfall Beacon (2.1.1 A): Kam die Begrüßung bis hierher nicht,
            // kommt sie jetzt, vor allem anderen (Reihenfolge erzwingt die
            // Warteschlange). Abgewählt: `melde` hakt sie still ab.
            self.melde(&mut aus, Slot::Boarding, Verfall::AbPhase(TakeoffRoll));
            // „Boarding completed“ (2.1.1 E, Entscheidung 5): kurz vor
            // „Türen zu“, nach der Begrüßung, auf jedem Flug, ohne OFP.
            self.melde(&mut aus, Slot::BoardingAbgeschlossen, Verfall::AbPhase(TakeoffRoll));
            self.melde(&mut aus, Slot::KommandoTuerenScharf, Verfall::AbPhase(TakeoffRoll));
            self.melde(&mut aus, Slot::BegruessungTuerenZu, Verfall::AbPhase(TakeoffRoll));
            self.melde(&mut aus, Slot::Sicherheit, Verfall::AbPhase(TakeoffRoll));
        }
        // Kapitän beim Rollen; kommt er am Boden nicht mehr dran, spielt ihn
        // der Host nach dem Start über 10.000 ft (Verfall erst im Sinkflug).
        if r >= rang(TaxiOut) && r < rang(TakeoffRoll) {
            self.melde(&mut aus, Slot::KapitaenBegruessung, Verfall::AbPhase(Descent));
        }
        // „Plätze für den Start“: Landelicht an beim Rollen, sonst mit dem
        // Startlauf. Seit v2.0.28 der Zustand statt des Einschalt-Moments:
        // Geht das Licht schon an, bevor AeroACARS „Rollen“ erkennt, gab es
        // im TaxiOut nie eine Flanke und das Kommando kam erst im Startlauf
        // (Peter DLH454). „War einmal aus“ seit dem Verlassen des Stands,
        // damit ein vom Gate an brennendes Licht nicht sofort auslöst.
        // 2.1.1 B (Thomas 19.10.2026, Nachtrag: „darf aber nicht in Pushback
        // funktionieren, sondern nur in Taxi“): nur beim Rollen, und das
        // Flugzeug rollt (ab 3 kt). Ohne brauchbares Signal zählt die Phase.
        if t.landelicht == Some(false) && r >= rang(Pushback) {
            self.landelicht_war_aus = true;
        }
        let landelicht_an = t.landelicht == Some(true) && self.landelicht_war_aus;
        let rollt = t
            .bodengeschwindigkeit_kt
            .filter(|v| v.is_finite() && *v >= 0.0)
            .is_none_or(|v| v >= ROLLT_AB_KT);
        if (t.phase == TaxiOut && rollt && landelicht_an) || (rang(TakeoffRoll)..=rang(Climb)).contains(&r) {
            self.melde(&mut aus, Slot::KommandoStart, Verfall::AbPhase(Climb));
        }

        // ---- Steigflug / Reiseflug ----
        if in_luft && self.abheben_epoch.is_none() && r >= rang(Takeoff) {
            self.abheben_epoch = Some(t.jetzt);
            // Nur ein beobachteter Start taugt für die Restzeit aus der
            // Planzeit; nach einem Neustart im Reiseflug wäre „jetzt“ falsch.
            self.abheben_gesehen = !self.wiedereinstieg;
        }
        if in_luft {
            self.hoechste_hoehe_ft = Some(self.hoechste_hoehe_ft.map_or(t.hoehe_ft, |h| h.max(t.hoehe_ft)));
        }
        // Aus dem Steigflug direkt weiter (Sinkflug, auch gleich Anflug oder
        // Warteschleife): Der Steigflug ist vorbei.
        if in_luft && !self.steigflug_vorbei && vorher == Some(Climb) && r > rang(Climb) && r < rang(Landing) {
            self.steigflug_vorbei = true;
            // Erkennt phase_v2 den Sinkflug erst unter 10.000 ft, ist die
            // Flanke für den Gong schon vorbei: einmal nachholen.
            if r >= rang(Descent) && t.vs_fpm < 0.0 && t.hoehe_ft < 10_000.0 && self.hoechste_hoehe_ft.is_some_and(|h| h >= 10_000.0) {
                self.melde(&mut aus, Slot::Chime10000Sinkflug, Verfall::Um(t.jetzt + 30));
            }
        }
        if let Some(h0) = self.letzte_hoehe_ft {
            if in_luft && h0 < 10_000.0 && t.hoehe_ft >= 10_000.0 && t.vs_fpm > 0.0 {
                self.melde(&mut aus, Slot::Chime10000Steigflug, Verfall::Um(t.jetzt + 30));
            }
            if in_luft
                && self.steigflug_vorbei
                && h0 >= 10_000.0
                && t.hoehe_ft < 10_000.0
                && t.vs_fpm < 0.0
            {
                self.melde(&mut aus, Slot::Chime10000Sinkflug, Verfall::Um(t.jetzt + 30));
            }
        }
        if in_luft && (rang(Climb)..=rang(Cruise)).contains(&r) {
            let aus_signal = match t.anschnallzeichen {
                Some(0) => true,
                // AUTO wechselt nie auf 0: über 10.000 ft im Steigflug gilt
                // es als erloschen (⚠️ je Profil prüfen).
                Some(1) | None => t.hoehe_ft >= 10_000.0,
                _ => false,
            };
            if aus_signal {
                let vorher = aus.len();
                self.melde(&mut aus, Slot::AnschnallzeichenAus, Verfall::AbPhase(Descent));
                // v2.0.28 (Michael LTU181): Stellt der Pilot den Schalter
                // wirklich auf AUS, sagt die Kabine es gleich an — auch unter
                // FL100. AUTO/unbekannt wartet weiter auf FL100.
                if t.anschnallzeichen == Some(0) {
                    if let Some(f) = aus.get_mut(vorher) {
                        f.trotz_ruhephase = true;
                    }
                }
            }
        }
        if t.phase == Cruise {
            self.steigflug_vorbei = true;
            // Nur der beobachtete Übergang zählt: Nach einem Wiedereinstieg im
            // Reiseflug ist das Erreichen der Höhe unbekannt (dann 25 %).
            if self.reiseflug_seit.is_none() && vorher.is_some_and(|v| v != Cruise) {
                self.reiseflug_seit = Some(t.jetzt);
            }
            // v2.0.28 (Peter): Der erste Service kommt außerhalb der
            // Kurzstrecke kurz nach Erreichen der Reiseflughöhe, spätestens
            // wie bisher bei 25 %.
            if !self.kurzstrecke()
                && !self.schon(Slot::Reiseflug25)
                && self.schon(Slot::AnschnallzeichenAus)
                && self.reiseflug_seit.is_some_and(|rs| t.jetzt - rs >= SERVICE_NACH_REISEFLUG_S)
            {
                self.melde_reiseflug(&mut aus, t, Slot::Reiseflug25);
            }
            if !self.kurzstrecke() {
                if let (Some(ab), Some(schwellen)) = (self.abheben_epoch, self.reiseflug_schwellen()) {
                    let seit = t.jetzt - ab;
                    let stufen = [schwellen[2], schwellen[1], schwellen[0]];
                    // Sind mehrere Schwellen zugleich überschritten (z. B. nach
                    // einem Wiedereinstieg), kommt nur die höchste; die
                    // niedrigeren entfallen (Verhalten wie UA).
                    if let Some(i) = stufen.iter().position(|(nach_s, _)| seit >= *nach_s) {
                        let slot = stufen[i].1;
                        if !self.schon(slot) {
                            self.melde_reiseflug(&mut aus, t, slot);
                            if self.schon(slot) {
                                for (_, tiefer) in &stufen[i + 1..] {
                                    if self.reiseflug_still_nachziehen {
                                        self.gemeldet.insert(*tiefer);
                                    } else {
                                        self.entfaellt(*tiefer, Entfall::Uebersprungen);
                                    }
                                }
                            }
                        }
                    }
                    self.reiseflug_still_nachziehen = false;
                }
            }
        }
        // Kurzstrecke mit bekannter Flugzeit: der Service nach „Anschnall-
        // zeichen aus“, im Steig- oder Reiseflug (kurze Flüge erreichen die
        // Phase Reiseflug nicht immer). Die 35-min-Regel der langen Flüge
        // gälte hier fast immer; stattdessen eine eigene Reserve.
        if self.kurzstrecke()
            && self.flugzeit_bekannt()
            && in_luft
            && (rang(Climb)..=rang(Cruise)).contains(&r)
            && self.schon(Slot::AnschnallzeichenAus)
            && !self.schon(Slot::Reiseflug25)
        {
            if self.restflug_min(t.jetzt, t.rest_min).is_some_and(|m| m <= KURZSTRECKE_SERVICE_REST_MIN) {
                self.entfaellt(Slot::Reiseflug25, Entfall::ZuWenigZeit);
            } else {
                self.melde(&mut aus, Slot::Reiseflug25, Verfall::AbPhase(Descent));
            }
        }

        // ---- Ende des Service/Bordverkaufs (2.1.0) ----
        // Auf jedem Flug (Thomas 19.10.: „ab dann kann man nichts mehr
        // kaufen“) 12 min vor dem geplanten Sinkflug — spätestens beim
        // tatsächlichen Sinkflugbeginn, damit es bei frühem Sinkflug nicht
        // wegfällt (Peter DLH454: Reiseflug 80 fiel so weg).
        // Altes Modul (art=ende_bordverkauf): Dann ist Reiseflug 80 schon das
        // Ende des Bordverkaufs — keine zweite Ende-Ansage (Codex 2.1.0).
        if self.zweiter_service == Some(false) && !self.schon(Slot::EndeService) {
            self.gemeldet.insert(Slot::EndeService);
        }
        // 2.1.1 C (Entscheidung 4): nie vor dem Service. EWG 221: nur nach
        // einem gespielten (Rückmeldung des Hosts, [`Self::service_ende`]).
        // Lief er nicht (kam nie, entfiel, abgewählt, stumm, verfallen),
        // entfällt das Ende beim Sinkflug als übersprungen; ein gemeldeter
        // Service, dessen Ende noch aussteht, hält es bis zum Ende des
        // Sinkflugs zurück (er kann noch zu Ende spielen).
        if !self.schon(Slot::EndeService) {
            let geplant = self
                .abheben_epoch
                .zip(self.bis_sinkflug_s())
                .is_some_and(|(ab, b)| t.jetzt >= ab + b.round() as i64 - REISEFLUG80_VOR_TOD_S);
            let im_fenster = in_luft && (rang(Climb)..=rang(Descent)).contains(&r);
            match self.service {
                Service::Gespielt => {
                    if im_fenster && (geplant || r >= rang(Descent)) {
                        self.melde(&mut aus, Slot::EndeService, Verfall::AbPhase(Approach));
                    }
                }
                Service::Gemeldet => {
                    if r > rang(Descent) {
                        self.entfaellt(Slot::EndeService, Entfall::Uebersprungen);
                    }
                }
                Service::Offen | Service::KommtNicht => {
                    if r >= rang(Descent) {
                        self.entfaellt(Slot::EndeService, Entfall::Uebersprungen);
                    }
                }
            }
        }

        // ---- Turbulenz (nur im Reiseflug, nach „Anschnallzeichen aus“) ----
        // Nicht mehr nach der Sinkflug-Ansage des Kapitäns oder kurz vor der
        // Landung: Dann schaltet der Pilot das Zeichen für den Sinkflug ein,
        // das ist keine Turbulenz (Michael BER181: 24 s vor dem Sinkflug).
        let sinkflug_naht = self.schon(Slot::KapitaenSinkflug) || t.rest_min.is_some_and(|m| m <= 30.0);
        if t.phase == Cruise && self.schon(Slot::AnschnallzeichenAus) && !sinkflug_naht {
            let an = t.anschnallzeichen == Some(2);
            let war_an = self.letztes_zeichen == Some(2);
            if an && !war_an && !self.turbulenz_aktiv {
                let frei = self
                    .letzte_turbulenz
                    .is_none_or(|l| t.jetzt - l >= TURBULENZ_ABSTAND_S);
                if frei {
                    self.letzte_turbulenz = Some(t.jetzt);
                    self.turbulenz_aktiv = true;
                    self.melde(&mut aus, Slot::Turbulenz, Verfall::Um(t.jetzt + 120));
                }
            }
            if t.anschnallzeichen == Some(0) && self.turbulenz_aktiv {
                self.turbulenz_aktiv = false;
                self.melde(&mut aus, Slot::TurbulenzVorbei, Verfall::Um(t.jetzt + 300));
            }
        }

        // ---- Warteschleife ----
        if t.phase == Holding {
            let seit = *self.warteschleife_seit.get_or_insert(t.jetzt);
            if !self.warteschleife_gemeldet && t.jetzt - seit >= WARTESCHLEIFE_NACH_S {
                self.warteschleife_gemeldet = true;
                self.melde(&mut aus, Slot::Warteschleife, Verfall::AbPhase(Final));
            }
        } else {
            self.warteschleife_seit = None;
            self.warteschleife_gemeldet = false;
        }

        // ---- Sinkflug und Landung ----
        if in_luft && self.steigflug_vorbei && r < rang(Final) {
            let nach_rest = !self.umgeleitet && t.rest_min.is_some_and(|m| m <= KAPITAEN_VOR_LANDUNG_MIN);
            let nach_tod = self.tod().is_some_and(|tod| t.jetzt >= tod - KAPITAEN_VOR_TOD_S);
            let nach_phase = r >= rang(Descent);
            if nach_rest || nach_tod || nach_phase {
                self.melde(&mut aus, Slot::KapitaenSinkflug, Verfall::AbPhase(Final));
                if self.schon(Slot::KapitaenSinkflug) && self.kapitaen_sinkflug == KapitaenSinkflug::Offen && !self.abgewaehlt.contains(&Slot::KapitaenSinkflug) {
                    self.kapitaen_sinkflug = KapitaenSinkflug::Gemeldet;
                }
            }
        }
        // Kabine: frühestens zwei Minuten nach dem ENDE der Kapitänsansage,
        // dann unter [`KABINE_SINKFLUG_UNTER_FT`] im Sinkflug bzw. im Anflug
        // (die frühere Regel „spätestens nach fünf Minuten, auch hoch“ ist
        // weg, Thomas 22.10.2026). Wartet der Kapitän noch (Ton, Pause), wartet die
        // Kabine mit — außer im Anflug. Kommt keine Kapitänsansage (stumm,
        // Fehler, Wiedereinstieg im Sinkflug), zählen Anflug bzw. Höhe allein.
        if in_luft && self.steigflug_vorbei && r < rang(Final) && !self.schon(Slot::KabineSinkflug) {
            let tief = t.phase == Approach || (r >= rang(Descent) && t.hoehe_ft < KABINE_SINKFLUG_UNTER_FT && t.vs_fpm < 0.0);
            let reif = match self.kapitaen_sinkflug {
                // Im Anflug sofort, sonst ginge sie bei einem kurzen Sinkflug
                // verloren; die Pause der Warteschlange trennt sie trotzdem.
                // Mindestens 2 min nach dem Ende, auch im Anflug (Codex 2.0.27).
                KapitaenSinkflug::Gespielt(k) => {
                    let seit = t.jetzt - k;
                    seit >= KABINE_NACH_KAPITAEN_MIN_S && (t.phase == Approach || tief)
                }
                KapitaenSinkflug::Gemeldet => t.phase == Approach,
                KapitaenSinkflug::Offen | KapitaenSinkflug::KommtNicht => tief,
            };
            // 1c (Rundumschlag 22.10.2026, ANZ 438): spätestens so früh vor der
            // Ruhephase unter 10.000 ft, dass die Ansage nach der Pause hinter
            // der vorigen ([`crate::warteschlange::PAUSE_NACH_ANSAGE_S`]) noch
            // darüber beginnt — sonst wartete sie in der Ruhephase und verfiel
            // bei Final. Das schlägt die 2-min-Frist nach dem Kapitän und gilt
            // auch, wenn der Kapitän selbst noch wartet.
            if reif || self.kurz_vor_ruhephase(t) {
                self.melde(&mut aus, Slot::KabineSinkflug, Verfall::AbPhase(Final));
            }
        }
        // „Cabin crew, prepare for landing“ erst nach der Landevorbereitung
        // der Kabine (Thomas 20.10.2026, Variante A; vorher kam es bei 5 von
        // 19 Flügen davor). Die Höhe bleibt (Entscheidung 19.10.). Kommt die
        // Kabine nicht mehr (abgewählt, Bizjet, entfallen) oder ist der Anflug
        // erreicht, wartet es nicht: kurzer Sinkflug, Ruhephase.
        let kabine_vorbei = self.schon(Slot::KabineSinkflug) || !self.kommt_noch(Slot::KabineSinkflug, t.phase);
        if in_luft
            && (kabine_vorbei || t.phase == Approach)
            && (t.phase == Approach
                || (r >= rang(Descent) && t.hoehe_ft < 20_000.0 && t.vs_fpm < -300.0))
        {
            self.melde(&mut aus, Slot::KommandoLandungVorbereiten, Verfall::AbPhase(Landing));
        }
        // „Plätze für die Landung“: Fahrwerk fährt aus oder unter 2.000 ft im
        // Anflug, sonst im Endanflug (Michael BER181: Final erst bei 645 ft).
        let fahrwerk_raus = t.fahrwerk_unten == Some(true) && self.letztes_fahrwerk == Some(false);
        let tief_im_anflug = t.phase == Approach && t.hoehe_agl_ft.is_some_and(|h| h < PLAETZE_LANDUNG_AGL_FT);
        if in_luft && r >= rang(Descent) && r < rang(Landing) && (fahrwerk_raus || tief_im_anflug) {
            self.melde(&mut aus, Slot::KommandoPlaetzeLandung, Verfall::AbPhase(Landing));
        }
        if t.phase == Final {
            self.melde(&mut aus, Slot::KommandoPlaetzeLandung, Verfall::AbPhase(Landing));
            if !t.durchgestartet {
                self.durchstart_laeuft = false;
            }
        }

        // ---- Durchstarten: aus Final/Landung wieder in eine Luftphase ----
        if let Some(v) = vorher {
            let aus_endanflug = matches!(v, Final | Landing);
            let wieder_hoch = matches!(t.phase, Takeoff | Climb | Cruise | Holding | Descent | Approach);
            let erkannt = t.durchgestartet && self.durchstart_faellig_um.is_none() && !self.durchstart_laeuft;
            if in_luft && ((aus_endanflug && wieder_hoch) || erkannt) {
                self.durchstart_laeuft = true;
                self.durchstart_faellig_um = Some(t.jetzt + DURCHSTART_VERZOEGERUNG_S);
                // Für den neuen Anflug darf das Kommando noch einmal kommen.
                self.gemeldet.remove(&Slot::KommandoPlaetzeLandung);
            }
        }
        if let Some(um) = self.durchstart_faellig_um {
            if t.jetzt >= um {
                self.durchstart_faellig_um = None;
                self.melde(&mut aus, Slot::Durchstarten, Verfall::AbPhase(Final));
            }
        }

        // ---- Umleitung ----
        if let (Some(plan), Some(jetzt)) = (&self.flug.ziel_plan_icao, &t.ziel_icao) {
            let neu = self
                .angesagte_umleitung
                .as_deref()
                .is_none_or(|alt| !alt.eq_ignore_ascii_case(jetzt));
            if in_luft && !jetzt.is_empty() && !jetzt.eq_ignore_ascii_case(plan) && neu {
                self.gemeldet.remove(&Slot::Umleitung);
                let vorher = aus.len();
                self.melde(&mut aus, Slot::Umleitung, Verfall::AbPhase(Final));
                if aus.len() > vorher {
                    self.angesagte_umleitung = Some(jetzt.clone());
                }
            }
        }

        // ---- Nach der Landung ----
        if t.phase == TaxiIn {
            self.melde(&mut aus, Slot::NachDerLandung, Verfall::AbPhase(BlocksOn));
        }
        // Türen entschärfen erst mit stehenden Triebwerken (v2.0.28: A346
        // B78DQ… hielt mit vier laufenden Triebwerken auf dem Rollweg, die
        // Phase stand kurz auf BlocksOn). Ohne Signal wie bisher; spätestens
        // bei „Angekommen“.
        let triebwerke_stehen = t.triebwerke_an != Some(true);
        if (t.phase == BlocksOn && triebwerke_stehen) || t.phase == Arrived {
            self.melde(&mut aus, Slot::KommandoTuerenEntschaerfen, Verfall::Um(t.jetzt + 120));
        }
        if t.phase == Arrived {
            self.melde(&mut aus, Slot::Aussteigen, Verfall::AbPhase(PirepSubmitted));
        }

        self.letzte_phase = Some(t.phase);
        self.letzte_hoehe_ft = Some(t.hoehe_ft);
        self.letztes_zeichen = t.anschnallzeichen;
        self.letztes_beacon = t.beacon;
        self.letztes_fahrwerk = t.fahrwerk_unten;
        aus
    }

    fn melde_reiseflug(&mut self, aus: &mut Vec<Faellig>, t: &Tick, slot: Slot) {
        if self.schon(slot) {
            return;
        }
        // Zu nah am Sinkflug: entfällt ganz. Die dritte kommt fest vor dem
        // TOD (Thomas: 10–15 min) und braucht nur die Reserve vor der
        // Sinkflug-Ansage des Kapitäns, nicht die 35 min (Codex 2.0.27).
        let reserve = match slot {
            Slot::Reiseflug80 if self.zweiter_service == Some(true) => ZWEITER_SERVICE_REST_MIN,
            Slot::Reiseflug80 => KURZSTRECKE_SERVICE_REST_MIN,
            _ => 35.0,
        };
        if self.restflug_min(t.jetzt, t.rest_min).is_some_and(|m| m <= reserve) {
            self.entfaellt(slot, Entfall::ZuWenigZeit);
            return;
        }
        let frei = self
            .letzte_reiseflug_ansage
            .is_none_or(|l| t.jetzt - l >= REISEFLUG_ABSTAND_S);
        if !frei {
            return; // später nochmal versuchen
        }
        let vorher = aus.len();
        self.melde(aus, slot, Verfall::AbPhase(FlightPhase::Descent));
        if aus.len() > vorher {
            self.letzte_reiseflug_ansage = Some(t.jetzt);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FlightPhase::*;

    fn flug(min: u32) -> Flugdaten {
        Flugdaten {
            flugart: Some(Flugart::Linie),
            geplante_flugzeit_min: Some(min),
            tod_nach_abheben_s: None,
            ziel_plan_icao: Some("LFBO".into()),
            reiseflughoehe_ft: Some(37_000),
            abheben_epoch: None,
        }
    }

    fn tick(jetzt: i64, phase: FlightPhase, hoehe: f64, vs: f64) -> Tick {
        Tick {
            jetzt,
            phase,
            hoehe_ft: hoehe,
            vs_fpm: vs,
            am_boden: matches!(phase, Preflight | Boarding | Pushback | TaxiOut | TakeoffRoll | TaxiIn | BlocksOn | Arrived),
            anschnallzeichen: Some(2),
            rest_min: None,
            ..Default::default()
        }
    }

    /// Die angezeigte Uhrzeit muss die sein, zu der `tick` die Ansage
    /// wirklich auslöst (nicht eine zweite, eigene Rechnung).
    #[test]
    fn zeitpunkt_trifft_das_ausloesen() {
        for (min, tod) in [(120u32, None), (300, Some(260 * 60i64)), (75, None)] {
            let mut f = flug(min);
            f.tod_nach_abheben_s = tod;
            f.abheben_epoch = Some(1_000);
            let mut p = Planer::fortsetzen(f, Cruise);
            let vorher: Vec<(Slot, Option<i64>)> = [Slot::Reiseflug25, Slot::Reiseflug50, Slot::Reiseflug80]
                .into_iter()
                .map(|s| (s, p.zeitpunkt(s, None)))
                .collect();
            let mut ausgeloest: Vec<(Slot, i64)> = Vec::new();
            let mut t = 1_000;
            while t < 1_000 + i64::from(min) * 60 {
                t += 10;
                for f in p.tick(&tick(t, Cruise, 37_000.0, 0.0)) {
                    if matches!(f.slot, Slot::Reiseflug25 | Slot::Reiseflug50 | Slot::Reiseflug80) {
                        ausgeloest.push((f.slot, t));
                    }
                }
            }
            for (s, um) in &ausgeloest {
                let erwartet = vorher.iter().find(|(x, _)| x == s).and_then(|(_, z)| *z).expect("Uhrzeit angezeigt");
                assert!((um - erwartet).abs() <= 10, "{min} min {s:?}: angezeigt {erwartet}, ausgelöst {um}");
            }
            assert!(!ausgeloest.is_empty(), "{min} min: nichts ausgelöst");
            // Was gelaufen ist, hat keine Uhrzeit mehr.
            assert_eq!(p.zeitpunkt(ausgeloest[0].0, None), None);
        }
        // Sinkflug: frühere von TOD − 90 s (TOD aus der Route) und
        // Landung − 20 min (22.10.2026); die starre OFP-Uhrzeit zählt nicht.
        let mut f = flug(120);
        f.tod_nach_abheben_s = Some(90 * 60);
        f.abheben_epoch = Some(0);
        let mut p = Planer::fortsetzen(f, Cruise);
        assert_eq!(p.zeitpunkt(Slot::KapitaenSinkflug, None), None, "ohne Route keine Uhrzeit aus dem OFP");
        let mut tk = tick(60, Cruise, 37_000.0, 0.0);
        tk.tod_erwartet = Some(90 * 60);
        p.tick(&tk);
        assert_eq!(p.zeitpunkt(Slot::KapitaenSinkflug, None), Some(90 * 60 - KAPITAEN_VOR_TOD_S));
        assert_eq!(p.zeitpunkt(Slot::KapitaenSinkflug, Some(100 * 60)), Some(80 * 60));
        // Boden und Landung: keine erfundene Uhrzeit.
        assert_eq!(p.zeitpunkt(Slot::NachDerLandung, Some(100 * 60)), None);
        // Vor dem Abheben keine Reiseflug-Uhrzeit.
        assert_eq!(Planer::neu(flug(120)).zeitpunkt(Slot::Reiseflug25, None), None);
    }

    /// Spielt einen ganzen Flug in 10-s-Schritten durch und sammelt alles.
    fn normalflug(min: u32) -> Vec<Slot> {
        normalflug_mit(Planer::neu(flug(min)), min)
    }

    fn normalflug_mit(mut p: Planer, min: u32) -> Vec<Slot> {
        let mut alle = Vec::new();
        let mut t = 0i64;
        // Der Host meldet die Sinkflug-Ansage des Kapitäns und den Service
        // gleich als gespielt.
        let lauf = |p: &mut Planer, tk: Tick, alle: &mut Vec<Slot>| {
            for f in p.tick(&tk) {
                if f.slot == Slot::KapitaenSinkflug {
                    p.kapitaen_sinkflug_ende(true, tk.jetzt);
                }
                if f.slot == Slot::Reiseflug25 {
                    p.service_ende(true);
                }
                alle.push(f.slot);
            }
        };
        // Boarding 7 min (Boarding-Ansage nach 5 min), sonst je 5 min.
        for phase in [Preflight, Boarding, Pushback, TaxiOut, TakeoffRoll] {
            for _ in 0..(if phase == Boarding { 42 } else { 30 }) {
                t += 10;
                lauf(&mut p, tick(t, phase, 300.0, 0.0), &mut alle);
            }
        }
        // Steigflug bis 37.000 ft
        let mut h = 300.0;
        while h < 37_000.0 {
            t += 10;
            h += 400.0;
            let mut tk = tick(t, Climb, h, 2400.0);
            if h > 12_000.0 {
                tk.anschnallzeichen = Some(0);
            }
            lauf(&mut p, tk, &mut alle);
        }
        let ende_reise = t + i64::from(min) * 60 - 40 * 60;
        while t < ende_reise {
            t += 10;
            let mut tk = tick(t, Cruise, 37_000.0, 0.0);
            tk.anschnallzeichen = Some(0);
            tk.rest_min = Some(((ende_reise - t) / 60 + 25) as f64);
            lauf(&mut p, tk, &mut alle);
        }
        while h > 3_000.0 {
            t += 10;
            h -= 300.0;
            let mut tk = tick(t, Descent, h, -1800.0);
            tk.anschnallzeichen = Some(2);
            lauf(&mut p, tk, &mut alle);
        }
        for phase in [Approach, Final, Landing, TaxiIn, BlocksOn, Arrived] {
            for _ in 0..12 {
                t += 10;
                lauf(&mut p, tick(t, phase, 300.0, -700.0), &mut alle);
            }
        }
        alle
    }

    // 75 min: Bordverkauf-Ende 12 min vor TOD kommt auch hier (seit 2.0.27,
    // vorher fiel die dritte Ansage bei 80 % in die 35-min-Sperre).
    const REISE_75: [Slot; 3] = [Slot::Reiseflug25, Slot::Reiseflug50, Slot::Reiseflug80];
    const REISE_90: [Slot; 3] = [Slot::Reiseflug25, Slot::Reiseflug50, Slot::Reiseflug80];
    const REISE_120: [Slot; 3] = [Slot::Reiseflug25, Slot::Reiseflug50, Slot::Reiseflug80];

    #[test]
    fn langstrecke_bekommt_den_vollen_ablauf_in_reihenfolge() {
        let alle = normalflug(240);
        // Ohne ZFW- und Beacon-Signal: Begrüßung und „Boarding completed“
        // beim Pushback, vor „Türen zu“ (2.1.1 A/E).
        let erwartet = [
            Slot::Boarding,
            Slot::BoardingAbgeschlossen,
            Slot::KommandoTuerenScharf,
            Slot::BegruessungTuerenZu,
            Slot::Sicherheit,
            Slot::KapitaenBegruessung,
            Slot::KommandoStart,
            Slot::Chime10000Steigflug,
            Slot::AnschnallzeichenAus,
            Slot::Reiseflug25,
            Slot::Reiseflug50,
            Slot::Reiseflug80,
            // 2.1.0: Ende des Service 12 min vor dem Sinkflug.
            Slot::EndeService,
            Slot::KapitaenSinkflug,
            Slot::KabineSinkflug,
            // Unter FL200 im Sinkflug — also vor dem 10.000-ft-Chime.
            Slot::KommandoLandungVorbereiten,
            Slot::Chime10000Sinkflug,
            Slot::KommandoPlaetzeLandung,
            Slot::NachDerLandung,
            Slot::KommandoTuerenEntschaerfen,
            Slot::Aussteigen,
        ];
        assert_eq!(alle, erwartet);
    }

    #[test]
    fn kommt_noch_nach_wiedereinstieg_und_kurzstrecke() {
        let p = Planer::fortsetzen(flug(120), Cruise);
        assert!(!p.kommt_noch(Slot::Boarding, Cruise), "vor dem Wiedereinstieg erledigt");
        assert!(p.kommt_noch(Slot::KapitaenSinkflug, Cruise));
        assert!(p.kommt_noch(Slot::Reiseflug80, Cruise));
        let kurz = Planer::neu(flug(55));
        assert!(kurz.kommt_noch(Slot::Boarding, Boarding));
        assert!(!kurz.kommt_noch(Slot::Reiseflug50, Climb), "Kurzstrecke ohne Zwischenstand");
        assert!(!kurz.kommt_noch(Slot::Reiseflug80, Climb), "Kurzstrecke ohne Vor-Sinkflug");
        assert!(kurz.kommt_noch(Slot::Reiseflug25, Climb), "Kurzstrecke mit Service");
        // Unbekannte Flugzeit: auch kein Service.
        let mut ohne = flug(55);
        ohne.geplante_flugzeit_min = None;
        assert!(!Planer::neu(ohne).kommt_noch(Slot::Reiseflug25, Climb));
        // Phase uebersprungen (nie Cruise gemeldet): im Sinkflug kommt der
        // Reiseflug nicht mehr, die Sinkflug-Ansage schon; im Final keine.
        let p = Planer::neu(flug(120));
        assert!(!p.kommt_noch(Slot::Reiseflug50, Descent));
        assert!(p.kommt_noch(Slot::KapitaenSinkflug, Descent));
        assert!(!p.kommt_noch(Slot::KapitaenSinkflug, Final));
        // 2.1.1 A: Die Begrüßung kommt noch beim Pushback/Rollen (Rückfall),
        // ab dem Startlauf nicht mehr.
        assert!(p.kommt_noch(Slot::Boarding, TaxiOut));
        assert!(!p.kommt_noch(Slot::Boarding, TakeoffRoll), "Boarding nur bis zum Startlauf");
    }

    #[test]
    fn kurzstrecke_nur_mit_service() {
        let alle = normalflug(55);
        for s in [Slot::Reiseflug50, Slot::Reiseflug80] {
            assert!(!alle.contains(&s), "{s:?} darf auf Kurzstrecke nicht kommen");
        }
        let pos = |s: Slot| alle.iter().position(|x| *x == s);
        assert!(pos(Slot::Reiseflug25).is_some(), "Service auf Kurzstrecke: {alle:?}");
        assert!(pos(Slot::AnschnallzeichenAus) < pos(Slot::Reiseflug25), "erst Anschnallzeichen aus");
        assert!(pos(Slot::Reiseflug25) < pos(Slot::KapitaenSinkflug), "vor dem Sinkflug");
    }

    /// Echte Kurzflüge: 45 und 55 min, Anschnallzeichen aus beim Durchsteigen
    /// von 10.000 ft, Reiseflug wird nie als Phase erkannt. Der Service kommt
    /// mit genug Abstand zum Sinkflug; ist zu wenig Zeit, entfällt er sichtbar.
    #[test]
    fn kurzstrecke_service_bei_45_und_55_min() {
        for min in [45u32, 55] {
            let mut p = Planer::neu(flug(min));
            let mut service = None;
            let mut t = 0i64;
            p.tick(&tick(t, TakeoffRoll, 0.0, 0.0));
            // Steigflug 2.000 ft/min, Zeichen auf AUTO (aus über 10.000 ft).
            let mut h = 0.0;
            while h < 24_000.0 {
                t += 10;
                h += 333.0;
                let mut tk = tick(t, Climb, h, 2_000.0);
                tk.anschnallzeichen = Some(1);
                tk.rest_min = Some(f64::from(min) - t as f64 / 60.0);
                for f in p.tick(&tk) {
                    if f.slot == Slot::Reiseflug25 {
                        service = Some(t);
                    }
                }
            }
            let s = service.unwrap_or_else(|| panic!("{min} min: kein Service"));
            assert!(f64::from(min) - s as f64 / 60.0 > KURZSTRECKE_SERVICE_REST_MIN, "{min} min: Service bei {s} s zu spät");
            assert!(p.entfallene_abholen().is_empty());
        }
        // Gegenprobe: 30-min-Flug, beim Anschnallzeichen sind nur 25 min übrig.
        let mut p = Planer::neu(flug(30));
        p.tick(&tick(0, TakeoffRoll, 0.0, 0.0));
        let mut tk = tick(300, Climb, 11_000.0, 2_000.0);
        tk.anschnallzeichen = Some(0);
        tk.rest_min = Some(25.0);
        assert!(p.tick(&tk).iter().all(|f| f.slot != Slot::Reiseflug25));
        assert_eq!(p.entfallene_abholen(), vec![(Slot::Reiseflug25, Entfall::ZuWenigZeit)]);
    }

    #[test]
    fn reiseflug_ansagen_halten_den_mindestabstand() {
        // 90 min: 25 % (~22 min) und 50 % (~45 min) liegen nah beieinander.
        let mut p = Planer::neu(flug(90));
        p.abheben_epoch = Some(0);
        p.gemeldet.insert(Slot::AnschnallzeichenAus);
        let mut zeiten = Vec::new();
        for t in (0..90 * 60).step_by(10) {
            let mut tk = tick(t, Cruise, 37_000.0, 0.0);
            tk.rest_min = Some(((90 * 60 - t) / 60) as f64);
            for f in p.tick(&tk) {
                if matches!(f.slot, Slot::Reiseflug25 | Slot::Reiseflug50 | Slot::Reiseflug80) {
                    zeiten.push(t);
                }
            }
        }
        for paar in zeiten.windows(2) {
            assert!(paar[1] - paar[0] >= REISEFLUG_ABSTAND_S, "{zeiten:?}");
        }
    }

    /// Welche Reiseflug-Ansagen bei welcher Flugdauer wirklich kommen —
    /// bewusst festgehalten, damit „R50 entfällt, wenn R80 zuerst erreicht
    /// ist“ eine Entscheidung bleibt und kein Zufall.
    #[test]
    fn reiseflug_ansagen_je_flugdauer() {
        let reise = |min: u32| -> Vec<Slot> {
            normalflug(min)
                .into_iter()
                .filter(|s| matches!(s, Slot::Reiseflug25 | Slot::Reiseflug50 | Slot::Reiseflug80))
                .collect()
        };
        assert_eq!(reise(55), vec![Slot::Reiseflug25]);
        assert_eq!(reise(75), REISE_75.to_vec());
        assert_eq!(reise(90), REISE_90.to_vec());
        assert_eq!(reise(120), REISE_120.to_vec());
        assert_eq!(
            reise(240),
            REISE_120.to_vec()
        );
    }

    #[test]
    fn wiedereinstieg_im_reiseflug_begruesst_nicht_nochmal() {
        let mut f = flug(240);
        f.abheben_epoch = Some(0);
        // Neustart 2 h nach dem Abheben (50 % der Planzeit).
        let mut p = Planer::fortsetzen(f, Cruise);
        let mut tk = tick(7_210, Cruise, 37_000.0, 0.0);
        tk.anschnallzeichen = Some(0);
        tk.rest_min = Some(120.0);
        let slots: Vec<Slot> = p.tick(&tk).into_iter().map(|f| f.slot).collect();
        // Nur die höchste erreichte %-Ansage, keine Begrüßung, kein
        // „Anschnallzeichen aus“, kein „Kapitän Reiseflug“.
        assert_eq!(slots, vec![Slot::Reiseflug50]);
        assert!(!p.schon(Slot::Reiseflug80));
        assert!(p.entfallene_abholen().is_empty(), "R25 lief vor dem Neustart, kein „entfallen“");
    }

    /// Lasse NOZ 607 (20.10.2026, Protokoll pMZYLPYPnQDxEyOQ): OFP FL330,
    /// geflogen 8 min FL250 – phase_v2 bleibt dabei in Climb und wechselt
    /// direkt in Descent (bei FL197). Bisher kamen Kapitän, Kabine und Gong im
    /// Sinkflug nie, nur „prepare for landing“. Jetzt kommen alle drei, und
    /// vor dem Ende des Steigflugs nennt die Seite keine Uhrzeit.
    #[test]
    fn lasse_noz607_sinkflug_ohne_erkannten_reiseflug() {
        let landung = 2_160; // 36 min nach dem Abheben
        let mut p = Planer::neu(flug(65));
        let mut wann: Vec<(Slot, i64)> = Vec::new();
        fn lauf(p: &mut Planer, wann: &mut Vec<(Slot, i64)>, s: i64, phase: FlightPhase, h: f64, vs: f64) {
            let mut tk = tick(s, phase, h, vs);
            tk.rest_min = Some((2_160 - s) as f64 / 60.0);
            for f in p.tick(&tk) {
                if f.slot == Slot::KapitaenSinkflug {
                    p.kapitaen_sinkflug_ende(true, s + 40);
                }
                wann.push((f.slot, s));
            }
        }
        lauf(&mut p, &mut wann, 0, Takeoff, 700.0, 3_000.0);
        for s in (10..600).step_by(10) {
            lauf(&mut p, &mut wann, s, Climb, 700.0 + f64::from(s as i32) * 40.0, 2_500.0);
        }
        // 8 min FL250, Restzeit fällt unter 25 min – weiter Climb.
        for s in (600..1_090).step_by(10) {
            lauf(&mut p, &mut wann, s, Climb, 25_000.0, 0.0);
            assert_eq!(p.zeitpunkt(Slot::KapitaenSinkflug, Some(landung)), None, "keine Uhrzeit vor dem Ende des Steigflugs ({s})");
        }
        // Sinken, phase_v2 bestätigt den Sinkflug erst nach 120 s.
        for s in (1_090..1_210).step_by(10) {
            lauf(&mut p, &mut wann, s, Climb, 25_000.0 - f64::from((s - 1_090) as i32) * 40.0, -2_000.0);
        }
        assert!(wann.iter().all(|(x, _)| *x != Slot::KapitaenSinkflug), "nicht im Steigflug");
        for s in (1_210..1_800).step_by(10) {
            let h = 19_700.0 - f64::from((s - 1_210) as i32) * 30.0;
            lauf(&mut p, &mut wann, s, if h < 5_000.0 { Approach } else { Descent }, h, -1_800.0);
        }
        let bei = |x: Slot| wann.iter().find(|(y, _)| *y == x).map(|(_, t)| *t);
        assert_eq!(bei(Slot::KapitaenSinkflug), Some(1_210), "mit dem Wechsel in den Sinkflug");
        let kabine = bei(Slot::KabineSinkflug).expect("Kabine kommt");
        assert!(kabine >= 1_250 + KABINE_NACH_KAPITAEN_MIN_S, "nach dem Kapitän: {kabine}");
        let gong = bei(Slot::Chime10000Sinkflug).expect("Gong kommt");
        assert!((1_530..=1_540).contains(&gong), "bei 10.000 ft: {gong}");
        assert_eq!(wann.iter().filter(|(x, _)| *x == Slot::Chime10000Sinkflug).count(), 1);
    }

    /// Hamadoun oBmVxn66 (E195): Höhe schwingt ±700 ft mit ±5000 fpm, nie
    /// ein Level, nie Cruise; Climb → Descent bei 17.658 ft. Der Kapitän
    /// kommt dann, nicht in der Schwingung.
    #[test]
    fn hamadoun_schwingung_kapitaen_beim_sinkflug() {
        let mut p = Planer::neu(flug(38));
        p.tick(&tick(0, Takeoff, 700.0, 3_000.0));
        for s in (10..1_500).step_by(10) {
            let auf = (s / 60) % 2 == 0;
            let mut tk = tick(s, Climb, if auf { 22_200.0 } else { 21_500.0 }, if auf { 5_000.0 } else { -5_000.0 });
            tk.rest_min = Some(20.0);
            assert!(slots(p.tick(&tk)).iter().all(|x| *x != Slot::KapitaenSinkflug), "nicht in der Schwingung ({s})");
        }
        let mut tk = tick(1_500, Descent, 17_658.0, -2_000.0);
        tk.rest_min = Some(13.0);
        assert!(slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug));
    }

    /// Warum es keine Regel „Höhe gehalten über 10.000 ft“ gibt: Eine ATC-
    /// Zwischenhöhe (3 min FL120) auf einem kurzen Flug darf den Kapitän nicht
    /// mitten im Steigflug auslösen. Er kommt erst mit dem Reiseflug.
    #[test]
    fn atc_zwischenhoehe_loest_den_kapitaen_nicht_aus() {
        let mut p = Planer::neu(flug(40));
        p.tick(&tick(0, Takeoff, 700.0, 3_000.0));
        for s in (10..=400).step_by(10) {
            let mut tk = tick(s, Climb, if s < 220 { 700.0 + f64::from(s as i32) * 52.0 } else { 12_000.0 }, if s < 220 { 2_500.0 } else { 0.0 });
            tk.rest_min = Some(22.0);
            assert!(slots(p.tick(&tk)).iter().all(|x| *x != Slot::KapitaenSinkflug), "nicht auf der Zwischenhöhe ({s})");
        }
        let mut tk = tick(700, Cruise, 20_000.0, 0.0);
        tk.rest_min = Some(17.0);
        assert!(slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug), "mit dem Reiseflug");
    }

    /// Erkennt phase_v2 den Sinkflug erst unter 10.000 ft (Spitze darüber),
    /// ist die Flanke vorbei: Gong einmal nachholen, nicht doppelt.
    #[test]
    fn gong_nachgeholt_wenn_der_sinkflug_erst_unter_10000_erkannt_wird() {
        let mut p = Planer::neu(flug(30));
        p.tick(&tick(0, Takeoff, 700.0, 3_000.0));
        p.tick(&tick(100, Climb, 12_000.0, 0.0));
        p.tick(&tick(200, Climb, 10_500.0, -1_500.0));
        assert!(slots(p.tick(&tick(240, Climb, 9_800.0, -1_500.0))).is_empty(), "Flanke im Steigflug: kein Gong");
        let f = slots(p.tick(&tick(300, Descent, 9_500.0, -1_500.0)));
        assert!(f.contains(&Slot::Chime10000Sinkflug), "{f:?}");
        // Gegenprobe: Steigflug endet in Reiseflug/Warteschleife unter
        // 10.000 ft (nach einer Spitze darüber) – das ist kein Sinkflug.
        for ende in [Cruise, Holding] {
            let mut q = Planer::neu(flug(30));
            q.tick(&tick(0, Takeoff, 700.0, 3_000.0));
            q.tick(&tick(100, Climb, 12_000.0, 0.0));
            q.tick(&tick(200, Climb, 9_800.0, -500.0));
            assert!(!slots(q.tick(&tick(300, ende, 9_000.0, 0.0))).contains(&Slot::Chime10000Sinkflug), "{ende:?}");
        }
        assert!(!slots(p.tick(&tick(310, Descent, 9_300.0, -1_500.0))).contains(&Slot::Chime10000Sinkflug));
    }

    /// Neustart im Sinkflug: Der Merker ist gesetzt, schon Gespieltes kommt
    /// nicht noch einmal.
    #[test]
    fn wiedereinstieg_im_sinkflug_ohne_doppelte_ansagen() {
        for phase in [Cruise, Holding, Descent, Approach, Final] {
            assert!(Planer::fortsetzen(flug(60), phase).steigflug_vorbei, "Merker nach dem Neustart in {phase:?}");
        }
        assert!(!Planer::fortsetzen(flug(60), Climb).steigflug_vorbei, "im Steigflug noch nicht");
        assert!(Planer::fortsetzen(flug(60), Descent).zeitpunkt(Slot::KapitaenSinkflug, Some(3_000)).is_some(), "Uhrzeit nach dem Neustart im Sinkflug");
        let mut p = Planer::fortsetzen(flug(60), Descent);
        p.schon_gespielt([Slot::KapitaenSinkflug]);
        let f = slots(p.tick(&tick(1_000, Descent, 18_000.0, -2_000.0)));
        assert!(!f.contains(&Slot::KapitaenSinkflug), "{f:?}");
        let mut q = Planer::fortsetzen(flug(60), Descent);
        assert!(slots(q.tick(&tick(1_000, Descent, 18_000.0, -2_000.0))).contains(&Slot::KapitaenSinkflug), "noch nicht gespielt: kommt");
    }

    /// SAS 315 (Hamadoun, 20.10.2026, KEymQD5G): Kapitän im Reiseflug,
    /// danach Sinkflug mit ~2000 fpm. „Prepare for landing“ (unter FL200)
    /// kam 6 s VOR der Kabine (5 min nach dem Kapitän). Variante A (Thomas
    /// 20.10.): erst die Kabine, im selben Takt danach das Kommando.
    #[test]
    fn sas315_kabine_vor_prepare_for_landing() {
        let mut p = Planer::fortsetzen(flug(60), Cruise);
        let mut tk = tick(0, Cruise, 30_400.0, 0.0);
        tk.rest_min = Some(20.0);
        assert!(slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug));
        p.kapitaen_sinkflug_ende(true, 30);
        let mut reihe: Vec<(i64, Slot)> = Vec::new();
        for s in (40..=500).step_by(10) {
            let h = 28_700.0 - f64::from((s - 40) as i32) * 33.0;
            for f in p.tick(&tick(s, Descent, h, -2_000.0)) {
                reihe.push((s, f.slot));
            }
        }
        let bei = |x: Slot| reihe.iter().position(|(_, y)| *y == x);
        let kab = bei(Slot::KabineSinkflug).expect("Kabine");
        let kom = bei(Slot::KommandoLandungVorbereiten).expect("Kommando");
        assert!(kab < kom, "Kabine vor dem Kommando: {reihe:?}");
        assert_eq!(reihe[kab].0, reihe[kom].0, "Kommando direkt danach, nicht erst später: {reihe:?}");
        assert_eq!(reihe[kab].0, 310, "Kabine beim ersten Tick unter 20.000 ft: {reihe:?}");
    }

    /// Kabine abgewählt: „Prepare for landing“ wie bisher unter FL200.
    /// Kurzer Sinkflug: Im Anflug wartet es nicht auf die Kabine (die noch
    /// 2 min Abstand zum Kapitän hält) – die Seite verspricht es „im Anflug“.
    #[test]
    fn prepare_for_landing_ohne_kabine_und_im_anflug() {
        let mut p = Planer::fortsetzen(flug(60), Cruise);
        p.abwaehlen([Slot::KabineSinkflug]);
        p.tick(&tick(0, Descent, 25_000.0, -2_000.0));
        assert!(slots(p.tick(&tick(10, Descent, 19_800.0, -2_000.0))).contains(&Slot::KommandoLandungVorbereiten));
        let mut q = Planer::fortsetzen(flug(60), Cruise);
        // Langsam sinkend, damit 1c (spätestens 90 s vor 10.000 ft) die
        // Kabine nicht schon hier meldet; geprüft wird das Kommando im Anflug.
        q.tick(&tick(0, Descent, 12_000.0, -500.0));
        q.kapitaen_sinkflug_ende(true, 40);
        let f = slots(q.tick(&tick(60, Approach, 4_800.0, -1_000.0)));
        assert!(f.contains(&Slot::KommandoLandungVorbereiten) && !f.contains(&Slot::KabineSinkflug), "{f:?}");
    }

    #[test]
    fn kabine_folgt_dem_kapitaen_im_sinkflug_mit_abstand() {
        let mut p = Planer::fortsetzen(flug(120), Cruise);
        let ticks = [(0, Descent, 30_000.0), (60, Descent, 27_000.0), (130, Descent, 24_000.0), (200, Descent, 14_000.0)];
        let mut wann = Vec::new();
        for (dt, ph, h) in ticks {
            for f in p.tick(&tick(1_000 + dt, ph, h, -2_000.0)) {
                if f.slot == Slot::KapitaenSinkflug {
                    p.kapitaen_sinkflug_ende(true, 1_000 + dt);
                }
                wann.push((f.slot, dt));
            }
        }
        let bei = |s: Slot| wann.iter().find(|(x, _)| *x == s).map(|(_, t)| *t);
        assert_eq!(bei(Slot::KapitaenSinkflug), Some(0));
        // Hoch über 20.000 ft: noch nicht (auch nicht nach 130 s).
        assert_eq!(bei(Slot::KabineSinkflug), Some(200), "erst unter 20.000 ft, nicht sofort");
        // 22.10.2026 (EDW432, SAS4112 …): Fünf Minuten nach dem Kapitän, aber
        // noch auf 31.000 ft, kommt die Kabine nicht mehr — erst unter 20.000 ft.
        let mut q = Planer::fortsetzen(flug(120), Cruise);
        q.tick(&tick(1_000, Descent, 35_000.0, -1_000.0));
        q.kapitaen_sinkflug_ende(true, 1_000);
        assert!(q.tick(&tick(1_300, Descent, 31_000.0, -1_000.0)).iter().all(|f| f.slot != Slot::KabineSinkflug), "nicht mehr auf 31.000 ft");
        assert!(q.tick(&tick(1_900, Descent, 21_000.0, -1_000.0)).iter().all(|f| f.slot != Slot::KabineSinkflug));
        assert!(q.tick(&tick(1_960, Descent, 19_900.0, -1_000.0)).iter().any(|f| f.slot == Slot::KabineSinkflug), "unter 20.000 ft");
    }

    /// 1c (Rundumschlag 22.10.2026, ANZ 438): Kabine Sinkflug wird nie erst
    /// in der Ruhephase fällig — spätestens, wenn die Sinkrate sie in der
    /// Pause hinter der vorigen Ansage unter 10.000 ft brächte; das schlägt
    /// die 2-min-Frist nach dem Kapitän und gilt auch, wenn der Kapitän noch
    /// wartet. Rot auf v2.1.6: bei 11.500 ft, 30 s nach dem Kapitän, nichts.
    #[test]
    fn kabine_sinkflug_spaetestens_vor_der_ruhephase() {
        let kabine = |p: &mut Planer, t: i64, h: f64| p.tick(&tick(t, Descent, h, -2_000.0)).iter().any(|f| f.slot == Slot::KabineSinkflug);
        // Kapitän spät zu Ende (Mac-Oberfläche hing): 30 s danach.
        let mut p = Planer::fortsetzen(flug(120), Cruise);
        assert!(p.tick(&tick(1_000, Descent, 30_000.0, -2_000.0)).iter().any(|f| f.slot == Slot::KapitaenSinkflug));
        p.kapitaen_sinkflug_ende(true, 1_500);
        assert!(!kabine(&mut p, 1_510, 14_000.0), "noch 120 s bis 10.000 ft: die Frist gilt");
        assert!(kabine(&mut p, 1_530, 11_500.0), "45 s bis 10.000 ft: jetzt, sonst Ruhephase");
        // Kapitän wartet noch (gemeldet, nicht gespielt): auch dann.
        let mut q = Planer::fortsetzen(flug(120), Cruise);
        q.tick(&tick(1_000, Descent, 30_000.0, -2_000.0));
        assert!(kabine(&mut q, 1_500, 11_000.0));
        // Ohne Ruhephase (Reiseflug unter 12.000 ft) greift die Regel nicht.
        let mut niedrig = flug(120);
        niedrig.reiseflughoehe_ft = Some(11_000);
        let mut r = Planer::fortsetzen(niedrig, Cruise);
        r.tick(&tick(1_000, Descent, 11_000.0, -500.0));
        r.kapitaen_sinkflug_ende(true, 1_500);
        assert!(!kabine(&mut r, 1_530, 10_500.0), "kein Flug mit Ruhephase: 2-min-Frist bleibt");
    }

    /// Der Abstand zählt ab dem Ende der Kapitänsansage, nicht ab ihrer
    /// Planung (Codex beta.6): Wartet der Kapitän vier Minuten auf seinen
    /// Ton, kommt die Kabine nicht vorher — und frühestens zwei Minuten danach.
    #[test]
    fn kabine_wartet_auf_den_gespielten_kapitaen() {
        let mut p = Planer::fortsetzen(flug(120), Cruise);
        let kabine = |p: &mut Planer, t: i64, h: f64, ph: FlightPhase| p.tick(&tick(t, ph, h, -2_000.0)).iter().any(|f| f.slot == Slot::KabineSinkflug);
        assert!(p.tick(&tick(1_000, Descent, 30_000.0, -2_000.0)).iter().any(|f| f.slot == Slot::KapitaenSinkflug));
        // Tief und lange nach der Meldung, aber der Kapitän lief noch nicht.
        assert!(!kabine(&mut p, 1_240, 14_000.0, Descent));
        p.kapitaen_sinkflug_ende(true, 1_250);
        // Höhen so, dass 1c (spätestens 90 s vor 10.000 ft) nicht greift:
        // hier geht es nur um den Abstand zum Kapitän.
        assert!(!kabine(&mut p, 1_300, 14_500.0, Descent), "noch keine zwei Minuten nach dem Ende");
        assert!(kabine(&mut p, 1_370, 14_000.0, Descent));
        // Stumm/Fehler: Die Kabine richtet sich nur nach der Höhe.
        let mut q = Planer::fortsetzen(flug(120), Cruise);
        q.tick(&tick(1_000, Descent, 30_000.0, -2_000.0));
        q.kapitaen_sinkflug_ende(false, 1_010);
        assert!(kabine(&mut q, 1_020, 14_000.0, Descent));
        // Im Anflug auch nach dem Kapitän: mindestens 2 min nach seinem Ende.
        let mut a = Planer::fortsetzen(flug(120), Cruise);
        a.tick(&tick(1_000, Descent, 30_000.0, -2_000.0));
        a.kapitaen_sinkflug_ende(true, 1_050);
        assert!(!kabine(&mut a, 1_100, 4_000.0, Approach), "erst 50 s nach dem Kapitän");
        assert!(kabine(&mut a, 1_170, 3_500.0, Approach));
        // Wartet der Kapitän noch im Anflug, kommt die Kabine trotzdem.
        let mut r = Planer::fortsetzen(flug(120), Cruise);
        r.tick(&tick(1_000, Descent, 30_000.0, -2_000.0));
        assert!(kabine(&mut r, 1_100, 4_000.0, Approach));
    }

    #[test]
    fn wiedereinstieg_im_sinkflug_bringt_nur_noch_die_landung() {
        let mut p = Planer::fortsetzen(flug(120), Descent);
        // Der Service lief vor dem Neustart (Host laut Verlauf, EWG 221).
        p.service_ende(true);
        let mut alle = Vec::new();
        for (i, (ph, h)) in [(Descent, 12_000.0), (Approach, 4_000.0), (Final, 1_000.0), (TaxiIn, 0.0)].into_iter().enumerate() {
            alle.extend(p.tick(&tick(1_000 + i as i64 * 10, ph, h, -800.0)).into_iter().map(|f| f.slot));
        }
        assert_eq!(
            alle,
            // Kabine nicht im selben Takt wie der Kapitän, sondern im Anflug.
            // Thomas 20.10.2026 (Variante A): erst die Landevorbereitung der
            // Kabine, dann „Cabin crew, prepare for landing“ (bis dahin hieß
            // es hier „Kapitän, Kommando an die Crew, dann die Kabine“).
            // 2.1.0: „Ende des Service“ kommt nach einem Neustart im Sinkflug
            // noch (der Host meldet, falls sie vorher schon lief).
            vec![Slot::EndeService, Slot::KapitaenSinkflug, Slot::Chime10000Sinkflug, Slot::KabineSinkflug, Slot::KommandoLandungVorbereiten, Slot::KommandoPlaetzeLandung, Slot::NachDerLandung]
        );
    }

    #[test]
    fn verspaetung_nicht_mehr_nach_dem_pushback() {
        let mut p = Planer::neu(flug(120));
        let mut tk = tick(1, TaxiOut, 0.0, 0.0);
        tk.abweichung_off_block_min = Some(30);
        assert!(!p.tick(&tk).iter().any(|f| f.slot == Slot::VerspaetungGate));
        let mut p = Planer::neu(flug(120));
        let mut tk = tick(1, Boarding, 0.0, 0.0);
        tk.abweichung_off_block_min = Some(30);
        tk.pushback_aktiv = true;
        assert!(!p.tick(&tk).iter().any(|f| f.slot == Slot::VerspaetungGate));
    }

    #[test]
    fn durchstarten_vom_host_signal_einmal() {
        let mut p = Planer::neu(flug(120));
        p.steigflug_vorbei = true;
        let mut n = 0;
        for t in (0..600).step_by(10) {
            let mut tk = tick(t, Approach, 2_500.0, 500.0);
            tk.durchgestartet = true;
            n += p.tick(&tk).iter().filter(|f| f.slot == Slot::Durchstarten).count();
        }
        assert_eq!(n, 1);
    }

    #[test]
    fn fracht_bekommt_nichts() {
        let mut f = flug(120);
        f.flugart = Some(Flugart::Aus);
        let mut p = Planer::neu(f);
        assert!(p.tick(&tick(1, Boarding, 0.0, 0.0)).is_empty());
        assert!(p.tick(&tick(2, TaxiOut, 0.0, 0.0)).is_empty());
    }

    #[test]
    fn bizjet_nur_kapitaen() {
        let mut f = flug(120);
        f.flugart = Some(Flugart::Bizjet);
        let mut p = Planer::neu(f);
        let mut alle = Vec::new();
        for (i, ph) in [Boarding, Pushback, TaxiOut, TakeoffRoll].into_iter().enumerate() {
            alle.extend(p.tick(&tick(i as i64, ph, 0.0, 0.0)).into_iter().map(|f| f.slot));
        }
        // Sicherheit als Kurzfassung des Kapitäns (der Host spielt sie nur mit
        // Kennzeichen des Servers), keine Kabine, keine Kommandos.
        assert_eq!(alle, vec![Slot::Sicherheit, Slot::KapitaenBegruessung]);
    }

    #[test]
    fn bizjet_bekommt_den_gong() {
        let mut f = flug(120);
        f.flugart = Some(Flugart::Bizjet);
        let mut p = Planer::neu(f);
        p.tick(&tick(0, Climb, 9_800.0, 2_000.0));
        assert!(p.tick(&tick(10, Climb, 10_200.0, 2_000.0)).iter().any(|f| f.slot == Slot::Chime10000Steigflug));
        p.tick(&tick(20, Cruise, 30_000.0, 0.0));
        p.tick(&tick(30, Descent, 10_200.0, -2_000.0));
        assert!(p.tick(&tick(40, Descent, 9_800.0, -2_000.0)).iter().any(|f| f.slot == Slot::Chime10000Sinkflug));
        // Gegenprobe: Reiseflug unter 10.000 ft — kein Gong angekündigt.
        let mut g = flug(60);
        g.flugart = Some(Flugart::Bizjet);
        g.reiseflughoehe_ft = Some(8_000);
        assert!(!Planer::neu(g).kommt_noch(Slot::Chime10000Steigflug, Climb));
    }

    fn boden(jetzt: i64, phase: FlightPhase) -> Tick {
        let mut t = tick(jetzt, phase, 0.0, 0.0);
        t.anschnallzeichen = Some(0);
        t.beacon = Some(false);
        t.landelicht = Some(false);
        t
    }

    fn slots(f: Vec<Faellig>) -> Vec<Slot> {
        f.into_iter().map(|x| x.slot).collect()
    }

    /// Ein Boden-Takt am Gate mit Zuladung (kg), Anschnallzeichen und Beacon.
    fn gate(jetzt: i64, zfw: Option<f64>, zeichen: u8, beacon: bool) -> Tick {
        let mut t = boden(jetzt, Boarding);
        t.zfw_kg = zfw;
        t.anschnallzeichen = Some(zeichen);
        t.beacon = Some(beacon);
        t
    }

    /// Wann (s) der Planer welche Ansage meldet — jeder Takt alle 10 s.
    fn melde_zeiten(p: &mut Planer, takte: impl IntoIterator<Item = Tick>) -> Vec<(i64, Slot)> {
        let mut aus = Vec::new();
        for t in takte {
            for f in p.tick(&t) {
                aus.push((t.jetzt, f.slot));
            }
        }
        aus
    }

    /// 2.1.1 A, Peter (Thomas 19.10.2026): Strom → Phase Boarding, Zuladung
    /// leer → Anschnallzeichen an (darf nichts auslösen) → nach 8 min steigt
    /// die Zuladung um 3.000 kg → Begrüßung 30 s danach.
    #[test]
    fn peter_begruessung_folgt_der_zuladung_nicht_dem_anschnallzeichen() {
        // Der Planer kennt keinen OFP mehr: mit und ohne OFP derselbe Ablauf.
        let mut p = Planer::fortsetzen(flug(45), Boarding);
        let mut takte = Vec::new();
        for s in (0..=8 * 60).step_by(10) {
            // Anschnallzeichen nach 1 min an, Zuladung leer (Grundgewicht).
            takte.push(gate(s, Some(43_000.0), if s >= 60 { 2 } else { 0 }, false));
        }
        for s in (8 * 60 + 10..=12 * 60).step_by(10) {
            takte.push(gate(s, Some(46_000.0), 2, false));
        }
        let z = melde_zeiten(&mut p, takte);
        assert_eq!(z, vec![(8 * 60 + 10 + BOARDING_ZULADUNG_S, Slot::Boarding)], "nur die Begrüßung, 30 s nach dem Anstieg");
    }

    /// 2.1.1 A: schon voll geladen, nichts steigt → keine Begrüßung bis zum
    /// Beacon; dort kommt sie, danach „Boarding completed“ und „Türen zu“.
    #[test]
    fn vorab_geladen_begruessung_beim_beacon() {
        let mut p = Planer::fortsetzen(flug(120), Boarding);
        let mut takte: Vec<Tick> = (0..=20 * 60).step_by(10).map(|s| gate(s, Some(72_744.0), 2, false)).collect();
        takte.push(gate(20 * 60 + 10, Some(72_744.0), 2, true));
        let z = melde_zeiten(&mut p, takte);
        let beacon = 20 * 60 + 10;
        assert_eq!(
            z,
            vec![
                (beacon, Slot::Boarding),
                (beacon, Slot::BoardingAbgeschlossen),
                (beacon, Slot::KommandoTuerenScharf),
                (beacon, Slot::BegruessungTuerenZu),
                (beacon, Slot::Sicherheit),
            ]
        );
        assert!(p.entfallene_abholen().is_empty(), "nichts entfällt");
    }

    fn mit_ofp(mut t: Tick, plan: f64) -> Tick {
        t.planned_zfw_kg = Some(plan);
        t
    }

    /// Peter SVA 179 (20.10.2026, Flugprotokoll wZR0goDQ): beim Start die
    /// Standardladung des Add-ons (237.682 kg), nach 12,5 min „Load instant“
    /// herunter auf den Plan (230.463 kg), Beacon nach 23,2 min. Bisher kam
    /// die Begrüßung erst am Beacon (keine Boarding-Musik); jetzt 60 s nach
    /// der Sofortladung.
    #[test]
    fn peter_sva179_sofortladung_auf_den_plan_begruesst() {
        let plan = 230_463.0;
        let mut p = Planer::fortsetzen(flug(400), Boarding);
        let mut takte: Vec<Tick> = (0..750).step_by(10).map(|s| mit_ofp(gate(s, Some(237_682.0), 2, false), plan)).collect();
        takte.extend((750..1390).step_by(10).map(|s| mit_ofp(gate(s, Some(plan), 2, false), plan)));
        let z = melde_zeiten(&mut p, takte);
        assert_eq!(z, vec![(750 + BOARDING_OFP_S, Slot::Boarding)], "60 s nach der Sofortladung, nicht erst am Beacon");
        let f = slots(p.tick(&mit_ofp(gate(1390, Some(plan), 2, true), plan)));
        assert!(!f.contains(&Slot::Boarding) && f.contains(&Slot::BegruessungTuerenZu), "{f:?}");
    }

    /// 7LRplO7E (Pilot 30): beladen gestartet (nicht am Plan), nach 7,9 min
    /// entladen, danach GSX-Boarding. Die Begrüßung kommt mit dem Boarding,
    /// nicht beim Start und nicht beim Entladen. Ohne neuen Bezug nach dem
    /// Entladen käme sie erst, wenn GSX die alte Startladung übertrifft.
    #[test]
    fn entladen_setzt_den_bezug_neu_begruessung_mit_dem_boarding() {
        let plan = 39_600.0;
        let mut p = Planer::fortsetzen(flug(90), Boarding);
        let mut takte: Vec<Tick> = (0..480).step_by(10).map(|s| mit_ofp(gate(s, Some(36_400.0), 2, false), plan)).collect();
        takte.extend((480..1260).step_by(10).map(|s| mit_ofp(gate(s, Some(27_600.0), 2, false), plan)));
        // GSX: alle 60 s 500 kg mehr.
        takte.extend((1260..1500).step_by(10).map(|s| mit_ofp(gate(s, Some(27_600.0 + 500.0 * f64::from(((s - 1200) / 60) as i32)), 2, false), plan)));
        let z = melde_zeiten(&mut p, takte);
        assert_eq!(z, vec![(1260 + BOARDING_ZULADUNG_S, Slot::Boarding)]);
    }

    /// Codex 20.10.2026: Entladen zählt vom Höchstwert, nicht vom Start
    /// (50 t → 55 t für 20 s → 54 t: der laufende Anstieg zählt nicht weiter);
    /// ein einzelner Ausreißer nach unten setzt keinen neuen Bezug.
    #[test]
    fn entladen_vom_hoechstwert_und_ausreisser_nach_unten() {
        let mut p = Planer::fortsetzen(flug(60), Boarding);
        let mut takte = vec![gate(0, Some(50_000.0), 0, false)];
        takte.extend((10..=20).step_by(10).map(|s| gate(s, Some(55_000.0), 0, false)));
        takte.extend((30..=200).step_by(10).map(|s| gate(s, Some(54_000.0 - f64::from(s as i32) * 10.0), 0, false)));
        assert!(melde_zeiten(&mut p, takte).is_empty(), "während des Entladens keine Begrüßung");
        // Ausreißer: ein Takt 300 kg tiefer, dann zurück → Bezug bleibt 60 t.
        let mut q = Planer::fortsetzen(flug(60), Boarding);
        let mut takte: Vec<Tick> = (0..=50).step_by(10).map(|s| gate(s, Some(60_000.0), 0, false)).collect();
        takte.push(gate(60, Some(59_700.0), 0, false));
        takte.extend((70..=200).step_by(10).map(|s| gate(s, Some(60_000.0), 0, false)));
        assert!(melde_zeiten(&mut q, takte).is_empty(), "Rückkehr nach dem Ausreißer ist kein Anstieg");
    }

    /// Kleiner Plan: dort gilt die Mindesttoleranz 300 kg (1 % wären 200 kg),
    /// und die 60 s am Stück bei laufender Abtastung.
    #[test]
    fn am_ofp_mindesttoleranz_300_kg() {
        let plan = 20_000.0;
        let lauf = |zfw: f64| {
            let mut p = Planer::fortsetzen(flug(30), Boarding);
            melde_zeiten(&mut p, (0..=60).step_by(10).map(|s| mit_ofp(gate(s, Some(zfw), 0, false), plan)))
        };
        assert_eq!(lauf(20_290.0), vec![(60, Slot::Boarding)], "290 kg über dem Plan: innerhalb");
        assert_eq!(lauf(19_700.0), vec![(60, Slot::Boarding)], "genau 300 kg darunter: innerhalb");
        assert!(lauf(20_310.0).is_empty(), "310 kg: außerhalb");
    }

    /// 51KwpLwM (Pilot 30): 45 min leer am Gate, OFP vorhanden → nichts bis
    /// zum Beacon (kein Zeit-Rückfall, ein leeres Flugzeug wird nicht begrüßt).
    #[test]
    fn leer_am_gate_mit_ofp_begruesst_erst_am_beacon() {
        let plan = 240_000.0;
        let mut p = Planer::fortsetzen(flug(500), Boarding);
        let takte: Vec<Tick> = (0..2730).step_by(10).map(|s| mit_ofp(gate(s, Some(184_600.0), 2, false), plan)).collect();
        assert!(melde_zeiten(&mut p, takte).is_empty());
        let f = slots(p.tick(&mit_ofp(gate(2730, Some(184_600.0), 2, true), plan)));
        assert_eq!(&f[..2], &[Slot::Boarding, Slot::BoardingAbgeschlossen], "{f:?}");
    }

    /// Am Plan, aber nur 50 s (Durchgang beim Laden) → nichts; die 60 s
    /// zählen beim nächsten Erreichen von vorn. Grenze der Toleranz:
    /// max(300 kg, 1 %).
    #[test]
    fn am_ofp_braucht_60_s_am_stueck_und_die_toleranz() {
        let plan = 60_000.0;
        // Toleranz: 1 % = 600 kg (größer als 300 kg).
        let mut aussen = Planer::fortsetzen(flug(60), Boarding);
        aussen.tick(&mit_ofp(gate(0, Some(60_700.0), 0, false), plan));
        assert!(aussen.tick(&mit_ofp(gate(70, Some(60_700.0), 0, false), plan)).is_empty(), "700 kg über dem Plan");
        let mut innen = Planer::fortsetzen(flug(60), Boarding);
        innen.tick(&mit_ofp(gate(0, Some(60_550.0), 0, false), plan));
        assert_eq!(slots(innen.tick(&mit_ofp(gate(60, Some(60_550.0), 0, false), plan))), vec![Slot::Boarding], "550 kg");
        // 40 s am Plan, dann weg vom Plan (nach unten, also kein Anstieg) → nichts.
        let mut p = Planer::fortsetzen(flug(60), Boarding);
        let mut takte: Vec<Tick> = (0..=40).step_by(10).map(|s| mit_ofp(gate(s, Some(60_100.0), 0, false), plan)).collect();
        takte.push(mit_ofp(gate(50, Some(58_000.0), 0, false), plan));
        assert!(melde_zeiten(&mut p, takte).is_empty());
        assert!(p.tick(&mit_ofp(gate(60, Some(60_000.0), 0, false), plan)).is_empty(), "neuer Anlauf beginnt");
        assert_eq!(slots(p.tick(&mit_ofp(gate(120, Some(60_000.0), 0, false), plan))), vec![Slot::Boarding], "60 s ab dem neuen Erreichen");
    }

    /// 2.1.1 A: Zuladung kurz über der Schwelle, dann zurück → nichts; ein
    /// neuer Anstieg zählt die 30 s von vorn.
    #[test]
    fn ausreisser_ueber_der_schwelle_loest_nichts_aus() {
        let mut p = Planer::fortsetzen(flug(60), Boarding);
        let mut takte = vec![gate(0, Some(50_000.0), 0, false)];
        takte.extend((10..=20).step_by(10).map(|s| gate(s, Some(50_300.0), 0, false)));
        takte.push(gate(30, Some(50_100.0), 0, false));
        takte.extend((40..=60).step_by(10).map(|s| gate(s, Some(50_300.0), 0, false)));
        assert!(melde_zeiten(&mut p, takte).is_empty(), "20 s + 20 s sind keine 30 s am Stück");
        assert_eq!(slots(p.tick(&gate(70, Some(50_300.0), 0, false))), vec![Slot::Boarding], "30 s ab dem neuen Anstieg");
    }

    /// 2.1.1 A: kein ZFW- und kein Beacon-Signal → Begrüßung beim Pushback;
    /// abgewählt → nichts, aber „Türen zu“ läuft.
    #[test]
    fn ohne_signale_beim_pushback_abgewaehlt_nie() {
        let mut p = Planer::fortsetzen(flug(60), Boarding);
        let mut t = boden(0, Boarding);
        t.beacon = None;
        assert!(slots(p.tick(&t)).is_empty());
        t.jetzt = 600;
        assert!(slots(p.tick(&t)).is_empty());
        t.jetzt = 700;
        t.phase = Pushback;
        let f = slots(p.tick(&t));
        assert_eq!(&f[..2], &[Slot::Boarding, Slot::BoardingAbgeschlossen], "{f:?}");
        let mut q = Planer::fortsetzen(flug(60), Boarding);
        q.abwaehlen([Slot::Boarding]);
        q.tick(&gate(0, Some(50_000.0), 0, false));
        q.tick(&gate(10, Some(55_000.0), 0, false));
        assert!(slots(q.tick(&gate(40, Some(55_000.0), 0, false))).is_empty(), "abgewählt");
        let f = slots(q.tick(&gate(50, Some(55_000.0), 0, true)));
        assert!(!f.contains(&Slot::Boarding) && f.contains(&Slot::BegruessungTuerenZu), "{f:?}");
        assert!(q.entfallene_abholen().is_empty());
    }

    /// 2.1.1 A: Wiedereinstieg im Boarding nimmt den Bezug neu; ab Pushback
    /// gilt die Begrüßung als erledigt (kein Nachholen).
    #[test]
    fn wiedereinstieg_im_boarding_neuer_bezug_ab_pushback_erledigt() {
        let mut p = Planer::fortsetzen(flug(60), Boarding);
        p.tick(&gate(0, Some(60_000.0), 2, false));
        p.tick(&gate(10, Some(60_150.0), 2, false));
        assert!(slots(p.tick(&gate(60, Some(60_150.0), 2, false))).is_empty(), "+150 kg zum neuen Bezug");
        p.tick(&gate(70, Some(60_250.0), 2, false));
        assert_eq!(slots(p.tick(&gate(100, Some(60_250.0), 2, false))), vec![Slot::Boarding]);
        let mut n = Planer::fortsetzen(flug(60), Pushback);
        let mut pb = gate(0, Some(60_000.0), 2, true);
        pb.phase = Pushback;
        assert!(!n.tick(&pb).iter().any(|f| matches!(f.slot, Slot::Boarding | Slot::BoardingAbgeschlossen)));
    }

    /// Michael BER181: kein Pushback (aus eigener Kraft zum Rollen) → der
    /// „Türen zu“-Block kam erst beim Losrollen. Jetzt: Beacon an.
    #[test]
    fn tueren_zu_beim_beacon_sonst_pushback_oder_rollen() {
        let mut p = Planer::neu(flug(120));
        p.tick(&boden(0, Boarding));
        let mut beacon = boden(600, Boarding);
        beacon.beacon = Some(true);
        let f = slots(p.tick(&beacon));
        assert!(f.contains(&Slot::KommandoTuerenScharf) && f.contains(&Slot::BegruessungTuerenZu) && f.contains(&Slot::Sicherheit), "{f:?}");
        // Gegenprobe: Beacon schon beim Laden an → keine Flanke, erst beim Rollen.
        let mut q = Planer::neu(flug(120));
        let mut an = boden(0, Boarding);
        an.beacon = Some(true);
        q.tick(&an);
        an.jetzt = 600;
        assert!(!slots(q.tick(&an)).contains(&Slot::BegruessungTuerenZu));
        let mut rollen = boden(700, TaxiOut);
        rollen.beacon = Some(true);
        assert!(slots(q.tick(&rollen)).contains(&Slot::BegruessungTuerenZu));
    }

    #[test]
    fn plaetze_fuer_den_start_beim_landelicht_sonst_startlauf() {
        let mut p = Planer::neu(flug(120));
        p.tick(&boden(0, TaxiOut));
        let mut licht = boden(60, TaxiOut);
        licht.landelicht = Some(true);
        assert!(slots(p.tick(&licht)).contains(&Slot::KommandoStart));
        // Licht schon beim Pushback an → keine Flanke beim Rollen, Startlauf.
        let mut q = Planer::neu(flug(120));
        let mut an = boden(0, Pushback);
        an.landelicht = Some(true);
        q.tick(&an);
        an.phase = TaxiOut;
        an.jetzt = 60;
        assert!(!slots(q.tick(&an)).contains(&Slot::KommandoStart));
        an.phase = TakeoffRoll;
        an.jetzt = 600;
        assert!(slots(q.tick(&an)).contains(&Slot::KommandoStart));
    }

    /// Michael BER181: Fahrwerk bei 2.179 ft, Final erst bei 645 ft.
    #[test]
    fn plaetze_fuer_die_landung_beim_fahrwerk_oder_unter_2000_ft() {
        let mut p = Planer::fortsetzen(flug(60), Approach);
        let mut a = tick(0, Approach, 2_500.0, -800.0);
        a.fahrwerk_unten = Some(false);
        a.hoehe_agl_ft = Some(2_300.0);
        assert!(!slots(p.tick(&a)).contains(&Slot::KommandoPlaetzeLandung));
        a.jetzt = 10;
        a.fahrwerk_unten = Some(true);
        a.hoehe_agl_ft = Some(2_179.0);
        assert!(slots(p.tick(&a)).contains(&Slot::KommandoPlaetzeLandung), "Fahrwerk raus");
        // Ohne Fahrwerksmeldung: unter 2.000 ft im Anflug.
        let mut q = Planer::fortsetzen(flug(60), Approach);
        let mut b = tick(0, Approach, 2_200.0, -800.0);
        b.hoehe_agl_ft = Some(2_100.0);
        assert!(!slots(q.tick(&b)).contains(&Slot::KommandoPlaetzeLandung));
        b.jetzt = 10;
        b.hoehe_agl_ft = Some(1_900.0);
        assert!(slots(q.tick(&b)).contains(&Slot::KommandoPlaetzeLandung));
    }

    /// Codex 2.0.27: Mit TOD aus dem OFP und kurzem Sinkflug liegt die
    /// Restzeit beim Bordverkauf-Ende unter 35 min — sie kommt trotzdem.
    #[test]
    fn reiseflug80_kommt_auch_mit_kurzer_restzeit() {
        let mut f = flug(120);
        f.abheben_epoch = Some(0);
        f.tod_nach_abheben_s = Some(100 * 60);
        let mut p = Planer::fortsetzen(f, Cruise);
        p.gemeldet.insert(Slot::Reiseflug25);
        p.gemeldet.insert(Slot::Reiseflug50);
        let t = 100 * 60 - REISEFLUG80_VOR_TOD_S;
        let mut tk = tick(t, Cruise, 37_000.0, 0.0);
        tk.anschnallzeichen = Some(0);
        tk.rest_min = Some(32.0);
        assert!(slots(p.tick(&tk)).contains(&Slot::Reiseflug80));
        // Gegenprobe: unter der Reserve entfällt sie sichtbar.
        let mut q = Planer::fortsetzen(Flugdaten { tod_nach_abheben_s: Some(100 * 60), abheben_epoch: Some(0), ..flug(120) }, Cruise);
        q.gemeldet.insert(Slot::Reiseflug25);
        q.gemeldet.insert(Slot::Reiseflug50);
        tk.rest_min = Some(25.0);
        assert!(!slots(q.tick(&tk)).contains(&Slot::Reiseflug80));
        assert_eq!(q.entfallene_abholen(), vec![(Slot::Reiseflug80, Entfall::ZuWenigZeit)]);
    }

    /// Thomas 18.10.2026: Bordverkauf-Ende 10–15 min vor Top of Descent.
    #[test]
    fn reiseflug80_zwoelf_minuten_vor_tod() {
        let mut f = flug(166);
        f.abheben_epoch = Some(0);
        let p = Planer::fortsetzen(f, Cruise);
        let tod = (166.0 - SINKFLUG_MIN) as i64 * 60;
        assert_eq!(p.zeitpunkt(Slot::Reiseflug80, None), Some(tod - REISEFLUG80_VOR_TOD_S));
    }

    /// Kapitänsbegrüßung verfällt nicht mehr beim Startlauf (Host spielt sie
    /// notfalls nach dem Start über 10.000 ft).
    #[test]
    fn kapitaen_begruessung_kommt_auch_nach_dem_start_noch() {
        let mut p = Planer::neu(flug(120));
        let f = p.tick(&boden(0, TaxiOut));
        let k = f.iter().find(|x| x.slot == Slot::KapitaenBegruessung).expect("gemeldet");
        assert_eq!(k.verfall, Verfall::AbPhase(Descent));
        assert!(Planer::neu(flug(120)).kommt_noch(Slot::KapitaenBegruessung, Climb));
    }

    /// Abgewählte Ansagen: kommen nicht, die Abläufe dahinter laufen weiter
    /// (Advisor beta.7).
    #[test]
    fn abgewaehlte_ansagen_halten_die_ablaeufe_nicht_auf() {
        // „Anschnallzeichen aus“ abgewählt: Kurzstrecken-Service kommt trotzdem
        // erst, wenn das Zeichen erlischt.
        let mut p = Planer::neu(flug(45));
        p.abwaehlen([Slot::AnschnallzeichenAus]);
        assert!(!p.kommt_noch(Slot::AnschnallzeichenAus, Climb));
        p.tick(&tick(0, TakeoffRoll, 0.0, 0.0));
        let mut c = tick(60, Climb, 5_000.0, 2_000.0);
        c.rest_min = Some(40.0);
        assert!(slots(p.tick(&c)).is_empty(), "Zeichen noch an");
        c.jetzt = 120;
        c.anschnallzeichen = Some(0);
        assert_eq!(slots(p.tick(&c)), vec![Slot::Reiseflug25], "Service ohne die abgewählte Ansage");
        // Kapitän-Sinkflug abgewählt: Die Kabine richtet sich nach der Höhe.
        let mut q = Planer::fortsetzen(flug(120), Cruise);
        q.abwaehlen([Slot::KapitaenSinkflug]);
        assert!(slots(q.tick(&tick(0, Descent, 30_000.0, -2_000.0))).iter().all(|s| *s != Slot::KapitaenSinkflug));
        assert!(slots(q.tick(&tick(30, Descent, 14_000.0, -2_000.0))).contains(&Slot::KabineSinkflug));
    }

    #[test]
    fn anschnallzeichen_auto_gilt_ueber_10000_als_aus() {
        let mut p = Planer::neu(flug(120));
        let mut tk = tick(1, Climb, 9_000.0, 2000.0);
        tk.anschnallzeichen = Some(1);
        assert!(!p.tick(&tk).iter().any(|f| f.slot == Slot::AnschnallzeichenAus));
        tk.jetzt = 2;
        tk.hoehe_ft = 10_500.0;
        assert!(p.tick(&tk).iter().any(|f| f.slot == Slot::AnschnallzeichenAus));
    }

    #[test]
    fn turbulenz_kommt_und_geht_mit_mindestabstand() {
        let mut p = Planer::neu(flug(240));
        p.gemeldet.insert(Slot::AnschnallzeichenAus);
        let zaehle = |p: &mut Planer, t: i64, z: u8| {
            let mut tk = tick(t, Cruise, 37_000.0, 0.0);
            tk.anschnallzeichen = Some(z);
            tk.rest_min = Some(200.0);
            p.tick(&tk).into_iter().filter(|f| matches!(f.slot, Slot::Turbulenz | Slot::TurbulenzVorbei)).map(|f| f.slot).collect::<Vec<_>>()
        };
        assert!(zaehle(&mut p, 0, 0).is_empty());
        assert_eq!(zaehle(&mut p, 10, 2), vec![Slot::Turbulenz]);
        assert!(zaehle(&mut p, 20, 2).is_empty());
        assert_eq!(zaehle(&mut p, 30, 0), vec![Slot::TurbulenzVorbei]);
        // Gleich wieder an: unter 10 min → keine zweite Ansage.
        assert!(zaehle(&mut p, 40, 2).is_empty());
    }

    #[test]
    fn verspaetung_nur_im_plausiblen_bereich() {
        for (abw, soll) in [(10, false), (20, true), (179, true), (300, false)] {
            let mut p = Planer::neu(flug(120));
            let mut tk = tick(1, Boarding, 0.0, 0.0);
            tk.abweichung_off_block_min = Some(abw);
            let da = p.tick(&tk).iter().any(|f| f.slot == Slot::VerspaetungGate);
            assert_eq!(da, soll, "Abweichung {abw}");
        }
    }

    #[test]
    fn warteschleife_erst_nach_zwei_minuten_und_einmal_je_schleife() {
        let mut p = Planer::neu(flug(120));
        let mut n = 0;
        for t in (0..600).step_by(10) {
            n += p.tick(&tick(t, Holding, 8_000.0, 0.0)).iter().filter(|f| f.slot == Slot::Warteschleife).count();
            if t == 110 {
                assert_eq!(n, 0);
            }
        }
        assert_eq!(n, 1);
    }

    #[test]
    fn durchstarten_kommt_verzoegert_und_kommando_darf_nochmal() {
        let mut p = Planer::neu(flug(120));
        p.steigflug_vorbei = true;
        let f = p.tick(&tick(0, Final, 800.0, -700.0));
        assert!(f.iter().any(|f| f.slot == Slot::KommandoPlaetzeLandung));
        assert!(p.tick(&tick(10, Climb, 1500.0, 2000.0)).iter().all(|f| f.slot != Slot::Durchstarten));
        let mut gesehen = false;
        for t in (20..400).step_by(10) {
            gesehen |= p.tick(&tick(t, Climb, 3000.0, 1000.0)).iter().any(|f| f.slot == Slot::Durchstarten);
            if t < 160 {
                assert!(!gesehen, "zu früh bei {t}");
            }
        }
        assert!(gesehen);
        assert!(p.tick(&tick(500, Final, 800.0, -700.0)).iter().any(|f| f.slot == Slot::KommandoPlaetzeLandung));
    }

    /// Nach dem Durchstarten (wie in echt): Die Kabine bleibt vorbereitet,
    /// der Kapitän erklärt das Durchstarten, vor der zweiten Landung kommt
    /// nur „Plätze für die Landung“ noch einmal – nicht „prepare for
    /// landing“, nicht die Landevorbereitung der Kabine, nicht der Kapitän.
    #[test]
    fn durchstarten_wiederholt_nur_plaetze_fuer_die_landung() {
        let mut p = Planer::fortsetzen(flug(120), Cruise);
        let mut erst = Vec::new();
        for (s, ph, h, vs) in [(0, Descent, 30_000.0, -2_000.0), (400, Descent, 14_000.0, -2_000.0), (700, Approach, 4_000.0, -800.0), (900, Final, 800.0, -700.0)] {
            for f in p.tick(&tick(s, ph, h, vs)) {
                if f.slot == Slot::KapitaenSinkflug {
                    p.kapitaen_sinkflug_ende(true, s + 40);
                }
                erst.push(f.slot);
            }
        }
        for x in [Slot::KapitaenSinkflug, Slot::KabineSinkflug, Slot::KommandoLandungVorbereiten, Slot::KommandoPlaetzeLandung] {
            assert!(erst.contains(&x), "erster Anflug ohne {x:?}: {erst:?}");
        }
        let mut zweit = Vec::new();
        for s in (910..1_300).step_by(10) {
            zweit.extend(p.tick(&tick(s, Climb, 3_000.0, 1_000.0)).into_iter().map(|f| f.slot));
        }
        for (s, ph, h) in [(1_400, Approach, 3_000.0), (1_600, Final, 800.0)] {
            zweit.extend(p.tick(&tick(s, ph, h, -700.0)).into_iter().map(|f| f.slot));
        }
        assert!(zweit.contains(&Slot::Durchstarten), "{zweit:?}");
        assert!(zweit.contains(&Slot::KommandoPlaetzeLandung), "{zweit:?}");
        for x in [Slot::KapitaenSinkflug, Slot::KabineSinkflug, Slot::KommandoLandungVorbereiten] {
            assert!(!zweit.contains(&x), "nach dem Durchstarten nicht noch einmal {x:?}: {zweit:?}");
        }
    }

    /// 20.10.2026: Der Kapitän sprach im Reiseflug (Restzeit-Schätzung),
    /// danach blieb das Flugzeug 15 min auf Höhe. Die Landevorbereitung der
    /// Kabine kam bisher nach 5 min mitten im Reiseflug (6 von 19 Flügen, bis
    /// 29 min vor dem Sinkflug). Jetzt wartet sie auf den Sinkflug.
    #[test]
    fn kabine_wartet_im_reiseflug_auf_den_sinkflug() {
        let mut p = Planer::fortsetzen(flug(120), Cruise);
        let mut tk = tick(0, Cruise, 35_000.0, 0.0);
        tk.rest_min = Some(KAPITAEN_VOR_LANDUNG_MIN);
        assert!(slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug));
        p.kapitaen_sinkflug_ende(true, 40);
        for s in (50..900).step_by(10) {
            assert!(!slots(p.tick(&tick(s, Cruise, 35_000.0, 0.0))).contains(&Slot::KabineSinkflug), "nicht im Reiseflug ({s})");
        }
        // Seit 22.10.2026 auch nicht gleich mit dem Sinkflug, erst tiefer.
        assert!(!slots(p.tick(&tick(900, Descent, 34_000.0, -1_500.0))).contains(&Slot::KabineSinkflug), "nicht auf 34.000 ft");
        assert!(slots(p.tick(&tick(1_500, Descent, 19_500.0, -1_500.0))).contains(&Slot::KabineSinkflug), "unter 20.000 ft");
    }

    /// Thomas 22.10.2026: „Was passiert, wenn wir keine 20.000 fliegen?“ —
    /// Reiseflug auf FL150: Die Kabine wartet auf Reiseflughöhe, kommt mit dem
    /// Sinkflug (≥ 2 min nach dem Kapitän), im Anflug sofort.
    #[test]
    fn kabine_bei_reiseflug_unter_20000_ft() {
        let mut niedrig = flug(60);
        niedrig.reiseflughoehe_ft = Some(15_000);
        let kabine = |p: &mut Planer, t: i64, ph: FlightPhase, h: f64, vs: f64| p.tick(&tick(t, ph, h, vs)).iter().any(|f| f.slot == Slot::KabineSinkflug);
        let mut p = Planer::fortsetzen(niedrig.clone(), Cruise);
        p.kapitaen_sinkflug_ende(true, 100);
        assert!(!kabine(&mut p, 400, Cruise, 15_000.0, 0.0), "auf Reiseflughöhe noch nicht");
        assert!(kabine(&mut p, 460, Descent, 14_500.0, -1_200.0), "mit dem Sinkflug");
        // Kapitän gerade erst fertig: die 2 min gelten auch hier.
        let mut q = Planer::fortsetzen(niedrig.clone(), Cruise);
        q.kapitaen_sinkflug_ende(true, 400);
        assert!(!kabine(&mut q, 460, Descent, 14_500.0, -1_200.0));
        assert!(kabine(&mut q, 520, Descent, 13_000.0, -1_200.0));
        // Sehr kurzer Sinkflug, schon im Anflug: sofort, nicht verloren.
        let mut r = Planer::fortsetzen(niedrig, Cruise);
        assert!(kabine(&mut r, 400, Approach, 4_000.0, -800.0));
    }

    /// 20.10.2026: Der TOD aus der Position auf der Route schlägt die starre
    /// OFP-Uhrzeit (Abheben + TOD-Abstand), die bis zu 35 min zu früh kam.
    /// Verlässt der Flug kurz die Route, gilt der zuletzt bekannte TOD weiter.
    #[test]
    fn kapitaen_folgt_dem_tod_aus_der_route() {
        let mut f = flug(120);
        f.abheben_epoch = Some(0);
        f.tod_nach_abheben_s = Some(3_000);
        let mut p = Planer::fortsetzen(f.clone(), Cruise);
        // OFP-Uhrzeit sagt TOD 3000 (Kapitän ab 2700), die Route sagt 4000.
        for s in (2_600..4_000 - KAPITAEN_VOR_TOD_S).step_by(10) {
            let mut tk = tick(s, Cruise, 35_000.0, 0.0);
            // Zwischen 3000 und 3200 neben der Route (Direct): kein Wert.
            tk.tod_erwartet = (!(3_000..3_200).contains(&s)).then_some(4_000);
            assert!(!slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug), "nicht vor TOD − 90 s der Route ({s})");
            assert_eq!(p.zeitpunkt(Slot::KapitaenSinkflug, None), Some(4_000 - KAPITAEN_VOR_TOD_S), "Anzeige wie Auslöser ({s})");
        }
        let mut tk = tick(4_000 - KAPITAEN_VOR_TOD_S, Cruise, 35_000.0, 0.0);
        tk.tod_erwartet = Some(4_000);
        assert!(slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug));
        // Ohne Route löst die starre OFP-Uhrzeit nicht mehr aus; es bleiben
        // Restzeit und Sinkflug.
        let mut q = Planer::fortsetzen(f.clone(), Cruise);
        assert!(!slots(q.tick(&tick(2_700, Cruise, 35_000.0, 0.0))).contains(&Slot::KapitaenSinkflug));
        assert!(slots(q.tick(&tick(2_800, Descent, 34_000.0, -1_500.0))).contains(&Slot::KapitaenSinkflug));
        // Länger als 10 min ohne Route: Der alte TOD gilt nicht mehr.
        let mut r = Planer::fortsetzen(f.clone(), Cruise);
        let mut tk = tick(1_000, Cruise, 35_000.0, 0.0);
        tk.tod_erwartet = Some(2_000);
        r.tick(&tk);
        assert!(!slots(r.tick(&tick(1_000 + TOD_FRISCH_S + 10, Cruise, 35_000.0, 0.0))).contains(&Slot::KapitaenSinkflug), "veralteter TOD löst nicht aus");
        // Umleitung: TOD und Landezeit des Hosts gelten der alten Route –
        // auch nicht im selben Takt, in dem der alte TOD fällig würde.
        let mut u = Planer::fortsetzen(f, Cruise);
        let mut tk = tick(1_000, Cruise, 35_000.0, 0.0);
        tk.tod_erwartet = Some(1_500);
        u.tick(&tk);
        let mut tk = tick(1_300, Cruise, 35_000.0, 0.0);
        tk.ziel_icao = Some("LFBD".into());
        tk.tod_erwartet = Some(1_500);
        tk.rest_min = Some(20.0);
        let f = slots(u.tick(&tk));
        assert!(f.contains(&Slot::Umleitung) && !f.contains(&Slot::KapitaenSinkflug), "{f:?}");
        assert_eq!(u.zeitpunkt(Slot::KapitaenSinkflug, Some(1_300 + 20 * 60)), None);
        tk.jetzt = 1_400;
        assert!(!slots(u.tick(&tk)).contains(&Slot::KapitaenSinkflug), "alte Route zählt nicht mehr");
        let mut tk = tick(1_500, Descent, 34_000.0, -1_500.0);
        tk.ziel_icao = Some("LFBD".into());
        assert!(slots(u.tick(&tk)).contains(&Slot::KapitaenSinkflug), "mit dem Sinkflug");
        // Zurück zum Planziel (oder dasselbe Ziel mit Leerzeichen): Die Route
        // zählt wieder.
        let mut z = Planer::fortsetzen(flug(120), Cruise);
        let mut tk = tick(1_000, Cruise, 35_000.0, 0.0);
        tk.ziel_icao = Some("LFBD".into());
        z.tick(&tk);
        let mut tk = tick(1_010, Cruise, 35_000.0, 0.0);
        tk.ziel_icao = Some(" lfbo ".into());
        tk.rest_min = Some(20.0);
        assert!(slots(z.tick(&tk)).contains(&Slot::KapitaenSinkflug), "Planziel wieder: Restzeit zählt");
    }

    /// Codex 20.10.2026: phase_v2 kann aus dem Steigflug direkt in den Anflug
    /// gehen (kurzer Flug, kein erkannter Sinkflug). Der Steigflug ist dann
    /// trotzdem vorbei, sonst blieben Kapitän und Kabine stumm.
    #[test]
    fn steigflug_direkt_in_den_anflug() {
        let mut p = Planer::neu(flug(40));
        p.tick(&tick(0, Takeoff, 700.0, 3_000.0));
        p.tick(&tick(10, Climb, 8_000.0, 1_500.0));
        let f = slots(p.tick(&tick(600, Approach, 4_800.0, -1_000.0)));
        assert!(f.contains(&Slot::KapitaenSinkflug), "{f:?}");
    }

    /// Sehr kurzer Anflug: Kapitän gerade fertig, Reiseflug → Anflug → Final
    /// innerhalb von 2 min. Die Kabine hält ihren Abstand zum Kapitän und
    /// entfällt dann (bisheriges Verhalten, festgehalten).
    #[test]
    fn sehr_kurzer_anflug_kabine_entfaellt() {
        let mut p = Planer::fortsetzen(flug(60), Cruise);
        let mut tk = tick(0, Cruise, 12_000.0, 0.0);
        tk.rest_min = Some(10.0);
        assert!(slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug));
        p.kapitaen_sinkflug_ende(true, 30);
        let mut alle = slots(p.tick(&tick(60, Approach, 4_000.0, -1_000.0)));
        alle.extend(slots(p.tick(&tick(120, Final, 900.0, -800.0))));
        assert!(!alle.contains(&Slot::KabineSinkflug), "{alle:?}");
        assert!(alle.contains(&Slot::KommandoLandungVorbereiten), "Kommando trotzdem: {alle:?}");
    }

    #[test]
    fn umleitung_einmal() {
        let mut p = Planer::neu(flug(120));
        let mut tk = tick(1, Cruise, 30_000.0, 0.0);
        tk.ziel_icao = Some("LFBD".into());
        assert!(p.tick(&tk).iter().any(|f| f.slot == Slot::Umleitung));
        tk.jetzt = 2;
        assert!(!p.tick(&tk).iter().any(|f| f.slot == Slot::Umleitung));
    }

    #[test]
    fn ruhephase_niedriger_reiseflug_bleibt_hoerbar() {
        let mut f = flug(45);
        f.reiseflughoehe_ft = Some(9_000);
        let p = Planer::neu(f);
        let mut tk = tick(0, Climb, 6_000.0, 1000.0);
        assert!(!p.ruhephase(&tk));
        tk.phase = Final;
        assert!(p.ruhephase(&tk));
        let hoch = Planer::neu(flug(120));
        tk.phase = Climb;
        assert!(hoch.ruhephase(&tk));
    }

    #[test]
    fn verspaeteter_start_verschiebt_den_tod_mit() {
        // TOD laut Route 80 min nach dem Abheben; abgehoben wird 2 h nach
        // Plan. Seit 20.10.2026 kommt der TOD aus der Position auf der Route.
        let mut f = flug(120);
        f.tod_nach_abheben_s = Some(80 * 60);
        let mut p = Planer::neu(f);
        let ab = 7_200;
        let sinkflug = |p: &mut Planer, jetzt: i64, phase| {
            let mut tk = tick(jetzt, phase, 36_000.0, 0.0);
            tk.tod_erwartet = (phase != Takeoff).then_some(ab + 80 * 60);
            p.tick(&tk).iter().any(|f| f.slot == Slot::KapitaenSinkflug)
        };
        assert!(!sinkflug(&mut p, ab, Takeoff));
        assert!(!sinkflug(&mut p, ab + 20 * 60, Cruise), "kurz nach dem Abheben noch kein Sinkflug");
        assert!(!sinkflug(&mut p, ab + 70 * 60, Cruise));
        assert!(!sinkflug(&mut p, ab + 76 * 60, Cruise), "5 min vor dem TOD ist seit 22.10.2026 zu früh");
        assert!(sinkflug(&mut p, ab + 79 * 60, Cruise), "90 s vor dem TOD");
    }

    #[test]
    fn fortsetzen_beim_start_wie_neu() {
        // Der Client legt den Planer immer mit `fortsetzen` an, auch beim
        // normalen Start im Boarding. Das muss denselben Ablauf ergeben.
        for min in [45, 75, 120] {
            let neu = normalflug(min);
            for phase in [Preflight, Boarding] {
                assert_eq!(normalflug_mit(Planer::fortsetzen(flug(min), phase), min), neu, "{min} min, {phase:?}");
            }
        }
    }

    #[test]
    fn wiedereinstieg_im_anflug_laesst_umleitung_zu() {
        let mut p = Planer::fortsetzen(flug(120), Approach);
        let mut tk = tick(1_000, Approach, 4_000.0, -800.0);
        tk.ziel_icao = Some("LFBD".into());
        assert!(p.tick(&tk).iter().any(|f| f.slot == Slot::Umleitung));
    }

    #[test]
    fn neues_umleitungsziel_wird_noch_einmal_angesagt() {
        let mut p = Planer::neu(flug(120));
        let mut tk = tick(1, Cruise, 30_000.0, 0.0);
        tk.ziel_icao = Some("LFBD".into());
        assert!(p.tick(&tk).iter().any(|f| f.slot == Slot::Umleitung));
        tk.jetzt = 2;
        assert!(!p.tick(&tk).iter().any(|f| f.slot == Slot::Umleitung), "dasselbe Ziel nicht zweimal");
        tk.jetzt = 3;
        tk.ziel_icao = Some("LFBO".into());
        assert!(!p.tick(&tk).iter().any(|f| f.slot == Slot::Umleitung), "zurück zum Plan: keine Umleitung");
        tk.jetzt = 4;
        tk.ziel_icao = Some("LFMT".into());
        assert!(p.tick(&tk).iter().any(|f| f.slot == Slot::Umleitung), "neues Ziel: neue Ansage");
    }

    /// 2.1.1 E (Entscheidung 5): „Boarding completed“ kurz vor „Türen zu“,
    /// nach der Begrüßung, auf jedem Flug — ohne OFP, ohne Ladeblatt.
    #[test]
    fn boarding_completed_beim_beacon_auf_jedem_flug() {
        // Begrüßung lief schon (Zuladung stieg): beim Beacon zuerst completed.
        let mut p = Planer::fortsetzen(flug(40), Boarding);
        p.tick(&gate(0, Some(49_203.0), 0, false));
        p.tick(&gate(10, Some(52_000.0), 0, false));
        assert_eq!(slots(p.tick(&gate(40, Some(60_000.0), 0, false))), vec![Slot::Boarding]);
        assert!(slots(p.tick(&gate(600, Some(72_744.0), 0, false))).is_empty(), "nicht beim Ende der Beladung");
        let f = slots(p.tick(&gate(900, Some(72_744.0), 0, true)));
        assert_eq!(&f[..2], &[Slot::BoardingAbgeschlossen, Slot::KommandoTuerenScharf], "{f:?}");
        assert!(slots(p.tick(&gate(910, Some(72_744.0), 0, true))).is_empty(), "nur einmal");
        // Ohne ZFW (kein OFP, kein Signal): genauso.
        let mut o = Planer::fortsetzen(flug(40), Boarding);
        o.tick(&gate(0, None, 0, false));
        assert!(slots(o.tick(&gate(400, None, 0, true))).contains(&Slot::BoardingAbgeschlossen));
        assert!(o.entfallene_abholen().is_empty());
        // Bizjet: gibt es nicht.
        let mut b = flug(40);
        b.flugart = Some(Flugart::Bizjet);
        let mut z = Planer::fortsetzen(b, Boarding);
        z.tick(&gate(0, Some(72_744.0), 0, false));
        assert!(!z.tick(&gate(400, Some(72_744.0), 0, true)).iter().any(|f| f.slot == Slot::BoardingAbgeschlossen));
    }

    /// v2.0.28, Michael LTU181: Schalter auf AUS unter FL100 → die Ansage
    /// darf die Ruhephase durchbrechen; AUTO wartet weiter auf FL100.
    #[test]
    fn anschnallzeichen_aus_vom_schalter_traegt_die_ausnahme() {
        let mut p = Planer::neu(flug(40));
        let mut tk = tick(0, Climb, 6_000.0, 2_000.0);
        tk.anschnallzeichen = Some(0);
        let f = p.tick(&tk);
        let aus = f.iter().find(|x| x.slot == Slot::AnschnallzeichenAus).expect("gemeldet");
        assert!(aus.trotz_ruhephase);
        assert!(p.ruhephase(&tk), "die Ruhephase selbst gilt weiter");
        let mut q = Planer::neu(flug(40));
        let mut auto = tick(0, Climb, 10_200.0, 2_000.0);
        auto.anschnallzeichen = Some(1);
        let f = q.tick(&auto);
        assert!(!f.iter().find(|x| x.slot == Slot::AnschnallzeichenAus).expect("über FL100").trotz_ruhephase);
    }

    /// v2.0.28, Peter: erster Service 10 min nach Erreichen der Reiseflughöhe
    /// statt bei 25 % (AFR218: +121 min).
    #[test]
    fn erster_service_kurz_nach_erreichen_der_reiseflughoehe() {
        let mut f = flug(480);
        f.abheben_epoch = Some(0);
        let mut p = Planer::neu(f);
        p.gemeldet.insert(Slot::AnschnallzeichenAus);
        p.tick(&tick(1_400, Climb, 35_000.0, 1_000.0));
        assert!(slots(p.tick(&tick(1_500, Cruise, 37_000.0, 0.0))).is_empty());
        assert_eq!(p.zeitpunkt(Slot::Reiseflug25, None), Some(1_500 + SERVICE_NACH_REISEFLUG_S));
        assert!(slots(p.tick(&tick(1_500 + SERVICE_NACH_REISEFLUG_S - 10, Cruise, 37_000.0, 0.0))).is_empty());
        assert_eq!(slots(p.tick(&tick(1_500 + SERVICE_NACH_REISEFLUG_S, Cruise, 37_000.0, 0.0))), vec![Slot::Reiseflug25]);
        // Reiseflug 50 bleibt bei 50 %, ohne Reiseflug 25 als übersprungen zu melden.
        let bis_tod = (480.0 - SINKFLUG_MIN) * 60.0;
        let r50 = (0.5 * bis_tod) as i64;
        assert_eq!(slots(p.tick(&tick(r50, Cruise, 37_000.0, 0.0))), vec![Slot::Reiseflug50]);
        assert!(p.entfallene_abholen().is_empty());
        // Kurzstrecke unverändert: Service am Anschnallzeichen, nicht an der Höhe.
        let mut k = Planer::neu(flug(40));
        k.tick(&tick(0, Climb, 20_000.0, 1_000.0));
        assert!(!k.tick(&tick(SERVICE_NACH_REISEFLUG_S + 100, Cruise, 29_000.0, 0.0)).iter().any(|x| x.slot == Slot::Reiseflug25));
    }

    /// v2.0.28, Peter DLH454: Licht ging an, bevor AeroACARS „Rollen“
    /// erkannte — keine Flanke im TaxiOut, das Kommando kam im Startlauf.
    #[test]
    fn plaetze_fuer_den_start_am_zustand_des_landelichts() {
        // 2.1.1 B (Nachtrag Thomas): Licht im Pushback an, rückwärts rollen →
        // kein Kommando; erst beim Rollen (TaxiOut) ab 3 kt.
        let mut p = Planer::neu(flug(120));
        p.tick(&boden(0, Pushback));
        let mut licht = boden(10, Pushback);
        licht.landelicht = Some(true);
        licht.bodengeschwindigkeit_kt = Some(4.0);
        assert!(!p.tick(&licht).iter().any(|f| f.slot == Slot::KommandoStart), "nicht im Pushback, auch rollend");
        licht.phase = TaxiOut;
        licht.jetzt = 20;
        licht.bodengeschwindigkeit_kt = Some(0.0);
        assert!(!p.tick(&licht).iter().any(|f| f.slot == Slot::KommandoStart), "Rollen erkannt, steht aber noch");
        licht.jetzt = 25;
        licht.bodengeschwindigkeit_kt = Some(2.9);
        assert!(!p.tick(&licht).iter().any(|f| f.slot == Slot::KommandoStart), "unter 3 kt rollt es nicht");
        licht.jetzt = 30;
        licht.bodengeschwindigkeit_kt = Some(ROLLT_AB_KT);
        assert!(p.tick(&licht).iter().any(|f| f.slot == Slot::KommandoStart), "Rollen und rollt (ab 3 kt)");
        // Rollen ohne Geschwindigkeitssignal (oder unbrauchbar): die Phase zählt allein.
        for gs in [None, Some(f64::NAN), Some(-1.0)] {
            let mut r = Planer::neu(flug(120));
            r.tick(&boden(0, TaxiOut));
            let mut l = boden(10, TaxiOut);
            l.landelicht = Some(true);
            l.bodengeschwindigkeit_kt = gs;
            assert!(r.tick(&l).iter().any(|f| f.slot == Slot::KommandoStart), "{gs:?}");
        }
        // Brennt das Licht schon am Gate und bleibt an: erst im Startlauf.
        let mut q = Planer::neu(flug(120));
        let mut gate = boden(0, Boarding);
        gate.landelicht = Some(true);
        q.tick(&gate);
        gate.phase = TaxiOut;
        gate.jetzt = 100;
        assert!(!q.tick(&gate).iter().any(|f| f.slot == Slot::KommandoStart));
        gate.phase = TakeoffRoll;
        gate.jetzt = 200;
        assert!(q.tick(&gate).iter().any(|f| f.slot == Slot::KommandoStart));
    }

    /// v2.0.28, A346 B78DQ…: Halt auf dem Rollweg mit laufenden Triebwerken
    /// ist kein „am Stand“ für die Türen.
    #[test]
    fn tueren_entschaerfen_erst_bei_stehenden_triebwerken() {
        let mut p = Planer::neu(flug(120));
        let mut halt = boden(0, BlocksOn);
        halt.triebwerke_an = Some(true);
        assert!(!p.tick(&halt).iter().any(|f| f.slot == Slot::KommandoTuerenEntschaerfen));
        halt.triebwerke_an = Some(false);
        halt.jetzt = 60;
        assert!(p.tick(&halt).iter().any(|f| f.slot == Slot::KommandoTuerenEntschaerfen));
        // Ohne Signal wie bisher; spätestens bei „Angekommen“.
        let mut q = Planer::neu(flug(120));
        assert!(q.tick(&boden(0, BlocksOn)).iter().any(|f| f.slot == Slot::KommandoTuerenEntschaerfen));
        let mut r = Planer::neu(flug(120));
        let mut an = boden(0, Arrived);
        an.triebwerke_an = Some(true);
        assert!(r.tick(&an).iter().any(|f| f.slot == Slot::KommandoTuerenEntschaerfen));
    }

    /// v2.0.28 (Thomas 19.10.): Der zweite Service beginnt ~90 min vor dem
    /// Sinkflug, das Ende des Bordverkaufs bleibt 12 min davor.
    #[test]
    fn zweiter_service_neunzig_minuten_vor_dem_sinkflug() {
        let mut f = flug(480);
        f.abheben_epoch = Some(0);
        f.tod_nach_abheben_s = Some(450 * 60);
        let mut p = Planer::fortsetzen(f.clone(), Cruise);
        p.reiseflug80_art(Some(true));
        assert_eq!(p.zeitpunkt(Slot::Reiseflug80, None), Some((450 - 90) * 60));
        p.tick(&tick(0, Cruise, 37_000.0, 0.0));
        let mut r80 = None;
        let mut t = 60;
        while t < 450 * 60 {
            let mut tk = tick(t, Cruise, 37_000.0, 0.0);
            tk.rest_min = Some((480 * 60 - t) as f64 / 60.0);
            if p.tick(&tk).iter().any(|x| x.slot == Slot::Reiseflug80) {
                r80 = Some(t);
                break;
            }
            t += 60;
        }
        assert_eq!(r80, Some((450 - 90) * 60), "zweiter Service bei TOD − 90 min");
        // Bordverkauf: 12 min vor dem Sinkflug wie bisher.
        let mut b = Planer::fortsetzen(f.clone(), Cruise);
        b.reiseflug80_art(Some(false));
        assert_eq!(b.zeitpunkt(Slot::Reiseflug80, None), Some(450 * 60 - REISEFLUG80_VOR_TOD_S));
        // Art unbekannt: wie bisher; ein späteres `None` löscht die Art nicht.
        let mut u = Planer::fortsetzen(f, Cruise);
        assert_eq!(u.zeitpunkt(Slot::Reiseflug80, None), Some(450 * 60 - REISEFLUG80_VOR_TOD_S));
        u.reiseflug80_art(Some(true));
        u.reiseflug80_art(None);
        assert_eq!(u.zeitpunkt(Slot::Reiseflug80, None), Some((450 - 90) * 60));
    }

    /// Kurzer Flug: Der zweite Service käme nach der Halbzeit zu spät — er
    /// entfällt mit Grund statt kurz vor dem Sinkflug.
    #[test]
    fn zweiter_service_entfaellt_auf_kurzem_flug() {
        let mut f = flug(100);
        f.abheben_epoch = Some(0);
        let mut p = Planer::fortsetzen(f, Cruise);
        p.reiseflug80_art(Some(true));
        // TOD 70 min: R50 bei 35 min, R80 frühestens 50 min — 50 min Rest.
        assert_eq!(p.zeitpunkt(Slot::Reiseflug80, None), Some(50 * 60));
        p.gemeldet.insert(Slot::Reiseflug25);
        p.gemeldet.insert(Slot::Reiseflug50);
        let mut q = Planer::fortsetzen(p.flug.clone(), Cruise);
        q.reiseflug80_art(Some(true));
        q.gemeldet = p.gemeldet.clone();
        let mut tk = tick(50 * 60, Cruise, 37_000.0, 0.0);
        tk.rest_min = Some(50.0);
        assert!(!p.tick(&tk).iter().any(|x| x.slot == Slot::Reiseflug80));
        assert!(p.entfallene_abholen().contains(&(Slot::Reiseflug80, Entfall::ZuWenigZeit)));
        // Ohne ETA vom Host (keine Navlog-Wegpunkte): Reserve aus der Planzeit.
        tk.rest_min = None;
        assert!(!q.tick(&tk).iter().any(|x| x.slot == Slot::Reiseflug80));
        assert!(q.entfallene_abholen().contains(&(Slot::Reiseflug80, Entfall::ZuWenigZeit)));
        // Neustart im Reiseflug ohne bekannte Abhebezeit: keine Schätzung
        // (sonst gälte der Neustart als Abheben und die ganze Planzeit als Rest).
        let mut ohne = flug(100);
        ohne.abheben_epoch = None;
        let mut w = Planer::fortsetzen(ohne, Cruise);
        w.tick(&tick(5_000, Cruise, 37_000.0, 0.0));
        assert_eq!(w.restflug_min(5_000 + 60, None), None);
        // Auch beim Wiedereinstieg im Steigflug (Codex-Nachprüfung).
        let mut ohne_steig = flug(100);
        ohne_steig.abheben_epoch = None;
        let mut st = Planer::fortsetzen(ohne_steig, Climb);
        st.tick(&tick(5_000, Climb, 8_000.0, 2_000.0));
        assert_eq!(st.restflug_min(5_000 + 60, None), None);
        // Beobachteter Start: Schätzung aus Planzeit.
        let mut z = Planer::neu(flug(100));
        z.tick(&tick(0, Takeoff, 500.0, 2_000.0));
        assert_eq!(z.restflug_min(40 * 60, None), Some(60.0));
    }

    /// 2.1.0 (Thomas/Peter): „Ende des Service/Bordverkaufs“ auf jedem Flug,
    /// 12 min vor dem geplanten Sinkflug, spätestens beim Sinkflugbeginn.
    #[test]
    fn ende_service_auf_jedem_flug_spaetestens_beim_sinkflug() {
        let mut f = flug(120);
        f.abheben_epoch = Some(0);
        let mut p = Planer::fortsetzen(f.clone(), Cruise);
        let bis = (120.0 - SINKFLUG_MIN) as i64 * 60;
        assert_eq!(p.zeitpunkt(Slot::EndeService, None), None, "2.1.1 C: hängt am Service");
        p.schon_gespielt([Slot::Reiseflug25]);
        p.service_ende(true);
        assert_eq!(p.zeitpunkt(Slot::EndeService, None), Some(bis - REISEFLUG80_VOR_TOD_S));
        assert!(!p.tick(&tick(bis - REISEFLUG80_VOR_TOD_S - 10, Cruise, 37_000.0, 0.0)).iter().any(|x| x.slot == Slot::EndeService));
        assert!(p.tick(&tick(bis - REISEFLUG80_VOR_TOD_S, Cruise, 37_000.0, 0.0)).iter().any(|x| x.slot == Slot::EndeService));
        // Früher Sinkflug (DLH454, Service gespielt): kommt beim Sinkflugbeginn.
        let mut q = Planer::fortsetzen(f.clone(), Cruise);
        q.tick(&tick(30 * 60, Cruise, 37_000.0, 0.0));
        q.service_ende(true);
        assert!(q.tick(&tick(31 * 60, Descent, 36_000.0, -2_000.0)).iter().any(|x| x.slot == Slot::EndeService));
        // Kurzstrecke unter einer Stunde mit Service: kommt auch.
        let mut k = flug(45);
        k.abheben_epoch = Some(0);
        let mut ks = Planer::fortsetzen(k, Climb);
        ks.service_ende(true);
        assert!(ks.tick(&tick(20 * 60, Descent, 15_000.0, -1_500.0)).iter().any(|x| x.slot == Slot::EndeService));
        // Altes Modul (Reiseflug 80 = Ende des Bordverkaufs): keine zweite Ende-Ansage.
        let mut alt = Planer::fortsetzen(f.clone(), Cruise);
        alt.reiseflug80_art(Some(false));
        assert!(!alt.tick(&tick(bis, Descent, 30_000.0, -2_000.0)).iter().any(|x| x.slot == Slot::EndeService));
        // Neustart im Sinkflug nach gespieltem Service: kommt noch — außer
        // sie lief laut Verlauf schon.
        let mut neu = Planer::fortsetzen(f.clone(), Descent);
        neu.service_ende(true);
        assert!(neu.tick(&tick(bis + 60, Descent, 25_000.0, -2_000.0)).iter().any(|x| x.slot == Slot::EndeService));
        let mut gelaufen = Planer::fortsetzen(f.clone(), Descent);
        gelaufen.schon_gespielt([Slot::EndeService]);
        assert!(!gelaufen.tick(&tick(bis + 60, Descent, 25_000.0, -2_000.0)).iter().any(|x| x.slot == Slot::EndeService));
        // EWG 221: Ein abgewählter Service gibt das Ende nicht frei (2.1.1 C
        // gab es frei wie ein gespielter) — beim Sinkflug „übersprungen“.
        let mut ab = Planer::fortsetzen(f.clone(), Cruise);
        ab.abwaehlen([Slot::Reiseflug25]);
        let mut gesehen = false;
        for s in (0..bis).step_by(10) {
            gesehen |= ab.tick(&tick(s, Cruise, 37_000.0, 0.0)).iter().any(|x| x.slot == Slot::EndeService);
        }
        gesehen |= ab.tick(&tick(bis, Descent, 30_000.0, -2_000.0)).iter().any(|x| x.slot == Slot::EndeService);
        assert!(!gesehen, "nach abgewähltem Service kein Ende");
        assert_eq!(ab.entfallene_abholen(), vec![(Slot::EndeService, Entfall::Uebersprungen)]);
        // Darf in der Ruhephase (Sinkflug unter 10.000 ft) spielen, sonst verfiele sie.
        assert!(Slot::EndeService.in_ruhephase_erlaubt());
        // Bizjet: nicht.
        let mut b = flug(120);
        b.flugart = Some(Flugart::Bizjet);
        b.abheben_epoch = Some(0);
        let mut bz = Planer::fortsetzen(b, Cruise);
        assert!(!bz.tick(&tick(bis, Descent, 30_000.0, -2_000.0)).iter().any(|x| x.slot == Slot::EndeService));
    }

    /// EWG 221 (22.10.2026): sehr kurzer Flug, Zeichen blieb an, kein
    /// Service. „Ende des Service“ kam beim Sinkflug trotzdem in die
    /// Schlange und verfiel `zu_spaet` (Verlust). Jetzt: nicht gemeldet,
    /// sondern entfallen als übersprungen (Regelgrund).
    #[test]
    fn ewg221_ohne_service_kein_ende_sondern_uebersprungen() {
        let mut p = Planer::neu(flug(25));
        let mut gemeldet = Vec::new();
        let mut s = 0;
        for (phase, h, vs, schritte) in [(Takeoff, 500.0, 2_000.0, 3), (Climb, 3_000.0, 2_000.0, 30), (Cruise, 7_000.0, 0.0, 12), (Descent, 6_000.0, -1_500.0, 3), (Approach, 2_500.0, -800.0, 3)] {
            for _ in 0..schritte {
                s += 10;
                let mut tk = tick(s, phase, h, vs);
                tk.anschnallzeichen = Some(1);
                gemeldet.extend(p.tick(&tk).into_iter().map(|f| f.slot));
            }
        }
        assert!(!gemeldet.contains(&Slot::Reiseflug25), "Vorbedingung: kein Service");
        assert!(!gemeldet.contains(&Slot::EndeService), "{gemeldet:?}");
        assert!(p.entfallene_abholen().contains(&(Slot::EndeService, Entfall::Uebersprungen)));
        assert!(!p.kommt_noch(Slot::EndeService, Approach));
    }

    /// Nur ein gespielter Service gibt das Ende frei: gemeldet, aber noch
    /// nicht zu Ende, hält es bis zum Ende des Sinkflugs zurück; verfallen,
    /// stumm, abgebrochen oder Fehler (`service_ende(false)`) nicht.
    #[test]
    fn ende_service_nur_nach_gespieltem_service() {
        let mut f = flug(120);
        f.abheben_epoch = Some(0);
        let bis = (120.0 - SINKFLUG_MIN) as i64 * 60;
        let ende = |p: &mut Planer, s: i64, ph: FlightPhase| p.tick(&tick(s, ph, 30_000.0, if ph == Cruise { 0.0 } else { -2_000.0 })).iter().any(|x| x.slot == Slot::EndeService);
        let uebersprungen = |p: &mut Planer| p.entfallene_abholen().contains(&(Slot::EndeService, Entfall::Uebersprungen));
        let mit_service = || {
            let mut p = Planer::fortsetzen(f.clone(), Cruise);
            let mut s = 0;
            while !p.erledigt(Slot::Reiseflug25) {
                s += 10;
                let mut tk = tick(s, Cruise, 37_000.0, 0.0);
                tk.anschnallzeichen = Some(0);
                p.tick(&tk);
            }
            p
        };
        // Gemeldet, Ende steht aus: hält beim Sinkflug zurück …
        let mut p = mit_service();
        assert!(!ende(&mut p, bis - REISEFLUG80_VOR_TOD_S, Cruise), "nicht zur geplanten Zeit");
        assert!(!ende(&mut p, bis, Descent), "nicht beim Sinkflug");
        assert!(!uebersprungen(&mut p), "noch nicht übersprungen");
        // … und kommt, wenn der Service noch im Sinkflug zu Ende spielt.
        p.service_ende(true);
        assert!(ende(&mut p, bis + 30, Descent));
        // Kein Ende bis zum Anflug: übersprungen.
        let mut q = mit_service();
        assert!(!ende(&mut q, bis, Descent));
        assert!(!ende(&mut q, bis + 300, Approach));
        assert!(uebersprungen(&mut q));
        // Verfallen/stumm/Fehler: beim Sinkflug übersprungen.
        let mut r = mit_service();
        r.service_ende(false);
        assert!(!ende(&mut r, bis - REISEFLUG80_VOR_TOD_S, Cruise));
        assert!(!ende(&mut r, bis, Descent));
        assert!(uebersprungen(&mut r));
        // Ein spätes „gespielt“ nach dem Aus ändert nichts mehr.
        r.service_ende(true);
        assert!(!ende(&mut r, bis + 30, Descent));
        // Gespielt: zur geplanten Zeit, wie bisher.
        let mut g = mit_service();
        g.service_ende(true);
        assert!(ende(&mut g, bis - REISEFLUG80_VOR_TOD_S, Cruise));
        // Neustart nach gespieltem Service (Host meldet es laut Verlauf).
        let mut n = Planer::fortsetzen(f.clone(), Descent);
        n.schon_gespielt([Slot::Reiseflug25]);
        n.service_ende(true);
        assert!(ende(&mut n, bis + 60, Descent));
        // Neustart ohne gespielten Service: übersprungen.
        let mut o = Planer::fortsetzen(f.clone(), Descent);
        o.schon_gespielt([Slot::Reiseflug25]);
        assert!(!ende(&mut o, bis + 60, Descent));
        assert!(uebersprungen(&mut o));
    }

    /// Planer und Warteschlange zusammen: Was spielt wann (alles spielbereit,
    /// jede Durchsage 20 s, Kommando 3 s) — wie der Host es abspielt.
    fn abspielen(p: &mut Planer, takte: impl IntoIterator<Item = Tick>) -> Vec<(i64, Slot)> {
        use crate::warteschlange::Warteschlange;
        let mut w = Warteschlange::default();
        let mut laeuft: Option<(Slot, i64)> = None;
        let mut gespielt = Vec::new();
        for t in takte {
            if let Some((s, bis)) = laeuft {
                if t.jetzt >= bis {
                    if s.loest_pause_aus() { w.ansage_zu_ende_nach(bis, s) } else { w.frei() }
                    if s == Slot::Reiseflug25 {
                        p.service_ende(true);
                    }
                    laeuft = None;
                }
            }
            let ruhe = p.ruhephase(&t);
            w.aufnehmen(p.tick(&t));
            if laeuft.is_none() {
                if let Some(s) = w.naechstes(t.jetzt, t.phase, ruhe) {
                    let dauer = if s.loest_pause_aus() { 20 } else { 3 };
                    if s.loest_pause_aus() { w.ansage_spielt_bis(t.jetzt + dauer) } else { w.spielt_bis(t.jetzt + dauer) }
                    laeuft = Some((s, t.jetzt + dauer));
                    gespielt.push((t.jetzt, s));
                }
            }
        }
        gespielt
    }

    /// 2.1.1 C (Entscheidung 4): 45-min-Flug ohne TOD aus dem OFP. Vorher
    /// lag „Ende des Service“ ~3 min nach dem Abheben, durfte in der Ruhephase
    /// spielen und kam vor dem Service.
    #[test]
    fn kurzer_flug_ohne_tod_ende_des_service_nach_dem_service() {
        let mut p = Planer::fortsetzen(flug(45), Boarding);
        let mut takte = Vec::new();
        let mut s = 0;
        for phase in [Boarding, Pushback, TaxiOut, TakeoffRoll] {
            for _ in 0..12 {
                s += 10;
                takte.push(tick(s, phase, 300.0, 0.0));
            }
        }
        // Steigflug auf FL240, Zeichen bei 6.000 ft aus (unter FL100: Ruhephase).
        let mut h = 300.0;
        while h < 24_000.0 {
            s += 10;
            h += 300.0;
            let mut t = tick(s, Climb, h, 1_800.0);
            if h > 6_000.0 {
                t.anschnallzeichen = Some(0);
            }
            takte.push(t);
        }
        for _ in 0..60 {
            s += 10;
            let mut t = tick(s, Cruise, 24_000.0, 0.0);
            t.anschnallzeichen = Some(0);
            takte.push(t);
        }
        let reihe: Vec<Slot> = abspielen(&mut p, takte).into_iter().map(|(_, s)| s).collect();
        let service = reihe.iter().position(|s| *s == Slot::Reiseflug25).expect("Service gespielt");
        let ende = reihe.iter().position(|s| *s == Slot::EndeService).expect("Ende gespielt");
        assert!(service < ende, "{reihe:?}");
    }

    /// 2.1.1 A, Entscheidung 1: Vorab geladen, Beacon an → Begrüßung,
    /// „Boarding completed“, „Türen zu“, Sicherheit in dieser Reihenfolge,
    /// je 10 s Pause zwischen den Durchsagen, nichts verfällt.
    #[test]
    fn beacon_reihenfolge_mit_zehn_sekunden_pause() {
        let mut p = Planer::fortsetzen(flug(120), Boarding);
        let mut takte: Vec<Tick> = (0..=600).step_by(1).map(|s| gate(s, Some(72_744.0), 2, s >= 300)).collect();
        for s in 601..=700 {
            let mut t = gate(s, Some(72_744.0), 2, true);
            t.phase = Pushback;
            takte.push(t);
        }
        let z = abspielen(&mut p, takte);
        assert_eq!(
            z,
            vec![
                (300, Slot::Boarding),
                (320, Slot::BoardingAbgeschlossen),
                (323, Slot::KommandoTuerenScharf),
                // 10 s nach dem Ende der Begrüßung (320), nicht 90 s.
                (330, Slot::BegruessungTuerenZu),
                (360, Slot::Sicherheit),
            ]
        );
    }

    /// 23.10.2026 (Restzeit der Sinkflug-Ansage): Die Prognose nimmt dieselben
    /// Quellen wie der Auslöser — das frühere von TOD − 90 s und Landung −
    /// 20 min —, gilt aber auch noch im Steigflug.
    #[test]
    fn kapitaen_prognose_wie_der_ausloeser() {
        // ETA-Regel früher als der TOD: Landung − 20 min.
        let mut p = Planer::fortsetzen(flug(120), Cruise);
        let mut tk = tick(1_000, Cruise, 37_000.0, 0.0);
        tk.tod_erwartet = Some(5_000);
        p.tick(&tk);
        let landung = 5_600;
        let um = p.kapitaen_sinkflug_prognose(Some(landung)).expect("Prognose");
        assert_eq!(um, landung - (KAPITAEN_VOR_LANDUNG_MIN * 60.0) as i64);
        assert_eq!(Some(um), p.zeitpunkt(Slot::KapitaenSinkflug, Some(landung)), "Anzeige wie Prognose");
        // Der Auslöser kommt genau dort (Restzeit 20 min), nicht früher.
        let mut tk = tick(um - 10, Cruise, 37_000.0, 0.0);
        tk.tod_erwartet = Some(5_000);
        tk.rest_min = Some((landung - (um - 10)) as f64 / 60.0);
        assert!(!slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug));
        tk.jetzt = um;
        tk.rest_min = Some((landung - um) as f64 / 60.0);
        assert!(slots(p.tick(&tk)).contains(&Slot::KapitaenSinkflug));
        assert_eq!(p.kapitaen_sinkflug_prognose(Some(landung)), None, "erledigt: keine Prognose");

        // TOD früher als die ETA-Regel: TOD − 90 s.
        let mut q = Planer::fortsetzen(flug(120), Cruise);
        let mut tk = tick(1_000, Cruise, 37_000.0, 0.0);
        tk.tod_erwartet = Some(4_000);
        q.tick(&tk);
        assert_eq!(q.kapitaen_sinkflug_prognose(Some(6_000)), Some(4_000 - KAPITAEN_VOR_TOD_S));

        // Kurzstrecke direkt nach dem Abheben: Der Steigflug ist nicht vorbei,
        // die Anzeige hat keine Uhrzeit — die Prognose aber schon.
        let mut k = Planer::neu(flug(45));
        k.tick(&tick(100, Takeoff, 500.0, 2_000.0));
        let mut tk = tick(110, Climb, 1_500.0, 2_000.0);
        tk.tod_erwartet = Some(1_400);
        k.tick(&tk);
        assert!(!k.steigflug_vorbei);
        assert_eq!(k.zeitpunkt(Slot::KapitaenSinkflug, Some(2_800)), None);
        assert_eq!(k.kapitaen_sinkflug_prognose(Some(2_800)), Some(1_400 - KAPITAEN_VOR_TOD_S));
        assert_eq!(k.kapitaen_sinkflug_prognose(Some(2_400)), Some(2_400 - 20 * 60));

        // Umleitung: TOD und ETA der alten Route fließen nicht ein.
        let mut u = Planer::fortsetzen(flug(120), Cruise);
        let mut tk = tick(1_000, Cruise, 37_000.0, 0.0);
        tk.tod_erwartet = Some(3_000);
        u.tick(&tk);
        let mut tk = tick(1_010, Cruise, 37_000.0, 0.0);
        tk.ziel_icao = Some("LFBD".into());
        tk.tod_erwartet = Some(3_000);
        u.tick(&tk);
        assert_eq!(u.kapitaen_sinkflug_prognose(Some(4_000)), None);

        // Keine Quelle: keine Prognose.
        let n = Planer::fortsetzen(flug(120), Cruise);
        assert_eq!(n.kapitaen_sinkflug_prognose(None), None);
    }
}
