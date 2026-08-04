// v0.19.x FIX: the terrain-fill polygon used to draw `y(v ?? 0)` for every
// sample — a missing (null) terrain reading fell back to 0 ft MSL, which is
// sea level under an aircraft over the Alps. The line series (MSL/AGL/speed)
// already broke into separate segments at null gaps instead of interpolating
// through them; the fill silhouette must do the same instead of drawing a
// fake ground level.

import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { FlightProfile, findHoverIndex, type ProfilePt } from "./FlightProfile";

describe("FlightProfile terrain fill", () => {
  it("draws one polygon per gap-free terrain run instead of one polygon spanning a null gap", () => {
    const route: ProfilePt[] = [
      { t: 0, alt_ft: 3000, gnd_ft: 1000 },
      { t: 1, alt_ft: 3100, gnd_ft: 1200 },
      { t: 2, alt_ft: 3200, gnd_ft: null }, // terrain unknown here
      { t: 3, alt_ft: 3300, gnd_ft: null }, // terrain unknown here
      { t: 4, alt_ft: 3400, gnd_ft: 2000 },
      { t: 5, alt_ft: 3500, gnd_ft: 2200 },
    ];
    const { container } = render(<FlightProfile route={route} />);

    const altBand = container.querySelector(".aa-fp-band");
    expect(altBand).not.toBeNull();
    const polygons = altBand!.querySelectorAll("polygon");

    // One run for indices [0,1], one for [4,5] — the null gap at [2,3] must
    // NOT be bridged by a third polygon or a single polygon spanning all 6.
    expect(polygons).toHaveLength(2);
  });

  it("draws a single unbroken polygon when terrain has no gaps", () => {
    const route: ProfilePt[] = [
      { t: 0, alt_ft: 3000, gnd_ft: 1000 },
      { t: 1, alt_ft: 3100, gnd_ft: 1100 },
      { t: 2, alt_ft: 3200, gnd_ft: 1200 },
    ];
    const { container } = render(<FlightProfile route={route} />);
    const altBand = container.querySelector(".aa-fp-band");
    const polygons = altBand!.querySelectorAll("polygon");
    expect(polygons).toHaveLength(1);
  });

  it("omits the fill entirely when no terrain data is present at all", () => {
    const route: ProfilePt[] = [
      { t: 0, alt_ft: 3000, gnd_ft: null },
      { t: 1, alt_ft: 3100, gnd_ft: null },
    ];
    const { container } = render(<FlightProfile route={route} />);
    const altBand = container.querySelector(".aa-fp-band");
    const polygons = altBand!.querySelectorAll("polygon");
    expect(polygons).toHaveLength(0);
  });
});

// v0.19.x FIX: the crosshair line is drawn at x(i) = (t(i)/tMax) * W —
// proportional to TIMESTAMP. hoverIdx used to be picked by treating the
// mouse's X-percentage as a linear fraction of the point ARRAY instead,
// which only agrees with x(i) when samples are evenly time-spaced. Real
// telemetry isn't (cruise ticks are sparser than approach ticks).
describe("findHoverIndex", () => {
  it("picks the point whose TIME matches the hover position, not the one at the linear array-index fraction (the exact bug)", () => {
    // Dense sampling for the first half of the flight (indices 0-4, t
    // 0..400), then one giant gap to a single late sample (index 5,
    // t=10000). Old code: hover at 50% -> round(0.5 * 5) = index 3 (an
    // early, densely-sampled point). Correct: hover at 50% of the TIME
    // axis (t=5000) is still much closer to the dense cluster than to
    // the late outlier, so it must still resolve within that cluster —
    // the old code got that right by accident here. The real divergence
    // shows at a hover position that falls in the middle of the BIG gap.
    const pts: ProfilePt[] = [
      { t: 0, alt_ft: 1000 },
      { t: 100, alt_ft: 1100 },
      { t: 200, alt_ft: 1200 },
      { t: 300, alt_ft: 1300 },
      { t: 400, alt_ft: 1400 },
      { t: 10000, alt_ft: 2000 },
    ];
    const tMax = 10000;
    // Hover at 95% of the timeline -> target t = 9500, which is far
    // closer (by time) to the t=10000 outlier than to the t=400 cluster.
    // The old index-linear logic would pick round(0.95 * 5) = index 5
    // too, by coincidence — so pick a position that actually diverges:
    // hover at 45% -> target t = 4500, closest by TIME to... still index
    // 4 (t=400) since |4500-400| < |4500-10000|. Old logic: round(0.45*5)
    // = round(2.25) = index 2 (t=200) — a DIFFERENT point. This is the
    // exact mismatch: the crosshair (drawn by time) would sit far to the
    // right of the value the old readout showed (from index 2).
    const oldBuggyIndex = Math.round((45 / 100) * (pts.length - 1));
    const fixedIndex = findHoverIndex(pts, true, tMax, 45);

    expect(oldBuggyIndex).toBe(2);
    expect(fixedIndex).toBe(4);
    expect(fixedIndex).not.toBe(oldBuggyIndex);
  });

  it("picks the nearest point by time even when spacing is uneven", () => {
    const pts: ProfilePt[] = [
      { t: 0 }, { t: 10 }, { t: 20 }, { t: 1000 }, { t: 2000 },
    ];
    // Hover position exactly at t=15 -> equidistant-ish, nearer to t=10 (idx 1) vs t=20 (idx 2).
    expect(findHoverIndex(pts, true, 2000, (12 / 2000) * 100)).toBe(1);
    expect(findHoverIndex(pts, true, 2000, (18 / 2000) * 100)).toBe(2);
  });

  it("falls back to linear index spacing when no point has a timestamp", () => {
    const pts: ProfilePt[] = [{ alt_ft: 1 }, { alt_ft: 2 }, { alt_ft: 3 }, { alt_ft: 4 }, { alt_ft: 5 }];
    expect(findHoverIndex(pts, false, 4, 0)).toBe(0);
    expect(findHoverIndex(pts, false, 4, 50)).toBe(2);
    expect(findHoverIndex(pts, false, 4, 100)).toBe(4);
  });

  it("clamps out-of-range hover percentages instead of returning an out-of-bounds index", () => {
    const pts: ProfilePt[] = [{ t: 0 }, { t: 100 }];
    expect(findHoverIndex(pts, true, 100, -20)).toBe(0);
    expect(findHoverIndex(pts, true, 100, 500)).toBe(1);
  });
});
