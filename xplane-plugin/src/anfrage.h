// =============================================================================
// AeroACARS X-Plane-Plugin — Anfrage-Parser (Protokoll 2)
// =============================================================================
//
// Zerlegt EIN Anfrage-Datagramm des Clients in eine `Anfrage`. Reine Funktion:
// kein XPLM, keine Allokation, kein globaler Zustand, keine Locale. Der
// Parser liest nie außerhalb von [daten, daten + laenge) — das prüfen die
// Unit-Tests und der Fuzz-Test unter AddressSanitizer.
//
// Format (ADR-0004, Abschnitt 2):
//
//   HALLO <protokoll> <client-version>
//   ABO <abo-id> <rate-hz> [<teil> <teile>]
//   <name>
//   <name>[<index>]
//   …
//   ENDE-ABO <abo-id>
//   LISTE <anfrage-id>
//   PING
//   BAND <seq> <ruhig> <zeilen>   (+ <zeilen> Folgezeilen, ADR-0005; Kopf hier,
//                                     Zeilen prüft band.cpp)
//
// Regeln, die über die ADR hinaus festgelegt sind (README „Protokoll 2“):
//   * Genau EIN Befehl je Datagramm. Nur ABO darf Folgezeilen (Namen) haben;
//     nach allen anderen Befehlen ist höchstens eine leere Schlusszeile
//     erlaubt.
//   * Zeilenende ist '\n'. Ein '\r' direkt davor wird toleriert (CRLF), weil
//     Werkzeuge unter Windows das gern anhängen; sonst ist '\r' ungültig.
//   * Argumente sind durch GENAU ein Leerzeichen getrennt.
//   * Zahlen sind reine Dezimalziffern (kein Vorzeichen, höchstens 10 Stellen).
//   * ABO trägt optional als LETZTES Wort die Generation "g<zahl>"
//     (1 … 2^31 − 1): ABO <id> <rate> [<teil> <teile>] [g<zahl>].
//   * Jede Zeile nach der ABO-Zeile ist genau ein Name und zählt mit. Eine
//     ungültige Zeile (leer, > 512 Byte, Leerzeichen, Nicht-ASCII, kaputter
//     Index) verwirft NICHT das Abo, sondern wird als NameRef mit
//     `ungueltig = true` geliefert (Status "fehlt").
//   * Ein Name endet nur dann auf einen Index, wenn er auf "]" endet; dann
//     muss "[<ziffern>]" davorstehen, sonst ist der Name ungültig.
//   * Fehler für die ganze Anfrage gibt es bei ABO nur noch für den Rahmen:
//     Kopfzeile, zu viele Namen, Datagramm zu groß.
// =============================================================================

#pragma once

#include <cstddef>
#include <cstdint>

