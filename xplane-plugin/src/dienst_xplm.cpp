// =============================================================================
// AeroACARS X-Plane-Plugin — Protokoll 2 an XPLM und Socket angebunden
// =============================================================================
//
// Hier steht alles, was X-Plane oder das Betriebssystem braucht; die Logik
// liegt im Dienst (dienst.cpp) und ist dort ohne X-Plane getestet.
//
// Drei Stellen verdienen Aufmerksamkeit:
//
// 1. XPLM-4.0-Funktionen (für LISTE) werden NICHT direkt aufgerufen, sondern
//    zur Laufzeit per XPLMFindSymbol geholt — und nur, wenn XPLMGetVersions
//    mindestens XPLM 400 meldet. Ein direkter Aufruf würde unter Windows eine
//    Import-Abhängigkeit auf XPLM_64.dll erzeugen, die X-Plane 11 nicht
//    erfüllt: Das Plugin ließe sich dort gar nicht mehr laden. Unter macOS
//    und Linux (Symbole erst zur Laufzeit gebunden) wäre es schlimmer: Laden
//    klappt, der erste Aufruf bringt X-Plane zum Absturz.
//
// 2. Der Steuer-Socket bindet nur 127.0.0.1 und prüft trotzdem jede
//    Absenderadresse. Eingerichtet wird er von netz::oeffne (netz.h): Unter
//    Windows verhindert SO_EXCLUSIVEADDRUSE, dass ein anderes Programm
//    denselben Port mitbindet und Anfragen abgreift, und SIO_UDP_CONNRESET =
//    aus, dass ein ICMP "Port unreachable" (Client beendet) jeden folgenden
//    recvfrom() mit WSAECONNRESET abbricht. Jede Rückgabe wird geprüft;
//    scheitert "nicht blockierend" oder die exklusive Bindung, bleibt
//    Protokoll 2 aus (Codex-Abnahme H5).
//
// 3. Kein eigener Thread: Empfangen und Senden laufen nicht blockierend im
//    Flight-Loop; alle XPLM-Aufrufe bleiben im Hauptthread.
// =============================================================================

#include "dienst_xplm.h"

#include "dienst.h"
#include "grenzen.h"
#include "netz.h"

#include <XPLM/XPLMDataAccess.h>
#include <XPLM/XPLMDefs.h>
#include <XPLM/XPLMPlugin.h>
#include <XPLM/XPLMUtilities.h>

#include <cerrno>
#include <chrono>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <new>

#if IBM
    #include <winsock2.h>
    #include <ws2tcpip.h>
    namespace { using socket_t = SOCKET; using socklen_x = int; }
    #define AA_INVALID_SOCK INVALID_SOCKET
#else
    #include <arpa/inet.h>
    #include <netinet/in.h>
    #include <sys/socket.h>
    #include <sys/types.h>
    #include <unistd.h>
    namespace { using socket_t = int; using socklen_x = socklen_t; }
    #define AA_INVALID_SOCK (-1)
#endif

using namespace aeroacars;

// Die Typ-Bits des Kerns müssen zu X-Plane passen.
static_assert(typ::I == xplmType_Int, "typ::I");
static_assert(typ::F == xplmType_Float, "typ::F");
static_assert(typ::D == xplmType_Double, "typ::D");
static_assert(typ::VF == xplmType_FloatArray, "typ::VF");
static_assert(typ::VI == xplmType_IntArray, "typ::VI");
static_assert(typ::B == xplmType_Data, "typ::B");
static_assert(sizeof(DatarefHandle) == sizeof(XPLMDataRef), "Handle-Größe");

