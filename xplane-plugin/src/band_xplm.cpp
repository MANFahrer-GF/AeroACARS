// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band an XPLM angebunden (ADR-0005 + Nachtrag)
// =============================================================================
//
// Fenster, Zeichnung, Maus, Menü und Merken. Anordnung (band_layout.cpp),
// Schriftatlas (band_schrift.cpp), Parser und Zustand (band.cpp) sind
// XPLM-frei und getestet; diese Schicht malt nur ab.
//
// Zeichenwege (Feldbefund X-Plane 12.4.3, macOS/Metal, 04.10.2026):
//   * TEXTURIERT (Standard): alles — Box, Rand, Linie, Punkt, Pille, Schrift —
//     als texturierte Quads mit EINER Textur (Atlas aus band_schrift.cpp):
//     XPLMSetGraphicsState(0,1,0,0,1,0,0) + XPLMBindTexture2d + GL_QUADS mit
//     glTexCoord, wie AviTab/ImGui auf XP12. Untexturierte GL-Flächen
//     erschienen im Feldtest NICHT.
//   * RÜCKFALL: XPLMDrawTranslucentDarkBox + XPLMDrawString (beide im
//     Feldtest sichtbar). Gewählt, wenn der Atlas oder die Textur nicht
//     entstehen oder die Selbstprüfung EINDEUTIG meldet, dass die eigene
//     Fläche nicht ankommt — dann für den Rest der Sitzung. So bleibt das Band
//     immer lesbar.
//   * Selbstprüfung: einmalig (mit Wiederholungen) per glReadPixels an der
//     Boxmitte, mit X-Planes eigener Box als Kontrolle; Rohwerte ins Log.
//     Sagt das Rücklesen nichts aus, bleibt es beim texturierten Weg.
//
// Texturen entstehen nur in band_frame() (Flight-Loop), nie im Zeichen-Callback.
// Grundsätze: Hauptthread, keine Threads, keine Ausnahmen, kein Blockieren.
// Jeder Fehlschlag wird protokolliert und das Band läuft ohne diese Funktion
// weiter. Dateizugriff nur beim Start (lesen) und bei einer Änderung
// (schreiben: Mausloslassen, Menü, Rad verzögert) — nie im Draw.
// =============================================================================

#include "band_xplm.h"

#include "band.h"
#include "band_layout.h"
#include "band_schrift.h"
#include "dienst_xplm.h"
#include "grenzen.h"

#include <XPLM/XPLMDefs.h>
#include <XPLM/XPLMDisplay.h>
#include <XPLM/XPLMGraphics.h>
#include <XPLM/XPLMMenus.h>
#include <XPLM/XPLMPlugin.h>
#include <XPLM/XPLMUtilities.h>

#if IBM
    #include <windows.h>
    #include <GL/gl.h>
#elif APL
    #ifndef GL_SILENCE_DEPRECATION
        #define GL_SILENCE_DEPRECATION 1
    #endif
    #include <OpenGL/gl.h>
#else
    #include <GL/gl.h>
#endif

#if !IBM
    #include <dlfcn.h>
#endif
#ifndef APIENTRY
    #define APIENTRY
#endif
#ifndef GL_CURRENT_PROGRAM
    #define GL_CURRENT_PROGRAM 0x8B8D
#endif
#ifndef GL_ARRAY_BUFFER
    #define GL_ARRAY_BUFFER 0x8892
#endif
#ifndef GL_ELEMENT_ARRAY_BUFFER
    #define GL_ELEMENT_ARRAY_BUFFER 0x8893
#endif
#ifndef GL_ARRAY_BUFFER_BINDING
    #define GL_ARRAY_BUFFER_BINDING 0x8894
#endif
#ifndef GL_FRAMEBUFFER_BINDING
    #define GL_FRAMEBUFFER_BINDING 0x8CA6
#endif
#ifndef GL_CLAMP_TO_EDGE
    #define GL_CLAMP_TO_EDGE 0x812F  // Windows-gl.h kennt nur OpenGL 1.1
#endif

#include <chrono>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>

using namespace aeroacars;

