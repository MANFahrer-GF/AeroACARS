// Einheitstests: Anfrage-Parser (anfrage.cpp)

#include "anfrage.h"
#include "grenzen.h"
#include "testrahmen.h"

#include <cstdlib>
#include <cstring>
#include <set>
#include <string>
#include <vector>

using namespace aeroacars;

namespace {

struct Ergebnis {
    bool ok = false;
    Anfrage a;
    std::vector<std::string> namen;
    std::vector<int32_t> index;
    std::string version;
};

// Das Datagramm liegt in einem Heap-Block GENAU seiner Länge: jedes Lesen
// über das Ende hinaus fällt unter AddressSanitizer sofort auf.
Ergebnis zerlege(const std::string& s, size_t kapazitaet = grenzen::MAX_NAMEN_JE_ABO) {
    Ergebnis e;
    std::vector<NameRef> puffer(kapazitaet > 0 ? kapazitaet : 1);
    char* kopie = static_cast<char*>(std::malloc(s.size() > 0 ? s.size() : 1));
    if (!s.empty()) std::memcpy(kopie, s.data(), s.size());
    e.ok = zerlege_anfrage(kopie, s.size(), puffer.data(), kapazitaet, &e.a);
    for (size_t i = 0; i < e.a.namen_anzahl; ++i) {
        e.namen.emplace_back(e.a.namen[i].basis, e.a.namen[i].basis_laenge);
        e.index.push_back(e.a.namen[i].index);
    }
    if (e.a.client_version) e.version.assign(e.a.client_version, e.a.client_version_laenge);
    std::free(kopie);
    e.a.namen = nullptr;
    e.a.client_version = nullptr;
    return e;
}

std::string grund(const Ergebnis& e) { return fehlergrund_text(e.a.fehler); }

void pruefe_fehler(const std::string& s, Fehlergrund g, uint32_t zeile, const char* wo) {
    const Ergebnis e = zerlege(s);
    if (e.ok || e.a.fehler != g || e.a.fehler_zeile != zeile ||
        e.a.befehl != Befehl::KEINER || e.a.namen_anzahl != 0) {
        testrahmen::melde(__FILE__, __LINE__,
                          std::string(wo) + ": erwartet " + fehlergrund_text(g) + "/Zeile " +
                              std::to_string(zeile) + ", ist " + grund(e) + "/Zeile " +
                              std::to_string(e.a.fehler_zeile) + (e.ok ? " (ok!)" : ""));
    }
    ++testrahmen::pruefzahl();
}

#define FEHLER(s, g, z) pruefe_fehler((s), Fehlergrund::g, (z), #s)

}  // namespace

TEST(leere_datagramme) {
    FEHLER(std::string(), LEERE_ANFRAGE, 0);
    FEHLER("\n", LEERE_ANFRAGE, 1);
    FEHLER("\n\n\n", LEERE_ANFRAGE, 1);
    FEHLER("\r\n", LEERE_ANFRAGE, 1);
    FEHLER("\r", LEERE_ANFRAGE, 1);
    FEHLER("\nPING", UNBEKANNTER_BEFEHL, 1);
}

TEST(ping) {
    PRUEFE(zerlege("PING").ok);
    PRUEFE(zerlege("PING\n").ok);
    PRUEFE(zerlege("PING\r\n").ok);
    PRUEFE(zerlege("PING").a.befehl == Befehl::PING);
    FEHLER("PING\n\n", UEBERZAEHLIGE_ZEILEN, 2);
    FEHLER("PING\nPING\n", UEBERZAEHLIGE_ZEILEN, 2);
    FEHLER("PING x", FALSCHE_ARGUMENTE, 1);
    FEHLER("PING ", FALSCHE_ARGUMENTE, 1);
    FEHLER(" PING", UNBEKANNTER_BEFEHL, 1);
    FEHLER("ping", UNBEKANNTER_BEFEHL, 1);
    FEHLER("PONG", UNBEKANNTER_BEFEHL, 1);
    FEHLER("PING\r\r\n", UNBEKANNTER_BEFEHL, 1);  // nur EIN \r wird toleriert
    FEHLER(std::string("PI\0NG", 5), UNBEKANNTER_BEFEHL, 1);
    FEHLER("P\xC3\x9CNG", UNBEKANNTER_BEFEHL, 1);
}

