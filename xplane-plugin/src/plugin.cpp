// =============================================================================
// AeroACARS X-Plane Premium Plugin
// =============================================================================
//
// SDK: X-Plane Plugin SDK 4.3.0 (BSD-licensed, vendored under
//      third_party/XPSDK430/).
//
// Purpose: read a curated set of DataRefs at flight-loop frequency and
// forward them to the AeroACARS desktop client over a UDP loopback socket
// (port 52000 by default — outside X-Plane's 49000-49003 range so we
// never clash with X-Plane's own UDP send/receive sockets). Plus a
// one-shot "touchdown" event packet at the
// physical moment of wheel-runway contact (fnrml_gear edge) — captured with
// frame-perfect timing, no UDP-eviction race, no VSI-smoothing artifacts.
//
// Design constraints (NON-NEGOTIABLE — see xplane-plugin/README.md):
//
//   1. NEVER crash X-Plane.
//      - Every XPLMFindDataRef result is NULL-checked before use.
//      - All errors are caught + logged via XPLMDebugString, never propagated.
//      - No C++ exceptions cross the C-ABI plugin boundary (-fno-exceptions).
//
//   2. NEVER stall the flight loop.
//      - The flight-loop callback runs on X-Plane's render thread.
//      - We read ~15 DataRefs (microseconds), build a small JSON string
//        (microseconds), and call sendto() on a non-blocking UDP socket
//        (microseconds when the buffer's empty, ECONNREFUSED-ignored
//        when the client isn't listening).
//      - No filesystem I/O, no malloc inside the hot path.
//
//   3. NEVER persist state outside the plugin's address space.
//      - No file writes, no registry edits, no env-var tweaks.
//      - Plugin is purely read-only against X-Plane state.
//
//   4. CLEAN SHUTDOWN on plugin reload.
//      - XPluginStop unregisters the flight loop, closes the socket,
//        zeros every DataRef handle. A second XPluginStart afterwards
//        starts from a known-good slate.
//
// Wire format: line-delimited JSON over UDP. Every packet is a single line
// terminated with `\n`. Schema versioned via "v":1. See README.md §"Wire
// Format" for details.
//
// -----------------------------------------------------------------------------
// Plugin 1.0.0: zwei Protokolle in einem Flight-Loop (ADR-0004)
// -----------------------------------------------------------------------------
//
//   Protokoll 1 (dieser Datei, UNVERÄNDERT seit 0.5.13): `telemetry` und
//   `touchdown` an 127.0.0.1:52000. Bleibt für ältere Clients; der neue
//   Client wertet weiter `touchdown` aus. Schweigt in Pause/Replay wie bisher.
//
//   Protokoll 2 (dienst.cpp + dienst_xplm.cpp): Dataref-Server auf
//   127.0.0.1:52001. Der Client meldet Namen an, das Plugin sucht, meldet den
//   Status je Name ("fehlt" ist eine Tatsache, kein Schein-Nullwert) und
//   liefert mit eigener Rate je Abo — auch in Pause und Replay.
//
// Es bleibt EIN Flight-Loop-Callback. Er ruft Protokoll 1 in genau dem Takt
// auf, den Protokoll 1 selbst bestimmt (20 Hz, unter 200 ft AGL jeder Frame),
// und Protokoll 2 bei jedem Aufruf. Solange Protokoll 2 nichts zu liefern hat,
// gibt der Callback wie früher das Intervall von Protokoll 1 zurück — ohne
// Client verhält sich das Plugin also exakt wie 0.5.13. Mit aktiven Abos
// läuft der Callback jeden Frame (Raten bis 50 Hz), Protokoll 1 wird dann
// über die Uhr auf seinen eigenen Takt gedrosselt.
// =============================================================================

#include <XPLM/XPLMDataAccess.h>
#include <XPLM/XPLMDefs.h>
#include <XPLM/XPLMProcessing.h>
#include <XPLM/XPLMUtilities.h>

#include "dienst_xplm.h"

#include <cerrno>
#include <cmath>
#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

#if IBM
    #include <winsock2.h>
    #include <ws2tcpip.h>
    using socket_t = SOCKET;
    constexpr socket_t INVALID_SOCK = INVALID_SOCKET;
    static inline void close_sock(socket_t s) { closesocket(s); }
    static inline int sock_err() { return WSAGetLastError(); }
#else
    #include <arpa/inet.h>
    #include <fcntl.h>
    #include <netinet/in.h>
    #include <sys/socket.h>
    #include <sys/types.h>
    #include <unistd.h>
    using socket_t = int;
    constexpr socket_t INVALID_SOCK = -1;
    static inline void close_sock(socket_t s) { close(s); }
    static inline int sock_err() { return errno; }
