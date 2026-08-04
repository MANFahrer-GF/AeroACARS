// v0.19.x FIX: RunwayDiagramV2's six SVG <title> hover tooltips (threshold,
// runway end, TDZ, aim point, touchdown point, brake point) were hardcoded
// German prose, never routed through t() — an English/Italian pilot
// hovering the diagram on the post-landing debrief screen saw raw German
// regardless of their chosen locale. Pins that the tooltip text now
// follows the active language.

import { describe, it, expect, beforeAll, afterEach, vi } from "vitest";
import { render, cleanup, screen } from "@testing-library/react";
import i18next from "i18next";
import { initReactI18next } from "react-i18next";
import deCommon from "../locales/de/common.json";
import enCommon from "../locales/en/common.json";
import { DEFAULT_SKIN, type V2Skin } from "./runwayV2Skin";

const skinBox = vi.hoisted(() => ({ current: null as V2Skin | null }));
vi.mock("./SkinContext", async () => {
  const actual = await vi.importActual<typeof import("./runwayV2Skin")>("./runwayV2Skin");
  return { useV2Skin: () => skinBox.current ?? actual.DEFAULT_SKIN };
});

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
  skinBox.current = null;
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

function polygonPoints(el: Element): number[][] {
  return el
    .getAttribute("points")!
    .trim()
    .split(/\s+/)
    .map((pair) => pair.split(",").map(Number));
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

// v0.19.x FIX: the runway-utilization percentage ("Bahn-Auslastung")
// divided by a defensively-floored `Math.max(500, length_m)` instead of
// the real LDA — that floor exists purely to keep the SVG geometry from
// degenerating on missing/corrupt data, but leaked into the SCORE math
// too. For a genuine short strip under 500 m LDA (bush/VFR fields, an
// explicitly supported case), utilization was computed against a
// fictionally inflated runway and read too LOW — a genuinely tight
// landing looked comfortable.
// v0.19.x FIX: the lateral-offset arrowhead's tip/base vertices were
// swapped — a proper arrowhead's single "tip" vertex must be the FURTHEST
// point in the pointing direction, with the flared 2-point base behind it.
// The old code had it backwards: the arrow visually pointed BACK toward
// the touchdown dot instead of away from it in the LEFT/RIGHT direction
// stated by the text label right next to it.
describe("RunwayDiagramV2 — lateral-offset arrowhead points away from the touchdown dot", () => {
  it("points LEFT (tip has the smallest x) when the touchdown is left of centerline", () => {
    const { container } = render(
      <RunwayDiagramV2
        {...props({ aim_point_m: null, td_distance_from_threshold_m: 2000, td_centerline_offset_m: -5 })}
      />,
    );
    const polygon = container.querySelector("polygon");
    expect(polygon, "offset arrowhead must render for |offset| > 0.5 m").not.toBeNull();
    const xs = polygonPoints(polygon!).map((p) => p[0]);
    // The tip is the vertex whose x is unique; the base pair shares one x.
    const tipX = xs.find((x) => xs.filter((v) => v === x).length === 1)!;
    const baseX = xs.find((x) => xs.filter((v) => v === x).length === 2)!;
    expect(tipX, `tip=${tipX} base=${baseX}`).toBeLessThan(baseX);
  });

  it("points RIGHT (tip has the largest x) when the touchdown is right of centerline", () => {
    const { container } = render(
      <RunwayDiagramV2
        {...props({ aim_point_m: null, td_distance_from_threshold_m: 2000, td_centerline_offset_m: 5 })}
      />,
    );
    const polygon = container.querySelector("polygon");
    const xs = polygonPoints(polygon!).map((p) => p[0]);
    const tipX = xs.find((x) => xs.filter((v) => v === x).length === 1)!;
    const baseX = xs.find((x) => xs.filter((v) => v === x).length === 2)!;
    expect(tipX, `tip=${tipX} base=${baseX}`).toBeGreaterThan(baseX);
  });
});

describe("RunwayDiagramV2 — runway-utilization percentage ignores the SVG-geometry floor", () => {
  it("computes utilization against the real (sub-500m) LDA, not the floored value", () => {
    // 300 m LDA, td=100 m past threshold, rollout=150 m -> used = max(100+150, 150) = 250 m.
    // Correct: 250 / 300 = 83%. Buggy (floored to 500): 250 / 500 = 50%.
    const { container } = render(
      <RunwayDiagramV2
        {...props({
          length_m: 300,
          td_distance_from_threshold_m: 100,
          rollout_m: 150,
        })}
      />,
    );
    expect(container.textContent).toContain("83 %");
    expect(container.textContent).not.toContain("50 %");
  });

  it("is unaffected for a normal-length runway (>= 500 m), matching the pre-fix output", () => {
    // 3000 m LDA, td=500, rollout=500 -> used = max(500+500,500) = 1000 -> 1000/3000 = 33%.
    const { container } = render(
      <RunwayDiagramV2
        {...props({
          length_m: 3000,
          td_distance_from_threshold_m: 500,
          rollout_m: 500,
        })}
      />,
    );
    expect(container.textContent).toContain("33 %");
  });
});

// v0.19.x FIX: `V2Skin.thresholds` (peak_g_warn/bad, crosswind_warn/bad,
// bank_warn_above, pitch_bad_below, bahn_auslastung_warn_above,
// centerline_warn_above/bad_above, hinter_schwelle_warn_above) was defined,
// defaulted and merged, but every tone decision in the component used a
// HARDCODED magic number instead of reading it — a VA admin changing the
// deployed VPS skin's thresholds would have had zero effect. These prove
// a non-default skin's thresholds now actually change what the pilot sees.
describe("RunwayDiagramV2 — skin thresholds are actually read, not just hardcoded copies", () => {
  it("re-tones the runway-utilization pill when the skin raises the warn threshold", () => {
    // 90% utilization: DEFAULT_SKIN.thresholds.bahn_auslastung_warn_above
    // is 85, so this must read "warn" (amber) by default...
    const p = props({ length_m: 1000, td_distance_from_threshold_m: 0, rollout_m: 900, aim_point_m: null });
    const def = render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText("90 %")).toHaveStyle({ color: "#fbbf24" });
    def.unmount();

    // ...but with a skin that raises the threshold to 95, the SAME 90%
    // must read as "good" (green) instead.
    skinBox.current = {
      ...DEFAULT_SKIN,
      thresholds: { ...DEFAULT_SKIN.thresholds, bahn_auslastung_warn_above: 95 },
    };
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText("90 %")).toHaveStyle({ color: "#22c55e" });
  });

  it("re-tones the peak-G readout in the aircraft bar when the skin lowers peak_g_warn", () => {
    const p = props({ aircraft_icao: "A320", landing_peak_g_force: 1.3 });
    const def = render(<RunwayDiagramV2 {...p} />);
    // 1.3 g is below DEFAULT_SKIN's peak_g_warn (1.5) -> green, no warning.
    expect(screen.getByText("1.30 g")).toHaveStyle({ color: "#22c55e" });
    def.unmount();

    // A skin lowering peak_g_warn to 1.0 must flag the SAME 1.3 g as amber.
    skinBox.current = {
      ...DEFAULT_SKIN,
      thresholds: { ...DEFAULT_SKIN.thresholds, peak_g_warn: 1.0 },
    };
    render(<RunwayDiagramV2 {...p} />);
    expect(screen.getByText("1.30 g")).toHaveStyle({ color: "#fbbf24" });
  });
});
