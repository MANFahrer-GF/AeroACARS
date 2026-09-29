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
    // Kopfzeilenfehler: fehler mit Zeile, ID und Generation.
    a.sende("ABO 3 99 g7\nsim/a");
    p = a.neue();
    PRUEFE_TEXT(p[0].hole("grund")->text, "rate_ungueltig");
    PRUEFE(p[0].hole("zeile")->zahl == 1);
    PRUEFE(p[0].hole("abo")->zahl == 3);
    PRUEFE(p[0].hole("gen")->zahl == 7);
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
    {
        // Sofort nur die Empfangsbestätigung; der Status erst nach der Suche.
        auto sofort = a.neue();
        PRUEFE_GLEICH(sofort.size(), size_t(1));
        if (!sofort.empty()) {
            PRUEFE_TEXT(art(sofort[0]), "abo_empfangen");
            PRUEFE(sofort[0].hole("namen")->zahl == 14);
        }
    }
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
    // 3000 Namen, alle ganze Arrays mit 256 floats → viele volle Pakete.
    // (Bis zur Codex-Abnahme 8192 — das sind im schlimmsten Fall 8192 × 4361
    // Byte ≈ 34 MiB Ausgabestapel und liegt seit H3 über dem Budget je Abo
    // (16 MiB); dienst_h3_speicher_limit_je_abo prüft genau diese Ablehnung.
    // 3000 × 4361 ≈ 12,5 MiB passt.)
    constexpr int N = 3000;
    for (int i = 0; i < N; ++i) {
        const std::string n = "arr" + std::to_string(i % 64);
        abo_text += n + "\n";
    }
    for (int i = 0; i < 64; ++i) {
        auto& r = a.welt.neu("arr" + std::to_string(i), typ::VF);
        r.vf = std::vector<float>(300, -1.17549435e-38f);  // länger als 256 → gekappt
    }
    a.welt.ueberschuss = 50;  // fehlerhaftes Plugin schreibt über max hinaus
    a.hallo();
    // In zwei Teilen (wie der Client bei großen Abos).
    std::string t1 = "ABO 1 1 1 2\n", t2 = "ABO 1 1 2 2\n";
    size_t pos = 0;
    for (int i = 0; i < N; ++i) {
        const size_t nl = abo_text.find('\n', pos);
        (i < N / 2 ? t1 : t2) += abo_text.substr(pos, nl - pos + 1);
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
    // Seit M1 zählt auch `flugzeug` gegen die 16 (vorher + 1).
    PRUEFE(max_je_frame <= grenzen::MAX_PAKETE_JE_FRAME);
    PRUEFE_GLEICH(st.size(), size_t(N));
    if (!st.empty()) PRUEFE_TEXT(status_text(st[0]), "vf,256");
    // Eine vollständige Lieferung: alle Werte, je 256 Elemente.
    PRUEFE(je_seq.size() >= 2);
    if (!je_seq.empty()) {
        const auto& erste = je_seq.begin()->second;
        PRUEFE_GLEICH(erste.size(), size_t(N));
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

// =============================================================================
// Cloud-QS (H1, N1, N3, Generation)
// =============================================================================

namespace {

// ABO-Datagramme wie der Client: ≤ 60 000 Byte je Teil, Generation am Ende.
std::vector<std::string> abo_datagramme(int id, int rate, const std::vector<std::string>& namen, int gen) {
    std::vector<std::string> koerper(1);
    for (const auto& n : namen) {
        if (koerper.back().size() + n.size() + 1 > 60000) koerper.emplace_back();
        koerper.back() += n + "\n";
    }
    std::vector<std::string> aus;
    for (size_t k = 0; k < koerper.size(); ++k) {
        std::string kopf = "ABO " + std::to_string(id) + " " + std::to_string(rate);
        if (koerper.size() > 1) kopf += " " + std::to_string(k + 1) + " " + std::to_string(koerper.size());
        kopf += " g" + std::to_string(gen) + "\n";
        aus.push_back(kopf + koerper[k]);
    }
    return aus;
}

// Grobe Kosten echter XPLM-Aufrufe (Hash-Suche + Handle-Prüfung), damit die
// Zeitbudgets in der Schein-Welt wirken. Am echten X-Plane misst das Plugin
// die Werte beim Start selbst (Log.txt).
void setze_xplm_kosten(Aufbau& a) {
    a.welt.such_kosten = 0.5e-6;
    a.welt.gueltig_kosten = 0.2e-6;
    a.welt.lese_kosten = 0.1e-6;
}

std::vector<std::string> vermessungs_namen(Aufbau& a, const std::string& praefix, int n) {
    std::vector<std::string> namen;
    for (int i = 0; i < n; ++i) {
        const std::string name = praefix + std::to_string(i);
        // Jeder fünfte fehlt (wie beim Vermessen: viele Kandidaten gibt es nicht).
        if (i % 5 != 0) a.welt.neu(name, i % 3 == 0 ? typ::I : typ::F).f = static_cast<float>(i);
        namen.push_back(name);
    }
    return namen;
}

// Treibt den Dienst mit 30 fps wie der Client: PING je Sekunde, und ein ABO,
// dessen Status nach 2 s nicht da ist, wird neu geschickt (Client-Regel aus
// der Cloud-QS). Liefert je Abo die Zeit bis zur vollständigen abo-Antwort.
struct Messung {
    std::map<int, double> erster_status;  // id → Sekunden ab Anmeldung
    std::map<int, int> neusendungen;
    std::map<int, int> w_pakete;
    std::map<int, int> empfangen;         // abo_empfangen je id
    int abo_antworten = 0;  // vollständige Antworten (Pakete mit teil 1)
};

Messung treibe(Aufbau& a, const std::map<int, std::vector<std::string>>& abos, double max_s,
               double neu_nach_s = 2.0) {
    Messung m;
    const double fps = 30.0;
    const double t0 = a.umg.zeit;
    std::map<int, double> gesendet_um;
    std::map<int, std::map<int, int>> teile;  // id → teil → 1
    for (const auto& kv : abos) {
        for (const auto& d : kv.second) a.sende(d);
        gesendet_um[kv.first] = a.umg.zeit;
    }
    double seit_ping = 0.0;
    while (a.umg.zeit - t0 < max_s) {
        a.frame(1.0 / fps);
        seit_ping += 1.0 / fps;
        if (seit_ping >= 1.0) { a.sende("PING"); seit_ping = 0.0; }
        for (auto& j : a.neue()) {
            const std::string t = art(j);
            if (t == "abo_empfangen") { m.empfangen[static_cast<int>(j.hole("abo")->zahl)]++; continue; }
            if (t == "w") { m.w_pakete[static_cast<int>(j.hole("abo")->zahl)]++; continue; }
            if (t != "abo") continue;
            if (j.hole("teil")->zahl == 1) ++m.abo_antworten;
            const int id = static_cast<int>(j.hole("abo")->zahl);
            teile[id][static_cast<int>(j.hole("teil")->zahl)] = 1;
            if (static_cast<int>(teile[id].size()) == static_cast<int>(j.hole("teile")->zahl) &&
                !m.erster_status.count(id)) {
                m.erster_status[id] = a.umg.zeit - t0;
            }
        }
        bool alle = true;
        for (const auto& kv : abos) {
            if (m.erster_status.count(kv.first)) continue;
            alle = false;
            if (a.umg.zeit - gesendet_um[kv.first] >= neu_nach_s) {
                for (const auto& d : kv.second) a.sende(d);
                gesendet_um[kv.first] = a.umg.zeit;
                m.neusendungen[kv.first]++;
            }
        }
        if (alle) break;
    }
    return m;
}

}  // namespace

TEST(dienst_h1_8192_namen_status_unter_1s) {
    Aufbau a;
    setze_xplm_kosten(a);
    const auto namen = vermessungs_namen(a, "vermessung/kandidat_", 8192);
    a.hallo();
    const auto m = treibe(a, {{2, abo_datagramme(2, 1, namen, 1)}}, 30.0);
    PRUEFE(m.erster_status.count(2));
    const double t = m.erster_status.count(2) ? m.erster_status.at(2) : 99.0;
    std::printf("     8192 Namen @30 fps: erster Status nach %.2f s, %d Neusendungen\n", t,
                m.neusendungen.count(2) ? m.neusendungen.at(2) : 0);
    PRUEFE(t < 1.0);
    PRUEFE(!m.neusendungen.count(2));
    PRUEFE_GLEICH(m.empfangen.at(2), 1);  // sofortige Empfangsbestätigung
}

TEST(dienst_h1_identisches_abo_setzt_nichts_zurueck) {
    Aufbau a;
    setze_xplm_kosten(a);
    a.welt.such_kosten = 2e-6;  // absichtlich langsam: Suche braucht > 1 s
    const auto namen = vermessungs_namen(a, "v/", 8192);
    a.hallo();
    // Aggressiver Client: schickt alle 0,25 s dasselbe ABO. Vorher begann
    // die Suche jedes Mal bei 0 und kam nie an.
    const auto m = treibe(a, {{2, abo_datagramme(2, 1, namen, 5)}}, 30.0, 0.25);
    PRUEFE(m.erster_status.count(2));
    std::printf("     8192 Namen, 2 us/Suche, Neusenden alle 0,25 s: Status nach %.2f s, %d Neusendungen\n",
                m.erster_status.count(2) ? m.erster_status.at(2) : 99.0,
                m.neusendungen.count(2) ? m.neusendungen.at(2) : 0);
    PRUEFE_GLEICH(m.abo_antworten, 1);  // genau EINE vollständige Antwort
    // Nach dem Status: identisches ABO → Status noch einmal, Werte laufen weiter.
    for (const auto& d : abo_datagramme(2, 1, namen, 5)) a.sende(d);
    a.frames(5);
    auto p = a.neue();
    int abo_pakete = 0, empfangen = 0;
    for (auto& j : p) {
        if (art(j) == "abo" && j.hole("teil")->zahl == 1) { ++abo_pakete; PRUEFE(j.hole("gen")->zahl == 5); }
        if (art(j) == "abo_empfangen") ++empfangen;
    }
    PRUEFE_GLEICH(abo_pakete, 1);  // Status genau einmal erneut
    PRUEFE_GLEICH(empfangen, 1);
    // Neue Generation = neues Abo (Suche neu).
    // (8192 Namen × 2,2 µs Suche bei 0,3 ms je Frame ≈ 60 Frames. Seit die Uhr
    // nach jedem XPLM-Aufruf gilt, beginnt ein am Budget abgebrochener
    // Eintrag im nächsten Frame neu — ein Frame mehr; daher 90 statt 60.)
    for (const auto& d : abo_datagramme(2, 1, namen, 6)) a.sende(d);
    a.frames(90);
    p = a.neue();
    bool gen6 = false;
    for (auto& j : p) if (art(j) == "w" && j.hole("gen")->zahl == 6) gen6 = true;
    PRUEFE(gen6);
}

TEST(dienst_h1_sechs_mal_8192_und_telemetrie) {
    Aufbau a;
    setze_xplm_kosten(a);
    std::map<int, std::vector<std::string>> abos;
    // Abo 1: Telemetrie (sim/…, 20 Hz).
    std::vector<std::string> tele;
    for (int i = 0; i < 20; ++i) {
        const std::string n = "sim/tele/" + std::to_string(i);
        a.welt.neu(n, typ::D).d = i;
        tele.push_back(n);
    }
    abos[1] = abo_datagramme(1, 20, tele, 1);
    for (int id = 2; id <= 7; ++id) {
        abos[id] = abo_datagramme(id, 1, vermessungs_namen(a, "abo" + std::to_string(id) + "/n", 8192), 1);
    }
    a.hallo();
    const double t0 = a.umg.zeit;
    const auto m = treibe(a, abos, 30.0);
    for (int id = 1; id <= 7; ++id) {
        PRUEFE(m.erster_status.count(id));
        std::printf("     Abo %d: erster Status nach %.2f s, %d Neusendungen\n", id,
                    m.erster_status.count(id) ? m.erster_status.at(id) : 99.0,
                    m.neusendungen.count(id) ? m.neusendungen.at(id) : 0);
    }
    // Telemetrie: Status im ersten Frame, danach durchgehend ~20 Hz, auch
    // während sechs große Abos suchen.
    PRUEFE(m.erster_status.count(1) && m.erster_status.at(1) < 0.05);
    const double dauer = a.umg.zeit - t0;
    const double hz = m.w_pakete.count(1) ? m.w_pakete.at(1) / dauer : 0.0;
    std::printf("     Telemetrie waehrend der Suche: %.1f Hz (Soll 20)\n", hz);
    PRUEFE(hz > 17.0);
    // Das erste große Abo deutlich unter 1 s (kürzester Rest zuerst).
    double erstes = 99.0;
    for (int id = 2; id <= 7; ++id) {
        if (m.erster_status.count(id) && m.erster_status.at(id) < erstes) erstes = m.erster_status.at(id);
    }
    PRUEFE(erstes < 1.0);
}

TEST(dienst_h1_rundlauf_nur_ueber_belegte_abos) {
    // Drei schwere Liefer-Abos (je 3000 Arrays) hinter Abo 3: vorher verhungerten
    // die höheren IDs. Jetzt müssen alle regelmäßig liefern.
    Aufbau a;
    for (int i = 0; i < 64; ++i) a.welt.neu("arr/" + std::to_string(i), typ::VF).vf = std::vector<float>(64, 1.0f);
    a.welt.neu("sim/x", typ::F);
    a.hallo();
    for (int id : {3, 9, 16}) {
        std::string d = "ABO " + std::to_string(id) + " 10\n";
        for (int i = 0; i < 3000; ++i) d += "arr/" + std::to_string(i % 64) + "\n";
        a.sende(d);
    }
    a.sende("ABO 1 20\nsim/x");
    std::map<int, int> runden;
    for (int i = 0; i < 300; ++i) {
        a.frame(1.0 / 30.0);
        if (i % 30 == 0) a.sende("PING");
        for (auto& j : a.neue_vom_typ("w")) {
            if (j.hole("teil")->zahl == j.hole("teile")->zahl) runden[static_cast<int>(j.hole("abo")->zahl)]++;
        }
    }
    for (int id : {1, 3, 9, 16}) {
        std::printf("     Abo %d: %d vollstaendige Lieferungen in 10 s\n", id, runden[id]);
        PRUEFE(runden[id] > 0);
    }
    PRUEFE(runden[1] > 150);  // Abo 1 behält Vorrang (Soll 200 in 10 s)
    // Keines der drei gleich schweren Abos bekommt weniger als die Hälfte des besten.
    const int maxi = std::max({runden[3], runden[9], runden[16]});
    for (int id : {3, 9, 16}) PRUEFE(runden[id] * 2 >= maxi);
}

TEST(dienst_generation_in_allen_antworten) {
    Aufbau a;
    a.welt.neu("sim/x", typ::F);
    a.hallo();
    a.sende("ABO 4 10 g42\nsim/x");
    a.frame();
    auto p = a.neue();
    PRUEFE_GLEICH(p.size(), size_t(3));  // abo_empfangen, abo, w
    PRUEFE_TEXT(a.umg.gesendet[a.umg.gesendet.size() - 3].second,
                "{\"p\":2,\"t\":\"abo_empfangen\",\"abo\":4,\"gen\":42,\"namen\":1}\n");
    for (auto& j : p) PRUEFE(j.hole("gen") && j.hole("gen")->zahl == 42);
    // Teile mit anderer Generation passen nicht zusammen.
    a.sende("ABO 5 10 1 2 g1\nsim/x");
    a.sende("ABO 5 10 2 2 g2\nsim/x");
    p = a.neue();
    PRUEFE_TEXT(p.back().hole("grund")->text, "abo_teile_widerspruch");
    PRUEFE(p.back().hole("gen")->zahl == 2);
    // Ohne g-Token: gen 0.
    a.sende("ABO 6 10\nsim/x");
    a.frame();
    for (auto& j : a.neue()) PRUEFE(j.hole("gen") && j.hole("gen")->zahl == 0);
}

TEST(dienst_n3_ungueltige_namen_einzeln_fehlt) {
    Aufbau a;
    a.welt.neu("sim/x", typ::F).f = 1.0f;
    a.welt.neu("sim/y", typ::F).f = 2.0f;
    a.hallo();
    a.sende("ABO 1 10\nsim/x\nkaputt]\nname mit leerzeichen\nsim/\xC3\xA4\nsim/y");
    a.frame();
    auto p = a.neue();
    auto st = status_aus(p);
    PRUEFE_GLEICH(st.size(), size_t(5));
    const char* soll[] = {"f,1", "fehlt", "fehlt", "fehlt", "f,1"};
    for (size_t k = 0; k < st.size() && k < 5; ++k) PRUEFE_TEXT(status_text(st[k]), soll[k]);
    auto w = werte_aus(p);
    PRUEFE(w.count(0) && w.count(4) && w[4].zahl == 2.0);
}

TEST(dienst_n1_verwaist_beim_lesen) {
    Aufbau a;
    auto& plug = a.welt.neu("toliss/apu", typ::I);
    plug.i = 1;
    a.welt.neu("sim/x", typ::F).f = 3.0f;
    a.hallo();
    a.sende("ABO 1 30\ntoliss/apu\nsim/x");
    a.frame();
    PRUEFE_TEXT(status_text(status_aus(a.neue())[0]), "i,1");
    // Plugin abgeschaltet, OHNE PLANE_LOADED: der nächste Lesezugriff merkt es.
    plug.gueltig = false;
    std::vector<JWert> p;
    for (int i = 0; i < 4; ++i) {
        a.frame(1.0 / 30.0);
        for (auto& j : a.neue()) p.push_back(j);
    }
    bool wert_nach_verwaisen = false, neue_antwort = false;
    for (auto& j : p) {
        if (art(j) == "abo") {
            neue_antwort = true;
            PRUEFE_TEXT(status_text(j.hole("st")->feld[0]), "fehlt");
        }
        if (art(j) == "w") {
            for (auto& e : j.hole("v")->feld) if (e.feld[0].zahl == 0) wert_nach_verwaisen = true;
        }
    }
    PRUEFE(!wert_nach_verwaisen);  // nie eine Schein-0
    PRUEFE(neue_antwort);           // Status sofort, nicht erst nach 2 s
    // sim/… wird nie geprüft (X-Plane-eigene Datarefs verwaisen nicht).
    const int vorher = a.welt.gueltig_pruefungen;
    a.frames(30);
    PRUEFE(a.welt.gueltig_pruefungen - vorher < 5);
}

TEST(dienst_n1_pause_nach_flugzeugwechsel) {
    Aufbau a;
    setze_xplm_kosten(a);
    // Neusuche über mehrere Frames, aber innerhalb der Pausen-Grenze
    // (grenzen::MAX_PAUSE_S); was danach gilt, prüft
    // dienst_pause_nach_flugzeugwechsel_begrenzt.
    a.welt.such_kosten = 3e-6;
    std::string plugin_abo = "ABO 2 30\n";
    for (int i = 0; i < 1000; ++i) {
        a.welt.neu("toliss/n" + std::to_string(i), typ::F).f = 1.0f;
        plugin_abo += "toliss/n" + std::to_string(i) + "\n";
    }
    a.welt.neu("sim/x", typ::F);
    a.hallo();
    a.sende(plugin_abo);
    a.sende("ABO 1 30\nsim/x");
    for (int i = 0; i < 60; ++i) a.frame(1.0 / 30.0);
    a.neue();
    // Flugzeugwechsel: toliss/* sind beim neuen Flugzeug nicht mehr da. Der
    // alte Handle meldet sich hier noch als gültig — die Prüfung beim Lesen
    // greift also NICHT; nur die Pause bis zur Neusuche verhindert Werte aus
    // alten Handles.
    // ⚠ Diesen Zustand (nicht registriert, Handle gültig) gibt es im echten
    // XPLM nicht: X-Plane zerstört Datarefs nie, und IsDataRefGood ist genau
    // dann wahr, wenn ein Plugin den Namen gerade bereitstellt (Cloud-QS
    // 29.09.2026). Der Test prüft die MECHANIK der Pause, kein reales Risiko.
    for (auto& r : a.welt.refs) if (r->name.rfind("toliss/", 0) == 0) r->registriert = false;
    a.d->flugzeug_geladen();
    bool abo2_wert_vor_status = false, abo2_status = false;
    int abo1_werte = 0, frames_bis_status = 0;
    for (int i = 0; i < 60 && !abo2_status; ++i) {
        a.frame(1.0 / 30.0);
        ++frames_bis_status;
        for (auto& j : a.neue()) {
            const int id = j.hole("abo") ? static_cast<int>(j.hole("abo")->zahl) : 0;
            if (art(j) == "w" && id == 1) ++abo1_werte;
            if (art(j) == "w" && id == 2 && !j.hole("v")->feld.empty()) abo2_wert_vor_status = true;
            if (art(j) == "abo" && id == 2) abo2_status = true;
        }
    }
    std::printf("     Neusuche 1000 Namen nach Flugzeugwechsel: %d Frames\n", frames_bis_status);
    PRUEFE(abo2_status);
    PRUEFE(!abo2_wert_vor_status);   // keine Werte aus alten Handles
    PRUEFE(frames_bis_status > 1);   // die Pause war wirklich nötig
    PRUEFE(abo1_werte >= frames_bis_status - 1);  // Telemetrie lief ohne Pause weiter
}

TEST(dienst_liste_ohne_index_namen) {
    Aufbau a;
    a.welt.neu("sim/a[3]", typ::F);
    a.welt.neu("sim/b", typ::F);
    a.hallo();
    a.sende("LISTE 1");
    std::set<std::string> namen;
    for (int i = 0; i < 10; ++i) {
        a.frame();
        for (auto& j : a.neue_vom_typ("liste")) for (auto& n : j.hole("n")->feld) namen.insert(n.text);
    }
    PRUEFE(namen.count("sim/b"));
    PRUEFE(!namen.count("sim/a[3]"));
}

TEST(dienst_pause_nach_flugzeugwechsel_begrenzt) {
    // Nachprüfung AP7: sechs Mess-Abos mit je 8192 Plugin-Namen bei 3 µs je
    // Suche brauchten ~16 s Neusuche — so lange lieferten sie vorher nichts.
    Aufbau a;
    setze_xplm_kosten(a);
    a.welt.such_kosten = 3e-6;
    // Abo 1: kleines Anzeige-Abo mit einem Plugin-Namen.
    auto& anzeige = a.welt.neu("toliss/anzeige", typ::F);
    anzeige.f = 7.0f;
    std::map<int, std::vector<std::string>> abos;
    abos[1] = abo_datagramme(1, 20, {"toliss/anzeige"}, 1);
    for (int id = 3; id <= 8; ++id) {
        abos[id] = abo_datagramme(id, 5, vermessungs_namen(a, "mess" + std::to_string(id) + "/n", 8192), 1);
    }
    a.hallo();
    const auto m = treibe(a, abos, 60.0);
    for (int id : {1, 3, 4, 5, 6, 7, 8}) PRUEFE(m.erster_status.count(id));
    a.frames(30, 1.0 / 30.0);
    a.neue();
    // Flugzeugwechsel: der Anzeige-Name ist beim neuen Flugzeug weg (Handle
    // meldet sich noch gültig — nur die Pause schützt). Wie oben: im echten
    // XPLM unmöglicher Zustand, geprüft wird die Mechanik der Pause.
    anzeige.registriert = false;
    a.d->flugzeug_geladen();
    std::map<int, double> letzte_lieferung;
    std::map<int, double> groesste_luecke;
    bool anzeige_wert_vor_status = false, anzeige_status = false;
    const double t0 = a.umg.zeit;
    for (int id = 3; id <= 8; ++id) letzte_lieferung[id] = t0;
    for (int i = 0; i < 150; ++i) {  // 5 s
        a.frame(1.0 / 30.0);
        if (i % 30 == 0) a.sende("PING");
        for (auto& j : a.neue()) {
            const int id = j.hole("abo") ? static_cast<int>(j.hole("abo")->zahl) : 0;
            if (id == 1 && art(j) == "abo") anzeige_status = true;
            if (id == 1 && art(j) == "w" && !anzeige_status) {
                for (auto& e : j.hole("v")->feld) if (e.feld[0].zahl == 0) anzeige_wert_vor_status = true;
            }
            if (id >= 3 && art(j) == "w" && j.hole("teil")->zahl == 1) {
                groesste_luecke[id] = std::max(groesste_luecke[id], a.umg.zeit - letzte_lieferung[id]);
                letzte_lieferung[id] = a.umg.zeit;
            }
        }
    }
    PRUEFE(anzeige_status);            // Abo 1 neu gesucht (kleinster Rest zuerst) …
    PRUEFE(!anzeige_wert_vor_status);  // … und bis dahin geschützt
    for (int id = 3; id <= 8; ++id) {
        std::printf("     Mess-Abo %d: groesste Luecke nach Flugzeugwechsel %.2f s\n", id, groesste_luecke[id]);
        PRUEFE(groesste_luecke[id] > 0.0 && groesste_luecke[id] < 1.0);
    }
}

// =============================================================================
// Codex-Abnahme (H1–H4, M1, M2)
// =============================================================================

namespace {

// Hat irgendein "w"-Paket in `pakete` einen Wert (auch in Arrays), der == x ist?
bool enthaelt_wert(const std::vector<JWert>& pakete, double x) {
    for (const auto& p : pakete) {
        if (art(p) != "w") continue;
        for (const auto& e : p.hole("v")->feld) {
            const JWert& w = e.feld[1];
            if (w.art == JWert::Art::ZAHL && w.zahl == x) return true;
            if (w.art == JWert::Art::FELD) {
                for (const auto& z : w.feld) if (z.art == JWert::Art::ZAHL && z.zahl == x) return true;
            }
        }
    }
    return false;
}

std::string log_mit(const Aufbau& a, const std::string& teil) {
    for (const auto& z : a.umg.log) if (z.find(teil) != std::string::npos) return z;
    return "";
}

int log_zaehle(const Aufbau& a, const std::string& teil) {
    int n = 0;
    for (const auto& z : a.umg.log) if (z.find(teil) != std::string::npos) ++n;
    return n;
}

}  // namespace

TEST(dienst_h2_flugzeugwechsel_mitten_im_lesen) {
    // Altes Flugzeug: alle Plugin-Werte 1.0; neues Flugzeug: 2.0. Lesen kostet
    // 100 µs je Wert → 1 ms Budget ≈ 10 Werte je Frame → eine Runde mit 400
    // Namen liest über ~40 Frames. Mitten darin: XPLM_MSG_PLANE_LOADED.
    Aufbau a;
    std::string abo = "ABO 2 1 g3\n";
    std::vector<ScheinRef*> refs;
    for (int i = 0; i < 400; ++i) {
        auto& r = a.welt.neu("toliss/w" + std::to_string(i), typ::F);
        r.f = 1.0f;
        refs.push_back(&r);
        abo += "toliss/w" + std::to_string(i) + "\n";
    }
    a.hallo();
    a.sende(abo);
    a.welt.lese_kosten = 1e-4;
    bool mitten = false;
    for (int i = 0; i < 200 && !mitten; ++i) {
        const int vorher = a.welt.lesezugriffe;
        a.frames(1);
        // Nach diesem Frame: Runde angefangen (Werte gelesen), noch nicht fertig.
        mitten = std::string(a.d->abo_phase(2)) == "lesen" && a.welt.lesezugriffe > vorher;
    }
    PRUEFE(mitten);
    PRUEFE_TEXT(a.d->abo_phase(2), "lesen");
    a.neue();
    for (auto* r : refs) r->f = 2.0f;
    a.d->flugzeug_geladen();
    PRUEFE_TEXT(a.d->abo_phase(2), "bereit");  // Runde verworfen
    std::vector<JWert> nachher;
    for (int i = 0; i < 300; ++i) {  // 5 s
        a.frames(1);
        for (auto& j : a.neue()) nachher.push_back(j);
        if (i % 60 == 0) a.sende("PING");
    }
    PRUEFE(!enthaelt_wert(nachher, 1.0));  // nie ein Wert des alten Flugzeugs
    PRUEFE(enthaelt_wert(nachher, 2.0));   // Lieferung läuft danach weiter
    // Das flugzeug-Paket kommt vor dem ersten neuen Wert.
    size_t erst_flugzeug = nachher.size(), erster_wert = nachher.size();
    for (size_t k = 0; k < nachher.size(); ++k) {
        if (art(nachher[k]) == "flugzeug" && erst_flugzeug == nachher.size()) erst_flugzeug = k;
        if (art(nachher[k]) == "w" && erster_wert == nachher.size()) erster_wert = k;
    }
    PRUEFE(erst_flugzeug < erster_wert);
}

TEST(dienst_h2_flugzeugwechsel_mitten_im_senden) {
    // 1500 Plugin-Arrays à 64 floats → eine Runde ≈ 26 Pakete; höchstens 16
    // je Frame, das Senden läuft also über mindestens zwei Frames.
    Aufbau a;
    std::vector<std::string> namen;
    std::vector<ScheinRef*> refs;
    for (int i = 0; i < 1500; ++i) {
        namen.push_back("addon/arr" + std::to_string(i));
        auto& r = a.welt.neu(namen.back(), typ::VF);
        r.vf = std::vector<float>(64, 1.0f);
        refs.push_back(&r);
    }
    a.hallo();
    for (const auto& d : abo_datagramme(5, 1, namen, 1)) a.sende(d);
    int teile_vorher = 0, seq_unterbrochen = -1;
    for (int i = 0; i < 400; ++i) {
        a.frames(1);
        for (auto& j : a.neue_vom_typ("w")) {
            seq_unterbrochen = static_cast<int>(j.hole("seq")->zahl);
            teile_vorher = static_cast<int>(j.hole("teil")->zahl);
        }
        if (std::string(a.d->abo_phase(5)) == "senden" && teile_vorher > 0) break;
    }
    PRUEFE_TEXT(a.d->abo_phase(5), "senden");
    PRUEFE(teile_vorher > 0);
    for (auto* r : refs) r->vf.assign(64, 2.0f);
    a.d->flugzeug_geladen();
    std::vector<JWert> nachher;
    for (int i = 0; i < 300; ++i) {
        a.frames(1);
        for (auto& j : a.neue()) nachher.push_back(j);
        if (i % 60 == 0) a.sende("PING");
    }
    PRUEFE(!enthaelt_wert(nachher, 1.0));  // kein Rest der alten Runde
    PRUEFE(enthaelt_wert(nachher, 2.0));
    for (const auto& j : nachher) {
        if (art(j) == "w") PRUEFE(static_cast<int>(j.hole("seq")->zahl) != seq_unterbrochen);
    }
}

TEST(dienst_h2_flugzeugwechsel_mitten_in_der_statusantwort) {
    // Status von 8192 Namen ≈ 15 Pakete. Nach 5 Paketen ist der Socket voll
    // (ok_noch) — die Status-Antwort steckt mitten im Senden, als das
    // Flugzeug wechselt. Danach: kein Wert, bis ein VOLLSTÄNDIGER neuer
    // Status draußen ist (die ersten 5 Teile allein zählen nicht).
    Aufbau a;
    for (int i = 0; i < 64; ++i) a.welt.neu("addon/a" + std::to_string(i), typ::VF).vf = std::vector<float>(64, 1.0f);
    std::vector<std::string> namen;
    for (int i = 0; i < 8192; ++i) namen.push_back("addon/a" + std::to_string(i % 64));
    a.hallo();
    for (const auto& d : abo_datagramme(6, 1, namen, 9)) a.sende(d);
    a.neue();
    a.umg.ok_noch = 5;
    a.frames(1);
    PRUEFE_TEXT(a.d->abo_phase(6), "antwort");
    const auto teil_status = a.neue_vom_typ("abo");
    PRUEFE_GLEICH(teil_status.size(), size_t(5));
    a.d->flugzeug_geladen();
    PRUEFE_TEXT(a.d->abo_phase(6), "neu");
    a.umg.ok_noch = -1;
    std::vector<JWert> p;
    for (int i = 0; i < 200; ++i) {
        a.frames(1);
        for (auto& j : a.neue()) p.push_back(j);
    }
    // Danach: ein vollständiger Status (Teile 1..n lückenlos, gleiches
    // "teile"), und kein einziger Wert vor seinem letzten Teil.
    std::set<int> teile;
    int teile_soll = 0;
    bool w_vor_status = false, teile_gemischt = false;
    for (const auto& j : p) {
        if (art(j) == "abo" && (teile_soll == 0 || static_cast<int>(teile.size()) < teile_soll)) {
            const int t = static_cast<int>(j.hole("teile")->zahl);
            if (teile_soll != 0 && t != teile_soll) teile_gemischt = true;
            teile_soll = t;
            teile.insert(static_cast<int>(j.hole("teil")->zahl));
        }
        if (art(j) == "w" && (teile_soll == 0 || static_cast<int>(teile.size()) < teile_soll)) w_vor_status = true;
    }
    PRUEFE(teile_soll > 5);
    PRUEFE_GLEICH(static_cast<int>(teile.size()), teile_soll);
    PRUEFE(!teile_gemischt);
    PRUEFE(!w_vor_status);
    PRUEFE(!werte_aus(p).empty());  // danach wird wieder geliefert
}

TEST(dienst_h1_mehrteiliger_status_und_vollstaendige_wiederholung) {
    // Der Client setzt mehrteilige Status-Antworten zusammen und fordert bei
    // einer Lücke per identischem ABO nach (Client-Seite von H1). Plugin-
    // Seite: teil/teile konsistent, und das Neu-ABO liefert ALLE Teile erneut.
    Aufbau a;
    std::vector<std::string> namen;
    for (int i = 0; i < 4000; ++i) {
        namen.push_back("sim/h1/n" + std::to_string(i));
        if (i % 3) a.welt.neu(namen.back(), typ::F);
    }
    a.hallo();
    const auto datagramme = abo_datagramme(7, 1, namen, 4);
    for (const auto& d : datagramme) a.sende(d);
    auto sammle_status = [&a]() {
        std::map<int, std::vector<JWert>> teile;
        int teile_soll = -1;
        bool konsistent = true;
        for (int i = 0; i < 60; ++i) {
            a.frames(1);
            for (auto& j : a.neue_vom_typ("abo")) {
                const int t = static_cast<int>(j.hole("teile")->zahl);
                if (teile_soll >= 0 && t != teile_soll) konsistent = false;
                teile_soll = t;
                if (j.hole("gen")->zahl != 4 || j.hole("abo")->zahl != 7) konsistent = false;
                for (auto& e : j.hole("st")->feld) teile[static_cast<int>(j.hole("teil")->zahl)].push_back(e);
            }
        }
        PRUEFE(konsistent);
        std::vector<JWert> st;
        for (int k = 1; k <= teile_soll; ++k) {
            PRUEFE(teile.count(k));
            for (auto& e : teile[k]) st.push_back(e);
        }
        PRUEFE_GLEICH(static_cast<int>(teile.size()), teile_soll);
        return std::make_pair(teile_soll, st);
    };
    const auto erst = sammle_status();
    PRUEFE(erst.first > 1);
    PRUEFE_GLEICH(erst.second.size(), size_t(4000));
    for (const auto& d : datagramme) a.sende(d);  // identisch: Nachforderung
    const auto noch = sammle_status();
    PRUEFE_GLEICH(noch.first, erst.first);
    PRUEFE_GLEICH(noch.second.size(), size_t(4000));
    for (size_t k = 0; k < noch.second.size() && k < erst.second.size(); ++k) {
        PRUEFE(noch.second[k].feld[0].zahl == static_cast<double>(k));
        PRUEFE_TEXT(status_text(noch.second[k]), status_text(erst.second[k]));
    }
}

TEST(dienst_h3_speicher_limit_je_abo) {
    // 8192-mal derselbe 1024-Byte-Dataref: je Eintrag 6153 Byte Werte-Reserve
    // → ≈ 48 MiB. Vorher reserviert, jetzt fehler/speicher_limit.
    Aufbau a;
    a.welt.neu("addon/text", typ::B).b = std::string(1024, 'x');
    a.welt.neu("sim/x", typ::F).f = 1.0f;
    std::vector<std::string> namen(8192, "addon/text");
    a.hallo();
    a.sende("ABO 1 10\nsim/x");
    for (const auto& d : abo_datagramme(3, 1, namen, 11)) a.sende(d);
    std::vector<JWert> p;
    for (int i = 0; i < 60; ++i) {
        a.frames(1);
        for (auto& j : a.neue()) p.push_back(j);
    }
    bool limit = false;
    for (const auto& j : p) {
        if (art(j) == "fehler" && j.hole("grund")->text == "speicher_limit") {
            limit = true;
            PRUEFE(j.hole("abo")->zahl == 3);
            PRUEFE(j.hole("gen")->zahl == 11);
        }
        if (art(j) == "w") PRUEFE(j.hole("abo")->zahl != 3);
    }
    PRUEFE(limit);
    PRUEFE_TEXT(a.d->abo_phase(3), "leer");
    PRUEFE(a.d->speicher_abos() < (size_t(1) << 20));   // wieder frei
    PRUEFE(!log_mit(a, "speicher_limit").empty());
    PRUEFE(!werte_aus(p).empty());                        // Abo 1 unberührt
    // Dasselbe mit 8192 × ganzem 256er-Array (≈ 34 MiB).
    a.welt.neu("addon/arr", typ::VF).vf = std::vector<float>(300, 1.0f);
    std::vector<std::string> arr(8192, "addon/arr");
    for (const auto& d : abo_datagramme(4, 1, arr, 1)) a.sende(d);
    limit = false;
    for (int i = 0; i < 60; ++i) {
        a.frames(1);
        for (auto& j : a.neue()) {
            if (art(j) == "fehler" && j.hole("grund")->text == "speicher_limit" && j.hole("abo")->zahl == 4) limit = true;
        }
    }
    PRUEFE(limit);
    PRUEFE(a.d->speicher_abos() <= grenzen::MAX_BYTES_ABOS);
}

TEST(dienst_h3_speicher_limit_gesamt) {
    // Je Abo 3400 ganze 256er-Arrays ≈ 15 MiB (unter 16 MiB je Abo). Vier
    // passen in 64 MiB, das fünfte nicht — die ersten vier bleiben unberührt.
    Aufbau a;
    a.welt.neu("addon/arr", typ::VF).vf = std::vector<float>(256, 0.5f);
    std::vector<std::string> namen(3400, "addon/arr");
    a.hallo();
    std::map<int, int> w_je_abo;
    std::map<int, std::string> fehler;
    auto laufen = [&](int frames) {
        for (int i = 0; i < frames; ++i) {
            a.frames(1);
            for (auto& j : a.neue()) {
                const int id = j.hole("abo") ? static_cast<int>(j.hole("abo")->zahl) : 0;
                if (art(j) == "w") w_je_abo[id]++;
                if (art(j) == "fehler") fehler[id] = j.hole("grund")->text;
            }
        }
    };
    for (int id = 2; id <= 5; ++id) {
        for (const auto& d : abo_datagramme(id, 1, namen, 1)) a.sende(d);
        laufen(30);
    }
    PRUEFE(fehler.empty());
    PRUEFE(a.d->speicher_abos() <= grenzen::MAX_BYTES_ABOS);
    std::printf("     4 Abos x 3400 ganze 256er-Arrays: %.1f MiB belegt\n",
                static_cast<double>(a.d->speicher_abos()) / (1 << 20));
    for (const auto& d : abo_datagramme(6, 1, namen, 1)) a.sende(d);
    w_je_abo.clear();
    laufen(120);
    PRUEFE(fehler.count(6) && fehler[6] == "speicher_limit");
    PRUEFE_TEXT(a.d->abo_phase(6), "leer");
    for (int id = 2; id <= 5; ++id) {
        PRUEFE(!fehler.count(id));
        PRUEFE(w_je_abo[id] > 0);
    }
    PRUEFE(a.d->speicher_abos() <= grenzen::MAX_BYTES_ABOS);
}

TEST(dienst_h3_legitime_vermessung_passt) {
    // Größter legitimer Fall beim Client: Abo 1 (≈ 200 Namen) plus Vermessung
    // mit 14 Abos (3–16) zu je 8192 Namen — 60 % Skalare, 35 % kleine Arrays
    // (8–24), 5 % lange Arrays (300 → 256). Nichts darf am Budget scheitern.
    Aufbau a;
    std::map<int, std::vector<std::string>> abos;
    std::vector<std::string> tele;
    for (int i = 0; i < 200; ++i) {
        tele.push_back("sim/tele/n" + std::to_string(i));
        a.welt.neu(tele.back(), typ::D).d = i;
    }
    abos[1] = abo_datagramme(1, 20, tele, 1);
    int k = 0;
    for (int id = 3; id <= 16; ++id) {
        std::vector<std::string> namen;
        for (int i = 0; i < 8192; ++i, ++k) {
            namen.push_back("mess/" + std::to_string(id) + "/dataref_mit_typischem_namen_" + std::to_string(i));
            auto& r = a.welt.neu(namen.back(), 0);
            const int art_nr = k % 20;
            if (art_nr == 0) { r.typen = typ::VF; r.vf = std::vector<float>(300, 0.25f); }
            else if (art_nr < 8) { r.typen = typ::VF; r.vf = std::vector<float>(8 + (k % 17), 1.5f); }
            else { r.typen = (k % 2) ? typ::F : typ::I; r.f = 3.0f; r.i = 3; }
        }
        abos[id] = abo_datagramme(id, 1, namen, 1);
    }
    a.hallo();
    const auto m = treibe(a, abos, 60.0);
    bool fehler = false;
    for (auto& j : a.neue()) if (art(j) == "fehler") fehler = true;
    PRUEFE(!fehler);
    PRUEFE(log_mit(a, "speicher").empty());
    for (int id : {1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16}) PRUEFE(m.erster_status.count(id));
    std::printf("     Abo 1 + 14 x 8192 Mess-Namen: %.1f MiB belegt (Budget %zu MiB)\n",
                static_cast<double>(a.d->speicher_abos()) / (1 << 20), grenzen::MAX_BYTES_ABOS >> 20);
    PRUEFE(a.d->speicher_abos() <= grenzen::MAX_BYTES_ABOS);
}

TEST(dienst_h3_liste_speicher_limit) {
    // 40 000 Namen à ≈ 60 Byte passen (≈ 2,5 MiB); 36 000 Namen à 510 Byte
    // (≈ 18 MiB) nicht → fehler/speicher_limit mit id, Speicher wieder frei.
    Aufbau a;
    for (int i = 0; i < 40000; ++i) a.welt.neu("sim/liste/ein_typischer_dataref_name_" + std::to_string(i), typ::F);
    // 1 µs je Name → ≈ 1000 Namen je Frame: das Sammeln läuft über viele
    // Frames, die Prüfung nach jedem Frame sieht also auch den Höchststand.
    a.welt.namen_kosten = 1e-6;
    a.hallo();
    a.sende("LISTE 5");
    int teile = 0, gesehen = 0;
    for (int i = 0; i < 2000 && (teile == 0 || gesehen < teile); ++i) {
        a.frames(1);
        for (auto& j : a.neue_vom_typ("liste")) { teile = static_cast<int>(j.hole("teile")->zahl); ++gesehen; }
        PRUEFE(a.d->speicher_liste() <= grenzen::MAX_BYTES_LISTE);
    }
    PRUEFE(teile > 0 && gesehen == teile);
    for (int i = 0; i < 36000; ++i) {
        std::string n = "addon/" + std::to_string(i) + "/";
        n += std::string(510 - n.size(), 'q');
        a.welt.neu(n, typ::F);
    }
    a.sende("LISTE 6");
    bool limit = false;
    for (int i = 0; i < 3000 && !limit; ++i) {
        a.frames(1);
        PRUEFE(a.d->speicher_liste() <= grenzen::MAX_BYTES_LISTE);
        for (auto& j : a.neue()) {
            if (art(j) == "fehler" && j.hole("grund")->text == "speicher_limit") {
                limit = true;
                PRUEFE(j.hole("id")->zahl == 6);
            }
        }
    }
    PRUEFE(limit);
    PRUEFE(!a.d->liste_laeuft());
    PRUEFE_GLEICH(a.d->speicher_liste(), size_t(0));
}

TEST(dienst_h4_teurer_accessor_wird_gedrosselt) {
    // 50 billige Plugin-Namen und einer, dessen Getter 5 ms braucht. Rate 20.
    Aufbau a;
    std::string abo = "ABO 2 20\n";
    for (int i = 0; i < 50; ++i) {
        a.welt.neu("addon/n" + std::to_string(i), typ::F).f = 1.0f;
        abo += "addon/n" + std::to_string(i) + "\n";
    }
    auto& langsam = a.welt.neu("addon/langsam", typ::F);
    langsam.f = 7.0f;
    langsam.kosten = 0.005;
    abo += "addon/langsam\n";
    a.hallo();
    a.sende(abo);
    double max_frame = 0.0;
    int w_pakete = 0, langsam_werte = 0;
    int getter_ab_2s = -1;
    bool fehlt = false;
    for (int i = 0; i < 600; ++i) {  // 10 s bei 60 fps
        if (i % 60 == 0) a.sende("PING");
        if (i == 120) getter_ab_2s = langsam.getter_aufrufe;
        const double vorher = a.umg.zeit + 1.0 / 60.0;
        a.frame();
        max_frame = std::max(max_frame, a.umg.zeit - vorher);
        for (auto& j : a.neue()) {
            if (art(j) == "abo") {
                for (auto& e : j.hole("st")->feld) if (e.feld[0].zahl == 50 && e.feld[1].text == "fehlt") fehlt = true;
            }
            if (art(j) != "w") continue;
            ++w_pakete;
            for (auto& e : j.hole("v")->feld) if (e.feld[0].zahl == 50) ++langsam_werte;
        }
    }
    const int getter_8s = langsam.getter_aufrufe - getter_ab_2s;
    std::printf("     5-ms-Getter: %d Aufrufe in 8 s (gedrosselt), laengster Frame %.2f ms, %d Lieferungen\n",
                getter_8s, max_frame * 1000.0, w_pakete);
    // Nie zwei langsame Aufrufe in einem Frame: ≤ 1 ms Budget + ein fremder Aufruf.
    PRUEFE(max_frame <= grenzen::ZEITBUDGET_S + grenzen::SUCH_BUDGET_S + 0.005 + 1e-4);
    // Gedrosselt: höchstens einmal je Sekunde (Soll 8, etwas Spiel).
    PRUEFE(getter_8s <= 10);
    PRUEFE(langsam_werte >= 5);                 // Wert kommt weiter, nur seltener
    PRUEFE(!fehlt);                             // Status bleibt wahr (nicht "fehlt")
    PRUEFE(w_pakete >= 190);                    // die anderen Namen: ~20 Hz
    PRUEFE_GLEICH(log_zaehle(a, "addon/langsam antwortet langsam"), 1);  // einmal im Log
    // Neues Flugzeug: Drosselung neu bewerten (der Name gehört evtl. einem
    // anderen Plugin) — nach drei langsamen Aufrufen wieder gedrosselt.
    langsam.kosten = 0.0;
    a.d->flugzeug_geladen();
    const int vor = langsam.getter_aufrufe;
    for (int i = 0; i < 120; ++i) {
        if (i % 60 == 0) a.sende("PING");
        a.frame();
    }
    PRUEFE(langsam.getter_aufrufe - vor >= 30);  // wieder volle Rate (Soll 40)
}

TEST(dienst_h4_einzelner_ausreisser_drosselt_nicht) {
    // Drei Mal je zwei langsame Aufrufe, dazwischen schnelle: sechs langsame
    // insgesamt, aber nie drei IN FOLGE → keine Drosselung. (Ohne die
    // Folge-Regel würde der dritte Ausreißer irgendwann drosseln.)
    Aufbau a;
    auto& r = a.welt.neu("addon/zucker", typ::F);
    a.hallo();
    a.sende("ABO 2 20\naddon/zucker");
    a.frames(10);
    for (int zyklus = 0; zyklus < 3; ++zyklus) {
        r.kosten = 0.005;
        const int start = r.getter_aufrufe;
        for (int i = 0; i < 60 && r.getter_aufrufe - start < 2; ++i) a.frames(1);
        r.kosten = 0.0;
        a.frames(30);
    }
    const int vor = r.getter_aufrufe;
    a.frames(120);
    PRUEFE(log_mit(a, "antwortet langsam").empty());
    PRUEFE(r.getter_aufrufe - vor >= 30);  // volle Rate
}

TEST(dienst_h4_uhr_nach_jedem_aufruf_bei_der_suche) {
    // Suche: finde kostet 0,2 ms, die Array-Länge (fremder Accessor) 0,05 ms.
    // Vorher lief nach einer Uhrprüfung die GANZE zweite Einheit (0,5 ms statt
    // 0,3 ms Budget); jetzt endet die Suche vor dem nächsten Aufruf.
    Aufbau a;
    std::string abo = "ABO 2 1\n";
    for (int i = 0; i < 40; ++i) {
        auto& r = a.welt.neu("addon/arr" + std::to_string(i), typ::VF);
        r.vf = std::vector<float>(4, 1.0f);
        r.kosten = 0.00005;
        abo += "addon/arr" + std::to_string(i) + "\n";
    }
    a.welt.such_kosten = 0.0002;
    a.hallo();
    a.sende(abo);
    int max_laengen = 0, max_suchen = 0, frames = 0;
    bool status = false;
    while (!status && frames < 200) {
        int l = 0;
        int laengen_vorher = 0;
        for (auto& r : a.welt.refs) laengen_vorher += r->laengen_aufrufe;
        const int suchen_vorher = a.welt.suchen;
        a.frames(1);
        ++frames;
        for (auto& r : a.welt.refs) l += r->laengen_aufrufe;
        max_laengen = std::max(max_laengen, l - laengen_vorher);
        max_suchen = std::max(max_suchen, a.welt.suchen - suchen_vorher);
        status = !a.neue_vom_typ("abo").empty();
    }
    std::printf("     Suche: hoechstens %d finde + %d Laengen je Frame, Status nach %d Frames\n",
                max_suchen, max_laengen, frames);
    PRUEFE(status);
    PRUEFE(max_laengen <= 1);  // nur die freie Einheit erreicht ihren fremden Aufruf
    PRUEFE(max_suchen <= 2);   // die zweite Einheit endet nach finde
}

TEST(dienst_h4_liste_je_name_budgetiert) {
    Aufbau a;
    for (int i = 0; i < 3000; ++i) a.welt.neu("sim/l/n" + std::to_string(i), typ::F);
    a.welt.namen_kosten = 0.00005;  // 50 µs je XPLMGetDataRefInfo
    a.hallo();
    a.sende("LISTE 9");
    int max_namen = 0, teile = 0, gesehen = 0;
    for (int i = 0; i < 1000 && (teile == 0 || gesehen < teile); ++i) {
        const int vorher = a.welt.namen_abfragen;
        a.frames(1);
        max_namen = std::max(max_namen, a.welt.namen_abfragen - vorher);
        for (auto& j : a.neue_vom_typ("liste")) { teile = static_cast<int>(j.hole("teile")->zahl); ++gesehen; }
    }
    std::printf("     LISTE: hoechstens %d Namen je Frame (vorher bis 256)\n", max_namen);
    PRUEFE(teile > 0 && gesehen == teile);
    PRUEFE(max_namen <= 22);  // 1 ms / 50 µs = 20, plus freie Einheit
}

TEST(dienst_m1_gemeinsamer_ausgang) {
    // Eine Runde ≈ 6000 × 25 Byte ≈ 19 Pakete — mehr, als ein Frame darf. Die
    // Lieferung will also jeden Frame alle 16; zusammen mit 8 pong wären es
    // 24, wenn die Antworten beim Empfang nicht mitzählten.
    Aufbau a;
    std::vector<std::string> namen;
    for (int i = 0; i < 6000; ++i) {
        namen.push_back("sim/m1/n" + std::to_string(i));
        a.welt.neu(namen.back(), typ::D).d = i + 0.12345678901234;
    }
    a.hallo();
    for (const auto& d : abo_datagramme(2, 50, namen, 1)) a.sende(d);
    a.frames(5);
    a.neue();
    int max_je_frame = 0, max_pong = 0, w_gesamt = 0;
    for (int f = 0; f < 120; ++f) {
        const size_t vorher = a.umg.gesendet.size();
        // PING-Flut: 60 PINGs + 4 kaputte Anfragen je Frame (64 = Empfangsgrenze).
        for (int k = 0; k < 60; ++k) a.sende("PING");
        for (int k = 0; k < 4; ++k) a.sende("QUATSCH", ANDERER);
        if (f % 20 == 0) a.d->flugzeug_geladen();
        a.frame();
        max_je_frame = std::max(max_je_frame, static_cast<int>(a.umg.gesendet.size() - vorher));
        int pong = 0;
        for (auto& j : a.neue()) {
            if (art(j) == "pong") ++pong;
            if (art(j) == "w") ++w_gesamt;
        }
        max_pong = std::max(max_pong, pong);
    }
    std::printf("     Flut: hoechstens %d Pakete je Frame, davon %d pong; %d Werte-Pakete\n",
                max_je_frame, max_pong, w_gesamt);
    PRUEFE(max_je_frame <= grenzen::MAX_PAKETE_JE_FRAME);
    PRUEFE(max_pong <= grenzen::MAX_KLEINE_JE_FRAME);
    PRUEFE(max_pong >= 1);         // Lebenszeichen kommt trotzdem
    // ≥ 8 Pakete je Frame bleiben der Lieferung; im Mittel etwas weniger, weil
    // der Frame mit dem Rest einer Runde keine neue beginnt (gemessen 6,7).
    PRUEFE(w_gesamt >= 120 * 6);
    PRUEFE(!log_mit(a, "kleine Antworten verworfen").empty());
}

TEST(dienst_m2_liste_verhungert_nicht) {
    // Drei Dauer-Abos (je 3000 Arrays, 50 Hz), jedes Lesen 2 µs → eine Runde
    // braucht 6 ms, jedes Budget ist jeden Frame erschöpft. Vorher kam LISTE
    // nie dran; jetzt ist sie im Rundlauf jeden vierten Frame als erste dran.
    Aufbau a;
    for (int i = 0; i < 64; ++i) a.welt.neu("arr/" + std::to_string(i), typ::VF).vf = std::vector<float>(16, 1.0f);
    for (int i = 0; i < 3000; ++i) a.welt.neu("sim/liste/n" + std::to_string(i), typ::F);
    a.welt.lese_kosten = 2e-6;
    a.hallo();
    for (int id : {3, 9, 16}) {
        std::string d = "ABO " + std::to_string(id) + " 50\n";
        for (int i = 0; i < 3000; ++i) d += "arr/" + std::to_string(i % 64) + "\n";
        a.sende(d);
    }
    a.frames(30);
    a.sende("LISTE 77");
    int teile = 0, gesehen = 0, frames = 0;
    std::map<int, int> w;
    for (; frames < 600 && (teile == 0 || gesehen < teile); ++frames) {
        a.frames(1);
        for (auto& j : a.neue()) {
            if (art(j) == "liste") { teile = static_cast<int>(j.hole("teile")->zahl); ++gesehen; }
            if (art(j) == "w") w[static_cast<int>(j.hole("abo")->zahl)]++;
        }
    }
    std::printf("     LISTE neben drei Dauer-Abos: fertig nach %d Frames\n", frames);
    PRUEFE(teile > 0 && gesehen == teile);
    for (int id : {3, 9, 16}) PRUEFE(w[id] > 0);
}

// =============================================================================
// Nachprüfung der Codex-Abnahme (Drosselung je Dataref, ungebudgetierte Wege,
// Abo-1-Teilbudget, vorgemerkte Fehler)
// =============================================================================

TEST(dienst_h4_drosselung_je_dataref_auch_bei_duplikaten) {
    // Derselbe langsame Dataref 2000× in Abo 2 und 500× in Abo 3 (dazu 20
    // billige Namen). Vorher hatte jeder EINTRAG seinen eigenen Zähler:
    // 2500 Einträge × 3 Treffer — die Drosselung griff praktisch nie.
    Aufbau a;
    auto& langsam = a.welt.neu("addon/langsam", typ::F);
    langsam.f = 7.0f;
    langsam.kosten = 0.005;
    std::vector<std::string> n2(2000, "addon/langsam"), n3(500, "addon/langsam");
    for (int i = 0; i < 20; ++i) {
        n3.push_back("addon/billig" + std::to_string(i));
        a.welt.neu(n3.back(), typ::F).f = 1.0f;
    }
    a.hallo();
    for (const auto& d : abo_datagramme(2, 5, n2, 1)) a.sende(d);
    for (const auto& d : abo_datagramme(3, 20, n3, 1)) a.sende(d);
    int getter_ab_2s = 0, w3 = 0;
    double max_frame = 0.0;
    for (int i = 0; i < 600; ++i) {
        if (i % 60 == 0) a.sende("PING");
        if (i == 120) getter_ab_2s = langsam.getter_aufrufe;
        const double vorher = a.umg.zeit + 1.0 / 60.0;
        a.frame();
        max_frame = std::max(max_frame, a.umg.zeit - vorher);
        for (auto& j : a.neue_vom_typ("w")) if (j.hole("abo")->zahl == 3) ++w3;
    }
    const int getter_8s = langsam.getter_aufrufe - getter_ab_2s;
    std::printf("     2500 Duplikate eines 5-ms-Getters in 2 Abos: %d Aufrufe in 8 s, laengster Frame %.2f ms\n",
                getter_8s, max_frame * 1000.0);
    PRUEFE(getter_8s <= 10);   // gemeinsam gedrosselt: ≈ einmal je Sekunde
    PRUEFE(max_frame <= grenzen::ZEITBUDGET_S + grenzen::SUCH_BUDGET_S + 0.005 + 1e-4);
    PRUEFE_GLEICH(log_zaehle(a, "antwortet langsam"), 1);  // einmal je Dataref
    PRUEFE(w3 >= 100);          // Abo 3 liefert weiter (Soll ≈ 200)
}

TEST(dienst_h4_drosseltabelle_voll_und_ausreisser_raeumen) {
    // 800 verschiedene langsame Datarefs: die Tabelle verfolgt höchstens 768,
    // meldet das einmal und läuft stabil weiter. Danach werden alle schnell:
    // ein schneller Aufruf räumt die (noch nicht gedrosselten) Plätze.
    Aufbau a;
    std::vector<std::string> namen;
    std::vector<ScheinRef*> refs;
    for (int i = 0; i < 800; ++i) {
        namen.push_back("addon/l" + std::to_string(i));
        auto& r = a.welt.neu(namen.back(), typ::F);
        r.kosten = 0.003;
        refs.push_back(&r);
    }
    a.hallo();
    for (const auto& d : abo_datagramme(2, 50, namen, 1)) a.sende(d);
    a.frames(4000);
    PRUEFE_GLEICH(log_zaehle(a, "Drosseltabelle voll"), 1);
    PRUEFE_GLEICH(log_zaehle(a, "antwortet langsam"), grenzen::MAX_LANGSAM_MELDUNGEN);
    for (auto* r : refs) r->kosten = 0.0;
    a.d->flugzeug_geladen();  // leert die Tabelle
    int vor = 0;
    for (auto* r : refs) vor += r->getter_aufrufe;
    a.frames(120);
    int nach = 0;
    for (auto* r : refs) nach += r->getter_aufrufe;
    PRUEFE(nach - vor >= 800 * 50);  // wieder volle Rate für alle
}

TEST(dienst_h4_empfang_hat_ein_zeitbudget) {
    Aufbau a;
    a.hallo();
    a.umg.tick_je_uhrabfrage = 0.0002;  // jede Uhrabfrage 0,2 ms
    a.d->empfang_beginnen();
    int n = 0;
    while (n < grenzen::MAX_DATAGRAMME_JE_FRAME && a.d->empfang_weiter()) {
        a.sende("PING");
        ++n;
    }
    std::printf("     Empfang mit 0,2 ms je Uhrabfrage: %d Datagramme in diesem Frame\n", n);
    PRUEFE(n >= 1 && n <= 4);
    a.umg.tick_je_uhrabfrage = 0.0;
    a.frame();
    a.d->empfang_beginnen();
    n = 0;
    while (n < grenzen::MAX_DATAGRAMME_JE_FRAME && a.d->empfang_weiter()) { a.sende("PING"); ++n; }
    PRUEFE_GLEICH(n, grenzen::MAX_DATAGRAMME_JE_FRAME);
}

TEST(dienst_h4_liste_zaehlt_erst_im_budget) {
    Aufbau a;
    for (int i = 0; i < 100; ++i) a.welt.neu("sim/z/n" + std::to_string(i), typ::F);
    a.hallo();
    a.sende("LISTE 3");
    PRUEFE_GLEICH(a.welt.zaehlungen, 0);  // nicht beim Empfang
    a.frame();
    PRUEFE_GLEICH(a.welt.zaehlungen, 1);
    int teile = 0, gesehen = 0;
    for (int i = 0; i < 20 && (teile == 0 || gesehen < teile); ++i) {
        a.frame();
        for (auto& j : a.neue_vom_typ("liste")) { teile = static_cast<int>(j.hole("teile")->zahl); ++gesehen; }
    }
    PRUEFE(teile > 0 && gesehen == teile);
}

TEST(dienst_h4_flugzeugkennung_ist_budgetiert) {
    // Jede der drei Kennungen kostet 0,4 ms (> Such-Budget 0,3 ms). Vorher
    // lasen alle drei vor jedem Budget im selben Frame; jetzt eine je Frame.
    Aufbau a;
    for (const char* n : {"sim/aircraft/view/acf_ICAO", "sim/aircraft/view/acf_descrip",
                          "sim/aircraft/view/acf_relative_path"}) {
        a.welt.such(n)->kosten = 0.0004;
    }
    a.sende("HALLO 2 test");
    a.neue();
    int max_je_frame = 0, frames = 0;
    std::vector<JWert> f;
    while (f.empty() && frames < 20) {
        int vorher = 0;
        for (auto& r : a.welt.refs) vorher += r->getter_aufrufe;
        a.frame();
        ++frames;
        int nachher = 0;
        for (auto& r : a.welt.refs) nachher += r->getter_aufrufe;
        max_je_frame = std::max(max_je_frame, nachher - vorher);
        f = a.neue_vom_typ("flugzeug");
    }
    PRUEFE(!f.empty());
    PRUEFE(max_je_frame <= 1);
    PRUEFE(frames >= 3 && frames <= 5);
    if (!f.empty()) {
        PRUEFE_TEXT(f[0].hole("icao")->text, "A20N");
        PRUEFE_TEXT(f[0].hole("pfad")->text, "Aircraft/A320/a320.acf");
    }
}

TEST(dienst_m2_grosses_abo1_hungert_niemanden_aus) {
    // Abo 1: 3000 Arrays, 50 Hz, 2 µs je Lesen → 6 ms je Runde, jeden Frame
    // fällig. Vorher lief Abo 1 immer vor dem Rundlauf mit dem ganzen Budget
    // — LISTE und Mess-Abo bekamen nie etwas. Jetzt: Teilbudget für Abo 1,
    // der Rundlauf bekommt den Rest samt freier Einheit.
    Aufbau a;
    for (int i = 0; i < 64; ++i) a.welt.neu("arr/" + std::to_string(i), typ::VF).vf = std::vector<float>(16, 1.0f);
    std::vector<std::string> mess;
    for (int i = 0; i < 300; ++i) {
        mess.push_back("mess/n" + std::to_string(i));
        a.welt.neu(mess.back(), typ::F).f = 2.0f;
    }
    for (int i = 0; i < 3000; ++i) a.welt.neu("sim/liste/x" + std::to_string(i), typ::F);
    a.welt.lese_kosten = 2e-6;
    a.hallo();
    std::string d = "ABO 1 50\n";
    for (int i = 0; i < 3000; ++i) d += "arr/" + std::to_string(i % 64) + "\n";
    a.sende(d);
    for (const auto& x : abo_datagramme(3, 5, mess, 1)) a.sende(x);
    a.frames(30);
    a.sende("LISTE 12");
    int teile = 0, gesehen = 0, frames = 0;
    std::map<int, int> w;
    for (; frames < 900 && (teile == 0 || gesehen < teile || w[3] < 5); ++frames) {
        a.frames(1);
        for (auto& j : a.neue()) {
            if (art(j) == "liste") { teile = static_cast<int>(j.hole("teile")->zahl); ++gesehen; }
            if (art(j) == "w") w[static_cast<int>(j.hole("abo")->zahl)]++;
        }
    }
    std::printf("     Abo 1 dauerlastig: LISTE nach %d Frames fertig, Abo 3 %d, Abo 1 %d Pakete\n",
                frames, w[3], w[1]);
    PRUEFE(teile > 0 && gesehen == teile);
    PRUEFE(w[3] >= 5);
    PRUEFE(w[1] > 0);
}

TEST(dienst_m1_vorgemerkter_fehler_ueberlebt_vollen_socket) {
    Aufbau a;
    a.welt.neu("addon/text", typ::B).b = std::string(1024, 'x');
    std::vector<std::string> namen(8192, "addon/text");
    a.hallo();
    for (const auto& d : abo_datagramme(3, 1, namen, 5)) a.sende(d);
    // Bis das Abo am Budget verworfen und der Fehler vorgemerkt ist.
    for (int i = 0; i < 60 && log_mit(a, "speicher_limit").empty(); ++i) a.frames(1);
    PRUEFE(!log_mit(a, "speicher_limit").empty());
    a.neue();
    a.umg.ok_noch = 0;  // Socket voll
    a.frames(3);
    PRUEFE(a.neue_vom_typ("fehler").empty());
    a.umg.ok_noch = -1;
    a.frames(1);
    const auto f = a.neue_vom_typ("fehler");
    PRUEFE_GLEICH(f.size(), size_t(1));
    if (!f.empty()) {
        PRUEFE_TEXT(f[0].hole("grund")->text, "speicher_limit");
        PRUEFE(f[0].hole("abo")->zahl == 3 && f[0].hole("gen")->zahl == 5);
    }
}

TEST(dienst_h4_ausreisser_fuellen_die_drosseltabelle_nicht) {
    // 800 verschiedene Datarefs mit je EINEM Ausreißer (3 ms) — wie über
    // einen langen Flug verstreut. Ein schneller Folgeaufruf räumt ihren
    // Platz; die Tabelle läuft nicht voll, und ein später wirklich langsamer
    // Dataref wird noch gedrosselt.
    Aufbau a;
    std::vector<std::string> namen;
    std::vector<ScheinRef*> refs;
    for (int i = 0; i < 800; ++i) {
        namen.push_back("addon/a" + std::to_string(i));
        refs.push_back(&a.welt.neu(namen.back(), typ::F));
    }
    auto& echt = a.welt.neu("addon/echt_langsam", typ::F);
    namen.push_back("addon/echt_langsam");
    a.hallo();
    for (const auto& d : abo_datagramme(2, 20, namen, 1)) a.sende(d);
    a.frames(30);
    // In 16 Schüben zu je 50 (über den Flug verstreut): jeder Schub hat Zeit
    // für seinen schnellen Folgeaufruf, bevor der nächste kommt.
    for (int schub = 0; schub < 16; ++schub) {
        for (int i = 0; i < 50; ++i) refs[static_cast<size_t>(schub * 50 + i)]->kosten_einmal = 0.003;
        a.frames(150);
    }
    for (auto* r : refs) PRUEFE(r->kosten_einmal == 0.0);  // jeder Ausreißer ist passiert
    echt.kosten = 0.005;
    a.frames(300);
    PRUEFE(log_mit(a, "Drosseltabelle voll").empty());
    PRUEFE(!log_mit(a, "addon/echt_langsam antwortet langsam").empty());
}

// =============================================================================
// Nachprüfung Claude (f03e072d): Drosselung fair, sparsam und umkehrbar
// =============================================================================

TEST(dienst_drossel_ein_aufruf_bedient_alle_elemente) {
    // Nachweis A: addon/eng[0..7] eines langsamen Arrays. Vorher bekam nur
    // eng[0] Werte (29 in 30 s), eng[1..7] keinen einzigen.
    Aufbau a;
    auto& eng = a.welt.neu("addon/eng", typ::VF);
    eng.vf = {10, 11, 12, 13, 14, 15, 16, 17};
    eng.kosten = 0.005;
    std::string abo = "ABO 2 20\n";
    for (int i = 0; i < 8; ++i) abo += "addon/eng[" + std::to_string(i) + "]\n";
    a.hallo();
    a.sende(abo);
    std::map<int, int> je_index;
    std::map<int, double> letzter;
    bool falsch = false;
    const int getter_vor = eng.getter_aufrufe;
    for (int i = 0; i < 1800; ++i) {  // 30 s
        if (i % 60 == 0) a.sende("PING");
        a.frame();
        for (auto& j : a.neue_vom_typ("w")) {
            for (auto& e : j.hole("v")->feld) {
                const int k = static_cast<int>(e.feld[0].zahl);
                ++je_index[k];
                if (e.feld[1].zahl != 10 + k) falsch = true;  // richtiges Element
            }
        }
    }
    std::printf("     eng[0..7] gedrosselt, Werte in 30 s:");
    for (int k = 0; k < 8; ++k) std::printf(" %d", je_index[k]);
    std::printf(" (Getter-Aufrufe %d)\n", eng.getter_aufrufe - getter_vor);
    for (int k = 0; k < 8; ++k) PRUEFE(je_index[k] >= 20);
    PRUEFE(!falsch);
    PRUEFE(eng.getter_aufrufe - getter_vor <= 40);  // ≈ einer je Sekunde, nicht 8
}

TEST(dienst_drossel_sperre_reihum_ueber_handles) {
    // Nachweis B: 10 langsame Datarefs. Vorher bekamen die hinteren Indizes
    // (5–9) in 50 s nichts. Jetzt geht die 0,2-s-Sperre an den am längsten
    // wartenden: je ≈ 0,5 Werte/s für alle.
    Aufbau a;
    std::string abo = "ABO 2 20\n";
    for (int i = 0; i < 10; ++i) {
        auto& r = a.welt.neu("addon/s" + std::to_string(i), typ::F);
        r.f = static_cast<float>(i);
        r.kosten = 0.005;
        abo += "addon/s" + std::to_string(i) + "\n";
    }
    a.hallo();
    a.sende(abo);
    std::map<int, int> je_index;
    for (int i = 0; i < 3000; ++i) {  // 50 s
        if (i % 60 == 0) a.sende("PING");
        a.frame();
        for (auto& j : a.neue_vom_typ("w")) {
            for (auto& e : j.hole("v")->feld) ++je_index[static_cast<int>(e.feld[0].zahl)];
        }
    }
    int mini = 1 << 30, maxi = 0;
    std::printf("     10 langsame Datarefs, Werte in 50 s:");
    for (int k = 0; k < 10; ++k) {
        std::printf(" %d", je_index[k]);
        mini = std::min(mini, je_index[k]);
        maxi = std::max(maxi, je_index[k]);
    }
    std::printf("\n");
    PRUEFE(mini >= 15);          // Soll ≈ 25 (5 Lesungen/s über 10 Handles)
    PRUEFE(maxi <= 2 * mini);    // fair
}

TEST(dienst_drossel_suche_fragt_laenge_je_handle) {
    // 256 Elemente eines langsamen Arrays: vorher 256 Längenaufrufe je
    // Suchlauf (≈ 1,3 s bei 5 ms), auch nach Flughafen-/Flugzeugwechsel.
    Aufbau a;
    auto& gross = a.welt.neu("addon/gross", typ::VF);
    gross.vf = std::vector<float>(256, 1.0f);
    gross.kosten = 0.005;
    std::string abo = "ABO 2 5\n";
    for (int i = 0; i < 256; ++i) abo += "addon/gross[" + std::to_string(i) + "]\n";
    a.hallo();
    a.sende(abo);
    for (int i = 0; i < 20 && a.neue_vom_typ("abo").empty(); ++i) a.frames(1);
    std::printf("     Anmeldung 256 Elemente: %d Laengenaufrufe\n", gross.laengen_aufrufe);
    PRUEFE(gross.laengen_aufrufe <= 1);  // eine Länge je Handle und Durchlauf
    a.frames(300);                        // gedrosselt (Werte-Getter langsam)
    const int vor = gross.laengen_aufrufe;
    a.d->flughafen_geladen();             // dringender Lauf über alle 256
    a.frames(120);
    std::printf("     dringender Lauf danach: %d Laengenaufrufe\n", gross.laengen_aufrufe - vor);
    PRUEFE(gross.laengen_aufrufe - vor <= 1);
    PRUEFE(!log_mit(a, "addon/gross").empty());  // war wirklich gedrosselt
}

TEST(dienst_drossel_hoechstens_zwei_fremde_aufrufe_je_frame) {
    // Langsame (4 ms) Accessoren gleichzeitig in Abo 1, Abo 2 und in der
    // Suche (Array-Längen neuer Abos). Vorher bis zu drei je Frame (12 ms).
    Aufbau a;
    std::string abo1 = "ABO 1 50\n", abo2 = "ABO 2 50\n";
    for (int i = 0; i < 20; ++i) {
        a.welt.neu("addon/a" + std::to_string(i), typ::F).kosten = 0.004;
        a.welt.neu("addon/b" + std::to_string(i), typ::F).kosten = 0.004;
        abo1 += "addon/a" + std::to_string(i) + "\n";
        abo2 += "addon/b" + std::to_string(i) + "\n";
        auto& arr = a.welt.neu("addon/arr" + std::to_string(i), typ::VF);
        arr.vf = std::vector<float>(4, 1.0f);
        arr.kosten = 0.004;
    }
    a.hallo();
    a.sende(abo1);
    a.sende(abo2);
    double max_frame = 0.0;
    std::map<int, int> w;
    for (int i = 0; i < 600; ++i) {
        if (i % 60 == 0) a.sende("PING");
        if (i % 10 == 0 && i < 400) {  // laufend neue Abos mit langsamen Längen
            a.sende("ABO " + std::to_string(3 + (i / 10) % 10) + " 5 g" + std::to_string(i + 1) +
                    "\naddon/arr" + std::to_string((i / 10) % 20));
        }
        const double vorher = a.umg.zeit + 1.0 / 60.0;
        a.frame();
        max_frame = std::max(max_frame, a.umg.zeit - vorher);
        for (auto& j : a.neue_vom_typ("w")) w[static_cast<int>(j.hole("abo")->zahl)]++;
    }
    std::printf("     4-ms-Accessoren in Suche, Abo 1, Abo 2: laengster Frame %.2f ms\n", max_frame * 1000.0);
    PRUEFE(max_frame <= grenzen::FRAME_BUDGET_S + 2 * 0.004 + 2e-4);
    PRUEFE(w[1] > 0 && w[2] > 0);  // beide kommen voran
}

TEST(dienst_drossel_wird_aufgehoben) {
    // Nachweis C: Der Accessor wird wieder schnell → nach 5 schnellen
    // gedrosselten Lesungen wieder volle Rate, einmal im Log.
    Aufbau a;
    auto& r = a.welt.neu("addon/erholt", typ::F);
    r.kosten = 0.005;
    a.hallo();
    a.sende("ABO 2 20\naddon/erholt");
    a.frames(300);
    PRUEFE(!log_mit(a, "addon/erholt antwortet langsam").empty());
    r.kosten = 0.0;
    a.frames(600);  // ≥ 5 gedrosselte Lesungen (≈ 1/s)
    PRUEFE_GLEICH(log_zaehle(a, "addon/erholt antwortet wieder schnell"), 1);
    const int vor = r.getter_aufrufe;
    a.frames(120);
    PRUEFE(r.getter_aufrufe - vor >= 35);  // volle Rate (Soll 40 in 2 s)
}

TEST(dienst_h2_auch_reine_sim_abos_verwerfen_die_runde) {
    // Nachweis: reines sim/-Abo, Runde über viele Frames; Wechsel mittendrin.
    // Vorher gingen danach noch vor dem Wechsel gelesene Werte hinaus.
    Aufbau a;
    std::string abo = "ABO 1 1\n";
    std::vector<ScheinRef*> refs;
    for (int i = 0; i < 400; ++i) {
        auto& r = a.welt.neu("sim/h2/n" + std::to_string(i), typ::F);
        r.f = 1.0f;
        refs.push_back(&r);
        abo += "sim/h2/n" + std::to_string(i) + "\n";
    }
    a.hallo();
    a.sende(abo);
    a.welt.lese_kosten = 1e-4;
    bool mitten = false;
    for (int i = 0; i < 200 && !mitten; ++i) {
        const int vorher = a.welt.lesezugriffe;
        a.frames(1);
        mitten = std::string(a.d->abo_phase(1)) == "lesen" && a.welt.lesezugriffe > vorher;
    }
    PRUEFE(mitten);
    a.neue();
    for (auto* r : refs) r->f = 2.0f;
    a.d->flugzeug_geladen();
    std::vector<JWert> nachher;
    for (int i = 0; i < 240; ++i) {
        a.frames(1);
        for (auto& j : a.neue()) nachher.push_back(j);
    }
    PRUEFE(!enthaelt_wert(nachher, 1.0));
    PRUEFE(enthaelt_wert(nachher, 2.0));  // ohne Pause weiter
    size_t flugzeug = nachher.size(), wert = nachher.size();
    for (size_t k = 0; k < nachher.size(); ++k) {
        if (art(nachher[k]) == "flugzeug" && flugzeug == nachher.size()) flugzeug = k;
        if (art(nachher[k]) == "w" && wert == nachher.size()) wert = k;
    }
    PRUEFE(flugzeug < wert);
}

TEST(dienst_h3_ersatz_abo_nahe_am_gesamtbudget) {
    // Abos 2–5 (je ≈ 14,4 MiB) + Abo 6 (≈ 4,8 MiB) ≈ 62 MiB. Ersatz für
    // Abo 2 mit ≈ 4 MiB Namenstext: vorher zählte das alte Abo 2 beim Aufbau
    // mit → speicher_limit, obwohl es nach dem Ersatz passt.
    Aufbau a;
    a.welt.neu("addon/arr", typ::VF).vf = std::vector<float>(256, 0.5f);
    std::vector<std::string> gross(3400, "addon/arr"), klein(1100, "addon/arr");
    a.hallo();
    std::map<int, std::string> fehler;
    auto laufen = [&](int frames) {
        for (int i = 0; i < frames; ++i) {
            a.frames(1);
            for (auto& j : a.neue_vom_typ("fehler")) fehler[static_cast<int>(j.hole("abo")->zahl)] = j.hole("grund")->text;
        }
    };
    for (int id = 2; id <= 5; ++id) { for (const auto& d : abo_datagramme(id, 1, gross, 1)) a.sende(d); laufen(20); }
    for (const auto& d : abo_datagramme(6, 1, klein, 1)) a.sende(d);
    laufen(20);
    PRUEFE(fehler.empty());
    std::printf("     vor dem Ersatz: %.1f MiB\n", static_cast<double>(a.d->speicher_abos()) / (1 << 20));
    std::vector<std::string> lang;
    for (int i = 0; i < 8192; ++i) {
        std::string n = "addon/ersatz/" + std::to_string(i) + "/";
        n += std::string(480 - n.size(), 'x');
        lang.push_back(n);
    }
    for (const auto& d : abo_datagramme(2, 1, lang, 2)) a.sende(d);
    bool status_gen2 = false;
    for (int i = 0; i < 120 && !status_gen2; ++i) {
        a.frames(1);
        for (auto& j : a.neue()) {
            if (art(j) == "fehler") fehler[static_cast<int>(j.hole("abo")->zahl)] = j.hole("grund")->text;
            if (art(j) == "abo" && j.hole("abo")->zahl == 2 && j.hole("gen")->zahl == 2) status_gen2 = true;
        }
    }
    PRUEFE(!fehler.count(2));
    PRUEFE(status_gen2);
    PRUEFE(a.d->speicher_abos() <= grenzen::MAX_BYTES_ABOS);
}

TEST(dienst_telemetrie_behaelt_rate_neben_langsamer_suche) {
    // Nachweis H: Abo 1 mit 50 schnellen sim/-Floats @ 20 Hz, Abo 2 mit 400
    // verschiedenen langsamen Arrays (3 ms). Mit der Frame-Grenze aus Such-
    // und Liefer-Budget kostete ein langsamer Such-Aufruf Abo 1 den Getter:
    // 46, 114, 201 … Pakete je 10 s. Soll: durchgehend ≈ 200.
    Aufbau a;
    std::string abo1 = "ABO 1 20\n";
    for (int i = 0; i < 50; ++i) {
        a.welt.neu("sim/tele/f" + std::to_string(i), typ::F).f = 1.0f;
        abo1 += "sim/tele/f" + std::to_string(i) + "\n";
    }
    std::vector<std::string> langsam;
    for (int i = 0; i < 400; ++i) {
        langsam.push_back("addon/arr" + std::to_string(i));
        auto& r = a.welt.neu(langsam.back(), typ::VF);
        r.vf = std::vector<float>(8, 1.0f);
        r.kosten = 0.003;
    }
    a.hallo();
    a.sende(abo1);
    for (const auto& d : abo_datagramme(2, 20, langsam, 1)) a.sende(d);
    std::vector<int> je_10s;
    int zaehler = 0;
    double max_frame = 0.0;
    for (int i = 0; i < 60 * 40; ++i) {  // 40 s
        if (i % 60 == 0) a.sende("PING");
        const double vorher = a.umg.zeit + 1.0 / 60.0;
        a.frame();
        max_frame = std::max(max_frame, a.umg.zeit - vorher);
        for (auto& j : a.neue_vom_typ("w")) if (j.hole("abo")->zahl == 1) ++zaehler;
        if ((i + 1) % 600 == 0) { je_10s.push_back(zaehler); zaehler = 0; }
    }
    std::printf("     Abo-1-Pakete je 10 s neben 400 langsamen Arrays:");
    for (int n : je_10s) std::printf(" %d", n);
    std::printf(" (laengster Frame %.2f ms)\n", max_frame * 1000.0);
    for (int n : je_10s) PRUEFE(n >= 180);  // ≈ 20 Hz durchgehend
    PRUEFE(max_frame <= grenzen::FRAME_BUDGET_S + 2 * 0.003 + 3e-4);
}

TEST(dienst_gedrosselt_kein_doppelter_index_in_einer_runde) {
    // Nachweis I: addon/x steht vorn und hinten in einem Abo, dazwischen
    // 300 Namen à 0,3 ms (1 ms Budget je Frame → Runde ≈ 1,7 s). In Runde 2 wird addon/x beim vorderen
    // Eintrag gedrosselt (dritter langsamer Aufruf in Folge) und ist beim
    // hinteren fällig. Vorher bediente der gedrosselte Aufruf den vorderen
    // Eintrag noch einmal — doppelter Index in derselben Runde.
    Aufbau a;
    auto& x = a.welt.neu("addon/x", typ::F);
    x.kosten = 0.005;
    std::vector<std::string> namen{"addon/x"};
    for (int i = 0; i < 300; ++i) {
        namen.push_back("sim/i/n" + std::to_string(i));
        a.welt.neu(namen.back(), typ::F);
    }
    namen.push_back("addon/x");
    a.welt.lese_kosten = 3e-4;
    a.hallo();
    for (const auto& d : abo_datagramme(2, 1, namen, 1)) a.sende(d);
    std::map<int, std::map<int, int>> je_seq;  // seq → index → Anzahl
    for (int i = 0; i < 60 * 12; ++i) {
        if (i % 60 == 0) a.sende("PING");
        a.frame();
        for (auto& j : a.neue_vom_typ("w")) {
            const int s = static_cast<int>(j.hole("seq")->zahl);
            for (auto& e : j.hole("v")->feld) je_seq[s][static_cast<int>(e.feld[0].zahl)]++;
        }
    }
    int doppelt = 0;
    for (auto& kv : je_seq) for (auto& iv : kv.second) if (iv.second > 1) ++doppelt;
    PRUEFE(!log_mit(a, "addon/x antwortet langsam").empty());  // wirklich gedrosselt
    PRUEFE(je_seq.size() >= 3);
    PRUEFE_GLEICH(doppelt, 0);
}

TEST(dienst_flugzeug_titel_aus_ui_name) {
    // ToLiss: acf_descrip ist eine Beschreibung, der UI-Name der Name, den
    // auch der Scan des Clients nimmt. titel bleibt acf_descrip (wie bis
    // 1.0.0, Cloud-QS 29.09.2026), der UI-Name kommt als eigenes Feld.
    Aufbau a;
    a.welt.such("sim/aircraft/view/acf_descrip")->b =
        std::string("A320 with high fidelity system modelling") + std::string(40, '\0');
    auto& ui = a.welt.neu("sim/aircraft/view/acf_ui_name", typ::B);
    ui.b = std::string("  ToLiSs A320 Hi Def \t") + std::string(200, '\0');
    a.sende("HALLO 2 test");  // (hallo() würde die Meldung schon verbrauchen)
    a.frame();
    auto f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    if (!f.empty()) {
        PRUEFE_TEXT(f[0].hole("titel")->text, "A320 with high fidelity system modelling");
        PRUEFE(f[0].hole("ui_name") != nullptr);
        if (f[0].hole("ui_name")) PRUEFE_TEXT(f[0].hole("ui_name")->text, "ToLiSs A320 Hi Def");  // getrimmt
        PRUEFE(f[0].hole("beschreibung") == nullptr);
        PRUEFE_TEXT(f[0].hole("icao")->text, "A20N");
    }
    // Nur der UI-Name ändert sich → neue Meldung (2-s-Takt).
    ui.b = std::string("ToLiSs A321") + std::string(200, '\0');
    a.frames(150);
    f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    if (!f.empty() && f[0].hole("ui_name")) PRUEFE_TEXT(f[0].hole("ui_name")->text, "ToLiSs A321");
    // Leerer UI-Name → kein Feld; titel unverändert acf_descrip.
    ui.b = std::string("   ") + std::string(200, '\0');
    a.d->flugzeug_geladen();
    a.frame();
    f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    if (!f.empty()) {
        PRUEFE_TEXT(f[0].hole("titel")->text, "A320 with high fidelity system modelling");
        PRUEFE(f[0].hole("ui_name") == nullptr);
    }
    // Leere Beschreibung, UI-Name da → titel bleibt leer/null, kippt NICHT
    // auf den UI-Namen (Cloud-QS P3).
    a.welt.such("sim/aircraft/view/acf_descrip")->b = std::string(10, '\0');
    ui.b = std::string("ToLiSs A320") + std::string(200, '\0');
    a.d->flugzeug_geladen();
    a.frame();
    f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    if (!f.empty()) {
        PRUEFE(f[0].hole("titel")->art != JWert::Art::TEXT || f[0].hole("titel")->text.empty());
        if (f[0].hole("ui_name")) PRUEFE_TEXT(f[0].hole("ui_name")->text, "ToLiSs A320");
    }
}

TEST(dienst_flugzeug_ohne_ui_name_wie_bisher) {
    // X-Plane 11: acf_ui_name gibt es nicht → titel = acf_descrip, kein
    // Feld "ui_name"; ohne Dataref titel null.
    Aufbau a;
    a.sende("HALLO 2 test");  // (hallo() würde die Meldung schon verbrauchen)
    a.frame();
    auto f = a.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    if (!f.empty()) {
        PRUEFE_TEXT(f[0].hole("titel")->text, "A320neo");
        PRUEFE(f[0].hole("ui_name") == nullptr);
    }
    a.welt.such("sim/aircraft/view/acf_descrip")->registriert = false;
    Aufbau b;  // Dataref fehlt ganz (frischer Dienst, kein zwischengespeicherter Handle)
    b.welt.such("sim/aircraft/view/acf_descrip")->registriert = false;
    b.sende("HALLO 2 test");
    b.frame();
    f = b.neue_vom_typ("flugzeug");
    PRUEFE_GLEICH(f.size(), size_t(1));
    if (!f.empty()) {
        PRUEFE(f[0].hole("titel")->art == JWert::Art::NUL);
        PRUEFE(f[0].hole("ui_name") == nullptr);
    }
}
