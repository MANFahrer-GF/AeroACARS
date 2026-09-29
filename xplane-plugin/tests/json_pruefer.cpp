// Strenger JSON-Leser für Tests — siehe json_pruefer.h.

#include "json_pruefer.h"

#include <clocale>
#include <cstdlib>
#include <cstring>

const JWert* JWert::hole(const std::string& schluessel) const {
    if (art != Art::OBJEKT) return nullptr;
    for (const auto& kv : objekt) {
        if (kv.first == schluessel) return &kv.second;
    }
    return nullptr;
}

bool ist_utf8(const std::string& s) {
    const unsigned char* u = reinterpret_cast<const unsigned char*>(s.data());
    const size_t n = s.size();
    size_t i = 0;
    while (i < n) {
        const unsigned char c = u[i];
        size_t len = 0;
        unsigned char lo = 0x80, hi = 0xBF;
        if (c < 0x80) { ++i; continue; }
        else if (c >= 0xC2 && c <= 0xDF) len = 2;
        else if (c >= 0xE0 && c <= 0xEF) { len = 3; if (c == 0xE0) lo = 0xA0; if (c == 0xED) hi = 0x9F; }
        else if (c >= 0xF0 && c <= 0xF4) { len = 4; if (c == 0xF0) lo = 0x90; if (c == 0xF4) hi = 0x8F; }
        else return false;
        if (i + len > n) return false;
        if (u[i + 1] < lo || u[i + 1] > hi) return false;
        for (size_t k = 2; k < len; ++k) {
            if (u[i + k] < 0x80 || u[i + k] > 0xBF) return false;
        }
        i += len;
    }
    return true;
}

namespace {

struct Leser {
    const std::string& s;
    size_t i = 0;
    std::string grund;
    int tiefe = 0;

    explicit Leser(const std::string& text) : s(text) {}