namespace {

// ---- Log ---------------------------------------------------------------------

void log_zeile(const char* text) noexcept {
    char zeile[640];
    std::snprintf(zeile, sizeof(zeile), "[AeroACARS] %s\n", text ? text : "");
    XPLMDebugString(zeile);
}

// ---- XPLM 4.0 zur Laufzeit ---------------------------------------------------

// Eigene Kopie von XPLMDataRefInfo_t (XPLM400). Wir definieren XPLM400 bewusst
// NICHT, damit niemand die Funktionen versehentlich direkt aufruft (siehe 1.).
// structSize teilt X-Plane mit, welche Fassung wir kennen.
struct XpDatarefInfo {
    int structSize;
    const char* name;
    XPLMDataTypeID type;
    int writable;
    XPLMPluginID owner;
};
using CountDataRefsFn = int (*)(void);
using GetDataRefsByIndexFn = void (*)(int, int, XPLMDataRef*);
using GetDataRefInfoFn = void (*)(XPLMDataRef, XpDatarefInfo*);

CountDataRefsFn g_count = nullptr;
GetDataRefsByIndexFn g_by_index = nullptr;
GetDataRefInfoFn g_info = nullptr;

template <typename Fn>
Fn hole_symbol(const char* name) noexcept {
    void* p = XPLMFindSymbol(name);
    Fn fn = nullptr;
    if (p != nullptr) std::memcpy(&fn, &p, sizeof(fn));
    return fn;
}

// ---- Datenquelle über XPLM ---------------------------------------------------

class XplmQuelle final : public Datenquelle {
public:
    DatarefHandle finde(const char* name) noexcept override {
        return XPLMFindDataRef(name);
    }
    bool ist_gueltig(DatarefHandle h) noexcept override {
        return h != nullptr && XPLMIsDataRefGood(h) != 0;
    }
    int typen(DatarefHandle h) noexcept override { return XPLMGetDataRefTypes(h); }
    int lese_i(DatarefHandle h) noexcept override { return XPLMGetDatai(h); }
    float lese_f(DatarefHandle h) noexcept override { return XPLMGetDataf(h); }
    double lese_d(DatarefHandle h) noexcept override { return XPLMGetDatad(h); }
    int lese_vi(DatarefHandle h, int* aus, int ab, int max) noexcept override {
        return XPLMGetDatavi(h, aus, ab, max);
    }
    int lese_vf(DatarefHandle h, float* aus, int ab, int max) noexcept override {
        return XPLMGetDatavf(h, aus, ab, max);
    }
    int lese_b(DatarefHandle h, void* aus, int ab, int max) noexcept override {
        return XPLMGetDatab(h, aus, ab, max);
    }
    bool liste_verfuegbar() noexcept override {
        return g_count != nullptr && g_by_index != nullptr && g_info != nullptr;
    }
    int anzahl_datarefs() noexcept override {
        return g_count ? g_count() : 0;
    }
    int datarefs_ab(int ab, int anzahl, DatarefHandle* aus) noexcept override {
        if (!g_by_index || anzahl <= 0) return 0;
        std::memset(aus, 0, sizeof(DatarefHandle) * static_cast<size_t>(anzahl));
        g_by_index(ab, anzahl, reinterpret_cast<XPLMDataRef*>(aus));
        return anzahl;
    }
    const char* name_von(DatarefHandle h) noexcept override {
        if (!g_info || h == nullptr) return nullptr;
        XpDatarefInfo info;
        std::memset(&info, 0, sizeof(info));
        info.structSize = static_cast<int>(sizeof(info));
        g_info(h, &info);
        return info.name;
    }
};

// ---- Umgebung: Socket, Uhr, Log ----------------------------------------------

socket_t g_sock = AA_INVALID_SOCK;
#if IBM
bool g_wsa = false;
#endif

int sock_fehler() noexcept {
#if IBM
    return WSAGetLastError();
#else
    return errno;
#endif
}

bool ist_voll(int e) noexcept {
#if IBM
    return e == WSAEWOULDBLOCK || e == WSAENOBUFS;
#else
    return e == EAGAIN || e == EWOULDBLOCK || e == ENOBUFS;
#endif
}

class SocketUmgebung final : public Umgebung {
public:
    SendeErgebnis sende(const Absender& an, const char* daten, size_t laenge) noexcept override {
        if (g_sock == AA_INVALID_SOCK || daten == nullptr || laenge == 0 ||
            laenge > grenzen::MAX_PAKET) {
            return SendeErgebnis::FEHLER;
        }
        sockaddr_in ziel;
        std::memset(&ziel, 0, sizeof(ziel));
        ziel.sin_family = AF_INET;
        ziel.sin_port = htons(an.port);
        ziel.sin_addr.s_addr = htonl(an.ip);
        const auto r = ::sendto(g_sock, daten, static_cast<int>(laenge), 0,
                                reinterpret_cast<const sockaddr*>(&ziel),
                                static_cast<socklen_x>(sizeof(ziel)));
        if (r >= 0) return SendeErgebnis::OK;
        return ist_voll(sock_fehler()) ? SendeErgebnis::VOLL : SendeErgebnis::FEHLER;
    }
    double jetzt() noexcept override {
        using namespace std::chrono;
        return duration<double>(steady_clock::now().time_since_epoch()).count();
    }
    void protokolliere(const char* zeile) noexcept override { log_zeile(zeile); }
};

XplmQuelle g_quelle;
SocketUmgebung g_umgebung;
Dienst* g_dienst = nullptr;

// Empfangspuffer: ein Byte mehr als erlaubt, damit ein zu großes Datagramm
// als solches erkannt wird (statt still abgeschnitten).
char g_empfang[grenzen::MAX_ANFRAGE_BYTES + 1];
bool g_fremd_gemeldet = false;

void schliesse_socket() noexcept {
    if (g_sock != AA_INVALID_SOCK) {
        netz::os_ops().schliesse(static_cast<netz::Sock>(g_sock));
        g_sock = AA_INVALID_SOCK;
    }
#if IBM
    if (g_wsa) {
        WSACleanup();
        g_wsa = false;
    }
#endif
}

bool oeffne_socket() noexcept {
#if IBM
    WSADATA wsa;
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) {
        log_zeile("Protokoll 2: WSAStartup gescheitert - Protokoll 2 aus");
        return false;
    }
    g_wsa = true;
#endif
    // Jede Rückgabe geprüft, Regel und Begründung in netz.h (H5).
    const netz::Sock s = netz::oeffne(netz::os_ops(), netz::Art::STEUER, grenzen::STEUER_PORT,
                                      "Protokoll 2", &log_zeile);
    if (s == netz::KEIN_SOCKET) {
        schliesse_socket();  // nur noch WSACleanup
        return false;
    }
    g_sock = static_cast<socket_t>(s);
    return true;
}

