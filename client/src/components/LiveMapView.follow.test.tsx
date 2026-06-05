// Deterministischer Schutz für die Live-Karten-„Follow"-Kamera (v0.15.6).
//
// Hintergrund: bis v0.15.5 hat jede Karten-Bewegung (Pan/Zoom/Scroll) das
// Verfolgen HEIMLICH für 15 s abgeschaltet — der Haken blieb gesetzt, die Karte
// folgte aber nicht (Michael: „Flugzeug nie im Bild" bei Boarding + Takeoff).
//
// Diese Tests nageln das korrigierte Verhalten fest, OHNE Sim/WebGL (MapLibre
// gemockt, läuft in jsdom/CI):
//   1. Follow=an → Karte zentriert auf das Flugzeug und folgt Positions-Updates.
//   2. Ein echter Nutzer-Pan (dragstart mit originalEvent) → Follow geht
//      SICHTBAR aus (Haken weg) und das Zentrieren stoppt.
//   3. Der „🎯 Flugzeug"-Knopf zentriert wieder und schaltet Follow an.

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, act, cleanup } from "@testing-library/react";

// Gemeinsamer mutbarer Zustand für den maplibre-Mock (vi.hoisted, damit die
// Mock-Factory ihn referenzieren darf — sie wird an den Modulanfang gehoistet).
const h = vi.hoisted(() => ({
  mapHandlers: {} as Record<string, Array<(e?: unknown) => void>>,
  easeTo: [] as Array<{ center?: [number, number]; zoom?: number }>,
  jumpTo: [] as Array<{ center?: [number, number]; zoom?: number }>,
}));

vi.mock("maplibre-gl", () => {
  class FakeMarker {
    el: HTMLElement;
    constructor(opts?: { element?: HTMLElement }) {
      this.el = opts?.element ?? document.createElement("div");
    }
    setLngLat() { return this; }
    setRotation() { return this; }
    addTo() { return this; }
    remove() { return this; }
    getElement() { return this.el; }
  }
  class FakePopup {
    setLngLat() { return this; }
    setHTML() { return this; }
    addTo() { return this; }
    remove() { return this; }
    on() { return this; }
  }
  class FakeBounds { extend() { return this; } }
  class FakeNav {}
  class FakeMap {
    center = { lng: 6, lat: 48 };
    zoom = 4;
    addControl() { return this; }
    on(ev: string, cb: (e?: unknown) => void) {
      (h.mapHandlers[ev] ||= []).push(cb);
      return this;
    }
    getCenter() { return this.center; }
    getZoom() { return this.zoom; }
    isStyleLoaded() { return true; }
    setStyle() { return this; }
    addSource() { return this; }
    addLayer() { return this; }
    getSource() { return { setData: () => {} }; }
    getLayer() { return {}; }
    easeTo(o: { center?: [number, number]; zoom?: number }) {
      h.easeTo.push(o);
      if (o.center) this.center = { lng: o.center[0], lat: o.center[1] };
      if (o.zoom != null) this.zoom = o.zoom;
      return this;
    }
    jumpTo(o: { center?: [number, number]; zoom?: number }) {
      h.jumpTo.push(o);
      if (o.center) this.center = { lng: o.center[0], lat: o.center[1] };
      if (o.zoom != null) this.zoom = o.zoom;
      return this;
    }
    remove() { return this; }
  }
  return {
    default: {
      Map: FakeMap,
      Marker: FakeMarker,
      Popup: FakePopup,
      NavigationControl: FakeNav,
      LngLatBounds: FakeBounds,
    },
  };
});

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === "flight_get_route_fixes") return [];
    if (cmd === "va_live_flights") return { data: [] };
    if (cmd === "airport_get") return { lat: null, lon: null };
    return null;
  }),
}));
vi.mock("./ActivityLogPanel", () => ({ ActivityLogPanel: () => null }));
vi.mock("../lib/trackStore", () => ({ getTrack: () => [] }));
vi.mock("../lib/aircraftIcon", () => ({ aircraftSvg: () => "<svg class='aircraft-svg'></svg>" }));
vi.mock("../lib/phaseColors", () => ({ phaseColor: () => "#ffffff", phaseLabel: () => "Steigflug" }));

import { LiveMapView } from "./LiveMapView";

const snap = (lat: number, lon: number) => ({
  lat,
  lon,
  altitude_msl_ft: 6000,
  groundspeed_kt: 280,
  indicated_airspeed_kt: 250,
  heading_deg_true: 295,
  heading_deg_magnetic: 295,
  aircraft_icao: "A21N",
  timestamp: 0,
});

const flight = {
  pirep_id: "P1",
  airline_icao: "THY",
  flight_number: "155",
  dpt_airport: "LTFM",
  arr_airport: "LYTV",
  phase: "climb",
  distance_nm: 120,
  was_just_resumed: false,
};

