// Serien: zeitlich benachbarte Bilder werden zu einer Kachel zusammengefasst.
import type { Eintrag } from "./typen";

export type Kachel =
  | { art: "bild"; bild: Eintrag }
  | { art: "serie"; bilder: Eintrag[]; schluessel: string };

export function bildZeit(e: Eintrag): number {
  const z = e.sidecar?.meta?.zeit_utc ? Date.parse(e.sidecar.meta.zeit_utc) : NaN;
  return Number.isFinite(z) ? z : (e.dateizeit_ms ?? 0);
}

/** `bilder` aelteste zuerst (wie im Entwurf: der Flug von links nach rechts); Serie = mindestens `minBilder` Bilder mit Abstand <= `abstandS`. */
export function serienBilden(bilder: Eintrag[], abstandS: number, minBilder: number): Kachel[] {
  const sortiert = [...bilder].sort((a, b) => bildZeit(a) - bildZeit(b));
  const kacheln: Kachel[] = [];
  let lauf: Eintrag[] = [];
  const abschliessen = () => {
    if (lauf.length >= minBilder) kacheln.push({ art: "serie", bilder: lauf, schluessel: `serie:${lauf[0].id}` });
    else lauf.forEach((b) => kacheln.push({ art: "bild", bild: b }));
    lauf = [];
  };
  for (const b of sortiert) {
    if (lauf.length && Math.abs(bildZeit(lauf[lauf.length - 1]) - bildZeit(b)) > abstandS * 1000) abschliessen();
    lauf.push(b);
  }
  abschliessen();
  return kacheln;
}

export const kachelBilder = (k: Kachel): Eintrag[] => (k.art === "bild" ? [k.bild] : k.bilder);
