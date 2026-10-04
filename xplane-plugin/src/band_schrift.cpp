// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band: Schriftatlas (Open Sans, stb_truetype)
// =============================================================================
//
// Siehe band_schrift.h. stb_truetype (public domain / MIT, third_party/stb)
// wird nur hier übersetzt und nur mit den eingebetteten, bekannten Schriften
// gefüttert — nie mit Daten von außen.
// =============================================================================

#include "band_schrift.h"

#include "schrift_daten.h"

#include <cmath>
#include <cstdlib>
#include <cstring>

// ---- stb_truetype: nur hier, statisch, ohne Warnungen des Fremdcodes ----------
#if defined(_MSC_VER)
    #pragma warning(push, 0)
#elif defined(__clang__)
    #pragma clang diagnostic push
    #pragma clang diagnostic ignored "-Weverything"
#elif defined(__GNUC__)
    #pragma GCC diagnostic push
    #pragma GCC diagnostic ignored "-Wall"
    #pragma GCC diagnostic ignored "-Wextra"
    #pragma GCC diagnostic ignored "-Wpedantic"
    #pragma GCC diagnostic ignored "-Wconversion"
    #pragma GCC diagnostic ignored "-Wsign-compare"
    #pragma GCC diagnostic ignored "-Wunused-function"
#endif
#define STBTT_STATIC
#define STB_TRUETYPE_IMPLEMENTATION
#include "stb_truetype.h"
#if defined(_MSC_VER)
    #pragma warning(pop)
#elif defined(__clang__)
    #pragma clang diagnostic pop
#elif defined(__GNUC__)
    #pragma GCC diagnostic pop
#endif

