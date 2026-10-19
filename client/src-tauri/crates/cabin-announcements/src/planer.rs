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
    /// Leergewicht mit Zuladung (ZFW) im Sim, kg; `None` = unbekannt.
    pub zfw_kg: Option<f64>,
    /// ZFW laut SimBrief-OFP, kg; `None` = kein OFP.
    pub zfw_plan_kg: Option<f64>,
    /// Ladeblatt-Ampel wie im Cockpit-Tab für Kraftstoff und ZFW grün (je
    /// unter 5 % Abweichung, nicht über Max; die TOW-Zeile zählt am Gate
    /// nicht, sie enthält das Rollkraftstoff); `None` = kein OFP.
    pub ladeblatt_gruen: Option<bool>,
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
        S::Boarding | S::BoardingAbgeschlossen => P::Pushback,
        S::BegruessungTuerenZu | S::Sicherheit => P::TakeoffRoll,
        // Kommt sie am Boden nicht mehr dran, nach dem Start (Thomas 18.10.2026).
        S::KapitaenBegruessung => P::Descent,
        S::AnschnallzeichenAus | S::Reiseflug25 | S::Reiseflug50 | S::Reiseflug80 => P::Descent,
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
        S::KapitaenSinkflug
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
    /// Die Türen waren schon zu, bevor das Boarding dran war.
    TuerenSchonZu,
    /// „Boarding completed“: ohne SimBrief-Ladeblatt nicht feststellbar.
    OhneLadeblatt,
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
/// … und spätestens so lange danach, auch wenn sie noch hoch ist.
pub const KABINE_NACH_KAPITAEN_SPAETESTENS_S: i64 = 300;
/// Boarding-Ansage spätestens so lange nach Beginn des Boardings, wenn das
/// Anschnallzeichen nicht vorher eingeschaltet wird (Thomas 18.10.2026).
pub const BOARDING_NACH_S: i64 = 5 * 60;
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
/// Boarding-Ansage, sobald die Zuladung so weit über dem Wert beim Flugstart
/// liegt (Einsteigen läuft) … (v2.0.28, Peter: „Begrüßung zu früh“)
pub const BOARDING_ZULADUNG_KG: f64 = 200.0;
/// … und das so lange (Sekunden).
pub const BOARDING_ZULADUNG_S: i64 = 30;
/// Liegt die ZFW beim Flugstart so nah am OFP (Prozent), ist schon geladen:
/// Boarding wie bisher (Anschnallzeichen / 5 min). Gleiche Grenze wie die
/// grüne Ampel im Cockpit-Tab.
pub const LADEBLATT_GRUEN_PROZENT: f64 = 5.0;
/// „Boarding completed“: ZFW so lange stabil (Sekunden) …
pub const BOARDING_FERTIG_STABIL_S: i64 = 30;
/// … innerhalb dieser Schwankung (kg) …
pub const BOARDING_FERTIG_STABIL_KG: f64 = 50.0;
/// … und frühestens so lange nach der Boarding-Ansage: War schon beim
/// Flugstart alles geladen, folgte sie sonst direkt auf „Willkommen an Bord“.
pub const BOARDING_FERTIG_NACH_ANSAGE_S: i64 = 120;
/// Erste Service-Ansage außerhalb der Kurzstrecke so lange nach Erreichen der
/// Reiseflughöhe (v2.0.28, Peter: „Service auf Langstrecke zu spät“).
pub const SERVICE_NACH_REISEFLUG_S: i64 = 10 * 60;

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
    war_im_reiseflug: bool,
    /// Die Kabine folgt dem Kapitän erst später ([`KABINE_NACH_KAPITAEN_MIN_S`]).
    kapitaen_sinkflug: KapitaenSinkflug,
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
    boarding_seit: Option<i64>,
    tueren_zu: bool,
    letztes_beacon: Option<bool>,
    letztes_fahrwerk: Option<bool>,
    /// Landelicht war seit dem Verlassen des Stands einmal aus (v2.0.28).
    landelicht_war_aus: bool,
    /// Erster Reiseflug-Takt (UTC-Epoch) für den frühen Service.
    reiseflug_seit: Option<i64>,
    /// ZFW beim ersten Boarding-Takt; `None` nach Wiedereinstieg.
    zfw_start_kg: Option<f64>,
    /// Beim Flugstart war die Zuladung noch nicht an Bord: Boarding folgt
    /// dem Einsteigen statt Anschnallzeichen / 5 min.
    boarding_erwartet: bool,
    /// Seit wann die Zuladung über Start + [`BOARDING_ZULADUNG_KG`] liegt.
    zuladung_steigt_seit: Option<i64>,
    /// ZFW-Bezug und seit wann sie innerhalb [`BOARDING_FERTIG_STABIL_KG`] ist.
    zfw_stabil: Option<(f64, i64)>,
    /// Wann die Boarding-Ansage gemeldet wurde (oder entfiel).
    boarding_um: Option<i64>,
    /// Art von „Reiseflug 80“ laut Modul: `Some(true)` = zweiter Service,
    /// `Some(false)` = Ende des Bordverkaufs, `None` = noch unbekannt.
    zweiter_service: Option<bool>,
    /// Wiedereinstieg nach Neustart: Der ZFW-Startwert ist unbekannt,
    /// Boarding folgt der bisherigen Regel.
    wiedereinstieg: bool,
}