function fireLoad() {
  act(() => {
    (h.mapHandlers["load"] ?? []).forEach((cb) => cb());
  });
}

beforeEach(() => {
  h.mapHandlers = {};
  h.easeTo.length = 0;
  h.jumpTo.length = 0;
  document.documentElement.dataset.theme = "dark";
});
afterEach(() => cleanup());

describe("LiveMapView Follow-Kamera", () => {
  it("zentriert bei Follow=an auf das Flugzeug und folgt Positions-Updates", () => {
    const { rerender } = render(
      <LiveMapView activeFlight={flight as never} simSnapshot={snap(41.2, 28.7) as never} />,
    );
    fireLoad();
    // großer Versatz (Default 6/48 → Istanbul) → harter jumpTo auf den Flieger
    expect(h.jumpTo.length).toBeGreaterThan(0);
    const c0 = h.jumpTo.at(-1)!.center!;
    expect(c0[0]).toBeCloseTo(28.7, 1);
    expect(c0[1]).toBeCloseTo(41.2, 1);

    // kleiner Schritt → sanftes easeTo auf die neue Position
    h.easeTo.length = 0;
    act(() => {
      rerender(<LiveMapView activeFlight={flight as never} simSnapshot={snap(41.4, 28.6) as never} />);
    });
    const c1 = h.easeTo.at(-1)!.center!;
    expect(c1[0]).toBeCloseTo(28.6, 1);
    expect(c1[1]).toBeCloseTo(41.4, 1);
    // laufendes Folgen darf den ZOOM NICHT mehr anfassen (dein manueller
    // Rauszoom bleibt erhalten — das war der zweite Teil des Bugs).
    expect(h.easeTo.at(-1)!.zoom).toBeUndefined();
  });

  it("zentriert auch aus weit entfernter/rausgezoomter Ansicht (Follow-Bug)", () => {
    const { rerender } = render(
      <LiveMapView activeFlight={flight as never} simSnapshot={snap(41.2, 28.7) as never} />,
    );
    fireLoad();
    // Erst-Lock erfolgt (jumpTo). Jetzt eine Position WEIT weg melden, so als
    // hätte man weit rausgezoomt und der Flieger ist quer über der Karte.
    h.jumpTo.length = 0;
    act(() => {
      rerender(<LiveMapView activeFlight={flight as never} simSnapshot={snap(60.0, 5.0) as never} />);
    });
    // Follow MUSS hart auf den Flieger zentrieren — egal wie weit weg.
    expect(h.jumpTo.length).toBeGreaterThan(0);
    const c = h.jumpTo.at(-1)!.center!;
    expect(c[0]).toBeCloseTo(5.0, 1);
    expect(c[1]).toBeCloseTo(60.0, 1);
  });

  it("ein Nutzer-Pan schaltet Follow sichtbar AUS und stoppt das Zentrieren", () => {
    const { rerender } = render(
      <LiveMapView activeFlight={flight as never} simSnapshot={snap(41.2, 28.7) as never} />,
    );
    fireLoad();
    const follow = screen.getByRole("button", { name: "Folgen" });
    expect(follow.getAttribute("aria-pressed")).toBe("true");

    // echter Pan (originalEvent gesetzt = Nutzergeste)
    act(() => {
      (h.mapHandlers["dragstart"] ?? []).forEach((cb) => cb({ originalEvent: {} }));
    });
    expect(follow.getAttribute("aria-pressed")).toBe("false");

    // weiteres Positions-Update darf NICHT mehr zentrieren
    h.easeTo.length = 0;
    h.jumpTo.length = 0;
    act(() => {
      rerender(<LiveMapView activeFlight={flight as never} simSnapshot={snap(43.0, 28.0) as never} />);
    });
    expect(h.easeTo.length).toBe(0);
    expect(h.jumpTo.length).toBe(0);
  });

  it("der „🎯 Flugzeug\"-Knopf zentriert wieder und schaltet Follow an", () => {
    render(<LiveMapView activeFlight={flight as never} simSnapshot={snap(41.2, 28.7) as never} />);
    fireLoad();
    // erst Follow per Pan ausschalten
    act(() => {
      (h.mapHandlers["dragstart"] ?? []).forEach((cb) => cb({ originalEvent: {} }));
    });
    const follow = screen.getByRole("button", { name: "Folgen" });
    expect(follow.getAttribute("aria-pressed")).toBe("false");

    h.easeTo.length = 0;
    const btn = screen.getByRole("button", { name: /Flugzeug/ });
    act(() => fireEvent.click(btn));

    expect(follow.getAttribute("aria-pressed")).toBe("true");
    const c = h.easeTo.at(-1)!.center!;
    expect(c[0]).toBeCloseTo(28.7, 1);
    expect(c[1]).toBeCloseTo(41.2, 1);
  });
});