void empfange_alles() noexcept {
    for (int i = 0; i < grenzen::MAX_DATAGRAMME_JE_FRAME; ++i) {
        sockaddr_in von;
        std::memset(&von, 0, sizeof(von));
        socklen_x von_laenge = static_cast<socklen_x>(sizeof(von));
        const auto n = ::recvfrom(g_sock, g_empfang, static_cast<int>(sizeof(g_empfang)), 0,
                                  reinterpret_cast<sockaddr*>(&von), &von_laenge);
        if (n < 0) {
            const int e = sock_fehler();
#if IBM
            // Einzelne kaputte Datagramme überspringen, nicht den Empfang
            // für diesen Frame abbrechen.
            if (e == WSAECONNRESET || e == WSAEMSGSIZE) continue;
#endif
            (void)e;
            break;  // nichts mehr da (oder Fehler) — nächster Frame
        }
        if (static_cast<size_t>(von_laenge) < sizeof(sockaddr_in) ||
            von.sin_family != AF_INET ||
            ntohl(von.sin_addr.s_addr) != INADDR_LOOPBACK) {
            if (!g_fremd_gemeldet) {
                g_fremd_gemeldet = true;
                log_zeile("Protokoll 2: Datagramm von fremder Adresse verworfen (nur 127.0.0.1 erlaubt)");
            }
            continue;
        }
        Absender a;
        a.ip = ntohl(von.sin_addr.s_addr);
        a.port = ntohs(von.sin_port);
        g_dienst->empfange(a, g_empfang, static_cast<size_t>(n));
    }
}

