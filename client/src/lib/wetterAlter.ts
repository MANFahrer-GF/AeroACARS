// v1.7.30 (Pilotenwunsch 16.09.2026): Wie alt ist das gezeigte Wetter?
//
// Absichtlich KEINE Uhrzeit: Die Frage am Cockpit lautet „ist das noch
// frisch", und dafür müsste der Pilot ein Datum erst im Kopf gegen die
// aktuelle Zeit rechnen. Ein METAR wird stündlich ausgegeben — älter als
// eine Stunde heisst: es kann längst überholt sein, und das muss man
// sehen, statt es aus einer Zahl zu erschliessen.

/** Ab hier gilt das Wetter als veraltet (METAR wird stündlich ausgegeben). */
export const VERALTET_AB_MIN = 60;

export interface WetterAlter {
  /** Minuten seit dem Zeitstempel, nie negativ. */
  minuten: number;
  /** Älter als [`VERALTET_AB_MIN`]. */
  veraltet: boolean;
  /** Schlüssel + Werte für die Übersetzung. */
  text: { key: string; werte?: Record<string, number> };
}

/**
 * Alter eines Wetter-Zeitstempels. `null`, wenn der Zeitstempel fehlt oder
 * unlesbar ist — dann zeigt die Oberfläche lieber nichts als eine erfundene
 * Angabe.
 *
 * Ein Zeitstempel aus der Zukunft (Uhr des Rechners läuft nach, Station
 * meldet die volle Stunde im Voraus) zählt als „gerade eben" statt als
 * negative Zahl.
 */
export function wetterAlter(zeit: string | null | undefined, jetzt: Date): WetterAlter | null {
  if (!zeit) return null;
  const t = Date.parse(zeit);
  if (Number.isNaN(t)) return null;
  const minuten = Math.max(0, Math.floor((jetzt.getTime() - t) / 60000));
  const veraltet = minuten >= VERALTET_AB_MIN;
  if (minuten < 1) return { minuten, veraltet, text: { key: "weather.age_now" } };
  if (minuten < 90) {
    return { minuten, veraltet, text: { key: "weather.age_minutes", werte: { min: minuten } } };
  }
  const stunden = Math.floor(minuten / 60);
  // Bis 47 Stunden in Stunden: „vor 1 Tagen" wäre falsch, und eine
  // Sonderform für die Einzahl in drei Sprachen lohnt hier nicht
  // (Codex-QS P3, 16.09.2026).
  if (stunden < 48) {
    return { minuten, veraltet, text: { key: "weather.age_hours", werte: { std: stunden } } };
  }
  return {
    minuten,
    veraltet,
    text: { key: "weather.age_days", werte: { tage: Math.floor(stunden / 24) } },
  };
}
