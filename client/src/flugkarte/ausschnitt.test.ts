import { describe, expect, it, vi } from "vitest";
const h = vi.hoisted(() => ({ webgl: false, laden: null as null | (() => Promise<void>), marken: 0, spruenge: 0 }));
vi.mock("maplibre-gl", () => ({
  default: {
    Map: class {
      constructor() {
        if (!h.webgl) throw new Error("kein WebGL");
        return new Proxy(this, { get: (_z, k) => {
          if (k === "on") return (art: string, f: () => Promise<void>) => { if (art === "load") h.laden = f; };
          if (k === "jumpTo") return () => { h.spruenge++; };
          if (k === "getSource") return () => undefined;
          return () => undefined;
        } });
      }
    },
    AttributionControl: class {},
    Marker: class { constructor() { h.marken++; } setLngLat() { return this; } addTo() { return this; } remove() {} },
    LngLatBounds: class {},
  },
}));
vi.mock("maplibre-gl/dist/maplibre-gl.css", () => ({}));
const vorschau = vi.hoisted(() => ({ marke: vi.fn((..._a: unknown[]) => vi.fn()) }));
vi.mock("./markenVorschau", () => ({ markeMitVorschau: vorschau.marke }));
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
  it("dieselbe Eingabe aus dem Tablet-Takt baut nichts neu und springt nicht zurueck; eine geaenderte schon", async () => {
    h.webgl = true;
    vi.stubGlobal("fetch", async () => { throw new Error("offline"); });
    const k = bildKarteAnzeigen(document.createElement("div"), { stilUrl: "x" });
    await h.laden!();
    const eingabe = () => ({ spur: null, marken: [{ id: "a", name: "A", lat: 50, lon: 8, aktiv: true }], fokus: { lat: 50, lon: 8 }, kugel: false });
    k.aktualisieren(eingabe());
    expect([h.marken, h.spruenge]).toEqual([1, 1]);
    k.aktualisieren(eingabe());
    expect([h.marken, h.spruenge]).toEqual([1, 1]);
    k.aktualisieren({ ...eingabe(), kugel: true });
    expect([h.marken, h.spruenge]).toEqual([2, 2]);
    h.webgl = false;
    vi.unstubAllGlobals();
  });
  it("Vorschau: je Marke das eigene Bild; neu gezeichnete Marken und das Aufraeumen schliessen die alte", async () => {
    h.webgl = true;
    vi.stubGlobal("fetch", async () => { throw new Error("offline"); });
    vorschau.marke.mockClear();
    const bildVorschau = vi.fn(() => ({ url: Promise.resolve("blob:v"), zeilen: [] }));
    const k = bildKarteAnzeigen(document.createElement("div"), { stilUrl: "x", bildVorschau });
    await h.laden!();
    const eingabe = { spur: null, marken: [{ id: "a", name: "A", lat: 50, lon: 8, aktiv: true }], fokus: { lat: 50, lon: 8 }, kugel: false };
    k.aktualisieren(eingabe);
    const [, , name, adresse] = vorschau.marke.mock.calls[0];
    expect(name).toBe("A");
    (adresse as () => unknown)();
    expect(bildVorschau).toHaveBeenCalledWith("a");
    k.aktualisieren({ ...eingabe, kugel: true });
    expect(vorschau.marke.mock.results[0].value).toHaveBeenCalledTimes(1);
    k.aufraeumen();
    expect(vorschau.marke.mock.results[1].value).toHaveBeenCalledTimes(1);
    h.webgl = false;
    vi.unstubAllGlobals();
  });
});
