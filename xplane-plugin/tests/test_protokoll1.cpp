// Einheitstests: Protokoll-1-Pakete (protokoll1.cpp, Codex-Abnahme M3).
//
// Referenz ist das snprintf-Format aus plugin.cpp bis 1.0.0 — hier WÖRTLICH
// kopiert. Für endliche Werte in der C-Locale muss die neue Ausgabe Byte für
// Byte gleich sein (alte Clients parsen per serde; Rust-Tests des Clients
// hängen an diesem Format). Zusätzlich: gültiges JSON auch bei NaN/±Inf und
// unter einer Locale mit Dezimalkomma.

#include "json_pruefer.h"
#include "protokoll1.h"
#include "testrahmen.h"

#include <clocale>
#include <cmath>
#include <cstdio>
#include <limits>
#include <string>

using namespace aeroacars;

namespace {

constexpr const char* PV = "1.0.0";

// ---- Referenz: das alte Format (plugin.cpp 0.5.13 – 1.0.0) -------------------

std::string alt_telemetrie(const Telemetrie& t) {
    char buf[2048];
    int n = std::snprintf(buf, sizeof(buf),
        "{"
        "\"v\":1,"
        "\"pv\":\"" "1.0.0" "\","
        "\"type\":\"telemetry\","
        "\"seq\":%u,"
        "\"ts\":%.6f,"
        "\"lat\":%.7f,"
        "\"lon\":%.7f,"
        "\"agl_ft\":%.2f,"
        "\"vs_fpm_raw\":%.2f,"
        "\"vs_fpm\":%.2f,"
        "\"fnrml_gear_n\":%.2f,"
        "\"on_ground\":%s,"
        "\"g_normal\":%.4f,"
        "\"pitch_deg\":%.3f,"
        "\"bank_deg\":%.3f,"
        "\"hdg_true\":%.3f,"
        "\"ias_kt\":%.2f,"
        "\"gs_kt\":%.2f"
        "}\n",
        t.seq,
        t.ts,
        t.lat, t.lon,
        static_cast<double>(t.agl_ft),
        static_cast<double>(t.vs_fpm_raw),
        static_cast<double>(t.vs_fpm),
        static_cast<double>(t.fnrml_gear_n),
        t.on_ground ? "true" : "false",
        static_cast<double>(t.g_normal),
        static_cast<double>(t.pitch_deg),
        static_cast<double>(t.bank_deg),
        static_cast<double>(t.hdg_true),
        static_cast<double>(t.ias_kt),
        static_cast<double>(t.gs_kt));
    return n > 0 ? std::string(buf, static_cast<size_t>(n)) : std::string();
}

std::string alt_aufsetzen(const Aufsetzen& a) {
    char buf[2048];
    int n = std::snprintf(buf, sizeof(buf),
        "{"
        "\"v\":1,"
        "\"pv\":\"" "1.0.0" "\","
        "\"type\":\"touchdown\","
        "\"seq\":%u,"
        "\"ts\":%.6f,"
        "\"lat\":%.7f,"
        "\"lon\":%.7f,"
        "\"captured_vs_fpm\":%.2f,"
        "\"captured_vs_source\":\"%s\","
        "\"captured_vs_window_ms\":%d,"
        "\"captured_vs_samples\":%d,"
        "\"captured_g_normal\":%.4f,"
        "\"captured_pitch_deg\":%.3f,"
        "\"captured_bank_deg\":%.3f,"
        "\"captured_ias_kt\":%.2f,"
        "\"captured_gs_kt\":%.2f,"
        "\"captured_heading_deg\":%.3f,"
        "\"fnrml_gear_n\":%.2f,"
        "\"agl_ft\":%.2f"
        "}\n",
        a.seq,
        a.ts,
        a.lat, a.lon,
        static_cast<double>(a.captured_vs_fpm),
        a.captured_vs_source,
        a.captured_vs_window_ms,
        a.captured_vs_samples,
        static_cast<double>(a.captured_g_normal),
        static_cast<double>(a.captured_pitch_deg),
        static_cast<double>(a.captured_bank_deg),
        static_cast<double>(a.captured_ias_kt),
        static_cast<double>(a.captured_gs_kt),
        static_cast<double>(a.captured_heading_deg),
        static_cast<double>(a.fnrml_gear_n),
        static_cast<double>(a.agl_ft));
    return n > 0 ? std::string(buf, static_cast<size_t>(n)) : std::string();
}

std::string neu_telemetrie(const Telemetrie& t) {
    char buf[2048];
    const size_t n = schreibe_telemetrie(buf, sizeof(buf), PV, t);
    return std::string(buf, n);
}

std::string neu_aufsetzen(const Aufsetzen& a) {
    char buf[2048];
    const size_t n = schreibe_aufsetzen(buf, sizeof(buf), PV, a);
    return std::string(buf, n);
}

Telemetrie beispiel_telemetrie(int k) {
    Telemetrie t;
    t.seq = 12345u + static_cast<uint32_t>(k);
    t.ts = 1234.567890123 + k * 0.05;
    t.lat = 50.034567890123 - k * 1e-5;
    t.lon = -8.571234567890 + k * 1e-5;
    t.agl_ft = 2150.4f - static_cast<float>(k) * 3.3f;
    t.vs_fpm_raw = -285.4f + static_cast<float>(k);
    t.vs_fpm = -285.1f + static_cast<float>(k);
    t.fnrml_gear_n = k % 2 ? 0.0f : 52312.0f;
    t.on_ground = k % 3 == 0;
    t.g_normal = 0.997f + static_cast<float>(k) * 0.0001f;
    t.pitch_deg = 3.42f;
    t.bank_deg = -0.15f * static_cast<float>(k);
    t.hdg_true = 253.117f + static_cast<float>(k) * 0.01f;
    t.ias_kt = 138.4f;
    t.gs_kt = 134.5f;
    return t;
}

Aufsetzen beispiel_aufsetzen(int k) {
    Aufsetzen a;
    a.seq = 12450u + static_cast<uint32_t>(k);
    a.ts = 1289.012345;
    a.lat = 50.0411111;
    a.lon = 8.5811111 + k;
    a.captured_vs_fpm = -285.4f - static_cast<float>(k);
    a.captured_vs_source = k % 2 ? "lua_30_sample" : "low_agl_vs_min";
    a.captured_vs_window_ms = 612 + k;
    a.captured_vs_samples = 30 - k;
    a.captured_g_normal = 1.18f;
    a.captured_pitch_deg = 3.4f;
    a.captured_bank_deg = -0.2f;
    a.captured_ias_kt = 138.0f;
    a.captured_gs_kt = 134.5f;
    a.captured_heading_deg = 253.1f;
    a.fnrml_gear_n = 52312.0f;
    a.agl_ft = 0.4f;
    return a;
}

}  // namespace

