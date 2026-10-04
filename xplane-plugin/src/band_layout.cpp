// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band: Optik und Anordnung (ADR-0005, Nachtrag)
// =============================================================================
//
// Siehe band_layout.h. Keine Allokation, keine Ausnahmen, keine Locale.
// =============================================================================

#include "band_layout.h"

namespace aeroacars {

namespace {

// Reihenfolge = BandStil.
const BandStilInfo STILE[BAND_STILE] = {
    {BandSchnitt::BOLD, 11.0f, 0.0f, false, bandmass::ALTER_MIN},  // t
    {BandSchnitt::REGULAR, 11.0f, 0.0f, false, 0.0f},              // m
    {BandSchnitt::BOLD, 13.0f, 0.0f, false, 0.0f},                 // i
    {BandSchnitt::SEMIBOLD, 13.0f, 0.0f, false, 0.0f},             // s
    {BandSchnitt::BOLD, 9.0f, 0.09f, true, 0.0f},                  // l
    {BandSchnitt::BOLD, 15.0f, 0.0f, false, 0.0f},                 // v
    {BandSchnitt::SEMIBOLD, 13.0f, 0.0f, false, 0.0f},             // x
    {BandSchnitt::BOLD, 24.0f, 0.0f, false, 0.0f},                 // z
    {BandSchnitt::BOLD, 10.0f, 0.08f, true, 0.0f},                 // e (800 → Bold)
};

// Abstand VOR Lauf `art`, wenn `vorher` direkt davor steht; 0 = neue Gruppe.
float enger_abstand(char vorher, char art) noexcept {
    if (vorher == 'l' && art == 'v') return bandmass::ABSTAND_ENG;
    if (vorher == 't' && art == 'm') return bandmass::ABSTAND_ENG;
    if (vorher == 'z' && art == 'e') return bandmass::ABSTAND_NOTE;
    return 0.0f;
}

struct Lauf {
    bool punkt;
    BandStil stil;
    float b, auf, ab;   // Breite, Aufstieg, Abstieg (Punkt: auf = Höhe, ab = 0)
    float x;
};

}  // namespace

const BandStilInfo& band_stil_info(BandStil s) noexcept {
    const int i = static_cast<int>(s);
    return STILE[(i >= 0 && i < BAND_STILE) ? i : 0];
}

bool band_stil_fuer_art(char art, BandStil* aus) noexcept {
    switch (art) {
        case 't': *aus = BandStil::ALTER; return true;
        case 'm': *aus = BandStil::MELDUNG; return true;
        case 'i': *aus = BandStil::KENNUNG; return true;
        case 's': *aus = BandStil::LAGE; return true;
        case 'l': *aus = BandStil::BESCHRIFTUNG; return true;
        case 'v': *aus = BandStil::WERT; return true;
        case 'x': *aus = BandStil::ANHANG; return true;
        case 'z': *aus = BandStil::NOTE; return true;
        case 'e': *aus = BandStil::ETIKETT; return true;
        default: return false;
    }
}

char band_stil_zeichen(BandStil s, char c) noexcept {
    if (band_stil_info(s).gross && c >= 'a' && c <= 'z') return static_cast<char>(c - 'a' + 'A');
    return c;
}

bool band_hat_inhalt(const BandBild& b) noexcept {
    for (int i = 0; i < b.zeilen; ++i) {
        if (b.zeile[i].laeufe_n > 0) return true;
    }
    return false;
}

void band_ordne(const BandBild& bild, const BandSchriftmass& mass, float skala,
                BandLayout* aus) noexcept {
    aus->n = 0;
    aus->breite = 0;
    aus->hoehe = 0;
    aus->skala = skala;
    if (!band_hat_inhalt(bild)) return;
    const float k = skala;

    // 1. Breiten und Zeilenhöhen.
    static Lauf laeufe[grenzen::BAND_MAX_ZEILEN][grenzen::BAND_MAX_LAEUFE];
    float zeile_b[grenzen::BAND_MAX_ZEILEN] = {0};
    float zeile_h[grenzen::BAND_MAX_ZEILEN] = {0};
    float inhalt_b = 0;
    float inhalt_h = 0;
    int belegte = 0;
    for (int zi = 0; zi < bild.zeilen; ++zi) {
        const BandZeile& z = bild.zeile[zi];
        if (z.laeufe_n == 0) continue;
        // Gruppen: Aufstieg/Abstieg je Gruppe → Gruppenhöhe; Zeilenhöhe = Maximum.
        float x = 0;
        float g_auf = 0, g_ab = 0;
        for (uint16_t li = 0; li < z.laeufe_n; ++li) {
            const BandLauf& l = z.laeufe[li];
            Lauf& L = laeufe[zi][li];
            L.punkt = (l.art == 'p');
            if (L.punkt) {
                L.stil = BandStil::WERT;
                L.b = bandmass::PUNKT * k;
                L.auf = bandmass::PUNKT * k;
                L.ab = 0;
            } else {
                if (!band_stil_fuer_art(l.art, &L.stil)) L.stil = BandStil::WERT;
                const BandStilInfo& si = band_stil_info(L.stil);
                L.b = mass.breite(L.stil, z.text + l.ofs, l.laenge);
                if (L.b < si.min_breite * k) L.b = si.min_breite * k;
                L.auf = mass.aufstieg(L.stil);
                L.ab = mass.abstieg(L.stil);
                if (L.stil == BandStil::ETIKETT) {   // Pille: Innenabstand 1/8 px
                    L.b += 2 * bandmass::PILLE_X * k;
                    L.auf += bandmass::PILLE_Y * k;
                    L.ab += bandmass::PILLE_Y * k;
                }
            }
            const float eng = li > 0 ? enger_abstand(z.laeufe[li - 1].art, l.art) : 0.0f;
            if (li > 0) {
                if (eng > 0) {
                    x += eng * k;
                } else {
                    if (g_auf + g_ab > zeile_h[zi]) zeile_h[zi] = g_auf + g_ab;
                    g_auf = g_ab = 0;
                    x += bandmass::ABSTAND_ZELLE * k;
                }
            }
            L.x = x;
            x += L.b;
            if (L.auf > g_auf) g_auf = L.auf;
            if (L.ab > g_ab) g_ab = L.ab;
        }
        if (g_auf + g_ab > zeile_h[zi]) zeile_h[zi] = g_auf + g_ab;
        zeile_b[zi] = x;
        if (x > inhalt_b) inhalt_b = x;
        inhalt_h += zeile_h[zi];
        ++belegte;
    }
    inhalt_h += static_cast<float>(belegte - 1) *
                (2 * bandmass::LINIE_LUFT + bandmass::LINIE_DICKE) * k;

    aus->breite = inhalt_b + 2 * (bandmass::INNEN_X + bandmass::RAND) * k;
    aus->hoehe = inhalt_h + 2 * (bandmass::INNEN_Y + bandmass::RAND) * k;
    const float links = (bandmass::INNEN_X + bandmass::RAND) * k;

    // 2. Elemente, von oben nach unten.
    float oben = aus->hoehe - (bandmass::INNEN_Y + bandmass::RAND) * k;
    bool erste = true;
    for (int zi = 0; zi < bild.zeilen; ++zi) {
        const BandZeile& z = bild.zeile[zi];
        if (z.laeufe_n == 0) continue;
        if (!erste) {
            oben -= bandmass::LINIE_LUFT * k;
            BandElement& e = aus->e[aus->n++];
            e = BandElement{};
            e.art = BandElementArt::LINIE;
            e.zeile = static_cast<uint8_t>(zi);
            e.x = links;
            e.b = inhalt_b;
            e.h = bandmass::LINIE_DICKE * k;
            e.y = oben - e.h;
            oben -= e.h + bandmass::LINIE_LUFT * k;
        }
        erste = false;
        const float hz = zeile_h[zi];
        // Gruppen erneut ablaufen, um ihren Aufstieg/Abstieg zu kennen.
        uint16_t g0 = 0;
        while (g0 < z.laeufe_n) {
            uint16_t g1 = g0 + 1;
            while (g1 < z.laeufe_n && enger_abstand(z.laeufe[g1 - 1].art, z.laeufe[g1].art) > 0) ++g1;
            float g_auf = 0, g_ab = 0;
            for (uint16_t li = g0; li < g1; ++li) {
                if (laeufe[zi][li].auf > g_auf) g_auf = laeufe[zi][li].auf;
                if (laeufe[zi][li].ab > g_ab) g_ab = laeufe[zi][li].ab;
            }
            const float grundlinie = oben - (hz - (g_auf + g_ab)) / 2 - g_auf;
            for (uint16_t li = g0; li < g1; ++li) {
                const Lauf& L = laeufe[zi][li];
                const BandLauf& l = z.laeufe[li];
                if (L.punkt) {
                    BandElement& e = aus->e[aus->n++];
                    e = BandElement{};
                    e.art = BandElementArt::PUNKT;
                    e.farbe = l.farbe;
                    e.zeile = static_cast<uint8_t>(zi);
                    e.lauf = li;
                    e.x = links + L.x;
                    e.b = e.h = L.b;
                    e.y = oben - hz / 2 - e.h / 2;   // senkrecht mittig in der Zeile
                    continue;
                }
                float tx = links + L.x;
                if (L.stil == BandStil::ETIKETT) {
                    BandElement& p = aus->e[aus->n++];
                    p = BandElement{};
                    p.art = BandElementArt::PILLE;
                    p.stil = L.stil;
                    p.farbe = l.farbe;
                    p.zeile = static_cast<uint8_t>(zi);
                    p.lauf = li;
                    p.x = tx;
                    p.b = L.b;
                    p.h = L.auf + L.ab;
                    p.y = grundlinie - L.ab;
                    tx += bandmass::PILLE_X * k;
                }
                BandElement& e = aus->e[aus->n++];
                e = BandElement{};
                e.art = BandElementArt::TEXT;
                e.stil = L.stil;
                e.farbe = l.farbe;
                e.zeile = static_cast<uint8_t>(zi);
                e.lauf = li;
                e.x = tx;
                e.y = grundlinie;
                e.b = (L.stil == BandStil::ETIKETT) ? L.b - 2 * bandmass::PILLE_X * k : L.b;
                e.h = L.auf + L.ab;
            }
            g0 = g1;
        }
        oben -= hz;
    }
}

// ---- Selbstprüfung -----------------------------------------------------------------

namespace {
int abstand(const uint8_t a[4], const uint8_t b[3]) noexcept {
    int m = 0;
    for (int i = 0; i < 3; ++i) {
        const int d = a[i] > b[i] ? a[i] - b[i] : b[i] - a[i];
        if (d > m) m = d;
    }
    return m;
}
}  // namespace

BandPruefung band_bewerte_pruefung(const uint8_t vorher[4], const uint8_t kontrolle[4],
                                   const uint8_t test[4], bool fehler_zeichnen,
                                   bool fehler_lesen) noexcept {
    if (fehler_zeichnen) return BandPruefung::RUECKFALL;
    if (fehler_lesen) return BandPruefung::UNKLAR;
    // Sieht das Rücklesen überhaupt, was gezeichnet wird? Die X-Plane-Box
    // zeichnet nachweislich; ändert sie nichts, ist die Lesung wertlos (oder
    // der Untergrund schon fast schwarz) → nicht urteilen.
    if (abstand(kontrolle, vorher) <= 3) return BandPruefung::UNKLAR;
    if (abstand(test, BAND_PRUEF_FARBE) <= 48) return BandPruefung::OK;
    // Testquadrat fehlt: Pixel steht unverändert auf dem Kontrollwert.
    if (abstand(test, kontrolle) <= 3) return BandPruefung::RUECKFALL;
    return BandPruefung::UNKLAR;
}

bool band_projiziere(const float mv[16], const float pr[16], const int vp[4], float x, float y,
                     int* px, int* py) noexcept {
    // Spaltenordnung: m[spalte*4 + zeile].
    float a[4], c[4];
    for (int r = 0; r < 4; ++r) a[r] = mv[0 * 4 + r] * x + mv[1 * 4 + r] * y + mv[3 * 4 + r];
    for (int r = 0; r < 4; ++r) {
        c[r] = pr[0 * 4 + r] * a[0] + pr[1 * 4 + r] * a[1] + pr[2 * 4 + r] * a[2] + pr[3 * 4 + r] * a[3];
    }
    if (!(c[3] > 1e-6f || c[3] < -1e-6f)) return false;
    const float nx = c[0] / c[3], ny = c[1] / c[3];
    if (!(nx >= -1.0f && nx <= 1.0f && ny >= -1.0f && ny <= 1.0f)) return false;  // auch NaN
    if (vp[2] <= 0 || vp[3] <= 0) return false;
    const float fx = static_cast<float>(vp[0]) + (nx + 1.0f) * 0.5f * static_cast<float>(vp[2]);
    const float fy = static_cast<float>(vp[1]) + (ny + 1.0f) * 0.5f * static_cast<float>(vp[3]);
    // Rundungsrest der Matrixrechnung (219,9999 statt 220) nicht abschneiden.
    int ix = static_cast<int>(fx + 1e-3f), iy = static_cast<int>(fy + 1e-3f);
    if (ix >= vp[0] + vp[2]) ix = vp[0] + vp[2] - 1;
    if (iy >= vp[1] + vp[3]) iy = vp[1] + vp[3] - 1;
    *px = ix;
    *py = iy;
    return true;
}

}  // namespace aeroacars
