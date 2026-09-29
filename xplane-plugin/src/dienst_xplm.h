// =============================================================================
// AeroACARS X-Plane-Plugin — Protokoll 2 an XPLM und Socket angebunden
// =============================================================================
//
// Dünne Schicht zwischen X-Plane und dem XPLM-freien Dienst (dienst.h):
// Steuer-Socket 127.0.0.1:52001, Uhr, Log, XPLM-Datenzugriff. plugin.cpp ruft
// nur diese vier Funktionen auf.
// =============================================================================

#pragma once

// Startet Protokoll 2. false = aus (Port belegt, kein Speicher); Protokoll 1
// läuft davon unberührt weiter (ADR-0004, Startschutz).
bool dienst_start(const char* plugin_version) noexcept;

// Beendet Protokoll 2 und gibt alles frei. Mehrfacher Aufruf ist harmlos.
void dienst_stopp() noexcept;

// Einmal je Flight-Loop-Aufruf: empfängt höchstens 64 Datagramme, liefert im
// Zeitbudget. Rückgabe true = der Dienst braucht jeden Frame.
bool dienst_frame() noexcept;

// Weitergereicht aus XPluginReceiveMessage.
void dienst_nachricht(int nachricht, void* parameter) noexcept;
