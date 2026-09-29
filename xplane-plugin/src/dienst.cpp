// =============================================================================
// AeroACARS X-Plane-Plugin — Dataref-Dienst (Umsetzung)
// =============================================================================
//
// Siehe dienst.h für Aufbau und Lebenszyklus. Die Kommentare hier begründen
// die Stellen, an denen es eine naheliegende, aber falsche Lösung gäbe.
// =============================================================================

#include "dienst.h"

#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstring>

namespace aeroacars {

namespace {

// Höchstlänge eines Status-Elements: [8191,"fehlt"] = 15, [8191,"vf",256] = 16,
// [8191,"b",1024] = 16. Mit Reserve.
constexpr size_t STATUS_MAX = 20;

// "[k," + "]" um jeden Wert.
constexpr size_t WERT_RAHMEN = 1 + 4 + 1 + 1;

// Platz der LISTE im Rundlauf der Lieferung (hinter den 16 Abo-Plätzen).
constexpr uint32_t LISTE_PLATZ = static_cast<uint32_t>(grenzen::MAX_ABOS);

int32_t begrenze(int n, int max) noexcept {
    if (n < 0) return 0;
    return n > max ? max : n;
}

size_t text_laenge(const char* s, size_t max) noexcept {
    size_t n = 0;
    while (n < max && s[n] != '\0') ++n;
    return n;
}

}  // namespace

// =============================================================================
// Aufbau / Abbau
// =============================================================================

Dienst::Dienst(Datenquelle& quelle, Umgebung& umgebung, const Kennung& kennung) noexcept
    : quelle_(quelle), umgebung_(umgebung), kennung_(kennung) {
    // Der einzige Grundspeicher: Platz für die Namen EINES Datagramms (die
    // Namen selbst bleiben im Datagramm, hier stehen nur Zeiger + Index).
    bereit_ = namen_puffer_.reserviere(grenzen::MAX_NAMEN_JE_ABO);
    std::memset(paket_, 0, sizeof(paket_));
}

Dienst::~Dienst() {
    alles_verwerfen();
}

size_t Dienst::aktive_abos() const noexcept {
    size_t n = 0;
    for (const Abo& a : abos_) {
        if (a.phase != Phase::LEER) ++n;
    }
    return n;
}

const char* Dienst::abo_phase(uint32_t id) const noexcept {
    if (id < grenzen::MIN_ABO_ID || id > grenzen::MAX_ABO_ID) return "leer";
    switch (abos_[id - 1].phase) {
        case Phase::LEER:    return "leer";
        case Phase::NEU:     return "neu";
        case Phase::ANTWORT: return "antwort";
        case Phase::BEREIT:  return "bereit";
        case Phase::LESEN:   return "lesen";
        case Phase::SENDEN:  return "senden";
    }
    return "leer";
}

void Dienst::alles_verwerfen() noexcept {
    for (Abo& a : abos_) abo_leeren(a);
    for (AboBau& b : bau_) {
        b.aktiv = false;
        b.text.freigeben();
        b.namen.freigeben();
    }
    liste_leeren();
    for (OffenerFehler& f : offene_fehler_) f = OffenerFehler{};
    langsam_leeren();
}

void Dienst::abo_leeren(Abo& abo) noexcept {
    abo.text.freigeben();
    abo.eintraege.freigeben();
    abo.stapel.freigeben();
    abo.grenzen.freigeben();
    abo.phase = Phase::LEER;
    abo.id = 0;
    abo.rate = 0;
    abo.generation = 0;
    abo.hat_plugin_namen = false;
    abo.pruefung_laeuft = false;
    abo.pruefung_fertig = false;
    abo.pruefung_alle = true;
    abo.pruefung_dringend = true;
    abo.erste_antwort = true;
    abo.antwort_erneut = false;
    abo.status_sofort = false;
    abo.pausiert = false;
    abo.pruef_cursor = 0;
    abo.teile = 0;
    abo.paket_cursor = 0;
    abo.lese_cursor = 0;
    abo.seq = 0;
}

void Dienst::liste_leeren() noexcept {
    liste_.aktiv = false;
    liste_.sammeln = false;
    liste_.namen.freigeben();
    liste_.grenzen.freigeben();
    liste_.teile = 0;
    liste_.paket_cursor = 0;
    liste_.cursor = 0;
    liste_.block_pos = 0;
    liste_.block_anzahl = 0;
    liste_.gesamt = 0;
    liste_.ausgelassen = 0;
}

bool Dienst::braucht_jeden_frame() const noexcept {
    if (!client_aktiv_) return false;
    if (liste_.aktiv) return true;
    for (const Abo& a : abos_) {
        if (a.phase != Phase::LEER) return true;
    }
    return false;
}

void Dienst::protokolliere(const char* format, ...) noexcept {
    char zeile[512];
    va_list ap;
    va_start(ap, format);
    std::vsnprintf(zeile, sizeof(zeile), format, ap);
    va_end(ap);
    umgebung_.protokolliere(zeile);
}

// =============================================================================
// Speicher (Codex-Abnahme H3)
// =============================================================================
//
// Gezählt wird die RESERVIERTE Kapazität, nicht der Inhalt: Das ist, was der
// X-Plane-Prozess tatsächlich belegt, und der Ausgabestapel ist absichtlich
// für den schlimmsten Fall jedes Werts reserviert (keine Allokation beim
// Lesen). Jeder Eintrag zählt einzeln — 8192-mal derselbe Name kostet
// 8192-mal (der Stapel hält 8192 Werte).

size_t Dienst::abo_bytes(const Abo& a) noexcept {
    return a.text.kapazitaet() + a.eintraege.kapazitaet() * sizeof(Eintrag) +
           a.grenzen.kapazitaet() * sizeof(uint32_t) + a.stapel.kapazitaet_bytes() +
           a.stapel.kapazitaet_elemente() * sizeof(uint32_t);
}

size_t Dienst::bau_bytes(const AboBau& b) noexcept {
    return b.text.kapazitaet() + b.namen.kapazitaet() * sizeof(RohName);
}

size_t Dienst::speicher_abos() const noexcept {
    size_t n = 0;
    for (const Abo& a : abos_) n += abo_bytes(a);
    for (const AboBau& b : bau_) n += bau_bytes(b);
    return n;
}

size_t Dienst::speicher_liste() const noexcept {
    return liste_.namen.belegt_bytes() + liste_.grenzen.kapazitaet() * sizeof(uint32_t);
}

bool Dienst::abo_speicher_passt(size_t alt, size_t neu) const noexcept {
    if (neu > grenzen::MAX_BYTES_JE_ABO) return false;
    const size_t gesamt = speicher_abos();
    const size_t ohne = gesamt >= alt ? gesamt - alt : 0;
    return ohne <= grenzen::MAX_BYTES_ABOS - neu;  // neu ≤ MAX_BYTES_JE_ABO < MAX_BYTES_ABOS
}

// =============================================================================
// Senden (Codex-Abnahme M1: ein gemeinsamer, begrenzter Ausgang)
// =============================================================================
//
// Jeder Sendeweg zählt gegen grenzen::MAX_PAKETE_JE_FRAME. Die Zähler laufen
// vom Empfang (vor frame()) bis zum Ende von frame() — das ist ein Aufruf des
// Flight-Loops. Reihenfolge und damit Vorrang: kleine Einzelantworten beim
// Empfang (höchstens MAX_KLEINE_JE_FRAME), dann `flugzeug`, dann Status,
// Werte und LISTE mit dem Rest.

void Dienst::sende_einzeln(const Absender& an, const JsonSchreiber& w) noexcept {
    if (w.ueberlauf() || w.laenge() == 0) return;
    if (kleine_frame_ >= grenzen::MAX_KLEINE_JE_FRAME ||
        pakete_frame_ >= grenzen::MAX_PAKETE_JE_FRAME) {
        // Flut (z. B. 64 PING in einem Frame): verwerfen statt zur
        // Antwortflut werden. Gezählt und höchstens alle 10 s gemeldet. Der
        // Client verliert nichts Wichtiges — hallo, pong, abo_empfangen und
        // fehler fragt er bei Bedarf erneut an.
        ++kleine_verworfen_;
        return;
    }
    ++kleine_frame_;
    ++pakete_frame_;
    // Einzelne Antworten werden nicht wiederholt: sie sind klein, und der
    // Client fragt bei Bedarf erneut.
    umgebung_.sende(an, w.daten(), w.laenge());
}

void Dienst::sende_abo_empfangen(uint32_t abo, uint32_t gen, size_t namen) noexcept {
    // Sofortige Empfangsbestätigung: der Client weiß ab jetzt, dass sein ABO
    // angekommen ist und gesucht wird — auch wenn die Suche bei tausenden
    // Namen und vielen Abos länger dauert als seine Wartezeit auf den Status.
    char puffer[128];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.roh("{\"p\":2,\"t\":\"abo_empfangen\",\"abo\":");
    w.ganzzahl(abo);
    w.roh(",\"gen\":");
    w.ganzzahl(gen);
    w.roh(",\"namen\":");
    w.ganzzahl(static_cast<int64_t>(namen));
    w.roh("}\n");
    sende_einzeln(client_, w);
}

void Dienst::sende_fehler(const Absender& an, Fehlergrund grund, uint32_t zeile,
                          uint32_t abo, uint32_t gen, int64_t id) noexcept {
    char puffer[256];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.roh("{\"p\":2,\"t\":\"fehler\",\"grund\":\"");
    w.roh(fehlergrund_text(grund));
    w.zeichen('"');
    if (zeile != 0) { w.roh(",\"zeile\":"); w.ganzzahl(zeile); }
    if (abo != 0) {
        w.roh(",\"abo\":");
        w.ganzzahl(abo);
        w.roh(",\"gen\":");
        w.ganzzahl(gen);
    }
    if (id >= 0)    { w.roh(",\"id\":");    w.ganzzahl(id); }
    w.roh("}\n");
    sende_einzeln(an, w);
}

void Dienst::fehler_vormerken(Fehlergrund grund, uint32_t abo, uint32_t gen, int64_t id) noexcept {
    const size_t platz = (abo >= grenzen::MIN_ABO_ID && abo <= grenzen::MAX_ABO_ID)
                             ? abo - 1 : grenzen::MAX_ABOS;
    OffenerFehler& f = offene_fehler_[platz];
    f.grund = grund;
    f.abo = abo;
    f.gen = gen;
    f.id = id;
}

void Dienst::sende_vorgemerkte_fehler() noexcept {
    for (OffenerFehler& f : offene_fehler_) {
        if (f.grund == Fehlergrund::KEINER) continue;
        // Gegen die Gesamtgrenze, nicht gegen die der kleinen Antworten: Beim
        // Empfang gehen höchstens 8 hinaus, hier ist also immer Platz für
        // mindestens 8 — mehr als 17 vorgemerkte gibt es nicht.
        char puffer[256];
        JsonSchreiber w(puffer, sizeof(puffer));
        w.roh("{\"p\":2,\"t\":\"fehler\",\"grund\":\"");
        w.roh(fehlergrund_text(f.grund));
        w.zeichen('"');
        if (f.abo != 0) {
            w.roh(",\"abo\":");
            w.ganzzahl(f.abo);
            w.roh(",\"gen\":");
            w.ganzzahl(f.gen);
        }
        if (f.id >= 0) { w.roh(",\"id\":"); w.ganzzahl(f.id); }
        w.roh("}\n");
        if (!w.ueberlauf()) {
            // Über den gemeinsamen Ausgang. VOLL (Frame-Grenze oder Socket-
            // Puffer): vorgemerkt lassen, nächster Frame. Nur OK oder ein
            // endgültiger FEHLER räumen den Platz (Nachprüfung Codex M1).
            const SendeErgebnis r = sende_paket(w.daten(), w.laenge());
            uhr_auffrischen();
            if (r == SendeErgebnis::VOLL) return;
        }
        f = OffenerFehler{};
    }
}

SendeErgebnis Dienst::sende_paket(const char* daten, size_t laenge) noexcept {
    if (pakete_frame_ >= grenzen::MAX_PAKETE_JE_FRAME) return SendeErgebnis::VOLL;
    const SendeErgebnis r = umgebung_.sende(client_, daten, laenge);
    if (r != SendeErgebnis::VOLL) ++pakete_frame_;
    return r;
}

// =============================================================================
// Zeitbudget (Codex-Abnahme H4)
// =============================================================================
//
// Vorher: je Arbeitseinheit einmal auf die Uhr sehen, die erste Einheit des
// Frames ganz ohne Uhr. Eine Sucheinheit waren aber bis zu vier XPLM-Aufrufe
// (Find, IsGood, Types, Array-Länge), eine LISTE-Einheit 256 Namen.
//
// Jetzt: Nach JEDEM XPLM-Aufruf wird die Uhr gelesen (nach_aufruf), vor jedem
// Aufruf gegen das Budget geprüft (darf_arbeiten). Frei ist nur die erste
// Einheit je Budget und Frame, und auch die nur bis einschließlich ihres
// einen fremden Accessors (nach_fremdaufruf) — höchstens also drei billige
// XPLM-interne Aufrufe plus EIN fremder. Bricht das Budget mitten in einem
// Eintrag, beginnt der nächste Frame diesen Eintrag neu (ein halbes Ergebnis
// wird nie aufgehoben — insbesondere gilt eine Verwaist-Prüfung nur in dem
// Moment, in dem sie gemacht wurde).
//
// Das bleibt ein WEICHES Budget: Einen fremden Accessor, der 20 ms braucht,
// kann kein Plugin unterbrechen; die Uhr sieht ihn erst danach. Was das
// Plugin tun kann: danach sofort aufhören und einen wiederholt langsamen
// Dataref drosseln (grenzen::LANGSAM_*).

void Dienst::beginne_budget(double dauer) noexcept {
    uhr_ = umgebung_.jetzt();
    budget_ende_ = uhr_ + dauer;
    garantie_ = true;
}

// Vor jedem XPLM-Aufruf. Jede Phase (Suche, Abo 1, Rundlauf) hat ihr
// eigenes Budget und ihre freie erste Einheit. Übergreifend gilt nur: Nach
// zwei LANGSAMEN fremden Aufrufen (grenzen::MAX_LANGSAME_JE_FRAME) beginnt in
// diesem Frame kein fremder Accessor mehr. So verdrängt ein langsamer
// Such-Aufruf die Telemetrie nicht, und kein Frame hat mehr als zwei
// langsame fremde Aufrufe.
bool Dienst::darf_arbeiten(bool fremd) noexcept {
    if (fremd && langsame_frame_ >= grenzen::MAX_LANGSAME_JE_FRAME) return false;
    return garantie_ || uhr_ < budget_ende_;
}

double Dienst::nach_aufruf() noexcept {
    const double t = umgebung_.jetzt();
    const double d = t - uhr_;
    uhr_ = t;
    return d;
}

void Dienst::nach_fremdaufruf(Abo& abo, Eintrag& e, DatarefHandle h) noexcept {
    const double d = nach_aufruf();
    garantie_ = false;
    if (d > grenzen::LANGSAM_AUFRUF_S) ++langsame_frame_;
    if (d <= grenzen::LANGSAM_AUFRUF_S) {
        // Nur Treffer IN FOLGE zählen: ein einzelner Ausreißer (der Thread
        // wurde vom Betriebssystem unterbrochen) drosselt nichts. Ein
        // schneller Aufruf räumt den Platz eines noch nicht gedrosselten
        // Handles wieder frei (die Tabelle füllt sich so nicht mit
        // Ausreißern eines langen Flugs).
        if (langsam_belegt_ == 0) return;
        const size_t i = langsam_platz(h);
        if (i >= grenzen::LANGSAM_PLAETZE) return;
        if (!langsam_[i].gedrosselt) {
            langsam_loeschen(i);
            return;
        }
        // Hysterese: gedrosselt und wieder schnell — nach LANGSAM_ERHOLUNG
        // schnellen Lesungen in Folge ist die Drosselung aufgehoben.
        if (++langsam_[i].schnell >= grenzen::LANGSAM_ERHOLUNG) {
            langsam_loeschen(i);
            if (langsam_meldungen_ < grenzen::MAX_LANGSAM_MELDUNGEN) {
                ++langsam_meldungen_;
                protokolliere("Protokoll 2: Dataref %s antwortet wieder schnell - volle Rate",
                              abo.text.daten() + e.name_ofs);
            }
        }
        return;
    }
    // Der Zustand gehört dem DATAREF, nicht dem Eintrag: Duplikate desselben
    // Namens (auch in verschiedenen Abos) zählen gemeinsam und werden
    // gemeinsam gedrosselt.
    Langsam* l = langsam_anlegen(h);
    if (l == nullptr) {
        if (!langsam_voll_gemeldet_) {
            langsam_voll_gemeldet_ = true;
            protokolliere("Protokoll 2: Drosseltabelle voll (%u langsame Datarefs) - weitere werden "
                          "nicht gedrosselt",
                          static_cast<unsigned>(langsam_belegt_));
        }
        return;
    }
    if (l->gedrosselt) {
        l->schnell = 0;  // Hysterese: wieder von vorn
        return;
    }
    if (l->treffer < 255) ++l->treffer;
    if (l->treffer < grenzen::LANGSAM_TREFFER) return;
    l->gedrosselt = true;
    l->schnell = 0;
    l->faellig = jetzt_ + grenzen::LANGSAM_INTERVALL_S;
    naechster_langsamer_ = jetzt_ + grenzen::LANGSAM_ABSTAND_S;
    if (langsam_meldungen_ < grenzen::MAX_LANGSAM_MELDUNGEN) {
        ++langsam_meldungen_;
        char idx[16] = {0};
        if (e.index >= 0) std::snprintf(idx, sizeof(idx), "[%d]", static_cast<int>(e.index));
        protokolliere("Protokoll 2: Dataref %s%s antwortet langsam (%.1f ms je Aufruf, Abo %u) - "
                      "wird hoechstens alle %.0f s gelesen",
                      abo.text.daten() + e.name_ofs, idx, d * 1000.0,
                      static_cast<unsigned>(abo.id), grenzen::LANGSAM_INTERVALL_S);
    }
}

// Fair statt "wer zuerst kommt": Die 0,2-s-Sperre geht an den gedrosselten
// Handle, der am längsten wartet (kleinstes `faellig`) — aber nur unter
// denen, die eine Runde zuletzt fällig angetroffen hat (sonst blockierte ein
// Handle, den kein Abo mehr liest, alle anderen). Einmal je Frame bestimmt.
void Dienst::waehle_langsam_vorrang() noexcept {
    langsam_vorrang_ = nullptr;
    if (langsam_belegt_ == 0) return;
    for (const Langsam& l : langsam_) {
        if (l.h == nullptr || !l.gedrosselt || l.faellig > jetzt_) continue;
        if (l.gesehen < jetzt_ - grenzen::LANGSAM_GESEHEN_S) continue;
        if (langsam_vorrang_ == nullptr || l.faellig < langsam_vorrang_faellig_) {
            langsam_vorrang_ = l.h;
            langsam_vorrang_faellig_ = l.faellig;
        }
    }
}

bool Dienst::darf_langsam_lesen(Langsam& l) noexcept {
    if (jetzt_ < l.faellig) return false;
    l.gesehen = jetzt_;
    if (jetzt_ < naechster_langsamer_) return false;
    if (langsam_vorrang_ != nullptr && langsam_vorrang_ != l.h &&
        langsam_vorrang_faellig_ < l.faellig) {
        return false;  // ein anderer wartet länger
    }
    return true;
}

// ---- Drosseltabelle: offene Adressierung, lineare Sondierung --------------
//
// Fest dimensioniert (grenzen::LANGSAM_PLAETZE), höchstens 3/4 belegt, also
// gibt es immer einen freien Platz und jede Suche endet. Löschen verschiebt
// die Folgeeinträge zurück (keine Grabsteine), damit die Tabelle über einen
// langen Flug nicht zuläuft. Keine Allokation.

namespace {

size_t streue(DatarefHandle h) noexcept {
    uint64_t x = static_cast<uint64_t>(reinterpret_cast<uintptr_t>(h));
    x ^= x >> 29;
    x *= 0x9E3779B97F4A7C15ull;
    x ^= x >> 32;
    return static_cast<size_t>(x);
}

constexpr size_t LANGSAM_MASKE = grenzen::LANGSAM_PLAETZE - 1;
static_assert((grenzen::LANGSAM_PLAETZE & LANGSAM_MASKE) == 0, "Zweierpotenz");

}  // namespace

size_t Dienst::langsam_platz(DatarefHandle h) const noexcept {
    if (h == nullptr || langsam_belegt_ == 0) return grenzen::LANGSAM_PLAETZE;
    size_t i = streue(h) & LANGSAM_MASKE;
    for (size_t n = 0; n < grenzen::LANGSAM_PLAETZE; ++n) {
        if (langsam_[i].h == nullptr) return grenzen::LANGSAM_PLAETZE;
        if (langsam_[i].h == h) return i;
        i = (i + 1) & LANGSAM_MASKE;
    }
    return grenzen::LANGSAM_PLAETZE;
}

Dienst::Langsam* Dienst::langsam_finde(DatarefHandle h) noexcept {
    const size_t i = langsam_platz(h);
    return i < grenzen::LANGSAM_PLAETZE ? &langsam_[i] : nullptr;
}

bool Dienst::ist_gedrosselt(DatarefHandle h) const noexcept {
    const size_t i = langsam_platz(h);
    return i < grenzen::LANGSAM_PLAETZE && langsam_[i].gedrosselt;
}

Dienst::Langsam* Dienst::langsam_anlegen(DatarefHandle h) noexcept {
    if (h == nullptr) return nullptr;
    if (Langsam* l = langsam_finde(h)) return l;
    if (langsam_belegt_ >= grenzen::LANGSAM_PLAETZE / 4 * 3) return nullptr;
    size_t i = streue(h) & LANGSAM_MASKE;
    while (langsam_[i].h != nullptr) i = (i + 1) & LANGSAM_MASKE;
    langsam_[i] = Langsam{};
    langsam_[i].h = h;
    ++langsam_belegt_;
    return &langsam_[i];
}

void Dienst::langsam_loeschen(size_t i) noexcept {
    size_t j = i;
    for (;;) {
        j = (j + 1) & LANGSAM_MASKE;
        if (langsam_[j].h == nullptr) break;
        const size_t k = streue(langsam_[j].h) & LANGSAM_MASKE;
        // Eintrag j darf auf das Loch i rücken, wenn sein Heimplatz k NICHT
        // zyklisch in (i, j] liegt — sonst fände ihn die Suche dort nicht mehr.
        const bool dazwischen = (i <= j) ? (i < k && k <= j) : (i < k || k <= j);
        if (!dazwischen) {
            langsam_[i] = langsam_[j];
            i = j;
        }
    }
    langsam_[i] = Langsam{};
    --langsam_belegt_;
}

void Dienst::langsam_leeren() noexcept {
    for (Langsam& l : langsam_) l = Langsam{};
    langsam_belegt_ = 0;
}

// =============================================================================
// Anfragen
// =============================================================================

void Dienst::empfang_beginnen() noexcept {
    empfang_ende_ = umgebung_.jetzt() + grenzen::EMPFANG_BUDGET_S;
    empfang_frei_ = true;
}

bool Dienst::empfang_weiter() noexcept {
    if (empfang_frei_) {
        empfang_frei_ = false;
        return true;
    }
    return umgebung_.jetzt() < empfang_ende_;
}

void Dienst::empfange(const Absender& von, const char* daten, size_t laenge) noexcept {
    if (!bereit_) return;
    const double jetzt = umgebung_.jetzt();
    const bool vom_client = client_aktiv_ && von == client_;
    // Jedes Datagramm des Clients — auch ein fehlerhaftes — beweist, dass er
    // lebt. Die Zeitüberschreitung soll tote Clients abräumen, nicht
    // fehlerhafte bestrafen.
    if (vom_client) letzte_anfrage_ = jetzt;

    Anfrage a;
    if (!zerlege_anfrage(daten, laenge, namen_puffer_.daten(),
                         namen_puffer_.kapazitaet(), &a)) {
        sende_fehler(von, a.fehler, a.fehler_zeile, a.abo_id, a.generation, -1);
        return;
    }

    if (a.befehl == Befehl::HALLO) {
        bearbeite_hallo(von, a);
        return;
    }
    if (!vom_client) {
        // Kein HALLO, anderer Port (Client neu gestartet) oder nach
        // Zeitüberschreitung vergessen: der Client muss sich neu melden.
        sende_fehler(von, Fehlergrund::KEIN_HALLO, 0, a.abo_id, a.generation, -1);
        return;
    }

    switch (a.befehl) {
        case Befehl::PING: {
            char puffer[32];
            JsonSchreiber w(puffer, sizeof(puffer));
            w.roh("{\"p\":2,\"t\":\"pong\"}\n");
            sende_einzeln(von, w);
            break;
        }
        case Befehl::ENDE_ABO: {
            // Auch für eine ID ohne Abo kein Fehler und keine Antwort: der
            // Client räumt beim Sitzungsstart vorsorglich alle IDs ab.
            const size_t i = a.abo_id - 1;
            abo_leeren(abos_[i]);
            bau_[i].aktiv = false;
            bau_[i].text.freigeben();
            bau_[i].namen.freigeben();
            break;
        }
        case Befehl::LISTE:
            bearbeite_liste(a);
            break;
        case Befehl::ABO:
            bearbeite_abo(a);
            break;
        case Befehl::HALLO:
        case Befehl::KEINER:
            break;
    }
}

void Dienst::bearbeite_hallo(const Absender& von, const Anfrage& a) noexcept {
    if (a.protokoll == grenzen::PROTOKOLL) {
        if (!client_aktiv_ || !(von == client_)) {
            // Neuer Client (oder derselbe nach Neustart mit neuem Port): alles
            // vom alten verwerfen — dessen Abos kennt der neue nicht.
            alles_verwerfen();
            client_ = von;
            client_aktiv_ = true;
            char version[grenzen::MAX_CLIENT_VERSION + 1];
            const size_t n = a.client_version_laenge < grenzen::MAX_CLIENT_VERSION
                                 ? a.client_version_laenge : grenzen::MAX_CLIENT_VERSION;
            std::memcpy(version, a.client_version, n);
            version[n] = '\0';
            protokolliere("Protokoll 2: Client angemeldet (Port %u, Version %s)",
                          static_cast<unsigned>(von.port), version);
        }
        letzte_anfrage_ = umgebung_.jetzt();
        flugzeug_offen_ = true;  // aktuelles Flugzeug gleich mitteilen
    }
    // Die Antwort geht auch bei fremder Protokollversion hinaus: an "p":2
    // erkennt der Client, dass er ein anderes Plugin vor sich hat.
    char puffer[160];
    JsonSchreiber w(puffer, sizeof(puffer));
    w.roh("{\"p\":2,\"t\":\"hallo\",\"plugin\":");
    w.text_bis_nul(kennung_.plugin_version, 32);
    w.roh(",\"xplane\":");
    w.ganzzahl(kennung_.xplane_version);
    w.roh(",\"xplm\":");
    w.ganzzahl(kennung_.xplm_version);
    w.roh("}\n");
    sende_einzeln(von, w);
}

void Dienst::bearbeite_abo(const Anfrage& a) noexcept {
    const uint32_t id = a.abo_id;
    AboBau& bau = bau_[id - 1];
    auto verwerfe_bau = [&bau]() noexcept {
        bau.aktiv = false;
        bau.text.freigeben();
        bau.namen.freigeben();
    };

    const uint32_t gen = a.generation;
    if (a.teil == 1) {
        // Teil 1 beginnt immer neu — auch wenn ein älterer Aufbau offen war.
        verwerfe_bau();
        bau.aktiv = true;
        bau.rate = a.rate_hz;
        bau.teile = a.teile;
        bau.generation = gen;
        bau.naechster = 1;
    } else if (!bau.aktiv || a.teil != bau.naechster) {
        // Fehlender, doppelter oder vertauschter Teil. Kein Raten, kein
        // Umsortieren: der ganze Aufbau ist ungültig, der Client schickt neu.
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::ABO_TEIL_REIHENFOLGE, 0, id, gen, -1);
        return;
    } else if (a.rate_hz != bau.rate || a.teile != bau.teile || gen != bau.generation) {
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::ABO_TEILE_WIDERSPRUCH, 0, id, gen, -1);
        return;
    }

    if (bau.namen.anzahl() + a.namen_anzahl > grenzen::MAX_NAMEN_JE_ABO) {
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::ZU_VIELE_NAMEN, 0, id, gen, -1);
        return;
    }
    // Speicher für diesen Teil GENAU reservieren (nicht verdoppeln: die
    // Kapazität wandert mit dem letzten Teil ins Abo und zählt dort) — und
    // vorher gegen die Budgets prüfen (H3).
    size_t teil_text = 0;
    for (size_t i = 0; i < a.namen_anzahl; ++i) {
        teil_text += (a.namen[i].ungueltig ? 0 : a.namen[i].basis_laenge) + 1u;
    }
    const size_t text_noetig = bau.text.anzahl() + teil_text;
    const size_t namen_noetig = bau.namen.anzahl() + a.namen_anzahl;
    {
        // Das alte Abo derselben ID wird ersetzt — es zählt nicht mit
        // (Nachprüfung Claude, Punkt 7; sonst scheiterte ein Ersatz nahe
        // 64 MiB unnötig). Vorübergehend, bis zum letzten Teil, belegen
        // altes Abo und Aufbau zusammen höchstens 16 MiB mehr.
        const size_t alt = bau_bytes(bau) + abo_bytes(abos_[id - 1]);
        const size_t text_kap = text_noetig > bau.text.kapazitaet() ? text_noetig : bau.text.kapazitaet();
        const size_t namen_kap = namen_noetig > bau.namen.kapazitaet() ? namen_noetig : bau.namen.kapazitaet();
        if (text_noetig > 0x7FFFFFFFu ||
            !abo_speicher_passt(alt, text_kap + namen_kap * sizeof(RohName))) {
            verwerfe_bau();
            sende_fehler(client_, Fehlergrund::SPEICHER_LIMIT, 0, id, gen, -1);
            return;
        }
    }
    if (!bau.text.reserviere(text_noetig) || !bau.namen.reserviere(namen_noetig)) {
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::SPEICHER, 0, id, gen, -1);
        return;
    }
    for (size_t i = 0; i < a.namen_anzahl; ++i) {
        const NameRef& n = a.namen[i];
        const size_t ofs = bau.text.anzahl();
        const char nul = '\0';
        // Ungültige Zeile: leerer Name, Index-Marke "ungültig" (Status fehlt).
        const int32_t index = n.ungueltig ? INDEX_UNGUELTIG : n.index;
        const size_t laenge = n.ungueltig ? 0 : n.basis_laenge;
        // Kapazität ist reserviert: haenge_an wächst hier nicht mehr.
        if (!bau.text.haenge_an(n.basis, laenge) ||
            !bau.text.haenge_an(nul) ||
            !bau.namen.haenge_an(RohName{static_cast<uint32_t>(ofs), index})) {
            verwerfe_bau();
            sende_fehler(client_, Fehlergrund::SPEICHER, 0, id, gen, -1);
            return;
        }
    }
    ++bau.naechster;
    if (a.teil < a.teile) return;  // weitere Teile folgen

    const size_t n_namen = bau.namen.anzahl();
    if (n_namen == 0) {
        verwerfe_bau();
        sende_fehler(client_, Fehlergrund::KEINE_NAMEN, 0, id, gen, -1);
        return;
    }
    Abo& alt = abos_[id - 1];
    if (gleiches_abo(alt, bau)) {
        // Dasselbe ABO noch einmal (Client hat nachgefragt, weil der Status
        // auf sich warten ließ): NICHTS zurücksetzen — eine laufende Suche
        // liefe sonst jedes Mal von vorn und käme nie an. Ist der Status schon
        // draußen, geht er noch einmal VOLLSTÄNDIG hinaus (alle Teile) — so
        // kann der Client ein verlorenes Status-Fragment nachfordern.
        if (!alt.erste_antwort) alt.antwort_erneut = true;
        sende_abo_empfangen(id, gen, n_namen);
        verwerfe_bau();
        return;
    }
    const Fehlergrund f = aktiviere(id, bau);
    if (f != Fehlergrund::KEINER) {
        sende_fehler(client_, f, 0, id, gen, -1);
    } else {
        sende_abo_empfangen(id, gen, n_namen);
    }
    verwerfe_bau();
}

