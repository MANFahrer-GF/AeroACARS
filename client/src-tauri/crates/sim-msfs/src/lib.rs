//! MSFS 2020 / MSFS 2024 simulator adapter — **raw SimConnect FFI**.
//!
//! See ADR-0002 in `docs/decisions/0002-msfs-simconnect-only.md`.
//!
//! Reference docs:
//! <https://docs.flightsimulator.com/msfs2024/html/6_Programming_APIs/SimConnect/SimConnect_SDK.htm>
//!
//! # Why raw FFI
//!
//! We previously used the third-party `simconnect-sdk` crate (archived
//! 2026-02-22). Its `SimConnectObject` derive macro builds one big
//! `#[repr(C, packed)]` struct from your fields and registers them
//! with `SimConnect_AddToDataDefinition` in field order. The fatal
//! quirk: when SimConnect rejects a single SimVar (because the
//! aircraft doesn't define it, or the SimVar moved/got renamed),
//! the rejection is delivered **asynchronously** via
//! `SIMCONNECT_RECV_EXCEPTION`. The crate does not surface those
//! exceptions, so the local struct still expects N fields but the
//! sim only sends N-1 — and every subsequent read shifts up,
//! producing memory-aligned garbage (we observed this with
//! `PLANE TOUCHDOWN *`, `FUELSYSTEM TANK WEIGHT:N` and FBW LVars).
//!
//! In raw FFI we:
//!   * drive `SimConnect_AddToDataDefinition` per SimVar with explicit
//!     HRESULT checks;
//!   * surface `SIMCONNECT_RECV_EXCEPTION` to the tracing log so the
//!     pilot/dev can see *which* SimVar got rejected;
//!   * parse the data block byte-by-byte at fixed offsets so a
//!     dropped SimVar can never corrupt another field.
//!
//! Status: minimal port (Phase L). Position, attitude, speeds,
//! fuel & weight (with EX1 SimVars), aircraft identity, on-ground.
//! Lights / AP / addon-specific LVars get re-added on top of this
//! foundation incrementally.

#![allow(dead_code)]

#[cfg(target_os = "windows")]
mod adapter;
/// MSFS-2024-Input-Events (B:-Variablen): Deutung + Ablauf, plattformunabhaengig.
pub mod eingabe_events;
pub mod facility;
/// Frist fuer einen Handle ohne Lebenszeichen — plattformunabhaengig.
pub(crate) mod handle_frist;
/// „Flugzeug vermessen": Messkanal fuer L:/A:-Variablen, plattformunabhaengig.
pub mod vermessung;
/// Zusatzwerte fuer den Telemetrie-Monitor — Logik plattformunabhaengig.
pub mod zusatz;
/// v3 Etappe 2: Paketprofile fuer neue Add-ons — plattformunabhaengig.
pub mod paketprofil;

#[cfg(target_os = "windows")]
pub use adapter::*;

/// PMDG SimConnect SDK integration (737 NG3 + 777X).
///
/// Cross-platform module — the data structures + variant detection
/// don't depend on Windows. The actual ClientData subscription
/// will be Windows-only when wired into the adapter (Phase 5.2);
/// for now this just defines the layouts so other crates can
/// reference `PmdgVariant` etc. on any platform.
pub mod pmdg;

// ---- Non-Windows stub ----

#[cfg(not(target_os = "windows"))]
mod stub {
    use serde::Serialize;
    use sim_core::{SimKind, SimSnapshot};

    #[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
    #[serde(rename_all = "snake_case")]
    pub enum ConnectionState {
        Disconnected,
        Connecting,
        Connected,
    }

    pub struct MsfsAdapter;

    /// Stubbed PMDG status for non-Windows targets — never has a
    /// variant detected, never subscribed, never receives data.
    #[derive(Debug, Clone)]
    pub struct PmdgStatus {
        pub variant: Option<crate::pmdg::PmdgVariant>,
        pub subscribed: bool,
        pub ever_received: bool,
        pub stale_secs: u64,
    }

