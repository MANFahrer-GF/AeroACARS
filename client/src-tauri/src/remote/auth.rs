//! Auth + rate-limiting for the LAN remote-control server.
//!
//! - A random **6-digit PIN** is shown on the desktop (settings panel +
//!   QR). The tablet posts it to `POST /api/auth`; on a constant-time
//!   match it gets a long-lived **256-bit bearer token** (hex). Every
//!   `/api/cmd/*` + `/ws` request carries that token.
//! - The token is **persisted** via the secrets store so a paired tablet
//!   keeps working across desktop restarts without re-pairing. The PIN is
//!   regenerated each process start (it only has to live long enough to
//!   pair) — but is stable for the lifetime of the running server.
//! - `POST /api/auth` is **rate-limited** (~5 attempts/min, then a short
//!   lockout) to make the 6-digit space impractical to brute-force.
//!
//! Both the PIN compare and the token compare use [`subtle`] so a remote
//! attacker cannot recover either via response-timing.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use subtle::ConstantTimeEq;

/// Max failed PIN attempts inside [`RATE_WINDOW`] before a *per-IP* lockout.
const MAX_ATTEMPTS: u32 = 5;
/// Sliding window the attempt counter is measured over.
const RATE_WINDOW: Duration = Duration::from_secs(60);
/// How long a single peer IP stays locked after the limit is hit.
const LOCKOUT: Duration = Duration::from_secs(60);
/// Global backstop: total failed PIN attempts across **all** peer IPs
/// before the PIN is rotated. A determined attacker spoofing many source
/// IPs cannot make per-IP lockouts bite, so this resets the secret itself
/// — the brute-force progress is thrown away. The already-paired tablet
/// is unaffected (it holds a bearer token, not the PIN); only a *new*
/// pairing needs the rotated PIN, which the desktop Settings panel
/// (polling `remote_server_status`) shows.
pub(crate) const GLOBAL_ROTATE_THRESHOLD: u32 = 50;

/// Hard cap on the number of per-IP rate-limiter entries tracked at once.
/// The per-IP map is GC'd only lazily (on a repeat hit of the *same* IP), so
/// a flood of distinct one-shot (spoofed) source IPs would otherwise leave
/// never-pruned entries — growth bounded only by the address space, not by
/// code. This cap bounds it in code: at the cap we first drain empty/expired
/// entries, and if still full we refuse to track a *new* IP. Refusing to
/// track does NOT open a bypass — the global failure counter and the
/// rotation backstop still see and act on those failures, so the
/// brute-force protection is unchanged; it merely stops unbounded growth.
const MAX_TRACKED_IPS: usize = 4096;

/// Shared, process-lifetime auth state. One instance lives behind an
/// `Arc` in [`crate::remote::RemoteContext`].
pub struct AuthState {
    /// 6-digit pairing PIN, e.g. `"048213"`. Regenerated on the global
    /// backstop rotation, so it is interior-mutable behind a `Mutex`.
    pin: Mutex<String>,
    /// 64-char hex bearer token. Persisted across restarts.
    token: String,
    /// Per-peer-IP PIN-attempt rate limiters + a global failure counter.
    /// Interior-mutable; handlers hold `&self`.
    limiter: Mutex<LimiterTable>,
}

impl AuthState {
    /// Resolve the shared auth state: load (or first-time generate +
    /// persist) the bearer token from the secrets store under
    /// `token_account`, and generate a fresh PIN for this run.
    ///
    /// A secrets-store failure (extremely unlikely) is non-fatal: we fall
    /// back to an in-memory token for this process so the feature still
    /// works for the current session — it just won't survive a restart.
    pub fn load_or_init(token_account: &str) -> std::sync::Arc<Self> {
        let token = match secrets::load_api_key(token_account) {
            Ok(Some(t)) if t.len() == 64 => t,
            _ => {
                let fresh = gen_token_hex();
                if let Err(e) = secrets::store_api_key(token_account, &fresh) {
                    tracing::warn!(error = %e, "remote: failed to persist bearer token — using in-memory token");
                }
                fresh
            }
        };
        std::sync::Arc::new(Self {
            pin: Mutex::new(gen_pin()),
            token,
            limiter: Mutex::new(LimiterTable::default()),
        })
    }

