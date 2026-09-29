// =============================================================================
// AeroACARS X-Plane-Plugin — JSON-Schreiber in einen festen Puffer
// =============================================================================
//
// Schreibt JSON-Bausteine in einen vom Aufrufer gestellten Puffer. Keine
// Allokation, keine Locale, keine NUL-Terminierung (Länge zählt).
//
// Überlauf: Passt ein Baustein nicht mehr, wird NICHTS über die Kapazität
// hinaus geschrieben, `ueberlauf()` wird true und alle weiteren Aufrufe sind
// wirkungslos. Mit marke()/zurueck_zu() lässt sich ein angefangener Baustein
// sauber zurücknehmen — so schneidet die Paketaufteilung nie mitten im Wert.
//
// Zahlen:
//   * double mit "%.17g" — 17 signifikante Stellen stellen jeden double
//     verlustfrei wieder her (Breite/Länge ohne die 0,5-m-Rundung von RREF).
//   * float mit "%.9g" — das gleiche für float.
//   * NaN und ±Inf gibt es in JSON nicht → null.
//   * printf ist locale-abhängig: stellt ein anderes Plugin im X-Plane-Prozess
//     LC_NUMERIC auf de_DE, käme "51,2" heraus. Der Schreiber ersetzt deshalb
//     jedes Zeichen, das in einer JSON-Zahl nichts zu suchen hat, durch '.'.
//
// Zeichenketten (Byte-Arrays aus Datarefs, Namen):
//   * '"' und '\\' werden maskiert, Steuerzeichen < 0x20 als \n \r \t \b \f
//     oder \u00XX, DEL (0x7F) als \u007f.
//   * Gültige UTF-8-Folgen (RFC 3629: keine Überlängen, keine Surrogate,
//     höchstens U+10FFFF) gehen unverändert durch.
//   * Jedes Byte, das zu keiner gültigen UTF-8-Folge gehört, wird als
//     \u00XX geschrieben, also als Latin-1 gelesen. Viele .acf-Titel sind
//     Latin-1 ("Café"); so bleiben sie lesbar, und das Ergebnis ist IMMER
//     gültiges UTF-8 — ein einziges kaputtes Byte darf dem Client (serde_json)
//     nicht die ganze Zeile verderben.
// =============================================================================

#pragma once

#include <cstddef>
#include <cstdint>

namespace aeroacars {

class JsonSchreiber {
public:
    JsonSchreiber(char* puffer, size_t kapazitaet) noexcept
        : puffer_(puffer), kapazitaet_(puffer ? kapazitaet : 0) {}

    // Rohtext (muss schon gültiges JSON sein, z. B. "{\"p\":2,").
    void roh(const char* text) noexcept;
    void roh(const char* text, size_t laenge) noexcept;
    void zeichen(char c) noexcept;

    // Zeichenkette mit Anführungszeichen, maskiert (siehe oben).
    void text(const char* s, size_t laenge) noexcept;
    // Wie text(), endet aber am ersten NUL oder nach `max` Bytes.
    void text_bis_nul(const char* s, size_t max) noexcept;

    void ganzzahl(int64_t wert) noexcept;
    void zahl_d(double wert) noexcept;
    void zahl_f(float wert) noexcept;
    void null() noexcept;

    size_t laenge() const noexcept { return laenge_; }
    bool ueberlauf() const noexcept { return ueberlauf_; }
    const char* daten() const noexcept { return puffer_; }

    size_t marke() const noexcept { return laenge_; }
    // Nimmt alles nach `marke` zurück und löscht den Überlauf-Zustand.
    void zurueck_zu(size_t marke) noexcept {
        if (marke <= laenge_) laenge_ = marke;
        ueberlauf_ = false;
    }

private:
    bool platz(size_t n) noexcept;
    void zahl_aus_printf(const char* formatiert, int n) noexcept;

    char* puffer_;
    size_t kapazitaet_;
    size_t laenge_ = 0;
    bool ueberlauf_ = false;
};

// Höchstlänge, die zahl_d/zahl_f/ganzzahl je schreiben (für Kapazitätsrechnung).
constexpr size_t JSON_MAX_DOUBLE = 25;   // "-2.2250738585072014e-308"
constexpr size_t JSON_MAX_FLOAT  = 16;   // "-1.17549435e-38"
constexpr size_t JSON_MAX_INT32  = 11;   // "-2147483648"
// Höchstlänge einer maskierten Zeichenkette aus n Rohbytes (mit Anführungszeichen).
constexpr size_t json_max_text(size_t n) { return 2 + 6 * n; }

// Länge der gültigen UTF-8-Folge, die bei s[0] beginnt (1–4), oder 0, wenn
// s[0] keine gültige Folge beginnt. Öffentlich für die Tests.
size_t utf8_folge(const unsigned char* s, size_t rest) noexcept;

}  // namespace aeroacars
