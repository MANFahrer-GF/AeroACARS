// v1.2.3 (#Hoppie-PDC-CPDLC) — opt-in means invisible, not disabled.
//
// The hook reports `enabled` so App.tsx can leave the tab out entirely
// while the feature is off. A dead tab that only points back at settings
// is not opt-in, it is clutter for every pilot who doesn't use datalink.

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

vi.stubGlobal(
  "Audio",
  class {
    currentTime = 0;
    play = () => Promise.resolve();
  },
);

import { useHoppieAttention } from "./useHoppieAttention";

beforeEach(() => {
  vi.useFakeTimers();
  invokeMock.mockReset();
});

afterEach(() => {
  vi.useRealTimers();
});

function settings(enabled: boolean) {
  return (cmd: string) => {
    if (cmd === "hoppie_get_settings")
      return Promise.resolve({ enabled, notify_sound: false });
    if (cmd === "hoppie_status")
      return Promise.resolve({ connected: false, pending_uplink_count: 0 });
    if (cmd === "hoppie_get_thread") return Promise.resolve([]);
    return Promise.resolve(undefined);
  };
}

describe("useHoppieAttention enabled flag", () => {
  it("starts disabled, before settings have loaded", () => {
    invokeMock.mockImplementation(settings(true));
    const { result } = renderHook(() => useHoppieAttention(true));
    expect(
      result.current.enabled,
      "must not flash the tab in before we know the setting",
    ).toBe(false);
  });

  it("reports enabled once the pilot has switched it on", async () => {
    invokeMock.mockImplementation(settings(true));
    const { result } = renderHook(() => useHoppieAttention(true));
    // The settings load resolves outside React's own batching, so the
    // re-render has to be flushed explicitly.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.enabled).toBe(true);
  });

  it("stays disabled when the feature is off", async () => {
    invokeMock.mockImplementation(settings(false));
    const { result } = renderHook(() => useHoppieAttention(true));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(result.current.enabled).toBe(false);
    // And nothing may be polled for a feature the pilot never turned on.
    expect(invokeMock.mock.calls.some((c) => c[0] === "hoppie_status")).toBe(false);
  });

  it("does not touch the network at all while logged out of phpVMS", async () => {
    invokeMock.mockImplementation(settings(true));
    renderHook(() => useHoppieAttention(false));
    await vi.advanceTimersByTimeAsync(5000);
    expect(invokeMock).not.toHaveBeenCalled();
  });
});
