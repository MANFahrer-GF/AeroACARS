// =============================================================================
// AeroACARS X-Plane-Plugin — Dataref-Dienst (Umsetzung)
// =============================================================================
//
// Siehe dienst.h für Aufbau und Lebenszyklus. Die Kommentare hier begründen
// die Stellen, an denen es eine naheliegende, aber falsche Lösung gäbe.
// =============================================================================

#include "dienst.h"

#include <cstdarg>
#include <cstdio>
#include <cstring>

namespace aeroacars {

namespace {

// Höchstlänge eines Status-Elements: [8191,"fehlt"] = 15, [8191,"vf",256] = 16,
// [8191,"b",1024] = 16. Mit Reserve.
constexpr size_t STATUS_MAX = 20;

// "[k," + "]" um jeden Wert.
constexpr size_t WERT_RAHMEN = 1 + 4 + 1 + 1;

int32_t begrenze(int n, int max) noexcept {
    if (n < 0) return 0;
    return n > max ? max : n;
}

size_t text_laenge(const char* s, size_t max) noexcept {
    size_t n = 0;
    while (n < max && s[n] != '\0') ++n;
    return n;
}

}  // namespace

// =============================================================================
// Aufbau / Abbau
// =============================================================================

Dienst::Dienst(Datenquelle& quelle, Umgebung& umgebung, const Kennung& kennung) noexcept
    : quelle_(quelle), umgebung_(umgebung), kennung_(kennung) {
    // Der einzige Grundspeicher: Platz für die Namen EINES Datagramms (die
    // Namen selbst bleiben im Datagramm, hier stehen nur Zeiger + Index).
    bereit_ = namen_puffer_.reserviere(grenzen::MAX_NAMEN_JE_ABO);
    std::memset(paket_, 0, sizeof(paket_));
}

Dienst::~Dienst() {
    alles_verwerfen();
}

size_t Dienst::aktive_abos() const noexcept {
    size_t n = 0;
    for (const Abo& a : abos_) {
        if (a.phase != Phase::LEER) ++n;
    }
    return n;
}

void Dienst::alles_verwerfen() noexcept {
    for (Abo& a : abos_) abo_leeren(a);
    for (AboBau& b : bau_) {
        b.aktiv = false;
        b.text.freigeben();
        b.namen.freigeben();
    }
    liste_leeren();
}

void Dienst::abo_leeren(Abo& abo) noexcept {
    abo.text.freigeben();
    abo.eintraege.freigeben();
    abo.stapel.freigeben();
    abo.grenzen.freigeben();
    abo.phase = Phase::LEER;
    abo.id = 0;
    abo.rate = 0;
    abo.pruefung_laeuft = false;
    abo.pruefung_fertig = false;
    abo.erste_antwort = true;
    abo.pruef_cursor = 0;
    abo.teile = 0;
    abo.paket_cursor = 0;
    abo.lese_cursor = 0;
    abo.seq = 0;
}

void Dienst::liste_leeren() noexcept {
    liste_.aktiv = false;
    liste_.sammeln = false;
    liste_.namen.freigeben();
    liste_.grenzen.freigeben();
    liste_.teile = 0;
    liste_.paket_cursor = 0;
    liste_.cursor = 0;
    liste_.gesamt = 0;
    liste_.ausgelassen = 0;
}

bool Dienst::braucht_jeden_frame() const noexcept {
    if (!client_aktiv_) return false;
    if (liste_.aktiv) return true;
    for (const Abo& a : abos_) {
        if (a.phase != Phase::LEER) return true;
    }
    return false;
}

void Dienst::protokolliere(const char* format, ...) noexcept {
    char zeile[512];
    va_list ap;
    va_start(ap, format);
    std::vsnprintf(zeile, sizeof(zeile), format, ap);
    va_end(ap);
    umgebung_.protokolliere(zeile);
}

// =============================================================================
// Senden
// =============================================================================

void Dienst::sende_einzeln(const Absender& an, const JsonSchreiber& w) noexcept {
    if (w.ueberlauf() || w.laenge() == 0) return;
    // Einzelne Antworten (hallo, pong, fehler, flugzeug) werden nicht
    // wiederholt: sie sind klein, und der Client fragt bei Bedarf erneut.
    umgebung_.sende(an, w.daten(), w.laenge());
}

void Dienst::sende_fehler(const Absender& an, Fehlergrund grund, uint32_t zeile,
                          uint32_t abo, int64_t id) noexcept {
    char puffer[256];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.roh("{\"p\":2,\"t\":\"fehler\",\"grund\":\"");
    w.roh(fehlergrund_text(grund));
    w.zeichen('"');
    if (zeile != 0) { w.roh(",\"zeile\":"); w.ganzzahl(zeile); }
    if (abo != 0)   { w.roh(",\"abo\":");   w.ganzzahl(abo); }
    if (id >= 0)    { w.roh(",\"id\":");    w.ganzzahl(id); }
    w.roh("}\n");
    sende_einzeln(an, w);
}

SendeErgebnis Dienst::sende_paket(const char* daten, size_t laenge) noexcept {
    if (pakete_frame_ >= grenzen::MAX_PAKETE_JE_FRAME) return SendeErgebnis::VOLL;
    const SendeErgebnis r = umgebung_.sende(client_, daten, laenge);
    if (r != SendeErgebnis::VOLL) ++pakete_frame_;
    return r;
}

// Zeitbudget: Die erste Arbeitseinheit jedes Frames ist frei (sonst könnte ein
// einziges langsames Plugin-Dataref, das das Budget allein sprengt, jeden
// Fortschritt verhindern); danach entscheidet die Uhr.
bool Dienst::darf_arbeiten() noexcept {
    if (garantie_) {
        garantie_ = false;
        return true;
    }
    return umgebung_.jetzt() < budget_ende_;
}

// =============================================================================
// Anfragen
// =============================================================================

void Dienst::empfange(const Absender& von, const char* daten, size_t laenge) noexcept {
    if (!bereit_) return;
    const double jetzt = umgebung_.jetzt();
    const bool vom_client = client_aktiv_ && von == client_;
    // Jedes Datagramm des Clients — auch ein fehlerhaftes — beweist, dass er
    // lebt. Die Zeitüberschreitung soll tote Clients abräumen, nicht
    // fehlerhafte bestrafen.
    if (vom_client) letzte_anfrage_ = jetzt;

    Anfrage a;
    if (!zerlege_anfrage(daten, laenge, namen_puffer_.daten(),
                         namen_puffer_.kapazitaet(), &a)) {
        sende_fehler(von, a.fehler, a.fehler_zeile, a.abo_id, -1);
        return;
    }

    if (a.befehl == Befehl::HALLO) {
        bearbeite_hallo(von, a);
        return;
    }
    if (!vom_client) {
        // Kein HALLO, anderer Port (Client neu gestartet) oder nach
        // Zeitüberschreitung vergessen: der Client muss sich neu melden.
        sende_fehler(von, Fehlergrund::KEIN_HALLO, 0, a.abo_id, -1);
        return;
    }

    switch (a.befehl) {
        case Befehl::PING: {
            char puffer[32];
            JsonSchreiber w(puffer, sizeof(puffer));
            w.roh("{\"p\":2,\"t\":\"pong\"}\n");
            sende_einzeln(von, w);
            break;
        }
        case Befehl::ENDE_ABO: {
            const size_t i = a.abo_id - 1;
            abo_leeren(abos_[i]);
            bau_[i].aktiv = false;
            bau_[i].text.freigeben();
            bau_[i].namen.freigeben();
            break;
        }
        case Befehl::LISTE:
            bearbeite_liste(a);
            break;
        case Befehl::ABO:
            bearbeite_abo(a);
            break;
        case Befehl::HALLO:
        case Befehl::KEINER:
            break;
    }
}

void Dienst::bearbeite_hallo(const Absender& von, const Anfrage& a) noexcept {
    if (a.protokoll == grenzen::PROTOKOLL) {
        if (!client_aktiv_ || !(von == client_)) {
            // Neuer Client (oder derselbe nach Neustart mit neuem Port): alles
            // vom alten verwerfen — dessen Abos kennt der neue nicht.
            alles_verwerfen();
            client_ = von;
            client_aktiv_ = true;
            char version[grenzen::MAX_CLIENT_VERSION + 1];
            const size_t n = a.client_version_laenge < grenzen::MAX_CLIENT_VERSION
                                 ? a.client_version_laenge : grenzen::MAX_CLIENT_VERSION;
            std::memcpy(version, a.client_version, n);
            version[n] = '\0';
            protokolliere("Protokoll 2: Client angemeldet (Port %u, Version %s)",
                          static_cast<unsigned>(von.port), version);
        }
        letzte_anfrage_ = umgebung_.jetzt();
        flugzeug_offen_ = true;  // aktuelles Flugzeug gleich mitteilen
    }
    // Die Antwort geht auch bei fremder Protokollversion hinaus: an "p":2
    // erkennt der Client, dass er ein anderes Plugin vor sich hat.
    char puffer[160];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.roh("{\"p\":2,\"t\":\"hallo\",\"plugin\":");
    w.text_bis_nul(kennung_.plugin_version, 32);
    w.roh(",\"xplane\":");
    w.ganzzahl(kennung_.xplane_version);
    w.roh(",\"xplm\":");
    w.ganzzahl(kennung_.xplm_version);
    w.roh("}\n");
    sende_einzeln(von, w);
}

void Dienst::bearbeite_abo(const Anfrage& a) noexcept {
    const uint32_t id = a.abo_id;
    AboBau& bau = bau_[id - 1];
    auto verwerfe_bau = [&bau]() noexcept {
        bau.aktiv = false;
        bau.text.freigeben();
        bau.namen.freigeben();
    };

    if (a.teil == 1) {
        // Teil 1 beginnt immer neu — auch wenn ein älterer Aufbau offen war.
        verwerfe_bau();
        bau.aktiv = true;
        bau.rate = a.rate_hz;
        bau.teile = a.teile;
        bau.naechster = 1;
    } else if (!bau.aktiv || a.teil != bau.naechster) {
        // Fehlender, doppelter oder vertauschter Teil. Kein Raten, kein
        // Umsortieren: der ganze Aufbau ist ungültig, der Client schickt neu.
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::ABO_TEIL_REIHENFOLGE, 0, id, -1);
        return;
    } else if (a.rate_hz != bau.rate || a.teile != bau.teile) {
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::ABO_TEILE_WIDERSPRUCH, 0, id, -1);
        return;
    }

    if (bau.namen.anzahl() + a.namen_anzahl > grenzen::MAX_NAMEN_JE_ABO) {
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::ZU_VIELE_NAMEN, 0, id, -1);
        return;
    }
    for (size_t i = 0; i < a.namen_anzahl; ++i) {
        const NameRef& n = a.namen[i];
        const size_t ofs = bau.text.anzahl();
        const char nul = '\0';
        if (ofs > 0x7FFFFFFFu ||
            !bau.text.haenge_an(n.basis, n.basis_laenge) ||
            !bau.text.haenge_an(nul) ||
            !bau.namen.haenge_an(RohName{static_cast<uint32_t>(ofs), n.index})) {
            verwerfe_bau();
            sende_fehler(client_, Fehlergrund::SPEICHER, 0, id, -1);
            return;
        }
    }
    ++bau.naechster;
    if (a.teil < a.teile) return;  // weitere Teile folgen

    if (bau.namen.anzahl() == 0) {
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::KEINE_NAMEN, 0, id, -1);
        return;
    }
    if (!aktiviere(id, bau)) {
        sende_fehler(client_, Fehlergrund::SPEICHER, 0, id, -1);
    }
    verwerfe_bau();
}