#endif

// =============================================================================
// Configuration constants (compile-time)
// =============================================================================

// UDP destination — loopback only. The AeroACARS desktop client binds this
// port and listens for our packets. Mismatched port = silent no-op (sendto
// gets ECONNREFUSED which we ignore), no crash.
static constexpr const char* AEROACARS_UDP_HOST = "127.0.0.1";
// IMPORTANT: must be OUTSIDE X-Plane's own UDP port range (49000-
// 49003) — X-Plane uses 49001 as its outgoing-data source port,
// and binding the same port causes X-Plane's "Local network will
// be disabled" error. v0.5.0-v0.5.2 made this mistake; v0.5.3
// moves to 52000.
static constexpr uint16_t AEROACARS_UDP_PORT = 52000;

// Flight-loop callback interval (in seconds). Negative = "every N frames".
// We use 0.05 s (= 20 Hz) as the baseline — matches xgs Landing Speed
// Plugin's cadence. At low AGL the touchdown sampler tightens to per-frame
// (-1.0f) for sub-frame accuracy at the moment of contact.
static constexpr float FLIGHT_LOOP_BASE_INTERVAL_S = 0.05f;
static constexpr float FLIGHT_LOOP_FAST_INTERVAL = -1.0f;  // every frame
static constexpr float FAST_AGL_THRESHOLD_FT = 200.0f;

// Touchdown-edge threshold on `fnrml_gear` (Newtons). xgs uses != 0.0,
// we choose 1.0 N as a tiny safety margin against potential float-noise
// with absolutely no risk of missing real touchdowns (a 60-tonne airliner
// at the moment of wheel contact spikes to >> 100,000 N immediately).
static constexpr float GEAR_TOUCHDOWN_THRESHOLD_N = 1.0f;

// VS lookback window for capturing the descent peak just before contact —
// matches the AeroACARS-Rust-side sampler's window so the data semantics
// are identical between premium-mode and UDP-fallback-mode.
static constexpr int64_t VS_LOOKBACK_MS = 500;

// Plugin metadata — matches X-Plane's plugin browser UI.
static constexpr const char* PLUGIN_NAME = "AeroACARS Premium";
static constexpr const char* PLUGIN_SIG  = "com.aeroacars.xplane.premium";
static constexpr const char* PLUGIN_DESC =
    "Native frame-rate telemetry bridge for the AeroACARS desktop client. "
    "Optional companion to the standard UDP integration — no extra config "
    "needed when both are running.";

// =============================================================================
// Plugin globals (zero-initialized)
// =============================================================================
//
// All globals start at safe defaults. If XPluginStart fails partway through
// (e.g. socket creation fails, or we're on a system where the SDK headers
// don't match the installed X-Plane), the rest of the lifecycle still runs
// without crashing — we just don't send packets.

