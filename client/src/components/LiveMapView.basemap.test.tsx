// Deterministischer Schutz fürs Basemap-Umschalten der Live-Karte (v0.16.27).
//
// Bug (Thomas): Satellit ↔ dunkle Karte umschalten — beim ERSTEN Mal blieb die
// Route da, beim ZWEITEN war sie weg (bis Tab-Wechsel). Ursache: `setStyle()`
// wirft alle Layer weg; ein EINMALIGER Re-Add (v0.16.26-Poll) feuert, während
// der alte Overlay-Layer noch existiert → addOverlays' „nur-wenn-fehlt"-Guard
// überspringt → danach wischt setStyle den Layer → Route weg. Beim 1. Mal gibt
// es keinen Alt-Layer, der den Skip auslöst → klappt scheinbar.
//
// Dieser Test stellt die Race NACH (FakeMap mit VERZÖGERTEM Layer-Wipe) und
// prüft, dass beim MEHRFACHEN Umschalten sowohl der Route-LAYER als auch die
// echte Route-GEOMETRIE (LineString-Feature in der Source) erhalten bleiben.
// Mit dem alten Einmal-Poll wäre der Layer ab dem 2. Umschalten weg.

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup } from "@testing-library/react";

const h = vi.hoisted(() => ({
  map: null as null | { layers: Set<string>; sourceData: Map<string, { features?: unknown[] }> },
  handlers: {} as Record<string, Array<(e?: unknown) => void>>,
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
    getCenter() { return this.center; }
    getZoom() { return this.zoom; }
    isStyleLoaded() { return true; } // immer „geladen" → modelliert das Race-Fenster direkt nach setStyle
    getLayer(id: string) { return this.layers.has(id) ? ({} as unknown) : undefined; }
    getSource(id: string) {
      if (!this.sources.has(id)) return undefined;
      return { setData: (d: { features?: unknown[] }) => { this.sourceData.set(id, d); } } as unknown;
    }
    addLayer(o: { id: string }) { this.layers.add(o.id); return this; }
    addSource(id: string) { this.sources.add(id); return this; }
    removeLayer(id: string) { this.layers.delete(id); return this; }
    removeSource(id: string) { this.sources.delete(id); this.sourceData.delete(id); return this; }
    setStyle() {
      // RACE-MODELL: setStyle wischt Quellen+Layer (und ihre Daten) VERZÖGERT,
      // nicht sofort. So existiert der alte Overlay-Layer noch, wenn ein
      // einmaliger Re-Add feuert (Guard überspringt), und verschwindet erst
      // 50 ms später — exakt die echte Race.
      setTimeout(() => { this.layers.clear(); this.sources.clear(); this.sourceData.clear(); }, 50);
      return this;
    }
    easeTo() { return this; }
    jumpTo() { return this; }
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

const LYR_ROUTE = "aa-planned-route-line"; // Layer (siehe LiveMapView; "aa-planned-route" ist die SOURCE)
const SRC_ROUTE = "aa-planned-route"; // Source mit der Routen-Geometrie

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
function toggleBasemap() { act(() => { fireEvent.click(screen.getByLabelText("Satellitenkarte umschalten")); }); }

// die echte, vom Nutzer sichtbare Route: ein LineString-Feature in der Source
function routeGeometryCount() {
  const fc = h.map!.sourceData.get(SRC_ROUTE);
  return (fc?.features ?? []).length;
}
function layerPresent() { return h.map!.layers.has(LYR_ROUTE); }

beforeEach(() => {
  h.map = null;
  h.handlers = {};
  document.documentElement.dataset.theme = "dark";
  localStorage.clear();
  vi.useFakeTimers();
});
afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
  cleanup();
});

describe("LiveMapView Basemap-Umschalten — Overlays + Geometrie bleiben (v0.16.27)", () => {
  it("Route-Layer UND Routen-Geometrie überleben MEHRFACHES Umschalten", async () => {
    render(<LiveMapView activeFlight={flight as never} simSnapshot={snap as never} />);
    fireLoad();
    await flushAsync(); // Routen-Fixes laden
    settle(600);
    expect(layerPresent()).toBe(true);
    expect(routeGeometryCount()).toBeGreaterThan(0); // echte Route gezeichnet

    toggleBasemap(); // 1. Umschalten (dunkel → Satellit) — klappte schon vorher
    settle(600);
    expect(layerPresent()).toBe(true);
    expect(routeGeometryCount()).toBeGreaterThan(0);

    toggleBasemap(); // 2. Umschalten (Satellit → dunkel) — HIER verschwand v0.16.26
    settle(600);
    expect(layerPresent()).toBe(true);
    expect(routeGeometryCount()).toBeGreaterThan(0);

    toggleBasemap(); // 3. Umschalten
    settle(600);
    expect(layerPresent()).toBe(true);
    expect(routeGeometryCount()).toBeGreaterThan(0);

    toggleBasemap(); // 4. Umschalten — bleibt stabil
    settle(600);
    expect(layerPresent()).toBe(true);
    expect(routeGeometryCount()).toBeGreaterThan(0);
  });

  it("selbstkorrigierend: ein während des Race-Fensters weggewischter Layer kommt zurück", async () => {
    render(<LiveMapView activeFlight={flight as never} simSnapshot={snap as never} />);
    fireLoad();
    await flushAsync();
    settle(600);
    toggleBasemap();
    settle(600); // erholt sich nach dem verzögerten Wipe + Re-Add
    expect(layerPresent()).toBe(true);
    expect(routeGeometryCount()).toBeGreaterThan(0);
  });
});
