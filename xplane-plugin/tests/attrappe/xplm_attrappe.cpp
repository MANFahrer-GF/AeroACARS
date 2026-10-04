// =============================================================================
// XPLM-Attrappe: lädt die gebaute .xpl wie X-Plane und treibt ihren Flight-Loop
// =============================================================================
//
//   aeroacars_attrappe <pfad/zu/mac.xpl|lin.xpl> <sekunden> [xp11|zyklus]
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
//   * mit "zyklus" (Codex-Abnahme N1) spielt sie selbst den Client und
//     schaltet das Plugin nach 2 s ab (XPluginDisable) und nach 3 s wieder
//     ein (XPluginEnable), wie der Plugin-Admin von X-Plane. Geprüft wird:
//     Port 52001 ist nach Disable frei, nach Enable wieder gebunden; das
//     neue Protokoll 2 kennt den alten Client nicht mehr (kein_hallo) und
//     nimmt ein neues HALLO an; Protokoll 1 läuft nach Enable weiter. Wie bei
//     X-Plane ruft die Attrappe den Flight-Loop eines abgeschalteten Plugins
//     nicht auf. Dazu Protokoll 1 über den Zyklus (Nachprüfung Codex N1):
//     abgeschaltet in der Luft, währenddessen gelandet → nach Enable KEIN
//     Schein-Touchdown; danach Start und echte Landung → genau ein Touchdown.
// Am Ende druckt sie die Protokoll-1-Rate und endet mit 1, wenn sie nicht
// bei ~20 Hz lag (AGL 1000 m → Protokoll 1 im 0,05-s-Takt).
//
// Nur macOS/Linux (dlopen). Das Gegenstück, das Protokoll 2 prüft, ist
// werkzeuge/plugin_sonde.py (siehe ende_zu_ende.py).
// =============================================================================

#include <XPLM/XPLMDataAccess.h>
#include <XPLM/XPLMDefs.h>
#include <XPLM/XPLMDisplay.h>
#include <XPLM/XPLMGraphics.h>
#include <XPLM/XPLMMenus.h>
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
#include <unordered_map>
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
// fnrml_gear per XPLMGetDataf liest nur Protokoll 1 (einmal je Tick) → Zähler = Protokoll-1-Ticks,
// auch wenn Port 52000 belegt ist (z. B. vom laufenden AeroACARS-Client).
Ref* g_fnrml = nullptr;
int g_p1_ticks = 0;
std::vector<double> g_aufsetzer;  // Zeitpunkte von "touchdown captured" (Protokoll 1)
XPLMFlightLoop_f g_cb = nullptr;
void* g_cb_ref = nullptr;
float g_cb_intervall = 0.0f;
const auto g_start = std::chrono::steady_clock::now();

// Wie X-Plane: Suche über eine Hash-Tabelle (die Kostenmessung des Plugins
// beim Start misst damit etwas Ähnliches wie am echten Sim).
std::unordered_map<std::string, Ref*> g_nach_name;

Ref* neu(const std::string& name, int typen) {
    Ref* r = new Ref();
    r->name = name;
    r->typen = typen;
    g_refs.push_back(r);
    g_nach_name.emplace(name, r);
    return r;
}

