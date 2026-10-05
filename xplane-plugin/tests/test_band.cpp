// Einheitstests: HUD-Band (band.cpp, Befehl BAND in anfrage.cpp/dienst.cpp).
//
// Der Kern kennt keine Uhr: die Tests geben die Zeit vor. Die Dienst-Tests
// laufen gegen die Schein-Welt und prüfen auch die Antwortpakete streng.

#include "anfrage.h"
#include "band.h"
#include "band_layout.h"
#include "band_schrift.h"
#include "dienst.h"
#include "json_pruefer.h"
#include "schein_welt.h"
#include "testrahmen.h"

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

using namespace aeroacars;

namespace {

const Absender CLIENT{0x7F000001u, 50000};

// Das Datagramm liegt in einem Heap-Block GENAU seiner Länge (ASan sieht jedes
// Lesen darüber hinaus).
struct Kopie {
    char* p;
    size_t n;
    explicit Kopie(const std::string& s) : p(static_cast<char*>(std::malloc(s.empty() ? 1 : s.size()))), n(s.size()) {
        if (!s.empty()) std::memcpy(p, s.data(), s.size());
    }
    ~Kopie() { std::free(p); }
};

// Körper direkt prüfen.
bool koerper(uint32_t zeilen, const std::string& rest, BandBild* aus = nullptr, uint32_t* fz = nullptr) {
    static BandBild lokal;
    Kopie k(rest);
    uint32_t zeile = 0;
    const bool ok = band_zerlege_koerper(zeilen, false, k.p, k.n, aus ? aus : &lokal, &zeile);
    if (fz) *fz = zeile;
    return ok;
}

std::string text_von(const BandBild& b, int zeile, int lauf) {
    const BandZeile& z = b.zeile[zeile];
    return std::string(z.text + z.laeufe[lauf].ofs, z.laeufe[lauf].laenge);
}

// Gesamtes Band-Datagramm für den Dienst.
std::string datagramm(uint32_t seq, int ruhig, uint32_t zeilen, const std::string& rest) {
    return "BAND " + std::to_string(seq) + " " + std::to_string(ruhig) + " " + std::to_string(zeilen) +
           "\n" + rest;
}

const std::string ZWEI = "ntvor 12 s\tdmPIREP prefiled\nbp\tniDLH 400\tdlRoute\tnvEDDF > KJFK\n";

BandErgebnis geben(Band& b, uint32_t seq, uint32_t zeilen, const std::string& rest, double t,
                   uint32_t* fz = nullptr) {
    Kopie k(rest);
    uint32_t zeile = 0;
    const BandErgebnis r = b.uebernehme(seq, false, zeilen, k.p, k.n, t, &zeile);
    if (fz) *fz = zeile;
    return r;
}

struct Aufbau {
    ScheinWelt welt;
    ScheinUmgebung umg;
    std::unique_ptr<Dienst> d;
    size_t gelesen = 0;
    Aufbau() {
        welt.uhr = &umg;
        Kennung k;
        k.plugin_version = "1.1.0";
        k.xplane_version = 12100;
        k.xplm_version = 430;
        welt.neu("sim/aircraft/view/acf_ICAO", typ::B).b = std::string("A20N") + std::string(36, '\0');
        welt.neu("sim/aircraft/view/acf_descrip", typ::B).b = std::string("A320neo") + std::string(253, '\0');
        welt.neu("sim/aircraft/view/acf_relative_path", typ::B).b = std::string("a.acf") + std::string(100, '\0');
        d.reset(new Dienst(welt, umg, k));
    }
    void sende(const std::string& s) { d->empfange(CLIENT, s.data(), s.size()); }
    void hallo() { sende("HALLO 2 test"); umg.zeit += 0.01; d->frame(); gelesen = umg.gesendet.size(); }
    std::vector<JWert> neue() {
        std::vector<JWert> aus;
        for (; gelesen < umg.gesendet.size(); ++gelesen) {
            JWert j;
            std::string grund;
            if (!pruefe_paket(umg.gesendet[gelesen].second, &j, &grund)) {
                testrahmen::melde(__FILE__, __LINE__, "ungueltiges Paket: " + grund);
                continue;
            }
            aus.push_back(j);
        }
        return aus;
    }
};

}  // namespace

// ---- Parser: Kopf ---------------------------------------------------------------

TEST(band_kopf_gueltig) {
    Kopie k(datagramm(5, 1, 2, ZWEI));
    NameRef puffer[4];
    Anfrage a;
    PRUEFE(zerlege_anfrage(k.p, k.n, puffer, 4, &a));
    PRUEFE(a.befehl == Befehl::BAND);
    PRUEFE_GLEICH(a.band_seq, 5u);
    PRUEFE(a.band_ruhig);
    PRUEFE_GLEICH(a.band_zeilen, 2u);
    PRUEFE_TEXT(std::string(a.band_rest, a.band_rest_laenge), ZWEI);
    // Grenzwerte
    for (const char* s : {"BAND 0 0 0", "BAND 2147483647 1 4", "BAND 7 0 1\r\nnvx\n"}) {
        Kopie g{std::string(s)};
        PRUEFE(zerlege_anfrage(g.p, g.n, puffer, 4, &a));
    }
}

