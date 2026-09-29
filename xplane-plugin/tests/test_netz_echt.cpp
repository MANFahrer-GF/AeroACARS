// =============================================================================
// Socket-Einrichtung gegen das ECHTE Betriebssystem (Codex-Abnahme H5)
// =============================================================================
//
//   aeroacars_netz_echt
//
// Läuft auf allen drei Plattformen, auch unter Windows — dort gibt es keinen
// Ende-zu-Ende-Test mit der .xpl (die Attrappe braucht dlopen). Geprüft wird
// genau das, was netz_os.cpp dem Plugin verspricht:
//   1. Der Steuer-Socket ist wirklich nicht blockierend: recvfrom() auf einem
//      leeren Socket kehrt sofort mit "würde blockieren" zurück.
//   2. Ein zweiter Steuer-Socket auf demselben Port scheitert (unter Windows
//      mit SO_EXCLUSIVEADDRUSE) — mit Log-Zeile "belegt", ohne Leck.
//   3. Der Sender-Socket (Protokoll 1) ist nicht blockierend und erreicht den
//      Steuer-Socket über Loopback.
// Endet mit 0 (grün) oder 1 (rot).
// =============================================================================

#include "netz.h"

#include <chrono>
#include <cstdio>
#include <cstring>
#include <string>
#include <thread>

#if IBM
    #include <winsock2.h>
    #include <ws2tcpip.h>
    using nativ_t = SOCKET;
    using laenge_t = int;
#else
    #include <arpa/inet.h>
    #include <cerrno>
    #include <netinet/in.h>
    #include <sys/socket.h>
    #include <unistd.h>
    using nativ_t = int;
    using laenge_t = socklen_t;
#endif

using namespace aeroacars;

namespace {

int g_rot = 0;
std::string g_log;

void log_fn(const char* z) {
    g_log += z;
    g_log += "\n";
    std::printf("  [log] %s\n", z);
}

void pruefe(bool ok, const char* was) {
    std::printf("%s %s\n", ok ? "ok  " : "ROT ", was);
    if (!ok) ++g_rot;
}

bool wuerde_blockieren() {
#if IBM
    return WSAGetLastError() == WSAEWOULDBLOCK;
#else
    return errno == EAGAIN || errno == EWOULDBLOCK;
#endif
}

nativ_t nativ(netz::Sock s) { return static_cast<nativ_t>(s); }

}  // namespace

int main() {
#if IBM
    WSADATA wsa;
    if (WSAStartup(MAKEWORD(2, 2), &wsa) != 0) {
        std::printf("ROT WSAStartup\n");
        return 1;
    }
#endif
    netz::Ops& ops = netz::os_ops();

    // 1. Steuer-Socket auf einem freien Port (0 = das System wählt).
    const netz::Sock s = netz::oeffne(ops, netz::Art::STEUER, 0, "Test", &log_fn);
    pruefe(s != netz::KEIN_SOCKET, "Steuer-Socket geoeffnet");
    if (s == netz::KEIN_SOCKET) return 1;
    sockaddr_in adr;
    std::memset(&adr, 0, sizeof(adr));
    laenge_t len = static_cast<laenge_t>(sizeof(adr));
    ::getsockname(nativ(s), reinterpret_cast<sockaddr*>(&adr), &len);
    const uint16_t port = ntohs(adr.sin_port);
    pruefe(port != 0 && ntohl(adr.sin_addr.s_addr) == INADDR_LOOPBACK, "gebunden an 127.0.0.1");

    char puffer[256];
    const auto t0 = std::chrono::steady_clock::now();
    const auto n = ::recvfrom(nativ(s), puffer, static_cast<int>(sizeof(puffer)), 0, nullptr, nullptr);
    const double ms = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
    std::printf("     recvfrom auf leerem Socket: %.3f ms\n", ms);
    pruefe(n < 0 && wuerde_blockieren() && ms < 100.0, "recvfrom blockiert nicht");

    // 2. Derselbe Port noch einmal: muss scheitern, Log "belegt".
    g_log.clear();
    const netz::Sock zweit = netz::oeffne(ops, netz::Art::STEUER, port, "Test", &log_fn);
    pruefe(zweit == netz::KEIN_SOCKET, "zweiter Steuer-Socket auf demselben Port scheitert");
    pruefe(g_log.find("belegt") != std::string::npos, "Log nennt den belegten Port");
    if (zweit != netz::KEIN_SOCKET) ops.schliesse(zweit);

    // 3. Sender (Protokoll 1) → Steuer-Socket über Loopback.
    const netz::Sock sender = netz::oeffne(ops, netz::Art::SENDER, 0, "Test", &log_fn);
    pruefe(sender != netz::KEIN_SOCKET, "Sender-Socket geoeffnet");
    if (sender != netz::KEIN_SOCKET) {
        sockaddr_in ziel;
        std::memset(&ziel, 0, sizeof(ziel));
        ziel.sin_family = AF_INET;
        ziel.sin_port = htons(port);
        ziel.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        const char text[] = "PING";
        const auto r = ::sendto(nativ(sender), text, 4, 0, reinterpret_cast<const sockaddr*>(&ziel),
                                static_cast<laenge_t>(sizeof(ziel)));
        pruefe(r == 4, "sendto ueber Loopback");
        bool da = false;
        for (int i = 0; i < 100 && !da; ++i) {
            const auto m = ::recvfrom(nativ(s), puffer, static_cast<int>(sizeof(puffer)), 0, nullptr, nullptr);
            if (m == 4 && std::memcmp(puffer, text, 4) == 0) da = true;
            else std::this_thread::sleep_for(std::chrono::milliseconds(10));
        }
        pruefe(da, "Steuer-Socket empfaengt das Datagramm");
        // Sender ist ebenfalls nicht blockierend: recvfrom kehrt sofort zurück.
        const auto t1 = std::chrono::steady_clock::now();
        const auto k = ::recvfrom(nativ(sender), puffer, static_cast<int>(sizeof(puffer)), 0, nullptr, nullptr);
        const double ms2 = std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t1).count();
        pruefe(k < 0 && ms2 < 100.0, "Sender-Socket blockiert nicht");
        ops.schliesse(sender);
    }
    ops.schliesse(s);
#if IBM
    WSACleanup();
#endif
    std::printf("\n%s (%d rot)\n", g_rot == 0 ? "gruen" : "rot", g_rot);
    return g_rot == 0 ? 0 : 1;
}