bool Dienst::aktiviere(uint32_t id, AboBau& bau) noexcept {
    Abo& abo = abos_[id - 1];
    abo_leeren(abo);  // ein neues ABO mit gleicher ID ersetzt das alte

    const size_t n = bau.namen.anzahl();
    abo.text.uebernehme(bau.text);
    if (!abo.eintraege.reserviere(n) ||
        !abo.grenzen.reserviere(n + 2) ||
        !abo.stapel.reserviere(n * STATUS_MAX, n)) {
        abo_leeren(abo);
        return false;
    }
    for (size_t i = 0; i < n; ++i) {
        Eintrag e;
        e.name_ofs = bau.namen[i].ofs;
        e.index = bau.namen[i].index;
        e.aktiv = Aufloesung{};
        e.kandidat = Aufloesung{};
        abo.eintraege.haenge_an(e);  // Kapazität reserviert
    }
    abo.id = id;
    abo.rate = bau.rate;
    abo.periode = 1.0 / static_cast<double>(bau.rate);
    abo.phase = Phase::NEU;
    abo.erste_antwort = true;
    abo.pruefung_laeuft = true;  // sofort suchen, nicht erst in 2 s
    abo.pruefung_fertig = false;
    abo.pruef_cursor = 0;
    abo.seq = 0;
    return true;
}

