// =============================================================================
// Fuzz-Test: Parser und Dienst gegen zufällige und verstümmelte Eingaben
// =============================================================================
//
//   aeroacars_fuzz [sekunden=5] [startwert=20260929]
//
// Fester Startwert → reproduzierbar (derselbe Startwert erzeugt dieselbe
// Folge; nur die Zahl der Durchläufe hängt von der Rechnergeschwindigkeit ab).
// Geprüft wird nur „kein Absturz, keine Grenzverletzung, Ausgabe gültig“:
//
//   Teil A — Parser: jede Eingabe liegt in einem Heap-Block exakt ihrer Länge
//            (ASan meldet jedes Lesen dahinter). Nach dem Parsen müssen alle
//            Zusagen aus anfrage.h gelten.
//   Teil B — Dienst: Ströme aus gültigen, verstümmelten und zufälligen
//            Anfragen von mehreren Absendern, dazwischen Frames, Zeitsprünge,
//            Flugzeugwechsel, verwaiste Datarefs, volle Sockets. JEDES
//            gesendete Paket muss gültiges JSON ≤ 8192 Byte sein.
//
// Unter ASan/UBSan (macOS/Linux) bricht jeder Speicherfehler sofort ab.
// =============================================================================

#include "anfrage.h"
#include "dienst.h"
#include "grenzen.h"
#include "json_pruefer.h"
#include "schein_welt.h"

#include <chrono>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <random>
#include <string>
#include <vector>

using namespace aeroacars;

