// =============================================================================
// AeroACARS X-Plane-Plugin — eingebettete Schriften (Open Sans, SIL OFL 1.1)
// =============================================================================
//
// Die Bytes erzeugt CMake beim Konfigurieren aus third_party/opensans/*.ttf
// (werkzeuge/schrift_einbetten.cmake → <build>/generiert/schrift_daten.cpp).
// Lizenz: third_party/opensans/OFL.txt (wird mit dem Plugin ausgeliefert).
// =============================================================================

#pragma once

#include <cstddef>

namespace aeroacars {
namespace schrift_daten {
extern const unsigned char OPENSANS_REGULAR[];
extern const size_t OPENSANS_REGULAR_GROESSE;
extern const unsigned char OPENSANS_SEMIBOLD[];
extern const size_t OPENSANS_SEMIBOLD_GROESSE;
extern const unsigned char OPENSANS_BOLD[];
extern const size_t OPENSANS_BOLD_GROESSE;
}  // namespace schrift_daten
}  // namespace aeroacars
