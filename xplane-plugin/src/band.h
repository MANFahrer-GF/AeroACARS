// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band (ADR-0005), XPLM-freier Kern
// =============================================================================
//
// Der Client rechnet das Band (Texte, Farben) und schickt es fertig als
//
//   BAND <seq> <ruhig> <zeilen>
//   <zeile 1>
//   …
//
// Das Plugin enthält KEINE Fachlogik: Dieser Kern parst, prüft, merkt sich das
// letzte Band samt Zeitstempel und liefert, was gezeichnet werden soll.
// Gezeichnet wird in band_xplm.cpp. Dieser Teil kennt weder XPLM noch OpenGL
// noch eine Uhr: die Zeit kommt als Parameter (Sekunden, monoton), dadurch ist
// alles in tests/test_band.cpp ohne X-Plane prüfbar.
//
// Zeile = Läufe, durch TAB getrennt; Lauf = Farbzeichen + Artzeichen + Text
// (ADR-0005, Nachtrag 04.10.2026):
//   Farbe: n normal · d gedämpft · g gut · w Warnung · b schlecht · a Akzent
//   Art:   t Ticker-Alter · m Ticker-Meldung · p Statuspunkt (Text leer) ·
//          i Kennung · s Lage · l Beschriftung (wird GROSS gezeichnet) ·
//          v Wert · x Anhang · z Landenote (groß) · e Noten-Etikett (Pille)
// Eine leere Zeile (nur "\n") hat keine Läufe und belegt keine Höhe.
//
// Regeln über die ADR hinaus (hier festgelegt, README „BAND“):
//   * Ein leerer Lauf (weniger als 2 Zeichen) ist ungültig, ebenso ein Punkt
//     mit Text.
//   * Jede Zeile endet mit '\n', auch die letzte; fehlt es hinter der letzten,
//     ist das ebenfalls erlaubt. Ein '\r' ist (anders als im Kopf) ungültig.
//   * Ungültig → ganze Nachricht verworfen, das bisherige Band bleibt; die
//     Antwort (fehler/band_ungueltig) schickt der Dienst.
//   * Eine zu alte seq (≤ letzte angenommene, Überlauf-Regel unten) wird
//     STILL verworfen — ein verspätetes UDP-Paket ist kein Fehler.
//
// Sichtbarkeit (ADR-0005, Vertrag vom 04.10.2026):
//   * Vor dem ersten gültigen BAND mit ≥ 1 Zeile wird nichts gezeichnet.
//   * zeilen = 0 blendet aus, bis wieder ein Band mit ≥ 1 Zeile kommt.
//   * Kommt seit grenzen::BAND_FRIST_S kein Band mehr, ersetzt der Kern Zeile 1
//     durch roten Punkt + "AeroACARS nicht erreichbar - laeuft die App?";
//     Zeilen 2 … bleiben stehen. Das gilt nur, wenn schon ein Band kam und es
//     nicht per zeilen = 0 ausgeblendet wurde.
// =============================================================================

#pragma once

#include "grenzen.h"

#include <cstddef>
#include <cstdint>

namespace aeroacars {

// ---- Bild ------------------------------------------------------------------

struct BandLauf {
    char farbe = 'n';
    char art = 'v';
    uint16_t ofs = 0;       // Text in BandZeile::text (hinter Farbe + Art)
    uint16_t laenge = 0;
};

struct BandZeile {
    uint16_t laeufe_n = 0;
    uint16_t text_n = 0;
    BandLauf laeufe[grenzen::BAND_MAX_LAEUFE];
    char text[grenzen::MAX_ZEILE];  // die Zeile unverändert (ohne Zeilenende)
};

struct BandBild {
    bool ruhig = false;
    uint8_t zeilen = 0;
    BandZeile zeile[grenzen::BAND_MAX_ZEILEN];
};

// Prüft und zerlegt die Folgezeilen. `zeilen` ist die Angabe aus dem Kopf
// (0 … 4); `rest` ist alles hinter der Kopfzeile. Bei Fehler bleibt *aus
// unbestimmt und *fehler_zeile nennt die 1-basierte Zeile des Datagramms (die
// Kopfzeile ist Zeile 1).
bool band_zerlege_koerper(uint32_t zeilen, bool ruhig, const char* rest, size_t laenge,
                          BandBild* aus, uint32_t* fehler_zeile) noexcept;

// ---- Zustand ---------------------------------------------------------------

enum class BandErgebnis : uint8_t {
    ANGENOMMEN,
    VERALTET,    // seq nicht neuer — still verworfen
    UNGUELTIG,   // → fehler/band_ungueltig
};

class Band {
public:
    // `jetzt`: Plugin-Uhr beim Start (Sekunden).
    explicit Band(double jetzt) noexcept;

    BandErgebnis uebernehme(uint32_t seq, bool ruhig, uint32_t zeilen, const char* rest,
                            size_t laenge, double jetzt, uint32_t* fehler_zeile) noexcept;