TEST(band_kopf_ungueltig) {
    NameRef puffer[4];
    const char* schlecht[] = {
        "BAND", "BAND 1", "BAND 1 0", "BAND 1 0 1 9", "BAND -1 0 1", "BAND 2147483648 0 1",
        "BAND x 0 1", "BAND 1 2 1", "BAND 1 00 1", "BAND 1 0 5", "BAND 1 0 x", "BAND  1 0 1",
        "BAND 1 0 1 ", "BAND 1 0 1 2 3 4 5 6",
    };
    for (const char* s : schlecht) {
        Kopie g{std::string(s) + "\nnvx\n"};
        Anfrage a;
        PRUEFE(!zerlege_anfrage(g.p, g.n, puffer, 4, &a));
        PRUEFE_TEXT(fehlergrund_text(a.fehler), "band_ungueltig");
        PRUEFE(a.befehl == Befehl::KEINER);
    }
}

// ---- Parser: Zeilen und Läufe ----------------------------------------------------

TEST(band_laeufe_farben_und_arten) {
    BandBild b;
    PRUEFE(koerper(2, ZWEI, &b));
    PRUEFE_GLEICH(int(b.zeilen), 2);
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 2);
    PRUEFE(b.zeile[0].laeufe[0].farbe == 'n');
    PRUEFE(b.zeile[0].laeufe[0].art == 't');
    PRUEFE_TEXT(text_von(b, 0, 0), "vor 12 s");
    PRUEFE(b.zeile[0].laeufe[1].art == 'm');
    PRUEFE_TEXT(text_von(b, 0, 1), "PIREP prefiled");
    PRUEFE(b.zeile[1].laeufe[0].art == 'p');      // Statuspunkt
    PRUEFE(b.zeile[1].laeufe[0].farbe == 'b');
    PRUEFE_GLEICH(int(b.zeile[1].laeufe[0].laenge), 0);
    PRUEFE(b.zeile[1].laeufe[1].art == 'i');
    PRUEFE(b.zeile[1].laeufe[2].art == 'l');
    PRUEFE_TEXT(text_von(b, 1, 3), "EDDF > KJFK");  // Leerzeichen bleiben im Lauf
    // alle Farben und alle Arten, Text darf leer sein (außer Bedeutung: leer)
    PRUEFE(koerper(1, "nv\tdv\tgv\twv\tbv\tav\n", &b));
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 6);
    PRUEFE_GLEICH(int(b.zeile[0].laeufe[0].laenge), 0);
    PRUEFE(koerper(1, "nt1\tnm2\tnp\tni3\tns4\tnl5\tnv6\tnx7\tnz8\tne9\n", &b));
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 10);
    PRUEFE(b.zeile[0].laeufe[8].art == 'z');
    PRUEFE(b.zeile[0].laeufe[9].art == 'e');
    PRUEFE_TEXT(text_von(b, 0, 9), "9");
    // "@" ist kein Sonderlauf mehr, nur Text
    PRUEFE(koerper(1, "nv@\n", &b));
    PRUEFE(b.zeile[0].laeufe[0].art == 'v');
    // ohne Zeilenende am Schluss
    PRUEFE(koerper(1, "nvx", &b));
    // Beispiel-Datenzeile des Rust-Teils (Landung mit Note + Etikett)
    PRUEFE(koerper(1, "gp\tniDLH 400\tnz87\tweFIRM\tdlRate\tnv-455\tdlG\tnv1.23\tdlBounces\tnv1\tdlBahn\tnv25C\n", &b));
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 12);
}

TEST(band_leere_zeilen) {
    BandBild b;
    // leere Zeile = keine Läufe; jede Zeile endet mit \n, auch die letzte
    PRUEFE(koerper(2, "\nbp\tniX\n", &b));
    PRUEFE_GLEICH(int(b.zeilen), 2);
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 0);
    PRUEFE_GLEICH(int(b.zeile[1].laeufe_n), 3 - 1);
    PRUEFE(koerper(1, "\n", &b));                     // eine leere Zeile
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 0);
    PRUEFE(koerper(2, "nvA\n\n", &b));                // leere LETZTE Zeile
    PRUEFE_GLEICH(int(b.zeile[1].laeufe_n), 0);
    PRUEFE(koerper(3, "nvA\n\nnvB\n", &b));           // leere Zeile in der Mitte
    PRUEFE(!koerper(2, "nvA\n"));                     // das Schluss-\n ist keine zweite Zeile
    PRUEFE(!koerper(1, "nvA\n\n"));                   // … aber ein zweites schon
    // Ein Band nur aus leeren Zeilen ist gültig, hat aber nichts zu zeigen
    Band band(0.0);
    PRUEFE(geben(band, 1, 2, "\n\n", 1.0) == BandErgebnis::ANGENOMMEN);
}