void Dienst::bearbeite_liste(const Anfrage& a) noexcept {
    if (!quelle_.liste_verfuegbar()) {
        sende_fehler(client_, Fehlergrund::LISTE_NICHT_VERFUEGBAR, 0, 0, a.anfrage_id);
        return;
    }
    // Eine neue LISTE ersetzt eine laufende (der Client wartet ohnehin nur auf
    // die jüngste Antwort).
    liste_leeren();
    int gesamt = quelle_.anzahl_datarefs();
    if (gesamt < 0) gesamt = 0;
    if (gesamt > grenzen::MAX_LISTE_DATAREFS) gesamt = grenzen::MAX_LISTE_DATAREFS;
    liste_.aktiv = true;
    liste_.sammeln = true;
    liste_.id = a.anfrage_id;
    liste_.gesamt = gesamt;
    liste_.cursor = 0;
    liste_.ausgelassen = 0;
}

// =============================================================================
// Frame
// =============================================================================

void Dienst::frame() noexcept {
    if (!bereit_) return;
    jetzt_ = umgebung_.jetzt();
    budget_ende_ = jetzt_ + grenzen::ZEITBUDGET_S;
    pakete_frame_ = 0;
    suchen_frame_ = 0;
    garantie_ = true;

    if (!client_aktiv_) return;
    if (jetzt_ - letzte_anfrage_ > grenzen::CLIENT_TIMEOUT_S) {
        // Abgestürzter oder beendeter Client: keine Last im Sim zurücklassen.
        protokolliere("Protokoll 2: Client seit %.0f s stumm - Abos verworfen",
                      grenzen::CLIENT_TIMEOUT_S);
        alles_verwerfen();
        client_aktiv_ = false;
        return;
    }

    pruefe_flugzeug();

    // Rundlauf: jedes Abo ist regelmäßig als erstes dran, damit ein großes
    // Abo (Flugzeug vermessen) die kleinen nicht dauerhaft verdrängt.
    const uint32_t start = rundlauf_++ % static_cast<uint32_t>(grenzen::MAX_ABOS);
    for (uint32_t i = 0; i < grenzen::MAX_ABOS; ++i) {
        bearbeite_abo_frame(abos_[(start + i) % grenzen::MAX_ABOS]);
    }
    liste_schritt();
}