namespace {

constexpr double WELLE_VERZOEGERUNG_S = 1.0;  // Rad: Prefs erst nach Ruhe schreiben
constexpr float DICHTE = 2.0f;                 // Atlas in 2× Auflösung (Retina, Skalierung)
constexpr int PRUEF_MAX_UNKLAR = 5;            // so oft "unklar" → sicherer Rueckfall (Band muss lesbar sein)
constexpr double PRUEF_ABSTAND_S = 1.0;

constexpr const char* PREFS_NAME = "AeroACARS-Band.txt";

enum class Weg : uint8_t { TEXTURIERT, RUECKFALL };

struct Zustand {
    XPLMWindowID fenster = nullptr;
    XPLMMenuID menu = nullptr;
    int menu_eintrag = -1;     // Eintrag in X-Planes Plugins-Menü
    int punkt_an = -1;         // Index "Band ein/aus" im Untermenü
    BandPrefs prefs;
    bool platziert = false;    // Position für die gemessene Größe gesetzt?
    bool sichtbar = false;
    bool ziehen = false;
    bool bewegt = false;
    int maus_x = 0, maus_y = 0;
    bool prefs_offen = false;      // Änderung noch nicht geschrieben
    double prefs_faellig = 0.0;    // > 0: nach dieser Zeit schreiben (Rad)
    char prefs_pfad[600] = {0};
    // Zeichenweg
    Weg weg = Weg::TEXTURIERT;
    bool gl_gemeldet = false;
    int textur = 0;                // XPLM-Texturnummer, 0 = keine
    bool textur_bereit = false;
    float textur_skala = 0.0f;     // Stufe, für die der Atlas gebacken ist
    // Selbstprüfung
    bool pruef_fertig = false;
    int pruef_unklar = 0;
    int pruef_rueckfall = 0;
    double pruef_naechste = 0.0;
    int variante = 1;              // V_CULL: Grundstellung wie ImgWindow; Selbstprüfung bestätigt/erweitert
};

Zustand g;

void log_zeile(const char* text) noexcept {
    char zeile[300];
    std::snprintf(zeile, sizeof(zeile), "[AeroACARS] Band: %s\n", text ? text : "");
    XPLMDebugString(zeile);
}

// ---- Prefs-Datei ---------------------------------------------------------------

void ermittle_prefs_pfad() noexcept {
    char voll[512] = {0};
    XPLMGetPrefsPath(voll);
    XPLMExtractFileAndPath(voll);  // bleibt: Verzeichnis (in-place)
    const char* sep = XPLMGetDirectorySeparator();
    std::snprintf(g.prefs_pfad, sizeof(g.prefs_pfad), "%s%s%s", voll, sep ? sep : "/", PREFS_NAME);
}

void lese_prefs() noexcept {
    g.prefs = BandPrefs{};
    if (g.prefs_pfad[0] == '\0') return;
    FILE* f = std::fopen(g.prefs_pfad, "rb");
    if (f == nullptr) return;  // erster Start: Standard
    char puffer[grenzen::BAND_PREFS_MAX + 1];
    const size_t n = std::fread(puffer, 1, sizeof(puffer), f);
    std::fclose(f);
    g.prefs = band_prefs_lese(puffer, n);  // zu große Datei → Standard
}

void schreibe_prefs() noexcept {
    g.prefs_offen = false;
    g.prefs_faellig = 0.0;
    if (g.prefs_pfad[0] == '\0') return;
    char puffer[128];
    const size_t n = band_prefs_schreibe(g.prefs, puffer, sizeof(puffer));
    if (n == 0) return;
    FILE* f = std::fopen(g.prefs_pfad, "wb");
    if (f == nullptr) {
        log_zeile("Prefs-Datei nicht schreibbar - Einstellungen gelten nur bis zum Neustart");
        return;
    }
    std::fwrite(puffer, 1, n, f);
    std::fclose(f);
}

void prefs_aendern_sofort() noexcept { schreibe_prefs(); }

// ---- Geometrie ------------------------------------------------------------------

BandRechteck schirm() noexcept {
    BandRechteck r;
    XPLMGetScreenBoundsGlobal(&r.links, &r.oben, &r.rechts, &r.unten);
    return r;
}

// Setzt Größe (und bei Bedarf Position) des Fensters. links/oben bleiben,
// wenn das Fenster schon platziert ist; sonst gemerkte Position bzw. Standard
// (oben mittig). Immer auf den Schirm geklemmt.
void passe_fenster_an(int w, int h) noexcept {
    if (w < 1) w = 1;
    if (h < 1) h = 1;
    int l = 0, o = 0, r = 0, u = 0;
    XPLMGetWindowGeometry(g.fenster, &l, &o, &r, &u);
    const BandRechteck s = schirm();
    int nl = l, no = o;
    if (!g.platziert || !g.prefs.pos_gesetzt) {
        if (!g.platziert && g.prefs.pos_gesetzt) {
            nl = g.prefs.links;
            no = g.prefs.oben;
            band_klemme_position(s, w, h, &nl, &no);
        } else {
            band_standard_position(s, w, h, &nl, &no);
        }
    } else {
        band_klemme_position(s, w, h, &nl, &no);
    }
    g.platziert = true;
    if (nl != l || no != o || (r - l) != w || (o - u) != h) {
        XPLMSetWindowGeometry(g.fenster, nl, no, nl + w, no - h);
    }
}

// Im Flight-Loop: das Fenster liegt immer (wenigstens teilweise) auf dem
// Schirm, auch beim ersten Zeigen und nach einem Monitor-/Größenwechsel.
// Die endgültige Größe und gemerkte Position setzt passe_fenster_an() im Draw.
void halte_auf_schirm() noexcept {
    BandRechteck f;
    XPLMGetWindowGeometry(g.fenster, &f.links, &f.oben, &f.rechts, &f.unten);
    int nl = f.links, no = f.oben;
    band_vor_dem_zeigen(schirm(), f, g.platziert, &nl, &no);
    if (nl != f.links || no != f.oben) {
        XPLMSetWindowGeometry(g.fenster, nl, no, nl + (f.rechts - f.links), no - (f.oben - f.unten));
    }
}

BandAtlas g_atlas;
BandLayout g_layout;

void gl_fehler_leeren() noexcept {
    for (int i = 0; i < 16 && glGetError() != GL_NO_ERROR; ++i) {
    }
}

void wechsle_auf_rueckfall(const char* grund) noexcept {
    if (g.weg == Weg::RUECKFALL) return;
    g.weg = Weg::RUECKFALL;
    char t[300];
    std::snprintf(t, sizeof(t), "Zeichenweg RUECKFALL (X-Plane-Box + XPLMDrawString): %s", grund);
    log_zeile(t);
}

// ---- Textur (nur aus band_frame, also außerhalb des Zeichen-Callbacks) ----------

void bereite_textur() noexcept {
    // GL erst anfassen, wenn das Band wirklich gezeigt wird (dann ist sicher
    // ein Kontext da) — einmal die Kennung ins Log.
    if (!g.gl_gemeldet) {
        g.gl_gemeldet = true;
        const GLubyte* v = glGetString(GL_VERSION);
        const GLubyte* r = glGetString(GL_RENDERER);
        char t[400];
        std::snprintf(t, sizeof(t), "GL_VERSION %s, GL_RENDERER %s, Zeichenweg %s",
                      v ? reinterpret_cast<const char*>(v) : "?", r ? reinterpret_cast<const char*>(r) : "?",
                      g.weg == Weg::TEXTURIERT ? "texturiert (Selbstpruefung folgt)" : "Rueckfall");
        log_zeile(t);
    }

    const float skala = band_skalierung(g.prefs.stufe);
    if (g.textur_bereit && g.textur_skala == skala) return;
    using uhr = std::chrono::steady_clock;
    const auto t0 = uhr::now();
    if (!g_atlas.backe(skala, DICHTE)) {
        wechsle_auf_rueckfall("Schriftatlas nicht backbar (Speicher?)");
        return;
    }
    gl_fehler_leeren();
    if (g.textur == 0) XPLMGenerateTextureNumbers(&g.textur, 1);
    if (g.textur == 0) {
        g_atlas.pixel_freigeben();
        wechsle_auf_rueckfall("XPLMGenerateTextureNumbers lieferte keine Nummer");
        return;
    }
    XPLMBindTexture2d(g.textur, 0);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
    glPixelStorei(GL_UNPACK_ALIGNMENT, 4);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, g_atlas.breite_px(), g_atlas.hoehe_px(), 0, GL_RGBA,
                 GL_UNSIGNED_BYTE, g_atlas.pixel());
    const GLenum e = glGetError();
    const int bw = g_atlas.breite_px(), bh = g_atlas.hoehe_px();
    g_atlas.pixel_freigeben();
    if (e != GL_NO_ERROR) {
        char t[120];
        std::snprintf(t, sizeof(t), "glTexImage2D-Fehler 0x%04X", static_cast<unsigned>(e));
        wechsle_auf_rueckfall(t);
        return;
    }
    g.textur_bereit = true;
    g.textur_skala = skala;
    const double ms = std::chrono::duration<double, std::milli>(uhr::now() - t0).count();
    char t[160];
    std::snprintf(t, sizeof(t), "Schriftatlas %dx%d fuer Stufe %.2f in %.1f ms", bw, bh,
                  static_cast<double>(skala), ms);
    log_zeile(t);
}

