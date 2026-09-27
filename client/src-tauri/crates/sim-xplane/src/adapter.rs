//! Connection / lifecycle for the X-Plane UDP DataRef adapter.
//!
//! Mirrors the public shape of `MsfsAdapter`: `new` / `start` / `stop`
//! / `state` / `snapshot` / `last_error`. The streamer in
//! `src/lib.rs` polls `snapshot()` at the position-streamer cadence
//! and the touchdown sampler polls it at 50 Hz — same code path as
//! MSFS, the adapter is what changes.
//!
//! Implementation: synchronous `std::net::UdpSocket` + a dedicated
//! `std::thread`. We deliberately avoid tokio here: tokio
//! requires the caller to be inside an async runtime, but
//! `sim_set_kind` is a synchronous Tauri command and can be invoked
//! from any thread Tauri picks. The early build that used
//! `tokio::spawn` from inside `start()` crashed the app on sim
//! switch because no runtime was available on the calling thread.
//! `std::thread` works from any context.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Time without any RREF packet after which we declare the X-Plane
/// connection stale. Mirrors `STALE_TIMEOUT` on the MSFS adapter
/// (5 s) — long enough to survive a brief sim pause / loading screen,
/// short enough that a quit-and-reload doesn't leave us showing the
/// pre-quit position to the briefing tab. Live bug 2026-05-03: pilot
/// loaded MSFS at default airport, switched flight to SCEL, saw
/// "3142.5 nm von SCEL" because adapter still served the old position.
/// Same class of bug exists here — fix it preemptively.
const STALE_TIMEOUT: Duration = Duration::from_secs(5);

use sim_core::{SimKind, SimSnapshot, Simulator};

use crate::dataref::{addon_quelle, XPlaneState, CATALOG};
use crate::premium::{PremiumListener, PremiumStatus, PremiumTouchdown};
use crate::profile::{build_active_catalog, profile_index_for_title, ActiveEntry, PROFILES};
use crate::rref::{decode_response, encode_request};
use crate::web_api::{AircraftInfo, DrefIdCache, WebApiClient};
use crate::zusatz::{ZusatzAbos, ZUSATZ_HZ};
use crate::{SUBSCRIPTION_HZ, XPLANE_LISTEN_PORT};

/// v0.12.2 (LE1): RREF index base for the aircraft-profile probes.
/// Probe subscriptions get one index each starting here — far above any
/// `CATALOG` index, so a probe packet is unambiguously identifiable and
/// never collides with a real catalog entry.
const DISCOVERY_INDEX_BASE: i32 = 10_000;

/// How often the Web API poller asks X-Plane for the aircraft info.
/// Aircraft identity rarely changes mid-flight (load a new plane =
/// new flight), so 30 s is plenty.
const AIRCRAFT_POLL_INTERVAL_SECS: u64 = 30;

#[derive(Debug, Clone, Copy, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// One DataRef catalog entry as exposed for the Settings → Debug
/// panel. The frontend renders these in a table so the pilot can
/// see exactly which DataRefs we subscribe to and their last value.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DatarefSample {
    pub index: u32,
    pub name: &'static str,
    pub value: f32,
    /// True if we've ever received a value for this index from
    /// X-Plane. Useful for spotting DataRefs the sim rejected
    /// (older XP build, missing payware, etc.) — they stay `false`
    /// and zero forever.
    pub has_value: bool,
}

struct AdapterShared {
    state: Mutex<ConnectionState>,
    last_error: Mutex<Option<String>>,
    /// Parsed accumulated DataRef state. Mutated from the listener
    /// thread, read by `snapshot()` and `subscribed_datarefs()`.
    parsed: Mutex<XPlaneState>,
    /// Per-index "has X-Plane sent us this DataRef yet?" flag — for
    /// the debug panel.
    seen: Mutex<Vec<bool>>,
    /// Per-index last raw float value (for debug panel display).
    last_values: Mutex<Vec<f32>>,
    /// v0.12.2 (LE6): the **active catalog** the listener is currently
    /// subscribed to — the static `CATALOG` with the detected aircraft
    /// profile's dataref overrides applied. Same length/indices as
    /// `CATALOG`. The listener owns the working copy and publishes it
    /// here so the debug panel shows the dataref names actually in use.
    active_catalog: Mutex<Vec<ActiveEntry>>,
    /// Aircraft identity from the X-Plane 12.1+ Web API. Empty
    /// `AircraftInfo` (all fields None) until the poller's first
    /// successful response, OR forever if the Web API is unreachable
    /// (X-Plane <12.1, or pilot didn't enable Settings → Network →
    /// Web Server).
    aircraft: Mutex<AircraftInfo>,
    /// Zusatzwerte des Telemetrie-Monitors (v1.8), eigene RREF-Abos ab
    /// `ZUSATZ_INDEX_BASE`. Leer, solange der Monitor zu ist.
    zusatz: Mutex<ZusatzAbos>,
    /// Add-on-Datarefs und Profil-Proben, die das geladene Flugzeug laut
    /// Web-API wirklich hat (Name ohne „[n]"). `None`, solange die Web-API
    /// nichts bestaetigt hat (X-Plane 11, Web-API aus, erste Sekunden) —
    /// dann gelten nur die Standardwerte. Grund: RREF liefert fuer JEDEN
    /// Namen Werte, auch fuer fehlende (gemessen 27.09.2026).
    addon_vorhanden: Mutex<Option<HashSet<String>>>,
    /// Zaehlt jede Aenderung von `addon_vorhanden`; der Listener leert
    /// daraufhin Felder, deren Quelle weggefallen ist.
    addon_generation: AtomicU64,
    /// Laufnummer des Web-API-Fadens. `stop()` wartet nicht auf ihn (eine
    /// langsame Web-API hielte sonst Simulatorwechsel und Sampler auf);
    /// ein alter Faden erkennt an der Nummer, dass er nichts mehr
    /// schreiben darf, und beendet sich.
    web_lauf: AtomicU64,
    /// Haelt der Web-API-Faden, waehrend er prueft, ob er noch dran ist, und
    /// schreibt; `start()`/`stop()` halten ihn beim Laufwechsel. So kann ein
    /// alter Faden nicht zwischen Pruefung und Schreiben ueberholt werden
    /// (Codex-Befund 27.09.2026).
    web_schreiben: Mutex<()>,
    /// Tells the worker thread to stop. Polled in the recv loop.
    stop: AtomicBool,
}

pub struct XPlaneAdapter {
    shared: Arc<AdapterShared>,
    worker: Option<JoinHandle<()>>,
    /// Web API poller (X-Plane 12.1+ Settings → Network → Web Server).
    /// Independently joined so we always tear down both threads on
    /// `stop()` even if one already exited.
    web_api_worker: Option<JoinHandle<()>>,
    /// Listener for the optional AeroACARS X-Plane Plugin (v0.5.0+).
    /// When the pilot has the plugin installed, this thread receives
    /// JSON telemetry/touchdown packets on UDP 52000 and surfaces a
    /// frame-perfect touchdown event. Inert if no plugin is present
    /// — bind succeeds, no packets ever arrive, RREF path handles
    /// everything as before.
    premium: PremiumListener,
    /// Cached SimKind so `snapshot()` knows whether to stamp
    /// `Simulator::XPlane11` or `XPlane12`.
    kind: SimKind,
}

