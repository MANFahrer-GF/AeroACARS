// Der Hinweis auf gemischte Bewertungs-Stände.
//
// **Warum es diesen Test gibt.** Die Funktion dahinter ist eine
// Sichtbarkeits-Regel, keine Rechnung — genau die Sorte, die still kaputt
// geht: sie erscheint entweder gar nicht (dann steht der Sprung in der
// Kurve unerklärt da) oder für immer (dann ist sie Lärm). Beides sieht man
// im Alltag nicht, weil man dafür einen Datenbestand mit genau der
// richtigen Mischung braucht.
import { describe, expect, it } from "vitest";
import { gemischteBewertungsstaende } from "./LandingPanel";

type R = { score_algorithm_version?: number | null };
const mach = (...versionen: (number | null)[]): R[] =>
  versionen.map((v) => ({ score_algorithm_version: v }));

describe("Hinweis auf gemischte Bewertungs-Stände", () => {
  it("erscheint, wenn alt und neu nebeneinander in der Kurve stehen", () => {
    expect(gemischteBewertungsstaende(mach(7, 7, 6, 6) as never)).toBe(true);
  });

  it("bleibt aus, wenn alles vom selben Stand ist", () => {
    expect(gemischteBewertungsstaende(mach(7, 7, 7) as never)).toBe(false);
    expect(gemischteBewertungsstaende(mach(6, 6, 6) as never)).toBe(false);
  });

  it("bleibt aus, wenn gar keine Version vermerkt ist", () => {
    expect(gemischteBewertungsstaende(mach(null, null) as never)).toBe(false);
  });

  it("bleibt aus bei leerer Liste und bei einer einzelnen Landung", () => {
    expect(gemischteBewertungsstaende([] as never)).toBe(false);
    expect(gemischteBewertungsstaende(mach(7) as never)).toBe(false);
  });

  it("verschwindet, sobald zwölf neue Landungen die Kurve füllen", () => {
    // Die entscheidende Eigenschaft: der Hinweis ist an den SICHTBAREN
    // Bereich gebunden, nicht an den gesamten Bestand. Sonst stünde er
    // dauerhaft da, solange irgendwo noch ein alter Flug liegt.
    const zwoelfNeue = mach(...Array(12).fill(7));
    const altDahinter = mach(...Array(50).fill(6));
    expect(
      gemischteBewertungsstaende([...zwoelfNeue, ...altDahinter] as never),
    ).toBe(false);
    // Eine neue weniger — und er ist wieder da.
    expect(
      gemischteBewertungsstaende([
        ...mach(...Array(11).fill(7)),
        ...altDahinter,
      ] as never),
    ).toBe(true);
  });
});