// Misst beim Start einmal grob, was die XPLM-Aufrufe kosten, auf die sich
// Such- und Lieferbudget stützen, und schreibt es ins Log.txt. Die Budgets
// sind Zeitbudgets und brauchen das nicht — aber so lässt sich am echten
// X-Plane ablesen, wie viele Namen je Frame tatsächlich gesucht und wie teuer
// die Verwaist-Prüfung vor jedem Lesen ist. Kostet einmalig ≈ 1–3 ms.
void messe_xplm_kosten() noexcept {
    using namespace std::chrono;
    constexpr int N = 500;
    const XPLMDataRef lat = XPLMFindDataRef("sim/flightmodel/position/latitude");
    auto mikro = [](steady_clock::time_point a, steady_clock::time_point b) {
        return duration<double, std::micro>(b - a).count() / N;
    };
    const auto t0 = steady_clock::now();
    for (int i = 0; i < N; ++i) (void)XPLMFindDataRef("sim/flightmodel/position/latitude");
    const auto t1 = steady_clock::now();
    for (int i = 0; i < N; ++i) (void)XPLMFindDataRef("aeroacars/messung/gibt_es_nicht");
    const auto t2 = steady_clock::now();
    int gut = 0;
    if (lat != nullptr) {
        for (int i = 0; i < N; ++i) gut += XPLMIsDataRefGood(lat);
    }
    const auto t3 = steady_clock::now();
    char t[256];
    std::snprintf(t, sizeof(t),
                  "Protokoll 2: Kosten je Aufruf - XPLMFindDataRef %.3f us (Treffer) / %.3f us "
                  "(Fehlgriff), XPLMIsDataRefGood %.3f us",
                  mikro(t0, t1), mikro(t1, t2), lat ? mikro(t2, t3) : -1.0);
    log_zeile(t);
    (void)gut;
}

}  // namespace

bool dienst_start(const char* plugin_version) noexcept {
    if (g_dienst != nullptr) return true;

    int xplane = 0, xplm = 0;
    XPLMHostApplicationID host = 0;
    XPLMGetVersions(&xplane, &xplm, &host);

    // LISTE nur mit XPLM 4.0+ UND allen drei Symbolen (siehe 1. oben).
    g_count = nullptr;
    g_by_index = nullptr;
    g_info = nullptr;
    if (xplm >= 400) {
        g_count = hole_symbol<CountDataRefsFn>("XPLMCountDataRefs");
        g_by_index = hole_symbol<GetDataRefsByIndexFn>("XPLMGetDataRefsByIndex");
        g_info = hole_symbol<GetDataRefInfoFn>("XPLMGetDataRefInfo");
        if (!g_count || !g_by_index || !g_info) {
            g_count = nullptr;
            g_by_index = nullptr;
            g_info = nullptr;
        }
    }

    Kennung kennung;
    kennung.plugin_version = plugin_version;
    kennung.xplane_version = xplane;
    kennung.xplm_version = xplm;
    g_dienst = new (std::nothrow) Dienst(g_quelle, g_umgebung, kennung);
    if (g_dienst == nullptr || !g_dienst->bereit()) {
        log_zeile("Protokoll 2: kein Speicher - Protokoll 2 aus");
        delete g_dienst;
        g_dienst = nullptr;
        return false;
    }
    if (!oeffne_socket()) {
        delete g_dienst;
        g_dienst = nullptr;
        return false;
    }
    g_fremd_gemeldet = false;
    messe_xplm_kosten();

    char t[200];
    std::snprintf(t, sizeof(t),
                  "Protokoll 2 bereit auf 127.0.0.1:%u (X-Plane %d, XPLM %d, LISTE %s)",
                  static_cast<unsigned>(grenzen::STEUER_PORT), xplane, xplm,
                  g_count ? "ja" : "nein");
    log_zeile(t);
    return true;
}

void dienst_stopp() noexcept {
    delete g_dienst;  // gibt alle Abos frei
    g_dienst = nullptr;
    schliesse_socket();
    g_count = nullptr;
    g_by_index = nullptr;
    g_info = nullptr;
}

bool dienst_frame() noexcept {
    if (g_dienst == nullptr || g_sock == AA_INVALID_SOCK) return false;
    empfange_alles();
    g_dienst->frame();
    return g_dienst->braucht_jeden_frame();
}

void dienst_nachricht(int nachricht, void* parameter) noexcept {
    if (g_dienst == nullptr) return;
    if (nachricht == XPLM_MSG_PLANE_LOADED) {
        // Parameter = Flugzeugindex als Zeiger; 0 = Nutzerflugzeug. KI-Flugzeuge
        // ändern die Datarefs des Nutzerflugzeugs nicht.
        if (reinterpret_cast<intptr_t>(parameter) == 0) g_dienst->flugzeug_geladen();
    } else if (nachricht == XPLM_MSG_AIRPORT_LOADED) {
        g_dienst->flughafen_geladen();
    }
}