impl Default for XPlaneAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl XPlaneAdapter {
    pub fn new() -> Self {
        let shared = Arc::new(AdapterShared {
            state: Mutex::new(ConnectionState::Disconnected),
            last_error: Mutex::new(None),
            parsed: Mutex::new(XPlaneState::default()),
            seen: Mutex::new(vec![false; CATALOG.len()]),
            last_values: Mutex::new(vec![0.0; CATALOG.len()]),
            active_catalog: Mutex::new(build_active_catalog(None)),
            aircraft: Mutex::new(AircraftInfo::default()),
            zusatz: Mutex::new(ZusatzAbos::default()),
            addon_vorhanden: Mutex::new(None),
            addon_generation: AtomicU64::new(0),
            web_lauf: AtomicU64::new(0),
            web_schreiben: Mutex::new(()),
            stop: AtomicBool::new(false),
        });
        Self {
            shared,
            worker: None,
            web_api_worker: None,
            premium: PremiumListener::new(),
            kind: SimKind::XPlane12,
        }
    }

    /// Start the listener for a given X-Plane version. Idempotent —
    /// if a worker is already running we stop it and start fresh
    /// (the `kind` may have changed between calls).
    pub fn start(&mut self, kind: SimKind) {
        if !kind.is_xplane() {
            tracing::warn!(
                ?kind,
                "XPlaneAdapter::start called with non-XPlane kind, ignoring"
            );
            return;
        }
        self.stop();
        self.kind = kind;
        // Reset state for a fresh run.
        *self.shared.state.lock() = ConnectionState::Connecting;
        *self.shared.last_error.lock() = None;
        *self.shared.parsed.lock() = XPlaneState::default();
        {
            let _w = self.shared.web_schreiben.lock();
            *self.shared.aircraft.lock() = AircraftInfo::default();
            *self.shared.addon_vorhanden.lock() = None;
            self.shared.addon_generation.fetch_add(1, Ordering::SeqCst);
        }
        // v0.12.2: a fresh run starts on the base catalog — profile
        // detection re-runs from scratch against the new sim session.
        *self.shared.active_catalog.lock() = build_active_catalog(None);
        for v in self.shared.seen.lock().iter_mut() {
            *v = false;
        }
        for v in self.shared.last_values.lock().iter_mut() {
            *v = 0.0;
        }
        self.shared.stop.store(false, Ordering::SeqCst);
        let shared_for_udp = Arc::clone(&self.shared);
        let udp_handle = std::thread::Builder::new()
            .name("xplane-udp".into())
            .spawn(move || run_listener(shared_for_udp))
            .expect("spawn xplane-udp thread");
        self.worker = Some(udp_handle);
        let shared_for_web = Arc::clone(&self.shared);
        let lauf = {
            let _w = self.shared.web_schreiben.lock();
            self.shared.web_lauf.fetch_add(1, Ordering::SeqCst) + 1
        };
        let web_handle = std::thread::Builder::new()
            .name("xplane-web-api".into())
            .spawn(move || run_web_api_poller(shared_for_web, lauf))
            .expect("spawn xplane-web-api thread");
        self.web_api_worker = Some(web_handle);
        // Start the premium plugin listener too. No-op unless the
        // optional X-Plane Plugin is installed — see `premium.rs`.
        self.premium.start();
        tracing::info!(?kind, "X-Plane adapter started");
    }

    pub fn stop(&mut self) {
        let had_worker = self.worker.is_some() || self.web_api_worker.is_some();
        if had_worker {
            self.shared.stop.store(true, Ordering::SeqCst);
        }
        if let Some(handle) = self.worker.take() {
            // Wait briefly for the thread to exit gracefully so we
            // unsubscribe RREFs before returning. 250 ms is plenty —
            // the recv loop has a 100 ms read timeout.
            let _ = handle.join();
        }
        if self.web_api_worker.take().is_some() {
            let _w = self.shared.web_schreiben.lock();
            // Nicht abwarten: eine Web-API-Abfrage kann bis zu 2 s haengen,
            // eine Pruefrunde viele davon (Codex-Befund 27.09.2026). Die
            // neue Laufnummer macht den alten Faden stumm; er beendet sich
            // bei der naechsten Abfrage des Stop-Flags.
            self.shared.web_lauf.fetch_add(1, Ordering::SeqCst);
        }
        // Tear down the premium listener last so it can drain any
        // in-flight packet before close. Idempotent.
        self.premium.stop();
        *self.shared.state.lock() = ConnectionState::Disconnected;
    }

    /// Status of the AeroACARS X-Plane Plugin connection (v0.5.0+).
    /// `active=true` when we've received a packet within the last
    /// 3 s — drives the "X-PLANE PREMIUM" badge in the UI.
    pub fn premium_status(&self) -> PremiumStatus {
        self.premium.status()
    }

    /// Drain a pending plugin-emitted touchdown event, if any. The
    /// flight sampler in the main app calls this each tick after
    /// the standard `snapshot()` read; if Some, the values override
    /// the sampler's own RREF-based touchdown detection (frame-
    /// perfect timing, lookback-peak VS).
    pub fn take_premium_touchdown(&self) -> Option<PremiumTouchdown> {
        self.premium.take_touchdown()
    }

    /// Last error from the premium listener (e.g. bind failure).
    /// Independent from `last_error()` so RREF and premium errors
    /// don't clobber each other.
    pub fn premium_last_error(&self) -> Option<String> {
        self.premium.last_error()
    }

    pub fn state(&self) -> ConnectionState {
        *self.shared.state.lock()
    }

    /// Force-clear the parsed RREF state so `snapshot()` returns
    /// `None` until X-Plane delivers a fresh batch of values. Used by
    /// the UI's "Re-check sim position" button when the pilot
    /// suspects the cached lat/lon is stale (e.g. flight switched in
    /// X-Plane but our 5 s STALE_TIMEOUT hasn't fired because UDP
    /// kept trickling stray packets through the load). Connection
    /// state is downgraded to Connecting so the UI shows "waiting
    /// for sim position …" until the next real packet lands.
    pub fn clear_snapshot(&self) {
        *self.shared.parsed.lock() = XPlaneState::default();
        for v in self.shared.seen.lock().iter_mut() {
            *v = false;
        }
        for v in self.shared.last_values.lock().iter_mut() {
            *v = 0.0;
        }
        *self.shared.state.lock() = ConnectionState::Connecting;
        tracing::info!("X-Plane snapshot cleared by user (force-resync)");
    }

    pub fn snapshot(&self) -> Option<SimSnapshot> {
        let parsed = self.shared.parsed.lock();
        if !parsed.got_first_packet {
            return None;
        }
        let sim = match self.kind {
            SimKind::XPlane11 => Simulator::XPlane11,
            _ => Simulator::XPlane12,
        };
        let mut snap = parsed.to_snapshot(sim);
        // Overlay aircraft identity from the Web API poller (X-Plane
        // 12.1+ Settings → Network → Web Server). Stays None until the
        // first successful poll, OR forever when the Web API isn't
        // reachable (X-Plane <12.1, or pilot didn't enable it). The
        // SimSnapshot fields default to None in that path so the
        // existing "(unknown)" UI label still shows.
        let aircraft = self.shared.aircraft.lock();
        if aircraft.descrip.is_some() {
            snap.aircraft_title = aircraft.descrip.clone();
        }
        if aircraft.icao.is_some() {
            snap.aircraft_icao = aircraft.icao.clone();
        }
        if aircraft.tailnum.is_some() {
            snap.aircraft_registration = aircraft.tailnum.clone();
        }
        Some(snap)
    }

