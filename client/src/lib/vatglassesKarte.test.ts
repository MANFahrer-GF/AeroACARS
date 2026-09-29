// Der Client holt die Sektoren beim Live-Server statt selbst zu rechnen.
// Geprüft wird, was dabei schiefgehen kann: Server weg, Antwort halb,
// Abbruch beim Kartenwechsel. Seit 29.09.2026 laeuft der Abruf ueber den
// Rust-Befehl `live_inhalt` (Pilot-Token), nicht mehr per `fetch`.
import { describe, expect, it, vi, afterEach } from "vitest";

const invoke = vi.fn();
vi.mock("./ipc", () => ({
  invoke: (...a: unknown[]) => invoke(...a),
  listen: vi.fn(async () => () => {}),
}));

import { ladeSektoren } from "./vatglassesKarte";

afterEach(() => { invoke.mockReset(); vi.restoreAllMocks(); });

function antwort(inhalt: unknown, ok = true, status = 200) {
  if (ok) invoke.mockResolvedValue(inhalt);
  else invoke.mockRejectedValue({ code: "live_fehler", message: `HTTP ${status}` });
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
    const r = await ladeSektoren(50);
    expect(r.nahbereich.features).toHaveLength(1);
    expect(r.abgedeckt.has("EDDM_APP")).toBe(true);
    expect(r.abgedeckt.has("EDGG_CTR")).toBe(true);
  });

  it("kommt ohne Nahverkehrsbereiche in der Antwort klar", async () => {
    antwort({ flaechen: { type: "FeatureCollection", features: [] } });
    const r = await ladeSektoren(50);
    expect(r.nahbereich.features).toHaveLength(0);
    expect(r.nahbereichMarken.type).toBe("FeatureCollection");
  });

  it("reicht Flächen, Marken und Abdeckung durch", async () => {
    antwort({
      flaechen: { type: "FeatureCollection", features: [{ type: "Feature", properties: { ruf: "EDGG_CTR" }, geometry: { type: "Polygon", coordinates: [[[8, 50], [9, 50], [9, 51], [8, 50]]] } }] },
      marken: { type: "FeatureCollection", features: [] },
      abgedeckt: ["EDGG_CTR", "EDWW_CTR"],
    });
    const r = await ladeSektoren(250);
    expect(r.flaechen.features).toHaveLength(1);
    expect(r.abgedeckt.has("EDWW_CTR")).toBe(true);
    expect(r.abgedeckt.size).toBe(2);
  });

  it("fragt die gewünschte Flugfläche ganzzahlig ab", async () => {
    antwort({ flaechen: { type: "FeatureCollection", features: [] } });
    await ladeSektoren(247.6);
    expect(invoke).toHaveBeenCalledWith("live_inhalt", { pfad: "/api/vatglasses?fl=248" });
  });

  it("liefert bei Serverfehler eine leere Lage statt zu werfen", async () => {
    antwort({}, false, 503);
    const r = await ladeSektoren(250);
    expect(r.flaechen.features).toHaveLength(0);
    expect(r.abgedeckt.size).toBe(0);
  });

  it("überlebt eine halbe Antwort ohne Marken", async () => {
    antwort({ flaechen: { type: "FeatureCollection", features: [] } });
    const r = await ladeSektoren(250);
    expect(r.marken.type).toBe("FeatureCollection");
    expect(r.marken.features).toHaveLength(0);
  });

  it("überlebt kaputtes JSON", async () => {
    // Rust meldet eine unlesbare Antwort als `live_fehler`.
    invoke.mockRejectedValue({ code: "live_fehler", message: "error decoding response body" });
    const r = await ladeSektoren(250);
    expect(r.flaechen.features).toHaveLength(0);
  });

  it("reicht einen Abbruch durch, statt ihn als leere Lage zu tarnen", async () => {
    // Wichtig: der Aufrufer verwirft Abbrüche gezielt. Würden sie hier
    // zu einer leeren Lage, löschte ein Kartenwechsel die Sektoren.
    invoke.mockReturnValue(new Promise(() => {}));
    const ac = new AbortController();
    const laeuft = ladeSektoren(250, ac.signal);
    const e = new Error("abgebrochen"); e.name = "AbortError";
    ac.abort(e);
    await expect(laeuft).rejects.toThrow(/abgebrochen/);
  });

  it("eine abgelaufene Zeitgrenze ist kein Abbruch, sondern eine leere Lage", async () => {
    // Wie bei `fetch`: AbortSignal.timeout meldet `TimeoutError`. Wuerde das
    // als Abbruch durchgereicht, verwirft der Aufrufer es und die Karte
    // behielte stumm alte Sektoren.
    vi.spyOn(console, "warn").mockImplementation(() => {});
    invoke.mockReturnValue(new Promise(() => {}));
    const ac = new AbortController();
    const laeuft = ladeSektoren(250, ac.signal);
    const t = new Error("zu lange"); t.name = "TimeoutError";
    ac.abort(t);
    const r = await laeuft;
    expect(r.flaechen.features).toHaveLength(0);
  });

  it("fragt IVAO ueber dieselbe Route mit netz=ivao", async () => {
    antwort({ flaechen: { type: "FeatureCollection", features: [] } });
    await ladeSektoren("alle", undefined, "ivao");
    expect(invoke).toHaveBeenCalledWith("live_inhalt", { pfad: "/api/vatglasses?fl=alle&netz=ivao" });
  });

  it("meldet einen Ausfall, verschluckt ihn aber nicht stumm", async () => {
    const warnung = vi.spyOn(console, "warn").mockImplementation(() => {});
    antwort({}, false, 500);
    await ladeSektoren(250);
    expect(warnung).toHaveBeenCalled();
  });
});
