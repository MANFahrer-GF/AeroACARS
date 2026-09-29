// =============================================================================
// AeroACARS X-Plane-Plugin — Protokoll-1-Pakete (Umsetzung)
// =============================================================================
//
// Reihenfolge und Nachkommastellen jedes Felds entsprechen exakt dem
// snprintf-Format aus Plugin 0.5.13 – 1.0.0 (Kommentar je Zeile). Nicht
// "verbessern": alte Clients (serde, f32/f64 mit default) und die
// Rust-Tests des Clients hängen an genau diesem Format.
// =============================================================================

#include "protokoll1.h"

#include "json_schreiber.h"

namespace aeroacars {

namespace {

size_t abschliessen(const JsonSchreiber& w) noexcept {
    return w.ueberlauf() ? 0 : w.laenge();
}

void kopf(JsonSchreiber& w, const char* plugin_version, const char* typ, uint32_t seq,
          double ts, double lat, double lon) noexcept {
    w.roh("{\"v\":1,\"pv\":");
    w.text_bis_nul(plugin_version ? plugin_version : "", 32);  // "pv":"%s"
    w.roh(",\"type\":\"");
    w.roh(typ);
    w.roh("\",\"seq\":");
    w.ganzzahl(seq);                                          // %u
    w.roh(",\"ts\":");
    w.zahl_fest(ts, 6);                                       // %.6f
    w.roh(",\"lat\":");
    w.zahl_fest(lat, 7);                                      // %.7f
    w.roh(",\"lon\":");
    w.zahl_fest(lon, 7);                                      // %.7f
}

}  // namespace

size_t schreibe_telemetrie(char* puffer, size_t kapazitaet, const char* plugin_version,
                           const Telemetrie& t) noexcept {
    JsonSchreiber w(puffer, kapazitaet);
    kopf(w, plugin_version, "telemetry", t.seq, t.ts, t.lat, t.lon);
    w.roh(",\"agl_ft\":");       w.zahl_fest(t.agl_ft, 2);        // %.2f
    w.roh(",\"vs_fpm_raw\":");   w.zahl_fest(t.vs_fpm_raw, 2);    // %.2f
    w.roh(",\"vs_fpm\":");       w.zahl_fest(t.vs_fpm, 2);        // %.2f
    w.roh(",\"fnrml_gear_n\":"); w.zahl_fest(t.fnrml_gear_n, 2);  // %.2f
    w.roh(",\"on_ground\":");    w.roh(t.on_ground ? "true" : "false");
    w.roh(",\"g_normal\":");     w.zahl_fest(t.g_normal, 4);      // %.4f
    w.roh(",\"pitch_deg\":");    w.zahl_fest(t.pitch_deg, 3);     // %.3f
    w.roh(",\"bank_deg\":");     w.zahl_fest(t.bank_deg, 3);      // %.3f
    w.roh(",\"hdg_true\":");     w.zahl_fest(t.hdg_true, 3);      // %.3f
    w.roh(",\"ias_kt\":");       w.zahl_fest(t.ias_kt, 2);        // %.2f
    w.roh(",\"gs_kt\":");        w.zahl_fest(t.gs_kt, 2);         // %.2f
    w.roh("}\n");
    return abschliessen(w);
}

size_t schreibe_aufsetzen(char* puffer, size_t kapazitaet, const char* plugin_version,
                          const Aufsetzen& a) noexcept {
    JsonSchreiber w(puffer, kapazitaet);
    kopf(w, plugin_version, "touchdown", a.seq, a.ts, a.lat, a.lon);
    w.roh(",\"captured_vs_fpm\":");       w.zahl_fest(a.captured_vs_fpm, 2);       // %.2f
    w.roh(",\"captured_vs_source\":");
    w.text_bis_nul(a.captured_vs_source ? a.captured_vs_source : "", 64);          // "%s"
    w.roh(",\"captured_vs_window_ms\":"); w.ganzzahl(a.captured_vs_window_ms);     // %d
    w.roh(",\"captured_vs_samples\":");   w.ganzzahl(a.captured_vs_samples);       // %d
    w.roh(",\"captured_g_normal\":");     w.zahl_fest(a.captured_g_normal, 4);     // %.4f
    w.roh(",\"captured_pitch_deg\":");    w.zahl_fest(a.captured_pitch_deg, 3);    // %.3f
    w.roh(",\"captured_bank_deg\":");     w.zahl_fest(a.captured_bank_deg, 3);     // %.3f
    w.roh(",\"captured_ias_kt\":");       w.zahl_fest(a.captured_ias_kt, 2);       // %.2f
    w.roh(",\"captured_gs_kt\":");        w.zahl_fest(a.captured_gs_kt, 2);        // %.2f
    w.roh(",\"captured_heading_deg\":");  w.zahl_fest(a.captured_heading_deg, 3);  // %.3f
    w.roh(",\"fnrml_gear_n\":");          w.zahl_fest(a.fnrml_gear_n, 2);          // %.2f
    w.roh(",\"agl_ft\":");                w.zahl_fest(a.agl_ft, 2);                // %.2f
    w.roh("}\n");
    return abschliessen(w);
}

}  // namespace aeroacars
