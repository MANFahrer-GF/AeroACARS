// =============================================================================
// Schein-X-Plane für Tests: Datenquelle + Umgebung ohne XPLM
// =============================================================================
//
// ScheinWelt verhält sich wie XPLM laut SDK-Doku: XPLMFindDataRef liefert
// nullptr für unbekannte Namen, XPLMGetDatav*(nullptr) die Array-Länge,
// verwaiste Datarefs sind "nicht gültig". Zusätzlich lässt sich einstellen,
// was echte Plugins falsch machen können (mehr zurückgeben als erbeten,
// langsame Lesezugriffe).
//
// ScheinUmgebung sammelt alle gesendeten Pakete und hat eine Uhr, die der
// Test vorstellt (optional pro Aufruf ein Stück weiter, für Budget-Tests).
// =============================================================================

#pragma once

#include "dienst.h"

#include <cstring>
#include <memory>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>

struct ScheinRef {
    std::string name;
    int typen = 0;
    int i = 0;
    float f = 0.0f;
    double d = 0.0;
    std::vector<int> vi;
    std::vector<float> vf;
    std::string b;
    bool gueltig = true;       // false = verwaist
    bool registriert = true;   // false = XPLMFindDataRef findet ihn (noch) nicht
    // Teurer FREMDER Accessor (Codex-Abnahme H4): so viele Sekunden kostet
    // jeder Aufruf seines Getters — auch die Längenabfrage (aus == nullptr),
    // denn in XPLM ist das derselbe Rückruf des besitzenden Plugins.
    double kosten = 0.0;
    int getter_aufrufe = 0;    // Werte lesen
    int laengen_aufrufe = 0;   // Array-Länge abfragen
};

class ScheinUmgebung;

class ScheinWelt final : public aeroacars::Datenquelle {
public:
    std::vector<std::unique_ptr<ScheinRef>> refs;
    bool liste_da = true;
    ScheinUmgebung* uhr = nullptr;  // für langsame Lesezugriffe
    double lese_kosten = 0.0;       // Sekunden je Lesezugriff
    double such_kosten = 0.0;       // Sekunden je finde()
    double gueltig_kosten = 0.0;    // Sekunden je ist_gueltig()
    double namen_kosten = 0.0;      // Sekunden je name_von() (LISTE)
    int lesezugriffe = 0;
    int suchen = 0;
    int gueltig_pruefungen = 0;
    int namen_abfragen = 0;
    std::unordered_map<std::string, ScheinRef*> nach_name;
    int ueberschuss = 0;            // schreibt so viele Werte MEHR als erbeten

    ScheinRef& neu(const std::string& name, int typen) {
        refs.emplace_back(new ScheinRef());
        refs.back()->name = name;
        refs.back()->typen = typen;
        nach_name.emplace(name, refs.back().get());
        return *refs.back();
    }
    ScheinRef* such(const std::string& name) {
        for (auto& r : refs) if (r->name == name) return r.get();
        return nullptr;
    }

