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

// Runde 13: Bandfarben der Sprit-Zahlen (Grün/Gelb/Rot, für dunklen Grund)
// werden im Druck über `.farbwert` abgedunkelt — vorher Gelb auf Weiß.
describe("Sprit-Zahlen im Druck lesbar", () => {
  it("jeder Text in Bandfarbe trägt .farbwert", () => {
    const original = MOCK_LANDING_OPTIONS.map((o) => (o.build() as { sprit?: SpritAuswertung }).sprit).find(Boolean)!;
    const { container } = render(
      <DruckKontext.Provider value={true}>
        <SpritSektion sprit={original} />
      </DruckKontext.Provider>,
    );
    const BAND = ["rgb(34, 197, 94)", "rgb(242, 178, 76)", "rgb(255, 92, 77)"];
    const farbig = Array.from(container.querySelectorAll<HTMLElement>("*")).filter(
      (e) => BAND.includes(e.style.color) && (e.textContent ?? "").trim() !== "",
    );
    expect(farbig.length).toBeGreaterThan(0);
    for (const e of farbig) expect(e.closest(".farbwert"), e.textContent ?? "").not.toBeNull();
  });
});
