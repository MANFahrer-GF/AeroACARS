// Grenzwerte der Teilnoten Sinkrate und G-Kraft — dieselben Zahlen wie in
// Rust (crates/landing-scoring: sub_landing_rate.rs T_VS_*, sub_g_force.rs
// T_G_*); `bewertungsGrenzen.test.ts` hält sie gegen die Rust-Quelle.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN): Die Farben
// der Forensik-Kacheln kommen auf beiden Seiten aus diesen Zahlen.

export const T_VS_SMOOTH_FPM = 200;
export const T_VS_FIRM_FPM = 400;
export const T_VS_HARD_FPM = 600;
export const T_VS_SEVERE_FPM = 1000;

export const T_G_SMOOTH = 1.2;
export const T_G_FIRM = 1.4;
export const T_G_HARD = 1.7;
export const T_G_SEVERE = 2.1;