    /// Construct directly from explicit values — test helper only.
    #[cfg(test)]
    pub fn for_test(pin: &str, token: &str) -> Self {
        Self {
            pin: Mutex::new(pin.to_string()),
            token: token.to_string(),
            limiter: Mutex::new(LimiterTable::default()),
        }
    }

    /// The current pairing PIN (shown on the desktop / embedded in the QR).
    /// Returns an owned `String` because the PIN can be rotated by the
    /// global backstop.
    pub fn pin(&self) -> String {
        self.pin.lock().expect("auth pin poisoned").clone()
    }

    /// Constant-time check that `candidate` equals the bearer token.
    /// Used on every `/api/cmd/*` + `/ws` request.
    pub fn verify_token(&self, candidate: &str) -> bool {
        ct_eq(candidate.as_bytes(), self.token.as_bytes())
    }

    /// Attempt to exchange a PIN for the token, keyed by the requesting
    /// peer's IP. Returns:
    /// - `Ok(token)` on a correct PIN,
    /// - `Err(AuthError::BadPin)` on a wrong PIN (counts toward this IP's
    ///   lockout *and* the global rotation backstop),
    /// - `Err(AuthError::RateLimited)` while THIS IP is locked out.
    ///
    /// The rate-limit check is **per peer IP** so one hostile LAN device
    /// cannot lock out a legitimate tablet pairing from a different IP
    /// (the previous global limiter was a trivial DoS). The per-IP
    /// lockout is checked BEFORE the compare so a locked-out client cannot
    /// keep probing the (constant-time) compare path.
    pub fn try_pin(&self, peer_ip: IpAddr, candidate: &str) -> Result<String, AuthError> {
        let now = Instant::now();
        {
            let mut table = self.limiter.lock().expect("auth limiter poisoned");
            if table.is_locked(peer_ip, now) {
                return Err(AuthError::RateLimited);
            }
        }

        // Snapshot the current PIN under its own lock (it may rotate).
        let current_pin = self.pin();
        if ct_eq(candidate.as_bytes(), current_pin.as_bytes()) {
            // Success resets this IP's limiter so a later re-pair from the
            // same device isn't blocked. The global counter is left intact
            // (a legit success shouldn't erase a separate attacker's tally).
            self.limiter
                .lock()
                .expect("auth limiter poisoned")
                .reset_ip(peer_ip);
            Ok(self.token.clone())
        } else {
            let rotate = {
                let mut table = self.limiter.lock().expect("auth limiter poisoned");
                table.record_failure(peer_ip, now)
            };
            // Global backstop: after enough total failures across all IPs,
            // rotate the PIN so a spoofing attacker's progress is reset.
            if rotate {
                self.rotate_pin();
            }
            Err(AuthError::BadPin)
        }
    }

    /// Regenerate the pairing PIN (global-backstop rotation) and reset the
    /// global failure tally so the next window starts fresh. Per-IP
    /// lockouts persist (they expire on their own). The persisted bearer
    /// token is NOT touched, so a paired tablet keeps working.
    fn rotate_pin(&self) {
        let fresh = gen_pin();
        *self.pin.lock().expect("auth pin poisoned") = fresh;
        self.limiter
            .lock()
            .expect("auth limiter poisoned")
            .reset_global();
        tracing::warn!(
            "remote: global PIN-failure backstop hit ({GLOBAL_ROTATE_THRESHOLD}) — \
             pairing PIN rotated; already-paired devices are unaffected"
        );
    }
}

/// Outcome of a failed [`AuthState::try_pin`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    /// Wrong PIN — counts toward the lockout.
    BadPin,
    /// Too many recent failures — endpoint is temporarily locked.
    RateLimited,
}

