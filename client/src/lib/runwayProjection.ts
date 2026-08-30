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
  /**
   * Sichtbarer Ausschnitt in Metern ab der Landeschwelle.
   *
   * Ohne Angabe wird die ganze Bahn gezeigt. Beim Hineinzoomen bekommen
   * **beide** Ansichten denselben Ausschnitt — dieselbe Projektion, also
   * dieselben Kanten. Getrennte Zoomzustände wären genau der Fehler, gegen
   * den §8.4 diese Funktion vorschreibt: Zwei Ansichten, die nicht mehr
   * fluchten, sind schlimmer als eine.
   */
  sichtVonM?: number;
  sichtBisM?: number;
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
  /**
   * Wie `mToX`, aber für Werte, die **ab BAHNANFANG** gemessen sind.
   *
   * # Warum es diese zweite Funktion gibt
   *
   * Der Payload führt BEIDE Bezugspunkte nebeneinander, und nirgends stand
   * geschrieben, welcher wo gilt:
   *
   *   ab Landeschwelle:  `td_distance_from_threshold_m`, `aim_point_m`
   *   ab Bahnanfang:     `lateral_samples[].laengs_m`, `mess_ende_laengs_m`,
   *                      `scoring_cutoff_m`, `clearance_point_m`
   *
   * Auf Bahnen ohne versetzte Schwelle sind beide gleich — deshalb fiel es
   * nie auf. Auf TJPS 12 (573 m versetzte Schwelle) lagen Aufsetzpunkt und
   * Rollspur exakt um diese 573 m auseinander: Der Pilot sah eine Marke,
   * die mit der Spur nicht zusammenhing, und einen Räumpunkt 573 m zu weit
   * rechts. Gemeldet an Flug LAN273 (30.08.2026).
   *
   * ⚠ Der eigentliche Fehler war, dass `mToX` BEIDE Bedeutungen annimmt,
   * ohne zu fragen. Deshalb steht die Bedeutung jetzt im NAMEN: Wer einen
   * Wert zeichnet, muss sich entscheiden, und ein Griff zur falschen
   * Funktion ist beim Lesen sichtbar statt still.
   */
  mAbBahnanfangZuX: (m: number) => number;
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
  /** Der gezeigte Ausschnitt in Metern — für Bedienelemente und Tests. */
  sichtVonM: number;
  sichtBisM: number;
  /** Ist überhaupt hineingezoomt? */
  gezoomt: boolean;
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

  // Der Ausschnitt: ohne Angabe die ganze Bahn, sonst der gewählte
  // Bereich — auf die Bahn begrenzt und mit einer Mindestbreite, damit
  // ein versehentlicher Vollzoom die Projektion nicht entarten lässt.
  const MIN_SICHT_M = 50;
  const ganzVon = -ddsM;
  const ganzBis = lengthM;
  let von = Number.isFinite(e.sichtVonM ?? NaN) ? e.sichtVonM! : ganzVon;
  let bis = Number.isFinite(e.sichtBisM ?? NaN) ? e.sichtBisM! : ganzBis;
  von = Math.max(ganzVon, Math.min(von, ganzBis - MIN_SICHT_M));
  bis = Math.min(ganzBis, Math.max(bis, von + MIN_SICHT_M));
  const sichtM = bis - von;

  const pxProMeter = e.innerW / sichtM;
  // X der Landeschwelle: dort, wo Meter 0 im Ausschnitt liegt.
  const thresholdX = e.padX + (0 - von) * pxProMeter;
  const mToXUnbegrenzt = (m: number) => thresholdX + m * pxProMeter;
  const mToX = (m: number) => mToXUnbegrenzt(Math.max(von, Math.min(bis, m)));
  // Ab Bahnanfang gemessen: erst auf die Schwelle beziehen, dann zeichnen.
  const mAbBahnanfangZuX = (m: number) => mToX(m - ddsM);

  return {
    mToX,
    mAbBahnanfangZuX,
    mToXUnbegrenzt,
    thresholdX,
    // Bahnanfang und -ende liegen ausserhalb des Zeichenbereichs, sobald
    // hineingezoomt ist. Sie werden geklemmt, damit die Flächen an den
    // Bildrändern enden statt darüber hinaus.
    bahnAnfangX: Math.max(e.padX, mToXUnbegrenzt(ganzVon)),
    bahnEndeX: Math.min(e.padX + e.innerW, mToXUnbegrenzt(ganzBis)),
    totalVisualM,
    pxProMeter,
    lengthM,
    ddsM,
    sichtVonM: von,
    sichtBisM: bis,
    gezoomt: von > ganzVon + 0.5 || bis < ganzBis - 0.5,
  };
}