namespace {

// DataRef handles. NULL = not found = silently skip that field in packets.
struct DataRefs {
    XPLMDataRef latitude          = nullptr;  // sim/flightmodel/position/latitude (deg)
    XPLMDataRef longitude         = nullptr;  // sim/flightmodel/position/longitude (deg)
    XPLMDataRef agl_m             = nullptr;  // sim/flightmodel/position/y_agl (meters)
    XPLMDataRef vertical_velocity = nullptr;  // sim/flightmodel/position/local_vy (m/s, raw, no smoothing)
    XPLMDataRef gear_fnrml_n      = nullptr;  // sim/flightmodel/forces/fnrml_gear (Newtons)
    XPLMDataRef on_ground_any     = nullptr;  // sim/flightmodel/failures/onground_any (0/1)
    XPLMDataRef gforce_normal     = nullptr;  // sim/flightmodel2/misc/gforce_normal (g's)
    XPLMDataRef pitch_deg         = nullptr;  // sim/flightmodel/position/theta (deg)
    XPLMDataRef bank_deg          = nullptr;  // sim/flightmodel/position/phi (deg)
    XPLMDataRef heading_deg_true  = nullptr;  // sim/flightmodel/position/psi (deg)
    XPLMDataRef ias_kt            = nullptr;  // sim/cockpit2/gauges/indicators/airspeed_kts_pilot (kt)
    XPLMDataRef gs_ms             = nullptr;  // sim/flightmodel/position/groundspeed (m/s)
    XPLMDataRef sim_paused        = nullptr;  // sim/time/paused (0/1)
    XPLMDataRef sim_in_replay     = nullptr;  // sim/time/is_in_replay (0/1)
};

DataRefs g_drefs;

// UDP socket state.
socket_t g_sock = INVALID_SOCK;
sockaddr_in g_dest{};

// Per-tick state for touchdown detection.
//
// `prev_in_air` tracks the previous tick's "are we airborne" state, used
// to detect the false→true→false edge (in_air→on_ground transition).
// `touchdown_captured` is a one-shot guard so we only emit the touchdown
// event packet once per landing. Both reset to safe defaults when the
// plugin reloads.
bool prev_in_air = true;
bool touchdown_captured = false;

// v0.5.6: running peak-descent VS tracker. Updated every flight-loop
// tick while airborne (gear normal force < threshold). Picks the most
// negative pitch-corrected VS seen across the ENTIRE airborne segment
// — the whole approach + final, not just the last 500 ms. Robust
// against aggressive flares where the peak descent happened seconds
// before the actual touchdown and the lookback ring buffer has only
// near-zero rebound samples by the time the edge fires.
//
// Reset to 0 (= "no descent yet") on every fresh ground→air transition
// (takeoff, go-around) so each landing attempt gets a clean tracker.
// Reset is also done after touchdown is captured so the next landing
// in a touch-and-go starts fresh.
float g_airborne_vs_min = 0.0f;

// Sequence counter for outgoing packets (monotonic). Resets on plugin
// reload, which is acceptable — the client uses it for diagnostics
// (gap detection) only, not for ordering.
uint32_t g_seq = 0;

// VS / AGL lookback ring buffer. Stores the last N samples so when
// the touchdown edge fires we can:
//   * Find the peak descent VS in the last VS_LOOKBACK_MS ms (legacy)
//   * Compute geometric descent rate via ΔAGL/Δt over multiple
//     windows (v0.5.8 — LandingRate-1 algorithm, primary method)
//
// AGL field added v0.5.8 — same idea as LandingRate-1.lua: VSI lies
// during flare, AGL geometry doesn't.
struct VSSample {
    double t_sec;     // X-Plane elapsed sim time (sim/time/total_running_time_sec)
    float vs_fpm;
    float pitch_deg;
    float agl_ft;     // v0.5.8: snap AGL for AGL-derivative method
};
constexpr size_t VS_BUFFER_CAP = 128;  // v0.5.8: 128 × ~30ms = ~3.8s history
VSSample g_vs_buffer[VS_BUFFER_CAP];
size_t g_vs_buffer_head = 0;  // next write index (ring)
size_t g_vs_buffer_count = 0; // valid samples (caps at VS_BUFFER_CAP)

// =============================================================================
// Logging — XPLM-safe, never blocks
// =============================================================================
//
// X-Plane's Log.txt is the canonical log destination for plugins.
// XPLMDebugString() is the only thread-safe sync logger. We prefix every
// line with "[AeroACARS]" so we're easy to grep.

void log_msg(const char* msg) noexcept {
    if (!msg) return;
    char buf[1024];
    std::snprintf(buf, sizeof(buf), "[AeroACARS] %s\n", msg);
    XPLMDebugString(buf);
}

void log_msgf(const char* fmt, ...) noexcept {
    if (!fmt) return;
    char body[896];
    va_list ap;
    va_start(ap, fmt);
    std::vsnprintf(body, sizeof(body), fmt, ap);
    va_end(ap);
    char line[1024];
    std::snprintf(line, sizeof(line), "[AeroACARS] %s\n", body);
    XPLMDebugString(line);
}

// =============================================================================
// DataRef helpers (NULL-safe)
// =============================================================================

float read_float(XPLMDataRef ref, float fallback = 0.0f) noexcept {
    if (ref == nullptr) return fallback;
    return XPLMGetDataf(ref);
}

double read_double(XPLMDataRef ref, double fallback = 0.0) noexcept {
    if (ref == nullptr) return fallback;
    return XPLMGetDatad(ref);
}

int read_int(XPLMDataRef ref, int fallback = 0) noexcept {
    if (ref == nullptr) return fallback;
    return XPLMGetDatai(ref);
}

XPLMDataRef find_ref(const char* path) noexcept {
    XPLMDataRef ref = XPLMFindDataRef(path);
    if (ref == nullptr) {
        log_msgf("warn: DataRef not found: %s (will use fallback values)", path);
    }
    return ref;
}

// =============================================================================
// UDP transport — non-blocking sendto, errors silently ignored
// =============================================================================

void make_socket_nonblocking(socket_t s) noexcept {
#if IBM
    u_long mode = 1;
    ioctlsocket(s, FIONBIO, &mode);
#else
    int flags = fcntl(s, F_GETFL, 0);
    if (flags >= 0) fcntl(s, F_SETFL, flags | O_NONBLOCK);
#endif
}

bool open_socket() noexcept {
#if IBM
    WSADATA wsa{};
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) {
        log_msg("error: WSAStartup failed; UDP transport disabled");
        return false;
    }
#endif
    g_sock = ::socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    if (g_sock == INVALID_SOCK) {
        log_msgf("error: socket() failed (errno=%d); UDP transport disabled", sock_err());
        return false;
    }
    make_socket_nonblocking(g_sock);