// ---- Texturierter Zeichenweg -------------------------------------------------------

// Ein Quad; (s0,t0) gehört zur Ecke links OBEN.
inline void quad(float x0, float y0, float x1, float y1, float s0, float t0, float s1, float t1) noexcept {
    glTexCoord2f(s0, t1); glVertex2f(x0, y0);
    glTexCoord2f(s1, t1); glVertex2f(x1, y0);
    glTexCoord2f(s1, t0); glVertex2f(x1, y1);
    glTexCoord2f(s0, t0); glVertex2f(x0, y1);
}

inline void flaeche(float x0, float y0, float x1, float y1) noexcept {
    const BandUV& w = g_atlas.weiss();
    quad(x0, y0, x1, y1, w.s0, w.t0, w.s1, w.t1);
}

// Abgerundetes Rechteck aus Fläche + vier Kreisvierteln (innerhalb glBegin).
void rundrechteck(float x, float y, float b, float h, float r) noexcept {
    if (r * 2 > b) r = b / 2;
    if (r * 2 > h) r = h / 2;
    const BandUV& k = g_atlas.kreis();
    const float sm = (k.s0 + k.s1) / 2, tm = (k.t0 + k.t1) / 2;
    flaeche(x + r, y, x + b - r, y + h);            // Mitte über die ganze Höhe
    flaeche(x, y + r, x + r, y + h - r);            // linker Streifen
    flaeche(x + b - r, y + r, x + b, y + h - r);    // rechter Streifen
    quad(x, y, x + r, y + r, k.s0, tm, sm, k.t1);                   // unten links
    quad(x + b - r, y, x + b, y + r, sm, tm, k.s1, k.t1);           // unten rechts
    quad(x, y + h - r, x + r, y + h, k.s0, k.t0, sm, tm);           // oben links
    quad(x + b - r, y + h - r, x + b, y + h, sm, k.t0, k.s1, tm);   // oben rechts
}

// Auf das Raster der Atlasdichte runden (Retina: halbe Boxel).
inline float raster(float v) noexcept { return std::floor(v * DICHTE + 0.5f) / DICHTE; }

void text(const BandBild& bild, const BandElement& e, float ox, float oy) noexcept {
    const BandZeile& z = bild.zeile[e.zeile];
    const BandLauf& l = z.laeufe[e.lauf];
    float stift = raster(ox + e.x);
    const float grund = raster(oy + e.y);
    for (uint16_t i = 0; i < l.laenge; ++i) {
        const BandGlyph& gl = g_atlas.glyph(e.stil, z.text[l.ofs + i]);
        if (gl.x1 > gl.x0) quad(stift + gl.x0, grund + gl.y0, stift + gl.x1, grund + gl.y1, gl.s0, gl.t0, gl.s1, gl.t1);
        stift += gl.vorschub;
    }
}

// ---- GL-Zustandskorrektur (Feldtest 04.10.2026) -------------------------------
// Messung im Sim: X-Planes eigene Box veraendert das Rueckgelesene Pixel, unsere
// texturierte Flaeche nicht - ohne GL-Fehler. Unsere Befehle kommen also an,
// werden aber von einem Zustand verworfen, den X-Plane unter Metal hinterlaesst.
// Kandidaten, je ein Bit: Rueckseiten-Wegschneiden (X-Plane spiegelt Y unter
// Metal), gebundener Shader/Puffer, Masken (Stencil/Farbe/Tiefe), Schere.
// Die Selbstpruefung probiert die Stufen in EINEM Frame der Reihe nach und
// nimmt die erste, die sichtbar zeichnet. Alles wird danach zurueckgesetzt.
enum : int { V_CULL = 1, V_PROG = 2, V_MASKE = 4, V_SCHERE = 8 };
// Belegt (04.10.2026, Quelltext gelesen): ImgWindow (ImGui-Fenster, laeuft auf
// XP12) schaltet vor dem Zeichnen ausdruecklich glDisable(GL_CULL_FACE); AviTab
// legt seine Quads im Uhrzeigersinn an (l,b)->(l,t)->(r,t)->(r,b). Unsere Quads
// laufen gegen den Uhrzeigersinn -> von X-Planes Rueckseiten-Wegschneiden
// verworfen. Darum ist V_CULL die Grundstellung; die weiteren Stufen sind nur
// Netz, falls die Selbstpruefung noch etwas anderes findet.
constexpr int VARIANTEN[] = {V_CULL, V_CULL | V_PROG, V_CULL | V_PROG | V_MASKE,
                             V_CULL | V_PROG | V_MASKE | V_SCHERE};

