// Grundkarte in einem Stil: Zusammenfuehren der Vektorstile, Umschalten, Nachladen.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { StyleSpecification } from "maplibre-gl";
import { EBENE_ANKER, grundkarteBauen, grundkarteHolen, startStil, type Grundkartensteuerung } from "./grundkarte";

const QUELLE = { type: "vector", tiles: ["https://x/{z}/{x}/{y}.mvt"] };

/** Aufbau wie die echten CARTO-Stile: gleiche Quelle, gleiche Ebenen in anderer Reihenfolge, eigene Sprites. */
function stil(name: string, ebenen: StyleSpecification["layers"]): StyleSpecification {
  return { version: 8, glyphs: "https://x/fonts/{fontstack}/{range}.pbf", sprite: `https://x/${name}/sprite`, sources: { protomaps: QUELLE as never }, layers: ebenen };
}
const EBENEN = (farbe: string): StyleSpecification["layers"] => [
  { id: "background", type: "background", paint: { "background-color": farbe } },
  { id: "wasser", type: "fill", source: "protomaps", "source-layer": "w", paint: { "fill-color": farbe } },
  { id: "dorf", type: "symbol", source: "protomaps", "source-layer": "p", layout: { "icon-image": "" } },
  { id: "stadt", type: "symbol", source: "protomaps", "source-layer": "p", layout: { "icon-image": "circle-11" } },
  { id: "spaeter", type: "line", source: "protomaps", "source-layer": "l", layout: { visibility: "none" } },
];

function karte() {
  const ebenen: { id: string; spec: Record<string, never> }[] = [];
  const sichtbar = new Map<string, string>();
  const quellen = new Set<string>();
  const sprites = new Set<string>();
  let glyphen = "start";
  const m = {
    addLayer: (spec: Record<string, never> & { id: string }, davor?: string) => {
      const i = davor ? ebenen.findIndex((e) => e.id === davor) : -1;
      ebenen.splice(i < 0 ? ebenen.length : i, 0, { id: spec.id, spec });
      sichtbar.set(spec.id, (spec.layout as { visibility?: string } | undefined)?.visibility ?? "visible");
    },
    removeLayer: (id: string) => { ebenen.splice(ebenen.findIndex((e) => e.id === id), 1); sichtbar.delete(id); },
    addSource: (id: string) => { quellen.add(id); },
    removeSource: (id: string) => { quellen.delete(id); },
    addSprite: (id: string) => { sprites.add(id); },
    removeSprite: (id: string) => { sprites.delete(id); },
    getGlyphs: () => glyphen,
    setGlyphs: (g: string) => { glyphen = g; },
    setLayoutProperty: vi.fn((id: string, _k: string, v: string) => { sichtbar.set(id, v); }),
    setPaintProperty: vi.fn(),
  };
  // Der Anker steht wie im Startstil schon in der Karte.
  m.addLayer({ id: "esri-imagery" } as never);
  m.addLayer({ id: "esri-reference" } as never);
  m.addLayer({ id: EBENE_ANKER } as never);
  m.addLayer({ id: "route-ueber-allem" } as never);
  return { m, ebenen, sichtbar, quellen, sprites, glyphen: () => glyphen };
}

const STILE = { dunkel: stil("dunkel", EBENEN("#000")), hell: stil("hell", [...EBENEN("#fff")].reverse()) };
const an = (k: ReturnType<typeof karte>, praefix: string) => [...k.sichtbar].filter(([id, v]) => id.startsWith(praefix) && v === "visible").map(([id]) => id);

