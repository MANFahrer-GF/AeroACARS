// Einheitstests: Dataref-Dienst (dienst.cpp) gegen eine Schein-X-Plane-Welt.
//
// Jeder Test prüft außer seinem eigentlichen Ziel auch jedes gesendete Paket
// mit dem strengen JSON-Leser (≤ 8192 Byte, eine Zeile, gültiges UTF-8,
// "p":2) — ein kaputtes Paket fällt also in JEDEM Test auf.

#include "dienst.h"
#include "json_pruefer.h"
#include "schein_welt.h"
#include "testrahmen.h"

#include <algorithm>
#include <cmath>
#include <map>
#include <memory>
#include <set>
#include <string>
#include <vector>

using namespace aeroacars;

namespace {

const Absender CLIENT{0x7F000001u, 50000};
const Absender ANDERER{0x7F000001u, 50001};

struct Aufbau {
    ScheinWelt welt;
    ScheinUmgebung umg;
    std::unique_ptr<Dienst> d;
    size_t gelesen = 0;  // Index in umg.gesendet

    Aufbau() {
        welt.uhr = &umg;
        Kennung k;
        k.plugin_version = "1.0.0";
        k.xplane_version = 12100;
        k.xplm_version = 430;
        // Flugzeug-Kennung wie in X-Plane (Byte-Arrays fester Länge).
        welt.neu("sim/aircraft/view/acf_ICAO", typ::B).b = std::string("A20N") + std::string(36, '\0');
        welt.neu("sim/aircraft/view/acf_descrip", typ::B).b = std::string("A320neo") + std::string(253, '\0');
        welt.neu("sim/aircraft/view/acf_relative_path", typ::B).b = std::string("Aircraft/A320/a320.acf") + std::string(100, '\0');
        d.reset(new Dienst(welt, umg, k));
    }
    void sende(const std::string& s, const Absender& von = CLIENT) {
        d->empfange(von, s.data(), s.size());
    }
    void frame(double dt = 1.0 / 60.0) {
        umg.zeit += dt;
        d->frame();
    }
    // Frames mit PING je Sekunde (der Client lebt).
    void frames(int n, double dt = 1.0 / 60.0) {
        double seit_ping = 0.0;
        for (int i = 0; i < n; ++i) {
            if (seit_ping >= 1.0) {
                sende("PING");
                seit_ping = 0.0;
            }
            frame(dt);
            seit_ping += dt;
        }
    }
    // Frames ohne jede Anfrage (der Client schweigt).
    void frames_still(int n, double dt = 1.0 / 60.0) {
        for (int i = 0; i < n; ++i) frame(dt);
    }
    // Alle neuen Pakete, streng geprüft.
    std::vector<JWert> neue() {
        std::vector<JWert> aus;
        for (; gelesen < umg.gesendet.size(); ++gelesen) {
            JWert j;
            std::string grund;
            const std::string& p = umg.gesendet[gelesen].second;
            if (!pruefe_paket(p, &j, &grund)) {
                testrahmen::melde(__FILE__, __LINE__, "ungueltiges Paket: " + grund + "\n    " + p.substr(0, 300));
                continue;
            }
            aus.push_back(j);
        }
        return aus;
    }
    std::vector<JWert> neue_vom_typ(const std::string& t) {
        std::vector<JWert> aus;
        for (auto& j : neue()) if (j.hole("t")->text == t) aus.push_back(j);
        return aus;
    }
    void hallo() {
        sende("HALLO 2 test");
        frame();
        neue();
    }
};

std::string art(const JWert& t) { return t.hole("t") ? t.hole("t")->text : ""; }

// Setzt die abo-Antworten eines Abos (evtl. mehrteilig) zu einer Statusliste zusammen.
std::vector<JWert> status_aus(const std::vector<JWert>& pakete) {
    std::vector<JWert> st;
    for (const auto& p : pakete) {
        if (art(p) != "abo") continue;
        for (const auto& e : p.hole("st")->feld) st.push_back(e);
    }
    return st;
}

// Alle Werte einer Lieferung (seq) aus "w"-Paketen: index → Wert.
std::map<int, JWert> werte_aus(const std::vector<JWert>& pakete, int* seq = nullptr) {
    std::map<int, JWert> w;
    for (const auto& p : pakete) {
        if (art(p) != "w") continue;
        if (seq) *seq = static_cast<int>(p.hole("seq")->zahl);
        for (const auto& e : p.hole("v")->feld) w[static_cast<int>(e.feld[0].zahl)] = e.feld[1];
    }
    return w;
}

std::string status_text(const JWert& s) {
    std::string t = s.feld[1].text;
    if (s.feld.size() > 2) t += "," + s.feld[2].roh;
    return t;
}

}  // namespace

