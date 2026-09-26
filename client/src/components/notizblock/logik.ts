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

/** Ab dieser Kontaktgroesse (CSS-Pixel, Breite oder Hoehe) ist eine
 *  Beruehrung ein Handballen, kein Finger. Safari auf dem iPad meldet die
 *  Groesse aus dem Beruehrungsradius; eine Fingerkuppe kann dort 40 px und
 *  mehr erreichen (Cloud-QS: 32 px haette Finger gesperrt), ein Handballen
 *  liegt deutlich darueber. Noch am echten iPad nachzumessen. */
export const HANDBALLEN_PX = 60;

/** Handballen-Schutz: Darf diese Beruehrung zeichnen?
 *  - Stift und Maus immer.
 *  - Grosse Auflageflaeche nie.
 *  - Finger nur, wenn freigegeben oder noch nie ein Stift gesehen wurde. */
export function darfZeichnen(
  typ: string,
  breite: number,
  hoehe: number,
  fingerFrei: boolean,
  stiftBekannt: boolean,
): boolean {
  if (typ === "pen" || typ === "mouse") return true;
  // Finger ausdruecklich erlaubt: nie sperren, auch nicht nach Groesse.
  if (fingerFrei) return true;
  if (Math.max(breite || 0, hoehe || 0) >= HANDBALLEN_PX) return false;
  return !stiftBekannt;
}

/** Wie lange vor dem ersten Stiftkontakt ein Finger-Strich als Handballen
 *  gilt und nachtraeglich verschwindet (ms). */
export const HANDBALLEN_NACHLAUF_MS = 1500;

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
    const zahl = (v: unknown) => typeof v === "number" && Number.isFinite(v);
    // Jeder Punkt muss x/y/p als endliche Zahl haben — ein kaputter Punkt
    // liesse das Zeichnen spaeter abbrechen (Codex-Befund 5).
    return d.filter(
      (s): s is Strich =>
        !!s &&
        typeof (s as Strich).farbe === "string" &&
        zahl((s as Strich).breite) &&
        Array.isArray((s as Strich).punkte) &&
        (s as Strich).punkte.length > 0 &&
        (s as Strich).punkte.every(
          (q) => !!q && zahl((q as Punkt).x) && zahl((q as Punkt).y) && zahl((q as Punkt).p),
        ),
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
