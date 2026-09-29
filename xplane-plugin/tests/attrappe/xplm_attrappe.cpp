// =============================================================================
// XPLM-Attrappe: lädt die gebaute .xpl wie X-Plane und treibt ihren Flight-Loop
// =============================================================================
//
//   aeroacars_attrappe <pfad/zu/mac.xpl|lin.xpl> <sekunden> [xp11]
//
// Das Programm stellt die XPLM-Funktionen bereit, die das Plugin benutzt
// (Datarefs, Flight-Loop, Log, Versionen, XPLMFindSymbol), lädt das Plugin
// per dlopen, ruft XPluginStart/Enable und dann den Flight-Loop mit 60 fps
// im Takt auf, den das Plugin zurückgibt — so wie X-Plane.
//
// Damit laufen die echte Socket-Schicht (dienst_xplm.cpp) und der Takt von
// Protokoll 1 (plugin.cpp) ohne X-Plane. Die Attrappe
//   * lauscht selbst auf 127.0.0.1:52000 und zählt Protokoll-1-Pakete,
//   * schickt nach 2 s XPLM_MSG_PLANE_LOADED (Flugzeug 0) und wechselt nach
//     4 s die ICAO-Kennung,
//   * mit "xp11" meldet sie XPLM 303 und kennt die XPLM-4.0-Symbole nicht.
// Am Ende druckt sie die Protokoll-1-Rate und endet mit 1, wenn sie nicht
// bei ~20 Hz lag (AGL 1000 m → Protokoll 1 im 0,05-s-Takt).
//
// Nur macOS/Linux (dlopen). Das Gegenstück, das Protokoll 2 prüft, ist
// werkzeuge/plugin_sonde.py (siehe ende_zu_ende.py).
// =============================================================================

#include <XPLM/XPLMDataAccess.h>
#include <XPLM/XPLMDefs.h>
#include <XPLM/XPLMPlugin.h>
#include <XPLM/XPLMProcessing.h>
#include <XPLM/XPLMUtilities.h>

#include <arpa/inet.h>
#include <dlfcn.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <unistd.h>

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <string>
#include <thread>
#include <vector>

namespace {

struct Ref {
    std::string name;
    int typen = 0;
    int i = 0;
    float f = 0.0f;
    double d = 0.0;
    std::vector<float> vf;
    std::vector<int> vi;
    std::string b;
};

std::vector<Ref*> g_refs;
bool g_xp11 = false;
// fnrml_gear liest nur Protokoll 1 (einmal je Tick) → Zähler = Protokoll-1-Ticks,
// auch wenn Port 52000 belegt ist (z. B. vom laufenden AeroACARS-Client).
Ref* g_fnrml = nullptr;
int g_p1_ticks = 0;
XPLMFlightLoop_f g_cb = nullptr;
void* g_cb_ref = nullptr;
float g_cb_intervall = 0.0f;
const auto g_start = std::chrono::steady_clock::now();

Ref* neu(const std::string& name, int typen) {
    Ref* r = new Ref();
    r->name = name;
    r->typen = typen;
    g_refs.push_back(r);
    return r;
}

Ref* such(const char* name) {
    for (Ref* r : g_refs) if (r->name == name) return r;
    return nullptr;
}

double sekunden() {
    return std::chrono::duration<double>(std::chrono::steady_clock::now() - g_start).count();
}

template <typename T>
int kopiere(const std::vector<T>& v, T* aus, int ab, int max) {
    if (aus == nullptr) return static_cast<int>(v.size());
    int n = 0;
    for (int k = ab; k >= 0 && k < static_cast<int>(v.size()) && n < max; ++k) aus[n++] = v[static_cast<size_t>(k)];
    return n;
}

// XPLM 4.0 (nur über XPLMFindSymbol erreichbar, wie beim echten X-Plane 11/12).
struct InfoKopie {
    int structSize;
    const char* name;
    XPLMDataTypeID type;
    int writable;
    XPLMPluginID owner;
};
int attrappe_count() { return static_cast<int>(g_refs.size()); }
void attrappe_by_index(int ab, int n, XPLMDataRef* aus) {
    for (int k = 0; k < n; ++k) {
        const int idx = ab + k;
        aus[k] = (idx >= 0 && idx < static_cast<int>(g_refs.size())) ? g_refs[static_cast<size_t>(idx)] : nullptr;
    }
}
void attrappe_info(XPLMDataRef h, InfoKopie* info) {
    Ref* r = static_cast<Ref*>(h);
    info->name = r->name.c_str();
    info->type = r->typen;
    info->writable = 0;
    info->owner = 0;
}

}  // namespace

