//! Die Warteschlange (Leitfaden 2b): Ansagen überlappen nie, Kommandos und
//! Chimes gehen vor, in der Ruhephase laufen nur Kommandos und Chimes, und
//! was zu spät ist, verfällt.

use sim_core::FlightPhase;

use crate::planer::{rang, Faellig, Verfall};
use crate::slot::Slot;

#[derive(Debug, Clone)]
struct Eintrag {
    slot: Slot,
    verfall: Verfall,
    reihenfolge: u64,
    trotz_ruhephase: bool,
}

/// Ruhe nach einer gesprochenen Ansage, bevor die nächste reguläre kommt.
/// Kommandos, Gong, Turbulenz und Durchstarten warten nicht ([`Slot::wartet_pause`]).
pub const PAUSE_NACH_ANSAGE_S: i64 = 90;
/// Ausnahme: Die Sicherheitshinweise folgen der Begrüßung „Türen zu“ nach
/// höchstens so vielen Sekunden (Thomas 18.10.2026); alles andere wartet 90 s.
pub const PAUSE_BEGRUESSUNG_SICHERHEIT_S: i64 = 10;
/// Am Boden folgt die Kapitän-Begrüßung den Sicherheitshinweisen nach
/// höchstens so vielen Sekunden — sonst rutscht sie bei kurzem Rollweg hinter
/// den Start (v2.0.28, QAF446). Reicht es nicht, spricht er nach dem Start.
pub const PAUSE_SICHERHEIT_KAPITAEN_S: i64 = 30;

#[derive(Debug, Default)]
pub struct Warteschlange {
    eintraege: Vec<Eintrag>,
    zaehler: u64,
    /// Bis wann die laufende Ansage spielt (UTC-Epoch-Sekunden).
    belegt_bis: Option<i64>,
    /// Bis wann nach einer gesprochenen Ansage Ruhe ist ([`PAUSE_NACH_ANSAGE_S`]).
    pause_bis: Option<i64>,
    /// Ein Slot, der die Pause schon ab diesem Zeitpunkt nicht mehr abwartet.
    pause_ausnahme: Option<(Slot, i64)>,
    verfallen: Vec<Slot>,
    /// Ein Slot, der bis zu diesem Zeitpunkt zurückgehalten wird: Er spielt
    /// nicht und verfällt nicht (der Host wartet auf frischen Ton, siehe
    /// [`Self::zurueckhalten`]).
    zurueckgehalten: Option<(Slot, i64)>,
    /// Nach dem Zurückhalten verfällt der Slot noch so lange nicht (Codex
    /// 23.10.2026: sonst ging die alte Ansage verloren, wenn die Frist bis in
    /// ihre Verfallsphase lief). `None` im Zeitpunkt = beginnt beim nächsten
    /// [`Self::naechstes_bereit`].
    nachfrist: Option<(Slot, Option<i64>)>,
}

/// Nachfrist nach dem Zurückhalten (s): Spielt der Slot darin nicht (etwa
/// wegen der Ruhephase), verfällt er wie gewohnt.
pub const NACHFRIST_S: i64 = 15;

impl Warteschlange {
    pub fn aufnehmen(&mut self, faellig: impl IntoIterator<Item = Faellig>) {
        for f in faellig {
            self.zaehler += 1;
            self.eintraege.push(Eintrag { slot: f.slot, verfall: f.verfall, reihenfolge: self.zaehler, trotz_ruhephase: f.trotz_ruhephase });
        }
    }

    pub fn ist_leer(&self) -> bool {
        self.eintraege.is_empty()
    }

    pub fn wartend(&self) -> Vec<Slot> {
        self.eintraege.iter().map(|e| e.slot).collect()
    }

    /// Darf dieser wartende Slot im Steigflug trotz Ruhephase spielen?
    pub fn trotz_ruhephase(&self, slot: Slot) -> bool {
        self.eintraege.iter().any(|e| e.slot == slot && e.trotz_ruhephase)
    }

    /// Slots, die seit dem letzten Aufruf verfallen sind (für „Bisher“).
    pub fn verfallene_abholen(&mut self) -> Vec<Slot> {
        std::mem::take(&mut self.verfallen)
    }

    /// Nimmt einen wartenden Slot heraus (er entfällt); `true`, wenn er wartete.
    pub fn entfernen(&mut self, slot: Slot) -> bool {
        let vorher = self.eintraege.len();
        self.eintraege.retain(|e| e.slot != slot);
        self.eintraege.len() != vorher
    }