using PfnUseProgram = void(APIENTRY*)(GLuint);
using PfnBindBuffer = void(APIENTRY*)(GLenum, GLuint);
PfnUseProgram gl_use_program = nullptr;
PfnBindBuffer gl_bind_buffer = nullptr;

void* gl_proc(const char* name) noexcept {
#if IBM
    return reinterpret_cast<void*>(wglGetProcAddress(name));
#else
    return dlsym(RTLD_DEFAULT, name);
#endif
}

void lade_gl_funktionen() noexcept {
    if (gl_use_program == nullptr) gl_use_program = reinterpret_cast<PfnUseProgram>(gl_proc("glUseProgram"));
    if (gl_bind_buffer == nullptr) gl_bind_buffer = reinterpret_cast<PfnBindBuffer>(gl_proc("glBindBuffer"));
}

struct GlSicherung {
    GLboolean cull = GL_FALSE, stencil = GL_FALSE, schere = GL_FALSE, tiefe = GL_FALSE;
    GLboolean maske[4] = {GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE};
    GLint prog = 0, puffer = 0;
};

void setze_variante(int v, GlSicherung& s) noexcept {
    if (v & V_CULL) {
        s.cull = glIsEnabled(GL_CULL_FACE);
        glDisable(GL_CULL_FACE);
    }
    if (v & V_PROG) {
        lade_gl_funktionen();
        glGetIntegerv(GL_CURRENT_PROGRAM, &s.prog);
        glGetIntegerv(GL_ARRAY_BUFFER_BINDING, &s.puffer);
        if (gl_use_program != nullptr) gl_use_program(0);
        if (gl_bind_buffer != nullptr) {
            gl_bind_buffer(GL_ARRAY_BUFFER, 0);
            gl_bind_buffer(GL_ELEMENT_ARRAY_BUFFER, 0);
        }
    }
    if (v & V_MASKE) {
        s.stencil = glIsEnabled(GL_STENCIL_TEST);
        s.tiefe = glIsEnabled(GL_DEPTH_TEST);
        glGetBooleanv(GL_COLOR_WRITEMASK, s.maske);
        glDisable(GL_STENCIL_TEST);
        glDisable(GL_DEPTH_TEST);
        glColorMask(GL_TRUE, GL_TRUE, GL_TRUE, GL_TRUE);
    }
    if (v & V_SCHERE) {
        s.schere = glIsEnabled(GL_SCISSOR_TEST);
        glDisable(GL_SCISSOR_TEST);
    }
}

void stelle_wieder_her(int v, const GlSicherung& s) noexcept {
    if ((v & V_SCHERE) && s.schere) glEnable(GL_SCISSOR_TEST);
    if (v & V_MASKE) {
        if (s.stencil) glEnable(GL_STENCIL_TEST);
        if (s.tiefe) glEnable(GL_DEPTH_TEST);
        glColorMask(s.maske[0], s.maske[1], s.maske[2], s.maske[3]);
    }
    if (v & V_PROG) {
        if (gl_use_program != nullptr) gl_use_program(static_cast<GLuint>(s.prog));
        if (gl_bind_buffer != nullptr) gl_bind_buffer(GL_ARRAY_BUFFER, static_cast<GLuint>(s.puffer));
    }
    if ((v & V_CULL) && s.cull) glEnable(GL_CULL_FACE);
}

// GL-Zustand beim Eintritt ins Zeichnen, einmal ins Log (Diagnose ohne neuen Start).
void logge_gl_zustand() noexcept {
    GLint prog = 0, puffer = 0, fb = 0, vorn = 0, cullmodus = 0, scher[4] = {0}, tex_env = 0;
    GLboolean maske[4] = {0, 0, 0, 0};
    glGetIntegerv(GL_CURRENT_PROGRAM, &prog);
    glGetIntegerv(GL_ARRAY_BUFFER_BINDING, &puffer);
    glGetIntegerv(GL_FRAMEBUFFER_BINDING, &fb);
    glGetIntegerv(GL_FRONT_FACE, &vorn);
    glGetIntegerv(GL_CULL_FACE_MODE, &cullmodus);
    glGetIntegerv(GL_SCISSOR_BOX, scher);
    glGetBooleanv(GL_COLOR_WRITEMASK, maske);
    glGetTexEnviv(GL_TEXTURE_ENV, GL_TEXTURE_ENV_MODE, &tex_env);
    char t[420];
    std::snprintf(t, sizeof(t),
                  "GL-Zustand: Programm %d, Puffer %d, Framebuffer %d, Cull %d (Modus 0x%X, Vorderseite 0x%X), "
                  "Stencil %d, Tiefe %d, Schere %d (%d %d %d %d), Farbmaske %d%d%d%d, Blend %d, Textur2D %d, "
                  "Lighting %d, TexEnv 0x%X, glUseProgram %s",
                  prog, puffer, fb, glIsEnabled(GL_CULL_FACE) ? 1 : 0, cullmodus, vorn,
                  glIsEnabled(GL_STENCIL_TEST) ? 1 : 0, glIsEnabled(GL_DEPTH_TEST) ? 1 : 0,
                  glIsEnabled(GL_SCISSOR_TEST) ? 1 : 0, scher[0], scher[1], scher[2], scher[3], maske[0] ? 1 : 0,
                  maske[1] ? 1 : 0, maske[2] ? 1 : 0, maske[3] ? 1 : 0, glIsEnabled(GL_BLEND) ? 1 : 0,
                  glIsEnabled(GL_TEXTURE_2D) ? 1 : 0, glIsEnabled(GL_LIGHTING) ? 1 : 0, tex_env,
                  gl_use_program != nullptr ? "da" : "fehlt");
    log_zeile(t);
}

