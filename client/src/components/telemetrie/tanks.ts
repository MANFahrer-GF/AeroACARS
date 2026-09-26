/**
 * Tanks des Telemetrie-Monitors: welche Säulen die Tafel „Sprit je Tank"
 * zeigt und ob die Summe der Tanks zum Sprit an Bord passt.
 *
 * Reine Rechnung ohne React, damit sie sich mit echten Werten testen lässt.
 */

/**
 * Reihenfolge der Säulen: MSFS-Standardtanks von links nach rechts, dann
 * Außentanks, PMDG-Zusatztank und die neun X-Plane-Tanks. Welche davon ein
 * Muster wirklich hat, entscheidet der Katalog (kein Wert = kein Tank).
 */
export const TANK_IDS = [
  "tank_links_tip",
  "tank_links_aux",
  "tank_links",
  "tank_mitte",
  "tank_mitte_2",
  "tank_mitte_3",
  "tank_rechts",
  "tank_rechts_aux",
  "tank_rechts_tip",
  "tank_extern_1",
  "tank_extern_2",
  "tank_4",
  "xp_tank_1",
  "xp_tank_2",
  "xp_tank_3",
  "xp_tank_4",
  "xp_tank_5",
  "xp_tank_6",
  "xp_tank_7",
  "xp_tank_8",
  "xp_tank_9",
  // MSFS 2024, modernes Treibstoffsystem (`NEW FUEL SYSTEM`): Tank.1 … Tank.12.
  "fs_tank_1",
  "fs_tank_2",
  "fs_tank_3",
  "fs_tank_4",
  "fs_tank_5",
  "fs_tank_6",
  "fs_tank_7",
  "fs_tank_8",
  "fs_tank_9",
  "fs_tank_10",
  "fs_tank_11",
  "fs_tank_12",
] as const;

export interface TankSaeule {
  id: string;
  /** Inhalt in kg. */
  kg: number;
  /** Fassungsvermögen in kg, falls bekannt. */
  kapKg: number | null;
  /** Säulenhöhe 0..1. */
  anteil: number;
}

export interface TankAnsicht {
  saeulen: TankSaeule[];
  /** true: Säule = Füllstand des Tanks; false: Anteil am Sprit an Bord. */
  nachKapazitaet: boolean;
}

type Wert = (id: string) => number | null;

export function tankAnsicht(wert: Wert): TankAnsicht {
  const roh: Array<{ id: string; kg: number; kapKg: number | null }> = [];
  for (const id of TANK_IDS) {
    const kg = wert(id);
    if (kg === null) continue;
    const kapKg = wert(`${id}_kap`);
    // Fassungsvermögen 0 = das Muster hat diesen Tank nicht.
    if (kapKg !== null && kapKg <= 0) continue;
    roh.push({ id, kg, kapKg });
  }
  const nachKapazitaet = roh.length > 0 && roh.every((x) => x.kapKg !== null && x.kapKg > 0);
  const bezug = Math.max(1, wert("sprit_gesamt") ?? roh.reduce((a, x) => a + x.kg, 0));
  const saeulen = roh.map((x) => {
    const nenner = nachKapazitaet && x.kapKg ? x.kapKg : bezug;
    return { ...x, anteil: Math.max(0, Math.min(1, x.kg / nenner)) };
  });
  return { saeulen, nachKapazitaet };
}

/** Grenze, ab der die Summe der Tanks vom Sprit an Bord abweicht. */
export const ABWEICHUNG_ANTEIL = 0.02;

/**
 * Weicht die Summe der Tanks um mehr als 2 % vom Sprit an Bord ab, liegt
 * Sprit in Tanks, die die Standardvariablen nicht abbilden.
 */
export function tanksAbweichend(summe: number | null, anBord: number | null): boolean {
  if (summe === null || anBord === null || anBord <= 0) return false;
  return Math.abs(summe - anBord) / anBord > ABWEICHUNG_ANTEIL;
}
