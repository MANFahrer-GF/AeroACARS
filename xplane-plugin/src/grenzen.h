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

// Davon höchstens so viele kleine Einzelantworten (hallo, pong, fehler,
// abo_empfangen) je Frame. Alle Sendewege teilen sich die 16 oben; die
// kleinen Antworten kommen zuerst (sie entstehen beim Empfang, vor der
// Lieferung) und haben so Vorrang, dürfen die Lieferung aber nie ganz
// verdrängen: Eine PING-Flut (64 Datagramme je Frame) wird zu höchstens 8
// Antworten, der Rest wird gezählt und verworfen (Codex-Abnahme M1).
constexpr int MAX_KLEINE_JE_FRAME = 8;

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

// Ein einzelner Aufruf eines FREMDEN Accessors (Getter oder Array-Länge eines
// anderen Plugins) gilt ab dieser Dauer als langsam. Das Zeitbudget kann ihn
// nicht unterbrechen — XPLM ruft den Accessor synchron im Hauptthread auf —,
// es kann nur danach aufhören. Ein Dataref, der so oft HINTEREINANDER
// langsam war, wird gedrosselt (Codex-Abnahme H4): höchstens einmal je
// LANGSAM_INTERVALL_S gelesen und über alle Abos höchstens ein gedrosselter
// Lesezugriff je LANGSAM_ABSTAND_S. Status und Wert bleiben wahr ("fehlt"
// wäre eine Lüge), der Wert kommt nur seltener. Drei Treffer in Folge, damit
// ein einzelner Ausreißer (Betriebssystem unterbricht den Thread) keine
// gesunde Telemetrie drosselt.
constexpr double LANGSAM_AUFRUF_S = 0.002;
constexpr int LANGSAM_TREFFER = 3;
constexpr double LANGSAM_INTERVALL_S = 1.0;
constexpr double LANGSAM_ABSTAND_S = 0.2;
// Höchstens so viele "langsam"-Zeilen je Dienst im Log.txt (je Dataref eine).
constexpr int MAX_LANGSAM_MELDUNGEN = 32;
// Drosseltabelle je Dataref-Handle (offene Adressierung, Zweierpotenz). Es
// stehen nur Handles darin, deren letzter Aufruf langsam war; ein schneller
// Aufruf ohne Drosselung räumt den Platz wieder. Höchstens 3/4 belegt →
// 768 gleichzeitig verfolgte langsame Datarefs, 24 KiB fest.
constexpr size_t LANGSAM_PLAETZE = 1024;
// Hysterese: so viele SCHNELLE gedrosselte Lesungen in Folge heben die
// Drosselung wieder auf (der Accessor ist wieder gesund) — bei einer Lesung
// je Sekunde also nach ≈ 5 s.
constexpr int LANGSAM_ERHOLUNG = 5;
// Ein gedrosselter Dataref gilt als "wartend", wenn eine Runde ihn in den
// letzten so vielen Sekunden fällig angetroffen hat. Die 0,2-s-Sperre geht
// reihum an den am längsten wartenden (ältestes `faellig` zuerst).
constexpr double LANGSAM_GESEHEN_S = 2.0;

// Empfang: nach jedem Datagramm auf die Uhr; ist dieses Budget je Frame um,
// bleiben weitere Datagramme bis zum nächsten Frame im Socket (zusätzlich
// zur Grenze von 64 Datagrammen). Ein Datagramm (Parser ≤ 64 KiB, ggf. ein
// ABO-Teil mit Speicher) wird immer ganz bearbeitet.
constexpr double EMPFANG_BUDGET_S = 0.0005;

// Anteil des Liefer-Budgets, den Abo 1 (Telemetrie, Vorrang) höchstens
// bekommt; der Rest samt einer freien Einheit gehört dem Rundlauf (übrige
// Abos + LISTE). Sonst könnte ein großes Abo 1 alles andere aushungern.
constexpr double ABO1_BUDGET_S = 0.0005;

// Frame-Grenze für fremde Accessoren (Nachprüfung Claude, Punkt 3): Such-
// und Liefer-Budget zusammen. Nach ihr beginnt je Frame höchstens EIN
// weiterer fremder Accessor-Aufruf (die "freie Einheit"), reihum vergeben an
// Suche, Abo 1 und Rundlauf — jeder ist spätestens jeden dritten Frame dran.
// Damit liegt ein Frame höchstens zwei fremde Aufrufe über dem Budget: der
// eine, der innerhalb des Budgets begann und sich als langsam herausstellte,
// und die freie Einheit.
constexpr double FRAME_BUDGET_S = SUCH_BUDGET_S + ZEITBUDGET_S;

// ---- Speicher (Codex-Abnahme H3) ---------------------------------------------
//
// Harte Bytebudgets für alles, was ein Client über Abos und LISTE belegen
// kann. Gezählt wird die tatsächlich reservierte Kapazität (Namen, Einträge,
// Paketplan und der Ausgabestapel mit dem SCHLIMMSTEN Fall je Wert — 17
// Zeichen je float, 6 je Byte), und zwar je Eintrag: 8192-mal derselbe
// 1024-Byte-Dataref kostet 8192 × 6153 Byte ≈ 48 MiB und wird abgelehnt.
// Bei Überschreitung: {"p":2,"t":"fehler","grund":"speicher_limit",…} statt
// Allokation.
//
// Größenordnung (gemessen in der Schein-Welt, tests/test_dienst.cpp):
//   * Abo 1 (≈ 200 Namen, Skalare): ≈ 30 KiB (gerechnet).
//   * Ein Mess-Abo mit 8192 typischen Namen (60 % Skalare, 35 % kleine
//     Arrays 8–24, 5 % ganze Arrays à 256): ≈ 3,4 MiB. Der Client vermisst
//     mit bis zu 14 Abos (3–16) zu je 8192 Namen: Abo 1 + 14 Mess-Abos
//     ≈ 47 MiB (dienst_h3_legitime_vermessung_passt).
//   * Ballung: LISTE liefert Namen gruppiert; ein Abo mit 3400 ganzen
//     256er-Arrays braucht ≈ 14,4 MiB — deshalb 16 MiB je Abo, nicht 8.
//   * LISTE mit 40 000 Namen: ≈ 2,5 MiB.
// Obergrenze für den ganzen Dienst damit ≈ 64 + 16 MiB (vorher unbegrenzt:
// 16 Abos × 48 MiB).
constexpr size_t MAX_BYTES_JE_ABO = size_t(16) << 20;
constexpr size_t MAX_BYTES_ABOS   = size_t(64) << 20;   // alle Abos + Teil-Abos
constexpr size_t MAX_BYTES_LISTE  = size_t(16) << 20;

// ---- LISTE -----------------------------------------------------------------

// Obergrenze für die Zahl der Datarefs, die LISTE überhaupt anfasst. X-Plane 12
// mit schweren Add-ons liegt bei einigen zehntausend; alles darüber ist eine
// kaputte Rückgabe, kein echter Bestand.
constexpr int MAX_LISTE_DATAREFS = 1000000;

// So viele Handles holt LISTE je Aufruf von XPLMGetDataRefsByIndex.
constexpr int LISTE_BLOCK = 256;

}  // namespace grenzen
}  // namespace aeroacars
