// =============================================================================
// AeroACARS X-Plane-Plugin — Dataref-Dienst (Protokoll 2), XPLM-freier Kern
// =============================================================================
//
// Der Dienst enthält die ganze Logik von Protokoll 2 (ADR-0004): Anmeldung
// des Clients, Abos, Namenssuche, Lieferung mit Rate und Zeitbudget, LISTE,
// Flugzeugmeldung, Zeitüberschreitung. Er spricht mit der Außenwelt nur über
// zwei schmale Schnittstellen:
//
//   * Datenquelle — Datarefs suchen und lesen (im Plugin: XPLM; in den Tests:
//     eine Schein-Welt mit frei wählbaren Datarefs).
//   * Umgebung    — Paket senden, Uhr, Log-Zeile (im Plugin: UDP-Socket,
//     steady_clock, XPLMDebugString).
//
// Dadurch ist der Kern ohne X-Plane testbar (tests/test_dienst.cpp) — auch
// Zeitbudget, Zeitüberschreitung und Statuswechsel beim Flugzeugwechsel.
//
// Alles läuft im Hauptthread von X-Plane (Flight-Loop). Es gibt keine
// Threads, keine Sperren, keine Ausnahmen.
//
// Lebenszyklus eines Abos:
//
//   ABO (letzter Teil) ──► NEU ──(Prüfdurchlauf fertig)──► ANTWORT ──► BEREIT
//                                                                        │ ▲
//                                              fällig nach 1/rate s ──►  ▼ │
//                                                                  LESEN ─► SENDEN
//
//   * Ein Prüfdurchlauf sucht die Namen (eigenes Zeitbudget je Frame, siehe
//     suchen_verteilen) und schreibt das Ergebnis in `kandidat`. Gelesen wird
//     nur aus `aktiv`.
//   * Übernommen wird ein fertiger Durchlauf nur zwischen zwei Lieferrunden
//     (NEU oder BEREIT). Hat sich ein Status geändert, geht zuerst eine neue
//     abo-Antwort hinaus, dann erst wieder Werte — der Client sieht nie einen
//     Wert zu einem Index, dessen Status er noch nicht kennt.
//   * Durchläufe starten beim Anmelden, alle 2 s, und sofort bei
//     XPLM_MSG_PLANE_LOADED (Flugzeug 0) / XPLM_MSG_AIRPORT_LOADED. Der
//     2-s-Durchlauf prüft nur fehlende Namen und Arrays (Länge); verwaiste
//     Einzelwerte fängt die Prüfung beim Lesen.
// =============================================================================

#pragma once

#include "anfrage.h"
#include "feld.h"
#include "grenzen.h"
#include "pakete.h"

#include <cstddef>
#include <cstdint>

namespace aeroacars {

// IPv4-Adresse + Port in Host-Byte-Reihenfolge.
struct Absender {
    uint32_t ip = 0;
    uint16_t port = 0;
};
inline bool operator==(const Absender& a, const Absender& b) noexcept {
    return a.ip == b.ip && a.port == b.port;
}

enum class SendeErgebnis : uint8_t {
    OK,      // gesendet
    VOLL,    // Socket-Puffer voll — im nächsten Frame noch einmal versuchen
    FEHLER,  // endgültig nicht zustellbar — verwerfen, nicht wiederholen
};

class Umgebung {
public:
    virtual SendeErgebnis sende(const Absender& an, const char* daten, size_t laenge) noexcept = 0;
    // Monotone Uhr in Sekunden (Startpunkt beliebig).
    virtual double jetzt() noexcept = 0;
    virtual void protokolliere(const char* zeile) noexcept = 0;

protected:
    ~Umgebung() = default;
};

using DatarefHandle = void*;

// Typ-Bits wie xplmType_* in XPLMDataAccess.h (hier kopiert, damit der Kern
// XPLM-frei bleibt; plugin-seitig per static_assert abgeglichen).
namespace typ {
constexpr int I  = 1;
constexpr int F  = 2;
constexpr int D  = 4;
constexpr int VF = 8;
constexpr int VI = 16;
constexpr int B  = 32;
}  // namespace typ

class Datenquelle {
public:
    virtual DatarefHandle finde(const char* name) noexcept = 0;
    // false für verwaiste Datarefs (ihr Plugin ist entladen; lesen ergäbe 0).
    virtual bool ist_gueltig(DatarefHandle h) noexcept = 0;
    virtual int typen(DatarefHandle h) noexcept = 0;
    virtual int lese_i(DatarefHandle h) noexcept = 0;
    virtual float lese_f(DatarefHandle h) noexcept = 0;
    virtual double lese_d(DatarefHandle h) noexcept = 0;
    // Wie XPLMGetDatav*/b: aus == nullptr → Länge des Arrays.
    virtual int lese_vi(DatarefHandle h, int* aus, int ab, int max) noexcept = 0;
    virtual int lese_vf(DatarefHandle h, float* aus, int ab, int max) noexcept = 0;
    virtual int lese_b(DatarefHandle h, void* aus, int ab, int max) noexcept = 0;
    // LISTE (XPLM 4.0+). Nur aufrufen, wenn liste_verfuegbar().
    virtual bool liste_verfuegbar() noexcept = 0;
    virtual int anzahl_datarefs() noexcept = 0;
    // Füllt aus[0..anzahl) ab Index `ab`; Rückgabe: Zahl gefüllter Einträge.
    virtual int datarefs_ab(int ab, int anzahl, DatarefHandle* aus) noexcept = 0;
    virtual const char* name_von(DatarefHandle h) noexcept = 0;

protected:
    ~Datenquelle() = default;
};

struct Kennung {
    const char* plugin_version = "0.0.0";
    int xplane_version = 0;
    int xplm_version = 0;
};

class Dienst {
public:
    Dienst(Datenquelle& quelle, Umgebung& umgebung, const Kennung& kennung) noexcept;
    ~Dienst();

