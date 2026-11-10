export interface BeladungsKlasse {
  id: number;
  code: string;
  name: string;
  /** 0 = Passagiere, 1 = Fracht. */
  typ: number;
  kapazitaet: number;
}

export interface VfrBeladung {
  fare_id: number;
  anzahl: number;
}

/**
 * Eine Zeile je Klasse, auch für leer gelassene Felder (0): So trägt der PIREP
 * die bewusste Angabe des Piloten, und die Webseite meldet keinen Flug „ohne
 * Beladung“. Werte über der Grenze werden auf die Grenze gesetzt.
 */
export function beladungFuerPlan(
  klassen: BeladungsKlasse[],
  werte: Record<number, string>,
): VfrBeladung[] {
  return klassen.map((k) => {
    const zahl = parseInt(werte[k.id] ?? "", 10);
    const anzahl = Number.isFinite(zahl) ? Math.min(Math.max(zahl, 0), k.kapazitaet) : 0;
    return { fare_id: k.id, anzahl };
  });
}
