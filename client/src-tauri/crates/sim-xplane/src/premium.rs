//! Premium plugin UDP listener.
//!
//! When the optional **AeroACARS X-Plane Plugin** (v0.5.3+) is installed
//! into the X-Plane plugins folder, it streams native flight-loop
//! telemetry to `127.0.0.1:52000`. Two packet types:
//!
//!   * `"telemetry"` — every flight-loop tick (~20 Hz baseline / per-
//!     frame near the ground). Same fields the RREF stream gives us
//!     today, but read at flight-loop frequency without the RREF
//!     subsystem's eviction quirks.
//!
//!   * `"touchdown"` — emitted exactly once per landing at the moment
//!     `fnrml_gear` crosses the touchdown threshold. Carries the peak-
//!     descent VS captured from a 500 ms lookback ring buffer
//!     **inside the plugin**, plus pitch-corrected. This is the value
//!     we want for "landing rate fpm" — frame-perfect, no UDP-
//!     eviction race, no VSI smoothing artifacts.
//!
//! ## Why a separate module / port / format?
//!
//! The RREF protocol on UDP 49000 is X-Plane's native subscription
//! mechanism — every X-Plane install speaks it. We can NEVER drop the
//! RREF path because pilots without the plugin still need a working
//! ACARS. So the plugin runs in parallel: same data, frame-perfect
//! timing, on a different loopback port (52000) using a JSON wire
//! format the plugin owns.
//!
//! ## Why 52000 (not 49001)
//!
//! X-Plane uses ports 49000-49003 for its own built-in UDP I/O —
//! 49000 receives, 49001 is X-Plane's outgoing-data source port.
//! v0.5.0-v0.5.2 mistakenly bound 49001 too, which collided with
//! X-Plane and caused the in-sim error "Fehler bei der Initialisierung
//! des UDP-Netzwerkausgangs (Port 49001). Lokales Netzwerk wird
//! deaktiviert." 52000 is well outside the conflict range.
//!
//! ## Failure modes
//!
//! - Plugin not installed → port 52000 stays silent, `is_active()`
//!   stays `false`, RREF path handles everything. Zero pilot impact.
//! - Plugin sends malformed JSON → we log at `warn`, drop the packet,
//!   keep listening. Cannot affect the rest of the adapter.
//! - Bind failure (port held by another app) → log at `warn`, leave
//!   the listener thread exited. RREF path keeps working.
//!
//! ## Threading
//!
//! Same pattern as the RREF listener: dedicated `std::thread`,
//! `std::net::UdpSocket` with a 200 ms read timeout so it can re-check
//! the shared `stop` flag and exit promptly on adapter shutdown.

use parking_lot::Mutex;
use std::net::UdpSocket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Loopback port the plugin sends to. Hardcoded by the plugin source —
/// `xplane-plugin/src/plugin.cpp::AEROACARS_UDP_PORT`. Changing this
/// requires a coordinated bump on both sides.
///
/// MUST be outside X-Plane's own 49000-49003 range. v0.5.0-v0.5.2
/// used 49001, which collided with X-Plane's outgoing-data source
/// port (in-sim error: "Fehler bei der Initialisierung des
/// UDP-Netzwerkausgangs (Port 49001). Lokales Netzwerk wird
/// deaktiviert."). 52000 is well clear of the conflict zone.
pub const PREMIUM_UDP_PORT: u16 = 52000;

/// How long a plugin packet keeps `is_active()` returning `true`.
/// 3 s comfortably covers a paused-sim hiccup (the plugin stops
/// emitting while paused) without flicking the badge off.
const ACTIVE_TIMEOUT: Duration = Duration::from_secs(3);

/// Maximum size of a single plugin packet in bytes. Plugin caps its
/// stack buffer at 2 KiB; we mirror that.
const RECV_BUF_SIZE: usize = 2048;

// =============================================================================
// Wire types — must match xplane-plugin/src/plugin.cpp JSON format
// =============================================================================
//
// We deliberately do NOT decode the full set of fields — we treat
// extra fields as opt-in for future expansion. `serde(default)` on
// every field means a future plugin version can add new fields
// without breaking older clients.

/// Common envelope for every premium packet. We dispatch on `type`.
#[derive(Debug, Deserialize)]
struct Envelope {
    /// Schema version. Plugin emits `1` today. Any non-1 value gets
    /// dropped at parse time so future incompatible upgrades don't
    /// confuse us.
    #[serde(default)]
    v: u32,
    /// `"telemetry"` or `"touchdown"`. Anything else → dropped.
    #[serde(default, rename = "type")]
    kind: String,
}