// ---- Die XPLM-Funktionen, die das Plugin braucht ------------------------------

extern "C" {

XPLM_API void XPLMDebugString(const char* s) { std::fputs(s, stdout); std::fflush(stdout); }
XPLM_API XPLMDataRef XPLMFindDataRef(const char* name) { return such(name); }
XPLM_API int XPLMIsDataRefGood(XPLMDataRef h) { return h != nullptr; }
XPLM_API XPLMDataTypeID XPLMGetDataRefTypes(XPLMDataRef h) { return static_cast<Ref*>(h)->typen; }
XPLM_API int XPLMGetDatai(XPLMDataRef h) { return h ? static_cast<Ref*>(h)->i : 0; }
XPLM_API float XPLMGetDataf(XPLMDataRef h) {
    if (h != nullptr && h == g_fnrml) ++g_p1_ticks;
    return h ? static_cast<Ref*>(h)->f : 0.0f;
}
XPLM_API double XPLMGetDatad(XPLMDataRef h) { return h ? static_cast<Ref*>(h)->d : 0.0; }
XPLM_API int XPLMGetDatavi(XPLMDataRef h, int* aus, int ab, int max) { return kopiere(static_cast<Ref*>(h)->vi, aus, ab, max); }
XPLM_API int XPLMGetDatavf(XPLMDataRef h, float* aus, int ab, int max) { return kopiere(static_cast<Ref*>(h)->vf, aus, ab, max); }
XPLM_API int XPLMGetDatab(XPLMDataRef h, void* aus, int ab, int max) {
    const std::string& s = static_cast<Ref*>(h)->b;
    if (aus == nullptr) return static_cast<int>(s.size());
    int n = 0;
    for (int k = ab; k >= 0 && k < static_cast<int>(s.size()) && n < max; ++k) static_cast<char*>(aus)[n++] = s[static_cast<size_t>(k)];
    return n;
}
XPLM_API float XPLMGetElapsedTime(void) { return static_cast<float>(sekunden()); }
XPLM_API void XPLMRegisterFlightLoopCallback(XPLMFlightLoop_f cb, float intervall, void* ref) {
    g_cb = cb;
    g_cb_intervall = intervall;
    g_cb_ref = ref;
}
XPLM_API void XPLMUnregisterFlightLoopCallback(XPLMFlightLoop_f, void*) { g_cb = nullptr; }
XPLM_API void XPLMGetVersions(int* xp, int* xplm, XPLMHostApplicationID* host) {
    *xp = g_xp11 ? 11550 : 12100;
    *xplm = g_xp11 ? 303 : 430;
    *host = 1;
}
XPLM_API void* XPLMFindSymbol(const char* name) {
    if (g_xp11) return nullptr;
    void* p = nullptr;
    if (std::strcmp(name, "XPLMCountDataRefs") == 0) { auto f = &attrappe_count; std::memcpy(&p, &f, sizeof(p)); }
    if (std::strcmp(name, "XPLMGetDataRefsByIndex") == 0) { auto f = &attrappe_by_index; std::memcpy(&p, &f, sizeof(p)); }
    if (std::strcmp(name, "XPLMGetDataRefInfo") == 0) { auto f = &attrappe_info; std::memcpy(&p, &f, sizeof(p)); }
    return p;
}

}  // extern "C"

