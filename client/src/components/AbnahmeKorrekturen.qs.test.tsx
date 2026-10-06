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

  it("Druck: Windlinien angehalten (feste Stelle), nicht abgeschaltet", () => {
    const css = readFileSync(resolve(__dirname, "touchdownAbschnitt.css"), "utf-8");
    const druck = css.slice(css.indexOf("@media print"));
    expect(druck).toMatch(/\.windflow__streak\s*\{\s*animation-play-state:\s*paused/);
  });

  it("kein „-0“ in Werten (gemeinsame Formatierung)", async () => {
    const { fmtNumber, fmtSigned } = await import("../lib/landungsFormat");
    expect(fmtSigned(-0.3, 0, "m")).toBe("0 m");
    expect(fmtNumber(-0.04, 1)).toBe("0.0");
    expect(fmtSigned(-0.6, 0)).toBe("-1");
    expect(fmtSigned(2, 0)).toBe("+2");
  });

  it("Druck: Windlinien abgedunkelt und deckend (Farbe im stroke-Attribut)", () => {
    const css = readFileSync(resolve(__dirname, "..", "App.css"), "utf-8");
    const druck = css.slice(css.indexOf("@media print"));
    expect(druck).toMatch(/\.landing-report \.windflow__streak\s*\{[^}]*filter:\s*brightness\(0\.55\)[^}]*opacity:\s*0\.85/);
  });

  it("kein Text prüft die Konfiguration „bei 1000 ft“ (de/en/it)", () => {
    for (const l of ["de", "en", "it"]) {
      const roh = readFileSync(resolve(__dirname, "..", "locales", l, "common.json"), "utf-8");
      expect(roh).not.toMatch(/Landekonfiguration bei 1000 ft|landing configuration at 1000 ft|configurazione d.atterraggio a 1000 ft|Klappen bei 1000 ft|flaps not in landing position at 1000 ft/i);
    }
  });
});