bool Dienst::gleiches_abo(const Abo& abo, const AboBau& bau) noexcept {
    if (abo.phase == Phase::LEER || abo.rate != bau.rate ||
        abo.generation != bau.generation ||
        abo.eintraege.anzahl() != bau.namen.anzahl() ||
        abo.text.anzahl() != bau.text.anzahl()) {
        return false;
    }
    if (abo.text.anzahl() > 0 &&
        std::memcmp(abo.text.daten(), bau.text.daten(), abo.text.anzahl()) != 0) {
        return false;
    }
    for (size_t i = 0; i < bau.namen.anzahl(); ++i) {
        if (abo.eintraege[i].name_ofs != bau.namen[i].ofs ||
            abo.eintraege[i].index != bau.namen[i].index) {
            return false;
        }
    }
    return true;
}

Fehlergrund Dienst::aktiviere(uint32_t id, AboBau& bau) noexcept {
    Abo& abo = abos_[id - 1];
    const size_t n = bau.namen.anzahl();

    // Budget VOR dem Abräumen prüfen: nachher belegt das neue Abo den Text
    // des Aufbaus plus Einträge, Paketplan und den Stapel für die erste
    // Status-Antwort; das alte Abo und der Aufbau sind dann frei. Ein neues
    // ABO mit gleicher ID ersetzt das alte auch dann, wenn es selbst am
    // Budget scheitert — danach ist Abo <id> leer, nie ein Gemisch.
    const size_t neu = bau.text.kapazitaet() + n * sizeof(Eintrag) + (n + 2) * sizeof(uint32_t) +
                       n * STATUS_MAX + n * sizeof(uint32_t);
    const bool passt = abo_speicher_passt(abo_bytes(abo) + bau_bytes(bau), neu);
    abo_leeren(abo);  // ein neues ABO mit gleicher ID ersetzt das alte
    if (!passt) return Fehlergrund::SPEICHER_LIMIT;

    abo.text.uebernehme(bau.text);
    if (!abo.eintraege.reserviere(n) ||
        !abo.grenzen.reserviere(n + 2) ||
        !abo.stapel.reserviere(n * STATUS_MAX, n)) {
        abo_leeren(abo);
        return Fehlergrund::SPEICHER;
    }
    for (size_t i = 0; i < n; ++i) {
        Eintrag e;
        e.name_ofs = bau.namen[i].ofs;
        e.index = bau.namen[i].index;
        // X-Planes eigene "sim/…"-Datarefs werden nie verwaist; nur für alle
        // anderen lohnt die Gültigkeitsprüfung vor jedem Lesen.
        const char* name = abo.text.daten() + e.name_ofs;
        e.darf_verwaisen = e.index != INDEX_UNGUELTIG && std::strncmp(name, "sim/", 4) != 0;
        if (e.darf_verwaisen) abo.hat_plugin_namen = true;
        e.bedient_runde = 0;
        e.aktiv = Aufloesung{};
        e.kandidat = Aufloesung{};
        abo.eintraege.haenge_an(e);  // Kapazität reserviert
    }
    abo.id = id;
    abo.rate = bau.rate;
    abo.generation = bau.generation;
    abo.periode = 1.0 / static_cast<double>(bau.rate);
    abo.phase = Phase::NEU;
    abo.erste_antwort = true;
    abo.seq = 0;
    starte_pruefung(abo, true, true);  // sofort suchen, nicht erst in 2 s
    return Fehlergrund::KEINER;
}

