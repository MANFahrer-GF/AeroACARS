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
const INDEX_KEY = "aa-track-index";
/**
 * v0.19.x FIX: ohne dies wächst `localStorage` unbegrenzt — ein Key pro
 * jemals geflogenem PIREP (bis zu ~220 KB bei 5000 Punkten), NIE geräumt.
 * Nach genug langen Flügen (Größenordnung 20-45) läuft das Quota voll; der
 * NÄCHSTE ungeschützte `localStorage.setItem`-Call anderswo (z.B. Theme)
 * wirft dann eine Exception. Hält höchstens die letzten N Flüge im Speicher,
 * räumt ältere beim Anlegen eines neuen automatisch weg.
 */
const MAX_PERSISTED_FLIGHTS = 20;
// PARITÄT: Die Ausdünn-Schwellen + die Kappe werden 1:1 im Rust-Streamer
// gespiegelt (record_track_point / MAX_TRACK_POINTS in
// client/src-tauri/src/lib.rs). Das Backend ist die Quelle der geflogenen
// Linie (fokus-unabhängig, lückenlos); ändert sich hier ein Wert, MUSS er dort
// mitgezogen werden, sonst weicht der gepollte Track vom alten in-memory-Track ab.
/** Sicherheitskappe pro Flug, damit localStorage bei Langstrecke nicht platzt. */
const MAX_POINTS = 5000;
/**
 * Schwelle (Grad) fürs Ausdünnen in der LUFT — lange Flüge überladen die
 * Linie sonst (~0,002° ≈ 220 m).
 */
const AIR_MIN_DELTA_DEG = 0.002;
/**
 * v0.15.13: deutlich feinere Schwelle am BODEN (~0,00015° ≈ 16 m). Mit der
 * groben Luft-Schwelle fielen beim Taxi (Bewegungen ≪ 220 m) fast alle Punkte
 * weg → der Rollweg am Flughafen war nicht nachvollziehbar (Live-Report Thomas,
 * BTX2222). Am Boden brauchen wir die Auflösung für Kurven/Rollwege; in der
 * Luft bleibt es grob, damit Langstrecken die Linie nicht überladen.
 */
const GROUND_MIN_DELTA_DEG = 0.00015;

/**
 * Merkt vor, dass `pirepId` gerade persistiert wird, und räumt die
 * ältesten Flüge weg, sobald mehr als `MAX_PERSISTED_FLIGHTS` im Index
 * stehen. Best-effort wie der Rest dieser Datei — ein voller/fehlender
 * localStorage lässt die App einfach ohne Bereinigung weiterlaufen statt
 * zu werfen.
 */
function evictOldFlights(pirepId: string): void {
  try {
    const raw = localStorage.getItem(INDEX_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    const idx: string[] = Array.isArray(parsed)
      ? parsed.filter((x): x is string => typeof x === "string")
      : [];
    const next = idx.filter((id) => id !== pirepId);
    next.push(pirepId);
    while (next.length > MAX_PERSISTED_FLIGHTS) {
      const evicted = next.shift();
      if (evicted) localStorage.removeItem(LS_PREFIX + evicted);
    }
    localStorage.setItem(INDEX_KEY, JSON.stringify(next));
  } catch {
    /* localStorage voll/nicht verfügbar → Eviction fällt diesmal aus,
       kein Absturz. */
  }
}

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
  const isNewThisSession = !hydrated.has(pirepId);
  const arr = isNewThisSession ? loadFromLs(pirepId) : [];
  hydrated.add(pirepId);
  if (isNewThisSession) evictOldFlights(pirepId);
  store.set(pirepId, arr);
  return arr;
}

/**
 * Einen Track-Punkt aufnehmen (lon/lat). No-op bei ungültigen Werten.
 *
 * `onGround` steuert die Ausdünn-Schwelle: am Boden fein (Taxi-Weg sichtbar),
 * in der Luft grob (Langstrecke bleibt leicht). Default `false` = Luft-Schwelle,
 * damit bestehende Aufrufer ohne Flag das alte Verhalten behalten.
 */
export function recordTrackPoint(
  pirepId: string,
  lon: number | null | undefined,
  lat: number | null | undefined,
  onGround: boolean = false,
): void {
  if (typeof lon !== "number" || typeof lat !== "number") return;
  if (Number.isNaN(lon) || Number.isNaN(lat)) return;
  const arr = ensure(pirepId);
  const last = arr[arr.length - 1];
  const minDelta = onGround ? GROUND_MIN_DELTA_DEG : AIR_MIN_DELTA_DEG;
  if (
    !last ||
    Math.abs(last[0] - lon) > minDelta ||
    Math.abs(last[1] - lat) > minDelta
  ) {
    arr.push([lon, lat]);
    if (arr.length > MAX_POINTS) arr.splice(0, arr.length - MAX_POINTS);
    store.set(pirepId, arr);
    persist(pirepId, arr);
  }
}

/**
 * v0.15.x: Den GESAMTEN Track eines PIREP setzen (lon/lat-Paare).
 *
 * Hintergrund: Die geflogene Linie wird jetzt im Rust-Streamer (Backend) bei
 * voller Tick-Rate akkumuliert — fokus-unabhängig, also LÜCKENLOS auch wenn das
 * AeroACARS-Fenster im Hintergrund liegt (X-Plane Vollbild). Der Webview-Poll
 * holte den Track früher aus dem gedrosselten Snapshot-Stream auf, was im
 * Hintergrund Lücken riss. `setTrack` spiegelt den vom Backend gelieferten,
 * bereits ausgedünnten Track 1:1 in den Store (überschreibt — kein Anhängen).
 *
 * Validiert auf endliche Koordinaten, kappt auf die letzten MAX_POINTS und
 * persistiert nach localStorage (gleiches Key-Schema wie `recordTrackPoint`),
 * damit ein App-Neustart mitten im Flug die Linie behält. Backend-Quelle:
 * `record_track_point` / `flight_get_track` in client/src-tauri/src/lib.rs.
 */
export function setTrack(pirepId: string, points: [number, number][]): void {
  if (!pirepId || !Array.isArray(points)) return;
  const clean = points.filter(
    (p): p is [number, number] =>
      Array.isArray(p) &&
      p.length === 2 &&
      typeof p[0] === "number" &&
      typeof p[1] === "number" &&
      Number.isFinite(p[0]) &&
      Number.isFinite(p[1]),
  );
  // Sicherheitskappe wie bei recordTrackPoint: die LETZTEN MAX_POINTS behalten.
  const capped =
    clean.length > MAX_POINTS ? clean.slice(clean.length - MAX_POINTS) : clean;
  // Ab jetzt gilt der Store als „hydratisiert" für diesen PIREP — ein
  // späteres getTrack soll NICHT mehr aus localStorage nachladen (das Backend
  // ist die Quelle der Wahrheit).
  if (!hydrated.has(pirepId)) evictOldFlights(pirepId);
  hydrated.add(pirepId);
  store.set(pirepId, capped);
  persist(pirepId, capped);
}

/** Akkumulierten Track für einen PIREP holen (leer wenn keiner). */
export function getTrack(pirepId: string | null | undefined): [number, number][] {
  return pirepId ? ensure(pirepId) : [];
}
