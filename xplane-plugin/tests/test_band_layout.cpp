// Einheitstests: HUD-Band-Optik (band_layout.cpp), Schriftatlas
// (band_schrift.cpp) und die Bewertung der Selbstprüfung.
//
// Die Anordnung wird gegen eine Schein-Schrift mit runden Maßen geprüft (jede
// Zahl im Test ist von Hand nachrechenbar), der Atlas gegen die echte,
// eingebettete Open Sans.

#include "band.h"
#include "band_layout.h"
#include "band_schrift.h"
#include "testrahmen.h"

#include <cmath>
#include <cstdlib>
#include <cstring>
#include <string>

using namespace aeroacars;

namespace {

// Schein-Schrift: jedes Zeichen 0,5 em breit (+ Sperrung des Stils),
// Aufstieg 0,8 em, Abstieg 0,2 em.
class ScheinMass final : public BandSchriftmass {
public:
    explicit ScheinMass(float skala) : k(skala) {}
    float breite(BandStil s, const char*, size_t n) const noexcept override {
        const BandStilInfo& si = band_stil_info(s);
        return static_cast<float>(n) * (0.5f + si.sperrung) * si.groesse * k;
    }
    float aufstieg(BandStil s) const noexcept override { return 0.8f * band_stil_info(s).groesse * k; }
    float abstieg(BandStil s) const noexcept override { return 0.2f * band_stil_info(s).groesse * k; }
    float k;
};

bool nah(float a, float b, float eps = 0.01f) { return std::fabs(a - b) <= eps; }

#define PRUEFE_NAH(a, b)                                                                     \
    do {                                                                                     \
        ++testrahmen::pruefzahl();                                                           \
        const float tr_a = (a), tr_b = (b);                                                  \
        if (!nah(tr_a, tr_b)) {                                                              \
            testrahmen::melde(__FILE__, __LINE__, std::string("PRUEFE_NAH(" #a ", " #b "): ") + \
                                                      std::to_string(tr_a) + " != " +        \
                                                      std::to_string(tr_b));                 \
        }                                                                                    \
    } while (0)

BandBild bild_aus(uint32_t zeilen, const std::string& rest) {
    BandBild b;
    uint32_t fz = 0;
    if (!band_zerlege_koerper(zeilen, false, rest.data(), rest.size(), &b, &fz)) {
        testrahmen::melde(__FILE__, __LINE__, "Testbild ungueltig: " + rest);
    }
    return b;
}

BandLayout g_lay;  // groß — nicht auf den Stapel

const BandElement* finde(const BandLayout& l, BandElementArt art, int zeile, int lauf) {
    for (int i = 0; i < l.n; ++i) {
        if (l.e[i].art == art && l.e[i].zeile == zeile && l.e[i].lauf == lauf) return &l.e[i];
    }
    return nullptr;
}

int zaehle(const BandLayout& l, BandElementArt art) {
    int n = 0;
    for (int i = 0; i < l.n; ++i) n += (l.e[i].art == art);
    return n;
}

}  // namespace

// ---- Stile = panel.css --------------------------------------------------------------

TEST(band_stile_wie_panel_css) {
    struct Soll { char art; float px; BandSchnitt s; bool gross; };
    const Soll soll[] = {
        {'t', 11, BandSchnitt::BOLD, false},  {'m', 11, BandSchnitt::REGULAR, false},
        {'i', 13, BandSchnitt::BOLD, false},  {'s', 13, BandSchnitt::SEMIBOLD, false},
        {'l', 9, BandSchnitt::BOLD, true},    {'v', 15, BandSchnitt::BOLD, false},
        {'x', 13, BandSchnitt::SEMIBOLD, false}, {'z', 24, BandSchnitt::BOLD, false},
        {'e', 10, BandSchnitt::BOLD, true},
    };
    for (const Soll& s : soll) {
        BandStil st;
        PRUEFE(band_stil_fuer_art(s.art, &st));
        const BandStilInfo& i = band_stil_info(st);
        PRUEFE(i.groesse == s.px);
        PRUEFE(i.schnitt == s.s);
        PRUEFE(i.gross == s.gross);
    }
    BandStil st;
    PRUEFE(!band_stil_fuer_art('p', &st));
    PRUEFE(band_stil_info(BandStil::BESCHRIFTUNG).sperrung == 0.09f);
    PRUEFE(band_stil_info(BandStil::ETIKETT).sperrung == 0.08f);
    PRUEFE(band_stil_info(BandStil::ALTER).min_breite == 62.0f);
    PRUEFE(band_stil_zeichen(BandStil::BESCHRIFTUNG, 'r') == 'R');
    PRUEFE(band_stil_zeichen(BandStil::WERT, 'r') == 'r');
    PRUEFE(band_stil_zeichen(BandStil::BESCHRIFTUNG, '1') == '1');
}

// ---- Anordnung ------------------------------------------------------------------------

TEST(band_layout_eine_datenzeile) {
    const BandBild b = bild_aus(1, "bp\tniDLH\tdlRoute\tnvEDDF\n");
    ScheinMass m(1.0f);
    band_ordne(b, m, 1.0f, &g_lay);
    // Breiten: Punkt 7; DLH 3×6,5 = 19,5; ROUTE 5×(4,5+0,81) = 26,55; EDDF 4×7,5 = 30
    // x: Punkt 0 | +13 | Kennung 20 | +13 | Beschriftung 52,5 | +5 | Wert 84,05 → 114,05
    PRUEFE_NAH(g_lay.breite, 114.05f + 2 * 16);
    // Zeilenhöhe = höchste Gruppe: Zelle (Aufstieg 12, Abstieg 3) = 15
    PRUEFE_NAH(g_lay.hoehe, 15 + 2 * 10);
    PRUEFE_GLEICH(int(g_lay.n), 4);
    PRUEFE_GLEICH(zaehle(g_lay, BandElementArt::LINIE), 0);
    const BandElement* p = finde(g_lay, BandElementArt::PUNKT, 0, 0);
    const BandElement* i = finde(g_lay, BandElementArt::TEXT, 0, 1);
    const BandElement* l = finde(g_lay, BandElementArt::TEXT, 0, 2);
    const BandElement* v = finde(g_lay, BandElementArt::TEXT, 0, 3);
    PRUEFE(p && i && l && v);
    if (!(p && i && l && v)) return;
    PRUEFE_NAH(p->x, 16);
    PRUEFE_NAH(p->b, 7);
    PRUEFE_NAH(p->y, 25 - 7.5f - 3.5f);          // senkrecht mittig in der Zeile
    PRUEFE_NAH(i->x, 16 + 20);
    PRUEFE_NAH(l->x, 16 + 52.5f);
    PRUEFE_NAH(v->x, 16 + 84.05f);
    PRUEFE_NAH(l->y, v->y);                       // Zelle: gemeinsame Grundlinie
    PRUEFE_NAH(v->y, 25 - 12);
    PRUEFE_NAH(i->y, 25 - 1 - 10.4f);             // Kennung mittig: (15−13)/2 Luft oben
    PRUEFE(i->farbe == 'n' && l->farbe == 'd');
    PRUEFE(l->stil == BandStil::BESCHRIFTUNG && v->stil == BandStil::WERT);
}

TEST(band_layout_ticker_abstaende_und_mindestbreite) {
    const BandBild b = bild_aus(1, "ntvor 3 s\tdmPIREP\n");
    ScheinMass m(1.0f);
    band_ordne(b, m, 1.0f, &g_lay);
    const BandElement* t = finde(g_lay, BandElementArt::TEXT, 0, 0);
    const BandElement* msg = finde(g_lay, BandElementArt::TEXT, 0, 1);
    PRUEFE(t && msg);
    if (!(t && msg)) return;
    // "vor 3 s" = 7 × 5,5 = 38,5 < 62 → 62; Meldung 5 px dahinter
    PRUEFE_NAH(t->b, 62);
    PRUEFE_NAH(msg->x - t->x, 62 + 5);
    PRUEFE_NAH(t->y, msg->y);
    PRUEFE_NAH(g_lay.breite, 62 + 5 + 5 * 5.5f + 32);
}

TEST(band_layout_leere_zeilen_und_haarlinie) {
    ScheinMass m(1.0f);
    // leere erste Zeile: wie eine einzige Zeile, keine Linie
    BandBild b = bild_aus(2, "\nbp\tniDLH\tdlRoute\tnvEDDF\n");
    band_ordne(b, m, 1.0f, &g_lay);
    PRUEFE_NAH(g_lay.hoehe, 35);
    PRUEFE_GLEICH(zaehle(g_lay, BandElementArt::LINIE), 0);
    // Ticker + Daten: Haarlinie mit 6 px Luft je Seite
    b = bild_aus(2, "ntvor 3 s\tdmPIREP\nbp\tniDLH\tdlRoute\tnvEDDF\n");
    band_ordne(b, m, 1.0f, &g_lay);
    PRUEFE_GLEICH(zaehle(g_lay, BandElementArt::LINIE), 1);
    // Ticker-Zeile: Aufstieg 8,8 + Abstieg 2,2 = 11
    PRUEFE_NAH(g_lay.hoehe, 11 + 13 + 15 + 20);
    const BandElement* linie = nullptr;
    for (int i = 0; i < g_lay.n; ++i) if (g_lay.e[i].art == BandElementArt::LINIE) linie = &g_lay.e[i];
    PRUEFE(linie != nullptr);
    if (linie) {
        PRUEFE_NAH(linie->y, 59 - 10 - 11 - 6 - 1);
        PRUEFE_NAH(linie->h, 1);
        PRUEFE_NAH(linie->x, 16);
        PRUEFE_NAH(linie->b, g_lay.breite - 32);   // volle Innenbreite
    }
    // nur leere Zeilen: nichts
    b = bild_aus(2, "\n\n");
    PRUEFE(!band_hat_inhalt(b));
    band_ordne(b, m, 1.0f, &g_lay);
    PRUEFE_GLEICH(int(g_lay.n), 0);
    PRUEFE_NAH(g_lay.breite, 0);
}

TEST(band_layout_note_und_pille) {
    const BandBild b = bild_aus(1, "nz87\tweFIRM\tdlRate\tnv-455\n");
    ScheinMass m(1.0f);
    band_ordne(b, m, 1.0f, &g_lay);
    const BandElement* z = finde(g_lay, BandElementArt::TEXT, 0, 0);
    const BandElement* pil = finde(g_lay, BandElementArt::PILLE, 0, 1);
    const BandElement* e = finde(g_lay, BandElementArt::TEXT, 0, 1);
    const BandElement* l = finde(g_lay, BandElementArt::TEXT, 0, 2);
    PRUEFE(z && pil && e && l);
    if (!(z && pil && e && l)) return;
    // "87" 24 px: 2 × 12 = 24; Pille 9 px dahinter
    PRUEFE_NAH(pil->x - z->x, 24 + 9);
    // FIRM: 4 × (5 + 0,8) = 23,2; Pille = Text + 2 × 8, Text 8 px eingerückt
    PRUEFE_NAH(e->b, 23.2f);
    PRUEFE_NAH(pil->b, 23.2f + 16);
    PRUEFE_NAH(e->x, pil->x + 8);
    PRUEFE_NAH(pil->h, 10 + 2);                    // Zeilenhöhe 10 + 2 × 1
    PRUEFE_NAH(pil->y, e->y - 2 - 1);              // unter der Grundlinie: Abstieg 2 + 1
    PRUEFE(pil->farbe == 'w');
    PRUEFE_NAH(e->y, z->y);                        // Note und Etikett auf einer Grundlinie
    PRUEFE_NAH(l->x - (pil->x + pil->b), 13);      // danach normale Zelle
    // Die Pille steht im Elementfeld VOR ihrem Text (Zeichenreihenfolge)
    PRUEFE(pil < e);
}

TEST(band_layout_skaliert_alles) {
    const BandBild b = bild_aus(2, "ntvor 3 s\tdmPIREP\nbp\tniDLH\tdlRoute\tnvEDDF\tnz87\twe OK\n");
    ScheinMass m1(1.0f), m2(2.0f);
    band_ordne(b, m1, 1.0f, &g_lay);
    const float b1 = g_lay.breite, h1 = g_lay.hoehe;
    const float x1 = finde(g_lay, BandElementArt::TEXT, 1, 3)->x;
    band_ordne(b, m2, 2.0f, &g_lay);
    PRUEFE_NAH(g_lay.breite, 2 * b1);
    PRUEFE_NAH(g_lay.hoehe, 2 * h1);
    PRUEFE_NAH(finde(g_lay, BandElementArt::TEXT, 1, 3)->x, 2 * x1);
    PRUEFE_NAH(g_lay.skala, 2.0f);
}

TEST(band_layout_alles_in_der_box) {
    // Viele Läufe, alle Arten: kein Element ragt über die Innenkante.
    std::string z = "gp";
    for (int i = 0; i < 20; ++i) z += "\tdlL" + std::to_string(i) + "\tnvW" + std::to_string(i);
    const BandBild b = bild_aus(3, "ntvor 1 min\tdmx\n" + z + "\nnz9\tbeHARD\tdxAnhang\n");
    ScheinMass m(1.25f);
    band_ordne(b, m, 1.25f, &g_lay);
    const float innen = 16 * 1.25f;
    for (int i = 0; i < g_lay.n; ++i) {
        const BandElement& e = g_lay.e[i];
        PRUEFE(e.x >= innen - 0.01f);
        PRUEFE(e.x + e.b <= g_lay.breite - innen + 0.01f);
        if (e.art != BandElementArt::TEXT) {
            PRUEFE(e.y >= innen * 0 + 10 * 1.25f - 0.01f);
            PRUEFE(e.y + e.h <= g_lay.hoehe - 10 * 1.25f + 0.01f);
        }
    }
}

// ---- Schriftatlas (echte Open Sans) ---------------------------------------------------

TEST(band_atlas_backt_open_sans) {
    static BandAtlas a;
    PRUEFE(a.backe(1.0f, 2.0f));
    PRUEFE(a.bereit());
    const int w = a.breite_px(), h = a.hoehe_px();
    PRUEFE(w >= 256 && w <= 4096 && (w & (w - 1)) == 0);
    PRUEFE(h >= 128 && h <= 4096 && (h & (h - 1)) == 0);
    PRUEFE(a.pixel() != nullptr);
    if (a.pixel() == nullptr) return;
    // weißer Block: Deckkraft 255 in der Mitte seiner UV
    const BandUV& ws = a.weiss();
    const int wx = static_cast<int>(ws.s0 * w), wy = static_cast<int>(ws.t0 * h);
    PRUEFE_GLEICH(int(a.pixel()[(static_cast<size_t>(wy) * w + wx) * 4 + 3]), 255);
    PRUEFE_GLEICH(int(a.pixel()[(static_cast<size_t>(wy) * w + wx) * 4 + 0]), 255);  // RGB weiß
    // Kreis: Mitte deckend, Ecke leer
    const BandUV& k = a.kreis();
    const int kx0 = static_cast<int>(k.s0 * w), ky0 = static_cast<int>(k.t0 * h);
    PRUEFE_GLEICH(static_cast<int>((k.s1 - k.s0) * w + 0.5f), BAND_KREIS_PX);
    PRUEFE_GLEICH(int(a.pixel()[(static_cast<size_t>(ky0 + 32) * w + kx0 + 32) * 4 + 3]), 255);
    PRUEFE_GLEICH(int(a.pixel()[(static_cast<size_t>(ky0) * w + kx0) * 4 + 3]), 0);
    // Glyphe: 'H' hat Fläche mit Abdeckung, liegt im Atlas, oben über der Grundlinie
    const BandGlyph& g = a.glyph(BandStil::WERT, 'H');
    PRUEFE(g.s1 > g.s0 && g.t1 > g.t0 && g.s1 <= 1.0f && g.t1 <= 1.0f);
    PRUEFE(g.y1 > 8.0f && g.y1 < 13.0f);          // Versalhöhe Open Sans ≈ 0,71 em bei 15 px
    PRUEFE(g.y0 > -1.0f && g.y0 < 1.0f);
    PRUEFE(g.vorschub > 7.0f && g.vorschub < 13.0f);
    int abgedeckt = 0;
    for (int y = static_cast<int>(g.t0 * h); y < static_cast<int>(g.t1 * h); ++y)
        for (int x = static_cast<int>(g.s0 * w); x < static_cast<int>(g.s1 * w); ++x)
            abgedeckt += a.pixel()[(static_cast<size_t>(y) * w + x) * 4 + 3] > 128;
    PRUEFE(abgedeckt > 20);
    // Leerzeichen hat Vorschub, aber keine Fläche nötig
    PRUEFE(a.glyph(BandStil::WERT, ' ').vorschub > 2.0f);
    // Metriken = hhea der Open Sans: Aufstieg ≈ 1,069 em, Abstieg ≈ 0,293 em
    PRUEFE(nah(a.aufstieg(BandStil::WERT), 1.069f * 15, 0.1f));
    PRUEFE(nah(a.abstieg(BandStil::WERT), 0.293f * 15, 0.1f));
    a.pixel_freigeben();
    PRUEFE(a.pixel() == nullptr);
    PRUEFE(a.bereit());                           // Maße bleiben nach dem Freigeben
}

TEST(band_atlas_schnitte_gross_und_sperrung) {
    static BandAtlas a;
    PRUEFE(a.backe(1.0f, 2.0f));
    const char* w = "Hamburg";
    // gleiche Größe (13 px): Bold breiter als SemiBold
    PRUEFE(a.breite(BandStil::KENNUNG, w, 7) > a.breite(BandStil::LAGE, w, 7));
    // 11 px: Bold (Alter) breiter als Regular (Meldung)
    PRUEFE(a.breite(BandStil::ALTER, w, 7) > a.breite(BandStil::MELDUNG, w, 7));
    // Beschriftung wird GROSS gemessen
    PRUEFE(nah(a.breite(BandStil::BESCHRIFTUNG, "route", 5), a.breite(BandStil::BESCHRIFTUNG, "ROUTE", 5)));
    // Sperrung: je Zeichen 0,09 em = 0,81 px mehr als ohne
    // (gleicher Schnitt Bold: 9-px-Vorschub = 15-px-Vorschub × 9/15, linear ohne Hinting)
    const float mit = a.glyph(BandStil::BESCHRIFTUNG, 'I').vorschub;
    const float ohne = a.glyph(BandStil::WERT, 'I').vorschub * 9.0f / 15.0f;
    PRUEFE(nah(mit, ohne + 0.81f, 0.05f));
    const float etikett = a.glyph(BandStil::ETIKETT, 'I').vorschub;   // 10 px, 0,08 em
    PRUEFE(nah(etikett, a.glyph(BandStil::WERT, 'I').vorschub * 10.0f / 15.0f + 0.8f, 0.05f));
    // unbekanntes Zeichen fällt auf '?' zurück statt abzustürzen
    PRUEFE(nah(a.glyph(BandStil::WERT, '\x01').vorschub, a.glyph(BandStil::WERT, '?').vorschub));
    PRUEFE(nah(a.glyph(BandStil::WERT, static_cast<char>(0xC3)).vorschub, a.glyph(BandStil::WERT, '?').vorschub));
}

TEST(band_atlas_stufen) {
    static BandAtlas a1, a3;
    PRUEFE(a1.backe(1.0f, 2.0f));
    PRUEFE(a3.backe(3.0f, 2.0f));                 // größte Stufe passt (≤ 4096²)
    const float b1 = a1.breite(BandStil::WERT, "EDDF > KJFK", 11);
    const float b3 = a3.breite(BandStil::WERT, "EDDF > KJFK", 11);
    PRUEFE(nah(b3 / b1, 3.0f, 0.06f));
    PRUEFE(a3.breite_px() >= a1.breite_px());
    PRUEFE(nah(a3.skala(), 3.0f));
    // unsinnige Stufe/Dichte → abgelehnt, Atlas leer
    static BandAtlas x;
    PRUEFE(!x.backe(0.0f, 2.0f));
    PRUEFE(!x.backe(1.0f, 0.5f));
    PRUEFE(!x.bereit());
    PRUEFE(x.pixel() == nullptr);
}

TEST(band_atlas_mit_layout) {
    static BandAtlas a;
    PRUEFE(a.backe(1.0f, 2.0f));
    const BandBild b = bild_aus(2, "ntvor 3 s\tdmPIREP prefiled\nbp\tniDLH 400\tdlRoute\tnvEDDF > KJFK\n");
    band_ordne(b, a, 1.0f, &g_lay);
    // Größenordnung eines echten Flow-Bands: ~ 220–330 px breit, ~ 60–70 px hoch
    PRUEFE(g_lay.breite > 200 && g_lay.breite < 340);
    PRUEFE(g_lay.hoehe > 55 && g_lay.hoehe < 75);
}

// ---- Selbstprüfung ---------------------------------------------------------------------

TEST(band_pruefung_bewertung) {
    const uint8_t hell[4] = {120, 160, 220, 255};    // Himmel
    const uint8_t dunkel[4] = {70, 90, 120, 255};    // nach X-Plane-Box
    const uint8_t magenta[4] = {250, 8, 247, 255};
    const uint8_t schwarz[4] = {0, 0, 0, 0};
    // eindeutig OK
    PRUEFE(band_bewerte_pruefung(hell, dunkel, magenta, false, false) == BandPruefung::OK);
    // eindeutig nicht angekommen: Pixel bleibt auf dem Kontrollwert
    PRUEFE(band_bewerte_pruefung(hell, dunkel, dunkel, false, false) == BandPruefung::RUECKFALL);
    // Rücklesen blind (liest immer 0 oder immer dasselbe) → nicht urteilen
    PRUEFE(band_bewerte_pruefung(schwarz, schwarz, schwarz, false, false) == BandPruefung::UNKLAR);
    PRUEFE(band_bewerte_pruefung(hell, hell, hell, false, false) == BandPruefung::UNKLAR);
    // Nacht: Box ändert auf Schwarz nichts → unklar, auch wenn Magenta fehlt
    PRUEFE(band_bewerte_pruefung(schwarz, schwarz, schwarz, false, false) == BandPruefung::UNKLAR);
    // GL-Fehler beim Lesen → unklar; beim eigenen Zeichnen → Rückfall
    PRUEFE(band_bewerte_pruefung(hell, dunkel, dunkel, false, true) == BandPruefung::UNKLAR);
    PRUEFE(band_bewerte_pruefung(hell, dunkel, magenta, true, false) == BandPruefung::RUECKFALL);
    // irgendwas Drittes (z. B. halb gemischt) → unklar, kein Rückfall
    const uint8_t mischung[4] = {160, 50, 180, 255};
    PRUEFE(band_bewerte_pruefung(hell, dunkel, mischung, false, false) == BandPruefung::UNKLAR);
    // Grenzen: Kontrolle ändert nur um 3 → unklar; um 4 → gilt
    const uint8_t fast[4] = {117, 160, 220, 255};
    const uint8_t knapp[4] = {116, 160, 220, 255};
    PRUEFE(band_bewerte_pruefung(hell, fast, fast, false, false) == BandPruefung::UNKLAR);
    PRUEFE(band_bewerte_pruefung(hell, knapp, knapp, false, false) == BandPruefung::RUECKFALL);
}

TEST(band_projektion) {
    // X-Plane-Fenster: Ortho 0…1920 × 0…1080, Retina: Viewport doppelt so groß.
    float mv[16] = {1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1};
    float pr[16] = {2.0f / 1920, 0, 0, 0, 0, 2.0f / 1080, 0, 0, 0, 0, -1, 0, -1, -1, 0, 1};
    const int vp[4] = {0, 0, 3840, 2160};
    int x = -1, y = -1;
    PRUEFE(band_projiziere(mv, pr, vp, 960, 540, &x, &y));
    PRUEFE_GLEICH(x, 1920);
    PRUEFE_GLEICH(y, 1080);
    PRUEFE(band_projiziere(mv, pr, vp, 100.25f, 50.25f, &x, &y));
    PRUEFE_GLEICH(x, 200);
    PRUEFE_GLEICH(y, 100);
    // Modelview-Verschiebung wirkt mit
    mv[12] = 10;
    PRUEFE(band_projiziere(mv, pr, vp, 100, 50, &x, &y));
    PRUEFE_GLEICH(x, 220);
    mv[12] = 0;
    // außerhalb, kaputte Matrix, kaputter Viewport → false
    PRUEFE(!band_projiziere(mv, pr, vp, 5000, 50, &x, &y));
    const float null16[16] = {0};
    PRUEFE(!band_projiziere(null16, null16, vp, 1, 1, &x, &y));
    const int vp0[4] = {0, 0, 0, 0};
    PRUEFE(!band_projiziere(mv, pr, vp0, 1, 1, &x, &y));
    float nan_pr[16];
    std::memcpy(nan_pr, pr, sizeof(nan_pr));
    nan_pr[0] = std::nanf("");
    PRUEFE(!band_projiziere(mv, nan_pr, vp, 1, 1, &x, &y));
}