void Dienst::bearbeite_liste(const Anfrage& a) noexcept {
    if (!quelle_.liste_verfuegbar()) {
        sende_fehler(client_, Fehlergrund::LISTE_NICHT_VERFUEGBAR, 0, 0, 0, a.anfrage_id);
        return;
    }
    // Eine neue LISTE ersetzt eine laufende (der Client wartet ohnehin nur auf
    // die jüngste Antwort).
    liste_leeren();
    // XPLMCountDataRefs erst im ersten budgetierten Schritt (liste_schritt),
    // nicht hier beim Empfang (Nachprüfung Codex H4).
    liste_.aktiv = true;
    liste_.sammeln = true;
    liste_.id = a.anfrage_id;
    liste_.gesamt = -1;
    liste_.cursor = 0;
    liste_.block_pos = 0;
    liste_.block_anzahl = 0;
    liste_.ausgelassen = 0;
}

// =============================================================================
// Frame
// =============================================================================

void Dienst::frame() noexcept {
    if (!bereit_) return;
    frame_intern();
    // Der gemeinsame Ausgang (M1) zählt einen Flight-Loop-Aufruf: erst die
    // Antworten beim Empfang, dann dieser Frame. Deshalb erst HIER zurück —
    // die Antworten des nächsten Empfangs zählen dann zum nächsten Frame.
    pakete_frame_ = 0;
    kleine_frame_ = 0;
}