void zeichne_texturiert(const BandBild& bild, float ox, float oy, float deck) noexcept {
    const float k = g_layout.skala;
    const float b = std::floor(g_layout.breite + 0.5f), h = std::floor(g_layout.hoehe + 0.5f);
    XPLMSetGraphicsState(0, 1, 0, 0, 1, 0, 0);
    XPLMBindTexture2d(g.textur, 0);
    GlSicherung sicherung;
    setze_variante(g.variante, sicherung);
    glBegin(GL_QUADS);
    // Kasten: Rand (1 px) und darin die Fläche.
    glColor4f(bandmass::BOX_RAND[0], bandmass::BOX_RAND[1], bandmass::BOX_RAND[2], bandmass::BOX_RAND[3] * deck);
    rundrechteck(ox, oy, b, h, bandmass::ECKE * k);
    const float rd = bandmass::RAND * k;
    glColor4f(bandmass::BOX[0], bandmass::BOX[1], bandmass::BOX[2], bandmass::BOX[3] * deck);
    rundrechteck(ox + rd, oy + rd, b - 2 * rd, h - 2 * rd, (bandmass::ECKE - bandmass::RAND) * k);

    // Linien, Punkte, Pillen
    for (uint16_t i = 0; i < g_layout.n; ++i) {
        const BandElement& e = g_layout.e[i];
        float rgb[3];
        switch (e.art) {
            case BandElementArt::LINIE:
                glColor4f(bandmass::LINIE[0], bandmass::LINIE[1], bandmass::LINIE[2], bandmass::LINIE[3] * deck);
                flaeche(ox + e.x, raster(oy + e.y), ox + e.x + e.b, raster(oy + e.y) + e.h);
                break;
            case BandElementArt::PUNKT: {
                band_farbe(e.farbe, 1.0f, rgb);
                glColor4f(rgb[0], rgb[1], rgb[2], deck);
                const BandUV& kr = g_atlas.kreis();
                const float x = raster(ox + e.x), y = raster(oy + e.y);
                quad(x, y, x + e.b, y + e.h, kr.s0, kr.t0, kr.s1, kr.t1);
                break;
            }
            case BandElementArt::PILLE:
                band_farbe(e.farbe, 1.0f, rgb);
                glColor4f(rgb[0], rgb[1], rgb[2], bandmass::PILLE_ALPHA * deck);
                rundrechteck(raster(ox + e.x), raster(oy + e.y), e.b, e.h, e.h / 2);
                break;
            case BandElementArt::TEXT:
                break;
        }
    }
    // Schrift: erst der Schatten (0 1px, 75 % schwarz), dann die Farbe.
    glColor4f(0.0f, 0.0f, 0.0f, bandmass::SCHATTEN_ALPHA * deck);
    for (uint16_t i = 0; i < g_layout.n; ++i) {
        if (g_layout.e[i].art == BandElementArt::TEXT) text(bild, g_layout.e[i], ox, oy - bandmass::SCHATTEN_Y * k);
    }
    for (uint16_t i = 0; i < g_layout.n; ++i) {
        const BandElement& e = g_layout.e[i];
        if (e.art != BandElementArt::TEXT) continue;
        float rgb[3];
        band_farbe(e.farbe, 1.0f, rgb);
        glColor4f(rgb[0], rgb[1], rgb[2], deck);
        text(bild, e, ox, oy);
    }
    glEnd();
    stelle_wieder_her(g.variante, sicherung);
}

// ---- Selbstprüfung (im Zeichen-Callback, vor dem eigentlichen Band) -------------------

void lies_pixel(int x, int y, uint8_t aus[4], bool* fehler) noexcept {
    aus[0] = aus[1] = aus[2] = aus[3] = 0;
    glReadPixels(x, y, 1, 1, GL_RGBA, GL_UNSIGNED_BYTE, aus);
    if (glGetError() != GL_NO_ERROR) *fehler = true;
}

