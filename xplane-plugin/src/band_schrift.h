// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band: Schriftatlas (Open Sans, stb_truetype)
// =============================================================================
//
// XPLM- und OpenGL-frei. Backt die eingebettete Open Sans (Regular, SemiBold,
// Bold; SIL OFL 1.1, third_party/opensans) für alle Bandstile in EINE
// RGBA-Textur: weiß, Deckkraft = Abdeckung. Dazu im selben Bild eine
// Kreisscheibe (Punkt, Boxecken) und ein weißer Block (Flächen, Linien) — so
// zeichnet das Band alles texturiert mit einer einzigen gebundenen Textur
// (Feldbefund X-Plane 12/Metal: untexturierte GL-Flächen erscheinen nicht).
//
// Gebacken wird in `dichte`-facher Auflösung (2 = scharf auf Retina und bei
// Skalierung) für genau eine Stufe; ändert sich die Stufe, backt der Zeichner
// neu — außerhalb des Zeichen-Callbacks. Nur ASCII 0x20–0x7E (das Wire-Format
// erlaubt nichts anderes). Kerning: keins.
// =============================================================================

#pragma once

#include "band_layout.h"

#include <cstddef>
#include <cstdint>

namespace aeroacars {

struct BandGlyph {
    float s0 = 0, t0 = 0, s1 = 0, t1 = 0;  // Texturkoordinaten (t0 = oben)
    float x0 = 0, y0 = 0, x1 = 0, y1 = 0;  // Boxel relativ zu Stift/Grundlinie, y nach oben
    float vorschub = 0;                    // Boxel, inkl. Sperrung des Stils
};

struct BandUV {
    float s0 = 0, t0 = 0, s1 = 0, t1 = 0;
};

class BandAtlas final : public BandSchriftmass {
public:
    BandAtlas() noexcept = default;
    ~BandAtlas();
    BandAtlas(const BandAtlas&) = delete;
    BandAtlas& operator=(const BandAtlas&) = delete;

    // Backt neu. false = kein Speicher / Schrift kaputt; dann ist der Atlas leer.
    bool backe(float skala, float dichte) noexcept;

    bool bereit() const noexcept { return bereit_; }
    float skala() const noexcept { return skala_; }
    float dichte() const noexcept { return dichte_; }

    // RGBA, Zeile 0 = t 0. Nach dem Hochladen freigeben (pixel_freigeben).
    const uint8_t* pixel() const noexcept { return pixel_; }
    int breite_px() const noexcept { return w_; }
    int hoehe_px() const noexcept { return h_; }
    void pixel_freigeben() noexcept;

    const BandGlyph& glyph(BandStil s, char c) const noexcept;  // c wird im Stil umgesetzt (GROSS)
    const BandUV& weiss() const noexcept { return weiss_; }     // Mitte des weißen Blocks
    const BandUV& kreis() const noexcept { return kreis_; }     // ganze Scheibe

    // BandSchriftmass
    float breite(BandStil s, const char* text, size_t n) const noexcept override;
    float aufstieg(BandStil s) const noexcept override;
    float abstieg(BandStil s) const noexcept override;

private:
    bool bereit_ = false;
    float skala_ = 1.0f;
    float dichte_ = 2.0f;
    uint8_t* pixel_ = nullptr;
    int w_ = 0, h_ = 0;
    BandGlyph glyphen_[BAND_STILE][95];
    float auf_[BAND_STILE] = {0};
    float ab_[BAND_STILE] = {0};
    BandUV weiss_, kreis_;
};

// Größe der Kreisscheibe im Atlas (Texel) und des weißen Blocks.
constexpr int BAND_KREIS_PX = 64;
constexpr int BAND_WEISS_PX = 4;

}  // namespace aeroacars
