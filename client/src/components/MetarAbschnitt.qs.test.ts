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
    expect(wetter).toContain("landing.metar.vorlage.leicht");
    expect(wetter.length).toBeLessThan(500);
  });
  it("grammatisch in jeder Sprache (Vorlagen statt Code-Reihenfolge)", async () => {
    const i18n = (await import("../i18n")).default;
    const wetter = async (lng: string, metar: string) => {
      await i18n.changeLanguage(lng);
      return metarAuswertung(i18n.t.bind(i18n) as never, metar).find((z) => z.id === "wetter")?.v;
    };
    const M = (wx: string) => `METAR EDDF 011920Z 18005KT 9999 ${wx} FEW046 14/12 Q1026`;
    expect(await wetter("de", M("-SHRA"))).toBe("Schauer mit Regen (leicht)");
    expect(await wetter("en", M("-SHRA"))).toBe("light rain showers");
    expect(await wetter("it", M("-SHRA"))).toBe("rovesci di pioggia (debole)");
    expect(await wetter("en", M("VCSH"))).toBe("showers in the vicinity");
    expect(await wetter("de", M("-FZDZ"))).toBe("gefrierender Niesel (leicht)");
    expect(await wetter("en", M("+TSRA"))).toBe("heavy thunderstorm with rain");
    await i18n.changeLanguage("de");
  });
  it("Sicht nur aus dem aktuellen Teil, auch weniger als eine Meile", () => {
    const t = (k: string) => ({ "landing.metar.sicht": "Sicht" })[k] ?? k;
    const sicht = (m: string) => metarAuswertung(t as never, m).find((z) => z.id === "sicht")?.v;
    expect(sicht("METAR KJFK 011951Z 18005KT 10SM FEW250 14/12 A3001 RMK AO2 WSHFT 1715 SLP123")).toBe("10 SM");
    expect(sicht("METAR KJFK 011951Z 18005KT M1/4SM FG VV002 14/12 A3001")).toBe("< 1/4 SM");
    // Trend darf weder Sicht noch Wolken liefern — hier steht im aktuellen
    // Teil keine Sicht, also darf auch keine erscheinen (vorher 3.0 km).
    const tempo = metarAuswertung(t as never, "METAR EDDF 011920Z 18005KT CAVOK 14/12 Q1026 TEMPO 3000 BKN008");
    expect(tempo.find((z) => z.id === "sicht")).toBeUndefined();
    expect(tempo.map((z) => z.v).join(" ")).not.toContain("800 ft");
  });
});
