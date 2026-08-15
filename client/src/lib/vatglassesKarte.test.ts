// Der Client holt die Sektoren beim Live-Server statt selbst zu rechnen.
// Geprüft wird, was dabei schiefgehen kann: Server weg, Antwort halb,
// Abbruch beim Kartenwechsel.
import { describe, expect, it, vi, afterEach } from "vitest";
import { ladeSektoren } from "./vatglassesKarte";

const BASIS = "https://test.invalid";
const echtesFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = echtesFetch; vi.restoreAllMocks(); });

function antwort(inhalt: unknown, ok = true, status = 200) {
  globalThis.fetch = vi.fn(async () => ({
    ok, status, json: async () => inhalt,
  })) as unknown as typeof fetch;
}

describe("ladeSektoren", () => {
  it("reicht die Nahverkehrsbereiche durch und zaehlt sie zur Abdeckung", async () => {
    // Ein Anflug mit eigener Flaeche braucht keine grobe FIR-Ersatzgrenze
    // mehr — sonst laege eine ganze FIR ueber seiner Zone.
    antwort({
      flaechen: { type: "FeatureCollection", features: [] },
      abgedeckt: ["EDGG_CTR"],
      tracon: {
        flaechen: { type: "FeatureCollection", features: [
          { type: "Feature", properties: { ruf: "EDDM_APP", art: "APP" },
            geometry: { type: "Polygon", coordinates: [[[11, 48], [12, 48], [12, 49], [11, 48]]] } }] },
        marken: { type: "FeatureCollection", features: [] },
        abgedeckt: ["EDDM_APP"],
      },
    });
    const r = await ladeSektoren(50, undefined, BASIS);
    expect(r.nahbereich.features).toHaveLength(1);
    expect(r.abgedeckt.has("EDDM_APP")).toBe(true);
    expect(r.abgedeckt.has("EDGG_CTR")).toBe(true);
  });

  it("kommt ohne Nahverkehrsbereiche in der Antwort klar", async () => {
    antwort({ flaechen: { type: "FeatureCollection", features: [] } });
    const r = await ladeSektoren(50, undefined, BASIS);
    expect(r.nahbereich.features).toHaveLength(0);
    expect(r.nahbereichMarken.type).toBe("FeatureCollection");
  });

  it("reicht Flächen, Marken und Abdeckung durch", async () => {
    antwort({
      flaechen: { type: "FeatureCollection", features: [{ type: "Feature", properties: { ruf: "EDGG_CTR" }, geometry: { type: "Polygon", coordinates: [[[8, 50], [9, 50], [9, 51], [8, 50]]] } }] },
      marken: { type: "FeatureCollection", features: [] },
      abgedeckt: ["EDGG_CTR", "EDWW_CTR"],
    });
    const r = await ladeSektoren(250, undefined, BASIS);
    expect(r.flaechen.features).toHaveLength(1);
    expect(r.abgedeckt.has("EDWW_CTR")).toBe(true);
    expect(r.abgedeckt.size).toBe(2);
  });

  it("fragt die gewünschte Flugfläche ganzzahlig ab", async () => {
    antwort({ flaechen: { type: "FeatureCollection", features: [] } });
    await ladeSektoren(247.6, undefined, BASIS);
    expect(globalThis.fetch).toHaveBeenCalledWith(
      `${BASIS}/api/vatglasses?fl=248`, expect.anything(),
    );
  });

  it("liefert bei Serverfehler eine leere Lage statt zu werfen", async () => {
    antwort({}, false, 503);
    const r = await ladeSektoren(250, undefined, BASIS);
    expect(r.flaechen.features).toHaveLength(0);
    expect(r.abgedeckt.size).toBe(0);
  });

  it("überlebt eine halbe Antwort ohne Marken", async () => {
    antwort({ flaechen: { type: "FeatureCollection", features: [] } });
    const r = await ladeSektoren(250, undefined, BASIS);
    expect(r.marken.type).toBe("FeatureCollection");
    expect(r.marken.features).toHaveLength(0);
  });

  it("überlebt kaputtes JSON", async () => {
    globalThis.fetch = vi.fn(async () => ({
      ok: true, status: 200, json: async () => { throw new SyntaxError("kaputt"); },
    })) as unknown as typeof fetch;
    const r = await ladeSektoren(250, undefined, BASIS);
    expect(r.flaechen.features).toHaveLength(0);
  });

  it("reicht einen Abbruch durch, statt ihn als leere Lage zu tarnen", async () => {
    // Wichtig: der Aufrufer verwirft Abbrüche gezielt. Würden sie hier
    // zu einer leeren Lage, löschte ein Kartenwechsel die Sektoren.
    globalThis.fetch = vi.fn(async () => {
      const e = new Error("abgebrochen"); e.name = "AbortError"; throw e;
    }) as unknown as typeof fetch;
    await expect(ladeSektoren(250, undefined, BASIS)).rejects.toThrow(/abgebrochen/);
  });

  it("meldet einen Ausfall, verschluckt ihn aber nicht stumm", async () => {
    const warnung = vi.spyOn(console, "warn").mockImplementation(() => {});
    antwort({}, false, 500);
    await ladeSektoren(250, undefined, BASIS);
    expect(warnung).toHaveBeenCalled();
  });
});