    pub fn last_error(&self) -> Option<String> {
        self.shared.last_error.lock().clone()
    }

    /// Return the catalog with each DataRef's most recent received
    /// value. Used by the Settings → Debug panel — analogous to the
    /// MSFS Inspector but auto-populated.
    ///
    /// v0.12.2: reads the **active catalog** so the panel shows the
    /// dataref names actually in use — including a detected aircraft
    /// profile's overrides (e.g. the CL650 flaps dataref).
    /// Telemetrie-Monitor: Zusatz-DataRefs setzen, (Kanal-ID, DataRef).
    /// Leere Liste = Abos beenden. Der Empfangsthread gleicht im naechsten
    /// Durchlauf ab.
    pub fn zusatz_setzen(&self, felder: Vec<(String, String)>) {
        self.shared.zusatz.lock().setzen(felder);
    }

    /// Aktuelle Zusatzwerte: (Kanal-ID, Rohwert des DataRefs).
    pub fn zusatz_werte(&self) -> Vec<(String, f64)> {
        self.shared.zusatz.lock().werte()
    }

    pub fn subscribed_datarefs(&self) -> Vec<DatarefSample> {
        let seen = self.shared.seen.lock();
        let last = self.shared.last_values.lock();
        let active = self.shared.active_catalog.lock();
        active
            .iter()
            .enumerate()
            .map(|(i, e)| DatarefSample {
                index: i as u32,
                name: e.name,
                value: last.get(i).copied().unwrap_or(0.0),
                has_value: seen.get(i).copied().unwrap_or(false),
            })
            .collect()
    }
}

impl Drop for XPlaneAdapter {
    fn drop(&mut self) {
        self.stop();
    }
}

/// v0.12.2 (LE1): decide which aircraft profile should be active from
/// the two detection signals. Pure so the runtime aircraft-swap logic
/// can be unit-tested without a UDP socket (QS-R4/P3).
///
/// * `title` — the Web API aircraft title, or `None` (Web API off / XP11).
/// * `probe_fresh` — per-profile (index = position in `PROFILES`): did
///   that profile's probe DataRef answer within `PROBE_STALE_AFTER`.
/// * `probe_seen` — per-profile: has that probe DataRef *ever* answered.
///
/// The probe is the **live ground truth**: a profile whose signature
/// DataRef answered recently IS the loaded aircraft, so a fresh probe
/// always wins. A title match only counts during the initial discovery
/// window — before that profile's probe has answered even once. Once a
/// probe has been heard and then fallen silent, the aircraft is gone;
/// the laggy Web API title (polled every 30 s, so up to 30 s stale)
/// must NOT revive a profile the probe already retired (QS-R2/P2).
/// When nothing points at a profile the result is `None` → base catalog.
fn desired_profile(
    title: Option<&str>,
    probe_fresh: &[bool],
    probe_seen: &[bool],
) -> Option<usize> {
    if let Some(pi) = probe_fresh.iter().position(|&fresh| fresh) {
        return Some(pi);
    }
    title
        .and_then(profile_index_for_title)
        .filter(|&pi| !probe_seen.get(pi).copied().unwrap_or(false))
}

/// Ob ein Empfangsfehler nur die ICMP-Rueckmeldung „Port nicht erreichbar"
/// auf ein eigenes RREF-Paket ist. Windows meldet sie auf UDP-Sockets als
/// `WSAECONNRESET` (10054 → `ConnectionReset`), Linux/macOS je nach Lage
/// als `ConnectionRefused`. Kein Fehler des Sockets — X-Plane hoert
/// nur gerade nicht zu.
fn ist_icmp_rueckmeldung(kind: std::io::ErrorKind) -> bool {
    matches!(
        kind,
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused
    )
}

#[cfg(test)]
mod icmp_rueckmeldung_tests {
    use super::ist_icmp_rueckmeldung;
    use std::io::ErrorKind;

    /// Genau der Fehler aus dem Diagnose-Log (Windows, os error 10054).
    #[test]
    fn windows_10054_ist_nur_eine_rueckmeldung() {
        #[cfg(windows)]
        assert!(ist_icmp_rueckmeldung(
            std::io::Error::from_raw_os_error(10054).kind()
        ));
        assert!(ist_icmp_rueckmeldung(ErrorKind::ConnectionReset));
        assert!(ist_icmp_rueckmeldung(ErrorKind::ConnectionRefused));
    }

    /// Gegenprobe: echte Socket-Fehler bleiben Warnungen mit Pause.
    #[test]
    fn echte_fehler_bleiben_fehler() {
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::InvalidInput,
            ErrorKind::AddrNotAvailable,
            ErrorKind::Other,
        ] {
            assert!(!ist_icmp_rueckmeldung(kind), "{kind:?}");
        }
    }
}

/// Grundname eines Datarefs ohne Array-Index („…[7]" → „…").
fn grundname(name: &str) -> &str {
    name.split('[').next().unwrap_or(name)
}

/// Alle Datarefs, deren Vorhandensein die Web-API bestaetigen muss:
/// Add-on-Quellen des Katalogs und die Profil-Proben.
fn zu_pruefende_datarefs() -> Vec<&'static str> {
    let mut v: Vec<&'static str> = CATALOG
        .iter()
        .filter(|e| addon_quelle(e.field))
        .map(|e| grundname(e.name))
        .chain(PROFILES.iter().map(|p| grundname(p.probe_dataref)))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Neuer Stand der bestaetigten Quellen aus einer Pruefrunde.
/// `Ok(true)` nimmt auf, `Ok(false)` streicht, ein Fehler (Zeitueberschreitung,
/// Simulator beschaeftigt) laesst den bisherigen Stand dieses Namens stehen —
/// eine Quelle faellt nur weg, wenn X-Plane klar absagt. Beim Flugzeugwechsel
/// (`neues_flugzeug`) wird nichts vom alten Flugzeug uebernommen.
fn vorhanden_neu(
    alt: Option<&HashSet<String>>,
    neues_flugzeug: bool,
    ergebnisse: &[(&str, Option<bool>)],
) -> HashSet<String> {
    let mut neu: HashSet<String> = if neues_flugzeug {
        HashSet::new()
    } else {
        alt.cloned().unwrap_or_default()
    };
    for (name, lesbar) in ergebnisse {
        match lesbar {
            Some(true) => {
                neu.insert((*name).to_string());
            }
            Some(false) => {
                neu.remove(*name);
            }
            None => {}
        }
    }
    neu
}