void Dienst::frame_intern() noexcept {
    jetzt_ = umgebung_.jetzt();
    uhr_ = jetzt_;

    if (kleine_verworfen_ > 0 && jetzt_ >= naechste_flut_meldung_) {
        protokolliere("Protokoll 2: %u kleine Antworten verworfen (hoechstens %d je Frame)",
                      static_cast<unsigned>(kleine_verworfen_), grenzen::MAX_KLEINE_JE_FRAME);
        kleine_verworfen_ = 0;
        naechste_flut_meldung_ = jetzt_ + 10.0;
    }

    if (!client_aktiv_) return;
    if (jetzt_ - letzte_anfrage_ > grenzen::CLIENT_TIMEOUT_S) {
        // Abgestürzter oder beendeter Client: keine Last im Sim zurücklassen.
        protokolliere("Protokoll 2: Client seit %.0f s stumm - Abos verworfen",
                      grenzen::CLIENT_TIMEOUT_S);
        alles_verwerfen();
        client_aktiv_ = false;
        return;
    }

    // Vorgemerkte Fehler (aus der Lieferung des letzten Frames) zuerst.
    sende_vorgemerkte_fehler();

    // 1. Flugzeugkennung und Suchen teilen sich das Such-Budget. Die
    //    Kennung zuerst (fortsetzbar, eine Einheit je Dataref): nach einem
    //    Flugzeugwechsel geht `flugzeug` so vor jeder Neusuche hinaus — die
    //    Suche bekommt in einem Frame erst Budget, wenn die Kennung fertig ist.
    langsame_frame_ = 0;
    waehle_langsam_vorrang();
    beginne_budget(grenzen::SUCH_BUDGET_S);
    pruefe_flugzeug();
    suchen_verteilen();

    // 2. Liefern — eigenes Budget (getrennt vom Suchen). Abo 1 (beim Client
    //    die Telemetrie) immer zuerst, danach Rundlauf NUR über belegte Abos
    //    UND die LISTE: jeder Teilnehmer ist regelmäßig als erster dran und
    //    bekommt dann das ganze Budget samt freier Einheit. (Vorher kam LISTE
    //    immer zuletzt und verhungerte hinter einem Dauer-Abo, das jedes
    //    Budget aufbrauchte — Codex-Abnahme M2.)
    //    Abo 1 hat Vorrang (Pünktlichkeit), aber nur ein Teilbudget
    //    (grenzen::ABO1_BUDGET_S): Danach beginnt für den Rundlauf der Rest
    //    des Liefer-Budgets MIT einer eigenen freien Einheit — ein großes
    //    oder teures Abo 1 kann die anderen und LISTE so nicht mehr
    //    aushungern (Nachprüfung Codex M2).
    beginne_budget(grenzen::ABO1_BUDGET_S);
    const double liefer_ende = uhr_ + grenzen::ZEITBUDGET_S;
    bearbeite_abo_frame(abos_[0]);
    uhr_auffrischen();
    budget_ende_ = liefer_ende;
    garantie_ = true;
    uint32_t teilnehmer[grenzen::MAX_ABOS + 1];
    uint32_t anzahl = 0;
    for (uint32_t i = 1; i < grenzen::MAX_ABOS; ++i) {
        if (abos_[i].phase != Phase::LEER) teilnehmer[anzahl++] = i;
    }
    if (liste_.aktiv) teilnehmer[anzahl++] = LISTE_PLATZ;
    if (anzahl > 0) {
        const uint32_t start = rundlauf_++ % anzahl;
        for (uint32_t k = 0; k < anzahl; ++k) {
            const uint32_t t = teilnehmer[(start + k) % anzahl];
            if (t == LISTE_PLATZ) {
                liste_schritt();
            } else {
                bearbeite_abo_frame(abos_[t]);
            }
        }
    }
}

void Dienst::verwirf_laufende_runde(Abo& abo) noexcept {
    switch (abo.phase) {
        case Phase::LESEN:
        case Phase::SENDEN:
            // Werte, die vor dem Wechsel gelesen wurden (auch schon geplante,
            // noch nicht gesendete Pakete), gehören zum alten Flugzeug. Die
            // Runde endet hier; der Client sieht höchstens eine unvollständige
            // Runde (wie bei UDP-Verlust) und nie einen alten Wert danach.
            abo.stapel.leeren();
            abo.lese_cursor = 0;
            abo.paket_cursor = 0;
            abo.teile = 0;
            abo.phase = Phase::BEREIT;
            break;
        case Phase::ANTWORT:
            // Status des alten Flugzeugs, womöglich schon zum Teil draußen.
            // Nicht zu Ende senden. Zurück nach NEU: dort gibt es KEINE Werte,
            // bis die Neusuche übernommen ist und ein vollständiger neuer
            // Status draußen ist (antwort_erneut erzwingt ihn, auch wenn sich
            // nichts geändert hat). BEREIT wäre falsch — nach der Pausen-
            // grenze kämen Werte zu einem Status, den der Client nie ganz
            // bekommen hat.
            abo.stapel.leeren();
            abo.paket_cursor = 0;
            abo.teile = 0;
            abo.antwort_erneut = true;
            abo.phase = Phase::NEU;
            break;
        case Phase::LEER:
        case Phase::NEU:
        case Phase::BEREIT:
            break;
    }
}

void Dienst::flugzeug_geladen() noexcept {
    flugzeug_offen_ = true;
    flugzeug_stufe_ = -1;  // eine laufende Kennungs-Prüfung liest neu
    // Anderes Flugzeug, andere Plugins: Drosselung neu bewerten.
    langsam_leeren();
    for (Abo& a : abos_) {
        if (a.phase == Phase::LEER) continue;
        starte_pruefung(a, true, true);
        // Namen fremder Plugins können jetzt verwaist oder neu belegt sein
        // (Codex-Abnahme H2): Eine laufende Runde wird verworfen, und bis die
        // Neusuche übernommen ist, keine neue Runde mit alten Handles —
        // höchstens grenzen::MAX_PAUSE_S lang (Begründung dort). Abos nur mit
        // "sim/…"-Namen (Telemetrie) liefern ohne Pause weiter; X-Planes
        // eigene Datarefs wechseln mit dem Flugzeug nicht den Besitzer.
        // Die laufende Runde verwirft JEDES Abo (Nachprüfung Claude, Punkt 5:
        // auch reine sim/-Abos sendeten sonst noch vor dem Wechsel gelesene
        // Werte nach dem flugzeug-Paket und mischten beide in einer Runde).
        verwirf_laufende_runde(a);
        if (!a.hat_plugin_namen) {
            a.faellig = 0.0;  // ohne Pause: sofort eine neue Runde
        } else {
            a.pausiert = true;
            // Uhr startet erst im ersten Frame danach (Nachpruefung AP7):
            // X-Plane laedt nach der Meldung oft Sekunden ohne Frame — ab der
            // Meldung gezaehlt waere die Pause dann schon abgelaufen.
            a.pause_bis = -1.0;
        }
    }
}

void Dienst::flughafen_geladen() noexcept {
    for (Abo& a : abos_) {
        if (a.phase != Phase::LEER) starte_pruefung(a, true, true);
    }
}

void Dienst::starte_pruefung(Abo& abo, bool alle, bool dringend) noexcept {
    if (abo.phase == Phase::LEER) return;
    abo.pruefung_laeuft = true;
    abo.pruefung_fertig = false;
    abo.pruefung_alle = alle;
    abo.pruefung_dringend = dringend;
    abo.pruef_cursor = 0;
    abo.such_h = nullptr;  // Längen-Zwischenspeicher gilt je Durchlauf
}

bool Dienst::braucht_nachsuche(const Eintrag& e) noexcept {
    if (e.index == INDEX_UNGUELTIG) return false;
    switch (e.aktiv.z) {
        case Zugriff::FEHLT:    // taucht er inzwischen auf?
        case Zugriff::VI:       // Arrays: hat sich die Länge geändert?
        case Zugriff::VF:
        case Zugriff::B:
        case Zugriff::ELEM_VI:
        case Zugriff::ELEM_VF:
        case Zugriff::ELEM_B:
            return true;
        case Zugriff::I:        // Einzelwerte: Verwaisen fängt das Lesen ab
        case Zugriff::F:
        case Zugriff::D:
            return false;
    }
    return true;
}

