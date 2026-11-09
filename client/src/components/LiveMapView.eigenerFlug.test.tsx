// Der eigene Flug auf der Live-Karte (Abnahmeliste B1 bis B10, B13 und G7), durch die echte Komponente.
//
// Die Attrappe von maplibre-gl zeichnet auf, was die Karte tut: Marker samt Drehung und Inhalt,
// Kamerafahrten mit ihren Argumenten, setData je Quelle, die Ebenen-Spezifikationen, Sichtbarkeit
// und Farben. `aircraftIcon` und `phaseColors` liefern Werte, die vom Argument abhaengen. Ohne
// das waeren Symbol, Farbe und Drehung unsichtbar (ein Leerlauf-Mock besteht jeden Test).

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

interface Marke {
  el: HTMLElement;
  opts: Record<string, unknown>;
  lngLat: [number, number] | null;
  rotation: number | null;
  entfernt: boolean;
}
type Aufruf = { art: "easeTo" | "jumpTo" | "flyTo"; arg: Record<string, unknown> };

const h = vi.hoisted(() => ({
  handlers: {} as Record<string, Array<(e?: unknown) => void>>,
  marker: [] as Marke[],
  kamera: [] as Aufruf[],
  einpassen: [] as Array<{ punkte: [number, number][]; opts: Record<string, unknown> }>,
  daten: new Map<string, { features?: Array<{ geometry: { coordinates: unknown }; properties: Record<string, unknown> }> }>(),
  spec: new Map<string, Record<string, unknown>>(),
  layers: new Set<string>(),
  sichtbar: new Map<string, string>(),
  farben: new Map<string, unknown>(),
  fixes: [] as unknown[],
  track: [] as unknown[],
  flughaefen: {} as Record<string, { lat: number; lon: number }>,
}));

vi.mock("maplibre-gl", () => {
  class FakeMarker {
    rec: Marke;
    constructor(opts?: { element?: HTMLElement }) {
      this.rec = { el: opts?.element ?? document.createElement("div"), opts: (opts ?? {}) as Record<string, unknown>, lngLat: null, rotation: null, entfernt: false };
      h.marker.push(this.rec);
    }
    setLngLat(ll: [number, number]) { this.rec.lngLat = ll; return this; }
    setRotation(r: number) { this.rec.rotation = r; return this; }
    addTo() { return this; }
    remove() { this.rec.entfernt = true; return this; }
    getElement() { return this.rec.el; }
  }
  class FakePopup { setLngLat() { return this; } setHTML() { return this; } addTo() { return this; } remove() { return this; } on() { return this; } }
  class FakeBounds {
    punkte: Array<[number, number]> = [];
    extend(p: [number, number]) { this.punkte.push(p); return this; }
  }
  class FakeMap {
    center = { lng: 6, lat: 48 };
    zoom = 4;
    bearing = 0;
    sources = new Set<string>();
    addControl() { return this; }
    on(ev: string, cb: (e?: unknown) => void) {
      if (ev === "load") { cb(); return this; }
      (h.handlers[ev] ||= []).push(cb);
      return this;
    }
    once() { return this; }
    off(ev: string, cb: (e?: unknown) => void) {
      const liste = h.handlers[ev];
      if (liste) h.handlers[ev] = liste.filter((f) => f !== cb);
      return this;
    }
    getBounds() {
      return { getSouth: () => this.center.lat - 1, getNorth: () => this.center.lat + 1, getWest: () => this.center.lng - 1, getEast: () => this.center.lng + 1 };
    }
    getCenter() { return this.center; }
    getZoom() { return this.zoom; }
    getBearing() { return this.bearing; }
    isStyleLoaded() { return true; }
    getLayer(id: string) { return h.layers.has(id) ? ({} as unknown) : undefined; }
    setLayoutProperty(id: string, _k: string, v: string) { h.sichtbar.set(id, v); return this; }
    setPaintProperty(id: string, k: string, v: unknown) { h.farben.set(`${id}|${k}`, v); return this; }
    setFilter() { return this; }
    setProjection() { return this; }
    addSprite() { return this; }
    removeSprite() { return this; }
    getGlyphs() { return ""; }
    setGlyphs() { return this; }
    addSource(id: string) { this.sources.add(id); return this; }
    getSource(id: string) {
      if (!this.sources.has(id)) return undefined;
      return { setData: (d: { features?: never[] }) => { h.daten.set(id, d as never); } } as unknown;
    }
    addLayer(o: { id: string; layout?: { visibility?: string } }) {
      h.spec.set(o.id, o as never);
      h.layers.add(o.id);
      if (o.layout?.visibility) h.sichtbar.set(o.id, o.layout.visibility);
      return this;
    }
    hasImage() { return false; }
    addImage() { return this; }
    removeLayer(id: string) { h.layers.delete(id); return this; }
    removeSource(id: string) { this.sources.delete(id); return this; }
    private fahren(art: Aufruf["art"], arg: Record<string, unknown>) {
      h.kamera.push({ art, arg });
      if (arg.center) this.center = { lng: (arg.center as number[])[0], lat: (arg.center as number[])[1] };
      if (arg.zoom != null) this.zoom = arg.zoom as number;
      if (arg.bearing != null) this.bearing = arg.bearing as number;
      return this;
    }
    easeTo(o: Record<string, unknown>) { return this.fahren("easeTo", o); }
    jumpTo(o: Record<string, unknown>) { return this.fahren("jumpTo", o); }
    flyTo(o: Record<string, unknown>) { return this.fahren("flyTo", o); }
    fitBounds(b: { punkte: [number, number][] }, opts: Record<string, unknown>) {
      h.einpassen.push({ punkte: [...b.punkte], opts });
      return this;
    }
    resize() { return this; }
    remove() { return this; }
  }
  return { default: { Map: FakeMap, Marker: FakeMarker, Popup: FakePopup, NavigationControl: class {}, LngLatBounds: FakeBounds } };
});

