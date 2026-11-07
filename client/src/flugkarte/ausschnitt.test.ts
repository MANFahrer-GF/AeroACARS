import { describe, expect, it, vi } from "vitest";
vi.mock("maplibre-gl", () => ({
  default: { Map: class { constructor() { throw new Error("kein WebGL"); } }, AttributionControl: class {}, Marker: class {}, LngLatBounds: class {} },
}));
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));
import { bildKarteAnzeigen, spurGeoJson } from "./ausschnitt";

describe("Kartenausschnitt", () => {
  it("Spur ueber die Datumsgrenze bleibt fortlaufend, Reihenfolge [lon, lat]", () => {
    const f = spurGeoJson([[10, 179], [10, -179], [11, -178]]);
    expect(f.geometry.coordinates).toEqual([[179, 10], [181, 10], [182, 11]]);
    expect(spurGeoJson(null).geometry.coordinates).toEqual([]);
  });
  it("ohne WebGL stuerzt nichts ab: Fehlermeldung an die Oberflaeche, Steuerung bleibt aufrufbar", () => {
    const fehler = vi.fn();
    const k = bildKarteAnzeigen(document.createElement("div"), { stilUrl: "x", beiFehler: fehler });
    expect(fehler).toHaveBeenCalledTimes(1);
    expect(() => { k.aktualisieren({ spur: null, marken: [], fokus: null, kugel: true }); k.aufraeumen(); }).not.toThrow();
  });
});