/// Per-peer-IP rate limiters + a global failure tally for the rotation
/// backstop. Keying by IP makes a single hostile device unable to lock
/// out pairing from other devices; the global tally catches an attacker
/// who spoofs many source IPs to dodge the per-IP lockouts.
#[derive(Default)]
struct LimiterTable {
    /// One sliding-window limiter per peer IP. Pruned lazily: a limiter is
    /// dropped once it is neither locked nor holding any in-window failures —
    /// but only when that IP is touched again. Lazy pruning alone does NOT
    /// bound the map (a flood of distinct one-shot IPs would never be
    /// touched again), so [`Self::record_failure`] additionally enforces a
    /// hard [`MAX_TRACKED_IPS`] cap: it sweeps empty entries at the cap and
    /// refuses to insert a new IP if still full.
    per_ip: HashMap<IpAddr, RateLimiter>,
    /// Total wrong-PIN attempts across all IPs since the last rotation.
    global_failures: u32,
}

impl LimiterTable {
    /// Is THIS peer IP currently locked out? Side-effect: expires a stale
    /// lockout and garbage-collects a now-empty per-IP entry.
    fn is_locked(&mut self, ip: IpAddr, now: Instant) -> bool {
        let Some(limiter) = self.per_ip.get_mut(&ip) else {
            return false;
        };
        let locked = limiter.is_locked(now);
        if !locked && limiter.is_empty() {
            self.per_ip.remove(&ip);
        }
        locked
    }

    /// Record a wrong-PIN attempt for `ip` and bump the global tally.
    /// Returns `true` when the global rotation threshold is reached (the
    /// caller then rotates the PIN, which calls [`Self::reset_global`]).
    ///
    /// The global tally is ALWAYS bumped — even if we decline to track this
    /// IP's per-IP limiter (see below) — so the rotation backstop still
    /// catches a many-IP brute-force.
    ///
    /// Per-IP tracking is hard-bounded by [`MAX_TRACKED_IPS`]: when the map
    /// is at the cap and this is a *new* IP, we first sweep entries that hold
    /// no live state (expired/empty), and if the map is STILL full we skip
    /// inserting the new IP. Skipping only forfeits this IP's own per-IP
    /// lockout — not the global protection — so it is not a bypass.
    fn record_failure(&mut self, ip: IpAddr, now: Instant) -> bool {
        self.global_failures = self.global_failures.saturating_add(1);

        match self.per_ip.get_mut(&ip) {
            // Already tracked → just record (no growth).
            Some(limiter) => limiter.record_failure(now),
            None => {
                if self.per_ip.len() >= MAX_TRACKED_IPS {
                    // At the cap: drain entries holding no live state.
                    self.per_ip.retain(|_, l| !l.is_empty());
                }
                if self.per_ip.len() < MAX_TRACKED_IPS {
                    self.per_ip.entry(ip).or_default().record_failure(now);
                }
                // Else: still full of live (locked/in-window) entries →
                // refuse to track this new IP. The global tally above still
                // counted the attempt.
            }
        }

        self.global_failures >= GLOBAL_ROTATE_THRESHOLD
    }

    /// Clear the limiter for one IP (called on that IP's successful pair).
    fn reset_ip(&mut self, ip: IpAddr) {
        self.per_ip.remove(&ip);
    }

    /// Reset the global failure tally (called right after a PIN rotation).
    fn reset_global(&mut self) {
        self.global_failures = 0;
    }
}

/// Sliding-window failed-attempt counter with a fixed lockout (one per IP).
#[derive(Default)]
struct RateLimiter {
    /// Timestamps of recent failures (pruned to [`RATE_WINDOW`]).
    failures: Vec<Instant>,
    /// `Some(until)` while locked out.
    locked_until: Option<Instant>,
}

impl RateLimiter {
    fn is_locked(&mut self, now: Instant) -> bool {
        match self.locked_until {
            Some(until) if now < until => true,
            Some(_) => {
                // Lockout expired — clear it and the failure history so
                // the client gets a fresh window.
                self.locked_until = None;
                self.failures.clear();
                false
            }
            None => false,
        }
    }

