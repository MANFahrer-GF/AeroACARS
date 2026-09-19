// Lupe „Abrollen im echten Massstab" — was sie versprechen muss.
//
// Die ganze Ansicht hängt an EINER Aussage: längs und quer gleicher
// Massstab, Winkel wie gefahren. Die überhöhte Queransicht zeichnete eine
// 30°-Ausfahrt als fast rechten Winkel; das hier darf das nie wieder.

import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { RunwayExitLupe, type LupeProps } from "./RunwayExitLupe";

const TOKENS = { tarmac: "#1a2230", rollout: "#22d3ee", rollweg: "#3b82f6", rollwegRand: "#7dd3fc" };

/** Eine Spur, die bis 1800 m geradeaus läuft und dann mit 30° links abgeht. */
function spur30(): Array<{ laengs_m: number; quer_m: number }> {
  const s: Array<{ laengs_m: number; quer_m: number }> = [];
  for (let m = 600; m <= 1800; m += 12) s.push({ laengs_m: m, quer_m: 0.5 });
  const tan = Math.tan((30 * Math.PI) / 180);
  for (let d = 10; d <= 160; d += 10) s.push({ laengs_m: 1800 + d, quer_m: 0.5 - d * tan });
  return s;
}

const basis: LupeProps = {
  samples: spur30(),
  runwayWidthM: 45,
  trackWidthM: 7.6,
  clearanceM: 1839,
  clearanceSide: "left",
  ausfahrten: [
    {
      name: "L6",
      laengs_m: 1840,
      seite: "left",
      verlauf: [
        { laengs_m: 1800, quer_m: 0 },
        { laengs_m: 1990, quer_m: -110 },
      ],
    },
  ],
  bandFarbe: "#22c55e",
  width: 1200,
  tokens: TOKENS,
};

/** Alle Kreise der Messpunkte (r=2) als Bildkoordinaten. */
function punkte(svg: string): Array<{ x: number; y: number }> {
  return [...svg.matchAll(/<circle cx="([-\d.]+)" cy="([-\d.]+)" r="2"/g)].map((m) => ({
    x: Number(m[1]),
    y: Number(m[2]),
  }));
}

describe("Lupe — Abrollen im echten Massstab", () => {
  it("zeichnet eine 30°-Ausfahrt mit 30°, nicht überhöht", () => {
    const svg = renderToStaticMarkup(<RunwayExitLupe {...basis} />);
    const p = punkte(svg);
    const a = p[p.length - 8]!;
    const b = p[p.length - 1]!;
    const grad = (Math.atan2(Math.abs(b.y - a.y), b.x - a.x) * 180) / Math.PI;
    expect(grad).toBeGreaterThan(28);
    expect(grad).toBeLessThan(32);
  });

  it("zeichnet einen Rollweg aus zwei weit auseinander liegenden OSM-Punkten", () => {
    // L6 läuft von 1800 m bis 1990 m; die Lupe endet bei 1949 m. Punkte zu
    // filtern ließ einen übrig, der Rollweg fehlte und die Lupe behauptete,
    // es gebe keine Rollwegdaten. Geschnitten bleibt das Stück im Bild.
    const svg = renderToStaticMarkup(<RunwayExitLupe {...basis} />);
    expect(svg.match(/stroke-dasharray="6 5"/g)?.length ?? 0).toBe(1);
  });

  it("gibt ohne Abrollen nichts aus", () => {
    // Keine Räumung, Spur bleibt auf der Bahn — es gibt keine Ausfahrt zu zeigen.
    const svg = renderToStaticMarkup(
      <RunwayExitLupe
        {...basis}
        clearanceM={null}
        samples={basis.samples.filter((s) => Math.abs(s.quer_m) < 20)}
      />,
    );
    expect(svg).toBe("");
  });

  it("setzt die Namen dicht beieinander liegender Rollwege nicht übereinander", () => {
    // EDDF 25L (#1426): M17, M19 und M21 liegen wenige Meter auseinander.
    const dicht = ["M17", "M19", "M21"].map((name, i) => ({
      name,
      laengs_m: 1830 + i * 4,
      seite: "left" as const,
      verlauf: null,
    }));
    const svg = renderToStaticMarkup(<RunwayExitLupe {...basis} ausfahrten={dicht} />);
    const xs = [...svg.matchAll(/<text x="([-\d.]+)"[^>]*>(M\d+)<\/text>/g)].map((m) => Number(m[1]));
    for (let i = 0; i < xs.length; i++) {
      for (let j = i + 1; j < xs.length; j++) {
        expect(Math.abs(xs[i]! - xs[j]!), "zwei Namen übereinander").toBeGreaterThan(20);
      }
    }
    expect(xs.length).toBeGreaterThan(0);
  });
});
