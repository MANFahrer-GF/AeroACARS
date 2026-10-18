// Punkt 6 (Thomas 18.10.2026): Untertitel statt Textwand. Mit den Satzzeiten
// des Ton-Servers genau, ohne sie geschätzt nach der Länge der Sätze.
import { describe, expect, it } from "vitest";
import { satzIndex, saetzeVon, stueckeVon } from "./KabinenSeite";

const teil = (text: string, dauer_s: number, stuecke?: { ab_s: number; text: string }[]) => ({ sprache: "de-DE", text, datei: "f/a.wav", dauer_s, stuecke });

describe("Untertitel", () => {
  it("nimmt die Satzzeiten des Ton-Servers", () => {
    const t = teil("Guten Tag. Willkommen an Bord.", 4, [
      { ab_s: 0, text: "Guten Tag." },
      { ab_s: 1.2, text: "Willkommen an Bord." },
    ]);
    expect(satzIndex(t, 1.1)).toBe(0);
    expect(satzIndex(t, 1.2)).toBe(1);
    expect(stueckeVon(t).map((x) => x.text)).toEqual(["Guten Tag.", "Willkommen an Bord."]);
  });

  it("schätzt ohne Satzzeiten nach der Länge der Sätze", () => {
    // Zweiter Satz dreimal so lang wie der erste: er beginnt nach einem Viertel.
    const t = teil("Kurz eins. Viel viel viel länger zwei.", 8);
    expect(saetzeVon(t.text)).toHaveLength(2);
    expect(satzIndex(t, 1.5)).toBe(0);
    expect(satzIndex(t, 2.6)).toBe(1);
    expect(satzIndex(t, 100)).toBe(1);
  });

  it("ein Satz ohne Satzzeichen bleibt ein Stück", () => {
    expect(stueckeVon(teil("Cabin crew, seats for take-off", 2))).toHaveLength(1);
  });
});
