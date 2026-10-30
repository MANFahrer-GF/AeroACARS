// Grenzwerte der Teilnoten Sinkrate und G-Kraft — dieselben Zahlen wie in
// Rust (crates/landing-scoring: sub_landing_rate.rs T_VS_*, sub_g_force.rs
// T_G_*); `bewertungsGrenzen.test.ts` hält sie gegen die Rust-Quelle.
//
// GESPIEGELT in die Webapp (scripts/anzeige-sync.mjs, DATEIEN): Die Farben
// der Forensik-Kacheln kommen auf beiden Seiten aus diesen Zahlen.
//
// v3 (P-4.21): Fuer 3.0-Fluege kommen die Grenzen aus der Modulausgabe
// (`modul_grenzen` im Datensatz, `grenzenAus`). Die Zahlen hier gelten nur
// noch fuer den 2.x-Altbestand und den Altbestand-Pfad in landingScoring.ts.

export const T_VS_SMOOTH_FPM = 200;
export const T_VS_FIRM_FPM = 400;
export const T_VS_HARD_FPM = 600;
export const T_VS_SEVERE_FPM = 1000;

export const T_G_SMOOTH = 1.2;
export const T_G_FIRM = 1.4;
export const T_G_HARD = 1.7;
export const T_G_SEVERE = 2.1;

/** Farbgrenzen der Forensik-Kacheln: Sinkrate (fpm) und G, je vier Stufen
 *  (glatt/fest/hart/schwer). Form der Modulausgabe `grenzen`. */
export interface Grenzen {
  vs_fpm: [number, number, number, number];
  g: [number, number, number, number];
}

/** v3 (P-4.21): Nur fuer 2.x-Datensaetze ohne Modulausgabe — die Grenzen,
 *  mit denen sie gerechnet wurden (Score-Version <= 26). Ein 3.0-Flug traegt
 *  seine eigenen (`modul_grenzen`, aus dem Bewertungsmodul seines Stands). */
export const GRENZEN_ALTBESTAND_2X: Grenzen = {
  vs_fpm: [T_VS_SMOOTH_FPM, T_VS_FIRM_FPM, T_VS_HARD_FPM, T_VS_SEVERE_FPM],
  g: [T_G_SMOOTH, T_G_FIRM, T_G_HARD, T_G_SEVERE],
};

/** Grenzen eines Datensatzes: aus dem Modul, sonst (2.x) die alten. */
export function grenzenAus(r: { modul_grenzen?: Grenzen | null }): Grenzen {
  return r.modul_grenzen ?? GRENZEN_ALTBESTAND_2X;
}
