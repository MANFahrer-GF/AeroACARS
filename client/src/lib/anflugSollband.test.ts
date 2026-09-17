import { describe, expect, it } from "vitest";
import {
  gleitwinkelFaktor,
  sollband,
  sollbandPfad,
  sollbandStuecke,
} from "./anflugSollband";

describe("anflugSollband", () => {
  it("rechnet wie die Bewertung: −GS × 5,31 auf 3°", () => {
    // Thorben 16.09.2026, PC-12: 85 kt GS → Soll −451 fpm, geflogen −450.
    const [p] = sollband([{ gs_kt: 85, t_ms: -20_000, is_flare: false }]);
    expect(p!.soll).toBeCloseTo(-451.35, 2);
    expect(p!.oben).toBeCloseTo(-351.35, 2);
    expect(p!.unten).toBeCloseTo(-551.35, 2);
  });

  it("ein Airliner mit 140 kt landet im alten −600…−900-Bereich", () => {
    const [p] = sollband([{ gs_kt: 140 }]);
    expect(p!.soll).toBeCloseTo(-743.4, 1);
  });

  it("skaliert mit dem Gleitwinkel wie das Backend (Clamp 2–7,5°)", () => {
    expect(gleitwinkelFaktor(null)).toBe(1);
    expect(gleitwinkelFaktor(3)).toBeCloseTo(1, 10);
    expect(gleitwinkelFaktor(1.5)).toBe(1);
    expect(gleitwinkelFaktor(8)).toBe(1);
    const [p] = sollband([{ gs_kt: 100 }], 5.5);
    expect(p!.soll).toBeCloseTo(-531 * gleitwinkelFaktor(5.5), 6);
  });

  it("erfindet kein Soll ohne Geschwindigkeit, im Flare, nach TD oder beim Rollen", () => {
    expect(sollband([{ gs_kt: null }, {}])).toEqual([]);
    expect(sollband([{ gs_kt: 90, is_flare: true }])).toEqual([]);
    expect(sollband([{ gs_kt: 90, t_ms: 500 }])).toEqual([]);
    expect(sollband([{ gs_kt: 12 }])).toEqual([]);
  });

  it("trennt das Band an Lücken und schließt jede Fläche", () => {
    const punkte = sollband([
      { gs_kt: 100 },
      { gs_kt: 100 },
      { gs_kt: 100, is_flare: true },
      { gs_kt: 100 },
    ]);
    const stuecke = sollbandStuecke(punkte);
    expect(stuecke.map((s) => s.map((p) => p.index))).toEqual([[0, 1], [3]]);
    const d = sollbandPfad(stuecke[0]!, (i) => i * 10, (v) => -v);
    expect(d.startsWith("M 0.0 ")).toBe(true);
    expect(d.endsWith("Z")).toBe(true);
  });
});