// Ein Eintrag des Prüfdurchlaufs: übersprungene Einträge (periodischer
// Durchlauf, vorhandener Einzelwert, gedrosselter Dataref) kosten keinen
// XPLM-Aufruf und zählen nicht; der Schritt endet nach genau einer echten
// Suche. false: Budget mitten in der Suche erschöpft — der Eintrag bleibt
// offen und beginnt im nächsten Frame neu.
bool Dienst::pruef_schritt(Abo& abo) noexcept {
    const size_t n = abo.eintraege.anzahl();
    while (abo.pruef_cursor < n) {
        Eintrag& e = abo.eintraege[abo.pruef_cursor];
        if (e.index == INDEX_UNGUELTIG) {
            e.kandidat = Aufloesung{};
            ++abo.pruef_cursor;
            continue;
        }
        // Periodisch: nur fehlende Namen und Arrays — und keine gedrosselten
        // (deren Längen-Accessor ist der langsame Getter selbst; neu gesucht
        // werden sie bei Anmeldung und Flugzeug-/Flughafenwechsel).
        if (!abo.pruefung_alle && (!braucht_nachsuche(e) || ist_gedrosselt(e.aktiv.h))) {
            e.kandidat = e.aktiv;
            ++abo.pruef_cursor;
            continue;
        }
        Aufloesung r;
        if (!loese_auf(abo, e, &r)) return false;
        e.kandidat = r;
        ++abo.pruef_cursor;
        einheit_fertig();
        break;
    }
    if (abo.pruef_cursor >= n) {
        abo.pruefung_laeuft = false;
        abo.pruefung_fertig = true;
        abo.naechste_pruefung = jetzt_ + grenzen::NACHSUCHE_INTERVALL_S;
    }
    return true;
}

// Verteilt das Such-Budget (grenzen::SUCH_BUDGET_S) auf die laufenden
// Prüfdurchläufe:
//   1. dringende (Anmeldung, Flugzeug-/Flughafenwechsel) vor periodischen,
//   2. darunter das Abo mit dem KLEINSTEN Rest zuerst.
// Kürzester Rest zuerst ergibt die kleinste mittlere Wartezeit auf den ersten
// Status: ein kleines Abo (Telemetrie) ist nach einem Frame fertig, auch wenn
// daneben sechs Vermessungs-Abos mit je 8192 Namen suchen; die großen kommen
// nacheinander dran statt alle gleichzeitig spät. Verhungern kann keines —
// jeder Durchlauf wird nur kleiner.
void Dienst::suchen_verteilen() noexcept {
    for (Abo& a : abos_) {
        if (a.phase != Phase::LEER && !a.pruefung_laeuft && !a.pruefung_fertig &&
            jetzt_ >= a.naechste_pruefung) {
            starte_pruefung(a, false, false);
        }
    }
    // Budget hat frame_intern schon begonnen (geteilt mit pruefe_flugzeug).
    for (;;) {
        Abo* bestes = nullptr;
        for (Abo& a : abos_) {
            if (a.phase == Phase::LEER || !a.pruefung_laeuft) continue;
            if (bestes == nullptr) { bestes = &a; continue; }
            const size_t rest_a = a.eintraege.anzahl() - a.pruef_cursor;
            const size_t rest_b = bestes->eintraege.anzahl() - bestes->pruef_cursor;
            if (a.pruefung_dringend != bestes->pruefung_dringend) {
                if (a.pruefung_dringend) bestes = &a;
            } else if (rest_a < rest_b) {
                bestes = &a;
            }
        }
        if (bestes == nullptr) return;
        while (bestes->pruefung_laeuft) {
            if (!darf_arbeiten()) return;
            if (!pruef_schritt(*bestes)) return;
        }
    }
}

// =============================================================================
// Abos
// =============================================================================

// Sucht einen Namen auf. Vor JEDEM XPLM-Aufruf gegen das Budget, nach jedem
// die Uhr (H4). false = Budget dazwischen erschöpft; *aus ist dann bedeutungslos.
bool Dienst::loese_auf(Abo& abo, Eintrag& e, Aufloesung* aus) noexcept {
    *aus = Aufloesung{};
    const char* name = abo.text.daten() + e.name_ofs;
    const int32_t index = e.index;
    if (index == INDEX_UNGUELTIG || name[0] == '\0') return true;

    if (!darf_arbeiten()) return false;
    const DatarefHandle h = quelle_.finde(name);
    nach_aufruf();
    if (h == nullptr) return true;

    // Verwaist: Das Plugin, das den Namen angelegt hat, ist entladen (z. B.
    // nach Flugzeugwechsel). XPLMFindDataRef findet ihn weiter, lesen ergibt
    // aber 0 — genau die Lüge, die Protokoll 2 abschaffen soll.
    if (!darf_arbeiten()) return false;
    const bool gut = quelle_.ist_gueltig(h);
    nach_aufruf();
    if (!gut) return true;

    if (!darf_arbeiten()) return false;
    const int t = quelle_.typen(h);
    nach_aufruf();

    Aufloesung r;
    r.h = h;
    if (index < 0) {
        // Vorrang bei mehreren Typen (ADR): double > float > int > Arrays >
        // Bytes; unter den Arrays float vor int.
        if (t & typ::D)      { r.z = Zugriff::D; r.laenge = 1; *aus = r; return true; }
        if (t & typ::F)      { r.z = Zugriff::F; r.laenge = 1; *aus = r; return true; }
        if (t & typ::I)      { r.z = Zugriff::I; r.laenge = 1; *aus = r; return true; }
        if (!(t & (typ::VF | typ::VI | typ::B))) return true;  // kein lesbarer Typ → fehlt
        r.z = (t & typ::VF) ? Zugriff::VF : (t & typ::VI) ? Zugriff::VI : Zugriff::B;
        int n = 0;
        if (!array_laenge(abo, e, h, t, &n)) return false;
        r.laenge = begrenze(n, r.z == Zugriff::B ? grenzen::MAX_BYTES : grenzen::MAX_ARRAY_ELEMENTE);
        *aus = r;
        return true;
    }
    // Array-Element: nur bei Array-Typen, Index gegen die AKTUELLE Länge.
    // Außerhalb der Länge gilt der Name als fehlend — wächst das Array später,
    // findet ihn der nächste Prüfdurchlauf.
    if (!(t & (typ::VF | typ::VI | typ::B))) return true;
    const Zugriff z = (t & typ::VF) ? Zugriff::ELEM_VF : (t & typ::VI) ? Zugriff::ELEM_VI : Zugriff::ELEM_B;
    int laenge = -1;
    if (!array_laenge(abo, e, h, t, &laenge)) return false;
    if (laenge <= 0 || index >= laenge) return true;
    r.z = z;
    r.laenge = 1;
    *aus = r;
    return true;
}

// Array-Länge über den fremden Accessor (mit nullptr aufgerufen) — aber
// sparsam (Nachprüfung Claude, Punkt 2):
//   * je Handle einmal im Prüfdurchlauf: Duplikate und Elemente desselben
//     Arrays teilen die Länge (vorher: 256 Elemente → 256 Aufrufe);
//   * ein gedrosselter Handle mit bekannter Länge wird nur gefragt, wenn er
//     auch lesen dürfte (fällig, Sperre, reihum) — auch in dringenden Läufen.
// false = Budget (Eintrag unerledigt).
bool Dienst::array_laenge(Abo& abo, Eintrag& e, DatarefHandle h, int t, int* n) noexcept {
    if (abo.such_h == h) {
        *n = abo.such_n;
        return true;
    }
    if (langsam_belegt_ > 0) {
        Langsam* l = langsam_finde(h);
        if (l != nullptr && l->gedrosselt && l->laenge_bekannt && !darf_langsam_lesen(*l)) {
            *n = l->laenge;
            return true;
        }
    }
    if (!darf_arbeiten(true)) return false;
    int laenge;
    if (t & typ::VF)      laenge = quelle_.lese_vf(h, nullptr, 0, 0);
    else if (t & typ::VI) laenge = quelle_.lese_vi(h, nullptr, 0, 0);
    else                  laenge = quelle_.lese_b(h, nullptr, 0, 0);
    nach_fremdaufruf(abo, e, h);
    if (langsam_belegt_ > 0) {
        Langsam* l = langsam_finde(h);  // neu: nach_fremdaufruf kann Plätze verschieben
        if (l != nullptr) {
            l->laenge = laenge;
            l->laenge_bekannt = true;
            if (l->gedrosselt) {
                l->faellig = jetzt_ + grenzen::LANGSAM_INTERVALL_S;
                naechster_langsamer_ = jetzt_ + grenzen::LANGSAM_ABSTAND_S;
            }
        }
    }
    abo.such_h = h;
    abo.such_n = laenge;
    *n = laenge;
    return true;
}

size_t Dienst::wert_max(const Aufloesung& a) noexcept {
    const size_t n = a.laenge > 0 ? static_cast<size_t>(a.laenge) : 0;
    switch (a.z) {
        case Zugriff::FEHLT:   return 0;
        case Zugriff::I:
        case Zugriff::ELEM_VI:
        case Zugriff::ELEM_B:  return WERT_RAHMEN + JSON_MAX_INT32;
        case Zugriff::F:
        case Zugriff::ELEM_VF: return WERT_RAHMEN + JSON_MAX_FLOAT;
        case Zugriff::D:       return WERT_RAHMEN + JSON_MAX_DOUBLE;
        case Zugriff::VI:      return WERT_RAHMEN + 2 + n * (JSON_MAX_INT32 + 1);
        case Zugriff::VF:      return WERT_RAHMEN + 2 + n * (JSON_MAX_FLOAT + 1);
        case Zugriff::B:       return WERT_RAHMEN + json_max_text(n);
    }
    return 0;
}

namespace {

const char* status_art(uint8_t z) noexcept {
    // Reihenfolge wie Dienst::Zugriff.
    static const char* const ARTEN[] = {
        "fehlt", "i", "f", "d", "vi", "vf", "b", "i", "f", "i",
    };
    return z < sizeof(ARTEN) / sizeof(ARTEN[0]) ? ARTEN[z] : "fehlt";
}

}  // namespace

Fehlergrund Dienst::uebernehme_pruefung(Abo& abo, bool* geaendert) noexcept {
    *geaendert = false;
    size_t werte_bytes = 0;
    const size_t n = abo.eintraege.anzahl();
    for (size_t i = 0; i < n; ++i) {
        Eintrag& e = abo.eintraege[i];
        const bool status_gleich =
            std::strcmp(status_art(static_cast<uint8_t>(e.aktiv.z)),
                        status_art(static_cast<uint8_t>(e.kandidat.z))) == 0 &&
            e.aktiv.laenge == e.kandidat.laenge;
        if (!status_gleich) *geaendert = true;
        e.aktiv = e.kandidat;
        werte_bytes += wert_max(e.aktiv);
    }
    const size_t status_bytes = n * STATUS_MAX;
    const size_t noetig = werte_bytes > status_bytes ? werte_bytes : status_bytes;
    // Wachsen nur beim Übernehmen (= Neu-Anmelden im Sinne der ADR), nie beim
    // Lesen. Schrumpfen lohnt nicht. Vorher gegen die Budgets (H3): hier
    // entscheidet sich, was die Werte eines Abos im schlimmsten Fall kosten.
    const size_t kap = abo.stapel.kapazitaet_bytes();
    if (noetig > kap) {
        const size_t alt = abo_bytes(abo);
        if (!abo_speicher_passt(alt, alt - kap + noetig)) return Fehlergrund::SPEICHER_LIMIT;
    }
    return abo.stapel.reserviere(noetig, n) ? Fehlergrund::KEINER : Fehlergrund::SPEICHER;
}