void pruefe_zeichenweg(float cx, float cy) noexcept {
    const double jetzt = dienst_jetzt();
    if (g.pruef_fertig || jetzt < g.pruef_naechste) return;
    g.pruef_naechste = jetzt + PRUEF_ABSTAND_S;

    gl_fehler_leeren();
    float mv[16] = {0}, pr[16] = {0};
    int vp[4] = {0};
    glGetFloatv(GL_MODELVIEW_MATRIX, mv);
    glGetFloatv(GL_PROJECTION_MATRIX, pr);
    glGetIntegerv(GL_VIEWPORT, vp);
    bool fehler_lesen = glGetError() != GL_NO_ERROR;
    int px = 0, py = 0;
    const bool getroffen = band_projiziere(mv, pr, vp, cx, cy, &px, &py);

    uint8_t vorher[4] = {0}, kontrolle[4] = {0}, test[4] = {0};
    bool fehler_zeichnen = false;
    BandPruefung befund = BandPruefung::UNKLAR;
    if (getroffen && !fehler_lesen) {
        lies_pixel(px, py, vorher, &fehler_lesen);
        const int ix = static_cast<int>(cx), iy = static_cast<int>(cy);
        XPLMDrawTranslucentDarkBox(ix - 5, iy + 5, ix + 5, iy - 5);   // Kontrolle: zeichnet sicher
        XPLMDrawTranslucentDarkBox(ix - 5, iy + 5, ix + 5, iy - 5);
        lies_pixel(px, py, kontrolle, &fehler_lesen);
        gl_fehler_leeren();  // X-Planes eigene Box soll nicht als unser Fehler zählen
        lade_gl_funktionen();
        logge_gl_zustand();
        for (int vi = 0; vi < static_cast<int>(sizeof(VARIANTEN) / sizeof(VARIANTEN[0])); ++vi) {
            const int v = VARIANTEN[vi];
            gl_fehler_leeren();
            XPLMSetGraphicsState(0, 1, 0, 0, 1, 0, 0);
            XPLMBindTexture2d(g.textur, 0);
            GlSicherung sich;
            setze_variante(v, sich);
            glColor4f(BAND_PRUEF_FARBE[0] / 255.0f, BAND_PRUEF_FARBE[1] / 255.0f, BAND_PRUEF_FARBE[2] / 255.0f, 1.0f);
            glBegin(GL_QUADS);
            flaeche(cx - 4, cy - 4, cx + 4, cy + 4);
            glEnd();
            fehler_zeichnen = glGetError() != GL_NO_ERROR;
            stelle_wieder_her(v, sich);
            lies_pixel(px, py, test, &fehler_lesen);
            befund = band_bewerte_pruefung(vorher, kontrolle, test, fehler_zeichnen, fehler_lesen);
            char tv[160];
            std::snprintf(tv, sizeof(tv), "Selbstpruefung Variante %d: Test %u/%u/%u/%u, GL-Fehler %d -> %s", v,
                          test[0], test[1], test[2], test[3], fehler_zeichnen ? 1 : 0,
                          befund == BandPruefung::OK ? "OK" : (befund == BandPruefung::RUECKFALL ? "nein" : "unklar"));
            log_zeile(tv);
            if (befund == BandPruefung::OK) {
                g.variante = v;
                break;
            }
            if (befund == BandPruefung::UNKLAR) break;  // Ruecklesen sagt nichts - keine weitere Variante
        }
    }

    char t[400];
    std::snprintf(t, sizeof(t),
                  "Selbstpruefung: Punkt (%.0f,%.0f)->Pixel (%d,%d)%s, Viewport %d %d %d %d, "
                  "vorher %u/%u/%u/%u, Kontrolle %u/%u/%u/%u, Test %u/%u/%u/%u, "
                  "GL-Fehler zeichnen %d lesen %d -> %s",
                  static_cast<double>(cx), static_cast<double>(cy), px, py, getroffen ? "" : " (ausserhalb)",
                  vp[0], vp[1], vp[2], vp[3], vorher[0], vorher[1], vorher[2], vorher[3], kontrolle[0],
                  kontrolle[1], kontrolle[2], kontrolle[3], test[0], test[1], test[2], test[3],
                  fehler_zeichnen ? 1 : 0, fehler_lesen ? 1 : 0,
                  befund == BandPruefung::OK ? "OK" : (befund == BandPruefung::RUECKFALL ? "RUECKFALL" : "UNKLAR"));
    log_zeile(t);

    switch (befund) {
        case BandPruefung::OK:
            g.pruef_fertig = true;
            {
                char ok[80];
                std::snprintf(ok, sizeof(ok), "Zeichenweg texturiert OK (Variante %d)", g.variante);
                log_zeile(ok);
            }
            break;
        case BandPruefung::RUECKFALL:
            // Zweimal in getrennten Frames, damit ein einzelner Ausreißer nicht reicht.
            if (++g.pruef_rueckfall >= 2) {
                g.pruef_fertig = true;
                wechsle_auf_rueckfall("Selbstpruefung: eigene texturierte Flaeche kam nicht an");
            }
            break;
        case BandPruefung::UNKLAR:
            if (++g.pruef_unklar >= PRUEF_MAX_UNKLAR) {
                g.pruef_fertig = true;
                // Entscheidung Koordinator 04.10.2026: ein unsichtbares Band ist
                // schlimmer als ein schlichtes. Ohne eindeutigen Befund daher den
                // im Feld nachweislich sichtbaren Weg nehmen; die Rohwerte im Log
                // zeigen, ob die Pruefung beim naechsten Mal schaerfer werden muss.
                wechsle_auf_rueckfall("Selbstpruefung ohne eindeutigen Befund - sicherer Rueckfall");
            }
            break;
    }
}

// ---- Rückfall: X-Plane-Box + XPLMDrawString (Feldtest: sichtbar) ---------------------

class XplmMass final : public BandSchriftmass {
public:
    float breite(BandStil s, const char* t, size_t n) const noexcept override {
        char tmp[grenzen::MAX_ZEILE + 1];
        if (n > grenzen::MAX_ZEILE) n = grenzen::MAX_ZEILE;
        for (size_t i = 0; i < n; ++i) tmp[i] = band_stil_zeichen(s, t[i]);
        tmp[n] = '\0';
        return XPLMMeasureString(xplmFont_Proportional, tmp, static_cast<int>(n));
    }
    float aufstieg(BandStil) const noexcept override { return hoehe() * 0.85f; }
    float abstieg(BandStil) const noexcept override { return hoehe() * 0.35f; }

private:
    static float hoehe() noexcept {
        int fh = 0;
        XPLMGetFontDimensions(xplmFont_Proportional, nullptr, &fh, nullptr);
        return (fh > 0 && fh < 64) ? static_cast<float>(fh) : 10.0f;
    }
};
XplmMass g_xplm_mass;

struct RueckfallAuftrag {
    const BandBild* bild;
    float faktor;
};

void zeichne_rueckfall_inhalt(void* ctx) noexcept {
    const RueckfallAuftrag& a = *static_cast<const RueckfallAuftrag*>(ctx);
    const int b = static_cast<int>(g_layout.breite), h = static_cast<int>(g_layout.hoehe);
    XPLMDrawTranslucentDarkBox(0, h, b, 0);
    if (a.faktor >= 0.99f) XPLMDrawTranslucentDarkBox(0, h, b, 0);   // wach: deckender
    for (uint16_t i = 0; i < g_layout.n; ++i) {
        const BandElement& e = g_layout.e[i];
        float rgb[3];
        band_farbe(e.farbe, a.faktor, rgb);
        if (e.art == BandElementArt::PUNKT) {
            char stern[] = "*";
            XPLMDrawString(rgb, static_cast<int>(e.x), static_cast<int>(e.y), stern, nullptr, xplmFont_Proportional);
        } else if (e.art == BandElementArt::TEXT) {
            const BandZeile& z = a.bild->zeile[e.zeile];
            const BandLauf& l = z.laeufe[e.lauf];
            char tmp[grenzen::MAX_ZEILE + 1];
            for (uint16_t k = 0; k < l.laenge; ++k) tmp[k] = band_stil_zeichen(e.stil, z.text[l.ofs + k]);
            tmp[l.laenge] = '\0';
            XPLMDrawString(rgb, static_cast<int>(e.x), static_cast<int>(e.y), tmp, nullptr, xplmFont_Proportional);
        }
    }
}