namespace aeroacars {

enum class Befehl : uint8_t {
    KEINER = 0,
    HALLO,
    ABO,
    ENDE_ABO,
    LISTE,
    PING,
    BAND,
};

// Fehlergründe. Die Texte (fehlergrund_text) sind Teil des Protokolls:
// {"p":2,"t":"fehler","grund":"<text>", …}
enum class Fehlergrund : uint8_t {
    KEINER = 0,
    // -- vom Parser -----------------------------------------------------------
    LEERE_ANFRAGE,          // Datagramm leer oder nur Zeilenenden
    DATAGRAMM_ZU_GROSS,     // > 64 KiB
    ZEILE_ZU_LANG,          // eine Zeile > 512 Byte (ohne Zeilenende)
    UNBEKANNTER_BEFEHL,     // erstes Wort ist kein bekannter Befehl
    FALSCHE_ARGUMENTE,      // Anzahl/Form der Argumente falsch
    PROTOKOLL_UNGUELTIG,    // HALLO: Protokollnummer keine Zahl / 0
    ABO_ID_UNGUELTIG,       // nicht 1–16
    RATE_UNGUELTIG,         // nicht 1–50
    TEIL_UNGUELTIG,         // teil/teile keine Zahl, 0, teil > teile, teile > Grenze
    NAME_UNGUELTIG,         // leer, zu lang, Leerzeichen, Steuer- oder Nicht-ASCII-Zeichen
    INDEX_UNGUELTIG,        // "name[]", "name[-1]", "name[x]", Index zu groß
    ZU_VIELE_NAMEN,         // > 8192 Namen in einem Abo
    UEBERZAEHLIGE_ZEILEN,   // Folgezeilen nach HALLO/ENDE-ABO/LISTE/PING
    // -- vom Dienst (Laufzeit) --------------------------------------------------
    KEIN_HALLO,             // Anfrage ohne gültiges HALLO von dieser Adresse
    ABO_TEIL_REIHENFOLGE,   // Teil fehlt, doppelt oder in falscher Reihenfolge
    ABO_TEILE_WIDERSPRUCH,  // Rate oder Teile-Zahl anders als in Teil 1
    KEINE_NAMEN,            // Abo ohne einen einzigen Namen
    LISTE_NICHT_VERFUEGBAR, // X-Plane älter als 12 (XPLM < 4.0)
    SPEICHER,               // Allokation gescheitert — Anfrage verworfen
    GENERATION_UNGUELTIG,   // ABO: "g<zahl>" nicht 1 … 2^31 − 1
    SPEICHER_LIMIT,         // Bytebudget (grenzen::MAX_BYTES_*) überschritten
    BAND_UNGUELTIG,         // BAND: Kopf, Zeilenzahl, Zeichen, Läufe oder Farben falsch
    // Neue Gründe immer HIER anhängen (Tests zählen bis zum letzten).
};

const char* fehlergrund_text(Fehlergrund grund) noexcept;

// Ein Name aus einer ABO-Anfrage. Zeigt IN das Datagramm (keine Kopie).
struct NameRef {
    const char* basis = nullptr;  // Name ohne "[i]"
    uint16_t basis_laenge = 0;    // 1..512
    int32_t index = -1;           // -1 = ganzer Dataref, sonst Array-Element
    bool ungueltig = false;       // Zeile war kein gültiger Name → Status "fehlt"
};

struct Anfrage {
    Befehl befehl = Befehl::KEINER;

    // HALLO
    uint32_t protokoll = 0;
    const char* client_version = nullptr;  // zeigt ins Datagramm
    size_t client_version_laenge = 0;

    // ABO / ENDE-ABO
    uint32_t abo_id = 0;
    uint32_t rate_hz = 0;
    uint32_t teil = 1;    // bei einteiligem ABO 1/1
    uint32_t teile = 1;
    bool mehrteilig = false;
    uint32_t generation = 0;  // "g<zahl>" am Ende der ABO-Zeile, 0 = ohne
    size_t ungueltige_namen = 0;

    // ABO: Namen in Reihenfolge; `namen` zeigt in den Puffer des Aufrufers.
    const NameRef* namen = nullptr;
    size_t namen_anzahl = 0;

    // LISTE
    uint32_t anfrage_id = 0;

    // BAND: Kopf geprüft; der Rest des Datagramms (die Folgezeilen) zeigt ins
    // Datagramm und wird von band_zerlege_koerper geprüft.
    uint32_t band_seq = 0;
    bool band_ruhig = false;
    uint32_t band_zeilen = 0;
    const char* band_rest = nullptr;
    size_t band_rest_laenge = 0;

    // Fehler: grund != KEINER heißt, die Anfrage ist als Ganzes verworfen.
    Fehlergrund fehler = Fehlergrund::KEINER;
    uint32_t fehler_zeile = 0;  // 1-basiert, 0 = ganzes Datagramm
};

// Zerlegt ein Datagramm. `namen_puffer` muss Platz für `namen_kapazitaet`
// Einträge haben; mehr Namen ergeben ZU_VIELE_NAMEN. Gibt true zurück, wenn
// die Anfrage gültig ist (dann ist out->fehler == KEINER).
bool zerlege_anfrage(const char* daten, size_t laenge,
                     NameRef* namen_puffer, size_t namen_kapazitaet,
                     Anfrage* out) noexcept;

// Prüft einen Dataref-Namen (ohne Index) gegen die Protokollregeln:
// 1..512 Byte, nur druckbares ASCII 0x21–0x7E. Wird auch von LISTE benutzt,
// damit nur abonnierbare Namen gemeldet werden.
bool ist_gueltiger_name(const char* name, size_t laenge) noexcept;

// Wie ist_gueltiger_name, aber zusätzlich nicht auf ']' endend — nur solche
// Namen lassen sich als GANZER Dataref abonnieren. LISTE meldet nur diese.
bool ist_abonnierbarer_name(const char* name, size_t laenge) noexcept;

}  // namespace aeroacars
