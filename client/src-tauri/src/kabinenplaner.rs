//! v3 Etappe 4 (P-3.58): Der Kabinenplaner laeuft als WASM-Modul
//! `module/kabine.wasm` aus dem Laufzeit-Paket — Planen ist vom Abspielen
//! getrennt, der Ablauf aendert sich mit dem Paket, ohne neuen Client.
//!
//! [`Planer`] hat die Methoden von `cabin_announcements::Planer`, die der
//! Client aufruft (gleiche Namen und Signaturen; `tueren_zu` braucht er
//! nicht, der Befehl `TuerenZu` besteht im Modul); dahinter liegt EINE Modulinstanz je Flug (Zustand im Modulspeicher), die
//! je Methode einen [`Befehl`] bekommt. Die Tabelle `tabellen/kabine.json`
//! des Paketstands geht mit dem ersten Befehl ins Modul.
//!
//! Rueckfall (Ansagen duerfen den Flug nie aufhalten): Modul des Pakets →
//! eingebautes Modul → Planer nativ. Anders als bei der Bewertung (P-4.31)
//! ist nativ hier erlaubt: Der Planer ist kein Bewertungsregelwerk, und er
//! liest nativ dieselbe Pakettabelle. Fuer den Wechsel merkt sich die
//! Fassade den Stand nach dem letzten Takt (`Sichern`, serde-Rundlauf) und
//! die seither aendernden Befehle und spielt beides im naechsten Weg nach.
//! Welcher Weg lief, steht als Kabinen-Notiz `planer` im Flugprotokoll.

use std::sync::{Mutex, PoisonError};

use bewertung_host::{Host, Instanz};
use cabin_announcements::modul::{self, Befehl};
use cabin_announcements::{Entfall, Faellig, Flugdaten, Slot, Tick};
use serde::de::DeserializeOwned;
use serde_json::Value;
use sim_core::FlightPhase;

/// Rechenzeitgrenze je Befehl. Gemessen 29.10. ueber sechs echte Fluege
/// (Test `messen` in kabine-wasm): Takt Median 0,17 Mio., Maximum 0,43 Mio.;
/// Anlegen (liest die Tabelle) 5,6 Mio. Also rund das Neunfache des
/// teuersten Befehls, gleich der Grenze der Bewertung.
pub(crate) const FUEL_GRENZE: u64 = 50_000_000;

const EXPORTE: &[&str] = &["memory", "alloc", "kabine"];

enum Weg {
    Paket(Instanz),
    Eingebaut(Instanz),
    Nativ(Option<cabin_announcements::Planer>),
}

impl Weg {
    fn name(&self) -> &'static str {
        match self {
            Weg::Paket(_) => "paket",
            Weg::Eingebaut(_) => "eingebaut",
            Weg::Nativ(_) => "nativ",
        }
    }

    fn rufen(&mut self, b: &Befehl) -> Result<Value, String> {
        #[cfg(test)]
        if STOEREN.with(|s| s.get().contains(&self.name())) {
            return Err("gestoert (Test)".into());
        }
        match self {
            Weg::Paket(i) | Weg::Eingebaut(i) => {
                let json = serde_json::to_string(b).map_err(|e| e.to_string())?;
                let (aus, _) = i.aufrufen(&json, FUEL_GRENZE).map_err(|e| e.to_string())?;
                serde_json::from_str::<Result<Value, String>>(&aus)
                    .map_err(|_| format!("Antwort ungueltig: {}", aus.chars().take(200).collect::<String>()))?
            }
            Weg::Nativ(p) if b.legt_an() => {
                *p = Some(modul::erzeugen(b.clone())?);
                Ok(Value::Null)
            }
            Weg::Nativ(p) => modul::anwenden(p.as_mut().ok_or("kein Planer")?, b.clone()),
        }
    }
}

// Nur fuer Tests: Befehle in diesen Wegen scheitern (wie ein Absturz im
// Modul), je Thread.
#[cfg(test)]
thread_local! {
    static STOEREN: std::cell::Cell<&'static [&'static str]> = const { std::cell::Cell::new(&[]) };
}

