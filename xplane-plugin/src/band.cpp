// =============================================================================
// AeroACARS X-Plane-Plugin — HUD-Band (ADR-0005), XPLM-freier Kern
// =============================================================================
//
// Siehe band.h. Grundsätze wie im Anfrage-Parser: nur (zeiger, länge)-Paare
// innerhalb des Datagramms, kein strlen auf Fremddaten, keine Locale (keine
// isdigit/strtol/printf mit Gleitkomma), keine Allokation, keine Ausnahmen.
// =============================================================================

#include "band.h"

#include <cstring>

namespace aeroacars {

const char BAND_TEXT_NICHT_ERREICHBAR[] = "AeroACARS nicht erreichbar - laeuft die App?";

namespace {

// 0x20–0x7E und TAB.
bool ist_bandzeichen(unsigned char c) noexcept {
    return (c >= 0x20 && c <= 0x7E) || c == '\t';
}

// Eine Zeile prüfen und in Läufe zerlegen. Leer = keine Läufe (gültig).
bool zerlege_zeile(const char* p, size_t n, BandZeile* z) noexcept {
    z->laeufe_n = 0;
    z->text_n = 0;
    if (n == 0) return true;
    if (n > grenzen::MAX_ZEILE) return false;
    for (size_t i = 0; i < n; ++i) {
        if (!ist_bandzeichen(static_cast<unsigned char>(p[i]))) return false;
    }
    z->text_n = static_cast<uint16_t>(n);
    std::memcpy(z->text, p, n);

    size_t i = 0;
    for (;;) {
        size_t j = i;
        while (j < n && p[j] != '\t') ++j;
        if (j - i < 2) return false;                       // Lauf ohne Farbe + Art
        if (!band_ist_farbe(p[i]) || !band_ist_art(p[i + 1])) return false;
        if (p[i + 1] == 'p' && j - i != 2) return false;   // Punkt trägt keinen Text
        if (z->laeufe_n >= grenzen::BAND_MAX_LAEUFE) return false;  // nur zur Sicherheit
        BandLauf& l = z->laeufe[z->laeufe_n++];
        l.farbe = p[i];
        l.art = p[i + 1];
        l.ofs = static_cast<uint16_t>(i + 2);
        l.laenge = static_cast<uint16_t>(j - i - 2);
        if (j == n) break;
        i = j + 1;                                         // hinter dem TAB
        if (i == n) return false;                          // leerer Schlusslauf
    }
    return true;
}

}  // namespace

bool band_ist_farbe(char c) noexcept {
    return c == 'n' || c == 'd' || c == 'g' || c == 'w' || c == 'b' || c == 'a';
}

bool band_ist_art(char c) noexcept {
    return c == 't' || c == 'm' || c == 'p' || c == 'i' || c == 's' || c == 'l' || c == 'v' ||
           c == 'x' || c == 'z' || c == 'e';
}

bool band_zerlege_koerper(uint32_t zeilen, bool ruhig, const char* rest, size_t laenge,
                          BandBild* aus, uint32_t* fehler_zeile) noexcept {
    auto fehler = [fehler_zeile](uint32_t z) noexcept {
        if (fehler_zeile != nullptr) *fehler_zeile = z;
        return false;
    };
    if (zeilen > grenzen::BAND_MAX_ZEILEN) return fehler(1);
    aus->ruhig = ruhig;
    aus->zeilen = 0;

    size_t pos = 0;
    uint32_t gefunden = 0;
    while (pos < laenge) {
        const char* anfang = rest + pos;
        const size_t links = laenge - pos;
        const void* nl = std::memchr(anfang, '\n', links);
        const size_t n = nl != nullptr ? static_cast<size_t>(static_cast<const char*>(nl) - anfang)
                                       : links;
        pos += (nl != nullptr) ? n + 1 : n;
        if (gefunden >= zeilen) return fehler(gefunden + 2);   // eine Zeile zu viel
        if (!zerlege_zeile(anfang, n, &aus->zeile[gefunden])) return fehler(gefunden + 2);
        ++gefunden;
    }
    if (gefunden != zeilen) return fehler(gefunden + 2);        // Zeilen fehlen
    aus->zeilen = static_cast<uint8_t>(gefunden);
    return true;
}

// ---- Zustand -----------------------------------------------------------------

Band::Band(double jetzt) noexcept : letzte_zeit_(jetzt) {}

void Band::neuer_client() noexcept {
    hat_seq_ = false;
    letzte_seq_ = 0;
}

BandErgebnis Band::uebernehme(uint32_t seq, bool ruhig, uint32_t zeilen, const char* rest,
                              size_t laenge, double jetzt, uint32_t* fehler_zeile) noexcept {
    BandBild& ziel = bilder_[1 - aktuell_];
    if (!band_zerlege_koerper(zeilen, ruhig, rest, laenge, &ziel, fehler_zeile)) {
        return BandErgebnis::UNGUELTIG;  // bisheriges Band bleibt
    }
    if (hat_seq_) {
        const bool neu = seq > letzte_seq_ ||
                         (letzte_seq_ - seq) > grenzen::BAND_SEQ_WRAP;
        if (!neu) return BandErgebnis::VERALTET;
    }
    hat_seq_ = true;
    letzte_seq_ = seq;
    letzte_zeit_ = jetzt;
    ersatz_gueltig_ = false;
    if (zeilen == 0) {
        ausgeblendet_ = true;   // das alte Bild bleibt liegen, wird aber nicht gezeigt
        return BandErgebnis::ANGENOMMEN;
    }
    ausgeblendet_ = false;
    hat_band_ = true;
    aktuell_ = 1 - aktuell_;
    return BandErgebnis::ANGENOMMEN;
}

bool Band::veraltet(double jetzt) const noexcept {
    return jetzt - letzte_zeit_ > grenzen::BAND_FRIST_S;
}

const BandBild* Band::ansicht(double jetzt) noexcept {
    if (!hat_band_ || ausgeblendet_) return nullptr;
    if (!veraltet(jetzt)) return &bilder_[aktuell_];

    if (!ersatz_gueltig_) {
        const BandBild& alt = bilder_[aktuell_];
        ersatz_ = alt;
        ersatz_.ruhig = false;   // die Warnung soll auffallen
        BandZeile& z = ersatz_.zeile[0];
        const char* text = BAND_TEXT_NICHT_ERREICHBAR;
        const size_t tn = std::strlen(text);
        // Zeile 1: "bp" TAB "ns<text>" — roter Punkt + Lage-Text.
        size_t k = 0;
        z.text[k++] = 'b';
        z.text[k++] = 'p';
        z.text[k++] = '\t';
        z.text[k++] = 'n';
        z.text[k++] = 's';
        std::memcpy(z.text + k, text, tn);
        k += tn;
        z.text_n = static_cast<uint16_t>(k);
        z.laeufe_n = 2;
        z.laeufe[0].farbe = 'b';
        z.laeufe[0].art = 'p';
        z.laeufe[0].ofs = 2;
        z.laeufe[0].laenge = 0;
        z.laeufe[1].farbe = 'n';
        z.laeufe[1].art = 's';
        z.laeufe[1].ofs = 5;
        z.laeufe[1].laenge = static_cast<uint16_t>(tn);
        ersatz_gueltig_ = true;
    }
    return &ersatz_;
}

// ---- Größe, Dimmen, Farben -----------------------------------------------------

namespace {
constexpr float STUFEN[BAND_STUFEN_N] = {0.6f, 0.8f, 1.0f, 1.25f, 1.5f, 2.0f, 3.0f};
}

int band_stufe_klemmen(int stufe) noexcept {
    if (stufe < 0) return 0;
    if (stufe >= BAND_STUFEN_N) return BAND_STUFEN_N - 1;
    return stufe;
}
float band_skalierung(int stufe) noexcept { return STUFEN[band_stufe_klemmen(stufe)]; }
int band_stufe_groesser(int stufe) noexcept { return band_stufe_klemmen(stufe + 1); }
int band_stufe_kleiner(int stufe) noexcept { return band_stufe_klemmen(stufe - 1); }

float band_dimm_faktor(bool ruhig, bool maus_darueber) noexcept {
    return (ruhig && !maus_darueber) ? 0.5f : 1.0f;
}

bool band_farbe(char farbe, float faktor, float rgb[3]) noexcept {
    float f[3];
    bool bekannt = true;
    switch (farbe) {
        // panel.css: ink #f2f5fa, muted #aeb8c9, good #3ecf6a, warn #f0a83f,
        // bad #ef5464, accent #4c8dff.
        case 'n': f[0] = 0xf2 / 255.0f; f[1] = 0xf5 / 255.0f; f[2] = 0xfa / 255.0f; break;
        case 'd': f[0] = 0xae / 255.0f; f[1] = 0xb8 / 255.0f; f[2] = 0xc9 / 255.0f; break;
        case 'g': f[0] = 0x3e / 255.0f; f[1] = 0xcf / 255.0f; f[2] = 0x6a / 255.0f; break;
        case 'w': f[0] = 0xf0 / 255.0f; f[1] = 0xa8 / 255.0f; f[2] = 0x3f / 255.0f; break;
        case 'b': f[0] = 0xef / 255.0f; f[1] = 0x54 / 255.0f; f[2] = 0x64 / 255.0f; break;
        case 'a': f[0] = 0x4c / 255.0f; f[1] = 0x8d / 255.0f; f[2] = 0xff / 255.0f; break;
        default:  f[0] = 0xf2 / 255.0f; f[1] = 0xf5 / 255.0f; f[2] = 0xfa / 255.0f; bekannt = false; break;
    }
    for (int i = 0; i < 3; ++i) rgb[i] = f[i] * faktor;
    return bekannt;
}

// ---- Position und Merken ---------------------------------------------------------

void band_klemme_position(const BandRechteck& schirm, int breite, int hoehe,
                          int* links, int* oben) noexcept {
    int l = *links;
    int o = *oben;
    if (l + breite > schirm.rechts) l = schirm.rechts - breite;
    if (l < schirm.links) l = schirm.links;
    if (o > schirm.oben) o = schirm.oben;
    if (o - hoehe < schirm.unten) o = schirm.unten + hoehe;
    if (o > schirm.oben) o = schirm.oben;   // höher als der Schirm: obere Kante
    *links = l;
    *oben = o;
}

void band_standard_position(const BandRechteck& schirm, int breite, int hoehe,
                            int* links, int* oben) noexcept {
    int l = schirm.links + ((schirm.rechts - schirm.links) - breite) / 2;
    int o = schirm.oben - 12;
    band_klemme_position(schirm, breite, hoehe, &l, &o);
    *links = l;
    *oben = o;
}

void band_vor_dem_zeigen(const BandRechteck& schirm, const BandRechteck& fenster, bool platziert,
                         int* links, int* oben) noexcept {
    const int breite = fenster.rechts - fenster.links;
    const int hoehe = fenster.oben - fenster.unten;
    int l = fenster.links;
    int o = fenster.oben;
    if (platziert) {
        band_klemme_position(schirm, breite, hoehe, &l, &o);
    } else {
        band_standard_position(schirm, breite, hoehe, &l, &o);
    }
    *links = l;
    *oben = o;
}

namespace {

size_t schreibe_zahl(char* aus, size_t kap, size_t pos, long wert) noexcept {
    char z[24];
    size_t n = 0;
    const bool neg = wert < 0;
    unsigned long u = neg ? static_cast<unsigned long>(-(wert + 1)) + 1ul : static_cast<unsigned long>(wert);
    do { z[n++] = static_cast<char>('0' + u % 10ul); u /= 10ul; } while (u != 0 && n < sizeof(z));
    if (neg) z[n++] = '-';
    if (pos + n > kap) return 0;
    for (size_t i = 0; i < n; ++i) aus[pos + i] = z[n - 1 - i];
    return n;
}

struct Schreiber {
    char* aus;
    size_t kap;
    size_t pos = 0;
    bool ok = true;
    void text(const char* t) noexcept {
        const size_t n = std::strlen(t);
        if (!ok || pos + n > kap) { ok = false; return; }
        std::memcpy(aus + pos, t, n);
        pos += n;
    }
    void zahl(long w) noexcept {
        if (!ok) return;
        const size_t n = schreibe_zahl(aus, kap, pos, w);
        if (n == 0) { ok = false; return; }
        pos += n;
    }
};

// Ganzzahl aus [p, p+n): optionales '-', 1 … 9 Ziffern.
bool lies_ganzzahl(const char* p, size_t n, long* out) noexcept {
    if (n == 0) return false;
    size_t i = 0;
    bool neg = false;
    if (p[0] == '-') { neg = true; i = 1; }
    if (n - i == 0 || n - i > 9) return false;
    long w = 0;
    for (; i < n; ++i) {
        if (p[i] < '0' || p[i] > '9') return false;
        w = w * 10 + (p[i] - '0');
    }
    *out = neg ? -w : w;
    return true;
}

}  // namespace

size_t band_prefs_schreibe(const BandPrefs& p, char* aus, size_t kapazitaet) noexcept {
    Schreiber s{aus, kapazitaet};
    s.text("an=");
    s.text(p.an ? "1" : "0");
    s.text("\nstufe=");
    s.zahl(band_stufe_klemmen(p.stufe));
    s.text("\n");
    if (p.pos_gesetzt) {
        s.text("x=");
        s.zahl(p.links);
        s.text("\ny=");
        s.zahl(p.oben);
        s.text("\n");
    }
    return s.ok ? s.pos : 0;
}

BandPrefs band_prefs_lese(const char* text, size_t laenge) noexcept {
    BandPrefs r;
    if (text == nullptr) return r;
    if (laenge > grenzen::BAND_PREFS_MAX) return r;
    bool x_da = false, y_da = false;
    long x = 0, y = 0;
    size_t pos = 0;
    while (pos < laenge) {
        const char* z = text + pos;
        const void* nl = std::memchr(z, '\n', laenge - pos);
        size_t n = nl != nullptr ? static_cast<size_t>(static_cast<const char*>(nl) - z) : laenge - pos;
        pos += (nl != nullptr) ? n + 1 : n;
        if (n > 0 && z[n - 1] == '\r') --n;
        if (n == 0 || z[0] == '#') continue;
        const void* gl = std::memchr(z, '=', n);
        if (gl == nullptr) continue;
        const size_t kn = static_cast<size_t>(static_cast<const char*>(gl) - z);
        const char* w = z + kn + 1;
        const size_t wn = n - kn - 1;
        auto schluessel = [&](const char* k) noexcept {
            return std::strlen(k) == kn && std::memcmp(z, k, kn) == 0;
        };
        long v = 0;
        if (schluessel("an")) {
            if (wn == 1 && (w[0] == '0' || w[0] == '1')) r.an = (w[0] == '1');
        } else if (schluessel("stufe")) {
            if (lies_ganzzahl(w, wn, &v)) r.stufe = band_stufe_klemmen(static_cast<int>(v));
        } else if (schluessel("x")) {
            if (lies_ganzzahl(w, wn, &v)) { x = v; x_da = true; }
        } else if (schluessel("y")) {
            if (lies_ganzzahl(w, wn, &v)) { y = v; y_da = true; }
        }
    }
    if (x_da && y_da) {
        r.pos_gesetzt = true;
        r.links = static_cast<int>(x);
        r.oben = static_cast<int>(y);
    }
    return r;
}

}  // namespace aeroacars