TEST(protokoll1_gleich_wie_bisher) {
    // Normale, negative, null und negative Null, große und winzige Werte.
    for (int k = 0; k < 40; ++k) {
        const Telemetrie t = beispiel_telemetrie(k);
        PRUEFE_TEXT(neu_telemetrie(t), alt_telemetrie(t));
        const Aufsetzen a = beispiel_aufsetzen(k);
        PRUEFE_TEXT(neu_aufsetzen(a), alt_aufsetzen(a));
    }
    Telemetrie t;  // alles 0
    PRUEFE_TEXT(neu_telemetrie(t), alt_telemetrie(t));
    t.vs_fpm = -0.0f;
    t.lat = -0.0;
    t.agl_ft = 1e30f;
    t.lon = -179.99999995;  // Rundung an der 7. Stelle
    t.ts = 1e7 + 0.0000005;
    t.seq = 4294967295u;
    PRUEFE_TEXT(neu_telemetrie(t), alt_telemetrie(t));
    Aufsetzen a;
    a.captured_vs_samples = -2147483647 - 1;
    a.captured_vs_window_ms = 2147483647;
    PRUEFE_TEXT(neu_aufsetzen(a), alt_aufsetzen(a));
    // Die Felder, auf die alte Clients per serde zugreifen, sind da.
    JWert j;
    std::string grund;
    PRUEFE(json_lies(neu_aufsetzen(beispiel_aufsetzen(3)).substr(0, neu_aufsetzen(beispiel_aufsetzen(3)).size() - 1), &j, &grund));
    for (const char* feld : {"v", "pv", "type", "seq", "ts", "lat", "lon", "captured_vs_fpm",
                             "captured_vs_source", "captured_vs_window_ms", "captured_vs_samples",
                             "captured_g_normal", "captured_pitch_deg", "captured_bank_deg",
                             "captured_ias_kt", "captured_gs_kt", "captured_heading_deg",
                             "fnrml_gear_n", "agl_ft"}) {
        PRUEFE(j.hole(feld) != nullptr);
    }
    PRUEFE(j.hole("captured_vs_fpm")->zahl == -288.4);
}