TEST(dienst_hallo_und_flugzeug) {
    Aufbau a;
    PRUEFE(a.d->bereit());
    a.sende("HALLO 2 1.9.12");
    auto p = a.neue();
    PRUEFE_GLEICH(p.size(), size_t(1));
    PRUEFE_TEXT(a.umg.gesendet[0].second,
                "{\"p\":2,\"t\":\"hallo\",\"plugin\":\"1.0.0\",\"xplane\":12100,\"xplm\":430}\n");
    PRUEFE(a.umg.gesendet[0].first == CLIENT);
    PRUEFE(a.d->client_angemeldet());
    a.frame();
    auto f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    PRUEFE_TEXT(f[0].hole("icao")->text, "A20N");
    PRUEFE_TEXT(f[0].hole("titel")->text, "A320neo");
    PRUEFE_TEXT(f[0].hole("pfad")->text, "Aircraft/A320/a320.acf");
    // Ohne Änderung keine weitere Meldung.
    a.frames(300);
    PRUEFE_GLEICH(a.neue_vom_typ("flugzeug").size(), size_t(0));
    // Flugzeugwechsel ohne Nachricht: der 2-s-Takt merkt es.
    a.welt.such("sim/aircraft/view/acf_ICAO")->b = std::string("B738") + std::string(36, '\0');
    a.frames(150);
    f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    PRUEFE_TEXT(f[0].hole("icao")->text, "B738");
    // Nach XPLM_MSG_PLANE_LOADED sofort (auch ohne Änderung).
    a.d->flugzeug_geladen();
    a.frame();
    PRUEFE_GLEICH(a.neue_vom_typ("flugzeug").size(), size_t(1));
}

TEST(dienst_flugzeug_fehlender_pfad_ist_null) {
    Aufbau a;
    a.welt.such("sim/aircraft/view/acf_relative_path")->registriert = false;
    a.welt.such("sim/aircraft/view/acf_descrip")->b = "Caf\xE9 \"Spezial\"\n";
    a.hallo();
    a.d->flugzeug_geladen();
    a.frame();
    auto f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    PRUEFE(f[0].hole("pfad")->art == JWert::Art::NUL);
    PRUEFE_TEXT(f[0].hole("titel")->text, "Caf\xC3\xA9 \"Spezial\"\n");
}

TEST(dienst_ohne_hallo_und_fremde_protokolle) {
    Aufbau a;
    a.sende("PING");
    a.sende("ABO 1 10\nsim/x");
    a.sende("LISTE 3");
    auto p = a.neue();
    PRUEFE_GLEICH(p.size(), size_t(3));
    for (auto& j : p) {
        PRUEFE_TEXT(art(j), "fehler");
        PRUEFE_TEXT(j.hole("grund")->text, "kein_hallo");
    }
    PRUEFE(p[1].hole("abo") && p[1].hole("abo")->zahl == 1);
    // Fremde Protokollversion: Antwort ja, Anmeldung nein.
    a.sende("HALLO 3 zukunft");
    p = a.neue();
    PRUEFE_GLEICH(p.size(), size_t(1));
    PRUEFE_TEXT(art(p[0]), "hallo");
    PRUEFE(!a.d->client_angemeldet());
    // Syntaxfehler: fehler mit Zeile.
    a.sende("ABO 1 10\nsim/a b");
    p = a.neue();
    PRUEFE_TEXT(p[0].hole("grund")->text, "name_ungueltig");
    PRUEFE(p[0].hole("zeile")->zahl == 2);
}

TEST(dienst_ping_pong) {
    Aufbau a;
    a.hallo();
    a.sende("PING\n");
    auto p = a.neue();
    PRUEFE_GLEICH(p.size(), size_t(1));
    PRUEFE_TEXT(a.umg.gesendet.back().second, "{\"p\":2,\"t\":\"pong\"}\n");
    // Anderer Port = anderer Client, nicht angemeldet.
    a.sende("PING", ANDERER);
    p = a.neue();
    PRUEFE_TEXT(p[0].hole("grund")->text, "kein_hallo");
    PRUEFE(a.umg.gesendet.back().first == ANDERER);
}

