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
use crate::plugin2::{Ziel, PLUGIN2_PORT};
use crate::plugin2_ziel::{
    ist_kern, katalog_plan, kennung_ueberlagern, ui_name_ueberlagern, KatalogPlan, P2Stand,
    PluginZugang,
};
use crate::premium::{PremiumListener, PremiumStatus, PremiumTouchdown, PREMIUM_UDP_PORT};
use crate::profile::{build_active_catalog, profile_index_for_title, ActiveEntry, PROFILES};
use crate::rref::{decode_response, encode_request};
use crate::web_api::{AircraftInfo, DrefIdCache, WebApiClient};
use crate::zusatz::{ZusatzAbos, ZUSATZ_HZ};
use crate::{SUBSCRIPTION_HZ, XPLANE_LISTEN_PORT};

/// Wohin der Adapter spricht. Ab Werk die festen X-Plane- und
/// Plugin-Ports; Tests setzen freie Ports, damit sie nicht mit einem
/// laufenden Client oder Simulator kollidieren.
#[derive(Debug, Clone)]
pub struct Anschluesse {
    /// X-Planes RREF-Port (Abos gehen dorthin).
    pub rref: std::net::SocketAddr,
    /// Eigener Port fuer das Plugin (Protokoll 1 kommt hier an, Protokoll 2
    /// wird von hier aus angefragt). 0 = beliebiger freier Port.
    pub plugin_p1: u16,
    /// Steuerport des Plugins (Protokoll 2).
    pub plugin_p2: std::net::SocketAddr,
    /// Basis der X-Plane-Web-API.
    pub web_api: String,
}

impl Default for Anschluesse {
    fn default() -> Self {
        Self {
            rref: std::net::SocketAddr::from(([127, 0, 0, 1], XPLANE_LISTEN_PORT)),
            plugin_p1: PREMIUM_UDP_PORT,
            plugin_p2: std::net::SocketAddr::from(([127, 0, 0, 1], PLUGIN2_PORT)),
            web_api: "http://127.0.0.1:8086".into(),
        }
    }
}

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

pub(crate) struct AdapterShared {
    pub(crate) state: Mutex<ConnectionState>,
    last_error: Mutex<Option<String>>,
    /// Parsed accumulated DataRef state. Mutated from the listener
    /// thread, read by `snapshot()` and `subscribed_datarefs()`.
    pub(crate) parsed: Mutex<XPlaneState>,
    /// Per-index "has X-Plane sent us this DataRef yet?" flag — for
    /// the debug panel.
    pub(crate) seen: Mutex<Vec<bool>>,
    /// Per-index last raw float value (for debug panel display).
    pub(crate) last_values: Mutex<Vec<f32>>,
    /// v0.12.2 (LE6): the **active catalog** the listener is currently
    /// subscribed to — the static `CATALOG` with the detected aircraft
    /// profile's dataref overrides applied. Same length/indices as
    /// `CATALOG`. The listener owns the working copy and publishes it
    /// here so the debug panel shows the dataref names actually in use.
    pub(crate) active_catalog: Mutex<Vec<ActiveEntry>>,
    /// Aircraft identity from the X-Plane 12.1+ Web API. Empty
    /// `AircraftInfo` (all fields None) until the poller's first
    /// successful response, OR forever if the Web API is unreachable
    /// (X-Plane <12.1, or pilot didn't enable Settings → Network →
    /// Web Server).
    aircraft: Mutex<AircraftInfo>,
    /// Zusatzwerte des Telemetrie-Monitors (v1.8), eigene RREF-Abos ab
    /// `ZUSATZ_INDEX_BASE`. Leer, solange der Monitor zu ist.
    pub(crate) zusatz: Mutex<ZusatzAbos>,
    /// Add-on-Datarefs und Profil-Proben, die das geladene Flugzeug laut
    /// Web-API wirklich hat (Name ohne „[n]"). `None`, solange die Web-API
    /// nichts bestaetigt hat (X-Plane 11, Web-API aus, erste Sekunden) —
    /// dann gelten nur die Standardwerte. Grund: RREF liefert fuer JEDEN
    /// Namen Werte, auch fuer fehlende (gemessen 27.09.2026).
    pub(crate) addon_vorhanden: Mutex<Option<HashSet<String>>>,
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
    // ---- Plugin, Protokoll 2 (AP7, siehe `plugin2_ziel.rs`) ----
    /// Sitzungsstand: Status je Name, Profil, Flugzeugmeldung, Messung.
    /// Sperrfolge: `p2` vor `parsed`/`seen`/`last_values`/`zusatz`/
    /// `active_catalog`, nie umgekehrt.
    pub(crate) p2: Mutex<P2Stand>,
    /// Das Katalog-Abo (Abo 1), einmal gebaut.
    pub(crate) p2_plan: KatalogPlan,
    /// Bezugspunkt fuer `p2_katalog_ms`.
    pub(crate) p2_basis: Instant,
    /// Zeitpunkt (ms seit `p2_basis`) der letzten Katalogwerte des Plugins,
    /// 0 = keine. Solange frisch, ruht RREF.
    pub(crate) p2_katalog_ms: AtomicU64,
    /// Besteht gerade eine Protokoll-2-Sitzung?
    pub(crate) p2_sitzung: AtomicBool,
    /// Steigt, wenn sich die gewuenschten Abos aendern (Monitor, Messung).
    pub(crate) p2_wunsch_gen: AtomicU64,
    /// Einmalige Anfragen an das Plugin (`LISTE`).
    pub(crate) p2_anfragen: Mutex<Vec<Vec<u8>>>,
    /// Uebergabeplatz des HUD-Bands (ADR-0005), von der App gefuellt.
    pub(crate) band: Arc<crate::hud_band::BandSlot>,
    /// Basis der Web-API (fuer den Rueckfall der Vermessung).
    pub(crate) web_api: String,
}