fn instanz(wasm: &[u8]) -> Result<Instanz, String> {
    Host::neu_mit(wasm, EXPORTE)
        .and_then(|h| h.instanz("kabine", FUEL_GRENZE))
        .map_err(|e| e.to_string())
}

/// Selbsttest eines Paketstands: Das Modul laedt und legt mit der Tabelle
/// dieses Stands einen Planer an.
pub(crate) fn pruefen(wasm: &[u8], tabelle: &str) -> Result<(), String> {
    let b = Befehl::Neu { tabelle: tabelle.to_string(), flug: Flugdaten::default() };
    Weg::Paket(instanz(wasm)?).rufen(&b).map(|_| ())
}

struct Innen {
    weg: Weg,
    eingebaut: Option<&'static [u8]>,
    /// Damit laesst sich der Stand in einem anderen Weg nachbauen: ein
    /// anlegender Befehl (zuerst `Neu`/`Fortsetzen`, nach jedem Takt `Laden`
    /// mit dem gesicherten Stand), dann die seither aendernden Befehle.
    nachbau: Vec<Befehl>,
    tabelle: &'static str,
    /// Weg-Wechsel fuers Flugprotokoll: (Weg, Grund).
    wechsel: Vec<(&'static str, Option<String>)>,
    /// Kein Weg konnte den Stand uebernehmen: Der Client baut den Planer
    /// neu auf (wie nach einem Neustart).
    unbrauchbar: bool,
}

impl Innen {
    /// Den Stand im naechsten Weg nachbauen. Ohne Erfolg: `unbrauchbar`.
    fn ausweichen(&mut self, grund: String) {
        let ab = match self.weg {
            Weg::Paket(_) => 1,
            Weg::Eingebaut(_) => 2,
            Weg::Nativ(_) => 3,
        };
        self.ausweichen_ab(ab, grund);
    }

    /// Stufen: 1 eingebautes Modul, 2 nativ.
    fn ausweichen_ab(&mut self, ab: u8, mut grund: String) {
        tracing::warn!(weg = self.weg.name(), grund = %grund, "Kabinenplaner: Rueckfall");
        for stufe in ab..=2 {
            let weg = match (stufe, self.eingebaut) {
                (1, Some(wasm)) => instanz(wasm).map(Weg::Eingebaut),
                (1, None) => continue,
                _ => Ok(Weg::Nativ(None)),
            };
            match weg.and_then(|mut w| self.nachbau.iter().try_for_each(|b| w.rufen(b).map(|_| ())).map(|()| w)) {
                Ok(w) => {
                    self.wechsel.push((w.name(), Some(grund)));
                    self.weg = w;
                    return;
                }
                Err(e) => grund = format!("{grund}; {e}"),
            }
        }
        tracing::error!(grund = %grund, "Kabinenplaner: kein Weg uebernimmt den Stand, Neuaufbau");
        self.wechsel.push(("neuaufbau", Some(grund)));
        self.unbrauchbar = true;
    }

    /// Nach einem erfolgreichen Befehl: fuer den Nachbau merken; nach einem
    /// Takt den Stand sichern (dann reicht `Laden` statt aller Befehle).
    fn merken(&mut self, b: Befehl) {
        if !b.aendert() {
            return;
        }
        let takt = matches!(b, Befehl::Tick { .. });
        self.nachbau.push(b);
        if takt {
            match self.weg.rufen(&Befehl::Sichern).and_then(|v| serde_json::from_value::<String>(v).map_err(|e| e.to_string())) {
                Ok(planer) => self.nachbau = vec![Befehl::Laden { tabelle: self.tabelle.to_string(), planer }],
                Err(e) => self.ausweichen(e),
            }
        }
    }
}

/// Der Kabinenplaner eines Flugs (Fassade, siehe Moduldoku).
pub(crate) struct Planer(Mutex<Innen>);

impl Planer {
    #[cfg(test)]
    pub fn neu(flug: Flugdaten) -> Self {
        let s = crate::laufzeit_paket::stand();
        Self::starten(s.kabine_modul, crate::laufzeit_paket::eingebautes_kabinenmodul(), s.kabine_json, Befehl::Neu { tabelle: s.kabine_json.into(), flug })
    }

