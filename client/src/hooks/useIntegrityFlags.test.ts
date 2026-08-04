// v0.19.x FIX: the async IIFE assigned `unlisten` AFTER `await listen(...)`.
// On a fast unmount (guaranteed every mount in React StrictMode dev), the
// effect's cleanup ran BEFORE that await resolved — `unlisten` was still
// undefined when cleanup checked it, and by the time the promise resolved
// and set it, cleanup had already run and would never run again. The Tauri
// event listener leaked permanently on every mount.

import { describe, it, expect, vi } from "vitest";
import { renderHook } from "@testing-library/react";

let resolveListenFn: ((v: () => void) => void) | undefined;

vi.mock("../lib/ipc", () => ({
  listen: vi.fn(
    () =>
      new Promise<() => void>((resolve) => {
        resolveListenFn = resolve;
      }),
  ),
}));

describe("useIntegrityFlags", () => {
  it("unregisters the Tauri listener even when unmounted before listen() resolves", async () => {
    const { useIntegrityFlags } = await import("./useIntegrityFlags");
    const unlistenMock = vi.fn();

    const { unmount } = renderHook(() => useIntegrityFlags());
    // Unmount SYNCHRONOUSLY, before the mocked listen() promise resolves —
    // reproduces the exact race (React StrictMode's mount/unmount/remount
    // does this on every single mount in dev).
    unmount();

    // Now let listen() resolve, simulating it landing after the unmount.
    expect(resolveListenFn).toBeDefined();
    resolveListenFn!(unlistenMock);
    // Flush the microtask queue so the hook's post-await code runs.
    await Promise.resolve();
    await Promise.resolve();

    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });

  it("still unregisters normally when unmount happens after listen() resolves", async () => {
    const { useIntegrityFlags } = await import("./useIntegrityFlags");
    const unlistenMock = vi.fn();

    const { unmount } = renderHook(() => useIntegrityFlags());
    expect(resolveListenFn).toBeDefined();
    resolveListenFn!(unlistenMock);
    await Promise.resolve();
    await Promise.resolve();

    expect(unlistenMock).not.toHaveBeenCalled();
    unmount();
    expect(unlistenMock).toHaveBeenCalledTimes(1);
  });
});
