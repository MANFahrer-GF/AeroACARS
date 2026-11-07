// Datumsbeschriftung der Gruppen: „Heute“, „Gestern“, sonst das Datum (Ortszeit des Piloten).
export type DatumArt = "heute" | "gestern" | "datum";

const tagStart = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

export function datumArt(ms: number, jetzt: number): { art: DatumArt; datum: string } {
  const d = new Date(ms);
  const tage = Math.round((tagStart(new Date(jetzt)) - tagStart(d)) / 86400000);
  return { art: tage === 0 ? "heute" : tage === 1 ? "gestern" : "datum", datum: "" };
}

export function datumText(ms: number, sprache: string): string {
  return new Date(ms).toLocaleDateString(sprache, { day: "2-digit", month: "2-digit", year: "numeric" });
}

/** Track des Programms ([lon, lat]) als [lat, lon]. */
export function spurAusTrack(track: unknown): Array<[number, number]> {
  if (!Array.isArray(track)) return [];
  return track
    .filter((p): p is [number, number] => Array.isArray(p) && typeof p[0] === "number" && typeof p[1] === "number")
    .map((p) => [p[1], p[0]]);
}
