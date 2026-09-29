// =============================================================================
// AeroACARS X-Plane-Plugin — Sockets einrichten (Regel, ohne Betriebssystem)
// =============================================================================
//
// Begründung der Reihenfolge und jeder Entscheidung: netz.h.
// =============================================================================

#include "netz.h"

#include <cstdio>

namespace aeroacars {
namespace netz {

namespace {

void melde(LogFn log, const char* wer, const char* was, int fehler, const char* folge) noexcept {
    if (log == nullptr) return;
    char zeile[256];
    std::snprintf(zeile, sizeof(zeile), "%s: %s (Fehler %d) - %s", wer ? wer : "?", was, fehler,
                  folge);
    log(zeile);
}

}  // namespace

Sock oeffne(Ops& ops, Art art, uint16_t port, const char* wer, LogFn log) noexcept {
    const bool steuer = art == Art::STEUER;
    const char* aus = steuer ? "Protokoll 2 aus" : "Protokoll 1 aus";

    const Sock s = ops.udp_socket();
    if (s == KEIN_SOCKET) {
        melde(log, wer, "socket() gescheitert", ops.fehler(), aus);
        return KEIN_SOCKET;
    }
    auto aufgeben = [&](const char* was) noexcept {
        const int e = ops.fehler();  // vor dem Schließen lesen
        ops.schliesse(s);
        melde(log, wer, was, e, aus);
        return KEIN_SOCKET;
    };

    // Vor bind(): SO_EXCLUSIVEADDRUSE wirkt nur auf einen noch ungebundenen Socket.
    if (steuer && !ops.exklusiv(s)) {
        return aufgeben("exklusive Portbindung (SO_EXCLUSIVEADDRUSE) nicht setzbar");
    }
    if (steuer && !ops.connreset_aus(s)) {
        melde(log, wer, "SIO_UDP_CONNRESET nicht abschaltbar", ops.fehler(),
              "weiter; beendete Clients erzeugen einzelne Empfangsfehler");
    }
    if (!ops.nicht_blockierend(s)) {
        return aufgeben("Socket nicht auf nicht-blockierend stellbar");
    }
    if (steuer) {
        ops.puffer(s, 1 << 20, 1 << 18);
        if (!ops.binde_loopback(s, port)) {
            char was[96];
            std::snprintf(was, sizeof(was), "Port 127.0.0.1:%u belegt", static_cast<unsigned>(port));
            const int e = ops.fehler();
            ops.schliesse(s);
            // Wortlaut prüft tests/attrappe/ende_zu_ende.py ("belegt",
            // "Protokoll 1 laeuft weiter").
            melde(log, wer, was, e, "Protokoll 2 aus, Protokoll 1 laeuft weiter");
            return KEIN_SOCKET;
        }
    }
    return s;
}

}  // namespace netz
}  // namespace aeroacars
