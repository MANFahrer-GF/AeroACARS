//! v3 Etappe 4 (P-3.58): Befehlskanal des Kabinenmoduls `module/kabine.wasm`.
//!
//! Eine Modulinstanz je Flug haelt den [`Planer`](crate::Planer) in ihrem
//! Speicher; der Client schickt je Aufruf einen [`Befehl`] als JSON an den
//! Export `kabine` und bekommt `{"Ok": Wert}` oder `{"Err": Text}` zurueck.
//! Je Methode des Planers ein Befehl, gleiche Namen und Argumente.
//!
//! Die Tabelle (`tabellen/kabine.json`) kommt als Text bei `Neu`,
//! `Fortsetzen` und `Laden` mit: So wirken Paket-Tabelle und Paket-Modul
//! unabhaengig voneinander, und Modul und Client lesen dieselbe Tabelle
//! (Reaktion, Warteschlange und Musik laufen weiter im Client).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sim_core::FlightPhase;

use crate::planer::{Flugdaten, Planer, Tick};
use crate::slot::Slot;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "befehl", rename_all = "snake_case")]
pub enum Befehl {
    Neu { tabelle: String, flug: Flugdaten },
    Fortsetzen { tabelle: String, flug: Flugdaten, phase: FlightPhase },
    /// Stand aus [`Befehl::Sichern`] uebernehmen (Rueckfall auf ein anderes Modul).
    Laden { tabelle: String, planer: String },
    /// Der Planer als JSON (serde-Rundlauf, Test `rundlauf_aendert_nichts`).
    Sichern,
    Tick { tick: Tick },
    Ruhephase { tick: Tick },
    RestflugMin { jetzt: i64, rest_min: Option<f64> },
    SchonGespielt { slots: Vec<Slot> },
    Reiseflug80Art { zweiter_service: Option<bool> },
    EntfalleneAbholen,
    KapitaenSinkflugEnde { gespielt: bool, jetzt: i64 },
    ServiceEnde { gespielt: bool },
    ServiceEntfallen,
    Abwaehlen { slots: Vec<Slot> },
    Erledigt { slot: Slot },
    TuerenZu,
    KommtNoch { slot: Slot, phase: FlightPhase },
    FlugzeitBekannt,
    IstKurzstrecke,
    Zeitpunkt { slot: Slot, landung_erwartet: Option<i64> },
    KapitaenSinkflugPrognose { landung_erwartet: Option<i64> },
}

impl Befehl {
    /// Legt der Befehl einen Planer an (`Neu`, `Fortsetzen`, `Laden`)?
    pub fn legt_an(&self) -> bool {
        matches!(self, Befehl::Neu { .. } | Befehl::Fortsetzen { .. } | Befehl::Laden { .. })
    }

    /// Aendert der Befehl den Planer? (Der Client merkt sich diese Befehle,
    /// um den Stand bei einem Rueckfall nachzuspielen.)
    pub fn aendert(&self) -> bool {
        self.legt_an()
            || matches!(
                self,
                Befehl::Tick { .. }
                    | Befehl::SchonGespielt { .. }
                    | Befehl::Reiseflug80Art { .. }
                    | Befehl::EntfalleneAbholen
                    | Befehl::KapitaenSinkflugEnde { .. }
                    | Befehl::ServiceEnde { .. }
                    | Befehl::ServiceEntfallen
                    | Befehl::Abwaehlen { .. }
            )
    }
}

/// Legt den Planer fuer `Neu`, `Fortsetzen` und `Laden` an. Die Tabelle
/// setzt der Aufrufer (Modul: aus dem Befehl; Client: sein Paketstand).
pub fn erzeugen(b: Befehl) -> Result<Planer, String> {
    match b {
        Befehl::Neu { flug, .. } => Ok(Planer::neu(flug)),
        Befehl::Fortsetzen { flug, phase, .. } => Ok(Planer::fortsetzen(flug, phase)),
        Befehl::Laden { planer, .. } => serde_json::from_str(&planer).map_err(|e| format!("Stand: {e}")),
        b => Err(format!("{b:?} legt keinen Planer an")),
    }
}

fn wert<T: Serialize>(t: T) -> Result<Value, String> {
    serde_json::to_value(t).map_err(|e| e.to_string())
}

/// Fuehrt einen Befehl am Planer aus; die Antwort ist der Rueckgabewert der
/// gleichnamigen Methode.
pub fn anwenden(p: &mut Planer, b: Befehl) -> Result<Value, String> {
    match b {
        Befehl::Neu { .. } | Befehl::Fortsetzen { .. } | Befehl::Laden { .. } => Err("Planer besteht schon".into()),
        Befehl::Sichern => wert(serde_json::to_string(p).map_err(|e| e.to_string())?),
        Befehl::Tick { tick } => wert(p.tick(&tick)),
        Befehl::Ruhephase { tick } => wert(p.ruhephase(&tick)),
        Befehl::RestflugMin { jetzt, rest_min } => wert(p.restflug_min(jetzt, rest_min)),
        Befehl::SchonGespielt { slots } => wert(p.schon_gespielt(slots)),
        Befehl::Reiseflug80Art { zweiter_service } => wert(p.reiseflug80_art(zweiter_service)),
        Befehl::EntfalleneAbholen => wert(p.entfallene_abholen()),
        Befehl::KapitaenSinkflugEnde { gespielt, jetzt } => wert(p.kapitaen_sinkflug_ende(gespielt, jetzt)),
        Befehl::ServiceEnde { gespielt } => wert(p.service_ende(gespielt)),
        Befehl::ServiceEntfallen => wert(p.service_entfallen()),
        Befehl::Abwaehlen { slots } => wert(p.abwaehlen(slots)),
        Befehl::Erledigt { slot } => wert(p.erledigt(slot)),
        Befehl::TuerenZu => wert(p.tueren_zu()),
        Befehl::KommtNoch { slot, phase } => wert(p.kommt_noch(slot, phase)),
        Befehl::FlugzeitBekannt => wert(p.flugzeit_bekannt()),
        Befehl::IstKurzstrecke => wert(p.ist_kurzstrecke()),
        Befehl::Zeitpunkt { slot, landung_erwartet } => wert(p.zeitpunkt(slot, landung_erwartet)),
        Befehl::KapitaenSinkflugPrognose { landung_erwartet } => wert(p.kapitaen_sinkflug_prognose(landung_erwartet)),
    }
}
