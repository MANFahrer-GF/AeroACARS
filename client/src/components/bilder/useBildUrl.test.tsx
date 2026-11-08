// Blob-Adressen der Bilder: beim Blaettern nie die alte Adresse, Speichergrenze in Bytes.
import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const h = vi.hoisted(() => ({ lesen: vi.fn(async (_p: string) => new Uint8Array(4)) }));
vi.mock("./logik/api", () => ({ bildAktionen: { lesen: h.lesen } }));
import { useBildUrl } from "./useBildUrl";

let n = 0;
beforeEach(() => {
  n = 0;
  URL.createObjectURL = vi.fn(() => `blob:${++n}`);
  URL.revokeObjectURL = vi.fn();
  h.lesen.mockReset();
  h.lesen.mockImplementation(async () => new Uint8Array(4));
});

describe("useBildUrl", () => {
  it("nach dem Pfadwechsel keine alte Adresse, sofort die neue, sobald sie da ist", async () => {
    const { result, rerender } = renderHook(({ p }) => useBildUrl(p), { initialProps: { p: "t1/a.jpg" } });
    await waitFor(() => expect(result.current).toBe("blob:1"));
    let fertig: (b: Uint8Array) => void = () => undefined;
    h.lesen.mockImplementation(() => new Promise((ok) => { fertig = ok; }));
    rerender({ p: "t1/b.jpg" });
    expect(result.current).toBeNull();
    fertig(new Uint8Array(4));
    await waitFor(() => expect(result.current).toBe("blob:2"));
    rerender({ p: "t1/a.jpg" });
    expect(result.current).toBe("blob:1");
  });
  it("gibt die aeltesten Adressen frei, wenn die Bytegrenze ueberschritten ist, und behaelt das neueste", async () => {
    h.lesen.mockImplementation(async () => new Uint8Array(120 * 1024 * 1024));
    URL.createObjectURL = vi.fn(() => `blob:gross-${++n}`); // der Zwischenspeicher lebt ueber die Tests hinweg
    const a = renderHook(() => useBildUrl("t2/gross-a.jpg"));
    await waitFor(() => expect(a.result.current).not.toBeNull());
    const urlA = a.result.current;
    const b = renderHook(() => useBildUrl("t2/gross-b.jpg"));
    await waitFor(() => expect(b.result.current).not.toBeNull());
    expect(URL.revokeObjectURL).toHaveBeenCalledWith(urlA);
    expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(b.result.current);
  });
});