/// One-shot landing event from the plugin. Captured at the
/// `fnrml_gear` edge (frame-perfect), with peak descent VS pulled
/// from the plugin's 500 ms lookback ring buffer.
///
/// Field semantics:
///   * `captured_vs_fpm` — pitch-corrected, lookback-peak. NEGATIVE
///     for descent (matches AeroACARS convention).
///   * `captured_g_normal` — vertical-axis g-force at edge.
///   * `captured_pitch_deg`, `captured_bank_deg` — attitude at edge.
///   * `captured_ias_kt`, `captured_gs_kt` — speeds at edge.
///   * `captured_heading_deg` — true heading at edge.
///   * `lat`, `lon` — touchdown position.
///   * `ts` — plugin's sim-time-elapsed seconds at edge. Only
///     useful for diagnostics (gap-detection across the seq
///     counter is more reliable).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PremiumTouchdown {
    #[serde(default)]
    pub captured_vs_fpm: f32,
    #[serde(default)]
    pub captured_g_normal: f32,
    #[serde(default)]
    pub captured_pitch_deg: f32,
    #[serde(default)]
    pub captured_bank_deg: f32,
    #[serde(default)]
    pub captured_ias_kt: f32,
    #[serde(default)]
    pub captured_gs_kt: f32,
    #[serde(default)]
    pub captured_heading_deg: f32,
    #[serde(default)]
    pub lat: f64,
    #[serde(default)]
    pub lon: f64,
    #[serde(default)]
    pub fnrml_gear_n: f32,
    #[serde(default)]
    pub agl_ft: f32,
    #[serde(default)]
    pub ts: f64,
    /// v0.5.11 plugin diagnostic metadata (optional, plugins from
    /// older versions don't send these). Lets the client log which
    /// AGL-window tier the plugin used and how many samples it had.
    #[serde(default)]
    pub captured_vs_source: String,
    #[serde(default)]
    pub captured_vs_window_ms: i64,
    #[serde(default)]
    pub captured_vs_samples: u32,
    /// Wall-clock time we received this packet on the client side.
    /// Useful for the UI layer ("touchdown captured 0.4 s ago").
    /// `None` until set by the listener.
    #[serde(skip)]
    pub received_at: Option<std::time::SystemTime>,
}

/// Public premium status surface for the rest of the app.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct PremiumStatus {
    /// Have we ever received a packet? (Sticky — stays true after
    /// disconnect so UI can show "plugin was here once".)
    pub ever_seen: bool,
    /// Are we receiving packets RIGHT NOW (within `ACTIVE_TIMEOUT`)?
    /// This is the value that drives the "X-PLANE PREMIUM" badge.
    pub active: bool,
    /// Total packets received since adapter start. Diagnostic only.
    pub packet_count: u64,
}

// =============================================================================
// Internal shared state
// =============================================================================

#[derive(Default)]
struct PremiumShared {
    last_packet_at: Mutex<Option<Instant>>,
    ever_seen: AtomicBool,
    packet_count: std::sync::atomic::AtomicU64,
    /// Most recent unconsumed touchdown event, if any. The flight
    /// step loop calls `take_touchdown()` to drain it (one-shot).
    pending_touchdown: Mutex<Option<PremiumTouchdown>>,
    /// Last error that aborted the listener (e.g. bind failure).
    /// `None` while running healthy. Surfaced via the existing
    /// `XPlaneAdapter::last_error()` channel? — no, we keep it
    /// separate to avoid clobbering RREF errors. UI exposes both.
    last_error: Mutex<Option<String>>,
    stop: AtomicBool,
}

/// Embedded inside the X-Plane adapter. Owns its own thread + UDP
/// socket on `PREMIUM_UDP_PORT`. Idempotent `start` / `stop` like
/// the RREF listener — adapter calls them in lock-step.
pub struct PremiumListener {
    shared: Arc<PremiumShared>,
    worker: Option<JoinHandle<()>>,
}

impl Default for PremiumListener {
    fn default() -> Self {
        Self::new()
    }
}

impl PremiumListener {
    pub fn new() -> Self {
        Self {
            shared: Arc::new(PremiumShared::default()),
            worker: None,
        }
    }

    /// Start the listener. Idempotent — calling again restarts.
    /// Returns silently on bind failure (most common cause: another
    /// AeroACARS instance is already bound). The adapter logs the
    /// reason via `tracing` so the pilot can see it in the dev
    /// console; UI surfaces it via `last_error()`.
    pub fn start(&mut self) {
        self.stop();
        // Reset state for a fresh run. We deliberately keep
        // `ever_seen` sticky across stop/start so toggling the
        // adapter doesn't make the UI badge flicker; it only
        // resets to false when the whole process exits.
        *self.shared.last_packet_at.lock() = None;
        *self.shared.pending_touchdown.lock() = None;
        *self.shared.last_error.lock() = None;
        self.shared.stop.store(false, Ordering::SeqCst);
        let shared = Arc::clone(&self.shared);
        let handle = std::thread::Builder::new()
            .name("xplane-premium".into())
            .spawn(move || run_listener(shared))
            .expect("spawn xplane-premium thread");
        self.worker = Some(handle);
        tracing::info!(
            port = PREMIUM_UDP_PORT,
            "X-Plane premium listener started (waiting for plugin packets)"
        );
    }