// Rückfall skaliert die ganze Zeichnung per Matrix (Feldtest: XPLMDrawString
// folgt der Modelview). Die einzige Stelle mit Matrix-Aufrufen.
void zeichne_skaliert(float links, float unten, float skala, void (*inhalt)(void*), void* ctx) noexcept {
    glPushMatrix();
    glTranslatef(links, unten, 0.0f);
    glScalef(skala, skala, 1.0f);
    inhalt(ctx);
    glPopMatrix();
}

// ---- Fenster-Callbacks -----------------------------------------------------------

void auf_zeichnen(XPLMWindowID, void*) noexcept {
    Band* band = dienst_band();
    if (band == nullptr) return;
    const BandBild* bild = band->ansicht(dienst_jetzt());
    if (bild == nullptr || !g.prefs.an || !band_hat_inhalt(*bild)) return;

    const float skala = band_skalierung(g.prefs.stufe);
    // Texturweg nur mit fertigem Atlas; bis band_frame ihn (neu) gebacken hat,
    // gilt der Atlas der vorigen Stufe weiter (Größe folgt einen Frame später).
    const bool texturiert = g.weg == Weg::TEXTURIERT && g.textur_bereit;
    if (texturiert) {
        band_ordne(*bild, g_atlas, g_atlas.skala(), &g_layout);
        passe_fenster_an(static_cast<int>(std::ceil(g_layout.breite)), static_cast<int>(std::ceil(g_layout.hoehe)));
    } else {
        band_ordne(*bild, g_xplm_mass, 1.0f, &g_layout);
        passe_fenster_an(static_cast<int>(std::ceil(g_layout.breite * skala)),
                         static_cast<int>(std::ceil(g_layout.hoehe * skala)));
    }

    int l = 0, o = 0, r = 0, u = 0;
    XPLMGetWindowGeometry(g.fenster, &l, &o, &r, &u);
    int mx = 0, my = 0;
    XPLMGetMouseLocationGlobal(&mx, &my);
    const bool maus_drauf = mx >= l && mx < r && my > u && my <= o;
    const bool hell = !bild->ruhig || maus_drauf || g.ziehen;

    if (texturiert) {
        pruefe_zeichenweg(static_cast<float>(l + r) / 2, static_cast<float>(o + u) / 2);
    }
    if (g.weg == Weg::TEXTURIERT && g.textur_bereit) {
        zeichne_texturiert(*bild, static_cast<float>(l), static_cast<float>(u),
                           hell ? 1.0f : bandmass::RUHIG_DECKKRAFT);
    } else {
        if (texturiert) {   // eben erst zurückgefallen: Maße neu
            band_ordne(*bild, g_xplm_mass, 1.0f, &g_layout);
            passe_fenster_an(static_cast<int>(std::ceil(g_layout.breite * skala)),
                             static_cast<int>(std::ceil(g_layout.hoehe * skala)));
        }
        RueckfallAuftrag a{bild, band_dimm_faktor(bild->ruhig, maus_drauf || g.ziehen)};
        zeichne_skaliert(static_cast<float>(l), static_cast<float>(u), skala, &zeichne_rueckfall_inhalt, &a);
    }
}

int auf_klick(XPLMWindowID, int x, int y, XPLMMouseStatus status, void*) noexcept {
    switch (status) {
        case xplm_MouseDown:
            g.ziehen = true;
            g.bewegt = false;
            g.maus_x = x;
            g.maus_y = y;
            break;
        case xplm_MouseDrag:
            if (g.ziehen) {
                const int dx = x - g.maus_x;
                const int dy = y - g.maus_y;
                g.maus_x = x;
                g.maus_y = y;
                if (dx != 0 || dy != 0) {
                    int l = 0, o = 0, r = 0, u = 0;
                    XPLMGetWindowGeometry(g.fenster, &l, &o, &r, &u);
                    int nl = l + dx, no = o + dy;
                    band_klemme_position(schirm(), r - l, o - u, &nl, &no);
                    XPLMSetWindowGeometry(g.fenster, nl, no, nl + (r - l), no - (o - u));
                    g.prefs.pos_gesetzt = true;
                    g.prefs.links = nl;
                    g.prefs.oben = no;
                    g.bewegt = true;
                }
            }
            break;
        case xplm_MouseUp:
            if (g.ziehen && g.bewegt) prefs_aendern_sofort();
            g.ziehen = false;
            g.bewegt = false;
            break;
        default:
            break;
    }
    return 1;  // gefressen: kein Klick geht ins Spiel, solange das Band getroffen ist
}

int auf_rechtsklick(XPLMWindowID, int, int, XPLMMouseStatus, void*) noexcept {
    return 1;
}

int auf_rad(XPLMWindowID, int, int, int rad, int klicks, void*) noexcept {
    if (rad == 0 && klicks != 0) {
        const int neu = klicks > 0 ? band_stufe_groesser(g.prefs.stufe)
                                   : band_stufe_kleiner(g.prefs.stufe);
        if (neu != g.prefs.stufe) {
            g.prefs.stufe = neu;
            g.prefs_offen = true;
            g.prefs_faellig = static_cast<double>(dienst_jetzt()) + WELLE_VERZOEGERUNG_S;
        }
    }
    return 1;
}

XPLMCursorStatus auf_cursor(XPLMWindowID, int, int, void*) noexcept {
    return xplm_CursorArrow;
}

void auf_taste(XPLMWindowID, char, XPLMKeyFlags, char, void*, int) noexcept {}

// ---- Menü ------------------------------------------------------------------------

enum MenuAktion : intptr_t { M_AN = 0, M_GROESSER, M_KLEINER, M_ZURUECK };

void aktualisiere_haken() noexcept {
    if (g.menu != nullptr && g.punkt_an >= 0) {
        XPLMCheckMenuItem(g.menu, g.punkt_an, g.prefs.an ? xplm_Menu_Checked : xplm_Menu_Unchecked);
    }
}

