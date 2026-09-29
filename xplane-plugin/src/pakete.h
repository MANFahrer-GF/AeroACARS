// =============================================================================
// AeroACARS X-Plane-Plugin — Aufteilung großer Antworten in Pakete ≤ 8 KiB
// =============================================================================
//
// Jede Antwort mit einer Liste (abo-Status, Werte, LISTE-Namen) hat die Form
//
//   <präfix>,"teil":<k>,"teile":<n>,"<feld>":[<e1>,<e2>,…]}\n
//
// z. B. präfix = {"p":2,"t":"w","abo":1,"seq":812   und feld = v.
//
// Ablauf in zwei Schritten, weil "teile" schon im ERSTEN Paket stehen muss:
//
//   1. Die Elemente werden fertig serialisiert in einer ElementListe gesammelt
//      (ein zusammenhängender Puffer + Endpositionen).
//   2. plane_pakete() legt die Grenzen fest (gierig: so viele Elemente wie
//      passen), schreibe_paket() baut Paket k.
//
// Garantien (Unit-Tests in tests/test_pakete.cpp):
//   * Kein Paket ist größer als die Grenze (8192 Byte inkl. '\n').
//   * Ein Element wird nie geteilt — es steht ganz in genau einem Paket.
//   * Ein Paket wird genau bis an die Grenze gefüllt, wenn es passt.
//   * Passt ein einzelnes Element selbst in ein leeres Paket nicht, schlägt die
//     Planung fehl (Rückgabe 0) statt still etwas abzuschneiden.
//   * Eine leere Liste ergibt genau ein Paket mit leerem Array.
//
// XPLM-frei, keine Allokation (die ElementListe reserviert vorab).
// =============================================================================

#pragma once

#include "feld.h"
#include "json_schreiber.h"

#include <cstddef>
#include <cstdint>

namespace aeroacars {

class ElementListe {
public:
    // Reserviert Platz für `bytes` Zeichen und `elemente` Einträge.
    bool reserviere(size_t bytes, size_t elemente) noexcept {
        return text_.reserviere(bytes) && enden_.reserviere(elemente);
    }
    void leeren() noexcept { text_.leeren(); enden_.leeren(); }
    void freigeben() noexcept { text_.freigeben(); enden_.freigeben(); }

    // Schreiber für das nächste Element (schreibt in den freien Rest).
    JsonSchreiber schreiber() noexcept {
        return JsonSchreiber(text_.daten() ? text_.daten() + text_.anzahl() : nullptr,
                             text_.kapazitaet() - text_.anzahl());
    }
    // Übernimmt das mit schreiber() geschriebene Element. false, wenn der
    // Schreiber übergelaufen ist oder die Eintragsliste voll ist — dann ist
    // nichts übernommen.
    bool uebernehme(const JsonSchreiber& w) noexcept;

    // Wie oben, aber wächst bei Bedarf (für LISTE, deren Größe vorab unbekannt
    // ist). `max_element` ist die Höchstlänge des nächsten Elements.
    bool sorge_fuer_platz(size_t max_element) noexcept;

    size_t anzahl() const noexcept { return enden_.anzahl(); }
    size_t bytes() const noexcept { return text_.anzahl(); }
    size_t element_laenge(size_t i) const noexcept {
        const uint32_t anfang = i == 0 ? 0u : enden_[i - 1];
        return enden_[i] - anfang;
    }
    const char* element(size_t i) const noexcept {
        return text_.daten() + (i == 0 ? 0u : enden_[i - 1]);
    }
    size_t kapazitaet_bytes() const noexcept { return text_.kapazitaet(); }
    size_t kapazitaet_elemente() const noexcept { return enden_.kapazitaet(); }

private:
    Feld<char> text_;
    Feld<uint32_t> enden_;
};

// Länge von `wert` in Dezimalziffern.
size_t dezimalstellen(uint32_t wert) noexcept;

// Plant die Aufteilung. `grenzen` braucht Platz für anzahl + 1 Einträge (bei
// leerer Liste 2). Ergebnis: Zahl der Pakete n ≥ 1; grenzen[k] ist der Index
// des ersten Elements von Paket k (0-basiert), grenzen[n] == anzahl.
// Rückgabe 0: ein Element passt allein nicht in ein Paket, oder `grenzen` ist
// zu klein.
uint32_t plane_pakete(size_t praefix_laenge, size_t feld_laenge,
                      const ElementListe& liste,
                      uint32_t* grenzen, size_t grenzen_kapazitaet,
                      size_t max_paket) noexcept;

// Schreibt Paket k (0-basiert) von `teile`. Rückgabe: Länge in Byte, oder 0,
// wenn `aus` zu klein ist (darf bei korrekter Planung nicht passieren).
size_t schreibe_paket(char* aus, size_t aus_kapazitaet,
                      const char* praefix, size_t praefix_laenge,
                      const char* feld,
                      const ElementListe& liste,
                      const uint32_t* grenzen, uint32_t k, uint32_t teile) noexcept;

// Länge eines Pakets mit den gegebenen Bestandteilen — die eine Formel, die
// Planung und Schreiben teilen.
size_t paket_laenge(size_t praefix_laenge, size_t feld_laenge,
                    uint32_t teil, uint32_t teile,
                    size_t element_bytes, size_t element_anzahl) noexcept;

}  // namespace aeroacars
