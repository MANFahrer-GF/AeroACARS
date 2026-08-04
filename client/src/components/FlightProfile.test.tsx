// v0.19.x FIX: the terrain-fill polygon used to draw `y(v ?? 0)` for every
// sample — a missing (null) terrain reading fell back to 0 ft MSL, which is
// sea level under an aircraft over the Alps. The line series (MSL/AGL/speed)
// already broke into separate segments at null gaps instead of interpolating
// through them; the fill silhouette must do the same instead of drawing a
// fake ground level.

import { describe, it, expect } from "vitest";
import { render } from "@testing-library/react";
import { FlightProfile, type ProfilePt } from "./FlightProfile";

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
