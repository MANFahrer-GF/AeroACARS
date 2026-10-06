// QS 06.10.2026: Sprit im Druck — kein ⓘ, und der Grund für einen leeren
// Wert ist der tatsächliche (Runde 4: „kein OFP …" stand neben „kein
// Landesprit erfasst").
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { SpritSektion } from "./SpritSektion";
import { DruckKontext } from "../lib/druck";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import type { SpritAuswertung } from "../lib/sprit";

const sprit = (): SpritAuswertung => {
  const s = MOCK_LANDING_OPTIONS.map((o) => (o.build() as { sprit?: SpritAuswertung }).sprit).find(Boolean)!;
  return { ...s, reserve: { ...s.reserve, status: "nicht_pruefbar", grund: "kein_landesprit" } } as SpritAuswertung;
};

describe("Sprit im Druck", () => {
  it("leere Final Reserve: der tatsächliche Grund, kein ⓘ", () => {
    const { container } = render(
      <DruckKontext.Provider value={true}>
        <SpritSektion sprit={sprit()} />
      </DruckKontext.Provider>,
    );
    const t = container.textContent ?? "";
    expect(t).toContain("kein Landesprit erfasst");
    expect(t).not.toContain("kein OFP oder Tankwert unplausibel");
    expect(t).not.toContain("ⓘ");
  });

  it("Gegenprobe: auf dem Bildschirm bleibt das ⓘ", () => {
    const { container } = render(<SpritSektion sprit={sprit()} />);
    expect(container.textContent).toContain("ⓘ");
  });
});
