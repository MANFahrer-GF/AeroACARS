// Grundkarte der Live-Karte: Der Wechsel schaltet nur Sichtbarkeiten.
//
// Frueher tauschte jeder Wechsel den ganzen Stil; dabei gingen Route und Ebenen verloren (v0.16.27,
// ab dem zweiten Umschalten) und die Weltkugel wurde flach. Dieser Test sichert das Verhalten am
// Bauteil: `setStyle` faellt nie, die Overlays bleiben dieselben, die Geometrie bleibt erhalten.

import { describe, it, expect, beforeEach, afterEach, beforeAll, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../bundle/texte/de.json";

// This test predates the Livemap-4a chrome rewrite, which added real i18n
// copy to LiveMapView — real i18next (same setup as CpdlcPanel.test.tsx),
// this file only checks map-engine behavior (layer/geometry survival across
// basemap switches), not translated copy, but the component now calls
// useTranslation() unconditionally so it needs a real instance to render.
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

const h = vi.hoisted(() => ({
  map: null as null | { layers: Set<string>; sichtbar: Map<string, string>; sourceData: Map<string, { features?: unknown[] }> },
  handlers: {} as Record<string, Array<(e?: unknown) => void>>,
  setStyle: 0,
  angelegt: 0,
  farben: new Map<string, unknown>(),
}));

vi.mock("maplibre-gl", () => {
  class FakeMarker {
    el: HTMLElement;
    constructor(opts?: { element?: HTMLElement }) { this.el = opts?.element ?? document.createElement("div"); }
    setLngLat() { return this; }
    setRotation() { return this; }
    addTo() { return this; }
    remove() { return this; }
    getElement() { return this.el; }
  }
  class FakePopup { setLngLat() { return this; } setHTML() { return this; } addTo() { return this; } remove() { return this; } on() { return this; } }
  class FakeBounds { extend() { return this; } }
  class FakeNav {}
  class FakeMap {
    layers = new Set<string>();
    sources = new Set<string>();
    sourceData = new Map<string, { features?: unknown[] }>(); // gespeicherte setData-Payloads
    center = { lng: 6, lat: 48 };
    zoom = 4;
    constructor() { h.map = this; }
    addControl() { return this; }
    on(ev: string, cb: (e?: unknown) => void) { (h.handlers[ev] ||= []).push(cb); return this; }
    once() { return this; }
    // v0.21: die Taxi-Karte haengt sich an moveend/zoomend und meldet sich beim
    // Aufraeumen wieder ab — der Mock muss das koennen, sonst kracht der Unmount.
    off(ev: string, cb: (e?: unknown) => void) {
      const list = h.handlers[ev];
      if (list) h.handlers[ev] = list.filter((f) => f !== cb);
      return this;
    }
    // Sichtbereich — die Taxi-Karte laedt, was gerade im Bild ist.
    getBounds() {
      return {
        getSouth: () => this.center.lat - 1,
        getNorth: () => this.center.lat + 1,
        getWest: () => this.center.lng - 1,
        getEast: () => this.center.lng + 1,
      };
    }
    getCenter() { return this.center; }
    getZoom() { return this.zoom; }
    isStyleLoaded() { return true; } // immer „geladen" → modelliert das Race-Fenster direkt nach setStyle
    getLayer(id: string) { return this.layers.has(id) ? ({} as unknown) : undefined; }
    sichtbar = new Map<string, string>();
    setLayoutProperty(id: string, _k: string, v: string) { this.sichtbar.set(id, v); return this; }
    setPaintProperty(id: string, k: string, v: unknown) { h.farben.set(`${id}|${k}`, v); return this; }
    addSprite() { return this; }
    removeSprite() { return this; }
    getGlyphs() { return ""; }
    setGlyphs() { return this; }
    getSource(id: string) {
      if (!this.sources.has(id)) return undefined;
      return { setData: (d: { features?: unknown[] }) => { this.sourceData.set(id, d); } } as unknown;
    }
    addLayer(o: { id: string; layout?: { visibility?: string } }) { this.layers.add(o.id); if (o.layout?.visibility) this.sichtbar.set(o.id, o.layout.visibility); h.angelegt += 1; return this; }
    addSource(id: string) { this.sources.add(id); return this; }
    hasImage() { return false; }
    addImage() { return this; }
    removeLayer(id: string) { this.layers.delete(id); return this; }
    removeSource(id: string) { this.sources.delete(id); this.sourceData.delete(id); return this; }
    setStyle() { h.setStyle += 1; return this; }
    easeTo() { return this; }
    jumpTo() { return this; }
    // v1.6.11: die Karte bleibt beim Reiterwechsel eingehaengt und misst sich
    // beim Wiederauftauchen neu. Der Mock kannte `resize` nicht — echtes
    // MapLibre hat es immer, die Luecke lag also im Mock, nicht im Code.
    resize() { return this; }
    remove() { return this; }
  }
  return { default: { Map: FakeMap, Marker: FakeMarker, Popup: FakePopup, NavigationControl: FakeNav, LngLatBounds: FakeBounds } };
});

vi.mock("../lib/ipc", () => ({
  isTauri: false,
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "flight_get_route_fixes") return [
      { ident: "WP1", lat: 49.4, lon: 9.2, kind: "wpt" },
      { ident: "WP2", lat: 48.8, lon: 10.1, kind: "wpt" },
      { ident: "WP3", lat: 48.4, lon: 11.0, kind: "wpt" },
    ];
    if (cmd === "flight_get_track") return [[9, 49], [9.5, 48.8]];
    if (cmd === "va_live_flights") return { data: [] };
    if (cmd === "airport_get") return { lat: null, lon: null };
    if (cmd === "airport_ground_index") return [];
    if (cmd === "airport_ground_get") return null;
    // Livemap-4a's Ereignisliste (useMapEvents) polls both of these.
    if (cmd === "activity_log_get") return [];
    if (cmd === "hoppie_get_thread") return [];
    return null;
  }),
  listen: vi.fn(async () => () => {}),
  openExternal: vi.fn(async () => {}),
}));
vi.mock("./ActivityLogPanel", () => ({ ActivityLogPanel: () => null }));
vi.mock("../lib/trackStore", () => ({ getTrack: () => [], setTrack: () => {} }));
vi.mock("../lib/aircraftIcon", () => ({ aircraftSvg: () => "<svg></svg>" }));
vi.mock("../lib/phaseColors", () => ({ phaseColor: () => "#ffffff", phaseLabel: () => "Steigflug" }));