    Dienst(const Dienst&) = delete;
    Dienst& operator=(const Dienst&) = delete;

    // false, wenn der Grundspeicher nicht reserviert werden konnte — dann
    // bleibt Protokoll 2 aus.
    bool bereit() const noexcept { return bereit_; }

    // Ein Datagramm vom Steuer-Socket (Absender ist bereits als 127.0.0.1
    // geprüft).
    void empfange(const Absender& von, const char* daten, size_t laenge) noexcept;

    // Einmal je Flight-Loop-Aufruf, NACH dem Empfang.
    void frame() noexcept;

    // XPLM_MSG_PLANE_LOADED mit Flugzeug 0 (Nutzerflugzeug).
    void flugzeug_geladen() noexcept;
    // XPLM_MSG_AIRPORT_LOADED.
    void flughafen_geladen() noexcept;

    // true, solange es etwas zu liefern gibt — dann braucht der Dienst jeden
    // Frame. Sonst genügt der gemächliche Takt von Protokoll 1.
    bool braucht_jeden_frame() const noexcept;

    // Alle Abos, Teil-Abos und LISTE verwerfen (Client bleibt angemeldet).
    void alles_verwerfen() noexcept;

    // -- Einblick für Tests ----------------------------------------------------
    bool client_angemeldet() const noexcept { return client_aktiv_; }
    size_t aktive_abos() const noexcept;

private:
    enum class Zugriff : uint8_t {
        FEHLT = 0, I, F, D, VI, VF, B, ELEM_VI, ELEM_VF, ELEM_B,
    };
    struct Aufloesung {
        DatarefHandle h = nullptr;
        Zugriff z = Zugriff::FEHLT;
        int32_t laenge = 0;  // Status-Länge: 1 für Skalare/Elemente
    };
    // Index-Wert für eine Zeile, die kein gültiger Name war (Status "fehlt",
    // wird nie gesucht).
    static constexpr int32_t INDEX_UNGUELTIG = -2;

    struct Eintrag {
        uint32_t name_ofs;   // Basisname, NUL-terminiert, in Abo::text
        int32_t index;       // -1 = ganzer Dataref, -2 = ungültige Zeile
        bool darf_verwaisen; // nicht "sim/…": vor jedem Lesen XPLMIsDataRefGood
        Aufloesung aktiv;    // danach wird geliefert
        Aufloesung kandidat; // Ergebnis des laufenden Prüfdurchlaufs
    };
    struct RohName {
        uint32_t ofs;
        int32_t index;
    };
    enum class Phase : uint8_t { LEER, NEU, ANTWORT, BEREIT, LESEN, SENDEN };

    struct Abo {
        Phase phase = Phase::LEER;
        uint32_t id = 0;
        uint32_t rate = 0;
        uint32_t generation = 0;
        double periode = 1.0;
        Feld<char> text;
        Feld<Eintrag> eintraege;
        bool hat_plugin_namen = false;  // mindestens ein Eintrag darf_verwaisen
        // Prüfdurchlauf
        bool pruefung_laeuft = false;
        bool pruefung_fertig = false;
        bool pruefung_alle = true;      // false: nur fehlende Namen + Arrays
        bool pruefung_dringend = true;  // Anmeldung / Flugzeugwechsel: vor periodischen
        bool erste_antwort = true;
        bool antwort_erneut = false;    // identisches ABO: Status noch einmal senden
        bool status_sofort = false;     // beim Lesen verwaist gefunden → neue Antwort
        bool pausiert = false;          // nach Flugzeugwechsel bis zur Neusuche …
        double pause_bis = 0.0;         // … höchstens bis hierhin (grenzen::MAX_PAUSE_S)
        uint32_t pruef_cursor = 0;
        double naechste_pruefung = 0.0;
        // Ausgabe (abo-Antwort und Werte teilen sich Stapel und Plan; sie
        // laufen nie gleichzeitig)
        ElementListe stapel;
        Feld<uint32_t> grenzen;
        uint32_t teile = 0;
        uint32_t paket_cursor = 0;
        char praefix[64] = {0};
        size_t praefix_laenge = 0;
        const char* feld = "v";
        // Lieferung
        uint32_t lese_cursor = 0;
        uint32_t seq = 0;
        double faellig = 0.0;
    };
    struct AboBau {
        bool aktiv = false;
        uint32_t rate = 0;
        uint32_t generation = 0;
        uint32_t teile = 0;
        uint32_t naechster = 0;
        Feld<char> text;
        Feld<RohName> namen;
    };
    struct ListeLauf {
        bool aktiv = false;
        bool sammeln = false;
        uint32_t id = 0;
        int gesamt = 0;
        int cursor = 0;
        uint32_t ausgelassen = 0;
        ElementListe namen;
        Feld<uint32_t> grenzen;
        uint32_t teile = 0;
        uint32_t paket_cursor = 0;
        char praefix[64] = {0};
        size_t praefix_laenge = 0;
    };