vi.mock("../lib/ipc", () => ({
  isTauri: false,
  invoke: vi.fn(async (cmd: string, args?: { icao?: string }) => {
    if (cmd === "flight_get_route_fixes") return h.fixes;
    if (cmd === "flight_get_track") return h.track;
    if (cmd === "airport_get") return h.flughaefen[args?.icao ?? ""] ?? { lat: null, lon: null };
    if (cmd === "va_live_flights") return { data: [] };
    if (cmd === "airport_ground_index") return [];
    if (cmd === "activity_log_get" || cmd === "hoppie_get_thread") return [];
    return null;
  }),
  listen: vi.fn(async () => () => {}),
  openExternal: vi.fn(async () => {}),
}));
vi.mock("./ActivityLogPanel", () => ({ ActivityLogPanel: () => null }));
vi.mock("../lib/trackStore", () => ({ getTrack: () => [], setTrack: () => {} }));
// Die Attrappen liefern Werte, die vom Argument abhaengen: so zeigt die Marke, WAS die Karte uebergeben hat.
vi.mock("../lib/aircraftIcon", () => ({ aircraftSvg: (icao: string | null) => `<svg data-muster="${icao ?? ""}"></svg>` }));
vi.mock("../lib/phaseColors", () => ({
  phaseColor: (p?: string | null) => ({ climb: "#11aa11", cruise: "#2222ee" } as Record<string, string>)[p ?? ""] ?? "#999999",
  phaseLabel: (p?: string | null) => `Phase ${p}`,
}));

import { LiveMapView } from "./LiveMapView";

const LTFM: [number, number] = [28.75, 41.27];
const LYTV: [number, number] = [19.25, 42.36];
const F1 = { ident: "F1", lat: 41.5, lon: 26.0, kind: "wpt" };
const F2 = { ident: "F2", lat: 41.9, lon: 22.0, kind: "wpt" };
const F3 = { ident: "F3", lat: 42.2, lon: 20.0, kind: "wpt" };