    /// Nimmt alles Wartende heraus (Stumm), in Reihenfolge des Eingangs.
    pub fn alle_entnehmen(&mut self) -> Vec<Slot> {
        std::mem::take(&mut self.eintraege).into_iter().map(|e| e.slot).collect()
    }

    /// Der Host meldet, dass eine Ansage bis `bis` läuft.
    pub fn spielt_bis(&mut self, bis: i64) {
        self.belegt_bis = Some(bis);
    }

    /// Läuft gerade etwas (Ansage oder Vorhören)?
    pub fn belegt(&self, jetzt: i64) -> bool {
        self.belegt_bis.is_some_and(|b| jetzt < b)
    }

    /// Die Wiedergabe ist früher zu Ende als geschätzt (Stopp, Vorhören vorbei).
    pub fn frei(&mut self) {
        self.belegt_bis = None;
    }

    /// Abbruch ohne Hörbares (Stumm): frei und keine Pause.
    pub fn abbrechen(&mut self) {
        self.belegt_bis = None;
        self.pause_bis = None;
        self.pause_ausnahme = None;
    }

    /// Eine echte Ansage spielt bis `bis`; danach folgt die Pause. Fürs
    /// Vorhören gilt weiter [`Self::spielt_bis`] (ohne Pause).
    pub fn ansage_spielt_bis(&mut self, bis: i64) {
        self.belegt_bis = Some(bis);
        self.pause_bis = Some(bis + PAUSE_NACH_ANSAGE_S);
        self.pause_ausnahme = None;
    }

    /// Eine echte Ansage ist zu Ende (gespielt, abgebrochen oder Fehler):
    /// frei, aber die Pause läuft ab jetzt.
    pub fn ansage_zu_ende(&mut self, jetzt: i64) {
        self.belegt_bis = None;
        self.pause_bis = Some(jetzt + PAUSE_NACH_ANSAGE_S);
        self.pause_ausnahme = None;
    }

    /// Wie [`Self::ansage_zu_ende`], mit dem Slot, der gerade lief: Nach der
    /// Begrüßung „Türen zu“ dürfen die Sicherheitshinweise schon nach
    /// [`PAUSE_BEGRUESSUNG_SICHERHEIT_S`] kommen, alles andere nach 90 s.
    pub fn ansage_zu_ende_nach(&mut self, jetzt: i64, slot: Slot) {
        self.ansage_zu_ende(jetzt);
        // 2.1.1 (Entscheidung 1): Begrüßung → „Türen zu“ → Sicherheit mit je
        // 10 s Pause, sonst hielte die 90-s-Pause sie beim Beacon bis zum
        // Startlauf auf.
        if slot == Slot::Boarding {
            self.pause_ausnahme = Some((Slot::BegruessungTuerenZu, jetzt + PAUSE_BEGRUESSUNG_SICHERHEIT_S));
        } else if slot == Slot::BegruessungTuerenZu {
            self.pause_ausnahme = Some((Slot::Sicherheit, jetzt + PAUSE_BEGRUESSUNG_SICHERHEIT_S));
        } else if slot == Slot::Sicherheit {
            self.pause_ausnahme = Some((Slot::KapitaenBegruessung, jetzt + PAUSE_SICHERHEIT_KAPITAEN_S));
        }
    }

    /// Hält einen wartenden Slot bis `bis` zurück (`None` = frei): Er spielt
    /// so lange nicht und verfällt auch nicht — danach gilt wieder sein
    /// normaler Verfall (23.10.2026: Die Sinkflug-Ansage des Kapitäns wartet
    /// höchstens eine Minute auf ihre aufgefrischte Restzeit).
    pub fn zurueckhalten(&mut self, slot: Slot, bis: Option<i64>) {
        if bis.is_none() && self.zurueckgehalten.is_some_and(|(s, _)| s == slot) {
            self.nachfrist = Some((slot, None));
        }
        self.zurueckgehalten = bis.map(|b| (slot, b));
    }

    fn haelt_zurueck(&self, slot: Slot, jetzt: i64) -> bool {
        self.zurueckgehalten.is_some_and(|(s, bis)| s == slot && jetzt < bis)
    }

    fn ausnahme_frei(&self, slot: Slot, jetzt: i64) -> bool {
        self.pause_ausnahme.is_some_and(|(s, ab)| s == slot && jetzt >= ab)
    }