void Dienst::flugzeug_geladen() noexcept {
    flugzeug_offen_ = true;
    for (Abo& a : abos_) starte_pruefung_neu(a);
}

void Dienst::flughafen_geladen() noexcept {
    for (Abo& a : abos_) starte_pruefung_neu(a);
}

void Dienst::starte_pruefung_neu(Abo& abo) noexcept {
    if (abo.phase == Phase::LEER) return;
    abo.pruefung_laeuft = true;
    abo.pruefung_fertig = false;
    abo.pruef_cursor = 0;
}

// =============================================================================
// Abos
// =============================================================================

Dienst::Aufloesung Dienst::loese_auf(const char* name, int32_t index) noexcept {
    Aufloesung r;
    const DatarefHandle h = quelle_.finde(name);
    if (h == nullptr) return r;
    // Verwaist: Das Plugin, das den Namen angelegt hat, ist entladen (z. B.
    // nach Flugzeugwechsel). XPLMFindDataRef findet ihn weiter, lesen ergibt
    // aber 0 — genau die Lüge, die Protokoll 2 abschaffen soll.
    if (!quelle_.ist_gueltig(h)) return r;
    const int t = quelle_.typen(h);
    r.h = h;
    if (index < 0) {
        // Vorrang bei mehreren Typen (ADR): double > float > int > Arrays >
        // Bytes; unter den Arrays float vor int.
        if (t & typ::D)      { r.z = Zugriff::D; r.laenge = 1; }
        else if (t & typ::F) { r.z = Zugriff::F; r.laenge = 1; }
        else if (t & typ::I) { r.z = Zugriff::I; r.laenge = 1; }
        else if (t & typ::VF) {
            r.z = Zugriff::VF;
            r.laenge = begrenze(quelle_.lese_vf(h, nullptr, 0, 0), grenzen::MAX_ARRAY_ELEMENTE);
        } else if (t & typ::VI) {
            r.z = Zugriff::VI;
            r.laenge = begrenze(quelle_.lese_vi(h, nullptr, 0, 0), grenzen::MAX_ARRAY_ELEMENTE);
        } else if (t & typ::B) {
            r.z = Zugriff::B;
            r.laenge = begrenze(quelle_.lese_b(h, nullptr, 0, 0), grenzen::MAX_BYTES);
        } else {
            r = Aufloesung{};
        }
        return r;
    }
    // Array-Element: nur bei Array-Typen, Index gegen die AKTUELLE Länge.
    // Außerhalb der Länge gilt der Name als fehlend — wächst das Array später,
    // findet ihn der nächste Prüfdurchlauf.
    int laenge = -1;
    Zugriff z = Zugriff::FEHLT;
    if (t & typ::VF)      { laenge = quelle_.lese_vf(h, nullptr, 0, 0); z = Zugriff::ELEM_VF; }
    else if (t & typ::VI) { laenge = quelle_.lese_vi(h, nullptr, 0, 0); z = Zugriff::ELEM_VI; }
    else if (t & typ::B)  { laenge = quelle_.lese_b(h, nullptr, 0, 0);  z = Zugriff::ELEM_B; }
    if (z == Zugriff::FEHLT || laenge <= 0 || index >= laenge) return Aufloesung{};
    r.z = z;
    r.laenge = 1;
    return r;
}

size_t Dienst::wert_max(const Aufloesung& a) noexcept {
    const size_t n = a.laenge > 0 ? static_cast<size_t>(a.laenge) : 0;
    switch (a.z) {
        case Zugriff::FEHLT:   return 0;
        case Zugriff::I:
        case Zugriff::ELEM_VI:
        case Zugriff::ELEM_B:  return WERT_RAHMEN + JSON_MAX_INT32;
        case Zugriff::F:
        case Zugriff::ELEM_VF: return WERT_RAHMEN + JSON_MAX_FLOAT;
        case Zugriff::D:       return WERT_RAHMEN + JSON_MAX_DOUBLE;
        case Zugriff::VI:      return WERT_RAHMEN + 2 + n * (JSON_MAX_INT32 + 1);
        case Zugriff::VF:      return WERT_RAHMEN + 2 + n * (JSON_MAX_FLOAT + 1);
        case Zugriff::B:       return WERT_RAHMEN + json_max_text(n);
    }
    return 0;
}

