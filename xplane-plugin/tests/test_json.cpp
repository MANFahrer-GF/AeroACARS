// Einheitstests: JSON-Schreiber (json_schreiber.cpp)

#include "json_pruefer.h"
#include "json_schreiber.h"
#include "testrahmen.h"

#include <cfloat>
#include <clocale>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <random>
#include <string>

using namespace aeroacars;

namespace {

std::string als_text(const std::string& roh) {
    char puffer[8192];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.text(roh.data(), roh.size());
    return w.ueberlauf() ? std::string("<ueberlauf>") : std::string(puffer, w.laenge());
}

std::string als_d(double v) {
    char puffer[64];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.zahl_d(v);
    return std::string(puffer, w.laenge());
}

std::string als_f(float v) {
    char puffer[64];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.zahl_f(v);
    return std::string(puffer, w.laenge());
}

// Zurückgelesener Text muss dem erwarteten entsprechen, und das Ergebnis
// muss gültiges JSON + UTF-8 sein.
std::string zurueck(const std::string& json) {
    JWert v;
    std::string grund;
    if (!json_lies(json, &v, &grund) || !v.ist_text()) return "<ungueltig: " + grund + ">";
    return v.text;
}

}  // namespace

TEST(json_maskierung) {
    PRUEFE_TEXT(als_text(""), "\"\"");
    PRUEFE_TEXT(als_text("A20N"), "\"A20N\"");
    PRUEFE_TEXT(als_text("a\"b"), "\"a\\\"b\"");
    PRUEFE_TEXT(als_text("a\\b"), "\"a\\\\b\"");
    PRUEFE_TEXT(als_text("\n\r\t\b\f"), "\"\\n\\r\\t\\b\\f\"");
    PRUEFE_TEXT(als_text(std::string("\x01\x1f\x7f", 3)), "\"\\u0001\\u001f\\u007f\"");
    PRUEFE_TEXT(als_text(std::string("a\0b", 3)), "\"a\\u0000b\"");
    PRUEFE_TEXT(als_text("/"), "\"/\"");
    PRUEFE_TEXT(zurueck(als_text("x\"y\\z\n")), "x\"y\\z\n");
}

TEST(json_utf8) {
    // Gültige Folgen gehen unverändert durch.
    PRUEFE_TEXT(als_text("Caf\xC3\xA9"), "\"Caf\xC3\xA9\"");
    PRUEFE_TEXT(als_text("\xE2\x82\xAC"), "\"\xE2\x82\xAC\"");              // €
    PRUEFE_TEXT(als_text("\xF0\x9F\x98\x80"), "\"\xF0\x9F\x98\x80\"");      // 😀
    PRUEFE_TEXT(als_text("\xEF\xBF\xBF"), "\"\xEF\xBF\xBF\"");              // U+FFFF
    PRUEFE_TEXT(als_text("\xF4\x8F\xBF\xBF"), "\"\xF4\x8F\xBF\xBF\"");      // U+10FFFF
    // Latin-1 und kaputte Folgen werden Byte für Byte als \u00XX geschrieben.
    PRUEFE_TEXT(als_text("Caf\xE9"), "\"Caf\\u00e9\"");
    PRUEFE_TEXT(als_text("\xC0\x80"), "\"\\u00c0\\u0080\"");                // Überlänge
    PRUEFE_TEXT(als_text("\xE0\x80\x80"), "\"\\u00e0\\u0080\\u0080\"");     // Überlänge
    PRUEFE_TEXT(als_text("\xED\xA0\x80"), "\"\\u00ed\\u00a0\\u0080\"");     // Surrogat
    PRUEFE_TEXT(als_text("\xF4\x90\x80\x80"), "\"\\u00f4\\u0090\\u0080\\u0080\"");  // > U+10FFFF
    PRUEFE_TEXT(als_text("\xE2\x82"), "\"\\u00e2\\u0082\"");                // abgeschnitten
    PRUEFE_TEXT(als_text("\xF5"), "\"\\u00f5\"");
    PRUEFE_TEXT(als_text("\x80x"), "\"\\u0080x\"");
    // Latin-1 é kommt beim Client als é an.
    PRUEFE_TEXT(zurueck(als_text("Caf\xE9")), "Caf\xC3\xA9");
}