/// Gilt dieser Katalogeintrag? Standardwerte immer; Add-on-Quellen nur,
/// wenn die Web-API sie bestaetigt hat.
///
/// Ohne Web-API (X-Plane 11, abgeschaltet — Codex-Befund 27.09.2026) gilt
/// eine Quelle, sobald sie einmal einen Wert ungleich 0 geliefert hat
/// (`ungleich_null`): ein fehlender Dataref liefert ueber RREF nur 0, kann
/// diese Schwelle also nie nehmen.
fn eintrag_gilt(
    field: crate::dataref::FieldId,
    name: &str,
    vorhanden: Option<&HashSet<String>>,
    ungleich_null: &HashSet<&'static str>,
) -> bool {
    if !addon_quelle(field) {
        return true;
    }
    match vorhanden {
        Some(v) => v.contains(grundname(name)),
        None => ungleich_null.contains(grundname(name)),
    }
}

/// Ein anderes Flugzeug, wenn sich das Leergewicht um mehr als 1 kg aendert
/// (Tankfuellung und Zuladung aendern es nicht).
fn flugzeug_gewechselt(alt_kg: f32, neu_kg: f32) -> bool {
    (alt_kg - neu_kg).abs() > 1.0
}

/// Wie `eintrag_gilt`, fuer die Profil-Proben.
fn probe_gilt(
    probe: &str,
    vorhanden: Option<&HashSet<String>>,
    ungleich_null: &HashSet<&'static str>,
) -> bool {
    match vorhanden {
        Some(v) => v.contains(grundname(probe)),
        None => ungleich_null.contains(grundname(probe)),
    }
}