    g_dest = {};
    g_dest.sin_family = AF_INET;
    g_dest.sin_port = htons(AEROACARS_UDP_PORT);
    if (inet_pton(AF_INET, AEROACARS_UDP_HOST, &g_dest.sin_addr) != 1) {
        log_msgf("error: inet_pton failed for %s", AEROACARS_UDP_HOST);
        close_sock(g_sock);
        g_sock = INVALID_SOCK;
        return false;
    }
    log_msgf("UDP socket open: forwarding to %s:%u",
             AEROACARS_UDP_HOST, (unsigned)AEROACARS_UDP_PORT);
    return true;
}

void close_socket() noexcept {
    if (g_sock != INVALID_SOCK) {
        close_sock(g_sock);
        g_sock = INVALID_SOCK;
    }
#if IBM
    WSACleanup();
#endif
}

// Fire-and-forget UDP send. Failures are silently ignored — the client may
// not be running, and that's a normal state for us (pilot has X-Plane open
// but AeroACARS Tauri client closed). NEVER throws, NEVER blocks.
void send_packet(const char* payload, size_t len) noexcept {
    if (g_sock == INVALID_SOCK || payload == nullptr || len == 0) return;
    ::sendto(g_sock, payload, static_cast<int>(len), 0,
             reinterpret_cast<const sockaddr*>(&g_dest), sizeof(g_dest));
    // Intentionally no error-checking. ECONNREFUSED, EAGAIN, etc. are all
    // ignored — the next tick will try again with fresh data.
}

// =============================================================================
// JSON building — printf-style with bounds checking
// =============================================================================
//
// We deliberately avoid heap allocation in the flight loop. Each packet is
// built into a fixed 2 KB stack buffer; if for some reason we overflow,
// we truncate cleanly (vsnprintf is bounded). 2 KB is well above any
// realistic packet size — a fully-populated telemetry frame is ~600 bytes.

constexpr size_t PACKET_BUF_SIZE = 2048;

// =============================================================================
// Flight-loop callback — the hot path
// =============================================================================
//
// Called by X-Plane every FLIGHT_LOOP_BASE_INTERVAL_S seconds (or every
// frame when low). MUST be fast — no I/O, no waiting, no malloc in here.
//
// Returns the seconds-until-next-call. We tighten the rate when at low
// AGL so the touchdown edge gets sub-frame resolution.