    fn record_failure(&mut self, now: Instant) {
        self.failures
            .retain(|t| now.duration_since(*t) < RATE_WINDOW);
        self.failures.push(now);
        if self.failures.len() as u32 >= MAX_ATTEMPTS {
            self.locked_until = Some(now + LOCKOUT);
        }
    }

    /// Whether this limiter holds no live state (safe to garbage-collect).
    fn is_empty(&self) -> bool {
        self.failures.is_empty() && self.locked_until.is_none()
    }
}

/// Constant-time byte-slice equality. `subtle`'s `ct_eq` requires equal
/// lengths to be meaningful, so we early-out on a length mismatch (the
/// length itself is not a secret) and otherwise compare in constant time.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

/// 32 bytes of OS entropy → a 64-char lowercase hex bearer token.
fn gen_token_hex() -> String {
    let mut buf = [0u8; 32];
    fill_random(&mut buf);
    let mut s = String::with_capacity(64);
    for b in buf {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap());
        s.push(char::from_digit((b & 0x0f) as u32, 16).unwrap());
    }
    s
}

/// A uniformly-distributed 6-digit PIN (`"000000"`..="999999") from OS
/// entropy. Uses rejection sampling on a u32 so the modulo is unbiased.
fn gen_pin() -> String {
    let n = uniform_below(1_000_000);
    format!("{n:06}")
}

/// Uniform `u32` in `0..bound` via rejection sampling (no modulo bias).
fn uniform_below(bound: u32) -> u32 {
    debug_assert!(bound > 0);
    // Largest multiple of `bound` that fits in u32; reject above it.
    let zone = u32::MAX - (u32::MAX % bound);
    loop {
        let mut b = [0u8; 4];
        fill_random(&mut b);
        let v = u32::from_le_bytes(b);
        if v < zone {
            return v % bound;
        }
    }
}