/// The blocking listener thread. Binds a UDP socket on an ephemeral
/// local port, subscribes the active catalog (+ aircraft-profile
/// probes) to 127.0.0.1:49000, then loops decoding responses until
/// `shared.stop` flips to `true`.
///
/// v0.12.2: the listener also runs aircraft-profile detection (LE1) —
/// title-match via the Web API overlay plus an RREF probe — and on a
/// match rebuilds the active catalog with the profile's dataref
/// overrides and re-subscribes (LE6).
fn run_listener(shared: Arc<AdapterShared>) {
    use std::net::UdpSocket;

    let socket = match UdpSocket::bind("127.0.0.1:0") {
        Ok(s) => s,
        Err(e) => {
            *shared.last_error.lock() = Some(format!("bind failed: {e}"));
            *shared.state.lock() = ConnectionState::Disconnected;
            tracing::error!(error = %e, "could not bind XPlane UDP socket");
            return;
        }
    };
    // Non-blocking-ish: 100 ms read timeout so we re-check the
    // stop flag at least every 100 ms.
    if let Err(e) = socket.set_read_timeout(Some(Duration::from_millis(100))) {
        tracing::warn!(error = %e, "could not set XPlane UDP read timeout");
    }
    let local_addr = socket
        .local_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "?".into());
    tracing::info!(local = %local_addr, "X-Plane UDP socket bound");

    let xplane_addr = format!("127.0.0.1:{XPLANE_LISTEN_PORT}");

    // ---- v0.12.2: active catalog + aircraft-profile state ----
    // `active` is the static CATALOG with the detected profile's
    // dataref overrides applied (LE6) — same length/indices as CATALOG.
    let mut active: Vec<ActiveEntry> = build_active_catalog(None);
    // Index into PROFILES of the active profile, None until detected.
    let mut active_profile: Option<usize> = None;
    // v0.12.2 (QS-R4/P1): per-profile timestamp of the last probe
    // response. The probes stay subscribed for the whole session, so a
    // profile counts as "the loaded aircraft" only while its probe
    // answered within `PROBE_STALE_AFTER`. Once it falls silent the
    // aircraft is gone and the profile drops back to the base catalog —
    // this is what makes the runtime aircraft-swap reset (LE6) work even
    // when there is no Web API title (XP11 / Web API off), the case the
    // old `last_title`-diff logic missed.
    let mut probe_last_seen: Vec<Option<Instant>> = vec![None; PROFILES.len()];
    // Stand der bestaetigten Add-on-Quellen, den dieser Thread zuletzt
    // uebernommen hat (siehe `AdapterShared::addon_vorhanden`).
    let mut addon_gen_gesehen: u64 = u64::MAX;
    let mut vorhanden: Option<HashSet<String>> = None;
    // Ersatz ohne Web-API: Quellen, die schon einmal ungleich 0 waren.
    let mut ungleich_null: HashSet<&'static str> = HashSet::new();
    // Leergewicht als Kennung des geladenen Flugzeugs — kommt per RREF, also
    // auch ohne Web-API. Aendert es sich, ist ein anderes Flugzeug geladen.
    let mut leergewicht: Option<f32> = None;

    // ---- Hard-armoured re-subscribe (v0.3.0) ----
    // Send the full RREF subscription set for the given catalog. Called
    // at startup, on profile activation, and periodically while not
    // Connected. Idempotent on X-Plane's side — a duplicate RREF at the
    // same index just refreshes the rate / re-binds the dataref.
    let subscribe_catalog = |sock: &UdpSocket, cat: &[ActiveEntry]| {
        for (i, entry) in cat.iter().enumerate() {
            let req = encode_request(SUBSCRIPTION_HZ as i32, i as i32, entry.name);
            if let Err(e) = sock.send_to(&req, &xplane_addr) {
                tracing::trace!(
                    error = %e,
                    dataref = entry.name,
                    "RREF subscribe send failed (will retry on next tick)"
                );
            }
        }
    };
    // v0.12.2 (LE1 stage 2): one probe subscription per profile, at a
    // reserved discovery index. Low rate — we only need to learn the
    // dataref exists, not stream it.
    let subscribe_probes = |sock: &UdpSocket| {
        for (pi, prof) in PROFILES.iter().enumerate() {
            let req = encode_request(1, DISCOVERY_INDEX_BASE + pi as i32, prof.probe_dataref);
            let _ = sock.send_to(&req, &xplane_addr);
        }
    };
    // Cancel all probe subscriptions (freq = 0) — best-effort cleanup
    // on listener shutdown. The probes otherwise stay subscribed for the
    // whole session (QS-R4/P1) so an aircraft swap is always detected.
    let unsubscribe_probes = |sock: &UdpSocket| {
        for (pi, prof) in PROFILES.iter().enumerate() {
            let stop = encode_request(0, DISCOVERY_INDEX_BASE + pi as i32, prof.probe_dataref);
            let _ = sock.send_to(&stop, &xplane_addr);
        }
    };

    // Telemetrie-Monitor: Zusatzabos, die gerade bestehen, und der
    // Listenstand, zu dem sie gehoeren.
    let mut zusatz_abonniert: Vec<(i32, String)> = Vec::new();
    let mut zusatz_generation: u64 = 0;
    // In Paketen senden: X-Plane verarbeitet nur etwa 140 Abos je Bild,
    // der Rest ginge sonst verloren.
    let zusatz_senden = |sock: &UdpSocket, abos: &[(i32, String)], hz: i32| {
        for (n, (idx, dataref)) in abos.iter().enumerate() {
            if n > 0 && n % 40 == 0 {
                std::thread::sleep(Duration::from_millis(30));
            }
            let req = encode_request(hz, *idx, dataref);
            let _ = sock.send_to(&req, &xplane_addr);
        }
    };

    // Initial subscribe — base catalog + profile probes.
    subscribe_catalog(&socket, &active);
    subscribe_probes(&socket);
    *shared.active_catalog.lock() = active.clone();

    // Listen.
    let mut buf = vec![0u8; 8192];
    let mut last_packet_at: Option<Instant> = None;
    let mut last_resubscribe_at = Instant::now();
    /// How often to re-send the full subscription set while we're
    /// not yet Connected. 5 seconds matches `STALE_TIMEOUT` so the
    /// recovery feels coherent.
    const RESUBSCRIBE_INTERVAL: Duration = Duration::from_secs(5);
    /// v0.12.2 (QS-R4): a profile's probe DataRef must answer at least
    /// this often for the profile to stay active. The probe runs at
    /// 1 Hz; 8 s tolerates a few dropped packets and a brief reconnect
    /// blip without flapping the profile, while still catching a real
    /// aircraft swap — the old aircraft's probe DataRef simply vanishes.
    const PROBE_STALE_AFTER: Duration = Duration::from_secs(8);

    // Einmal je Ausfall melden, dass X-Plane nicht zuhoert (siehe
    // `ist_icmp_rueckmeldung`); zurueckgesetzt beim naechsten Paket.
    let mut sim_hoert_nicht_gemeldet = false;

    while !shared.stop.load(Ordering::SeqCst) {
        match socket.recv_from(&mut buf) {
            Ok((n, _peer)) => {
                sim_hoert_nicht_gemeldet = false;
                let pairs = decode_response(&buf[..n]);
                if pairs.is_empty() {
                    continue;
                }
                // `last_packet_at` erst setzen, wenn ein KATALOGWERT dabei war
                // (siehe unten). Zusatzwerte des Telemetrie-Monitors und
                // Proben duerfen einen Ausfall des Katalogs nicht verdecken —
                // sonst raeumte der Watchdog den veralteten Snapshot nicht
                // weg (Codex-Befund 1, 25.09.2026).
                let mut katalog_paket = false;
                let mut parsed = shared.parsed.lock();
                let mut seen = shared.seen.lock();
                let mut last = shared.last_values.lock();
                let mut zusatz = shared.zusatz.lock();
                for p in pairs {
                    // Telemetrie-Monitor: Zusatzwerte liegen ueber den
                    // Proben und werden vor deren Pruefung abgefangen.
                    if zusatz.empfangen(p.index, p.value) {
                        continue;
                    }
                    // v0.12.2 (LE1): discovery-index packets are PROBE
                    // responses — they only prove a profile's dataref
                    // exists. Intercepted BEFORE `apply_field`; they
                    // never mutate a snapshot value.
                    if p.index >= DISCOVERY_INDEX_BASE {
                        let pi = (p.index - DISCOVERY_INDEX_BASE) as usize;
                        if let Some(slot) = probe_last_seen.get_mut(pi) {
                            *slot = Some(Instant::now());
                        }
                        if p.value != 0.0 {
                            if let Some(prof) = PROFILES.get(pi) {
                                ungleich_null.insert(grundname(prof.probe_dataref));
                            }
                        }
                        continue;
                    }
                    // Normal catalog packet: index → active entry →
                    // FieldId, with the profile's ValueMapping applied.
                    if let Some(entry) = active.get(p.index as usize) {
                        katalog_paket = true;
                        if entry.field == crate::dataref::FieldId::EmptyWeightKg && p.value > 0.0 {
                            if let Some(alt) = leergewicht {
                                if flugzeug_gewechselt(alt, p.value) {
                                    // Ohne Web-API sonst unbemerkt (Codex-Befund):
                                    // Ersatzliste und Proben des alten Flugzeugs weg.
                                    ungleich_null.clear();
                                    for t in probe_last_seen.iter_mut() {
                                        *t = None;
                                    }
                                    if vorhanden.is_none() {
                                        for e in CATALOG.iter().filter(|e| addon_quelle(e.field)) {
                                            parsed.addon_leeren(e.field);
                                        }
                                    }
                                    tracing::info!(
                                        alt,
                                        neu = p.value,
                                        "X-Plane: Leergewicht geaendert — Flugzeugwechsel"
                                    );
                                }
                            }
                            leergewicht = Some(p.value);
                        }
                        if p.value != 0.0 && addon_quelle(entry.field) {
                            ungleich_null.insert(grundname(entry.name));
                        }
                        let gilt = eintrag_gilt(
                            entry.field,
                            entry.name,
                            vorhanden.as_ref(),
                            &ungleich_null,
                        );
                        if gilt {
                            if let Some(mapped) = entry.mapping.map(p.value) {
                                parsed.apply_field(entry.field, mapped);
                            }
                        }
                        // seen/last reflect the RAW value X-Plane sent.
                        if let Some(slot) = seen.get_mut(p.index as usize) {
                            *slot = true;
                        }
                        if let Some(slot) = last.get_mut(p.index as usize) {
                            *slot = p.value;
                        }
                    }
                }
                if katalog_paket {
                    last_packet_at = Some(Instant::now());
                }
                if parsed.got_first_packet {
                    let mut s = shared.state.lock();
                    if *s != ConnectionState::Connected {
                        *s = ConnectionState::Connected;
                        tracing::info!("X-Plane: first RREF packet received → Connected");
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // No data this tick — check stale timeout + resubscribe-
                // due timer below, then loop.
            }
            Err(e) if ist_icmp_rueckmeldung(e.kind()) => {
                // Befund 21.09.2026 (Diagnose-Log Ralf T): nach dem Beenden
                // von X-Plane schickt der Resubscribe alle 5 s den ganzen
                // Katalog an einen toten Port. Windows liefert fuer JEDES
                // dieser Pakete einen eigenen `os error 10054` zurueck. Mit
                // 100 ms Pause je Fehler holte die Schleife den Rueckstand
                // nie auf: 38 822 Warnungen in 65 Minuten, und eine
                // Wiederverbindung waere erst nach dem Abarbeiten des
                // Rueckstands gesehen worden. Die Rueckmeldung heisst nur
                // „niemand hoert zu" — wie „keine Daten" behandeln.
                if !sim_hoert_nicht_gemeldet {
                    tracing::info!(
                        error = %e,
                        "X-Plane hoert auf dem RREF-Port nicht (mehr) zu — warte auf den Simulator"
                    );
                    sim_hoert_nicht_gemeldet = true;
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "X-Plane UDP recv error");
                std::thread::sleep(Duration::from_millis(100));
            }
        }

        // ---- v0.12.2 (LE1/LE6): aircraft-profile detection ----
        // Re-evaluated every tick from the two LE1 signals:
        //   * title — case-insensitive substring match on the Web API
        //             aircraft title (None when the Web API is off / XP11)
        //   * probe — the profile's signature DataRef answered within
        //             `PROBE_STALE_AFTER` (works without the Web API)
        // Title wins when it points at a profile; otherwise the probe
        // decides. When neither signal points at a profile — e.g. the
        // pilot swapped from the CL650 to a non-profile aircraft, so the
        // CL650 probe DataRef vanished and its `probe_last_seen` went
        // stale — `desired` is None and the adapter falls back to the
        // base catalog. This is the runtime aircraft-swap path (LE6) and
        // (QS-R4/P1) now works even with no Web API title.
        // Neuer Stand der bestaetigten Add-on-Quellen: uebernehmen und
        // Felder leeren, deren Quelle nicht (mehr) bestaetigt ist.
        let gen = shared.addon_generation.load(Ordering::SeqCst);
        if gen != addon_gen_gesehen {
            addon_gen_gesehen = gen;
            vorhanden = shared.addon_vorhanden.lock().clone();
            let mut parsed = shared.parsed.lock();
            for e in CATALOG.iter() {
                if addon_quelle(e.field)
                    && !eintrag_gilt(e.field, e.name, vorhanden.as_ref(), &ungleich_null)
                {
                    parsed.addon_leeren(e.field);
                }
            }
            tracing::info!(
                bestaetigt = ?vorhanden.as_ref().map(|v| {
                    let mut l: Vec<&String> = v.iter().collect();
                    l.sort();
                    l
                }),
                "X-Plane: Add-on-Quellen laut Web-API"
            );
        }

        let current_title = shared.aircraft.lock().descrip.clone();
        // Eine Probe zaehlt nur, wenn die Web-API ihren Dataref bestaetigt:
        // RREF antwortet auch fuer fehlende Datarefs (mit 0) — bis
        // 27.09.2026 galt dadurch JEDES X-Plane-Flugzeug als Challenger 650.
        let probe_bestaetigt: Vec<bool> = PROFILES
            .iter()
            .map(|p| probe_gilt(p.probe_dataref, vorhanden.as_ref(), &ungleich_null))
            .collect();
        let probe_fresh: Vec<bool> = probe_last_seen
            .iter()
            .zip(&probe_bestaetigt)
            .map(|(t, &ok)| ok && t.is_some_and(|seen| seen.elapsed() < PROBE_STALE_AFTER))
            .collect();
        let probe_seen: Vec<bool> = probe_last_seen
            .iter()
            .zip(&probe_bestaetigt)
            .map(|(t, &ok)| ok && t.is_some())
            .collect();
        let desired = desired_profile(current_title.as_deref(), &probe_fresh, &probe_seen);

        if desired != active_profile {
            match desired {
                Some(pi) => tracing::info!(
                    profile = PROFILES[pi].name,
                    via = if probe_fresh.get(pi).copied().unwrap_or(false) {
                        "probe"
                    } else {
                        "title"
                    },
                    "X-Plane: aircraft DataRef profile activated"
                ),
                None => {
                    tracing::info!("X-Plane: aircraft changed — resetting to base DataRef catalog")
                }
            }
            active_profile = desired;
            // Rebuild the active catalog (LE6) and re-subscribe with
            // fresh indices. The probes keep running regardless, so a
            // later aircraft swap is always detected.
            active = build_active_catalog(desired.map(|pi| &PROFILES[pi]));
            *shared.active_catalog.lock() = active.clone();
            subscribe_catalog(&socket, &active);
        }

        // Telemetrie-Monitor: Zusatzabos mit der gewuenschten Liste
        // abgleichen. Alte Abos erst abbestellen (freq = 0), dann die
        // neuen setzen.
        let (gen_jetzt, abos_jetzt) = {
            let z = shared.zusatz.lock();
            (z.generation, z.abos())
        };
        if gen_jetzt != zusatz_generation {
            zusatz_senden(&socket, &zusatz_abonniert, 0);
            zusatz_senden(&socket, &abos_jetzt, ZUSATZ_HZ);
            tracing::info!(
                anzahl = abos_jetzt.len(),
                "X-Plane: Zusatzwerte fuer den Telemetrie-Monitor abonniert"
            );
            zusatz_abonniert = abos_jetzt;
            zusatz_generation = gen_jetzt;
        }

        // Stale-snapshot guard: if we WERE connected but haven't
        // seen any packet for STALE_TIMEOUT, treat the connection
        // as dropped — clear the parsed state (so snapshot() returns
        // None until fresh data arrives) and downgrade the connection
        // state. The next packet repopulates parsed and snaps us back
        // to Connected without intervention.
        if let Some(at) = last_packet_at {
            if at.elapsed() > STALE_TIMEOUT {
                let mut parsed = shared.parsed.lock();
                if parsed.got_first_packet {
                    tracing::warn!(
                        "X-Plane: no RREF packets for {:?} — clearing snapshot, marking connecting",
                        STALE_TIMEOUT
                    );
                    *parsed = XPlaneState::default();
                    let mut seen = shared.seen.lock();
                    for v in seen.iter_mut() {
                        *v = false;
                    }
                    let mut last = shared.last_values.lock();
                    for v in last.iter_mut() {
                        *v = 0.0;
                    }
                    shared.zusatz.lock().leeren();
                    ungleich_null.clear();
                    *shared.state.lock() = ConnectionState::Connecting;
                }
                // Reset so we don't fire the warning every tick.
                last_packet_at = None;
            }
        }

        // ---- Hard-armoured re-subscribe poll (v0.3.0) ----
        // Whenever we're not Connected, periodically resend the full
        // subscription set (active catalog + probes) so the connection
        // recovers from a cold start or an X-Plane restart on its own.
        let state_now = *shared.state.lock();
        if state_now != ConnectionState::Connected
            && last_resubscribe_at.elapsed() >= RESUBSCRIBE_INTERVAL
        {
            tracing::debug!("X-Plane: not connected — re-sending RREF subscriptions");
            subscribe_catalog(&socket, &active);
            subscribe_probes(&socket);
            zusatz_senden(&socket, &zusatz_abonniert, ZUSATZ_HZ);
            last_resubscribe_at = Instant::now();
        }
    }

    // Best-effort: send freq=0 RREF for every active catalog entry and
    // every probe so we don't leave X-Plane streaming into the void.
    for (i, entry) in active.iter().enumerate() {
        let req = encode_request(0, i as i32, entry.name);
        let _ = socket.send_to(&req, &xplane_addr);
    }
    unsubscribe_probes(&socket);
    zusatz_senden(&socket, &zusatz_abonniert, 0);
    tracing::info!("X-Plane UDP listener stopped");
}

/// Long-running poller that reads aircraft identity from the X-Plane
/// 12.1+ Web API (`http://localhost:8086`) and stashes the result in
/// `shared.aircraft`. Runs in its own thread so it can do blocking
/// HTTP without stalling the 50 Hz UDP listener.
///
/// Cadence is sparse (`AIRCRAFT_POLL_INTERVAL_SECS`) because aircraft
/// identity rarely changes mid-flight. On repeated failures
/// (X-Plane <12.1, or Web API not enabled in Settings → Network)
/// we back off further so we don't spam.
fn run_web_api_poller(shared: Arc<AdapterShared>, lauf: u64) {
    let client = WebApiClient::new();
    // Darf dieser Faden noch schreiben? Nein nach `stop()` oder einem
    // Neustart (neue Laufnummer).
    let aktiv =
        || !shared.stop.load(Ordering::SeqCst) && shared.web_lauf.load(Ordering::SeqCst) == lauf;
    let mut consecutive_failures: u32 = 0;
    let mut last_logged_path: Option<String> = None;
    // Flugzeug, fuer das `addon_vorhanden` zuletzt geprueft wurde.
    let mut geprueft_fuer: Option<String> = None;
    tracing::info!("X-Plane Web API poller started");
    // Sofort neu pruefen (Flugzeugwechsel waehrend einer Pruefung).
    let mut sofort = false;
    while aktiv() {
        // Dataref-IDs JEDEN Poll frisch auflösen. X-Plane baut beim
        // Flugzeugwechsel seine Dataref-Registry neu auf — eine prozess-
        // weit gecachte numerische ID wird dann stale: `read_string`
        // liefert dann den alten Flieger oder schlägt fehl, sodass der
        // Poller die alte `AircraftInfo` behält und der Fliegerwechsel
        // unbemerkt bleibt. Re-Discovery = 6 Loopback-GETs alle 30 s,
        // vernachlässigbar; dafür wird ein Aircraft-Swap zuverlässig
        // erkannt. (Pilot-Befund Michel, X-Plane 12.)
        let mut id_cache = DrefIdCache::default();
        match client.fetch_aircraft_info(&mut id_cache) {
            Ok(_) if !aktiv() => return,
            Ok(info) => {
                if consecutive_failures > 0 {
                    tracing::info!(
                        "X-Plane Web API recovered after {} failed polls",
                        consecutive_failures
                    );
                }
                consecutive_failures = 0;
                // Welche Add-on-Quellen hat das geladene Flugzeug wirklich?
                // Jede Runde neu (ein Add-on-Plugin laedt oft erst nach dem
                // Flugzeug); beim Wechsel ohne Altlasten.
                let neues_flugzeug = info.relative_path != geprueft_fuer;
                let mut ergebnisse: Vec<(&str, Option<bool>)> = Vec::new();
                for n in zu_pruefende_datarefs() {
                    // Bis zu zwei GETs je Name — zwischendurch aufs Stoppen
                    // achten, sonst haengt `stop()` (Codex-Befund).
                    if !aktiv() {
                        tracing::info!("X-Plane Web API poller stopped");
                        return;
                    }
                    ergebnisse.push((n, client.dataref_lesbar(n).ok()));
                }
                // Hat der Pilot waehrend der Pruefung das Flugzeug gewechselt,
                // gehoeren die Ergebnisse zu keinem Flugzeug — verwerfen.
                // Ohne erkannten Pfad laesst sich nichts zuordnen — dann
                // nichts uebernehmen (Codex-Befund).
                let pfad_bekannt = info.relative_path.is_some();
                let noch_dasselbe = pfad_bekannt
                    && client
                        .flugzeug_pfad()
                        .map(|p| p == info.relative_path)
                        .unwrap_or(false);
                let _w = shared.web_schreiben.lock();
                if !aktiv() {
                    return;
                }
                if noch_dasselbe {
                    let mut v = shared.addon_vorhanden.lock();
                    let neu = vorhanden_neu(v.as_ref(), neues_flugzeug, &ergebnisse);
                    if v.as_ref() != Some(&neu) {
                        *v = Some(neu);
                        shared.addon_generation.fetch_add(1, Ordering::SeqCst);
                    }
                    geprueft_fuer = info.relative_path.clone();
                } else if pfad_bekannt {
                    // Waehrend der Pruefung gewechselt: alte Quellen sofort
                    // verwerfen und gleich neu pruefen, nicht erst in 30 s.
                    *shared.addon_vorhanden.lock() = Some(HashSet::new());
                    shared.addon_generation.fetch_add(1, Ordering::SeqCst);
                    geprueft_fuer = None;
                    sofort = true;
                }
                // Pfad unbekannt: nichts zuordnen, normal weiter warten — kein
                // sofortiges Wiederholen, sonst liefe die Pruefung ungebremst.
                if info.has_any() {
                    // Log on first detection AND on aircraft change
                    // (e.g. pilot loaded a different plane). Identity
                    // by `relative_path` since it's the .acf path —
                    // unique even when two planes share a description.
                    let path = info.relative_path.clone();
                    let changed = last_logged_path != path;
                    if changed {
                        tracing::info!(
                            descrip = ?info.descrip,
                            icao = ?info.icao,
                            tailnum = ?info.tailnum,
                            "X-Plane aircraft detected via Web API"
                        );
                        last_logged_path = path;
                    }
                }
                // Nach erkanntem Wechsel nicht die alte Kennung veroeffentlichen.
                if !pfad_bekannt || noch_dasselbe {
                    *shared.aircraft.lock() = info;
                }
                drop(_w);
            }
            Err(e) => {
                consecutive_failures += 1;
                // Log once at info level on the first failure so the
                // pilot has something to grep for. Subsequent failures
                // stay at debug — a permanently-disabled Web API
                // would otherwise spam.
                if consecutive_failures == 1 {
                    tracing::info!(
                        error = %e,
                        "X-Plane Web API unavailable — aircraft identity will stay (unknown). \
                         Enable in X-Plane → Settings → Network → Web Server (X-Plane 12.1+)."
                    );
                } else {
                    tracing::debug!(error = %e, "X-Plane Web API poll failed");
                }
            }
        }
        // Sleep between polls. Back off after repeated failures so we
        // don't keep slamming a sim that obviously isn't going to
        // answer. Wake every 100 ms to re-check the stop flag.
        let secs = if consecutive_failures > 5 {
            AIRCRAFT_POLL_INTERVAL_SECS * 4
        } else {
            AIRCRAFT_POLL_INTERVAL_SECS
        };
        let ticks = if std::mem::take(&mut sofort) {
            0
        } else {
            secs * 10
        };
        for tick in 1..=ticks {
            if !aktiv() {
                tracing::info!("X-Plane Web API poller stopped");
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
            // Alle 5 s kurz nachsehen, ob ein anderes Flugzeug geladen ist.
            // Ohne das galten nach einem Wechsel bis zu 30 s die Quellen des
            // alten Flugzeugs — dessen Datarefs liefern dann nur noch Nullen
            // (Codex-Befund 27.09.2026). Beim Wechsel sofort alles verwerfen
            // und neu pruefen.
            if consecutive_failures == 0 && tick % 50 == 0 {
                if let Ok(pfad) = client.flugzeug_pfad() {
                    if !aktiv() {
                        return;
                    }
                    if pfad != geprueft_fuer {
                        let _w = shared.web_schreiben.lock();
                        if !aktiv() {
                            return;
                        }
                        tracing::info!(neu = ?pfad, "X-Plane: Flugzeugwechsel — Add-on-Quellen verworfen");
                        *shared.addon_vorhanden.lock() = Some(HashSet::new());
                        shared.addon_generation.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }
    }
    tracing::info!("X-Plane Web API poller stopped");
}

#[cfg(test)]
mod quellen_tests {
    use super::{eintrag_gilt, probe_gilt, vorhanden_neu, zu_pruefende_datarefs};
    use crate::dataref::FieldId;
    use std::collections::HashSet;

    fn menge(n: &[&str]) -> HashSet<String> {
        n.iter().map(|s| s.to_string()).collect()
    }

    /// Befund 27.09.2026: RREF liefert auch fehlende Datarefs (als 0).
    /// Ohne Bestaetigung der Web-API darf keine Add-on-Quelle gelten —
    /// Standardwerte immer.
    #[test]
    fn ohne_bestaetigung_nur_standardwerte() {
        let leer = HashSet::new();
        assert!(!eintrag_gilt(
            FieldId::B738XpdrKnob,
            "laminar/B738/knob/transponder_pos",
            None,
            &leer
        ));
        assert!(!eintrag_gilt(
            FieldId::TolissStrobeSwitch,
            "AirbusFBW/OHPLightSwitches[7]",
            None,
            &leer
        ));
        assert!(eintrag_gilt(
            FieldId::LightStrobe,
            "sim/cockpit2/switches/strobe_lights_on",
            None,
            &leer
        ));
        let v = menge(&[
            "AirbusFBW/OHPLightSwitches",
            "laminar/a333/switches/strobe_pos",
        ]);
        assert!(eintrag_gilt(
            FieldId::TolissStrobeSwitch,
            "AirbusFBW/OHPLightSwitches[7]",
            Some(&v),
            &leer
        ));
        assert!(eintrag_gilt(
            FieldId::A333StrobePos,
            "laminar/a333/switches/strobe_pos",
            Some(&v),
            &leer
        ));
        assert!(!eintrag_gilt(
            FieldId::B738XpdrKnob,
            "laminar/B738/knob/transponder_pos",
            Some(&v),
            &leer
        ));
    }

    /// Ohne Web-API (X-Plane 11): eine Quelle gilt erst, wenn sie einmal
    /// ungleich 0 war — ein fehlender Dataref liefert nur 0 und bleibt aus.
    #[test]
    fn ohne_web_api_zaehlt_erst_ein_wert_ungleich_null() {
        let mut gesehen: HashSet<&'static str> = HashSet::new();
        let knopf = "laminar/B738/knob/transponder_pos";
        assert!(!eintrag_gilt(FieldId::B738XpdrKnob, knopf, None, &gesehen));
        gesehen.insert(knopf);
        assert!(eintrag_gilt(FieldId::B738XpdrKnob, knopf, None, &gesehen));
        // Mit Web-API entscheidet allein deren Antwort.
        assert!(!eintrag_gilt(
            FieldId::B738XpdrKnob,
            knopf,
            Some(&HashSet::new()),
            &gesehen
        ));
        let probe = "abus/CL650/ARINC429/L-DCU-7/words/FCTL/0/FLAPS_LVR";
        assert!(!probe_gilt(probe, None, &HashSet::new()));
        assert!(!probe_gilt(
            probe,
            Some(&HashSet::new()),
            &HashSet::from([probe])
        ));
    }

    /// Leergewicht als Flugzeugkennung ohne Web-API.
    #[test]
    fn leergewicht_zeigt_den_wechsel() {
        assert!(!super::flugzeug_gewechselt(41_413.0, 41_413.0));
        assert!(!super::flugzeug_gewechselt(41_413.0, 41_413.6));
        assert!(super::flugzeug_gewechselt(41_413.0, 120_900.0));
    }

    /// Nur eine klare Absage streicht eine Quelle; ein Zeitfehler laesst
    /// den alten Stand stehen. Beim Flugzeugwechsel beginnt alles leer.
    #[test]
    fn stand_folgt_nur_klaren_antworten() {
        let alt = menge(&["a", "b"]);
        let neu = vorhanden_neu(
            Some(&alt),
            false,
            &[("a", None), ("b", Some(false)), ("c", Some(true))],
        );
        assert_eq!(neu, menge(&["a", "c"]));
        let wechsel = vorhanden_neu(Some(&alt), true, &[("a", None), ("c", Some(true))]);
        assert_eq!(wechsel, menge(&["c"]));
        assert_eq!(vorhanden_neu(None, false, &[("x", None)]), HashSet::new());
    }

    /// Jede Profil-Probe und jede Add-on-Quelle wird geprueft — sonst gaelte
    /// sie nie (oder ein Profil wie der CL650 wieder immer).
    #[test]
    fn alle_quellen_werden_geprueft() {
        let liste = zu_pruefende_datarefs();
        for p in crate::profile::PROFILES {
            assert!(
                liste.contains(&p.probe_dataref.split('[').next().unwrap()),
                "{}",
                p.name
            );
        }
        assert!(liste.contains(&"AirbusFBW/OHPLightSwitches"));
        assert!(liste.contains(&"laminar/A333/transponder/ta_ra_knob_pos"));
        assert!(!liste.iter().any(|n| n.contains('[')));
        assert!(!liste.contains(&"sim/cockpit2/switches/strobe_lights_on"));
    }
}

#[cfg(test)]
mod tests {
    use super::desired_profile;

    // The only profile shipped today (index 0) is the Hot-Start CL650.
    const CL650_TITLE: &str = "Challenger 650 published by X-Aviation";

    #[test]
    fn title_match_activates_profile() {
        // Initial discovery: title matches, probe has not answered yet.
        assert_eq!(
            desired_profile(Some(CL650_TITLE), &[false], &[false]),
            Some(0)
        );
    }

    #[test]
    fn probe_activates_profile_without_title() {
        // XP11 / Web API off: title is None but the probe answered.
        assert_eq!(desired_profile(None, &[true], &[true]), Some(0));
    }

    #[test]
    fn no_signal_yields_base_catalog() {
        assert_eq!(desired_profile(None, &[false], &[false]), None);
        assert_eq!(
            desired_profile(Some("Cessna 172"), &[false], &[false]),
            None
        );
    }

    #[test]
    fn probe_wins_when_title_does_not_match() {
        // A title that matches no profile must not veto a fresh probe.
        assert_eq!(
            desired_profile(Some("Cessna 172"), &[true], &[true]),
            Some(0)
        );
    }

    /// QS-R4/P1: the regression the old `last_title`-diff logic missed —
    /// a profile activated purely by probe (title stays `None`), then the
    /// pilot swaps to a non-profile aircraft so the probe falls silent.
    /// With no title signal the swap must STILL reset to the base catalog.
    #[test]
    fn probe_activated_then_stale_resets_to_base() {
        // CL650 loaded, probe fresh, no title → profile active.
        assert_eq!(desired_profile(None, &[true], &[true]), Some(0));
        // Pilot swaps to a non-profile aircraft: probe DataRef vanishes,
        // `probe_last_seen` goes stale → probe_fresh = false, title still
        // None. desired_profile must drop back to the base catalog.
        assert_eq!(desired_profile(None, &[false], &[true]), None);
    }

    /// QS-R2/P2: the laggy Web API title (polled every 30 s) must not
    /// revive a profile the probe already retired. CL650 → non-profile
    /// swap: the probe goes stale within 8 s, but `aircraft.descrip` can
    /// still name the CL650 for up to 30 s. Because the CL650 probe HAS
    /// been seen and is now stale, the stale title must not win.
    #[test]
    fn stale_title_cannot_revive_retired_profile() {
        // probe seen earlier, now stale; title still says CL650 → base.
        assert_eq!(desired_profile(Some(CL650_TITLE), &[false], &[true]), None);
        // sanity: same title, but probe fresh again (swapped back) → active.
        assert_eq!(
            desired_profile(Some(CL650_TITLE), &[true], &[true]),
            Some(0)
        );
    }
}