// Protokoll 1 — bis auf den Namen (vorher flight_loop_cb) unverändert. Der
// Rückgabewert ist weiterhin das gewünschte Intervall bis zum nächsten Tick;
// flight_loop_cb unten setzt es um.
float protokoll1_tick() noexcept {
    // Skip work entirely while the sim is paused or in replay — those
    // states give us frozen / weird telemetry that the AeroACARS client
    // wouldn't know how to interpret. Sim/replay-aware code is the
    // client's job, not the plugin's.
    if (read_int(g_drefs.sim_paused) != 0 || read_int(g_drefs.sim_in_replay) != 0) {
        return FLIGHT_LOOP_BASE_INTERVAL_S;
    }

    // -- Read DataRefs (NULL-safe, fast) ---------------------------------
    const double sim_t       = static_cast<double>(XPLMGetElapsedTime());
    const double lat         = read_double(g_drefs.latitude);
    const double lon         = read_double(g_drefs.longitude);
    const float  agl_m       = read_float(g_drefs.agl_m);
    const float  agl_ft      = agl_m * 3.28084f;
    const float  vy_ms       = read_float(g_drefs.vertical_velocity);
    const float  vs_fpm_raw  = vy_ms * 196.8504f;  // m/s → fpm
    const float  pitch_deg   = read_float(g_drefs.pitch_deg);
    const float  pitch_rad   = pitch_deg * 0.0174533f;
    const float  pitch_cos   = std::cos(pitch_rad);
    // Pitch-corrected VS (xgs convention) — cos(pitch) projects world-
    // vertical Y-velocity to the body-axial direction. For typical
    // touchdowns at 3-5° pitch this is a 0.1-0.4% adjustment; for
    // STOL-style 10° flares ~1.5%. Free accuracy.
    const float  vs_fpm      = vs_fpm_raw * pitch_cos;
    const float  fnrml_n     = read_float(g_drefs.gear_fnrml_n);
    const int    on_ground   = read_int(g_drefs.on_ground_any);
    const float  gnorm       = read_float(g_drefs.gforce_normal, 1.0f);
    const float  bank_deg    = read_float(g_drefs.bank_deg);
    const float  hdg_true    = read_float(g_drefs.heading_deg_true);
    const float  ias_kt      = read_float(g_drefs.ias_kt);
    const float  gs_ms       = read_float(g_drefs.gs_ms);
    const float  gs_kt       = gs_ms * 1.94384f;

    // -- Push to VS ring buffer (always, regardless of touchdown) --------
    {
        VSSample& slot = g_vs_buffer[g_vs_buffer_head];
        slot.t_sec     = sim_t;
        slot.vs_fpm    = vs_fpm;
        slot.pitch_deg = pitch_deg;
        slot.agl_ft    = agl_ft;  // v0.5.8: for AGL-derivative method
        g_vs_buffer_head = (g_vs_buffer_head + 1) % VS_BUFFER_CAP;
        if (g_vs_buffer_count < VS_BUFFER_CAP) g_vs_buffer_count++;
    }

    // -- Touchdown-edge detection (fnrml_gear-based) --------------------
    //
    // Definition: in_air = gear-normal-force below threshold. xgs uses
    // != 0; we use > 1 N as a tiny noise filter. Edge fires when we
    // transition from "in air" to "on ground" — and only ONCE per
    // landing (touchdown_captured guard). The guard clears when AGL
    // climbs above 50 ft so a go-around resets us cleanly.
    const bool in_air_now = (fnrml_n < GEAR_TOUCHDOWN_THRESHOLD_N);
    if (touchdown_captured && agl_ft > 50.0f) {
        // Got back airborne — re-arm.
        touchdown_captured = false;
    }
    const bool edge = prev_in_air && !in_air_now && !touchdown_captured;

    // -- v0.5.6/11: running peak-descent VS tracker (LOW-AGL only) ------
    //
    // v0.5.11 hardening per pilot's deep analysis: this tracker now
    // ONLY accumulates samples while AGL ≤ 250 ft (= touchdown
    // footprint). Earlier versions accumulated across the entire
    // airborne segment, which produced phantom hard-landing values
    // when a steep pre-flare descent (e.g. -1346 fpm @ 943 ft AGL)
    // beat the actual gentle touchdown. Limiting to AGL ≤ 250 ft
    // ensures pre-flare descent rates can't pollute the touchdown
    // capture.
    //
    // Reset on the ground→air edge so a fresh approach starts clean.
    if (in_air_now && !prev_in_air) {
        g_airborne_vs_min = 0.0f;
    }
    if (in_air_now && agl_ft <= 250.0f && vs_fpm < g_airborne_vs_min) {
        g_airborne_vs_min = vs_fpm;
    }

    // -- Build + send the per-tick telemetry packet ----------------------
    {
        char buf[PACKET_BUF_SIZE];
        int n = std::snprintf(buf, sizeof(buf),
            "{"
            "\"v\":1,"
            "\"type\":\"telemetry\","
            "\"seq\":%u,"
            "\"ts\":%.6f,"
            "\"lat\":%.7f,"
            "\"lon\":%.7f,"
            "\"agl_ft\":%.2f,"
            "\"vs_fpm_raw\":%.2f,"
            "\"vs_fpm\":%.2f,"
            "\"fnrml_gear_n\":%.2f,"
            "\"on_ground\":%s,"
            "\"g_normal\":%.4f,"
            "\"pitch_deg\":%.3f,"
            "\"bank_deg\":%.3f,"
            "\"hdg_true\":%.3f,"
            "\"ias_kt\":%.2f,"
            "\"gs_kt\":%.2f"
            "}\n",
            ++g_seq,
            sim_t,
            lat, lon,
            static_cast<double>(agl_ft),
            static_cast<double>(vs_fpm_raw),
            static_cast<double>(vs_fpm),
            static_cast<double>(fnrml_n),
            on_ground != 0 ? "true" : "false",
            static_cast<double>(gnorm),
            static_cast<double>(pitch_deg),
            static_cast<double>(bank_deg),
            static_cast<double>(hdg_true),
            static_cast<double>(ias_kt),
            static_cast<double>(gs_kt));
        if (n > 0 && static_cast<size_t>(n) < sizeof(buf)) {
            send_packet(buf, static_cast<size_t>(n));
        }
    }

    // -- Touchdown event packet (one-shot, frame-perfect) ---------------
    if (edge) {
        // v0.5.13: Lua-style adaptive 30-sample AGL-Δ (LandingRate-1
        // method by Dan Berry, 2014+). Replaces the v0.5.11 fixed
        // time-tier approach (750ms/1s/1.5s/...) with a single
        // sample-count window that adapts to flight-loop frequency
        // automatically.
        //
        //   * Take the LATEST 30 samples (or fewer if buffer not full)
        //     — at 60 fps that's ~0.5s, at 30 fps ~1s, at 10 fps ~3s
        //   * AGL guards identical to before:
        //     - touchdown sample AGL ≤ 5 ft (or on_ground equivalent)
        //     - window-start AGL ≤ 250 ft (no pre-flare contamination)
        //   * Result must be < 0 fpm (positive = unphysical)
        //
        // Pilot test 2026-05-07 (MYNN→MBGT, X-Plane v0.5.11): old
        // time-tier method gave -394 fpm because 750ms tier had
        // <5 samples (X-Plane RREF rate too low for that fps), 1500ms
        // tier won and pulled in pre-flare data. Lua-style 30-sample
        // adapts to this case with ~0.6s effective window → matches
        // LandingRate-1.lua's 273 fpm.
        constexpr int LUA_SAMPLE_COUNT = 30;
        constexpr int LUA_MIN_SAMPLES  = 5;
        constexpr float TD_AGL_MAX_AT_TD_FT    = 5.0f;
        constexpr float TD_AGL_MAX_AT_START_FT = 250.0f;

        float captured_vs = 0.0f;
        const char* vs_source = "none";
        int vs_window_ms = 0;
        int vs_sample_count = 0;

        // Collect samples up to current sim_t (descending by time
        // so we can take the LATEST N easily).
        struct SampleRef { double t_sec; float agl_ft; };
        SampleRef sorted[VS_BUFFER_CAP];
        int sorted_n = 0;
        for (size_t i = 0; i < g_vs_buffer_count; ++i) {
            const VSSample& s = g_vs_buffer[i];
            if (s.t_sec <= sim_t) {
                sorted[sorted_n++] = { s.t_sec, s.agl_ft };
            }
        }
        // Simple insertion-sort by t_sec ascending (small N, ≤128).
        for (int i = 1; i < sorted_n; ++i) {
            SampleRef key = sorted[i];
            int j = i - 1;
            while (j >= 0 && sorted[j].t_sec > key.t_sec) {
                sorted[j + 1] = sorted[j];
                j--;
            }
            sorted[j + 1] = key;
        }

        if (sorted_n >= LUA_MIN_SAMPLES) {
            const int take = (sorted_n < LUA_SAMPLE_COUNT)
                ? sorted_n : LUA_SAMPLE_COUNT;
            const SampleRef* recent = &sorted[sorted_n - take];
            const SampleRef& first = recent[0];
            const SampleRef& last = recent[take - 1];

            const bool ok_td_agl = (last.agl_ft <= TD_AGL_MAX_AT_TD_FT);
            const bool ok_start_agl = (first.agl_ft <= TD_AGL_MAX_AT_START_FT);
            const float timespan = static_cast<float>(last.t_sec - first.t_sec);
            if (ok_td_agl && ok_start_agl && timespan >= 0.2f) {
                float agl_sum = 0.0f;
                for (int i = 0; i < take; ++i) {
                    agl_sum += recent[i].agl_ft;
                }
                const float avg_agl = agl_sum / static_cast<float>(take);
                const float agl_midpoint = last.agl_ft - avg_agl;
                const float fpm = (agl_midpoint / (timespan / 2.0f)) * 60.0f;
                if (std::isfinite(fpm) && fpm < 0.0f) {
                    captured_vs = fpm;
                    vs_source = "lua_30_sample";
                    vs_window_ms = static_cast<int>(timespan * 1000.0f);
                    vs_sample_count = take;
                }
            }
        }

        // Fallback: if Lua-style estimate failed (very sparse buffer,
        // pre-touchdown AGL spike etc.), fall back to the low-AGL
        // VS-min tracker (now AGL ≤ 250 ft only — see
        // g_airborne_vs_min update above). Last resort.
        if (captured_vs == 0.0f && g_airborne_vs_min < 0.0f) {
            captured_vs = g_airborne_vs_min;
            vs_source = "low_agl_vs_min";
            vs_window_ms = 0;
            vs_sample_count = 0;
        }

        char buf[PACKET_BUF_SIZE];
        int n = std::snprintf(buf, sizeof(buf),
            "{"
            "\"v\":1,"
            "\"type\":\"touchdown\","
            "\"seq\":%u,"
            "\"ts\":%.6f,"
            "\"lat\":%.7f,"
            "\"lon\":%.7f,"
            "\"captured_vs_fpm\":%.2f,"
            "\"captured_vs_source\":\"%s\","
            "\"captured_vs_window_ms\":%d,"
            "\"captured_vs_samples\":%d,"
            "\"captured_g_normal\":%.4f,"
            "\"captured_pitch_deg\":%.3f,"
            "\"captured_bank_deg\":%.3f,"
            "\"captured_ias_kt\":%.2f,"
            "\"captured_gs_kt\":%.2f,"
            "\"captured_heading_deg\":%.3f,"
            "\"fnrml_gear_n\":%.2f,"
            "\"agl_ft\":%.2f"
            "}\n",
            ++g_seq,
            sim_t,
            lat, lon,
            static_cast<double>(captured_vs),
            vs_source,
            vs_window_ms,
            vs_sample_count,
            static_cast<double>(gnorm),
            static_cast<double>(pitch_deg),
            static_cast<double>(bank_deg),
            static_cast<double>(ias_kt),
            static_cast<double>(gs_kt),
            static_cast<double>(hdg_true),
            static_cast<double>(fnrml_n),
            static_cast<double>(agl_ft));
        if (n > 0 && static_cast<size_t>(n) < sizeof(buf)) {
            send_packet(buf, static_cast<size_t>(n));
        }
        log_msgf("touchdown captured: vs=%.1f fpm  g=%.2f  ias=%.1f kt",
                 static_cast<double>(captured_vs),
                 static_cast<double>(gnorm),
                 static_cast<double>(ias_kt));
        touchdown_captured = true;
        // Reset airborne tracker so the next ground→air edge starts
        // a clean run (touch-and-go, missed approach with go-around).
        g_airborne_vs_min = 0.0f;
    }

    // -- Update edge state for next tick --------------------------------
    prev_in_air = in_air_now;

    // -- Adaptive interval: faster when close to ground -----------------
    if (agl_ft < FAST_AGL_THRESHOLD_FT) {
        return FLIGHT_LOOP_FAST_INTERVAL;  // every frame
    }
    return FLIGHT_LOOP_BASE_INTERVAL_S;
}