TEST(dienst_abo_status_und_werte) {
    Aufbau a;
    auto& lat = a.welt.neu("sim/flightmodel/position/latitude", typ::D | typ::F);
    lat.d = 51.23456789012345;
    lat.f = 51.2345f;
    a.welt.neu("sim/f", typ::F).f = 8.5f;
    a.welt.neu("sim/i", typ::I).i = -42;
    auto& vf = a.welt.neu("sim/vf", typ::VF);
    vf.vf = {0.5f, 1.5f, 2.5f, 3.5f, 4.5f, 5.5f, 6.5f, 7.5f};
    a.welt.neu("sim/vi", typ::VI).vi = {0, 0, 1};
    a.welt.neu("sim/b", typ::B).b = std::string("A20N") + std::string(36, '\0');
    a.welt.neu("sim/nan", typ::F).f = std::nanf("");
    a.welt.neu("sim/leer", 0);  // Typ unbekannt → fehlt
    a.hallo();

    a.sende("ABO 1 10\n"
            "sim/flightmodel/position/latitude\n"  // 0 d
            "sim/f\n"                              // 1 f
            "sim/i\n"                              // 2 i
            "sim/vf\n"                             // 3 vf,8
            "sim/vi\n"                             // 4 vi,3
            "sim/b\n"                              // 5 b,40
            "gibt/es/nicht\n"                      // 6 fehlt
            "sim/vf[7]\n"                          // 7 f
            "sim/vf[8]\n"                          // 8 fehlt (Index = Länge)
            "sim/f[0]\n"                           // 9 fehlt (Skalar mit Index)
            "sim/vi[2]\n"                          // 10 i
            "sim/b[0]\n"                           // 11 i (Byte)
            "sim/nan\n"                            // 12 f → null
            "sim/leer\n");                         // 13 fehlt
    PRUEFE(a.neue().empty());  // Antwort erst nach der Suche im Frame
    a.frame();
    auto p = a.neue();
    auto st = status_aus(p);
    PRUEFE_GLEICH(st.size(), size_t(14));
    const char* soll[] = {"d,1", "f,1", "i,1", "vf,8", "vi,3", "b,40", "fehlt", "f,1",
                          "fehlt", "fehlt", "i,1", "i,1", "f,1", "fehlt"};
    for (size_t k = 0; k < st.size() && k < 14; ++k) {
        PRUEFE(st[k].feld[0].zahl == static_cast<double>(k));
        PRUEFE_TEXT(status_text(st[k]), soll[k]);
    }
    // Die abo-Antwort kommt VOR dem ersten Wert.
    PRUEFE_TEXT(art(p[0]), "abo");
    int seq = 0;
    auto w = werte_aus(p, &seq);
    PRUEFE_GLEICH(seq, 1);
    PRUEFE(w.count(0) && w[0].zahl == 51.23456789012345);  // volle double-Genauigkeit
    PRUEFE(w[1].zahl == 8.5);
    PRUEFE(w[2].zahl == -42);
    PRUEFE_GLEICH(w[3].feld.size(), size_t(8));
    PRUEFE(w[3].feld[7].zahl == 7.5);
    PRUEFE_GLEICH(w[4].feld.size(), size_t(3));
    PRUEFE_TEXT(w[5].text, "A20N");
    PRUEFE(w[7].zahl == 7.5);
    PRUEFE(w[10].zahl == 1);
    PRUEFE(w[11].zahl == 'A');
    PRUEFE(w[12].art == JWert::Art::NUL);
    // Ein fehlender Name liefert nie einen Wert.
    PRUEFE(!w.count(6) && !w.count(8) && !w.count(9) && !w.count(13));
}