impl Planer {
    pub fn neu(flug: Flugdaten) -> Self {
        let abheben_epoch = flug.abheben_epoch;
        Planer { flug, abheben_epoch, ..Default::default() }
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
        // ZFW-Wert als Startwert: nach einem Neustart mitten im Boarding
        // wartet die Ansage dann auf weiteres Einsteigen.
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
            p.war_im_reiseflug = true;
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
            let ab = self.abheben_epoch?;
            Some((plan - (jetzt - ab) as f64 / 60.0).max(0.0))
        })
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
    fn reiseflug_schwellen(&self) -> Option<[(i64, Slot); 3]> {
        let plan = self.flug.geplante_flugzeit_min?;
        let bis_sinkflug_s = match self.flug.tod_nach_abheben_s {
            Some(tod) if tod > 0 => tod as f64,
            _ => (f64::from(plan) - SINKFLUG_MIN).max(1.0) * 60.0,
        };
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
            return;
        }
        aus.push(Faellig { slot, verfall, trotz_ruhephase: false });
    }

    /// Ansagen, die der Pilot abgewählt hat (GSX, Cockpit 2 Ground, eigene
    /// Ansagen des Flugzeugs …). Sie kommen nicht und werden nicht angefragt.
    pub fn abwaehlen(&mut self, slots: impl IntoIterator<Item = Slot>) {
        self.abgewaehlt = slots.into_iter().collect();
    }

    /// TOD als Uhrzeit — erst bekannt, wenn das Flugzeug abgehoben hat.
    fn tod_epoch(&self) -> Option<i64> {
        let ab = self.abheben_epoch?;
        self.flug.tod_nach_abheben_s.filter(|&s| s > 0).map(|s| ab + s)
    }

    fn schon(&self, slot: Slot) -> bool {
        self.gemeldet.contains(&slot)
    }

    /// Ist dieser Slot auf diesem Flug schon ausgelöst (oder übersprungen)?
    pub fn erledigt(&self, slot: Slot) -> bool {
        self.schon(slot)
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
            // Kabine: nach dem Ende der Kapitänsansage (frühestens 2 min);
            // vorher keine eigene Uhrzeit — die Seite sagt „nach dem Kapitän“
            // (Thomas AIB437: beide zeigten dieselbe Zeit, die Kabine kam später).
            Slot::KabineSinkflug => match self.kapitaen_sinkflug {
                KapitaenSinkflug::Gespielt(k) => Some(k + KABINE_NACH_KAPITAEN_MIN_S),
                _ => None,
            },
            Slot::KapitaenSinkflug => {
                let aus_tod = self.tod_epoch().map(|t| t - 300);
                let aus_landung = landung_erwartet.map(|l| l - 25 * 60);
                match (aus_tod, aus_landung) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    (a, b) => a.or(b),
                }
            }
            _ => None,
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
        let in_luft = !t.am_boden;

        // ---- Boden vor dem Start ----
        // Boarding nicht gleich beim Laden (Thomas 18.10.2026, „oft
        // bemängelt“): beim Einschalten des Anschnallzeichens, sonst nach
        // fünf Minuten Boarding.
        if t.phase == Boarding {
            let seit = *self.boarding_seit.get_or_insert(t.jetzt);
            // v2.0.28 (Peter: „Begrüßung zu früh“): Ist die Zuladung beim
            // Flugstart noch nicht an Bord, folgt die Ansage dem Einsteigen —
            // Peter schaltet das Anschnallzeichen schon vor dem Boarding ein.
            if self.zfw_start_kg.is_none() && !self.wiedereinstieg {
                if let Some(z) = t.zfw_kg {
                    self.zfw_start_kg = Some(z);
                    self.boarding_erwartet = t
                        .zfw_plan_kg
                        .filter(|p| *p > 0.0)
                        .is_some_and(|p| (z - p).abs() / p * 100.0 >= LADEBLATT_GRUEN_PROZENT);
                }
            }
            if self.boarding_erwartet {
                // Fehlen beim (Neu-)Start noch mindestens 5 % zum OFP, steigt die
                // Zuladung noch weit über 200 kg; sonst gilt die alte Regel.
                let steigt = t.zfw_kg.zip(self.zfw_start_kg).is_some_and(|(z, s)| z >= s + BOARDING_ZULADUNG_KG);
                if steigt {
                    let ab = *self.zuladung_steigt_seit.get_or_insert(t.jetzt);
                    if t.jetzt - ab >= BOARDING_ZULADUNG_S {
                        self.melde(&mut aus, Slot::Boarding, Verfall::AbPhase(Pushback));
                    }
                }
            } else {
                let zeichen_an = t.anschnallzeichen == Some(2) && self.letztes_zeichen.is_some_and(|z| z != 2);
                if zeichen_an || t.jetzt - seit >= BOARDING_NACH_S {
                    self.melde(&mut aus, Slot::Boarding, Verfall::AbPhase(Pushback));
                }
            }
        }
        // „Boarding completed“ (v2.0.28, Peter): Ladeblatt im Cockpit-Tab
        // grün und die Zuladung steht — erst nach der Boarding-Ansage, vor
        // „Türen zu“. Ohne OFP nicht feststellbar.
        if let Some(z) = t.zfw_kg {
            match self.zfw_stabil {
                Some((bezug, _)) if (z - bezug).abs() < BOARDING_FERTIG_STABIL_KG => {}
                _ => self.zfw_stabil = Some((z, t.jetzt)),
            }
        }
        if self.schon(Slot::Boarding) && self.boarding_um.is_none() {
            self.boarding_um = Some(t.jetzt);
        }
        if t.am_boden && r < rang(Pushback) && !self.tueren_zu && self.schon(Slot::Boarding) {
            let stabil = self.zfw_stabil.is_some_and(|(_, seit)| t.jetzt - seit >= BOARDING_FERTIG_STABIL_S);
            let abstand = self.boarding_um.is_some_and(|b| t.jetzt - b >= BOARDING_FERTIG_NACH_ANSAGE_S);
            if t.ladeblatt_gruen == Some(true) && stabil && abstand {
                self.melde(&mut aus, Slot::BoardingAbgeschlossen, Verfall::AbPhase(Pushback));
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
            // Kam das Boarding bis dahin nicht dran, ist es zu spät.
            if self.abgewaehlt.contains(&Slot::Boarding) {
                self.gemeldet.insert(Slot::Boarding);
            } else {
                self.entfaellt(Slot::Boarding, Entfall::TuerenSchonZu);
            }
            if self.abgewaehlt.contains(&Slot::BoardingAbgeschlossen) {
                self.gemeldet.insert(Slot::BoardingAbgeschlossen);
            } else if t.ladeblatt_gruen.is_none() {
                self.entfaellt(Slot::BoardingAbgeschlossen, Entfall::OhneLadeblatt);
            } else {
                self.entfaellt(Slot::BoardingAbgeschlossen, Entfall::TuerenSchonZu);
            }
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
        if t.landelicht == Some(false) && r >= rang(Pushback) {
            self.landelicht_war_aus = true;
        }
        let landelicht_an = t.landelicht == Some(true) && self.landelicht_war_aus;
        if (t.phase == TaxiOut && landelicht_an) || (rang(TakeoffRoll)..=rang(Climb)).contains(&r) {
            self.melde(&mut aus, Slot::KommandoStart, Verfall::AbPhase(Climb));
        }

        // ---- Steigflug / Reiseflug ----
        if in_luft && self.abheben_epoch.is_none() && r >= rang(Takeoff) {
            self.abheben_epoch = Some(t.jetzt);
        }
        if let Some(h0) = self.letzte_hoehe_ft {
            if in_luft && h0 < 10_000.0 && t.hoehe_ft >= 10_000.0 && t.vs_fpm > 0.0 {
                self.melde(&mut aus, Slot::Chime10000Steigflug, Verfall::Um(t.jetzt + 30));
            }
            if in_luft
                && self.war_im_reiseflug
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
            self.war_im_reiseflug = true;
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
            if t.rest_min.is_some_and(|m| m <= KURZSTRECKE_SERVICE_REST_MIN) {
                self.entfaellt(Slot::Reiseflug25, Entfall::ZuWenigZeit);
            } else {
                self.melde(&mut aus, Slot::Reiseflug25, Verfall::AbPhase(Descent));
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
        if in_luft && self.war_im_reiseflug && r < rang(Final) {
            let nach_rest = t.rest_min.is_some_and(|m| m <= 25.0);
            let nach_tod = self.tod_epoch().is_some_and(|tod| t.jetzt >= tod - 300);
            let nach_phase = r >= rang(Descent);
            if nach_rest || nach_tod || nach_phase {
                self.melde(&mut aus, Slot::KapitaenSinkflug, Verfall::AbPhase(Final));
                if self.schon(Slot::KapitaenSinkflug) && self.kapitaen_sinkflug == KapitaenSinkflug::Offen && !self.abgewaehlt.contains(&Slot::KapitaenSinkflug) {
                    self.kapitaen_sinkflug = KapitaenSinkflug::Gemeldet;
                }
            }
        }
        // Kabine: frühestens zwei Minuten nach dem ENDE der Kapitänsansage,
        // dann unter 15.000 ft im Sinkflug bzw. im Anflug, spätestens nach
        // fünf Minuten. Wartet der Kapitän noch (Ton, Pause), wartet die
        // Kabine mit — außer im Anflug. Kommt keine Kapitänsansage (stumm,
        // Fehler, Wiedereinstieg im Sinkflug), zählen Anflug bzw. Höhe allein.
        if in_luft && self.war_im_reiseflug && r < rang(Final) && !self.schon(Slot::KabineSinkflug) {
            let tief = t.phase == Approach || (r >= rang(Descent) && t.hoehe_ft < 15_000.0 && t.vs_fpm < 0.0);
            let reif = match self.kapitaen_sinkflug {
                // Im Anflug sofort, sonst ginge sie bei einem kurzen Sinkflug
                // verloren; die Pause der Warteschlange trennt sie trotzdem.
                // Mindestens 2 min nach dem Ende, auch im Anflug (Codex 2.0.27).
                KapitaenSinkflug::Gespielt(k) => {
                    let seit = t.jetzt - k;
                    seit >= KABINE_NACH_KAPITAEN_MIN_S && (t.phase == Approach || tief || seit >= KABINE_NACH_KAPITAEN_SPAETESTENS_S)
                }
                KapitaenSinkflug::Gemeldet => t.phase == Approach,
                KapitaenSinkflug::Offen | KapitaenSinkflug::KommtNicht => tief,
            };
            if reif {
                self.melde(&mut aus, Slot::KabineSinkflug, Verfall::AbPhase(Final));
            }
        }
        if in_luft
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
        // Sinkflug: frühere von TOD − 5 min und Landung − 25 min.
        let mut f = flug(120);
        f.tod_nach_abheben_s = Some(90 * 60);
        f.abheben_epoch = Some(0);
        let p = Planer::fortsetzen(f, Cruise);
        assert_eq!(p.zeitpunkt(Slot::KapitaenSinkflug, None), Some(85 * 60));
        assert_eq!(p.zeitpunkt(Slot::KapitaenSinkflug, Some(100 * 60)), Some(75 * 60));
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
        // Der Host meldet die Sinkflug-Ansage des Kapitäns gleich als gespielt.
        let lauf = |p: &mut Planer, tk: Tick, alle: &mut Vec<Slot>| {
            for f in p.tick(&tk) {
                if f.slot == Slot::KapitaenSinkflug {
                    p.kapitaen_sinkflug_ende(true, tk.jetzt);
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
        let erwartet = [
            Slot::Boarding,
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
        assert!(!p.kommt_noch(Slot::Boarding, TaxiOut), "Boarding nur bis zum Pushback");
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
        // Hoch über 15.000 ft und unter fünf Minuten: noch nicht (auch nicht nach 130 s).
        assert_eq!(bei(Slot::KabineSinkflug), Some(200), "erst unter 15.000 ft, nicht sofort");
        // Gegenprobe: spätestens fünf Minuten nach dem Kapitän, auch wenn noch hoch.
        let mut q = Planer::fortsetzen(flug(120), Cruise);
        q.tick(&tick(1_000, Descent, 35_000.0, -1_000.0));
        q.kapitaen_sinkflug_ende(true, 1_000);
        assert!(q.tick(&tick(1_200, Descent, 32_000.0, -1_000.0)).iter().all(|f| f.slot != Slot::KabineSinkflug));
        assert!(q.tick(&tick(1_300, Descent, 31_000.0, -1_000.0)).iter().any(|f| f.slot == Slot::KabineSinkflug));
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
        assert!(!kabine(&mut p, 1_300, 13_000.0, Descent), "noch keine zwei Minuten nach dem Ende");
        assert!(kabine(&mut p, 1_370, 12_000.0, Descent));
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
        let mut alle = Vec::new();
        for (i, (ph, h)) in [(Descent, 12_000.0), (Approach, 4_000.0), (Final, 1_000.0), (TaxiIn, 0.0)].into_iter().enumerate() {
            alle.extend(p.tick(&tick(1_000 + i as i64 * 10, ph, h, -800.0)).into_iter().map(|f| f.slot));
        }
        assert_eq!(
            alle,
            // Kabine nicht im selben Takt wie der Kapitän, sondern im Anflug
            // (wie in echt: Kapitän, Kommando an die Crew, dann die Kabine).
            vec![Slot::KapitaenSinkflug, Slot::KommandoLandungVorbereiten, Slot::Chime10000Sinkflug, Slot::KabineSinkflug, Slot::KommandoPlaetzeLandung, Slot::NachDerLandung]
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
        p.war_im_reiseflug = true;
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

    /// Thomas 18.10.2026: Boarding nicht gleich beim Laden — beim Einschalten
    /// des Anschnallzeichens, sonst nach 5 Minuten; sind die Türen vorher zu,
    /// entfällt es sichtbar.
    #[test]
    fn boarding_beim_anschnallzeichen_oder_nach_fuenf_minuten() {
        let mut p = Planer::neu(flug(120));
        assert!(slots(p.tick(&boden(0, Boarding))).is_empty(), "nicht beim Laden");
        let mut an = boden(60, Boarding);
        an.anschnallzeichen = Some(2);
        assert_eq!(slots(p.tick(&an)), vec![Slot::Boarding], "Zeichen an");
        // Ohne Zeichen: nach 5 Minuten.
        let mut q = Planer::neu(flug(120));
        q.tick(&boden(0, Boarding));
        assert!(slots(q.tick(&boden(BOARDING_NACH_S - 10, Boarding))).is_empty());
        assert_eq!(slots(q.tick(&boden(BOARDING_NACH_S, Boarding))), vec![Slot::Boarding]);
        // War das Zeichen beim Laden schon an, zählt das nicht als Einschalten.
        let mut r = Planer::neu(flug(120));
        let mut schon = boden(0, Boarding);
        schon.anschnallzeichen = Some(2);
        r.tick(&schon);
        schon.jetzt = 60;
        assert!(slots(r.tick(&schon)).is_empty());
        // Türen vorher zu (Beacon nach 2 min): Boarding entfällt mit Grund.
        let mut b = Planer::neu(flug(120));
        b.tick(&boden(0, Boarding));
        let mut beacon = boden(120, Boarding);
        beacon.beacon = Some(true);
        let f = slots(b.tick(&beacon));
        assert!(!f.contains(&Slot::Boarding) && f.contains(&Slot::BegruessungTuerenZu), "{f:?}");
        assert_eq!(
            b.entfallene_abholen(),
            vec![(Slot::Boarding, Entfall::TuerenSchonZu), (Slot::BoardingAbgeschlossen, Entfall::OhneLadeblatt)]
        );
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
        p.war_im_reiseflug = true;
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
        // TOD laut OFP 80 min nach dem Abheben; abgehoben wird 2 h nach Plan.
        let mut f = flug(120);
        f.tod_nach_abheben_s = Some(80 * 60);
        let mut p = Planer::neu(f);
        let ab = 7_200;
        let sinkflug = |p: &mut Planer, jetzt: i64, phase| {
            p.tick(&tick(jetzt, phase, 36_000.0, 0.0)).iter().any(|f| f.slot == Slot::KapitaenSinkflug)
        };
        assert!(!sinkflug(&mut p, ab, Takeoff));
        assert!(!sinkflug(&mut p, ab + 20 * 60, Cruise), "kurz nach dem Abheben noch kein Sinkflug");
        assert!(!sinkflug(&mut p, ab + 70 * 60, Cruise));
        assert!(sinkflug(&mut p, ab + 76 * 60, Cruise), "5 min vor dem TOD");
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

    fn mit_ladung(jetzt: i64, zfw: f64, gruen: bool) -> Tick {
        let mut t = boden(jetzt, Boarding);
        t.zfw_kg = Some(zfw);
        t.zfw_plan_kg = Some(72_746.0);
        t.ladeblatt_gruen = Some(gruen);
        t
    }

    /// v2.0.28, Peter DLH16: Anschnallzeichen 1 min nach Flugstart an, die
    /// Zuladung stieg erst 12 min später. Die Boarding-Ansage folgt jetzt dem
    /// Einsteigen (+200 kg, 30 s), nicht dem Schalter.
    #[test]
    fn boarding_folgt_dem_einsteigen() {
        let mut p = Planer::neu(flug(40));
        p.tick(&mit_ladung(0, 49_203.0, false));
        let mut an = mit_ladung(60, 49_203.0, false);
        an.anschnallzeichen = Some(2);
        assert!(slots(p.tick(&an)).is_empty(), "Schalter allein reicht nicht mehr");
        assert!(slots(p.tick(&mit_ladung(BOARDING_NACH_S + 60, 49_203.0, false))).is_empty(), "auch nicht nach 5 min");
        assert!(slots(p.tick(&mit_ladung(700, 49_400.0, false))).is_empty(), "+197 kg: noch nicht");
        assert!(slots(p.tick(&mit_ladung(710, 50_675.0, false))).is_empty(), "steigt seit eben");
        assert_eq!(slots(p.tick(&mit_ladung(740, 51_555.0, false))), vec![Slot::Boarding]);
        // Gegenprobe: schon beim Flugstart geladen (KLM759) → alte Regel.
        let mut q = Planer::neu(flug(40));
        q.tick(&mit_ladung(0, 72_744.0, true));
        let mut an = mit_ladung(60, 72_744.0, true);
        an.anschnallzeichen = Some(2);
        assert_eq!(slots(q.tick(&an)), vec![Slot::Boarding]);
        assert!(slots(q.tick(&mit_ladung(60 + BOARDING_FERTIG_NACH_ANSAGE_S - 5, 72_744.0, true))).is_empty(), "nicht direkt nach „Willkommen“");
        assert_eq!(slots(q.tick(&mit_ladung(60 + BOARDING_FERTIG_NACH_ANSAGE_S, 72_744.0, true))), vec![Slot::BoardingAbgeschlossen]);
        // So legt der Host den Planer an (auch beim normalen Start): die
        // Zuladungs-Regel gilt genauso (Codex 2.0.28).
        let mut w = Planer::fortsetzen(flug(40), Boarding);
        w.tick(&mit_ladung(0, 49_203.0, false));
        assert!(slots(w.tick(&mit_ladung(BOARDING_NACH_S, 49_203.0, false))).is_empty(), "wartet aufs Einsteigen");
        w.tick(&mit_ladung(BOARDING_NACH_S + 10, 50_000.0, false));
        assert_eq!(slots(w.tick(&mit_ladung(BOARDING_NACH_S + 40, 50_500.0, false))), vec![Slot::Boarding]);
        // Neustart kurz vor Boarding-Ende (unter 5 % fehlen): kein Anstieg
        // mehr zu erwarten → alte Regel, spätestens nach 5 min (Codex 2.0.28).
        let mut e = Planer::fortsetzen(flug(40), Boarding);
        e.tick(&mit_ladung(0, 69_500.0, false));
        assert!(slots(e.tick(&mit_ladung(BOARDING_NACH_S - 10, 69_500.0, false))).is_empty());
        assert_eq!(slots(e.tick(&mit_ladung(BOARDING_NACH_S, 69_500.0, false))), vec![Slot::Boarding]);
        // Neustart im Pushback: kein Startwert mehr, nichts vom Boden.
        let mut n = Planer::fortsetzen(flug(40), Pushback);
        let mut pb = mit_ladung(0, 60_000.0, false);
        pb.phase = Pushback;
        assert!(!n.tick(&pb).iter().any(|f| f.slot == Slot::Boarding));
    }

    /// v2.0.28, Peter: „Boarding completed“, wenn das Ladeblatt grün ist und
    /// die Zuladung steht — erst nach der Boarding-Ansage, vor „Türen zu“.
    #[test]
    fn boarding_completed_bei_gruenem_stehendem_ladeblatt() {
        let mut p = Planer::neu(flug(40));
        p.tick(&mit_ladung(0, 49_203.0, false));
        p.tick(&mit_ladung(10, 52_000.0, false));
        assert_eq!(slots(p.tick(&mit_ladung(40, 60_000.0, false))), vec![Slot::Boarding]);
        // Grün, aber es wird noch geladen (ZFW ändert sich) → noch nicht.
        assert!(slots(p.tick(&mit_ladung(100, 70_000.0, true))).is_empty());
        assert!(slots(p.tick(&mit_ladung(115, 72_700.0, true))).is_empty());
        assert!(slots(p.tick(&mit_ladung(140, 72_740.0, true))).is_empty(), "erst 25 s stabil");
        assert!(slots(p.tick(&mit_ladung(146, 72_744.0, true))).is_empty(), "stabil, aber erst 106 s nach der Boarding-Ansage");
        assert_eq!(slots(p.tick(&mit_ladung(160, 72_744.0, true))), vec![Slot::BoardingAbgeschlossen]);
        assert!(slots(p.tick(&mit_ladung(300, 72_744.0, true))).is_empty(), "nur einmal");
        // Nicht vor der Boarding-Ansage, auch wenn alles grün steht.
        let mut q = Planer::neu(flug(40));
        q.tick(&mit_ladung(0, 72_744.0, true));
        assert!(slots(q.tick(&mit_ladung(60, 72_744.0, true))).is_empty(), "Boarding kam noch nicht");
        // Ohne OFP: entfällt beim Türen-zu mit Grund.
        let mut o = Planer::neu(flug(40));
        o.tick(&boden(0, Boarding));
        let mut zu = boden(400, Boarding);
        zu.beacon = Some(true);
        o.tick(&zu);
        assert!(o.entfallene_abholen().contains(&(Slot::BoardingAbgeschlossen, Entfall::OhneLadeblatt)));
        // Bizjet: gibt es nicht.
        let mut b = flug(40);
        b.flugart = Some(Flugart::Bizjet);
        let mut z = Planer::neu(b);
        z.tick(&mit_ladung(0, 72_744.0, true));
        assert!(!z.tick(&mit_ladung(400, 72_744.0, true)).iter().any(|f| f.slot == Slot::BoardingAbgeschlossen));
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
        let mut p = Planer::neu(flug(120));
        p.tick(&boden(0, Pushback));
        let mut licht = boden(10, Pushback);
        licht.landelicht = Some(true);
        assert!(!p.tick(&licht).iter().any(|f| f.slot == Slot::KommandoStart), "beim Zurückschieben nicht");
        licht.phase = TaxiOut;
        licht.jetzt = 20;
        assert!(p.tick(&licht).iter().any(|f| f.slot == Slot::KommandoStart), "brennt beim Wechsel ins Rollen");
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
    }

}