// -- Takt für Protokoll 1, wenn der Callback jeden Frame läuft ---------------
//
// X-Plane ruft einen Callback mit Intervall 0,05 s im ersten Frame auf, in dem
// 0,05 s vergangen sind. Genau das bildet die Uhr hier nach, wenn Protokoll 2
// den Callback auf "jeden Frame" gestellt hat. Die Toleranz fängt Rundung der
// float-Uhr (XPLMGetElapsedTime) ab, damit ein Tick bei 49,99 ms nicht einen
// ganzen Frame zu spät kommt.
constexpr double P1_TOLERANZ_S = 0.002;
double g_p1_faellig = 0.0;  // XPLMGetElapsedTime-Sekunden; <= jetzt = fällig

float flight_loop_cb(float, float, int, void*) noexcept {
    const double jetzt = static_cast<double>(XPLMGetElapsedTime());

    // -- Protokoll 1 in seinem eigenen Takt ---------------------------------
    float p1_intervall = FLIGHT_LOOP_BASE_INTERVAL_S;
    const bool p1_lief = (jetzt + P1_TOLERANZ_S >= g_p1_faellig);
    if (p1_lief) {
        p1_intervall = protokoll1_tick();
        // Negativ = "jeden Frame" → im nächsten Aufruf wieder fällig.
        g_p1_faellig = (p1_intervall > 0.0f) ? jetzt + static_cast<double>(p1_intervall) : jetzt;
    }

    // -- Protokoll 2 bei jedem Aufruf (auch in Pause/Replay) -----------------
    const bool p2_jeder_frame = dienst_frame();
    if (p2_jeder_frame) return FLIGHT_LOOP_FAST_INTERVAL;

    // Ohne Lieferauftrag: exakt das Verhalten von 0.5.13 — das Intervall, das
    // Protokoll 1 gerade verlangt hat, bzw. die Restzeit bis zu seinem Tick.
    if (p1_lief) return p1_intervall;
    const double rest = g_p1_faellig - jetzt;
    return rest > 0.001 ? static_cast<float>(rest) : FLIGHT_LOOP_FAST_INTERVAL;
}

}  // namespace

