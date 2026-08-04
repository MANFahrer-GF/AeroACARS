// v0.19.x FIX: RunwayDiagramV2's six SVG <title> hover tooltips (threshold,
// runway end, TDZ, aim point, touchdown point, brake point) were hardcoded
// German prose, never routed through t() — an English/Italian pilot
// hovering the diagram on the post-landing debrief screen saw raw German
// regardless of their chosen locale. Pins that the tooltip text now
// follows the active language.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, cleanup } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import { RunwayDiagramV2, type RunwayDiagramV2Props } from "./RunwayDiagramV2";

beforeAll(async () => {
  if (!i18next.isInitialized) {
    await i18next.use(initReactI18next).init({
      lng: "de",
      resources: { de: { common: deCommon }, en: { common: enCommon } },
      defaultNS: "common",
      interpolation: { escapeValue: false },
    });
  }
});

afterEach(async () => {
  cleanup();
  await i18next.changeLanguage("de");
});

function props(overrides: Partial<RunwayDiagramV2Props> = {}): RunwayDiagramV2Props {
  return {
    airport_ident: "EDDF",
    runway_ident: "25C",
    length_m: 4000,
    source: "navigraph",
    td_distance_from_threshold_m: 350,
    td_centerline_offset_m: 2,
    td_tdz_length_m: 900,
    aim_point_m: 300,
    ...overrides,
  };
}

function titles(container: HTMLElement): string[] {
  return [...container.querySelectorAll("title")].map((t) => t.textContent ?? "");
}

describe("RunwayDiagramV2 — hover tooltips follow the active locale", () => {
  it("shows German tooltip prose under de, different English prose under en", async () => {
    await i18next.changeLanguage("de");
    const de = render(<RunwayDiagramV2 {...props()} />);
    const deTitles = titles(de.container).join("\n");
    expect(deTitles).toContain("Landeschwelle (Threshold)");
    expect(deTitles).toContain("Bahn-Ende");
    expect(deTitles).toContain("Aufsetzpunkt (Touchdown)");
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(<RunwayDiagramV2 {...props()} />);
    const enTitles = titles(en.container).join("\n");
    expect(enTitles).toContain("Landing threshold");
    expect(enTitles).toContain("Runway end");
    expect(enTitles).toContain("Touchdown point");
    expect(enTitles).not.toContain("Landeschwelle");
    expect(enTitles).not.toContain("Bahn-Ende");
  });

  it("localizes the direction words inside the touchdown tooltip, not just the surrounding prose", async () => {
    // Before the threshold, right of the centerline.
    await i18next.changeLanguage("de");
    const de = render(
      <RunwayDiagramV2 {...props({ td_distance_from_threshold_m: -20, td_centerline_offset_m: 3 })} />,
    );
    const deTitle = titles(de.container).find((t) => t.includes("Aufsetzpunkt"))!;
    expect(deTitle).toContain("vor");
    expect(deTitle).toContain("rechts");
    de.unmount();

    await i18next.changeLanguage("en");
    const en = render(
      <RunwayDiagramV2 {...props({ td_distance_from_threshold_m: -20, td_centerline_offset_m: 3 })} />,
    );
    const enTitle = titles(en.container).find((t) => t.includes("Touchdown point"))!;
    expect(enTitle).toContain("before");
    expect(enTitle).toContain("right of");
    expect(enTitle).not.toContain("vor ");
    expect(enTitle).not.toContain("rechts");
  });
});
