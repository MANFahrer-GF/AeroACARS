// QS 06.10.2026: Wind-Block im Touchdown-Abschnitt (gespiegelt) — nichts
// erfinden, nichts weglassen.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { TouchdownAbschnitt } from "./TouchdownAbschnitt";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";

const mit = (headwind_kt: number | null, crosswind_kt: number | null) => {
  const r = { ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord), headwind_kt, crosswind_kt };
  return render(<TouchdownAbschnitt record={r} />).container.querySelector(".windflow")!.textContent ?? "";
};

describe("Wind-Block", () => {
  it("bei „Windstill“ stehen Seitenwind und Gesamtwind trotzdem da", () => {
    const t = mit(0.4, -1.2);
    expect(t).toContain("Windstill");
    expect(t).toContain("Seitenwind · von links 1 kt");
    expect(t).toContain("Gesamtwind 1 kt");
  });

  it("ohne gemessenen Seitenwind: keine 0 und keine erfundene Seite", () => {
    const t = mit(5, null);
    expect(t).toContain("—");
    expect(t).not.toMatch(/von rechts|von links/);
    expect(t).toContain("Gegenwind 5 kt");
    // Gegenprobe: mit Seitenwind steht die Seite da.
    expect(mit(5, 8)).toContain("von rechts");
  });
});