TEST(dienst_rate_je_abo) {
    Aufbau a;
    a.welt.neu("sim/x", typ::F).f = 1.0f;
    a.hallo();
    a.sende("ABO 1 10\nsim/x");
    a.sende("ABO 2 50\nsim/x");
    a.sende("ABO 3 1\nsim/x");
    a.frame();
    a.neue();
    std::map<int, int> lieferungen;
    for (int i = 0; i < 600; ++i) {  // 10 s bei 60 fps
        if (i % 60 == 0) a.sende("PING");
        a.frame();
        for (auto& j : a.neue_vom_typ("w")) lieferungen[static_cast<int>(j.hole("abo")->zahl)]++;
    }
    PRUEFE(lieferungen[1] >= 98 && lieferungen[1] <= 102);
    // 50 Hz bei 60 fps: höchstens eine Lieferung je Frame, im Mittel 50/s.
    PRUEFE(lieferungen[2] >= 480 && lieferungen[2] <= 502);
    PRUEFE(lieferungen[3] >= 9 && lieferungen[3] <= 11);
    // Langsamer Sim (20 fps): 50-Hz-Abo liefert einmal je Frame, ohne Stoß.
    std::map<int, int> langsam;
    for (int i = 0; i < 100; ++i) {
        a.frame(0.05);
        std::map<int, int> jetzt;
        for (auto& j : a.neue_vom_typ("w")) jetzt[static_cast<int>(j.hole("abo")->zahl)]++;
        PRUEFE(jetzt[2] <= 1);
        if (i % 20 == 0) a.sende("PING");
    }
}

TEST(dienst_nachsuche_fehlender_namen) {
    Aufbau a;
    auto& spaet = a.welt.neu("laminar/a330/spaet", typ::I);
    spaet.i = 7;
    spaet.registriert = false;  // das Flugzeug-Plugin hat ihn noch nicht angelegt
    a.welt.neu("sim/x", typ::F).f = 2.0f;
    a.hallo();
    a.sende("ABO 1 5\nsim/x\nlaminar/a330/spaet");
    a.frame();
    auto st = status_aus(a.neue());
    PRUEFE_TEXT(status_text(st[1]), "fehlt");
    spaet.registriert = true;
    // Innerhalb von 2 s + ein paar Frames: neue abo-Antwort, danach Werte.
    std::vector<JWert> p;
    for (int i = 0; i < 150; ++i) {
        a.frame();
        for (auto& j : a.neue()) p.push_back(j);
        if (i % 60 == 0) a.sende("PING");
    }
    auto neu = status_aus(p);
    PRUEFE_GLEICH(neu.size(), size_t(2));
    if (neu.size() == 2) PRUEFE_TEXT(status_text(neu[1]), "i,1");
    // Werte für Index 1 erst NACH der neuen abo-Antwort.
    bool antwort_gesehen = false, wert_vorher = false, wert_nachher = false;
    for (auto& j : p) {
        if (art(j) == "abo") antwort_gesehen = true;
        if (art(j) == "w") {
            for (auto& e : j.hole("v")->feld) {
                if (e.feld[0].zahl == 1) (antwort_gesehen ? wert_nachher : wert_vorher) = true;
            }
        }
    }
    PRUEFE(!wert_vorher);
    PRUEFE(wert_nachher);
    // Unveränderter Status: keine weiteren abo-Antworten über 10 s.
    int antworten = 0;
    for (int i = 0; i < 600; ++i) {
        a.frame();
        antworten += static_cast<int>(a.neue_vom_typ("abo").size());
        if (i % 60 == 0) a.sende("PING");
    }
    PRUEFE_GLEICH(antworten, 0);
}

TEST(dienst_verwaiste_datarefs_nach_flugzeugwechsel) {
    Aufbau a;
    auto& toliss = a.welt.neu("AirbusFBW/APU_Avail", typ::I);
    toliss.i = 1;
    a.hallo();
    a.sende("ABO 1 5\nAirbusFBW/APU_Avail");
    a.frame();
    PRUEFE_TEXT(status_text(status_aus(a.neue())[0]), "i,1");
    // Flugzeugwechsel: Plugin entladen, Name bleibt findbar, aber verwaist.
    toliss.gueltig = false;
    a.d->flugzeug_geladen();
    a.frame();
    auto p = a.neue();
    auto st = status_aus(p);
    PRUEFE_GLEICH(st.size(), size_t(1));
    if (!st.empty()) PRUEFE_TEXT(status_text(st[0]), "fehlt");
    PRUEFE(!werte_aus(p).count(0));
    bool flugzeug = false;
    for (auto& j : p) flugzeug = flugzeug || art(j) == "flugzeug";
    PRUEFE(flugzeug);
    // Gleiches Flugzeug wieder geladen: Name wird wieder gültig.
    toliss.gueltig = true;
    a.d->flughafen_geladen();
    a.frame();
    st = status_aus(a.neue());
    PRUEFE_GLEICH(st.size(), size_t(1));
    if (!st.empty()) PRUEFE_TEXT(status_text(st[0]), "i,1");
}

