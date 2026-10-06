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
  it("Altbestand: kein X-Plane-Zusatz (der Versionshinweis kommt vom Kontext)", () => {
    const alt = erklaerungen({ score_algorithm_version: 17, sim_kind: "xplane" }).join(" | ");
    expect(alt).not.toContain("X-Plane:");
  });
});

// Runde 8: Der Versionshinweis hängt an JEDER Erklärung einer Landung vor
// Version 19 — gesetzt an der Wurzel (`LandungsAbschnitte`), gelesen im echten
// InfoBadge (hier ohne Attrappe).
describe("Versionshinweis an allen Erklärungen", () => {
  it("Altbestand: jedes geöffnete Fenster trägt den Hinweis, Version 19 keines", async () => {
    vi.doUnmock("./InfoBadge");
    vi.resetModules();
    const { fireEvent } = await import("@testing-library/react");
    const { LandingDetail } = await import("./LandingPanel");
    const fenster = (version: number) => {
      const r = { ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord), score_algorithm_version: version };
      const { container, unmount } = render(<LandingDetail record={r} allRecords={[r]} onBack={() => {}} />);
      const knoepfe = Array.from(container.querySelectorAll(".info-badge")) as HTMLElement[];
      const texte = knoepfe.map((k) => {
        fireEvent.click(k);
        const f = document.querySelector(".info-badge__popover")?.textContent ?? "";
        fireEvent.click(k);
        return f;
      });
      unmount();
      return texte;
    };
    const alt = fenster(17);
    expect(alt.length).toBeGreaterThan(10);
    const HINWEIS = "Die Erklärung beschreibt die heutigen Regeln";
    for (const t of alt) {
      // Texte, die selbst die damaligen Regeln beschreiben (Runde 9), ohne
      // den Hinweis — alle übrigen mit, und nie doppelt.
      const eigeneAltfassung =
        t.startsWith("Anflug-Urteil für Flüge vor Score-Version 19") ||
        t.startsWith("Diese Landung wurde vor Score-Version 19 bewertet und behält");
      // Runde 12: Teilnoten, die es nur noch im Altbestand gibt (Sprit,
      // Loadsheet), tragen ihren eigenen Hinweis statt „heutige Regeln".
      const ALT_ACHSE = "gibt es nur bei Landungen vor Score-Version 19";
      if (eigeneAltfassung) expect(t).not.toContain(HINWEIS);
      else expect(t.includes(HINWEIS) !== t.includes(ALT_ACHSE), t).toBe(true);
      expect(t.split("vor Score-Version 19").length).toBeLessThanOrEqual(2);
    }
    expect(alt.some((t) => t.startsWith("Anflug-Urteil für Flüge vor Score-Version 19"))).toBe(true);
    expect(alt.some((t) => t.startsWith("Diese Landung wurde vor Score-Version 19 bewertet und behält"))).toBe(true);
    const neu = fenster(19);
    expect(neu.length).toBeGreaterThan(10);
    for (const t of neu) expect(t).not.toContain("vor Score-Version 19 bewertet");
  });
});