TEST(json_zufallstexte_bleiben_gueltig) {
    std::mt19937 rng(4711);
    for (int runde = 0; runde < 3000; ++runde) {
        std::string roh(rng() % 64, '\0');
        for (char& c : roh) c = static_cast<char>(rng() & 0xFF);
        const std::string j = als_text(roh);
        JWert v;
        std::string grund;
        const bool ok = json_lies(j, &v, &grund) && v.ist_text() && ist_utf8(j);
        PRUEFE(ok);
        if (!ok) break;
        // War die Eingabe schon gültiges UTF-8, muss sie exakt zurückkommen.
        if (ist_utf8(roh)) PRUEFE(v.text == roh);
    }
}

TEST(json_text_bis_nul) {
    char puffer[64];
    JsonSchreiber w(puffer, sizeof(puffer));
    const char quelle[8] = {'B', '7', '3', '8', '\0', 'x', 'y', 'z'};
    w.text_bis_nul(quelle, sizeof(quelle));
    PRUEFE_TEXT(std::string(puffer, w.laenge()), "\"B738\"");
    JsonSchreiber w2(puffer, sizeof(puffer));
    w2.text_bis_nul("ABCDEFGH", 3);  // ohne NUL: an der Grenze Schluss
    PRUEFE_TEXT(std::string(puffer, w2.laenge()), "\"ABC\"");
    JsonSchreiber w3(puffer, sizeof(puffer));
    w3.text_bis_nul(nullptr, 5);
    PRUEFE_TEXT(std::string(puffer, w3.laenge()), "\"\"");
}

TEST(json_double_volle_genauigkeit) {
    const double werte[] = {
        0.0, 1.0, -1.0, 8.5, 51.2345678, 8.571234567890123, -122.3456789012345,
        0.1, 1.0 / 3.0, 1e-300, 1e300, DBL_MAX, -DBL_MAX, DBL_MIN,
        std::numeric_limits<double>::denorm_min(), 12345678901234567.0,
    };
    for (double v : werte) {
        const std::string s = als_d(v);
        JWert j;
        std::string grund;
        PRUEFE(json_lies(s, &j, &grund) && j.ist_zahl());
        PRUEFE(j.zahl == v);  // exakt, Bit für Bit
    }
    PRUEFE_TEXT(als_d(8.5), "8.5");
    PRUEFE_TEXT(als_d(0.0), "0");
    PRUEFE_TEXT(als_d(-0.0), "-0");
    PRUEFE_TEXT(als_d(std::nan("")), "null");
    PRUEFE_TEXT(als_d(HUGE_VAL), "null");
    PRUEFE_TEXT(als_d(-HUGE_VAL), "null");
    PRUEFE(als_d(-DBL_MAX).size() <= JSON_MAX_DOUBLE);
    PRUEFE(als_d(-2.2250738585072014e-308).size() <= JSON_MAX_DOUBLE);
}

TEST(json_float_volle_genauigkeit) {
    std::mt19937 rng(99);
    for (int i = 0; i < 20000; ++i) {
        uint32_t bits = rng();
        float f;
        std::memcpy(&f, &bits, sizeof(f));
        const std::string s = als_f(f);
        if (!std::isfinite(f)) {
            PRUEFE_TEXT(s, "null");
            continue;
        }
        PRUEFE(s.size() <= JSON_MAX_FLOAT);
        JWert j;
        std::string grund;
        const bool ok = json_lies(s, &j, &grund) && j.ist_zahl() &&
                        static_cast<float>(j.zahl) == f;
        PRUEFE(ok);
        if (!ok) break;
    }
    PRUEFE_TEXT(als_f(0.5f), "0.5");
    PRUEFE_TEXT(als_f(-FLT_MAX), "-3.40282347e+38");
}

TEST(json_ganzzahl) {
    auto g = [](int64_t v) {
        char p[32];
        JsonSchreiber w(p, sizeof(p));
        w.ganzzahl(v);
        return std::string(p, w.laenge());
    };
    PRUEFE_TEXT(g(0), "0");
    PRUEFE_TEXT(g(-1), "-1");
    PRUEFE_TEXT(g(2147483647), "2147483647");
    PRUEFE_TEXT(g(-2147483647 - 1), "-2147483648");
    PRUEFE_TEXT(g(INT64_MAX), "9223372036854775807");
    PRUEFE_TEXT(g(INT64_MIN), "-9223372036854775808");
}

