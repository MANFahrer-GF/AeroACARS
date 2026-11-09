// Die Taxi-Karte: wie sie gezeichnet wird und was der Schalter „Taxiweg“ tut.
//
// Abnahmeliste Live-Karte, Zeilen C2 und C3 (docs/spec/livekarte-umbau-abnahme.md).
// maplibre-gl ist ein Nachbau, der aufzeichnet, was die Karte angelegt und gesetzt bekommt:
// die Spezifikation jeder Ebene, jedes `setPaintProperty`/`setLayoutProperty` und jedes `setData`.

import { describe, it, expect, beforeEach, afterEach, beforeAll, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../../../../bundle/texte/de.json";

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

type Spec = {
  id: string; type: string; source?: string; minzoom?: number; filter?: unknown;
  layout?: Record<string, unknown>; paint?: Record<string, unknown>;
};
const h = vi.hoisted(() => ({
  handlers: [] as Array<{ ev: string; cb: (e?: unknown) => void }>,
  ebenen: new Map<string, Record<string, unknown>>(),
  quellen: new Set<string>(),
  daten: new Map<string, { type: string; features: unknown[] }>(),
  sichtbar: new Map<string, string>(),
  farben: new Map<string, unknown>(),
  mitte: { lng: 11.8, lat: 48.3 },
  zoom: 12,
  index: [] as Array<{ icao: string; lat: number; lon: number }>,
  boden: {} as Record<string, unknown>,
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
    addControl() { return this; }
    on(ev: string, cb: (e?: unknown) => void) { h.handlers.push({ ev, cb }); return this; }
    once() { return this; }
    off(ev: string, cb: (e?: unknown) => void) { h.handlers = h.handlers.filter((x) => !(x.ev === ev && x.cb === cb)); return this; }
    getCanvas() { return { style: {} }; }
    getBounds() {
      return {
        getSouth: () => h.mitte.lat - 1, getNorth: () => h.mitte.lat + 1,
        getWest: () => h.mitte.lng - 1, getEast: () => h.mitte.lng + 1,
      };
    }
    getCenter() { return h.mitte; }
    getZoom() { return h.zoom; }
    getBearing() { return 0; }
    isStyleLoaded() { return true; }
    getLayer(id: string) { return h.ebenen.get(id); }
    addLayer(spec: Spec) {
      h.ebenen.set(spec.id, spec as unknown as Record<string, unknown>);
      if (spec.layout?.visibility) h.sichtbar.set(spec.id, String(spec.layout.visibility));
      return this;
    }
    addSource(id: string) { h.quellen.add(id); return this; }
    getSource(id: string) {
      if (!h.quellen.has(id)) return undefined;
      return { setData: (d: { type: string; features: unknown[] }) => { h.daten.set(id, d); } };
    }
    setFilter() { return this; }
    setLayoutProperty(id: string, k: string, v: string) { if (k === "visibility") h.sichtbar.set(id, v); return this; }
    setPaintProperty(id: string, k: string, v: unknown) { h.farben.set(`${id}|${k}`, v); return this; }
    addSprite() { return this; }
    removeSprite() { return this; }
    getGlyphs() { return ""; }
    setGlyphs() { return this; }
    removeLayer() { return this; }
    removeSource() { return this; }
    hasImage() { return false; }
    addImage() { return this; }
    easeTo() { return this; }
    jumpTo() { return this; }
    flyTo() { return this; }
    fitBounds() { return this; }
    resize() { return this; }
    remove() { return this; }
  }
  return { default: { Map: FakeMap, Marker: FakeMarker, Popup: FakePopup, NavigationControl: FakeNav, LngLatBounds: FakeBounds } };
});

vi.mock("../../lib/ipc", () => ({
  isTauri: false,
  invoke: vi.fn(async (cmd: string, args?: { icao?: string }) => {
    if (cmd === "airport_ground_index") return h.index;
    if (cmd === "airport_ground_get") return h.boden[args?.icao ?? ""] ?? null;
    if (cmd === "va_live_flights") return { data: [] };
    if (cmd === "flight_get_route_fixes") return [];
    if (cmd === "flight_get_track") return [];
    if (cmd === "airport_get") return { lat: null, lon: null };
    if (cmd === "activity_log_get") return [];
    if (cmd === "hoppie_get_thread") return [];
    return null;
  }),
  listen: vi.fn(async () => () => {}),
  openExternal: vi.fn(async () => {}),
}));
vi.mock("../ActivityLogPanel", () => ({ ActivityLogPanel: () => null }));
vi.mock("../../lib/trackStore", () => ({ getTrack: () => [], setTrack: () => {} }));
vi.mock("../../lib/aircraftIcon", () => ({ aircraftSvg: () => "<svg></svg>" }));
vi.mock("../../lib/phaseColors", () => ({ phaseColor: () => "#ffffff", phaseLabel: () => "Steigflug" }));

import { LiveMapView } from "../LiveMapView";

const snap = {
  lat: 48.3, lon: 11.8, altitude_msl_ft: 1500, groundspeed_kt: 20, indicated_airspeed_kt: 20,
  heading_deg_true: 80, heading_deg_magnetic: 80, aircraft_icao: "A21N", timestamp: 0,
};
const flight = {
  pirep_id: "P1", airline_icao: "GSG", flight_number: "1", dpt_airport: "EDDM",
  arr_airport: "EDDF", phase: "taxi_out", distance_nm: 0, was_just_resumed: false,
};

beforeEach(() => {
  h.handlers = [];
  h.ebenen.clear();
  h.quellen.clear();
  h.daten.clear();
  h.sichtbar.clear();
  h.farben.clear();
  h.mitte = { lng: 11.8, lat: 48.3 };
  h.zoom = 12;
  h.index = [];
  h.boden = {};
  localStorage.clear();
  document.documentElement.dataset.theme = "dark";
  vi.stubGlobal("fetch", () => new Promise(() => {})); // Kartenstile: eigener Test
  // jsdom kennt keine Zeichenflaeche; das Flieger-Symbol des Netzes braucht eine, also ein Stummel.
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    clearRect() {}, beginPath() {}, moveTo() {}, lineTo() {}, closePath() {}, fill() {}, stroke() {},
    getImageData: () => ({ width: 48, height: 48, data: new Uint8ClampedArray(48 * 48 * 4) }),
  } as unknown as CanvasRenderingContext2D);
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function karte() {
  render(<LiveMapView activeFlight={flight as never} simSnapshot={snap as never} simKind={undefined} onSwitchToBriefing={() => {}} />);
  act(() => { h.handlers.filter((x) => x.ev === "load").forEach((x) => x.cb()); });
  await waitFor(() => expect(h.ebenen.has("aa-ground-apron")).toBe(true));
  // Der Farben-Effekt laeuft, sobald die Karte bereit ist.
  await waitFor(() => expect(h.farben.has("aa-ground-apron|line-color")).toBe(true));
}
const spec = (id: string) => h.ebenen.get(id) as unknown as Spec;
const knopf = (name: string) => screen.getByRole("button", { name });

describe("Taxi-Karte: Darstellung (C2)", () => {
  it("zeichnet Rollwege gruen, farbcodierte blau oder orange, ab Zoom 12 mit runden Enden", async () => {
    await karte();
    const s = spec("aa-ground-taxi");
    expect(s.type).toBe("line");
    expect(s.minzoom).toBe(12);
    expect(s.filter).toEqual(["==", ["get", "k"], "taxiway"]);
    expect(s.layout).toEqual({ "line-cap": "round", "line-join": "round" });
    expect(s.paint!["line-color"]).toEqual([
      "case", ["==", ["get", "c"], "blue"], "#3b82f6", ["==", ["get", "c"], "orange"], "#f97316", "#4ade80",
    ]);
    expect(s.paint!["line-width"]).toEqual(["interpolate", ["linear"], ["zoom"], 12, 1.2, 16, 3.5, 18, 6]);
    expect(s.paint!["line-opacity"]).toBe(0.95);
  });

  it("zeichnet Stand-Zuwege blau ab Zoom 13 und die Standplaetze als blaue Punkte ab Zoom 14", async () => {
    await karte();
    const zuwege = spec("aa-ground-stand-lanes");
    expect(zuwege.type).toBe("line");
    expect(zuwege.minzoom).toBe(13);
    expect(zuwege.paint!["line-color"]).toBe("#60a5fa");
    expect(zuwege.paint!["line-width"]).toEqual(["interpolate", ["linear"], ["zoom"], 13, 0.5, 16, 1.6, 18, 3]);
    const staende = spec("aa-ground-stands");
    expect(staende.type).toBe("circle");
    expect(staende.minzoom).toBe(14);
    expect(staende.paint!["circle-color"]).toBe("#60a5fa");
    expect(staende.paint!["circle-radius"]).toEqual(["interpolate", ["linear"], ["zoom"], 14, 1.5, 18, 4]);
    expect(staende.filter).toEqual([
      "all", ["in", ["get", "k"], ["literal", ["gate", "parking_position"]]], ["==", ["geometry-type"], "Point"],
    ]);
  });

  it("zeichnet Haltepunkte gelb mit dunklem Rand ab Zoom 13", async () => {
    await karte();
    const s = spec("aa-ground-holding");
    expect(s.type).toBe("circle");
    expect(s.minzoom).toBe(13);
    expect(s.filter).toEqual(["==", ["get", "k"], "holding_position"]);
    expect(s.paint!["circle-color"]).toBe("#facc15");
    expect(s.paint!["circle-stroke-color"]).toBe("#0b0f16");
    expect(s.paint!["circle-radius"]).toEqual(["interpolate", ["linear"], ["zoom"], 13, 2, 18, 6]);
  });

  it("zeichnet Bahnen ab Zoom 12 mit der Bezeichnung entlang der Bahn in Weiss", async () => {
    await karte();
    const bahn = spec("aa-ground-runway");
    expect(bahn.type).toBe("line");
    expect(bahn.minzoom).toBe(12);
    expect(bahn.filter).toEqual(["==", ["get", "k"], "runway"]);
    expect(bahn.paint!["line-width"]).toEqual(["interpolate", ["linear"], ["zoom"], 12, 3, 16, 12, 18, 24]);
    const name = spec("aa-ground-runway-labels");
    expect(name.type).toBe("symbol");
    expect(name.minzoom).toBe(12);
    expect(name.layout!["symbol-placement"]).toBe("line");
    expect(name.layout!["text-field"]).toEqual(["get", "r"]);
    expect(name.paint!["text-color"]).toBe("#ffffff");
    expect(name.paint!["text-halo-color"]).toBe("#0b0f16");
  });

  it("zeichnet Terminals als Umriss ab Zoom 13", async () => {
    await karte();
    const s = spec("aa-ground-terminal");
    expect(s.type).toBe("line");
    expect(s.minzoom).toBe(13);
    expect(s.filter).toEqual(["==", ["get", "k"], "terminal"]);
    expect(s.paint!["line-width"]).toBe(1.4);
    expect(s.paint!["line-opacity"]).toBe(0.85);
  });

  it("beschriftet je Zoom: Bahnen ab 12, Rollwege ab 14 in der Bodenfarbe, Staende ab 15", async () => {
    await karte();
    const rollwege = spec("aa-ground-taxi-labels");
    expect(rollwege.minzoom).toBe(14);
    expect(rollwege.filter).toEqual(["all", ["==", ["get", "k"], "taxiway"], ["has", "r"]]);
    expect(rollwege.layout!["text-size"]).toEqual(["interpolate", ["linear"], ["zoom"], 14, 9, 18, 14]);
    expect(rollwege.paint!["text-color"]).toEqual([
      "case", ["==", ["get", "c"], "blue"], "#93c5fd", ["==", ["get", "c"], "orange"], "#fdba74", "#eaf5ec",
    ]);
    const staende = spec("aa-ground-stand-labels");
    expect(staende.minzoom).toBe(15);
    expect(staende.layout!["text-field"]).toEqual(["get", "r"]);
    expect(staende.layout!["text-size"]).toEqual(["interpolate", ["linear"], ["zoom"], 15, 8, 18, 12]);
    expect(staende.paint!["text-color"]).toBe("#dbeafe");
    expect(spec("aa-ground-runway-labels").minzoom).toBe(12);
  });

  it("liegt mit allen zehn Ebenen unter Netz, Kollegen-Routen, Route und Spur", async () => {
    await karte();
    const reihenfolge = [...h.ebenen.keys()];
    const boden = reihenfolge.filter((id) => spec(id).source === "aa-ground");
    expect(boden).toHaveLength(10);
    const ersteOben = Math.min(
      ...["vatsim-sectors-fill", "fremde-routen-line", "aa-planned-route-line", "aa-flown-track-line"].map((id) => {
        const i = reihenfolge.indexOf(id);
        expect(i, id).toBeGreaterThanOrEqual(0);
        return i;
      }),
    );
    for (const id of boden) expect(reihenfolge.indexOf(id), id).toBeLessThan(ersteOben);
  });

  it("faerbt Vorfeld, Bahnen und Terminals auf der dunklen Karte und auf Satellit verschieden und kehrt zurueck", async () => {
    await karte();
    const farbe = (id: string, k: string) => h.farben.get(`${id}|${k}`);
    const dunkel = () => ({
      vorfeld: [farbe("aa-ground-apron", "line-color"), farbe("aa-ground-apron", "line-opacity")],
      bahn: [farbe("aa-ground-runway", "line-color"), farbe("aa-ground-runway", "line-opacity")],
      terminal: farbe("aa-ground-terminal", "line-color"),
    });
    expect(dunkel()).toEqual({ vorfeld: ["#3a4a5c", 0.7], bahn: ["#c9ccd1", 0.9], terminal: "#8b7fd4" });
    fireEvent.click(knopf("Satellit"));
    await waitFor(() => expect(farbe("aa-ground-apron", "line-color")).toBe("#8aa0b4"));
    expect(dunkel()).toEqual({ vorfeld: ["#8aa0b4", 0.5], bahn: ["#e8edf2", 0.75], terminal: "#c4b5fd" });
    fireEvent.click(knopf("Karte"));
    await waitFor(() => expect(farbe("aa-ground-apron", "line-color")).toBe("#3a4a5c"));
    expect(dunkel()).toEqual({ vorfeld: ["#3a4a5c", 0.7], bahn: ["#c9ccd1", 0.9], terminal: "#8b7fd4" });
  });
});

describe("Schalter „Taxiweg“ (C3)", () => {
  const MUC = { icao: "EDDM", geojson: JSON.stringify({ type: "FeatureCollection", features: [
    { type: "Feature", properties: { k: "taxiway", r: "A" }, geometry: { type: "LineString", coordinates: [[11.8, 48.3], [11.81, 48.31]] } },
  ] }) };

  const bodenEbenen = () => [...h.ebenen.keys()].filter((id) => spec(id).source === "aa-ground");

  it("blendet alle zehn Bodenebenen aus und wieder ein, die Daten bleiben geladen", async () => {
    h.index = [{ icao: "EDDM", lat: 48.35, lon: 11.78 }];
    h.boden = { EDDM: MUC };
    await karte();
    await waitFor(() => expect(h.daten.get("aa-ground")?.features.length).toBe(1));
    expect(bodenEbenen()).toHaveLength(10);
    const stand = () => bodenEbenen().map((id) => h.sichtbar.get(id));
    expect(stand()).toEqual(Array(10).fill("visible"));
    expect(knopf("Taxiweg").getAttribute("aria-pressed")).toBe("true");

    fireEvent.click(knopf("Taxiweg"));
    expect(stand()).toEqual(Array(10).fill("none"));
    expect(knopf("Taxiweg").getAttribute("aria-pressed")).toBe("false");
    expect(h.daten.get("aa-ground")?.features.length).toBe(1);

    fireEvent.click(knopf("Taxiweg"));
    expect(stand()).toEqual(Array(10).fill("visible"));
  });

  it("nennt im Tooltip die geladenen Plaetze, sortiert, und ohne Plaetze nur den Namen", async () => {
    h.index = [{ icao: "EDDM", lat: 48.35, lon: 11.78 }, { icao: "ETSI", lat: 48.62, lon: 11.53 }, { icao: "EDDF", lat: 50.03, lon: 8.57 }];
    h.boden = { EDDM: MUC, ETSI: { icao: "ETSI", geojson: MUC.geojson }, EDDF: { icao: "EDDF", geojson: MUC.geojson } };
    h.zoom = 4; // weit draussen: nichts wird geladen
    await karte();
    expect(knopf("Taxiweg").getAttribute("title")).toBe("Taxiweg");

    h.zoom = 12;
    act(() => { h.handlers.filter((x) => x.ev === "moveend").forEach((x) => x.cb()); });
    // Im Bild liegen EDDM und ETSI, nicht EDDF.
    await waitFor(() => expect(knopf("Taxiweg").getAttribute("title")).toBe("Rollwege geladen: EDDM · ETSI"));
  });
});
