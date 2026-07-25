// v1.3.0 (#Hoppie-PDC-CPDLC) — what raises an alert.
//
// EVERY inbound message chimes, a logon accept included. Deciding for
// the pilot that some traffic is "unimportant enough" to stay silent is
// not this code's call — if it arrived, they get told. The only thing
// suppressed is the backlog on the very first poll after connecting,
// which would otherwise fire a burst of alerts for stale messages.

import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { renderHook } from "@testing-library/react";

const invokeMock = vi.fn();
vi.mock("../lib/ipc", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const playMock = vi.fn(() => Promise.resolve());
vi.stubGlobal(
  "Audio",
  class {
    currentTime = 0;
    play = playMock;
  },
);

import { useHoppieAttention } from "./useHoppieAttention";

const STATUS = { connected: true, pending_uplink_count: 0 };

/** Drives the hook's two polls with a scripted thread. */
function backend(threads: Array<Array<{ direction: string; element_id: string | null }>>) {
  let call = 0;
  return (cmd: string) => {
    if (cmd === "hoppie_get_settings")
      return Promise.resolve({ enabled: true, notify_sound: true });
    if (cmd === "hoppie_status") return Promise.resolve(STATUS);
    if (cmd === "hoppie_get_thread") {
      const t = threads[Math.min(call, threads.length - 1)];
      call += 1;
      return Promise.resolve(t);
    }
    return Promise.resolve(undefined);
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  invokeMock.mockReset();
  playMock.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

const uplink = (element_id: string | null) => ({ direction: "received", element_id });

describe("useHoppieAttention alerts", () => {
  it("chimes for a logon accept too", async () => {
    invokeMock.mockImplementation(backend([[], [uplink("UM_LOGON_ACCEPTED")]]));
    renderHook(() => useHoppieAttention(true));

    // An extra cycle: the settings load has to resolve before the poll
    // effect even starts, which costs this first-in-file test one tick.
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5000);
    await vi.advanceTimersByTimeAsync(5000);

    expect(playMock).toHaveBeenCalledTimes(1);
  });

  it("still alerts for a real instruction", async () => {
    invokeMock.mockImplementation(backend([[], [uplink("UM20")]]));
    renderHook(() => useHoppieAttention(true));

    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5000);

    expect(playMock).toHaveBeenCalledTimes(1);
  });

  it("chimes once when several messages land together", async () => {
    invokeMock.mockImplementation(
      backend([[], [uplink("UM20"), uplink("UM74"), uplink("UM19")]]),
    );
    renderHook(() => useHoppieAttention(true));

    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5000);

    expect(playMock).toHaveBeenCalledTimes(1);
  });

  it("does not alert for the backlog already waiting at startup", async () => {
    // First poll only establishes the baseline.
    invokeMock.mockImplementation(backend([[uplink("UM20"), uplink("UM74")]]));
    renderHook(() => useHoppieAttention(true));

    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5000);

    expect(playMock).not.toHaveBeenCalled();
  });
});