namespace aeroacars {

namespace {

const unsigned char* schrift(BandSchnitt s) noexcept {
    switch (s) {
        case BandSchnitt::REGULAR:  return schrift_daten::OPENSANS_REGULAR;
        case BandSchnitt::SEMIBOLD: return schrift_daten::OPENSANS_SEMIBOLD;
        case BandSchnitt::BOLD:     return schrift_daten::OPENSANS_BOLD;
    }
    return schrift_daten::OPENSANS_REGULAR;
}

constexpr int ERSTES = 0x20;
constexpr int ANZAHL = 95;  // 0x20 … 0x7E

}  // namespace

BandAtlas::~BandAtlas() { pixel_freigeben(); }

void BandAtlas::pixel_freigeben() noexcept {
    std::free(pixel_);
    pixel_ = nullptr;
}

bool BandAtlas::backe(float skala, float dichte) noexcept {
    pixel_freigeben();
    bereit_ = false;
    w_ = h_ = 0;
    if (!(skala > 0.05f && skala < 10.0f) || !(dichte >= 1.0f && dichte <= 4.0f)) return false;
    skala_ = skala;
    dichte_ = dichte;

    // Metriken (hhea: Aufstieg/Abstieg = CSS line-height: normal).
    stbtt_fontinfo info[BAND_SCHNITTE];
    for (int s = 0; s < BAND_SCHNITTE; ++s) {
        const unsigned char* d = schrift(static_cast<BandSchnitt>(s));
        if (!stbtt_InitFont(&info[s], d, stbtt_GetFontOffsetForIndex(d, 0))) return false;
    }

    // Größe ermitteln: kleinste Zweierpotenz, in die alles passt. Unten im Bild
    // bleibt ein Streifen für Kreis und weißen Block frei.
    static stbtt_packedchar gepackt[BAND_STILE][ANZAHL];
    for (int w = 256; w <= 4096; w *= 2) {
        for (int hfaktor = 1; hfaktor <= 2; ++hfaktor) {
            const int h = (hfaktor == 1) ? w / 2 : w;  // erst w×w/2, dann w×w
            const int h_glyphen = h - BAND_KREIS_PX - 2;
            if (h_glyphen < 32) continue;
            uint8_t* alpha = static_cast<uint8_t*>(std::calloc(static_cast<size_t>(w) * h, 1));
            if (alpha == nullptr) return false;
            stbtt_pack_context pc;
            bool ok = stbtt_PackBegin(&pc, alpha, w, h_glyphen, w, 1, nullptr) != 0;
            for (int st = 0; ok && st < BAND_STILE; ++st) {
                const BandStilInfo& si = band_stil_info(static_cast<BandStil>(st));
                const float px = si.groesse * skala * dichte;
                ok = stbtt_PackFontRange(&pc, schrift(si.schnitt), 0, STBTT_POINT_SIZE(px), ERSTES,
                                         ANZAHL, gepackt[st]) != 0;
            }
            if (ok) stbtt_PackEnd(&pc);
            else {
                stbtt_PackEnd(&pc);
                std::free(alpha);
                continue;
            }

            // Kreisscheibe (geglätteter Rand) und weißer Block unten links.
            const int ky = h - BAND_KREIS_PX;
            const float r = BAND_KREIS_PX * 0.5f;
            for (int y = 0; y < BAND_KREIS_PX; ++y) {
                for (int x = 0; x < BAND_KREIS_PX; ++x) {
                    const float dx = x + 0.5f - r, dy = y + 0.5f - r;
                    float a = (r - std::sqrt(dx * dx + dy * dy)) * 0.5f + 0.5f;  // ≈ 2 Texel Kante
                    a = a < 0 ? 0 : (a > 1 ? 1 : a);
                    alpha[static_cast<size_t>(ky + y) * w + x] = static_cast<uint8_t>(a * 255.0f + 0.5f);
                }
            }
            const int wx = BAND_KREIS_PX + 2;
            for (int y = 0; y < BAND_WEISS_PX; ++y) {
                for (int x = 0; x < BAND_WEISS_PX; ++x) alpha[static_cast<size_t>(ky + y) * w + wx + x] = 255;
            }

            // In RGBA (weiß, Deckkraft = Abdeckung) umsetzen.
            pixel_ = static_cast<uint8_t*>(std::malloc(static_cast<size_t>(w) * h * 4));
            if (pixel_ == nullptr) {
                std::free(alpha);
                return false;
            }
            for (size_t i = 0; i < static_cast<size_t>(w) * h; ++i) {
                pixel_[i * 4 + 0] = 255;
                pixel_[i * 4 + 1] = 255;
                pixel_[i * 4 + 2] = 255;
                pixel_[i * 4 + 3] = alpha[i];
            }
            std::free(alpha);
            w_ = w;
            h_ = h;

            const float fw = static_cast<float>(w), fh = static_cast<float>(h);
            kreis_ = BandUV{0.0f, ky / fh, BAND_KREIS_PX / fw, (ky + BAND_KREIS_PX) / fh};
            const float wm = wx + BAND_WEISS_PX * 0.5f, hm = ky + BAND_WEISS_PX * 0.5f;
            weiss_ = BandUV{wm / fw, hm / fh, wm / fw, hm / fh};

            for (int st = 0; st < BAND_STILE; ++st) {
                const BandStilInfo& si = band_stil_info(static_cast<BandStil>(st));
                const stbtt_fontinfo& fi = info[static_cast<int>(si.schnitt)];
                int asc = 0, desc = 0, gap = 0;
                stbtt_GetFontVMetrics(&fi, &asc, &desc, &gap);
                const float sk = stbtt_ScaleForMappingEmToPixels(&fi, si.groesse * skala);
                auf_[st] = static_cast<float>(asc) * sk;
                ab_[st] = static_cast<float>(-desc) * sk;
                const float sperr = si.sperrung * si.groesse * skala;
                for (int c = 0; c < ANZAHL; ++c) {
                    const stbtt_packedchar& p = gepackt[st][c];
                    BandGlyph& g = glyphen_[st][c];
                    g.s0 = p.x0 / fw;
                    g.t0 = p.y0 / fh;
                    g.s1 = p.x1 / fw;
                    g.t1 = p.y1 / fh;
                    g.x0 = p.xoff / dichte;
                    g.x1 = p.xoff2 / dichte;
                    g.y0 = -p.yoff2 / dichte;   // unten
                    g.y1 = -p.yoff / dichte;    // oben
                    g.vorschub = p.xadvance / dichte + sperr;
                }
            }
            bereit_ = true;
            return true;
        }
    }
    return false;
}

const BandGlyph& BandAtlas::glyph(BandStil s, char c) const noexcept {
    int st = static_cast<int>(s);
    if (st < 0 || st >= BAND_STILE) st = 0;
    int i = static_cast<unsigned char>(band_stil_zeichen(s, c)) - ERSTES;
    if (i < 0 || i >= ANZAHL) i = '?' - ERSTES;
    return glyphen_[st][i];
}

float BandAtlas::breite(BandStil s, const char* text, size_t n) const noexcept {
    float b = 0;
    for (size_t i = 0; i < n; ++i) b += glyph(s, text[i]).vorschub;
    return b;
}

float BandAtlas::aufstieg(BandStil s) const noexcept {
    const int st = static_cast<int>(s);
    return (st >= 0 && st < BAND_STILE) ? auf_[st] : 0.0f;
}

float BandAtlas::abstieg(BandStil s) const noexcept {
    const int st = static_cast<int>(s);
    return (st >= 0 && st < BAND_STILE) ? ab_[st] : 0.0f;
}

}  // namespace aeroacars
