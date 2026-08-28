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
pub mod facility;

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
