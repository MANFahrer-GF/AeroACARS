// Weltkugel-Umschalter und Fokus der Live-Karte (P-11.36).
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const marker: Array<{ lngLat?: number[]; entfernt: boolean }> = [];
vi.mock("maplibre-gl", () => ({
  default: {
    Marker: class {
      d: { lngLat?: number[]; entfernt: boolean } = { entfernt: false };
      constructor() { marker.push(this.d); }
      setLngLat(l: number[]) { this.d.lngLat = l; return this; }
      addTo() { return this; }
      remove() { this.d.entfernt = true; }
    },
  },
}));
vi.mock("./bilderStore", () => ({
  useBilder: () => ({ liste: { eintraege: h.eintraege } }),
}));
const h = vi.hoisted(() => ({ eintraege: [] as unknown[] }));
import { eintrag } from "./logik/testhilfen";
import { useBildKarte } from "./useBildKarte";

function karte() {
  const aufrufe: unknown[] = [];
  return { aufrufe, m: { setProjection: (p: unknown) => aufrufe.push(["projektion", p]), flyTo: (o: unknown) => aufrufe.push(["fly", o]), getZoom: () => 4, isStyleLoaded: () => true, once: vi.fn() } };
}

describe("useBildKarte", () => {
  it("Umschalter setzt die Projektion globe und wieder mercator", () => {
    const k = karte();
    const { result } = renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    expect(k.aufrufe.at(-1)).toEqual(["projektion", { type: "mercator" }]);
    act(() => result.current.setKugel(true));
    expect(k.aufrufe.at(-1)).toEqual(["projektion", { type: "globe" }]);
    act(() => result.current.setKugel(false));
    expect(k.aufrufe.at(-1)).toEqual(["projektion", { type: "mercator" }]);
  });
  it("Marken nur fuer Bilder desselben Flugs mit Position; Fokus fliegt zur Stelle", () => {
    h.eintraege = [
      eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } }),
      eintrag({ id: "b", meta: { pirep_id: "P2", breite: 11, laenge: 21 } }),
      eintrag({ id: "c", meta: { pirep_id: "P1", breite: null, laenge: null } }),
    ];
    marker.length = 0;
    const k = karte();
    const { result } = renderHook(() => useBildKarte({ current: k.m as never }, { lat: 10, lon: 20, id: "a", pirep: "P1", n: 1 }, null));
    expect(result.current.marken).toBe(1);
    expect(marker.map((x) => x.lngLat)).toEqual([[20, 10]]);
    expect(k.aufrufe.some((a) => (a as unknown[])[0] === "fly")).toBe(true);
  });
});
