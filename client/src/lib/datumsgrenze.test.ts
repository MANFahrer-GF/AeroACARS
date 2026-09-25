import { describe, expect, it } from "vitest";
import { laengeNebenVorgaenger, ohneDatumsgrenzenSprung } from "./datumsgrenze";

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
});