TEST(dienst_arraylaenge_aendert_sich) {
    Aufbau a;
    auto& arr = a.welt.neu("sim/arr", typ::VF);
    arr.vf = std::vector<float>(8, 1.0f);
    a.hallo();
    a.sende("ABO 1 10\nsim/arr\nsim/arr[6]");
    a.frame();
    auto st = status_aus(a.neue());
    PRUEFE_TEXT(status_text(st[0]), "vf,8");
    PRUEFE_TEXT(status_text(st[1]), "f,1");
    arr.vf.resize(4);
    // Bis zur nächsten Prüfung: Array kürzer geliefert, Element fällt aus.
    a.frame(0.2);
    auto w = werte_aus(a.neue());
    PRUEFE_GLEICH(w[0].feld.size(), size_t(4));
    PRUEFE(!w.count(1));
    std::vector<JWert> p;
    for (int i = 0; i < 150; ++i) {
        a.frame();
        for (auto& j : a.neue()) p.push_back(j);
        if (i % 60 == 0) a.sende("PING");
    }
    st = status_aus(p);
    PRUEFE_GLEICH(st.size(), size_t(2));
    if (st.size() == 2) {
        PRUEFE_TEXT(status_text(st[0]), "vf,4");
        PRUEFE_TEXT(status_text(st[1]), "fehlt");
    }
}

TEST(dienst_zeitueberschreitung) {
    Aufbau a;
    a.welt.neu("sim/x", typ::F);
    a.hallo();
    a.sende("ABO 1 10\nsim/x");
    // Mit PING alle 1 s bleibt alles.
    for (int s = 0; s < 10; ++s) {
        a.sende("PING");
        a.frames(60);
    }
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(1));
    PRUEFE(a.d->braucht_jeden_frame());
    a.neue();
    // 5 s Stille: alles weg, Client vergessen.
    a.frames_still(305);
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(0));
    PRUEFE(!a.d->client_angemeldet());
    PRUEFE(!a.d->braucht_jeden_frame());
    a.neue();
    a.frames_still(60);
    PRUEFE(a.neue_vom_typ("w").empty());
    a.sende("PING");
    auto p = a.neue();
    PRUEFE_TEXT(p[0].hole("grund")->text, "kein_hallo");
}

TEST(dienst_mehrteiliges_abo) {
    Aufbau a;
    for (int i = 0; i < 6; ++i) a.welt.neu("n" + std::to_string(i), typ::I).i = i * 10;
    a.hallo();
    a.sende("ABO 4 5 1 3\nn0\nn1");
    a.sende("ABO 4 5 2 3\nn2");
    a.frame();
    PRUEFE(a.neue_vom_typ("abo").empty());  // erst mit dem letzten Teil
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(0));
    a.sende("ABO 4 5 3 3\nn3\nn4\nn5");
    a.frame();
    auto p = a.neue();
    auto st = status_aus(p);
    PRUEFE_GLEICH(st.size(), size_t(6));
    auto w = werte_aus(p);
    for (int i = 0; i < 6; ++i) PRUEFE(w[i].zahl == i * 10);  // Reihenfolge über Teile
}