const flug = {
  pirep_id: "P1", airline_icao: "THY", flight_number: "155", dpt_airport: "LTFM", arr_airport: "LYTV",
  phase: "climb", distance_nm: 120, was_just_resumed: false,
};
const snap = (lat = 41.2, lon = 28.7, extra: Record<string, unknown> = {}) => ({
  lat, lon, altitude_msl_ft: 6000, groundspeed_kt: 280, indicated_airspeed_kt: 250,
  heading_deg_true: 295, heading_deg_magnetic: 295, aircraft_icao: "A21N", timestamp: 0, ...extra,
});
type Angaben = { flug?: typeof flug | null; snap?: ReturnType<typeof snap> | null; sichtbar?: boolean };
const ansicht = (a: Angaben = {}) => (
  <LiveMapView
    activeFlight={(a.flug === undefined ? { ...flug } : a.flug) as never}
    simSnapshot={(a.snap === undefined ? snap() : a.snap) as never}
    simKind={undefined}
    onSwitchToBriefing={() => {}}
    sichtbar={a.sichtbar}
  />
);
async function fertig() {
  await act(async () => { await new Promise((r) => setTimeout(r, 20)); });
}
async function karte(a: Angaben = {}) {
  const r = render(ansicht(a));
  await fertig();
  return { ...r, neu: async (b: Angaben) => { r.rerender(ansicht(b)); await fertig(); } };
}
const eigene = () => h.marker.find((m) => m.el.className === "aa-ac-marker" && !m.entfernt)!;
const pins = () => h.marker.filter((m) => m.el.className.startsWith("aa-pin"));
const mitMitte = () => h.kamera.filter((k) => k.art !== "flyTo" && k.arg.center);
const nurBearing = () => h.kamera.filter((k) => k.art === "easeTo" && !k.arg.center);
const features = (quelle: string) => h.daten.get(quelle)?.features ?? [];
const knopf = (name: string) => screen.getByRole("button", { name });

beforeEach(() => {
  vi.stubGlobal("fetch", () => new Promise(() => {})); // Stile der Grundkarte: eigener Test
  h.handlers = {}; h.marker = []; h.kamera = []; h.einpassen = [];
  h.daten.clear(); h.spec.clear(); h.layers.clear(); h.sichtbar.clear(); h.farben.clear();
  h.fixes = []; h.track = [];
  h.flughaefen = { LTFM: { lat: LTFM[1], lon: LTFM[0] }, LYTV: { lat: LYTV[1], lon: LYTV[0] } };
  document.documentElement.dataset.theme = "dark";
  localStorage.clear();
});
afterEach(() => { cleanup(); vi.useRealTimers(); vi.unstubAllGlobals(); });

describe("Flugzeug-Marke (B1)", () => {
  it("zeigt das Symbol des Musters und tauscht es, wenn das Muster nachgeladen wird", async () => {
    const k = await karte();
    const marke = eigene();
    expect(marke.el.innerHTML).toContain('data-muster="A21N"');
    await k.neu({ snap: snap(41.2, 28.7, { aircraft_icao: "B738" }) });
    expect(eigene(), "dieselbe Marke, nicht neu gebaut").toBe(marke);
    expect(marke.el.innerHTML).toContain('data-muster="B738"');
  });

  it("faerbt die Marke nach der Flugphase", async () => {
    const k = await karte();
    expect(eigene().el.style.getPropertyValue("--ac-color")).toBe("#11aa11"); // climb
    await k.neu({ flug: { ...flug, phase: "cruise" } });
    expect(eigene().el.style.getPropertyValue("--ac-color")).toBe("#2222ee");
  });

  it("dreht die Marke auf den Kurs und richtet sie an der Karte aus", async () => {
    const k = await karte();
    expect(eigene().rotation).toBe(295);
    expect(eigene().opts.rotationAlignment).toBe("map");
    await k.neu({ snap: snap(41.2, 28.7, { heading_deg_true: 90 }) });
    expect(eigene().rotation).toBe(90);
  });
});