namespace {

const char* status_art(uint8_t z) noexcept {
    // Reihenfolge wie Dienst::Zugriff.
    static const char* const ARTEN[] = {
        "fehlt", "i", "f", "d", "vi", "vf", "b", "i", "f", "i",
    };
    return z < sizeof(ARTEN) / sizeof(ARTEN[0]) ? ARTEN[z] : "fehlt";
}

}  // namespace

bool Dienst::uebernehme_pruefung(Abo& abo, bool* geaendert) noexcept {
    *geaendert = false;
    size_t werte_bytes = 0;
    const size_t n = abo.eintraege.anzahl();
    for (size_t i = 0; i < n; ++i) {
        Eintrag& e = abo.eintraege[i];
        const bool status_gleich =
            std::strcmp(status_art(static_cast<uint8_t>(e.aktiv.z)),
                        status_art(static_cast<uint8_t>(e.kandidat.z))) == 0 &&
            e.aktiv.laenge == e.kandidat.laenge;
        if (!status_gleich) *geaendert = true;
        e.aktiv = e.kandidat;
        werte_bytes += wert_max(e.aktiv);
    }
    const size_t status_bytes = n * STATUS_MAX;
    const size_t noetig = werte_bytes > status_bytes ? werte_bytes : status_bytes;
    // Wachsen nur beim Übernehmen (= Neu-Anmelden im Sinne der ADR), nie beim
    // Lesen. Schrumpfen lohnt nicht.
    return abo.stapel.reserviere(noetig, n);
}

bool Dienst::bereite_antwort(Abo& abo) noexcept {
    abo.stapel.leeren();
    const size_t n = abo.eintraege.anzahl();
    for (size_t k = 0; k < n; ++k) {
        const Eintrag& e = abo.eintraege[k];
        JsonSchreiber w = abo.stapel.schreiber();
        w.zeichen('[');
        w.ganzzahl(static_cast<int64_t>(k));
        w.roh(",\"");
        w.roh(status_art(static_cast<uint8_t>(e.aktiv.z)));
        w.zeichen('"');
        if (e.aktiv.z != Zugriff::FEHLT) {
            w.zeichen(',');
            w.ganzzahl(e.aktiv.laenge);
        }
        w.zeichen(']');
        if (!abo.stapel.uebernehme(w)) return false;
    }
    JsonSchreiber p(abo.praefix, sizeof(abo.praefix));
    p.roh("{\"p\":2,\"t\":\"abo\",\"abo\":");
    p.ganzzahl(abo.id);
    if (p.ueberlauf()) return false;
    abo.praefix_laenge = p.laenge();
    abo.feld = "st";
    abo.teile = plane_pakete(abo.praefix_laenge, 2, abo.stapel,
                             abo.grenzen.daten(), abo.grenzen.kapazitaet(),
                             grenzen::MAX_PAKET);
    if (abo.teile == 0) return false;
    abo.paket_cursor = 0;
    abo.phase = Phase::ANTWORT;
    return true;
}

void Dienst::starte_runde(Abo& abo) noexcept {
    ++abo.seq;
    abo.stapel.leeren();
    abo.lese_cursor = 0;
    abo.phase = Phase::LESEN;
}