TEST(dienst_mehrteilig_fehlerfaelle) {
    Aufbau a;
    a.welt.neu("x", typ::I);
    a.hallo();
    auto grund = [&a]() {
        auto p = a.neue();
        return p.empty() ? std::string("<nichts>") : p.back().hole("grund") ? p.back().hole("grund")->text : art(p.back());
    };
    // Falsche Reihenfolge.
    a.sende("ABO 1 5 1 3\nx");
    a.sende("ABO 1 5 3 3\nx");
    PRUEFE_TEXT(grund(), "abo_teil_reihenfolge");
    // Danach ist der Aufbau verworfen: auch Teil 2 passt nicht mehr.
    a.sende("ABO 1 5 2 3\nx");
    PRUEFE_TEXT(grund(), "abo_teil_reihenfolge");
    // Doppelter Teil.
    a.sende("ABO 1 5 1 3\nx");
    a.sende("ABO 1 5 2 3\nx");
    a.sende("ABO 1 5 2 3\nx");
    PRUEFE_TEXT(grund(), "abo_teil_reihenfolge");
    // Teil ohne Teil 1.
    a.sende("ABO 2 5 2 2\nx");
    PRUEFE_TEXT(grund(), "abo_teil_reihenfolge");
    // Widerspruch in Rate oder Teilezahl.
    a.sende("ABO 1 5 1 2\nx");
    a.sende("ABO 1 6 2 2\nx");
    PRUEFE_TEXT(grund(), "abo_teile_widerspruch");
    a.sende("ABO 1 5 1 2\nx");
    a.sende("ABO 1 5 2 3\nx");
    PRUEFE_TEXT(grund(), "abo_teile_widerspruch");
    // Fehlender letzter Teil: nichts wird aktiv.
    a.sende("ABO 3 5 1 2\nx");
    a.frames(10);
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(0));
    // Neuer Teil 1 beginnt sauber neu.
    a.sende("ABO 3 5 1 1\nx");
    a.frame();
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(1));
    // Ohne Namen.
    a.neue();
    a.sende("ABO 5 5");
    PRUEFE_TEXT(grund(), "keine_namen");
    a.sende("ABO 5 5 1 2");
    a.sende("ABO 5 5 2 2");
    PRUEFE_TEXT(grund(), "keine_namen");
    // Zu viele Namen über zwei Teile.
    std::string t1 = "ABO 6 1 1 2\n", t2 = "ABO 6 1 2 2\n";
    for (int i = 0; i < 5000; ++i) { t1 += "x\n"; t2 += "x\n"; }
    a.sende(t1);
    PRUEFE(a.neue().empty());
    a.sende(t2);
    PRUEFE_TEXT(grund(), "zu_viele_namen");
}

TEST(dienst_ende_abo_und_ersetzen) {
    Aufbau a;
    a.welt.neu("x", typ::I).i = 1;
    a.welt.neu("y", typ::I).i = 2;
    a.hallo();
    a.sende("ABO 1 10\nx");
    a.frame();
    PRUEFE_GLEICH(status_aus(a.neue()).size(), size_t(1));
    // Gleiche ID ersetzt.
    a.sende("ABO 1 10\ny\nx");
    a.frame();
    auto p = a.neue();
    PRUEFE_GLEICH(status_aus(p).size(), size_t(2));
    auto w = werte_aus(p);
    PRUEFE(w[0].zahl == 2 && w[1].zahl == 1);
    a.sende("ENDE-ABO 1");
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(0));
    a.frames(30);
    PRUEFE(a.neue_vom_typ("w").empty());
    a.sende("ENDE-ABO 9");  // unbekannt: harmlos, keine Antwort
    PRUEFE(a.neue().empty());
}

TEST(dienst_neuer_client_verwirft_alte_abos) {
    Aufbau a;
    a.welt.neu("x", typ::I);
    a.hallo();
    a.sende("ABO 1 10\nx");
    a.frame();
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(1));
    a.sende("HALLO 2 neu", ANDERER);
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(0));
    a.neue();
    a.sende("PING");  // alter Port ist nicht mehr angemeldet
    PRUEFE_TEXT(a.neue()[0].hole("grund")->text, "kein_hallo");
    // Erneutes HALLO vom selben Client behält die Abos.
    a.sende("ABO 2 10\nx", ANDERER);
    a.frame();
    a.sende("HALLO 2 neu", ANDERER);
    PRUEFE_GLEICH(a.d->aktive_abos(), size_t(1));
}