pub struct XPlaneAdapter {
    shared: Arc<AdapterShared>,
    anschluesse: Anschluesse,
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
        Self::mit_anschluessen(Anschluesse::default())
    }

    /// Adapter mit eigenen Ports (Tests, siehe [`Anschluesse`]).
    pub fn mit_anschluessen(anschluesse: Anschluesse) -> Self {
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
            p2: Mutex::new(P2Stand::default()),
            p2_plan: katalog_plan(),
            p2_basis: Instant::now(),
            p2_katalog_ms: AtomicU64::new(0),
            p2_sitzung: AtomicBool::new(false),
            p2_wunsch_gen: AtomicU64::new(0),
            p2_anfragen: Mutex::new(Vec::new()),
            band: Arc::new(crate::hud_band::BandSlot::default()),
            web_api: anschluesse.web_api.clone(),
        });
        Self {
            shared,
            anschluesse,
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
        *self.shared.p2.lock() = P2Stand::default();
        self.shared.p2_katalog_ms.store(0, Ordering::SeqCst);
        self.shared.p2_sitzung.store(false, Ordering::SeqCst);
        self.shared.p2_anfragen.lock().clear();
        self.shared.stop.store(false, Ordering::SeqCst);
        let shared_for_udp = Arc::clone(&self.shared);
        let rref_ziel = self.anschluesse.rref;
        let udp_handle = std::thread::Builder::new()
            .name("xplane-udp".into())
            .spawn(move || run_listener(shared_for_udp, rref_ziel))
            .expect("spawn xplane-udp thread");
        self.worker = Some(udp_handle);
        let shared_for_web = Arc::clone(&self.shared);
        let lauf = {
            let _w = self.shared.web_schreiben.lock();
            self.shared.web_lauf.fetch_add(1, Ordering::SeqCst) + 1
        };
        let web_basis = self.anschluesse.web_api.clone();
        let web_handle = std::thread::Builder::new()
            .name("xplane-web-api".into())
            .spawn(move || run_web_api_poller(shared_for_web, lauf, web_basis))
            .expect("spawn xplane-web-api thread");
        self.web_api_worker = Some(web_handle);
        // Start the premium plugin listener too. No-op unless the
        // optional X-Plane Plugin is installed — see `premium.rs`.
        // Ab Plugin 1.0 fuehrt derselbe Faden die Protokoll-2-Sitzung;
        // Ziel der Werte ist dieser Adapter.
        let ziel: Arc<dyn Ziel> = Arc::clone(&self.shared) as Arc<dyn Ziel>;
        self.premium.start_mit(
            self.anschluesse.plugin_p1,
            self.anschluesse.plugin_p2,
            Some(ziel),
        );
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
        let mut s = self.premium.status();
        let st = self.shared.p2.lock();
        if st.offen {
            s.namen_da = st.status.iter().filter(|x| **x == Some(true)).count() as u32;
            s.namen_fehlen = st.status.iter().filter(|x| **x == Some(false)).count() as u32;
        }
        s
    }

    /// Uebergabeplatz des HUD-Bands (ADR-0005): die App legt dort im Takt das
    /// fertige Band ab; die Plugin-Sitzung sendet es. Bleibt ueber
    /// `start()`/`stop()` derselbe.
    pub fn band_slot(&self) -> Arc<crate::hud_band::BandSlot> {
        Arc::clone(&self.shared.band)
    }

    /// Zugang zur Plugin-Sitzung fuer „Flugzeug vermessen" — `None`, wenn
    /// gerade keine Protokoll-2-Sitzung besteht (dann Web-API).
    pub fn plugin_zugang(&self) -> Option<PluginZugang> {
        self.shared.p2.lock().offen.then(|| PluginZugang {
            shared: Arc::clone(&self.shared),
        })
    }

    /// Letzte Flugzeugkennung des Web-API-Pollers (leer, bis er einmal
    /// geantwortet hat). Rueckfall fuer „Flugzeug vermessen", wenn die
    /// eigene Abfrage der Messung nichts liefert (05.10.2026, FF777).
    pub fn flugzeug(&self) -> AircraftInfo {
        self.shared.aircraft.lock().clone()
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
        // Sperrfolge: `p2` nie waehrend `parsed` gehalten (siehe AdapterShared).
        let plugin_flugzeug = self.shared.p2_flugzeug();
        // Ein Profil mit eigener Klappenquelle (CL650, MD-11) schreibt seine
        // eigene Skala in den Klappenhebel — die Rasten der Flugzeugdatei
        // passen dazu nicht. Vor `parsed` gelesen, nie beide zugleich.
        let klappen_aus_profil = klappen_aus_profil(&self.shared.active_catalog.lock());
        let parsed = self.shared.parsed.lock();
        if !parsed.got_first_packet {
            return None;
        }
        let sim = match self.kind {
            SimKind::XPlane11 => Simulator::XPlane11,
            _ => Simulator::XPlane12,
        };
        let mut snap = parsed.to_snapshot(sim);
        drop(parsed);
        if klappen_aus_profil {
            snap.flap_handle_index = None;
            snap.flap_num_positions = None;
        }
        // Overlay aircraft identity from the Web API poller (X-Plane
        // 12.1+ Settings → Network → Web Server). Stays None until the
        // first successful poll, OR forever when the Web API isn't
        // reachable (X-Plane <12.1, or pilot didn't enable it). The
        // SimSnapshot fields default to None in that path so the
        // existing "(unknown)" UI label still shows.
        //
        // Protokoll 2: die Flugzeugmeldung des Plugins hat Vorrang (frisch
        // bei jedem Wechsel); die Web-API fuellt nur, was sie nicht hat.
        let aircraft = self.shared.aircraft.lock();
        let (titel, icao, kennz) = kennung_ueberlagern(plugin_flugzeug.as_ref(), &aircraft);
        if titel.is_some() {
            snap.aircraft_title = titel;
        }
        // Nur fuer „Flugzeug vermessen“: derselbe Titel, den die Messung
        // traegt (acf_ui_name) — der Titel oben bleibt acf_descrip.
        snap.aircraft_ui_name = ui_name_ueberlagern(plugin_flugzeug.as_ref(), &aircraft);
        if icao.is_some() {
            snap.aircraft_icao = icao;
        }
        if kennz.is_some() {
            snap.aircraft_registration = kennz;
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
        if self.shared.zusatz.lock().setzen(felder) {
            // Protokoll 2: Abo 2 neu abgleichen.
            self.shared.p2_wunsch_gen.fetch_add(1, Ordering::SeqCst);
        }
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

/// Liest der aktive Katalog den Klappenhebel aus einer Profil-Quelle statt
/// aus `flap_handle_request_ratio`? Dann gibt es keine Rastenangabe.
pub(crate) fn klappen_aus_profil(aktiv: &[ActiveEntry]) -> bool {
    aktiv.iter().any(|e| {
        e.field == crate::dataref::FieldId::FlapsHandle
            && CATALOG
                .iter()
                .any(|c| c.field == e.field && c.name != e.name)
    })
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
/// Ein Profil mit `titel_und_probe` gilt nur, wenn auch der Titel passt.
pub(crate) fn desired_profile(
    title: Option<&str>,
    probe_fresh: &[bool],
    probe_seen: &[bool],
) -> Option<usize> {
    let titel_passt = |pi: usize| {
        PROFILES
            .get(pi)
            .is_some_and(|p| !p.titel_und_probe || title.is_some_and(|t| p.matches_title(t)))
    };
    if let Some(pi) = probe_fresh
        .iter()
        .enumerate()
        .position(|(pi, &fresh)| fresh && titel_passt(pi))
    {
        return Some(pi);
    }
    title
        .and_then(profile_index_for_title)
        .filter(|&pi| !probe_seen.get(pi).copied().unwrap_or(false))
}

/// Profilwahl im RREF-Pfad. Wie [`desired_profile`], nur: Kennt der RREF-Pfad
/// keinen Titel (Web-API aus), ist das „unbekannt", nicht „passt nicht" —
/// ein aktives Profil bleibt, solange seine Probe frisch ist. Sonst verloere
/// die FF777 (`titel_und_probe`) beim Rueckfall vom Plugin ihr Profil, das
/// das Plugin mit seinem Titel gesetzt hatte (Codex 05.10.2026).
///
/// `probe_traegt`: frisch UND zuletzt ≠ 0. RREF antwortet auch fuer
/// fehlende Datarefs mit 0 — „frisch" allein hielte das Profil nach einem
/// Flugzeugwechsel fest (Codex-Nachpruefung).
pub(crate) fn rref_profil(
    title: Option<&str>,
    aktiv: Option<usize>,
    probe_fresh: &[bool],
    probe_seen: &[bool],
    probe_traegt: &[bool],
) -> Option<usize> {
    match desired_profile(title, probe_fresh, probe_seen) {
        None if title.is_none() => {
            aktiv.filter(|&pi| probe_traegt.get(pi).copied().unwrap_or(false))
        }
        d => d,
    }
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
pub(crate) fn grundname(name: &str) -> &str {
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
///
/// AP7: Liefert das Plugin (Protokoll 2) Katalogwerte, ruht dieser Faden —
/// Abos abbestellt, RREF-Pakete verworfen — und uebernimmt nahtlos wieder,
/// sobald sie versiegen (siehe `plugin2_ziel.rs`, „Uebergabe").
fn run_listener(shared: Arc<AdapterShared>, xplane_addr: std::net::SocketAddr) {
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
    // Letzter Probenwert ungleich 0? RREF antwortet auch fuer fehlende
    // Datarefs (mit 0) — nur ein Wert ≠ 0 traegt `rref_profil`s Halten
    // ohne Titel (Codex-Nachpruefung 05.10.2026).
    let mut probe_nicht_null: Vec<bool> = vec![false; PROFILES.len()];
    // Stand der bestaetigten Add-on-Quellen, den dieser Thread zuletzt
    // uebernommen hat (siehe `AdapterShared::addon_vorhanden`).
    let mut addon_gen_gesehen: u64 = u64::MAX;
    let mut vorhanden: Option<HashSet<String>> = None;
    // `vorhanden` stammt aus der Plugin-Sitzung (Rueckfall), nicht von der
    // Web-API — gilt nur fuer dieses Flugzeug (siehe Leergewicht).
    let mut vorhanden_aus_plugin = false;
    // Das Plugin (Protokoll 2) liefert — RREF ruht.
    let mut rref_ruht = false;
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
            if let Err(e) = sock.send_to(&req, xplane_addr) {
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
            let _ = sock.send_to(&req, xplane_addr);
        }
    };
    // Cancel all probe subscriptions (freq = 0) — best-effort cleanup
    // on listener shutdown. The probes otherwise stay subscribed for the
    // whole session (QS-R4/P1) so an aircraft swap is always detected.
    let unsubscribe_probes = |sock: &UdpSocket| {
        for (pi, prof) in PROFILES.iter().enumerate() {
            let stop = encode_request(0, DISCOVERY_INDEX_BASE + pi as i32, prof.probe_dataref);
            let _ = sock.send_to(&stop, xplane_addr);
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
            let _ = sock.send_to(&req, xplane_addr);
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
                // Plugin liefert: RREF-Nachzuegler (X-Plane sendet nach dem
                // Abbestellen noch kurz) nicht schreiben — nie zwei Quellen
                // fuer ein Feld.
                if rref_ruht || shared.p2_frisch() {
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
                        if let Some(nn) = probe_nicht_null.get_mut(pi) {
                            *nn = p.value != 0.0;
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
                                    probe_nicht_null.fill(false);
                                    // Vom Plugin uebernommene Quellen galten fuer
                                    // das alte Flugzeug — verwerfen.
                                    if vorhanden_aus_plugin {
                                        vorhanden = None;
                                        vorhanden_aus_plugin = false;
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

        // ---- AP7: Uebergabe an das Plugin (Protokoll 2) und zurueck ----
        let plugin_liefert = shared.p2_frisch();
        if plugin_liefert && !rref_ruht {
            // Plugin uebernimmt: RREF-Abos abbestellen (freq 0) und die
            // Quellen, die hier galten, als Beleg weiterreichen (Kern-Namen
            // `laminar/…`, siehe `plugin2_ziel::ist_kern`).
            let vorab: HashSet<String> = CATALOG
                .iter()
                .filter(|e| {
                    addon_quelle(e.field)
                        && eintrag_gilt(e.field, e.name, vorhanden.as_ref(), &ungleich_null)
                })
                .map(|e| grundname(e.name).to_string())
                .chain(
                    PROFILES
                        .iter()
                        .filter(|p| probe_gilt(p.probe_dataref, vorhanden.as_ref(), &ungleich_null))
                        .map(|p| grundname(p.probe_dataref).to_string()),
                )
                .collect();
            shared.p2_vorab_setzen(vorab);
            for (i, entry) in active.iter().enumerate() {
                let req = encode_request(0, i as i32, entry.name);
                let _ = socket.send_to(&req, xplane_addr);
            }
            unsubscribe_probes(&socket);
            zusatz_senden(&socket, &zusatz_abonniert, 0);
            zusatz_abonniert.clear();
            last_packet_at = None;
            rref_ruht = true;
            tracing::info!("X-Plane: Plugin (Protokoll 2) liefert — RREF-Abos abbestellt");
        } else if !plugin_liefert && rref_ruht {
            // Rueckfall: Profil und geltende Quellen aus der Plugin-Sitzung
            // uebernehmen und sofort neu abonnieren. Die Felder behalten bis
            // zum ersten RREF-Wert den letzten Plugin-Wert — nichts faellt
            // auf 0. Der Stale-Waechter zaehlt ab dem letzten Plugin-Wert.
            let (profil, gilt) = shared.p2_uebergabe();
            active_profile = profil;
            active = build_active_catalog(profil.map(|pi| &PROFILES[pi]));
            *shared.active_catalog.lock() = active.clone();
            for t in probe_last_seen.iter_mut() {
                *t = None;
            }
            probe_nicht_null.fill(false);
            if let Some(pi) = profil {
                if let Some(t) = probe_last_seen.get_mut(pi) {
                    *t = Some(Instant::now());
                }
                // Das Plugin hatte die Probe bestaetigt; der erste RREF-Wert
                // ueberschreibt das (0 → Profil faellt weg).
                if let Some(nn) = probe_nicht_null.get_mut(pi) {
                    *nn = true;
                }
            }
            vorhanden = Some(gilt);
            vorhanden_aus_plugin = true;
            addon_gen_gesehen = shared.addon_generation.load(Ordering::SeqCst);
            ungleich_null.clear();
            last_packet_at = Some(shared.p2_letzte_werte().unwrap_or_else(Instant::now));
            subscribe_catalog(&socket, &active);
            subscribe_probes(&socket);
            let (gen_jetzt, abos_jetzt) = {
                let z = shared.zusatz.lock();
                (z.generation, z.abos())
            };
            zusatz_senden(&socket, &abos_jetzt, ZUSATZ_HZ);
            zusatz_abonniert = abos_jetzt;
            zusatz_generation = gen_jetzt;
            last_resubscribe_at = Instant::now();
            rref_ruht = false;
            tracing::info!(
                profil = ?profil.map(|pi| PROFILES[pi].name),
                "X-Plane: Plugin liefert nicht mehr — zurueck auf RREF"
            );
        }
        if rref_ruht {
            // Profil, Quellen, Zusatzwerte und Verbindung fuehrt das Plugin.
            continue;
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
            vorhanden_aus_plugin = false;
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
        let probe_traegt: Vec<bool> = probe_fresh
            .iter()
            .zip(&probe_nicht_null)
            .map(|(&f, &nn)| f && nn)
            .collect();
        let desired = rref_profil(
            current_title.as_deref(),
            active_profile,
            &probe_fresh,
            &probe_seen,
            &probe_traegt,
        );

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
        let _ = socket.send_to(&req, xplane_addr);
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
fn run_web_api_poller(shared: Arc<AdapterShared>, lauf: u64, basis: String) {
    let client = WebApiClient::mit_basis(&basis);
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
                // Liefert das Plugin (Protokoll 2), meldet es die Existenz
                // selbst — geprueft werden nur noch die Kern-Namen
                // (`laminar/…`), bei denen die Registrierung allein nichts
                // beweist (siehe `plugin2_ziel::ist_kern`).
                let plugin = shared.p2_frisch();
                for n in zu_pruefende_datarefs()
                    .into_iter()
                    .filter(|n| !plugin || ist_kern(n))
                {
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
                // Nur Kern-Namen geprueft, aber das Plugin liefert inzwischen
                // nicht mehr: dieser halbe Stand taugt fuer RREF nicht (er
                // striche dort ToLiss & Co.) — verwerfen und gleich
                // vollstaendig pruefen.
                let halb = plugin && !shared.p2_frisch();
                if halb {
                    sofort = true;
                } else if noch_dasselbe {
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
mod klappen_profil_tests {
    use super::*;

    /// Profile mit eigener Klappenquelle (CL650, MD-11) verlieren die
    /// Rastenangabe der Flugzeugdatei; ohne Profil und beim 737-Profil
    /// (nur AP-Modi ersetzt) bleibt sie. Geprueft ueber `snapshot()`, den
    /// Weg, den die App nimmt.
    #[test]
    fn rasten_nur_mit_dem_standard_hebel() {
        let ad = XPlaneAdapter::default();
        {
            let mut p = ad.shared.parsed.lock();
            p.apply_field(crate::dataref::FieldId::FlapDetents, 4.0);
            p.apply_field(crate::dataref::FieldId::FlapsHandle, 0.5);
        }
        let raste = |ad: &XPlaneAdapter| {
            let s = ad.snapshot().expect("Schnappschuss");
            (s.flap_handle_index, s.flap_num_positions)
        };
        assert_eq!(raste(&ad), (Some(2), Some(4)), "ohne Profil");
        let mut geprueft_eigen = 0;
        for p in PROFILES {
            *ad.shared.active_catalog.lock() = build_active_catalog(Some(p));
            let eigen = p
                .overrides
                .iter()
                .any(|o| o.field == crate::dataref::FieldId::FlapsHandle);
            let soll = if eigen {
                geprueft_eigen += 1;
                (None, None)
            } else {
                (Some(2), Some(4))
            };
            assert_eq!(raste(&ad), soll, "{}", p.name);
        }
        assert!(geprueft_eigen >= 2, "CL650 und MD-11");
        *ad.shared.active_catalog.lock() = build_active_catalog(None);
        assert_eq!(raste(&ad), (Some(2), Some(4)), "Profil wieder weg");
    }
}

#[cfg(test)]
mod tests {
    use super::{desired_profile, rref_profil};

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

    /// 05.10.2026: die FF777 braucht Titel UND Probe — `1-sim/cduL/ok`
    /// haben alle FlightFactor-Muster, „777“ im Titel auch die freie
    /// Stratosphere-777.
    #[test]
    fn ff777_braucht_titel_und_probe() {
        let ff = crate::profile::PROFILES
            .iter()
            .position(|p| p.name == "FlightFactor 777")
            .expect("FF777-Profil vorhanden");
        let n = crate::profile::PROFILES.len();
        let nur = |i: usize| {
            let mut v = vec![false; n];
            v[i] = true;
            v
        };
        let keine = vec![false; n];
        // Michels Titel (acf_descrip im Log NWS419) + Probe → Profil.
        assert_eq!(
            desired_profile(Some("Boeing 777-300ER"), &nur(ff), &nur(ff)),
            Some(ff)
        );
        // Probe ohne passenden Titel (anderes FlightFactor-Muster) → nichts.
        assert_eq!(
            desired_profile(Some("Airbus A350-900"), &nur(ff), &nur(ff)),
            None
        );
        // Probe ohne Titel (Web-API aus) → nichts.
        assert_eq!(desired_profile(None, &nur(ff), &nur(ff)), None);
        // Titel ohne Probe (Stratosphere-777) → nichts.
        assert_eq!(
            desired_profile(Some("Boeing 777-300ER"), &keine, &keine),
            None
        );
    }

    /// Codex 05.10.2026: Rueckfall vom Plugin auf RREF ohne Web-API — das
    /// vom Plugin gesetzte FF777-Profil bleibt, solange die Probe frisch ist.
    #[test]
    fn ff777_bleibt_beim_rueckfall_ohne_titel() {
        let ff = crate::profile::PROFILES
            .iter()
            .position(|p| p.name == "FlightFactor 777")
            .unwrap();
        let n = crate::profile::PROFILES.len();
        let mut frisch = vec![false; n];
        frisch[ff] = true;
        let keine = vec![false; n];
        // Kein Titel, Profil aktiv, Probe frisch und ≠ 0 → bleibt.
        assert_eq!(
            rref_profil(None, Some(ff), &frisch, &frisch, &frisch),
            Some(ff)
        );
        // Probe frisch, aber 0 (RREF fuer fehlenden Dataref nach einem
        // Flugzeugwechsel) → Basis-Katalog (Codex-Nachpruefung).
        assert_eq!(rref_profil(None, Some(ff), &frisch, &frisch, &keine), None);
        // Probe verstummt → Basis-Katalog.
        assert_eq!(rref_profil(None, Some(ff), &keine, &frisch, &keine), None);
        // Bekannter, fremder Titel → weg, auch mit tragender Probe.
        assert_eq!(
            rref_profil(Some("Airbus A350-900"), Some(ff), &frisch, &frisch, &frisch),
            None
        );
        // Ohne aktives Profil setzt eine Probe allein es nicht.
        assert_eq!(rref_profil(None, None, &frisch, &frisch, &frisch), None);
    }

    /// Ganze Kette mit Michels Werten: aktiver FF777-Katalog → Hebel 0.5
    /// ist ARMED und eingefahren, 1.0 ausgefahren und nicht ARMED.
    #[test]
    fn ff777_hebel_halb_ist_armed() {
        use crate::dataref::{FieldId, XPlaneState};
        use sim_core::Simulator;
        let ff = crate::profile::PROFILES
            .iter()
            .find(|p| p.name == "FlightFactor 777")
            .unwrap();
        let aktiv = crate::profile::build_active_catalog(Some(ff));
        let hebel = aktiv
            .iter()
            .find(|e| e.field == FieldId::SpoilersHandle)
            .unwrap();
        let snap = |roh: f32| {
            let mut s = XPlaneState::default();
            s.apply_field(FieldId::SpoilersHandle, hebel.mapping.map(roh).unwrap());
            s.to_snapshot(Simulator::XPlane12)
        };
        assert_eq!(snap(0.5).spoilers_armed, Some(true));
        assert_eq!(snap(0.5).spoilers_handle_position, Some(0.0));
        assert_eq!(snap(1.0).spoilers_armed, Some(false));
        assert_eq!(snap(1.0).spoilers_handle_position, Some(1.0));
        assert_eq!(snap(0.0).spoilers_armed, Some(false));
        // Hebel auf dem Weg durch die Mitte (Log: 0.8/0.9) bleibt Stellung.
        assert_eq!(snap(0.45).spoilers_armed, Some(false));
        // Gegenprobe: ohne Profil ist 0.5 halb ausgefahren, nicht ARMED.
        let mut s = XPlaneState::default();
        s.apply_field(FieldId::SpoilersHandle, 0.5);
        assert_eq!(
            s.to_snapshot(Simulator::XPlane12).spoilers_armed,
            Some(false)
        );
    }
}

/// Echter Loopback-Test mit einem kleinen Schein-Plugin (AP7): Sitzung auf,
/// Werte kommen an (doppelt genau), fehlender Name bleibt leer, RREF ruht,
/// Vermessung ueber das Plugin, Sitzung bricht ab → Rueckfall auf RREF ohne
/// Nullen. Alle Ports frei gewaehlt — kollidiert nicht mit einem laufenden
/// Client oder X-Plane.
#[cfg(test)]
mod plugin2_loopback_tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::{SocketAddr, UdpSocket};

    const BREITE: f64 = 51.234_567_890_1;
    const LAENGE: f64 = 8.543_210_987_6;

    #[derive(Default)]
    struct FakeStand {
        /// Auf HALLO antworten und Werte liefern?
        liefern: bool,
        /// Langsames Plugin: Mess-Abos (ID ≥ 3) bekommen nie einen Status.
        mess_stumm: bool,
        /// Zibo 737-800X statt A320: Flugzeugmeldung und CMD-A-Lampe an,
        /// `servos_on` bleibt 0 (wie im echten Flug, 30.09.2026).
        zibo: bool,
        /// Zibo: nur CMD B an (Copilot fliegt), CMD A aus.
        zibo_nur_cmd_b: bool,
        client: Option<SocketAddr>,
        /// Vollstaendige Abos: ID → (Generation, Namen).
        abos: HashMap<u8, (u32, Vec<String>)>,
        /// Teile unvollstaendiger Abos: ID → (Teil → Namen).
        teile: HashMap<u8, HashMap<u32, Vec<String>>>,
        anfragen: Vec<String>,
    }

    /// Status wie ein echtes Plugin: Profil-Proben/Ersetzungen fremder
    /// Flugzeuge und der ToLiss-AP1 fehlen, Position ist `double`.
    fn fake_status(name: &str) -> serde_json::Value {
        if name.starts_with("abus/")
            || name.starts_with("CL650/")
            || name.starts_with("Rotate/")
            || name == "AirbusFBW/AP1Engage"
        {
            serde_json::json!("fehlt")
        } else if name.ends_with("/latitude") || name.ends_with("/longitude") {
            serde_json::json!("d")
        } else if name == "sim/test/text" {
            serde_json::json!("b")
        } else if name == "sim/test/feld" {
            serde_json::json!("vf")
        } else {
            serde_json::json!("f")
        }
    }

    fn fake_wert(name: &str, zibo: bool, nur_b: bool) -> Option<serde_json::Value> {
        if zibo {
            match name {
                "laminar/B738/autopilot/cmd_a_status" => {
                    return Some(serde_json::json!(if nur_b { 0 } else { 1 }))
                }
                "laminar/B738/autopilot/cmd_b_status" => {
                    return Some(serde_json::json!(if nur_b { 1 } else { 0 }))
                }
                "sim/cockpit2/autopilot/servos_on" => return Some(serde_json::json!(0)),
                // Zibo-Audit 01.10.2026: Schalter, die der Zibo selbst traegt.
                "laminar/B738/toggle_switch/taxi_light_brightness_pos" => {
                    return Some(serde_json::json!(2))
                }
                "laminar/B738/toggle_switch/capt_probes_pos"
                | "laminar/B738/toggle_switch/logo_light"
                | "laminar/B738/ice/eng1_heat_pos"
                | "laminar/B738/annunciator/master_caution_light"
                | "laminar/B738/autopilot/autothrottle_status" => {
                    return Some(serde_json::json!(1))
                }
                "sim/cockpit2/switches/taxi_light_on"
                | "sim/cockpit2/ice/ice_pitot_heat_on_pilot" => return Some(serde_json::json!(0)),
                _ => {}
            }
        }
        Some(match name {
            "sim/flightmodel/position/latitude" => serde_json::json!(BREITE),
            "sim/flightmodel/position/longitude" => serde_json::json!(LAENGE),
            "sim/flightmodel/position/y_agl" => serde_json::json!(0.5),
            "AirbusFBW/APUMaster" => serde_json::json!(1),
            // Kern-Name mit Wert 0: „da", aber ohne zweiten Beleg.
            "laminar/B738/knob/transponder_pos" => serde_json::json!(0),
            "sim/test/eins" => serde_json::json!(1.5),
            "sim/test/zwei" => serde_json::json!(2.5),
            "sim/test/text" => serde_json::json!("A20N"),
            "sim/test/feld" => serde_json::json!([1.0, 2.0, 3.0]),
            _ => return None,
        })
    }

    fn senden(sock: &UdpSocket, an: SocketAddr, v: serde_json::Value) {
        let _ = sock.send_to(format!("{v}\n").as_bytes(), an);
    }

    fn status_senden(sock: &UdpSocket, an: SocketAddr, abo: u8, gen: u32, namen: &[String]) {
        let st: Vec<serde_json::Value> = namen
            .iter()
            .enumerate()
            .map(|(i, n)| match fake_status(n) {
                serde_json::Value::String(s) if s == "fehlt" => serde_json::json!([i, "fehlt"]),
                t => serde_json::json!([i, t, 1]),
            })
            .collect();
        let teile: Vec<&[serde_json::Value]> = st.chunks(150).collect();
        for (k, t) in teile.iter().enumerate() {
            senden(
                sock,
                an,
                serde_json::json!({"p":2,"t":"abo","abo":abo,"gen":gen,"teil":k+1,"teile":teile.len(),"st":t}),
            );
        }
    }

    /// Schein-Plugin: beantwortet HALLO/ABO/LISTE, liefert alle 50 ms Werte.
    fn fake_plugin(
        sock: UdpSocket,
        stand: Arc<Mutex<FakeStand>>,
        stop: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        sock.set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 65536];
            let mut zuletzt = Instant::now();
            while !stop.load(Ordering::SeqCst) {
                if let Ok((n, von)) = sock.recv_from(&mut buf) {
                    let text = String::from_utf8_lossy(&buf[..n]).to_string();
                    let mut zeilen = text.lines();
                    let kopf = zeilen.next().unwrap_or("").to_string();
                    let mut st = stand.lock();
                    st.anfragen.push(kopf.clone());
                    let mut teile: Vec<&str> = kopf.split(' ').collect();
                    // Generation `g<zahl>` als letztes Wort der ABO-Kopfzeile.
                    let gen: u32 = match teile.last() {
                        Some(g) if teile[0] == "ABO" && g.starts_with('g') => {
                            let g = g[1..].parse().unwrap();
                            teile.pop();
                            g
                        }
                        _ => 0,
                    };
                    match teile.first().copied() {
                        Some("HALLO") if st.liefern => {
                            st.client = Some(von);
                            senden(
                                &sock,
                                von,
                                serde_json::json!({"p":2,"t":"hallo","plugin":"1.0.0","xplane":12100,"xplm":430}),
                            );
                            senden(
                                &sock,
                                von,
                                if st.zibo {
                                    serde_json::json!({"p":2,"t":"flugzeug","icao":"B738","titel":"Boeing 737-800X","ui_name":"Boeing 737-800X (4k)","pfad":"Aircraft/B737-800X/b738_4k.acf"})
                                } else {
                                    serde_json::json!({"p":2,"t":"flugzeug","icao":"A20N","titel":"A320neo Test","pfad":"Aircraft/Test/a320.acf"})
                                },
                            );
                        }
                        Some("ABO") if st.liefern => {
                            let id: u8 = teile[1].parse().unwrap();
                            let (teil, anzahl) = if teile.len() == 5 {
                                (teile[3].parse().unwrap(), teile[4].parse().unwrap())
                            } else {
                                (1u32, 1u32)
                            };
                            let namen: Vec<String> = zeilen.map(str::to_string).collect();
                            let t = st.teile.entry(id).or_default();
                            t.insert(teil, namen);
                            if t.len() as u32 == anzahl {
                                let mut alle = Vec::new();
                                for k in 1..=anzahl {
                                    alle.extend(t.remove(&k).unwrap());
                                }
                                st.teile.remove(&id);
                                if id < 3 || !st.mess_stumm {
                                    // Wie das Plugin ab dem Cloud-QS-Stand: erst
                                    // die Annahme, dann der Status.
                                    senden(
                                        &sock,
                                        von,
                                        serde_json::json!({"p":2,"t":"abo_empfangen","abo":id,"gen":gen,"namen":alle.len()}),
                                    );
                                    status_senden(&sock, von, id, gen, &alle);
                                    st.abos.insert(id, (gen, alle));
                                }
                            }
                        }
                        Some("ENDE-ABO") => {
                            let id: u8 = teile[1].parse().unwrap();
                            st.abos.remove(&id);
                        }
                        Some("LISTE") if st.liefern => {
                            let id: u32 = teile[1].parse().unwrap();
                            // Absichtlich in umgekehrter Reihenfolge.
                            senden(
                                &sock,
                                von,
                                serde_json::json!({"p":2,"t":"liste","id":id,"teil":2,"teile":2,"n":["sim/test/feld"]}),
                            );
                            senden(
                                &sock,
                                von,
                                serde_json::json!({"p":2,"t":"liste","id":id,"teil":1,"teile":2,"n":["sim/test/eins","sim/test/zwei","sim/test/text"]}),
                            );
                        }
                        _ => {}
                    }
                }
                if zuletzt.elapsed() >= Duration::from_millis(50) {
                    zuletzt = Instant::now();
                    let st = stand.lock();
                    if let (true, Some(an)) = (st.liefern, st.client) {
                        for (id, (gen, namen)) in &st.abos {
                            let v: Vec<serde_json::Value> = namen
                                .iter()
                                .enumerate()
                                .filter(|(_, n)| fake_status(n) != serde_json::json!("fehlt"))
                                .filter_map(|(i, n)| {
                                    fake_wert(n, st.zibo, st.zibo_nur_cmd_b)
                                        .map(|w| serde_json::json!([i, w]))
                                })
                                .collect();
                            if !v.is_empty() {
                                senden(
                                    &sock,
                                    an,
                                    serde_json::json!({"p":2,"t":"w","abo":id,"gen":gen,"seq":1,"teil":1,"teile":1,"v":v}),
                                );
                            }
                        }
                    }
                }
            }
        })
    }

    /// Schein-X-Plane fuer RREF: nimmt Abos an, antwortet nie. Merkt sich
    /// die Frequenz jeder Anfrage.
    fn fake_rref(
        sock: UdpSocket,
        freqs: Arc<Mutex<Vec<i32>>>,
        stop: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        sock.set_read_timeout(Some(Duration::from_millis(10)))
            .unwrap();
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 1024];
            while !stop.load(Ordering::SeqCst) {
                if let Ok((n, _)) = sock.recv_from(&mut buf) {
                    if n == crate::rref::RREF_REQUEST_SIZE && &buf[0..4] == b"RREF" {
                        let f = i32::from_le_bytes([buf[5], buf[6], buf[7], buf[8]]);
                        freqs.lock().push(f);
                    }
                }
            }
        })
    }

    fn warte(bis: Duration, mut ok: impl FnMut() -> bool) -> bool {
        let ende = Instant::now() + bis;
        while Instant::now() < ende {
            if ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        ok()
    }

    /// Zibo 737-800X (01.10.2026, Michel THY 372): der Autopilot kommt aus
    /// der CMD-A-Lampe des Zibo, nicht aus `servos_on` (das der Zibo nie
    /// setzt). Ganze Kette: Schein-Plugin → Profil → Snapshot.
    #[test]
    fn zibo_autopilot_aus_der_cmd_a_lampe() {
        zibo_autopilot_lauf(false);
    }

    /// CMD B allein (Copilot fliegt): ebenfalls Autopilot an.
    #[test]
    fn zibo_autopilot_nur_cmd_b() {
        zibo_autopilot_lauf(true);
    }

    fn zibo_autopilot_lauf(nur_b: bool) {
        let stop = Arc::new(AtomicBool::new(false));
        let plugin_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rref_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let web_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let anschluesse = Anschluesse {
            rref: rref_sock.local_addr().unwrap(),
            plugin_p1: 0,
            plugin_p2: plugin_sock.local_addr().unwrap(),
            web_api: format!("http://127.0.0.1:{web_port}"),
        };
        let stand = Arc::new(Mutex::new(FakeStand {
            liefern: true,
            zibo: true,
            zibo_nur_cmd_b: nur_b,
            ..FakeStand::default()
        }));
        let freqs = Arc::new(Mutex::new(Vec::new()));
        let f1 = fake_plugin(plugin_sock, Arc::clone(&stand), Arc::clone(&stop));
        let f2 = fake_rref(rref_sock, Arc::clone(&freqs), Arc::clone(&stop));
        let mut ad = XPlaneAdapter::mit_anschluessen(anschluesse);
        ad.start(SimKind::XPlane12);

        let profil_ok = warte(Duration::from_secs(15), || {
            ad.shared
                .p2
                .lock()
                .profil
                .is_some_and(|pi| PROFILES[pi].name == "Laminar/Zibo 737-800")
        });
        let ap = warte(Duration::from_secs(15), || {
            ad.snapshot()
                .is_some_and(|s| s.autopilot_master == Some(true))
        });
        // Zibo-Audit 01.10.2026: Taxilicht, Sonden-Heizung, Logo, Anti-Eis
        // und Master Caution aus den Zibo-Schaltern. (A/T-ARM wird gelesen,
        // aber nicht ausgegeben — siehe `autothrottle_is_arm` in dataref.rs.)
        let schalter = warte(Duration::from_secs(15), || {
            ad.snapshot().is_some_and(|s| {
                s.light_taxi == Some(true)
                    && s.pitot_heat == Some(true)
                    && s.light_logo == Some(true)
                    && s.engine_anti_ice == Some(true)
                    && !s.autothrottle_is_arm
                    && s.master_caution == Some(true)
            })
        });
        let letzter = ad.snapshot();
        stop.store(true, Ordering::SeqCst);
        ad.stop();
        let _ = f1.join();
        let _ = f2.join();
        assert!(
            profil_ok,
            "Zibo-Profil nicht aktiv: {:?}",
            stand.lock().anfragen
        );
        assert!(
            ap,
            "Autopilot nicht an (nur CMD B: {nur_b}) — Lampe kam nicht im Snapshot an"
        );
        assert!(
            schalter,
            "Zibo-Schalter kamen nicht an: taxi={:?} pitot={:?} logo={:?} anti_eis={:?} at_arm={:?} caution={:?}",
            letzter.as_ref().map(|s| s.light_taxi),
            letzter.as_ref().map(|s| s.pitot_heat),
            letzter.as_ref().map(|s| s.light_logo),
            letzter.as_ref().map(|s| s.engine_anti_ice),
            letzter.as_ref().map(|s| s.autothrottle_is_arm),
            letzter.as_ref().map(|s| s.master_caution),
        );
    }

    #[test]
    fn sitzung_werte_vermessung_und_rueckfall() {
        let stop = Arc::new(AtomicBool::new(false));
        let plugin_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rref_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        // Ein Port, an dem sicher nichts lauscht (Web-API aus).
        let web_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let anschluesse = Anschluesse {
            rref: rref_sock.local_addr().unwrap(),
            plugin_p1: 0,
            plugin_p2: plugin_sock.local_addr().unwrap(),
            web_api: format!("http://127.0.0.1:{web_port}"),
        };
        let stand = Arc::new(Mutex::new(FakeStand {
            liefern: true,
            ..FakeStand::default()
        }));
        let freqs = Arc::new(Mutex::new(Vec::new()));
        let f1 = fake_plugin(plugin_sock, Arc::clone(&stand), Arc::clone(&stop));
        let f2 = fake_rref(rref_sock, Arc::clone(&freqs), Arc::clone(&stop));

        let mut ad = XPlaneAdapter::mit_anschluessen(anschluesse);
        ad.start(SimKind::XPlane12);

        // 1. Sitzung auf, Werte kommen an — Breite/Laenge doppelt genau.
        assert!(
            warte(Duration::from_secs(15), || ad
                .snapshot()
                .is_some_and(|s| s.lat == BREITE && s.lon == LAENGE)),
            "keine Plugin-Werte; Anfragen: {:?}",
            stand.lock().anfragen
        );
        assert!(stand
            .lock()
            .anfragen
            .iter()
            .any(|a| a.starts_with("HALLO 2 ")));
        assert_eq!(ad.state(), ConnectionState::Connected);
        let st = ad.premium_status();
        assert_eq!(st.protokoll, 2);
        assert_eq!(st.plugin_version.as_deref(), Some("1.0.0"));
        assert_eq!(st.xplane_version, Some(12100));
        assert!(!st.veraltet);
        assert!(st.namen_da > 0 && st.namen_fehlen > 0);
        {
            let p = ad.shared.parsed.lock();
            // Fehlender Name bleibt leer.
            assert!(!p.toliss_ap1);
            // Vorhandene Add-on-Quelle gilt (ohne Web-API).
            assert_eq!(p.toliss_apu_master, Some(true));
            // Kern-Name „da" mit Wert 0 und ohne zweiten Beleg: bleibt leer
            // (sonst Transponder „TEST" bei jedem Flugzeug).
            assert_eq!(p.b738_xpdr_knob, None);
        }
        assert_eq!(ad.shared.p2.lock().profil, None, "kein fremdes Profil");
        // Flugzeugmeldung des Plugins ohne Web-API.
        assert!(warte(Duration::from_secs(10), || ad
            .snapshot()
            .and_then(|s| s.aircraft_icao)
            .as_deref()
            == Some("A20N")));
        // RREF ruht: Abos abbestellt (freq 0).
        assert!(
            warte(Duration::from_secs(10), || freqs.lock().contains(&0)),
            "RREF nicht abbestellt"
        );

        // 2. Flugzeug vermessen ueber das Plugin.
        let spiegel = crate::vermessung::Spiegel::starten_mit(ad.plugin_zugang()).expect("Messung");
        assert_eq!(spiegel.quelle(), "plugin");
        assert_eq!(spiegel.flugzeug.icao.as_deref(), Some("A20N"));
        assert!(
            warte(Duration::from_secs(10), || spiegel.verbunden() >= 3),
            "Messwerte kamen nicht an"
        );
        let schnapp = spiegel.schnappschuss();
        assert_eq!(schnapp.get("sim/test/eins"), Some(&1.5));
        assert_eq!(schnapp.get("sim/test/feld[2]"), Some(&3.0));
        // Seit 05.10.2026 (FF777) zaehlen Texte mit: als Kennzahl im Stand,
        // der Text kommt ueber `Spiegel::text` zurueck.
        let k = *schnapp.get("sim/test/text").expect("Text im Stand");
        assert_eq!(spiegel.text("sim/test/text", k).as_deref(), Some("A20N"));
        assert_eq!(spiegel.text("sim/test/eins", 1.5), None);
        let abo = spiegel.abo_stand();
        // eins, zwei, feld und (seit 05.10.2026) der Text.
        assert_eq!((abo.angemeldet, abo.abgelehnt), (4, 0));
        assert_eq!(abo.quelle, "plugin");
        assert!(spiegel.lebt());
        // (Beim Oeffnen ging schon ein ENDE-ABO 3 hinaus — Aufraeumen, M1.)
        let ende3 = |st: &FakeStand| st.anfragen.iter().filter(|a| *a == "ENDE-ABO 3").count();
        let vorher = ende3(&stand.lock());
        drop(spiegel);
        assert!(warte(Duration::from_secs(5), || ende3(&stand.lock()) > vorher));

        // 3. Plugin verstummt → nach 3 s Rueckfall auf RREF, ohne Nullen.
        let n_vorher = freqs.lock().len();
        stand.lock().liefern = false;
        let ab = Instant::now();
        assert!(
            warte(Duration::from_secs(15), || freqs.lock()[n_vorher..]
                .iter()
                .any(|f| *f > 0)),
            "RREF nicht wieder abonniert"
        );
        // Untergrenze grosszuegig: nur „nicht sofort" (FRISCH = 3 s).
        let dauer = ab.elapsed();
        assert!(
            dauer >= Duration::from_millis(2000),
            "Rueckfall zu frueh: {dauer:?}"
        );
        let snap = ad.snapshot().expect("Schnappschuss nach dem Rueckfall weg");
        assert_eq!(snap.lat, BREITE, "Feld fiel beim Rueckfall");
        assert!(warte(Duration::from_secs(10), || ad
            .premium_status()
            .protokoll
            != 2));
        // Ohne RREF-Werte raeumt der Waechter 5 s nach dem letzten
        // Plugin-Wert auf — wie ohne Plugin.
        assert!(
            warte(Duration::from_secs(15), || ad.snapshot().is_none()),
            "veralteter Schnappschuss blieb stehen"
        );
        // Und es wird wieder HALLO gesagt.
        let hallos = stand
            .lock()
            .anfragen
            .iter()
            .filter(|a| a.starts_with("HALLO"))
            .count();
        assert!(hallos >= 2, "{hallos}");

        ad.stop();
        stop.store(true, Ordering::SeqCst);
        f1.join().unwrap();
        f2.join().unwrap();
    }

    /// QS AP7 H1: Langsames (aelteres) Plugin, das Mess-Abos weder annimmt
    /// (`abo_empfangen`) noch bestaetigt. Die
    /// Vermessung sendet das Abo genau einmal neu, gibt dann auf und nimmt
    /// fuer diesen Lauf die Web-API (hier absichtlich tot → deren Fehler);
    /// die Sitzung bleibt dabei offen (PINGs laufen weiter), das Mess-Abo
    /// wird abbestellt.
    #[test]
    fn langsames_plugin_vermessung_weicht_auf_web_api_aus() {
        let stop = Arc::new(AtomicBool::new(false));
        let plugin_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let rref_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let web_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let stand = Arc::new(Mutex::new(FakeStand {
            liefern: true,
            mess_stumm: true,
            ..FakeStand::default()
        }));
        let freqs = Arc::new(Mutex::new(Vec::new()));
        let f1 = fake_plugin(
            plugin_sock.try_clone().unwrap(),
            Arc::clone(&stand),
            Arc::clone(&stop),
        );
        let f2 = fake_rref(
            rref_sock.try_clone().unwrap(),
            Arc::clone(&freqs),
            Arc::clone(&stop),
        );
        let mut ad = XPlaneAdapter::mit_anschluessen(Anschluesse {
            rref: rref_sock.local_addr().unwrap(),
            plugin_p1: 0,
            plugin_p2: plugin_sock.local_addr().unwrap(),
            web_api: format!("http://127.0.0.1:{web_port}"),
        });
        ad.start(SimKind::XPlane12);
        assert!(warte(Duration::from_secs(15), || ad
            .plugin_zugang()
            .is_some()));
        let ergebnis = crate::vermessung::Spiegel::starten_mit(ad.plugin_zugang());
        let fehler = ergebnis
            .err()
            .expect("Web-API ist tot — Messung muss scheitern");
        assert!(
            fehler.contains("Web-API"),
            "kein Web-API-Rueckfall: {fehler}"
        );
        let st = stand.lock();
        let abos3 = st
            .anfragen
            .iter()
            .filter(|a| a.starts_with("ABO 3 "))
            .count();
        assert_eq!(abos3, 2, "genau eine Wiederholung: {:?}", st.anfragen);
        let pings = st.anfragen.iter().filter(|a| *a == "PING").count();
        assert!(pings >= 1, "PINGs waehrend des Wartens: {pings}");
        drop(st);
        assert_eq!(
            ad.premium_status().protokoll,
            2,
            "Sitzung darf nicht fallen"
        );
        // Mess-Abo abbestellt (nach dem Aufraeumen beim Oeffnen ein zweites Mal).
        assert!(warte(Duration::from_secs(5), || stand
            .lock()
            .anfragen
            .iter()
            .filter(|a| *a == "ENDE-ABO 3")
            .count()
            >= 2));
        ad.stop();
        stop.store(true, Ordering::SeqCst);
        f1.join().unwrap();
        f2.join().unwrap();
    }

    /// Ohne Plugin (niemand am Steuerport): keine Sitzung, RREF bleibt
    /// abonniert und wird nie abbestellt — wie vor AP7.
    #[test]
    fn ohne_plugin_bleibt_rref() {
        let stop = Arc::new(AtomicBool::new(false));
        let rref_sock = UdpSocket::bind("127.0.0.1:0").unwrap();
        let tot = UdpSocket::bind("127.0.0.1:0").unwrap();
        let web_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let freqs = Arc::new(Mutex::new(Vec::new()));
        let f = fake_rref(
            rref_sock.try_clone().unwrap(),
            Arc::clone(&freqs),
            Arc::clone(&stop),
        );
        let mut ad = XPlaneAdapter::mit_anschluessen(Anschluesse {
            rref: rref_sock.local_addr().unwrap(),
            plugin_p1: 0,
            plugin_p2: tot.local_addr().unwrap(),
            web_api: format!("http://127.0.0.1:{web_port}"),
        });
        ad.start(SimKind::XPlane12);
        assert!(warte(Duration::from_secs(10), || freqs.lock().len()
            >= CATALOG.len()));
        std::thread::sleep(Duration::from_millis(500));
        assert!(!freqs.lock().contains(&0), "RREF ohne Plugin abbestellt");
        let st = ad.premium_status();
        assert_eq!(st.protokoll, 0);
        assert!(!st.veraltet);
        assert!(ad.plugin_zugang().is_none());
        ad.stop();
        stop.store(true, Ordering::SeqCst);
        f.join().unwrap();
    }
}
