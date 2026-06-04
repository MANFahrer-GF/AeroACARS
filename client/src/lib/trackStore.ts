// v0.13.x — geteilter Track-Store für die In-App-Live-Map.
//
// Wird APP-WEIT von App.tsx gefüttert (aus dem Live-Snapshot-Stream), sobald
// ein Flug aktiv ist — NICHT erst wenn der Karten-Tab geöffnet wird. So zeigt
// die Map den geflogenen Track ab Flugstart, auch wenn man die Karte erst
// später öffnet. Pro PIREP gespeichert, übersteht Tab-Wechsel; bewusst NICHT
// persistent über App-Neustart (nach Restart beginnt der Track neu — der
// Backend-PIREP/JSONL bleibt davon unberührt).

const store = new Map<string, [number, number][]>();

/** Schwelle (Grad) fürs Ausdünnen — lange Flüge überladen die Linie sonst. */
const MIN_DELTA_DEG = 0.002;

/** Einen Track-Punkt aufnehmen (lon/lat). No-op bei ungültigen Werten. */
export function recordTrackPoint(
  pirepId: string,
  lon: number | null | undefined,
  lat: number | null | undefined,
): void {
  if (typeof lon !== "number" || typeof lat !== "number") return;
  if (Number.isNaN(lon) || Number.isNaN(lat)) return;
  const arr = store.get(pirepId) ?? [];
  const last = arr[arr.length - 1];
  if (
    !last ||
    Math.abs(last[0] - lon) > MIN_DELTA_DEG ||
    Math.abs(last[1] - lat) > MIN_DELTA_DEG
  ) {
    arr.push([lon, lat]);
    store.set(pirepId, arr);
  }
}

/** Akkumulierten Track für einen PIREP holen (leer wenn keiner). */
export function getTrack(pirepId: string | null | undefined): [number, number][] {
  return pirepId ? store.get(pirepId) ?? [] : [];
}
