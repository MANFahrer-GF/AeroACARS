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
};

class ScheinUmgebung;

class ScheinWelt final : public aeroacars::Datenquelle {
public:
    std::vector<std::unique_ptr<ScheinRef>> refs;
    bool liste_da = true;
    ScheinUmgebung* uhr = nullptr;  // für langsame Lesezugriffe
    double lese_kosten = 0.0;       // Sekunden je Lesezugriff
    int lesezugriffe = 0;
    int suchen = 0;
    int ueberschuss = 0;            // schreibt so viele Werte MEHR als erbeten

    ScheinRef& neu(const std::string& name, int typen) {
        refs.emplace_back(new ScheinRef());
        refs.back()->name = name;
        refs.back()->typen = typen;
        return *refs.back();
    }
    ScheinRef* such(const std::string& name) {
        for (auto& r : refs) if (r->name == name) return r.get();
        return nullptr;
    }

    aeroacars::DatarefHandle finde(const char* name) noexcept override {
        ++suchen;
        for (auto& r : refs) {
            if (r->registriert && r->name == name) return r.get();
        }
        return nullptr;
    }
    bool ist_gueltig(aeroacars::DatarefHandle h) noexcept override {
        return static_cast<ScheinRef*>(h)->gueltig;
    }
    int typen(aeroacars::DatarefHandle h) noexcept override {
        return static_cast<ScheinRef*>(h)->typen;
    }
    int lese_i(aeroacars::DatarefHandle h) noexcept override { lies(); return r(h)->i; }
    float lese_f(aeroacars::DatarefHandle h) noexcept override { lies(); return r(h)->f; }
    double lese_d(aeroacars::DatarefHandle h) noexcept override { lies(); return r(h)->d; }
    int lese_vi(aeroacars::DatarefHandle h, int* aus, int ab, int max) noexcept override {
        return kopiere(r(h)->vi, aus, ab, max);
    }
    int lese_vf(aeroacars::DatarefHandle h, float* aus, int ab, int max) noexcept override {
        return kopiere(r(h)->vf, aus, ab, max);
    }
    int lese_b(aeroacars::DatarefHandle h, void* aus, int ab, int max) noexcept override {
        const std::string& s = r(h)->b;
        if (aus == nullptr) return static_cast<int>(s.size());
        lies();
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
    const char* name_von(aeroacars::DatarefHandle h) noexcept override {
        return static_cast<ScheinRef*>(h)->name.c_str();
    }

private:
    static ScheinRef* r(aeroacars::DatarefHandle h) { return static_cast<ScheinRef*>(h); }
    void lies() noexcept;
    template <typename T>
    int kopiere(const std::vector<T>& v, T* aus, int ab, int max) noexcept {
        if (aus == nullptr) return static_cast<int>(v.size());
        lies();
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

    aeroacars::SendeErgebnis sende(const aeroacars::Absender& an, const char* daten,
                                   size_t laenge) noexcept override {
        if (voll_noch > 0) { --voll_noch; return aeroacars::SendeErgebnis::VOLL; }
        if (fehler_noch > 0) { --fehler_noch; return aeroacars::SendeErgebnis::FEHLER; }
        gesendet.emplace_back(an, std::string(daten, laenge));
        return aeroacars::SendeErgebnis::OK;
    }
    double jetzt() noexcept override {
        zeit += tick_je_uhrabfrage;
        return zeit;
    }
    void protokolliere(const char* zeile) noexcept override { log.emplace_back(zeile); }
};

inline void ScheinWelt::lies() noexcept {
    ++lesezugriffe;
    if (uhr != nullptr) uhr->zeit += lese_kosten;
}
