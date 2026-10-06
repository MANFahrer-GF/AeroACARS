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
  // Wie in der Webapp: Hülle `.la-shell` (Ziel des Fensters) in einem
  // Rahmen, der den Bezugsrahmen für `position: fixed` stellt.
  const { container } = render(
    <div data-rahmen style={{ cssText: rahmenStil } as unknown as React.CSSProperties}>
      <div className="la-shell">
        <InfoBadge explanation="Erklärung" />
      </div>
    </div>,
  );
  const rahmen = container.querySelector("[data-rahmen]") as HTMLElement;
  rahmen.setAttribute("style", rahmenStil);
  vi.spyOn(rahmen, "getBoundingClientRect").mockReturnValue(rechteck(100, 80, 800, 600));
  const knopf = container.querySelector(".info-badge") as HTMLElement;
  vi.spyOn(knopf, "getBoundingClientRect").mockReturnValue(rechteck(300, 200, 16, 16));
  fireEvent.click(knopf);
  return document.querySelector(".info-badge__popover") as HTMLElement;
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
        <div className="la-shell">
          <InfoBadge explanation="Erklärung" />
        </div>
      </div>,
    );
    const rahmen = container.querySelector("[data-rahmen]") as HTMLElement;
    vi.spyOn(rahmen, "getBoundingClientRect").mockReturnValue(rechteck(100, 80, 400, 600));
    const knopf = container.querySelector(".info-badge") as HTMLElement;
    vi.spyOn(knopf, "getBoundingClientRect").mockReturnValue(rechteck(480, 200, 16, 16));
    fireEvent.click(knopf);
    const f = document.querySelector(".info-badge__popover") as HTMLElement;
    // Rahmen rechts bei 500: Fenster endet 8 px davor → left = 500-8-280 = 212, relativ 112.
    expect(f.style.left).toBe("112px");
  });
});

// QS 06.10.2026: per Tastatur ließen sich beliebig viele (i) öffnen; die
// Rolle „tooltip" passte nicht zu einem Fenster mit Schließen-Knopf.
describe("(i) — eines offen, Tastatur, Rollen", () => {
  it("öffnet sich ein zweites, schließt das erste", () => {
    const { container } = render(
      <div>
        <InfoBadge explanation="Eins" />
        <InfoBadge explanation="Zwei" />
      </div>,
    );
    const [a, b] = [...container.querySelectorAll(".info-badge")] as HTMLElement[];
    fireEvent.click(a!);
    expect(document.body.textContent).toContain("Eins");
    fireEvent.click(b!);
    expect(document.body.textContent).toContain("Zwei");
    expect(document.body.textContent).not.toContain("Eins");
  });

  it("Escape schließt und gibt den Fokus an den Knopf zurück; Rolle dialog", () => {
    const { container } = render(<InfoBadge explanation="Text" />);
    const k = container.querySelector(".info-badge") as HTMLElement;
    fireEvent.click(k);
    const f = document.querySelector(".info-badge__popover") as HTMLElement;
    expect(f.getAttribute("role")).toBe("dialog");
    expect(k.getAttribute("aria-controls")).toBe(f.id);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(document.querySelector(".info-badge__popover")).toBeNull();
    expect(document.activeElement).toBe(k);
  });
});

// QS 06.10.2026: In einer Kachel mit `opacity` war das Fenster durchsichtig
// und wurde von Nachbarkacheln überdeckt — es hängt jetzt außerhalb.
describe("(i) — Fenster hängt nicht in der Kachel", () => {
  it("Fenster liegt nicht im halbtransparenten Vorfahren; Klick hinein schließt nicht", () => {
    const { container } = render(
      <div className="kachel" style={{ opacity: 0.75 }}>
        <InfoBadge explanation="Erklärtext" />
      </div>,
    );
    fireEvent.click(container.querySelector(".info-badge") as HTMLElement);
    const f = document.querySelector(".info-badge__popover") as HTMLElement;
    expect(f).not.toBeNull();
    expect(container.querySelector(".kachel")!.contains(f)).toBe(false);
    fireEvent.pointerDown(f);
    expect(document.querySelector(".info-badge__popover")).not.toBeNull();
    fireEvent.pointerDown(document.body);
    expect(document.querySelector(".info-badge__popover")).toBeNull();
  });
});

// QS 06.10.2026 (Runde 5): Im Client hängt das Fenster direkt an <body>,
// außerhalb von #root — der Druck blendet nur #root-Fremdes nicht aus. Ein
// per Tastatur offenes Fenster (Tab zum PDF-Knopf schließt es nicht) kam so
// mit Erklärtext und ×-Knopf aufs Papier.
describe("(i)-Fenster im Druck", () => {
  it("die Druckregel trifft das offene Fenster", async () => {
    const { readFileSync } = await import("node:fs");
    const { resolve } = await import("node:path");
    const style = document.createElement("style");
    style.textContent = readFileSync(resolve(__dirname, "infoBadge.css"), "utf-8");
    document.head.appendChild(style);
    const { container } = render(<InfoBadge explanation="Erklärung" />);
    fireEvent.keyDown(container.querySelector(".info-badge") as HTMLElement, { key: "Enter" });
    fireEvent.click(container.querySelector(".info-badge") as HTMLElement);
    const fenster = document.querySelector(".info-badge__popover") as HTMLElement;
    expect(fenster).not.toBeNull();
    expect(fenster.parentElement).toBe(document.body);
    const druckRegeln = Array.from(style.sheet!.cssRules)
      .filter((r): r is CSSMediaRule => r instanceof CSSMediaRule && /print/.test(r.media.mediaText))
      .flatMap((m) => Array.from(m.cssRules) as CSSStyleRule[]);
    const verbirgt = druckRegeln.some(
      (r) => fenster.matches(r.selectorText) && r.style.getPropertyValue("display") === "none",
    );
    expect(verbirgt).toBe(true);
    style.remove();
  });
});
