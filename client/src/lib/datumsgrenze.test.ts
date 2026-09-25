import { describe, expect, it } from "vitest";
import {
  laengeNebenVorgaenger,
  ohneDatumsgrenzenSprung,
  punkteAufKleinstemBogen,
  umrissOhneNaht,
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

  it("kaputter Punkt verdirbt nicht den Rest der Linie", () => {
    expect(
      ohneDatumsgrenzenSprung([
        [170, 0],
        [Number.NaN, 0],
        [175, 0],
        [-179, 0],
      ]),
    ).toEqual([
      [170, 0],
      [175, 0],
      [181, 0],
    ]);
  });

  it("erster Punkt ausserhalb ±180 wird zurueckgeholt", () => {
    expect(
      ohneDatumsgrenzenSprung([
        [540, 0],
        [-179, 0],
      ]),
    ).toEqual([
      [180, 0],
      [181, 0],
    ]);
  });

  it("Umriss laesst nur die Schnittkante bei ±180° weg", () => {
    // Anchorage-artig: westliches Stueck bis +180, oestliches ab −180.
    const fc: GeoJSON.FeatureCollection = {
      type: "FeatureCollection",
      features: [
        {
          type: "Feature",
          properties: { farbe: "#f00" },
          geometry: {
            type: "MultiPolygon",
            coordinates: [
              [
                [
                  [170, 50],
                  [180, 50],
                  [180, 60],
                  [170, 60],
                  [170, 50],
                ],
              ],
              [
                [
                  [-180, 50],
                  [-170, 50],
                  [-170, 60],
                  [-180, 60],
                  [-180, 50],
                ],
              ],
            ],
          },
        },
      ],
    };
    const u = umrissOhneNaht(fc);
    expect(u.features).toHaveLength(1);
    expect(u.features[0].properties).toEqual({ farbe: "#f00" });
    const kanten = u.features[0].geometry.coordinates.flatMap((zug) =>
      zug.slice(1).map((p, i) => [zug[i], p]),
    );
    // 2 Ringe x 4 Kanten = 8, davon je eine auf der Datumsgrenze.
    expect(kanten).toHaveLength(6);
    for (const [a, b] of kanten) {
      expect(Math.abs(a[0]) === 180 && Math.abs(b[0]) === 180).toBe(false);
    }
  });

  it("Umriss eines gewoehnlichen Sektors bleibt vollstaendig", () => {
    const ring = [
      [5, 47],
      [15, 47],
      [15, 55],
      [5, 55],
      [5, 47],
    ];
    const u = umrissOhneNaht({
      type: "FeatureCollection",
      features: [
        {
          type: "Feature",
          properties: {},
          geometry: { type: "Polygon", coordinates: [ring] },
        },
      ],
    });
    expect(u.features[0].geometry.coordinates).toEqual([ring]);
  });

  it("Umriss laesst auch eine Kante von +180 direkt nach −180 weg", () => {
    // Kommt in den heutigen VATSpy-Daten nicht vor; die Kante liefe sonst
    // einmal quer ueber die Welt (Claude-QS 25.09.2026).
    const u = umrissOhneNaht({
      type: "FeatureCollection",
      features: [
        {
          type: "Feature",
          properties: {},
          geometry: {
            type: "Polygon",
            coordinates: [
              [
                [170, 50],
                [180, 50],
                [-180, 60],
                [170, 60],
                [170, 50],
              ],
            ],
          },
        },
      ],
    });
    const kanten = u.features[0]!.geometry.coordinates.flatMap((zug) =>
      zug.slice(1).map((p, i) => [zug[i]!, p] as const),
    );
    expect(kanten).toHaveLength(3);
    for (const [a, b] of kanten) {
      expect(Math.abs(a[0]!) === 180 && Math.abs(b[0]!) === 180).toBe(false);
    }
  });
});
