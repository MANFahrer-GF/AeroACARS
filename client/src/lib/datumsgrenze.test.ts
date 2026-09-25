import { describe, expect, it } from "vitest";
import {
  laengeNebenVorgaenger,
  ohneDatumsgrenzenSprung,
  punkteAufKleinstemBogen,
} from "./datumsgrenze";

describe("Datumsgrenze", () => {
  it("westwaerts ueber 180°: weiterzaehlen statt um die Welt", () => {
    expect(
      ohneDatumsgrenzenSprung([
        [178, 52],
        [179.5, 52.5],
        [-179.5, 53],
        [-178, 53],
      ]),
    ).toEqual([
      [178, 52],
      [179.5, 52.5],
      [180.5, 53],
      [182, 53],
    ]);
  });

  it("ostwaerts ueber 180°", () => {
    expect(
      ohneDatumsgrenzenSprung([
        [-178, 52],
        [179, 52],
      ]),
    ).toEqual([
      [-178, 52],
      [-181, 52],
    ]);
  });

  it("gewoehnliche Route bleibt unveraendert", () => {
    const route: [number, number][] = [
      [8.57, 50.03],
      [-0.46, 51.47],
      [-73.78, 40.64],
    ];
    expect(ohneDatumsgrenzenSprung(route)).toEqual(route);
  });

  it("Nachbarlage", () => {
    expect(laengeNebenVorgaenger(-179, 179)).toBe(181);
    expect(laengeNebenVorgaenger(10, 9)).toBe(10);
    expect(laengeNebenVorgaenger(179, -179)).toBe(-181);
  });

  it("Punktmenge beiderseits der Datumsgrenze: kleiner Bogen", () => {
    const bogen = punkteAufKleinstemBogen([
      [-179, 50],
      [179, 52],
      [-175, 55],
    ]);
    const laengen = bogen.map((p) => p[0]);
    expect(Math.max(...laengen) - Math.min(...laengen)).toBe(6);
    expect(bogen).toEqual([
      [181, 50],
      [179, 52],
      [185, 55],
    ]);
  });

  it("Punktmenge in Europa bleibt, wie sie ist", () => {
    const pts: [number, number][] = [
      [8.5, 50],
      [-0.5, 51.5],
      [16.5, 48],
    ];
    expect(punkteAufKleinstemBogen(pts)).toEqual(pts);
  });

  it("Punktmenge ueber die halbe Welt nimmt den kuerzeren Weg", () => {
    // Frankfurt und Tokio: 131° ostwaerts ist kuerzer als 229° westwaerts.
    const bogen = punkteAufKleinstemBogen([
      [8.5, 50],
      [139.8, 35.5],
    ]);
    expect(bogen).toEqual([
      [8.5, 50],
      [139.8, 35.5],
    ]);
  });
});