namespace {

int g_fehler = 0;

void fehler(const char* was, const std::string& eingabe) {
    ++g_fehler;
    if (g_fehler > 20) return;
    std::string hex;
    for (size_t i = 0; i < eingabe.size() && i < 120; ++i) {
        char b[4];
        std::snprintf(b, sizeof(b), "%02x", static_cast<unsigned char>(eingabe[i]));
        hex += b;
    }
    std::fprintf(stderr, "FUZZ-FEHLER: %s (Eingabe %zu Byte: %s...)\n", was, eingabe.size(), hex.c_str());
}

const char* const VORLAGEN[] = {
    "HALLO 2 1.9.12\n",
    "HALLO 2 x",
    "PING\n",
    "PING",
    "LISTE 7\n",
    "ENDE-ABO 3\n",
    "ABO 1 10\nsim/flightmodel/position/latitude\nsim/flightmodel/position/longitude\n",
    "ABO 2 50\nsim/cockpit2/engine/actuators/throttle_ratio[0]\nsim/aircraft/view/acf_ICAO\n",
    "ABO 3 1 1 2\nsim/a\nsim/b[3]\n",
    "ABO 3 1 2 2\nsim/c\n",
    "ABO 16 5\r\nAirbusFBW/APU_Avail\r\nsim/vf[7]\r\n",
    "ABO 4 20\nx\ny\nz\nsim/vi[2]\nsim/b\nfehlt/ganz\n",
};

const char SONDERZEICHEN[] = {'\n', '\r', ' ', '[', ']', '0', '9', '-', '\0', '\t',
                              '"', '\\', '\x7f', '\xc3', '\xa4', '\xff', 'A', 'z'};

std::string zufall_bytes(std::mt19937& rng, size_t max) {
    std::string s(rng() % (max + 1), '\0');
    for (char& c : s) c = static_cast<char>(rng() & 0xFF);
    return s;
}

std::string verstuemmle(std::mt19937& rng, std::string s) {
    const int schritte = 1 + static_cast<int>(rng() % 6);
    for (int k = 0; k < schritte; ++k) {
        const size_t n = s.size();
        switch (rng() % 9) {
            case 0:  // Bit kippen
                if (n) s[rng() % n] ^= static_cast<char>(1u << (rng() % 8));
                break;
            case 1:  // Sonderzeichen einfügen
                s.insert(s.begin() + (n ? rng() % (n + 1) : 0),
                         SONDERZEICHEN[rng() % sizeof(SONDERZEICHEN)]);
                break;
            case 2:  // Bereich löschen
                if (n) {
                    const size_t a = rng() % n;
                    s.erase(a, 1 + rng() % (n - a));
                }
                break;
            case 3:  // Bereich verdoppeln
                if (n) {
                    const size_t a = rng() % n;
                    const size_t l = 1 + rng() % (n - a);
                    s.insert(a, s.substr(a, l));
                }
                break;
            case 4:  // abschneiden
                if (n) s.resize(rng() % n);
                break;
            case 5:  // lange Zeile um die 512er-Grenze
                s += std::string(508 + rng() % 8, 'L') + (rng() % 2 ? "\n" : "");
                break;
            case 6:  // Zahl ersetzen
                if (n) s[rng() % n] = static_cast<char>('0' + rng() % 10);
                break;
            case 7:  // viele Namen anhängen
                for (int i = static_cast<int>(rng() % 3000); i > 0; --i) s += "n\n";
                break;
            case 8:  // Index-Syntax anhängen
                s += "a[" + std::to_string(static_cast<int>(rng())) + "]\n";
                break;
        }
        if (s.size() > 70000) s.resize(70000);
    }
    return s;
}

// `gueltig_anteil` in Prozent: so viele Eingaben sind unveränderte Vorlagen
// (für den Dienst wichtig, damit er überhaupt in tiefe Zustände kommt).
std::string erzeuge(std::mt19937& rng, unsigned gueltig_anteil) {
    const unsigned w = rng() % 100;
    std::string s = VORLAGEN[rng() % (sizeof(VORLAGEN) / sizeof(VORLAGEN[0]))];
    if (w < gueltig_anteil) return s;
    if (w < gueltig_anteil + 15) return zufall_bytes(rng, 600);
    if (w < gueltig_anteil + 17) return zufall_bytes(rng, 70000);
    return verstuemmle(rng, s);
}

void pruefe_parser(const std::string& s, std::vector<NameRef>& puffer) {
    char* block = static_cast<char*>(std::malloc(s.size() ? s.size() : 1));
    if (!s.empty()) std::memcpy(block, s.data(), s.size());
    const size_t kap = puffer.size();
    Anfrage a;
    const bool ok = zerlege_anfrage(block, s.size(), puffer.data(), kap, &a);
    if (ok) {
        if (a.fehler != Fehlergrund::KEINER) fehler("ok mit Fehlergrund", s);
        if (a.befehl == Befehl::KEINER) fehler("ok ohne Befehl", s);
        if (a.namen_anzahl > kap || a.namen_anzahl > grenzen::MAX_NAMEN_JE_ABO) fehler("zu viele Namen", s);
        if (a.namen_anzahl > 0 && a.befehl != Befehl::ABO) fehler("Namen ohne ABO", s);
        for (size_t i = 0; i < a.namen_anzahl; ++i) {
            const NameRef& n = a.namen[i];
            if (n.basis < block || n.basis + n.basis_laenge > block + s.size()) fehler("Name ausserhalb", s);
            if (n.basis_laenge == 0 || n.basis_laenge > grenzen::MAX_ZEILE) fehler("Namenslaenge", s);
            if (!ist_gueltiger_name(n.basis, n.basis_laenge)) fehler("ungueltiger Name akzeptiert", s);
            if (n.index < -1) fehler("Index < -1", s);
        }
        if (a.befehl == Befehl::ABO) {
            if (a.abo_id < 1 || a.abo_id > 16 || a.rate_hz < 1 || a.rate_hz > 50) fehler("ABO-Grenzen", s);
            if (a.teil < 1 || a.teil > a.teile || a.teile > grenzen::MAX_ABO_TEILE) fehler("Teil-Grenzen", s);
        }
        if (a.befehl == Befehl::HALLO) {
            if (a.client_version < block || a.client_version + a.client_version_laenge > block + s.size() ||
                a.client_version_laenge == 0 || a.client_version_laenge > grenzen::MAX_CLIENT_VERSION) {
                fehler("Client-Version", s);
            }
        }
    } else {
        if (a.fehler == Fehlergrund::KEINER) fehler("Fehler ohne Grund", s);
        if (a.befehl != Befehl::KEINER) fehler("Fehler mit Befehl", s);
        if (a.namen_anzahl != 0) fehler("Fehler mit Namen", s);
    }
    std::free(block);
}

struct Welt {
    ScheinWelt welt;
    ScheinUmgebung umg;
    std::unique_ptr<Dienst> d;
    size_t geprueft = 0;

    Welt() {
        welt.uhr = &umg;
        welt.neu("sim/aircraft/view/acf_ICAO", typ::B).b = "A20N";
        welt.neu("sim/aircraft/view/acf_descrip", typ::B).b = "Caf\xE9\x01";
        welt.neu("sim/aircraft/view/acf_relative_path", typ::B).b = "a/b.acf";
        welt.neu("sim/flightmodel/position/latitude", typ::D | typ::F).d = 51.5;
        welt.neu("sim/flightmodel/position/longitude", typ::D).d = -0.1;
        welt.neu("sim/vf", typ::VF).vf = std::vector<float>(300, 1.25f);
        welt.neu("sim/vi", typ::VI).vi = {1, 2, 3};
        welt.neu("sim/b", typ::B).b = std::string("x\"\\\n\xff", 5);
        welt.neu("AirbusFBW/APU_Avail", typ::I).i = 1;
        welt.neu("sim/a", typ::F);
        welt.neu("sim/c", typ::I);
        welt.neu("n", typ::I);
        welt.neu("x", typ::F);
        for (int i = 0; i < 400; ++i) welt.neu("gen/" + std::to_string(i), typ::F);
        welt.ueberschuss = 3;
        Kennung k;
        k.plugin_version = "1.0.0";
        d.reset(new Dienst(welt, umg, k));
    }

