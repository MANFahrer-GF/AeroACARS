// Der Zoom der Bahn-Ansichten.
//
// Diese Prüfungen sind aus dem Ansehen entstanden, nicht aus dem Lesen:
// Beim Abnehmen reagierten die Zoomknöpfe scheinbar überhaupt nicht.
// Sie taten es doch — nur ging jeder Klick verloren, der vor dem nächsten
// Render kam, weil `stufe` aus dem Zustand des laufenden Renders rechnete
// statt aus dem vorigen. Ein Mensch, der zweimal zügig drückt, sieht genau
// einen Schritt. Ein Mausrad liefert mehrere Ereignisse pro Umdrehung und
// verliert entsprechend mehr.

import { describe, it, expect } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useBahnZoom } from "./useBahnZoom";

const GANZ_VON = -400;
const GANZ_BIS = 3250;

describe("useBahnZoom", () => {
  it("beginnt ohne Ausschnitt", () => {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    expect(result.current.gezoomt).toBe(false);
    expect(result.current.vonM).toBeUndefined();
  });

  it("addiert schnell aufeinanderfolgende Klicks", () => {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    // Drei Klicks OHNE Render dazwischen — genau das, was ein zügiges
    // Drücken auslöst.
    act(() => {
      result.current.stufe(1);
      result.current.stufe(1);
      result.current.stufe(1);
    });
    const breite = result.current.bisM! - result.current.vonM!;
    const erwartet = (GANZ_BIS - GANZ_VON) / 1.25 ** 3;
    expect(breite).toBeCloseTo(erwartet, 1);
  });

  it("zoomt auf die Mitte des Ausschnitts", () => {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    const mitteVorher = (GANZ_VON + GANZ_BIS) / 2;
    act(() => result.current.stufe(1));
    const mitte = (result.current.vonM! + result.current.bisM!) / 2;
    expect(mitte).toBeCloseTo(mitteVorher, 1);
  });

  it("kehrt beim Herauszoomen auf die ganze Bahn zurück", () => {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    act(() => result.current.stufe(1));
    expect(result.current.gezoomt).toBe(true);
    act(() => result.current.stufe(-1));
    // Nicht „fast ganz", sondern wirklich zurück: Ein Restausschnitt von
    // einem halben Meter liesse den Zurücksetzen-Knopf stehen und die
    // Ansicht anders rechnen als im Ruhezustand.
    expect(result.current.gezoomt).toBe(false);
    expect(result.current.vonM).toBeUndefined();
  });

  it("läuft nicht über die Bahn hinaus", () => {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    act(() => {
      for (let i = 0; i < 20; i++) result.current.stufe(-1);
    });
    expect(result.current.gezoomt).toBe(false);
  });

  it("zoomt nicht unter die Mindestbreite", () => {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    act(() => {
      for (let i = 0; i < 40; i++) result.current.stufe(1);
    });
    const breite = result.current.bisM! - result.current.vonM!;
    expect(breite).toBeGreaterThanOrEqual(50);
  });

  it("zurücksetzen räumt den Ausschnitt weg", () => {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    act(() => result.current.stufe(1));
    act(() => result.current.zuruecksetzen());
    expect(result.current.gezoomt).toBe(false);
  });

  /**
   * Ein echtes SVG, an dem der Anschluss hängt — kein nachgebautes
   * Ereignisobjekt.
   *
   * Der Unterschied ist genau der Punkt: Ein selbstgebautes Objekt mit
   * einer `preventDefault`-Attrappe hätte auch dann gemeldet „verhindert",
   * wenn der Zuhörer passiv angemeldet ist und der Browser den Aufruf
   * ignoriert. Am 23.08.2026 war das der Fall — der Zoom griff, und der
   * Browser zoomte die Seite gleich mit. `dispatchEvent` gibt `false`
   * zurück, wenn `preventDefault` **gewirkt** hat; das lässt sich nicht
   * vortäuschen.
   */
  function mitSvg() {
    const { result } = renderHook(() => useBahnZoom(GANZ_VON, GANZ_BIS));
    const svg = document.createElementNS(
      "http://www.w3.org/2000/svg",
      "svg",
    ) as SVGSVGElement;
    svg.getBoundingClientRect = () =>
      ({ left: 0, width: 600, top: 0, height: 300 }) as DOMRect;
    document.body.appendChild(svg);
    act(() => {
      result.current.radAnschluss(svg);
    });
    const rad = (anteil: number, mitStrg: boolean, runter = false) =>
      svg.dispatchEvent(
        new WheelEvent("wheel", {
          deltaY: runter ? 120 : -120,
          ctrlKey: mitStrg,
          bubbles: true,
          cancelable: true,
          clientX: anteil * 600,
        }),
      );
    return { result, svg, rad };
  }

  it("lässt die Seite scrollen, wenn Strg nicht gedrückt ist", () => {
    // Sonst verschluckt die Grafik jedes Rad-Ereignis über ihr, und die
    // Seite lässt sich nicht mehr scrollen, sobald der Zeiger darüber
    // steht. Genau das ist beim ersten Anlauf passiert.
    const { result, rad } = mitSvg();
    let nichtVerhindert = false;
    act(() => {
      nichtVerhindert = rad(0.5, false);
    });
    expect(result.current.gezoomt).toBe(false);
    expect(nichtVerhindert, "die Seite muss weiter scrollen").toBe(true);
  });

  it("hält den Browser zurück, wenn Strg gedrückt ist", () => {
    // Strg + Rad ist im Browser der Seitenzoom. Ohne ein wirksames
    // `preventDefault` zoomt die Grafik UND die ganze Seite.
    const { rad } = mitSvg();
    let nichtVerhindert = true;
    act(() => {
      nichtVerhindert = rad(0.5, true);
    });
    expect(
      nichtVerhindert,
      "der Browser zoomt die Seite mit — der Zuhörer ist passiv angemeldet",
    ).toBe(false);
  });

  it("zoomt mit Strg auf die Stelle unter dem Zeiger", () => {
    const { result, rad } = mitSvg();
    // Zeiger am linken Viertel: Was dort liegt, muss dort bleiben.
    const unterZeigerVorher = GANZ_VON + 0.25 * (GANZ_BIS - GANZ_VON);
    act(() => {
      rad(0.25, true);
    });
    const unterZeigerNachher =
      result.current.vonM! + 0.25 * (result.current.bisM! - result.current.vonM!);
    expect(unterZeigerNachher).toBeCloseTo(unterZeigerVorher, 1);
  });

  it("verliert keine schnellen Radschritte", () => {
    const { result, rad } = mitSvg();
    act(() => {
      rad(0.5, true);
      rad(0.5, true);
      rad(0.5, true);
    });
    const breite = result.current.bisM! - result.current.vonM!;
    expect(breite).toBeCloseTo((GANZ_BIS - GANZ_VON) / 1.25 ** 3, 1);
  });
});
