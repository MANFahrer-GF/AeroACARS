// =============================================================================
// AeroACARS X-Plane-Plugin — Anfrage-Parser (Protokoll 2)
// =============================================================================
//
// Siehe anfrage.h für Format und Regeln. Grundsätze der Umsetzung:
//
//   * Jeder Zugriff auf das Datagramm läuft über (zeiger, länge)-Paare, die
//     aus [daten, daten + laenge) abgeleitet sind. Es gibt keine
//     NUL-Terminierung und kein strlen — ein Datagramm darf NUL-Bytes
//     enthalten, sie sind dann einfach ungültige Zeichen.
//   * Keine Locale-abhängigen Funktionen (isdigit/isprint/strtol hängen an
//     setlocale(), und ein anderes Plugin im selben Prozess kann das umstellen).
//   * Der erste Fehler bricht ab; die Anfrage gilt als Ganzes als verworfen.
//     Ein halb übernommenes ABO wäre schlimmer als ein abgelehntes.
// =============================================================================

#include "anfrage.h"

#include "grenzen.h"

#include <cstring>

namespace aeroacars {

const char* fehlergrund_text(Fehlergrund grund) noexcept {
    switch (grund) {
        case Fehlergrund::KEINER:                 return "keiner";
        case Fehlergrund::LEERE_ANFRAGE:          return "leere_anfrage";
        case Fehlergrund::DATAGRAMM_ZU_GROSS:     return "datagramm_zu_gross";
        case Fehlergrund::ZEILE_ZU_LANG:          return "zeile_zu_lang";
        case Fehlergrund::UNBEKANNTER_BEFEHL:     return "unbekannter_befehl";
        case Fehlergrund::FALSCHE_ARGUMENTE:      return "falsche_argumente";
        case Fehlergrund::PROTOKOLL_UNGUELTIG:    return "protokoll_ungueltig";
        case Fehlergrund::ABO_ID_UNGUELTIG:       return "abo_id_ungueltig";
        case Fehlergrund::RATE_UNGUELTIG:         return "rate_ungueltig";
        case Fehlergrund::TEIL_UNGUELTIG:         return "teil_ungueltig";
        case Fehlergrund::NAME_UNGUELTIG:         return "name_ungueltig";
        case Fehlergrund::INDEX_UNGUELTIG:        return "index_ungueltig";
        case Fehlergrund::ZU_VIELE_NAMEN:         return "zu_viele_namen";
        case Fehlergrund::UEBERZAEHLIGE_ZEILEN:   return "ueberzaehlige_zeilen";
        case Fehlergrund::KEIN_HALLO:             return "kein_hallo";
        case Fehlergrund::ABO_TEIL_REIHENFOLGE:   return "abo_teil_reihenfolge";
        case Fehlergrund::ABO_TEILE_WIDERSPRUCH:  return "abo_teile_widerspruch";
        case Fehlergrund::KEINE_NAMEN:            return "keine_namen";
        case Fehlergrund::LISTE_NICHT_VERFUEGBAR: return "liste_nicht_verfuegbar";
        case Fehlergrund::SPEICHER:               return "speicher";
        case Fehlergrund::GENERATION_UNGUELTIG:   return "generation_ungueltig";
        case Fehlergrund::SPEICHER_LIMIT:         return "speicher_limit";
    }
    return "unbekannt";
}

namespace {

// Druckbares ASCII ohne Leerzeichen.
inline bool ist_namenszeichen(unsigned char c) noexcept {
    return c >= 0x21 && c <= 0x7E;
}

inline bool ist_ziffer(char c) noexcept {
    return c >= '0' && c <= '9';
}

// Dezimalzahl aus 1..10 Ziffern, Wert ≤ 2^32 − 1. Kein Vorzeichen, keine
// Leerzeichen, keine Locale.
bool lies_zahl(const char* p, size_t n, uint32_t* out) noexcept {
    if (n == 0 || n > 10) return false;
    uint64_t wert = 0;
    for (size_t i = 0; i < n; ++i) {
        if (!ist_ziffer(p[i])) return false;
        wert = wert * 10u + static_cast<uint64_t>(p[i] - '0');
    }
    if (wert > 0xFFFFFFFFull) return false;
    *out = static_cast<uint32_t>(wert);
    return true;
}

struct Stueck {
    const char* p = nullptr;
    size_t n = 0;
};

bool gleich(const Stueck& s, const char* wort) noexcept {
    const size_t n = std::strlen(wort);
    return s.n == n && std::memcmp(s.p, wort, n) == 0;
}

// Liest die nächste Zeile ab *pos. Ein '\r' am Zeilenende wird abgeschnitten.
// Gibt false zurück, wenn nichts mehr da ist.
bool naechste_zeile(const char* daten, size_t laenge, size_t* pos, Stueck* zeile) noexcept {
    if (*pos >= laenge) return false;
    const char* anfang = daten + *pos;
    const size_t rest = laenge - *pos;
    const void* nl = std::memchr(anfang, '\n', rest);
    size_t n;
    if (nl != nullptr) {
        n = static_cast<size_t>(static_cast<const char*>(nl) - anfang);
        *pos += n + 1;
    } else {
        n = rest;
        *pos = laenge;
    }
    if (n > 0 && anfang[n - 1] == '\r') --n;
    zeile->p = anfang;
    zeile->n = n;
    return true;
}

// Zerlegt die Befehlszeile an einzelnen Leerzeichen. Höchstens `max` Wörter;
// leere Wörter (doppelte, führende, abschließende Leerzeichen) sind ungültig.
// Rückgabe: Wortzahl, 0 bei Formfehler, max+1 bei zu vielen Wörtern.
size_t zerlege_woerter(const Stueck& zeile, Stueck* woerter, size_t max) noexcept {
    size_t anzahl = 0;
    size_t i = 0;
    while (i <= zeile.n) {
        size_t j = i;
        while (j < zeile.n && zeile.p[j] != ' ') ++j;
        if (j == i) return 0;  // leeres Wort
        if (anzahl == max) return max + 1;
        woerter[anzahl].p = zeile.p + i;
        woerter[anzahl].n = j - i;
        ++anzahl;
        if (j == zeile.n) break;
        i = j + 1;  // hinter dem Leerzeichen; i == n heißt: Leerzeichen am Ende
    }
    return anzahl;
}

bool nur_namenszeichen(const Stueck& s) noexcept {
    for (size_t i = 0; i < s.n; ++i) {
        if (!ist_namenszeichen(static_cast<unsigned char>(s.p[i]))) return false;
    }
    return true;
}

// Nur Zeilenenden im Rest? (für die Unterscheidung leere_anfrage)
bool nur_zeilenenden(const char* daten, size_t laenge, size_t pos) noexcept {
    for (size_t i = pos; i < laenge; ++i) {
        if (daten[i] != '\n' && daten[i] != '\r') return false;
    }
    return true;
}

bool setze_fehler(Anfrage* out, Fehlergrund grund, uint32_t zeile) noexcept {
    out->fehler = grund;
    out->fehler_zeile = zeile;
    out->namen_anzahl = 0;
    return false;
}

// Zerlegt eine Namenszeile in Basis + optionalen Index.
Fehlergrund zerlege_name(const Stueck& zeile, NameRef* name) noexcept {
    if (zeile.n == 0 || zeile.n > grenzen::MAX_ZEILE) return Fehlergrund::NAME_UNGUELTIG;
    if (!nur_namenszeichen(zeile)) return Fehlergrund::NAME_UNGUELTIG;

    size_t basis_n = zeile.n;
    int32_t index = -1;
    if (zeile.p[zeile.n - 1] == ']') {
        // Letztes '[' suchen. Ohne '[' ist "name]" ein kaputter Index.
        size_t auf = zeile.n - 1;
        bool gefunden = false;
        while (auf > 0) {
            --auf;
            if (zeile.p[auf] == '[') { gefunden = true; break; }
        }
        if (!gefunden) return Fehlergrund::INDEX_UNGUELTIG;
        const char* ziffern = zeile.p + auf + 1;
        const size_t ziffern_n = zeile.n - 1 - (auf + 1);
        uint32_t wert = 0;
        if (!lies_zahl(ziffern, ziffern_n, &wert) || wert > grenzen::MAX_INDEX) {
            return Fehlergrund::INDEX_UNGUELTIG;
        }
        if (auf == 0) return Fehlergrund::NAME_UNGUELTIG;  // "[3]" ohne Namen
        basis_n = auf;
        index = static_cast<int32_t>(wert);
    }
    name->basis = zeile.p;
    name->basis_laenge = static_cast<uint16_t>(basis_n);
    name->index = index;
    return Fehlergrund::KEINER;
}

}  // namespace

bool ist_abonnierbarer_name(const char* name, size_t laenge) noexcept {
    // Ein Name, der auf ']' endet, würde im ABO als Array-Element gelesen
    // ("a[3]" = Element 3 von "a") oder als kaputter Index — als ganzer Name
    // ist er nicht abonnierbar.
    return ist_gueltiger_name(name, laenge) && name[laenge - 1] != ']';
}

bool ist_gueltiger_name(const char* name, size_t laenge) noexcept {
    if (name == nullptr || laenge == 0 || laenge > grenzen::MAX_ZEILE) return false;
    for (size_t i = 0; i < laenge; ++i) {
        if (!ist_namenszeichen(static_cast<unsigned char>(name[i]))) return false;
    }
    return true;
}

bool zerlege_anfrage(const char* daten, size_t laenge,
                     NameRef* namen_puffer, size_t namen_kapazitaet,
                     Anfrage* out) noexcept {
    *out = Anfrage{};
    if (laenge > grenzen::MAX_ANFRAGE_BYTES) {
        return setze_fehler(out, Fehlergrund::DATAGRAMM_ZU_GROSS, 0);
    }
    if (daten == nullptr || laenge == 0) {
        return setze_fehler(out, Fehlergrund::LEERE_ANFRAGE, 0);
    }

    size_t pos = 0;
    Stueck zeile;
    naechste_zeile(daten, laenge, &pos, &zeile);  // laenge > 0 → immer eine Zeile
    if (zeile.n > grenzen::MAX_ZEILE) {
        return setze_fehler(out, Fehlergrund::ZEILE_ZU_LANG, 1);
    }
    if (zeile.n == 0) {
        return setze_fehler(out,
                            nur_zeilenenden(daten, laenge, pos)
                                ? Fehlergrund::LEERE_ANFRAGE
                                : Fehlergrund::UNBEKANNTER_BEFEHL,
                            1);
    }

    constexpr size_t MAX_WOERTER = 6;  // ABO id rate teil teile g<n>
    Stueck w[MAX_WOERTER];
    // Erst das Befehlswort allein prüfen: ein unlesbares erstes Wort ist ein
    // unbekannter Befehl, kein Argumentfehler.
    {
        size_t j = 0;
        while (j < zeile.n && zeile.p[j] != ' ') ++j;
        Stueck befehl{zeile.p, j};
        if (befehl.n == 0 || !nur_namenszeichen(befehl)) {
            return setze_fehler(out, Fehlergrund::UNBEKANNTER_BEFEHL, 1);
        }
        if (gleich(befehl, "HALLO"))         out->befehl = Befehl::HALLO;
        else if (gleich(befehl, "ABO"))      out->befehl = Befehl::ABO;
        else if (gleich(befehl, "ENDE-ABO")) out->befehl = Befehl::ENDE_ABO;
        else if (gleich(befehl, "LISTE"))    out->befehl = Befehl::LISTE;
        else if (gleich(befehl, "PING"))     out->befehl = Befehl::PING;
        else return setze_fehler(out, Fehlergrund::UNBEKANNTER_BEFEHL, 1);
    }
    const size_t n_woerter = zerlege_woerter(zeile, w, MAX_WOERTER);
    if (n_woerter == 0 || n_woerter > MAX_WOERTER) {
        out->befehl = Befehl::KEINER;
        return setze_fehler(out, Fehlergrund::FALSCHE_ARGUMENTE, 1);
    }
    for (size_t i = 1; i < n_woerter; ++i) {
        if (!nur_namenszeichen(w[i])) {
            out->befehl = Befehl::KEINER;
            return setze_fehler(out, Fehlergrund::FALSCHE_ARGUMENTE, 1);
        }
    }

    const Befehl befehl = out->befehl;
    // Bei jedem Fehler unten wird der Befehl zurückgesetzt: eine verworfene
    // Anfrage darf nie wie eine gültige aussehen.
    auto fehler = [out](Fehlergrund g, uint32_t z) noexcept {
        out->befehl = Befehl::KEINER;
        return setze_fehler(out, g, z);
    };

    switch (befehl) {
        case Befehl::HALLO: {
            if (n_woerter != 3) return fehler(Fehlergrund::FALSCHE_ARGUMENTE, 1);
            uint32_t protokoll = 0;
            if (!lies_zahl(w[1].p, w[1].n, &protokoll) || protokoll == 0) {
                return fehler(Fehlergrund::PROTOKOLL_UNGUELTIG, 1);
            }
            if (w[2].n == 0 || w[2].n > grenzen::MAX_CLIENT_VERSION) {
                return fehler(Fehlergrund::FALSCHE_ARGUMENTE, 1);
            }
            out->protokoll = protokoll;
            out->client_version = w[2].p;
            out->client_version_laenge = w[2].n;
            break;
        }
        case Befehl::ENDE_ABO: {
            if (n_woerter != 2) return fehler(Fehlergrund::FALSCHE_ARGUMENTE, 1);
            uint32_t id = 0;
            if (!lies_zahl(w[1].p, w[1].n, &id) ||
                id < grenzen::MIN_ABO_ID || id > grenzen::MAX_ABO_ID) {
                return fehler(Fehlergrund::ABO_ID_UNGUELTIG, 1);
            }
            out->abo_id = id;
            break;
        }
        case Befehl::LISTE: {
            if (n_woerter != 2) return fehler(Fehlergrund::FALSCHE_ARGUMENTE, 1);
            uint32_t id = 0;
            // Die Anfrage-ID wird als JSON-Zahl zurückgegeben; auf 2^31 − 1
            // begrenzt, damit sie in jedem Client in einen i32 passt.
            if (!lies_zahl(w[1].p, w[1].n, &id) || id > 0x7FFFFFFFu) {
                return fehler(Fehlergrund::FALSCHE_ARGUMENTE, 1);
            }
            out->anfrage_id = id;
            break;
        }
        case Befehl::PING: {
            if (n_woerter != 1) return fehler(Fehlergrund::FALSCHE_ARGUMENTE, 1);
            break;
        }
        case Befehl::ABO: {
            // Optionales letztes Wort "g<zahl>": Generation des Abos (der Client
            // zählt sie bei jeder inhaltlichen Änderung hoch). Wird zuerst
            // gelesen, damit auch Fehlerantworten sie schon tragen.
            size_t n_kopf = n_woerter;
            if (n_kopf >= 4 && w[n_kopf - 1].n >= 1 && w[n_kopf - 1].p[0] == 'g') {
                uint32_t gen = 0;
                if (!lies_zahl(w[n_kopf - 1].p + 1, w[n_kopf - 1].n - 1, &gen) ||
                    gen == 0 || gen > 0x7FFFFFFFu) {
                    return fehler(Fehlergrund::GENERATION_UNGUELTIG, 1);
                }
                out->generation = gen;
                --n_kopf;
            }
            if (n_kopf != 3 && n_kopf != 5) {
                return fehler(Fehlergrund::FALSCHE_ARGUMENTE, 1);
            }
            uint32_t id = 0, rate = 0;
            if (!lies_zahl(w[1].p, w[1].n, &id) ||
                id < grenzen::MIN_ABO_ID || id > grenzen::MAX_ABO_ID) {
                return fehler(Fehlergrund::ABO_ID_UNGUELTIG, 1);
            }
            out->abo_id = id;  // gleich setzen: Fehlerantworten nennen das Abo
            if (!lies_zahl(w[2].p, w[2].n, &rate) ||
                rate < grenzen::MIN_RATE_HZ || rate > grenzen::MAX_RATE_HZ) {
                return fehler(Fehlergrund::RATE_UNGUELTIG, 1);
            }
            out->rate_hz = rate;
            if (n_kopf == 5) {
                uint32_t teil = 0, teile = 0;
                if (!lies_zahl(w[3].p, w[3].n, &teil) ||
                    !lies_zahl(w[4].p, w[4].n, &teile) ||
                    teile == 0 || teile > grenzen::MAX_ABO_TEILE ||
                    teil == 0 || teil > teile) {
                    return fehler(Fehlergrund::TEIL_UNGUELTIG, 1);
                }
                out->teil = teil;
                out->teile = teile;
                out->mehrteilig = true;
            }

            // Namenszeilen. Jede Zeile ist ein Name und zählt mit — auch eine
            // ungültige (leer, zu lang, Nicht-ASCII, kaputter Index). Sie
            // verwirft NICHT das ganze Abo, sondern bekommt den Status
            // "fehlt": die Nummerierung bleibt stabil, und ein einzelner
            // seltsamer Name aus einer Flugzeugliste kostet nicht alle anderen.
            size_t kapazitaet = namen_kapazitaet;
            if (kapazitaet > grenzen::MAX_NAMEN_JE_ABO) kapazitaet = grenzen::MAX_NAMEN_JE_ABO;
            size_t anzahl = 0;
            uint32_t zeilennummer = 1;
            while (naechste_zeile(daten, laenge, &pos, &zeile)) {
                ++zeilennummer;
                NameRef name;
                if (zerlege_name(zeile, &name) != Fehlergrund::KEINER) {
                    name = NameRef{};
                    name.ungueltig = true;
                    ++out->ungueltige_namen;
                }
                if (anzahl >= kapazitaet || namen_puffer == nullptr) {
                    return fehler(Fehlergrund::ZU_VIELE_NAMEN, zeilennummer);
                }
                namen_puffer[anzahl++] = name;
            }
            out->namen = namen_puffer;
            out->namen_anzahl = anzahl;
            return true;
        }
        case Befehl::KEINER:
            return fehler(Fehlergrund::UNBEKANNTER_BEFEHL, 1);
    }

    // Einzeilen-Befehle: danach darf nur noch das Datagramm-Ende kommen.
    if (pos < laenge) {
        return fehler(Fehlergrund::UEBERZAEHLIGE_ZEILEN, 2);
    }
    return true;
}

}  // namespace aeroacars
