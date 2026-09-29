// Einheitstests: Paketaufteilung (pakete.cpp)

#include "json_pruefer.h"
#include "pakete.h"
#include "testrahmen.h"

#include <cstdio>
#include <cstring>
#include <random>
#include <string>
#include <vector>

using namespace aeroacars;

namespace {

const std::string PRAEFIX = "{\"p\":2,\"t\":\"w\",\"abo\":1,\"seq\":812";

bool fuelle(ElementListe& l, const std::vector<std::string>& elemente) {
    size_t bytes = 0;
    for (const auto& e : elemente) bytes += e.size();
    if (!l.reserviere(bytes + 1, elemente.size() + 1)) return false;
    for (const auto& e : elemente) {
        JsonSchreiber w = l.schreiber();
        w.roh(e.data(), e.size());
        if (!l.uebernehme(w)) return false;
    }
    return true;
}

struct Aufteilung {
    uint32_t teile = 0;
    std::vector<std::string> pakete;
};

Aufteilung teile_auf(const std::vector<std::string>& elemente, size_t max = 8192,
                     const std::string& praefix = PRAEFIX, const char* feld = "v") {
    ElementListe l;
    Aufteilung a;
    if (!fuelle(l, elemente)) return a;
    std::vector<uint32_t> grenzen(elemente.size() + 2);
    a.teile = plane_pakete(praefix.size(), std::strlen(feld), l, grenzen.data(), grenzen.size(), max);
    std::vector<char> puffer(max + 16);
    for (uint32_t k = 0; k < a.teile; ++k) {
        const size_t n = schreibe_paket(puffer.data(), max, praefix.data(), praefix.size(), feld,
                                        l, grenzen.data(), k, a.teile);
        a.pakete.emplace_back(puffer.data(), n);
    }
    return a;
}

// Prüft alle Garantien einer Aufteilung.
void pruefe_aufteilung(const std::vector<std::string>& elemente, size_t max) {
    const Aufteilung a = teile_auf(elemente, max);
    PRUEFE(a.teile >= 1);
    std::vector<std::string> wieder;
    for (uint32_t k = 0; k < a.teile; ++k) {
        const std::string& p = a.pakete[k];
        PRUEFE(!p.empty());
        PRUEFE(p.size() <= max);
        JWert j;
        std::string grund;
        const bool ok = pruefe_paket(p, &j, &grund);
        PRUEFE(ok);
        if (!ok) {
            std::printf("     %s\n", grund.c_str());
            return;
        }
        PRUEFE(j.hole("teil")->zahl == k + 1);
        PRUEFE(j.hole("teile")->zahl == a.teile);
        const JWert* v = j.hole("v");
        PRUEFE(v && v->ist_feld());
        for (const JWert& e : v->feld) wieder.push_back(e.text);
        // Gierig: das erste Element des nächsten Pakets hätte nicht mehr gepasst.
        if (k + 1 < a.teile) {
            const size_t naechstes = elemente[wieder.size()].size();
            PRUEFE(p.size() + 1 + naechstes > max);
        }
    }
    PRUEFE_GLEICH(wieder.size(), elemente.size());
    for (size_t i = 0; i < elemente.size() && i < wieder.size(); ++i) {
        PRUEFE_TEXT("\"" + wieder[i] + "\"", elemente[i]);
    }
}

std::string element(size_t laenge, char c = 'x') {
    // Ein JSON-Text genau `laenge` Zeichen lang (mit Anführungszeichen).
    return "\"" + std::string(laenge >= 2 ? laenge - 2 : 0, c) + "\"";
}

}  // namespace

TEST(pakete_leere_liste) {
    const Aufteilung a = teile_auf({});
    PRUEFE_GLEICH(a.teile, 1u);
    PRUEFE_TEXT(a.pakete[0], PRAEFIX + ",\"teil\":1,\"teile\":1,\"v\":[]}\n");
}

TEST(pakete_einfach) {
    const Aufteilung a = teile_auf({"[0,51.2345678]", "[2,8.5]", "[3,\"A20N\"]"});
    PRUEFE_GLEICH(a.teile, 1u);
    PRUEFE_TEXT(a.pakete[0], PRAEFIX +
                                 ",\"teil\":1,\"teile\":1,\"v\":[[0,51.2345678],[2,8.5],[3,\"A20N\"]]}\n");
    const Aufteilung st = teile_auf({"[0,\"f\",1]", "[1,\"fehlt\"]"}, 8192,
                                    "{\"p\":2,\"t\":\"abo\",\"abo\":1", "st");
    PRUEFE_TEXT(st.pakete[0],
                "{\"p\":2,\"t\":\"abo\",\"abo\":1,\"teil\":1,\"teile\":1,\"st\":[[0,\"f\",1],[1,\"fehlt\"]]}\n");
}

