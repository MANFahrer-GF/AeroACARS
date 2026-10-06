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

function rgb(c: string): [number, number, number] | null {
  const m = c.match(/rgba?\((\d+),\s*(\d+),\s*(\d+)/);
  return m ? [Number(m[1]), Number(m[2]), Number(m[3])] : null;
}
function leuchte([r, g, b]: [number, number, number]): number {
  const k = (v: number) => { const x = v / 255; return x <= 0.03928 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4; };
  return 0.2126 * k(r) + 0.7152 * k(g) + 0.0722 * k(b);
}
function kontrast(vorne: string, hinten: string): number {
  const a = rgb(vorne), b = rgb(hinten);
  if (!a || !b) return 0;
  const [h, d] = [leuchte(a), leuchte(b)].sort((x, y) => y - x);
  return (h! + 0.05) / (d! + 0.05);
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
      // Elemente mit eigenem Hintergrund regeln ihre Druckfarben selbst: dann
      // muss der Kontrast schon stimmen (≥ 4,5:1) oder das Element ist im
      // Druck ausgeblendet (Runde „Abnahme": der Filter dunkelte sonst auch
      // den Hintergrund ab, die helle Bahn-Warnung wurde dunkel).
      const deckend = (e: HTMLElement) => /background(-color)?: rgb\(/.test(e.getAttribute("style") ?? "");
      const nichtErfasst = farbig.filter((e) => {
        if (deckend(e)) {
          const bg = e.style.backgroundColor || e.style.background;
          // Deckender Kasten: entweder ausgenommen (papierfest/Bahn-Warnung)
          // und dann mit ausreichendem Kontrast, oder unter der Druckregel.
          if (e.closest(".bahn-warnung")) return false;
          if (!e.closest(".papierfest")) return !sel.some((s) => e.matches(s) || e.closest(s) != null);
          return !(
            kontrast(e.style.color, bg) >= 4.5 ||
            e.closest(".nur-bildschirm, .bahn-nur-bildschirm") != null
          );
        }
        return !sel.some((s) => e.matches(s) || e.closest(s) != null);
      });
      // Die allgemeine Regel (Inline-Farbe) darf keinen Kasten mit eigenem
      // Hintergrund treffen — `.farbwert` setzt der Baustein dagegen bewusst.
      const allgemein = sel.filter((x) => x.includes("[style"));
      const abgedunkelteKaesten = Array.from(container.querySelectorAll<HTMLElement>("[style]")).filter(
        (e) => /background(-color)?: rgb\(/.test(e.getAttribute("style") ?? "") && allgemein.some((x) => e.matches(x)),
      );
      expect(abgedunkelteKaesten.map((e) => e.textContent?.slice(0, 30))).toEqual([]);
      expect(nichtErfasst.map((e) => `${e.tagName} ${e.getAttribute("style") ?? ""} ${e.textContent?.slice(0, 30)}`)).toEqual([]);
      unmount();
    }
  });
});
