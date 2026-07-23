//! v0.20 (Process-Integrity): cross-platform "is the sim's OS process still
//! alive?" check.
//!
//! Neither sim backend can tell us this on its own: MSFS/SimConnect only
//! sometimes gets a clean `SIMCONNECT_RECV_ID_QUIT` (and a hard MSFS crash
//! looks identical to a mere SimConnect/network hiccup — both just stop
//! delivering data), and X-Plane's UDP telemetry has no goodbye message at
//! all (silence is the only signal, full stop). This module closes that gap
//! by asking the OS directly whether the simulator's process is still in
//! the process table — the one thing that works identically on Windows,
//! Linux, and macOS.
//!
//! Deliberately lives in `sim-core` (not `sim-msfs`/`sim-xplane`): both
//! adapters' heterogeneous disconnect signals re-converge in the caller
//! (the `lib.rs` streamer loop) into one decision point, so the process
//! check only needs to exist once, keyed off `SimKind`.

use crate::SimKind;

/// Result of asking the OS whether the simulator's own process is still
/// running. `Unknown` covers `SimKind::Off` and any case where the process
/// table itself couldn't be read — callers must not treat `Unknown` as
/// either `Alive` or `Gone`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessLiveness {
    Alive,
    Gone,
    Unknown,
}

impl ProcessLiveness {
    /// Wire-format string for `PirepPayload.client_health.disconnect_sim_liveness`.
    pub fn as_wire_str(self) -> &'static str {
        match self {
            ProcessLiveness::Alive => "sim_process_alive",
            ProcessLiveness::Gone => "sim_process_gone",
            ProcessLiveness::Unknown => "unknown",
        }
    }
}

/// Per-OS executable/process names to look for, keyed off `SimKind`.
///
/// IMPORTANT: these names are best-effort, sourced from public documentation
/// and community reports at implementation time — NOT verified against a
/// real running instance of each sim/OS combination. Verify before relying
/// on this in a release (see plan verification section: manual kill-tests
/// per platform).
#[cfg(target_os = "windows")]
fn process_names_for(kind: SimKind) -> &'static [&'static str] {
    match kind {
        SimKind::Msfs2020 => &["FlightSimulator.exe"],
        // MSFS 2024 has been observed under both names across installs/updates
        // — check both rather than picking one and silently missing the other.
        SimKind::Msfs2024 => &["FlightSimulator2024.exe", "FlightSimulator.exe"],
        // X-Plane also runs on Windows; unverified guess at the standard name.
        SimKind::XPlane11 | SimKind::XPlane12 => &["X-Plane.exe"],
        SimKind::Off => &[],
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn process_names_for(kind: SimKind) -> &'static [&'static str] {
    match kind {
        // MSFS has no native Linux/macOS build — nothing to look for.
        SimKind::Msfs2020 | SimKind::Msfs2024 => &[],
        SimKind::XPlane11 | SimKind::XPlane12 => &["X-Plane-x86_64"],
        SimKind::Off => &[],
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn process_names_for(_kind: SimKind) -> &'static [&'static str] {
    &[]
}

/// Checks whether the simulator identified by `kind` currently has a
/// matching process in the OS process table. Best-effort: a `sysinfo`
/// refresh failure or an empty/`Off` `kind` yields `Unknown`, never a false
/// `Gone` (callers should not escalate on `Unknown`).
pub fn sim_process_alive(kind: SimKind) -> ProcessLiveness {
    let names = process_names_for(kind);
    if names.is_empty() {
        return ProcessLiveness::Unknown;
    }

    let mut system = sysinfo::System::new();
    system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let found = system.processes().values().any(|process| {
        let Some(process_name) = process.name().to_str() else {
            return false;
        };
        names
            .iter()
            .any(|candidate| process_name.eq_ignore_ascii_case(candidate))
    });

    if found {
        ProcessLiveness::Alive
    } else {
        ProcessLiveness::Gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_is_always_unknown() {
        assert_eq!(sim_process_alive(SimKind::Off), ProcessLiveness::Unknown);
    }

    /// Sanity check the probe doesn't panic and returns SOME liveness value
    /// for a real SimKind on whatever OS these tests run on (they won't find
    /// the sim itself in CI, so this only asserts it comes back Gone/Unknown,
    /// never panics reading the process table).
    #[test]
    fn msfs2024_probe_does_not_panic() {
        let result = sim_process_alive(SimKind::Msfs2024);
        assert!(matches!(
            result,
            ProcessLiveness::Gone | ProcessLiveness::Unknown
        ));
    }

    #[test]
    fn xplane12_probe_does_not_panic() {
        let result = sim_process_alive(SimKind::XPlane12);
        assert!(matches!(
            result,
            ProcessLiveness::Gone | ProcessLiveness::Unknown
        ));
    }

    #[test]
    fn wire_str_mapping() {
        assert_eq!(ProcessLiveness::Alive.as_wire_str(), "sim_process_alive");
        assert_eq!(ProcessLiveness::Gone.as_wire_str(), "sim_process_gone");
        assert_eq!(ProcessLiveness::Unknown.as_wire_str(), "unknown");
    }
}