int main(int argc, char** argv) {
    if (argc < 3) {
        std::fprintf(stderr, "Aufruf: %s <plugin.xpl> <sekunden> [xp11]\n", argv[0]);
        return 2;
    }
    const double laufzeit = std::atof(argv[2]);
    g_xp11 = argc > 3 && std::strcmp(argv[3], "xp11") == 0;

    // Welt: die Datarefs von Protokoll 1, die Flugzeug-Kennung, ein paar
    // typische Namen und viele Füllnamen für LISTE.
    neu("sim/flightmodel/position/latitude", xplmType_Double | xplmType_Float)->d = 50.03456789012345;
    neu("sim/flightmodel/position/longitude", xplmType_Double | xplmType_Float)->d = 8.571234567890123;
    neu("sim/flightmodel/position/y_agl", xplmType_Float)->f = 1000.0f;
    neu("sim/flightmodel/position/local_vy", xplmType_Float)->f = -2.0f;
    g_fnrml = neu("sim/flightmodel/forces/fnrml_gear", xplmType_Float);
    neu("sim/flightmodel/failures/onground_any", xplmType_Int);
    neu("sim/flightmodel2/misc/gforce_normal", xplmType_Float)->f = 1.0f;
    neu("sim/flightmodel/position/theta", xplmType_Float)->f = 2.5f;
    neu("sim/flightmodel/position/phi", xplmType_Float);
    neu("sim/flightmodel/position/psi", xplmType_Float)->f = 253.1f;
    neu("sim/cockpit2/gauges/indicators/airspeed_kts_pilot", xplmType_Float)->f = 250.0f;
    neu("sim/flightmodel/position/groundspeed", xplmType_Float)->f = 140.0f;
    neu("sim/time/paused", xplmType_Int);
    neu("sim/time/is_in_replay", xplmType_Int);
    neu("sim/aircraft/view/acf_ICAO", xplmType_Data)->b = std::string("A20N") + std::string(36, '\0');
    neu("sim/aircraft/view/acf_descrip", xplmType_Data)->b = std::string("Airbus A320neo") + std::string(246, '\0');
    neu("sim/aircraft/view/acf_relative_path", xplmType_Data)->b = std::string("Aircraft/Laminar Research/Airbus A320neo/A320neo.acf") + std::string(100, '\0');
    neu("sim/flightmodel/engine/ENGN_N1_", xplmType_FloatArray)->vf = std::vector<float>(16, 21.5f);
    for (int k = 0; k < 6000; ++k) neu("attrappe/fuellung/nr_" + std::to_string(k), xplmType_Float);

    // Protokoll-1-Empfänger (statt des alten Clients).
    const int p1 = socket(AF_INET, SOCK_DGRAM, 0);
    sockaddr_in a{};
    a.sin_family = AF_INET;
    a.sin_port = htons(52000);
    a.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    const bool p1_da = bind(p1, reinterpret_cast<sockaddr*>(&a), sizeof(a)) == 0;
    fcntl(p1, F_SETFL, fcntl(p1, F_GETFL, 0) | O_NONBLOCK);

    void* bib = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
    if (!bib) {
        std::fprintf(stderr, "dlopen: %s\n", dlerror());
        return 2;
    }
    using StartFn = int (*)(char*, char*, char*);
    using VoidFn = void (*)();
    using EnableFn = int (*)();
    using MsgFn = void (*)(XPLMPluginID, int, void*);
    auto start = reinterpret_cast<StartFn>(dlsym(bib, "XPluginStart"));
    auto stop = reinterpret_cast<VoidFn>(dlsym(bib, "XPluginStop"));
    auto enable = reinterpret_cast<EnableFn>(dlsym(bib, "XPluginEnable"));
    auto disable = reinterpret_cast<VoidFn>(dlsym(bib, "XPluginDisable"));
    auto msg = reinterpret_cast<MsgFn>(dlsym(bib, "XPluginReceiveMessage"));
    if (!start || !stop || !enable || !disable || !msg) {
        std::fprintf(stderr, "Einstiegspunkte fehlen\n");
        return 2;
    }
    char name[256], sig[256], desc[256];
    if (start(name, sig, desc) != 1 || enable() != 1) {
        std::fprintf(stderr, "XPluginStart/Enable gescheitert\n");
        return 2;
    }
    std::printf("[Attrappe] Plugin geladen: %s (%s)%s\n", name, sig, g_xp11 ? " als X-Plane 11" : "");

    // Flight-Loop mit 60 fps, im Takt, den das Plugin verlangt.
    const double frame = 1.0 / 60.0;
    double naechster_aufruf = g_cb_intervall > 0 ? sekunden() + g_cb_intervall : 0.0;
    double letzter_aufruf = sekunden();
    int aufrufe = 0, jeder_frame = 0, p1_pakete = 0;
    long zaehler = 0;
    bool flugzeug_gemeldet = false, icao_gewechselt = false;
    double max_dauer = 0.0;
    while (sekunden() < laufzeit) {
        const double t = sekunden();
        if (!flugzeug_gemeldet && t > 2.0) {
            flugzeug_gemeldet = true;
            msg(XPLM_PLUGIN_XPLANE, XPLM_MSG_PLANE_LOADED, nullptr);
            msg(7, XPLM_MSG_PLANE_LOADED, nullptr);  // fremdes Plugin: muss ignoriert werden
        }
        if (!icao_gewechselt && t > 4.0) {
            icao_gewechselt = true;
            such("sim/aircraft/view/acf_ICAO")->b = std::string("B738") + std::string(36, '\0');
        }
        if (g_cb && t >= naechster_aufruf) {
            const double vorher = sekunden();
            const float r = g_cb(static_cast<float>(t - letzter_aufruf), static_cast<float>(t - letzter_aufruf),
                                 static_cast<int>(zaehler), g_cb_ref);
            const double dauer = sekunden() - vorher;
            if (dauer > max_dauer) max_dauer = dauer;
            letzter_aufruf = t;
            ++aufrufe;
            if (r < 0) {
                ++jeder_frame;
                naechster_aufruf = t;  // nächster Frame
            } else {
                naechster_aufruf = t + r;
            }
        }
        char puffer[4096];
        while (p1_da && recv(p1, puffer, sizeof(puffer), 0) > 0) ++p1_pakete;
        ++zaehler;
        // Fester 60-fps-Raster (bis zum nächsten Frame-Termin schlafen, nicht
        // "16,7 ms ab jetzt" — sonst bremst die Laufzeit jedes Frames mit).
        std::this_thread::sleep_until(g_start + std::chrono::duration_cast<std::chrono::steady_clock::duration>(
                                                    std::chrono::duration<double>(zaehler * frame)));
    }
    disable();
    stop();
    dlclose(bib);
    close(p1);

    const double tick_hz = g_p1_ticks / laufzeit;
    const double paket_hz = p1_pakete / laufzeit;
    std::printf("[Attrappe] %d Flight-Loop-Aufrufe (%d mit \"jeden Frame\"), laengster %.3f ms\n",
                aufrufe, jeder_frame, max_dauer * 1000.0);
    std::printf("[Attrappe] Protokoll 1: %d Ticks = %.1f Hz (Soll ~20 Hz bei 1000 m AGL)\n", g_p1_ticks, tick_hz);
    if (p1_da) {
        std::printf("[Attrappe] Protokoll 1: %d Pakete auf 52000 empfangen = %.1f Hz\n", p1_pakete, paket_hz);
    } else {
        std::printf("[Attrappe] Port 52000 belegt - Pakete nicht gezaehlt, nur Ticks\n");
    }
    // Grobe Schranken fuer genau zwei Rueckfaelle: Protokoll 1 laeuft
    // ploetzlich jeden Frame (> 21,5 Hz) oder steht (< 5 Hz). Keine engere
    // Untergrenze: Auf dem macOS-Runner unter ASan kam Protokoll 1 auch GANZ
    // OHNE Protokoll 2 (Port belegt, kein Aufruf "jeden Frame") nur auf
    // ~10 Hz, obwohl die Attrappe 60 fps hielt (CI 29.09.2026) — die Zeit-
    // steuerung der VM, nicht das Plugin. Das alte Plugin 0.5.13 verlangt
    // dasselbe Intervall (0,05 s).
    std::printf("[Attrappe] Bildrate %.1f fps\n", static_cast<double>(zaehler) / laufzeit);
    const bool ticks_ok = tick_hz > 5.0 && tick_hz < 21.5;
    const bool pakete_ok = !p1_da || (paket_hz > 5.0 && paket_hz < 21.5);
    return (ticks_ok && pakete_ok) ? 0 : 1;
}
