//! Pure protocol/data layer for the Hoppie ACARS PDC/CPDLC feature
//! (v1.3.0, #Hoppie-PDC-CPDLC — see docs/spec/v1.3.0-hoppie-pdc-cpdlc.md).
//!
//! No `tauri`/`reqwest` dependency — fully unit-testable without a
//! runtime. Wiring (HTTP client, Tauri commands, background poller,
//! settings/secrets persistence) lives in `client/src-tauri/src/hoppie/`
//! (the app crate), which depends on this crate.
//!
//! ## Layout
//!
//! - [`wire`] — `connect.html` GET request/response codec (plain text,
//!   no HTTP types).
//! - [`cpdlc`] — `/data2/{MIN}/{MRN}/{RESPONSE}/{ELEMENT}` frame codec.
//! - [`elements`] — generic, data-driven GOLD element template
//!   fill/match engine.
//! - [`elements_data`] — the actual UM (uplink) / DM (downlink) element
//!   table. Phase 1 ships ~10 placeholder rows just wide enough to
//!   exercise the codec end-to-end; the full ~300-row GOLD library
//!   lands in Phase 4 as its own isolated diff.
//! - [`thread`] — MIN/MRN threading state machine for one CPDLC
//!   connection.
//! - [`pdc`] — PDC-as-telex request/reply formatting (there is no
//!   dedicated PDC wire type in the Hoppie protocol).

pub mod cpdlc;
pub mod elements;
pub mod elements_data;
pub mod pdc;
pub mod thread;
pub mod wire;