import { LiveMapView } from "./LiveMapView";
import { karteQuelltext } from "./livekarte/quelltext.testhilfe";

const LYR_ROUTE = "aa-planned-route-line"; // Layer (siehe LiveMapView; "aa-planned-route" ist die SOURCE)
const SRC_ROUTE = "aa-planned-route"; // Source mit der Routen-Geometrie
const CASING = "aa-planned-route-casing";

const snap = {
  lat: 49, lon: 9, altitude_msl_ft: 6000, groundspeed_kt: 280, indicated_airspeed_kt: 250,
  heading_deg_true: 295, heading_deg_magnetic: 295, aircraft_icao: "A21N", timestamp: 0,
};
const flight = {
  pirep_id: "P1", airline_icao: "THY", flight_number: "155", dpt_airport: "LTFM",
  arr_airport: "LYTV", phase: "climb", distance_nm: 120, was_just_resumed: false,
};

function fireLoad() { act(() => { (h.handlers["load"] ?? []).forEach((cb) => cb()); }); }
function settle(ms = 600) { act(() => { vi.advanceTimersByTime(ms); }); }
async function flushAsync() {
  // Microtasks flushen, damit der invoke().then(setRouteFixes)-Fetch ankommt
  // (Fake-Timer faken keine Promises → echte Microtask-Runden).
  await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); });
}
// v1.3.5 (#Livemap-4a): the old single icon-toggle (aria-label
// "Satellitenkarte umschalten") became a two-button ANSICHT segmented
// control ("Karte" / "Satellit", README §3) — clicking whichever one isn't
// currently pressed reproduces the same alternating dark↔sat↔dark toggle
// this test cycles through.
function toggleBasemap() {
  act(() => {
    const map = screen.getByRole("button", { name: "Karte" });
    const sat = screen.getByRole("button", { name: "Satellit" });
    const inactive = map.getAttribute("aria-pressed") === "false" ? map : sat;
    fireEvent.click(inactive);
  });
}

// die echte, vom Nutzer sichtbare Route: ein LineString-Feature in der Source
function routeGeometryCount() {
  const fc = h.map!.sourceData.get(SRC_ROUTE);
  return (fc?.features ?? []).length;
}
function layerPresent() { return h.map!.layers.has(LYR_ROUTE); }