    pub fn stop(&mut self) {
        if self.worker.is_some() {
            self.shared.stop.store(true, Ordering::SeqCst);
        }
        if let Some(handle) = self.worker.take() {
            // 500 ms is plenty — the recv loop has a 200 ms read
            // timeout, so it'll see the stop flag within one cycle.
            let _ = handle.join();
        }
    }

    pub fn status(&self) -> PremiumStatus {
        let active = match *self.shared.last_packet_at.lock() {
            Some(t) => t.elapsed() < ACTIVE_TIMEOUT,
            None => false,
        };
        PremiumStatus {
            ever_seen: self.shared.ever_seen.load(Ordering::Relaxed),
            active,
            packet_count: self.shared.packet_count.load(Ordering::Relaxed),
        }
    }

    /// Drain the pending touchdown event (one-shot semantics).
    /// Called from the flight sampler after each tick. Returns
    /// `None` until the next plugin-emitted touchdown.
    pub fn take_touchdown(&self) -> Option<PremiumTouchdown> {
        self.shared.pending_touchdown.lock().take()
    }

    pub fn last_error(&self) -> Option<String> {
        self.shared.last_error.lock().clone()
    }
}

impl Drop for PremiumListener {
    fn drop(&mut self) {
        self.stop();
    }
}

// =============================================================================
// Listener thread
// =============================================================================

fn run_listener(shared: Arc<PremiumShared>) {
    // Bind to loopback only — the plugin sends to 127.0.0.1:52000 and
    // we want to refuse traffic from any other interface for security
    // (plugin packets contain telemetry that's no business of the LAN).
    let bind_addr = format!("127.0.0.1:{PREMIUM_UDP_PORT}");
    let socket = match UdpSocket::bind(&bind_addr) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!(
                "premium listener bind failed on {bind_addr}: {e} \
                 (plugin telemetry will not be received this session)"
            );
            tracing::warn!("{msg}");
            *shared.last_error.lock() = Some(msg);
            return;
        }
    };
    if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(200))) {
        tracing::warn!(error = %e, "premium socket: set_read_timeout failed");
    }

    let mut buf = vec![0u8; RECV_BUF_SIZE];
    while !shared.stop.load(Ordering::SeqCst) {
        match socket.recv_from(&mut buf) {
            Ok((n, peer)) => {
                // Loopback-only sanity check — if a packet somehow
                // arrived from a non-loopback address we drop it.
                // (We bound to 127.0.0.1 so this should be impossible,
                // but defensive coding is cheap.)
                if !peer.ip().is_loopback() {
                    tracing::warn!(?peer, "premium: dropped non-loopback packet");
                    continue;
                }
                handle_packet(&buf[..n], &shared);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // Idle tick — re-check stop flag and loop.
            }
            Err(e) => {
                // Unexpected — log + back off briefly so we don't
                // hot-loop on a permanent error condition.
                tracing::warn!(error = %e, "premium UDP recv error");
                std::thread::sleep(Duration::from_millis(250));
            }
        }
    }
    tracing::info!("X-Plane premium listener stopped");
}

