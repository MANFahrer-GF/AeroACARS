// =============================================================================
// AeroACARS X-Plane-Plugin — Paketaufteilung (Umsetzung)
// =============================================================================

#include "pakete.h"

#include <cstring>

namespace aeroacars {

namespace {

// Feste Bestandteile um Präfix, Zahlen, Feldname und Elemente herum.
constexpr size_t LEN_TEIL   = 8;  // ,"teil":
constexpr size_t LEN_TEILE  = 9;  // ,"teile":
constexpr size_t LEN_FELD_A = 2;  // ,"
constexpr size_t LEN_FELD_B = 3;  // ":[
constexpr size_t LEN_ENDE   = 3;  // ]}\n

size_t laenge_mit_stellen(size_t praefix, size_t feld,
                          size_t stellen_teil, size_t stellen_teile,
                          size_t element_bytes, size_t element_anzahl) noexcept {
    const size_t kommas = element_anzahl > 0 ? element_anzahl - 1 : 0;
    return praefix + LEN_TEIL + stellen_teil + LEN_TEILE + stellen_teile +
           LEN_FELD_A + feld + LEN_FELD_B + element_bytes + kommas + LEN_ENDE;
}

}  // namespace

bool ElementListe::uebernehme(const JsonSchreiber& w) noexcept {
    if (w.ueberlauf()) return false;
    if (enden_.anzahl() >= enden_.kapazitaet()) return false;
    const size_t neu = text_.anzahl() + w.laenge();
    if (neu > text_.kapazitaet() || neu > 0xFFFFFFFFu) return false;
    text_.setze_anzahl(neu);
    enden_.haenge_an(static_cast<uint32_t>(neu));  // Kapazität oben geprüft
    return true;
}

bool ElementListe::sorge_fuer_platz(size_t max_element) noexcept {
    if (text_.kapazitaet() - text_.anzahl() < max_element) {
        size_t neu = text_.kapazitaet() < 4096 ? 4096 : text_.kapazitaet();
        while (neu - text_.anzahl() < max_element) {
            if (neu > (SIZE_MAX / 2)) return false;
            neu *= 2;
        }
        if (!text_.reserviere(neu)) return false;
    }
    if (enden_.anzahl() == enden_.kapazitaet()) {
        const size_t neu = enden_.kapazitaet() < 256 ? 256 : enden_.kapazitaet() * 2;
        if (!enden_.reserviere(neu)) return false;
    }
    return true;
}

size_t dezimalstellen(uint32_t wert) noexcept {
    size_t n = 1;
    while (wert >= 10u) { wert /= 10u; ++n; }
    return n;
}

size_t paket_laenge(size_t praefix_laenge, size_t feld_laenge,
                    uint32_t teil, uint32_t teile,
                    size_t element_bytes, size_t element_anzahl) noexcept {
    return laenge_mit_stellen(praefix_laenge, feld_laenge,
                              dezimalstellen(teil), dezimalstellen(teile),
                              element_bytes, element_anzahl);
}

uint32_t plane_pakete(size_t praefix_laenge, size_t feld_laenge,
                      const ElementListe& liste,
                      uint32_t* grenzen, size_t grenzen_kapazitaet,
                      size_t max_paket) noexcept {
    const size_t anzahl = liste.anzahl();
    if (grenzen == nullptr || grenzen_kapazitaet < 2 ||
        grenzen_kapazitaet < anzahl + 1 || anzahl > 0xFFFFFFFEu) {
        return 0;
    }

    // Die Stellenzahl von "teile" steht in jedem Paket, ist aber erst nach der
    // Planung bekannt. Also mit einer Annahme planen und wiederholen, bis die
    // Annahme stimmt. Mehr Stellen → vollere Köpfe → nie weniger Pakete; die
    // Schleife steigt also nur und endet spätestens bei 10 Stellen.
    size_t stellen_teile = 1;
    for (int versuch = 0; versuch < 12; ++versuch) {
        uint32_t n = 0;
        size_t i = 0;
        bool fehler = false;
        do {
            if (static_cast<size_t>(n) + 1 >= grenzen_kapazitaet) { fehler = true; break; }
            grenzen[n] = static_cast<uint32_t>(i);
            const size_t stellen_teil = dezimalstellen(n + 1);
            size_t bytes = 0;
            size_t im_paket = 0;
            while (i < anzahl) {
                const size_t e = liste.element_laenge(i);
                const size_t mit = laenge_mit_stellen(praefix_laenge, feld_laenge,
                                                      stellen_teil, stellen_teile,
                                                      bytes + e, im_paket + 1);
                if (mit > max_paket) break;
                bytes += e;
                ++im_paket;
                ++i;
            }
            if (im_paket == 0) {
                // Leeres Paket ist nur für die leere Liste erlaubt — und auch
                // dann muss schon der Rahmen passen.
                const size_t leer = laenge_mit_stellen(praefix_laenge, feld_laenge,
                                                       stellen_teil, stellen_teile, 0, 0);
                if (anzahl != 0 || leer > max_paket) { fehler = true; break; }
            }
            ++n;
        } while (i < anzahl);
        if (fehler) return 0;
        grenzen[n] = static_cast<uint32_t>(anzahl);
        if (dezimalstellen(n) <= stellen_teile) return n;
        stellen_teile = dezimalstellen(n);
    }
    return 0;
}

size_t schreibe_paket(char* aus, size_t aus_kapazitaet,
                      const char* praefix, size_t praefix_laenge,
                      const char* feld,
                      const ElementListe& liste,
                      const uint32_t* grenzen, uint32_t k, uint32_t teile) noexcept {
    if (k >= teile || grenzen == nullptr) return 0;
    const size_t von = grenzen[k];
    const size_t bis = grenzen[k + 1];
    if (von > bis || bis > liste.anzahl()) return 0;

    JsonSchreiber w(aus, aus_kapazitaet);
    w.roh(praefix, praefix_laenge);
    w.roh(",\"teil\":", LEN_TEIL);
    w.ganzzahl(static_cast<int64_t>(k) + 1);
    w.roh(",\"teile\":", LEN_TEILE);
    w.ganzzahl(teile);
    w.roh(",\"", LEN_FELD_A);
    w.roh(feld);
    w.roh("\":[", LEN_FELD_B);
    for (size_t i = von; i < bis; ++i) {
        if (i > von) w.zeichen(',');
        w.roh(liste.element(i), liste.element_laenge(i));
    }
    w.roh("]}\n", LEN_ENDE);
    if (w.ueberlauf()) return 0;
    return w.laenge();
}

}  // namespace aeroacars