TEST(json_zahlen_ohne_locale_komma) {
    // Stellt ein anderes Plugin LC_NUMERIC auf Deutsch, schreibt printf "1,5".
    // Der Schreiber muss trotzdem "1.5" liefern.
    const char* kandidaten[] = {"de_DE.UTF-8", "de_DE.utf8", "de_DE", "German_Germany.1252",
                                "fr_FR.UTF-8", "ru_RU.UTF-8"};
    const char* alt = std::setlocale(LC_NUMERIC, nullptr);
    std::string alt_kopie = alt ? alt : "C";
    bool umgestellt = false;
    for (const char* k : kandidaten) {
        if (std::setlocale(LC_NUMERIC, k) != nullptr) { umgestellt = true; break; }
    }
    char puffer[64];
    std::snprintf(puffer, sizeof(puffer), "%.1f", 1.5);
    const bool komma = std::string(puffer) == "1,5";
    const std::string d = als_d(1.5);
    const std::string f = als_f(-2.25f);
    const std::string e = als_d(1.5e-300);
    std::setlocale(LC_NUMERIC, alt_kopie.c_str());
    PRUEFE_TEXT(d, "1.5");
    PRUEFE_TEXT(f, "-2.25");
    JWert j;
    std::string grund;
    PRUEFE(json_lies(e, &j, &grund) && j.zahl == 1.5e-300);
    if (!umgestellt || !komma) {
        std::printf("     (Hinweis: keine Komma-Locale verfuegbar - nur ohne Umstellung geprueft)\n");
    }
}

TEST(json_ueberlauf_schreibt_nie_ueber_die_grenze) {
    for (size_t kap = 0; kap < 40; ++kap) {
        char puffer[64];
        std::memset(puffer, '#', sizeof(puffer));
        JsonSchreiber w(puffer, kap);
        w.roh("{\"p\":2,");
        w.text("\xE9\"\\abc", 6);
        w.zahl_d(51.2345678);
        w.ganzzahl(-12345);
        w.null();
        PRUEFE(w.laenge() <= kap);
        for (size_t i = kap; i < sizeof(puffer); ++i) PRUEFE(puffer[i] == '#');
        if (kap < 20) PRUEFE(w.ueberlauf());
    }
    // Marke: angefangenen Baustein zurücknehmen, danach geht es weiter.
    char p[16];
    JsonSchreiber w(p, sizeof(p));
    w.roh("[1,");
    const size_t m = w.marke();
    w.text("dieser text passt nicht mehr", 28);
    PRUEFE(w.ueberlauf());
    w.zurueck_zu(m);
    PRUEFE(!w.ueberlauf());
    w.roh("2]");
    PRUEFE_TEXT(std::string(p, w.laenge()), "[1,2]");
    // Null-Puffer ist harmlos.
    JsonSchreiber leer(nullptr, 100);
    leer.roh("x");
    PRUEFE(leer.ueberlauf());
    PRUEFE_GLEICH(leer.laenge(), size_t(0));
}

TEST(json_utf8_folge) {
    auto f = [](const char* s, size_t n) {
        return utf8_folge(reinterpret_cast<const unsigned char*>(s), n);
    };
    PRUEFE_GLEICH(f("a", 1), size_t(1));
    PRUEFE_GLEICH(f("\xC3\xA9", 2), size_t(2));
    PRUEFE_GLEICH(f("\xC3\xA9", 1), size_t(0));
    PRUEFE_GLEICH(f("\xC1\xBF", 2), size_t(0));
    PRUEFE_GLEICH(f("\xE2\x82\xAC", 3), size_t(3));
    PRUEFE_GLEICH(f("\xED\x9F\xBF", 3), size_t(3));  // U+D7FF
    PRUEFE_GLEICH(f("\xED\xA0\x80", 3), size_t(0));  // U+D800
    PRUEFE_GLEICH(f("\xF0\x90\x80\x80", 4), size_t(4));
    PRUEFE_GLEICH(f("\xF0\x8F\xBF\xBF", 4), size_t(0));
    PRUEFE_GLEICH(f("\xF4\x90\x80\x80", 4), size_t(0));
    PRUEFE_GLEICH(f("", 0), size_t(0));
}
