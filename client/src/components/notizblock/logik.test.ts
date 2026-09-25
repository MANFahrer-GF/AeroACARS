import { describe, expect, it } from "vitest";
import { Verlauf, kompakt, laden, pixelBreite, trifft, type Strich } from "./logik";

const strich = (punkte: Array<[number, number]>): Strich => ({
  farbe: "--text",
  breite: 0.003,
  punkte: punkte.map(([x, y]) => ({ x, y, p: 0.5 })),
});

describe("Notizblock-Logik", () => {
  it("Radierer trifft einen Strich nahe der Linie, nicht daneben", () => {
    const s = strich([
      [0.1, 0.5],
      [0.9, 0.5],
    ]);
    expect(trifft(s, 0.5, 0.505, 0.01, 1)).toBe(true);
    expect(trifft(s, 0.5, 0.6, 0.01, 1)).toBe(false);
    // Hochformat: y zaehlt in Breiteneinheiten doppelt.
    expect(trifft(s, 0.5, 0.507, 0.01, 2)).toBe(false);
  });

  it("Andruck macht den Strich dicker", () => {
    const s = strich([[0, 0]]);
    expect(pixelBreite(s, 1, 1000)).toBeGreaterThan(pixelBreite(s, 0.1, 1000));
    expect(pixelBreite(s, 0, 1000)).toBeGreaterThanOrEqual(0.8);
  });

  it("Rückgängig liefert die Zustände in umgekehrter Reihenfolge", () => {
    const v = new Verlauf();
    expect(v.leer).toBe(true);
    const a: Strich[] = [];
    const b = [strich([[0, 0]])];
    v.merken(a);
    v.merken(b);
    expect(v.zurueck()).toBe(b);
    expect(v.zurueck()).toBe(a);
    expect(v.zurueck()).toBeNull();
  });

  it("kaputter Speicher ergibt eine leere Fläche", () => {
    expect(laden(null)).toEqual([]);
    expect(laden("kein json")).toEqual([]);
    expect(laden('{"a":1}')).toEqual([]);
    expect(laden('[{"farbe":1}]')).toEqual([]);
    const s = [strich([[0.1, 0.2]])];
    expect(laden(JSON.stringify(s))).toEqual(s);
  });

  it("kompakt rundet, ohne Striche zu verlieren", () => {
    const s = [{ farbe: "--acc", breite: 0.003, punkte: [{ x: 0.123456789, y: 0.987654321, p: 0.33333 }] }];
    expect(kompakt(s)[0].punkte[0]).toEqual({ x: 0.1235, y: 0.9877, p: 0.33 });
  });
});
