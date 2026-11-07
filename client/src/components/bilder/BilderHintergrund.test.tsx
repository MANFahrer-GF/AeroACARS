// Die Uebernahme laeuft auch, wenn der Tab nicht gezeichnet ist: der Hintergrund-Baustein
// wacht auf Ereignisse und auf einen Takt (Webview gedrosselt, Funde im Eingang).
import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({ hoerer: new Map<string, Array<() => void>>(), eingang: vi.fn(async () => undefined) }));
vi.mock("../../lib/ipc", () => ({
  listen: async (n: string, cb: () => void) => { h.hoerer.set(n, [...(h.hoerer.get(n) ?? []), cb]); return () => h.hoerer.set(n, []); },
  invoke: async () => undefined,
}));
vi.mock("./bilderStore", () => ({ bilderStore: { eingang: h.eingang } }));
import { BilderHintergrund, EINGANG_TAKT_MS } from "./BilderHintergrund";

afterEach(() => { vi.useRealTimers(); h.eingang.mockClear(); h.hoerer.clear(); });

describe("BilderHintergrund", () => {
  it("arbeitet beim Start den Eingang ab, ohne dass ein Tab gezeichnet ist", () => {
    render(<BilderHintergrund />);
    expect(h.eingang).toHaveBeenCalledTimes(1);
  });
  it("Ereignisse sind nur Wecksignale", async () => {
    render(<BilderHintergrund />);
    await vi.waitFor(() => expect(h.hoerer.get("bild_quelle_neu")?.length).toBe(1));
    h.eingang.mockClear();
    h.hoerer.get("bild_quelle_neu")![0]();
    h.hoerer.get("bild_aufgenommen")![0]();
    expect(h.eingang).toHaveBeenCalledTimes(2);
  });
  it("ein Takt holt nach, was kein Ereignis gemeldet hat; nach dem Abbau ruht er", () => {
    vi.useFakeTimers();
    const { unmount } = render(<BilderHintergrund />);
    h.eingang.mockClear();
    vi.advanceTimersByTime(EINGANG_TAKT_MS * 2 + 10);
    expect(h.eingang).toHaveBeenCalledTimes(2);
    unmount();
    vi.advanceTimersByTime(EINGANG_TAKT_MS * 3);
    expect(h.eingang).toHaveBeenCalledTimes(2);
  });
});