// =============================================================================
// XPLM Plugin entry points
// =============================================================================
//
// These four callbacks form the entire X-Plane plugin contract. We use the
// stable XPLM extern-C exports + PLUGIN_API decorators per SDK convention.

PLUGIN_API int XPluginStart(char* outName, char* outSig, char* outDesc) {
    // Defensive: copy our metadata into the SDK's caller-owned buffers.
    // Format docs say these are 256-byte buffers; strncpy + force-null is
    // the canonical safe pattern.
    std::strncpy(outName, PLUGIN_NAME, 255); outName[255] = '\0';
    std::strncpy(outSig,  PLUGIN_SIG,  255); outSig[255]  = '\0';
    std::strncpy(outDesc, PLUGIN_DESC, 255); outDesc[255] = '\0';

    // v0.7.17 (N-004): version comes from CMake project() so the
    // log matches the actual built binary. Before this we hardcoded
    // "v0.5.0" while the code drifted through 0.5.3 → 0.5.13.
    log_msgf("starting AeroACARS X-Plane Plugin v" AEROACARS_PLUGIN_VERSION " (SDK 4.3.0)");

    // Resolve all DataRef handles. NULL is acceptable for any of these
    // — the read_*-helpers fall back to a sensible default. We log
    // each warning so the pilot can see in Log.txt if their X-Plane
    // version is missing something we expect.
    g_drefs.latitude          = find_ref("sim/flightmodel/position/latitude");
    g_drefs.longitude         = find_ref("sim/flightmodel/position/longitude");
    g_drefs.agl_m             = find_ref("sim/flightmodel/position/y_agl");
    g_drefs.vertical_velocity = find_ref("sim/flightmodel/position/local_vy");
    g_drefs.gear_fnrml_n      = find_ref("sim/flightmodel/forces/fnrml_gear");
    g_drefs.on_ground_any     = find_ref("sim/flightmodel/failures/onground_any");
    g_drefs.gforce_normal     = find_ref("sim/flightmodel2/misc/gforce_normal");
    g_drefs.pitch_deg         = find_ref("sim/flightmodel/position/theta");
    g_drefs.bank_deg          = find_ref("sim/flightmodel/position/phi");
    g_drefs.heading_deg_true  = find_ref("sim/flightmodel/position/psi");
    g_drefs.ias_kt            = find_ref("sim/cockpit2/gauges/indicators/airspeed_kts_pilot");
    g_drefs.gs_ms             = find_ref("sim/flightmodel/position/groundspeed");
    g_drefs.sim_paused        = find_ref("sim/time/paused");
    g_drefs.sim_in_replay     = find_ref("sim/time/is_in_replay");

    // Open UDP socket. Failure here is non-fatal — we just won't send
    // packets, but the plugin still loads cleanly.
    if (!open_socket()) {
        log_msg("warn: UDP socket setup failed; plugin loaded but inert");
    }

    // Protokoll 2 (Dataref-Server). Scheitert es (Port 52001 belegt, kein
    // Speicher), steht der Grund im Log und Protokoll 1 läuft allein weiter.
    dienst_start(AEROACARS_PLUGIN_VERSION);
    g_p1_faellig = 0.0;

    // Register the flight-loop callback. Returning 1 = plugin started OK.
    XPLMRegisterFlightLoopCallback(flight_loop_cb, FLIGHT_LOOP_BASE_INTERVAL_S, nullptr);

    log_msg("AeroACARS X-Plane Plugin started successfully");
    return 1;
}