    void pruefe_ausgabe() {
        for (; geprueft < umg.gesendet.size(); ++geprueft) {
            JWert j;
            std::string grund;
            if (!pruefe_paket(umg.gesendet[geprueft].second, &j, &grund)) {
                fehler(("ungueltiges Paket: " + grund).c_str(), umg.gesendet[geprueft].second);
            }
        }
        // Speicher der Prüfliste klein halten.
        if (umg.gesendet.size() > 20000) {
            umg.gesendet.clear();
            geprueft = 0;
        }
    }
};

}  // namespace

int main(int argc, char** argv) {
    const double sekunden = argc > 1 ? std::atof(argv[1]) : 5.0;
    const unsigned startwert = argc > 2 ? static_cast<unsigned>(std::strtoul(argv[2], nullptr, 10)) : 20260929u;
    std::mt19937 rng(startwert);
    using uhr = std::chrono::steady_clock;

    // ---- Teil A: Parser ------------------------------------------------------
    std::vector<NameRef> puffer(grenzen::MAX_NAMEN_JE_ABO);
    std::vector<NameRef> klein(3);
    long parser_laeufe = 0;
    const auto ende_a = uhr::now() + std::chrono::duration<double>(sekunden / 2);
    while (uhr::now() < ende_a) {
        for (int k = 0; k < 200; ++k) {
            const std::string s = erzeuge(rng, 20);
            pruefe_parser(s, (k % 7 == 0) ? klein : puffer);
            ++parser_laeufe;
        }
    }

    // ---- Teil B: Dienst --------------------------------------------------------
    long dienst_anfragen = 0, frames = 0, pakete = 0, welten = 0;
    const Absender absender[] = {{0x7F000001u, 50000}, {0x7F000001u, 50001}, {0x7F000001u, 1}};
    const auto ende_b = uhr::now() + std::chrono::duration<double>(sekunden / 2);
    while (uhr::now() < ende_b) {
        Welt w;
        ++welten;
        for (int schritt = 0; schritt < 400; ++schritt) {
            const unsigned r = rng() % 100;
            if (r < 45) {
                std::string s = erzeuge(rng, 50);
                // Oft zuerst ein gültiges HALLO, sonst passiert zu wenig.
                if (rng() % 4 == 0) s = "HALLO 2 fuzz";
                const Absender& von = absender[rng() % 3 == 0 ? rng() % 3 : 0];
                w.d->empfange(von, s.data(), s.size());
                ++dienst_anfragen;
            } else if (r < 90) {
                w.umg.zeit += (rng() % 50 == 0) ? 6.0 : 0.001 * (rng() % 40);
                w.umg.tick_je_uhrabfrage = (rng() % 10 == 0) ? 0.0002 : 0.0;
                if (rng() % 20 == 0) w.umg.voll_noch = static_cast<int>(rng() % 5);
                if (rng() % 30 == 0) w.umg.fehler_noch = static_cast<int>(rng() % 3);
                w.d->frame();
                ++frames;
            } else if (r < 94) {
                auto& ref = *w.welt.refs[rng() % w.welt.refs.size()];
                ref.registriert = rng() % 3 != 0;
                ref.gueltig = rng() % 3 != 0;
                if (!ref.vf.empty()) ref.vf.resize(rng() % 400, 0.5f);
                if (!ref.b.empty() || ref.typen == typ::B) ref.b = zufall_bytes(rng, 80);
            } else if (r < 97) {
                w.d->flugzeug_geladen();
            } else if (r < 99) {
                w.d->flughafen_geladen();
            } else {
                w.welt.liste_da = !w.welt.liste_da;
            }
            pakete += static_cast<long>(w.umg.gesendet.size() - w.geprueft);
            w.pruefe_ausgabe();
        }
    }

    std::printf("Fuzz (Startwert %u, %.1f s): %ld Parser-Eingaben; %ld Dienst-Anfragen, %ld Frames, "
                "%ld Pakete geprueft in %ld Welten; %d Fehler\n",
                startwert, sekunden, parser_laeufe, dienst_anfragen, frames, pakete, welten, g_fehler);
    return g_fehler == 0 ? 0 : 1;
}
