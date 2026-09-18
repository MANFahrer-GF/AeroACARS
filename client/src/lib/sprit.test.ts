/**
 * v1.7.35 — der Farbton der Sprit-Phasen.
 *
 * Diese Zusicherungen halten fest, was das Release ausmacht: Der Sprit wird
 * gezeigt, nicht angeklagt. Ohne sie könnte jemand die Warnfarbe
 * zurückbringen, ohne dass ein Test es merkt.
 */
import { describe, expect, it } from "vitest";
import { phaseTon } from "./sprit";
import type { SpritPhase } from "./sprit";

const phase = (pct: number): SpritPhase => ({
  ist_kg: 1000,
  plan_kg: 1000,
  abweichung_pct: pct,
  als_kg: false,
});

describe("phaseTon", () => {
  it("gibt für sparsame Flüge die ruhige Farbe — bis Sinkflug", () => {
    expect(phaseTon(phase(-8.5), "bis_sinkflug")).toBe("ok");
    expect(phaseTon(phase(3), "bis_sinkflug")).toBe("ok");
  });

  it("warnt NIE — auch nicht bei extremem Mehrverbrauch", () => {
    // Die Achse ist aus der Note geflogen, weil sie die Flugsicherung
    // benotete. Eine Warnfarbe wäre derselbe Vorwurf in Rot.
    for (const p of [3.1, 20, 47, 192.8, 342.2]) {
      expect(phaseTon(phase(p), "bis_sinkflug")).toBe("neutral");
      expect(phaseTon(phase(p), "anflug")).toBe("neutral");
    }
  });

  it("färbt den Anflug auch bei Sparsamkeit nicht ein", () => {
    // Der Anflug misst überwiegend die Radarführung — in beide Richtungen
    // ist das keine Aussage über den Piloten.
    expect(phaseTon(phase(-67.6), "anflug")).toBe("neutral");
    expect(phaseTon(phase(-10), "anflug")).toBe("neutral");
  });

  it("gibt ohne Phase neutral zurück", () => {
    expect(phaseTon(null)).toBe("neutral");
    expect(phaseTon(null, "anflug")).toBe("neutral");
  });

  it("behandelt eine Phase ohne Angabe der Art wie „bis Sinkflug“", () => {
    expect(phaseTon(phase(-5))).toBe("ok");
    expect(phaseTon(phase(50))).toBe("neutral");
  });
});
