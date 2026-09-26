/**
 * Notizblock (v1.8.1/v1.8.2): Handballen-Schutz ueber die ECHTE Ereigniskette
 * der Komponente, nicht nur ueber die Hilfsfunktion (Codex-Befund 6: das
 * Entfernen von `stiftErkannt()` oder des pointercancel-Zweigs waere sonst
 * nicht aufgefallen).
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { act, cleanup, render } from "@testing-library/react";
import { Notizblock } from "./Notizblock";
import { SPEICHER_SCHLUESSEL } from "./logik";

beforeAll(() => {
  // jsdom kennt weder PointerEvent noch ResizeObserver.
  class PE extends MouseEvent {
    pointerId: number;
    pointerType: string;
    pressure: number;
    width: number;
    height: number;
    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 1;
      this.pointerType = init.pointerType ?? "mouse";
      this.pressure = init.pressure ?? 0.5;
      this.width = init.width ?? 1;
      this.height = init.height ?? 1;
    }
  }
  (globalThis as unknown as { PointerEvent: typeof PE }).PointerEvent = PE;
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    disconnect() {}
  };
  HTMLCanvasElement.prototype.getContext = () => null;
});

beforeEach(() => localStorage.clear());
afterEach(cleanup);

function aufbau() {
  const r = render(<Notizblock />);
  const c = r.container.querySelector("canvas")!;
  c.getBoundingClientRect = () =>
    ({ left: 0, top: 0, width: 1000, height: 700, right: 1000, bottom: 700, x: 0, y: 0, toJSON() {} }) as DOMRect;
  const ev = (typ: string, id: number, art: string, x: number, y: number, groesse = 10) =>
    act(() => {
      c.dispatchEvent(
        new PointerEvent(typ, {
          bubbles: true,
          clientX: x,
          clientY: y,
          pointerId: id,
          pointerType: art,
          width: groesse,
          height: groesse,
          buttons: typ === "pointerup" || typ === "pointercancel" ? 0 : 1,
        }),
      );
    });
  const strich = (id: number, art: string, y: number, groesse = 10) => {
    ev("pointerdown", id, art, 100, y, groesse);
    ev("pointermove", id, art, 200, y, groesse);
    ev("pointerup", id, art, 300, y, groesse);
  };
  const anzahl = () => JSON.parse(localStorage.getItem(SPEICHER_SCHLUESSEL) ?? "[]").length;
  return { ev, strich, anzahl };
}

describe("Notizblock — Handballen-Schutz ueber die Ereigniskette", () => {
  it("der Stift zeichnet", () => {
    const { strich, anzahl } = aufbau();
    strich(1, "pen", 100);
    expect(anzahl()).toBe(1);
  });

  it("grosse Auflageflaeche zeichnet nie", () => {
    const { strich, anzahl } = aufbau();
    strich(1, "touch", 100, 60);
    expect(anzahl()).toBe(0);
  });

  it("Fingerstrich kurz vor dem ersten Stift verschwindet, danach zeichnet der Finger nicht", () => {
    const { strich, anzahl } = aufbau();
    strich(1, "touch", 100);
    expect(anzahl()).toBe(1);
    strich(2, "pen", 200);
    expect(anzahl()).toBe(1); // Handstrich weg, Stiftstrich da
    strich(3, "touch", 300);
    expect(anzahl()).toBe(1);
  });

  it("der Stift wird ueber ein Neuladen hinweg gemerkt", () => {
    const erste = aufbau();
    erste.strich(1, "pen", 100);
    cleanup();
    const zweite = aufbau();
    zweite.strich(2, "touch", 200);
    expect(zweite.anzahl()).toBe(1);
  });

  it("der Stift verdraengt einen laufenden Handstrich", () => {
    const { ev, strich, anzahl } = aufbau();
    // Vor dem ersten Stift darf der Finger — er beginnt einen Strich …
    ev("pointerdown", 1, "touch", 100, 100);
    ev("pointermove", 1, "touch", 150, 100);
    // … und der Stift setzt auf, waehrend die Hand noch liegt.
    strich(2, "pen", 200);
    ev("pointerup", 1, "touch", 200, 100);
    expect(anzahl()).toBe(1);
  });

  it("ein von iPadOS abgebrochener Fingerstrich wird verworfen", () => {
    const { ev, anzahl } = aufbau();
    ev("pointerdown", 1, "touch", 100, 100);
    ev("pointermove", 1, "touch", 150, 100);
    ev("pointercancel", 1, "touch", 150, 100);
    expect(anzahl()).toBe(0);
  });
});
