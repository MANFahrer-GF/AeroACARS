// Abnahme 06.10.2026: Was der Auftrag verlangt und die Schlussrunde als
// „Kosmetik" stehen ließ — jetzt behoben und hier festgehalten.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import i18n from "../i18n";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { BahnUeberschrift, ScoreBreakdown } from "./LandungsBewertung";
import { TouchdownAbschnitt } from "./TouchdownAbschnitt";
import { metarAuswertung } from "./MetarAbschnitt";

const basis = () => MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord;

describe("Abnahme-Korrekturen", () => {
  it("unsichere Geometrie (auch falscher Platz): Werte des alten Berichts mit Vermerk", () => {
    const r = {
      ...basis(),
      runway_geometry_trusted: false,
      runway_geometry_reason: "icao_mismatch",
      runway_match: { airport_ident: "EDDF", runway_ident: "25C", length_ft: 13123, surface: "ASP" },
      td_in_tdz: true,
      aim_delta_m: -42,
    } as unknown as LandingRecord;
    const text = render(<BahnUeberschrift record={r} />).container.textContent ?? "";
    expect(text).toContain("EDDF 25C");
    expect(text).toContain("4000 m");
    expect(text).toContain("Touchdown-Zone im TDZ-Marker");
    expect(text).toContain("Aim-Point Δ -42 m");
    expect(text).toContain("Geometrie unsicher");
  });

  it("Abfangen-Wert in der Sprache der Oberfläche", async () => {
    const teil = { key: "abfangen", points: 100, value: "14.0 s ab 50 ft", band: "good", rationale: "", messwert: 14 };
    await i18n.changeLanguage("en");
    const en = render(<ScoreBreakdown subs={[teil as never]} record={basis()} />).container.textContent ?? "";
    await i18n.changeLanguage("de");
    expect(en).toContain("14.0 s from 50 ft");
    expect(en).not.toContain("ab 50 ft");
  });

  it("nur Seitenwind gemessen: Gegenwind ausdrücklich unbekannt", () => {
    const r = { ...basis(), headwind_kt: null, crosswind_kt: 8 } as LandingRecord;
    const t = render(<TouchdownAbschnitt record={r} />).container.querySelector(".windflow")!.textContent ?? "";
    expect(t).toContain("Gegenwind —");
    expect(t).not.toContain("Gesamtwind");
  });

  it("METAR: Sicht in Meilen und Wettercodes getrennt", () => {
    const tt = (k: string) => ({ "landing.metar.wx.SH": "Schauer", "landing.metar.wx.RA": "Regen", "landing.metar.leicht": "leicht", "landing.metar.sicht": "Sicht" })[k] ?? k;
    const z = metarAuswertung(tt as never, "METAR KJFK 011951Z 18005KT 1 1/2SM -SHRA BKN010 14/12 A3001");
    const text = z.map((x) => `${x.k}=${x.v}`).join(" | ");
    expect(text).toContain("Sicht=1 1/2 SM");
    expect(text).toContain("leicht Schauer Regen");
  });

  it("Druck: Windlinien angehalten (feste Stelle), nicht abgeschaltet", () => {
    const css = readFileSync(resolve(__dirname, "touchdownAbschnitt.css"), "utf-8");
    const druck = css.slice(css.indexOf("@media print"));
    expect(druck).toMatch(/\.windflow__streak\s*\{\s*animation-play-state:\s*paused/);
  });
});
