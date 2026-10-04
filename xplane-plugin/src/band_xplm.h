// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band an XPLM angebunden (ADR-0005)
// =============================================================================
//
// Fenster, Zeichnung, Maus, Menü und Merken. Die Logik (Parser, Frist, Stufen,
// Prefs-Text, Klemmung) liegt in band.cpp und ist dort ohne X-Plane getestet;
// diese Schicht ist dünn und im Sim zu prüfen. plugin.cpp ruft nur diese drei
// Funktionen auf (Enable/Disable/Flight-Loop).
// =============================================================================

#pragma once

// Legt Menü "Plugins → AeroACARS" und das Band-Fenster an und liest die Prefs.
// Nach dienst_start() aufrufen (das Band liest seinen Zustand aus dem Dienst).
// false = ohne Band weiterlaufen; nichts anderes ist davon betroffen.
bool band_start() noexcept;

// Schreibt offene Änderungen, zerstört Fenster und Menü. Vor dienst_stopp()
// aufrufen. Mehrfacher Aufruf ist harmlos.
void band_stopp() noexcept;

// Einmal je Flight-Loop-Aufruf: Sichtbarkeit des Fensters, verzögertes
// Schreiben der Prefs. Blockiert nie.
void band_frame() noexcept;
