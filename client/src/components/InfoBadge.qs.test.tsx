// QS 06.10.2026: Das (i)-Fenster liegt `position: fixed`. In der Webapp
// steckt die Landungsanalyse in einem Modal mit `backdrop-filter` — das macht
// das Modal zum Bezugsrahmen für fixed, und das Fenster saß um dessen Rand
// versetzt (und wurde an dessen `overflow: hidden` abgeschnitten).
import { describe, expect, it, vi } from "vitest";
import { fireEvent, render } from "@testing-library/react";
import { InfoBadge } from "./InfoBadge";

function rechteck(left: number, top: number, w: number, h: number): DOMRect {
  return { left, top, width: w, height: h, right: left + w, bottom: top + h, x: left, y: top, toJSON: () => ({}) } as DOMRect;
}

function oeffnen(rahmenStil: string) {
  const { container } = render(
    <div data-rahmen style={{ cssText: rahmenStil } as unknown as React.CSSProperties}>
      <InfoBadge explanation="Erklärung" />
    </div>,
  );
  const rahmen = container.querySelector("[data-rahmen]") as HTMLElement;
  rahmen.setAttribute("style", rahmenStil);
  vi.spyOn(rahmen, "getBoundingClientRect").mockReturnValue(rechteck(100, 80, 800, 600));
  const knopf = container.querySelector(".info-badge") as HTMLElement;
  vi.spyOn(knopf, "getBoundingClientRect").mockReturnValue(rechteck(300, 200, 16, 16));
  fireEvent.click(knopf);
  return container.querySelector(".info-badge__popover") as HTMLElement;
}

describe("(i)-Fenster im Bezugsrahmen", () => {
  it("ohne Rahmen: Bildschirm-Koordinaten unter dem Knopf", () => {
    const f = oeffnen("");
    expect([f.style.left, f.style.top]).toEqual(["292px", "224px"]);
  });

  // jsdom kennt `backdrop-filter` nicht (der Browser schon) — `filter`
  // macht denselben Bezugsrahmen und läuft über dieselbe Erkennung.
  it("im Modal mit Filter: gegen dessen Rand verrechnet", () => {
    const f = oeffnen("filter: blur(20px)");
    expect([f.style.left, f.style.top]).toEqual(["192px", "144px"]);
  });

  it("bleibt innerhalb des Rahmens (rechter Rand)", () => {
    const { container } = render(
      <div data-rahmen style={{ transform: "translateX(0)" }}>
        <InfoBadge explanation="Erklärung" />
      </div>,
    );
    const rahmen = container.querySelector("[data-rahmen]") as HTMLElement;
    vi.spyOn(rahmen, "getBoundingClientRect").mockReturnValue(rechteck(100, 80, 400, 600));
    const knopf = container.querySelector(".info-badge") as HTMLElement;
    vi.spyOn(knopf, "getBoundingClientRect").mockReturnValue(rechteck(480, 200, 16, 16));
    fireEvent.click(knopf);
    const f = container.querySelector(".info-badge__popover") as HTMLElement;
    // Rahmen rechts bei 500: Fenster endet 8 px davor → left = 500-8-280 = 212, relativ 112.
    expect(f.style.left).toBe("112px");
  });
});
