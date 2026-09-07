// v1.3.0 (#Hoppie-PDC-CPDLC) — what raises an alert.
//
// EVERY inbound message chimes, a logon accept included. Deciding for
// the pilot that some traffic is "unimportant enough" to stay silent is
// not this code's call — if it arrived, they get told. The only thing
// suppressed is the backlog on the very first poll after connecting,
// which would otherwise fire a burst of alerts for stale messages.

import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { act, renderHook } from "@testing-library/react";

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
function backend(
  threads: Array<Array<{ direction: string; element_id: string | null; kind: string }>>,
) {
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

const uplink = (element_id: string | null, kind: "telex" | "cpdlc" = "cpdlc") => ({
  direction: "received",
  element_id,
  kind,
});

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

// v1.7.20 (#pdc-cpdlc-mode-routing): field report 06.09.2026 — clicking
// the attention banner for a CPDLC message always opened the PDC
// sub-tab, because nothing distinguished which kind of traffic had
// actually arrived. `unseenCpdlcCount` is what a caller uses to decide.
describe("useHoppieAttention — PDC vs CPDLC unseen counts", () => {
  // How many 5000ms poll cycles a test needs before `result.current`
  // reflects the second `hoppie_get_thread` response is not fixed at 2 —
  // exactly like "chimes for a logon accept too" above needing an extra
  // cycle for being first-in-file, WHICHEVER test runs first after a
  // fresh `vi.useFakeTimers()` in this block needs one more than the
  // ones after it (observed empirically: fixing it for test N shifts the
  // same one-tick shortfall to test N+1). A single large jump sidesteps
  // guessing the exact count — `advanceTimersByTimeAsync` fires every
  // interval tick that falls inside the window in one pass.
  async function advanceThroughPolls() {
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(30000);
  }

  it("keeps a PDC telex out of the CPDLC-specific count", async () => {
    invokeMock.mockImplementation(backend([[], [uplink(null, "telex")]]));
    const { result } = renderHook(() => useHoppieAttention(true));

    await advanceThroughPolls();

    expect(result.current.unseenCount).toBe(1);
    expect(result.current.unseenCpdlcCount).toBe(0);
  });

  it("counts a CPDLC uplink in both the total and the CPDLC-specific count", async () => {
    invokeMock.mockImplementation(backend([[], [uplink("UM20", "cpdlc")]]));
    const { result } = renderHook(() => useHoppieAttention(true));

    await advanceThroughPolls();

    expect(result.current.unseenCount).toBe(1);
    expect(result.current.unseenCpdlcCount).toBe(1);
  });

  it("markSeen clears both counts together", async () => {
    invokeMock.mockImplementation(backend([[], [uplink("UM20", "cpdlc")]]));
    const { result } = renderHook(() => useHoppieAttention(true));

    await advanceThroughPolls();
    expect(result.current.unseenCpdlcCount).toBe(1);

    act(() => result.current.markSeen());
    expect(result.current.unseenCount).toBe(0);
    expect(result.current.unseenCpdlcCount).toBe(0);
  });
});

// QS round 06.09.2026: `poll()` is fire-and-forget every 5s — a slow
// `hoppie_get_thread` round trip can still be in flight when a LATER
// poll already dispatched (or even resolved). Without the generation
// guard, that stale response would land after the fact and recompute a
// delta against a `receivedSeen` baseline that has since moved on,
// double-counting or re-chiming for a message already accounted for.
describe("useHoppieAttention — overlapping polls", () => {
  it("discards a hoppie_get_thread response from a superseded poll", async () => {
    // Phase 1: let a couple of ordinary, fast polls run so `enabled` has
    // flipped and a baseline (zero messages) is established normally —
    // same big-jump approach as the block above, avoiding this file's
    // own documented first-poll timing quirk.
    invokeMock.mockImplementation(backend([[], []]));
    const { result } = renderHook(() => useHoppieAttention(true));
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(30000);
    expect(result.current.unseenCpdlcCount).toBe(0);

    // Phase 2: switch to a mock that hands back hand-controlled,
    // unresolved promises for every further `hoppie_get_thread` call,
    // capturing each one's resolver in dispatch order.
    const pending: Array<(entries: ReturnType<typeof uplink>[]) => void> = [];
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "hoppie_get_settings")
        return Promise.resolve({ enabled: true, notify_sound: true });
      if (cmd === "hoppie_status") return Promise.resolve({ connected: true, pending_uplink_count: 0 });
      if (cmd === "hoppie_get_thread") {
        return new Promise((resolve) => pending.push(resolve));
      }
      return Promise.resolve(undefined);
    });

    // Phase 3: two more poll cycles fire (the interval from phase 1 keeps
    // running), each dispatching a `hoppie_get_thread` call that now
    // hangs — neither resolves yet, so state is unchanged so far.
    await vi.advanceTimersByTimeAsync(10000);
    expect(pending.length).toBe(2);

    // Phase 4: resolve the NEWER dispatch first, with one fresh CPDLC
    // message relative to the phase-1 baseline. Wrapped in act() — unlike
    // a timer-driven resolution, this state update is triggered directly
    // from test code, outside any React-tracked event.
    await act(async () => {
      pending[1]([uplink("UM20", "cpdlc")]);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.unseenCpdlcCount).toBe(1);

    // Now the OLDER dispatch finally resolves — empty, as if it had
    // raced back with the phase-1 baseline content. It must be discarded
    // outright, not reset the count and not double-count.
    await act(async () => {
      pending[0]([]);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.unseenCpdlcCount).toBe(1);
    expect(result.current.unseenCount).toBe(1);
  });

  // QS round 3 (07.09.2026): the generation guard above only protects
  // against being superseded by a NEWER poll — nothing bumped it when the
  // feature was simply switched off mid-flight. A response still in
  // flight at that moment would land AFTER the reset and silently
  // re-apply a stale delta on top of the freshly-zeroed counts.
  it("does not let a response in flight when the feature is disabled resurrect the counts", async () => {
    let settingsEnabled = true;
    let threadCalls = 0;
    const pending: Array<(entries: ReturnType<typeof uplink>[]) => void> = [];
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "hoppie_get_settings")
        return Promise.resolve({ enabled: settingsEnabled, notify_sound: true });
      if (cmd === "hoppie_status") return Promise.resolve({ connected: true, pending_uplink_count: 0 });
      if (cmd === "hoppie_get_thread") {
        threadCalls += 1;
        // First call establishes the baseline synchronously; every
        // later one hangs until manually resolved.
        if (threadCalls === 1) return Promise.resolve([]);
        return new Promise((resolve) => pending.push(resolve));
      }
      return Promise.resolve(undefined);
    });

    const { result } = renderHook(() => useHoppieAttention(true));
    await vi.advanceTimersByTimeAsync(0);
    await vi.advanceTimersByTimeAsync(5000); // baseline poll dispatched
    await vi.advanceTimersByTimeAsync(5000); // a second poll dispatched — now pending
    expect(pending.length).toBeGreaterThan(0);
    const stale = pending[pending.length - 1];

    // The pilot switches CPDLC off while that poll is still in flight.
    settingsEnabled = false;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(result.current.enabled).toBe(false);
    expect(result.current.unseenCount).toBe(0);

    // The stale response finally arrives, carrying a "new" message.
    await act(async () => {
      stale([uplink("UM20", "cpdlc")]);
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(
      result.current.unseenCount,
      "a response from before the feature was disabled must not resurrect the badge",
    ).toBe(0);
    expect(result.current.unseenCpdlcCount).toBe(0);
  });
});
