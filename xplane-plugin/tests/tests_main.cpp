// Einstieg der Einheitstests: führt alle registrierten TESTs aus.

#include "testrahmen.h"

#include <cstdio>

namespace testrahmen {
Test*& erster_test() {
    static Test* t = nullptr;
    return t;
}
int& fehlerzahl() {
    static int n = 0;
    return n;
}
int& pruefzahl() {
    static int n = 0;
    return n;
}
}  // namespace testrahmen

int main() {
    // Registrierung stellt voran — umdrehen, damit die Reihenfolge der Datei
    // entspricht (lesbarere Ausgabe).
    testrahmen::Test* umgedreht = nullptr;
    for (testrahmen::Test* t = testrahmen::erster_test(); t != nullptr;) {
        testrahmen::Test* n = t->naechster;
        t->naechster = umgedreht;
        umgedreht = t;
        t = n;
    }
    int tests = 0;
    int rote = 0;
    for (testrahmen::Test* t = umgedreht; t != nullptr; t = t->naechster) {
        const int vorher = testrahmen::fehlerzahl();
        t->fn();
        ++tests;
        const bool rot = testrahmen::fehlerzahl() != vorher;
        if (rot) ++rote;
        std::printf("%s %s\n", rot ? "ROT " : "ok  ", t->name);
    }
    std::printf("\n%d Tests, %d Pruefungen, %d rot, %d Fehler\n", tests,
                testrahmen::pruefzahl(), rote, testrahmen::fehlerzahl());
    return testrahmen::fehlerzahl() == 0 ? 0 : 1;
}
