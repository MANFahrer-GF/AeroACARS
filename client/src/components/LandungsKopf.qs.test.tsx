// QS 06.10.2026: Kopf der Landungsseite (gespiegelt, LandungsBewertung.tsx).
import { describe, expect, it } from "vitest";
import { render } from "@testing-library/react";
import { LandingDetail, type LandingRecord } from "./LandingPanel";
import { OffAirportBanner, QuickFlags } from "./LandungsBewertung";
import { MOCK_LANDING_OPTIONS } from "../dev/mockLandingRecords";
import { LANDUNGS_FAELLE } from "../lib/landungsFaelle";

function basis(): LandingRecord {
  return MOCK_LANDING_OPTIONS[0]!.build() as unknown as LandingRecord;
}

describe("Kopf der Landungsseite", () => {
  it("Flugzeugzeile auch ohne Titel — Kennzeichen, Muster, Simulator", () => {
    const r = { ...basis(), aircraft_title: null, aircraft_registration: "TC-LGA", aircraft_icao: "A359", sim_kind: "msfs" } as LandingRecord;
    const { container } = render(<LandingDetail record={r} allRecords={[r]} onBack={() => {}} />);
    expect(container.querySelector(".landing-headline__aircraft")?.textContent).toBe("TC-LGA · A359 · MSFS");
  });

  it("Simulator einheitlich geschrieben (Client X-PLANE, Recorder xplane)", () => {
    for (const sim of ["X-PLANE", "xplane"]) {
      const r = { ...basis(), aircraft_title: "Zibo 738", aircraft_registration: null, aircraft_icao: null, sim_kind: sim } as LandingRecord;
      const { container, unmount } = render(<LandingDetail record={r} allRecords={[r]} onBack={() => {}} />);
      expect(container.querySelector(".landing-headline__aircraft")?.textContent).toBe("Zibo 738 · X-Plane");
      unmount();
    }
  });

  it("kein Divert-Banner ohne geplanten Platz", () => {
    const r = { ...basis(), touchdown_airport: "EDDF", arr_airport: "", touchdown_airport_source: "runway_match" } as LandingRecord;
    expect(render(<OffAirportBanner record={r} />).container.textContent).toBe("");
    // Gegenprobe: mit anderem geplanten Platz erscheint es.
    const d = { ...r, arr_airport: "EDDM" } as LandingRecord;
    expect(render(<OffAirportBanner record={d} />).container.textContent).toContain("EDDM");
  });

  it("bei Unfall die Marke UNFALL statt harter Landung", () => {
    const hart = { ...basis(), score_algorithm_version: 17, landing_rate_fpm: -900, vs_at_edge_fpm: -900, landing_peak_vs_fpm: -900 } as LandingRecord;
    const marken = (r: LandingRecord) =>
      [...render(<QuickFlags record={r} />).container.querySelectorAll(".landing-flag")].map((e) => e.textContent);
    // Gegenprobe: ohne Unfall steht die harte Landung da.
    expect(marken(hart)).toContain("HARTE LANDUNG");
    const unfall = marken({ ...hart, accident: true } as LandingRecord);
    expect(unfall[0]).toBe("UNFALL");
    expect(unfall).not.toContain("HARTE LANDUNG");
  });

  it("Anflugkarte mit Stable Gate behält ihren Coach-Satz", () => {
    const f = LANDUNGS_FAELLE[0]!;
    const r = {
      ...basis(),
      score_numeric: f.landing_score,
      score_algorithm_version: f.score_algorithm_version,
      ux_version: 1,
      sub_scores: f.sub_scores,
    } as unknown as LandingRecord;
    const { getByTestId } = render(<LandingDetail record={r} allRecords={[r]} onBack={() => {}} />);
    expect(getByTestId("gate-kacheln")).toBeTruthy();
    expect(getByTestId("gate-coach").textContent!.length).toBeGreaterThan(20);
  });

  it("Note ohne gespeichertes Wort: nur die Zahl, kein erfundenes Wort", () => {
    const r = { ...basis(), score_numeric: 95, score_label: null, score_algorithm_version: 17 } as unknown as LandingRecord;
    const { container } = render(<LandingDetail record={r} allRecords={[r]} onBack={() => {}} />);
    const kopf = container.querySelector(".landing-headline")!.textContent ?? "";
    expect(kopf).toContain("95/100");
    expect(kopf).not.toMatch(/FEST|GUT|HERVORRAGEND|AUSREICHEND/i);
    // Gegenprobe: mit gespeichertem Wort steht es da.
    const mit = { ...r, score_label: "smooth" } as unknown as LandingRecord;
    const m = render(<LandingDetail record={mit} allRecords={[mit]} onBack={() => {}} />);
    expect(m.container.querySelectorAll(".landing-headline")[0]!.textContent).not.toBe(kopf);
  });
});
