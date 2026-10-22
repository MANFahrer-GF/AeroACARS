// PROTOTYP (proto/bahnansicht, 22.10.2026) — Querachse für die Varianten
// der Bahnansicht.
//
// Derselbe Querschnitt-Massstab wie in `RunwayCrossSection` (REFERENZ 60 m
// füllen 264 Einheiten, mindestens 120), hier als Funktion, damit Draufsicht
// und Queransicht DIESELBE Querachse benutzen können (Spec §8.4 verlangt das
// für die Längsachse; in der Draufsicht stand bis hier ein fester 45-m-
// Platzhalter: `widthM = 45`, ±22,5 m auf ±85 px, egal wie breit die Bahn ist).
//
// Für ein Produkt würde `RunwayCrossSection` auf diese Funktion umgestellt;
// im Prototyp bleibt die Queransicht unverändert, die Werte sind gleich.

export const REFERENZ_BREITE_M = 60;
export const MAX_BAHN_H = 264;
export const MIN_BAHN_H = 120;

export interface QuerMassstab {
  /** Gezeichnete Höhe der befestigten Fläche in Einheiten. */
  bahnH: number;
  /** Einheiten je Meter quer. */
  pxProQuerM: number;
  /** Faktor, um den eine sehr schmale Bahn aufgezogen wurde (1 = keiner). */
  dehnung: number;
}

export function querMassstab(breiteM: number): QuerMassstab {
  const roh = (breiteM / REFERENZ_BREITE_M) * MAX_BAHN_H;
  const bahnH = Math.min(MAX_BAHN_H, Math.max(MIN_BAHN_H, roh));
  return { bahnH, pxProQuerM: bahnH / breiteM, dehnung: bahnH / roh };
}

/**
 * Halbe Breite bis zur Reifen-Aussenkante — Spiegel von
 * `aussenkante_halb_aus_spur` (landing-scoring/src/spurweite.rs). Genau
 * diese Kante bewertet der Client (Randabstand = halbe Bahn − |Versatz| −
 * dieser Wert). Die heutige Queransicht zeichnet ihr Band nur bis zur
 * Radmitte (Spurweite / 2).
 */
export function aussenkanteHalbM(spurweiteM: number): number {
  const paket = spurweiteM < 4 ? 0.3 : spurweiteM <= 8 ? 0.9 : 1.1;
  return spurweiteM / 2 + paket / 2;
}

/** Ende des bewerteten Teils: das früheste der gespeicherten Enden. */
export function bewertungsEnde(...enden: Array<number | null | undefined>): number | null {
  const da = enden.filter((e): e is number => e != null && Number.isFinite(e));
  return da.length ? Math.min(...da) : null;
}

/** Randabstand des äusseren Rades je Probe — wie der Client ihn rechnet. */
export function randabstandM(querM: number, halbeBahnM: number, spurweiteM: number): number {
  return halbeBahnM - (Math.abs(querM) + aussenkanteHalbM(spurweiteM));
}

export interface Randstelle {
  laengs_m: number;
  quer_m: number;
  randabstand_m: number;
  /** Erste und letzte Probe mit negativem Randabstand (Bahn verlassen). */
  draussenVon: number | null;
  draussenBis: number | null;
}

/**
 * Die schlechteste Stelle im bewerteten Teil (bis `bisM`, Spur-Nullpunkt).
 * Rechnet nichts neu, was gespeichert ist — sie SUCHT die Stelle, an der
 * der gespeicherte Wert entstand, damit das Bild sie zeigen kann.
 */
export function randstelle(
  samples: Array<{ laengs_m: number; quer_m: number }>,
  halbeBahnM: number,
  spurweiteM: number,
  bisM: number | null,
  /**
   * Der vom Client gespeicherte kleinste Randabstand (`min_edge_clearance_m`).
   * Liegt er vor, ist ER die Zahl (eine Quelle, der Client rechnet); gesucht
   * wird nur die Probe, an der er entstand.
   */
  gespeichertM?: number | null,
): Randstelle | null {
  const imFenster = samples.filter((s) => bisM == null || s.laengs_m <= bisM);
  if (!imFenster.length) return null;
  let schlechteste = imFenster[0]!;
  let wert = randabstandM(schlechteste.quer_m, halbeBahnM, spurweiteM);
  let von: number | null = null;
  let bis: number | null = null;
  for (const s of imFenster) {
    const r = randabstandM(s.quer_m, halbeBahnM, spurweiteM);
    if (r < wert) {
      wert = r;
      schlechteste = s;
    }
    if (r < 0) {
      if (von == null) von = s.laengs_m;
      bis = s.laengs_m;
    }
  }
  if (gespeichertM != null && Number.isFinite(gespeichertM)) {
    schlechteste = imFenster.reduce((a, b) =>
      Math.abs(randabstandM(b.quer_m, halbeBahnM, spurweiteM) - gespeichertM) <
      Math.abs(randabstandM(a.quer_m, halbeBahnM, spurweiteM) - gespeichertM)
        ? b
        : a,
    );
    wert = gespeichertM;
  }
  return {
    laengs_m: schlechteste.laengs_m,
    quer_m: schlechteste.quer_m,
    randabstand_m: wert,
    draussenVon: von,
    draussenBis: bis,
  };
}
