// Weltkugel-Umschalter und Fokus der Live-Karte (P-11.36).
import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const marker: Array<{ lngLat?: number[]; entfernt: boolean; el?: HTMLElement }> = [];
vi.mock("maplibre-gl", () => ({
  default: {
    Marker: class {
      d: { lngLat?: number[]; entfernt: boolean; el?: HTMLElement } = { entfernt: false };
      constructor(o: { element?: HTMLElement }) { this.d.el = o.element; marker.push(this.d); }
      setLngLat(l: number[]) { this.d.lngLat = l; return this; }
      addTo() { return this; }
      remove() { this.d.entfernt = true; }
    },
  },
}));
vi.mock("./bilderStore", () => ({
  useBilder: () => ({ liste: h.ohneListe ? null : { eintraege: h.eintraege } }),
  bilderStore: { laden: h.laden },
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: h.t, i18n: { language: "de" } }) }));
vi.mock("./useBildUrl", () => ({ vorschauAdresse: h.vorschau }));
const h = vi.hoisted(() => ({ vorschau: vi.fn(async (_e: unknown) => "blob:v" as string | null), eintraege: [] as unknown[], ohneListe: false, laden: vi.fn(async () => undefined), t: (k: string) => k }));
import { eintrag } from "./logik/testhilfen";
import { useBildKarte } from "./useBildKarte";

function karte() {
  const aufrufe: unknown[] = [];
  return { aufrufe, m: { setProjection: (p: unknown) => aufrufe.push(["projektion", p]), flyTo: (o: unknown) => aufrufe.push(["fly", o]), getZoom: () => 4, isStyleLoaded: () => true, once: vi.fn(), on: vi.fn(), off: vi.fn(), getContainer: () => document.body } };
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
  it("Klick auf eine Marke liefert das Bild und alle Bilder des Flugs in Zeitfolge, ohne die Karte zu erreichen", () => {
    h.eintraege = [
      eintrag({ id: "spaet", meta: { pirep_id: "P1", zeit_utc: "2026-11-07T12:00:00Z", breite: 10, laenge: 20 } }),
      eintrag({ id: "frueh", meta: { pirep_id: "P1", zeit_utc: "2026-11-07T10:00:00Z", breite: null, laenge: null } }),
    ];
    marker.length = 0;
    const k = karte();
    const { result } = renderHook(() => useBildKarte({ current: k.m as never }, null, "P1"));
    expect(result.current.gross).toBeNull();
    const el = marker[0].el!;
    expect(el.tagName).toBe("BUTTON");
    const beiKarte = vi.fn();
    const huelle = document.createElement("div");
    huelle.addEventListener("click", beiKarte);
    huelle.appendChild(el);
    act(() => el.click());
    expect(beiKarte).not.toHaveBeenCalled();
    expect(result.current.gross?.id).toBe("spaet");
    expect(result.current.gross?.bilder.map((b) => b.id)).toEqual(["frueh", "spaet"]);
    act(() => result.current.grossZu());
    expect(result.current.gross).toBeNull();
  });
  it("eine neue Liste mit denselben Marken baut sie nicht neu (Tablet-Takt), eine geaenderte schon", () => {
    const zeile = () => eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } });
    h.eintraege = [zeile()];
    marker.length = 0;
    const k = karte();
    const ref = { current: k.m as never };
    const { rerender } = renderHook(() => useBildKarte(ref, null, "P1"));
    expect(marker).toHaveLength(1);
    h.eintraege = [zeile()];
    rerender();
    expect(marker).toHaveLength(1);
    h.eintraege = [zeile(), eintrag({ id: "b", meta: { pirep_id: "P1", breite: 11, laenge: 21 } })];
    rerender();
    expect(marker).toHaveLength(3);
  });
  it("Ueberfahren einer Marke zeigt die Vorschau dieses Bildes; beim Abbau verschwindet sie", async () => {
    h.eintraege = [
      eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } }),
      eintrag({ id: "b", meta: { pirep_id: "P1", breite: 11, laenge: 21, rufzeichen: "GSG412", abflug: "EDDF", ziel: "LSZH", zeit_utc: "2026-11-08T14:21:00Z", hoehe_ft: 11200, phase: "descent", flugzeug_titel: "A320-251N" } }),
    ];
    marker.length = 0;
    h.vorschau.mockClear();
    const k = karte();
    const { unmount } = renderHook(() => useBildKarte({ current: k.m as never }, null, "P1"));
    const ev = new MouseEvent("pointerenter");
    Object.defineProperty(ev, "pointerType", { value: "mouse" });
    marker[1].el!.dispatchEvent(ev);
    await vi.waitFor(() => expect(h.vorschau).toHaveBeenCalledTimes(1));
    expect((h.vorschau.mock.calls[0][0] as { id: string }).id).toBe("b");
    expect([...document.querySelectorAll(".marken-vorschau__zeile")].map((z) => z.textContent)).toEqual([
      "GSG412 · EDDF–LSZH", `14:21Z · ${(11200).toLocaleString("de")} ft · ${h.t("bilder.detail.phase.descent")}`, "A320-251N",
    ]);
    unmount();
    expect(document.querySelector(".marken-vorschau")).toBeNull();
  });
  it("Schalter Bilder: steht anfangs an, aus nimmt Marken und offene Vorschau weg, an setzt sie wieder", async () => {
    h.eintraege = [eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } })];
    marker.length = 0;
    const ref = { current: karte().m as never };
    const { result } = renderHook(() => useBildKarte(ref, null, "P1"));
    expect(result.current.bilderAn).toBe(true);
    expect(marker).toHaveLength(1);
    const ev = new MouseEvent("pointerenter");
    Object.defineProperty(ev, "pointerType", { value: "mouse" });
    marker[0].el!.dispatchEvent(ev);
    await vi.waitFor(() => expect(document.querySelector(".marken-vorschau")).not.toBeNull());
    act(() => result.current.setBilderAn(false));
    expect(marker[0].entfernt).toBe(true);
    expect(document.querySelector(".marken-vorschau")).toBeNull();
    expect(marker).toHaveLength(1);
    act(() => result.current.setBilderAn(true));
    expect(marker).toHaveLength(2);
    expect(marker[1].entfernt).toBe(false);
  });
  it("ein neuer Fokus aus dem Bilder-Tab schaltet die Bilder wieder ein, derselbe nicht", () => {
    h.eintraege = [eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } })];
    const ref = { current: karte().m as never };
    let fokus = { lat: 10, lon: 20, id: "a", pirep: "P1", n: 1 };
    const { result, rerender } = renderHook(() => useBildKarte(ref, fokus, null));
    act(() => result.current.setBilderAn(false));
    rerender();
    expect(result.current.bilderAn).toBe(false);
    fokus = { ...fokus, n: 2 };
    rerender();
    expect(result.current.bilderAn).toBe(true);
  });
  it("laedt die Liste einmal, wenn ein Flug laeuft und noch keine da ist; ohne Flug nicht", () => {
    h.ohneListe = true;
    h.laden.mockClear();
    const k = karte();
    renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    expect(h.laden).not.toHaveBeenCalled();
    const { rerender } = renderHook(() => useBildKarte({ current: k.m as never }, null, "P1"));
    rerender();
    expect(h.laden).toHaveBeenCalledTimes(1);
    h.ohneListe = false;
  });
});