    // Anfragen
    void bearbeite_hallo(const Absender& von, const Anfrage& a) noexcept;
    void bearbeite_abo(const Anfrage& a) noexcept;
    void bearbeite_liste(const Anfrage& a) noexcept;
    bool aktiviere(uint32_t id, AboBau& bau) noexcept;

    // Abos
    void bearbeite_abo_frame(Abo& abo) noexcept;
    void suchen_verteilen() noexcept;
    void pruef_schritt(Abo& abo) noexcept;
    static bool braucht_nachsuche(const Eintrag& e) noexcept;
    static bool gleiches_abo(const Abo& abo, const AboBau& bau) noexcept;
    Aufloesung loese_auf(const char* name, int32_t index) noexcept;
    bool uebernehme_pruefung(Abo& abo, bool* geaendert) noexcept;
    bool bereite_antwort(Abo& abo) noexcept;
    void starte_runde(Abo& abo) noexcept;
    bool lese_schritt(Abo& abo) noexcept;   // true = Runde fertig gelesen
    bool sende_schritt(Abo& abo) noexcept;  // true = alle Pakete draußen
    bool schreibe_wert(JsonSchreiber& w, const Eintrag& e, uint32_t k) noexcept;
    void abo_leeren(Abo& abo) noexcept;
    void abo_verwerfen_mit_fehler(Abo& abo, Fehlergrund grund) noexcept;
    void starte_pruefung(Abo& abo, bool alle, bool dringend) noexcept;
    static size_t wert_max(const Aufloesung& a) noexcept;

    // LISTE, Flugzeug
    void liste_schritt() noexcept;
    void liste_leeren() noexcept;
    void pruefe_flugzeug() noexcept;
    bool lies_kennung(DatarefHandle* h, const char* name, char* aus, size_t kap) noexcept;

    // Senden
    void sende_einzeln(const Absender& an, const JsonSchreiber& w) noexcept;
    // `abo` != 0 → "abo" und "gen" werden mitgeschickt; `id` >= 0 → "id".
    void sende_fehler(const Absender& an, Fehlergrund grund, uint32_t zeile,
                      uint32_t abo, uint32_t gen, int64_t id) noexcept;
    void sende_abo_empfangen(uint32_t abo, uint32_t gen, size_t namen) noexcept;
    SendeErgebnis sende_paket(const char* daten, size_t laenge) noexcept;

    // Budget
    bool darf_arbeiten() noexcept;
    void protokolliere(const char* format, ...) noexcept;

    Datenquelle& quelle_;
    Umgebung& umgebung_;
    Kennung kennung_;
    bool bereit_ = false;

    bool client_aktiv_ = false;
    Absender client_{};
    double letzte_anfrage_ = 0.0;

    Abo abos_[grenzen::MAX_ABOS];
    AboBau bau_[grenzen::MAX_ABOS];
    ListeLauf liste_;
    Feld<NameRef> namen_puffer_;
    uint32_t rundlauf_ = 0;

    // Frame-Zustand
    double jetzt_ = 0.0;
    double budget_ende_ = 0.0;
    int pakete_frame_ = 0;
    bool garantie_ = false;

    // Flugzeug
    bool flugzeug_offen_ = false;
    double naechste_flugzeug_pruefung_ = 0.0;
    DatarefHandle h_icao_ = nullptr;
    DatarefHandle h_titel_ = nullptr;
    DatarefHandle h_pfad_ = nullptr;
    char gesendet_icao_[41] = {0};
    char gesendet_titel_[261] = {0};
    char gesendet_pfad_[1025] = {0};
    bool gesendet_gueltig_ = false;
    bool gesendet_hat_[3] = {false, false, false};

    // Arbeitspuffer. Größer als je angefordert: ein fehlerhaftes Plugin, das
    // mehr Werte schreibt als erbeten, trifft Reserve statt fremden Speicher.
    char paket_[grenzen::MAX_PAKET];
    float fwerte_[4 * grenzen::MAX_ARRAY_ELEMENTE];
    int iwerte_[4 * grenzen::MAX_ARRAY_ELEMENTE];
    unsigned char bytes_[4 * grenzen::MAX_BYTES + 8];
    DatarefHandle handles_[grenzen::LISTE_BLOCK];
};

}  // namespace aeroacars
