//! v3 Etappe 2 (P-3.52): Paketprofile fuer neue MSFS-Add-ons anbinden —
//! plattformunabhaengig, damit Erkennung, Anmeldung der LVars und das
//! Anwenden auch auf dem Mac getestet werden. Das Verdrahten mit SimConnect
//! (eigene Datendefinition #5) steht im Windows-Adapter.
//!
//! Ablauf je Snapshot ([`PaketAnbindung::schritt`]):
//! 1. Nur wenn die eingebaute Erkennung `Default` ergab, wird ein Paketprofil
//!    gesucht (Titel, ICAO, `aircraft.cfg`-Pfad).
//! 2. Wechselt das Profil, bekommt der [`ZusatzState`] die neue LVar-Liste
//!    (der Adapter legt die Definition daraufhin neu an) und der Laufzustand
//!    beginnt neu — ein Wert des vorigen Flugzeugs gilt nicht weiter.
//! 3. Die Werte des letzten gueltigen Blocks werden NACH der eingebauten
//!    Logik angewendet; eine Regel greift erst, wenn eine ihrer LVars einmal
//!    ungleich 0 war (`msfs_paketprofil::Lauf`).

use std::collections::HashMap;

use sim_core::msfs_paketprofil::{self, Lauf, Profil};
use sim_core::{AircraftProfile, SimSnapshot};

use crate::zusatz::{ZusatzFeld, ZusatzState};

/// Laufzustand der Anbindung (eine Instanz je Adapter).
#[derive(Debug, Default)]
pub struct PaketAnbindung {
    aktiv: Option<(String, Lauf)>,
}

impl PaketAnbindung {
    /// Name des aktiven Paketprofils (Diagnose).
    pub fn aktiv(&self) -> Option<&str> {
        self.aktiv.as_ref().map(|(n, _)| n.as_str())
    }

    /// Ein Snapshot: Profil bestimmen, bei Wechsel `zusatz` neu setzen, Werte
    /// anwenden. `profile` sind die geltenden Paketprofile.
    pub fn schritt(&mut self, profile: &[Profil], snap: &mut SimSnapshot, pfad: Option<&str>, zusatz: &mut ZusatzState) {
        let gefunden = if snap.aircraft_profile == AircraftProfile::Default {
            msfs_paketprofil::erkennen(
                profile,
                snap.aircraft_title.as_deref().unwrap_or(""),
                snap.aircraft_icao.as_deref().unwrap_or(""),
                pfad,
            )
        } else {
            None
        };
        if gefunden.map(|p| p.name.as_str()) != self.aktiv() {
            zusatz.setzen(gefunden.map(felder).unwrap_or_default());
            self.aktiv = gefunden.map(|p| (p.name.clone(), Lauf::default()));
            if let Some(p) = gefunden {
                tracing::info!(profil = %p.name, "MSFS-Paketprofil aktiv");
            }
        }
        if let (Some(p), Some((_, lauf))) = (gefunden, self.aktiv.as_mut()) {
            let werte: HashMap<String, f64> = zusatz.werte().into_iter().collect();
            lauf.anwenden(p, &werte, snap);
        }
    }
}

/// LVar-Liste eines Profils fuer die Datendefinition (Kanal = Name).
fn felder(p: &Profil) -> Vec<ZusatzFeld> {
    msfs_paketprofil::variablen(p)
        .into_iter()
        .map(|v| ZusatzFeld { kanal: v.to_string(), simvar: v.to_string(), einheit: "Number".into() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    const PROFIL: &str = r#"{"format":1,"profile":[{"name":"Probe","erkennung":{"titel_enthaelt_alle":["probejet"]},"regeln":[
        {"feld":"licht_beacon","quellen":["L:P_BEACON"],"abbildung":{"art":"ungleich_null"}},
        {"feld":"ap_master","quellen":["L:P_AP"],"abbildung":{"art":"ungleich_null"}}
    ]}]}"#;

    fn snap(titel: &str, profil: AircraftProfile) -> SimSnapshot {
        SimSnapshot { aircraft_title: Some(titel.into()), aircraft_profile: profil, ..Default::default() }
    }

    /// Simuliert, was der Adapter tut: Definition anlegen, einen Block liefern.
    fn block_liefern(z: &mut ZusatzState, werte: &[f64]) {
        let reihenfolge = z.zu_registrieren().iter().map(|(i, _)| *i).collect();
        let t0 = Instant::now() - Duration::from_secs(5);
        z.registriert(reihenfolge, Vec::new(), t0);
        let bytes: Vec<u8> = werte.iter().flat_map(|v| v.to_le_bytes()).collect();
        z.einlesen(&bytes, Instant::now());
    }

    #[test]
    fn neues_addon_wird_angebunden_und_angewendet() {
        let d = msfs_paketprofil::lesen(PROFIL).unwrap();
        let mut a = PaketAnbindung::default();
        let mut z = ZusatzState::default();
        let mut s = snap("ProbeJet 100", AircraftProfile::Default);
        a.schritt(&d.profile, &mut s, None, &mut z);
        assert_eq!(a.aktiv(), Some("Probe"));
        assert!(z.dirty, "Adapter muss die LVars anmelden");
        block_liefern(&mut z, &[1.0, 1.0]);
        let mut s = snap("ProbeJet 100", AircraftProfile::Default);
        a.schritt(&d.profile, &mut s, None, &mut z);
        assert_eq!((s.light_beacon, s.autopilot_master), (Some(true), Some(true)));
    }

    #[test]
    fn eingebautes_profil_hat_vorrang() {
        let d = msfs_paketprofil::lesen(PROFIL).unwrap();
        let mut a = PaketAnbindung::default();
        let mut z = ZusatzState::default();
        let mut s = snap("ProbeJet 100", AircraftProfile::FenixA320);
        a.schritt(&d.profile, &mut s, None, &mut z);
        assert_eq!(a.aktiv(), None);
        assert!(!z.dirty);
    }

    #[test]
    fn flugzeugwechsel_vergisst_das_profil() {
        let d = msfs_paketprofil::lesen(PROFIL).unwrap();
        let mut a = PaketAnbindung::default();
        let mut z = ZusatzState::default();
        a.schritt(&d.profile, &mut snap("ProbeJet 100", AircraftProfile::Default), None, &mut z);
        block_liefern(&mut z, &[1.0, 1.0]);
        let mut s = snap("Asobo A320neo", AircraftProfile::Default);
        a.schritt(&d.profile, &mut s, None, &mut z);
        assert_eq!(a.aktiv(), None);
        assert_eq!((s.light_beacon, s.autopilot_master), (None, None), "nichts vom vorigen Flugzeug");
        assert!(z.werte().is_empty());
    }
}
