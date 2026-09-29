// =============================================================================
// AeroACARS X-Plane-Plugin — harte Obergrenzen für Protokoll 2
// =============================================================================
//
// Alle Zahlen an EINER Stelle, weil sie zusammen die Sicherheitsgarantie
// ergeben (ADR-0004, Abschnitt 6): Das Plugin läuft im X-Plane-Prozess. Jede
// Eingabe von außen (Datagramme vom Client) und jede Größe, die von anderen
// Plugins kommt (Array-Längen, Namen), wird gegen eine dieser Grenzen geprüft,
// bevor irgendetwas Speicher belegt oder Zeit kostet.
//
// Diese Datei ist XPLM-frei — sie wird vom Plugin UND von den Tests benutzt.
// =============================================================================

#pragma once

#include <cstddef>
#include <cstdint>

namespace aeroacars {
namespace grenzen {

// ---- Transport -------------------------------------------------------------

// Steuer-Socket: nur Loopback. 52000 ist Protokoll 1 (Plugin → Client),
// 52001 ist der Steuer-Port für Protokoll 2 (Client ↔ Plugin). Beide liegen
// bewusst außerhalb von X-Planes eigenem Bereich 49000–49003.
constexpr uint16_t STEUER_PORT = 52001;

// Anfrage-Datagramm: höchstens 64 KiB (ADR). Größer ist ein Protokollfehler.
constexpr size_t MAX_ANFRAGE_BYTES = 65536;

// Eine Anfragezeile: höchstens 512 Byte OHNE Zeilenende.
constexpr size_t MAX_ZEILE = 512;

// Antwort-Datagramm: höchstens 8 KiB EINSCHLIESSLICH des abschließenden '\n'.
// 8192 liegt unter macOS' Standardgrenze net.inet.udp.maxdgram (9216), sonst
// würde sendto() mit EMSGSIZE scheitern.
constexpr size_t MAX_PAKET = 8192;

// Höchstens so viele eingehende Datagramme je Frame (ADR 4). Ein Client, der
// den Port flutet, kann damit höchstens 64 × Parserlauf je Frame kosten.
constexpr int MAX_DATAGRAMME_JE_FRAME = 64;

// Höchstens so viele ausgehende Pakete je Frame, über alles. Schützt X-Plane
// (sendto kostet je Paket einige Mikrosekunden) und den Empfangspuffer des
// Clients (Windows hat ab Werk nur 64 KiB): 16 × 8 KiB = 128 KiB je Frame.
constexpr int MAX_PAKETE_JE_FRAME = 16;

// ---- Anfragen --------------------------------------------------------------

constexpr uint32_t PROTOKOLL = 2;

constexpr uint32_t MIN_ABO_ID = 1;
constexpr uint32_t MAX_ABO_ID = 16;
constexpr size_t   MAX_ABOS   = 16;

constexpr uint32_t MIN_RATE_HZ = 1;
constexpr uint32_t MAX_RATE_HZ = 50;

// Namen je Abo (über alle Teile eines mehrteiligen ABO zusammen).
constexpr size_t MAX_NAMEN_JE_ABO = 8192;

// Teile eines mehrteiligen ABO. Mehr als ein Teil je Name ist sinnlos.
constexpr uint32_t MAX_ABO_TEILE = 8192;

// Client-Version im HALLO (freier Text ohne Leerzeichen).
constexpr size_t MAX_CLIENT_VERSION = 64;

// Array-Index in "name[i]": X-Plane nimmt int als Offset.
constexpr uint32_t MAX_INDEX = 0x7FFFFFFFu;

// ---- Werte -----------------------------------------------------------------

// Ganze Arrays (vi/vf) werden höchstens mit so vielen Elementen geliefert.
// 256 × 17 Zeichen passen sicher in ein 8-KiB-Paket; für längere Arrays
// abonniert der Client einzelne Elemente "name[i]".
constexpr int MAX_ARRAY_ELEMENTE = 256;

// Byte-Arrays (Zeichenketten) höchstens so viele Bytes. Auch im schlimmsten
// Fall (jedes Byte als \u00XX = 6 Zeichen) passt das in ein Paket.
constexpr int MAX_BYTES = 1024;

// ---- Zeit ------------------------------------------------------------------

// Zeitbudget für Lesen + Senden je Frame (ADR 4).
constexpr double ZEITBUDGET_S = 0.001;

// Ohne Anfrage des Clients für so lange werden alle Abos verworfen und der
// Client vergessen (ADR 3).
constexpr double CLIENT_TIMEOUT_S = 5.0;

// Prüfdurchlauf (Nachsuche fehlender Namen, Waisen, Längen) je Abo.
constexpr double NACHSUCHE_INTERVALL_S = 2.0;

// Zeitbudget für Dataref-Suchen je Frame, über alle Abos — getrennt vom
// Liefer-Budget, damit das Anmelden großer Abos weder die Lieferung
// verdrängt noch von ihr verdrängt wird. (Bis zur Cloud-QS: feste 64 Suchen
// je Frame — 8192 Namen brauchten damit ≥ 128 Frames, bei 30 fps über 4 s,
// und ein Client, der nach 2 s neu anmeldete, bekam nie einen Status.)
constexpr double SUCH_BUDGET_S = 0.0003;

// Nach XPLM_MSG_PLANE_LOADED pausieren Abos mit Plugin-Namen, bis ihre
// Neusuche übernommen ist — aber höchstens so lange. Kleine Abos (beim Client
// Abo 1/2: Anzeige, Flugzustand) sucht die Reihenfolge "kürzester Rest
// zuerst" in wenigen Frames neu, sie sind also voll geschützt. Große
// Mess-Abos (8192 Namen, viele davon) bräuchten sonst viele Sekunden Pause;
// nach der Grenze liefern sie weiter, und die IsDataRefGood-Prüfung vor jedem
// Lesen hält verwaiste Namen trotzdem heraus.
constexpr double MAX_PAUSE_S = 0.5;

// Flugzeug-Kennung (ICAO/Titel/Pfad) wird so oft auf Änderung geprüft.
constexpr double FLUGZEUG_PRUEFINTERVALL_S = 2.0;

// ---- LISTE -----------------------------------------------------------------

// Obergrenze für die Zahl der Datarefs, die LISTE überhaupt anfasst. X-Plane 12
// mit schweren Add-ons liegt bei einigen zehntausend; alles darüber ist eine
// kaputte Rückgabe, kein echter Bestand.
constexpr int MAX_LISTE_DATAREFS = 1000000;

// So viele Handles holt LISTE je Aufruf von XPLMGetDataRefsByIndex.
constexpr int LISTE_BLOCK = 256;

}  // namespace grenzen
}  // namespace aeroacars
