// QS-Abnahme 06.10.2026: Die Wetter-Übersetzung suchte im schon ersetzten
// Text weiter. Enthielt der Ersatz selbst einen Code (fehlende Übersetzung →
// Schlüssel „landing.metar.wx.RA"), wuchs der Text endlos — die ganze
// Landungsseite hing (echte Landung EDLP, „-RA").
import { describe, expect, it } from "vitest";
import { metarAuswertung } from "./MetarAbschnitt";

describe("METAR-Wetter", () => {
  it("endet auch, wenn die Übersetzung den Code enthält", () => {
    const t = (k: string) => k; // wie eine fehlende Übersetzung
    const zeilen = metarAuswertung(t as never, "METAR EDLP 011920Z 18005KT 9999 -RA FEW046 14/12 Q1026");
    const wetter = zeilen.map((z) => z.v).join(" | ");
    expect(wetter).toContain("landing.metar.wx.RA");
    expect(wetter.length).toBeLessThan(500);
  });
  it("übersetzt mehrere Codes nacheinander", () => {
    const t = (k: string) => ({ "landing.metar.wx.SH": "Schauer", "landing.metar.wx.RA": "Regen", "landing.metar.leicht": "leicht" })[k] ?? k;
    const zeilen = metarAuswertung(t as never, "METAR EDDF 011920Z 18005KT 9999 -SHRA FEW046 14/12 Q1026");
    expect(zeilen.map((z) => z.v).join(" | ")).toContain("leicht SchauerRegen");
  });
});
