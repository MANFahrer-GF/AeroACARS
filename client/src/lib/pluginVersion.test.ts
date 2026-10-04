import { describe, it, expect } from "vitest";
import { bandBrauchtUpdate, versionKleiner } from "./pluginVersion";

describe("versionKleiner", () => {
  it("vergleicht numerisch, nicht als Text", () => {
    expect(versionKleiner("1.0.0", "1.1.0")).toBe(true);
    expect(versionKleiner("1.1.0", "1.1.0")).toBe(false);
    expect(versionKleiner("1.10.0", "1.9.0")).toBe(false);
    expect(versionKleiner("1.2", "1.10.0")).toBe(true);
  });
  it("unlesbar ist kein Urteil", () => {
    expect(versionKleiner("dev", "1.1.0")).toBeNull();
  });
});

describe("bandBrauchtUpdate", () => {
  const basis = { active: true, protokoll: 2 as const, veraltet: false };
  it("meldet Plugin 1.0.0 mit Protokoll 2", () => {
    expect(bandBrauchtUpdate({ ...basis, plugin_version: "1.0.0" })).toBe(true);
  });
  it("schweigt ab 1.1.0, ohne Plugin, bei Protokoll 1 (eigener Hinweis) und bei unlesbarer Version", () => {
    expect(bandBrauchtUpdate({ ...basis, plugin_version: "1.1.0" })).toBe(false);
    expect(bandBrauchtUpdate({ ...basis, active: false, plugin_version: "1.0.0" })).toBe(false);
    expect(bandBrauchtUpdate({ ...basis, veraltet: true, plugin_version: "0.5.13" })).toBe(false);
    expect(bandBrauchtUpdate({ ...basis, plugin_version: "dev" })).toBe(false);
    expect(bandBrauchtUpdate(null)).toBe(false);
  });
});
