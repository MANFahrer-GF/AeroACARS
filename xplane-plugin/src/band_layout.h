// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band: Optik und Anordnung (ADR-0005, Nachtrag)
// =============================================================================
//
// XPLM- und OpenGL-frei. Bildet die Flow-Optik aus
// msfs-panel/…/AeroACARSPanel/panel.css nach und rechnet aus einem BandBild
// die Lage jedes Elements (Text, Punkt, Trennlinie) und die Boxgröße. Der
// Zeichner (band_xplm.cpp) malt nur noch ab — dadurch ist die Anordnung ohne
// X-Plane prüfbar (tests/test_band_layout.cpp).
//
// Maße in Boxeln bei Stufe 1,0 = CSS-px; alles skaliert mit der Stufe
// (Mausrad), auch die Schriftgröße — kein glScalef.
//
// Koordinaten: Ursprung unten links in der Box, y nach oben (wie X-Plane).
// =============================================================================

#pragma once

#include "band.h"

#include <cstddef>
#include <cstdint>

namespace aeroacars {

// ---- Maße aus panel.css (px bei Stufe 1,0) ----------------------------------

namespace bandmass {
constexpr float RAND = 1.0f;            // border: 1px
constexpr float ECKE = 7.0f;            // border-radius: 7px
constexpr float INNEN_X = 15.0f;        // padding: 9px 15px
constexpr float INNEN_Y = 9.0f;
constexpr float ABSTAND_ENG = 5.0f;     // --aa2-gap-tight
constexpr float ABSTAND_ZELLE = 13.0f;  // --aa2-gap-cell
constexpr float LINIE_LUFT = 6.0f;      // .aa2-rule margin: 6px 0
constexpr float LINIE_DICKE = 1.0f;
constexpr float PUNKT = 7.0f;           // .aa2-dot 7×7
constexpr float ALTER_MIN = 62.0f;      // .aa2-age min-width
constexpr float ABSTAND_NOTE = 9.0f;    // .aa2-score > * + * { margin-left: 9px }
constexpr float PILLE_X = 8.0f;         // .aa2-band padding: 1px 8px
constexpr float PILLE_Y = 1.0f;
constexpr float PILLE_ALPHA = 0.24f;    // Hintergrund = Lauffarbe mit 24 %
constexpr float SCHATTEN_Y = 1.0f;      // text-shadow: 0 1px 2px rgba(0,0,0,.75)
constexpr float SCHATTEN_ALPHA = 0.75f;
// Farben der Box (RGBA 0…1)
constexpr float BOX[4] = {13 / 255.0f, 17 / 255.0f, 24 / 255.0f, 0.88f};
constexpr float BOX_RAND[4] = {1.0f, 1.0f, 1.0f, 0.13f};
constexpr float LINIE[4] = {1.0f, 1.0f, 1.0f, 0.12f};
// Ruhezustand: opacity 0.5 für den ganzen Kasten
constexpr float RUHIG_DECKKRAFT = 0.5f;
}  // namespace bandmass

// ---- Schriftstile ---------------------------------------------------------------

enum class BandSchnitt : uint8_t { REGULAR = 0, SEMIBOLD = 1, BOLD = 2 };
constexpr int BAND_SCHNITTE = 3;

// Je Art (außer Punkt) ein Stil.
enum class BandStil : uint8_t {
    ALTER = 0,      // t  .aa2-age   11px 700
    MELDUNG,        // m  .aa2-msg   11px 400
    KENNUNG,        // i  .aa2-ident 13px 700
    LAGE,           // s  .aa2-state 13px 600
    BESCHRIFTUNG,   // l  .aa2-lbl    9px 700, GROSS, letter-spacing .09em
    WERT,           // v  .aa2-val   15px 700
    ANHANG,         // x  .aa2-tail  13px 600
    NOTE,           // z  .aa2-val.aa2-xl 24px 700
    ETIKETT,        // e  .aa2-band  10px 800 (eingebettet: Bold), GROSS, .08em, Pille
};
constexpr int BAND_STILE = 9;

struct BandStilInfo {
    BandSchnitt schnitt;
    float groesse;     // font-size in px
    float sperrung;    // letter-spacing in em (nach jedem Zeichen)
    bool gross;        // text-transform: uppercase
    float min_breite;  // min-width in px (0 = keine)
};

const BandStilInfo& band_stil_info(BandStil s) noexcept;
// false für 'p' (Punkt) und unbekannte Arten.
bool band_stil_fuer_art(char art, BandStil* aus) noexcept;
// Das Zeichen, das für `c` in diesem Stil gezeichnet wird (GROSS bei 'l').
char band_stil_zeichen(BandStil s, char c) noexcept;

// Messung, wie sie der Zeichner hat (im Plugin: der Schriftatlas). Alle
// Werte in Boxeln für die Stufe, für die gemessen wird.
class BandSchriftmass {
public:
    // Breite inkl. Sperrung und Großschreibung des Stils.
    virtual float breite(BandStil s, const char* text, size_t n) const noexcept = 0;
    virtual float aufstieg(BandStil s) const noexcept = 0;  // über der Grundlinie, > 0
    virtual float abstieg(BandStil s) const noexcept = 0;   // unter der Grundlinie, > 0

protected:
    ~BandSchriftmass() = default;
};

// ---- Anordnung -------------------------------------------------------------------

enum class BandElementArt : uint8_t { TEXT, PUNKT, LINIE, PILLE };

struct BandElement {
    BandElementArt art = BandElementArt::TEXT;
    BandStil stil = BandStil::WERT;
    char farbe = 'n';
    uint8_t zeile = 0;
    uint16_t lauf = 0;
    // TEXT: x = Anfang, y = Grundlinie, b = gemessene Breite.
    // PUNKT/LINIE/PILLE: Rechteck links/unten + Breite/Höhe. Eine PILLE steht
    // unmittelbar VOR dem TEXT-Element ihres Laufs.
    float x = 0, y = 0, b = 0, h = 0;
};

constexpr size_t BAND_MAX_ELEMENTE =
    grenzen::BAND_MAX_ZEILEN * grenzen::BAND_MAX_LAEUFE * 2 + grenzen::BAND_MAX_ZEILEN;

struct BandLayout {
    float breite = 0, hoehe = 0;  // ganze Box inkl. Rand; 0 = nichts zu zeichnen
    float skala = 1.0f;
    uint16_t n = 0;
    BandElement e[BAND_MAX_ELEMENTE];
};

// true, wenn mindestens eine Zeile Läufe hat (sonst kein Kasten — wie
// `.aa2-strip:empty { display: none }`).
bool band_hat_inhalt(const BandBild& b) noexcept;

// Ordnet an. Abstände: Beschriftung→Wert und Alter→Meldung 5 px, Note→Etikett
// 9 px, sonst 13 px.
// Läufe, die so eng verbunden sind, bilden eine Gruppe (Zelle); Gruppen werden
// in der Zeile senkrecht mittig gestellt, innerhalb der Gruppe stehen sie auf
// einer Grundlinie (panel.css: .aa2-data center, .aa2-cell/.aa2-ticker baseline).
// Leere Zeilen belegen keine Höhe; zwischen zwei belegten Zeilen steht eine
// Haarlinie (.aa2-rule, 6 px Luft je Seite).
void band_ordne(const BandBild& bild, const BandSchriftmass& mass, float skala,
                BandLayout* aus) noexcept;

// ---- Selbstprüfung des Zeichenwegs ---------------------------------------------------

enum class BandPruefung : uint8_t {
    OK,         // eigene texturierte Fläche kam an
    RUECKFALL,  // eindeutig: sie kam nicht an (oder GL-Fehler beim Zeichnen)
    UNKLAR,     // Rücklesen sagt nichts aus — nach PRUEF_MAX_UNKLAR Versuchen sicherer Rückfall
};

// Farbe des Prüf-Quadrats (deckend).
constexpr uint8_t BAND_PRUEF_FARBE[3] = {255, 0, 255};

// Bewertet drei Rücklesungen (RGBA, 1 Pixel) an derselben Stelle:
//   vorher    — vor jeder eigenen Zeichnung,
//   kontrolle — nach XPLMDrawTranslucentDarkBox (zeichnet nachweislich),
//   test      — nach dem eigenen texturierten Quadrat in BAND_PRUEF_FARBE.
// Nur wenn die Kontrolle das Rücklesen bestätigt (sichtbare Änderung) und das
// Testquadrat danach fehlt, ist der Befund eindeutig. GL-Fehler beim
// Rücklesen → UNKLAR; GL-Fehler beim eigenen Zeichnen → RUECKFALL.
BandPruefung band_bewerte_pruefung(const uint8_t vorher[4], const uint8_t kontrolle[4],
                                   const uint8_t test[4], bool fehler_zeichnen,
                                   bool fehler_lesen) noexcept;

// Bildet einen Punkt (Fensterkoordinaten wie beim Zeichnen) über Modelview,
// Projektion und Viewport (GL-Spaltenordnung) auf Bildpunkte ab. false, wenn
// die Matrizen unbrauchbar sind oder der Punkt außerhalb des Viewports liegt.
bool band_projiziere(const float modelview[16], const float projektion[16],
                     const int viewport[4], float x, float y, int* px, int* py) noexcept;

}  // namespace aeroacars