bool Dienst::bereite_antwort(Abo& abo) noexcept {
    abo.stapel.leeren();
    const size_t n = abo.eintraege.anzahl();
    for (size_t k = 0; k < n; ++k) {
        const Eintrag& e = abo.eintraege[k];
        JsonSchreiber w = abo.stapel.schreiber();
        w.zeichen('[');
        w.ganzzahl(static_cast<int64_t>(k));
        w.roh(",\"");
        w.roh(status_art(static_cast<uint8_t>(e.aktiv.z)));
        w.zeichen('"');
        if (e.aktiv.z != Zugriff::FEHLT) {
            w.zeichen(',');
            w.ganzzahl(e.aktiv.laenge);
        }
        w.zeichen(']');
        if (!abo.stapel.uebernehme(w)) return false;
    }
    JsonSchreiber p(abo.praefix, sizeof(abo.praefix));
    p.roh("{\"p\":2,\"t\":\"abo\",\"abo\":");
    p.ganzzahl(abo.id);
    p.roh(",\"gen\":");
    p.ganzzahl(abo.generation);
    if (p.ueberlauf()) return false;
    abo.praefix_laenge = p.laenge();
    abo.feld = "st";
    abo.teile = plane_pakete(abo.praefix_laenge, 2, abo.stapel,
                             abo.grenzen.daten(), abo.grenzen.kapazitaet(),
                             grenzen::MAX_PAKET);
    if (abo.teile == 0) return false;
    abo.paket_cursor = 0;
    abo.phase = Phase::ANTWORT;
    return true;
}

void Dienst::starte_runde(Abo& abo) noexcept {
    ++abo.seq;
    abo.stapel.leeren();
    abo.lese_cursor = 0;
    abo.phase = Phase::LESEN;
}

// Liest EINEN Wert (genau ein fremder Accessor-Aufruf) und schreibt ihn als
// [k,wert]. Die Uhr wird direkt nach dem Accessor gelesen, VOR dem
// Formatieren — sonst zählte die eigene JSON-Arbeit (bis 256 Zahlen) zur
// Dauer des fremden Getters.
bool Dienst::schreibe_wert(JsonSchreiber& w, Abo& abo, Eintrag& e, uint32_t k) noexcept {
    const DatarefHandle h = e.aktiv.h;
    const int laenge = e.aktiv.laenge;
    w.zeichen('[');
    w.ganzzahl(k);
    w.zeichen(',');
    switch (e.aktiv.z) {
        case Zugriff::FEHLT:
            return false;  // ein fehlender Name liefert nie einen Wert
        case Zugriff::I: {
            const int v = quelle_.lese_i(h);
            nach_fremdaufruf(abo, e, h);
            w.ganzzahl(v);
            break;
        }
        case Zugriff::F: {
            const float v = quelle_.lese_f(h);
            nach_fremdaufruf(abo, e, h);
            w.zahl_f(v);
            break;
        }
        case Zugriff::D: {
            const double v = quelle_.lese_d(h);
            nach_fremdaufruf(abo, e, h);
            w.zahl_d(v);
            break;
        }
        case Zugriff::VI: {
            const int n = begrenze(quelle_.lese_vi(h, iwerte_, 0, laenge), laenge);
            nach_fremdaufruf(abo, e, h);
            w.zeichen('[');
            for (int i = 0; i < n; ++i) {
                if (i > 0) w.zeichen(',');
                w.ganzzahl(iwerte_[i]);
            }
            w.zeichen(']');
            break;
        }
        case Zugriff::VF: {
            const int n = begrenze(quelle_.lese_vf(h, fwerte_, 0, laenge), laenge);
            nach_fremdaufruf(abo, e, h);
            w.zeichen('[');
            for (int i = 0; i < n; ++i) {
                if (i > 0) w.zeichen(',');
                w.zahl_f(fwerte_[i]);
            }
            w.zeichen(']');
            break;
        }
        case Zugriff::B: {
            const int n = begrenze(quelle_.lese_b(h, bytes_, 0, laenge), laenge);
            nach_fremdaufruf(abo, e, h);
            // Zeichenkette bis zum ersten NUL (ADR).
            w.text_bis_nul(reinterpret_cast<const char*>(bytes_), static_cast<size_t>(n));
            break;
        }
        case Zugriff::ELEM_VI: {
            const int n = quelle_.lese_vi(h, iwerte_, e.index, 1);
            nach_fremdaufruf(abo, e, h);
            if (n != 1) return false;
            w.ganzzahl(iwerte_[0]);
            break;
        }
        case Zugriff::ELEM_VF: {
            const int n = quelle_.lese_vf(h, fwerte_, e.index, 1);
            nach_fremdaufruf(abo, e, h);
            if (n != 1) return false;
            w.zahl_f(fwerte_[0]);
            break;
        }
        case Zugriff::ELEM_B: {
            const int n = quelle_.lese_b(h, bytes_, e.index, 1);
            nach_fremdaufruf(abo, e, h);
            if (n != 1) return false;
            w.ganzzahl(bytes_[0]);
            break;
        }
    }
    w.zeichen(']');
    return !w.ueberlauf();
}

bool Dienst::lese_schritt(Abo& abo) noexcept {
    const uint32_t n = static_cast<uint32_t>(abo.eintraege.anzahl());
    while (abo.lese_cursor < n) {
        Eintrag& e = abo.eintraege[abo.lese_cursor];
        // Fehlend, oder in dieser Runde schon mit dem Aufruf eines anderen
        // Eintrags desselben (gedrosselten) Handles bedient.
        if (e.aktiv.z == Zugriff::FEHLT || e.bedient_runde == abo.seq) {
            ++abo.lese_cursor;
            continue;
        }
        const DatarefHandle h = e.aktiv.h;
        // Gedrosselt (wiederholt langsamer Accessor, H4): gelesen wird nur,
        // wenn der Handle an der Reihe ist — dann aber mit EINEM Aufruf für
        // ALLE seine Einträge dieser Runde (bediene_gedrosselt). Sonst fällt
        // der Wert in dieser Runde aus; der Client behält den letzten.
        if (langsam_belegt_ > 0) {
            Langsam* l = langsam_finde(h);
            if (l != nullptr && l->gedrosselt) {
                if (darf_langsam_lesen(*l)) {
                    if (!bediene_gedrosselt(abo, abo.lese_cursor)) return false;
                    einheit_fertig();
                }
                ++abo.lese_cursor;
                continue;
            }
        }
        // Verwaist (Plugin entladen/abgeschaltet)? Dann liefert XPLM 0 —
        // also keinen Wert, und der Status geht sofort auf "fehlt" (neue
        // abo-Antwort nach dieser Runde). Auch der Kandidat eines
        // laufenden Durchlaufs, damit er den Namen nicht wieder "da" macht.
        if (e.darf_verwaisen) {
            if (!darf_arbeiten()) return false;
            const bool gut = quelle_.ist_gueltig(h);
            nach_aufruf();
            if (!gut) {
                e.aktiv = Aufloesung{};
                e.kandidat = Aufloesung{};
                abo.status_sofort = true;
                ++abo.lese_cursor;
                einheit_fertig();
                continue;
            }
        }
        // Zwischen Verwaist-Prüfung und Getter noch einmal gegen das Budget.
        // Endet es hier, prüft der nächste Frame den Eintrag neu — die Prüfung
        // von eben gilt nur jetzt, nicht im nächsten Frame.
        if (!darf_arbeiten(true)) return false;
        JsonSchreiber w = abo.stapel.schreiber();
        // Ein Element, das nicht gelesen werden konnte (Array inzwischen
        // kürzer) oder wider Erwarten nicht passt, fällt in dieser Runde
        // aus — es wird nie abgeschnitten.
        const bool gut = schreibe_wert(w, abo, e, abo.lese_cursor);
        if (gut) abo.stapel.uebernehme(w);
        // In dieser Runde erledigt — wird der Handle später in derselben
        // Runde gedrosselt und fällig, bedient bediene_gedrosselt diesen
        // Eintrag nicht noch einmal (sonst doppelter Index, Nachweis I).
        e.bedient_runde = abo.seq;
        ++abo.lese_cursor;
        einheit_fertig();
    }
    return true;
}