TEST(band_verwerfungsgruende) {
    uint32_t fz = 0;
    PRUEFE(!koerper(1, "xvText\n"));                // unbekannte Farbe
    PRUEFE(!koerper(1, "NvText\n"));                // Farbe ist klein geschrieben
    PRUEFE(!koerper(1, "nqText\n"));                // unbekannte Art
    PRUEFE(!koerper(1, "nVText\n"));                // Art ist klein geschrieben
    PRUEFE(!koerper(1, "n\n"));                     // Lauf ohne Art
    PRUEFE(!koerper(1, "bp@\n"));                   // Punkt mit Text
    PRUEFE(!koerper(1, "b@\n"));                    // alter Sonderlauf
    PRUEFE(!koerper(1, ""));                        // Zeile fehlt
    PRUEFE(!koerper(1, "nv\tx\n"));                 // zweiter Lauf zu kurz
    PRUEFE(!koerper(1, "nv\t\tnv\n"));              // leerer Lauf in der Mitte
    PRUEFE(!koerper(1, "nvA\t\n"));                 // leerer Schlusslauf
    PRUEFE(!koerper(1, "\tnvA\n"));                 // leerer Anfangslauf
    PRUEFE(!koerper(1, "nvA\x01\n"));               // Steuerzeichen
    PRUEFE(!koerper(1, "nvA\x7F\n"));               // DEL
    PRUEFE(!koerper(1, "nv\xC3\xA4\n"));            // Nicht-ASCII
    PRUEFE(!koerper(1, std::string("nvA\0B\n", 6)));  // NUL
    PRUEFE(!koerper(1, "nvA\r\n"));                 // CR im Körper
    PRUEFE(!koerper(1, "\r\n"));                    // CR als einziger Inhalt
    // Zeilenzahl ≠ Angabe, in beide Richtungen; die Fehlerzeile zeigt die Stelle
    PRUEFE(!koerper(2, "nvA\n", nullptr, &fz));
    PRUEFE_GLEICH(fz, 3u);
    PRUEFE(!koerper(1, "nvA\nnvB\n", nullptr, &fz));
    PRUEFE_GLEICH(fz, 3u);
    PRUEFE(!koerper(0, "nvA\n"));                   // zeilen=0 hat keine Folgezeilen
    PRUEFE(!koerper(0, "\n"));                      // auch keine leere
    PRUEFE(!koerper(5, "nvA\n"));                   // mehr als 4
    PRUEFE(!koerper(1, "nvA\xC3\xA4\n", nullptr, &fz));
    PRUEFE_GLEICH(fz, 2u);
}

TEST(band_zeilenlaenge_und_laeufe_grenze) {
    // 512 Byte sind erlaubt, 513 nicht (Zeile ohne Zeilenende).
    PRUEFE(koerper(1, "nv" + std::string(510, 'x') + "\n"));
    PRUEFE(!koerper(1, "nv" + std::string(511, 'x') + "\n"));
    // 171 Läufe zu je 2 Byte + 170 TAB = 512 Byte: genau die Grenze.
    std::string viele;
    for (int i = 0; i < 171; ++i) viele += (i ? "\tnv" : "nv");
    PRUEFE_GLEICH(viele.size(), size_t(512));
    PRUEFE_GLEICH(grenzen::BAND_MAX_LAEUFE, size_t(171));
    BandBild b;
    PRUEFE(koerper(1, viele + "\n", &b));
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 171);
    // vier volle Zeilen
    const std::string voll = "nv" + std::string(510, 'y') + "\n";
    PRUEFE(koerper(4, voll + voll + voll + voll));
}

TEST(band_null_zeilen) {
    BandBild b;
    PRUEFE(koerper(0, "", &b));
    PRUEFE_GLEICH(int(b.zeilen), 0);
}

// ---- Zustand: seq ---------------------------------------------------------------------