    /// Läuft gerade die Pause nach einer Ansage (und spielt nichts)?
    pub fn in_pause(&self, jetzt: i64) -> bool {
        !self.belegt(jetzt) && self.pause_bis.is_some_and(|p| jetzt < p)
    }

    fn abgelaufen(e: &Eintrag, jetzt: i64, phase: FlightPhase) -> bool {
        match e.verfall {
            Verfall::AbPhase(p) => rang(phase) >= rang(p),
            Verfall::Um(t) => jetzt >= t,
        }
    }

    /// Nächste spielbare Ansage, oder `None`, wenn gerade etwas läuft, die
    /// Ruhephase alles Verfügbare sperrt oder nichts ansteht.
    pub fn naechstes(&mut self, jetzt: i64, phase: FlightPhase, ruhephase: bool) -> Option<Slot> {
        self.naechstes_bereit(jetzt, phase, ruhephase, |_| true)
    }

    /// Wie [`Self::naechstes`], aber nur Slots, für die `bereit` gilt (Ton
    /// fertig erzeugt). Die anderen bleiben liegen, bis sie bereit sind oder
    /// verfallen — sonst ginge eine Ansage verloren, deren Text noch
    /// unterwegs ist.
    pub fn naechstes_bereit(
        &mut self,
        jetzt: i64,
        phase: FlightPhase,
        ruhephase: bool,
        bereit: impl Fn(Slot) -> bool,
    ) -> Option<Slot> {
        // Frist von selbst abgelaufen: wie freigegeben, Nachfrist ab ihrem Ende.
        if let Some((s, bis)) = self.zurueckgehalten.filter(|(_, bis)| jetzt >= *bis) {
            self.zurueckgehalten = None;
            self.nachfrist = Some((s, Some(bis + NACHFRIST_S)));
        }
        if let Some((s, None)) = self.nachfrist {
            self.nachfrist = Some((s, Some(jetzt + NACHFRIST_S)));
        }
        let gehalten = self.zurueckgehalten.map(|(s, _)| s);
        let nachfrist = self.nachfrist.filter(|(_, bis)| bis.is_some_and(|b| jetzt < b)).map(|(s, _)| s);
        let (behalten, weg): (Vec<_>, Vec<_>) = std::mem::take(&mut self.eintraege)
            .into_iter()
            .partition(|e| gehalten == Some(e.slot) || nachfrist == Some(e.slot) || !Self::abgelaufen(e, jetzt, phase));
        self.eintraege = behalten;
        self.verfallen.extend(weg.into_iter().map(|e| e.slot));

        if self.belegt_bis.is_some_and(|b| jetzt < b) {
            return None;
        }
        let pause = self.pause_bis.is_some_and(|p| jetzt < p);
        // Am Boden feste Reihenfolge: Begrüßung und Sicherheit vor dem
        // Kapitän, sobald sie spielbereit sind — sonst überholt er sie mit
        // seinem Vorrang, und die Sicherheit liefe in den Startlauf (beta.6).
        let kabine_zuerst = self
            .eintraege
            .iter()
            .any(|e| matches!(e.slot, Slot::BegruessungTuerenZu | Slot::Sicherheit) && bereit(e.slot));
        let boarding_wartet = self.eintraege.iter().any(|e| e.slot == Slot::Boarding);
        // 2.1.1 A/E: Begrüßung → „Boarding completed“ → „Türen zu“ →
        // Sicherheit, auch gegenüber Kommandos, auch wenn der Ton der Begrüßung
        // noch nicht fertig ist. Kommt er sicher nie (keine Stimme, endgültig
        // abgelehnt), nimmt der Host sie heraus.
        let abgeschlossen_wartet = self.eintraege.iter().any(|e| e.slot == Slot::BoardingAbgeschlossen);
        let tueren_zu_gruppe = |s: Slot| matches!(s, Slot::KommandoTuerenScharf | Slot::BegruessungTuerenZu | Slot::Sicherheit);
        // 2.1.1 C: „Ende des Service“ nie vor einem gemeldeten, noch
        // wartenden Service; beim Sinkflug verfällt der zuerst (oben).
        let service_wartet = self.eintraege.iter().any(|e| e.slot == Slot::Reiseflug25);
        // „Ende des Service“ kommt vor der Sinkflug-Ansage des Kapitäns, wenn
        // beide zugleich fällig sind (Sinkflugbeginn) und es spielbereit ist.
        let ende_service_zuerst = self.eintraege.iter().any(|e| e.slot == Slot::EndeService && bereit(e.slot));
        // „Prepare for landing“ überholt die wartende Landevorbereitung der
        // Kabine nicht (Thomas 20.10.2026, Variante A). Nicht in der Ruhephase
        // (die Kabine darf dort nicht sprechen) und im Anflug nur, solange
        // ihr Ton fertig ist – sonst hinge das Kommando bis zum Endanflug.
        let kabine_vor_kommando = !ruhephase
            && self
                .eintraege
                .iter()
                .any(|e| e.slot == Slot::KabineSinkflug && (bereit(e.slot) || rang(phase) < rang(FlightPhase::Approach)));
        let idx = self
            .eintraege
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                !ruhephase || e.slot.in_ruhephase_erlaubt() || (e.trotz_ruhephase && phase == FlightPhase::Climb)
            })
            .filter(|(_, e)| !(pause && e.slot.wartet_pause() && !self.ausnahme_frei(e.slot, jetzt)))
            .filter(|(_, e)| !(kabine_zuerst && e.slot == Slot::KapitaenBegruessung))
            // „Boarding completed“ (Kommando-Vorrang) überholt die noch
            // wartende Boarding-Ansage nicht.
            .filter(|(_, e)| !(e.slot == Slot::BoardingAbgeschlossen && boarding_wartet))
            .filter(|(_, e)| !(tueren_zu_gruppe(e.slot) && (boarding_wartet || abgeschlossen_wartet)))
            .filter(|(_, e)| !(e.slot == Slot::EndeService && service_wartet))
            .filter(|(_, e)| !(e.slot == Slot::KapitaenSinkflug && ende_service_zuerst))
            .filter(|(_, e)| !(e.slot == Slot::KommandoLandungVorbereiten && kabine_vor_kommando))
            .filter(|(_, e)| !self.haelt_zurueck(e.slot, jetzt))
            .filter(|(_, e)| bereit(e.slot))
            .min_by_key(|(_, e)| (e.slot.vorrang(), e.reihenfolge))
            .map(|(i, _)| i)?;
        Some(self.eintraege.remove(idx).slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use FlightPhase::*;

    fn f(slot: Slot, verfall: Verfall) -> Faellig {
        Faellig { slot, verfall, trotz_ruhephase: false }
    }

    #[test]
    fn kommando_vor_kabine_und_kapitaen_vor_kabine() {
        let mut w = Warteschlange::default();
        w.aufnehmen([
            f(Slot::BegruessungTuerenZu, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::KommandoTuerenScharf, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::KabineSinkflug, Verfall::AbPhase(Final)),
            f(Slot::KapitaenSinkflug, Verfall::AbPhase(Final)),
        ]);
        let mut reihe = Vec::new();
        while let Some(s) = w.naechstes(0, Pushback, false) {
            reihe.push(s);
        }
        assert_eq!(
            reihe,
            vec![Slot::KommandoTuerenScharf, Slot::KapitaenSinkflug, Slot::BegruessungTuerenZu, Slot::KabineSinkflug]
        );
    }

    /// Zurückgehalten: spielt nicht und verfällt nicht, auch nicht in seiner
    /// Verfallsphase; danach gilt der normale Verfall. Andere laufen weiter.
    #[test]
    fn zurueckgehalten_spielt_nicht_und_verfaellt_nicht() {
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::KapitaenSinkflug, Verfall::AbPhase(Final)), f(Slot::EndeService, Verfall::AbPhase(Approach))]);
        w.zurueckhalten(Slot::KapitaenSinkflug, Some(160));
        assert_eq!(w.naechstes(100, Descent, false), Some(Slot::EndeService), "andere nicht blockiert");
        w.frei();
        assert_eq!(w.naechstes(150, Final, false), None);
        assert!(w.verfallene_abholen().is_empty(), "verfällt nicht in der Frist");
        assert_eq!(w.wartend(), vec![Slot::KapitaenSinkflug]);
        // Frist vorbei: Nachfrist — die Ansage spielt noch, statt zu verfallen (Codex).
        assert_eq!(w.naechstes(160, Final, false), Some(Slot::KapitaenSinkflug), "spielt nach der Frist noch");
        assert!(w.verfallene_abholen().is_empty());
        // Hält die Ruhephase sie auch in der Nachfrist auf, verfällt sie danach.
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::KapitaenSinkflug, Verfall::AbPhase(Final))]);
        w.zurueckhalten(Slot::KapitaenSinkflug, Some(160));
        assert_eq!(w.naechstes(150, Final, true), None);
        w.zurueckhalten(Slot::KapitaenSinkflug, None);
        assert_eq!(w.naechstes(155, Final, true), None, "Ruhephase");
        assert!(w.verfallene_abholen().is_empty(), "in der Nachfrist nicht verfallen");
        assert_eq!(w.naechstes(155 + NACHFRIST_S, Final, true), None);
        assert_eq!(w.verfallene_abholen(), vec![Slot::KapitaenSinkflug], "nach der Nachfrist normal");
        // Vorzeitig freigegeben: sofort spielbar.
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::KapitaenSinkflug, Verfall::AbPhase(Final))]);
        w.zurueckhalten(Slot::KapitaenSinkflug, Some(160));
        assert_eq!(w.naechstes(100, Descent, false), None);
        w.zurueckhalten(Slot::KapitaenSinkflug, None);
        assert_eq!(w.naechstes(101, Descent, false), Some(Slot::KapitaenSinkflug));
    }

    #[test]
    fn nichts_ueberlappt() {
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::Boarding, Verfall::Um(1000)), f(Slot::Sicherheit, Verfall::Um(1000))]);
        assert_eq!(w.naechstes(0, Boarding, false), Some(Slot::Boarding));
        w.spielt_bis(30);
        assert_eq!(w.naechstes(10, Boarding, false), None);
        assert_eq!(w.naechstes(30, Boarding, false), Some(Slot::Sicherheit));
    }

    #[test]
    fn ruhephase_haelt_ki_ansagen_zurueck_ohne_sie_zu_verwerfen() {
        let mut w = Warteschlange::default();
        w.aufnehmen([
            f(Slot::KapitaenBegruessung, Verfall::AbPhase(Cruise)),
            f(Slot::KommandoStart, Verfall::Um(100)),
        ]);
        assert_eq!(w.naechstes(0, TakeoffRoll, true), Some(Slot::KommandoStart));
        assert_eq!(w.naechstes(1, TakeoffRoll, true), None);
        assert_eq!(w.wartend(), vec![Slot::KapitaenBegruessung]);
        assert_eq!(w.naechstes(2, Climb, false), Some(Slot::KapitaenBegruessung));
    }

    #[test]
    fn zu_spaet_verfaellt() {
        let mut w = Warteschlange::default();
        w.aufnehmen([
            f(Slot::Sicherheit, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::KommandoTuerenEntschaerfen, Verfall::Um(50)),
        ]);
        assert_eq!(w.naechstes(60, TakeoffRoll, true), None);
        let mut weg = w.verfallene_abholen();
        weg.sort();
        assert_eq!(weg, vec![Slot::Sicherheit, Slot::KommandoTuerenEntschaerfen]);
        assert!(w.ist_leer());
    }

    #[test]
    fn nicht_bereites_bleibt_liegen_bis_es_bereit_ist() {
        let mut w = Warteschlange::default();
        w.aufnehmen([
            f(Slot::KapitaenBegruessung, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::Boarding, Verfall::AbPhase(Pushback)),
        ]);
        // Kapitän geht vor, ist aber noch nicht fertig: die Kabine spielt.
        assert_eq!(w.naechstes_bereit(0, Boarding, false, |s| s != Slot::KapitaenBegruessung), Some(Slot::Boarding));
        assert_eq!(w.naechstes_bereit(1, Boarding, false, |s| s != Slot::KapitaenBegruessung), None);
        assert_eq!(w.wartend(), vec![Slot::KapitaenBegruessung]);
        assert_eq!(w.naechstes_bereit(2, TaxiOut, false, |_| true), Some(Slot::KapitaenBegruessung));
    }


    #[test]
    fn pause_nach_ansage_nur_fuer_regulaere_ansagen() {
        let mut w = Warteschlange::default();
        w.ansage_spielt_bis(100);
        w.ansage_zu_ende(100);
        w.aufnehmen([
            f(Slot::KabineSinkflug, Verfall::AbPhase(Final)),
            // Nicht „prepare for landing“: Das wartet seit 20.10. auf die Kabine.
            f(Slot::KommandoPlaetzeLandung, Verfall::AbPhase(Landing)),
        ]);
        assert!(w.in_pause(150));
        // Kommando darf sofort, die Kabine wartet die Pause ab.
        assert_eq!(w.naechstes(150, Descent, false), Some(Slot::KommandoPlaetzeLandung));
        assert_eq!(w.naechstes(150, Descent, false), None, "Kabine wartet");
        assert_eq!(w.naechstes(100 + PAUSE_NACH_ANSAGE_S, Descent, false), Some(Slot::KabineSinkflug));
    }

    /// Thomas 20.10.2026, Variante A: „Prepare for landing“ überholt die
    /// wartende Landevorbereitung der Kabine nicht – trotz Kommando-Vorrang.
    /// Ausnahmen: Ruhephase (Kabine darf nicht sprechen) und Anflug mit noch
    /// nicht fertigem Kabinen-Ton (sonst hinge das Kommando bis Final).
    #[test]
    fn prepare_for_landing_ueberholt_die_kabine_nicht() {
        let beide = || {
            let mut w = Warteschlange::default();
            w.aufnehmen([
                f(Slot::KommandoLandungVorbereiten, Verfall::AbPhase(Landing)),
                f(Slot::KabineSinkflug, Verfall::AbPhase(Final)),
            ]);
            w
        };
        let mut w = beide();
        assert_eq!(w.naechstes(0, Descent, false), Some(Slot::KabineSinkflug));
        assert_eq!(w.naechstes(0, Descent, false), Some(Slot::KommandoLandungVorbereiten));
        // Kabinen-Ton im Sinkflug noch nicht fertig: Kommando wartet mit.
        let mut t = beide();
        assert_eq!(t.naechstes_bereit(0, Descent, false, |s| s != Slot::KabineSinkflug), None);
        // Im Anflug ohne fertigen Kabinen-Ton: Kommando geht.
        let mut a = beide();
        assert_eq!(a.naechstes_bereit(0, Approach, false, |s| s != Slot::KabineSinkflug), Some(Slot::KommandoLandungVorbereiten));
        // Ruhephase: Kabine darf nicht, Kommando geht.
        let mut r = beide();
        assert_eq!(r.naechstes(0, Descent, true), Some(Slot::KommandoLandungVorbereiten));
        // Pause nach einer Ansage: Die Kabine wartet sie ab, das Kommando
        // zieht trotz Kommando-Ausnahme nicht vorbei.
        let mut p = beide();
        p.ansage_spielt_bis(100);
        p.ansage_zu_ende(100);
        assert_eq!(p.naechstes(150, Descent, false), None);
        assert_eq!(p.naechstes(100 + PAUSE_NACH_ANSAGE_S, Descent, false), Some(Slot::KabineSinkflug));
    }

    #[test]
    fn turbulenz_wartet_keine_pause_und_vorhoeren_macht_keine() {
        let mut w = Warteschlange::default();
        w.ansage_zu_ende(0);
        w.aufnehmen([f(Slot::Turbulenz, Verfall::Um(500))]);
        assert_eq!(w.naechstes(10, Cruise, false), Some(Slot::Turbulenz));
        // Gegenprobe: Vorhören (spielt_bis + frei) löst keine Pause aus.
        let mut v = Warteschlange::default();
        v.spielt_bis(20);
        v.frei();
        v.aufnehmen([f(Slot::Reiseflug50, Verfall::AbPhase(Descent))]);
        assert!(!v.in_pause(5));
        assert_eq!(v.naechstes(5, Cruise, false), Some(Slot::Reiseflug50));
    }

    #[test]
    fn am_boden_begruessung_und_sicherheit_vor_dem_kapitaen() {
        let mut w = Warteschlange::default();
        w.aufnehmen([
            f(Slot::BegruessungTuerenZu, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::Sicherheit, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::KapitaenBegruessung, Verfall::AbPhase(TakeoffRoll)),
        ]);
        let mut reihe = Vec::new();
        while let Some(s) = w.naechstes(0, TaxiOut, false) {
            reihe.push(s);
        }
        assert_eq!(reihe, vec![Slot::BegruessungTuerenZu, Slot::Sicherheit, Slot::KapitaenBegruessung]);
        // Gegenprobe: Ist die Sicherheit noch nicht fertig, wartet der Kapitän nicht auf sie.
        let mut v = Warteschlange::default();
        v.aufnehmen([f(Slot::Sicherheit, Verfall::AbPhase(TakeoffRoll)), f(Slot::KapitaenBegruessung, Verfall::AbPhase(TakeoffRoll))]);
        assert_eq!(v.naechstes_bereit(0, TaxiOut, false, |s| s != Slot::Sicherheit), Some(Slot::KapitaenBegruessung));
    }

    /// Thomas 18.10.2026: Sicherheit höchstens 10 s nach der Begrüßung; der
    /// Kapitän wartet weiter 90 s.
    #[test]
    fn sicherheit_folgt_der_begruessung_nach_zehn_sekunden() {
        let mut w = Warteschlange::default();
        w.ansage_spielt_bis(100);
        w.ansage_zu_ende_nach(100, Slot::BegruessungTuerenZu);
        w.aufnehmen([f(Slot::Sicherheit, Verfall::AbPhase(TakeoffRoll)), f(Slot::KapitaenBegruessung, Verfall::AbPhase(Descent))]);
        assert_eq!(w.naechstes_bereit(105, TaxiOut, false, |s| s == Slot::KapitaenBegruessung), None, "Kapitän wartet 90 s");
        assert_eq!(w.naechstes(109, TaxiOut, false), None, "noch keine 10 s");
        assert_eq!(w.naechstes(110, TaxiOut, false), Some(Slot::Sicherheit));
        w.ansage_spielt_bis(300);
        w.ansage_zu_ende_nach(300, Slot::Sicherheit);
        // v2.0.28: Nach der Sicherheit wartet der Kapitän nur 30 s (QAF446).
        assert_eq!(w.naechstes(329, TaxiOut, false), None, "noch keine 30 s");
        assert_eq!(w.naechstes(300 + PAUSE_SICHERHEIT_KAPITAEN_S, TaxiOut, false), Some(Slot::KapitaenBegruessung));
    }

    #[test]
    fn nach_dem_kapitaen_wieder_neunzig_sekunden() {
        let mut w = Warteschlange::default();
        w.ansage_spielt_bis(100);
        w.ansage_zu_ende_nach(100, Slot::KapitaenBegruessung);
        w.aufnehmen([f(Slot::AnschnallzeichenAus, Verfall::AbPhase(Descent))]);
        assert_eq!(w.naechstes(130, Climb, false), None, "die 30-s-Ausnahme gilt nur für den Kapitän nach der Sicherheit");
        assert_eq!(w.naechstes(100 + PAUSE_NACH_ANSAGE_S, Climb, false), Some(Slot::AnschnallzeichenAus));
    }

    #[test]
    fn anschnallzeichen_aus_durchbricht_die_ruhephase_nur_im_steigflug() {
        let mut w = Warteschlange::default();
        let mut aus = f(Slot::AnschnallzeichenAus, Verfall::AbPhase(Descent));
        aus.trotz_ruhephase = true;
        w.aufnehmen([aus, f(Slot::Reiseflug25, Verfall::AbPhase(Descent))]);
        assert_eq!(w.naechstes(0, Takeoff, true), None, "beim Abheben gesperrt");
        assert_eq!(w.naechstes(1, Climb, true), Some(Slot::AnschnallzeichenAus));
        w.ansage_zu_ende(2);
        assert_eq!(w.naechstes(2 + PAUSE_NACH_ANSAGE_S, Climb, true), None, "der Service wartet weiter auf FL100");
        assert_eq!(w.naechstes(2 + PAUSE_NACH_ANSAGE_S, Climb, false), Some(Slot::Reiseflug25));
    }

    #[test]
    fn boarding_completed_ueberholt_die_boarding_ansage_nicht() {
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::Boarding, Verfall::AbPhase(Pushback)), f(Slot::BoardingAbgeschlossen, Verfall::AbPhase(Pushback))]);
        assert_eq!(w.naechstes_bereit(0, Boarding, false, |s| s != Slot::Boarding), None, "Boarding-Ton fehlt noch: warten");
        assert_eq!(w.naechstes(1, Boarding, false), Some(Slot::Boarding));
        w.ansage_zu_ende(30);
        assert_eq!(w.naechstes(31, Boarding, false), Some(Slot::BoardingAbgeschlossen), "Kommando wartet keine Pause ab");
    }

    #[test]
    fn entfernen_und_alle_entnehmen() {
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::Sicherheit, Verfall::Um(100)), f(Slot::Boarding, Verfall::Um(100)), f(Slot::KommandoStart, Verfall::Um(100))]);
        assert!(w.entfernen(Slot::Sicherheit));
        assert!(!w.entfernen(Slot::Sicherheit), "nur einmal");
        assert_eq!(w.alle_entnehmen(), vec![Slot::Boarding, Slot::KommandoStart]);
        assert!(w.ist_leer());
    }

    #[test]
    fn durchstart_ansage_spielt_auch_in_der_ruhephase() {
        let mut w = Warteschlange::default();
        w.aufnehmen([
            f(Slot::Durchstarten, Verfall::AbPhase(Final)),
            f(Slot::Reiseflug50, Verfall::AbPhase(Descent)),
        ]);
        // Unter 10.000 ft nach dem Durchstarten: Ruhephase, trotzdem spielt sie.
        assert_eq!(w.naechstes(10, Climb, true), Some(Slot::Durchstarten));
        // Gegenprobe: eine normale Ansage bleibt in der Ruhephase gesperrt.
        assert_eq!(w.naechstes(10, Climb, true), None);
    }
    /// 2.1.1 A/E: Begrüßung → „Boarding completed“ → „Türen zu“ (Kommando
    /// und Kabine) → Sicherheit, auch gegen den Kommando-Vorrang.
    #[test]
    fn beacon_reihenfolge_auch_gegen_kommandos() {
        let mut w = Warteschlange::default();
        w.aufnehmen([
            f(Slot::Boarding, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::BoardingAbgeschlossen, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::KommandoTuerenScharf, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::BegruessungTuerenZu, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::Sicherheit, Verfall::AbPhase(TakeoffRoll)),
        ]);
        let mut reihe = Vec::new();
        let mut t = 0;
        while !w.ist_leer() && t < 600 {
            if let Some(s) = w.naechstes(t, Boarding, false) {
                reihe.push((t, s));
                if s.loest_pause_aus() {
                    w.ansage_spielt_bis(t + 20);
                    w.ansage_zu_ende_nach(t + 20, s);
                } else {
                    w.spielt_bis(t + 3);
                    w.frei();
                }
            }
            t += 1;
        }
        assert_eq!(
            reihe,
            vec![
                (0, Slot::Boarding),
                (1, Slot::BoardingAbgeschlossen),
                (2, Slot::KommandoTuerenScharf),
                (30, Slot::BegruessungTuerenZu),
                (60, Slot::Sicherheit),
            ]
        );
        // Ton der Begrüßung noch nicht fertig: „Türen zu“ überholt sie nicht
        // (Codex 2.1.1), auch das Kommando nicht.
        let mut v = Warteschlange::default();
        v.aufnehmen([
            f(Slot::Boarding, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::KommandoTuerenScharf, Verfall::AbPhase(TakeoffRoll)),
            f(Slot::BegruessungTuerenZu, Verfall::AbPhase(TakeoffRoll)),
        ]);
        assert_eq!(v.naechstes_bereit(0, Boarding, false, |s| s != Slot::Boarding), None);
        assert_eq!(v.naechstes_bereit(1, Boarding, false, |_| true), Some(Slot::Boarding));
    }

    /// 2.1.1 C: „Ende des Service“ wartet auf einen gemeldeten Service; beim
    /// Sinkflug verfällt der zuerst, dann darf das Ende.
    #[test]
    fn ende_service_nie_vor_wartendem_service() {
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::Reiseflug25, Verfall::AbPhase(Descent)), f(Slot::EndeService, Verfall::AbPhase(Approach))]);
        assert_eq!(w.naechstes(0, Climb, true), None, "Ruhephase: das Ende überholt den Service nicht");
        assert_eq!(w.naechstes(1, Descent, true), Some(Slot::EndeService));
        assert_eq!(w.verfallene_abholen(), vec![Slot::Reiseflug25]);
    }

    #[test]
    fn ende_service_vor_dem_kapitaen_beim_sinkflug() {
        let mut w = Warteschlange::default();
        w.aufnehmen([f(Slot::KapitaenSinkflug, Verfall::AbPhase(Final)), f(Slot::EndeService, Verfall::AbPhase(Approach))]);
        assert_eq!(w.naechstes(0, Descent, false), Some(Slot::EndeService));
        // Ohne Ton für die Kabine wartet der Kapitän nicht.
        let mut v = Warteschlange::default();
        v.aufnehmen([f(Slot::KapitaenSinkflug, Verfall::AbPhase(Final)), f(Slot::EndeService, Verfall::AbPhase(Approach))]);
        assert_eq!(v.naechstes_bereit(0, Descent, false, |s| s != Slot::EndeService), Some(Slot::KapitaenSinkflug));
    }

}