    // Wie XPLM: Hash-Suche über den Namen.
    aeroacars::DatarefHandle finde(const char* name) noexcept override;
    bool ist_gueltig(aeroacars::DatarefHandle h) noexcept override;
    int typen(aeroacars::DatarefHandle h) noexcept override {
        return static_cast<ScheinRef*>(h)->typen;
    }
    int lese_i(aeroacars::DatarefHandle h) noexcept override { lies(r(h)); return r(h)->i; }
    float lese_f(aeroacars::DatarefHandle h) noexcept override { lies(r(h)); return r(h)->f; }
    double lese_d(aeroacars::DatarefHandle h) noexcept override { lies(r(h)); return r(h)->d; }
    int lese_vi(aeroacars::DatarefHandle h, int* aus, int ab, int max) noexcept override {
        return kopiere(r(h), r(h)->vi, aus, ab, max);
    }
    int lese_vf(aeroacars::DatarefHandle h, float* aus, int ab, int max) noexcept override {
        return kopiere(r(h), r(h)->vf, aus, ab, max);
    }
    int lese_b(aeroacars::DatarefHandle h, void* aus, int ab, int max) noexcept override {
        const std::string& s = r(h)->b;
        if (aus == nullptr) { laenge(r(h)); return static_cast<int>(s.size()); }
        lies(r(h));
        int n = 0;
        for (int k = ab; k >= 0 && k < static_cast<int>(s.size()) && n < max + ueberschuss; ++k) {
            static_cast<char*>(aus)[n++] = s[static_cast<size_t>(k)];
        }
        return n > max ? max : n;
    }
    bool liste_verfuegbar() noexcept override { return liste_da; }
    int anzahl_datarefs() noexcept override { return static_cast<int>(refs.size()); }
    int datarefs_ab(int ab, int anzahl, aeroacars::DatarefHandle* aus) noexcept override {
        int n = 0;
        for (int k = ab; k < ab + anzahl && k < static_cast<int>(refs.size()); ++k) {
            aus[n++] = refs[static_cast<size_t>(k)].get();
        }
        return n;
    }
    const char* name_von(aeroacars::DatarefHandle h) noexcept override;

private:
    static ScheinRef* r(aeroacars::DatarefHandle h) { return static_cast<ScheinRef*>(h); }
    void lies(ScheinRef* ref) noexcept;
    void laenge(ScheinRef* ref) noexcept;
    template <typename T>
    int kopiere(ScheinRef* ref, const std::vector<T>& v, T* aus, int ab, int max) noexcept {
        if (aus == nullptr) { laenge(ref); return static_cast<int>(v.size()); }
        lies(ref);
        int n = 0;
        // `ueberschuss` bildet ein fehlerhaftes Plugin nach, das über max
        // hinaus schreibt — der Dienst muss dafür Reserve im Puffer haben.
        for (int k = ab; k >= 0 && k < static_cast<int>(v.size()) && n < max + ueberschuss; ++k) {
            aus[n++] = v[static_cast<size_t>(k)];
        }
        return n > max ? max : n;
    }
};

class ScheinUmgebung final : public aeroacars::Umgebung {
public:
    double zeit = 1000.0;
    double tick_je_uhrabfrage = 0.0;
    std::vector<std::pair<aeroacars::Absender, std::string>> gesendet;
    std::vector<std::string> log;
    int voll_noch = 0;    // so viele Sendeversuche melden VOLL
    int fehler_noch = 0;  // so viele Sendeversuche melden FEHLER
    int ok_noch = -1;     // ≥ 0: nur noch so viele Pakete gehen durch, danach VOLL
    long am_socket = 0;   // Sendeversuche, die den Socket erreichten (OK + FEHLER)

    aeroacars::SendeErgebnis sende(const aeroacars::Absender& an, const char* daten,
                                   size_t laenge) noexcept override {
        if (voll_noch > 0) { --voll_noch; return aeroacars::SendeErgebnis::VOLL; }
        if (ok_noch == 0) return aeroacars::SendeErgebnis::VOLL;
        ++am_socket;
        if (fehler_noch > 0) { --fehler_noch; return aeroacars::SendeErgebnis::FEHLER; }
        if (ok_noch > 0) --ok_noch;
        gesendet.emplace_back(an, std::string(daten, laenge));
        return aeroacars::SendeErgebnis::OK;
    }
    double jetzt() noexcept override {
        zeit += tick_je_uhrabfrage;
        return zeit;
    }
    void protokolliere(const char* zeile) noexcept override { log.emplace_back(zeile); }
};

inline aeroacars::DatarefHandle ScheinWelt::finde(const char* name) noexcept {
    ++suchen;
    if (uhr != nullptr) uhr->zeit += such_kosten;
    auto it = nach_name.find(name);
    return (it != nach_name.end() && it->second->registriert) ? it->second : nullptr;
}

inline bool ScheinWelt::ist_gueltig(aeroacars::DatarefHandle h) noexcept {
    ++gueltig_pruefungen;
    if (uhr != nullptr) uhr->zeit += gueltig_kosten;
    return static_cast<ScheinRef*>(h)->gueltig;
}

inline void ScheinWelt::lies(ScheinRef* ref) noexcept {
    ++lesezugriffe;
    ++ref->getter_aufrufe;
    if (uhr != nullptr) uhr->zeit += lese_kosten + ref->kosten;
}

inline void ScheinWelt::laenge(ScheinRef* ref) noexcept {
    ++ref->laengen_aufrufe;
    if (uhr != nullptr) uhr->zeit += ref->kosten;
}

inline const char* ScheinWelt::name_von(aeroacars::DatarefHandle h) noexcept {
    ++namen_abfragen;
    if (uhr != nullptr) uhr->zeit += namen_kosten;
    return static_cast<ScheinRef*>(h)->name.c_str();
}