// Ein gedrosselter Handle ist an der Reihe: EIN Getter-Aufruf, daraus
// werden ALLE noch offenen Einträge dieses Handles in dieser Runde bedient
// (Nachprüfung Claude, Punkt 1 — vorher bekam immer nur der erste passende
// Eintrag den Wert: addon/eng[0] 29 Werte in 30 s, eng[1..7] keinen).
//   * Arrays: ein Aufruf über das Fenster, das alle gebrauchten Indizes
//     abdeckt (ganze Arrays [0, Länge), Elemente [i, i+1)). Höchstens
//     256 Elemente bzw. 1024 Byte je Aufruf (Puffer mit 4-facher Reserve);
//     liegen die Indizes weiter auseinander, wandert das Fenster reihum
//     (Langsam::fenster) — jedes kommt dran.
//   * Skalare (i/f/d): ein Aufruf. Hat derselbe Handle Skalar- UND
//     Array-Einträge (mehrere Typen), wechseln sich die beiden Gruppen ab.
// false = Budget (dann ist nichts gelesen).
bool Dienst::bediene_gedrosselt(Abo& abo, uint32_t k) noexcept {
    const DatarefHandle h = abo.eintraege[k].aktiv.h;
    const size_t n = abo.eintraege.anzahl();
    auto passt = [&](const Eintrag& x) noexcept {
        return x.aktiv.h == h && x.aktiv.z != Zugriff::FEHLT && x.bedient_runde != abo.seq;
    };
    // Verwaist? (einmal für den Handle)
    if (abo.eintraege[k].darf_verwaisen) {
        if (!darf_arbeiten()) return false;
        const bool gut = quelle_.ist_gueltig(h);
        nach_aufruf();
        if (!gut) {
            for (size_t j = 0; j < n; ++j) {
                Eintrag& x = abo.eintraege[j];
                if (x.aktiv.h != h) continue;
                x.aktiv = Aufloesung{};
                x.kandidat = Aufloesung{};
            }
            abo.status_sofort = true;
            return true;
        }
    }
    // Bedarf: Gruppen und Indexbereich.
    bool skalar = false, feld = false;
    Zugriff skalar_z = Zugriff::FEHLT, feld_z = Zugriff::FEHLT;  // VF/VI/B
    int64_t lo = INT64_MAX, hi = 0;
    for (size_t j = 0; j < n; ++j) {
        const Eintrag& x = abo.eintraege[j];
        if (!passt(x)) continue;
        switch (x.aktiv.z) {
            case Zugriff::I: case Zugriff::F: case Zugriff::D:
                skalar = true; skalar_z = x.aktiv.z; break;
            case Zugriff::VF: case Zugriff::ELEM_VF: feld = true; feld_z = Zugriff::VF; break;
            case Zugriff::VI: case Zugriff::ELEM_VI: feld = true; feld_z = Zugriff::VI; break;
            case Zugriff::B:  case Zugriff::ELEM_B:  feld = true; feld_z = Zugriff::B;  break;
            case Zugriff::FEHLT: break;
        }
        const bool ganz = x.aktiv.z == Zugriff::VF || x.aktiv.z == Zugriff::VI || x.aktiv.z == Zugriff::B;
        const bool elem = x.aktiv.z == Zugriff::ELEM_VF || x.aktiv.z == Zugriff::ELEM_VI || x.aktiv.z == Zugriff::ELEM_B;
        if (ganz) { lo = 0 < lo ? 0 : lo; hi = x.aktiv.laenge > hi ? x.aktiv.laenge : hi; }
        if (elem) { lo = x.index < lo ? x.index : lo; hi = x.index + 1 > hi ? x.index + 1 : hi; }
    }
    Langsam* l = langsam_finde(h);
    if (l == nullptr) return true;  // kann nicht sein (Aufrufer hat ihn gefunden)
    bool lies_skalar = skalar && !feld;
    if (skalar && feld) {
        lies_skalar = !l->skalar_zuletzt;
        l->skalar_zuletzt = lies_skalar;
    }
    // Fenster für die Array-Gruppe.
    const int64_t kap = feld_z == Zugriff::B ? grenzen::MAX_BYTES : grenzen::MAX_ARRAY_ELEMENTE;
    int64_t ws = lo, we = hi;
    if (feld && !lies_skalar && hi - lo > kap) {
        // Kleinster gebrauchter Anfang ≥ fenster, sonst von vorn.
        int64_t start = INT64_MAX;
        for (size_t j = 0; j < n; ++j) {
            const Eintrag& x = abo.eintraege[j];
            if (!passt(x)) continue;
            const bool elem = x.aktiv.z == Zugriff::ELEM_VF || x.aktiv.z == Zugriff::ELEM_VI || x.aktiv.z == Zugriff::ELEM_B;
            const int64_t a = elem ? x.index : 0;
            if (a >= l->fenster && a < start) start = a;
        }
        ws = start == INT64_MAX ? lo : start;
        we = ws + kap < hi ? ws + kap : hi;
        l->fenster = static_cast<int32_t>(we >= hi ? 0 : we);
    }

    if (!darf_arbeiten(true)) return false;
    const Eintrag& e0 = abo.eintraege[k];
    int ivalue = 0;
    float fvalue = 0.0f;
    double dvalue = 0.0;
    int gelesen = 0;
    if (lies_skalar) {
        if (skalar_z == Zugriff::I)      ivalue = quelle_.lese_i(h);
        else if (skalar_z == Zugriff::F) fvalue = quelle_.lese_f(h);
        else                             dvalue = quelle_.lese_d(h);
    } else {
        const int ab = static_cast<int>(ws);
        const int anzahl = static_cast<int>(we - ws);
        if (feld_z == Zugriff::VF)      gelesen = begrenze(quelle_.lese_vf(h, fwerte_, ab, anzahl), anzahl);
        else if (feld_z == Zugriff::VI) gelesen = begrenze(quelle_.lese_vi(h, iwerte_, ab, anzahl), anzahl);
        else                            gelesen = begrenze(quelle_.lese_b(h, bytes_, ab, anzahl), anzahl);
    }
    nach_fremdaufruf(abo, abo.eintraege[k], h);
    (void)e0;

    // Verteilen.
    for (size_t j = 0; j < n; ++j) {
        Eintrag& x = abo.eintraege[j];
        if (!passt(x)) continue;
        const Zugriff z = x.aktiv.z;
        const bool ist_skalar = z == Zugriff::I || z == Zugriff::F || z == Zugriff::D;
        if (ist_skalar != lies_skalar) continue;
        JsonSchreiber w = abo.stapel.schreiber();
        w.zeichen('[');
        w.ganzzahl(static_cast<int64_t>(j));
        w.zeichen(',');
        bool ok = true;
        if (ist_skalar) {
            if (z == Zugriff::I)      w.ganzzahl(ivalue);
            else if (z == Zugriff::F) w.zahl_f(fvalue);
            else                      w.zahl_d(dvalue);
        } else if (z == Zugriff::VF || z == Zugriff::VI || z == Zugriff::B) {
            if (ws != 0) continue;  // ganzes Array nicht in diesem Fenster
            const int m = begrenze(gelesen, x.aktiv.laenge);
            if (z == Zugriff::B) {
                w.text_bis_nul(reinterpret_cast<const char*>(bytes_), static_cast<size_t>(m));
            } else {
                w.zeichen('[');
                for (int i = 0; i < m; ++i) {
                    if (i > 0) w.zeichen(',');
                    if (z == Zugriff::VF) w.zahl_f(fwerte_[i]); else w.ganzzahl(iwerte_[i]);
                }
                w.zeichen(']');
            }
        } else {
            if (x.index < ws || x.index >= we) continue;  // anderes Fenster
            const int64_t off = x.index - ws;
            if (off >= gelesen) ok = false;  // Array inzwischen kürzer
            else if (z == Zugriff::ELEM_VF) w.zahl_f(fwerte_[off]);
            else if (z == Zugriff::ELEM_VI) w.ganzzahl(iwerte_[off]);
            else                            w.ganzzahl(bytes_[off]);
        }
        w.zeichen(']');
        x.bedient_runde = abo.seq;  // auch ohne Wert: in dieser Runde erledigt
        if (ok && !w.ueberlauf()) abo.stapel.uebernehme(w);
    }
    // Nächster Termin (die Hysterese in nach_fremdaufruf kann den Platz
    // gerade geräumt haben — dann ist er wieder ungedrosselt).
    Langsam* l2 = langsam_finde(h);
    if (l2 != nullptr && l2->gedrosselt) {
        l2->faellig = jetzt_ + grenzen::LANGSAM_INTERVALL_S;
    }
    naechster_langsamer_ = jetzt_ + grenzen::LANGSAM_ABSTAND_S;
    langsam_vorrang_ = nullptr;  // wer als nächster wartet, bestimmt der nächste Frame
    return true;
}

bool Dienst::sende_schritt(Abo& abo) noexcept {
    while (abo.paket_cursor < abo.teile) {
        if (!darf_arbeiten()) return false;
        const size_t len = schreibe_paket(paket_, sizeof(paket_),
                                          abo.praefix, abo.praefix_laenge, abo.feld,
                                          abo.stapel, abo.grenzen.daten(),
                                          abo.paket_cursor, abo.teile);
        if (len == 0) {
            // Planung und Schreiben sind aus einer Formel — das darf nicht
            // passieren. Falls doch: Paket auslassen statt hängen bleiben.
            ++abo.paket_cursor;
            continue;
        }
        const SendeErgebnis r = sende_paket(paket_, len);
        nach_aufruf();
        if (r == SendeErgebnis::VOLL) return false;
        ++abo.paket_cursor;
        einheit_fertig();
    }
    return true;
}

void Dienst::abo_verwerfen_mit_fehler(Abo& abo, Fehlergrund grund) noexcept {
    const uint32_t id = abo.id;
    const uint32_t gen = abo.generation;
    protokolliere("Protokoll 2: Abo %u verworfen (%s)", static_cast<unsigned>(id),
                  fehlergrund_text(grund));
    abo_leeren(abo);
    fehler_vormerken(grund, id, gen, -1);
}

void Dienst::bearbeite_abo_frame(Abo& abo) noexcept {
    if (abo.phase == Phase::LEER) return;

    // 1. Fertigen Prüfdurchlauf (gesucht in suchen_verteilen) zwischen zwei
    //    Runden übernehmen.
    const bool zwischen_runden = abo.phase == Phase::NEU || abo.phase == Phase::BEREIT;
    if (abo.pruefung_fertig && zwischen_runden) {
        abo.pruefung_fertig = false;
        abo.pausiert = false;
        bool geaendert = false;
        const Fehlergrund f = uebernehme_pruefung(abo, &geaendert);
        if (f != Fehlergrund::KEINER) {
            abo_verwerfen_mit_fehler(abo, f);
            return;
        }
        if (geaendert || abo.erste_antwort || abo.antwort_erneut || abo.status_sofort) {
            abo.erste_antwort = false;
            abo.antwort_erneut = false;
            abo.status_sofort = false;
            if (!bereite_antwort(abo)) {
                abo_verwerfen_mit_fehler(abo, Fehlergrund::SPEICHER);
                return;
            }
        }
        uhr_auffrischen();
    } else if ((abo.antwort_erneut || abo.status_sofort) && abo.phase == Phase::BEREIT) {
        // Status noch einmal (identisches ABO) oder sofort neu (beim Lesen
        // verwaist gefunden) — ohne auf den nächsten Durchlauf zu warten.
        abo.antwort_erneut = false;
        abo.status_sofort = false;
        if (!bereite_antwort(abo)) {
            abo_verwerfen_mit_fehler(abo, Fehlergrund::SPEICHER);
            return;
        }
        uhr_auffrischen();
    }

    // 2. Zustand weiterschalten. Die Fälle fallen absichtlich durch: ein
    //    kleines Abo erledigt Lesen und Senden im selben Frame.
    switch (abo.phase) {
        case Phase::LEER:
        case Phase::NEU:
            return;
        case Phase::ANTWORT:
            if (!sende_schritt(abo)) return;
            abo.phase = Phase::BEREIT;
            abo.faellig = jetzt_;  // Werte sofort, nicht erst nach 1/rate
            [[fallthrough]];
        case Phase::BEREIT:
            if (abo.pausiert) {
                if (abo.pause_bis < 0.0) abo.pause_bis = jetzt_ + grenzen::MAX_PAUSE_S;
                if (jetzt_ < abo.pause_bis) return;
                abo.pausiert = false;  // Grenze erreicht, Lesen prüft weiter auf Waisen
            }
            if (jetzt_ < abo.faellig) return;
            starte_runde(abo);
            // Nächster Termin vom Soll aus, damit die Rate im Mittel stimmt;
            // hinkt der Sim hinterher, kein Nachholen im Stoß.
            abo.faellig += abo.periode;
            if (abo.faellig <= jetzt_) abo.faellig = jetzt_ + abo.periode;
            [[fallthrough]];
        case Phase::LESEN: {
            if (!lese_schritt(abo)) return;
            JsonSchreiber p(abo.praefix, sizeof(abo.praefix));
            p.roh("{\"p\":2,\"t\":\"w\",\"abo\":");
            p.ganzzahl(abo.id);
            p.roh(",\"gen\":");
            p.ganzzahl(abo.generation);
            p.roh(",\"seq\":");
            p.ganzzahl(abo.seq);
            abo.praefix_laenge = p.laenge();
            abo.feld = "v";
            abo.teile = plane_pakete(abo.praefix_laenge, 1, abo.stapel,
                                     abo.grenzen.daten(), abo.grenzen.kapazitaet(),
                                     grenzen::MAX_PAKET);
            if (abo.teile == 0) {
                abo_verwerfen_mit_fehler(abo, Fehlergrund::SPEICHER);
                return;
            }
            abo.paket_cursor = 0;
            abo.phase = Phase::SENDEN;
            uhr_auffrischen();
            [[fallthrough]];
        }
        case Phase::SENDEN:
            if (!sende_schritt(abo)) return;
            abo.phase = Phase::BEREIT;
            return;
    }
}

// =============================================================================
// LISTE
// =============================================================================
//
// Budget je NAME (H4): ein Name = ein XPLMGetDataRefInfo. Die 256 Handles
// eines Blocks bleiben über Frames hinweg in handles_ (Datarefs werden nie
// zerstört, die Handles bleiben gültig); vorher kam nach EINER Budgetprüfung
// ein ganzer Block. Speicher mit eigenem Budget (grenzen::MAX_BYTES_LISTE).

