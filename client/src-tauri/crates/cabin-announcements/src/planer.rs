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
        S::Boarding => P::Pushback,
        S::BegruessungTuerenZu | S::Sicherheit | S::KapitaenBegruessung => P::TakeoffRoll,
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
        S::Boarding | S::VerspaetungGate => P::Boarding,
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
}

/// Grenze für „Kurzstrecke“: darunter keine Reiseflug-Ansagen (8, 8a–c).
pub const KURZSTRECKE_MIN: u32 = 60;
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
    /// Zuletzt angesagtes Umleitungsziel: Ein neues Ziel (der Pilot ändert
    /// die Umleitung) wird noch einmal angesagt, dasselbe nicht.
    angesagte_umleitung: Option<String>,
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
        aus.push(Faellig { slot, verfall });
    }

    /// TOD als Uhrzeit — erst bekannt, wenn das Flugzeug abgehoben hat.
    fn tod_epoch(&self) -> Option<i64> {
        let ab = self.abheben_epoch?;
        self.flug.tod_nach_abheben_s.filter(|&s| s > 0).map(|s| ab + s)
    }

    fn schon(&self, slot: Slot) -> bool {
        self.gemeldet.contains(&slot)
    }

    /// Für die Anzeige „Als Nächstes“: kommt dieser Slot auf diesem Flug
    /// noch (erlaubt und noch nicht ausgelöst oder übersprungen)?
    /// `phase` ist die aktuelle Flugphase: Was nur bis zu einer Phase
    /// ausgelöst wird, kommt danach nicht mehr — auch dann nicht, wenn die
    /// Phasenerkennung die auslösende Phase übersprungen hat (Codex-QS).
    pub fn kommt_noch(&self, slot: Slot, phase: FlightPhase) -> bool {
        let reiseflug = matches!(slot, Slot::Reiseflug25 | Slot::Reiseflug50 | Slot::Reiseflug80);
        self.flugart().erlaubt(slot)
            && !self.schon(slot)
            && !(reiseflug && self.kurzstrecke())
            && spaetestens_vor(slot).is_none_or(|grenze| rang(phase) < rang(grenze))
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
        if t.phase == Boarding {
            self.melde(&mut aus, Slot::Boarding, Verfall::AbPhase(Pushback));
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
        // Bewusst KEINE Cockpit-Schalter (Beacon, Lichter): die sind je
        // Flugzeug anders belegt oder fehlen (Thomas 17.10.; UA scheitert
        // genau daran). Nur AeroACARS-Phasen und der Pushback-Zustand.
        let tueren_zu = t.pushback_aktiv || r >= rang(Pushback);
        if tueren_zu && r < rang(TakeoffRoll) {
            self.melde(&mut aus, Slot::KommandoTuerenScharf, Verfall::AbPhase(TakeoffRoll));
            self.melde(&mut aus, Slot::BegruessungTuerenZu, Verfall::AbPhase(TakeoffRoll));
            self.melde(&mut aus, Slot::Sicherheit, Verfall::AbPhase(TakeoffRoll));
        }
        if r >= rang(TaxiOut) && r < rang(TakeoffRoll) {
            self.melde(&mut aus, Slot::KapitaenBegruessung, Verfall::AbPhase(TakeoffRoll));
        }
        // „Plätze für den Start“: mit Beginn des Startlaufs (AeroACARS-Phase).
        if (rang(TakeoffRoll)..=rang(Climb)).contains(&r) {
            self.melde(&mut aus, Slot::KommandoStart, Verfall::Um(t.jetzt + 60));
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
                self.melde(&mut aus, Slot::AnschnallzeichenAus, Verfall::AbPhase(Descent));
            }
        }
        if t.phase == Cruise {
            self.war_im_reiseflug = true;
            if !self.kurzstrecke() {
                if let (Some(ab), Some(plan)) = (self.abheben_epoch, self.flug.geplante_flugzeit_min) {
                    // Bezugsgröße ist die Zeit bis zum Sinkflug, nicht die ganze
                    // Flugzeit — sonst läge 80 % bei 2 h schon im Sinkflug.
                    // TOD aus dem OFP, sonst Planzeit minus 30 min Sinkflug.
                    let bis_sinkflug_s = match self.flug.tod_nach_abheben_s {
                        Some(tod) if tod > 0 => tod as f64,
                        _ => (f64::from(plan) - SINKFLUG_MIN).max(1.0) * 60.0,
                    };
                    let anteil = (t.jetzt - ab) as f64 / bis_sinkflug_s;
                    let stufen = [(0.80, Slot::Reiseflug80), (0.50, Slot::Reiseflug50), (0.25, Slot::Reiseflug25)];
                    // Sind mehrere Schwellen zugleich überschritten (z. B. nach
                    // einem Wiedereinstieg), kommt nur die höchste; die
                    // niedrigeren entfallen (Verhalten wie UA).
                    if let Some(i) = stufen.iter().position(|(s, _)| anteil >= *s) {
                        let slot = stufen[i].1;
                        if !self.schon(slot) {
                            self.melde_reiseflug(&mut aus, t, slot);
                            if self.schon(slot) {
                                for (_, tiefer) in &stufen[i + 1..] {
                                    self.gemeldet.insert(*tiefer);
                                }
                            }
                        }
                    }
                }
            }
        }

        // ---- Turbulenz (nur im Reiseflug, nach „Anschnallzeichen aus“) ----
        if t.phase == Cruise && self.schon(Slot::AnschnallzeichenAus) {
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
                self.melde(&mut aus, Slot::KabineSinkflug, Verfall::AbPhase(Final));
            }
        }
        if in_luft
            && (t.phase == Approach
                || (r >= rang(Descent) && t.hoehe_ft < 20_000.0 && t.vs_fpm < -300.0))
        {
            self.melde(&mut aus, Slot::KommandoLandungVorbereiten, Verfall::AbPhase(Landing));
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
        if t.phase == BlocksOn || t.phase == Arrived {
            self.melde(&mut aus, Slot::KommandoTuerenEntschaerfen, Verfall::Um(t.jetzt + 120));
        }
        if t.phase == Arrived {
            self.melde(&mut aus, Slot::Aussteigen, Verfall::AbPhase(PirepSubmitted));
        }

        self.letzte_phase = Some(t.phase);
        self.letzte_hoehe_ft = Some(t.hoehe_ft);
        self.letztes_zeichen = t.anschnallzeichen;
        aus
    }

    fn melde_reiseflug(&mut self, aus: &mut Vec<Faellig>, t: &Tick, slot: Slot) {
        if self.schon(slot) {
            return;
        }
        // Zu nah am Sinkflug: entfällt ganz.
        if t.rest_min.is_some_and(|m| m <= 35.0) {
            self.gemeldet.insert(slot);
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

    /// Spielt einen ganzen Flug in 10-s-Schritten durch und sammelt alles.
    fn normalflug(min: u32) -> Vec<Slot> {
        normalflug_mit(Planer::neu(flug(min)), min)
    }

    fn normalflug_mit(mut p: Planer, min: u32) -> Vec<Slot> {
        let mut alle = Vec::new();
        let mut t = 0i64;
        let lauf = |p: &mut Planer, tk: Tick, alle: &mut Vec<Slot>| {
            alle.extend(p.tick(&tk).into_iter().map(|f| f.slot));
        };
        for phase in [Preflight, Boarding, Pushback, TaxiOut, TakeoffRoll] {
            for _ in 0..30 {
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

    // 75 min: 80 % fiele schon in den Sinkflug → entfällt.
    const REISE_75: [Slot; 2] = [Slot::Reiseflug25, Slot::Reiseflug50];
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
        assert!(!kurz.kommt_noch(Slot::Reiseflug50, Climb), "Kurzstrecke ohne Reiseflug-Ansagen");
        // Phase uebersprungen (nie Cruise gemeldet): im Sinkflug kommt der
        // Reiseflug nicht mehr, die Sinkflug-Ansage schon; im Final keine.
        let p = Planer::neu(flug(120));
        assert!(!p.kommt_noch(Slot::Reiseflug50, Descent));
        assert!(p.kommt_noch(Slot::KapitaenSinkflug, Descent));
        assert!(!p.kommt_noch(Slot::KapitaenSinkflug, Final));
        assert!(!p.kommt_noch(Slot::Boarding, TaxiOut), "Boarding nur bis zum Pushback");
    }

    #[test]
    fn kurzstrecke_ohne_reiseflug_ansagen() {
        let alle = normalflug(55);
        for s in [Slot::Reiseflug25, Slot::Reiseflug50, Slot::Reiseflug80] {
            assert!(!alle.contains(&s), "{s:?} darf auf Kurzstrecke nicht kommen");
        }
        assert!(alle.contains(&Slot::KapitaenSinkflug));
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
        assert_eq!(reise(55), vec![]);
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
            vec![Slot::KapitaenSinkflug, Slot::KabineSinkflug, Slot::KommandoLandungVorbereiten, Slot::Chime10000Sinkflug, Slot::KommandoPlaetzeLandung, Slot::NachDerLandung]
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
        assert_eq!(alle, vec![Slot::KapitaenBegruessung]);
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

}