beforeEach(() => {
  h.map = null;
  h.handlers = {};
  h.setStyle = 0;
  h.angelegt = 0;
  h.farben.clear();
  const stil = (n: string) => ({ version: 8, sprite: `https://x/${n}`, sources: { protomaps: { type: "vector", tiles: ["https://x/{z}.mvt"] } }, layers: [{ id: "wasser", type: "fill", source: "protomaps", "source-layer": "w" }] });
  vi.stubGlobal("fetch", (url: string) => Promise.resolve({ ok: true, json: async () => stil(url.includes("dark") ? "d" : "h") }));
  document.documentElement.dataset.theme = "dark";
  localStorage.clear();
  vi.useFakeTimers();
});
afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  cleanup();
});

describe("LiveMapView Grundkarte", () => {
  async function bereit() {
    render(<LiveMapView activeFlight={flight as never} simSnapshot={snap as never} simKind={undefined} onSwitchToBriefing={() => {}} />);
    fireLoad();
    await flushAsync();
    settle(600);
    await flushAsync();
  }
  const karteSichtbar = () => h.map!.sichtbar.get("d:wasser");

  it("laesst Overlays und Routen-Geometrie stehen, auch nach mehrfachem Umschalten", async () => {
    await bereit();
    expect(layerPresent()).toBe(true);
    expect(routeGeometryCount()).toBeGreaterThan(0);
    const vorher = h.angelegt;
    for (let i = 0; i < 4; i++) {
      toggleBasemap();
      settle(600);
      expect(layerPresent()).toBe(true);
      expect(routeGeometryCount()).toBeGreaterThan(0);
    }
    expect(h.angelegt, "beim Umschalten darf nichts neu angelegt werden").toBe(vorher);
    expect(h.setStyle, "der Stil wird nie getauscht").toBe(0);
  });

  it("schaltet zwischen Karte und Satellit, die Routen-Unterlage nur auf Satellit", async () => {
    await bereit();
    expect(karteSichtbar()).toBe("visible");
    expect(h.map!.sichtbar.get("esri-imagery")).toBe("none");
    expect(h.map!.sichtbar.get(CASING)).toBe("none");
    toggleBasemap();
    settle(600);
    expect(karteSichtbar()).toBe("none");
    expect(h.map!.sichtbar.get("esri-imagery")).toBe("visible");
    expect(h.map!.sichtbar.get(CASING)).toBe("visible");
    expect(localStorage.getItem("aaLivemapBasemap")).toBe("sat");
  });

  it("zieht die Overlay-Farben mit: auf Satellit heller Track und dunkle Halos", async () => {
    await bereit();
    toggleBasemap();
    settle(600);
    expect(h.farben.get("aa-flown-track-line|line-color")).toBe("#f2f5fa");
    expect(h.farben.get("aa-planned-wpts-labels|text-halo-color")).toBe("#0b0f16");
  });

  it("folgt dem App-Theme, ohne den Stil zu tauschen", async () => {
    await bereit();
    expect(h.map!.sichtbar.get("h:wasser")).toBe("none");
    act(() => { document.documentElement.dataset.theme = "light"; });
    await flushAsync();
    settle(50);
    expect(h.map!.sichtbar.get("h:wasser")).toBe("visible");
    expect(karteSichtbar()).toBe("none");
    expect(h.setStyle).toBe(0);
  });

  it("legt die Bodendaten unter Netz, Kollegen-Routen und eigenen Flug", async () => {
    await bereit();
    const ebenen = [...h.map!.layers];
    const pos = (id: string) => { const i = ebenen.indexOf(id); expect(i, id).toBeGreaterThanOrEqual(0); return i; };
    expect(pos("aa-ground-stand-labels")).toBeLessThan(pos("vatsim-sectors-fill"));
    expect(pos("vatsim-sectors-fill")).toBeLessThan(pos(LYR_ROUTE));
  });

  it("holt die Stile neu, sobald der Server andere Adressen nennt", () => {
    expect(karteQuelltext()).toMatch(/grundkarteHolen\([\s\S]{0,240}\}, \[mapReady, grundlage\.dunkel, grundlage\.hell, grundlage\.schluessel\]\)/);
  });

  it("kennt weder Stiltausch noch Nachlege-Schleife mehr", () => {
    const q = karteQuelltext();
    for (const altlast of ["map.setStyle(", '"styledata"', "ensureOverlays", "addOverlays("]) expect(q).not.toContain(altlast);
  });

  it("nimmt den gemerkten Satellit beim Start, sobald die Karte steht", async () => {
    localStorage.setItem("aaLivemapBasemap", "sat");
    await bereit();
    expect(h.map!.sichtbar.get("esri-imagery")).toBe("visible");
    expect(karteSichtbar()).toBe("none");
  });
});
