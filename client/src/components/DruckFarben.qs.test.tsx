// QS 06.10.2026 (Runde 14): Im PDF wird jeder Text mit eigener Farbe und jede
// Grafikbeschriftung abgedunkelt — eine Regel statt Stelle für Stelle
// `.farbwert` (Runde 13/14 fanden sie einzeln: Sprit je Wegpunkt,
// Touchdown-Marke, Grenzwert, „Gate"). Geprüft an gerenderten Abschnitten
// gegen die Selektoren der echten Druckregel aus App.css.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { DruckKontext } from "../lib/druck";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import THY from "../lib/landungsFaelle.thy39.json";

function druckSelektoren(): string[] {
  const css = readFileSync(resolve(__dirname, "..", "App.css"), "utf-8");
  const druck = css.slice(css.indexOf("@media print")).replace(/\/\*[\s\S]*?\*\//g, "");
  const sel: string[] = [];
  for (const m of druck.matchAll(/([^{}]+)\{[^{}]*filter:\s*brightness\(0\.55\)/g)) {
    for (const s of m[1]!.split(",")) {
      const t = s.replace(/\/\*[\s\S]*?\*\//g, "").trim();
      if (t) sel.push(t);
    }
  }
  return sel;
}

describe("Farbige Texte im Druck abgedunkelt", () => {
  it("jeder Text mit eigener Farbe und jede SVG-Beschriftung fällt unter die Druckregel", async () => {
    const sel = druckSelektoren();
    expect(sel.length).toBeGreaterThan(2);
    const { LandingDetail } = await import("./LandingPanel");
    for (const r of [THY as unknown as LandingRecord, MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord]) {
      const { container, unmount } = render(
        <DruckKontext.Provider value={true}>
          <div className="landing-report">
            <LandingDetail record={r} allRecords={[r]} onBack={() => {}} />
          </div>
        </DruckKontext.Provider>,
      );
      await new Promise((res) => setTimeout(res, 200));
      const farbig = Array.from(container.querySelectorAll<HTMLElement>("*")).filter((e) => {
        if ((e.textContent ?? "").trim() === "") return false;
        if (e.tagName.toLowerCase() === "text") return true;
        const c = e.style.color;
        return c !== "" && !c.startsWith("var(");
      });
      expect(farbig.length).toBeGreaterThan(5);
      const nichtErfasst = farbig.filter((e) => !sel.some((s) => e.matches(s) || e.closest(s) != null));
      expect(nichtErfasst.map((e) => `${e.tagName} ${e.getAttribute("style") ?? ""} ${e.textContent?.slice(0, 30)}`)).toEqual([]);
      unmount();
    }
  });
});