    bool fehler(const char* g) {
        if (grund.empty()) grund = std::string(g) + " bei Byte " + std::to_string(i);
        return false;
    }
    void leer() {
        while (i < s.size() && (s[i] == ' ' || s[i] == '\t' || s[i] == '\n' || s[i] == '\r')) ++i;
    }
    bool wort(const char* w) {
        const size_t n = std::strlen(w);
        if (s.compare(i, n, w) != 0) return fehler("unbekanntes Wort");
        i += n;
        return true;
    }
    static int hex(char c) {
        if (c >= '0' && c <= '9') return c - '0';
        if (c >= 'a' && c <= 'f') return c - 'a' + 10;
        if (c >= 'A' && c <= 'F') return c - 'A' + 10;
        return -1;
    }
    static void utf8_anhaengen(std::string* aus, unsigned cp) {
        if (cp < 0x80) aus->push_back(static_cast<char>(cp));
        else if (cp < 0x800) {
            aus->push_back(static_cast<char>(0xC0 | (cp >> 6)));
            aus->push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else if (cp < 0x10000) {
            aus->push_back(static_cast<char>(0xE0 | (cp >> 12)));
            aus->push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            aus->push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        } else {
            aus->push_back(static_cast<char>(0xF0 | (cp >> 18)));
            aus->push_back(static_cast<char>(0x80 | ((cp >> 12) & 0x3F)));
            aus->push_back(static_cast<char>(0x80 | ((cp >> 6) & 0x3F)));
            aus->push_back(static_cast<char>(0x80 | (cp & 0x3F)));
        }
    }
    bool vier_hex(unsigned* cp) {
        if (i + 4 > s.size()) return fehler("\\u zu kurz");
        unsigned v = 0;
        for (int k = 0; k < 4; ++k) {
            const int h = hex(s[i + k]);
            if (h < 0) return fehler("\\u ohne Hex");
            v = v * 16 + static_cast<unsigned>(h);
        }
        i += 4;
        *cp = v;
        return true;
    }
    bool text(std::string* aus) {
        if (i >= s.size() || s[i] != '"') return fehler("\" erwartet");
        ++i;
        const size_t anfang_roh = i;
        while (true) {
            if (i >= s.size()) return fehler("Text nicht beendet");
            const unsigned char c = static_cast<unsigned char>(s[i]);
            if (c == '"') { ++i; break; }
            if (c < 0x20) return fehler("rohes Steuerzeichen im Text");
            if (c == '\\') {
                ++i;
                if (i >= s.size()) return fehler("Escape am Ende");
                const char e = s[i++];
                switch (e) {
                    case '"': aus->push_back('"'); break;
                    case '\\': aus->push_back('\\'); break;
                    case '/': aus->push_back('/'); break;
                    case 'b': aus->push_back('\b'); break;
                    case 'f': aus->push_back('\f'); break;
                    case 'n': aus->push_back('\n'); break;
                    case 'r': aus->push_back('\r'); break;
                    case 't': aus->push_back('\t'); break;
                    case 'u': {
                        unsigned cp = 0;
                        if (!vier_hex(&cp)) return false;
                        if (cp >= 0xD800 && cp <= 0xDBFF) {
                            if (i + 2 > s.size() || s[i] != '\\' || s[i + 1] != 'u') {
                                return fehler("einzelnes hohes Surrogat");
                            }
                            i += 2;
                            unsigned lo = 0;
                            if (!vier_hex(&lo)) return false;
                            if (lo < 0xDC00 || lo > 0xDFFF) return fehler("kaputtes Surrogatpaar");
                            cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                        } else if (cp >= 0xDC00 && cp <= 0xDFFF) {
                            return fehler("einzelnes tiefes Surrogat");
                        }
                        utf8_anhaengen(aus, cp);
                        break;
                    }
                    default:
                        return fehler("unbekanntes Escape");
                }
                continue;
            }
            aus->push_back(static_cast<char>(c));
            ++i;
        }
        // Rohbytes zwischen den Anführungszeichen müssen UTF-8 sein.
        if (!ist_utf8(s.substr(anfang_roh, i - 1 - anfang_roh))) return fehler("kein UTF-8 im Text");
        return true;
    }
    bool zahl(JWert* aus) {
        const size_t anfang = i;
        if (i < s.size() && s[i] == '-') ++i;
        if (i >= s.size()) return fehler("Zahl zu kurz");
        if (s[i] == '0') {
            ++i;
        } else if (s[i] >= '1' && s[i] <= '9') {
            while (i < s.size() && s[i] >= '0' && s[i] <= '9') ++i;
        } else {
            return fehler("Ziffer erwartet");
        }
        if (i < s.size() && s[i] == '.') {
            ++i;
            const size_t z = i;
            while (i < s.size() && s[i] >= '0' && s[i] <= '9') ++i;
            if (i == z) return fehler("Ziffer nach . erwartet");
        }
        if (i < s.size() && (s[i] == 'e' || s[i] == 'E')) {
            ++i;
            if (i < s.size() && (s[i] == '+' || s[i] == '-')) ++i;
            const size_t z = i;
            while (i < s.size() && s[i] >= '0' && s[i] <= '9') ++i;
            if (i == z) return fehler("Ziffer im Exponenten erwartet");
        }
        aus->art = JWert::Art::ZAHL;
        aus->roh = s.substr(anfang, i - anfang);
        aus->zahl = std::strtod(aus->roh.c_str(), nullptr);
        return true;
    }
    bool wert(JWert* aus) {
        if (++tiefe > 64) return fehler("zu tief");
        leer();
        if (i >= s.size()) return fehler("Wert erwartet");
        const char c = s[i];
        bool ok = false;
        if (c == '{') {
            aus->art = JWert::Art::OBJEKT;
            ++i;
            leer();
            if (i < s.size() && s[i] == '}') { ++i; ok = true; }
            else {
                while (true) {
                    leer();
                    std::string k;
                    if (!text(&k)) return false;
                    leer();
                    if (i >= s.size() || s[i] != ':') return fehler(": erwartet");
                    ++i;
                    JWert v;
                    if (!wert(&v)) return false;
                    for (const auto& kv : aus->objekt) {
                        if (kv.first == k) return fehler("doppelter Schluessel");
                    }
                    aus->objekt.emplace_back(k, v);
                    leer();
                    if (i < s.size() && s[i] == ',') { ++i; continue; }
                    if (i < s.size() && s[i] == '}') { ++i; ok = true; break; }
                    return fehler(", oder } erwartet");
                }
            }
        } else if (c == '[') {
            aus->art = JWert::Art::FELD;
            ++i;
            leer();
            if (i < s.size() && s[i] == ']') { ++i; ok = true; }
            else {
                while (true) {
                    JWert v;
                    if (!wert(&v)) return false;
                    aus->feld.push_back(v);
                    leer();
                    if (i < s.size() && s[i] == ',') { ++i; continue; }
                    if (i < s.size() && s[i] == ']') { ++i; ok = true; break; }
                    return fehler(", oder ] erwartet");
                }
            }
        } else if (c == '"') {
            aus->art = JWert::Art::TEXT;
            ok = text(&aus->text);
        } else if (c == 't') {
            aus->art = JWert::Art::BOOL; aus->b = true; ok = wort("true");
        } else if (c == 'f') {
            aus->art = JWert::Art::BOOL; aus->b = false; ok = wort("false");
        } else if (c == 'n') {
            aus->art = JWert::Art::NUL; ok = wort("null");
        } else {
            ok = zahl(aus);
        }
        --tiefe;
        return ok;
    }
};

}  // namespace

bool json_lies(const std::string& s, JWert* aus, std::string* grund) {
    Leser l(s);
    *aus = JWert{};
    if (!l.wert(aus)) {
        if (grund) *grund = l.grund;
        return false;
    }
    l.leer();
    if (l.i != s.size()) {
        if (grund) *grund = "Rest hinter dem Wert bei Byte " + std::to_string(l.i);
        return false;
    }
    return true;
}

bool pruefe_paket(const std::string& paket, JWert* aus, std::string* grund) {
    if (paket.size() > 8192) {
        *grund = "Paket groesser als 8192 Byte: " + std::to_string(paket.size());
        return false;
    }
    if (paket.empty() || paket.back() != '\n') {
        *grund = "Paket endet nicht mit \\n";
        return false;
    }
    if (paket.find('\n') != paket.size() - 1) {
        *grund = "Paket enthaelt mehr als eine Zeile";
        return false;
    }
    if (!ist_utf8(paket)) {
        *grund = "Paket ist kein UTF-8";
        return false;
    }
    const std::string zeile = paket.substr(0, paket.size() - 1);
    if (!json_lies(zeile, aus, grund)) return false;
    if (aus->art != JWert::Art::OBJEKT) {
        *grund = "Paket ist kein Objekt";
        return false;
    }
    const JWert* p = aus->hole("p");
    const JWert* t = aus->hole("t");
    if (!p || !p->ist_zahl() || p->roh != "2" || !t || !t->ist_text()) {
        *grund = "p != 2 oder t fehlt";
        return false;
    }
    return true;
}
