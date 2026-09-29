// Einheitstests: Socket-Einrichtung (netz.cpp, Codex-Abnahme H5).
//
// Eine Attrappe für netz::Ops lässt jeden Betriebssystemaufruf gezielt
// scheitern — so laufen auch die Windows-Fehlerpfade (SO_EXCLUSIVEADDRUSE,
// SIO_UDP_CONNRESET, ioctlsocket(FIONBIO)) auf jeder Plattform. Die echten
// Aufrufe prüft tests/test_netz_echt.cpp (alle drei Plattformen in der CI).

#include "netz.h"
#include "testrahmen.h"

#include <string>
#include <vector>

using namespace aeroacars;

namespace {

struct Attrappe final : netz::Ops {
    bool socket_ok = true;
    bool exklusiv_ok = true;
    bool connreset_ok = true;
    bool nb_ok = true;
    bool bind_ok = true;
    std::vector<std::string> aufrufe;
    int offen = 0;  // geöffnete, noch nicht geschlossene Sockets

    netz::Sock udp_socket() noexcept override {
        aufrufe.push_back("socket");
        if (!socket_ok) return netz::KEIN_SOCKET;
        ++offen;
        return 42;
    }
    bool exklusiv(netz::Sock) noexcept override { aufrufe.push_back("exklusiv"); return exklusiv_ok; }
    bool connreset_aus(netz::Sock) noexcept override { aufrufe.push_back("connreset"); return connreset_ok; }
    bool nicht_blockierend(netz::Sock) noexcept override { aufrufe.push_back("nb"); return nb_ok; }
    void puffer(netz::Sock, int, int) noexcept override { aufrufe.push_back("puffer"); }
    bool binde_loopback(netz::Sock, uint16_t) noexcept override { aufrufe.push_back("bind"); return bind_ok; }
    void schliesse(netz::Sock s) noexcept override {
        aufrufe.push_back("close");
        if (s == 42) --offen;
    }
    int fehler() noexcept override { return 10035; }
};

std::vector<std::string>& g_log() {
    static std::vector<std::string> l;
    return l;
}
void log_fn(const char* z) { g_log().push_back(z); }

std::string reihe(const Attrappe& a) {
    std::string s;
    for (const auto& x : a.aufrufe) s += (s.empty() ? "" : ",") + x;
    return s;
}

}  // namespace

TEST(netz_steuer_socket_erfolg_und_reihenfolge) {
    Attrappe a;
    g_log().clear();
    const netz::Sock s = netz::oeffne(a, netz::Art::STEUER, 52001, "Protokoll 2", &log_fn);
    PRUEFE(s == 42);
    // Exklusiv VOR bind (wirkt nur auf ungebundene Sockets), nicht-blockierend
    // vor dem ersten recvfrom.
    PRUEFE_TEXT(reihe(a), "socket,exklusiv,connreset,nb,puffer,bind");
    PRUEFE_GLEICH(a.offen, 1);
    PRUEFE(g_log().empty());
}

TEST(netz_nicht_blockierend_scheitert_steuer_aus) {
    Attrappe a;
    a.nb_ok = false;
    g_log().clear();
    PRUEFE(netz::oeffne(a, netz::Art::STEUER, 52001, "Protokoll 2", &log_fn) == netz::KEIN_SOCKET);
    PRUEFE_GLEICH(a.offen, 0);  // geschlossen, kein Leck
    PRUEFE_TEXT(reihe(a), "socket,exklusiv,connreset,nb,close");  // nie gebunden
    PRUEFE_GLEICH(g_log().size(), size_t(1));
    PRUEFE(g_log()[0].find("nicht-blockierend") != std::string::npos);
    PRUEFE(g_log()[0].find("Protokoll 2 aus") != std::string::npos);
    PRUEFE(g_log()[0].find("10035") != std::string::npos);  // Fehlercode im Log
}

TEST(netz_nicht_blockierend_scheitert_sender_aus) {
    Attrappe a;
    a.nb_ok = false;
    g_log().clear();
    PRUEFE(netz::oeffne(a, netz::Art::SENDER, 0, "Protokoll 1", &log_fn) == netz::KEIN_SOCKET);
    PRUEFE_GLEICH(a.offen, 0);
    PRUEFE_TEXT(reihe(a), "socket,nb,close");  // Sender: kein exklusiv, kein bind
    PRUEFE(g_log()[0].find("Protokoll 1 aus") != std::string::npos);
}

TEST(netz_sender_erfolg) {
    Attrappe a;
    g_log().clear();
    PRUEFE(netz::oeffne(a, netz::Art::SENDER, 0, "Protokoll 1", &log_fn) == 42);
    PRUEFE_TEXT(reihe(a), "socket,nb");
    PRUEFE(g_log().empty());
}

TEST(netz_exklusiv_scheitert_steuer_aus) {
    Attrappe a;
    a.exklusiv_ok = false;
    g_log().clear();
    PRUEFE(netz::oeffne(a, netz::Art::STEUER, 52001, "Protokoll 2", &log_fn) == netz::KEIN_SOCKET);
    PRUEFE_GLEICH(a.offen, 0);
    PRUEFE_TEXT(reihe(a), "socket,exklusiv,close");
    PRUEFE(g_log()[0].find("SO_EXCLUSIVEADDRUSE") != std::string::npos);
}

TEST(netz_connreset_scheitert_nur_warnung) {
    Attrappe a;
    a.connreset_ok = false;
    g_log().clear();
    PRUEFE(netz::oeffne(a, netz::Art::STEUER, 52001, "Protokoll 2", &log_fn) == 42);
    PRUEFE_GLEICH(a.offen, 1);
    PRUEFE_GLEICH(g_log().size(), size_t(1));
    PRUEFE(g_log()[0].find("SIO_UDP_CONNRESET") != std::string::npos);
    PRUEFE(g_log()[0].find("weiter") != std::string::npos);
}

TEST(netz_bind_scheitert_port_belegt) {
    Attrappe a;
    a.bind_ok = false;
    g_log().clear();
    PRUEFE(netz::oeffne(a, netz::Art::STEUER, 52001, "Protokoll 2", &log_fn) == netz::KEIN_SOCKET);
    PRUEFE_GLEICH(a.offen, 0);
    // Wortlaut, den ende_zu_ende.py und Piloten im Log.txt suchen.
    PRUEFE(g_log()[0].find("Port 127.0.0.1:52001 belegt") != std::string::npos);
    PRUEFE(g_log()[0].find("Protokoll 1 laeuft weiter") != std::string::npos);
}

TEST(netz_socket_scheitert) {
    Attrappe a;
    a.socket_ok = false;
    g_log().clear();
    PRUEFE(netz::oeffne(a, netz::Art::STEUER, 52001, "Protokoll 2", &log_fn) == netz::KEIN_SOCKET);
    PRUEFE_TEXT(reihe(a), "socket");  // nichts zu schließen
    PRUEFE(g_log()[0].find("socket()") != std::string::npos);
    // Ohne Log-Funktion kein Absturz.
    PRUEFE(netz::oeffne(a, netz::Art::SENDER, 0, "x", nullptr) == netz::KEIN_SOCKET);
}
