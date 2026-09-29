// =============================================================================
// AeroACARS X-Plane-Plugin — Protokoll-1-Pakete (telemetry, touchdown)
// =============================================================================
//
// Bis Plugin 1.0.0 baute plugin.cpp diese beiden Pakete direkt mit snprintf.
// Die Codex-Abnahme (M3) fand zwei Wege zu ungültigem JSON:
//
//   * printf ist locale-abhängig. Stellt ein anderes Plugin im X-Plane-Prozess
//     LC_NUMERIC um (z. B. de_DE), wird aus 51.2 → "51,2" — der Client
//     verliert die ganze Zeile, beim einmaligen `touchdown` endgültig.
//   * NaN/±Inf (kaputter Dataref-Wert, 0/0 in der Sinkratenrechnung) kamen als
//     "nan"/"inf" heraus — kein JSON.
//
// Deshalb schreibt jetzt der locale-feste JsonSchreiber (json_schreiber.h)
// die Pakete. FELDSCHEMA UND WERTE bleiben unverändert: dieselben Felder in
// derselben Reihenfolge, dieselbe Zahl Nachkommastellen je Feld (das alte
// printf-Format steht als Kommentar an jedem Feld). Für endliche Werte in der
// C-Locale ist die Ausgabe Byte für Byte die alte (Test
// protokoll1_gleich_wie_bisher). Einzige Änderung: ein nicht endlicher Wert
// wird `null` statt "nan" — damit ist wenigstens die Zeile gültiges JSON.
//
// Rein und XPLM-frei (Tests: tests/test_protokoll1.cpp).
// =============================================================================

#pragma once

#include <cstddef>
#include <cstdint>

namespace aeroacars {

struct Telemetrie {
    uint32_t seq = 0;
    double ts = 0.0;
    double lat = 0.0;
    double lon = 0.0;
    float agl_ft = 0.0f;
    float vs_fpm_raw = 0.0f;
    float vs_fpm = 0.0f;
    float fnrml_gear_n = 0.0f;
    bool on_ground = false;
    float g_normal = 0.0f;
    float pitch_deg = 0.0f;
    float bank_deg = 0.0f;
    float hdg_true = 0.0f;
    float ias_kt = 0.0f;
    float gs_kt = 0.0f;
};

struct Aufsetzen {
    uint32_t seq = 0;
    double ts = 0.0;
    double lat = 0.0;
    double lon = 0.0;
    float captured_vs_fpm = 0.0f;
    const char* captured_vs_source = "none";  // nur ASCII-Kennungen
    int captured_vs_window_ms = 0;
    int captured_vs_samples = 0;
    float captured_g_normal = 0.0f;
    float captured_pitch_deg = 0.0f;
    float captured_bank_deg = 0.0f;
    float captured_ias_kt = 0.0f;
    float captured_gs_kt = 0.0f;
    float captured_heading_deg = 0.0f;
    float fnrml_gear_n = 0.0f;
    float agl_ft = 0.0f;
};

// Schreibt das Paket samt '\n' nach `puffer`. Rückgabe: Länge, oder 0, wenn
// `kapazitaet` nicht reicht (dann wird nichts gesendet). `plugin_version`
// ist das Feld "pv".
size_t schreibe_telemetrie(char* puffer, size_t kapazitaet, const char* plugin_version,
                           const Telemetrie& t) noexcept;
size_t schreibe_aufsetzen(char* puffer, size_t kapazitaet, const char* plugin_version,
                          const Aufsetzen& a) noexcept;

}  // namespace aeroacars
