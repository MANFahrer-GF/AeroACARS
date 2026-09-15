import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook } from "@testing-library/react";

const invoke = vi.fn((_cmd: string, _args?: unknown) => Promise.resolve(true));
vi.mock("./ipc", () => ({ invoke: (cmd: string, args?: unknown) => invoke(cmd, args), isTauri: true }));

import { BEOBACHTUNG_MS, MESSTAKT_MS, useFensterAnInhalt } from "./useFensterAnInhalt";

function seiteMit(scrollHeight: number): HTMLElement {
  document.body.innerHTML = '<main class="app"></main>';
  const seite = document.querySelector<HTMLElement>("main.app")!;
  Object.defineProperty(seite, "clientHeight", { configurable: true, get: () => window.innerHeight });
  let hoehe = scrollHeight;
  Object.defineProperty(seite, "scrollHeight", { configurable: true, get: () => hoehe });
  (seite as unknown as { setzen: (h: number) => void }).setzen = (h) => (hoehe = h);
  return seite;
}

describe("useFensterAnInhalt in der Desktop-App", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    invoke.mockClear();
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 770 });
  });
  afterEach(() => vi.useRealTimers());

  it("fragt beim Betreten die benötigte Höhe an", () => {
    seiteMit(1030);
    renderHook(() => useFensterAnInhalt(true, null));
    expect(invoke).toHaveBeenCalledWith("fenster_an_inhalt_anpassen", { benoetigtHoehe: 1030 });
  });

  it("bemerkt später nachgeladenen Inhalt ohne Größenänderung der Seite", () => {
    const seite = seiteMit(770);
    renderHook(() => useFensterAnInhalt(true, null));
    expect(invoke).not.toHaveBeenCalled();
    (seite as unknown as { setzen: (h: number) => void }).setzen(1200);
    vi.advanceTimersByTime(MESSTAKT_MS);
    expect(invoke).toHaveBeenCalledWith("fenster_an_inhalt_anpassen", { benoetigtHoehe: 1200 });
  });

  it("fragt dieselbe oder eine kleinere Höhe nicht erneut an", () => {
    const seite = seiteMit(1030);
    renderHook(() => useFensterAnInhalt(true, null));
    vi.advanceTimersByTime(MESSTAKT_MS * 3);
    (seite as unknown as { setzen: (h: number) => void }).setzen(900);
    vi.advanceTimersByTime(MESSTAKT_MS * 3);
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("hört nach der Beobachtungszeit auf — späteres Verkleinern wird nicht überstimmt", () => {
    const seite = seiteMit(770);
    renderHook(() => useFensterAnInhalt(true, null));
    vi.advanceTimersByTime(BEOBACHTUNG_MS + MESSTAKT_MS);
    (seite as unknown as { setzen: (h: number) => void }).setzen(1500);
    vi.advanceTimersByTime(MESSTAKT_MS * 4);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("misst neu, wenn sich der Auslöser ändert (Flug beginnt)", () => {
    const seite = seiteMit(770);
    const { rerender } = renderHook(({ id }) => useFensterAnInhalt(true, id), {
      initialProps: { id: null as string | null },
    });
    vi.advanceTimersByTime(BEOBACHTUNG_MS + MESSTAKT_MS);
    (seite as unknown as { setzen: (h: number) => void }).setzen(1300);
    rerender({ id: "pirep-1" });
    expect(invoke).toHaveBeenCalledWith("fenster_an_inhalt_anpassen", { benoetigtHoehe: 1300 });
  });

  it("tut nichts auf anderen Seiten", () => {
    seiteMit(2000);
    renderHook(() => useFensterAnInhalt(false, null));
    vi.advanceTimersByTime(MESSTAKT_MS * 4);
    expect(invoke).not.toHaveBeenCalled();
  });
});