    impl PmdgStatus {
        pub fn looks_like_sdk_disabled(&self) -> bool {
            false
        }
    }

    impl MsfsAdapter {
        pub fn new() -> Self {
            Self
        }
        pub fn start(&mut self, _kind: SimKind) {}
        pub fn stop(&mut self) {}
        pub fn state(&self) -> ConnectionState {
            ConnectionState::Disconnected
        }
        pub fn snapshot(&self) -> Option<SimSnapshot> {
            None
        }
        pub fn clear_snapshot(&self) {}
        pub fn pmdg_ng3_snapshot(&self) -> Option<crate::pmdg::ng3::Pmdg738Snapshot> {
            None
        }
        pub fn pmdg_x777_snapshot(&self) -> Option<crate::pmdg::x777::Pmdg777XSnapshot> {
            None
        }
        pub fn pmdg_status(&self) -> PmdgStatus {
            PmdgStatus {
                variant: None,
                subscribed: false,
                ever_received: false,
                stale_secs: u64::MAX,
            }
        }
        pub fn last_error(&self) -> Option<String> {
            Some("MSFS adapter is Windows-only".into())
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub use stub::*;

/// Quelltext-Pruefung fuer `adapter.rs` (auf macOS/Linux nicht uebersetzt,
/// Muster wie in `facility.rs`).
#[cfg(test)]
mod optionale_abfrage_tests {
    const ADAPTER: &str = include_str!("adapter.rs");

    fn nur_code(s: &str) -> String {
        s.lines()
            .filter(|z| !z.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect()
    }

    /// Morgenbericht 19.11.2026: Unter MSFS 2020 lehnt der Simulator die
    /// optionale SimVar `FUEL TOTAL QUANTITY WEIGHT EX1` ab (send_id 430), die
    /// folgende Abfrage auf die leere Definition endet in UNRECOGNIZED_ID
    /// (send_id 431) und stand als WARN ohne Feld im Log und als „neues
    /// Fehlermuster“ im Health-Report. Die Abfrage merkt ihre Kennung, und nur
    /// UNRECOGNIZED_ID darauf gilt als erwartet (Codex: jede andere Ausnahme
    /// der Abfrage bleibt eine Warnung).
    #[test]
    fn die_abfrage_einer_optionalen_simvar_merkt_ihre_kennung() {
        let start = ADAPTER.find("fn register_optionale(").expect("register_optionale vorhanden");
        let ende = ADAPTER[start + 1..].find("\n    fn ").map_or(ADAPTER.len(), |i| start + 1 + i);
        let rumpf = nur_code(&ADAPTER[start..ende]);
        let abfrage = rumpf.find("SimConnect_RequestDataOnSimObject(").expect("Abfrage im Rumpf");
        let nach = &rumpf[abfrage..];
        let kennung = nach.find("SimConnect_GetLastSentPacketID(").expect("Kennung nach der Abfrage geholt");
        assert!(nach[kennung..].contains("self.optionale_abfrage_ids.push("), "Kennung der Abfrage gemerkt");
    }

    #[test]
    fn nur_unrecognized_id_auf_die_abfrage_wird_herabgestuft() {
        let code = nur_code(ADAPTER);
        let bedingung = "ifname==\"UNRECOGNIZED_ID\"&&conn.optionale_abfrage_ids.contains(&send_id){";
        let stelle = code.find(bedingung).expect("Herabstufung nur fuer UNRECOGNIZED_ID auf eine Abfragekennung");
        let block = &code[stelle..stelle + 200];
        assert!(block.contains("tracing::info!(") && block.contains("continue;"), "als INFO und ohne Warnung");
        assert_eq!(code.matches("optionale_abfrage_ids.contains(").count(), 1, "keine zweite, weitere Herabstufung");
    }
}
