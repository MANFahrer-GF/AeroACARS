/**
 * Notizblock (v1.8.1) — reine Logik ohne DOM, damit sie testbar ist.
 *
 * Striche werden in Breiten-Einheiten gespeichert: x UND y relativ zur
 * Flaechenbreite. So behaelt die Schrift beim Drehen des iPads oder bei
 * einer anderen Fensterhoehe ihre Form (getrennte Achsen haetten sie
 * gestreckt, Cloud-QS Befund 1). Was nach dem Drehen unter den Rand faellt,
 * ist nach dem Zurueckdrehen wieder da.
 */

export interface Punkt {
  /** 0..1 der Breite */
  x: number;
  /** Abstand von oben, ebenfalls in Einheiten der Breite */
  y: number;
  /** Andruck 0..1 (Maus/Finger: 0.5) */
  p: number;
}

export interface Strich {
  farbe: string;
  /** Grundbreite relativ zur Flaechenbreite */
  breite: number;
  punkte: Punkt[];
}

export const SPEICHER_SCHLUESSEL = "aeroacars.notizblock.v2";
const MAX_VERLAUF = 60;

/** Linienbreite in Pixeln fuer einen Punkt: Andruck macht dicker. */
export function pixelBreite(s: Strich, p: number, flaecheBreite: number): number {
  const druck = 0.35 + 0.9 * Math.max(0, Math.min(1, p));
  return Math.max(0.8, s.breite * flaecheBreite * druck);
}

/** Trifft ein Radierpunkt einen Strich? Alles in Breiten-Einheiten. */
export function trifft(s: Strich, x: number, y: number, radius: number): boolean {
  for (let i = 0; i < s.punkte.length; i++) {
    const a = s.punkte[i];
    const b = s.punkte[i + 1] ?? a;
    if (abstandZuStrecke(x, y, a.x, a.y, b.x, b.y) <= radius) {
      return true;
    }
  }
  return false;
}

function abstandZuStrecke(px: number, py: number, ax: number, ay: number, bx: number, by: number): number {
  const dx = bx - ax;
  const dy = by - ay;
  const l2 = dx * dx + dy * dy;
  const t = l2 === 0 ? 0 : Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / l2));
  const qx = ax + t * dx;
  const qy = ay + t * dy;
  return Math.hypot(px - qx, py - qy);
}

/** Verlauf fuer „Rueckgaengig": Zustaende vor jeder Aenderung. */
export class Verlauf {
  private stapel: Strich[][] = [];
  merken(zustand: Strich[]) {
    this.stapel.push(zustand);
    if (this.stapel.length > MAX_VERLAUF) this.stapel.shift();
  }
  zurueck(): Strich[] | null {
    return this.stapel.pop() ?? null;
  }
  get leer(): boolean {
    return this.stapel.length === 0;
  }
}

/** Gespeicherte Striche laden — kaputte oder fremde Daten ergeben leer. */
export function laden(roh: string | null): Strich[] {
  if (!roh) return [];
  try {
    const d = JSON.parse(roh) as unknown;
    if (!Array.isArray(d)) return [];
    return d.filter(
      (s): s is Strich =>
        !!s &&
        typeof (s as Strich).farbe === "string" &&
        typeof (s as Strich).breite === "number" &&
        Array.isArray((s as Strich).punkte),
    );
  } catch {
    return [];
  }
}

/** Punkte runden, damit der Speicher klein bleibt (4 Stellen = 0,1 mm auf dem iPad). */
export function kompakt(striche: Strich[]): Strich[] {
  const r = (v: number) => Math.round(v * 10000) / 10000;
  return striche.map((s) => ({
    ...s,
    punkte: s.punkte.map((q) => ({ x: r(q.x), y: r(q.y), p: Math.round(q.p * 100) / 100 })),
  }));
}
