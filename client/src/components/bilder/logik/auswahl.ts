// Auswahl der Kacheln: getrennt vom Oeffnen (Checkbox), hoechstens `max` je Sendung.
export type Auswahl = ReadonlySet<string>;

export function umschalten(a: Auswahl, id: string): Set<string> {
  const n = new Set(a);
  if (n.has(id)) n.delete(id);
  else n.add(id);
  return n;
}

/** Serie als Einheit: Sind alle gewaehlt, fallen alle heraus, sonst kommen alle dazu. */
export function serieUmschalten(a: Auswahl, ids: string[]): Set<string> {
  const n = new Set(a);
  const alle = ids.every((id) => n.has(id));
  for (const id of ids) {
    if (alle) n.delete(id);
    else n.add(id);
  }
  return n;
}

/** Nur Bilder, die es noch gibt (nach Loeschen oder Neuladen). */
export function bereinigen(a: Auswahl, vorhanden: Iterable<string>): Set<string> {
  const v = new Set(vorhanden);
  return new Set([...a].filter((id) => v.has(id)));
}

/**
 * Welches Bild rechts steht, nachdem die Auswahl auf `gewaehlt` gewechselt hat und `id` zuletzt angefasst wurde.
 * Breite Spalte: rechts steht das eben gewaehlte Bild, sonst redeten Detail und Leiste von verschiedenen Bildern.
 * Blatt ueber der Liste (`breit` falsch): ein Haken schlaegt es nicht auf, sonst verdeckt es die naechsten Kacheln.
 * Faellt der Haken am offenen Bild, ruecken das zuletzt gewaehlte Bild nach; ohne Rest bleibt es stehen.
 */
export function offenNachHaken(offen: string | null, id: string, gewaehlt: Auswahl, breit: boolean): string | null {
  if (gewaehlt.has(id)) return breit || offen !== null ? id : null;
  if (offen !== id) return offen;
  return [...gewaehlt].pop() ?? offen;
}

export const zuViele = (a: Auswahl, max: number): boolean => a.size > max;