Ref* such(const char* name) {
    auto it = g_nach_name.find(name);
    return it == g_nach_name.end() ? nullptr : it->second;
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

XPLM_API void XPLMDebugString(const char* s) {
    if (std::strstr(s, "touchdown captured") != nullptr) g_aufsetzer.push_back(sekunden());
    std::fputs(s, stdout);
    std::fflush(stdout);
}
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

// ---- HUD-Band (Plugin 1.1.0): Fenster, Menü, Schrift, Prefs -----------------------
// Nur so viel, dass das Plugin sich laden, starten und beenden lässt; gezeichnet
// wird in der Attrappe nichts (kein Fenstersystem).
XPLM_API void XPLMGetPrefsPath(char* p) { std::strcpy(p, "/nonexistent-aeroacars-attrappe/X-Plane.prf"); }
XPLM_API const char* XPLMGetDirectorySeparator(void) { return "/"; }
XPLM_API char* XPLMExtractFileAndPath(char* p) {
    char* sl = std::strrchr(p, '/');
    if (sl == nullptr) return p;
    *sl = '\0';
    return sl + 1;
}
XPLM_API void XPLMGetFontDimensions(XPLMFontID, int* w, int* h, int* d) {
    if (w) *w = 6;
    if (h) *h = 9;
    if (d) *d = 0;
}
XPLM_API float XPLMMeasureString(XPLMFontID, const char*, int n) { return 6.0f * static_cast<float>(n); }
XPLM_API void XPLMDrawString(float*, int, int, const char*, int*, XPLMFontID) {}
XPLM_API void XPLMSetGraphicsState(int, int, int, int, int, int, int) {}
XPLM_API void XPLMDrawTranslucentDarkBox(int, int, int, int) {}
XPLM_API void XPLMBindTexture2d(int, int) {}
XPLM_API void XPLMGenerateTextureNumbers(int* ids, int n) {
    static int naechste = 1;
    for (int i = 0; i < n; ++i) ids[i] = naechste++;
}
XPLM_API int XPLMHasFeature(const char*) { return 1; }
XPLM_API void XPLMEnableFeature(const char*, int) {}
XPLM_API XPLMWindowID XPLMCreateWindowEx(XPLMCreateWindow_t*) { static int fenster; return &fenster; }
XPLM_API void XPLMDestroyWindow(XPLMWindowID) {}
XPLM_API void XPLMGetWindowGeometry(XPLMWindowID, int* l, int* t, int* r, int* b) {
    if (l) *l = 0;
    if (t) *t = 0;
    if (r) *r = 0;
    if (b) *b = 0;
}
XPLM_API void XPLMSetWindowGeometry(XPLMWindowID, int, int, int, int) {}
XPLM_API int XPLMGetWindowIsVisible(XPLMWindowID) { return 0; }
XPLM_API void XPLMSetWindowIsVisible(XPLMWindowID, int) {}
XPLM_API void XPLMGetScreenBoundsGlobal(int* l, int* t, int* r, int* b) {
    if (l) *l = 0;
    if (t) *t = 1080;
    if (r) *r = 1920;
    if (b) *b = 0;
}
XPLM_API void XPLMGetMouseLocationGlobal(int* x, int* y) {
    if (x) *x = 0;
    if (y) *y = 0;
}
XPLM_API XPLMMenuID XPLMFindPluginsMenu(void) { static int menue; return &menue; }
XPLM_API XPLMMenuID XPLMCreateMenu(const char*, XPLMMenuID, int, XPLMMenuHandler_f, void*) { static int menue2; return &menue2; }
XPLM_API void XPLMDestroyMenu(XPLMMenuID) {}
XPLM_API int XPLMAppendMenuItem(XPLMMenuID, const char*, void*, int) { static int n = 0; return n++; }
XPLM_API void XPLMCheckMenuItem(XPLMMenuID, int, XPLMMenuCheck) {}
XPLM_API void XPLMRemoveMenuItem(XPLMMenuID, int) {}
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
    const bool zyklus = argc > 3 && std::strcmp(argv[3], "zyklus") == 0;

    // Welt: die Datarefs von Protokoll 1, die Flugzeug-Kennung, ein paar
    // typische Namen und viele Füllnamen für LISTE.
    neu("sim/flightmodel/position/latitude", xplmType_Double | xplmType_Float)->d = 50.03456789012345;
    neu("sim/flightmodel/position/longitude", xplmType_Double | xplmType_Float)->d = 8.571234567890123;
    neu("sim/flightmodel/position/y_agl", xplmType_Float)->f = 1000.0f;
    neu("sim/flightmodel/position/local_vy", xplmType_Float)->f = -2.0f;
    // Auch als double registriert: Ein Protokoll-2-Abo (der echte Client
    // abonniert fnrml_gear mit 50 Hz) liest dann per XPLMGetDatad — nur das
    // Lesen per XPLMGetDataf (Protokoll 1) zaehlt als Tick. Sonst meldete die
    // Attrappe mit echtem Client ~47 Hz „Protokoll 1“ (29.09.2026).
    g_fnrml = neu("sim/flightmodel/forces/fnrml_gear", xplmType_Float | xplmType_Double);
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

    // Zyklus: die Attrappe als Client von Protokoll 2 (eigener Port).
    const int cl = socket(AF_INET, SOCK_DGRAM, 0);
    sockaddr_in steuer{};
    steuer.sin_family = AF_INET;
    steuer.sin_port = htons(52001);
    steuer.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    fcntl(cl, F_SETFL, fcntl(cl, F_GETFL, 0) | O_NONBLOCK);
    auto an_plugin = [&](const char* text) {
        sendto(cl, text, std::strlen(text), 0, reinterpret_cast<sockaddr*>(&steuer), sizeof(steuer));
    };
    // Lässt sich 127.0.0.1:52001 gerade binden (= Plugin hat ihn nicht)?
    auto port_frei = [&]() {
        const int t = socket(AF_INET, SOCK_DGRAM, 0);
        const bool frei = bind(t, reinterpret_cast<sockaddr*>(&steuer), sizeof(steuer)) == 0;
        close(t);
        return frei;
    };
    struct Antwort { double t; std::string text; };
    std::vector<Antwort> antworten;
    bool z_hallo1 = false, z_aus = false, z_ein = false, z_ping2 = false, z_hallo2 = false;
    bool z_start = false, z_landung = false;
    bool frei_nach_aus = false, belegt_nach_ein = false;
    int p1_ticks_ein = -1, p1_pakete_ein = -1;

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
    int aufrufe = 0, jeder_frame = 0, p1_pakete = 0, p1_mit_pv = 0;
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
        if (zyklus) {
            if (!z_hallo1 && t > 1.0) { z_hallo1 = true; an_plugin("HALLO 2 attrappe"); }
            if (!z_aus && t > 2.0) {
                z_aus = true;
                disable();
                // Während das Plugin aus ist: gelandet.
                such("sim/flightmodel/position/y_agl")->f = 0.0f;
                g_fnrml->f = 50000.0f;
                frei_nach_aus = port_frei();
                an_plugin("PING");  // geht ins Leere
                std::printf("[Attrappe] Zyklus: XPluginDisable, Port 52001 %s\n", frei_nach_aus ? "frei" : "BELEGT");
            }
            if (!z_ein && t > 3.0) {
                z_ein = true;
                if (enable() != 1) std::printf("[Attrappe] Zyklus: XPluginEnable gab nicht 1 zurueck\n");
                belegt_nach_ein = !port_frei();
                p1_ticks_ein = g_p1_ticks;
                p1_pakete_ein = p1_pakete;
                naechster_aufruf = t;
                std::printf("[Attrappe] Zyklus: XPluginEnable, Port 52001 %s\n", belegt_nach_ein ? "gebunden" : "FREI");
            }
            if (!z_ping2 && t > 3.5) { z_ping2 = true; an_plugin("PING"); }
            if (!z_start && t > 4.5) {  // Start: wieder in der Luft
                z_start = true;
                such("sim/flightmodel/position/y_agl")->f = 100.0f;
                g_fnrml->f = 0.0f;
            }
            if (!z_landung && t > 5.2) {  // echte Landung
                z_landung = true;
                such("sim/flightmodel/position/y_agl")->f = 0.0f;
                g_fnrml->f = 50000.0f;
            }
            if (!z_hallo2 && t > 4.0) { z_hallo2 = true; an_plugin("HALLO 2 attrappe"); }
            char a[9000];
            for (;;) {
                const auto m = recv(cl, a, sizeof(a) - 1, 0);
                if (m <= 0) break;
                antworten.push_back({t, std::string(a, static_cast<size_t>(m))});
            }
        }
        // Wie X-Plane: Callbacks eines abgeschalteten Plugins laufen nicht.
        const bool abgeschaltet = zyklus && z_aus && !z_ein;
        if (g_cb && !abgeschaltet && t >= naechster_aufruf) {
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
        for (;;) {
            const auto n = p1_da ? recv(p1, puffer, sizeof(puffer) - 1, 0) : -1;
            if (n <= 0) break;
            ++p1_pakete;
            puffer[n] = '\0';
            if (std::strstr(puffer, "\"pv\":\"") != nullptr) ++p1_mit_pv;
        }
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
    close(cl);

    bool zyklus_ok = true;
    if (zyklus) {
        auto gesehen = [&](double von, double bis, const char* teil) {
            for (const auto& a : antworten) {
                if (a.t >= von && a.t < bis && a.text.find(teil) != std::string::npos) return true;
            }
            return false;
        };
        const bool hallo1 = gesehen(1.0, 2.0, "\"t\":\"hallo\"");
        const bool nichts_aus = !gesehen(2.0, 3.0, "\"p\":2");
        const bool kein_hallo = gesehen(3.5, 4.0, "kein_hallo");
        const bool hallo2 = gesehen(4.0, laufzeit + 1.0, "\"t\":\"hallo\"");
        const bool p1_weiter = g_p1_ticks > p1_ticks_ein && (!p1_da || p1_pakete > p1_pakete_ein);
        std::printf("[Attrappe] Zyklus: hallo=%d, Port frei nach Disable=%d, still waehrend aus=%d, "
                    "gebunden nach Enable=%d, kein_hallo danach=%d, neues hallo=%d, Protokoll 1 weiter=%d\n",
                    hallo1, frei_nach_aus, nichts_aus, belegt_nach_ein, kein_hallo, hallo2, p1_weiter);
        int schein = 0, echt = 0;
        for (double t : g_aufsetzer) {
            if (t >= 2.0 && t < 4.5) ++schein;
            if (t >= 5.2) ++echt;
        }
        std::printf("[Attrappe] Zyklus: Touchdown nach Enable am Boden=%d (Soll 0), nach echter Landung=%d (Soll 1)\n",
                    schein, echt);
        zyklus_ok = hallo1 && frei_nach_aus && nichts_aus && belegt_nach_ein && kein_hallo && hallo2 &&
                    p1_weiter && schein == 0 && echt == 1;
        std::printf("[Attrappe] Zyklus %s\n", zyklus_ok ? "gruen" : "ROT");
    }

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
    // Im Zyklus steht das Flugzeug zeitweise am Boden (unter 200 ft läuft
    // Protokoll 1 absichtlich jeden Frame) — dort nur die Untergrenze.
    const double obergrenze = zyklus ? 1e9 : 21.5;
    const bool ticks_ok = tick_hz > 5.0 && tick_hz < obergrenze;
    const bool pakete_ok = !p1_da || (paket_hz > 5.0 && paket_hz < obergrenze);
    // Protokoll 1 trägt seit 1.0.0 die Plugin-Version ("pv") in jedem Paket.
    const bool pv_ok = !p1_da || p1_pakete == p1_mit_pv;
    if (p1_da) std::printf("[Attrappe] Protokoll 1: %d von %d Paketen mit \"pv\"\n", p1_mit_pv, p1_pakete);
    return (ticks_ok && pakete_ok && pv_ok && zyklus_ok) ? 0 : 1;
}
