// Soll-Sinkrate im Anflug — dieselbe Rechnung wie die Bewertung.
//
// # Warum es diese Datei gibt
//
// Die Anflug-Grafik zeichnete ein festes grünes Band −600…−900 fpm. Das ist
// die Sinkrate eines Verkehrsflugzeugs mit 115–170 kt über Grund auf 3°.
// Die Bewertung dagegen (`compute_approach_stability_v2`, Kachel „V/S vs.
// 3°-ILS") rechnet seit jeher mit der echten Geschwindigkeit über Grund:
// Soll = −GS × 5,31 × tan(Gleitwinkel)/tan(3°). Grafik und Note sagten also
// Verschiedenes. Pilot-Befund Thorben 16.09.2026 (PC-12, 85–100 kt GS):
// exakt auf dem 3°-Pfad geflogen (−450 bis −540 fpm) und trotzdem immer
// „über der grünen Linie" — die Linie galt für ein anderes Flugzeug.
//
// # Kanonisch
//
// Diese Datei liegt kanonisch im Client (`client/src/lib`) und wird per
// `scripts/anzeige-sync.mjs` in die Webapp kopiert. Client-Grafik
// (LandingPanel) und Webapp-Grafik (_ApproachChart) benutzen beide nur sie.

/** fpm Sinkrate je kt Geschwindigkeit über Grund auf einem 3°-Pfad —
 *  identisch zum Backend (`target_vs = -gs_kt * 5.31 * gs_factor`). */
export const FPM_JE_KT_3_GRAD = 5.31;

/** Breite des Bandes nach oben und unten. Entspricht der grünen Schwelle
 *  der Kachel „V/S vs. 3°-ILS" (Abweichung < 100 fpm = gut). */
export const SOLLBAND_TOLERANZ_FPM = 100;

/** Unter dieser Geschwindigkeit über Grund zeichnen wir kein Soll: Rollen,
 *  Stillstand, Sim-Aussetzer. Kein Flugzeug fliegt einen Anflug so langsam. */
const MIN_GS_KT = 30;

/**
 * Faktor für einen vom Standard abweichenden Gleitwinkel — 1:1 das Backend
 * (`gs_factor = tan(g)/tan(3°)`, Plausibilitäts-Clamp 2–7,5°, sonst 1,0).
 */
export function gleitwinkelFaktor(winkelGrad?: number | null): number {
  if (winkelGrad == null || !Number.isFinite(winkelGrad)) return 1;
  if (winkelGrad < 2 || winkelGrad > 7.5) return 1;
  return Math.tan((winkelGrad * Math.PI) / 180) / Math.tan((3 * Math.PI) / 180);
}

export interface SollbandProbe {
  gs_kt?: number | null;
  is_flare?: boolean | null;
  t_ms?: number | null;
}

export interface SollbandPunkt {
  /** Index in die Probenliste der Grafik. */
  index: number;
  /** Soll-Sinkrate in fpm (negativ = sinken). */
  soll: number;
  /** Obere (flachere) Kante des Bandes. */
  oben: number;
  /** Untere (steilere) Kante des Bandes. */
  unten: number;
}

/**
 * Soll-Band je Probe. Leer, wenn keine Probe eine Geschwindigkeit trägt —
 * dann hat der Flug diese Daten nicht (Client-Aufzeichnung vor dieser
 * Änderung), und die Grafik darf kein Soll erfinden.
 *
 * Nur Proben VOR dem Aufsetzen und außerhalb des Flares: im Abfangen ist
 * „Sinkrate wie auf dem Gleitpfad" gerade nicht das Ziel.
 */
export function sollband(
  proben: readonly SollbandProbe[],
  winkelGrad?: number | null,
): SollbandPunkt[] {
  const faktor = gleitwinkelFaktor(winkelGrad);
  const punkte: SollbandPunkt[] = [];
  proben.forEach((p, index) => {
    const gs = p.gs_kt;
    if (gs == null || !Number.isFinite(gs) || gs < MIN_GS_KT) return;
    if (p.is_flare) return;
    if (p.t_ms != null && p.t_ms > 0) return;
    const soll = -gs * FPM_JE_KT_3_GRAD * faktor;
    punkte.push({
      index,
      soll,
      oben: soll + SOLLBAND_TOLERANZ_FPM,
      unten: soll - SOLLBAND_TOLERANZ_FPM,
    });
  });
  return punkte;
}

/**
 * Zerlegt die Bandpunkte in zusammenhängende Stücke (lückenlose Indizes),
 * damit die Grafik über eine Lücke — etwa den Flare — keine Fläche zieht.
 */
export function sollbandStuecke(punkte: readonly SollbandPunkt[]): SollbandPunkt[][] {
  const stuecke: SollbandPunkt[][] = [];
  for (const p of punkte) {
    const letztes = stuecke[stuecke.length - 1];
    if (letztes && letztes[letztes.length - 1]!.index === p.index - 1) {
      letztes.push(p);
    } else {
      stuecke.push([p]);
    }
  }
  return stuecke;
}

/** SVG-Pfad einer Bandfläche: obere Kante vorwärts, untere rückwärts. */
export function sollbandPfad(
  stueck: readonly SollbandPunkt[],
  x: (index: number) => number,
  y: (fpm: number) => number,
): string {
  if (stueck.length === 0) return "";
  const vor = stueck.map((p, i) => `${i === 0 ? "M" : "L"} ${x(p.index).toFixed(1)} ${y(p.oben).toFixed(1)}`);
  const zurueck = [...stueck]
    .reverse()
    .map((p) => `L ${x(p.index).toFixed(1)} ${y(p.unten).toFixed(1)}`);
  return `${vor.join(" ")} ${zurueck.join(" ")} Z`;
}
