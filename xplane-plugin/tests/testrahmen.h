// =============================================================================
// Minimaler Testrahmen (ohne Fremdbibliothek, ohne Ausnahmen)
// =============================================================================
//
//   TEST(name) { PRUEFE(bedingung); PRUEFE_GLEICH(a, b); PRUEFE_TEXT(s, "x"); }
//
// Jeder TEST registriert sich selbst; tests_main.cpp führt alle aus und endet
// mit Code 1, sobald eine Prüfung fehlschlug.
// =============================================================================

#pragma once

#include <cstdio>
#include <cstring>
#include <string>

namespace testrahmen {

struct Test {
    const char* name;
    void (*fn)();
    Test* naechster;
};

Test*& erster_test();
int& fehlerzahl();
int& pruefzahl();

struct Registrierung {
    Test t;
    Registrierung(const char* name, void (*fn)()) : t{name, fn, nullptr} {
        t.naechster = erster_test();
        erster_test() = &t;
    }
};

inline void melde(const char* datei, int zeile, const std::string& text) {
    ++fehlerzahl();
    std::fprintf(stderr, "  FEHLER %s:%d: %s\n", datei, zeile, text.c_str());
}

}  // namespace testrahmen

#define TR_VERBINDE2(a, b) a##b
#define TR_VERBINDE(a, b) TR_VERBINDE2(a, b)

#define TEST(name)                                                              \
    static void name();                                                         \
    static testrahmen::Registrierung TR_VERBINDE(reg_, name)(#name, &name);     \
    static void name()

#define PRUEFE(bed)                                                             \
    do {                                                                        \
        ++testrahmen::pruefzahl();                                              \
        if (!(bed)) testrahmen::melde(__FILE__, __LINE__, "PRUEFE(" #bed ")");  \
    } while (0)

#define PRUEFE_GLEICH(a, b)                                                     \
    do {                                                                        \
        ++testrahmen::pruefzahl();                                              \
        const auto tr_a = (a);                                                  \
        const auto tr_b = (b);                                                  \
        if (!(tr_a == tr_b)) {                                                  \
            testrahmen::melde(__FILE__, __LINE__,                               \
                              std::string("PRUEFE_GLEICH(" #a ", " #b "): ") +  \
                                  std::to_string(tr_a) + " != " +               \
                                  std::to_string(tr_b));                        \
        }                                                                       \
    } while (0)

#define PRUEFE_TEXT(a, b)                                                       \
    do {                                                                        \
        ++testrahmen::pruefzahl();                                              \
        const std::string tr_a = (a);                                           \
        const std::string tr_b = (b);                                           \
        if (tr_a != tr_b) {                                                     \
            testrahmen::melde(__FILE__, __LINE__,                               \
                              std::string("PRUEFE_TEXT(" #a "):\n    ist:  ") + \
                                  tr_a + "\n    soll: " + tr_b);                \
        }                                                                       \
    } while (0)