PLUGIN_API void XPluginStop(void) {
    log_msg("stopping AeroACARS X-Plane Plugin");

    // Reverse-order cleanup:
    //   1. Unregister flight-loop callback FIRST so we stop sending.
    //   2. Close the socket.
    //   3. Zero DataRef handles (defensive — plugin reload will re-find).
    XPLMUnregisterFlightLoopCallback(flight_loop_cb, nullptr);
    dienst_stopp();
    close_socket();

    g_drefs = DataRefs{};
    g_vs_buffer_head = 0;
    g_vs_buffer_count = 0;
    prev_in_air = true;
    touchdown_captured = false;
    g_airborne_vs_min = 0.0f;
    g_seq = 0;
    g_p1_faellig = 0.0;

    log_msg("AeroACARS X-Plane Plugin stopped cleanly");
}

PLUGIN_API int XPluginEnable(void) {
    // Nothing to do — we run continuously while loaded. X-Plane only
    // calls Disable/Enable on user request, not normally.
    return 1;
}

PLUGIN_API void XPluginDisable(void) {
    // Same — no-op. State stays valid until XPluginStop.
}

PLUGIN_API void XPluginReceiveMessage(XPLMPluginID inFrom, int inMessage, void* inParam) {
    // Nur X-Planes eigene Meldungen zu Flugzeug und Flughafen interessieren
    // (Namen neu suchen, Flugzeugwechsel melden). Nachrichten anderer Plugins
    // werden weiter nicht angenommen — ein fremdes Plugin, das zufällig
    // dieselbe Nachrichtennummer benutzt, soll keine Neusuche auslösen.
    if (inFrom != XPLM_PLUGIN_XPLANE) return;
    dienst_nachricht(inMessage, inParam);
}