TEST(dienst_liste) {
    Aufbau a;
    std::set<std::string> gueltig;
    for (const auto& r : a.welt.refs) gueltig.insert(r->name);
    for (int i = 0; i < 3000; ++i) {
        const std::string n = "sim/test/dataref_mit_langem_namen_" + std::to_string(i);
        a.welt.neu(n, typ::F);
        gueltig.insert(n);
    }
    a.welt.neu("mit leerzeichen", typ::F);
    a.welt.neu("umlaut/\xC3\xA4", typ::F);
    a.welt.neu(std::string(513, 'z'), typ::F);
    a.welt.neu("", typ::F);
    a.welt.neu("x\"y\\z", typ::I);  // druckbar → abonnierbar → gemeldet
    gueltig.insert("x\"y\\z");
    a.hallo();
    a.sende("LISTE 42");
    std::vector<JWert> p;
    int max_je_frame = 0;
    for (int i = 0; i < 200; ++i) {
        const size_t vorher = a.umg.gesendet.size();
        a.frame();
        max_je_frame = std::max(max_je_frame, static_cast<int>(a.umg.gesendet.size() - vorher));
        for (auto& j : a.neue_vom_typ("liste")) p.push_back(j);
        if (i % 60 == 0) a.sende("PING");
    }
    PRUEFE(!p.empty());
    PRUEFE(max_je_frame <= grenzen::MAX_PAKETE_JE_FRAME);
    std::set<std::string> gemeldet;
    for (size_t k = 0; k < p.size(); ++k) {
        PRUEFE(p[k].hole("id")->zahl == 42);
        PRUEFE(p[k].hole("teil")->zahl == static_cast<double>(k + 1));
        PRUEFE(p[k].hole("teile")->zahl == static_cast<double>(p.size()));
        for (auto& n : p[k].hole("n")->feld) gemeldet.insert(n.text);
    }
    PRUEFE(gemeldet == gueltig);
    PRUEFE(!a.d->braucht_jeden_frame());  // fertig, Speicher frei
    // Nicht verfügbar (X-Plane 11).
    a.welt.liste_da = false;
    a.sende("LISTE 43");
    auto f = a.neue();
    PRUEFE_TEXT(f[0].hole("grund")->text, "liste_nicht_verfuegbar");
    PRUEFE(f[0].hole("id")->zahl == 43);
}

TEST(dienst_zeitbudget_verteilt_grosses_abo) {
    Aufbau a;
    std::string abo = "ABO 1 50\n";
    for (int i = 0; i < 200; ++i) {
        a.welt.neu("v" + std::to_string(i), typ::F).f = static_cast<float>(i);
        abo += "v" + std::to_string(i) + "\n";
    }
    a.hallo();
    a.sende(abo);
    a.frame();
    a.neue();
    // Jeder Lesezugriff kostet 100 µs → 1 ms Budget ≈ 10 Werte je Frame.
    a.welt.lese_kosten = 0.0001;
    std::map<int, JWert> w;
    int seq_start = -1, seq = 0;
    int max_lesen = 0;
    for (int i = 0; i < 100; ++i) {
        const int vorher = a.welt.lesezugriffe;
        a.frame();
        max_lesen = std::max(max_lesen, a.welt.lesezugriffe - vorher);
        for (auto& j : a.neue_vom_typ("w")) {
            const int s = static_cast<int>(j.hole("seq")->zahl);
            if (seq_start < 0) seq_start = s;
            if (s == seq_start) {
                for (auto& e : j.hole("v")->feld) w[static_cast<int>(e.feld[0].zahl)] = e.feld[1];
            }
            seq = s;
        }
        if (i % 30 == 0) a.sende("PING");
    }
    // Budget: höchstens ~1 ms Lesen je Frame (plus die freie erste Einheit).
    PRUEFE(max_lesen <= 12);
    // Die Lieferung ist trotzdem vollständig (über mehrere Frames).
    PRUEFE_GLEICH(w.size(), size_t(200));
    PRUEFE(seq > seq_start);
}