TEST(hallo) {
    const Ergebnis e = zerlege("HALLO 2 1.9.12\n");
    PRUEFE(e.ok);
    PRUEFE(e.a.befehl == Befehl::HALLO);
    PRUEFE_GLEICH(e.a.protokoll, 2u);
    PRUEFE_TEXT(e.version, "1.9.12");

    PRUEFE_GLEICH(zerlege("HALLO 3 x").a.protokoll, 3u);  // Version prüft der Dienst
    FEHLER("HALLO 2", FALSCHE_ARGUMENTE, 1);
    FEHLER("HALLO", FALSCHE_ARGUMENTE, 1);
    FEHLER("HALLO 2 a b", FALSCHE_ARGUMENTE, 1);
    FEHLER("HALLO 2  a", FALSCHE_ARGUMENTE, 1);
    FEHLER("HALLO x a", PROTOKOLL_UNGUELTIG, 1);
    FEHLER("HALLO 0 a", PROTOKOLL_UNGUELTIG, 1);
    FEHLER("HALLO -2 a", PROTOKOLL_UNGUELTIG, 1);
    FEHLER("HALLO 99999999999 a", PROTOKOLL_UNGUELTIG, 1);
    FEHLER("HALLO 4294967296 a", PROTOKOLL_UNGUELTIG, 1);
    PRUEFE(zerlege("HALLO 4294967295 a").ok);
    PRUEFE(zerlege("HALLO 2 " + std::string(64, 'v')).ok);
    FEHLER("HALLO 2 " + std::string(65, 'v'), FALSCHE_ARGUMENTE, 1);
    FEHLER("HALLO 2 v\xC3\xA4", FALSCHE_ARGUMENTE, 1);
    FEHLER("HALLO 2 v\tx", FALSCHE_ARGUMENTE, 1);
    FEHLER("HALLO 2 v\nPING", UEBERZAEHLIGE_ZEILEN, 2);
}

TEST(abo_einfach) {
    const Ergebnis e = zerlege("ABO 3 20\nsim/flightmodel/position/latitude\nsim/x/arr[7]\n");
    PRUEFE(e.ok);
    PRUEFE(e.a.befehl == Befehl::ABO);
    PRUEFE_GLEICH(e.a.abo_id, 3u);
    PRUEFE_GLEICH(e.a.rate_hz, 20u);
    PRUEFE_GLEICH(e.a.teil, 1u);
    PRUEFE_GLEICH(e.a.teile, 1u);
    PRUEFE(!e.a.mehrteilig);
    PRUEFE_GLEICH(e.namen.size(), size_t(2));
    PRUEFE_TEXT(e.namen[0], "sim/flightmodel/position/latitude");
    PRUEFE_GLEICH(e.index[0], -1);
    PRUEFE_TEXT(e.namen[1], "sim/x/arr");
    PRUEFE_GLEICH(e.index[1], 7);

    // Ohne abschließendes \n, mit CRLF, ohne Namen.
    PRUEFE_GLEICH(zerlege("ABO 1 1\na\nb").namen.size(), size_t(2));
    const Ergebnis crlf = zerlege("ABO 1 1\r\na\r\nb\r\n");
    PRUEFE(crlf.ok);
    PRUEFE_TEXT(crlf.namen[0], "a");
    PRUEFE_TEXT(crlf.namen[1], "b");
    const Ergebnis ohne = zerlege("ABO 1 1\n");
    PRUEFE(ohne.ok);
    PRUEFE_GLEICH(ohne.a.namen_anzahl, size_t(0));
}

TEST(abo_id_und_rate) {
    PRUEFE(zerlege("ABO 1 1\na").ok);
    PRUEFE(zerlege("ABO 16 50\na").ok);
    FEHLER("ABO 0 10\na", ABO_ID_UNGUELTIG, 1);
    FEHLER("ABO 17 10\na", ABO_ID_UNGUELTIG, 1);
    FEHLER("ABO x 10\na", ABO_ID_UNGUELTIG, 1);
    FEHLER("ABO -1 10\na", ABO_ID_UNGUELTIG, 1);
    FEHLER("ABO 1 0\na", RATE_UNGUELTIG, 1);
    FEHLER("ABO 1 51\na", RATE_UNGUELTIG, 1);
    FEHLER("ABO 1 1.5\na", RATE_UNGUELTIG, 1);
    FEHLER("ABO 1\na", FALSCHE_ARGUMENTE, 1);
    FEHLER("ABO 1 10 1\na", FALSCHE_ARGUMENTE, 1);
    FEHLER("ABO 1 10 1 2 3\na", FALSCHE_ARGUMENTE, 1);
    FEHLER("ENDE-ABO 0", ABO_ID_UNGUELTIG, 1);
    FEHLER("ENDE-ABO 17", ABO_ID_UNGUELTIG, 1);
    FEHLER("ENDE-ABO", FALSCHE_ARGUMENTE, 1);
    const Ergebnis ende = zerlege("ENDE-ABO 16\n");
    PRUEFE(ende.ok);
    PRUEFE(ende.a.befehl == Befehl::ENDE_ABO);
    PRUEFE_GLEICH(ende.a.abo_id, 16u);
}

