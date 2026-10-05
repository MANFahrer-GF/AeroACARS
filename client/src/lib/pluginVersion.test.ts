import { describe, it, expect } from "vitest";
import fs from "node:fs";
import path from "node:path";
import {
  PLUGIN_AKTUELL,
  bandBrauchtUpdate,
  pluginUpdateEmpfohlen,
  versionKleiner,
} from "./pluginVersion";

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

describe("pluginUpdateEmpfohlen", () => {
  const basis = { active: true, protokoll: 2 as const, veraltet: false };
  it("meldet 1.1.0 (kann das Band, aber nicht auf dem zweiten Monitor)", () => {
    expect(pluginUpdateEmpfohlen({ ...basis, plugin_version: "1.1.0" })).toBe(true);
  });
  it("schweigt beim aktuellen und neueren Plugin, unter 1.1.0 (eigener Hinweis), ohne Plugin und bei unlesbarer Version", () => {
    expect(pluginUpdateEmpfohlen({ ...basis, plugin_version: PLUGIN_AKTUELL })).toBe(false);
    expect(pluginUpdateEmpfohlen({ ...basis, plugin_version: "1.2.0" })).toBe(false);
    expect(pluginUpdateEmpfohlen({ ...basis, plugin_version: "1.0.0" })).toBe(false);
    expect(pluginUpdateEmpfohlen({ ...basis, active: false, plugin_version: "1.1.0" })).toBe(false);
    expect(pluginUpdateEmpfohlen({ ...basis, veraltet: true, plugin_version: "1.1.0" })).toBe(false);
    expect(pluginUpdateEmpfohlen({ ...basis, plugin_version: "dev" })).toBe(false);
    expect(pluginUpdateEmpfohlen(null)).toBe(false);
  });
  it("PLUGIN_AKTUELL ist die Version aus xplane-plugin/CMakeLists.txt", () => {
    const cmake = fs.readFileSync(
      path.resolve(__dirname, "../../../xplane-plugin/CMakeLists.txt"),
      "utf-8",
    );
    expect(/^\s*VERSION (\d+\.\d+\.\d+)\s*$/m.exec(cmake)?.[1]).toBe(PLUGIN_AKTUELL);
  });
});
