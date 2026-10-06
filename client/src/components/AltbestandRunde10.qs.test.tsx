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
    // Runde 12: die Achse gibt es nur im Altbestand — eigener Hinweis, nicht
    // „beschreibt die heutigen Regeln".
    expect(alt).toContain("gibt es nur bei Landungen vor Score-Version 19");
    expect(alt).not.toContain("beschreibt die heutigen Regeln");
    const neu = render(<RunwayUtilizationHilfeInhalt />).container.textContent ?? "";
    expect(neu).not.toContain("Score-Version 19");
  });
});

describe("Runde 12", () => {
  it("Rohdaten: Abstand hinter der Schwelle wie Aufsetz-Qualität (Rückfall vor v1.7.15)", async () => {
    const { RohdatenAbschnitt } = await import("./RohdatenAbschnitt");
    const basis = MOCK_LANDING_OPTIONS[0]!.build() as unknown as Record<string, unknown>;
    const { td_distance_from_threshold_m: _weg, ...ohneFeld } = basis;
    const r = { ...ohneFeld, landing_float_distance_m: 420 } as unknown as LandingRecord;
    const text = render(<RohdatenAbschnitt record={r} />).container.textContent ?? "";
    expect(text).toContain("+420.00 m");
  });
  it("Sprit-Teilnote im Altbestand: Hinweis zur alten Achse", async () => {
    const { ScoreBreakdown } = await import("./LandungsBewertung");
    const { fireEvent } = await import("@testing-library/react");
    const r = { ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord), score_algorithm_version: 12 };
    const teil = { key: "fuel", points: 90, value: "+3 %", band: "good", rationale: "" };
    const { container } = render(
      <AltbestandKontext.Provider value={true}>
        <ScoreBreakdown subs={[teil as never]} record={r} />
      </AltbestandKontext.Provider>,
    );
    fireEvent.click(container.querySelector(".info-badge") as HTMLElement);
    const f = document.querySelector(".info-badge__popover")?.textContent ?? "";
    expect(f).toContain("gibt es nur bei Landungen vor Score-Version 19");
    expect(f).not.toContain("beschreibt die heutigen Regeln");
  });
});

describe("Runde 13", () => {
  it("Gleitpfad aufgezeichnet, aber ohne Zeitbezug zur Spur: Hinweis statt nichts", async () => {
    const { AnflugGrafikAbschnitt } = await import("./AnflugGrafik");
    // Spur ohne t_ms → der Streifen kann nicht platziert werden.
    const samples = Array.from({ length: 10 }, (_, k) => ({ vs_fpm: -700, bank_deg: 0, agl_ft: 1000 - k * 90 }));
    const verlauf = [{ t: -60, h: 900, d: 0.2 }, { t: -30, h: 500, d: -0.4 }];
    const text = render(
      <AnflugGrafikAbschnitt samples={samples as never} profile={null} gleitpfadVerlauf={verlauf as never} />,
    ).container.textContent ?? "";
    expect(text).toContain("ohne gemeinsame Zeitachse");
    expect(text).not.toContain("nicht aufgezeichnet");
  });
  it("Nicht bewertete Bahndisziplin hat ihre Erklärung", async () => {
    const { ScoreBreakdown } = await import("./LandungsBewertung");
    const r = { ...(MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord), score_algorithm_version: 19 };
    const teil = { key: "rollout", label_key: "landing.sub.runway_discipline", points: 0, value: "—", band: "skipped", rationale: "", skipped: true };
    const { container } = render(<ScoreBreakdown subs={[teil as never]} record={r} />);
    expect(container.querySelector(".info-badge")).not.toBeNull();
  });
});
