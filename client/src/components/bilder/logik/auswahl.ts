// Auswahl der Kacheln: getrennt vom Oeffnen (Checkbox), hoechstens `max` je Sendung.
export type Auswahl = ReadonlySet<string>;

export function umschalten(a: Auswahl, id: string): Set<string> {
  const n = new Set(a);
  if (n.has(id)) n.delete(id);
  else n.add(id);
  return n;
}

/** Nur Bilder, die es noch gibt (nach Loeschen oder Neuladen). */
export function bereinigen(a: Auswahl, vorhanden: Iterable<string>): Set<string> {
  const v = new Set(vorhanden);
  return new Set([...a].filter((id) => v.has(id)));
}

export const zuViele = (a: Auswahl, max: number): boolean => a.size > max;
