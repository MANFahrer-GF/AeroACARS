//! AeroACARS-Schalterprüfung — geführtes Konsolenwerkzeug: Welche MSFS-LVar
//! gehört zu welchem Cockpitschalter?
//!
//! Aufbau:
//! * `skripte`    — Prüfskripte je Flugzeug + Auswahl per Titel
//! * `auswertung` — Ruhe-/Rauschfilter, Kandidaten, Rangfolge
//! * `mobiflight` — Protokoll des MobiFlight-WASM-Moduls (Kodieren/Deuten)
//! * `input_events` — SimConnect-Input-Events (B:), Deutung der Rohdaten
//! * `bericht`    — Live-Protokoll (JSONL) + Endbericht (JSON/TXT)
//! * `ablauf`     — geführter Dialog (Ruhemessung, Skript, freier Modus)
//! * `sim`        — SimConnect (nur Windows)
//! * `start`      — Programmstart, Fehlermeldungen, Strg+C (nur Windows)
//!
//! Alles außer `sim`/`start` ist plattformunabhängig und wird auf dem Mac
//! getestet; die Windows-Exe baut die CI.

#![cfg_attr(not(windows), allow(dead_code))]

mod ablauf;
mod auswertung;
mod bericht;
mod input_events;
mod mobiflight;
mod skripte;

#[cfg(windows)]
mod sim;
#[cfg(windows)]
mod start;

fn main() {
    #[cfg(windows)]
    {
        std::process::exit(start::run());
    }
    #[cfg(not(windows))]
    {
        eprintln!("Die Schalterprüfung läuft nur unter Windows (MSFS 2020/2024).");
        std::process::exit(1);
    }
}
