// QS 06.10.2026 (Runde 10): (1) Der geplante Trip-Verbrauch steht auch dann
// da, wenn der Ist-Verbrauch fehlt (der alte Bericht zeigte „Trip · Plan"
// immer). (2) Der Hilfe-Dialog der Anflugkarte trägt an Altbestand denselben
// Versionshinweis wie jedes Erklärfenster.
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import type { LandingRecord } from "../lib/landungsDatensatz";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { LadeblattAbschnitt } from "./LadeblattAbschnitt";
import { ApproachStabilityHilfeInhalt } from "./ApproachStabilityHilfeInhalt";
import { RunwayUtilizationHilfeInhalt } from "./RunwayUtilizationHilfeInhalt";
import { AltbestandKontext } from "./InfoBadge";

describe("Trip · Plan ohne Ist-Verbrauch", () => {
  it("der Plan-Wert steht da, wenn der Ist-Wert fehlt", () => {
    const r = {
      ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord),
      sprit: null,
      planned_burn_kg: 4321,
      actual_trip_burn_kg: null,
    } as LandingRecord;
    const text = render(<LadeblattAbschnitt record={r} />).container.textContent ?? "";
    expect(text).toMatch(/Plan\s*4[.,  ]?321\s*kg/);
  });
});

describe("Hilfe-Dialog der Anflugkarte", () => {
  it("Altbestand mit Hinweis, Version 19 ohne", () => {
    const alt = render(
      <AltbestandKontext.Provider value={true}>
        <ApproachStabilityHilfeInhalt />
      </AltbestandKontext.Provider>,
    ).container.textContent ?? "";
    expect(alt).toContain("vor Score-Version 19 bewertet");
    const neu = render(<ApproachStabilityHilfeInhalt />).container.textContent ?? "";
    expect(neu).not.toContain("vor Score-Version 19 bewertet");
  });
});

describe("Hilfe-Dialog der Bahn-Auslastung (Runde 11)", () => {
  it("Altbestand mit Hinweis, sonst ohne", () => {
    const alt = render(
      <AltbestandKontext.Provider value={true}>
        <RunwayUtilizationHilfeInhalt />
      </AltbestandKontext.Provider>,
    ).container.textContent ?? "";
    expect(alt).toContain("vor Score-Version 19 bewertet");
    const neu = render(<RunwayUtilizationHilfeInhalt />).container.textContent ?? "";
    expect(neu).not.toContain("vor Score-Version 19 bewertet");
  });
});
