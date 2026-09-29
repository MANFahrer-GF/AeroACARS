// =============================================================================
// AeroACARS X-Plane-Plugin — JSON-Schreiber (Umsetzung)
// =============================================================================

#include "json_schreiber.h"

#include <cmath>
#include <cstdio>
#include <cstring>

namespace aeroacars {

bool JsonSchreiber::platz(size_t n) noexcept {
    if (ueberlauf_) return false;
    if (n > kapazitaet_ - laenge_) {
        ueberlauf_ = true;
        return false;
    }
    return true;
}

void JsonSchreiber::roh(const char* text) noexcept {
    if (text == nullptr) return;
    roh(text, std::strlen(text));
}

void JsonSchreiber::roh(const char* text, size_t n) noexcept {
    if (n == 0) return;
    if (!platz(n)) return;
    std::memcpy(puffer_ + laenge_, text, n);
    laenge_ += n;
}

void JsonSchreiber::zeichen(char c) noexcept {
    if (!platz(1)) return;
    puffer_[laenge_++] = c;
}

void JsonSchreiber::null() noexcept { roh("null", 4); }

size_t utf8_folge(const unsigned char* s, size_t rest) noexcept {
    if (rest == 0) return 0;
    const unsigned char c = s[0];
    if (c < 0x80) return 1;
    auto folge = [](unsigned char b) { return b >= 0x80 && b <= 0xBF; };
    if (c >= 0xC2 && c <= 0xDF) {
        return (rest >= 2 && folge(s[1])) ? 2 : 0;
    }
    if (c >= 0xE0 && c <= 0xEF) {
        if (rest < 3) return 0;
        // Zweites Byte: E0 → A0..BF (keine Überlänge), ED → 80..9F (keine
        // Surrogate), sonst 80..BF.
        const unsigned char lo = (c == 0xE0) ? 0xA0 : 0x80;
        const unsigned char hi = (c == 0xED) ? 0x9F : 0xBF;
        if (s[1] < lo || s[1] > hi || !folge(s[2])) return 0;
        return 3;
    }
    if (c >= 0xF0 && c <= 0xF4) {
        if (rest < 4) return 0;
        // F0 → 90..BF (keine Überlänge), F4 → 80..8F (≤ U+10FFFF).
        const unsigned char lo = (c == 0xF0) ? 0x90 : 0x80;
        const unsigned char hi = (c == 0xF4) ? 0x8F : 0xBF;
        if (s[1] < lo || s[1] > hi || !folge(s[2]) || !folge(s[3])) return 0;
        return 4;
    }
    return 0;  // 80..C1, F5..FF: nie Anfang einer gültigen Folge
}

void JsonSchreiber::text(const char* s, size_t n) noexcept {
    static const char HEX[] = "0123456789abcdef";
    zeichen('"');
    const unsigned char* u = reinterpret_cast<const unsigned char*>(s);
    size_t i = 0;
    while (i < n && !ueberlauf_) {
        const unsigned char c = u[i];
        if (c >= 0x20 && c < 0x7F && c != '"' && c != '\\') {
            // Häufigster Fall: ganzen Lauf druckbarer Zeichen am Stück kopieren.
            size_t j = i + 1;
            while (j < n) {
                const unsigned char d = u[j];
                if (d < 0x20 || d >= 0x7F || d == '"' || d == '\\') break;
                ++j;
            }
            roh(s + i, j - i);
            i = j;
            continue;
        }
        switch (c) {
            case '"':  roh("\\\"", 2); ++i; continue;
            case '\\': roh("\\\\", 2); ++i; continue;
            case '\n': roh("\\n", 2);  ++i; continue;
            case '\r': roh("\\r", 2);  ++i; continue;
            case '\t': roh("\\t", 2);  ++i; continue;
            case '\b': roh("\\b", 2);  ++i; continue;
            case '\f': roh("\\f", 2);  ++i; continue;
            default: break;
        }
        if (c >= 0x80) {
            const size_t folge = utf8_folge(u + i, n - i);
            if (folge > 0) {
                roh(s + i, folge);
                i += folge;
                continue;
            }
        }
        // Steuerzeichen, DEL oder Byte ohne gültige UTF-8-Folge → \u00XX.
        char esc[6] = {'\\', 'u', '0', '0', HEX[c >> 4], HEX[c & 0x0F]};
        roh(esc, 6);
        ++i;
    }
    zeichen('"');
}

void JsonSchreiber::text_bis_nul(const char* s, size_t max) noexcept {
    size_t n = 0;
    if (s != nullptr) {
        while (n < max && s[n] != '\0') ++n;
    }
    text(s, n);
}

void JsonSchreiber::ganzzahl(int64_t wert) noexcept {
    char ziffern[24];
    size_t n = 0;
    // Über uint64 rechnen, damit INT64_MIN nicht überläuft.
    uint64_t betrag = wert < 0 ? (0u - static_cast<uint64_t>(wert))
                               : static_cast<uint64_t>(wert);
    do {
        ziffern[n++] = static_cast<char>('0' + (betrag % 10u));
        betrag /= 10u;
    } while (betrag != 0);
    char aus[24];
    size_t k = 0;
    if (wert < 0) aus[k++] = '-';
    while (n > 0) aus[k++] = ziffern[--n];
    roh(aus, k);
}

// Übernimmt eine printf-Zahl und macht sie locale-fest: Ziffern, '-', '+',
// 'e' bleiben; jede andere Bytefolge (das Dezimaltrennzeichen der Locale,
// auch mehrbytige) wird zu genau einem '.'. Schreibt direkt in den Puffer
// (kein Zwischenpuffer fester Länge — "%.7f" von 1e300 hat über 300 Stellen
// und darf nicht still abgeschnitten werden).
void JsonSchreiber::zahl_aus_printf(const char* formatiert, size_t n) noexcept {
    if (n == 0) { null(); return; }
    auto zahlzeichen = [](char c) noexcept {
        return (c >= '0' && c <= '9') || c == '-' || c == '+' || c == 'e' || c == 'E';
    };
    // Zwei Durchgänge: erst die Ausgabelänge zählen (ein mehrbytiges
    // Trennzeichen wird zu EINEM '.'), dann genau so viel Platz verlangen.
    size_t k = 0;
    bool im_fremden = false;
    for (size_t i = 0; i < n; ++i) {
        if (zahlzeichen(formatiert[i])) { ++k; im_fremden = false; }
        else if (!im_fremden)           { ++k; im_fremden = true; }
    }
    if (!platz(k)) return;
    im_fremden = false;
    for (size_t i = 0; i < n; ++i) {
        const char c = formatiert[i];
        if (zahlzeichen(c)) {
            puffer_[laenge_++] = c;
            im_fremden = false;
        } else if (!im_fremden) {
            puffer_[laenge_++] = '.';
            im_fremden = true;
        }
    }
}

void JsonSchreiber::zahl_d(double wert) noexcept {
    if (!std::isfinite(wert)) { null(); return; }
    char tmp[48];
    const int n = std::snprintf(tmp, sizeof(tmp), "%.17g", wert);
    if (n <= 0 || static_cast<size_t>(n) >= sizeof(tmp)) { null(); return; }
    zahl_aus_printf(tmp, static_cast<size_t>(n));
}

void JsonSchreiber::zahl_f(float wert) noexcept {
    if (!std::isfinite(wert)) { null(); return; }
    char tmp[48];
    const int n = std::snprintf(tmp, sizeof(tmp), "%.9g", static_cast<double>(wert));
    if (n <= 0 || static_cast<size_t>(n) >= sizeof(tmp)) { null(); return; }
    zahl_aus_printf(tmp, static_cast<size_t>(n));
}

void JsonSchreiber::zahl_fest(double wert, int nachkommastellen) noexcept {
    if (!std::isfinite(wert)) { null(); return; }
    if (nachkommastellen < 0) nachkommastellen = 0;
    if (nachkommastellen > 17) nachkommastellen = 17;
    // Größter endlicher double: 309 Vorkommastellen + Vorzeichen + Punkt
    // (evtl. mehrbytig) + 17 Nachkommastellen < 400.
    char tmp[400];
    const int n = std::snprintf(tmp, sizeof(tmp), "%.*f", nachkommastellen, wert);
    if (n <= 0 || static_cast<size_t>(n) >= sizeof(tmp)) { null(); return; }
    zahl_aus_printf(tmp, static_cast<size_t>(n));
}

}  // namespace aeroacars
