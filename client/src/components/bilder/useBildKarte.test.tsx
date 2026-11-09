// Weltkugel-Umschalter und Fokus der Live-Karte (P-11.36).
import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

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
  const handler: Record<string, Array<() => void>> = {};
  return { aufrufe, handler, m: { setProjection: (p: unknown) => aufrufe.push(["projektion", p]), flyTo: (o: unknown) => aufrufe.push(["fly", o]), getZoom: () => 4, isStyleLoaded: () => true, once: vi.fn(), on: (ev: string, cb: () => void) => { (handler[ev] ??= []).push(cb); }, off: (ev: string, cb: () => void) => { handler[ev] = (handler[ev] ?? []).filter((x) => x !== cb); }, getContainer: () => document.body, fitBounds: (b: unknown, o: unknown) => aufrufe.push(["einpassen", b, o]) } };
}
beforeEach(() => { try { localStorage.removeItem("aaLivemapKugel"); } catch { /* egal */ } });

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
  it("ohne Flug wird einmal auf alle Punkte eingepasst, mit Flug, Fokus oder verdeckter Karte nicht", () => {
    h.eintraege = [
      eintrag({ id: "tokio", meta: { pirep_id: "P1", breite: 35, laenge: 139 } }),
      eintrag({ id: "osaka", meta: { pirep_id: "P1", breite: 34, laenge: 135 } }),
    ];
    const einpassen = (k: ReturnType<typeof karte>) => k.aufrufe.filter((a) => (a as unknown[])[0] === "einpassen");
    const ohne = karte();
    const { rerender } = renderHook(() => useBildKarte({ current: ohne.m as never }, null, null));
    rerender();
    expect(einpassen(ohne)).toHaveLength(1);
    expect((einpassen(ohne)[0] as unknown[])[1]).toEqual([[135, 34], [139, 35]]);
    const mitFlug = karte();
    renderHook(() => useBildKarte({ current: mitFlug.m as never }, null, "P1"));
    expect(einpassen(mitFlug)).toHaveLength(0);
    const mitFokus = karte();
    renderHook(() => useBildKarte({ current: mitFokus.m as never }, { lat: 35, lon: 139, id: "tokio", pirep: "P1", n: 1 }, null));
    expect(einpassen(mitFokus)).toHaveLength(0);
    const verdeckt = karte();
    const v = renderHook(({ s }) => useBildKarte({ current: verdeckt.m as never }, null, null, s), { initialProps: { s: false } });
    expect(einpassen(verdeckt)).toHaveLength(0);
    v.rerender({ s: true });
    expect(einpassen(verdeckt)).toHaveLength(1);
  });
  it("Marken bei aktivem Flug nur fuer dessen Bilder mit Position; Fokus fliegt zur Stelle", () => {
    h.eintraege = [
      eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } }),
      eintrag({ id: "b", meta: { pirep_id: "P2", breite: 11, laenge: 21 } }),
      eintrag({ id: "c", meta: { pirep_id: "P1", breite: null, laenge: null } }),
    ];
    marker.length = 0;
    const k = karte();
    const { result } = renderHook(() => useBildKarte({ current: k.m as never }, { lat: 10, lon: 20, id: "a", pirep: "P1", n: 1 }, "P1"));
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
  it("laedt die Liste einmal, sobald sie fehlt, auch ohne Flug", () => {
    h.ohneListe = true;
    h.laden.mockClear();
    const k = karte();
    renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    expect(h.laden).toHaveBeenCalledTimes(1);
    h.ohneListe = false;
  });
  it("passt ueber die Datumsgrenze auf den kleinen Bogen ein, nicht ueber die ganze Welt", () => {
    h.eintraege = [
      eintrag({ id: "ost", meta: { pirep_id: "P1", breite: 10, laenge: 170 } }),
      eintrag({ id: "west", meta: { pirep_id: "P1", breite: 20, laenge: -170 } }),
    ];
    const k = karte();
    renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    const box = (k.aufrufe.find((a) => (a as unknown[])[0] === "einpassen") as unknown[])[1];
    expect(box).toEqual([[170, 10], [190, 20]]);
  });
  it("springt nach der Abgabe nicht auf alle Punkte", () => {
    h.eintraege = [
      eintrag({ id: "tokio", meta: { pirep_id: "P1", breite: 35, laenge: 139 } }),
      eintrag({ id: "osaka", meta: { pirep_id: "P1", breite: 34, laenge: 135 } }),
    ];
    const k = karte();
    const { rerender } = renderHook(({ flug }) => useBildKarte({ current: k.m as never }, null, flug), { initialProps: { flug: "P1" as string | null } });
    rerender({ flug: null });
    expect(k.aufrufe.filter((a) => (a as unknown[])[0] === "einpassen")).toHaveLength(0);
  });
  it("scheitert der Abruf beim Start, holt das Oeffnen der Karte ihn nach", () => {
    h.ohneListe = true;
    h.laden.mockClear();
    const k = karte();
    const { rerender } = renderHook(({ sichtbar }) => useBildKarte({ current: k.m as never }, null, null, sichtbar), { initialProps: { sichtbar: false } });
    expect(h.laden).toHaveBeenCalledTimes(1);
    rerender({ sichtbar: true });
    expect(h.laden).toHaveBeenCalledTimes(2);
    h.ohneListe = false;
  });
  it("ohne aktiven Flug stehen die Bilder aller Fluege mit Position auf der Karte", () => {
    h.eintraege = [
      eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } }),
      eintrag({ id: "b", meta: { pirep_id: "P2", breite: 11, laenge: 21 } }),
      eintrag({ id: "c", meta: { pirep_id: "P2", breite: null, laenge: null } }),
    ];
    marker.length = 0;
    const k = karte();
    const { result } = renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    expect(result.current.marken).toBe(2);
    expect(marker.map((x) => x.lngLat)).toEqual([[20, 10], [21, 11]]);
  });
  it("mit aktivem Flug kommen die Bilder aus dem Fokus-Flug dazu; der Fokus verdraengt den aktiven Flug nicht", () => {
    h.eintraege = [
      eintrag({ id: "a", meta: { pirep_id: "P1", breite: 10, laenge: 20 } }),
      eintrag({ id: "b", meta: { pirep_id: "P2", breite: 11, laenge: 21 } }),
      eintrag({ id: "c", meta: { pirep_id: "P3", breite: 12, laenge: 22 } }),
    ];
    marker.length = 0;
    const k = karte();
    const { result } = renderHook(() => useBildKarte({ current: k.m as never }, { lat: 11, lon: 21, id: "b", pirep: "P2", n: 1 }, "P1"));
    expect(result.current.marken).toBe(2);
    expect(marker.map((x) => x.lngLat)).toEqual([[20, 10], [21, 11]]);
  });
  it("Klick auf eine Marke oeffnet die Grossansicht nur mit den Bildern desselben Flugs", () => {
    h.eintraege = [
      eintrag({ id: "a1", meta: { pirep_id: "P1", zeit_utc: "2026-11-07T10:00:00Z", breite: 10, laenge: 20 } }),
      eintrag({ id: "a2", meta: { pirep_id: "P1", zeit_utc: "2026-11-07T11:00:00Z", breite: 10, laenge: 21 } }),
      eintrag({ id: "b1", meta: { pirep_id: "P2", zeit_utc: "2026-11-07T12:00:00Z", breite: 11, laenge: 22 } }),
    ];
    marker.length = 0;
    const k = karte();
    const { result } = renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    act(() => marker[2].el!.click());
    expect(result.current.gross?.bilder.map((b) => b.id)).toEqual(["b1"]);
    act(() => marker[0].el!.click());
    expect(result.current.gross?.bilder.map((b) => b.id)).toEqual(["a1", "a2"]);
  });
  it("Weltkugel: die Projektion wird beim style.load gesetzt, der Listener wird abgebaut", () => {
    const k = karte();
    const { result, unmount } = renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    act(() => result.current.setKugel(true));
    k.aufrufe.length = 0;
    expect(k.handler["style.load"]).toHaveLength(1);
    k.handler["style.load"].forEach((cb) => cb());
    expect(k.aufrufe.at(-1)).toEqual(["projektion", { type: "globe" }]);
    unmount();
    expect(k.handler["style.load"]).toHaveLength(0);
  });
  it("Weltkugel: die Wahl bleibt in localStorage und gilt nach einem Neustart", () => {
    const k = karte();
    const { result, unmount } = renderHook(() => useBildKarte({ current: k.m as never }, null, null));
    expect(result.current.kugel).toBe(false);
    act(() => result.current.setKugel(true));
    expect(localStorage.getItem("aaLivemapKugel")).toBe("1");
    unmount();
    const k2 = karte();
    const { result: neu } = renderHook(() => useBildKarte({ current: k2.m as never }, null, null));
    expect(neu.current.kugel).toBe(true);
    expect(k2.aufrufe.at(-1)).toEqual(["projektion", { type: "globe" }]);
  });
});
