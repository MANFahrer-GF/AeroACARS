// =============================================================================
// AeroACARS X-Plane-Plugin — Sockets: die echten Betriebssystemaufrufe
// =============================================================================
//
// Dünne Umsetzung von netz::Ops für Winsock (IBM) und POSIX. Keine Regel hier
// — was bei welchem Fehler geschieht, entscheidet netz::oeffne (netz.cpp).
// Jede Funktion meldet ehrlich, ob der Aufruf gelungen ist.
//
// WSAStartup/WSACleanup ruft der jeweilige Besitzer des Sockets selbst
// (plugin.cpp für Protokoll 1, dienst_xplm.cpp für Protokoll 2), paarweise.
// =============================================================================

#include "netz.h"

#include <cerrno>
#include <cstring>

#if IBM
    #include <winsock2.h>
    #include <ws2tcpip.h>
    #ifndef SIO_UDP_CONNRESET
        #define SIO_UDP_CONNRESET _WSAIOW(IOC_VENDOR, 12)
    #endif
#else
    #include <arpa/inet.h>
    #include <fcntl.h>
    #include <netinet/in.h>
    #include <sys/socket.h>
    #include <sys/types.h>
    #include <unistd.h>
#endif

namespace aeroacars {
namespace netz {

namespace {

#if IBM
using nativ_t = SOCKET;
using laenge_t = int;
inline nativ_t nativ(Sock s) noexcept { return static_cast<nativ_t>(s); }
#else
using nativ_t = int;
using laenge_t = socklen_t;
inline nativ_t nativ(Sock s) noexcept { return static_cast<nativ_t>(s); }
#endif

class OsOps final : public Ops {
public:
    Sock udp_socket() noexcept override {
        const nativ_t s = ::socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP);
#if IBM
        if (s == INVALID_SOCKET) return KEIN_SOCKET;
#else
        if (s < 0) return KEIN_SOCKET;
#endif
        return static_cast<Sock>(s);
    }

    bool exklusiv(Sock s) noexcept override {
#if IBM
        BOOL ja = TRUE;
        return ::setsockopt(nativ(s), SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
                            reinterpret_cast<const char*>(&ja),
                            static_cast<laenge_t>(sizeof(ja))) == 0;
#else
        (void)s;
        return true;
#endif
    }

    bool connreset_aus(Sock s) noexcept override {
#if IBM
        BOOL aus = FALSE;
        DWORD bytes = 0;
        return ::WSAIoctl(nativ(s), SIO_UDP_CONNRESET, &aus, static_cast<DWORD>(sizeof(aus)),
                          nullptr, 0, &bytes, nullptr, nullptr) == 0;
#else
        (void)s;
        return true;
#endif
    }

    bool nicht_blockierend(Sock s) noexcept override {
#if IBM
        u_long ja = 1;
        return ::ioctlsocket(nativ(s), FIONBIO, &ja) == 0;
#else
        const int flags = ::fcntl(nativ(s), F_GETFL, 0);
        if (flags < 0) return false;
        if (::fcntl(nativ(s), F_SETFL, flags | O_NONBLOCK) < 0) return false;
        // Gegenprobe: gesetzt ist gesetzt, nicht nur "kein Fehler gemeldet".
        const int danach = ::fcntl(nativ(s), F_GETFL, 0);
        return danach >= 0 && (danach & O_NONBLOCK) != 0;
#endif
    }

    void puffer(Sock s, int senden, int empfangen) noexcept override {
        ::setsockopt(nativ(s), SOL_SOCKET, SO_SNDBUF, reinterpret_cast<const char*>(&senden),
                     static_cast<laenge_t>(sizeof(senden)));
        ::setsockopt(nativ(s), SOL_SOCKET, SO_RCVBUF, reinterpret_cast<const char*>(&empfangen),
                     static_cast<laenge_t>(sizeof(empfangen)));
    }

    bool binde_loopback(Sock s, uint16_t port) noexcept override {
        sockaddr_in adresse;
        std::memset(&adresse, 0, sizeof(adresse));
        adresse.sin_family = AF_INET;
        adresse.sin_port = htons(port);
        adresse.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        return ::bind(nativ(s), reinterpret_cast<const sockaddr*>(&adresse),
                      static_cast<laenge_t>(sizeof(adresse))) == 0;
    }

    void schliesse(Sock s) noexcept override {
        if (s == KEIN_SOCKET) return;
#if IBM
        ::closesocket(nativ(s));
#else
        ::close(nativ(s));
#endif
    }

    int fehler() noexcept override {
#if IBM
        return ::WSAGetLastError();
#else
        return errno;
#endif
    }
};

OsOps g_os;

}  // namespace

Ops& os_ops() noexcept { return g_os; }

}  // namespace netz
}  // namespace aeroacars