describe("Folgen: Phasen-Zoom (B2)", () => {
  it("setzt den Phasen-Zoom beim ersten Folgen und fasst den Zoom danach nicht mehr an", async () => {
    const k = await karte();
    // Klettern = 7,5; die Karte steht weit weg (6/48), also ein Sprung auf den Flieger.
    expect(mitMitte()[0]).toMatchObject({ art: "jumpTo", arg: { zoom: 7.5 } });
    h.kamera = [];
    await k.neu({ flug: { ...flug, phase: "cruise" }, snap: snap(41.3, 28.6) });
    expect(mitMitte().length).toBeGreaterThan(0);
    for (const fahrt of mitMitte()) expect(fahrt.arg, "laufendes Folgen laesst den Zoom stehen").not.toHaveProperty("zoom");
  });

  it("setzt den Phasen-Zoom neu, sobald Folgen bewusst wieder eingeschaltet wird", async () => {
    await karte({ flug: { ...flug, phase: "cruise" } });
    fireEvent.click(knopf("Folgen")); // aus
    h.kamera = [];
    fireEvent.click(knopf("Folgen")); // bewusst wieder an
    // Reiseflug = 5
    expect(mitMitte().at(-1)!.arg).toMatchObject({ zoom: 5, center: [28.7, 41.2] });
  });
});

