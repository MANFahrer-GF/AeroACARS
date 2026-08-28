//! X-Plane 11 / X-Plane 12 simulator adapter.
//!
//! See ADR-0004 (revision: native UDP RREF, no plugin). This crate
//! talks to X-Plane via the built-in UDP DataRef protocol — same as
//! every other XPlane-aware ACARS / GA tool out there. No XPLM plugin
//! to install, no third-party gateway. Pilot just runs X-Plane and
//! AeroACARS picks it up from `localhost:49000`.
//!
//! ## Protocol summary
//!
//! - We bind a local UDP socket on any free port.
//! - We send RREF subscription packets to `127.0.0.1:49000` (X-Plane's
//!   listen port). Each request says "send DataRef #N at K Hz, with
//!   index I in your responses".
//! - X-Plane streams response packets back to the source port of our
//!   subscription, each containing one or more `(index, float32)` pairs.
//! - We translate each index back to its DataRef name → field on the
//!   shared `SimSnapshot`.
//!
//! Reference docs:
//!   * <https://xppython3.readthedocs.io/en/latest/development/udp/rref.html>
//!   * <https://questions.x-plane.com/6880/where-can-find-the-complete-protocol-specification-dataref>
//!   * <http://www.nuclearprojects.com/xplane/xplaneref.html>
//!   * <https://forums.x-plane.org/forums/topic/110870-x-plane-11-udp-interface-rref/>
//!
//! ## Why all-floats
//!
//! RREF returns every DataRef as a `float32`, even for booleans or
//! integers. We cast at the snapshot-mapping boundary so downstream
//! code keeps using the same `SimSnapshot` types as the MSFS path.
//!
//! ## Status
//!
//! Phase 1: foundation — UDP bind, RREF encode/decode, the most
//! important DataRefs (position, attitude, V/S, G, on-ground, fuel,
//! gear, flaps), snapshot mapping. Connection state machine. Async
//! listener task. Compiles cross-platform.
//!
//! Phase 2 (future): full DataRef coverage (lights, AP, body velocity
//! for sideslip, wind components for head/crosswind), aircraft-
//! identity probe, version detection.

#![allow(dead_code)]

mod adapter;
mod dataref;
mod premium;
mod probe;
mod profile;
mod rref;
pub mod szenerie;
mod web_api;

pub use adapter::{ConnectionState, XPlaneAdapter};
pub use premium::{PremiumStatus, PremiumTouchdown, PREMIUM_UDP_PORT};
pub use probe::is_xplane_running;

/// Default UDP port X-Plane listens on. Subscriptions get sent here.
pub const XPLANE_LISTEN_PORT: u16 = 49000;

/// Default subscription rate (Hz). Matches our 50 Hz touchdown
/// sampling target. X-Plane caps the actual rate at the sim's
/// rendered framerate, so on a slow PC we'll naturally fall to
/// 30-40 Hz without protocol-level intervention.
pub const SUBSCRIPTION_HZ: u32 = 50;