bool Dienst::schreibe_wert(JsonSchreiber& w, const Eintrag& e, uint32_t k) noexcept {
    const DatarefHandle h = e.aktiv.h;
    const int laenge = e.aktiv.laenge;
    w.zeichen('[');
    w.ganzzahl(k);
    w.zeichen(',');
    switch (e.aktiv.z) {
        case Zugriff::FEHLT:
            return false;  // ein fehlender Name liefert nie einen Wert
        case Zugriff::I:
            w.ganzzahl(quelle_.lese_i(h));
            break;
        case Zugriff::F:
            w.zahl_f(quelle_.lese_f(h));
            break;
        case Zugriff::D:
            w.zahl_d(quelle_.lese_d(h));
            break;
        case Zugriff::VI: {
            const int n = begrenze(quelle_.lese_vi(h, iwerte_, 0, laenge), laenge);
            w.zeichen('[');
            for (int i = 0; i < n; ++i) {
                if (i > 0) w.zeichen(',');
                w.ganzzahl(iwerte_[i]);
            }
            w.zeichen(']');
            break;
        }
        case Zugriff::VF: {
            const int n = begrenze(quelle_.lese_vf(h, fwerte_, 0, laenge), laenge);
            w.zeichen('[');
            for (int i = 0; i < n; ++i) {
                if (i > 0) w.zeichen(',');
                w.zahl_f(fwerte_[i]);
            }
            w.zeichen(']');
            break;
        }
        case Zugriff::B: {
            const int n = begrenze(quelle_.lese_b(h, bytes_, 0, laenge), laenge);
            // Zeichenkette bis zum ersten NUL (ADR).
            w.text_bis_nul(reinterpret_cast<const char*>(bytes_), static_cast<size_t>(n));
            break;
        }
        case Zugriff::ELEM_VI:
            if (quelle_.lese_vi(h, iwerte_, e.index, 1) != 1) return false;
            w.ganzzahl(iwerte_[0]);
            break;
        case Zugriff::ELEM_VF:
            if (quelle_.lese_vf(h, fwerte_, e.index, 1) != 1) return false;
            w.zahl_f(fwerte_[0]);
            break;
        case Zugriff::ELEM_B:
            if (quelle_.lese_b(h, bytes_, e.index, 1) != 1) return false;
            w.ganzzahl(bytes_[0]);
            break;
    }
    w.zeichen(']');
    return !w.ueberlauf();
}

bool Dienst::lese_schritt(Abo& abo) noexcept {
    const uint32_t n = static_cast<uint32_t>(abo.eintraege.anzahl());
    while (abo.lese_cursor < n) {
        const Eintrag& e = abo.eintraege[abo.lese_cursor];
        if (e.aktiv.z != Zugriff::FEHLT) {
            if (!darf_arbeiten()) return false;
            JsonSchreiber w = abo.stapel.schreiber();
            // Ein Element, das nicht gelesen werden konnte (Array inzwischen
            // kürzer) oder wider Erwarten nicht passt, fällt in dieser Runde
            // aus — es wird nie abgeschnitten.
            if (schreibe_wert(w, e, abo.lese_cursor)) abo.stapel.uebernehme(w);
        }
        ++abo.lese_cursor;
    }
    return true;
}

bool Dienst::sende_schritt(Abo& abo) noexcept {
    while (abo.paket_cursor < abo.teile) {
        if (!darf_arbeiten()) return false;
        const size_t len = schreibe_paket(paket_, sizeof(paket_),
                                          abo.praefix, abo.praefix_laenge, abo.feld,
                                          abo.stapel, abo.grenzen.daten(),
                                          abo.paket_cursor, abo.teile);
        if (len == 0) {
            // Planung und Schreiben sind aus einer Formel — das darf nicht
            // passieren. Falls doch: Paket auslassen statt hängen bleiben.
            ++abo.paket_cursor;
            continue;
        }
        if (sende_paket(paket_, len) == SendeErgebnis::VOLL) return false;
        ++abo.paket_cursor;
    }
    return true;
}

void Dienst::abo_verwerfen_mit_fehler(Abo& abo, Fehlergrund grund) noexcept {
    const uint32_t id = abo.id;
    protokolliere("Protokoll 2: Abo %u verworfen (%s)", static_cast<unsigned>(id),
                  fehlergrund_text(grund));
    abo_leeren(abo);
    sende_fehler(client_, grund, 0, id, -1);
}