TEST(abo_teile) {
    const Ergebnis e = zerlege("ABO 2 5 3 4\nx\n");
    PRUEFE(e.ok);
    PRUEFE(e.a.mehrteilig);
    PRUEFE_GLEICH(e.a.teil, 3u);
    PRUEFE_GLEICH(e.a.teile, 4u);
    PRUEFE(zerlege("ABO 2 5 8192 8192\nx").ok);
    FEHLER("ABO 2 5 0 4\nx", TEIL_UNGUELTIG, 1);
    FEHLER("ABO 2 5 5 4\nx", TEIL_UNGUELTIG, 1);
    FEHLER("ABO 2 5 1 0\nx", TEIL_UNGUELTIG, 1);
    FEHLER("ABO 2 5 1 8193\nx", TEIL_UNGUELTIG, 1);
    FEHLER("ABO 2 5 a 4\nx", TEIL_UNGUELTIG, 1);
}

TEST(zeilenlaengen_511_512_513) {
    const std::string n511(511, 'a'), n512(512, 'b'), n513(513, 'c');
    PRUEFE(zerlege("ABO 1 1\n" + n511 + "\n").ok);
    const Ergebnis e512 = zerlege("ABO 1 1\n" + n512 + "\n");
    PRUEFE(e512.ok);
    PRUEFE_GLEICH(e512.namen[0].size(), size_t(512));
    PRUEFE(zerlege("ABO 1 1\n" + n512 + "\r\n").ok);  // \r zählt zum Zeilenende
    PRUEFE(zerlege("ABO 1 1\n" + n512).ok);           // letzte Zeile ohne \n
    FEHLER("ABO 1 1\n" + n513 + "\n", ZEILE_ZU_LANG, 2);
    FEHLER("ABO 1 1\na\n" + n513, ZEILE_ZU_LANG, 3);
    // Index zählt zur Zeile: 508 + "[12]" = 512 ok, 509 + "[12]" = 513 zu lang.
    PRUEFE(zerlege("ABO 1 1\n" + std::string(508, 'd') + "[12]").ok);
    FEHLER("ABO 1 1\n" + std::string(509, 'd') + "[12]", ZEILE_ZU_LANG, 2);
    // Befehlszeile selbst.
    FEHLER("HALLO 2 " + std::string(505, 'v'), ZEILE_ZU_LANG, 1);
    FEHLER(std::string(513, 'P'), ZEILE_ZU_LANG, 1);
    FEHLER(std::string(512, 'P'), UNBEKANNTER_BEFEHL, 1);
}

