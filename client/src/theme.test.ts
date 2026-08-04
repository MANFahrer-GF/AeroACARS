// v0.19.x FIX: getInitialTheme/applyTheme were the only unguarded localStorage
// calls in the lib layer, and applyTheme runs before the Sentry ErrorBoundary
// mounts — a throwing localStorage there used to mean a blank page with no
// error surfaced at all.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { getInitialTheme, applyTheme } from "./theme";

describe("theme localStorage resilience", () => {
  beforeEach(() => {
    document.documentElement.removeAttribute("data-theme");
  });

  it("getInitialTheme falls back to the media query when localStorage throws", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new DOMException("quota", "QuotaExceededError");
      },
      setItem: () => {
        throw new DOMException("quota", "QuotaExceededError");
      },
    });
    vi.stubGlobal("matchMedia", (query: string) => ({
      matches: query.includes("dark"),
      media: query,
    }));

    expect(() => getInitialTheme()).not.toThrow();
    expect(getInitialTheme()).toBe("dark");
  });

  it("applyTheme still updates the DOM when localStorage throws", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => null,
      setItem: () => {
        throw new DOMException("quota", "QuotaExceededError");
      },
    });

    expect(() => applyTheme("light")).not.toThrow();
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("round-trips a stored theme through localStorage when it works normally", () => {
    const mem = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => mem.get(k) ?? null,
      setItem: (k: string, v: string) => {
        mem.set(k, v);
      },
    });

    applyTheme("dark");
    expect(getInitialTheme()).toBe("dark");
  });
});