void Dienst::bearbeite_abo_frame(Abo& abo) noexcept {
    if (abo.phase == Phase::LEER) return;

    // 1. Prüfdurchlauf alle 2 s anstoßen.
    if (!abo.pruefung_laeuft && !abo.pruefung_fertig && jetzt_ >= abo.naechste_pruefung) {
        starte_pruefung_neu(abo);
    }

    // 2. Prüfschritte — höchstens 64 Suchen je Frame über ALLE Abos.
    while (abo.pruefung_laeuft && suchen_frame_ < grenzen::MAX_SUCHEN_JE_FRAME) {
        if (!darf_arbeiten()) break;
        Eintrag& e = abo.eintraege[abo.pruef_cursor];
        e.kandidat = loese_auf(abo.text.daten() + e.name_ofs, e.index);
        ++suchen_frame_;
        if (++abo.pruef_cursor >= abo.eintraege.anzahl()) {
            abo.pruefung_laeuft = false;
            abo.pruefung_fertig = true;
            abo.naechste_pruefung = jetzt_ + grenzen::NACHSUCHE_INTERVALL_S;
        }
    }

    // 3. Fertigen Durchlauf zwischen zwei Runden übernehmen.
    if (abo.pruefung_fertig && (abo.phase == Phase::NEU || abo.phase == Phase::BEREIT)) {
        abo.pruefung_fertig = false;
        bool geaendert = false;
        if (!uebernehme_pruefung(abo, &geaendert)) {
            abo_verwerfen_mit_fehler(abo, Fehlergrund::SPEICHER);
            return;
        }
        if (geaendert || abo.erste_antwort) {
            abo.erste_antwort = false;
            if (!bereite_antwort(abo)) {
                abo_verwerfen_mit_fehler(abo, Fehlergrund::SPEICHER);
                return;
            }
        }
    }

    // 4. Zustand weiterschalten. Die Fälle fallen absichtlich durch: ein
    //    kleines Abo erledigt Lesen und Senden im selben Frame.
    switch (abo.phase) {
        case Phase::LEER:
        case Phase::NEU:
            return;
        case Phase::ANTWORT:
            if (!sende_schritt(abo)) return;
            abo.phase = Phase::BEREIT;
            abo.faellig = jetzt_;  // Werte sofort, nicht erst nach 1/rate
            [[fallthrough]];
        case Phase::BEREIT:
            if (jetzt_ < abo.faellig) return;
            starte_runde(abo);
            // Nächster Termin vom Soll aus, damit die Rate im Mittel stimmt;
            // hinkt der Sim hinterher, kein Nachholen im Stoß.
            abo.faellig += abo.periode;
            if (abo.faellig <= jetzt_) abo.faellig = jetzt_ + abo.periode;
            [[fallthrough]];
        case Phase::LESEN: {
            if (!lese_schritt(abo)) return;
            JsonSchreiber p(abo.praefix, sizeof(abo.praefix));
            p.roh("{\"p\":2,\"t\":\"w\",\"abo\":");
            p.ganzzahl(abo.id);
            p.roh(",\"seq\":");
            p.ganzzahl(abo.seq);
            abo.praefix_laenge = p.laenge();
            abo.feld = "v";
            abo.teile = plane_pakete(abo.praefix_laenge, 1, abo.stapel,
                                     abo.grenzen.daten(), abo.grenzen.kapazitaet(),
                                     grenzen::MAX_PAKET);
            if (abo.teile == 0) {
                abo_verwerfen_mit_fehler(abo, Fehlergrund::SPEICHER);
                return;
            }
            abo.paket_cursor = 0;
            abo.phase = Phase::SENDEN;
            [[fallthrough]];
        }
        case Phase::SENDEN:
            if (!sende_schritt(abo)) return;
            abo.phase = Phase::BEREIT;
            return;
    }
}

// =============================================================================
// LISTE
// =============================================================================

void Dienst::liste_schritt() noexcept {
    if (!liste_.aktiv) return;

    if (liste_.sammeln) {
        while (liste_.cursor < liste_.gesamt) {
            if (!darf_arbeiten()) return;
            int block = liste_.gesamt - liste_.cursor;
            if (block > grenzen::LISTE_BLOCK) block = grenzen::LISTE_BLOCK;
            const int geholt = begrenze(quelle_.datarefs_ab(liste_.cursor, block, handles_), block);
            if (geholt == 0) {
                // Weniger Datarefs als gemeldet — mit dem Bestand weitermachen.
                liste_.gesamt = liste_.cursor;
                break;
            }
            for (int j = 0; j < geholt; ++j) {
                const char* name = handles_[j] ? quelle_.name_von(handles_[j]) : nullptr;
                const size_t n = name ? text_laenge(name, grenzen::MAX_ZEILE + 1) : 0;
                // Nur abonnierbare Namen melden: was nicht durch den
                // ABO-Parser käme, nützt dem Client nichts.
                if (!ist_gueltiger_name(name, n)) {
                    ++liste_.ausgelassen;
                    continue;
                }
                if (!liste_.namen.sorge_fuer_platz(json_max_text(n))) {
                    const uint32_t id = liste_.id;
                    liste_leeren();
                    sende_fehler(client_, Fehlergrund::SPEICHER, 0, 0, id);
                    return;
                }
                JsonSchreiber w = liste_.namen.schreiber();
                w.text(name, n);
                liste_.namen.uebernehme(w);
            }
            liste_.cursor += geholt;
        }
        // Gesammelt: planen.
        const size_t anzahl = liste_.namen.anzahl();
        JsonSchreiber p(liste_.praefix, sizeof(liste_.praefix));
        p.roh("{\"p\":2,\"t\":\"liste\",\"id\":");
        p.ganzzahl(liste_.id);
        liste_.praefix_laenge = p.laenge();
        if (!liste_.grenzen.reserviere(anzahl + 2)) {
            const uint32_t id = liste_.id;
            liste_leeren();
            sende_fehler(client_, Fehlergrund::SPEICHER, 0, 0, id);
            return;
        }
        liste_.teile = plane_pakete(liste_.praefix_laenge, 1, liste_.namen,
                                    liste_.grenzen.daten(), liste_.grenzen.kapazitaet(),
                                    grenzen::MAX_PAKET);
        if (liste_.teile == 0) {
            const uint32_t id = liste_.id;
            liste_leeren();
            sende_fehler(client_, Fehlergrund::SPEICHER, 0, 0, id);
            return;
        }
        protokolliere("Protokoll 2: LISTE %u - %u Namen in %u Paketen (%u nicht abonnierbar ausgelassen)",
                      static_cast<unsigned>(liste_.id), static_cast<unsigned>(anzahl),
                      static_cast<unsigned>(liste_.teile),
                      static_cast<unsigned>(liste_.ausgelassen));
        liste_.sammeln = false;
        liste_.paket_cursor = 0;
    }

    while (liste_.paket_cursor < liste_.teile) {
        if (!darf_arbeiten()) return;
        const size_t len = schreibe_paket(paket_, sizeof(paket_),
                                          liste_.praefix, liste_.praefix_laenge, "n",
                                          liste_.namen, liste_.grenzen.daten(),
                                          liste_.paket_cursor, liste_.teile);
        if (len != 0 && sende_paket(paket_, len) == SendeErgebnis::VOLL) return;
        ++liste_.paket_cursor;
    }
    liste_leeren();  // fertig: Speicher sofort zurückgeben
}