    // Neuer oder neu gestarteter Client (HALLO): seine seq beginnt von vorn.
    // Das angezeigte Band bleibt stehen.
    void neuer_client() noexcept;

    // Was jetzt zu zeichnen ist; nullptr = nichts. Der Zeiger gilt bis zum
    // nächsten Aufruf von uebernehme().
    const BandBild* ansicht(double jetzt) noexcept;

    // Einblick für Tests
    bool hat_band() const noexcept { return hat_band_; }
    bool ausgeblendet() const noexcept { return ausgeblendet_; }
    bool veraltet(double jetzt) const noexcept;

private:
    BandBild bilder_[2];
    int aktuell_ = 0;
    BandBild ersatz_;
    bool ersatz_gueltig_ = false;
    bool hat_band_ = false;        // schon ein Band mit ≥ 1 Zeile gesehen
    bool ausgeblendet_ = false;    // letztes Band hatte zeilen = 0
    bool hat_seq_ = false;
    uint32_t letzte_seq_ = 0;
    double letzte_zeit_ = 0.0;
};

// ---- Größe, Dimmen, Farben ---------------------------------------------------

constexpr int BAND_STUFEN_N = 7;
constexpr int BAND_STUFE_STANDARD = 2;  // 1,0

// Stufen 0,6 · 0,8 · 1,0 · 1,25 · 1,5 · 2,0 · 3,0.
int band_stufe_klemmen(int stufe) noexcept;
float band_skalierung(int stufe) noexcept;           // geklemmt
int band_stufe_groesser(int stufe) noexcept;         // am Ende: bleibt
int band_stufe_kleiner(int stufe) noexcept;

// ruhig → 0,5, Maus darüber → 1,0 (X-Plane-Text kennt kein Alpha).
float band_dimm_faktor(bool ruhig, bool maus_darueber) noexcept;

// RGB 0 … 1 (Werte aus panel.css) mal Faktor. false bei unbekanntem
// Farbzeichen (dann normal).
bool band_farbe(char farbe, float faktor, float rgb[3]) noexcept;

bool band_ist_farbe(char c) noexcept;
bool band_ist_art(char c) noexcept;

// Ersatzzeile bei fehlendem Band.
extern const char BAND_TEXT_NICHT_ERREICHBAR[];

// ---- Position und Merken ------------------------------------------------------

// Fensterkoordinaten wie X-Plane: Ursprung unten links, y nach oben,
// oben > unten.
struct BandRechteck {
    int links = 0;
    int oben = 0;
    int rechts = 0;
    int unten = 0;
};

// Setzt (links, oben) so, dass das Band ganz auf dem Schirm liegt; ist es
// breiter/höher als der Schirm, gilt die linke bzw. obere Kante.
void band_klemme_position(const BandRechteck& schirm, int breite, int hoehe,
                          int* links, int* oben) noexcept;
// Standard: oben mittig.
void band_standard_position(const BandRechteck& schirm, int breite, int hoehe,
                            int* links, int* oben) noexcept;
// Wohin das Fenster VOR dem Zeichnen muss: X-Plane ruft den Zeichen-Callback
// für ein Fenster ganz außerhalb des Schirms nie auf — und erst dort wird es
// platziert. Der Schirm-Ursprung ist nicht immer (0,0) (Feldbefund 05.10.2026:
// X-Plane auf externem Monitor, Schirm y −1080…0). Noch nicht platziert →
// Standardposition, sonst nur auf den Schirm klemmen.
void band_vor_dem_zeigen(const BandRechteck& schirm, const BandRechteck& fenster, bool platziert,
                         int* links, int* oben) noexcept;
// Mehrere Vollbild-Monitore unterschiedlicher Größe: der globale Schirm
// enthält Lücken, die kein Monitor zeigt (XPLMDisplay.h). Darum auf einen
// echten Monitor klemmen: den, der das Fenster am meisten überdeckt;
// überdeckt keiner, den ersten. Ohne Monitore (Fenstermodus: das SDK meldet
// nur Vollbild-Monitore) gilt der globale Schirm.
constexpr int BAND_MONITORE_MAX = 8;
BandRechteck band_waehle_schirm(const BandRechteck* monitore, int n, const BandRechteck& fenster,
                                const BandRechteck& global) noexcept;

struct BandPrefs {
    bool an = true;
    int stufe = BAND_STUFE_STANDARD;
    bool pos_gesetzt = false;
    int links = 0;
    int oben = 0;
};

// "schlüssel=wert", eine Zeile je Wert. Gibt die Länge zurück, 0 wenn es nicht
// in `kapazitaet` passt. Kein NUL am Ende.
size_t band_prefs_schreibe(const BandPrefs& p, char* aus, size_t kapazitaet) noexcept;
// Liest tolerant: unbekannte Schlüssel und Kommentare ("#") werden übergangen,
// ein kaputter Wert behält den Standard dieses Schlüssels, die Stufe wird
// geklemmt. Position nur, wenn x UND y gültig sind.
BandPrefs band_prefs_lese(const char* text, size_t laenge) noexcept;

}  // namespace aeroacars
