// Die Uebernahme laeuft auch, wenn der Tab nicht gezeichnet ist: der Hintergrund-Baustein
// wacht auf Ereignisse und auf einen Takt (Webview gedrosselt, Funde im Eingang).
import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({ hoerer: new Map<string, Array<(e: unknown) => void>>(), eingang: vi.fn(async () => undefined), gescheitert: vi.fn() }));
vi.mock("../../lib/ipc", () => ({
  isTauri: true,
  listen: async (n: string, cb: (e: unknown) => void) => { h.hoerer.set(n, [...(h.hoerer.get(n) ?? []), cb]); return () => h.hoerer.set(n, []); },
  invoke: async () => undefined,
}));
vi.mock("./bilderStore", () => ({ bilderStore: { eingang: h.eingang, aufnahmeGescheitert: h.gescheitert } }));
import { BilderHintergrund, EINGANG_TAKT_MS } from "./BilderHintergrund";

afterEach(() => { vi.useRealTimers(); h.eingang.mockClear(); h.gescheitert.mockClear(); h.hoerer.clear(); });

describe("BilderHintergrund", () => {
  it("arbeitet beim Start den Eingang ab, ohne dass ein Tab gezeichnet ist", () => {
    render(<BilderHintergrund />);
    expect(h.eingang).toHaveBeenCalledTimes(1);
  });
  it("Ereignisse sind nur Wecksignale", async () => {
    render(<BilderHintergrund />);
    await vi.waitFor(() => expect(h.hoerer.get("bild_quelle_neu")?.length).toBe(1));
    h.eingang.mockClear(); h.gescheitert.mockClear();
    h.hoerer.get("bild_quelle_neu")![0]({ payload: {} });
    h.hoerer.get("bild_aufgenommen")![0]({ payload: { ok: true } });
    expect(h.eingang).toHaveBeenCalledTimes(2);
    expect(h.gescheitert).not.toHaveBeenCalled();
  });
  it("eine gescheiterte Aufnahme per Taste oder Yoke wird gemeldet", async () => {
    render(<BilderHintergrund />);
    await vi.waitFor(() => expect(h.hoerer.get("bild_aufgenommen")?.length).toBe(1));
    h.hoerer.get("bild_aufgenommen")![0]({ payload: { ok: false, code: "verdeckt", meldung: "liegt darueber" } });
    expect(h.gescheitert).toHaveBeenCalledWith("verdeckt", "liegt darueber");
  });
  it("ein Takt holt nach, was kein Ereignis gemeldet hat; nach dem Abbau ruht er", () => {
    vi.useFakeTimers();
    const { unmount } = render(<BilderHintergrund />);
    h.eingang.mockClear(); h.gescheitert.mockClear();
    vi.advanceTimersByTime(EINGANG_TAKT_MS * 2 + 10);
    expect(h.eingang).toHaveBeenCalledTimes(2);
    unmount();
    vi.advanceTimersByTime(EINGANG_TAKT_MS * 3);
    expect(h.eingang).toHaveBeenCalledTimes(2);
  });
});
