// Die **eine** Projektionsfunktion für Längs- und Queransicht.
//
// Spec: `docs/spec/v1.7.0-bahndisziplin.md` §8.4 — verbindlich.
//
// # Warum das ein eigenes Modul ist
//
// Im ersten Entwurf war die Längsansicht nach Augenmaß aus einem Screenshot
// nachgebaut. Gemessen am 23.08.2026 stand der Aim-Marker **209 m** an der
// falschen Stelle, die Pre-Threshold-Zone 102 m, der Aufsetzpunkt 78 m. Für
// sich betrachtet sah die Ansicht plausibel aus — auffällig wurde es erst,
// als beide Ansichten untereinander standen und nicht fluchteten.
//
// Das ist dieselbe Fehlerklasse wie die Zweitimplementierungen in §9: Zwei
// Stellen, die dasselbe rechnen sollen, driften auseinander, sobald sie nicht
// dieselbe Quelle benutzen. Die Antwort darauf ist nicht Sorgfalt, sondern
// eine Funktion, die beide benutzen **müssen**, weil es keine zweite gibt.
//
// # Was „Meter" hier heisst
//
// Immer die Distanz **vom Landethreshold**, vorzeichenbehaftet. Negative
// Werte liegen davor, in der Zone der versetzten Schwelle. Das ist dieselbe
// Konvention wie in der Telemetrie (`td_distance_from_threshold_m`) — damit
// muss an keiner Stelle umgerechnet werden.

/** Eingang: die Bahn und der verfügbare Zeichenbereich. */
export interface ProjektionsEingang {
  /** Nutzbare Landebahn ab der versetzten Schwelle, in Metern. */
  lengthM: number;
  /** Länge der Zone vor der Landeschwelle (versetzte Schwelle), in Metern. */
  ddsM: number;
  /** Linker Rand des Zeichenbereichs, in Pixeln. */
  padX: number;
  /** Breite des Zeichenbereichs, in Pixeln. */
  innerW: number;
}

/** Die Projektion — für beide Ansichten identisch. */
export interface Projektion {
  /** Meter ab Landeschwelle → X in Pixeln. Begrenzt auf die Bahn. */
  mToX: (m: number) => number;
  /**
   * Wie `mToX`, aber **ohne** Begrenzung. Nur für Messungen, nie zum
   * Zeichnen: Ein Wert jenseits der Bahn würde sonst aus dem Bild laufen.
   */
  mToXUnbegrenzt: (m: number) => number;
  /** X der Landeschwelle. Bei versetzter Schwelle rechts vom Bahnanfang. */
  thresholdX: number;
  /** X des physischen Bahnanfangs (= linker Rand der Fläche). */
  bahnAnfangX: number;
  /** X des Bahnendes (= rechter Rand der Fläche). */
  bahnEndeX: number;
  /** Gesamte gezeichnete Bahnlänge in Metern (Vor-Zone + Landebahn). */
  totalVisualM: number;
  /** Pixel je Meter — für Massstabsangaben in der Grafik. */
  pxProMeter: number;
  /** Die bereinigten Eingangswerte (nach Untergrenze). */
  lengthM: number;
  ddsM: number;
}

/**
 * Untergrenze für die nutzbare Länge.
 *
 * Tief genug, dass keine echte Bahn sie berührt — die kürzeste mit versetzter
 * Schwelle in den Navdaten hat 292 m nutzbare Länge. Hoch genug, dass ein
 * kaputter Kleinstwert die Zeichnung nicht entarten lässt: Bei 0,5 m bildete
 * die Projektion jeden Meter auf ein Vielfaches der Bahnbreite ab.
 *
 * Die früheren 500 m waren dafür zu grob — sie überschrieben echte kurze
 * Plätze. 19 Bahnen rutschen durch den Schwellenabzug unter 500 m (EDKU,
 * EDXZ, EDNG, LOAD …); dort hätte das Bild eine Bahn gezeichnet, die es nicht
 * gibt, mit dem Aufsetzpunkt an der falschen Stelle.
 */
export const MIN_LAENGE_M = 100;

/** Ersatzlänge, wenn gar kein brauchbarer Wert vorliegt. */
const ERSATZ_LAENGE_M = 500;

export function erzeugeProjektion(e: ProjektionsEingang): Projektion {
  const lengthM = Number.isFinite(e.lengthM)
    ? Math.max(MIN_LAENGE_M, e.lengthM)
    : ERSATZ_LAENGE_M;
  const ddsM = Number.isFinite(e.ddsM) ? Math.max(0, e.ddsM) : 0;
  const totalVisualM = lengthM + ddsM;
  const pxProMeter = e.innerW / totalVisualM;

  const thresholdX = e.padX + ddsM * pxProMeter;
  const mToXUnbegrenzt = (m: number) => thresholdX + m * pxProMeter;
  const mToX = (m: number) =>
    mToXUnbegrenzt(Math.max(-ddsM, Math.min(lengthM, m)));

  return {
    mToX,
    mToXUnbegrenzt,
    thresholdX,
    bahnAnfangX: e.padX,
    bahnEndeX: e.padX + e.innerW,
    totalVisualM,
    pxProMeter,
    lengthM,
    ddsM,
  };
}
