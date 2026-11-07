// Stundenkontingent (P-7.25, 12 je Stunde laut Server): Text „diese Stunde noch N von G“,
// nie „heute“. Zahlen kommen aus der Serverantwort, nie aus dem Code.
import type { Kontingent } from "./typen";

export type KontingentAnzeige =
  | { art: "rest"; rest: number; grenze: number }
  | { art: "leer"; minuten: number }
  | { art: "unbekannt" };

export function kontingentAnzeige(k: Kontingent | null): KontingentAnzeige {
  if (!k) return { art: "unbekannt" };
  if (k.rest > 0) return { art: "rest", rest: k.rest, grenze: k.grenze };
  return { art: "leer", minuten: Math.max(1, Math.ceil(k.retry_after / 60)) };
}

/** Darf gesendet werden? Mit bekanntem Kontingent nur, wenn es fuer alle Bilder reicht. */
export function reichtKontingent(anzahl: number, k: Kontingent | null): boolean {
  return k === null || anzahl <= k.rest;
}

/** `retry_after` aus Serverantwort oder Kopfzeile (Sekunden oder Datum) in Sekunden. */
export function retrySekunden(v: unknown): number | undefined {
  if (typeof v === "number" && Number.isFinite(v) && v > 0) return Math.ceil(v);
  if (typeof v === "string" && v.trim() !== "") {
    const n = Number(v);
    if (Number.isFinite(n) && n > 0) return Math.ceil(n);
  }
  return undefined;
}
