// v0.13.x — geteilter Track-Store für die In-App-Live-Map.
//
// Wird APP-WEIT von App.tsx gefüttert (aus dem Live-Snapshot-Stream), sobald
// ein Flug aktiv ist — NICHT erst wenn der Karten-Tab geöffnet wird. So zeigt
// die Map den geflogenen Track ab Flugstart, auch wenn man die Karte erst
// später öffnet. Pro PIREP gespeichert, übersteht Tab-Wechsel.
//
// v0.15.7: Track ist jetzt zusätzlich in localStorage gespiegelt und übersteht
// damit auch einen APP-NEUSTART (vorher in-memory → nach Restart war die
// geflogene Linie weg). Beim ersten Zugriff auf einen PIREP wird aus
// localStorage hydratisiert. Best-effort: schlägt localStorage fehl, läuft
// alles weiter in-memory.

const store = new Map<string, [number, number][]>();
const hydrated = new Set<string>();

const LS_PREFIX = "aa-track-";
/** Sicherheitskappe pro Flug, damit localStorage bei Langstrecke nicht platzt. */
const MAX_POINTS = 5000;
/** Schwelle (Grad) fürs Ausdünnen — lange Flüge überladen die Linie sonst. */
const MIN_DELTA_DEG = 0.002;

function persist(pirepId: string, arr: [number, number][]): void {
  try {
    localStorage.setItem(LS_PREFIX + pirepId, JSON.stringify(arr));
  } catch {
    /* localStorage voll/nicht verfügbar → in-memory reicht */
  }
}

function loadFromLs(pirepId: string): [number, number][] {
  try {
    const raw = localStorage.getItem(LS_PREFIX + pirepId);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (p): p is [number, number] =>
        Array.isArray(p) &&
        p.length === 2 &&
        typeof p[0] === "number" &&
        typeof p[1] === "number" &&
        !Number.isNaN(p[0]) &&
        !Number.isNaN(p[1]),
    );
  } catch {
    return [];
  }
}

/** In-memory-Array für einen PIREP holen, beim ersten Mal aus localStorage hydratisieren. */
function ensure(pirepId: string): [number, number][] {
  const existing = store.get(pirepId);
  if (existing) return existing;
  // Erster Zugriff in dieser App-Session → ggf. aus localStorage laden
  // (übersteht App-Neustart mitten im Flug).
  const arr = hydrated.has(pirepId) ? [] : loadFromLs(pirepId);
  hydrated.add(pirepId);
  store.set(pirepId, arr);
  return arr;
}

/** Einen Track-Punkt aufnehmen (lon/lat). No-op bei ungültigen Werten. */
export function recordTrackPoint(
  pirepId: string,
  lon: number | null | undefined,
  lat: number | null | undefined,
): void {
  if (typeof lon !== "number" || typeof lat !== "number") return;
  if (Number.isNaN(lon) || Number.isNaN(lat)) return;
  const arr = ensure(pirepId);
  const last = arr[arr.length - 1];
  if (
    !last ||
    Math.abs(last[0] - lon) > MIN_DELTA_DEG ||
    Math.abs(last[1] - lat) > MIN_DELTA_DEG
  ) {
    arr.push([lon, lat]);
    if (arr.length > MAX_POINTS) arr.splice(0, arr.length - MAX_POINTS);
    store.set(pirepId, arr);
    persist(pirepId, arr);
  }
}

/** Akkumulierten Track für einen PIREP holen (leer wenn keiner). */
export function getTrack(pirepId: string | null | undefined): [number, number][] {
  return pirepId ? ensure(pirepId) : [];
}