TEST(protokoll1_nicht_endlich_wird_null) {
    Telemetrie t = beispiel_telemetrie(1);
    t.vs_fpm = std::numeric_limits<float>::quiet_NaN();
    t.lat = std::numeric_limits<double>::infinity();
    t.g_normal = -std::numeric_limits<float>::infinity();
    const std::string s = neu_telemetrie(t);
    PRUEFE(!s.empty() && s.back() == '\n');
    JWert j;
    std::string grund;
    PRUEFE(json_lies(s.substr(0, s.size() - 1), &j, &grund));
    PRUEFE(j.hole("vs_fpm")->art == JWert::Art::NUL);
    PRUEFE(j.hole("lat")->art == JWert::Art::NUL);
    PRUEFE(j.hole("g_normal")->art == JWert::Art::NUL);
    PRUEFE(j.hole("lon")->art == JWert::Art::ZAHL);  // der Rest bleibt
    PRUEFE(s.find("nan") == std::string::npos && s.find("inf") == std::string::npos);
    // Das alte Format war hier kein JSON (Nachweis, dass der Test etwas prüft).
    PRUEFE(!json_lies(alt_telemetrie(t).substr(0, alt_telemetrie(t).size() - 1), &j, &grund));

    Aufsetzen a = beispiel_aufsetzen(0);
    a.captured_vs_fpm = std::numeric_limits<float>::quiet_NaN();
    const std::string td = neu_aufsetzen(a);
    PRUEFE(json_lies(td.substr(0, td.size() - 1), &j, &grund));
    PRUEFE(j.hole("captured_vs_fpm")->art == JWert::Art::NUL);
}

TEST(protokoll1_locale_fest) {
    // Ein anderes Plugin im X-Plane-Prozess stellt LC_NUMERIC um. Nicht jede
    // Maschine hat eine Locale mit Dezimalkomma — wo keine da ist, prüft der
    // Test nur die C-Locale (und sagt es).
    const char* kandidaten[] = {"de_DE.UTF-8", "de_DE.utf8", "de_DE", "fr_FR.UTF-8", "German_Germany.1252"};
    const char* gesetzt = nullptr;
    for (const char* k : kandidaten) {
        if (std::setlocale(LC_NUMERIC, k) != nullptr) { gesetzt = k; break; }
    }
    const Telemetrie t = beispiel_telemetrie(7);
    const Aufsetzen a = beispiel_aufsetzen(7);
    const std::string s = neu_telemetrie(t);
    const std::string td = neu_aufsetzen(a);
    char probe[16];
    std::snprintf(probe, sizeof(probe), "%.1f", 1.5);
    const bool komma = std::string(probe) == "1,5";
    std::setlocale(LC_NUMERIC, "C");
    if (gesetzt == nullptr || !komma) {
        std::printf("     (keine Locale mit Dezimalkomma verfuegbar - nur C-Locale geprueft)\n");
    } else {
        std::printf("     Locale %s: printf schreibt 1,5 - Protokoll 1 trotzdem mit Punkt\n", gesetzt);
    }
    JWert j;
    std::string grund;
    PRUEFE(json_lies(s.substr(0, s.size() - 1), &j, &grund));
    PRUEFE(json_lies(td.substr(0, td.size() - 1), &j, &grund));
    // Unter der fremden Locale erzeugt, in der C-Locale verglichen: gleich.
    PRUEFE_TEXT(s, alt_telemetrie(t));
    PRUEFE_TEXT(td, alt_aufsetzen(a));
}

TEST(protokoll1_zu_kleiner_puffer) {
    char buf[64];
    PRUEFE_GLEICH(schreibe_telemetrie(buf, sizeof(buf), PV, beispiel_telemetrie(0)), size_t(0));
    PRUEFE_GLEICH(schreibe_aufsetzen(buf, sizeof(buf), PV, beispiel_aufsetzen(0)), size_t(0));
}
