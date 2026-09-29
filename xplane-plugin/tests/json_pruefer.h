// =============================================================================
// Strenger JSON-Leser NUR für Tests (RFC 8259 + UTF-8-Prüfung)
// =============================================================================
//
// Prüft jedes Paket, das der Dienst in den Tests erzeugt: gültiges JSON,
// gültiges UTF-8, keine rohen Steuerzeichen, kein Rest hinter dem Wert. Der
// Client (serde_json) ist genauso streng — was hier durchfällt, würde dort
// die ganze Zeile verlieren.
// =============================================================================

#pragma once

#include <cstddef>
#include <string>
#include <utility>
#include <vector>

struct JWert {
    enum class Art { NUL, BOOL, ZAHL, TEXT, FELD, OBJEKT };
    Art art = Art::NUL;
    bool b = false;
    double zahl = 0.0;
    std::string roh;  // Zahl im Originaltext
    std::string text;
    std::vector<JWert> feld;
    std::vector<std::pair<std::string, JWert>> objekt;

    const JWert* hole(const std::string& schluessel) const;
    bool ist_zahl() const { return art == Art::ZAHL; }
    bool ist_text() const { return art == Art::TEXT; }
    bool ist_feld() const { return art == Art::FELD; }
};

// Liest genau einen JSON-Wert aus `s` (ohne führende/abschließende Leerzeichen
// außer dem, was RFC 8259 erlaubt). false + Grund bei jedem Verstoß.
bool json_lies(const std::string& s, JWert* aus, std::string* grund);

// Prüft ein Protokoll-2-Paket: ≤ 8192 Byte, genau ein '\n' am Ende, Objekt
// mit "p":2 und "t" als Text. Liefert das gelesene Objekt.
bool pruefe_paket(const std::string& paket, JWert* aus, std::string* grund);

// true, wenn `s` gültiges UTF-8 ist (RFC 3629).
bool ist_utf8(const std::string& s);