TEST(band_seq_reihenfolge) {
    Band b(0.0);
    PRUEFE(geben(b, 10, 1, "nvA\n", 1.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE(geben(b, 10, 1, "nvB\n", 1.1) == BandErgebnis::VERALTET);   // gleich
    PRUEFE(geben(b, 9, 1, "nvB\n", 1.2) == BandErgebnis::VERALTET);    // kleiner
    PRUEFE_TEXT(text_von(*b.ansicht(1.3), 0, 0), "A");                // altes Bild bleibt
    PRUEFE(geben(b, 11, 1, "nvC\n", 1.4) == BandErgebnis::ANGENOMMEN);
    PRUEFE_TEXT(text_von(*b.ansicht(1.5), 0, 0), "C");
    // Lücken sind in Ordnung
    PRUEFE(geben(b, 5000, 1, "nvD\n", 1.6) == BandErgebnis::ANGENOMMEN);
}

TEST(band_seq_ueberlauf) {
    Band b(0.0);
    PRUEFE(geben(b, 2147483647u, 1, "nvA\n", 1.0) == BandErgebnis::ANGENOMMEN);
    // Zähler läuft über: 0 ist neu (Differenz > 2^30) …
    PRUEFE(geben(b, 0, 1, "nvB\n", 1.1) == BandErgebnis::ANGENOMMEN);
    PRUEFE_TEXT(text_von(*b.ansicht(1.2), 0, 0), "B");
    // … 2147483647 ist jetzt alt (nur 2^31−1 über der letzten, kein Überlauf nach unten)
    PRUEFE(geben(b, 1, 1, "nvC\n", 1.3) == BandErgebnis::ANGENOMMEN);
    PRUEFE(geben(b, 0, 1, "nvD\n", 1.4) == BandErgebnis::VERALTET);
    // Differenz genau 2^30 gilt NICHT als Überlauf (ADR: „> 2^30“)
    Band c(0.0);
    PRUEFE(geben(c, 1073741824u, 1, "nvA\n", 1.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE(geben(c, 0, 1, "nvB\n", 1.1) == BandErgebnis::VERALTET);
    PRUEFE(geben(c, 1073741823u + 0u, 1, "nvB\n", 1.1) == BandErgebnis::VERALTET);
}

TEST(band_ungueltig_laesst_band_stehen) {
    Band b(0.0);
    PRUEFE(geben(b, 1, 1, "nvA\n", 1.0) == BandErgebnis::ANGENOMMEN);
    uint32_t fz = 0;
    PRUEFE(geben(b, 2, 1, "xB\n", 2.0, &fz) == BandErgebnis::UNGUELTIG);
    PRUEFE_GLEICH(fz, 2u);
    PRUEFE_TEXT(text_von(*b.ansicht(2.1), 0, 0), "A");
    // der Fehler hat weder seq noch Zeitstempel berührt: seq 2 ist noch frei,
    // und die 5-s-Frist zählt ab 1.0
    PRUEFE(b.ansicht(6.5) != nullptr && b.veraltet(6.5));
    PRUEFE(geben(b, 2, 1, "nvC\n", 6.6) == BandErgebnis::ANGENOMMEN);
    PRUEFE(!b.veraltet(6.7));
}

TEST(band_neuer_client_beginnt_seq_von_vorn) {
    Band b(0.0);
    PRUEFE(geben(b, 900, 1, "nvA\n", 1.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE(geben(b, 3, 1, "nvB\n", 1.1) == BandErgebnis::VERALTET);
    b.neuer_client();
    PRUEFE_TEXT(text_von(*b.ansicht(1.2), 0, 0), "A");   // Bild bleibt
    PRUEFE(geben(b, 3, 1, "nvB\n", 1.3) == BandErgebnis::ANGENOMMEN);
    // auch seq 0 ist für den neuen Client wieder frei (sonst käme er nie an)
    b.neuer_client();
    PRUEFE(geben(b, 0, 1, "nvC\n", 1.4) == BandErgebnis::ANGENOMMEN);
}

// ---- Zustand: Sichtbarkeit und 5-s-Frist ----------------------------------------------

TEST(band_nie_ein_band_gesehen_bleibt_unsichtbar) {
    Band b(100.0);
    PRUEFE(b.ansicht(100.0) == nullptr);
    PRUEFE(b.ansicht(106.0) == nullptr);      // auch nach der Frist: kein Fantom-Band
    PRUEFE(b.ansicht(100000.0) == nullptr);
    PRUEFE(!b.hat_band());
    // ein ungültiges oder veraltetes Datagramm ändert daran nichts
    PRUEFE(geben(b, 1, 1, "xv\n", 101.0) == BandErgebnis::UNGUELTIG);
    PRUEFE(b.ansicht(101.5) == nullptr);
}

TEST(band_null_zeilen_blendet_aus) {
    Band b(0.0);
    PRUEFE(geben(b, 1, 2, ZWEI, 1.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE(b.ansicht(1.5) != nullptr);
    PRUEFE(geben(b, 2, 0, "", 2.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE(b.ausgeblendet());
    PRUEFE(b.ansicht(2.1) == nullptr);
    // ausgeblendet + Frist (und viel länger): bleibt unsichtbar, KEINE Ersatzzeile
    PRUEFE(b.ansicht(7.5) == nullptr);
    PRUEFE(b.ansicht(7000.0) == nullptr);
    // zeilen=0 ist auch als ERSTES Band gültig und zeigt nichts
    Band c(0.0);
    PRUEFE(geben(c, 1, 0, "", 1.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE(c.ansicht(1.1) == nullptr);
    PRUEFE(!c.hat_band());
    PRUEFE(c.ansicht(9.0) == nullptr);
    // ein neues Band mit ≥ 1 Zeile holt es zurück
    PRUEFE(geben(b, 3, 1, "nvNeu\n", 8000.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE(!b.ausgeblendet());
    PRUEFE(b.ansicht(8000.5) != nullptr);
    PRUEFE_TEXT(text_von(*b.ansicht(8000.5), 0, 0), "Neu");
}

TEST(band_ersatzzeile_nach_fuenf_sekunden) {
    Band b(0.0);
    PRUEFE(geben(b, 1, 3, "dvA\nnvB\nnvC\n", 10.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE_GLEICH(int(b.ansicht(15.0)->zeilen), 3);
    PRUEFE_TEXT(text_von(*b.ansicht(15.0), 0, 0), "A");    // genau 5,0 s: noch frisch
    const BandBild* e = b.ansicht(15.01);
    PRUEFE(e != nullptr);
    PRUEFE_GLEICH(int(e->zeilen), 3);                        // Rest bleibt
    PRUEFE_GLEICH(int(e->zeile[0].laeufe_n), 2);
    PRUEFE(e->zeile[0].laeufe[0].art == 'p');
    PRUEFE(e->zeile[0].laeufe[0].farbe == 'b');              // roter Punkt
    PRUEFE_TEXT(text_von(*e, 0, 1), "AeroACARS nicht erreichbar - laeuft die App?");
    PRUEFE_TEXT(text_von(*e, 1, 0), "B");
    PRUEFE_TEXT(text_von(*e, 2, 0), "C");
    PRUEFE(!e->ruhig);                                       // Warnung nicht gedimmt
    // Ein neues Band holt das Original zurück, die Frist beginnt neu.
    PRUEFE(geben(b, 2, 1, "gvOK\n", 20.0) == BandErgebnis::ANGENOMMEN);
    PRUEFE_GLEICH(int(b.ansicht(20.1)->zeilen), 1);
    PRUEFE_TEXT(text_von(*b.ansicht(20.1), 0, 0), "OK");
    PRUEFE(b.ansicht(24.9)->zeile[0].laeufe_n == 1);
    PRUEFE(b.ansicht(25.1)->zeile[0].laeufe[0].art == 'p');
}

TEST(band_ruhig_wird_uebernommen) {
    Band b(0.0);
    Kopie k(std::string("nvA\n"));
    PRUEFE(b.uebernehme(1, true, 1, k.p, k.n, 1.0, nullptr) == BandErgebnis::ANGENOMMEN);
    PRUEFE(b.ansicht(1.1)->ruhig);
    PRUEFE(b.uebernehme(2, false, 1, k.p, k.n, 1.2, nullptr) == BandErgebnis::ANGENOMMEN);
    PRUEFE(!b.ansicht(1.3)->ruhig);
}

// ---- Stufen, Dimmen, Farben ------------------------------------------------------------

TEST(band_stufen_und_klemmung) {
    const float soll[BAND_STUFEN_N] = {0.6f, 0.8f, 1.0f, 1.25f, 1.5f, 2.0f, 3.0f};
    for (int i = 0; i < BAND_STUFEN_N; ++i) PRUEFE(band_skalierung(i) == soll[i]);
    PRUEFE(band_skalierung(BAND_STUFE_STANDARD) == 1.0f);
    PRUEFE_GLEICH(band_stufe_klemmen(-5), 0);
    PRUEFE_GLEICH(band_stufe_klemmen(99), BAND_STUFEN_N - 1);
    PRUEFE(band_skalierung(-1) == 0.6f);
    PRUEFE(band_skalierung(1000) == 3.0f);
    PRUEFE_GLEICH(band_stufe_groesser(2), 3);
    PRUEFE_GLEICH(band_stufe_groesser(BAND_STUFEN_N - 1), BAND_STUFEN_N - 1);  // am Ende: bleibt
    PRUEFE_GLEICH(band_stufe_kleiner(2), 1);
    PRUEFE_GLEICH(band_stufe_kleiner(0), 0);
}

TEST(band_dimm_faktor) {
    PRUEFE(band_dimm_faktor(true, false) == 0.5f);
    PRUEFE(band_dimm_faktor(true, true) == 1.0f);     // Maus darüber
    PRUEFE(band_dimm_faktor(false, false) == 1.0f);   // nicht ruhig: nie gedimmt
    PRUEFE(band_dimm_faktor(false, true) == 1.0f);
    float rgb[3];
    PRUEFE(band_farbe('g', 0.5f, rgb));
    float voll[3];
    PRUEFE(band_farbe('g', 1.0f, voll));
    for (int i = 0; i < 3; ++i) PRUEFE(rgb[i] == voll[i] * 0.5f);
    PRUEFE(!band_farbe('?', 1.0f, rgb));
    for (char c : std::string("ndgwba")) PRUEFE(band_farbe(c, 1.0f, rgb));
}

// ---- Position und Prefs --------------------------------------------------------------

TEST(band_position_klemmung) {
    const BandRechteck s{0, 1080, 1920, 0};
    int l = 0, o = 0;
    l = -50; o = 2000;
    band_klemme_position(s, 400, 40, &l, &o);
    PRUEFE_GLEICH(l, 0);
    PRUEFE_GLEICH(o, 1080);
    l = 1900; o = 10;
    band_klemme_position(s, 400, 40, &l, &o);
    PRUEFE_GLEICH(l, 1520);
    PRUEFE_GLEICH(o, 40);
    l = 700; o = 500;                       // liegt schon drin: unverändert
    band_klemme_position(s, 400, 40, &l, &o);
    PRUEFE_GLEICH(l, 700);
    PRUEFE_GLEICH(o, 500);
    l = 100; o = 100;                       // breiter/höher als der Schirm: linke/obere Kante
    band_klemme_position(s, 3000, 2000, &l, &o);
    PRUEFE_GLEICH(l, 0);
    PRUEFE_GLEICH(o, 1080);
    // Schirm mit Ursprung ≠ 0 (zweiter Monitor links)
    const BandRechteck s2{-1920, 1080, 1920, 0};
    l = -3000; o = 500;
    band_klemme_position(s2, 400, 40, &l, &o);
    PRUEFE_GLEICH(l, -1920);
}

TEST(band_standardposition_oben_mittig) {
    const BandRechteck s{0, 1080, 1920, 0};
    int l = 0, o = 0;
    band_standard_position(s, 400, 40, &l, &o);
    PRUEFE_GLEICH(l, 760);
    PRUEFE(o <= 1080 && o > 1000);
}

// Feldbefund 05.10.2026 (Michel, Mac, X-Plane auf externem Monitor): Schirm
// laut Log "L-190 T0 R1730 B-1080". Das Fenster entstand bei y 100…140 — ganz
// oberhalb des Schirms; X-Plane rief den Zeichen-Callback nie auf, das Band
// blieb unsichtbar.
TEST(band_vor_dem_zeigen_zweiter_monitor) {
    const BandRechteck s{-190, 0, 1730, -1080};
    const BandRechteck start{100, 140, 500, 100};  // wie band_start() es anlegt
    int l = 0, o = 0;
    band_vor_dem_zeigen(s, start, false, &l, &o);
    PRUEFE_GLEICH(l, 570);                 // oben mittig auf DIESEM Schirm
    PRUEFE_GLEICH(o, -12);
    PRUEFE(o <= s.oben && o - 40 >= s.unten);
    // Schon platziert, Schirm danach gewechselt: zurück auf den Schirm klemmen.
    band_vor_dem_zeigen(s, start, true, &l, &o);
    PRUEFE_GLEICH(l, 100);
    PRUEFE_GLEICH(o, 0);
    // Liegt es drin, bleibt es, wo der Pilot es hingezogen hat.
    const BandRechteck drin{-100, -500, 300, -540};
    band_vor_dem_zeigen(s, drin, true, &l, &o);
    PRUEFE_GLEICH(l, -100);
    PRUEFE_GLEICH(o, -500);
}

// Codex-QS v1.9.23: zwei Vollbild-Monitore unterschiedlicher Größe — der
// globale Schirm enthält eine Lücke, die kein Monitor zeigt.
TEST(band_waehle_schirm_echter_monitor) {
    const BandRechteck global{0, 1440, 4480, 0};
    const BandRechteck mon[2] = {{0, 1440, 2560, 0}, {2560, 1080, 4480, 0}};
    // Überdeckt keinen Monitor (liegt in der Lücke über Monitor 2): der erste.
    BandRechteck s = band_waehle_schirm(mon, 2, BandRechteck{3000, 1400, 3400, 1360}, global);
    PRUEFE_GLEICH(s.rechts, 2560);
    // Größte Überdeckung gewinnt.
    s = band_waehle_schirm(mon, 2, BandRechteck{2500, 500, 2900, 460}, global);
    PRUEFE_GLEICH(s.links, 2560);
    // Fenstermodus: das SDK meldet keine Monitore → globaler Schirm.
    s = band_waehle_schirm(nullptr, 0, BandRechteck{100, 140, 500, 100}, global);
    PRUEFE_GLEICH(s.oben, 1440);
    PRUEFE_GLEICH(s.rechts, 4480);
    // Ragt das Band nur ein Stück in die Lücke: auf den Monitor darunter klemmen.
    const BandRechteck halb{3000, 1100, 3400, 1060};
    s = band_waehle_schirm(mon, 2, halb, global);
    PRUEFE_GLEICH(s.links, 2560);
    int l = 0, o = 0;
    band_vor_dem_zeigen(s, halb, true, &l, &o);
    PRUEFE_GLEICH(l, 3000);
    PRUEFE_GLEICH(o, 1080);
}

TEST(band_prefs_runde) {
    BandPrefs p;
    p.an = false;
    p.stufe = 5;
    p.pos_gesetzt = true;
    p.links = -1200;
    p.oben = 987;
    char puffer[128];
    const size_t n = band_prefs_schreibe(p, puffer, sizeof(puffer));
    PRUEFE(n > 0);
    PRUEFE_TEXT(std::string(puffer, n), "an=0\nstufe=5\nx=-1200\ny=987\n");
    const BandPrefs q = band_prefs_lese(puffer, n);
    PRUEFE(!q.an);
    PRUEFE_GLEICH(q.stufe, 5);
    PRUEFE(q.pos_gesetzt);
    PRUEFE_GLEICH(q.links, -1200);
    PRUEFE_GLEICH(q.oben, 987);
    // ohne Position keine x/y-Zeilen
    BandPrefs r;
    const size_t m = band_prefs_schreibe(r, puffer, sizeof(puffer));
    PRUEFE_TEXT(std::string(puffer, m), "an=1\nstufe=2\n");
    PRUEFE(!band_prefs_lese(puffer, m).pos_gesetzt);
    // zu kleiner Puffer
    PRUEFE_GLEICH(band_prefs_schreibe(p, puffer, 5), size_t(0));
}

TEST(band_prefs_kaputt) {
    auto lies = [](const std::string& s) { return band_prefs_lese(s.data(), s.size()); };
    // leer / Müll → Standard
    for (const std::string& s : {std::string(), std::string("\xff\xfe\x00\x01", 4), std::string("garbage"),
                                 std::string("====\n=\n\n#x\n"), std::string(2000, 'a')}) {
        const BandPrefs p = lies(s);
        PRUEFE(p.an);
        PRUEFE_GLEICH(p.stufe, BAND_STUFE_STANDARD);
        PRUEFE(!p.pos_gesetzt);
    }
    // ein kaputter Wert ändert nur seinen Schlüssel
    BandPrefs p = lies("an=ja\nstufe=3\nx=12\ny=abc\n");
    PRUEFE(p.an);
    PRUEFE_GLEICH(p.stufe, 3);
    PRUEFE(!p.pos_gesetzt);                            // y kaputt → keine halbe Position
    // Stufe wird geklemmt, nicht verworfen
    PRUEFE_GLEICH(lies("stufe=99\n").stufe, BAND_STUFEN_N - 1);
    PRUEFE_GLEICH(lies("stufe=-4\n").stufe, 0);
    PRUEFE_GLEICH(lies("stufe=12345678901234\n").stufe, BAND_STUFE_STANDARD);   // zu lang → Standard
    // CRLF, Kommentare, unbekannte Schlüssel, Reihenfolge egal
    p = lies("# Kommentar\r\ny=40\r\nzukunft=1\r\nx=-7\r\nan=0\r\n");
    PRUEFE(!p.an);
    PRUEFE(p.pos_gesetzt);
    PRUEFE_GLEICH(p.links, -7);
    PRUEFE_GLEICH(p.oben, 40);
    // die Position wird NICHT von der Prefs-Schicht geklemmt, das macht band_klemme_position:
    // absurde Werte kommen unverändert an, aber als gültige int
    p = lies("x=999999999\ny=-999999999\n");
    PRUEFE_GLEICH(p.links, 999999999);
    int l = p.links, o = p.oben;
    band_klemme_position(BandRechteck{0, 1080, 1920, 0}, 400, 40, &l, &o);
    PRUEFE_GLEICH(l, 1520);
    PRUEFE_GLEICH(o, 40);
    PRUEFE(band_prefs_lese(nullptr, 3).an);
    // zu große Datei: gar nicht gelesen, auch wenn vorn gültige Werte stehen
    const std::string gross = "stufe=4\nan=0\n#" + std::string(2000, 'x') + "\n";
    PRUEFE_GLEICH(lies(gross).stufe, BAND_STUFE_STANDARD);
    PRUEFE(lies(gross).an);
    PRUEFE_GLEICH(lies("stufe=4\nan=0\n").stufe, 4);
}

// ---- Einbindung in den Dienst -----------------------------------------------------

TEST(band_vor_hallo_ist_kein_hallo) {
    Aufbau a;
    a.sende(datagramm(1, 0, 1, "nvA\n"));
    auto p = a.neue();
    PRUEFE_GLEICH(p.size(), size_t(1));
    PRUEFE_TEXT(p[0].hole("t")->text, "fehler");
    PRUEFE_TEXT(p[0].hole("grund")->text, "kein_hallo");
    PRUEFE(a.d->band().ansicht(a.umg.zeit) == nullptr);   // nichts angenommen
    PRUEFE(!a.d->band().hat_band());
}

TEST(band_im_dienst_gueltig_ohne_antwort) {
    Aufbau a;
    a.hallo();
    a.sende(datagramm(1, 1, 2, ZWEI));
    PRUEFE_GLEICH(a.neue().size(), size_t(0));            // fire-and-forget
    const BandBild* b = a.d->band().ansicht(a.umg.zeit);
    PRUEFE(b != nullptr);
    PRUEFE(b->ruhig);
    PRUEFE_GLEICH(int(b->zeilen), 2);
    // alt → still verworfen, ebenfalls ohne Antwort
    a.sende(datagramm(1, 0, 1, "nvX\n"));
    PRUEFE_GLEICH(a.neue().size(), size_t(0));
    PRUEFE_GLEICH(int(a.d->band().ansicht(a.umg.zeit)->zeilen), 2);
}

TEST(band_im_dienst_ungueltig_antwortet_und_laesst_stehen) {
    Aufbau a;
    a.hallo();
    a.sende(datagramm(1, 0, 2, ZWEI));
    a.neue();
    const char* schlecht[] = {"BAND 2 0 2\nnvA\n", "BAND 2 0 1\nxvA\n", "BAND 2 0 1\nnv\xC3\xA4\n",
                              "BAND 2 7 1\nnvA\n", "BAND\n", "BAND 2 0 1\nb@\n"};
    for (const char* s : schlecht) {
        a.umg.zeit += 0.05;
        a.sende(s);
        auto p = a.neue();
        PRUEFE_GLEICH(p.size(), size_t(1));
        if (p.size() == 1) {
            PRUEFE_TEXT(p[0].hole("t")->text, "fehler");
            PRUEFE_TEXT(p[0].hole("grund")->text, "band_ungueltig");
            PRUEFE_GLEICH(int(p[0].hole("p")->zahl), 2);
        }
        PRUEFE_GLEICH(int(a.d->band().ansicht(a.umg.zeit)->zeilen), 2);   // Band steht
        PRUEFE_TEXT(text_von(*a.d->band().ansicht(a.umg.zeit), 0, 0), "vor 12 s");
    }
}

TEST(band_im_dienst_hallo_antwort_meldet_version) {
    Aufbau a;
    a.sende("HALLO 2 test");
    PRUEFE_TEXT(a.umg.gesendet[0].second,
                "{\"p\":2,\"t\":\"hallo\",\"plugin\":\"1.1.0\",\"xplane\":12100,\"xplm\":430}\n");
}

TEST(band_im_dienst_neuer_client_seq_von_vorn) {
    Aufbau a;
    a.hallo();
    a.sende(datagramm(500, 0, 1, "nvA\n"));
    // Client startet neu (anderer Port) → HALLO → seine seq beginnt bei 1
    const Absender neu{0x7F000001u, 50077};
    const std::string h = "HALLO 2 neu";
    a.d->empfange(neu, h.data(), h.size());
    const std::string b = datagramm(1, 0, 1, "nvB\n");
    a.d->empfange(neu, b.data(), b.size());
    PRUEFE_TEXT(text_von(*a.d->band().ansicht(a.umg.zeit), 0, 0), "B");
}

TEST(band_im_dienst_null_zeilen) {
    Aufbau a;
    a.hallo();
    a.sende(datagramm(1, 0, 1, "nvA\n"));
    a.sende(datagramm(2, 0, 0, ""));
    PRUEFE_GLEICH(a.neue().size(), size_t(0));
    PRUEFE(a.d->band().ansicht(a.umg.zeit) == nullptr);
    a.umg.zeit += 60.0;
    PRUEFE(a.d->band().ansicht(a.umg.zeit) == nullptr);   // auch nach der Frist
}

// ---- Golden-Datagramme des Rust-Clients ----------------------------------------------
//
// hud_band_golden.json: Liste von {"name", "eingabe", "erwartet"}; "erwartet" ist
// das Datagramm, das der Client sendet. Hier wird nur das Feld "erwartet"
// gelesen (JSON-String mit \n, \t, \", \\, \uXXXX) und durch den echten Parser
// geschickt. Jedes davon muss angenommen werden — Zeile 1 ist oft leer (nur
// "\n"), und das letzte "\n" ist keine zusätzliche Zeile.

#ifdef AEROACARS_BAND_GOLDEN
namespace {
std::vector<std::string> golden_datagramme(bool* gelesen) {
    std::vector<std::string> aus;
    *gelesen = false;
    FILE* f = std::fopen(AEROACARS_BAND_GOLDEN, "rb");
    if (f == nullptr) return aus;
    std::string d;
    char buf[4096];
    size_t n;
    while ((n = std::fread(buf, 1, sizeof(buf), f)) > 0) d.append(buf, n);
    std::fclose(f);
    *gelesen = true;
    const std::string schluessel = "\"erwartet\":\"";
    size_t pos = 0;
    while ((pos = d.find(schluessel, pos)) != std::string::npos) {
        pos += schluessel.size();
        std::string s;
        for (; pos < d.size() && d[pos] != '"'; ++pos) {
            if (d[pos] != '\\') { s += d[pos]; continue; }
            ++pos;
            switch (d[pos]) {
                case 'n': s += '\n'; break;
                case 't': s += '\t'; break;
                case 'r': s += '\r'; break;
                case '"': s += '"'; break;
                case '\\': s += '\\'; break;
                case '/': s += '/'; break;
                case 'u': {
                    const unsigned v = static_cast<unsigned>(std::strtoul(d.substr(pos + 1, 4).c_str(), nullptr, 16));
                    s += v < 0x80 ? static_cast<char>(v) : '\x01';  // Nicht-ASCII bliebe ungültig
                    pos += 4;
                    break;
                }
                default: s += '\x01'; break;
            }
        }
        aus.push_back(s);
    }
    return aus;
}
}  // namespace
#endif

TEST(band_golden_vom_rust_client) {
#ifdef AEROACARS_BAND_GOLDEN
    bool gelesen = false;
    const std::vector<std::string> alle = golden_datagramme(&gelesen);
    PRUEFE(gelesen);                    // Datei fehlt → rot, nicht still grün
    PRUEFE(alle.size() >= 10);
    Band band(0.0);
    uint32_t seq = 0;
    static BandAtlas atlas;
    PRUEFE(atlas.backe(1.0f, 2.0f));
    static BandLayout lay;
    float breiteste = 0;
    bool leere_zeile_gesehen = false;
    std::string arten_gesehen;
    for (const std::string& g : alle) {
        Kopie k(g);
        NameRef puffer[1];
        Anfrage a;
        if (!zerlege_anfrage(k.p, k.n, puffer, 1, &a) || a.befehl != Befehl::BAND) {
            testrahmen::melde(__FILE__, __LINE__, "Golden-Kopf abgelehnt: " + g.substr(0, 60));
            continue;
        }
        BandBild b;
        uint32_t fz = 0;
        if (!band_zerlege_koerper(a.band_zeilen, a.band_ruhig, a.band_rest, a.band_rest_laenge, &b, &fz)) {
            testrahmen::melde(__FILE__, __LINE__, "Golden-Körper abgelehnt (Zeile " + std::to_string(fz) + "): " + g.substr(0, 80));
            continue;
        }
        ++testrahmen::pruefzahl();
        PRUEFE_GLEICH(uint32_t(b.zeilen), a.band_zeilen);
        for (int z = 0; z < b.zeilen; ++z) {
            if (b.zeile[z].laeufe_n == 0) leere_zeile_gesehen = true;
            for (int k = 0; k < b.zeile[z].laeufe_n; ++k) {
                if (arten_gesehen.find(b.zeile[z].laeufe[k].art) == std::string::npos) arten_gesehen += b.zeile[z].laeufe[k].art;
            }
        }
        // durch die Anordnung mit der echten Schrift: Kasten genau dann, wenn
        // es Läufe gibt, und auf jedem Schirm handhabbar
        band_ordne(b, atlas, 1.0f, &lay);
        PRUEFE((lay.breite > 0) == band_hat_inhalt(b));
        if (lay.breite > breiteste) breiteste = lay.breite;
        // und durch den Zustand: angenommen, sichtbar genau bei ≥ 1 Zeile
        Kopie r(std::string(a.band_rest, a.band_rest_laenge));
        PRUEFE(band.uebernehme(++seq, a.band_ruhig, a.band_zeilen, r.p, r.n, 1.0, nullptr) ==
               BandErgebnis::ANGENOMMEN);
        PRUEFE((band.ansicht(1.5) != nullptr) == (a.band_zeilen > 0));
    }
    PRUEFE(leere_zeile_gesehen);        // der Fall „leere Zeile“ kommt im Golden vor
    for (char c : std::string("tmpislvxze")) PRUEFE(arten_gesehen.find(c) != std::string::npos);
    PRUEFE_GLEICH(alle.size(), size_t(314));
    std::printf("     Golden: %zu Datagramme, breitestes Band %.0f px bei Stufe 1,0\n", alle.size(),
                static_cast<double>(breiteste));
#endif
}

TEST(band_zeile_leer_und_schluss_newline) {
    BandBild b;
    PRUEFE(koerper(2, "\nnvX\n", &b));          // Zeile 1 leer — gültig, zeichnet nichts
    PRUEFE_GLEICH(int(b.zeile[0].laeufe_n), 0);
    PRUEFE_GLEICH(int(b.zeilen), 2);               // das letzte \n ist keine dritte Zeile
    PRUEFE(!koerper(3, "\nnvX\n"));
}
