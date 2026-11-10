import { describe, expect, it } from "vitest";
import { beladungFuerPlan, type BeladungsKlasse } from "./vfrBeladung";

const klassen: BeladungsKlasse[] = [
  { id: 6, code: "Y", name: "Economy", typ: 0, kapazitaet: 4 },
  { id: 7, code: "CGO", name: "Cargo", typ: 1, kapazitaet: 150 },
];

describe("beladungFuerPlan", () => {
  it("übernimmt eingetragene Werte", () => {
    expect(beladungFuerPlan(klassen, { 6: "3", 7: "40" })).toEqual([
      { fare_id: 6, anzahl: 3 },
      { fare_id: 7, anzahl: 40 },
    ]);
  });

  it("schickt leere Felder als 0, damit der PIREP Tarifzeilen trägt", () => {
    expect(beladungFuerPlan(klassen, {})).toEqual([
      { fare_id: 6, anzahl: 0 },
      { fare_id: 7, anzahl: 0 },
    ]);
  });

  it("hält die Grenzen des Flugzeugs ein", () => {
    expect(beladungFuerPlan(klassen, { 6: "9", 7: "-5" })).toEqual([
      { fare_id: 6, anzahl: 4 },
      { fare_id: 7, anzahl: 0 },
    ]);
  });
});