/// Parse one UDP datagram. The plugin sends one JSON object per
/// datagram (no batching), terminated with `\n`. We're tolerant of
/// trailing whitespace and accept either `\n` or no terminator.
fn handle_packet(bytes: &[u8], shared: &Arc<PremiumShared>) {
    // Trim trailing newline / whitespace so serde_json sees a
    // clean object. `from_slice` itself tolerates trailing space
    // but we keep it explicit for clarity.
    let trimmed = trim_trailing_ws(bytes);
    if trimmed.is_empty() {
        return;
    }

    // Two-phase decode: first the envelope so we know `kind`, then
    // the typed body when it's a touchdown. Telemetry packets are
    // dropped here — we just use them to update the activity
    // heartbeat. (The RREF stream already gives us telemetry.)
    let env: Envelope = match serde_json::from_slice(trimmed) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, "premium: malformed JSON envelope");
            return;
        }
    };

    if env.v != 1 {
        tracing::warn!(
            v = env.v,
            "premium: unsupported schema version (expected 1) — dropping packet. \
             AeroACARS client may need an update."
        );
        return;
    }

    // ---- Heartbeat (any valid packet counts) ----
    *shared.last_packet_at.lock() = Some(Instant::now());
    shared.ever_seen.store(true, Ordering::Relaxed);
    shared.packet_count.fetch_add(1, Ordering::Relaxed);

    match env.kind.as_str() {
        "telemetry" => {
            // Heartbeat done above. Body fields ignored — the RREF
            // stream is authoritative for live telemetry. Future:
            // overlay vs_fpm, g_normal etc. for the live ribbon.
        }
        "touchdown" => match serde_json::from_slice::<PremiumTouchdown>(trimmed) {
            Ok(mut td) => {
                td.received_at = Some(std::time::SystemTime::now());
                tracing::info!(
                    vs_fpm = td.captured_vs_fpm,
                    g = td.captured_g_normal,
                    ias_kt = td.captured_ias_kt,
                    pitch_deg = td.captured_pitch_deg,
                    "premium: touchdown event received from plugin"
                );
                *shared.pending_touchdown.lock() = Some(td);
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "premium: touchdown packet decode failed (heartbeat still counted)"
                );
            }
        },
        other => {
            tracing::debug!(
                kind = %other,
                "premium: unknown packet type (forward-compat ignore)"
            );
        }
    }
}

fn trim_trailing_ws(bytes: &[u8]) -> &[u8] {
    let mut end = bytes.len();
    while end > 0 && matches!(bytes[end - 1], b'\n' | b'\r' | b' ' | b'\t') {
        end -= 1;
    }
    &bytes[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_touchdown_packet() {
        let bytes = br#"{"v":1,"type":"touchdown","seq":42,"ts":1234.5,"lat":50.03,"lon":8.57,"captured_vs_fpm":-285.4,"captured_g_normal":1.18,"captured_pitch_deg":3.4,"captured_bank_deg":0.2,"captured_ias_kt":138.0,"captured_gs_kt":134.5,"captured_heading_deg":253.1,"fnrml_gear_n":52312.0,"agl_ft":0.4}"#;
        let trimmed = trim_trailing_ws(bytes);
        let env: Envelope = serde_json::from_slice(trimmed).unwrap();
        assert_eq!(env.v, 1);
        assert_eq!(env.kind, "touchdown");
        let td: PremiumTouchdown = serde_json::from_slice(trimmed).unwrap();
        assert!((td.captured_vs_fpm - -285.4).abs() < 0.01);
        assert!((td.captured_g_normal - 1.18).abs() < 0.001);
        assert_eq!(td.lat, 50.03);
    }

    #[test]
    fn drops_unknown_schema_version() {
        let shared = Arc::new(PremiumShared::default());
        // v: 99 is unsupported; envelope decodes but heartbeat
        // shouldn't tick.
        handle_packet(br#"{"v":99,"type":"telemetry"}"#, &shared);
        assert_eq!(shared.packet_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn telemetry_packet_increments_heartbeat_only() {
        let shared = Arc::new(PremiumShared::default());
        // Note: real plugin packets are terminated with a literal
        // newline byte. We include it here to verify the trim path,
        // hence the regular byte string + `\n` escape (raw byte
        // strings would treat `\n` as two characters).
        handle_packet(
            b"{\"v\":1,\"type\":\"telemetry\",\"seq\":1,\"vs_fpm\":-450.0}\n",
            &shared,
        );
        assert_eq!(shared.packet_count.load(Ordering::Relaxed), 1);
        assert!(shared.pending_touchdown.lock().is_none());
    }

    #[test]
    fn touchdown_packet_is_captured_and_drained_once() {
        let shared = Arc::new(PremiumShared::default());
        handle_packet(
            br#"{"v":1,"type":"touchdown","captured_vs_fpm":-300.0,"captured_g_normal":1.2}"#,
            &shared,
        );
        // First pickup returns Some, second returns None.
        let td = shared.pending_touchdown.lock().take();
        assert!(td.is_some());
        assert_eq!(td.unwrap().captured_vs_fpm as i32, -300);
        let td2 = shared.pending_touchdown.lock().take();
        assert!(td2.is_none());
    }

    #[test]
    fn malformed_json_is_dropped_silently() {
        let shared = Arc::new(PremiumShared::default());
        handle_packet(b"not json {{", &shared);
        assert_eq!(shared.packet_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn trims_trailing_whitespace_and_newlines() {
        assert_eq!(trim_trailing_ws(b"abc\n"), b"abc");
        assert_eq!(trim_trailing_ws(b"abc\r\n"), b"abc");
        assert_eq!(trim_trailing_ws(b"abc \t \n"), b"abc");
        assert_eq!(trim_trailing_ws(b"abc"), b"abc");
        assert_eq!(trim_trailing_ws(b""), b"");
    }
}