    pub fn fortsetzen(flug: Flugdaten, phase: FlightPhase) -> Self {
        let s = crate::laufzeit_paket::stand();
        Self::starten(
            s.kabine_modul,
            crate::laufzeit_paket::eingebautes_kabinenmodul(),
            s.kabine_json,
            Befehl::Fortsetzen { tabelle: s.kabine_json.into(), flug, phase },
        )
    }

    /// Mit dem Modul `paket` anlegen; scheitert das, wie im Flug ausweichen.
    pub(crate) fn starten(paket: &[u8], eingebaut: Option<&'static [u8]>, tabelle: &'static str, anlegen: Befehl) -> Self {
        let mut innen = Innen {
            weg: Weg::Nativ(None),
            eingebaut,
            nachbau: vec![anlegen.clone()],
            tabelle,
            wechsel: Vec::new(),
            unbrauchbar: false,
        };
        match instanz(paket).map(Weg::Paket).and_then(|mut w| w.rufen(&anlegen).map(|_| w)) {
            Ok(w) => {
                innen.weg = w;
                innen.wechsel.push(("paket", None));
            }
            Err(e) => innen.ausweichen_ab(1, format!("paket: {e}")),
        }
        Self(Mutex::new(innen))
    }

    /// Einen Befehl im aktuellen Weg ausfuehren; scheitert er, im naechsten
    /// Weg (mit nachgebautem Stand) wiederholen. Unbrauchbar: Vorgabewert.
    fn rufen<T: DeserializeOwned + Default>(&self, b: Befehl) -> T {
        let mut innen = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if innen.unbrauchbar {
                return T::default();
            }
            let weg = innen.weg.name();
            match innen.weg.rufen(&b).and_then(|v| serde_json::from_value::<T>(v).map_err(|e| e.to_string())) {
                Ok(t) => {
                    innen.merken(b);
                    return t;
                }
                Err(e) => innen.ausweichen(format!("{weg}: {e}")),
            }
        }
    }