describe("Zentrieren-Knopf (B3)", () => {
  it("fehlt ohne laufenden Flug, mit Flug aber ohne Position, und steht mit beidem da", async () => {
    const ohneFlug = await karte({ flug: null, snap: null });
    expect(screen.queryByRole("button", { name: "Auf Flug zentrieren" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Folgen" })).toBeNull();
    ohneFlug.unmount();
    const ohnePosition = await karte({ snap: null });
    expect(screen.queryByRole("button", { name: "Auf Flug zentrieren" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Folgen" })).not.toBeNull(); // Gegenstueck: der Flug allein bringt Folgen
    ohnePosition.unmount();
    await karte();
    expect(screen.queryByRole("button", { name: "Auf Flug zentrieren" })).not.toBeNull();
  });
});

describe("Folgen abgehakt (B4)", () => {
  beforeEach(() => { h.fixes = [F1, F2, F3]; });

  it("passt beim bewussten Abhaken genau einmal auf die ganze Route ein", async () => {
    const k = await karte();
    expect(h.einpassen, "solange Folgen an ist, wird nicht eingepasst").toHaveLength(0);
    fireEvent.click(knopf("Folgen"));
    expect(h.einpassen).toHaveLength(1);
    expect(h.einpassen[0].punkte).toEqual([LTFM, [26, 41.5], [22, 41.9], [20, 42.2], LYTV]);
    expect(h.einpassen[0].opts).toEqual({ padding: 80, duration: 600, maxZoom: 8 });
    // Jede Antwort des Backends bringt ein neues Flug-Objekt: trotzdem nicht noch einmal.
    await k.neu({ flug: { ...flug }, snap: snap(41.3, 28.5) });
    fireEvent.click(knopf("Folgen"));
    fireEvent.click(knopf("Folgen"));
    expect(h.einpassen).toHaveLength(1);
  });

  it("passt nach einem Pan nicht ein", async () => {
    const k = await karte();
    act(() => { (h.handlers["dragstart"] ?? []).forEach((cb) => cb({ originalEvent: {} })); });
    expect(knopf("Folgen").getAttribute("aria-pressed")).toBe("false");
    await k.neu({ flug: { ...flug }, snap: snap(41.3, 28.5) });
    expect(h.einpassen).toHaveLength(0);
  });
});

describe("Kurs oben (B5)", () => {
  it("startet auf Norden oben, dreht nach Klick auf die Peilung und kehrt auf Norden zurueck", async () => {
    await karte();
    expect(knopf("Norden oben").getAttribute("aria-pressed")).toBe("true");
    expect(localStorage.getItem("aaLivemapTrackUp")).toBe("0");
    h.kamera = [];
    fireEvent.click(knopf("Kurs oben"));
    expect(nurBearing().at(-1)!.arg).toEqual({ bearing: 295, duration: 400 });
    expect(localStorage.getItem("aaLivemapTrackUp")).toBe("1");
    fireEvent.click(knopf("Norden oben"));
    expect(nurBearing().at(-1)!.arg).toEqual({ bearing: 0, duration: 400 });
    expect(localStorage.getItem("aaLivemapTrackUp")).toBe("0");
  });

  it("zittert nicht: Rauschen unter einem halben Grad loest keine Drehung aus, echte Kursaenderung schon", async () => {
    const k = await karte();
    fireEvent.click(knopf("Kurs oben"));
    const vorher = nurBearing().length;
    await k.neu({ snap: snap(41.2, 28.7, { heading_deg_true: 295.4 }) });
    await k.neu({ snap: snap(41.2, 28.7, { heading_deg_true: 294.8 }) });
    expect(nurBearing().length, "gerundet bleibt es 295").toBe(vorher);
    // Das Folgen zielt dabei auf dieselbe Peilung, damit sich die beiden Fahrten nicht bekaempfen.
    expect(mitMitte().at(-1)!.arg.bearing).toBe(295);
    await k.neu({ snap: snap(41.2, 28.7, { heading_deg_true: 301.2 }) });
    expect(nurBearing().at(-1)!.arg).toEqual({ bearing: 301, duration: 400 });
    expect(mitMitte().at(-1)!.arg.bearing).toBe(301);
  });

  it("merkt sich die Wahl und nimmt sie beim naechsten Start auf", async () => {
    localStorage.setItem("aaLivemapTrackUp", "1");
    await karte();
    expect(knopf("Kurs oben").getAttribute("aria-pressed")).toBe("true");
    expect(nurBearing().at(-1)!.arg).toEqual({ bearing: 295, duration: 400 });
  });

  it("merkt sich die Wahl auch verdeckt, bewegt aber die Kamera nicht", async () => {
    await karte({ sichtbar: false });
    h.kamera = [];
    fireEvent.click(knopf("Kurs oben"));
    expect(localStorage.getItem("aaLivemapTrackUp")).toBe("1");
    expect(h.kamera).toHaveLength(0);
  });
});

describe("Spur (B6)", () => {
  const spur = Array.from({ length: 10 }, (_, i) => [28.0 + i * 0.01, 41.0 + i * 0.01] as [number, number]);
  beforeEach(() => { h.track = spur; });

  it("zieht die durchgezogene Linie bis an den Marker und setzt jeden 8. Punkt", async () => {
    await karte();
    const linie = features("aa-flown-track")[0].geometry.coordinates as number[][];
    expect(linie).toHaveLength(11);
    expect(linie[0]).toEqual(spur[0]);
    expect(linie.at(-1)).toEqual([28.7, 41.2]); // die Live-Position, nicht der letzte aufgezeichnete Punkt
    expect(eigene().lngLat).toEqual([28.7, 41.2]);
    expect(features("aa-flown-track-dots").map((f) => f.geometry.coordinates)).toEqual([spur[0], spur[8]]);
  });

  it("zeichnet die Spur durchgezogen und die Route gestrichelt", async () => {
    await karte();
    expect((h.spec.get("aa-flown-track-line")!.paint as Record<string, unknown>)["line-dasharray"]).toBeUndefined();
    expect((h.spec.get("aa-planned-route-line")!.paint as Record<string, unknown>)["line-dasharray"]).toEqual([2, 2]);
  });

  it("blendet Spur und Punkte mit dem Schalter Track aus und wieder ein", async () => {
    await karte();
    const ebenen = ["aa-flown-track-line", "aa-flown-track-dots-circles"];
    for (const e of ebenen) expect(h.sichtbar.get(e), e).toBe("visible");
    fireEvent.click(knopf("Track"));
    for (const e of ebenen) expect(h.sichtbar.get(e), e).toBe("none");
    fireEvent.click(knopf("Track"));
    for (const e of ebenen) expect(h.sichtbar.get(e), e).toBe("visible");
  });
});

describe("Route und OFP (B7)", () => {
  it("nennt Wegpunkte erst ab Zoom 6,5, mit ihrem Namen", async () => {
    await karte();
    const namen = h.spec.get("aa-planned-wpts-labels")!;
    expect(namen.minzoom).toBe(6.5);
    expect((namen.layout as Record<string, unknown>)["text-field"]).toEqual(["get", "ident"]);
  });

  it("hebt den naechsten Wegpunkt hervor und rueckt mit dem Flug weiter", async () => {
    h.fixes = [F1, F2, F3];
    const k = await karte();
    const naechster = () => features("aa-planned-wpts").filter((f) => f.properties.isNext).map((f) => f.properties.ident);
    expect(features("aa-planned-wpts")).toHaveLength(3);
    expect(naechster()).toEqual(["F1"]);
    // F1 und F2 liegen hinter dem Flugzeug, voraus ist F3.
    await k.neu({ snap: snap(41.95, 21.5) });
    expect(naechster()).toEqual(["F3"]);
  });

  it("zeigt eine spaet geladene OFP, sobald sie ankommt", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
    const k = await karte();
    // Noch keine OFP: keine Wegpunkte, die Route ist die Luftlinie Start-Ziel.
    expect(features("aa-planned-wpts")).toHaveLength(0);
    expect(features("aa-planned-route")[0].geometry.coordinates).toEqual([LTFM, LYTV]);
    h.fixes = [F1, F2, F3];
    await act(async () => { vi.advanceTimersByTime(5000); await new Promise((r) => setTimeout(r, 20)); });
    expect(features("aa-planned-wpts").map((f) => f.properties.ident)).toEqual(["F1", "F2", "F3"]);
    expect(features("aa-planned-route")[0].geometry.coordinates).toEqual([[26, 41.5], [22, 41.9], [20, 42.2]]);
    void k;
  });
});

// Kleiner Auswerter fuer die Ausdruecke der Wegpunkt-Ebene: so zaehlt, was MapLibre daraus machen wuerde.
function auswerten(ausdruck: unknown, p: Record<string, unknown>): unknown {
  if (!Array.isArray(ausdruck)) return ausdruck;
  const [op, ...a] = ausdruck as [string, ...unknown[]];
  switch (op) {
    case "get": return p[a[0] as string];
    case "literal": return a[0];
    case "==": return auswerten(a[0], p) === auswerten(a[1], p);
    case "in": return (auswerten(a[1], p) as unknown[]).includes(auswerten(a[0], p));
    case "case": {
      for (let i = 0; i + 1 < a.length; i += 2) if (auswerten(a[i], p)) return auswerten(a[i + 1], p);
      return auswerten(a[a.length - 1], p);
    }
  }
  throw new Error(`Ausdruck ${op} nicht nachgebaut`);
}

describe("TOC und TOD (B8)", () => {
  it("haben eine eigene Farbe und Groesse, der naechste Wegpunkt ist am groessten", async () => {
    h.fixes = [{ ident: "TOC", lat: 41.6, lon: 25, kind: "wpt" }, F1, { ident: "TOD", lat: 42.0, lon: 21, kind: "wpt" }];
    await karte();
    // Die Daten tragen TOC/TOD als Art, egal was das Backend als Art meldet.
    expect(features("aa-planned-wpts").map((f) => f.properties.kind)).toEqual(["TOC", "wpt", "TOD"]);
    const radius = h.spec.get("aa-planned-wpts-circles")!.paint as Record<string, unknown>;
    const farbe = h.farben.get("aa-planned-wpts-circles|circle-color");
    const wert = (art: string, istNaechster = false) => ({
      radius: auswerten(radius["circle-radius"], { kind: art, isNext: istNaechster }),
      farbe: auswerten(farbe, { kind: art, isNext: istNaechster }),
    });
    expect(wert("TOC")).toEqual({ radius: 5, farbe: "#ff9f0a" });
    expect(wert("TOD")).toEqual({ radius: 5, farbe: "#ff9f0a" });
    expect(wert("wpt")).toEqual({ radius: 3, farbe: "#0a84ff" });
    expect(wert("wpt", true)).toEqual({ radius: 6, farbe: "#0a84ff" });
  });
});

describe("Start- und Ziel-Pins (B9)", () => {
  it("setzen das ICAO an beide Plaetze, ohne bei jedem Takt Duplikate zu hinterlassen", async () => {
    const k = await karte();
    const lebend = () => pins().filter((m) => !m.entfernt);
    expect(lebend().map((m) => [m.el.className, m.el.textContent, m.lngLat])).toEqual([
      ["aa-pin aa-pin--dep", "LTFM", LTFM],
      ["aa-pin aa-pin--arr", "LYTV", LYTV],
    ]);
    expect(lebend()[0].opts.anchor).toBe("bottom");
    await k.neu({ snap: snap(41.3, 28.5) });
    expect(lebend()).toHaveLength(2);
  });
});

describe("Datumsgrenze (B10)", () => {
  it("zaehlt die Laenge der eigenen Route ueber 180 Grad hinaus weiter", async () => {
    h.fixes = [{ ident: "OST", lat: 52, lon: 179, kind: "wpt" }, { ident: "WEST", lat: 53, lon: -179, kind: "wpt" }];
    await karte();
    expect(features("aa-planned-route")[0].geometry.coordinates).toEqual([[179, 52], [181, 53]]);
  });

  it("zaehlt auch die Laenge der Luftlinie Start-Ziel weiter, solange keine Route da ist", async () => {
    h.flughaefen = { LTFM: { lat: 52, lon: 179 }, LYTV: { lat: 53, lon: -179 } };
    await karte();
    expect(features("aa-planned-route")[0].geometry.coordinates).toEqual([[179, 52], [181, 53]]);
  });

  it("zaehlt die Laenge der geflogenen Spur ueber 180 Grad hinaus weiter", async () => {
    h.track = [[178.5, 52], [179.5, 52.2], [-179.5, 52.4]];
    await karte({ snap: snap(52.6, -179.2) });
    const linie = features("aa-flown-track")[0].geometry.coordinates as number[][];
    expect(linie.map((p) => p[0])).toEqual([178.5, 179.5, 180.5, expect.closeTo(180.8, 6)]);
    expect(linie.map((p) => p[1])).toEqual([52, 52.2, 52.4, 52.6]);
  });
});

describe("Resume-Schutz (B13)", () => {
  it("folgt nicht auf die Ladeposition, solange der Flug gerade fortgesetzt wurde", async () => {
    const k = await karte({ flug: { ...flug, was_just_resumed: true } });
    expect(mitMitte(), "keine Kamerafahrt zur Ladeposition").toHaveLength(0);
    expect(eigene().lngLat, "die Marke steht trotzdem an der Position").toEqual([28.7, 41.2]);
    // Gegenstueck: ist der Resume-Schutz vorbei, zentriert die Karte auf die frische Position.
    await k.neu({ flug: { ...flug, was_just_resumed: false } });
    expect(mitMitte().at(-1)!.arg.center).toEqual([28.7, 41.2]);
  });
});

describe("Marken werden nicht ständig neu gebaut (G7)", () => {
  it("die Flugzeug-Marke und die Pins bleiben ueber viele Takte dieselben", async () => {
    const k = await karte();
    const marke = eigene();
    for (let i = 1; i <= 5; i++) await k.neu({ snap: snap(41.2 + i * 0.01, 28.7 - i * 0.01) });
    expect(eigene()).toBe(marke);
    expect(h.marker.filter((m) => m.el.className === "aa-ac-marker")).toHaveLength(1);
    expect(pins(), "Pins wurden nur einmal gebaut").toHaveLength(2);
    expect(marke.lngLat).toEqual([28.7 - 0.05, 41.2 + 0.05]);
  });
});