// =============================================================================
// Flugzeug
// =============================================================================

bool Dienst::lies_kennung(DatarefHandle* h, const char* name, char* aus, size_t kap) noexcept {
    aus[0] = '\0';
    if (*h == nullptr) *h = quelle_.finde(name);  // billig; bis er da ist
    if (*h == nullptr) return false;
    const int max = static_cast<int>(kap - 1);
    // In den großzügigen Arbeitspuffer lesen, dann begrenzt kopieren.
    const int n = begrenze(quelle_.lese_b(*h, bytes_, 0, max), max);
    std::memcpy(aus, bytes_, static_cast<size_t>(n));
    aus[n] = '\0';
    // Zeichenkette endet am ersten NUL (text_laenge in der JSON-Ausgabe).
    return true;
}

void Dienst::pruefe_flugzeug() noexcept {
    if (!flugzeug_offen_ && jetzt_ < naechste_flugzeug_pruefung_) return;
    naechste_flugzeug_pruefung_ = jetzt_ + grenzen::FLUGZEUG_PRUEFINTERVALL_S;

    char icao[sizeof(gesendet_icao_)];
    char titel[sizeof(gesendet_titel_)];
    char pfad[sizeof(gesendet_pfad_)];
    bool hat[3];
    hat[0] = lies_kennung(&h_icao_, "sim/aircraft/view/acf_ICAO", icao, sizeof(icao));
    hat[1] = lies_kennung(&h_titel_, "sim/aircraft/view/acf_descrip", titel, sizeof(titel));
    hat[2] = lies_kennung(&h_pfad_, "sim/aircraft/view/acf_relative_path", pfad, sizeof(pfad));

    const bool gleich = gesendet_gueltig_ &&
                        hat[0] == gesendet_hat_[0] && hat[1] == gesendet_hat_[1] &&
                        hat[2] == gesendet_hat_[2] &&
                        std::strcmp(icao, gesendet_icao_) == 0 &&
                        std::strcmp(titel, gesendet_titel_) == 0 &&
                        std::strcmp(pfad, gesendet_pfad_) == 0;
    if (gleich && !flugzeug_offen_) return;

    JsonSchreiber w(paket_, sizeof(paket_));
    auto feld = [&w](const char* schluessel, bool vorhanden, const char* wert, size_t max) noexcept {
        w.roh(schluessel);
        if (vorhanden) w.text_bis_nul(wert, max); else w.null();
    };
    for (int versuch = 0; versuch < 2; ++versuch) {
        w.zurueck_zu(0);
        w.roh("{\"p\":2,\"t\":\"flugzeug\"");
        feld(",\"icao\":", hat[0], icao, sizeof(icao));
        feld(",\"titel\":", hat[1], titel, sizeof(titel));
        // Im (theoretischen) Fall, dass alles maskiert nicht in 8 KiB passt,
        // lieber den Pfad weglassen als die Meldung.
        feld(",\"pfad\":", hat[2] && versuch == 0, pfad, sizeof(pfad));
        w.roh("}\n");
        if (!w.ueberlauf()) break;
    }
    if (w.ueberlauf()) return;
    if (umgebung_.sende(client_, w.daten(), w.laenge()) == SendeErgebnis::VOLL) {
        return;  // im nächsten Prüftakt erneut
    }
    std::memcpy(gesendet_icao_, icao, sizeof(icao));
    std::memcpy(gesendet_titel_, titel, sizeof(titel));
    std::memcpy(gesendet_pfad_, pfad, sizeof(pfad));
    for (int i = 0; i < 3; ++i) gesendet_hat_[i] = hat[i];
    gesendet_gueltig_ = true;
    flugzeug_offen_ = false;
}

}  // namespace aeroacars
