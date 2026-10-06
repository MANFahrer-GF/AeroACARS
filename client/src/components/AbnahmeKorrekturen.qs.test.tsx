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
    expect(text).toContain("im TDZ-Marker ✓");
    expect(text).not.toContain("Touchdown-Zone im TDZ-Marker");
    expect(text).toContain("Aim-Point Δ -42 m");
    expect(text).toContain("Geometrie unsicher");
  });

  it("Werttexte der Teilnoten in der Sprache der Oberfläche — echte Kette ab Datensatz", async () => {
    const { subScoresAusDatensatz } = await import("./LandungsBewertung");
    const r = {
      ...basis(),
      sub_scores: [
        { key: "abfangen", points: 100, band: "good", value: "14.0 s ab 50 ft", messwert: 14,
          value_key: "landing.abfangen.teil_wert", value_params: { s: "14.0" }, label_key: "landing.sub.abfangen" },
        { key: "touchdown_point", points: 0, band: "bad", value: "12 m vor der Schwelle",
          value_key: "landing.wert.td_vor_schwelle", value_params: { m: "12" }, label_key: "landing.sub.touchdown_point" },
      ],
    } as unknown as LandingRecord;
    await i18n.changeLanguage("en");
    const en = render(<ScoreBreakdown subs={subScoresAusDatensatz(r)} record={r} />).container.textContent ?? "";
    await i18n.changeLanguage("de");
    expect(en).toContain("14.0 s from 50 ft");
    expect(en).toContain("12 m before the threshold");
    expect(en).not.toMatch(/ab 50 ft|vor der Schwelle/);
  });

  it("Bahn-Warnung im Druck: eigene helle Regel, nicht unter dem Abdunkel-Filter", () => {
    const r = {
      ...basis(),
      runway_geometry_trusted: false,
      runway_geometry_reason: "icao_mismatch",
      runway_match: { airport_ident: "EDDF", runway_ident: "25C", length_ft: 13123 },
    } as unknown as LandingRecord;
    const w = render(<BahnUeberschrift record={r} />).container.querySelector<HTMLElement>(".bahn-warnung");
    expect(w).not.toBeNull();
    const css = readFileSync(resolve(__dirname, "..", "App.css"), "utf-8");
    const druck = css.slice(css.indexOf("@media print")).replace(/\/\*[\s\S]*?\*\//g, "");
    expect(druck).toMatch(/\.landing-report \.bahn-warnung\s*\{[^}]*background:\s*#fff7e6 !important/);
    const filterSel = [...druck.matchAll(/([^{}]+)\{[^{}]*filter:\s*brightness\(0\.55\)/g)]
      .flatMap((m) => m[1]!.split(",").map((x) => x.trim()).filter(Boolean));
    expect(filterSel.some((x) => w!.matches(x))).toBe(false);
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

  it("Werttext: unbekannter Schlüssel zeigt den gespeicherten Text, nie den Schlüssel; Sprit übersetzt", async () => {
    const { subScoresAusDatensatz } = await import("./LandungsBewertung");
    const r = {
      ...basis(),
      sub_scores: [
        { key: "fuel", points: 100, band: "good", value: "+2.0% bewertet · roh +13.5% · 72 NM Mehrweg (−110 kg)",
          value_key: "landing.wert.sprit_mehrweg", value_params: { kern: "+2.0%", roh: "+13.5%", nm: "72", kg_mehrweg: "110" },
          label_key: "landing.sub.fuel" },
        { key: "loadsheet", points: 100, band: "good", value: "Plan · ZFW 61234 / TOW 70747 kg",
          value_key: "landing.wert.gibt_es_nicht", value_params: {}, label_key: "landing.sub.loadsheet" },
      ],
    } as unknown as LandingRecord;
    await i18n.changeLanguage("en");
    const en = render(<ScoreBreakdown subs={subScoresAusDatensatz(r)} record={r} />).container.textContent ?? "";
    await i18n.changeLanguage("de");
    expect(en).toContain("+2.0% scored · raw +13.5% · 72 NM extra distance (−110 kg)");
    expect(en).toContain("Plan · ZFW 61234 / TOW 70747 kg");
    expect(en).not.toContain("landing.wert.");
  });

  it("Rohdaten: Richtungswort passt zur angezeigten (gerundeten) Zahl", async () => {
    const { RohdatenAbschnitt } = await import("./RohdatenAbschnitt");
    const b = basis();
    const r = { ...b, runway_match: { ...(b.runway_match ?? {}), centerline_distance_m: -0.004 } } as unknown as LandingRecord;
    const text = render(<RohdatenAbschnitt record={r} />).container.textContent ?? "";
    expect(text).toContain("0.00 m (Mitte");
    expect(text).not.toContain("0.00 m (links");
  });

  it("Verzögerung statt Bremsenergie (2.0.1): Wert in m/s², Farbe gewichtsunabhängig", async () => {
    const { LandingQualitaet, tonVerzoegerung } = await import("./LandingQualitaet");
    const r = { ...basis(), landing_decel_mps2: 1.62, landing_brake_energy_proxy: 450 } as unknown as LandingRecord;
    const text = render(<LandingQualitaet record={r} />).container.textContent ?? "";
    expect(text).toContain("Verzögerung");
    expect(text).toContain("1.6");
    expect(text).toContain("m/s²");
    expect(text).not.toMatch(/Bremsenergie|kJ\/m/);
    expect([1.6, 2.4].map(tonVerzoegerung)).toEqual(["good", "good"]);
    expect([3.0, 4.0, 5.0].map(tonVerzoegerung)).toEqual(["neutral", "warn", "err"]);
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