TEST(ungueltige_namen) {
    FEHLER("ABO 1 1\nsim/\xC3\xA4", NAME_UNGUELTIG, 2);        // UTF-8 ä
    FEHLER("ABO 1 1\nsim/\xFF", NAME_UNGUELTIG, 2);            // kaputtes Byte
    FEHLER("ABO 1 1\nsim/a b", NAME_UNGUELTIG, 2);             // Leerzeichen
    FEHLER("ABO 1 1\n sim/a", NAME_UNGUELTIG, 2);
    FEHLER("ABO 1 1\nsim/a ", NAME_UNGUELTIG, 2);
    FEHLER("ABO 1 1\nsim/a\tb", NAME_UNGUELTIG, 2);
    FEHLER("ABO 1 1\nsim/a\rb", NAME_UNGUELTIG, 2);
    FEHLER("ABO 1 1\nsim/\x7F", NAME_UNGUELTIG, 2);
    FEHLER(std::string("ABO 1 1\nsim/a\0b", 15), NAME_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na\n\nb", NAME_UNGUELTIG, 3);              // leere Zeile mitten drin
    FEHLER("ABO 1 1\na\n\n", NAME_UNGUELTIG, 3);               // zweites \n = leere Zeile
    FEHLER("ABO 1 1\na\nb\xC3", NAME_UNGUELTIG, 3);
    // Anführungszeichen und Backslash sind druckbares ASCII → erlaubt.
    const Ergebnis e = zerlege("ABO 1 1\nx\"y\\z");
    PRUEFE(e.ok);
    PRUEFE_TEXT(e.namen[0], "x\"y\\z");
}

TEST(indizes) {
    auto idx = [](const std::string& name) {
        const Ergebnis e = zerlege("ABO 1 1\n" + name);
        return e.ok ? e.index[0] : -99;
    };
    auto basis = [](const std::string& name) {
        const Ergebnis e = zerlege("ABO 1 1\n" + name);
        return e.ok ? e.namen[0] : std::string("<fehler>");
    };
    PRUEFE_GLEICH(idx("a[0]"), 0);
    PRUEFE_GLEICH(idx("a[7]"), 7);
    PRUEFE_GLEICH(idx("a[007]"), 7);
    PRUEFE_GLEICH(idx("a[2147483647]"), 2147483647);
    PRUEFE_TEXT(basis("a[3]"), "a");
    PRUEFE_TEXT(basis("a[1][2]"), "a[1]");
    PRUEFE_GLEICH(idx("a[1][2]"), 2);
    PRUEFE_TEXT(basis("a[3]x"), "a[3]x");      // endet nicht auf ] → kein Index
    PRUEFE_GLEICH(idx("a[3]x"), -1);
    PRUEFE_TEXT(basis("a[b"), "a[b");
    FEHLER("ABO 1 1\na[-1]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na[+1]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na[]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na[x]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na[1x]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na[2147483648]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na[4294967295]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na[99999999999]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\na]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\n]", INDEX_UNGUELTIG, 2);
    FEHLER("ABO 1 1\n[3]", NAME_UNGUELTIG, 2);
}

TEST(namenszahl_8192_und_8193) {
    std::string d = "ABO 1 1\n";
    for (int i = 0; i < 8192; ++i) d += "n\n";
    const Ergebnis ok = zerlege(d);
    PRUEFE(ok.ok);
    PRUEFE_GLEICH(ok.a.namen_anzahl, size_t(8192));
    d += "n\n";
    FEHLER(d, ZU_VIELE_NAMEN, 8194);
    // Kleinerer Puffer des Aufrufers wird respektiert.
    const Ergebnis klein = zerlege("ABO 1 1\na\nb\nc\nd", 3);
    PRUEFE(!klein.ok);
    PRUEFE(klein.a.fehler == Fehlergrund::ZU_VIELE_NAMEN);
    PRUEFE_GLEICH(klein.a.fehler_zeile, 5u);
}

TEST(datagrammgroesse_64k) {
    // Genau 65536 Byte: gültig.
    std::string d = "ABO 1 1\n";  // 8
    for (int i = 0; i < 655; ++i) d += std::string(99, 'x') + "\n";
    d += std::string(27, 'y') + "\n";
    PRUEFE_GLEICH(d.size(), size_t(65536));
    const Ergebnis e = zerlege(d);
    PRUEFE(e.ok);
    PRUEFE_GLEICH(e.a.namen_anzahl, size_t(656));
    // Ein Byte mehr: zu groß, ohne überhaupt zu lesen.
    FEHLER(d + "z", DATAGRAMM_ZU_GROSS, 0);
}

TEST(liste) {
    const Ergebnis e = zerlege("LISTE 7\n");
    PRUEFE(e.ok);
    PRUEFE(e.a.befehl == Befehl::LISTE);
    PRUEFE_GLEICH(e.a.anfrage_id, 7u);
    PRUEFE(zerlege("LISTE 0").ok);
    PRUEFE(zerlege("LISTE 2147483647").ok);
    FEHLER("LISTE 2147483648", FALSCHE_ARGUMENTE, 1);
    FEHLER("LISTE", FALSCHE_ARGUMENTE, 1);
    FEHLER("LISTE -1", FALSCHE_ARGUMENTE, 1);
    FEHLER("LISTE 1 2", FALSCHE_ARGUMENTE, 1);
}

TEST(fehlergruende_sind_eindeutig) {
    std::set<std::string> gesehen;
    for (int g = 0; g <= static_cast<int>(Fehlergrund::SPEICHER); ++g) {
        const std::string t = fehlergrund_text(static_cast<Fehlergrund>(g));
        PRUEFE(!t.empty());
        PRUEFE(gesehen.insert(t).second);
        for (char c : t) PRUEFE((c >= 'a' && c <= 'z') || c == '_');
    }
    PRUEFE_TEXT(fehlergrund_text(Fehlergrund::LISTE_NICHT_VERFUEGBAR), "liste_nicht_verfuegbar");
    PRUEFE_TEXT(fehlergrund_text(Fehlergrund::ZEILE_ZU_LANG), "zeile_zu_lang");
}

TEST(ist_gueltiger_name) {
    PRUEFE(ist_gueltiger_name("sim/a", 5));
    PRUEFE(!ist_gueltiger_name("", 0));
    PRUEFE(!ist_gueltiger_name(nullptr, 3));
    PRUEFE(!ist_gueltiger_name("a b", 3));
    PRUEFE(!ist_gueltiger_name("\xC3\xA4", 2));
    const std::string lang(512, 'a');
    PRUEFE(ist_gueltiger_name(lang.c_str(), 512));
    const std::string zu_lang(513, 'a');
    PRUEFE(!ist_gueltiger_name(zu_lang.c_str(), 513));
}