describe("Grundkarte", () => {
  it("der Startstil braucht kein Netz und zeigt nichts ausser der Flaeche", () => {
    const s = startStil("https://x/{fontstack}/{range}.pbf");
    expect(s.glyphs).toBe("https://x/{fontstack}/{range}.pbf");
    expect(s.layers.at(-1)!.id).toBe(EBENE_ANKER);
    expect(s.layers.filter((l) => l.layout?.visibility !== "none").map((l) => l.type)).toEqual(["background"]);
  });

  it("legt alle Grundkarten-Ebenen unter den Anker, auch beim Ersetzen", () => {
    const k = karte();
    const g = grundkarteBauen(k.m as never);
    g.einsetzen(STILE);
    g.einsetzen(STILE);
    const anker = k.ebenen.findIndex((e) => e.id === EBENE_ANKER);
    const letzte = Math.max(...k.ebenen.flatMap((e, i) => (/^[dh]:/.test(e.id) ? [i] : [])));
    expect(letzte).toBeLessThan(anker);
    expect(k.ebenen.at(-1)!.id).toBe("route-ueber-allem");
    expect(k.ebenen.filter((e) => /^[dh]:/.test(e.id))).toHaveLength(10); // nichts doppelt
  });

  it("nutzt eine gleiche Quelle nur einmal, verschiedene getrennt", () => {
    const k = karte();
    grundkarteBauen(k.m as never).einsetzen(STILE);
    expect([...k.quellen]).toEqual(["grund-protomaps"]);

    const k2 = karte();
    const anders = stil("hell", EBENEN("#fff"));
    anders.sources.protomaps = { type: "vector", tiles: ["https://y/{z}/{x}/{y}.mvt"] };
    grundkarteBauen(k2.m as never).einsetzen({ dunkel: STILE.dunkel, hell: anders });
    expect([...k2.quellen]).toEqual(["grund-protomaps", "grund-h-protomaps"]);
    expect(k2.ebenen.find((e) => e.id === "h:wasser")!.spec.source).toBe("grund-h-protomaps");
  });

  it("gibt jeder Gruppe ihre Symbole, leere Namen bleiben leer", () => {
    const k = karte();
    grundkarteBauen(k.m as never).einsetzen(STILE);
    expect([...k.sprites]).toEqual(["d", "h"]);
    const bild = (id: string) => (k.ebenen.find((e) => e.id === id)!.spec.layout as Record<string, string>)["icon-image"];
    expect(bild("d:stadt")).toBe("d:circle-11");
    expect(bild("h:stadt")).toBe("h:circle-11");
    expect(bild("d:dorf")).toBe("");
  });

  it("schaltet nur Sichtbarkeiten, und eine im Stil versteckte Ebene bleibt versteckt", () => {
    const k = karte();
    const g = grundkarteBauen(k.m as never);
    g.einsetzen(STILE);
    expect(an(k, "d:")).toHaveLength(4);
    expect(an(k, "h:")).toHaveLength(0);
    expect(k.sichtbar.get("d:spaeter")).toBe("none");

    g.zeigen("hell");
    expect(an(k, "d:")).toHaveLength(0);
    expect(an(k, "h:")).toHaveLength(4);

    g.zeigen("satellit");
    expect(an(k, "d:")).toHaveLength(0);
    expect(an(k, "h:")).toHaveLength(0);
    expect(k.sichtbar.get("esri-imagery")).toBe("visible");
    expect(k.sichtbar.get("esri-reference")).toBe("visible");

    g.zeigen("dunkel");
    expect(k.sichtbar.get("esri-imagery")).toBe("none");
    expect(an(k, "d:")).toHaveLength(4);
  });

  it("setzt die laufende Wahl auch dann um, wenn die Stile erst danach kommen", () => {
    const k = karte();
    const g = grundkarteBauen(k.m as never);
    g.zeigen("hell");
    g.einsetzen(STILE);
    expect(an(k, "h:")).toHaveLength(4);
    expect(an(k, "d:")).toHaveLength(0);
  });

  it("uebernimmt die Schriften des Stils", () => {
    const k = karte();
    grundkarteBauen(k.m as never).einsetzen(STILE);
    expect(k.glyphen()).toBe("https://x/fonts/{fontstack}/{range}.pbf");
  });
});

describe("Grundkarte holen", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });
  const steuerung = () => ({ einsetzen: vi.fn(), zeigen: vi.fn() }) satisfies Grundkartensteuerung;
  const antwort = (ok = true) => Promise.resolve({ ok, status: ok ? 200 : 503, json: async () => STILE.dunkel });

  it("schickt die Stil-Anfrage durch den Umschreiber, sonst fehlte dort der Schluessel", async () => {
    const abruf = vi.fn(() => antwort());
    vi.stubGlobal("fetch", abruf);
    const g = steuerung();
    grundkarteHolen(g, { dunkel: "https://a", hell: "https://b" }, (url) => ({ url: `${url}?key=K` }));
    await vi.advanceTimersByTimeAsync(0);
    expect(abruf.mock.calls.map((c: unknown[]) => c[0])).toEqual(["https://a?key=K", "https://b?key=K"]);
    expect(g.einsetzen).toHaveBeenCalledTimes(1);
  });

  it("versucht es nach einem Fehlschlag erneut", async () => {
    const abruf = vi.fn().mockImplementationOnce(() => Promise.reject(new Error("offline"))).mockImplementation(() => antwort());
    vi.stubGlobal("fetch", abruf);
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const g = steuerung();
    grundkarteHolen(g, { dunkel: "https://a", hell: "https://b" }, () => undefined);
    await vi.advanceTimersByTimeAsync(0);
    expect(g.einsetzen).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(15_000);
    expect(g.einsetzen).toHaveBeenCalledTimes(1);
  });

  it("wertet auch einen HTTP-Fehler als Fehlschlag", async () => {
    vi.stubGlobal("fetch", vi.fn(() => antwort(false)));
    vi.spyOn(console, "warn").mockImplementation(() => {});
    const g = steuerung();
    grundkarteHolen(g, { dunkel: "https://a", hell: "https://b" }, () => undefined);
    await vi.advanceTimersByTimeAsync(0);
    expect(g.einsetzen).not.toHaveBeenCalled();
  });

  it("setzt nach dem Abbrechen nichts mehr ein", async () => {
    vi.stubGlobal("fetch", vi.fn(() => antwort()));
    const g = steuerung();
    const stopp = grundkarteHolen(g, { dunkel: "https://a", hell: "https://b" }, () => undefined);
    stopp();
    await vi.advanceTimersByTimeAsync(20_000);
    expect(g.einsetzen).not.toHaveBeenCalled();
  });
});
