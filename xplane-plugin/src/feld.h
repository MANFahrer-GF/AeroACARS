// =============================================================================
// AeroACARS X-Plane-Plugin — Feld<T>: Speicher ohne Ausnahmen
// =============================================================================
//
// Warum nicht std::vector? Das Plugin ist mit -fno-exceptions gebaut, die
// Standardbibliothek aber nicht: scheitert eine Allokation in std::vector,
// wirft sie std::bad_alloc durch Code ohne Ausnahmebehandlung — das endet in
// std::terminate, also im Absturz von X-Plane. Feld<T> benutzt malloc/realloc
// und meldet ein Scheitern als Rückgabewert; der Aufrufer antwortet dann mit
// fehler/speicher und X-Plane läuft weiter.
//
// Nur für trivial kopierbare Typen (Zahlen, Zeiger, POD-Structs) — es werden
// weder Konstruktoren noch Destruktoren aufgerufen.
//
// XPLM-frei, wird auch von den Tests benutzt.
// =============================================================================

#pragma once

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <type_traits>

namespace aeroacars {

template <typename T>
class Feld {
    static_assert(std::is_trivially_copyable<T>::value,
                  "Feld<T> kopiert mit memcpy/realloc und ruft keine Konstruktoren");

public:
    Feld() = default;
    ~Feld() { freigeben(); }

    Feld(const Feld&) = delete;
    Feld& operator=(const Feld&) = delete;

    // Übernimmt den Speicher eines anderen Felds (das danach leer ist).
    void uebernehme(Feld& andere) noexcept {
        if (&andere == this) return;
        freigeben();
        daten_ = andere.daten_;
        anzahl_ = andere.anzahl_;
        kapazitaet_ = andere.kapazitaet_;
        andere.daten_ = nullptr;
        andere.anzahl_ = 0;
        andere.kapazitaet_ = 0;
    }

    void freigeben() noexcept {
        std::free(daten_);
        daten_ = nullptr;
        anzahl_ = 0;
        kapazitaet_ = 0;
    }

    // Sorgt für Platz für mindestens `n` Elemente. Bestehender Inhalt bleibt.
    bool reserviere(size_t n) noexcept {
        if (n <= kapazitaet_) return true;
        if (n > SIZE_MAX / sizeof(T)) return false;
        void* neu = std::realloc(daten_, n * sizeof(T));
        if (neu == nullptr) return false;  // alter Block bleibt gültig
        daten_ = static_cast<T*>(neu);
        kapazitaet_ = n;
        return true;
    }

    // Hängt ein Element an; wächst bei Bedarf auf das Doppelte.
    bool haenge_an(const T& wert) noexcept {
        if (anzahl_ == kapazitaet_) {
            size_t neu = kapazitaet_ < 16 ? 16 : kapazitaet_ * 2;
            if (neu < kapazitaet_ || !reserviere(neu)) return false;
        }
        daten_[anzahl_++] = wert;
        return true;
    }

    // Hängt `n` Elemente an einem Stück an.
    bool haenge_an(const T* werte, size_t n) noexcept {
        if (n == 0) return true;
        if (anzahl_ + n < anzahl_) return false;
        if (anzahl_ + n > kapazitaet_) {
            size_t neu = kapazitaet_ < 16 ? 16 : kapazitaet_;
            while (neu < anzahl_ + n) {
                if (neu > SIZE_MAX / 2) return false;
                neu *= 2;
            }
            if (!reserviere(neu)) return false;
        }
        std::memcpy(daten_ + anzahl_, werte, n * sizeof(T));
        anzahl_ += n;
        return true;
    }

    // Setzt die Anzahl (nur innerhalb der Kapazität, z. B. nach direktem
    // Schreiben in daten() + anzahl()).
    bool setze_anzahl(size_t n) noexcept {
        if (n > kapazitaet_) return false;
        anzahl_ = n;
        return true;
    }

    void leeren() noexcept { anzahl_ = 0; }

    T* daten() noexcept { return daten_; }
    const T* daten() const noexcept { return daten_; }
    size_t anzahl() const noexcept { return anzahl_; }
    size_t kapazitaet() const noexcept { return kapazitaet_; }
    bool leer() const noexcept { return anzahl_ == 0; }

    // Zugriff ohne Prüfung — Aufrufer prüft gegen anzahl().
    T& operator[](size_t i) noexcept { return daten_[i]; }
    const T& operator[](size_t i) const noexcept { return daten_[i]; }

private:
    T* daten_ = nullptr;
    size_t anzahl_ = 0;
    size_t kapazitaet_ = 0;
};

}  // namespace aeroacars
