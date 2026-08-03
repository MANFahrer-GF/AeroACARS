// Field feedback (2026-08-03): "Logbuch abfragen härten — da kam grad keine
// Verbindung zu phpvms" — the list fetch had zero retry/backoff. Guards the
// retry helper's actual decision logic: retry transient-looking failures,
// give up on the rest, and eventually surface the last error if every
// attempt fails.
import { describe, it, expect, vi } from "vitest";
import { invokeWithRetry, RETRY_DELAYS_MS } from "./LogbookView";

describe("invokeWithRetry", () => {
  it("returns the result immediately on first success, no retry", async () => {
    const fn = vi.fn().mockResolvedValue("ok");
    await expect(invokeWithRetry(fn)).resolves.toBe("ok");
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("retries a network-looking error and succeeds once the connection recovers", async () => {
    vi.useFakeTimers();
    const fn = vi
      .fn()
      .mockRejectedValueOnce(new Error("network error: fetch failed"))
      .mockResolvedValueOnce("ok");
    const p = invokeWithRetry(fn);
    await vi.advanceTimersByTimeAsync(RETRY_DELAYS_MS[0]);
    await expect(p).resolves.toBe("ok");
    expect(fn).toHaveBeenCalledTimes(2);
    vi.useRealTimers();
  });

  it("does NOT retry a non-transient error (e.g. auth/404) — fails immediately", async () => {
    const authErr = new Error("401 unauthorized");
    const fn = vi.fn().mockRejectedValue(authErr);
    await expect(invokeWithRetry(fn)).rejects.toBe(authErr);
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it("gives up after exhausting all retries and surfaces the last error", async () => {
    vi.useFakeTimers();
    const netErr = new Error("connect timeout");
    const fn = vi.fn().mockRejectedValue(netErr);
    const p = invokeWithRetry(fn);
    // Swallow the eventual rejection so it doesn't count as an unhandled
    // rejection while the fake timers below are still advancing.
    p.catch(() => {});
    for (const delay of RETRY_DELAYS_MS) {
      await vi.advanceTimersByTimeAsync(delay);
    }
    await expect(p).rejects.toBe(netErr);
    expect(fn).toHaveBeenCalledTimes(1 + RETRY_DELAYS_MS.length);
    vi.useRealTimers();
  });
});
