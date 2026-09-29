// =============================================================================
// AeroACARS X-Plane-Plugin — Sockets einrichten (beide Protokolle)
// =============================================================================
//
// Die Codex-Abnahme (H5) fand: Unter Windows wurden die Rückgaben von
// ioctlsocket(FIONBIO), SO_EXCLUSIVEADDRUSE und SIO_UDP_CONNRESET ignoriert,
// beim Protokoll-1-Socket auf allen Plattformen die von O_NONBLOCK. Scheitert
// "nicht blockierend" still, kann das nächste recvfrom() den Hauptthread von
// X-Plane unbegrenzt anhalten.
//
// Deshalb richtet jetzt EINE Funktion (oeffne) beide Sockets ein, prüft jeden
// Schritt und entscheidet nach fester Regel:
//
//   Schritt                       | scheitert → …
//   ------------------------------+---------------------------------------------
//   socket()                      | aus
//   SO_EXCLUSIVEADDRUSE (Win,     | Socket zu, aus. Ohne ihn könnte ein anderes
//     nur Steuer-Socket)          | Programm den Port mitbinden und Anfragen
//                                 | abgreifen (ADR-0004 §1) — lieber kein P2.
//   SIO_UDP_CONNRESET aus (Win,   | Log-Warnung, weiter: ohne ihn meldet
//     nur Steuer-Socket)          | recvfrom() nach einem beendeten Client
//                                 | WSAECONNRESET; der Empfang überspringt das
//                                 | (dienst_xplm.cpp) — lästig, nicht gefährlich.
//   nicht blockierend             | Socket zu, aus. Ein blockierender Socket im
//                                 | Flight-Loop kann X-Plane anhalten.
//   Puffergrößen                  | egal (nur ein Wunsch an das System)
//   bind 127.0.0.1:<port>         | Socket zu, aus ("Port belegt")
//     (nur Steuer-Socket)         |
//
// "aus" heißt: Protokoll 2 bzw. Protokoll 1 bleibt abgeschaltet, eine Zeile
// steht im Log.txt, das Plugin und das jeweils andere Protokoll laufen weiter.
//
// Die Betriebssystemaufrufe stecken hinter der Schnittstelle Ops: Im Plugin
// ist das netz_os.cpp (Winsock bzw. POSIX), in den Tests eine Attrappe, die
// jeden Schritt gezielt scheitern lässt (tests/test_netz.cpp) — so sind auch
// die Windows-Fehlerpfade auf jeder Plattform getestet.
//
// Diese Datei und netz.cpp sind XPLM-frei und ohne Betriebssystem-Header.
// =============================================================================

#pragma once

#include <cstddef>
#include <cstdint>

namespace aeroacars {
namespace netz {

// Socket-Kennung plattformneutral: POSIX int bzw. Windows SOCKET (UINT_PTR);
// INVALID_SOCKET (~0) ist als intptr_t ebenfalls −1.
using Sock = std::intptr_t;
constexpr Sock KEIN_SOCKET = -1;

class Ops {
public:
    virtual Sock udp_socket() noexcept = 0;
    // Windows: SO_EXCLUSIVEADDRUSE. Andere Plattformen: true (dort bindet ohne
    // SO_REUSEADDR/SO_REUSEPORT ohnehin niemand denselben Port mit).
    virtual bool exklusiv(Sock s) noexcept = 0;
    // Windows: SIO_UDP_CONNRESET = aus. Andere Plattformen: true.
    virtual bool connreset_aus(Sock s) noexcept = 0;
    virtual bool nicht_blockierend(Sock s) noexcept = 0;
    virtual void puffer(Sock s, int senden, int empfangen) noexcept = 0;
    virtual bool binde_loopback(Sock s, uint16_t port) noexcept = 0;
    virtual void schliesse(Sock s) noexcept = 0;
    // Letzter Fehlercode (errno bzw. WSAGetLastError) — nur fürs Log.
    virtual int fehler() noexcept = 0;

protected:
    ~Ops() = default;
};

enum class Art : uint8_t {
    STEUER,  // Protokoll 2: gebunden an 127.0.0.1:<port>, empfängt und sendet
    SENDER,  // Protokoll 1: nicht gebunden, sendet nur
};

using LogFn = void (*)(const char* zeile);

// Richtet einen Socket nach der Tabelle oben ein. `wer` steht vor jeder
// Log-Zeile ("Protokoll 1"/"Protokoll 2"). Rückgabe: fertiger Socket oder
// KEIN_SOCKET (dann ist ein halb eingerichteter Socket schon geschlossen).
Sock oeffne(Ops& ops, Art art, uint16_t port, const char* wer, LogFn log) noexcept;

// Die echten Betriebssystemaufrufe (netz_os.cpp; nicht in den Kern-Tests).
Ops& os_ops() noexcept;

}  // namespace netz
}  // namespace aeroacars