    /// Weg-Wechsel seit dem letzten Abholen: (Weg, Grund) — beim Anlegen der
    /// erste Weg ohne Grund. Fuer die Kabinen-Notiz `planer`.
    pub(crate) fn wechsel_abholen(&self) -> Vec<(&'static str, Option<String>)> {
        std::mem::take(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner).wechsel)
    }

    /// Kein Weg konnte den Stand uebernehmen: neu aufbauen wie nach einem
    /// Neustart (`kabinen_schritt`).
    pub(crate) fn unbrauchbar(&self) -> bool {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).unbrauchbar
    }

    // ---- wie `cabin_announcements::Planer` (was der Client aufruft) ----

    pub fn restflug_min(&self, jetzt: i64, rest_min: Option<f64>) -> Option<f64> {
        self.rufen(Befehl::RestflugMin { jetzt, rest_min })
    }

    pub fn schon_gespielt(&mut self, slots: impl IntoIterator<Item = Slot>) {
        self.rufen::<()>(Befehl::SchonGespielt { slots: slots.into_iter().collect() });
    }

    pub fn reiseflug80_art(&mut self, zweiter_service: Option<bool>) {
        self.rufen::<()>(Befehl::Reiseflug80Art { zweiter_service });
    }

    pub fn entfallene_abholen(&mut self) -> Vec<(Slot, Entfall)> {
        self.rufen(Befehl::EntfalleneAbholen)
    }

    pub fn kapitaen_sinkflug_ende(&mut self, gespielt: bool, jetzt: i64) {
        self.rufen::<()>(Befehl::KapitaenSinkflugEnde { gespielt, jetzt });
    }

    pub fn service_ende(&mut self, gespielt: bool) {
        self.rufen::<()>(Befehl::ServiceEnde { gespielt });
    }

    pub fn service_entfallen(&mut self) {
        self.rufen::<()>(Befehl::ServiceEntfallen);
    }

    pub fn ruhephase(&self, t: &Tick) -> bool {
        self.rufen(Befehl::Ruhephase { tick: t.clone() })
    }

    pub fn abwaehlen(&mut self, slots: impl IntoIterator<Item = Slot>) {
        self.rufen::<()>(Befehl::Abwaehlen { slots: slots.into_iter().collect() });
    }

    pub fn erledigt(&self, slot: Slot) -> bool {
        self.rufen(Befehl::Erledigt { slot })
    }

    pub fn kommt_noch(&self, slot: Slot, phase: FlightPhase) -> bool {
        self.rufen(Befehl::KommtNoch { slot, phase })
    }

    pub fn flugzeit_bekannt(&self) -> bool {
        self.rufen(Befehl::FlugzeitBekannt)
    }

    pub fn ist_kurzstrecke(&self) -> bool {
        self.rufen(Befehl::IstKurzstrecke)
    }

    pub fn zeitpunkt(&self, slot: Slot, landung_erwartet: Option<i64>) -> Option<i64> {
        self.rufen(Befehl::Zeitpunkt { slot, landung_erwartet })
    }

    pub fn kapitaen_sinkflug_prognose(&self, landung_erwartet: Option<i64>) -> Option<i64> {
        self.rufen(Befehl::KapitaenSinkflugPrognose { landung_erwartet })
    }

    pub fn tick(&mut self, t: &Tick) -> Vec<Faellig> {
        self.rufen(Befehl::Tick { tick: t.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cabin_announcements::Flugart;

    type Takt = (i64, FlightPhase, f64, f64, f64, u8, Option<u8>, u8, u8, f64, u8, u8, f64, Option<u8>);

    #[derive(serde::Deserialize)]
    struct Daten {
        landung: i64,
        ticks: Vec<Takt>,
    }

    fn stoeren(wege: &'static [&'static str]) {
        STOEREN.with(|s| s.set(wege));
    }

    fn modul() -> &'static [u8] {
        crate::laufzeit_paket::eingebautes_kabinenmodul().unwrap()
    }

    fn flug() -> Flugdaten {
        Flugdaten { flugart: Some(Flugart::Linie), geplante_flugzeit_min: Some(100), reiseflughoehe_ft: Some(37_000), ..Default::default() }
    }

    fn anlegen(phase: FlightPhase) -> Befehl {
        Befehl::Fortsetzen { tabelle: cabin_announcements::EINGEBAUT.into(), flug: flug(), phase }
    }

    /// DLH16 Takt fuer Takt: Fassade und Planer nativ melden dasselbe. Ab
    /// Takt `stoerung` scheitern Befehle in den Wegen `wege`.
    fn fliegen(p: &mut Planer, stoerung: usize, wege: &'static [&'static str]) -> usize {
        let d: Daten = serde_json::from_str(include_str!("../crates/cabin-announcements/tests/daten/dlh16.json")).unwrap();
        let mut n = cabin_announcements::Planer::fortsetzen(flug(), d.ticks[0].1);
        let mut gemeldet = 0;
        for (i, &(t, phase, alt, agl, vs, boden, zeichen, beacon, licht, fahrwerk, pushback, triebwerke, zfw, _)) in d.ticks.iter().enumerate() {
            if i == stoerung {
                stoeren(wege);
            }
            let tk = Tick {
                jetzt: t,
                phase,
                hoehe_ft: alt,
                vs_fpm: vs,
                am_boden: boden == 1,
                anschnallzeichen: zeichen,
                pushback_aktiv: pushback == 1,
                rest_min: (t < d.landung).then(|| (d.landung - t) as f64 / 60.0),
                beacon: Some(beacon == 1),
                landelicht: Some(licht == 1),
                fahrwerk_unten: Some(fahrwerk >= 0.9),
                hoehe_agl_ft: Some(agl),
                triebwerke_an: Some(triebwerke > 0),
                zfw_kg: (zfw > 0.0).then_some(zfw),
                ..Default::default()
            };
            let f = n.tick(&tk);
            assert_eq!(p.tick(&tk), f, "t={t}");
            assert_eq!(p.ruhephase(&tk), n.ruhephase(&tk), "t={t}");
            assert_eq!(p.kommt_noch(Slot::Aussteigen, phase), n.kommt_noch(Slot::Aussteigen, phase), "t={t}");
            gemeldet += f.len();
        }
        stoeren(&[]);
        gemeldet
    }

    #[test]
    fn paketmodul_plant_und_der_weg_steht_im_protokoll() {
        let mut p = Planer::starten(modul(), Some(modul()), cabin_announcements::EINGEBAUT, anlegen(FlightPhase::Preflight));
        assert!(fliegen(&mut p, usize::MAX, &[]) > 10);
        assert_eq!(p.wechsel_abholen(), vec![("paket", None)]);
        assert!(p.wechsel_abholen().is_empty(), "nur einmal");
    }

    /// Paketmodul nicht ladbar: eingebautes Modul, Grund im Protokoll.
    #[test]
    fn kaputtes_paketmodul_faellt_auf_das_eingebaute() {
        let mut p = Planer::starten(b"\0asm kaputt", Some(modul()), cabin_announcements::EINGEBAUT, anlegen(FlightPhase::Preflight));
        let w = p.wechsel_abholen();
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].0, "eingebaut");
        assert!(w[0].1.as_deref().unwrap().starts_with("paket: Modul"), "{w:?}");
        assert!(fliegen(&mut p, usize::MAX, &[]) > 10);
    }

    /// Beide Module unbrauchbar: nativ — die Ansagen laufen weiter.
    #[test]
    fn beide_kaputt_plant_nativ() {
        let mut p = Planer::starten(b"\0asm kaputt", None, cabin_announcements::EINGEBAUT, anlegen(FlightPhase::Preflight));
        assert_eq!(p.wechsel_abholen().iter().map(|w| w.0).collect::<Vec<_>>(), ["nativ"]);
        assert!(fliegen(&mut p, usize::MAX, &[]) > 10);
    }

    /// Absturz mitten im Flug (nach dem Boarding): Der gesicherte Stand geht
    /// ins eingebaute Modul, danach nativ — kein Takt meldet anders.
    #[test]
    fn stoerung_im_flug_uebernimmt_den_stand() {
        let mut p = Planer::starten(modul(), Some(modul()), cabin_announcements::EINGEBAUT, anlegen(FlightPhase::Preflight));
        assert!(fliegen(&mut p, 300, &["paket"]) > 10);
        let w = p.wechsel_abholen();
        assert_eq!(w.iter().map(|w| w.0).collect::<Vec<_>>(), ["paket", "eingebaut"]);
        let mut q = Planer::starten(modul(), Some(modul()), cabin_announcements::EINGEBAUT, anlegen(FlightPhase::Preflight));
        assert!(fliegen(&mut q, 300, &["paket", "eingebaut"]) > 10);
        assert_eq!(q.wechsel_abholen().iter().map(|w| w.0).collect::<Vec<_>>(), ["paket", "nativ"]);
        assert!(!q.unbrauchbar());
    }

    /// Uebernimmt kein Weg den Stand: unbrauchbar, Vorgabewerte, der Client
    /// baut neu auf.
    #[test]
    fn ohne_uebernahme_wird_neu_aufgebaut() {
        let p = Planer::starten(modul(), None, cabin_announcements::EINGEBAUT, anlegen(FlightPhase::Cruise));
        p.0.lock().unwrap().nachbau = vec![Befehl::Laden { tabelle: cabin_announcements::EINGEBAUT.into(), planer: "kaputt".into() }];
        stoeren(&["paket"]);
        assert!(!p.kommt_noch(Slot::Aussteigen, FlightPhase::Cruise));
        stoeren(&[]);
        assert!(p.unbrauchbar());
        assert_eq!(p.wechsel_abholen().last().unwrap().0, "neuaufbau");
    }
}