void auf_menue(void*, void* eintrag) noexcept {
    switch (reinterpret_cast<intptr_t>(eintrag)) {
        case M_AN:      g.prefs.an = !g.prefs.an; break;
        case M_GROESSER: g.prefs.stufe = band_stufe_groesser(g.prefs.stufe); break;
        case M_KLEINER:  g.prefs.stufe = band_stufe_kleiner(g.prefs.stufe); break;
        case M_ZURUECK: {
            const bool an = g.prefs.an;
            g.prefs = BandPrefs{};
            g.prefs.an = an;
            g.platziert = false;  // Standardposition neu bestimmen
            break;
        }
        default: return;
    }
    aktualisiere_haken();
    prefs_aendern_sofort();
}

}  // namespace

// =============================================================================
// Öffentliche Funktionen
// =============================================================================

bool band_start() noexcept {
    if (g.fenster != nullptr) return true;
    g = Zustand{};
    // Ohne native Pfade liefert XPLMGetPrefsPath auf dem Mac das alte
    // HFS-Format ("Macintosh HD:Users:...") - fopen scheitert, die Prefs
    // wurden nie geschrieben (Feldtest 04.10.2026). Betrifft nur dieses
    // Plugin; der Flugzeugpfad kommt aus einem Dataref, nicht aus der API.
    if (XPLMHasFeature("XPLM_USE_NATIVE_PATHS")) XPLMEnableFeature("XPLM_USE_NATIVE_PATHS", 1);
    ermittle_prefs_pfad();
    {
        char z[700];
        std::snprintf(z, sizeof(z), "Prefs-Datei: %s", g.prefs_pfad);
        log_zeile(z);
    }
    lese_prefs();

    XPLMCreateWindow_t p;
    std::memset(&p, 0, sizeof(p));
    p.structSize = static_cast<int>(sizeof(p));
    p.left = 100;
    p.top = 140;
    p.right = 500;
    p.bottom = 100;
    p.visible = 0;  // band_frame zeigt es, sobald es etwas zu zeigen gibt
    p.drawWindowFunc = reinterpret_cast<XPLMDrawWindow_f>(&auf_zeichnen);
    p.handleMouseClickFunc = reinterpret_cast<XPLMHandleMouseClick_f>(&auf_klick);
    p.handleKeyFunc = reinterpret_cast<XPLMHandleKey_f>(&auf_taste);
    p.handleCursorFunc = reinterpret_cast<XPLMHandleCursor_f>(&auf_cursor);
    p.handleMouseWheelFunc = reinterpret_cast<XPLMHandleMouseWheel_f>(&auf_rad);
    p.handleRightClickFunc = reinterpret_cast<XPLMHandleMouseClick_f>(&auf_rechtsklick);
    p.refcon = nullptr;
    p.decorateAsFloatingWindow = xplm_WindowDecorationNone;
    p.layer = xplm_WindowLayerFloatingWindows;
    g.fenster = XPLMCreateWindowEx(&p);
    if (g.fenster == nullptr) {
        log_zeile("Fenster nicht anlegbar - Band aus");
        return false;
    }

    // Menü "Plugins → AeroACARS". Scheitert es, läuft das Band trotzdem.
    XPLMMenuID plugins = XPLMFindPluginsMenu();
    if (plugins != nullptr) {
        g.menu_eintrag = XPLMAppendMenuItem(plugins, "AeroACARS", nullptr, 0);
        if (g.menu_eintrag >= 0) {
            g.menu = XPLMCreateMenu("AeroACARS", plugins, g.menu_eintrag, &auf_menue, nullptr);
        }
        if (g.menu != nullptr) {
            g.punkt_an = XPLMAppendMenuItem(g.menu, "Band ein/aus", reinterpret_cast<void*>(M_AN), 0);
            XPLMAppendMenuItem(g.menu, "Band groesser", reinterpret_cast<void*>(M_GROESSER), 0);
            XPLMAppendMenuItem(g.menu, "Band kleiner", reinterpret_cast<void*>(M_KLEINER), 0);
            XPLMAppendMenuItem(g.menu, "Band zuruecksetzen", reinterpret_cast<void*>(M_ZURUECK), 0);
            aktualisiere_haken();
        } else {
            log_zeile("Menue nicht anlegbar - Band laeuft ohne Menue");
        }
    }
    log_zeile("bereit");
    return true;
}

void band_stopp() noexcept {
    if (g.fenster == nullptr && g.menu == nullptr) return;
    if (g.prefs_offen) schreibe_prefs();
    if (g.fenster != nullptr) XPLMDestroyWindow(g.fenster);
    g.fenster = nullptr;
    if (g.menu != nullptr) XPLMDestroyMenu(g.menu);
    g.menu = nullptr;
    if (g.menu_eintrag >= 0) {
        XPLMMenuID plugins = XPLMFindPluginsMenu();
        if (plugins != nullptr) XPLMRemoveMenuItem(plugins, g.menu_eintrag);
    }
    g.menu_eintrag = -1;
    g.punkt_an = -1;
    if (g.textur != 0) {
        GLuint t = static_cast<GLuint>(g.textur);
        glDeleteTextures(1, &t);
    }
    g.textur = 0;
    g.textur_bereit = false;
    g_atlas.pixel_freigeben();
}

void band_frame() noexcept {
    if (g.fenster == nullptr) return;
    const double jetzt = dienst_jetzt();

    Band* band = dienst_band();
    const BandBild* bild = band != nullptr ? band->ansicht(jetzt) : nullptr;
    const bool soll = g.prefs.an && bild != nullptr && band_hat_inhalt(*bild);
    // Atlas/Textur erst, wenn es etwas zu zeigen gibt — und hier, nicht im Draw.
    if (soll && g.weg == Weg::TEXTURIERT) bereite_textur();
    if (soll) halte_auf_schirm();  // vor dem Zeigen: sonst ruft X-Plane auf_zeichnen nie auf
    if (soll != g.sichtbar || soll != (XPLMGetWindowIsVisible(g.fenster) != 0)) {
        XPLMSetWindowIsVisible(g.fenster, soll ? 1 : 0);
        g.sichtbar = soll;
        if (!soll) g.ziehen = false;
    }

    if (g.prefs_offen && g.prefs_faellig > 0.0 && jetzt >= g.prefs_faellig) schreibe_prefs();
}