TEST(pakete_genau_an_der_grenze) {
    const size_t rahmen = paket_laenge(PRAEFIX.size(), 1, 1, 1, 0, 0);
    // Ein Element, das das Paket auf genau 8192 Byte bringt.
    const size_t e = 8192 - rahmen;
    const Aufteilung a = teile_auf({element(e)});
    PRUEFE_GLEICH(a.teile, 1u);
    PRUEFE_GLEICH(a.pakete[0].size(), size_t(8192));
    // Ein Byte mehr passt in kein Paket: Planung scheitert, statt zu schneiden.
    PRUEFE_GLEICH(teile_auf({element(e + 1)}).teile, 0u);

    // Zwei Elemente, zusammen genau an der Grenze (ein Komma dazwischen).
    const size_t e1 = 3000;
    const size_t e2 = 8192 - rahmen - 1 - e1;
    const Aufteilung zwei = teile_auf({element(e1), element(e2)});
    PRUEFE_GLEICH(zwei.teile, 1u);
    PRUEFE_GLEICH(zwei.pakete[0].size(), size_t(8192));
    // Ein Byte mehr: zwei Pakete, jedes Element ganz.
    const Aufteilung zwei_plus = teile_auf({element(e1), element(e2 + 1)});
    PRUEFE_GLEICH(zwei_plus.teile, 2u);
    pruefe_aufteilung({element(e1), element(e2 + 1)}, 8192);
}

TEST(pakete_viele_werte) {
    std::vector<std::string> el;
    for (int i = 0; i < 8192; ++i) el.push_back("\"[" + std::to_string(i) + ",0.123456789]\"");
    pruefe_aufteilung(el, 8192);
    const Aufteilung a = teile_auf(el);
    PRUEFE(a.teile > 10);
}

TEST(pakete_stellenwechsel_der_teilezahl) {
    // Kleine Pakete, damit die Teilezahl von 1 auf 2 und 3 Stellen springt:
    // der Kopf wächst dann um je ein Zeichen, die Planung muss das merken.
    for (size_t n : {9u, 10u, 11u, 99u, 100u, 101u, 250u}) {
        std::vector<std::string> el;
        for (size_t i = 0; i < n; ++i) el.push_back(element(20 + (i % 7)));
        pruefe_aufteilung(el, PRAEFIX.size() + 60);
    }
}

TEST(pakete_zufall) {
    std::mt19937 rng(20260929);
    for (int runde = 0; runde < 300; ++runde) {
        const size_t max = 80 + rng() % 400;
        const size_t anzahl = rng() % 300;
        std::vector<std::string> el;
        const size_t rahmen = paket_laenge(PRAEFIX.size(), 1, 999, 999, 0, 0);
        for (size_t i = 0; i < anzahl; ++i) el.push_back(element(2 + rng() % (max - rahmen - 1)));
        pruefe_aufteilung(el, max);
    }
}

TEST(pakete_zu_kleiner_grenzenpuffer) {
    ElementListe l;
    fuelle(l, {"1", "2", "3"});
    uint32_t grenzen[3];
    PRUEFE_GLEICH(plane_pakete(10, 1, l, grenzen, 3, 8192), 0u);  // braucht anzahl + 1 = 4
    PRUEFE_GLEICH(plane_pakete(10, 1, l, nullptr, 0, 8192), 0u);
    uint32_t genug[4];
    PRUEFE_GLEICH(plane_pakete(10, 1, l, genug, 4, 8192), 1u);
}

TEST(elementliste_ueberlauf) {
    ElementListe l;
    PRUEFE(l.reserviere(8, 2));
    JsonSchreiber w = l.schreiber();
    w.roh("123456789");  // 9 > 8
    PRUEFE(!l.uebernehme(w));
    PRUEFE_GLEICH(l.anzahl(), size_t(0));
    JsonSchreiber a = l.schreiber();
    a.roh("1234");
    PRUEFE(l.uebernehme(a));
    JsonSchreiber b = l.schreiber();
    b.roh("5678");
    PRUEFE(l.uebernehme(b));
    JsonSchreiber c = l.schreiber();  // Bytes voll, Einträge voll
    PRUEFE(!l.uebernehme(c));
    PRUEFE_GLEICH(l.anzahl(), size_t(2));
    PRUEFE_TEXT(std::string(l.element(1), l.element_laenge(1)), "5678");
    // Wachsen für LISTE.
    PRUEFE(l.sorge_fuer_platz(10000));
    JsonSchreiber d = l.schreiber();
    d.roh(std::string(10000, 'z').c_str());
    PRUEFE(l.uebernehme(d));
    PRUEFE_GLEICH(l.element_laenge(2), size_t(10000));
}