TEST(dienst_grosses_abo_pakete_und_puffer) {
    Aufbau a;
    std::string abo_text;
    // 8192 Namen, davon viele lange Arrays → viele volle Pakete.
    for (int i = 0; i < 8192; ++i) {
        const std::string n = "arr" + std::to_string(i % 64);
        abo_text += n + "\n";
    }
    for (int i = 0; i < 64; ++i) {
        auto& r = a.welt.neu("arr" + std::to_string(i), typ::VF);
        r.vf = std::vector<float>(300, -1.17549435e-38f);  // länger als 256 → gekappt
    }
    a.welt.ueberschuss = 50;  // fehlerhaftes Plugin schreibt über max hinaus
    a.hallo();
    // In zwei Teilen (ein Datagramm darf höchstens 64 KiB haben).
    std::string t1 = "ABO 1 1 1 2\n", t2 = "ABO 1 1 2 2\n";
    size_t pos = 0;
    for (int i = 0; i < 8192; ++i) {
        const size_t nl = abo_text.find('\n', pos);
        (i < 4096 ? t1 : t2) += abo_text.substr(pos, nl - pos + 1);
        pos = nl + 1;
    }
    a.sende(t1);
    a.sende(t2);
    // Pakete gleich auswerten statt sammeln (eine Lieferung sind ~8000 Pakete).
    int max_je_frame = 0;
    std::vector<JWert> st;
    std::map<int, std::map<int, size_t>> je_seq;
    for (int i = 0; i < 1500; ++i) {
        const size_t vorher = a.umg.gesendet.size();
        a.frame();
        max_je_frame = std::max(max_je_frame, static_cast<int>(a.umg.gesendet.size() - vorher));
        for (auto& j : a.neue()) {
            if (art(j) == "abo") {
                for (auto& e : j.hole("st")->feld) st.push_back(e);
            } else if (art(j) == "w") {
                const int s = static_cast<int>(j.hole("seq")->zahl);
                for (auto& e : j.hole("v")->feld) {
                    je_seq[s][static_cast<int>(e.feld[0].zahl)] = e.feld[1].feld.size();
                }
            }
        }
        a.umg.gesendet.clear();
        a.gelesen = 0;
        if (i % 60 == 0) a.sende("PING");
        if (je_seq.size() >= 2) break;  // erste Lieferung ist vollständig
    }
    PRUEFE(max_je_frame <= grenzen::MAX_PAKETE_JE_FRAME + 1);  // + evtl. flugzeug
    PRUEFE_GLEICH(st.size(), size_t(8192));
    if (!st.empty()) PRUEFE_TEXT(status_text(st[0]), "vf,256");
    // Eine vollständige Lieferung: alle 8192 Werte, je 256 Elemente.
    PRUEFE(je_seq.size() >= 2);
    if (!je_seq.empty()) {
        const auto& erste = je_seq.begin()->second;
        PRUEFE_GLEICH(erste.size(), size_t(8192));
        bool alle_256 = true;
        for (auto& kv : erste) alle_256 = alle_256 && kv.second == 256;
        PRUEFE(alle_256);
    }
}

TEST(dienst_voller_socket_verliert_nichts) {
    Aufbau a;
    std::string abo = "ABO 1 1\n";
    for (int i = 0; i < 2000; ++i) {
        a.welt.neu("w" + std::to_string(i), typ::D).d = i + 0.25;
        abo += "w" + std::to_string(i) + "\n";
    }
    a.hallo();
    a.sende(abo);
    a.frame();
    a.neue();
    a.umg.voll_noch = 3;  // die nächsten drei Sendeversuche scheitern "voll"
    std::vector<JWert> p;
    for (int i = 0; i < 70; ++i) {
        a.frame();
        for (auto& j : a.neue_vom_typ("w")) p.push_back(j);
    }
    // Erste Lieferung vollständig, Teile lückenlos in Reihenfolge.
    PRUEFE(!p.empty());
    const int seq = static_cast<int>(p[0].hole("seq")->zahl);
    const int teile = static_cast<int>(p[0].hole("teile")->zahl);
    PRUEFE(teile > 1);
    int erwartet = 1;
    std::set<int> idx;
    for (auto& j : p) {
        if (static_cast<int>(j.hole("seq")->zahl) != seq) break;
        PRUEFE(j.hole("teil")->zahl == erwartet);
        ++erwartet;
        for (auto& e : j.hole("v")->feld) idx.insert(static_cast<int>(e.feld[0].zahl));
    }
    PRUEFE_GLEICH(erwartet - 1, teile);
    PRUEFE_GLEICH(idx.size(), size_t(2000));
}

TEST(dienst_bytes_mit_steuerzeichen_und_latin1) {
    Aufbau a;
    a.welt.neu("s", typ::B).b = std::string("A\x01\"\\\n\xE9\xC3\xA9", 9) + std::string("\0rest", 5);
    a.hallo();
    a.sende("ABO 1 10\ns");
    a.frame();
    auto w = werte_aus(a.neue());
    PRUEFE_TEXT(w[0].text, "A\x01\"\\\n\xC3\xA9\xC3\xA9");  // bis zum NUL
}

TEST(dienst_braucht_jeden_frame) {
    Aufbau a;
    PRUEFE(!a.d->braucht_jeden_frame());
    a.hallo();
    PRUEFE(!a.d->braucht_jeden_frame());  // angemeldet, aber nichts zu liefern
    a.welt.neu("x", typ::I);
    a.sende("ABO 1 1\nx");
    PRUEFE(a.d->braucht_jeden_frame());
    a.sende("ENDE-ABO 1");
    PRUEFE(!a.d->braucht_jeden_frame());
}