/// Fill `buf` with OS entropy. A `getrandom` failure here would mean the
/// OS RNG is unavailable, which is catastrophic for a security-gated
/// feature — panic rather than silently emit a predictable PIN/token.
fn fill_random(buf: &mut [u8]) {
    getrandom::getrandom(buf).expect("OS entropy source (getrandom) unavailable");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two distinct LAN peer IPs for the per-IP isolation tests.
    fn ip_a() -> IpAddr {
        "192.168.1.10".parse().unwrap()
    }
    fn ip_b() -> IpAddr {
        "192.168.1.20".parse().unwrap()
    }

    #[test]
    fn correct_token_verifies_wrong_does_not() {
        let auth = AuthState::for_test("123456", &"a".repeat(64));
        assert!(auth.verify_token(&"a".repeat(64)));
        assert!(!auth.verify_token(&"b".repeat(64)));
        // Length mismatch is rejected without panicking.
        assert!(!auth.verify_token("short"));
        assert!(!auth.verify_token(""));
    }

    #[test]
    fn correct_pin_returns_token() {
        let auth = AuthState::for_test("424242", "tok");
        assert_eq!(auth.try_pin(ip_a(), "424242"), Ok("tok".to_string()));
    }

    #[test]
    fn wrong_pin_is_bad_pin() {
        let auth = AuthState::for_test("424242", "tok");
        assert_eq!(auth.try_pin(ip_a(), "000000"), Err(AuthError::BadPin));
    }

    #[test]
    fn locks_out_after_max_attempts() {
        let auth = AuthState::for_test("424242", "tok");
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(auth.try_pin(ip_a(), "999999"), Err(AuthError::BadPin));
        }
        // Next attempt from the SAME IP — even with the CORRECT pin — is
        // rate-limited.
        assert_eq!(auth.try_pin(ip_a(), "424242"), Err(AuthError::RateLimited));
    }

    #[test]
    fn lockout_is_per_ip_isolated() {
        // A hostile device on IP A exhausts its attempts and is locked.
        let auth = AuthState::for_test("424242", "tok");
        for _ in 0..MAX_ATTEMPTS {
            assert_eq!(auth.try_pin(ip_a(), "000000"), Err(AuthError::BadPin));
        }
        assert_eq!(auth.try_pin(ip_a(), "424242"), Err(AuthError::RateLimited));
        // A legitimate tablet on a DIFFERENT IP B is NOT affected — it can
        // still pair with the correct PIN (this was the DoS the global
        // limiter caused).
        assert_eq!(auth.try_pin(ip_b(), "424242"), Ok("tok".to_string()));
    }

    #[test]
    fn global_backstop_rotates_pin_after_threshold() {
        let auth = AuthState::for_test("424242", "tok");
        let original = auth.pin();
        assert_eq!(original, "424242");

        // Drive GLOBAL_ROTATE_THRESHOLD total failures, spread across many
        // spoofed IPs so NO per-IP lockout ever bites (each IP stays under
        // MAX_ATTEMPTS) — exactly the attack the backstop defends against.
        for i in 0..GLOBAL_ROTATE_THRESHOLD {
            // A fresh IP per attempt → per-IP counters never reach the cap.
            let ip: IpAddr = format!("10.0.{}.{}", i / 256, i % 256).parse().unwrap();
            assert_eq!(auth.try_pin(ip, "000000"), Err(AuthError::BadPin));
        }

        // The PIN has rotated away from the original.
        let rotated = auth.pin();
        assert_ne!(rotated, original, "PIN must rotate after the backstop");
        assert_eq!(rotated.len(), 6);
        assert!(rotated.chars().all(|c| c.is_ascii_digit()));

        // The persisted bearer token is UNCHANGED — a paired tablet keeps
        // working through a rotation.
        assert!(auth.verify_token("tok"));
    }

    #[test]
    fn ct_eq_matches_std_eq() {
        assert!(ct_eq(b"hello", b"hello"));
        assert!(!ct_eq(b"hello", b"world"));
        assert!(!ct_eq(b"hello", b"hell"));
        assert!(ct_eq(b"", b""));
    }

    #[test]
    fn generated_pin_is_six_digits() {
        let pin = gen_pin();
        assert_eq!(pin.len(), 6);
        assert!(pin.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn generated_token_is_64_hex() {
        let tok = gen_token_hex();
        assert_eq!(tok.len(), 64);
        assert!(tok.chars().all(|c| c.is_ascii_hexdigit()));
        // Two calls differ (entropy, not a constant).
        assert_ne!(tok, gen_token_hex());
    }

    #[test]
    fn uniform_below_stays_in_range() {
        for _ in 0..1000 {
            assert!(uniform_below(1_000_000) < 1_000_000);
            assert!(uniform_below(10) < 10);
        }
    }

    // FIX B: the per-IP limiter map must stay hard-bounded under a flood of
    // distinct (spoofed) one-shot source IPs. Lazy GC alone never prunes IPs
    // that are never seen again, so without the cap this map would grow with
    // the address space. Drive far more distinct IPs than the cap through
    // `record_failure` and assert the map never exceeds MAX_TRACKED_IPS.
    #[test]
    fn per_ip_map_stays_bounded_under_many_distinct_ips() {
        let mut table = LimiterTable::default();
        let now = Instant::now();
        // ~3x the cap of distinct IPv4 addresses (spread across /8s so each
        // is unique and none repeats → lazy GC can't help).
        let total: u32 = (MAX_TRACKED_IPS as u32) * 3;
        for i in 0..total {
            let ip: IpAddr = std::net::Ipv4Addr::from(i).into();
            table.record_failure(ip, now);
            assert!(
                table.per_ip.len() <= MAX_TRACKED_IPS,
                "per_ip map exceeded MAX_TRACKED_IPS ({}) at i={i}",
                MAX_TRACKED_IPS
            );
        }
        // After the flood the map is bounded by the cap, NOT by the number of
        // attacking IPs.
        assert!(table.per_ip.len() <= MAX_TRACKED_IPS);
        // The global tally counted EVERY attempt (incl. the untracked ones),
        // so the rotation backstop still sees the full brute-force volume —
        // refusing to track a new IP is not a bypass. (Saturated at u32::MAX
        // only if `total` were huge; here it equals `total`.)
        assert_eq!(table.global_failures, total);
    }
}