void Dienst::liste_schritt() noexcept {
    if (!liste_.aktiv) return;

    auto aufgeben = [this](Fehlergrund grund) noexcept {
        const uint32_t id = liste_.id;
        protokolliere("Protokoll 2: LISTE %u abgebrochen (%s)", static_cast<unsigned>(id),
                      fehlergrund_text(grund));
        liste_leeren();
        fehler_vormerken(grund, 0, 0, id);
    };

    if (liste_.sammeln) {
        if (liste_.gesamt < 0) {
            if (!darf_arbeiten()) return;
            int gesamt = quelle_.anzahl_datarefs();
            nach_aufruf();
            if (gesamt < 0) gesamt = 0;
            if (gesamt > grenzen::MAX_LISTE_DATAREFS) gesamt = grenzen::MAX_LISTE_DATAREFS;
            liste_.gesamt = gesamt;
        }
        for (;;) {
            if (liste_.block_pos >= liste_.block_anzahl) {
                if (liste_.cursor >= liste_.gesamt) break;
                if (!darf_arbeiten()) return;
                int block = liste_.gesamt - liste_.cursor;
                if (block > grenzen::LISTE_BLOCK) block = grenzen::LISTE_BLOCK;
                const int geholt = begrenze(quelle_.datarefs_ab(liste_.cursor, block, handles_), block);
                nach_aufruf();
                if (geholt == 0) {
                    // Weniger Datarefs als gemeldet — mit dem Bestand weitermachen.
                    liste_.gesamt = liste_.cursor;
                    break;
                }
                liste_.cursor += geholt;
                liste_.block_pos = 0;
                liste_.block_anzahl = geholt;
            }
            if (!darf_arbeiten()) return;
            const DatarefHandle h = handles_[liste_.block_pos];
            const char* name = h ? quelle_.name_von(h) : nullptr;
            nach_aufruf();
            ++liste_.block_pos;
            einheit_fertig();
            const size_t n = name ? text_laenge(name, grenzen::MAX_ZEILE + 1) : 0;
            // Nur abonnierbare Namen melden: was nicht durch den
            // ABO-Parser käme, nützt dem Client nichts.
            if (!ist_abonnierbarer_name(name, n)) {
                ++liste_.ausgelassen;
                continue;
            }
            const size_t budget = grenzen::MAX_BYTES_LISTE -
                                  liste_.grenzen.kapazitaet() * sizeof(uint32_t);
            const ElementListe::Platz platz = liste_.namen.sorge_fuer_platz(json_max_text(n), budget);
            if (platz != ElementListe::Platz::OK) {
                aufgeben(platz == ElementListe::Platz::LIMIT ? Fehlergrund::SPEICHER_LIMIT
                                                             : Fehlergrund::SPEICHER);
                return;
            }
            JsonSchreiber w = liste_.namen.schreiber();
            w.text(name, n);
            liste_.namen.uebernehme(w);
        }
        // Gesammelt: planen.
        const size_t anzahl = liste_.namen.anzahl();
        JsonSchreiber p(liste_.praefix, sizeof(liste_.praefix));
        p.roh("{\"p\":2,\"t\":\"liste\",\"id\":");
        p.ganzzahl(liste_.id);
        liste_.praefix_laenge = p.laenge();
        if (liste_.namen.belegt_bytes() + (anzahl + 2) * sizeof(uint32_t) > grenzen::MAX_BYTES_LISTE) {
            aufgeben(Fehlergrund::SPEICHER_LIMIT);
            return;
        }
        if (!liste_.grenzen.reserviere(anzahl + 2)) {
            aufgeben(Fehlergrund::SPEICHER);
            return;
        }
        liste_.teile = plane_pakete(liste_.praefix_laenge, 1, liste_.namen,
                                    liste_.grenzen.daten(), liste_.grenzen.kapazitaet(),
                                    grenzen::MAX_PAKET);
        if (liste_.teile == 0) {
            aufgeben(Fehlergrund::SPEICHER);
            return;
        }
        protokolliere("Protokoll 2: LISTE %u - %u Namen in %u Paketen (%u nicht abonnierbar ausgelassen)",
                      static_cast<unsigned>(liste_.id), static_cast<unsigned>(anzahl),
                      static_cast<unsigned>(liste_.teile),
                      static_cast<unsigned>(liste_.ausgelassen));
        liste_.sammeln = false;
        liste_.paket_cursor = 0;
        uhr_auffrischen();
    }

    while (liste_.paket_cursor < liste_.teile) {
        if (!darf_arbeiten()) return;
        const size_t len = schreibe_paket(paket_, sizeof(paket_),
                                          liste_.praefix, liste_.praefix_laenge, "n",
                                          liste_.namen, liste_.grenzen.daten(),
                                          liste_.paket_cursor, liste_.teile);
        if (len != 0) {
            const SendeErgebnis r = sende_paket(paket_, len);
            nach_aufruf();
            if (r == SendeErgebnis::VOLL) return;
        }
        ++liste_.paket_cursor;
        einheit_fertig();
    }
    liste_leeren();  // fertig: Speicher sofort zurückgeben
}

// =============================================================================
// Flugzeug
// =============================================================================

// Liest eine Kennung (Byte-Dataref von X-Plane selbst). Nach jedem
// XPLM-Aufruf die Uhr (Nachprüfung Codex H4) — die Kennung läuft unter dem
// Such-Budget und ist eine Einheit je Dataref.
int Dienst::lies_kennung(DatarefHandle* h, const char* name, char* aus, size_t kap) noexcept {
    aus[0] = '\0';
    if (*h == nullptr) {  // billig; bis er da ist
        *h = quelle_.finde(name);
        nach_aufruf();
        if (*h == nullptr) return 0;
        if (!darf_arbeiten()) return -1;  // Lesen im nächsten Frame (Handle bleibt)
    }
    const int max = static_cast<int>(kap - 1);
    // In den großzügigen Arbeitspuffer lesen, dann begrenzt kopieren.
    const int n = begrenze(quelle_.lese_b(*h, bytes_, 0, max), max);
    nach_aufruf();
    std::memcpy(aus, bytes_, static_cast<size_t>(n));
    aus[n] = '\0';
    // Zeichenkette endet am ersten NUL (text_laenge in der JSON-Ausgabe).
    return 1;
}

// Fortsetzbar über Frames: Stufe 0..3 liest ICAO, Beschreibung, Pfad und
// UI-Namen (je eine Einheit unter dem Such-Budget), Stufe 4 vergleicht und
// sendet. Vorher lief die ganze Prüfung (bis zu drei Find + drei Getter +
// Senden) vor jedem Budget.
//
// `titel` ist der Name, wie ihn X-Plane im UI zeigt (acf_ui_name, z. B.
// "ToLiSs A320 Hi Def") — denselben nimmt der Szenerie-/Flugzeug-Scan des
// Clients; acf_descrip ist bei vielen Add-ons eine Beschreibung ("A320 with
// high fidelity system modelling"), Messung und Scan passten so nie
// zusammen. Fehlt acf_ui_name (X-Plane 11) oder ist er leer, gilt wie bisher
// acf_descrip. Die Beschreibung kommt zusätzlich als `beschreibung`.
void Dienst::pruefe_flugzeug() noexcept {
    if (flugzeug_stufe_ < 0) {
        if (!flugzeug_offen_ && jetzt_ < naechste_flugzeug_pruefung_) return;
        naechste_flugzeug_pruefung_ = jetzt_ + grenzen::FLUGZEUG_PRUEFINTERVALL_S;
        flugzeug_stufe_ = 0;
    }
    while (flugzeug_stufe_ < 4) {
        if (!darf_arbeiten()) return;
        int r;
        switch (flugzeug_stufe_) {
            case 0: r = lies_kennung(&h_icao_, "sim/aircraft/view/acf_ICAO", lese_icao_, sizeof(lese_icao_)); break;
            case 1: r = lies_kennung(&h_titel_, "sim/aircraft/view/acf_descrip", lese_titel_, sizeof(lese_titel_)); break;
            case 2: r = lies_kennung(&h_pfad_, "sim/aircraft/view/acf_relative_path", lese_pfad_, sizeof(lese_pfad_)); break;
            default: r = lies_kennung(&h_ui_, "sim/aircraft/view/acf_ui_name", lese_ui_, sizeof(lese_ui_)); break;
        }
        if (r < 0) return;  // Budget zwischen finde und Lesen um
        lese_hat_[flugzeug_stufe_] = r > 0;
        ++flugzeug_stufe_;
        einheit_fertig();
    }

    const bool* hat = lese_hat_;
    const bool gleich = gesendet_gueltig_ &&
                        hat[0] == gesendet_hat_[0] && hat[1] == gesendet_hat_[1] &&
                        hat[2] == gesendet_hat_[2] && hat[3] == gesendet_hat_[3] &&
                        std::strcmp(lese_icao_, gesendet_icao_) == 0 &&
                        std::strcmp(lese_titel_, gesendet_titel_) == 0 &&
                        std::strcmp(lese_pfad_, gesendet_pfad_) == 0 &&
                        std::strcmp(lese_ui_, gesendet_ui_) == 0;
    if (gleich && !flugzeug_offen_) {
        flugzeug_stufe_ = -1;
        return;
    }
    if (!darf_arbeiten()) return;  // Senden im nächsten Frame (Stufe 4 bleibt)

    // Format (Cloud-QS 29.09.2026, P3): `titel` = acf_descrip wie bis 1.0.0,
    // der UI-Name als EIGENES Feld `ui_name` (ohne Leerraum am Rand; leer
    // oder ohne Dataref: kein Feld). Vorher trug `titel` den UI-Namen und
    // `beschreibung` die Beschreibung — ein älterer Client oder eine leere
    // Beschreibung ließ den Titel still auf den UI-Namen kippen, und der
    // Titel steuert Profilerkennung und Buchungsabgleich.
    auto rand = [](const char* t, size_t max, size_t* anfang) noexcept {
        auto leer = [](char c) { return c == ' ' || c == '\t' || c == '\r' || c == '\n'; };
        size_t n = 0;
        while (n < max && t[n] != '\0') ++n;
        size_t a = 0;
        while (a < n && leer(t[a])) ++a;
        while (n > a && leer(t[n - 1])) --n;
        *anfang = a;
        return n - a;
    };
    size_t ui_anfang = 0;
    const size_t ui_laenge = hat[3] ? rand(lese_ui_, sizeof(lese_ui_), &ui_anfang) : 0;

    JsonSchreiber w(paket_, sizeof(paket_));
    auto feld = [&w](const char* schluessel, bool vorhanden, const char* wert, size_t max) noexcept {
        w.roh(schluessel);
        if (vorhanden) w.text_bis_nul(wert, max); else w.null();
    };
    for (int versuch = 0; versuch < 2; ++versuch) {
        w.zurueck_zu(0);
        w.roh("{\"p\":2,\"t\":\"flugzeug\"");
        feld(",\"icao\":", hat[0], lese_icao_, sizeof(lese_icao_));
        feld(",\"titel\":", hat[1], lese_titel_, sizeof(lese_titel_));
        if (ui_laenge > 0) {
            w.roh(",\"ui_name\":");
            w.text(lese_ui_ + ui_anfang, ui_laenge);
        }
        // Im (theoretischen) Fall, dass alles maskiert nicht in 8 KiB passt,
        // lieber den Pfad weglassen als die Meldung.
        feld(",\"pfad\":", hat[2] && versuch == 0, lese_pfad_, sizeof(lese_pfad_));
        w.roh("}\n");
        if (!w.ueberlauf()) break;
    }
    flugzeug_stufe_ = -1;
    if (w.ueberlauf()) return;
    // Über den gemeinsamen Ausgang (M1). Kein Platz mehr in diesem Frame oder
    // Socket voll: im nächsten Frame neu lesen und senden (die Meldung bleibt
    // offen).
    const SendeErgebnis r = sende_paket(w.daten(), w.laenge());
    nach_aufruf();
    einheit_fertig();
    if (r == SendeErgebnis::VOLL) {
        flugzeug_offen_ = true;
        return;
    }
    std::memcpy(gesendet_icao_, lese_icao_, sizeof(lese_icao_));
    std::memcpy(gesendet_titel_, lese_titel_, sizeof(lese_titel_));
    std::memcpy(gesendet_pfad_, lese_pfad_, sizeof(lese_pfad_));
    std::memcpy(gesendet_ui_, lese_ui_, sizeof(lese_ui_));
    for (int i = 0; i < 4; ++i) gesendet_hat_[i] = hat[i];
    gesendet_gueltig_ = true;
    flugzeug_offen_ = false;
}

}  // namespace aeroacars
