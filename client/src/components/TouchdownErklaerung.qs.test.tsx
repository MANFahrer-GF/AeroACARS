// QS 06.10.2026 (Runde 6): Die (i)-Erklärungen der Touchdown-Kacheln passen
// zur Version, mit der bewertet wurde (Altbestand wird nie neu gerechnet),
// und nennen bei X-Plane die Umrechnung der G-Kraft (`g_auf_referenzkette`).
import { describe, expect, it, vi } from "vitest";
import { render } from "@testing-library/react";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";

vi.mock("./InfoBadge", () => ({
  InfoBadge: ({ explanation }: { explanation: string }) => <span data-erklaerung>{explanation}</span>,
}));
const { TouchdownAbschnitt } = await import("./TouchdownAbschnitt");

const erklaerungen = (extra: Partial<LandingRecord>) => {
  const r = { ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord), ...extra };
  const { container } = render(<TouchdownAbschnitt record={r} />);
  return Array.from(container.querySelectorAll("[data-erklaerung]")).map((e) => e.textContent ?? "");
};

describe("Erklärungen der Touchdown-Kacheln", () => {
  it("Version 19: Punkte-Grenzen; X-Plane mit Umrechnung, MSFS ohne", () => {
    const msfs = erklaerungen({ score_algorithm_version: 19, sim_kind: "msfs" }).join(" | ");
    expect(msfs).toContain("unter 1,20 G 100 Punkte");
    expect(msfs).not.toContain("X-Plane:");
    const xp = erklaerungen({ score_algorithm_version: 19, sim_kind: "xplane" }).join(" | ");
    expect(xp).toContain("aus 1,90 G werden 1,39 G");
  });
  it("Altbestand: keine v19-Grenzen, Hinweis auf die damalige Version", () => {
    const alt = erklaerungen({ score_algorithm_version: 17, sim_kind: "xplane" }).join(" | ");
    expect(alt).not.toMatch(/unter 1,20 G 100 Punkte|90–249 fpm 100 Punkte|Ab zwei Hopsern/);
    expect(alt).toContain("damaligen Score-Version");
    expect(alt).not.toContain("X-Plane:");
  });
});
