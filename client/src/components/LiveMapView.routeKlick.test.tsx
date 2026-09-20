// Der Klick auf einen Kollegen — WIRKLICH geklickt, nicht nachgelesen.
//
// Thomas, 20.09.2026: „das mit dem anklicken der route geht nicht … was
// testest du eigentlich". Der Vorwurf saß: Alle bisherigen Prüfungen
// dieses Features waren Quelltext-Wächter (`quelle.indexOf(...)`) und
// Server-Tests. Keine einzige hat je auf einen Marker geklickt. Deshalb
// blieben zwei Fehler unbemerkt, die beim ersten echten Klick auffielen:
// der Server filterte auf eine Spalte, die nie gefüllt ist, und der
// Hinweis lag hinter der Kartenschalter-Leiste.
//
// Dieser Test rendert die Karte, klickt auf das Marker-Element eines
// Kollegen und sieht nach, was tatsächlich passiert:
//   1. Der Klick fragt die Route unter DIESER Kennung ab.
//   2. Kommt eine Route, wird sie gezeichnet; ein zweiter Klick nimmt
//      sie wieder weg.
//   3. Kommt keine, steht der Hinweis im Bild — sichtbar, nicht nur
//      vorhanden.

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { render, screen, act, cleanup, waitFor } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";

const h = vi.hoisted(() => ({
  marker: [] as Array<{ el: HTMLElement }>,
  quellen: {} as Record<string, { type: string; features: unknown[] }>,
  invokeAntworten: {} as Record<string, unknown>,
  invokeRufe: [] as Array<{ cmd: string; args?: Record<string, unknown> }>,
}));

vi.mock("../lib/ipc", () => ({
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    h.invokeRufe.push({ cmd, args });
    const a = h.invokeAntworten[cmd];
    if (a instanceof Error) return Promise.reject(a);
    return Promise.resolve(a ?? null);
  },
}));

vi.mock("maplibre-gl", () => {
  class FakeMarker {
    el: HTMLElement;
    constructor(opts?: { element?: HTMLElement }) {
      this.el = opts?.element ?? document.createElement("div");
      h.marker.push(this);
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
  class FakeMap {
    center = { lng: 6, lat: 48 };
    zoom = 4;
    addControl() { return this; }
    on(ev: string, cb: (e?: unknown) => void) {
      if (ev === "load" || ev === "styledata") cb();
      return this;
    }
    once() { return this; }
    off() { return this; }
    getBounds() {
      return {
        getSouth: () => 40, getNorth: () => 60, getWest: () => 0, getEast: () => 20,
      };
    }
    getCenter() { return this.center; }
    getZoom() { return this.zoom; }
    isStyleLoaded() { return true; }
    setStyle() { return this; }
    addSource(id: string) { h.quellen[id] = { type: "FeatureCollection", features: [] }; return this; }
    hasImage() { return false; }
    addImage() { return this; }
    addLayer() { return this; }
    getSource(id: string) {
      return {
        setData: (d: { type: string; features: unknown[] }) => { h.quellen[id] = d; },
      };
    }
    getLayer() { return {}; }
    setLayoutProperty() { return this; }
    easeTo() { return this; }
    jumpTo() { return this; }
    fitBounds() { return this; }
    remove() { return this; }
  }
  return {
    default: {
      Map: FakeMap, Marker: FakeMarker, Popup: FakePopup,
      LngLatBounds: FakeBounds, NavigationControl: class {},
    },
  };
});

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

const KOLLEGE = {
  id: "PIREP-KOLLEGE",
  ident: "SK4084",
  flight_number: "4084",
  dpt_airport_id: "ENGM",
  arr_airport_id: "ENEV",
  aircraft: { icao: "A319" },
  status_text: "Cruise",
  position: { lat: 60.2, lon: 11.1, gs: 420, heading: 20 },
};

/** Das Marker-Element des Kollegen — dasselbe, das der Pilot anklickt. */
function kollegenMarker(): HTMLElement | null {
  return (
    h.marker.map((m) => m.el).find((el) => el.className.includes("aa-ac-marker--va")) ?? null
  );
}

async function karteMitKollege(routeAntwort: unknown) {
  h.invokeAntworten = {
    va_live_flights: [KOLLEGE],
    fremde_flugroute: routeAntwort,
  };
  const { LiveMapView } = await import("./LiveMapView");
  await act(async () => {
    render(<LiveMapView />);
  });
  await waitFor(() => expect(kollegenMarker()).not.toBeNull(), { timeout: 3000 });
}

beforeEach(() => {
  h.marker = [];
  h.quellen = {};
  h.invokeRufe = [];
});
afterEach(() => cleanup());

describe("Klick auf einen Kollegen", () => {
  it("fragt die Route unter seiner Kennung ab", async () => {
    await karteMitKollege([[11.1, 60.2], [16.5, 68.5]]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    const ruf = h.invokeRufe.find((r) => r.cmd === "fremde_flugroute");
    expect(ruf, "der Klick hat die Route nie abgefragt").toBeTruthy();
    // Unter DIESER Kennung — nicht unter der Flugnummer oder leer.
    expect(ruf!.args?.pirepId).toBe("PIREP-KOLLEGE");
  });

  it("zeichnet die Route und nimmt sie beim zweiten Klick wieder weg", async () => {
    await karteMitKollege([[11.1, 60.2], [16.5, 68.5]]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await waitFor(() =>
      expect(h.quellen["fremde-routen"]?.features.length).toBe(1),
    );
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    await waitFor(() =>
      expect(h.quellen["fremde-routen"]?.features.length).toBe(0),
    );
  });

  it("sagt es sichtbar, wenn keine Route vorliegt", async () => {
    // Genau der Fall aus dem Betrieb: Der Server antwortet mit 200 und
    // einer leeren Liste. Bis 20.09.2026 lag der Hinweis dazu hinter der
    // Kartenschalter-Leiste — der Klick sah aus, als tue er nichts.
    await karteMitKollege([]);
    await act(async () => {
      kollegenMarker()!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });
    const hinweis = await screen.findByRole("status");
    expect(hinweis.textContent).toMatch(/keine Route/i);
    // Und er muss oben drauf liegen: Die Leiste traegt z-index 500.
    expect(Number(hinweis.style.zIndex)).toBeGreaterThan(500);
  });
});
